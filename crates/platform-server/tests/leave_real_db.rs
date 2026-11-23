//! `modules/leave` 的**装配级**真库用例（REQ-020 第二批 ④⑤ + 第三批 ①②）。
//!
//! # 这里验的是「服务真的能加载这个模块」，不是「service 函数能跑」
//!
//! 第一批的 `state_machine.rs` 只测纯函数；本文件走**真实路径**：
//!
//! ```text
//! 真库（app 角色，RLS 生效）
//!   → leave::connect → with_audit(真 PgAuditSink) → with_notifier(真 DbNotificationSender)
//!     → leave::http::router（与 main.rs 的 `"leave"` 臂**同一套**装配）
//!   → 认证层（验真 JWT → 建上下文 → 注入真 RbacChecker）
//!   → 模块的 route_layer(require_permission(...))
//!   → handler → repository
//! ```
//!
//! ⚠️ 第二批走的是 `leave::mount`（**不透传注入**）→ 第三批改成显式注入：
//! 否则 `/api/leave/*` 不写审计、不发通知，计数表（⑦）无从判起。
//!
//! | # | 判据 | 删掉它，哪条 AC 判不了 |
//! |---|---|---|
//! | ① | `POST /api/leave/requests` → 201 且**独立查询**能查到 | AC-S-190/192 的前置（模块能加载） |
//! | ② | `GET` 列表能查到刚建的单 | 同上 |
//! | ③ | 不带令牌 → 401 | AC-S-188 ③（鉴权复用） |
//! | ④ | 无 `leave:request:write` → 403 | 同上（且是 C-5 的运行期对照） |
//! | ⑤ | 租户 A 建的单，租户 B 的列表**看不到** | AC-S-188 ①（租户隔离） |
//! | ⑥ | 非法流转 409 + `LEAVE_INVALID_TRANSITION` 且 `title` 不是兜底 `"Error"` | AC-S-189 + 本批的错误码注册（①） |
//! | ⑦ | **计数表**：建单/提交/通过/驳回/非法流转 的审计与通知条数逐行对上（查真表） | AC-S-189（第三批 ①②）；退化成 `leave::mount` 装配 → 全红 |
//!
//! # ⚠️ 一处**如实登记的局限**：认证中间件是**复刻**的
//!
//! `authenticate_module` 是 `crates/platform-server/src/main.rs`（**bin**）里的
//! 私有函数 —— `tests/` 是独立 crate，**调不到**它（编译不过，不是不想用）。
//! → 本文件的 [`authenticate`] 是它的**逐行复刻**（同一组平台 API：
//! `IamService::verify_access_token` + `RequestContext` + `RbacChecker` +
//! `ensure_fresh`），并由 `main.rs` 的 `"leave"` 臂本身保证生产接线一致。
//!
//! 根本解法是 TD-076 的后续：把 `build_router` / 中间件从 `main` 抽到 lib。
//! 本轮不改（平台改动只做登记过的那几项）。
//!
//! 跑法：
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-server --test leave_real_db -- --test-threads=1 --include-ignored
//! ```

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use axum::Router;
use platform_core::RequestContext;
use platform_core::extension::permission::{PermissionChecker, PermissionRefresher};
use platform_db::{DbOptions, TenantDb};
use platform_iam::config::{
    Argon2Settings, IamConfig, JwtSettings, LoginSettings, PasswordPolicySettings, TokenSettings,
    TotpSettings,
};
use platform_iam::http::IamState;
use platform_iam::jwt::AccessClaims;
use platform_iam::service::IamService;
use platform_iam::store::IamStore;
use platform_notify::config::{NotifyBoxFuture, NotifyConfig, NotifyConfigSource};
use platform_notify::model::{category, channel};
use platform_notify::service::NotifyService;
use platform_notify::store::{NewTemplate, NotifyStore};
use platform_rbac::entity::Role;
use platform_rbac::{RbacChecker, RbacService, RbacStore, ScopePolicy};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

use support::{repo_root, setup_database, unique_tenant};

/// 密码（满足平台策略）。
const PASSWORD: &str = "correct-horse-battery-staple";

/// 建表 / 应用 policy 的 advisory lock（与其它 crate 的夹具**不同**，避免互等）。
const SETUP_LOCK: i64 = 6_020_020;

