//! 轻量 Rust 源码扫描：只认「trait 声明」这一种形态。
//!
//! # 为什么不走 AST
//!
//! 设计摘要 §8 明确要求：**不要**为了生成索引引入代码解析框架（如 `syn` 全量解析）。
//! 低配机对新增依赖敏感，而 trait 声明的形态足够规整 —— 一个 trait 名 + 一个方法体，
//! 用「扫关键字 + 配花括号」的约定就能可靠提取。
//!
//! # 认得什么
//!
//! | 形态 | 处理 |
//! |------|------|
//! | `pub trait Name: Super { ... }` | 提取 trait 名、超 trait、方法、关联类型 |
//! | 方法有默认实现（`{ ... }`） | 标记 `has_default` |
//! | 方法无默认实现（`;` 结尾） | `has_default = false` |
//! | `type Id: Send + Sync;` | 作为关联类型提取 |
//!
//! 认不得的（静默跳过）：宏体内的 trait、trait 内的嵌套 impl 块。
//! 平台扩展点目录里没有这两种形态，所以不影响本期的索引正确性。

use std::path::Path;

use anyhow::{Context, Result};

/// 一个 trait 声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitDecl {
    /// trait 名。
    pub name: String,
    /// 泛型参数（如 `<T: Send + Sync + 'static>`），无则为空串。
    pub generics: String,
    /// 超 trait 约束（如 `Send + Sync + 'static`）。
    pub supertraits: String,
    /// 文档注释摘要（第一段非空行）。
    pub description: String,
    /// 关联类型声明。
    pub associated_types: Vec<MemberDecl>,
    /// 方法声明。
    pub methods: Vec<MemberDecl>,
}

/// trait 里的一个成员（方法或关联类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberDecl {
    /// 成员名。
    pub name: String,
    /// 规范化后的声明文本（空白已归一）。
    pub declaration: String,
    /// 是否带默认实现。
    pub has_default: bool,
}

