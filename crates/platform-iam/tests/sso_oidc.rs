//! AC-S-100（OIDC 端到端，含 https）/ AC-S-101（state·nonce·PKCE）/
//! AC-S-020 步骤 1·2（发现文档可诊断）/ AC-S-100 步骤 10·11（P15 与失败 302）。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-iam --test sso_oidc -- --test-threads=2 --include-ignored
//! ```
//!
//! 未设置 `DATABASE_URL_TEST` 时**直接失败**（不能静默跳过 = 假绿）。

mod sso_support;

use std::collections::{BTreeSet, HashMap};

use axum::http::StatusCode;
use serde_json::Value;

use sso_support::*;

const FRONTEND_BASE: &str = "http://app.example.com";
const CALLBACK_URI: &str = "http://app.example.com/api/iam/sso/oidc/callback";

/// 一套装好的测试环境。
struct Env {
    stack: Stack,
    idp: MockIdp,
    tenant: String,
}

impl Env {
    async fn http() -> Self {
        let app_url = setup_database().await;
        let stack = build_stack(&app_url).await;
        let idp = MockIdp::start_http().await;
        let tenant = unique_tenant("oidc");
        install_config(
            &stack.service,
            platform_iam::sso::config::SsoConfig {
                oidc: oidc_settings(&idp.issuer, CALLBACK_URI),
                ..Default::default()
            },
        );
        stack
            .service
            .set_sso_http_client(std::sync::Arc::new(
                platform_iam::sso::oidc::OidcHttpClient::new().expect("http 客户端应能构造"),
            ));
        stack.service.set_sso_frontend_base_url(FRONTEND_BASE);
        stack.service.set_sso_oidc_client_secret("SECRET-CANARY-oidc");
        let env = Self {
            stack,
            idp,
            tenant,
        };
        // AC-S-100 的前置：`bob` 在 IdP 侧存在且**已绑定**到本地 user。
        env.bind_bob().await;
        env
    }

    /// 建本地 `bob` 并把 `sub = idp-bob-001` 绑上去（AC-S-100 的前置）。
    async fn bind_bob(&self) -> String {
        let user = register(
            &self.stack.router,
            &self.tenant,
            "bob",
            PASSWORD,
            Some("bob@example.com"),
        )
        .await;
        let user_id = user["id"].as_str().expect("id").to_string();
        let admin = admin_pool().await;
        sqlx::query(
            "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
             VALUES ($1, $2, 'oidc', 'idp-bob-001', $3) ON CONFLICT DO NOTHING",
        )
        .bind(&self.tenant)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&user_id)
        .execute(&admin)
        .await
        .expect("绑定应能写入");
        user_id
    }
}

/// 发起授权，返回 (state, nonce, challenge, redirect_uri 等 query)。
async fn authorize(env: &Env) -> (StatusCode, HashMap<String, String>) {
    let path = format!(
        "/api/iam/sso/oidc/authorize?tenant={}",
        env.tenant
    );
    let (status, headers, bytes) = http_get(&env.stack.router, &path, None, None).await;
    assert_eq!(
        status,
        StatusCode::FOUND,
        "authorize 应 302，body = {}",
        String::from_utf8_lossy(&bytes)
    );
    (status, location_query(&headers))
}

/// 走完授权 → 回调（返回 ticket）。
async fn callback_with_ticket(env: &Env, code: &str, state: &str, tenant_header: Option<&str>) -> (StatusCode, HashMap<String, String>) {
    let path = format!("/api/iam/sso/oidc/callback?code={code}&state={state}");
    let (status, headers, bytes) = http_get(&env.stack.router, &path, tenant_header, None).await;
    assert_eq!(
        status,
        StatusCode::FOUND,
        "callback 应 302，body = {}",
        String::from_utf8_lossy(&bytes)
    );
    let query = location_query(&headers);
    assert!(query.get("error").is_none(), "回调意外失败：{query:?}");
    (status, query)
}

