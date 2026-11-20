//! 审计接口：`/api/audit/*`（REQ-014 AC-S-130 / AC-S-131 / AC-S-022）。
//!
//! # 端点与权限点（D138）
//!
//! | 方法 | 路径 | 权限点 |
//! |------|------|--------|
//! | GET | `/api/audit/events` | `audit:events:read` |
//! | GET | `/api/audit/events/{id}` | `audit:events:read` |
//! | GET | `/api/audit/verify` | `audit:verify:run` |
//! | POST | `/api/audit/archive` | `audit:archive:run` |
//!
//! 🔑 **鉴权薄封装**在 `platform-audit`（D160）—— 本文件**不**直接写
//! `require_permission("audit:…")` 字面量：C-5 只扫「manifest 所属 crate 的
//! `src/`」→ 字面量必须与权限点登记**同处一地**。
//!
//! # 数值查询参数**自解析**（D153）
//!
//! axum 0.8 的 `QueryRejection` 是**纯文本** 400 → 违反契约 C6 的
//! `ProblemDetails` 要求。→ 参数由 `platform-audit` 的
//! [`AuditQueryParams::parse`] 解析，错误由本文件转成 `ProblemDetails`
//! （`AUDIT_INVALID_FILTER`，400）。
//!
//! # 租户**只**来自认证上下文（D162）
//!
//! 查询参数里的 `tenant_id` **忽略 + warn**（不 400）—— 租户由认证上下文与
//! RLS 决定。
//!
//! # 响应字段名 camelCase（D167 / 契约 C13）
//!
//! `id` / `actorId` / `action` / `objectType` / `objectId` / `before` / `after` /
//! `ip` / `userAgent` / `createdAt` / `prevHash` / `hash` + `total` + `nextCursor`。

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Extension, OriginalUri, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use platform_audit::{
    ArchiveService, AuditEventRow, AuditQueryParams, AuditQueryService, ChainVerifier,
};
use platform_core::RequestContext;
use platform_core::extension::permission::{PermissionChecker, PermissionRefresher};
use platform_core::problem::{ProblemDetails, problem_response};
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;

/// 令牌校验的失败原因。
///
/// # 为什么不是 `Result<_, ()>`
///
/// 仓内惯例是**带类型的错误**（`platform-rbac` 用 `RbacError`、
/// `platform-schema` 用 `AuthError`）—— `()` 会让 clippy 报
/// `result_unit_err`，也丢掉「令牌为什么无效」这个信息（D191）。
///
/// ⚠️ 本 trait **只**属于 `platform-server`：其他 crate 的同名 `TokenVerifier`
/// 是**各自独立的 trait**，签名不受此处影响。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenVerifyError {
    /// 令牌缺失、过期、被篡改，或签发方不认。
    Invalid,
}

impl std::fmt::Display for TokenVerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid => f.write_str("the access token is not valid"),
        }
    }
}

impl std::error::Error for TokenVerifyError {}

/// 令牌校验接缝（可对象化 —— 测试不必连 IAM）。
pub trait TokenVerifier: Send + Sync {
    /// 校验 access token，返回其中的身份。
    fn verify(&self, token: &str) -> Result<VerifiedToken, TokenVerifyError>;
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
    fn verify(&self, token: &str) -> Result<VerifiedToken, TokenVerifyError> {
        let claims = self
            .inner
            .verify_access_token(token)
            .map_err(|_| TokenVerifyError::Invalid)?;
        Ok(VerifiedToken {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
        })
    }
}

/// HTTP 层共享的状态。
#[derive(Clone)]
pub struct AuditState {
    /// 查询服务（分页 / 过滤 / 排序）。
    pub query: Arc<AuditQueryService>,
    /// 链校验服务。
    pub verifier: Arc<ChainVerifier>,
    /// 归档服务。
    pub archive: Arc<ArchiveService>,
    /// 令牌校验。
    pub tokens: Arc<dyn TokenVerifier>,
    /// 权限判定（放进请求扩展，供 `require_permission` 用）。
    pub checker: Arc<dyn PermissionChecker>,
    /// 判定缓存刷新（授权在下一个请求即生效）。
    pub permissions: Arc<dyn PermissionRefresher>,
}

// ---------------------------------------------------------------------------
// 响应形状
// ---------------------------------------------------------------------------

