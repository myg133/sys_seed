//! HTTP 层：`/api/iam/*` 路由 + 租户上下文中间件 + 统一错误体 + 幂等键。
//!
//! # 租户上下文（契约 C3）
//!
//! | 请求头 | 含义 | 缺失时 |
//! |--------|------|--------|
//! | `X-Tenant-Id` | 租户上下文 | 由认证令牌推断；推断不出返回 401 |
//! | `X-Space-Id` | 空间上下文 | 视为「不限空间」，不返回错误 |
//!
//! 请求头值**必须**与令牌绑定的租户一致，不一致返回 **404**（不泄露资源存在性）。
//! 业务模块**不得**手工解析请求头 —— 上下文由中间件注入。
//!
//! # 错误体（契约 C6）
//!
//! RFC 9457（Problem Details）：`type` / `title` / `status` / `detail` / `instance` /
//! `code` / `trace_id` / `params`。
//!
//! ⚠️ **`detail` 必须是英文 ASCII**（面向开发者）；面向用户的中文文案由前端 i18n
//! 按 `code` 渲染 —— 后端**不得**在 `detail` 里拼中文句子。
//!
//! # 幂等键（契约 C8）
//!
//! 写操作（POST / PUT / DELETE）支持 `Idempotency-Key` 请求头（UUID）：
//! 同一 key 在 24 小时内重复请求**返回首次结果，不重复执行**；
//! 幂等记录**随租户维度存储**，跨租户不共享。

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use platform_core::RequestContext;
use serde::Serialize;

use crate::error::IamError;
use crate::jwt::AccessClaims;
use crate::service::{
    ChangePasswordRequest, IamService, LoginRequest, LoginResponse, MfaEnrollResponse,
    RefreshRequest, RegisterRequest, RequestMeta, ResetPasswordRequest, SessionView, UserView,
};

/// 租户上下文请求头（契约 C3）。
pub const TENANT_HEADER: &str = "x-tenant-id";
/// 空间上下文请求头（契约 C3）。
pub const SPACE_HEADER: &str = "x-space-id";
/// 设备标签请求头（可选；用于设备会话列表）。
pub const DEVICE_LABEL_HEADER: &str = "x-device-label";
/// 幂等键请求头（契约 C8）。
pub const IDEMPOTENCY_HEADER: &str = "idempotency-key";

/// 幂等键的保存时长（契约 C8：24 小时）。
pub const IDEMPOTENCY_TTL_HOURS: i64 = 24;

/// 幂等响应体的最大字节数（超出就不缓存，只放行本次执行）。
const IDEMPOTENCY_MAX_BODY_BYTES: usize = 256 * 1024;

/// HTTP 层共享的状态。
#[derive(Debug, Clone)]
pub struct IamState {
    /// 业务服务（`Arc` —— 组织架构（REQ-008）的接口要复用它的验签能力）。
    pub service: Arc<IamService>,
}

/// 统一错误响应体（RFC 9457，契约 C6）。
#[derive(Debug, Clone, Serialize)]
pub struct ProblemDetails {
    /// 错误类型 URI。
    #[serde(rename = "type")]
    pub kind: String,
    /// 简短标题。
    pub title: String,
    /// HTTP 状态码。
    pub status: u16,
    /// 面向开发者的**英文**描述（**不得**是中文）。
    pub detail: String,
    /// 出错的接口路径。
    pub instance: String,
    /// 稳定的机器可读错误码。
    pub code: String,
    /// 追踪 ID（串联日志）。
    pub trace_id: String,
    /// 补充参数（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, serde_json::Value>>,
}

/// 构造错误响应体。
pub fn problem_details(error: &IamError, ctx: &RequestContext, instance: &str) -> ProblemDetails {
    let params = error.params();
    ProblemDetails {
        kind: format!("https://errors.base.example/{}", kebab_case(error.code())),
        title: title_of(error),
        status: error.status().as_u16(),
        detail: error.detail(),
        instance: instance.to_string(),
        code: error.code().to_string(),
        trace_id: ctx.trace_id().to_string(),
        params: if params.is_empty() {
            None
        } else {
            Some(params)
        },
    }
}

