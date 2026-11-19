//! SSO 登录票据（`loginTicket`）—— 回调 → 前端的一次性交接（P4 / D124 决策 ①）。
//!
//! # 为什么需要它
//!
//! `/callback` 是**顶层浏览器导航**的落点。返回 `200 + JSON` 会被浏览器**渲染成文本**，
//! SPA 拿不到 `refreshToken`。而把 token 放进 URL 又会进浏览器历史、`Referer` 头、
//! 服务端访问日志（§8）。
//!
//! 所以：
//!
//! ```text
//! GET /callback?code&state → 校验 + 建会话 → 302 {前端基址}/sso/callback?ticket=…
//! POST /sso/exchange {ticket} → 消费 → 200 LoginResponse（token 只在响应体里）
//! ```
//!
//! # 票据的三条性质
//!
//! - **一次性**：`consumed_at` 非空即拒
//! - **短命**：TTL **30 秒**（代码常量，不新增配置键 —— P9）
//! - **绑租户**：`ticket.tenant_id` 必须与请求租户一致（有请求头时）
//!
//! **表里只存 `session_id`，不存任何 token。**

use std::time::Duration;

use platform_core::RequestContext;
use time::OffsetDateTime;

use crate::error::IamError;
use crate::store::{IamStore, SsoTicketRecord};

use super::oidc::random_token;

/// 票据 TTL —— **代码常量**。
pub const SSO_TICKET_TTL: Duration = Duration::from_secs(30);

/// 生成一个票据串（base64url(32B)）。
pub fn new_ticket() -> String {
    random_token()
}

/// 落一条票据（回调建好会话之后）。
pub async fn issue(
    store: &IamStore,
    ctx: &RequestContext,
    ticket: &str,
    user_id: &str,
    session_id: &str,
) -> Result<(), IamError> {
    let record = SsoTicketRecord {
        ticket: ticket.to_string(),
        tenant_id: ctx
            .tenant_id()
            .ok_or(IamError::TenantContextMissing)?
            .to_string(),
        user_id: user_id.to_string(),
        session_id: session_id.to_string(),
        created_at: OffsetDateTime::now_utc(),
        expires_at: OffsetDateTime::now_utc() + SSO_TICKET_TTL,
        consumed_at: None,
    };
    store.insert_sso_ticket(ctx, &record).await.map_err(|err| {
        tracing::error!(error = %err, "the sso ticket could not be stored");
        IamError::Internal {
            reason: "the data access layer failed".to_string(),
        }
    })
}

/// 校验并**消费**一个票据，返回票据记录（含 `session_id` / `tenant_id`）。
///
/// 所有失败（不存在 / 已用 / 过期 / 租户不匹配 / 并发消费）都返回**同一个**
/// `401 SSO_TICKET_INVALID` —— 票据是未认证端点上的凭证，
/// 区分失败原因只会给攻击者提供探测信号。
pub async fn consume(
    store: &IamStore,
    ticket: &str,
    header_tenant: Option<&str>,
) -> Result<SsoTicketRecord, IamError> {
    let record = store
        .find_sso_ticket(ticket)
        .await
        .map_err(internal)?
        .ok_or(IamError::SsoTicketInvalid)?;

    if record.consumed_at.is_some() {
        return Err(IamError::SsoTicketInvalid);
    }
    if record.expires_at <= OffsetDateTime::now_utc() {
        return Err(IamError::SsoTicketInvalid);
    }
    if let Some(tenant) = header_tenant {
        if tenant != record.tenant_id {
            return Err(IamError::SsoTicketInvalid);
        }
    }

    // 原子消费（**租户作用域**）。
    let consumed = store
        .consume_sso_ticket(&record.tenant_id, ticket)
        .await
        .map_err(internal)?;
    if consumed == 0 {
        return Err(IamError::SsoTicketInvalid);
    }
    Ok(record)
}

/// 数据库错误 → 内部错误。
fn internal(err: platform_db::DbError) -> IamError {
    tracing::error!(error = %err, "the sso ticket data access failed");
    IamError::Internal {
        reason: "the data access layer failed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_ttl_is_thirty_seconds() {
        assert_eq!(SSO_TICKET_TTL.as_secs(), 30);
    }

    #[test]
    fn tickets_are_high_entropy_and_unique() {
        use std::collections::BTreeSet;
        let mut seen = BTreeSet::new();
        for _ in 0..20 {
            let ticket = new_ticket();
            assert!(ticket.len() >= 43);
            assert!(
                ticket
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            );
            assert!(seen.insert(ticket));
        }
    }
}
