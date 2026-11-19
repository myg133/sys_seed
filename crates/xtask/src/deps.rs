//! 依赖方向检查：平台代码永不反向依赖业务模块。
//!
//! ADR-0001 D7：依赖方向恒为
//!
//! ```text
//! platform-core (定义 trait)
//!       ↑ trait
//! platform-contracts (依赖 platform-core)
//!       ↑ trait
//! 业务模块 modules/<name>/backend (依赖 platform-core + platform-contracts)
//! ```
//!
//! 因此平台 crate 的依赖图里**不得出现任何业务模块**（`modules/...`）。
//!
//! # 唯一的例外：装配层
//!
//! [`ASSEMBLY_CRATES`]（当前只有 `platform-server`）**允许**依赖业务模块。
//!
//! REQ-012 的 D103 要把 `modules/sandbox` 挂进服务 —— 挂载只能发生在**装配层**：
//! `platform-server` 是交付用的 `base` 二进制，不是被业务模块依赖的库；
//! 它依赖业务模块正是它的职责（把编译期注册的模块接到 `/api/<module-id>` 下）。
//!
//! 例外**只有这一个**：`platform-core` / `platform-db` / `platform-contracts`
//! 依赖业务模块 = 依赖方向真的反了。
//!
//! # 怎么查的
//!
//! 读 `cargo metadata --format-version 1 --no-deps` 的成员依赖图，
//! 从每个平台 crate 出发做可达性遍历，碰到 `modules/` 下的包就是违规。
//!
//! **不是文本匹配**：workspace 继承写法（`dep.workspace = true`）把路径写在根
//! `Cargo.toml` 的 `[workspace.dependencies]` 里，成员自己的 `Cargo.toml` 里根本没有
//! `modules/` 字样，逐行扫字符串会漏掉这类绕过。
//!
//! # 平台 crate 怎么定的
//!
//! 两条来源取并集：
//!
//! - [`PLATFORM_CRATES`] 显式清单；
//! - `crates/` 下的工作区成员 —— 新增平台 crate 不会因为忘了改清单而漏检。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

/// 必须不依赖业务模块的平台 crate（显式清单）。
///
/// 清单之外的平台 crate 由「`crates/` 下的工作区成员」这条规则兜底。
pub const PLATFORM_CRATES: [&str; 4] = [
    "platform-core",
    "platform-contracts",
    "platform-contracts-macros",
    "platform-db",
];

/// **装配层**：允许依赖业务模块的平台 crate（例外，见模块文档）。
///
/// 加进这个清单等于放弃「依赖方向」这条硬规则对这个 crate 的约束 ——
/// 只有「可执行交付物」才有资格进来。库 crate 一律不行。
pub const ASSEMBLY_CRATES: [&str; 1] = ["platform-server"];

/// `cargo metadata` 的最小结构（只取用得到的字段）。
#[derive(Debug, Deserialize)]
struct Metadata {
    /// 工作区根目录。
    workspace_root: PathBuf,
    /// 工作区成员。
    packages: Vec<MetadataPackage>,
}

/// 工作区成员。
#[derive(Debug, Deserialize)]
struct MetadataPackage {
    /// 包名。
    name: String,
    /// `Cargo.toml` 的绝对路径。
    manifest_path: PathBuf,
    /// 直接依赖。
    #[serde(default)]
    dependencies: Vec<MetadataDependency>,
}

/// 成员的一条直接依赖。
#[derive(Debug, Clone, Deserialize)]
struct MetadataDependency {
    /// 依赖名。
    name: String,
    /// 路径依赖的绝对目录；非路径依赖为 `None`。
    #[serde(default)]
    path: Option<PathBuf>,
}

/// 工作区成员之间的依赖图。
#[derive(Debug)]
struct DependencyGraph {
    /// 工作区根目录。
    workspace_root: PathBuf,
    /// 业务模块目录（`<workspace_root>/modules`）。
    modules_dir: PathBuf,
    /// 平台 crate 目录（`<workspace_root>/crates`）。
    crates_dir: PathBuf,
    /// 成员目录 → 成员名。
    member_names: BTreeMap<PathBuf, String>,
    /// 成员名 → 它的直接依赖。
    edges: BTreeMap<String, Vec<MetadataDependency>>,
}

