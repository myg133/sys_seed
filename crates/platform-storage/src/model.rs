//! 数据模型：**4 张表的行类型** + 跨栈 DTO（REQ-018，design §2 / §3.1）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// 对象元数据表（**租户级**）。
pub const OBJECT_TABLE: &str = "platform_storage_object";
/// 上传会话表（**租户级**）。
pub const UPLOAD_TABLE: &str = "platform_storage_upload";
/// 导入导出任务表（**租户级**）。
pub const JOB_TABLE: &str = "platform_storage_job";
/// 行级错误回执表（**租户级**）。
pub const JOB_ERROR_TABLE: &str = "platform_storage_job_error";

/// 对象的**状态**。
pub mod object_status {
    /// 正在写（分片上传未完成）。
    pub const PENDING: &str = "pending";
    /// 可用。
    pub const AVAILABLE: &str = "available";
    /// 正在删。
    pub const DELETING: &str = "deleting";

    /// 全部取值。
    pub const ALL: &[&str] = &[PENDING, AVAILABLE, DELETING];
}

/// 病毒扫描的**判定**（D326）。
pub mod scan_verdict {
    /// **没扫**（默认实现返回这个）—— 与「扫过且干净」是两件事。
    pub const SKIPPED: &str = "skipped";
    /// 扫过且干净。
    pub const CLEAN: &str = "clean";
    /// 扫过且判定为感染。
    pub const INFECTED: &str = "infected";

    /// 全部取值。
    pub const ALL: &[&str] = &[SKIPPED, CLEAN, INFECTED];
}

/// 上传会话的**状态**。
pub mod upload_status {
    /// 待上传分片。
    pub const PENDING: &str = "pending";
    /// 正在合并。
    pub const COMPLETING: &str = "completing";
    /// 已完成。
    pub const COMPLETED: &str = "completed";
    /// 已中止。
    pub const ABORTED: &str = "aborted";

    /// 全部取值。
    pub const ALL: &[&str] = &[PENDING, COMPLETING, COMPLETED, ABORTED];
}

/// 任务的**状态**。
pub mod job_status {
    /// 待处理。
    pub const PENDING: &str = "pending";
    /// 处理中。
    pub const RUNNING: &str = "running";
    /// 已完成。
    pub const COMPLETED: &str = "completed";
    /// 失败。
    pub const FAILED: &str = "failed";

    /// 全部取值。
    pub const ALL: &[&str] = &[PENDING, RUNNING, COMPLETED, FAILED];
}

/// 任务的**种类**。
pub mod job_kind {
    /// 导入。
    pub const IMPORT: &str = "import";
    /// 导出。
    pub const EXPORT: &str = "export";

    /// 全部取值。
    pub const ALL: &[&str] = &[IMPORT, EXPORT];
}

/// `platform_storage_object` 的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRow {
    /// 对象 ID。
    pub id: String,
    /// 租户 ID。
    pub tenant_id: String,
    /// 逻辑路径。
    pub object_key: String,
    /// 字节数。
    pub size_bytes: i64,
    /// 内容摘要（sha256，小写 hex）。
    pub sha256: String,
    /// MIME 类型。
    pub content_type: String,
    /// 扫描判定（见 [`scan_verdict`]）。
    pub scan_verdict: String,
    /// 状态（见 [`object_status`]）。
    pub status: String,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 更新时间。
    pub updated_at: OffsetDateTime,
}

/// `platform_storage_upload` 的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadRow {
    /// 上传会话 ID。
    pub upload_id: String,
    /// 租户 ID。
    pub tenant_id: String,
    /// 目标对象 key。
    pub object_key: String,
    /// 分片总数。
    pub total_parts: i32,
    /// 每片字节数。
    pub part_size: i64,
    /// 客户端声明的 checksum（`complete` 时比对）。
    pub declared_checksum: Option<String>,
    /// 状态（见 [`upload_status`]）。
    pub status: String,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 更新时间。
    pub updated_at: OffsetDateTime,
    /// 过期时间（未完成会话的清理依据）。
    pub expires_at: OffsetDateTime,
}

