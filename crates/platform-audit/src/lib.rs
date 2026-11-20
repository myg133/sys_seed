//! 平台审计：哈希链写入、查询、链校验与归档（REQ-014）。
//!
//! # 这个 crate 解决什么
//!
//! 审计是**横切关注点**：被 iam / rbac / config / org 同时使用。
//! 放进 `platform-core` 会让 core 背上 sqlx；放进任一业务 crate 则其它 crate
//! 都得依赖它 → 单独成 crate。
//!
//! ```text
//! platform-core/extension/audit.rs   通用 AuditEvent + AuditSink trait
//!        ↑ trait
//! platform-iam / platform-org / platform-rbac / platform-config
//!        │ （棒 2：各模块把事件翻译成通用事件并注入 sink）
//! platform-audit（本 crate）  PgAuditSink / 查询 / 链校验 / 归档
//!        ↑ 装配
//! platform-server（棒 2：/api/audit 路由 + PgAuditSink 构造）
//! ```
//!
//! # 为什么 HTTP 不在本 crate
//!
//! 权限检查要用 `platform-rbac`，而 `platform-audit → platform-rbac → platform-iam
//! → platform-audit` 会**成环**。HTTP 放 `platform-server`（它已依赖全部 crate）。
//!
//! # 本轮（棒 1）交付到哪
//!
//! - ✅ 平台核心的审计类型 + 敏感键扫描器 + JSON 规范化；
//! - ✅ 数据库（3 张表 + RLS + append-only + 受限的特权函数）；
//! - ✅ 权限点（3 个）与 manifest；
//! - ❌ 业务模块埋点、`/api/audit` 路由、前端页面 —— 那是**棒 2**。

pub mod archive;
pub mod authz;
pub mod error;
pub mod query;
pub mod sensitive;
pub mod sink;
pub mod verify;

pub use archive::{ArchiveResult, ArchiveService, DEFAULT_ONLINE_DAYS, RECOMMENDED_INTERVAL_DAYS};
pub use authz::{require_archive_run, require_events_read, require_verify_run};
pub use error::ALL_CODES;
pub use platform_core::extension::audit::{AuditError, AuditEvent, AuditSink};
pub use query::{
    AuditEventRow, AuditPage, AuditQueryParams, AuditQueryService, DEFAULT_LIMIT, FilterOp,
    MAX_LIMIT, SortDirection,
};
pub use sensitive::{SENSITIVE_KEYS, scan_sensitive_keys};
pub use sink::PgAuditSink;
pub use verify::{BrokenEntry, BrokenReason, ChainVerifier, VerifyReport};

/// 平台审计的版本号（与 `Cargo.toml` 中的 `version` 保持一致）。
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
        assert_eq!(ALL_CODES.len(), 5);
    }
}