/// 一条审计记录（camelCase，D167）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventView {
    /// 记录 ID。
    pub id: String,
    /// 操作人（未认证时为 `None`）。
    pub actor_id: Option<String>,
    /// 动作名（事件码）。
    pub action: String,
    /// 对象类型。
    pub object_type: String,
    /// 对象 ID。
    pub object_id: Option<String>,
    /// 变更前（**JSON 值**；列里存的是 JSON 文本）。
    pub before: Option<Value>,
    /// 变更后（**JSON 值**；列里存的是 JSON 文本）。
    pub after: Option<Value>,
    /// 来源 IP（空串 = 未知）。
    pub ip: String,
    /// 设备（空串 = 未知）。
    pub user_agent: String,
    /// 创建时刻（UTC、微秒 6 位）。
    pub created_at: String,
    /// 上一条的哈希。
    pub prev_hash: String,
    /// 本条的哈希。
    pub hash: String,
}

/// 一页审计记录。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditPageView {
    /// 本页记录。
    pub items: Vec<AuditEventView>,
    /// 该租户的记录总数（**不受分页影响**）。
    pub total: i64,
    /// 下一页的游标（没有下一页时为 `None`）。
    pub next_cursor: Option<String>,
}

/// 链校验结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyView {
    /// 链是否完好。
    pub ok: bool,
    /// 检查了多少条。
    pub checked: usize,
    /// 断裂清单。
    pub broken: Vec<BrokenView>,
}

/// 一条断裂记录。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokenView {
    /// 断裂记录的 ID。
    pub id: String,
    /// 断裂原因（`hash_mismatch` / `prev_hash_mismatch`）。
    pub reason: String,
    /// 期望值。
    pub expected: String,
    /// 实际值。
    pub actual: String,
}

/// 归档结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveView {
    /// 搬走了多少条。
    pub moved: i64,
    /// 本次归档时刻。
    pub archived_at: Option<String>,
    /// 建议的下次归档时刻（D137：让运维看得见）。
    pub next_recommended_at: Option<String>,
}

/// 把查询层的行转成响应视图。
///
/// ⚠️ `before` / `after` 在**列里是 JSON 文本**（D141）→ 这里**解析成 JSON 值**
/// 返回（契约 C7 的列表项形状）。解析不了 → `null`（不 500）。
fn to_view(row: &AuditEventRow) -> AuditEventView {
    AuditEventView {
        id: row.id.clone(),
        actor_id: row.actor_id.clone(),
        action: row.action.clone(),
        object_type: row.object_type.clone(),
        object_id: row.object_id.clone(),
        before: row
            .before
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok()),
        after: row
            .after
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok()),
        ip: row.ip.clone(),
        user_agent: row.user_agent.clone(),
        created_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.created_at),
        prev_hash: row.prev_hash.clone(),
        hash: row.hash.clone(),
    }
}

/// 格式化时刻（UTC、微秒 6 位）。
fn format_moment(moment: Option<OffsetDateTime>) -> Option<String> {
    moment.map(|value| platform_core::extension::audit::format_timestamp_micros_utc(&value))
}

// ---------------------------------------------------------------------------
// 路由
// ---------------------------------------------------------------------------

/// 构造 `/api/audit` 的路由。
///
/// ⚠️ **鉴权层必须是外层**：`authenticate` 跑在 `require_permission` **之前**
/// —— 否则鉴权层拿不到用户 → 一律 401（fail closed），永远 403/401 都分不清。
pub fn router<S>(state: Arc<AuditState>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    // 读事件。
    let read = Router::new()
        .route("/events", get(list_events))
        .route("/events/{id}", get(get_event))
        .route_layer(platform_audit::require_events_read());

    // 链校验。
    let verify = Router::new()
        .route("/verify", get(verify_chain))
        .route_layer(platform_audit::require_verify_run());

    // 归档。
    let archive = Router::new()
        .route("/archive", post(run_archive))
        .route_layer(platform_audit::require_archive_run());

    Router::new()
        .merge(read)
        .merge(verify)
        .merge(archive)
        // 认证中间件必须跑在 `require_permission` 的**外层**：先验令牌、再判权限。
        .layer(axum::middleware::from_fn_with_state(state, authenticate))
}

/// 请求上下文（从扩展里取）。
#[derive(Debug, Clone)]
struct ReqCtx(RequestContext);

