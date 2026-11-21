//! HTTP 层：应用注册表 / 工作台 / 常用最近 / 导航（REQ-016，design §3）。
//!
//! # 11 个端点（本表是唯一权威）
//!
//! | 方法 | 路径（相对 `/api/apps`） | 权限点 / 认证 |
//! |------|--------------------------|---------------|
//! | `GET` | `/` | `app:registry:read` |
//! | `POST` | `/` | `app:registry:write` → **201** |
//! | `GET` | `/{appId}` | `app:registry:read` |
//! | `PATCH` | `/{appId}` | `app:registry:write` |
//! | `DELETE` | `/{appId}` | `app:registry:delete` → **204** |
//! | `GET` | `/workspace` | **登录即可** |
//! | `GET` | `/recent` | **登录即可** |
//! | `GET` | `/frequent` | **登录即可** |
//! | `POST` | `/{appId}/access` | **登录即可** |
//! | `GET` | `/navigation` | **登录即可** |
//! | `DELETE` | `/{appId}/launch-url` | `app:registry:write` → **204** |
//!
//! # ⚠️ 认证中间件**必须**调 `ensure_fresh`（TD-047）
//!
//! `PermissionChecker::has` 是**纯缓存查询**（`RbacChecker::has` 在缓存里没有该用户时
//! **fail closed** 直接 false），而填缓存是异步的 `ensure_fresh`。
//!
//! 漏掉这一步 → 用户**第一次**打受权限保护的端点**必然 403**，
//! 第二次才好（缓存被别的请求填上了）—— 这种「第一次失败、重试就好」的缺陷最难查。
//!
//! 范式：`crates/platform-oauth/src/http.rs` 的 `authenticate`（修好后的版本）。
//!
//! # ⚠️ 每个端点只挂一个权限点
//!
//! 不能在一个 `Router` 上叠多个 `route_layer` —— axum 的 `Router::route_layer`
//! 是**叠加**语义（多次调用 = 多层全跑）→ `GET` 会同时要求读 **AND** 写 **AND** 删。
//! → 逐 method 挂层（`MethodRouter::route_layer` 只作用于已设置的 method）。
//!
//! # ⚠️ 命名禁区（D216）
//!
//! 本 crate 的 `src/` 里**不得**出现 `authorize` 等 8 个标识符 ——
//! 外部请求身份校验的函数因此叫 [`authenticate`]，**不叫** `authorize_*`。

use std::sync::Arc;

use axum::extract::{FromRequest, OriginalUri, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use platform_core::RequestContext;
use platform_core::extension::permission::{PermissionChecker, PermissionRefresher};
use platform_core::problem::{ProblemDetails, problem_response};
use serde::de::DeserializeOwned;

use crate::authz;
use crate::error::{AppError, ServiceError};
use crate::model::{CreateAppBody, UpdateAppBody};
use crate::service::AppService;

/// HTTP 层共享的状态。
#[derive(Clone)]
pub struct AppState {
    /// 业务服务。
    pub service: Arc<AppService>,
    /// 权限判定器（鉴权层从请求扩展里取）。
    pub checker: Arc<dyn PermissionChecker>,
    /// 权限判定的**异步刷新**接缝。
    ///
    /// ⚠️ **不能省**：`PermissionChecker::has` 是**纯缓存查询**，
    /// 而填缓存是异步的 `ensure_fresh`。少了它 → 用户第一次打本模块的
    /// 受权限保护的端点**必然 403**，第二次才行（TD-047）。
    pub refresher: Arc<dyn PermissionRefresher>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// 提取器
// ---------------------------------------------------------------------------

/// 带租户上下文的请求上下文（**外加**调用方请求的原始路径）。
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
        let instance = original_path(&parts.extensions, "/api/apps");
        Ok(Ctx { ctx, instance })
    }
}

