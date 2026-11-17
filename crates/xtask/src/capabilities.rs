//! 能力索引生成器：`cargo xtask capabilities` / `--check`。
//!
//! # 为什么要自动生成
//!
//! **过期的索引比没有索引更危险** —— AI 会严格按一份过时清单办事，
//! 错得比没有依据还理直气壮。所以索引必须「跟着源码走」，且 CI 强制校验。
//!
//! # 产物
//!
//! | 文件 | 来源 |
//! |------|------|
//! | `index.json` | 总索引（含 `already_provided`），AI 的入口 |
//! | `permissions.json` | 扫描全部 `modules/*/manifest.json` 的 `permissions[]` |
//! | `extension-points.json` | 解析 `crates/platform-core/src/extension/` 的 trait 声明 |
//! | `schema/manifest.schema.json` | `platform-core` 内置的 manifest Schema 常量 |
//! | `schema/metadata.schema.json` | `packages/schema/src/index.ts` 的字段类型枚举 |
//! | `sdk/<package>.json` | 各 `packages/*/package.json` + `src/index.ts` 的导出清单 |
//! | `api.json` | HTTP 层未就绪，占位并注明补齐需求 |
//!
//! # 确定性
//!
//! 产物**逐字节可复现**：不含墙钟时间戳，全部内容只由源码决定。
//! 这样 `--check` 才能发现「任何一处手改」—— 手改即失败，是 AC-S-004 的核心。

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::catalog::{ALREADY_PROVIDED, NOT_YET_PROVIDED, SDK_USAGE};
use crate::rustscan::{self, TraitDecl};

/// 索引产物：相对 `capabilities/` 的路径 → 文件内容。
pub type GeneratedFiles = BTreeMap<String, String>;

/// 生成器的名字，写进每个产物的 `generated_by` 字段。
const GENERATOR: &str = "cargo xtask capabilities";

/// `extension-points v1` 契约冻结的 5 个扩展点 trait。
///
/// 目录里还有 `TransactionalWrite` —— 它是平台为 C-4 提供的能力声明面，
/// **不是**业务模块实现的接缝，因此单独归类。
const EXTENSION_POINT_TRAITS: [&str; 5] = [
    "PlatformModule",
    "Repository",
    "Auditable",
    "NotificationSender",
    "WorkflowHandler",
];

/// 生成全部能力索引文件。
pub fn generate(root: &Path) -> Result<GeneratedFiles> {
    let mut files = GeneratedFiles::new();
    files.insert("permissions.json".into(), permissions_json(root)?);
    files.insert("extension-points.json".into(), extension_points_json(root)?);
    files.insert(
        "schema/manifest.schema.json".into(),
        manifest_schema_json()?,
    );
    files.insert("schema/metadata.schema.json".into(), metadata_schema_json(root)?);
    for (name, content) in sdk_files(root)? {
        files.insert(name, content);
    }
    files.insert("api.json".into(), api_json());
    let index = index_json(&files)?;
    files.insert("index.json".into(), index);
    Ok(files)
}

/// `--check` 模式的比对结果。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// 缺失的文件（源码里有、磁盘上没有）。
    pub missing: Vec<String>,
    /// 内容不一致的文件。
    pub changed: Vec<String>,
    /// 多余的文件（磁盘上有、生成器不产出）。
    pub extra: Vec<String>,
}

impl CheckReport {
    /// 是否完全一致。
    pub fn is_consistent(&self) -> bool {
        self.missing.is_empty() && self.changed.is_empty() && self.extra.is_empty()
    }
}

/// 比对磁盘上的索引与重新生成的结果。
pub fn check(root: &Path, generated: &GeneratedFiles) -> Result<CheckReport> {
    let mut report = CheckReport::default();
    let capabilities_dir = root.join("capabilities");

    for (name, content) in generated {
        let path = capabilities_dir.join(name);
        if !path.is_file() {
            report.missing.push(name.clone());
            continue;
        }
        let on_disk = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if on_disk != *content {
            report.changed.push(name.clone());
        }
    }

    for name in on_disk_files(&capabilities_dir)? {
        if !generated.contains_key(&name) {
            report.extra.push(name);
        }
    }

    report.missing.sort();
    report.changed.sort();
    report.extra.sort();
    Ok(report)
}

