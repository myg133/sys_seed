//! SSO 审计事件：**声明 + 发出**（REQ-013 AC-S-108）。
//!
//! # 为什么在 `platform-iam` 里自建（P14）
//!
//! - `platform_core` **没有** `AuditRecorder` trait —— 只有 [`Auditable`] +
//!   [`AuditEventSpec`]，而 `AuditEventSpec` 的字段是
//!   `code/action/object_type/description`，**没有 payload**；
//! - `platform-org` 的 `RecordingAuditRecorder` 绑死 `OrgAuditEvent`（只有 `detail`，无 payload），
//!   且 `platform-iam → platform-org` 会成环（`platform-org` 已经依赖 `platform-iam`）。
//!
//! 所以本模块自建 [`SsoAuditEvent`] + [`SsoAuditRecorder`]，
//! 同时**仍然**实现 [`Auditable`] —— 那是平台契约要求的**声明面**，
//! 与 payload 接缝是两件不同的事。
//!
//! # 本轮**不落库**
//!
//! 审计 sink（落库 + 查询）在 REQ-014。本轮只声明 + 发出。
//! `NoopSsoAuditRecorder` 是默认实现，它**每次都记一条 warn** ——
//! 「以为在审计其实没有」比「没有审计」更危险（D27：告警 = 静默放行的一种）。
//!
//! # payload 的键集合是**白名单**，由类型保证
//!
//! AC-S-108 要求：
//!
//! | 事件 | 键集合**恰好** |
//! |------|----------------|
//! | `iam.sso.login.succeeded` | `{provider, external_id, user_id, ip, user_agent}` |
//! | `iam.sso.login.failed` | `{provider, code, ip}` |
//!
//! `Serialize` 是**手写**的：按事件码决定发哪些键。多一个键就是失败 ——
//! 靠「记得别塞」是靠不住的，所以让类型/序列化层做到。

use std::sync::OnceLock;

use platform_core::extension::auditable::{AuditEventSpec, Auditable};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

/// 登录成功。
pub const EVENT_LOGIN_SUCCEEDED: &str = "iam.sso.login.succeeded";
/// 登录失败。
pub const EVENT_LOGIN_FAILED: &str = "iam.sso.login.failed";
/// 首次建立外部身份绑定（JIT 建号后的绑定）。
pub const EVENT_LINK_CREATED: &str = "iam.sso.link.created";

/// 一个审计事件的**载荷**。
///
/// 字段都在这里（便于构造），但**序列化时按事件码只发白名单内的键**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoAuditPayload {
    /// 协议（`oidc` / `ldap`）。
    pub provider: String,
    /// 外部身份标识（OIDC 的 `sub` / LDAP 的 DN）。
    pub external_id: Option<String>,
    /// 本地账号 ID。
    pub user_id: Option<String>,
    /// 客户端 IP。
    pub ip: String,
    /// 客户端 UA。
    pub user_agent: String,
    /// 失败时的错误码（成功时为 `None`）。
    pub code: Option<String>,
}

impl SsoAuditPayload {
    /// 一次成功登录的载荷。
    pub fn succeeded(
        provider: impl Into<String>,
        external_id: impl Into<String>,
        user_id: impl Into<String>,
        ip: impl Into<String>,
        user_agent: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            external_id: Some(external_id.into()),
            user_id: Some(user_id.into()),
            ip: ip.into(),
            user_agent: user_agent.into(),
            code: None,
        }
    }

    /// 一次失败登录的载荷（**只带 provider / code / ip**）。
    pub fn failed(
        provider: impl Into<String>,
        code: impl Into<String>,
        ip: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            external_id: None,
            user_id: None,
            ip: ip.into(),
            user_agent: String::new(),
            code: Some(code.into()),
        }
    }

    /// 一次新建绑定的载荷。
    pub fn link_created(
        provider: impl Into<String>,
        external_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            external_id: Some(external_id.into()),
            user_id: Some(user_id.into()),
            ip: String::new(),
            user_agent: String::new(),
            code: None,
        }
    }
}

/// 一个 SSO 审计事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoAuditEvent {
    /// 事件码（[`EVENT_LOGIN_SUCCEEDED`] 等）。
    pub code: &'static str,
    /// 载荷。
    pub payload: SsoAuditPayload,
}

impl SsoAuditEvent {
    /// 构造。
    pub fn new(code: &'static str, payload: SsoAuditPayload) -> Self {
        Self { code, payload }
    }

    /// 该事件**允许**发出的键（白名单，顺序固定）。
    pub fn allowed_keys(&self) -> &'static [&'static str] {
        match self.code {
            EVENT_LOGIN_SUCCEEDED => &["provider", "external_id", "user_id", "ip", "user_agent"],
            EVENT_LOGIN_FAILED => &["provider", "code", "ip"],
            EVENT_LINK_CREATED => &["provider", "external_id", "user_id"],
            _ => &[],
        }
    }
}

impl Serialize for SsoAuditEvent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let payload = &self.payload;
        let mut map = serializer.serialize_map(Some(self.allowed_keys().len()))?;
        for key in self.allowed_keys() {
            match *key {
                "provider" => map.serialize_entry("provider", &payload.provider)?,
                "external_id" => {
                    if let Some(value) = &payload.external_id {
                        map.serialize_entry("external_id", value)?;
                    }
                }
                "user_id" => {
                    if let Some(value) = &payload.user_id {
                        map.serialize_entry("user_id", value)?;
                    }
                }
                "ip" => map.serialize_entry("ip", &payload.ip)?,
                "user_agent" => map.serialize_entry("user_agent", &payload.user_agent)?,
                "code" => {
                    if let Some(value) = &payload.code {
                        map.serialize_entry("code", value)?;
                    }
                }
                _ => {}
            }
        }
        map.end()
    }
}

