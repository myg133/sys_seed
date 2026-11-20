//! REQ-014 真实库用例的**夹具**。
//!
//! # 规矩（与 REQ-009 / REQ-010 一致）
//!
//! - 只连 `base_test`（`base_dev` 是开发库，测试绝不能碰）；
//! - 缺 `DATABASE_URL_TEST` → **失败**，不是「跳过」（跳过也算通过 = 假绿）；
//! - 业务数据访问走**应用角色** `base_app`（超管会绕过 RLS → 租户隔离 fail open）；
//! - 造数 / 清库走管理员连接（造数是夹具，不是业务访问）。

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::OnceLock;

use platform_db::{DbOptions, TenantDb};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// 主测试租户（D168：统一用 `tenant-a` / `tenant-b`）。
pub const TENANT_A: &str = "tenant-a";
/// 跨租对照租户。
pub const TENANT_B: &str = "tenant-b";
/// 操作人。
pub const USER_M: &str = "u-2001";

/// 部署准备用的 advisory lock（与其它需求的键互不踩）。
pub const SETUP_LOCK: i64 = 6_140_020;

/// 测试库地址：只允许 `base_test`。
pub fn required_test_database_url() -> String {
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

/// 把管理员连接串换成**应用角色**的连接串。
pub fn app_role_url(admin_url: &str) -> String {
    let (scheme, rest) = admin_url.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

/// 仓库根目录。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 串行锁（同一测试二进制内）。
pub async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

/// 测试夹具。
pub struct Fixture {
    /// 管理员连接池（造数 / DDL / 原始 SQL 断言）。
    pub admin_pool: PgPool,
    /// 应用角色连接池（**不绕过 RLS**；直接 DML 权限断言用它）。
    pub app_pool: PgPool,
    /// 应用角色的租户作用域入口（业务读写的真实形态）。
    pub app_db: TenantDb,
}

impl Fixture {
    /// 租户 `tenant-a` 的请求上下文（已认证 + 带 IP / UA）。
    pub fn ctx_tenant_a(&self) -> platform_core::RequestContext {
        platform_core::RequestContext::new()
            .with_tenant(TENANT_A)
            .with_user(USER_M)
            .with_client_ip("203.0.113.9")
            .with_user_agent("audit-test-agent")
    }

    /// 租户 `tenant-b` 的请求上下文。
    pub fn ctx_tenant_b(&self) -> platform_core::RequestContext {
        platform_core::RequestContext::new()
            .with_tenant(TENANT_B)
            .with_user(USER_M)
            .with_client_ip("203.0.113.10")
            .with_user_agent("audit-test-agent")
    }
}

/// 准备真实库环境并恢复夹具数据。
pub async fn setup() -> Fixture {
    let url = required_test_database_url();

    let admin_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));

    let root = repo_root();

    // 整段准备过程串行化（两个测试并行时不会互相踩 DDL）。
    let mut conn = admin_pool.acquire().await.expect("应当能取到连接");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能拿到部署锁");

    // 1) 应用角色（RLS 前提：应用连接不能是超级用户）。
    let role_sql = std::fs::read_to_string(root.join("deploy/seed/02-app-role.sql"))
        .expect("应用角色脚本应可读");
    sqlx::raw_sql(&role_sql)
        .execute(&mut *conn)
        .await
        .expect("应用角色准备失败");

    // 2) 平台迁移（含 REQ-014 的三表 + 权限点种子）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    // 3) 清空审计夹具（管理员连接是表 owner，可以 DELETE）。
    reset_audit(&admin_pool).await;

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    // 4) 应用角色连接（**不绕过 RLS** 的普通角色）。
    let app_url = app_role_url(&url);
    let app_db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let app_pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&app_url)
        .await
        .expect("应用角色连接池应能建立");

    Fixture {
        admin_pool,
        app_pool,
        app_db,
    }
}

/// 清空三张审计表（夹具复位）。
pub async fn reset_audit(pool: &PgPool) {
    sqlx::query("DELETE FROM platform_audit_log")
        .execute(pool)
        .await
        .expect("在线审计表应能清空");
    sqlx::query("DELETE FROM platform_audit_archive")
        .execute(pool)
        .await
        .expect("归档表应能清空");
    sqlx::query("DELETE FROM platform_audit_chain_state")
        .execute(pool)
        .await
        .expect("链基准表应能清空");
}
