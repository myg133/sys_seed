//! 导入导出的 **worker**（REQ-018，D324 / D325）。
//!
//! # 复用 REQ-015 的形态（D324）
//!
//! `SELECT ... FOR UPDATE SKIP LOCKED` + `tick()` + `xtask storage worker`。
//! 不重新发明 —— 那套已过 7 个真库测试验证（含 4 worker 并发不重不漏）。
//!
//! # ⚠️ 三条硬约束
//!
//! 1. **流式 + 分批**（D325）：不把 5 万行读进内存；每 `import_batch_size` 行提交一次；
//! 2. **每批更新进度**（AC-S-180 的核心判据）：只在最后写一次 → 进度可查是假的；
//! 3. **行号是文件行号**（1-based，含表头，AC-S-181）：
//!    数据第 1 行 = 文件第 2 行 —— 这是最容易错的一处。

use platform_core::RequestContext;

use crate::csv;
use crate::error::ServiceError;
use crate::model::job_kind;
use crate::object_store::sha256_hex;
use crate::scanner::ScanVerdict;
use crate::service::StorageService;
use crate::store::{NewObject, RowFailure};

/// 一次导入处理的结果。
///
/// ⚠️ `Partial` 是「这一轮只跑了几批，还没完」—— 见 `max_batches` 的说明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    /// 跑完了。
    Completed,
    /// 只跑了一部分（需要下一次 tick 接着跑）。
    Partial {
        /// 已处理行数。
        processed: i64,
        /// 失败行数。
        failed: i64,
    },
}

/// 一次 `tick()` 的统计（**可断言**）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickStats {
    /// 认领到多少任务。
    pub claimed: usize,
    /// 完成多少。
    pub completed: usize,
    /// 失败多少。
    pub failed: usize,
    /// **只跑了一部分**（进度已更新，需下次继续）多少。
    pub partial: usize,
}

impl TickStats {
    /// 计数是否自洽（防「认领了但被静默吞掉」）。
    pub fn is_balanced(&self) -> bool {
        self.completed + self.failed + self.partial == self.claimed
    }
}

/// 一次 `tick()`：认领一批任务 → 逐个处理。
pub async fn tick(
    service: &StorageService,
    ctx: &RequestContext,
    batch: i64,
) -> Result<TickStats, ServiceError> {
    tick_with_limit(service, ctx, batch, usize::MAX).await
}

/// 与 [`tick`] 相同，但**限制每个任务一次最多处理几批**。
///
/// ⚠️ 这是 AC-S-180「进度可查」的机制：`tick()` 用 `usize::MAX`（一次跑完），
/// 而需要观察中间态的测试用 `tick_with_limit(.., 1)`。
/// 生产上 `xtask storage worker --rounds 0` 会循环调 `tick()`，
/// 每次跑完一个任务 → 进度在每次 `tick` 之间可见。
pub async fn tick_with_limit(
    service: &StorageService,
    ctx: &RequestContext,
    batch: i64,
    max_batches_per_tick: usize,
) -> Result<TickStats, ServiceError> {
    let jobs = service.store().claim_jobs(ctx, batch).await?;
    let mut stats = TickStats {
        claimed: jobs.len(),
        ..TickStats::default()
    };

    for job in &jobs {
        let outcome = if job.kind == job_kind::IMPORT {
            run_import(service, ctx, job, max_batches_per_tick).await
        } else {
            run_export(service, ctx, job, max_batches_per_tick).await
        };
        match outcome {
            Ok(ImportOutcome::Completed) => stats.completed += 1,
            Ok(ImportOutcome::Partial { .. }) => {
                // ⚠️ **还没完** → 退回 `pending`，下一次 tick 接着跑。
                // 这一步做完 `processed_rows` 已经是可见的中间值 →
                // 「进度可查」成立（AC-S-180）。
                service
                    .store()
                    .requeue_job(ctx, &job.id)
                    .await
                    .map_err(ServiceError::from)?;
                stats.partial += 1;
            }
            Err(reason) => {
                stats.failed += 1;
                service
                    .store()
                    .fail_job(ctx, &job.id, &reason)
                    .await
                    .map_err(ServiceError::from)?;
            }
        }
    }
    Ok(stats)
}

