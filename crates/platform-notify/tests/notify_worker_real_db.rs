//! REQ-015 **真实库**集成用例：异步投递（多 worker 并发）与重试退避。
//!
//! 覆盖：AC-S-170、AC-S-171。
//!
//! # ⚠️ 这两条判据**只有真库能验**
//!
//! - AC-S-170：「4 个 worker 并发 `tick()` → 100 条**不重复不遗漏**」——
//!   要 `FOR UPDATE SKIP LOCKED` 的**真实行锁**；单测里没有锁；
//! - AC-S-171：「第 5 次失败进死信」—— 要真实的 `attempt_count` 累加与状态迁移。

mod common;

use common::*;
use platform_notify::model::status;
use platform_notify::store::NewDelivery;
use platform_notify::worker::{PlatformChannel, tick};
use std::sync::Arc;

/// **`prune_terminal_deliveries` 只删终态且足够旧的**（TD-083 的判据）。
///
/// # 防什么
///
/// 本表每次入队加一行，原**没有删除路径** → 无限增长。
/// 但清理**绝不能误删**还在队列里的记录 —— 那会等于丢消息。
///
/// # 判据
///
/// 造 4 条：`delivered` 旧 / `dead_letter` 旧 / `pending` 旧 / `delivered` 新。
/// `prune(older_than_days = 7)` 后：
/// - 两条**旧的终态**被删；
/// - `pending` **保留**（还在队列，删了就是丢消息）；
/// - `delivered` 新的**保留**（还在保留期内）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn prune_removes_only_old_terminal_deliveries() {
    let fixture = setup().await;
    fixture
        .admin
        .execute("DELETE FROM platform_notify_delivery")
        .await;

    // 造 4 条，`updated_at` 手工指定（序列化后由 `insert_delivery` 覆盖，
    // 所以用 SQL 直改）。
    seed_pending(&fixture, 4).await;
    // 按 id 排序取 4 个 id。
    let ids: Vec<String> = fixture
        .admin
        .query_text("SELECT id::text FROM platform_notify_delivery ORDER BY id")
        .await;

    // [0] delivered + 30 天前 → 该删
    // [1] dead_letter + 30 天前 → 该删
    // [2] pending + 30 天前 → **不该删**
    // [3] delivered + 1 小时前 → **不该删**（保留期内）
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET status='delivered', updated_at = now() - interval '30 days' WHERE id='{}'",
            ids[0]
        ))
        .await;
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET status='dead_letter', updated_at = now() - interval '30 days' WHERE id='{}'",
            ids[1]
        ))
        .await;
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET status='pending', updated_at = now() - interval '30 days' WHERE id='{}'",
            ids[2]
        ))
        .await;
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET status='delivered', updated_at = now() - interval '1 hour' WHERE id='{}'",
            ids[3]
        ))
        .await;

    let deleted = fixture
        .service
        .store()
        .prune_terminal_deliveries(&fixture.ctx_a(), 7)
        .await
        .expect("清理应当成功");
    assert_eq!(deleted, 2, "应当恰好删 2 条（两条旧的终态）");

    let remaining: Vec<String> = fixture
        .admin
        .query_text("SELECT status FROM platform_notify_delivery ORDER BY status")
        .await;
    assert_eq!(
        remaining,
        vec!["delivered".to_string(), "pending".to_string()],
        "必须剩下「新增的那条 delivered」与「pending」—— pending 删了就是丢消息"
    );

    fixture
        .admin
        .execute("DELETE FROM platform_notify_delivery")
        .await;
}

