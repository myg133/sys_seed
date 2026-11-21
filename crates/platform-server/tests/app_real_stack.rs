//! REQ-016 的**装配层**判据：真实 IAM + 真实 RBAC + 真实 `ConfigService`
//! + 真实 `PgAuditSink` + 真实路由。
//!
//! # 这个文件守的四件事（`app_integration.rs` 用假 sink / 内存配置**测不到**）
//!
//! | AC | 判据 | 为什么必须在这里 |
//! |----|------|------------------|
//! | **AC-S-160 步骤 4/5** | 用 **`platform_config_upsert`**（真库通道）改 `app.workspace.maxApps` → **立刻**生效 | 需要真实 `ConfigService` 的 **epoch 失效**路径（内存源测不出「改配置生效」） |
//! | **AC-S-161 步骤 6** | 改 `app.recentCount` → **立刻**生效 | 同上 |
//! | **AC-S-164** | 注册/修改/删除**真的落进** `platform_audit_log` + **哈希链自洽** | 真实 `PgAuditSink` 只在装配层接得上（D150）；内存 `Vec` 不能证明「落库」 |
//! | **AC-S-162** | 导航打真实端点（真 manifest 的 `menus[]` + 真权限点） | 需要真实 RBAC 的权限判定 |
//!
//! # 为什么「调一个函数」不算
//!
//! 审计侧：`RecordingSink` 只证明「事件被构造出来了」，**不能证明它落库了**。
//! → 这里用真 `PgAuditSink`，再从 `platform_audit_log` **读回来**断字段，
//! 并用 `ChainVerifier` 校验哈希链。

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use platform_core::RequestContext;
use platform_iam::config::{
    Argon2Settings, IamConfig, JwtSettings, LoginSettings, PasswordPolicySettings, TokenSettings,
    TotpSettings,
};
use platform_iam::http::IamState;
use platform_iam::jwt::AccessClaims;
use platform_iam::service::IamService;
use platform_iam::store::IamStore;
use platform_rbac::entity::Role;
use platform_rbac::http::{IamTokenVerifier, RbacState};
use platform_rbac::{RbacChecker, RbacService, RbacStore, ScopePolicy};
use serde_json::{Value, json};
use tower::ServiceExt;

mod support;

use support::*;

/// 密码（满足平台策略）。
const PASSWORD: &str = "correct-horse-battery-staple";

