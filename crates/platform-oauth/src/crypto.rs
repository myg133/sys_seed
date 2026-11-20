//! 摘要与随机值（REQ-022，D228）。
//!
//! # 摘要口径：`digest_base64url`（**不是** hex）
//!
//! 本需求所有「只存摘要」的列 —— `client_secret_hash` / `code_hash` /
//! `device_code_hash` / `user_code_hash` —— 一律用：
//!
//! ```text
//! SHA-256 原始 32 字节 → base64url（无填充）→ 43 字符
//! ```
//!
//! ⚠️ 仓里另有 `platform_iam::crypto::digest_hex`（小写 hex）—— **本需求不用它**。
//! 本模块的 `digest_base64url` 与 `platform_iam::crypto::digest_base64url`
//! **逐字节一致**（有一致性单测钉住）→ 两边不会漂移。
//!
//! # 为什么不用 `RandomState`
//!
//! `RandomState` 的哈希种子**不确定** → 同样的输入两次运行得到不同的顺序 →
//! 断言不可复现。凡是需要稳定顺序的地方都用 `BTreeMap` / `BTreeSet`；
//! 需要稳定摘要的地方一律用 `sha2`（确定性）。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// 高熵秘密的随机字节数（256 位熵）。
///
/// 与 `platform_iam::crypto::REFRESH_TOKEN_BYTES` 同档 ——
/// `client_secret` / 授权码 / 设备码都靠**高熵 + 快哈希**（D200）。
pub const SECRET_BYTES: usize = 32;

/// 摘要（**base64url 无填充**，43 字符）。
pub fn digest_base64url(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// 生成随机字节。
pub fn random_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes
}

/// 生成随机字节的 base64url 串（**无填充**）。
pub fn random_base64url(len: usize) -> String {
    URL_SAFE_NO_PAD.encode(random_bytes(len))
}

/// 生成一个高熵秘密（`client_secret` / 授权码 / 设备码都用它）。
pub fn new_secret() -> String {
    random_base64url(SECRET_BYTES)
}

/// 设备码的 `user_code` 字符集（**base20**，RFC 8628 §6.1）。
///
/// ⚠️ 去掉了易混字符（`0/O`、`1/I/L`…）→ 用户在设备上**手输**不会打错。
pub const USER_CODE_ALPHABET: &[u8] = b"BCDFGHJKLMNPQRSTVWXZ";

/// `user_code` 的长度（RFC 8628 §6.1）。
pub const USER_CODE_LENGTH: usize = 8;

/// 生成 `user_code`（8 个 base20 字符）。
///
/// ⚠️ **不均匀分布的隐患**：本实现用**拒绝采样**（把字节映射到
/// `0..=255` 的 20 个等分区间之外的重抽）—— 直接取模会让前几个字母
/// 出现得更频繁（256 不是 20 的倍数）→ 熵被悄悄削掉。
pub fn new_user_code() -> String {
    let alphabet = USER_CODE_ALPHABET;
    let len = alphabet.len();
    // 20 的最大公约数是 4 → 256 / 20 = 12.8 → 取 12 * 20 = 240，
    // 字节值 >= 240 的重抽（拒绝采样）→ 每个字符等概率 1/20。
    let ceiling = (256 / len) * len;
    let mut code = String::with_capacity(USER_CODE_LENGTH);
    while code.len() < USER_CODE_LENGTH {
        for byte in random_bytes(USER_CODE_LENGTH * 2) {
            if code.len() == USER_CODE_LENGTH {
                break;
            }
            let index = usize::from(byte);
            if index >= ceiling {
                continue;
            }
            code.push(alphabet[index % len] as char);
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D228：摘要必须是 base64url 无填充的 **43 字符**。
    #[test]
    fn the_digest_is_base64url_without_padding() {
        let digest = digest_base64url(b"hello");
        assert_eq!(digest.len(), 43, "SHA-256 → base64url 无填充 = 43 字符");
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "摘要的字符集必须是 base64url：{digest}"
        );
        assert!(!digest.contains('='), "摘要不得有填充：{digest}");
    }

    /// 与仓里既有的 `digest_base64url` **逐字节一致**（防止两边漂移）。
    #[test]
    fn the_digest_matches_the_platform_iam_one() {
        for input in ["", "a", "the client secret", "code-value-1234"] {
            assert_eq!(
                digest_base64url(input.as_bytes()),
                platform_iam::crypto::digest_base64url(input.as_bytes()),
                "两个 `digest_base64url` 的口径必须一致（D228）"
            );
        }
    }

    /// ⚠️ 摘要**不是** hex（仓里另有一个 `digest_hex`）。
    #[test]
    fn the_digest_is_not_hex() {
        let digest = digest_base64url(b"hello");
        assert!(
            !digest.chars().all(|c| c.is_ascii_hexdigit()),
            "摘要不得是 hex：{digest}"
        );
    }

    /// 摘要对同样的输入**永远**相同（确定性 → 断言可复现）。
    #[test]
    fn the_digest_is_deterministic() {
        assert_eq!(digest_base64url(b"same"), digest_base64url(b"same"));
        assert_ne!(digest_base64url(b"a"), digest_base64url(b"b"));
    }

    /// 随机秘密每次都不同。
    #[test]
    fn secrets_are_unique() {
        let a = new_secret();
        let b = new_secret();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
    }

    /// `user_code`：8 个字符，且逐字符 ∈ base20 字符集。
    #[test]
    fn the_user_code_has_the_rfc_shape() {
        for _ in 0..64 {
            let code = new_user_code();
            assert_eq!(code.len(), USER_CODE_LENGTH);
            for ch in code.chars() {
                assert!(
                    USER_CODE_ALPHABET.contains(&(ch as u8)),
                    "user_code 的字符必须 ∈ base20 字符集：{code}"
                );
            }
        }
    }

    /// `user_code` 里**不得**出现易混字符（`0` / `1` / `O` / `I`）。
    ///
    /// ⚠️ 字符集是 RFC 8628 §6.1 **逐字**定的 `BCDFGHJKLMNPQRSTVWXZ`
    /// —— 它**含** `L` / `M` / `N`，本需求**不得**自行删减（D246）。
    #[test]
    fn the_user_code_avoids_confusable_characters() {
        for _ in 0..64 {
            let code = new_user_code();
            for forbidden in ['0', '1', 'O', 'I'] {
                assert!(
                    !code.contains(forbidden),
                    "user_code 不得含易混字符 `{forbidden}`：{code}"
                );
            }
        }
    }
}
