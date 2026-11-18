//! 三张配置表的数据访问（**只经 `platform-db` 的租户作用域入口**）。
//!
//! | 表 | 作用域 | 访问方式 |
//! |----|--------|----------|
//! | `platform_config` | 平台级（无 `tenant_id`、无 RLS） | 伪租户 `"platform"` + `{tenant} IS NOT NULL`（REQ-009 的既有模式） |
//! | `platform_tenant_config` | 租户级 | `tenant_id = {tenant}`（RLS 兜底） |
//! | `platform_space_config` | 空间级 | `tenant_id = {tenant} AND space_id = ?`（RLS 兜底） |
//! | `platform_config_epoch` | 缓存版本 | `scope_key` 主键，读一条查询两行 |
//!
//! # 平台级表的**写通道**（D56）
//!
//! 直接 `INSERT/UPDATE/DELETE ON platform_config` 已被迁移 `REVOKE`；
//! 唯一写通道是 `SECURITY DEFINER` 的 `platform_config_upsert(TEXT, JSONB)`，
//! 它在**同一事务**里自增 `scope_key='platform'` 的 epoch（D71）——
//! 写进去与失效信号必须绑在一起，否则缓存永不失效。
//!
//! # 伪租户 `"platform"` 是**保留值**（D72）
//!
//! 真实租户 id 取 `"platform"` 会让平台级读取与租户级读取撞车。本模块在对外
//! 入口（`ConfigService`）显式拒绝该取值。

use std::collections::BTreeMap;

use platform_core::config::ConfigScope;
use platform_core::context::RequestContext;
use platform_db::{DbError, TenantDb, TenantQuery, TenantTx};
use serde_json::Value;

use crate::error::ConfigError;

/// 平台级配置表。
pub const PLATFORM_TABLE: &str = "platform_config";
/// 租户级配置表（REQ-006 建，本需求把 `value` 迁成 `jsonb`）。
pub const TENANT_TABLE: &str = "platform_tenant_config";
/// 空间级配置表。
pub const SPACE_TABLE: &str = "platform_space_config";
/// 配置缓存 epoch 表。
pub const EPOCH_TABLE: &str = "platform_config_epoch";
/// 平台级表的「伪租户」（`TenantDb` 的每次访问都要一个租户）。
pub const PLATFORM_SCOPE_TENANT: &str = "platform";
/// epoch 表里平台级那一行的 `scope_key`。
pub const PLATFORM_SCOPE_KEY: &str = "platform";

/// 租户级 epoch 的 `scope_key`：`tenant:<id>`。
pub fn tenant_scope_key(tenant: &str) -> String {
    format!("tenant:{tenant}")
}

/// 读平台级表时用的伪租户上下文。
pub fn platform_ctx() -> RequestContext {
    RequestContext::new().with_tenant(PLATFORM_SCOPE_TENANT)
}

/// 一对 epoch（**逐一比较**，不取 `max` —— D55）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Epochs {
    /// `scope_key='platform'` 的 epoch；没有行时为 0。
    pub platform: i64,
    /// `scope_key='tenant:<id>'` 的 epoch；没有租户时为 0。
    pub tenant: i64,
}

/// 三层原始行（值已从 `jsonb` 解出）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayerRows {
    /// 平台级。
    pub platform: BTreeMap<String, Value>,
    /// 租户级。
    pub tenant: BTreeMap<String, Value>,
    /// 空间级。
    pub space: BTreeMap<String, Value>,
}

/// 配置数据访问。
#[derive(Debug, Clone)]
pub struct ConfigStore {
    db: TenantDb,
}

impl ConfigStore {
    /// 用平台数据访问层构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 底层数据访问层（供服务层开事务 / 测试用）。
    pub fn db(&self) -> &TenantDb {
        &self.db
    }

    // -- 读 ---------------------------------------------------------------

