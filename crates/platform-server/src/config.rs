//! 部署期配置：环境变量 → 强类型配置，缺必填项 **fail-fast**。
//!
//! 同一套镜像靠环境变量切分 dev / staging / prod，不重新构建（REQ-005 AC-S-005）。
//! 配置只在进程启动时读一次；读不到必填项就在监听端口之前退出，
//! 不允许「启动到一半才报错」。
//!
//! 优先级（本期只实现前两级，配置文件与配置中心留给后续 REQ）：
//!
//! ```text
//! 代码默认值 < 环境变量
//! ```

use std::fmt;

/// 缺必填环境变量时的错误码。
pub const CODE_MISSING: &str = "CONFIG_MISSING_REQUIRED";
/// 环境变量取值非法时的错误码。
pub const CODE_INVALID: &str = "CONFIG_INVALID_VALUE";
/// 环境变量组合被禁止时的错误码。
pub const CODE_FORBIDDEN: &str = "CONFIG_FORBIDDEN_COMBINATION";

/// 运行环境。同一镜像靠它切分 dev / staging / prod。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Dev,
    Staging,
    Prod,
}

impl Environment {
    /// 解析 `BASE_ENV`。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dev" => Some(Self::Dev),
            "staging" => Some(Self::Staging),
            "prod" => Some(Self::Prod),
            _ => None,
        }
    }

    /// 环境名（用于日志与响应体）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Staging => "staging",
            Self::Prod => "prod",
        }
    }
}

/// 租户隔离模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantMode {
    Shared,
    Schema,
    Database,
}

impl TenantMode {
    /// 解析 `BASE_TENANT_MODE`。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "shared" => Some(Self::Shared),
            "schema" => Some(Self::Schema),
            "database" => Some(Self::Database),
            _ => None,
        }
    }

    /// 模式名。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Schema => "schema",
            Self::Database => "database",
        }
    }
}

/// 认证模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Local,
    Sso,
    Hybrid,
}

impl AuthMode {
    /// 解析 `BASE_AUTH_MODE`。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "local" => Some(Self::Local),
            "sso" => Some(Self::Sso),
            "hybrid" => Some(Self::Hybrid),
            _ => None,
        }
    }

    /// 模式名。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Sso => "sso",
            Self::Hybrid => "hybrid",
        }
    }
}

/// 一条配置问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigProblem {
    /// 稳定错误码。
    pub code: &'static str,
    /// 出问题的环境变量名。
    pub key: &'static str,
    /// 人类可读说明（面向运维，不面向最终用户）。
    pub detail: String,
}

impl fmt::Display for ConfigProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}：{}", self.code, self.key, self.detail)
    }
}

