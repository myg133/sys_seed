//! REQ-022 的**装配层**判据：真实 IAM + 真实 RBAC + 真实审计 sink + 真实路由。
//!
//! # 这个文件守的两条 AC
//!
//! | AC | 判据 | 为什么必须在 `platform-server` |
//! |----|------|------------------------------|
//! | **AC-S-151** | 第三方 OAuth 令牌打 `/api/rbac/me/permissions` 与 `/api/audit/events` → **401 `UNAUTHENTICATED`**（且**不含** `error` 键） | 需要**真实的业务端点**（`platform-oauth` 的夹具里根本没有这些路由） |
//! | **AC-S-154** | 关键动作**真的落进** `platform_audit_log` + 哈希链自洽 + 从表里扫敏感值（带阳性对照） | 真实 `PgAuditSink` 只在装配层接得上（D150） |
//!
//! # 为什么「调一个函数」不算
//!
//! 复核 B-01：旧用例只写了
//! `fixture.iam.verify_access_token(&oauth_token).is_err()` ——
//! 那证明的是「IAM 的验签函数会拒」，**不是**「业务端点默认拒绝」。
//! 若有人给 `/api/rbac/me/permissions` 加一个「接受 OAuth 令牌」的分支，
//! 旧断言照样绿。→ 本文件**打真端点**，并带**端点级的阳性对照**。

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
            kid: "kid-oauth-stack".to_string(),
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
        secret_key: [7u8; 32],
    }
}

/// 装配好的一套真栈。
struct World {
    router: axum::Router,
    iam: Arc<IamService>,
    tenant: String,
    user_id: String,
    admin: sqlx::PgPool,
    app_url: String,
    /// 明文 client secret（P0-4 的「四处输出不得含明文」要用）。
    client_secret: String,
    client_id: String,
}