/// **`claim_batch` 必须严格尊重 `batch`**（QA 复查 M3 的判据）。
///
/// # ⚠️ 为什么必须有这条
///
/// 原实现是 `UPDATE ... WHERE id IN (SELECT ... LIMIT ? FOR UPDATE SKIP LOCKED)`。
/// `IN (子查询)` 可能被实现成 semi join → 子查询在**外层每一行**上重跑
/// （计划里 `loops = N`）→ `LIMIT` 只约束每次子查询，约束不了 UPDATE 总行数。
///
/// **实测**（修之前）：突发 500 条队列上 `claim_batch(batch = 10)` 认领了 **16 条**
/// （超出 60%）；小表 6 条时 `batch = 2` 认领了 **3 条**。
///
/// # ⚠️ 为什么原先没有任何测试
///
/// 我 grep 过：`notify_worker_real_db.rs` 里**没有一条**断言 batch 上限的用例。
/// 这正是「批量上限」这个不变量长期无声失效的原因。
///
/// 修法：改成 **CTE 物化 + `= ANY(ARRAY(...))`**，子查询只执行一次。
#[tokio::test]
async fn claim_batch_strictly_respects_the_batch_limit() {
    let fixture = setup().await;
    fixture
        .admin
        .execute("DELETE FROM platform_notify_delivery")
        .await;

    // ⚠️ **突发 500 条**（一次性插入，统计信息尚未刷新）——
    // 这是生产上最容易触发的形态（队列从空突然涌入）。
    seed_pending(&fixture, 500).await;

    let claimed = fixture
        .service
        .store()
        .claim_batch(&fixture.ctx_a(), 10, 60)
        .await
        .expect("认领应当成功");

    assert_eq!(
        claimed.len(),
        10,
        "`claim_batch(batch = 10)` 在突发 500 条队列上必须**恰好**认领 10 条；\
         实际 {} 条。超过 10 说明 LIMIT 在重扫计划下失效（QA 实测修前是 16 条）",
        claimed.len()
    );

    fixture
        .admin
        .execute("DELETE FROM platform_notify_delivery")
        .await;
}

/// 小表场景同样必须严格（`batch = 2` / 6 条队列）。
///
/// ⚠️ 小表更容易走到重扫计划（规划器对小表的估算不同），
/// 所以这一条与上面那条**不是重复**：QA 实测小表场景 `batch=2` 认领了 3 条。
#[tokio::test]
async fn claim_batch_respects_a_small_batch_on_a_small_queue() {
    let fixture = setup().await;
    fixture
        .admin
        .execute("DELETE FROM platform_notify_delivery")
        .await;

    seed_pending(&fixture, 6).await;

    let claimed = fixture
        .service
        .store()
        .claim_batch(&fixture.ctx_a(), 2, 60)
        .await
        .expect("认领应当成功");

    assert_eq!(
        claimed.len(),
        2,
        "`claim_batch(batch = 2)` 在 6 条队列上必须**恰好**认领 2 条；\
         实际 {} 条（QA 实测修前是 3 条）",
        claimed.len()
    );

    fixture
        .admin
        .execute("DELETE FROM platform_notify_delivery")
        .await;
}

/// 造 N 条可立即投递的记录。
async fn seed_pending(fixture: &Fixture, count: usize) {
    for _ in 0..count {
        let new = NewDelivery {
            id: uuid::Uuid::new_v4().to_string(),
            template_code: "code".to_string(),
            channel: "in_app".to_string(),
            category: "transactional".to_string(),
            recipient: "u1".to_string(),
            locale: "zh-CN".to_string(),
            payload: serde_json::json!({}),
            rendered_title: "t".to_string(),
            rendered_body: "b".to_string(),
            next_attempt_at: time::OffsetDateTime::now_utc(),
        };
        fixture
            .service
            .store()
            .insert_delivery(&fixture.ctx_a(), &new)
            .await
            .expect("造投递记录");
    }
}

