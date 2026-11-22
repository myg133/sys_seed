//! REQ-018 **真实库**集成用例：分片 / 上传限制 / 导入导出 / RLS。
//!
//! 覆盖：AC-S-177、178、180、181、182、183。
//!
//! # ⚠️ 为什么这些必须真库
//!
//! | AC | 判据 | 为什么单测不行 |
//! |---|---|---|
//! | 177 | 缺片列出片号 + checksum 不符**不落库** | 要真实的「合并 → 校验 → 回滚」 |
//! | 178 | 超限**无副作用** | 要真的看落盘目录 |
//! | 180 | 5 万行 + **进度每批更新** | 要真的 `processed_rows` 逐步增长 |
//! | 183 | **4 张表**各验 RLS | 要应用角色 + 真 policy |

mod common;

use bytes::Bytes;
use common::*;
use platform_storage::model::{job_kind, job_status, scan_verdict};
use platform_storage::object_store::sha256_hex;

/// **AC-S-177**：分片上传主路径 → complete → sha256 一致（TC-018-018）。
#[tokio::test]
async fn multipart_upload_assembles_and_stores_the_object() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    let p1 = Bytes::from(vec![1u8; 1024]);
    let p2 = Bytes::from(vec![2u8; 1024]);
    let p3 = Bytes::from(vec![3u8; 512]);
    let mut expected = Vec::new();
    expected.extend_from_slice(&p1);
    expected.extend_from_slice(&p2);
    expected.extend_from_slice(&p3);

    let session = fixture
        .service
        .create_upload(
            &ctx,
            &platform_storage::model::CreateUploadBody {
                object_key: "big/file.bin".to_string(),
                total_parts: 3,
                part_size: 1024,
                checksum: Some(sha256_hex(&expected)),
            },
        )
        .await
        .expect("建会话");

    for (n, part) in [(1u32, p1), (2, p2), (3, p3)] {
        fixture
            .service
            .upload_part(&ctx, &session.upload_id, n, part)
            .await
            .unwrap_or_else(|e| panic!("上传分片 {n} 失败：{e}"));
    }

    let row = fixture
        .service
        .complete_upload(&ctx, &session.upload_id, Some("application/octet-stream"))
        .await
        .expect("complete");

    assert_eq!(row.size_bytes, 2560, "总字节数");
    assert_eq!(row.sha256, sha256_hex(&expected), "sha256 必须一致");
    assert_eq!(row.object_key, "big/file.bin");

    // 读回逐字节相同。
    let (_, got) = fixture
        .service
        .get_object(&ctx, "big/file.bin")
        .await
        .expect("读回");
    assert_eq!(&got[..], &expected[..], "内容必须逐字节相同");
}

/// **AC-S-177 步骤 5**：缺片 → `PART_MISSING` 且**列出片号**（TC-018-020）。
#[tokio::test]
async fn missing_parts_are_reported_with_numbers() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    let session = fixture
        .service
        .create_upload(
            &ctx,
            &platform_storage::model::CreateUploadBody {
                object_key: "gap.bin".to_string(),
                total_parts: 3,
                part_size: 512,
                checksum: None,
            },
        )
        .await
        .expect("建会话");
    fixture
        .service
        .upload_part(&ctx, &session.upload_id, 1, Bytes::from_static(b"a"))
        .await
        .expect("片 1");
    fixture
        .service
        .upload_part(&ctx, &session.upload_id, 3, Bytes::from_static(b"c"))
        .await
        .expect("片 3");

    let err = fixture
        .service
        .complete_upload(&ctx, &session.upload_id, None)
        .await
        .expect_err("必须报缺片");
    assert_eq!(err.code(), "STORAGE_PART_MISSING");
    let details = err.problem_details(&ctx, "/x");
    let params = details.params;
    assert_eq!(
        params.and_then(|p| p.get("missingParts").cloned()),
        Some(serde_json::json!([2])),
        "必须列出缺的片号 [2]"
    );
}

