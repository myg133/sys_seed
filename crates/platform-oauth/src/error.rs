//! 出站 IdP 的**平台错误码**（REQ-022，D213 / D226）。
//!
//! # 归属
//!
//! 与 `platform-audit` 同款：码的**常量**住在本 crate（业务 crate 风格），
//! 而**标题**的唯一来源是 `platform_core::titles::TITLES`（D113）——
//! 面向用户的中文文案由前端 i18n 按 `code` 渲染（契约 C6）。
//!
//! 本模块提供：
//!
//! - 9 个**平台错误码**的常量与枚举（`OAUTH_*`，大写下划线）；
//! - 本 crate 的**完备性清单** [`ALL_CODES`]（`platform-server` 的
//!   `titles_cover_every_code` 会汇总它 → `TITLES` 必须 ⊇ 本清单）；
//! - 把 [`OAuthError`] 转成统一错误体（RFC 9457）。
//!
//! # ⚠️ 与**协议错误码**是两套东西（最容易搞混的一处）
//!
//! | 类别 | 端点 | 错误体 |
//! |------|------|--------|
//! | **平台错误**（本模块） | `/api/oauth/authorize/*`、`/clients/*`、`/grants/*`、`/logout`、`/device/*` | RFC 9457 Problem Details + `OAUTH_*` |
//! | **协议错误**（[`crate::protocol`]） | `/api/oauth/token`、`/introspect`、`/revoke`、`/device_authorization` | `{"error":"invalid_grant"}` 小写 |
//!
//! **两套不得混用**（契约 C14 §14.2）。

use std::collections::BTreeMap;

use axum::http::StatusCode;
use platform_core::RequestContext;
use platform_core::problem::ProblemDetails;

/// 稳定的机器可读错误码（平台侧，`OAUTH_*`）。
pub mod code {
    /// 查不到 OAuth 客户端。
    pub const CLIENT_NOT_FOUND: &str = "OAUTH_CLIENT_NOT_FOUND";
    /// OAuth 客户端已停用。
    pub const CLIENT_DISABLED: &str = "OAUTH_CLIENT_DISABLED";
    /// OAuth 客户端已过期。
    pub const CLIENT_EXPIRED: &str = "OAUTH_CLIENT_EXPIRED";
    /// 请求参数缺失或非法（含缺 `code_challenge`）。
    pub const INVALID_REQUEST: &str = "OAUTH_INVALID_REQUEST";
    /// 回调地址不在白名单内。
    pub const INVALID_REDIRECT_URI: &str = "OAUTH_INVALID_REDIRECT_URI";
    /// scope 为空 / 格式非法 / 通配未注册 / 超出白名单。
    pub const INVALID_SCOPE: &str = "OAUTH_INVALID_SCOPE";
    /// `code_challenge_method` 不是 `S256`（`plain` 不支持）。
    pub const UNSUPPORTED_CODE_CHALLENGE_METHOD: &str = "OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD";
    /// consent ticket 验签失败 / 过期 / 被篡改。
    pub const TICKET_INVALID: &str = "OAUTH_TICKET_INVALID";
    /// 授权记录不存在。
    pub const GRANT_NOT_FOUND: &str = "OAUTH_GRANT_NOT_FOUND";
}

/// 本 crate 的全部**平台**错误码（**完备性测试的输入**，D113）。
///
/// ⚠️ 这里**只有 9 个**：8 个 `oauth.*` 审计事件码**不进**本清单
/// （事件码不进 `TITLES` —— D223），协议码也不进（它们不是平台错误码）。
pub const ALL_CODES: &[&str] = &[
    code::CLIENT_NOT_FOUND,
    code::CLIENT_DISABLED,
    code::CLIENT_EXPIRED,
    code::INVALID_REQUEST,
    code::INVALID_REDIRECT_URI,
    code::INVALID_SCOPE,
    code::UNSUPPORTED_CODE_CHALLENGE_METHOD,
    code::TICKET_INVALID,
    code::GRANT_NOT_FOUND,
];

/// 出站 IdP 的平台错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthError {
    /// 查不到 OAuth 客户端。
    ClientNotFound,
    /// OAuth 客户端已停用。
    ClientDisabled,
    /// OAuth 客户端已过期。
    ClientExpired,
    /// 请求参数缺失或非法。
    ///
    /// `reason` 是**固定的 ASCII 短语**，**不得**含任何请求值
    /// （`code_verifier` / `code_challenge` / `client_secret` 的明文一律不得出现）。
    InvalidRequest {
        /// 面向开发者的英文原因。
        reason: &'static str,
    },
    /// 回调地址不在白名单内。
    InvalidRedirectUri,
    /// scope 非法。
    InvalidScope,
    /// `code_challenge_method` 不支持。
    UnsupportedCodeChallengeMethod,
    /// consent ticket 无效。
    TicketInvalid,
    /// 授权记录不存在。
    GrantNotFound,
}

