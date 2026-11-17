//! 扩展点 4：通知投递。
//!
//! 本期只定义 trait 与配套类型；通知通道的实际消费方在 REQ-015。

use std::collections::BTreeMap;
use std::future::Future;

use anyhow::Result;

use crate::context::RequestContext;

/// 通知消息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationMessage {
    /// 标题。
    pub title: String,
    /// 正文。
    pub body: String,
    /// 接收人。
    pub recipients: Vec<String>,
    /// 模板变量。
    pub payload: BTreeMap<String, String>,
}

impl NotificationMessage {
    /// 新建一条通知消息。
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            recipients: Vec::new(),
            payload: BTreeMap::new(),
        }
    }

    /// 添加接收人。
    pub fn with_recipient(mut self, recipient: impl Into<String>) -> Self {
        self.recipients.push(recipient.into());
        self
    }

    /// 添加模板变量。
    pub fn with_payload(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.payload.insert(key.into(), value.into());
        self
    }
}

/// 投递结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryResult {
    /// 是否投递成功。
    pub delivered: bool,
    /// 通道返回的消息 ID。
    pub provider_message_id: Option<String>,
    /// 失败原因（成功时为空）。
    pub error: Option<String>,
}

impl DeliveryResult {
    /// 投递成功。
    pub fn delivered(provider_message_id: impl Into<String>) -> Self {
        Self {
            delivered: true,
            provider_message_id: Some(provider_message_id.into()),
            error: None,
        }
    }

    /// 投递失败。
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            delivered: false,
            provider_message_id: None,
            error: Some(error.into()),
        }
    }
}

/// 通知投递扩展点。
pub trait NotificationSender: Send + Sync {
    /// 通道标识，与 `manifest.json` 的 `config` 中声明的通道对应。
    fn channel(&self) -> &str;

    /// 投递一条通知。
    fn send(
        &self,
        ctx: &RequestContext,
        msg: &NotificationMessage,
    ) -> impl Future<Output = Result<DeliveryResult>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_message_collects_recipients_and_payload() {
        let msg = NotificationMessage::new("title", "body")
            .with_recipient("user_a@example.com")
            .with_payload("code", "1234");
        assert_eq!(msg.recipients, vec!["user_a@example.com".to_string()]);
        assert_eq!(msg.payload.get("code").map(String::as_str), Some("1234"));
    }

    #[test]
    fn delivery_result_marks_success_and_failure() {
        let ok = DeliveryResult::delivered("msg-1");
        assert!(ok.delivered);
        assert_eq!(ok.provider_message_id.as_deref(), Some("msg-1"));
        assert!(ok.error.is_none());

        let err = DeliveryResult::failed("smtp timeout");
        assert!(!err.delivered);
        assert_eq!(err.error.as_deref(), Some("smtp timeout"));
    }
}
