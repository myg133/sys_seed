//! 租户上下文的 **`SET LOCAL` 语义**（TD-082）。
//!
//! # ⚠️ 这个文件防的是什么
//!
//! `crates/platform-db/src/tenant.rs` 用
//! `SELECT set_config('app.tenant_id', $1, true)` 设置租户上下文 ——
//! 第三个参数 `true` 表示 **事务局部**（等价 `SET LOCAL`），事务结束自动清除。
//!
//! 若有人把它改成 `false`（会话级），**租户值会残留在连接上**：
//! 连接归还池后，**下一个借到它的请求**会带着**上一个租户**的上下文 →
//! RLS 的 `current_setting('app.tenant_id')` 读到错的值 →
//! **RLS 这道最后防线也随之失效**。
//!
//! # 为什么这条判据必须存在
//!
//! 独立 QA 做了变异实证：把 `true` 改成 `false` 后，
//! `platform-db` 的 **38 个测试全绿** —— 这个平台最关键的隔离机制**零覆盖**。
//!
//! # 判据设计
//!
//! 用 `max_connections = 1` 的池**强制复用同一条物理连接**：
//!
//! 1. 在租户 A 的事务里读一次 `app.tenant_id` → 必须是 A（证明设置生效）；
//! 2. 提交后，在**同一条连接**上再读 → 必须**已清除**；
//! 3. `true` = 已清除（绿）；`false` = 残留 `'tenant-a@td082'`（**红**）。
//!
//! ⚠️ `max_connections = 1` 是关键：池里只有一条连接，
//! 才能证明「残留」而不是「恰好借到了另一条干净连接」。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-db --test tenant_context --all-targets
//! ```

use platform_core::RequestContext;
use platform_db::TenantQuery;
use platform_db::{DbOptions, TenantDb};

/// 读测试库 URL；没设就**失败**（不是跳过 —— 跳过也算通过就是假绿）。
fn database_url() -> String {
    let url = std::env::var("DATABASE_URL_TEST").unwrap_or_else(|_| {
        panic!(
            "缺少 DATABASE_URL_TEST：本用例必须连真库跑。\n\
             本地开发：先起库（bun run db:up），再跑 scripts/test-integration.sh。\n\
             库名必须恰好是 base_test（base_dev 是开发库，测试绝不能碰）。"
        )
    });
    assert!(
        url.contains("/base_test"),
        "DATABASE_URL_TEST 必须指向 base_test（当前：{url}）"
    );
    url
}

/// 建一个**只有 1 条连接**的池（强制复用同一条物理连接）。
async fn single_connection_db(url: &str) -> TenantDb {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));
    TenantDb::from_pool(
        pool,
        DbOptions {
            max_connections: 1,
            schema: "public".to_string(),
        },
    )
}

/// 事务内读 `app.tenant_id`。
///
/// ⚠️ 用 `TenantQuery` 而不是裸 SQL：`render` 强制恰好一个 `{tenant}` 占位符，
/// 这里用 `{tenant} IS NOT NULL` 满足它（恒真，只为了能走租户作用域入口）。
struct SettingRow {
    value: Option<String>,
}

platform_db::tenant_row!(SettingRow { value: "v" });

