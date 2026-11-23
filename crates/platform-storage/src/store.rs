//! 数据访问（REQ-018，design §2）。
//!
//! # 租户作用域
//!
//! 四张表都带 `tenant_id` → 语句里**必须**有 `tenant_id = {tenant}`（RLS 兜底 + 应用层兜底）。
//!
//! # ⚠️ `"column"` 是保留字
//!
//! `platform_storage_job_error.column` 在 SQL 里**必须**写双引号 `"column"`
//! （`column` 是 SQL 标准保留字）。这里所有相关语句都已经加引号 ——
//! 漏一个就会在运行时炸（而且报错信息很难看出是这个原因）。
//!
//! # ⚠️ 进度必须**每批更新**（D325 / AC-S-180）
//!
//! [`StorageStore::update_job_progress`] 由 worker **每批调一次**。
//! 只在最后写一次 → 「进度可查」是假的（TC-018-040 会红）。

use platform_core::RequestContext;
use platform_db::{DbResult, TenantDb, TenantQuery};
use time::OffsetDateTime;

use crate::model::{
    JOB_ERROR_TABLE, JOB_TABLE, JobErrorRow, JobRow, OBJECT_TABLE, ObjectRow, PROBE_ROW_TABLE,
    UPLOAD_TABLE, UploadRow,
};

/// 待写入目标实体表的一行（D333）。
///
/// ⚠️ 字段就是 `platform_storage_probe_row` 的业务列（`email` / `amount` / `note`），
/// **不含** `id` / `tenant_id` / `job_id` / `created_at` ——
/// 那四个由写入时补齐（`id` 是新 uuid，`tenant_id` 走 `TenantQuery`，`job_id` 是当前任务）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRow {
    /// 必填列（AC-S-181 要求列名是真实的 `email`）。
    pub email: String,
    /// 数字列（AC-S-181 要求列名是真实的 `amount`）。
    pub amount: i64,
    /// 可选列（承载含逗号 / 含换行的 round-trip 字段）。
    pub note: Option<String>,
}

/// 对象表的列清单（**库层真列名**，DDL 断言用）。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn object_columns() -> &'static str {
    "id, tenant_id, object_key, size_bytes, sha256, content_type, scan_verdict, status, \
     created_at, updated_at"
}

/// 查询用的对象列清单（`id` 是 `uuid`，转 `text` 以匹配 `String` 字段）。
fn object_select_columns() -> &'static str {
    "id::text AS id, tenant_id, object_key, size_bytes, sha256, content_type, scan_verdict, \
     status, created_at, updated_at"
}

/// 上传会话表的列清单。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn upload_columns() -> &'static str {
    "upload_id, tenant_id, object_key, total_parts, part_size, declared_checksum, status, \
     created_at, updated_at, expires_at"
}

/// 查询用的上传会话列清单。
fn upload_select_columns() -> &'static str {
    "upload_id::text AS upload_id, tenant_id, object_key, total_parts, part_size, \
     declared_checksum, status, created_at, updated_at, expires_at"
}

/// 任务表的列清单。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn job_columns() -> &'static str {
    "id, tenant_id, kind, entity, status, total_rows, processed_rows, failed_rows, object_key, \
     error, created_at, updated_at, finished_at"
}

/// 查询用的任务列清单。
fn job_select_columns() -> &'static str {
    "id::text AS id, tenant_id, kind, entity, status, total_rows, processed_rows, failed_rows, \
     object_key, error, created_at, updated_at, finished_at"
}

/// 错误回执表的列清单（⚠️ `column` 带引号）。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn job_error_columns() -> &'static str {
    "job_id, tenant_id, row_number, \"column\", reason"
}

/// 待写入的对象。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewObject {
    /// 对象 ID。
    pub id: String,
    /// 逻辑路径。
    pub object_key: String,
    /// 字节数。
    pub size_bytes: i64,
    /// sha256（小写 hex）。
    pub sha256: String,
    /// MIME。
    pub content_type: String,
    /// 扫描判定。
    pub scan_verdict: String,
}

/// 待写入的上传会话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUpload {
    /// 会话 ID。
    pub upload_id: String,
    /// 目标 key。
    pub object_key: String,
    /// 分片总数。
    pub total_parts: i32,
    /// 每片字节数。
    pub part_size: i64,
    /// 声明的 checksum。
    pub declared_checksum: Option<String>,
    /// 过期时间。
    pub expires_at: OffsetDateTime,
}

