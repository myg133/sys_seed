//! 汇率快照的**真实库层**（REQ-021 / AC-S-097）。
//!
//! 验五件事：
//!
//! 1. **不同快照 → 不同结果**，且结果带 `id` / `as_of` / `source`（可追溯）；
//! 2. **不会隐式回退到别的快照**：`as_of` 没有精确匹配 → `None`
//!    （这是 R4「并发下不自觉」的判据 —— 取「最近一条」的实现会在这里通过）；
//! 3. **不存在的币种对 → `None`**（由 HTTP 层映射成 `FX_RATE_NOT_FOUND` / 404，
//!    裁定 #21）；
//! 4. **播种幂等**：同一 `(from, to, as_of)` 重复写不产生第二行；
//! 5. **汇率为 0 / 负数会被数据库拒绝**（`CHECK (rate_micros > 0)`）。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-db --all-targets
//! ```
//!
//! 未设置 `DATABASE_URL_TEST` 时**判为失败**，不是「跳过」（跳过也算通过 = 假绿）。
//! 数据库名不是 `base_test` 时直接失败 —— `base_dev` 是开发库，测试绝不能碰。
//!
//! # 夹具隔离
//!
//! 本表是**平台级**（无 `tenant_id`），所以每条用例用**自己的一对币种**
//! （CNY→USD / USD→JPY / …），并行跑也不会互相踩；表里的残留由用例自己清。

use std::path::{Path, PathBuf};

use platform_core::money::{CurrencyCode, Money};
use platform_db::fx::{FxRateStore, NewFxRate};
use platform_db::{DbOptions, TenantDb};
use sqlx::postgres::PgPoolOptions;

/// 测试库地址：只允许 `base_test`。
fn required_test_database_url() -> String {
    let url = std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "缺少 DATABASE_URL_TEST：真实库用例必须真的连库跑，不能静默跳过。\n  \
                 本地开发：先起库（bun run db:up），再跑 scripts/test-integration.sh。\n  \
                 库名必须恰好是 base_test（base_dev 是开发库，测试绝不能碰）。"
            )
        });
    let db = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_string();
    assert_eq!(
        db, "base_test",
        "集成测试只允许跑在 base_test 库上（当前：{db}）—— base_dev 是开发库，测试绝不能碰"
    );
    url
}

/// 把管理员连接串换成**应用角色**的连接串（超管会绕过 RLS → 隔离 fail open）。
fn app_role_url(admin_url: &str) -> String {
    let (scheme, rest) = admin_url.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()))
}

/// 准备真实库环境：迁移（建表）→ 应用角色（授权）→ 清掉本用例的夹具行。
async fn setup(from: &str, to: &str) -> (TenantDb, FxRateStore) {
    let url = required_test_database_url();
    let admin_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));

    let root = repo_root();
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&admin_pool).await.expect("平台迁移应能应用");

    // 迁移先建表、角色脚本后跑 → 走「存量表授权」路径（与部署同形）。
    let role_sql = read_file(&root.join("deploy/seed/02-app-role.sql"));
    sqlx::raw_sql(&role_sql)
        .execute(&admin_pool)
        .await
        .expect("应用角色准备失败");

    sqlx::query("DELETE FROM platform_fx_rate WHERE from_currency = $1 AND to_currency = $2")
        .bind(from)
        .bind(to)
        .execute(&admin_pool)
        .await
        .expect("清理汇率夹具应成功");

    let app_url = app_role_url(&url);
    let db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let store = FxRateStore::new(db.clone());
    (db, store)
}

fn currency(code: &str) -> CurrencyCode {
    CurrencyCode::parse(code).unwrap_or_else(|error| panic!("{code} 应是现行货币：{error}"))
}

fn snapshot(from: &str, to: &str, rate_micros: i64, as_of: &str) -> NewFxRate {
    NewFxRate {
        from: currency(from),
        to: currency(to),
        rate_micros,
        as_of: as_of.to_string(),
        source: "manual".to_string(),
    }
}

/// AC-S-097 步骤 1~4：两条不同日期的快照 → 两个不同结果 + 可追溯信息。
#[tokio::test]
async fn two_snapshots_give_two_results_and_carry_the_snapshot_identity() {
    let (_db, store) = setup("CNY", "USD").await;

    let january = store
        .insert_rate(&snapshot("CNY", "USD", 139_500, "2026-01-01T00:00:00Z"))
        .await
        .expect("第一份快照应写入");
    let june = store
        .insert_rate(&snapshot("CNY", "USD", 142_000, "2026-06-01T00:00:00Z"))
        .await
        .expect("第二份快照应写入");
    assert_ne!(january.id, june.id, "两份快照必须是两行");

    let amount = Money::from_minor(10_000, currency("CNY")); // 100.00 CNY

    let at_january = store
        .resolve_rate(currency("CNY"), currency("USD"), "2026-01-01T00:00:00Z")
        .await
        .expect("查第一份快照应成功")
        .expect("第一份快照应存在");
    let at_june = store
        .resolve_rate(currency("CNY"), currency("USD"), "2026-06-01T00:00:00Z")
        .await
        .expect("查第二份快照应成功")
        .expect("第二份快照应存在");

    let january_amount = at_january.convert(&amount).expect("一月换算应成功");
    let june_amount = at_june.convert(&amount).expect("六月换算应成功");

    // 结果不同（证明真的用了不同快照）—— 整数精确相等，不用浮点比较。
    assert_ne!(january_amount, june_amount);
    assert_eq!(january_amount.minor_units(), 1_395);
    assert_eq!(june_amount.minor_units(), 1_420);
    assert_eq!(january_amount.currency().as_str(), "USD");

    println!(
        "2026-01-01: rateMicros={} minorOut={} rateId={} source={}",
        at_january.rate_micros,
        january_amount.minor_units(),
        at_january.id,
        at_january.source
    );
    println!(
        "2026-06-01: rateMicros={} minorOut={} rateId={} source={}",
        at_june.rate_micros,
        june_amount.minor_units(),
        at_june.id,
        at_june.source
    );

    // 可追溯：结果能反查到「用的是哪条汇率」。
    assert_eq!(at_january.rate_micros, 139_500);
    assert_eq!(at_january.source, "manual");
    assert_eq!(at_june.rate_micros, 142_000);
    assert_eq!(at_january.as_of, january.as_of);
    assert_ne!(at_january.as_of, at_june.as_of);
    assert_eq!(at_january.id, january.id);
}

