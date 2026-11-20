//! REQ-010 真实库用例的**夹具**。
//!
//! # 规矩（与 REQ-009 一致）
//!
//! - 只连 `base_test`（`base_dev` 是开发库，测试绝不能碰）；
//! - 缺 `DATABASE_URL_TEST` → **失败**，不是「跳过」（跳过也算通过 = 假绿）；
//! - 业务数据访问走**应用角色** `base_app`（超管会绕过 RLS → 租户隔离 fail open）；
//! - 造数走管理员连接（造数是夹具，不是业务访问）。
//!
//! # 串行化
//!
//! 配置用例会改写**平台级**行（跨租户共享）与固定的 `tenant-a` / `tenant-b` 夹具。
//! 同一测试二进制内的用例用一把异步锁**串行**执行，每条用例开始前把夹具恢复原样
//! （用例不得依赖执行顺序）。

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use platform_config::store::ConfigStore;
use platform_config::{ConfigRegistry, ConfigService, DbConfigResolver};
use platform_core::RequestContext;
use platform_db::{DbOptions, TenantDb, apply_policies, verify_policies};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// 主测试租户。
pub const TENANT_A: &str = "tenant-a";
/// 跨租对照租户。
pub const TENANT_B: &str = "tenant-b";
/// T1 的主空间（T2 也用同一字符串 —— 复合维度是 `(tenant_id, space_id)`）。
pub const SPACE_S1: &str = "space-sales-1";
/// T1 的第二个空间（局部性对照）。
pub const SPACE_S2: &str = "space-legal";

/// 普通用户（T1）。
pub const USER_M: &str = "u-2001";
/// 有 `entry` + `flag` 写权限的用户（T1）。
pub const USER_A: &str = "u-2002";
/// 有全部 4 个 config 权限点的用户（T1）。
pub const USER_P: &str = "u-2003";

/// 部署准备用的 advisory lock（与 REQ-009 不同的键，互不踩）。
pub const SETUP_LOCK: i64 = 6_010_010;

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
    /// 配置键注册表。
    pub registry: Arc<ConfigRegistry>,
    /// 配置服务（真实 DB + 真实注册表）。
    pub service: Arc<ConfigService>,
}

impl Fixture {
    /// 再建一个**独立**的解析器实例（各自持缓存、共享同一 DB）——
    /// 用于 AC-S-052 步骤 4 / AC-S-057 步骤 7 的跨实例断言。
    pub fn new_resolver(&self) -> Arc<DbConfigResolver> {
        Arc::new(DbConfigResolver::new(
            ConfigStore::new(self.app_db.clone()),
            self.registry.clone(),
        ))
    }
}

/// 准备真实库环境并恢复夹具数据。
pub async fn setup() -> Fixture {
    prepare(true).await
}

/// 准备真实库环境，但**不重置**夹具（供跨进程用例的子进程继承父进程写入的状态）。
pub async fn setup_without_reset() -> Fixture {
    prepare(false).await
}

async fn prepare(reset: bool) -> Fixture {
    let url = required_test_database_url();

    let admin_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));

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

    // 2) 平台迁移（含 REQ-010 的三表 + 权限点种子）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    // 3) 按 manifest 的 tenantScoped 段生成 RLS 策略（策略已就位时跳过 DDL，
    //    避免与并发用例的 AccessExclusiveLock 死锁）。
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    // ⚠️ 只校验 / 生成本 crate 自己的表。
    //    `collect_scopes` 会把 sandbox 等模块的表也收进来，而那些表由**各自的**
    //    迁移建 —— 全新库上本文件先跑时它们还不存在（integration 脚本会重置 schema），
    //    对它们 `ALTER TABLE` 会直接 42P01。
    let specs = vec![platform_core::extension::manifest::TenantScopedSpec {
        tables: vec!["platform_space_config".to_string()],
        column: "tenant_id".to_string(),
    }];
    let report = verify_policies(&admin_db, &specs)
        .await
        .expect("隔离状态应当能查出");
    let policies_ready = report.tables.iter().all(|table| table.rls_enabled)
        && report.tables.iter().all(|table| table.policy_count > 0);
    if policies_ready {
        tracing::warn!("RLS 策略已就位，跳过策略生成（避免与并发用例的 DDL 死锁）");
    } else {
        for spec in &specs {
            apply_policies(&admin_db, spec)
                .await
                .expect("RLS 策略生成应能应用");
        }
    }

    // 4) 恢复夹具数据（每条用例开始前）。
    if reset {
        reset_fixture(&admin_pool).await;
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
    let app_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&app_url)
        .await
        .expect("应用角色连接池应能建立");

    let registry = Arc::new(
        ConfigRegistry::from_manifest_root(&root).expect("配置键注册表应能从 manifest 建立"),
    );
    let service = Arc::new(ConfigService::new(app_db.clone(), registry.clone()));

    Fixture {
        admin_pool,
        app_pool,
        app_db,
        registry,
        service,
    }
}

