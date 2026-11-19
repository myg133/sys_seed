//! 静态扫描：`ProblemDetails` 的 `title` **不许**是非 ASCII 字面量（REQ-021 / AC-S-088）。
//!
//! # 为什么 rev 1 的验收方式必然漏报
//!
//! rev 1 扫的是「`ProblemDetails` 构造点上的中文字面量」，但第一轮漏网的
//! 12 处构造点**全是跨行**的：
//!
//! ```text
//! ProblemDetails::from_context(
//!     platform_core::problem::CODE_INTERNAL_ERROR,
//!     "服务内部错误",          ← title 是**第 2 个实参**，且在**下一行**
//!     ...
//! );
//! ```
//!
//! 按行匹配的扫法看不到第 2 行 → 扫完 0 命中、全部通过（假绿）。
//! 而且那一轮还把 `modules/**` 划在了扫描范围之外 —— 12 处里有 **10 处在
//! `modules/sandbox/`**。
//!
//! 所以本测试的判据是**词法级**的：
//!
//! 1. **剥注释**（`//` 与 `/* */`，含嵌套）—— 注释里的中文不是响应体文案；
//! 2. **跨行匹配**：先把源码切成 token 流（空白与换行都被丢掉），
//!    再在 token 流上找「`from_context(` 的第 2 个实参」与「`title:` 的值」；
//! 3. 命中条件：那个位置上是**字符串字面量**且**非 ASCII**。
//!
//! 因为判定发生在 token 流上，**跨不跨行都一样**（换行只是被丢掉的空白）。
//!
//! # 阳性对照（防「扫描恒不命中」的假绿）
//!
//! [`the_scanner_detects_an_injected_non_ascii_title`] 把**同一段扫描器**
//! 喂进**故意注入**的非 ASCII title（含**同行**、**跨行**、`title:` 三种位置），
//! 断言它**必须命中**；[`the_scanner_ignores_non_title_positions`] 断言
//! 日志 / 参数 / 注释里的中文**不会**被误报。
//! 扫描器若恒不命中，正对照就会红 —— 两种「不命中」因此可区分。
//!
//! # 扫描范围
//!
//! `crates/` 与 `modules/` 下的**全部** `.rs` 文件（`modules/` 必须在内 ——
//! 第一轮正是在这里漏了 10 处）。断言里钉死了「`modules/` 确实被扫到了」，
//! 防止扫描范围被悄悄改小后又变成假绿。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ token 层

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    Str,
    Ident,
    Punct,
}

#[derive(Debug, Clone)]
struct Tok {
    kind: TokKind,
    /// `Str` = 字面量内容（不含引号与转义处理）；`Ident` = 标识符；`Punct` = 单个符号。
    text: String,
    /// 1 起始的行号。
    line: usize,
}

/// 一次命中：**非 ASCII 字符串字面量**出现在 title 位置。
#[derive(Debug, Clone, PartialEq)]
struct Hit {
    line: usize,
    kind: &'static str,
    text: String,
}

