//! 配置类型骨架。
//!
//! 两组类型：
//!
//! - `AppConfig`：进程级配置（地址、数据库、缓存连接串），只做类型与默认值骨架。
//! - `ConfigScope` / `ConfigKey` / `ConfigEntry` / `ConfigResolver`：
//!   平台配置项模型，遵循 `api-conventions v1` C9 —— 键名 camelCase、层级 `.` 分隔、
//!   作用域优先级 空间 > 租户 > 平台。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{CoreError, CoreResult};

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
    pub source: ConfigScope,
}

/// 配置解析器骨架：按作用域优先级命中一条配置。
#[derive(Debug, Clone, Default)]
pub struct ConfigResolver {
    entries: Vec<ConfigEntry>,
}

impl ConfigResolver {
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
        let mut resolver = ConfigResolver::new();
        resolver.insert(ConfigEntry {
            key: "logo".to_string(),
            value: json!("/platform.svg"),
            source: ConfigScope::Platform,
        });
        resolver.insert(ConfigEntry {
            key: "logo".to_string(),
            value: json!("/tenant.svg"),
            source: ConfigScope::Tenant,
        });
        resolver.insert(ConfigEntry {
            key: "logo".to_string(),
            value: json!("/space.svg"),
            source: ConfigScope::Space,
        });

        let hit = resolver.resolve("logo").expect("应命中配置");
        assert_eq!(hit.value, json!("/space.svg"));
        assert_eq!(hit.source, ConfigScope::Space);
        assert_eq!(resolver.len(), 3);
    }

    #[test]
    fn resolver_returns_none_for_unknown_key() {
        let resolver = ConfigResolver::new();
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