/// 把错误变成 HTTP 响应。
pub fn problem_response(error: &IamError, ctx: &RequestContext, instance: &str) -> Response {
    let status = error.status();
    let body = problem_details(error, ctx, instance);
    // 运维视角的中文说明放**日志**，不进响应体（契约 C6）。
    tracing::warn!(
        code = body.code,
        status = status.as_u16(),
        trace_id = %body.trace_id,
        "IAM 请求失败（面向用户的中文文案由前端按 code 渲染）"
    );
    (status, Json(body)).into_response()
}

/// 错误码 → kebab-case（用于 `type` 字段）。
fn kebab_case(code: &str) -> String {
    code.to_ascii_lowercase().replace('_', "-")
}

/// 错误码 → 简短标题。
///
/// 标题是**错误类别的名字**，不是面向用户的句子；
/// 面向用户的中文文案由前端 i18n 按 `code` 渲染。
fn title_of(error: &IamError) -> String {
    match error {
        IamError::InvalidCredentials => "凭证无效".to_string(),
        IamError::MfaRequired => "需要多因子认证".to_string(),
        IamError::MfaInvalid => "多因子认证失败".to_string(),
        IamError::AccountLocked => "账号已锁定".to_string(),
        IamError::AccountDisabled => "账号已禁用".to_string(),
        IamError::TokenExpired => "令牌已过期".to_string(),
        IamError::TokenRevoked => "令牌已撤销".to_string(),
        IamError::TokenReuseDetected => "检测到令牌重放".to_string(),
        IamError::PasswordPolicyViolation { .. } => "不满足密码策略".to_string(),
        IamError::PasswordReused => "密码重复".to_string(),
        IamError::LoginRateLimited { .. } => "登录过于频繁".to_string(),
        IamError::TenantContextMissing => "缺少租户上下文".to_string(),
        IamError::Unauthenticated => "未认证".to_string(),
        IamError::NotFound { .. } => "资源不存在".to_string(),
        IamError::Conflict { .. } => "状态冲突".to_string(),
        IamError::NotImplemented { .. } => "尚未实现".to_string(),
        IamError::Internal { .. } => "服务内部错误".to_string(),
    }
}

/// 租户上下文提取器。
#[derive(Debug, Clone)]
pub struct TenantContext(pub RequestContext);

impl<S: Send + Sync> FromRequestParts<S> for TenantContext {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .map(TenantContext)
            .ok_or_else(|| {
                let ctx = RequestContext::new();
                problem_response(&IamError::TenantContextMissing, &ctx, parts.uri.path())
            })
    }
}

/// 已认证的调用方。
#[derive(Debug, Clone)]
pub struct Authenticated {
    /// 账号 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**，不来自请求头）。
    pub tenant_id: String,
    /// 会话 ID。
    pub session_id: String,
    /// 令牌声明。
    pub claims: AccessClaims,
}

impl FromRequestParts<Arc<IamState>> for Authenticated {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &Arc<IamState>,
    ) -> Result<Self, Self::Rejection> {
        let instance = parts.uri.path().to_string();
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);

        let token = bearer_token(&parts.headers)
            .ok_or_else(|| problem_response(&IamError::Unauthenticated, &ctx, &instance))?;
        let claims = state
            .service
            .verify_access_token(&token)
            .map_err(|error| problem_response(&error, &ctx, &instance))?;

        // 租户必须一致：请求头与令牌绑定的租户不一致 → **404**（不泄露资源存在性）。
        match ctx.tenant_id() {
            Some(tenant) if tenant != claims.tenant_id => {
                return Err(problem_response(
                    &IamError::NotFound {
                        resource: "resource",
                    },
                    &ctx,
                    &instance,
                ));
            }
            Some(_) => {}
            // 请求头缺失 → **由令牌推断**（契约 C3）。
            None => {
                let inferred = RequestContext::new()
                    .with_tenant(claims.tenant_id.clone())
                    .with_space(ctx.space_id().unwrap_or_default());
                parts.extensions.insert(inferred);
            }
        }

        Ok(Self {
            user_id: claims.subject.clone(),
            tenant_id: claims.tenant_id.clone(),
            session_id: claims.session_id.clone(),
            claims,
        })
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

