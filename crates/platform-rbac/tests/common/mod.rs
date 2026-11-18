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

/// 探针 schema 的前缀。
///
/// ⚠️ **主部门歧义的验证必须摘掉部分唯一索引**（预防层失效），
/// 而那个索引是**全局**的 —— 摘掉它会影响所有租户与所有并发用例
/// （QA 问题 #2：这就是「预防层被关掉」的窗口）。
/// 所以那条验证改在**独立 schema** 里做 —— 用的正是平台自己的
/// 「L2 独立 schema」能力（`DbOptions.schema`，见 design-summary §2）：
/// **全局索引一点不碰**。
pub const PROBE_SCHEMA_PREFIX: &str = "rbac_probe";

/// 探针 schema 的**最长存活时间**（小时）—— 超过就当残留清理掉。
///
/// 探针 schema 名带**本轮运行标识**（并发跑不会撞名），
/// 残留只可能来自「跑挂了」的那一轮 → 按创建时间过期清理。
pub const PROBE_SCHEMA_TTL_HOURS: i64 = 1;

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

// ---------------------------------------------------------------------------
// 探针 schema（**预防层验证的隔离区**）
// ---------------------------------------------------------------------------

/// 建一个**独立 schema** 的夹具（表结构 + 数据从 `public` 复制一份）。
///
/// # 为什么要有它
///
/// 「主部门歧义 → fail closed」的验证必须**摘掉部分唯一索引**（预防层失效）——
/// 而那个索引是**全局**的：摘掉它会影响所有租户与所有并发用例
/// （QA 问题 #2 实测：索引摘掉期间第二个主部门写得进去，
/// `CREATE UNIQUE INDEX` 随即失败；若在两者之间 panic，
/// 预防层会留在关闭状态直到本轮结束 → 后续用例失去 DB 层保护）。
///
/// 独立 schema 用的正是平台自己的「L2 独立 schema」能力（`DbOptions.schema`，
/// 见 design-summary §2）：**全局索引一点不碰**，索引操作被限制在探针里。
///
/// # 怎么做的
///
/// 1. 建 schema，把 `public` 下的表**结构 + 数据**复制一份
///    （`LIKE ... INCLUDING ALL` —— 主键 / 唯一约束 / 索引一并复制）；
/// 2. 把复制来的**部分唯一索引改名**成规范名 `user_org_one_primary_idx`
///    （规范名 → 断言可以按名字查）；
/// 3. 授权给应用角色组（**探针表不开 RLS** —— 探针只验范围判定层，
///    租户隔离由 `public` 里那套用例负责）；
/// 4. 连接把 `search_path` 指到探针 schema —— 夹具里的 SQL 用的是
///    **不带 schema 的表名**，这样同一套夹具函数能直接用在探针上。
pub async fn create_probe_fixture(fixture: &Fixture, schema: &str) -> Fixture {
    assert!(
        schema.starts_with(PROBE_SCHEMA_PREFIX)
            && schema
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
        "探针 schema 名必须是安全标识符：{schema}"
    );
    let url = required_test_database_url();

    // 0) 清掉上一轮跑挂留下的同名残留（保证从干净状态开始）。
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&fixture.admin_pool)
        .await
        .expect("清理同名探针 schema 应当成功");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&fixture.admin_pool)
        .await
        .expect("探针 schema 应当能建");
    // 记下创建时间（跑挂残留的判据）。
    stamp_probe_schema(fixture, schema).await;

    // 1) 把 `public` 的表（结构 + 数据）复制一份。
    sqlx::raw_sql(&format!(
        r#"
        DO $$
        DECLARE r record;
        BEGIN
            FOR r IN
                SELECT c.relname AS name
                FROM pg_class c
                JOIN pg_namespace n ON n.oid = c.relnamespace
                WHERE n.nspname = 'public'
                  AND c.relkind = 'r'
                  AND c.relispartition = false
            LOOP
                EXECUTE format('CREATE TABLE %I.%I (LIKE public.%I INCLUDING ALL)',
                               '{schema}', r.name, r.name);
                EXECUTE format('INSERT INTO %I.%I SELECT * FROM public.%I',
                               '{schema}', r.name, r.name);
            END LOOP;
        END $$;
        "#
    ))
    .execute(&fixture.admin_pool)
    .await
    .expect("探针 schema 应当能复制表");

    // 2) 部分唯一索引 → 规范名（复制时 Postgres 会改名）。
    sqlx::raw_sql(&format!(
        r#"
        DO $$
        DECLARE idx text;
        BEGIN
            SELECT indexname INTO idx
            FROM pg_indexes
            WHERE schemaname = '{schema}'
              AND tablename = 'user_org'
              AND indexdef LIKE '%UNIQUE INDEX%'
              AND indexdef LIKE '%is_primary%'
            LIMIT 1;
            IF idx IS NOT NULL THEN
                EXECUTE format('ALTER INDEX %I.%I RENAME TO user_org_one_primary_idx',
                               '{schema}', idx);
            END IF;
        END $$;
        CREATE UNIQUE INDEX IF NOT EXISTS user_org_one_primary_idx
            ON {schema}.user_org (tenant_id, user_id) WHERE is_primary;
        "#
    ))
    .execute(&fixture.admin_pool)
    .await
    .expect("探针的部分唯一索引应当能就位");

    // 3) 授权给应用角色组（探针表不开 RLS —— 探针只验范围判定层）。
    sqlx::raw_sql(&format!(
        "GRANT USAGE ON SCHEMA {schema} TO base_platform_app; \
         GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA {schema} TO base_platform_app; \
         GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA {schema} TO base_platform_app;"
    ))
    .execute(&fixture.admin_pool)
    .await
    .expect("探针 schema 应当能授权给应用角色");

    // 4) 连接：`search_path` 指到探针 schema（夹具 SQL 用的是不带 schema 的表名）。
    let admin_pool = probe_pool(&url, schema, 4)
        .await
        .expect("探针管理员连接应当能建立");
    let app_pool = probe_pool(&app_role_url(&url), schema, 4)
        .await
        .expect("探针应用连接应当能建立");

    Fixture {
        admin_db: TenantDb::from_pool(
            admin_pool.clone(),
            DbOptions {
                schema: schema.to_string(),
                ..DbOptions::default()
            },
        ),
        admin_pool,
        app_db: TenantDb::from_pool(
            app_pool,
            DbOptions {
                schema: schema.to_string(),
                ..DbOptions::default()
            },
        ),
    }
}

