//! REQ-016 集成用例的**夹具**（真库 + 真 HTTP，design §9）。
//!
//! # 规矩
//!
//! - 只连 `base_test`（`base_dev` 是开发库，测试绝不能碰）；
//! - 缺 `DATABASE_URL_TEST` → **失败**，不是「跳过」（跳过也算通过 = 假绿）；
//! - 业务数据访问走**应用角色** `base_app`（超管会绕过 RLS → 租户隔离 fail open）；
//! - 造数 / 篡改走管理员连接（造数是夹具，不是业务访问）。
//!
//! # 登录令牌怎么来
//!
//! 本文件的用例**不跑登录流程**（低配机上 Argon2 是最慢的一步）——
//! 用 [`StaticTokenVerifier`] 把「令牌字符串」映射成固定的租户 + 用户
//! （见 `platform_app::tokens::StaticTokenVerifier`，**测试专用**）。
//! 真实登录令牌的验签由 `platform-iam` 的既有用例覆盖。
//!
//! # 权限点怎么来
//!
//! 用内存 [`PermissionTable`]（构造时把数据填好）→ `ensure_fresh` 是 **no-op**。
//! ⚠️ **生产实现不是 no-op**：`RbacChecker::has` 是纯缓存查询 —— 漏调
//! `ensure_fresh` 会让用户第一次打受保护的端点必然 403（TD-047）。

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use platform_core::RequestContext;
use platform_core::extension::audit::{AuditError, AuditEvent, AuditSink};
use platform_core::extension::expr::CompiledCondition;
use platform_core::extension::manifest::Manifest;
use platform_core::extension::permission::{
    DataScope, PermissionChecker, PermissionRefresher, ScopeFilter,
};
use platform_db::{DbOptions, TenantDb, apply_policies, collect_scopes};
use platform_oauth::crypto::digest_base64url;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

/// 主测试租户。
pub const TENANT_A: &str = "tenant-a";
/// 跨租对照租户。
pub const TENANT_B: &str = "tenant-b";

/// 部署准备用的 advisory lock 键（与其它 crate 的夹具**不同**，避免互相阻塞）。
pub const SETUP_LOCK: i64 = 6_016_016;

/// 仓库根目录。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()))
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

// ---------------------------------------------------------------------------
// 权限源 / 判定器
// ---------------------------------------------------------------------------

/// 用户 → 有效权限点集合（可变，便于用例临时加减）。
#[derive(Debug, Clone, Default)]
pub struct PermissionTable {
    inner: Arc<Mutex<BTreeMap<String, BTreeSet<String>>>>,
}

impl PermissionTable {
    /// 空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 给某个用户加权限点。
    pub fn grant(&self, user_id: &str, code: &str) {
        let mut guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        guard
            .entry(user_id.to_string())
            .or_default()
            .insert(code.to_string());
    }

    /// 某个用户的权限点集合。
    pub fn of(&self, user_id: &str) -> BTreeSet<String> {
        let guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        guard.get(user_id).cloned().unwrap_or_default()
    }
}

/// **按用户**判定的判定器（上下文里带用户 ID）。
#[derive(Debug, Clone, Default)]
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

/// 测试用的**恒新鲜**刷新器（内存权限表没有 epoch 这回事）。
///
/// ⚠️ **生产实现不是 no-op** —— 见模块注释与 TD-047。
pub struct AlwaysFresh;

impl PermissionRefresher for AlwaysFresh {
    fn ensure_fresh<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// **会先拒绝一次**的刷新器（TD-047 的**阴性对照**用）。
///
/// `ensure_fresh` 第一次返回 `Err` → 带权限点的端点第一次必须 **500**（fail closed）。
/// 用于证明「中间件真的调了 `ensure_fresh`」—— 删掉那次调用这条用例就变绿。
#[derive(Debug, Clone, Default)]
pub struct FailingOnceRefresher {
    calls: Arc<Mutex<usize>>,
}

impl FailingOnceRefresher {
    /// `ensure_fresh` 被调用的次数。
    pub fn calls(&self) -> usize {
        *self.calls.lock().unwrap_or_else(|err| err.into_inner())
    }
}

impl PermissionRefresher for FailingOnceRefresher {
    fn ensure_fresh<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        let calls = self.calls.clone();
        Box::pin(async move {
            let mut guard = calls.lock().unwrap_or_else(|err| err.into_inner());
            *guard += 1;
            Err("the permission cache is unavailable".to_string())
        })
    }
}

