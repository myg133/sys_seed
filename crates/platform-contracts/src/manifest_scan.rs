//! C-2 的支撑：扫描模块源码里用到的权限点。
//!
//! 做法：读 `backend/` 下的 Rust 源码，抽出字符串字面量，
//! 挑出形如 `<module-id>:<resource>:<action>` 的那些 —— 这就是代码里用到的权限点。
//!
//! 为什么不走 AST：低配机对新增依赖敏感（syn 树会明显拖慢冷编译），
//! 而权限点的形态足够特殊（必须是三段小写编码），字面量扫描足够精确。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// 扫描 `backend_dir` 下的 Rust 源码，返回代码里用到的权限点编码（去重、排序）。
pub fn scan_permission_codes(backend_dir: &Path, module_id: &str) -> Result<Vec<String>> {
    let mut codes: Vec<String> = Vec::new();
    for file in rust_files(backend_dir)? {
        let source = std::fs::read_to_string(&file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        for literal in string_literals(&source) {
            if is_permission_code(&literal, module_id) && !codes.contains(&literal) {
                codes.push(literal);
            }
        }
    }
    codes.sort();
    Ok(codes)
}

/// 递归收集目录下的 `.rs` 文件。
fn rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }

    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("cannot read {}", current.display()))?;
        for entry in entries {
            let path = entry
                .with_context(|| format!("cannot read an entry under {}", current.display()))?
                .path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension() == Some(std::ffi::OsStr::new("rs")) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// 抽出源码里的字符串字面量（不含引号）。
fn string_literals(source: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut chars = source.chars();
    while let Some(ch) = chars.next() {
        if ch != '"' {
            continue;
        }
        let mut literal = String::new();
        while let Some(inner) = chars.next() {
            match inner {
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        literal.push(escaped);
                    }
                }
                '"' => break,
                _ => literal.push(inner),
            }
        }
        literals.push(literal);
    }
    literals
}

/// 是否是本模块的权限点编码：`module:resource:action`。
fn is_permission_code(value: &str, module_id: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 3 && parts[0] == module_id && parts[1..].iter().all(|part| is_segment(part))
}

/// 权限点的一段：`^[a-z][a-z0-9-]*$`。
fn is_segment(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_codes_are_recognised_only_for_the_given_module() {
        assert!(is_permission_code("sandbox:demo:read", "sandbox"));
        assert!(is_permission_code("sandbox:demo-item:read", "sandbox"));
        assert!(!is_permission_code("other:demo:read", "sandbox"));
        assert!(!is_permission_code("sandbox:demo", "sandbox"));
        assert!(!is_permission_code("sandbox:Demo:read", "sandbox"));
        assert!(!is_permission_code("sandbox:demo:read:extra", "sandbox"));
    }

    #[test]
    fn string_literals_are_extracted() {
        let literals = string_literals(r#"const A: &str = "sandbox:demo:read"; let B = "x";"#);
        assert_eq!(
            literals,
            vec!["sandbox:demo:read".to_string(), "x".to_string()]
        );
    }

    #[test]
    fn scanning_a_missing_directory_yields_no_codes() {
        let codes = scan_permission_codes(Path::new("/definitely/not/here"), "sandbox")
            .expect("扫描应成功");
        assert!(codes.is_empty());
    }
}