/// 配置校验失败。进程据此 fail-fast 退出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// 全部问题（一次报全，避免运维来回试）。
    pub problems: Vec<ConfigProblem>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "配置校验失败（{} 项）", self.problems.len())?;
        for problem in &self.problems {
            write!(f, "\n  - {problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

/// 部署期配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployConfig {
    /// 运行环境。
    pub environment: Environment,
    /// 数据库连接串（必填）。
    pub database_url: String,
    /// 缓存连接串（可选）。
    pub redis_url: Option<String>,
    /// 监听地址。
    pub host: String,
    /// 监听端口。
    pub port: u16,
    /// 租户隔离模式。
    pub tenant_mode: TenantMode,
    /// 日志级别。
    pub log_level: String,
    /// 认证模式。
    pub auth_mode: AuthMode,
    /// 启动时是否灌种子数据（生产必须 false）。
    pub seed_on_start: bool,
    /// 迁移目录（相对路径或绝对路径）。
    pub migrations_dir: String,
    /// 前端静态资源目录；`None` 表示不托管前端。
    pub static_dir: Option<String>,
}

impl DeployConfig {
    /// 从进程环境读取配置。
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// 从任意查找函数读取配置 —— 测试用注入的键值表，不碰真实环境。
    pub fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut problems: Vec<ConfigProblem> = Vec::new();

        let database_url = required(
            &lookup,
            "DATABASE_URL",
            "数据库连接串必须显式提供，不使用默认值；示例：postgres://user:pass@host:5432/base",
            &mut problems,
        )
        .unwrap_or_default();

        let environment = match optional(&lookup, "BASE_ENV") {
            None => Environment::Dev,
            Some(raw) => Environment::parse(&raw).unwrap_or_else(|| {
                problems.push(invalid("BASE_ENV", &raw, "dev | staging | prod"));
                Environment::Dev
            }),
        };

        let tenant_mode = match optional(&lookup, "BASE_TENANT_MODE") {
            None => TenantMode::Shared,
            Some(raw) => TenantMode::parse(&raw).unwrap_or_else(|| {
                problems.push(invalid(
                    "BASE_TENANT_MODE",
                    &raw,
                    "shared | schema | database",
                ));
                TenantMode::Shared
            }),
        };

        let auth_mode = match optional(&lookup, "BASE_AUTH_MODE") {
            None => AuthMode::Local,
            Some(raw) => AuthMode::parse(&raw).unwrap_or_else(|| {
                problems.push(invalid("BASE_AUTH_MODE", &raw, "local | sso | hybrid"));
                AuthMode::Local
            }),
        };

        let port = match optional(&lookup, "SERVER_PORT") {
            None => 8080,
            Some(raw) => match raw.trim().parse::<u16>() {
                Ok(value) => value,
                Err(_) => {
                    problems.push(invalid("SERVER_PORT", &raw, "1-65535 的整数"));
                    8080
                }
            },
        };

        let seed_on_start = match optional(&lookup, "BASE_SEED_ON_START") {
            None => false,
            Some(raw) => match raw.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => true,
                "false" | "0" | "no" | "off" => false,
                _ => {
                    problems.push(invalid("BASE_SEED_ON_START", &raw, "true | false"));
                    false
                }
            },
        };

        if environment == Environment::Prod && seed_on_start {
            problems.push(ConfigProblem {
                code: CODE_FORBIDDEN,
                key: "BASE_SEED_ON_START",
                detail: "生产环境禁止启动时灌种子数据（请设为 false）".to_string(),
            });
        }

        if !problems.is_empty() {
            return Err(ConfigError { problems });
        }

        Ok(Self {
            environment,
            database_url,
            redis_url: optional(&lookup, "REDIS_URL"),
            host: optional(&lookup, "HOST").unwrap_or_else(|| "0.0.0.0".to_string()),
            port,
            tenant_mode,
            log_level: optional(&lookup, "BASE_LOG_LEVEL").unwrap_or_else(|| "info".to_string()),
            auth_mode,
            seed_on_start,
            migrations_dir: optional(&lookup, "BASE_MIGRATIONS_DIR")
                .unwrap_or_else(|| "./migrations".to_string()),
            static_dir: optional(&lookup, "BASE_STATIC_DIR"),
        })
    }

    /// 监听地址。
    pub fn socket_addr(&self) -> std::net::SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .unwrap_or_else(|_| std::net::SocketAddr::from(([0, 0, 0, 0], self.port)))
    }
}

/// 取必填项；缺失或为空串记一条问题。
fn required<F>(
    lookup: &F,
    key: &'static str,
    hint: &str,
    problems: &mut Vec<ConfigProblem>,
) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    match lookup(key) {
        Some(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => {
            problems.push(ConfigProblem {
                code: CODE_MISSING,
                key,
                detail: format!("必填环境变量缺失或为空；{hint}"),
            });
            None
        }
    }
}

