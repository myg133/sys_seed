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
//! 否则就成了账号枚举的接口。区分「锁定的账号」与「密码错误」是**允许**的
//! —— 账号已存在且被锁定这件事本身不是秘密（且前端需要它提示用户）。
//!
//! 注意：锁定判定发生在**账号存在**的前提下，所以它与「密码错误」的错误码不同，
//! 但**不构成账号枚举**——未注册账号永远不会拿到 `ACCOUNT_LOCKED`。

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
}

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
