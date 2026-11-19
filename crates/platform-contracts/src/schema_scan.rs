//! C-6 的支撑：扫描模块源码里的**投影调用点**。
//!
//! # 判据（design §4.4 已选定，不要加强）
//!
//! > 若某模块的 manifest 声明了任何带 `permission` 的字段，
//! > 则该模块源码里**必须出现** `SchemaProjector::project` 的调用。
//!
//! 这是「**至少一处**」判据，不是「每个 handler 都调用」——
//! 静态判定「哪个 handler 返回哪个实体」需要类型推导，在 Rust 源码扫描里不可靠，
//! 会**误报**，进而被人关掉。关掉之后**比没有更坏**：有个绿色的检查在那儿，
//! 让人以为守住了（REQ-009 的 C-5 就是这么吃过的）。
//!
//! **局限**（必须写在设计里，也已写进 `design-summary §4.4`）：
//!
//! - 「3 个 handler 只有 1 个调用」**能过** C-6；
//! - 缓解是靠 AC-S-080 的用例覆盖列表接口这条主路径，
//!   而不是把判据做得更脆。
//!
//! # 为什么不能只搜文本
//!
//! 注释里的 `SchemaProjector::project` 是文档，字符串里的也是数据 ——
//! 照抄文本就能过检查的话，C-6 与没有一样。所以先
//! [`strip_comments`]（保留字符串）再 [`blank_strings`]（只看代码）。
//!
//! [`strip_comments`]: crate::authz_scan::strip_comments
//! [`blank_strings`]: crate::authz_scan::blank_strings

use std::path::Path;

use anyhow::{Context, Result};

use crate::authz_scan::{blank_strings, rust_files, strip_comments};

/// 平台的投影入口类型名。
pub const PROJECTOR_TYPE: &str = "SchemaProjector";

/// 平台的投影方法名。
pub const PROJECT_METHOD: &str = "project";

/// 一次扫描的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectCallScan {
    /// 找到的调用点（`文件:行:列`）。
    pub calls: Vec<String>,
}

impl ProjectCallScan {
    /// 是否至少有一处调用。
    pub fn has_call(&self) -> bool {
        !self.calls.is_empty()
    }
}

/// 扫描 `source_dir` 下的 Rust 源码里的 `SchemaProjector::project` 调用点。
pub fn scan_project_calls(source_dir: &Path) -> Result<ProjectCallScan> {
    let mut scan = ProjectCallScan::default();
    for file in rust_files(source_dir)? {
        let source = std::fs::read_to_string(&file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        let stripped = strip_comments(&source);
        let code = blank_strings(&stripped);
        scan.calls
            .extend(find_project_calls(&code, &file, &stripped));
    }
    scan.calls.sort();
    scan.calls.dedup();
    Ok(scan)
}

/// 在一段**已剥注释、已抹字符串**的源码里找 `SchemaProjector::project(` 调用。
///
/// 位置（行:列）从**原始（剥注释后但保留字符串）**的文本算 ——
/// 行号是定位用的关键信息，两块文本的换行结构完全一致（见 `authz_scan::strip_comments`）。
fn find_project_calls(code: &str, file: &Path, located: &str) -> Vec<String> {
    let mut positions = Vec::new();
    let mut search_from = 0usize;

    while let Some(offset) = code[search_from..].find(PROJECTOR_TYPE) {
        let start = search_from + offset;
        search_from = start + PROJECTOR_TYPE.len();

        // 前后必须是标识符边界（`MySchemaProjector` 不算）。
        if start > 0 && is_ident_char(code.as_bytes()[start - 1] as char) {
            continue;
        }
        let after_type = start + PROJECTOR_TYPE.len();
        if code
            .as_bytes()
            .get(after_type)
            .is_some_and(|byte| is_ident_char(*byte as char))
        {
            continue;
        }

        // `::` → 可空白
        let Some(rest) = code[after_type..].trim_start().strip_prefix("::") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix(PROJECT_METHOD) else {
            continue;
        };
        if rest
            .as_bytes()
            .first()
            .is_some_and(|byte| is_ident_char(*byte as char))
        {
            continue;
        }
        // 一定是**调用**（后面跟 `(`），不是方法名出现在别的上下文。
        if !rest.trim_start().starts_with('(') {
            continue;
        }

        // 字符下标而不是字节下标：`blank_strings` 逐字符抹空白，**字符数**与
        // 原文本一致，字节数不一致（中文串会变短）。
        let char_offset = code[..start].chars().count();
        positions.push(position(file, located, char_offset));
    }

    positions
}

/// 源码里的位置（**行:列**，都从 1 起算）。
fn position(file: &Path, source: &str, char_offset: usize) -> String {
    let mut line = 1usize;
    let mut column = 1usize;
    for ch in source.chars().take(char_offset) {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    format!("{}:{}:{}", file.display(), line, column)
}

/// 标识符字符。
fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(source: &str) -> ProjectCallScan {
        let stripped = strip_comments(source);
        let code = blank_strings(&stripped);
        let mut scan = ProjectCallScan::default();
        scan.calls
            .extend(find_project_calls(&code, Path::new("demo.rs"), &stripped));
        scan
    }

    #[test]
    fn a_call_is_recognised() {
        let scan = scan(
            r#"
            fn handler(raw: Value) -> Value {
                SchemaProjector::project(raw, &schema, &ctx, checker.as_ref())
            }
        "#,
        );
        assert_eq!(scan.calls.len(), 1);
        assert!(scan.calls[0].starts_with("demo.rs:3:17"), "{:?}", scan.calls);
    }

    #[test]
    fn a_call_with_spaces_around_the_path_separator_is_recognised() {
        let scan = scan("fn f() { SchemaProjector :: project (raw) }");
        assert_eq!(scan.calls.len(), 1);
    }

    #[test]
    fn comments_are_not_call_sites() {
        let scan = scan(
            r#"
            /// 用 SchemaProjector::project 剔除字段
            // SchemaProjector::project(raw, &schema, &ctx, checker)
            /* SchemaProjector::project(raw) */
            fn handler() {}
        "#,
        );
        assert!(scan.calls.is_empty(), "{:?}", scan.calls);
    }

    #[test]
    fn string_literals_are_not_call_sites() {
        let scan = scan(r#"fn f() { let text = "SchemaProjector::project(raw)"; }"#);
        assert!(scan.calls.is_empty(), "{:?}", scan.calls);
    }

    #[test]
    fn the_type_name_alone_is_not_a_call_site() {
        let scan = scan("use platform_core::extension::schema::SchemaProjector;");
        assert!(scan.calls.is_empty(), "{:?}", scan.calls);
    }

    #[test]
    fn a_method_reference_without_parentheses_is_not_a_call_site() {
        let scan = scan("let f = SchemaProjector::project;");
        assert!(scan.calls.is_empty(), "{:?}", scan.calls);
    }

    #[test]
    fn a_longer_type_name_is_not_matched() {
        let scan = scan("fn f() { MySchemaProjector::project(raw) }");
        assert!(scan.calls.is_empty(), "{:?}", scan.calls);
    }

    #[test]
    fn a_definition_of_project_is_not_a_call_site() {
        let scan = scan("impl SchemaProjector { fn project(raw: Value) -> Value { raw } }");
        assert!(scan.calls.is_empty(), "{:?}", scan.calls);
    }
}
