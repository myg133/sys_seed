//! 业务编排（REQ-022，design §4）。
//!
//! # 两套错误标识的**分界**
//!
//! | 端点族 | 出错时返回 |
//! |--------|-----------|
//! | 平台端点（`authorize/*` / `clients/*` / `grants/*` / `logout` / `device/*`） | [`OAuthError`]（Problem Details） |
//! | 协议端点（`token` / `introspect` / `revoke` / `device_authorization`） | [`ProtocolError`]（`{"error": "…"}`） |
//!
//! # scope 的**三重交集**（D201 / AC-S-140）
//!
//! ```text
//! 请求 scope ∩ client.allowed_scopes ∩ 用户有效权限点
//! ```
//!
//! - **全有或全无**：请求的**每一个** scope 都必须在后两者里 → 否则
//!   **400 `invalid_scope`**（AC-S-140 路径 2 / 3）。
//! - **通配一律不生效**：请求的 scope 里**含 `*` 的 token** → **400 `invalid_scope`**，
//!   **即使**它被登记进了 `platform_permission`（D241 的第三条判据）。
//!
//! # 授权码的**一次性**（D205 / AC-S-139）
//!
//! 交换顺序（**顺序错了就是漏洞**，design §4.2）：
//!
//! 1. client 认证 → 2. grant type → 3. 查授权码 → 4. 过期 → 5. 绑定项逐字相同
//! → 6. PKCE → **7. 删除授权码** → 8. scope → 9. 写 grant → 10. 签发令牌 → 11. 审计。
//!
//! ⚠️ 第 7 步（删除）**必须**在第 10 步（签发）**之前** ——
//! 否则「签发成功但码没删」= 可重放。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use platform_core::RequestContext;
use platform_core::extension::audit::{AuditEvent, AuditSink, format_timestamp_micros_utc};
use platform_core::extension::permission::PermissionProvider;
use platform_iam::service::IamService;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::client_auth;
use crate::config::{OAuthConfigSource, OAuthSettings};
use crate::crypto::{digest_base64url, new_secret, new_user_code};
use crate::error::{OAuthError, ServiceError};
use crate::events::{code as event_code, object_type};
use crate::keys::PlatformKeys;
use crate::model::{
    ClientStatus, ClientType, DeviceCodeStatus, GrantStatus, SUPPORTED_CODE_CHALLENGE_METHODS,
    SUPPORTED_GRANT_TYPES, SUPPORTED_RESPONSE_TYPES, TokenStatus, grant_type,
};
use crate::outbound::OutboundClient;
use crate::pkce::{self, PkceError};
use crate::protocol::ProtocolError;
use crate::store::{NewClient, NewCode, NewDeviceCode, NewToken, OAuthStore};
use crate::ticket;

/// 当前时刻（UTC）。
fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// 当前时刻（秒）。
pub fn unix_now() -> i64 {
    now().unix_timestamp()
}

/// 已认证的用户（平台会话）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedUser {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**）。
    pub tenant_id: String,
    /// 会话 ID（`sid`）—— 登出时进 `logout_token`（D230）。
    pub session_id: String,
}

/// OAuth 客户端的**视图**（响应体；**不含** secret 与摘要）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientView {
    /// client ID。
    pub client_id: String,
    /// client 类型（`confidential` / `public`）。
    pub client_type: String,
    /// 展示名。
    pub name: String,
    /// 回调白名单。
    pub redirect_uris: Vec<String>,
    /// 允许的 grant type。
    pub allowed_grant_types: Vec<String>,
    /// 允许的 scope。
    pub allowed_scopes: Vec<String>,
    /// back-channel 登出回调。
    pub backchannel_logout_uri: Option<String>,
    /// front-channel 登出回调。
    pub frontchannel_logout_uri: Option<String>,
    /// 状态。
    pub status: String,
    /// 过期时刻（RFC 3339，UTC，微秒 6 位）。
    pub expires_at: Option<String>,
    /// 创建时刻（RFC 3339，UTC，微秒 6 位）。
    pub created_at: String,
}

/// 注册 client 的**一次性**响应（含明文 secret）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedClient {
    /// client ID。
    pub client_id: String,
    /// 明文 secret —— **只在此处出现一次**（D200）。
    ///
    /// ⚠️ `public` client **没有** secret → 该键**不存在**（不是 `null`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// client 类型。
    pub client_type: String,
    /// 展示名。
    pub name: String,
    /// 回调白名单。
    pub redirect_uris: Vec<String>,
    /// 允许的 grant type。
    pub allowed_grant_types: Vec<String>,
    /// 允许的 scope。
    pub allowed_scopes: Vec<String>,
    /// back-channel 登出回调。
    pub backchannel_logout_uri: Option<String>,
    /// front-channel 登出回调。
    pub frontchannel_logout_uri: Option<String>,
    /// 状态。
    pub status: String,
    /// 过期时刻。
    pub expires_at: Option<String>,
    /// 创建时刻。
    pub created_at: String,
}

/// 同意页上的 **client**（展示用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationView {
    /// client ID。
    pub client_id: String,
    /// 展示名。
    pub name: String,
}

/// `preview` 的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    /// 是否需要用户同意。
    pub needs_consent: bool,
    /// 同意票据（**仅** `needsConsent == true` 时有）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// client 的展示信息。
    pub application: ApplicationView,
    /// **实际授予**的 scope（数组）—— 已与 `client.allowed_scopes` 和
    /// **用户自身的有效权限点**取过交集（AC-S-146「用户所见 = 实际授予」）。
    ///
    /// ⚠️ 与**票据**里装的 scope **不同**：票据装的是**请求的** scope，
    /// 令牌端点据此判 `invalid_scope`（AC-S-140 的全有或全无）。
    /// 这里展示交集 → 用户不会看到「拿不到的权限点」。
    pub scopes: Vec<String>,
    /// 免登直发的跳转地址（**仅** `needsConsent == false` 时有）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_to: Option<String>,
}

/// `confirm` / `deny` 的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedirectResult {
    /// 跳转地址（带 `code` 或 `error` 与 `state`）。
    pub redirect_to: String,
}

/// 「我的授权」的一条记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantView {
    /// client ID。
    pub client_id: String,
    /// client 的展示名（查不到时为 `None`）。
    pub client_name: Option<String>,
    /// 授予的 scope。
    pub scopes: Vec<String>,
    /// 授予时刻。
    pub granted_at: String,
    /// 状态（`active` / `revoked`）。
    pub status: String,
    /// 撤销时刻（未撤销时为 `null`）。
    pub revoked_at: Option<String>,
}

/// 撤销授权的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeGrantResult {
    /// 撤销后的状态（恒为 `revoked`）。
    pub status: String,
}

/// 登出的统计。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggedOutCounts {
    /// 被撤销的授权记录数。
    pub grants: u64,
    /// 被作废的会话（refresh token）数。
    pub sessions: u64,
}

/// 一条 front-channel 登出通知。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontChannelEntry {
    /// client ID。
    pub client_id: String,
    /// front-channel 回调地址（**逐字**取自 client 表）。
    pub logout_url: String,
    /// `logout_token`（JWT）。
    pub logout_token: String,
}

/// 登出的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogoutResult {
    /// 撤销统计。
    pub logged_out: LoggedOutCounts,
    /// front-channel 通知（**永远是数组**，没有就是 `[]`，绝不是 `null`）。
    pub front_channel: Vec<FrontChannelEntry>,
}

/// 设备码页的展示信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    /// client ID。
    pub client_id: String,
    /// client 的展示名。
    pub client_name: String,
    /// 请求的 scope。
    pub scopes: Vec<String>,
}

/// 设备码批准 / 拒绝的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceApprovalResult {
    /// `approved` / `denied`。
    pub status: String,
}

/// 令牌响应（**协议端点**：RFC 的 snake_case）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenResponse {
    /// 访问令牌（JWT）。
    pub access_token: String,
    /// 恒为 `Bearer`。
    pub token_type: String,
    /// 有效期（秒）。
    pub expires_in: i64,
    /// 授予的 scope（**空格分隔**）。
    pub scope: String,
}

/// 设备码申请响应（**协议端点**：RFC 的 snake_case）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceAuthorizationResponse {
    /// 设备码（**只以摘要落库**）。
    pub device_code: String,
    /// 用户短码（8 个 base20 字符）。
    pub user_code: String,
    /// 验证页地址（`{授权页基址}/oauth/device`）。
    pub verification_uri: String,
    /// 带短码的验证页地址。
    pub verification_uri_complete: String,
    /// 有效期（秒）。
    pub expires_in: i64,
    /// 轮询间隔（秒）。
    pub interval: i64,
}

/// 内省响应（RFC 7662）。
///
/// ⚠️ `active == false` 时**只有** `active` 一个键（D208 / AC-S-143）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntrospectionResponse {
    /// 令牌是否有效。
    pub active: bool,
    /// 授予的 scope。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// client ID。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// 用户名（**无用户主体的令牌没有该键** —— RFC 7662）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// 恒为 `Bearer`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    /// 过期时刻（秒）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    /// 签发时刻（秒）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    /// 用户 ID（无用户主体时**没有该键**）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    /// 签发者。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    /// 受众（= client ID）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
}

