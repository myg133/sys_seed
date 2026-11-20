//! 单点登出编排（REQ-022，design §4.5 / AC-S-149 / AC-S-150）。
//!
//! # 顺序（**不得**调换）
//!
//! 1. 撤销该用户的**全部** grant（`platform_oauth_grant.status = revoked`）；
//! 2. 作废**全部**会话（refresh token + session 行）；
//! 3. 收集有 `backchannel_logout_uri` 的 client → **并发** POST `logout_token`；
//! 4. 收集有 `frontchannel_logout_uri` 的 client → 返回 `frontChannel` 数组；
//! 5. 埋审计 `oauth.logout.executed`。
//!
//! # ⚠️ 通知失败**绝不能**让登出失败（D211）
//!
//! 出站投递**超时 5 秒**、**并发**进行、**失败只记日志** ——
//! 登出的主流程（撤销 grant + 作废会话）**已经**完成 → HTTP 200。
//!
//! # ⚠️ 已签发的**登录 access token 不杀**（D229 / TD-043）
//!
//! 平台的登录 access token 是**自包含 JWT**（验签不查库）→
//! 最长延迟一个 `iam.accessTokenTtl` 秒才失效。
//! 而 **OAuth 令牌**由 `platform_oauth_token` 表的状态决定 →
//! 撤销 grant 后**立即**失效（D210）。
//!
//! # `sid` 的取值（D230）
//!
//! `sid` = **触发登出的这次请求所使用的会话 ID** ——
//! 一个登出请求 = **一个** `logout_token`；
//! 「作废全部会话」是**平台侧**的动作，**不**意味着发多个 `logout_token`。

use std::collections::BTreeMap;

use platform_core::RequestContext;
use uuid::Uuid;

use crate::error::ServiceError;
use crate::events::{code as event_code, object_type};
use crate::service::LogoutTokenClaims;
use crate::service::{AuthenticatedUser, FrontChannelEntry, LoggedOutCounts, LogoutResult};

/// back-channel 登出的事件键（OIDC Back-Channel Logout 规范）。
pub const BACKCHANNEL_LOGOUT_EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

impl crate::service::OAuthService {
    /// 全局登出：撤销全部授权 + 作废全部会话 + 通知 client。
    ///
    /// # 通知范围
    ///
    /// 只通知**该用户确实授权过**、**且**登记了对应回调地址的 client
    /// → 不给「用户从未授权过的应用」发 `logout_token`。
    ///
    /// ⚠️ **撤销范围**与通知范围**无关**：即使某个 client 没配 front-channel URI，
    /// 它的 grant **照样**被撤销（AC-S-150 的补充判据：D248）。
    pub async fn logout(
        &self,
        ctx: &RequestContext,
        user: &AuthenticatedUser,
    ) -> Result<LogoutResult, ServiceError> {
        // 1. 撤销该用户的**全部** grant（先取 client 列表 → 通知要用）。
        let grants = self
            .store()
            .list_grants_for_user(ctx, &user.user_id)
            .await
            .map_err(|error| self.internal_logout("the oauth grants could not be listed", error))?;
        let revoked_grants = self
            .store()
            .revoke_all_grants_for_user(ctx, &user.user_id)
            .await
            .map_err(|error| {
                self.internal_logout("the oauth grants could not be revoked", error)
            })?;
        // grant 级撤销 → 该用户**全部** OAuth 令牌立即失效（D210 / D254）。
        self.store()
            .revoke_tokens_for_user(ctx, &user.user_id)
            .await
            .map_err(|error| {
                self.internal_logout("the oauth tokens could not be revoked", error)
            })?;

        // 2. 作废**全部**会话（refresh token + session 行）。
        let revoked_sessions = self
            .iam()
            .logout_all(ctx, &user.user_id)
            .await
            .map_err(|error| self.internal_logout("the sessions could not be revoked", error))?;

        // 3. 收集要通知的 client（**该用户授权过**的）。
        let client_ids: Vec<String> = grants.iter().map(|grant| grant.client_id.clone()).collect();
        let clients = self
            .store()
            .get_clients(&client_ids)
            .await
            .map_err(|error| self.internal_logout("the oauth clients could not be read", error))?;

        // 4. back-channel：签 `logout_token` → 并发投递（失败只记日志）。
        let mut backchannel = Vec::new();
        let mut frontchannel = Vec::new();
        for client in clients {
            if let Some(uri) = client.backchannel_logout_uri.clone() {
                if let Some(token) = self.sign_logout_token(user, &client.client_id)? {
                    // ⚠️ `client_id` **必须**一起带下去：日志要记的是 **clientId**
                    // （AC-S-149 的补充判据），不是投递 URI。
                    backchannel.push((client.client_id.clone(), uri, token));
                }
            }
            if let Some(uri) = client.frontchannel_logout_uri.clone() {
                if let Some(token) = self.sign_logout_token(user, &client.client_id)? {
                    frontchannel.push(FrontChannelEntry {
                        client_id: client.client_id.clone(),
                        logout_url: uri,
                        logout_token: token,
                    });
                }
            }
        }
        self.deliver_backchannel_logouts(backchannel).await;

        // 5. 埋审计。
        self.audit_event(
            ctx,
            event_code::LOGOUT_EXECUTED,
            object_type::SESSION,
            &user.user_id,
            None,
            Some(serde_json::json!({
                "grants": revoked_grants,
                "sessions": revoked_sessions,
                "frontChannel": frontchannel.len(),
            })),
        )
        .await;

        Ok(LogoutResult {
            logged_out: LoggedOutCounts {
                grants: revoked_grants,
                sessions: revoked_sessions,
            },
            // ⚠️ **永远是数组**：没有就是 `[]`，绝不是 `null`（D258）。
            front_channel: frontchannel,
        })
    }

