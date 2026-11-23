//! 请假审批模块入口：实现 [`PlatformModule`] 扩展点。
//!
//! # 它是什么
//!
//! REQ-020 的**业务消费方**（D348）：一个实体 + 列表 + 表单 + 一个审批流，
//! 只写业务代码，其余全部复用平台能力。
//!
//! # 依赖方向
//!
//! ```text
//! platform-core (定义 trait) ← platform-db ← modules/leave
//! platform-contracts 只在 [dev-dependencies]（契约测试）
//! ```
//!
//! 模块**不依赖**平台内部模块：没有 `platform-notify`（通知走 trait）、
//! 没有 `platform-rbac`（鉴权走 `require_permission`）、
//! 没有 `platform-audit`（审计走 `AuditSink`）。
//!
//! # 审批流为什么在业务层（D347）
//!
//! REQ-017（工作流引擎）已取消，`WorkflowHandler::on_transition` 在平台侧零调用点。
//! 本模块**不**声称复用工作流引擎 —— 三态状态机是业务代码（`service.rs`），
//! 平台提供的是流转的横切能力（审计 + 通知）。

pub mod entity;
pub mod http;
pub mod permission;
pub mod repository;
pub mod service;

use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use platform_core::extension::platform_module::{ModuleContext, PlatformModule};
use platform_db::{DbOptions, TenantDb};

use crate::http::LeaveState;

/// 请假模块实现。
#[derive(Debug, Clone, Copy, Default)]
pub struct LeaveModule;

impl PlatformModule for LeaveModule {
    /// 必须与 `manifest.json` 的 `id` 一致（契约检查 C-1）。
    fn id(&self) -> &'static str {
        "leave"
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

/// 连库并构造模块状态（装配层可继续 `.with_audit(...)` / `.with_notifier(...)`）。
pub async fn connect(database_url: &str) -> Result<LeaveState> {
    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .map_err(|error| anyhow::anyhow!("the leave database connection failed: {error}"))?;
    Ok(LeaveState::new(db))
}

/// 把请假模块的路由装配起来 —— **不带任何注入**的便捷入口。
///
/// # ⚠️ 生产装配**不**用它（REQ-020 第三批的实测结论）
///
/// 它只做 `connect` + `router`，**不透传**审计 / 通知 → 用它挂出来的
/// `/api/leave/*` **不写审计、不发通知**（AC-S-189 的计数表会全红）。
/// 平台的装配臂（`crates/platform-server/src/main.rs` 的 `"leave"` 分支）
/// 走的是「`connect` → `.with_audit(...)` / `.with_notifier(...)` → `router`」。
///
/// 保留它是给「只要路由、不接横切能力」的场景（如路由形状自检）；
/// 新增调用方时**先确认**是否真的不需要审计与通知。
pub async fn mount<S2>(database_url: &str) -> Result<Router<S2>> {
    let state = connect(database_url).await?;
    Ok(crate::http::router::<S2>(Arc::new(state)))
}

/// 模块实例（编译期注册用）。
static LEAVE_MODULE: LeaveModule = LeaveModule;

platform_core::register_platform_module!(LEAVE_MODULE);
