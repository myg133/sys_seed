//! REQ-014 的哈希链用例：分链、独立复算、并发不分叉、篡改可检测。
//!
//! 覆盖 AC-S-022 / AC-S-124 / AC-S-125。
//!
//! # 复算纪律（D141 / D151）
//!
//! 测试侧**独立实现** sha256 + 长度前缀拼接，**不调用**生产代码的哈希函数；
//! `before` / `after` **直接用列里存的那段文本** —— 解析回 JSON 再序列化是
//! 「验证生产代码的拷贝」，且会因序列化差异假红。

mod common;

use common::{Fixture, TENANT_A, TENANT_B, USER_M};
use platform_audit::{AuditEvent, ChainVerifier, PgAuditSink};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use time::OffsetDateTime;

/// 写一条事件（走平台 sink）。
async fn write(
    fixture: &Fixture,
    tenant: &str,
    action: &str,
) -> Result<String, platform_core::extension::audit::AuditError> {
    let ctx = if tenant == TENANT_A {
        fixture.ctx_tenant_a()
    } else {
        fixture.ctx_tenant_b()
    };
    let sink = PgAuditSink::new(fixture.app_db.clone());
    let event = AuditEvent::new(tenant, action, "demo_item")
        .with_actor(USER_M)
        .with_object_id("obj-1")
        .with_before(json!({"b": 1, "a": 2}))
        .with_after(json!({"k": "中文"}));
    sink.record_event(&ctx, &event).await
}

/// 读回该租户的全部记录（按 `created_at ASC, id ASC`）。
async fn rows(fixture: &Fixture, tenant: &str) -> Vec<sqlx::postgres::PgRow> {
    sqlx::query(
        "SELECT id, tenant_id, actor_id, action, object_type, object_id, \"before\", \
         \"after\", ip, user_agent, created_at, prev_hash, hash \
         FROM platform_audit_log WHERE tenant_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(tenant)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能读回审计记录")
}

const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// AC-S-124 / TC-014-017 ~ 020：分链 + 接续 + 首条是 genesis。
#[tokio::test]
async fn the_chain_is_linked_per_tenant() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    for index in 0..3 {
        write(&fixture, TENANT_A, &format!("demo.item.update{index}"))
            .await
            .expect("写入应当成功");
    }
    write(&fixture, TENANT_B, "demo.item.update")
        .await
        .expect("写入应当成功");

    let chain_a = rows(&fixture, TENANT_A).await;
    assert_eq!(chain_a.len(), 3);
    assert_eq!(
        chain_a[0].try_get::<String, _>("prev_hash").unwrap(),
        GENESIS,
        "本租户首条的 prev_hash 必须是 genesis（恰好 64 个 0）"
    );
    assert_eq!(
        chain_a[1].try_get::<String, _>("prev_hash").unwrap(),
        chain_a[0].try_get::<String, _>("hash").unwrap()
    );
    assert_eq!(
        chain_a[2].try_get::<String, _>("prev_hash").unwrap(),
        chain_a[1].try_get::<String, _>("hash").unwrap()
    );
    let hash_0 = chain_a[0].try_get::<String, _>("hash").unwrap();
    let hash_1 = chain_a[1].try_get::<String, _>("hash").unwrap();
    let hash_2 = chain_a[2].try_get::<String, _>("hash").unwrap();
    assert!(hash_0 != hash_1 && hash_1 != hash_2 && hash_0 != hash_2);

    // 另一个租户**独立成链**（不接 tenant-a 的尾）。
    let chain_b = rows(&fixture, TENANT_B).await;
    assert_eq!(chain_b.len(), 1);
    assert_eq!(
        chain_b[0].try_get::<String, _>("prev_hash").unwrap(),
        GENESIS,
        "另一个租户必须从 genesis 开始（分链）"
    );
    assert_ne!(
        chain_b[0].try_get::<String, _>("prev_hash").unwrap(),
        hash_2,
        "另一个租户不得接上本租户的尾"
    );
}