/// 扫描目录下的全部 `.rs` 文件，提取 trait 声明。
///
/// 返回结果按 trait 名排序 —— 索引必须是确定性的。
pub fn scan_traits(dir: &Path) -> Result<Vec<TraitDecl>> {
    let mut traits = Vec::new();
    for file in rust_files(dir)? {
        let source = std::fs::read_to_string(&file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        traits.extend(scan_source(&source));
    }
    traits.sort_by(|left, right| left.name.cmp(&right.name));
    traits.dedup_by(|left, right| left.name == right.name);
    Ok(traits)
}

/// 扫描一段源码里的 trait 声明。
pub fn scan_source(source: &str) -> Vec<TraitDecl> {
    let mut traits = Vec::new();
    let mut cursor = 0usize;

    while let Some(found) = source[cursor..].find("pub trait ") {
        let start = cursor + found;
        let after_keyword = start + "pub trait ".len();
        let name_len = ident_len(&source[after_keyword..]);
        if name_len == 0 {
            cursor = after_keyword;
            continue;
        }
        let name = source[after_keyword..after_keyword + name_len].to_string();
        let Some(open) = source[after_keyword + name_len..].find('{') else {
            break;
        };
        let open = after_keyword + name_len + open;
        let Some(close) = matching_brace(source, open) else {
            break;
        };

        let (generics, supertraits) =
            split_trait_head(&normalize_ws(&source[after_keyword + name_len..open]));
        let body = &source[open + 1..close];
        let (associated_types, methods) = parse_members(body);

        traits.push(TraitDecl {
            name,
            generics,
            supertraits,
            description: doc_above(source, start),
            associated_types,
            methods,
        });

        cursor = close + 1;
    }

    traits.sort_by(|left, right| left.name.cmp(&right.name));
    traits
}

/// 递归收集目录下的 `.rs` 文件（按路径排序）。
fn rust_files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
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

/// 标识符的字符长度（首字符为字母或下划线，其余为字母数字下划线）。
fn ident_len(text: &str) -> usize {
    let mut count = 0usize;
    for (index, ch) in text.char_indices() {
        let is_head = ch == '_' || ch.is_ascii_alphabetic();
        let is_tail = ch == '_' || ch.is_ascii_alphanumeric();
        if index == 0 {
            if !is_head {
                return 0;
            }
        } else if !is_tail {
            break;
        }
        count += ch.len_utf8();
    }
    count
}

/// 找出与 `open` 位置的花括号配对的 `}`。
fn matching_brace(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// 规范化空白：连续空白压成一个空格，并去掉标点处的多余空格。
///
/// 规则：`(` / `,` 后面不留空格，`,` / `)` / `;` / `>` 前面不留空格。
/// 这样多行签名会被压成一行可读的声明。
fn normalize_ws(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        // `)` 前面的尾逗号去掉（多行签名的收尾）
        if ch == ')' && out.ends_with(',') {
            out.pop();
        }
        let after_open = out.ends_with('(');
        if pending_space && !after_open && !matches!(ch, ',' | ')' | ';' | '>') {
            out.push(' ');
        }
        pending_space = false;
        out.push(ch);
    }
    out
}

/// 拆出 trait 头的泛型参数与超 trait 约束。
fn split_trait_head(text: &str) -> (String, String) {
    let text = text.trim();
    let (generics, rest) = match text.find('<') {
        Some(pos) => {
            let end = text[pos..]
                .find('>')
                .map(|offset| pos + offset + 1)
                .unwrap_or(text.len());
            (text[..end].to_string(), text[end..].to_string())
        }
        None => (String::new(), text.to_string()),
    };
    let supertraits = rest.trim_start_matches(':').trim().to_string();
    (generics, supertraits)
}

/// 抽取 trait 声明正上方的文档注释。
///
/// 从 `start`（`pub trait` 的位置）往上逐行找连续的 `///` 注释块。
fn doc_above(source: &str, start: usize) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut cursor = start;

    loop {
        let line_start = source[..cursor].rfind('\n').map(|pos| pos + 1).unwrap_or(0);
        if line_start == 0 {
            break;
        }
        let previous_end = line_start - 1;
        let previous_start = source[..previous_end]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let previous = source[previous_start..previous_end].trim();
        let Some(doc) = previous.strip_prefix("///") else {
            break;
        };
        lines.push(doc.trim());
        cursor = previous_start;
    }

    lines.reverse();
    lines
        .into_iter()
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// 解析 trait 体里的成员：返回（关联类型，方法）。
fn parse_members(body: &str) -> (Vec<MemberDecl>, Vec<MemberDecl>) {
    let mut associated_types = Vec::new();
    let mut methods = Vec::new();
    let mut cursor = 0usize;

    while cursor < body.len() {
        cursor = skip_trivia(body, cursor);
        if cursor >= body.len() {
            break;
        }
        let rest = &body[cursor..];
        if rest.starts_with("fn ") {
            let Some((member, next)) = parse_fn(body, cursor) else {
                break;
            };
            methods.push(member);
            cursor = next;
        } else if rest.starts_with("type ") {
            let Some(end) = body[cursor..].find(';') else {
                break;
            };
            let end = cursor + end;
            let declaration = normalize_ws(&body[cursor..end]);
            let name = declaration
                .strip_prefix("type ")
                .map(|rest| rest.split(':').next().unwrap_or_default().trim())
                .unwrap_or_default()
                .to_string();
            associated_types.push(MemberDecl {
                name,
                declaration,
                has_default: false,
            });
            cursor = end + 1;
        } else {
            cursor = skip_member(body, cursor);
        }
    }

    (associated_types, methods)
}

/// 跳过空白与注释。
fn skip_trivia(body: &str, mut cursor: usize) -> usize {
    loop {
        let rest = &body[cursor..];
        let trimmed = rest.trim_start();
        cursor += rest.len() - trimmed.len();
        if body[cursor..].starts_with("//") {
            let end = body[cursor..].find('\n').map(|pos| cursor + pos + 1);
            match end {
                Some(end) => cursor = end,
                None => return body.len(),
            }
            continue;
        }
        if body[cursor..].starts_with("/*") {
            let end = body[cursor..].find("*/").map(|pos| cursor + pos + 2);
            match end {
                Some(end) => cursor = end,
                None => return body.len(),
            }
            continue;
        }
        return cursor;
    }
}

/// 跳过一个不认识的成员（到下一个 `;` 或配对的花括号结束）。
fn skip_member(body: &str, cursor: usize) -> usize {
    let mut index = cursor;
    while index < body.len() {
        match body.as_bytes()[index] {
            b';' => return index + 1,
            b'{' => {
                return matching_brace(body, index)
                    .map(|end| end + 1)
                    .unwrap_or(body.len());
            }
            _ => index += 1,
        }
    }
    body.len()
}

/// 解析一个方法成员（从 `fn` 关键字开始）。
fn parse_fn(body: &str, cursor: usize) -> Option<(MemberDecl, usize)> {
    let name_len = ident_len(&body[cursor + 3..]);
    let name = body[cursor + 3..cursor + 3 + name_len].to_string();

    let mut index = cursor + 3 + name_len;
    let mut paren_depth = 0usize;
    while index < body.len() {
        match body.as_bytes()[index] {
            b'(' => paren_depth += 1,
            b')' => paren_depth -= 1,
            b'{' if paren_depth == 0 => {
                let end = matching_brace(body, index)?;
                return Some((
                    MemberDecl {
                        name,
                        declaration: normalize_ws(&body[cursor..index]),
                        has_default: true,
                    },
                    end + 1,
                ));
            }
            b';' if paren_depth == 0 => {
                return Some((
                    MemberDecl {
                        name,
                        declaration: normalize_ws(&body[cursor..index]),
                        has_default: false,
                    },
                    index + 1,
                ));
            }
            _ => {}
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
//! 模块级文档。
//
//! 第二段。

/// 扩展点 2：数据访问。
///
/// 更长的说明。
pub trait Repository<T: Send + Sync + 'static>: Send + Sync {
    /// 实体主键类型。
    type Id: Send + Sync;

    /// 按主键查询。
    fn find(
        &self,
        ctx: &RequestContext,
        id: &Self::Id,
    ) -> impl Future<Output = Result<Option<T>>> + Send;

    /// 列出数据。
    fn list(&self, ctx: &RequestContext, q: &ListQuery)
        -> impl Future<Output = Result<List<T>>> + Send;

    /// 默认实现。
    fn setup(&self, ctx: &ModuleContext) -> Result<()> {
        let _ = ctx;
        Ok(())
    }
}
"#;

    #[test]
    fn trait_name_and_supertraits_are_extracted() {
        let traits = scan_source(SOURCE);
        assert_eq!(traits.len(), 1);
        let repository = &traits[0];
        assert_eq!(repository.name, "Repository");
        assert_eq!(repository.generics, "<T: Send + Sync + 'static>");
        assert_eq!(repository.supertraits, "Send + Sync");
        assert_eq!(repository.description, "扩展点 2：数据访问。");
    }

    #[test]
    fn associated_types_are_extracted() {
        let traits = scan_source(SOURCE);
        let associated = &traits[0].associated_types;
        assert_eq!(associated.len(), 1);
        assert_eq!(associated[0].name, "Id");
        assert_eq!(associated[0].declaration, "type Id: Send + Sync");
    }

    #[test]
    fn methods_are_extracted_with_normalized_signatures() {
        let traits = scan_source(SOURCE);
        let methods = &traits[0].methods;
        assert_eq!(methods.len(), 3);
        assert_eq!(methods[0].name, "find");
        assert_eq!(
            methods[0].declaration,
            "fn find(&self, ctx: &RequestContext, id: &Self::Id) -> impl Future<Output = Result<Option<T>>> + Send"
        );
        assert!(!methods[0].has_default);
        assert_eq!(methods[1].name, "list");
        assert_eq!(
            methods[1].declaration,
            "fn list(&self, ctx: &RequestContext, q: &ListQuery) -> impl Future<Output = Result<List<T>>> + Send"
        );
        assert_eq!(methods[2].name, "setup");
        assert!(methods[2].has_default);
        assert_eq!(
            methods[2].declaration,
            "fn setup(&self, ctx: &ModuleContext) -> Result<()>"
        );
    }

    #[test]
    fn trait_head_is_split_into_generics_and_bounds() {
        assert_eq!(
            split_trait_head(": Send + Sync"),
            (String::new(), "Send + Sync".to_string())
        );
        assert_eq!(
            split_trait_head("<T: Send>: Send + Sync"),
            ("<T: Send>".to_string(), "Send + Sync".to_string())
        );
        assert_eq!(
            split_trait_head("<T: Send>"),
            ("<T: Send>".to_string(), String::new())
        );
    }

    #[test]
    fn a_source_without_traits_yields_nothing() {
        assert!(scan_source("pub struct Foo { a: u32 }").is_empty());
    }

    #[test]
    fn multiple_traits_are_sorted_by_name() {
        let source = "pub trait Zebra { fn a(&self); }\npub trait Alpha { fn b(&self); }\n";
        let traits = scan_source(source);
        let names: Vec<&str> = traits.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Zebra"]);
    }

    #[test]
    fn identifiers_stop_at_non_identifier_characters() {
        assert_eq!(ident_len("foo"), 3);
        assert_eq!(ident_len("foo<T>"), 3);
        assert_eq!(ident_len("foo_bar1"), 8);
        assert_eq!(ident_len("1foo"), 0);
        assert_eq!(ident_len(""), 0);
    }

    #[test]
    fn braces_are_matched_pairwise() {
        assert_eq!(matching_brace("{ a { b } c }", 0), Some(12));
        assert_eq!(matching_brace("{ a", 0), None);
    }
}
