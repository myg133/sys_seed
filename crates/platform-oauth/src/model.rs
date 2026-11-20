//! 数据模型：5 张表的行类型 + 领域枚举（REQ-022，design §2）。
//!
//! # 5 张表
//!
//! | 表 | 租户维度 | RLS |
//! |----|----------|-----|
//! | `platform_oauth_client` | **无**（平台级） | **不启用**（D219） |
//! | `platform_oauth_grant` | 有 | 启用 |
//! | `platform_oauth_code` | 有 | 启用 |
//! | `platform_oauth_device_code` | 有 | 启用 |
//! | `platform_oauth_token` | 有 | 启用 |
//!
//! ⚠️ 4 张带 `tenant_id` 的表的 RLS policy **由 `cargo xtask tenant-policies` 生成**
//! （manifest 的 `tenantScoped` 段是唯一来源，D227）→ 迁移里**不手写** policy。

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// `platform_oauth_client` 表名。
pub const CLIENT_TABLE: &str = "platform_oauth_client";
/// `platform_oauth_grant` 表名。
pub const GRANT_TABLE: &str = "platform_oauth_grant";
/// `platform_oauth_code` 表名。
pub const CODE_TABLE: &str = "platform_oauth_code";
/// `platform_oauth_device_code` 表名。
pub const DEVICE_CODE_TABLE: &str = "platform_oauth_device_code";
/// `platform_oauth_token` 表名。
pub const TOKEN_TABLE: &str = "platform_oauth_token";

/// client 的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientType {
    /// 有 secret（可用 `client_secret_basic`）。
    Confidential,
    /// 无 secret（只能用 PKCE）。
    Public,
}

impl ClientType {
    /// 落库 / API 用的名字。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Confidential => "confidential",
            Self::Public => "public",
        }
    }

    /// 从落库 / API 的名字解析。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "confidential" => Some(Self::Confidential),
            "public" => Some(Self::Public),
            _ => None,
        }
    }
}

/// client 的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientStatus {
    /// 可用。
    Active,
    /// 已停用。
    Disabled,
}

impl ClientStatus {
    /// 落库 / API 用的名字。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    /// 从落库 / API 的名字解析。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// 授权记录（grant）的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantStatus {
    /// 有效。
    Active,
    /// 已撤销。
    Revoked,
}

impl GrantStatus {
    /// 落库 / API 用的名字。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }

    /// 从落库 / API 的名字解析。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }
}

/// OAuth 令牌的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenStatus {
    /// 有效。
    Active,
    /// 已撤销。
    Revoked,
}

impl TokenStatus {
    /// 落库 / API 用的名字。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }

    /// 从落库 / API 的名字解析。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }
}

/// 设备码的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceCodeStatus {
    /// 已申请，用户还没批准。
    Pending,
    /// 用户已批准。
    Approved,
    /// 用户已拒绝。
    Denied,
}

impl DeviceCodeStatus {
    /// 落库 / API 用的名字。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
        }
    }

    /// 从落库 / API 的名字解析。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "denied" => Some(Self::Denied),
            _ => None,
        }
    }
}

/// 授权码流程的 grant type（RFC 6749 §4）。
pub mod grant_type {
    /// 授权码流程。
    pub const AUTHORIZATION_CODE: &str = "authorization_code";
    /// 客户端凭据流程。
    pub const CLIENT_CREDENTIALS: &str = "client_credentials";
    /// 设备码流程（RFC 8628）。
    pub const DEVICE_CODE: &str = "urn:ietf:params:oauth:grant-type:device_code";
}

/// 全部支持的 grant type（**顺序钉死**，D250）。
///
/// ⚠️ **不含** `refresh_token`（D217：本轮不发 refresh token）。
pub const SUPPORTED_GRANT_TYPES: [&str; 3] = [
    grant_type::AUTHORIZATION_CODE,
    grant_type::CLIENT_CREDENTIALS,
    grant_type::DEVICE_CODE,
];

/// 唯一的 `response_type`（AC-S-134）。
pub const SUPPORTED_RESPONSE_TYPES: [&str; 1] = ["code"];

/// 唯一的 PKCE 挑战方法（D206）。
pub const SUPPORTED_CODE_CHALLENGE_METHODS: [&str; 1] = ["S256"];