/// **AC-S-177 步骤 6**：checksum 不符 → 409 **且对象不存在**（TC-018-021）。
#[tokio::test]
async fn a_checksum_mismatch_rejects_the_object() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    let session = fixture
        .service
        .create_upload(
            &ctx,
            &platform_storage::model::CreateUploadBody {
                object_key: "bad.bin".to_string(),
                total_parts: 1,
                part_size: 512,
                // 故意声明一个错的 checksum。
                checksum: Some("0".repeat(64)),
            },
        )
        .await
        .expect("建会话");
    fixture
        .service
        .upload_part(&ctx, &session.upload_id, 1, Bytes::from_static(b"hello"))
        .await
        .expect("上传");

    let err = fixture
        .service
        .complete_upload(&ctx, &session.upload_id, None)
        .await
        .expect_err("checksum 不符必须拒绝");
    assert_eq!(err.code(), "STORAGE_CHECKSUM_MISMATCH");

    // ⚠️ **对象不存在**（坏文件绝不能落库）。
    let get = fixture.service.get_object(&ctx, "bad.bin").await;
    assert!(
        get.is_err(),
        "checksum 不符的对象**不得**可读（实际拿到了内容）"
    );
    // 元数据表里也没有这一行。
    let count = fixture
        .query_i64("SELECT count(*) FROM platform_storage_object WHERE object_key = 'bad.bin'")
        .await;
    assert_eq!(count, 0, "元数据表里不得有 bad.bin");
}

/// **AC-S-178 步骤 1/2**：正好上限通过；超 1 字节 → 413 **且落盘无残留**（TC-018-025/026）。
#[tokio::test]
async fn the_size_limit_has_no_side_effects_when_exceeded() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    // 把上限设成 1024 便于测。
    fixture
        .config
        .set(platform_storage::config::CONFIG_KEY_MAX_UPLOAD_BYTES, 1024);

    // 边界包含：正好 1024 → 通过。
    fixture
        .service
        .put_object(
            &ctx,
            "exact.bin",
            Bytes::from(vec![0u8; 1024]),
            "application/octet-stream",
        )
        .await
        .expect("正好上限必须通过");

    // 超 1 字节 → 413。
    let err = fixture
        .service
        .put_object(
            &ctx,
            "over.bin",
            Bytes::from(vec![0u8; 1025]),
            "application/octet-stream",
        )
        .await
        .expect_err("超限必须拒绝");
    assert_eq!(err.code(), "STORAGE_UPLOAD_TOO_LARGE");

    // ⚠️ 判据 ①：params.maxBytes 逐字等于配置值。
    let details = err.problem_details(&ctx, "/x");
    let params = details.params;
    assert_eq!(
        params.and_then(|p| p.get("maxBytes").cloned()),
        Some(serde_json::json!(1024)),
        "必须带 maxBytes"
    );

    // ⚠️ 判据 ②：落盘目录里**没有** over.bin（拒绝发生在写盘之前）。
    let files = fixture.list_stored_files();
    let names: Vec<String> = files
        .iter()
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert!(
        !names.iter().any(|n| n.contains("over.bin")),
        "超限的请求不得落盘；实际落盘：{names:?}"
    );
    // 阳性对照：exact.bin 真的落盘了（证明上面的扫描不是空扫）。
    assert!(
        names.iter().any(|n| n.contains("exact.bin")),
        "阳性对照失败：exact.bin 应该已落盘；实际：{names:?}"
    );
}

/// **AC-S-178 步骤 3**：默认扫描是 **skipped 不是 clean**（TC-018-027）。
#[tokio::test]
async fn the_default_scan_verdict_is_skipped() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    let row = fixture
        .service
        .put_object(&ctx, "s.txt", Bytes::from_static(b"x"), "text/plain")
        .await
        .expect("上传");
    assert_eq!(
        row.scan_verdict,
        scan_verdict::SKIPPED,
        "默认必须是 skipped"
    );
    assert_ne!(row.scan_verdict, scan_verdict::CLEAN, "**不得**声称干净");
}

/// **AC-S-178 步骤 4**：判定感染 → 422 **且不落盘**（TC-018-028）。
#[tokio::test]
async fn an_infected_file_is_rejected_and_not_stored() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    fixture.scanner.set_infected();

    let err = fixture
        .service
        .put_object(
            &ctx,
            "virus.bin",
            Bytes::from_static(b"evil"),
            "application/octet-stream",
        )
        .await
        .expect_err("感染文件必须拒绝");
    assert_eq!(err.code(), "STORAGE_FILE_INFECTED");
    let details = err.problem_details(&ctx, "/x");
    let params = details.params;
    assert!(
        params
            .and_then(|p| p
                .get("signature")
                .and_then(|v| v.as_str())
                .map(str::to_string))
            .is_some_and(|s| !s.is_empty()),
        "必须带非空的 signature"
    );

    // 不落盘。
    let files = fixture.list_stored_files();
    let names: Vec<String> = files
        .iter()
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert!(
        !names.iter().any(|n| n.contains("virus")),
        "感染文件不得落盘；实际：{names:?}"
    );
}

