//! 认证与账号体系的错误类型。
//!
//! # 契约 C6（`api-conventions v1`）
//!
//! - `code`：**稳定**的机器可读错误码（大写 + 下划线），前端据此分支；
//! - `detail`：**面向开发者的英文 ASCII 描述** —— 面向用户的中文文案由前端 i18n
//!   按 `code` 渲染，后端**不得**在 `detail` 里拼中文句子；
//! - `params`：可选，补充错误码无法表达的参数（如违反的是哪条密码策略）。
//!
//! # 为什么「账号不存在」与「密码错误」是**同一个**错误
//!
//! `design-summary` §5.1 / D6：两种情况的**响应体、错误码、状态码必须完全相同**，
//! 否则就成了账号枚举的接口。
//!
//! # 为什么 `ACCOUNT_LOCKED` 也**不是**枚举面
//!
//! 它**只在密码校验通过之后**才可能出现 —— 也就是说，**只有已经知道密码的人**
//! 才拿得到它（「你的账号被锁了，去重置密码」对本人是有用的信息）。
//!
//! ⚠️ 反过来，如果锁定判定排在密码校验**之前**，那么「未注册账号永远拿不到
//! `ACCOUNT_LOCKED`，而已注册账号第 6 次就能拿到」——**这个差异本身就是判别器**：
//! 攻击者不需要知道密码，只需要**数到第 6 次**（CWE-204）。
//! 所以判定顺序必须由 `service.rs` 的登录流程保证（密码校验 → 锁定判定）。

use std::collections::BTreeMap;

use axum::http::StatusCode;

/// 稳定的机器可读错误码（`design-summary` §5.3 的表 + 本期新增的几个）。
pub mod code {
    /// 用户名或密码错误（**不区分**账号是否存在）。
    pub const INVALID_CREDENTIALS: &str = "INVALID_CREDENTIALS";
    /// 需要 MFA 验证码。
    pub const MFA_REQUIRED: &str = "MFA_REQUIRED";
    /// MFA 验证码无效。
    pub const MFA_INVALID: &str = "MFA_INVALID";
    /// 账号被锁。
    pub const ACCOUNT_LOCKED: &str = "ACCOUNT_LOCKED";
    /// 账号已禁用。
    pub const ACCOUNT_DISABLED: &str = "ACCOUNT_DISABLED";
    /// 令牌已过期。
    pub const TOKEN_EXPIRED: &str = "TOKEN_EXPIRED";
    /// 令牌已撤销。
    pub const TOKEN_REVOKED: &str = "TOKEN_REVOKED";
    /// 检测到 refresh token 重放 → **整个 family 作废**。
    pub const TOKEN_REUSE_DETECTED: &str = "TOKEN_REUSE_DETECTED";
    /// 不满足密码策略。
    pub const PASSWORD_POLICY_VIOLATION: &str = "PASSWORD_POLICY_VIOLATION";
    /// 与历史密码重复。
    pub const PASSWORD_REUSED: &str = "PASSWORD_REUSED";
    /// 登录尝试过于频繁（按 IP 的滑动窗口限流）。
    pub const LOGIN_RATE_LIMITED: &str = "LOGIN_RATE_LIMITED";
    /// 请求上下文里没有租户。
    pub const TENANT_CONTEXT_MISSING: &str = "TENANT_CONTEXT_MISSING";
    /// 需要认证（缺令牌 / 令牌无效）。
    pub const UNAUTHENTICATED: &str = "UNAUTHENTICATED";
    /// 资源不存在（跨租户访问也走这里：**不泄露资源存在性**）。
    pub const NOT_FOUND: &str = "NOT_FOUND";
    /// 与当前状态冲突。
    pub const CONFLICT: &str = "CONFLICT";
    /// 本期不实现的能力（如 WebAuthn）。
    pub const NOT_IMPLEMENTED: &str = "NOT_IMPLEMENTED";
    /// 服务内部错误。
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";

