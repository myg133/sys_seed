//! 异步投递的 **worker**（REQ-015，D296 / D297 / D298）。
//!
//! # 为什么是「数据库驱动的显式 worker」，不是进程内队列
//!
//! | 进程内队列 | 本实现 |
//! |---|---|
//! | web 进程重启 → **丢消息** | 消息在库里，重启后照常取 |
//! | 多实例 → 各自持队列 → **重复或遗漏** | `FOR UPDATE SKIP LOCKED` → 不重复 |
//! | 测试要 `sleep` 等后台 | `tick()` 是**纯函数式的一批处理**，测试直接调 |
//!
//! 入口有两条：
//! - `xtask notify worker`（运维 / 生产）；
//! - `tick()`（测试 / 手工重跑）。
//!
//! # ⚠️ 方案 A：短事务认领 + 事务外投递（design §4.3）
//!
//! `FOR UPDATE` 的锁持有到事务结束，而投递要发网络请求（SMTP 可能 10 秒）→
//! 长时间持锁会拖死其他 worker。所以：
//!
//! ```text
//! ① 短事务：UPDATE … SET status='delivering', attempt_count+1 … RETURNING   ← 认领
//! ② 事务外：逐个投递（网络 I/O）
//! ③ 短事务：逐个写回结果
//! ```
//!
//! **代价**：崩溃时会留下 `delivering` 残留 →
//! 由「`delivering` 超时回收」兜底（`notify.deliveringTimeoutSeconds`，D313）。
//! **这条不是可选项，是方案 A 的必需配套。**

use std::sync::Arc;

use platform_core::RequestContext;
use platform_core::extension::notification::{
    DeliveryResult, NotificationMessage, NotificationSender,
};
use time::OffsetDateTime;

use crate::config::RetryPolicy;
use crate::model::status;
use crate::service::NotifyService;
use crate::store::{ClaimedDelivery, DeliveryOutcome, NotifyStore};

/// 一次 `tick()` 的统计（**可断言**，不是日志文案）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickStats {
    /// 认领到多少条。
    pub claimed: usize,
    /// 投递成功多少条。
    pub delivered: usize,
    /// 失败但还会重试多少条。
    pub retried: usize,
    /// 进了死信多少条。
    pub dead_lettered: usize,
    /// 认领了但找不到对应 provider（通道未注册）的条数。
    pub no_provider: usize,
}

impl TickStats {
    /// 是否一条都没处理（`tick()` 空转的判据）。
    pub fn is_empty(&self) -> bool {
        self.claimed == 0
    }

    /// 处理数之和是否等于认领数（**不遗漏**的自检）。
    ///
    /// ⚠️ 这条自检是判断「有没有条目被静默丢掉」的**唯一**手段：
    /// `claimed = delivered + retried + dead_lettered + no_provider`
    /// 任何一项缺失都说明有一条被吞了（既没成功也没失败 → 永远卡在 `delivering`）。
    pub fn is_balanced(&self) -> bool {
        self.delivered + self.retried + self.dead_lettered + self.no_provider == self.claimed
    }
}

/// 一次 `tick()`：认领一批 → 逐个投递 → 写回结果。
///
/// # 参数
///
/// - `batch`：这批最多取多少条；
/// - `senders`：通道名 → provider（装配层注入）。
pub async fn tick(
    service: &NotifyService,
    ctx: &RequestContext,
    senders: &dyn ChannelLookup,
    batch: i64,
) -> Result<TickStats, crate::error::ServiceError> {
    let policy: RetryPolicy = service.config().retry_policy(ctx).await;
    let store: &NotifyStore = service.store();

    // ① 认领（短事务，`FOR UPDATE SKIP LOCKED`）。
    let claimed = store
        .claim_batch(ctx, batch, policy.delivering_timeout_seconds)
        .await?;

    let mut stats = TickStats {
        claimed: claimed.len(),
        ..TickStats::default()
    };

    // ② + ③ 逐个投递（事务外）并写回。
    for item in &claimed {
        let outcome = deliver_one(senders, ctx, item, &policy).await;
        match &outcome {
            DeliveryOutcome::Delivered { .. } => stats.delivered += 1,
            DeliveryOutcome::Retry { .. } => stats.retried += 1,
            DeliveryOutcome::DeadLetter { .. } => stats.dead_lettered += 1,
        }
        store.record_outcome(ctx, &item.id, &outcome).await?;
    }

    Ok(stats)
}