/// 恢复 §3.5 的夹具（平台 / 租户 / 空间三层 + epoch）。
pub async fn reset_fixture(pool: &PgPool) {
    // 平台级（管理员连接绕过 RLS / 直接 DML 权限）。
    //
    // ⚠️ **只删本夹具管的键**（D187）—— 不能用 `DELETE FROM platform_config` 清整表：
    // 那会把**别的 crate 登记在迁移里的键**（如 `audit.*`）一并删掉，而迁移已进
    // `_sqlx_migrations` **不会重跑** → 登记值在共享测试库里静默消失，且**没有任何用例变红**。
    let platform_keys: Vec<String> = platform_fixture()
        .into_iter()
        .map(|(key, _)| key.to_string())
        .collect();
    sqlx::query("DELETE FROM platform_config WHERE key = ANY($1)")
        .bind(&platform_keys)
        .execute(pool)
        .await
        .expect("平台级配置应能清空");
    for (key, value) in platform_fixture() {
        insert_json(pool, "platform_config", &[("key", key.to_string())], &value).await;
    }

    sqlx::query("DELETE FROM platform_config_epoch")
        .execute(pool)
        .await
        .expect("epoch 应能清空");
    sqlx::query(
        "INSERT INTO platform_config_epoch (scope_key, epoch) VALUES \
         ('platform', 0), ('tenant:tenant-a', 0), ('tenant:tenant-b', 0)",
    )
    .execute(pool)
    .await
    .expect("epoch 夹具应能插入");

    let tenants = vec![TENANT_A.to_string(), TENANT_B.to_string()];
    sqlx::query("DELETE FROM platform_tenant_config WHERE tenant_id = ANY($1)")
        .bind(&tenants)
        .execute(pool)
        .await
        .expect("租户级配置应能清空");
    for (tenant, key, value) in tenant_fixture() {
        insert_json(
            pool,
            "platform_tenant_config",
            &[("tenant_id", tenant.to_string()), ("key", key.to_string())],
            &value,
        )
        .await;
    }

    sqlx::query("DELETE FROM platform_space_config WHERE tenant_id = ANY($1)")
        .bind(&tenants)
        .execute(pool)
        .await
        .expect("空间级配置应能清空");
    for (tenant, space, key, value) in space_fixture() {
        insert_json(
            pool,
            "platform_space_config",
            &[
                ("tenant_id", tenant.to_string()),
                ("space_id", space.to_string()),
                ("key", key.to_string()),
            ],
            &value,
        )
        .await;
    }
}

/// 平台级夹具（§3.5）。
pub fn platform_fixture() -> Vec<(&'static str, Value)> {
    vec![
        ("logo", json!("/branding/platform-a.svg")),
        ("favicon", json!("/favicon-platform.svg")),
        ("primaryColor", json!("#111111")),
        ("conflictStrategy", json!("manual")),
        ("scimBaseUrl", json!("https://scim.example.com")),
        ("cacheTtlSeconds", json!(300)),
    ]
}

