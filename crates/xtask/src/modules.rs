//! `modules/<name>/` 目录规范校验。
//!
//! 约定来自 ADR-0001 D7：一个目录 = 一个完整业务模块，
//! 必备 `manifest.json` / `backend/` / `frontend/` / `migrations/` / `AGENTS.md` / `README.md`。
//!
//! REQ-003 补齐的五项校验：
//!
//! | 校验 | 失败信息 |
//! |------|----------|
//! | 必备文件齐全 | 列出缺失的文件 |
//! | `manifest.json` 符合 Schema | 给出字段路径与原因 |
//! | `manifest.id` 与目录名一致 | 说明应以哪个为准 |
//! | `backend/`、`frontend/`、`migrations/` 非空 | 指出空目录 |
//! | `AGENTS.md`、`README.md` 存在 | 列出缺失 |

use std::path::Path;

use platform_core::extension::manifest::Manifest;

/// 模块必备文件。
pub const REQUIRED_FILES: [&str; 3] = ["manifest.json", "AGENTS.md", "README.md"];

/// 模块必备目录。
pub const REQUIRED_DIRS: [&str; 3] = ["backend", "frontend", "migrations"];

/// 单个模块的校验结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleStatus {
    /// 模块名（目录名）。
    pub name: String,
    /// 缺失的文件。
    pub missing_files: Vec<String>,
    /// 缺失的目录。
    pub missing_dirs: Vec<String>,
    /// 存在但为空的目录。
    pub empty_dirs: Vec<String>,
    /// `manifest.json` 解析失败时的原因。
    pub invalid_manifest: Option<String>,
    /// `manifest.json` 不符合 Schema 时的问题清单。
    pub manifest_issues: Vec<String>,
    /// `manifest.id` 与目录名不一致时的说明。
    pub id_mismatch: Option<String>,
}

impl ModuleStatus {
    /// 模块是否完全合规。
    pub fn is_valid(&self) -> bool {
        self.missing_files.is_empty()
            && self.missing_dirs.is_empty()
            && self.empty_dirs.is_empty()
            && self.invalid_manifest.is_none()
            && self.manifest_issues.is_empty()
            && self.id_mismatch.is_none()
    }

    /// 全部问题清单（供 CLI 直接打印）。
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for file in &self.missing_files {
            problems.push(format!("缺少文件 {file}"));
        }
        for dir in &self.missing_dirs {
            problems.push(format!("缺少目录 {dir}/"));
        }
        for dir in &self.empty_dirs {
            problems.push(format!("目录 {dir}/ 为空"));
        }
        if let Some(reason) = &self.invalid_manifest {
            problems.push(format!("manifest.json 不是合法 JSON：{reason}"));
        }
        problems.extend(self.manifest_issues.iter().cloned());
        if let Some(mismatch) = &self.id_mismatch {
            problems.push(mismatch.clone());
        }
        problems
    }
}

/// 仓库内所有业务模块的校验汇总。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleReport {
    /// 各模块的校验结果。
    pub modules: Vec<ModuleStatus>,
}

impl ModuleReport {
    /// 是否没有发现任何模块。
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// 所有模块是否都合规。
    pub fn is_valid(&self) -> bool {
        self.modules.iter().all(ModuleStatus::is_valid)
    }

    /// 不合规的模块。
    pub fn invalid_modules(&self) -> impl Iterator<Item = &ModuleStatus> {
        self.modules.iter().filter(|m| !m.is_valid())
    }
}

/// 扫描 `<root>/modules/` 下的业务模块目录并校验目录规范。
///
/// `modules/` 不存在或为空时返回空报告 —— 没有模块是合法状态，不是错误。
pub fn check_modules(root: &Path) -> ModuleReport {
    let modules_dir = root.join("modules");
    let Ok(entries) = std::fs::read_dir(&modules_dir) else {
        return ModuleReport::default();
    };

    let mut modules: Vec<ModuleStatus> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .map(|name| !name.starts_with('.'))
                .unwrap_or(false)
        })
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            check_module(&entry.path(), name)
        })
        .collect();

    modules.sort_by(|a, b| a.name.cmp(&b.name));
    ModuleReport { modules }
}

/// 校验单个模块目录。
fn check_module(module_dir: &Path, name: String) -> ModuleStatus {
    let missing_files = REQUIRED_FILES
        .iter()
        .filter(|required| !module_dir.join(required).is_file())
        .map(|required| (*required).to_string())
        .collect();

    let missing_dirs = REQUIRED_DIRS
        .iter()
        .filter(|required| !module_dir.join(required).is_dir())
        .map(|required| (*required).to_string())
        .collect();

    let empty_dirs = REQUIRED_DIRS
        .iter()
        .filter(|required| module_dir.join(required).is_dir())
        .filter(|required| is_empty_dir(&module_dir.join(required)))
        .map(|required| (*required).to_string())
        .collect();

    let manifest_path = module_dir.join("manifest.json");
    let invalid_manifest = check_manifest(&manifest_path);
    let manifest_issues = check_manifest_schema(&manifest_path);
    let id_mismatch = check_manifest_id(&manifest_path, &name);

    ModuleStatus {
        name,
        missing_files,
        missing_dirs,
        empty_dirs,
        invalid_manifest,
        manifest_issues,
        id_mismatch,
    }
}

