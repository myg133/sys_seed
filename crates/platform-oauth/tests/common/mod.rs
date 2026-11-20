//! 出站 IdP 集成用例的**夹具**（REQ-022）。
//!
//! # 规矩
//!
//! - 只连 `base_test`（`base_dev` 是开发库，测试绝不能碰）；
//! - 缺 `DATABASE_URL_TEST` → **失败**，不是「跳过」（跳过也算通过 = 假绿）；
//! - 业务数据访问走**应用角色** `base_app`（超管会绕过 RLS → 租户隔离 fail open）；
//! - 造数走管理员连接（造数是夹具，不是业务访问）。
//!
//! # 登录令牌怎么来
//!
//! **直接用 IAM 的密钥集签一枚登录令牌**（不走 Argon2 登录）——
//! 低配机上 Argon2 是最慢的一步，而本轮要验的是 OAuth 侧的行为。
//! 登录流程本身由 `platform-iam` 的既有用例覆盖。

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use platform_core::RequestContext;
use platform_core::extension::audit::{AuditError, AuditEvent, AuditSink};
use platform_core::extension::expr::CompiledCondition;
use platform_core::extension::permission::{
    DataScope, PermissionChecker, PermissionProvider, PermissionRefresher, ScopeFilter,
};
use platform_db::{DbOptions, TenantDb, apply_policies, collect_scopes};
use platform_iam::config::{
    Argon2Settings, IamConfig, LoginSettings, PasswordPolicySettings, TokenSettings, TotpSettings,
};
use platform_iam::jwt::AccessClaims;
use platform_iam::service::IamService;
use platform_iam::store::IamStore;
use platform_oauth::config::{OAuthSettings, StaticOAuthConfigSource};
use platform_oauth::outbound::OutboundClient;
use platform_oauth::service::OAuthService;
use platform_oauth::store::OAuthStore;
use sqlx::postgres::PgPoolOptions;

/// 主测试租户。
pub const TENANT_A: &str = "tenant-a";
/// 跨租对照租户。
pub const TENANT_B: &str = "tenant-b";

/// 部署准备用的 advisory lock 键。
pub const SETUP_LOCK: i64 = 6_022_022;

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

/// 仓库根目录。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 读一个文件（缺失就 panic —— 夹具坏了要立刻暴露）。
fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()))
}

// ---------------------------------------------------------------------------
// 假权限源 / 假判定器
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

