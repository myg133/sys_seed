//! 出站 IdP 与单点登出（REQ-022）。
//!
//! # 这个 crate 解决什么
//!
//! 底座**作为 OAuth2 授权服务器（IdP 侧）**向第三方应用签发令牌，
//! 并支持 RFC 8414 / RFC 7662 / RFC 7009 / RFC 8628 四个协议面，
//! 以及 OIDC 的 **back-channel / front-channel 单点登出**。
//!
//! ```text
//! platform-core   ← platform-oauth ← platform-server（装配层）
//!       ↑               ↑
//!   platform-db   platform-iam
//! ```
//!
//! # 两套错误标识（**最容易搞混的一处**）
//!
//! | 类别 | 端点 | 错误体 |
//! |------|------|--------|
//! | **协议错误** | `/api/oauth/token` `/introspect` `/revoke` `/device_authorization` | `{"error":"invalid_grant"}` 小写 |
//! | **平台错误** | `/api/oauth/authorize/*` `/clients/*` `/grants/*` `/logout` `/device/*` | RFC 9457 Problem Details + `OAUTH_*` 大写 |
//!
//! **两套不得混用**（契约 C14 §14.2）。
//!
//! # 依赖方向（单向，无环）
//!
//! `platform-oauth` 依赖 `platform-core`（trait / 错误体 / 上下文）、
//! `platform-db`（租户作用域数据访问）与 `platform-iam`（密钥 + 用户会话校验）；
//! **不依赖** `platform-rbac` —— 「用户有哪些有效权限点」通过
//! `platform-core` 的 `PermissionProvider` trait 拿，由**装配层**注入。

pub mod authz;
pub mod client_auth;
pub mod config;
pub mod crypto;
pub mod error;
pub mod events;
pub mod http;
pub mod keys;
pub mod logout;
pub mod model;
pub mod outbound;
pub mod pkce;
pub mod protocol;
pub mod sensitive;
pub mod service;
pub mod store;
pub mod ticket;

pub use error::{ALL_CODES, OAuthError};
pub use events::ALL_CODES as ALL_EVENT_CODES;
pub use protocol::{ProtocolError, ProtocolErrorBody};

/// 出站 IdP 的版本号（与 `Cargo.toml` 中的 `version` 保持一致）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_exposed() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn the_error_code_list_is_exported() {
        assert_eq!(ALL_CODES.len(), 9);
    }
}