/// 建一个 `search_path` 指向探针 schema 的连接池。
async fn probe_pool(
    url: &str,
    schema: &str,
    max_connections: u32,
) -> Result<sqlx::PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .after_connect({
            let schema = schema.to_string();
            move |conn, _| {
                let schema = schema.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}, public"))
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            }
        })
        .connect(url)
        .await
}

/// 删掉探针 schema（**用例收尾** —— 别把隔离区留在库里）。
pub async fn drop_probe_schema(fixture: &Fixture, schema: &str) {
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&fixture.admin_pool)
        .await
        .expect("探针 schema 应当能删掉");
}

/// 清理**过期**的探针 schema（跑挂的那一轮留下的）。
///
/// 判据 = schema 的注释里记的创建时间（`created_at=...`）——
/// 超过 [`PROBE_SCHEMA_TTL_HOURS`] 的才算残留。
/// **不碰本轮的**（并发跑时别人的探针还活着）。
pub async fn drop_stale_probe_schemas(fixture: &Fixture, keep: &str) {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT nspname, COALESCE(obj_description(oid, 'pg_namespace'), '') \
         FROM pg_namespace WHERE nspname LIKE $1",
    )
    .bind(format!("{}%", PROBE_SCHEMA_PREFIX))
    .fetch_all(&fixture.admin_pool)
    .await
    .unwrap_or_default();

    for (name, comment) in rows {
        if name == keep {
            continue;
        }
        let Some(created_at) = comment
            .strip_prefix("created_at=")
            .and_then(|value| value.parse::<i64>().ok())
        else {
            continue;
        };
        let age_hours = (now_epoch() - created_at) / 3600;
        if age_hours >= PROBE_SCHEMA_TTL_HOURS {
            sqlx::query(&format!("DROP SCHEMA IF EXISTS {name} CASCADE"))
                .execute(&fixture.admin_pool)
                .await
                .ok();
        }
    }
}

/// 当前 Unix 时间（秒）。
fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default()
}

/// 给探针 schema 记上创建时间（供 [`drop_stale_probe_schemas`] 判残留）。
pub async fn stamp_probe_schema(fixture: &Fixture, schema: &str) {
    let created_at = now_epoch();
    sqlx::query(&format!(
        "COMMENT ON SCHEMA {schema} IS 'created_at={created_at}'"
    ))
    .execute(&fixture.admin_pool)
    .await
    .expect("探针 schema 应当能记创建时间");
}
