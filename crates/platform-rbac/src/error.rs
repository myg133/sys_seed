//! 角色与权限体系的错误类型。
//!
//! # 契约 C6（`api-conventions v1`）
//!
//! - `code`：稳定的机器可读错误码（大写 + 下划线）；
//! - `detail`：**面向开发者的英文 ASCII** —— 面向用户的中文文案由前端 i18n 渲染；
//! - `params`：可选，补充错误码无法表达的参数（如被拒的权限点编码）。
//!
//! # 越权语义（契约 C5）
//!
//! | 情形 | 码 | HTTP |
//! |------|----|------|
//! | 缺权限点（动作类越权） | `INSUFFICIENT_PERMISSION` | **403** |
//! | 记录不在数据范围内 / 跨租户 id（资源类越权） | `NOT_FOUND` | **404** |
//!
//! 「无权访问的资源」与「不存在的资源」用**同一个**错误码与同一个状态码 ——
//! 两者**必须**无法区分。

use std::collections::BTreeMap;

use axum::http::StatusCode;

/// 稳定的机器可读错误码。
pub mod code {
    /// 需要认证（缺令牌 / 令牌无效）。
    pub const UNAUTHENTICATED: &str = "UNAUTHENTICATED";
    /// 权限不足（动作类越权）。
    pub const INSUFFICIENT_PERMISSION: &str = "INSUFFICIENT_PERMISSION";
    /// 资源不存在（资源类越权；**不泄露资源存在性**）。
    pub const NOT_FOUND: &str = "NOT_FOUND";
    /// 与当前状态冲突。
    pub const CONFLICT: &str = "CONFLICT";
    /// 参数不合法。
    pub const INVALID_ARGUMENT: &str = "INVALID_ARGUMENT";
    /// 内置角色受保护（不可删除 / 不可降权）。
    pub const BUILTIN_ROLE_PROTECTED: &str = "BUILTIN_ROLE_PROTECTED";
    /// 通配权限点被拒（`*:*` 与跨模块通配）。
    pub const WILDCARD_FORBIDDEN: &str = "WILDCARD_FORBIDDEN";
    /// 角色继承成环。
    pub const INHERITANCE_CYCLE: &str = "INHERITANCE_CYCLE";
    /// 权限点注册表对账不一致（**启动失败**，不是告警）。
    pub const REGISTRY_MISMATCH: &str = "REGISTRY_MISMATCH";
    /// 服务内部错误。
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
}

