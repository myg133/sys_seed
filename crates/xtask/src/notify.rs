//! 通知投递的**批处理入口**（REQ-015，D296）。
//!
//! # 为什么在 `xtask` 里
//!
//! worker 是**运维 / 定时任务**，不是 HTTP 请求路径：
//! - 跑在 web 进程内 → 重启丢消息、多实例重复投递（D296 的两条理由）；
//! - 放 `xtask` → **显式执行**、可与部署流水线绑定、测试里可直接调 [`run_rounds`]。
//!
//! # ⚠️ 出队靠数据库，不靠内存
//!
//! `SELECT … FOR UPDATE SKIP LOCKED`（`platform_notify::store::NotifyStore::claim_batch`）→
//! 多实例天然不重复。本模块只负责「装配 + 循环」，不重复实现出队语义。
//!
//! # ⚠️ 通道装配是**最小集**
//!
//! 本命令装配 3 个真实现（`in_app` / `smtp` / `webhook`）+ 4 个显式未配置。
//! SMTP 的 5 个环境变量没配齐时 → 该通道仍注册但投递返回
//! `NOTIFY_CHANNEL_NOT_CONFIGURED`（**不是静默成功**，D292）。

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use platform_core::RequestContext;
use platform_db::{DbOptions, TenantDb};
use platform_notify::channels::ChannelRegistry;
use platform_notify::channels::in_app::InAppSender;
use platform_notify::channels::smtp::SmtpSender;
use platform_notify::channels::unconfigured::UnconfiguredSender;
use platform_notify::channels::webhook::WebhookSender;
use platform_notify::config::{NotifyConfig, NotifyConfigSource, SmtpConfig};
use platform_notify::model::channel;
use platform_notify::service::NotifyService;
use platform_notify::store::NotifyStore;
use platform_notify::worker::PlatformChannel;
use platform_notify::worker::{TickStats, tick};

/// 一轮的结果（打印用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundOutcome {
    /// 认领数。
    pub claimed: usize,
    /// 成功数。
    pub delivered: usize,
    /// 待重试数。
    pub retried: usize,
    /// 进死信数。
    pub dead_lettered: usize,
    /// 没有 provider 的条数。
    pub no_provider: usize,
}

impl From<TickStats> for RoundOutcome {
    fn from(stats: TickStats) -> Self {
        Self {
            claimed: stats.claimed,
            delivered: stats.delivered,
            retried: stats.retried,
            dead_lettered: stats.dead_lettered,
            no_provider: stats.no_provider,
        }
    }
}

impl RoundOutcome {
    /// 这一轮一条都没取到（循环终止条件）。
    pub fn is_empty(&self) -> bool {
        self.claimed == 0
    }

    /// 计数自洽（`claimed == delivered + retried + dead_lettered + no_provider`）。
    ///
    /// ⚠️ 不自洽说明有一条被**静默吞掉**了（既没成功也没失败 → 永远卡在 `delivering`）。
    pub fn is_balanced(&self) -> bool {
        self.delivered + self.retried + self.dead_lettered + self.no_provider == self.claimed
    }

    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "认领 {}｜成功 {}｜待重试 {}｜死信 {}｜无通道 {}",
            self.claimed, self.delivered, self.retried, self.dead_lettered, self.no_provider
        )
    }
}

/// 跑 `rounds` 轮（`rounds = 0` → 跑到没有待投递为止）。
///
/// 返回每一轮的结果（**测试直接断言它**，不用 sleep）。
pub async fn run_rounds(
    service: &NotifyService,
    ctx: &RequestContext,
    senders: &ChannelRegistry,
    batch: i64,
    rounds: u32,
) -> Result<Vec<RoundOutcome>> {
    let mut outcomes = Vec::new();
    let mut done = 0u32;
    loop {
        let stats = tick(service, ctx, senders, batch)
            .await
            .map_err(|err| anyhow::anyhow!("投递批处理失败：{err}"))?;
        let outcome = RoundOutcome::from(stats);
        let empty = outcome.is_empty();
        if !outcome.is_balanced() {
            bail!(
                "计数不自洽：{}（有一条被静默吞掉 —— 它会永远卡在 delivering）",
                outcome.summary()
            );
        }
        outcomes.push(outcome);
        done += 1;
        if rounds != 0 && done >= rounds {
            break;
        }
        if empty {
            break;
        }
    }
    Ok(outcomes)
}

