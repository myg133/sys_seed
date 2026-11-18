//! 平台核心骨架。
//!
//! 本期只放最小组件：配置类型、统一错误类型、请求上下文。
//! 扩展点 trait 由 REQ-003 定义，业务模块能力由后续 Sprint 补齐。

pub mod config;
pub mod context;
pub mod error;
pub mod extension;
pub mod problem;

pub use config::{AppConfig, ConfigEntry, ConfigKey, ConfigResolver, ConfigScope};
pub use context::RequestContext;
pub use error::{CoreError, CoreResult};
pub use extension::{
    Auditable, DataScope, Manifest, ModuleContext, NotificationSender, PermissionChecker,
    PlatformModule, Repository, ScopeFilter, WorkflowHandler,
};
pub use problem::ProblemDetails;

/// 平台核心版本号，与 `Cargo.toml` 中的 `version` 保持一致。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_exposed() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn public_items_are_reachable_from_crate_root() {
        let ctx = RequestContext::new().with_tenant("tenant-a");
        assert_eq!(ctx.tenant_id(), Some("tenant-a"));
        assert_eq!(ConfigScope::Platform.rank(), 0);
    }
}
