//! HTTP 层：`/api/rbac/*` 路由 + 认证中间件 + 统一错误体。
//!
//! # 管理接口自身受保护（自举防护）
//!
//! `/api/rbac/*` 的**管理接口**（建 / 删角色、授权、撤权、设继承、绑定用户角色、读角色列表）
//! **必须**要求管理权限点 —— 普通用户不得调用。
//!
//! 理由：不挡住的话，**任何登录用户都能给自己授权 `platform-admin`** ——
//! 整个权限系统被架空。
//!
//! | 路由 | 权限点 |
//! |------|--------|
//! | `GET /api/rbac/roles` | `rbac:role:read` |
//! | `GET /api/rbac/roles/{id}` | `rbac:role:read` |
//! | `GET /api/rbac/roles/{id}/permissions` | `rbac:role:read` |
//! | `POST /api/rbac/roles` | `rbac:role:write` |
//! | `DELETE /api/rbac/roles/{id}` | `rbac:role:write` |
//! | `POST /api/rbac/roles/{id}/permissions` | `rbac:role:write` |
//! | `DELETE /api/rbac/roles/{id}/permissions/{code}` | `rbac:role:write` |
//! | `POST /api/rbac/roles/{id}/inherit` | `rbac:role:write` |
//! | `DELETE /api/rbac/roles/{id}/inherit/{parent_id}` | `rbac:role:write` |
//! | `POST /api/rbac/users/{user_id}/roles` | `rbac:user-role:write` |
//! | `DELETE /api/rbac/users/{user_id}/roles/{role_id}` | `rbac:user-role:write` |
//! | `GET /api/rbac/permissions` | **豁免**（任何已认证用户都可读） |
//! | `GET /api/rbac/me/permissions` | **豁免**（任何已认证用户都可读） |
//!
//! # 为什么注册表查询豁免
//!
//! 前端 `can()` 靠它判断（前端权限组件属 REQ-011，本期只保证「注册表有接口可读」）。
//! 把它也保护起来 → 前端拿不到注册表 → 与 REQ-009 的完成定义冲突。
//!
//! # 为什么「当前用户的权限点」也豁免
//!
//! 它的用途**就是**让前端判 `can()`。若它自己需要权限点，
//! 就会出现「要先有权限才能问有没有权限」的循环 —— 用户第一次进页面就拿不到权限点。
//! 豁免是**唯一自洽**的选择；**要求已认证**这一条由认证中间件保证。
//!
//! # 错误体（契约 C6）
//!
//! RFC 9457，形状取自 `platform-core` 的**唯一一份**定义
//! （`platform-iam` 与本模块共用它 —— 三处各写一份必然漂移）。

use std::sync::Arc;

use axum::extract::{FromRequestParts, OriginalUri, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use platform_core::RequestContext;
use platform_core::extension::permission::{DataScope, PermissionChecker, require_permission};
use platform_core::problem::{ProblemDetails, problem_response};
use serde::{Deserialize, Serialize};

use crate::checker::RbacChecker;
use crate::error::RbacError;
use crate::service::RbacService;

/// 读角色的管理权限点。
pub const PERMISSION_ROLE_READ: &str = "rbac:role:read";
/// 改角色（含授权、撤权、继承）的管理权限点。
pub const PERMISSION_ROLE_WRITE: &str = "rbac:role:write";
/// 绑定 / 解绑用户角色的管理权限点。
pub const PERMISSION_USER_ROLE_WRITE: &str = "rbac:user-role:write";
/// 读权限点注册表的权限点（**接口豁免** —— 任何已认证用户都可读）。
pub const PERMISSION_REGISTRY_READ: &str = "rbac:permission:read";

/// 令牌校验接缝（可对象化 —— 测试不必连 IAM）。
pub trait TokenVerifier: Send + Sync {
    /// 校验 access token，返回其中的身份。
    fn verify(&self, token: &str) -> Result<VerifiedToken, RbacError>;
}

/// 令牌里的身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**）。
    pub tenant_id: String,
}

/// 平台 `IamService` 的适配器（只转发验签，不重写语义）。
#[derive(Debug, Clone)]
pub struct IamTokenVerifier {
    inner: Arc<platform_iam::service::IamService>,
}

impl IamTokenVerifier {
    /// 包一个平台的认证服务。
    pub fn new(inner: Arc<platform_iam::service::IamService>) -> Self {
        Self { inner }
    }
}

impl TokenVerifier for IamTokenVerifier {
    fn verify(&self, token: &str) -> Result<VerifiedToken, RbacError> {
        let claims = self
            .inner
            .verify_access_token(token)
            .map_err(|_| RbacError::Unauthenticated)?;
        Ok(VerifiedToken {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
        })
    }
}

