//! 应用接入的**鉴权薄封装**（REQ-016，D271 / D216）。
//!
//! # 为什么封装在本 crate 里，而不是写在 `platform-server`
//!
//! 契约检查 C-5 的入口 `check_c5_for(manifest_path, source_dir)`
//! **只扫「该 manifest 所属 crate 自己的 `src/`」**
//! （`crates/platform-contracts/src/checks.rs` 的 `check_c5_for`）。
//! 本需求的 manifest 在 `platform-app`、HTTP 在**同一个 crate** 里 →
//! 调用点与权限点编码**字面量**同处一地，C-5 扫得到。
//!
//! 同时**不**引入 `platform-app → platform-rbac`（鉴权入口在 **core**，
//! 判定实现由装配层注入）。
//!
//! # ⚠️ 签名说明
//!
//! `platform_core::extension::permission::require_permission` 是**路由层**构造器
//! （返回 `PermissionLayer`，供 `.route_layer(...)` 用），**没有**
//! 「传 ctx 直接判定」的形态。本封装按 core 的**真实 API** 返回 `PermissionLayer`。
//!
//! # ⚠️ 命名禁区（D216）
//!
//! `crates/platform-app/src/` 里**不得**出现这 8 个标识符：
//!
//! ```text
//! check_permission   check_perm     has_permission   require_perm
//! authorize          ensure_permission  verify_permission  permission_check
//! ```
//!
//! ⚠️ **`authorize` 在「应用接入」语境下极易踩**（如 `authorize_app_access` /
//! `authorize_external_launch`）→ 写了就是「自建鉴权入口」→ C-5 直接失败。
//!
//! # ⚠️ 只有 3 个权限点（D271）
//!
//! **工作台 / 导航不进权限点**：它们是**登录用户都能看**的（导航项已按权限点过滤）。
//! 若给它们加权限点，普通用户就打不开工作台了 —— 与「统一入口」的目标冲突。

use platform_core::extension::permission::{PermissionLayer, require_permission};

/// 要求 `app:registry:read`（查看应用注册表）。
pub fn require_registry_read() -> PermissionLayer {
    require_permission("app:registry:read")
}

/// 要求 `app:registry:write`（注册 / 修改应用；**含**清空 `launch_url`）。
pub fn require_registry_write() -> PermissionLayer {
    require_permission("app:registry:write")
}

/// 要求 `app:registry:delete`（删除应用）。
pub fn require_registry_delete() -> PermissionLayer {
    require_permission("app:registry:delete")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wrapper_binds_its_permission_literal() {
        assert_eq!(require_registry_read().permission(), "app:registry:read");
        assert_eq!(require_registry_write().permission(), "app:registry:write");
        assert_eq!(require_registry_delete().permission(), "app:registry:delete");
    }

    /// 3 个封装**互不相同**（复制粘贴漏改会让两个端点共用一个权限点）。
    #[test]
    fn the_three_wrappers_are_distinct() {
        let codes = [
            require_registry_read().permission().to_string(),
            require_registry_write().permission().to_string(),
            require_registry_delete().permission().to_string(),
        ];
        let unique: std::collections::BTreeSet<&String> = codes.iter().collect();
        assert_eq!(unique.len(), 3, "三个封装必须绑定三个不同的权限点");
    }
}