/// 投递一条并算出它该往哪个状态走。
///
/// # ⚠️ 三种「不可重试」的情形直接进死信
///
/// 1. **通道未注册**（`no_provider`）—— 重试永远失败；
/// 2. **provider 返回 `Err(ChannelNotConfigured)`** —— 同上（D292 的铁律）；
/// 3. **次数用尽** —— `attempt_count >= max_attempts`。
///
/// 其余失败 → `Retry`（带指数退避）。
async fn deliver_one(
    senders: &dyn ChannelLookup,
    ctx: &RequestContext,
    item: &ClaimedDelivery,
    policy: &RetryPolicy,
) -> DeliveryOutcome {
    let Some(sender) = senders.lookup(&item.channel) else {
        // 通道没注册：不重试（重试也不会出现 provider）。
        return DeliveryOutcome::DeadLetter {
            error: format!("no provider registered for channel `{}`", item.channel),
        };
    };

    let message = NotificationMessage {
        title: item.rendered_title.clone(),
        body: item.rendered_body.clone(),
        recipients: vec![item.recipient.clone()],
        payload: item.payload.clone(),
    };

    match sender.send_boxed(ctx, &message).await {
        Ok(result) if result.delivered => DeliveryOutcome::Delivered {
            provider_message_id: result.provider_message_id,
        },
        Ok(result) => {
            // 投递跑完了但失败 → 按次数决定重试还是死信。
            let error = result
                .error
                .unwrap_or_else(|| "the channel reported a failure without a reason".to_string());
            retry_or_dead_letter(item, policy, error)
        }
        Err(err) => {
            // ⚠️ `Err` 里若携带 `NOTIFY_CHANNEL_NOT_CONFIGURED` → **不重试**。
            let text = err.to_string();
            if text.contains(crate::error::code::CHANNEL_NOT_CONFIGURED) {
                return DeliveryOutcome::DeadLetter { error: text };
            }
            retry_or_dead_letter(item, policy, text)
        }
    }
}

/// 按 `attempt_count` 决定重试还是死信。
fn retry_or_dead_letter(
    item: &ClaimedDelivery,
    policy: &RetryPolicy,
    error: String,
) -> DeliveryOutcome {
    // ⚠️ `attempt_count` 是**含首次**的计数（认领时已 +1）。
    // `has_attempts_left(5)` == false → 第 5 次失败 → 死信。
    if !policy.has_attempts_left(item.attempt_count) {
        return DeliveryOutcome::DeadLetter { error };
    }
    let backoff = policy.backoff_seconds(item.attempt_count);
    DeliveryOutcome::Retry {
        error,
        next_attempt_at: OffsetDateTime::now_utc() + time::Duration::seconds(backoff),
    }
}

/// 装箱的 future（让接缝保持**可对象化**）。
pub type SendFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = anyhow::Result<DeliveryResult>> + Send + 'a>,
>;