impl<S> axum::extract::FromRequestParts<S> for ReqCtx
where
    S: Send + Sync,
{
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
        Ok(ReqCtx(ctx))
    }
}

/// 认证中间件：验令牌 → 建上下文 → 刷新判定缓存 → 注入判定器。
///
/// ⚠️ 状态用 **`State`** 取（不是 `Extension`）：中间件的状态由
/// `from_fn_with_state(state, authenticate)` 提供 —— `Extension` 取的是
/// **请求扩展**，而此刻扩展里还没有审计状态（只有本函数自己会插）→
/// 用 `Extension` 会直接 **500**（axum 的「扩展缺失」）。
/// 处理器那边用 `Extension<Arc<AuditState>>` 是对的：本函数会把状态
/// **插进请求扩展**（见下方）。
async fn authenticate(
    State(state): State<Arc<AuditState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let instance = original_path(&request);
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    let Some(token) = bearer_token(request.headers()) else {
        return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
    };
    let verified = match state.tokens.verify(&token) {
        Ok(verified) => verified,
        Err(TokenVerifyError::Invalid) => {
            return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
        }
    };

    // 请求头租户与令牌租户不一致 → **404**（不泄露资源存在性，契约 C3）。
    if let Some(tenant) = ctx.tenant_id() {
        if tenant != verified.tenant_id {
            return problem_response(&ProblemDetails::not_found(&ctx, &instance, "resource"));
        }
    } else {
        ctx = ctx.with_tenant(verified.tenant_id.clone());
    }
    ctx = ctx.with_user(verified.user_id.clone());
    request.extensions_mut().insert(ctx.clone());

    // 刷新判定缓存（权限撤销在下一个请求即生效）。
    if let Err(reason) = state.permissions.ensure_fresh(&ctx).await {
        return problem_response(&ProblemDetails::from_context(
            platform_core::problem::CODE_INTERNAL_ERROR,
            platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR),
            500,
            format!("the permission cache could not be refreshed: {reason}"),
            &ctx,
            &instance,
            None,
        ));
    }

    let checker: Arc<dyn PermissionChecker> = state.checker.clone();
    request.extensions_mut().insert(checker);
    // 审计状态也放进扩展（处理器用 `Extension` 取 → 路由的 state 类型保持通用，
    // 才能被 `nest` 进顶层路由）。
    request.extensions_mut().insert(state.clone());

    next.run(request).await
}

/// 请求的**原始路径**（含 `/api/audit` 前缀）。
fn original_path(request: &Request) -> String {
    request
        .extensions()
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_string())
        .unwrap_or_else(|| request.uri().path().to_string())
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

// ---------------------------------------------------------------------------
// 处理器
// ---------------------------------------------------------------------------

/// `GET /api/audit/events` —— 分页 + 过滤 + 排序（契约 C7 口径）。
async fn list_events(
    Extension(state): Extension<Arc<AuditState>>,
    req_ctx: ReqCtx,
    req: Result<Query<HashMap<String, String>>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let ctx = req_ctx.0.clone();
    let params = match req {
        Ok(Query(params)) => params,
        Err(rejection) => {
            // ⚠️ axum 的 `QueryRejection` 是**纯文本** 400 → 必须转成
            // `ProblemDetails`（契约 C6）。
            return invalid_filter_response(
                &ctx,
                "/api/audit/events",
                &format!("the query string is malformed: {rejection}"),
            );
        }
    };
    let parsed = match AuditQueryParams::parse(&params) {
        Ok(parsed) => parsed,
        Err(error) => {
            return invalid_filter_response(&ctx, "/api/audit/events", &error.detail());
        }
    };

    match state.query.query(&ctx, &parsed).await {
        Ok(page) => {
            let view = AuditPageView {
                items: page.items.iter().map(to_view).collect(),
                total: page.total,
                next_cursor: page.next_cursor,
            };
            (StatusCode::OK, Json(view)).into_response()
        }
        Err(error) => audit_error_response(&error, &ctx, "/api/audit/events"),
    }
}

/// `GET /api/audit/events/{id}` —— 单条详情。
async fn get_event(
    Extension(state): Extension<Arc<AuditState>>,
    req_ctx: ReqCtx,
    Path(id): Path<String>,
) -> Response {
    let ctx = req_ctx.0.clone();
    match state.query.find_by_id(&ctx, &id).await {
        Ok(Some(row)) => (StatusCode::OK, Json(to_view(&row))).into_response(),
        Ok(None) => problem_response(&ProblemDetails::audit_event_not_found(
            &ctx,
            &format!("/api/audit/events/{id}"),
        )),
        Err(error) => audit_error_response(&error, &ctx, "/api/audit/events"),
    }
}