impl OAuthError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::ClientNotFound => code::CLIENT_NOT_FOUND,
            Self::ClientDisabled => code::CLIENT_DISABLED,
            Self::ClientExpired => code::CLIENT_EXPIRED,
            Self::InvalidRequest { .. } => code::INVALID_REQUEST,
            Self::InvalidRedirectUri => code::INVALID_REDIRECT_URI,
            Self::InvalidScope => code::INVALID_SCOPE,
            Self::UnsupportedCodeChallengeMethod => code::UNSUPPORTED_CODE_CHALLENGE_METHOD,
            Self::TicketInvalid => code::TICKET_INVALID,
            Self::GrantNotFound => code::GRANT_NOT_FOUND,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::ClientNotFound | Self::GrantNotFound => StatusCode::NOT_FOUND,
            Self::ClientDisabled | Self::ClientExpired => StatusCode::FORBIDDEN,
            Self::InvalidRequest { .. }
            | Self::InvalidRedirectUri
            | Self::InvalidScope
            | Self::UnsupportedCodeChallengeMethod
            | Self::TicketInvalid => StatusCode::BAD_REQUEST,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述（契约 C6）。
    ///
    /// ⚠️ 这里**不允许**出现中文：面向用户的中文文案由前端 i18n 按 `code` 渲染。
    /// ⚠️ 也不得回显任何**凭据明文**（D251）。
    pub fn detail(&self) -> String {
        match self {
            Self::ClientNotFound => "the oauth client was not found".to_string(),
            Self::ClientDisabled => "the oauth client is disabled".to_string(),
            Self::ClientExpired => "the oauth client has expired".to_string(),
            Self::InvalidRequest { reason } => {
                format!("the oauth request is invalid: {reason}")
            }
            Self::InvalidRedirectUri => {
                "the redirect uri is not registered for this oauth client".to_string()
            }
            Self::InvalidScope => "the requested scope is invalid".to_string(),
            Self::UnsupportedCodeChallengeMethod => {
                "the code challenge method is not supported".to_string()
            }
            Self::TicketInvalid => "the oauth consent ticket is invalid".to_string(),
            Self::GrantNotFound => "the oauth grant was not found".to_string(),
        }
    }

    /// 错误码之外的补充参数（契约 C6 的 `params`）。
    pub fn params(&self) -> BTreeMap<String, serde_json::Value> {
        let mut params = BTreeMap::new();
        if let Self::InvalidRequest { reason } = self {
            params.insert("reason".to_string(), serde_json::Value::from(*reason));
        }
        params
    }

    /// 把错误转成统一错误体（RFC 9457，契约 C6）。
    pub fn problem_details(&self, ctx: &RequestContext, instance: &str) -> ProblemDetails {
        let params = self.params();
        ProblemDetails::from_context(
            self.code(),
            platform_core::titles::title_of_code(self.code()),
            self.status().as_u16(),
            self.detail(),
            ctx,
            instance,
            if params.is_empty() {
                None
            } else {
                Some(params)
            },
        )
    }
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for OAuthError {}

/// 服务层的错误：**OAuth 专有错误** + **平台通用错误**。
///
/// # 为什么要分开
///
/// `OAUTH_*` **恰好 9 个**（D226）—— 那是 OAuth **专有**的失败。
/// 「没带令牌」「数据库挂了」这类失败**不是** OAuth 专有的 →
/// 复用平台**既有**的 `UNAUTHENTICATED` / `INTERNAL_ERROR`
/// （它们早已在 `TITLES` 与 i18n 词库里登记，D125）。
///
/// ⚠️ **不得**为它们新增第 10 个 `OAUTH_*` 码 —— 死码就是「永远不可能出现
/// 却要占一行标题、一条词库」的负担（D226 的原话）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceError {
    /// OAuth 专有错误（9 个 `OAUTH_*` 码之一）。
    OAuth(OAuthError),
    /// 需要认证（缺令牌 / 令牌无效）→ **401 `UNAUTHENTICATED`**。
    Unauthenticated,
    /// 内部错误 → **500 `INTERNAL_ERROR`**（**不把内部细节漏给调用方**）。
    Internal {
        /// 面向开发者的英文原因。
        reason: String,
    },
}

