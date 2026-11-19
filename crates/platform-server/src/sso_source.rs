//! SSO 配置源：用平台配置服务读那 12 个键（REQ-013 / D129）。
//!
//! # 为什么在 lib 里而不在 `main.rs` 里
//!
//! 这个类型是**生产装配层的一环**：`main.rs` 把它注入 `IamService`
//! （`set_sso_config_source`）。它一旦只在 bin crate 里，集成测试就**够不到**
//! —— 测试只能自己抄一份「等价实现」，于是生产副本丢了租户上下文（`ctx`）
//! 测试也不会红（D129 的 Q6 同型问题）。
//!
//! 放在 lib 里之后，`tests/sso_real_db.rs` 用的是**同一个类型**，不是拷贝。
//!
//! # 它守的两件事
//!
//! 1. `GET /api/config/entries` **同一套解析**（三层覆盖 + manifest 默认值），
//!    「按精确键名读」—— **没有**「全量导出配置」这种能力（P1 / S1）；
//! 2. **租户上下文必须原样透传**：`read(&self, ctx)` 的 `ctx` 决定读到哪个租户的
//!    覆盖值。丢了它，强制 SSO 会退化成「读平台默认值」，**所有租户一起失效**。

use std::sync::Arc;

use platform_core::{RequestContext, ResolvedConfig};

/// SSO 配置源：用平台配置服务读那 12 个键（REQ-013）。
pub struct ConfigServiceSsoSource {
    config: Arc<platform_config::ConfigService>,
}

impl ConfigServiceSsoSource {
    /// 用配置服务构造。
    pub fn new(config: Arc<platform_config::ConfigService>) -> Self {
        Self { config }
    }
}

impl platform_iam::sso::config::SsoConfigSource for ConfigServiceSsoSource {
    fn read(
        &self,
        ctx: &RequestContext,
    ) -> platform_iam::sso::config::SsoBoxFuture<
        '_,
        Result<platform_iam::sso::config::SsoConfig, platform_iam::sso::config::SsoConfigError>,
    > {
        let config = self.config.clone();
        // 租户上下文必须跟着走：这一行丢了 = 所有租户的 enforce 一起失效。
        let ctx = ctx.clone();
        Box::pin(async move {
            let keys: Vec<String> = platform_iam::sso::config::SSO_CONFIG_KEYS
                .iter()
                .map(|key| (*key).to_string())
                .collect();
            let entries = config
                .read_entries(&ctx, &keys)
                .await
                .map_err(|_| platform_iam::sso::config::SsoConfigError)?;
            Ok(sso_config_from_entries(&entries))
        })
    }
}

/// 把解析结果映射成 [`platform_iam::sso::config::SsoConfig`]。
///
/// 缺失的键用**代码里的默认值**（与 `manifest.json` 登记的默认值一致）——
/// 但注意：迁移 / 注册表都在时它们必然齐全，缺失只可能是注册表被改坏了。
pub fn sso_config_from_entries(entries: &[ResolvedConfig]) -> platform_iam::sso::config::SsoConfig {
    use platform_iam::sso::config::{LdapSettings, OidcSettings, SsoConfig};

    fn text(entries: &[ResolvedConfig], key: &str) -> Option<String> {
        entries
            .iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.value.as_str())
            .map(str::to_string)
    }
    fn flag(entries: &[ResolvedConfig], key: &str) -> Option<bool> {
        entries
            .iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.value.as_bool())
    }

    let defaults = SsoConfig::default();
    SsoConfig {
        enforce: flag(entries, "sso.enforce").unwrap_or(defaults.enforce),
        jit_provisioning: flag(entries, "sso.jitProvisioning").unwrap_or(defaults.jit_provisioning),
        oidc: OidcSettings {
            enabled: flag(entries, "sso.oidc.enabled").unwrap_or(defaults.oidc.enabled),
            issuer: text(entries, "sso.oidc.issuer").unwrap_or(defaults.oidc.issuer),
            client_id: text(entries, "sso.oidc.clientId").unwrap_or(defaults.oidc.client_id),
            scopes: text(entries, "sso.oidc.scopes").unwrap_or(defaults.oidc.scopes),
            redirect_uri: text(entries, "sso.oidc.redirectUri")
                .unwrap_or(defaults.oidc.redirect_uri),
        },
        ldap: LdapSettings {
            enabled: flag(entries, "sso.ldap.enabled").unwrap_or(defaults.ldap.enabled),
            url: text(entries, "sso.ldap.url").unwrap_or(defaults.ldap.url),
            base_dn: text(entries, "sso.ldap.baseDn").unwrap_or(defaults.ldap.base_dn),
            user_search_filter: text(entries, "sso.ldap.userSearchFilter")
                .unwrap_or(defaults.ldap.user_search_filter),
            mail_attribute: text(entries, "sso.ldap.mailAttribute")
                .unwrap_or(defaults.ldap.mail_attribute),
        },
    }
}
