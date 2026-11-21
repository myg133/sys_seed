//! 通知**通道实现**与装配（REQ-015，D292 / D305）。
//!
//! # 这里放什么
//!
//! | 子模块 | 状态 |
//! |---|---|
//! | [`in_app`] | ✅ 真实现（站内信，落库） |
//! | [`smtp`] | ✅ 真实现（自己写的 SMTP 客户端，D305） |
//! | [`webhook`] | ✅ 真实现（`hyper` + `hyper-rustls`） |
//! | [`unconfigured`] | ⚠️ 4 个显式「未配置」（`sms` / `dingtalk` / `wecom` / `feishu`） |
//!
//! # [`ChannelRegistry`]
//!
//! 按通道名装配多个 provider，编排层（worker）只拿通道名查表，
//! **不知道**具体实现类型。
//!
//! # ⚠️ 为什么这里用 [`ChannelSender`] 而不是 `dyn NotificationSender`
//!
//! 平台的 `NotificationSender` 是 **RPITIT** 形态
//! （`fn send(...) -> impl Future<...>`，`notification.rs:86-91`）→
//! **不可用于 trait object**，`Arc<dyn NotificationSender>` **编译不过**。
//! → 用 [`crate::worker::ChannelSender`]（把返回类型装箱）+ [`crate::worker::PlatformChannel`]
//! 做转发（与 `platform-org/src/notify.rs:43` 的 `PlatformNotifier` 同款手法）。

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::worker::{ChannelLookup, ChannelSender};

pub mod in_app;
pub mod smtp;
pub mod unconfigured;
pub mod webhook;

/// 通道注册表：`channel()` 名 → provider。
///
/// 用 `BTreeMap` 而不是 `HashMap`：一是 [`Self::channels`] 天然有序（日志 / 断言稳定），
/// 二是通道数量只有个位数，查找性能无所谓。
#[derive(Default)]
pub struct ChannelRegistry {
    /// 已装配的 provider（键是 `channel()` 返回的名字）。
    senders: BTreeMap<String, Arc<dyn ChannelSender>>,
}

impl ChannelRegistry {
    /// 空注册表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 装配一个 provider，返回**被替换掉的**同通道 provider（若有）。
    ///
    /// 同名后写覆盖先写：装配层可以先用「未配置」占位，再用真实现替换（D292）。
    pub fn insert(
        &mut self,
        channel: &str,
        sender: Arc<dyn ChannelSender>,
    ) -> Option<Arc<dyn ChannelSender>> {
        self.senders.insert(channel.to_string(), sender)
    }

    /// 按通道名取 provider；未装配返回 `None`（调用方据此判「未配置」）。
    pub fn get(&self, channel: &str) -> Option<Arc<dyn ChannelSender>> {
        // ⚠️ 返回 `Arc` 的克隆而不是引用：投递是异步的，`Arc` 让 future 不必借 `&self`。
        self.senders.get(channel).cloned()
    }

    /// 已装配的通道名（**字典序**）。
    pub fn channels(&self) -> Vec<&str> {
        self.senders.keys().map(String::as_str).collect()
    }
}

impl std::fmt::Debug for ChannelRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 只打通道名：provider 内部可能持有密码（`SmtpConfig` 的 `Debug` 已打码，
        // 但这里不依赖各实现的 `Debug` 纪律）。
        f.debug_struct("ChannelRegistry")
            .field("channels", &self.channels())
            .finish()
    }
}