    /// 读一对 epoch（**一条查询、两行**，D55）。
    pub async fn epochs(&self, tenant: Option<&str>) -> Result<Epochs, ConfigError> {
        let ctx = RequestContext::new().with_tenant(tenant.unwrap_or(PLATFORM_SCOPE_TENANT));
        let query = match tenant {
            Some(tenant) => TenantQuery::new(format!(
                "SELECT scope_key, epoch FROM {} \
                 WHERE {{tenant}} IS NOT NULL AND scope_key IN ('{PLATFORM_SCOPE_KEY}', ?) \
                 ORDER BY scope_key",
                self.db.table(EPOCH_TABLE)
            ))
            .bind(tenant_scope_key(tenant)),
            None => TenantQuery::new(format!(
                "SELECT scope_key, epoch FROM {} \
                 WHERE {{tenant}} IS NOT NULL AND scope_key = '{PLATFORM_SCOPE_KEY}'",
                self.db.table(EPOCH_TABLE)
            )),
        };
        let rows: Vec<(String, i64)> = self.db.fetch_all(&ctx, &query).await?;
        let mut epochs = Epochs::default();
        for (scope_key, epoch) in rows {
            if scope_key == PLATFORM_SCOPE_KEY {
                epochs.platform = epoch;
            } else if tenant.is_some_and(|tenant| scope_key == tenant_scope_key(tenant)) {
                epochs.tenant = epoch;
            }
        }
        Ok(epochs)
    }

    /// 在一个事务里读三层的原始值（快照一致）。
    ///
    /// `tenant = None` 时只读平台级（单元层的「无租户上下文」）。
    pub async fn load(
        &self,
        tenant: Option<&str>,
        space: Option<&str>,
    ) -> Result<LayerRows, ConfigError> {
        let read_ctx = RequestContext::new().with_tenant(tenant.unwrap_or(PLATFORM_SCOPE_TENANT));
        let mut tx = self.db.begin(&read_ctx).await?;
        let mut layers = LayerRows::default();

        let platform_query = TenantQuery::new(format!(
            "SELECT key, value::text FROM {} WHERE {{tenant}} IS NOT NULL ORDER BY key",
            self.db.table(PLATFORM_TABLE)
        ));
        layers.platform = decode_rows(tx.fetch_all(&platform_query).await?)?;

        if tenant.is_some() {
            let tenant_query = TenantQuery::new(format!(
                "SELECT key, value::text FROM {} WHERE tenant_id = {{tenant}} ORDER BY key",
                self.db.table(TENANT_TABLE)
            ));
            layers.tenant = decode_rows(tx.fetch_all(&tenant_query).await?)?;

            if let Some(space) = space {
                let space_query = TenantQuery::new(format!(
                    "SELECT key, value::text FROM {} \
                     WHERE tenant_id = {{tenant}} AND space_id = ? ORDER BY key",
                    self.db.table(SPACE_TABLE)
                ))
                .bind(space.to_string());
                layers.space = decode_rows(tx.fetch_all(&space_query).await?)?;
            }
        }

        tx.commit().await?;
        Ok(layers)
    }

    // -- 写 ---------------------------------------------------------------

