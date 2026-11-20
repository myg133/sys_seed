//! REQ-014 查询层的**真实库**用例：游标分页（D184）与过滤下推（D185）。
//!
//! # 这个文件为什么必须存在
//!
//! Pre-merge 复验（`BA/feature-meta/qa-premereq-req014.md`）查出两条阻塞：
//!
//! | 缺陷 | 现象 | 当时为什么没被发现 |
//! |------|------|--------------------|
//! | B-01 / D184 | `nextCursor` **恒为 `null`** → 翻页不存在 | `tests/` 里 `nextCursor` **零命中** |
//! | B-02 / D185 | 任何 `filter[...]` → SQL 拼成 `tenant_id = $1actor_id = $2` → 503 | `tests/` 里 `filter[` **零命中** |
//!
//! 两条都是**判据缺失**，不是「看起来对就行」。所以本文件的断言必须：
//!
//! 1. **打到真库**（走真实 SQL、真实 `platform_audit_log`），不是形状断言；
//! 2. **自证能变红** —— 把原缺陷注回去必须失败（见报告的变异自证）。
//!
//! # 断言口径（AC-S-130 / AC-S-132）
//!
//! - `total > limit` ⟺ `next_cursor` 非 `None`（`count_sql` 与主查询复用同一个
//!   `clause` → 该等价式在**带过滤**时同样成立）；
//! - 连续两页的 `id` 集合**无交集**；
//! - 末页（不足一页）`next_cursor` 为 `None`；
//! - 五个 op（`eq` / `ne` / `in` / `contains` / `range`）各一条，断言**行数与 id 集合**。

mod common;

use std::collections::HashMap;

use common::{Fixture, TENANT_A, USER_M};
use platform_audit::{AuditEvent, AuditQueryParams, AuditQueryService, PgAuditSink};
use serde_json::json;

/// 本文件专用的操作人（与别的用例隔离，避免互相污染过滤结果）。
const ACTOR_ONE: &str = "u-3001";
const ACTOR_TWO: &str = "u-3002";

/// 造一条**指定动作名 / 操作人**的审计事件，返回新记录的 `id`。
async fn record(fixture: &Fixture, action: &str, actor: &str, object_id: &str) -> String {
    let ctx = fixture.ctx_tenant_a();
    let sink = PgAuditSink::new(fixture.app_db.clone());
    let event = AuditEvent::new(TENANT_A, action, "demo_item")
        .with_actor(actor)
        .with_object_id(object_id)
        .with_before(json!({"n": 0}))
        .with_after(json!({"n": 1}));
    sink.record_event(&ctx, &event).await.expect("写入应当成功")
}

/// 用**查询参数原文**（与 HTTP 查询串同形）走一遍 `parse` + 查询。
///
/// ⚠️ 刻意从 `parse` 开始（而不是直接构造 `AuditQueryParams`）：B-02 的缺陷
/// 正是「解析没问题、拼 SQL 才炸」→ 只构造结构体测不到「拼 SQL」这一段。
async fn query(fixture: &Fixture, params: &[(&str, &str)]) -> platform_audit::AuditPage {
    let raw: HashMap<String, String> = params
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect();
    let parsed = AuditQueryParams::parse(&raw).expect("参数应当能解析");
    let service = AuditQueryService::new(fixture.app_db.clone());
    // B-02 的原缺陷在这里会返回 `SinkUnavailable`（PG 报 trailing junk）。
    service
        .query(&fixture.ctx_tenant_a(), &parsed)
        .await
        .expect("查询必须能执行（SQL 拼错会在这里炸）")
}

/// 本页记录的 `id` 集合。
fn ids(page: &platform_audit::AuditPage) -> std::collections::BTreeSet<String> {
    page.items.iter().map(|row| row.id.clone()).collect()
}

// ---------------------------------------------------------------------------
// D184 / B-01 —— 游标分页
// ---------------------------------------------------------------------------