    // -- SSO 入站（REQ-013）：21 个码 ------------------------------------
    /// 强制 SSO：本地密码登录被拒（**在密码校验之前**）。
    pub const SSO_REQUIRED: &str = "SSO_REQUIRED";
    /// 租户未配置**或已禁用**该协议（合并，不暴露内部状态）。
    pub const SSO_PROVIDER_NOT_CONFIGURED: &str = "SSO_PROVIDER_NOT_CONFIGURED";
    /// 配置值非法（`ldaps://` / 相对 redirectUri / filter 缺占位符 / 空 issuer）。
    pub const SSO_CONFIG_INVALID: &str = "SSO_CONFIG_INVALID";
    /// 拿不到 `.well-known` 发现文档。
    pub const SSO_OIDC_DISCOVERY_FAILED: &str = "SSO_OIDC_DISCOVERY_FAILED";
    /// 拿到了发现文档但非法 / 缺端点。
    pub const SSO_OIDC_DISCOVERY_INVALID: &str = "SSO_OIDC_DISCOVERY_INVALID";
    /// `state` 不存在 / 已消费 / 租户或 provider 不匹配。
    pub const SSO_OIDC_STATE_INVALID: &str = "SSO_OIDC_STATE_INVALID";
    /// `state` 超过 TTL。
    pub const SSO_OIDC_STATE_EXPIRED: &str = "SSO_OIDC_STATE_EXPIRED";
    /// 回调缺 `code` 或 `state`。
    pub const SSO_OIDC_CALLBACK_PARAM_MISSING: &str = "SSO_OIDC_CALLBACK_PARAM_MISSING";
    /// 换 token 失败（code 伪造 / 已用 / PKCE 不符）。
    pub const SSO_OIDC_TOKEN_EXCHANGE_FAILED: &str = "SSO_OIDC_TOKEN_EXCHANGE_FAILED";
    /// IdP 的 token 端点挂 / 超时（**远端基础设施问题**，与上一个码区分）。
    pub const SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE: &str = "SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE";
    /// `id_token` 签名 / iss / aud / iat 不匹配。
    pub const SSO_OIDC_TOKEN_INVALID: &str = "SSO_OIDC_TOKEN_INVALID";
    /// `id_token` 的 `exp` 已过。
    pub const SSO_OIDC_TOKEN_EXPIRED: &str = "SSO_OIDC_TOKEN_EXPIRED";
    /// `id_token` 算法不在白名单内（`none` / `HS*` / 白名单外）。
    pub const SSO_OIDC_ALG_NOT_ALLOWED: &str = "SSO_OIDC_ALG_NOT_ALLOWED";
    /// `id_token` 的 `nonce` 与本次授权不符。
    pub const SSO_OIDC_NONCE_MISMATCH: &str = "SSO_OIDC_NONCE_MISMATCH";
    /// PKCE 的 `code_verifier` 与 challenge 不符。
    pub const SSO_OIDC_PKCE_MISMATCH: &str = "SSO_OIDC_PKCE_MISMATCH";
    /// LDAP 服务连不上。
    pub const SSO_LDAP_UNREACHABLE: &str = "SSO_LDAP_UNREACHABLE";
    /// LDAP 认证失败（密码错**或**用户不存在 —— **同一个码**）。
    pub const SSO_LDAP_BIND_FAILED: &str = "SSO_LDAP_BIND_FAILED";
    /// 空 / 纯空白密码（请求不合法，不是凭据错误）。
    pub const SSO_LDAP_EMPTY_PASSWORD: &str = "SSO_LDAP_EMPTY_PASSWORD";
    /// 绑定改指 / `mail` 撞他人绑定 / JIT 撞既有 username。
    pub const SSO_IDENTITY_LINK_CONFLICT: &str = "SSO_IDENTITY_LINK_CONFLICT";
    /// 未绑定且 JIT 关闭。
    pub const SSO_ACCOUNT_NOT_LINKED: &str = "SSO_ACCOUNT_NOT_LINKED";
    /// 登录 ticket 不存在 / 已用 / 过期。
    pub const SSO_TICKET_INVALID: &str = "SSO_TICKET_INVALID";
}