/// 装配 worker（连接库 + 装通道 + 建服务）。
///
/// # ⚠️ `platform_config` 的读取这里用**默认值**
///
/// 本命令不装配 `platform-config` 的 `ConfigSource`（那需要 epoch 缓存等一整套）——
/// 用 `RetryPolicy::default()`（与 manifest 默认值逐字一致，`contract_c5` 钉死了）。
/// 生产环境要读库里的配置时，由装配层注入真正的 `NotifyConfigSource`。
pub async fn build_service(
    database_url: &str,
    tenant: &str,
) -> Result<(NotifyService, ChannelRegistry)> {
    if tenant.trim().is_empty() {
        bail!("缺少租户 ID：投递记录是租户级的（D299），必须用 --tenant 指定");
    }

    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .context("连接数据库失败")?;
    let store = NotifyStore::new(db);

    let mut registry = ChannelRegistry::new();
    // 3 个真实现。
    registry.insert(
        channel::IN_APP,
        Arc::new(PlatformChannel::new(Arc::new(InAppSender::new()))),
    );
    // ⚠️ `WebhookSender::new()` 返回 `Result`（TLS connector 构造可能失败）——
    // 构造失败**不得 panic**：降级为「未配置」（投递返回明确错误）。
    match WebhookSender::new() {
        Ok(sender) => {
            registry.insert(
                channel::WEBHOOK,
                Arc::new(PlatformChannel::new(Arc::new(sender))),
            );
        }
        Err(err) => {
            eprintln!("提示：Webhook 的出站 client 构造失败（{err}）→ webhook 通道将返回明确错误");
            registry.insert(
                channel::WEBHOOK,
                Arc::new(PlatformChannel::new(Arc::new(UnconfiguredSender::new(
                    channel::WEBHOOK,
                )))),
            );
        }
    }
    match SmtpConfig::from_env() {
        Some(config) => {
            registry.insert(
                channel::SMTP,
                Arc::new(PlatformChannel::new(Arc::new(SmtpSender::new(config)))),
            );
        }
        None => {
            // ⚠️ SMTP 未配置 → 仍注册，但投递一律 `NOTIFY_CHANNEL_NOT_CONFIGURED`。
            eprintln!(
                "提示：SMTP 未配置（缺 BASE_SMTP_HOST/PORT/USERNAME/PASSWORD/FROM 之一）\
                 → smtp 通道将返回 NOTIFY_CHANNEL_NOT_CONFIGURED"
            );
            registry.insert(
                channel::SMTP,
                Arc::new(PlatformChannel::new(Arc::new(UnconfiguredSender::new(
                    channel::SMTP,
                )))),
            );
        }
    }
    // 4 个显式未配置。
    for name in channel::UNCONFIGURED {
        registry.insert(
            name,
            Arc::new(PlatformChannel::new(Arc::new(UnconfiguredSender::new(
                name,
            )))),
        );
    }

    let config = NotifyConfig::new(Arc::new(DefaultConfigSource));
    let service = NotifyService::new(store, config);
    Ok((service, registry))
}

/// 只有默认值的配置源（见 [`build_service`] 的说明）。
struct DefaultConfigSource;

impl NotifyConfigSource for DefaultConfigSource {
    fn read_platform<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _key: &'a str,
    ) -> platform_notify::config::NotifyBoxFuture<'a, Option<i64>> {
        Box::pin(async { None })
    }

    fn tenant_default_locale(
        &self,
        _ctx: &RequestContext,
    ) -> platform_notify::config::NotifyBoxFuture<'_, String> {
        // 与 `platform-config` 的 `FALLBACK_LOCALES` 一致（`registry.rs:44`）。
        Box::pin(async { "zh-CN".to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空轮：`claimed == 0` 且计数自洽。
    #[test]
    fn an_empty_round_is_balanced() {
        let outcome = RoundOutcome {
            claimed: 0,
            delivered: 0,
            retried: 0,
            dead_lettered: 0,
            no_provider: 0,
        };
        assert!(outcome.is_empty());
        assert!(outcome.is_balanced());
    }

    /// 被吞掉一条 → 不自洽（这是 worker 唯一的自检）。
    #[test]
    fn a_swallowed_item_breaks_the_balance() {
        let outcome = RoundOutcome {
            claimed: 3,
            delivered: 2,
            retried: 0,
            dead_lettered: 0,
            no_provider: 0,
        };
        assert!(!outcome.is_balanced(), "少记一条必须被抓住");
    }

    /// 四种结果齐了 → 自洽。
    #[test]
    fn all_four_outcomes_sum_to_claimed() {
        let outcome = RoundOutcome {
            claimed: 4,
            delivered: 1,
            retried: 1,
            dead_lettered: 1,
            no_provider: 1,
        };
        assert!(outcome.is_balanced());
    }

    /// 摘要里四个数字都出现（运维要看得到）。
    #[test]
    fn the_summary_mentions_every_counter() {
        let outcome = RoundOutcome {
            claimed: 5,
            delivered: 2,
            retried: 1,
            dead_lettered: 1,
            no_provider: 1,
        };
        let text = outcome.summary();
        assert!(text.contains('5'));
        assert!(text.contains('2'));
        assert!(text.contains("待重试"));
        assert!(text.contains("死信"));
        assert!(text.contains("无通道"));
    }

    /// 空租户 ID 必须拒绝（投递记录是租户级的）。
    #[tokio::test]
    async fn an_empty_tenant_is_rejected() {
        let Err(err) = build_service("postgres://x/y", "   ").await else {
            panic!("空租户必须拒绝");
        };
        assert!(err.to_string().contains("租户"), "错误要说明原因：{err}");
    }
}
