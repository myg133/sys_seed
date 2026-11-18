//! RBAC 真实库用例的**夹具**。
//!
//! # 规矩
//!
//! - 只连 `base_test`（`base_dev` 是开发库，测试绝不能碰）；
//! - 缺 `DATABASE_URL_TEST` → **失败**，不是「跳过」（跳过也算通过 = 假绿）；
//! - 业务数据访问走**应用角色** `base_app`（超管会绕过 RLS → 租户隔离 fail open）；
//! - 造数走管理员连接（造数是夹具，不是业务访问）。

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use platform_core::RequestContext;
use platform_db::{DbOptions, TenantDb, apply_policies, collect_scopes, verify_policies};
use sqlx::postgres::PgPoolOptions;

/// 主测试租户。
pub const TENANT_A: &str = "tenant-a";
/// 跨租对照租户。
pub const TENANT_B: &str = "tenant-b";

/// 部署准备用的 advisory lock 键（两个测试并行时不互相踩）。
pub const SETUP_LOCK: i64 = 6_009_009;

/// 数据范围示例表。
pub const SCOPE_ITEM_TABLE: &str = "sandbox_scope_item";

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

/// 读一个文件（缺失就 panic —— 夹具坏了要立刻暴露）。
fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()))
}

/// 测试夹具：管理员连接（造数 / 建策略）+ 应用角色连接（业务数据访问）。
pub struct Fixture {
    /// 管理员连接（造数用；**绕过 RLS**，只用于夹具）。
    pub admin_db: TenantDb,
    /// 管理员裸连接池 —— **平台级表**（没有 `tenant_id`）只能用它写。
    pub admin_pool: sqlx::PgPool,
    /// 应用角色连接（**不绕过 RLS**；业务数据访问用这个）。
    pub app_db: TenantDb,
}

/// 准备真实库环境。
///
/// 顺序：应用角色 → 平台迁移 → 模块建表 → **平台按 manifest 生成 RLS 策略** → 应用角色连接。
pub async fn setup() -> Fixture {
    let url = required_test_database_url();

    let admin_pool = match PgPoolOptions::new().max_connections(4).connect(&url).await {
        Ok(pool) => pool,
        Err(err) => {
            panic!("测试库不可达（{url}）：{err}\n  需要集成测试时先执行 'bun run db:up'");
        }
    };

    let root = repo_root();

    // 整段准备过程串行化（两个测试并行时不会互相踩 DDL / policy）。
    let mut conn = admin_pool.acquire().await.expect("应当能取到连接");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能拿到部署锁");

    // 1) 应用角色（RLS 前提：应用连接不能是超级用户）。
    let role_sql = read_file(&root.join("deploy/seed/02-app-role.sql"));
    sqlx::raw_sql(&role_sql)
        .execute(&mut *conn)
        .await
        .expect("应用角色准备失败");

    // 2) 平台迁移（平台表 + RBAC 的权限点种子）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    // 3) 模块建表（模块的表由模块自己的迁移负责；policy 不在这里 —— 见下一步）。
    let module_migrations = root.join("modules/sandbox/migrations");
    let mut up_files: Vec<PathBuf> = std::fs::read_dir(&module_migrations)
        .expect("模块迁移目录应可读")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".up.sql"))
        })
        .collect();
    up_files.sort();
    for path in up_files {
        let sql = read_file(&path);
        sqlx::raw_sql(&sql)
            .execute(&mut *conn)
            .await
            .expect("模块迁移应能应用");
    }

    // 4) **平台按 manifest 的 tenantScoped 段统一生成 RLS 策略**（模块不手写）。
    //
    //    ⚠️ **只在策略缺失时才执行 DDL**：`ALTER TABLE ... ENABLE ROW LEVEL SECURITY`
    //    要 **AccessExclusiveLock**，而另一个用例正在往同一张表 **INSERT** 时
    //    持有 **RowShareLock** → 两边互等 → **死锁**（实测复现）。
    //    策略已经就位时跳过 DDL，这条死锁就消失了。
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    let scopes = collect_scopes(&root).expect("租户作用域声明应能收集");
    let specs: Vec<platform_core::extension::manifest::TenantScopedSpec> =
        scopes.iter().map(|scope| scope.spec.clone()).collect();
    let report = verify_policies(&admin_db, &specs)
        .await
        .expect("隔离状态应当能查出");
    let policies_ready = report.tables.iter().all(|table| table.rls_enabled)
        && report.tables.iter().all(|table| table.policy_count > 0);
    if policies_ready {
        tracing::warn!("RLS 策略已就位，跳过策略生成（避免与并发用例的 DDL 死锁）");
    } else {
        for scope in &scopes {
            apply_policies(&admin_db, &scope.spec)
                .await
                .expect("RLS 策略生成应能应用");
        }
    }

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    // 5) 应用角色连接（**不绕过 RLS** 的普通角色）。
    let app_url = app_role_url(&url);
    let app_db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    Fixture {
        admin_db,
        admin_pool: admin_pool.clone(),
        app_db,
    }
}