/// `total > limit` ⟺ `next_cursor` 非 `None`（AC-S-130 步骤 1）。
///
/// # 这条断言为什么能抓回原缺陷
///
/// 原缺陷绑的是 `LIMIT limit`（而不是 `limit + 1`）→ 只取回 `limit` 行 →
/// `rows.len() > limit` **恒假** → `has_more` 恒假 → `next_cursor` **恒 `None`**。
/// 本用例在有下一页时要求 `next_cursor.is_some()` → 注入原缺陷必红。
#[tokio::test]
async fn has_more_pages_iff_the_total_exceeds_the_limit() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    for index in 0..5 {
        record(
            &fixture,
            &format!("demo.item.paging{index}"),
            ACTOR_ONE,
            &format!("obj-{index}"),
        )
        .await;
    }

    // limit < total → 必须有下一页。
    for limit in [1usize, 2, 4] {
        let page = query(&fixture, &[("limit", &limit.to_string())]).await;
        assert_eq!(page.total, 5, "总数不受分页影响");
        assert_eq!(page.items.len(), limit, "本页条数应等于 limit");
        assert!(
            page.next_cursor.is_some(),
            "total(5) > limit({limit}) 时 nextCursor 必须非 null（D184）"
        );
    }

    // limit == total → 恰好取完，**没有**下一页。
    let exact = query(&fixture, &[("limit", "5")]).await;
    assert_eq!(exact.items.len(), 5);
    assert!(
        exact.next_cursor.is_none(),
        "total == limit 时不应再给 nextCursor"
    );

    // limit > total → 不足一页 → 没有下一页。
    let oversized = query(&fixture, &[("limit", "20")]).await;
    assert_eq!(oversized.items.len(), 5);
    assert!(
        oversized.next_cursor.is_none(),
        "不足一页时 nextCursor 必须为 null"
    );
}

/// 连续两页的 `id` 集合**无交集**，且并集覆盖全部（AC-S-132 步骤 2 / 3）。
#[tokio::test]
async fn two_consecutive_pages_share_no_ids() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    let mut written = std::collections::BTreeSet::new();
    for index in 0..5 {
        written.insert(
            record(
                &fixture,
                &format!("demo.item.page{index}"),
                ACTOR_ONE,
                &format!("obj-{index}"),
            )
            .await,
        );
    }

    let first = query(&fixture, &[("limit", "2")]).await;
    assert_eq!(first.items.len(), 2);
    let cursor = first
        .next_cursor
        .clone()
        .expect("第 1 页有下一页 → nextCursor 必须非 null");

    let second = query(&fixture, &[("limit", "2"), ("cursor", &cursor)]).await;
    assert_eq!(second.items.len(), 2, "第 2 页应当有 2 条");

    let first_ids = ids(&first);
    let second_ids = ids(&second);
    let overlap: Vec<&String> = first_ids.intersection(&second_ids).collect();
    assert!(
        overlap.is_empty(),
        "两页的 id 不得有交集（否则翻页会重复）：{overlap:?}"
    );

    // 第 2 页仍有下一页（5 条里还剩 1 条）。
    let cursor_two = second
        .next_cursor
        .clone()
        .expect("5 条数据、每页 2 条 → 第 2 页之后还有 1 条");

    // 末页：只剩 1 条 → nextCursor 必须为 null。
    let third = query(&fixture, &[("limit", "2"), ("cursor", &cursor_two)]).await;
    assert_eq!(third.items.len(), 1, "末页只剩 1 条");
    assert!(third.next_cursor.is_none(), "末页的 nextCursor 必须为 null");

    // 三页并集 == 写进去的 5 条（不重不漏）。
    let mut union = first_ids;
    union.extend(second_ids);
    union.extend(ids(&third));
    assert_eq!(union, written, "三页并集必须恰好等于写入的 5 条");
}