/// 测试用 IAM 配置。
fn test_config(pem: String) -> IamConfig {
    IamConfig {
        jwt: JwtSettings {
            issuer: "base".to_string(),
            audience: "base-api".to_string(),
            private_key_pem: pem,
            kid: "kid-app-stack".to_string(),
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

/// 装配好的一套真栈。
struct World {
    router: axum::Router,
    /// IAM 服务（签登录令牌用）。
    iam: Arc<IamService>,
    tenant: String,
    /// 持 3 个 `app:*` 权限点 + `audit:events:read` 的用户。
    admin_user: String,
    /// 只持 `app:registry:read` 的用户（导航过滤的对照）。
    read_user: String,
    admin: sqlx::PgPool,
    /// 应用角色连接（业务查询用）。
    app_url: String,
    /// OAuth 客户端（每个用例自建，避免撞车）。
    client_id: String,
}

/// 建真栈：IAM + RBAC（真实权限表）+ 真配置服务 + 真审计 sink + `/api/apps`。
async fn world() -> World {
    let app_url = setup_database().await;
    let tenant = unique_tenant("app-stack");
    let root = repo_root();

    let tenant_db = platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    // **只**给本需求自己的 2 张表生成 RLS 策略（别碰共享库里别人的表）。
    let admin_url = std::env::var("DATABASE_URL_TEST").expect("DATABASE_URL_TEST");
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&admin_url)
        .await
        .expect("管理员连接应当可建");
    for scope in platform_db::collect_scopes(&root).expect("租户作用域声明应能收集") {
        if scope
            .spec
            .tables
            .iter()
            .any(|table| table.starts_with("platform_app"))
        {
            let admin_db =
                platform_db::TenantDb::from_pool(admin.clone(), platform_db::DbOptions::default());
            platform_db::apply_policies(&admin_db, &scope.spec)
                .await
                .expect("app 表的 RLS 策略应能应用");
        }
    }

    // 1) IAM（真实服务）。
    let pem = platform_iam::jwt::generate_key_pair().expect("测试密钥");
    let iam = Arc::new(
        IamService::new(IamStore::new(tenant_db.clone()), test_config(pem))
            .expect("IAM 应当能构造"),
    );

    // 2) RBAC（真实服务 + 真实注册表）。
    let rbac = Arc::new(RbacService::new(
        RbacStore::new(tenant_db.clone()),
        ScopePolicy::default(),
    ));
    let provider: Arc<dyn platform_core::extension::permission::PermissionProvider> =
        rbac.clone();
    assert!(
        iam.set_permission_provider(provider),
        "权限点提供者应当注入成功"
    );

    // 3) **真**配置服务（走真库 + epoch 失效）。
    let registry = platform_config::ConfigRegistry::from_manifest_root(&root)
        .expect("配置注册表应能加载");
    let config_service = Arc::new(platform_config::ConfigService::new(
        tenant_db.clone(),
        Arc::new(registry),
    ));

    // 4) **真**审计 sink（AC-S-164：**不是**内存 Vec）。
    let audit_sink: Arc<dyn platform_core::extension::audit::AuditSink> =
        Arc::new(platform_audit::PgAuditSink::new(tenant_db.clone()));

    // 5) 用户：走**真实注册接口**（RBAC 的 user_role 有指向 iam_user 的外键）。
    let iam_state = Arc::new(IamState {
        service: iam.clone(),
    });
    let iam_router = axum::Router::new()
        .nest("/api/iam", platform_iam::http::router(iam_state.clone()))
        .merge(platform_iam::http::jwks_router(iam_state.clone()));
    let admin_user = register(&iam_router, &tenant, "app-admin").await;
    let read_user = register(&iam_router, &tenant, "app-reader").await;

    // 6) 两个角色：管理员（read/write/delete + audit）与只读。
    let ctx = RequestContext::new().with_tenant(tenant.clone());
    grant_role(
        &rbac,
        &ctx,
        &tenant,
        &admin_user,
        &[
            "app:registry:read",
            "app:registry:write",
            "app:registry:delete",
            "audit:events:read",
        ],
    )
    .await;
    grant_role(&rbac, &ctx, &tenant, &read_user, &["app:registry:read"]).await;

    // 7) 一个 OAuth client（平台级 → 管理员连接直插）。
    let client_id = format!("client-appstack-{}", uuid::Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO platform_oauth_client \
         (client_id, client_secret_hash, client_type, name, redirect_uris, \
          allowed_grant_types, allowed_scopes, status, created_at) \
         VALUES ($1, NULL, 'public', '装配层应用', ARRAY['https://c.example/cb'], \
                 ARRAY['authorization_code'], ARRAY['rbac:role:read'], 'active', now())",
    )
    .bind(&client_id)
    .execute(&admin)
    .await
    .expect("client 应当能插入");

    // 8) 路由：与 `main.rs` 相同的挂法。
    let checker = Arc::new(RbacChecker::new(rbac.clone()));
    let rbac_state = Arc::new(RbacState {
        service: rbac.clone(),
        checker: checker.clone(),
        tokens: Arc::new(IamTokenVerifier::new(iam.clone())),
    });
    let app_state = Arc::new(platform_app::http::AppState {
        service: Arc::new(platform_app::service::AppService::new(
            Arc::new(platform_app::store::AppStore::new(tenant_db.clone())),
            Arc::new(StackAppConfigSource::new(config_service.clone())),
            audit_sink.clone(),
            Arc::new(load_manifests(&root)),
            checker.clone() as Arc<dyn platform_core::extension::permission::PermissionChecker>,
            Arc::new(StackTokenVerifier::new(iam.clone())),
        )),
        checker: checker.clone() as Arc<dyn platform_core::extension::permission::PermissionChecker>,
        refresher: checker
            as Arc<dyn platform_core::extension::permission::PermissionRefresher>,
    });
    let router = axum::Router::new()
        .nest("/api/iam", platform_iam::http::router(iam_state))
        .nest("/api/rbac", platform_rbac::http::router(rbac_state))
        .nest("/api/apps", platform_app::http::router(app_state));

    World {
        router,
        iam,
        tenant,
        admin_user,
        read_user,
        admin,
        app_url,
        client_id,
    }
}

/// 走**真实注册接口**建用户，返回 user id。
async fn register(iam_router: &axum::Router, tenant: &str, username: &str) -> String {
    let response = iam_router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/register")
                .header("x-tenant-id", tenant)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "username": username,
                        "password": PASSWORD,
                        "email": format!("{username}@example.com"),
                    })
                    .to_string(),
                ))
                .expect("请求"),
        )
        .await
        .expect("请求应能发出");
    assert_eq!(response.status(), StatusCode::CREATED, "注册应当 201");
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("响应体");
    let user: Value = serde_json::from_slice(&bytes).expect("应是 JSON");
    user["id"].as_str().expect("id").to_string()
}