/// 租户 A 的上下文。
pub fn ctx_a() -> RequestContext {
    RequestContext::new().with_tenant(TENANT_A)
}

/// 租户 B 的上下文。
pub fn ctx_b() -> RequestContext {
    RequestContext::new().with_tenant(TENANT_B)
}

/// 造一个本地账号（`platform_user_role` 的外键目标）。
pub async fn seed_iam_user(fixture: &Fixture, tenant_id: &str, user_id: &str) {
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let query = platform_db::TenantQuery::new(
        "INSERT INTO platform_iam_user (id, tenant_id, username, email, password_hash, status) \
         VALUES (?, {tenant}, ?, ?, 'x', 'active') \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id.to_string())
    .bind(format!("{user_id}@example.com"))
    .bind(format!("{user_id}@example.com"));
    fixture
        .admin_db
        .execute(&ctx, &query)
        .await
        .expect("账号夹具应能插入");
}

/// 造组织节点（`parent_id = None` 表示根）。
pub async fn seed_org_node(fixture: &Fixture, tenant_id: &str, id: &str, parent_id: Option<&str>) {
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let query = platform_db::TenantQuery::new(
        "INSERT INTO org_node (tenant_id, id, parent_id, node_type, code, name, path_labels) \
         VALUES ({tenant}, ?, ?, 'dept', ?, ?, '[]') \
         ON CONFLICT (tenant_id, id) DO NOTHING",
    )
    .bind(id.to_string())
    .bind(parent_id.map(str::to_string))
    .bind(id.to_string())
    .bind(id.to_string());
    fixture
        .admin_db
        .execute(&ctx, &query)
        .await
        .expect("组织节点夹具应能插入");
}

/// 造成员关系（`is_primary` 标主部门）。
pub async fn seed_membership(
    fixture: &Fixture,
    tenant_id: &str,
    user_id: &str,
    org_node_id: &str,
    is_primary: bool,
) {
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let query = platform_db::TenantQuery::new(
        "INSERT INTO user_org (tenant_id, id, user_id, org_node_id, role, is_primary) \
         VALUES ({tenant}, ?, ?, ?, NULL, ?) \
         ON CONFLICT (tenant_id, id) DO NOTHING",
    )
    .bind(format!("uo-{user_id}-{org_node_id}"))
    .bind(user_id.to_string())
    .bind(org_node_id.to_string())
    .bind(is_primary);
    fixture
        .admin_db
        .execute(&ctx, &query)
        .await
        .expect("成员关系夹具应能插入");
}

/// 造数据范围示例记录。
pub async fn seed_scope_item(
    fixture: &Fixture,
    tenant_id: &str,
    id: &str,
    org_node_id: &str,
    created_by: &str,
    amount: i64,
) {
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let query = platform_db::TenantQuery::new(
        "INSERT INTO sandbox_scope_item (tenant_id, id, title, amount, org_node_id, created_by) \
         VALUES ({tenant}, ?, ?, ?, ?, ?) \
         ON CONFLICT (tenant_id, id) DO UPDATE \
         SET org_node_id = EXCLUDED.org_node_id, created_by = EXCLUDED.created_by, \
             amount = EXCLUDED.amount",
    )
    .bind(id.to_string())
    .bind(format!("标题 {id}"))
    .bind(amount)
    .bind(org_node_id.to_string())
    .bind(created_by.to_string());
    fixture
        .admin_db
        .execute(&ctx, &query)
        .await
        .expect("数据范围夹具应能插入");
}

/// 读某租户的全部记录 id（**按 id 排序**）。
pub async fn list_scope_item_ids(fixture: &Fixture, tenant_id: &str) -> Vec<String> {
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let query = platform_db::TenantQuery::new(
        "SELECT id FROM sandbox_scope_item WHERE tenant_id = {tenant} ORDER BY id",
    );
    let rows: Vec<(String,)> = fixture
        .admin_db
        .fetch_all(&ctx, &query)
        .await
        .expect("记录应能读出");
    rows.into_iter().map(|(id,)| id).collect()
}

/// 读某租户的全部记录 id（**经由应用角色连接**，受 RLS 约束）。
pub async fn list_scope_item_ids_as_app(fixture: &Fixture, tenant_id: &str) -> Vec<String> {
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let query = platform_db::TenantQuery::new(
        "SELECT id FROM sandbox_scope_item WHERE tenant_id = {tenant} ORDER BY id",
    );
    let rows: Vec<(String,)> = fixture
        .app_db
        .fetch_all(&ctx, &query)
        .await
        .expect("记录应能读出");
    rows.into_iter().map(|(id,)| id).collect()
}
