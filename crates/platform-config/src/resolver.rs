//! 数据库支撑的配置解析器：三层覆盖 + **成对 epoch** 缓存失效 + 确定性灰度。
//!
//! # 失效判据（D55，**不要改成 `max(epoch)`**）
//!
//! 缓存项存一对 `(platform_epoch, tenant_epoch)`；**逐一相等**才算新鲜：
//!
//! ```text
//! 新鲜 = (缓存.platform_epoch == 库里.platform) && (缓存.tenant_epoch == 库里.tenant)
//! ```
//!
//! 反例（rev 1 的错误写法）：`max(epoch)`。租户 epoch = 7、platform epoch = 3 时，
//! 管理员改平台级配置只让 platform 变成 4，`max(7, 4) = 7` 不变 → 该租户的缓存
//! 被判为新鲜 → 继续返回旧值。**只要某个租户先改过自己的配置，之后所有平台级
//! 变更对它都失效不掉**。AC-S-053 步骤 3 专门构造这个顺序。
//!
//! # TTL 的定位
//!
//! 配置里登记了 `cacheTtlSeconds`（默认 60 / 夹具设 300）。本实现的**正确性
//! 不依赖 TTL**：每个请求先比对 epoch。TTL 只作为「即使 epoch 没变也最多缓存
//! 这么久」的兜底上限（防御性），因此 TTL 再长也不影响「下一个请求生效」。
//!
//! # 为什么 `refresh` 是异步的而 trait 方法是同步的
//!
//! `ConfigResolver` trait 要能 `Arc<dyn ConfigResolver>`（业务模块调用），
//! 所以方法必须 dyn 兼容（同步）。数据访问（读 epoch、必要时重算）由平台在
//! **调用前**用 [`DbConfigResolver::refresh`] 完成；trait 的 `resolve` / `flag`
//! 只从已刷新的快照里取值 —— 拿不到快照时 **fail closed**（`None` / 关闭）。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use platform_core::config::{ConfigKey, ConfigScope};
use platform_core::context::RequestContext;
use platform_core::extension::config::{
    ConfigResolver, ConfigSource, FlagDecision, ResolvedConfig,
};
use serde_json::Value;

use crate::error::{ConfigError, json_type_name};
use crate::flag;
use crate::registry::{ConfigRegistry, RegisteredKey};
use crate::store::{ConfigStore, LayerRows, PLATFORM_SCOPE_TENANT};
use crate::whitelist::is_branding_key;

/// 默认的缓存兜底 TTL（与 `cacheTtlSeconds` 的默认值一致）。
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(60);

/// 缓存键：(租户, 空间)。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    tenant: Option<String>,
    space: Option<String>,
}

/// 一份已解析的三层快照。
#[derive(Debug, Clone)]
struct CachedSnapshot {
    platform_epoch: i64,
    tenant_epoch: i64,
    layers: LayerRows,
    loaded_at: Instant,
}

/// 数据库支撑的配置解析器。
#[derive(Debug)]
pub struct DbConfigResolver {
    store: ConfigStore,
    registry: Arc<ConfigRegistry>,
    cache: RwLock<HashMap<CacheKey, CachedSnapshot>>,
    ttl: Duration,
}

impl DbConfigResolver {
    /// 用数据访问层与注册表构造（TTL 取默认值）。
    pub fn new(store: ConfigStore, registry: Arc<ConfigRegistry>) -> Self {
        Self::with_ttl(store, registry, DEFAULT_CACHE_TTL)
    }