/// **租户上下文在事务结束后必须被清除**（`SET LOCAL` 语义，TD-082 的核心判据）。
///
/// 变异验证：把 `set_config('app.tenant_id', $1, true)` 的 `true` 改成 `false`
/// → 本用例立刻红（残留 `'tenant-a@td082'`）。
#[tokio::test]
async fn the_tenant_context_is_cleared_after_the_transaction() {
    let url = database_url();
    let db = single_connection_db(&url).await;

    let tenant = "tenant-a@td082";
    let ctx = RequestContext::new().with_tenant(tenant);

    // ① 事务内读 —— 必须是当前租户（证明 `set_config` 确实执行了）。
    {
        let mut tx = db.begin(&ctx).await.expect("应当能开事务");
        let query = TenantQuery::new(
            "SELECT NULLIF(current_setting('app.tenant_id', true), '') AS v \
             WHERE {tenant} IS NOT NULL",
        );
        let row: SettingRow = tx.fetch_one(&query).await.expect("事务内读取租户上下文");
        assert_eq!(
            row.value.as_deref(),
            Some(tenant),
            "事务内 `app.tenant_id` 必须是当前租户"
        );
        tx.commit().await.expect("提交应当成功");
    }

    // ② 提交后，在**同一条连接**上读 —— 必须已被清除。
    //
    // ⚠️ 池只有 1 条连接 → 这里借到的**就是刚才那条**。
    // `set_config(..., true)` → 事务结束自动清除 → 这里是 NULL；
    // `set_config(..., false)` → 残留 → 这条红。
    let after: Option<String> = {
        let mut conn = db.test_acquire().await.expect("应当能借到那条唯一的连接");
        sqlx::query_scalar("SELECT NULLIF(current_setting('app.tenant_id', true), '')")
            .fetch_one(&mut *conn)
            .await
            .expect("读取租户上下文")
    };

    assert!(
        after.is_none(),
        "事务结束后 `app.tenant_id` **必须被清除**（`SET LOCAL` 语义），\
         但残留了 `{after:?}` —— 说明 `set_config` 的第三个参数不是 `true`。\n\
         \n\
         后果：连接归还池后，**下一个借到它的请求会带着上一个租户的上下文**\n\
         → RLS 的 `current_setting('app.tenant_id')` 读到错的值 → **最后一道防线一起失效**。\n\
         \n\
         修法：`crates/platform-db/src/tenant.rs` 的 \
         `set_config('app.tenant_id', $1, true)`。"
    );
}

/// **回滚**路径同样必须清除（`ROLLBACK` 与 `COMMIT` 对 `SET LOCAL` 一视同仁）。
///
/// # ⚠️ 能力边界（实测过）
///
/// 这条**抓不住** `true`→`false` 的变异。原因在 PostgreSQL 语义：
/// `SET` / `set_config(..., false)` 在**显式事务内**是**事务性**的 ——
/// 事务回滚时它**也**被回滚，所以残留不会出现在 ROLLBACK 之后。
///
/// → 真正能区分 `true` / `false` 的只有上面那条 **COMMIT** 用例
/// （实测：改成 `false` 后它红，本条仍绿）。
/// 本条的价值是锁住「回滚路径也是干净的」这个独立事实 —— 不是变异判据。
#[tokio::test]
async fn the_tenant_context_is_cleared_after_a_rollback() {
    let url = database_url();
    let db = single_connection_db(&url).await;

    let tenant = "tenant-c@td082";
    let ctx = RequestContext::new().with_tenant(tenant);

    {
        let mut tx = db.begin(&ctx).await.expect("应当能开事务");
        let query = TenantQuery::new(
            "SELECT NULLIF(current_setting('app.tenant_id', true), '') AS v \
             WHERE {tenant} IS NOT NULL",
        );
        let row: SettingRow = tx.fetch_one(&query).await.expect("事务内读");
        assert_eq!(row.value.as_deref(), Some(tenant));
        tx.rollback().await.expect("回滚应当成功");
    }

    let after: Option<String> = {
        let mut conn = db.test_acquire().await.expect("借连接");
        sqlx::query_scalar("SELECT NULLIF(current_setting('app.tenant_id', true), '')")
            .fetch_one(&mut *conn)
            .await
            .expect("读取租户上下文")
    };
    assert!(
        after.is_none(),
        "回滚后同样必须清除（`SET LOCAL` 在 COMMIT 与 ROLLBACK 上都生效）；\
         实际残留 {after:?}"
    );
}

// ---------------------------------------------------------------------------
// 关于「RLS 隔离本身」为什么不在本文件
// ---------------------------------------------------------------------------
//
// 「租户 A 看不到租户 B 的行」这件事由**既有测试**覆盖（`platform-audit` /
// `platform-config` / `platform-app` 的真库用例都跑在**应用角色**下验证 RLS）。
//
// 本文件只负责**本 TD 的缺口**：`SET LOCAL` 的清除语义。
// 我一开始在这里写过一条「两个租户轮流用同一连接」的端到端用例，
// 但它：
//   1. **抓不住** `true`→`false` 的变异（每次 `begin` 都会重设值）；
//   2. 用超级用户连接跑会**绕过 RLS** → 恒看到全部行（我实测到 5 行，测试红）。
// → 删掉它，而不是把它改成「看起来在验隔离、实际什么都没验」的样子。
// 与 RLS 相关的真判据在各自 crate 的真库用例里，那边用的是应用角色。
