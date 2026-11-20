//! 敏感键扫描器（REQ-022，D243 / AC-S-136 步骤 5 / AC-S-154 步骤 3）。
//!
//! # 为什么是**可单独调用的 `pub fn`**
//!
//! QA 的 S-17 指出：「全仓扫描 `client_secret` 的明文是否出现在任何响应构造点」
//! **没有可实现的宿主** —— Rust 源码里根本不会出现**运行时生成**的明文 secret，
//! 扫源码必然 0 命中（REQ-014 S-08 踩过的坑）。
//!
//! → 裁决（D243 / S-17 的建议改法）拆成**两条**：
//!
//! 1. **运行期断言**（**主判据**）：响应体逐字不含明文 secret —— 由测试直接断言；
//! 2. **静态扫描**（**辅判据**，**必须带阳性对照**）：本模块提供。
//!
//! 本模块的函数**不依赖任何 crate 内部状态** → 集成测试可以直接调用，
//! 也可以在**临时副本**上调用 → 阳性对照成立。
//!
//! # 三种扫描面
//!
//! | 函数 | 扫什么 | 判据来源 |
//! |---|---|---|
//! | [`scan_sensitive_keys`] | 一段文本里出现的**敏感键名** | AC-S-154 步骤 3（审计表全列） |
//! | [`scan_sensitive_fields`] | 若干列（列名 + 文本） | AC-S-154 步骤 3 |
//! | [`scan_response_source`] | Rust 源码里的**敏感 JSON 字段名** | AC-S-136 步骤 5（辅判据） |
//!
//! # ⚠️ 「完整键名匹配」**不是**子串匹配（变异验证点，D243）
//!
//! 敏感键集合里同时有 `code` 与 `code_verifier` / `device_code` / `user_code`。
//! 若用**子串**匹配，`code_verifier` 会连带命中 `code` → 误报。
//! → 本模块用**词边界**匹配：键名两侧必须是**非标识符字符**
//! （标识符字符 = `[A-Za-z0-9_]`）→ `code` **不会**命中 `code_verifier`，
//! `authorization` **不会**命中 `authorization_code`。
//!
//! # ⚠️ 静态扫描为什么扫 **JSON 名**（`clientSecret`）而不是 Rust 名（`client_secret`）
//!
//! - `client_secret` 作为 **Rust 字段名**在本 crate 里**合法地存在一次** ——
//!   注册接口的**一次性**明文交付（D200 / AC-S-135 的 `clientSecret` 键）。
//!   扫 Rust 名会让判据**恒红** → 判据不可用。
//! - 本 crate 的所有响应体都由 `#[serde(rename_all = "camelCase")]` 派生构造
//!   → **响应体里出现字面量 `clientSecret`** 只可能来自**手写的 JSON 字面量**
//!   （`json!({...})` / `Json(...)`）→ 那**正是**要防的泄漏面。
//! - ⚠️ 静态扫描**只是辅判据**：读接口是否回显 secret 由**运行期断言**把关
//!   （AC-S-136 步骤 3 / 4）。
//!
//! # ⚠️ 不用 `RandomState`
//!
//! 哈希种子不确定 → 命中顺序不可复现 → 断言不可复现（dev-guide §9.2）。
//! 本模块**完全不用哈希表**，命中列表按**敏感键集合的固定顺序**输出。

use std::fmt;

/// 敏感键的**固定顺序**清单（AC-S-154 步骤 3 要求的 10 个**至少**）。
///
/// ⚠️ 顺序**钉死** → 命中列表可复现、可逐字断言。
pub const SENSITIVE_KEYS: &[&str] = &[
    "client_secret",
    "clientSecret",
    "code",
    "code_verifier",
    "code_challenge",
    "device_code",
    "user_code",
    "access_token",
    "refresh_token",
    "logout_token",
    "authorization",
    "password",
];

/// 敏感键清单（**别名**，方便按「清单」语义调用）。
pub fn sensitive_keys() -> &'static [&'static str] {
    SENSITIVE_KEYS
}

