//! 出站 IdP 的**运行期配置**（REQ-022，design §7）。
//!
//! # 5 个配置键（**键名 camelCase**，契约 C9）
//!
//! | 键 | 类型 | 默认 | 说明 |
//! |----|------|------|------|
//! | `oauth.frontendBaseUrl` | string | `""` | 授权页 / 设备码页的前端基址；**空 = 用 `{issuer}`**（D222） |
//! | `oauth.accessTokenTtlSeconds` | number | 3600 | 撤销延迟上界（D210） |
//! | `oauth.authorizationCodeTtlSeconds` | number | 60 | 授权码 TTL（D205） |
//! | `oauth.deviceCodeTtlSeconds` | number | 600 | 设备码 TTL |
//! | `oauth.deviceCodePollIntervalSeconds` | number | 5 | 设备码轮询间隔（D246） |
//!
//! # 读取路径：**每请求读**（不是装配时读一次）
//!
//! 配置由 `platform_config_upsert` 写进 `platform_config` →
//! 运行期由**装配层**注入的 [`OAuthConfigSource`] 读（实现是
//! `platform-config` 的 `ConfigService::read_platform_entries`，
//! 它自带 epoch 失效的缓存）。
//!
//! **每请求读**的语义是「配置改了**下一个请求**就生效」——
//! 装配时读一次 = 配置改了**永远不生效**（除非重启）。
//!
//! ⚠️ 本 crate **不依赖** `platform-config`（依赖方向单向）→
//! 用 trait 接缝（与 `platform-iam` 的 `SsoConfigSource` 同款）。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use platform_core::RequestContext;

/// 装箱的 future（让 trait 保持**可对象化** —— `async fn` 在 trait 上不可 `dyn`）。
pub type OAuthBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 授权页 / 设备码页的前端基址。
pub const CONFIG_KEY_FRONTEND_BASE_URL: &str = "oauth.frontendBaseUrl";
/// access token 的有效期（秒）。
pub const CONFIG_KEY_ACCESS_TOKEN_TTL: &str = "oauth.accessTokenTtlSeconds";
/// 授权码的有效期（秒）。
pub const CONFIG_KEY_AUTHORIZATION_CODE_TTL: &str = "oauth.authorizationCodeTtlSeconds";
/// 设备码的有效期（秒）。
pub const CONFIG_KEY_DEVICE_CODE_TTL: &str = "oauth.deviceCodeTtlSeconds";
/// 设备码的轮询间隔（秒）。
pub const CONFIG_KEY_DEVICE_CODE_POLL_INTERVAL: &str = "oauth.deviceCodePollIntervalSeconds";

/// 全部配置键（**逐字**，顺序固定）。
pub const CONFIG_KEYS: [&str; 5] = [
    CONFIG_KEY_FRONTEND_BASE_URL,
    CONFIG_KEY_ACCESS_TOKEN_TTL,
    CONFIG_KEY_AUTHORIZATION_CODE_TTL,
    CONFIG_KEY_DEVICE_CODE_TTL,
    CONFIG_KEY_DEVICE_CODE_POLL_INTERVAL,
];

/// 配置读取失败（**不含任何配置值**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OAuthConfigError;

/// 出站 IdP 的运行期配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthSettings {
    /// 授权页 / 设备码页的前端基址；**空串 = 用 `{issuer}`**（D222）。
    pub frontend_base_url: String,
    /// access token 的有效期（秒）。
    pub access_token_ttl_seconds: i64,
    /// 授权码的有效期（秒）。
    pub authorization_code_ttl_seconds: i64,
    /// 设备码的有效期（秒）。
    pub device_code_ttl_seconds: i64,
    /// 设备码的轮询间隔（秒）。
    pub device_code_poll_interval_seconds: i64,
}

impl Default for OAuthSettings {
    /// 默认值与 manifest 的 `config` 段**逐字一致**。
    fn default() -> Self {
        Self {
            frontend_base_url: String::new(),
            access_token_ttl_seconds: 3600,
            authorization_code_ttl_seconds: 60,
            device_code_ttl_seconds: 600,
            device_code_poll_interval_seconds: 5,
        }
    }
}

impl OAuthSettings {
    /// 授权页基址：**未配置（空串）时回退到 `{issuer}`**（D222）。
    ///
    /// ⚠️ 这一个函数是「授权页 / 设备码页 URL」的唯一出口 ——
    /// 任何地方都**不得**直接用 `{issuer}` 拼授权页 URL
    /// （E2E 里二者**不相等**：`https://e2e.base` vs `http://127.0.0.1:8080`）。
    pub fn frontend_base(&self, issuer: &str) -> String {
        let configured = self.frontend_base_url.trim();
        if configured.is_empty() {
            issuer.trim_end_matches('/').to_string()
        } else {
            configured.trim_end_matches('/').to_string()
        }
    }
}

