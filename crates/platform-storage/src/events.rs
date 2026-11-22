//! 文件存储的**审计事件码**（REQ-018，D223 同款口径）。
//!
//! # ⚠️ 事件码**不进** `TITLES`，**也不进** [`crate::error::ALL_CODES`]
//!
//! 「审计事件码不是 HTTP 错误码」—— 与 `oauth.*` / `app.*` / `notify.*` 同一口径（D223）。
//!
//! # 命名：三段式 `storage.<对象>.<动作>`

/// 文件上传成功。
pub const FILE_UPLOADED: &str = "storage.file.uploaded";
/// 文件删除。
pub const FILE_DELETED: &str = "storage.file.deleted";
/// 导入任务完成。
pub const IMPORT_COMPLETED: &str = "storage.import.completed";
/// 导出任务完成。
pub const EXPORT_COMPLETED: &str = "storage.export.completed";

/// 本 crate 的全部审计事件码。
pub const ALL_CODES: &[&str] = &[
    FILE_UPLOADED,
    FILE_DELETED,
    IMPORT_COMPLETED,
    EXPORT_COMPLETED,
];

/// 审计对象类型（契约 C12 的 `object_type`，用**表名语义**）。
pub const OBJECT_TYPE_OBJECT: &str = "platform_storage_object";
/// 任务表的表名语义。
pub const OBJECT_TYPE_JOB: &str = "platform_storage_job";

#[cfg(test)]
mod tests {
    use super::*;

    /// 恰好 4 条且无重复。
    #[test]
    fn the_event_code_list_has_exactly_four_distinct_codes() {
        assert_eq!(ALL_CODES.len(), 4, "AC-S-184 步骤 5 要求恰好 4 个事件码");
        let unique: std::collections::BTreeSet<&str> = ALL_CODES.iter().copied().collect();
        assert_eq!(unique.len(), 4);
    }

    /// 全部三段式 `storage.<对象>.<动作>`。
    #[test]
    fn every_event_code_is_three_segments() {
        for code in ALL_CODES {
            let segments: Vec<&str> = code.split('.').collect();
            assert_eq!(segments.len(), 3, "事件码必须三段式：{code} → {segments:?}");
            assert_eq!(segments[0], "storage", "第一段必须是 storage：{code}");
        }
    }

    /// 全部 ASCII 小写。
    #[test]
    fn every_event_code_is_ascii_lower() {
        for code in ALL_CODES {
            assert!(
                code.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '-' || c.is_ascii_digit()),
                "事件码必须是 ASCII 小写：{code}"
            );
        }
    }

    /// 事件码与错误码**互斥**（两套命名空间不混）。
    #[test]
    fn event_codes_are_disjoint_from_error_codes() {
        for event in ALL_CODES {
            assert!(
                !crate::error::ALL_CODES.contains(event),
                "事件码不得出现在错误码清单里：{event}"
            );
        }
    }

    /// `object_type` 用表名语义。
    #[test]
    fn object_types_are_table_names() {
        assert_eq!(OBJECT_TYPE_OBJECT, "platform_storage_object");
        assert_eq!(OBJECT_TYPE_JOB, "platform_storage_job");
        for name in [OBJECT_TYPE_OBJECT, OBJECT_TYPE_JOB] {
            assert!(
                name.starts_with("platform_storage_"),
                "必须是本 crate 的表名：{name}"
            );
        }
    }
}