/// 建一个角色并授予权限点，再绑给用户。
async fn grant_role(
    rbac: &RbacService,
    ctx: &RequestContext,
    tenant: &str,
    user_id: &str,
    permissions: &[&str],
) {
    let role_id = format!("role-appstack-{}", uuid::Uuid::new_v4().simple());
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

/// 加载**已启用模块**的 manifest（导航的 module 来源）。
fn load_manifests(root: &std::path::Path) -> Vec<platform_core::extension::manifest::Manifest> {
    let mut manifests = Vec::new();
    for directory in ["modules", "crates"] {
        let Ok(entries) = std::fs::read_dir(root.join(directory)) else {
            continue;
        };
        let mut paths: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        paths.sort();
        for module_dir in paths {
            let manifest_path = module_dir.join("manifest.json");
            if !manifest_path.is_file() {
                continue;
            }
            let raw = std::fs::read_to_string(&manifest_path).expect("manifest 应可读");
            let manifest = platform_core::extension::manifest::Manifest::parse_checked(&raw)
                .expect("manifest 应合法");
            if manifest.enabled {
                manifests.push(manifest);
            }
        }
    }
    manifests
}

/// 真 IAM 的令牌校验。
struct StackTokenVerifier {
    inner: Arc<IamService>,
}

impl StackTokenVerifier {
    fn new(inner: Arc<IamService>) -> Self {
        Self { inner }
    }
}

impl platform_app::tokens::TokenVerifier for StackTokenVerifier {
    fn verify(
        &self,
        token: &str,
    ) -> Result<platform_app::tokens::VerifiedToken, platform_app::tokens::AuthError> {
        let claims = self
            .inner
            .verify_access_token(token)
            .map_err(|_| platform_app::tokens::AuthError)?;
        Ok(platform_app::tokens::VerifiedToken {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
        })
    }
}

/// 真 `ConfigService` 的配置源（**每请求读**，走 epoch 失效）。
struct StackAppConfigSource {
    config: Arc<platform_config::ConfigService>,
}

impl StackAppConfigSource {
    fn new(config: Arc<platform_config::ConfigService>) -> Self {
        Self { config }
    }
}

impl platform_app::config::AppConfigSource for StackAppConfigSource {
    fn read<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> platform_app::config::AppBoxFuture<
        'a,
        Result<platform_app::config::AppSettings, platform_app::config::AppConfigError>,
    > {
        Box::pin(async move {
            let keys: Vec<String> = platform_app::config::CONFIG_KEYS
                .iter()
                .map(|key| key.to_string())
                .collect();
            let entries = self
                .config
                .read_platform_entries(ctx, &keys)
                .await
                .map_err(|_| platform_app::config::AppConfigError)?;
            let mut settings = platform_app::config::AppSettings::default();
            for entry in entries {
                match entry.key.as_str() {
                    platform_app::config::CONFIG_KEY_RECENT_COUNT => {
                        if let Some(value) = entry.value.as_i64() {
                            settings.recent_count = value;
                        }
                    }
                    platform_app::config::CONFIG_KEY_WORKSPACE_MAX_APPS => {
                        if let Some(value) = entry.value.as_i64() {
                            settings.workspace_max_apps = value;
                        }
                    }
                    platform_app::config::CONFIG_KEY_ALLOW_HTTP => {
                        if let Some(value) = entry.value.as_bool() {
                            settings.allow_http = value;
                        }
                    }
                    _ => {}
                }
            }
            Ok(settings)
        })
    }
}

impl World {
    /// 签一枚**真实**登录令牌（用 `world` 自己的 IAM 密钥）。
    fn token(&self, user_id: &str) -> String {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let claims = AccessClaims {
            subject: user_id.to_string(),
            tenant_id: self.tenant.clone(),
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

    /// 管理员令牌（持 3 个 `app:*` + `audit:events:read`）。
    async fn admin_token(&self) -> String {
        self.token(&self.admin_user)
    }

    /// 只读用户令牌（只持 `app:registry:read`）。
    async fn read_token(&self) -> String {
        self.token(&self.read_user)
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
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        let body = match body {
            Some(value) => {
                builder = builder.header("content-type", "application/json");
                Body::from(serde_json::to_vec(&value).expect("请求体应能序列化"))
            }
            None => Body::empty(),
        };
        let response = self
            .router
            .clone()
            .oneshot(builder.body(body).expect("请求"))
            .await
            .expect("请求应能发出");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("响应体");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    /// 用 `platform_config_upsert` 改一个配置（**自增 epoch**，D282）。
    async fn set_config(&self, key: &str, value: &str) {
        sqlx::query(&format!(
            "SELECT platform_config_upsert('{key}', '{value}'::jsonb)"
        ))
        .execute(&self.admin)
        .await
        .expect("配置写入应能执行");
    }

    /// 造一个应用（管理员连接；绕开 HTTP —— 只给「需要夹具」的用例用）。
    ///
    /// ⚠️ `id` 会**加上本租户的后缀**：`base_test` 是**共享库**，裸 id（`app-a` 等）
    /// 可能已被别的用例占用 → `ON CONFLICT (id) DO UPDATE` **不会**改 `tenant_id`
    /// → 行留在**别人的租户**里 → 本租户看不到（表现为「工作台 0 组」，最难查的一类）。
    /// 返回真正落库的 id。
    async fn seed_app(
        &self,
        id: &str,
        display_name: &str,
        category: &str,
        sort_order: i32,
        status: &str,
        launch_url: Option<&str>,
    ) -> String {
        let id = format!("{id}-{}", self.tenant);
        // ⚠️ `UNIQUE (tenant_id, client_id)`：同一租户**不能**用同一个 client 接两个应用
        // → 每个夹具应用**自建一个** client（这也是真实场景的形态）。
        let client_id = format!("client-{id}");
        sqlx::query(
            "INSERT INTO platform_oauth_client \
             (client_id, client_secret_hash, client_type, name, redirect_uris, \
              allowed_grant_types, allowed_scopes, status, created_at) \
             VALUES ($1, NULL, 'public', $2, ARRAY['https://c.example/cb'], \
                     ARRAY['authorization_code'], ARRAY['rbac:role:read'], 'active', now()) \
             ON CONFLICT (client_id) DO NOTHING",
        )
        .bind(&client_id)
        .bind(format!("{client_id} 的 client 名"))
        .execute(&self.admin)
        .await
        .expect("夹具 client 应能插入");
        sqlx::query(
            "INSERT INTO platform_app \
             (id, tenant_id, client_id, display_name, description, category, icon_url, \
              launch_url, sort_order, status, created_at, created_by) \
             VALUES ($1, $2, $3, $4, NULL, $5, NULL, $6, $7, $8, now(), 'fixture') \
             ON CONFLICT (id) DO UPDATE SET display_name = EXCLUDED.display_name, \
                 category = EXCLUDED.category, sort_order = EXCLUDED.sort_order, \
                 status = EXCLUDED.status, launch_url = EXCLUDED.launch_url",
        )
        .bind(&id)
        .bind(&self.tenant)
        .bind(&client_id)
        .bind(display_name)
        .bind(category)
        .bind(launch_url)
        .bind(sort_order)
        .bind(status)
        .execute(&self.admin)
        .await
        .expect("应用夹具应能插入");
        id
    }
}

// ---------------------------------------------------------------------------
// AC-S-160 步骤 3/4/5：工作台的**原始 JSON** 排序 + 配置真库生效
// ---------------------------------------------------------------------------

/// AC-S-160 步骤 3：**原始 HTTP 响应体**的数组顺序逐字断言（D286：前端会重排 → 真判据在这里）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn the_workspace_raw_json_order_is_pinned() {
    let world = world().await;
    let token = world.admin_token().await;

    // 五个应用：A/B 在 finance，C/D 在 general，E 是 hidden。
    // ⚠️ **不给 `launch_url`**（D279：工作台不按它过滤）。
    let app_a = world
        .seed_app("app-a", "差旅报销", "finance", 20, "active", None)
        .await;
    world
        .seed_app("app-b", "费用申请", "finance", 10, "active", None)
        .await;
    // ⚠️ **id 与 displayName 顺序刻意相反**（"app-z"→"C 应用"、"app-a2"→"D 应用"）：
    // 让「按 id 兜底」的实现得到 D→C（错），只有按 displayName 才是 C→D（对）
    // → 否则次级排序键删掉也测不出来（D286 的 raw-JSON 判据必须有区分力）。
    world
        .seed_app("app-z", "C 应用", "general", 0, "active", None)
        .await;
    world
        .seed_app("app-a2", "D 应用", "general", 0, "active", None)
        .await;
    let app_e = world
        .seed_app("app-e", "E 应用", "general", 0, "hidden", None)
        .await;

    let (status, body) = world
        .call("GET", "/api/apps/workspace", Some(&token), None)
        .await;
    assert_eq!(status, 200, "工作台必须 200：{body}");

    let groups = body["groups"].as_array().expect("必须有 groups 数组");
    assert_eq!(groups.len(), 2, "**恰好** 2 组：{body}");
    assert_eq!(groups[0]["category"], "finance", "finance 在前（字典序升序）");
    assert_eq!(groups[1]["category"], "general", "general 在后");

    // ⚠️ **数组顺序逐字**（JSON 对象无序，但数组有序）。
    let finance: Vec<&str> = groups[0]["apps"]
        .as_array()
        .expect("数组")
        .iter()
        .map(|app| app["displayName"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        finance,
        vec!["费用申请", "差旅报销"],
        "finance 组内必须 B → A（sortOrder 10 < 20），**原始 JSON 顺序**：{body}"
    );
    let general: Vec<&str> = groups[1]["apps"]
        .as_array()
        .expect("数组")
        .iter()
        .map(|app| app["displayName"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        general,
        vec!["C 应用", "D 应用"],
        "general 组内 sortOrder 并列 → displayName 升序：{body}"
    );

    // hidden 不在任何组里；总数 4。
    let ids: Vec<&str> = groups
        .iter()
        .flat_map(|group| group["apps"].as_array().expect("数组").iter())
        .map(|app| app["appId"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(ids.len(), 4, "总数必须 4：{ids:?}");
    assert!(
        !ids.contains(&app_e.as_str()),
        "hidden 应用必须被过滤（{app_e}）：{ids:?}"
    );
    let _ = app_a;
}

// ---------------------------------------------------------------------------
// AC-S-160 步骤 5：配置**真库通道**立刻生效
// ---------------------------------------------------------------------------

/// AC-S-160 步骤 5：`platform_config_upsert('app.workspace.maxApps','2')` → **立刻**恰好 2 条。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn max_apps_from_the_database_takes_effect_immediately() {
    let world = world().await;
    let token = world.admin_token().await;
    let mut seeded = Vec::new();
    for (id, name, category, sort_order) in [
        ("app-b", "费用申请", "finance", 10),
        ("app-a", "差旅报销", "finance", 20),
        ("app-c", "C 应用", "general", 0),
        ("app-d", "D 应用", "general", 0),
    ] {
        seeded.push(
            world
                .seed_app(id, name, category, sort_order, "active", None)
                .await,
        );
    }
    // `seeded[0]` 是 app-b、`seeded[1]` 是 app-a。
    let expected = vec![seeded[0].clone(), seeded[1].clone()];

    // 改配置（真库通道，**自增 epoch**）→ **立刻**（不 sleep）再取。
    world.set_config("app.workspace.maxApps", "2").await;

    let (status, body) = world
        .call("GET", "/api/apps/workspace", Some(&token), None)
        .await;
    assert_eq!(status, 200, "工作台必须 200：{body}");
    let ids: Vec<&str> = body["groups"]
        .as_array()
        .expect("数组")
        .iter()
        .flat_map(|group| group["apps"].as_array().expect("数组").iter())
        .map(|app| app["appId"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(ids.len(), 2, "**恰好** 2 条（配置真的生效）：{body}");
    assert_eq!(
        ids, expected,
        "必须是全局 (category, sortOrder, displayName) 序的前 2 条（S-19）：{body}"
    );

    // 还原，避免污染共享库的后续运行。
    world.set_config("app.workspace.maxApps", "60").await;
}

/// AC-S-161 步骤 6：`platform_config_upsert('app.recentCount','1')` → recent 长度 1。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn recent_count_from_the_database_takes_effect_immediately() {
    let world = world().await;
    let token = world.admin_token().await;
    let app_a = world
        .seed_app("app-a", "A 应用", "general", 0, "active", None)
        .await;
    let app_b = world
        .seed_app("app-b", "B 应用", "general", 0, "active", None)
        .await;
    // 两条访问记录。
    world
        .call("POST", &format!("/api/apps/{app_a}/access"), Some(&token), None)
        .await;
    world
        .call("POST", &format!("/api/apps/{app_b}/access"), Some(&token), None)
        .await;

    // 默认上限 6 → 2 条。
    let (status, before) = world
        .call("GET", "/api/apps/recent", Some(&token), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        before.as_array().expect("数组").len(),
        2,
        "默认最近的 2 条都在：{before}"
    );

    // 改配置 → **立刻** 1 条。
    world.set_config("app.recentCount", "1").await;
    let (status, after) = world
        .call("GET", "/api/apps/recent", Some(&token), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        after.as_array().expect("数组").len(),
        1,
        "recentCount=1 必须**立刻**生效：{after}"
    );
    // 且是**最后访问**的那条（B）。
    assert_eq!(
        after[0]["appId"], app_b.as_str(),
        "应该是最后访问的 B（{app_b}）：{after}"
    );

    world.set_config("app.recentCount", "6").await;
}

// ---------------------------------------------------------------------------
// AC-S-164：审计**落库** + 哈希链
// ---------------------------------------------------------------------------

/// AC-S-164：注册/修改/删除 → `platform_audit_log` 里**三条记录** + 字段集 + **哈希链自洽**。
///
/// ⚠️ 这是**真 sink 落库**的判据（`app_integration.rs` 的 `RecordingSink` 只证明事件被构造出来）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn app_mutations_land_in_the_audit_log_with_an_intact_chain() {
    let world = world().await;
    let token = world.admin_token().await;

    // 注册。
    let (status, created) = world
        .call(
            "POST",
            "/api/apps",
            Some(&token),
            Some(json!({
                "clientId": world.client_id,
                "displayName": "审计落库探针",
            })),
        )
        .await;
    assert_eq!(status, 201, "注册应当 201：{created}");
    let app_id = created["appId"].as_str().expect("appId").to_string();

    // 修改。
    let (status, _) = world
        .call(
            "PATCH",
            &format!("/api/apps/{app_id}"),
            Some(&token),
            Some(json!({ "sortOrder": 5 })),
        )
        .await;
    assert_eq!(status, 200);

    // 删除。
    let (status, _) = world
        .call("DELETE", &format!("/api/apps/{app_id}"), Some(&token), None)
        .await;
    assert_eq!(status, 204);

    // 从**真表**读回三条记录。
    let rows: Vec<(String, String, Option<String>, String, String, String)> = sqlx::query_as(
        "SELECT action, object_type, object_id, ip, user_agent, actor_id \
         FROM platform_audit_log WHERE tenant_id = $1 AND object_id = $2 ORDER BY created_at",
    )
    .bind(&world.tenant)
    .bind(&app_id)
    .fetch_all(&world.admin)
    .await
    .expect("审计记录应能读出");

    let actions: Vec<&str> = rows.iter().map(|row| row.0.as_str()).collect();
    assert_eq!(
        actions,
        vec![
            "app.registry.registered",
            "app.registry.updated",
            "app.registry.deleted"
        ],
        "必须恰好这 3 个事件码（三段式，D163）：{actions:?}"
    );
    for (_, object_type, object_id, ip, user_agent, actor_id) in &rows {
        assert_eq!(object_type, "platform_app", "object_type 必须恰好是 platform_app");
        assert_eq!(object_id.as_deref(), Some(app_id.as_str()), "object_id 必须是 appId");
        assert!(
            !actor_id.is_empty(),
            "actor_id 必须非空（真实登录用户）"
        );
        // ip / user_agent 列 NOT NULL DEFAULT '' —— 这里不断言非空（请求没带那些头时是空串），
        // 只断言「列存在且不是 NULL」（由 schema 的 NOT NULL 保证）。
        let _ = (ip, user_agent);
    }

    // **哈希链自洽**。
    let verifier = platform_audit::ChainVerifier::new(
        platform_db::TenantDb::connect(&world.app_url, platform_db::DbOptions::default())
            .await
            .expect("连接串"),
    );
    let ctx = RequestContext::new().with_tenant(world.tenant.clone());
    let report = verifier.verify(&ctx).await.expect("链校验应能执行");
    assert!(report.ok, "哈希链必须自洽：{report:?}");
    assert!(report.broken.is_empty(), "不得有断裂：{report:?}");
    assert!(report.checked >= 3, "至少要检查 3 条：{report:?}");
}

// ---------------------------------------------------------------------------
// AC-S-162：导航（真 manifest + 真权限）
// ---------------------------------------------------------------------------

/// AC-S-162：持 `audit:events:read` → audit 项在；不持 → 消失但 app 项仍在。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn navigation_filters_module_items_by_real_permissions() {
    let world = world().await;
    // 一个带入口的应用（`launch_url` 非空 → 出现在导航）。
    world
        .seed_app(
            "app-nav",
            "第三方报销",
            "finance",
            10,
            "active",
            Some("https://expense.example/app"),
        )
        .await;

    // 管理员**持** `audit:events:read` → audit 项在 + app 项在。
    let admin_token = world.admin_token().await;
    let (status, body) = world
        .call("GET", "/api/apps/navigation", Some(&admin_token), None)
        .await;
    assert_eq!(status, 200, "导航必须 200：{body}");
    let items = body["items"].as_array().expect("必须有 items");
    assert!(
        items
            .iter()
            .any(|item| item["source"] == "module" && item["target"] == "/admin/audit"),
        "持 audit 权限时必须下发 audit 项：{items:?}"
    );
    assert!(
        items.iter().any(|item| item["source"] == "app"),
        "app 项必须存在：{items:?}"
    );

    // 只读用户**不持** audit → audit 项消失，app 项仍在。
    let read_token = world.read_token().await;
    let (status, body) = world
        .call("GET", "/api/apps/navigation", Some(&read_token), None)
        .await;
    assert_eq!(status, 200);
    let items = body["items"].as_array().expect("数组");
    assert!(
        !items.iter().any(|item| item["target"] == "/admin/audit"),
        "不持 audit 权限时 audit 项必须消失：{items:?}"
    );
    assert!(
        items.iter().any(|item| item["source"] == "app"),
        "app 项与权限点无关，必须仍在：{items:?}"
    );
    // `perm` 缺省的「工作台」项**总是**下发（D269）。
    assert!(
        items.iter().any(|item| item["target"] == "/workbench"),
        "perm 缺省的项必须总是下发：{items:?}"
    );
}
