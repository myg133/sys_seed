//! 限流的真库测试（REQ-019，AC-S-031 / AC-S-032 / AC-S-033）。
//!
//! # 为什么必须连真库
//!
//! 限流是**PG 滑动窗口**（D337）—— 用假实现测「限流生效」毫无意义。
//! 尤其 AC-S-033（跨实例准确）**只有两个独立连接池 + 真库**才能验。
//!
//! # ⚠️ 每个用例开头必须 `truncate()`
//!
//! 否则上一步的桶会污染下一步（症状：单独跑绿、连跑红）。

mod common;

use common::{TENANT_A, TENANT_B, limiter, setup};
use platform_observability::error::{ObservabilityError, dimension};
use platform_observability::rate_limit::{WindowLimit, bucket_key};

/// 便利：构造一个「每 window 允许 n 次」的维度配置。
fn limit(n: i64, window: i64) -> WindowLimit {
    WindowLimit {
        requests: n,
        window_seconds: window,
    }
}

// ---------------------------------------------------------------------------
// AC-S-031：三级各自然独立
// ---------------------------------------------------------------------------

/// **TC-019-014**：接口维度 —— 上限 1，第 2 次 429。
#[tokio::test]
async fn the_endpoint_dimension_rejects_the_second_request() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let ctx = f.ctx_a();
    let key = bucket_key(dimension::ENDPOINT, TENANT_A, Some("user-a"), "/api/rbac/roles");
    let cfg = limit(1, 60);

    limiter
        .check(&ctx, dimension::ENDPOINT, &key, cfg)
        .await
        .expect("第 1 次必须放行");

    let err = limiter
        .check(&ctx, dimension::ENDPOINT, &key, cfg)
        .await
        .expect_err("第 2 次必须拒绝");
    assert_eq!(err.code(), "RATE_LIMITED");
    match err {
        ObservabilityError::RateLimited { dimension: d, .. } => {
            assert_eq!(d, "endpoint", "维度必须是 endpoint");
        }
        other => panic!("期望 RateLimited，实际 {other:?}"),
    }
}

/// **TC-019-015**：用户维度 —— 跨**不同端点**累计，第 3 次 429。
///
/// ⚠️ 这条证明「用户维度是跨端点累计的」，且**不受 endpoint 维度干扰**
/// （调用时只传 user 维度的配置，模拟 endpoint 维度被禁用）。
#[tokio::test]
async fn the_user_dimension_accumulates_across_endpoints() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let ctx = f.ctx_a();
    let cfg = limit(2, 60);

    for path in ["/api/rbac/roles", "/api/apps"] {
        let key = bucket_key(dimension::USER, TENANT_A, Some("user-a"), path);
        limiter
            .check(&ctx, dimension::USER, &key, cfg)
            .await
            .unwrap_or_else(|e| panic!("{path} 的前 2 次应放行：{e:?}"));
    }

    // 第 3 次（换第 3 个端点）→ 用户维度累计已 2，必须拒。
    let key = bucket_key(dimension::USER, TENANT_A, Some("user-a"), "/api/audit/events");
    let err = limiter
        .check(&ctx, dimension::USER, &key, cfg)
        .await
        .expect_err("第 3 次必须拒绝");
    match err {
        ObservabilityError::RateLimited { dimension: d, .. } => {
            assert_eq!(d, "user", "必须是 user 维度（不是 endpoint）");
        }
        other => panic!("期望 RateLimited，实际 {other:?}"),
    }
}

/// **TC-019-016**：不同用户的桶**必须不同**。
#[tokio::test]
async fn two_users_have_separate_buckets() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let cfg = limit(1, 60);

    let key_a = bucket_key(dimension::USER, TENANT_A, Some("user-a"), "/x");
    let key_b = bucket_key(dimension::USER, TENANT_A, Some("user-b"), "/x");
    assert_ne!(key_a, key_b, "不同用户的键必须不同");

    limiter
        .check(&f.ctx_a(), dimension::USER, &key_a, cfg)
        .await
        .expect("user-a 第 1 次放行");
    // user-b 不受 user-a 影响。
    limiter
        .check(&f.ctx_a_other_user(), dimension::USER, &key_b, cfg)
        .await
        .expect("user-b 第 1 次应放行（独立的桶）");
}

