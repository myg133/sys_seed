//! 沙箱模块权限点。
//!
//! 编码遵守 `api-conventions v1` C4：`module:resource:action`，全小写连字符分隔。
//! 权限点必须同时出现在 `manifest.json` 的 `permissions` 里 —— 契约检查 C-2 负责核对。

/// 查看权限点。
pub const PERMISSION_DEMO_READ: &str = "sandbox:demo:read";

/// 编辑权限点。
pub const PERMISSION_DEMO_WRITE: &str = "sandbox:demo:write";

/// 本模块用到的全部权限点。
pub const PERMISSIONS: [&str; 2] = [PERMISSION_DEMO_READ, PERMISSION_DEMO_WRITE];
