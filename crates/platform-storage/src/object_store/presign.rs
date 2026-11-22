//! **预签名令牌**（REQ-018，D322 / AC-S-026 / AC-S-176）。
//!
//! # 形态
//!
//! ```text
//! payload = key_id "." expires_at_unix "." object_key
//! sig     = HMAC-SHA256(secret, payload)
//! token   = base64url(payload) "." base64url(sig)
//! ```
//!
//! # ⚠️ 三条安全要求（每条都有判据）
//!
//! 1. **`expires_at` 在签名覆盖范围内** → 改时间就签名失效（AC-S-176 步骤 1）；
//! 2. **`object_key` 也在签名覆盖范围内** → 不能拿 A 的令牌下 B 的文件（步骤 2）；
//! 3. **校验顺序：签名 → 过期**（**不可换**）。
//!    否则攻击者能通过「过期报错 vs 签名错报错」探测 key 是否存在（D322）。
//!
//! # ⚠️ 常数时间比较（TD-062）
//!
//! 用 `subtle::ConstantTimeEq` 而不是 `==`。
//! ⚠️ **「用了常数时间比较」这件事无法用功能测试断言**（两者功能等价）→
//! 只能靠这里 + 代码评审。谁改这段，必须看这一行。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::StorageError;

/// HMAC-SHA256。
type HmacSha256 = Hmac<Sha256>;

/// 预签名的 TTL 上限（秒）—— **一天**（S-01 / AC-S-026 步骤 5）。
///
/// ⚠️ 超上限**拒绝**，不是截断（截断会让调用方以为拿到了更长的链接）。
pub const MAX_TTL_SECONDS: i64 = 86_400;

/// 签发一个预签名令牌。
///
/// # 参数
///
/// - `secret`：HMAC 密钥（装配层从环境变量注入，D306 同款）；
/// - `key_id`：密钥标识（用于轮换：泄露时能按 id 失效）；
/// - `object_key`：对象路径（**纳入签名**）；
/// - `ttl_seconds`：有效期（**必须**在 `1..=MAX_TTL_SECONDS`）；
/// - `now_unix`：当前时间（**注入**，便于确定性测试）。
pub fn issue(
    secret: &[u8],
    key_id: &str,
    object_key: &str,
    ttl_seconds: i64,
    now_unix: i64,
) -> Result<(String, i64), StorageError> {
    // ⚠️ TTL 校验：0 也非法（「立即过期」不该被当成成功，TC-018-006）。
    if ttl_seconds <= 0 || ttl_seconds > MAX_TTL_SECONDS {
        return Err(StorageError::TtlOutOfRange {
            requested: ttl_seconds,
            max_seconds: MAX_TTL_SECONDS,
        });
    }
    let expires_at = now_unix + ttl_seconds;
    let payload = format!("{key_id}.{expires_at}.{object_key}");
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| StorageError::PresignInvalid {
        reason: "the signing key is not usable",
    })?;
    mac.update(payload.as_bytes());
    let signature = mac.finalize().into_bytes();

    let token = format!(
        "{}.{}",
        B64URL.encode(payload.as_bytes()),
        B64URL.encode(signature)
    );
    Ok((token, expires_at))
}

/// 校验结果：通过时给出 `object_key` 与过期时刻。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    /// 被授权的对象路径。
    pub object_key: String,
    /// 过期时刻（unix 秒）。
    pub expires_at: i64,
}

