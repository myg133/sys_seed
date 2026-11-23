//! REQ-015 真实库集成用例的**夹具**（真库 + 真服务层，design §9）。
//!
//! # 规矩（沿用 REQ-016 的 `tests/common/mod.rs`）
//!
//! - 只连 `base_test`（`base_dev` 是开发库，测试绝不能碰）；
//! - 缺 `DATABASE_URL_TEST` → **失败**，不是「跳过」（跳过也算通过 = 假绿）；
//! - 业务数据访问走**应用角色** `base_app`（超管会绕过 RLS → 租户隔离 fail open）；
//! - 造数 / 篡改走管理员连接（造数是夹具，不是业务访问）。
//!
//! # 为什么必须有真库用例
//!
//! 以下判据**单测写不出来**，只有连真库才能验：
//!
//! | AC | 判据 | 为什么必须真库 |
//! |---|---|---|
//! | 023 | 重投只重置死信那一条，已成功的**不重复投递** | 要真实的 `rows_affected` 与状态机 |
//! | 168 | 三级回退**取对了哪一行** | 要真实的 `UNIQUE (tenant, code, channel, locale)` |
//! | 170 | 并发 4 个 `tick()` **不重复不遗漏** | 要 `FOR UPDATE SKIP LOCKED` 的真实行锁 |
//! | 172 | 并发重投**恰好 1 成功 1 失败** | 同上：要真并发的 `rows_affected` |
//! | 173 | 3 张表的 RLS 真的挡住跨租户 | 要应用角色 + 真 policy |

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use platform_core::RequestContext;
use platform_core::extension::expr::CompiledCondition;
use platform_core::extension::permission::{
    DataScope, PermissionChecker, PermissionRefresher, ScopeFilter,
};
use platform_db::{DbOptions, TenantDb, apply_policies, collect_scopes};
use sqlx::postgres::PgPoolOptions;

use platform_notify::config::NotifyConfig;
use platform_notify::model::channel;
use platform_notify::service::NotifyService;
use platform_notify::store::NotifyStore;

/// 主测试租户。
pub const TENANT_A: &str = "tenant-a";
/// 跨租对照租户。
pub const TENANT_B: &str = "tenant-b";

/// 部署准备用的 advisory lock 键（与其它 crate 的夹具**不同**，避免互相阻塞）。
pub const SETUP_LOCK: i64 = 6_016_017;

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
    assert_eq!(db, "base_test", "测试库必须恰好是 base_test，实际是 {db}");
    url
}