/// 可对象化的投递端（**为什么需要这一层**）。
///
/// ⚠️ 平台的 `NotificationSender` 是 **RPITIT** 形态
/// （`fn send(...) -> impl Future<...>`，`notification.rs:86-91`）——
/// **不可用于 trait object**，`Arc<dyn NotificationSender>` 编译不过。
/// 而 worker 需要持有「可替换的投递端」（测试里要能断言「发了什么」）。
///
/// → 本 trait 把 `send` 的返回类型**装箱**，从而可 `dyn`。
/// [`PlatformChannel`] 把平台的 `NotificationSender` **转发**过来 ——
/// **只转发，不重写语义**（与 `platform-org/src/notify.rs:43` 的
/// `PlatformNotifier` 同款手法）。
pub trait ChannelSender: Send + Sync {
    /// 投递一条（装箱 future）。
    fn send_boxed<'a>(
        &'a self,
        ctx: &'a RequestContext,
        message: &'a NotificationMessage,
    ) -> SendFuture<'a>;
}

/// 平台 `NotificationSender` 的**适配器**（只转发）。
pub struct PlatformChannel<N: NotificationSender> {
    inner: Arc<N>,
}

impl<N: NotificationSender> PlatformChannel<N> {
    /// 包一个平台的投递端。
    pub fn new(inner: Arc<N>) -> Self {
        Self { inner }
    }
}

impl<N: NotificationSender> ChannelSender for PlatformChannel<N> {
    fn send_boxed<'a>(
        &'a self,
        ctx: &'a RequestContext,
        message: &'a NotificationMessage,
    ) -> SendFuture<'a> {
        Box::pin(async move { self.inner.send(ctx, message).await })
    }
}

/// 通道查找（装配层注入；`ChannelRegistry` 是它的实现）。
pub trait ChannelLookup: Send + Sync {
    /// 按通道名找 provider。
    fn lookup(&self, channel: &str) -> Option<Arc<dyn ChannelSender>>;
}

/// 一行都不认领时也把 `delivering` 残留回收掉（**幂等**）。
///
/// ⚠️ 这是给「流水线停了一段时间」的场景用的：`claim_batch` 已经会回收
/// 超阈值的 `delivering`，本函数只是**显式**提供一个可单独调用的入口
/// （运维手工跑、或 E2E 里断言「残留会被回收」）。
pub async fn reclaim_stale(
    service: &NotifyService,
    ctx: &RequestContext,
    batch: i64,
) -> Result<usize, crate::error::ServiceError> {
    let policy = service.config().retry_policy(ctx).await;
    let claimed = service
        .store()
        .claim_batch(ctx, batch, policy.delivering_timeout_seconds)
        .await?;
    Ok(claimed.len())
}