/// HTTP 层共享的状态。
#[derive(Clone)]
pub struct RbacState {
    /// 业务服务。
    pub service: Arc<RbacService>,
    /// 权限判定器（同时以 `Arc<dyn PermissionChecker>` 放进请求扩展）。
    pub checker: Arc<RbacChecker>,
    /// 令牌校验。
    pub tokens: Arc<dyn TokenVerifier>,
}

impl std::fmt::Debug for RbacState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RbacState").finish_non_exhaustive()
    }
}

/// 已认证的调用方。
#[derive(Debug, Clone)]
pub struct Authenticated {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**）。
    pub tenant_id: String,
}

impl FromRequestParts<Arc<RbacState>> for Authenticated {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &Arc<RbacState>,
    ) -> Result<Self, Self::Rejection> {
        let instance = original_path(&parts.extensions, parts.uri.path());
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        let user_id = parts
            .extensions
            .get::<RequestContext>()
            .and_then(|ctx| ctx.user_id())
            .map(str::to_string);
        let Some(user_id) = user_id else {
            return Err(problem_response(&ProblemDetails::unauthenticated(
                &ctx, &instance,
            )));
        };
        Ok(Self {
            user_id,
            tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
        })
    }
}

/// 请求的**原始路径**（含 `/api/rbac` 前缀）。
///
/// # 为什么不能直接用 `request.uri().path()`
///
/// `Router::nest` 会**剥掉前缀**（axum 的 `StripPrefix` 层）——
/// 嵌套路由里 `uri().path()` 是 `/me/permissions`，不是调用方请求的
/// `/api/rbac/me/permissions`。错误体的 `instance` 必须是**调用方请求的路径**
/// —— 与 `platform-core` 的鉴权入口（`require_permission`）同一口径。
///
/// 扩展里没有 `OriginalUri` 时（路由没有被嵌套）退回 `fallback`。
fn original_path(extensions: &axum::http::Extensions, fallback: &str) -> String {
    extensions
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

/// 认证中间件：验令牌 → 写入带用户身份的上下文 → 刷新判定缓存 → 注入判定器。
///
/// **必须跑在 `require_permission` 之前** —— 它是路由级层的**外层**。
pub async fn authenticate(
    State(state): State<Arc<RbacState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let instance = original_path(request.extensions(), request.uri().path());
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    let token = match bearer_token(request.headers()) {
        Some(token) => token,
        None => {
            return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
        }
    };
    let verified = match state.tokens.verify(&token) {
        Ok(verified) => verified,
        Err(error) => return rbac_problem_response(&error, &ctx, &instance),
    };

    // 租户上下文（契约 C3）：**请求头优先**；请求头缺失 → 由令牌推断。
    //
    // ⚠️ 请求头**必须**被读到 —— 否则「请求头租户 ≠ 令牌租户 → 404」这条
    //    根本不可达：调用方随便改一个头就能让服务端按**令牌**的租户回答，
    //    头与实际作用域不一致时没有任何信号。
    if let Some(tenant) = header_value(request.headers(), TENANT_HEADER) {
        ctx = ctx.with_tenant(tenant);
    }

    // 请求头租户与令牌租户不一致 → **404**（不泄露资源存在性，契约 C3）。
    match ctx.tenant_id() {
        Some(tenant) if tenant != verified.tenant_id => {
            return problem_response(&ProblemDetails::not_found(&ctx, &instance, "resource"));
        }
        Some(_) => {}
        // 请求头缺失 → 由令牌推断。
        None => {
            ctx = ctx.with_tenant(verified.tenant_id.clone());
        }
    }

    // 上下文里带上**已认证的用户** —— 权限判定需要它。
    ctx = ctx.with_user(verified.user_id.clone());
    request.extensions_mut().insert(ctx.clone());

    // 刷新判定缓存（epoch 不一致 → 重算 → 撤销在下一个请求即生效）。
    if let Err(error) = state.checker.ensure_fresh(&ctx).await {
        return rbac_problem_response(&error, &ctx, &instance);
    }

    // 注入判定实现（`require_permission` 从扩展里取）。
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

/// 租户上下文请求头（契约 C3）。
pub const TENANT_HEADER: &str = "x-tenant-id";

/// 读一个请求头（去空白；空值视为**没有**）。
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 把 RBAC 的错误变成响应体。
pub fn rbac_problem_response(error: &RbacError, ctx: &RequestContext, instance: &str) -> Response {
    let params = error.params();
    let details = ProblemDetails::from_context(
        error.code(),
        title_of(error),
        error.status().as_u16(),
        error.detail(),
        ctx,
        instance,
        if params.is_empty() {
            None
        } else {
            Some(params)
        },
    );
    problem_response(&details)
}

/// 错误码 → 简短标题（**唯一来源**：`platform_core::titles::TITLES`，D113）。
///
/// **只剩转发** —— 本文件不再持有任何 title 文案；面向用户的中文文案由前端按 `code` 渲染。
fn title_of(error: &RbacError) -> &'static str {
    platform_core::titles::title_of_code(error.code())
}

// ---------------------------------------------------------------------------
// 请求 / 响应体
// ---------------------------------------------------------------------------

/// 建角色的请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRoleRequest {
    /// 角色 ID（不给就生成）。
    #[serde(default)]
    pub id: Option<String>,
    /// 角色编码（租户内唯一）。
    pub code: String,
    /// 角色名称。
    pub name: String,
    /// 数据范围（`ALL` / `OWN_ORG` / `OWN_ORG_AND_BELOW` / `SELF` / `CUSTOM`）。
    #[serde(default)]
    pub data_scope: Option<String>,
    /// `CUSTOM` 的过滤表达式。
    #[serde(default)]
    pub custom_filter: Option<String>,
    /// 是否取所有成员节点的并集（**默认 false** = 只按主部门）。
    #[serde(default)]
    pub use_all_memberships: Option<bool>,
}