/// 磁盘上 `capabilities/` 下的全部文件（相对路径，已排序）。
fn on_disk_files(capabilities_dir: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    if !capabilities_dir.is_dir() {
        return Ok(files);
    }
    let mut stack = vec![capabilities_dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("cannot read {}", current.display()))?;
        for entry in entries {
            let path = entry
                .with_context(|| format!("cannot read an entry under {}", current.display()))?
                .path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(capabilities_dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push(relative);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// 把生成结果写进磁盘。
pub fn write_files(root: &Path, files: &GeneratedFiles) -> Result<()> {
    let capabilities_dir = root.join("capabilities");
    for (name, content) in files {
        let path = capabilities_dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        std::fs::write(&path, content).with_context(|| format!("cannot write {}", path.display()))?;
    }
    Ok(())
}

/// 权限点索引：扫描全部模块的 `manifest.json`。
fn permissions_json(root: &Path) -> Result<String> {
    let modules_dir = root.join("modules");
    let mut modules: Vec<Value> = Vec::new();
    let mut total = 0usize;

    for name in module_dir_names(&modules_dir)? {
        let module_dir = modules_dir.join(&name);
        let manifest_path = module_dir.join("manifest.json");
        if !manifest_path.is_file() {
            continue;
        }
        let raw = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("cannot read {}", manifest_path.display()))?;
        let manifest = platform_core::extension::manifest::Manifest::parse_checked(&raw)
            .map_err(|issues| {
                let detail = issues
                    .iter()
                    .map(|issue| format!("{}: {}", issue.field(), issue.reason()))
                    .collect::<Vec<_>>()
                    .join("; ");
                anyhow::anyhow!("{} 不符合 Schema：{detail}", manifest_path.display())
            })?;

        let permissions: Vec<Value> = manifest
            .permissions
            .iter()
            .map(|permission| {
                json!({
                    "code": permission.code,
                    "name": permission.name,
                    "type": permission.kind,
                })
            })
            .collect();
        total += permissions.len();

        modules.push(json!({
            "module": manifest.id,
            "manifest": format!("modules/{name}/manifest.json"),
            "permissions": permissions,
        }));
    }

    Ok(to_json(&json!({
        "generated_by": GENERATOR,
        "modules": modules,
        "total": total,
    })))
}

/// 扩展点索引：解析平台扩展点目录里的 trait 声明。
fn extension_points_json(root: &Path) -> Result<String> {
    let extension_dir = root.join("crates/platform-core/src/extension");
    let traits = rustscan::scan_traits(&extension_dir)?;
    let entries: Vec<Value> = traits.iter().map(trait_value).collect();
    let extension_points = traits
        .iter()
        .filter(|item| EXTENSION_POINT_TRAITS.contains(&item.name.as_str()))
        .count();

    Ok(to_json(&json!({
        "generated_by": GENERATOR,
        "source": "crates/platform-core/src/extension/",
        "contract": "extension-points v1",
        "extension_point_count": extension_points,
        "traits": entries,
    })))
}

/// 一个 trait 的 JSON 形态。
fn trait_value(trait_decl: &TraitDecl) -> Value {
    let kind = if EXTENSION_POINT_TRAITS.contains(&trait_decl.name.as_str()) {
        "extension-point"
    } else {
        "platform-capability"
    };
    json!({
        "name": trait_decl.name,
        "kind": kind,
        "generics": trait_decl.generics,
        "supertraits": trait_decl.supertraits,
        "description": trait_decl.description,
        "associated_types": trait_decl.associated_types.iter().map(member_value).collect::<Vec<_>>(),
        "methods": trait_decl.methods.iter().map(member_value).collect::<Vec<_>>(),
    })
}

/// 一个 trait 成员的 JSON 形态。
fn member_value(member: &rustscan::MemberDecl) -> Value {
    json!({
        "name": member.name,
        "declaration": member.declaration,
        "has_default": member.has_default,
    })
}

/// manifest 的 JSON Schema：取平台内置常量，重新序列化后落盘。
///
/// **重新序列化**是刻意的：所有产物用同一套 JSON 风格（`serde_json` 的缩进），
/// 生成物因此是「机器写的」，不是「人手排的」—— `capabilities/` 整体在
/// `.prettierignore` 里，不参与 prettier 校验。
/// 内容与常量**完全一致**（平台核心的测试按解析后的 JSON 比对）。
fn manifest_schema_json() -> Result<String> {
    let constant: Value = serde_json::from_str(
        platform_core::extension::manifest_schema::MANIFEST_JSON_SCHEMA,
    )
    .context("平台内置的 manifest Schema 不是合法 JSON")?;
    Ok(to_json(&constant))
}

/// 元数据的 JSON Schema：字段类型枚举来自 `@base/schema` 的真实定义。
fn metadata_schema_json(root: &Path) -> Result<String> {
    let source_path = root.join("packages/schema/src/index.ts");
    let source = std::fs::read_to_string(&source_path)
        .with_context(|| format!("cannot read {}", source_path.display()))?;
    let field_types = string_union(&source, "FieldType");
    if field_types.is_empty() {
        anyhow::bail!(
            "{} 里找不到 FieldType 联合类型，字段元数据 Schema 无法生成",
            source_path.display()
        );
    }

    Ok(to_json(&json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Base Platform Field Metadata",
        "description": "字段元数据的形状，来源：packages/schema/src/index.ts 的 FieldMeta / FieldType",
        "type": "object",
        "required": ["key", "label", "type"],
        "additionalProperties": false,
        "properties": {
            "key": { "type": "string", "pattern": "^[a-z][a-zA-Z0-9]*$" },
            "label": { "type": "string", "minLength": 1 },
            "type": { "enum": field_types },
            "required": { "type": "boolean", "default": false },
        },
    })))
}