/// 配置的**读取源**（由装配层实现；本 crate **不依赖** `platform-config`）。
///
/// 实现方式：在 `platform-server` 里用 `ConfigService::read_platform_entries`
/// 读那 5 个键，映射成 [`OAuthSettings`]。
///
/// ⚠️ 读取失败必须返回 `Err`，**不得**降级成默认值 ——
/// 「读不出来就用默认」会让「改了配置不生效」变成一件看不见的事。
pub trait OAuthConfigSource: Send + Sync + 'static {
    /// 读取当前的有效配置。
    ///
    /// ⚠️ `self` 与 `ctx` 的生命周期**统一**成同一个 `'a` ——
    /// 实现里要把 `ctx` **跨 await** 传下去（读库），返回的 future
    /// 必须**同时**活得比两者久 → 不统一就编译不过。
    fn read<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> OAuthBoxFuture<'a, Result<OAuthSettings, OAuthConfigError>>;
}

/// 内存里的配置源（测试与「从别的来源已读好」的装配场景）。
#[derive(Debug, Clone, Default)]
pub struct StaticOAuthConfigSource {
    inner: Arc<RwLock<OAuthSettings>>,
}

impl StaticOAuthConfigSource {
    /// 用一份配置构造。
    pub fn new(settings: OAuthSettings) -> Self {
        Self {
            inner: Arc::new(RwLock::new(settings)),
        }
    }

    /// 替换配置。
    pub fn set(&self, settings: OAuthSettings) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = settings;
        }
    }

    /// 读一份快照。
    pub fn snapshot(&self) -> OAuthSettings {
        self.inner
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

impl OAuthConfigSource for StaticOAuthConfigSource {
    fn read<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> OAuthBoxFuture<'a, Result<OAuthSettings, OAuthConfigError>> {
        Box::pin(async move { Ok(self.snapshot()) })
    }
}

/// **永远读失败**的配置源（fail closed 的测试夹具）。
#[derive(Debug, Clone, Copy, Default)]
pub struct FailingOAuthConfigSource;

impl OAuthConfigSource for FailingOAuthConfigSource {
    fn read<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> OAuthBoxFuture<'a, Result<OAuthSettings, OAuthConfigError>> {
        Box::pin(async move { Err(OAuthConfigError) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_set_is_exactly_the_contract_set() {
        assert_eq!(CONFIG_KEYS.len(), 5);
        for key in CONFIG_KEYS {
            assert!(
                platform_core::config::ConfigKey::parse(key).is_ok(),
                "配置键必须是平台接受的形态（每段 lowerCamelCase）：{key}"
            );
        }
    }

    /// 默认值与 manifest 的 `config` 段**逐字一致**（design §7）。
    #[test]
    fn the_defaults_are_pinned() {
        let settings = OAuthSettings::default();
        assert_eq!(settings.frontend_base_url, "");
        assert_eq!(settings.access_token_ttl_seconds, 3600);
        assert_eq!(settings.authorization_code_ttl_seconds, 60);
        assert_eq!(settings.device_code_ttl_seconds, 600);
        assert_eq!(settings.device_code_poll_interval_seconds, 5);
    }

    /// 未配置前端基址 → 回退 `{issuer}`（D222）。
    #[test]
    fn an_empty_frontend_base_falls_back_to_the_issuer() {
        let settings = OAuthSettings::default();
        assert_eq!(
            settings.frontend_base("https://issuer.example"),
            "https://issuer.example"
        );
    }

    /// 配了 → 用配置值（**不用** issuer）。
    #[test]
    fn a_configured_frontend_base_wins() {
        let settings = OAuthSettings {
            frontend_base_url: "https://e2e.base".to_string(),
            ..OAuthSettings::default()
        };
        assert_eq!(
            settings.frontend_base("https://issuer.example"),
            "https://e2e.base"
        );
    }

    /// 末尾的斜杠被去掉 → 拼 `/oauth/authorize` 时不会出双斜杠。
    #[test]
    fn the_trailing_slash_is_trimmed() {
        let settings = OAuthSettings {
            frontend_base_url: "https://e2e.base/".to_string(),
            ..OAuthSettings::default()
        };
        assert_eq!(
            settings.frontend_base("https://issuer.example/"),
            "https://e2e.base"
        );
    }

    /// 内存配置源可以替换配置（模拟「配置改了要生效」）。
    #[test]
    fn the_static_source_can_be_replaced() {
        let source = StaticOAuthConfigSource::default();
        assert_eq!(source.snapshot().access_token_ttl_seconds, 3600);
        source.set(OAuthSettings {
            access_token_ttl_seconds: 60,
            ..OAuthSettings::default()
        });
        assert_eq!(source.snapshot().access_token_ttl_seconds, 60);
    }
}
