//! TOTP（HMAC-SHA1 + base32）与一次性恢复码。
//!
//! # 实现要点（design-summary §4）
//!
//! - **secret 用 base32**，存 DB 时**静态加密**（密钥只从环境注入）；
//! - 时间窗口容差 **±1 个 30 秒步**；
//! - **恢复码**：绑定时生成 10 个一次性恢复码，**只存 hash**；
//! - **防重放**：记录最近一次成功的时间步，**同一时间步的验证码不能复用**。
//!
//! # WebAuthn
//!
//! 本期**只留接口不实现**（见 [`MfaMethod`]）：`WebAuthn` 是合法的 MFA 方式，
//! 但调用它会返回 `NOT_IMPLEMENTED` —— 接口先立住，依赖重的实现留给后续需求。

use totp_rs::{Algorithm, Secret, TOTP};

use crate::config::TotpSettings;
use crate::error::IamError;

/// TOTP secret 的字节长度（160 位 —— RFC 4226 推荐）。
pub const SECRET_BYTES: usize = 20;

/// 恢复码的字符数（去掉分隔符后）。
const RECOVERY_CODE_CHARS: usize = 16;

/// 恢复码字符集（去掉 `0`/`1`/`I`/`O`/`U` 等易混字符）。
const RECOVERY_ALPHABET: &[u8; 31] = b"ABCDEFGHJKLMNPQRSTVWXYZ23456789";

/// 恢复码的分组长度（展示用）。
const RECOVERY_GROUP: usize = 4;

/// MFA 方式。
///
/// `WebAuthn` **只留接口不实现**（design-summary §4 / §9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MfaMethod {
    /// TOTP（本期实现）。
    Totp,
    /// WebAuthn / FIDO2（**本期只留接口**）。
    WebAuthn,
}

impl MfaMethod {
    /// 方式名（稳定的机器可读标识）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Totp => "totp",
            Self::WebAuthn => "webauthn",
        }
    }

    /// 本期是否已实现。
    pub fn is_implemented(self) -> bool {
        match self {
            Self::Totp => true,
            Self::WebAuthn => false,
        }
    }

    /// 调用一个未实现的方式 → `NOT_IMPLEMENTED`。
    pub fn ensure_implemented(self) -> Result<(), IamError> {
        if self.is_implemented() {
            Ok(())
        } else {
            Err(IamError::NotImplemented {
                feature: "webauthn",
            })
        }
    }
}

/// 绑定 TOTP 时返回给用户的信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotpEnrollment {
    /// base32 编码的 secret（用户可手输）。
    pub secret_base32: String,
    /// otpauth URI（扫码用）。
    pub otpauth_uri: String,
}

/// 生成一个新的 TOTP secret。
///
/// 返回 `(原始字节, base32 串)`。
pub fn generate_secret() -> (Vec<u8>, String) {
    let raw = crate::crypto::random_bytes(SECRET_BYTES);
    let encoded = match Secret::Raw(raw.clone()).to_encoded() {
        Secret::Encoded(text) => text,
        Secret::Raw(_) => unreachable!("to_encoded() 一定返回 Encoded"),
    };
    (raw, encoded)
}

/// 用 base32 secret 还原原始字节。
pub fn decode_secret(base32: &str) -> Result<Vec<u8>, IamError> {
    Secret::Encoded(base32.to_string())
        .to_bytes()
        .map_err(|_| IamError::Internal {
            reason: "the totp secret is not valid base32".to_string(),
        })
}

/// 构造 otpauth URI。
pub fn otpauth_uri(
    issuer: &str,
    account: &str,
    secret_base32: &str,
    settings: &TotpSettings,
) -> Result<String, IamError> {
    let totp = build_totp(
        &decode_secret(secret_base32)?,
        settings,
        Some(issuer.to_string()),
        account.to_string(),
    )?;
    Ok(totp.get_url())
}

/// 构造 TOTP 实例（**skew = 0**：容差由本模块自己逐时间步检查，
/// 这样才能知道**命中的是哪一步** —— 防重放需要它）。
fn build_totp(
    secret: &[u8],
    settings: &TotpSettings,
    issuer: Option<String>,
    account: String,
) -> Result<TOTP, IamError> {
    TOTP::new(
        Algorithm::SHA1,
        settings.digits as usize,
        0,
        settings.interval,
        secret.to_vec(),
        issuer,
        account,
    )
    .map_err(|err| IamError::Internal {
        reason: format!("the totp instance could not be created: {err}"),
    })
}

/// 校验验证码。
///
/// 返回**命中的时间步**（调用方要把它记下来，用于防重放）。
///
/// 判定规则：
///
/// 1. 验证码必须在容差窗口内（默认 ±1 步）；
/// 2. 命中的时间步必须 **大于** `last_used_step` —— 否则是**重放**，拒绝。
pub fn verify_code(
    secret: &[u8],
    code: &str,
    last_used_step: Option<i64>,
    now_unix: i64,
    settings: &TotpSettings,
) -> Result<i64, IamError> {
    let normalized = code.trim();
    if normalized.is_empty() {
        return Err(IamError::MfaInvalid);
    }

    let totp = build_totp(secret, settings, None, String::new())?;
    let current_step = (now_unix.max(0) as u64) / settings.interval;

    let mut matched: Option<i64> = None;
    // 从当前步往两侧扩：先试当前步，再试相邻步。
    for offset in 0..=settings.tolerance_steps {
        for step in [
            current_step.saturating_sub(offset),
            current_step.saturating_add(offset),
        ] {
            if matched.is_some() {
                break;
            }
            if totp.check(normalized, step * settings.interval) {
                matched = Some(step as i64);
            }
        }
    }

    let Some(step) = matched else {
        return Err(IamError::MfaInvalid);
    };

    // 防重放：同一时间步的验证码**不能**用第二次。
    if let Some(last) = last_used_step
        && step <= last
    {
        return Err(IamError::MfaInvalid);
    }
    Ok(step)
}

