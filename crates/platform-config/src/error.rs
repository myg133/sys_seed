//! 配置模块的错误：**稳定错误码** + HTTP 状态 + 参数（契约 C6）。
//!
//! # 为什么每个拒绝都要一个码
//!
//! 契约 C6 要求 `code` 是**稳定的机器可读错误码**，前端据此做逻辑判断。
//! REQ-010 之前全仓只有 4 个码（`UNAUTHENTICATED` / `INSUFFICIENT_PERMISSION` /
//! `NOT_FOUND` / `INTERNAL_ERROR`），**没有任何 400 码** —— 没有码就只能靠
//! `detail` 文本匹配，既不可判定也不稳定。
//!
//! D62 定了 6 个；rev 3 的 D70 又补了第 7 个（`CONFIG_FLAG_USE_FLAG_ENDPOINT`）。
//! 本模块另外用了一个 **D72 的保留值错误码**（`CONFIG_TENANT_RESERVED`）——
//! 规格只要求「读写都报错」而没有给码；按更严格的一侧实现，并把这件事写进
//! `feature-meta/verification-report.md`。

use std::collections::BTreeMap;

use platform_core::problem::{
    CODE_INSUFFICIENT_PERMISSION, CODE_INTERNAL_ERROR, CODE_NOT_FOUND, CODE_UNAUTHENTICATED,
};
use platform_db::DbError;
use serde_json::Value;

/// 键名不符合 camelCase 分段规则。
pub const CODE_KEY_INVALID: &str = "CONFIG_KEY_INVALID";
/// 键未在 manifest 登记（写 → 400；读 → **404**）。
pub const CODE_KEY_NOT_REGISTERED: &str = "CONFIG_KEY_NOT_REGISTERED";
/// 往比登记更深的层写，或空间级写不在品牌白名单。
pub const CODE_SCOPE_NOT_ALLOWED: &str = "CONFIG_SCOPE_NOT_ALLOWED";
/// JSON 类型与登记类型不符。
pub const CODE_TYPE_MISMATCH: &str = "CONFIG_TYPE_MISMATCH";
/// 数值越界（`rolloutPercent` 不在 `0..=100`）。
pub const CODE_VALUE_OUT_OF_RANGE: &str = "CONFIG_VALUE_OUT_OF_RANGE";
/// flag 名不符合 `^[a-z][a-zA-Z0-9]{0,63}$`。
pub const CODE_FLAG_NAME_INVALID: &str = "FLAG_NAME_INVALID";
/// 经通用配置写路由改 flag（只能走 flag 接口，D70）。
pub const CODE_FLAG_USE_FLAG_ENDPOINT: &str = "CONFIG_FLAG_USE_FLAG_ENDPOINT";
/// 真实租户 id 取了保留值 `"platform"`（D72）。
pub const CODE_TENANT_RESERVED: &str = "CONFIG_TENANT_RESERVED";

/// 配置模块的错误。
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigError {
    /// 需要认证（401）。
    Unauthenticated,
    /// 真实租户 id 取了保留值 `platform`（D72）。
    ReservedTenant {
        /// 被拒绝的租户 id。
        tenant: String,
    },
    /// 键名不合法（键名格式先于注册表，D64）。
    KeyInvalid {
        /// 原始键名。
        key: String,
    },
    /// 写入时键未登记（400）。
    KeyNotRegistered {
        /// 原始键名。
        key: String,
    },
    /// 读取时含未登记键（**整请求 404**，D76）。
    UnknownKeys {
        /// 未登记的键名（按请求顺序）。
        keys: Vec<String>,
    },
    /// 作用域越级 / 路由不匹配 / 空间级不在白名单。
    ScopeNotAllowed {
        /// 出问题的键；路由级错误时为 `None`。
        key: Option<String>,
        /// 允许的最深层。
        allowed_scope: String,
        /// 实际尝试的层。
        attempted_scope: String,
        /// 英文原因（`whitelist` / `route` / `space id missing` …）。
        reason: String,
    },
    /// 通用写路由收到 flag 命名空间（D70）。
    FlagEndpointRequired {
        /// 被拒绝的键。
        key: String,
    },
    /// 通用读路由收到了平台级键（AC-S-063：整请求 403）��
    PlatformKeyOnGenericRoute {
        /// 被拒的平台级键（按请求顺序）。
        keys: Vec<String>,
    },
    /// JSON 类型与登记类型不符。
    TypeMismatch {
        /// 键名。
        key: String,
        /// 登记的类型。
        expected: String,
        /// 实际 JSON 类型。
        actual: String,
    },
    /// 数值越界。
    ValueOutOfRange {
        /// 键名。
        key: String,
        /// 实际值。
        value: Value,
        /// 允许的范围（`0..=100`）。
        allowed: String,
    },
    /// flag 名不合法（非法名**不进日志**，D65）。
    FlagNameInvalid {
        /// 原始 flag 名（截断后用于回显）。
        flag: String,
    },
    /// 空间级写没有 `X-Space-Id`。
    SpaceRequired {
        /// 键名。
        key: String,
    },
    /// 资源不存在（不泄露存在性）。
    NotFound {
        /// 资源类型。
        resource: String,
    },
    /// 数据访问错误（只保留可读描述；对外一律 `INTERNAL_ERROR`）。
    Database(String),
    /// 其它内部错误。
    Internal {
        /// 英文原因。
        reason: String,
    },
}

