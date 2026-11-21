//! 站内信（`in_app`）通道的真实现（REQ-015，D292）。
//!
//! # 为什么是「内存记录器」
//!
//! 站内信**不需要外部服务** —— 它最终落在平台自己的库里。本期把「落库」的
//! 边界留在编排层（它已经有投递记录表 `platform_notify_delivery`）：
//! 这个通道的职责只是「确认这条通知对收件人可见」并回一个消息 ID。
//!
//! 为了让判据可测，这里把「已投递的消息」记进一个
//! `Arc<Mutex<Vec<Recorded>>>`：[`InAppSender::delivered`] 让测试直接读回，
//! 不必连库、也不必猜编排层的状态机。
//!
//! # 失败语义
//!
//! - **收件人为空** → `Ok(DeliveryResult { delivered: false, error: Some(..) })`。
//!   这是「投递跑完了、但没有投递对象」——**合法失败**，不是 `Err`：
//!   worker 重试一次也不会有收件人冒出来，所以 `Err` 只会让人误以为可重试。
//!   实际上空收件人是入队前就该拦掉的情况，这里只做纵深防御。

use std::future::Future;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use platform_core::RequestContext;
use platform_core::extension::notification::{DeliveryResult, NotificationMessage};

/// 一条**已投递**的站内信（测试与排障可读的快照）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// 收件人列表（逐字保留，用于核对）。
    pub recipients: Vec<String>,
    /// 标题。
    pub title: String,
    /// 正文。
    pub body: String,
    /// 平台生成的消息 ID。
    pub provider_message_id: String,
}

/// 站内信投递端。
///
/// `Clone` 是**共享同一份记录**的克隆（内部是 `Arc`）——
/// 编排层把 sender 装进注册表，测试再拿一个克隆去读记录。
#[derive(Debug, Clone, Default)]
pub struct InAppSender {
    /// 已投递消息的记录（按投递顺序追加）。
    recorded: Arc<Mutex<Vec<Recorded>>>,
}

impl InAppSender {
    /// 新建一个站内信投递端（记录为空）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 已投递的全部站内信（按投递顺序）。
    ///
    /// 锁中毒时取回数据而不是 panic —— 记录器不该让请求失败。
    pub fn delivered(&self) -> Vec<Recorded> {
        self.recorded
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
}

impl platform_core::extension::notification::NotificationSender for InAppSender {
    fn channel(&self) -> &str {
        crate::model::channel::IN_APP
    }

    fn send(
        &self,
        _ctx: &RequestContext,
        msg: &NotificationMessage,
    ) -> impl Future<Output = Result<DeliveryResult>> + Send {
        // 先判定入参，再决定是「投递成功」还是「合法失败」。
        let outcome = if msg.recipients.is_empty() {
            DeliveryResult::failed("the notification has no recipients")
        } else {
            let provider_message_id = uuid::Uuid::new_v4().to_string();
            let record = Recorded {
                recipients: msg.recipients.clone(),
                title: msg.title.clone(),
                body: msg.body.clone(),
                provider_message_id: provider_message_id.clone(),
            };
            self.recorded
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(record);
            DeliveryResult::delivered(provider_message_id)
        };
        async move { Ok(outcome) }
    }
}

#[cfg(test)]
mod tests {
    use platform_core::RequestContext;
    use platform_core::extension::notification::{NotificationMessage, NotificationSender};

    use super::InAppSender;

    /// 投递一条 → `delivered == true`、带消息 ID、无错误。
    #[tokio::test]
    async fn a_delivered_message_reports_success() {
        let sender = InAppSender::new();
        let ctx = RequestContext::new();
        let msg = NotificationMessage::new("标题", "正文").with_recipient("user-1");

        let result = sender.send(&ctx, &msg).await.expect("站内信不该返回 Err");
        assert!(result.delivered, "收件人非空时必须投递成功");
        assert!(
            result.provider_message_id.is_some(),
            "成功投递必须带消息 ID"
        );
        assert!(result.error.is_none(), "成功时不得带错误：{result:?}");
    }

    /// 记录器里能查到刚才那条，且 `recipients` / `title` / `body` 逐字一致。
    #[tokio::test]
    async fn the_recorded_message_matches_the_input_verbatim() {
        let sender = InAppSender::new();
        let ctx = RequestContext::new();
        let msg = NotificationMessage::new("账号即将过期", "请在 7 天内修改密码")
            .with_recipient("user-1")
            .with_recipient("user-2");

        let result = sender.send(&ctx, &msg).await.expect("站内信不该返回 Err");
        let recorded = sender.delivered();
        assert_eq!(recorded.len(), 1, "应当只有刚投递的这一条");
        assert_eq!(recorded[0].recipients, vec!["user-1", "user-2"]);
        assert_eq!(recorded[0].title, "账号即将过期");
        assert_eq!(recorded[0].body, "请在 7 天内修改密码");
        assert_eq!(
            Some(recorded[0].provider_message_id.clone()),
            result.provider_message_id,
            "记录里的消息 ID 必须就是返回给调用方的那个"
        );
    }

    /// **收件人为空** → `delivered == false` 且 `error.is_some()`（不是 `Err`）。
    #[tokio::test]
    async fn an_empty_recipient_list_is_a_legal_failure() {
        let sender = InAppSender::new();
        let ctx = RequestContext::new();
        let msg = NotificationMessage::new("标题", "正文");

        let result = sender.send(&ctx, &msg).await.expect("空收件人不是 Err");
        assert!(!result.delivered, "没有收件人时不得报告投递成功");
        assert!(result.error.is_some(), "失败必须说明原因：{result:?}");
        assert!(result.provider_message_id.is_none(), "失败时不应有消息 ID");
        assert!(sender.delivered().is_empty(), "失败的投递不得进记录器");
    }

    /// 投两条 → 记录器里 **2 条**（只数最后一条会漏掉重复投递的 bug）。
    #[tokio::test]
    async fn two_deliveries_are_both_recorded() {
        let sender = InAppSender::new();
        let ctx = RequestContext::new();

        for (title, body) in [("第一条", "正文一"), ("第二条", "正文二")] {
            let msg = NotificationMessage::new(title, body).with_recipient("user-1");
            sender.send(&ctx, &msg).await.expect("站内信不该返回 Err");
        }

        let recorded = sender.delivered();
        assert_eq!(recorded.len(), 2, "两次投递都要留下记录");
        assert_eq!(recorded[0].title, "第一条");
        assert_eq!(recorded[1].title, "第二条");
        assert_ne!(
            recorded[0].provider_message_id, recorded[1].provider_message_id,
            "每次投递都要有独立的消息 ID"
        );
    }
}
