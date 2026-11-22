//! 编排层（REQ-018，design §4）。
//!
//! # 职责
//!
//! 把「HTTP 请求」变成「存储调用 + 数据库记录」：
//!
//! | 场景 | 流程 |
//! |---|---|
//! | 直接上传 | 大小校验 → 扫描 → 落存储 → UPSERT 元数据 |
//! | 分片上传 | 建会话 → 逐片落存储 → complete 时校验缺片 + sha256 |
//! | 预签名 | 读元数据 → 签发 token |
//! | 建导入 | 建任务（pending）→ worker 处理 |
//! | 建导出 | 建任务（pending）→ worker 处理 |
//!
//! # ⚠️ 三条不可妥协的顺序
//!
//! 1. **大小校验在写盘之前**（AC-S-178 步骤 2 的判据 ②：超限时**落盘目录为空**）；
//! 2. **扫描在落库之前**（感染的文件绝不入库，AC-S-178 步骤 4）；
//! 3. **`checksum` 校验在落对象之前**（坏文件绝不覆盖好文件，AC-S-177 步骤 6）。

use std::sync::Arc;

use bytes::Bytes;
use platform_core::RequestContext;
use platform_core::extension::audit::{AuditEvent, AuditSink, AuditSinkSlot};
use time::OffsetDateTime;

use crate::config::StorageConfig;
use crate::error::{ServiceError, StorageError};
use crate::model::{
    CreateJobBody, CreateUploadBody, JobRow, ObjectRow, UploadRow, job_kind, upload_status,
};
use crate::object_store::{ObjectStore, is_safe_object_key, presign, sha256_hex};
use crate::scanner::VirusScanner;
use crate::store::{NewJob, NewObject, NewUpload, RowFailure, StorageStore};

/// 存储服务的依赖。
///
/// ⚠️ 用**显式字段**而不是 `Arc<dyn Trait>` 的组合 —— 便于测试里逐个替换。
pub struct StorageService {
    store: StorageStore,
    config: StorageConfig,
    objects: Arc<dyn ObjectStore>,
    scanner: Arc<dyn VirusScanner>,
    /// 预签名密钥。
    presign_secret: Vec<u8>,
    /// 密钥标识（用于轮换）。
    presign_key_id: String,
    /// 审计 sink 槽（D334）。
    ///
    /// ⚠️ 用 **`AuditSinkSlot`（`OnceLock`）而不是必填构造参数** ——
    /// 与 `platform-iam` / `platform-rbac` 同款（D150：业务模块只认 core 的 trait）。
    /// 好处：现有测试与裸用服务的构造点**不用改**，未装配时只是不记审计。
    /// 代价：装配漏了会在运行期才发现 → 所以「未装配」必须打 **warn**
    /// （「以为在审计其实没有」比「没有审计」更危险）。
    audit: AuditSinkSlot,
}

impl StorageService {
    /// 构造。
    pub fn new(
        store: StorageStore,
        config: StorageConfig,
        objects: Arc<dyn ObjectStore>,
        scanner: Arc<dyn VirusScanner>,
        presign_secret: Vec<u8>,
        presign_key_id: String,
    ) -> Self {
        Self {
            store,
            config,
            objects,
            scanner,
            presign_secret,
            presign_key_id,
            audit: AuditSinkSlot::default(),
        }
    }

    /// 装配审计 sink（D334）。
    ///
    /// 返回 `false` 表示**已经装配过**（`OnceLock` 只能设一次）——
    /// 与 `platform-iam::with_audit_sink` 同款语义。
    pub fn with_audit_sink(&self, sink: Arc<dyn AuditSink>) -> bool {
        self.audit.set(sink)
    }

    /// 读底层 store（worker 用）。
    pub fn store(&self) -> &StorageStore {
        &self.store
    }

    /// 读配置（worker 用）。
    pub fn config(&self) -> &StorageConfig {
        &self.config
    }

    /// 读对象存储（worker 用）。
    pub fn objects(&self) -> &Arc<dyn ObjectStore> {
        &self.objects
    }

    /// 读扫描器（worker 用）。
    pub fn scanner(&self) -> &Arc<dyn VirusScanner> {
        &self.scanner
    }