/// **AC-S-170 步骤 1~3**：4 个并发 `tick()` → 100 条**全部 delivered** 且
/// provider 调用次数**恰好 100**。
///
/// ⚠️ **两个数字都要断言**：只断言「全部 delivered」的话，
/// 「投两次但状态只记一次」这种重复投递会漏过去。
#[tokio::test]
async fn four_concurrent_ticks_deliver_each_message_exactly_once() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    seed_pending(&fixture, 100).await;

    let sender = CountingSender::new();
    let mut registry = platform_notify::channels::ChannelRegistry::new();
    registry.insert(
        "in_app",
        Arc::new(PlatformChannel::new(Arc::new(sender.clone()))),
    );

    // 4 个 worker 并发，每批取 30（100 条需要多轮才能全取完，但并发下会交错）。
    let (a, b, c, d) = tokio::join!(
        tick(&fixture.service, &ctx, &registry, 30),
        tick(&fixture.service, &ctx, &registry, 30),
        tick(&fixture.service, &ctx, &registry, 30),
        tick(&fixture.service, &ctx, &registry, 30),
    );
    let mut total_claimed = 0;
    for outcome in [&a, &b, &c, &d] {
        let stats = outcome.as_ref().expect("tick 不应失败");
        assert!(stats.is_balanced(), "每轮的计数必须自洽：{stats:?}");
        total_claimed += stats.claimed;
    }

    // 把剩下的取完（4 个 worker 各取 30，最多 120，够 100）。
    assert_eq!(
        total_claimed, 100,
        "4×30 的批次应能取走全部 100 条（实际 {total_claimed}）"
    );

    // 判据 ①：100 条**全部 delivered**（不遗漏）。
    let rows = fixture
        .admin
        .query_text(&format!(
            "SELECT count(*)::text FROM platform_notify_delivery \
             WHERE tenant_id = '{TENANT_A}' AND status = 'delivered'"
        ))
        .await;
    assert_eq!(
        rows.first().map(String::as_str),
        Some("100"),
        "100 条必须全部 delivered（不遗漏）"
    );

    // 判据 ②：provider 调用次数**恰好 100**（不重复）。
    assert_eq!(
        sender.call_count(),
        100,
        "provider 必须被调用恰好 100 次（`FOR UPDATE SKIP LOCKED` 保证不重复）"
    );
}

/// **AC-S-170 步骤 4**：`next_attempt_at` 未到期**不许取**。
#[tokio::test]
async fn a_delivery_scheduled_in_the_future_is_not_claimed() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    let new = NewDelivery {
        id: uuid::Uuid::new_v4().to_string(),
        template_code: "code".to_string(),
        channel: "in_app".to_string(),
        category: "transactional".to_string(),
        recipient: "u1".to_string(),
        locale: "zh-CN".to_string(),
        payload: serde_json::json!({}),
        rendered_title: "t".to_string(),
        rendered_body: "b".to_string(),
        next_attempt_at: time::OffsetDateTime::now_utc() + time::Duration::hours(1),
    };
    let row = fixture
        .service
        .store()
        .insert_delivery(&ctx, &new)
        .await
        .expect("造记录");

    let sender = CountingSender::new();
    let mut registry = platform_notify::channels::ChannelRegistry::new();
    registry.insert(
        "in_app",
        Arc::new(PlatformChannel::new(Arc::new(sender.clone()))),
    );

    let stats = tick(&fixture.service, &ctx, &registry, 10)
        .await
        .expect("tick");
    assert_eq!(stats.claimed, 0, "未到期的不许取");
    assert_eq!(sender.call_count(), 0, "provider 不得被调用");

    // 那条仍是 pending。
    assert_eq!(
        delivery_field(&fixture, &row.id, "status").await,
        Some(status::PENDING.to_string())
    );
}