/// 测试用 IAM 配置（与 `app_real_stack.rs` 同款 —— 只有密钥不同）。
fn test_config(pem: String) -> IamConfig {
    IamConfig {
        jwt: JwtSettings {
            issuer: "base".to_string(),
            audience: "base-api".to_string(),
            private_key_pem: pem,
            kid: "kid-leave".to_string(),
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
            ip_limit: 100,
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
        secret_key: [11u8; 32],
    }
}

/// `authenticate_module` 的复刻状态（见文件头「局限」）。
#[derive(Clone)]
struct AuthState {
    iam: Option<Arc<IamService>>,
    checker: Arc<dyn PermissionChecker>,
    permissions: Arc<dyn PermissionRefresher>,
}

/// `authenticate_module` 的**逐行复刻**。
async fn authenticate(State(auth): State<AuthState>, mut request: Request, next: Next) -> Response {
    let instance = request
        .extensions()
        .get::<axum::extract::OriginalUri>()
        .map(|uri| uri.0.path().to_string())
        .unwrap_or_else(|| request.uri().path().to_string());
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    let Some(iam) = &auth.iam else {
        return platform_core::problem::problem_response(
            &platform_core::problem::ProblemDetails::unauthenticated(&ctx, &instance),
        );
    };
    let Some(token) = bearer_token(request.headers()) else {
        return platform_core::problem::problem_response(
            &platform_core::problem::ProblemDetails::unauthenticated(&ctx, &instance),
        );
    };
    let Ok(claims) = iam.verify_access_token(&token) else {
        return platform_core::problem::problem_response(
            &platform_core::problem::ProblemDetails::unauthenticated(&ctx, &instance),
        );
    };

    // 请求头租户优先；与令牌租户不一致 → 404（契约 C3）。
    if let Some(tenant) = header_value(request.headers(), "x-tenant-id") {
        ctx = ctx.with_tenant(tenant);
    }
    match ctx.tenant_id() {
        Some(tenant) if tenant != claims.tenant_id => {
            return platform_core::problem::problem_response(
                &platform_core::problem::ProblemDetails::not_found(&ctx, &instance, "resource"),
            );
        }
        Some(_) => {}
        None => ctx = ctx.with_tenant(claims.tenant_id.clone()),
    }
    ctx = ctx.with_user(claims.subject.clone());

    if auth.permissions.ensure_fresh(&ctx).await.is_err() {
        return platform_core::problem::problem_response(
            &platform_core::problem::ProblemDetails::from_context(
                platform_core::problem::CODE_INTERNAL_ERROR,
                platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR),
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "the permission cache could not be refreshed",
                &ctx,
                &instance,
                None,
            ),
        );
    }

    request.extensions_mut().insert(ctx);
    request.extensions_mut().insert(auth.checker.clone());
    next.run(request).await
}

/// `Authorization: Bearer <token>`。
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let trimmed = raw.trim();
    let token = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))?
        .trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// 读一个请求头（去空白；空值视为没有）。
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 装配好的一套真栈。
struct World {
    /// 与 `main.rs` 同一挂法的路由（`/api/leave` + 认证层）。
    router: Router,
    /// 签真 JWT 用。
    iam: Arc<IamService>,
    /// 租户 A（建单方）。
    tenant_a: String,
    /// 租户 B（隔离对照）。
    tenant_b: String,
    /// 租户 A 的持 `read + write` 用户。
    writer_a: String,
    /// 租户 A 的只持 `read` 用户（403 对照）。
    reader_a: String,
    /// 租户 A 的持 `read + approve` 用户（审批人 B，AC-S-189 步骤 3）。
    approver_a: String,
    /// 租户 B 的持 `read + write` 用户。
    writer_b: String,
    /// 管理员连接（**独立查询**落库用）。
    admin: sqlx::PgPool,
}

impl World {
    /// 签一枚**真实**的 access token（用本 world 自己的密钥）。
    fn token(&self, user_id: &str, tenant: &str) -> String {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let claims = AccessClaims {
            subject: user_id.to_string(),
            tenant_id: tenant.to_string(),
            session_id: format!("session-{user_id}"),
            issuer: "base".to_string(),
            audience: "base-api".to_string(),
            issued_at: now,
            not_before: now,
            expires_at: now + 900,
            token_id: uuid::Uuid::new_v4().to_string(),
        };
        self.iam.keys().sign(&claims).expect("令牌应当能签发")
    }

    /// 走一次 HTTP 请求，返回 `(状态码, JSON 体)`。
    async fn call(
        &self,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let body = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(&value).expect("请求体应能序列化"))
            }
            None => Body::empty(),
        };
        let response = self
            .router
            .clone()
            .oneshot(builder.body(body).expect("请求应当能构造"))
            .await
            .expect("请求应当能完成");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("响应体应当能读");
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }
}