/// 让注册表能被 worker 直接用来查通道（`ChannelLookup` 的实现）。
impl ChannelLookup for ChannelRegistry {
    fn lookup(&self, channel: &str) -> Option<Arc<dyn ChannelSender>> {
        self.get(channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::{PlatformChannel, SendFuture};
    use anyhow::Result;
    use platform_core::RequestContext;
    use platform_core::extension::notification::{
        DeliveryResult, NotificationMessage, NotificationSender,
    };

    /// 注册表断言用的测试替身（实现**平台**的 `NotificationSender`）。
    struct StubSender {
        /// 通道名。
        name: &'static str,
    }

    impl NotificationSender for StubSender {
        fn channel(&self) -> &str {
            self.name
        }

        async fn send(
            &self,
            _ctx: &RequestContext,
            _msg: &NotificationMessage,
        ) -> Result<DeliveryResult> {
            Ok(DeliveryResult::delivered(self.name))
        }
    }

    /// 另一个替身：**永远失败**（证明 `Err` 能穿过适配器）。
    struct FailingSender;

    impl NotificationSender for FailingSender {
        fn channel(&self) -> &str {
            crate::model::channel::SMS
        }

        async fn send(
            &self,
            _ctx: &RequestContext,
            _msg: &NotificationMessage,
        ) -> Result<DeliveryResult> {
            Err(anyhow::Error::new(crate::error::ServiceError::Notify(
                crate::error::NotifyError::ChannelNotConfigured { channel: "sms" },
            )))
        }
    }

    fn stub(name: &'static str) -> (String, Arc<dyn ChannelSender>) {
        (
            name.to_string(),
            Arc::new(PlatformChannel::new(Arc::new(StubSender { name }))),
        )
    }

    #[test]
    fn an_empty_registry_has_no_channels() {
        let registry = ChannelRegistry::new();
        assert!(registry.channels().is_empty());
        assert!(registry.get("smtp").is_none());
    }

    #[test]
    fn channels_are_listed_in_dictionary_order() {
        let mut registry = ChannelRegistry::new();
        for name in ["webhook", "in_app", "smtp"] {
            let (key, sender) = stub(name);
            registry.insert(&key, sender);
        }
        assert_eq!(registry.channels(), vec!["in_app", "smtp", "webhook"]);
    }

    #[test]
    fn insert_reports_the_replaced_provider() {
        let mut registry = ChannelRegistry::new();
        let (key, first) = stub("smtp");
        assert!(registry.insert(&key, first).is_none());
        let (key2, second) = stub("smtp");
        assert!(
            registry.insert(&key2, second).is_some(),
            "第二次装配应报告被替换者"
        );
        // 覆盖不是新增：通道数仍是 1。
        assert_eq!(registry.channels(), vec!["smtp"]);
    }

    #[tokio::test]
    async fn get_returns_a_usable_provider() {
        let mut registry = ChannelRegistry::new();
        let (key, sender) = stub("smtp");
        registry.insert(&key, sender);
        let provider = registry.get("smtp").expect("应能取到");
        let result = provider
            .send_boxed(&RequestContext::new(), &NotificationMessage::new("t", "b"))
            .await
            .expect("stub 不会失败");
        assert_eq!(result.provider_message_id.as_deref(), Some("smtp"));
    }

    /// ⚠️ `Err` 必须能**穿透**适配器（否则 worker 会把失败当成成功）。
    #[tokio::test]
    async fn errors_pass_through_the_adapter() {
        let mut registry = ChannelRegistry::new();
        registry.insert(
            crate::model::channel::SMS,
            Arc::new(PlatformChannel::new(Arc::new(FailingSender))),
        );
        let provider = registry.get(crate::model::channel::SMS).expect("应能取到");
        let err = provider
            .send_boxed(&RequestContext::new(), &NotificationMessage::new("t", "b"))
            .await
            .expect_err("FailingSender 必须返回 Err");
        // 错误必须是 ServiceError（worker 据此判断「不可重试」）。
        assert!(
            err.downcast_ref::<crate::error::ServiceError>().is_some(),
            "Err 必须是 ServiceError 才能取回错误码"
        );
        assert!(err.to_string().contains("NOTIFY_CHANNEL_NOT_CONFIGURED"));
    }

    /// 注册表能被当 `ChannelLookup` 用（worker 的入口）。
    #[test]
    fn the_registry_implements_channel_lookup() {
        let mut registry = ChannelRegistry::new();
        let (key, sender) = stub("in_app");
        registry.insert(&key, sender);
        let lookup: &dyn ChannelLookup = &registry;
        assert!(lookup.lookup("in_app").is_some());
        assert!(lookup.lookup("nope").is_none());
    }

    /// `SendFuture` 类型别名可实例化（防止它写错而无人发现）。
    #[test]
    fn the_send_future_alias_is_usable() {
        fn _takes(_f: SendFuture<'static>) {}
    }
}