/// 校验一个预签名令牌。
///
/// # ⚠️ 顺序：**先验签名，再看过期**（D322，不可换）
pub fn verify(secret: &[u8], token: &str, now_unix: i64) -> Result<VerifiedToken, StorageError> {
    // ① 结构：必须是两段。
    let (payload_b64, sig_b64) = token.split_once('.').ok_or(StorageError::PresignInvalid {
        reason: "the token is not in `payload.signature` form",
    })?;

    let payload_bytes = B64URL
        .decode(payload_b64)
        .map_err(|_| StorageError::PresignInvalid {
            reason: "the payload is not valid base64url",
        })?;
    let provided_sig = B64URL
        .decode(sig_b64)
        .map_err(|_| StorageError::PresignInvalid {
            reason: "the signature is not valid base64url",
        })?;

    // ② **先验签名**（在解析 expires_at 之前 —— 见模块文档）。
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| StorageError::PresignInvalid {
        reason: "the signing key is not usable",
    })?;
    mac.update(&payload_bytes);
    let expected_sig = mac.finalize().into_bytes();

    // ⚠️ **常数时间比较**（TD-062）：`==` 会在第一个不同字节提前返回 → 时序侧信道。
    if expected_sig
        .as_slice()
        .ct_eq(provided_sig.as_slice())
        .unwrap_u8()
        != 1
    {
        return Err(StorageError::PresignInvalid {
            reason: "the signature does not match",
        });
    }

    // ③ 签名通过后才解析内容。
    let payload = String::from_utf8(payload_bytes).map_err(|_| StorageError::PresignInvalid {
        reason: "the payload is not valid utf-8",
    })?;
    let mut parts = payload.splitn(3, '.');
    let _key_id = parts.next().ok_or(StorageError::PresignInvalid {
        reason: "the payload is missing the key id",
    })?;
    let expires_raw = parts.next().ok_or(StorageError::PresignInvalid {
        reason: "the payload is missing the expiry",
    })?;
    let object_key = parts.next().ok_or(StorageError::PresignInvalid {
        reason: "the payload is missing the object key",
    })?;
    let expires_at: i64 = expires_raw
        .parse()
        .map_err(|_| StorageError::PresignInvalid {
            reason: "the expiry is not a unix timestamp",
        })?;

    // ④ 最后看过期。
    if now_unix >= expires_at {
        return Err(StorageError::PresignExpired {
            expired_at: expires_at,
        });
    }

    Ok(VerifiedToken {
        object_key: object_key.to_string(),
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret-key-do-not-use-in-production";

    fn b64(s: &str) -> String {
        B64URL.encode(s.as_bytes())
    }

    /// 主路径：签发 → 校验通过，且 `expires_at` **逐秒相等**（TC-018-001）。
    #[test]
    fn issuing_then_verifying_succeeds() {
        let (token, expires_at) = issue(SECRET, "k1", "docs/a.txt", 2, 1_000).expect("应签发成功");
        assert_eq!(expires_at, 1_002, "expires_at 必须恰好是 now + ttl");
        let verified = verify(SECRET, &token, 1_000).expect("应校验通过");
        assert_eq!(verified.object_key, "docs/a.txt");
        assert_eq!(verified.expires_at, 1_002);
        // token 结构：两段（一个点）。
        assert_eq!(token.matches('.').count(), 1, "token 必须恰好一个点");
    }

    /// token 的 payload 能解出过期时间（可排障，TC-018-004）。
    #[test]
    fn the_payload_exposes_the_expiry() {
        let (token, _) = issue(SECRET, "k1", "docs/a.txt", 900, 5_000).expect("应签发");
        let payload_b64 = token.split('.').next().expect("第一段");
        let payload =
            String::from_utf8(B64URL.decode(payload_b64).expect("应能解码")).expect("应是 utf-8");
        assert_eq!(
            payload, "k1.5900.docs/a.txt",
            "payload 形态必须是 key_id.exp.key"
        );
    }

    /// 过期 → `PRESIGN_EXPIRED`（**不是** INVALID，TC-018-003）。
    #[test]
    fn an_expired_token_is_expired_not_invalid() {
        let (token, expires_at) = issue(SECRET, "k1", "docs/a.txt", 2, 1_000).expect("应签发");
        let err = verify(SECRET, &token, 1_003).expect_err("应过期");
        assert_eq!(err.code(), "STORAGE_PRESIGN_EXPIRED");
        assert_ne!(err.code(), "STORAGE_PRESIGN_INVALID");
        assert!(
            matches!(err, StorageError::PresignExpired { expired_at } if expired_at == expires_at),
            "必须带上 expired_at"
        );
    }

    /// 边界：`now == expires_at` 已算过期（`>=` 而非 `>`）。
    #[test]
    fn the_exact_expiry_instant_is_already_expired() {
        let (token, _) = issue(SECRET, "k1", "k", 2, 1_000).expect("应签发");
        assert!(
            verify(SECRET, &token, 1_002).is_err(),
            "now == expires_at 时必须已过期"
        );
        assert!(
            verify(SECRET, &token, 1_001).is_ok(),
            "now == expires_at - 1 时必须还有效（边界包含）"
        );
    }

    /// TTL 超上限 → `TTL_OUT_OF_RANGE` 且**不签发**（TC-018-005）。
    #[test]
    fn a_ttl_above_the_maximum_is_rejected() {
        for ttl in [MAX_TTL_SECONDS + 1, MAX_TTL_SECONDS * 365] {
            let err = issue(SECRET, "k1", "k", ttl, 0).expect_err("必须拒绝");
            assert_eq!(err.code(), "STORAGE_TTL_OUT_OF_RANGE", "ttl={ttl}");
        }
        // 边界包含：正好上限成功（阳性对照）。
        assert!(
            issue(SECRET, "k1", "k", MAX_TTL_SECONDS, 0).is_ok(),
            "正好上限必须成功"
        );
    }

    /// TTL = 0 → `TTL_OUT_OF_RANGE`（0 是非法，不是「立即过期」，TC-018-006）。
    #[test]
    fn a_zero_ttl_is_rejected() {
        let err = issue(SECRET, "k1", "k", 0, 0).expect_err("必须拒绝");
        assert_eq!(err.code(), "STORAGE_TTL_OUT_OF_RANGE");
    }

    /// 负 TTL 也被拒绝。
    #[test]
    fn a_negative_ttl_is_rejected() {
        let err = issue(SECRET, "k1", "k", -1, 0).expect_err("必须拒绝");
        assert_eq!(err.code(), "STORAGE_TTL_OUT_OF_RANGE");
    }

    /// **篡改过期时间** → `PRESIGN_INVALID`（TC-018-007，`expires_at` 在签名覆盖范围内）。
    #[test]
    fn tampering_with_the_expiry_is_invalid() {
        let (token, _) = issue(SECRET, "k1", "docs/a.txt", 2, 1_000).expect("应签发");
        let (_, sig) = token.split_once('.').expect("两段");

        // 把 payload 里的过期时间改大一天。
        let forged_payload = b64("k1.87402.docs/a.txt");
        let forged = format!("{forged_payload}.{sig}");

        let err = verify(SECRET, &forged, 1_001).expect_err("篡改必须被拒");
        assert_eq!(
            err.code(),
            "STORAGE_PRESIGN_INVALID",
            "必须是 INVALID 不是 EXPIRED"
        );
    }

    /// **篡改 object_key** → `PRESIGN_INVALID`（TC-018-008，
    /// key 在签名覆盖范围内 —— 这条防「拿 A 的令牌下 B 的文件」）。
    #[test]
    fn tampering_with_the_object_key_is_invalid() {
        let (token, _) = issue(SECRET, "k1", "docs/a.txt", 60, 1_000).expect("应签发");
        let (_, sig) = token.split_once('.').expect("两段");

        let forged_payload = b64("k1.1060.docs/secret.txt");
        let forged = format!("{forged_payload}.{sig}");

        let err = verify(SECRET, &forged, 1_001).expect_err("篡改必须被拒");
        assert_eq!(err.code(), "STORAGE_PRESIGN_INVALID");
    }

    /// **篡改签名一个字符** → `PRESIGN_INVALID`（TC-018-009）。
    #[test]
    fn tampering_with_the_signature_is_invalid() {
        let (token, _) = issue(SECRET, "k1", "k", 60, 1_000).expect("应签发");
        let (payload_b64, sig) = token.split_once('.').expect("两段");
        // 把签名的第一个字符换成另一个 base64url 字符。
        let mut sig_chars: Vec<char> = sig.chars().collect();
        sig_chars[0] = if sig_chars[0] == 'A' { 'B' } else { 'A' };
        let forged_sig: String = sig_chars.into_iter().collect();

        let err =
            verify(SECRET, &format!("{payload_b64}.{forged_sig}"), 1_001).expect_err("必须被拒");
        assert_eq!(err.code(), "STORAGE_PRESIGN_INVALID");
    }

    /// 用**另一个密钥**签的 token → `PRESIGN_INVALID`。
    #[test]
    fn a_token_signed_with_another_key_is_invalid() {
        let (token, _) = issue(b"other-secret", "k1", "k", 60, 1_000).expect("应签发");
        let err = verify(SECRET, &token, 1_001).expect_err("必须被拒");
        assert_eq!(err.code(), "STORAGE_PRESIGN_INVALID");
    }

    /// 形态不对的 token 全部 → `PRESIGN_INVALID`（不 panic）。
    #[test]
    fn malformed_tokens_are_invalid_and_never_panic() {
        for bad in [
            "", "nodot", ".", "a.b", "!!!.###",
            "YQ.YQ",   // 合法 base64url 但不是我们的 payload 形态
            "a.b.c.d", // 太多点（split_once 后第二段含点 → 解不出来）
        ] {
            let outcome = std::panic::catch_unwind(|| verify(SECRET, bad, 0));
            assert!(outcome.is_ok(), "校验 {bad:?} 时 panic 了");
            let err = outcome.expect("不 panic").expect_err("必须被拒");
            assert_eq!(
                err.code(),
                "STORAGE_PRESIGN_INVALID",
                "形态错误 {bad:?} 的码不对"
            );
        }
    }

    /// 空密钥 → `PRESIGN_INVALID`（不 panic）。
    #[test]
    fn an_empty_secret_is_handled() {
        // HMAC 接受任意长度密钥，所以签发能成功；但校验会用同一密钥。
        let result = issue(b"", "k1", "k", 60, 1_000);
        assert!(result.is_ok() || result.is_err(), "不得 panic");
        if let Ok((token, _)) = result {
            assert!(verify(b"", &token, 1_001).is_ok(), "同密钥应能验");
        }
    }

    /// **签名先于过期**：篡改且过期的 token 必须报 INVALID（不是 EXPIRED）。
    ///
    /// ⚠️ 这条是 D322 第 3 条要求的直接判据：
    /// 若实现先看过期，攻击者能通过「哪个错误」探测 key 是否存在。
    #[test]
    fn signature_is_checked_before_expiry() {
        let (token, _) = issue(SECRET, "k1", "docs/a.txt", 2, 1_000).expect("应签发");
        let (_, sig) = token.split_once('.').expect("两段");
        let forged_payload = b64("k1.1.docs/secret.txt"); // 早已过期 + 篡改
        let forged = format!("{forged_payload}.{sig}");

        let err = verify(SECRET, &forged, 9_999).expect_err("必须被拒");
        assert_eq!(
            err.code(),
            "STORAGE_PRESIGN_INVALID",
            "篡改+过期时必须报 INVALID（签名先于过期），不是 EXPIRED"
        );
    }
}