/// 待写入的任务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJob {
    /// 任务 ID。
    pub id: String,
    /// 种类（`import` / `export`）。
    pub kind: String,
    /// 实体。
    pub entity: String,
    /// 源 / 结果对象 key。
    pub object_key: Option<String>,
}

/// 一行的校验失败（worker 用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowFailure {
    /// **文件行号**（1-based，含表头）。
    pub row_number: i64,
    /// 列名。
    pub column: String,
    /// 原因（ASCII）。
    pub reason: String,
}

/// 存储的数据访问。
pub struct StorageStore {
    db: TenantDb,
}

impl StorageStore {
    /// 用连接池构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 底层 `TenantDb`。
    pub fn db(&self) -> &TenantDb {
        &self.db
    }

    // -- 对象 ---------------------------------------------------------------

    /// 写（UPSERT）一个对象。
    pub async fn upsert_object(
        &self,
        ctx: &RequestContext,
        new: &NewObject,
    ) -> DbResult<ObjectRow> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
               (id, tenant_id, object_key, size_bytes, sha256, content_type, scan_verdict, \
                status, created_at, updated_at) \
             VALUES (?::uuid, {{tenant}}, ?, ?, ?, ?, ?, ?, now(), now()) \
             ON CONFLICT (tenant_id, object_key) DO UPDATE SET \
               size_bytes = EXCLUDED.size_bytes, \
               sha256 = EXCLUDED.sha256, \
               content_type = EXCLUDED.content_type, \
               scan_verdict = EXCLUDED.scan_verdict, \
               status = EXCLUDED.status, \
               updated_at = now() \
             RETURNING {columns}",
            table = self.db.table(OBJECT_TABLE),
            columns = object_select_columns()
        ))
        .bind(new.id.clone())
        .bind(new.object_key.clone())
        .bind(new.size_bytes)
        .bind(new.sha256.clone())
        .bind(new.content_type.clone())
        .bind(new.scan_verdict.clone())
        .bind(crate::model::object_status::AVAILABLE.to_string());
        self.db.fetch_one(ctx, &query).await
    }

    /// 按 key 读对象元数据。
    pub async fn find_object(
        &self,
        ctx: &RequestContext,
        key: &str,
    ) -> DbResult<Option<ObjectRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} AND object_key = ?",
            table = self.db.table(OBJECT_TABLE),
            columns = object_select_columns()
        ))
        .bind(key.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 列出对象（按 key 升序）。
    pub async fn list_objects(&self, ctx: &RequestContext) -> DbResult<Vec<ObjectRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} ORDER BY object_key",
            table = self.db.table(OBJECT_TABLE),
            columns = object_select_columns()
        ));
        self.db.fetch_all(ctx, &query).await
    }

    /// 删对象元数据，返回影响行数。
    pub async fn delete_object(&self, ctx: &RequestContext, key: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND object_key = ?",
            table = self.db.table(OBJECT_TABLE)
        ))
        .bind(key.to_string());
        self.db.execute(ctx, &query).await
    }

    // -- 上传会话 -----------------------------------------------------------

    /// 建上传会话。
    pub async fn insert_upload(
        &self,
        ctx: &RequestContext,
        new: &NewUpload,
    ) -> DbResult<UploadRow> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
               (upload_id, tenant_id, object_key, total_parts, part_size, declared_checksum, \
                status, created_at, updated_at, expires_at) \
             VALUES (?::uuid, {{tenant}}, ?, ?, ?, ?, ?, now(), now(), ?) \
             RETURNING {columns}",
            table = self.db.table(UPLOAD_TABLE),
            columns = upload_select_columns()
        ))
        .bind(new.upload_id.clone())
        .bind(new.object_key.clone())
        .bind(new.total_parts)
        .bind(new.part_size)
        .bind(new.declared_checksum.clone())
        .bind(crate::model::upload_status::PENDING.to_string())
        .bind(new.expires_at);
        self.db.fetch_one(ctx, &query).await
    }

    /// 按 ID 读上传会话。
    pub async fn find_upload(
        &self,
        ctx: &RequestContext,
        upload_id: &str,
    ) -> DbResult<Option<UploadRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} \
             WHERE tenant_id = {{tenant}} AND upload_id = ?::uuid",
            table = self.db.table(UPLOAD_TABLE),
            columns = upload_select_columns()
        ))
        .bind(upload_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 改上传会话状态，返回影响行数。
    pub async fn update_upload_status(
        &self,
        ctx: &RequestContext,
        upload_id: &str,
        status: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND upload_id = ?::uuid",
            table = self.db.table(UPLOAD_TABLE)
        ))
        .bind(status.to_string())
        .bind(upload_id.to_string());
        self.db.execute(ctx, &query).await
    }

    // -- 任务 ---------------------------------------------------------------

    /// 建任务。
    pub async fn insert_job(&self, ctx: &RequestContext, new: &NewJob) -> DbResult<JobRow> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
               (id, tenant_id, kind, entity, status, total_rows, processed_rows, failed_rows, \
                object_key, created_at, updated_at) \
             VALUES (?::uuid, {{tenant}}, ?, ?, ?, 0, 0, 0, ?, now(), now()) \
             RETURNING {columns}",
            table = self.db.table(JOB_TABLE),
            columns = job_select_columns()
        ))
        .bind(new.id.clone())
        .bind(new.kind.clone())
        .bind(new.entity.clone())
        .bind(crate::model::job_status::PENDING.to_string())
        .bind(new.object_key.clone());
        self.db.fetch_one(ctx, &query).await
    }

    /// 按 ID 读任务。
    pub async fn find_job(&self, ctx: &RequestContext, job_id: &str) -> DbResult<Option<JobRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} AND id = ?::uuid",
            table = self.db.table(JOB_TABLE),
            columns = job_select_columns()
        ))
        .bind(job_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 列出任务（新的在前）。
    pub async fn list_jobs(&self, ctx: &RequestContext) -> DbResult<Vec<JobRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} \
             ORDER BY created_at DESC, id",
            table = self.db.table(JOB_TABLE),
            columns = job_select_columns()
        ));
        self.db.fetch_all(ctx, &query).await
    }

    /// 认领一批待处理任务（**`FOR UPDATE SKIP LOCKED`**，D324）。
    ///
    /// # ⚠️ `LIMIT` 为什么用 **CTE + `= ANY(...)`** 而不是 `IN (SELECT ... LIMIT)`（QA M3）
    ///
    /// 原写法是：
    ///
    /// ```sql
    /// UPDATE t SET ... WHERE id IN (
    ///   SELECT id FROM t WHERE ... ORDER BY created_at LIMIT ? FOR UPDATE SKIP LOCKED
    /// )
    /// ```
    ///
    /// 问题：`IN (子查询)` 在 PostgreSQL 里可能被实现成 **semi join**，
    /// 子查询会在**外层每一行**上重跑一次（计划里 `loops = N`）→
    /// `LIMIT` 只约束**每次**子查询，约束不了外层 UPDATE 的总行数。
    ///
    /// **实测**（QA + 我复测）：突发 500 条且统计信息陈旧时，
    /// `claim_batch(batch=10)` 认领了 **16 条**（超出 60%）；
    /// 小表 6 条时 `batch=2` 认领了 **3 条**。
    ///
    /// → 改成 **CTE 物化 + `= ANY(数组)`**：子查询在 CTE 里**只执行一次**，
    /// 认领出的 id 变成一个数组，外层 UPDATE 按数组精确匹配 —— 行数由 `LIMIT` 严格决定。
    pub async fn claim_jobs(&self, ctx: &RequestContext, batch: i64) -> DbResult<Vec<JobRow>> {
        let mut tx = self.db.begin(ctx).await?;
        let table = format!("{}.{}", tx.schema(), JOB_TABLE);
        // ⚠️ `{tenant}` **只能出现一次**（`TenantQuery::render` 强制恰好一个）。
        let query = TenantQuery::new(format!(
            "WITH picked AS ( \
               SELECT id FROM {table} \
               WHERE tenant_id = {{tenant}} AND status = ? \
               ORDER BY created_at \
               LIMIT ? \
               FOR UPDATE SKIP LOCKED \
             ) \
             UPDATE {table} SET status = ?, updated_at = now() \
             WHERE id = ANY(ARRAY(SELECT id FROM picked)) \
             RETURNING {columns}",
            table = table,
            columns = job_select_columns()
        ))
        .bind(crate::model::job_status::PENDING.to_string())
        .bind(batch)
        .bind(crate::model::job_status::RUNNING.to_string());
        let rows: Vec<JobRow> = tx.fetch_all(&query).await?;
        tx.commit().await?;
        Ok(rows)
    }

    /// **每批更新进度**（D325 / AC-S-180 的核心判据）。
    pub async fn update_job_progress(
        &self,
        ctx: &RequestContext,
        job_id: &str,
        processed_rows: i64,
        failed_rows: i64,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET processed_rows = ?, failed_rows = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?::uuid",
            table = self.db.table(JOB_TABLE)
        ))
        .bind(processed_rows)
        .bind(failed_rows)
        .bind(job_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 设置任务的总行数（导入解析完表头后能知道行数）。
    pub async fn set_job_total(
        &self,
        ctx: &RequestContext,
        job_id: &str,
        total_rows: i64,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET total_rows = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?::uuid",
            table = self.db.table(JOB_TABLE)
        ))
        .bind(total_rows)
        .bind(job_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 完成一个任务。
    pub async fn complete_job(
        &self,
        ctx: &RequestContext,
        job_id: &str,
        object_key: Option<&str>,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = ?, finished_at = now(), updated_at = now(), \
               object_key = COALESCE(?, object_key) \
             WHERE tenant_id = {{tenant}} AND id = ?::uuid",
            table = self.db.table(JOB_TABLE)
        ))
        .bind(crate::model::job_status::COMPLETED.to_string())
        .bind(object_key.map(str::to_string))
        .bind(job_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 把任务**退回 `pending`**（一批跑完但整体还没完 → 下一次 tick 接着跑）。
    ///
    /// ⚠️ 这是 AC-S-180「进度可查」的配套：任务在多次 tick 之间保持 `pending`
    /// 且 `processed_rows` 已更新 → 用户查得到真实进度。
    pub async fn requeue_job(&self, ctx: &RequestContext, job_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?::uuid",
            table = self.db.table(JOB_TABLE)
        ))
        .bind(crate::model::job_status::PENDING.to_string())
        .bind(job_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 让任务失败。
    pub async fn fail_job(&self, ctx: &RequestContext, job_id: &str, error: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = ?, error = ?, finished_at = now(), updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?::uuid",
            table = self.db.table(JOB_TABLE)
        ))
        .bind(crate::model::job_status::FAILED.to_string())
        .bind(error.to_string())
        .bind(job_id.to_string());
        self.db.execute(ctx, &query).await
    }

    // -- 错误回执 -----------------------------------------------------------

    /// 批量写错误回执（**一条 SQL 插多行**，避免 1 万行时 1 万次往返）。
    pub async fn insert_job_errors(
        &self,
        ctx: &RequestContext,
        job_id: &str,
        failures: &[RowFailure],
    ) -> DbResult<u64> {
        if failures.is_empty() {
            return Ok(0);
        }
        // ⚠️ 逐行插（不用 UNNEST 一次多行）：
        // `TenantQuery` 要求 `{tenant}` **恰好出现一次**，而多行插入的每个
        // value tuple 都要 tenant → 只能靠 `SELECT ... UNION ALL` 拼，
        // 那会随行数线性膨胀 SQL 文本（1 万行会撞 Postgres 的语句长度上限）。
        // 错误行的量级通常是**几十~几百**（非法行），逐行插的开销可接受；
        // 而「合法行」走的是批量插入（见 [`Self::insert_rows_batch`]）。
        let mut affected = 0u64;
        for failure in failures {
            let query = TenantQuery::new(format!(
                "INSERT INTO {table} (job_id, tenant_id, row_number, \"column\", reason) \
                 VALUES (?::uuid, {{tenant}}, ?, ?, ?) \
                 ON CONFLICT (job_id, row_number, \"column\") DO UPDATE SET reason = EXCLUDED.reason",
                table = self.db.table(JOB_ERROR_TABLE)
            ))
            .bind(job_id.to_string())
            .bind(failure.row_number)
            .bind(failure.column.clone())
            .bind(failure.reason.clone());
            affected += self.db.execute(ctx, &query).await?;
        }
        Ok(affected)
    }

    /// 列出某任务的错误回执（按行号升序）。
    pub async fn list_job_errors(
        &self,
        ctx: &RequestContext,
        job_id: &str,
    ) -> DbResult<Vec<JobErrorRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} \
             WHERE tenant_id = {{tenant}} AND job_id = ?::uuid \
             ORDER BY row_number, \"column\"",
            table = self.db.table(JOB_ERROR_TABLE),
            columns = "job_id::text AS job_id, tenant_id, row_number, \"column\", reason"
        ))
        .bind(job_id.to_string());
        self.db.fetch_all(ctx, &query).await
    }

    /// 数某任务的错误回执条数。
    pub async fn count_job_errors(&self, ctx: &RequestContext, job_id: &str) -> DbResult<i64> {
        let query = TenantQuery::new(format!(
            "SELECT count(*) AS n FROM {table} \
             WHERE tenant_id = {{tenant}} AND job_id = ?::uuid",
            table = self.db.table(JOB_ERROR_TABLE)
        ))
        .bind(job_id.to_string());
        #[derive(Debug)]
        struct CountRow {
            n: i64,
        }
        platform_db::tenant_row!(CountRow { n: "n" });
        let row: CountRow = self.db.fetch_one(ctx, &query).await?;
        Ok(row.n)
    }

    // -- 导入目标实体（D333）-------------------------------------------------

    /// 批量写入导入的**合法行**到目标实体表。
    ///
    /// # ⚠️ 为什么用单条语句插整批
    ///
    /// `TenantQuery` 要求 `{tenant}` **恰好出现一次** —— 那就不能写
    /// `VALUES (?, {tenant}, ...), (?, {tenant}, ...)`（每行一个 tenant）。
    /// 但逐行 `execute` 又会让 5 万行变成 5 万次往返（低配机上要跑几十分钟）。
    ///
    /// → 用**参数数组 + `UNNEST`**：一次往返插一整批，
    /// 且 `{tenant}` 只出现**一次**（在 `SELECT {tenant} AS tenant_id` 里）。
    ///
    /// # 幂等
    ///
    /// 断点续跑时会从头重算「哪些行已经处理过」（见 `worker::run_import` 的
    /// `already_done`），所以这里**不需要**去重约束；但为避免「部分成功 + 重试」
    /// 造成重复行，加了 `(tenant_id, job_id, email)` 的唯一约束（迁移里）。
    /// 这里用 `ON CONFLICT DO NOTHING` —— 重复导入同一份 CSV 不会翻倍。
    pub async fn insert_rows_batch(
        &self,
        ctx: &RequestContext,
        job_id: &str,
        rows: &[ProbeRow],
    ) -> DbResult<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        // ⚠️ 分块：一批 1000 行时数组元素也是 1000 个；
        // Postgres 的绑定参数上限是 65535 → 远在上限内，
        // 但为防止调用方传超大 `batch_size`，这里硬切 5000 行一块。
        const CHUNK: usize = 5_000;
        let mut affected = 0u64;
        for chunk in rows.chunks(CHUNK) {
            // ⚠️ `platform-db` 的 `BindValue` 只支持 `Vec<String>`（不支持 `Vec<i64>` /
            // `Vec<Option<String>>`）→ 一律按 `text[]` 传参，在 SQL 里 `::bigint[]` 转换。
            // 这不是绕路：`text[] → bigint[]` 的转换由 Postgres 做，非法值会在**写库时**报错，
            // 而非法值在 `validate_row` 阶段就已被挡掉（`amount` 必然是合法整数）。
            let ids: Vec<String> = chunk
                .iter()
                .map(|_| uuid::Uuid::new_v4().to_string())
                .collect();
            let emails: Vec<String> = chunk.iter().map(|r| r.email.clone()).collect();
            let amounts: Vec<String> = chunk.iter().map(|r| r.amount.to_string()).collect();
            // `note` 为 `None` 时传空串，SQL 里用 `NULLIF(..., '')` 还原成 NULL。
            let notes: Vec<String> = chunk
                .iter()
                .map(|r| r.note.clone().unwrap_or_default())
                .collect();

            let query = TenantQuery::new(format!(
                "INSERT INTO {table} (id, tenant_id, job_id, email, amount, note) \
                 SELECT u.id::uuid, {{tenant}}, ?::uuid, u.email, u.amount::bigint, \
                        NULLIF(u.note, '') \
                 FROM UNNEST(?::text[], ?::text[], ?::text[], ?::text[]) \
                      AS u(id, email, amount, note) \
                 ON CONFLICT (tenant_id, job_id, email) DO NOTHING",
                table = self.db.table(PROBE_ROW_TABLE)
            ))
            .bind(job_id.to_string())
            .bind(ids)
            .bind(emails)
            .bind(amounts)
            .bind(notes);
            affected += self.db.execute(ctx, &query).await?;
        }
        Ok(affected)
    }

    /// 数目标实体表里某任务写入了多少行。
    ///
    /// ⚠️ 这是 AC-S-180 步骤 5 / AC-S-181 步骤 4 的**唯一可信数据源**：
    /// `processed_rows` 只证明「数了行」，本方法才证明「入了库」。
    pub async fn count_probe_rows(&self, ctx: &RequestContext, job_id: &str) -> DbResult<i64> {
        let query = TenantQuery::new(format!(
            "SELECT count(*) AS n FROM {table} \
             WHERE tenant_id = {{tenant}} AND job_id = ?::uuid",
            table = self.db.table(PROBE_ROW_TABLE)
        ))
        .bind(job_id.to_string());
        #[derive(Debug)]
        struct CountRow {
            n: i64,
        }
        platform_db::tenant_row!(CountRow { n: "n" });
        let row: CountRow = self.db.fetch_one(ctx, &query).await?;
        Ok(row.n)
    }

    /// 清理**终态**的过期任务及其错误明细（TD-083）。
    ///
    /// # ⚠️ 为什么需要它
    ///
    /// `platform_storage_job` 与 `platform_storage_job_error` 都是「每任务/每失败行一行」，
    /// 而**没有任何删除路径** —— 长期运行的实例会无限增长。
    /// 同项目的 `platform_rate_limit` 有 `xtask observability prune-rate-limit`，
    /// 本表此前没有对应命令。
    ///
    /// # 顺序：先删错误，再删任务
    ///
    /// `platform_storage_job_error.job_id` 引用任务（**逻辑引用，无外键**），
    /// 所以必须先删明细再删主表 —— 反过来会留下孤儿错误行。
    ///
    /// # 只删**终态**，且只删**足够旧**的
    ///
    /// - `completed` / `failed` 是终态，删了不影响正在跑的导入导出；
    /// - `pending` / `running` **绝不删**。
    ///
    /// # 返回
    ///
    /// `(删除的任务数, 删除的错误明细数)`。
    pub async fn prune_terminal_jobs(
        &self,
        ctx: &RequestContext,
        older_than_days: i64,
    ) -> DbResult<(u64, u64)> {
        // ⚠️ 至少保留 1 天（同 notify 的纪律：排障最需要看最近那批）。
        let keep_days = if older_than_days <= 0 { 1 } else { older_than_days };

        // ① 先删错误明细：只删「属于终态且足够旧的任务」的那些行。
        //
        // ⚠️ `{tenant}` **只能出现一次**（`TenantQuery::render` 强制恰好一个）。
        // 外层 DELETE 不再写租户条件 —— 它的 `job_id IN (子查询)` 已被子查询限定在
        // 本租户内（子查询的租户条件 + RLS 双重保证），与 `claim_batch` 同款处理。
        let errors_query = TenantQuery::new(format!(
            "DELETE FROM {errors} \
              WHERE job_id IN ( \
                SELECT id FROM {jobs} \
                 WHERE tenant_id = {{tenant}} \
                   AND status IN (?, ?) \
                   AND updated_at < now() - make_interval(days => ?::int) \
              )",
            errors = self.db.table(JOB_ERROR_TABLE),
            jobs = self.db.table(JOB_TABLE)
        ))
        .bind(crate::model::job_status::COMPLETED.to_string())
        .bind(crate::model::job_status::FAILED.to_string())
        .bind(keep_days);
        let errors_deleted = self.db.execute(ctx, &errors_query).await?;

        // ② 再删任务本体。
        let jobs_query = TenantQuery::new(format!(
            "DELETE FROM {jobs} \
              WHERE tenant_id = {{tenant}} \
                AND status IN (?, ?) \
                AND updated_at < now() - make_interval(days => ?::int)",
            jobs = self.db.table(JOB_TABLE)
        ))
        .bind(crate::model::job_status::COMPLETED.to_string())
        .bind(crate::model::job_status::FAILED.to_string())
        .bind(keep_days);
        let jobs_deleted = self.db.execute(ctx, &jobs_query).await?;

        Ok((jobs_deleted, errors_deleted))
    }

    /// 读目标实体表里某任务写入的行（导出用）。
    pub async fn list_probe_rows(
        &self,
        ctx: &RequestContext,
        job_id: Option<&str>,
    ) -> DbResult<Vec<ProbeRow>> {
        let query = match job_id {
            Some(id) => TenantQuery::new(format!(
                "SELECT email, amount, note FROM {table} \
                 WHERE tenant_id = {{tenant}} AND job_id = ?::uuid ORDER BY email, amount",
                table = self.db.table(PROBE_ROW_TABLE)
            ))
            .bind(id.to_string()),
            None => TenantQuery::new(format!(
                "SELECT email, amount, note FROM {table} \
                 WHERE tenant_id = {{tenant}} ORDER BY email, amount",
                table = self.db.table(PROBE_ROW_TABLE)
            )),
        };
        #[derive(Debug)]
        struct Row {
            email: String,
            amount: i64,
            note: Option<String>,
        }
        platform_db::tenant_row!(Row {
            email: "email",
            amount: "amount",
            note: "note",
        });
        let rows: Vec<Row> = self.db.fetch_all(ctx, &query).await?;
        Ok(rows
            .into_iter()
            .map(|r| ProbeRow {
                email: r.email,
                amount: r.amount,
                note: r.note,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{JOB_COLUMNS, JOB_ERROR_COLUMNS, OBJECT_COLUMNS, UPLOAD_COLUMNS};

    /// 四张表的列清单与 `model` 的常量**逐字一致**（防漂移）。
    #[test]
    fn the_column_lists_match_the_model_constants() {
        let parse = |s: &str| -> Vec<String> {
            // 处理 `id::text AS id` 这种：取 `AS` 后或者裸列名。
            s.split(',')
                .map(|part| {
                    let trimmed = part.trim();
                    if let Some((_, alias)) = trimmed.split_once(" AS ") {
                        alias.trim().to_string()
                    } else {
                        trimmed.trim_matches('"').to_string()
                    }
                })
                .collect()
        };
        assert_eq!(parse(object_columns()), OBJECT_COLUMNS.to_vec(), "对象表");
        assert_eq!(parse(upload_columns()), UPLOAD_COLUMNS.to_vec(), "上传表");
        assert_eq!(parse(job_columns()), JOB_COLUMNS.to_vec(), "任务表");
        assert_eq!(
            parse(job_error_columns()),
            JOB_ERROR_COLUMNS.to_vec(),
            "错误回执表"
        );
    }

    /// 查询用的列清单与库层清单**数量一致**（只是 `id` 多了 `::text`）。
    #[test]
    fn the_select_lists_have_the_same_number_of_columns() {
        assert_eq!(
            object_select_columns().split(',').count(),
            object_columns().split(',').count()
        );
        assert_eq!(
            upload_select_columns().split(',').count(),
            upload_columns().split(',').count()
        );
        assert_eq!(
            job_select_columns().split(',').count(),
            job_columns().split(',').count()
        );
    }

    /// ⚠️ **`column` 在错误回执的列清单里必须带双引号**（SQL 保留字）。
    #[test]
    fn the_column_keyword_is_quoted() {
        assert!(
            job_error_columns().contains("\"column\""),
            "`column` 是 SQL 保留字，必须写成 \"column\"：{}",
            job_error_columns()
        );
        assert!(
            !job_error_columns().contains(", column,")
                && !job_error_columns().ends_with(", column"),
            "不得出现未加引号的 column：{}",
            job_error_columns()
        );
    }

    /// 对象表列数 10。
    #[test]
    fn the_object_table_has_ten_columns() {
        assert_eq!(object_columns().split(',').count(), 10);
    }

    /// 上传表列数 10。
    #[test]
    fn the_upload_table_has_ten_columns() {
        assert_eq!(upload_columns().split(',').count(), 10);
    }

    /// 任务表列数 13。
    #[test]
    fn the_job_table_has_thirteen_columns() {
        assert_eq!(job_columns().split(',').count(), 13);
    }

    /// 错误回执表列数 5。
    #[test]
    fn the_job_error_table_has_five_columns() {
        assert_eq!(job_error_columns().split(',').count(), 5);
    }
}
