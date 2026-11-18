//! 权限点注册表的**双向对账**（design-summary §2.1）。
//!
//! # 为什么必须「启动失败」而不是「告警」
//!
//! 告警 = 静默放行的一种。权限点缺失时，角色授了也判不出来，
//! 表现为「配了权限却不生效」—— 排查成本极高，且容易被误判成业务 bug。
//!
//! | 情况 | 判定 |
//! |------|------|
//! | manifest 有、库里没有 | **启动失败**（退出码非 0），逐条列出缺失编码与来源 manifest |
//! | 库里有、manifest 没有 | **启动失败**，逐条列出多余编码 |
//! | 两边一致 | 正常启动 |
//!
//! # 对账的两侧
//!
//! | 侧 | 来源 |
//! |----|------|
//! | manifest 侧 | `<root>/modules/*/manifest.json` 与 `<root>/crates/*/manifest.json`（**逐个读取**） |
//! | 库侧 | `platform_permission` 表 |
//!
//! manifest 侧直接读 manifest（不是读生成物）——
//! 生成物 `capabilities/permissions.json` 是**开发期**的产物，运行时未必存在；
//! 权威来源永远是 manifest 本身。
//!
//! # 库里的权限点由**迁移**写入
//!
//! 不是运行时自动改库 —— 运行时自动改库会让对账失去意义。

use std::collections::BTreeMap;
use std::path::Path;

use platform_core::extension::manifest::Manifest;

use crate::error::RbacError;
use crate::store::RbacStore;

/// 一条 manifest 侧登记的权限点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestPermission {
    /// 权限点编码。
    pub code: String,
    /// 权限点名称。
    pub name: String,
    /// 属性级校验条件（可空）。
    pub condition: Option<String>,
    /// 来源 manifest 的路径（**逐条列出**时用）。
    pub source: String,
}

/// 对账结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reconciliation {
    /// manifest 有、库里没有的权限点（含来源 manifest 路径）。
    pub missing_in_database: Vec<ManifestPermission>,
    /// 库里有、manifest 没有的权限点。
    pub extra_in_database: Vec<String>,
    /// 条件不一致的权限点（编码）。
    pub condition_mismatches: Vec<String>,
}

impl Reconciliation {
    /// 是否一致。
    pub fn is_consistent(&self) -> bool {
        self.missing_in_database.is_empty()
            && self.extra_in_database.is_empty()
            && self.condition_mismatches.is_empty()
    }

    /// 逐条列出差异（**启动失败时打印**）。
    pub fn differences(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for entry in &self.missing_in_database {
            lines.push(format!(
                "the permission `{}` is declared in {} but missing from the database",
                entry.code, entry.source
            ));
        }
        for code in &self.extra_in_database {
            lines.push(format!(
                "the permission `{code}` exists in the database but is not declared in any manifest"
            ));
        }
        for code in &self.condition_mismatches {
            lines.push(format!(
                "the condition of the permission `{code}` differs between the manifest and the database"
            ));
        }
        lines
    }
}

/// 扫描 manifest 根目录，收集全部已登记的权限点。
///
/// 扫两处（与 `platform-db::collect_scopes` 的口径一致）：
///
/// | 目录 | 谁 |
/// |------|-----|
/// | `modules/<模块>/manifest.json` | 业务模块 |
/// | `crates/<crate>/manifest.json` | 平台 crate（如 `platform-org`、`platform-rbac`） |
pub fn collect_manifest_permissions(root: &Path) -> Result<Vec<ManifestPermission>, RbacError> {
    let mut entries = Vec::new();
    for (dir_name, manifest_root) in [
        ("modules", root.join("modules")),
        ("crates", root.join("crates")),
    ] {
        let Ok(dir_entries) = std::fs::read_dir(&manifest_root) else {
            continue;
        };
        let mut names: Vec<String> = dir_entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default()
            })
            .collect();
        names.sort();

        for name in names {
            let manifest_path = manifest_root.join(&name).join("manifest.json");
            if !manifest_path.is_file() {
                continue;
            }
            let raw =
                std::fs::read_to_string(&manifest_path).map_err(|error| RbacError::Internal {
                    reason: format!("{} cannot be read: {error}", manifest_path.display()),
                })?;
            let manifest = Manifest::parse_checked(&raw).map_err(|issues| {
                let detail = issues
                    .iter()
                    .map(|issue| format!("{}: {}", issue.field(), issue.reason()))
                    .collect::<Vec<_>>()
                    .join("; ");
                RbacError::Internal {
                    reason: format!("{} is invalid: {detail}", manifest_path.display()),
                }
            })?;
            let source = format!("{dir_name}/{name}/manifest.json");
            for permission in &manifest.permissions {
                entries.push(ManifestPermission {
                    code: permission.code.clone(),
                    name: permission.name.clone(),
                    condition: permission.condition.clone(),
                    source: source.clone(),
                });
            }
        }
    }
    entries.sort_by(|left, right| left.code.cmp(&right.code));
    Ok(entries)
}

