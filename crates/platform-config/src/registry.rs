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
use platform_core::extension::manifest::{ConfigFormat, ConfigType, Manifest};
use serde_json::Value;

use crate::error::ConfigError;
use crate::flag::{DEFAULT_ROLLOUT_PERCENT, rollout_key};

/// 已注册语言的后备清单（`packages/i18n/src/locales/` 不存在时用）。
///
/// 基准语言是 `zh-CN`（design §6.5）；翻译包目录还不存在时只承认它 ——
/// 这样「目录没建好」不会让服务起不来，也不会让协商返回一个加载不到翻译包的 locale。
pub const FALLBACK_LOCALES: &[&str] = &["zh-CN"];

/// 前端翻译包目录（相对仓库根）。**文件名 = 已注册语言清单**（裁定 #16）。
///
/// ⚠️ **耦合声明**：这个路径指向 `packages/i18n/src/locales/*.json`，
/// 由前端（REQ-021 的前端 Dev）创建；后端**只读**，不写、不生成。
/// 目录不存在 → 回退到 [`FALLBACK_LOCALES`] 并 `tracing::warn`
/// （回退而不是失败：全新检出时前端产物可能还没生成）。
pub const LOCALES_DIR: &str = "packages/i18n/src/locales";

/// 扫描翻译包目录，得到**已注册语言**集合。
///
/// `root` 是仓库根（与其它 manifest 扫描同一口径）。文件名必须是
/// `zh-CN.json` 这样的语言标签；其它文件（`index.ts` / `README.md`）忽略。
pub fn registered_locales(root: &Path) -> BTreeSet<String> {
    let dir = root.join(LOCALES_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        tracing::warn!(
            dir = %dir.display(),
            "未找到翻译包目录：已注册语言回退为 zh-CN（前端产物缺失时协商只会返回基准语言）"
        );
        return FALLBACK_LOCALES.iter().map(|v| (*v).to_string()).collect();
    };

    let mut locales = BTreeSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if is_locale_tag(stem) {
            locales.insert(stem.to_string());
        } else {
            tracing::warn!(file = %path.display(), "翻译包文件名不是语言标签，忽略");
        }
    }
    if locales.is_empty() {
        tracing::warn!(
            dir = %dir.display(),
            "翻译包目录里没有语言标签文件：已注册语言回退为 zh-CN"
        );
        return FALLBACK_LOCALES.iter().map(|v| (*v).to_string()).collect();
    }
    locales
}