impl DependencyGraph {
    /// 从 `cargo metadata` 的输出建图。
    fn build(metadata: &Metadata) -> Self {
        let member_names: BTreeMap<PathBuf, String> = metadata
            .packages
            .iter()
            .filter_map(|package| {
                package
                    .manifest_path
                    .parent()
                    .map(|dir| (dir.to_path_buf(), package.name.clone()))
            })
            .collect();
        let edges = metadata
            .packages
            .iter()
            .map(|package| (package.name.clone(), package.dependencies.clone()))
            .collect();
        Self {
            workspace_root: metadata.workspace_root.clone(),
            modules_dir: metadata.workspace_root.join("modules"),
            crates_dir: metadata.workspace_root.join("crates"),
            member_names,
            edges,
        }
    }

    /// 需要检查的平台 crate：显式清单 ∪ `crates/` 下的工作区成员，**去掉装配层**。
    fn platform_crates(&self) -> Vec<String> {
        let mut seeds: BTreeSet<String> = PLATFORM_CRATES
            .iter()
            .map(|name| (*name).to_string())
            .collect();
        for (dir, name) in &self.member_names {
            if dir.starts_with(&self.crates_dir) {
                seeds.insert(name.clone());
            }
        }
        seeds
            .into_iter()
            .filter(|name| !ASSEMBLY_CRATES.contains(&name.as_str()))
            .collect()
    }

    /// 路径是否指向业务模块（位于 `modules/` 下）。
    fn is_business_module(&self, path: &Path) -> bool {
        path.starts_with(&self.modules_dir)
    }

    /// 从 `platform` 出发能到达的业务模块。
    fn reverse_dependencies(&self, platform: &str) -> Vec<String> {
        let mut violations = Vec::new();
        let mut visited: BTreeSet<String> = BTreeSet::new();
        let mut pending: Vec<(String, Vec<String>)> =
            vec![(platform.to_string(), vec![platform.to_string()])];

        while let Some((current, chain)) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            let Some(dependencies) = self.edges.get(&current) else {
                continue;
            };
            for dependency in dependencies {
                // 非路径依赖（registry / git）不可能指向工作区成员。
                let Some(dependency_path) = dependency.path.as_ref() else {
                    continue;
                };
                let label = self
                    .member_names
                    .get(dependency_path)
                    .cloned()
                    .unwrap_or_else(|| dependency.name.clone());
                let mut next_chain = chain.clone();
                next_chain.push(label);
                if self.is_business_module(dependency_path) {
                    violations.push(self.format_violation(&next_chain, dependency_path));
                    continue;
                }
                if let Some(member) = self.member_names.get(dependency_path) {
                    pending.push((member.clone(), next_chain));
                }
            }
        }
        violations
    }

    /// 违规说明：谁、通过哪条链、反向依赖了哪个业务模块。
    fn format_violation(&self, chain: &[String], module_path: &Path) -> String {
        let relative = module_path
            .strip_prefix(&self.workspace_root)
            .unwrap_or(module_path);
        format!(
            "平台 crate `{}` 反向依赖了业务模块 `{}`（路径：{}，依赖链：{}）",
            chain.first().map(String::as_str).unwrap_or_default(),
            chain.last().map(String::as_str).unwrap_or_default(),
            relative.display(),
            chain.join(" → ")
        )
    }
}

/// 扫描工作区的依赖图，返回平台 crate → 业务模块的反向依赖。
///
/// 读的是 `cargo metadata` 的成员依赖图，因此 workspace 继承写法
/// （`dep.workspace = true`）与新增的平台 crate 一样能被看见。
pub fn dependency_direction_violations(root: &Path) -> Result<Vec<String>> {
    let metadata = load_metadata(root)?;
    let graph = DependencyGraph::build(&metadata);
    let mut violations = Vec::new();
    for platform in graph.platform_crates() {
        violations.extend(graph.reverse_dependencies(&platform));
    }
    Ok(violations)
}

