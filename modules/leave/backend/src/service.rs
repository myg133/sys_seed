//! 请假审批的**业务状态机** + 编排（D347：审批流是业务逻辑，不是平台能力）。
//!
//! # 状态机（AC-S-189）
//!
//! ```text
//! draft ──submit──► pending ──approve──► approved（终态）
//!                     └──reject──► rejected（终态）
//! ```
//!
//! 非法流转**一律**返回 [`CODE_INVALID_TRANSITION`]（HTTP 409）。
//! 「非法」的完整清单只有 5 条（只有 `submit` / `decide` 两个动作端点）：
//!
//! | 当前状态 | 动作 | 结果 |
//! |---|---|---|
//! | `pending` | `submit` | 409 |
//! | `approved` | `submit` | 409 |
//! | `approved` | `decide` | 409 |
//! | `rejected` | `submit` | 409 |
//! | `rejected` | `decide` | 409 |
//!
//! # 横切能力全部来自平台（AC-S-188 ⑤⑥⑦）
//!
//! - 审计：`AuditEvent` 交给**注入的** [`AuditSink`]（业务不写审计表 SQL）；
//! - 通知：`NotificationMessage` 交给**注入的** [`NotificationSender`]
//!   （业务**不依赖** `platform-notify`）。
//!
//! 两个注入口都是 `Option`（未装配时不假装发过）：审计由平台 sink 填，
//! 通知由平台装配层注入的落库适配器填（`DbNotificationSender`，D354）。

use std::sync::Arc;

use platform_core::context::RequestContext;
use platform_core::extension::audit::{AuditEvent, AuditSink, BoxFuture};
use platform_core::extension::notification::{
    DeliveryResult, NotificationMessage, NotificationSender,
};
use serde::Deserialize;
use time::{Date, OffsetDateTime};

use crate::entity::{LeaveRequest, Status};
use crate::permission::PERMISSION_REQUEST_APPROVE;
use crate::repository::{PgLeaveRepository, StatusUpdate};

/// 非法流转的稳定错误码（AC-S-189 N3 写死这个值）。
pub const CODE_INVALID_TRANSITION: &str = "LEAVE_INVALID_TRANSITION";

/// 审计对象类型。
pub const OBJECT_TYPE: &str = "leave.request";

/// 审批结果通知用的**模板编码**（`platform_notify_template.code`）。
///
/// # 为什么业务模块自己声明它
///
/// 模板是**租户级数据**（`platform_notify_template` 有 `tenant_id`，按
/// `(tenant_id, code, channel, locale)` 唯一），由租户自己种 —— 平台不预置任何模板。
/// 所以「用哪条模板」是业务方的契约：装配层把它交给
/// `DbNotificationSender`（D354），业务代码里**没有**一行投递逻辑。
///
/// 通道固定 `in_app`（适配器只落站内信），locale 走三级回退（D294）：
/// 用户偏好 → 租户默认 → 失败。租户至少要种 **zh-CN 与 en-US** 两条，
/// 否则非基准语言的收件人会拿到 `NOTIFY_TEMPLATE_NOT_FOUND`。
pub const NOTIFICATION_TEMPLATE: &str = "leave_decision";

/// 建单的审计动作（创建 = 状态初始化，算 1 条审计、0 条通知）。
pub const ACTION_CREATED: &str = "leave.request.created";

/// 提交的审计动作。
pub const ACTION_SUBMITTED: &str = "leave.request.submitted";

/// 审批通过的审计动作。
pub const ACTION_APPROVED: &str = "leave.request.approved";

/// 驳回的审计动作。
pub const ACTION_REJECTED: &str = "leave.request.rejected";

/// 审批决定（`decide` 端点的请求体取值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// 通过。
    Approve,
    /// 驳回。
    Reject,
}

impl Decision {
    /// 面向开发者的动作名（进错误详情）。
    pub fn action(self) -> &'static str {
        match self {
            Decision::Approve => "approve",
            Decision::Reject => "reject",
        }
    }

    /// 落库的目标状态。
    pub fn next_status(self) -> Status {
        match self {
            Decision::Approve => Status::Approved,
            Decision::Reject => Status::Rejected,
        }
    }

    /// 审计动作。
    pub fn audit_action(self) -> &'static str {
        match self {
            Decision::Approve => ACTION_APPROVED,
            Decision::Reject => ACTION_REJECTED,
        }
    }
}