/// 把 `modules/*/migrations/*.up.sql` 全部应用一遍（幂等：迁移里都有 `IF NOT EXISTS`）。
///
/// ⚠️ 为什么不是只应用 leave 的：`collect_scopes` 会读到**所有**模块的
/// `tenantScoped` 段，然后 `apply_policies` 对每张声明的表执行 `ALTER TABLE`
/// —— 缺一张表就整个 setup 挂掉。
async fn apply_module_migrations(pool: &sqlx::PgPool) {
    let root = repo_root();
    let modules = std::fs::read_dir(root.join("modules")).expect("modules 目录应可读");
    let mut up_files: Vec<PathBuf> = Vec::new();
    for entry in modules.flatten() {
        let dir = entry.path().join("migrations");
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".up.sql"))
            {
                up_files.push(path);
            }
        }
    }
    up_files.sort();
    for path in up_files {
        let sql = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()));
        sqlx::raw_sql(&sql)
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("模块迁移 {} 失败：{error}", path.display()));
    }
}

/// 建真栈。
async fn world() -> World {
    let app_url = setup_database().await;
    let root = repo_root();
    let admin_url = std::env::var("DATABASE_URL_TEST").expect("DATABASE_URL_TEST");
    let admin = PgPoolOptions::new()
        .max_connections(4)
        .connect(&admin_url)
        .await
        .expect("管理员连接应当可建");

    // 1) 模块建表 + 租户策略（DDL 串行化）。
    let mut conn = admin.acquire().await.expect("应当能取到连接");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能拿到部署锁");
    drop(conn);
    apply_module_migrations(&admin).await;
    let admin_db = TenantDb::from_pool(admin.clone(), DbOptions::default());
    for scope in platform_db::collect_scopes(&root).expect("租户作用域声明应能收集") {
        platform_db::apply_policies(&admin_db, &scope.spec)
            .await
            .unwrap_or_else(|error| {
                panic!("模块 `{}` 的 RLS 策略应用失败：{error}", scope.module)
            });
    }
    let mut conn = admin.acquire().await.expect("应当能取到连接");
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    // 2) 业务连接走**应用角色**（RLS 才生效）。
    let tenant_db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接应当能建立");

    // 3) 真 IAM + 真 RBAC。
    let pem = platform_iam::jwt::generate_key_pair().expect("测试密钥");
    let iam = Arc::new(
        IamService::new(IamStore::new(tenant_db.clone()), test_config(pem))
            .expect("IAM 应当能构造"),
    );
    let rbac = Arc::new(RbacService::new(
        RbacStore::new(tenant_db.clone()),
        ScopePolicy::default(),
    ));
    iam.set_permission_provider(
        rbac.clone() as Arc<dyn platform_core::extension::permission::PermissionProvider>
    );

    // 4) 用户走**真实注册接口**（RBAC 的 user_role 有指向 iam_user 的外键）。
    let iam_router = Router::new().nest(
        "/api/iam",
        platform_iam::http::router(Arc::new(IamState {
            service: iam.clone(),
        })),
    );
    let tenant_a = unique_tenant("leave-a");
    let tenant_b = unique_tenant("leave-b");
    let writer_a = register(&iam_router, &tenant_a, "leave-writer").await;
    let reader_a = register(&iam_router, &tenant_a, "leave-reader").await;
    // 审批人 B（AC-S-189 步骤 3）：只有**审批权限点**才决定 `decide` 能不能过。
    let approver_a = register(&iam_router, &tenant_a, "leave-approver").await;
    let writer_b = register(&iam_router, &tenant_b, "leave-writer").await;

    // 5) 授权：A 的 writer 有 read+write，A 的 reader 只有 read，B 的 writer 有 read+write。
    let ctx_a = RequestContext::new().with_tenant(tenant_a.clone());
    let ctx_b = RequestContext::new().with_tenant(tenant_b.clone());
    grant_role(
        &rbac,
        &ctx_a,
        &tenant_a,
        &writer_a,
        &["leave:request:read", "leave:request:write"],
    )
    .await;
    grant_role(
        &rbac,
        &ctx_a,
        &tenant_a,
        &reader_a,
        &["leave:request:read"],
    )
    .await;
    grant_role(
        &rbac,
        &ctx_a,
        &tenant_a,
        &approver_a,
        &["leave:request:read", "leave:request:approve"],
    )
    .await;
    grant_role(
        &rbac,
        &ctx_b,
        &tenant_b,
        &writer_b,
        &["leave:request:read", "leave:request:write"],
    )
    .await;

    // 6) 通知服务 + `leave_decision` 模板（AC-S-189 的通知计数要查**真表**）。
    //
    // ⚠️ 模板是租户级数据 → 只能在**知道租户之后**种（迁移里没法种，见
    //    `seed_decision_template` 的文档）。服务实例与 `/api/notify/*` 同一个
    //    `NotifyService` 语义（同一张表、同一份配置源）。
    let notify_service = Arc::new(NotifyService::new(
        NotifyStore::new(tenant_db.clone()),
        NotifyConfig::new(Arc::new(TestNotifyConfigSource)),
    ));
    seed_decision_template(&notify_service, &ctx_a).await;

    // 7) 路由：与 `main.rs` 的 `"leave"` 臂**同一套**装配 ——
    //    `connect` → 显式注入审计与通知 → `router`（不走 `leave::mount`，
    //    它不透传注入，装配出来的模块不写审计、不发通知）。
    let checker = Arc::new(RbacChecker::new(rbac.clone()));
    let leave_state = leave::connect(&app_url)
        .await
        .expect("请假模块装配应当成功")
        .with_audit(Arc::new(platform_audit::PgAuditSink::new(tenant_db.clone())))
        .with_notifier(platform_notify::db_sender::DbNotificationSender::new(
            notify_service,
            leave::service::NOTIFICATION_TEMPLATE,
        ));
    let router: Router = leave::http::router(Arc::new(leave_state));
    let router = router.layer(axum::middleware::from_fn_with_state(
        AuthState {
            iam: Some(iam.clone()),
            checker: checker.clone(),
            permissions: checker,
        },
        authenticate,
    ));
    let router = Router::new().nest("/api/leave", router);

    World {
        router,
        iam,
        tenant_a,
        tenant_b,
        writer_a,
        reader_a,
        approver_a,
        writer_b,
        admin,
    }
}

