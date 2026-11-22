//! 导入导出的**批处理入口**（REQ-018，D324）。
//!
//! # 为什么在 `xtask` 里
//!
//! 与 REQ-015 的通知投递同构（见 [`crate::notify`]）：
//! - 跑在 web 进程内 → 重启丢任务、多实例重复处理；
//! - 放 `xtask` → **显式执行**、可与部署流水线绑定、测试里可直接调 [`run_rounds`]。
//!
//! # ⚠️ 认领靠数据库，不靠内存
//!
//! `SELECT … FOR UPDATE SKIP LOCKED`（`platform_storage::store::StorageStore::claim_jobs`）→
//! 多实例天然不重复。本模块只负责「装配 + 循环」，不重复实现认领语义。
//!
//! # ⚠️ 对象存储是**本地 FS 真实现**
//!
//! 本命令装配 `LocalFsStore`（D319）。S3 的 `S3Store` 只有 SigV4 签名按官方向量验证，
//! **没有真实端点证明可用**（TD-059）→ 这里不装配它。

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use platform_core::RequestContext;
use platform_db::{DbOptions, TenantDb};
use platform_storage::config::{LocalStorageEnv, StorageConfig, StorageConfigSource};
use platform_storage::object_store::local_fs::LocalFsStore;
use platform_storage::scanner::NoopScanner;
use platform_storage::service::StorageService;
use platform_storage::store::StorageStore;
use platform_storage::worker::{TickStats, tick};

/// 一轮的结果（打印用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundOutcome {
    /// 认领数。
    pub claimed: usize,
    /// 完成数。
    pub completed: usize,
    /// 失败数。
    pub failed: usize,
    /// 只跑了一部分（进度已更新，需下次继续）的数量。
    pub partial: usize,
}

impl From<TickStats> for RoundOutcome {
    fn from(stats: TickStats) -> Self {
        Self {
            claimed: stats.claimed,
            completed: stats.completed,
            failed: stats.failed,
            partial: stats.partial,
        }
    }
}

impl RoundOutcome {
    /// 这一轮一条都没取到（循环终止条件）。
    pub fn is_empty(&self) -> bool {
        self.claimed == 0
    }

    /// 计数自洽（`claimed == completed + failed + partial`）。
    ///
    /// ⚠️ 不自洽说明有一个任务被**静默吞掉**了（既没完成、没失败、也没记进度 →
    /// 会永远卡在认领后的中间态）。与 `TickStats::is_balanced` 同一口径。
    pub fn is_balanced(&self) -> bool {
        self.completed + self.failed + self.partial == self.claimed
    }

    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "认领 {}｜完成 {}｜失败 {}｜部分 {}",
            self.claimed, self.completed, self.failed, self.partial
        )
    }
}