/// 双向对账：manifest 侧 ↔ 数据库侧。
pub async fn reconcile(
    store: &RbacStore,
    manifest_permissions: &[ManifestPermission],
) -> Result<Reconciliation, RbacError> {
    let rows = store
        .list_permissions()
        .await
        .map_err(|error| RbacError::Internal {
            reason: format!("the permission registry could not be read: {error}"),
        })?;

    let mut database: BTreeMap<String, Option<String>> = BTreeMap::new();
    for row in rows {
        database.insert(row.code, row.condition);
    }

    let mut report = Reconciliation::default();
    let mut seen: BTreeMap<String, &ManifestPermission> = BTreeMap::new();
    for entry in manifest_permissions {
        seen.insert(entry.code.clone(), entry);
        match database.get(&entry.code) {
            None => report.missing_in_database.push(entry.clone()),
            Some(condition) => {
                let expected = entry.condition.as_deref().map(str::trim);
                let actual = condition.as_deref().map(str::trim);
                if expected != actual {
                    report.condition_mismatches.push(entry.code.clone());
                }
            }
        }
    }

    for code in database.keys() {
        if !seen.contains_key(code) {
            report.extra_in_database.push(code.clone());
        }
    }

    Ok(report)
}

/// 对账并把不一致变成错误（**启动失败**用）。
pub async fn require_consistent(
    store: &RbacStore,
    manifest_permissions: &[ManifestPermission],
) -> Result<(), RbacError> {
    let report = reconcile(store, manifest_permissions).await?;
    if report.is_consistent() {
        return Ok(());
    }
    Err(RbacError::RegistryMismatch {
        differences: report.differences(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(code: &str, source: &str) -> ManifestPermission {
        ManifestPermission {
            code: code.to_string(),
            name: code.to_string(),
            condition: None,
            source: source.to_string(),
        }
    }

    #[test]
    fn a_consistent_registry_has_no_differences() {
        let report = Reconciliation::default();
        assert!(report.is_consistent());
        assert!(report.differences().is_empty());
    }

    #[test]
    fn a_missing_permission_names_its_source_manifest() {
        let report = Reconciliation {
            missing_in_database: vec![entry("sandbox:demo:read", "modules/sandbox/manifest.json")],
            ..Reconciliation::default()
        };
        assert!(!report.is_consistent());
        let differences = report.differences();
        assert_eq!(differences.len(), 1);
        assert!(differences[0].contains("sandbox:demo:read"));
        assert!(
            differences[0].contains("modules/sandbox/manifest.json"),
            "差异必须带来源 manifest 路径：{}",
            differences[0]
        );
    }

    #[test]
    fn an_extra_permission_is_listed() {
        let report = Reconciliation {
            extra_in_database: vec!["ghost:perm:read".to_string()],
            ..Reconciliation::default()
        };
        let differences = report.differences();
        assert_eq!(differences.len(), 1);
        assert!(differences[0].contains("ghost:perm:read"));
    }

    #[test]
    fn a_condition_mismatch_is_reported() {
        let report = Reconciliation {
            condition_mismatches: vec!["sandbox:scope-item:approve".to_string()],
            ..Reconciliation::default()
        };
        assert!(!report.is_consistent());
        assert!(report.differences()[0].contains("condition"));
    }
}