/// `GET /api/audit/verify` —— 链校验。
async fn verify_chain(Extension(state): Extension<Arc<AuditState>>, req_ctx: ReqCtx) -> Response {
    let ctx = req_ctx.0.clone();
    match state.verifier.verify(&ctx).await {
        Ok(report) => {
            let view = VerifyView {
                ok: report.ok,
                checked: report.checked,
                broken: report
                    .broken
                    .iter()
                    .map(|entry| BrokenView {
                        id: entry.id.clone(),
                        reason: entry.reason.as_str().to_string(),
                        expected: entry.expected.clone(),
                        actual: entry.actual.clone(),
                    })
                    .collect(),
            };
            (StatusCode::OK, Json(view)).into_response()
        }
        Err(error) => audit_error_response(&error, &ctx, "/api/audit/verify"),
    }
}

/// `POST /api/audit/archive` —— 触发归档（**单租户**，D158）。
async fn run_archive(Extension(state): Extension<Arc<AuditState>>, req_ctx: ReqCtx) -> Response {
    let ctx = req_ctx.0.clone();
    match state
        .archive
        .archive_older_than(&ctx, platform_audit::DEFAULT_ONLINE_DAYS)
        .await
    {
        Ok(result) => {
            let view = ArchiveView {
                moved: result.moved,
                archived_at: format_moment(result.archived_at),
                next_recommended_at: format_moment(result.next_recommended_at),
            };
            (StatusCode::OK, Json(view)).into_response()
        }
        Err(error) => audit_error_response(&error, &ctx, "/api/audit/archive"),
    }
}

// ---------------------------------------------------------------------------
// 错误响应
// ---------------------------------------------------------------------------

/// 400 `AUDIT_INVALID_FILTER`。
fn invalid_filter_response(ctx: &RequestContext, instance: &str, reason: &str) -> Response {
    problem_response(&ProblemDetails::audit_invalid_filter(ctx, instance, reason))
}

