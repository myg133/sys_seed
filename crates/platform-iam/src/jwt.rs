//! JWT：签发、验签、密钥轮换与 JWKS 发布。
//!
//! # 密钥管理（design-summary §2.2 / D3）
//!
//! - **密钥只从环境注入**，不入库、不入 Git；
//! - 密钥带 **`kid`**，JWKS 同时暴露当前与上一把（**轮换宽限期**内双有效）；
//! - **JWKS 只暴露公钥** —— 私钥绝不出现在响应里。
//!
//! # 为什么是混合令牌里的「JWT 那一半」
//!
//! access token 自包含（**不查库**），高频接口上延迟可接受；
//! 撤销能力由 refresh token（不透明、存 hash、可立即撤销）承担。

use std::collections::HashSet;

use jsonwebtoken::jwk::{Jwk, JwkSet, PublicKeyUse};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode,
};
use serde::{Deserialize, Serialize};

use crate::config::JwtSettings;
use crate::error::IamError;

/// 签名算法：**ES256**（P-256 + SHA-256）。
///
/// 选椭圆曲线而不是 RSA：JWKS 里的公钥短得多，签发/验签更快，低配机上更划算。
pub const SIGNING_ALGORITHM: Algorithm = Algorithm::ES256;

/// access token 的声明（claim）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessClaims {
    /// 用户 ID（`sub`）。
    #[serde(rename = "sub")]
    pub subject: String,
    /// 租户 ID（`tid`）—— **所有令牌都带租户维度**（D11）。
    #[serde(rename = "tid")]
    pub tenant_id: String,
    /// 会话 ID（`sid`）—— 用于「踢下线」后让旧 access token 失效的判定。
    #[serde(rename = "sid")]
    pub session_id: String,
    /// 签发者（`iss`）。
    #[serde(rename = "iss")]
    pub issuer: String,
    /// 受众（`aud`）。
    #[serde(rename = "aud")]
    pub audience: String,
    /// 签发时间（秒，UTC）。
    #[serde(rename = "iat")]
    pub issued_at: i64,
    /// 生效时间（秒，UTC）。
    #[serde(rename = "nbf")]
    pub not_before: i64,
    /// 过期时间（秒，UTC）。
    #[serde(rename = "exp")]
    pub expires_at: i64,
    /// 令牌唯一 ID（`jti`）。
    #[serde(rename = "jti")]
    pub token_id: String,
}

/// 一把密钥（当前或上一把）。
#[derive(Debug, Clone)]
pub struct KeyEntry {
    /// 密钥标识。
    pub kid: String,
    /// 签名用。
    encoding: EncodingKey,
    /// 验签用。
    decoding: DecodingKey,
    /// **公钥**的 JWK（JWKS 用；私钥不在这里）。
    jwk: Jwk,
}

impl KeyEntry {
    /// 从 PKCS#8 PEM 与 `kid` 构造。
    fn from_pem(kid: &str, pem: &str) -> Result<Self, IamError> {
        let encoding =
            EncodingKey::from_ec_pem(pem.as_bytes()).map_err(|err| IamError::Internal {
                reason: format!("the JWT signing key is not a valid PKCS#8 EC key: {err}"),
            })?;
        let mut jwk = Jwk::from_encoding_key(&encoding, SIGNING_ALGORITHM).map_err(|err| {
            IamError::Internal {
                reason: format!("the JWT public key could not be derived: {err}"),
            }
        })?;
        jwk.common.key_id = Some(kid.to_string());
        jwk.common.public_key_use = Some(PublicKeyUse::Signature);
        let decoding = DecodingKey::from_jwk(&jwk).map_err(|err| IamError::Internal {
            reason: format!("the JWT verification key could not be derived: {err}"),
        })?;
        Ok(Self {
            kid: kid.to_string(),
            encoding,
            decoding,
            jwk,
        })
    }
}

/// 密钥集：当前密钥 + （可选）上一把密钥。
///
/// 轮换策略：**当前密钥 + 上一把密钥**同时有效一小段时间，客户端按 `kid` 选。
#[derive(Debug, Clone)]
pub struct JwtKeySet {
    current: KeyEntry,
    previous: Option<KeyEntry>,
}

impl JwtKeySet {
    /// 按配置构造密钥集。
    pub fn new(settings: &JwtSettings) -> Result<Self, IamError> {
        let current = KeyEntry::from_pem(&settings.kid, &settings.private_key_pem)?;
        let previous = match (
            settings.previous_private_key_pem.as_ref(),
            settings.previous_kid.as_ref(),
        ) {
            (Some(pem), Some(kid)) => Some(KeyEntry::from_pem(kid, pem)?),
            _ => None,
        };
        Ok(Self { current, previous })
    }

    /// 当前密钥的 `kid`。
    pub fn current_kid(&self) -> &str {
        &self.current.kid
    }