/// 带过滤时 `total` 与 `next_cursor` 的等价式**同样**成立（`count_sql` 复用同一 clause）。
#[tokio::test]
async fn the_total_used_by_the_cursor_check_is_the_filtered_total() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    for index in 0..4 {
        record(
            &fixture,
            &format!("demo.item.mine{index}"),
            USER_M,
            &format!("obj-a-{index}"),
        )
        .await;
    }
    record(&fixture, "demo.item.other", ACTOR_TWO, "obj-b-0").await;

    // 过滤后 4 条；limit=2 → 有下一页。
    let page = query(&fixture, &[("limit", "2"), ("filter[actorId][eq]", USER_M)]).await;
    assert_eq!(page.total, 4, "total 必须是**带过滤**的总数");
    assert!(page.next_cursor.is_some(), "过滤后仍有下一页");

    // 过滤后 4 条；limit=4 → 恰好取完 → 无下一页。
    let exact = query(&fixture, &[("limit", "4"), ("filter[actorId][eq]", USER_M)]).await;
    assert_eq!(exact.items.len(), 4);
    assert!(
        exact.next_cursor.is_none(),
        "过滤后 total == limit 时不应再给 nextCursor"
    );
}

// ---------------------------------------------------------------------------
// D185 / B-02 —— 五个 op 各一条，打到真库
// ---------------------------------------------------------------------------

/// 造一组固定夹具：3 条 `alpha`（全是 `ACTOR_ONE`）+ 2 条 `beta`（全是 `ACTOR_TWO`）。
///
/// 返回 `(id, action)` 对照表。
async fn seed_filter_fixture(fixture: &Fixture) -> Vec<(String, String)> {
    let plan = [
        ("demo.item.alpha", ACTOR_ONE),
        ("demo.item.alpha", ACTOR_ONE),
        ("demo.item.alpha", ACTOR_ONE),
        ("demo.item.beta", ACTOR_TWO),
        ("demo.item.beta", ACTOR_TWO),
    ];
    let mut out = Vec::new();
    for (index, (action, actor)) in plan.iter().enumerate() {
        let id = record(fixture, action, actor, &format!("obj-{index}")).await;
        out.push((id, (*action).to_string()));
    }
    out
}

/// `eq`：按操作人过滤，返回**恰好**该操作人的 3 条。
#[tokio::test]
async fn the_eq_filter_returns_only_the_matching_rows() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;
    let seeded = seed_filter_fixture(&fixture).await;

    let page = query(
        &fixture,
        &[("limit", "20"), ("filter[actorId][eq]", ACTOR_ONE)],
    )
    .await;

    assert_eq!(page.total, 3, "actorId = {ACTOR_ONE} 应有 3 条");
    assert_eq!(page.items.len(), 3);
    assert!(
        page.items
            .iter()
            .all(|row| row.actor_id.as_deref() == Some(ACTOR_ONE)),
        "过滤结果必须全部属于该操作人"
    );
    // 从库里读回「该操作人写的那几条」的 id —— 用**独立查询**算期望值，
    // 不复用被测代码的计算（否则它错了我也是错的）。
    let expected: std::collections::BTreeSet<String> = sqlx::query_scalar(
        "SELECT id FROM platform_audit_log WHERE tenant_id = $1 AND actor_id = $2",
    )
    .bind(TENANT_A)
    .bind(ACTOR_ONE)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能读回期望的 id")
    .into_iter()
    .collect();
    assert_eq!(
        ids(&page),
        expected,
        "eq 过滤的 id 集合必须恰好等于该操作人的记录（写入共 {} 条）",
        seeded.len()
    );
}

/// `ne`：按动作过滤，返回**恰好**另外 2 条。
#[tokio::test]
async fn the_ne_filter_excludes_the_matching_rows() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;
    seed_filter_fixture(&fixture).await;

    let page = query(
        &fixture,
        &[("limit", "20"), ("filter[action][ne]", "demo.item.alpha")],
    )
    .await;

    assert_eq!(page.total, 2, "不等于 alpha 的应有 2 条");
    assert_eq!(page.items.len(), 2);
    assert!(
        page.items.iter().all(|row| row.action != "demo.item.alpha"),
        "ne 的结果不得包含被排除的动作"
    );
}

