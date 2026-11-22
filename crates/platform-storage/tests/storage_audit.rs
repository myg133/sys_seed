//! **AC-S-184 步骤 5** 的真库测试：4 个审计事件码真的进 `platform_audit_log`（D334）。
//!
//! # 这个文件为什么存在
//!
//! 实现复核的 **F-03（BLOCKER）** 指出：4 个 `storage.*` 事件码在 `events.rs` 里
//! **只有常量定义**，全仓零发出点 → AC-S-184 步骤 5 整条未实现。
//! 本文件是该判据的唯一验证入口 —— **删掉它 F-03 就会复发**。
//!
//! # 验的是「真有 4 条」，不是「有审计表」
//!
//! 判据必须能区分「埋点生效」和「埋点没生效」：
//!
//! - 断言**每个事件码恰好 1 条**（不是「至少有审计行」—— 那样上传成功、
//!   删除漏埋点时也会绿）；
//! - 断言**事件码集合双向相等**于显式列出的 4 个（漏一个就红）；
//! - 断言 `ChainVerifier::verify` 的 `ok == true`（哈希链没断）。

mod common;

use bytes::Bytes;
use common::setup;
use platform_core::RequestContext;
use platform_core::extension::audit::AuditSink;
use std::sync::Arc;

/// ⚠️ **本文件专用的租户**，不复用 `tenant-a`。
///
/// 理由：`platform_audit_log` 是 **append-only** 的（哈希链，删一行就断链），
/// 所以**不能靠清表**做用例隔离。而其他用例（如 sandbox 的 demo 事件）
/// 也在 `tenant-a` 名下写审计 → 共用租户会让「恰好 4 条」的断言被别人的行污染。
///
/// → 审计类测试**必须**用专属租户，让「本租户的审计行」恰好等于「本用例产生的行」。
const AUDIT_TENANT: &str = "tenant-req018-audit";
/// 第二条用例的专属租户（它断言「零条审计」，必须与第一条完全隔开）。
const QUIET_TENANT: &str = "tenant-req018-noaudit";

/// 走完「上传 → 删除 → 导入 → 导出」，断言 4 个事件码各落 1 条审计。
#[tokio::test]
async fn the_four_storage_events_reach_the_audit_log() {
    let fixture = setup().await;

    // ① 装配 PgAuditSink（**真**落库，不是测试替身）。
    //    复用 fixture 的管理员连接池 —— 审计表没有 RLS 之外的额外限制，
    //    但为了与生产一致，仍然走 `TenantDb` 作用域入口。
    let audit_db = platform_db::TenantDb::from_pool(
        fixture.admin.clone(),
        platform_db::DbOptions::default(),
    );
    let sink: Arc<dyn AuditSink> = Arc::new(platform_audit::PgAuditSink::new(audit_db.clone()));
    assert!(
        fixture.service.with_audit_sink(sink),
        "sink 应当装配成功（`OnceLock` 只允许设一次）"
    );

    // 审计事件要带 actor / ip / ua（契约 C12），这里给全。
    let ctx = RequestContext::new()
        .with_tenant(AUDIT_TENANT)
        .with_user("user-018")
        .with_client_ip("10.0.0.18")
        .with_user_agent("req018-audit-test");

    // ⚠️ **基线快照**：`platform_audit_log` 是 append-only（哈希链，不能删），
    // 所以本用例**不可能**靠清表做到幂等 —— 必须用「运行前后的**差值**」断言。
    // 不做基线的话，第二次跑会因为上一次的行还在而把计数翻倍（本用例初版就踩了）。
    let baseline = counts_by_action(&fixture, AUDIT_TENANT).await;

    // ② 上传 → `storage.file.uploaded`。
    fixture
        .service
        .put_object(
            &ctx,
            "audit/doc.txt",
            Bytes::from_static(b"audited"),
            "text/plain",
        )
        .await
        .expect("上传应当成功");

    // ③ 删除 → `storage.file.deleted`。
    fixture
        .service
        .delete_object(&ctx, "audit/doc.txt")
        .await
        .expect("删除应当成功");

    // ④ 导入 → `storage.import.completed`。
    let csv = "email,amount,note\nrow@example.com,1,ok\n";
    fixture
        .service
        .put_object(
            &ctx,
            "audit/seed.csv",
            Bytes::from_static(csv.as_bytes()),
            "text/csv",
        )
        .await
        .expect("上传种子 csv");
    fixture
        .service
        .create_import(
            &ctx,
            &platform_storage::model::CreateJobBody {
                entity: "probe_row".to_string(),
                object_key: Some("audit/seed.csv".to_string()),
            },
        )
        .await
        .expect("建导入任务");
    platform_storage::worker::tick(&fixture.service, &ctx, 1)
        .await
        .expect("跑导入");

    // ⑤ 导出 → `storage.export.completed`。
    fixture
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
    platform_storage::worker::tick(&fixture.service, &ctx, 1)
        .await
        .expect("跑导出");

    // ⑥ 断言：本次运行**新增**的 4 个事件码各恰好 1 条（用差值，不用绝对值）。
    //
    // ⚠️ 判据是「**码的集合双向相等**（4 个不多不少）」+「每个码恰好新增 1 条」。
    // 不能写「包含这 4 个码」—— 那样漏埋一个点、但别的点重复时仍会绿。
    let after = counts_by_action(&fixture, AUDIT_TENANT).await;
    let delta = |action: &str| -> i64 {
        after.get(action).copied().unwrap_or(0) - baseline.get(action).copied().unwrap_or(0)
    };

    // 每个码期望的**新增条数**。`file.uploaded` 是 2 —— 主对象 1 次 + 种子 CSV 1 次，
    // 两次都是**真实发生**的上传（不是重复计数）。
    let expected: [(&str, i64); 4] = [
        ("storage.file.uploaded", 2),
        ("storage.file.deleted", 1),
        ("storage.import.completed", 1),
        ("storage.export.completed", 1),
    ];
    // ① 这 4 个码**都**必须按期望新增（漏一个埋点就红）。
    for (action, want) in expected {
        assert_eq!(
            delta(action),
            want,
            "事件码 {action} 必须**恰好新增 {want} 条**；实际新增 {}（全部计数：{after:?}）",
            delta(action)
        );
    }
    // ② 没有**第 5 个** storage 事件码混进来（防「埋点写错名字」）。
    let mut seen: Vec<&str> = after
        .keys()
        .map(String::as_str)
        .filter(|k| k.starts_with("storage."))
        .collect();
    seen.sort_unstable();
    let mut expected_sorted: Vec<&str> = expected.iter().map(|(a, _)| *a).collect();
    expected_sorted.sort_unstable();
    assert_eq!(
        seen, expected_sorted,
        "storage 事件码必须**恰好这 4 个**（双向相等，不是「包含」）"
    );

    // ⑦ 每条都要带上契约 C12 的固定字段（不能是空壳事件）。
    let incomplete = fixture
        .query_i64(
            &format!(
                "SELECT count(*) FROM platform_audit_log \
                 WHERE tenant_id = '{AUDIT_TENANT}' \
                   AND (actor_id IS NULL OR actor_id = '' \
                        OR ip IS NULL \
                        OR user_agent IS NULL \
                        OR object_type IS NULL OR object_type = '')"
            ),
        )
        .await;
    assert_eq!(
        incomplete, 0,
        "每条审计事件都必须带 actor / ip / user_agent / object_type（契约 C12）"
    );

    // ⑧ 哈希链完好（`ChainVerifier::verify` → `ok == true`）。
    let verifier = platform_audit::ChainVerifier::new(audit_db);
    let report = verifier.verify(&ctx).await.expect("校验审计链");
    assert!(report.ok, "审计链表必须完好：{report:?}");
    assert!(
        report.checked >= 4,
        "至少要检查到那 4 条（实际 {}）",
        report.checked
    );
}