/// 租户级夹具（§3.5）。
pub fn tenant_fixture() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (TENANT_A, "logo", json!("/branding/tenant-a.svg")),
        (TENANT_A, "title", json!("T1平台")),
        (TENANT_A, "newInvoice", json!(true)),
        (TENANT_A, "pageSize", json!(20)),
        (TENANT_B, "logo", json!("/branding/tenant-b.svg")),
    ]
}

/// 空间级夹具（§3.5；`conflictStrategy` 是管理员 SQL 合成的行，用来验证品牌
/// 接口**永不读取**空间级的非白名单键）。
pub fn space_fixture() -> Vec<(&'static str, &'static str, &'static str, Value)> {
    vec![
        (TENANT_A, SPACE_S1, "logo", json!("/branding/space-s1.svg")),
        (TENANT_A, SPACE_S2, "logo", json!("/branding/space-s2.svg")),
        (
            TENANT_B,
            SPACE_S1,
            "logo",
            json!("/branding/space-t2-s1.svg"),
        ),
        (TENANT_A, SPACE_S1, "conflictStrategy", json!("remote_wins")),
    ]
}

/// 往带 `jsonb` 值列的表插一行（夹具用；管理员连接）。
async fn insert_json(pool: &PgPool, table: &str, columns: &[(&str, String)], value: &Value) {
    let mut names: Vec<String> = columns
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    names.push("value".to_string());
    let mut placeholders: Vec<String> =
        (1..=names.len()).map(|index| format!("${index}")).collect();
    let value_index = names.len();
    // 最后一列是 `jsonb`：绑定的文本要显式 cast（sqlx 不带 json feature）。
    placeholders[value_index - 1] = format!("${value_index}::jsonb");
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        names.join(", "),
        placeholders.join(", ")
    );
    let mut query = sqlx::query(&sql);
    for (_name, value) in columns {
        query = query.bind(value.clone());
    }
    query
        .bind(value.to_string())
        .execute(pool)
        .await
        .unwrap_or_else(|err| panic!("夹具插入 {table} 失败（value 参数 #{value_index}）：{err}"));
}

/// 租户（+ 可选用户）上下文。
pub fn ctx(tenant: &str) -> RequestContext {
    RequestContext::new().with_tenant(tenant)
}

/// 租户 + 空间上下文。
pub fn ctx_space(tenant: &str, space: &str) -> RequestContext {
    RequestContext::new().with_tenant(tenant).with_space(space)
}

/// 租户 + 用户上下文（灰度判定需要用户身份）。
pub fn ctx_user(tenant: &str, user: &str) -> RequestContext {
    RequestContext::new().with_tenant(tenant).with_user(user)
}

/// 读某个 key 在某一层的原始值（管理员连接，绕过 RLS）。
pub async fn raw_value(pool: &PgPool, table: &str, key: &str) -> Option<String> {
    let sql = format!("SELECT value::text FROM {table} WHERE key = $1");
    let row: Option<(String,)> = sqlx::query_as(&sql)
        .bind(key)
        .fetch_optional(pool)
        .await
        .unwrap_or_else(|err| panic!("读 {table}.{key} 失败：{err}"));
    row.map(|(value,)| value)
}

/// 读 epoch。
pub async fn epoch(pool: &PgPool, scope_key: &str) -> i64 {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT epoch FROM platform_config_epoch WHERE scope_key = $1")
            .bind(scope_key)
            .fetch_optional(pool)
            .await
            .expect("epoch 应能读取");
    row.map(|(value,)| value).unwrap_or(0)
}

/// 跑一段原始 SQL 并返回 `sqlx` 错误（用于断言 42501）。
pub async fn raw_error(pool: &PgPool, sql: &str) -> Option<sqlx::Error> {
    sqlx::query(sql).execute(pool).await.err()
}

/// 取出 SQLSTATE。
pub fn sqlstate(error: &sqlx::Error) -> Option<String> {
    match error {
        sqlx::Error::Database(database) => database.code().map(|code| code.to_string()),
        _ => None,
    }
}
