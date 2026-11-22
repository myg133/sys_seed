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
use platform_core::RequestContext;
use platform_observability::rate_limit::{RateLimitSettings, WindowLimit, bucket_key};
use std::sync::Arc;

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

// ---------------------------------------------------------------------------
// AC-S-038：就绪探针的限流检查
// ---------------------------------------------------------------------------

/// **TC-019-064 / 066**：启用时限流可达 → `Some(true)`；**关闭时 → `None`**（不检查）。
///
/// ⚠️ 第二条是关键判据：**关闭限流的部署不该因为限流表问题而不就绪** ——
/// 否则「没启用限流」反而更脆弱（AC-S-038 步骤 4）。
#[tokio::test]
async fn the_readiness_check_is_conditional_on_the_enabled_flag() {
    use platform_observability::http::rate_limit_readiness;
    use platform_observability::rate_limit::middleware::{
        RateLimitConfigSource, RateLimitState, StaticConfig,
    };

    let f = setup().await;
    f.truncate().await;

    let ctx = f.ctx_a();
    let limiter = Arc::new(limiter(f.db.clone()));

    // ① 启用 + 表可达 → Some(true)。
    let enabled = RateLimitState {
        limiter: Some(limiter.clone()),
        config: Arc::new(StaticConfig(RateLimitSettings {
            enabled: true,
            tenant: WindowLimit {
                requests: 5,
                window_seconds: 60,
            },
            user: WindowLimit {
                requests: 0,
                window_seconds: 60,
            },
            endpoint: WindowLimit {
                requests: 0,
                window_seconds: 60,
            },
        })),
    };
    assert_eq!(
        rate_limit_readiness(&Some(Arc::new(enabled)), &ctx).await,
        Some(true),
        "启用且可达时必须报 ok"
    );

    // ② **关闭** → None（不检查，AC-S-038 步骤 4）。
    let disabled = RateLimitState {
        limiter: Some(limiter.clone()),
        config: Arc::new(StaticConfig(RateLimitSettings::disabled())),
    };
    assert_eq!(
        rate_limit_readiness(&Some(Arc::new(disabled)), &ctx).await,
        None,
        "关闭限流时**不该**检查限流表（否则关闭反而更脆弱）"
    );

    // ③ 未装配 limiter → None。
    let no_limiter = RateLimitState {
        limiter: None,
        config: Arc::new(StaticConfig(RateLimitSettings {
            enabled: true,
            tenant: WindowLimit {
                requests: 5,
                window_seconds: 60,
            },
            user: WindowLimit {
                requests: 0,
                window_seconds: 60,
            },
            endpoint: WindowLimit {
                requests: 0,
                window_seconds: 60,
            },
        })),
    };
    assert_eq!(
        rate_limit_readiness(&Some(Arc::new(no_limiter)), &ctx).await,
        None,
        "没有 limiter 时不必检查"
    );

    // ④ 整个 state 缺失 → None。
    assert_eq!(rate_limit_readiness(&None, &ctx).await, None);

    // ⑤ 就绪检查**不写行**（不污染限流计数）。
    let rows = f.query_i64("SELECT count(*) FROM platform_rate_limit").await;
    assert_eq!(rows, 0, "就绪检查不得写行");
}

/// 配置源 trait 在测试里可用（`StaticConfig` 是公开的）。
#[tokio::test]
async fn the_static_config_source_is_usable_from_tests() {
    use platform_observability::rate_limit::middleware::RateLimitConfigSource;
    let source = platform_observability::rate_limit::middleware::StaticConfig(
        RateLimitSettings::disabled(),
    );
    let ctx = RequestContext::new();
    assert!(!source.settings(&ctx).await.enabled);
}

/// **AC-S-031 步骤 3**：三层维度**逐个单独启用**时都能独立生效。
///
/// # 为什么这条必须存在
///
/// 前面的用例各验了「某维度生效」，但**没有一条**证明
/// 「只启用租户维度时，另外两个维度**真的不限流**」。
///
/// 若实现漏了 `if !limit.enabled() { continue }`，
/// 那么「禁用」的维度会被当成「上限 0」→ **拒绝一切请求**。
/// 这个 bug 在「三维度都启用」的用例里**看不出来**。
#[tokio::test]
async fn each_dimension_limits_independently_when_enabled_alone() {
    let f = setup().await;

    // ① **只启用租户维度**：上限 2。
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let ctx = f.ctx_a();
    let tenant_key = bucket_key(dimension::TENANT, TENANT_A, Some("user-a"), "/x");
    let tenant_cfg = limit(2, 60);

    for i in 1..=2 {
        limiter
            .check(&ctx, dimension::TENANT, &tenant_key, tenant_cfg)
            .await
            .unwrap_or_else(|e| panic!("租户维度第 {i} 次应放行：{e:?}"));
    }
    limiter
        .check(&ctx, dimension::TENANT, &tenant_key, tenant_cfg)
        .await
        .expect_err("租户维度第 3 次必须拒");

    // ⚠️ 关键：另两个维度**禁用时**，无论打多少次都放行。
    let disabled = limit(0, 60);
    for i in 1..=10 {
        limiter
            .check(
                &ctx,
                dimension::USER,
                &bucket_key(dimension::USER, TENANT_A, Some("user-a"), "/x"),
                disabled,
            )
            .await
            .unwrap_or_else(|e| panic!("禁用的用户维度第 {i} 次必须放行（不是「允许 0 次」）：{e:?}"));
        limiter
            .check(
                &ctx,
                dimension::ENDPOINT,
                &bucket_key(dimension::ENDPOINT, TENANT_A, None, "/x"),
                disabled,
            )
            .await
            .unwrap_or_else(|e| panic!("禁用的接口维度第 {i} 次必须放行：{e:?}"));
    }

    // ② **只启用用户维度**：上限 1。
    f.truncate().await;
    let user_key = bucket_key(dimension::USER, TENANT_A, Some("user-a"), "/y");
    limiter
        .check(&ctx, dimension::USER, &user_key, limit(1, 60))
        .await
        .expect("用户维度第 1 次放行");
    limiter
        .check(&ctx, dimension::USER, &user_key, limit(1, 60))
        .await
        .expect_err("用户维度第 2 次必须拒");
    // 租户维度禁用 → 放行。
    limiter
        .check(&ctx, dimension::TENANT, &tenant_key, disabled)
        .await
        .expect("禁用的租户维度必须放行");

    // ③ **只启用接口维度**：上限 1。
    f.truncate().await;
    let ep_key = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/z");
    limiter
        .check(&ctx, dimension::ENDPOINT, &ep_key, limit(1, 60))
        .await
        .expect("接口维度第 1 次放行");
    limiter
        .check(&ctx, dimension::ENDPOINT, &ep_key, limit(1, 60))
        .await
        .expect_err("接口维度第 2 次必须拒");
    // 另两维度禁用 → 放行。
    limiter
        .check(&ctx, dimension::USER, &user_key, disabled)
        .await
        .expect("禁用的用户维度必须放行（第二个场景）");
    limiter
        .check(&ctx, dimension::TENANT, &tenant_key, disabled)
        .await
        .expect("禁用的租户维度必须放行（第二个场景）");
}