    // -- 审计（D334）--------------------------------------------------------

    /// 记一条存储审计事件（D334 / AC-S-184 步骤 5）。
    ///
    /// # 失败语义：**审计失败不影响主流程**
    ///
    /// 与 IAM 的登录审计**不同**（那边是「宁可不登录，不可无审计地登录」，
    /// 因为登录本身是安全准入事件）。存储的上传/删除不会因为审计写不进去就失败 ——
    /// 拒掉用户的上传只会让业务不可用，而审计的目的是**事后追溯**，不是准入控制。
    /// 所以这里只 `warn`，不返回错误。
    ///
    /// # 未装配时也 `warn`
    ///
    /// 「以为在审计其实没有」比「没有审计」更危险 —— 必须留下痕迹。
    pub(crate) async fn record_audit(&self, ctx: &RequestContext, event: AuditEvent) {
        let Some(sink) = self.audit.get() else {
            tracing::warn!(
                event = %event.action,
                "a storage audit event was produced but no platform audit sink is installed; \
                 the event is NOT persisted"
            );
            return;
        };
        if let Err(error) = sink.record(ctx, &event).await {
            tracing::warn!(
                error = %error,
                event = %event.action,
                "the storage audit event could not be recorded"
            );
        }
    }

    /// 造一条存储审计事件（填好 tenant / actor / IP / UA）。
    pub(crate) fn audit_event(
        &self,
        ctx: &RequestContext,
        action: &str,
        object_type: &str,
        object_id: Option<&str>,
        after: Option<serde_json::Value>,
    ) -> AuditEvent {
        AuditEvent {
            // 无租户上下文时落空串（免鉴权端点如预签名下载就没有租户）。
            tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
            actor_id: ctx.user_id().map(str::to_string),
            action: action.to_string(),
            object_type: object_type.to_string(),
            object_id: object_id.map(str::to_string),
            before: None,
            after,
            // ⚠️ 取不到时落**空串**而不是 `NULL`（契约 C12，同 platform-app）。
            ip: Some(ctx.client_ip().unwrap_or_default().to_string()),
            user_agent: Some(ctx.user_agent().unwrap_or_default().to_string()),
        }
    }

    // -- 直接上传 -----------------------------------------------------------

    /// 上传一个对象（**直接 PUT**）。
    ///
    /// # 顺序（不可换）
    ///
    /// ① 大小校验（**在写盘之前**）→ ② 扫描 → ③ 落存储 → ④ UPSERT 元数据。
    pub async fn put_object(
        &self,
        ctx: &RequestContext,
        key: &str,
        bytes: Bytes,
        content_type: &str,
    ) -> Result<ObjectRow, ServiceError> {
        if !is_safe_object_key(key) {
            return Err(StorageError::ObjectNotFound {
                object_key: key.to_string(),
            }
            .into());
        }
        let settings = self.config.settings(ctx).await;

        // ① 大小校验 —— **必须在写盘之前**（否则超限的请求会留下残骸）。
        let actual = bytes.len() as i64;
        if actual > settings.max_upload_bytes {
            return Err(StorageError::UploadTooLarge {
                max_bytes: settings.max_upload_bytes,
                actual_bytes: actual,
            }
            .into());
        }

        // ② 扫描 —— 感染则**不落盘也不落库**。
        let verdict = self
            .scanner
            .scan(&bytes)
            .await
            .map_err(|reason| ServiceError::Internal {
                reason: format!("the virus scanner failed: {reason}"),
            })?;
        if let Some(signature) = verdict.signature() {
            return Err(StorageError::FileInfected {
                signature: signature.to_string(),
            }
            .into());
        }

        // ③ 落存储。
        self.objects.put(key, bytes.clone()).await?;

        // ④ UPSERT 元数据。
        let new = NewObject {
            id: uuid::Uuid::new_v4().to_string(),
            object_key: key.to_string(),
            size_bytes: actual,
            sha256: sha256_hex(&bytes),
            content_type: content_type.to_string(),
            scan_verdict: verdict.as_str().to_string(),
        };
        let row = self.store.upsert_object(ctx, &new).await?;

        // ⑤ 审计（D334 / AC-S-184 步骤 5）。
        // 放在落库**之后**：审计的是「已发生的事实」，写库失败就不该记一条成功审计。
        let event = self.audit_event(
            ctx,
            crate::events::FILE_UPLOADED,
            crate::events::OBJECT_TYPE_OBJECT,
            Some(&row.id),
            Some(serde_json::json!({
                "objectKey": row.object_key,
                "sizeBytes": row.size_bytes,
                "sha256": row.sha256,
                "scanVerdict": row.scan_verdict,
            })),
        );
        self.record_audit(ctx, event).await;
        Ok(row)
    }

