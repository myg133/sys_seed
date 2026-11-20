//! 出站 IdP 的**鉴权薄封装**（REQ-022，D214 / D216）。
//!
//! # 为什么封装在本 crate 里，而不是写在 `platform-server`
//!
//! 契约检查 C-5 的入口 `check_c5_for(manifest_path, source_dir)`
//! **只扫「该 manifest 所属 crate 自己的 `src/`」**
//! （`crates/platform-contracts/src/checks.rs` 的 `check_c5_for`）。
//! 本需求的 manifest 在 `platform-oauth`、HTTP 在**同一个 crate** 里 →
//! 调用点与权限点编码**字面量**同处一地，C-5 扫得到。
//!
//! 同时**不**引入 `platform-oauth → platform-rbac`（鉴权入口在 **core**）。
//!
//! # ⚠️ 签名说明（与 design §15.2 的示例不同，是有意的）
//!
//! `platform_core::extension::permission::require_permission` 是**路由层**构造器
//! （返回 `PermissionLayer`，供 `.route_layer(...)` 用），**没有**
//! 「传 ctx 直接判定」的形态 —— 写不成 design 的示例。
//!
//! 本封装按 core 的**真实 API** 返回 `PermissionLayer`，
//! 与既有惯例一致（`crates/platform-rbac/src/http.rs` 的
//! `.route_layer(require_permission("rbac:role:read"))`）。
//!
//! # ⚠️ 命名禁区（D216）
//!
//! `crates/platform-oauth/src/` 里**不得**出现这 8 个标识符：
//!
//! ```text
//! check_permission   check_perm     has_permission   require_perm
//! authorize          ensure_permission  verify_permission  permission_check
//! ```
//!
//! 令牌校验的函数因此叫 [`crate::client_auth::verify_client_secret`] 等，
//! **不叫** `authorize_*`。

use platform_core::extension::permission::{PermissionLayer, require_permission};

/// 要求 `oauth:client:read`（查看 OAuth 客户端）。
pub fn require_client_read() -> PermissionLayer {
    require_permission("oauth:client:read")
}

/// 要求 `oauth:client:write`（注册 / 修改 OAuth 客户端）。
pub fn require_client_write() -> PermissionLayer {
    require_permission("oauth:client:write")
}

/// 要求 `oauth:client:delete`（删除 OAuth 客户端）。
pub fn require_client_delete() -> PermissionLayer {
    require_permission("oauth:client:delete")
}

/// 要求 `oauth:grant:read`（查看授权记录）。
pub fn require_grant_read() -> PermissionLayer {
    require_permission("oauth:grant:read")
}

/// 要求 `oauth:grant:revoke`（撤销授权）。
pub fn require_grant_revoke() -> PermissionLayer {
    require_permission("oauth:grant:revoke")
}

/// 要求 `oauth:token:introspect`（管理视角的令牌内省）。
///
/// ⚠️ **客户端自助内省**（`POST /api/oauth/introspect`）走的是 **client 认证**，
/// **不走**这个权限点 —— D214 的口径。
pub fn require_token_introspect() -> PermissionLayer {
    require_permission("oauth:token:introspect")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wrapper_binds_its_permission_literal() {
        assert_eq!(require_client_read().permission(), "oauth:client:read");
        assert_eq!(require_client_write().permission(), "oauth:client:write");
        assert_eq!(require_client_delete().permission(), "oauth:client:delete");
        assert_eq!(require_grant_read().permission(), "oauth:grant:read");
        assert_eq!(require_grant_revoke().permission(), "oauth:grant:revoke");
        assert_eq!(
            require_token_introspect().permission(),
            "oauth:token:introspect"
        );
    }
}