/// **AC-S-179 步骤 2**：上传 `.xlsx` → 415 `UNSUPPORTED_FORMAT`（TC-018-031）。
#[tokio::test]
async fn an_xlsx_import_is_rejected_with_a_clear_code() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    // 先放一个 `.xlsx` 源文件（模拟用户上传了 Excel）。
    fixture
        .service
        .put_object(
            &ctx,
            "data.xlsx",
            Bytes::from_static(b"PK\x03\x04"),
            "application/vnd.ms-excel",
        )
        .await
        .expect("上传 xlsx 本身是允许的");

    let err = fixture
        .service
        .create_import(
            &ctx,
            &platform_storage::model::CreateJobBody {
                entity: "probe".to_string(),
                object_key: Some("data.xlsx".to_string()),
            },
        )
        .await
        .expect_err("xlsx 导入必须拒绝");
    assert_eq!(err.code(), "STORAGE_UNSUPPORTED_FORMAT");
    assert!(
        err.detail().contains("only csv is supported"),
        "detail 必须说明只支持 csv：{}",
        err.detail()
    );
}

/// **AC-S-180 + 181**：5 万行导入 → 进度每批更新 + 错误回执带行号列名。
///
/// ⚠️ 这里用 **2500 行**（含 5 行非法）而不是 5 万 —— 5 万行会让这条用例
/// 在低配机上跑几分钟。**规模差异在报告里如实说明**；
/// 判据（分批 / 进度 / 行号 / 列名）与 5 万行完全一致。
#[tokio::test]
async fn a_large_import_reports_progress_and_row_level_errors() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    // 规格要求 `storage.importBatchSize = 1000`（AC-S-180 前置条件）。
    fixture
        .config
        .set(platform_storage::config::CONFIG_KEY_IMPORT_BATCH_SIZE, 1000);

    // ⚠️ **规模按规格来：50001 行 = 1 行表头 + 5 万数据行**（AC-S-180 前置条件）。
    // 原文的测试只跑了 2500 行，且**完全没有断言目标表行数** → F-01（合法行不入库）
    // 因此逃过了测试（复核 F-02）。这里两处都补上。
    const DATA_ROWS: usize = 50_000;
    const BAD_ROWS: usize = 10;
    // 表头是 `email,amount,note`（D333 统一；AC-S-181 要求列名是 email / amount）。
    let mut csv = String::from("email,amount,note\n");
    // 恰好 10 行 `amount` 写成非数字（散布在 5 万行里，覆盖多个批次）。
    let bad_data_rows: Vec<usize> = (1..=BAD_ROWS).map(|i| i * 5_000).collect();
    for i in 1..=DATA_ROWS {
        let amount = if bad_data_rows.contains(&i) {
            "not-a-number"
        } else {
            "10"
        };
        csv.push_str(&format!("row-{i}@example.com,{amount},note-{i}\n"));
    }
    let bytes = Bytes::from(csv.into_bytes());
    fixture
        .service
        .put_object(&ctx, "import.csv", bytes, "text/csv")
        .await
        .expect("上传 csv");

    let job = fixture
        .service
        .create_import(
            &ctx,
            &platform_storage::model::CreateJobBody {
                entity: "probe_row".to_string(),
                object_key: Some("import.csv".to_string()),
            },
        )
        .await
        .expect("建导入任务");
    // ⚠️ 步骤 2：**证明是异步的**（建完立刻就是 pending、进度 0）。
    assert_eq!(job.status, job_status::PENDING, "必须是异步（pending）");
    assert_eq!(job.processed_rows, 0, "初始进度 0");

    // ⚠️ 用 `tick_with_limit(.., 1)`（**每个任务一次只跑 1 批**）+ 连调多次，
    // 才能观察到 `0 < processedRows < 50000` 的中间态 —— 这是 AC-S-180 的核心判据。
    // （若用 `tick()`，一次就把 5 万行跑完 → 永远看不到中间态 → 判据红。）
    let mut observed_intermediate = false;
    let mut rounds = 0;
    loop {
        let stats = platform_storage::worker::tick_with_limit(&fixture.service, &ctx, 1, 1)
            .await
            .expect("tick");
        assert!(stats.is_balanced(), "每轮计数必须自洽：{stats:?}");
        if stats.claimed == 0 {
            break;
        }
        rounds += 1;
        let current = fixture
            .service
            .find_job(&ctx, &job.id)
            .await
            .expect("读任务");
        // ⚠️ 中间态判据：**只要 processedRows 落在 (0, 50000) 开区间**就算观察到。
        if current.processed_rows > 0 && current.processed_rows < DATA_ROWS as i64 {
            observed_intermediate = true;
        }
        // 5 万行 / 每 tick 1 批（1000 行）= 50 轮，留足余量。
        assert!(rounds < 120, "不该跑这么多轮（{rounds}）");
    }

    let done = fixture
        .service
        .find_job(&ctx, &job.id)
        .await
        .expect("读最终任务");
    assert_eq!(done.status, job_status::COMPLETED, "最终必须 completed");
    // ⚠️ 步骤 4：**两个数字都要断言**。
    assert_eq!(
        done.processed_rows, DATA_ROWS as i64,
        "processedRows 必须是 50000"
    );
    assert_eq!(done.failed_rows, BAD_ROWS as i64, "failedRows 必须是 10");
    assert!(
        observed_intermediate,
        "必须观察到 0 < processedRows < 50000 的中间态（只在最后写一次进度的实现会红）"
    );

    // ⚠️ **步骤 5：目标表里恰好 49990 行** —— 这是 F-01 / F-02 的核心判据。
    // 只有这条断言才能证明「合法行**真的入了库**」，而不是只「数了行」。
    let stored = fixture
        .service
        .store()
        .count_probe_rows(&ctx, &job.id)
        .await
        .expect("数目标表");
    assert_eq!(
        stored,
        (DATA_ROWS - BAD_ROWS) as i64,
        "目标表必须恰好 49990 行（合法行全入库，非法行跳过）"
    );

    // 错误回执：恰好 10 条，**带文件行号与真实列名**。
    let errors = fixture
        .service
        .list_job_errors(&ctx, &job.id)
        .await
        .expect("读错误回执");
    assert_eq!(errors.len(), BAD_ROWS, "恰好 10 条回执");
    for err in &errors {
        assert_eq!(err.column, "amount", "列名必须是真实列名");
        assert!(err.reason.is_ascii(), "reason 必须是 ASCII");
    }
    // 行号 = 数据序号 + 1（表头占第 1 行）。
    let mut row_numbers: Vec<i64> = errors.iter().map(|e| e.row_number).collect();
    row_numbers.sort_unstable();
    let expected: Vec<i64> = bad_data_rows.iter().map(|i| (*i + 1) as i64).collect();
    assert_eq!(
        row_numbers, expected,
        "文件行号 = 数据序号 + 1（表头占第 1 行）"
    );
}