/// 断言回调失败 → `302 ...?error=<CODE>`（rev 3.1 / P4 失败分支）。
async fn callback_error(env: &Env, path: &str, tenant_header: Option<&str>, expected: &str) {
    let (status, headers, bytes) = http_get(&env.stack.router, path, tenant_header, None).await;
    assert_eq!(
        status,
        StatusCode::FOUND,
        "回调失败也必须 302 回前端（不是 JSON 错误页），实际 {status}，body = {}",
        String::from_utf8_lossy(&bytes)
    );
    let query = location_query(&headers);
    assert_eq!(
        query.get("error").map(String::as_str),
        Some(expected),
        "错误码不对：{query:?}"
    );
    // `error` 只放错误码：不得带 detail / 配置值 / token。
    for forbidden in ["issuer", "clientId", "client_secret", "detail", "accessToken"] {
        assert!(
            !format!("{query:?}").contains(forbidden),
            "失败交接的 query 不得包含 {forbidden}：{query:?}"
        );
    }
    assert!(
        location_path(&headers).ends_with("/sso/callback"),
        "失败交接必须落在前端回调页"
    );
}

// ---------------------------------------------------------------------------
// AC-S-100：授权码流程端到端
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_100_authorize_redirects_to_the_discovered_endpoint_with_every_parameter() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;

    assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(query.get("client_id").map(String::as_str), Some("client-1"));
    assert_eq!(
        query.get("redirect_uri").map(String::as_str),
        Some(CALLBACK_URI),
        "redirect_uri 必须与配置**逐字相同**"
    );
    assert!(
        query
            .get("scope")
            .is_some_and(|scope| scope.split_whitespace().any(|part| part == "openid")),
        "scope 必须含 openid：{query:?}"
    );
    assert_eq!(
        query.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    assert!(query.get("code_challenge").is_some());
}

/// `state` / `nonce` 的形状与不重复（S38：替代「高熵」空话）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_100_state_and_nonce_are_long_random_and_never_repeat() {
    let env = Env::http().await;
    let mut states = BTreeSet::new();
    let mut nonces = BTreeSet::new();
    for _ in 0..20 {
        let (_, query) = authorize(&env).await;
        let state = query.get("state").expect("state").clone();
        let nonce = query.get("nonce").expect("nonce").clone();
        for (label, value, set) in [
            ("state", state, &mut states),
            ("nonce", nonce, &mut nonces),
        ] {
            assert!(value.len() >= 43, "{label} 太短：{value}");
            assert!(
                value
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'),
                "{label} 只允许 [A-Za-z0-9_-]：{value}"
            );
            assert!(set.insert(value), "20 次内出现重复 {label}");
        }
    }
}

