//! 请假模块的 HTTP 层：4 个端点，**每个路由组都挂鉴权层**。
//!
//! | 方法 | 路径 | 权限点 |
//! |---|---|---|
//! | `GET` | `/api/leave/requests` | `leave:request:read` |
//! | `POST` | `/api/leave/requests` | `leave:request:write` |
//! | `POST` | `/api/leave/requests/{id}/submit` | `leave:request:write` |
//! | `POST` | `/api/leave/requests/{id}/decide` | `leave:request:approve` |
//!
//! # 三条硬约束
//!
//! 1. `require_permission` 的参数必须是**字面量** —— 契约检查 C-5 要能静态确认
//!    「这个权限点真的有鉴权调用点」，常量参数会被判**失败**（fail closed）；
//! 2. 非法流转返回 **409 + `LEAVE_INVALID_TRANSITION`**（AC-S-189 N3）；
//! 3. 别的租户的单子与「不存在」**同一个结果**（404），不泄露存在性。

use std::sync::Arc;

use axum::extract::{FromRequestParts, OriginalUri, Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use platform_core::RequestContext;
use platform_core::extension::audit::AuditSink;

use platform_core::extension::permission::require_permission;
use platform_core::extension::repository::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};
use platform_core::problem::{ProblemDetails, problem_response};
use platform_core::titles::title_of_code;
use platform_db::TenantDb;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::entity::{LeaveRequest, Status, parse_date};
use crate::repository::PgLeaveRepository;
use crate::service::{Decision, LeaveError, LeaveService, NewLeaveRequest, Notifier};

/// 状态机非法流转的错误码（409）。
pub use crate::service::CODE_INVALID_TRANSITION;

/// HTTP 层共享状态。
///
/// 两个横切口（审计 / 通知）都是 `Option`：未装配时业务照常工作，
/// 只是不假装发过 —— 真实适配器由平台装配层注入（D354 的 `DbNotificationSender`）。
#[derive(Clone)]
pub struct LeaveState {
    /// 租户作用域的数据访问层。
    pub db: TenantDb,
    /// 平台审计落点。
    pub audit: Option<Arc<dyn AuditSink>>,
    /// 平台通知投递（`NotificationSender` 装箱后的注入口见 [`Notifier`]）。
    pub notifier: Option<Arc<dyn Notifier>>,
}

impl std::fmt::Debug for LeaveState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LeaveState")
            .field("audit", &self.audit.is_some())
            .field("notifier", &self.notifier.is_some())
            .finish_non_exhaustive()
    }
}

impl LeaveState {
    /// 只有数据库的初始状态。
    pub fn new(db: TenantDb) -> Self {
        Self {
            db,
            audit: None,
            notifier: None,
        }
    }

    /// 注入审计落点。
    pub fn with_audit(mut self, sink: Arc<dyn AuditSink>) -> Self {
        self.audit = Some(sink);
        self
    }

    /// 注入通知投递。
    ///
    /// 参数是**平台的** `NotificationSender` 实现（落库适配器由平台补，见 REQ-020 D354），
    /// 这里只把它装箱成可存储的对象 —— 业务侧不写投递逻辑。
    pub fn with_notifier<N>(mut self, sender: N) -> Self
    where
        N: platform_core::extension::notification::NotificationSender + 'static,
    {
        self.notifier = Some(Arc::new(sender));
        self
    }

    /// 由状态构造服务。
    pub fn service(&self) -> LeaveService {
        LeaveService::new(
            PgLeaveRepository::new(self.db.clone()),
            self.audit.clone(),
            self.notifier.clone(),
        )
    }
}

/// 请求上下文提取器（平台在认证层注入）。
#[derive(Debug, Clone)]
pub struct Ctx(pub RequestContext);

impl<S: Send + Sync> FromRequestParts<S> for Ctx {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // 认证层没跑时是**空上下文**：没有租户 → 数据访问会报错（fail closed）。
        Ok(Ctx(parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new)))
    }
}

/// 建单请求体（`applicantId` / `status` **不接受**客户端指定）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBody {
    /// 起始日期，`YYYY-MM-DD`。
    pub starts_on: String,
    /// 结束日期，`YYYY-MM-DD`。
    pub ends_on: String,
    /// 请假理由。
    pub reason: String,
}

/// 审批请求体：`{"decision":"approve"}` 或 `{"decision":"reject"}`。
#[derive(Debug, Deserialize)]
pub struct DecideBody {
    /// 审批决定。
    pub decision: Decision,
}

/// 列表分页参数。
#[derive(Debug, Deserialize)]
pub struct ListParams {
    /// 每页条数（缺省 20，上限 200）。
    pub limit: Option<u32>,
}

