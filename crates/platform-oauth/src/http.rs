//! HTTP 层：平台端点 + 协议端点 + 路由（REQ-022，design §3）。
//!
//! # ⚠️ 两套错误体，**两套路由**（契约 C14 §14.2）
//!
//! | 端点族 | 路径（相对 `/api/oauth`） | 错误体 |
//! |--------|------------------------|--------|
//! | **平台端点** | `/authorize/*` `/clients/*` `/grants/*` `/logout` `/device/*` | RFC 9457 Problem Details + `OAUTH_*` |
//! | **协议端点** | `/token` `/device_authorization` `/introspect` `/revoke` | `{"error":"invalid_grant"}` 小写 |
//!
//! # ⚠️ 字段名：平台端点 **camelCase**，协议端点用 **RFC 的 snake_case**（D258）
//!
//! - 平台端点的请求 / 响应字段逐字见 `decisions.md` **D258**；
//! - 协议端点的字段名由 RFC 定死（`access_token` / `token_type` / `expires_in` /
//!   `scope` / `device_code` / `user_code` / `verification_uri` / …）。
//!
//! # ⚠️ 缺键与空串**不是一回事**
//!
//! `preview` 的请求体里**缺键** = 「没传」（`Option<T>` + `#[serde(default)]`）；
//! **显式传空串** = 「传了但是空的」→ 两者**必须**能区分开
//! （棒 2 的约定：`put()` 只在值非 `undefined` 时写键）。
//!
//! # ⚠️ 认证的**三种**形态
//!
//! | 端点族 | 认证 |
//! |--------|------|
//! | 平台端点 | **用户会话**（`Bearer` + 平台登录令牌）→ 租户与用户都来自令牌 |
//! | 协议端点 | **client 认证**（`client_secret_basic` / `none`） |
//! | 平台**管理**端点（`clients/*` / `grants/*`） | 用户会话 **+** 权限点（`require_permission`） |

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{FromRequest, OriginalUri, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use platform_core::RequestContext;
use platform_core::extension::permission::{PermissionChecker, PermissionRefresher};
use platform_core::problem::{ProblemDetails, problem_response};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::authz;
use crate::client_auth;
use crate::error::ServiceError;
use crate::protocol::ProtocolError;
use crate::service::{AuthenticatedUser, PreviewRequest, RegisterClientRequest};

/// HTTP 层共享的状态。
#[derive(Clone)]
pub struct OAuthState {
    /// 业务服务。
    pub service: Arc<crate::service::OAuthService>,
    /// 权限判定器（鉴权层从请求扩展里取）。
    pub checker: Arc<dyn PermissionChecker>,
    /// 权限判定的**异步刷新**接缝。
    ///
    /// ⚠️ **不能省**：`PermissionChecker::has` 是**纯缓存查询**
    /// （`RbacChecker::has` 在缓存里没有该用户时 **fail closed** 直接 false），
    /// 而填缓存是**异步**的 `ensure_fresh`。少了它 → 用户第一次打本模块的
    /// 受权限保护的端点**必然 403 `INSUFFICIENT_PERMISSION`**，
    /// 第二次才行（缓存被别的请求填上了）——这种「第一次失败、重试就好」的
    /// 缺陷最难查（终版 E2E 就抓到了它）。
    ///
    /// 范式同 `platform-rbac/src/http.rs` 与 `platform-config` 的中间件。
    pub refresher: Arc<dyn PermissionRefresher>,
}

impl std::fmt::Debug for OAuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthState").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// 提取器
// ---------------------------------------------------------------------------

/// 带租户上下文的请求上下文（**外加**调用方请求的原始路径）。
///
/// ⚠️ `instance` 必须是**调用方请求的路径**（含 `/api/oauth` 前缀）——
/// `Router::nest` 会剥掉前缀 → 只能从 `OriginalUri` 扩展里取。
#[derive(Debug, Clone)]
struct Ctx {
    /// 请求上下文（租户 / 用户 / IP / UA）。
    ctx: RequestContext,
    /// 调用方请求的原始路径。
    instance: String,
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Ctx {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        let instance = original_path(&parts.extensions, "/api/oauth");
        Ok(Ctx { ctx, instance })
    }
}

/// 已认证的调用方（用户会话）。
///
/// ⚠️ 由**认证中间件**写进请求扩展 → 这里只读。
#[derive(Debug, Clone)]
struct Caller(AuthenticatedUser);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Caller {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        let instance = original_path(&parts.extensions, "/api/oauth");
        let Some(user) = parts.extensions.get::<AuthenticatedUser>().cloned() else {
            return Err(problem_response(&ProblemDetails::unauthenticated(
                &ctx, &instance,
            )));
        };
        Ok(Caller(user))
    }
}

