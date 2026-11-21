//! 显式「未配置」通道（REQ-015，D292）。
//!
//! # 为什么**不是**「空实现」
//!
//! `sms` / `dingtalk` / `wecom` / `feishu` 四类通道本期**不接**（没有凭据、也没有
//! 供应商适配），但**不能**从注册表里消失 —— 管理员配置模板时要能看到它们，
//! 投递时要能拿到一个**明确**的失败原因。
//!
//! # ⚠️ 铁律：`send()` **必须返回 `Err`**
//!
//! 返回 `Ok(DeliveryResult { delivered: false, .. })` 的语义是「投递跑完了但失败」，
//! 编排层的 worker 会把它当成**可重试**的失败 → 对一个**永远不可能成功**的通道
//! 白重试 5 次才进死信。返回 `Err(NOTIFY_CHANNEL_NOT_CONFIGURED)` 才是
//! 「这条通道不存在」的正确语义：worker 直接判死信，不重试。

use std::future::Future;

use anyhow::Result;
use platform_core::RequestContext;
use platform_core::extension::notification::{DeliveryResult, NotificationMessage};

use crate::error::{NotifyError, ServiceError};

/// 一个「未配置」通道的投递端。
///
/// 只持有一个通道名（`&'static str`，取自 [`crate::model::channel::UNCONFIGURED`]），
/// 因此 `send()` 里没有任何 I/O —— 它**注定**失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnconfiguredSender {
    /// 通道名（`sms` / `dingtalk` / `wecom` / `feishu`）。
    channel: &'static str,
}

impl UnconfiguredSender {
    /// 新建一个未配置通道的投递端。
    pub const fn new(channel: &'static str) -> Self {
        Self { channel }
    }
}

impl platform_core::extension::notification::NotificationSender for UnconfiguredSender {
    fn channel(&self) -> &str {
        self.channel
    }

    /// **永远**返回 `Err(NOTIFY_CHANNEL_NOT_CONFIGURED)`。
    ///
    /// ⚠️ 这里**不得**改成返回 `Ok(DeliveryResult::failed(..))` ——
    /// 那会让 worker 走重试分支（见模块文档）。
    /// 错误经 [`crate::error::ServiceError`] 转成 `anyhow::Error`，调用方仍可
    /// `downcast_ref::<ServiceError>()` 拿回 `NOTIFY_CHANNEL_NOT_CONFIGURED`。
    fn send(
        &self,
        _ctx: &RequestContext,
        _msg: &NotificationMessage,
    ) -> impl Future<Output = Result<DeliveryResult>> + Send {
        let channel = self.channel;
        async move {
            // ⚠️ 包装成 `ServiceError` 再进 `anyhow` —— 否则调用方
            // `downcast_ref::<ServiceError>()` 拿不到码，只能看到一句字符串。
            let err: ServiceError = NotifyError::ChannelNotConfigured { channel }.into();
            Err(err.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::code;
    use crate::error::{NotifyError, ServiceError};
    use crate::model::channel;
    use platform_core::extension::notification::{NotificationMessage, NotificationSender};

    use super::UnconfiguredSender;

    /// 从 `anyhow::Error` 里取出平台错误码；取不到就当场失败。
    fn code_of(err: &anyhow::Error) -> &'static str {
        let service = err
            .downcast_ref::<ServiceError>()
            .unwrap_or_else(|| panic!("错误必须能 downcast 回 ServiceError：{err}"));
        service.code()
    }

    /// 从 `anyhow::Error` 里取出「未配置」的通道名。
    fn unconfigured_channel_of(err: &anyhow::Error) -> &'static str {
        let service = err
            .downcast_ref::<ServiceError>()
            .unwrap_or_else(|| panic!("错误必须能 downcast 回 ServiceError：{err}"));
        match service {
            ServiceError::Notify(NotifyError::ChannelNotConfigured { channel }) => channel,
            other => panic!("错误必须是 ChannelNotConfigured，实际是：{other}"),
        }
    }

    /// 4 个未配置通道**逐个** `send()` → **全部 `Err`**，且码是
    /// `NOTIFY_CHANNEL_NOT_CONFIGURED`、通道名与构造时一致。
    #[tokio::test]
    async fn every_unconfigured_channel_fails_with_channel_not_configured() {
        let ctx = platform_core::RequestContext::new();
        let msg = NotificationMessage::new("标题", "正文").with_recipient("user-1");
        for channel in channel::UNCONFIGURED {
            let sender = UnconfiguredSender::new(channel);
            let err = sender
                .send(&ctx, &msg)
                .await
                .expect_err("未配置通道的 send() 必须返回 Err，绝不能是 Ok");
            assert_eq!(
                code_of(&err),
                code::CHANNEL_NOT_CONFIGURED,
                "通道 {channel} 的错误码必须是 NOTIFY_CHANNEL_NOT_CONFIGURED"
            );
            assert_eq!(
                unconfigured_channel_of(&err),
                *channel,
                "错误里点名的通道必须与构造时一致"
            );
        }
    }

    /// **没有任何一个**未配置通道返回 `Ok(_)` —— 显式断言 `is_err()`。
    ///
    /// ⚠️ 这条和上一条**不是重复**：上一条核对「错得对」，这条钉死
    /// 「`Ok(DeliveryResult { delivered: false })` 的假绿」不许出现。
    #[tokio::test]
    async fn no_unconfigured_channel_ever_returns_ok() {
        let ctx = platform_core::RequestContext::new();
        let msg = NotificationMessage::new("标题", "正文").with_recipient("user-1");
        for channel in channel::UNCONFIGURED {
            let sender = UnconfiguredSender::new(channel);
            let result = sender.send(&ctx, &msg).await;
            assert!(
                result.is_err(),
                "通道 {channel} 返回了 Ok，这会让 worker 白重试 5 次：{result:?}"
            );
            // 连「delivered: false 的 Ok」也不允许 —— `is_err()` 已经覆盖，
            // 这里再显式取一次值，保证上面断言没有被优化成只看类型。
            assert!(result.ok().is_none(), "通道 {channel} 不得返回 Ok");
        }
    }

    /// `channel()` 返回构造时给的通道名（注册表按它对上号）。
    #[test]
    fn the_channel_name_is_returned_verbatim() {
        for channel in channel::UNCONFIGURED {
            assert_eq!(UnconfiguredSender::new(channel).channel(), *channel);
        }
        // 单一通道也核对一次，避免上面循环恰好空转。
        assert_eq!(UnconfiguredSender::new(channel::SMS).channel(), "sms");
    }
}