/// 授权服务器元数据（RFC 8414）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorizationServerMetadata {
    /// 签发者。
    pub issuer: String,
    /// 授权端点（**前端路由**，`{授权页基址}/oauth/authorize`）。
    pub authorization_endpoint: String,
    /// 令牌端点。
    pub token_endpoint: String,
    /// 设备码端点。
    pub device_authorization_endpoint: String,
    /// 内省端点。
    pub introspection_endpoint: String,
    /// 撤销端点。
    pub revocation_endpoint: String,
    /// JWKS 地址。
    pub jwks_uri: String,
    /// 支持的 grant type（**顺序钉死**，不含 `refresh_token`）。
    pub grant_types_supported: Vec<String>,
    /// 支持的 response type。
    pub response_types_supported: Vec<String>,
    /// 支持的 PKCE 挑战方法（**只有** `S256`）。
    pub code_challenge_methods_supported: Vec<String>,
    /// 令牌端点的认证方式。
    pub token_endpoint_auth_methods_supported: Vec<String>,
    /// 内省端点的认证方式。
    pub introspection_endpoint_auth_methods_supported: Vec<String>,
    /// 撤销端点的认证方式。
    pub revocation_endpoint_auth_methods_supported: Vec<String>,
    /// access token 的有效期（秒）—— 撤销延迟的上界（D210）。
    pub access_token_expires_in: i64,
}

/// 注册 client 的请求。
#[derive(Debug, Clone, Default)]
pub struct RegisterClientRequest {
    /// 展示名。
    pub name: String,
    /// client 类型。
    pub client_type: String,
    /// 回调白名单。
    pub redirect_uris: Vec<String>,
    /// 允许的 grant type。
    pub allowed_grant_types: Vec<String>,
    /// 允许的 scope。
    pub allowed_scopes: Vec<String>,
    /// back-channel 登出回调。
    pub backchannel_logout_uri: Option<String>,
    /// front-channel 登出回调。
    pub frontchannel_logout_uri: Option<String>,
    /// 有效期（秒）。
    pub expires_in_seconds: Option<i64>,
}

/// `preview` 的请求。
#[derive(Debug, Clone, Default)]
pub struct PreviewRequest {
    /// `response_type`（**必填**）。
    pub response_type: Option<String>,
    /// client ID（**必填**）。
    pub client_id: Option<String>,
    /// 回调地址。
    pub redirect_uri: Option<String>,
    /// 请求的 scope（**空格分隔**）。
    pub scope: Option<String>,
    /// `state`。
    pub state: Option<String>,
    /// PKCE 挑战（**必填**）。
    pub code_challenge: Option<String>,
    /// PKCE 挑战方法（**必填**，恒为 `S256`）。
    pub code_challenge_method: Option<String>,
    /// `prompt`（`none` = 不许交互）。
    pub prompt: Option<String>,
}

/// OAuth access token 的声明（ES256 JWT）。
///
/// ⚠️ `sub` 在 `client_credentials` 时**省略该键**（不是 `null`、不是空串）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessTokenClaims {
    /// 签发者。
    pub iss: String,
    /// 受众（= client ID）。
    pub aud: String,
    /// 用户 ID（**可缺省**）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    /// 租户 ID。
    pub tid: String,
    /// client ID。
    pub client_id: String,
    /// 授予的 scope（**空格分隔**）。
    pub scope: String,
    /// grant type。
    pub grant_type: String,
    /// 签发时刻（秒）。
    pub iat: i64,
    /// 生效时刻（秒）。
    pub nbf: i64,
    /// 过期时刻（秒）。
    pub exp: i64,
    /// 令牌 ID。
    pub jti: String,
}

/// `logout_token` 的声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogoutTokenClaims {
    /// 签发者。
    pub iss: String,
    /// 受众（= client ID）。
    pub aud: String,
    /// 用户 ID。
    pub sub: String,
    /// 租户 ID。
    pub tid: String,
    /// 触发登出的会话 ID（D230）。
    pub sid: String,
    /// 事件（back-channel logout）。
    pub events: BTreeMap<String, serde_json::Value>,
    /// 签发时刻（秒）。
    pub iat: i64,
    /// 令牌 ID。
    pub jti: String,
}

/// 出站 IdP 的业务服务。
#[derive(Clone)]
pub struct OAuthService {
    /// 数据访问。
    store: Arc<OAuthStore>,
    /// 认证与账号体系（验登录令牌 / 签发密钥 / 会话作废）。
    iam: Arc<IamService>,
    /// 平台公钥视图（验 OAuth 令牌 / `logout_token`）。
    platform_keys: Arc<PlatformKeys>,
    /// 配置源。
    config: Arc<dyn OAuthConfigSource>,
    /// 权限点提供者（「用户有效权限点」的**唯一**来源）。
    permissions: Arc<dyn PermissionProvider>,
    /// 审计 sink。
    audit: Arc<dyn AuditSink>,
    /// 出站 HTTP client（back-channel 登出）。
    outbound: Arc<OutboundClient>,
}

impl std::fmt::Debug for OAuthService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthService").finish_non_exhaustive()
    }
}

impl OAuthService {
    /// 构造服务。
    pub fn new(
        store: Arc<OAuthStore>,
        iam: Arc<IamService>,
        config: Arc<dyn OAuthConfigSource>,
        permissions: Arc<dyn PermissionProvider>,
        audit: Arc<dyn AuditSink>,
        outbound: Arc<OutboundClient>,
    ) -> Self {
        let platform_keys = Arc::new(PlatformKeys::from_key_set(iam.keys()));
        Self {
            store,
            iam,
            platform_keys,
            config,
            permissions,
            audit,
            outbound,
        }
    }

    /// 平台 issuer。
    pub fn issuer(&self) -> &str {
        &self.iam.config().jwt.issuer
    }

    /// 当前的运行期配置。
    pub async fn settings(&self, ctx: &RequestContext) -> Result<OAuthSettings, ServiceError> {
        self.config
            .read(ctx)
            .await
            .map_err(|_| ServiceError::Internal {
                reason: "the oauth configuration could not be read".to_string(),
            })
    }

    /// 授权页基址（**未配置时回退 issuer**，D222）。
    pub async fn frontend_base(&self, ctx: &RequestContext) -> Result<String, ServiceError> {
        let settings = self.settings(ctx).await?;
        Ok(settings.frontend_base(self.issuer()))
    }

    /// 验登录 access token → 已认证用户。
    ///
    /// ⚠️ OAuth 令牌**过不了**这一关（`aud` 是 client_id、且没有 `sid`）→
    /// 底座业务端点**默认拒绝**第三方 OAuth 令牌（D209 / AC-S-151）。
    pub fn authenticate_bearer(&self, token: &str) -> Result<AuthenticatedUser, ServiceError> {
        let claims = self
            .iam
            .verify_access_token(token)
            .map_err(|_| ServiceError::Unauthenticated)?;
        Ok(AuthenticatedUser {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
            session_id: claims.session_id,
        })
    }

    /// 验 client 凭据 → 已认证的 client（**协议端点**用）。
    ///
    /// # 失败语义
    ///
    /// **一律** `invalid_client`（**不区分**「client 不存在」与「secret 错」，D207）→
    /// 否则就成了 client 枚举接口。
    ///
    /// # `require_secret`
    ///
    /// - `true`（内省 / 撤销）：**必须**是带 secret 的 confidential client（D208）→
    ///   `public` client **无权**调用；
    /// - `false`（令牌 / 设备码申请）：`public` client 可以只带 `client_id`，
    ///   但 `confidential` client **必须**带 secret（缺 secret → 失败）。
    pub async fn authenticate_client(
        &self,
        credentials: &client_auth::ClientCredentials,
        require_secret: bool,
    ) -> Result<ClientIdentity, ProtocolError> {
        let row = self
            .store
            .get_client(&credentials.client_id)
            .await
            .map_err(|_| ProtocolError::ServerError)?
            .ok_or(ProtocolError::InvalidClient)?;
        // 停用 / 过期的 client → **认证失败**（D249：令牌端点回 `invalid_client`）。
        if row.status == ClientStatus::Disabled.as_str() {
            return Err(ProtocolError::InvalidClient);
        }
        if let Some(expires_at) = row.expires_at {
            if expires_at <= now() {
                return Err(ProtocolError::InvalidClient);
            }
        }
        match credentials.client_secret.as_deref() {
            Some(secret) => {
                if !client_auth::verify_client_secret(row.client_secret_hash.as_deref(), secret) {
                    return Err(ProtocolError::InvalidClient);
                }
            }
            None => {
                // 没带 secret → 只允许 `public` client（库里**没有**摘要）。
                if row.client_secret_hash.is_some() {
                    return Err(ProtocolError::InvalidClient);
                }
                if require_secret {
                    // `public` client **无权**内省 / 撤销（D208）。
                    return Err(ProtocolError::InvalidClient);
                }
            }
        }
        Ok(ClientIdentity {
            client_id: row.client_id,
            client_type: row.client_type,
            redirect_uris: row.redirect_uris,
            allowed_scopes: row.allowed_scopes,
            allowed_grant_types: row.allowed_grant_types,
            name: row.name,
        })
    }

    // -----------------------------------------------------------------------
    // client 管理
    // -----------------------------------------------------------------------