/// HTTP 接口清单：本期未就绪。
fn api_json() -> String {
    to_json(&json!({
        "generated_by": GENERATOR,
        "status": "not-ready",
        "note": "HTTP 层未就绪：接口清单由 REQ-011（接口层）与 REQ-012（元数据 schema）落地后并入本索引",
        "openapi": Value::Null,
        "paths": [],
    }))
}

/// 各前端包的 SDK 索引。
fn sdk_files(root: &Path) -> Result<Vec<(String, String)>> {
    let packages_dir = root.join("packages");
    let mut files = Vec::new();

    for name in package_dir_names(&packages_dir)? {
        let package_dir = packages_dir.join(&name);
        let manifest_path = package_dir.join("package.json");
        if !manifest_path.is_file() {
            continue;
        }
        let raw = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("cannot read {}", manifest_path.display()))?;
        let manifest: Value =
            serde_json::from_str(&raw).with_context(|| format!("{} 不是合法 JSON", manifest_path.display()))?;
        let package_name = manifest["name"]
            .as_str()
            .with_context(|| format!("{} 缺少 name 字段", manifest_path.display()))?
            .to_string();

        let source_path = package_dir.join("src/index.ts");
        let source = std::fs::read_to_string(&source_path)
            .with_context(|| format!("cannot read {}", source_path.display()))?;
        let exports = ts_exports(&source);

        let usage = SDK_USAGE
            .iter()
            .find(|item| item.package == package_name)
            .map(|item| json!({ "example": item.example }))
            .unwrap_or_else(|| json!({ "example": Value::Null }));

        let content = to_json(&json!({
            "generated_by": GENERATOR,
            "package": package_name,
            "version": manifest["version"],
            "source": format!("packages/{name}/src/index.ts"),
            "entry": manifest["exports"]["."]["import"],
            "exports": exports,
            "usage": usage,
        }));
        files.push((format!("sdk/{name}.json"), content));
    }

    Ok(files)
}

/// 总索引。
fn index_json(files: &GeneratedFiles) -> Result<String> {
    let fingerprint = source_fingerprint(files);
    let artifacts = artifact_summary(files);
    let already: BTreeMap<&str, &str> = ALREADY_PROVIDED
        .iter()
        .map(|item| (item.key, item.summary))
        .collect();
    let not_yet: BTreeMap<&str, &str> = NOT_YET_PROVIDED
        .iter()
        .map(|item| (item.key, item.summary))
        .collect();

    Ok(to_json(&json!({
        "generated_by": GENERATOR,
        "source_fingerprint": fingerprint,
        "platform": {
            "version": platform_version(),
            "modules": module_names(),
        },
        "already_provided": already,
        "not_yet_provided": not_yet,
        "entry_points": {
            "extension_traits": "crates/platform-core/src/extension/",
            "contract_suite": "crates/platform-contracts/",
            "xtask": "crates/xtask/src/",
            "modules": "modules/",
            "packages": "packages/",
            "prompts": "prompts/",
        },
        "artifacts": artifacts,
    })))
}

