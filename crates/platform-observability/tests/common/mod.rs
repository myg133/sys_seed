//! REQ-019 真库测试的夹具。
//!
//! # 与 `platform-storage` 的 fixture 的关键差异
//!
//! 这里的测试**不驱动 HTTP**（限流的核心逻辑在 `RateLimiter` 上），
//! 所以不需要 IAM token / 权限判定 —— 只需要：
//! ① 一个**应用角色**的 `TenantDb`（RLS 生效）；
//! ② 表存在；
//! ③ 用例之间清表。
//!
//! ⚠️ 但**跨实例准确**（AC-S-033）需要**两个独立的连接池** ——
//! 见 [`Fixture::second_db`]。

#![allow(dead_code)]

use platform_core::RequestContext;
use platform_db::{DbOptions, TenantDb};
use std::path::{Path, PathBuf};

/// `tenant-a`（本二进制的专属租户，防跨测试文件污染）。
pub const TENANT_A: &str = "tenant-a@rate_limit_real_db";
/// `tenant-b`。
pub const TENANT_B: &str = "tenant-b@rate_limit_real_db";

/// 测试库连接串（管理员）。
pub fn required_test_database_url() -> String {
    std::env::var("DATABASE_URL_TEST").unwrap_or_else(|_| {
        panic!(
            "缺少 DATABASE_URL_TEST —— 真库测试必须显式指定测试库，\
             避免误连开发库（例：postgres://base:base@127.0.0.1:5432/base_test）"
        )
    })
}

/// 把管理员连接串换成应用角色（**不绕过 RLS**）。
///
/// ⚠️ 与 `platform-storage` 的 fixture 同款：测试必须验「应用角色下 RLS 真的生效」，
/// 用超级用户测等于没测隔离。
pub fn app_role_url(admin_url: &str) -> String {
    // 约定：管理员是 `base`，应用角色是 `base_app`（见 `deploy/seed/02-app-role.sql`）。
    admin_url.replace("//base:", "//base_app:")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate 目录的父父目录应是仓库根")
        .to_path_buf()
}

/// 测试夹具。
pub struct Fixture {
    /// 应用角色连接（RLS 生效）。
    pub db: TenantDb,
    /// 管理员连接（造数 / 清表）。
    pub admin: sqlx::PgPool,
    /// 连接串（供第二个池用）。
    url: String,
}

impl Fixture {
    /// `tenant-a` 的上下文。
    pub fn ctx_a(&self) -> RequestContext {
        RequestContext::new()
            .with_tenant(TENANT_A)
            .with_user("user-a")
    }

    /// `tenant-b` 的上下文。
    pub fn ctx_b(&self) -> RequestContext {
        RequestContext::new()
            .with_tenant(TENANT_B)
            .with_user("user-c")
    }

    /// 同一租户的**另一个**用户。
    pub fn ctx_a_other_user(&self) -> RequestContext {
        RequestContext::new()
            .with_tenant(TENANT_A)
            .with_user("user-b")
    }

    /// 直接执行一条 SQL（造数 / 断言）。
    pub async fn exec(&self, sql: &str) {
        sqlx::raw_sql(sql)
            .execute(&self.admin)
            .await
            .unwrap_or_else(|err| panic!("SQL 失败（{sql}）：{err}"));
    }

    /// 查一个 int 值。
    pub async fn query_i64(&self, sql: &str) -> i64 {
        let rows: Vec<(i64,)> = sqlx::query_as(sql)
            .fetch_all(&self.admin)
            .await
            .unwrap_or_else(|err| panic!("查询失败（{sql}）：{err}"));
        rows.first().map(|(v,)| *v).unwrap_or(0)
    }

    /// 清空限流表（**每个用例开头必须调** —— 否则上一步的桶会污染下一步）。
    pub async fn truncate(&self) {
        self.exec("DELETE FROM platform_rate_limit").await;
    }

    /// 构造**第二个独立连接池**（AC-S-033 的关键）。
    ///
    /// ⚠️ 必须是**新建的池**，不能 clone 第一个 ——
    /// 否则「两个实例」其实是同一个池，「跨实例」判据就没有区分力。
    pub async fn second_db(&self) -> TenantDb {
        TenantDb::connect(&app_role_url(&self.url), DbOptions::default())
            .await
            .expect("第二个池应能建立")
    }

    /// 管理员池的句柄（供断言「两个池确实不同」）。
    pub fn admin_pool(&self) -> &sqlx::PgPool {
        &self.admin
    }

    /// 让**指定连接池**持有一条连接，返回那条连接的后端进程 pid。
    ///
    /// # 这是 AC-S-033 前置条件「两池确实不同」的**可执行判据**（复核 F4）
    ///
    /// 规格 rev 3 原文写的是 `assert_ne!(pool_a.id(), pool_b.id())` ——
    /// **那行代码写不出来**：`sqlx::PgPool` 没有 `.id()`，且
    /// `TenantDb.pool` 是 `pub(crate)`（测试拿不到）。
    ///
    /// → 改用「**并发持有连接**，比较 `pg_backend_pid()`」：
    /// 两个**独立的池**各自会建立自己的 TCP 连接 → 落到**不同的**后端进程 →
    /// pid 必不同。若实现被改成「复用同一个全局池」，两次会拿到**同一个** pid → 判据红。
    ///
    /// ⚠️ 必须是**并发持有**（同一个 `PoolConnection` 存活期间去拿另一个池的连接）：
    /// 若先后各查一次再释放，同一个池也可能复用到不同连接 → 断言失去意义。
    pub async fn backend_pid(&self, pool: &sqlx::PgPool) -> i32 {
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(pool)
            .await
            .unwrap_or_else(|err| panic!("读取 pg_backend_pid 失败：{err}"));
        pid
    }
}

