//! 组织变更事件的**通知投递**（复用 REQ-003 的 `NotificationSender` 扩展点）。
//!
//! # 为什么有一层适配器
//!
//! 平台的 `NotificationSender` 是 **RPITIT** 形态（`extension-points v1` §2），
//! **不可用于 trait object** —— `Box<dyn NotificationSender>` 编译不过。
//! 而服务层需要持有「可替换的投递端」（测试里要能断言「发了什么」）。
//!
//! 所以这里定义一个**可对象化**的 [`Notifier`]，并提供 [`PlatformNotifier`]
//! 把平台的 `NotificationSender` **转发**过去 —— 适配器只转发，不重写语义。
//!
//! # 谁该收到通知
//!
//! | 事件 | 通知对象 |
//! |------|---------|
//! | 用户被加入节点 | 该用户 + 该节点的成员 |
//! | 用户被移出节点 | 该用户 + 新旧节点的成员 |
//! | 节点移动 / 重命名 | 该节点（含子树）下的全部成员 |
//! | 节点删除 | 该节点下的全部成员（删除前已处理） |
//! | 用户离职 | 该用户 + 其直属上级 |

use std::sync::Arc;

use anyhow::Result;
use platform_core::RequestContext;
use platform_core::extension::notification::{
    DeliveryResult, NotificationMessage, NotificationSender,
};

/// 装箱的 future（让接缝保持**可对象化**）。
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// 通知投递接缝（可对象化）。
pub trait Notifier: Send + Sync {
    /// 投递一条通知。
    fn notify<'a>(
        &'a self,
        ctx: &'a RequestContext,
        message: &'a NotificationMessage,
    ) -> BoxFuture<'a, Result<DeliveryResult>>;
}

/// 平台 `NotificationSender` 的**适配器**（只转发，不重写语义）。
#[derive(Debug, Clone)]
pub struct PlatformNotifier<N: NotificationSender> {
    inner: Arc<N>,
}

impl<N: NotificationSender> PlatformNotifier<N> {
    /// 包一个平台的投递端。
    pub fn new(inner: Arc<N>) -> Self {
        Self { inner }
    }
}

impl<N: NotificationSender> Notifier for PlatformNotifier<N> {
    fn notify<'a>(
        &'a self,
        ctx: &'a RequestContext,
        message: &'a NotificationMessage,
    ) -> BoxFuture<'a, Result<DeliveryResult>> {
        Box::pin(async move { self.inner.send(ctx, message).await })
    }
}

/// 什么都不发的投递端（**默认**）。
///
/// 平台的通知通道在 REQ-015 才落地；在那之前，组织变更**照常发生**，
/// 只是没有渠道把它送出去 —— 不能因为「没渠道」就让组织操作失败。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn notify<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _message: &'a NotificationMessage,
    ) -> BoxFuture<'a, Result<DeliveryResult>> {
        Box::pin(async { Ok(DeliveryResult::delivered("noop")) })
    }
}

/// 只写日志的投递端（**默认**）。
///
/// 平台的通知通道在 REQ-015 才落地 —— 在那之前，组织变更事件**照常产生**，
/// 至少能在日志里看到「谁被通知了」，而不是悄悄消失。
#[derive(Debug, Clone, Copy, Default)]
pub struct LoggingNotifier;

impl Notifier for LoggingNotifier {
    fn notify<'a>(
        &'a self,
        ctx: &'a RequestContext,
        message: &'a NotificationMessage,
    ) -> BoxFuture<'a, Result<DeliveryResult>> {
        tracing::info!(
            trace_id = %ctx.trace_id(),
            title = %message.title,
            recipients = ?message.recipients,
            payload = ?message.payload,
            "组织变更事件（通知通道待 REQ-015 落地，当前只记日志）"
        );
        Box::pin(async { Ok(DeliveryResult::delivered("logging")) })
    }
}

/// 组织变更事件的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrgEventKind {
    /// 用户被加入组织节点。
    MemberAdded,
    /// 用户被移出组织节点。
    MemberRemoved,
    /// 节点被移动。
    NodeMoved,
    /// 节点被重命名。
    NodeRenamed,
    /// 节点被删除。
    NodeDeleted,
    /// 用户离职（自动禁用）。
    UserDeparted,
}

