//! 既有 `SsoAuditRecorder` → 平台 sink 的**桥接**（REQ-014 D135 / D150）。
//!
//! # 为什么不改既有 trait 的签名
//!
//! D135 钉死：`platform-iam` 的 `SsoAuditRecorder`（绑死 `SsoAuditEvent`）
//! **签名一律不改** —— REQ-013 已合并，改签名会连带破坏既有测试
//! （`platform-iam` 129 条用例 + SSO 真实库用例）。
//! → 这里实现**同一个 trait**，把事件翻译成通用 `AuditEvent` 后交给平台 sink。
//!
//! # 为什么只桥接 `link.created`
//!
//! 两个**登录**事件码（`iam.sso.login.succeeded` / `iam.sso.login.failed`）
//! 由 [`crate::service::IamService`] **自己**写（**await** 平台 sink）。
//!
//! 原因：**D157 要求「审计写不进去 → 拒绝登录（503）」**，
//! 而 `SsoAuditRecorder::record` 是**同步**的 —— 它**表达不了**「写失败要拒绝」。
//! 服务自己写 → 能 `await` → 写失败就返回 `AUDIT_SINK_UNAVAILABLE`。
//!
//! ⚠️ 如果这里**也**桥接那两个登录事件码 → 同一次登录会产生**两条**同样的记录
//! → AC-S-127 的「产生 1 条」直接失败。→ **只桥接 `link.created`**。
//!
//! # 未装配平台 sink 时
//!
//! 桥接器**总是**装着（它是 SSO recorder 槽位里装的那个），
//! 但它持有的**平台 sink** 可能是「未装配」状态 → 那时**跳过并 warn**
//! （与 `NoopSsoAuditRecorder` 同一口径：「以为在审计其实没有」比没有更危险）。

use std::sync::Arc;

use platform_core::RequestContext;
use platform_core::extension::audit::{AuditEvent, AuditSink};

use crate::sso::audit::{
    EVENT_LINK_CREATED, EVENT_LOGIN_FAILED, EVENT_LOGIN_SUCCEEDED, SsoAuditEvent, SsoAuditRecorder,
};

/// 把既有的 SSO 审计事件**桥接**到平台 sink 的实现。
///
/// 装进 `IamService::with_sso_audit_recorder`（**既有注入点，签名不变**）。
#[derive(Clone, Default)]
pub struct PlatformSinkSsoRecorder {
    /// 平台 sink（`Arc<dyn AuditSink>`）。
    sink: Option<Arc<dyn AuditSink>>,
}

impl PlatformSinkSsoRecorder {
    /// 用平台 sink 构造。
    pub fn new(sink: Arc<dyn AuditSink>) -> Self {
        Self { sink: Some(sink) }
    }

    /// 该事件码是否**由服务自己写**（→ 这里**不重复**写）。
    fn is_written_by_the_service(code: &str) -> bool {
        matches!(code, EVENT_LOGIN_SUCCEEDED | EVENT_LOGIN_FAILED)
    }
}

impl std::fmt::Debug for PlatformSinkSsoRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformSinkSsoRecorder")
            .field("sink_installed", &self.sink.is_some())
            .finish()
    }
}

impl SsoAuditRecorder for PlatformSinkSsoRecorder {
    fn record(&self, event: &SsoAuditEvent) {
        // 服务自己写的那两个事件码 → **不重复**写（否则一次登录两条记录）。
        if Self::is_written_by_the_service(event.code) {
            return;
        }
        let Some(sink) = &self.sink else {
            tracing::warn!(
                event = event.code,
                "an SSO audit event was produced but no platform audit sink is installed; \
                 the event is NOT persisted"
            );
            return;
        };

        let translated = match translate(event) {
            Some(translated) => translated,
            None => return,
        };
        let ctx = RequestContext::new().with_tenant(translated.tenant_id.clone());
        let sink = sink.clone();
        // `SsoAuditRecorder::record` 是**同步**的（签名不可改）→
        // 这里只能**派生**一个任务去写。
        //
        // ⚠️ 失败**不得**被静默吞掉 → 记 **error** 级日志（含链相关信息）。
        let event_code = event.code;
        tokio::spawn(async move {
            if let Err(error) = sink.record(&ctx, &translated).await {
                tracing::error!(
                    event = event_code,
                    code = error.code(),
                    "the platform audit sink could not persist an SSO audit event"
                );
            }
        });
    }
}