/// 走真实注册接口建用户，返回 user id。
async fn register(iam_router: &Router, tenant: &str, username: &str) -> String {
    let response = iam_router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/register")
                .header("x-tenant-id", tenant)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "username": username,
                        "password": PASSWORD,
                        "email": format!("{username}@{tenant}.example.com"),
                    })
                    .to_string(),
                ))
                .expect("请求应当能构造"),
        )
        .await
        .expect("请求应当能完成");
    assert_eq!(response.status(), StatusCode::CREATED, "注册应当 201");
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("响应体应当能读");
    let user: Value = serde_json::from_slice(&bytes).expect("响应体应当是 JSON");
    user["id"].as_str().expect("id 必须存在").to_string()
}

/// 建角色 → 授权限点 → 绑用户。
async fn grant_role(
    rbac: &RbacService,
    ctx: &RequestContext,
    tenant: &str,
    user_id: &str,
    permissions: &[&str],
) {
    let role_id = format!("role-leave-{}", uuid::Uuid::new_v4().simple());
    rbac.create_role(
        ctx,
        &Role {
            tenant_id: tenant.to_string(),
            id: role_id.clone(),
            code: role_id.clone(),
            name: role_id.clone(),
            data_scope: platform_core::extension::permission::DataScope::All,
            custom_filter: None,
            use_all_memberships: false,
            builtin: false,
        },
    )
    .await
    .expect("角色应当能建成");
    for permission in permissions {
        rbac.grant_permission(ctx, &role_id, permission, None)
            .await
            .unwrap_or_else(|error| panic!("授 {permission} 应当成功：{error}"));
    }
    rbac.bind_user_role(ctx, user_id, &role_id)
        .await
        .expect("绑定用户角色应当成功");
}

/// 通知的**配置源**（测试固定值）：5 个退避参数全走默认，租户默认 locale = `zh-CN`。
///
/// 生产实现是 `main.rs` 的 `PlatformNotifyConfigSource`（每请求读 `platform_config`）；
/// 这里只关心「三级回退的第 2 级拿到了什么」，所以写死。
struct TestNotifyConfigSource;

impl NotifyConfigSource for TestNotifyConfigSource {
    fn read_platform<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _key: &'a str,
    ) -> NotifyBoxFuture<'a, Option<i64>> {
        Box::pin(async { None })
    }

    fn tenant_default_locale(&self, _ctx: &RequestContext) -> NotifyBoxFuture<'_, String> {
        Box::pin(async { "zh-CN".to_string() })
    }
}