/// 把管理员连接串换成**应用角色**（不绕过 RLS）。
pub fn app_role_url(admin_url: &str) -> String {
    let (scheme, rest) = admin_url.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

// ---------------------------------------------------------------------------
// 权限点（恒新鲜刷新器，见 TD-047）
// ---------------------------------------------------------------------------

/// 用户 → 有效权限点集合。
#[derive(Debug, Clone, Default)]
pub struct PermissionTable {
    inner: Arc<Mutex<BTreeMap<String, BTreeSet<String>>>>,
}

impl PermissionTable {
    /// 空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 给某用户授一组权限点（**并集**）。
    pub fn grant(&self, user_id: &str, permissions: &[&str]) {
        let mut guard = self.inner.lock().expect("锁");
        let entry = guard.entry(user_id.to_string()).or_default();
        for permission in permissions {
            entry.insert((*permission).to_string());
        }
    }

    /// 某用户的权限点集合。
    pub fn of(&self, user_id: &str) -> BTreeSet<String> {
        self.inner
            .lock()
            .expect("锁")
            .get(user_id)
            .cloned()
            .unwrap_or_default()
    }
}

/// 内存权限判定器。
pub struct UserPermissionChecker {
    /// 权限表。
    pub table: PermissionTable,
}

impl PermissionChecker for UserPermissionChecker {
    fn has(&self, ctx: &RequestContext, permission: &str) -> bool {
        let Some(user_id) = ctx.user_id() else {
            return false;
        };
        self.table.of(user_id).contains(permission)
    }

    fn scope(&self, _ctx: &RequestContext, _scope: DataScope) -> ScopeFilter {
        ScopeFilter::All
    }

    fn effective_scope(&self, _ctx: &RequestContext, _permission: &str) -> ScopeFilter {
        ScopeFilter::All
    }

    fn condition(&self, _ctx: &RequestContext, _permission: &str) -> Option<CompiledCondition> {
        None
    }
}

/// 恒新鲜刷新器（内存权限表没有 epoch 这回事）。
///
/// ⚠️ **生产实现不是 no-op** —— 见 TD-047。
pub struct AlwaysFresh;

impl PermissionRefresher for AlwaysFresh {
    fn ensure_fresh<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

// ---------------------------------------------------------------------------
// 配置源（固定值，可改）
// ---------------------------------------------------------------------------

/// 可变的配置源（用例可临时改退避参数 —— 这是 AC-S-171 步骤 4 的判据）。
#[derive(Debug, Clone)]
pub struct StaticConfigSource {
    inner: Arc<Mutex<BTreeMap<String, i64>>>,
}

impl StaticConfigSource {
    /// 用默认值构造。
    pub fn new() -> Self {
        let mut map = BTreeMap::new();
        map.insert(
            platform_notify::config::CONFIG_KEY_MAX_ATTEMPTS.to_string(),
            i64::from(platform_notify::config::defaults::MAX_ATTEMPTS),
        );
        map.insert(
            platform_notify::config::CONFIG_KEY_RETRY_BASE_SECONDS.to_string(),
            platform_notify::config::defaults::RETRY_BASE_SECONDS,
        );
        map.insert(
            platform_notify::config::CONFIG_KEY_RETRY_MULTIPLIER.to_string(),
            platform_notify::config::defaults::RETRY_MULTIPLIER,
        );
        map.insert(
            platform_notify::config::CONFIG_KEY_RETRY_MAX_SECONDS.to_string(),
            platform_notify::config::defaults::RETRY_MAX_SECONDS,
        );
        map.insert(
            platform_notify::config::CONFIG_KEY_DELIVERING_TIMEOUT_SECONDS.to_string(),
            platform_notify::config::defaults::DELIVERING_TIMEOUT_SECONDS,
        );
        Self {
            inner: Arc::new(Mutex::new(map)),
        }
    }

    /// 改一个键（用例用）。
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

impl platform_notify::config::NotifyConfigSource for StaticConfigSource {
    fn read_platform<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        key: &'a str,
    ) -> platform_notify::config::NotifyBoxFuture<'a, Option<i64>> {
        let value = self.inner.lock().expect("锁").get(key).copied();
        Box::pin(async move { value })
    }

    fn tenant_default_locale(
        &self,
        _ctx: &RequestContext,
    ) -> platform_notify::config::NotifyBoxFuture<'_, String> {
        Box::pin(async { "zh-CN".to_string() })
    }
}

// ---------------------------------------------------------------------------
// 计数型通道（**AC-S-023 的关键判据来源**）
// ---------------------------------------------------------------------------

use platform_core::extension::notification::{
    DeliveryResult, NotificationMessage, NotificationSender,
};

/// 记录每次投递并计数的假通道。
///
/// ⚠️ **为什么不能只看库里的状态**：
/// 「不重复投递已成功的」如果只看 `attempt_count`，
/// 一个「发了两次但状态只记一次」的实现也会绿 —— 所以必须数**调用次数**。
#[derive(Debug, Clone, Default)]
pub struct CountingSender {
    calls: Arc<Mutex<Vec<(String, String)>>>,
}

impl CountingSender {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 已被调用的次数。
    pub fn call_count(&self) -> usize {
        self.calls.lock().expect("锁").len()
    }

    /// 已投递的 `(title, body)` 列表。
    pub fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().expect("锁").clone()
    }

    /// 清空计数（用例分段断言用）。
    pub fn reset(&self) {
        self.calls.lock().expect("锁").clear();
    }
}

impl NotificationSender for CountingSender {
    fn channel(&self) -> &str {
        channel::IN_APP
    }