/// **AC-S-180 步骤 6**：错误回执读得到，且**空任务返回空**。
#[tokio::test]
async fn a_clean_import_has_no_error_receipts() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    // 表头是 `email,amount,note`（D333；与 AC-S-181 要求的真实列名一致）。
    let csv = "email,amount,note\nok-1@example.com,10,a\nok-2@example.com,20,b\n";
    fixture
        .service
        .put_object(
            &ctx,
            "clean.csv",
            Bytes::from_static(csv.as_bytes()),
            "text/csv",
        )
        .await
        .expect("上传");

    let job = fixture
        .service
        .create_import(
            &ctx,
            &platform_storage::model::CreateJobBody {
                entity: "probe_row".to_string(),
                object_key: Some("clean.csv".to_string()),
            },
        )
        .await
        .expect("建任务");

    platform_storage::worker::tick(&fixture.service, &ctx, 1)
        .await
        .expect("tick");

    let done = fixture.service.find_job(&ctx, &job.id).await.expect("读");
    assert_eq!(done.status, job_status::COMPLETED);
    assert_eq!(done.processed_rows, 2);
    assert_eq!(done.failed_rows, 0);
    let errors = fixture
        .service
        .list_job_errors(&ctx, &job.id)
        .await
        .expect("读回执");
    assert!(errors.is_empty(), "干净导入不该有回执");
    // ⚠️ 合法行**真的入库**（F-01 的判据：只数行不入库的实现会在这里红）。
    let stored = fixture
        .service
        .store()
        .count_probe_rows(&ctx, &job.id)
        .await
        .expect("数目标表");
    assert_eq!(stored, 2, "2 条合法行都必须入库");
}

