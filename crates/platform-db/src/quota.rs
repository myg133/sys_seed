//! 租户配额：用量计数**写 DB**，限额可查。
//!
//! # 为什么计数写 DB 而不是内存计数
//!
//! 内存计数**重启就丢**：服务重启、扩容、滚动发布都会把用量清零 ——
//! 配额就变成了「形同虚设的累计值」。所以用量落在 `platform_tenant_quota` 表里。
//!
//! # 配额 ≠ 限流
//!
//! | 概念 | 防什么 | 数据源 |
//! |------|--------|--------|
//! | **配额** | 长期超量（存储空间、API 调用次数） | 累计计数 → **DB**（本模块） |
//! | **限流** | 瞬时打爆 | 滑动窗口 → Redis（中间件，REQ-019） |
//!
//! 两者不要混用。

use platform_core::RequestContext;
use sqlx::postgres::PgRow;
use sqlx::{FromRow, Row};

use crate::error::DbResult;
use crate::query::TenantQuery;
use crate::tenant::TenantDb;

/// 配额计数表名。
pub const QUOTA_TABLE: &str = "platform_tenant_quota";

/// 某个租户、某项指标的配额用量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaUsage {
    /// 归属租户。
    pub tenant_id: String,
    /// 指标名（如 `api_calls`、`storage_bytes`）。
    pub metric: String,
    /// 已用量。
    pub used: i64,
    /// 限额；`None` = 未设限额。
    pub limit: Option<i64>,
}

impl QuotaUsage {
    /// 是否已超过限额。
    pub fn is_exceeded(&self) -> bool {
        self.limit.is_some_and(|limit| self.used > limit)
    }

    /// 剩余额度；未设限额时为 `None`。
    pub fn remaining(&self) -> Option<i64> {
        self.limit.map(|limit| limit - self.used)
    }
}

/// 配额表的一行。
struct QuotaRow {
    tenant_id: String,
    metric: String,
    used: i64,
    quota_limit: Option<i64>,
}

impl<'r> FromRow<'r, PgRow> for QuotaRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            tenant_id: row.try_get("tenant_id")?,
            metric: row.try_get("metric")?,
            used: row.try_get("used")?,
            quota_limit: row.try_get("quota_limit")?,
        })
    }
}

/// 配额行（只取用量的返回值）。
struct UsedRow {
    used: i64,
}

impl<'r> FromRow<'r, PgRow> for UsedRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            used: row.try_get("used")?,
        })
    }
}

impl TenantDb {
    /// 累加某项指标的用量，返回累加后的用量。
    ///
    /// 计数**落库**（`INSERT ... ON CONFLICT DO UPDATE`），不是内存计数 ——
    /// 重启、扩容都不会丢。
    ///
    /// 租户条件由 `{tenant}` 占位符保证；RLS 的 `WITH CHECK` 再兜一层
    /// （写不进别的租户的账）。
    pub async fn add_quota_usage(
        &self,
        ctx: &RequestContext,
        metric: &str,
        delta: i64,
    ) -> DbResult<i64> {
        let statement = format!(
            "INSERT INTO {table} (tenant_id, metric, used) \
             VALUES ({{tenant}}, ?, ?) \
             ON CONFLICT (tenant_id, metric) DO UPDATE \
             SET used = {table}.used + EXCLUDED.used, updated_at = now() \
             RETURNING used",
            table = self.table(QUOTA_TABLE)
        );
        let query = TenantQuery::new(statement).bind(metric).bind(delta);
        let row: UsedRow = self.fetch_one(ctx, &query).await?;
        Ok(row.used)
    }

    /// 查某项指标的用量与限额。
    ///
    /// 从未记过账的指标返回 `used = 0`、`limit = None`。
    pub async fn quota_usage(&self, ctx: &RequestContext, metric: &str) -> DbResult<QuotaUsage> {
        let statement = format!(
            "SELECT tenant_id, metric, used, quota_limit FROM {table} \
             WHERE tenant_id = {{tenant}} AND metric = ?",
            table = self.table(QUOTA_TABLE)
        );
        let query = TenantQuery::new(statement).bind(metric);
        let row: Option<QuotaRow> = self.fetch_optional(ctx, &query).await?;
        Ok(match row {
            Some(row) => QuotaUsage {
                tenant_id: row.tenant_id,
                metric: row.metric,
                used: row.used,
                limit: row.quota_limit,
            },
            None => QuotaUsage {
                tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
                metric: metric.to_string(),
                used: 0,
                limit: None,
            },
        })
    }

    /// 设置某项指标的限额（`None` = 取消限额）。
    pub async fn set_quota_limit(
        &self,
        ctx: &RequestContext,
        metric: &str,
        limit: Option<i64>,
    ) -> DbResult<()> {
        let statement = format!(
            "INSERT INTO {table} (tenant_id, metric, used, quota_limit) \
             VALUES ({{tenant}}, ?, 0, ?) \
             ON CONFLICT (tenant_id, metric) DO UPDATE \
             SET quota_limit = EXCLUDED.quota_limit, updated_at = now()",
            table = self.table(QUOTA_TABLE)
        );
        let query = TenantQuery::new(statement).bind(metric).bind(limit);
        self.execute(ctx, &query).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_usage_reports_exceeded_and_remaining() {
        let usage = QuotaUsage {
            tenant_id: "tenant-a".to_string(),
            metric: "api_calls".to_string(),
            used: 120,
            limit: Some(100),
        };
        assert!(usage.is_exceeded());
        assert_eq!(usage.remaining(), Some(-20));

        let unlimited = QuotaUsage {
            tenant_id: "tenant-a".to_string(),
            metric: "api_calls".to_string(),
            used: 120,
            limit: None,
        };
        assert!(!unlimited.is_exceeded());
        assert_eq!(unlimited.remaining(), None);
    }
}