/// 目录是否为空。
fn is_empty_dir(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

/// 校验 `manifest.json` 是否为合法 JSON。
fn check_manifest(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => return Some(err.to_string()),
    };
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(_) => None,
        Err(err) => Some(err.to_string()),
    }
}

/// 校验 `manifest.json` 是否符合 Schema（字段路径 + 原因）。
///
/// 结构级违规（缺必备字段、未知字段、字段类型错）在 serde 反序列化阶段就会失败，
/// 这里必须把它们报出来 —— 否则 `manifest.json` = `{}` 这种整体不合规的 manifest
/// 会因为「解析失败就跳过」而蒙混过关。
fn check_manifest_schema(path: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    // JSON 语法错误已由 `invalid_manifest` 记录，不重复报。
    if serde_json::from_str::<serde_json::Value>(&raw).is_err() {
        return Vec::new();
    }
    match Manifest::parse_str(&raw) {
        Ok(manifest) => manifest
            .validate()
            .into_iter()
            .map(|issue| {
                format!(
                    "manifest.json 不符合 Schema：{}：{}",
                    issue.field(),
                    issue.reason()
                )
            })
            .collect(),
        // serde 的报错自带字段路径与位置，如 "missing field `id` at line 1 column 2"。
        Err(err) => vec![format!("manifest.json 不符合 Schema：{}", err.reason())],
    }
}