    fn send(
        &self,
        _ctx: &RequestContext,
        msg: &NotificationMessage,
    ) -> impl std::future::Future<Output = anyhow::Result<DeliveryResult>> + Send {
        let title = msg.title.clone();
        let body = msg.body.clone();
        let calls = self.calls.clone();
        async move {
            calls.lock().expect("锁").push((title, body));
            Ok(DeliveryResult::delivered(format!(
                "counted-{}",
                calls.lock().expect("锁").len()
            )))
        }
    }
}

/// 恒定失败的假通道（`AC-S-171` 的退避判据来源）。
#[derive(Debug, Clone)]
pub struct FailingSender {
    /// 失败原因。
    pub reason: Arc<Mutex<String>>,
}

impl FailingSender {
    /// 新建。
    pub fn new(reason: &str) -> Self {
        Self {
            reason: Arc::new(Mutex::new(reason.to_string())),
        }
    }
}

impl NotificationSender for FailingSender {
    fn channel(&self) -> &str {
        channel::IN_APP
    }

    fn send(
        &self,
        _ctx: &RequestContext,
        _msg: &NotificationMessage,
    ) -> impl std::future::Future<Output = anyhow::Result<DeliveryResult>> + Send {
        let reason = self.reason.clone();
        async move {
            let text = reason.lock().expect("锁").clone();
            Ok(DeliveryResult::failed(text))
        }
    }
}

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 只操作用例自己的数据（造数 / 篡改 / 断言用；**管理员**连接）。
pub struct AdminConn {
    /// 管理员连接池。
    pub pool: sqlx::PgPool,
}

impl AdminConn {
    /// 查**单文本列**（断言用；SQL 里必须显式 `::text`）。
    ///
    /// ⚠️ 用 `String` 而不是 `serde_json::Value`：后者在 sqlx 里绑的是 `JSONB`，
    /// 与 `TEXT` 类型不兼容（会报 `mismatched types ... SQL type TEXT`）。
    pub async fn query_text(&self, sql: &str) -> Vec<String> {
        let rows: Vec<(String,)> = sqlx::query_as(sql)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_else(|err| panic!("造数/断言 SQL 失败（{sql}）：{err}"));
        rows.into_iter().map(|(value,)| value).collect()
    }

    /// 执行一条 DML（造数 / 强制改状态）。
    pub async fn execute(&self, sql: &str) {
        sqlx::raw_sql(sql)
            .execute(&self.pool)
            .await
            .unwrap_or_else(|err| panic!("造数/篡改 SQL 失败（{sql}）：{err}"));
    }
}

/// 测试夹具。
pub struct Fixture {
    /// 业务服务（走**应用角色**连接，RLS 生效）。
    ///
    /// ⚠️ 是 `Arc` 而不是裸值：REQ-020 的落库适配器
    /// （`platform_notify::db_sender::DbNotificationSender`）按装配层的真实形态
    /// 持有 `Arc<NotifyService>` —— 夹具与生产同一形状，测试才测得到真实用法。
    pub service: Arc<NotifyService>,
    /// 配置源（可改退避参数）。
    pub config: StaticConfigSource,
    /// 权限表（可临时加减权限点）。
    pub permissions: PermissionTable,
    /// 管理员连接（造数 / 断言 / 强制改状态）。
    pub admin: AdminConn,
}

impl Fixture {
    /// `tenant-a` 的请求上下文。
    pub fn ctx_a(&self) -> RequestContext {
        RequestContext::new().with_tenant(TENANT_A)
    }

    /// `tenant-b` 的请求上下文。
    pub fn ctx_b(&self) -> RequestContext {
        RequestContext::new().with_tenant(TENANT_B)
    }

    /// `tenant-a` 下某用户的上下文。
    pub fn ctx_user(&self, user_id: &str) -> RequestContext {
        RequestContext::new()
            .with_tenant(TENANT_A)
            .with_user(user_id)
    }
}