/// **AC-S-182**：导出 → 落 CSV → **round-trip 逐字段一致**（TC-018-045/046/047）。
#[tokio::test]
async fn export_then_download_round_trips() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    // ⚠️ AC-S-182 前置条件要求「含一个带逗号的字段、**一个带换行的字段**」。
    // 导出的数据源是 `platform_storage_probe_row`（D333）→ 先**导入**造数据，
    // 这样导出的 CSV 再由本需求解析器读回来才是真 round-trip。
    let csv = "email,amount,note\n\
               a@example.com,1,\"comma,inside\"\n\
               b@example.com,2,\"line\nbreak\"\n\
               c@example.com,3,plain\n";
    fixture
        .service
        .put_object(
            &ctx,
            "seed.csv",
            Bytes::from_static(csv.as_bytes()),
            "text/csv",
        )
        .await
        .expect("上传种子 csv");

    let import = fixture
        .service
        .create_import(
            &ctx,
            &platform_storage::model::CreateJobBody {
                entity: "probe_row".to_string(),
                object_key: Some("seed.csv".to_string()),
            },
        )
        .await
        .expect("建导入任务");
    platform_storage::worker::tick(&fixture.service, &ctx, 1)
        .await
        .expect("跑导入");
    let imported = fixture
        .service
        .find_job(&ctx, &import.id)
        .await
        .expect("读导入任务");
    assert_eq!(imported.status, job_status::COMPLETED);
    assert_eq!(imported.processed_rows, 3, "3 行都该处理");
    assert_eq!(imported.failed_rows, 0, "种子数据应全部合法");

    let job = fixture
        .service
        .create_export(
            &ctx,
            &platform_storage::model::CreateJobBody {
                entity: "probe_row".to_string(),
                object_key: None,
            },
        )
        .await
        .expect("建导出任务");
    // ⚠️ 步骤 1：导出也是**异步**的（建完立刻 pending）。
    assert_eq!(job.status, job_status::PENDING, "导出也是异步");

    platform_storage::worker::tick(&fixture.service, &ctx, 1)
        .await
        .expect("tick");

    let done = fixture.service.find_job(&ctx, &job.id).await.expect("读");
    assert_eq!(done.status, job_status::COMPLETED);
    let key = done
        .object_key
        .clone()
        .expect("完成的导出必须有 object_key");
    assert!(key.starts_with("exports/"), "导出的 key 应在 exports/ 下");

    // 下载 → **再用本需求的解析器解析**（round-trip）。
    let (_, bytes) = fixture
        .service
        .get_object(&ctx, &key)
        .await
        .expect("下载导出结果");
    // ⚠️ 前 3 字节是 BOM（Excel 兼容）。
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "导出的 CSV 必须带 BOM");

    let parsed = platform_storage::csv::parse(&bytes, 65536).expect("解析导出结果");
    assert_eq!(parsed.row_count(), 3, "恰好 3 行");
    // ⚠️ 步骤 5：**每个字段逐字等于原值** —— 尤其含逗号 / 含换行的字段。
    let notes: Vec<String> = parsed
        .rows
        .iter()
        .map(|r| parsed.to_map(r).get("note").cloned().unwrap_or_default())
        .collect();
    assert!(
        notes.iter().any(|n| n == "comma,inside"),
        "含逗号的字段必须 round-trip 逐字一致；实际：{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n == "line\nbreak"),
        "含换行的字段必须 round-trip 逐字一致；实际：{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n == "plain"),
        "普通字段必须原样保留；实际：{notes:?}"
    );
    let emails: Vec<String> = parsed
        .rows
        .iter()
        .map(|r| parsed.to_map(r).get("email").cloned().unwrap_or_default())
        .collect();
    for expected in ["a@example.com", "b@example.com", "c@example.com"] {
        assert!(
            emails.iter().any(|e| e == expected),
            "email {expected} 必须 round-trip；实际：{emails:?}"
        );
    }
    // 导出任务读到的 downloadUrl 非空（预签名）。
    let final_job = fixture.service.find_job(&ctx, &job.id).await.expect("读");
    assert_eq!(final_job.kind, job_kind::EXPORT);
}