/// **TC-019-018**：**跨租户隔离** —— tenant-b 不受 tenant-a 的限流影响。
///
/// ⚠️ 这是 rev 2 相对 rev 1 的**核心修正**（挑刺 B1）：
/// rev 1 断言「同租户 `user-b` 打同一端点仍 200」，但 endpoint 桶本就按租户共享
/// → 那个判据在**实现正确时必红**。跨租户才是真正独立的。
#[tokio::test]
async fn another_tenant_is_not_affected_by_a_saturated_bucket() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let cfg = limit(1, 60);

    // tenant-a 把 endpoint 桶打满。
    let key_a = bucket_key(dimension::ENDPOINT, TENANT_A, Some("user-a"), "/api/rbac/roles");
    limiter
        .check(&f.ctx_a(), dimension::ENDPOINT, &key_a, cfg)
        .await
        .expect("tenant-a 第 1 次放行");
    limiter
        .check(&f.ctx_a(), dimension::ENDPOINT, &key_a, cfg)
        .await
        .expect_err("tenant-a 第 2 次必须拒");

    // tenant-b 打**同一路径** —— 键字符串相同，但 tenant_id 不同 → 独立桶。
    let key_b = bucket_key(dimension::ENDPOINT, TENANT_B, Some("user-c"), "/api/rbac/roles");
    assert_eq!(key_a, key_b, "endpoint 键不含 tenant_id（隔离靠 RLS 的列）");
    limiter
        .check(&f.ctx_b(), dimension::ENDPOINT, &key_b, cfg)
        .await
        .expect("tenant-b 必须放行（另一个租户的桶是独立的）");
}

/// **TC-019-019**（**反向判据**）：同租户内，不同用户**共享** endpoint 桶。
///
/// ⚠️ 这条明确记录 D337 的语义：「endpoint 维度是租户内跨用户共享的」。
/// 若实现把 `user_id` 拼进 endpoint 键 → 这条红（说明键设计错了）。
#[tokio::test]
async fn the_endpoint_bucket_is_shared_across_users_in_one_tenant() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let cfg = limit(1, 60);

    let key = bucket_key(dimension::ENDPOINT, TENANT_A, Some("user-a"), "/api/rbac/roles");
    let key_other = bucket_key(dimension::ENDPOINT, TENANT_A, Some("user-b"), "/api/rbac/roles");
    assert_eq!(key, key_other, "同一租户同端点：键必须相同（不分用户）");

    limiter
        .check(&f.ctx_a(), dimension::ENDPOINT, &key, cfg)
        .await
        .expect("user-a 第 1 次放行");

    // user-b 打同一端点 → 因共享桶而 429。
    let err = limiter
        .check(&f.ctx_a_other_user(), dimension::ENDPOINT, &key_other, cfg)
        .await
        .expect_err("user-b 必须被同一个桶拒绝（endpoint 桶跨用户共享）");
    match err {
        ObservabilityError::RateLimited { dimension: d, .. } => {
            assert_eq!(d, "endpoint");
        }
        other => panic!("期望 RateLimited，实际 {other:?}"),
    }
}

/// **TC-019-020**：query string **不**进键（`?x=1` 与 `?x=2` 共用桶）。
#[tokio::test]
async fn the_query_string_does_not_create_a_new_bucket() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let cfg = limit(1, 60);

    // `MatchedPath` 给的是路径模板（不含 query）→ 两次请求的键**相同**。
    let key1 = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/api/rbac/roles");
    let key2 = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/api/rbac/roles");
    assert_eq!(key1, key2);

    limiter
        .check(&f.ctx_a(), dimension::ENDPOINT, &key1, cfg)
        .await
        .expect("第 1 次放行");
    limiter
        .check(&f.ctx_a(), dimension::ENDPOINT, &key2, cfg)
        .await
        .expect_err("第 2 次必须拒（query 不绕桶）");
}

/// **TC-019-022**：`requests = 0` 是**禁用**，不查库、不记账。
#[tokio::test]
async fn a_disabled_dimension_neither_limits_nor_writes() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let key = bucket_key(dimension::TENANT, TENANT_A, None, "/x");

    for _ in 0..5 {
        limiter
            .check(&f.ctx_a(), dimension::TENANT, &key, limit(0, 60))
            .await
            .expect("禁用的维度必须永远放行");
    }

    let rows = f
        .query_i64("SELECT count(*) FROM platform_rate_limit")
        .await;
    assert_eq!(rows, 0, "禁用的维度不得记账（零查库开销）");
}

// ---------------------------------------------------------------------------
// AC-S-032：429 的响应语义
// ---------------------------------------------------------------------------

