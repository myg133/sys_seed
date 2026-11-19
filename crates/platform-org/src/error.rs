//! 组织架构的错误类型。
//!
//! 契约 C6（`api-conventions v1`）：
//!
//! - `code`：**稳定**的机器可读错误码（大写 + 下划线），前端据此分支；
//! - `detail`：**面向开发者的英文 ASCII 描述** —— 面向用户的中文文案由前端 i18n
//!   按 `code` 渲染，后端**不得**在 `detail` 里拼中文句子；
//! - `params`：可选，补充错误码无法表达的参数。
//!
//! # 为什么「资源不存在」对跨租户访问也用它
//!
//! 契约 C5：资源类越权返回 **404**，不泄露资源是否存在。
//! 租户隔离在这里不是「越权」，而是**根本查不到** ——
//! 数据访问层每条语句都带租户条件，别的租户的 id 查出来就是「不存在」。

use std::collections::BTreeMap;

use axum::http::StatusCode;

/// 稳定的机器可读错误码。
pub mod code {
    /// 请求上下文里没有租户。
    pub const TENANT_CONTEXT_MISSING: &str = "TENANT_CONTEXT_MISSING";
    /// 需要认证。
    pub const UNAUTHENTICATED: &str = "UNAUTHENTICATED";
    /// 资源不存在（跨租户访问也走这里）。
    pub const NOT_FOUND: &str = "NOT_FOUND";
    /// 组织树操作会形成环（把节点移到自己的子孙下面）。
    pub const ORG_CYCLE: &str = "ORG_CYCLE";
    /// 组织节点仍有子节点，不能删除。
    pub const ORG_NODE_NOT_EMPTY: &str = "ORG_NODE_NOT_EMPTY";
    /// 组织节点仍有成员，必须先迁移或确认。
    pub const ORG_NODE_HAS_MEMBERS: &str = "ORG_NODE_HAS_MEMBERS";
    /// 节点编码在租户内重复。
    pub const ORG_CODE_DUPLICATED: &str = "ORG_CODE_DUPLICATED";
    /// 节点类型非法。
    pub const ORG_NODE_TYPE_INVALID: &str = "ORG_NODE_TYPE_INVALID";
    /// 参数不合法。
    pub const INVALID_ARGUMENT: &str = "INVALID_ARGUMENT";
    /// 与当前状态冲突。
    pub const CONFLICT: &str = "CONFLICT";
    /// 同步冲突尚未裁决。
    pub const SYNC_CONFLICT_PENDING: &str = "SYNC_CONFLICT_PENDING";
    /// 同步策略不允许自动覆盖。
    pub const SYNC_CONFLICT_STRATEGY: &str = "SYNC_CONFLICT_STRATEGY";
    /// 同步出站失败（第三方不可达等）。
    pub const SYNC_PUSH_FAILED: &str = "SYNC_PUSH_FAILED";
    /// 本期不实现的能力（如 SCIM 入站、HTTPS）。
    pub const NOT_IMPLEMENTED: &str = "NOT_IMPLEMENTED";
    /// 服务内部错误。
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
}

/// 本 crate 的全部错误码（**完备性测试的输入**，D113）。
///
/// 组织事件码（`org.member-added` 等）在 [`crate::notify::OrgEventKind::ALL_CODES`]。
pub const ALL_CODES: &[&str] = &[
    code::TENANT_CONTEXT_MISSING,
    code::UNAUTHENTICATED,
    code::NOT_FOUND,
    code::ORG_CYCLE,
    code::ORG_NODE_NOT_EMPTY,
    code::ORG_NODE_HAS_MEMBERS,
    code::ORG_CODE_DUPLICATED,
    code::ORG_NODE_TYPE_INVALID,
    code::INVALID_ARGUMENT,
    code::CONFLICT,
    code::SYNC_CONFLICT_PENDING,
    code::SYNC_CONFLICT_STRATEGY,
    code::SYNC_PUSH_FAILED,
    code::NOT_IMPLEMENTED,
    code::INTERNAL_ERROR,
];

