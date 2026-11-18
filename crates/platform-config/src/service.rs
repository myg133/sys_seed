//! 配置服务：**校验顺序** + 三层读写 + 品牌聚合 + Feature Flag 编排。
//!
//! # 校验顺序**写死**（D64，不能换）
//!
//! ```text
//! 1. 认证（无令牌 → 401）                —— HTTP 中间件
//! 2. 权限点（缺 → 403）                  —— 静态 route_layer
//! 3. 键名格式（→ 400 CONFIG_KEY_INVALID）
//! 4. 注册表（写 → 400；读 → 404 CONFIG_KEY_NOT_REGISTERED）
//! 5. 作用域（→ 400 CONFIG_SCOPE_NOT_ALLOWED）
//! 6. 类型 / 范围（→ 400 CONFIG_TYPE_MISMATCH / CONFIG_VALUE_OUT_OF_RANGE）
//! ```
//!
//! 不写顺序时，「被拦住」可能来自任意一条规则，无法证明**具体规则**生效
//! （例如越级写会把类型校验顶包 —— 类型校验根本没被验到）。
//! 本模块对**整批**条目**逐阶段**校验（不是逐条校验完就写），
//! 所以同一请求里「未登记」一定先于「类型错」返回（AC-S-051 步骤 5）。
//!
//! # flag 命名空间隔离（D70）
//!
//! 通用写路由收到**已登记的 flag 名**或 `flags.` 前缀的键 → **400
//! `CONFIG_FLAG_USE_FLAG_ENDPOINT`**。否则持 `config:entry:write` 的人就能改开关，
//! `config:flag:write` 被架空（这是**权限提升**，不是风格问题）。
//!
//! # 平台级读的键级判定（AC-S-063 / D69）
//!
//! 通用读路由收到**登记 `scope` 为 `platform`** 的键 → **整请求 403**
//! （不是「返回其余键、忽略平台级键」）。注意判据是**登记作用域**，
//! 不是「这次命中了平台层」：`favicon` 是 `space` 作用域，即使回落平台值也不需要
//! `config:platform:read`。

use std::collections::BTreeSet;
use std::sync::Arc;

use platform_core::config::{ConfigKey, ConfigScope};
use platform_core::context::RequestContext;
use platform_core::extension::config::{ConfigResolver, FlagDecision, ResolvedConfig};
use platform_core::extension::manifest::ConfigType;
use platform_db::TenantDb;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{ConfigError, json_type_name};
use crate::flag;
use crate::registry::{ConfigRegistry, RegisteredKey, config_type_name};
use crate::resolver::DbConfigResolver;
use crate::store::{ConfigStore, PLATFORM_SCOPE_TENANT};

/// 写请求的体：`{"scope": "tenant|space|platform", "entries": [{"key","value"}]}`。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteRequest {
    /// 写入作用域（必须与路由匹配）。
    pub scope: String,
    /// 条目（整批要么全成功、要么全失败）。
    pub entries: Vec<WriteEntry>,
}

/// 一条写条目。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteEntry {
    /// 配置键。
    pub key: String,
    /// JSON 原生值。
    pub value: Value,
}

/// 改 flag 的请求体：`{"enabled": true, "rolloutPercent": 50}`（两者都可选）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlagWriteRequest {
    /// 总开关（布尔）。
    #[serde(default)]
    pub enabled: Option<Value>,
    /// 灰度比例（整数 `0..=100`，写入派生键 `flags.<flag>.rolloutPercent`）。
    #[serde(default)]
    pub rollout_percent: Option<Value>,
}

/// 写路由的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteRoute {
    /// `PUT /api/config/entries`：租户级 / 空间级。
    Scoped,
    /// `PUT /api/config/platform-entries`：平台级（独立路由，D59）。
    Platform,
}

/// 配置服务。
#[derive(Debug, Clone)]
pub struct ConfigService {
    store: ConfigStore,
    registry: Arc<ConfigRegistry>,
    resolver: Arc<DbConfigResolver>,
}

impl ConfigService {
    /// 用数据访问层与注册表构造（内部持有解析器与其缓存）。
    pub fn new(db: TenantDb, registry: Arc<ConfigRegistry>) -> Self {
        let store = ConfigStore::new(db);
        let resolver = Arc::new(DbConfigResolver::new(store.clone(), registry.clone()));
        Self {
            store,
            registry,
            resolver,
        }
    }