    /// 注册 client（**secret 只显示一次**，D200）。
    pub async fn register_client(
        &self,
        ctx: &RequestContext,
        request: &RegisterClientRequest,
    ) -> Result<CreatedClient, ServiceError> {
        let name = request.name.trim();
        if name.is_empty() {
            return Err(OAuthError::InvalidRequest {
                reason: "the client name is empty",
            }
            .into());
        }
        let client_type = match request.client_type.trim() {
            "" | "confidential" => ClientType::Confidential,
            "public" => ClientType::Public,
            _ => {
                return Err(OAuthError::InvalidRequest {
                    reason: "the client type is not supported",
                }
                .into());
            }
        };
        if request.redirect_uris.is_empty() {
            return Err(OAuthError::InvalidRequest {
                reason: "at least one redirect uri is required",
            }
            .into());
        }
        for uri in &request.redirect_uris {
            if !is_absolute_http_url(uri) {
                return Err(OAuthError::InvalidRequest {
                    reason: "the redirect uri must be an absolute http or https url",
                }
                .into());
            }
        }
        for grant in &request.allowed_grant_types {
            if !SUPPORTED_GRANT_TYPES.contains(&grant.as_str()) {
                return Err(OAuthError::InvalidRequest {
                    reason: "the allowed grant type is not supported",
                }
                .into());
            }
        }
        for uri in [
            request.backchannel_logout_uri.as_deref(),
            request.frontchannel_logout_uri.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            validate_logout_uri(uri).map_err(|reason| OAuthError::InvalidRequest { reason })?;
        }

        let client_id = format!("client-{}", new_secret());
        let (client_secret, client_secret_hash) = match client_type {
            ClientType::Confidential => {
                let secret = new_secret();
                let hash = digest_base64url(secret.as_bytes());
                (Some(secret), Some(hash))
            }
            ClientType::Public => (None, None),
        };

        let new_client = NewClient {
            client_id: client_id.clone(),
            client_secret_hash,
            client_type: client_type.as_str().to_string(),
            name: name.to_string(),
            redirect_uris: request.redirect_uris.clone(),
            allowed_grant_types: request.allowed_grant_types.clone(),
            allowed_scopes: request.allowed_scopes.clone(),
            backchannel_logout_uri: request.backchannel_logout_uri.clone(),
            frontchannel_logout_uri: request.frontchannel_logout_uri.clone(),
            status: ClientStatus::Active.as_str().to_string(),
            expires_at: request
                .expires_in_seconds
                .map(|seconds| now() + time::Duration::seconds(seconds)),
            created_by: ctx.user_id().map(str::to_string),
        };
        let row = self
            .store
            .insert_client(&new_client)
            .await
            .map_err(|error| self.internal("the oauth client could not be created", error))?;

        self.audit_event(
            ctx,
            event_code::CLIENT_REGISTERED,
            object_type::CLIENT,
            &row.client_id,
            None,
            Some(serde_json::json!({
                "clientId": row.client_id,
                "clientType": row.client_type,
                "name": row.name,
                "status": row.status,
            })),
        )
        .await;

        Ok(CreatedClient {
            client_id: row.client_id,
            client_secret,
            client_type: row.client_type,
            name: row.name,
            redirect_uris: row.redirect_uris,
            allowed_grant_types: row.allowed_grant_types,
            allowed_scopes: row.allowed_scopes,
            backchannel_logout_uri: row.backchannel_logout_uri,
            frontchannel_logout_uri: row.frontchannel_logout_uri,
            status: row.status,
            expires_at: row.expires_at.map(|at| format_timestamp_micros_utc(&at)),
            created_at: format_timestamp_micros_utc(&row.created_at),
        })
    }