/// R4：**不隐式取最新**。`as_of` 没有精确匹配 → `None`，不退回「最近的一条」。
#[tokio::test]
async fn resolving_an_unsnapshotted_instant_does_not_fall_back_to_another_row() {
    let (_db, store) = setup("EUR", "CNY").await;
    store
        .insert_rate(&snapshot("EUR", "CNY", 780_000, "2026-01-01T00:00:00Z"))
        .await
        .expect("快照应写入");
    store
        .insert_rate(&snapshot("EUR", "CNY", 790_000, "2026-06-01T00:00:00Z"))
        .await
        .expect("快照应写入");

    // 中间时刻没有快照：取「最近一条」的实现会返回 2026-01-01 或 2026-06-01，
    // 这里必须是 None —— 这就是「显式 asOf」与「隐式最新」的分界。
    let middle = store
        .resolve_rate(currency("EUR"), currency("CNY"), "2026-03-01T00:00:00Z")
        .await
        .expect("查询本身应成功（只是没有行）");
    assert!(middle.is_none(), "没有精确匹配的快照时必须返回 None");

    // 早于第一份快照也一样。
    let before = store
        .resolve_rate(currency("EUR"), currency("CNY"), "2025-12-31T23:59:59Z")
        .await
        .expect("查询本身应成功");
    assert!(before.is_none());
}

/// AC-S-097 步骤 5：不存在的币种对 → `None`（HTTP 层映射成 404 `FX_RATE_NOT_FOUND`）。
#[tokio::test]
async fn an_unknown_currency_pair_resolves_to_nothing() {
    let (_db, store) = setup("USD", "JPY").await;
    store
        .insert_rate(&snapshot("USD", "JPY", 150_000_000, "2026-01-01T00:00:00Z"))
        .await
        .expect("快照应写入");

    let missing = store
        .resolve_rate(currency("JPY"), currency("USD"), "2026-01-01T00:00:00Z")
        .await
        .expect("查询本身应成功");
    assert!(missing.is_none(), "反向币种对必须查不到（不是自反的）");

    let other_instant = store
        .resolve_rate(currency("USD"), currency("JPY"), "2027-01-01T00:00:00Z")
        .await
        .expect("查询本身应成功");
    assert!(other_instant.is_none());
}

/// 播种幂等（D101）：同一 `(from, to, as_of)` 写两次只有一行，且值被更新。
#[tokio::test]
async fn inserting_the_same_snapshot_twice_updates_one_row() {
    let (db, store) = setup("CHF", "USD").await;

    let first = store
        .insert_rate(&snapshot("CHF", "USD", 1_120_000, "2026-02-01T00:00:00Z"))
        .await
        .expect("第一次写入");
    let second = store
        .insert_rate(&snapshot("CHF", "USD", 1_130_000, "2026-02-01T00:00:00Z"))
        .await
        .expect("第二次写入");
    assert_eq!(first.id, second.id, "同一快照标识必须是同一行");
    assert_eq!(second.rate_micros, 1_130_000, "冲突时应覆盖为新值");

    // 行数用**直连**数一下（store 没有 count 接口，也不该为测试加一个）。
    let ctx = platform_core::RequestContext::new().with_tenant("platform");
    let rows: Vec<(i64,)> = db
        .fetch_all(
            &ctx,
            &platform_db::TenantQuery::new(
                "SELECT count(*) FROM platform_fx_rate \
                 WHERE {tenant} IS NOT NULL AND from_currency = 'CHF' AND to_currency = 'USD'"
                    .to_string(),
            ),
        )
        .await
        .expect("计数应成功");
    assert_eq!(rows.first().map(|row| row.0), Some(1), "幂等写入只能有一行");
}

/// 汇率必须是正数：0 / 负数由数据库 `CHECK` 拒绝（不落库）。
#[tokio::test]
async fn non_positive_rates_are_rejected_by_the_database() {
    let (_db, store) = setup("GBP", "USD").await;
    for bad in [0_i64, -1] {
        let error = store
            .insert_rate(&snapshot("GBP", "USD", bad, "2026-01-01T00:00:00Z"))
            .await
            .expect_err("非正汇率必须被拒绝");
        assert_eq!(
            error.code(),
            "DATABASE_ERROR",
            "应当是数据库约束拒绝（而非静默写入）"
        );
    }
    let resolved = store
        .resolve_rate(currency("GBP"), currency("USD"), "2026-01-01T00:00:00Z")
        .await
        .expect("查询应成功");
    assert!(resolved.is_none(), "被拒绝的写入不得留下任何行");
}