/// 角色与权限体系的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RbacError {
    /// 需要认证。
    Unauthenticated,
    /// 权限不足（缺该权限点）。
    InsufficientPermission {
        /// 被拒的权限点编码（**开发者信息**；响应里不得出现资源的业务细节）。
        required: String,
    },
    /// 资源不存在（跨租户 id 也走这里）。
    NotFound {
        /// 资源类型（英文）。
        resource: &'static str,
    },
    /// 与当前状态冲突。
    Conflict {
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 参数不合法。
    InvalidArgument {
        /// 出错的字段名。
        field: String,
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 内置角色受保护：不可删除、不可降权。
    BuiltinRoleProtected {
        /// 被拒的改动（`delete` / `data_scope` / `permission`）。
        operation: &'static str,
        /// 内置角色编码。
        role_code: String,
    },
    /// 通配权限点被拒（`*:*` 或跨模块通配）。
    WildcardForbidden {
        /// 被拒的编码。
        code: String,
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 角色继承成环（**指出是哪两个角色构成的环**）。
    InheritanceCycle {
        /// 环上的角色编码（至少两个）。
        roles: Vec<String>,
    },
    /// 权限点注册表与库不一致。
    RegistryMismatch {
        /// 逐条差异说明。
        differences: Vec<String>,
    },
    /// 服务内部错误（**不把内部细节漏给调用方**）。
    Internal {
        /// 面向开发者的英文原因。
        reason: String,
    },
}

impl RbacError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthenticated => code::UNAUTHENTICATED,
            Self::InsufficientPermission { .. } => code::INSUFFICIENT_PERMISSION,
            Self::NotFound { .. } => code::NOT_FOUND,
            Self::Conflict { .. } => code::CONFLICT,
            Self::InvalidArgument { .. } => code::INVALID_ARGUMENT,
            Self::BuiltinRoleProtected { .. } => code::BUILTIN_ROLE_PROTECTED,
            Self::WildcardForbidden { .. } => code::WILDCARD_FORBIDDEN,
            Self::InheritanceCycle { .. } => code::INHERITANCE_CYCLE,
            Self::RegistryMismatch { .. } => code::REGISTRY_MISMATCH,
            Self::Internal { .. } => code::INTERNAL_ERROR,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::InsufficientPermission { .. } => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Conflict { .. } | Self::BuiltinRoleProtected { .. } => StatusCode::CONFLICT,
            Self::InvalidArgument { .. } | Self::WildcardForbidden { .. } => {
                StatusCode::BAD_REQUEST
            }
            Self::InheritanceCycle { .. } => StatusCode::CONFLICT,
            Self::RegistryMismatch { .. } | Self::Internal { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    /// 面向开发者的**英文 ASCII** 描述（契约 C6）。
    pub fn detail(&self) -> String {
        match self {
            Self::Unauthenticated => "authentication is required".to_string(),
            Self::InsufficientPermission { required } => {
                format!("the current role does not grant the permission `{required}`")
            }
            Self::NotFound { resource } => format!("the {resource} was not found"),
            Self::Conflict { reason } => reason.clone(),
            Self::InvalidArgument { field, reason } => format!("{field}: {reason}"),
            Self::BuiltinRoleProtected {
                operation,
                role_code,
            } => format!("the built-in role `{role_code}` is protected and cannot be {operation}"),
            Self::WildcardForbidden { code, reason } => {
                format!("the permission wildcard `{code}` is not allowed: {reason}")
            }
            Self::InheritanceCycle { roles } => format!(
                "the role inheritance would create a cycle: {}",
                roles.join(" -> ")
            ),
            Self::RegistryMismatch { differences } => format!(
                "the permission registry does not match the database: {}",
                differences.join("; ")
            ),
            Self::Internal { reason } => format!("an internal error occurred: {reason}"),
        }
    }

    /// 错误码之外的补充参数（契约 C6 的 `params`）。
    pub fn params(&self) -> BTreeMap<String, serde_json::Value> {
        let mut params = BTreeMap::new();
        match self {
            Self::InsufficientPermission { required } => {
                params.insert(
                    "required".to_string(),
                    serde_json::Value::from(required.clone()),
                );
            }
            Self::NotFound { resource } => {
                params.insert("resource".to_string(), serde_json::Value::from(*resource));
            }
            Self::InvalidArgument { field, .. } => {
                params.insert("field".to_string(), serde_json::Value::from(field.clone()));
            }
            Self::BuiltinRoleProtected {
                operation,
                role_code,
            } => {
                params.insert("operation".to_string(), serde_json::Value::from(*operation));
                params.insert(
                    "roleCode".to_string(),
                    serde_json::Value::from(role_code.clone()),
                );
            }
            Self::WildcardForbidden { code, .. } => {
                params.insert("code".to_string(), serde_json::Value::from(code.clone()));
            }
            Self::InheritanceCycle { roles } => {
                params.insert("roles".to_string(), serde_json::Value::from(roles.clone()));
            }
            Self::RegistryMismatch { differences } => {
                params.insert(
                    "differences".to_string(),
                    serde_json::Value::from(differences.clone()),
                );
            }
            _ => {}
        }
        params
    }
}

impl std::fmt::Display for RbacError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for RbacError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_code_is_upper_snake_case() {
        let errors = [
            RbacError::Unauthenticated,
            RbacError::InsufficientPermission {
                required: "rbac:role:write".to_string(),
            },
            RbacError::NotFound { resource: "role" },
            RbacError::Conflict {
                reason: "busy".to_string(),
            },
            RbacError::InvalidArgument {
                field: "code".to_string(),
                reason: "bad".to_string(),
            },
            RbacError::BuiltinRoleProtected {
                operation: "delete",
                role_code: "platform-admin".to_string(),
            },
            RbacError::WildcardForbidden {
                code: "*:*".to_string(),
                reason: "bad".to_string(),
            },
            RbacError::InheritanceCycle {
                roles: vec!["a".to_string(), "b".to_string()],
            },
            RbacError::RegistryMismatch {
                differences: vec!["x".to_string()],
            },
            RbacError::Internal {
                reason: "boom".to_string(),
            },
        ];
        for error in &errors {
            let code = error.code();
            assert!(!code.is_empty());
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "错误码必须是大写 + 下划线：{code}"
            );
        }
    }

    /// 契约 C6：`detail` 必须是**英文 ASCII**。
    #[test]
    fn every_detail_is_english_ascii() {
        let errors = [
            RbacError::Unauthenticated,
            RbacError::InsufficientPermission {
                required: "rbac:role:write".to_string(),
            },
            RbacError::NotFound { resource: "role" },
            RbacError::BuiltinRoleProtected {
                operation: "delete",
                role_code: "platform-admin".to_string(),
            },
            RbacError::InheritanceCycle {
                roles: vec!["a".to_string(), "b".to_string()],
            },
        ];
        for error in &errors {
            let detail = error.detail();
            assert!(detail.is_ascii(), "detail 必须是 ASCII：{detail}");
            assert!(!detail.is_empty());
        }
    }

    /// 越权的两种语义：缺权限点 → 403；资源不在范围 → 404。
    #[test]
    fn the_three_rejection_statuses_follow_contract_c5() {
        assert_eq!(
            RbacError::Unauthenticated.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            RbacError::InsufficientPermission {
                required: "x".to_string()
            }
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            RbacError::NotFound { resource: "role" }.status(),
            StatusCode::NOT_FOUND
        );
    }

    /// 成环的错误必须**指出是哪两个角色构成的环**。
    #[test]
    fn the_cycle_error_names_the_roles() {
        let error = RbacError::InheritanceCycle {
            roles: vec!["rbac-cycle-a".to_string(), "rbac-cycle-b".to_string()],
        };
        assert!(error.detail().contains("rbac-cycle-a"));
        assert!(error.detail().contains("rbac-cycle-b"));
        let roles = error.params().get("roles").cloned().expect("必须带 roles");
        assert_eq!(roles.as_array().map(Vec::len), Some(2));
    }

    /// 内置角色的错误码要与「内置角色受保护」明确对应。
    #[test]
    fn the_builtin_role_error_code_is_distinct() {
        let error = RbacError::BuiltinRoleProtected {
            operation: "delete",
            role_code: "platform-admin".to_string(),
        };
        assert_eq!(error.code(), "BUILTIN_ROLE_PROTECTED");
        assert_eq!(error.status(), StatusCode::CONFLICT);
    }
}
