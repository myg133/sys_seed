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
//! 因此 `crates/platform-core` 与 `crates/platform-contracts` 的依赖图里
//! **不得出现任何业务模块路径**（`modules/...`）。

use std::path::Path;

/// 必须不依赖业务模块的平台 crate。
pub const PLATFORM_CRATES: [&str; 2] = ["platform-core", "platform-contracts"];

/// 扫描平台 crate 的依赖清单，返回发现的业务模块依赖。
///
/// 返回值每一项形如
/// `crates/platform-core/Cargo.toml:12: platform-something = { path = "modules/xxx" }`。
pub fn dependency_direction_violations(root: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    for crate_name in PLATFORM_CRATES {
        let manifest_path = root.join("crates").join(crate_name).join("Cargo.toml");
        let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        for (index, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if !trimmed.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                continue;
            }
            if !trimmed.contains("modules/") {
                continue;
            }
            violations.push(format!(
                "crates/{crate_name}/Cargo.toml:{} 依赖了业务模块路径：{trimmed}",
                index + 1
            ));
        }
    }
    violations
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

    fn write_manifest(root: &Path, crate_name: &str, body: &str) {
        let dir = root.join("crates").join(crate_name);
        std::fs::create_dir_all(&dir).expect("目录应可创建");
        std::fs::write(dir.join("Cargo.toml"), body).expect("写入应成功");
    }

    #[test]
    fn business_module_dependencies_are_reported() {
        let temp = TempDir::new("violation");
        write_manifest(
            temp.path(),
            "platform-core",
            "[dependencies]\nsandbox = { path = \"modules/sandbox\" }\n",
        );

        let violations = dependency_direction_violations(temp.path());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("platform-core"));
        assert!(violations[0].contains("modules/sandbox"));
    }

    #[test]
    fn platform_only_dependencies_are_allowed() {
        let temp = TempDir::new("clean");
        write_manifest(
            temp.path(),
            "platform-core",
            "[dependencies]\nplatform-contracts = { path = \"crates/platform-contracts\" }\n",
        );

        assert!(dependency_direction_violations(temp.path()).is_empty());
    }

    #[test]
    fn the_real_workspace_has_no_reverse_dependency() {
        let violations = dependency_direction_violations(&crate::workspace_root());
        assert!(
            violations.is_empty(),
            "平台 crate 不得依赖业务模块：{violations:?}"
        );
    }
}