    /// 读对象内容。
    pub async fn get_object(
        &self,
        ctx: &RequestContext,
        key: &str,
    ) -> Result<(ObjectRow, Bytes), ServiceError> {
        // 先查元数据（**同时起到租户校验作用**：别人的 key 查不到 → 404）。
        let row = self.store.find_object(ctx, key).await?.ok_or_else(|| {
            StorageError::ObjectNotFound {
                object_key: key.to_string(),
            }
        })?;
        let bytes = self.objects.get(key).await?;
        Ok((row, bytes))
    }

    /// 删对象（元数据 + 内容）。
    pub async fn delete_object(&self, ctx: &RequestContext, key: &str) -> Result<(), ServiceError> {
        let affected = self.store.delete_object(ctx, key).await?;
        if affected == 0 {
            return Err(StorageError::ObjectNotFound {
                object_key: key.to_string(),
            }
            .into());
        }
        self.objects.delete(key).await?;

        // 审计（D334）：删除只记 key —— 对象内容已经没了，
        // 记 size/sha256 只会让人以为还能找回。
        let event = self.audit_event(
            ctx,
            crate::events::FILE_DELETED,
            crate::events::OBJECT_TYPE_OBJECT,
            Some(key),
            Some(serde_json::json!({ "objectKey": key })),
        );
        self.record_audit(ctx, event).await;
        Ok(())
    }

    /// 列出对象。
    pub async fn list_objects(&self, ctx: &RequestContext) -> Result<Vec<ObjectRow>, ServiceError> {
        Ok(self.store.list_objects(ctx).await?)
    }

    // -- 预签名 -------------------------------------------------------------

    /// 签发预签名 URL。
    pub async fn presign_object(
        &self,
        ctx: &RequestContext,
        key: &str,
        ttl_seconds: Option<i64>,
    ) -> Result<(String, OffsetDateTime), ServiceError> {
        // 元数据必须存在（防「对任意 key 签链接」）。
        self.store
            .find_object(ctx, key)
            .await?
            .ok_or_else(|| StorageError::ObjectNotFound {
                object_key: key.to_string(),
            })?;

        let settings = self.config.settings(ctx).await;
        let ttl = ttl_seconds.unwrap_or(settings.presign_ttl_seconds);
        let now = OffsetDateTime::now_utc();
        let (token, expires_at_unix) = presign::issue(
            &self.presign_secret,
            &self.presign_key_id,
            key,
            ttl,
            now.unix_timestamp(),
        )?;
        let expires_at = OffsetDateTime::from_unix_timestamp(expires_at_unix).map_err(|_| {
            ServiceError::Internal {
                reason: "the computed expiry is out of range".to_string(),
            }
        })?;
        Ok((format!("/api/storage/download/{token}"), expires_at))
    }

    /// 校验预签名令牌（**免鉴权下载端点用**）。
    pub async fn verify_presign(&self, token: &str) -> Result<String, ServiceError> {
        let verified = presign::verify(
            &self.presign_secret,
            token,
            OffsetDateTime::now_utc().unix_timestamp(),
        )?;
        Ok(verified.object_key)
    }

    // -- 分片上传 -----------------------------------------------------------

