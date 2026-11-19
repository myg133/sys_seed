//! 汇率快照的存取（`platform_fx_rate`，REQ-021 §5.3）。
//!
//! # 为什么这张表**没有** `tenant_id`
//!
//! 汇率是**平台级参考数据**：租户 A 与租户 B 必须看到同一份汇率，否则「跨租户
//! 报表口径一致」就不成立。所以本表与 `platform_config` 同类 —— 无 `tenant_id`、
//! 不开 RLS。
//!
//! 而 [`TenantDb`] 的每一次访问都要求租户上下文（这是全仓的硬约束，绕不开），
//! 所以这里沿用 `platform_config` 的**伪租户**模式：上下文租户固定为
//! [`PLATFORM_SCOPE_TENANT`]，语句里写 `{tenant} IS NOT NULL`（恒真）以满足
//! `TenantQuery` 的「必须出现租户占位符」规则。
//!
//! # 换算时**显式**传 `as_of`（R4 / AC-S-097）
//!
//! [`FxRateStore::resolve_rate`] 按 `(from, to, as_of)` **精确匹配**一条快照 ——
//! 不取「最新」、不做区间回退。理由：
//!
//! - 「最新」在并发下不自觉：两次请求之间插了一条新快照，同一个查询两个结果；
//! - 历史单据重算需要**可复现**：同一组入参必须永远得到同一条快照。
//!
//! 代价：调用方必须知道快照的时刻（列表接口 / 管理端提供）。本轮没有管理端
//! （D115），所以这是**已知的**使用限制，不是遗漏。
//!
//! # 时间参数的形态
//!
//! `as_of` 以 **RFC 3339 文本**传入，由 PostgreSQL 的 `timestamptz` 解析
//! （`?::timestamptz`）。这样平台侧不需要 `time` 的 `parsing` feature
//! （本仓 `time` 只启用 `formatting`，见 design §4 / D114），也不用自己写日期解析。
//! 非法文本会由数据库报错 —— HTTP 层在此之前先用
//! `platform_server::i18n::is_rfc3339` 挡成 400（不把 500 暴露给调用方）。

use platform_core::RequestContext;
use platform_core::money::{CurrencyCode, FxRate};
use time::OffsetDateTime;

use crate::error::{DbError, DbResult};
use crate::query::TenantQuery;
use crate::tenant::TenantDb;

/// 汇率快照表。
pub const FX_RATE_TABLE: &str = "platform_fx_rate";
/// 访问平台级表时用的伪租户（与 `platform-config` 同一约定）。
pub const PLATFORM_SCOPE_TENANT: &str = "platform";

/// 一条待写入的汇率快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFxRate {
    /// 源币种。
    pub from: CurrencyCode,
    /// 目标币种。
    pub to: CurrencyCode,
    /// 汇率 × 10^6（必须为正）。
    pub rate_micros: i64,
    /// 快照生效时刻（RFC 3339 文本，如 `2026-01-01T00:00:00Z`）。
    pub as_of: String,
    /// 来源（`manual` / `ecb` …）。
    pub source: String,
}

/// 查询结果行（DB 形状；币种是裸文本，转 [`FxRate`] 时才校验）。
#[derive(Debug, Clone)]
pub struct FxRateRow {
    /// 行 ID。
    pub id: i64,
    /// 源币种（`CHAR(3)`）。
    pub from_currency: String,
    /// 目标币种（`CHAR(3)`）。
    pub to_currency: String,
    /// 汇率 × 10^6。
    pub rate_micros: i64,
    /// 快照时刻。
    pub as_of: OffsetDateTime,
    /// 来源。
    pub source: String,
}

crate::tenant_row!(FxRateRow {
    id: "id",
    from_currency: "from_currency",
    to_currency: "to_currency",
    rate_micros: "rate_micros",
    as_of: "as_of",
    source: "source",
});

impl FxRateRow {
    /// 转成 `platform-core` 的 [`FxRate`]（币种必须是现行 ISO 4217 代码）。
    ///
    /// `CHAR(3)` 是定长类型，读取时可能带尾随空格 —— 先 `trim_end` 再解析。
    pub fn into_rate(self) -> DbResult<FxRate> {
        let from = CurrencyCode::parse(self.from_currency.trim_end()).map_err(|error| {
            DbError::InvalidQuery {
                reason: format!(
                    "the stored from_currency `{}` is not a current ISO 4217 code: {}",
                    self.from_currency,
                    error.detail()
                ),
            }
        })?;
        let to = CurrencyCode::parse(self.to_currency.trim_end()).map_err(|error| {
            DbError::InvalidQuery {
                reason: format!(
                    "the stored to_currency `{}` is not a current ISO 4217 code: {}",
                    self.to_currency,
                    error.detail()
                ),
            }
        })?;
        Ok(FxRate {
            id: self.id,
            from,
            to,
            rate_micros: self.rate_micros,
            as_of: self.as_of,
            source: self.source,
        })
    }
}