/// 审计事件的落点（**由装配层注入**）。
///
/// 本轮没有平台 sink（REQ-014 才做）。默认实现是 [`NoopSsoAuditRecorder`]。
pub trait SsoAuditRecorder: Send + Sync {
    /// 记一条事件。
    ///
    /// 实现**必须**用 [`SsoAuditEvent`] 的 `Serialize`（或 [`SsoAuditEvent::allowed_keys`]）
    /// 输出载荷 —— 那层白名单是「不含敏感值」的结构性保证。
    fn record(&self, event: &SsoAuditEvent);
}

/// 默认实现：**不落任何地方，但每次都 warn**。
///
/// 「静默 noop」会让运维以为审计在工作 —— 那比没有审计更危险。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopSsoAuditRecorder;

impl SsoAuditRecorder for NoopSsoAuditRecorder {
    fn record(&self, event: &SsoAuditEvent) {
        tracing::warn!(
            event = event.code,
            provider = %event.payload.provider,
            "SSO audit event was produced but no audit sink is installed (REQ-014 provides it); the event is NOT persisted"
        );
    }
}

/// 声明的事件清单（`Auditable` 的返回面）。
fn declared_events() -> &'static [AuditEventSpec] {
    static EVENTS: OnceLock<Vec<AuditEventSpec>> = OnceLock::new();
    EVENTS.get_or_init(|| {
        vec![
            AuditEventSpec::new(EVENT_LOGIN_SUCCEEDED, "login", "iam_sso_session")
                .with_description("a user signed in through a single sign-on provider"),
            AuditEventSpec::new(EVENT_LOGIN_FAILED, "login_failed", "iam_sso_session")
                .with_description("a single sign-on attempt failed"),
            AuditEventSpec::new(EVENT_LINK_CREATED, "link", "external_identity")
                .with_description("an external identity was linked to a local account"),
        ]
    })
}

/// 声明面：`platform-iam` 的全部审计事件。
pub fn audit_events() -> &'static [AuditEventSpec] {
    declared_events()
}

/// 让 [`Auditable`] 有一个可复用的空实现骨架（模块没有自身状态时用）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SsoAuditDeclaration;

impl Auditable for SsoAuditDeclaration {
    fn audit_events(&self) -> &[AuditEventSpec] {
        declared_events()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn declared_events_cover_the_three_required_codes() {
        let codes: Vec<&str> = audit_events()
            .iter()
            .map(|spec| spec.code.as_str())
            .collect();
        for required in [
            EVENT_LOGIN_SUCCEEDED,
            EVENT_LOGIN_FAILED,
            EVENT_LINK_CREATED,
        ] {
            assert!(codes.contains(&required), "缺少声明的审计事件 {required}");
        }
        for spec in audit_events() {
            assert!(!spec.code.is_empty());
            assert!(!spec.action.is_empty(), "{} 缺 action", spec.code);
            assert!(!spec.object_type.is_empty(), "{} 缺 objectType", spec.code);
        }
    }

    /// 成功的 payload 键集合**恰好**是 `{provider, external_id, user_id, ip, user_agent}`。
    #[test]
    fn succeeded_payload_has_the_exact_whitelist() {
        let event = SsoAuditEvent::new(
            EVENT_LOGIN_SUCCEEDED,
            SsoAuditPayload::succeeded("oidc", "idp-bob-001", "user-1", "10.0.0.1", "agent"),
        );
        let value = serde_json::to_value(&event).expect("应能序列化");
        let keys: BTreeSet<&str> = value
            .as_object()
            .expect("应是对象")
            .keys()
            .map(String::as_str)
            .collect();
        let expected: BTreeSet<&str> = ["provider", "external_id", "user_id", "ip", "user_agent"]
            .into_iter()
            .collect();
        assert_eq!(keys, expected);
    }

    /// 失败的 payload 键集合**恰好**是 `{provider, code, ip}`（**不含** token / user_agent）。
    #[test]
    fn failed_payload_has_the_exact_whitelist() {
        let mut payload = SsoAuditPayload::failed("oidc", "SSO_OIDC_ALG_NOT_ALLOWED", "10.0.0.1");
        payload.user_agent = "should-not-be-emitted".to_string();
        let event = SsoAuditEvent::new(EVENT_LOGIN_FAILED, payload);
        let value = serde_json::to_value(&event).expect("应能序列化");
        let keys: BTreeSet<&str> = value
            .as_object()
            .expect("应是对象")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            ["provider", "code", "ip"]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        assert!(!value.to_string().contains("should-not-be-emitted"));
    }

    #[test]
    fn link_created_payload_has_no_ip_field() {
        let event = SsoAuditEvent::new(
            EVENT_LINK_CREATED,
            SsoAuditPayload::link_created("ldap", "uid=erin,ou=people", "user-2"),
        );
        let value = serde_json::to_value(&event).expect("应能序列化");
        assert!(value.get("ip").is_none());
        assert_eq!(value["external_id"], "uid=erin,ou=people");
    }
}