/// 校验 `manifest.id` 是否与目录名一致。
fn check_manifest_id(path: &Path, dir_name: &str) -> Option<String> {
    let manifest = Manifest::from_path(path).ok()?;
    if manifest.id == dir_name {
        return None;
    }
    Some(format!(
        "manifest.id 为 `{}`，与目录名 `{dir_name}` 不一致（应以目录名为准）",
        manifest.id
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 建一个临时目录，测试结束后自动清理。
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("xtask-{label}-{}-{seq}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("临时目录应可创建");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 合法的 manifest 内容（`id` 与目录名一致）。
    fn manifest_json(id: &str) -> String {
        format!(
            r#"{{
              "id": "{id}",
              "name": "demo",
              "version": "0.1.0",
              "enabled": true,
              "permissions": [],
              "menus": [],
              "dependencies": []
            }}"#
        )
    }

    fn make_module(root: &Path, name: &str) -> PathBuf {
        let dir = root.join("modules").join(name);
        std::fs::create_dir_all(&dir).expect("模块目录应可创建");
        std::fs::write(dir.join("manifest.json"), manifest_json(name)).expect("写入应成功");
        std::fs::write(dir.join("AGENTS.md"), "# demo\n").expect("写入应成功");
        std::fs::write(dir.join("README.md"), "# demo\n").expect("写入应成功");
        for required in REQUIRED_DIRS {
            std::fs::create_dir_all(dir.join(required)).expect("目录应可创建");
            std::fs::write(dir.join(required).join("placeholder.txt"), "x\n").expect("写入应成功");
        }
        dir
    }

    #[test]
    fn missing_modules_dir_reports_no_modules() {
        let temp = TempDir::new("no-modules");
        let report = check_modules(temp.path());
        assert!(report.is_empty());
        assert!(report.is_valid());
    }

    #[test]
    fn empty_modules_dir_reports_no_modules() {
        let temp = TempDir::new("empty-modules");
        std::fs::create_dir_all(temp.path().join("modules")).expect("目录应可创建");
        let report = check_modules(temp.path());
        assert!(report.is_empty());
        assert!(report.is_valid());
    }

    #[test]
    fn complete_module_is_valid() {
        let temp = TempDir::new("complete");
        make_module(temp.path(), "demo");
        let report = check_modules(temp.path());
        assert_eq!(report.modules.len(), 1);
        assert!(report.is_valid());
        assert_eq!(report.modules[0].name, "demo");
    }

    #[test]
    fn missing_entries_are_listed() {
        let temp = TempDir::new("missing");
        let dir = temp.path().join("modules").join("broken");
        std::fs::create_dir_all(&dir).expect("目录应可创建");
        std::fs::write(dir.join("manifest.json"), manifest_json("broken")).expect("写入应成功");

        let report = check_modules(temp.path());
        assert!(!report.is_valid());
        let status = &report.modules[0];
        assert_eq!(status.missing_files, vec!["AGENTS.md", "README.md"]);
        assert_eq!(
            status.missing_dirs,
            vec!["backend", "frontend", "migrations"]
        );
        assert_eq!(status.problems().len(), 5);
    }

    #[test]
    fn invalid_manifest_json_is_reported() {
        let temp = TempDir::new("bad-manifest");
        let dir = temp.path().join("modules").join("broken");
        std::fs::create_dir_all(&dir).expect("目录应可创建");
        std::fs::write(dir.join("manifest.json"), "{ not json").expect("写入应成功");

        let report = check_modules(temp.path());
        assert!(!report.is_valid());
        assert!(report.modules[0].invalid_manifest.is_some());
    }

    #[test]
    fn dot_directories_are_ignored() {
        let temp = TempDir::new("dotdir");
        std::fs::create_dir_all(temp.path().join("modules").join(".cache")).expect("目录应可创建");
        let report = check_modules(temp.path());
        assert!(report.is_empty());
    }

    #[test]
    fn invalid_modules_are_iterable() {
        let temp = TempDir::new("mixed");
        make_module(temp.path(), "good");
        std::fs::create_dir_all(temp.path().join("modules").join("bad")).expect("目录应可创建");

        let report = check_modules(temp.path());
        assert_eq!(report.modules.len(), 2);
        let invalid: Vec<&str> = report.invalid_modules().map(|m| m.name.as_str()).collect();
        assert_eq!(invalid, vec!["bad"]);
    }

    #[test]
    fn manifest_schema_violations_are_reported_with_field_path() {
        let temp = TempDir::new("schema");
        let dir = make_module(temp.path(), "demo");
        std::fs::write(
            dir.join("manifest.json"),
            r#"{
              "id": "Demo",
              "name": "demo",
              "version": "0.1",
              "enabled": true,
              "permissions": [],
              "menus": [],
              "dependencies": []
            }"#,
        )
        .expect("写入应成功");

        let report = check_modules(temp.path());
        assert!(!report.is_valid());
        let status = &report.modules[0];
        assert!(
            status
                .manifest_issues
                .iter()
                .any(|issue| issue.contains("id") && issue.contains("Schema")),
            "issues = {:?}",
            status.manifest_issues
        );
        assert!(
            status
                .manifest_issues
                .iter()
                .any(|issue| issue.contains("version")),
            "issues = {:?}",
            status.manifest_issues
        );
    }

    #[test]
    fn manifest_without_required_fields_is_reported() {
        let temp = TempDir::new("empty-manifest");
        let dir = make_module(temp.path(), "demo");
        std::fs::write(dir.join("manifest.json"), "{}").expect("写入应成功");

        let report = check_modules(temp.path());
        assert!(!report.is_valid(), "空 manifest 必须判失败");
        let status = &report.modules[0];
        assert_eq!(status.manifest_issues.len(), 1, "issues = {status:?}");
        let issue = &status.manifest_issues[0];
        assert!(issue.contains("Schema"), "issue = {issue}");
        assert!(issue.contains("missing field"), "issue = {issue}");
        assert!(issue.contains("id"), "issue = {issue}");
    }

    #[test]
    fn manifest_with_unknown_field_is_reported() {
        let temp = TempDir::new("unknown-field");
        let dir = make_module(temp.path(), "demo");
        let raw =
            manifest_json("demo").replace("\"enabled\": true,", "\"enabled\": true, \"bogus\": 1,");
        std::fs::write(dir.join("manifest.json"), raw).expect("写入应成功");

        let report = check_modules(temp.path());
        assert!(!report.is_valid(), "含未知字段的 manifest 必须判失败");
        let status = &report.modules[0];
        assert_eq!(status.manifest_issues.len(), 1, "issues = {status:?}");
        let issue = &status.manifest_issues[0];
        assert!(issue.contains("Schema"), "issue = {issue}");
        assert!(issue.contains("unknown field"), "issue = {issue}");
        assert!(issue.contains("bogus"), "issue = {issue}");
    }

    #[test]
    fn manifest_id_must_match_the_directory_name() {
        let temp = TempDir::new("id-mismatch");
        let dir = make_module(temp.path(), "demo");
        std::fs::write(dir.join("manifest.json"), manifest_json("other")).expect("写入应成功");

        let report = check_modules(temp.path());
        assert!(!report.is_valid());
        let mismatch = report.modules[0]
            .id_mismatch
            .as_deref()
            .expect("id 不一致应被报告");
        assert!(mismatch.contains("other"));
        assert!(mismatch.contains("demo"));
    }

    #[test]
    fn empty_required_directories_are_reported() {
        let temp = TempDir::new("empty-dirs");
        let dir = make_module(temp.path(), "demo");
        for required in REQUIRED_DIRS {
            let path = dir.join(required).join("placeholder.txt");
            std::fs::remove_file(&path).expect("删除应成功");
        }

        let report = check_modules(temp.path());
        assert!(!report.is_valid());
        let status = &report.modules[0];
        assert_eq!(status.empty_dirs, vec!["backend", "frontend", "migrations"]);
    }
}
