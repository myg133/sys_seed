//! 配置键的**注册表**：唯一来源是各 manifest 的 `config` 段（契约 C9 §9.5 / D53）。
//!
//! | 目录 | 谁 |
//! |------|-----|
//! | `modules/<模块>/manifest.json` | 业务模块 |
//! | `crates/<crate>/manifest.json` | 平台 crate（`platform-config` / `platform-org` / `platform-rbac`） |
//!
//! 口径与 `platform-rbac` 的权限点对账、`platform-db` 的租户作用域收集**一致**：
//! 运行时直接读 manifest（不是读生成物 `capabilities/`）。
//!
//! # 派生键（D57）
//!
//! 为每个**已登记的 flag** `F` 自动存在：
//!
//! ```text
//! flags.<F>.rolloutPercent    type: number    scope: tenant    0..=100    默认 100
//! ```
//!
//! 它**不写进 manifest**，但**视为已声明** —— 因此写它不会触发
//! 「未声明 → 400」；`flags.nosuch.rolloutPercent` 才是未声明。
//!
//! # 哪些 config 键是 flag？
//!
//! 契约 C10 与 design §5.3 的口径：flag 本体登记在 `config` 段、`type: boolean`。
//! 所以本实现把**登记为 `boolean` 的配置键**视为 flag**（当前仓库里只有
//! `newInvoice` / `beta`）。这条推导规则在 `feature-meta/verification-report.md`
//! 里作为「对规格的疑问」记录：规格没有给 flag 单独的声明段，`boolean` 是唯一
//! 可推导的判据。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use platform_core::config::{ConfigKey, ConfigScope};
use platform_core::extension::manifest::{ConfigType, Manifest};
use serde_json::Value;

use crate::error::ConfigError;
use crate::flag::{DEFAULT_ROLLOUT_PERCENT, rollout_key};

/// 一个已登记的配置键。
#[derive(Debug, Clone, PartialEq)]
pub struct RegisteredKey {
    /// 键名。
    pub key: String,
    /// 登记的类型。
    pub kind: ConfigType,
    /// manifest 登记的默认值。
    pub default: Value,
    /// 「允许覆盖到的最深层」（D53）。
    pub scope: ConfigScope,
    /// 是否由 flag 推导出来的派生键。
    pub derived: bool,
    /// 来源 manifest（`crates/platform-config/manifest.json`）。
    pub source: String,
}

impl RegisteredKey {
    /// 登记类型名（错误参数 `expected` 用）。
    pub fn type_name(&self) -> &'static str {
        config_type_name(self.kind)
    }
}

/// 配置类型名。
pub fn config_type_name(kind: ConfigType) -> &'static str {
    match kind {
        ConfigType::String => "string",
        ConfigType::Number => "number",
        ConfigType::Boolean => "boolean",
        ConfigType::Array => "array",
        ConfigType::Object => "object",
    }
}

/// 注册表。
#[derive(Debug, Clone, Default)]
pub struct ConfigRegistry {
    keys: BTreeMap<String, RegisteredKey>,
    flags: BTreeSet<String>,
}

