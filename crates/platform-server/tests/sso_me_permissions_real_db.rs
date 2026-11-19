//! **AC-S-100 步骤 5**（rev 3.3，D128 / Q3）：SSO 登录拿到的 accessToken 打
//! `GET /api/rbac/me/permissions`，必须返回 **`bob` 的真实非空权限点**。
//!
//! # 为什么这条必须在这里（而不是 `platform-iam` 的用例里）
//!
//! AC 说的不是「登录响应里 `permissions` 是数组」，而是**拿 SSO 会话的令牌去问 RBAC**。
//! 那要求真实 IAM + 真实 RBAC 同时在场 —— 只有装配层有这个组合：
//! IAM 不依赖 RBAC（依赖方向 `rbac → iam`），权限点提供者与 `/api/rbac/*`
//! 都由**装配层**注入/挂载（`platform-server/src/main.rs` 的装配顺序）。
//!
//! # 原用例为什么不算（Q3）
//!
//! `platform-iam/tests/sso_oidc.rs` 只断言 `login["permissions"].is_array()` ——
//! 那是本节**预期 4** 的「恒为数组」。SSO 会话若拿到**空权限集**，那条断言照样绿。
//! 所以本文件断言：`len() > 0` **且包含 `bob` 角色被授的那个具体权限点**。
//!
//! # 会红的情形
//!
//! | 回归 | 哪条断言会红 |
//! |------|--------------|
//! | SSO 会话的 `user_id` / `tenant_id` 没进令牌 | `/me/permissions` 401 / 404 |
//! | SSO 登录没接上真实 RBAC（`permissions == []`） | `len() > 0` |
//! | `/me/permissions` 与登录响应用两套展开逻辑 | 两处集合不等 |
//! | 角色绑到了别的租户 | 本租户查不到权限点 + 401/404 |

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use platform_core::RequestContext;
use platform_core::extension::permission::{DataScope, PermissionProvider};
use platform_iam::config::{
    Argon2Settings, IamConfig, JwtSettings, LoginSettings, PasswordPolicySettings, TokenSettings,
    TotpSettings,
};
use platform_iam::http::IamState;
use platform_iam::service::IamService;
use platform_iam::store::IamStore;
use platform_rbac::entity::Role;
use platform_rbac::http::{IamTokenVerifier, RbacState};
use platform_rbac::{RbacChecker, RbacService, RbacStore, ScopePolicy};
use serde_json::{Value, json};
use tower::ServiceExt;

use support::*;

/// 测试密码（满足策略）。
const PASSWORD: &str = "correct-horse-battery-staple";
/// 「当前用户的有效权限点」端点。
const ME_PERMISSIONS_PATH: &str = "/api/rbac/me/permissions";
/// `bob` 的角色被授的**具体**权限点。
///
/// 取自迁移里已种下的 RBAC 权限点（`20261118000901_rbac_permission_seed.up.sql`），
/// **不新增注册表条目** —— 配额不该为测试扩充。
const PERMISSION_FOR_BOB: &str = "rbac:role:read";

// ---------------------------------------------------------------------------
// IAM 配置
// ---------------------------------------------------------------------------

