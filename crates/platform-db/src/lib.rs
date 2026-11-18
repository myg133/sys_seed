//! 平台数据访问层：租户作用域入口 + PostgreSQL RLS 兜底。
//!
//! # 租户隔离做双保险
//!
//! | 层 | 做什么 | 失效后果 |
//! |----|--------|----------|
//! | **应用层** | 每条语句都带租户条件（`{tenant}` 占位符，平台强制） | 写错 SQL → 查到他租数据 |
//! | **数据库层** | PostgreSQL RLS（`current_setting('app.tenant_id', true)`） | 应用层完全失守 → 仍然查不到他租数据 |
//!
//! 任一层失效都不至于跨租户泄漏 —— 这是本层存在的全部理由。
//!
//! # 业务模块怎么用
//!
//! ```text
//! platform-core (RequestContext)  ←  platform-db (本层)  ←  业务模块
//! ```
//!
//! 业务模块**只**通过 [`TenantDb`] 访问数据，公开 API 里**没有**「拿裸连接」的方法
//! （由 `no_public_api_returns_a_raw_connection` 测试守着）——
//! 拿不到裸连接，就绕不过租户条件。
//!
//! ```ignore
//! let db = TenantDb::connect(&url, DbOptions::default()).await?;
//! let ctx = RequestContext::new().with_tenant("tenant-a");
//!
//! // 单次操作
//! let rows = db.fetch_all(&ctx, &query).await?;
//!
//! // 多操作：同一个事务、同一个租户上下文
//! let mut tx = db.begin(&ctx).await?;
//! tx.execute(&first).await?;
//! tx.execute(&second).await?;
//! tx.commit().await?;
//! ```
//!
//! # 为什么是 `SET LOCAL` 而不是 `SET`
//!
//! 连接池会把连接**借出去再还回来**。普通 `SET` 是**会话级**的：
//!
//! ```text
//! 连接 A：SET app.tenant_id = 'tenant-a'   ← 查询完成，连接归还池子
//! 连接 A：被下一个请求借走 → 仍然带着 'tenant-a'
//! 下一个请求（tenant-b）→ 查到了 tenant-a 的数据   ← 串租户
//! ```
//!
//! `SET LOCAL` 是**事务级**的：事务提交或回滚时自动清除。连接归还池子时是干净的，
//! 下一个租户借到它时不会继承上一个租户的上下文。
//!
//! 本层的每一次访问都在**显式事务**里执行，并在事务内设置租户；
//! 租户上下文**只在事务内有效**，绝不留在连接上。
//!
//! 代价：每次访问多一个事务（一次 `BEGIN`/`COMMIT`）。
//! 这是刻意的 —— 串租户这类 bug 极难复现（取决于连接池的调度顺序），
//! 宁可多一次事务往返。

mod error;
mod query;
mod quota;
mod rls;
mod tenant;

pub use error::{DbError, DbResult};
pub use query::{BindValue, TENANT_TOKEN, TenantQuery};
pub use quota::{QUOTA_TABLE, QuotaUsage};
pub use rls::{
    IsolationReport, TableIsolation, apply_policies, policy_statements, require_isolation,
    unscoped_row_count, verify_policies,
};
pub use tenant::{ACQUIRE_TIMEOUT, DbOptions, TenantDb, TenantTx};

/// 供 [`tenant_row!`] 宏使用：sqlx 错误类型。
pub use sqlx::Error as SqlxError;
/// 供 [`tenant_row!`] 宏使用：`FromRow` trait。
pub use sqlx::FromRow;
/// 供 [`tenant_row!`] 宏使用：`Row` trait。
pub use sqlx::Row;
/// 供 [`tenant_row!`] 宏使用：行类型。
pub use sqlx::postgres::PgRow;

