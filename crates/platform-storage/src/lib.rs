//! 文件存储与导入导出（REQ-018）。
//!
//! # 这个 crate 解决什么
//!
//! 统一文件存取 + 大批量导入导出**异步可回溯**：
//!
//! ```text
//! platform-core ← platform-storage ← platform-server（装配层）
//!                     ↑
//!          platform-db / platform-config
//! ```
//!
//! # 三件必须知道的事
//!
//! ## 1. 对象存储 = **本地 FS 真实现 + S3 只做协议**（D319）
//!
//! 父 AC-025 要求「S3 兼容」，但**没有 MinIO 环境**（`podman ps` 只有 postgres/redis）→
//! `S3Store` 的 SigV4 签名能按 **AWS 官方向量**验证，但**没有真实端点证明可用**
//! → **TD-059**。⚠️ 报告里只许写「签名按官方向量验证通过」，**不许**写「S3 可用」。
//!
//! ## 2. **只做 CSV**，Excel 显式拒绝（D320）
//!
//! `.xlsx` = ZIP + XML，`zip`/`flate2`/`quick-xml` **全 0 个**在树里 →
//! 上传 `.xlsx` 返回 **415 `STORAGE_UNSUPPORTED_FORMAT`**（不是静默 500，也不是当 CSV 解析）
//! → **TD-060**。
//!
//! ## 3. 异步导入 = **复用 REQ-015 的数据库 worker 形态**（D324）
//!
//! `SELECT ... FOR UPDATE SKIP LOCKED` + `xtask storage worker` + `tick()`。
//! 不重新发明，因为那套已经过 **7 个真库测试**验证（含 4 worker 并发不重不漏）。
//!
//! # ⚠️ 命名禁区（C-5，D216）
//!
//! `src/` 里**不得**出现 `authorize` 等 8 个标识符。
//! 存储域里「是否允许访问这个 key」听起来像鉴权 —— 它**不是**，
//! 那是业务规则「这个 key 属不属于本租户」→ 用 `is_key_visible_to_tenant`。

pub mod csv;
pub mod error;
pub mod events;
pub mod model;
pub mod object_store;

pub use error::{ALL_CODES, ServiceError, StorageError};
pub use events::ALL_CODES as ALL_EVENT_CODES;

/// 版本号。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_exposed() {
        assert!(!VERSION.is_empty());
    }

    /// 错误码**恰好 9 条**（D330：TITLES 107 → 116）。
    #[test]
    fn the_error_code_list_is_exported() {
        assert_eq!(ALL_CODES.len(), 9);
    }

    /// 事件码**恰好 4 条**（AC-S-184 步骤 5）。
    #[test]
    fn the_event_code_list_is_exported() {
        assert_eq!(ALL_EVENT_CODES.len(), 4);
    }

    /// 两套码互斥（D223）。
    #[test]
    fn error_and_event_codes_are_disjoint() {
        for event in ALL_EVENT_CODES {
            assert!(
                !ALL_CODES.contains(event),
                "事件码不得出现在错误码清单里：{event}"
            );
        }
    }
}