/// 汇率快照的存取。
///
/// 只经 [`TenantDb`] 访问 —— 拿不到裸连接（`platform-db` 的硬约束）。
#[derive(Debug, Clone)]
pub struct FxRateStore {
    db: TenantDb,
}

impl FxRateStore {
    /// 用平台数据访问层构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 平台级访问用的请求上下文（伪租户，与 `ConfigStore` 同一模式）。
    fn platform_ctx() -> RequestContext {
        RequestContext::new().with_tenant(PLATFORM_SCOPE_TENANT)
    }

    /// 写入（或覆盖）一条快照，返回落库后的行。
    ///
    /// # 幂等
    ///
    /// `(from_currency, to_currency, as_of)` 上有唯一约束，冲突时
    /// `DO UPDATE` 覆盖 `rate_micros` / `source` —— 同一份夹具反复播种不会插出重复行
    /// （D101：夹具独立幂等）。
    ///
    /// # 本轮没有 HTTP 写接口（D115）
    ///
    /// 只有 store API：测试自己播种，生产靠 SQL / 后续迁移。
    pub async fn insert_rate(&self, rate: &NewFxRate) -> DbResult<FxRate> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "INSERT INTO {} (from_currency, to_currency, rate_micros, as_of, source) \
             SELECT ?, ?, ?, ?::timestamptz, ? WHERE {{tenant}} IS NOT NULL \
             ON CONFLICT (from_currency, to_currency, as_of) DO UPDATE \
             SET rate_micros = EXCLUDED.rate_micros, source = EXCLUDED.source \
             RETURNING id, from_currency, to_currency, rate_micros, as_of, source",
            self.db.table(FX_RATE_TABLE)
        ))
        .bind(rate.from.as_str().to_string())
        .bind(rate.to.as_str().to_string())
        .bind(rate.rate_micros)
        .bind(rate.as_of.clone())
        .bind(rate.source.clone());

        let row: FxRateRow = self.db.fetch_one(&ctx, &query).await?;
        row.into_rate()
    }

    /// 按 `(from, to, as_of)` **精确**取一条快照。
    ///
    /// 没有这一条 → `Ok(None)`（由调用方映射成 `FX_RATE_NOT_FOUND` / 404）。
    /// **不会**退回「最近的一条」—— 见模块文档。
    pub async fn resolve_rate(
        &self,
        from: CurrencyCode,
        to: CurrencyCode,
        as_of: &str,
    ) -> DbResult<Option<FxRate>> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT id, from_currency, to_currency, rate_micros, as_of, source FROM {} \
             WHERE {{tenant}} IS NOT NULL \
               AND from_currency = ? AND to_currency = ? AND as_of = ?::timestamptz",
            self.db.table(FX_RATE_TABLE)
        ))
        .bind(from.as_str().to_string())
        .bind(to.as_str().to_string())
        .bind(as_of.to_string());

        let row: Option<FxRateRow> = self.db.fetch_optional(&ctx, &query).await?;
        row.map(FxRateRow::into_rate).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表名 / 伪租户是**公开常量**（store 之外只有测试与装配层用到）。
    #[test]
    fn the_table_and_pseudo_tenant_are_the_documented_ones() {
        assert_eq!(FX_RATE_TABLE, "platform_fx_rate");
        assert_eq!(PLATFORM_SCOPE_TENANT, "platform");
    }

    /// 行 → `FxRate` 的转换会校验币种；`CHAR(3)` 的尾随空格要被兜住。
    #[test]
    fn rows_convert_with_currency_validation() {
        let row = FxRateRow {
            id: 1,
            from_currency: "CNY".to_string(),
            to_currency: "USD ".to_string(),
            rate_micros: 139_500,
            as_of: OffsetDateTime::from_unix_timestamp(1_767_225_600).expect("合法时间"),
            source: "manual".to_string(),
        };
        let rate = row.into_rate().expect("合法行应能转换");
        assert_eq!(rate.from.as_str(), "CNY");
        assert_eq!(rate.to.as_str(), "USD");

        let broken = FxRateRow {
            id: 2,
            from_currency: "ZZZ".to_string(),
            to_currency: "USD".to_string(),
            rate_micros: 1,
            as_of: OffsetDateTime::from_unix_timestamp(0).expect("合法时间"),
            source: "manual".to_string(),
        };
        let error = broken.into_rate().expect_err("非法币种应被拒");
        assert_eq!(error.code(), "INVALID_QUERY");
    }
}