    /// 建上传会话。
    pub async fn create_upload(
        &self,
        ctx: &RequestContext,
        body: &CreateUploadBody,
    ) -> Result<UploadRow, ServiceError> {
        if !is_safe_object_key(&body.object_key) {
            return Err(StorageError::ObjectNotFound {
                object_key: body.object_key.clone(),
            }
            .into());
        }
        if body.total_parts <= 0 {
            return Err(StorageError::PartMissing {
                missing_parts: vec![],
            }
            .into());
        }
        let settings = self.config.settings(ctx).await;
        // 预估总大小也要在上限内（否则分片传完才发现超限，浪费带宽）。
        let estimated = body.part_size.saturating_mul(i64::from(body.total_parts));
        if estimated > settings.max_upload_bytes {
            return Err(StorageError::UploadTooLarge {
                max_bytes: settings.max_upload_bytes,
                actual_bytes: estimated,
            }
            .into());
        }

        // ⚠️ **单片上限**（D335 / 复核 F-04）：`storage.partSizeBytes` 是**硬上限**，
        // 不是「推荐值」。
        //
        // 为什么必须有服务端上限（客户端说了不算）：
        // 1. 客户端声明 `totalParts = 1_000_000, partSize = 1`（差一字节一片）
        //    → 在磁盘上制造上百万个小文件（**inode 耗尽**，比磁盘满更难处理）；
        // 2. 反过来声明一个超大单片 → 绕过 `maxUploadBytes` 的**逐片**检查
        //    （上面只校验 `partSize × totalParts` 的**乘积**，单片本身没有上限）；
        // 3. 单片过大还会让 `put_object` 的内存压力失控（见 TD-066）。
        //
        // ⚠️ 复核 F-04 指出：改这个配置项原本**对行为无影响**（零读取）
        // → 违反 D331「6 个配置项每个必须被真实读取」。这一处就是它的真实读取点。
        if body.part_size > settings.part_size_bytes {
            return Err(StorageError::UploadTooLarge {
                max_bytes: settings.part_size_bytes,
                actual_bytes: body.part_size,
            }
            .into());
        }
        if body.part_size <= 0 {
            return Err(StorageError::UploadTooLarge {
                max_bytes: settings.part_size_bytes,
                actual_bytes: body.part_size,
            }
            .into());
        }

        let upload_id = uuid::Uuid::new_v4().to_string();
        self.objects
            .create_multipart(&upload_id, &body.object_key, body.total_parts)
            .await?;

        let new = NewUpload {
            upload_id,
            object_key: body.object_key.clone(),
            total_parts: body.total_parts,
            part_size: body.part_size,
            declared_checksum: body.checksum.clone(),
            expires_at: OffsetDateTime::now_utc()
                + time::Duration::seconds(settings.upload_session_ttl_seconds),
        };
        Ok(self.store.insert_upload(ctx, &new).await?)
    }

    /// 上传一个分片（**幂等**）。
    pub async fn upload_part(
        &self,
        ctx: &RequestContext,
        upload_id: &str,
        part_number: u32,
        bytes: Bytes,
    ) -> Result<(), ServiceError> {
        let session = self
            .store
            .find_upload(ctx, upload_id)
            .await?
            .ok_or_else(|| StorageError::ObjectNotFound {
                object_key: upload_id.to_string(),
            })?;
        if session.status != upload_status::PENDING {
            return Err(StorageError::ObjectNotFound {
                object_key: format!("the upload session is {}", session.status),
            }
            .into());
        }
        if part_number == 0 || part_number > session.total_parts.max(0) as u32 {
            return Err(StorageError::PartMissing {
                missing_parts: vec![part_number],
            }
            .into());
        }
        self.objects
            .upload_part(upload_id, part_number, bytes)
            .await?;
        Ok(())
    }

