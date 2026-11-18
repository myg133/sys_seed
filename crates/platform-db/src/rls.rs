//! RLS 策略的**统一生成**与校验。
//!
//! # 为什么要平台统一生成
//!
//! 业务模块手写 policy 迟早会漏 —— **漏一张表 = 该表完全无隔离**（不是「少一个条件」，
//! 是「谁都能查」）。所以模块只在 manifest 里声明 `tenantScoped`，平台据此生成：
//!
//! ```sql
//! ALTER TABLE <schema>.<table> ENABLE ROW LEVEL SECURITY;
//! CREATE POLICY <table>_tenant_isolation ON <schema>.<table>
//!   USING (<column> = current_setting('app.tenant_id', true))
//!   WITH CHECK (<column> = current_setting('app.tenant_id', true));
//! ```
//!
//! # 为什么是 `current_setting('app.tenant_id', true)`
//!
//! 第二个参数 `true` = **缺失时返回 NULL 而不是报错**：
//!
//! | 写法 | 应用层漏设租户上下文时 |
//! |------|------------------------|
//! | `current_setting('app.tenant_id')` | **直接报错** —— 所有未设上下文的查询全挂，开发运维阶段很痛苦 |
//! | `current_setting('app.tenant_id', true)` | 返回 NULL → `tenant_id = NULL` → 行不可见（**fail closed**） |
//!
//! fail closed 更符合安全语义：**宁可看不到数据，也不要看到别人的数据**。
//!
//! `WITH CHECK` 同样重要：没有它，应用层可以往表里写入别的租户的行
//! （写得进去、读不出来 —— 静默的「写侧泄漏」）。

use platform_core::extension::manifest::{TenantScopedSpec, is_sql_identifier};
use sqlx::Row;

use crate::error::{DbError, DbResult};
use crate::tenant::TenantDb;

/// policy 名字后缀。
const POLICY_SUFFIX: &str = "_tenant_isolation";

/// 应用角色组名：持有租户作用域表的权限（成员是实际的应用连接角色）。
///
/// 详见 `deploy/seed/02-app-role.sql`：应用连接**必须**用组里的普通角色，
/// 超级用户会绕过 RLS（租户隔离 fail open）。
pub const APP_ROLE_GROUP: &str = "base_platform_app";

/// 一张租户作用域表的隔离状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableIsolation {
    /// 表名（不含 schema）。
    pub table: String,
    /// 是否已 `ENABLE ROW LEVEL SECURITY`。
    pub rls_enabled: bool,
    /// 该表上的 policy 数量。
    pub policy_count: i64,
}

/// 隔离自检报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationReport {
    /// 当前连接角色。
    pub role: String,
    /// 该角色是否绕过 RLS（超级用户或 `BYPASSRLS`）。
    pub role_bypasses_rls: bool,
    /// 各张表的隔离状态。
    pub tables: Vec<TableIsolation>,
}

impl IsolationReport {
    /// 报告是否全部合规。
    ///
    /// 合规 = 连接角色**不**绕过 RLS，且每张声明的表都开了 RLS 且至少有一条 policy。
    pub fn is_ok(&self) -> bool {
        !self.role_bypasses_rls
            && self
                .tables
                .iter()
                .all(|table| table.rls_enabled && table.policy_count > 0)
    }

    /// 不合规项的说明（面向开发者）。
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.role_bypasses_rls {
            problems.push(format!(
                "the connected role `{}` bypasses RLS (superuser or BYPASSRLS): tenant isolation would fail open",
                self.role
            ));
        }
        problems.extend(self.table_problems());
        problems
    }

    /// **表级**不合规项的说明。
    ///
    /// 与 [`Self::problems`] 的区别：**不含**角色问题 ——
    /// 运维用管理员角色跑校验是常态，角色问题应当单独告警（而不是让整条校验失败）。
    pub fn table_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for table in &self.tables {
            if !table.rls_enabled {
                problems.push(format!(
                    "table `{}` does not have ROW LEVEL SECURITY enabled",
                    table.table
                ));
            }
            if table.policy_count == 0 {
                problems.push(format!("table `{}` has no isolation policy", table.table));
            }
        }
        problems
    }
}