/// `platform_storage_job` 的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    /// 任务 ID。
    pub id: String,
    /// 租户 ID。
    pub tenant_id: String,
    /// 种类（见 [`job_kind`]）。
    pub kind: String,
    /// 目标实体。
    pub entity: String,
    /// 状态（见 [`job_status`]）。
    pub status: String,
    /// 总行数（导入时有；导出时是预计行数）。
    pub total_rows: i64,
    /// 已处理行数（**每批更新**，D325）。
    pub processed_rows: i64,
    /// 失败行数。
    pub failed_rows: i64,
    /// 导入的源 / 导出的结果。
    pub object_key: Option<String>,
    /// 整体失败原因。
    pub error: Option<String>,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 更新时间。
    pub updated_at: OffsetDateTime,
    /// 完成时间。
    pub finished_at: Option<OffsetDateTime>,
}

/// `platform_storage_job_error` 的一行（行级错误回执）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobErrorRow {
    /// 任务 ID。
    pub job_id: String,
    /// 租户 ID。
    pub tenant_id: String,
    /// **文件行号**（1-based，含表头）—— 不是数据序号。
    pub row_number: i64,
    /// 真实列名（`email` / `amount`），不是下标。
    pub column: String,
    /// 英文 ASCII 原因。
    pub reason: String,
}

/// 四张表的列清单（**DDL 断言用**，与 `information_schema` 双向比对）。
pub const OBJECT_COLUMNS: &[&str] = &[
    "id",
    "tenant_id",
    "object_key",
    "size_bytes",
    "sha256",
    "content_type",
    "scan_verdict",
    "status",
    "created_at",
    "updated_at",
];

/// 上传会话表的列清单。
pub const UPLOAD_COLUMNS: &[&str] = &[
    "upload_id",
    "tenant_id",
    "object_key",
    "total_parts",
    "part_size",
    "declared_checksum",
    "status",
    "created_at",
    "updated_at",
    "expires_at",
];

/// 任务表的列清单。
pub const JOB_COLUMNS: &[&str] = &[
    "id",
    "tenant_id",
    "kind",
    "entity",
    "status",
    "total_rows",
    "processed_rows",
    "failed_rows",
    "object_key",
    "error",
    "created_at",
    "updated_at",
    "finished_at",
];

/// 错误回执表的列清单。
pub const JOB_ERROR_COLUMNS: &[&str] = &["job_id", "tenant_id", "row_number", "column", "reason"];

// ---------------------------------------------------------------------------
// 跨栈 DTO（camelCase）
// ---------------------------------------------------------------------------

/// 对象的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectView {
    /// 逻辑路径。
    pub object_key: String,
    /// 字节数。
    pub size_bytes: i64,
    /// sha256。
    pub sha256: String,
    /// MIME。
    pub content_type: String,
    /// 扫描判定。
    pub scan_verdict: String,
    /// 创建时间（RFC3339 微秒 6 位）。
    pub created_at: String,
}

/// 预签名响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresignView {
    /// 可下载的 URL。
    pub url: String,
    /// 过期时刻（RFC3339 微秒 6 位）。
    pub expires_at: String,
}

/// 任务的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    /// 任务 ID。
    pub job_id: String,
    /// 种类。
    pub kind: String,
    /// 实体。
    pub entity: String,
    /// 状态。
    pub status: String,
    /// 总行数。
    pub total_rows: i64,
    /// 已处理行数。
    pub processed_rows: i64,
    /// 失败行数。
    pub failed_rows: i64,
    /// 导出结果的可下载 URL。
    pub download_url: Option<String>,
    /// 导出结果的大小（字节）；非导出任务或未完成时为 `None`。
    ///
    /// ⚠️ AC-S-182 步骤 3 明确要求 `downloadUrl` **+ `size`** ——
    /// 只有 URL 的话前端无法显示进度、也无法判断下载是否完整。
    pub size: Option<i64>,
    /// 整体失败原因。
    pub error: Option<String>,
    /// 创建时间。
    pub created_at: String,
    /// 完成时间。
    pub finished_at: Option<String>,
}

/// 错误回执的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobErrorView {
    /// 文件行号（1-based）。
    pub row_number: i64,
    /// 列名。
    pub column: String,
    /// 原因（ASCII）。
    pub reason: String,
}

/// 建上传会话的请求体。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUploadBody {
    /// 目标对象 key。
    pub object_key: String,
    /// 分片总数。
    pub total_parts: i32,
    /// 每片字节数。
    pub part_size: i64,
    /// 声明的 checksum。
    #[serde(default)]
    pub checksum: Option<String>,
}

