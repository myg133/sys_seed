//! 平台权限：角色模型、数据范围求值、鉴权中间件。
//!
//! # 这个 crate 是什么
//!
//! **判定逻辑只在这里写一次** —— REQ-008 交付的组织结构与汇报关系是它的**输入**，
//! 不是它的副本。
//!
//! | 能力 | 位置 |
//! |------|------|
//! | 角色 / 授权 / 继承 / 绑定 | [`service`] |
//! | 数据范围求值（5 种 scope、兼岗、fail closed） | [`scope`] |
//! | 受限表达式 DSL | `platform-core::extension::expr` |
//! | 缓存 + 数据库 epoch 主动失效 | [`checker`] |
//! | 权限点注册表双向对账 | [`registry`] |
//! | `/api/rbac/*` | [`http`] |
//!
//! # 依赖方向
//!
//! ```text
//! platform-core (PermissionChecker / require_permission / DataScope)
//!       ↑
//! platform-db (TenantDb / TenantQuery)
//!       ↑
//! platform-rbac (本 crate：判定实现)
//!       ↑
//! platform-server (装配：注入 Arc<dyn PermissionChecker>)
//! ```
//!
//! 业务模块**不依赖本 crate** —— 它们只用 `platform-core` 的
//! `require_permission` 与 `platform-db` 的 `TenantQuery::with_scope`
//! （AGENTS.md 硬规则 2）。

pub mod checker;
pub mod entity;
pub mod error;
pub mod http;
pub mod registry;
pub mod scope;
pub mod service;
pub mod store;

pub use checker::RbacChecker;
pub use entity::{
    BUILTIN_ADMIN, BUILTIN_USER, Permission, PermissionPattern, Role, RoleInherit, RolePermission,
    UserRole,
};
pub use error::{RbacError, code};
pub use registry::{ManifestPermission, Reconciliation, collect_manifest_permissions, reconcile};
pub use scope::{
    CombinedScope, ScopeContribution, ScopePolicy, ScopeResolver, combine_scopes,
    primary_department,
};
pub use service::{RbacService, RoleGraph, RoleView, UserPermissionView};
pub use store::RbacStore;

use std::sync::Arc;

use platform_core::extension::permission::PermissionChecker;

/// 把判定器同时包成 `Arc<dyn PermissionChecker>`（请求扩展里放的就是它）。
pub fn as_dyn_checker(checker: Arc<RbacChecker>) -> Arc<dyn PermissionChecker> {
    checker
}
