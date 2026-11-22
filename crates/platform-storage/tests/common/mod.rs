//! REQ-018 **真实库**集成用例的夹具（真库 + 真临时目录，design §9）。
//!
//! # 规矩（沿用 REQ-015 / REQ-016）
//!
//! - 只连 `base_test`；缺 `DATABASE_URL_TEST` → **失败**，不是跳过；
//! - 业务数据访问走**应用角色** `base_app`（超管会绕过 RLS → 隔离 fail open）；
//! - 造数 / 断言走管理员连接。

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use platform_core::RequestContext;
use platform_db::{DbOptions, TenantDb, apply_policies, collect_scopes};
use sqlx::postgres::PgPoolOptions;

use platform_storage::config::{
    LocalStorageEnv, StorageBoxFuture, StorageConfig, StorageConfigSource,
};
use platform_storage::object_store::ObjectStore;
use platform_storage::object_store::local_fs::LocalFsStore;
use platform_storage::scanner::{NoopScanner, ScanFuture, ScanVerdict, VirusScanner};
use platform_storage::service::StorageService;
use platform_storage::store::StorageStore;

/// 主测试租户。
pub const TENANT_A: &str = "tenant-a";
/// 跨租对照租户。
pub const TENANT_B: &str = "tenant-b";

/// 部署准备用的 advisory lock 键（与其它 crate 不同，避免互相阻塞）。
pub const SETUP_LOCK: i64 = 6_018_018;

/// 仓库根目录。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()))
}

/// 测试库地址：只允许 `base_test`。
pub fn required_test_database_url() -> String {
    let url = std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "缺少 DATABASE_URL_TEST：真实库用例必须真的连库跑，不能静默跳过。\n  \
                 库名必须恰好是 base_test。"
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
    assert_eq!(db, "base_test", "测试库必须恰好是 base_test，实际是 {db}");
    url
}

/// 把管理员连接串换成**应用角色**。
pub fn app_role_url(admin_url: &str) -> String {
    let (scheme, rest) = admin_url.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

// ---------------------------------------------------------------------------
// 可配置的配置源
// ---------------------------------------------------------------------------

/// 固定的配置源（用例可改）。
#[derive(Debug, Clone)]
pub struct StaticConfigSource {
    inner: Arc<std::sync::Mutex<std::collections::BTreeMap<String, i64>>>,
}

impl StaticConfigSource {
    /// 用默认值构造。
    pub fn new() -> Self {
        let d = platform_storage::config::StorageSettings::default();
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            platform_storage::config::CONFIG_KEY_MAX_UPLOAD_BYTES.to_string(),
            d.max_upload_bytes,
        );
        map.insert(
            platform_storage::config::CONFIG_KEY_PART_SIZE_BYTES.to_string(),
            d.part_size_bytes,
        );
        map.insert(
            platform_storage::config::CONFIG_KEY_PRESIGN_TTL_SECONDS.to_string(),
            d.presign_ttl_seconds,
        );
        map.insert(
            platform_storage::config::CONFIG_KEY_IMPORT_BATCH_SIZE.to_string(),
            d.import_batch_size,
        );
        map.insert(
            platform_storage::config::CONFIG_KEY_MAX_ROW_BYTES.to_string(),
            d.max_row_bytes,
        );
        map.insert(
            platform_storage::config::CONFIG_KEY_UPLOAD_SESSION_TTL_SECONDS.to_string(),
            d.upload_session_ttl_seconds,
        );
        Self {
            inner: Arc::new(std::sync::Mutex::new(map)),
        }
    }

    /// 改一个键。
    pub fn set(&self, key: &str, value: i64) {
        self.inner
            .lock()
            .expect("锁")
            .insert(key.to_string(), value);
    }
}

impl Default for StaticConfigSource {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageConfigSource for StaticConfigSource {
    fn read_platform<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        key: &'a str,
    ) -> StorageBoxFuture<'a, Option<i64>> {
        let value = self.inner.lock().expect("锁").get(key).copied();
        Box::pin(async move { value })
    }
}

// ---------------------------------------------------------------------------
// 可控的扫描器
// ---------------------------------------------------------------------------

/// 可控的扫描器（用例可以切成「判定感染」）。
#[derive(Debug, Clone)]
pub struct SwitchableScanner {
    /// 为 `true` 时返回「感染」。
    pub infect: Arc<std::sync::atomic::AtomicBool>,
}

