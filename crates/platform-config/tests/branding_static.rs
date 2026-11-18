//! AC-S-055 的静态扫描：`apps/shell/**` 不得含**品牌具体值**，且必须真的从接口读。
//!
//! 判据（acceptance AC-S-055）：
//!
//! | 检查 | 期望 |
//! |------|------|
//! | `底座壳` / `#1677ff`（任意写法）/ `示例平台` / `#0055aa` 在 `apps/shell/**` | **0** |
//! | 品牌接口调用（`/api/config/branding`） | **≥ 1** |
//! | `e2e/shell.spec.ts` 仍断言 `底座壳` | **0** |
//!
//! `packages/theme` 的 `#1677ff` 是**允许保留**的兜底默认（rev 2 收窄），
//! 由运行时判据（渲染色 ≠ `#1677ff`）把关，不在这里扫。

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn collect_files(root: &Path, files: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(root).expect("目录应可读") {
        let entry = entry.expect("目录项");
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            // 构建产物与依赖目录不算源码。
            if matches!(name.as_str(), "dist" | "node_modules" | ".svelte-kit") {
                continue;
            }
            collect_files(&path, files);
        } else {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            files.push((path.display().to_string(), content));
        }
    }
}

fn shell_sources() -> Vec<(String, String)> {
    let mut files = Vec::new();
    collect_files(&repo_root().join("apps/shell"), &mut files);
    assert!(!files.is_empty(), "apps/shell 下应有源码");
    files
}

fn occurrences(files: &[(String, String)], needle: &str) -> Vec<String> {
    files
        .iter()
        .filter(|(_, content)| content.contains(needle))
        .map(|(path, _)| path.clone())
        .collect()
}

#[test]
fn the_shell_has_no_hardcoded_brand_values() {
    let files = shell_sources();
    for banned in ["底座壳", "#1677ff", "#1677FF", "示例平台", "#0055aa"] {
        let hits = occurrences(&files, banned);
        assert!(
            hits.is_empty(),
            "`{banned}` 是品牌具体值，不得出现在 apps/shell 源码里：{hits:?}"
        );
    }
}

#[test]
fn the_shell_really_reads_the_branding_endpoint() {
    let files = shell_sources();
    let hits = occurrences(&files, "/api/config/branding");
    assert!(
        !hits.is_empty(),
        "apps/shell 必须真的调用品牌接口（不是把品牌删掉）：{:?}",
        hits
    );
}

#[test]
fn the_document_title_is_not_hardcoded() {
    let html = std::fs::read_to_string(repo_root().join("apps/shell/index.html"))
        .expect("index.html 应可读");
    assert!(
        html.contains("<title></title>"),
        "index.html 的 <title> 必须留空、由运行时写入：{html}"
    );
}

#[test]
fn the_e2e_spec_asserts_the_runtime_title_instead_of_the_old_literal() {
    let spec = std::fs::read_to_string(repo_root().join("e2e/shell.spec.ts"))
        .expect("e2e/shell.spec.ts 应可读");
    assert!(!spec.contains("底座壳"), "e2e 不得再断言旧品牌值「底座壳」");
    assert!(
        spec.contains("branding"),
        "e2e 必须断言运行时标题（从品牌接口取值）"
    );
    assert!(
        spec.contains("toHaveTitle"),
        "e2e 必须断言浏览器标题（AC-S-055 步骤 2）"
    );
}