/// 平台端点的 JSON 请求体。
///
/// ⚠️ **解析失败也要回平台的错误体** —— axum 默认的拒绝体是纯文本，
/// 那会让「错误体形状」在两端不一致。
struct ApiJson<T>(T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let ctx = req
            .extensions()
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        let instance = original_path(req.extensions(), req.uri().path());
        let bytes = match axum::body::to_bytes(req.into_body(), 256 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => {
                return Err(invalid_request_response(&ctx, &instance));
            }
        };
        match serde_json::from_slice::<T>(&bytes) {
            Ok(value) => Ok(ApiJson(value)),
            Err(_) => Err(invalid_request_response(&ctx, &instance)),
        }
    }
}

/// 协议端点的 **form** 请求体（`application/x-www-form-urlencoded`）。
///
/// ⚠️ **自己解析**（不用 axum 的 `Form` 提取器）—— axum 的拒绝体是纯文本，
/// 而协议端点必须回 `{"error": "invalid_request"}`（契约 C14 §14.2）。
struct ProtocolForm(BTreeMap<String, String>);

impl<S: Send + Sync> FromRequest<S> for ProtocolForm {
    type Rejection = Response;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => return Err(ProtocolError::InvalidRequest.bare_response()),
        };
        let text = String::from_utf8_lossy(&bytes);
        Ok(ProtocolForm(parse_form(&text)))
    }
}

/// 读一个 form 值（**缺键**与**空串**都返回 `None` —— 协议端点不区分二者）。
fn form_value<'a>(form: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    form.get(key).map(String::as_str).filter(|v| !v.is_empty())
}

/// 协议端点的**租户上下文**。
///
/// # 为什么协议端点要自己定租户
///
/// 协议端点（`/token` / `/device_authorization` / `/introspect` / `/revoke`）
/// **没有用户会话**（RFC 6749：调用方是 client 的后端服务）→ 它**不知道**租户。
///
/// → 协议端点的租户**一律**是 OAuth 自己的保留伪租户；真正的租户由**授权凭据**
/// 决定（服务层会**覆盖**这个租户）：
///
/// | 端点 | 租户从哪来 |
/// |------|-----------|
/// | `/token`（`authorization_code`） | 授权码行的 `tenant_id`（D245） |
/// | `/token`（`device_code`） | 设备码行的 `tenant_id`（**批准时**绑定） |
/// | `/token`（`client_credentials`） | **无用户、无授权** → 平台级（D219） |
/// | `/introspect` / `/revoke` | 令牌 JWT 的 `tid` 声明（D245） |
/// | `/device_authorization` | 批准前**还没有**租户 → 平台级 |
///
/// ⚠️ **不是** `platform-config` 保留的 `"platform"`（D72）：用它读配置会被
/// `ensure_tenant_not_reserved` 拒掉 → 协议端点读不到 OAuth 配置 → 500。
fn protocol_ctx(ctx: &RequestContext) -> RequestContext {
    ctx.clone().with_tenant(crate::store::OAUTH_PLATFORM_TENANT)
}

// ---------------------------------------------------------------------------
// 路由
// ---------------------------------------------------------------------------

