//! 沙箱模块权限点。
//!
//! 编码遵守 `api-conventions v1` C4：`module:resource:action`，全小写连字符分隔。
//! 权限点必须同时出现在 `manifest.json` 的 `permissions` 里 —— 契约检查 C-2 负责核对。
//!
//! ⚠️ **常量不是鉴权**：契约检查 C-5 要求每个已登记的权限点都有
//! **真实的鉴权调用点**（`require_permission("...")`）。
//! 只定义常量、从不鉴权 = **假权限**（界面上看着有权限控制，接口其实裸奔）。

/// 查看沙箱示例的权限点。
pub const PERMISSION_DEMO_READ: &str = "sandbox:demo:read";

/// 编辑沙箱示例的权限点。
pub const PERMISSION_DEMO_WRITE: &str = "sandbox:demo:write";

/// 查看数据范围示例的权限点。
pub const PERMISSION_SCOPE_ITEM_READ: &str = "sandbox:scope-item:read";

/// 审批数据范围示例的权限点（**携带属性级条件** `$record.amount <= 10000`）。
pub const PERMISSION_SCOPE_ITEM_APPROVE: &str = "sandbox:scope-item:approve";

/// 导出数据范围示例的权限点。
pub const PERMISSION_SCOPE_ITEM_EXPORT: &str = "sandbox:scope-item:export";

/// 本模块用到的全部权限点。
pub const PERMISSIONS: [&str; 5] = [
    PERMISSION_DEMO_READ,
    PERMISSION_DEMO_WRITE,
    PERMISSION_SCOPE_ITEM_READ,
    PERMISSION_SCOPE_ITEM_APPROVE,
    PERMISSION_SCOPE_ITEM_EXPORT,
];