/// 给某租户种 `leave_decision` 模板（**zh-CN + en-US**，D294 的两种基准语言）。
///
/// # ⚠️ 为什么是测试夹具，不是迁移（进度文件 ③ 的实测结论）
///
/// 模板表是**租户级**的：`platform_notify_template` 的 `tenant_id` 是 `NOT NULL`，
/// 唯一键是 `(tenant_id, code, channel, locale)`，而 `resolve_template` 的 SQL 是
/// `WHERE tenant_id = {tenant} AND code = ? …` → **迁移里没有租户可写**，
/// 写一行 `tenant_id = '*'` 也永远匹配不上任何 `ctx`。
///
/// 平台的既有约定就是「模板由租户自己通过 `POST /api/notify/templates` 种」
/// （REQ-015 里没有任何模板种子迁移）。这里用**同一个服务方法**在真表里种，
/// 不是假 sink、也不是直接拼 SQL —— AC-S-189 的通知计数查的仍是真表。
async fn seed_decision_template(service: &NotifyService, ctx: &RequestContext) {
    for (locale, title, body) in [
        (
            "zh-CN",
            "请假审批结果",
            "你的请假申请已处理，当前状态：{{status}}",
        ),
        (
            "en-US",
            "Leave decision",
            "Your leave request has been reviewed; status: {{status}}",
        ),
    ] {
        let new = NewTemplate {
            id: uuid::Uuid::new_v4().to_string(),
            code: leave::service::NOTIFICATION_TEMPLATE.to_string(),
            channel: channel::IN_APP.to_string(),
            locale: locale.to_string(),
            category: category::TRANSACTIONAL.to_string(),
            title_template: title.to_string(),
            body_template: body.to_string(),
        };
        service
            .create_template(ctx, &new)
            .await
            .unwrap_or_else(|error| panic!("种 `leave_decision` 模板（{locale}）失败：{error}"));
    }
}

/// 一条合法的建单请求体。
fn create_body() -> Value {
    json!({
        "startsOn": "2026-12-01",
        "endsOn": "2026-12-02",
        "reason": "family matters",
    })
}

/// 独立查询：某租户下这张单是否存在、状态是什么。
async fn stored_status(world: &World, tenant: &str, id: &str) -> Option<String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT status FROM leave_request WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&world.admin)
    .await
    .expect("独立查询应当能执行");
    row.map(|(status,)| status)
}

// ---------------------------------------------------------------------------
// ④ 建单 + 列表（真装配，真令牌）
// ---------------------------------------------------------------------------

/// **主判据**：`POST /api/leave/requests` → 201，且**独立查询**能查到 `draft`。
///
/// 删掉 `main.rs` 的 `"leave"` 臂（或 `leave.workspace` 依赖），`leave::connect` 调不到
/// → 本用例编译不过；删掉 `route_layer`，401/403 用例必红。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn a_created_request_is_persisted_and_listed() {
    let world = world().await;
    let token = world.token(&world.writer_a, &world.tenant_a);

    let (status, body) = world
        .call("POST", "/api/leave/requests", Some(&token), Some(create_body()))
        .await;
    assert_eq!(status, StatusCode::CREATED, "建单必须 201：{body}");
    let id = body["id"].as_str().expect("响应必须带 id").to_string();
    assert_eq!(body["status"], "draft");
    assert_eq!(
        body["applicantId"], world.writer_a,
        "申请人来自令牌的用户，不接受客户端指定"
    );

    // 独立查询（管理员连接绕过 RLS，确认**真的落库**）。
    assert_eq!(
        stored_status(&world, &world.tenant_a, &id).await.as_deref(),
        Some("draft"),
        "建单必须落进 leave_request"
    );

    // 列表能查到。
    let (status, listed) = world
        .call("GET", "/api/leave/requests", Some(&token), None)
        .await;
    assert_eq!(status, StatusCode::OK, "列表必须 200：{listed}");
    let items = listed.as_array().expect("列表是数组");
    assert!(
        items.iter().any(|item| item["id"] == id),
        "列表里必须有刚建的单：{listed}"
    );
}

// ---------------------------------------------------------------------------
// 阴性对照：401 / 403（不允许「只测阳性」）
// ---------------------------------------------------------------------------

/// 不带令牌 → **401 `UNAUTHENTICATED`**（并且**不得**建出任何单）。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn a_request_without_a_token_is_unauthenticated() {
    let world = world().await;
    let before = count_rows(&world, &world.tenant_a).await;

    let (status, body) = world
        .call("POST", "/api/leave/requests", None, Some(create_body()))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "不带令牌必须 401：{body}");
    assert_eq!(body["code"], "UNAUTHENTICATED");
    assert_eq!(
        count_rows(&world, &world.tenant_a).await,
        before,
        "401 的请求不得落库"
    );
}

