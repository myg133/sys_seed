//! 平台签名的 JWT 的**验签**（REQ-022）。
//!
//! # 为什么需要自己验
//!
//! `platform_iam::jwt::JwtKeySet::verify` 被**特化**到 `AccessClaims`
//! （要求 `sub` / `sid` / `aud` = 平台 audience）→ OAuth 令牌**过不了**它：
//!
//! - OAuth 令牌的 `aud` 是 **client_id**，不是平台 audience；
//! - `client_credentials` 令牌**没有** `sub` 键；
//! - OAuth 令牌**没有** `sid`。
//!
//! 而 OAuth 令牌**必须**与登录令牌**同键同算法**（第三方用平台 JWKS 验签，
//! D198 / AC-S-145）→ 验签的**密钥**来自同一套密钥集。
//!
//! # 本模块做什么
//!
//! 从 [`JwtKeySet`] 的**公钥**（`jwks()`）重建验签密钥表，按 `kid` 选密钥，
//! 然后用 `jsonwebtoken` 验签**任意**声明。
//!
//! ⚠️ **只取公钥** —— 私钥永远不出 `platform-iam`（JWKS 只暴露公钥，D3）。

use std::collections::{BTreeMap, HashSet};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use platform_iam::jwt::JwtKeySet;
use serde::de::DeserializeOwned;

/// 验签失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    /// 令牌格式不对 / 解不开。
    Malformed,
    /// `kid` 缺失或不在密钥集里。
    UnknownKey,
    /// 签名不对 / 声明不合法 / `iss` 不符 / 已过期。
    Invalid,
}

impl VerifyError {
    /// 面向开发者的英文原因（**不含任何令牌内容**）。
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Malformed => "the token is malformed",
            Self::UnknownKey => "the token was signed with an unknown key",
            Self::Invalid => "the token signature or claims are invalid",
        }
    }
}

/// 平台密钥集的**公钥视图**（可验签）。
#[derive(Debug, Clone)]
pub struct PlatformKeys {
    /// `kid` → 验签密钥。
    decoding: BTreeMap<String, DecodingKey>,
}

impl PlatformKeys {
    /// 从密钥集构造（轮换宽限期内当前与上一把都在表里）。
    pub fn from_key_set(keys: &JwtKeySet) -> Self {
        let mut decoding = BTreeMap::new();
        for jwk in keys.jwks().keys {
            let Some(kid) = jwk.common.key_id.clone() else {
                continue;
            };
            if let Ok(key) = DecodingKey::from_jwk(&jwk) {
                decoding.insert(kid, key);
            }
        }
        Self { decoding }
    }

    /// 密钥表里的 `kid` 集合（**当前 + 上一把**）。
    pub fn kids(&self) -> Vec<&str> {
        self.decoding.keys().map(String::as_str).collect()
    }