/// 为一份租户作用域声明生成 RLS 策略语句。
///
/// 标识符会被拼进 DDL，所以先校验（表名 / 列名必须是 `^[a-z][a-z0-9_]*$`）。
pub fn policy_statements(spec: &TenantScopedSpec, schema: &str) -> DbResult<Vec<String>> {
    if !is_sql_identifier(schema) {
        return Err(DbError::InvalidQuery {
            reason: format!("the schema name `{schema}` is not a safe SQL identifier"),
        });
    }
    if !is_sql_identifier(&spec.column) {
        return Err(DbError::InvalidQuery {
            reason: format!(
                "the tenant column `{}` is not a safe SQL identifier",
                spec.column
            ),
        });
    }

    let mut statements = Vec::new();
    for table in &spec.tables {
        if !is_sql_identifier(table) {
            return Err(DbError::InvalidQuery {
                reason: format!("the table name `{table}` is not a safe SQL identifier"),
            });
        }
        let qualified = format!("{schema}.{table}");
        let policy = format!("{table}{POLICY_SUFFIX}");
        statements.push(format!("ALTER TABLE {qualified} ENABLE ROW LEVEL SECURITY"));
        statements.push(format!("DROP POLICY IF EXISTS {policy} ON {qualified}"));
        statements.push(format!(
            "CREATE POLICY {policy} ON {qualified} \
             USING ({column} = current_setting('app.tenant_id', true)) \
             WITH CHECK ({column} = current_setting('app.tenant_id', true))",
            column = spec.column
        ));
        statements.push(grant_statements(&qualified, APP_ROLE_GROUP)?);
    }
    Ok(statements)
}

/// 授权语句：把表的权限授予应用角色组（**组不存在时跳过** —— 角色由部署流程创建）。
fn grant_statements(qualified: &str, role_group: &str) -> DbResult<String> {
    if !is_sql_identifier(role_group) {
        return Err(DbError::InvalidQuery {
            reason: format!("the role group name `{role_group}` is not a safe SQL identifier"),
        });
    }
    Ok(format!(
        "DO $$ BEGIN \
           IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{role_group}') THEN \
             EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON {qualified} TO {role_group}'; \
           END IF; \
         END $$"
    ))
}