    /// 底层的解析器（HTTP 中间件在每个请求前调用 `refresh`）。
    pub fn resolver(&self) -> Arc<DbConfigResolver> {
        self.resolver.clone()
    }

    /// 注册表。
    pub fn registry(&self) -> &ConfigRegistry {
        &self.registry
    }

    // -- 读 ---------------------------------------------------------------

    /// `GET /api/config/entries?keys=a,b`：租户级 / 空间级读。
    pub async fn read_entries(
        &self,
        ctx: &RequestContext,
        raw_keys: &[String],
    ) -> Result<Vec<ResolvedConfig>, ConfigError> {
        self.ensure_tenant_not_reserved(ctx)?;
        let keys = validated_keys(raw_keys)?;

        // 第 4 步：注册表（未登记 → 整请求 404，D76）。
        let unknown = self.unknown_keys(&keys);
        if !unknown.is_empty() {
            return Err(ConfigError::UnknownKeys { keys: unknown });
        }

        // 第 5 步（读侧）：平台级键 → 整请求 403（AC-S-063 / 契约 C9 §9.7）。
        let platform_keys: Vec<String> = keys
            .iter()
            .filter(|key| {
                self.registry
                    .get(key)
                    .is_some_and(|spec| spec.scope == ConfigScope::Platform)
            })
            .cloned()
            .collect();
        if !platform_keys.is_empty() {
            return Err(ConfigError::PlatformKeyOnGenericRoute {
                keys: platform_keys,
            });
        }

        self.resolver.refresh(ctx).await?;
        let mut entries = Vec::with_capacity(keys.len());
        for key in keys {
            let config_key =
                ConfigKey::parse(&key).map_err(|_| ConfigError::KeyInvalid { key: key.clone() })?;
            let resolved = self
                .resolver
                .resolve(ctx, &config_key)
                .ok_or_else(|| ConfigError::KeyNotRegistered { key: key.clone() })?;
            entries.push(resolved);
        }
        Ok(entries)
    }

    /// `GET /api/config/platform-entries?keys=a,b`：平台级读（独立路由，D69）。
    pub async fn read_platform_entries(
        &self,
        ctx: &RequestContext,
        raw_keys: &[String],
    ) -> Result<Vec<ResolvedConfig>, ConfigError> {
        self.ensure_tenant_not_reserved(ctx)?;
        let keys = validated_keys(raw_keys)?;

        let unknown = self.unknown_keys(&keys);
        if !unknown.is_empty() {
            return Err(ConfigError::UnknownKeys { keys: unknown });
        }

        let mut wrong_scope = None;
        for key in &keys {
            let spec = self.registry.get(key).expect("刚刚校验过注册表");
            if spec.scope != ConfigScope::Platform {
                wrong_scope = Some((key.clone(), spec.scope));
                break;
            }
        }
        if let Some((key, scope)) = wrong_scope {
            return Err(ConfigError::ScopeNotAllowed {
                key: Some(key),
                allowed_scope: "platform".to_string(),
                attempted_scope: scope.as_str().to_string(),
                reason: "the platform endpoint only serves platform-scoped keys".to_string(),
            });
        }

        self.resolver.refresh(ctx).await?;
        let mut entries = Vec::with_capacity(keys.len());
        for key in keys {
            let config_key =
                ConfigKey::parse(&key).map_err(|_| ConfigError::KeyInvalid { key: key.clone() })?;
            let resolved = self
                .resolver
                .resolve_platform(ctx, &config_key)
                .ok_or_else(|| ConfigError::KeyNotRegistered { key: key.clone() })?;
            entries.push(resolved);
        }
        Ok(entries)
    }

    /// `GET /api/config/branding`：公开的品牌扁平 map（白名单 + 忽略空间级）。
    pub async fn branding(
        &self,
        tenant: Option<&str>,
    ) -> Result<Vec<(String, Value)>, ConfigError> {
        if tenant == Some(PLATFORM_SCOPE_TENANT) {
            return Err(ConfigError::ReservedTenant {
                tenant: PLATFORM_SCOPE_TENANT.to_string(),
            });
        }
        self.resolver.branding(tenant).await
    }