impl PermissionProvider for PermissionTable {
    fn effective_permissions<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        user_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, String>> + Send + 'a>>
    {
        let codes: Vec<String> = self.of(user_id).into_iter().collect();
        Box::pin(async move { Ok(codes) })
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

/// 测试用的**恒新鲜**刷新器。
///
/// 集成测试的权限判定器是内存 [`PermissionTable`]（构造时就把数据填好了），
/// 没有「epoch 变了要重算」这回事 → `ensure_fresh` 是 **no-op**。
///
/// ⚠️ **生产实现不是 no-op**：漏调 `ensure_fresh` 会让用户第一次打受保护的端点
/// 必然 403（`RbacChecker::has` 是纯缓存查询）—— 那是 TD-047 记下的真缺陷。
pub struct AlwaysFresh;

impl PermissionRefresher for AlwaysFresh {
    fn ensure_fresh<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>
    {
        Box::pin(async { Ok(()) })
    }
}

/// 记录事件的**假 sink**（不落库 —— 本用例只断言**埋点契约**）。
#[derive(Debug, Clone, Default)]
pub struct RecordingSink(Arc<Mutex<Vec<AuditEvent>>>);

impl RecordingSink {
    /// 全部记录。
    pub fn events(&self) -> Vec<AuditEvent> {
        self.0.lock().unwrap_or_else(|err| err.into_inner()).clone()
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
    /// 管理员连接池（造数用；**绕过 RLS**，只用于夹具）。
    pub admin_pool: sqlx::PgPool,
    /// 应用角色连接（**不绕过 RLS**；业务数据访问用这个）。
    pub app_db: TenantDb,
    /// IAM 服务（验登录令牌 + 签发密钥）。
    pub iam: Arc<IamService>,
    /// OAuth 服务。
    pub oauth: Arc<OAuthService>,
    /// 权限表（用户 → 权限点）。
    pub permissions: PermissionTable,
    /// 审计 sink（记录 `oauth.*` 事件）。
    pub sink: RecordingSink,
    /// 路由（与 `platform-server` 一样的挂法；**含** `/.well-known/jwks.json`）。
    pub router: axum::Router,
    /// 运行期配置源（可在用例里中途改 TTL 等 —— AC-S-145 步骤 4 要「已过期」令牌）。
    pub settings: Arc<StaticOAuthConfigSource>,
    /// IAM 的**私钥 PEM**（与平台同 `kid`）。
    ///
    /// ⚠️ 只给 AC-S-146 步骤 3 的「私钥泄露」判据用：测试侧**自己签**票据。
    /// 别的用例**不得**用它 —— 正常路径必须走平台签发。
    pub iam_private_pem: String,
    /// IAM 的 `kid`（自签票据要带同一个 `kid` 才会被平台验签命中）。
    pub iam_kid: String,
}

/// 测试用的 IAM 配置（**低配** Argon2 —— 本轮不验登录流程）。
///
/// `pem` / `kid` 由调用方给 —— 夹具要把它们**留在 [`Fixture`] 上**
/// （AC-S-146 步骤 3 的「私钥泄露」判据要用同一把私钥自签票据）。
fn test_iam_config(pem: String, kid: &str) -> IamConfig {
    IamConfig {
        jwt: platform_iam::config::JwtSettings {
            issuer: "base".to_string(),
            audience: "base-api".to_string(),
            private_key_pem: pem,
            kid: kid.to_string(),
            previous_private_key_pem: None,
            previous_kid: None,
        },
        tokens: TokenSettings {
            access_ttl: std::time::Duration::from_secs(900),
            refresh_ttl: std::time::Duration::from_secs(30 * 24 * 3600),
            reset_ttl: std::time::Duration::from_secs(3600),
        },
        argon2: Argon2Settings {
            memory_kib: 8192,
            iterations: 2,
            parallelism: 1,
        },
        password_policy: PasswordPolicySettings {
            min_length: 12,
            max_age_days: Some(180),
            history: 5,
        },
        login: LoginSettings {
            ip_limit: 20,
            ip_window: std::time::Duration::from_secs(300),
            account_failure_limit: 5,
            lockout_duration: std::time::Duration::from_secs(900),
        },
        totp: TotpSettings {
            interval: 30,
            digits: 6,
            tolerance_steps: 1,
            recovery_code_count: 10,
        },
        secret_key: [3u8; 32],
    }
}

/// 准备真实库环境 + 服务 + 路由（**默认配置**：`frontendBaseUrl` 空 → 回退 issuer）。
pub async fn setup() -> Fixture {
    setup_with(OAuthSettings::default()).await
}

/// 用**指定**的 OAuth 运行期配置准备夹具。
///
/// # 为什么需要它（AC-S-134 / AC-S-142 的区分力）
///
/// 默认配置里 `frontendBaseUrl` 是**空串** → `frontend_base()` 回退到 `{issuer}`
/// → `authorization_endpoint` 与 `token_endpoint` 的**前缀相同**
/// → 「代码用了 `{授权页基址}`」与「代码误用了 `{issuer}`」**判不出**（H-01）。
///
/// → 需要一条用例把 `frontendBaseUrl` 设成 **≠ issuer** 再断端点。
pub async fn setup_with(settings: OAuthSettings) -> Fixture {
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

    // 2) 平台迁移（含本需求的 3 组 OAuth 迁移）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    // 3) 模块建表。
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

    // 4) **平台按 manifest 的 tenantScoped 段统一生成 RLS 策略**（含本需求的 4 张表）。
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    let scopes = collect_scopes(&root).expect("租户作用域声明应能收集");
    for scope in &scopes {
        apply_policies(&admin_db, &scope.spec)
            .await
            .expect("RLS 策略生成应能应用");
    }

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    // 5) **清空 5 张 OAuth 表** —— 用例之间**不得**互相污染。
    //
    // ⚠️ 为什么必须清：client 注册表是**平台级**的（`client-a` / `client-b`）
    // → 上一条用例留下的 client / grant / token 会让下一条用例的断言
    // 「碰巧」成立或「碰巧」失败（这正是 REQ-014 的「假绿」教训）。
    //
    // ⚠️ 用**管理员连接**删（超管绕过 RLS）→ 5 张表一次删干净。
    for table in [
        "platform_oauth_token",
        "platform_oauth_device_code",
        "platform_oauth_code",
        "platform_oauth_grant",
        "platform_oauth_client",
    ] {
        sqlx::query(&format!("DELETE FROM {table}"))
            .execute(&admin_pool)
            .await
            .expect("清空 OAuth 表应能执行");
    }

    // 5) 应用角色连接（**不绕过 RLS** 的普通角色）。
    let app_url = app_role_url(&url);
    let app_db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    // 6) 服务。
    //
    // 私钥 PEM 与 kid **留在夹具上**：AC-S-146 步骤 3 的「私钥泄露」判据
    // 要用**同一把**私钥自签票据（否则签出来的票据 kid 不命中 → 验签失败 →
    // 判据退化成「票据无效」，就测不到「私钥泄露也拿不到越权令牌」）。
    let iam_private_pem = platform_iam::jwt::generate_key_pair().expect("测试密钥应当能生成");
    let iam_kid = "kid-oauth-test".to_string();
    let iam = Arc::new(
        IamService::new(
            IamStore::new(app_db.clone()),
            test_iam_config(iam_private_pem.clone(), &iam_kid),
        )
        .expect("IAM 服务应当能构造"),
    );
    let permissions = PermissionTable::new();
    let sink = RecordingSink::default();
    let config_source = Arc::new(StaticOAuthConfigSource::new(settings));
    let oauth = Arc::new(OAuthService::new(
        Arc::new(OAuthStore::new(app_db.clone())),
        iam.clone(),
        config_source.clone(),
        Arc::new(permissions.clone()),
        Arc::new(sink.clone()),
        Arc::new(OutboundClient::default()),
    ));

    // 7) 路由（与 `platform-server` 一样的挂法）。
    //
    // ⚠️ **必须**把 `/.well-known/jwks.json` 也挂上（AC-S-145 步骤 1/3）：
    // 第三方拿公钥独立验签的入口就是它。之前夹具**没有**它 →
    // 「取 JWKS 再验签」这条判据**结构上就写不出来**。
    let state = Arc::new(platform_oauth::http::OAuthState {
        service: oauth.clone(),
        checker: Arc::new(UserPermissionChecker {
            table: permissions.clone(),
        }),
        refresher: Arc::new(AlwaysFresh),
    });
    let iam_state = Arc::new(platform_iam::http::IamState {
        service: iam.clone(),
    });
    let router = axum::Router::new()
        .nest("/api/oauth", platform_oauth::http::router(state.clone()))
        .merge(platform_oauth::http::metadata_router(state))
        .merge(platform_iam::http::jwks_router(iam_state));

    Fixture {
        admin_pool,
        app_db,
        iam,
        oauth,
        permissions,
        sink,
        router,
        settings: config_source,
        iam_private_pem,
        iam_kid,
    }
}

/// 签一枚**登录令牌**（不走 Argon2 登录）。
pub fn login_token(fixture: &Fixture, tenant_id: &str, user_id: &str) -> String {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let claims = AccessClaims {
        subject: user_id.to_string(),
        tenant_id: tenant_id.to_string(),
        session_id: format!("session-{user_id}"),
        issuer: "base".to_string(),
        audience: "base-api".to_string(),
        issued_at: now,
        not_before: now,
        expires_at: now + 900,
        token_id: uuid::Uuid::new_v4().to_string(),
    };
    fixture
        .iam
        .keys()
        .sign(&claims)
        .expect("登录令牌应当能签发")
}

/// `Authorization: Bearer <token>`。
pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// `Authorization: Basic base64(client_id:client_secret)`。
pub fn basic(client_id: &str, client_secret: &str) -> String {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    format!(
        "Basic {}",
        BASE64.encode(format!("{client_id}:{client_secret}").as_bytes())
    )
}

/// 造一个 client（**平台级** → 管理员连接直插）。
pub async fn seed_client(
    fixture: &Fixture,
    client_id: &str,
    client_secret_hash: Option<&str>,
    client_type: &str,
    allowed_scopes: &[&str],
    allowed_grant_types: &[&str],
    backchannel: Option<&str>,
    frontchannel: Option<&str>,
) {
    let query = platform_db::TenantQuery::new(
        "INSERT INTO platform_oauth_client (client_id, client_secret_hash, client_type, name, \
         redirect_uris, allowed_grant_types, allowed_scopes, backchannel_logout_uri, \
         frontchannel_logout_uri, status, created_at) \
         SELECT ?, ?, ?, ?, ARRAY['https://client.example/cb'], ?, ?, ?, ?, 'active', now() \
         WHERE {tenant} IS NOT NULL ON CONFLICT (client_id) DO UPDATE \
         SET client_secret_hash = EXCLUDED.client_secret_hash, \
             client_type = EXCLUDED.client_type, name = EXCLUDED.name, \
             redirect_uris = EXCLUDED.redirect_uris, \
             allowed_grant_types = EXCLUDED.allowed_grant_types, \
             allowed_scopes = EXCLUDED.allowed_scopes, \
             backchannel_logout_uri = EXCLUDED.backchannel_logout_uri, \
             frontchannel_logout_uri = EXCLUDED.frontchannel_logout_uri, \
             status = 'active'",
    )
    .bind(client_id.to_string())
    .bind(client_secret_hash.map(str::to_string))
    .bind(client_type.to_string())
    .bind(format!("{client_id} 的展示名"))
    .bind(
        allowed_grant_types
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>(),
    )
    .bind(
        allowed_scopes
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>(),
    )
    .bind(backchannel.map(str::to_string))
    .bind(frontchannel.map(str::to_string));
    let ctx = RequestContext::new().with_tenant(platform_db::fx::PLATFORM_SCOPE_TENANT);
    fixture
        .app_db
        .execute(&ctx, &query)
        .await
        .expect("client 夹具应能插入");
}

/// 造一条 grant（同意记录）。
pub async fn seed_grant(
    fixture: &Fixture,
    tenant_id: &str,
    client_id: &str,
    user_id: &str,
    scopes: &[&str],
) {
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let query = platform_db::TenantQuery::new(
        "INSERT INTO platform_oauth_grant (tenant_id, id, client_id, user_id, scopes, status, \
         granted_at, consent_at) VALUES ({tenant}, ?, ?, ?, ?, 'active', now(), now()) \
         ON CONFLICT (tenant_id, client_id, user_id) DO UPDATE SET scopes = EXCLUDED.scopes, \
         status = 'active', revoked_at = NULL",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(client_id.to_string())
    .bind(user_id.to_string())
    .bind(
        scopes
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>(),
    );
    fixture
        .app_db
        .execute(&ctx, &query)
        .await
        .expect("grant 夹具应能插入");
}

/// 造一条 OAuth 令牌状态行（`active`）。
pub async fn seed_token(
    fixture: &Fixture,
    tenant_id: &str,
    client_id: &str,
    user_id: Option<&str>,
    token_id: &str,
    grant_id: Option<&str>,
    scopes: &[&str],
) {
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let query = platform_db::TenantQuery::new(
        "INSERT INTO platform_oauth_token (tenant_id, id, client_id, user_id, token_id, scopes, \
         grant_id, status, issued_at, expires_at) \
         VALUES ({tenant}, ?, ?, ?, ?, ?, ?, 'active', now(), now() + interval '3600 seconds')",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(client_id.to_string())
    .bind(user_id.map(str::to_string))
    .bind(token_id.to_string())
    .bind(
        scopes
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>(),
    )
    .bind(grant_id.map(str::to_string));
    fixture
        .app_db
        .execute(&ctx, &query)
        .await
        .expect("令牌夹具应能插入");
}
