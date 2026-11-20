//! OAuth **协议错误码**（小写；契约 C14 §14.2）。
//!
//! # 与平台错误码**是两套东西**
//!
//! 协议端点（`/api/oauth/token`、`/introspect`、`/revoke`、`/device_authorization`）
//! 按 RFC 说 RFC 的话：错误体**逐字**是
//!
//! ```json
//! {"error": "invalid_grant"}
//! ```
//!
//! 或带描述：
//!
//! ```json
//! {"error": "invalid_grant", "error_description": "the authorization code is invalid"}
//! ```
//!
//! **不得**含 `type` / `title` / `status` / `instance` / `code` / `trace_id`
//! —— 那是 RFC 9457 Problem Details 的字段，协议端点**不用**它（D244）。
//!
//! # 取值集合是**封闭**的
//!
//! 契约 C14 §14.2 登记了 14 个码，**没有** `invalid_token`
//! （它来自 RFC 6750，本契约未登记）→ **不得使用**（D224）。
//!
//! # `error_description` 的两条硬约束（D251）
//!
//! 1. **不得**回显 `code_verifier` / `code_challenge` / `client_secret` / `code`
//!    的**明文**；
//! 2. **不得**含任何 **CJK 字符**（面向开发者，英文 ASCII）。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// 协议错误码（**小写下划线**，契约 C14 §14.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    /// 请求缺参数 / 格式非法。
    InvalidRequest,
    /// client 认证失败（**不区分**「不存在」与「secret 错」）。
    InvalidClient,
    /// 授权码 / 设备码 / 刷新凭据无效或已过期。
    InvalidGrant,
    /// client 不允许用这个 grant type。
    UnauthorizedClient,
    /// 不认识的 `grant_type`。
    UnsupportedGrantType,
    /// 不支持的 `response_type`。
    UnsupportedResponseType,
    /// scope 非法或超出白名单。
    InvalidScope,
    /// 用户拒绝了授权。
    AccessDenied,
    /// 需要重新登录。
    LoginRequired,
    /// 需要用户同意。
    ConsentRequired,
    /// 设备码尚未被用户授权。
    AuthorizationPending,
    /// 轮询过快。
    SlowDown,
    /// 令牌 / 设备码已过期。
    ExpiredToken,
    /// 服务内部错误。
    ServerError,
}

impl ProtocolError {
    /// 协议错误码的**字符串**形态（进错误体的 `error` 键）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::UnauthorizedClient => "unauthorized_client",
            Self::UnsupportedGrantType => "unsupported_grant_type",
            Self::UnsupportedResponseType => "unsupported_response_type",
            Self::InvalidScope => "invalid_scope",
            Self::AccessDenied => "access_denied",
            Self::LoginRequired => "login_required",
            Self::ConsentRequired => "consent_required",
            Self::AuthorizationPending => "authorization_pending",
            Self::SlowDown => "slow_down",
            Self::ExpiredToken => "expired_token",
            Self::ServerError => "server_error",
        }
    }

    /// 全部协议错误码（**完备性测试的输入**）。
    pub const ALL_CODES: [&'static str; 14] = [
        "invalid_request",
        "invalid_client",
        "invalid_grant",
        "unauthorized_client",
        "unsupported_grant_type",
        "unsupported_response_type",
        "invalid_scope",
        "access_denied",
        "login_required",
        "consent_required",
        "authorization_pending",
        "slow_down",
        "expired_token",
        "server_error",
    ];

    /// HTTP 状态码。
    ///
    /// ⚠️ 认证失败一律 **401**（`invalid_client`）；其余一律 **400**。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::InvalidClient => StatusCode::UNAUTHORIZED,
            Self::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述。
    ///
    /// ⚠️ **不得**含任何凭据明文（D251），**不得**含 CJK。
    pub fn description(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "the request is missing a required parameter",
            Self::InvalidClient => "the client authentication failed",
            Self::InvalidGrant => "the grant is invalid",
            Self::UnauthorizedClient => "the client is not allowed to use this grant type",
            Self::UnsupportedGrantType => "the grant type is not supported",
            Self::UnsupportedResponseType => "the response type is not supported",
            Self::InvalidScope => "the requested scope is invalid",
            Self::AccessDenied => "the resource owner denied the request",
            Self::LoginRequired => "the end user is not authenticated",
            Self::ConsentRequired => "the end user has not granted the requested scope",
            Self::AuthorizationPending => "the device authorization is pending",
            Self::SlowDown => "the client is polling too frequently",
            Self::ExpiredToken => "the grant has expired",
            Self::ServerError => "the authorization server encountered an unexpected condition",
        }
    }

    /// 错误体（**逐字**是 `{"error": "…"}` / `{"error": "…", "error_description": "…"}`）。
    pub fn body(&self) -> ProtocolErrorBody {
        ProtocolErrorBody {
            error: self.code(),
            error_description: Some(self.description().to_string()),
        }
    }

    /// 错误体（**只有** `error` 键 —— AC-S-143 步骤 5/6/7 要求**逐字相等**）。
    ///
    /// 用于「认证失败」这一类：**不得**附带描述 —— AC 断言响应体逐字等于
    /// `{"error":"invalid_client"}`，多一个键就不是逐字相等了。
    pub fn bare_body(&self) -> ProtocolErrorBody {
        ProtocolErrorBody {
            error: self.code(),
            error_description: None,
        }
    }

    /// 变成 HTTP 响应。
    ///
    /// ⚠️ 状态码由 [`ProtocolError::status`] 决定（`invalid_client` → **401**、
    /// `server_error` → **500**、其余 → **400**）—— **不是**一律 400。
    pub fn response(&self) -> Response {
        (self.status(), axum::Json(self.body())).into_response()
    }

    /// 变成 HTTP 响应（**只有** `error` 键）。
    pub fn bare_response(&self) -> Response {
        (self.status(), axum::Json(self.bare_body())).into_response()
    }
}

