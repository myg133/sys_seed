//! 统一错误类型（沿用 `platform-core` 的 `CoreError` 风格）。
//!
//! 错误码遵循 `api-conventions v1` C6：稳定的机器可读错误码（大写 + 下划线），
//! 面向用户的中文文案由前端 i18n 渲染，后端只返回错误码与参数。

use thiserror::Error;

/// 平台数据访问层的错误类型。
#[derive(Debug, Error)]
pub enum DbError {
    /// 数据库访问失败。
    #[error("数据库访问失败：{0}")]
    Database(#[from] sqlx::Error),

    /// 请求上下文里没有绑定租户。
    ///
    /// 租户作用域的数据访问**必须**有租户 —— 没有租户就意味着「查谁的」这个问题
    /// 没有答案，宁可报错也不查。
    #[error("请求上下文里没有绑定租户")]
    MissingTenant,

    /// 语句构造非法（缺租户占位符、占位符与绑定值数量对不上……）。
    #[error("SQL 构造非法：{reason}")]
    InvalidQuery {
        /// 面向开发者的英文原因。
        reason: String,
    },

    /// 查询没有命中任何行。
    #[error("查询没有命中任何行")]
    RowNotFound,

    /// 租户作用域表缺少 RLS 策略 —— 漏建 = 该表**完全无隔离**。
    #[error("表 {table} 没有 RLS 策略")]
    MissingPolicy {
        /// 缺策略的表名。
        table: String,
    },

    /// 连接角色绕过 RLS（超级用户或带 `BYPASSRLS` 属性）。
    ///
    /// 这种连接上 RLS 形同虚设 —— 租户隔离会 **fail open**（而不是 fail closed）。
    #[error("连接角色 {role} 绕过 RLS（超级用户或 BYPASSRLS），租户隔离会失效")]
    RlsBypassed {
        /// 角色名。
        role: String,
    },
}

impl DbError {
    /// 稳定的机器可读错误码，供前端按错误码分支处理。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Database(_) => "DATABASE_ERROR",
            Self::MissingTenant => "TENANT_CONTEXT_MISSING",
            Self::InvalidQuery { .. } => "INVALID_QUERY",
            Self::RowNotFound => "RECORD_NOT_FOUND",
            Self::MissingPolicy { .. } => "RLS_POLICY_MISSING",
            Self::RlsBypassed { .. } => "RLS_BYPASSED_BY_ROLE",
        }
    }

    /// 外键违约时的**约束名**（`None` = 不是外键违约）。
    ///
    /// 外键违约的含义是「**引用的那一行不存在**」——幽灵 id、跨租户 id 都落在这一类。
    /// 对调用方来说这是**客户端错误**（该资源查不到 → 404），不是服务端错误（500），
    /// 所以必须能把它从「数据库访问失败」里**认出来**，而不是当成普通错误码。
    pub fn foreign_key_constraint(&self) -> Option<&str> {
        let Self::Database(error) = self else {
            return None;
        };
        let database_error = error.as_database_error()?;
        if !database_error.is_foreign_key_violation() {
            return None;
        }
        database_error.constraint()
    }
}

/// 平台数据访问层统一的返回类型。
pub type DbResult<T> = Result<T, DbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable_and_upper_snake() {
        let errors = [
            DbError::MissingTenant,
            DbError::InvalidQuery {
                reason: "bad".to_string(),
            },
            DbError::RowNotFound,
            DbError::MissingPolicy {
                table: "t".to_string(),
            },
            DbError::RlsBypassed {
                role: "base".to_string(),
            },
        ];
        for error in &errors {
            let code = error.code();
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "错误码必须是大写 + 下划线：{code}"
            );
            assert!(!code.is_empty());
        }
        assert_eq!(DbError::MissingTenant.code(), "TENANT_CONTEXT_MISSING");
    }

    #[test]
    fn sqlx_error_converts_into_db_error() {
        let source = sqlx::Error::RowNotFound;
        let error: DbError = source.into();
        assert_eq!(error.code(), "DATABASE_ERROR");
    }

    /// 外键违约的替身（只为测「能不能认出它是外键违约」）。
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

    #[test]
    fn a_foreign_key_violation_is_recognized_by_its_constraint() {
        let source = sqlx::Error::Database(Box::new(FakeDatabaseError {
            code: "23503",
            constraint: Some("user_org_user_fk"),
        }));
        let error: DbError = source.into();
        assert_eq!(error.foreign_key_constraint(), Some("user_org_user_fk"));
    }

    #[test]
    fn other_database_errors_are_not_foreign_key_violations() {
        let source = sqlx::Error::Database(Box::new(FakeDatabaseError {
            code: "23505",
            constraint: Some("user_org_uq"),
        }));
        let error: DbError = source.into();
        assert_eq!(error.foreign_key_constraint(), None);

        let not_a_database_error: DbError = sqlx::Error::RowNotFound.into();
        assert_eq!(not_a_database_error.foreign_key_constraint(), None);
    }
}