/// 请求体的 JSON 提取器：**解析失败也回平台的错误体**。
///
/// axum 默认的拒绝体是纯文本，那会让「错误体形状」在两端不一致
/// （AC-S-156 路径 5 要求缺必填 / 类型错都是 Problem Details）。
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
            Err(_) => return Err(invalid_request_response(&ctx, &instance)),
        };
        match serde_json::from_slice::<T>(&bytes) {
            Ok(value) => Ok(ApiJson(value)),
            Err(_) => Err(invalid_request_response(&ctx, &instance)),
        }
    }
}

// ---------------------------------------------------------------------------
// 路由
// ---------------------------------------------------------------------------

/// 路由（由装配层 **nest** 到 `/api/apps`）。
pub fn router<S2>(state: Arc<AppState>) -> Router<S2> {
    // 注册表 CRUD（用户会话 + **逐端点**的权限点）。
    let registry = Router::new()
        .route("/", get(list_apps).route_layer(authz::require_registry_read()))
        .route(
            "/",
            post(create_app).route_layer(authz::require_registry_write()),
        )
        .route(
            "/{app_id}",
            get(get_app).route_layer(authz::require_registry_read()),
        )
        .route(
            "/{app_id}",
            patch(update_app).route_layer(authz::require_registry_write()),
        )
        .route(
            "/{app_id}",
            delete(delete_app).route_layer(authz::require_registry_delete()),
        )
        // ⚠️ 专用端点：把 `launch_url` 置 NULL（D283）。
        .route(
            "/{app_id}/launch-url",
            delete(clear_launch_url).route_layer(authz::require_registry_write()),
        );

    // 工作台 / 常用 / 最近 / 导航 / 记访问（**登录即可**，不加权限点 —— D271）。
    let workspace = Router::new()
        .route("/workspace", get(workspace))
        .route("/recent", get(recent))
        .route("/frequent", get(frequent))
        .route("/navigation", get(navigation))
        .route("/{app_id}/access", post(record_access));

    Router::new()
        .merge(registry)
        .merge(workspace)
        // 用户会话中间件（**最外层** —— 必须跑在 `require_permission` 之前）。
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// 认证中间件
// ---------------------------------------------------------------------------

/// 用户会话中间件：验登录令牌 → 写入租户 / 用户上下文 → 注入权限判定器。
///
/// **必须**跑在 `require_permission` 之前 —— 它是路由级层的**外层**。
async fn authenticate(
    State(state): State<Arc<AppState>>,
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
    let user = match state.service_authenticate(&token) {
        Ok(user) => user,
        Err(_) => return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance)),
    };

    // 租户与用户**都来自令牌**。
    ctx = ctx.with_tenant(user.tenant_id.clone());
    ctx = ctx.with_user(user.user_id.clone());
    request.extensions_mut().insert(ctx.clone());

    // ⚠️ **必须**先刷新判定缓存，再让鉴权层判定（TD-047）。
    if let Err(reason) = state.refresher.ensure_fresh(&ctx).await {
        // fail closed：刷新不出来就**不能**让鉴权层读到空缓存（那会一律 403，
        // 看起来像「没权限」而不是「后端坏了」）。
        tracing::error!(%reason, "刷新权限判定缓存失败");
        let details = ProblemDetails::from_context(
            platform_core::problem::CODE_INTERNAL_ERROR,
            platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR),
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

impl AppState {
    /// 验登录令牌 → 已认证用户（转发给服务层，避免 HTTP 层直接依赖 IAM）。
    fn service_authenticate(&self, token: &str) -> Result<crate::tokens::VerifiedToken, ServiceError> {
        self.service.authenticate(token)
    }
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

/// 请求的**原始路径**（含 `/api/apps` 前缀）。
fn original_path(extensions: &axum::http::Extensions, fallback: &str) -> String {
    extensions
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

// ---------------------------------------------------------------------------
// Handler：注册表
// ---------------------------------------------------------------------------

/// 列出全部应用（**数组**，不是 `{ apps: [] }`）。
async fn list_apps(State(state): State<Arc<AppState>>, Ctx { ctx, instance }: Ctx) -> Response {
    match state.service.list_apps(&ctx).await {
        Ok(rows) => Json(rows).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 注册应用 → **201 CREATED**。
async fn create_app(
    State(state): State<Arc<AppState>>,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<CreateAppBody>,
) -> Response {
    match state.service.create_app(&ctx, &body).await {
        Ok(created) => (StatusCode::CREATED, Json(created)).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 读单个应用。
async fn get_app(
    State(state): State<Arc<AppState>>,
    Path(app_id): Path<String>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.get_app(&ctx, &app_id).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 修改应用。
async fn update_app(
    State(state): State<Arc<AppState>>,
    Path(app_id): Path<String>,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<UpdateAppBody>,
) -> Response {
    match state.service.update_app(&ctx, &app_id, &body).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 删除应用 → **204**。
async fn delete_app(
    State(state): State<Arc<AppState>>,
    Path(app_id): Path<String>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.delete_app(&ctx, &app_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 把 `launch_url` 置 NULL → **204**（D283）。
async fn clear_launch_url(
    State(state): State<Arc<AppState>>,
    Path(app_id): Path<String>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.clear_launch_url(&ctx, &app_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Handler：工作台 / 最近 / 常用 / 导航 / 访问
// ---------------------------------------------------------------------------

/// 工作台清单。
async fn workspace(State(state): State<Arc<AppState>>, Ctx { ctx, instance }: Ctx) -> Response {
    match state.service.workspace(&ctx).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 最近访问。
async fn recent(State(state): State<Arc<AppState>>, Ctx { ctx, instance }: Ctx) -> Response {
    match state.service.recent(&ctx).await {
        Ok(rows) => Json(rows).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 常用应用。
async fn frequent(State(state): State<Arc<AppState>>, Ctx { ctx, instance }: Ctx) -> Response {
    match state.service.frequent(&ctx).await {
        Ok(rows) => Json(rows).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 导航项下发。
async fn navigation(State(state): State<Arc<AppState>>, Ctx { ctx, instance }: Ctx) -> Response {
    match state.service.navigation(&ctx).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 记一次访问。
async fn record_access(
    State(state): State<Arc<AppState>>,
    Path(app_id): Path<String>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.record_access(&ctx, &app_id).await {
        Ok(recorded) => Json(recorded).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// 错误体
// ---------------------------------------------------------------------------

/// 服务错误 → Problem Details 响应。
fn service_problem_response(error: &ServiceError, ctx: &RequestContext, instance: &str) -> Response {
    problem_response(&error.problem_details(ctx, instance))
}

/// 请求体解析失败 → `APP_INVALID_REQUEST`（**不是** axum 的纯文本）。
fn invalid_request_response(ctx: &RequestContext, instance: &str) -> Response {
    let error = AppError::InvalidRequest {
        reason: "the request body is not valid json for this endpoint",
    };
    problem_response(&error.problem_details(ctx, instance))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("头名应当合法"),
                value.parse().expect("头值应当合法"),
            );
        }
        map
    }

    #[test]
    fn the_bearer_token_is_extracted() {
        assert_eq!(
            bearer_token(&headers(&[("authorization", "Bearer abc.def.ghi")])),
            Some("abc.def.ghi".to_string())
        );
        assert_eq!(
            bearer_token(&headers(&[("authorization", "bearer xyz")])),
            Some("xyz".to_string())
        );
    }

    #[test]
    fn a_missing_or_blank_bearer_is_absent() {
        assert_eq!(bearer_token(&headers(&[])), None);
        assert_eq!(bearer_token(&headers(&[("authorization", "Bearer   ")])), None);
        assert_eq!(
            bearer_token(&headers(&[("authorization", "Basic dXNlcjpwdw==")])),
            None
        );
    }

    /// 请求体解析失败 → `APP_INVALID_REQUEST` 的 Problem Details（不是纯文本）。
    #[test]
    fn an_unparsable_body_maps_to_app_invalid_request() {
        let ctx = RequestContext::new().with_tenant("tenant-a");
        let response = invalid_request_response(&ctx, "/api/apps");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