    /// 签发 access token（**用当前密钥**）。
    ///
    /// # 实现说明
    ///
    /// 本方法**内部转调** [`Self::sign_claims`] —— 签出来的令牌**逐字节相同**
    /// （同一个 header、同一个密钥、同一个编码）。
    pub fn sign(&self, claims: &AccessClaims) -> Result<String, IamError> {
        self.sign_claims(claims)
    }

    /// 签发**任意**声明的令牌（**用当前密钥**）。
    ///
    /// # 为什么需要它（REQ-022 D109 #1）
    ///
    /// 登录令牌与 OAuth 令牌**共用同一套 ES256 密钥集**（第三方用平台 JWKS
    /// 自行验签，D198），但两者的 claim 集合**不同**：
    ///
    /// | | 登录令牌 | OAuth 令牌 |
    /// |---|---|---|
    /// | `aud` | 平台 audience | **client_id** |
    /// | `sub` | 必有 | `client_credentials` 时**省略该键** |
    /// | `sid` | 必有 | 无 |
    /// | `scope` / `grant_type` / `tid` | 无 | 有 |
    ///
    /// `sign` 被**特化**到 `AccessClaims` → OAuth 侧无法复用 →
    /// 于是这里提供泛型版本。
    ///
    /// # ⚠️ 泛型只放宽了「claim 的形状」，**没有**放宽别的
    ///
    /// 密钥、算法（恒为 ES256）、`kid` 与错误语义都**不变** ——
    /// OAuth 令牌因此与登录令牌**同键同算法**，第三方验签路径一致。
    pub fn sign_claims<T: Serialize>(&self, claims: &T) -> Result<String, IamError> {
        let mut header = Header::new(SIGNING_ALGORITHM);
        header.kid = Some(self.current.kid.clone());
        encode(&header, claims, &self.current.encoding).map_err(|err| IamError::Internal {
            reason: format!("the access token could not be signed: {err}"),
        })
    }

    /// 验签并返回声明。
    ///
    /// 按 `kid` 选密钥：当前或上一把（**宽限期内旧令牌仍可用**）。
    pub fn verify(
        &self,
        token: &str,
        issuer: &str,
        audience: &str,
    ) -> Result<AccessClaims, IamError> {
        let header = decode_header(token).map_err(|_| IamError::Unauthenticated)?;
        let kid = header.kid.ok_or(IamError::Unauthenticated)?;
        let entry = self.entry_by_kid(&kid).ok_or(IamError::Unauthenticated)?;

        let mut validation = Validation::new(SIGNING_ALGORITHM);
        // ⚠️ jsonwebtoken 的默认 leeway 是 **60 秒** —— 对 access token 来说
        // 那是「多活一分钟」，必须归零（短 TTL 才有意义）。
        validation.leeway = 0;
        validation.set_required_spec_claims(&["exp", "sub", "iss", "aud"]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.validate_aud = true;
        validation.aud = Some(HashSet::from([audience.to_string()]));
        validation.iss = Some(HashSet::from([issuer.to_string()]));

        match decode::<AccessClaims>(token, &entry.decoding, &validation) {
            Ok(data) => Ok(data.claims),
            Err(err) => Err(match err.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => IamError::TokenExpired,
                _ => IamError::Unauthenticated,
            }),
        }
    }

    /// 按 `kid` 找密钥（当前 → 上一把）。
    fn entry_by_kid(&self, kid: &str) -> Option<&KeyEntry> {
        if self.current.kid == kid {
            return Some(&self.current);
        }
        self.previous.as_ref().filter(|entry| entry.kid == kid)
    }

    /// JWKS：**只暴露公钥**。
    ///
    /// 轮换宽限期内同时暴露两个 `kid`。
    pub fn jwks(&self) -> JwkSet {
        let mut keys = vec![self.current.jwk.clone()];
        if let Some(previous) = &self.previous {
            keys.push(previous.jwk.clone());
        }
        JwkSet { keys }
    }
}