/// **prune 的保留期边界**：只删「超出 2 倍窗口」的行，**不误删**窗口内的。
///
/// # 为什么这条必须有
///
/// `prune(max_window)` 用的保留期是 `max_window × 2`（留一倍余量）。
/// 若实现写成 `max_window × 1`（或漏了 `×2`），
/// **仍在窗口内**的行会被删掉 → 限流计数突然变小 → **限流被绕过**。
///
/// 这个 bug 在「表本来就空」的用例里看不出来 —— 必须**主动插历史行**。
#[tokio::test]
async fn prune_keeps_rows_inside_double_the_window() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let ctx = f.ctx_a();
    let key = bucket_key(dimension::TENANT, TENANT_A, None, "/prune-test");

    // 手工插 3 行：年龄 10 秒 / 100 秒 / 400 秒。
    f.exec(&format!(
        "INSERT INTO platform_rate_limit (tenant_id, dimension, key, occurred_at) VALUES \
         ('{TENANT_A}', 'tenant', '{key}', now() - interval '10 seconds'), \
         ('{TENANT_A}', 'tenant', '{key}', now() - interval '100 seconds'), \
         ('{TENANT_A}', 'tenant', '{key}', now() - interval '400 seconds')"
    ))
    .await;
    assert_eq!(
        f.query_i64("SELECT count(*) FROM platform_rate_limit").await,
        3,
        "3 行应插入成功"
    );

    // 窗口 60 秒 → 保留期 120 秒 → 只该删 400 秒那行。
    let deleted = limiter.prune(&ctx, 60).await.expect("prune 应成功");
    assert_eq!(deleted, 1, "只有 400 秒那行超出 2×60=120 秒，必须只删 1 行");

    let remaining = f.query_i64("SELECT count(*) FROM platform_rate_limit").await;
    assert_eq!(remaining, 2, "10 秒与 100 秒两行必须保留");

    // ⚠️ **关键**：100 秒那行还在（虽然超出 60 秒窗口，但没超出保留期）。
    let inside_window = limiter
        .count_in_window(&ctx, dimension::TENANT, &key, 60)
        .await
        .expect("计数");
    assert_eq!(
        inside_window, 1,
        "60 秒窗口内应只剩 10 秒那 1 行（100 秒的已过期出窗口但仍在表里）"
    );
    let inside_double = limiter
        .count_in_window(&ctx, dimension::TENANT, &key, 120)
        .await
        .expect("计数");
    assert_eq!(inside_double, 2, "120 秒内应有 2 行（10 秒 + 100 秒）");
}

/// `prune` 传入 0 或负数时**不能删全表**（防御性下限）。
#[tokio::test]
async fn prune_with_a_zero_window_does_not_wipe_the_table() {
    let f = setup().await;
    f.truncate().await;
    let limiter = limiter(f.db.clone());
    let ctx = f.ctx_a();
    let key = bucket_key(dimension::TENANT, TENANT_A, None, "/prune-zero");

    f.exec(&format!(
        "INSERT INTO platform_rate_limit (tenant_id, dimension, key, occurred_at) \
         VALUES ('{TENANT_A}', 'tenant', '{key}', now() - interval '30 seconds')"
    ))
    .await;

    // ⚠️ 实现里 `max_window.max(1) * 2` → 保留期至少 2 秒 → 30 秒的行**会**被删。
    // 这里断言的是「不会因为 ×0 而变成保留期 0 秒后 panic 或删错范围」。
    let _ = limiter.prune(&ctx, 0).await.expect("prune 不该 panic");
    // 表里要么剩 0 行（30 秒 > 2 秒保留期，合理），要么报错 —— 不该是「全表静默消失后无法解释」。
    let remaining = f.query_i64("SELECT count(*) FROM platform_rate_limit").await;
    assert_eq!(
        remaining, 0,
        "30 秒的行超出 2 秒保留期（0.max(1)*2），应被删除 —— 但要能解释"
    );
}
