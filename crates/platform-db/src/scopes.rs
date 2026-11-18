//! 租户作用域声明的**收集**（业务模块 manifest → 策略规格）。
//!
//! # 为什么放在这里
//!
//! RLS 策略的**唯一来源**是业务模块 manifest 的 `tenantScoped` 段。
//! 两个地方都要读它：
//!
//! | 调用方 | 场景 |
//! |--------|------|
//! | `cargo xtask tenant-policies` | 开发期：本地生成 / 校验策略 |
//! | `base tenant-policies`（交付二进制） | 部署期：镜像里没有 cargo，策略仍要能生成 |
//!
//! 两边读的是**同一份** manifest —— 所以收集逻辑只有一份实现（这里），
//! 避免「开发期按 A 规则收集、部署期按 B 规则收集」这种漂移。
//!
//! # 目录约定
//!
//! ```text
//! <root>/modules/<module>/manifest.json
//! ```
//!
//! `root` 是工作区根目录（开发期 = 仓库根；镜像内 = `/app`，manifest 拷到 `/app/modules`）。

use std::path::Path;

use anyhow::{Context, Result};
use platform_core::extension::manifest::{Manifest, TenantScopedSpec};

use crate::error::DbResult;
use crate::rls::{IsolationReport, apply_policies, verify_policies};
use crate::tenant::TenantDb;

/// 一个模块声明的租户作用域表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantScope {
    /// 模块 ID（manifest 的 `id` 字段）。
    pub module: String,
    /// 租户作用域声明。
    pub spec: TenantScopedSpec,
}

/// 收集全部模块声明的租户作用域表。
///
/// 没有业务模块、或模块都没声明 `tenantScoped` 时返回**空列表**（不是错误）——
/// 「没有要隔离的表」是合法状态。
pub fn collect_scopes(root: &Path) -> Result<Vec<TenantScope>> {
    let modules_dir = root.join("modules");
    let Ok(entries) = std::fs::read_dir(&modules_dir) else {
        return Ok(Vec::new());
    };
    let mut scopes = Vec::new();
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
        scopes.push(TenantScope {
            module: manifest.id,
            spec,
        });
    }
    Ok(scopes)
}

/// 应用策略（幂等）：按 manifest 的声明生成 / 补齐 RLS 策略。
///
/// ⚠️ **需要表的属主权限**（`ALTER TABLE` / `CREATE POLICY`）——
/// 用管理员角色执行；用应用角色跑会报「must be owner of table」。
pub async fn apply_scopes(db: &TenantDb, scopes: &[TenantScope]) -> DbResult<()> {
    for scope in scopes {
        apply_policies(db, &scope.spec).await?;
    }
    Ok(())
}

/// 校验：每张声明的表都必须**真的有** policy（漏建 = 完全无隔离）。
///
/// 连接角色绕过 RLS **不算失败**（运维通常用管理员角色跑这条检查）——
/// 角色问题由调用方单独告警（见 [`IsolationReport::table_problems`] 与
/// [`IsolationReport::role_bypasses_rls`]）。
pub async fn check_scopes(db: &TenantDb, scopes: &[TenantScope]) -> DbResult<IsolationReport> {
    let specs: Vec<TenantScopedSpec> = scopes.iter().map(|scope| scope.spec.clone()).collect();
    verify_policies(db, &specs).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时目录里造一个最小工作区。
    fn workspace_with(module: &str, manifest: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("platform-db-scopes-{module}"));
        let dir = root.join("modules").join(module);
        std::fs::create_dir_all(&dir).expect("应当能建临时目录");
        std::fs::write(dir.join("manifest.json"), manifest).expect("应当能写 manifest");
        root
    }

    /// 清理临时目录。
    fn cleanup(root: &Path) {
        let _ = std::fs::remove_dir_all(root);
    }

    const WITH_SCOPE: &str = r#"{
      "id": "demo",
      "name": "demo",
      "version": "0.1.0",
      "enabled": true,
      "permissions": [],
      "menus": [],
      "dependencies": [],
      "tenantScoped": { "tables": ["demo_item"], "column": "tenant_id" }
    }"#;

    #[test]
    fn a_module_without_the_directory_yields_no_scope() {
        let root = std::env::temp_dir().join("platform-db-scopes-absent");
        cleanup(&root);
        let scopes = collect_scopes(&root).expect("没有 modules 目录不是错误");
        assert!(scopes.is_empty());
    }

    #[test]
    fn a_manifest_with_tenant_scoped_is_collected() {
        let root = workspace_with("demo", WITH_SCOPE);
        let scopes = collect_scopes(&root).expect("收集应当成功");
        cleanup(&root);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].module, "demo");
        assert_eq!(scopes[0].spec.tables, vec!["demo_item".to_string()]);
        assert_eq!(scopes[0].spec.column, "tenant_id");
    }

    #[test]
    fn a_manifest_without_tenant_scoped_is_skipped() {
        let root = workspace_with(
            "plain",
            r#"{
              "id": "plain",
              "name": "plain",
              "version": "0.1.0",
              "enabled": true,
              "permissions": [],
              "menus": [],
              "dependencies": []
            }"#,
        );
        let scopes = collect_scopes(&root).expect("收集应当成功");
        cleanup(&root);
        assert!(scopes.is_empty());
    }
}