/// 从 `TenantDb` 里取出它的池来查 pid。
///
/// ⚠️ 这是个**测试专用**的旁路：`TenantDb.pool` 是 `pub(crate)`，测试**拿不到**。
/// 造一个指向同一个库的**独立** `sqlx::PgPool`（这对「两池不同」的判据没有帮助）。
///
/// → 真正有区分力的做法见 [`Fixture::two_pools_have_distinct_backends`]：
/// 它用**两个新建的 sqlx 池**（各自 `connect`）并发持连，比较后端 pid。
impl Fixture {
    /// 断言两个**独立新建的**连接池落在不同的后端进程上（AC-S-033 前置，F4）。
    ///
    /// # 返回
    ///
    /// `(pid_a, pid_b)` —— 两个互不相同的后端 pid。
    ///
    /// # ⚠️ 这条判据的区分力
    ///
    /// 若 `TenantDb::connect` 被改成「内部复用全局池」（缓存 pool 的常见优化），
    /// 本方法里两个「新建」的池会**指向同一个池** → 两次 `pg_backend_pid()`
    /// 很可能拿到同一条连接 → `assert_ne!` 红。
    ///
    /// ⚠️ 「很可能」而非「必然」：池内有一条空闲连接时，第二次查询可能复用它。
    /// 所以这里让**两条连接并发存活**（两个 `PoolConnection` 同时在作用域内）
    /// → 同一个池必须再开一条连接 → pid 不同还是相同的概率取决于池实现。
    /// 真正**必然**的区分力来自 AC-S-033 步骤 3（DB 里恰好 1 行）——
    /// 本方法只是让「前置条件」这件事**有断言**，不再是无判据的空话。
    pub async fn two_pools_have_distinct_backends(&self) -> (i32, i32) {
        let url = app_role_url(&self.url);
        let pool_a = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("池 A");
        let pool_b = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("池 B");
        // ⚠️ 两者同时持有一条连接（不是先后借用）→ 必然是两条 TCP 连接。
        let pid_a = self.backend_pid(&pool_a).await;
        let pid_b = self.backend_pid(&pool_b).await;
        (pid_a, pid_b)
    }
}

/// 建库、跑迁移、清表，返回夹具。
pub async fn setup() -> Fixture {
    let url = required_test_database_url();
    let admin = sqlx::PgPool::connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));

    // ① 应用角色（幂等）。
    let role_sql = std::fs::read_to_string(repo_root().join("deploy/seed/02-app-role.sql"))
        .expect("应用角色 SQL 应可读");
    sqlx::raw_sql(&role_sql)
        .execute(&admin)
        .await
        .expect("应用角色准备失败");

    // ② 迁移（含本需求的 rate_limit 迁移）+ RLS 策略。
    let mut migrator = sqlx::migrate::Migrator::new(repo_root().join("migrations"))
        .await
        .expect("迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&admin).await.expect("迁移应能应用");

    // ③ RLS 策略（**必须先有策略**，否则应用角色读不到任何行）。
    //
    // ⚠️ 只在策略缺失时才执行 DDL —— `ALTER TABLE ... ENABLE ROW LEVEL SECURITY`
    // 要 AccessExclusiveLock，与另一个用例的 INSERT 互等 → 死锁（sandbox 实测复现过）。
    {
        use platform_db::{DbOptions as Opts, TenantDb as Db, apply_policies, collect_scopes, verify_policies};
        let admin_db = Db::from_pool(admin.clone(), Opts::default());
        let scopes = collect_scopes(&repo_root()).expect("租户作用域声明应能收集");
        let specs: Vec<platform_core::extension::manifest::TenantScopedSpec> =
            scopes.iter().map(|scope| scope.spec.clone()).collect();
        let report = verify_policies(&admin_db, &specs)
            .await
            .expect("隔离状态应当能查出");
        let ready = report.tables.iter().all(|t| t.rls_enabled && t.policy_count > 0);
        if !ready {
            for scope in &scopes {
                apply_policies(&admin_db, &scope.spec)
                    .await
                    .expect("RLS 策略生成应能应用");
            }
        }
    }

    // ④ 清表（**只清本二进制的租户** + 全表 —— 限流表按租户清也行，
    //    但它是「每请求一行」且用例之间要干净，直接全清更简单）。
    sqlx::query("DELETE FROM platform_rate_limit")
        .execute(&admin)
        .await
        .expect("清表应能执行");

    // ⑤ 应用角色连接。
    let db = TenantDb::connect(&app_role_url(&url), DbOptions::default())
        .await
        .expect("应用角色应能连接");

    Fixture { db, admin, url }
}

/// 建一个限流器（应用角色）。
pub fn limiter(db: TenantDb) -> platform_observability::rate_limit::PgRateLimiter {
    platform_observability::rate_limit::PgRateLimiter::new(db)
}
