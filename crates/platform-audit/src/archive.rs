//! 审计归档（REQ-014 AC-S-131）。
//!
//! # 归档**不由应用角色直接做**
//!
//! 「DELETE + INSERT」需要给应用角色 `DELETE` 权限 → **append-only 立刻破功**
//! （AC-S-123 / AC-S-131 直接失败）。
//! 所以归档走一个**受限的特权函数**（`SECURITY DEFINER`），它把「把超期行搬到
//! 归档表」这一件事做完 —— 这是同时满足两者的唯一做法（D133）。
//!
//! # 归档**不破坏链**（D159）
//!
//! 归档把在线表的一段搬到归档表，**在线表的第一条**的 `prev_hash` 指向的是
//! **已被搬走**的那条 → 接续校验**必然误报断裂** → 「归档一次就链坏了」。
//!
//! → 链基准存在 `platform_audit_chain_state`（`head_hash`）：
//! 归档时更新成**被搬走那段的尾 `hash`**，校验时在线表第一条与它比对。
//!
//! # 归档是**单租户**的（D158）
//!
//! `platform_audit_archive_move` 里有**显式的租户守卫**：
//! 调用方上下文里的租户与参数里的租户不一致 → 报错。
//! 不提供「全平台归档」。

use platform_core::RequestContext;
use platform_core::extension::audit::AuditError;
use platform_db::{TenantDb, TenantQuery};
use time::OffsetDateTime;

/// 建议的归档节奏（相邻两次归档之间的间隔）。
///
/// D137 要求归档响应里**看得见**「上次归档时间 / 下次建议时间」——
/// 本仓**没有**调度器（不引入常驻进程），归档由外部调度器周期触发。
pub const RECOMMENDED_INTERVAL_DAYS: i64 = 1;

/// 在线保留天数（**与登记的 `audit.retention.online_days` 默认值一致**，D178）。
///
/// ⚠️ 本轮**只登记**配置项（D178）—— 归档阈值**尚未**从配置里读。
/// 这里用**与配置默认值一致的常量**，避免「配置改了但行为没变」的错觉。
pub const DEFAULT_ONLINE_DAYS: i64 = 90;

/// `platform_audit_archive_move` 的返回值（搬走了多少条）。
struct MovedCount {
    moved: i64,
}

platform_db::tenant_row!(MovedCount {
    moved: "platform_audit_archive_move",
});

/// 链状态表里的归档时刻。
struct ArchivedAt {
    archived_at: Option<OffsetDateTime>,
}

platform_db::tenant_row!(ArchivedAt {
    archived_at: "archived_at",
});

/// 归档结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveResult {
    /// 搬走了多少条。
    pub moved: i64,
    /// 本次归档时刻（没搬走任何行时为 `None`）。
    pub archived_at: Option<OffsetDateTime>,
    /// 建议的下次归档时刻（让运维看得见，D137）。
    pub next_recommended_at: Option<OffsetDateTime>,
}

/// 归档服务。
#[derive(Debug, Clone)]
pub struct ArchiveService {
    db: TenantDb,
}

impl ArchiveService {
    /// 用已有的租户作用域数据入口构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 把 `before` 之前的记录搬进归档表。
    ///
    /// **幂等**：没有可搬的行 → 影响 0 行，**不报错**（AC-S-131 步骤 3）。
    pub async fn archive_before(
        &self,
        ctx: &RequestContext,
        before: OffsetDateTime,
    ) -> Result<ArchiveResult, AuditError> {
        let mut tx = self.db.begin(ctx).await.map_err(unavailable)?;

        let moved: MovedCount = tx
            .fetch_one(
                &TenantQuery::new("SELECT platform_audit_archive_move({tenant}, ?)").bind(before),
            )
            .await
            .map_err(unavailable)?;

        // 归档时刻（链状态表里记着）。
        let archived_at: Option<ArchivedAt> = tx
            .fetch_optional(&TenantQuery::new(
                "SELECT archived_at FROM platform_audit_chain_state WHERE tenant_id = {tenant}",
            ))
            .await
            .map_err(unavailable)?;

        tx.commit().await.map_err(unavailable)?;

        let archived_at = archived_at.and_then(|row| row.archived_at);
        let next_recommended_at =
            archived_at.map(|moment| moment + time::Duration::days(RECOMMENDED_INTERVAL_DAYS));

        Ok(ArchiveResult {
            moved: moved.moved,
            archived_at,
            next_recommended_at,
        })
    }

    /// 按「在线保留天数」归档（`before = now - days 天`）。
    ///
    /// 归档接口与 `cargo xtask audit archive` **共用**这一条路径 →
    /// 两个入口的语义**必然一致**。
    pub async fn archive_older_than(
        &self,
        ctx: &RequestContext,
        days: i64,
    ) -> Result<ArchiveResult, AuditError> {
        let cutoff = OffsetDateTime::now_utc() - time::Duration::days(days);
        self.archive_before(ctx, cutoff).await
    }
}

fn unavailable(error: impl std::fmt::Display) -> AuditError {
    AuditError::SinkUnavailable {
        reason: format!("the audit archive could not be executed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recommended_interval_is_one_day() {
        assert_eq!(RECOMMENDED_INTERVAL_DAYS, 1);
    }
}