/// 租户上下文中间件：`X-Tenant-Id` → [`RequestContext`]。
///
/// **头缺失时不报错** —— 契约 C3 说「由认证令牌推断」；
/// 推断不出来（未认证的接口）由处理器返回 401。
pub async fn tenant_context(mut request: Request, next: Next) -> Response {
    let tenant = header_value(request.headers(), TENANT_HEADER);
    let space = header_value(request.headers(), SPACE_HEADER);
    let ctx = RequestContext::new()
        .with_tenant(tenant.unwrap_or_default())
        .with_space(space.unwrap_or_default());
    request.extensions_mut().insert(ctx);
    next.run(request).await
}

/// 读一个请求头（去空白；空值视为没有）。
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 请求来源信息（IP / UA / 设备标签）。
fn request_meta(headers: &HeaderMap) -> RequestMeta {
    RequestMeta {
        ip: header_value(headers, "x-forwarded-for")
            .and_then(|value| value.split(',').next().map(str::trim).map(str::to_string))
            .unwrap_or_default(),
        user_agent: header_value(headers, "user-agent").unwrap_or_default(),
        device_label: header_value(headers, DEVICE_LABEL_HEADER).unwrap_or_default(),
    }
}

/// 幂等键中间件（契约 C8）。
pub async fn idempotency(
    State(state): State<Arc<IamState>>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    if method == Method::GET || method == Method::HEAD {
        return next.run(request).await;
    }
    let Some(key) = header_value(request.headers(), IDEMPOTENCY_HEADER) else {
        return next.run(request).await;
    };
    let Some(ctx) = request.extensions().get::<RequestContext>().cloned() else {
        return next.run(request).await;
    };
    // 没有租户就没法按租户维度存幂等键 → 交给处理器报错。
    if ctx.tenant_id().is_none() {
        return next.run(request).await;
    }

    let path = request.uri().path().to_string();
    let claimed = match state
        .service
        .store()
        .claim_idempotency(&ctx, &key, method.as_str(), &path)
        .await
    {
        Ok(claimed) => claimed,
        Err(error) => {
            tracing::error!(error = %error, "the idempotency claim failed");
            return problem_response(
                &IamError::Internal {
                    reason: "the data access layer failed".to_string(),
                },
                &ctx,
                &path,
            );
        }
    };

    if !claimed {
        // 已经有请求在处理或已经处理过 → 返回首次结果。
        let existing = state.service.store().find_idempotency(&ctx, &key).await;
        let Ok(Some(record)) = existing else {
            return problem_response(
                &IamError::Conflict {
                    reason: "the idempotent request is still being processed",
                },
                &ctx,
                &path,
            );
        };
        if record.status > 0 {
            let body = record.body.unwrap_or_default();
            let status = StatusCode::from_u16(record.status as u16).unwrap_or(StatusCode::OK);
            return (status, [(header::CONTENT_TYPE, "application/json")], body).into_response();
        }
        return problem_response(
            &IamError::Conflict {
                reason: "the idempotent request is still being processed",
            },
            &ctx,
            &path,
        );
    }

    let response = next.run(request).await;
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, IDEMPOTENCY_MAX_BODY_BYTES).await else {
        return Response::from_parts(parts, Body::empty());
    };
    if parts.status.is_success() {
        let _ = state
            .service
            .store()
            .complete_idempotency(
                &ctx,
                &key,
                parts.status.as_u16(),
                &String::from_utf8_lossy(&bytes),
            )
            .await;
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(bytes))
}

/// 构造 `/api/iam` 的路由。
///
/// 返回类型的状态参数是通用的：调用方（`platform-server`）可以把它
/// `nest` 进自己的路由里 —— IAM 的状态由 `Arc<IamState>` 自带。
pub fn router<S2>(state: Arc<IamState>) -> Router<S2> {
    Router::new()
        .route("/login", post(login))
        .route("/register", post(register))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/logout-all", post(logout_all))
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}", delete(revoke_session))
        .route("/password/forgot", post(forgot_password))
        .route("/password/reset", post(reset_password))
        .route("/password/change", post(change_password))
        .route("/mfa/enroll", post(mfa_enroll))
        .route("/mfa/verify", post(mfa_verify))
        .layer(middleware::from_fn_with_state(state.clone(), idempotency))
        .layer(middleware::from_fn(tenant_context))
        .with_state(state)
}

