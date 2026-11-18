//! 组织架构的**环境配置**。
//!
//! 配置只从环境注入（**不硬编码**）：密钥、口令、地址一律不进仓库。
//!
//! | 环境变量 | 含义 |
//! |----------|------|
//! | `BASE_ORG_SCIM_PROVIDER` | SCIM provider 标识（默认 `default`） |
//! | `BASE_ORG_SCIM_BASE_URL` | SCIM 出站目标地址（**本期只支持 `http://`**） |
//! | `BASE_ORG_SCIM_AUTH_HEADER` | 认证头（整行，如 `Authorization: Bearer xxx`） |
//! | `BASE_ORG_SCIM_MAPPING` | 字段映射（JSON：`{"userFields":{...},"groupFields":{...}}`） |
//! | `BASE_ORG_LDAP_SOURCE` | LDAP 源的映射配置（JSON，见 `LdapSourceConfig`） |
//! | `BASE_ORG_CONFLICT_STRATEGY` | 冲突策略（`manual` / `local_wins` / `remote_wins`） |
//!
//! **一个都没配 → 不提供同步能力**（服务照常起），而不是用空配置假装能同步。

use crate::ldap::LdapSourceConfig;
use crate::scim::{ScimGroupFields, ScimProviderConfig, ScimUserFields};
use crate::sync::ConflictStrategy;

/// SCIM provider 标识的环境变量。
pub const SCIM_PROVIDER_ENV: &str = "BASE_ORG_SCIM_PROVIDER";
/// SCIM 目标地址的环境变量。
pub const SCIM_BASE_URL_ENV: &str = "BASE_ORG_SCIM_BASE_URL";
/// SCIM 认证头的环境变量。
pub const SCIM_AUTH_HEADER_ENV: &str = "BASE_ORG_SCIM_AUTH_HEADER";
/// SCIM 字段映射的环境变量。
pub const SCIM_MAPPING_ENV: &str = "BASE_ORG_SCIM_MAPPING";
/// LDAP 源配置的环境变量。
pub const LDAP_SOURCE_ENV: &str = "BASE_ORG_LDAP_SOURCE";
/// 冲突策略的环境变量。
pub const CONFLICT_STRATEGY_ENV: &str = "BASE_ORG_CONFLICT_STRATEGY";

/// 从环境变量读 SCIM provider 配置。
///
/// 没配 `BASE_ORG_SCIM_BASE_URL` → 返回 `None`（**不提供** SCIM 出站能力）。
pub fn scim_provider_from_env() -> Option<ScimProviderConfig> {
    let base_url = std::env::var(SCIM_BASE_URL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let provider = std::env::var(SCIM_PROVIDER_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "default".to_string());

    let mut config = ScimProviderConfig::new(provider, base_url);
    config.auth_header = std::env::var(SCIM_AUTH_HEADER_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if let Ok(raw) = std::env::var(SCIM_MAPPING_ENV) {
        if let Ok(mapping) = serde_json::from_str::<ScimMapping>(&raw) {
            if let Some(fields) = mapping.user_fields {
                config.user_fields = fields;
            }
            if let Some(fields) = mapping.group_fields {
                config.group_fields = fields;
            }
        } else {
            tracing::warn!(
                env = SCIM_MAPPING_ENV,
                "字段映射配置不是合法 JSON，已忽略（用默认映射）"
            );
        }
    }
    Some(config)
}

/// 从环境变量读 LDAP 源配置。
pub fn ldap_source_from_env() -> Option<LdapSourceConfig> {
    let raw = std::env::var(LDAP_SOURCE_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    match LdapSourceConfig::from_json(&raw) {
        Ok(config) => Some(config),
        Err(error) => {
            tracing::warn!(env = LDAP_SOURCE_ENV, error = %error, "LDAP 源配置不合法，已忽略");
            None
        }
    }
}

/// 从环境变量读冲突策略（默认 `manual`）。
pub fn conflict_strategy_from_env() -> ConflictStrategy {
    match std::env::var(CONFLICT_STRATEGY_ENV) {
        Ok(raw) => ConflictStrategy::parse(&raw).unwrap_or_else(|| {
            tracing::warn!(
                env = CONFLICT_STRATEGY_ENV,
                value = %raw,
                "冲突策略不合法，回退到 manual（绝不静默覆盖）"
            );
            ConflictStrategy::Manual
        }),
        Err(_) => ConflictStrategy::Manual,
    }
}

/// SCIM 字段映射（环境变量里的 JSON）。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScimMapping {
    #[serde(default)]
    user_fields: Option<ScimUserFields>,
    #[serde(default)]
    group_fields: Option<ScimGroupFields>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_environment_means_no_scim_provider() {
        // 环境变量没设时不能凭空造一个 provider。
        let provider = scim_provider_from_env();
        if std::env::var(SCIM_BASE_URL_ENV).is_err() {
            assert!(provider.is_none());
        }
    }

    #[test]
    fn the_conflict_strategy_defaults_to_manual() {
        // 不设环境变量时必须是 manual —— 绝不静默覆盖。
        if std::env::var(CONFLICT_STRATEGY_ENV).is_err() {
            assert_eq!(conflict_strategy_from_env(), ConflictStrategy::Manual);
        }
    }

    #[test]
    fn an_invalid_strategy_falls_back_to_manual() {
        // 用解析函数验证（环境变量是进程级的，测试里不污染它）。
        assert_eq!(ConflictStrategy::parse("nope"), None);
        assert_eq!(ConflictStrategy::default(), ConflictStrategy::Manual);
    }
}