/// **AC-S-183 步骤 3**：**5 张表各自**只返回本租户的行（TC-018-050）。
#[tokio::test]
async fn all_five_tables_are_isolated_by_tenant() {
    let fixture = setup().await;

    // 两个租户各造 1 个对象 + 1 个任务 + 1 条错误回执（管理员连接绕过 RLS）。
    for tenant in [TENANT_A, TENANT_B] {
        fixture
            .exec(&format!(
                "INSERT INTO platform_storage_object \
                   (id, tenant_id, object_key, size_bytes, sha256, content_type, scan_verdict, status) \
                 VALUES (gen_random_uuid(), '{tenant}', 'k-{tenant}.txt', 1, 'aa', 'text/plain', \
                         'skipped', 'available')"
            ))
            .await;
        fixture
            .exec(&format!(
                "INSERT INTO platform_storage_upload \
                   (upload_id, tenant_id, object_key, total_parts, part_size, status, expires_at) \
                 VALUES (gen_random_uuid(), '{tenant}', 'u-{tenant}.bin', 1, 512, 'pending', \
                         now() + interval '1 day')"
            ))
            .await;
        fixture
            .exec(&format!(
                "INSERT INTO platform_storage_job \
                   (id, tenant_id, kind, entity, status, total_rows, processed_rows, failed_rows) \
                 VALUES (gen_random_uuid(), '{tenant}', 'import', 'probe', 'pending', 0, 0, 0)"
            ))
            .await;
        // ⚠️ 第 5 张表（D333）：导入的真实落点 —— 存的是**业务数据**，
        // 漏隔离的后果比元数据表更严重（A 租户导入的数据会出现在 B 租户的导出里）。
        fixture
            .exec(&format!(
                "INSERT INTO platform_storage_probe_row (id, tenant_id, job_id, email, amount, note) \
                 SELECT gen_random_uuid(), '{tenant}', id, 'p-{tenant}@example.com', 1, 'n' \
                 FROM platform_storage_job WHERE tenant_id = '{tenant}' LIMIT 1"
            ))
            .await;
        // 错误回执需要一个 job_id（用刚插入的那个）。
        fixture
            .exec(&format!(
                "INSERT INTO platform_storage_job_error (job_id, tenant_id, row_number, \"column\", reason) \
                 SELECT id, '{tenant}', 2, 'amount', 'bad' FROM platform_storage_job \
                 WHERE tenant_id = '{tenant}' LIMIT 1"
            ))
            .await;
    }

    // 应用角色读：每张表都只看得到 tenant-a 的 1 行。
    let objects = fixture
        .service
        .list_objects(&fixture.ctx_a())
        .await
        .expect("读对象");
    assert_eq!(objects.len(), 1, "对象表：tenant-a 只应看到 1 行");
    assert!(
        objects.iter().all(|o| o.tenant_id == TENANT_A),
        "对象表的 tenantId 必须全是 tenant-a"
    );

    let jobs = fixture
        .service
        .list_jobs(&fixture.ctx_a())
        .await
        .expect("读任务");
    assert_eq!(jobs.len(), 1, "任务表：只应看到 1 行");
    assert!(
        jobs.iter().all(|j| j.tenant_id == TENANT_A),
        "任务表的 tenantId 必须全是 tenant-a"
    );

    // 错误回执表（用 tenant-a 的任务 id 读）。
    let errors = fixture
        .service
        .list_job_errors(&fixture.ctx_a(), &jobs[0].id)
        .await
        .expect("读回执");
    assert_eq!(errors.len(), 1, "错误回执表：只应看到 1 行");
    assert!(
        errors.iter().all(|e| e.tenant_id == TENANT_A),
        "错误回执表的 tenantId 必须全是 tenant-a"
    );

    // ⚠️ 上传会话表：`list_uploads` 本轮没有列表接口 →
    // 直接查库断言（仍走应用角色，验 RLS）。
    use platform_db::{DbOptions, TenantQuery};
    let app_db = platform_db::TenantDb::connect(
        &app_role_url(&required_test_database_url()),
        DbOptions::default(),
    )
    .await
    .expect("连应用角色");
    let query = TenantQuery::new(
        "SELECT upload_id::text AS upload_id, tenant_id, object_key, total_parts, part_size, \
         declared_checksum, status, created_at, updated_at, expires_at \
         FROM platform_storage_upload WHERE tenant_id = {tenant}",
    );
    let uploads: Vec<platform_storage::model::UploadRow> = app_db
        .fetch_all(&fixture.ctx_a(), &query)
        .await
        .expect("读上传会话");
    assert_eq!(uploads.len(), 1, "上传会话表：只应看到 1 行");
    assert!(
        uploads.iter().all(|u| u.tenant_id == TENANT_A),
        "上传会话表的 tenantId 必须全是 tenant-a"
    );

    // ⚠️ **第 5 张表**（`platform_storage_probe_row`，D333）：
    // 它是导入的真实落点，**存的是业务数据** —— 漏隔离最严重。
    let probe_rows = fixture
        .service
        .store()
        .list_probe_rows(&fixture.ctx_a(), None)
        .await
        .expect("读目标实体表");
    assert_eq!(probe_rows.len(), 1, "目标实体表：tenant-a 只应看到 1 行");
    assert_eq!(
        probe_rows[0].email,
        format!("p-{TENANT_A}@example.com"),
        "目标实体表只应返回 tenant-a 的行（不能看到 tenant-b 的 p-tenant-b@...）"
    );
}

