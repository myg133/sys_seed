//! 文件存储的**平台错误码**（REQ-018，D330 / D125 的 8 处登记 / D226 的「无死码」纪律）。
//!
//! # 归属
//!
//! 与其他平台 crate 同款：码的**常量**住在本 crate，**标题**的唯一来源是
//! `platform_core::titles::TITLES`（D113）—— 面向用户的中文文案由前端 i18n 按 `code` 渲染。
//!
//! # ⚠️ 恰好 9 个 —— 每个都有一条 AC 会**真的触发**它（D330）
//!
//! | 码 | HTTP | 触发它的 AC |
//! |----|------|-------------|
//! | `STORAGE_OBJECT_NOT_FOUND` | 404 | AC-S-175 步骤 4 |
//! | `STORAGE_PRESIGN_EXPIRED` | 403 | AC-S-026 步骤 3 / AC-S-176 步骤 4 |
//! | `STORAGE_PRESIGN_INVALID` | 403 | AC-S-176 步骤 2 与 3 |
//! | `STORAGE_PART_MISSING` | 400 | AC-S-177 步骤 5 |
//! | `STORAGE_CHECKSUM_MISMATCH` | 409 | AC-S-177 步骤 6 |
//! | `STORAGE_UPLOAD_TOO_LARGE` | 413 | AC-S-178 步骤 2 |
//! | `STORAGE_UNSUPPORTED_FORMAT` | 415 | AC-S-179 步骤 2 与 3 |
//! | `STORAGE_FILE_INFECTED` | 422 | AC-S-178 步骤 4 |
//! | `STORAGE_TTL_OUT_OF_RANGE` | 400 | AC-S-026 步骤 5 |
//!
//! # ⚠️ 为什么区分 413 与 400（S-07）
//!
//! **413 = 资源超限**（请求本身**合法**，只是太大，RFC 9110 §15.5.14）；
//! **400 = 格式 / 取值非法**（请求本身**不合法**）。
//! → `UPLOAD_TOO_LARGE` 用 413；`UNSUPPORTED_FORMAT` 与 `TTL_OUT_OF_RANGE` 用 400。

use std::collections::BTreeMap;

use axum::http::StatusCode;
use platform_core::RequestContext;
use platform_core::problem::ProblemDetails;

/// 稳定的机器可读错误码（平台侧，`STORAGE_*`）。
pub mod code {
    /// 对象不存在（含「不属于当前租户」—— 两者对外不可区分，避免探测）。
    pub const OBJECT_NOT_FOUND: &str = "STORAGE_OBJECT_NOT_FOUND";
    /// 预签名已过期（**签名是对的**）。
    pub const PRESIGN_EXPIRED: &str = "STORAGE_PRESIGN_EXPIRED";
    /// 预签名被篡改（签名不符）。
    pub const PRESIGN_INVALID: &str = "STORAGE_PRESIGN_INVALID";
    /// complete 时缺片。
    pub const PART_MISSING: &str = "STORAGE_PART_MISSING";
    /// 合并后 sha256 与声明不符。
    pub const CHECKSUM_MISMATCH: &str = "STORAGE_CHECKSUM_MISMATCH";
    /// 超过 `storage.maxUploadBytes`。
    pub const UPLOAD_TOO_LARGE: &str = "STORAGE_UPLOAD_TOO_LARGE";
    /// 不支持的文件格式（本轮只支持 CSV，D320）。
    pub const UNSUPPORTED_FORMAT: &str = "STORAGE_UNSUPPORTED_FORMAT";
    /// 病毒扫描判定为感染。
    pub const FILE_INFECTED: &str = "STORAGE_FILE_INFECTED";
    /// 请求的 TTL 超上限或为 0。
    pub const TTL_OUT_OF_RANGE: &str = "STORAGE_TTL_OUT_OF_RANGE";
    /// `entity` 不在白名单里（复核 R-05）。
    pub const UNKNOWN_ENTITY: &str = "STORAGE_UNKNOWN_ENTITY";
}

/// 本 crate 的全部**平台**错误码（**完备性测试的输入**，D113）。
///
/// ⚠️ 4 个 `storage.*` 审计事件码**不进**本清单（D223）。
pub const ALL_CODES: &[&str] = &[
    code::OBJECT_NOT_FOUND,
    code::PRESIGN_EXPIRED,
    code::PRESIGN_INVALID,
    code::PART_MISSING,
    code::CHECKSUM_MISMATCH,
    code::UPLOAD_TOO_LARGE,
    code::UNSUPPORTED_FORMAT,
    code::FILE_INFECTED,
    code::TTL_OUT_OF_RANGE,
    code::UNKNOWN_ENTITY,
];