    /// `GET /api/feature-flags/{flag}`。
    pub async fn read_flag(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> Result<FlagDecision, ConfigError> {
        self.ensure_tenant_not_reserved(ctx)?;
        ensure_valid_flag_name(name)?;
        self.resolver.refresh(ctx).await?;
        Ok(self.resolver.flag(ctx, name))
    }

    /// `GET /api/feature-flags`：本租户全部已登记开关。
    pub async fn read_flags(&self, ctx: &RequestContext) -> Result<Vec<FlagDecision>, ConfigError> {
        self.ensure_tenant_not_reserved(ctx)?;
        self.resolver.refresh(ctx).await?;
        Ok(self
            .registry
            .flags()
            .map(|name| self.resolver.flag(ctx, name))
            .collect())
    }

    // -- 写 ---------------------------------------------------------------

    /// 写配置（通用路由 / 平台路由）。
    pub async fn write_entries(
        &self,
        ctx: &RequestContext,
        request: &WriteRequest,
        route: WriteRoute,
    ) -> Result<usize, ConfigError> {
        self.ensure_tenant_not_reserved(ctx)?;

        // 路由与 body 的 scope 必须匹配（D59 / AC-S-060 步骤 4）。
        let scope = parse_scope(&request.scope)?;
        match route {
            WriteRoute::Scoped if scope == ConfigScope::Platform => {
                return Err(ConfigError::ScopeNotAllowed {
                    key: None,
                    allowed_scope: "tenant|space".to_string(),
                    attempted_scope: "platform".to_string(),
                    reason: "platform-scoped writes use PUT /api/config/platform-entries"
                        .to_string(),
                });
            }
            WriteRoute::Platform if scope != ConfigScope::Platform => {
                return Err(ConfigError::ScopeNotAllowed {
                    key: None,
                    allowed_scope: "platform".to_string(),
                    attempted_scope: scope.as_str().to_string(),
                    reason: "this endpoint only accepts scope=platform".to_string(),
                });
            }
            _ => {}
        }

        // 第 3 步：键名格式（整批先查完）。
        for entry in &request.entries {
            ConfigKey::parse(&entry.key).map_err(|_| ConfigError::KeyInvalid {
                key: entry.key.clone(),
            })?;
        }

        // 第 4 步：注册表。
        for entry in &request.entries {
            if self.registry.get(&entry.key).is_none() {
                return Err(ConfigError::KeyNotRegistered {
                    key: entry.key.clone(),
                });
            }
        }

        // flag 命名空间（D70）：在作用域/类型之前拒绝，避免「写进去后才发现」。
        for entry in &request.entries {
            if self.registry.is_flag_namespace(&entry.key) {
                return Err(ConfigError::FlagEndpointRequired {
                    key: entry.key.clone(),
                });
            }
        }

        // 空间级写必须带空间身份（C3 缺失 = 不限空间，不能用来定位一行）。
        if scope == ConfigScope::Space {
            for entry in &request.entries {
                self.require_space(ctx, &entry.key)?;
            }
        }

        // 第 5 步：作用域（登记的 scope = 允许覆盖到的最深层）。
        for entry in &request.entries {
            let spec = self.registry.get(&entry.key).expect("上一步已确认登记过");
            if scope.rank() > spec.scope.rank() {
                return Err(ConfigError::ScopeNotAllowed {
                    key: Some(entry.key.clone()),
                    allowed_scope: spec.scope.as_str().to_string(),
                    attempted_scope: scope.as_str().to_string(),
                    reason: "the registered scope is the deepest writable layer".to_string(),
                });
            }
        }

        // 第 5 步（D66 的第二道门）：空间级写还要求键在品牌白名单内。
        if scope == ConfigScope::Space {
            for entry in &request.entries {
                if !crate::whitelist::is_branding_key(&entry.key) {
                    return Err(ConfigError::ScopeNotAllowed {
                        key: Some(entry.key.clone()),
                        allowed_scope: "space + branding whitelist".to_string(),
                        attempted_scope: "space".to_string(),
                        reason: "whitelist".to_string(),
                    });
                }
            }
        }

        // 第 6 步：类型 + 范围。
        for entry in &request.entries {
            let spec = self.registry.get(&entry.key).expect("上一步已确认登记过");
            self.check_type_and_range(&entry.key, &entry.value, spec)?;
        }

        let entries: Vec<(String, Value)> = request
            .entries
            .iter()
            .map(|entry| (entry.key.clone(), entry.value.clone()))
            .collect();
        match route {
            WriteRoute::Scoped => self.store.write_scoped(ctx, scope, &entries).await?,
            WriteRoute::Platform => self.store.write_platform(&entries).await?,
        }
        Ok(entries.len())
    }

    /// `PUT /api/feature-flags/{flag}`：改开关（含灰度比例）。
    pub async fn write_flag(
        &self,
        ctx: &RequestContext,
        name: &str,
        request: &FlagWriteRequest,
    ) -> Result<FlagDecision, ConfigError> {
        self.ensure_tenant_not_reserved(ctx)?;
        ensure_valid_flag_name(name)?;
        if !self.registry.is_flag(name) {
            return Err(ConfigError::KeyNotRegistered {
                key: name.to_string(),
            });
        }

        let mut entries: Vec<(String, Value)> = Vec::new();
        if let Some(value) = &request.enabled {
            if !value.is_boolean() {
                return Err(ConfigError::TypeMismatch {
                    key: name.to_string(),
                    expected: "boolean".to_string(),
                    actual: json_type_name(value).to_string(),
                });
            }
            entries.push((name.to_string(), value.clone()));
        }
        if let Some(value) = &request.rollout_percent {
            let key = flag::rollout_key(name);
            if !value.is_number() {
                return Err(ConfigError::TypeMismatch {
                    key,
                    expected: "number".to_string(),
                    actual: json_type_name(value).to_string(),
                });
            }
            let Some(percent) = value.as_i64() else {
                return Err(ConfigError::ValueOutOfRange {
                    key,
                    value: value.clone(),
                    allowed: format!(
                        "{}..={}",
                        flag::ROLLOUT_RANGE.start(),
                        flag::ROLLOUT_RANGE.end()
                    ),
                });
            };
            if !flag::ROLLOUT_RANGE.contains(&percent) {
                return Err(ConfigError::ValueOutOfRange {
                    key,
                    value: value.clone(),
                    allowed: format!(
                        "{}..={}",
                        flag::ROLLOUT_RANGE.start(),
                        flag::ROLLOUT_RANGE.end()
                    ),
                });
            }
            entries.push((key, value.clone()));
        }

        if !entries.is_empty() {
            self.store
                .write_scoped(ctx, ConfigScope::Tenant, &entries)
                .await?;
        }
        self.resolver.refresh(ctx).await?;
        Ok(self.resolver.flag(ctx, name))
    }

    // -- 内部 -------------------------------------------------------------

    fn require_space(&self, ctx: &RequestContext, key: &str) -> Result<(), ConfigError> {
        if ctx.space_id().is_none() {
            return Err(ConfigError::SpaceRequired {
                key: key.to_string(),
            });
        }
        Ok(())
    }

    fn ensure_tenant_not_reserved(&self, ctx: &RequestContext) -> Result<(), ConfigError> {
        // D72：`"platform"` 是伪租户的保留值；真实租户 id 取它会让平台级读取
        // 与租户级读取撞车。没有租户注册表可校验，所以在**对外的读写入口**拒绝。
        if let Some(tenant) = ctx.tenant_id()
            && tenant == PLATFORM_SCOPE_TENANT
        {
            return Err(ConfigError::ReservedTenant {
                tenant: tenant.to_string(),
            });
        }
        Ok(())
    }

    fn unknown_keys(&self, keys: &[String]) -> Vec<String> {
        keys.iter()
            .filter(|key| self.registry.get(key).is_none())
            .cloned()
            .collect()
    }

    fn check_type_and_range(
        &self,
        key: &str,
        value: &Value,
        spec: &RegisteredKey,
    ) -> Result<(), ConfigError> {
        if !value_matches(value, spec.kind) {
            return Err(ConfigError::TypeMismatch {
                key: key.to_string(),
                expected: config_type_name(spec.kind).to_string(),
                actual: json_type_name(value).to_string(),
            });
        }
        // 范围只对派生键 `flags.<F>.rolloutPercent` 有定义。
        if spec.derived && key.ends_with(".rolloutPercent") {
            let Some(percent) = value.as_i64() else {
                return Err(ConfigError::ValueOutOfRange {
                    key: key.to_string(),
                    value: value.clone(),
                    allowed: format!(
                        "{}..={}",
                        flag::ROLLOUT_RANGE.start(),
                        flag::ROLLOUT_RANGE.end()
                    ),
                });
            };
            if !flag::ROLLOUT_RANGE.contains(&percent) {
                return Err(ConfigError::ValueOutOfRange {
                    key: key.to_string(),
                    value: value.clone(),
                    allowed: format!(
                        "{}..={}",
                        flag::ROLLOUT_RANGE.start(),
                        flag::ROLLOUT_RANGE.end()
                    ),
                });
            }
        }
        Ok(())
    }
}

/// 解析作用域字符串。
fn parse_scope(raw: &str) -> Result<ConfigScope, ConfigError> {
    match raw {
        "platform" => Ok(ConfigScope::Platform),
        "tenant" => Ok(ConfigScope::Tenant),
        "space" => Ok(ConfigScope::Space),
        other => Err(ConfigError::ScopeNotAllowed {
            key: None,
            allowed_scope: "platform|tenant|space".to_string(),
            attempted_scope: other.to_string(),
            reason: "unknown scope".to_string(),
        }),
    }
}

/// 键名格式校验 + 去重（保留请求顺序）。
fn validated_keys(raw_keys: &[String]) -> Result<Vec<String>, ConfigError> {
    let mut keys = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in raw_keys {
        let parsed =
            ConfigKey::parse(raw).map_err(|_| ConfigError::KeyInvalid { key: raw.clone() })?;
        let key = parsed.as_str().to_string();
        if seen.insert(key.clone()) {
            keys.push(key);
        }
    }
    Ok(keys)
}

/// flag 名格式校验（非法名 → 400，且**不进日志**，D65）。
fn ensure_valid_flag_name(name: &str) -> Result<(), ConfigError> {
    if !flag::is_valid_flag_name(name) {
        return Err(ConfigError::FlagNameInvalid {
            flag: flag::display_name(name),
        });
    }
    Ok(())
}

/// JSON 类型是否匹配登记的 `ConfigType`（`"20"` **不等于** `20`，布尔不是数字）。
fn value_matches(value: &Value, kind: ConfigType) -> bool {
    match kind {
        ConfigType::String => value.is_string(),
        ConfigType::Number => value.is_number(),
        ConfigType::Boolean => value.is_boolean(),
        ConfigType::Array => value.is_array(),
        ConfigType::Object => value.is_object(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_strings_round_trip_and_default_is_rejected() {
        assert_eq!(parse_scope("platform"), Ok(ConfigScope::Platform));
        assert_eq!(parse_scope("tenant"), Ok(ConfigScope::Tenant));
        assert_eq!(parse_scope("space"), Ok(ConfigScope::Space));
        let error = parse_scope("default").expect_err("default 不是可写作用域");
        assert_eq!(error.code(), crate::error::CODE_SCOPE_NOT_ALLOWED);
    }

    #[test]
    fn key_validation_happens_before_anything_else() {
        let error = validated_keys(&["Theme.radius".to_string()]).expect_err("非法键名应被拒");
        assert_eq!(error.code(), crate::error::CODE_KEY_INVALID);
    }

    #[test]
    fn duplicate_keys_are_deduped_in_request_order() {
        let keys = validated_keys(&["logo".to_string(), "title".to_string(), "logo".to_string()])
            .expect("合法键");
        assert_eq!(keys, vec!["logo", "title"]);
    }

    #[test]
    fn json_types_are_judged_strictly() {
        assert!(value_matches(&Value::from("20"), ConfigType::String));
        assert!(!value_matches(&Value::from("20"), ConfigType::Number));
        assert!(!value_matches(&Value::from(true), ConfigType::Number));
        assert!(value_matches(&Value::from(20), ConfigType::Number));
        assert!(value_matches(&Value::from(true), ConfigType::Boolean));
        assert!(value_matches(&serde_json::json!([]), ConfigType::Array));
        assert!(value_matches(&serde_json::json!({}), ConfigType::Object));
    }

    #[test]
    fn flag_names_are_validated_with_the_contract_rule() {
        assert!(ensure_valid_flag_name("beta").is_ok());
        for bad in ["bad\nINJECTED", "new-invoice", &"a".repeat(1024)] {
            let error = ensure_valid_flag_name(bad).expect_err("非法 flag 名应被拒");
            assert_eq!(error.code(), crate::error::CODE_FLAG_NAME_INVALID);
        }
    }
}