/// 语言标签的形状：`zh` / `zh-CN` / `zh-Hant` / `zh-Hant-TW`（BCP 47 的常用子集）。
///
/// - 第一段：2~3 个 ASCII 字母（主语言）；
/// - 后续段：2~8 个 ASCII 字母或数字（地区 / 书写系统 / 变体）。
///
/// **只判形状**：`xx-YY` 形状合法但通常未注册 —— 是否放行由
/// 「已注册语言集合」决定（`is_registered_locale`）。
pub fn is_locale_tag(value: &str) -> bool {
    let mut segments = value.split('-');
    let Some(primary) = segments.next() else {
        return false;
    };
    if !(primary.len() >= 2
        && primary.len() <= 3
        && primary.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        return false;
    }
    segments.all(|segment| {
        (2..=8).contains(&segment.len()) && segment.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

/// 该值是否是**已注册**语言（形状合法且在翻译包清单里）。
pub fn is_registered_locale(value: &str, registered: &BTreeSet<String>) -> bool {
    is_locale_tag(value) && registered.contains(value)
}

/// IANA 时区的**已知区域**（`Area/Location` 的第一段）。
///
/// 取自 IANA tzdata 的顶层目录（2024 版）。
const IANA_AREAS: &[&str] = &[
    "Africa",
    "America",
    "Antarctica",
    "Arctic",
    "Asia",
    "Atlantic",
    "Australia",
    "Brazil",
    "Canada",
    "Chile",
    "Etc",
    "Europe",
    "Indian",
    "Mexico",
    "Pacific",
    "US",
];

/// 是否是可接受的 IANA 时区名。
///
/// # 后端没有 tzdb（D114 / 裁定 #9）
///
/// `time` crate 只启用 `formatting`，加 tzdb（`time-tz` / `chrono-tz`）会违反
/// 「零新依赖」。所以这里做**结构 + 区域白名单**校验，判据写死为：
///
/// 1. `UTC` 单独放行（`Etc/UTC` 也放行 —— 它走正常规则）；
/// 2. 必须含 `/`（`Asia/Shanghai`），可含多段（`America/Argentina/Buenos_Aires`）；
/// 3. 第一段必须在 [`IANA_AREAS`] 里（**大小写敏感**：`asia/shanghai` 不是合法名）；
/// 4. 每一段的字符集是 ASCII 字母 / 数字 / `_` / `-` / `+`（`Etc/GMT+8`），
///    且不得为空、不得以 `/` 开头或结尾。
///
/// # 能拒什么 / 拒不了什么（**局限，如实记录**）
///
/// 能拒：`Not/AZone`（区域不在白名单）、`Asia`（没有 `/`）、`Asia/`（空段）、
/// `asia/shanghai`（区域大小写错）、`Mars/Olympus`、`Europe/Berlin/Extra` 之外的
/// 空段与非法字符、`+08:00` 这类**偏移量**（我们要的是 IANA 名，不是偏移）。
///
/// **拒不了**：区域正确但城市拼错的（`Asia/Shanghia`）、
/// 已废弃但历史上真实存在的别名（`Asia/Calcutta` 若不在当前 tzdata 里）。
/// 要精确判定必须内嵌 tzdb —— 本轮明确不做（D114），所以**错拼的城市名会被放行**。
pub fn is_iana_timezone(value: &str) -> bool {
    if value == "UTC" {
        return true;
    }
    let segments: Vec<&str> = value.split('/').collect();
    if segments.len() < 2 {
        return false;
    }
    if !IANA_AREAS.contains(&segments[0]) {
        return false;
    }
    segments.iter().all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'+')
    })
}

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
    /// 值的格式约束（D111）；`None` = 不校验格式。
    pub format: Option<ConfigFormat>,
    /// 来源 manifest（`crates/platform-config/manifest.json`）。
    pub source: String,
}

impl RegisteredKey {
    /// 登记类型名（错误参数 `expected` 用）。
    pub fn type_name(&self) -> &'static str {
        config_type_name(self.kind)
    }

    /// 登记格式名（错误参数 `format` 用）；没登记格式时返回 `None`。
    pub fn format_name(&self) -> Option<&'static str> {
        self.format.map(ConfigFormat::as_str)
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
    /// 已注册语言（`packages/i18n/src/locales/*.json` 的文件名）。
    locales: BTreeSet<String>,
}