/// 平台端点 + 协议端点的路由（由装配层 **nest** 到 `/api/oauth`）。
pub fn router<S2>(state: Arc<OAuthState>) -> Router<S2> {
    // 授权（用户会话）。
    let authorize = Router::new()
        .route("/authorize/preview", post(preview))
        .route("/authorize/confirm", post(confirm))
        .route("/authorize/deny", post(deny));

    // client 管理（用户会话 + **逐端点**的权限点）。
    //
    // ⚠️ **每个端点只挂一个权限点**（design §3.2 的逐端点表）。
    //
    // ⚠️ **不能**在 `Router` 上叠多个 `route_layer` —— axum 的 `Router::route_layer`
    // 是**叠加**语义（多次调用 = 多层全跑）→ `GET /clients` 会同时要求
    // `oauth:client:read` **AND** `oauth:client:write` **AND** `oauth:client:delete`
    // → 只持读权限的管理员被拒。
    //
    // → 改成**逐 method** 挂层：`MethodRouter::route_layer` 只作用于**已设置**的
    // method；而 `Router::route` 对**同一路径**是**按 method 合并**的
    // （axum 的 `path_router.route` → `merge_for_path`）→ 每个 method 各自保留自己的层。
    let clients = Router::new()
        .route(
            "/clients",
            get(list_clients).route_layer(authz::require_client_read()),
        )
        .route(
            "/clients",
            post(register_client).route_layer(authz::require_client_write()),
        )
        .route(
            "/clients/{client_id}",
            get(get_client).route_layer(authz::require_client_read()),
        )
        .route(
            "/clients/{client_id}",
            patch(update_client).route_layer(authz::require_client_write()),
        )
        .route(
            "/clients/{client_id}",
            delete(delete_client).route_layer(authz::require_client_delete()),
        );

    // 授权记录（用户会话 + **逐端点**的权限点）。
    //
    // ⚠️ 数据范围**永远**限定在当前用户（见 `service::list_grants` /
    // `service::revoke_grant`）—— 即便调用者持有 `oauth:grant:revoke`，
    // 也**只能**撤销**自己**的授权（AC-S-147 步骤 5）。
    let grants = Router::new()
        .route(
            "/grants",
            get(list_grants).route_layer(authz::require_grant_read()),
        )
        .route(
            "/grants/revoke",
            post(revoke_grant).route_layer(authz::require_grant_revoke()),
        );

    // 登出（用户会话）。
    let logout = Router::new().route("/logout", post(logout));

    // 设备码（D257：批准前展示 + 批准 / 拒绝）。
    let device = Router::new()
        .route("/device/verify", get(verify_device))
        .route("/device/approve", post(approve_device));

    // 平台端点：统一走**用户会话**中间件（它顺带注入权限判定器）。
    let platform = Router::new()
        .merge(authorize)
        .merge(clients)
        .merge(grants)
        .merge(logout)
        .merge(device)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ));

    // 协议端点：**不走**用户会话中间件（它们是 **client 认证**）。
    let protocol = Router::new()
        .route("/token", post(token))
        .route("/device_authorization", post(device_authorization))
        .route("/introspect", post(introspect))
        .route("/revoke", post(revoke));

    Router::new()
        .merge(platform)
        .merge(protocol)
        .with_state(state)
}

