//! 实体元数据的**全局注册表**与阶段 2 校验。
//!
//! # 为什么需要注册表
//!
//! 元数据接口要回答「模块 `sandbox` 的实体 `demoItem` 长什么样」——
//! 这需要把**全部** `manifest.json` 读进来建索引，而不是逐文件解析。
//!
//! # 阶段 2（跨文件）校验
//!
//! 单文件校验（[`EntitySchema::validate`]）判不出这两条：
//!
//! | 规则 | 为什么单文件判不了 |
//! |------|--------------------|
//! | 字段的 `permission` 必须已登记 | 权限点可能由**别的** manifest 声明 |
//! | `reference.module/entity` 必须存在 | 目标实体在**别的** manifest 里 |
//!
//! 所以它们只能在**所有** manifest 加载完后判。失败 → 进程起不来（与 D27 同源：
//! 跨文件一致性只能在所有文件都加载后判）。
//!
//! # 未启用的模块
//!
//! `enabled: false` 的模块**不进入**注册表 —— 它的实体对外表现为
//! `SCHEMA_MODULE_NOT_FOUND`（模块没加载，就不该有元数据）。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use platform_core::extension::manifest::{Manifest, ManifestError};
use platform_core::extension::schema::EntitySchema;

/// 一处加载 / 校验问题：**哪份 manifest 的哪个字段**出了什么问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestIssue {
    /// `modules/sandbox/manifest.json` 形式的来源路径。
    pub source: String,
    /// 字段路径（如 `schema.demoItem.fields[4].type`）。
    pub field: String,
    /// 面向开发者的英文原因。
    pub reason: String,
}

impl ManifestIssue {
    /// 从 `ManifestError` 造一条，附上 manifest 来源。
    fn from_error(source: &str, error: &ManifestError) -> Self {
        Self {
            source: source.to_string(),
            field: error.field().to_string(),
            reason: error.reason().to_string(),
        }
    }

    /// 新造一条（阶段 2 用）。
    fn new(source: &str, field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            source: source.to_string(),
            field: field.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ManifestIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.source, self.field, self.reason)
    }
}

/// 元数据注册表加载失败：**全部**问题（不是只报第一个）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaLoadError {
    /// 逐条问题（阶段 1 与阶段 2 汇总）。
    pub issues: Vec<ManifestIssue>,
}

impl SchemaLoadError {
    /// 一条问题的错误。
    pub fn single(issue: ManifestIssue) -> Self {
        Self {
            issues: vec![issue],
        }
    }
}