/// 通知投递的**对象化**包装。
///
/// # 为什么业务侧要这一层
///
/// 平台的 [`NotificationSender::send`] 返回 `impl Future` —— 该 trait **不是
/// dyn-compatible**，所以 `Arc<dyn NotificationSender>` 写不出来（实测 E0038）。
/// 业务侧要「一个可存储的注入口」，就只能自己把 future 装箱：
///
/// ```text
/// with_notifier(impl NotificationSender)  →  Arc<dyn Notifier>
/// ```
///
/// 这不是自造轮子：装箱后的实现**仍然调平台的 `send()`**，
/// 业务代码里没有一行通知投递逻辑（AC-S-188 ⑦⑧ 同时成立）。
pub trait Notifier: Send + Sync {
    /// 投递一条通知（对象化签名）。
    fn notify<'a>(
        &'a self,
        ctx: &'a RequestContext,
        message: &'a NotificationMessage,
    ) -> BoxFuture<'a, Result<DeliveryResult, anyhow::Error>>;
}

impl<T: NotificationSender> Notifier for T {
    fn notify<'a>(
        &'a self,
        ctx: &'a RequestContext,
        message: &'a NotificationMessage,
    ) -> BoxFuture<'a, Result<DeliveryResult, anyhow::Error>> {
        Box::pin(self.send(ctx, message))
    }
}

/// 建单入参（HTTP 层解析后交过来；**不含** `status` / `applicant_id`）。
#[derive(Debug, Clone)]
pub struct NewLeaveRequest {
    /// 起始日期。
    pub starts_on: Date,
    /// 结束日期。
    pub ends_on: Date,
    /// 请假理由。
    pub reason: String,
}