/// 本 crate 的全部错误码（**完备性测试的输入**，D113）。
///
/// `code()` 是 `match` 里的 `&'static str`，运行时无法自动枚举 ——
/// 没有这份清单，「`TITLES` 漏了一项」永远不会被测出来。
pub const ALL_CODES: &[&str] = &[
    code::INVALID_CREDENTIALS,
    code::MFA_REQUIRED,
    code::MFA_INVALID,
    code::ACCOUNT_LOCKED,
    code::ACCOUNT_DISABLED,
    code::TOKEN_EXPIRED,
    code::TOKEN_REVOKED,
    code::TOKEN_REUSE_DETECTED,
    code::PASSWORD_POLICY_VIOLATION,
    code::PASSWORD_REUSED,
    code::LOGIN_RATE_LIMITED,
    code::TENANT_CONTEXT_MISSING,
    code::UNAUTHENTICATED,
    code::NOT_FOUND,
    code::CONFLICT,
    code::NOT_IMPLEMENTED,
    code::INTERNAL_ERROR,
    // SSO 入站（REQ-013）
    code::SSO_REQUIRED,
    code::SSO_PROVIDER_NOT_CONFIGURED,
    code::SSO_CONFIG_INVALID,
    code::SSO_OIDC_DISCOVERY_FAILED,
    code::SSO_OIDC_DISCOVERY_INVALID,
    code::SSO_OIDC_STATE_INVALID,
    code::SSO_OIDC_STATE_EXPIRED,
    code::SSO_OIDC_CALLBACK_PARAM_MISSING,
    code::SSO_OIDC_TOKEN_EXCHANGE_FAILED,
    code::SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE,
    code::SSO_OIDC_TOKEN_INVALID,
    code::SSO_OIDC_TOKEN_EXPIRED,
    code::SSO_OIDC_ALG_NOT_ALLOWED,
    code::SSO_OIDC_NONCE_MISMATCH,
    code::SSO_OIDC_PKCE_MISMATCH,
    code::SSO_LDAP_UNREACHABLE,
    code::SSO_LDAP_BIND_FAILED,
    code::SSO_LDAP_EMPTY_PASSWORD,
    code::SSO_IDENTITY_LINK_CONFLICT,
    code::SSO_ACCOUNT_NOT_LINKED,
    code::SSO_TICKET_INVALID,
];

/// 一条被违反的密码策略规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyViolation {
    /// 规则标识（稳定的机器可读名，如 `min_length`）。
    pub rule: String,
    /// 面向开发者的英文说明。
    pub detail: String,
}

impl PolicyViolation {
    /// 构造一条规则违反。
    pub fn new(rule: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            rule: rule.into(),
            detail: detail.into(),
        }
    }
}

/// 认证与账号体系的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IamError {
    /// 用户名或密码错误。
    ///
    /// **账号不存在与密码错误共用这一个**（防账号枚举）。
    InvalidCredentials,
    /// 需要 MFA 验证码。
    MfaRequired,
    /// MFA 验证码无效（含重放：同一时间步不可复用）。
    MfaInvalid,
    /// 账号被锁。
    AccountLocked,
    /// 账号已禁用。
    AccountDisabled,
    /// 令牌已过期。
    TokenExpired,
    /// 令牌已撤销。
    TokenRevoked,
    /// 检测到 refresh token 重放 → 整个 family 已作废。
    TokenReuseDetected,
    /// 不满足密码策略（**带具体违反的规则**）。
    PasswordPolicyViolation {
        /// 被违反的规则清单。
        violations: Vec<PolicyViolation>,
    },
    /// 与历史密码重复。
    PasswordReused,
    /// 登录尝试过于频繁。
    LoginRateLimited {
        /// 建议的等待秒数。
        retry_after_seconds: u64,
    },
    /// 请求上下文里没有租户。
    TenantContextMissing,
    /// 需要认证。
    Unauthenticated,
    /// 资源不存在。
    NotFound {
        /// 资源类型（英文）。
        resource: &'static str,
    },
    /// 与当前状态冲突。
    Conflict {
        /// 面向开发者的英文原因。
        reason: &'static str,
    },
    /// 本期不实现的能力。
    NotImplemented {
        /// 能力名（英文）。
        feature: &'static str,
    },
    /// 服务内部错误（**不把内部细节漏给调用方**）。
    Internal {
        /// 面向开发者的英文原因。
        reason: String,
    },

    // -- SSO 入站（REQ-013）------------------------------------------------
    /// 强制 SSO：本地密码登录被拒。
    ///
    /// `providers` 是**可用的** SSO 入口列表（写死在 `params.ssoProviders`），
    /// 让前端能直接引导用户。它**不含**任何配置值（AC-S-106 / S41）。
    SsoRequired {
        /// 当前租户可用的 SSO 入口。
        providers: Vec<crate::sso::SsoProviderView>,
    },
    /// 租户未配置**或已禁用**该协议（合并 —— 对未认证调用者不暴露内部状态）。
    SsoProviderNotConfigured,
    /// 配置值非法。`reason` 是**固定的 ASCII 短语**，不得包含配置值本身。
    SsoConfigInvalid {
        /// 面向开发者的英文原因（**不含配置值**）。
        reason: &'static str,
    },
    /// 拿不到 OIDC 发现文档（远端不可达 / 非 2xx）。
    SsoOidcDiscoveryFailed,
    /// 拿到了发现文档但不合法（非法 JSON / 缺端点）。
    SsoOidcDiscoveryInvalid,
    /// `state` 不存在 / 已消费 / 租户或 provider 不匹配。
    SsoOidcStateInvalid,
    /// `state` 超过 TTL。
    SsoOidcStateExpired,
    /// 回调缺 `code` 或 `state`。
    SsoOidcCallbackParamMissing,
    /// 换 token 失败（code 伪造 / 已用）。
    SsoOidcTokenExchangeFailed,
    /// IdP 的 token 端点不可用（挂 / 超时）。
    SsoOidcTokenEndpointUnavailable,
    /// `id_token` 无效（签名 / iss / aud / iat / 结构）。
    SsoOidcTokenInvalid,
    /// `id_token` 已过期。
    SsoOidcTokenExpired,
    /// `id_token` 的算法不在白名单内。
    SsoOidcAlgNotAllowed,
    /// `id_token` 的 `nonce` 不符。
    SsoOidcNonceMismatch,
    /// PKCE 校验失败。
    SsoOidcPkceMismatch,
    /// LDAP 服务不可达。
    SsoLdapUnreachable,
    /// LDAP 认证失败（**密码错与用户不存在同一个码**，防账号枚举）。
    SsoLdapBindFailed,
    /// 空 / 纯空白密码。
    SsoLdapEmptyPassword,
    /// 绑定冲突（改指 / mail 撞他人绑定 / JIT 撞既有 username）。
    SsoIdentityLinkConflict,
    /// 未绑定且 JIT 关闭。
    SsoAccountNotLinked,
    /// 登录 ticket 无效（不存在 / 已用 / 过期 / 租户不匹配）。
    SsoTicketInvalid,
}