/// 产物摘要：让 AI 知道每个文件里有什么。
fn artifact_summary(files: &GeneratedFiles) -> Value {
    let mut summary = serde_json::Map::new();
    for (name, content) in files {
        let detail = match name.as_str() {
            "permissions.json" => json!({ "total": count_field(content, "total") }),
            "extension-points.json" => json!({
                "traits": count_field(content, "extension_point_count"),
            }),
            "schema/metadata.schema.json" => json!({}),
            "api.json" => json!({ "status": "not-ready" }),
            other if other.starts_with("sdk/") => json!({
                "exports": count_field(content, "exports"),
            }),
            _ => json!({}),
        };
        summary.insert(name.clone(), detail);
    }
    Value::Object(summary)
}

/// 从 JSON 文本里取一个字段的条目数（用于产物摘要）。
///
/// 数字字段取其值，数组字段取其长度。
fn count_field(content: &str, field: &str) -> usize {
    let Some(value) = serde_json::from_str::<Value>(content)
        .ok()
        .and_then(|value| value.get(field).cloned())
    else {
        return 0;
    };
    match value {
        Value::Number(number) => number.as_u64().unwrap_or_default() as usize,
        Value::Array(items) => items.len(),
        _ => 0,
    }
}

/// 平台版本（取自工作区 `Cargo.toml` 的版本号）。
fn platform_version() -> String {
    let manifest = std::fs::read_to_string(crate::workspace_root().join("Cargo.toml"));
    let Ok(manifest) = manifest else {
        return "unknown".to_string();
    };
    for line in manifest.lines() {
        let trimmed = line.trim();
        if let Some(version) = trimmed.strip_prefix("version = ") {
            return version.trim_matches('"').to_string();
        }
    }
    "unknown".to_string()
}

/// 已有模块名（目录名，已排序）。
fn module_names() -> Vec<String> {
    module_dir_names(&crate::workspace_root().join("modules")).unwrap_or_default()
}

/// `modules/` 下的模块目录名（已排序，跳过以 `.` 开头的）。
fn module_dir_names(modules_dir: &Path) -> Result<Vec<String>> {
    dir_names(modules_dir, |name| !name.starts_with('.'))
}

/// `packages/` 下的包目录名（已排序，跳过以 `.` 开头的）。
fn package_dir_names(packages_dir: &Path) -> Result<Vec<String>> {
    dir_names(packages_dir, |name| !name.starts_with('.'))
}