/// 准备真实库环境 + 服务。
pub async fn setup() -> Fixture {
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

    // 2) 平台迁移（含本需求的 3 组 notify 迁移）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    // 3) 按 manifest 的 tenantScoped 段统一生成 RLS 策略（只看本需求的 3 张表）。
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    for scope in collect_scopes(&root).expect("租户作用域声明应能收集") {
        if scope
            .spec
            .tables
            .iter()
            .any(|table| table.starts_with("platform_notify_"))
        {
            apply_policies(&admin_db, &scope.spec)
                .await
                .expect("notify 表的 RLS 策略应能应用");
        }
    }

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    // 4) **清空本需求的 3 张表**（用例之间不得互相污染）。
    for table in [
        "platform_notify_delivery",
        "platform_notify_preference",
        "platform_notify_template",
    ] {
        sqlx::query(&format!("DELETE FROM {table}"))
            .execute(&admin_pool)
            .await
            .expect("清空表应能执行");
    }

    // 4b) **重置 5 个配置键到种子默认值** —— 否则上一条用例改过的
    //     `notify.maxAttempts` 会污染下一条（顺序相关 = 假绿）。
    for (key, value) in [
        ("notify.maxAttempts", "5"),
        ("notify.retryBaseSeconds", "30"),
        ("notify.retryMultiplier", "2"),
        ("notify.retryMaxSeconds", "3600"),
        ("notify.deliveringTimeoutSeconds", "300"),
    ] {
        sqlx::query(&format!(
            "SELECT platform_config_upsert('{key}', '{value}'::jsonb)"
        ))
        .execute(&admin_pool)
        .await
        .expect("重置配置应能执行");
    }

    // 5) 应用角色连接（**不绕过 RLS**）。
    let app_url = app_role_url(&url);
    let app_db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    let config = StaticConfigSource::new();
    let service = Arc::new(NotifyService::new(
        NotifyStore::new(app_db),
        NotifyConfig::new(Arc::new(config.clone())),
    ));

    Fixture {
        service,
        config,
        permissions: PermissionTable::new(),
        admin: AdminConn { pool: admin_pool },
    }
}

// ---------------------------------------------------------------------------
// 造数辅助
// ---------------------------------------------------------------------------

/// 建一个模板（直接走 `service`，**不绕过业务规则**）。
pub async fn seed_template(
    fixture: &Fixture,
    ctx: &RequestContext,
    code: &str,
    channel_name: &str,
    locale: &str,
    category: &str,
) -> platform_notify::model::TemplateRow {
    let new = platform_notify::store::NewTemplate {
        id: uuid::Uuid::new_v4().to_string(),
        code: code.to_string(),
        channel: channel_name.to_string(),
        locale: locale.to_string(),
        category: category.to_string(),
        title_template: "标题 {{name}}".to_string(),
        body_template: "正文 {{name}}".to_string(),
    };
    fixture
        .service
        .create_template(ctx, &new)
        .await
        .unwrap_or_else(|err| panic!("建模板失败（{code}/{channel_name}/{locale}）：{err}"))
}

/// 造一条投递记录并返回其 ID（**直接写库**，用于精确控制初始状态）。
pub async fn seed_delivery(
    fixture: &Fixture,
    tenant: &str,
    status: &str,
    attempt_count: i32,
    next_attempt_at: &str,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    fixture
        .admin
        .execute(&format!(
            "INSERT INTO platform_notify_delivery \
               (id, tenant_id, template_code, channel, category, recipient, locale, payload, \
                rendered_title, rendered_body, status, attempt_count, next_attempt_at) \
             VALUES ('{id}', '{tenant}', 'code', 'in_app', 'transactional', 'u1', 'zh-CN', \
                     '{{}}'::jsonb, 't', 'b', '{status}', {attempt_count}, '{next_attempt_at}')"
        ))
        .await;
    id
}

/// 读一条投递记录的某个字段（造数断言用；**管理员**连接绕过 RLS）。
pub async fn delivery_field(fixture: &Fixture, id: &str, field: &str) -> Option<String> {
    let rows = fixture
        .admin
        .query_text(&format!(
            "SELECT coalesce(({field})::text, '<null>') FROM platform_notify_delivery WHERE id = '{id}'"
        ))
        .await;
    match rows.first().map(String::as_str) {
        Some("<null>") | None => None,
        Some(text) => Some(text.to_string()),
    }
}