/// 文件存储的平台错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// 对象不存在。
    ObjectNotFound {
        /// 对象 key（**已过 path 校验**，可以回显）。
        object_key: String,
    },
    /// 预签名已过期。
    PresignExpired {
        /// 过期的 unix 时间戳（秒）。
        expired_at: i64,
    },
    /// 预签名被篡改。
    PresignInvalid {
        /// 面向开发者的英文原因（固定短语，**不得**回显 token 内容）。
        reason: &'static str,
    },
    /// 缺片。
    PartMissing {
        /// 缺失的片号（**升序**）。
        missing_parts: Vec<u32>,
    },
    /// checksum 不符。
    ChecksumMismatch {
        /// 客户端声明的摘要。
        expected: String,
        /// 实际算出的摘要。
        actual: String,
    },
    /// 超过上传大小上限。
    UploadTooLarge {
        /// 上限（字节）。
        max_bytes: i64,
        /// 实际大小（字节）。
        actual_bytes: i64,
    },
    /// 不支持的文件格式。
    UnsupportedFormat {
        /// 客户端给的扩展名（**已小写化**，如 `xlsx`）。
        extension: String,
    },
    /// `entity` 不在白名单里。
    ///
    /// ⚠️ **不是** `UnsupportedFormat`：格式是「文件长什么样」，
    /// 实体是「要写进哪张表」—— 混用会让排障时分不清是文件选错还是目标表选错。
    UnknownEntity {
        /// 客户端给的 entity。
        entity: String,
        /// 当前支持的 entity（逗号分隔，**回显给调用方**便于自查）。
        known: String,
    },
    /// 病毒扫描判定为感染。
    FileInfected {
        /// 命中的签名名（扫描器给的）。
        signature: String,
    },
    /// TTL 超上限或为 0。
    TtlOutOfRange {
        /// 请求的 TTL（秒）。
        requested: i64,
        /// 允许的最大值（秒）。
        max_seconds: i64,
    },
    /// **内部错误**（文件 IO 失败等）。
    ///
    /// ⚠️ **不进 [`ALL_CODES`]** —— 它映射到平台的 `INTERNAL_ERROR`（500），
    /// 不是 `STORAGE_*` 码。理由：IO 失败不是**调用方**能修的错，
    /// 给它一个 `STORAGE_*` 码就是**死码**（没有 AC 会触发它，D226 / D330）。
    Internal {
        /// 面向开发者的英文原因（**不含路径 / 租户信息**）。
        reason: &'static str,
    },
}