/// JWKS 端点：**只暴露公钥**。
pub async fn jwks(State(state): State<Arc<IamState>>) -> Json<jsonwebtoken::jwk::JwkSet> {
    Json(state.service.jwks())
}

/// JWKS 路由（挂在根路径 `/.well-known/jwks.json`）。
pub fn jwks_router<S2>(state: Arc<IamState>) -> Router<S2> {
    Router::new()
        .route("/.well-known/jwks.json", get(jwks))
        .with_state(state)
}

/// 注册。
async fn register(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    Json(request): Json<RegisterRequest>,
) -> Response {
    match state.service.register(&ctx, &request).await {
        Ok(user) => (StatusCode::CREATED, Json(user)).into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 登录。
async fn login(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Response {
    let meta = request_meta(&headers);
    match state.service.login(&ctx, &request, &meta).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 刷新（轮换 + 重放检测）。
async fn refresh(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    Json(request): Json<RefreshRequest>,
) -> Response {
    match state.service.refresh(&ctx, &request).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 登出当前会话。
async fn logout(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    auth: Authenticated,
) -> Response {
    match state.service.logout(&ctx, &auth.session_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 登出全端。
async fn logout_all(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    auth: Authenticated,
) -> Response {
    match state.service.logout_all(&ctx, &auth.user_id).await {
        Ok(revoked) => Json(serde_json::json!({ "revokedSessions": revoked })).into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 设备会话列表。
async fn list_sessions(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    auth: Authenticated,
) -> Response {
    match state.service.list_sessions(&ctx, &auth.user_id).await {
        Ok(sessions) => Json(sessions).into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 踢下线（撤销指定会话）。
async fn revoke_session(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    Path(session_id): Path<String>,
    auth: Authenticated,
) -> Response {
    match state
        .service
        .revoke_session(&ctx, &auth.user_id, &session_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 发起密码找回。
///
/// ⚠️ 响应**不区分**邮箱是否已注册（design-summary §5.2 / D7）。
async fn forgot_password(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    Json(request): Json<ForgotPasswordRequest>,
) -> Response {
    let outcome = state.service.forgot_password(&ctx, &request.email).await;
    match outcome {
        Ok(Some(token)) => {
            // 真实系统里这个令牌走邮件通道；**绝不**放进响应体。
            tracing::info!(
                trace_id = %ctx.trace_id(),
                "密码重置令牌已生成（走邮件通道投递，不在响应体里返回）"
            );
            let _ = token;
            Json(serde_json::json!({ "status": "accepted" })).into_response()
        }
        Ok(None) => {
            // 未注册：**完全相同**的响应体。
            Json(serde_json::json!({ "status": "accepted" })).into_response()
        }
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 用重置令牌改密码。
async fn reset_password(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    Json(request): Json<ResetPasswordRequest>,
) -> Response {
    match state.service.reset_password(&ctx, &request).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 已认证用户改密。
async fn change_password(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    auth: Authenticated,
    Json(request): Json<ChangePasswordRequest>,
) -> Response {
    match state
        .service
        .change_password(&ctx, &auth.user_id, &auth.session_id, &request)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 开始绑定 TOTP。
async fn mfa_enroll(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    auth: Authenticated,
) -> Response {
    match state.service.mfa_enroll(&ctx, &auth.user_id).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 校验并绑定 TOTP。
async fn mfa_verify(
    State(state): State<Arc<IamState>>,
    TenantContext(ctx): TenantContext,
    uri: Uri,
    auth: Authenticated,
    Json(request): Json<MfaVerifyRequest>,
) -> Response {
    match state
        .service
        .mfa_verify(&ctx, &auth.user_id, &request.code)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => problem_response(&error, &ctx, uri.path()),
    }
}

/// 密码找回请求。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ForgotPasswordRequest {
    /// 邮箱。
    pub email: String,
}

/// MFA 校验请求。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MfaVerifyRequest {
    /// 验证码（TOTP 或恢复码）。
    pub code: String,
}

/// 让编译器替我们检查响应类型的一致性。
#[allow(dead_code)]
fn _response_types(
    _: Option<LoginResponse>,
    _: Option<UserView>,
    _: Option<SessionView>,
    _: Option<MfaEnrollResponse>,
) {
}
