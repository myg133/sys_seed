//! 随机数、摘要与静态加密。
//!
//! # 三类「秘密」的存储口径不一样
//!
//! | 东西 | 熵 | 存储 | 原因 |
//! |------|----|------|------|
//! | **密码** | 低 | Argon2id **PHC 串** | 低熵 → 必须用慢哈希 |
//! | **refresh token / 重置令牌 / 恢复码** | 高 | **SHA-256 摘要** | 高熵 → 快哈希就够（可即时撤销） |
//! | **TOTP secret** | 高 | **ChaCha20-Poly1305 密文** | 要能**还原**才能算验证码 → 必须可逆加密 |
//!
//! 静态加密的密钥**只从环境注入**（`BASE_IAM_SECRET_KEY`），不入库不入 Git。

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, Key, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::error::IamError;

/// 静态加密用的密钥长度（ChaCha20-Poly1305 → 32 字节）。
pub const SECRET_KEY_LEN: usize = 32;

/// refresh token 的随机字节数（256 位熵）。
pub const REFRESH_TOKEN_BYTES: usize = 32;

/// 密码重置令牌的随机字节数（256 位熵）。
pub const RESET_TOKEN_BYTES: usize = 32;

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

/// 生成 refresh token（不透明随机串，**只在签发时出现一次**）。
pub fn new_refresh_token() -> String {
    random_base64url(REFRESH_TOKEN_BYTES)
}

/// 生成密码重置令牌。
pub fn new_reset_token() -> String {
    random_base64url(RESET_TOKEN_BYTES)
}

/// 摘要（base64url）。
///
/// 用于**高熵值**的秘密（refresh token / 重置令牌 / 恢复码）：存 hash，不存明文。
pub fn digest_base64url(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// 摘要（十六进制）—— 用于需要固定宽度、可读性好的场景。
pub fn digest_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 静态加密（ChaCha20-Poly1305）。
///
/// 输出格式：`base64url(nonce ‖ ciphertext‖tag)`。
pub fn encrypt(plaintext: &[u8], key: &[u8; SECRET_KEY_LEN]) -> Result<String, IamError> {
    let cipher = new_cipher(key)?;
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = nonce_from(&nonce_bytes)?;
    let ciphertext =
        cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|_| IamError::Internal {
                reason: "the secret could not be encrypted".to_string(),
            })?;
    let mut payload = nonce_bytes.to_vec();
    payload.extend_from_slice(&ciphertext);
    Ok(URL_SAFE_NO_PAD.encode(payload))
}

/// 解密（ChaCha20-Poly1305）。
pub fn decrypt(payload: &str, key: &[u8; SECRET_KEY_LEN]) -> Result<Vec<u8>, IamError> {
    let raw = URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .map_err(|_| IamError::Internal {
            reason: "the encrypted secret is not valid base64url".to_string(),
        })?;
    if raw.len() <= 12 {
        return Err(IamError::Internal {
            reason: "the encrypted secret is too short".to_string(),
        });
    }
    let (nonce_bytes, ciphertext) = raw.split_at(12);
    let nonce = nonce_from(nonce_bytes)?;
    let cipher = new_cipher(key)?;
    cipher
        .decrypt(&nonce, ciphertext.as_ref())
        .map_err(|_| IamError::Internal {
            reason: "the encrypted secret could not be decrypted".to_string(),
        })
}

/// 构造 AEAD 实例。
fn new_cipher(key: &[u8; SECRET_KEY_LEN]) -> Result<ChaCha20Poly1305, IamError> {
    let key =
        Key::<ChaCha20Poly1305>::try_from(key.as_slice()).map_err(|_| IamError::Internal {
            reason: "the secret key must be 32 bytes long".to_string(),
        })?;
    Ok(ChaCha20Poly1305::new(&key))
}

/// 构造 nonce（96 位）。
fn nonce_from(bytes: &[u8]) -> Result<Nonce, IamError> {
    Nonce::try_from(bytes).map_err(|_| IamError::Internal {
        reason: "the nonce must be 12 bytes long".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的固定密钥。
    fn test_key() -> [u8; SECRET_KEY_LEN] {
        [42u8; SECRET_KEY_LEN]
    }

    #[test]
    fn random_values_differ() {
        let a = random_base64url(32);
        let b = random_base64url(32);
        assert_ne!(a, b);
        assert!(!a.contains('='), "base64url 不该有填充：{a}");
        assert!(!a.contains('+'), "base64url 不该有 +：{a}");
        assert!(!a.contains('/'), "base64url 不该有 /：{a}");
    }

    #[test]
    fn digests_are_stable_and_different_per_input() {
        assert_eq!(digest_base64url(b"abc"), digest_base64url(b"abc"));
        assert_ne!(digest_base64url(b"abc"), digest_base64url(b"abd"));
        assert_eq!(digest_hex(b"abc").len(), 64);
    }

    #[test]
    fn encryption_round_trips() {
        let key = test_key();
        let plaintext = b"the-totp-secret-bytes";
        let payload = encrypt(plaintext, &key).expect("加密应当成功");
        assert!(!payload.contains("the-totp-secret-bytes"));
        let decrypted = decrypt(&payload, &key).expect("解密应当成功");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encryption_uses_a_fresh_nonce_every_time() {
        let key = test_key();
        let a = encrypt(b"same-plaintext", &key).expect("加密应当成功");
        let b = encrypt(b"same-plaintext", &key).expect("加密应当成功");
        assert_ne!(a, b, "同一明文两次加密必须不同（nonce 必须随机）");
    }

    #[test]
    fn a_wrong_key_cannot_decrypt() {
        let payload = encrypt(b"secret", &test_key()).expect("加密应当成功");
        let other = [7u8; SECRET_KEY_LEN];
        assert!(decrypt(&payload, &other).is_err(), "错误的密钥必须解不开");
    }

    #[test]
    fn a_tampered_payload_is_rejected() {
        let key = test_key();
        let payload = encrypt(b"secret", &key).expect("加密应当成功");
        let mut raw = URL_SAFE_NO_PAD
            .decode(payload.as_bytes())
            .expect("解码应当成功");
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        let tampered = URL_SAFE_NO_PAD.encode(&raw);
        assert!(decrypt(&tampered, &key).is_err(), "被篡改的密文必须解不开");
    }
}
