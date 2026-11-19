//! HTTP 层：`/api/config/*` 与 `/api/feature-flags/*`。
//!
//! # 路由与鉴权（design §6，**路径照此实现，不自创**）
//!
//! | 路由 | 鉴权 |
//! |------|------|
//! | `GET /api/config/entries` | 需要令牌（租户级 / 空间级；平台级键 → 403） |
//! | `GET /api/config/platform-entries` | 权限点 `config:platform:read`（独立路由，D69） |
//! | `GET /api/config/branding` | **公开**（白名单 + 扁平 map） |
//! | `PUT /api/config/entries` | 权限点 `config:entry:write` |
//! | `PUT /api/config/platform-entries` | 权限点 `config:platform:write`（独立路由，D59） |
//! | `GET /api/feature-flags` | 需要令牌 |
//! | `GET /api/feature-flags/{flag}` | 需要令牌 |
//! | `PUT /api/feature-flags/{flag}` | 权限点 `config:flag:write` |
//!
//! # 为什么平台级读写要**各拆一条路由**（D59 / D69）
//!
//! `require_permission(...)` 返回的是**静态路由层**，挂在路由上就对**该路由的所有
//! 请求**生效。若把 `config:platform:write` 也挂到 `PUT /api/config/entries`：
//! 只持有 `config:entry:write` 的用户**连租户级写都会被 403**（与 AC-S-060 步骤 3
//! 冲突）；若改成「在 handler 里按 body 的 scope 判定」，则 `config:platform:write`
//! **没有** `require_permission` 调用点 → C-5 失败。读侧同构（D69）。
//!
//! # 跨租户请求头
//!
//! 令牌属于 T1、请求头写 `X-Tenant-Id: T2` → **404**（当资源不存在处理，不泄露存在性；
//! AC-S-060 步骤 7 / 契约 C3）。注意 `README`/rbac 的 `authenticate` 没有解析这个头
//! （它假定上游中间件注入），本模块自带认证中间件，所以**自己解析**。

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{OriginalUri, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use platform_core::RequestContext;
use platform_core::extension::permission::{
    PermissionChecker, PermissionRefresher, require_permission,
};
use platform_core::problem::{CODE_INTERNAL_ERROR, ProblemDetails, problem_response};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ConfigError;
use crate::service::{ConfigService, FlagWriteRequest, WriteRequest, WriteRoute};

/// 读租户级 / 空间级配置的路径。
pub const ENTRIES_PATH: &str = "/api/config/entries";
/// 平台级配置的路径（读写共用前缀，方法不同）。
pub const PLATFORM_ENTRIES_PATH: &str = "/api/config/platform-entries";
/// 公开的品牌路径。
pub const BRANDING_PATH: &str = "/api/config/branding";
/// Feature Flag 批量 / 单条路径。
pub const FLAGS_PATH: &str = "/api/feature-flags";

/// 令牌校验接缝（可对象化 —— 测试不必连 IAM）。
pub trait TokenVerifier: Send + Sync {
    /// 校验 access token，返回其中的身份。
    fn verify(&self, token: &str) -> Result<VerifiedToken, ConfigError>;
}

/// 令牌里的身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**）。
    pub tenant_id: String,
}

/// HTTP 层共享状态。
#[derive(Clone)]
pub struct ConfigState {
    /// 业务服务。
    pub service: Arc<ConfigService>,
    /// 令牌校验。
    pub tokens: Arc<dyn TokenVerifier>,
    /// 权限判定器（同时以 `Arc<dyn PermissionChecker>` 放进请求扩展）。
    pub checker: Arc<dyn PermissionChecker>,
    /// 判定缓存刷新（认证中间件在鉴权前调用）。
    pub permissions: Arc<dyn PermissionRefresher>,
}

impl std::fmt::Debug for ConfigState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigState").finish_non_exhaustive()
    }
}