    /// 验签并返回**任意**声明。
    ///
    /// # 参数
    ///
    /// - `issuer`：必须与令牌的 `iss` **逐字相同**（`None` = 不校验 `iss`）；
    /// - `audience`：必须与令牌的 `aud` **逐字相同**（`None` = 不校验 `aud`）；
    /// - `required_claims`：必须出现的**声明名**（如 `["exp", "jti"]`）。
    ///
    /// # 时钟偏移
    ///
    /// leeway **归零**（与 `platform-iam` 的 `JwtKeySet::verify` 同口径）——
    /// 短 TTL 才有意义（「多活一分钟」等于没有 TTL）。
    pub fn verify<T: DeserializeOwned>(
        &self,
        token: &str,
        issuer: Option<&str>,
        audience: Option<&str>,
        required_claims: &[&str],
    ) -> Result<T, VerifyError> {
        let header = decode_header(token).map_err(|_| VerifyError::Malformed)?;
        let kid = header.kid.ok_or(VerifyError::UnknownKey)?;
        let key = self.decoding.get(&kid).ok_or(VerifyError::UnknownKey)?;

        let mut validation = Validation::new(Algorithm::ES256);
        validation.leeway = 0;
        validation.validate_exp = true;
        validation.validate_nbf = true;
        // `aud` 由调用方**逐字**判定（OAuth 令牌的 `aud` 是 client_id）。
        validation.validate_aud = false;
        // `exp` 由 `validate_exp` 强制；其余必需声明在**解码后**逐个查
        // （jsonwebtoken 的 `required_spec_claims` 只认 exp/nbf/aud/iss/sub，
        //  其余名字会被**静默忽略** —— 那正是「登记了却不检查」的假绿）。
        validation.set_required_spec_claims(&["exp"]);
        if let Some(issuer) = issuer {
            validation.iss = Some(HashSet::from([issuer.to_string()]));
        }

        let token_data = decode::<serde_json::Value>(token, key, &validation)
            .map_err(|_| VerifyError::Invalid)?;
        let claims = token_data.claims;
        for claim in required_claims {
            if !claims.get(*claim).is_some() {
                return Err(VerifyError::Invalid);
            }
        }
        if let Some(audience) = audience {
            let actual = claims
                .get("aud")
                .and_then(|aud| aud.as_str().map(str::to_string))
                .or_else(|| {
                    claims
                        .get("aud")
                        .and_then(|aud| aud.as_array())
                        .and_then(|items| items.first())
                        .and_then(|item| item.as_str().map(str::to_string))
                });
            if actual.as_deref() != Some(audience) {
                return Err(VerifyError::Invalid);
            }
        }
        serde_json::from_value(claims).map_err(|_| VerifyError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_iam::config::JwtSettings;
    use platform_iam::jwt::{AccessClaims, JwtKeySet, generate_key_pair};
    use serde::Serialize;

    fn settings(pem: &str, kid: &str) -> JwtSettings {
        JwtSettings {
            issuer: "base".to_string(),
            audience: "base-api".to_string(),
            private_key_pem: pem.to_string(),
            kid: kid.to_string(),
            previous_private_key_pem: None,
            previous_kid: None,
        }
    }

    #[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
    struct Claims {
        iss: String,
        aud: String,
        jti: String,
        exp: i64,
    }

    fn claims(issuer: &str, audience: &str) -> Claims {
        Claims {
            iss: issuer.to_string(),
            aud: audience.to_string(),
            jti: "jti-1".to_string(),
            exp: time::OffsetDateTime::now_utc().unix_timestamp() + 900,
        }
    }

    #[test]
    fn a_token_signed_by_the_platform_verifies() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let token = keys
            .sign_claims(&claims("base", "client-a"))
            .expect("签发应当成功");

        let view = PlatformKeys::from_key_set(&keys);
        let decoded: Claims = view
            .verify(&token, Some("base"), Some("client-a"), &["exp", "jti"])
            .expect("验签应当成功");
        assert_eq!(decoded.aud, "client-a");
    }

    /// 换一把密钥签的 → 验签失败。
    #[test]
    fn a_token_signed_by_another_key_is_rejected() {
        let pem_a = generate_key_pair().expect("密钥应当能生成");
        let pem_b = generate_key_pair().expect("密钥应当能生成");
        let keys_a = JwtKeySet::new(&settings(&pem_a, "kid-a")).expect("密钥集应当能构造");
        let keys_b = JwtKeySet::new(&settings(&pem_b, "kid-b")).expect("密钥集应当能构造");
        let token = keys_b
            .sign_claims(&claims("base", "client-a"))
            .expect("签发应当成功");

        let view = PlatformKeys::from_key_set(&keys_a);
        assert_eq!(
            view.verify::<Claims>(&token, None, None, &["exp"]),
            Err(VerifyError::UnknownKey)
        );
    }

    /// `iss` 不符 → 失败。
    #[test]
    fn a_token_with_another_issuer_is_rejected() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let token = keys
            .sign_claims(&claims("other", "client-a"))
            .expect("签发应当成功");
        let view = PlatformKeys::from_key_set(&keys);
        assert_eq!(
            view.verify::<Claims>(&token, Some("base"), None, &["exp"]),
            Err(VerifyError::Invalid)
        );
    }

    /// `aud` 不符 → 失败（`aud` 是 client_id 的场景）。
    #[test]
    fn a_token_with_another_audience_is_rejected() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let token = keys
            .sign_claims(&claims("base", "client-b"))
            .expect("签发应当成功");
        let view = PlatformKeys::from_key_set(&keys);
        assert_eq!(
            view.verify::<Claims>(&token, Some("base"), Some("client-a"), &["exp"]),
            Err(VerifyError::Invalid)
        );
    }

    /// 缺必需声明 → 失败。
    #[test]
    fn a_token_missing_a_required_claim_is_rejected() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let token = keys
            .sign_claims(&claims("base", "client-a"))
            .expect("签发应当成功");
        let view = PlatformKeys::from_key_set(&keys);
        assert_eq!(
            view.verify::<Claims>(&token, Some("base"), None, &["exp", "nope"]),
            Err(VerifyError::Invalid)
        );
    }

    /// 登录令牌**不能**被当成 OAuth 令牌验（`aud` / 声明集合不同）。
    #[test]
    fn a_login_token_is_not_an_oauth_token() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let token = keys
            .sign(&AccessClaims {
                subject: "user-1".to_string(),
                tenant_id: "tenant-a".to_string(),
                session_id: "session-1".to_string(),
                issuer: "base".to_string(),
                audience: "base-api".to_string(),
                issued_at: now,
                not_before: now,
                expires_at: now + 900,
                token_id: "jti-1".to_string(),
            })
            .expect("签发应当成功");

        let view = PlatformKeys::from_key_set(&keys);
        // `aud` 是平台 audience，不是 client_id → 拒。
        assert_eq!(
            view.verify::<Claims>(&token, Some("base"), Some("client-a"), &["exp"]),
            Err(VerifyError::Invalid)
        );
    }
}