/// **AC-S-170 步骤 5-3a/3b**：`delivering` 残留按阈值回收，**未超阈值不回收**。
///
/// ⚠️ 3b 是**阴性对照**，缺了它就有零区分力：
/// 一个「只要看到 `delivering` 就抢」的实现也能过 3a，
/// 而它会在**正常投递进行中**把行抢走 → 重复投递。
#[tokio::test]
async fn stale_delivering_is_reclaimed_but_fresh_delivering_is_not() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    // 3a：超阈值（默认 300 秒 → 造 400 秒前）。
    let stale = seed_delivery(&fixture, TENANT_A, "delivering", 1, "now()").await;
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET updated_at = now() - interval '400 seconds' \
             WHERE id = '{stale}'"
        ))
        .await;

    // 3b：未超阈值（5 秒前）。
    let fresh = seed_delivery(&fixture, TENANT_A, "delivering", 1, "now()").await;
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET updated_at = now() - interval '5 seconds' \
             WHERE id = '{fresh}'"
        ))
        .await;

    let sender = CountingSender::new();
    let mut registry = platform_notify::channels::ChannelRegistry::new();
    registry.insert(
        "in_app",
        Arc::new(PlatformChannel::new(Arc::new(sender.clone()))),
    );

    let stats = tick(&fixture.service, &ctx, &registry, 10)
        .await
        .expect("tick");

    // 3a：超阈值那条被回收并投递。
    assert_eq!(
        delivery_field(&fixture, &stale, "status").await,
        Some(status::DELIVERED.to_string()),
        "超阈值的 delivering 残留必须被回收并投递（否则永远卡住）"
    );
    // 3b：未超阈值那条**不得**被抢。
    assert_eq!(
        delivery_field(&fixture, &fresh, "status").await,
        Some(status::DELIVERING.to_string()),
        "未超阈值的 delivering **不得**被抢（否则正常投递中会被重复投递）"
    );
    assert_eq!(
        stats.claimed, 1,
        "这一轮只应认领超阈值的那 1 条（阴性对照）"
    );
    assert_eq!(sender.call_count(), 1, "provider 只应被调用 1 次");
}

/// **AC-S-170 步骤 6**：`delivered` 是终态，再 `tick()` 三次调用次数**不再增加**。
#[tokio::test]
async fn delivered_rows_are_never_claimed_again() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    seed_pending(&fixture, 1).await;

    let sender = CountingSender::new();
    let mut registry = platform_notify::channels::ChannelRegistry::new();
    registry.insert(
        "in_app",
        Arc::new(PlatformChannel::new(Arc::new(sender.clone()))),
    );

    let first = tick(&fixture.service, &ctx, &registry, 10)
        .await
        .expect("tick 1");
    assert_eq!(first.delivered, 1);
    assert_eq!(sender.call_count(), 1);

    for _ in 0..3 {
        let stats = tick(&fixture.service, &ctx, &registry, 10)
            .await
            .expect("再 tick");
        assert_eq!(stats.claimed, 0, "delivered 是终态，不参与出队");
    }
    assert_eq!(
        sender.call_count(),
        1,
        "provider 调用次数不得再增加（幂等）"
    );
}

/// **AC-S-171 步骤 1**：恒定失败 → 第 1~4 次 `pending` 且计数递增，
/// **第 5 次**进死信（`maxAttempts` 含首次）。
#[tokio::test]
async fn five_failures_lead_to_dead_letter() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    seed_pending(&fixture, 1).await;

    let sender = FailingSender::new("550 permanent failure");
    let mut registry = platform_notify::channels::ChannelRegistry::new();
    registry.insert(
        "in_app",
        Arc::new(PlatformChannel::new(Arc::new(sender.clone()))),
    );

    let id = fixture
        .admin
        .query_text("SELECT id::text FROM platform_notify_delivery LIMIT 1")
        .await
        .first()
        .expect("应有一条")
        .clone();

    // 前 4 次：都应是 pending，计数 1..4。
    for expected in 1..=4 {
        // 把 next_attempt_at 拨到过去（模拟退避时间已到）。
        fixture
            .admin
            .execute(&format!(
                "UPDATE platform_notify_delivery SET next_attempt_at = now() - interval '1 second' \
                 WHERE id = '{id}'"
            ))
            .await;
        let stats = tick(&fixture.service, &ctx, &registry, 10)
            .await
            .expect("tick");
        assert_eq!(stats.retried, 1, "第 {expected} 次失败应重试");
        assert_eq!(
            delivery_field(&fixture, &id, "attempt_count").await,
            Some(expected.to_string()),
            "第 {expected} 次后 attempt_count 应为 {expected}"
        );
        assert_eq!(
            delivery_field(&fixture, &id, "status").await,
            Some(status::PENDING.to_string()),
            "第 {expected} 次失败后仍是 pending"
        );
    }

    // 第 5 次：进死信。
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET next_attempt_at = now() - interval '1 second' \
             WHERE id = '{id}'"
        ))
        .await;
    let stats = tick(&fixture.service, &ctx, &registry, 10)
        .await
        .expect("tick 5");
    assert_eq!(stats.dead_lettered, 1, "第 5 次失败必须进死信");
    assert_eq!(
        delivery_field(&fixture, &id, "status").await,
        Some(status::DEAD_LETTER.to_string()),
        "第 5 次后必须是 dead_letter（不是第 6 次）"
    );

    // last_error 保留原因。
    let reason = delivery_field(&fixture, &id, "last_error")
        .await
        .unwrap_or_default();
    assert!(reason.contains("550"), "死信必须保留失败原因：{reason}");
}

