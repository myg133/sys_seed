//! 同意票据（consent ticket）：**ES256 签名、零新表**（REQ-022，D204）。
//!
//! # 为什么用票据而不是「重新接受原始参数」
//!
//! `confirm` / `deny` **只信票据** → 「用户看到的 scope」与「实际授予的 scope」
//! **必然一致**。
//!
//! 若改成「重新接受原始参数 + 取交集」：交集只会更小，但**可能与用户所见不一致**
//! （用户看到 `{read}`，被改成 `{read,write}` → 交集 = `{read,write}` → 知情同意被破坏）。
//!
//! # ⚠️ `typ` **必须**校验（D204）
//!
//! 票据与登录令牌**用同一套密钥**签 → 不校验 `typ` 的话，
//! **一个登录令牌可以被当成同意票据用**（拿它换一枚授权码）。
//!
//! # 一次性
//!
//! 票据本身**无状态**（不落库）→ 5 分钟内可重复 `confirm`；
//! 但**同一请求** confirm 两次 → 第二次授权码已被消费 → 失败。

use std::time::Duration;

use platform_iam::jwt::JwtKeySet;
use serde::{Deserialize, Serialize};

use crate::keys::{PlatformKeys, VerifyError};

/// 票据的 `typ`（**必须**校验，防令牌混用）。
pub const TICKET_TYPE: &str = "oauth_consent_ticket";

/// 票据的有效期（**5 分钟**，D204）。
pub const TICKET_TTL: Duration = Duration::from_secs(300);

/// 同意票据的声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentTicket {
    /// 票据类型（恒为 [`TICKET_TYPE`]）。
    pub typ: String,
    /// 签发者（平台 issuer）。
    pub iss: String,
    /// client ID。
    pub client_id: String,
    /// 回调地址（**精确匹配**白名单）。
    pub redirect_uri: String,
    /// 请求的 scope（**空格分隔**）。
    pub scope: String,
    /// PKCE 挑战。
    pub code_challenge: String,
    /// PKCE 挑战方法（恒为 `S256`）。
    pub code_challenge_method: String,
    /// `state`（防 CSRF；可缺省）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// 用户 ID。
    pub sub: String,
    /// 租户 ID。
    pub tid: String,
    /// 签发时刻（秒）。
    pub iat: i64,
    /// 过期时刻（秒）。
    pub exp: i64,
    /// 票据 ID。
    pub jti: String,
}

impl ConsentTicket {
    /// 票据里的 scope（**数组**形态）。
    pub fn scopes(&self) -> Vec<String> {
        self.scope.split_whitespace().map(str::to_string).collect()
    }
}

/// 签发同意票据。
#[allow(clippy::too_many_arguments)]
pub fn sign(
    keys: &JwtKeySet,
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    code_challenge: &str,
    code_challenge_method: &str,
    state: Option<&str>,
    subject: &str,
    tenant_id: &str,
    now: i64,
) -> Result<String, crate::error::OAuthError> {
    let ticket = ConsentTicket {
        typ: TICKET_TYPE.to_string(),
        iss: issuer.to_string(),
        client_id: client_id.to_string(),
        redirect_uri: redirect_uri.to_string(),
        scope: scope.to_string(),
        code_challenge: code_challenge.to_string(),
        code_challenge_method: code_challenge_method.to_string(),
        state: state.map(str::to_string),
        sub: subject.to_string(),
        tid: tenant_id.to_string(),
        iat: now,
        exp: now + TICKET_TTL.as_secs() as i64,
        jti: crate::crypto::new_secret(),
    };
    keys.sign_claims(&ticket)
        .map_err(|_| crate::error::OAuthError::TicketInvalid)
}

/// 验签同意票据。
///
/// # 失败一律 `TicketInvalid`
///
/// - 验签失败 / 过期 / `typ` 不对 / 缺声明 → **同一个**错误码
///   （不区分原因：不泄露「票据存在但 typ 不对」这类信息）。
pub fn verify(
    keys: &PlatformKeys,
    issuer: &str,
    token: &str,
) -> Result<ConsentTicket, crate::error::OAuthError> {
    let ticket: ConsentTicket = keys
        .verify(token, Some(issuer), None, &["exp", "jti", "typ"])
        .map_err(|error| map_verify_error(error))?;
    if ticket.typ != TICKET_TYPE {
        return Err(crate::error::OAuthError::TicketInvalid);
    }
    Ok(ticket)
}