/// 为行类型生成 `FromRow` 实现（**手写版 derive**，不需要 sqlx 的 `macros` feature）。
///
/// 低配机的冷编译对 proc-macro 编译链敏感（`sqlx` 的 `macros` feature 会拉进
/// `syn` / `quote` / `proc-macro2`），所以这里用声明宏手写 `FromRow` 实现 ——
/// 运行时校验查询，编译期不拉任何新依赖。
///
/// ```ignore
/// platform_db::tenant_row!(OrderItem {
///     id: "id",
///     tenant_id: "tenant_id",
///     title: "title",
/// });
/// ```
///
/// 业务模块因此**不需要**直接依赖 `sqlx`。
#[macro_export]
macro_rules! tenant_row {
    ($ty:ty { $($field:ident : $column:literal),* $(,)? }) => {
        impl<'r> $crate::FromRow<'r, $crate::PgRow> for $ty {
            fn from_row(
                row: &'r $crate::PgRow,
            ) -> ::std::result::Result<Self, $crate::SqlxError> {
                ::std::result::Result::Ok(Self {
                    $( $field: $crate::Row::try_get(row, $column)?, )*
                })
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 公开签名里**不允许**出现的类型：拿到它们就等于拿到「不带租户的连接」。
    const FORBIDDEN_TYPES: [&str; 4] = [
        "PgPool",
        "PgConnection",
        "Transaction<",
        "&mut PgConnection",
    ];

    /// 读本 crate 的全部源码文件。
    fn source_files() -> Vec<(String, String)> {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        let entries = std::fs::read_dir(&src_dir).expect("src 目录应可读");
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let content = std::fs::read_to_string(&path).expect("源码文件应可读");
            files.push((name, content));
        }
        files.sort();
        files
    }

    /// 抽出全部公开函数签名（可能跨多行）。
    fn public_signatures(source: &str) -> Vec<String> {
        let mut signatures = Vec::new();
        let mut current: Option<String> = None;
        for line in source.lines() {
            let trimmed = line.trim();
            match current.as_mut() {
                None => {
                    if trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ") {
                        current = Some(trimmed.to_string());
                    }
                }
                Some(acc) => {
                    acc.push(' ');
                    acc.push_str(trimmed);
                }
            }
            let done = current
                .as_ref()
                .is_some_and(|acc| acc.contains('{') || acc.contains(';'));
            if done {
                signatures.push(current.take().expect("刚检查过"));
            }
        }
        if let Some(acc) = current {
            signatures.push(acc);
        }
        signatures
    }

    /// 抽出全部公开字段的类型。
    fn public_field_types(source: &str) -> Vec<String> {
        source
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if !trimmed.starts_with("pub ") || trimmed.starts_with("pub(") {
                    return None;
                }
                let rest = trimmed.trim_start_matches("pub ").trim();
                let (name, kind) = rest.split_once(':')?;
                if name.contains('(') || name.contains(' ') {
                    return None;
                }
                Some(kind.trim().trim_end_matches(',').to_string())
            })
            .collect()
    }

    /// 返回类型片段（`->` 之后、`where` 之前）。
    fn return_type(signature: &str) -> &str {
        let Some((_, after)) = signature.split_once("->") else {
            return "";
        };
        after.split(" where ").next().unwrap_or(after).trim()
    }

    /// 公开 API 表面**没有**「拿裸连接」的方法。
    ///
    /// 这是需求 D4 / 风险 R2 的机械保障：业务模块拿不到裸连接，
    /// 就绕不过租户条件。方法名与返回类型一起查 —— 只查类型会漏掉
    /// 「返回 `impl Deref<Target = PgPool>`」这类绕法。
    #[test]
    fn no_public_api_returns_a_raw_connection() {
        for (name, source) in source_files() {
            for signature in public_signatures(&source) {
                let returned = return_type(&signature);
                for forbidden in FORBIDDEN_TYPES {
                    assert!(
                        !returned.contains(forbidden),
                        "{name} 的公开方法返回了裸连接类型 `{forbidden}`：{signature}"
                    );
                }
            }
            for kind in public_field_types(&source) {
                for forbidden in FORBIDDEN_TYPES {
                    assert!(
                        !kind.contains(forbidden),
                        "{name} 的公开字段暴露了裸连接类型 `{forbidden}`：{kind}"
                    );
                }
            }
        }
    }

    /// 公开方法名里不允许出现「拿连接」的入口。
    #[test]
    fn no_public_api_is_named_after_a_raw_connection() {
        const FORBIDDEN_NAMES: [&str; 5] = ["pool", "connection", "conn", "tx", "transaction"];
        for (name, source) in source_files() {
            for signature in public_signatures(&source) {
                let method = method_name(&signature);
                assert!(
                    !FORBIDDEN_NAMES.contains(&method),
                    "{name} 出现了疑似裸连接入口的方法名 `{method}`：{signature}"
                );
            }
        }
    }

    /// 从签名里取出方法名。
    fn method_name(signature: &str) -> &str {
        let after = signature
            .strip_prefix("pub async fn ")
            .or_else(|| signature.strip_prefix("pub fn "))
            .unwrap_or(signature);
        after
            .split(|ch: char| ch == '(' || ch == '<' || ch.is_whitespace())
            .next()
            .unwrap_or("")
    }

    /// 租户占位符是公开常量，业务模块据此写条件。
    #[test]
    fn tenant_token_is_exported() {
        assert_eq!(TENANT_TOKEN, "{tenant}");
    }
}
