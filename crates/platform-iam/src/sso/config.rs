//! SSO 配置：读取 + `validate()` + **读取失败 fail-closed**（REQ-013）。
//!
//! # 12 个键（`scope: tenant`）
//!
//! | 键 | 类型 | 默认 |
//! |----|------|------|
//! | `sso.enforce` | boolean | `false` |
//! | `sso.jitProvisioning` | boolean | `false` |
//! | `sso.oidc.enabled` | boolean | `false` |
//! | `sso.oidc.issuer` | string | `""` |
//! | `sso.oidc.clientId` | string | `""` |
//! | `sso.oidc.scopes` | string | `"openid email profile"` |
//! | `sso.oidc.redirectUri` | string | `""` |
//! | `sso.ldap.enabled` | boolean | `false` |
//! | `sso.ldap.url` | string | `""` |
//! | `sso.ldap.baseDn` | string | `""` |
//! | `sso.ldap.userSearchFilter` | string | `"(uid={username})"` |
//! | `sso.ldap.mailAttribute` | string | `"mail"` |
//!
//! # `client_secret` **不在**这里
//!
//! 它只从环境变量 `BASE_SSO_OIDC_CLIENT_SECRET` 取（AC-S-106）。
//! 本模块的类型里**没有**这个字段 —— 让「把 secret 放进配置 / 响应 / 日志」
//! 在类型上不可表达（`SsoConfig` 的 `Debug` 因此也不会带出它）。
//!
//! # 读取失败 **绝不**当 `false`
//!
//! `sso.enforce` 读不出来时若当成 `false`，强制 SSO 就**静默失效**。
//! 所以读取失败 → [`SsoConfigOutcome::Unreadable`] → 调用方 **403 `SSO_REQUIRED`**（fail closed）+ warn 日志。
//!
//! 注意与「**没有注入读取源**」（[`SsoConfigOutcome::NotInstalled`]）区分：
//! 那是「本部署没装配 SSO」，不是「读了但失败」。前者按未启用处理（否则所有
//! 未装配 SSO 的部署与既有测试都会登不进去），后者必须 fail closed。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use platform_core::RequestContext;

use super::SsoProviderView;

/// 配置读取失败（**不含任何配置值**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SsoConfigError;

/// 装箱的 future（让 trait 保持**可对象化** —— `async fn` 在 trait 上不可 `dyn`）。
pub type SsoBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// SSO 配置的**读取源**（由装配层实现；`platform-iam` 不依赖 `platform-config`）。
///
/// 实现方式：在 `platform-server` 里用 `ConfigService::read_entries` 读那 12 个键，
/// 映射成 [`SsoConfig`]。读取失败必须返回 `Err`，**不得**降级成默认值。
pub trait SsoConfigSource: Send + Sync + 'static {
    /// 读取当前租户的有效 SSO 配置。
    fn read(&self, ctx: &RequestContext) -> SsoBoxFuture<'_, Result<SsoConfig, SsoConfigError>>;
}

/// 12 个配置键（**逐字**，AC-S-106 要求 `entries` 长度 == 12）。
pub const SSO_CONFIG_KEYS: [&str; 12] = [
    "sso.enforce",
    "sso.jitProvisioning",
    "sso.oidc.enabled",
    "sso.oidc.issuer",
    "sso.oidc.clientId",
    "sso.oidc.scopes",
    "sso.oidc.redirectUri",
    "sso.ldap.enabled",
    "sso.ldap.url",
    "sso.ldap.baseDn",
    "sso.ldap.userSearchFilter",
    "sso.ldap.mailAttribute",
];

/// `client_secret` 的环境变量名（**唯一来源**）。
pub const OIDC_CLIENT_SECRET_ENV: &str = "BASE_SSO_OIDC_CLIENT_SECRET";

/// 服务账号 bind 的 DN 环境变量（可选；不配则匿名 bind）。
pub const LDAP_SERVICE_BIND_DN_ENV: &str = "BASE_SSO_LDAP_BIND_DN";

/// 服务账号 bind 的口令环境变量（可选）。
pub const LDAP_SERVICE_BIND_PASSWORD_ENV: &str = "BASE_SSO_LDAP_BIND_PASSWORD";

/// OIDC 配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcSettings {
    /// 是否启用。
    pub enabled: bool,
    /// IdP issuer（用于发现文档）。
    pub issuer: String,
    /// `client_id`（非敏感）。
    pub client_id: String,
    /// 空格分隔的 scope。
    pub scopes: String,
    /// 回调地址（**绝对 URL**，须与 IdP 登记逐字一致）。
    pub redirect_uri: String,
}

impl Default for OidcSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            issuer: String::new(),
            client_id: String::new(),
            scopes: "openid email profile".to_string(),
            redirect_uri: String::new(),
        }
    }
}

