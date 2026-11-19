//! C-5 的支撑：扫描模块源码里的**鉴权调用点**。
//!
//! # 为什么不能沿用 C-2 的字面量扫描
//!
//! C-2 扫的是「源码里出现的、形如 `module:resource:action` 的**字符串字面量**」。
//! 它**分不出**「这是权限点常量」与「这是鉴权调用点」——
//!
//! ```text
//! const PERMISSION_DEMO_READ: &str = "sandbox:demo:read";   // 只是常量
//! require_permission("sandbox:demo:read")                    // 真的鉴权了
//! ```
//!
//! 两者对 C-2 来说**一模一样**。若 C-5 沿用 C-2 的逻辑，
//! 一个只定义常量、从不鉴权的模块会**假绿** —— 而那正是 v1 §4 要关掉的缺口。
//!
//! 所以 C-5 扫的是**调用**：函数名 + 括号 + **字面量参数**。
//!
//! # 三条判定
//!
//! | 情况 | 判定 |
//! |------|------|
//! | 已登记，且有 `require_permission("字面量")` 调用点 | ✅ |
//! | 已登记，但**没有任何**鉴权调用点 | ❌ **失败**（登记了却不执行 = 假权限） |
//! | 调用点的参数**不是字面量** | ❌ **失败**（无法静态确认 → fail closed） |
//! | 出现**第二个**鉴权入口（模块自己写一个） | ❌ **失败**（扫描会被绕过） |
//!
//! # 入口必须唯一
//!
//! 「唯一」指的是**定义处唯一**（当前是 `crates/platform-core`）——
//! 业务模块**必须调用平台那一个**；自己写一个 `check_perm()` 就是绕过。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// 平台的鉴权入口函数名。
pub const AUTH_ENTRY_POINT: &str = "require_permission";

/// 平台的鉴权入口定义处（相对仓库根）。
pub const PLATFORM_ENTRY_FILE: &str = "crates/platform-core/src/extension/permission.rs";

/// 「看起来像鉴权入口」的其它函数名 —— 模块自己写一个就绕过了扫描。
pub const AUTH_ENTRY_ALIASES: [&str; 8] = [
    "check_permission",
    "check_perm",
    "has_permission",
    "require_perm",
    "authorize",
    "ensure_permission",
    "verify_permission",
    "permission_check",
];

/// 一次扫描的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthCallScan {
    /// 通过 `require_permission("字面量")` 绑定的权限点（去重、排序）。
    pub codes: Vec<String>,
    /// 参数**不是字面量**的调用点（fail closed）。
    pub non_literal_calls: Vec<String>,
    /// 绕过平台入口的自定义鉴权入口（`文件:行:列`）。
    pub custom_entries: Vec<String>,
}