/// OAuth 客户端（一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthClientRow {
    /// client ID（平台生成，全局唯一）。
    pub client_id: String,
    /// secret 的摘要；`None` = public client。
    pub client_secret_hash: Option<String>,
    /// client 类型。
    pub client_type: String,
    /// 展示名（同意页用）。
    pub name: String,
    /// 回调白名单（**精确匹配**）。
    pub redirect_uris: Vec<String>,
    /// 允许的 grant type。
    pub allowed_grant_types: Vec<String>,
    /// 允许的 scope（权限点码）。
    pub allowed_scopes: Vec<String>,
    /// back-channel 登出回调（**可空**）。
    pub backchannel_logout_uri: Option<String>,
    /// front-channel 登出回调（**可空**）。
    pub frontchannel_logout_uri: Option<String>,
    /// 状态。
    pub status: String,
    /// 过期时刻；`None` = 不过期。
    pub expires_at: Option<OffsetDateTime>,
    /// 创建时刻。
    pub created_at: OffsetDateTime,
    /// 创建人。
    pub created_by: Option<String>,
}

platform_db::tenant_row!(OAuthClientRow {
    client_id: "client_id",
    client_secret_hash: "client_secret_hash",
    client_type: "client_type",
    name: "name",
    redirect_uris: "redirect_uris",
    allowed_grant_types: "allowed_grant_types",
    allowed_scopes: "allowed_scopes",
    backchannel_logout_uri: "backchannel_logout_uri",
    frontchannel_logout_uri: "frontchannel_logout_uri",
    status: "status",
    expires_at: "expires_at",
    created_at: "created_at",
    created_by: "created_by",
});

/// 同意记录 / 授权记录（一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthGrantRow {
    /// 租户。
    pub tenant_id: String,
    /// 记录 ID。
    pub id: String,
    /// client ID。
    pub client_id: String,
    /// 用户 ID。
    pub user_id: String,
    /// **实际授予**的 scope。
    pub scopes: Vec<String>,
    /// 状态。
    pub status: String,
    /// 授予时刻。
    pub granted_at: OffsetDateTime,
    /// 用户点击「允许」的时刻。
    pub consent_at: Option<OffsetDateTime>,
    /// 撤销时刻。
    pub revoked_at: Option<OffsetDateTime>,
}

platform_db::tenant_row!(OAuthGrantRow {
    tenant_id: "tenant_id",
    id: "id",
    client_id: "client_id",
    user_id: "user_id",
    scopes: "scopes",
    status: "status",
    granted_at: "granted_at",
    consent_at: "consent_at",
    revoked_at: "revoked_at",
});

/// 授权码（一行）。
///
/// ⚠️ **没有** `consumed_at` / `used_at` 列 —— 删除即消费（D253）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthCodeRow {
    /// 租户。
    pub tenant_id: String,
    /// 记录 ID。
    pub id: String,
    /// client ID。
    pub client_id: String,
    /// 用户 ID。
    pub user_id: String,
    /// 授权码的**摘要**（base64url 无填充）。
    pub code_hash: String,
    /// 签发时的回调地址（交换时必须**逐字相同**）。
    pub redirect_uri: String,
    /// 签发时的 scope（**空格分隔**）。
    pub scope: String,
    /// PKCE 挑战。
    pub code_challenge: String,
    /// PKCE 挑战方法（恒为 `S256`）。
    pub code_challenge_method: String,
    /// 过期时刻（签发 + 60 秒）。
    pub expires_at: OffsetDateTime,
}

platform_db::tenant_row!(OAuthCodeRow {
    tenant_id: "tenant_id",
    id: "id",
    client_id: "client_id",
    user_id: "user_id",
    code_hash: "code_hash",
    redirect_uri: "redirect_uri",
    scope: "scope",
    code_challenge: "code_challenge",
    code_challenge_method: "code_challenge_method",
    expires_at: "expires_at",
});

/// 设备码（一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthDeviceCodeRow {
    /// 租户。
    pub tenant_id: String,
    /// 记录 ID。
    pub id: String,
    /// client ID。
    pub client_id: String,
    /// 设备码的**摘要**。
    pub device_code_hash: String,
    /// 用户短码的**摘要**。
    pub user_code_hash: String,
    /// 请求的 scope（**空格分隔**）。
    pub scope: String,
    /// 状态。
    pub status: String,
    /// 用户 ID（批准后写入）。
    pub user_id: Option<String>,
    /// 设备码过期时刻。
    pub device_code_expires_at: OffsetDateTime,
    /// 用户短码过期时刻。
    pub user_code_expires_at: OffsetDateTime,
    /// 当前轮询间隔（秒）；`slow_down` 时 **+5**。
    pub poll_interval_seconds: i32,
    /// 上次轮询时刻。
    pub last_polled_at: Option<OffsetDateTime>,
}