fn test_config() -> IamConfig {
    let pem = platform_iam::jwt::generate_key_pair().expect("测试密钥");
    IamConfig {
        jwt: JwtSettings {
            issuer: "base".to_string(),
            audience: "base-api".to_string(),
            private_key_pem: pem,
            kid: "kid-test".to_string(),
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

// ---------------------------------------------------------------------------
// 进程内 mock IdP（发现文档 + PKCE 校验的 /token + /jwks）
// ---------------------------------------------------------------------------

/// 一次授权的登记内容（`/token` 用来校验 PKCE）。
#[derive(Clone)]
struct CodeEntry {
    nonce: String,
    challenge: String,
    redirect_uri: String,
}

#[derive(Clone)]
struct MiniIdp {
    issuer: String,
    state: Arc<MiniIdpState>,
}

struct MiniIdpState {
    issuer: String,
    /// `kid → (EncodingKey, Jwk)`
    keys: BTreeMap<String, (jsonwebtoken::EncodingKey, jsonwebtoken::jwk::Jwk)>,
    kid: String,
    codes: Mutex<BTreeMap<String, CodeEntry>>,
}

impl MiniIdp {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("应能绑定随机端口");
        let addr = listener.local_addr().expect("应能取本地地址");
        let issuer = format!("http://127.0.0.1:{}/oidc", addr.port());

        // 一把 ES256 密钥（与生产验签白名单一致）。
        let pem = platform_iam::jwt::generate_key_pair().expect("密钥");
        let encoding = jsonwebtoken::EncodingKey::from_ec_pem(pem.as_bytes()).expect("PEM");
        let mut jwk =
            jsonwebtoken::jwk::Jwk::from_encoding_key(&encoding, jsonwebtoken::Algorithm::ES256)
                .expect("应能导出公钥");
        jwk.common.key_id = Some("kid-test".to_string());

        let mut keys = BTreeMap::new();
        keys.insert("kid-test".to_string(), (encoding, jwk));

        let state = Arc::new(MiniIdpState {
            issuer: issuer.clone(),
            keys,
            kid: "kid-test".to_string(),
            codes: Mutex::new(BTreeMap::new()),
        });

        let app = axum::Router::new()
            .route("/oidc/.well-known/openid-configuration", get(discovery))
            .route("/oidc/jwks", get(jwks))
            .route("/oidc/token", post(token))
            .with_state(state.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self { issuer, state }
    }

    fn register_code(&self, code: &str, nonce: &str, challenge: &str, redirect_uri: &str) {
        self.state.codes.lock().expect("锁").insert(
            code.to_string(),
            CodeEntry {
                nonce: nonce.to_string(),
                challenge: challenge.to_string(),
                redirect_uri: redirect_uri.to_string(),
            },
        );
    }
}

async fn discovery(
    axum::extract::State(state): axum::extract::State<Arc<MiniIdpState>>,
) -> axum::response::Response {
    axum::Json(json!({
        "issuer": state.issuer,
        "authorization_endpoint": format!("{}/authorize", state.issuer),
        "token_endpoint": format!("{}/token", state.issuer),
        "jwks_uri": format!("{}/jwks", state.issuer),
    }))
    .into_response()
}

async fn jwks(
    axum::extract::State(state): axum::extract::State<Arc<MiniIdpState>>,
) -> axum::response::Response {
    let jwks = jsonwebtoken::jwk::JwkSet {
        keys: state.keys.values().map(|(_, jwk)| jwk.clone()).collect(),
    };
    axum::Json(jwks).into_response()
}

async fn token(
    axum::extract::State(state): axum::extract::State<Arc<MiniIdpState>>,
    body: String,
) -> axum::response::Response {
    let form: BTreeMap<String, String> = body
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (percent_decode(key), percent_decode(value)))
        .collect();

    let code = form.get("code").cloned().unwrap_or_default();
    let Some(entry) = state.codes.lock().expect("锁").remove(&code) else {
        return (StatusCode::BAD_REQUEST, "{\"error\":\"invalid_grant\"}").into_response();
    };

    // **真实** PKCE 校验：`S256(code_verifier) == code_challenge`。
    let verifier = form.get("code_verifier").cloned().unwrap_or_default();
    let digest = {
        use sha2::Digest as _;
        URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()))
    };
    if digest != entry.challenge {
        return (
            StatusCode::BAD_REQUEST,
            "{\"error\":\"invalid_grant\",\"error_description\":\"pkce\"}",
        )
            .into_response();
    }
    if form.get("redirect_uri") != Some(&entry.redirect_uri) {
        return (
            StatusCode::BAD_REQUEST,
            "{\"error\":\"invalid_grant\",\"error_description\":\"redirect\"}",
        )
            .into_response();
    }

    let (encoding, _) = state.keys.get(&state.kid).expect("当前密钥应存在");
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let header = jsonwebtoken::Header {
        alg: jsonwebtoken::Algorithm::ES256,
        kid: Some(state.kid.clone()),
        ..jsonwebtoken::Header::default()
    };
    let id_token = jsonwebtoken::encode(
        &header,
        &json!({
            "iss": state.issuer,
            "sub": "idp-bob-001",
            "aud": "client-1",
            "exp": now + 300,
            "iat": now,
            "nonce": entry.nonce,
            "email": "bob@example.com",
            "name": "Bob Example",
        }),
        encoding,
    )
    .expect("签名应成功");

    axum::Json(json!({
        "access_token": "mock-access-token",
        "token_type": "Bearer",
        "id_token": id_token,
    }))
    .into_response()
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("00");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            other => {
                out.push(other);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

// ---------------------------------------------------------------------------
// 装配：真实的 IAM + 真实的 RBAC（与 `main.rs` 同一顺序）
// ---------------------------------------------------------------------------

struct World {
    router: axum::Router,
    tenant: String,
    bob_id: String,
    admin: sqlx::PgPool,
    /// mock IdP（用例里登记授权码用）。
    idp: MiniIdp,
}

async fn world() -> World {
    let app_url = setup_database().await;
    let idp = MiniIdp::start().await;
    let tenant = unique_tenant("meperm");

    let tenant_db = platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    // 1) IAM（真实服务）+ SSO 配置源。
    let iam = Arc::new(
        IamService::new(IamStore::new(tenant_db.clone()), test_config()).expect("IAM 应当能构造"),
    );
    assert!(
        iam.set_sso_config_source(Arc::new(
            platform_iam::sso::config::StaticSsoConfigSource::new(
                platform_iam::sso::config::SsoConfig {
                    oidc: platform_iam::sso::config::OidcSettings {
                        enabled: true,
                        issuer: idp.issuer.clone(),
                        client_id: "client-1".to_string(),
                        scopes: "openid email profile".to_string(),
                        redirect_uri: "http://app.example.com/api/iam/sso/oidc/callback"
                            .to_string(),
                    },
                    ..Default::default()
                },
            )
        )),
        "SSO 配置源应能注入"
    );
    iam.set_sso_http_client(Arc::new(
        platform_iam::sso::oidc::OidcHttpClient::new().expect("http 客户端"),
    ));
    iam.set_sso_frontend_base_url("http://app.example.com");
    iam.set_sso_oidc_client_secret("SECRET-CANARY-mepermissions");

    // 2) RBAC（真实服务）。
    let rbac = Arc::new(RbacService::new(
        RbacStore::new(tenant_db.clone()),
        ScopePolicy::default(),
    ));

    // 3) **装配层的动作**：把 RBAC 注入 IAM 的权限点槽位。
    //    未注入时登录仍然成功、permissions 是空数组 —— 那正是本用例要排除的状态。
    let provider: Arc<dyn PermissionProvider> = rbac.clone();
    assert!(
        iam.set_permission_provider(provider),
        "权限点提供者应当注入成功"
    );

    // 4) 建本地 bob（走真实注册接口）+ 绑定 IdP 的 sub。
    let iam_state = Arc::new(IamState {
        service: iam.clone(),
    });
    let iam_router = axum::Router::new()
        .nest("/api/iam", platform_iam::http::router(iam_state.clone()))
        .nest(
            "/api/iam",
            platform_iam::http::sso_router(iam_state.clone()),
        );
    let register_response = iam_router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/register")
                .header("x-tenant-id", &tenant)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "username": "bob",
                        "password": PASSWORD,
                        "email": "bob@example.com",
                    })
                    .to_string(),
                ))
                .expect("请求"),
        )
        .await
        .expect("请求应能发出");
    assert_eq!(register_response.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(register_response.into_body(), 64 * 1024)
        .await
        .expect("响应体");
    let user: Value = serde_json::from_slice(&bytes).expect("应是 JSON");
    let bob_id = user["id"].as_str().expect("id").to_string();

    let admin = admin_pool(&app_url).await;
    sqlx::query(
        "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
         VALUES ($1, $2, 'oidc', 'idp-bob-001', $3) ON CONFLICT DO NOTHING",
    )
    .bind(&tenant)
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&bob_id)
    .execute(&admin)
    .await
    .expect("绑定应能写入");

    // 5) 给 bob 授一个**具体**权限点（自定义角色，避免动内置角色）。
    let ctx = RequestContext::new().with_tenant(tenant.clone());
    let role_id = format!("role-meperm-{}", uuid::Uuid::new_v4().simple());
    rbac.create_role(
        &ctx,
        &Role {
            tenant_id: tenant.clone(),
            id: role_id.clone(),
            code: role_id.clone(),
            name: role_id.clone(),
            data_scope: DataScope::All,
            custom_filter: None,
            use_all_memberships: false,
            builtin: false,
        },
    )
    .await
    .expect("角色应当能建成");
    rbac.grant_permission(&ctx, &role_id, PERMISSION_FOR_BOB, None)
        .await
        .expect("授权应当成功");
    rbac.bind_user_role(&ctx, &bob_id, &role_id)
        .await
        .expect("绑定用户角色应当成功");

    // 6) 路由：`/api/iam/*` + `/api/rbac/*`（令牌校验复用 IAM 的验签）。
    let rbac_state = Arc::new(RbacState {
        service: rbac.clone(),
        checker: Arc::new(RbacChecker::new(rbac.clone())),
        tokens: Arc::new(IamTokenVerifier::new(iam.clone())),
    });
    let router = axum::Router::new()
        .nest("/api/iam", platform_iam::http::router(iam_state.clone()))
        .nest(
            "/api/iam",
            platform_iam::http::sso_router(iam_state.clone()),
        )
        .nest("/api/rbac", platform_rbac::http::router(rbac_state));

    World {
        router,
        tenant,
        bob_id,
        admin,
        idp,
    }
}

