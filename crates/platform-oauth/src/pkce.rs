//! PKCE：**强制 S256**（REQ-022，D206）。
//!
//! # 为什么强制
//!
//! 底座的 client 可能是 **public client**（无 secret）→ PKCE 是唯一的绑定手段；
//! 不强制 = 授权码被截获后**任何人都能换到令牌**。
//!
//! `plain` **不支持**（RFC 7636 §4.2 允许，但 `plain` 与不校验等价 → 降级保护）。
//!
//! # 校验口径
//!
//! ```text
//! base64url(SHA256(code_verifier)) == code_challenge
//! ```
//!
//! ⚠️ 摘要口径是 **base64url 无填充**（D228），**不是** hex。

use crate::crypto::digest_base64url;

/// 唯一支持的挑战方法（**元数据里也只声明它**）。
pub const CODE_CHALLENGE_METHOD_S256: &str = "S256";

/// 计算 S256 挑战：`base64url(SHA256(verifier))`。
pub fn code_challenge(verifier: &str) -> String {
    digest_base64url(verifier.as_bytes())
}

/// 校验 PKCE。
///
/// # 判定顺序（**顺序错了就是漏洞**）
///
/// 1. 挑战**缺失** → `Err(MissingChallenge)`（调用方报
///    `OAUTH_INVALID_REQUEST`）；
/// 2. 方法**不是** `S256`（含 `plain` / 缺失）→ `Err(UnsupportedMethod)`
///    （调用方报 `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD`）；
/// 3. 重算的挑战与登记的**逐字不同** → `Err(Mismatch)`
///    （调用方报协议错误 `invalid_grant` —— D206：**不是** `invalid_request`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkceError {
    /// 缺 `code_challenge`。
    MissingChallenge,
    /// `code_challenge_method` 不是 `S256`（含缺失 / `plain`）。
    UnsupportedMethod,
    /// verifier 与挑战不匹配。
    Mismatch,
}

/// 校验 PKCE（挑战缺失 / 方法不支持 / 不匹配）。
pub fn verify(
    challenge: Option<&str>,
    method: Option<&str>,
    verifier: &str,
) -> Result<(), PkceError> {
    verify_challenge(challenge, method)?;
    if verifier.trim().is_empty() {
        return Err(PkceError::Mismatch);
    }
    if code_challenge(verifier) != challenge.unwrap_or_default() {
        return Err(PkceError::Mismatch);
    }
    Ok(())
}

/// 只校验**挑战本身**（**不含** verifier）。
///
/// # 为什么单独一个入口
///
/// 参数校验发生在 **preview**（D239）：那时**还没有** verifier ——
/// verifier 要等到**交换令牌**时才有。
/// 所以 preview 判「挑战缺失 / 方法不支持」，交换时才判「verifier 是否匹配」。
pub fn verify_challenge(challenge: Option<&str>, method: Option<&str>) -> Result<(), PkceError> {
    let challenge = challenge.map(str::trim).filter(|value| !value.is_empty());
    if challenge.is_none() {
        return Err(PkceError::MissingChallenge);
    }
    // 方法**必填**：缺失等价于 `plain`（RFC 7636 的缺省是 `plain`）→ 一律拒。
    let method = method.map(str::trim).unwrap_or("");
    if method != CODE_CHALLENGE_METHOD_S256 {
        return Err(PkceError::UnsupportedMethod);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636 §4.2 的示例向量（verifier → challenge）。
    #[test]
    fn the_rfc_example_vector_matches() {
        // RFC 7636 附录 B：verifier "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        // 的 S256 challenge 是 "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"。
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            code_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    /// 挑战是 base64url 无填充的 **43 字符**（D228）。
    #[test]
    fn the_challenge_is_base64url_without_padding() {
        let challenge = code_challenge("some-verifier");
        assert_eq!(challenge.len(), 43);
        assert!(!challenge.contains('='));
    }

    /// 正确的 verifier → 通过。
    #[test]
    fn a_matching_verifier_passes() {
        let verifier = "the-verifier-value";
        let challenge = code_challenge(verifier);
        assert_eq!(verify(Some(&challenge), Some("S256"), verifier), Ok(()));
    }

    /// 缺挑战 → `MissingChallenge`（**不是** `UnsupportedMethod`）。
    #[test]
    fn a_missing_challenge_is_reported_first() {
        assert_eq!(
            verify(None, Some("S256"), "v"),
            Err(PkceError::MissingChallenge)
        );
        assert_eq!(
            verify(Some(""), Some("S256"), "v"),
            Err(PkceError::MissingChallenge)
        );
    }

    /// `plain` 与缺失的方法 → `UnsupportedMethod`。
    #[test]
    fn plain_and_missing_methods_are_rejected() {
        let challenge = code_challenge("v");
        assert_eq!(
            verify(Some(&challenge), Some("plain"), "v"),
            Err(PkceError::UnsupportedMethod)
        );
        assert_eq!(
            verify(Some(&challenge), None, "v"),
            Err(PkceError::UnsupportedMethod)
        );
        assert_eq!(
            verify(Some(&challenge), Some("S512"), "v"),
            Err(PkceError::UnsupportedMethod)
        );
    }

    /// verifier 不匹配 → `Mismatch`（调用方据此报 `invalid_grant`）。
    #[test]
    fn a_wrong_verifier_is_a_mismatch() {
        let challenge = code_challenge("right-verifier");
        assert_eq!(
            verify(Some(&challenge), Some("S256"), "wrong-verifier"),
            Err(PkceError::Mismatch)
        );
    }

    /// ⚠️ 挑战**不是**原文比较 —— 必须真的算过 S256。
    ///
    /// 若实现改成「直接比较 `code_challenge` 原文」，本用例会红。
    #[test]
    fn the_challenge_is_recomputed_not_compared_literally() {
        // `code_challenge` 的值与 verifier **不同** → 只可能靠重算通过。
        let verifier = "abc";
        let challenge = code_challenge(verifier);
        assert_ne!(challenge, verifier);
        assert_eq!(verify(Some(&challenge), Some("S256"), verifier), Ok(()));
    }
}
