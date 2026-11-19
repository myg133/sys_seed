//! REQ-013 的测试支撑：真实库夹具 + 进程内 mock IdP + 可计数的 LDAP / 审计替身。
//!
//! # 为什么 mock IdP 起真实端口
//!
//! OIDC 的出站调用是**真实 HTTP**（hyper + TLS connector）。用 `oneshot` 驱动
//! Router 无法验证「connector 真的能连」—— 而那正是 D123 的验证点。
//! 所以这里起一个 `127.0.0.1:0` 的 axum 服务（https 场景再套一层 tokio-rustls）。
//!
//! # 为什么 LDAP 用 trait 替身而不是 mock 服务
//!
//! `ldap3` **没有**服务端/内存实现（S6）。所以 AC-S-104 的「0 次 connect / 0 次 bind」
//! 只能通过 [`SsoLdapClient`] 的可计数实现来观测（P12）。

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use platform_db::{DbOptions, TenantDb};
use platform_iam::config::{
    Argon2Settings, IamConfig, JwtSettings, LoginSettings, PasswordPolicySettings, TokenSettings,
    TotpSettings,
};
use platform_iam::http::IamState;
use platform_iam::service::{IamService, RequestMeta};
use platform_iam::sso::audit::{SsoAuditEvent, SsoAuditRecorder};
use platform_iam::sso::config::{
    LdapSettings, OidcSettings, SsoBoxFuture, SsoConfig, StaticSsoConfigSource,
};
use platform_iam::sso::ldap::{LdapEntry, SsoLdapClient, SsoLdapConn, SsoLdapError};
use platform_iam::store::IamStore;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

/// 测试密码（满足策略）。
pub const PASSWORD: &str = "correct-horse-battery-staple";

// ---------------------------------------------------------------------------
// 数据库夹具
// ---------------------------------------------------------------------------

/// 仓库根目录。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn admin_url() -> String {
    std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "缺少 DATABASE_URL_TEST：SSO 的真实库用例必须真的连库跑，不能静默跳过。\n  \
                 本地开发：先起库（bun run db:up），再跑 scripts/test-integration.sh。"
            )
        })
}

/// 应用角色连接串（RLS 前提：应用连接**不能**是超级用户）。
fn app_url(admin: &str) -> String {
    let (scheme, rest) = admin.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

/// 准备真实库：跑迁移 + 应用角色脚本，返回**应用角色**连接串。
pub async fn setup_database() -> String {
    let url = admin_url();
    let db = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    assert_eq!(db, "base_test", "只允许跑在 base_test 上（当前：{db}）");

    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}\n  先执行 'bun run db:up'"));

    let root = repo_root();
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&admin).await.expect("平台迁移应能应用");

    let role_sql = std::fs::read_to_string(root.join("deploy/seed/02-app-role.sql"))
        .expect("应用角色脚本应可读");
    sqlx::raw_sql(&role_sql)
        .execute(&admin)
        .await
        .expect("应用角色准备失败");

    app_url(&url)
}

/// 管理员连接池（测试要**直接改库**制造过期 / 改绑定）。
pub async fn admin_pool() -> sqlx::PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&admin_url())
        .await
        .expect("管理员连接应成功")
}

/// 一个每次运行都不同的租户 ID（用例之间不互相污染）。
pub fn unique_tenant(label: &str) -> String {
    format!("sso-{label}-{}", uuid::Uuid::new_v4().simple())
}

/// 一个唯一的 IP（限流桶是全局的 —— 共用 IP 会让用例互相顶到 429）。
pub fn unique_ip() -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
}

// ---------------------------------------------------------------------------
// 服务与路由
// ---------------------------------------------------------------------------