/// **AC-S-171 步骤 3/4**：配置**真的被读**（改 `maxAttempts=2` → 第 2 次就死信）。
///
/// ⚠️ 这条的区分力全部来自「配置不是写死 5」：
/// 一个把 5 硬编码的实现会在第 2 次失败后仍停在 `pending` → 本条红。
#[tokio::test]
async fn a_configured_max_attempts_of_two_dead_letters_on_the_second_failure() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    // 改配置：最多尝试 2 次。
    fixture
        .config
        .set(platform_notify::config::CONFIG_KEY_MAX_ATTEMPTS, 2);
    seed_pending(&fixture, 1).await;

    let sender = FailingSender::new("550 permanent failure");
    let mut registry = platform_notify::channels::ChannelRegistry::new();
    registry.insert(
        "in_app",
        Arc::new(PlatformChannel::new(Arc::new(sender.clone()))),
    );

    let id = fixture
        .admin
        .query_text("SELECT id::text FROM platform_notify_delivery LIMIT 1")
        .await
        .first()
        .expect("应有一条")
        .clone();

    // 第 1 次：重试。
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET next_attempt_at = now() - interval '1 second' \
             WHERE id = '{id}'"
        ))
        .await;
    let stats1 = tick(&fixture.service, &ctx, &registry, 10)
        .await
        .expect("tick 1");
    assert_eq!(stats1.retried, 1, "第 1 次应重试（maxAttempts=2 还够）");

    // 第 2 次：死信（证明读的是配置，不是写死的 5）。
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET next_attempt_at = now() - interval '1 second' \
             WHERE id = '{id}'"
        ))
        .await;
    let stats2 = tick(&fixture.service, &ctx, &registry, 10)
        .await
        .expect("tick 2");
    assert_eq!(
        stats2.dead_lettered, 1,
        "maxAttempts=2 时第 2 次失败必须进死信（证明读的是配置）"
    );
}

/// **AC-S-171 步骤 2**：退避序列逐值 `[30, 60, 120, 240]`（真库验 `next_attempt_at`）。
#[tokio::test]
async fn the_backoff_sequence_is_observed_in_the_database() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    seed_pending(&fixture, 1).await;

    let sender = FailingSender::new("550");
    let mut registry = platform_notify::channels::ChannelRegistry::new();
    registry.insert(
        "in_app",
        Arc::new(PlatformChannel::new(Arc::new(sender.clone()))),
    );

    let id = fixture
        .admin
        .query_text("SELECT id::text FROM platform_notify_delivery LIMIT 1")
        .await
        .first()
        .expect("应有一条")
        .clone();

    for expected_seconds in [30_i64, 60, 120, 240] {
        fixture
            .admin
            .execute(&format!(
                "UPDATE platform_notify_delivery SET next_attempt_at = now() - interval '1 second' \
                 WHERE id = '{id}'"
            ))
            .await;
        tick(&fixture.service, &ctx, &registry, 10)
            .await
            .expect("tick");

        // 库里 next_attempt_at 应约等于 now + expected_seconds。
        let rows = fixture
            .admin
            .query_text(&format!(
                "SELECT (extract(epoch from (next_attempt_at - now())))::int::text \
                 FROM platform_notify_delivery WHERE id = '{id}'"
            ))
            .await;
        let actual: i64 = rows
            .first()
            .and_then(|text| text.parse().ok())
            .unwrap_or(-1);
        // 允许 ±2 秒的执行抖动。
        assert!(
            (expected_seconds - 2..=expected_seconds + 2).contains(&actual),
            "退避应约 {expected_seconds} 秒，实际 {actual} 秒"
        );
    }
}