/// 建真栈：IAM + RBAC（真实权限表）+ 真审计 sink + 四条路由。
async fn world() -> World {
    let app_url = setup_database().await;
    let tenant = unique_tenant("oauth-stack");

    let tenant_db = platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    // **只**给本需求自己的 4 张表生成 RLS 策略。
    //
    // ⚠️ 不能对全部 manifest 跑一遍：`sandbox_tenant_item` 等表在共享的 `base_test`
    // 里可能是别的用例以别的角色建的 → `ALTER TABLE ... ENABLE ROW LEVEL SECURITY`
    // 会因「不是属主」失败（与本需求无关的环境噪声）。
    let admin_url = std::env::var("DATABASE_URL_TEST").expect("DATABASE_URL_TEST");
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&admin_url)
        .await
        .expect("管理员连接应当可建");
    for scope in platform_db::collect_scopes(&repo_root()).expect("租户作用域声明应能收集")
    {
        if scope
            .spec
            .tables
            .iter()
            .any(|table| table.starts_with("platform_oauth_"))
        {
            // ⚠️ 用**管理员**连接改 DDL（`ALTER TABLE` 要求属主）。
            let admin_db =
                platform_db::TenantDb::from_pool(admin.clone(), platform_db::DbOptions::default());
            platform_db::apply_policies(&admin_db, &scope.spec)
                .await
                .expect("OAuth 表的 RLS 策略应能应用");
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
    let provider: Arc<dyn platform_core::extension::permission::PermissionProvider> = rbac.clone();
    assert!(
        iam.set_permission_provider(provider),
        "权限点提供者应当注入成功"
    );

    // 3) 真审计 sink（AC-S-154：**不是**内存 Vec）。
    let audit_sink: Arc<dyn platform_core::extension::audit::AuditSink> =
        Arc::new(platform_audit::PgAuditSink::new(tenant_db.clone()));

    // 4) OAuth 服务（真 sink + 真权限源 + 静态运行期配置）。
    let oauth = Arc::new(platform_oauth::service::OAuthService::new(
        Arc::new(platform_oauth::store::OAuthStore::new(tenant_db.clone())),
        iam.clone(),
        Arc::new(platform_oauth::config::StaticOAuthConfigSource::new(
            platform_oauth::config::OAuthSettings::default(),
        )),
        rbac.clone(),
        audit_sink.clone(),
        Arc::new(platform_oauth::outbound::OutboundClient::default()),
    ));

    // 5) 用户：走**真实注册接口**（RBAC 的 user_role 有指向 iam_user 的外键）。
    let iam_state = Arc::new(IamState {
        service: iam.clone(),
    });
    let iam_router = axum::Router::new()
        .nest("/api/iam", platform_iam::http::router(iam_state.clone()))
        .merge(platform_iam::http::jwks_router(iam_state.clone()));
    let registered = iam_router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/register")
                .header("x-tenant-id", &tenant)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "username": "oauth-user",
                        "password": PASSWORD,
                        "email": "oauth-user@example.com",
                    })
                    .to_string(),
                ))
                .expect("请求"),
        )
        .await
        .expect("请求应能发出");
    assert_eq!(registered.status(), StatusCode::CREATED, "注册应当 201");
    let bytes = axum::body::to_bytes(registered.into_body(), 64 * 1024)
        .await
        .expect("响应体");
    let user: Value = serde_json::from_slice(&bytes).expect("应是 JSON");
    let user_id = user["id"].as_str().expect("id").to_string();

    // 6) 给该用户一个角色，授足两个 AC 需要的权限点。
    let ctx = RequestContext::new().with_tenant(tenant.clone());
    let role_id = format!("role-oauth-stack-{}", uuid::Uuid::new_v4().simple());
    rbac.create_role(
        &ctx,
        &Role {
            tenant_id: tenant.clone(),
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
    for permission in [
        "rbac:role:read",
        "audit:events:read",
        "oauth:client:read",
        "oauth:client:write",
        "oauth:grant:read",
        "oauth:grant:revoke",
    ] {
        rbac.grant_permission(&ctx, &role_id, permission, None)
            .await
            .unwrap_or_else(|error| panic!("授 {permission} 应当成功：{error}"));
    }
    rbac.bind_user_role(&ctx, &user_id, &role_id)
        .await
        .expect("绑定用户角色应当成功");

    // 7) 一个 client（`client_credentials`，scope = rbac:role:read）。
    let client_id = format!("client-stack-{}", uuid::Uuid::new_v4().simple());
    let client_secret = "stack-secret-value".to_string();
    let hash = platform_oauth::crypto::digest_base64url(client_secret.as_bytes());
    sqlx::query(
        "INSERT INTO platform_oauth_client \
         (client_id, client_secret_hash, client_type, name, redirect_uris, \
          allowed_grant_types, allowed_scopes, status, created_at) \
         VALUES ($1, $2, 'confidential', '装配层应用', \
                 ARRAY['https://client.example/cb'], \
                 ARRAY['client_credentials','authorization_code'], \
                 ARRAY['rbac:role:read'], 'active', now())",
    )
    .bind(&client_id)
    .bind(&hash)
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
    let audit_state = Arc::new(platform_server::audit::AuditState {
        query: Arc::new(platform_audit::AuditQueryService::new(tenant_db.clone())),
        verifier: Arc::new(platform_audit::ChainVerifier::new(tenant_db.clone())),
        archive: Arc::new(platform_audit::ArchiveService::new(tenant_db.clone())),
        tokens: Arc::new(platform_server::audit::IamTokenVerifier::new(iam.clone())),
        checker: checker.clone(),
        permissions: checker.clone(),
    });
    let oauth_state = Arc::new(platform_oauth::http::OAuthState {
        service: oauth.clone(),
        checker: checker.clone(),
        refresher: checker.clone(),
    });
    let router = axum::Router::new()
        .nest("/api/iam", platform_iam::http::router(iam_state.clone()))
        .nest("/api/rbac", platform_rbac::http::router(rbac_state))
        .nest("/api/audit", platform_server::audit::router(audit_state))
        .nest(
            "/api/oauth",
            platform_oauth::http::router(oauth_state.clone()),
        )
        .merge(platform_oauth::http::metadata_router(oauth_state));

    World {
        router,
        iam,
        tenant,
        user_id,
        admin,
        app_url,
        client_secret,
        client_id,
    }
}

impl World {
    /// 签一枚**真实**的登录令牌（同一套密钥；`sid` 固定便于断言）。
    fn login_token(&self) -> String {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        self.iam
            .keys()
            .sign(&AccessClaims {
                subject: self.user_id.clone(),
                tenant_id: self.tenant.clone(),
                session_id: "session-stack".to_string(),
                issuer: "base".to_string(),
                audience: "base-api".to_string(),
                issued_at: now,
                not_before: now,
                expires_at: now + 900,
                token_id: uuid::Uuid::new_v4().to_string(),
            })
            .expect("登录令牌应当能签发")
    }

    /// 发一个请求 → (状态码, 头, 原始字节)。
    async fn send(&self, request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("请求应能发出");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("响应体应能读");
        (status, headers, bytes.to_vec())
    }

    /// 用 OAuth 令牌或登录令牌打一个 GET。
    async fn get_with_token(
        &self,
        path: &str,
        token: &str,
        tenant: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut builder = Request::builder()
            .method("GET")
            .uri(path)
            .header("authorization", format!("Bearer {token}"));
        if let Some(tenant) = tenant {
            builder = builder.header("x-tenant-id", tenant);
        }
        let (status, headers, bytes) = self.send(builder.body(Body::empty()).expect("请求")).await;
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, headers, value)
    }

    /// 取一枚 **OAuth** access token（`client_credentials`）。
    async fn oauth_token(&self) -> String {
        let basic = {
            use base64::Engine as _;
            use base64::engine::general_purpose::STANDARD as BASE64;
            BASE64.encode(format!("{}:{}", self.client_id, self.client_secret).as_bytes())
        };
        let body = "grant_type=client_credentials&scope=rbac%3Arole%3Aread";
        let (status, _, bytes) = self
            .send(
                Request::builder()
                    .method("POST")
                    .uri("/api/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header("authorization", format!("Basic {basic}"))
                    .body(Body::from(body.to_string()))
                    .expect("请求"),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "换令牌应当 200");
        let value: Value = serde_json::from_slice(&bytes).expect("应是 JSON");
        value["access_token"]
            .as_str()
            .expect("应当有 access_token")
            .to_string()
    }
}

/// AC-S-151：**底座业务端点**默认拒绝第三方 OAuth 令牌（端到端）。
#[tokio::test]
async fn an_oauth_token_is_rejected_by_the_business_endpoints() {
    let world = world().await;
    let oauth_token = world.oauth_token().await;
    let tenant = world.tenant.clone();

    // 步骤 1：`/api/rbac/me/permissions` → **401 + UNAUTHENTICATED + 无 `error` 键**。
    let (status, headers, body) = world
        .get_with_token("/api/rbac/me/permissions", &oauth_token, Some(&tenant))
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "OAuth 令牌打业务端点必须 401：{body}"
    );
    assert_eq!(body["code"], "UNAUTHENTICATED", "错误码必须钉死（D224）");
    assert!(
        body.get("error").is_none(),
        "业务端点的错误体**不得**有 error 键（那是协议话）：{body}"
    );
    assert!(
        headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .starts_with("application/json"),
        "错误体必须是 JSON"
    );

    // 步骤 2：`/api/audit/events` → 同上。
    let (status, _, body) = world
        .get_with_token("/api/audit/events", &oauth_token, Some(&tenant))
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "OAuth 令牌打审计端点必须 401：{body}"
    );
    assert_eq!(body["code"], "UNAUTHENTICATED");
    assert!(body.get("error").is_none(), "不得用协议错误体：{body}");

    // 步骤 3：**同一个**端点再来一次（AC 特意写了两遍）。
    let (status, _, body) = world
        .get_with_token("/api/rbac/me/permissions", &oauth_token, Some(&tenant))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    // 步骤 3.5：该令牌在 **OAuth 协议端点**上**有效**（200 + active）。
    let basic = {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;
        BASE64.encode(format!("{}:{}", world.client_id, world.client_secret).as_bytes())
    };
    let (status, _, bytes) = world
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/oauth/introspect")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("authorization", format!("Basic {basic}"))
                .body(Body::from(format!("token={oauth_token}")))
                .expect("请求"),
        )
        .await;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::OK, "内省应当 200：{body}");
    assert_eq!(
        body["active"], true,
        "该令牌只在 OAuth 协议端点上有效：{body}"
    );

    // 步骤 5：**阳性对照** —— 同一个用户用**登录令牌**必须**成功**。
    // 若下面也 401，说明整个平台坏了 → 上面那些 401 **没有区分力**。
    let login = world.login_token();
    let (status, _, body) = world
        .get_with_token("/api/rbac/me/permissions", &login, Some(&tenant))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "阳性对照失败：登录令牌必须能读 /me/permissions：{body}"
    );
    let permissions = body["permissions"]
        .as_array()
        .expect("应当是数组")
        .iter()
        .filter_map(|item| item.as_str())
        .collect::<Vec<_>>();
    assert!(
        permissions.contains(&"audit:events:read"),
        "阳性对照：该用户的权限点应当含 audit:events:read：{permissions:?}"
    );
    // D241 第 2 条的可判形式：有效权限点集合里**不含**通配。
    assert!(
        !permissions.iter().any(|code| code.contains('*')),
        "有效权限点集合不得含通配：{permissions:?}"
    );

    let (status, _, body) = world
        .get_with_token("/api/audit/events", &login, Some(&tenant))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "阳性对照：登录令牌必须能读 /api/audit/events：{body}"
    );
}