/// 协议端点的错误体。
///
/// ⚠️ **只有** `error` 与 `error_description` 两个字段 —— 不得含
/// `type` / `title` / `status` / `instance` / `code` / `trace_id`（D244）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProtocolErrorBody {
    /// 协议错误码（小写）。
    pub error: &'static str,
    /// 面向开发者的英文描述（**可缺省** —— 认证失败时逐字只留 `error`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

impl IntoResponse for ProtocolError {
    fn into_response(self) -> Response {
        self.response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_set_has_exactly_fourteen_entries() {
        assert_eq!(ProtocolError::ALL_CODES.len(), 14);
    }

    /// 集合里**没有** `invalid_token`（D224）—— 用了它就是契约违例。
    #[test]
    fn invalid_token_is_not_in_the_code_set() {
        assert!(!ProtocolError::ALL_CODES.contains(&"invalid_token"));
        for error in [
            ProtocolError::InvalidRequest,
            ProtocolError::InvalidClient,
            ProtocolError::InvalidGrant,
            ProtocolError::UnauthorizedClient,
            ProtocolError::UnsupportedGrantType,
            ProtocolError::UnsupportedResponseType,
            ProtocolError::InvalidScope,
            ProtocolError::AccessDenied,
            ProtocolError::LoginRequired,
            ProtocolError::ConsentRequired,
            ProtocolError::AuthorizationPending,
            ProtocolError::SlowDown,
            ProtocolError::ExpiredToken,
            ProtocolError::ServerError,
        ] {
            assert_ne!(error.code(), "invalid_token");
        }
    }

    /// 每个码都在封闭集合里（不能自创）。
    #[test]
    fn every_variant_produces_a_registered_code() {
        let all = [
            ProtocolError::InvalidRequest,
            ProtocolError::InvalidClient,
            ProtocolError::InvalidGrant,
            ProtocolError::UnauthorizedClient,
            ProtocolError::UnsupportedGrantType,
            ProtocolError::UnsupportedResponseType,
            ProtocolError::InvalidScope,
            ProtocolError::AccessDenied,
            ProtocolError::LoginRequired,
            ProtocolError::ConsentRequired,
            ProtocolError::AuthorizationPending,
            ProtocolError::SlowDown,
            ProtocolError::ExpiredToken,
            ProtocolError::ServerError,
        ];
        for error in all {
            assert!(
                ProtocolError::ALL_CODES.contains(&error.code()),
                "`{}` 不在契约登记的协议码集合里",
                error.code()
            );
        }
    }

    /// 协议码是**小写**（与平台码的大写下划线是两套）。
    #[test]
    fn every_code_is_lowercase() {
        for code in ProtocolError::ALL_CODES {
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "协议码必须是小写 + 下划线：{code}"
            );
        }
    }

    /// `error_description` 必须是英文 ASCII，且**不得**回显凭据（D251）。
    #[test]
    fn every_description_is_ascii_and_credential_free() {
        for error in [
            ProtocolError::InvalidRequest,
            ProtocolError::InvalidClient,
            ProtocolError::InvalidGrant,
            ProtocolError::UnauthorizedClient,
            ProtocolError::UnsupportedGrantType,
            ProtocolError::UnsupportedResponseType,
            ProtocolError::InvalidScope,
            ProtocolError::AccessDenied,
            ProtocolError::LoginRequired,
            ProtocolError::ConsentRequired,
            ProtocolError::AuthorizationPending,
            ProtocolError::SlowDown,
            ProtocolError::ExpiredToken,
            ProtocolError::ServerError,
        ] {
            let description = error.description();
            assert!(description.is_ascii(), "描述必须是 ASCII：{description}");
            for forbidden in ["code_verifier", "code_challenge", "client_secret", "secret"] {
                assert!(
                    !description.contains(forbidden),
                    "描述不得回显 `{forbidden}`：{description}"
                );
            }
        }
    }

    /// 认证失败 → **401**；其余 → **400**。
    #[test]
    fn the_status_codes_are_pinned() {
        assert_eq!(
            ProtocolError::InvalidClient.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            ProtocolError::InvalidGrant.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ProtocolError::InvalidScope.status(),
            StatusCode::BAD_REQUEST
        );
    }
}