/// 授权服务器元数据的路由（挂在根路径 `/.well-known/oauth-authorization-server`）。
pub fn metadata_router<S2>(state: Arc<OAuthState>) -> Router<S2> {
    Router::new()
        .route("/.well-known/oauth-authorization-server", get(metadata))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// 认证中间件（平台端点）
// ---------------------------------------------------------------------------

/// 用户会话中间件：验登录令牌 → 写入租户 / 用户上下文 → 注入权限判定器。
///
/// **必须**跑在 `require_permission` 之前 —— 它是路由级层的**外层**。
async fn authenticate(
    State(state): State<Arc<OAuthState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let instance = original_path(request.extensions(), request.uri().path());
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    let Some(token) = bearer_token(request.headers()) else {
        return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
    };
    let user = match state.service.authenticate_bearer(&token) {
        Ok(user) => user,
        Err(_) => return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance)),
    };

    // 租户与用户**都来自令牌**（平台会话是 Bearer + sessionStorage）。
    ctx = ctx.with_tenant(user.tenant_id.clone());
    ctx = ctx.with_user(user.user_id.clone());
    // clone 而不是 move：下面刷新判定缓存还要用 `ctx`（`ensure_fresh` 要它）。
    request.extensions_mut().insert(ctx.clone());
    // 会话 ID（`sid`）也要带下去 —— 登出的 `logout_token` 要用（D230）。
    request.extensions_mut().insert(user);

    // ⚠️ **必须**先刷新判定缓存，再让鉴权层判定。
    //
    // `PermissionChecker::has` 是**纯缓存查询**（`RbacChecker::has` 在缓存里
    // 没有该用户时 fail closed 直接 false），而填缓存是异步的 `ensure_fresh`。
    // 漏掉这一步 → 用户第一次打受保护的端点**必然 403**，重试才好 ——
    // 这正是终版 E2E 抓到的缺陷（`POST /api/oauth/clients` 第一次 403、
    // 手工重试 201）。
    if let Err(reason) = state.refresher.ensure_fresh(&ctx).await {
        // fail closed：刷新不出来就**不能**让鉴权层读到空缓存（那会一律 403，
        // 看起来像「没权限」而不是「后端坏了」）。
        tracing::error!(%reason, "刷新权限判定缓存失败");
        let details = ProblemDetails::from_context(
            "INTERNAL_ERROR",
            platform_core::titles::title_of_code("INTERNAL_ERROR"),
            500,
            "failed to refresh the permission cache",
            &ctx,
            instance.as_str(),
            None,
        );
        return problem_response(&details);
    }

    // 注入权限判定实现（鉴权层从扩展里取）。
    let checker: Arc<dyn PermissionChecker> = state.checker.clone();
    request.extensions_mut().insert(checker);

    next.run(request).await
}

/// 从 `Authorization: Bearer <token>` 里取令牌。
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let trimmed = raw.trim();
    let token = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// 请求的**原始路径**（含 `/api/oauth` 前缀）。
///
/// `Router::nest` 会**剥掉前缀** → `uri().path()` 不是调用方请求的路径；
/// 错误体的 `instance` 必须是**调用方请求的路径**。
fn original_path(extensions: &axum::http::Extensions, fallback: &str) -> String {
    extensions
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

// ---------------------------------------------------------------------------
// 平台端点：授权
// ---------------------------------------------------------------------------

/// `preview` 的请求体（**camelCase**，D258）。
///
/// ⚠️ 每个字段都是 `Option<T>` + `#[serde(default)]`：
/// **缺键** = 没传；**显式空串** = 传了但是空的 → 两者**必须**能区分。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewBody {
    /// `response_type`（**必填**）。
    #[serde(default)]
    response_type: Option<String>,
    /// client ID（**必填**）。
    #[serde(default)]
    client_id: Option<String>,
    /// 回调地址。
    #[serde(default)]
    redirect_uri: Option<String>,
    /// 请求的 scope（**空格分隔的字符串**）。
    #[serde(default)]
    scope: Option<String>,
    /// `state`。
    #[serde(default)]
    state: Option<String>,
    /// PKCE 挑战（**必填**）。
    #[serde(default)]
    code_challenge: Option<String>,
    /// PKCE 挑战方法（**必填**，恒为 `S256`）。
    #[serde(default)]
    code_challenge_method: Option<String>,
    /// `prompt`（`none` = 不许交互；**缺省 = 允许交互**，AC-S-148 步骤 5）。
    #[serde(default)]
    prompt: Option<String>,
}

/// 校验授权请求 → 同意票据（或免登直发的跳转地址）。
async fn preview(
    State(state): State<Arc<OAuthState>>,
    Caller(caller): Caller,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<PreviewBody>,
) -> Response {
    let request = PreviewRequest {
        response_type: body.response_type,
        client_id: body.client_id,
        redirect_uri: body.redirect_uri,
        scope: body.scope,
        state: body.state,
        code_challenge: body.code_challenge,
        code_challenge_method: body.code_challenge_method,
        prompt: body.prompt,
    };
    let user = &caller;
    match state.service.preview(&ctx, &user, &request).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// `confirm` 的请求体。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TicketBody {
    /// 同意票据（JWT）。
    #[serde(default)]
    ticket: Option<String>,
}

