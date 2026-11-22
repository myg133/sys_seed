//! 文件存储的**鉴权薄封装**（REQ-018，D332 / D216）。
//!
//! # 为什么封装在本 crate 里
//!
//! C-5 的入口 `check_c5_for(manifest_path, source_dir)` **只扫「该 manifest 所属
//! crate 自己的 `src/`」**。本需求的 manifest 在 `platform-storage`、HTTP 也在
//! **同一个 crate** 里 → 调用点与权限点编码**字面量**同处一地，C-5 扫得到。
//!
//! # ⚠️ 命名禁区（D216）
//!
//! `src/` 里**不得**出现这 8 个标识符：
//!
//! ```text
//! check_permission   check_perm     has_permission   require_perm
//! authorize          ensure_permission  verify_permission  permission_check
//! ```
//!
//! ⚠️ **存储域极易踩**：「是否允许访问这个 key」听起来像鉴权 —— 它**不是**，
//! 那是**业务规则**（这个 key 属不属于本租户）→ 用 `is_key_visible_to_tenant`。
//! 真正的权限判定只有下面 3 个权限点。

use platform_core::extension::permission::{PermissionLayer, require_permission};

/// 要求 `storage:file:read`（查看 / 下载文件 + 预签名 + 下载导出结果）。
pub fn require_file_read() -> PermissionLayer {
    require_permission("storage:file:read")
}

/// 要求 `storage:file:write`（上传 / 删除文件 + 建分片会话 + 建导入导出任务）。
pub fn require_file_write() -> PermissionLayer {
    require_permission("storage:file:write")
}

/// 要求 `storage:job:read`（查看任务与错误回执）。
pub fn require_job_read() -> PermissionLayer {
    require_permission("storage:job:read")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3 个封装各自绑定正确的字面量。
    #[test]
    fn every_wrapper_binds_its_permission_literal() {
        assert_eq!(require_file_read().permission(), "storage:file:read");
        assert_eq!(require_file_write().permission(), "storage:file:write");
        assert_eq!(require_job_read().permission(), "storage:job:read");
    }

    /// 3 个封装**互不相同**（复制粘贴漏改会让两个端点共用一个权限点）。
    #[test]
    fn the_three_wrappers_are_distinct() {
        let codes = [
            require_file_read().permission().to_string(),
            require_file_write().permission().to_string(),
            require_job_read().permission().to_string(),
        ];
        let unique: std::collections::BTreeSet<&String> = codes.iter().collect();
        assert_eq!(unique.len(), 3, "三个封装必须绑定三个不同的权限点");
    }

    /// 全部在 `storage:` 命名空间下。
    #[test]
    fn every_permission_is_in_the_storage_namespace() {
        for layer in [
            require_file_read(),
            require_file_write(),
            require_job_read(),
        ] {
            assert!(
                layer.permission().starts_with("storage:"),
                "权限点必须在 storage: 下：{}",
                layer.permission()
            );
        }
    }
}