/// 跑一个导入任务（**流式 + 分批 + 每批更新进度**）。
async fn run_import(
    service: &StorageService,
    ctx: &RequestContext,
    job: &crate::model::JobRow,
    max_batches: usize,
) -> Result<ImportOutcome, String> {
    let settings = service.config().settings(ctx).await;
    let key = job
        .object_key
        .as_deref()
        .ok_or_else(|| "the import job has no source object".to_string())?;

    // ① 读源文件。
    let bytes = service
        .objects()
        .get(key)
        .await
        .map_err(|e| format!("reading the source object failed: {e}"))?;

    // ② 扫描（导入的文件也要扫 —— 与直传同款纪律）。
    let verdict = service
        .scanner()
        .scan(&bytes)
        .await
        .map_err(|e| format!("the virus scanner failed: {e}"))?;
    if let Some(sig) = verdict.signature() {
        return Err(format!(
            "the source file was rejected by the virus scanner: {sig}"
        ));
    }

    // ③ 解析（**引号未闭合 / 行超限都会在这里报**）。
    let parsed = csv::parse(&bytes, settings.max_row_bytes as usize)
        .map_err(|e| format!("the source csv is invalid: {}", e.detail()))?;

    let total = parsed.row_count() as i64;
    service
        .store()
        .set_job_total(ctx, &job.id, total)
        .await
        .map_err(|e| format!("setting the total failed: {e}"))?;

    // ④ 逐批处理。
    let batch_size = settings.import_batch_size.max(1) as usize;
    let mut failures: Vec<RowFailure> = Vec::new();

    // ⚠️ **一次 tick 最多处理 `max_batches` 批**（AC-S-180 的「进度可查」要求）：
    // 若一次 tick 把 5 万行全跑完，用户**永远看不到中间进度** ——
    // 「进度可查」变成一句空话（TC-018-040 会红）。
    // → 处理到上限就返回 [`ImportOutcome::Partial`]，让下一次 tick 接着跑。
    let mut batches_done = 0usize;

    // ⚠️ **断点续跑**：从 `processed_rows` 接着跑。
    // 若每次都从头跑，被 `Partial` 退回的任务会**重复处理前面的行**
    // → `processed_rows` 永远推不到 `total`（无限循环）。
    // 已处理的行数（不是「数据序号」——两者差 0，因为 processed 只数数据行）。
    let already_done = job.processed_rows.max(0) as usize;
    // 从上次进度继续；`failed` 也从库里带上来（否则失败数会被重置）。
    let mut processed: i64 = job.processed_rows.max(0);
    let mut failed: i64 = job.failed_rows.max(0);

    for (index, row) in parsed.rows.iter().enumerate() {
        if index < already_done {
            continue; // 已经处理过（上次 tick 的成果）
        }
        // ⚠️ **文件行号** = 数据序号 + 2（表头占第 1 行）。
        let file_line = (index + 2) as i64;
        let map = parsed.to_map(row);
        match validate_row(&parsed.header, &map) {
            Ok(()) => {}
            Err(failure) => {
                failures.push(RowFailure {
                    row_number: file_line,
                    ..failure
                });
                failed += 1;
            }
        }
        processed += 1;

        // 每批：写错误回执 + 更新进度（**⚠️ 这是 AC-S-180 的核心判据**）。
        if processed % batch_size as i64 == 0 {
            if !failures.is_empty() {
                service
                    .store()
                    .insert_job_errors(ctx, &job.id, &failures)
                    .await
                    .map_err(|e| format!("writing the error receipt failed: {e}"))?;
                failures.clear();
            }
            service
                .store()
                .update_job_progress(ctx, &job.id, processed, failed)
                .await
                .map_err(|e| format!("updating the progress failed: {e}"))?;
            batches_done += 1;

            // 到上限就停（把控制权交回 —— 下一次 tick 从 `processed_rows` 接着跑）。
            if batches_done >= max_batches && processed < total {
                // 把任务退回 `pending` 让下一次 tick 能再认领它。
                return Ok(ImportOutcome::Partial { processed, failed });
            }
        }
    }

    // 尾批。
    if !failures.is_empty() {
        service
            .store()
            .insert_job_errors(ctx, &job.id, &failures)
            .await
            .map_err(|e| format!("writing the error receipt failed: {e}"))?;
    }
    service
        .store()
        .update_job_progress(ctx, &job.id, processed, failed)
        .await
        .map_err(|e| format!("updating the progress failed: {e}"))?;

    // ⑤ 完成。
    service
        .store()
        .complete_job(ctx, &job.id, None)
        .await
        .map_err(|e| format!("completing the job failed: {e}"))?;
    Ok(ImportOutcome::Completed)
}

