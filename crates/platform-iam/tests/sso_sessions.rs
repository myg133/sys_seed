//! AC-S-107（SSO 会话与本地会话**同构**）与 AC-S-109（多 IdP 并存与租户隔离）。
//!
//! # 为什么放在 `platform-iam` 的测试里
//!
//! 这两条要用到同一套真实库夹具 + mock IdP（design §8 把 `sso_real_db.rs` 放在
//! `platform-server`；这里是**同一批断言的会话/隔离部分**，装配层那部分见
//! `platform-server/tests/sso_real_db.rs`）。夹具复用避免两套夹具漂移。

mod sso_support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use platform_core::RequestContext;
use platform_iam::sso::SsoProviderView;
use platform_iam::sso::config::{SsoBoxFuture, SsoConfig, SsoConfigError, SsoConfigSource};
use serde_json::json;

use sso_support::*;

const CALLBACK_URI: &str = "http://app.example.com/api/iam/sso/oidc/callback";
const LDAP_URL: &str = "ldap://127.0.0.1:389";
const ERIN_DN: &str = "uid=erin,ou=people,dc=example,dc=com";
const ERIN_PASSWORD: &str = "Er1n-pass";

/// 按租户返回不同配置的配置源（AC-S-109 需要 a/b/c 三个租户配置不同）。
#[derive(Clone, Default)]
struct TenantScopedConfig {
    by_tenant: Arc<Mutex<HashMap<String, SsoConfig>>>,
    fallback: Arc<Mutex<SsoConfig>>,
}

impl TenantScopedConfig {
    fn set(&self, tenant: &str, config: SsoConfig) {
        self.by_tenant
            .lock()
            .expect("锁")
            .insert(tenant.to_string(), config);
    }

    fn set_fallback(&self, config: SsoConfig) {
        *self.fallback.lock().expect("锁") = config;
    }
}

impl SsoConfigSource for TenantScopedConfig {
    fn read(&self, ctx: &RequestContext) -> SsoBoxFuture<'_, Result<SsoConfig, SsoConfigError>> {
        let tenant = ctx.tenant_id().unwrap_or_default().to_string();
        let config = self
            .by_tenant
            .lock()
            .expect("锁")
            .get(&tenant)
            .cloned()
            .unwrap_or_else(|| self.fallback.lock().expect("锁").clone());
        Box::pin(async move { Ok(config) })
    }
}

struct Env {
    stack: Stack,
    idp_a: MockIdp,
    /// tenant-b 的 IdP（AC-S-109 只用它的 issuer；登录走 `idp_a` 的授权链路）。
    #[allow(dead_code)]
    idp_b: MockIdp,
    ldap: Arc<RecordingLdap>,
    tenant_a: String,
    tenant_b: String,
    tenant_c: String,
    config: TenantScopedConfig,
    admin: sqlx::PgPool,
}

impl Env {
    /// 建 a / b / c 三个租户：a = OIDC + LDAP，b = 另一个 OIDC，c = 什么都没配。
    async fn new() -> Self {
        let app_url = setup_database().await;
        let stack = build_stack(&app_url).await;
        let idp_a = MockIdp::start_http().await;
        let idp_b = MockIdp::start_http().await;
        let tenant_a = unique_tenant("iso-a");
        let tenant_b = unique_tenant("iso-b");
        let tenant_c = unique_tenant("iso-c");

        let ldap = Arc::new(RecordingLdap::new());
        ldap.add_user(DirectoryUser {
            dn: ERIN_DN.to_string(),
            password: ERIN_PASSWORD.to_string(),
            mail: Some("erin@example.com".to_string()),
        });
        assert!(stack.service.set_sso_ldap_client(ldap.clone()));
        stack
            .service
            .set_sso_http_client(Arc::new(
                platform_iam::sso::oidc::OidcHttpClient::new().expect("http 客户端"),
            ));
        stack.service.set_sso_frontend_base_url("http://app.example.com");
        stack.service.set_sso_oidc_client_secret("SECRET-CANARY-iso");

        let config = TenantScopedConfig::default();
        config.set_fallback(SsoConfig::default());
        config.set(
            &tenant_a,
            SsoConfig {
                oidc: oidc_settings(&idp_a.issuer, CALLBACK_URI),
                ldap: ldap_settings(LDAP_URL),
                ..Default::default()
            },
        );
        config.set(
            &tenant_b,
            SsoConfig {
                oidc: oidc_settings(&idp_b.issuer, CALLBACK_URI),
                ..Default::default()
            },
        );
        assert!(stack.service.set_sso_config_source(Arc::new(config.clone())));

        let admin = admin_pool().await;
        // 前置：bob 在 a 里绑定；erin 在 a 里绑定（LDAP）。
        for (tenant, username, email, provider, external_id) in [
            (&tenant_a, "bob", "bob@example.com", "oidc", "idp-bob-001"),
            (&tenant_a, "erin", "erin@example.com", "ldap", ERIN_DN),
            (&tenant_b, "bob", "bob@example.com", "oidc", "idp-bob-001"),
        ] {
            let user = register(&stack.router, tenant, username, PASSWORD, Some(email)).await;
            let user_id = user["id"].as_str().expect("id").to_string();
            sqlx::query(
                "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
                 VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            )
            .bind(tenant)
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(provider)
            .bind(external_id)
            .bind(&user_id)
            .execute(&admin)
            .await
            .expect("绑定应能写入");
        }

        Self {
            stack,
            idp_a,
            idp_b,
            ldap,
            tenant_a,
            tenant_b,
            tenant_c,
            config,
            admin,
        }
    }