/// LDAP 配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LdapSettings {
    /// 是否启用。
    pub enabled: bool,
    /// `ldap://host:389`（本轮**不支持** `ldaps://`）。
    pub url: String,
    /// search 起点。
    pub base_dn: String,
    /// 用户检索过滤器（**必须**含 `{username}`）。
    pub user_search_filter: String,
    /// 映射到本地账号的属性名。
    pub mail_attribute: String,
}

impl Default for LdapSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            base_dn: String::new(),
            user_search_filter: "(uid={username})".to_string(),
            mail_attribute: "mail".to_string(),
        }
    }
}

/// SSO 配置（**不含 `client_secret`**）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SsoConfig {
    /// 强制 SSO（禁用本地密码登录）。
    pub enforce: bool,
    /// 未绑定时是否自动建号。
    pub jit_provisioning: bool,
    /// OIDC 设置。
    pub oidc: OidcSettings,
    /// LDAP 设置。
    pub ldap: LdapSettings,
}

/// 一次配置读取的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsoConfigOutcome {
    /// **没有注入读取源** —— 本部署没装配 SSO（按未启用处理）。
    NotInstalled,
    /// 读到了。
    Loaded(SsoConfig),
    /// 注入过读取源，但**读取失败** → 强制 SSO **fail closed**。
    Unreadable,
}

impl SsoConfigOutcome {
    /// 强制 SSO 是否生效。
    ///
    /// - `NotInstalled` → `false`（没装配 SSO）
    /// - `Loaded(cfg)` → `cfg.enforce`
    /// - `Unreadable` → **`true`**（fail closed，见模块文档）
    pub fn enforcement(&self) -> bool {
        match self {
            Self::NotInstalled => false,
            Self::Loaded(config) => config.enforce,
            Self::Unreadable => true,
        }
    }
}

/// 配置校验失败的原因（**固定的 ASCII 短语**，不含配置值）。
pub type SsoValidationError = &'static str;

impl SsoConfig {
    /// 校验当前租户的配置。
    ///
    /// 只校验**已启用**的协议：未启用时 `issuer` / `url` 是空串是正常状态，
    /// 不该因此报错（否则「默认关闭」的租户连登录页都进不去）。
    pub fn validate(&self) -> Result<(), SsoValidationError> {
        if self.oidc.enabled {
            if !is_absolute_http_url(&self.oidc.issuer) {
                return Err("the oidc issuer must be an absolute http or https url");
            }
            if self.oidc.client_id.trim().is_empty() {
                return Err("the oidc client id is empty");
            }
            if !is_absolute_http_url(&self.oidc.redirect_uri) {
                return Err("the oidc redirect uri must be an absolute http or https url");
            }
            if !self
                .oidc
                .scopes
                .split_whitespace()
                .any(|scope| scope == "openid")
            {
                return Err("the oidc scopes must include openid");
            }
        }
        if self.ldap.enabled {
            if !is_supported_ldap_url(&self.ldap.url) {
                return Err("the ldap url must use the ldap scheme");
            }
            if self.ldap.base_dn.trim().is_empty() {
                return Err("the ldap base dn is empty");
            }
            if !self.ldap.user_search_filter.contains("{username}") {
                return Err("the ldap user search filter must contain the {username} placeholder");
            }
            if self.ldap.mail_attribute.trim().is_empty() {
                return Err("the ldap mail attribute is empty");
            }
        }
        Ok(())
    }

    /// 当前租户**可用**的 provider 列表（登录页用）。
    ///
    /// 「可用」= 启用 **且** 配置校验通过 —— 配错的 provider 不显示
    /// （点进去必然 400，不如不显示）。
    pub fn providers(&self) -> Vec<SsoProviderView> {
        if self.validate().is_err() {
            return Vec::new();
        }
        let mut providers = Vec::new();
        if self.oidc.enabled {
            providers.push(SsoProviderView::oidc());
        }
        if self.ldap.enabled {
            providers.push(SsoProviderView::ldap());
        }
        providers
    }

    /// 该协议是否可用（启用 + 校验通过）。
    pub fn oidc_available(&self) -> bool {
        self.oidc.enabled && self.validate().is_ok()
    }

    /// LDAP 是否可用（启用 + 校验通过）。
    pub fn ldap_available(&self) -> bool {
        self.ldap.enabled && self.validate().is_ok()
    }
}