/// 生成一次性恢复码。
///
/// 16 个字符（80 位熵），4-4-4-4 分组展示。
pub fn generate_recovery_codes(count: usize) -> Vec<String> {
    (0..count)
        .map(|_| {
            let raw = crate::crypto::random_bytes(RECOVERY_CODE_CHARS);
            let mut chars = String::with_capacity(RECOVERY_CODE_CHARS + 3);
            for (index, byte) in raw.iter().enumerate() {
                if index > 0 && index % RECOVERY_GROUP == 0 {
                    chars.push('-');
                }
                chars.push(RECOVERY_ALPHABET[(*byte as usize) % RECOVERY_ALPHABET.len()] as char);
            }
            chars
        })
        .collect()
}

/// 归一化恢复码（去分隔符 + 转大写）—— 比较与哈希都用它。
pub fn normalize_recovery_code(code: &str) -> String {
    code.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的 TOTP 参数。
    fn settings() -> TotpSettings {
        TotpSettings::default()
    }

    #[test]
    fn a_generated_secret_is_base32_and_long_enough() {
        let (raw, encoded) = generate_secret();
        assert_eq!(raw.len(), SECRET_BYTES);
        assert!(!encoded.is_empty());
        assert!(
            encoded
                .chars()
                .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c))
        );
        assert_eq!(decode_secret(&encoded).expect("解码应当成功"), raw);
    }

    #[test]
    fn a_valid_code_is_accepted_and_returns_the_step() {
        let (raw, _) = generate_secret();
        let settings = settings();
        let now = 1_700_000_000i64;
        let totp = build_totp(&raw, &settings, None, String::new()).expect("构造应当成功");
        let code = totp.generate(now as u64);

        let step = verify_code(&raw, &code, None, now, &settings).expect("正确验证码应当通过");
        assert_eq!(step, now / settings.interval as i64);
    }

    /// AC-S-015：同一时间步的验证码**不能**用第二次。
    #[test]
    fn the_same_code_cannot_be_used_twice() {
        let (raw, _) = generate_secret();
        let settings = settings();
        let now = 1_700_000_000i64;
        let totp = build_totp(&raw, &settings, None, String::new()).expect("构造应当成功");
        let code = totp.generate(now as u64);

        let first = verify_code(&raw, &code, None, now, &settings).expect("第一次应当通过");
        let error = verify_code(&raw, &code, Some(first), now, &settings)
            .expect_err("同一时间步的验证码必须被拒");
        assert_eq!(error.code(), "MFA_INVALID");
    }

    #[test]
    fn a_code_outside_the_tolerance_window_is_rejected() {
        let (raw, _) = generate_secret();
        let settings = settings();
        let now = 1_700_000_000i64;
        let totp = build_totp(&raw, &settings, None, String::new()).expect("构造应当成功");
        // 10 分钟前 —— 远超 ±1 步的容差。
        let old_code = totp.generate((now - 600) as u64);
        let error =
            verify_code(&raw, &old_code, None, now, &settings).expect_err("过期验证码必须被拒");
        assert_eq!(error.code(), "MFA_INVALID");
    }

    #[test]
    fn a_code_within_the_tolerance_window_is_accepted() {
        let (raw, _) = generate_secret();
        let settings = settings();
        let now = 1_700_000_000i64;
        let totp = build_totp(&raw, &settings, None, String::new()).expect("构造应当成功");
        // 上一时间步的验证码在容差窗口内。
        let previous = totp.generate((now - settings.interval as i64) as u64);
        assert!(verify_code(&raw, &previous, None, now, &settings).is_ok());
    }

    #[test]
    fn a_wrong_code_is_rejected() {
        let (raw, _) = generate_secret();
        let settings = settings();
        let error = verify_code(&raw, "000000", None, 1_700_000_000, &settings)
            .expect_err("错误验证码必须被拒");
        assert_eq!(error.code(), "MFA_INVALID");
    }

    #[test]
    fn recovery_codes_are_unique_and_normalizable() {
        let codes = generate_recovery_codes(10);
        assert_eq!(codes.len(), 10);
        let mut sorted = codes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10, "恢复码不得重复");
        for code in &codes {
            assert_eq!(code.len(), RECOVERY_CODE_CHARS + 3, "code = {code}");
            assert_eq!(normalize_recovery_code(code).len(), RECOVERY_CODE_CHARS);
        }
        assert_eq!(
            normalize_recovery_code("abcd-efgh-jkmn-pqrs"),
            "ABCDEFGHJK MNPQRS".replace(' ', "")
        );
    }

    #[test]
    fn the_otpauth_uri_contains_the_secret() {
        let (_, encoded) = generate_secret();
        let uri = otpauth_uri("base", "alice@example.com", &encoded, &settings())
            .expect("URI 应当能构造");
        assert!(uri.starts_with("otpauth://totp/"), "uri = {uri}");
        assert!(uri.contains(&encoded), "uri 里必须带 secret");
        assert!(uri.contains("issuer=base"), "uri = {uri}");
    }

    /// WebAuthn 只留接口不实现。
    #[test]
    fn webauthn_is_declared_but_not_implemented() {
        assert_eq!(MfaMethod::WebAuthn.as_str(), "webauthn");
        assert!(!MfaMethod::WebAuthn.is_implemented());
        let error = MfaMethod::WebAuthn
            .ensure_implemented()
            .expect_err("WebAuthn 本期不实现");
        assert_eq!(error.code(), "NOT_IMPLEMENTED");
        assert!(MfaMethod::Totp.ensure_implemented().is_ok());
    }
}