    async fn oidc_login(&self, tenant: &str, idp: &MockIdp) -> serde_json::Value {
        let path = format!("/api/iam/sso/oidc/authorize?tenant={tenant}");
        let (status, headers, _) = http_get(&self.stack.router, &path, None, None).await;
        assert_eq!(status, StatusCode::FOUND);
        let query = location_query(&headers);
        let code = format!("code-{}", uuid::Uuid::new_v4().simple());
        idp.register_code(
            &code,
            &query["nonce"],
            &query["code_challenge"],
            &query["redirect_uri"],
        );
        let callback = format!(
            "/api/iam/sso/oidc/callback?code={code}&state={}",
            query["state"]
        );
        let (status, headers, bytes) = http_get(&self.stack.router, &callback, None, None).await;
        assert_eq!(status, StatusCode::FOUND, "{}", String::from_utf8_lossy(&bytes));
        let ticket = location_query(&headers)["ticket"].clone();
        let (status, _, bytes) = post_json(
            &self.stack.router,
            "/api/iam/sso/exchange",
            None,
            None,
            json!({ "ticket": ticket }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
        json_of(&bytes)
    }

    async fn sessions(&self, tenant: &str, access: &str) -> serde_json::Value {
        let (status, _, bytes) = send(
            &self.stack.router,
            axum::http::Request::builder()
                .method("GET")
                .uri("/api/iam/sessions")
                .header("authorization", format!("Bearer {access}"))
                .header("x-tenant-id", tenant)
                .body(axum::body::Body::empty())
                .expect("请求应能构造"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
        json_of(&bytes)
    }
}

fn jwt_header(token: &str) -> serde_json::Value {
    let header = token.split('.').next().expect("JWT 头");
    let bytes = URL_SAFE_NO_PAD
        .decode(header)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(header))
        .expect("JWT 头应可解码");
    serde_json::from_slice(&bytes).expect("JWT 头应是 JSON")
}

fn jwt_claims(token: &str) -> serde_json::Value {
    let payload = token.split('.').nth(1).expect("JWT payload");
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .expect("JWT payload 应可解码");
    serde_json::from_slice(&bytes).expect("JWT payload 应是 JSON")
}

// ---------------------------------------------------------------------------
// AC-S-107
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_107_sso_and_local_sessions_share_one_shape() {
    let env = Env::new().await;

    // 本地账号 grace 登录。
    register(&env.stack.router, &env.tenant_a, "grace", PASSWORD, Some("grace@example.com")).await;
    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/login",
        Some(&env.tenant_a),
        Some(&unique_ip()),
        json!({ "username": "grace", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let local = json_of(&bytes);

    // SSO 账号 bob 登录。
    let sso = env.oidc_login(&env.tenant_a, &env.idp_a).await;

    let local_sessions = env
        .sessions(&env.tenant_a, local["accessToken"].as_str().expect("accessToken"))
        .await;
    let sso_sessions = env
        .sessions(&env.tenant_a, sso["accessToken"].as_str().expect("accessToken"))
        .await;

    let local_row = local_sessions.as_array().expect("数组")[0].clone();
    let sso_row = sso_sessions.as_array().expect("数组")[0].clone();

    // 键集合完全相同（同一个视图类型）。
    let keys = |value: &serde_json::Value| -> std::collections::BTreeSet<String> {
        value.as_object().expect("对象").keys().cloned().collect()
    };
    assert_eq!(
        keys(&local_row),
        keys(&sso_row),
        "两条会话的键集合必须完全相同"
    );

    // `authProvider` 是**唯一允许取值不同**的字段。
    assert_eq!(local_row["authProvider"], "local");
    assert_eq!(sso_row["authProvider"], "oidc");
    for key in keys(&local_row) {
        if key == "authProvider" {
            continue;
        }
        // 其余字段只允许「值不同」，键必须都在（上面已断言），这里再确认不是 null。
        assert!(
            !local_row[&key].is_null() && !sso_row[&key].is_null() || key == "revokedAt",
            "{key} 不应为 null"
        );
    }

    // access token 的 header / claim 结构完全一致（没有第二套 token 体系）。
    let local_token = local["accessToken"].as_str().expect("token");
    let sso_token = sso["accessToken"].as_str().expect("token");
    let (local_header, sso_header) = (jwt_header(local_token), jwt_header(sso_token));
    assert_eq!(local_header["alg"], sso_header["alg"]);
    assert_eq!(local_header["kid"], sso_header["kid"]);
    assert_eq!(
        keys(&jwt_claims(local_token)),
        keys(&jwt_claims(sso_token)),
        "claim 结构必须一致"
    );

    // 无「SSO 专用会话表」：两条会话都只在 `platform_iam_session`。
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_iam_session WHERE tenant_id = $1 AND revoked_at IS NULL",
    )
    .bind(&env.tenant_a)
    .fetch_one(&env.admin)
    .await
    .expect("count sessions");
    assert_eq!(rows, 2, "两条会话应在同一张表里");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_107_deleting_a_session_revokes_the_family_and_logout_all_covers_sso() {
    let env = Env::new().await;
    let sso = env.oidc_login(&env.tenant_a, &env.idp_a).await;
    let access = sso["accessToken"].as_str().expect("accessToken").to_string();
    let refresh = sso["refreshToken"].as_str().expect("refreshToken").to_string();

    let sessions = env.sessions(&env.tenant_a, &access).await;
    let session_id = sessions.as_array().expect("数组")[0]["id"]
        .as_str()
        .expect("id")
        .to_string();

    // DELETE → 204 且响应体长度为 0。
    let (status, _, bytes) = send(
        &env.stack.router,
        axum::http::Request::builder()
            .method("DELETE")
            .uri(format!("/api/iam/sessions/{session_id}"))
            .header("authorization", format!("Bearer {access}"))
            .header("x-tenant-id", env.tenant_a.clone())
            .body(axum::body::Body::empty())
            .expect("请求应能构造"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty(), "204 的响应体长度必须是 0");

    // 之后 refresh 必须失败（TOKEN_REVOKED）。
    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/refresh",
        Some(&env.tenant_a),
        None,
        json!({ "refreshToken": refresh }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(json_of(&bytes)["code"], "TOKEN_REVOKED");

    // 再登一次，然后 logout-all：全部 family 作废（含 SSO 会话）。
    let sso2 = env.oidc_login(&env.tenant_a, &env.idp_a).await;
    let access2 = sso2["accessToken"].as_str().expect("accessToken").to_string();
    let refresh2 = sso2["refreshToken"].as_str().expect("refreshToken").to_string();
    let (status, _, bytes) = send(
        &env.stack.router,
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/iam/logout-all")
            .header("authorization", format!("Bearer {access2}"))
            .header("x-tenant-id", env.tenant_a.clone())
            .body(axum::body::Body::empty())
            .expect("请求应能构造"),
    )
    .await;
    // AC-S-107 步骤 4（**rev 3.3 由 D128 裁定**）：`200 {"revokedSessions": n}`。
    // 原 AC 写 204，与 REQ-007 已合并的 `logout-all` 契约冲突；BA 裁定改规格而非改实现
    // —— 200 带作废回执，语义优于无法告知数量的 204。
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    let body = json_of(&bytes);
    assert!(
        body["revokedSessions"].as_u64().is_some_and(|n| n >= 1),
        "应回执本次作废的会话数：{body}"
    );
    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/refresh",
        Some(&env.tenant_a),
        None,
        json!({ "refreshToken": refresh2 }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{}", String::from_utf8_lossy(&bytes));
}

// ---------------------------------------------------------------------------
// AC-S-109
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_109_providers_are_per_tenant_and_never_leak_configuration() {
    let env = Env::new().await;
    for (tenant, expected) in [
        (&env.tenant_a, 2usize),
        (&env.tenant_b, 1usize),
        (&env.tenant_c, 0usize),
    ] {
        let (status, _, bytes) = http_get(
            &env.stack.router,
            "/api/iam/sso/providers",
            Some(tenant),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "未配置任何 provider 也必须是 200：{}",
            String::from_utf8_lossy(&bytes)
        );
        let body = json_of(&bytes);
        let providers = body["providers"].as_array().expect("providers 必须是数组");
        assert_eq!(providers.len(), expected, "{tenant} 的 provider 数不对：{body}");
    }

    // 响应里只有 type / id / label / 端点路径。
    let (_, _, bytes) = http_get(
        &env.stack.router,
        "/api/iam/sso/providers",
        Some(&env.tenant_a),
        None,
    )
    .await;
    let raw = String::from_utf8_lossy(&bytes).to_string();
    for forbidden in ["issuer", "clientId", "clientSecret", "ldapUrl", "baseDn", "127.0.0.1"] {
        assert!(
            !raw.contains(forbidden),
            "providers 响应不得包含配置值 {forbidden}：{raw}"
        );
    }
    let providers: Vec<SsoProviderView> =
        serde_json::from_value(json_of(&bytes)["providers"].clone()).expect("应能反序列化");
    for provider in providers {
        assert!(["oidc", "ldap"].contains(&provider.kind.as_str()));
        assert!(!provider.id.is_empty());
        assert!(!provider.label.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_109_ldap_and_oidc_sessions_have_different_families() {
    let env = Env::new().await;

    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/sso/ldap/login",
        Some(&env.tenant_a),
        Some(&unique_ip()),
        json!({ "username": "erin", "password": ERIN_PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));

    let oidc = env.oidc_login(&env.tenant_a, &env.idp_a).await;
    assert_eq!(oidc["tokenType"], "Bearer");

    let families: Vec<String> = sqlx::query_scalar(
        "SELECT family_id FROM platform_iam_session WHERE tenant_id = $1 AND revoked_at IS NULL",
    )
    .bind(&env.tenant_a)
    .fetch_all(&env.admin)
    .await
    .expect("应能读到会话");
    assert_eq!(families.len(), 2, "应有两条会话（LDAP + OIDC）");
    assert_ne!(families[0], families[1], "两次登录的 family_id 不得相等");

    let providers: Vec<String> = sqlx::query_scalar(
        "SELECT auth_provider FROM platform_iam_session WHERE tenant_id = $1 AND revoked_at IS NULL ORDER BY auth_provider",
    )
    .bind(&env.tenant_a)
    .fetch_all(&env.admin)
    .await
    .expect("应能读到来源");
    assert_eq!(providers, vec!["ldap".to_string(), "oidc".to_string()]);
    let _ = &env.ldap;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_109_a_state_from_another_tenant_cannot_be_used() {
    let env = Env::new().await;
    // tenant-b 的 state 拿到 tenant-a 回调。
    let path = format!("/api/iam/sso/oidc/authorize?tenant={}", env.tenant_b);
    let (status, headers, _) = http_get(&env.stack.router, &path, None, None).await;
    assert_eq!(status, StatusCode::FOUND);
    let state = location_query(&headers)["state"].clone();

    let callback = format!("/api/iam/sso/oidc/callback?code=x&state={state}");
    let (status, headers, _) = http_get(
        &env.stack.router,
        &callback,
        Some(&env.tenant_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(
        location_query(&headers).get("error").map(String::as_str),
        Some("SSO_OIDC_STATE_INVALID")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_109_an_unconfigured_tenant_gets_the_same_code_for_both_protocols() {
    let env = Env::new().await;
    let path = format!("/api/iam/sso/oidc/authorize?tenant={}", env.tenant_c);
    let (status, _, bytes) = http_get(&env.stack.router, &path, None, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{}", String::from_utf8_lossy(&bytes));
    let authorize_code = json_of(&bytes)["code"].clone();
    assert_eq!(authorize_code, "SSO_PROVIDER_NOT_CONFIGURED");

    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/sso/ldap/login",
        Some(&env.tenant_c),
        Some(&unique_ip()),
        json!({ "username": "erin", "password": ERIN_PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(
        json_of(&bytes)["code"], authorize_code,
        "两种协议在「未配置」时必须是同一个码"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_109_disabling_a_provider_looks_like_not_configured() {
    let env = Env::new().await;
    let mut config = env.config.by_tenant.lock().expect("锁")[&env.tenant_a].clone();
    config.oidc.enabled = false;
    env.config.set(&env.tenant_a, config);

    let path = format!("/api/iam/sso/oidc/authorize?tenant={}", env.tenant_a);
    let (status, _, bytes) = http_get(&env.stack.router, &path, None, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_of(&bytes)["code"], "SSO_PROVIDER_NOT_CONFIGURED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_109_changing_one_tenant_issuer_does_not_affect_another() {
    let env = Env::new().await;
    // tenant-b 的 issuer 改坏。
    let mut config = env.config.by_tenant.lock().expect("锁")[&env.tenant_b].clone();
    config.oidc.issuer = "http://127.0.0.1:9/broken".to_string();
    env.config.set(&env.tenant_b, config);

    // tenant-a 的登录不受影响。
    let login = env.oidc_login(&env.tenant_a, &env.idp_a).await;
    assert_eq!(login["tokenType"], "Bearer");

    // tenant-b 现在是 503（发现文档拿不到）。
    let path = format!("/api/iam/sso/oidc/authorize?tenant={}", env.tenant_b);
    let (status, _, bytes) = http_get(&env.stack.router, &path, None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(json_of(&bytes)["code"], "SSO_OIDC_DISCOVERY_FAILED");
}