/// 构造**全部**路由（配置 + 开关 + 公开品牌）。
pub fn router(state: Arc<ConfigState>) -> Router {
    // 租户级 / 空间级读：需要令牌，但**不挂**平台读权限点（否则普通用户连租户键都读不到）。
    let read = Router::new().route(ENTRIES_PATH, get(read_entries));

    // 平台级读：独立路由 + 独立权限点（D69）。
    let platform_read = Router::new()
        .route(PLATFORM_ENTRIES_PATH, get(read_platform_entries))
        .route_layer(require_permission("config:platform:read"));

    // 租户级 / 空间级写。
    let write = Router::new()
        .route(ENTRIES_PATH, put(write_entries))
        .route_layer(require_permission("config:entry:write"));

    // 平台级写：独立路由 + 独立权限点（D59）。
    let platform_write = Router::new()
        .route(PLATFORM_ENTRIES_PATH, put(write_platform_entries))
        .route_layer(require_permission("config:platform:write"));

    // 开关读：需要令牌，无额外权限点。
    let flags_read = Router::new()
        .route(FLAGS_PATH, get(list_flags))
        .route(&format!("{FLAGS_PATH}/{{flag}}"), get(get_flag));

    // 改开关：专用权限点。
    let flags_write = Router::new()
        .route(&format!("{FLAGS_PATH}/{{flag}}"), put(put_flag))
        .route_layer(require_permission("config:flag:write"));

    let protected = Router::new()
        .merge(read)
        .merge(platform_read)
        .merge(write)
        .merge(platform_write)
        .merge(flags_read)
        .merge(flags_write)
        // 认证中间件必须跑在 `require_permission` 的**外层**：先验令牌、再判权限。
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ));

    // 公开接口**不**经过认证中间件。
    let branding = Router::new().route(BRANDING_PATH, get(branding));

    Router::new()
        .merge(protected)
        .merge(branding)
        .with_state(state)
}

/// 已认证的请求上下文提取器。
#[derive(Debug, Clone)]
struct ReqCtx(RequestContext);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ReqCtx {
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
        Ok(ReqCtx(ctx))
    }
}

/// 认证中间件：验令牌 → 解析租户 / 空间头 → 刷新权限缓存 → 注入判定器。
async fn authenticate(
    State(state): State<Arc<ConfigState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let instance = request.uri().path().to_string();
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    if let Some(tenant) = header_tenant(request.headers()) {
        ctx = ctx.with_tenant(tenant);
    }
    if let Some(space) = header_space(request.headers()) {
        ctx = ctx.with_space(space);
    }

    let Some(token) = bearer_token(request.headers()) else {
        return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
    };
    let verified = match state.tokens.verify(&token) {
        Ok(verified) => verified,
        Err(_) => return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance)),
    };

    // 请求头租户与令牌租户不一致 → **404**（不泄露资源存在性，契约 C3）。
    match ctx.tenant_id() {
        Some(tenant) if tenant != verified.tenant_id => {
            return problem_response(&ProblemDetails::not_found(&ctx, &instance, "resource"));
        }
        Some(_) => {}
        None => {
            ctx = ctx.with_tenant(verified.tenant_id.clone());
        }
    }
    ctx = ctx.with_user(verified.user_id.clone());
    request.extensions_mut().insert(ctx.clone());

    // 刷新判定缓存（权限撤销在下一个请求生效）。
    if let Err(reason) = state.permissions.ensure_fresh(&ctx).await {
        return problem_response(&ProblemDetails::from_context(
            CODE_INTERNAL_ERROR,
            platform_core::titles::title_of_code(CODE_INTERNAL_ERROR),
            500,
            format!("the permission cache could not be refreshed: {reason}"),
            &ctx,
            &instance,
            None,
        ));
    }

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

/// `X-Tenant-Id`（缺失 / 空串 = 未知租户）。
fn header_tenant(headers: &HeaderMap) -> Option<String> {
    header_text(headers, "x-tenant-id")
}