async fn send(
    router: &axum::Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = router.clone().oneshot(request).await.expect("请求应能发出");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("响应体应能读");
    (status, headers, bytes.to_vec())
}

/// 管理员连接池（造数 / 直接读库断言）。
async fn admin_pool(app_url: &str) -> sqlx::PgPool {
    use sqlx::postgres::PgPoolOptions;
    let url = std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| panic!("缺少 DATABASE_URL_TEST"));
    let _ = app_url;
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("管理员连接应成功")
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

fn location_query(headers: &axum::http::HeaderMap) -> BTreeMap<String, String> {
    let location = headers
        .get("location")
        .and_then(|value| value.to_str().ok())
        .expect("响应必须带 Location");
    let query = location
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or("");
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

/// **AC-S-100 步骤 5**：SSO 会话的 accessToken → `GET /api/rbac/me/permissions`
/// → **非空**且含 `bob` 角色对应的具体权限点。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_100_step_5_an_sso_session_reaches_its_real_permission_points() {
    let world = world().await;

    // 1) authorize（顶层导航形态：`?tenant=`）。
    let path = format!("/api/iam/sso/oidc/authorize?tenant={}", world.tenant);
    let (status, headers, bytes) = send(
        &world.router,
        Request::builder()
            .method("GET")
            .uri(&path)
            .body(Body::empty())
            .expect("请求"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FOUND,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let query = location_query(&headers);
    let state = query.get("state").expect("state").clone();
    let nonce = query.get("nonce").expect("nonce").clone();
    let challenge = query.get("code_challenge").expect("challenge").clone();
    let redirect_uri = query.get("redirect_uri").expect("redirect_uri").clone();

    // 2) 模拟 IdP 授权完成，回调（**不带** `x-tenant-id`，见 P3）。
    let code = format!("code-{}", uuid::Uuid::new_v4().simple());
    world
        .idp
        .register_code(&code, &nonce, &challenge, &redirect_uri);
    let callback = format!("/api/iam/sso/oidc/callback?code={code}&state={state}");
    let (status, headers, bytes) = send(
        &world.router,
        Request::builder()
            .method("GET")
            .uri(&callback)
            .body(Body::empty())
            .expect("请求"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FOUND,
        "回调应 302 回前端：{}",
        String::from_utf8_lossy(&bytes)
    );
    let ticket_query = location_query(&headers);
    assert!(
        !ticket_query.contains_key("error"),
        "回调不得失败：{ticket_query:?}"
    );
    let ticket = ticket_query
        .get("ticket")
        .unwrap_or_else(|| panic!("回调没给 ticket：{ticket_query:?}"))
        .clone();

    // 3) 用 ticket 换会话（拿到 SSO 的 accessToken）。
    let (status, _, bytes) = send(
        &world.router,
        Request::builder()
            .method("POST")
            .uri("/api/iam/sso/exchange")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "ticket": ticket }).to_string()))
            .expect("请求"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let login = json_of(&bytes);
    let access = login["accessToken"]
        .as_str()
        .expect("SSO 登录必须给 accessToken")
        .to_string();
    assert_eq!(
        login["user"]["id"].as_str(),
        Some(world.bob_id.as_str()),
        "SSO 会话必须落在已绑定的 bob 上"
    );

    // 4) **本 AC 的核心**：拿这个令牌打 `/api/rbac/me/permissions`。
    let (status, _, bytes) = send(
        &world.router,
        Request::builder()
            .method("GET")
            .uri(ME_PERMISSIONS_PATH)
            .header("authorization", format!("Bearer {access}"))
            .header("x-tenant-id", world.tenant.clone())
            .body(Body::empty())
            .expect("请求"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "SSO 会话的令牌必须能打 /me/permissions：{}",
        String::from_utf8_lossy(&bytes)
    );
    let body = json_of(&bytes);
    let permissions: BTreeSet<String> = body["permissions"]
        .as_array()
        .unwrap_or_else(|| panic!("permissions 必须是数组：{body}"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("权限点必须是字符串：{body}"))
                .to_string()
        })
        .collect();

    // 4a) **非空** —— 空权限集在旧用例下也是「绿」的（Q3 的原缺口）。
    assert!(
        !permissions.is_empty(),
        "SSO 会话必须拿到真实权限点（空集正是本用例要防的假绿）：{body}"
    );
    // 4b) 含 bob 角色被授的**具体**权限点。
    assert!(
        permissions.contains(PERMISSION_FOR_BOB),
        "必须包含 bob 角色被授的 `{PERMISSION_FOR_BOB}`：{permissions:?}"
    );

    // 4c) 与登录响应里的权限点**逐项相同**（两个入口不得给出不同答案）。
    let login_permissions: BTreeSet<String> = login["permissions"]
        .as_array()
        .expect("登录响应 permissions 恒为数组")
        .iter()
        .map(|item| item.as_str().expect("字符串").to_string())
        .collect();
    assert_eq!(
        permissions, login_permissions,
        "登录响应与 /me/permissions 必须是同一份数据"
    );

    // 4d) 会话记录里 authored 的来源确实是 oidc（把「这是 SSO 会话」钉住）。
    let provider: String = sqlx::query_scalar(
        "SELECT auth_provider FROM platform_iam_session \
         WHERE tenant_id = $1 AND user_id = $2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&world.tenant)
    .bind(&world.bob_id)
    .fetch_one(&world.admin)
    .await
    .expect("应能读到会话行");
    assert_eq!(provider, "oidc", "本条必须是 SSO 会话（不是本地登录）");
}