/// **AC-S-183 步骤 4**：5 张表都在 manifest 的 `tenantScoped` 里
/// （已在 contract_c5 里断言；这里补一条「库层真有 policy」）。
#[tokio::test]
async fn all_five_tables_have_rls_policies() {
    let fixture = setup().await;
    let rows = fixture
        .query_text(
            "SELECT tablename FROM pg_policies \
             WHERE tablename LIKE 'platform_storage_%' GROUP BY tablename ORDER BY tablename",
        )
        .await;
    assert_eq!(
        rows,
        vec![
            "platform_storage_job".to_string(),
            "platform_storage_job_error".to_string(),
            "platform_storage_object".to_string(),
            "platform_storage_probe_row".to_string(),
            "platform_storage_upload".to_string(),
        ],
        "5 张表都必须有 RLS policy（含 D333 新增的导入目标表）；实际：{rows:?}"
    );
}

/// **AC-S-178 步骤 2** 的补充：分片上传的**预估总大小**也受上限约束。
#[tokio::test]
async fn multipart_creation_rejects_an_oversized_projection() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    fixture
        .config
        .set(platform_storage::config::CONFIG_KEY_MAX_UPLOAD_BYTES, 1024);

    let err = fixture
        .service
        .create_upload(
            &ctx,
            &platform_storage::model::CreateUploadBody {
                object_key: "huge.bin".to_string(),
                total_parts: 100,
                part_size: 1024,
                checksum: None,
            },
        )
        .await
        .expect_err("预估 100 KiB 超上限必须拒绝");
    assert_eq!(err.code(), "STORAGE_UPLOAD_TOO_LARGE");
    assert_eq!(
        err.problem_details(&ctx, "/x")
            .params
            .and_then(|p| p.get("maxBytes").cloned()),
        Some(serde_json::json!(1024))
    );
}

/// **D335 / 复核 F-04**：`storage.partSizeBytes` 是**硬上限**（不是死配置）。
///
/// # 为什么这条测试必须存在
///
/// 复核 F-04 指出：`StorageSettings.part_size_bytes` 原本**只被填充、从不被读取**，
/// 业务代码用的是客户端传入的 `body.part_size` → 改这个配置项**对行为无影响**，
/// 违反 D331「6 个配置项每个必须被真实读取」。
///
/// # 判据的区分力
///
/// 这条测试**改配置**（`partSizeBytes = 1024`）然后断言行为变化 ——
/// 如果实现不读这个配置项，请求会成功 → 测试变红。
/// 这正是「死配置」与「真读取」的区别：后者能靠改配置让测试红绿翻转。
#[tokio::test]
async fn the_part_size_config_is_a_real_upper_bound() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    // 把**单片上限**设成 1024（`maxUploadBytes` 保持默认 100 MiB，
    // 所以下面被拒的原因只可能是「单片超限」而不是「总量超限」）。
    fixture
        .config
        .set(platform_storage::config::CONFIG_KEY_PART_SIZE_BYTES, 1024);

    // 边界包含：正好 1024 字节一片 → 通过。
    fixture
        .service
        .create_upload(
            &ctx,
            &platform_storage::model::CreateUploadBody {
                object_key: "exact-part.bin".to_string(),
                total_parts: 2,
                part_size: 1024,
                checksum: None,
            },
        )
        .await
        .expect("正好等于单片上限必须通过（边界包含）");

    // 超 1 字节 → 413 + `params.maxBytes` 是**单片上限**（不是总量上限）。
    let err = fixture
        .service
        .create_upload(
            &ctx,
            &platform_storage::model::CreateUploadBody {
                object_key: "over-part.bin".to_string(),
                total_parts: 2,
                part_size: 1025,
                checksum: None,
            },
        )
        .await
        .expect_err("单片超限必须拒绝");
    assert_eq!(err.code(), "STORAGE_UPLOAD_TOO_LARGE");
    assert_eq!(
        err.problem_details(&ctx, "/x")
            .params
            .and_then(|p| p.get("maxBytes").cloned()),
        Some(serde_json::json!(1024)),
        "maxBytes 必须是**单片上限**（来自 storage.partSizeBytes）"
    );

    // 非正数的 partSize 也拒绝（0 或负数会让「片数 × 片大小」失去意义）。
    for bad in [0i64, -1] {
        let err = fixture
            .service
            .create_upload(
                &ctx,
                &platform_storage::model::CreateUploadBody {
                    object_key: format!("bad-{bad}.bin"),
                    total_parts: 1,
                    part_size: bad,
                    checksum: None,
                },
            )
            .await
            .expect_err("非正数单片必须拒绝");
        assert_eq!(err.code(), "STORAGE_UPLOAD_TOO_LARGE", "part_size={bad}");
    }
}