/// 把源码切成 token 流：**丢掉注释与空白，保留字符串字面量**。
///
/// 自己写词法分析而不是用正则：正则做不到「剥掉注释里的 `//` 但保留字符串里的
/// `//`」，也做不到「认得出 `r#"..."#` 原始字符串」。
fn tokenize(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut toks: Vec<Tok> = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;

    while i < n {
        let c = chars[i];

        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // -- 行注释 --
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // -- 块注释（Rust 允许嵌套）--
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            let mut depth = 0usize;
            while i < n {
                if chars[i] == '\n' {
                    line += 1;
                }
                if chars[i] == '/' && i + 1 < n && chars[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                    continue;
                }
                if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                i += 1;
            }
            continue;
        }

        // -- 原始字符串：r"..." / r#"..."# / br#"..."# --
        if let Some((content, next)) = scan_raw_string(&chars, i) {
            line += count_newlines(&chars[i..next]);
            toks.push(Tok {
                kind: TokKind::Str,
                text: content,
                line,
            });
            i = next;
            continue;
        }

        // -- 普通字符串 --
        if c == '"' {
            let (content, next) = scan_string(&chars, i);
            line += count_newlines(&chars[i..next]);
            toks.push(Tok {
                kind: TokKind::Str,
                text: content,
                line,
            });
            i = next;
            continue;
        }

        // -- 字符字面量 / 生命周期 --
        if c == '\'' {
            // 字符字面量：'\n'
            if i + 1 < n && chars[i + 1] == '\\' {
                let mut j = i + 2;
                while j < n && chars[j] != '\'' {
                    if chars[j] == '\n' {
                        line += 1;
                    }
                    j += 1;
                }
                i = (j + 1).min(n);
                continue;
            }
            // 字符字面量：'x'
            if i + 2 < n && chars[i + 2] == '\'' {
                i += 3;
                continue;
            }
            // 生命周期（'static / 'a）：只跳过引号。
            i += 1;
            continue;
        }

        // -- 标识符 --
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            toks.push(Tok {
                kind: TokKind::Ident,
                text: chars[i..j].iter().collect(),
                line,
            });
            i = j;
            continue;
        }

        // -- 其它符号 --
        toks.push(Tok {
            kind: TokKind::Punct,
            text: c.to_string(),
            line,
        });
        i += 1;
    }

    toks
}

/// 普通字符串字面量（`"..."`，处理 `\` 转义）。
fn scan_string(chars: &[char], start: usize) -> (String, usize) {
    let n = chars.len();
    let mut j = start + 1;
    let mut content = String::new();
    while j < n {
        let ch = chars[j];
        if ch == '\\' && j + 1 < n {
            content.push(ch);
            content.push(chars[j + 1]);
            j += 2;
            continue;
        }
        if ch == '"' {
            break;
        }
        content.push(ch);
        j += 1;
    }
    (content, (j + 1).min(n))
}

/// 原始字符串字面量（`r"..."` / `r#"..."#` / `br#"..."#`）。
///
/// 不是原始字符串时返回 `None`（调用方继续走普通分支）。
fn scan_raw_string(chars: &[char], start: usize) -> Option<(String, usize)> {
    let n = chars.len();
    let mut i = start;
    if i < n && (chars[i] == 'b' || chars[i] == 'B') {
        i += 1;
    }
    if i >= n || (chars[i] != 'r' && chars[i] != 'R') {
        return None;
    }
    i += 1;
    let mut hashes = 0usize;
    while i < n && chars[i] == '#' {
        hashes += 1;
        i += 1;
    }
    if i >= n || chars[i] != '"' {
        return None;
    }
    let mut j = i + 1;
    let mut content = String::new();
    while j < n {
        if chars[j] == '"' {
            let mut k = j + 1;
            let mut seen = 0usize;
            while k < n && seen < hashes && chars[k] == '#' {
                seen += 1;
                k += 1;
            }
            if seen == hashes {
                return Some((content, k));
            }
        }
        content.push(chars[j]);
        j += 1;
    }
    // 未闭合（源码本身有问题）：当作到文件末尾。
    Some((content, n))
}

fn count_newlines(chars: &[char]) -> usize {
    chars.iter().filter(|c| **c == '\n').count()
}

// ------------------------------------------------------------------ 扫描层

/// 一个 `{` 的上下文。
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// 这个 `{` 是不是 `ProblemDetails` 的**结构体字面量**（错误体的 `title` 就在里面）。
    is_pd_literal: bool,
    /// 这个 `{` 是不是 `impl ProblemDetails { ... }`（`Self { ... }` 在里面）。
    is_pd_impl: bool,
}