/// 跑 `rounds` 轮（`rounds = 0` → 跑到没有待处理任务为止）。
///
/// 返回每一轮的结果（**测试直接断言它**，不用 sleep）。
pub async fn run_rounds(
    service: &StorageService,
    ctx: &RequestContext,
    batch: i64,
    rounds: u32,
) -> Result<Vec<RoundOutcome>> {
    let mut outcomes = Vec::new();
    let mut done = 0u32;
    loop {
        let stats = tick(service, ctx, batch)
            .await
            .map_err(|err| anyhow::anyhow!("导入导出批处理失败：{err}"))?;
        let outcome = RoundOutcome::from(stats);
        let empty = outcome.is_empty();
        if !outcome.is_balanced() {
            bail!(
                "计数不自洽：{}（有一个任务被静默吞掉 —— 它会永远卡在处理中）",
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

/// 装配 worker（连接库 + 装对象存储 + 建服务）。
///
/// 第二个元素是**占位**（`()`）—— 本轮 worker 不需要额外的注册表
/// （通知的 `ChannelRegistry` 在这里没有对应物）。
///
/// # ⚠️ `platform_config` 的读取这里用**默认值**
///
/// 本命令不装配 `platform-config` 的 `ConfigSource`（那需要 epoch 缓存等一整套）——
/// 用 [`StorageSettings::default()`](platform_storage::config::StorageSettings) 对应的缺省值。
/// 生产环境要读库里的配置时，由装配层注入真正的 [`StorageConfigSource`]。
///
/// # ⚠️ 存储后端的三个环境变量**必须齐**
///
/// `BASE_STORAGE_ROOT` / `BASE_STORAGE_PRESIGN_SECRET` / `BASE_STORAGE_PRESIGN_KEY_ID`，
/// 缺一个 → 直接失败（**不是**静默用默认根目录）。
pub async fn build_service(database_url: &str, tenant: &str) -> Result<(StorageService, ())> {
    if tenant.trim().is_empty() {
        bail!("缺少租户 ID：导入导出任务是租户级的（D324），必须用 --tenant 指定");
    }

    // ⚠️ 先读环境变量：缺了就**不要**白连一次数据库。
    let Some(env) = LocalStorageEnv::from_env() else {
        bail!("缺少 BASE_STORAGE_ROOT / BASE_STORAGE_PRESIGN_SECRET / BASE_STORAGE_PRESIGN_KEY_ID");
    };

    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .context("连接数据库失败")?;
    let store = StorageStore::new(db);

    let objects = Arc::new(LocalFsStore::new(env.root));
    let scanner = Arc::new(NoopScanner);
    let config = StorageConfig::new(Arc::new(DefaultConfigSource));

    let service = StorageService::new(
        store,
        config,
        objects,
        scanner,
        env.presign_secret.into_bytes(),
        env.presign_key_id,
    );
    Ok((service, ()))
}

/// 只有默认值的配置源（见 [`build_service`] 的说明）。
struct DefaultConfigSource;

impl StorageConfigSource for DefaultConfigSource {
    fn read_platform<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _key: &'a str,
    ) -> platform_storage::config::StorageBoxFuture<'a, Option<i64>> {
        Box::pin(async { None })
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
            completed: 0,
            failed: 0,
            partial: 0,
        };
        assert!(outcome.is_empty());
        assert!(outcome.is_balanced());
    }

    /// 被吞掉一个 → 不自洽（这是 worker 唯一的自检）。
    #[test]
    fn a_swallowed_job_breaks_the_balance() {
        let outcome = RoundOutcome {
            claimed: 3,
            completed: 2,
            failed: 0,
            partial: 0,
        };
        assert!(!outcome.is_balanced(), "少记一个必须被抓住");
    }

    /// 完成 + 失败 + 部分 == 认领 → 自洽。
    #[test]
    fn completed_failed_and_partial_sum_to_claimed() {
        let outcome = RoundOutcome {
            claimed: 4,
            completed: 2,
            failed: 1,
            partial: 1,
        };
        assert!(outcome.is_balanced());
    }

    /// 全部失败也算自洽（失败是**记账**，不是漏洞）。
    #[test]
    fn all_failed_is_still_balanced() {
        let outcome = RoundOutcome {
            claimed: 2,
            completed: 0,
            failed: 2,
            partial: 0,
        };
        assert!(outcome.is_balanced());
    }

    /// 只有 `partial` 也算自洽（大文件导入分多次 tick 是**正常**路径）。
    #[test]
    fn a_fully_partial_round_is_balanced() {
        let outcome = RoundOutcome {
            claimed: 3,
            completed: 0,
            failed: 0,
            partial: 3,
        };
        assert!(outcome.is_balanced());
        assert!(!outcome.is_empty(), "记了进度 → 不是空轮，循环应继续");
    }

    /// 摘要里四个数字都出现（运维要看得到）。
    #[test]
    fn the_summary_mentions_every_counter() {
        let outcome = RoundOutcome {
            claimed: 5,
            completed: 3,
            failed: 1,
            partial: 1,
        };
        let text = outcome.summary();
        assert!(text.contains('5'));
        assert!(text.contains('3'));
        assert!(text.contains('1'));
        assert!(text.contains("完成"));
        assert!(text.contains("失败"));
        assert!(text.contains("部分"));
    }

    /// 空租户 ID 必须拒绝（导入导出任务是租户级的）。
    #[tokio::test]
    async fn an_empty_tenant_is_rejected() {
        let Err(err) = build_service("postgres://x/y", "   ").await else {
            panic!("空租户必须拒绝");
        };
        assert!(err.to_string().contains("租户"), "错误要说明原因：{err}");
    }

    /// `TickStats` → `RoundOutcome` 逐字搬过来（不丢字段）。
    #[test]
    fn tick_stats_map_one_to_one() {
        let stats = TickStats {
            claimed: 7,
            completed: 4,
            failed: 1,
            partial: 2,
        };
        let outcome = RoundOutcome::from(stats);
        assert_eq!(outcome.claimed, 7);
        assert_eq!(outcome.completed, 4);
        assert_eq!(outcome.failed, 1);
        assert_eq!(outcome.partial, 2);
        assert!(outcome.is_balanced());
    }
}