/// AC-S-154：关键动作**真的落库** + 哈希链自洽 + 从表里扫敏感值（带阳性对照）。
#[tokio::test]
async fn the_oauth_actions_land_in_the_audit_table_with_an_intact_chain() {
    let world = world().await;
    let tenant = world.tenant.clone();
    let login = world.login_token();

    // 走一遍：注册 client → 同意 → 换令牌 → 撤销 → 登出。
    let (status, _, bytes) = world
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/oauth/clients")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {login}"))
                .header("x-tenant-id", &tenant)
                .body(Body::from(
                    json!({
                        "name": "审计用应用",
                        "clientType": "confidential",
                        "redirectUris": ["https://client.example/cb"],
                        "allowedGrantTypes": ["authorization_code"],
                        "allowedScopes": ["rbac:role:read"]
                    })
                    .to_string(),
                ))
                .expect("请求"),
        )
        .await;
    let created: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(
        status,
        StatusCode::CREATED,
        "注册 client 应当 201：{created}"
    );
    let new_client = created["clientId"]
        .as_str()
        .expect("应当有 clientId")
        .to_string();
    let new_secret = created["clientSecret"]
        .as_str()
        .expect("应当有一次性明文 secret")
        .to_string();

    // 同意（preview → confirm）。
    let code_challenge = {
        use base64::Engine as _;
        use sha2::Digest as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(
            b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
        ))
    };
    let (_, _, bytes) = world
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/oauth/authorize/preview")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {login}"))
                .header("x-tenant-id", &tenant)
                .body(Body::from(
                    json!({
                        "responseType": "code",
                        "clientId": new_client,
                        "redirectUri": "https://client.example/cb",
                        "scope": "rbac:role:read",
                        "codeChallenge": code_challenge,
                        "codeChallengeMethod": "S256"
                    })
                    .to_string(),
                ))
                .expect("请求"),
        )
        .await;
    let preview: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let ticket = preview["ticket"].as_str().expect("应当有票据").to_string();
    let (_, _, bytes) = world
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/oauth/authorize/confirm")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {login}"))
                .header("x-tenant-id", &tenant)
                .body(Body::from(json!({ "ticket": ticket }).to_string()))
                .expect("请求"),
        )
        .await;
    let confirmed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let redirect = confirmed["redirectTo"].as_str().expect("应当有 redirectTo");
    let code = redirect
        .split("code=")
        .nth(1)
        .expect("应当有 code")
        .split('&')
        .next()
        .expect("code 应当有值")
        .to_string();

    // 换令牌。
    let basic = {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;
        BASE64.encode(format!("{new_client}:{new_secret}").as_bytes())
    };
    let (status, _, bytes) = world
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/oauth/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("authorization", format!("Basic {basic}"))
                .body(Body::from(format!(
                    "grant_type=authorization_code&code={code}\
                     &redirect_uri=https%3A%2F%2Fclient.example%2Fcb\
                     &code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
                )))
                .expect("请求"),
        )
        .await;
    let issued: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::OK, "换令牌应当 200：{issued}");

    // grant 级撤销（`oauth.token.revoked`）。
    let (status, _, bytes) = world
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/oauth/grants/revoke")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {login}"))
                .header("x-tenant-id", &tenant)
                .body(Body::from(json!({ "clientId": new_client }).to_string()))
                .expect("请求"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "撤销应当 200：{bytes:?}");

    // 登出。
    let (status, _, _) = world
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/oauth/logout")
                .header("authorization", format!("Bearer {login}"))
                .header("x-tenant-id", &tenant)
                .body(Body::empty())
                .expect("请求"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "登出应当 200");

    // ---- 步骤 2：查**审计表**（不是内存 Vec）----
    let rows: Vec<(
        String,
        Option<String>,
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        String,
        String,
    )> = sqlx::query_as(
        "SELECT action, actor_id, object_type, object_id, ip, user_agent, \
                created_at::text, prev_hash, hash, tenant_id \
         FROM platform_audit_log WHERE tenant_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(&tenant)
    .fetch_all(&world.admin)
    .await
    .expect("应当能读审计表");
    let actions: Vec<String> = rows.iter().map(|row| row.0.clone()).collect();
    for expected in [
        "oauth.client.registered",
        "oauth.authorization.approved",
        "oauth.token.issued",
        "oauth.token.revoked",
        "oauth.logout.executed",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "审计表里缺少事件码 {expected}：{actions:?}"
        );
    }
    assert!(rows.len() >= 5, "至少 5 条审计行，实际 {} 条", rows.len());

    // ---- 步骤 2（续）：契约 C12 的固定字段集 ----
    for (
        action,
        actor_id,
        object_type,
        object_id,
        ip,
        user_agent,
        created_at,
        prev_hash,
        hash,
        _,
    ) in &rows
    {
        assert!(!action.is_empty(), "action 不得为空");
        assert!(!object_type.is_empty(), "object_type 不得为空");
        assert!(!created_at.is_empty(), "created_at 不得为空");
        assert_eq!(prev_hash.len(), 64, "prev_hash 必须是 64 位十六进制");
        assert_eq!(hash.len(), 64, "hash 必须是 64 位十六进制");
        // `actor_id` / `object_id` / `ip` / `user_agent` 在协议端点事件上可能为空
        // （client 的后端调用没有用户、没有 UA）→ 这里只要求**列存在且是 TEXT**。
        let _ = (actor_id, object_id, ip, user_agent);
    }
    // 用户主动发起的事件必须带 actor_id。
    let actor_for_registered = rows
        .iter()
        .find(|row| row.0 == "oauth.client.registered")
        .expect("应当有 client.registered");
    assert_eq!(
        actor_for_registered.1.as_deref(),
        Some(world.user_id.as_str()),
        "client.registered 的 actor_id 必须是发起注册的用户"
    );

    // ---- 步骤 4：哈希链自洽（`ChainVerifier::verify` → `ok == true`）----
    let ctx = RequestContext::new().with_tenant(tenant.clone());
    let verifier = platform_audit::ChainVerifier::new(
        platform_db::TenantDb::connect(&world.app_url, platform_db::DbOptions::default())
            .await
            .expect("应用角色连接"),
    );
    let report = verifier.verify(&ctx).await.expect("链校验应当能跑");
    assert!(report.ok, "哈希链必须自洽（断裂：{:?}）", report.broken);
    assert!(
        report.checked >= rows.len(),
        "链校验必须真的检查了那些行（checked={} rows={}",
        report.checked,
        rows.len()
    );

    // ---- 步骤 3：扫审计表的**全部文本列** → 真实数据 0 命中 ----
    let scan_rows = |admin: sqlx::postgres::PgPool, tenant: String| async move {
        sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT action, object_type, COALESCE(object_id, ''), \
                    COALESCE(\"before\", ''), COALESCE(\"after\", '') \
             FROM platform_audit_log WHERE tenant_id = $1 ORDER BY created_at ASC, id ASC",
        )
        .bind(tenant)
        .fetch_all(&admin)
        .await
        .expect("应当能读审计行")
    };
    let hits_of = |rows: &[(String, String, String, String, String)]| {
        let mut fields: Vec<(&str, &str)> = Vec::new();
        for (action, object_type, object_id, before, after) in rows {
            fields.push(("action", action.as_str()));
            fields.push(("object_type", object_type.as_str()));
            fields.push(("object_id", object_id.as_str()));
            fields.push(("before", before.as_str()));
            fields.push(("after", after.as_str()));
        }
        platform_oauth::sensitive::scan_sensitive_fields(&fields)
    };

    let real_rows = scan_rows(world.admin.clone(), tenant.clone()).await;
    let hits = hits_of(&real_rows);
    assert!(hits.is_empty(), "真实审计数据不得命中敏感键：{hits:?}");

    // ---- 步骤 4（阳性对照）：管理员直连插一条含假 secret 的行 ----
    let control_id = format!("audit-control-{}", uuid::Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO platform_audit_log \
         (id, tenant_id, actor_id, action, object_type, object_id, \"before\", \"after\", \
          ip, user_agent, created_at, prev_hash, hash) \
         VALUES ($1, $2, 'positive-control', 'test.control', 'oauth_client', $1, \
                 '{\"client_secret\":\"POSITIVE-CONTROL\"}', NULL, '', '', now(), \
                 repeat('0', 64), repeat('a', 64))",
    )
    .bind(&control_id)
    .bind(&tenant)
    .execute(&world.admin)
    .await
    .expect("阳性对照行应当能插入");
    let with_control = scan_rows(world.admin.clone(), tenant.clone()).await;
    let control_hits = hits_of(&with_control);
    assert!(
        !control_hits.is_empty(),
        "阳性对照失败：插了含假 secret 的行，扫描器**必须**命中"
    );

    // ---- 删掉对照行 → 必须不再命中 ----
    sqlx::query("DELETE FROM platform_audit_log WHERE id = $1 AND tenant_id = $2")
        .bind(&control_id)
        .bind(&tenant)
        .execute(&world.admin)
        .await
        .expect("对照行应当能删除");
    let after_delete = scan_rows(world.admin.clone(), tenant.clone()).await;
    assert!(
        hits_of(&after_delete).is_empty(),
        "删掉对照行后必须不再命中"
    );

    // ---- 额外一面（AC-S-136 步骤 4）：审计表文本不得含 client 明文 secret ----
    let mut all_text: Vec<String> = Vec::new();
    for (action, object_type, object_id, before, after) in &after_delete {
        all_text.push(format!(
            "{action} {object_type} {object_id} {before} {after}"
        ));
    }
    // 也把新建 client 的明文 secret 加进来一起比对（它**不该**出现在审计表里）。
    let haystack = all_text.join("\n");
    assert!(
        !haystack.contains(&new_secret),
        "审计表文本不得含明文 client_secret"
    );

    // ---- 同一面走**端点**（AC-S-136 步骤 4 的字面要求：`GET /api/audit/events`）----
    let (status, _, body) = world
        .get_with_token("/api/audit/events", &world.login_token(), Some(&tenant))
        .await;
    assert_eq!(status, StatusCode::OK, "审计查询应当 200：{body}");
    assert!(
        !body.to_string().contains(&new_secret),
        "`GET /api/audit/events` 的响应体不得含明文 client_secret"
    );
}