/// 在 token 流上找 title 位置上的非 ASCII 字面量。
///
/// 「title 位置」= **错误体**的 title，两条路径：
///
/// 1. `ProblemDetails::from_context(<code>, <title>, ...)` / `ProblemDetails::new(...)`
///    的**第 2 个实参** —— 构造函数路径，与括号怎么换行无关；
/// 2. `ProblemDetails { ..., title: <字面量>, ... }` 的 `title` 字段
///    —— 结构体字面量路径。
///
/// **刻意不算**业务实体的 `title` 字段（如 `TenantItem { title: "同名记录" }`）：
/// 那是**用户数据**，本来就该本地化（契约 C6 的 title 约束只管错误体）。
fn scan_tokens(toks: &[Tok]) -> Vec<Hit> {
    let mut hits: Vec<Hit> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut pending_impl_pd = false;

    for (idx, tok) in toks.iter().enumerate() {
        // -- 花括号上下文 --
        if tok.kind == TokKind::Punct {
            match tok.text.as_str() {
                "{" => {
                    let is_pd_impl = pending_impl_pd;
                    pending_impl_pd = false;
                    let prev_is_pd = prev_ident(toks, idx);
                    let is_pd_literal = if is_pd_impl {
                        false
                    } else {
                        match prev_is_pd.as_deref() {
                            Some("ProblemDetails") => true,
                            Some("Self") => frames.last().map(|f| f.is_pd_impl).unwrap_or(false),
                            _ => false,
                        }
                    };
                    frames.push(Frame {
                        is_pd_literal,
                        is_pd_impl,
                    });
                    continue;
                }
                "}" => {
                    frames.pop();
                    continue;
                }
                _ => {}
            }
        }

        if tok.kind != TokKind::Ident {
            continue;
        }

        // -- `impl ProblemDetails { ... }`（记给下一个 `{`）--
        if tok.text == "impl" {
            pending_impl_pd = impl_block_is_problem_details(toks, idx);
            continue;
        }

        // (a) `ProblemDetails::from_context(<code>, <title>, ...)`
        //     以及 `ProblemDetails::new(<code>, <title>, ...)`（绕过 from_context 的后门）。
        if tok.text == "from_context" || tok.text == "new" {
            if tok.text == "new" && !is_problem_details_new(toks, idx, &frames) {
                continue;
            }
            if let Some(open) = call_open_paren(toks, idx) {
                if let Some(arg) = call_argument(toks, open, 2) {
                    if let Some(first) = arg.first() {
                        if first.kind == TokKind::Str && !first.text.is_ascii() {
                            hits.push(Hit {
                                line: first.line,
                                kind: "from_context",
                                text: first.text.clone(),
                            });
                        }
                    }
                }
            }
            continue;
        }

        // (b) `title: <值>` —— 只在**错误体**的结构体字面量里才算。
        if tok.text == "title" && frames.last().map(|f| f.is_pd_literal).unwrap_or(false) {
            let Some(colon) = toks.get(idx + 1) else {
                continue;
            };
            if colon.kind != TokKind::Punct || colon.text != ":" {
                continue;
            }
            let Some(value) = toks.get(idx + 2) else {
                continue;
            };
            if value.kind == TokKind::Str && !value.text.is_ascii() {
                hits.push(Hit {
                    line: value.line,
                    kind: "title:",
                    text: value.text.clone(),
                });
            }
        }
    }

    hits
}

/// `idx` 前面那个标识符（没有则 `None`）。
fn prev_ident(toks: &[Tok], idx: usize) -> Option<String> {
    if idx == 0 {
        return None;
    }
    let prev = &toks[idx - 1];
    if prev.kind == TokKind::Ident {
        Some(prev.text.clone())
    } else {
        None
    }
}

/// `impl ProblemDetails { ... }` 吗？
fn impl_block_is_problem_details(toks: &[Tok], impl_idx: usize) -> bool {
    let mut i = impl_idx + 1;
    while i < toks.len() {
        let t = &toks[i];
        if t.kind == TokKind::Punct && (t.text == "{" || t.text == ";") {
            return false;
        }
        if t.kind == TokKind::Ident && t.text == "ProblemDetails" {
            return true;
        }
        i += 1;
    }
    false
}