/// 扫描模块后端源码里的鉴权调用点。
pub fn scan_auth_calls(backend_dir: &Path) -> Result<AuthCallScan> {
    let mut scan = AuthCallScan::default();
    for file in rust_files(backend_dir)? {
        let source = std::fs::read_to_string(&file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        let stripped = strip_comments(&source);
        scan_source(&stripped, &file, &mut scan);
    }
    scan.codes.sort();
    scan.codes.dedup();
    scan.non_literal_calls.sort();
    scan.non_literal_calls.dedup();
    scan.custom_entries.sort();
    scan.custom_entries.dedup();
    Ok(scan)
}

/// 扫描一段源码（**注释已被剥掉**）。
fn scan_source(source: &str, file: &Path, scan: &mut AuthCallScan) {
    let bytes: Vec<char> = source.chars().collect();
    let mut index = 0usize;
    while index < bytes.len() {
        let ch = bytes[index];
        if !ch.is_ascii_alphabetic() && ch != '_' {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == '_') {
            index += 1;
        }
        let name: String = bytes[start..index].iter().collect();

        // 前一个字符是 `.` / `:` → 是方法调用或路径调用，不是自由函数调用。
        let preceded_by_path = start > 0 && (bytes[start - 1] == '.' || bytes[start - 1] == ':');
        // 前一个字符是标识符字符 → 是某个更长标识符的一部分（已被词法切分排除）。
        let is_definition = is_followed_by_fn_keyword(&bytes, start);

        // 下一个非空白字符是不是 `(`？
        let mut cursor = index;
        while cursor < bytes.len() && bytes[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != '(' {
            continue;
        }

        if name == AUTH_ENTRY_POINT {
            if is_definition {
                // 模块自己定义了鉴权入口 → 绕过平台入口。
                scan.custom_entries.push(position(file, &bytes, start));
                continue;
            }
            if preceded_by_path {
                continue;
            }
            match first_argument(&bytes, cursor) {
                Some(Some(literal)) => scan.codes.push(literal),
                Some(None) => scan.non_literal_calls.push(position(file, &bytes, start)),
                None => {}
            }
            continue;
        }

        // 别名：定义了它，或自由调用了它 → 都是「绕过平台入口」。
        if AUTH_ENTRY_ALIASES.contains(&name.as_str()) && (is_definition || !preceded_by_path) {
            scan.custom_entries.push(position(file, &bytes, start));
        }
    }
}

/// 源码里的位置（**行:列**，都从 1 起算）。
///
/// ⚠️ **不是字符偏移** —— 字符偏移报出来的是「文件里有第几个字符」，
/// 人根本没法定位（QA 实测报出 `lib.rs:96`，而那个文件只有 3 行）。
///
/// 行号是**精确**的：注释被剥掉时**换行结构被保留**（见 [`strip_comments`]），
/// 所以剥注释前后的行号一致。列号在同一行有块注释时可能偏小 ——
/// 行号才是定位用的关键信息。
fn line_and_column(source: &[char], offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for ch in source.iter().take(offset) {
        if *ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// 拼出「`文件:行:列`」。
fn position(file: &Path, source: &[char], offset: usize) -> String {
    let (line, column) = line_and_column(source, offset);
    format!("{}:{}:{}", file.display(), line, column)
}

/// 标识符前面是不是 `fn`（即它是**定义**而不是调用）。
fn is_followed_by_fn_keyword(bytes: &[char], start: usize) -> bool {
    let mut cursor = start;
    while cursor > 0 && bytes[cursor - 1].is_whitespace() {
        cursor -= 1;
    }
    if cursor < 2 {
        return false;
    }
    bytes[cursor - 2] == 'f' && bytes[cursor - 1] == 'n'
}

/// 取第一个实参：`Some(Some(字面量))` / `Some(None)`（非字面量）/ `None`（取不到）。
fn first_argument(bytes: &[char], open_paren: usize) -> Option<Option<String>> {
    let mut index = open_paren + 1;
    while index < bytes.len() && bytes[index].is_whitespace() {
        index += 1;
    }
    if index >= bytes.len() {
        return None;
    }
    if bytes[index] != '"' {
        return Some(None);
    }
    // 读字符串字面量（处理转义）。
    index += 1;
    let mut literal = String::new();
    while index < bytes.len() {
        let ch = bytes[index];
        if ch == '\\' {
            if let Some(escaped) = bytes.get(index + 1) {
                literal.push(*escaped);
                index += 2;
                continue;
            }
        }
        if ch == '"' {
            return Some(Some(literal));
        }
        literal.push(ch);
        index += 1;
    }
    Some(None)
}

/// 剥掉注释（**保留字符串字面量**）。
///
/// 注释里出现的 `require_permission("...")` 是文档，不是调用点 ——
/// 不剥掉就会把文档当成调用点（或反过来制造假阳性）。
pub fn strip_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0usize;
    let mut in_string = false;
    let mut in_char = false;

    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            out.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.get(index + 1) {
                    out.push(*next);
                    index += 2;
                    continue;
                }
            }
            if ch == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if in_char {
            out.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.get(index + 1) {
                    out.push(*next);
                    index += 2;
                    continue;
                }
            }
            if ch == '\'' {
                in_char = false;
            }
            index += 1;
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            index += 1;
            continue;
        }
        if ch == '\'' {
            // 生命周期（`'a`）不是字符字面量 —— 不能进入 in_char，否则会把
            // 后面一大段代码当成字符内容吞掉。
            if starts_char_literal(&chars, index) {
                in_char = true;
            }
            out.push(ch);
            index += 1;
            continue;
        }
        // 行注释
        if ch == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        // 块注释
        if ch == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            let mut depth = 1usize;
            while index < chars.len() && depth > 0 {
                if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                    depth += 1;
                    index += 2;
                    continue;
                }
                if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    depth -= 1;
                    index += 2;
                    continue;
                }
                if chars[index] == '\n' {
                    out.push('\n');
                }
                index += 1;
            }
            continue;
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// 递归收集目录下的 `.rs` 文件。
pub(crate) fn rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
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

