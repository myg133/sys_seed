//! client 认证：`client_secret_basic`（REQ-022，D207）。
//!
//! # 认证方式
//!
//! ```text
//! Authorization: Basic base64(urlencode(client_id) + ":" + urlencode(client_secret))
//! ```
//!
//! - `confidential` client：**必须**带 `Basic` 头；
//! - `public` client：只带请求体里的 `client_id`，**不带** secret；
//!   它用 `client_secret_basic` 认证（secret 为空串）→ **一律 401**
//!   （D244 的步骤 5.5：这是「public 无权调内省」与「压根没带认证」的唯一区分形态）。
//!
//! # ⚠️ 认证失败**不区分**「client 不存在」与「secret 错」（D207）
//!
//! 两种情况都返回 **401 + `{"error":"invalid_client"}`** ——
//! 否则就成了 client 枚举接口。

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::crypto::digest_base64url;
use crate::protocol::ProtocolError;

/// 已解析的 client 凭据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCredentials {
    /// client ID。
    pub client_id: String,
    /// client secret；`None` = 请求里**没有** secret（public client 的形态）。
    pub client_secret: Option<String>,
}

/// 从请求里解析 client 凭据。
///
/// 优先级：`Authorization: Basic` **优先**；没有该头时回落到请求体的 `client_id`
/// （public client 的形态）。
///
/// # 失败语义
///
/// 解析不出任何凭据 → `ProtocolError::InvalidClient`（**401**）。
pub fn parse(
    authorization: Option<&str>,
    body_client_id: Option<&str>,
) -> Result<ClientCredentials, ProtocolError> {
    if let Some(header) = authorization.map(str::trim).filter(|v| !v.is_empty()) {
        let (client_id, secret) = parse_basic(header).ok_or(ProtocolError::InvalidClient)?;
        return Ok(ClientCredentials {
            client_id,
            client_secret: Some(secret),
        });
    }
    let client_id = body_client_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ProtocolError::InvalidClient)?;
    Ok(ClientCredentials {
        client_id: client_id.to_string(),
        client_secret: None,
    })
}

/// 解析 `Authorization: Basic <base64>`。
///
/// 格式（RFC 6749 §2.3.1）：base64 解开后是
/// `urlencode(client_id) + ":" + urlencode(client_secret)`。
///
/// 返回 `None` = 头部缺失 / 格式不对 / 解不开 base64。
pub fn parse_basic(header: &str) -> Option<(String, String)> {
    let raw = header.trim();
    let encoded = raw
        .strip_prefix("Basic ")
        .or_else(|| raw.strip_prefix("basic "))?
        .trim();
    if encoded.is_empty() {
        return None;
    }
    let decoded = BASE64.decode(encoded).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (client_id, secret) = decoded.split_once(':')?;
    let client_id = percent_decode(client_id);
    let secret = percent_decode(secret);
    if client_id.trim().is_empty() {
        return None;
    }
    Some((client_id, secret))
}

/// 校验 client secret。
///
/// - 库里**没有**摘要（public client）→ **永远失败**（public client 不得用 secret 认证）；
/// - 摘要对不上 → 失败。
///
/// ⚠️ **不区分**失败原因（D207）：调用方一律回 `invalid_client`。
pub fn verify_client_secret(stored_hash: Option<&str>, secret: &str) -> bool {
    let Some(stored) = stored_hash.map(str::trim).filter(|v| !v.is_empty()) else {
        return false;
    };
    let candidate = digest_base64url(secret.as_bytes());
    constant_time_eq(stored.as_bytes(), candidate.as_bytes())
}

/// `application/x-www-form-urlencoded` 的解码（RFC 6749 §2.3.1 附录 B）。
///
/// - `+` → 空格；
/// - `%XX` → 字节；
/// - 其余原样。
///
/// ⚠️ **不是** URL path 的解码（那里 `+` 是字面加号）—— 这里按表单语义。
pub fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = hex_value(bytes[index + 1]);
                let low = hex_value(bytes[index + 2]);
                match (high, low) {
                    (Some(high), Some(low)) => {
                        out.push(high * 16 + low);
                        index += 3;
                    }
                    _ => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 十六进制字符的值。
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// 定长比较（**不**因为第一个不同字节就提前返回）。
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic(client_id: &str, secret: &str) -> String {
        format!(
            "Basic {}",
            BASE64.encode(format!("{client_id}:{secret}").as_bytes())
        )
    }

    #[test]
    fn a_basic_header_is_parsed() {
        let header = basic("client-a", "s3cret");
        let (client_id, secret) = parse_basic(&header).expect("应当解析成功");
        assert_eq!(client_id, "client-a");
        assert_eq!(secret, "s3cret");
    }

    /// RFC 6749 §2.3.1：client_id / secret 在 base64 前要**先**做表单编码。
    #[test]
    fn the_credentials_are_form_decoded() {
        let header = format!("Basic {}", BASE64.encode("client%3Aid:sec+ret".as_bytes()));
        let (client_id, secret) = parse_basic(&header).expect("应当解析成功");
        assert_eq!(client_id, "client:id");
        assert_eq!(secret, "sec ret");
    }

    /// 头部不是 `Basic` / 解不开 / 没有冒号 → `None`。
    #[test]
    fn a_malformed_header_is_rejected() {
        assert!(parse_basic("Bearer abc").is_none());
        assert!(parse_basic("Basic !!!!").is_none());
        assert!(parse_basic("Basic ").is_none());
        // 没有冒号。
        assert!(parse_basic(&format!("Basic {}", BASE64.encode("nocolon"))).is_none());
    }

    /// `Basic` 头优先于请求体的 `client_id`。
    #[test]
    fn the_basic_header_wins_over_the_body() {
        let credentials =
            parse(Some(&basic("from-header", "s")), Some("from-body")).expect("应当解析成功");
        assert_eq!(credentials.client_id, "from-header");
        assert_eq!(credentials.client_secret.as_deref(), Some("s"));
    }

    /// 没有 `Basic` 头 → 回落到请求体的 `client_id`（public client 的形态）。
    #[test]
    fn the_body_client_id_is_the_fallback() {
        let credentials = parse(None, Some("public-client")).expect("应当解析成功");
        assert_eq!(credentials.client_id, "public-client");
        assert_eq!(credentials.client_secret, None);
    }

    /// 什么都没有 → 认证失败（**401**）。
    #[test]
    fn no_credentials_at_all_is_a_failure() {
        assert_eq!(parse(None, None), Err(ProtocolError::InvalidClient));
        assert_eq!(parse(None, Some("  ")), Err(ProtocolError::InvalidClient));
    }

    /// secret 正确 → 通过。
    #[test]
    fn the_right_secret_passes() {
        let hash = digest_base64url(b"s3cret");
        assert!(verify_client_secret(Some(&hash), "s3cret"));
        assert!(!verify_client_secret(Some(&hash), "wrong"));
    }

    /// ⚠️ public client（**没有**摘要）→ secret 认证**永远失败**。
    #[test]
    fn a_public_client_cannot_authenticate_with_a_secret() {
        assert!(!verify_client_secret(None, ""));
        assert!(!verify_client_secret(Some(""), ""));
        assert!(!verify_client_secret(None, "anything"));
    }

    /// 定长比较：长度不同直接失败，但**不**逐字节提前返回。
    #[test]
    fn the_comparison_rejects_different_lengths() {
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
    }

    /// 表单解码的边界。
    #[test]
    fn the_form_decoder_handles_the_edges() {
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("a%2Fb"), "a/b");
        // 非法转义 → 原样保留。
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("a%2"), "a%2");
    }
}