/// `new` 是不是**错误体**的构造函数（`ProblemDetails::new` / PD impl 里的 `Self::new`）。
fn is_problem_details_new(toks: &[Tok], idx: usize, frames: &[Frame]) -> bool {
    if idx < 2 {
        return false;
    }
    let prev = &toks[idx - 1];
    if prev.kind != TokKind::Punct || prev.text != ":" {
        return false;
    }
    let owner = &toks[idx - 2];
    if owner.kind != TokKind::Ident {
        return false;
    }
    match owner.text.as_str() {
        "ProblemDetails" => true,
        "Self" => frames.last().map(|f| f.is_pd_impl).unwrap_or(false),
        _ => false,
    }
}

/// 找到调用实参列表的左括号（允许 turbofish：`::<T>`）。
fn call_open_paren(toks: &[Tok], ident_idx: usize) -> Option<usize> {
    let mut i = ident_idx + 1;
    while i < toks.len() {
        let t = &toks[i];
        if t.kind == TokKind::Punct && t.text == "(" {
            return Some(i);
        }
        // turbofish 允许出现的 token：`::<T>` 里的 `: < > ,` 与标识符。
        let turbofish_ok = (t.kind == TokKind::Punct
            && (t.text == ":" || t.text == "<" || t.text == ">" || t.text == ","))
            || t.kind == TokKind::Ident;
        if !turbofish_ok {
            return None;
        }
        i += 1;
    }
    None
}

/// 取调用的第 `n` 个实参（1 起始）的 token 序列。
fn call_argument(toks: &[Tok], open_idx: usize, n: usize) -> Option<Vec<&Tok>> {
    let mut depth = 0i32;
    let mut arg_index = 0usize;
    let mut arg_start = open_idx + 1;
    for i in (open_idx + 1)..toks.len() {
        let t = &toks[i];
        if t.kind != TokKind::Punct {
            continue;
        }
        match t.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                if depth == 0 {
                    // 调用结束（没有第 n 个实参）。
                    break;
                }
                depth -= 1;
            }
            "," if depth == 0 => {
                arg_index += 1;
                if arg_index == n - 1 {
                    arg_start = i + 1;
                } else if arg_index == n {
                    return Some(toks[arg_start..i].iter().collect());
                }
            }
            _ => {}
        }
    }
    None
}

// ------------------------------------------------------------------ 仓库扫描

/// 仓库根目录（测试进程的 cwd 是 crate 根，往上两级才是仓库根）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/platform-server 的上两级应当是仓库根")
        .to_path_buf()
}

/// 递归收集 `.rs` 文件（跳过 `target/` / `node_modules/` / `.git/`）。
fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            collect_rust_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// 全仓扫描的结果。
struct RepoScan {
    /// 扫过的文件总数。
    files: usize,
    /// 扫过的、位于 `modules/` 下的文件数。
    files_in_modules: usize,
    /// 命中的字面量（相对仓库根的路径 + 行号）。
    hits: Vec<(String, Hit)>,
}

/// 扫全仓 `crates/` 与 `modules/` 下的所有 `.rs` 文件。
fn scan_repository(root: &Path) -> RepoScan {
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in ["crates", "modules"] {
        collect_rust_files(&root.join(dir), &mut files);
    }
    files.sort();

    let mut scan = RepoScan {
        files: 0,
        files_in_modules: 0,
        hits: Vec::new(),
    };

    for path in &files {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        scan.files += 1;
        let relative = path.strip_prefix(root).unwrap_or(path);
        if relative
            .components()
            .next()
            .map(|c| c.as_os_str() == "modules")
            == Some(true)
        {
            scan.files_in_modules += 1;
        }
        let toks = tokenize(&src);
        for hit in scan_tokens(&toks) {
            scan.hits.push((relative.display().to_string(), hit));
        }
    }

    scan
}

// ------------------------------------------------------------------ 测试