/// 完成上传的请求体。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteUploadBody {
    /// MIME 类型（可选）。
    #[serde(default)]
    pub content_type: Option<String>,
}

/// 预签名请求体。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresignBody {
    /// 请求的 TTL（秒）。
    pub ttl_seconds: i64,
}

/// 建导入 / 导出任务的请求体。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateJobBody {
    /// 目标实体。
    pub entity: String,
    /// 导入的源对象 key（导出时省略）。
    #[serde(default)]
    pub object_key: Option<String>,
}

// ---------------------------------------------------------------------------
// 视图转换
// ---------------------------------------------------------------------------

/// 对象行 → 响应体。
pub fn view_of_object(row: &ObjectRow) -> ObjectView {
    ObjectView {
        object_key: row.object_key.clone(),
        size_bytes: row.size_bytes,
        sha256: row.sha256.clone(),
        content_type: row.content_type.clone(),
        scan_verdict: row.scan_verdict.clone(),
        created_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.created_at),
    }
}

/// 任务行 → 响应体。
///
/// `download_url` 与 `size` 只在「已完成的导出任务」上非空 ——
/// 两者都由调用方（HTTP 层）从对象元数据取，`model` 层不查库。
pub fn view_of_job(row: &JobRow, download_url: Option<String>, size: Option<i64>) -> JobView {
    JobView {
        job_id: row.id.clone(),
        kind: row.kind.clone(),
        entity: row.entity.clone(),
        status: row.status.clone(),
        total_rows: row.total_rows,
        processed_rows: row.processed_rows,
        failed_rows: row.failed_rows,
        download_url,
        size,
        error: row.error.clone(),
        created_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.created_at),
        finished_at: row
            .finished_at
            .as_ref()
            .map(platform_core::extension::audit::format_timestamp_micros_utc),
    }
}

/// 错误回执行 → 响应体。
pub fn view_of_job_error(row: &JobErrorRow) -> JobErrorView {
    JobErrorView {
        row_number: row.row_number,
        column: row.column.clone(),
        reason: row.reason.clone(),
    }
}

/// 从 `BTreeMap` 取一个必填字段（缺失 → `None`）。
pub fn required_field<'a>(map: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    map.get(name).map(String::as_str).filter(|v| !v.is_empty())
}

// ---------------------------------------------------------------------------
// `FromRow` 实现（用平台声明的宏，**不直接依赖 sqlx**）
// ---------------------------------------------------------------------------

platform_db::tenant_row!(ObjectRow {
    id: "id",
    tenant_id: "tenant_id",
    object_key: "object_key",
    size_bytes: "size_bytes",
    sha256: "sha256",
    content_type: "content_type",
    scan_verdict: "scan_verdict",
    status: "status",
    created_at: "created_at",
    updated_at: "updated_at",
});

platform_db::tenant_row!(UploadRow {
    upload_id: "upload_id",
    tenant_id: "tenant_id",
    object_key: "object_key",
    total_parts: "total_parts",
    part_size: "part_size",
    declared_checksum: "declared_checksum",
    status: "status",
    created_at: "created_at",
    updated_at: "updated_at",
    expires_at: "expires_at",
});

platform_db::tenant_row!(JobRow {
    id: "id",
    tenant_id: "tenant_id",
    kind: "kind",
    entity: "entity",
    status: "status",
    total_rows: "total_rows",
    processed_rows: "processed_rows",
    failed_rows: "failed_rows",
    object_key: "object_key",
    error: "error",
    created_at: "created_at",
    updated_at: "updated_at",
    finished_at: "finished_at",
});

platform_db::tenant_row!(JobErrorRow {
    job_id: "job_id",
    tenant_id: "tenant_id",
    row_number: "row_number",
    column: "column",
    reason: "reason",
});

#[cfg(test)]
mod tests {
    use super::*;

    /// 4 张表的列清单都无重复。
    #[test]
    fn every_column_list_is_unique() {
        for (name, columns) in [
            ("object", OBJECT_COLUMNS),
            ("upload", UPLOAD_COLUMNS),
            ("job", JOB_COLUMNS),
            ("job_error", JOB_ERROR_COLUMNS),
        ] {
            let unique: std::collections::BTreeSet<&str> = columns.iter().copied().collect();
            assert_eq!(unique.len(), columns.len(), "{name} 表列清单有重复");
        }
    }