impl OrgEventKind {
    /// 稳定的事件码（审计与通知共用）。
    pub fn code(&self) -> &'static str {
        match self {
            OrgEventKind::MemberAdded => "org.member-added",
            OrgEventKind::MemberRemoved => "org.member-removed",
            OrgEventKind::NodeMoved => "org.node-moved",
            OrgEventKind::NodeRenamed => "org.node-renamed",
            OrgEventKind::NodeDeleted => "org.node-deleted",
            OrgEventKind::UserDeparted => "org.user-departed",
        }
    }

    /// 通知标题（**面向开发者**，中文文案由前端 i18n 按 code 渲染）。
    pub fn title(&self) -> &'static str {
        match self {
            OrgEventKind::MemberAdded => "organization membership added",
            OrgEventKind::MemberRemoved => "organization membership removed",
            OrgEventKind::NodeMoved => "organization node moved",
            OrgEventKind::NodeRenamed => "organization node renamed",
            OrgEventKind::NodeDeleted => "organization node deleted",
            OrgEventKind::UserDeparted => "user departed",
        }
    }
}

/// 组织变更事件的消息（收件人 + 载荷）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgEvent {
    /// 事件种类。
    pub kind: OrgEventKind,
    /// 收件人（用户 ID）。
    pub recipients: Vec<String>,
    /// 载荷（节点 ID / 名称 / 用户 ID …）。
    pub payload: Vec<(String, String)>,
}

impl OrgEvent {
    /// 造一个事件。
    pub fn new(kind: OrgEventKind, recipients: Vec<String>) -> Self {
        Self {
            kind,
            recipients,
            payload: Vec::new(),
        }
    }

    /// 加一个载荷项。
    pub fn with_payload(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.payload.push((key.into(), value.into()));
        self
    }

    /// 转成平台的通知消息。
    pub fn to_message(&self) -> NotificationMessage {
        let mut message = NotificationMessage::new(self.kind.title(), self.kind.title())
            .with_payload("eventCode", self.kind.code());
        for recipient in &self.recipients {
            message = message.with_recipient(recipient.clone());
        }
        for (key, value) in &self.payload {
            message = message.with_payload(key.clone(), value.clone());
        }
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 平台的投递端（测试替身）。
    #[derive(Debug, Default)]
    struct RecordingSender {
        sent: Mutex<Vec<NotificationMessage>>,
    }

    impl NotificationSender for RecordingSender {
        fn channel(&self) -> &str {
            "test"
        }

        fn send(
            &self,
            _ctx: &RequestContext,
            message: &NotificationMessage,
        ) -> impl std::future::Future<Output = Result<DeliveryResult>> + Send {
            let mut sent = self.sent.lock().unwrap_or_else(|err| err.into_inner());
            sent.push(message.clone());
            async { Ok(DeliveryResult::delivered("sender")) }
        }
    }

    #[tokio::test]
    async fn the_platform_sender_is_forwarded_through_the_adapter() {
        let sender = Arc::new(RecordingSender::default());
        let notifier = PlatformNotifier::new(sender.clone());
        let ctx = RequestContext::new().with_tenant("tenant-a");
        let message = NotificationMessage::new("t", "b").with_recipient("u1");

        let result = notifier.notify(&ctx, &message).await.expect("投递应成功");
        assert!(result.delivered);
        assert_eq!(sender.sent.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_logging_notifier_never_fails() {
        let notifier = LoggingNotifier;
        let ctx = RequestContext::new().with_tenant("tenant-a");
        let message = NotificationMessage::new("t", "b");
        let result = notifier.notify(&ctx, &message).await.expect("不应失败");
        assert!(result.delivered);
    }

    #[tokio::test]
    async fn the_noop_notifier_never_fails() {
        let notifier = NoopNotifier;
        let ctx = RequestContext::new().with_tenant("tenant-a");
        let message = NotificationMessage::new("t", "b");
        let result = notifier.notify(&ctx, &message).await.expect("不应失败");
        assert!(result.delivered);
    }

    #[test]
    fn an_org_event_carries_its_recipients_and_payload() {
        let event = OrgEvent::new(OrgEventKind::MemberRemoved, vec!["u1".to_string()])
            .with_payload("orgNodeId", "dept-1")
            .with_payload("orgNodeName", "Engineering");
        let message = event.to_message();
        assert_eq!(message.recipients, vec!["u1".to_string()]);
        assert_eq!(
            message.payload.get("eventCode").map(String::as_str),
            Some("org.member-removed")
        );
        assert_eq!(
            message.payload.get("orgNodeName").map(String::as_str),
            Some("Engineering")
        );
    }

    #[test]
    fn every_event_kind_has_a_stable_code() {
        let kinds = [
            OrgEventKind::MemberAdded,
            OrgEventKind::MemberRemoved,
            OrgEventKind::NodeMoved,
            OrgEventKind::NodeRenamed,
            OrgEventKind::NodeDeleted,
            OrgEventKind::UserDeparted,
        ];
        for kind in kinds {
            assert!(kind.code().starts_with("org."));
            assert!(
                kind.code()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '-')
            );
        }
    }
}