impl From<OAuthError> for ServiceError {
    fn from(error: OAuthError) -> Self {
        Self::OAuth(error)
    }
}

impl ServiceError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::OAuth(error) => error.code(),
            Self::Unauthenticated => platform_core::problem::CODE_UNAUTHENTICATED,
            Self::Internal { .. } => platform_core::problem::CODE_INTERNAL_ERROR,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::OAuth(error) => error.status(),
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述。
    pub fn detail(&self) -> String {
        match self {
            Self::OAuth(error) => error.detail(),
            Self::Unauthenticated => "authentication is required".to_string(),
            Self::Internal { reason } => format!("an internal error occurred: {reason}"),
        }
    }

    /// 把错误转成统一错误体（RFC 9457，契约 C6）。
    pub fn problem_details(&self, ctx: &RequestContext, instance: &str) -> ProblemDetails {
        match self {
            Self::OAuth(error) => error.problem_details(ctx, instance),
            Self::Unauthenticated => ProblemDetails::unauthenticated(ctx, instance),
            Self::Internal { .. } => ProblemDetails::from_context(
                platform_core::problem::CODE_INTERNAL_ERROR,
                platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR),
                500,
                self.detail(),
                ctx,
                instance,
                None,
            ),
        }
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for ServiceError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的**全部变体**（D213 第 8 处：测试的两个数组之一）。
    fn all_variants() -> Vec<OAuthError> {
        vec![
            OAuthError::ClientNotFound,
            OAuthError::ClientDisabled,
            OAuthError::ClientExpired,
            OAuthError::InvalidRequest {
                reason: "the code challenge is missing",
            },
            OAuthError::InvalidRedirectUri,
            OAuthError::InvalidScope,
            OAuthError::UnsupportedCodeChallengeMethod,
            OAuthError::TicketInvalid,
            OAuthError::GrantNotFound,
        ]
    }

    #[test]
    fn the_code_list_has_exactly_nine_entries() {
        assert_eq!(ALL_CODES.len(), 9);
    }

    /// `ALL_CODES` 必须覆盖**每一个变体**（漏一个变体 → 变红）。
    #[test]
    fn all_codes_lists_every_variant() {
        use std::collections::BTreeSet;

        let variants = all_variants();
        let mut seen = BTreeSet::new();
        for error in &variants {
            assert!(
                ALL_CODES.contains(&error.code()),
                "ALL_CODES 缺少 {} 的码 {}",
                std::any::type_name::<OAuthError>(),
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

    /// 错误码必须是**大写 + 下划线**（i18n 的 `error.<CODE>` 键名）。
    #[test]
    fn every_error_code_is_upper_snake_case() {
        for error in all_variants() {
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
        for error in all_variants() {
            let detail = error.detail();
            assert!(detail.is_ascii(), "detail 必须是 ASCII：{detail}");
            assert!(!detail.is_empty(), "detail 不得为空");
        }
    }

    /// `detail` **不得**回显凭据明文（D251）。
    #[test]
    fn no_detail_echoes_a_credential() {
        for error in all_variants() {
            let detail = error.detail();
            for forbidden in [
                "code_verifier",
                "code_challenge",
                "client_secret",
                "secret",
                "token",
            ] {
                assert!(
                    !detail.contains(forbidden),
                    "detail 不得回显 `{forbidden}`：{detail}"
                );
            }
        }
    }

    /// 每个码都已在 `TITLES` 登记（D125）→ 否则前端会看到裸码。
    #[test]
    fn every_code_is_registered_in_titles() {
        for code in ALL_CODES {
            assert!(
                platform_core::titles::is_registered(code),
                "`{code}` 必须在 TITLES 里登记（D125）"
            );
        }
    }

    /// 状态码逐个钉死（AC-S-135 / AC-S-138 / AC-S-146）。
    #[test]
    fn the_status_codes_are_pinned() {
        assert_eq!(OAuthError::ClientNotFound.status(), StatusCode::NOT_FOUND);
        assert_eq!(OAuthError::GrantNotFound.status(), StatusCode::NOT_FOUND);
        assert_eq!(OAuthError::ClientDisabled.status(), StatusCode::FORBIDDEN);
        assert_eq!(OAuthError::ClientExpired.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            OAuthError::InvalidRequest {
                reason: "the code challenge is missing"
            }
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            OAuthError::InvalidRedirectUri.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(OAuthError::InvalidScope.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            OAuthError::UnsupportedCodeChallengeMethod.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(OAuthError::TicketInvalid.status(), StatusCode::BAD_REQUEST);
    }
}
