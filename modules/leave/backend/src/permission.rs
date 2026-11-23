//! 请假模块权限点。
//!
//! 编码遵守 `api-conventions v1` C4：`module:resource:action`。
//! 权限点必须同时出现在 `manifest.json` 的 `permissions` 里 —— 契约检查 C-2 核对。
//!
//! ⚠️ **常量不是鉴权**：C-5 要求每个已登记的权限点都有真实的
//! `require_permission("...")` 调用点（见 `http.rs` 的 `route_layer`）。
//! 只定义常量、从不鉴权 = **假权限**。

/// 查看请假单（列表）。
pub const PERMISSION_REQUEST_READ: &str = "leave:request:read";

/// 建单 / 提交（申请人侧的两个写动作）。
pub const PERMISSION_REQUEST_WRITE: &str = "leave:request:write";

/// 审批（通过 / 驳回）。
pub const PERMISSION_REQUEST_APPROVE: &str = "leave:request:approve";

/// 本模块用到的全部权限点。
pub const PERMISSIONS: [&str; 3] = [
    PERMISSION_REQUEST_READ,
    PERMISSION_REQUEST_WRITE,
    PERMISSION_REQUEST_APPROVE,
];