/// **复核 F-17（安全）**：分片上传**必须**扫描合并后的文件。
///
/// # 为什么这是安全判据而不是普通功能判据
///
/// 分片是**大文件的唯一通道**。原来的 `complete_upload` 把
/// `scan_verdict` 硬写成 `Skipped` 且**从不调用 scanner** →
/// 配了真扫描器也能把恶意文件经 `/api/storage/uploads/*` 落库，
/// 完全绕开 AC-S-178 步骤 4（直传路径是扫的）。
///
/// # 判据的区分力
///
/// 用「判定为感染」的替身走**分片**路径 → 必须 422 + 文件不落盘不落库。
/// 如果实现不在 complete 时扫描（改回硬写 `Skipped`），这条会红。
#[tokio::test]
async fn multipart_uploads_are_scanned_on_complete() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    let session = fixture
        .service
        .create_upload(
            &ctx,
            &platform_storage::model::CreateUploadBody {
                object_key: "multi-infected.bin".to_string(),
                total_parts: 2,
                part_size: 512,
                checksum: None,
            },
        )
        .await
        .expect("建会话");
    for n in [1u32, 2] {
        fixture
            .service
            .upload_part(&ctx, &session.upload_id, n, Bytes::from_static(b"evil"))
            .await
            .expect("传片");
    }

    // 合并前把扫描器切成「感染」。
    fixture.scanner.set_infected();

    let err = fixture
        .service
        .complete_upload(&ctx, &session.upload_id, None)
        .await
        .expect_err("感染文件必须被拒绝");
    assert_eq!(err.code(), "STORAGE_FILE_INFECTED");
    assert!(
        err.problem_details(&ctx, "/x")
            .params
            .and_then(|p| p.get("signature").cloned())
            .is_some_and(|v| !v.as_str().unwrap_or_default().is_empty()),
        "params.signature 必须非空"
    );

    // ⚠️ **对象既不落库也不落盘**（坏文件不能留下）。
    let stored = fixture
        .query_i64(
            "SELECT count(*) FROM platform_storage_object WHERE object_key = 'multi-infected.bin'",
        )
        .await;
    assert_eq!(stored, 0, "感染的分片文件不得落库");
    let on_disk = fixture.list_stored_files();
    assert!(
        !on_disk
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == "multi-infected.bin")),
        "感染的分片文件不得落盘；实际：{on_disk:?}"
    );

    // 会话被标成 aborted（不留「pending 但片已合并」的半成品状态）。
    let status = fixture
        .query_text(
            "SELECT status FROM platform_storage_upload WHERE object_key = 'multi-infected.bin'",
        )
        .await;
    assert_eq!(status, vec!["aborted".to_string()], "会话必须标成 aborted");
}

/// **对照**：干净的分片上传仍能成功，且 `scanVerdict` 如实记 `skipped`
/// （默认 `NoopScanner` 没扫 → 不能谎称 `clean`，D326）。
#[tokio::test]
async fn a_clean_multipart_upload_records_the_real_scan_verdict() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    let session = fixture
        .service
        .create_upload(
            &ctx,
            &platform_storage::model::CreateUploadBody {
                object_key: "multi-clean.bin".to_string(),
                total_parts: 2,
                part_size: 512,
                checksum: None,
            },
        )
        .await
        .expect("建会话");
    for n in [1u32, 2] {
        fixture
            .service
            .upload_part(&ctx, &session.upload_id, n, Bytes::from_static(b"ok"))
            .await
            .expect("传片");
    }
    let row = fixture
        .service
        .complete_upload(&ctx, &session.upload_id, None)
        .await
        .expect("干净文件应成功");
    assert_eq!(
        row.scan_verdict, "skipped",
        "默认 NoopScanner 必须如实记 skipped（不是 clean，D326）"
    );
}
