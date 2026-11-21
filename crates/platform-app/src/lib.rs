//! 应用接入与应用注册表（REQ-016）。
//!
//! # 这个 crate 解决什么
//!
//! 底座**作为业务平台**提供「应用注册表 + 工作台 + 导航下发」：
//! 租户管理员可以把第三方应用（REQ-022 的 OAuth client）**接入**到自己的租户里，
//! 定义展示名 / 分类 / 图标 / 入口 URL，并让登录用户在工作台与导航里看到它们。
//!
//! ```text
//! platform-core ← platform-app ← platform-server（装配层）
//!                     ↑
//!              platform-db / platform-oauth（**只读** client 表）
//! ```
//!
//! # 职责边界（与 REQ-022 的分工）
//!
//! | | REQ-022（IdP 侧） | REQ-016（应用侧，本 crate） |
//! |---|---|---|
//! | 关注 | 协议级 client / 同意 / 撤销 / 令牌 | 业务级注册表 / 工作台 / 导航 |
//! | 表 | `platform_oauth_client`（**平台级**） | `platform_app` + `platform_app_access`（**租户级**） |
//! | 关系 | — | **引用**（不复制）client；`ON DELETE CASCADE` |
//!
//! # 隔离层级（D262）
//!
//! 应用注册表是**租户级**（`tenant_id` + RLS，由 `cargo xtask tenant-policies` 生成策略），
//! 而 client 是**平台级**（D219）—— 两者靠**外键**连接，不复制敏感字段。
//!
//! # 依赖方向（单向，无环）
//!
//! `platform-app` 依赖 `platform-core`（trait / 错误体 / 上下文）、
//! `platform-db`（租户作用域数据访问）与 `platform-oauth`（**只读** client 表）；
//! **不依赖** `platform-rbac` / `platform-iam` / `platform-config` ——
//! 权限判定、登录令牌校验、运行期配置都通过 `platform-core` 的 trait 拿，
//! 由**装配层**注入。
//!
//! # ⚠️ 命名禁区（C-5，D216）
//!
//! `src/` 里**不得**出现这 8 个标识符：
//!
//! ```text
//! check_permission   check_perm     has_permission   require_perm
//! authorize          ensure_permission  verify_permission  permission_check
//! ```
//!
//! → 令牌校验函数叫 [`service::AppService::authenticate`]，
//! 鉴权薄封装叫 [`authz::require_registry_read`] 等，**不叫** `authorize_*`。

pub mod authz;
pub mod config;
pub mod error;
pub mod events;
pub mod http;
pub mod model;
pub mod service;
pub mod store;
pub mod tokens;

pub use error::{ALL_CODES, AppError};
pub use events::ALL_CODES as ALL_EVENT_CODES;

/// 应用接入的版本号（与 `Cargo.toml` 中的 `version` 保持一致）。
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
        assert_eq!(ALL_CODES.len(), 6);
    }

    #[test]
    fn the_event_code_list_is_exported() {
        assert_eq!(ALL_EVENT_CODES.len(), 3);
    }
}