impl ConfigRegistry {
    /// 从 manifest 根目录收集全部已登记的键。
    ///
    /// 扫 `modules/*/manifest.json` 与 `crates/*/manifest.json`。
    pub fn from_manifest_root(root: &Path) -> Result<Self, ConfigError> {
        let mut keys: BTreeMap<String, RegisteredKey> = BTreeMap::new();
        // 「已注册语言」的唯一来源（裁定 #16）：翻译包目录的文件名。
        let locales = registered_locales(root);

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
                        format: spec.format,
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
        //
        // ⚠️ **必须同时是合法的 flag 名**（`[a-z][a-z0-9]{0,63}`，见 `flag::is_valid_flag_name`）。
        //
        // 为什么：带点号的布尔键（如 REQ-013 的 `sso.enforce`）不是「功能开关」——
        // 它是**普通租户配置**。若把它们也当 flag，会同时坏两件事：
        //
        // 1. 通用写路由把它们拦成 `FlagEndpointRequired`（写不进去）；
        // 2. flag 写路由又因为名字里有点号被判 `FlagNameInvalid`（还是写不进去）
        //    → **两条路都走不通**，配置永远停在 manifest 默认值；
        // 3. 还会给它们派生出无意义的 `flags.sso.enforce.rolloutPercent`。
        //
        // 「boolean ⇒ flag」这条推导规则本来就是启发式（规格没给 flag 单独声明段，
        // 见模块文档第 22~28 行）。加上名字约束后它才自洽：
        // **dimension-free 的布尔键是开关，点号键只是布尔类型的配置。**
        let mut flags: BTreeSet<String> = keys
            .values()
            .filter(|registered| {
                registered.kind == ConfigType::Boolean
                    && crate::flag::is_valid_flag_name(&registered.key)
            })
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
                format: None,
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

        // 格式校验器自检：manifest 的**默认值**必须是合法值。
        // 默认值是「谁都没配」时的取值 —— 它错了会让每个未配置的租户都拿到
        // 一个渲染层加载不到的值，而写入路径永远拦不到它（没人写过）。
        for registered in keys.values() {
            let Some(format) = registered.format else {
                continue;
            };
            let Some(default) = registered.default.as_str() else {
                continue;
            };
            if let Err(reason) = check_format_value(format, default, &locales) {
                return Err(ConfigError::Internal {
                    reason: format!(
                        "the manifest default of `{}` is not a valid `{}` value: {reason}",
                        registered.key,
                        format.as_str()
                    ),
                });
            }
        }

        Ok(Self {
            keys,
            flags,
            locales,
        })
    }