impl StorageError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::ObjectNotFound { .. } => code::OBJECT_NOT_FOUND,
            Self::PresignExpired { .. } => code::PRESIGN_EXPIRED,
            Self::PresignInvalid { .. } => code::PRESIGN_INVALID,
            Self::PartMissing { .. } => code::PART_MISSING,
            Self::ChecksumMismatch { .. } => code::CHECKSUM_MISMATCH,
            Self::UploadTooLarge { .. } => code::UPLOAD_TOO_LARGE,
            Self::UnsupportedFormat { .. } => code::UNSUPPORTED_FORMAT,
            Self::UnknownEntity { .. } => code::UNKNOWN_ENTITY,
            Self::FileInfected { .. } => code::FILE_INFECTED,
            Self::TtlOutOfRange { .. } => code::TTL_OUT_OF_RANGE,
            Self::Internal { .. } => platform_core::problem::CODE_INTERNAL_ERROR,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::ObjectNotFound { .. } => StatusCode::NOT_FOUND,
            Self::PresignExpired { .. } | Self::PresignInvalid { .. } => StatusCode::FORBIDDEN,
            Self::PartMissing { .. } | Self::TtlOutOfRange { .. } => StatusCode::BAD_REQUEST,
            Self::ChecksumMismatch { .. } => StatusCode::CONFLICT,
            Self::UploadTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnsupportedFormat { .. } => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            // ⚠️ 415 是「请求体里的**文件格式**不接受」；
            // 「目标实体不认识」属于**请求参数非法** → 400（不是 415）。
            Self::UnknownEntity { .. } => StatusCode::BAD_REQUEST,
            Self::FileInfected { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述（契约 C6）。
    ///
    /// ⚠️ 不得回显请求里的原始字节 / token 内容 / 文件内容。
    pub fn detail(&self) -> String {
        match self {
            Self::ObjectNotFound { object_key } => {
                format!("the object was not found: {object_key}")
            }
            Self::PresignExpired { expired_at } => {
                format!("the presigned link expired at {expired_at}")
            }
            Self::PresignInvalid { reason } => {
                format!("the presigned token is not valid: {reason}")
            }
            Self::PartMissing { missing_parts } => {
                let list: Vec<String> = missing_parts.iter().map(u32::to_string).collect();
                format!("required upload parts are missing: [{}]", list.join(", "))
            }
            Self::ChecksumMismatch { expected, actual } => {
                format!("checksum mismatch: expected {expected}, got {actual}")
            }
            Self::UploadTooLarge {
                max_bytes,
                actual_bytes,
            } => format!("the upload is too large: {actual_bytes} bytes exceeds {max_bytes}"),
            Self::UnsupportedFormat { extension } => format!(
                "the file format is not supported: `{extension}` - only csv is supported in this release"
            ),
            Self::UnknownEntity { entity, known } => format!(
                "the entity is not supported: `{entity}` - known entities are: {known}"
            ),
            Self::FileInfected { signature } => {
                format!("the uploaded file was rejected by the virus scanner: {signature}")
            }
            Self::TtlOutOfRange {
                requested,
                max_seconds,
            } => format!(
                "the requested ttl is out of range: {requested} (must be 1..={max_seconds})"
            ),
            Self::Internal { reason } => format!("an internal storage error occurred: {reason}"),
        }
    }

    /// 错误码之外的补充参数（契约 C6 的 `params`）。
    pub fn params(&self) -> BTreeMap<String, serde_json::Value> {
        let mut params = BTreeMap::new();
        match self {
            Self::ObjectNotFound { object_key } => {
                params.insert(
                    "objectKey".to_string(),
                    serde_json::Value::from(object_key.clone()),
                );
            }
            Self::PresignExpired { expired_at } => {
                params.insert(
                    "expiredAt".to_string(),
                    serde_json::Value::from(*expired_at),
                );
            }
            Self::PresignInvalid { reason } => {
                params.insert("reason".to_string(), serde_json::Value::from(*reason));
            }
            Self::PartMissing { missing_parts } => {
                params.insert(
                    "missingParts".to_string(),
                    serde_json::Value::from(missing_parts.clone()),
                );
            }
            Self::ChecksumMismatch { expected, actual } => {
                params.insert(
                    "expected".to_string(),
                    serde_json::Value::from(expected.clone()),
                );
                params.insert(
                    "actual".to_string(),
                    serde_json::Value::from(actual.clone()),
                );
            }
            Self::UploadTooLarge { max_bytes, .. } => {
                // ⚠️ **必须**带 `maxBytes`（AC-S-178 步骤 2 的判据 ①）
                params.insert("maxBytes".to_string(), serde_json::Value::from(*max_bytes));
            }
            Self::UnsupportedFormat { extension } => {
                params.insert(
                    "extension".to_string(),
                    serde_json::Value::from(extension.clone()),
                );
            }
            Self::UnknownEntity { entity, known } => {
                params.insert("entity".to_string(), serde_json::Value::from(entity.clone()));
                params.insert("known".to_string(), serde_json::Value::from(known.clone()));
            }
            Self::FileInfected { signature } => {
                // ⚠️ **必须**带 `signature`（AC-S-178 步骤 4 的判据）
                params.insert(
                    "signature".to_string(),
                    serde_json::Value::from(signature.clone()),
                );
            }
            Self::TtlOutOfRange { max_seconds, .. } => {
                params.insert(
                    "maxSeconds".to_string(),
                    serde_json::Value::from(*max_seconds),
                );
            }
            Self::Internal { .. } => {}
        }
        params
    }

    /// 把错误转成统一错误体（RFC 9457，契约 C6）。
    pub fn problem_details(&self, ctx: &RequestContext, instance: &str) -> ProblemDetails {
        let params = self.params();
        ProblemDetails::from_context(
            self.code(),
            platform_core::titles::title_of_code(self.code()),
            self.status().as_u16(),
            self.detail(),
            ctx,
            instance,
            if params.is_empty() {
                None
            } else {
                Some(params)
            },
        )
    }
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for StorageError {}

/// 服务层的错误：**存储专有错误** + **平台通用错误**。
///
/// `STORAGE_*` **恰好 9 个** —— 「没带令牌」「数据库挂了」**不是**存储专有的 →
/// 复用平台的 `UNAUTHENTICATED` / `INTERNAL_ERROR`（D226：不许为它们新增第 10 个码）。
#[derive(Debug)]
pub enum ServiceError {
    /// 存储专有错误。
    Storage(StorageError),
    /// 需要认证 → **401**。
    Unauthenticated,
    /// 内部错误（含数据访问 / IO 失败）→ **500**（**不漏内部细节**）。
    Internal {
        /// 面向开发者的英文原因（**不含**路径 / SQL / 凭据）。
        reason: String,
    },
}

impl ServiceError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Storage(err) => err.code(),
            Self::Unauthenticated => platform_core::problem::CODE_UNAUTHENTICATED,
            Self::Internal { .. } => platform_core::problem::CODE_INTERNAL_ERROR,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Storage(err) => err.status(),
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 面向开发者的英文 ASCII 描述。
    pub fn detail(&self) -> String {
        match self {
            Self::Storage(err) => err.detail(),
            Self::Unauthenticated => "authentication is required".to_string(),
            Self::Internal { reason } => format!("an internal error occurred: {reason}"),
        }
    }

    /// 把错误转成统一错误体。
    pub fn problem_details(&self, ctx: &RequestContext, instance: &str) -> ProblemDetails {
        match self {
            Self::Storage(err) => err.problem_details(ctx, instance),
            Self::Unauthenticated => ProblemDetails::unauthenticated(ctx, instance),
            Self::Internal { .. } => ProblemDetails::from_context(
                self.code(),
                platform_core::titles::title_of_code(self.code()),
                500,
                self.detail(),
                ctx,
                instance,
                None,
            ),
        }
    }
}

