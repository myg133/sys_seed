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
}