/// 有令牌但缺 `leave:request:write` → **403 `INSUFFICIENT_PERMISSION`**。
///
/// ⚠️ 这一条同时是 C-5 的**运行期对照**：如果 `POST /requests` 的
/// `route_layer(require_permission("leave:request:write"))` 被删，
/// 只持 `read` 的用户就会建单成功 → 本用例必红。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn a_user_without_the_write_permission_is_forbidden() {
    let world = world().await;
    let token = world.token(&world.reader_a, &world.tenant_a);

    let (status, body) = world
        .call("POST", "/api/leave/requests", Some(&token), Some(create_body()))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "缺权限点必须 403：{body}");
    assert_eq!(body["code"], "INSUFFICIENT_PERMISSION");
    assert_eq!(
        count_rows(&world, &world.tenant_a).await,
        0,
        "403 的请求不得落库"
    );
}

/// 某租户下的行数（管理员连接，独立于被测路径）。
async fn count_rows(world: &World, tenant: &str) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM leave_request WHERE tenant_id = $1")
        .bind(tenant)
        .fetch_one(&world.admin)
        .await
        .expect("count 查询应当能执行");
    row.0
}

// ---------------------------------------------------------------------------
// ⑤ 租户隔离（应用角色连接 → RLS 真的生效）
// ---------------------------------------------------------------------------

/// 租户 A 建的单，租户 B 的列表**看不到**。
///
/// ⚠️ 用**应用角色**连接（不是管理员）：管理员 `base` 是超级用户，
/// 会**绕过 RLS** → 那种写法下本用例永远绿，等于没测。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn another_tenant_cannot_see_the_request() {
    let world = world().await;
    let token_a = world.token(&world.writer_a, &world.tenant_a);

    let (status, created) = world
        .call("POST", "/api/leave/requests", Some(&token_a), Some(create_body()))
        .await;
    assert_eq!(status, StatusCode::CREATED, "A 建单应当 201：{created}");
    let id = created["id"].as_str().expect("id").to_string();
    assert_eq!(count_rows(&world, &world.tenant_a).await, 1);

    // B 自己的列表：**看不到** A 的单。
    let token_b = world.token(&world.writer_b, &world.tenant_b);
    let (status, listed) = world
        .call("GET", "/api/leave/requests", Some(&token_b), None)
        .await;
    assert_eq!(status, StatusCode::OK, "B 的列表应当 200：{listed}");
    let items = listed.as_array().expect("列表是数组");
    assert!(
        items.is_empty(),
        "租户 B 不得看到租户 A 的单：{listed}"
    );
    assert!(
        !items.iter().any(|item| item["id"] == id),
        "按 id 也不得泄露"
    );

    // 阳性对照：A 自己的列表看得到（否则「两个租户都空」也会绿）。
    let (_, listed_a) = world
        .call("GET", "/api/leave/requests", Some(&token_a), None)
        .await;
    assert!(
        listed_a
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == id)),
        "租户 A 必须看得到自己的单：{listed_a}"
    );
}

// ---------------------------------------------------------------------------
// ① 错误码注册的**运行期**证明
// ---------------------------------------------------------------------------