    /// 写租户级 / 空间级配置（**同一事务**写全部条目 + 自增租户 epoch）。
    ///
    /// 校验已在服务层完成；这里只负责落库。整批要么全成功、要么全失败。
    pub async fn write_scoped(
        &self,
        ctx: &RequestContext,
        scope: ConfigScope,
        entries: &[(String, Value)],
    ) -> Result<(), ConfigError> {
        let mut tx = self.db.begin(ctx).await?;
        for (key, value) in entries {
            let json = encode_value(value)?;
            let query = match scope {
                ConfigScope::Tenant => TenantQuery::new(format!(
                    "INSERT INTO {} (tenant_id, key, value, updated_at) \
                     VALUES ({{tenant}}, ?, ?::jsonb, now()) \
                     ON CONFLICT (tenant_id, key) DO UPDATE \
                     SET value = EXCLUDED.value, updated_at = now()",
                    self.db.table(TENANT_TABLE)
                ))
                .bind(key.clone())
                .bind(json),
                ConfigScope::Space => {
                    let space = ctx
                        .space_id()
                        .ok_or_else(|| ConfigError::SpaceRequired { key: key.clone() })?;
                    TenantQuery::new(format!(
                        "INSERT INTO {} (tenant_id, space_id, key, value, updated_at) \
                         VALUES ({{tenant}}, ?, ?, ?::jsonb, now()) \
                         ON CONFLICT (tenant_id, space_id, key) DO UPDATE \
                         SET value = EXCLUDED.value, updated_at = now()",
                        self.db.table(SPACE_TABLE)
                    ))
                    .bind(space.to_string())
                    .bind(key.clone())
                    .bind(json)
                }
                // 平台级只能走 `write_platform`（唯一写通道）。
                ConfigScope::Platform => {
                    return Err(ConfigError::Internal {
                        reason: "platform writes must go through the SECURITY DEFINER function"
                            .to_string(),
                    });
                }
            };
            tx.execute(&query).await?;
        }
        bump_tenant_epoch(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// 写平台级配置（**唯一写通道**：`platform_config_upsert`）。
    ///
    /// 函数体内**同事务**自增 `scope_key='platform'` 的 epoch（D71），
    /// 所以这里**不再**额外 bump —— 否则一次写入会 +2。
    pub async fn write_platform(&self, entries: &[(String, Value)]) -> Result<(), ConfigError> {
        let ctx = platform_ctx();
        let mut tx = self.db.begin(&ctx).await?;
        for (key, value) in entries {
            let json = encode_value(value)?;
            let query = TenantQuery::new(
                "SELECT platform_config_upsert(?, ?::jsonb) FROM (SELECT 1) AS _config_upsert \
                 WHERE {tenant} IS NOT NULL",
            )
            .bind(key.clone())
            .bind(json);
            // ⚠️ `FROM (SELECT 1)` 让语句在任何 Postgres 版本下都明确是「有 FROM 的 SELECT」；
            // 条件 `{tenant} IS NOT NULL` 恒真（绑定的伪租户非空）。
            tx.execute(&query).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

/// 自增 `tenant:<id>` 的 epoch（**与写操作同一事务**）。
///
/// `INSERT ... SELECT ... WHERE {tenant} IS NOT NULL` 让 `TenantQuery` 的
/// `{tenant}` 硬约束有落点；对 epoch 表而言条件恒真。
async fn bump_tenant_epoch(tx: &mut TenantTx) -> Result<(), DbError> {
    let table = format!("{}.{}", tx.schema(), EPOCH_TABLE);
    let query = TenantQuery::new(format!(
        "INSERT INTO {table} (scope_key, epoch, updated_at) \
         SELECT ?, 1, now() WHERE {{tenant}} IS NOT NULL \
         ON CONFLICT (scope_key) DO UPDATE \
         SET epoch = {table}.epoch + 1, updated_at = now()"
    ))
    .bind(tenant_scope_key(tx.tenant_id()));
    tx.execute(&query).await?;
    Ok(())
}

/// 把 `jsonb` 的文本形式解成 JSON 值。
fn decode_rows(rows: Vec<(String, String)>) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut map = BTreeMap::new();
    for (key, raw) in rows {
        let value = serde_json::from_str(&raw).map_err(|error| ConfigError::Internal {
            reason: format!("the stored config value of `{key}` is not valid JSON: {error}"),
        })?;
        map.insert(key, value);
    }
    Ok(map)
}

/// 把 JSON 值编码成 `jsonb` 的文本形式（`?::jsonb` 绑定用）。
fn encode_value(value: &Value) -> Result<String, ConfigError> {
    serde_json::to_string(value).map_err(|error| ConfigError::Internal {
        reason: format!("the config value cannot be encoded as JSON: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scope_key_shapes_are_the_documented_ones() {
        assert_eq!(tenant_scope_key("tenant-a"), "tenant:tenant-a");
        assert_eq!(PLATFORM_SCOPE_KEY, "platform");
        assert_eq!(PLATFORM_SCOPE_TENANT, "platform");
    }

    #[test]
    fn json_round_trips_through_the_text_form() {
        for value in [
            serde_json::json!("s"),
            serde_json::json!(60),
            serde_json::json!(true),
            serde_json::json!(["a", "b"]),
            serde_json::json!({"density": "compact"}),
        ] {
            let encoded = encode_value(&value).expect("应能编码");
            let decoded: Value = serde_json::from_str(&encoded).expect("应能解码");
            assert_eq!(decoded, value);
        }
    }
}