    /// 4 张表都有 `tenant_id`（RLS 前提）。
    #[test]
    fn every_table_has_tenant_id() {
        for (name, columns) in [
            ("object", OBJECT_COLUMNS),
            ("upload", UPLOAD_COLUMNS),
            ("job", JOB_COLUMNS),
            ("job_error", JOB_ERROR_COLUMNS),
        ] {
            assert!(
                columns.contains(&"tenant_id"),
                "{name} 表必须有 tenant_id（RLS 前提）"
            );
        }
    }

    /// 扫描判定的默认语义：`skipped` 与 `clean` 是**不同**的值（D326）。
    #[test]
    fn skipped_and_clean_are_distinct_verdicts() {
        assert_ne!(scan_verdict::SKIPPED, scan_verdict::CLEAN);
        assert_eq!(scan_verdict::ALL.len(), 3);
        assert!(scan_verdict::ALL.contains(&scan_verdict::SKIPPED));
        assert!(scan_verdict::ALL.contains(&scan_verdict::CLEAN));
        assert!(scan_verdict::ALL.contains(&scan_verdict::INFECTED));
    }

    /// 状态取值集合的个数。
    #[test]
    fn the_status_sets_have_the_expected_sizes() {
        assert_eq!(object_status::ALL.len(), 3);
        assert_eq!(upload_status::ALL.len(), 4);
        assert_eq!(job_status::ALL.len(), 4);
        assert_eq!(job_kind::ALL.len(), 2);
    }

    /// DTO 序列化成 camelCase。
    #[test]
    fn the_dtos_serialize_in_camel_case() {
        let view = ObjectView {
            object_key: "docs/a.txt".into(),
            size_bytes: 10,
            sha256: "abc".into(),
            content_type: "text/plain".into(),
            scan_verdict: scan_verdict::SKIPPED.into(),
            created_at: "2026-11-22T00:00:00.000000Z".into(),
        };
        let json = serde_json::to_value(&view).expect("序列化");
        for key in [
            "objectKey",
            "sizeBytes",
            "sha256",
            "contentType",
            "scanVerdict",
            "createdAt",
        ] {
            assert!(json.get(key).is_some(), "缺 camelCase 字段：{key}");
        }
        assert!(json.get("object_key").is_none(), "不得出现 snake_case");
    }

    /// 任务 DTO 的 camelCase 字段逐个核对。
    #[test]
    fn the_job_dto_uses_the_documented_field_names() {
        let view = JobView {
            job_id: "j1".into(),
            kind: job_kind::IMPORT.into(),
            entity: "probe_rows".into(),
            status: job_status::PENDING.into(),
            total_rows: 0,
            processed_rows: 0,
            failed_rows: 0,
            download_url: None,
            size: None,
            error: None,
            created_at: "2026-11-22T00:00:00.000000Z".into(),
            finished_at: None,
        };
        let json = serde_json::to_value(&view).expect("序列化");
        for key in [
            "jobId",
            "kind",
            "entity",
            "status",
            "totalRows",
            "processedRows",
            "failedRows",
            "downloadUrl",
            // ⚠️ AC-S-182 步骤 3 明确要求 `downloadUrl` **+ `size`** ——
            // 原本这条断言里没有 `size`，所以缺字段时测试是绿的。
            "size",
            "createdAt",
            "finishedAt",
        ] {
            assert!(json.get(key).is_some(), "缺 camelCase 字段：{key}");
        }
    }

    /// 错误回执 DTO 的字段名。
    #[test]
    fn the_job_error_dto_uses_the_documented_field_names() {
        let view = JobErrorView {
            row_number: 3,
            column: "email".into(),
            reason: "missing required value".into(),
        };
        let json = serde_json::to_value(&view).expect("序列化");
        for key in ["rowNumber", "column", "reason"] {
            assert!(json.get(key).is_some(), "缺 camelCase 字段：{key}");
        }
    }

    /// `required_field` 把空串也当成缺失。
    #[test]
    fn required_field_treats_empty_as_missing() {
        let mut map = BTreeMap::new();
        map.insert("a".to_string(), "x".to_string());
        map.insert("b".to_string(), String::new());
        assert_eq!(required_field(&map, "a"), Some("x"));
        assert_eq!(required_field(&map, "b"), None, "空串视为缺失");
        assert_eq!(required_field(&map, "c"), None);
    }
}