/// 授权的请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrantPermissionRequest {
    /// 权限点编码（可含通配）。
    pub permission_code: String,
    /// 权限点级数据范围（**可空 = 不覆盖角色级**）。
    #[serde(default)]
    pub data_scope: Option<String>,
}

/// 设置继承的请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InheritRequest {
    /// 被继承的角色 ID。
    pub parent_role_id: String,
}

/// 绑定用户角色的请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindRoleRequest {
    /// 角色 ID。
    pub role_id: String,
}

/// 角色的响应体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleResponse {
    /// 角色 ID。
    pub id: String,
    /// 角色编码。
    pub code: String,
    /// 角色名称。
    pub name: String,
    /// 角色级数据范围。
    pub data_scope: String,
    /// `CUSTOM` 的过滤表达式。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_filter: Option<String>,
    /// 是否取所有成员节点的并集。
    pub use_all_memberships: bool,
    /// 是否内置角色。
    pub builtin: bool,
    /// 展开后的权限点编码（**逐个列出**）。
    pub permissions: Vec<String>,
    /// 展开后的有效权限点（内置管理员 = 注册表全集）。
    pub effective_permissions: Vec<String>,
}

impl From<crate::service::RoleView> for RoleResponse {
    fn from(view: crate::service::RoleView) -> Self {
        Self {
            id: view.id,
            code: view.code,
            name: view.name,
            data_scope: view.data_scope,
            custom_filter: view.custom_filter,
            use_all_memberships: view.use_all_memberships,
            builtin: view.builtin,
            permissions: view.permissions,
            effective_permissions: view.effective_permissions,
        }
    }
}

/// 权限点的响应体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    /// 权限点编码。
    pub code: String,
    /// 权限点名称。
    pub name: String,
    /// 权限点类型。
    pub kind: String,
    /// 属性级校验条件。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// 授权明细的响应体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePermissionResponse {
    /// 权限点编码。
    pub permission_code: String,
    /// 权限点级数据范围（**可空 = 不覆盖角色级**）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_scope: Option<String>,
}

/// 当前用户的有效权限点的响应体。
///
/// # 为什么是**对象**而不是裸数组
///
/// 后续要加 `dataScope`（前端可能想按范围提示用户）或 `registryVersion` 时
/// 不必改响应形状 —— 改形状是破坏性变更。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyPermissionsResponse {
    /// 权限点编码（**升序、去重**；已展开通配）。
    pub permissions: Vec<String>,
}

// ---------------------------------------------------------------------------
// 路由
// ---------------------------------------------------------------------------