impl From<StorageError> for ServiceError {
    fn from(err: StorageError) -> Self {
        Self::Storage(err)
    }
}

/// 数据访问层失败 → **500**（只带 `DbError::code()`，不带 `Display`）。
impl From<platform_db::error::DbError> for ServiceError {
    fn from(err: platform_db::error::DbError) -> Self {
        tracing::error!(error = %err, "存储系统的数据库操作失败");
        Self::Internal {
            reason: format!("database error ({})", err.code()),
        }
    }
}

/// 文件 IO 失败 → [`StorageError::Internal`] → 平台 **500**
/// （**不回显路径**：路径里可能含租户信息）。
impl From<std::io::Error> for StorageError {
    fn from(err: std::io::Error) -> Self {
        tracing::error!(error = %err, "存储系统的文件操作失败");
        Self::Internal {
            reason: "a filesystem operation failed",
        }
    }
}

/// 文件 IO 失败 → **500**（保留给上层直接 `?` 用）。
impl From<std::io::Error> for ServiceError {
    fn from(err: std::io::Error) -> Self {
        tracing::error!(error = %err, "存储系统的文件操作失败");
        Self::Internal {
            reason: format!("filesystem error ({:?})", err.kind()),
        }
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for ServiceError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL_CODES` **恰好 10 条**且无重复（D330 基线 9 + R-05 的 `UNKNOWN_ENTITY`）。
    #[test]
    fn the_code_list_has_exactly_ten_distinct_codes() {
        assert_eq!(
            ALL_CODES.len(),
            10,
            "D330 基线 9 个 + R-05 新增 `STORAGE_UNKNOWN_ENTITY` = 10"
        );
        let unique: std::collections::BTreeSet<&str> = ALL_CODES.iter().copied().collect();
        assert_eq!(unique.len(), 10, "ALL_CODES 不得有重复");
    }

    /// 每个码都以 `STORAGE_` 开头。
    #[test]
    fn every_code_is_in_the_storage_namespace() {
        for code in ALL_CODES {
            assert!(
                code.starts_with("STORAGE_"),
                "错误码必须以 STORAGE_ 开头：{code}"
            );
        }
    }

    /// 每个码都是纯 ASCII 大写下划线。
    #[test]
    fn every_code_is_ascii_upper_snake() {
        for code in ALL_CODES {
            assert!(
                code.chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                "错误码必须是 ASCII 大写下划线：{code}"
            );
        }
    }

    /// 10 个变体各映射到**互不相同**的码。
    #[test]
    fn each_variant_maps_to_a_distinct_code() {
        let errors = sample_errors();
        let codes: std::collections::BTreeSet<&str> =
            errors.iter().map(StorageError::code).collect();
        assert_eq!(codes.len(), 10, "10 个变体必须映射到 10 个不同的码");
        assert_eq!(codes.len(), ALL_CODES.len());
    }

    /// 每个变体的码都登记在 `ALL_CODES`（防漏登记 → i18n 测试假绿）。
    #[test]
    fn every_variant_code_is_registered() {
        for err in sample_errors() {
            assert!(
                ALL_CODES.contains(&err.code()),
                "变体 {} 的码没登记进 ALL_CODES",
                err.code()
            );
        }
    }

    /// HTTP 状态码逐条核对 —— **特别是 413 vs 400 的区分**（S-07）。
    #[test]
    fn every_code_maps_to_the_documented_status() {
        assert_eq!(
            StorageError::ObjectNotFound {
                object_key: "k".into()
            }
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            StorageError::PresignExpired { expired_at: 0 }.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            StorageError::PresignInvalid { reason: "x" }.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            StorageError::PartMissing {
                missing_parts: vec![2]
            }
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            StorageError::ChecksumMismatch {
                expected: "a".into(),
                actual: "b".into()
            }
            .status(),
            StatusCode::CONFLICT
        );
        // ⚠️ 资源超限 → 413（不是 400）
        assert_eq!(
            StorageError::UploadTooLarge {
                max_bytes: 1,
                actual_bytes: 2
            }
            .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        // ⚠️ 格式不支持 → 415
        assert_eq!(
            StorageError::UnsupportedFormat {
                extension: "xlsx".into()
            }
            .status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(
            StorageError::FileInfected {
                signature: "EICAR".into()
            }
            .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        // ⚠️ 取值非法 → 400（不是 413）
        assert_eq!(
            StorageError::TtlOutOfRange {
                requested: 0,
                max_seconds: 86400
            }
            .status(),
            StatusCode::BAD_REQUEST
        );
    }

    /// 所有 `detail()` 都是 ASCII（契约 C6）。
    #[test]
    fn every_detail_is_ascii() {
        for err in sample_errors() {
            assert!(
                err.detail().is_ascii(),
                "detail 必须是 ASCII：{} → {}",
                err.code(),
                err.detail()
            );
        }
    }

    /// `UploadTooLarge` 的 `params` **必须**带 `maxBytes`（AC-S-178 判据 ①）。
    #[test]
    fn the_too_large_error_carries_max_bytes() {
        let err = StorageError::UploadTooLarge {
            max_bytes: 1048576,
            actual_bytes: 1048577,
        };
        assert_eq!(
            err.params().get("maxBytes").and_then(|v| v.as_i64()),
            Some(1048576),
            "必须带 maxBytes 参数"
        );
    }

    /// `FileInfected` 的 `params` **必须**带 `signature`（AC-S-178 判据）。
    #[test]
    fn the_infected_error_carries_the_signature() {
        let err = StorageError::FileInfected {
            signature: "EICAR-Test-File".into(),
        };
        assert_eq!(
            err.params().get("signature").and_then(|v| v.as_str()),
            Some("EICAR-Test-File")
        );
    }

    /// `PartMissing` 的 `params.missingParts` 是**数组**且升序（AC-S-177 步骤 5）。
    #[test]
    fn the_part_missing_error_lists_part_numbers() {
        let err = StorageError::PartMissing {
            missing_parts: vec![2, 5],
        };
        let params = err.params();
        let value = params.get("missingParts").expect("必须有 missingParts");
        assert_eq!(value, &serde_json::json!([2, 5]));
    }

    /// `UnsupportedFormat` 的 detail 含「only csv」字样（AC-S-179 步骤 2 判据）。
    #[test]
    fn the_unsupported_format_error_mentions_csv_only() {
        let err = StorageError::UnsupportedFormat {
            extension: "xlsx".into(),
        };
        let detail = err.detail();
        assert!(
            detail.contains("only csv is supported"),
            "detail 必须说明只支持 csv：{detail}"
        );
    }

    /// 预设的 9 个错误样本（供多个测试复用）。
    fn sample_errors() -> Vec<StorageError> {
        vec![
            StorageError::ObjectNotFound {
                object_key: "docs/a.txt".into(),
            },
            StorageError::PresignExpired { expired_at: 0 },
            StorageError::PresignInvalid {
                reason: "signature mismatch",
            },
            StorageError::PartMissing {
                missing_parts: vec![2],
            },
            StorageError::ChecksumMismatch {
                expected: "a".into(),
                actual: "b".into(),
            },
            StorageError::UploadTooLarge {
                max_bytes: 1,
                actual_bytes: 2,
            },
            StorageError::UnsupportedFormat {
                extension: "xlsx".into(),
            },
            StorageError::FileInfected {
                signature: "EICAR".into(),
            },
            StorageError::TtlOutOfRange {
                requested: 0,
                max_seconds: 86400,
            },
            StorageError::UnknownEntity {
                entity: "users".into(),
                known: "probe_row".into(),
            },
        ]
    }
}