    /// 列出**全部** client（平台级 → 所有租户共享）。
    pub async fn list_clients(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<ClientView>, ServiceError> {
        let rows = self
            .store
            .list_clients()
            .await
            .map_err(|error| self.internal("the oauth clients could not be listed", error))?;
        let _ = ctx;
        Ok(rows.iter().map(client_view).collect())
    }

    /// 读单个 client。
    pub async fn get_client(
        &self,
        ctx: &RequestContext,
        client_id: &str,
    ) -> Result<ClientView, ServiceError> {
        let _ = ctx;
        let row = self
            .store
            .get_client(client_id)
            .await
            .map_err(|error| self.internal("the oauth client could not be read", error))?
            .ok_or(OAuthError::ClientNotFound)?;
        Ok(client_view(&row))
    }

    /// 改 client 的状态（停用 / 启用）。
    pub async fn set_client_status(
        &self,
        ctx: &RequestContext,
        client_id: &str,
        status: ClientStatus,
    ) -> Result<ClientView, ServiceError> {
        let affected = self
            .store
            .set_client_status(client_id, status.as_str())
            .await
            .map_err(|error| self.internal("the oauth client could not be updated", error))?;
        if affected == 0 {
            return Err(OAuthError::ClientNotFound.into());
        }
        self.audit_event(
            ctx,
            event_code::CLIENT_UPDATED,
            object_type::CLIENT,
            client_id,
            None,
            Some(serde_json::json!({ "clientId": client_id, "status": status.as_str() })),
        )
        .await;
        self.get_client(ctx, client_id).await
    }

    /// 删除 client。
    pub async fn delete_client(
        &self,
        ctx: &RequestContext,
        client_id: &str,
    ) -> Result<(), ServiceError> {
        let affected = self
            .store
            .delete_client(client_id)
            .await
            .map_err(|error| self.internal("the oauth client could not be deleted", error))?;
        if affected == 0 {
            return Err(OAuthError::ClientNotFound.into());
        }
        self.audit_event(
            ctx,
            event_code::CLIENT_DELETED,
            object_type::CLIENT,
            client_id,
            None,
            Some(serde_json::json!({ "clientId": client_id })),
        )
        .await;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 授权：preview / confirm / deny
    // -----------------------------------------------------------------------

    /// 校验授权请求并返回同意票据（或免登直发的跳转地址）。
    pub async fn preview(
        &self,
        ctx: &RequestContext,
        user: &AuthenticatedUser,
        request: &PreviewRequest,
    ) -> Result<PreviewResult, ServiceError> {
        let settings = self.settings(ctx).await?;
        let client = self
            .usable_client(request.client_id.as_deref().unwrap_or_default())
            .await?;

        // response_type：只支持 `code`。
        let response_type = request.response_type.as_deref().unwrap_or_default();
        if response_type.is_empty() {
            return Err(OAuthError::InvalidRequest {
                reason: "the response type is missing",
            }
            .into());
        }
        if !SUPPORTED_RESPONSE_TYPES.contains(&response_type) {
            return Err(OAuthError::InvalidRequest {
                reason: "the response type is not supported",
            }
            .into());
        }

        // 回调地址必须在白名单里（**精确匹配** → 防开放重定向）。
        let redirect_uri = request.redirect_uri.as_deref().unwrap_or_default().trim();
        if redirect_uri.is_empty() {
            return Err(OAuthError::InvalidRequest {
                reason: "the redirect uri is missing",
            }
            .into());
        }
        if !client.redirect_uris.iter().any(|uri| uri == redirect_uri) {
            return Err(OAuthError::InvalidRedirectUri.into());
        }

        // PKCE：挑战**必填**，方法**只支持** `S256`。
        let code_challenge = request.code_challenge.as_deref().unwrap_or_default();
        match pkce::verify_challenge(
            Some(code_challenge),
            request.code_challenge_method.as_deref(),
        ) {
            Ok(()) => {}
            Err(PkceError::MissingChallenge) => {
                return Err(OAuthError::InvalidRequest {
                    reason: "the code challenge is missing",
                }
                .into());
            }
            Err(PkceError::UnsupportedMethod) => {
                return Err(OAuthError::UnsupportedCodeChallengeMethod.into());
            }
            Err(PkceError::Mismatch) => {
                return Err(OAuthError::InvalidRequest {
                    reason: "the code challenge is invalid",
                }
                .into());
            }
        }

        // scope：这里**只**判语法（缺 scope / 不是合法的 `scope-token`）。
        //
        // ⚠️ **不判交集** —— 交集由**令牌端点**判（AC-S-140 要的是**协议错误体**
        // `{"error":"invalid_scope"}`；`preview` 是平台端点，错误体是 Problem
        // Details → 在这里判死，那条 AC 就永远走不到令牌端点）。
        //
        // ⚠️ 但 `scopes`（**展示**用）必须是**交集**：AC-S-146 的标题是
        // 「用户所见 = 实际授予」。展示「请求值」会让用户看到自己**拿不到**的
        // scope（同意页显示 read+write，grant 里只有 read）——「所见非所得」。
        //
        // → `scopes` 字段 = `requested ∩ client 白名单 ∩ 用户权限点`（= `confirm`
        //   写进 grant 的那一份）；而**票据**仍装 `requested`，
        //   → 令牌端点才会按**请求的** scope 判 400 `invalid_scope`（AC-S-140 不变）。
        let requested = Self::parse_requested_scope(request.scope.as_deref().unwrap_or_default())?;
        let granted = self
            .granted_scopes(ctx, &user.user_id, &client, &requested)
            .await?;

        let application = ApplicationView {
            client_id: client.client_id.clone(),
            name: client.name.clone(),
        };

        // 免登：`prompt=none` 且已有 active grant 覆盖请求的 scope。
        if request.prompt.as_deref().map(str::trim) == Some("none") {
            if let Some(grant) = self
                .store
                .get_grant(ctx, &client.client_id, &user.user_id)
                .await
                .map_err(|error| self.internal("the oauth grant could not be read", error))?
            {
                if grant.status == GrantStatus::Active.as_str()
                    && scopes_cover(&grant.scopes, &requested)
                {
                    let redirect_to = self
                        .issue_authorization_code(
                            ctx,
                            &client,
                            user,
                            &redirect_uri,
                            &requested,
                            request.state.as_deref(),
                            code_challenge,
                            request.code_challenge_method.as_deref().unwrap_or_default(),
                            &settings,
                        )
                        .await?;
                    return Ok(PreviewResult {
                        needs_consent: false,
                        ticket: None,
                        application,
                        scopes: granted,
                        redirect_to: Some(redirect_to),
                    });
                }
            }
        }

        // 需要同意 → 签票据。
        //
        // ⚠️ **票据装 `requested`**（不是 `granted`）：AC-S-140 的判据要求
        //    「请求了用户没有的 scope → 令牌端点 400 `invalid_scope`」。
        //    若票据只装交集，那条路径**永远成功** → AC-S-140 失效（D260 的取舍）。
        let ticket = ticket::sign(
            self.iam.keys(),
            self.issuer(),
            &client.client_id,
            redirect_uri,
            &requested.join(" "),
            code_challenge,
            request.code_challenge_method.as_deref().unwrap_or_default(),
            request.state.as_deref(),
            &user.user_id,
            &user.tenant_id,
            unix_now(),
        )?;

        Ok(PreviewResult {
            needs_consent: true,
            ticket: Some(ticket),
            application,
            scopes: granted,
            redirect_to: None,
        })
    }

    /// 用户点「允许」→ 验签票据 → 写同意记录 → 签发授权码。
    pub async fn confirm(
        &self,
        ctx: &RequestContext,
        user: &AuthenticatedUser,
        ticket_token: &str,
    ) -> Result<RedirectResult, ServiceError> {
        let ticket = ticket::verify(&self.platform_keys, self.issuer(), ticket_token)?;
        if ticket.sub != user.user_id || ticket.tid != user.tenant_id {
            return Err(OAuthError::TicketInvalid.into());
        }
        let settings = self.settings(ctx).await?;
        let client = self.usable_client(&ticket.client_id).await?;
        // 票据里的 scope = **请求的** scope（D204：`confirm` **只信 ticket**，
        // 不重新接受原始参数 → 用户看到的与授予的必然一致）。
        let requested = ticket.scopes();

        // 同意记录里存的是**实际能授予**的 scope（D202：`scopes` = 实际授予的，
        // **不是**请求的）→ 交集 = client 白名单 ∩ 用户自身的有效权限点。
        //
        // ⚠️ 交集为空也**照写**：令牌端点会按**请求的** scope 判 → 不覆盖就
        // 400 `invalid_scope`（AC-S-140）→ 不会有令牌被签发。
        let granted = self
            .granted_scopes(ctx, &user.user_id, &client, &requested)
            .await?;

        // 写 / 更新同意记录（**幂等**）。
        self.store
            .upsert_grant(ctx, &client.client_id, &user.user_id, &granted, now())
            .await
            .map_err(|error| self.internal("the oauth grant could not be written", error))?;

        self.audit_event(
            ctx,
            event_code::AUTHORIZATION_APPROVED,
            object_type::GRANT,
            &client.client_id,
            None,
            Some(serde_json::json!({
                "clientId": client.client_id,
                "userId": user.user_id,
                "scopes": granted,
            })),
        )
        .await;

        let redirect_to = self
            .issue_authorization_code(
                ctx,
                &client,
                user,
                &ticket.redirect_uri,
                &requested,
                ticket.state.as_deref(),
                &ticket.code_challenge,
                &ticket.code_challenge_method,
                &settings,
            )
            .await?;
        Ok(RedirectResult { redirect_to })
    }

    /// 用户点「拒绝」→ 验签票据 → 跳回 `redirect_uri?error=access_denied&state=…`。
    pub async fn deny(
        &self,
        ctx: &RequestContext,
        user: &AuthenticatedUser,
        ticket_token: &str,
    ) -> Result<RedirectResult, ServiceError> {
        let ticket = ticket::verify(&self.platform_keys, self.issuer(), ticket_token)?;
        if ticket.sub != user.user_id || ticket.tid != user.tenant_id {
            return Err(OAuthError::TicketInvalid.into());
        }
        self.audit_event(
            ctx,
            event_code::AUTHORIZATION_DENIED,
            object_type::GRANT,
            &ticket.client_id,
            None,
            Some(serde_json::json!({
                "clientId": ticket.client_id,
                "userId": user.user_id,
            })),
        )
        .await;
        let redirect_to = build_redirect(&ticket.redirect_uri, "error", "access_denied")
            + &ticket
                .state
                .as_deref()
                .map(|state| format!("&state={}", encode_query_value(state)))
                .unwrap_or_default();
        Ok(RedirectResult { redirect_to })
    }

    // -----------------------------------------------------------------------
    // 令牌端点
    // -----------------------------------------------------------------------

    /// 用授权码换令牌。
    pub async fn exchange_authorization_code(
        &self,
        ctx: &RequestContext,
        client: &ClientIdentity,
        code: &str,
        redirect_uri: &str,
        code_verifier: &str,
    ) -> Result<TokenResponse, ProtocolError> {
        let settings = self
            .settings(ctx)
            .await
            .map_err(|_| ProtocolError::ServerError)?;
        let code_hash = digest_base64url(code.as_bytes());
        // 协议端点**没有租户上下文**（调用方是 client 的后端服务）→ 跨租户定位
        // （按 43 字符的不可枚举摘要；函数是 SECURITY DEFINER，只返回那一行）。
        let row = self
            .store
            .get_code_by_hash_any_tenant(&code_hash)
            .await
            .map_err(|_| ProtocolError::ServerError)?
            .ok_or(ProtocolError::InvalidGrant)?;

        // 授权码**绑定租户**（D245）→ 用它**覆盖**租户上下文。
        //
        // ⚠️ 覆盖之后，每一步（删码 / 写 grant / 写令牌 / 读权限点）都在**那个租户**
        // 里 → RLS 仍然是兜底，跨租户依然读不到。
        let scoped = ctx.clone().with_tenant(row.tenant_id.clone());
        let ctx = &scoped;

        // 过期 → invalid_grant（**不**泄露「存在但过期」与「不存在」的区别）。
        if row.expires_at <= now() {
            return Err(ProtocolError::InvalidGrant);
        }
        // 绑定项必须逐字相同。
        if row.client_id != client.client_id || row.redirect_uri != redirect_uri {
            return Err(ProtocolError::InvalidGrant);
        }
        // PKCE。
        if pkce::verify(
            Some(&row.code_challenge),
            Some(&row.code_challenge_method),
            code_verifier,
        )
        .is_err()
        {
            return Err(ProtocolError::InvalidGrant);
        }

        // scope：**三重交集**（AC-S-140）——全有或全无。
        //
        // ⚠️ 判据在**令牌端点**（协议错误体 `{"error":"invalid_scope"}`）：
        // 通配一律不生效（D241）/ 不得超出 client 白名单 / 不得超出用户本人。
        let scopes = row
            .scope
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let granted = self
            .granted_scopes(ctx, &row.user_id, client, &scopes)
            .await
            .map_err(|_| ProtocolError::ServerError)?;
        Self::require_scope_granted(&scopes, &granted)?;

        // 交换：**先删码**（一次性），再写 grant 与令牌。
        //
        // ⚠️ 删除是**单条 DELETE** → 天然 exactly-once：
        // 并发下两个请求都读到码，但只有一个能删掉它（另一个 `deleted == 0`）。
        let deleted = self
            .store
            .delete_code_by_hash(ctx, &code_hash)
            .await
            .map_err(|_| ProtocolError::ServerError)?;
        if deleted == 0 {
            return Err(ProtocolError::InvalidGrant);
        }
        self.store
            .upsert_grant(ctx, &client.client_id, &row.user_id, &scopes, now())
            .await
            .map_err(|_| ProtocolError::ServerError)?;
        let grant_id = self
            .store
            .get_grant(ctx, &client.client_id, &row.user_id)
            .await
            .map_err(|_| ProtocolError::ServerError)?
            .map(|grant| grant.id);

        let token = self
            .issue_access_token(
                ctx,
                client,
                Some(&row.user_id),
                &scopes,
                grant_type::AUTHORIZATION_CODE,
                grant_id.as_deref(),
                settings.access_token_ttl_seconds,
            )
            .await?;

        self.audit_event(
            ctx,
            event_code::TOKEN_ISSUED,
            object_type::TOKEN,
            &client.client_id,
            None,
            Some(serde_json::json!({
                "clientId": client.client_id,
                "grantType": grant_type::AUTHORIZATION_CODE,
                "scopes": scopes,
            })),
        )
        .await;

        Ok(token)
    }

    /// 用 client credentials 换令牌（**无用户**；scope 是**两重**交集，D231）。
    pub async fn exchange_client_credentials(
        &self,
        ctx: &RequestContext,
        client: &ClientIdentity,
        requested_scope: &str,
    ) -> Result<TokenResponse, ProtocolError> {
        // `client_credentials` **无用户、无授权** → 令牌是**平台级**资产（D219）
        // → 落在 OAuth 自己的保留伪租户里（**不是** `platform-config` 保留的
        // `"platform"` —— 用它读配置会被 `ensure_tenant_not_reserved` 拒掉）。
        let scoped = ctx.clone().with_tenant(crate::store::OAUTH_PLATFORM_TENANT);
        let ctx = &scoped;
        let settings = self
            .settings(ctx)
            .await
            .map_err(|_| ProtocolError::ServerError)?;
        let requested = parse_scope(requested_scope);
        if requested.is_empty() {
            return Err(ProtocolError::InvalidScope);
        }
        // 通配**一律不生效**（D241）：令牌 scope 层面通配**永远**不生效，
        // **即使**它被登记进了权限点注册表（AC-S-140 的 TC-022-043）。
        if requested.iter().any(|scope| scope.contains('*')) {
            return Err(ProtocolError::InvalidScope);
        }
        let allowed: BTreeSet<&str> = client.allowed_scopes.iter().map(String::as_str).collect();
        if requested
            .iter()
            .any(|scope| !allowed.contains(scope.as_str()))
        {
            return Err(ProtocolError::InvalidScope);
        }

        let token = self
            .issue_access_token(
                ctx,
                client,
                None,
                &requested,
                grant_type::CLIENT_CREDENTIALS,
                None,
                settings.access_token_ttl_seconds,
            )
            .await?;

        self.audit_event(
            ctx,
            event_code::TOKEN_ISSUED,
            object_type::TOKEN,
            &client.client_id,
            None,
            Some(serde_json::json!({
                "clientId": client.client_id,
                "grantType": grant_type::CLIENT_CREDENTIALS,
                "scopes": requested,
            })),
        )
        .await;

        Ok(token)
    }

    /// 用设备码换令牌（RFC 8628）。
    pub async fn exchange_device_code(
        &self,
        ctx: &RequestContext,
        client: &ClientIdentity,
        device_code: &str,
    ) -> Result<TokenResponse, ProtocolError> {
        let settings = self
            .settings(ctx)
            .await
            .map_err(|_| ProtocolError::ServerError)?;
        let device_code_hash = digest_base64url(device_code.as_bytes());
        // 设备码行在**批准之前**落在 OAuth 的保留伪租户里 → 跨租户定位。
        let row = self
            .store
            .get_device_code_by_hash_any_tenant(&device_code_hash)
            .await
            .map_err(|_| ProtocolError::ServerError)?
            .ok_or(ProtocolError::InvalidGrant)?;

        // 批准时行已被**迁到用户的租户** → 用它**覆盖**租户上下文。
        // 之后写 grant / 写令牌都在**用户的租户**里 → grant 级撤销（D210）才生效。
        let scoped = ctx.clone().with_tenant(row.tenant_id.clone());
        let ctx = &scoped;

        if row.client_id != client.client_id {
            return Err(ProtocolError::InvalidGrant);
        }
        if row.device_code_expires_at <= now() {
            return Err(ProtocolError::ExpiredToken);
        }

        match DeviceCodeStatus::parse(&row.status) {
            Some(DeviceCodeStatus::Approved) => {}
            Some(DeviceCodeStatus::Denied) => return Err(ProtocolError::AccessDenied),
            Some(DeviceCodeStatus::Pending) | None => {
                // 未授权：过快 → slow_down，否则 authorization_pending。
                let interval = row.poll_interval_seconds.max(0) as i64;
                let last_polled = row.last_polled_at;
                let too_fast = match last_polled {
                    Some(last) => (now() - last).whole_seconds() < interval,
                    None => false,
                };
                if too_fast {
                    let next_interval = interval + 5;
                    self.store
                        .mark_device_code_polled(ctx, &row.id, now(), next_interval as i32)
                        .await
                        .map_err(|_| ProtocolError::ServerError)?;
                    return Err(ProtocolError::SlowDown);
                }
                self.store
                    .mark_device_code_polled(ctx, &row.id, now(), row.poll_interval_seconds)
                    .await
                    .map_err(|_| ProtocolError::ServerError)?;
                return Err(ProtocolError::AuthorizationPending);
            }
        }

        let Some(user_id) = row.user_id.clone() else {
            return Err(ProtocolError::AuthorizationPending);
        };
        let scopes = row
            .scope
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();

        // scope：**重新判一次**（不追溯，D242）—— 批准之后用户可能已经被收回权限。
        let granted = self
            .granted_scopes(ctx, &user_id, client, &scopes)
            .await
            .map_err(|_| ProtocolError::ServerError)?;
        Self::require_scope_granted(&scopes, &granted)?;

        // 令牌发出后**删除设备码行**（一次性）。
        self.store
            .delete_device_code(ctx, &row.id)
            .await
            .map_err(|_| ProtocolError::ServerError)?;

        let grant_id = self
            .store
            .get_grant(ctx, &client.client_id, &user_id)
            .await
            .map_err(|_| ProtocolError::ServerError)?
            .map(|grant| grant.id);

        let token = self
            .issue_access_token(
                ctx,
                client,
                Some(&user_id),
                &scopes,
                grant_type::DEVICE_CODE,
                grant_id.as_deref(),
                settings.access_token_ttl_seconds,
            )
            .await?;

        self.audit_event(
            ctx,
            event_code::TOKEN_ISSUED,
            object_type::TOKEN,
            &client.client_id,
            None,
            Some(serde_json::json!({
                "clientId": client.client_id,
                "grantType": grant_type::DEVICE_CODE,
                "scopes": scopes,
            })),
        )
        .await;

        Ok(token)
    }

    // -----------------------------------------------------------------------
    // 设备码端点
    // -----------------------------------------------------------------------

    /// 申请设备码（RFC 8628 §3.1 / §3.2）。
    pub async fn device_authorization(
        &self,
        ctx: &RequestContext,
        client: &ClientIdentity,
        requested_scope: &str,
    ) -> Result<DeviceAuthorizationResponse, ProtocolError> {
        // 设备码行在**批准之前**还不知道用户属于哪个租户 → 落在 OAuth 自己的
        // 保留伪租户里；**批准**时再迁到用户的租户（见 `approve_device_code`）。
        let scoped = ctx.clone().with_tenant(crate::store::OAUTH_PLATFORM_TENANT);
        let ctx = &scoped;
        let settings = self
            .settings(ctx)
            .await
            .map_err(|_| ProtocolError::ServerError)?;
        let requested = parse_scope(requested_scope);
        if requested.is_empty() {
            return Err(ProtocolError::InvalidScope);
        }
        // 通配**一律不生效**（D241）。
        if requested.iter().any(|scope| scope.contains('*')) {
            return Err(ProtocolError::InvalidScope);
        }
        // 此刻**还没有用户** → 只校验 client 白名单（两重交集）。
        let allowed: BTreeSet<&str> = client.allowed_scopes.iter().map(String::as_str).collect();
        if requested
            .iter()
            .any(|scope| !allowed.contains(scope.as_str()))
        {
            return Err(ProtocolError::InvalidScope);
        }

        let device_code = new_secret();
        let user_code = new_user_code();
        let base = settings.frontend_base(self.issuer());
        let device_code_expires_at =
            now() + time::Duration::seconds(settings.device_code_ttl_seconds);
        let new_device = NewDeviceCode {
            id: uuid::Uuid::new_v4().to_string(),
            client_id: client.client_id.clone(),
            device_code_hash: digest_base64url(device_code.as_bytes()),
            user_code_hash: digest_base64url(user_code.as_bytes()),
            scope: requested.join(" "),
            device_code_expires_at,
            user_code_expires_at: device_code_expires_at,
            poll_interval_seconds: settings.device_code_poll_interval_seconds as i32,
        };
        self.store
            .insert_device_code(ctx, &new_device)
            .await
            .map_err(|_| ProtocolError::ServerError)?;

        Ok(DeviceAuthorizationResponse {
            device_code,
            user_code: user_code.clone(),
            verification_uri: format!("{base}/oauth/device"),
            verification_uri_complete: format!(
                "{base}/oauth/device?user_code={}",
                encode_query_value(&user_code)
            ),
            expires_in: settings.device_code_ttl_seconds,
            interval: settings.device_code_poll_interval_seconds,
        })
    }

    /// 设备码页的展示信息（批准**前**）。
    ///
    /// ⚠️ **不按租户过滤**：设备码行在**批准之前**落在 OAuth 的保留伪租户里
    /// （还不知道用户属于哪个租户）→ 没有可过滤的租户。
    ///
    /// # ⚠️ 这算不算越权
    ///
    /// 算「**有限**的越权」，是 RFC 8628 的固有形状：`user_code` 只有 8 位
    /// base20（20^8 ≈ 2.6×10^10）→ **可枚举** → 所以这里**只**回
    /// client 名与请求的 scope，**不回**任何用户数据、**不回**令牌。
    /// 真正的授权发生在 `approve_device_code`（要用户会话 + 用户的权限点）。
    pub async fn verify_device_code(
        &self,
        _ctx: &RequestContext,
        user_code: &str,
    ) -> Result<DeviceView, ServiceError> {
        let hash = digest_base64url(user_code.as_bytes());
        // 设备码行在**批准之前**落在 OAuth 的保留伪租户里 → 跨租户定位。
        let row = self
            .store
            .get_device_code_by_user_code_hash_any_tenant(&hash)
            .await
            .map_err(|error| self.internal("the device code could not be read", error))?
            .ok_or(OAuthError::InvalidRequest {
                reason: "the user code is unknown",
            })?;
        if row.user_code_expires_at <= now() {
            return Err(OAuthError::InvalidRequest {
                reason: "the user code has expired",
            }
            .into());
        }
        let client = self
            .store
            .get_client(&row.client_id)
            .await
            .map_err(|error| self.internal("the oauth client could not be read", error))?
            .ok_or(OAuthError::ClientNotFound)?;
        Ok(DeviceView {
            client_id: client.client_id,
            client_name: client.name,
            scopes: row.scope.split_whitespace().map(str::to_string).collect(),
        })
    }

    /// 设备码的**批准 / 拒绝**（D257）。
    pub async fn approve_device_code(
        &self,
        ctx: &RequestContext,
        user: &AuthenticatedUser,
        user_code: &str,
        approve: bool,
    ) -> Result<DeviceApprovalResult, ServiceError> {
        let hash = digest_base64url(user_code.as_bytes());
        // 设备码行在**批准之前**落在 OAuth 的保留伪租户里 → 跨租户定位。
        let row = self
            .store
            .get_device_code_by_user_code_hash_any_tenant(&hash)
            .await
            .map_err(|error| self.internal("the device code could not be read", error))?
            .ok_or(OAuthError::InvalidRequest {
                reason: "the user code is unknown",
            })?;
        if row.user_code_expires_at <= now() {
            return Err(OAuthError::InvalidRequest {
                reason: "the user code has expired",
            }
            .into());
        }
        if row.status != DeviceCodeStatus::Pending.as_str() {
            return Err(OAuthError::InvalidRequest {
                reason: "the user code has already been used",
            }
            .into());
        }

        // 批准：**不得越过用户本人** → scope 必须是用户有效权限点的子集。
        //
        // ⚠️ **先验 scope、再落库** —— 反过来会让「scope 不合法」把设备码行
        // 提前标成 `approved`，而同意记录又没写成 → 用户点了「允许」却换不到令牌。
        let scopes = row
            .scope
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if approve {
            let user_permissions = self
                .permissions
                .effective_permissions(ctx, &user.user_id)
                .await
                .map_err(|error| self.internal("the user permissions could not be read", error))?;
            let allowed: BTreeSet<&str> = user_permissions.iter().map(String::as_str).collect();
            if scopes.iter().any(|scope| !allowed.contains(scope.as_str())) {
                return Err(OAuthError::InvalidScope.into());
            }
        }

        // 批准 / 拒绝：**跨租户**写（SECURITY DEFINER）→ 并在**批准**时把设备码行
        // **迁到用户的租户**（`user.tenant_id` 来自平台会话）。
        //
        // ⚠️ 为什么必须迁：轮询换令牌时要按设备码行的 `tenant_id` 定位租户 →
        // 迁完之后，写 grant / 写令牌都在**用户的租户**里 → grant 级撤销（D210）
        // 才找得到那些令牌。
        let decided = self
            .store
            .decide_device_code(&hash, approve, &user.user_id, user.tenant_id.as_str())
            .await
            .map_err(|error| self.internal("the device code could not be updated", error))?;
        if !decided {
            return Err(OAuthError::InvalidRequest {
                reason: "the user code has already been used",
            }
            .into());
        }

        if !approve {
            return Ok(DeviceApprovalResult {
                status: "denied".to_string(),
            });
        }

        // 写 / 更新同意记录（与授权码路径**同构**，D202 / D257）。
        //
        // ⚠️ 同意记录落在**用户的租户**里（`ctx` 的租户来自平台会话）。
        self.store
            .upsert_grant(ctx, &row.client_id, &user.user_id, &scopes, now())
            .await
            .map_err(|error| self.internal("the oauth grant could not be written", error))?;

        self.audit_event(
            ctx,
            event_code::AUTHORIZATION_APPROVED,
            object_type::GRANT,
            &row.client_id,
            None,
            Some(serde_json::json!({
                "clientId": row.client_id,
                "userId": user.user_id,
                "scopes": scopes,
                "deviceCode": true,
            })),
        )
        .await;

        Ok(DeviceApprovalResult {
            status: "approved".to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // 内省 / 撤销
    // -----------------------------------------------------------------------

    /// 内省令牌（RFC 7662）。
    ///
    /// # 判定顺序（design §4.4）
    ///
    /// 验签 → `exp` → 查令牌状态 → 查 grant 状态 → **属于本 client？**
    /// → 任一不满足 → `{"active": false}`（**不报错**、**不泄露**）。
    pub async fn introspect(
        &self,
        ctx: &RequestContext,
        client: &ClientIdentity,
        token: &str,
    ) -> Result<IntrospectionResponse, ProtocolError> {
        let inactive = IntrospectionResponse {
            active: false,
            scope: None,
            client_id: None,
            username: None,
            token_type: None,
            exp: None,
            iat: None,
            sub: None,
            iss: None,
            aud: None,
        };

        let claims: AccessTokenClaims = match self.platform_keys.verify(
            token,
            Some(self.issuer()),
            None,
            &["exp", "jti", "client_id"],
        ) {
            Ok(claims) => claims,
            // 验签失败 / 已过期 / 声明不合法 → **只回 `active: false`**（不报错、不泄露）。
            Err(_) => return Ok(inactive),
        };
        if claims.exp <= unix_now() {
            return Ok(inactive);
        }
        // **不属于本 client** → 只回 `{"active": false}`。
        if claims.client_id != client.client_id {
            return Ok(inactive);
        }
        // 令牌**绑定租户**（`tid` = 授权时的租户 ID）→ 用它**覆盖**租户上下文。
        //
        // ⚠️ 协议端点**没有租户上下文** → 没有 `tid` 的令牌**查不到**状态行 →
        // 回 `active: false`（不得退回到请求方的租户去猜）。
        if claims.tid.trim().is_empty() {
            return Ok(inactive);
        }
        let scoped = ctx.clone().with_tenant(claims.tid.clone());
        let ctx = &scoped;
        let Some(row) = self
            .store
            .get_token(ctx, &claims.jti)
            .await
            .map_err(|_| ProtocolError::ServerError)?
        else {
            return Ok(inactive);
        };
        if row.status != TokenStatus::Active.as_str() {
            return Ok(inactive);
        }
        if row.expires_at <= now() {
            return Ok(inactive);
        }
        // grant 被撤销 → 令牌立即失效（D210）。
        if let Some(grant_id) = row.grant_id.as_deref() {
            let grant = self
                .store
                .get_grant(
                    ctx,
                    &row.client_id,
                    row.user_id.as_deref().unwrap_or_default(),
                )
                .await
                .map_err(|_| ProtocolError::ServerError)?;
            match grant {
                Some(grant)
                    if grant.id == grant_id && grant.status == GrantStatus::Active.as_str() => {}
                _ => return Ok(inactive),
            }
        }

        Ok(IntrospectionResponse {
            active: true,
            scope: Some(claims.scope.clone()),
            client_id: Some(claims.client_id.clone()),
            // RFC 7662：无用户主体时**没有** `username` 键。
            username: claims.sub.clone(),
            token_type: Some("Bearer".to_string()),
            exp: Some(claims.exp),
            iat: Some(claims.iat),
            sub: claims.sub.clone(),
            iss: Some(claims.iss.clone()),
            aud: Some(claims.aud.clone()),
        })
    }

    /// 撤销令牌（RFC 7009）。
    ///
    /// ⚠️ **只撤自己的**；别人的令牌 → **200** 且**不撤销**（不泄露，D208）。
    /// 无效令牌 → **200**（RFC 7009 §2.2）。
    pub async fn revoke(
        &self,
        ctx: &RequestContext,
        client: &ClientIdentity,
        token: &str,
    ) -> Result<(), ProtocolError> {
        let claims: AccessTokenClaims = match self.platform_keys.verify(
            token,
            Some(self.issuer()),
            None,
            &["exp", "jti", "client_id"],
        ) {
            Ok(claims) => claims,
            Err(_) => return Ok(()),
        };
        if claims.client_id != client.client_id {
            return Ok(());
        }
        // 令牌**绑定租户**（`tid` = 授权时的租户 ID）→ 用它**覆盖**租户上下文。
        //
        // ⚠️ 协议端点**没有租户上下文** → 没有 `tid` 的令牌**撤不掉**
        // （也不该撤：撤销同样只影响**自己的**令牌）。
        if claims.tid.trim().is_empty() {
            return Ok(());
        }
        let scoped = ctx.clone().with_tenant(claims.tid.clone());
        let ctx = &scoped;
        let Some(row) = self
            .store
            .get_token(ctx, &claims.jti)
            .await
            .map_err(|_| ProtocolError::ServerError)?
        else {
            return Ok(());
        };
        self.store
            .revoke_token(ctx, &row.token_id)
            .await
            .map_err(|_| ProtocolError::ServerError)?;
        self.audit_event(
            ctx,
            event_code::TOKEN_REVOKED,
            object_type::TOKEN,
            &row.token_id,
            None,
            Some(serde_json::json!({
                "clientId": client.client_id,
                "tokenId": row.token_id,
            })),
        )
        .await;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 授权记录
    // -----------------------------------------------------------------------

    /// 「我的授权」列表。
    pub async fn list_grants(
        &self,
        ctx: &RequestContext,
        user: &AuthenticatedUser,
    ) -> Result<Vec<GrantView>, ServiceError> {
        let rows = self
            .store
            .list_grants_for_user(ctx, &user.user_id)
            .await
            .map_err(|error| self.internal("the oauth grants could not be listed", error))?;
        let client_ids: Vec<String> = rows.iter().map(|row| row.client_id.clone()).collect();
        let clients = self
            .store
            .get_clients(&client_ids)
            .await
            .map_err(|error| self.internal("the oauth clients could not be read", error))?;
        let names: BTreeMap<String, String> = clients
            .into_iter()
            .map(|client| (client.client_id, client.name))
            .collect();
        Ok(rows
            .iter()
            .map(|row| GrantView {
                client_id: row.client_id.clone(),
                client_name: names.get(&row.client_id).cloned(),
                scopes: row.scopes.clone(),
                granted_at: format_timestamp_micros_utc(&row.granted_at),
                status: row.status.clone(),
                revoked_at: row.revoked_at.map(|at| format_timestamp_micros_utc(&at)),
            })
            .collect())
    }

    /// 撤销授权（= **按应用登出**）。
    pub async fn revoke_grant(
        &self,
        ctx: &RequestContext,
        user: &AuthenticatedUser,
        client_id: &str,
    ) -> Result<RevokeGrantResult, ServiceError> {
        let grant = self
            .store
            .get_grant(ctx, client_id, &user.user_id)
            .await
            .map_err(|error| self.internal("the oauth grant could not be read", error))?
            .ok_or(OAuthError::GrantNotFound)?;
        self.store
            .revoke_grant(ctx, client_id, &user.user_id)
            .await
            .map_err(|error| self.internal("the oauth grant could not be revoked", error))?;
        // grant 级撤销 → 该 grant 下的**全部**令牌立即失效（D210 / D254）。
        self.store
            .revoke_tokens_for_grant(ctx, &grant.id)
            .await
            .map_err(|error| self.internal("the oauth tokens could not be revoked", error))?;
        self.audit_event(
            ctx,
            event_code::TOKEN_REVOKED,
            object_type::GRANT,
            client_id,
            None,
            Some(serde_json::json!({
                "clientId": client_id,
                "userId": user.user_id,
            })),
        )
        .await;
        Ok(RevokeGrantResult {
            status: "revoked".to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // 元数据
    // -----------------------------------------------------------------------

    /// 授权服务器元数据（RFC 8414）。
    pub async fn metadata(
        &self,
        ctx: &RequestContext,
    ) -> Result<AuthorizationServerMetadata, ServiceError> {
        let settings = self.settings(ctx).await?;
        let issuer = self.issuer().to_string();
        let base = settings.frontend_base(&issuer);
        Ok(AuthorizationServerMetadata {
            authorization_endpoint: format!("{base}/oauth/authorize"),
            token_endpoint: format!("{issuer}/api/oauth/token"),
            device_authorization_endpoint: format!("{issuer}/api/oauth/device_authorization"),
            introspection_endpoint: format!("{issuer}/api/oauth/introspect"),
            revocation_endpoint: format!("{issuer}/api/oauth/revoke"),
            jwks_uri: format!("{issuer}/.well-known/jwks.json"),
            grant_types_supported: SUPPORTED_GRANT_TYPES
                .iter()
                .map(|value| value.to_string())
                .collect(),
            response_types_supported: SUPPORTED_RESPONSE_TYPES
                .iter()
                .map(|value| value.to_string())
                .collect(),
            code_challenge_methods_supported: SUPPORTED_CODE_CHALLENGE_METHODS
                .iter()
                .map(|value| value.to_string())
                .collect(),
            token_endpoint_auth_methods_supported: vec![
                "client_secret_basic".to_string(),
                "none".to_string(),
            ],
            introspection_endpoint_auth_methods_supported: vec!["client_secret_basic".to_string()],
            revocation_endpoint_auth_methods_supported: vec!["client_secret_basic".to_string()],
            access_token_expires_in: settings.access_token_ttl_seconds,
            issuer,
        })
    }

    // -----------------------------------------------------------------------
    // 内部
    // -----------------------------------------------------------------------

    /// 取一个**可用**的 client（不存在 / 停用 / 过期 → 各自的错误码）。
    async fn usable_client(&self, client_id: &str) -> Result<ClientIdentity, ServiceError> {
        if client_id.trim().is_empty() {
            return Err(OAuthError::InvalidRequest {
                reason: "the client id is missing",
            }
            .into());
        }
        let row = self
            .store
            .get_client(client_id)
            .await
            .map_err(|error| self.internal("the oauth client could not be read", error))?
            .ok_or(OAuthError::ClientNotFound)?;
        if row.status == ClientStatus::Disabled.as_str() {
            return Err(OAuthError::ClientDisabled.into());
        }
        if let Some(expires_at) = row.expires_at {
            if expires_at <= now() {
                return Err(OAuthError::ClientExpired.into());
            }
        }
        Ok(ClientIdentity {
            client_id: row.client_id,
            client_type: row.client_type,
            redirect_uris: row.redirect_uris,
            allowed_scopes: row.allowed_scopes,
            allowed_grant_types: row.allowed_grant_types,
            name: row.name,
        })
    }

    /// scope 的**语法**校验（**不**判交集）。
    ///
    /// # 只管语法的原因
    ///
    /// **交集**由**令牌端点**判（AC-S-140：请求的 scope 超出用户本人 / 超出 client
    /// 白名单 / 含通配 → 400 `{"error":"invalid_scope"}`，**协议错误体**）。
    ///
    /// 而 `preview` 是**平台端点**（错误体是 Problem Details）→ 在这里把 scope
    /// 判死，AC-S-140 的判据就**永远走不到令牌端点** → 那条 AC 就没了。
    ///
    /// → `preview` 只挡**参数非法**（缺 scope / 不是合法的 `scope-token`），
    /// 其余一律留给令牌端点（D233 第 3 条：preview 的错误只针对「参数非法、
    /// client 不可用」）。
    fn parse_requested_scope(raw: &str) -> Result<Vec<String>, ServiceError> {
        let requested = parse_scope(raw);
        if requested.is_empty() {
            return Err(OAuthError::InvalidScope.into());
        }
        // 字符集必须是 RFC 6749 §3.3 的 `scope-token`（**不静默截断**）。
        if requested.iter().any(|scope| !is_scope_token(scope)) {
            return Err(OAuthError::InvalidScope.into());
        }
        Ok(requested)
    }

    /// scope 的**三重交集**（返回交集，**不**判断是否「全有或全无」）。
    ///
    /// = 请求 scope ∩ `client.allowed_scopes` ∩ 用户自身的有效权限点（D201 §2）。
    ///
    /// ⚠️ **通配不参与交集**：含 `*` 的 scope **一律**在令牌端点被拒（D241），
    /// 所以这里**直接不把它算进交集** → 交集里永远不会出现通配字面量。
    async fn granted_scopes(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        client: &ClientIdentity,
        requested: &[String],
    ) -> Result<Vec<String>, ServiceError> {
        let client_scopes: BTreeSet<&str> =
            client.allowed_scopes.iter().map(String::as_str).collect();
        let user_permissions = self
            .permissions
            .effective_permissions(ctx, user_id)
            .await
            .map_err(|error| self.internal("the user permissions could not be read", error))?;
        let user_scopes: BTreeSet<&str> = user_permissions.iter().map(String::as_str).collect();
        Ok(requested
            .iter()
            .filter(|scope| {
                !scope.contains('*')
                    && client_scopes.contains(scope.as_str())
                    && user_scopes.contains(scope.as_str())
            })
            .cloned()
            .collect())
    }

    /// 令牌端点的 scope 判定：**全有或全无**。
    ///
    /// ⚠️ **三件事都要判**（AC-S-140 的三条路径）：
    ///
    /// 1. **通配一律不生效**（D241）—— 含 `*` 的 scope **一律**拒；
    /// 2. 请求的 scope 必须**逐个**落在 `client.allowed_scopes` 里；
    /// 3. 请求的 scope 必须**逐个**落在**用户当前**的有效权限点里
    ///    （**不追溯**：后来被授予的权限**不得**扩大已签发的令牌，D242）。
    fn require_scope_granted(
        requested: &[String],
        granted: &[String],
    ) -> Result<(), ProtocolError> {
        let allowed: BTreeSet<&str> = granted.iter().map(String::as_str).collect();
        if requested
            .iter()
            .any(|scope| scope.contains('*') || !allowed.contains(scope.as_str()))
        {
            return Err(ProtocolError::InvalidScope);
        }
        Ok(())
    }

    /// 签发授权码（**只存摘要**），返回 `redirect_to`。
    #[allow(clippy::too_many_arguments)]
    async fn issue_authorization_code(
        &self,
        ctx: &RequestContext,
        client: &ClientIdentity,
        user: &AuthenticatedUser,
        redirect_uri: &str,
        scopes: &[String],
        state: Option<&str>,
        code_challenge: &str,
        code_challenge_method: &str,
        settings: &OAuthSettings,
    ) -> Result<String, ServiceError> {
        let code = new_secret();
        let new_code = NewCode {
            id: uuid::Uuid::new_v4().to_string(),
            client_id: client.client_id.clone(),
            user_id: user.user_id.clone(),
            code_hash: digest_base64url(code.as_bytes()),
            redirect_uri: redirect_uri.to_string(),
            scope: scopes.join(" "),
            // PKCE 挑战**原样落库**（交换时重算比对）——来源是调用方：
            // `confirm` 传**票据**里的值（用户所见 = 实际授予，D204），
            // 免登直发路径传**请求**里的值。
            code_challenge: code_challenge.to_string(),
            code_challenge_method: code_challenge_method.to_string(),
            expires_at: now() + time::Duration::seconds(settings.authorization_code_ttl_seconds),
        };
        self.store
            .insert_code(ctx, &new_code)
            .await
            .map_err(|error| self.internal("the authorization code could not be written", error))?;

        let mut redirect_to = build_redirect(redirect_uri, "code", &code);
        if let Some(state) = state {
            redirect_to.push_str(&format!("&state={}", encode_query_value(state)));
        }
        Ok(redirect_to)
    }

    /// 签发 access token（JWT + 状态行）。
    async fn issue_access_token(
        &self,
        ctx: &RequestContext,
        client: &ClientIdentity,
        user_id: Option<&str>,
        scopes: &[String],
        grant: &str,
        grant_id: Option<&str>,
        ttl_seconds: i64,
    ) -> Result<TokenResponse, ProtocolError> {
        let issued_at = unix_now();
        let expires_at = issued_at + ttl_seconds;
        let jti = uuid::Uuid::new_v4().to_string();
        let claims = AccessTokenClaims {
            iss: self.issuer().to_string(),
            aud: client.client_id.clone(),
            sub: user_id.map(str::to_string),
            tid: ctx.tenant_id().unwrap_or_default().to_string(),
            client_id: client.client_id.clone(),
            scope: scopes.join(" "),
            grant_type: grant.to_string(),
            iat: issued_at,
            nbf: issued_at,
            exp: expires_at,
            jti: jti.clone(),
        };
        let access_token = self
            .iam
            .keys()
            .sign_claims(&claims)
            .map_err(|_| ProtocolError::ServerError)?;

        self.store
            .insert_token(
                ctx,
                &NewToken {
                    id: uuid::Uuid::new_v4().to_string(),
                    client_id: client.client_id.clone(),
                    user_id: user_id.map(str::to_string),
                    token_id: jti,
                    scopes: scopes.to_vec(),
                    grant_id: grant_id.map(str::to_string),
                    expires_at: now() + time::Duration::seconds(ttl_seconds),
                },
            )
            .await
            .map_err(|_| ProtocolError::ServerError)?;

        Ok(TokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in: ttl_seconds,
            scope: scopes.join(" "),
        })
    }

    /// 记一条审计事件（**失败只记日志** —— 审计不可用**不得**让登出失败，D211）。
    /// 审计埋点（`logout` 模块用）。
    pub(crate) async fn audit_event(
        &self,
        ctx: &RequestContext,
        action: &str,
        object_type: &str,
        object_id: &str,
        before: Option<serde_json::Value>,
        after: Option<serde_json::Value>,
    ) {
        let tenant_id = ctx.tenant_id().unwrap_or_default().to_string();
        let event = AuditEvent {
            tenant_id,
            actor_id: ctx.user_id().map(str::to_string),
            action: action.to_string(),
            object_type: object_type.to_string(),
            object_id: Some(object_id.to_string()),
            before,
            after,
            ip: ctx.client_ip().map(str::to_string),
            user_agent: ctx.user_agent().map(str::to_string),
        };
        if let Err(error) = self.audit.record(ctx, &event).await {
            tracing::warn!(error = %error, action, "the oauth audit event could not be recorded");
        }
    }

    /// 数据访问层错误 → 平台内部错误（**不把内部细节漏给调用方**）。
    ///
    /// 泛型取 `Display`：`DbError` 与 `PermissionProvider` 的 `String` 原因都能进来。
    fn internal<E: std::fmt::Display>(&self, summary: &str, error: E) -> ServiceError {
        tracing::error!(error = %error, summary, "the oauth data access failed");
        ServiceError::Internal {
            reason: summary.to_string(),
        }
    }

    /// 出站 client（登出用）。
    pub(crate) fn outbound(&self) -> &Arc<OutboundClient> {
        &self.outbound
    }

    /// 数据访问（登出用）。
    pub(crate) fn store(&self) -> &Arc<OAuthStore> {
        &self.store
    }

    /// 认证与账号体系（登出要作废全部会话）。
    pub(crate) fn iam(&self) -> &Arc<IamService> {
        &self.iam
    }

    /// 密钥集（签发 `logout_token` 用）。
    pub(crate) fn keys(&self) -> &platform_iam::jwt::JwtKeySet {
        self.iam.keys()
    }
}

/// 已认证的 client（协议端点用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    /// client ID。
    pub client_id: String,
    /// client 类型。
    pub client_type: String,
    /// 回调白名单（**精确匹配** → 防开放重定向）。
    pub redirect_uris: Vec<String>,
    /// 允许的 scope。
    pub allowed_scopes: Vec<String>,
    /// 允许的 grant type。
    pub allowed_grant_types: Vec<String>,
    /// 展示名。
    pub name: String,
}

/// 解析请求里的 scope（**空格分隔**）。
pub fn parse_scope(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(str::to_string).collect()
}

/// scope-token 的字符集（RFC 6749 §3.3：`%x21 / %x23-5B / %x5D-7E`）。
///
/// ⚠️ `:`（0x3A）**允许** → `module:resource:action` 合规；
/// 空格（0x20）与 `"`（0x22）、`\`（0x5C）**不允许**。
pub fn is_scope_token(raw: &str) -> bool {
    !raw.is_empty()
        && raw.bytes().all(|byte| {
            byte == 0x21 || (0x23..=0x5B).contains(&byte) || (0x5D..=0x7E).contains(&byte)
        })
}

/// `granted` 是否覆盖 `requested`（免登判定）。
fn scopes_cover(granted: &[String], requested: &[String]) -> bool {
    let granted: BTreeSet<&str> = granted.iter().map(String::as_str).collect();
    requested
        .iter()
        .all(|scope| granted.contains(scope.as_str()))
}

/// 拼 `redirect_uri?key=value`。
fn build_redirect(redirect_uri: &str, key: &str, value: &str) -> String {
    let separator = if redirect_uri.contains('?') { '&' } else { '?' };
    format!(
        "{redirect_uri}{separator}{key}={}",
        encode_query_value(value)
    )
}

/// query 值的百分号编码（RFC 3986 的 unreserved 保留）。
fn encode_query_value(raw: &str) -> String {
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

/// 是否**绝对**的 http(s) URL。
fn is_absolute_http_url(raw: &str) -> bool {
    let value = raw.trim();
    let rest = match value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
    {
        Some(rest) => rest,
        None => return false,
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    !host.is_empty()
}

/// 校验登出回调地址（**无 SSRF 入口**，D220）。
///
/// scheme 必须是 `https`；**本地开发**允许 `http`（且必须是回环地址）。
fn validate_logout_uri(raw: &str) -> Result<(), &'static str> {
    let value = raw.trim();
    if value.is_empty() {
        return Err("the logout uri is empty");
    }
    if let Some(rest) = value.strip_prefix("https://") {
        if host_of(rest).is_empty() {
            return Err("the logout uri has no host");
        }
        return Ok(());
    }
    if let Some(rest) = value.strip_prefix("http://") {
        let host = host_of(rest);
        if host.is_empty() {
            return Err("the logout uri has no host");
        }
        // 本地开发的 `http` 只允许回环地址。
        if !is_loopback_host(&host) {
            return Err("the logout uri must use https outside of local development");
        }
        return Ok(());
    }
    Err("the logout uri must use the https scheme")
}

/// 取 URL 的 host 部分（不含端口）。
fn host_of(rest: &str) -> String {
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// 是否回环地址。
fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

/// client 行 → 视图。
fn client_view(row: &crate::model::OAuthClientRow) -> ClientView {
    ClientView {
        client_id: row.client_id.clone(),
        client_type: row.client_type.clone(),
        name: row.name.clone(),
        redirect_uris: row.redirect_uris.clone(),
        allowed_grant_types: row.allowed_grant_types.clone(),
        allowed_scopes: row.allowed_scopes.clone(),
        backchannel_logout_uri: row.backchannel_logout_uri.clone(),
        frontchannel_logout_uri: row.frontchannel_logout_uri.clone(),
        status: row.status.clone(),
        expires_at: row.expires_at.map(|at| format_timestamp_micros_utc(&at)),
        created_at: format_timestamp_micros_utc(&row.created_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scope_parser_splits_on_whitespace() {
        assert_eq!(parse_scope(""), Vec::<String>::new());
        assert_eq!(parse_scope("a"), vec!["a".to_string()]);
        assert_eq!(
            parse_scope("  a   b "),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    /// RFC 6749 §3.3 的 `scope-token` 字符集。
    #[test]
    fn the_scope_token_charset_is_enforced() {
        assert!(is_scope_token("rbac:role:read"));
        assert!(is_scope_token("sandbox:demo:read"));
        assert!(is_scope_token("a-b_c.d~e"));
        // 空格、引号、反斜杠 → 不合法。
        assert!(!is_scope_token("a b"));
        assert!(!is_scope_token("a\"b"));
        assert!(!is_scope_token("a\\b"));
        assert!(!is_scope_token(""));
        // `:` 允许。
        assert!(is_scope_token("a:b"));
    }

    #[test]
    fn the_query_encoder_escapes_the_reserved_characters() {
        assert_eq!(encode_query_value("xyz"), "xyz");
        assert_eq!(encode_query_value("a b"), "a%20b");
        assert_eq!(encode_query_value("a&b=c"), "a%26b%3Dc");
        assert_eq!(encode_query_value("a/b"), "a%2Fb");
    }

    /// 免登判定：已授予的 scope 必须**覆盖**请求的 scope。
    #[test]
    fn the_scope_coverage_check_is_strict() {
        let granted = vec!["rbac:role:read".to_string()];
        assert!(scopes_cover(&granted, &["rbac:role:read".to_string()]));
        assert!(!scopes_cover(
            &granted,
            &["rbac:role:read".to_string(), "rbac:role:write".to_string()]
        ));
        assert!(scopes_cover(&granted, &[]));
    }

    /// 登出回调的 scheme 校验（**无 SSRF 入口**）。
    #[test]
    fn the_logout_uri_scheme_is_enforced() {
        assert!(validate_logout_uri("https://client.example/logout").is_ok());
        assert!(validate_logout_uri("http://127.0.0.1:8080/logout").is_ok());
        assert!(validate_logout_uri("http://localhost:8080/logout").is_ok());
        assert!(validate_logout_uri("http://client.example/logout").is_err());
        assert!(validate_logout_uri("file:///etc/passwd").is_err());
        assert!(validate_logout_uri("ftp://client.example/logout").is_err());
        assert!(validate_logout_uri("").is_err());
        assert!(validate_logout_uri("https://").is_err());
    }

    #[test]
    fn the_redirect_builder_keeps_the_existing_query() {
        assert_eq!(
            build_redirect("https://client.example/cb", "code", "abc"),
            "https://client.example/cb?code=abc"
        );
        assert_eq!(
            build_redirect("https://client.example/cb?x=1", "code", "abc"),
            "https://client.example/cb?x=1&code=abc"
        );
    }
}