/// 回调 302 带 ticket（**不是** 200+JSON），exchange 返回与本地登录同构的 `LoginResponse`。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_100_the_callback_hands_off_a_ticket_and_exchange_returns_a_login_response() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();
    let nonce = query["nonce"].clone();
    let challenge = query["code_challenge"].clone();
    let redirect_uri = query["redirect_uri"].clone();

    let code = "code-happy".to_string();
    env.idp
        .register_code(&code, &nonce, &challenge, &redirect_uri);

    let path = format!("/api/iam/sso/oidc/callback?code={code}&state={state}");
    let (status, headers, _) = http_get(&env.stack.router, &path, None, None).await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(
        location_path(&headers),
        "http://app.example.com/sso/callback",
        "成功交接必须落在前端回调页"
    );
    let ticket_query = location_query(&headers);
    let ticket = ticket_query
        .get("ticket")
        .unwrap_or_else(|| panic!("回调没给 ticket：{ticket_query:?}"))
        .clone();
    assert!(
        !ticket_query.contains_key("accessToken") && !ticket_query.contains_key("refreshToken"),
        "URL 里不得出现 token（§8）"
    );

    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/sso/exchange",
        None,
        None,
        serde_json::json!({ "ticket": ticket }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    let login = json_of(&bytes);
    assert_eq!(login["tokenType"], "Bearer");
    assert!(login["expiresIn"].as_u64().is_some_and(|value| value > 0));
    assert!(login["permissions"].is_array(), "permissions 恒为数组");

    // 键集合与**本地登录**相同（AC-S-107 同构的直接要求）。
    let local_tenant = unique_tenant("local");
    let _user = register(&env.stack.router, &local_tenant, "grace", PASSWORD, Some("grace@example.com")).await;
    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/login",
        Some(&local_tenant),
        Some(&unique_ip()),
        serde_json::json!({ "username": "grace", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let local = json_of(&bytes);
    let keys = |value: &Value| -> BTreeSet<String> {
        value.as_object().expect("应是对象").keys().cloned().collect()
    };
    assert_eq!(
        keys(&login),
        keys(&local),
        "SSO 与本地登录的响应键集合必须完全相同"
    );

    // access token 可用；refresh token 可刷新；会话列表里有 SSO 会话且来源是 oidc。
    let access = login["accessToken"].as_str().expect("accessToken");
    let (status, _, bytes) = send(
        &env.stack.router,
        axum::http::Request::builder()
            .method("GET")
            .uri("/api/iam/sessions")
            .header("authorization", format!("Bearer {access}"))
            .header("x-tenant-id", env.tenant.clone())
            .body(axum::body::Body::empty())
            .expect("请求应能构造"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    let sessions = json_of(&bytes);
    let row = sessions
        .as_array()
        .and_then(|rows| rows.first())
        .expect("应有本次 SSO 会话");
    assert_eq!(row["authProvider"], "oidc");

    let refresh = login["refreshToken"].as_str().expect("refreshToken");
    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/refresh",
        Some(&env.tenant),
        None,
        serde_json::json!({ "refreshToken": refresh }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    assert!(json_of(&bytes)["accessToken"].is_string());
}

/// https 的 mock IdP 必须同样走通（D123 的直接验证点 / P10）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_100_an_https_issuer_works_with_an_injected_test_root() {
    let app_url = setup_database().await;
    let stack = build_stack(&app_url).await;
    let (idp, ca_der) = MockIdp::start_https().await;
    let tenant = unique_tenant("oidc-tls");

    install_config(
        &stack.service,
        platform_iam::sso::config::SsoConfig {
            oidc: oidc_settings(&idp.issuer, CALLBACK_URI),
            ..Default::default()
        },
    );
    // **只信任测试根**（`with_webpki_roots()` 不信任自签根 —— S13）。
    stack
        .service
        .set_sso_http_client(std::sync::Arc::new(
            platform_iam::sso::oidc::OidcHttpClient::with_root_certificate_der(&ca_der)
                .expect("测试根应可用"),
        ));
    stack.service.set_sso_frontend_base_url(FRONTEND_BASE);
    stack.service.set_sso_oidc_client_secret("SECRET-CANARY-oidc");

    let env = Env {
        stack,
        idp,
        tenant,
    };
    env.bind_bob().await;

    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();
    let nonce = query["nonce"].clone();
    let challenge = query["code_challenge"].clone();
    let redirect_uri = query["redirect_uri"].clone();
    let code = "code-tls".to_string();
    env.idp
        .register_code(&code, &nonce, &challenge, &redirect_uri);

    let (_, ticket_query) = callback_with_ticket(&env, &code, &state, None).await;
    let ticket = ticket_query.get("ticket").expect("ticket").clone();
    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/sso/exchange",
        None,
        None,
        serde_json::json!({ "ticket": ticket }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "https issuer 必须可用，body = {}",
        String::from_utf8_lossy(&bytes)
    );
}

/// P15：authorize 缺租户 → 401 `TENANT_CONTEXT_MISSING`。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_100_authorize_without_a_tenant_is_401() {
    let env = Env::http().await;
    let (status, _, bytes) = http_get(&env.stack.router, "/api/iam/sso/oidc/authorize", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json_of(&bytes)["code"], "TENANT_CONTEXT_MISSING");
}

/// P15：`?tenant=` 优先于请求头。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_100_the_tenant_query_parameter_wins_over_the_header() {
    let env = Env::http().await;
    let path = format!("/api/iam/sso/oidc/authorize?tenant={}", env.tenant);
    // 头里给一个**别的**租户：query 应当赢（否则拿错租户的 IdP 配置）。
    let (status, headers, _) = http_get(
        &env.stack.router,
        &path,
        Some("some-other-tenant"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    let query = location_query(&headers);
    assert_eq!(query["client_id"], "client-1");
}

/// 回调失败也必须 302 回前端带错误码（P4 失败分支 / rev 3.1）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_100_a_failed_callback_redirects_with_only_the_error_code() {
    let env = Env::http().await;
    callback_error(
        &env,
        "/api/iam/sso/oidc/callback?code=x&state=not-a-real-state",
        None,
        "SSO_OIDC_STATE_INVALID",
    )
    .await;
}

// ---------------------------------------------------------------------------
// AC-S-101：state / nonce / PKCE
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_101_a_state_can_only_be_used_once() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();
    let nonce = query["nonce"].clone();
    let challenge = query["code_challenge"].clone();
    let redirect_uri = query["redirect_uri"].clone();

    let first = "code-first".to_string();
    env.idp.register_code(&first, &nonce, &challenge, &redirect_uri);
    let (status, _, _) = http_get(
        &env.stack.router,
        &format!("/api/iam/sso/oidc/callback?code={first}&state={state}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);

    // 同一个 state 再用一次（哪怕换一个新 code）。
    let second = "code-second".to_string();
    env.idp.register_code(&second, &nonce, &challenge, &redirect_uri);
    callback_error(
        &env,
        &format!("/api/iam/sso/oidc/callback?code={second}&state={state}"),
        None,
        "SSO_OIDC_STATE_INVALID",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_101_an_expired_state_reports_expiry_not_invalidity() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();

    let admin = admin_pool().await;
    sqlx::query("UPDATE platform_iam_oidc_state SET expires_at = now() - interval '1 second' WHERE state = $1")
        .bind(&state)
        .execute(&admin)
        .await
        .expect("DB 侧制造过期应成功");
    drop(admin);

    callback_error(
        &env,
        &format!("/api/iam/sso/oidc/callback?code=x&state={state}"),
        None,
        "SSO_OIDC_STATE_EXPIRED",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_101_a_state_from_another_tenant_is_rejected() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();
    let other = unique_tenant("other");

    callback_error(
        &env,
        &format!("/api/iam/sso/oidc/callback?code=x&state={state}"),
        Some(&other),
        "SSO_OIDC_STATE_INVALID",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_101_a_state_bound_to_another_provider_is_rejected() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();

    let admin = admin_pool().await;
    sqlx::query("UPDATE platform_iam_oidc_state SET provider = 'oidc-other' WHERE state = $1")
        .bind(&state)
        .execute(&admin)
        .await
        .expect("DB 侧改 provider 应成功");
    drop(admin);

    callback_error(
        &env,
        &format!("/api/iam/sso/oidc/callback?code=x&state={state}"),
        None,
        "SSO_OIDC_STATE_INVALID",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_101_a_mismatched_nonce_is_rejected() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();
    let nonce = query["nonce"].clone();
    let challenge = query["code_challenge"].clone();
    let redirect_uri = query["redirect_uri"].clone();
    env.idp.set_behavior(|behavior| {
        behavior
            .claims_override
            .insert("nonce".to_string(), serde_json::json!("another-nonce"));
    });

    let code = "code-nonce".to_string();
    env.idp.register_code(&code, &nonce, &challenge, &redirect_uri);
    callback_error(
        &env,
        &format!("/api/iam/sso/oidc/callback?code={code}&state={state}"),
        None,
        "SSO_OIDC_NONCE_MISMATCH",
    )
    .await;
}

/// PKCE 篡改：DB 侧改 `code_verifier` → **本地重算 challenge** 就能发现（400 PKCE）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_101_a_tampered_code_verifier_is_detected_locally() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();
    let nonce = query["nonce"].clone();
    let challenge = query["code_challenge"].clone();
    let redirect_uri = query["redirect_uri"].clone();

    // 另一个**合法形状**的 verifier（43 字符 base64url）。
    let other_verifier = platform_iam::sso::oidc::new_code_verifier();
    let admin = admin_pool().await;
    sqlx::query("UPDATE platform_iam_oidc_state SET code_verifier = $1 WHERE state = $2")
        .bind(&other_verifier)
        .bind(&state)
        .execute(&admin)
        .await
        .expect("DB 侧改 verifier 应成功");
    drop(admin);

    let code = "code-pkce".to_string();
    env.idp.register_code(&code, &nonce, &challenge, &redirect_uri);
    callback_error(
        &env,
        &format!("/api/iam/sso/oidc/callback?code={code}&state={state}"),
        None,
        "SSO_OIDC_PKCE_MISMATCH",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_101_a_callback_without_state_or_code_is_rejected() {
    let env = Env::http().await;
    callback_error(
        &env,
        "/api/iam/sso/oidc/callback?code=only-code",
        None,
        "SSO_OIDC_CALLBACK_PARAM_MISSING",
    )
    .await;
    callback_error(
        &env,
        "/api/iam/sso/oidc/callback?state=only-state",
        None,
        "SSO_OIDC_CALLBACK_PARAM_MISSING",
    )
    .await;
}

/// 已消费的 state 会留下 `consumed_at`（或不删行），且**不存在**同一 state 的两条有效记录。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_101_a_consumed_state_is_marked_in_the_database() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();
    let nonce = query["nonce"].clone();
    let challenge = query["code_challenge"].clone();
    let redirect_uri = query["redirect_uri"].clone();
    let code = "code-consumed".to_string();
    env.idp.register_code(&code, &nonce, &challenge, &redirect_uri);
    let _ = http_get(
        &env.stack.router,
        &format!("/api/iam/sso/oidc/callback?code={code}&state={state}"),
        None,
        None,
    )
    .await;

    let admin = admin_pool().await;
    let row: (Option<time::OffsetDateTime>, i64) = sqlx::query_as(
        "SELECT consumed_at, (SELECT count(*) FROM platform_iam_oidc_state s2 WHERE s2.state = s1.state) \
         FROM platform_iam_oidc_state s1 WHERE state = $1",
    )
    .bind(&state)
    .fetch_one(&admin)
    .await
    .expect("应能查到 state 行");
    assert!(row.0.is_some(), "已用 state 的 consumed_at 必须非空");
    assert_eq!(row.1, 1, "同一 state 不得有两条记录");
}

// ---------------------------------------------------------------------------
// AC-S-020 步骤 1·2：发现文档的可区分失败
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_020_a_500_from_the_issuer_is_discovery_failed() {
    let env = Env::http().await;
    env.idp.set_behavior(|behavior| behavior.discovery_status = Some(500));
    let path = format!("/api/iam/sso/oidc/authorize?tenant={}", env.tenant);
    let (status, _, bytes) = http_get(&env.stack.router, &path, None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(json_of(&bytes)["code"], "SSO_OIDC_DISCOVERY_FAILED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_020_invalid_discovery_json_is_discovery_invalid() {
    let env = Env::http().await;
    env.idp.set_behavior(|behavior| {
        behavior.discovery_body = Some("{not json".to_string());
    });
    let path = format!("/api/iam/sso/oidc/authorize?tenant={}", env.tenant);
    let (status, _, bytes) = http_get(&env.stack.router, &path, None, None).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(json_of(&bytes)["code"], "SSO_OIDC_DISCOVERY_INVALID");
}

// ---------------------------------------------------------------------------
// AC-S-020 步骤 3·4：换 token 的两类失败**必须不同**
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_020_a_bogus_code_is_token_exchange_failed() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();
    // 不登记 code → mock 的 /token 返回 400 `invalid_grant`。
    callback_error(
        &env,
        &format!("/api/iam/sso/oidc/callback?code=bogus&state={state}"),
        None,
        "SSO_OIDC_TOKEN_EXCHANGE_FAILED",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_020_a_503_from_the_token_endpoint_is_endpoint_unavailable() {
    let env = Env::http().await;
    let (_, query) = authorize(&env).await;
    let state = query["state"].clone();
    let nonce = query["nonce"].clone();
    let challenge = query["code_challenge"].clone();
    let redirect_uri = query["redirect_uri"].clone();
    let code = "code-503".to_string();
    env.idp.register_code(&code, &nonce, &challenge, &redirect_uri);
    env.idp.set_behavior(|behavior| behavior.token_status = Some(503));

    callback_error(
        &env,
        &format!("/api/iam/sso/oidc/callback?code={code}&state={state}"),
        None,
        "SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE",
    )
    .await;
}
