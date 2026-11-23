//! `trace_id` 的生成、校验与注入（REQ-019，D340）。
//!
//! # 格式
//!
//! **32 位小写 hex**（16 字节随机）。与 OTel 的 `TraceId` 天然一致
//! （OTel 也是 16 字节 / 32 hex）→ 可以直接用入站值作为 OTel trace_id（AC-S-037）。
//!
//! ⚠️ 这是**格式变更**：`platform-core` 原来的 `new_trace_id()` 是
//! `Uuid::new_v4().to_string()`（**36 字符带连字符**），
//! 且有单测断言 `len()==36`（`context.rs:156`）→ 实现时必须同步改
//! （`design-summary.md` §12 已点名该测试）。

pub mod middleware;

/// trace_id 的十六进制长度（16 字节 → 32 hex）。
pub const TRACE_ID_HEX_LEN: usize = 32;

/// 生成一个新的 trace_id（32 位小写 hex）。
///
/// # 为什么不用 uuid 的 `to_string()`
///
/// `Uuid::new_v4().to_string()` 是 **36 字符带连字符**（`8-4-4-4-12`），
/// 与 OTel 的 `TraceId`（32 hex）不一致 → 要做字符串转换才能对接，
/// 而且日志里带连字符会让按 trace 搜索变难（运维直接 `grep` 32 hex 更快）。
///
/// 这里直接取 16 字节随机 → hex，**与 OTel 同构**。
pub fn new_trace_id() -> String {
    let bytes: [u8; 16] = rand_bytes();
    to_hex(&bytes)
}

/// 校验并归一入站的 `X-Request-Id`（D340 / AC-S-030）。
///
/// # 规则（顺序不可换）
///
/// 1. **先判**「全 ASCII hex」—— 这一步就排除了所有多字节 UTF-8；
/// 2. **再判**长度 == 32 —— 此时字节数 == 字符数，`len()` 无歧义；
/// 3. 归一为**小写**（大写输入被接受，但输出统一小写）。
///
/// # 为什么顺序很重要
///
/// `str::len()` 是**字节数**。若先判 `len()==32`，一个含多字节 UTF-8 的串
/// （如 `"é".repeat(16)` 恰好 32 字节但只有 16 字符）会通过长度检查；
/// 虽然第 2 步的 hex 检查最终也会拒它，但**先 ASCII 后长度**的写法
/// 让「为什么安全」不需要额外论证（D340 挑刺 m3 的修正）。
///
/// # 返回
///
/// - `Some(小写 32 hex)`：入站值合法，**沿用**
/// - `None`：非法（太短 / 太长 / 含非 hex 字符 / 含控制字符）→ 调用方应**生成新的**
pub fn normalize_trace_id(raw: &str) -> Option<String> {
    // ① 先判 ASCII hex —— 排除所有多字节 UTF-8 与非 hex 字符
    //    （包括 `/`、`.`、空格、`\r`、`\n`、ESC 等）。
    if !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    // ② 再判长度：此时字节数 == 字符数（全 ASCII）。
    if raw.len() != TRACE_ID_HEX_LEN {
        return None;
    }
    Some(raw.to_ascii_lowercase())
}

/// 从入站头取值并归一；非法则生成新的（AC-S-030 的整体语义）。
///
/// 这是中间件实际调用的入口。
pub fn resolve_trace_id(incoming: Option<&str>) -> String {
    incoming
        .and_then(normalize_trace_id)
        .unwrap_or_else(new_trace_id)
}

