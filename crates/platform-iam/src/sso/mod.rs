//! SSO **入站**（OIDC + LDAP）与强制 SSO（REQ-013）。
//!
//! # 范围
//!
//! | 做 | 不做（本轮） |
//! |----|-------------|
//! | OIDC 授权码 + PKCE 入站 | SAML / CAS（树里没有 XML 库，XMLDSig 手写验签是 XSW 重灾区） |
//! | LDAP bind 认证 | 出站 IdP / 单点登出（REQ-022） |
//! | 强制 SSO 策略 | 审计**落库**（REQ-014 才提供 sink） |
//! | 账号绑定 + JIT 建号 | 多 IdP 的管理界面 |
//!
//! # 三个必须存在的接缝（rev 3）
//!
//! 1. [`ldap::SsoLdapClient`] / [`ldap::SsoLdapConn`] —— `ldap3` **没有**服务端/内存实现，
//!    `platform-org` 的 `InMemoryLdapSource` 只实现 `search`、**不能测 bind**。
//!    没有这层 trait，「空密码必须 0 次 bind」这条断言**没有观测面**。
//! 2. [`audit::SsoAuditEvent`] / [`audit::SsoAuditRecorder`] —— `platform_core` 只有
//!    `Auditable` + `AuditEventSpec`（**没有 payload 字段**）；`platform-org` 的 recorder
//!    绑死 `OrgAuditEvent`，且 `iam → org` 会成环。所以本 crate 自建。
//! 3. [`AuthProvider`] + `SessionView.authProvider` —— 会话要能看出 SSO 来源。
//!
//! # 依赖方向
//!
//! `platform-iam` **不依赖** `platform-config` / `platform-org` / `platform-rbac`。
//! SSO 配置通过 [`config::SsoConfigSource`] 后设槽注入（由装配层实现），
//! 审计通过 [`audit::SsoAuditRecorder`] 后设槽注入，LDAP 客户端通过
//! [`ldap::SsoLdapClient`] 后设槽注入。

pub mod audit;
pub mod binding;
pub mod bridge;
pub mod config;
pub mod ldap;
pub mod oidc;
pub mod ticket;

use serde::{Deserialize, Serialize};

/// 会话 / 登录的**来源**。
///
/// 落在 `platform_iam_session.auth_provider` 列上（`TEXT NOT NULL DEFAULT 'local'`）。
/// 这是两条会话**唯一允许取值不同**的字段（AC-S-107）——
/// 其余字段只是值不同，键集合必须完全一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthProvider {
    /// 本地密码登录。
    Local,
    /// OIDC 单点登录。
    Oidc,
    /// LDAP bind 登录。
    Ldap,
}

impl AuthProvider {
    /// 落库 / 响应里的取值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Oidc => "oidc",
            Self::Ldap => "ldap",
        }
    }
}

impl std::fmt::Display for AuthProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 登录页可见的 SSO 入口。
///
/// # 只含「怎么发起」，**不含任何配置值**（AC-S-109 步骤 8 / S41）
///
/// 允许出现：`type` / `id` / `label` / 端点路径。
/// **不得**出现：`issuer` / `clientId` / `clientSecret` / LDAP `url` / `baseDn`。
///
/// `label` 用 ASCII 英文占位：面向用户的中文文案由前端 i18n 按 `type` 渲染，
/// 后端不持有中文文案（与错误 `title` 同一条纪律）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoProviderView {
    /// 协议类型（`oidc` / `ldap`）。
    #[serde(rename = "type")]
    pub kind: String,
    /// provider ID（本轮与 `type` 相同；将来多 IdP 时区分）。
    pub id: String,
    /// 展示用标签（ASCII）。
    pub label: String,
    /// OIDC 的发起端点（LDAP 时为 `None`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorize_url: Option<String>,
    /// LDAP 的登录端点（OIDC 时为 `None`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_url: Option<String>,
}

/// OIDC 的发起端点路径。
pub const OIDC_AUTHORIZE_PATH: &str = "/api/iam/sso/oidc/authorize";
/// OIDC 的回调端点路径。
pub const OIDC_CALLBACK_PATH: &str = "/api/iam/sso/oidc/callback";
/// LDAP 的登录端点路径。
pub const LDAP_LOGIN_PATH: &str = "/api/iam/sso/ldap/login";
/// provider 列表端点路径。
pub const PROVIDERS_PATH: &str = "/api/iam/sso/providers";
/// 票据交换端点路径。
pub const EXCHANGE_PATH: &str = "/api/iam/sso/exchange";

impl SsoProviderView {
    /// OIDC 入口。
    pub fn oidc() -> Self {
        Self {
            kind: "oidc".to_string(),
            id: "oidc".to_string(),
            label: "Enterprise single sign-on".to_string(),
            authorize_url: Some(OIDC_AUTHORIZE_PATH.to_string()),
            login_url: None,
        }
    }

    /// LDAP 入口。
    pub fn ldap() -> Self {
        Self {
            kind: "ldap".to_string(),
            id: "ldap".to_string(),
            label: "LDAP / AD".to_string(),
            authorize_url: None,
            login_url: Some(LDAP_LOGIN_PATH.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_provider_names_are_the_contract_three() {
        assert_eq!(AuthProvider::Local.as_str(), "local");
        assert_eq!(AuthProvider::Oidc.as_str(), "oidc");
        assert_eq!(AuthProvider::Ldap.as_str(), "ldap");
    }

    /// provider 视图**不得**携带任何配置值（AC-S-109 步骤 8）。
    #[test]
    fn provider_views_carry_no_configuration_values() {
        for view in [SsoProviderView::oidc(), SsoProviderView::ldap()] {
            let json = serde_json::to_string(&view).expect("应能序列化");
            for forbidden in ["issuer", "clientId", "clientSecret", "baseDn", "ldapUrl"] {
                assert!(
                    !json.contains(forbidden),
                    "provider 视图不得出现配置值字段 {forbidden}：{json}"
                );
            }
            let value: serde_json::Value = serde_json::from_str(&json).expect("应能解析");
            let keys: Vec<&String> = value.as_object().expect("应是对象").keys().collect();
            let allowed = ["type", "id", "label", "authorizeUrl", "loginUrl"];
            for key in keys {
                assert!(
                    allowed.contains(&key.as_str()),
                    "provider 视图出现未允许的键 {key}：{json}"
                );
            }
        }
    }
}