platform_db::tenant_row!(OAuthDeviceCodeRow {
    tenant_id: "tenant_id",
    id: "id",
    client_id: "client_id",
    device_code_hash: "device_code_hash",
    user_code_hash: "user_code_hash",
    scope: "scope",
    status: "status",
    user_id: "user_id",
    device_code_expires_at: "device_code_expires_at",
    user_code_expires_at: "user_code_expires_at",
    poll_interval_seconds: "poll_interval_seconds",
    last_polled_at: "last_polled_at",
});

/// OAuth 令牌的状态行（一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthTokenRow {
    /// 租户。
    pub tenant_id: String,
    /// 记录 ID。
    pub id: String,
    /// client ID。
    pub client_id: String,
    /// 用户 ID；`client_credentials` 签发的令牌**没有**用户。
    pub user_id: Option<String>,
    /// 令牌的 `jti`（全局唯一）。
    pub token_id: String,
    /// 授予的 scope。
    pub scopes: Vec<String>,
    /// 关联的授权记录；`client_credentials` 的令牌**没有**。
    pub grant_id: Option<String>,
    /// 状态。
    pub status: String,
    /// 签发时刻。
    pub issued_at: OffsetDateTime,
    /// 过期时刻。
    pub expires_at: OffsetDateTime,
    /// 撤销时刻。
    pub revoked_at: Option<OffsetDateTime>,
}

platform_db::tenant_row!(OAuthTokenRow {
    tenant_id: "tenant_id",
    id: "id",
    client_id: "client_id",
    user_id: "user_id",
    token_id: "token_id",
    scopes: "scopes",
    grant_id: "grant_id",
    status: "status",
    issued_at: "issued_at",
    expires_at: "expires_at",
    revoked_at: "revoked_at",
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grant_type_set_is_pinned() {
        assert_eq!(
            SUPPORTED_GRANT_TYPES,
            [
                "authorization_code",
                "client_credentials",
                "urn:ietf:params:oauth:grant-type:device_code",
            ]
        );
        assert!(!SUPPORTED_GRANT_TYPES.contains(&"refresh_token"));
    }

    #[test]
    fn the_response_type_and_challenge_method_sets_are_pinned() {
        assert_eq!(SUPPORTED_RESPONSE_TYPES, ["code"]);
        assert_eq!(SUPPORTED_CODE_CHALLENGE_METHODS, ["S256"]);
    }

    #[test]
    fn the_enum_names_round_trip() {
        for value in [ClientType::Confidential, ClientType::Public] {
            assert_eq!(ClientType::parse(value.as_str()), Some(value));
        }
        for value in [ClientStatus::Active, ClientStatus::Disabled] {
            assert_eq!(ClientStatus::parse(value.as_str()), Some(value));
        }
        for value in [GrantStatus::Active, GrantStatus::Revoked] {
            assert_eq!(GrantStatus::parse(value.as_str()), Some(value));
        }
        for value in [TokenStatus::Active, TokenStatus::Revoked] {
            assert_eq!(TokenStatus::parse(value.as_str()), Some(value));
        }
        for value in [
            DeviceCodeStatus::Pending,
            DeviceCodeStatus::Approved,
            DeviceCodeStatus::Denied,
        ] {
            assert_eq!(DeviceCodeStatus::parse(value.as_str()), Some(value));
        }
        assert_eq!(ClientType::parse("nope"), None);
        assert_eq!(ClientStatus::parse("nope"), None);
        assert_eq!(GrantStatus::parse("nope"), None);
        assert_eq!(TokenStatus::parse("nope"), None);
        assert_eq!(DeviceCodeStatus::parse("nope"), None);
    }

    /// 平台级表**不带** `tenant_id`（D219）→ 它的行类型里也**不得**有该字段。
    #[test]
    fn the_client_row_has_no_tenant_column() {
        // 编译期保证：行类型里没有 `tenant_id` 字段（见结构体定义）。
        let row = OAuthClientRow {
            client_id: "client-a".to_string(),
            client_secret_hash: None,
            client_type: "public".to_string(),
            name: "demo".to_string(),
            redirect_uris: vec![],
            allowed_grant_types: vec![],
            allowed_scopes: vec![],
            backchannel_logout_uri: None,
            frontchannel_logout_uri: None,
            status: "active".to_string(),
            expires_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            created_by: None,
        };
        assert_eq!(row.client_id, "client-a");
    }
}