/// AC-S-124 步骤 4 / TC-014-021 ~ 025：测试侧**独立复算**逐条比对。
#[tokio::test]
async fn the_stored_hash_can_be_recomputed_independently() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    write(&fixture, TENANT_A, "demo.item.update")
        .await
        .expect("写入应当成功");
    let chain = rows(&fixture, TENANT_A).await;
    assert_eq!(chain.len(), 1);

    let row = &chain[0];
    let recomputed = recompute(row);
    assert_eq!(
        recomputed,
        row.try_get::<String, _>("hash").unwrap(),
        "独立复算必须与存储值逐字节相同"
    );

    // TC-014-023：长度前缀是**字节长度**（中文值多字节）。
    let after = row.try_get::<Option<String>, _>("after").unwrap().unwrap();
    assert!(
        after.len() > after.chars().count(),
        "夹具必须含多字节字符（否则字节/字符对照不成立）"
    );

    // TC-014-024：`prev_hash` 以 **64 字节文本**参与（不是 32 字节二进制）。
    let prev_hash = row.try_get::<String, _>("prev_hash").unwrap();
    assert_eq!(prev_hash.len(), 64);
    let binary_variant = recompute_with_binary_prev(row);
    assert_ne!(
        binary_variant,
        row.try_get::<String, _>("hash").unwrap(),
        "用二进制 prev_hash 算出的哈希必须**不同**"
    );
}

/// AC-S-124 / TC-014-026：同租户两次写入的 `created_at` **严格递增**。
#[tokio::test]
async fn the_created_at_is_strictly_increasing_per_tenant() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    write(&fixture, TENANT_A, "demo.first")
        .await
        .expect("写入应当成功");
    write(&fixture, TENANT_A, "demo.second")
        .await
        .expect("写入应当成功");
    let chain = rows(&fixture, TENANT_A).await;
    assert_eq!(chain.len(), 2);
    let first: OffsetDateTime = chain[0].try_get("created_at").unwrap();
    let second: OffsetDateTime = chain[1].try_get("created_at").unwrap();
    assert!(
        second > first,
        "同租户的 created_at 必须严格递增（{first} → {second}）"
    );
}

/// 🔑 AC-S-124 步骤 5~6 / D161：**并发**写入同租户**不产生分叉**。
#[tokio::test]
async fn concurrent_writes_to_one_tenant_do_not_fork_the_chain() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    // 两个**独立连接**同时写同租户（不等待前一个完成）。
    let first = write(&fixture, TENANT_A, "demo.concurrent.a");
    let second = write(&fixture, TENANT_A, "demo.concurrent.b");
    let (first, second) = tokio::join!(first, second);
    first.expect("并发写入 1 应当成功");
    second.expect("并发写入 2 应当成功");

    let chain = rows(&fixture, TENANT_A).await;
    assert_eq!(chain.len(), 2, "两条都应当写入成功");
    assert_eq!(chain[0].try_get::<String, _>("prev_hash").unwrap(), GENESIS);
    assert_eq!(
        chain[1].try_get::<String, _>("prev_hash").unwrap(),
        chain[0].try_get::<String, _>("hash").unwrap(),
        "排序后第 2 条的 prev_hash 必须是第 1 条的 hash（无分叉）"
    );
    assert_ne!(
        chain[0].try_get::<String, _>("prev_hash").unwrap(),
        chain[1].try_get::<String, _>("prev_hash").unwrap(),
        "两条记录的 prev_hash 必须互不相同"
    );

    // 链校验必须通过。
    let verifier = ChainVerifier::new(fixture.app_db.clone());
    let report = verifier
        .verify(&fixture.ctx_tenant_a())
        .await
        .expect("链校验应当能跑完");
    assert!(report.ok, "链校验必须通过：{:?}", report.broken);
    assert_eq!(report.checked, 2);
    assert!(report.broken.is_empty());
}