    /// 完成分片上传（**缺片检测 + sha256 校验**）。
    pub async fn complete_upload(
        &self,
        ctx: &RequestContext,
        upload_id: &str,
        content_type: Option<&str>,
    ) -> Result<ObjectRow, ServiceError> {
        let session = self
            .store
            .find_upload(ctx, upload_id)
            .await?
            .ok_or_else(|| StorageError::ObjectNotFound {
                object_key: upload_id.to_string(),
            })?;
        if session.status != upload_status::PENDING {
            return Err(StorageError::ObjectNotFound {
                object_key: format!("the upload session is {}", session.status),
            }
            .into());
        }

        // 缺片检测（`complete_multipart` 内部会做，且会返回 `PART_MISSING`）。
        let actual_sha = self
            .objects
            .complete_multipart(upload_id, &session.object_key, session.total_parts)
            .await?;

        // ⚠️ **sha256 校验在落库之前**：不符就删掉刚合并的对象（不能留坏文件）。
        if let Some(expected) = session.declared_checksum.as_ref()
            && !expected.eq_ignore_ascii_case(&actual_sha)
        {
            // 清掉刚合并出来的坏对象。
            let _ = self.objects.delete(&session.object_key).await;
            self.store
                .update_upload_status(ctx, upload_id, upload_status::ABORTED)
                .await?;
            return Err(StorageError::ChecksumMismatch {
                expected: expected.clone(),
                actual: actual_sha,
            }
            .into());
        }

        let size = self
            .objects
            .get(&session.object_key)
            .await
            .map(|b| b.len() as i64)
            .unwrap_or(0);

        // ⚠️ **合并后必须扫描**（复核 F-17，安全）。
        //
        // 原来的实现直接 `scan_verdict: Skipped` 落库 —— 而分片是**大文件的唯一通道**，
        // 这意味着「配了真扫描器也能把恶意文件经 `/uploads/*` 落库」，
        // 绕开了 AC-S-178 步骤 4（直传路径是扫的）。
        //
        // 顺序不可换：**扫描在落库之前**，且感染要**删掉刚合并的文件**
        // （否则磁盘上留一个可被预签名链接取回、但数据库里没有记录的文件）。
        let scan_bytes = self
            .objects
            .get(&session.object_key)
            .await
            .map_err(ServiceError::Storage)?;
        let verdict = self
            .scanner
            .scan(&scan_bytes)
            .await
            .map_err(|reason| ServiceError::Internal {
                reason: format!("the virus scanner failed: {reason}"),
            })?;
        if let Some(signature) = verdict.signature() {
            // 清掉合并出来的感染文件 + 把会话标成 aborted（不留半成品状态）。
            let _ = self.objects.delete(&session.object_key).await;
            self.store
                .update_upload_status(ctx, upload_id, upload_status::ABORTED)
                .await?;
            return Err(StorageError::FileInfected {
                signature: signature.to_string(),
            }
            .into());
        }

        let new = NewObject {
            id: uuid::Uuid::new_v4().to_string(),
            object_key: session.object_key.clone(),
            size_bytes: size,
            sha256: actual_sha,
            content_type: content_type
                .unwrap_or("application/octet-stream")
                .to_string(),
            // 如实记扫描器的判定（`skipped` / `clean` / `infected` 三选一）。
            scan_verdict: verdict.as_str().to_string(),
        };
        let row = self.store.upsert_object(ctx, &new).await?;
        self.store
            .update_upload_status(ctx, upload_id, upload_status::COMPLETED)
            .await?;
        Ok(row)
    }

    /// 中止分片上传。
    pub async fn abort_upload(
        &self,
        ctx: &RequestContext,
        upload_id: &str,
    ) -> Result<(), ServiceError> {
        let session = self
            .store
            .find_upload(ctx, upload_id)
            .await?
            .ok_or_else(|| StorageError::ObjectNotFound {
                object_key: upload_id.to_string(),
            })?;
        if session.status == upload_status::COMPLETED {
            return Err(StorageError::ObjectNotFound {
                object_key: "the upload session is already completed".to_string(),
            }
            .into());
        }
        self.objects.abort_multipart(upload_id).await?;
        self.store
            .update_upload_status(ctx, upload_id, upload_status::ABORTED)
            .await?;
        Ok(())
    }

    // -- 任务 ---------------------------------------------------------------

    /// 建导入任务（**202 pending**）。
    pub async fn create_import(
        &self,
        ctx: &RequestContext,
        body: &CreateJobBody,
    ) -> Result<JobRow, ServiceError> {
        let key = body.object_key.as_deref().ok_or_else(|| {
            ServiceError::Storage(StorageError::ObjectNotFound {
                object_key: "(missing objectKey for import)".to_string(),
            })
        })?;
        // 源对象必须存在（否则异步跑起来才发现 → 用户等半天才报错）。
        self.store
            .find_object(ctx, key)
            .await?
            .ok_or_else(|| StorageError::ObjectNotFound {
                object_key: key.to_string(),
            })?;
        // ⚠️ 格式校验在最前面：非 .csv 立即 415（不等 worker）。
        ensure_csv_key(key)?;

        let new = NewJob {
            id: uuid::Uuid::new_v4().to_string(),
            kind: job_kind::IMPORT.to_string(),
            entity: body.entity.clone(),
            object_key: Some(key.to_string()),
        };
        Ok(self.store.insert_job(ctx, &new).await?)
    }

