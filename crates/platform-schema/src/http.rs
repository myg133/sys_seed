//! 元数据读取接口：`GET /api/schema/<moduleId>/<entityKey>`。
//!
//! # 不新增权限点（design §3.1）
//!
//! 元数据是**界面的描述**，不是数据。要求额外权限点会让前端在拿到元数据之前
//! 就要先知道该要什么权限 —— 循环依赖。所以这里只要求**已认证**。
//!
//! 但**不是免费的**：无权字段在元数据里就被剔掉（不是「有但标记隐藏」），
//! 所以「读元数据」不泄露任何用户本来就无权知道的东西。
//!
//! # 字段过滤发生在哪
//!
//! [`EntitySchema::visible_fields`] —— 与数据侧的 [`SchemaProjector`] 用**同一个**
//! 判定函数（[`field_is_visible`]）。两处各写一套判定 = 元数据说能看到、数据却剔掉
//! （或反过来）的经典分叉。
//!
//! [`EntitySchema::visible_fields`]: platform_core::extension::schema::EntitySchema::visible_fields
//!
//! [`SchemaProjector`]: platform_core::extension::schema::SchemaProjector
//! [`field_is_visible`]: platform_core::extension::schema::field_is_visible

use std::sync::Arc;

use axum::extract::{FromRequestParts, OriginalUri, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use platform_core::RequestContext;
use platform_core::extension::permission::{PermissionChecker, PermissionRefresher};
use platform_core::problem::{
    CODE_SCHEMA_ENTITY_NOT_FOUND, CODE_SCHEMA_MODULE_NOT_FOUND, ProblemDetails, problem_response,
};
use serde::Serialize;

use crate::registry::SchemaRegistry;

/// 租户上下文请求头（与 `platform-rbac` 同一口径，契约 C3）。
pub const TENANT_HEADER: &str = "x-tenant-id";

/// 令牌校验接缝。
///
/// 元数据接口**不依赖**任何认证实现（否则 `platform-schema` 要依赖 `platform-iam`，
/// 而 IAM 是可选的部署件）。装配层（`platform-server`）注入真实实现 ——
/// 与 `platform-config` 的 `TokenVerifier` 同款。
pub trait TokenVerifier: Send + Sync {
    /// 验签并取出主体；失败一律按「未认证」处理（不区分原因，避免探测）。
    fn verify(&self, token: &str) -> Result<VerifiedToken, AuthError>;
}

/// 已认证的主体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**）。
    pub tenant_id: String,
}

/// 认证失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthError;

/// 元数据接口的共享状态。
#[derive(Clone)]
pub struct SchemaState {
    /// 实体元数据注册表。
    pub registry: Arc<SchemaRegistry>,
    /// 令牌校验（装配层注入）。
    pub tokens: Arc<dyn TokenVerifier>,
    /// 权限判定（字段级过滤）。
    pub checker: Arc<dyn PermissionChecker>,
    /// 判定缓存刷新（**授权在下一个请求即生效**，不靠 TTL）。
    pub permissions: Arc<dyn PermissionRefresher>,
}

impl std::fmt::Debug for SchemaState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaState")
            .field("entities", &self.registry.entity_count())
            .finish_non_exhaustive()
    }
}

/// 实体元数据的响应体（design §3.2）。
///
/// **没有任何「被过滤」的痕迹**：不含 `hiddenFields` / `deniedFields` / `total` ——
/// 那等于告诉无权限用户「这里有个字段你看不到」。
///
/// 未声明的可选属性由 [`EntitySchema`] 的 `skip_serializing_if` 省略该键（**不是 `null`**）。
#[derive(Debug, Clone, Serialize)]
pub struct EntityMetadata {
    /// 模块 ID。
    pub module: String,
    /// 实体键。
    pub entity: String,
    /// 实体展示名。
    pub label: String,
    /// 数据接口地址（D102）—— 前端据此取数，不声明外部 URL 的限制在启动校验里。
    pub api: platform_core::extension::schema::EntityApi,
    /// 字段清单：**顺序 = manifest 声明顺序**，无权字段已剔除。
    pub fields: Vec<platform_core::extension::schema::FieldSchema>,
}

/// 元数据路由（挂到 `/api/schema` 下）。
pub fn router<S2>(state: Arc<SchemaState>) -> Router<S2> {
    Router::new()
        .route("/{module_id}/{entity_key}", get(entity_metadata))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ))
        .with_state(state)
}

/// 已认证的调用方（`authenticate` 写入请求扩展）。
#[derive(Debug, Clone)]
pub struct Authenticated {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID。
    pub tenant_id: String,
}