/// 仓库里**不允许**出现「title 位置上的非 ASCII 字面量」。
///
/// 断言用 `assert!` 把命中的**文件:行号**一并打出来 —— 只报「有 N 处命中」
/// 等于没报。
#[test]
fn no_non_ascii_literal_sits_in_a_title_position() {
    let root = repo_root();
    let scan = scan_repository(&root);

    // 扫描范围不能是空的（否则「0 命中」是废话）。
    assert!(
        scan.files >= 40,
        "扫到的 .rs 文件太少（{}），扫描范围可能没生效",
        scan.files
    );
    // `modules/` 必须真的被扫到 —— 第一轮正是在这里漏了 10 处。
    assert!(
        scan.files_in_modules >= 5,
        "`modules/` 下只扫到 {} 个 .rs 文件，扫描范围可能漏了业务模块",
        scan.files_in_modules
    );

    let details: Vec<String> = scan
        .hits
        .iter()
        .map(|(file, hit)| format!("{file}:{} [{}] {:?}", hit.line, hit.kind, hit.text))
        .collect();

    println!(
        "扫描范围：crates/ + modules/ 共 {} 个 .rs 文件（其中 modules/ 下 {} 个）；\
         剥注释后 title 位置的中文字面量命中数 = {}",
        scan.files,
        scan.files_in_modules,
        scan.hits.len()
    );
    for detail in &details {
        println!("  命中：{detail}");
    }

    assert!(
        scan.hits.is_empty(),
        "title 位置还有 {} 处非 ASCII 字面量：\n{}",
        scan.hits.len(),
        details.join("\n")
    );
}

/// 阳性对照：同一段扫描器**必须**能命中注入的非 ASCII title。
///
/// 三个位置各来一次：**同行**、**跨行**、`title:` 字段。
/// 跨行那一例是第一轮 12 处漏网的**原样形状** —— 如果扫描器退化成按行匹配，
/// 这一例就会漏，测试立刻变红。
#[test]
fn the_scanner_detects_an_injected_non_ascii_title() {
    let cases: [(&str, &str); 3] = [
        ("同行", SINGLE_LINE),
        ("跨行", MULTI_LINE),
        ("title: 字段", TITLE_FIELD),
    ];

    for (label, source) in cases {
        let hits = scan_tokens(&tokenize(source));
        assert!(
            !hits.is_empty(),
            "阳性对照失效（{label}）：扫描器对注入的非 ASCII title **没有命中** —— \
             扫描恒不命中与扫描生效长得一样，这个对照就是用来区分它们的"
        );
        assert!(
            hits.iter().any(|hit| !hit.text.is_ascii()),
            "阳性对照失效（{label}）：命中的不是非 ASCII 字面量：{hits:?}"
        );
        println!(
            "阳性对照（{label}）：注入 1 处非 ASCII title → 扫描器命中 {} 处：{:?}",
            hits.len(),
            hits.iter()
                .map(|hit| (hit.line, hit.kind, hit.text.clone()))
                .collect::<Vec<_>>()
        );
    }
}

/// 阴性对照：中文在**非 title 位置**（日志 / 参数 / 注释）时**不许**被命中。
///
/// 防的是「过度整改」与「扫得太宽导致假红」。
#[test]
fn the_scanner_ignores_non_title_positions() {
    let hits = scan_tokens(&tokenize(NON_TITLE_POSITIONS));
    assert!(
        hits.is_empty(),
        "阴性对照失效：非 title 位置被误报了 {hits:?}"
    );
}

/// 阴性对照：业务实体的 `title` 是**用户数据**（该本地化），**不许**被命中。
#[test]
fn the_scanner_ignores_business_data_titles() {
    let hits = scan_tokens(&tokenize(BUSINESS_DATA_TITLE));
    assert!(
        hits.is_empty(),
        "阴性对照失效：业务数据的 title 被误报了 {hits:?}"
    );
}