/// 请假单出参。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveResponse {
    /// 主键。
    pub id: String,
    /// 申请人。
    pub applicant_id: String,
    /// 起始日期。
    pub starts_on: String,
    /// 结束日期。
    pub ends_on: String,
    /// 理由。
    pub reason: String,
    /// 状态。
    pub status: Status,
    /// 审批人。
    pub decided_by: Option<String>,
    /// 审批时间（RFC 3339）。
    pub decided_at: Option<String>,
    /// 创建时间（RFC 3339）。
    pub created_at: String,
    /// 更新时间（RFC 3339）。
    pub updated_at: String,
}

impl From<LeaveRequest> for LeaveResponse {
    fn from(request: LeaveRequest) -> Self {
        Self {
            id: request.id,
            applicant_id: request.applicant_id,
            starts_on: request.starts_on.to_string(),
            ends_on: request.ends_on.to_string(),
            reason: request.reason,
            status: request.status,
            decided_by: request.decided_by,
            decided_at: request.decided_at.map(format_timestamp),
            created_at: format_timestamp(request.created_at),
            updated_at: format_timestamp(request.updated_at),
        }
    }
}

/// `timestamptz` → RFC 3339（契约 C13：时间一律 UTC 进出）。
fn format_timestamp(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}

/// 构造请假模块的路由。
///
/// **一组路由一个 `route_layer`**：读、写、审批是三条不同的权限边界。
pub fn router<S2>(state: Arc<LeaveState>) -> Router<S2> {
    let read_routes = Router::new().route("/requests", get(list_requests));
    let write_routes = Router::new()
        .route("/requests", post(create_request))
        .route("/requests/{id}/submit", post(submit_request));
    let approve_routes = Router::new().route("/requests/{id}/decide", post(decide_request));

    Router::new()
        .merge(read_routes.route_layer(require_permission("leave:request:read")))
        .merge(write_routes.route_layer(require_permission("leave:request:write")))
        .merge(approve_routes.route_layer(require_permission("leave:request:approve")))
        .with_state(state)
}

/// 列表。
async fn list_requests(
    State(state): State<Arc<LeaveState>>,
    Ctx(ctx): Ctx,
    Query(params): Query<ListParams>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    let limit = params.limit.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE);
    match state.service().list(&ctx, limit).await {
        Ok(requests) => {
            let body: Vec<LeaveResponse> = requests.into_iter().map(LeaveResponse::from).collect();
            Json(body).into_response()
        }
        Err(error) => problem(&error, &ctx, uri.path()),
    }
}

/// 建单（初始状态 `draft`）。
async fn create_request(
    State(state): State<Arc<LeaveState>>,
    Ctx(ctx): Ctx,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<CreateBody>,
) -> Response {
    let (Some(starts_on), Some(ends_on)) = (parse_date(&body.starts_on), parse_date(&body.ends_on))
    else {
        return problem(
            &LeaveError::InvalidInput("startsOn/endsOn"),
            &ctx,
            uri.path(),
        );
    };
    let input = NewLeaveRequest {
        starts_on,
        ends_on,
        reason: body.reason,
    };
    match state.service().create(&ctx, input).await {
        Ok(request) => (
            axum::http::StatusCode::CREATED,
            Json(LeaveResponse::from(request)),
        )
            .into_response(),
        Err(error) => problem(&error, &ctx, uri.path()),
    }
}

/// 提交（`draft → pending`）。
async fn submit_request(
    State(state): State<Arc<LeaveState>>,
    Ctx(ctx): Ctx,
    Path(id): Path<String>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    match state.service().submit(&ctx, &id).await {
        Ok(request) => Json(LeaveResponse::from(request)).into_response(),
        Err(error) => problem(&error, &ctx, uri.path()),
    }
}

/// 审批（`pending → approved / rejected`）。
async fn decide_request(
    State(state): State<Arc<LeaveState>>,
    Ctx(ctx): Ctx,
    Path(id): Path<String>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<DecideBody>,
) -> Response {
    match state.service().decide(&ctx, &id, body.decision).await {
        Ok(request) => Json(LeaveResponse::from(request)).into_response(),
        Err(error) => problem(&error, &ctx, uri.path()),
    }
}

/// 服务层错误 → RFC 9457 错误体（`code` 稳定，`title` 取自平台标题表）。
fn problem(error: &LeaveError, ctx: &RequestContext, path: &str) -> Response {
    if matches!(error, LeaveError::Storage(_) | LeaveError::Audit(_)) {
        // 内部错误只进日志：详情里已经有原因，但**不回显**给调用方以外的内容。
        tracing::error!(trace_id = %ctx.trace_id(), detail = %error.detail(), "leave request failed");
    }
    let code = error.code();
    problem_response(&ProblemDetails::from_context(
        code,
        title_of_code(code),
        error.status(),
        error.detail(),
        ctx,
        path,
        None,
    ))
}