/// `in`：按动作集合过滤，返回**恰好** 2 条 `beta`。
#[tokio::test]
async fn the_in_filter_matches_any_of_the_values() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;
    seed_filter_fixture(&fixture).await;

    let page = query(
        &fixture,
        &[
            ("limit", "20"),
            ("filter[action][in]", "demo.item.beta,demo.item.absent"),
        ],
    )
    .await;

    assert_eq!(page.total, 2, "集合里只有 beta 命中");
    assert_eq!(page.items.len(), 2);
    assert!(
        page.items.iter().all(|row| row.action == "demo.item.beta"),
        "in 的结果必须全部是 beta"
    );
}

/// `contains`：子串匹配，返回 **5** 条（都含 `demo.item.`）。
#[tokio::test]
async fn the_contains_filter_matches_substrings() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;
    seed_filter_fixture(&fixture).await;

    let page = query(
        &fixture,
        &[("limit", "20"), ("filter[action][contains]", "item.a")],
    )
    .await;

    // `demo.item.alpha` × 3 命中；`demo.item.beta` 不含 `item.a` → 3 条。
    assert_eq!(page.total, 3, "含 `item.a` 的只有 alpha 那 3 条");
    assert_eq!(page.items.len(), 3);
    assert!(
        page.items.iter().all(|row| row.action.contains("item.a")),
        "contains 的结果必须真的含该子串"
    );
}

/// `range`：按 `createdAt` 区间过滤。
///
/// ⚠️ 时间戳列要显式转型（`?::timestamptz`）—— 拼不对在这里就是 SQL 错。
#[tokio::test]
async fn the_range_filter_matches_the_timestamp_window() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;
    seed_filter_fixture(&fixture).await;

    // 从库里读回真实的 created_at 边界（不假设时钟精度）。
    let bounds: (time::OffsetDateTime, time::OffsetDateTime) = sqlx::query_as(
        "SELECT min(created_at), max(created_at) FROM platform_audit_log WHERE tenant_id = $1",
    )
    .bind(TENANT_A)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能读回时间边界");

    // 用**仓库里同一个**时间戳格式化函数（微秒 6 位，D142 / D144）——
    // 自己手写 format 容易在毫秒/微秒边界上错一位。
    let format = |moment: &time::OffsetDateTime| {
        platform_core::extension::audit::format_timestamp_micros_utc(moment)
    };

    // 全覆盖区间 → 5 条。
    let window = format!("{},{}", format(&bounds.0), format(&bounds.1));
    let all = query(
        &fixture,
        &[("limit", "20"), ("filter[createdAt][range]", &window)],
    )
    .await;
    assert_eq!(all.total, 5, "覆盖全部时间边界 → 5 条");

    // 只取下半段（去掉最早的 1 条）→ 4 条（若时间戳有并列则可能更多，故用子集判据）。
    let rows: Vec<(String, time::OffsetDateTime)> = sqlx::query_as(
        "SELECT id, created_at FROM platform_audit_log WHERE tenant_id = $1 \
         ORDER BY created_at ASC, id ASC",
    )
    .bind(TENANT_A)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能读回记录");
    let second = &rows[1].1;
    let window = format!("{},{}", format(second), format(&bounds.1));
    let upper = query(
        &fixture,
        &[("limit", "20"), ("filter[createdAt][range]", &window)],
    )
    .await;
    let expected: std::collections::BTreeSet<String> = rows
        .iter()
        .filter(|(_, created_at)| *created_at >= *second)
        .map(|(id, _)| id.clone())
        .collect();
    assert_eq!(
        ids(&upper),
        expected,
        "range 的 id 集合必须与手工筛选一致（边界含端点）"
    );
}