/// 构造 `/api/rbac` 的路由。
///
/// **每个路由都绑定自己的管理权限点** —— 三组路由各自 `route_layer`，
/// 合并后各自保留自己的绑定。
///
/// ⚠️ **参数必须是字面量**（不是常量）：契约检查 C-5 要能**静态确认**
/// 「这个权限点真的有鉴权调用点」—— 参数是常量就没法确认，判为**失败**
/// （无法确认 = 不放行，fail closed）。
pub fn router<S2>(state: Arc<RbacState>) -> Router<S2> {
    // 读角色（**要管理权限点**）。
    let read = Router::new()
        .route("/roles", get(list_roles))
        .route("/roles/{id}", get(get_role))
        .route("/roles/{id}/permissions", get(list_role_permissions))
        .route_layer(require_permission("rbac:role:read"));

    // 改角色（建 / 删 / 授权 / 撤权 / 继承）。
    let write = Router::new()
        .route("/roles", post(create_role))
        .route("/roles/{id}", delete(delete_role))
        .route("/roles/{id}/permissions", post(grant_permission))
        .route("/roles/{id}/permissions/{code}", delete(revoke_permission))
        .route("/roles/{id}/inherit", post(set_inherit))
        .route("/roles/{id}/inherit/{parent_id}", delete(remove_inherit))
        .route_layer(require_permission("rbac:role:write"));

    // 绑定 / 解绑用户角色。
    let bindings = Router::new()
        .route("/users/{user_id}/roles", post(bind_user_role))
        .route("/users/{user_id}/roles/{role_id}", delete(unbind_user_role))
        .route_layer(require_permission("rbac:user-role:write"));

    // 权限点注册表 —— **豁免**：任何已认证用户都可读（前端 `can()` 靠它）。
    let registry = Router::new().route("/permissions", get(list_permissions));

    // 当前用户的有效权限点 —— **豁免**权限点要求（但**要求已认证**）。
    //
    // # 为什么豁免
    //
    // 这个接口的用途就是让前端判 `can()`。若它自己需要权限点，
    // 就会出现「要先有权限才能问有没有权限」的循环 —— 用户第一次进页面
    // 就拿不到权限点。豁免是**唯一自洽**的选择。
    //
    // # 不引入新权限点
    //
    // 因此**不需要** `require_permission` 调用点 → 不触发 C-5 扫描与 D27 启动对账。
    let me = Router::new().route("/me/permissions", get(my_permissions));

    // 认证中间件（**路由级层的外面**）：验令牌 + 刷新判定缓存 + 注入判定器。
    Router::new()
        .merge(read)
        .merge(write)
        .merge(bindings)
        .merge(registry)
        .merge(me)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ))
        .with_state(state)
}