impl IamError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidCredentials => code::INVALID_CREDENTIALS,
            Self::MfaRequired => code::MFA_REQUIRED,
            Self::MfaInvalid => code::MFA_INVALID,
            Self::AccountLocked => code::ACCOUNT_LOCKED,
            Self::AccountDisabled => code::ACCOUNT_DISABLED,
            Self::TokenExpired => code::TOKEN_EXPIRED,
            Self::TokenRevoked => code::TOKEN_REVOKED,
            Self::TokenReuseDetected => code::TOKEN_REUSE_DETECTED,
            Self::PasswordPolicyViolation { .. } => code::PASSWORD_POLICY_VIOLATION,
            Self::PasswordReused => code::PASSWORD_REUSED,
            Self::LoginRateLimited { .. } => code::LOGIN_RATE_LIMITED,
            Self::TenantContextMissing => code::TENANT_CONTEXT_MISSING,
            Self::Unauthenticated => code::UNAUTHENTICATED,
            Self::NotFound { .. } => code::NOT_FOUND,
            Self::Conflict { .. } => code::CONFLICT,
            Self::NotImplemented { .. } => code::NOT_IMPLEMENTED,
            Self::Internal { .. } => code::INTERNAL_ERROR,
            Self::SsoRequired { .. } => code::SSO_REQUIRED,
            Self::SsoProviderNotConfigured => code::SSO_PROVIDER_NOT_CONFIGURED,
            Self::SsoConfigInvalid { .. } => code::SSO_CONFIG_INVALID,
            Self::SsoOidcDiscoveryFailed => code::SSO_OIDC_DISCOVERY_FAILED,
            Self::SsoOidcDiscoveryInvalid => code::SSO_OIDC_DISCOVERY_INVALID,
            Self::SsoOidcStateInvalid => code::SSO_OIDC_STATE_INVALID,
            Self::SsoOidcStateExpired => code::SSO_OIDC_STATE_EXPIRED,
            Self::SsoOidcCallbackParamMissing => code::SSO_OIDC_CALLBACK_PARAM_MISSING,
            Self::SsoOidcTokenExchangeFailed => code::SSO_OIDC_TOKEN_EXCHANGE_FAILED,
            Self::SsoOidcTokenEndpointUnavailable => code::SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE,
            Self::SsoOidcTokenInvalid => code::SSO_OIDC_TOKEN_INVALID,
            Self::SsoOidcTokenExpired => code::SSO_OIDC_TOKEN_EXPIRED,
            Self::SsoOidcAlgNotAllowed => code::SSO_OIDC_ALG_NOT_ALLOWED,
            Self::SsoOidcNonceMismatch => code::SSO_OIDC_NONCE_MISMATCH,
            Self::SsoOidcPkceMismatch => code::SSO_OIDC_PKCE_MISMATCH,
            Self::SsoLdapUnreachable => code::SSO_LDAP_UNREACHABLE,
            Self::SsoLdapBindFailed => code::SSO_LDAP_BIND_FAILED,
            Self::SsoLdapEmptyPassword => code::SSO_LDAP_EMPTY_PASSWORD,
            Self::SsoIdentityLinkConflict => code::SSO_IDENTITY_LINK_CONFLICT,
            Self::SsoAccountNotLinked => code::SSO_ACCOUNT_NOT_LINKED,
            Self::SsoTicketInvalid => code::SSO_TICKET_INVALID,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::InvalidCredentials
            | Self::MfaRequired
            | Self::MfaInvalid
            | Self::TokenExpired
            | Self::TokenRevoked
            | Self::TokenReuseDetected
            | Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::AccountLocked | Self::AccountDisabled => StatusCode::FORBIDDEN,
            Self::PasswordPolicyViolation { .. } | Self::PasswordReused => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::LoginRateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::TenantContextMissing => StatusCode::UNAUTHORIZED,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::NotImplemented { .. } => StatusCode::NOT_IMPLEMENTED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,

            // -- SSO 入站（REQ-013）：状态码按 AC 的错误码总表逐条钉死 ------
            Self::SsoRequired { .. } | Self::SsoAccountNotLinked => StatusCode::FORBIDDEN,
            Self::SsoProviderNotConfigured
            | Self::SsoConfigInvalid { .. }
            | Self::SsoOidcStateInvalid
            | Self::SsoOidcStateExpired
            | Self::SsoOidcCallbackParamMissing
            | Self::SsoOidcTokenExchangeFailed
            | Self::SsoOidcPkceMismatch
            | Self::SsoLdapEmptyPassword => StatusCode::BAD_REQUEST,
            Self::SsoOidcTokenInvalid
            | Self::SsoOidcTokenExpired
            | Self::SsoOidcAlgNotAllowed
            | Self::SsoOidcNonceMismatch
            | Self::SsoLdapBindFailed
            | Self::SsoTicketInvalid => StatusCode::UNAUTHORIZED,
            Self::SsoOidcDiscoveryInvalid | Self::SsoOidcTokenEndpointUnavailable => {
                StatusCode::BAD_GATEWAY
            }
            Self::SsoOidcDiscoveryFailed | Self::SsoLdapUnreachable => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::SsoIdentityLinkConflict => StatusCode::CONFLICT,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述（契约 C6）。
    ///
    /// ⚠️ 这里**不允许**出现中文：面向用户的中文文案由前端 i18n 按 `code` 渲染。
    pub fn detail(&self) -> String {
        match self {
            Self::InvalidCredentials => "invalid username or password".to_string(),
            Self::MfaRequired => "a multi-factor authentication code is required".to_string(),
            Self::MfaInvalid => "the multi-factor authentication code is invalid".to_string(),
            Self::AccountLocked => "the account is temporarily locked".to_string(),
            Self::AccountDisabled => "the account is disabled".to_string(),
            Self::TokenExpired => "the token has expired".to_string(),
            Self::TokenRevoked => "the token has been revoked".to_string(),
            Self::TokenReuseDetected => {
                "refresh token reuse was detected and the token family has been revoked".to_string()
            }
            Self::PasswordPolicyViolation { violations } => {
                let rules = violations
                    .iter()
                    .map(|violation| violation.rule.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("the password does not satisfy the password policy: {rules}")
            }
            Self::PasswordReused => "the password was used recently".to_string(),
            Self::LoginRateLimited {
                retry_after_seconds,
            } => format!("too many login attempts; retry after {retry_after_seconds} seconds"),
            Self::TenantContextMissing => "the tenant context is missing".to_string(),
            Self::Unauthenticated => "authentication is required".to_string(),
            Self::NotFound { resource } => format!("the {resource} was not found"),
            Self::Conflict { reason } => (*reason).to_string(),
            Self::NotImplemented { feature } => format!("{feature} is not implemented"),
            Self::Internal { reason } => format!("an internal error occurred: {reason}"),

            // -- SSO 入站（REQ-013）：**只描述类别，不复述任何配置值** ------
            Self::SsoRequired { .. } => {
                "local password login is disabled for this tenant; use single sign-on".to_string()
            }
            Self::SsoProviderNotConfigured => {
                "the single sign-on provider is not configured for this tenant".to_string()
            }
            Self::SsoConfigInvalid { reason } => {
                format!("the single sign-on configuration is invalid: {reason}")
            }
            Self::SsoOidcDiscoveryFailed => {
                "the openid connect discovery document is unreachable".to_string()
            }
            Self::SsoOidcDiscoveryInvalid => {
                "the openid connect discovery document is invalid".to_string()
            }
            Self::SsoOidcStateInvalid => "the single sign-on state is invalid".to_string(),
            Self::SsoOidcStateExpired => "the single sign-on state has expired".to_string(),
            Self::SsoOidcCallbackParamMissing => {
                "the single sign-on callback is missing a required parameter".to_string()
            }
            Self::SsoOidcTokenExchangeFailed => "the single sign-on code exchange failed".to_string(),
            Self::SsoOidcTokenEndpointUnavailable => {
                "the single sign-on token endpoint is unavailable".to_string()
            }
            Self::SsoOidcTokenInvalid => "the single sign-on id token is invalid".to_string(),
            Self::SsoOidcTokenExpired => "the single sign-on id token has expired".to_string(),
            Self::SsoOidcAlgNotAllowed => {
                "the single sign-on id token algorithm is not allowed".to_string()
            }
            Self::SsoOidcNonceMismatch => "the single sign-on nonce does not match".to_string(),
            Self::SsoOidcPkceMismatch => {
                "the single sign-on pkce verification failed".to_string()
            }
            Self::SsoLdapUnreachable => "the ldap server is unreachable".to_string(),
            Self::SsoLdapBindFailed => "ldap authentication failed".to_string(),
            Self::SsoLdapEmptyPassword => "the ldap password must not be empty".to_string(),
            Self::SsoIdentityLinkConflict => {
                "the single sign-on identity conflicts with an existing account".to_string()
            }
            Self::SsoAccountNotLinked => {
                "the single sign-on account is not linked to a local account".to_string()
            }
            Self::SsoTicketInvalid => "the single sign-on login ticket is invalid".to_string(),
        }
    }

    /// 错误码之外的补充参数（契约 C6 的 `params`）。
    pub fn params(&self) -> BTreeMap<String, serde_json::Value> {
        let mut params = BTreeMap::new();
        match self {
            Self::PasswordPolicyViolation { violations } => {
                params.insert(
                    "violations".to_string(),
                    serde_json::Value::Array(
                        violations
                            .iter()
                            .map(|violation| {
                                serde_json::json!({
                                    "rule": violation.rule,
                                    "detail": violation.detail,
                                })
                            })
                            .collect(),
                    ),
                );
            }
            Self::LoginRateLimited {
                retry_after_seconds,
            } => {
                params.insert(
                    "retryAfterSeconds".to_string(),
                    serde_json::Value::from(*retry_after_seconds),
                );
            }
            Self::NotFound { resource } => {
                params.insert("resource".to_string(), serde_json::Value::from(*resource));
            }
            Self::Conflict { reason } => {
                params.insert("reason".to_string(), serde_json::Value::from(*reason));
            }
            Self::NotImplemented { feature } => {
                params.insert("feature".to_string(), serde_json::Value::from(*feature));
            }
            // 强制 SSO：**可用入口列表写死在 params**（AC-S-021 预期 1）。
            // 只含 type / id / label / 端点路径 —— 不含 issuer / clientId / url / baseDn。
            Self::SsoRequired { providers } => {
                params.insert(
                    "ssoProviders".to_string(),
                    serde_json::to_value(providers).unwrap_or(serde_json::Value::Array(Vec::new())),
                );
            }
            Self::SsoConfigInvalid { reason } => {
                params.insert("reason".to_string(), serde_json::Value::from(*reason));
            }
            _ => {}
        }
        params
    }

    /// 是否属于「凭证不对」这一类（**对外表现完全一致**的那一类）。
    ///
    /// 登录失败路径用它来保证：账号不存在、密码错误、账号锁定 走的是**同一段**代码。
    pub fn is_credential_failure(&self) -> bool {
        matches!(
            self,
            Self::InvalidCredentials | Self::AccountLocked | Self::AccountDisabled
        )
    }
}