/// 用户点「允许」→ 签发授权码。
async fn confirm(
    State(state): State<Arc<OAuthState>>,
    Caller(caller): Caller,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<TicketBody>,
) -> Response {
    let user = &caller;
    let Some(ticket) = body.ticket.as_deref().filter(|value| !value.is_empty()) else {
        return invalid_request_response(&ctx, instance.as_str());
    };
    match state.service.confirm(&ctx, &user, ticket).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 用户点「拒绝」→ 跳回 `redirect_uri?error=access_denied&state=…`。
async fn deny(
    State(state): State<Arc<OAuthState>>,
    Caller(caller): Caller,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<TicketBody>,
) -> Response {
    let user = &caller;
    let Some(ticket) = body.ticket.as_deref().filter(|value| !value.is_empty()) else {
        return invalid_request_response(&ctx, instance.as_str());
    };
    match state.service.deny(&ctx, &user, ticket).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// 平台端点：client 管理
// ---------------------------------------------------------------------------

/// 注册 client 的请求体（**camelCase**，D258）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterClientBody {
    /// 展示名。
    #[serde(default)]
    name: String,
    /// client 类型（`confidential` / `public`）。
    #[serde(default)]
    client_type: String,
    /// 回调白名单。
    #[serde(default)]
    redirect_uris: Vec<String>,
    /// 允许的 grant type。
    #[serde(default)]
    allowed_grant_types: Vec<String>,
    /// 允许的 scope。
    #[serde(default)]
    allowed_scopes: Vec<String>,
    /// back-channel 登出回调。
    #[serde(default)]
    backchannel_logout_uri: Option<String>,
    /// front-channel 登出回调。
    #[serde(default)]
    frontchannel_logout_uri: Option<String>,
    /// 有效期（秒）。
    #[serde(default)]
    expires_in_seconds: Option<i64>,
}

/// 改 client 的请求体。
///
/// ⚠️ 本轮**只支持改 `status`**（停用 / 启用）—— AC-S-135 步骤 4 只验这一项；
/// 其余字段要改就是「扩需求」，不在本轮范围（见报告「未完成的部分」）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateClientBody {
    /// 状态（`active` / `disabled`）。
    #[serde(default)]
    status: Option<String>,
}