/// AC-S-125 案例 A：改第 2 条的 `after` → **恰好 1 条**断裂，reason == `hash_mismatch`。
#[tokio::test]
async fn tampering_with_the_after_column_is_detected() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    for index in 0..3 {
        write(&fixture, TENANT_A, &format!("demo.item.update{index}"))
            .await
            .expect("写入应当成功");
    }
    let chain = rows(&fixture, TENANT_A).await;
    let target = chain[1].try_get::<String, _>("id").unwrap();

    // 绕过应用，以管理员直连改 `after`（不动 hash）。
    sqlx::query("UPDATE platform_audit_log SET \"after\" = '{\"tampered\":true}' WHERE id = $1")
        .bind(&target)
        .execute(&fixture.admin_pool)
        .await
        .expect("管理员应当能改（模拟篡改）");

    let verifier = ChainVerifier::new(fixture.app_db.clone());
    let report = verifier
        .verify(&fixture.ctx_tenant_a())
        .await
        .expect("链校验应当能跑完");
    assert!(!report.ok, "篡改必须被检出");
    assert_eq!(report.checked, 3);
    assert_eq!(report.broken.len(), 1, "改 after 只应断裂 1 条");
    assert_eq!(report.broken[0].id, target);
    assert_eq!(report.broken[0].reason.as_str(), "hash_mismatch");
}

/// AC-S-125 案例 B：改第 2 条**存储的 `hash`** → **恰好 2 条**断裂。
#[tokio::test]
async fn tampering_with_the_stored_hash_breaks_the_next_link_too() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    for index in 0..3 {
        write(&fixture, TENANT_A, &format!("demo.item.update{index}"))
            .await
            .expect("写入应当成功");
    }
    let chain = rows(&fixture, TENANT_A).await;
    let target = chain[1].try_get::<String, _>("id").unwrap();

    // 只改**存储的 hash**（其它列不动）。
    sqlx::query("UPDATE platform_audit_log SET hash = $1 WHERE id = $2")
        .bind("f".repeat(64))
        .bind(&target)
        .execute(&fixture.admin_pool)
        .await
        .expect("管理员应当能改（模拟篡改）");

    let verifier = ChainVerifier::new(fixture.app_db.clone());
    let report = verifier
        .verify(&fixture.ctx_tenant_a())
        .await
        .expect("链校验应当能跑完");
    assert!(!report.ok, "篡改必须被检出");
    assert_eq!(report.broken.len(), 2, "改存储 hash 会连带断掉下一条");

    let second = report
        .broken
        .iter()
        .find(|entry| entry.id == target)
        .expect("第 2 条必须在断裂清单里");
    assert_eq!(
        second.reason.as_str(),
        "hash_mismatch",
        "第 2 条自身被改 → hash_mismatch"
    );

    let third_id = chain[2].try_get::<String, _>("id").unwrap();
    let third = report
        .broken
        .iter()
        .find(|entry| entry.id == third_id)
        .expect("第 3 条也必须被牵连（这证明链是**连锁**的）");
    assert_eq!(
        third.reason.as_str(),
        "prev_hash_mismatch",
        "第 3 条自己没被改 → 断掉的是**接续关系**"
    );
}

/// D144：Rust 侧手工格式化的时间戳必须与 DB 侧 `to_char` **逐字节相同**。
#[tokio::test]
async fn the_rust_timestamp_matches_the_database_one() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    write(&fixture, TENANT_A, "demo.item.update")
        .await
        .expect("写入应当成功");
    let chain = rows(&fixture, TENANT_A).await;
    let row = &chain[0];

    let db_text: String = sqlx::query_scalar(
        "SELECT to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
         FROM platform_audit_log WHERE id = $1",
    )
    .bind(row.try_get::<String, _>("id").unwrap())
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能取到 DB 侧的时间戳文本");

    let rust_text = platform_core::extension::audit::format_timestamp_micros_utc(
        &row.try_get::<OffsetDateTime, _>("created_at").unwrap(),
    );
    assert_eq!(
        rust_text, db_text,
        "两侧的时间戳文本必须逐字节相同（否则链校验永远失败）"
    );

    // 边界：微秒为 0 时也必须是 **6 位**（`time` 的 Rfc3339 在这里会漏掉小数部分）。
    let zero: String = sqlx::query_scalar(
        "SELECT to_char('2026-11-20 01:02:03'::timestamp AT TIME ZONE 'UTC', \
         'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
    )
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能取到 DB 侧的边界时间戳");
    assert_eq!(zero, "2026-11-20T01:02:03.000000Z");
    assert_eq!(zero.len(), "2026-11-20T01:02:03.000000Z".len());
}