    /// 签 `logout_token`（OIDC Back-Channel Logout）。
    ///
    /// ⚠️ **不含 `nonce`**（D211）。
    fn sign_logout_token(
        &self,
        user: &AuthenticatedUser,
        client_id: &str,
    ) -> Result<Option<String>, ServiceError> {
        let mut events = BTreeMap::new();
        events.insert(BACKCHANNEL_LOGOUT_EVENT.to_string(), serde_json::json!({}));
        let claims = LogoutTokenClaims {
            iss: self.issuer().to_string(),
            aud: client_id.to_string(),
            sub: user.user_id.clone(),
            tid: user.tenant_id.clone(),
            // `sid` = **触发登出的这次请求**的会话 ID（D230）。
            sid: user.session_id.clone(),
            events,
            iat: crate::service::unix_now(),
            jti: Uuid::new_v4().to_string(),
        };
        match self.keys().sign_claims(&claims) {
            Ok(token) => Ok(Some(token)),
            Err(error) => {
                // 签不出令牌 → **该 client 不通知**，但**登出主流程照常**（D211）。
                tracing::warn!(
                    error = %error,
                    client_id,
                    "the logout token could not be signed"
                );
                Ok(None)
            }
        }
    }

    /// **并发**投递 back-channel 登出通知。
    ///
    /// ⚠️ **失败只记日志**（含 `clientId` 与失败原因）—— **绝不影响**登出结果（D211）。
    async fn deliver_backchannel_logouts(&self, deliveries: Vec<(String, String, String)>) {
        if deliveries.is_empty() {
            return;
        }
        let mut tasks = tokio::task::JoinSet::new();
        for (client_id, uri, token) in deliveries {
            let outbound = self.outbound().clone();
            tasks.spawn(async move {
                let body = format!("logout_token={}", encode_form_value(&token));
                let result = outbound.post_form(&uri, &body).await;
                (client_id, result)
            });
        }
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok((client_id, Ok(status))) => {
                    tracing::info!(client_id, status, "the logout token was delivered");
                }
                Ok((client_id, Err(error))) => {
                    // ⚠️ 失败**只记日志**（AC-S-149 的补充判据：要含 clientId 与原因）。
                    tracing::warn!(
                        client_id,
                        reason = error.reason(),
                        "the logout token could not be delivered"
                    );
                }
                Err(error) => {
                    tracing::warn!(error = %error, "the logout delivery task failed");
                }
            }
        }
    }

    /// 数据访问层错误 → 平台内部错误（**不把内部细节漏给调用方**）。
    fn internal_logout<E: std::fmt::Display>(&self, summary: &str, error: E) -> ServiceError {
        tracing::error!(error = %error, summary, "the oauth logout data access failed");
        ServiceError::Internal {
            reason: summary.to_string(),
        }
    }
}

/// form 编码的值（RFC 6749 §2.3.1 的 `application/x-www-form-urlencoded`）。
///
/// `logout_token` 是 JWT → 它的字符集里**没有** `&` / `=` / `+` / 空格 →
//**不必**编码；这里仍走一遍编码 → 将来换别的载荷也不会破。
fn encode_form_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JWT 的字符集**不需要**编码 → 编码是恒等变换。
    #[test]
    fn the_form_encoder_leaves_a_jwt_untouched() {
        let jwt = "eyJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJ1MSJ9.abc-_123";
        assert_eq!(encode_form_value(jwt), jwt);
    }

    /// 真的需要编码的字符**要**被编码。
    #[test]
    fn the_form_encoder_escapes_the_reserved_characters() {
        assert_eq!(encode_form_value("a b"), "a%20b");
        assert_eq!(encode_form_value("a&b=c"), "a%26b%3Dc");
    }
}