/// 目录下的子目录名（已排序）。
fn dir_names(dir: &Path, keep: impl Fn(&str) -> bool) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(names);
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if keep(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// 源码指纹：全部产物的 FNV-1a 64 位散列。
///
/// 用途：一眼看出「索引对应的是哪一份源码」。用 FNV-1a 而不是现成的哈希库，
/// 是为了**不新增任何第三方依赖**（设计摘要 §8 的硬约束）。
fn source_fingerprint(files: &GeneratedFiles) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, content) in files {
        for byte in name.as_bytes().iter().chain(content.as_bytes()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("fnv1a64:{hash:016x}")
}

/// TypeScript 源码里的字符串联合类型（`'a' | 'b'`）。
fn string_union(source: &str, type_name: &str) -> Vec<String> {
    let needle = format!("export type {type_name}");
    let Some(start) = source.find(&needle) else {
        return Vec::new();
    };
    let rest = &source[start + needle.len()..];
    let Some(equals) = rest.find('=') else {
        return Vec::new();
    };
    let rest = &rest[equals + 1..];
    let Some(end) = rest.find(';') else {
        return Vec::new();
    };
    rest[..end]
        .split('|')
        .map(|part| part.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// TypeScript 源码里的导出清单。
fn ts_exports(source: &str) -> Vec<Value> {
    let mut exports = Vec::new();
    let mut doc: Vec<String> = Vec::new();
    let mut in_block = false;

    for line in source.lines() {
        let trimmed = line.trim();

        if in_block {
            if trimmed == "*/" {
                in_block = false;
                continue;
            }
            let content = trimmed.trim_start_matches('*').trim_end_matches("*/").trim();
            if !content.is_empty() {
                doc.push(content.to_string());
            }
            in_block = !trimmed.ends_with("*/");
            continue;
        }

        if trimmed.starts_with("/*") {
            in_block = !trimmed.ends_with("*/");
            let content = block_doc_content(trimmed);
            if !content.is_empty() {
                doc.push(content.to_string());
            }
            continue;
        }

        if let Some(text) = trimmed.strip_prefix("///") {
            doc.push(text.trim().to_string());
            continue;
        }

        let Some((kind, name)) = export_head(trimmed) else {
            if !trimmed.is_empty() {
                doc.clear();
            }
            continue;
        };
        exports.push(json!({
            "name": name,
            "kind": kind,
            "declaration": trimmed.trim_end_matches(';'),
            "description": doc.first().cloned().unwrap_or_default(),
        }));
        doc.clear();
    }

    exports
}

/// 取块注释一行的正文（去掉 `/*`、`*`、`*/` 记号）。
fn block_doc_content(line: &str) -> String {
    line.trim_start_matches("/*")
        .trim_start_matches('*')
        .trim_end_matches("*/")
        .trim()
        .to_string()
}

/// 识别一行导出声明的种类与名字。
fn export_head(line: &str) -> Option<(&'static str, String)> {
    let rest = line.strip_prefix("export ")?;
    for (keyword, kind) in [
        ("async function ", "function"),
        ("function ", "function"),
        ("interface ", "interface"),
        ("type ", "type"),
        ("const ", "const"),
        ("class ", "class"),
        ("enum ", "enum"),
    ] {
        if let Some(after) = rest.strip_prefix(keyword) {
            let name: String = after
                .chars()
                .take_while(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '$')
                .collect();
            if !name.is_empty() {
                return Some((kind, name));
            }
        }
    }
    None
}

/// 序列化成稳定的 JSON 文本（键按字典序，末尾带换行）。
fn to_json(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("索引必须是可序列化的 JSON");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_union_is_extracted() {
        let source = "export type FieldType = 'text' | 'number' | 'boolean';";
        assert_eq!(
            string_union(source, "FieldType"),
            vec!["text", "number", "boolean"]
        );
    }

    #[test]
    fn string_union_returns_nothing_for_unknown_types() {
        assert!(string_union("export type Other = 'a';", "FieldType").is_empty());
    }

    #[test]
    fn ts_exports_are_listed_with_kind_and_description() {
        let source = concat!(
            "/**\n",
            " * 认证占位包。\n",
            " */\n",
            "export interface AuthState {\n",
            "  authenticated: boolean;\n",
            "}\n",
            "\n",
            "/** 是否已登录。 */\n",
            "export function isAuthenticated(state: AuthState): boolean {\n",
            "  return state.authenticated;\n",
            "}\n",
        );
        let exports = ts_exports(source);
        let names: Vec<&str> = exports
            .iter()
            .filter_map(|item| item["name"].as_str())
            .collect();
        assert_eq!(names, vec!["AuthState", "isAuthenticated"]);
        assert_eq!(exports[0]["kind"], "interface");
        assert_eq!(exports[0]["description"], "认证占位包。");
        assert_eq!(exports[1]["kind"], "function");
        assert_eq!(exports[1]["description"], "是否已登录。");
    }

    #[test]
    fn export_head_recognises_every_kind() {
        assert_eq!(export_head("export function foo() {"), Some(("function", "foo".into())));
        assert_eq!(export_head("export async function foo() {"), Some(("function", "foo".into())));
        assert_eq!(export_head("export interface Foo {"), Some(("interface", "Foo".into())));
        assert_eq!(export_head("export type Foo = string;"), Some(("type", "Foo".into())));
        assert_eq!(export_head("export const FOO = 1;"), Some(("const", "FOO".into())));
        assert_eq!(export_head("export class Foo {"), Some(("class", "Foo".into())));
        assert_eq!(export_head("export enum Foo {"), Some(("enum", "Foo".into())));
        assert_eq!(export_head("const foo = 1;"), None);
    }

    #[test]
    fn fingerprint_is_stable_and_sensitive() {
        let mut files = GeneratedFiles::new();
        files.insert("a.json".into(), "content".to_string());
        let first = source_fingerprint(&files);
        assert_eq!(first, source_fingerprint(&files));
        files.insert("a.json".into(), "changed".to_string());
        assert_ne!(first, source_fingerprint(&files));
    }

    #[test]
    fn manifest_schema_matches_the_platform_constant() {
        let generated = manifest_schema_json().expect("Schema 应能生成");
        let parsed: Value = serde_json::from_str(&generated).expect("Schema 必须是合法 JSON");
        let constant: Value = serde_json::from_str(
            platform_core::extension::manifest_schema::MANIFEST_JSON_SCHEMA,
        )
        .expect("内置 Schema 必须是合法 JSON");
        assert_eq!(parsed, constant);
    }
}