/// 把验签错误映射成票据错误（**不区分原因**）。
fn map_verify_error(error: VerifyError) -> crate::error::OAuthError {
    match error {
        VerifyError::Malformed | VerifyError::UnknownKey | VerifyError::Invalid => {
            crate::error::OAuthError::TicketInvalid
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_iam::config::JwtSettings;
    use platform_iam::jwt::generate_key_pair;

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

    fn keys() -> (JwtKeySet, PlatformKeys) {
        let pem = generate_key_pair().expect("密钥应当能生成");
        let key_set = JwtKeySet::new(&settings(&pem, "kid-1")).expect("密钥集应当能构造");
        let view = PlatformKeys::from_key_set(&key_set);
        (key_set, view)
    }

    fn now() -> i64 {
        time::OffsetDateTime::now_utc().unix_timestamp()
    }

    fn ticket_token(keys: &JwtKeySet) -> String {
        sign(
            keys,
            "base",
            "client-a",
            "https://client.example/cb",
            "rbac:role:read",
            "challenge",
            "S256",
            Some("xyz"),
            "user-1",
            "tenant-a",
            now(),
        )
        .expect("签发应当成功")
    }

    #[test]
    fn a_ticket_round_trips() {
        let (keys, view) = keys();
        let token = ticket_token(&keys);
        let ticket = verify(&view, "base", &token).expect("验签应当成功");
        assert_eq!(ticket.typ, TICKET_TYPE);
        assert_eq!(ticket.client_id, "client-a");
        assert_eq!(ticket.redirect_uri, "https://client.example/cb");
        assert_eq!(ticket.scope, "rbac:role:read");
        assert_eq!(ticket.code_challenge, "challenge");
        assert_eq!(ticket.code_challenge_method, "S256");
        assert_eq!(ticket.state.as_deref(), Some("xyz"));
        assert_eq!(ticket.sub, "user-1");
        assert_eq!(ticket.tid, "tenant-a");
    }

    /// 票据的 scope 是**空格分隔**的字符串（RFC 6749 §3.3）。
    #[test]
    fn the_ticket_scope_round_trips_as_an_array() {
        let (keys, view) = keys();
        let token = sign(
            &keys,
            "base",
            "client-a",
            "https://client.example/cb",
            "rbac:role:read rbac:role:write",
            "challenge",
            "S256",
            None,
            "user-1",
            "tenant-a",
            now(),
        )
        .expect("签发应当成功");
        let ticket = verify(&view, "base", &token).expect("验签应当成功");
        assert_eq!(
            ticket.scopes(),
            vec!["rbac:role:read".to_string(), "rbac:role:write".to_string()]
        );
    }

    /// ⚠️ **签名有效但 `typ` 不对** → 拒（D204 的核心）。
    #[test]
    fn a_token_with_another_type_is_rejected() {
        let (keys, view) = keys();
        // 用**同一套密钥**签一个 `typ` 不同的声明 → 签名有效，但必须被拒。
        let forged = serde_json::json!({
            "typ": "not_a_consent_ticket",
            "iss": "base",
            "client_id": "client-a",
            "redirect_uri": "https://client.example/cb",
            "scope": "rbac:role:write",
            "code_challenge": "challenge",
            "code_challenge_method": "S256",
            "sub": "user-1",
            "tid": "tenant-a",
            "iat": now(),
            "exp": now() + 300,
            "jti": "jti-1",
        });
        let token = keys.sign_claims(&forged).expect("签发应当成功");
        assert_eq!(
            verify(&view, "base", &token),
            Err(crate::error::OAuthError::TicketInvalid)
        );
    }

    /// **登录令牌不能当票据用**（它没有 `typ` 键）。
    #[test]
    fn a_login_token_is_not_a_ticket() {
        let (keys, view) = keys();
        let now = now();
        let login = keys
            .sign(&platform_iam::jwt::AccessClaims {
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
        assert_eq!(
            verify(&view, "base", &login),
            Err(crate::error::OAuthError::TicketInvalid)
        );
    }

    /// 篡改一个字节 → 验签失败。
    #[test]
    fn a_tampered_ticket_is_rejected() {
        let (keys, view) = keys();
        let token = ticket_token(&keys);
        let mut parts: Vec<&str> = token.split('.').collect();
        // 改 payload 的一个字符。
        let payload = parts[1].to_string();
        let mut chars: Vec<char> = payload.chars().collect();
        let index = chars.len() / 2;
        chars[index] = if chars[index] == 'a' { 'b' } else { 'a' };
        let new_payload: String = chars.into_iter().collect();
        parts[1] = &new_payload;
        let tampered = parts.join(".");
        assert_eq!(
            verify(&view, "base", &tampered),
            Err(crate::error::OAuthError::TicketInvalid)
        );
    }

    /// 过期 → 拒。
    #[test]
    fn an_expired_ticket_is_rejected() {
        let (keys, view) = keys();
        let expired = now() - TICKET_TTL.as_secs() as i64 - 10;
        let token = sign(
            &keys,
            "base",
            "client-a",
            "https://client.example/cb",
            "rbac:role:read",
            "challenge",
            "S256",
            None,
            "user-1",
            "tenant-a",
            expired,
        )
        .expect("签发应当成功");
        assert_eq!(
            verify(&view, "base", &token),
            Err(crate::error::OAuthError::TicketInvalid)
        );
    }
}