/// 服务层错误：每种都有**稳定的错误码**（契约 C6）。
#[derive(Debug)]
pub enum LeaveError {
    /// 非法流转 → 409 `LEAVE_INVALID_TRANSITION`。
    InvalidTransition {
        /// 流转发起时的状态。
        from: Status,
        /// 被拒绝的动作（`submit` / `approve` / `reject`）。
        action: &'static str,
    },
    /// 记录不存在，或不属于当前租户（**两者同一个结果**，不泄露存在性）。
    NotFound,
    /// 入参不合法 → 422。
    InvalidInput(&'static str),
    /// 数据访问失败 → 500。
    Storage(anyhow::Error),
    /// 审计写不进去 → 500（**不得**吞掉审计失败）。
    Audit(String),
}

impl LeaveError {
    /// 稳定错误码。
    pub fn code(&self) -> &'static str {
        match self {
            LeaveError::InvalidTransition { .. } => CODE_INVALID_TRANSITION,
            LeaveError::NotFound => platform_core::problem::CODE_NOT_FOUND,
            LeaveError::InvalidInput(_) => platform_core::problem::CODE_INVALID_REQUEST,
            LeaveError::Storage(_) | LeaveError::Audit(_) => {
                platform_core::problem::CODE_INTERNAL_ERROR
            }
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> u16 {
        match self {
            LeaveError::InvalidTransition { .. } => 409,
            LeaveError::NotFound => 404,
            LeaveError::InvalidInput(_) => 422,
            LeaveError::Storage(_) | LeaveError::Audit(_) => 500,
        }
    }

    /// 面向开发者的英文详情（不含任何业务数据回显）。
    pub fn detail(&self) -> String {
        match self {
            LeaveError::InvalidTransition { from, action } => format!(
                "cannot {action} a leave request in status `{}`",
                from.as_str()
            ),
            LeaveError::NotFound => "the leave request does not exist".to_string(),
            LeaveError::InvalidInput(field) => format!("the field `{field}` is invalid"),
            LeaveError::Storage(error) => format!("the data access layer failed: {error}"),
            LeaveError::Audit(error) => format!("the audit sink failed: {error}"),
        }
    }
}

/// 状态机：`submit` 动作。
pub fn submit_from(current: Status) -> Result<Status, LeaveError> {
    match current {
        Status::Draft => Ok(Status::Pending),
        other => Err(LeaveError::InvalidTransition {
            from: other,
            action: "submit",
        }),
    }
}

/// 状态机：`decide` 动作。
pub fn decide_from(current: Status, decision: Decision) -> Result<Status, LeaveError> {
    match current {
        Status::Pending => Ok(decision.next_status()),
        other => Err(LeaveError::InvalidTransition {
            from: other,
            action: decision.action(),
        }),
    }
}

/// 请假单服务：仓储 + 两个横切注入口。
#[derive(Clone)]
pub struct LeaveService {
    repository: PgLeaveRepository,
    audit: Option<Arc<dyn AuditSink>>,
    notifier: Option<Arc<dyn Notifier>>,
}

impl LeaveService {
    /// 构造服务（两个横切口都允许为 `None` —— 未装配时不假装发过审计/通知）。
    pub fn new(
        repository: PgLeaveRepository,
        audit: Option<Arc<dyn AuditSink>>,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Self {
        Self {
            repository,
            audit,
            notifier,
        }
    }

    /// 列出当前租户的请假单。
    pub async fn list(
        &self,
        ctx: &RequestContext,
        limit: u32,
    ) -> Result<Vec<LeaveRequest>, LeaveError> {
        self.repository
            .list(ctx, limit)
            .await
            .map_err(LeaveError::Storage)
    }

    /// 建单：状态初始化成 `draft`，**只**记审计，不发通知。
    pub async fn create(
        &self,
        ctx: &RequestContext,
        input: NewLeaveRequest,
    ) -> Result<LeaveRequest, LeaveError> {
        let tenant = tenant_of(ctx)?;
        let applicant = ctx
            .user_id()
            .ok_or(LeaveError::InvalidInput("applicantId"))
            .map(str::to_string)?;
        let reason = input.reason.trim().to_string();
        if reason.is_empty() {
            return Err(LeaveError::InvalidInput("reason"));
        }
        if input.ends_on < input.starts_on {
            return Err(LeaveError::InvalidInput("endsOn"));
        }
        let request = LeaveRequest::new(
            tenant,
            applicant,
            input.starts_on,
            input.ends_on,
            reason,
            OffsetDateTime::now_utc(),
        );
        self.repository
            .insert(ctx, &request)
            .await
            .map_err(LeaveError::Storage)?;
        self.record(ctx, ACTION_CREATED, &request, None).await?;
        Ok(request)
    }

    /// 提交：`draft → pending`，**不发通知**。
    ///
    /// # ⚠️ 为什么这里不发通知（AC-S-189 计数表第 2 行，TD-088）
    ///
    /// 原设想是「提交时提醒审批人去审批」，但平台**没有「按权限点反查用户」的能力**
    /// （`PermissionChecker` 只有 `has` / `scope` / `effective_scope`）——
    /// 业务方知道的是「谁持有 `leave:request:approve`」，不是「那是谁」。
    ///
    /// → 计数表规定提交 **0 条通知**；`notify(...)` 仍被调用，但只带
    /// `audience` 权限点、**不带收件人** → 落库适配器按「没有投递对象」
    /// 返回 `delivered: false`，**不写** `platform_notify_delivery`。
    /// 这样代码里保留了「待办提醒」的意图，等平台补上反查能力（TD-088）即可生效。
    pub async fn submit(&self, ctx: &RequestContext, id: &str) -> Result<LeaveRequest, LeaveError> {
        let current = self.load(ctx, id).await?;
        let next = submit_from(current.status)?;
        let now = OffsetDateTime::now_utc();
        let updated = self
            .apply(
                ctx,
                current,
                StatusUpdate {
                    next,
                    expected: Status::Draft,
                    decided_by: None,
                    decided_at: None,
                },
                now,
                "submit",
            )
            .await?;
        self.record(
            ctx,
            ACTION_SUBMITTED,
            &updated,
            Some(Status::Draft.as_str()),
        )
        .await?;
        self.notify(
            ctx,
            &updated,
            false,
            "A leave request is waiting for your review",
        )
        .await;
        Ok(updated)
    }

    /// 审批：`pending → approved / rejected`，通知申请人。
    pub async fn decide(
        &self,
        ctx: &RequestContext,
        id: &str,
        decision: Decision,
    ) -> Result<LeaveRequest, LeaveError> {
        let current = self.load(ctx, id).await?;
        let next = decide_from(current.status, decision)?;
        let now = OffsetDateTime::now_utc();
        let decided_by = ctx.user_id().map(str::to_string);
        let updated = self
            .apply(
                ctx,
                current,
                StatusUpdate {
                    next,
                    expected: Status::Pending,
                    decided_by: decided_by.as_deref(),
                    decided_at: Some(now),
                },
                now,
                decision.action(),
            )
            .await?;
        self.record(
            ctx,
            decision.audit_action(),
            &updated,
            Some(Status::Pending.as_str()),
        )
        .await?;
        self.notify(ctx, &updated, true, "Your leave request has been reviewed")
            .await;
        Ok(updated)
    }

    /// 读一条当前租户的单子；查不到 = 404（不泄露别的租户是否用过这个 ID）。
    async fn load(&self, ctx: &RequestContext, id: &str) -> Result<LeaveRequest, LeaveError> {
        self.repository
            .find(ctx, id)
            .await
            .map_err(LeaveError::Storage)?
            .ok_or(LeaveError::NotFound)
    }

    /// 条件更新（乐观锁）+ 回填内存里的那一条。
    ///
    /// `expected` 对不上 = 有人在这一次读写之间抢先流转了 → 同一个 409，
    /// 而不是把后到的判定覆盖上去。
    async fn apply(
        &self,
        ctx: &RequestContext,
        current: LeaveRequest,
        update: StatusUpdate<'_>,
        now: OffsetDateTime,
        action: &'static str,
    ) -> Result<LeaveRequest, LeaveError> {
        let expected = update.expected;
        let next = update.next;
        let decided_by = update.decided_by.map(str::to_string);
        let decided_at = update.decided_at;
        let applied = self
            .repository
            .transition(ctx, &current.id, &update, now)
            .await
            .map_err(LeaveError::Storage)?;
        if !applied {
            return Err(LeaveError::InvalidTransition {
                from: expected,
                action,
            });
        }
        Ok(LeaveRequest {
            status: next,
            decided_by: decided_by.or(current.decided_by),
            decided_at: decided_at.or(current.decided_at),
            updated_at: now,
            ..current
        })
    }

    /// 写一条审计事件（sink 未注入时**不发**，也不假装发过）。
    async fn record(
        &self,
        ctx: &RequestContext,
        action: &'static str,
        request: &LeaveRequest,
        before: Option<&'static str>,
    ) -> Result<(), LeaveError> {
        let Some(sink) = &self.audit else {
            return Ok(());
        };
        let mut event = AuditEvent::new(tenant_of(ctx)?, action, OBJECT_TYPE)
            .with_object_id(request.id.clone())
            .with_after(serde_json::json!({ "status": request.status.as_str() }));
        if let Some(actor) = ctx.user_id() {
            event = event.with_actor(actor);
        }
        if let Some(previous) = before {
            event = event.with_before(serde_json::json!({ "status": previous }));
        }
        sink.record(ctx, &event)
            .await
            .map_err(|error| LeaveError::Audit(error.to_string()))
    }

    /// 发一条通知（sender 未注入时跳过；发送失败只记日志，**不回滚**已落库的流转）。
    async fn notify(
        &self,
        ctx: &RequestContext,
        request: &LeaveRequest,
        to_applicant: bool,
        title: &str,
    ) {
        let Some(sender) = &self.notifier else {
            return;
        };
        let mut message = NotificationMessage::new(title, format!("leave request {}", request.id))
            .with_payload("leaveRequestId", request.id.clone())
            .with_payload("status", request.status.as_str());
        if to_applicant {
            message = message.with_recipient(request.applicant_id.clone());
        } else {
            // 审批人身份由平台适配器按权限点解析（业务侧不查权限表）。
            message = message.with_payload("audience", PERMISSION_REQUEST_APPROVE.to_string());
        }
        if let Err(error) = sender.notify(ctx, &message).await {
            tracing::warn!(
                request_id = %request.id,
                error = %error,
                "the leave notification could not be sent"
            );
        }
    }
}

/// 从请求上下文取租户（租户只来自 `ctx`，没有任何第二条途径）。
fn tenant_of(ctx: &RequestContext) -> Result<&str, LeaveError> {
    ctx.tenant_id().ok_or(LeaveError::InvalidInput("tenantId"))
}