/// 列出 client。
async fn list_clients(
    State(state): State<Arc<OAuthState>>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.list_clients(&ctx).await {
        Ok(rows) => Json(rows).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 注册 client → **201 CREATED**（D234：平台创建类端点的统一口径）。
async fn register_client(
    State(state): State<Arc<OAuthState>>,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<RegisterClientBody>,
) -> Response {
    let request = RegisterClientRequest {
        name: body.name,
        client_type: body.client_type,
        redirect_uris: body.redirect_uris,
        allowed_grant_types: body.allowed_grant_types,
        allowed_scopes: body.allowed_scopes,
        backchannel_logout_uri: body.backchannel_logout_uri,
        frontchannel_logout_uri: body.frontchannel_logout_uri,
        expires_in_seconds: body.expires_in_seconds,
    };
    match state.service.register_client(&ctx, &request).await {
        Ok(created) => (StatusCode::CREATED, Json(created)).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 读单个 client。
async fn get_client(
    State(state): State<Arc<OAuthState>>,
    Path(client_id): Path<String>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.get_client(&ctx, &client_id).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 改 client（停用 / 启用）。
async fn update_client(
    State(state): State<Arc<OAuthState>>,
    Path(client_id): Path<String>,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<UpdateClientBody>,
) -> Response {
    let status = match body.status.as_deref().map(str::trim) {
        Some("disabled") => crate::model::ClientStatus::Disabled,
        Some("active") => crate::model::ClientStatus::Active,
        _ => return invalid_request_response(&ctx, instance.as_str()),
    };
    match state
        .service
        .set_client_status(&ctx, &client_id, status)
        .await
    {
        Ok(view) => Json(view).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 删除 client → **204**。
async fn delete_client(
    State(state): State<Arc<OAuthState>>,
    Path(client_id): Path<String>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.delete_client(&ctx, &client_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// 平台端点：授权记录与登出
// ---------------------------------------------------------------------------

/// 「我的授权」列表。
async fn list_grants(
    State(state): State<Arc<OAuthState>>,
    Caller(caller): Caller,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    let user = &caller;
    match state.service.list_grants(&ctx, &user).await {
        Ok(rows) => Json(rows).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 撤销授权（= 按应用登出）的请求体。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevokeGrantBody {
    /// client ID。
    #[serde(default)]
    client_id: Option<String>,
}

/// 撤销授权。
async fn revoke_grant(
    State(state): State<Arc<OAuthState>>,
    Caller(caller): Caller,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<RevokeGrantBody>,
) -> Response {
    let user = &caller;
    let Some(client_id) = body.client_id.as_deref().filter(|value| !value.is_empty()) else {
        return invalid_request_response(&ctx, instance.as_str());
    };
    match state.service.revoke_grant(&ctx, &user, client_id).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 全局登出。
async fn logout(
    State(state): State<Arc<OAuthState>>,
    Caller(caller): Caller,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    let user = &caller;
    match state.service.logout(&ctx, &user).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// 平台端点：设备码（D257）
// ---------------------------------------------------------------------------

/// `device/verify` 的查询参数（**camelCase**：`userCode`，D257）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceVerifyQuery {
    /// 用户短码。
    #[serde(default)]
    user_code: Option<String>,
}

/// 批准**前**展示 client 名与请求的 scope。
async fn verify_device(
    State(state): State<Arc<OAuthState>>,
    Ctx { ctx, instance }: Ctx,
    Query(query): Query<DeviceVerifyQuery>,
) -> Response {
    let Some(user_code) = query.user_code.as_deref().filter(|value| !value.is_empty()) else {
        return invalid_request_response(&ctx, instance.as_str());
    };
    match state.service.verify_device_code(&ctx, user_code).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// `device/approve` 的请求体。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceApproveBody {
    /// 用户短码。
    #[serde(default)]
    user_code: Option<String>,
    /// `approve` / `deny`。
    #[serde(default)]
    action: Option<String>,
}

/// 设备码的批准 / 拒绝。
async fn approve_device(
    State(state): State<Arc<OAuthState>>,
    Caller(caller): Caller,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<DeviceApproveBody>,
) -> Response {
    let user = &caller;
    let Some(user_code) = body.user_code.as_deref().filter(|value| !value.is_empty()) else {
        return invalid_request_response(&ctx, instance.as_str());
    };
    let approve = match body.action.as_deref().map(str::trim) {
        Some("approve") => true,
        Some("deny") => false,
        _ => return invalid_request_response(&ctx, instance.as_str()),
    };
    match state
        .service
        .approve_device_code(&ctx, &user, user_code, approve)
        .await
    {
        Ok(result) => Json(result).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// 协议端点
// ---------------------------------------------------------------------------

/// 令牌端点（RFC 6749 §3.2）。
async fn token(
    State(state): State<Arc<OAuthState>>,
    Ctx { ctx, .. }: Ctx,
    headers: HeaderMap,
    ProtocolForm(form): ProtocolForm,
) -> Response {
    let credentials = match client_auth::parse(
        authorization_header(headers.get(header::AUTHORIZATION)),
        form_value(&form, "client_id"),
    ) {
        Ok(credentials) => credentials,
        Err(error) => return protocol_error_response(&error),
    };
    let client = match state.service.authenticate_client(&credentials, false).await {
        Ok(client) => client,
        Err(error) => return protocol_error_response(&error),
    };

    // 协议端点**没有租户上下文** → 先落 OAuth 的保留伪租户；
    // 真正的租户由**授权凭据**决定（服务层会覆盖）。
    let scoped = protocol_ctx(&ctx);
    let ctx = &scoped;

    let grant_type = form_value(&form, "grant_type")
        .unwrap_or_default()
        .to_string();
    let result = match grant_type.as_str() {
        crate::model::grant_type::AUTHORIZATION_CODE => {
            let code = form_value(&form, "code").unwrap_or_default().to_string();
            let redirect_uri = form_value(&form, "redirect_uri")
                .unwrap_or_default()
                .to_string();
            let verifier = form_value(&form, "code_verifier")
                .unwrap_or_default()
                .to_string();
            state
                .service
                .exchange_authorization_code(&ctx, &client, &code, &redirect_uri, &verifier)
                .await
        }
        crate::model::grant_type::CLIENT_CREDENTIALS => {
            let scope = form_value(&form, "scope").unwrap_or_default().to_string();
            state
                .service
                .exchange_client_credentials(&ctx, &client, &scope)
                .await
        }
        crate::model::grant_type::DEVICE_CODE => {
            let device_code = form_value(&form, "device_code")
                .unwrap_or_default()
                .to_string();
            state
                .service
                .exchange_device_code(&ctx, &client, &device_code)
                .await
        }
        // 不认识的 grant type。
        _ => Err(ProtocolError::UnsupportedGrantType),
    };
    match result {
        Ok(token) => Json(token).into_response(),
        Err(error) => protocol_error_response(&error),
    }
}

/// 设备码申请端点（RFC 8628 §3.1）。
async fn device_authorization(
    State(state): State<Arc<OAuthState>>,
    Ctx { ctx, .. }: Ctx,
    headers: HeaderMap,
    ProtocolForm(form): ProtocolForm,
) -> Response {
    let credentials = match client_auth::parse(
        authorization_header(headers.get(header::AUTHORIZATION)),
        form_value(&form, "client_id"),
    ) {
        Ok(credentials) => credentials,
        Err(error) => return protocol_error_response(&error),
    };
    let client = match state.service.authenticate_client(&credentials, false).await {
        Ok(client) => client,
        Err(error) => return protocol_error_response(&error),
    };
    let scope = form_value(&form, "scope").unwrap_or_default().to_string();
    // 协议端点**没有租户上下文** → 落 OAuth 的保留伪租户
    // （设备码行的租户在**批准**时才绑定到用户的租户）。
    let scoped = protocol_ctx(&ctx);
    let ctx = &scoped;
    match state
        .service
        .device_authorization(&ctx, &client, &scope)
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => protocol_error_response(&error),
    }
}

/// 内省端点（RFC 7662）。
///
/// ⚠️ **必须**是 confidential client（D208）→ `public` client 一律 **401**。
async fn introspect(
    State(state): State<Arc<OAuthState>>,
    Ctx { ctx, .. }: Ctx,
    headers: HeaderMap,
    ProtocolForm(form): ProtocolForm,
) -> Response {
    let credentials = match client_auth::parse(
        authorization_header(headers.get(header::AUTHORIZATION)),
        form_value(&form, "client_id"),
    ) {
        Ok(credentials) => credentials,
        Err(error) => return protocol_error_response(&error),
    };
    let client = match state.service.authenticate_client(&credentials, true).await {
        Ok(client) => client,
        Err(error) => return protocol_error_response(&error),
    };
    let token = form_value(&form, "token").unwrap_or_default().to_string();
    // 协议端点**没有租户上下文** → 落 OAuth 的保留伪租户；
    // 真正的租户由**令牌 JWT 的 `tid` 声明**决定（服务层会覆盖）。
    let scoped = protocol_ctx(&ctx);
    let ctx = &scoped;
    match state.service.introspect(&ctx, &client, &token).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => protocol_error_response(&error),
    }
}

/// 撤销端点（RFC 7009）。
async fn revoke(
    State(state): State<Arc<OAuthState>>,
    Ctx { ctx, .. }: Ctx,
    headers: HeaderMap,
    ProtocolForm(form): ProtocolForm,
) -> Response {
    let credentials = match client_auth::parse(
        authorization_header(headers.get(header::AUTHORIZATION)),
        form_value(&form, "client_id"),
    ) {
        Ok(credentials) => credentials,
        Err(error) => return protocol_error_response(&error),
    };
    let client = match state.service.authenticate_client(&credentials, true).await {
        Ok(client) => client,
        Err(error) => return protocol_error_response(&error),
    };
    let token = form_value(&form, "token").unwrap_or_default().to_string();
    // 协议端点**没有租户上下文** → 落 OAuth 的保留伪租户；
    // 真正的租户由**令牌 JWT 的 `tid` 声明**决定（服务层会覆盖）。
    let scoped = protocol_ctx(&ctx);
    let ctx = &scoped;
    match state.service.revoke(&ctx, &client, &token).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => protocol_error_response(&error),
    }
}

// ---------------------------------------------------------------------------
// 元数据
// ---------------------------------------------------------------------------

/// 授权服务器元数据（RFC 8414）。
async fn metadata(State(state): State<Arc<OAuthState>>, Ctx { ctx, instance }: Ctx) -> Response {
    match state.service.metadata(&ctx).await {
        Ok(metadata) => Json(metadata).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// `Authorization` 头的值（**原样**；`Basic` / `Bearer` 的解析在各自模块里）。
fn authorization_header(value: Option<&header::HeaderValue>) -> Option<&str> {
    value.and_then(|value| value.to_str().ok()).map(str::trim)
}

/// 平台错误 → Problem Details（契约 C6）。
fn service_problem_response(
    error: &ServiceError,
    ctx: &RequestContext,
    instance: &str,
) -> Response {
    problem_response(&error.problem_details(ctx, instance))
}

/// `OAUTH_INVALID_REQUEST` 的响应（缺键 / 格式非法）。
fn invalid_request_response(ctx: &RequestContext, instance: &str) -> Response {
    let error = crate::error::OAuthError::InvalidRequest {
        reason: "the request is missing a required parameter",
    };
    problem_response(&error.problem_details(ctx, instance))
}

/// 协议错误 → 协议错误体（**小写**，契约 C14 §14.2）。
///
/// ⚠️ **认证失败**（`invalid_client`）→ **只有** `error` 一个键 ——
/// AC-S-143 步骤 5/6/7 要求响应体**逐字**等于 `{"error":"invalid_client"}`。
fn protocol_error_response(error: &ProtocolError) -> Response {
    match error {
        ProtocolError::InvalidClient => error.bare_response(),
        _ => error.response(),
    }
}

/// 解析 `application/x-www-form-urlencoded`。
///
/// ⚠️ **自己解析**（不引 `serde_urlencoded` / `form_urlencoded`）——
/// 零新增第三方依赖（dev-guide §9.10）；语义按 RFC 6749 §2.3.1 附录 B。
fn parse_form(raw: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        let key = client_auth::percent_decode(key);
        if key.is_empty() {
            continue;
        }
        out.insert(key, client_auth::percent_decode(value));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_parser_reads_the_pairs() {
        let form = parse_form("grant_type=authorization_code&code=abc&scope=a%20b");
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some("authorization_code")
        );
        assert_eq!(form.get("code").map(String::as_str), Some("abc"));
        assert_eq!(form.get("scope").map(String::as_str), Some("a b"));
    }

    /// `+` → 空格（表单语义，不是 URL path 语义）。
    #[test]
    fn the_form_parser_decodes_plus_as_space() {
        let form = parse_form("scope=a+b");
        assert_eq!(form.get("scope").map(String::as_str), Some("a b"));
    }

    /// 缺键与空串**不同**（`Option` 的语义）—— 协议端点只看「有没有值」。
    #[test]
    fn a_missing_key_and_an_empty_value_are_different() {
        let form = parse_form("client_id=");
        assert_eq!(form.get("client_id").map(String::as_str), Some(""));
        assert!(form.get("scope").is_none());
        assert!(form_value(&form, "client_id").is_none());
    }
}