/// 测试用的 IAM 配置（Argon2 用低配值 —— 参数可配性由 `config.rs` 单测覆盖）。
pub fn test_config() -> IamConfig {
    let pem = platform_iam::jwt::generate_key_pair().expect("测试密钥应能生成");
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
            access_ttl: Duration::from_secs(900),
            refresh_ttl: Duration::from_secs(30 * 24 * 3600),
            reset_ttl: Duration::from_secs(3600),
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
            ip_window: Duration::from_secs(300),
            account_failure_limit: 5,
            lockout_duration: Duration::from_secs(900),
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

/// 一套完整的测试栈。
pub struct Stack {
    /// 服务（用来注入槽位）。
    pub service: Arc<IamService>,
    /// 路由（`/api/iam` + `/api/iam/sso` + JWKS）。
    pub router: Router,
    /// 应用角色连接串。
    pub app_url: String,
}

/// 建一套栈（配置源先给空配置）。
pub async fn build_stack(app_url: &str) -> Stack {
    let tenant_db = TenantDb::connect(app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let service = Arc::new(
        IamService::new(IamStore::new(tenant_db), test_config()).expect("服务应当能构造"),
    );
    let state = Arc::new(IamState {
        service: service.clone(),
    });
    let router = Router::new()
        .nest("/api/iam", platform_iam::http::router(state.clone()))
        .nest("/api/iam", platform_iam::http::sso_router(state.clone()))
        .merge(platform_iam::http::jwks_router(state));
    Stack {
        service,
        router,
        app_url: app_url.to_string(),
    }
}

/// 注入一份静态 SSO 配置（可在用例里改 —— AC-S-021 步骤 7 要改回 `enforce=false`）。
pub fn install_config(service: &IamService, config: SsoConfig) -> Arc<StaticSsoConfigSource> {
    let source = Arc::new(StaticSsoConfigSource::new(config));
    assert!(
        service.set_sso_config_source(source.clone()),
        "配置源应能注入"
    );
    source
}

/// 默认的 OIDC 配置（指向给定 issuer）。
pub fn oidc_settings(issuer: &str, redirect_uri: &str) -> OidcSettings {
    OidcSettings {
        enabled: true,
        issuer: issuer.to_string(),
        client_id: "client-1".to_string(),
        scopes: "openid email profile".to_string(),
        redirect_uri: redirect_uri.to_string(),
    }
}

/// 默认的 LDAP 配置。
pub fn ldap_settings(url: &str) -> LdapSettings {
    LdapSettings {
        enabled: true,
        url: url.to_string(),
        base_dn: "ou=people,dc=example,dc=com".to_string(),
        user_search_filter: "(uid={username})".to_string(),
        mail_attribute: "mail".to_string(),
    }
}

// ---------------------------------------------------------------------------
// HTTP 辅助
// ---------------------------------------------------------------------------

/// 发一个请求并返回 (状态码, 响应头, body)。
pub async fn send(
    router: &Router,
    request: axum::http::Request<axum::body::Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = router.clone().oneshot(request).await.expect("请求应能发出");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("响应体应能读");
    (status, headers, bytes.to_vec())
}

/// 发一个 GET（带可选租户头）。
pub async fn http_get(
    router: &Router,
    path: &str,
    tenant: Option<&str>,
    ip: Option<&str>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut builder = axum::http::Request::builder().method("GET").uri(path);
    if let Some(tenant) = tenant {
        builder = builder.header("x-tenant-id", tenant);
    }
    if let Some(ip) = ip {
        builder = builder.header("x-forwarded-for", ip);
    }
    let request = builder
        .header("user-agent", "sso-test-agent")
        .body(axum::body::Body::empty())
        .expect("请求应能构造");
    send(router, request).await
}

/// 发一个 POST JSON（带可选租户头）。
pub async fn post_json(
    router: &Router,
    path: &str,
    tenant: Option<&str>,
    ip: Option<&str>,
    body: Value,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(tenant) = tenant {
        builder = builder.header("x-tenant-id", tenant);
    }
    if let Some(ip) = ip {
        builder = builder.header("x-forwarded-for", ip);
    }
    let request = builder
        .header("user-agent", "sso-test-agent")
        .body(axum::body::Body::from(body.to_string()))
        .expect("请求应能构造");
    send(router, request).await
}

/// body → JSON（解析失败给 `Null`）。
pub fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

/// 解析 `Location` 的 query（不引 `url` crate）。
pub fn location_query(headers: &axum::http::HeaderMap) -> HashMap<String, String> {
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("响应必须带 Location");
    let query = location.split_once('?').map(|(_, query)| query).unwrap_or("");
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

/// 解析 `Location` 的 path（不含 query）。
pub fn location_path(headers: &axum::http::HeaderMap) -> String {
    let location = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("响应必须带 Location");
    location
        .split('?')
        .next()
        .unwrap_or_default()
        .to_string()
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

/// 注册一个本地账号（走真实接口），返回账号 JSON。
pub async fn register(
    router: &Router,
    tenant: &str,
    username: &str,
    password: &str,
    email: Option<&str>,
) -> Value {
    let body = json!({ "username": username, "password": password, "email": email });
    let (status, _, bytes) = post_json(router, "/api/iam/register", Some(tenant), None, body).await;
    assert_eq!(status, StatusCode::CREATED, "注册应成功：{}", String::from_utf8_lossy(&bytes));
    json_of(&bytes)
}

// ---------------------------------------------------------------------------
// Mock IdP（进程内 axum，真实端口）
// ---------------------------------------------------------------------------

/// 一次授权的信息（`/token` 用来校验 `code` 与 PKCE）。
#[derive(Debug, Clone)]
pub struct CodeEntry {
    /// 授权时的 nonce。
    pub nonce: String,
    /// 授权时的 PKCE challenge（S256）。
    pub challenge: String,
    /// 授权时的 redirect_uri。
    pub redirect_uri: String,
}

/// mock IdP 的可变行为（测试按需改）。
#[derive(Debug, Clone, Default)]
pub struct MockBehavior {
    /// 发现文档返回的状态码覆盖（`Some(500)` → 503 `DISCOVERY_FAILED`）。
    pub discovery_status: Option<u16>,
    /// 发现文档的原始 body 覆盖（`Some("{not json")` → `DISCOVERY_INVALID`）。
    pub discovery_body: Option<String>,
    /// token 端点返回的状态码覆盖（`Some(503)` → `TOKEN_ENDPOINT_UNAVAILABLE`）。
    pub token_status: Option<u16>,
    /// `/token` 返回的 `id_token` 完全覆盖（构造攻击样例时用）。
    pub id_token_override: Option<String>,
    /// 对 claims 的覆盖（`nonce` / `iss` / `aud` / `exp` 等）。
    pub claims_override: HashMap<String, Value>,
    /// 是否对 `/token` 做**真实** PKCE 校验（默认 true）。
    pub enforce_pkce: bool,
}

impl MockBehavior {
    /// 设定 `/token` 返回的 `sub` / `email`。
    pub fn insert_sub(&mut self, sub: &str, email: &str) {
        self.claims_override
            .insert("sub".to_string(), Value::String(sub.to_string()));
        self.claims_override
            .insert("email".to_string(), Value::String(email.to_string()));
    }
}

/// mock IdP。
#[derive(Clone)]
pub struct MockIdp {
    /// issuer（`http://127.0.0.1:<port>/oidc`）。
    pub issuer: String,
    /// 服务端地址。
    pub addr: SocketAddr,
    state: Arc<MockState>,
}

struct MockState {
    issuer: String,
    keys: Mutex<Vec<(String, EncodingKey, Jwk)>>,
    current_kid: Mutex<String>,
    codes: Mutex<HashMap<String, CodeEntry>>,
    behavior: Mutex<MockBehavior>,
    /// 攻击者的 JWKS 端点被请求的次数（AC-S-102 步骤 9/11 要断言 == 0）。
    attacker_hits: Arc<Mutex<u32>>,
    /// 攻击者 JWKS 端点的地址（jku 指向它）。
    attacker_url: Mutex<String>,
    /// **本 mock 的** `/jwks` 端点被请求的次数（AC-S-102 步骤 7：未知 kid 必须触发刷新）。
    jwks_hits: Arc<Mutex<u32>>,
}

impl MockIdp {
    /// 起一个 http mock IdP。
    pub async fn start_http() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("应能绑定随机端口");
        let addr = listener.local_addr().expect("应能取本地地址");
        let issuer = format!("http://127.0.0.1:{}/oidc", addr.port());
        let state = Arc::new(MockState::new(issuer.clone()));
        let router = mock_router(state.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Self {
            issuer,
            addr,
            state,
        }
    }

    /// 起一个 https mock IdP（自签根 + 测试根注入）。返回 (mock, 根证书 DER)。
    pub async fn start_https() -> (Self, Vec<u8>) {
        let (ca_der, server_config) = sso_tls::test_server_tls();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("应能绑定随机端口");
        let addr = listener.local_addr().expect("应能取本地地址");
        let issuer = format!("https://localhost:{}/oidc", addr.port());
        let state = Arc::new(MockState::new(issuer.clone()));
        let router = mock_router(state.clone());
        let tls_listener = TlsListener {
            inner: listener,
            acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(server_config)),
        };
        tokio::spawn(async move {
            let _ = axum::serve(tls_listener, router).await;
        });
        (
            Self {
                issuer,
                addr,
                state,
            },
            ca_der,
        )
    }

    /// 当前 kid。
    pub fn current_kid(&self) -> String {
        self.state.current_kid.lock().expect("锁").clone()
    }

    /// 轮换密钥（新增一把并设为当前）。
    pub fn rotate_keys(&self) {
        self.state.rotate();
    }

    /// 登记一次授权（mock IdP 在 `/authorize` 收到请求时调用；测试也可直接调）。
    pub fn register_code(&self, code: &str, nonce: &str, challenge: &str, redirect_uri: &str) {
        self.state.codes.lock().expect("锁").insert(
            code.to_string(),
            CodeEntry {
                nonce: nonce.to_string(),
                challenge: challenge.to_string(),
                redirect_uri: redirect_uri.to_string(),
            },
        );
    }

    /// 改行为。
    pub fn set_behavior(&self, mutate: impl FnOnce(&mut MockBehavior)) {
        let mut guard = self.state.behavior.lock().expect("锁");
        mutate(&mut guard);
    }

    /// 攻击者 JWKS 端点被请求的次数。
    pub fn attacker_hits(&self) -> u32 {
        *self.state.attacker_hits.lock().expect("锁")
    }

    /// **本 mock 的** `/jwks` 端点被请求的次数（AC-S-102 步骤 7：证明真的触发了刷新）。
    pub fn jwks_hits(&self) -> u32 {
        *self.state.jwks_hits.lock().expect("锁")
    }

    /// 攻击者 JWKS 端点 URL（`jku` 指向它）。
    pub fn attacker_jwks_url(&self) -> String {
        self.state.attacker_url.lock().expect("锁").clone()
    }

    /// 起一个「攻击者的 JWKS 端点」（只计数）。
    pub async fn start_attacker_jwks(&self) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("应能绑定");
        let addr = listener.local_addr().expect("应能取地址");
        let hits = self.state.attacker_hits.clone();
        let router = Router::new().route(
            "/jwks",
            get({
                let hits = hits.clone();
                move || {
                    let hits = hits.clone();
                    async move {
                        *hits.lock().expect("锁") += 1;
                        axum::Json(json!({ "keys": [] }))
                    }
                }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        *self.state.attacker_url.lock().expect("锁") =
            format!("http://127.0.0.1:{}/jwks", addr.port());
        addr
    }

    /// 用**当前**密钥签一个 id_token（正常路径用）。
    pub fn sign_id_token(&self, claims: Value) -> String {
        let kid = self.current_kid();
        let (_, encoding, _) = self
            .state
            .keys
            .lock()
            .expect("锁")
            .iter()
            .find(|(key_id, _, _)| *key_id == kid)
            .map(|(key_id, encoding, jwk)| (key_id.clone(), encoding.clone(), jwk.clone()))
            .expect("当前密钥应存在");
        let header = Header {
            alg: Algorithm::ES256,
            kid: Some(kid),
            ..Header::default()
        };
        jsonwebtoken::encode(&header, &claims, &encoding).expect("签名应成功")
    }

    /// 用**指定 kid** 的密钥签（未知 kid / 轮换场景）。
    pub fn sign_id_token_with_kid(&self, kid: &str, claims: Value) -> String {
        let (_, encoding, _) = self
            .state
            .keys
            .lock()
            .expect("锁")
            .iter()
            .find(|(key_id, _, _)| key_id == kid)
            .map(|(key_id, encoding, jwk)| (key_id.clone(), encoding.clone(), jwk.clone()))
            .expect("指定密钥应存在");
        let header = Header {
            alg: Algorithm::ES256,
            kid: Some(kid.to_string()),
            ..Header::default()
        };
        jsonwebtoken::encode(&header, &claims, &encoding).expect("签名应成功")
    }

    /// 默认 claims（合法）。
    pub fn default_claims(&self, nonce: &str) -> Value {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        json!({
            "iss": self.issuer,
            "sub": "idp-bob-001",
            "aud": "client-1",
            "exp": now + 300,
            "iat": now,
            "nonce": nonce,
            "email": "bob@example.com",
            "name": "Bob Example",
        })
    }
}

impl MockState {
    fn new(issuer: String) -> Self {
        let mut keys = Vec::new();
        let (kid, encoding, jwk) = new_key("kid-1");
        keys.push((kid.clone(), encoding, jwk));
        Self {
            issuer,
            keys: Mutex::new(keys),
            current_kid: Mutex::new(kid),
            codes: Mutex::new(HashMap::new()),
            behavior: Mutex::new(MockBehavior {
                enforce_pkce: true,
                ..MockBehavior::default()
            }),
            attacker_hits: Arc::new(Mutex::new(0)),
            attacker_url: Mutex::new(String::new()),
            jwks_hits: Arc::new(Mutex::new(0)),
        }
    }

    fn rotate(&self) {
        let next = format!("kid-{}", self.keys.lock().expect("锁").len() + 1);
        let (kid, encoding, jwk) = new_key(&next);
        self.keys.lock().expect("锁").push((kid.clone(), encoding, jwk));
        *self.current_kid.lock().expect("锁") = kid;
    }

    fn jwks(&self) -> JwkSet {
        JwkSet {
            keys: self
                .keys
                .lock()
                .expect("锁")
                .iter()
                .map(|(_, _, jwk)| jwk.clone())
                .collect(),
        }
    }
}

fn new_key(kid: &str) -> (String, EncodingKey, Jwk) {
    let pem = platform_iam::jwt::generate_key_pair().expect("密钥应能生成");
    let encoding = EncodingKey::from_ec_pem(pem.as_bytes()).expect("PEM 应可解析");
    let mut jwk = Jwk::from_encoding_key(&encoding, Algorithm::ES256).expect("应能导出公钥");
    jwk.common.key_id = Some(kid.to_string());
    (kid.to_string(), encoding, jwk)
}

fn mock_router(state: Arc<MockState>) -> Router {
    Router::new()
        .route("/oidc/.well-known/openid-configuration", get(discovery))
        .route("/oidc/jwks", get(jwks))
        .route("/oidc/token", post(token))
        .route("/oidc/authorize", get(authorize))
        .with_state(state)
}

async fn discovery(State(state): State<Arc<MockState>>) -> Response {
    let behavior = state.behavior.lock().expect("锁").clone();
    if let Some(body) = behavior.discovery_body {
        return (StatusCode::OK, body).into_response();
    }
    if let Some(status) = behavior.discovery_status {
        return (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), "{}")
            .into_response();
    }
    axum::Json(json!({
        "issuer": state.issuer,
        "authorization_endpoint": format!("{}/authorize", state.issuer),
        "token_endpoint": format!("{}/token", state.issuer),
        "jwks_uri": format!("{}/jwks", state.issuer),
    }))
    .into_response()
}

async fn jwks(State(state): State<Arc<MockState>>) -> Response {
    // 计数真实请求次数（AC-S-102 步骤 7：未知 kid 必须**再取一次** JWKS）。
    {
        let hits = state.jwks_hits.clone();
        *hits.lock().expect("锁") += 1;
    }
    axum::Json(state.jwks()).into_response()
}

async fn authorize(State(state): State<Arc<MockState>>) -> Response {
    // mock 的授权端点只用于「302 到 IdP」的落点断言；这里返回 200 占位。
    (StatusCode::OK, axum::Json(json!({ "status": "authorized", "issuer": state.issuer })))
        .into_response()
}

async fn token(
    State(state): State<Arc<MockState>>,
    body: String,
) -> Response {
    let behavior = state.behavior.lock().expect("锁").clone();
    if let Some(status) = behavior.token_status {
        return (
            StatusCode::from_u16(status).unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
            "{}",
        )
            .into_response();
    }

    let form: HashMap<String, String> = body
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (percent_decode(key), percent_decode(value)))
        .collect();

    let code = form.get("code").cloned().unwrap_or_default();
    let entry = state.codes.lock().expect("锁").remove(&code);
    let Some(entry) = entry else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({ "error": "invalid_grant", "error_description": "unknown code" })),
        )
            .into_response();
    };

    if form.get("redirect_uri") != Some(&entry.redirect_uri) {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({ "error": "invalid_grant", "error_description": "redirect mismatch" })),
        )
            .into_response();
    }

    if behavior.enforce_pkce {
        let verifier = form.get("code_verifier").cloned().unwrap_or_default();
        let digest = {
            use sha2::{Digest, Sha256};
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
        };
        if digest != entry.challenge {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({
                    "error": "invalid_grant",
                    "error_description": "PKCE verification failed"
                })),
            )
                .into_response();
        }
    }

    let mut claims = json!({
        "iss": state.issuer,
        "sub": "idp-bob-001",
        "aud": "client-1",
        "exp": time::OffsetDateTime::now_utc().unix_timestamp() + 300,
        "iat": time::OffsetDateTime::now_utc().unix_timestamp(),
        "nonce": entry.nonce,
        "email": "bob@example.com",
        "name": "Bob Example",
    });
    if let Some(object) = claims.as_object_mut() {
        for (key, value) in &behavior.claims_override {
            object.insert(key.clone(), value.clone());
        }
    }

    let id_token = match behavior.id_token_override {
        Some(token) => token,
        None => {
            let kid = state.current_kid.lock().expect("锁").clone();
            let encoding = state
                .keys
                .lock()
                .expect("锁")
                .iter()
                .find(|(key_id, _, _)| *key_id == kid)
                .map(|(_, encoding, _)| encoding.clone())
                .expect("当前密钥应存在");
            let header = Header {
                alg: Algorithm::ES256,
                kid: Some(kid),
                ..Header::default()
            };
            jsonwebtoken::encode(&header, &claims, &encoding).expect("签名应成功")
        }
    };

    axum::Json(json!({
        "access_token": "mock-access-token",
        "token_type": "Bearer",
        "id_token": id_token,
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// 服务端 TLS（P10）：自签根 + leaf，只信任注入的根
// ---------------------------------------------------------------------------

mod sso_tls {
    use std::sync::Arc;

    use rcgen::{
        BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose,
    };
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    /// 造一套「测试根 CA + localhost leaf」，返回 (根证书 DER, 服务端 rustls 配置)。
    pub fn test_server_tls() -> (Vec<u8>, rustls::ServerConfig) {
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("CA 参数");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "sso-test-ca");
        ca_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let ca_key = KeyPair::generate().expect("CA 密钥");
        let ca = ca_params.self_signed(&ca_key).expect("CA 自签");

        let mut leaf_params =
            CertificateParams::new(vec!["localhost".to_string()]).expect("leaf 参数");
        leaf_params
            .distinguished_name
            .push(DnType::CommonName, "localhost");
        let leaf_key = KeyPair::generate().expect("leaf 密钥");
        let leaf = leaf_params
            .signed_by(&leaf_key, &ca, &ca_key)
            .expect("leaf 签发");

        let cert_chain = vec![
            CertificateDer::from(leaf.der().to_vec()),
            CertificateDer::from(ca.der().to_vec()),
        ];
        let key = PrivateKeyDer::Pkcs8(leaf_key.serialize_der().into());

        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("协议版本")
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .expect("服务端证书应可用");

        (ca.der().to_vec(), config)
    }
}

/// 套了 TLS 的 axum `Listener`（只为了起 mock IdP 的 https 服务端）。
struct TlsListener {
    inner: tokio::net::TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
}

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let Ok((stream, addr)) = self.inner.accept().await else {
                continue;
            };
            if let Ok(tls) = self.acceptor.accept(stream).await {
                return (tls, addr);
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

// ---------------------------------------------------------------------------
// 可计数的 LDAP 替身（P12）
// ---------------------------------------------------------------------------

/// 目录里的一个用户。
#[derive(Debug, Clone)]
pub struct DirectoryUser {
    /// DN（= `external_id`）。
    pub dn: String,
    /// 密码。
    pub password: String,
    /// 映射属性（`mailAttribute`）。
    pub mail: Option<String>,
}

/// 记录 connect / search / bind 的**次数与参数**的 LDAP 客户端。
#[derive(Debug, Default)]
pub struct RecordingLdap {
    state: Arc<Mutex<RecordingState>>,
}

#[derive(Debug, Default)]
struct RecordingState {
    connects: u32,
    searches: Vec<(String, String, Vec<String>)>,
    binds: Vec<(String, String)>,
    unreachable: bool,
    users: Vec<DirectoryUser>,
    service_bind_ok: bool,
}

impl RecordingLdap {
    /// 构造（默认服务账号 bind 成功）。
    pub fn new() -> Self {
        let mut state = RecordingState {
            service_bind_ok: true,
            ..RecordingState::default()
        };
        state.users = Vec::new();
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// 加一个目录用户。
    pub fn add_user(&self, user: DirectoryUser) {
        self.state.lock().expect("锁").users.push(user);
    }

    /// 目录侧改密码。
    pub fn set_password(&self, dn: &str, password: &str) {
        let mut guard = self.state.lock().expect("锁");
        if let Some(user) = guard.users.iter_mut().find(|user| user.dn == dn) {
            user.password = password.to_string();
        }
    }

    /// 目录侧改映射属性（`mail`）。
    pub fn set_mail(&self, dn: &str, mail: Option<&str>) {
        let mut guard = self.state.lock().expect("锁");
        if let Some(user) = guard.users.iter_mut().find(|user| user.dn == dn) {
            user.mail = mail.map(str::to_string);
        }
    }

    /// 模拟「目录服务停掉了」（连接失败）。
    pub fn set_unreachable(&self, unreachable: bool) {
        self.state.lock().expect("锁").unreachable = unreachable;
    }

    /// 服务账号 bind 是否成功（模拟凭据错）。
    pub fn set_service_bind_ok(&self, ok: bool) {
        self.state.lock().expect("锁").service_bind_ok = ok;
    }

    /// connect 次数。
    pub fn connects(&self) -> u32 {
        self.state.lock().expect("锁").connects
    }

    /// search 次数。
    pub fn searches(&self) -> usize {
        self.state.lock().expect("锁").searches.len()
    }

    /// bind 次数。
    pub fn binds(&self) -> usize {
        self.state.lock().expect("锁").binds.len()
    }

    /// 全部 bind 的 (dn, password)。
    pub fn bind_calls(&self) -> Vec<(String, String)> {
        self.state.lock().expect("锁").binds.clone()
    }

    /// 全部 search 的 (base, filter, attrs)。
    pub fn search_calls(&self) -> Vec<(String, String, Vec<String>)> {
        self.state.lock().expect("锁").searches.clone()
    }
}

impl SsoLdapClient for RecordingLdap {
    fn connect(&self) -> SsoBoxFuture<'_, Result<Box<dyn SsoLdapConn>, SsoLdapError>> {
        {
            let mut state = self.state.lock().expect("锁");
            state.connects += 1;
            if state.unreachable {
                return Box::pin(async { Err(SsoLdapError::Connection) });
            }
        }
        let state = self.state.clone();
        Box::pin(async move {
            Ok(Box::new(RecordingConn { state }) as Box<dyn SsoLdapConn>)
        })
    }
}

struct RecordingConn {
    state: Arc<Mutex<RecordingState>>,
}

impl SsoLdapConn for RecordingConn {
    fn search(
        &mut self,
        base: &str,
        filter: &str,
        attrs: &[&str],
    ) -> SsoBoxFuture<'_, Result<Vec<LdapEntry>, SsoLdapError>> {
        let base = base.to_string();
        let filter = filter.to_string();
        let attrs: Vec<String> = attrs.iter().map(|value| (*value).to_string()).collect();
        let state = self.state.clone();
        Box::pin(async move {
            let mut guard = state.lock().expect("锁");
            guard
                .searches
                .push((base.clone(), filter.clone(), attrs.clone()));

            // 极简的 `(uid=<value>)` 匹配：只认**字面量**（转义已在生产侧完成）。
            let needle = filter
                .strip_prefix("(uid=")
                .and_then(|rest| rest.strip_suffix(')'))
                .unwrap_or_default()
                .replace("\\2a", "*")
                .replace("\\28", "(")
                .replace("\\29", ")")
                .replace("\\5c", "\\");
            let entries: Vec<LdapEntry> = guard
                .users
                .iter()
                .filter(|user| {
                    let uid = user
                        .dn
                        .split(',')
                        .next()
                        .unwrap_or_default()
                        .trim_start_matches("uid=");
                    uid == needle
                })
                .map(|user| {
                    let mut attributes = BTreeMap::new();
                    if let Some(mail) = &user.mail {
                        attributes.insert("mail".to_string(), vec![mail.clone()]);
                    }
                    LdapEntry::new(user.dn.clone(), attributes)
                })
                .collect();
            Ok(entries)
        })
    }

    fn simple_bind(
        &mut self,
        dn: &str,
        pw: &str,
    ) -> SsoBoxFuture<'_, Result<bool, SsoLdapError>> {
        let dn = dn.to_string();
        let pw = pw.to_string();
        let state = self.state.clone();
        Box::pin(async move {
            let mut guard = state.lock().expect("锁");
            guard.binds.push((dn.clone(), pw.clone()));

            // 服务账号（dn 为空）→ 按配置的开关。
            if dn.is_empty() {
                return Ok(guard.service_bind_ok);
            }
            let matched = guard
                .users
                .iter()
                .find(|user| user.dn == dn)
                .is_some_and(|user| user.password == pw);
            Ok(matched)
        })
    }
}

// ---------------------------------------------------------------------------
// 可记录的审计 recorder（P14）
// ---------------------------------------------------------------------------

/// 把事件原样记下来（测试断言键集合与敏感值）。
#[derive(Debug, Default)]
pub struct RecordingAudit {
    events: Arc<Mutex<Vec<SsoAuditEvent>>>,
}

impl RecordingAudit {
    /// 构造。
    pub fn new() -> Self {
        Self::default()
    }

    /// 已记录的事件。
    pub fn events(&self) -> Vec<SsoAuditEvent> {
        self.events.lock().expect("锁").clone()
    }

    /// 已记录事件的 JSON 载荷。
    pub fn payloads(&self) -> Vec<(String, Value)> {
        self.events
            .lock()
            .expect("锁")
            .iter()
            .map(|event| {
                (
                    event.code.to_string(),
                    serde_json::to_value(event).expect("事件应能序列化"),
                )
            })
            .collect()
    }
}

impl SsoAuditRecorder for RecordingAudit {
    fn record(&self, event: &SsoAuditEvent) {
        self.events.lock().expect("锁").push(event.clone());
    }
}

/// 构造 `RequestMeta`（与 HTTP 层的 `request_meta` 一致）。
pub fn request_meta(ip: &str) -> RequestMeta {
    RequestMeta {
        ip: ip.to_string(),
        user_agent: "sso-test-agent".to_string(),
        device_label: String::new(),
    }
}
