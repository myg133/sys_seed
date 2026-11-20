//! 审计接口的鉴权薄封装（REQ-014 D160）。
//!
//! # 为什么封装在本 crate 里，而不是写在 `platform-server`
//!
//! 契约检查 C-5 的入口 `check_c5_for(manifest_path, source_dir)`
//! **只扫「该 manifest 所属 crate 自己的 `src/`」**
//! （`crates/platform-contracts/src/checks.rs` 的 `check_c5_for`）。
//! 本需求的 manifest 在 `platform-audit`、HTTP 在 `platform-server` →
//! 直接把 `require_permission("audit:events:read")` 写在 server 侧的话，
//! 对 `platform-audit` 跑 C-5 会报「3 个已登记权限点**没有鉴权调用点**」→ 违例。
//!
//! 所以这里放**三个薄封装**：调用点与权限点编码**字面量**同处一地，
//! C-5 扫得到；同时**不**引入 `platform-audit → platform-rbac`
//! （鉴权入口在 **core**，见 [`platform_core::extension::permission`]）。
//!
//! # 签名说明（与 design §15.2 的示例不同，是有意的）
//!
//! design 的示例写的是 `fn require_events_read(ctx: &RequestContext) -> Result<()>`，
//! 但 core 里的鉴权入口 [`require_permission`] 是**路由层**构造器
//! （返回 [`PermissionLayer`]，供 `.route_layer(...)` 用），
//! **没有**「传 ctx 直接判定」的形态 —— 写不成 design 的示例。
//!
//! 本封装按 core 的**真实 API** 返回 [`PermissionLayer`]，
//! 与既有惯例一致（`crates/platform-rbac/src/http.rs` 的
//! `.route_layer(require_permission("rbac:role:read"))`）。
//!
//! # 鉴权入口必须唯一
//!
//! ⚠️ **不得**在本 crate 里另写一个 `check_permission()` 之类的函数 ——
//! C-5 的扫描器会把「第二个鉴权入口」判为违例。

use platform_core::extension::permission::{PermissionLayer, require_permission};

/// 要求 `audit:events:read`（查看审计日志）。
pub fn require_events_read() -> PermissionLayer {
    require_permission("audit:events:read")
}

/// 要求 `audit:verify:run`（校验审计链）。
pub fn require_verify_run() -> PermissionLayer {
    require_permission("audit:verify:run")
}

/// 要求 `audit:archive:run`（触发审计归档）。
pub fn require_archive_run() -> PermissionLayer {
    require_permission("audit:archive:run")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wrapper_binds_its_permission_literal() {
        assert_eq!(require_events_read().permission(), "audit:events:read");
        assert_eq!(require_verify_run().permission(), "audit:verify:run");
        assert_eq!(require_archive_run().permission(), "audit:archive:run");
    }
}