/// 从模块后端目录往上找仓库根（能找到 `crates/platform-core` 的那一层）。
pub fn find_repo_root(backend_dir: &Path) -> Option<PathBuf> {
    let mut current = backend_dir.to_path_buf();
    loop {
        if current.join(PLATFORM_ENTRY_FILE).is_file() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// 全仓库的**鉴权入口定义处**。
///
/// 「唯一」指的是**定义处唯一** —— 业务模块自己写一个 `check_perm()` 就是绕过。
pub fn scan_entry_definitions(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let mut definitions = Vec::new();
    for dir in [repo_root.join("crates"), repo_root.join("modules")] {
        for file in rust_files(&dir)? {
            let source = std::fs::read_to_string(&file)
                .with_context(|| format!("cannot read {}", file.display()))?;
            let stripped = strip_comments(&source);
            if defines_auth_entry(&stripped) {
                definitions.push(file);
            }
        }
    }
    definitions.sort();
    Ok(definitions)
}

/// 源码里是否**定义**了鉴权入口。
///
/// ⚠️ **只看代码，不看字符串**：测试里写着 `"pub fn require_permission(...)"` 的
/// 字符串字面量**不是**定义 —— 否则扫描器会把自己判成「第二个入口」。
fn defines_auth_entry(source: &str) -> bool {
    let code = blank_strings(source);
    let bytes: Vec<char> = code.chars().collect();
    let needle: Vec<char> = format!("fn {AUTH_ENTRY_POINT}").chars().collect();
    let mut index = 0usize;
    while index + needle.len() <= bytes.len() {
        if bytes[index..index + needle.len()] == needle[..] {
            let before_ok = index == 0 || !is_ident_char(bytes[index - 1]);
            let after = bytes.get(index + needle.len());
            let after_ok = after.is_none_or(|ch| !is_ident_char(*ch));
            if before_ok && after_ok {
                return true;
            }
        }
        index += 1;
    }
    false
}

/// `chars[index] == '\''` 是否开始一个**字符字面量**（而不是生命周期）。
///
/// Rust 的 `'a`（生命周期）与 `'a'`（字符字面量）在源码里都以 `'` 开头。
/// 旧实现把所有 `'` 都当成字符字面量 → 遇到 `fn f<'a>(...)` 时会把后面
/// 一大段代码当成「字符内容」吞掉。REQ-010 在 `permission.rs` 里新增了带
/// 生命周期的 trait 后，C-5 一度扫不到 `fn require_permission`（假绿 → 假红）。
fn starts_char_literal(chars: &[char], index: usize) -> bool {
    match chars.get(index + 1) {
        Some('\\') => {
            // 转义形式：`'\n'` / `'\''` / `'\\'` / `'\u{1F600}'` —— 向后找闭合的 `'`。
            let mut cursor = index + 3;
            let limit = (index + 14).min(chars.len());
            while cursor < limit {
                if chars[cursor] == '\'' {
                    return true;
                }
                cursor += 1;
            }
            false
        }
        Some(_) => chars.get(index + 2) == Some(&'\''),
        None => false,
    }
}

/// 把**字符串字面量的内容**换成空格（长度不变）。
///
/// 用于「只看代码」的扫描：字符串里的 `fn require_permission` 不是定义。
pub(crate) fn blank_strings(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0usize;
    let mut in_string = false;
    let mut in_char = false;

    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            if ch == '\\' {
                out.push(' ');
                if let Some(next) = chars.get(index + 1) {
                    out.push(if *next == '\n' { '\n' } else { ' ' });
                }
                index += 2;
                continue;
            }
            if ch == '"' {
                in_string = false;
                out.push('"');
            } else {
                out.push(if ch == '\n' { '\n' } else { ' ' });
            }
            index += 1;
            continue;
        }
        if in_char {
            if ch == '\\' {
                out.push(' ');
                if let Some(next) = chars.get(index + 1) {
                    out.push(if *next == '\n' { '\n' } else { ' ' });
                }
                index += 2;
                continue;
            }
            if ch == '\'' {
                in_char = false;
                out.push('\'');
            } else {
                out.push(if ch == '\n' { '\n' } else { ' ' });
            }
            index += 1;
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push('"');
            index += 1;
            continue;
        }
        if ch == '\'' {
            if starts_char_literal(&chars, index) {
                in_char = true;
            }
            out.push('\'');
            index += 1;
            continue;
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// 标识符字符。
fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_with_a_literal_is_recognised() {
        let source = r#"
            fn handler() {
                router.route_layer(require_permission("sandbox:demo:read"));
            }
        "#;
        let stripped = strip_comments(source);
        let mut scan = AuthCallScan::default();
        scan_source(&stripped, Path::new("demo.rs"), &mut scan);
        assert_eq!(scan.codes, vec!["sandbox:demo:read".to_string()]);
        assert!(scan.non_literal_calls.is_empty());
    }

    #[test]
    fn a_call_with_a_constant_is_reported_as_non_literal() {
        let source = r#"
            fn handler() {
                router.route_layer(require_permission(PERMISSION_DEMO_READ));
            }
        "#;
        let stripped = strip_comments(source);
        let mut scan = AuthCallScan::default();
        scan_source(&stripped, Path::new("demo.rs"), &mut scan);
        assert!(scan.codes.is_empty());
        assert_eq!(scan.non_literal_calls.len(), 1, "非字面量必须被报出来");
    }

    #[test]
    fn a_permission_constant_is_not_a_call_site() {
        // **这正是 C-2 与 C-5 的分工**：C-2 看字面量，C-5 看调用。
        let source = r#"
            pub const PERMISSION_DEMO_READ: &str = "sandbox:demo:read";
        "#;
        let stripped = strip_comments(source);
        let mut scan = AuthCallScan::default();
        scan_source(&stripped, Path::new("demo.rs"), &mut scan);
        assert!(scan.codes.is_empty(), "常量不是鉴权调用点");
        assert!(scan.non_literal_calls.is_empty());
    }

    #[test]
    fn a_custom_entry_point_is_reported() {
        let source = r#"
            fn check_perm(code: &str) -> bool { true }
            fn handler() { check_perm("sandbox:demo:read"); }
        "#;
        let stripped = strip_comments(source);
        let mut scan = AuthCallScan::default();
        scan_source(&stripped, Path::new("demo.rs"), &mut scan);
        assert!(
            !scan.custom_entries.is_empty(),
            "自定义鉴权入口必须被报出来"
        );
    }

    #[test]
    fn comments_are_stripped_so_documentation_is_not_a_call_site() {
        let source = r#"
            /// 用 `require_permission("sandbox:demo:read")` 绑定权限点。
            // require_permission("sandbox:demo:write")
            /* require_permission("sandbox:demo:export") */
            fn handler() {}
        "#;
        let stripped = strip_comments(source);
        let mut scan = AuthCallScan::default();
        scan_source(&stripped, Path::new("demo.rs"), &mut scan);
        assert!(scan.codes.is_empty(), "注释里的不是调用点");
        assert!(scan.non_literal_calls.is_empty());
    }

    #[test]
    fn a_method_call_is_not_a_free_function_call() {
        let source = r#"
            fn handler() { self.require_permission("sandbox:demo:read"); }
        "#;
        let stripped = strip_comments(source);
        let mut scan = AuthCallScan::default();
        scan_source(&stripped, Path::new("demo.rs"), &mut scan);
        assert!(scan.codes.is_empty());
    }

    /// 生命周期（`'a`）不是字符字面量：不能把后面的代码吞掉。
    ///
    /// 这是 REQ-010 暴露的真实回归：`permission.rs` 新增了带生命周期的
    /// `PermissionRefresher` trait 后，`fn require_permission` 之前出现 `'a`，
    /// 旧实现把它当字符字面量开始 → 后面整段被当成「字符内容」→
    /// C-5 报「入口定义 0 处」。
    #[test]
    fn lifetimes_do_not_swallow_the_following_code() {
        let source = "pub trait T { fn f<'a>(&'a self); }\n\
                      pub fn require_permission(code: impl Into<String>) -> X { }";
        let stripped = strip_comments(source);
        assert!(
            defines_auth_entry(&stripped),
            "生命周期不得让入口定义消失：{stripped}"
        );
        assert!(
            blank_strings(&stripped).contains("fn require_permission"),
            "生命周期不得让后面的代码被当成字符串/字符内容"
        );
    }

    /// 真正的字符字面量仍然要被当成字符（不误判成生命周期）。
    #[test]
    fn real_char_literals_are_still_handled() {
        let source = "fn f() { let a = '\''; let b = 'x'; }\n\
                      pub fn require_permission(code: impl Into<String>) -> X { }";
        let stripped = strip_comments(source);
        assert!(
            defines_auth_entry(&stripped),
            "字符字面量后面的入口定义必须仍能被扫到：{stripped}"
        );
    }

    #[test]
    fn the_entry_definition_is_detected() {
        assert!(defines_auth_entry(
            "pub fn require_permission(code: impl Into<String>) -> X { }"
        ));
        assert!(!defines_auth_entry(
            "pub fn require_permissions(code: &str) { }"
        ));
        assert!(!defines_auth_entry("let x = require_permission(\"a\");"));
    }

    /// 违例位置必须是**行:列**，不是字符偏移 —— 报错要能直接定位到那一行。
    ///
    /// ⚠️ 曾经的写法是 `format!("{}:{}", file, start)`，`start` 是**字符下标**：
    /// 报出来的是「文件里第几个字符」，人没法定位（QA 实测报出 `lib.rs:96`，
    /// 而那个文件只有 3 行）。
    #[test]
    fn a_violation_points_at_the_line_it_is_on() {
        let source = "fn handler() {}\nfn check_perm(code: &str) -> bool { true }\n";
        let stripped = strip_comments(source);
        let mut scan = AuthCallScan::default();
        scan_source(&stripped, Path::new("demo.rs"), &mut scan);
        assert_eq!(scan.custom_entries.len(), 1, "自定义入口必须被报出来");
        assert_eq!(
            scan.custom_entries[0], "demo.rs:2:4",
            "位置必须是行:列（要能直接定位到那一行）：{}",
            scan.custom_entries[0]
        );
    }

    /// 非字面量调用的位置同样是**行:列**。
    #[test]
    fn a_non_literal_call_points_at_the_line_it_is_on() {
        let source = "fn handler() {\n    require_permission(PERM);\n}\n";
        let stripped = strip_comments(source);
        let mut scan = AuthCallScan::default();
        scan_source(&stripped, Path::new("demo.rs"), &mut scan);
        assert_eq!(scan.non_literal_calls.len(), 1);
        assert_eq!(
            scan.non_literal_calls[0], "demo.rs:2:5",
            "位置必须是行:列：{}",
            scan.non_literal_calls[0]
        );
    }
}