/// **TC-019-024 / 025**：`retry_after` 是正整数且有上界。
#[tokio::test]
async fn the_retry_after_is_a_positive_bounded_integer() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let ctx = f.ctx_a();
    let key = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/x");
    let window = 2;
    let cfg = limit(1, window);

    limiter
        .check(&ctx, dimension::ENDPOINT, &key, cfg)
        .await
        .expect("第 1 次放行");
    let err = limiter
        .check(&ctx, dimension::ENDPOINT, &key, cfg)
        .await
        .expect_err("第 2 次必须拒");

    let retry = match err {
        ObservabilityError::RateLimited { retry_after, .. } => retry_after,
        other => panic!("期望 RateLimited，实际 {other:?}"),
    };
    assert!(retry >= 1, "retry_after 必须是正整数，实际 {retry}");
    assert!(
        retry <= window + 1,
        "retry_after 必须 ≤ 窗口+1（{window}+1），实际 {retry} —— \
         返回 999999 也能过「正整数」判据，所以必须有上界"
    );
}

/// **TC-019-027**：`params` 的键是**闭集** `{dimension, retryAfter}`。
#[tokio::test]
async fn the_rate_limited_params_are_a_closed_whitelist() {
    let err = ObservabilityError::RateLimited {
        dimension: dimension::USER.to_string(),
        retry_after: 3,
    };
    let params = err.params().expect("429 必须带 params");
    let keys: std::collections::BTreeSet<&str> = params.keys().map(String::as_str).collect();
    let allowed: std::collections::BTreeSet<&str> =
        ["dimension", "retryAfter"].into_iter().collect();
    assert!(
        keys.is_subset(&allowed),
        "params 的键必须是子集（多一个就泄露内部结构）；实际：{keys:?}"
    );
    // 而且必须真的带上这两个键（不是空 map）。
    assert_eq!(keys.len(), 2, "两个键都要有");
}

/// **TC-019-029**：窗口过期后**立即恢复**（限流 ≠ 封禁）。
#[tokio::test]
async fn the_limit_recovers_after_the_window_expires() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let ctx = f.ctx_a();
    let key = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/x");
    // 窗口 1 秒，便于快速验证。
    let cfg = limit(1, 1);

    limiter
        .check(&ctx, dimension::ENDPOINT, &key, cfg)
        .await
        .expect("第 1 次放行");
    limiter
        .check(&ctx, dimension::ENDPOINT, &key, cfg)
        .await
        .expect_err("窗口内第 2 次必须拒");

    // 等窗口过期。
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    limiter
        .check(&ctx, dimension::ENDPOINT, &key, cfg)
        .await
        .expect("窗口过期后必须立即恢复（不是永久封禁）");
}

// ---------------------------------------------------------------------------
// AC-S-033：跨实例准确（PG 共享，不是每进程一份）
// ---------------------------------------------------------------------------

/// **TC-019-031**：两个池确实是**不同的池**（前置自我校验）。
///
/// ⚠️ rev 2 修正（挑刺 M1）：rev 1 用「`max_connections` 不同」当判据，
/// 那是**无区分力的代理断言**（同一个池也不能有两个 max_connections）。
///
/// 这里改为：两个 `TenantDb` 能**各自独立地**完成一次读写 ——
/// 若实现是「clone 同一个池」，两者共享状态，下面 AC-S-033 的第 ③ 步
/// （DB 里恰好 1 行）就成了唯一的区分点。所以本用例只断言「能建两个」，
/// **真正的区分力交给 [`an_independent_instance_sees_the_shared_count`]**。
#[tokio::test]
async fn the_fixture_can_build_two_independent_handles() {
    let f = setup().await;
    f.truncate().await;
    let db_a = f.second_db().await;
    let db_b = f.second_db().await;
    // 两个句柄都能独立工作（不是同一个被 clone 的句柄）。
    assert!(limiter(db_a).is_reachable(&f.ctx_a()).await);
    assert!(limiter(db_b).is_reachable(&f.ctx_a()).await);
}