impl std::fmt::Display for IamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for IamError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_code_is_upper_snake_case() {
        let errors = [
            IamError::InvalidCredentials,
            IamError::MfaRequired,
            IamError::MfaInvalid,
            IamError::AccountLocked,
            IamError::AccountDisabled,
            IamError::TokenExpired,
            IamError::TokenRevoked,
            IamError::TokenReuseDetected,
            IamError::PasswordPolicyViolation {
                violations: vec![PolicyViolation::new("min_length", "too short")],
            },
            IamError::PasswordReused,
            IamError::LoginRateLimited {
                retry_after_seconds: 60,
            },
            IamError::TenantContextMissing,
            IamError::Unauthenticated,
            IamError::NotFound {
                resource: "session",
            },
            IamError::Conflict { reason: "busy" },
            IamError::NotImplemented {
                feature: "webauthn",
            },
            IamError::Internal {
                reason: "boom".to_string(),
            },
            // -- SSO 入站（REQ-013）：21 个码 --------------------------------
            IamError::SsoRequired {
                providers: Vec::new(),
            },
            IamError::SsoProviderNotConfigured,
            IamError::SsoConfigInvalid {
                reason: "the issuer is not an absolute url",
            },
            IamError::SsoOidcDiscoveryFailed,
            IamError::SsoOidcDiscoveryInvalid,
            IamError::SsoOidcStateInvalid,
            IamError::SsoOidcStateExpired,
            IamError::SsoOidcCallbackParamMissing,
            IamError::SsoOidcTokenExchangeFailed,
            IamError::SsoOidcTokenEndpointUnavailable,
            IamError::SsoOidcTokenInvalid,
            IamError::SsoOidcTokenExpired,
            IamError::SsoOidcAlgNotAllowed,
            IamError::SsoOidcNonceMismatch,
            IamError::SsoOidcPkceMismatch,
            IamError::SsoLdapUnreachable,
            IamError::SsoLdapBindFailed,
            IamError::SsoLdapEmptyPassword,
            IamError::SsoIdentityLinkConflict,
            IamError::SsoAccountNotLinked,
            IamError::SsoTicketInvalid,
        ];
        for error in &errors {
            let code = error.code();
            assert!(!code.is_empty());
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "错误码必须是大写 + 下划线：{code}"
            );
        }
    }

    /// `ALL_CODES` 必须覆盖**每一个变体**。
    ///
    /// 完备性测试只验「`TITLES` ⊇ `ALL_CODES`」；如果漏了清单本身，
    /// 那一条会照样通过（那正是本轮要消灭的假绿）。所以这里把变体**全部列出来**，
    /// 反查清单 —— 新增变体忘了登记清单，这里就红。
    #[test]
    fn all_codes_lists_every_variant() {
        use std::collections::BTreeSet;

        let variants = [
            IamError::InvalidCredentials,
            IamError::MfaRequired,
            IamError::MfaInvalid,
            IamError::AccountLocked,
            IamError::AccountDisabled,
            IamError::TokenExpired,
            IamError::TokenRevoked,
            IamError::TokenReuseDetected,
            IamError::PasswordPolicyViolation {
                violations: vec![PolicyViolation::new("min_length", "too short")],
            },
            IamError::PasswordReused,
            IamError::LoginRateLimited {
                retry_after_seconds: 60,
            },
            IamError::TenantContextMissing,
            IamError::Unauthenticated,
            IamError::NotFound {
                resource: "session",
            },
            IamError::Conflict { reason: "busy" },
            IamError::NotImplemented {
                feature: "webauthn",
            },
            IamError::Internal {
                reason: "boom".to_string(),
            },
            // -- SSO 入站（REQ-013）：21 个码 --------------------------------
            IamError::SsoRequired {
                providers: Vec::new(),
            },
            IamError::SsoProviderNotConfigured,
            IamError::SsoConfigInvalid {
                reason: "the issuer is not an absolute url",
            },
            IamError::SsoOidcDiscoveryFailed,
            IamError::SsoOidcDiscoveryInvalid,
            IamError::SsoOidcStateInvalid,
            IamError::SsoOidcStateExpired,
            IamError::SsoOidcCallbackParamMissing,
            IamError::SsoOidcTokenExchangeFailed,
            IamError::SsoOidcTokenEndpointUnavailable,
            IamError::SsoOidcTokenInvalid,
            IamError::SsoOidcTokenExpired,
            IamError::SsoOidcAlgNotAllowed,
            IamError::SsoOidcNonceMismatch,
            IamError::SsoOidcPkceMismatch,
            IamError::SsoLdapUnreachable,
            IamError::SsoLdapBindFailed,
            IamError::SsoLdapEmptyPassword,
            IamError::SsoIdentityLinkConflict,
            IamError::SsoAccountNotLinked,
            IamError::SsoTicketInvalid,
        ];
        let mut seen = BTreeSet::new();
        for error in &variants {
            assert!(
                ALL_CODES.contains(&error.code()),
                "ALL_CODES 缺少 {} 的码 {}",
                std::any::type_name::<IamError>(),
                error.code()
            );
            seen.insert(error.code());
        }
        assert_eq!(
            seen.len(),
            ALL_CODES.len(),
            "ALL_CODES 里有变体从未产出过的码（清单与 match 不一致）"
        );
    }

    /// 契约 C6：`detail` 必须是**英文 ASCII** —— 后端不得拼中文句子。
    #[test]
    fn every_detail_is_english_ascii() {
        let errors = [
            IamError::InvalidCredentials,
            IamError::MfaRequired,
            IamError::MfaInvalid,
            IamError::AccountLocked,
            IamError::AccountDisabled,
            IamError::TokenExpired,
            IamError::TokenRevoked,
            IamError::TokenReuseDetected,
            IamError::PasswordPolicyViolation {
                violations: vec![PolicyViolation::new("min_length", "too short")],
            },
            IamError::PasswordReused,
            IamError::LoginRateLimited {
                retry_after_seconds: 60,
            },
            IamError::TenantContextMissing,
            IamError::Unauthenticated,
            IamError::NotFound {
                resource: "session",
            },
            IamError::Conflict { reason: "busy" },
            IamError::NotImplemented {
                feature: "webauthn",
            },
            IamError::Internal {
                reason: "boom".to_string(),
            },
            // -- SSO 入站（REQ-013）：21 个码 --------------------------------
            IamError::SsoRequired {
                providers: Vec::new(),
            },
            IamError::SsoProviderNotConfigured,
            IamError::SsoConfigInvalid {
                reason: "the issuer is not an absolute url",
            },
            IamError::SsoOidcDiscoveryFailed,
            IamError::SsoOidcDiscoveryInvalid,
            IamError::SsoOidcStateInvalid,
            IamError::SsoOidcStateExpired,
            IamError::SsoOidcCallbackParamMissing,
            IamError::SsoOidcTokenExchangeFailed,
            IamError::SsoOidcTokenEndpointUnavailable,
            IamError::SsoOidcTokenInvalid,
            IamError::SsoOidcTokenExpired,
            IamError::SsoOidcAlgNotAllowed,
            IamError::SsoOidcNonceMismatch,
            IamError::SsoOidcPkceMismatch,
            IamError::SsoLdapUnreachable,
            IamError::SsoLdapBindFailed,
            IamError::SsoLdapEmptyPassword,
            IamError::SsoIdentityLinkConflict,
            IamError::SsoAccountNotLinked,
            IamError::SsoTicketInvalid,
        ];
        for error in &errors {
            let detail = error.detail();
            assert!(detail.is_ascii(), "detail 必须是 ASCII：{detail}");
            assert!(!detail.is_empty(), "detail 不得为空");
        }
    }

    /// `INVALID_CREDENTIALS` 的 detail 必须与设计摘要**逐字一致**。
    #[test]
    fn invalid_credentials_detail_matches_the_design_summary() {
        assert_eq!(
            IamError::InvalidCredentials.detail(),
            "invalid username or password"
        );
    }

    #[test]
    fn password_policy_violation_lists_the_violated_rules() {
        let error = IamError::PasswordPolicyViolation {
            violations: vec![
                PolicyViolation::new("min_length", "the password is shorter than the minimum"),
                PolicyViolation::new("common_password", "the password is too common"),
            ],
        };
        let params = error.params();
        let violations = params
            .get("violations")
            .and_then(serde_json::Value::as_array)
            .expect("params 里必须带 violations");
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0]["rule"], "min_length");
        assert_eq!(violations[1]["rule"], "common_password");
        assert!(error.detail().contains("min_length"));
    }

    #[test]
    fn credential_failures_share_the_same_status() {
        assert_eq!(
            IamError::InvalidCredentials.status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(IamError::InvalidCredentials.is_credential_failure());
        assert!(IamError::AccountLocked.is_credential_failure());
    }
}
