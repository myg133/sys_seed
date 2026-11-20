//! 登录审计埋点（REQ-014 AC-S-127 / D163 / D157）。
//!
//! # 事件码与形状（**照抄** D163，不得自行改名）
//!
//! | 事件 | 事件码 | `object_type` | `before` / `after` |
//! |------|--------|---------------|-------------------|
//! | 本地 / LDAP 登录成功 | `iam.login.succeeded` | `iam_session` | `NULL` / `NULL` |
//! | 本地 / LDAP 登录失败 | `iam.login.failed` | `iam_session` | `NULL` / `NULL` |
//! | SSO（OIDC）登录成功 | `iam.sso.login.succeeded` | `iam_sso_session` | `NULL` / `NULL` |
//! | SSO（OIDC）登录失败 | `iam.sso.login.failed` | `iam_sso_session` | `NULL` / `NULL` |
//!
//! # 🔑 失败事件**不得含账号名**（AC-S-127 步骤 5 的不可枚举判据）
//!
//! 失败事件的 `actor_id` / `object_id` / `before` / `after` **全部为空** ——
//! 只带「动作 + 对象类型 + IP + UA」。
//! 带了账号名 → 「用不存在的账号登录失败」与「用真实账号登录失败」产生的记录**不同**
//! → 审计表本身就成了枚举预言机。
//!
//! # 失败条数**同量级**（AC-S-127 步骤 5）
//!
//! 「账号不存在」与「密码错误」**各产生恰好 1 条** `iam.login.failed` ——
//! 两条路径的埋点调用点**完全对称**。
//!
//! # 审计写不进去 → **拒绝登录**（D157）
//!
//! 「宁可不登录，不可无审计地登录」：审计 sink 写入失败 → 503
//! `AUDIT_SINK_UNAVAILABLE`。
//!
//! ⚠️ **未装配 sink ≠ 写入失败**：未装配（单测、裸用服务）→ **跳过埋点**
//! （并记一条 warn，「以为在审计其实没有」比没有更危险）；
//! **装配了但写失败** → 拒绝。

use platform_core::extension::audit::AuditEvent;

/// 本地 / LDAP 登录成功。
pub const EVENT_LOGIN_SUCCEEDED: &str = "iam.login.succeeded";
/// 本地 / LDAP 登录失败。
pub const EVENT_LOGIN_FAILED: &str = "iam.login.failed";
/// SSO（OIDC）登录成功。
pub const EVENT_SSO_LOGIN_SUCCEEDED: &str = "iam.sso.login.succeeded";
/// SSO（OIDC）登录失败。
pub const EVENT_SSO_LOGIN_FAILED: &str = "iam.sso.login.failed";

/// 本地 / LDAP 登录的对象类型。
pub const OBJECT_TYPE_SESSION: &str = "iam_session";
/// SSO 登录的对象类型。
pub const OBJECT_TYPE_SSO_SESSION: &str = "iam_sso_session";

/// 造一条登录事件。
///
/// # 参数
///
/// - `tenant_id`：租户（链的边界）；
/// - `actor_id`：操作人。**失败时传 `None`** —— 失败时用户尚未通过认证；
/// - `ip` / `user_agent`：来源（**客户端可控的头**，用于关联与排查，不是取证证据，D149）；
/// - `sso`：`true` = SSO（OIDC）事件码与对象类型；`false` = 本地 / LDAP。
///
/// ⚠️ **没有账号名参数** —— 这是**刻意的**：本函数在结构上**无法**产出
/// 含账号名的失败事件（不可枚举判据是结构性的，不是靠「记得别塞」）。
pub fn login_event(
    tenant_id: &str,
    actor_id: Option<&str>,
    succeeded: bool,
    sso: bool,
    ip: &str,
    user_agent: &str,
) -> AuditEvent {
    let (action, object_type) = match (sso, succeeded) {
        (false, true) => (EVENT_LOGIN_SUCCEEDED, OBJECT_TYPE_SESSION),
        (false, false) => (EVENT_LOGIN_FAILED, OBJECT_TYPE_SESSION),
        (true, true) => (EVENT_SSO_LOGIN_SUCCEEDED, OBJECT_TYPE_SSO_SESSION),
        (true, false) => (EVENT_SSO_LOGIN_FAILED, OBJECT_TYPE_SSO_SESSION),
    };

    let mut event = AuditEvent::new(tenant_id, action, object_type);
    if let Some(actor) = actor_id {
        event = event.with_actor(actor);
    }
    // `ip` / `user_agent` 取不到时落**空串**（不是 `NULL`）—— AC-S-122 要求不得缺键。
    event = event.with_ip(ip).with_user_agent(user_agent);
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_event_codes_and_object_types_follow_d163() {
        let cases = [
            (false, true, EVENT_LOGIN_SUCCEEDED, OBJECT_TYPE_SESSION),
            (false, false, EVENT_LOGIN_FAILED, OBJECT_TYPE_SESSION),
            (
                true,
                true,
                EVENT_SSO_LOGIN_SUCCEEDED,
                OBJECT_TYPE_SSO_SESSION,
            ),
            (true, false, EVENT_SSO_LOGIN_FAILED, OBJECT_TYPE_SSO_SESSION),
        ];
        for (sso, succeeded, action, object_type) in cases {
            let event = login_event("tenant-a", Some("u-1"), succeeded, sso, "1.2.3.4", "agent");
            assert_eq!(event.action, action);
            assert_eq!(event.object_type, object_type);
        }
    }

    /// 🔑 失败事件**不得含账号名**（AC-S-127 步骤 5）。
    #[test]
    fn a_failed_login_carries_no_account_identifier() {
        let event = login_event("tenant-a", None, false, false, "1.2.3.4", "agent");
        assert!(event.actor_id.is_none(), "失败事件不得有操作人");
        assert!(event.object_id.is_none(), "失败事件不得有对象 ID");
        assert!(event.before.is_none(), "失败事件不得有变更前值");
        assert!(event.after.is_none(), "失败事件不得有变更后值");
        assert_eq!(event.ip.as_deref(), Some("1.2.3.4"));
        assert_eq!(event.user_agent.as_deref(), Some("agent"));
    }

    #[test]
    fn a_successful_login_carries_the_actor() {
        let event = login_event("tenant-a", Some("u-1"), true, false, "", "");
        assert_eq!(event.actor_id.as_deref(), Some("u-1"));
    }
}