/// 非法流转 → 409 + `LEAVE_INVALID_TRANSITION`，且 `title` **不是**兜底 `"Error"`。
///
/// 这是本批 ①（`titles.rs` + `ALL_CODES` 注册）的判据：不注册，
/// `title_of_code()` 返回 `FALLBACK_TITLE`（`"Error"`）→ 本条必红。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn an_illegal_transition_reports_a_registered_title() {
    let world = world().await;
    let token = world.token(&world.writer_a, &world.tenant_a);

    let (status, created) = world
        .call("POST", "/api/leave/requests", Some(&token), Some(create_body()))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().expect("id").to_string();

    // 第一次 submit：draft → pending。
    let (status, submitted) = world
        .call(
            "POST",
            &format!("/api/leave/requests/{id}/submit"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "首次提交应当成功：{submitted}");
    assert_eq!(submitted["status"], "pending");

    // 第二次 submit：pending → 409（非法流转）。
    let (status, rejected) = world
        .call(
            "POST",
            &format!("/api/leave/requests/{id}/submit"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "重复提交必须 409：{rejected}");
    assert_eq!(rejected["code"], "LEAVE_INVALID_TRANSITION");
    assert_eq!(
        rejected["title"], "Invalid Leave Transition",
        "title 必须来自 TITLES（未注册会落兜底 `Error`）：{rejected}"
    );
    // 409 不得改状态。
    assert_eq!(
        stored_status(&world, &world.tenant_a, &id).await.as_deref(),
        Some("pending")
    );
}

// ---------------------------------------------------------------------------
// AC-S-189 的**计数表**（真装配 → 真审计表 + 真投递表）
// ---------------------------------------------------------------------------

/// 本单在 `platform_audit_log` 里的动作序列（**管理员连接**，按 `created_at` 升序）。
///
/// 管理员连接**绕过 RLS**，所以这里看到的是「真的落库了什么」；
/// 被测路径用的是**应用角色**（RLS 生效）。
async fn audit_actions(world: &World, tenant: &str, id: &str) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT action FROM platform_audit_log \
         WHERE tenant_id = $1 AND object_type = 'leave.request' AND object_id = $2 \
         ORDER BY created_at, id",
    )
    .bind(tenant)
    .bind(id)
    .fetch_all(&world.admin)
    .await
    .expect("审计表的独立查询应当能执行");
    rows.into_iter().map(|(action,)| action).collect()
}

/// 本单在 `platform_notify_delivery` 里的投递记录（**真表**，按 `created_at` 升序）。
async fn deliveries_of(world: &World, tenant: &str, id: &str) -> Vec<(String, String, String)> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT recipient, channel, template_code FROM platform_notify_delivery \
         WHERE tenant_id = $1 AND payload->>'leaveRequestId' = $2 \
         ORDER BY created_at, id",
    )
    .bind(tenant)
    .bind(id)
    .fetch_all(&world.admin)
    .await
    .expect("投递表的独立查询应当能执行");
    rows
}

/// 建单，返回主键。
async fn create_request(world: &World, token: &str) -> String {
    let (status, body) = world
        .call("POST", "/api/leave/requests", Some(token), Some(create_body()))
        .await;
    assert_eq!(status, StatusCode::CREATED, "建单应当 201：{body}");
    body["id"].as_str().expect("响应必须带 id").to_string()
}

