//! 通知系统的**鉴权薄封装**（REQ-015，D300 / D216）。
//!
//! # 为什么封装在本 crate 里，而不是写在 `platform-server`
//!
//! 契约检查 C-5 的入口 `check_c5_for(manifest_path, source_dir)`
//! **只扫「该 manifest 所属 crate 自己的 `src/`」**
//! （`crates/platform-contracts/src/checks.rs` 的 `check_c5_for`）。
//! 本需求的 manifest 在 `platform-notify`、HTTP 在**同一个 crate** 里 →
//! 调用点与权限点编码**字面量**同处一地，C-5 扫得到。
//!
//! 同时**不**引入 `platform-notify → platform-rbac`（鉴权入口在 **core**，
//! 判定实现由装配层注入）。
//!
//! # ⚠️ 命名禁区（D216）
//!
//! `crates/platform-notify/src/` 里**不得**出现这 8 个标识符：
//!
//! ```text
//! check_permission   check_perm     has_permission   require_perm
//! authorize          ensure_permission  verify_permission  permission_check
//! ```
//!
//! ⚠️ **通知域里极易踩**：「是否允许给这个用户发通知」听起来很像鉴权，
//! 但它**不是** —— 那是**业务规则**（订阅偏好），不是**权限判定**。
//! → 那种函数叫 `is_recipient_eligible` / `should_deliver`，**不叫** `authorize_*`。
//!
//! # ⚠️ 只有 3 个权限点（D300）
//!
//! **订阅偏好不进权限点**：那是**用户自己的**设置（登录即可改），
//! 与 REQ-016 的 D271 同款口径。若给它加权限点，普通用户就改不了自己的退订了。

use platform_core::extension::permission::{PermissionLayer, require_permission};

/// 要求 `notify:template:read`（查看模板 + 预览）。
pub fn require_template_read() -> PermissionLayer {
    require_permission("notify:template:read")
}

/// 要求 `notify:template:write`（建 / 改 / 删模板）。
pub fn require_template_write() -> PermissionLayer {
    require_permission("notify:template:write")
}

/// 要求 `notify:delivery:read`（查看投递记录 + 重投死信）。
///
/// ⚠️ **重投死信也用这个点**：D300 明确「`notify:delivery:read` 的用途 =
/// 查看投递记录 **+ 重投死信**」—— 不单独拆 `notify:delivery:write`。
/// 理由：能看投递记录的人本来就能看到失败原因，重投是**补救**动作，
/// 拆成两个点只会让「运维能看不能救」。
pub fn require_delivery_read() -> PermissionLayer {
    require_permission("notify:delivery:read")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3 个封装各自绑定正确的字面量（C-5 要求字面量）。
    #[test]
    fn every_wrapper_binds_its_permission_literal() {
        assert_eq!(require_template_read().permission(), "notify:template:read");
        assert_eq!(
            require_template_write().permission(),
            "notify:template:write"
        );
        assert_eq!(require_delivery_read().permission(), "notify:delivery:read");
    }

    /// 3 个封装**互不相同**（复制粘贴漏改会让两个端点共用一个权限点）。
    #[test]
    fn the_three_wrappers_are_distinct() {
        let codes = [
            require_template_read().permission().to_string(),
            require_template_write().permission().to_string(),
            require_delivery_read().permission().to_string(),
        ];
        let unique: std::collections::BTreeSet<&String> = codes.iter().collect();
        assert_eq!(unique.len(), 3, "三个封装必须绑定三个不同的权限点");
    }

    /// 3 个码都在 `notify:` 命名空间下（与 manifest 一致）。
    #[test]
    fn every_permission_is_in_the_notify_namespace() {
        for layer in [
            require_template_read(),
            require_template_write(),
            require_delivery_read(),
        ] {
            assert!(
                layer.permission().starts_with("notify:"),
                "权限点必须在 notify: 下：{}",
                layer.permission()
            );
        }
    }
}