impl ConfigError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthenticated => CODE_UNAUTHENTICATED,
            Self::ReservedTenant { .. } => CODE_TENANT_RESERVED,
            Self::KeyInvalid { .. } => CODE_KEY_INVALID,
            Self::KeyNotRegistered { .. } => CODE_KEY_NOT_REGISTERED,
            Self::UnknownKeys { .. } => CODE_KEY_NOT_REGISTERED,
            Self::ScopeNotAllowed { .. } => CODE_SCOPE_NOT_ALLOWED,
            Self::FlagEndpointRequired { .. } => CODE_FLAG_USE_FLAG_ENDPOINT,
            Self::PlatformKeyOnGenericRoute { .. } => CODE_INSUFFICIENT_PERMISSION,
            Self::TypeMismatch { .. } => CODE_TYPE_MISMATCH,
            Self::ValueOutOfRange { .. } => CODE_VALUE_OUT_OF_RANGE,
            Self::FlagNameInvalid { .. } => CODE_FLAG_NAME_INVALID,
            Self::SpaceRequired { .. } => CODE_SCOPE_NOT_ALLOWED,
            Self::NotFound { .. } => CODE_NOT_FOUND,
            Self::Database(_) | Self::Internal { .. } => CODE_INTERNAL_ERROR,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> u16 {
        match self {
            Self::Unauthenticated => 401,
            // 通用读路由收到平台级键：整请求 fail closed（AC-S-063 / 契约 C9 §9.7）。
            Self::UnknownKeys { .. } => 404,
            Self::PlatformKeyOnGenericRoute { .. } => 403,
            Self::NotFound { .. } => 404,
            Self::Database(_) | Self::Internal { .. } => 500,
            _ => 400,
        }
    }

    /// 简短标题（**不是**面向用户的句子）。
    pub fn title(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "未认证",
            Self::ReservedTenant { .. } => "租户标识被保留",
            Self::KeyInvalid { .. } => "配置键不合法",
            Self::KeyNotRegistered { .. } | Self::UnknownKeys { .. } => "配置键未登记",
            Self::ScopeNotAllowed { .. } | Self::SpaceRequired { .. } => "配置作用域不被允许",
            Self::FlagEndpointRequired { .. } => "功能开关只能经开关接口修改",
            Self::PlatformKeyOnGenericRoute { .. } => "权限不足",
            Self::TypeMismatch { .. } => "配置值类型不符",
            Self::ValueOutOfRange { .. } => "配置值超出范围",
            Self::FlagNameInvalid { .. } => "功能开关名不合法",
            Self::NotFound { .. } => "资源不存在",
            Self::Database(_) | Self::Internal { .. } => "服务内部错误",
        }
    }

    /// 面向开发者的英文 ASCII 描述（契约 C6：**不得**拼中文句子）。
    pub fn detail(&self) -> String {
        match self {
            Self::Unauthenticated => "authentication is required".to_string(),
            Self::ReservedTenant { tenant } => format!(
                "the tenant id `{}` is reserved for platform-scoped access and cannot be used as a real tenant",
                truncate(tenant)
            ),
            Self::KeyInvalid { key } => format!(
                "the config key `{}` is not a valid camelCase key",
                truncate(key)
            ),
            Self::KeyNotRegistered { key } => {
                format!(
                    "the config key `{}` is not declared in any manifest",
                    truncate(key)
                )
            }
            Self::UnknownKeys { keys } => format!(
                "{} requested config key(s) are not declared in any manifest",
                keys.len()
            ),
            Self::ScopeNotAllowed {
                key,
                allowed_scope,
                attempted_scope,
                reason,
            } => match key {
                Some(key) => format!(
                    "writing `{}` at scope `{}` is not allowed (allowed: `{allowed_scope}`, reason: {reason})",
                    truncate(key),
                    truncate(attempted_scope)
                ),
                None => format!(
                    "the scope `{}` is not allowed on this endpoint (allowed: `{allowed_scope}`, reason: {reason})",
                    truncate(attempted_scope)
                ),
            },
            Self::FlagEndpointRequired { key } => format!(
                "the config key `{}` belongs to the feature-flag namespace; use PUT /api/feature-flags/{{flag}} instead",
                truncate(key)
            ),
            Self::PlatformKeyOnGenericRoute { keys } => format!(
                "{} requested config key(s) are platform-scoped; use GET /api/config/platform-entries with the `config:platform:read` permission",
                keys.len()
            ),
            Self::TypeMismatch {
                key,
                expected,
                actual,
            } => format!(
                "the value of `{}` must be a JSON `{expected}` value, got `{actual}`",
                truncate(key)
            ),
            Self::ValueOutOfRange {
                key,
                value,
                allowed,
            } => format!(
                "the value `{}` of `{}` is out of range (allowed: {allowed})",
                truncate(&value.to_string()),
                truncate(key)
            ),
            Self::FlagNameInvalid { flag } => format!(
                "the feature flag name `{}` does not match ^[a-z][a-zA-Z0-9]{{0,63}}$",
                truncate(flag)
            ),
            Self::SpaceRequired { key } => format!(
                "a space-scoped write of `{}` requires the X-Space-Id header",
                truncate(key)
            ),
            Self::NotFound { resource } => format!("the {resource} was not found"),
            Self::Database(code) => format!("the config store is unavailable ({code})"),
            Self::Internal { reason } => format!("internal config error: {}", ascii_safe(reason)),
        }
    }

    /// 补充参数（能给出「期望 / 实际」时都给出）。
    pub fn params(&self) -> BTreeMap<String, Value> {
        let mut params = BTreeMap::new();
        match self {
            Self::ReservedTenant { tenant } => {
                params.insert("tenant".to_string(), Value::from(truncate(tenant)));
            }
            Self::KeyInvalid { key } | Self::KeyNotRegistered { key } => {
                params.insert("key".to_string(), Value::from(truncate(key)));
            }
            Self::UnknownKeys { keys } => {
                params.insert("unknownKeys".to_string(), Value::from(keys.clone()));
            }
            Self::ScopeNotAllowed {
                key,
                allowed_scope,
                attempted_scope,
                reason,
            } => {
                if let Some(key) = key {
                    params.insert("key".to_string(), Value::from(truncate(key)));
                }
                params.insert(
                    "allowedScope".to_string(),
                    Value::from(allowed_scope.clone()),
                );
                params.insert(
                    "attemptedScope".to_string(),
                    Value::from(attempted_scope.clone()),
                );
                params.insert("reason".to_string(), Value::from(reason.clone()));
            }
            Self::FlagEndpointRequired { key } => {
                params.insert("key".to_string(), Value::from(truncate(key)));
            }
            Self::PlatformKeyOnGenericRoute { keys } => {
                params.insert(
                    "required".to_string(),
                    Value::from("config:platform:read".to_string()),
                );
                params.insert("forbiddenKeys".to_string(), Value::from(keys.clone()));
            }
            Self::TypeMismatch {
                key,
                expected,
                actual,
            } => {
                params.insert("key".to_string(), Value::from(truncate(key)));
                params.insert("expected".to_string(), Value::from(expected.clone()));
                params.insert("actual".to_string(), Value::from(actual.clone()));
            }
            Self::ValueOutOfRange {
                key,
                value,
                allowed,
            } => {
                params.insert("key".to_string(), Value::from(truncate(key)));
                params.insert("value".to_string(), value.clone());
                params.insert("allowed".to_string(), Value::from(allowed.clone()));
            }
            Self::FlagNameInvalid { flag } => {
                params.insert("flag".to_string(), Value::from(truncate(flag)));
            }
            Self::SpaceRequired { key } => {
                params.insert("key".to_string(), Value::from(truncate(key)));
            }
            Self::NotFound { resource } => {
                params.insert("resource".to_string(), Value::from(resource.clone()));
            }
            Self::Unauthenticated | Self::Database(_) | Self::Internal { .. } => {}
        }
        params
    }
}

