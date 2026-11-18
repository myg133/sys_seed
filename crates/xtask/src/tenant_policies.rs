//! 租户作用域策略的**统一生成**与校验（REQ-006）。
//!
//! 业务模块在 manifest 的 `tenantScoped` 段声明自己的租户作用域表，
//! **平台统一生成** RLS 策略 —— 模块不手写。
//!
//! 理由：手写迟早会漏 —— **漏一张表 = 该表完全无隔离**（不是「少一个条件」，
//! 是「谁都能查」）。
//!
//! ```bash
//! DATABASE_URL=postgres://... cargo xtask tenant-policies          # 生成 / 补齐
//! DATABASE_URL=postgres://... cargo xtask tenant-policies --check  # 只校验
//! ```

use anyhow::{Context, Result, bail};
use platform_db::{DbOptions, TenantDb, apply_policies, check_scopes};

/// 一个模块声明的租户作用域表。
///
/// 收集逻辑在 `platform-db`（`collect_scopes`）—— 交付二进制 `base tenant-policies`
/// 读的是**同一份** manifest，两边规则不能漂移。
pub type ModuleScope = platform_db::TenantScope;

/// 收集全部模块声明的租户作用域表。
pub use platform_db::collect_scopes;

/// 应用策略（幂等）：按 manifest 的声明生成 / 补齐 RLS 策略。
///
/// ⚠️ **需要表的属主权限**（`ALTER TABLE` / `CREATE POLICY`）——
/// 用管理员角色执行；用应用角色跑会报「must be owner of table」。
pub async fn apply(scopes: &[ModuleScope], database_url: &str) -> Result<()> {
    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .context("cannot connect to the database")?;
    for scope in scopes {
        apply_policies(&db, &scope.spec)
            .await
            .with_context(|| format!("cannot apply the policies of module `{}`", scope.module))?;
    }
    Ok(())
}

/// 校验：每张 `tenantScoped` 声明的表都必须**真的有** policy（漏建 = 完全无隔离）。
///
/// 连接角色绕过 RLS（超级用户 / `BYPASSRLS`）时**只警告不失败**：
/// 运维通常用管理员角色跑这条检查，而「应用连接用的是什么角色」是另一件事
/// （用应用角色跑一次 `require_isolation` 才是它的判据，见 `platform-db`）。
pub async fn check(scopes: &[ModuleScope], database_url: &str) -> Result<()> {
    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .context("cannot connect to the database")?;
    let report = check_scopes(&db, scopes)
        .await
        .context("cannot verify the tenant isolation policies")?;

    let problems = report.table_problems();
    if !problems.is_empty() {
        bail!(
            "租户隔离前提不成立：\n{}",
            problems
                .iter()
                .map(|problem| format!("  - {problem}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    if report.role_bypasses_rls {
        println!(
            "警告：当前连接角色 `{}` 绕过 RLS（超级用户或 BYPASSRLS）。\
             如果**应用**也用这个角色连接，租户隔离会 fail open —— \
             请用应用角色（组 base_platform_app 的成员）再自检一次。",
            report.role
        );
    }
    Ok(())
}