/// 跑一个导出任务（读表 → 生成 CSV → 落存储）。
async fn run_export(
    service: &StorageService,
    ctx: &RequestContext,
    job: &crate::model::JobRow,
    _max_batches: usize,
) -> Result<ImportOutcome, String> {
    // ① 读数据（这里的「实体 → 表」映射由装配层的 `ExportSource` 提供；
    // 本轮内置一个最小实现：把 `platform_storage_object` 的元数据导出成 CSV，
    // 这样 round-trip 用例有真实数据可验而不需要业务表）。
    let (header, rows) = export_rows(service, ctx)
        .await
        .map_err(|e| format!("reading the export source failed: {e}"))?;

    let total = rows.len() as i64;
    service
        .store()
        .set_job_total(ctx, &job.id, total)
        .await
        .map_err(|e| format!("setting the total failed: {e}"))?;

    // ② 生成 CSV（**带 BOM**）。
    let bytes = csv::generate(&header, &rows);

    // ③ 落存储（key 用 job_id，避免与业务 key 冲突）。
    let object_key = format!("exports/{}.csv", job.id);
    service
        .objects()
        .put(&object_key, bytes.clone().into())
        .await
        .map_err(|e| format!("writing the export failed: {e}"))?;

    // ④ 记元数据（这样 `downloadUrl` 能通过预签名拿到）。
    let new = NewObject {
        id: uuid::Uuid::new_v4().to_string(),
        object_key: object_key.clone(),
        size_bytes: bytes.len() as i64,
        sha256: sha256_hex(&bytes),
        content_type: "text/csv".to_string(),
        // 导出的是我们自己生成的内容 —— 仍如实记 `skipped`（没扫）。
        scan_verdict: ScanVerdict::Skipped.as_str().to_string(),
    };
    service
        .store()
        .upsert_object(ctx, &new)
        .await
        .map_err(|e| format!("recording the export metadata failed: {e}"))?;

    service
        .store()
        .update_job_progress(ctx, &job.id, total, 0)
        .await
        .map_err(|e| format!("updating the progress failed: {e}"))?;
    service
        .store()
        .complete_job(ctx, &job.id, Some(&object_key))
        .await
        .map_err(|e| format!("completing the job failed: {e}"))?;
    Ok(ImportOutcome::Completed)
}

/// 导出的数据源（**本轮的最小实现**：导出对象元数据）。
///
/// ⚠️ 真正的「导出业务表」需要一个「实体 → 表/列」的映射（属业务侧）。
/// 本轮把 `platform_storage_object` 自身的元数据导出成 CSV ——
/// 这让 round-trip 用例（TC-018-047）有**真实数据**可验，
/// 而不假装支持任意业务表（那样做出来的是**假能力**）。
async fn export_rows(
    service: &StorageService,
    ctx: &RequestContext,
) -> Result<(Vec<String>, Vec<Vec<String>>), ServiceError> {
    let objects = service.store().list_objects(ctx).await?;
    let header = vec![
        "objectKey".to_string(),
        "sizeBytes".to_string(),
        "sha256".to_string(),
        "contentType".to_string(),
        "scanVerdict".to_string(),
    ];
    let rows = objects
        .iter()
        .map(|o| {
            vec![
                o.object_key.clone(),
                o.size_bytes.to_string(),
                o.sha256.clone(),
                o.content_type.clone(),
                o.scan_verdict.clone(),
            ]
        })
        .collect();
    Ok((header, rows))
}