impl fmt::Display for SchemaLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} entity metadata problem(s) (each line is `<manifest>: <field>: <reason>`)",
            self.issues.len()
        )?;
        for issue in &self.issues {
            write!(f, "\n  - {issue}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SchemaLoadError {}

/// 一个实体在注册表里的位置（模块 + 实体键）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityLocation {
    /// 模块 ID。
    pub module: String,
    /// 实体键。
    pub entity: String,
}

/// 实体元数据注册表：`模块 → 实体 → 元数据`。
#[derive(Debug, Clone, Default)]
pub struct SchemaRegistry {
    entities: BTreeMap<String, BTreeMap<String, EntitySchema>>,
    /// 已加载的模块 → manifest 来源（错误信息用）。
    sources: BTreeMap<String, String>,
}

impl SchemaRegistry {
    /// 从 manifest 根目录构建注册表（**阶段 1 + 阶段 2** 全过才成功）。
    ///
    /// 扫 `modules/*/manifest.json` 与 `crates/*/manifest.json`
    /// （与 `platform-config` / `platform-rbac` / `platform-db` 的口径一致）。
    pub fn from_manifest_root(root: &Path) -> Result<Self, SchemaLoadError> {
        let loaded = load_manifests(root)?;
        Self::from_manifests(loaded)
    }

    /// 用已解析的 manifest 构建注册表（测试与嵌入场景用）。
    ///
    /// 入参是 `(来源, manifest)`；`enabled: false` 的模块会被忽略。
    pub fn from_manifests(
        loaded: Vec<(String, Manifest)>,
    ) -> Result<Self, SchemaLoadError> {
        let mut registry = Self::default();
        let mut issues = Vec::new();

        // 1) 建索引（只收启用的模块）。
        for (source, manifest) in &loaded {
            if !manifest.enabled {
                continue;
            }
            registry.sources.insert(manifest.id.clone(), source.clone());
            let Some(schemas) = &manifest.schema else {
                continue;
            };
            let slot = registry
                .entities
                .entry(manifest.id.clone())
                .or_default();
            for (entity, spec) in schemas {
                slot.insert(entity.clone(), spec.clone());
            }
        }

        // 2) 阶段 2：跨文件一致性。
        let codes: BTreeSet<&str> = loaded
            .iter()
            .filter(|(_, manifest)| manifest.enabled)
            .flat_map(|(_, manifest)| manifest.permission_codes())
            .collect();

        for (source, manifest) in &loaded {
            if !manifest.enabled {
                continue;
            }
            let Some(schemas) = &manifest.schema else {
                continue;
            };
            for (entity, spec) in schemas {
                for (index, field) in spec.fields.iter().enumerate() {
                    if let Some(permission) = field.permission() {
                        if !codes.contains(permission) {
                            issues.push(ManifestIssue::new(
                                source,
                                format!("schema.{entity}.fields[{index}].permission"),
                                format!(
                                    "the permission point `{permission}` is not declared in any manifest"
                                ),
                            ));
                        }
                    }
                    let Some(reference) = &field.reference else {
                        continue;
                    };
                    if !registry.sources.contains_key(&reference.module) {
                        issues.push(ManifestIssue::new(
                            source,
                            format!("schema.{entity}.fields[{index}].reference.module"),
                            format!(
                                "the referenced module `{}` does not exist (or is disabled)",
                                reference.module
                            ),
                        ));
                        continue;
                    }
                    let exists = registry
                        .entities
                        .get(&reference.module)
                        .is_some_and(|entities| entities.contains_key(&reference.entity));
                    if !exists {
                        issues.push(ManifestIssue::new(
                            source,
                            format!("schema.{entity}.fields[{index}].reference.entity"),
                            format!(
                                "the reference points to `{}/{}`, but module `{}` declares no entity `{}`",
                                reference.module, reference.entity, reference.module, reference.entity
                            ),
                        ));
                    }
                }
            }
        }

        if issues.is_empty() {
            Ok(registry)
        } else {
            Err(SchemaLoadError { issues })
        }
    }

    /// 取一个实体的元数据。
    pub fn entity(&self, module: &str, entity: &str) -> Option<&EntitySchema> {
        self.entities.get(module)?.get(entity)
    }

    /// 模块是否已加载（**有 manifest 就存在**，与是否声明 schema 段无关）。
    pub fn has_module(&self, module: &str) -> bool {
        self.sources.contains_key(module)
    }

    /// 已加载的模块 ID（排序）。
    pub fn module_ids(&self) -> Vec<&str> {
        self.sources.keys().map(String::as_str).collect()
    }

    /// 已加载的实体（排序）。
    pub fn entities(&self) -> Vec<EntityLocation> {
        self.entities
            .iter()
            .flat_map(|(module, entities)| {
                entities.keys().map(|entity| EntityLocation {
                    module: module.clone(),
                    entity: entity.clone(),
                })
            })
            .collect()
    }

    /// 实体总数（启动日志用）。
    pub fn entity_count(&self) -> usize {
        self.entities.values().map(BTreeMap::len).sum()
    }

    /// 所有声明了字段级权限的字段：`(模块, 实体, 字段键, 权限点)`。
    ///
    /// 启动日志据此说明「哪些字段会被过滤」—— 元数据生效与否要能在日志里看到。
    pub fn permission_fields(&self) -> Vec<(String, String, String, String)> {
        self.entities
            .iter()
            .flat_map(|(module, entities)| {
                entities.iter().flat_map(move |(entity, spec)| {
                    spec.permission_fields().into_iter().filter_map(|field| {
                        field.permission().map(|permission| {
                            (
                                module.clone(),
                                entity.clone(),
                                field.key.clone(),
                                permission.to_string(),
                            )
                        })
                    })
                })
            })
            .collect()
    }
}

/// 读入全部 manifest（阶段 1 校验也在这里）。
fn load_manifests(root: &Path) -> Result<Vec<(String, Manifest)>, SchemaLoadError> {
    let mut loaded = Vec::new();
    let mut issues = Vec::new();

    for (dir_name, manifest_root) in [
        ("modules", root.join("modules")),
        ("crates", root.join("crates")),
    ] {
        let Ok(entries) = std::fs::read_dir(&manifest_root) else {
            continue;
        };
        let mut names: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        names.sort();

        for dir in names {
            let manifest_path = dir.join("manifest.json");
            if !manifest_path.is_file() {
                continue;
            }
            let Some(name) = dir.file_name().map(|name| name.to_string_lossy().to_string()) else {
                continue;
            };
            let source = format!("{dir_name}/{name}/manifest.json");
            let raw = match std::fs::read_to_string(&manifest_path) {
                Ok(raw) => raw,
                Err(error) => {
                    issues.push(ManifestIssue::new(
                        &source,
                        "",
                        format!("cannot be read: {error}"),
                    ));
                    continue;
                }
            };
            match Manifest::parse_checked(&raw) {
                Ok(manifest) => loaded.push((source, manifest)),
                Err(errors) => {
                    issues.extend(
                        errors
                            .iter()
                            .map(|error| ManifestIssue::from_error(&source, error)),
                    );
                }
            }
        }
    }

    if issues.is_empty() {
        Ok(loaded)
    } else {
        Err(SchemaLoadError { issues })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 仓库根（`crates/platform-schema` 往上两级）。
    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crates/platform-schema 应有两级父目录")
            .to_path_buf()
    }

    fn manifest_with_schema(entity: &str, fields: &str, permissions: &str) -> String {
        format!(
            r#"{{
              "id": "sandbox",
              "name": "Sandbox",
              "version": "0.1.0",
              "permissions": [{permissions}],
              "menus": [],
              "dependencies": [],
              "schema": {{
                "{entity}": {{
                  "label": "示例条目",
                  "api": {{
                    "list": "/api/sandbox/demo-items",
                    "create": "/api/sandbox/demo-items"
                  }},
                  "fields": [{fields}]
                }}
              }}
            }}"#
        )
    }

    fn parse(raw: &str) -> Manifest {
        Manifest::parse_checked(raw).expect("测试夹具的 manifest 应能通过阶段 1")
    }

    /// 仓库里的真实 manifest 必须能建注册表（阶段 1 + 阶段 2 全过）。
    #[test]
    fn the_repository_manifests_build_a_registry() {
        let registry =
            SchemaRegistry::from_manifest_root(&repo_root()).expect("仓库 manifest 应能建注册表");
        assert!(registry.has_module("sandbox"), "sandbox 应在注册表里");
        assert!(registry.has_module("org"));
        let entity = registry
            .entity("sandbox", "demoItem")
            .expect("sandbox 应声明 demoItem");
        assert_eq!(entity.label, "示例条目");
    }

    #[test]
    fn an_unknown_module_is_not_found() {
        let registry = SchemaRegistry::from_manifest_root(&repo_root()).expect("应能建注册表");
        assert!(!registry.has_module("not-a-module"));
        assert!(registry.entity("not-a-module", "demoItem").is_none());
        assert!(registry.entity("sandbox", "notAnEntity").is_none());
    }

    /// 阶段 2：字段权限点未登记 → 点名**字段路径 + 权限点**。
    #[test]
    fn an_undeclared_field_permission_is_reported() {
        let raw = manifest_with_schema(
            "demoItem",
            r#"{ "key": "secret", "label": "内部备注", "type": "text", "permission": "sandbox:demo:secret" }"#,
            r#"{ "code": "sandbox:demo:read", "name": "读", "type": "menu" }"#,
        );
        let error = SchemaRegistry::from_manifests(vec![("modules/sandbox/manifest.json".into(), parse(&raw))])
            .expect_err("未登记的权限点必须让注册表构建失败");
        assert_eq!(error.issues.len(), 1, "issues = {:?}", error.issues);
        let issue = &error.issues[0];
        assert_eq!(issue.field, "schema.demoItem.fields[0].permission");
        assert!(
            issue.reason.contains("sandbox:demo:secret"),
            "reason = {}",
            issue.reason
        );
        assert!(
            issue.reason.contains("not declared"),
            "reason = {}",
            issue.reason
        );
    }

    /// 阶段 2：`reference` 指向不存在的实体 → 点名 module 与 entity。
    #[test]
    fn a_reference_to_a_missing_entity_is_reported() {
        let raw = manifest_with_schema(
            "demoItem",
            r#"{ "key": "ownerId", "label": "负责人", "type": "reference", "reference": { "module": "org", "entity": "notAnEntity" } }"#,
            r#"{ "code": "sandbox:demo:read", "name": "读", "type": "menu" }"#,
        );
        let error = SchemaRegistry::from_manifests(vec![
            ("modules/sandbox/manifest.json".into(), parse(&raw)),
            ("crates/platform-org/manifest.json".into(), org_manifest()),
        ])
        .expect_err("reference 指向不存在的实体必须失败");
        assert_eq!(error.issues.len(), 1, "issues = {:?}", error.issues);
        let issue = &error.issues[0];
        assert_eq!(issue.field, "schema.demoItem.fields[0].reference.entity");
        assert!(issue.reason.contains("org"), "reason = {}", issue.reason);
        assert!(
            issue.reason.contains("notAnEntity"),
            "reason = {}",
            issue.reason
        );
    }

    /// 阶段 2：`reference` 指向不存在的**模块** → 报在 `.reference.module`。
    #[test]
    fn a_reference_to_a_missing_module_is_reported() {
        let raw = manifest_with_schema(
            "demoItem",
            r#"{ "key": "ownerId", "label": "负责人", "type": "reference", "reference": { "module": "nowhere", "entity": "user" } }"#,
            r#"{ "code": "sandbox:demo:read", "name": "读", "type": "menu" }"#,
        );
        let error = SchemaRegistry::from_manifests(vec![("modules/sandbox/manifest.json".into(), parse(&raw))])
            .expect_err("reference 指向不存在的模块必须失败");
        assert_eq!(error.issues[0].field, "schema.demoItem.fields[0].reference.module");
        assert!(error.issues[0].reason.contains("nowhere"));
    }

    /// 未启用模块的实体不进入注册表（对外表现为模块不存在）。
    #[test]
    fn a_disabled_module_is_not_registered() {
        let raw = manifest_with_schema(
            "demoItem",
            r#"{ "key": "title", "label": "标题", "type": "text" }"#,
            r#"{ "code": "sandbox:demo:read", "name": "读", "type": "menu" }"#,
        )
        .replace("\"version\": \"0.1.0\",", "\"version\": \"0.1.0\", \"enabled\": false,");
        let registry = SchemaRegistry::from_manifests(vec![(
            "modules/sandbox/manifest.json".into(),
            parse(&raw),
        )])
        .expect("未启用的模块不该让注册表构建失败");
        assert!(!registry.has_module("sandbox"));
        assert_eq!(registry.entity_count(), 0);
    }

    /// 阶段 1 的违规在**读盘**阶段就让注册表构建失败（进程起不来）。
    ///
    /// 这里用临时目录造一份坏 manifest：`scripts/test-integration.sh` 会重置测试库，
    /// 但**不该**为了这条用例往仓库里写文件（会污染 `the_permission_registry_matches_every_manifest`）。
    #[test]
    fn a_phase_one_violation_fails_the_registry_build() {
        let root = std::env::temp_dir().join(format!(
            "base-schema-phase1-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        let module_dir = root.join("modules/sandbox");
        std::fs::create_dir_all(&module_dir).expect("临时目录应可建");
        let raw = manifest_with_schema(
            "demoItem",
            r#"{ "key": "dueAt", "label": "到期日", "type": "currency" }"#,
            r#"{ "code": "sandbox:demo:read", "name": "读", "type": "menu" }"#,
        );
        std::fs::write(module_dir.join("manifest.json"), raw).expect("临时 manifest 应可写");

        let error = SchemaRegistry::from_manifest_root(&root)
            .expect_err("阶段 1 违规必须让注册表构建失败");
        std::fs::remove_dir_all(&root).ok();

        assert_eq!(error.issues.len(), 1, "issues = {:?}", error.issues);
        assert_eq!(error.issues[0].field, "schema.demoItem.fields[0].type");
        assert_eq!(error.issues[0].source, "modules/sandbox/manifest.json");
        assert!(error.issues[0].reason.contains("currency"));
    }

    /// manifest 的 permission 字段被收集成「模块/实体/字段/权限点」四元组。
    #[test]
    fn permission_fields_are_listed_for_the_startup_log() {
        let registry = SchemaRegistry::from_manifest_root(&repo_root()).expect("应能建注册表");
        let fields = registry.permission_fields();
        assert!(
            fields.iter().any(|(module, entity, field, code)| module == "sandbox"
                && entity == "demoItem"
                && field == "secret"
                && code == "sandbox:demo:secret"),
            "fields = {fields:?}"
        );
    }

    fn org_manifest() -> Manifest {
        parse(
            r#"{
              "id": "org",
              "name": "Org",
              "version": "0.1.0",
              "permissions": [],
              "menus": [],
              "dependencies": []
            }"#,
        )
    }
}