// ---------------------------------------------------------------------------
// 辅助：测试侧**独立实现**的哈希复算
// ---------------------------------------------------------------------------

fn recompute(row: &sqlx::postgres::PgRow) -> String {
    let text = |column: &str| -> String {
        row.try_get::<Option<String>, _>(column)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let created_at: OffsetDateTime = row.try_get("created_at").expect("应当能读时间");
    let created_text = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        created_at.year(),
        created_at.month() as u8,
        created_at.day(),
        created_at.hour(),
        created_at.minute(),
        created_at.second(),
        created_at.millisecond() as u32 * 1000 + created_at.microsecond() % 1000,
    );
    let prev_hash = row.try_get::<String, _>("prev_hash").unwrap();
    let fields = [
        row.try_get::<String, _>("tenant_id").unwrap(),
        text("actor_id"),
        row.try_get::<String, _>("action").unwrap(),
        row.try_get::<String, _>("object_type").unwrap(),
        text("object_id"),
        text("before"),
        text("after"),
        row.try_get::<String, _>("ip").unwrap(),
        row.try_get::<String, _>("user_agent").unwrap(),
        created_text,
    ];
    let mut buffer: Vec<u8> = Vec::new();
    buffer.extend_from_slice(prev_hash.as_bytes());
    for field in fields {
        buffer.extend_from_slice(field.len().to_string().as_bytes());
        buffer.push(b':');
        buffer.extend_from_slice(field.as_bytes());
    }
    Sha256::digest(&buffer)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 反例：用 **32 字节二进制**的 prev_hash 参与拼接（TC-014-024 的对照组）。
fn recompute_with_binary_prev(row: &sqlx::postgres::PgRow) -> String {
    let mut buffer: Vec<u8> = Vec::new();
    let prev_hash = row.try_get::<String, _>("prev_hash").unwrap();
    let mut binary = Vec::with_capacity(32);
    let bytes = prev_hash.as_bytes();
    for chunk in bytes.chunks(2) {
        let text = std::str::from_utf8(chunk).unwrap_or("00");
        binary.push(u8::from_str_radix(text, 16).unwrap_or(0));
    }
    buffer.extend_from_slice(&binary);
    let text = |column: &str| -> String {
        row.try_get::<Option<String>, _>(column)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let created_at: OffsetDateTime = row.try_get("created_at").expect("应当能读时间");
    let created_text = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        created_at.year(),
        created_at.month() as u8,
        created_at.day(),
        created_at.hour(),
        created_at.minute(),
        created_at.second(),
        created_at.millisecond() as u32 * 1000 + created_at.microsecond() % 1000,
    );
    let fields = [
        row.try_get::<String, _>("tenant_id").unwrap(),
        text("actor_id"),
        row.try_get::<String, _>("action").unwrap(),
        row.try_get::<String, _>("object_type").unwrap(),
        text("object_id"),
        text("before"),
        text("after"),
        row.try_get::<String, _>("ip").unwrap(),
        row.try_get::<String, _>("user_agent").unwrap(),
        created_text,
    ];
    for field in fields {
        buffer.extend_from_slice(field.len().to_string().as_bytes());
        buffer.push(b':');
        buffer.extend_from_slice(field.as_bytes());
    }
    Sha256::digest(&buffer)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