/// **令牌 → 主体**映射（测试专用）。
///
/// 格式：`<tenant>|<user>` —— 例：`tenant-a|u-perm-app`。
/// 解析不出来 → `AuthError`（未认证）。
#[derive(Debug, Clone, Copy, Default)]
pub struct MappedTokenVerifier;

impl platform_app::tokens::TokenVerifier for MappedTokenVerifier {
    fn verify(
        &self,
        token: &str,
    ) -> Result<platform_app::tokens::VerifiedToken, platform_app::tokens::AuthError> {
        let (tenant_id, user_id) = token
            .split_once('|')
            .ok_or(platform_app::tokens::AuthError)?;
        if tenant_id.trim().is_empty() || user_id.trim().is_empty() {
            return Err(platform_app::tokens::AuthError);
        }
        Ok(platform_app::tokens::VerifiedToken {
            user_id: user_id.trim().to_string(),
            tenant_id: tenant_id.trim().to_string(),
        })
    }
}

/// 记录事件的**假 sink**（本文件只断言**埋点契约**）。
///
/// ⚠️ **落库**的判据在 `app_real_stack.rs`（真 `PgAuditSink`）——
/// 这个假 sink 只能证明「事件被构造出来了」，不能证明「它真的落库了」（dev-guide §9.15）。
#[derive(Debug, Clone, Default)]
pub struct RecordingSink(Arc<Mutex<Vec<AuditEvent>>>);

impl RecordingSink {
    /// 全部记录。
    pub fn events(&self) -> Vec<AuditEvent> {
        self.0.lock().unwrap_or_else(|err| err.into_inner()).clone()
    }

    /// 某个动作的记录。
    pub fn with_action(&self, action: &str) -> Vec<AuditEvent> {
        self.events()
            .into_iter()
            .filter(|event| event.action == action)
            .collect()
    }
}

impl AuditSink for RecordingSink {
    fn record<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        event: &'a AuditEvent,
    ) -> platform_core::extension::audit::BoxFuture<'a, Result<(), AuditError>> {
        self.0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(event.clone());
        Box::pin(async { Ok(()) })
    }
}

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 测试夹具。
pub struct Fixture {
    /// 管理员连接池（造数 / 篡改用；**绕过 RLS**）。
    pub admin_pool: sqlx::PgPool,
    /// 应用角色连接（**不绕过 RLS**；业务数据访问走它）。
    pub app_db: TenantDb,
    /// 权限表。
    pub permissions: PermissionTable,
    /// 审计 sink（记录 `app.*` 事件）。
    pub sink: RecordingSink,
    /// 运行期配置源（**可变** —— 用例可以模拟「配置改了要生效」）。
    ///
    /// ⚠️ 生产实现是「每请求读库」的 `PlatformAppConfigSource`；
    /// 这里用内存源是因为集成用例不该依赖 `platform-config` 的缓存路径 ——
    /// 那条路径由 `platform-server` 的 `app_real_stack.rs` 用真实 `ConfigService` 覆盖。
    pub settings: Arc<platform_app::config::StaticAppConfigSource>,
    /// 路由（与 `platform-server` 一样的挂法）。
    pub router: axum::Router,
}

/// 默认的 `menus[]`（导航用例的 `source="module"` 那一半）。
///
/// 三条：
/// - `审计日志` + `perm="audit:events:read"`（**有权限才下发**的判据）；
/// - `工作台` + **无 `perm`**（`perm` 缺省 = 登录即可见，AC-S-162 步骤 3）；
/// - `OAuth 授权` + `perm="oauth:grant:read"`（**第二条**带 perm 的对照）。
pub fn default_manifests() -> Vec<Manifest> {
    let raw = r#"{
      "id": "probe",
      "name": "探针模块",
      "version": "0.1.0",
      "enabled": true,
      "permissions": [],
      "menus": [
        { "label": "审计日志", "icon": "shield", "route": "/admin/audit", "perm": "audit:events:read" },
        { "label": "工作台", "icon": "grid", "route": "/workbench" },
        { "label": "OAuth 授权", "icon": "key", "route": "/oauth/grants", "perm": "oauth:grant:read" }
      ],
      "dependencies": []
    }"#;
    vec![Manifest::parse_checked(raw).expect("probe manifest 应合法")]
}