/// 取可选项：缺失或为空串都算「没设」。
fn optional<F>(lookup: &F, key: &str) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    lookup(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 记一条「取值非法」问题。
fn invalid(key: &'static str, raw: &str, expected: &str) -> ConfigProblem {
    ConfigProblem {
        code: CODE_INVALID,
        key,
        detail: format!("取值 `{raw}` 非法，期望 {expected}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 构造一个查找函数。
    fn lookup_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn missing_database_url_is_reported_with_stable_code() {
        let err =
            DeployConfig::from_lookup(lookup_from(&[])).expect_err("缺 DATABASE_URL 必须失败");
        assert_eq!(err.problems.len(), 1);
        assert_eq!(err.problems[0].code, CODE_MISSING);
        assert_eq!(err.problems[0].key, "DATABASE_URL");
    }

    #[test]
    fn blank_database_url_counts_as_missing() {
        let err = DeployConfig::from_lookup(lookup_from(&[("DATABASE_URL", "   ")]))
            .expect_err("空串必须判为缺失");
        assert_eq!(err.problems[0].code, CODE_MISSING);
    }

    #[test]
    fn all_problems_are_reported_at_once() {
        let err = DeployConfig::from_lookup(lookup_from(&[
            ("DATABASE_URL", "postgres://base:base@db:5432/base"),
            ("SERVER_PORT", "not-a-port"),
            ("BASE_ENV", "pro"),
            ("BASE_AUTH_MODE", "ldap"),
            ("BASE_TENANT_MODE", "row"),
        ]))
        .expect_err("多个非法值必须一次报全");
        assert_eq!(err.problems.len(), 4, "problems = {err}");
        assert!(err.problems.iter().all(|p| p.code == CODE_INVALID));
    }

    #[test]
    fn missing_and_invalid_are_reported_together() {
        let err = DeployConfig::from_lookup(lookup_from(&[("BASE_ENV", "pro")]))
            .expect_err("缺必填项与非法值必须一起报");
        assert_eq!(err.problems.len(), 2, "problems = {err}");
        assert_eq!(err.problems[0].code, CODE_MISSING);
        assert_eq!(err.problems[1].code, CODE_INVALID);
    }

    #[test]
    fn defaults_apply_when_only_database_url_is_set() {
        let config = DeployConfig::from_lookup(lookup_from(&[(
            "DATABASE_URL",
            "postgres://base:base@localhost:5432/base_dev",
        )]))
        .expect("只给 DATABASE_URL 应当通过");
        assert_eq!(config.environment, Environment::Dev);
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.tenant_mode, TenantMode::Shared);
        assert_eq!(config.auth_mode, AuthMode::Local);
        assert_eq!(config.log_level, "info");
        assert!(!config.seed_on_start);
        assert_eq!(config.migrations_dir, "./migrations");
        assert_eq!(config.static_dir, None);
    }

    #[test]
    fn same_image_switches_environment_by_env_var() {
        let config = DeployConfig::from_lookup(lookup_from(&[
            ("DATABASE_URL", "postgres://base:base@db:5432/base"),
            ("BASE_ENV", "prod"),
            ("BASE_TENANT_MODE", "schema"),
            ("BASE_AUTH_MODE", "sso"),
            ("SERVER_PORT", "9090"),
        ]))
        .expect("合法配置应当通过");
        assert_eq!(config.environment, Environment::Prod);
        assert_eq!(config.tenant_mode, TenantMode::Schema);
        assert_eq!(config.auth_mode, AuthMode::Sso);
        assert_eq!(config.port, 9090);
        assert_eq!(config.socket_addr().port(), 9090);
    }

    #[test]
    fn prod_forbids_seed_on_start() {
        let err = DeployConfig::from_lookup(lookup_from(&[
            ("DATABASE_URL", "postgres://base:base@db:5432/base"),
            ("BASE_ENV", "prod"),
            ("BASE_SEED_ON_START", "true"),
        ]))
        .expect_err("生产环境禁止 seed");
        assert_eq!(err.problems[0].code, CODE_FORBIDDEN);
    }

    #[test]
    fn dev_allows_seed_on_start() {
        let config = DeployConfig::from_lookup(lookup_from(&[
            ("DATABASE_URL", "postgres://base:base@db:5432/base"),
            ("BASE_SEED_ON_START", "true"),
        ]))
        .expect("开发环境允许 seed");
        assert!(config.seed_on_start);
    }

    #[test]
    fn optional_values_are_trimmed_and_blank_is_treated_as_unset() {
        let config = DeployConfig::from_lookup(lookup_from(&[
            ("DATABASE_URL", " postgres://base:base@db:5432/base "),
            ("REDIS_URL", "  "),
            ("BASE_LOG_LEVEL", " debug "),
        ]))
        .expect("合法配置应当通过");
        assert_eq!(config.database_url, "postgres://base:base@db:5432/base");
        assert_eq!(config.redis_url, None);
        assert_eq!(config.log_level, "debug");
    }

    #[test]
    fn error_message_lists_every_problem() {
        let err = DeployConfig::from_lookup(lookup_from(&[("BASE_ENV", "pro")]))
            .expect_err("非法环境必须失败");
        let text = err.to_string();
        assert!(text.contains("配置校验失败"), "text = {text}");
        assert!(text.contains("BASE_ENV"), "text = {text}");
        assert!(text.contains(CODE_INVALID), "text = {text}");
    }
}