    /// 指定缓存兜底 TTL。
    pub fn with_ttl(store: ConfigStore, registry: Arc<ConfigRegistry>, ttl: Duration) -> Self {
        Self {
            store,
            registry,
            cache: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// 注册表。
    pub fn registry(&self) -> &ConfigRegistry {
        &self.registry
    }

    /// 缓存兜底 TTL。
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// **每个请求调用一次**：读一对 epoch，不一致（或超过 TTL）就重算快照。
    ///
    /// 这是「改完下一个请求生效」的实现点；失效信号来自**共享的数据库**，
    /// 不是进程内广播（AC-S-052 步骤 4 / AC-S-053）。
    pub async fn refresh(&self, ctx: &RequestContext) -> Result<(), ConfigError> {
        let tenant = ctx.tenant_id();
        if tenant == Some(PLATFORM_SCOPE_TENANT) {
            return Err(ConfigError::ReservedTenant {
                tenant: tenant.unwrap_or_default().to_string(),
            });
        }
        let key = CacheKey {
            tenant: tenant.map(str::to_string),
            space: ctx.space_id().map(str::to_string),
        };

        let epochs = self.store.epochs(tenant).await?;
        if let Some(cached) = self.cache.read().ok().and_then(|cache| {
            cache.get(&key).map(|cached| {
                cached.platform_epoch == epochs.platform && cached.tenant_epoch == epochs.tenant
            })
        }) {
            let fresh_by_ttl = self
                .cache
                .read()
                .ok()
                .and_then(|cache| {
                    cache
                        .get(&key)
                        .map(|cached| cached.loaded_at.elapsed() < self.ttl)
                })
                .unwrap_or(false);
            if cached && fresh_by_ttl {
                return Ok(());
            }
        }

        let layers = self.store.load(tenant, ctx.space_id()).await?;
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(
                key,
                CachedSnapshot {
                    platform_epoch: epochs.platform,
                    tenant_epoch: epochs.tenant,
                    layers,
                    loaded_at: Instant::now(),
                },
            );
        }
        Ok(())
    }

    /// 平台级读取（`GET /api/config/platform-entries`）：只命中平台层或默认值。
    ///
    /// 调用方负责先校验「该键登记为 `scope: platform`」。
    pub fn resolve_platform(
        &self,
        ctx: &RequestContext,
        key: &ConfigKey,
    ) -> Option<ResolvedConfig> {
        let spec = self.registry.get(key.as_str())?;
        if spec.scope != ConfigScope::Platform {
            return None;
        }
        let layers = self.cached_layers(ctx)?;
        if let Some(value) = layers.platform.get(key.as_str()) {
            return Some(ResolvedConfig {
                key: key.as_str().to_string(),
                value: value.clone(),
                source: ConfigSource::Platform,
            });
        }
        Some(ResolvedConfig {
            key: key.as_str().to_string(),
            value: spec.default.clone(),
            source: ConfigSource::Default,
        })
    }

    /// 快照里该上下文对应的三层。
    ///
    /// 没有快照 → `None`（**fail closed**：调用方必须先 `refresh`）。
    fn cached_layers(&self, ctx: &RequestContext) -> Option<LayerRows> {
        let key = CacheKey {
            tenant: ctx.tenant_id().map(str::to_string),
            space: ctx.space_id().map(str::to_string),
        };
        self.cache
            .read()
            .ok()
            .and_then(|cache| cache.get(&key).map(|cached| cached.layers.clone()))
    }

    /// 站点品牌：只返回白名单键（供 `GET /api/config/branding`）。
    ///
    /// **忽略空间级覆盖**（未认证请求不带可信空间身份，design §4.1）——
    /// 调用方传入的 ctx 即使带 space，这里也只用平台级 + 租户级。
    pub async fn branding(
        &self,
        tenant: Option<&str>,
    ) -> Result<Vec<(String, Value)>, ConfigError> {
        if tenant == Some(PLATFORM_SCOPE_TENANT) {
            return Err(ConfigError::ReservedTenant {
                tenant: tenant.unwrap_or_default().to_string(),
            });
        }
        let ctx = RequestContext::new().with_tenant(tenant.unwrap_or_default());
        self.refresh(&ctx).await?;

        // 重新构造一个**不带空间**的键读取上下文（refresh 已按无空间建缓存）。
        let read_ctx = RequestContext::new().with_tenant(tenant.unwrap_or_default());
        let layers = self.cached_layers(&read_ctx).unwrap_or_default();
        let mut entries = Vec::with_capacity(crate::whitelist::BRANDING_WHITELIST.len());
        for key in crate::whitelist::BRANDING_WHITELIST {
            if let Some(spec) = self.registry.get(key) {
                let resolved = if let Some(value) = layers.tenant.get(key) {
                    (value.clone(), ConfigSource::Tenant)
                } else if let Some(value) = layers.platform.get(key) {
                    (value.clone(), ConfigSource::Platform)
                } else {
                    (spec.default.clone(), ConfigSource::Default)
                };
                // 品牌接口的用途是「直接渲染」：只给值，不给 `source`（D61）。
                let _ = resolved.1;
                entries.push((key.to_string(), resolved.0));
            }
        }
        Ok(entries)
    }

    /// 该 flag 当前的总开关与灰度比例（快照内），供服务层校验前读取。
    pub fn flag_spec(&self, flag: &str) -> Option<&RegisteredKey> {
        self.registry.get(flag)
    }
}

impl ConfigResolver for DbConfigResolver {
    fn resolve(&self, ctx: &RequestContext, key: &ConfigKey) -> Option<ResolvedConfig> {
        let spec = self.registry.get(key.as_str())?;
        let layers = self.cached_layers(ctx)?;

        // 空间级：**只对登记为 space 且在白名单内的键生效**（D66 / §4.1）。
        if let Some(space_id) = ctx.space_id() {
            let _ = space_id;
            if spec.scope == ConfigScope::Space
                && is_branding_key(key.as_str())
                && let Some(value) = layers.space.get(key.as_str())
            {
                return Some(ResolvedConfig {
                    key: key.as_str().to_string(),
                    value: value.clone(),
                    source: ConfigSource::Space,
                });
            }
        }

        if let Some(value) = layers.tenant.get(key.as_str()) {
            return Some(ResolvedConfig {
                key: key.as_str().to_string(),
                value: value.clone(),
                source: ConfigSource::Tenant,
            });
        }
        if let Some(value) = layers.platform.get(key.as_str()) {
            return Some(ResolvedConfig {
                key: key.as_str().to_string(),
                value: value.clone(),
                source: ConfigSource::Platform,
            });
        }
        Some(ResolvedConfig {
            key: key.as_str().to_string(),
            value: spec.default.clone(),
            source: ConfigSource::Default,
        })
    }