/// 把审计错误转成 `ProblemDetails`。
fn audit_error_response(
    error: &platform_core::extension::audit::AuditError,
    ctx: &RequestContext,
    instance: &str,
) -> Response {
    problem_response(&error.problem_details(ctx, instance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::extension::expr::CompiledCondition;
    use platform_core::extension::permission::{
        DataScope, PermissionChecker, PermissionRefresher, ScopeFilter,
    };
    use platform_db::{DbOptions, TenantDb};
    use tower::ServiceExt;

    /// 恒定「有权限」的判定器（测试用）。
    struct AllowAll;

    impl PermissionChecker for AllowAll {
        fn has(&self, _ctx: &RequestContext, _permission: &str) -> bool {
            true
        }

        fn scope(&self, _ctx: &RequestContext, _scope: DataScope) -> ScopeFilter {
            ScopeFilter::All
        }

        fn effective_scope(&self, _ctx: &RequestContext, _permission: &str) -> ScopeFilter {
            ScopeFilter::All
        }

        fn condition(&self, _ctx: &RequestContext, _permission: &str) -> Option<CompiledCondition> {
            None
        }
    }

    impl PermissionRefresher for AllowAll {
        fn ensure_fresh<'a>(
            &'a self,
            _ctx: &'a RequestContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>
        {
            Box::pin(async { Ok(()) })
        }
    }

    /// 恒定通过的令牌校验（测试用）。
    struct FixedTokens;

    impl TokenVerifier for FixedTokens {
        fn verify(&self, _token: &str) -> Result<VerifiedToken, TokenVerifyError> {
            Ok(VerifiedToken {
                user_id: "u-2001".to_string(),
                tenant_id: "tenant-a".to_string(),
            })
        }
    }

    /// 造一套审计状态（**数据库连不上**是故意的 —— 用例只关心「请求有没有走到处理器」）。
    async fn state_for_test() -> Arc<AuditState> {
        // 端口 1 上没有服务 → 连接**立刻被拒**（不是超时）→ 处理器会拿到「库不可用」。
        let db = TenantDb::connect(
            "postgres://base:base@127.0.0.1:1/base_test",
            DbOptions::default(),
        )
        .await
        .expect("连接串应当能解析");
        Arc::new(AuditState {
            query: Arc::new(AuditQueryService::new(db.clone())),
            verifier: Arc::new(ChainVerifier::new(db.clone())),
            archive: Arc::new(ArchiveService::new(db)),
            tokens: Arc::new(FixedTokens),
            checker: Arc::new(AllowAll),
            permissions: Arc::new(AllowAll),
        })
    }

    /// 🔑 端到端地打一次 `/api/audit/events`。
    ///
    /// **这条用例盯的是「提取器接线」**：处理器用 `ReqCtx`（`FromRequestParts`）拿上下文，
    /// 而**不是** `Extension<ReqCtx>` —— 后者要求扩展里**有** `ReqCtx`，
    /// 而中间件只插了 `RequestContext` → 提取失败 → axum 直接给 **500**（纯文本）。
    /// 这类 bug 编译期**看不出来**，只能真的打一次请求。
    #[tokio::test]
    async fn the_events_route_reaches_the_handler_instead_of_failing_to_extract() {
        let state = state_for_test().await;
        let app: Router<()> = Router::new().nest("/api/audit", router(state));

        let request = Request::builder()
            .uri("/api/audit/events")
            .header("authorization", "Bearer test-token")
            .body(axum::body::Body::empty())
            .expect("应当能构造请求");
        let response = app.oneshot(request).await.expect("应当能跑完路由");

        // 处理器**跑到了**（它去查库 → 库不可用 → 503 `AUDIT_SINK_UNAVAILABLE`）。
        // ⚠️ 如果提取器接线坏了，这里会是 **500**（axum 的「扩展缺失」）。
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "处理器必须被真正调用（库不可用 → 503），而不是提取器失败（500）"
        );
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("应当能读出响应体");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("AUDIT_SINK_UNAVAILABLE"),
            "响应体应当是 ProblemDetails（含审计错误码）：{text}"
        );
    }

    /// 没有令牌 → **401**（不是 403、不是 500）。
    #[tokio::test]
    async fn a_request_without_a_token_is_unauthenticated() {
        let state = state_for_test().await;
        let app: Router<()> = Router::new().nest("/api/audit", router(state));

        let request = Request::builder()
            .uri("/api/audit/events")
            .body(axum::body::Body::empty())
            .expect("应当能构造请求");
        let response = app.oneshot(request).await.expect("应当能跑完路由");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn the_bearer_token_is_parsed_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer abc".parse().expect("应当是合法头值"),
        );
        assert_eq!(bearer_token(&headers).as_deref(), Some("abc"));
        headers.insert(
            header::AUTHORIZATION,
            "bearer  xyz ".parse().expect("应当是合法头值"),
        );
        assert_eq!(bearer_token(&headers).as_deref(), Some("xyz"));
        headers.insert(
            header::AUTHORIZATION,
            "Basic abc".parse().expect("应当是合法头值"),
        );
        assert_eq!(bearer_token(&headers), None);
    }

    /// 响应字段名必须是 camelCase（D167）。
    #[test]
    fn the_event_view_uses_camel_case_field_names() {
        let row = AuditEventRow {
            id: "e-1".to_string(),
            actor_id: Some("u-1".to_string()),
            action: "demo.item.update".to_string(),
            object_type: "demo_item".to_string(),
            object_id: Some("obj-1".to_string()),
            before: Some(r#"{"name":"old"}"#.to_string()),
            after: Some(r#"{"name":"new"}"#.to_string()),
            ip: "203.0.113.9".to_string(),
            user_agent: "audit-test-agent".to_string(),
            created_at: OffsetDateTime::now_utc(),
            prev_hash: "0".repeat(64),
            hash: "a".repeat(64),
        };
        let value = serde_json::to_value(to_view(&row)).expect("应当能序列化");
        for key in [
            "id",
            "actorId",
            "action",
            "objectType",
            "objectId",
            "before",
            "after",
            "ip",
            "userAgent",
            "createdAt",
            "prevHash",
            "hash",
        ] {
            assert!(value.get(key).is_some(), "响应必须含 `{key}`：{value}");
        }
        // `before` / `after` 是 **JSON 值**（不是 JSON 文本）。
        assert_eq!(value["before"]["name"], "old");
        assert_eq!(value["after"]["name"], "new");
    }
}