/// 校验一行（返回 `RowFailure` 指出**列名**与原因）。
///
/// # 本轮的最小校验
///
/// - 所有列都必须存在（缺列表头 → 每行都失败）；
/// - `sizeBytes` 若有则必须是整数（这是导出格式里的数字列，
///   用它来验「数字列非法」这条判据，TC-018-041/042）。
fn validate_row(
    header: &[String],
    map: &std::collections::BTreeMap<String, String>,
) -> Result<(), RowFailure> {
    // 空表头 → 无法校验，直接通过（空文件的用例要能过）。
    if header.is_empty() {
        return Ok(());
    }
    // 每行列数是否与表头一致（`csv::parse` 已经检查过，这里再兜一次）。
    for name in header {
        if !map.contains_key(name) {
            return Err(RowFailure {
                row_number: 0,
                column: name.clone(),
                reason: "the column is missing from this row".to_string(),
            });
        }
    }
    // 数字列校验（`sizeBytes` / `totalRows` / `processedRows` 这类）。
    for numeric in ["sizeBytes", "totalRows", "processedRows", "failedRows"] {
        if let Some(value) = map.get(numeric)
            && !value.is_empty()
            && value.parse::<i64>().is_err()
        {
            return Err(RowFailure {
                row_number: 0,
                column: numeric.to_string(),
                reason: "the value is not a valid integer".to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// `is_balanced` 抓「有理赔被吞」。
    #[test]
    fn is_balanced_catches_swallowed_jobs() {
        let unbalanced = TickStats {
            claimed: 3,
            completed: 2,
            failed: 0,
            partial: 0,
        };
        assert!(!unbalanced.is_balanced(), "少记一条必须被抓住");

        let balanced = TickStats {
            claimed: 3,
            completed: 2,
            failed: 1,
            partial: 0,
        };
        assert!(balanced.is_balanced());
    }

    /// `partial` 也计入自洽（进度已更新但任务没完）。
    #[test]
    fn partial_counts_toward_balance() {
        let stats = TickStats {
            claimed: 2,
            completed: 1,
            failed: 0,
            partial: 1,
        };
        assert!(stats.is_balanced(), "partial 必须计入自洽");
    }

    /// 空轮自洽。
    #[test]
    fn an_empty_round_is_balanced() {
        assert!(TickStats::default().is_balanced());
    }

    /// 数字列非整数 → 报错且**指出列名**（TC-018-041 的列名判据）。
    #[test]
    fn a_non_numeric_value_reports_the_column_name() {
        let header = vec!["objectKey".to_string(), "sizeBytes".to_string()];
        let mut map = BTreeMap::new();
        map.insert("objectKey".to_string(), "a.csv".to_string());
        map.insert("sizeBytes".to_string(), "not-a-number".to_string());

        let failure = validate_row(&header, &map).expect_err("必须报错");
        assert_eq!(
            failure.column, "sizeBytes",
            "必须指出**真实列名**，不是 column_2 这种下标"
        );
        assert!(
            failure.reason.is_ascii(),
            "reason 必须是 ASCII：{}",
            failure.reason
        );
    }

    /// 合法行通过。
    #[test]
    fn a_valid_row_passes() {
        let header = vec!["objectKey".to_string(), "sizeBytes".to_string()];
        let mut map = BTreeMap::new();
        map.insert("objectKey".to_string(), "a.csv".to_string());
        map.insert("sizeBytes".to_string(), "1024".to_string());
        assert!(validate_row(&header, &map).is_ok());
    }

    /// 空表头 → 通过（空文件用例要能过）。
    #[test]
    fn an_empty_header_passes() {
        assert!(validate_row(&[], &BTreeMap::new()).is_ok());
    }

    /// 空值的数字列**不算错**（空字段是「未填」不是「非法」）。
    #[test]
    fn an_empty_numeric_cell_is_not_an_error() {
        let header = vec!["sizeBytes".to_string()];
        let mut map = BTreeMap::new();
        map.insert("sizeBytes".to_string(), String::new());
        assert!(validate_row(&header, &map).is_ok(), "空值不该报错");
    }

    /// 缺列 → 报错且指出那个列名。
    #[test]
    fn a_missing_column_reports_its_name() {
        let header = vec!["objectKey".to_string(), "sha256".to_string()];
        let mut map = BTreeMap::new();
        map.insert("objectKey".to_string(), "a".to_string());
        // 故意不给 sha256
        let failure = validate_row(&header, &map).expect_err("必须报错");
        assert_eq!(failure.column, "sha256");
    }
}