    fn flag(&self, ctx: &RequestContext, flag_name: &str) -> FlagDecision {
        // 未登记的 flag：**不返回 404**，返回关闭 + default，并留一条结构化 WARN。
        if !self.registry.is_flag(flag_name) {
            tracing::warn!(
                flag = %flag::display_name(flag_name),
                "the feature flag is not registered; returning disabled (fail closed)"
            );
            return FlagDecision::disabled(flag_name);
        }

        let Some(spec) = self.registry.get(flag_name) else {
            return FlagDecision::disabled(flag_name);
        };
        let Some(layers) = self.cached_layers(ctx) else {
            // 没有快照 = 平台没有先 refresh：**fail closed**。
            return FlagDecision::disabled(flag_name);
        };

        // 总开关：租户 > 平台 > manifest 默认值（flag 无空间级，D51）。
        let (body, source) = if let Some(value) = layers.tenant.get(flag_name) {
            (value.clone(), ConfigSource::Tenant)
        } else if let Some(value) = layers.platform.get(flag_name) {
            (value.clone(), ConfigSource::Platform)
        } else {
            (spec.default.clone(), ConfigSource::Default)
        };

        let Some(enabled) = body.as_bool() else {
            tracing::warn!(
                flag = %flag::display_name(flag_name),
                actual = json_type_name(&body),
                "the stored feature-flag switch is not a boolean; failing closed"
            );
            return FlagDecision {
                flag: flag_name.to_string(),
                enabled: false,
                source,
            };
        };
        if !enabled {
            return FlagDecision {
                flag: flag_name.to_string(),
                enabled: false,
                source,
            };
        }

        // 灰度的判据是「这个**用户**在不在比例里」；没有身份 → fail closed。
        // `rolloutPercent = 100` 也不改变这个结论（缺的不是比例而是身份）。
        let Some(user_id) = ctx.user_id() else {
            return FlagDecision {
                flag: flag_name.to_string(),
                enabled: false,
                source,
            };
        };

        let Some(percent) = self.rollout_percent(flag_name, &layers).flatten() else {
            return FlagDecision {
                flag: flag_name.to_string(),
                enabled: false,
                source,
            };
        };

        FlagDecision {
            flag: flag_name.to_string(),
            enabled: flag::in_rollout(flag_name, user_id, percent),
            source,
        }
    }
}

impl DbConfigResolver {
    /// 读灰度比例：租户 > 平台 > 派生键默认值。
    ///
    /// 库里是脏数据（非整数 / 越界）→ **WARN + `None`**，调用方 fail closed；
    /// **不返回 400**（读它的客户端没有发送非法值，AC-S-057 步骤 6）。
    fn rollout_percent(&self, flag_name: &str, layers: &LayerRows) -> Option<Option<i64>> {
        let key = flag::rollout_key(flag_name);
        let spec = self.registry.get(&key)?;
        let value = layers
            .tenant
            .get(&key)
            .or_else(|| layers.platform.get(&key))
            .cloned()
            .unwrap_or_else(|| spec.default.clone());

        let Some(raw) = value.as_i64() else {
            tracing::warn!(
                flag = %flag::display_name(flag_name),
                value = %value,
                "the stored rolloutPercent is not an integer; failing closed"
            );
            return Some(None);
        };
        if !flag::ROLLOUT_RANGE.contains(&raw) {
            tracing::warn!(
                flag = %flag::display_name(flag_name),
                value = raw,
                "the stored rolloutPercent is out of range 0..=100; failing closed"
            );
            return Some(None);
        }
        Some(Some(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_ttl_is_a_known_constant() {
        assert_eq!(DEFAULT_CACHE_TTL, Duration::from_secs(60));
    }

    #[test]
    fn the_cache_key_distinguishes_tenants_and_spaces() {
        let a = CacheKey {
            tenant: Some("tenant-a".to_string()),
            space: None,
        };
        let b = CacheKey {
            tenant: Some("tenant-a".to_string()),
            space: Some("space-1".to_string()),
        };
        let c = CacheKey {
            tenant: Some("tenant-b".to_string()),
            space: Some("space-1".to_string()),
        };
        assert_ne!(a, b);
        assert_ne!(b, c);
    }
}
