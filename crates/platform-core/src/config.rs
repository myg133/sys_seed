//! 配置类型骨架。
//!
//! 两组类型：
//!
//! - `AppConfig`：进程级配置（地址、数据库、缓存连接串），只做类型与默认值骨架。
//! - `ConfigScope` / `ConfigKey` / `ConfigEntry` / `InMemoryConfigResolver`：
//!   平台配置项模型，遵循 `api-conventions v2` C9 —— 键名 camelCase、层级 `.` 分隔、
//!   作用域优先级 空间 > 租户 > 平台。
//!
//! # REQ-010 的迁移（design-summary §1 / D63）
//!
//! | 内容 | 位置 |
//! |------|------|
//! | `ConfigResolver` **trait** / `ConfigSource` / `ResolvedConfig` / `FlagDecision` | `crate::extension::config`（能力索引只扫 `extension/`） |
//! | `AppConfig`（进程级配置） | 本文件 |
//! | `InMemoryConfigResolver`（REQ-002 的骨架实现） | 本文件 |
//!
//! 骨架原来叫 `ConfigResolver`（结构体）；REQ-010 把它改成 trait 并把这个
//! 纯内存实现改名为 `InMemoryConfigResolver` —— **不是**新增同名类型，
//! 所有引用点都跟着改（`lib.rs` / 能力目录 / 测试）。
//!
//! `source` 用的是四值的 [`ConfigSource`]，`ConfigScope` 保持三值（D60）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::RequestContext;
use crate::error::{CoreError, CoreResult};
use crate::extension::config::{ConfigResolver, ConfigSource, FlagDecision, ResolvedConfig};

/// 进程级配置骨架。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub database_url: Option<String>,
    pub redis_url: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            database_url: None,
            redis_url: None,
        }
    }
}

impl AppConfig {
    /// 从环境变量读取，未设置时回落到默认值。
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            host: env_or("HOST", defaults.host),
            port: env_or("PORT", defaults.port.to_string())
                .parse()
                .unwrap_or(defaults.port),
            database_url: std::env::var("DATABASE_URL").ok(),
            redis_url: std::env::var("REDIS_URL").ok(),
        }
    }

    /// 监听地址。
    pub fn socket_addr(&self) -> std::net::SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], self.port)))
    }
}

fn env_or(key: &str, fallback: String) -> String {
    match std::env::var(key) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => fallback,
    }
}

/// 配置作用域。优先级：空间 > 租户 > 平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigScope {
    Platform,
    Tenant,
    Space,
}

impl ConfigScope {
    /// 落库 / API 用的名字（`platform` / `tenant` / `space`）。
    ///
    /// ⚠️ **没有 `default`** —— `source` 的四值见
    /// [`crate::extension::config::ConfigSource`]（D60）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Tenant => "tenant",
            Self::Space => "space",
        }
    }

    /// 优先级权重，数值越大越优先。
    pub fn rank(self) -> u8 {
        match self {
            Self::Platform => 0,
            Self::Tenant => 1,
            Self::Space => 2,
        }
    }
}

/// 配置键：层级用 `.` 分隔，每段为 lowerCamelCase。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigKey(String);

impl ConfigKey {
    /// 解析并校验配置键。
    pub fn parse(raw: &str) -> CoreResult<Self> {
        let trimmed = raw.trim();
        let valid = !trimmed.is_empty() && trimmed.split('.').all(is_lower_camel_case);
        if valid {
            Ok(Self(trimmed.to_string()))
        } else {
            Err(CoreError::InvalidConfigKey {
                key: raw.to_string(),
            })
        }
    }

    /// 键的字符串形式。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ConfigKey {
    type Error = CoreError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

/// 一条配置记录：值 + 命中的作用域来源。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntry {
    pub key: String,
    pub value: Value,
    pub source: ConfigSource,
}

/// 纯内存的配置解析器（REQ-002 的骨架实现）。
///
/// 不连库、不缓存失效 —— 真实实现是 `platform-config` 的 `DbConfigResolver`；
/// 这个实现留在平台核心里供业务模块的单元测试与最小示例使用。
#[derive(Debug, Clone, Default)]
pub struct InMemoryConfigResolver {
    entries: Vec<ConfigEntry>,
}

impl InMemoryConfigResolver {
    /// 空解析器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 写入一条配置记录。
    pub fn insert(&mut self, entry: ConfigEntry) {
        self.entries.push(entry);
    }

    /// 按优先级解析配置键；未命中返回 `None`。
    pub fn resolve(&self, key: &str) -> Option<&ConfigEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.key == key)
            .max_by_key(|entry| entry.source.rank())
    }

    /// 已登记的配置条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl ConfigResolver for InMemoryConfigResolver {
    fn resolve(&self, _ctx: &RequestContext, key: &ConfigKey) -> Option<ResolvedConfig> {
        let entry = self
            .entries
            .iter()
            .filter(|entry| entry.key == key.as_str())
            .max_by_key(|entry| entry.source.rank())?;
        Some(ResolvedConfig {
            key: entry.key.clone(),
            value: entry.value.clone(),
            source: entry.source,
        })
    }

    fn flag(&self, _ctx: &RequestContext, flag: &str) -> FlagDecision {
        // 内存实现没有 flag 配置：一律「未配置 → 关闭 + default」（fail closed）。
        FlagDecision::disabled(flag)
    }
}

/// 判断一段是否是 lowerCamelCase：首字符小写字母，其余为字母或数字。
fn is_lower_camel_case(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_key_accepts_camel_case_segments() {
        let key = ConfigKey::parse("theme.radius").expect("合法键应解析成功");
        assert_eq!(key.as_str(), "theme.radius");
        assert!(ConfigKey::parse("logo").is_ok());
        assert!(ConfigKey::parse("primaryColor").is_ok());
    }

    #[test]
    fn config_key_rejects_non_camel_case() {
        for raw in [
            "",
            "  ",
            "Theme",
            "theme-radius",
            "theme.radius_x",
            ".theme",
        ] {
            let err = ConfigKey::parse(raw).expect_err("非法键应报错");
            assert_eq!(err.code(), "INVALID_CONFIG_KEY");
        }
    }

    #[test]
    fn resolver_prefers_space_over_tenant_over_platform() {
        let mut resolver = InMemoryConfigResolver::new();
        resolver.insert(ConfigEntry {
            key: "logo".to_string(),
            value: json!("/platform.svg"),
            source: ConfigSource::Platform,
        });
        resolver.insert(ConfigEntry {
            key: "logo".to_string(),
            value: json!("/tenant.svg"),
            source: ConfigSource::Tenant,
        });
        resolver.insert(ConfigEntry {
            key: "logo".to_string(),
            value: json!("/space.svg"),
            source: ConfigSource::Space,
        });

        let hit = resolver.resolve("logo").expect("应命中配置");
        assert_eq!(hit.value, json!("/space.svg"));
        assert_eq!(hit.source, ConfigSource::Space);
        assert_eq!(resolver.len(), 3);
    }

    #[test]
    fn resolver_returns_none_for_unknown_key() {
        let resolver = InMemoryConfigResolver::new();
        assert!(resolver.is_empty());
        assert!(resolver.resolve("missing").is_none());
    }

    #[test]
    fn scope_rank_orders_platform_tenant_space() {
        assert!(ConfigScope::Space.rank() > ConfigScope::Tenant.rank());
        assert!(ConfigScope::Tenant.rank() > ConfigScope::Platform.rank());
    }

    #[test]
    fn app_config_default_socket_addr() {
        let config = AppConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.socket_addr().port(), 8080);
    }
}