    /// 建导出任务。
    pub async fn create_export(
        &self,
        ctx: &RequestContext,
        body: &CreateJobBody,
    ) -> Result<JobRow, ServiceError> {
        let new = NewJob {
            id: uuid::Uuid::new_v4().to_string(),
            kind: job_kind::EXPORT.to_string(),
            entity: body.entity.clone(),
            object_key: None,
        };
        Ok(self.store.insert_job(ctx, &new).await?)
    }

    /// 读任务。
    pub async fn find_job(
        &self,
        ctx: &RequestContext,
        job_id: &str,
    ) -> Result<JobRow, ServiceError> {
        self.store.find_job(ctx, job_id).await?.ok_or_else(|| {
            StorageError::ObjectNotFound {
                object_key: job_id.to_string(),
            }
            .into()
        })
    }

    /// 列任务。
    pub async fn list_jobs(&self, ctx: &RequestContext) -> Result<Vec<JobRow>, ServiceError> {
        Ok(self.store.list_jobs(ctx).await?)
    }

    /// 列某任务的错误回执。
    pub async fn list_job_errors(
        &self,
        ctx: &RequestContext,
        job_id: &str,
    ) -> Result<Vec<crate::model::JobErrorRow>, ServiceError> {
        Ok(self.store.list_job_errors(ctx, job_id).await?)
    }
}

/// 只接受 `.csv`（D320：Excel 显式拒绝）。
///
/// ⚠️ **不是**「静默当 CSV 解析」——`.xlsx` 的内容是 ZIP，当 CSV 解析会得到乱码行，
/// 用户会以为「文件坏了」而不是「格式不支持」。
pub fn ensure_csv_key(key: &str) -> Result<(), StorageError> {
    let lower = key.to_ascii_lowercase();
    if lower.ends_with(".csv") {
        return Ok(());
    }
    // 取出扩展名（无扩展名 → 空串）。
    let extension = lower
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or("")
        .to_string();
    Err(StorageError::UnsupportedFormat { extension })
}

/// 导入一行的校验结果（worker 用）。
pub type RowValidation = Result<Vec<(String, String)>, RowFailure>;

#[cfg(test)]
mod tests {
    use super::*;

    /// `.csv` 通过（含大写扩展名）。
    #[test]
    fn csv_keys_are_accepted() {
        for key in ["a.csv", "dir/b.CSV", "x.y.csv"] {
            assert!(ensure_csv_key(key).is_ok(), "应接受：{key}");
        }
    }

    /// Excel 被拒，且**错误里带扩展名**（AC-S-179 步骤 2 的判据）。
    #[test]
    fn excel_keys_are_rejected_with_the_extension() {
        for (key, expected_ext) in [("a.xlsx", "xlsx"), ("b.xls", "xls"), ("c.XLSX", "xlsx")] {
            let err = ensure_csv_key(key).expect_err("必须拒绝");
            assert_eq!(err.code(), "STORAGE_UNSUPPORTED_FORMAT", "key={key}");
            assert_eq!(
                err.params().get("extension").and_then(|v| v.as_str()),
                Some(expected_ext),
                "扩展名必须逐字出现在 params 里"
            );
            assert!(
                err.detail().contains("only csv is supported"),
                "detail 必须说明只支持 csv：{}",
                err.detail()
            );
        }
    }

    /// 无扩展名也被拒（扩展名是空串）。
    #[test]
    fn a_key_without_extension_is_rejected() {
        let err = ensure_csv_key("noext").expect_err("必须拒绝");
        assert_eq!(
            err.params().get("extension").and_then(|v| v.as_str()),
            Some(""),
            "无扩展名时 extension 是空串"
        );
    }
}