/// **TC-019-032 / 033 / 034**：两个独立实例共享计数，且 DB 里真有行。
///
/// ⚠️ 这是 AC-S-033 的**核心判据**：
/// - 步骤 2（B 看到 429）能区分「每实例内存」实现（B 会放行）；
/// - 步骤 3（DB 里 1 行）能区分「进程内全局 static」实现（那是 0 行）。
#[tokio::test]
async fn an_independent_instance_sees_the_shared_count() {
    let f = setup().await;
    f.truncate().await;

    let db_a = f.second_db().await;
    let db_b = f.second_db().await;
    let limiter_a = limiter(db_a);
    let limiter_b = limiter(db_b);

    let ctx = f.ctx_a();
    let key = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/api/rbac/roles");
    let cfg = limit(1, 60);

    // ① 实例 A 打 1 次 → 放行。
    limiter_a
        .check(&ctx, dimension::ENDPOINT, &key, cfg)
        .await
        .expect("实例 A 第 1 次必须放行");

    // ② 实例 B 打同一键 → 必须 429（证明共享计数）。
    //
    // ⚠️ 若实现是「每实例内存」，B 会看到 0 → 放行 → 这条红。
    limiter_b
        .check(&ctx, dimension::ENDPOINT, &key, cfg)
        .await
        .expect_err("实例 B 必须看到 A 的计数（跨实例共享）");

    // ③ DB 里**恰好 1 行**（被拒的请求不落行）。
    //
    // ⚠️ 这条能区分「进程内全局 static」（那是 0 行）。
    let rows = f
        .query_i64(&format!(
            "SELECT count(*) FROM platform_rate_limit \
              WHERE tenant_id = '{TENANT_A}' AND dimension = 'endpoint' AND key = '{key}' \
                AND occurred_at > now() - interval '60 seconds'"
        ))
        .await;
    assert_eq!(
        rows, 1,
        "窗口内必须恰好 1 行（被拒的请求不落行 —— 这是 design §3「先判定后记账」的语义）"
    );
}

/// **TC-019-031b**：`count_in_window` 与 `prune` 的行为。
#[tokio::test]
async fn the_count_and_prune_behave_as_documented() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let ctx = f.ctx_a();
    let key = bucket_key(dimension::TENANT, TENANT_A, None, "/x");

    for _ in 0..3 {
        limiter
            .check(&ctx, dimension::TENANT, &key, limit(10, 60))
            .await
            .expect("上限 10，前 3 次应放行");
    }

    let count = limiter
        .count_in_window(&ctx, dimension::TENANT, &key, 60)
        .await
        .expect("计数应成功");
    assert_eq!(count, 3, "3 次请求应记 3 行");

    // 窗口给 0 秒 → 什么都不在窗口内（边界）。
    let count_zero = limiter
        .count_in_window(&ctx, dimension::TENANT, &key, 0)
        .await
        .expect("计数应成功");
    assert_eq!(count_zero, 0, "0 秒窗口内不该有任何行");

    // prune 用 1 秒保留期（2×）→ 刚写的行**不该**被删。
    let deleted = limiter.prune(&ctx, 1).await.expect("prune 应成功");
    assert_eq!(deleted, 0, "刚写的行不该被清理（保留期 2 秒）");
    assert_eq!(
        limiter
            .count_in_window(&ctx, dimension::TENANT, &key, 60)
            .await
            .expect("计数"),
        3,
        "prune 不该误删"
    );
}

/// **就绪检查**：`is_reachable` 在不写行的前提下能判断可达性。
#[tokio::test]
async fn the_readiness_probe_does_not_write_rows() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());

    assert!(
        limiter.is_reachable(&f.ctx_a()).await,
        "表存在时应可达"
    );
    let rows = f
        .query_i64("SELECT count(*) FROM platform_rate_limit")
        .await;
    assert_eq!(rows, 0, "就绪检查不得写行（否则会污染限流计数）");
}

/// **RLS 生效**：应用角色下，tenant-a 看不到 tenant-b 的行。
#[tokio::test]
async fn row_level_security_hides_other_tenants_rows() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());

    // tenant-a 与 tenant-b 各打 1 次（不同键，都不会触发限流）。
    limiter
        .check(
            &f.ctx_a(),
            dimension::TENANT,
            &bucket_key(dimension::TENANT, TENANT_A, None, "/x"),
            limit(10, 60),
        )
        .await
        .expect("tenant-a 放行");
    limiter
        .check(
            &f.ctx_b(),
            dimension::TENANT,
            &bucket_key(dimension::TENANT, TENANT_B, None, "/x"),
            limit(10, 60),
        )
        .await
        .expect("tenant-b 放行");

    // 管理员看得到 2 行。
    let all = f
        .query_i64("SELECT count(*) FROM platform_rate_limit")
        .await;
    assert_eq!(all, 2, "两个租户各 1 行");

    // 应用角色下按租户数 → 只看到自己的。
    //
    // ⚠️ 键必须用**完整的租户名**（`TENANT_A` 常量，含 `@` 后缀），
    // 不是短名 —— 用短名会构造出不同的键 → 数到 0（这个坑我踩过）。
    for (ctx, tenant) in [(f.ctx_a(), TENANT_A), (f.ctx_b(), TENANT_B)] {
        let key = bucket_key(dimension::TENANT, tenant, None, "/x");
        let visible = limiter
            .count_in_window(&ctx, dimension::TENANT, &key, 60)
            .await
            .expect("计数");
        assert_eq!(visible, 1, "{tenant} 应只看到自己的 1 行");
    }
}