/// `tick()` 之后仍处于 `delivering` 的条目数（诊断用）。
///
/// 正常应为 0（除了**正在被另一个 worker 处理**的）。
pub async fn count_delivering(
    service: &NotifyService,
    ctx: &RequestContext,
) -> Result<usize, crate::error::ServiceError> {
    let rows = service.store().list_deliveries(ctx).await?;
    Ok(rows
        .iter()
        .filter(|row| row.status == status::DELIVERING)
        .count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::defaults;

    fn claimed(attempt_count: i32) -> ClaimedDelivery {
        ClaimedDelivery {
            id: "d1".to_string(),
            channel: "in_app".to_string(),
            recipient: "u1".to_string(),
            rendered_title: "t".to_string(),
            rendered_body: "b".to_string(),
            payload: Default::default(),
            attempt_count,
        }
    }

    /// `TickStats::is_empty` 只看 `claimed`。
    #[test]
    fn is_empty_is_true_when_nothing_was_claimed() {
        assert!(TickStats::default().is_empty());
        assert!(
            !TickStats {
                claimed: 1,
                delivered: 1,
                ..TickStats::default()
            }
            .is_empty()
        );
    }

    /// `is_balanced` 抓「有理赔被吞」：四项之和必须等于认领数。
    #[test]
    fn is_balanced_catches_swallowed_items() {
        // 认领 3 条，但只记了 2 条结果 → 不平衡（有一条被吞了）。
        let unbalanced = TickStats {
            claimed: 3,
            delivered: 2,
            ..TickStats::default()
        };
        assert!(!unbalanced.is_balanced(), "少记一条必须被抓住");

        let balanced = TickStats {
            claimed: 3,
            delivered: 2,
            dead_lettered: 1,
            ..TickStats::default()
        };
        assert!(balanced.is_balanced());
    }

    /// **退避用 `attempt_count`**：第 1 次失败 → +30s（AC-S-171 步骤 1）。
    #[test]
    fn the_first_failure_backs_off_thirty_seconds() {
        let policy = RetryPolicy::default();
        let outcome = retry_or_dead_letter(&claimed(1), &policy, "boom".to_string());
        match outcome {
            DeliveryOutcome::Retry {
                next_attempt_at, ..
            } => {
                let delta = next_attempt_at - OffsetDateTime::now_utc();
                // 允许 ±2 秒的执行抖动。
                assert!(
                    (28..=32).contains(&delta.whole_seconds()),
                    "第 1 次失败应退避约 30 秒，实际 {} 秒",
                    delta.whole_seconds()
                );
            }
            other => panic!("第 1 次失败应是 Retry，实际 {other:?}"),
        }
    }

    /// 第 4 次失败 → +240s。
    #[test]
    fn the_fourth_failure_backs_off_two_forty_seconds() {
        let policy = RetryPolicy::default();
        match retry_or_dead_letter(&claimed(4), &policy, "boom".to_string()) {
            DeliveryOutcome::Retry {
                next_attempt_at, ..
            } => {
                let delta = (next_attempt_at - OffsetDateTime::now_utc()).whole_seconds();
                assert!(
                    (238..=242).contains(&delta),
                    "第 4 次失败应退避约 240 秒，实际 {delta} 秒"
                );
            }
            other => panic!("第 4 次失败应是 Retry，实际 {other:?}"),
        }
    }

    /// **第 5 次失败 → 死信**（`max_attempts = 5` 含首次，AC-S-171 步骤 1）。
    #[test]
    fn the_fifth_failure_goes_to_dead_letter() {
        let policy = RetryPolicy::default();
        let outcome = retry_or_dead_letter(&claimed(5), &policy, "boom".to_string());
        assert!(
            matches!(outcome, DeliveryOutcome::DeadLetter { .. }),
            "第 5 次失败必须进死信，实际 {outcome:?}"
        );
    }

    /// **配置生效**：`max_attempts = 2` 时第 2 次就死信（AC-S-171 步骤 4）。
    #[test]
    fn a_configured_max_attempts_of_two_dead_letters_on_the_second() {
        let policy = RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        };
        // 第 1 次：还能重试。
        assert!(matches!(
            retry_or_dead_letter(&claimed(1), &policy, "boom".to_string()),
            DeliveryOutcome::Retry { .. }
        ));
        // 第 2 次：死信（证明读的是配置，不是写死的 5）。
        assert!(matches!(
            retry_or_dead_letter(&claimed(2), &policy, "boom".to_string()),
            DeliveryOutcome::DeadLetter { .. }
        ));
    }

    /// 死信时**保留**失败原因（可追溯）。
    #[test]
    fn the_dead_letter_keeps_the_error() {
        let policy = RetryPolicy::default();
        match retry_or_dead_letter(&claimed(5), &policy, "550 rejected".to_string()) {
            DeliveryOutcome::DeadLetter { error } => {
                assert!(error.contains("550"), "必须保留原因：{error}");
            }
            other => panic!("应是死信，实际 {other:?}"),
        }
    }

    /// 默认参数确实是 5 / 30 / 2 / 3600 / 300（与 manifest 一致）。
    #[test]
    fn the_defaults_match_the_manifest() {
        assert_eq!(defaults::MAX_ATTEMPTS, 5);
        assert_eq!(defaults::RETRY_BASE_SECONDS, 30);
        assert_eq!(defaults::RETRY_MULTIPLIER, 2);
        assert_eq!(defaults::RETRY_MAX_SECONDS, 3600);
        assert_eq!(defaults::DELIVERING_TIMEOUT_SECONDS, 300);
    }
}