/// 提交，断言 `draft → pending`。
async fn submit_request(world: &World, token: &str, id: &str) {
    let (status, body) = world
        .call(
            "POST",
            &format!("/api/leave/requests/{id}/submit"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "提交应当 200：{body}");
    assert_eq!(body["status"], "pending");
}

/// 审批，断言目标状态。
async fn decide_request(world: &World, token: &str, id: &str, decision: &str, expected: &str) {
    let (status, body) = world
        .call(
            "POST",
            &format!("/api/leave/requests/{id}/decide"),
            Some(token),
            Some(json!({ "decision": decision })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{decision} 应当 200：{body}");
    assert_eq!(body["status"], expected, "{decision} 后状态必须是 {expected}：{body}");
}

/// **AC-S-189 的计数表**（`acceptance.md:187-193`，逐行断言）。
///
/// | # | 事件 | 审计 | 通知 | 收件人 |
/// |---|---|---|---|---|
/// | 1 | 建单 | 1 | 0 | — |
/// | 2 | 提交 | 1 | **0**（TD-088：平台无法按权限点反查审批人） | — |
/// | 3 | 审批通过 | 1 | 1 | 申请人 A |
/// | 4 | 驳回 | 1 | 1 | 申请人 A |
/// | 5 | 非法流转 | 0 | 0 | — |
///
/// # ⚠️ 为什么这是**本批次 ① 的判据**
///
/// 批次 2 的装配走 `leave::mount`，它**不透传**审计 / 通知注入口 → 真实装配下
/// `/api/leave/*` 一条审计也不写、一条通知也不发。把 `main.rs` 的注入改回
/// `leave::mount`（或删掉 `.with_audit(...)` / `.with_notifier(...)`），
/// 本用例的**每一行**都会红。
///
/// # ⚠️ 两条不能偷懒的地方
///
/// 1. **查真表**：审计读 `platform_audit_log`、通知读 `platform_notify_delivery`。
///    用内存 sink / 计数 sender 只会证明「方法被调过」，证不了「落了库」（§9.15 的假 sink 陷阱）。
/// 2. **逐条匹配 action**：只数条数的话，「建单记了 3 条 approved」也能绿。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn the_audit_and_notification_counts_match_the_table() {
    let world = world().await;
    let applicant = world.token(&world.writer_a, &world.tenant_a);
    let approver = world.token(&world.approver_a, &world.tenant_a);

    // ---- 第 1 行：建单 → 审计 1 / 通知 0 ----
    let approved_id = create_request(&world, &applicant).await;
    assert_eq!(
        audit_actions(&world, &world.tenant_a, &approved_id).await,
        vec!["leave.request.created"],
        "第 1 行：建单恰好 1 条审计（leave.request.created）"
    );
    assert!(
        deliveries_of(&world, &world.tenant_a, &approved_id)
            .await
            .is_empty(),
        "第 1 行：建单不发通知（0 条投递）"
    );

    // ---- 第 2 行：提交 → 审计 1 / 通知 0 ----
    submit_request(&world, &applicant, &approved_id).await;
    assert_eq!(
        audit_actions(&world, &world.tenant_a, &approved_id).await,
        vec!["leave.request.created", "leave.request.submitted"],
        "第 2 行：提交再加 1 条审计（leave.request.submitted）"
    );
    assert!(
        deliveries_of(&world, &world.tenant_a, &approved_id)
            .await
            .is_empty(),
        "第 2 行：提交不发通知（0 条；平台无法按权限点反查审批人，TD-088）"
    );

    // ---- 第 3 行：审批通过 → 审计 1 / 通知 1（收件人 = 申请人）----
    decide_request(&world, &approver, &approved_id, "approve", "approved").await;
    assert_eq!(
        audit_actions(&world, &world.tenant_a, &approved_id).await,
        vec![
            "leave.request.created",
            "leave.request.submitted",
            "leave.request.approved",
        ],
        "第 3 行：审批通过再加 1 条审计（leave.request.approved）"
    );
    assert_eq!(
        deliveries_of(&world, &world.tenant_a, &approved_id).await,
        vec![(
            world.writer_a.clone(),
            channel::IN_APP.to_string(),
            leave::service::NOTIFICATION_TEMPLATE.to_string(),
        )],
        "第 3 行：恰好 1 条投递，收件人是申请人 A，通道 in_app"
    );

    // ---- 第 4 行：驳回（另建一单）→ 审计 1 / 通知 1 ----
    let rejected_id = create_request(&world, &applicant).await;
    submit_request(&world, &applicant, &rejected_id).await;
    decide_request(&world, &approver, &rejected_id, "reject", "rejected").await;
    assert_eq!(
        audit_actions(&world, &world.tenant_a, &rejected_id).await,
        vec![
            "leave.request.created",
            "leave.request.submitted",
            "leave.request.rejected",
        ],
        "第 4 行：驳回恰好 3 条审计（最后一跳是 leave.request.rejected）"
    );
    assert_eq!(
        deliveries_of(&world, &world.tenant_a, &rejected_id).await,
        vec![(
            world.writer_a.clone(),
            channel::IN_APP.to_string(),
            leave::service::NOTIFICATION_TEMPLATE.to_string(),
        )],
        "第 4 行：恰好 1 条投递，收件人是申请人 A"
    );

    // ---- 第 5 行：非法流转（对已 approved 的单再 submit / 再 decide）→ 0 / 0 ----
    let audit_before = audit_actions(&world, &world.tenant_a, &approved_id).await;
    let deliveries_before = deliveries_of(&world, &world.tenant_a, &approved_id).await;
    assert_eq!(audit_before.len(), 3, "前置：approved 的单应有 3 条审计");
    assert_eq!(deliveries_before.len(), 1, "前置：approved 的单应有 1 条投递");

    let (status, body) = world
        .call(
            "POST",
            &format!("/api/leave/requests/{approved_id}/submit"),
            Some(&applicant),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "已 approved 再 submit 必须 409：{body}");
    assert_eq!(body["code"], "LEAVE_INVALID_TRANSITION");

    let (status, body) = world
        .call(
            "POST",
            &format!("/api/leave/requests/{approved_id}/decide"),
            Some(&approver),
            Some(json!({ "decision": "reject" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "已 approved 再 decide 必须 409：{body}");
    assert_eq!(body["code"], "LEAVE_INVALID_TRANSITION");

    assert_eq!(
        audit_actions(&world, &world.tenant_a, &approved_id).await,
        audit_before,
        "第 5 行：非法流转**不得**多写审计（0 条）"
    );
    assert_eq!(
        deliveries_of(&world, &world.tenant_a, &approved_id).await,
        deliveries_before,
        "第 5 行：非法流转**不得**多发通知（0 条）"
    );
}