/// 跑 `cargo metadata` 并解析出成员依赖图。
fn load_metadata(root: &Path) -> Result<Metadata> {
    let manifest_path = root.join("Cargo.toml");
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .arg("--manifest-path")
        .arg(&manifest_path)
        .output()
        .with_context(|| format!("运行 cargo metadata 失败（{}）", manifest_path.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "cargo metadata 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("解析 cargo metadata 的输出失败")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("xtask-deps-{label}-{}-{seq}", std::process::id()));
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

    /// 写一个最小 cargo 工作区。
    ///
    /// `members` 每项是 `(工作区成员目录, 包名, [dependencies] 片段)`；
    /// `workspace_dependencies` 写进根 `Cargo.toml` 的 `[workspace.dependencies]`。
    fn write_workspace(root: &Path, members: &[(&str, &str, &str)], workspace_dependencies: &str) {
        let mut workspace_manifest = String::from("[workspace]\nresolver = \"2\"\nmembers = [");
        for (dir, _, _) in members {
            workspace_manifest.push_str(&format!("\"{dir}\", "));
        }
        workspace_manifest.push_str("]\n");
        if !workspace_dependencies.is_empty() {
            workspace_manifest.push_str("\n[workspace.dependencies]\n");
            workspace_manifest.push_str(workspace_dependencies);
        }
        std::fs::write(root.join("Cargo.toml"), workspace_manifest).expect("写入应成功");

        for (dir, name, dependencies) in members {
            let package_dir = root.join(dir);
            std::fs::create_dir_all(package_dir.join("src")).expect("目录应可创建");
            std::fs::write(package_dir.join("src/lib.rs"), "").expect("写入应成功");
            let manifest = format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{dependencies}"
            );
            std::fs::write(package_dir.join("Cargo.toml"), manifest).expect("写入应成功");
        }
    }

    /// 跑依赖方向检查，失败信息直接进断言消息。
    fn violations_of(root: &Path) -> Vec<String> {
        dependency_direction_violations(root).expect("依赖图应可读")
    }

    #[test]
    fn business_module_dependencies_are_reported() {
        let temp = TempDir::new("violation");
        write_workspace(
            temp.path(),
            &[
                (
                    "crates/platform-core",
                    "platform-core",
                    "sandbox = { path = \"../../modules/sandbox\" }\n",
                ),
                ("modules/sandbox", "sandbox", ""),
            ],
            "",
        );

        let violations = violations_of(temp.path());
        assert_eq!(violations.len(), 1, "violations = {violations:?}");
        assert!(
            violations[0].contains("platform-core"),
            "violations = {violations:?}"
        );
        assert!(
            violations[0].contains("modules/sandbox"),
            "violations = {violations:?}"
        );
    }

    /// 装配层可以依赖业务模块（D103），但**只有它**可以。
    ///
    /// 两条断言语义不同：
    ///
    /// 1. `platform-server → sandbox` **不**报违规（挂载只能在装配层发生）；
    /// 2. `platform-core → sandbox` **报**违规（依赖方向真的反了）。
    ///
    /// 只断言第 1 条是不够的 —— 那样把整条规则删掉也能过。
    #[test]
    fn only_the_assembly_layer_may_depend_on_business_modules() {
        let temp = TempDir::new("assembly");
        write_workspace(
            temp.path(),
            &[
                (
                    "crates/platform-server",
                    "platform-server",
                    "sandbox = { path = \"../../modules/sandbox\" }\n",
                ),
                ("modules/sandbox", "sandbox", ""),
            ],
            "",
        );
        assert!(
            violations_of(temp.path()).is_empty(),
            "装配层依赖业务模块是它的职责：{:?}",
            violations_of(temp.path())
        );

        let temp = TempDir::new("assembly-guard");
        write_workspace(
            temp.path(),
            &[
                (
                    "crates/platform-core",
                    "platform-core",
                    "sandbox = { path = \"../../modules/sandbox\" }\n",
                ),
                ("modules/sandbox", "sandbox", ""),
            ],
            "",
        );
        assert_eq!(
            violations_of(temp.path()).len(),
            1,
            "例外只给装配层，库 crate 仍受约束"
        );
    }

    #[test]
    fn platform_only_dependencies_are_allowed() {
        let temp = TempDir::new("clean");
        write_workspace(
            temp.path(),
            &[
                (
                    "crates/platform-core",
                    "platform-core",
                    "platform-contracts = { path = \"../platform-contracts\" }\n",
                ),
                ("crates/platform-contracts", "platform-contracts", ""),
            ],
            "",
        );

        assert!(violations_of(temp.path()).is_empty());
    }

    #[test]
    fn workspace_inherited_dependency_is_reported() {
        // 路径写在根 Cargo.toml 的 [workspace.dependencies] 里，
        // 成员自己的 Cargo.toml 只有 `sandbox.workspace = true` —— 文本扫描会漏掉。
        let temp = TempDir::new("workspace-inherit");
        write_workspace(
            temp.path(),
            &[
                (
                    "crates/platform-contracts",
                    "platform-contracts",
                    "sandbox.workspace = true\n",
                ),
                ("modules/sandbox", "sandbox", ""),
            ],
            "sandbox = { path = \"modules/sandbox\" }\n",
        );

        let violations = violations_of(temp.path());
        assert_eq!(violations.len(), 1, "violations = {violations:?}");
        assert!(
            violations[0].contains("platform-contracts"),
            "violations = {violations:?}"
        );
        assert!(
            violations[0].contains("modules/sandbox"),
            "violations = {violations:?}"
        );
    }

    #[test]
    fn macros_crate_is_subject_to_the_direction_rule() {
        // platform-contracts-macros 是本期新增的平台 crate，历史上不在清单里。
        let temp = TempDir::new("macros-crate");
        write_workspace(
            temp.path(),
            &[
                (
                    "crates/platform-contracts-macros",
                    "platform-contracts-macros",
                    "sandbox = { path = \"../../modules/sandbox\" }\n",
                ),
                ("modules/sandbox", "sandbox", ""),
            ],
            "",
        );

        let violations = violations_of(temp.path());
        assert_eq!(violations.len(), 1, "violations = {violations:?}");
        assert!(
            violations[0].contains("platform-contracts-macros"),
            "violations = {violations:?}"
        );
        assert!(
            violations[0].contains("modules/sandbox"),
            "violations = {violations:?}"
        );
    }

    #[test]
    fn platform_crate_outside_the_explicit_list_is_also_checked() {
        // `crates/` 下的工作区成员一律视为平台 crate：新增平台 crate 不会漏检。
        let temp = TempDir::new("unlisted-crate");
        write_workspace(
            temp.path(),
            &[
                (
                    "crates/platform-extras",
                    "platform-extras",
                    "sandbox = { path = \"../../modules/sandbox\" }\n",
                ),
                ("modules/sandbox", "sandbox", ""),
            ],
            "",
        );

        let violations = violations_of(temp.path());
        assert_eq!(violations.len(), 1, "violations = {violations:?}");
        assert!(
            violations[0].contains("platform-extras"),
            "violations = {violations:?}"
        );
        assert!(
            violations[0].contains("modules/sandbox"),
            "violations = {violations:?}"
        );
    }

    #[test]
    fn the_real_workspace_has_no_reverse_dependency() {
        let violations = violations_of(&crate::workspace_root());
        assert!(
            violations.is_empty(),
            "平台 crate 不得依赖业务模块：{violations:?}"
        );
    }

    /// REQ-006 新建的 `platform-db` 必须**自动**被纳入平台 crate 集合。
    ///
    /// 集合 = 显式清单 ∪ `crates/` 下的工作区成员。
    /// 业务模块将来要依赖 `platform-db` —— 它一旦反向依赖业务模块，
    /// 就等于开了个绕过门禁的口子，所以「自动纳管」必须真的生效。
    #[test]
    fn platform_db_is_covered_by_the_platform_crate_rules() {
        let metadata = load_metadata(&crate::workspace_root()).expect("cargo metadata 应可读");
        let graph = DependencyGraph::build(&metadata);
        let platform_crates = graph.platform_crates();
        assert!(
            platform_crates.iter().any(|name| name == "platform-db"),
            "platform-db 必须被纳入平台 crate 集合（自动纳管失效 = 门禁被绕过）：{platform_crates:?}"
        );
    }
}