/// `X-Space-Id`（缺失 / 空串 = 不限空间，契约 C3）。
fn header_space(headers: &HeaderMap) -> Option<String> {
    header_text(headers, "x-space-id")
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    let value = headers.get(name)?.to_str().ok()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// 读请求的 query。
#[derive(Debug, Deserialize)]
struct KeysQuery {
    /// 逗号分隔的键名。
    keys: Option<String>,
}

fn split_keys(raw: Option<String>) -> Vec<String> {
    match raw {
        Some(raw) => raw.split(',').map(str::to_string).collect(),
        None => Vec::new(),
    }
}

/// `GET /api/config/entries`。
async fn read_entries(
    State(state): State<Arc<ConfigState>>,
    ctx: ReqCtx,
    uri: OriginalUri,
    Query(query): Query<KeysQuery>,
) -> Response {
    match state
        .service
        .read_entries(&ctx.0, &split_keys(query.keys))
        .await
    {
        Ok(entries) => Json(serde_json::json!({ "entries": entries })).into_response(),
        Err(error) => config_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// `GET /api/config/platform-entries`。
async fn read_platform_entries(
    State(state): State<Arc<ConfigState>>,
    ctx: ReqCtx,
    uri: OriginalUri,
    Query(query): Query<KeysQuery>,
) -> Response {
    match state
        .service
        .read_platform_entries(&ctx.0, &split_keys(query.keys))
        .await
    {
        Ok(entries) => Json(serde_json::json!({ "entries": entries })).into_response(),
        Err(error) => config_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// `PUT /api/config/entries`。
async fn write_entries(
    State(state): State<Arc<ConfigState>>,
    ctx: ReqCtx,
    uri: OriginalUri,
    Json(request): Json<WriteRequest>,
) -> Response {
    match state
        .service
        .write_entries(&ctx.0, &request, WriteRoute::Scoped)
        .await
    {
        Ok(written) => Json(serde_json::json!({ "written": written })).into_response(),
        Err(error) => config_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// `PUT /api/config/platform-entries`。
async fn write_platform_entries(
    State(state): State<Arc<ConfigState>>,
    ctx: ReqCtx,
    uri: OriginalUri,
    Json(request): Json<WriteRequest>,
) -> Response {
    match state
        .service
        .write_entries(&ctx.0, &request, WriteRoute::Platform)
        .await
    {
        Ok(written) => Json(serde_json::json!({ "written": written })).into_response(),
        Err(error) => config_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// `GET /api/config/branding`（公开）。
async fn branding(
    State(state): State<Arc<ConfigState>>,
    headers: HeaderMap,
    uri: OriginalUri,
) -> Response {
    let ctx = RequestContext::new();
    let tenant = header_tenant(&headers);
    match state.service.branding(tenant.as_deref()).await {
        Ok(entries) => {
            let mut map = serde_json::Map::new();
            for (key, value) in entries {
                map.insert(key, value);
            }
            Json(Value::Object(map)).into_response()
        }
        Err(error) => config_problem_response(&error, &ctx, uri.path()),
    }
}

/// `GET /api/feature-flags`。
async fn list_flags(
    State(state): State<Arc<ConfigState>>,
    ctx: ReqCtx,
    uri: OriginalUri,
) -> Response {
    match state.service.read_flags(&ctx.0).await {
        Ok(flags) => Json(serde_json::json!({ "flags": flags })).into_response(),
        Err(error) => config_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// `GET /api/feature-flags/{flag}`。
async fn get_flag(
    State(state): State<Arc<ConfigState>>,
    Path(flag): Path<String>,
    ctx: ReqCtx,
    uri: OriginalUri,
) -> Response {
    match state.service.read_flag(&ctx.0, &flag).await {
        Ok(decision) => Json(decision).into_response(),
        Err(error) => config_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// `PUT /api/feature-flags/{flag}`。
async fn put_flag(
    State(state): State<Arc<ConfigState>>,
    Path(flag): Path<String>,
    ctx: ReqCtx,
    uri: OriginalUri,
    Json(request): Json<FlagWriteRequest>,
) -> Response {
    match state.service.write_flag(&ctx.0, &flag, &request).await {
        Ok(decision) => Json(decision).into_response(),
        Err(error) => config_problem_response(&error, &ctx.0, uri.path()),
    }
}

/// 把配置错误变成 RFC 9457 响应体（契约 C6）。
fn config_problem_response(error: &ConfigError, ctx: &RequestContext, instance: &str) -> Response {
    if matches!(
        error,
        ConfigError::Database(_) | ConfigError::Internal { .. }
    ) {
        // 运维视角的细节只进日志，不进响应体。
        tracing::error!(code = error.code(), detail = %error.detail(), "配置服务内部错误");
    }
    let params = error.params();
    let details = ProblemDetails::from_context(
        error.code(),
        error.title(),
        error.status(),
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

/// 未使用的响应辅助类型：写接口的响应形状是 `{"written": n}`（测试只读状态码）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WrittenResponse {
    /// 写入条目数。
    pub written: usize,
}

/// 一个错误码 → HTTP 状态的公开映射（供测试与文档复用，避免各自硬编码）。
pub fn status_of(error: &ConfigError) -> StatusCode {
    StatusCode::from_u16(error.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// 品牌响应（扁平 map）的类型别名，便于调用方构造断言。
pub type BrandingResponse = BTreeMap<String, Value>;