/// 列出角色。
async fn list_roles(
    State(state): State<Arc<RbacState>>,
    ctx: TenantCtx,
    uri: OriginalUri,
) -> Response {
    match state.service.list_roles(&ctx.0).await {
        Ok(roles) => {
            let body: Vec<RoleResponse> = roles.into_iter().map(RoleResponse::from).collect();
            Json(body).into_response()
        }
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 读角色。
async fn get_role(
    State(state): State<Arc<RbacState>>,
    Path(id): Path<String>,
    ctx: TenantCtx,
    uri: OriginalUri,
) -> Response {
    match state.service.get_role(&ctx.0, &id).await {
        Ok(role) => Json(RoleResponse::from(role)).into_response(),
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 角色的授权明细。
async fn list_role_permissions(
    State(state): State<Arc<RbacState>>,
    Path(id): Path<String>,
    ctx: TenantCtx,
    uri: OriginalUri,
) -> Response {
    match state.service.list_role_permissions(&ctx.0, &id).await {
        Ok(rows) => {
            let body: Vec<RolePermissionResponse> = rows
                .into_iter()
                .map(|row| RolePermissionResponse {
                    permission_code: row.permission_code,
                    data_scope: row.data_scope.map(|scope| scope.as_str().to_string()),
                })
                .collect();
            Json(body).into_response()
        }
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 建角色。
async fn create_role(
    State(state): State<Arc<RbacState>>,
    ctx: TenantCtx,
    uri: OriginalUri,
    Json(request): Json<CreateRoleRequest>,
) -> Response {
    let data_scope = match request.data_scope.as_deref() {
        Some(value) => match DataScope::parse(value) {
            Some(scope) => scope,
            None => {
                return rbac_problem_response(
                    &RbacError::InvalidArgument {
                        field: "dataScope".to_string(),
                        reason: "must be one of ALL, OWN_ORG, OWN_ORG_AND_BELOW, SELF, CUSTOM"
                            .to_string(),
                    },
                    &ctx.0,
                    uri.path(),
                );
            }
        },
        None => DataScope::OnlySelf,
    };

    let id = request
        .id
        .clone()
        .unwrap_or_else(|| format!("role-{}", uuid::Uuid::new_v4()));
    let role = crate::entity::Role {
        tenant_id: ctx.0.tenant_id().unwrap_or_default().to_string(),
        id,
        code: request.code.clone(),
        name: request.name.clone(),
        data_scope,
        custom_filter: request.custom_filter.clone(),
        use_all_memberships: request.use_all_memberships.unwrap_or(false),
        builtin: false,
    };

    match state.service.create_role(&ctx.0, &role).await {
        Ok(created) => (StatusCode::CREATED, Json(RoleResponse::from(created))).into_response(),
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 删角色。
async fn delete_role(
    State(state): State<Arc<RbacState>>,
    Path(id): Path<String>,
    ctx: TenantCtx,
    uri: OriginalUri,
) -> Response {
    match state.service.delete_role(&ctx.0, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 授权。
async fn grant_permission(
    State(state): State<Arc<RbacState>>,
    Path(id): Path<String>,
    ctx: TenantCtx,
    uri: OriginalUri,
    Json(request): Json<GrantPermissionRequest>,
) -> Response {
    let scope = match request.data_scope.as_deref() {
        Some(value) => match DataScope::parse(value) {
            Some(scope) => Some(scope),
            None => {
                return rbac_problem_response(
                    &RbacError::InvalidArgument {
                        field: "dataScope".to_string(),
                        reason: "must be one of ALL, OWN_ORG, OWN_ORG_AND_BELOW, SELF, CUSTOM"
                            .to_string(),
                    },
                    &ctx.0,
                    uri.path(),
                );
            }
        },
        None => None,
    };
    match state
        .service
        .grant_permission(&ctx.0, &id, &request.permission_code, scope)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 撤权。
async fn revoke_permission(
    State(state): State<Arc<RbacState>>,
    Path((id, code)): Path<(String, String)>,
    ctx: TenantCtx,
    uri: OriginalUri,
) -> Response {
    match state.service.revoke_permission(&ctx.0, &id, &code).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 设置继承。
async fn set_inherit(
    State(state): State<Arc<RbacState>>,
    Path(id): Path<String>,
    ctx: TenantCtx,
    uri: OriginalUri,
    Json(request): Json<InheritRequest>,
) -> Response {
    match state
        .service
        .set_inherit(&ctx.0, &id, &request.parent_role_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 删除继承。
async fn remove_inherit(
    State(state): State<Arc<RbacState>>,
    Path((id, parent_id)): Path<(String, String)>,
    ctx: TenantCtx,
    uri: OriginalUri,
) -> Response {
    match state.service.remove_inherit(&ctx.0, &id, &parent_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 绑定用户角色。
async fn bind_user_role(
    State(state): State<Arc<RbacState>>,
    Path(user_id): Path<String>,
    ctx: TenantCtx,
    uri: OriginalUri,
    Json(request): Json<BindRoleRequest>,
) -> Response {
    match state
        .service
        .bind_user_role(&ctx.0, &user_id, &request.role_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 解绑用户角色。
async fn unbind_user_role(
    State(state): State<Arc<RbacState>>,
    Path((user_id, role_id)): Path<(String, String)>,
    ctx: TenantCtx,
    uri: OriginalUri,
) -> Response {
    match state
        .service
        .unbind_user_role(&ctx.0, &user_id, &role_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 读权限点注册表（**豁免**：任何已认证用户都可读）。
async fn list_permissions(
    State(state): State<Arc<RbacState>>,
    ctx: TenantCtx,
    uri: OriginalUri,
) -> Response {
    match state.service.list_permissions().await {
        Ok(permissions) => {
            let body: Vec<PermissionResponse> = permissions
                .into_iter()
                .map(|permission| PermissionResponse {
                    code: permission.code,
                    name: permission.name,
                    kind: format!("{:?}", permission.kind).to_lowercase(),
                    condition: permission.condition,
                })
                .collect();
            Json(body).into_response()
        }
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 读当前用户的有效权限点（**豁免**权限点要求；**要求已认证**）。
///
/// 数据来自 [`RbacService::effective_permissions`] —— 与 `require_permission`
/// 的判定**同一套展开逻辑**，保证「前端 `can()` 说有」与「后端放行」一致。
async fn my_permissions(
    State(state): State<Arc<RbacState>>,
    caller: Authenticated,
    ctx: TenantCtx,
    uri: OriginalUri,
) -> Response {
    match state
        .service
        .effective_permissions(&ctx.0, &caller.user_id)
        .await
    {
        Ok(permissions) => {
            let body = MyPermissionsResponse {
                permissions: permissions.into_iter().collect(),
            };
            Json(body).into_response()
        }
        Err(error) => rbac_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 带租户上下文的请求上下文提取器。
#[derive(Debug, Clone)]
struct TenantCtx(RequestContext);

impl<S: Send + Sync> FromRequestParts<S> for TenantCtx {
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
        Ok(TenantCtx(ctx))
    }
}