/// 为一份租户作用域声明**应用** RLS 策略（幂等）。
///
/// 这是**平台内部**的迁移期动作：它跑在裸连接上（DDL 不带租户条件）。
pub async fn apply_policies(db: &TenantDb, spec: &TenantScopedSpec) -> DbResult<()> {
    // **整段在一个事务里**：`DROP POLICY` + `CREATE POLICY` 分成两条语句执行时，
    // 中间存在一个「表上没有 policy」的窗口 —— 那一瞬间 RLS 变成**全部拒绝**
    // （fail closed，不泄漏，但读写全挂）。并发访问时这个窗口会让查询看到
    // 「表是空的」这种假象。放进同一个事务就没有这个窗口。
    let statements = policy_statements(spec, db.schema())?;
    let mut tx = db.pool.begin().await?;
    for statement in statements {
        sqlx::query(&statement).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// 校验：连接角色会不会绕过 RLS + 每张声明的表是否真的有 policy。
pub async fn verify_policies(
    db: &TenantDb,
    specs: &[TenantScopedSpec],
) -> DbResult<IsolationReport> {
    let role_row = sqlx::query(
        "SELECT rolname, (rolsuper OR rolbypassrls) AS bypasses FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&db.pool)
    .await?;
    let role: String = role_row.try_get("rolname")?;
    let role_bypasses_rls: bool = role_row.try_get("bypasses")?;

    let mut tables = Vec::new();
    for spec in specs {
        for table in &spec.tables {
            let row = sqlx::query(
                "SELECT c.relrowsecurity AS rls_enabled, \
                        (SELECT count(*) FROM pg_policy p \
                           JOIN pg_class pc ON pc.oid = p.polrelid \
                           JOIN pg_namespace pn ON pn.oid = pc.relnamespace \
                          WHERE pn.nspname = $1 AND pc.relname = $2) AS policies \
                   FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                  WHERE n.nspname = $1 AND c.relname = $2",
            )
            .bind(db.schema())
            .bind(table)
            .fetch_optional(&db.pool)
            .await?;
            tables.push(match row {
                Some(row) => TableIsolation {
                    table: table.clone(),
                    rls_enabled: row.try_get("rls_enabled")?,
                    policy_count: row.try_get("policies")?,
                },
                None => TableIsolation {
                    table: table.clone(),
                    rls_enabled: false,
                    policy_count: 0,
                },
            });
        }
    }

    Ok(IsolationReport {
        role,
        role_bypasses_rls,
        tables,
    })
}

/// 严格校验：不满足隔离前提就报错（fail fast）。
pub async fn require_isolation(
    db: &TenantDb,
    specs: &[TenantScopedSpec],
) -> DbResult<IsolationReport> {
    let report = verify_policies(db, specs).await?;
    if report.role_bypasses_rls {
        return Err(DbError::RlsBypassed {
            role: report.role.clone(),
        });
    }
    for table in &report.tables {
        if !table.rls_enabled || table.policy_count == 0 {
            return Err(DbError::MissingPolicy {
                table: table.table.clone(),
            });
        }
    }
    Ok(report)
}

/// 平台内部自检：**不设租户上下文**直查租户作用域表，返回可见行数。
///
/// 按 RLS 的 fail closed 语义，它必须返回 **0**：
/// 即使应用层完全失守（漏掉租户条件、漏掉 `SET LOCAL`），数据库层也不该泄漏任何行。
///
/// **它不是数据访问入口** —— 业务模块不需要它，也拿不到任何行数据；
/// 它的存在只是让「RLS fail closed」这条断言可测。
pub async fn unscoped_row_count(db: &TenantDb, table: &str) -> DbResult<i64> {
    let qualified = db.table(table);
    let row = sqlx::query(&format!("SELECT count(*) AS rows FROM {qualified}"))
        .fetch_one(&db.pool)
        .await?;
    Ok(row.try_get("rows")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::extension::manifest::TenantScopedSpec;

    fn spec() -> TenantScopedSpec {
        TenantScopedSpec {
            tables: vec!["order_item".to_string()],
            column: "tenant_id".to_string(),
        }
    }

    #[test]
    fn policy_statements_enable_rls_and_use_the_fail_closed_setting() {
        let statements = policy_statements(&spec(), "public").expect("应当能生成");
        assert_eq!(statements.len(), 4, "statements = {statements:?}");
        assert_eq!(
            statements[0],
            "ALTER TABLE public.order_item ENABLE ROW LEVEL SECURITY"
        );
        assert_eq!(
            statements[1],
            "DROP POLICY IF EXISTS order_item_tenant_isolation ON public.order_item"
        );
        assert!(
            statements[2].contains("current_setting('app.tenant_id', true)"),
            "policy 必须用 fail closed 的 current_setting：{}",
            statements[2]
        );
        assert!(
            statements[2].contains("WITH CHECK"),
            "policy 必须带 WITH CHECK（否则可以写入他租的行）：{}",
            statements[2]
        );
        assert!(
            statements[3].contains(APP_ROLE_GROUP),
            "必须把表的权限授予应用角色组：{}",
            statements[3]
        );
    }

    #[test]
    fn schema_comes_from_the_argument_not_from_a_hardcoded_public() {
        let statements = policy_statements(&spec(), "tenant_a").expect("应当能生成");
        assert!(statements[0].contains("tenant_a.order_item"));
    }

    #[test]
    fn unsafe_identifiers_are_rejected() {
        let bad = TenantScopedSpec {
            tables: vec!["order item; DROP TABLE users".to_string()],
            column: "tenant_id".to_string(),
        };
        let error = policy_statements(&bad, "public").expect_err("非法表名必须被拒绝");
        assert_eq!(error.code(), "INVALID_QUERY");

        let bad_column = TenantScopedSpec {
            tables: vec!["order_item".to_string()],
            column: "tenant-id".to_string(),
        };
        let error = policy_statements(&bad_column, "public").expect_err("非法列名必须被拒绝");
        assert_eq!(error.code(), "INVALID_QUERY");
    }

    #[test]
    fn report_problems_name_the_offending_table() {
        let report = IsolationReport {
            role: "base".to_string(),
            role_bypasses_rls: true,
            tables: vec![TableIsolation {
                table: "order_item".to_string(),
                rls_enabled: false,
                policy_count: 0,
            }],
        };
        assert!(!report.is_ok());
        let problems = report.problems();
        assert_eq!(problems.len(), 3, "problems = {problems:?}");
        assert!(problems[0].contains("bypasses RLS"));
        assert!(problems[1].contains("order_item"));
    }
}
