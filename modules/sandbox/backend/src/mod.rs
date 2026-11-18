//! 沙箱模块入口：实现 [`PlatformModule`] 扩展点。
//!
//! # 这是二次开发的标准范例
//!
//! 业务模块的依赖只有一条：
//!
//! ```text
//! platform-core (定义 trait) ← platform-contracts ← modules/<name>/backend
//! ```
//!
//! - 后端只依赖 `platform-core`（扩展点 trait）；
//! - `platform-contracts` 只在测试里依赖（`[dev-dependencies]`）；
//! - 模块**不依赖**平台的任何内部模块。
//!
//! REQ-006 起，模块的数据访问走 **`platform-db`**（租户作用域入口）：
//!
//! ```text
//! platform-core ← platform-db ← modules/<name>/backend
//! ```
//!
//! `platform-db` 只暴露**带租户上下文**的入口，拿不到裸连接 ——
//! 所以业务模块绕不过租户条件。

pub mod entity;
pub mod http;
pub mod permission;
pub mod repository;
pub mod scope_item;
pub mod service;
pub mod tenant_repository;

use anyhow::Result;
use platform_core::extension::platform_module::{ModuleContext, PlatformModule};

/// 沙箱模块实现。
#[derive(Debug, Clone, Copy, Default)]
pub struct SandboxModule;

impl PlatformModule for SandboxModule {
    /// 必须与 `manifest.json` 的 `id` 一致（契约检查 C-1）。
    fn id(&self) -> &'static str {
        "sandbox"
    }

    /// 必须与 `manifest.json` 的 `version` 一致（契约检查 C-1）。
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// 依赖的其它模块 ID（契约检查 C-1）。
    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    /// 启动时调用一次。
    fn setup(&self, ctx: &ModuleContext) -> Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// 是否启用（契约检查 C-1）。
    fn enabled(&self) -> bool {
        true
    }
}

/// 沙箱模块实例（编译期注册用）。
static SANDBOX_MODULE: SandboxModule = SandboxModule;

platform_core::register_platform_module!(SANDBOX_MODULE);