/// 组织架构的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgError {
    /// 请求上下文里没有租户。
    TenantContextMissing,
    /// 需要认证。
    Unauthenticated,
    /// 资源不存在。
    NotFound {
        /// 资源类型（英文）。
        resource: &'static str,
    },
    /// 组织树操作会形成环。
    OrgCycle {
        /// 面向开发者的英文原因。
        reason: &'static str,
    },
    /// 组织节点仍有子节点。
    OrgNodeNotEmpty {
        /// 子节点数量。
        children: usize,
    },
    /// 组织节点仍有成员。
    OrgNodeHasMembers {
        /// 成员数量。
        members: usize,
    },
    /// 节点编码重复。
    OrgCodeDuplicated {
        /// 重复的编码。
        code: String,
    },
    /// 节点类型非法。
    OrgNodeTypeInvalid {
        /// 非法值。
        value: String,
    },
    /// 参数不合法。
    InvalidArgument {
        /// 参数名。
        field: &'static str,
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 与当前状态冲突。
    Conflict {
        /// 面向开发者的英文原因。
        reason: &'static str,
    },
    /// 同步冲突尚未裁决。
    SyncConflictPending {
        /// 冲突 ID。
        conflict_id: String,
    },
    /// 同步策略不允许自动覆盖。
    SyncConflictStrategy {
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 同步出站失败。
    SyncPushFailed {
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 本期不实现的能力。
    NotImplemented {
        /// 能力名（英文）。
        feature: &'static str,
    },
    /// 服务内部错误（**不把内部细节漏给调用方**）。
    Internal {
        /// 面向开发者的英文原因。
        reason: String,
    },
}

impl OrgError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::TenantContextMissing => code::TENANT_CONTEXT_MISSING,
            Self::Unauthenticated => code::UNAUTHENTICATED,
            Self::NotFound { .. } => code::NOT_FOUND,
            Self::OrgCycle { .. } => code::ORG_CYCLE,
            Self::OrgNodeNotEmpty { .. } => code::ORG_NODE_NOT_EMPTY,
            Self::OrgNodeHasMembers { .. } => code::ORG_NODE_HAS_MEMBERS,
            Self::OrgCodeDuplicated { .. } => code::ORG_CODE_DUPLICATED,
            Self::OrgNodeTypeInvalid { .. } => code::ORG_NODE_TYPE_INVALID,
            Self::InvalidArgument { .. } => code::INVALID_ARGUMENT,
            Self::Conflict { .. } => code::CONFLICT,
            Self::SyncConflictPending { .. } => code::SYNC_CONFLICT_PENDING,
            Self::SyncConflictStrategy { .. } => code::SYNC_CONFLICT_STRATEGY,
            Self::SyncPushFailed { .. } => code::SYNC_PUSH_FAILED,
            Self::NotImplemented { .. } => code::NOT_IMPLEMENTED,
            Self::Internal { .. } => code::INTERNAL_ERROR,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::TenantContextMissing | Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::OrgCycle { .. }
            | Self::OrgNodeNotEmpty { .. }
            | Self::OrgNodeHasMembers { .. }
            | Self::OrgCodeDuplicated { .. }
            | Self::OrgNodeTypeInvalid { .. }
            | Self::InvalidArgument { .. }
            | Self::SyncConflictStrategy { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Conflict { .. } | Self::SyncConflictPending { .. } => StatusCode::CONFLICT,
            Self::SyncPushFailed { .. } => StatusCode::BAD_GATEWAY,
            Self::NotImplemented { .. } => StatusCode::NOT_IMPLEMENTED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述（契约 C6）。
    pub fn detail(&self) -> String {
        match self {
            Self::TenantContextMissing => "the tenant context is missing".to_string(),
            Self::Unauthenticated => "authentication is required".to_string(),
            Self::NotFound { resource } => format!("the {resource} was not found"),
            Self::OrgCycle { reason } => (*reason).to_string(),
            Self::OrgNodeNotEmpty { children } => {
                format!("the organization node still has {children} child nodes")
            }
            Self::OrgNodeHasMembers { members } => {
                format!("the organization node still has {members} members")
            }
            Self::OrgCodeDuplicated { code } => {
                format!("the organization node code `{code}` is already in use")
            }
            Self::OrgNodeTypeInvalid { value } => {
                format!("`{value}` is not a valid organization node type")
            }
            Self::InvalidArgument { field, reason } => format!("invalid `{field}`: {reason}"),
            Self::Conflict { reason } => (*reason).to_string(),
            Self::SyncConflictPending { conflict_id } => {
                format!("the sync conflict `{conflict_id}` has not been adjudicated")
            }
            Self::SyncConflictStrategy { reason } => reason.clone(),
            Self::SyncPushFailed { reason } => reason.clone(),
            Self::NotImplemented { feature } => format!("{feature} is not implemented"),
            Self::Internal { reason } => format!("an internal error occurred: {reason}"),
        }
    }

    /// 错误码之外的补充参数（契约 C6 的 `params`）。
    pub fn params(&self) -> BTreeMap<String, serde_json::Value> {
        let mut params = BTreeMap::new();
        match self {
            Self::NotFound { resource } => {
                params.insert("resource".to_string(), serde_json::Value::from(*resource));
            }
            Self::OrgNodeNotEmpty { children } => {
                params.insert("children".to_string(), serde_json::Value::from(*children));
            }
            Self::OrgNodeHasMembers { members } => {
                params.insert("members".to_string(), serde_json::Value::from(*members));
            }
            Self::OrgCodeDuplicated { code } => {
                params.insert("code".to_string(), serde_json::Value::from(code.clone()));
            }
            Self::OrgNodeTypeInvalid { value } => {
                params.insert("value".to_string(), serde_json::Value::from(value.clone()));
            }
            Self::InvalidArgument { field, .. } => {
                params.insert("field".to_string(), serde_json::Value::from(*field));
            }
            Self::SyncConflictPending { conflict_id } => {
                params.insert(
                    "conflictId".to_string(),
                    serde_json::Value::from(conflict_id.clone()),
                );
            }
            _ => {}
        }
        params
    }
}

impl std::fmt::Display for OrgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for OrgError {}

impl From<anyhow::Error> for OrgError {
    fn from(error: anyhow::Error) -> Self {
        // 内部错误**不把细节漏给调用方**（契约 C6）：细节留在服务端日志里。
        OrgError::Internal {
            reason: error.to_string(),
        }
    }
}

impl From<platform_db::DbError> for OrgError {
    fn from(error: platform_db::DbError) -> Self {
        // 数据访问层的错误码是稳定的机器可读名，直接透传；
        // 细节留在服务端日志里，不进响应体。
        match error.code() {
            "MISSING_TENANT" => OrgError::TenantContextMissing,
            "ROW_NOT_FOUND" => OrgError::NotFound { resource: "record" },
            _ => match error.foreign_key_constraint() {
                // 外键违约 = **引用的那一行不存在**（幽灵 user_id / 跨租户 user_id）——
                // 这是**客户端错误**：该资源查不到 → 404（契约 C5：不泄露存在性），
                // 绝不是 500（那是「服务端坏了」，会把结构保证说成故障）。
                Some(constraint) => OrgError::NotFound {
                    resource: referenced_resource(constraint),
                },
                None => OrgError::Internal {
                    reason: error.code().to_string(),
                },
            },
        }
    }
}

/// 外键约束名 → **资源类型**（进 `params.resource`）。
///
/// 约束名来自迁移（`migrations/20261118000*_org_*.sql`）；对不上就退回通用的 `record`
/// —— 宁可粗一点，也不要猜错资源类型。
fn referenced_resource(constraint: &str) -> &'static str {
    match constraint {
        "user_org_user_fk" | "user_position_user_fk" | "external_identity_user_fk" => "user",
        "user_org_node_fk" | "user_position_node_fk" | "org_node_parent_fk" => "org_node",
        _ => "record",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL_CODES` 必须覆盖**每一个变体**（清单漏项 = 完备性测试假绿）。
    #[test]
    fn all_codes_lists_every_variant() {
        use std::collections::BTreeSet;

        let mut seen = BTreeSet::new();
        for error in all_errors() {
            assert!(
                ALL_CODES.contains(&error.code()),
                "ALL_CODES 缺少 {}",
                error.code()
            );
            seen.insert(error.code());
        }
        assert_eq!(seen.len(), ALL_CODES.len(), "清单与 match 不一致");
    }

    fn all_errors() -> Vec<OrgError> {
        vec![
            OrgError::TenantContextMissing,
            OrgError::Unauthenticated,
            OrgError::NotFound {
                resource: "org_node",
            },
            OrgError::OrgCycle {
                reason: "the target parent is a descendant of the node",
            },
            OrgError::OrgNodeNotEmpty { children: 2 },
            OrgError::OrgNodeHasMembers { members: 3 },
            OrgError::OrgCodeDuplicated {
                code: "d1".to_string(),
            },
            OrgError::OrgNodeTypeInvalid {
                value: "position".to_string(),
            },
            OrgError::InvalidArgument {
                field: "code",
                reason: "must not be empty".to_string(),
            },
            OrgError::Conflict { reason: "busy" },
            OrgError::SyncConflictPending {
                conflict_id: "c1".to_string(),
            },
            OrgError::SyncConflictStrategy {
                reason: "the manual strategy forbids overwriting".to_string(),
            },
            OrgError::SyncPushFailed {
                reason: "the remote server is unreachable".to_string(),
            },
            OrgError::NotImplemented {
                feature: "scim-inbound",
            },
            OrgError::Internal {
                reason: "boom".to_string(),
            },
        ]
    }

    #[test]
    fn every_error_code_is_upper_snake_case() {
        for error in all_errors() {
            let code = error.code();
            assert!(!code.is_empty());
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "错误码必须是大写 + 下划线：{code}"
            );
        }
    }

    #[test]
    fn every_detail_is_english_ascii() {
        for error in all_errors() {
            let detail = error.detail();
            assert!(detail.is_ascii(), "detail 必须是 ASCII：{detail}");
            assert!(!detail.is_empty(), "detail 不得为空");
        }
    }

    #[test]
    fn cross_tenant_access_is_reported_as_not_found() {
        // 契约 C5：资源类越权返回 404，不泄露资源存在性。
        assert_eq!(
            OrgError::NotFound {
                resource: "org_node"
            }
            .status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn a_cycle_is_rejected_with_a_dedicated_code() {
        let error = OrgError::OrgCycle {
            reason: "the target parent is a descendant of the node",
        };
        assert_eq!(error.code(), code::ORG_CYCLE);
        assert_eq!(error.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// 外键违约的替身（只为测「服务层能不能把它认成 404」）。
    #[derive(Debug)]
    struct FakeDatabaseError {
        code: &'static str,
        constraint: Option<&'static str>,
    }

    impl std::fmt::Display for FakeDatabaseError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "fake database error")
        }
    }

    impl std::error::Error for FakeDatabaseError {}

    impl sqlx::error::DatabaseError for FakeDatabaseError {
        fn message(&self) -> &str {
            "fake database error"
        }

        fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
            Some(std::borrow::Cow::Borrowed(self.code))
        }

        fn constraint(&self) -> Option<&str> {
            self.constraint
        }

        fn kind(&self) -> sqlx::error::ErrorKind {
            match self.code {
                "23503" => sqlx::error::ErrorKind::ForeignKeyViolation,
                _ => sqlx::error::ErrorKind::Other,
            }
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
    }

    fn db_error(code: &'static str, constraint: Option<&'static str>) -> platform_db::DbError {
        platform_db::DbError::Database(sqlx::Error::Database(Box::new(FakeDatabaseError {
            code,
            constraint,
        })))
    }

    #[test]
    fn a_foreign_key_violation_is_a_client_error_not_an_internal_one() {
        // 幽灵 user_id / 跨租户 user_id → 外键违约 → 404（不是 500）。
        let error = OrgError::from(db_error("23503", Some("user_org_user_fk")));
        assert_eq!(error.code(), code::NOT_FOUND);
        assert_eq!(error.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            error
                .params()
                .get("resource")
                .and_then(|value| value.as_str()),
            Some("user")
        );
    }

    #[test]
    fn other_database_errors_stay_internal() {
        let error = OrgError::from(db_error("23505", Some("user_org_uq")));
        assert_eq!(error.code(), code::INTERNAL_ERROR);
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