/// 阴性对照：ASCII 的 title 字面量**允许**存在（不误报）。
#[test]
fn the_scanner_accepts_ascii_title_literals() {
    let hits = scan_tokens(&tokenize(ASCII_TITLE));
    assert!(
        hits.is_empty(),
        "阴性对照失效：ASCII 字面量被误报了 {hits:?}"
    );
}

/// 仓库里 `ProblemDetails` 的 title **必须**来自 `TITLES`（运行时的兜底判据）。
///
/// 静态扫描只管「不许有非 ASCII 字面量」；这一条管「取到的确实是表里的值」。
#[test]
fn title_literals_would_resolve_to_the_titles_table() {
    // `platform_core::titles::title_of_code` 对表内码返回表里的值。
    assert_eq!(
        platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR),
        "Internal Server Error"
    );
    // `title_of_code` 永远返回 ASCII（表的值是 ASCII，兜底也是 ASCII）。
    for code in [
        "INTERNAL_ERROR",
        "NOT_FOUND",
        "DATABASE_ERROR",
        "RLS_POLICY_MISSING",
    ] {
        let title = platform_core::titles::title_of_code(code);
        assert!(title.is_ascii(), "`{code}` 的 title 不是 ASCII：{title}");
    }
    // 表里的每一行都必须是 ASCII（与 `platform-core` 内部的测试同款判据）。
    for (code, title) in platform_core::titles::TITLES {
        assert!(
            title.is_ascii(),
            "TITLES 里出现非 ASCII 字符：{code} → {title}"
        );
    }
    // 所有被登记的码都**不等于**码本身（否则等于把机器码漏给用户）。
    let registered: BTreeSet<&str> = platform_core::titles::TITLES
        .iter()
        .map(|(code, _)| *code)
        .collect();
    assert!(
        registered.len() == platform_core::titles::TITLES.len(),
        "TITLES 里有重复键"
    );
}

// ------------------------------------------------------------------ 阳性对照样本

/// 阳性对照：同行。
const SINGLE_LINE: &str = r#"
fn handler() {
    let p = ProblemDetails::from_context("INTERNAL_ERROR", "服务内部错误", 500);
}
"#;

/// 阳性对照：**跨行**（第一轮 12 处漏网的原样形状）。
const MULTI_LINE: &str = r#"
fn handler() {
    let p = ProblemDetails::from_context(
        platform_core::problem::CODE_INTERNAL_ERROR,
        "服务内部错误",
        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        "the permission cache could not be refreshed",
        &ctx,
        &instance,
        None,
    );
}
"#;

/// 阳性对照：`title:` 字段位置。
const TITLE_FIELD: &str = r#"
fn handler() {
    let p = ProblemDetails {
        code: "INTERNAL_ERROR",
        title: "服务内部错误",
        status: 500,
    };
}
"#;

/// 阴性对照：中文在**非 title 位置**。
const NON_TITLE_POSITIONS: &str = r#"
// 注释里的中文不是响应体文案，不该被命中。
fn handler() {
    tracing::error!("服务内部错误");
    let p = ProblemDetails::from_context(
        "INTERNAL_ERROR",
        title_of_code("INTERNAL_ERROR"),
        500,
    );
}
"#;

/// 阴性对照：业务实体的 `title` 是**用户数据**，本来就该本地化，不该被命中。
///
/// （`modules/sandbox/tests/c3_real_db.rs` 里就有 `TenantItem { title: "同名记录" }`。）
const BUSINESS_DATA_TITLE: &str = r#"
fn seed() {
    let item = TenantItem {
        tenant_id: tenant.to_string(),
        id: id.to_string(),
        title: "同名记录".to_string(),
        revision: 0,
    };
}
"#;

/// 阴性对照：ASCII 的 title 字面量。
const ASCII_TITLE: &str = r#"
fn handler() {
    let p = ProblemDetails::from_context("INTERNAL_ERROR", "Internal Server Error", 500);
    let q = ProblemDetails {
        code: "INTERNAL_ERROR",
        title: "Internal Server Error",
        status: 500,
    };
}
"#;