/// 把字节转成小写 hex。
fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// 取 16 字节随机（不新增依赖 —— 用 `uuid` 的两个 v4 拼）。
///
/// ⚠️ 为什么不用 `rand` crate：那是新依赖（本项目零依赖约束）。
/// `uuid::Uuid::new_v4()` 内部用的就是系统 CSPRNG，取它的 16 字节即可。
fn rand_bytes() -> [u8; 16] {
    *uuid::Uuid::new_v4().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TC-019-001：生成的 trace_id 是 32 位小写 hex，且多次不重复。
    #[test]
    fn a_generated_trace_id_is_32_lowercase_hex() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..100 {
            let id = new_trace_id();
            assert_eq!(id.len(), TRACE_ID_HEX_LEN, "必须是 32 位：{id}");
            assert!(
                id.bytes().all(|b| b.is_ascii_hexdigit()),
                "必须全是 hex：{id}"
            );
            assert!(
                !id.bytes().any(|b| b.is_ascii_uppercase()),
                "必须是小写：{id}"
            );
            seen.insert(id);
        }
        assert_eq!(seen.len(), 100, "100 次不应有重复（16 字节随机）");
    }

    /// ⚠️ 生成的**不是** uuid 形态（36 字符带连字符）—— 这是格式变更的判据。
    #[test]
    fn a_generated_trace_id_is_not_the_uuid_shape() {
        let id = new_trace_id();
        assert_ne!(id.len(), 36, "不得是 uuid 的 36 字符形态");
        assert!(!id.contains('-'), "不得含连字符：{id}");
    }

    /// TC-019-006：合法 32 hex 被沿用（逐字）。
    #[test]
    fn a_valid_incoming_trace_id_is_kept_verbatim() {
        let raw = "0123456789abcdef0123456789abcdef";
        assert_eq!(normalize_trace_id(raw), Some(raw.to_string()));
    }

    /// TC-019-007：大写被归一为小写（**不是**原样透传）。
    #[test]
    fn an_uppercase_incoming_trace_id_is_lowercased() {
        let raw = "ABCDEF0123456789ABCDEF0123456789";
        let got = normalize_trace_id(raw).expect("大写应被接受");
        assert_eq!(got, "abcdef0123456789abcdef0123456789");
        assert_ne!(got, raw, "必须归一，不能原样透传");
    }

    /// TC-019-008：太短被拒。
    #[test]
    fn a_too_short_trace_id_is_rejected() {
        assert_eq!(normalize_trace_id("abc"), None);
        assert_eq!(normalize_trace_id(""), None);
    }

    /// 太长被拒。
    #[test]
    fn a_too_long_trace_id_is_rejected() {
        let long = "a".repeat(33);
        assert_eq!(normalize_trace_id(&long), None);
    }

    /// TC-019-009：路径穿越字符被拒。
    #[test]
    fn a_path_traversal_trace_id_is_rejected() {
        assert_eq!(normalize_trace_id("../../etc/passwd"), None);
        assert_eq!(normalize_trace_id("/0123456789abcdef0123456789abcdef"), None);
    }

    /// TC-019-010：**CRLF 注入被拒**（安全核心，与路径穿越是两个向量）。
    #[test]
    fn a_crlf_injection_trace_id_is_rejected() {
        // 恰好 32 个 'a' + CRLF + 内容 → 长度超了，且含非 hex
        let bad = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\nFAKE LOG LINE";
        assert_eq!(normalize_trace_id(bad), None);
        // 也测一个**长度恰好 32 但含 \r** 的（只判长度会漏）
        let bad32 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n";
        assert_eq!(
            normalize_trace_id(bad32),
            None,
            "含 CRLF 的串即使长度对也必须拒（否则日志注入）"
        );
    }

    /// TC-019-011：长度对但含非 hex 字符被拒。
    #[test]
    fn a_32_length_but_non_hex_trace_id_is_rejected() {
        let g32 = "g".repeat(32);
        assert_eq!(normalize_trace_id(&g32), None, "非 hex 必须拒");
        // 含 `z`（非 hex）
        let mixed = "0123456789abcdef0123456789abcdez";
        assert_eq!(normalize_trace_id(mixed), None);
    }

    /// TC-019-012：**多字节 UTF-8 被拒**（32 字节但 16 字符）。
    #[test]
    fn a_multibyte_utf8_trace_id_is_rejected() {
        let bad = "é".repeat(16);
        assert_eq!(bad.len(), 32, "构造前提：32 字节");
        assert_eq!(
            normalize_trace_id(&bad),
            None,
            "多字节 UTF-8 必须拒 —— 只判 len()==32 的实现会让它通过"
        );
    }

    /// 含控制字符（ESC）被拒。
    #[test]
    fn a_control_character_trace_id_is_rejected() {
        let bad = "0123456789abcdef0123456789abcde\x1b";
        assert_eq!(normalize_trace_id(bad), None);
    }

    /// `resolve_trace_id`：合法沿用、非法生成、缺失生成。
    #[test]
    fn resolve_keeps_valid_and_generates_otherwise() {
        let valid = "0123456789abcdef0123456789abcdef";
        assert_eq!(resolve_trace_id(Some(valid)), valid);

        let generated = resolve_trace_id(Some("bad"));
        assert_ne!(generated, "bad");
        assert_eq!(generated.len(), TRACE_ID_HEX_LEN);

        let generated_none = resolve_trace_id(None);
        assert_eq!(generated_none.len(), TRACE_ID_HEX_LEN);
    }

    /// 归一后的值一定是 32 小写 hex（闭合性）。
    #[test]
    fn normalization_is_closed_over_its_own_output() {
        for raw in [
            "0123456789abcdef0123456789abcdef",
            "ABCDEF0123456789ABCDEF0123456789",
        ] {
            let once = normalize_trace_id(raw).expect("应通过");
            let twice = normalize_trace_id(&once).expect("归一结果必须仍是合法输入");
            assert_eq!(once, twice, "归一必须幂等");
        }
    }
}