impl SwitchableScanner {
    /// 默认「不扫」。
    pub fn new() -> Self {
        Self {
            infect: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// 切成「判定感染」。
    pub fn set_infected(&self) {
        self.infect.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Default for SwitchableScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl VirusScanner for SwitchableScanner {
    fn scan<'a>(&'a self, _bytes: &'a [u8]) -> ScanFuture<'a> {
        let infected = self.infect.load(std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            if infected {
                Ok(ScanVerdict::Infected {
                    signature: "EICAR-Test-File".to_string(),
                })
            } else {
                Ok(ScanVerdict::Skipped)
            }
        })
    }
}

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 测试夹具。
pub struct Fixture {
    /// 业务服务（走**应用角色**连接，RLS 生效）。
    ///
    /// ⚠️ 用 `Arc` 共享：HTTP 层测试要把同一个实例装进 `StorageState`，
    /// 而 `StorageService` **不是 `Clone`**（它持有 `OnceLock` 审计槽，
    /// 克隆会得到两个独立的槽 → 装配了 sink 的那份可能不是被测的那份）。
    pub service: Arc<StorageService>,
    /// 配置源（可改）。
    pub config: StaticConfigSource,
    /// 扫描器（可切成感染）。
    pub scanner: SwitchableScanner,
    /// 管理员连接池（造数 / 断言 / 跨租造数）。
    pub admin: sqlx::PgPool,
    /// 本地存储根目录（用例结束时删除）。
    pub storage_root: PathBuf,
}

impl Fixture {
    /// `tenant-a` 的上下文。
    pub fn ctx_a(&self) -> RequestContext {
        RequestContext::new().with_tenant(TENANT_A)
    }

    /// `tenant-b` 的上下文。
    pub fn ctx_b(&self) -> RequestContext {
        RequestContext::new().with_tenant(TENANT_B)
    }

    /// 直接执行一条 SQL（造数 / 断言）。
    pub async fn exec(&self, sql: &str) {
        sqlx::raw_sql(sql)
            .execute(&self.admin)
            .await
            .unwrap_or_else(|err| panic!("SQL 失败（{sql}）：{err}"));
    }

    /// 查一个文本列。
    pub async fn query_text(&self, sql: &str) -> Vec<String> {
        let rows: Vec<(String,)> = sqlx::query_as(sql)
            .fetch_all(&self.admin)
            .await
            .unwrap_or_else(|err| panic!("查询失败（{sql}）：{err}"));
        rows.into_iter().map(|(v,)| v).collect()
    }

    /// 查一个 int 列。
    pub async fn query_i64(&self, sql: &str) -> i64 {
        let rows: Vec<(i64,)> = sqlx::query_as(sql)
            .fetch_all(&self.admin)
            .await
            .unwrap_or_else(|err| panic!("查询失败（{sql}）：{err}"));
        rows.first().map(|(v,)| *v).unwrap_or(0)
    }

    /// 存储根目录下的所有文件（相对路径）。
    pub fn list_stored_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        walk(&self.storage_root, &mut out);
        out
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.storage_root);
    }
}

/// 准备真实库环境 + 真临时目录 + 服务。
pub async fn setup() -> Fixture {
    let url = required_test_database_url();
    let admin_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));
    let root = repo_root();

    let mut conn = admin_pool.acquire().await.expect("应当能取到连接");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能拿到部署锁");

    // 1) 应用角色。
    let role_sql = read_file(&root.join("deploy/seed/02-app-role.sql"));
    sqlx::raw_sql(&role_sql)
        .execute(&mut *conn)
        .await
        .expect("应用角色准备失败");

    // 2) 迁移（含本需求的 3 组）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("迁移应能应用");

    // 3) RLS 策略（只看本需求的 4 张表）。
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    for scope in collect_scopes(&root).expect("作用域收集应成功") {
        if scope
            .spec
            .tables
            .iter()
            .any(|t| t.starts_with("platform_storage_"))
        {
            apply_policies(&admin_db, &scope.spec)
                .await
                .expect("storage 表的 RLS 策略应能应用");
        }
    }

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    // 4) 清表（用例之间不得互相污染）。
    // ⚠️ 新增租户表**必须**加进这个清单 —— 漏了会让上一条用例的数据
    // 被下一条用例读到（症状是「单独跑绿、连跑红」这种最难查的假失败）。
    for table in [
        "platform_storage_job_error",
        "platform_storage_job",
        "platform_storage_probe_row",
        "platform_storage_upload",
        "platform_storage_object",
    ] {
        sqlx::query(&format!("DELETE FROM {table}"))
            .execute(&admin_pool)
            .await
            .expect("清表应能执行");
    }

    // 4b) 重置 6 个配置键到默认值（防上一条用例改过的值污染）。
    let d = platform_storage::config::StorageSettings::default();
    for (key, value) in [
        ("storage.maxUploadBytes", d.max_upload_bytes),
        ("storage.partSizeBytes", d.part_size_bytes),
        ("storage.presignTtlSeconds", d.presign_ttl_seconds),
        ("storage.importBatchSize", d.import_batch_size),
        ("storage.maxRowBytes", d.max_row_bytes),
        (
            "storage.uploadSessionTtlSeconds",
            d.upload_session_ttl_seconds,
        ),
    ] {
        sqlx::query(&format!(
            "SELECT platform_config_upsert('{key}', '{value}'::jsonb)"
        ))
        .execute(&admin_pool)
        .await
        .expect("重置配置应能执行");
    }

    // 5) 应用角色连接（**不绕过 RLS**）。
    let app_db = TenantDb::connect(&app_role_url(&url), DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    // 6) 真临时目录。
    let storage_root =
        std::env::temp_dir().join(format!("platform-storage-it-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&storage_root).expect("应能建临时目录");

    let config = StaticConfigSource::new();
    let scanner = SwitchableScanner::new();
    let objects: Arc<dyn ObjectStore> = Arc::new(LocalFsStore::new(&storage_root));

    // 用 `LocalStorageEnv` 构造一次（**顺带验证它的 `from_lookup`**）。
    let env = LocalStorageEnv::from_lookup(|k| match k {
        platform_storage::config::STORAGE_ROOT_ENV => {
            Some(storage_root.to_string_lossy().to_string())
        }
        platform_storage::config::PRESIGN_SECRET_ENV => Some("test-presign-secret".to_string()),
        platform_storage::config::PRESIGN_KEY_ID_ENV => Some("k-test".to_string()),
        _ => None,
    })
    .expect("测试环境变量应齐");

    let service = Arc::new(StorageService::new(
        StorageStore::new(app_db),
        StorageConfig::new(Arc::new(config.clone())),
        objects,
        Arc::new(scanner.clone()),
        env.presign_secret.clone().into_bytes(),
        env.presign_key_id.clone(),
    ));

    Fixture {
        service,
        config,
        scanner,
        admin: admin_pool,
        storage_root,
    }
}

/// 让 `NoopScanner` 被引用（防 unused）。
pub fn noop_scanner() -> NoopScanner {
    NoopScanner
}