/// 事件码 → `object_type`（与 `sso/audit.rs` 的**声明面**一致）。
fn object_type_of(code: &str) -> &'static str {
    match code {
        EVENT_LOGIN_SUCCEEDED | EVENT_LOGIN_FAILED => "iam_sso_session",
        EVENT_LINK_CREATED => "external_identity",
        _ => "iam_sso_session",
    }
}

/// 把 `SsoAuditEvent` 翻译成通用 `AuditEvent`。
///
/// ⚠️ **载荷白名单**沿用 `SsoAuditEvent::allowed_keys`（序列化层保证）
/// → 翻译出来的 `before` 里**不含** token / 密钥等敏感值。
///
/// # 租户缺失 → `None`（不写）
///
/// 既有的 `SsoAuditRecorder::record` **签名不可改**（D135）→ 没有 `ctx`
/// → 租户只能由**知道租户的服务**填进载荷（`SsoAuditPayload::with_tenant`）。
/// 没填 → **写不了**（审计记录**必须**属于某个租户）→ 跳过并 warn。
fn translate(event: &SsoAuditEvent) -> Option<AuditEvent> {
    let payload = &event.payload;
    let tenant_id = payload.tenant_id.clone()?;
    let mut translated = AuditEvent::new(tenant_id, event.code, object_type_of(event.code));
    if let Some(user_id) = &payload.user_id {
        translated = translated.with_actor(user_id.clone());
    }
    if let Some(external_id) = &payload.external_id {
        translated = translated.with_object_id(external_id.clone());
    }
    translated = translated.with_ip(payload.ip.clone());
    if !payload.user_agent.is_empty() {
        translated = translated.with_user_agent(payload.user_agent.clone());
    }
    // `before` 用**载荷的白名单视图**（`Serialize` 的实现按事件码只发白名单内的键）。
    let before = serde_json::to_value(event).ok()?;
    translated = translated.with_before(before);
    Some(translated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sso::audit::SsoAuditPayload;

    #[test]
    fn the_login_codes_are_left_to_the_service() {
        assert!(PlatformSinkSsoRecorder::is_written_by_the_service(
            EVENT_LOGIN_SUCCEEDED
        ));
        assert!(PlatformSinkSsoRecorder::is_written_by_the_service(
            EVENT_LOGIN_FAILED
        ));
        assert!(!PlatformSinkSsoRecorder::is_written_by_the_service(
            EVENT_LINK_CREATED
        ));
    }

    /// 翻译出来的载荷**不含**敏感值（白名单由 `Serialize` 保证）。
    #[test]
    fn the_translation_only_carries_the_whitelisted_keys() {
        let event = SsoAuditEvent::new(
            EVENT_LINK_CREATED,
            SsoAuditPayload::link_created("ldap", "uid=erin,ou=people", "user-2")
                .with_tenant("tenant-a"),
        );
        let translated = translate(&event).expect("有租户时应当能翻译");
        assert_eq!(translated.action, EVENT_LINK_CREATED);
        assert_eq!(translated.tenant_id, "tenant-a");
        assert_eq!(translated.object_type, "external_identity");
        assert_eq!(translated.actor_id.as_deref(), Some("user-2"));
        assert_eq!(translated.object_id.as_deref(), Some("uid=erin,ou=people"));
        let before = translated.before.expect("应当带上前置载荷");
        let mut keys: Vec<&str> = before
            .as_object()
            .expect("载荷应当是对象")
            .keys()
            .map(String::as_str)
            .collect();
        // ⚠️ 键的**顺序**取决于 `serde_json` 的 `preserve_order` 特性
        // （默认是 BTreeMap → 字典序）→ 断言前先排序（断言的是**键集合**，不是顺序）。
        keys.sort_unstable();
        assert_eq!(keys, ["external_id", "provider", "user_id"]);
        // 白名单**之外**的键一个都没有（`token` / `secret` 等绝不外泄）。
        assert!(
            !before
                .as_object()
                .expect("载荷应当是对象")
                .contains_key("token")
        );
        assert!(
            !before
                .as_object()
                .expect("载荷应当是对象")
                .contains_key("secret")
        );
    }

    /// **没有租户 → 翻译不出来**（审计记录必须属于某个租户）。
    #[test]
    fn a_payload_without_a_tenant_cannot_be_translated() {
        let event = SsoAuditEvent::new(
            EVENT_LINK_CREATED,
            SsoAuditPayload::link_created("ldap", "uid=erin,ou=people", "user-2"),
        );
        assert!(translate(&event).is_none());
    }
}