impl FromRequestParts<Arc<SchemaState>> for Authenticated {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &Arc<SchemaState>,
    ) -> Result<Self, Self::Rejection> {
        let instance = original_path(&parts.extensions, parts.uri.path());
        let ctx = request_context(&parts.extensions);
        let Some(user_id) = ctx.user_id().map(str::to_string) else {
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

/// 认证中间件：验令牌 → 建立租户/用户上下文 → 刷新判定缓存。
///
/// **必须**刷新判定缓存：否则「授予权限后下一个请求就能看到字段」（AC-S-079 步骤 4）
/// 要等 60 秒 TTL 才成立。失效主机制是数据库 epoch（见 `platform-rbac` 的 checker）。
pub async fn authenticate(
    State(state): State<Arc<SchemaState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let instance = original_path(request.extensions(), request.uri().path());
    let mut ctx = request_context(request.extensions());

    let Some(token) = bearer_token(request.headers()) else {
        return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
    };
    let verified = match state.tokens.verify(&token) {
        Ok(verified) => verified,
        Err(AuthError) => {
            return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
        }
    };

    // 请求头租户优先；缺省由令牌推断。头与令牌不一致 → 404（不泄露资源存在性，契约 C3）。
    if let Some(tenant) = header_value(request.headers(), TENANT_HEADER) {
        ctx = ctx.with_tenant(tenant);
    }
    match ctx.tenant_id() {
        Some(tenant) if tenant != verified.tenant_id => {
            return problem_response(&ProblemDetails::not_found(&ctx, &instance, "resource"));
        }
        Some(_) => {}
        None => ctx = ctx.with_tenant(verified.tenant_id.clone()),
    }
    ctx = ctx.with_user(verified.user_id.clone());

    // 刷新判定缓存：撤销 / 授予在**下一个请求**即生效（不靠 TTL）。
    if state.permissions.ensure_fresh(&ctx).await.is_err() {
        // 判定缓存读不出来 → **进不去**（fail closed），不是「当作没权限放行」。
        return problem_response(&ProblemDetails::from_context(
            platform_core::problem::CODE_INTERNAL_ERROR,
            "服务内部错误",
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            "the permission cache could not be refreshed",
            &ctx,
            &instance,
            None,
        ));
    }

    request.extensions_mut().insert(ctx);
    next.run(request).await
}

/// 读实体元数据。
async fn entity_metadata(
    State(state): State<Arc<SchemaState>>,
    Path((module_id, entity_key)): Path<(String, String)>,
    uri: OriginalUri,
    ctx: TenantContext,
) -> Response {
    // 模块不存在 → SCHEMA_MODULE_NOT_FOUND（**不是**实体不存在）。
    if !state.registry.has_module(&module_id) {
        return schema_not_found(
            &ctx.0,
            uri.path(),
            CODE_SCHEMA_MODULE_NOT_FOUND,
            format!("the module `{module_id}` is not loaded on this service"),
        );
    }
    let Some(schema) = state.registry.entity(&module_id, &entity_key) else {
        return schema_not_found(
            &ctx.0,
            uri.path(),
            CODE_SCHEMA_ENTITY_NOT_FOUND,
            format!("module `{module_id}` declares no entity `{entity_key}`"),
        );
    };

    let fields = schema
        .visible_fields(&ctx.0, state.checker.as_ref())
        .into_iter()
        .cloned()
        .collect();

    Json(EntityMetadata {
        module: module_id,
        entity: entity_key,
        label: schema.label.clone(),
        api: schema.api.clone(),
        fields,
    })
    .into_response()
}

/// 404 的两种语义（模块 / 实体）。
fn schema_not_found(ctx: &RequestContext, instance: &str, code: &str, detail: String) -> Response {
    let details = ProblemDetails::from_context(code, "元数据不存在", 404, detail, ctx, instance, None);
    problem_response(&details)
}

/// 从请求扩展取上下文（没有就新建 —— 与平台其他地方一致）。
fn request_context(extensions: &axum::http::Extensions) -> RequestContext {
    extensions
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new)
}

/// 请求的**原始路径**（`Router::nest` 会剥前缀，错误体的 `instance` 必须是调用方的路径）。
fn original_path(extensions: &axum::http::Extensions, fallback: &str) -> String {
    extensions
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

/// `Authorization: Bearer <token>`。
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

/// 读一个请求头（去空白；空值视为**没有**）。
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 租户 + 用户上下文提取器（`authenticate` 已把它写进扩展）。
#[derive(Debug, Clone)]
pub struct TenantContext(pub RequestContext);

impl FromRequestParts<Arc<SchemaState>> for TenantContext {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &Arc<SchemaState>,
    ) -> Result<Self, Self::Rejection> {
        Ok(TenantContext(request_context(&parts.extensions)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_tokens_are_extracted_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc".to_string()));

        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "bearer abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc".to_string()));

        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn an_empty_bearer_is_not_a_token() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer   ".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn blank_headers_are_treated_as_absent() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tenant-id", "   ".parse().unwrap());
        assert_eq!(header_value(&headers, TENANT_HEADER), None);
    }
}