/// 按事件码统计某租户的审计条数（`action -> count`）。
///
/// ⚠️ 因为审计表 append-only，所有断言都必须基于**前后差值**。
async fn counts_by_action(
    fixture: &common::Fixture,
    tenant: &str,
) -> std::collections::BTreeMap<String, i64> {
    let rows = fixture
        .query_text(&format!(
            "SELECT action || '|' || count(*)::text FROM platform_audit_log \
             WHERE tenant_id = '{tenant}' GROUP BY action"
        ))
        .await;
    let mut map = std::collections::BTreeMap::new();
    for row in rows {
        let (action, count) = row.split_once('|').expect("格式应是 action|count");
        map.insert(action.to_string(), count.parse().expect("count 应是整数"));
    }
    map
}

/// **未装配 sink 时不 panic**（D334 的失败语义：存储不因审计失败而失败）。
///
/// ⚠️ 这条是「装配漏了会怎样」的护栏：服务仍然能用，只打 warn。
/// 与 IAM 的登录审计**相反**（那边是拒绝登录）—— 理由见 `record_audit` 的注释。
#[tokio::test]
async fn storage_still_works_without_an_audit_sink() {
    let fixture = setup().await;
    // ⚠️ 用**专属租户**：本用例断言「零条审计行」，
    // 与 `tenant-a` 共用的话会被别的用例的审计行污染。
    let ctx = RequestContext::new().with_tenant(QUIET_TENANT);

    // 故意**不**装配 sink。
    fixture
        .service
        .put_object(
            &ctx,
            "no-audit/x.txt",
            Bytes::from_static(b"x"),
            "text/plain",
        )
        .await
        .expect("未装配审计时上传也必须成功（审计不是准入控制）");

    let (row, bytes) = fixture
        .service
        .get_object(&ctx, "no-audit/x.txt")
        .await
        .expect("读回");
    assert_eq!(&bytes[..], b"x");
    assert_eq!(row.object_key, "no-audit/x.txt");

    // 而且**没有**任何审计行（证明上一条真的没写，不是靠别处兜底）。
    let count = fixture
        .query_i64(&format!(
            "SELECT count(*) FROM platform_audit_log WHERE tenant_id = '{QUIET_TENANT}'"
        ))
        .await;
    assert_eq!(count, 0, "未装配 sink 时不该有审计行");
}