/// 准备真实库环境 + 服务 + 路由。
pub async fn setup() -> Fixture {
    setup_with_manifests(default_manifests()).await
}

/// 用**指定**的 manifest 集合准备夹具（导航用例需要）。
pub async fn setup_with_manifests(manifests: Vec<Manifest>) -> Fixture {
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

    // 2) 平台迁移（含本需求的 3 组 app 迁移）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    // 3) 平台按 manifest 的 tenantScoped 段统一生成 RLS 策略（只看本需求的 2 张表）。
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    for scope in collect_scopes(&root).expect("租户作用域声明应能收集") {
        if scope
            .spec
            .tables
            .iter()
            .any(|table| table.starts_with("platform_app"))
        {
            apply_policies(&admin_db, &scope.spec)
                .await
                .expect("app 表的 RLS 策略应能应用");
        }
    }

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    // 4) **清空本需求的 3 张表**（用例之间不得互相污染）。
    //
    // ⚠️ 必须先删 `platform_app_access` / `platform_app`（它们引用 client），
    // 再删 client —— 否则外键会挡住。
    for table in [
        "platform_app_access",
        "platform_app",
        "platform_oauth_client",
    ] {
        sqlx::query(&format!("DELETE FROM {table}"))
            .execute(&admin_pool)
            .await
            .expect("清空表应能执行");
    }

    // 4b) **重置 3 个配置键到种子默认值** —— 否则上一条用例改过的
    //     `app.workspace.maxApps` / `app.recentCount` 会污染下一条（顺序相关 = 假绿）。
    for (key, value) in [
        ("app.recentCount", "6"),
        ("app.workspace.maxApps", "60"),
        ("app.icon.allowHttp", "false"),
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

    // 6) 服务 + 路由。
    let permissions = PermissionTable::new();
    let sink = RecordingSink::default();
    let settings = Arc::new(platform_app::config::StaticAppConfigSource::default());
    let store = Arc::new(platform_app::store::AppStore::new(app_db.clone()));
    let service = Arc::new(platform_app::service::AppService::new(
        store,
        settings.clone(),
        Arc::new(sink.clone()),
        Arc::new(manifests),
        Arc::new(UserPermissionChecker {
            table: permissions.clone(),
        }),
        Arc::new(MappedTokenVerifier),
    ));
    let state = Arc::new(platform_app::http::AppState {
        service,
        checker: Arc::new(UserPermissionChecker {
            table: permissions.clone(),
        }),
        refresher: Arc::new(AlwaysFresh),
    });
    let router = axum::Router::new().nest("/api/apps", platform_app::http::router(state));

    Fixture {
        admin_pool,
        app_db,
        permissions,
        sink,
        settings,
        router,
    }
}

impl Fixture {
    /// 造一个 client（**平台级** → 管理员连接直插）。
    pub async fn seed_client(
        &self,
        client_id: &str,
        status: &str,
        allowed_scopes: &[&str],
        expires_in_seconds: Option<i64>,
    ) {
        let hash = digest_base64url(b"test-secret");
        let expires_clause = match expires_in_seconds {
            Some(seconds) => format!("now() + interval '{seconds} seconds'"),
            None => "NULL".to_string(),
        };
        sqlx::query(&format!(
            "INSERT INTO platform_oauth_client \
             (client_id, client_secret_hash, client_type, name, redirect_uris, \
              allowed_grant_types, allowed_scopes, status, expires_at, created_at) \
             VALUES ($1, $2, 'confidential', $3, ARRAY['https://client.example/cb'], \
                     ARRAY['authorization_code'], $4, $5, {expires_clause}, now()) \
             ON CONFLICT (client_id) DO UPDATE SET status = EXCLUDED.status, \
                 allowed_scopes = EXCLUDED.allowed_scopes, expires_at = EXCLUDED.expires_at"
        ))
        .bind(client_id)
        .bind(&hash)
        .bind(format!("{client_id} 的 client 名"))
        .bind(
            allowed_scopes
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>(),
        )
        .bind(status)
        .execute(&self.admin_pool)
        .await
        .expect("client 夹具应能插入");
    }

    /// 给某用户授指定权限点。
    pub fn grant(&self, user_id: &str, permissions: &[&str]) {
        for permission in permissions {
            self.permissions.grant(user_id, permission);
        }
    }

    /// 走一次 HTTP 请求，返回 `(状态码, JSON 体)`。
    pub async fn call(
        &self,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        let body = match body {
            Some(value) => {
                builder = builder.header("content-type", "application/json");
                Body::from(serde_json::to_vec(&value).expect("请求体应能序列化"))
            }
            None => Body::empty(),
        };
        let request = builder.body(body).expect("应当能构造请求");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("应当能跑完路由");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("应当能读出响应体");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    /// 管理员直连查询单值（`i64`）。
    pub async fn scalar(&self, sql: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(&self.admin_pool)
            .await
            .unwrap_or_else(|err| panic!("查询失败（{sql}）：{err}"))
    }

    /// 管理员直连执行一条语句。
    pub async fn execute(&self, sql: &str) {
        sqlx::query(sql)
            .execute(&self.admin_pool)
            .await
            .unwrap_or_else(|err| panic!("执行失败（{sql}）：{err}"));
    }

    /// 某租户 + 某展示名对应的 `app_id`（造完应用后回查用）。
    pub async fn app_id_by_display_name(&self, tenant: &str, display_name: &str) -> String {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM platform_app WHERE tenant_id = $1 AND display_name = $2",
        )
        .bind(tenant)
        .bind(display_name)
        .fetch_one(&self.admin_pool)
        .await
        .unwrap_or_else(|err| panic!("查 app_id 失败：{err}"))
    }

    /// 以管理员身份**造一条应用行**（不走 HTTP —— 给「只需要夹具」的用例用）。
    #[allow(clippy::too_many_arguments)]
    pub async fn seed_app(
        &self,
        id: &str,
        tenant: &str,
        client_id: &str,
        display_name: &str,
        category: &str,
        sort_order: i32,
        status: &str,
        launch_url: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO platform_app \
             (id, tenant_id, client_id, display_name, description, category, icon_url, \
              launch_url, sort_order, status, created_at, created_by) \
             VALUES ($1, $2, $3, $4, NULL, $5, NULL, $6, $7, $8, now(), 'fixture') \
             ON CONFLICT (id) DO UPDATE SET client_id = EXCLUDED.client_id, \
                 display_name = EXCLUDED.display_name, category = EXCLUDED.category, \
                 sort_order = EXCLUDED.sort_order, status = EXCLUDED.status, \
                 launch_url = EXCLUDED.launch_url",
        )
        .bind(id)
        .bind(tenant)
        .bind(client_id)
        .bind(display_name)
        .bind(category)
        .bind(launch_url)
        .bind(sort_order)
        .bind(status)
        .execute(&self.admin_pool)
        .await
        .expect("应用夹具应能插入");
    }

    /// 造一条访问记录（给「最近 / 常用」用例准备起点）。
    pub async fn seed_access(
        &self,
        tenant: &str,
        user_id: &str,
        client_id: &str,
        access_count: i64,
        seconds_ago: i64,
    ) {
        sqlx::query(&format!(
            "INSERT INTO platform_app_access \
             (tenant_id, user_id, client_id, last_accessed_at, access_count) \
             VALUES ($1, $2, $3, now() - interval '{seconds_ago} seconds', $4) \
             ON CONFLICT (tenant_id, user_id, client_id) DO UPDATE \
             SET last_accessed_at = EXCLUDED.last_accessed_at, \
                 access_count = EXCLUDED.access_count"
        ))
        .bind(tenant)
        .bind(user_id)
        .bind(client_id)
        .bind(access_count)
        .execute(&self.admin_pool)
        .await
        .expect("访问记录夹具应能插入");
    }
}

/// `Authorization: Bearer <token>` 里的令牌串（测试专用：`<tenant>|<user>`）。
pub fn token_for(tenant: &str, user_id: &str) -> String {
    format!("{tenant}|{user_id}")
}