/// 一处命中（哪个键、在哪一列 / 哪一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveHit {
    /// 命中的**敏感键名**。
    pub key: &'static str,
    /// 命中位置（列名 / 文件名）。
    pub location: String,
    /// 该处的**行号**（**从 1 开始**；文本扫描时为 `None`）。
    pub line: Option<usize>,
}

impl fmt::Display for SensitiveHit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{}: {}", self.location, line, self.key),
            None => write!(f, "{}: {}", self.location, self.key),
        }
    }
}

/// 在一段文本里找出出现的**敏感键名**。
///
/// # 匹配口径：**完整键名**（词边界），**不是**子串
///
/// - `code_verifier` → **只**命中 `code_verifier`，**不**命中 `code`；
/// - `authorization_code` → **不**命中 `authorization`。
///
/// # 顺序
///
/// 命中列表按 [`SENSITIVE_KEYS`] 的**固定顺序**返回（不是出现顺序）→ 可复现。
pub fn scan_sensitive_keys(text: &str) -> Vec<&'static str> {
    let mut hits = Vec::new();
    for key in SENSITIVE_KEYS {
        if contains_whole_key(text, key) {
            hits.push(*key);
        }
    }
    hits
}

/// 在若干列（列名 + 文本）里扫描敏感键 —— **审计表的全列**（AC-S-154 步骤 3）。
///
/// - `fields` 的每一项是 `(列名, 该列的文本)`；
/// - 返回**所有**命中（同一列可命中多个键）。
pub fn scan_sensitive_fields(fields: &[(&str, &str)]) -> Vec<SensitiveHit> {
    let mut hits = Vec::new();
    for (column, text) in fields {
        for key in scan_sensitive_keys(text) {
            hits.push(SensitiveHit {
                key,
                location: (*column).to_string(),
                line: None,
            });
        }
    }
    hits
}

/// 文本里是否**逐字**含有某个**明文值**（运行期断言用，AC-S-136 步骤 4）。
///
/// ⚠️ 空串**永远**判「含有」是错的 → 空串一律判「**不**含有」。
pub fn contains_secret(text: &str, secret: &str) -> bool {
    if secret.is_empty() {
        return false;
    }
    text.contains(secret)
}

/// 扫 Rust 源码里的**敏感 JSON 字段名**（AC-S-136 步骤 5 的静态扫描）。
///
/// # 扫描面
///
/// 只在**响应构造点**上判定 —— 判据是这一行里**同时**出现：
///
/// 1. 响应构造的标记（`json!` / `Json(` / `#[serde(` / `Serialize` / `Response`）；
/// 2. 一个敏感键的 **JSON 名**（`clientSecret` 等）。
///
/// # 为什么只扫 JSON 名
///
/// 见模块文档：Rust 字段名 `client_secret` 在**一次性交付**的响应里合法存在一次，
/// 扫它会让判据恒红；本 crate 的响应体一律由 serde 派生构造，
/// **字面量** `clientSecret` 只可能来自手写 JSON → 那正是要防的泄漏面。
///
/// # 返回
///
/// 每个命中带**行号**（从 1 开始）→ 阳性对照能指认「命中位置包含被塞入的那个文件」。
pub fn scan_response_source(location: &str, source: &str) -> Vec<SensitiveHit> {
    let mut hits = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if !is_response_construction_line(line) {
            continue;
        }
        for key in SENSITIVE_KEYS {
            if !is_json_name(key) {
                continue;
            }
            if contains_whole_key(line, key) {
                hits.push(SensitiveHit {
                    key,
                    location: location.to_string(),
                    line: Some(index + 1),
                });
            }
        }
    }
    hits
}

/// 这一行是不是**响应构造点**。
fn is_response_construction_line(line: &str) -> bool {
    const MARKERS: &[&str] = &[
        "json!",
        "Json(",
        "#[serde(",
        "Serialize",
        "Response",
        "to_response",
    ];
    MARKERS.iter().any(|marker| line.contains(marker))
}