impl ConfigRegistry {
    /// 从 manifest 根目录收集全部已登记的键。
    ///
    /// 扫 `modules/*/manifest.json` 与 `crates/*/manifest.json`。
    pub fn from_manifest_root(root: &Path) -> Result<Self, ConfigError> {
        let mut keys: BTreeMap<String, RegisteredKey> = BTreeMap::new();

        for (dir_name, manifest_root) in [
            ("modules", root.join("modules")),
            ("crates", root.join("crates")),
        ] {
            let Ok(entries) = std::fs::read_dir(&manifest_root) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .map(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
                .collect();
            names.sort();

            for name in names {
                let manifest_path = manifest_root.join(&name).join("manifest.json");
                if !manifest_path.is_file() {
                    continue;
                }
                let raw = std::fs::read_to_string(&manifest_path).map_err(|error| {
                    ConfigError::Internal {
                        reason: format!("{} cannot be read: {error}", manifest_path.display()),
                    }
                })?;
                let manifest = Manifest::parse_checked(&raw).map_err(|issues| {
                    let detail = issues
                        .iter()
                        .map(|issue| format!("{}: {}", issue.field(), issue.reason()))
                        .collect::<Vec<_>>()
                        .join("; ");
                    ConfigError::Internal {
                        reason: format!("{} is invalid: {detail}", manifest_path.display()),
                    }
                })?;

                let source = format!("{dir_name}/{name}/manifest.json");
                let Some(config) = &manifest.config else {
                    continue;
                };
                for (key, spec) in config {
                    // 键名必须是合法的 ConfigKey（manifest 里写错了要在**启动时**暴露，
                    // 而不是等第一次请求）。
                    ConfigKey::parse(key)
                        .map_err(|_| ConfigError::KeyInvalid { key: key.clone() })?;
                    let registered = RegisteredKey {
                        key: key.clone(),
                        kind: spec.kind,
                        default: spec.default.clone(),
                        scope: spec.scope,
                        derived: false,
                        source: source.clone(),
                    };
                    if keys.insert(key.clone(), registered).is_some() {
                        return Err(ConfigError::Internal {
                            reason: format!(
                                "the config key `{key}` is declared in more than one manifest (last: {source})"
                            ),
                        });
                    }
                }
            }
        }

        // 登记为 boolean 的键 = flag（见模块文档）。
        let mut flags: BTreeSet<String> = keys
            .values()
            .filter(|registered| registered.kind == ConfigType::Boolean)
            .map(|registered| registered.key.clone())
            .collect();

        // 派生键：对每个 flag 自动加 `flags.<F>.rolloutPercent`。
        let derived: Vec<RegisteredKey> = flags
            .iter()
            .map(|flag| RegisteredKey {
                key: rollout_key(flag),
                kind: ConfigType::Number,
                default: Value::from(DEFAULT_ROLLOUT_PERCENT),
                scope: ConfigScope::Tenant,
                derived: true,
                source: format!("derived from flag `{flag}`"),
            })
            .collect();
        for registered in derived {
            if keys.contains_key(&registered.key) {
                return Err(ConfigError::Internal {
                    reason: format!(
                        "the derived key `{}` collides with a declared config key",
                        registered.key
                    ),
                });
            }
            keys.insert(registered.key.clone(), registered);
        }

        // 派生键不改变 flag 集合。
        flags.retain(|flag| keys.contains_key(flag));

        Ok(Self { keys, flags })
    }

    /// 查一个已登记的键。
    pub fn get(&self, key: &str) -> Option<&RegisteredKey> {
        self.keys.get(key)
    }

    /// 全部已登记的键（含派生键，按名字排序）。
    pub fn keys(&self) -> impl Iterator<Item = &RegisteredKey> {
        self.keys.values()
    }

    /// 已登记的 flag（按名字排序）。
    pub fn flags(&self) -> impl Iterator<Item = &String> {
        self.flags.iter()
    }

    /// 该名字是否是已登记的 flag。
    pub fn is_flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    /// 该键是否属于 flag 命名空间（D70）。
    ///
    /// = **已登记的 flag 名** 或 `flags.` 前缀的键。
    pub fn is_flag_namespace(&self, key: &str) -> bool {
        self.is_flag(key) || key.starts_with(crate::flag::FLAG_NAMESPACE_PREFIX)
    }

    /// 已登记键的数量。
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::whitelist::{BRANDING_WHITELIST, is_branding_key};

    fn repo_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn registry() -> ConfigRegistry {
        ConfigRegistry::from_manifest_root(&repo_root()).expect("注册表应能从 manifest 建立")
    }

    #[test]
    fn the_registry_merges_all_manifests() {
        let registry = registry();
        assert!(registry.get("logo").is_some());
        assert!(registry.get("pageSize").is_some(), "sandbox 的键也要在");
        assert!(registry.get("scimBaseUrl").is_some(), "org 的键也要在");
        assert!(registry.get("cacheTtlSeconds").is_some(), "rbac 的键也要在");
        assert!(registry.get("flags.beta.rolloutPercent").is_some());
        assert!(registry.get("flags.newInvoice.rolloutPercent").is_some());
        assert!(
            registry.get("flags.nosuch.rolloutPercent").is_none(),
            "派生键只对已登记的 flag 存在"
        );
    }

    /// D74 / R13：品牌键**必须显式**写 `scope: space`（缺省是 `tenant`）。
    #[test]
    fn every_branding_whitelist_key_is_registered_as_space() {
        let registry = registry();
        for key in BRANDING_WHITELIST {
            let registered = registry
                .get(key)
                .unwrap_or_else(|| panic!("白名单键未登记：{key}"));
            assert_eq!(
                registered.scope,
                ConfigScope::Space,
                "白名单键必须登记为 space（否则空间级写会 400）：{key}"
            );
            assert!(
                is_branding_key(key),
                "白名单与 is_branding_key 必须一致：{key}"
            );
        }
    }

    /// 反向不必：登记为 space 但不在白名单的键（D66 的判据夹具）。
    #[test]
    fn a_space_key_can_stay_outside_the_whitelist() {
        let registry = registry();
        let key = registry.get("loginBanner").expect("loginBanner 应已登记");
        assert_eq!(key.scope, ConfigScope::Space);
        assert!(!is_branding_key("loginBanner"));
    }

    #[test]
    fn registered_scopes_are_the_expected_ones() {
        let registry = registry();
        assert_eq!(
            registry.get("conflictStrategy").map(|key| key.scope),
            Some(ConfigScope::Platform)
        );
        assert_eq!(
            registry.get("pageSize").map(|key| key.scope),
            Some(ConfigScope::Tenant)
        );
        assert_eq!(
            registry.get("logo").map(|key| key.scope),
            Some(ConfigScope::Space)
        );
    }

    #[test]
    fn boolean_keys_become_flags_and_their_derived_key_is_a_tenant_number() {
        let registry = registry();
        assert!(registry.is_flag("beta"));
        assert!(registry.is_flag("newInvoice"));
        assert!(!registry.is_flag("logo"));
        let derived = registry
            .get("flags.beta.rolloutPercent")
            .expect("派生键应存在");
        assert_eq!(derived.kind, ConfigType::Number);
        assert_eq!(derived.scope, ConfigScope::Tenant);
        assert_eq!(derived.default, Value::from(100));
        assert!(derived.derived);
    }

    #[test]
    fn the_flag_namespace_covers_exact_flag_names_and_the_prefix() {
        let registry = registry();
        assert!(registry.is_flag_namespace("beta"));
        assert!(registry.is_flag_namespace("flags.beta.rolloutPercent"));
        assert!(registry.is_flag_namespace("flags.anything.at.all"));
        assert!(!registry.is_flag_namespace("logo"));
        assert!(!registry.is_flag_namespace("pageSize"));
    }

    #[test]
    fn key_names_are_validated_at_registry_build_time() {
        // 直接构造一个不符合 ConfigKey 的 manifest 不在这里做（会污染仓库）；
        // 这里只验证 `ConfigKey::parse` 与注册表的判定一致。
        assert!(ConfigKey::parse("theme.radius").is_ok());
        assert!(ConfigKey::parse("Theme.radius").is_err());
    }
}