    /// 已注册语言（与 `packages/i18n/src/locales/*.json` 同一份）。
    pub fn locales(&self) -> &BTreeSet<String> {
        &self.locales
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

/// 按格式校验一个字符串值；通过返回 `Ok(())`，不通过返回英文原因。
///
/// 这是**唯一**的格式校验入口：写入路径（[`crate::service`]）与启动自检
/// （manifest 默认值）都调它 —— 测试与生产走同一段代码（D85）。
pub fn check_format_value(
    format: ConfigFormat,
    value: &str,
    registered_locales: &BTreeSet<String>,
) -> Result<(), String> {
    match format {
        ConfigFormat::Locale => {
            if !is_locale_tag(value) {
                return Err(
                    "the value is not a BCP 47 language tag (expected e.g. `zh-CN`)".to_string(),
                );
            }
            if !is_registered_locale(value, registered_locales) {
                let mut known: Vec<&str> = registered_locales.iter().map(String::as_str).collect();
                known.sort_unstable();
                return Err(format!(
                    "the locale is not registered (no translation package); registered: {}",
                    known.join(", ")
                ));
            }
            Ok(())
        }
        ConfigFormat::IanaTimezone => {
            if is_iana_timezone(value) {
                Ok(())
            } else {
                Err(
                    "the value is not a known IANA time zone name (expected `Area/Location`, e.g. `Asia/Shanghai`)"
                        .to_string(),
                )
            }
        }
        ConfigFormat::Iso4217 => {
            if platform_core::money::is_iso4217(value) {
                Ok(())
            } else {
                Err(
                    "the value is not a current ISO 4217 currency code (expected e.g. `CNY`)"
                        .to_string(),
                )
            }
        }
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

    /// **P17（D128 / Q7）**：带点号的布尔键**不是** flag。
    ///
    /// 推导规则加了 `&& is_valid_flag_name(...)`（`boolean ⇒ flag` 的启发式约束）——
    /// 不带这个约束时 REQ-013 的 4 个 SSO 布尔键会同时坏掉两条写路由：
    /// 通用路由把它们拦成 `FlagEndpointRequired`、flag 路由又因点号判 `FlagNameInvalid`。
    ///
    /// 会红的情形：约束被删回 `registered.kind == ConfigType::Boolean` 一行 ——
    /// 那时 `sso.enforce` 会出现在 `flags` 集合里，下面第一条断言变红。
    #[test]
    fn a_dotted_boolean_key_is_plain_config_and_not_a_flag() {
        let registry = registry();
        for key in [
            "sso.enforce",
            "sso.jitProvisioning",
            "sso.oidc.enabled",
            "sso.ldap.enabled",
        ] {
            assert!(
                registry.get(key).is_some(),
                "{key} 必须已登记（否则下面的断言没意义）"
            );
            assert_eq!(
                registry.get(key).map(|registered| registered.kind),
                Some(ConfigType::Boolean),
                "{key} 登记类型应是 boolean"
            );
            assert!(!registry.is_flag(key), "{key} 带点号，不是 flag");
            // 也不该被派生出无意义的灰度比例键。
            assert!(
                registry
                    .get(&format!("flags.{key}.rolloutPercent"))
                    .is_none(),
                "{key} 不是 flag，不该有派生键"
            );
            // 但它**仍然**是普通租户配置：两条写路由的判据只该命中后者。
            assert!(
                !registry.is_flag_namespace(key),
                "{key} 不得落在 flag 命名空间里（否则通用写路由会拒它）"
            );
        }

        // 既有契约**不变**：dimension-free 的布尔键仍是 flag。
        assert!(registry.is_flag("beta"));
        assert!(registry.is_flag("newInvoice"));
        assert!(
            registry
                .get("flags.beta.rolloutPercent")
                .is_some_and(|registered| registered.kind == ConfigType::Number)
        );
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

    // -- REQ-021：i18n 三键 + 格式校验器 ------------------------------------

    /// 三个 i18n 键都登记为 `tenant` 且带 `format`（AC-S-093 步骤 1/2）。
    #[test]
    fn the_i18n_keys_are_registered_as_tenant_with_a_format() {
        let registry = registry();
        for (key, format) in [
            ("locale", ConfigFormat::Locale),
            ("timezone", ConfigFormat::IanaTimezone),
            ("currency", ConfigFormat::Iso4217),
        ] {
            let registered = registry.get(key).unwrap_or_else(|| panic!("未登记：{key}"));
            assert_eq!(registered.scope, ConfigScope::Tenant, "{key} 必须是 tenant");
            assert_eq!(registered.format, Some(format), "{key} 的 format 不对");
            assert_eq!(registered.kind, ConfigType::String);
            assert_eq!(registered.format_name(), Some(format.as_str()));
        }
        // 默认值来源是 manifest 的 `default`（裁定 #17）。
        assert_eq!(
            registry.get("locale").map(|key| key.default.clone()),
            Some(Value::from("zh-CN"))
        );
        assert_eq!(
            registry.get("timezone").map(|key| key.default.clone()),
            Some(Value::from("Asia/Shanghai"))
        );
        assert_eq!(
            registry.get("currency").map(|key| key.default.clone()),
            Some(Value::from("CNY"))
        );
    }

    /// 语言标签的**形状**规则。
    #[test]
    fn locale_tag_shape_is_strict_but_accepts_region_and_script() {
        for good in ["zh", "zh-CN", "en-US", "zh-Hant", "zh-Hant-TW"] {
            assert!(is_locale_tag(good), "`{good}` 形状合法");
        }
        for bad in [
            "", "z", "zh_CN", "zh-", "-CN", "zh-CN-", "zh-CN-x", "中文", "zh--CN",
        ] {
            assert!(!is_locale_tag(bad), "`{bad}` 形状非法");
        }
    }

    /// 语言必须**已注册**：形状合法但目录里没有 → 拒（裁定 #16）。
    #[test]
    fn locales_must_be_registered_not_just_well_formed() {
        let registered: std::collections::BTreeSet<String> =
            ["zh-CN".to_string()].into_iter().collect();
        assert!(is_registered_locale("zh-CN", &registered));
        assert!(!is_registered_locale("en-US", &registered));
        assert!(!is_registered_locale("ja-JP", &registered));
        assert!(!is_registered_locale("not a tag", &registered));
    }

    /// 翻译包目录不存在时回退到 `zh-CN`（不 panic、不起不来）。
    #[test]
    fn missing_locale_directory_falls_back_to_the_base_locale() {
        let empty = std::env::temp_dir().join("req021-no-such-repo");
        let locales = registered_locales(&empty);
        assert_eq!(locales.len(), 1);
        assert!(locales.contains("zh-CN"));
    }

    /// IANA 时区：能拒的与拒不了的都钉死（D114 的局限）。
    #[test]
    fn iana_timezone_validation_is_structural() {
        for good in [
            "Asia/Shanghai",
            "America/New_York",
            "Europe/London",
            "America/Argentina/Buenos_Aires",
            "Etc/GMT+8",
            "UTC",
        ] {
            assert!(is_iana_timezone(good), "`{good}` 应被放行");
        }
        for bad in [
            "Not/AZone",
            "Asia",
            "Asia/",
            "/Shanghai",
            "asia/shanghai",
            "Mars/Olympus",
            "+08:00",
            "Asia/Shang hai",
            "",
        ] {
            assert!(!is_iana_timezone(bad), "`{bad}` 应被拒");
        }
        // **局限**：没有 tzdb，拼错的城市名会被放行 —— 这是 D114 的已知代价。
        assert!(
            is_iana_timezone("Asia/Shanghia"),
            "无 tzdb 时无法识别拼错的城市名（局限，如实断言）"
        );
    }

    /// 格式校验器按枚举分派，且**同一个函数**同时服务于写入与启动自检。
    #[test]
    fn format_checker_dispatches_by_format() {
        let registered: std::collections::BTreeSet<String> =
            ["zh-CN".to_string()].into_iter().collect();
        assert!(check_format_value(ConfigFormat::Locale, "zh-CN", &registered).is_ok());
        assert!(check_format_value(ConfigFormat::Locale, "en-US", &registered).is_err());
        assert!(
            check_format_value(ConfigFormat::IanaTimezone, "Asia/Shanghai", &registered).is_ok()
        );
        assert!(check_format_value(ConfigFormat::IanaTimezone, "Not/AZone", &registered).is_err());
        assert!(check_format_value(ConfigFormat::Iso4217, "CNY", &registered).is_ok());
        // `XXXX` 必须被拒（`^[A-Z]{3}$` 会放行它 —— 这就是不用正则的理由）。
        assert!(check_format_value(ConfigFormat::Iso4217, "XXXX", &registered).is_err());
        assert!(check_format_value(ConfigFormat::Iso4217, "XXX", &registered).is_err());
    }

    /// 非字符串键不允许配 `format`（manifest 写错要在启动时暴露）。
    #[test]
    fn format_is_only_allowed_on_string_config_keys() {
        let raw = r#"{
          "id": "sandbox",
          "name": "Sandbox",
          "version": "0.1.0",
          "permissions": [],
          "menus": [],
          "dependencies": [],
          "config": {
            "pageSize": { "type": "number", "default": 20, "scope": "tenant", "format": "locale" }
          }
        }"#;
        let issues = Manifest::parse_checked(raw).expect_err("number + format 应被拒");
        assert!(
            issues
                .iter()
                .any(|issue| issue.field() == "config.pageSize.format"),
            "issues = {issues:?}"
        );
    }

    #[test]
    fn key_names_are_validated_at_registry_build_time() {
        // 直接构造一个不符合 ConfigKey 的 manifest 不在这里做（会污染仓库）；
        // 这里只验证 `ConfigKey::parse` 与注册表的判定一致。
        assert!(ConfigKey::parse("theme.radius").is_ok());
        assert!(ConfigKey::parse("Theme.radius").is_err());
    }
}