/// 是否**绝对** http(s) URL。
///
/// 相对地址一律拒绝：回调 `/callback` 若配成相对路径，IdP 侧登记与我们的
/// `redirect_uri` 必然对不上（而且 OIDC 规范要求绝对 URL）。
fn is_absolute_http_url(raw: &str) -> bool {
    let value = raw.trim();
    let rest = match value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
    {
        Some(rest) => rest,
        None => return false,
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    !host.is_empty()
}

/// `ldap://` 且带 host；`ldaps://` 与其它 scheme 一律拒绝（本轮不支持 TLS 的 LDAP）。
fn is_supported_ldap_url(raw: &str) -> bool {
    let value = raw.trim();
    let Some(rest) = value.strip_prefix("ldap://") else {
        return false;
    };
    let host_port = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    !host_port.is_empty()
}

/// 内存里的配置源（测试与「从别的来源已读好」的装配场景）。
///
/// `set` 允许在运行期改（AC-S-021 步骤 7 要把 `sso.enforce` 改回 `false`）。
#[derive(Debug, Clone, Default)]
pub struct StaticSsoConfigSource {
    inner: Arc<RwLock<SsoConfig>>,
}

impl StaticSsoConfigSource {
    /// 用一份配置构造。
    pub fn new(config: SsoConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(config)),
        }
    }

    /// 替换配置。
    pub fn set(&self, config: SsoConfig) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = config;
        }
    }

    /// 读一份快照。
    pub fn snapshot(&self) -> SsoConfig {
        self.inner
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

impl SsoConfigSource for StaticSsoConfigSource {
    fn read(&self, _ctx: &RequestContext) -> SsoBoxFuture<'_, Result<SsoConfig, SsoConfigError>> {
        Box::pin(async move { Ok(self.snapshot()) })
    }
}

/// **永远读失败**的配置源（AC-S-021 步骤 8：读取失败必须 fail closed）。
#[derive(Debug, Clone, Copy, Default)]
pub struct FailingSsoConfigSource;

impl SsoConfigSource for FailingSsoConfigSource {
    fn read(&self, _ctx: &RequestContext) -> SsoBoxFuture<'_, Result<SsoConfig, SsoConfigError>> {
        Box::pin(async move { Err(SsoConfigError) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oidc_config() -> SsoConfig {
        SsoConfig {
            oidc: OidcSettings {
                enabled: true,
                issuer: "https://idp.example.com".to_string(),
                client_id: "client-1".to_string(),
                scopes: "openid email".to_string(),
                redirect_uri: "https://app.example.com/api/iam/sso/oidc/callback".to_string(),
            },
            ..SsoConfig::default()
        }
    }

    #[test]
    fn twelve_keys_are_exactly_the_contract_set() {
        assert_eq!(SSO_CONFIG_KEYS.len(), 12);
        let unique: std::collections::BTreeSet<&str> = SSO_CONFIG_KEYS.iter().copied().collect();
        assert_eq!(unique.len(), 12, "配置键不得重复");
        for key in SSO_CONFIG_KEYS {
            assert!(
                platform_core::config::ConfigKey::parse(key).is_ok(),
                "配置键必须是平台接受的形态：{key}"
            );
        }
    }

    #[test]
    fn a_disabled_protocol_is_not_validated() {
        // 默认全是空串 —— 未启用时不该报错。
        assert_eq!(SsoConfig::default().validate(), Ok(()));
    }

    #[test]
    fn relative_redirect_uri_is_rejected() {
        let mut config = oidc_config();
        config.oidc.redirect_uri = "/callback".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn ldaps_is_rejected() {
        let config = SsoConfig {
            ldap: LdapSettings {
                enabled: true,
                url: "ldaps://ldap.example.com:636".to_string(),
                base_dn: "dc=example,dc=com".to_string(),
                ..LdapSettings::default()
            },
            ..SsoConfig::default()
        };
        assert_eq!(
            config.validate(),
            Err("the ldap url must use the ldap scheme")
        );
    }

    #[test]
    fn filter_without_placeholder_is_rejected() {
        let config = SsoConfig {
            ldap: LdapSettings {
                enabled: true,
                url: "ldap://ldap.example.com:389".to_string(),
                base_dn: "dc=example,dc=com".to_string(),
                user_search_filter: "(uid=someone)".to_string(),
                ..LdapSettings::default()
            },
            ..SsoConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn providers_only_include_enabled_and_valid_ones() {
        assert!(SsoConfig::default().providers().is_empty());
        let kinds: Vec<String> = oidc_config()
            .providers()
            .into_iter()
            .map(|view| view.kind)
            .collect();
        assert_eq!(kinds, vec!["oidc".to_string()]);

        let mut broken = oidc_config();
        broken.oidc.issuer = "idp.example.com".to_string();
        assert!(
            broken.providers().is_empty(),
            "配置非法的 provider 不得出现在登录页"
        );
    }

    /// 读取失败 **必须** fail closed。
    #[test]
    fn an_unreadable_config_enforces_sso() {
        assert!(SsoConfigOutcome::Unreadable.enforcement());
        assert!(!SsoConfigOutcome::NotInstalled.enforcement());
        assert!(!SsoConfigOutcome::Loaded(SsoConfig::default()).enforcement());
    }
}