/// 生成一对新的 P-256 密钥（PKCS#8 PEM）。
///
/// 用途：本地开发与测试夹具；生产密钥由运维离线生成后经环境变量注入
/// （**绝不能**把生成的私钥提交进 Git）。
pub fn generate_key_pair() -> Result<String, IamError> {
    use p256::ecdsa::SigningKey;
    use p256::pkcs8::{EncodePrivateKey, LineEnding};

    let signing_key = SigningKey::random(&mut rand::rngs::OsRng);
    let pem = signing_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|err| IamError::Internal {
            reason: format!("the JWT key pair could not be generated: {err}"),
        })?;
    Ok(pem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一份 JWT 设置。
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

    /// 造一份声明。
    fn claims(exp_offset: i64) -> AccessClaims {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        AccessClaims {
            subject: "user-1".to_string(),
            tenant_id: "tenant-a".to_string(),
            session_id: "session-1".to_string(),
            issuer: "base".to_string(),
            audience: "base-api".to_string(),
            issued_at: now,
            not_before: now,
            expires_at: now + exp_offset,
            token_id: "jti-1".to_string(),
        }
    }

    #[test]
    fn a_signed_token_round_trips() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let token = keys.sign(&claims(900)).expect("签发应当成功");
        let decoded = keys
            .verify(&token, "base", "base-api")
            .expect("验签应当成功");
        assert_eq!(decoded.subject, "user-1");
        assert_eq!(decoded.tenant_id, "tenant-a");
        assert_eq!(decoded.session_id, "session-1");
    }

    #[test]
    fn an_expired_token_is_reported_as_expired() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let token = keys.sign(&claims(-10)).expect("签发应当成功");
        let error = keys
            .verify(&token, "base", "base-api")
            .expect_err("过期必须被拒");
        assert_eq!(error.code(), "TOKEN_EXPIRED");
    }

    #[test]
    fn a_token_signed_by_another_key_is_rejected() {
        let pem_a = generate_key_pair().expect("密钥应当能生成");
        let pem_b = generate_key_pair().expect("密钥应当能生成");
        let keys_a = JwtKeySet::new(&settings(&pem_a, "kid-a")).expect("密钥集应当能构造");
        let keys_b = JwtKeySet::new(&settings(&pem_b, "kid-b")).expect("密钥集应当能构造");
        let token = keys_b.sign(&claims(900)).expect("签发应当成功");
        let error = keys_a
            .verify(&token, "base", "base-api")
            .expect_err("别的密钥必须被拒");
        assert_eq!(error.code(), "UNAUTHENTICATED");
    }

    /// AC-S-010：轮换后旧令牌在宽限期内仍可用，JWKS 同时暴露两个 kid。
    #[test]
    fn rotation_keeps_the_previous_key_valid_and_publishes_both_kids() {
        let pem_old = generate_key_pair().expect("密钥应当能生成");
        let pem_new = generate_key_pair().expect("密钥应当能生成");

        let mut rotated = settings(&pem_new, "kid-new");
        rotated.previous_private_key_pem = Some(pem_old.clone());
        rotated.previous_kid = Some("kid-old".to_string());
        let keys = JwtKeySet::new(&rotated).expect("密钥集应当能构造");

        // 旧密钥签发的令牌在宽限期内仍能验签。
        let old_keys = JwtKeySet::new(&settings(&pem_old, "kid-old")).expect("旧密钥集应当能构造");
        let old_token = old_keys.sign(&claims(900)).expect("签发应当成功");
        let decoded = keys
            .verify(&old_token, "base", "base-api")
            .expect("宽限期内旧令牌应当仍可用");
        assert_eq!(decoded.subject, "user-1");

        // 新令牌用新密钥签发。
        let new_token = keys.sign(&claims(900)).expect("签发应当成功");
        assert!(keys.verify(&new_token, "base", "base-api").is_ok());

        // JWKS 同时暴露两个 kid。
        let jwks = keys.jwks();
        assert_eq!(jwks.keys.len(), 2);
        let kids: Vec<&str> = jwks
            .keys
            .iter()
            .filter_map(|key| key.common.key_id.as_deref())
            .collect();
        assert!(kids.contains(&"kid-new"), "kids = {kids:?}");
        assert!(kids.contains(&"kid-old"), "kids = {kids:?}");
    }

    /// JWKS **只暴露公钥** —— 私钥绝不出现在响应里。
    #[test]
    fn the_jwks_never_contains_private_key_material() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let json = serde_json::to_string(&keys.jwks()).expect("序列化应当成功");

        assert!(!json.contains("PRIVATE"), "JWKS 里不得出现私钥标记：{json}");
        // 逐个 key 检查成员：**不得**出现私钥成员（EC 私钥的 `d`、RSA 的 `d`/`p`/`q`…）。
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("JWKS 必须是 JSON");
        let keys = parsed["keys"].as_array().expect("JWKS 里必须有 keys 数组");
        assert_eq!(keys.len(), 1);
        let key = &keys[0];
        for forbidden in ["d", "p", "q", "dp", "dq", "qi", "k"] {
            assert!(
                key.get(forbidden).is_none(),
                "JWKS 里不得出现私钥成员 `{forbidden}`：{json}"
            );
        }
        // 公钥参数必须在（x / y / kty / crv）。
        assert_eq!(key["kty"], "EC", "jwks = {json}");
        assert_eq!(key["crv"], "P-256", "jwks = {json}");
        assert!(key["x"].is_string(), "jwks = {json}");
        assert!(key["y"].is_string(), "jwks = {json}");
        assert_eq!(key["kid"], "kid-1", "jwks = {json}");
    }

    #[test]
    fn a_token_for_another_audience_is_rejected() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let token = keys.sign(&claims(900)).expect("签发应当成功");
        let error = keys
            .verify(&token, "base", "another-api")
            .expect_err("受众不匹配必须被拒");
        assert_eq!(error.code(), "UNAUTHENTICATED");
    }

    #[test]
    fn a_garbage_token_is_rejected() {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let keys = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        assert!(keys.verify("not-a-token", "base", "base-api").is_err());
        assert!(keys.verify("", "base", "base-api").is_err());
    }
}