impl From<DbError> for ConfigError {
    fn from(error: DbError) -> Self {
        // 底层错误的中文描述只进**日志**；响应体里的 `detail` 必须是英文 ASCII（契约 C6）。
        tracing::error!(code = error.code(), error = %error, "配置数据访问失败");
        Self::Database(error.code().to_string())
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.detail())
    }
}

impl std::error::Error for ConfigError {}

/// 把字符串截断到 64 个字符（日志 / 回显用，防止刷爆与伪造日志行）。
///
/// 控制字符替换成 `?` —— 换行会把一条日志 / JSON 字段拆成两行（日志注入）。
pub fn truncate(value: &str) -> String {
    ascii_safe(value).chars().take(64).collect()
}

/// 把非 ASCII / 控制字符替换成 `?`。
///
/// 契约 C6：`detail` 必须是**英文 ASCII**。键名 / flag 名来自调用方，
/// 可能是中文或控制字符 —— 原样拼进去会违反契约，也会让日志出现伪造行。
pub fn ascii_safe(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_control() {
                c
            } else {
                '?'
            }
        })
        .collect()
}

/// JSON 值的中文类型名 → 稳定的英文类型名（`params.actual`）。
pub fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_detail_is_english_ascii() {
        let errors = [
            ConfigError::KeyInvalid {
                key: "Theme".to_string(),
            },
            ConfigError::KeyNotRegistered {
                key: "nope".to_string(),
            },
            ConfigError::UnknownKeys {
                keys: vec!["nope".to_string()],
            },
            ConfigError::ScopeNotAllowed {
                key: Some("conflictStrategy".to_string()),
                allowed_scope: "platform".to_string(),
                attempted_scope: "tenant".to_string(),
                reason: "registered scope".to_string(),
            },
            ConfigError::FlagEndpointRequired {
                key: "beta".to_string(),
            },
            ConfigError::TypeMismatch {
                key: "pageSize".to_string(),
                expected: "number".to_string(),
                actual: "string".to_string(),
            },
            ConfigError::ValueOutOfRange {
                key: "flags.beta.rolloutPercent".to_string(),
                value: Value::from(101),
                allowed: "0..=100".to_string(),
            },
            ConfigError::FlagNameInvalid {
                flag: "bad\nINJECTED".to_string(),
            },
            ConfigError::ReservedTenant {
                tenant: "platform".to_string(),
            },
        ];
        for error in &errors {
            assert!(
                error.detail().is_ascii(),
                "detail 必须是英文 ASCII：{}",
                error.detail()
            );
            assert!(!error.title().is_empty());
        }
    }

    /// 键名 / flag 名来自调用方，可能是中文或控制字符 —— `detail` 仍必须是 ASCII。
    #[test]
    fn details_are_ascii_even_for_non_ascii_keys() {
        for error in [
            ConfigError::KeyInvalid {
                key: "配置项".to_string(),
            },
            ConfigError::KeyNotRegistered {
                key: "未登记\n键".to_string(),
            },
            ConfigError::FlagNameInvalid {
                flag: "开关\t名".to_string(),
            },
            ConfigError::Internal {
                reason: "内部错误".to_string(),
            },
        ] {
            assert!(
                error.detail().is_ascii(),
                "detail 必须是 ASCII：{}",
                error.detail()
            );
        }
    }

    #[test]
    fn a_read_of_unknown_keys_is_404_and_a_write_is_400() {
        let read = ConfigError::UnknownKeys {
            keys: vec!["nope".to_string()],
        };
        assert_eq!(read.status(), 404);
        assert_eq!(read.code(), CODE_KEY_NOT_REGISTERED);
        assert_eq!(
            read.params().get("unknownKeys"),
            Some(&Value::from(vec!["nope".to_string()]))
        );

        let write = ConfigError::KeyNotRegistered {
            key: "nope".to_string(),
        };
        assert_eq!(write.status(), 400);
        assert_eq!(write.code(), CODE_KEY_NOT_REGISTERED);
    }

    #[test]
    fn truncation_keeps_log_fields_bounded() {
        assert_eq!(truncate(&"a".repeat(200)).len(), 64);
        assert!(!truncate("bad\nINJECTED").contains('\n'));
    }
}
