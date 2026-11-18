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

use std::path::Path;

use anyhow::{Context, Result, bail};
use platform_core::extension::manifest::{Manifest, TenantScopedSpec};
use platform_db::{DbOptions, TenantDb, apply_policies, verify_policies};

/// 一个模块声明的租户作用域表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleScope {
    /// 模块 ID。
    pub module: String,
    /// 租户作用域声明。
    pub spec: TenantScopedSpec,
}

/// 收集全部模块声明的租户作用域表。
pub fn collect_scopes(root: &Path) -> Result<Vec<ModuleScope>> {
    let modules_dir = root.join("modules");
    let mut scopes = Vec::new();
    let Ok(entries) = std::fs::read_dir(&modules_dir) else {
        return Ok(scopes);
    };
    let mut names: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    names.sort();

    for module_dir in names {
        let manifest_path = module_dir.join("manifest.json");
        if !manifest_path.is_file() {
            continue;
        }
        let raw = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("cannot read {}", manifest_path.display()))?;
        let manifest = Manifest::parse_checked(&raw).map_err(|issues| {
            let detail = issues
                .iter()
                .map(|issue| format!("{}: {}", issue.field(), issue.reason()))
                .collect::<Vec<_>>()
                .join("; ");
            anyhow::anyhow!("{} 不符合 Schema：{detail}", manifest_path.display())
        })?;
        let Some(spec) = manifest.tenant_scoped else {
            continue;
        };
        scopes.push(ModuleScope {
            module: manifest.id,
            spec,
        });
    }
    Ok(scopes)
}

/// 应用策略（幂等）：按 manifest 的声明生成 / 补齐 RLS 策略。
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
pub async fn check(scopes: &[ModuleScope], database_url: &str) -> Result<()> {
    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .context("cannot connect to the database")?;
    let specs: Vec<TenantScopedSpec> = scopes.iter().map(|scope| scope.spec.clone()).collect();
    let report = verify_policies(&db, &specs)
        .await
        .context("cannot verify the tenant isolation policies")?;

    let mut problems = report.problems();
    if report.role_bypasses_rls {
        problems.push(format!(
            "the connected role `{}` bypasses RLS: the application must connect with a \
             non-superuser role (see deploy/seed/02-app-role.sql)",
            report.role
        ));
    }
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
    Ok(())
}
