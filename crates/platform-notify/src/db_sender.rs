//! **落库适配器**：把平台的通知扩展点接到 `platform_notify_delivery`（REQ-020 D354）。
//!
//! # 补的是什么缺口（B4 的实测）
//!
//! `platform_core::extension::notification::NotificationSender` 只有
//! `channel()` / `send()`（`notification.rs:81-91`）——**没有入队方法**。
//! 而 `platform_notify_delivery` 的唯一写入入口是本 crate 的
//! [`crate::service::NotifyService::enqueue`]。
//!
//! ```text
//! 业务 handler → NotificationSender::send()
//!                      ↓（本适配器）
//!                NotifyService::enqueue → platform_notify_delivery
//! ```
//!
//! 业务侧只依赖 `platform_core` 的 trait（AC-S-193），落库由平台做
//! （AC-S-189 的通知计数可查真表）。
//!
//! # ⚠️ 两个**实测的**限制（不是本适配器能消除的）
//!
//! 1. **必须有模板**：`enqueue` 的第 ① 步是
//!    `resolve_template(code, channel, locale)`（`service.rs:196-199`），
//!    找不到模板 → `NOTIFY_TEMPLATE_NOT_FOUND`。
//!    → 所以本适配器的构造参数是**模板编码**，不是「标题 + 正文」：
//!    `NotificationMessage.title` / `.body` **不参与渲染**（模板才是渲染来源，
//!    这是 D293「受限变量替换」的契约决定的，不能改成直接写死正文）。
//! 2. **收件人要由调用方给出**：`enqueue` 的接收人是单一字符串
//!    （`EnqueueRequest.recipient`），没有「按权限点解析审批人」的能力。
//!    → `NotificationMessage.recipients` 为空时**不发**（返回
//!    `delivered: false`），不会凭 `payload["audience"]` 猜收件人。
//!
//! # 语义
//!
//! - 一个收件人 → 一条 `platform_notify_delivery` 记录（逐条入队，各自独立）；
//! - **不是整体事务**：第 3 个收件人失败时前 2 条已经落库（`enqueue` 每条自己的
//!   事务）。失败通过 `Err` 冒给调用方，由其决定是否重试；
//! - `provider_message_id` 是**本次入队的全部投递记录主键**，逗号分隔
//!   （单收件人时就是一个 id）—— 便于排障时直接对上表。

use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use platform_core::RequestContext;
use platform_core::extension::notification::{DeliveryResult, NotificationMessage, NotificationSender};

use crate::model::channel;
use crate::service::{EnqueueRequest, NotifyService};

/// 把通知**落库**的 `NotificationSender` 实现（站内信通道）。
///
/// 装配层构造一次，之后交给业务模块的注入口
/// （`modules/leave` 的 `LeaveState::with_notifier`）。
#[derive(Clone)]
pub struct DbNotificationSender {
    /// 编排服务（它持有 `platform_notify_delivery` 的唯一写入口）。
    service: Arc<NotifyService>,
    /// 用哪一条模板渲染（`platform_notify_template.code`）。
    template_code: String,
    /// 渲染用的 locale 偏好（`None` = 只走「租户默认 locale」那一级回退）。
    locale: Option<String>,
}

impl DbNotificationSender {
    /// 用「编排服务 + 模板编码」构造。
    pub fn new(service: Arc<NotifyService>, template_code: impl Into<String>) -> Self {
        Self {
            service,
            template_code: template_code.into(),
            locale: None,
        }
    }

    /// 指定用户的 locale 偏好（三级回退的第 1 级，D294）。
    pub fn with_locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = Some(locale.into());
        self
    }

    /// 模板编码（装配层自检 / 排障用）。
    pub fn template_code(&self) -> &str {
        &self.template_code
    }
}

impl NotificationSender for DbNotificationSender {
    /// 本适配器只做站内信（`in_app`）——它落的是平台自己的库。
    ///
    /// 邮件 / Webhook 的投递由 worker 从同一条记录里取（`channel` 列决定
    /// 用哪个 provider），不在 `send()` 里同步发。
    fn channel(&self) -> &str {
        channel::IN_APP
    }

    fn send(
        &self,
        ctx: &RequestContext,
        msg: &NotificationMessage,
    ) -> impl Future<Output = Result<DeliveryResult>> + Send {
        // 先拷出需要的字段：`async move` 里不再借用入参之外的东西。
        let recipients = msg.recipients.clone();
        let payload = msg.payload.clone();
        let service = self.service.clone();
        let template_code = self.template_code.clone();
        let locale = self.locale.clone();

        async move {
            // 空收件人 = 「没有投递对象」：**合法失败**，不是 `Err`
            // （与 `channels/in_app.rs:55-62` 同一语义 —— `Err` 会被误当成可重试）。
            if recipients.is_empty() {
                return Ok(DeliveryResult::failed("the notification has no recipients"));
            }

            let mut delivery_ids: Vec<String> = Vec::with_capacity(recipients.len());
            for recipient in recipients {
                let request = EnqueueRequest {
                    template_code: template_code.clone(),
                    channel: channel::IN_APP.to_string(),
                    recipient,
                    locale: locale.clone(),
                    payload: payload.clone(),
                };
                // 失败**立刻**冒出去：吞掉会让业务以为通知已经排上了。
                let outcome = service.enqueue(ctx, &request).await.map_err(|error| {
                    anyhow::anyhow!(
                        "the notification could not be enqueued ({}): {}",
                        error.code(),
                        error.detail()
                    )
                })?;
                delivery_ids.push(outcome.delivery.id);
            }

            Ok(DeliveryResult::delivered(delivery_ids.join(",")))
        }
    }
}

impl std::fmt::Debug for DbNotificationSender {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DbNotificationSender")
            .field("template_code", &self.template_code)
            .field("locale", &self.locale)
            .finish_non_exhaustive()
    }
}