/// 只有 **camelCase / snake_case 的 JSON 名**才进静态扫描面。
///
/// ⚠️ `code` / `authorization` 这类**单词**不进静态扫描面 ——
/// 它们在源码里太常见（变量名、注释、路由片段）→ 静态扫描会变成噪音。
/// 静态扫描只认**复合名**（含 `_` 或含大写字母）。
fn is_json_name(key: &str) -> bool {
    key.contains('_') || key.chars().any(|c| c.is_ascii_uppercase())
}

/// 文本里是否**完整**含有这个键名（**词边界**匹配，**不是**子串）。
///
/// # 词边界的定义
///
/// 键名两侧必须是**非标识符字符**；标识符字符 = `[A-Za-z0-9_.]`。
///
/// ⚠️ **`.` 也算标识符字符** —— 平台的**事件码**是**点分**的
/// （`oauth.authorization.approved`，D215）→ 若 `.` 当分隔符，
/// `authorization` 会在这条事件码里**整词命中** → 每次授权都误报。
///
/// 而 JSON 里的键两侧是 `"` / `{` / `,` → **不是**标识符字符 → 照常命中 ✓。
fn contains_whole_key(text: &str, key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    let bytes = text.as_bytes();
    let needle = key.as_bytes();
    let mut start = 0usize;
    while let Some(found) = text[start..].find(key) {
        let at = start + found;
        let before_ok = at == 0 || !is_identifier_byte(bytes[at - 1]);
        let after = at + needle.len();
        let after_ok = after >= bytes.len() || !is_identifier_byte(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
        start = at + 1;
        if start >= bytes.len() {
            break;
        }
    }
    false
}

/// 是否标识符字符（`[A-Za-z0-9_.]`）—— `serde` 字段名里**不含**这些。
fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-S-154 步骤 3 要求的**至少** 10 个键**必须**在清单里。
    #[test]
    fn the_key_list_covers_the_required_keys() {
        for required in [
            "client_secret",
            "code",
            "code_verifier",
            "device_code",
            "user_code",
            "access_token",
            "refresh_token",
            "logout_token",
            "authorization",
            "password",
        ] {
            assert!(
                SENSITIVE_KEYS.contains(&required),
                "敏感键清单缺少 {required}"
            );
        }
    }

    /// **完整键名**匹配：`code` **不**命中 `code_verifier`。
    #[test]
    fn the_matching_is_on_whole_keys_not_substrings() {
        let hits = scan_sensitive_keys("{\"code_verifier\":\"abc\"}");
        assert_eq!(hits, vec!["code_verifier"]);

        // `authorization_code`：`authorization` 后面跟着 `_`、`code` 前面也是 `_`
        // → 两个都不是**完整键名** → 0 命中。
        let hits = scan_sensitive_keys("{\"authorization_code\":\"abc\"}");
        assert!(hits.is_empty(), "复合键名不该拆出子键：{hits:?}");
    }

    /// **点分**的事件码（D215）**不**命中其中的单词。
    ///
    /// ⚠️ `oauth.authorization.approved` 是**平台钉死**的事件码 → 若 `.` 当分隔符，
    /// 每一次授权都会误报 `authorization` → 判据变成噪音 → 不可用。
    #[test]
    fn a_dotted_event_code_does_not_hit_a_single_word_key() {
        for code in [
            "oauth.authorization.approved",
            "oauth.authorization.denied",
            "oauth.token.issued",
            "oauth.client.registered",
            "oauth.logout.executed",
        ] {
            let hits = scan_sensitive_keys(code);
            assert!(hits.is_empty(), "事件码 {code} 不该命中敏感键：{hits:?}");
        }
    }

    /// 阳性对照：含明文 secret 的样本**必须**命中。
    #[test]
    fn a_sample_with_a_secret_is_hit() {
        let hits = scan_sensitive_keys("{\"client_secret\":\"POSITIVE-CONTROL\"}");
        assert!(
            hits.contains(&"client_secret"),
            "阳性对照失败：扫描器没命中含明文 secret 的样本"
        );
    }

    /// 干净样本 → 0 命中。
    #[test]
    fn a_clean_sample_has_no_hit() {
        let hits =
            scan_sensitive_keys("{\"action\":\"oauth.token.issued\",\"objectType\":\"token\"}");
        assert!(hits.is_empty(), "干净样本不该有命中：{hits:?}");
    }

    /// 列扫描：命中带**列名**。
    #[test]
    fn the_column_scan_reports_the_column_name() {
        let hits = scan_sensitive_fields(&[
            ("action", "oauth.logout.executed"),
            ("after", "{\"logout_token\":\"abc\"}"),
        ]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].key, "logout_token");
        assert_eq!(hits[0].location, "after");
    }

    /// 静态扫描：源码里的敏感 JSON 名**必须**被命中（阳性对照）。
    #[test]
    fn the_source_scan_hits_a_response_field_name() {
        let source = "#[derive(Serialize)]\nstruct X {\n    #[serde(rename = \"clientSecret\")]\n    pub v: String,\n}\n";
        let hits = scan_response_source("sample.rs", source);
        assert!(
            hits.iter().any(|hit| hit.key == "clientSecret"),
            "静态扫描没命中响应构造点里的敏感字段名：{hits:?}"
        );
        assert_eq!(hits[0].line, Some(3));
    }

    /// 静态扫描：非响应构造行**不**算命中。
    #[test]
    fn the_source_scan_ignores_non_response_lines() {
        let source = "// 注释里提到 clientSecret，但**不是**响应构造点。\n";
        assert!(scan_response_source("comment.rs", source).is_empty());
    }

    /// 明文值判定：空串一律「**不**含有」。
    #[test]
    fn the_plaintext_check_rejects_an_empty_secret() {
        assert!(!contains_secret("anything", ""));
        assert!(contains_secret("the secret is s3cr3t", "s3cr3t"));
        assert!(!contains_secret("nothing here", "s3cr3t"));
    }

    /// §5.5：静态扫描**必须带阳性对照** —— 先证明扫描器会命中，再说真实代码 0 命中。
    ///
    /// 顺序很重要：**先**跑阳性对照（往**临时副本**里塞一个 `clientSecret` 响应字段
    /// → 扫描器**必须**命中），**再**断言本 crate 的真实源码 **0 命中**。
    /// 反过来写就成了「扫描器坏了所以 0 命中」的假绿。
    #[test]
    fn the_crate_source_has_no_sensitive_response_field() {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let src_dir = crate_dir.join("src");
        let mut files = Vec::new();
        collect_rust_files(&src_dir, &mut files);
        files.sort();
        assert!(!files.is_empty(), "应当能扫到本 crate 的源码");

        // 阳性对照：把一个 `clientSecret` 响应字段塞进**临时副本**。
        let first = std::fs::read_to_string(&files[0]).expect("源码应当可读");
        let poisoned = format!(
            "{first}\nfn poisoned() {{\n    let body = json!({{ \"clientSecret\": \"s3cr3t\" }});\n}}\n"
        );
        let control = scan_response_source("阳性对照.rs", &poisoned);
        assert!(
            control.iter().any(|hit| hit.key == "clientSecret"),
            "阳性对照必须命中（否则下面的 0 命中是空跑）：{control:?}"
        );

        // 真实代码：0 命中。
        //
        // ⚠️ 扫描面 = **非测试代码**：`#[cfg(test)]` 段里**必然**含 `clientSecret`
        // （那正是阳性对照的素材）→ 把测试段算进来会自证失败。
        let mut hits = Vec::new();
        for file in &files {
            let source = std::fs::read_to_string(file).expect("源码应当可读");
            let location = file
                .strip_prefix(crate_dir)
                .unwrap_or(file)
                .display()
                .to_string();
            hits.extend(scan_response_source(&location, production_part(&source)));
        }
        assert!(hits.is_empty(), "真实源码不该有敏感响应字段：{hits:?}");
    }

    /// 截掉 `#[cfg(test)]` 段（测试夹具不算生产代码）。
    fn production_part(source: &str) -> &str {
        for (index, line) in source.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                let offset = source
                    .lines()
                    .take(index)
                    .map(|line| line.len() + 1)
                    .sum::<usize>();
                return &source[..offset.min(source.len())];
            }
        }
        source
    }

    /// 递归收集 `*.rs`（只扫本 crate 的 `src/`）。
    fn collect_rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
}
