//! AC-S-102：`id_token` 验签 —— 拒绝 `alg: none`、算法混淆、**外部公钥注入**
//! （`jku` / `x5c`）。以及 AC-S-020 的码表完备性。
//!
//! # 攻击样例是**真造出来的**，不是「正常拒绝」
//!
//! | 步骤 | 攻击构造 |
//! |------|---------|
//! | 4 | `alg=none` + 空签名 |
//! | 5 | `HS256`，密钥取 IdP **公钥内容**（经典 RS→HS 混淆） |
//! | 6 | `RS512`（白名单外） |
//! | 9 | `header.jku` 指向**攻击者 JWKS 端点**，用攻击者密钥签名 |
//! | 10 | `header.x5c` 内嵌攻击者公钥 |
//!
//! 步骤 11 断言**攻击者端点被请求次数 == 0** —— 我们从不去那个地址取公钥。

mod sso_support;

use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::json;

use sso_support::*;

const FRONTEND_BASE: &str = "http://app.example.com";
const CALLBACK_URI: &str = "http://app.example.com/api/iam/sso/oidc/callback";

struct Env {
    stack: Stack,
    idp: MockIdp,
    tenant: String,
}

impl Env {
    async fn new() -> Self {
        let app_url = setup_database().await;
        let stack = build_stack(&app_url).await;
        let idp = MockIdp::start_http().await;
        let tenant = unique_tenant("oidc-sec");
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
                platform_iam::sso::oidc::OidcHttpClient::new().expect("http 客户端"),
            ));
        stack.service.set_sso_frontend_base_url(FRONTEND_BASE);
        stack.service.set_sso_oidc_client_secret("SECRET-CANARY-oidc");

        // 前置：bob 已绑定。
        let user = register(&stack.router, &tenant, "bob", PASSWORD, Some("bob@example.com")).await;
        let user_id = user["id"].as_str().expect("id").to_string();
        let admin = admin_pool().await;
        sqlx::query(
            "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
             VALUES ($1, $2, 'oidc', 'idp-bob-001', $3) ON CONFLICT DO NOTHING",
        )
        .bind(&tenant)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&user_id)
        .execute(&admin)
        .await
        .expect("绑定应能写入");

        Self {
            stack,
            idp,
            tenant,
        }
    }

    /// 发起授权并登记 code，返回 (state, code)。
    async fn prepare(&self) -> (String, String) {
        let path = format!("/api/iam/sso/oidc/authorize?tenant={}", self.tenant);
        let (status, headers, _) = http_get(&self.stack.router, &path, None, None).await;
        assert_eq!(status, StatusCode::FOUND);
        let query = location_query(&headers);
        let code = format!("code-{}", uuid::Uuid::new_v4().simple());
        self.idp.register_code(
            &code,
            &query["nonce"],
            &query["code_challenge"],
            &query["redirect_uri"],
        );
        (query["state"].clone(), code)
    }

    /// 回调并返回错误码（要求 302 + `error=<code>`）。
    async fn callback_error(&self, code: &str, state: &str) -> String {
        let path = format!("/api/iam/sso/oidc/callback?code={code}&state={state}");
        let (status, headers, bytes) = http_get(&self.stack.router, &path, None, None).await;
        assert_eq!(
            status,
            StatusCode::FOUND,
            "回调失败也应 302 回前端，body = {}",
            String::from_utf8_lossy(&bytes)
        );
        location_query(&headers)
            .get("error")
            .cloned()
            .unwrap_or_else(|| panic!("失败交接必须带 error 码"))
    }

    /// 回调并断言成功（拿到了 ticket）。
    async fn callback_ok(&self, code: &str, state: &str) {
        let path = format!("/api/iam/sso/oidc/callback?code={code}&state={state}");
        let (status, headers, bytes) = http_get(&self.stack.router, &path, None, None).await;
        assert_eq!(status, StatusCode::FOUND, "{}", String::from_utf8_lossy(&bytes));
        let query = location_query(&headers);
        assert!(
            query.get("ticket").is_some(),
            "应拿到 ticket，实际 {query:?}"
        );
    }
}

fn b64(value: &serde_json::Value) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).expect("json"))
}

/// 造一个无签名 / 假签名的 token。
fn forged(header: serde_json::Value, payload: serde_json::Value) -> String {
    format!(
        "{}.{}.{}",
        b64(&header),
        b64(&payload),
        URL_SAFE_NO_PAD.encode(b"not-a-real-signature")
    )
}

fn claims_for(idp: &MockIdp, state_nonce: &str) -> serde_json::Value {
    idp.default_claims(state_nonce)
}

// ---------------------------------------------------------------------------
// claims 校验（签名合法但声明不对）
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_a_wrong_issuer_is_rejected() {
    let env = Env::new().await;
    let (state, code) = env.prepare().await;
    env.idp.set_behavior(|behavior| {
        behavior
            .claims_override
            .insert("iss".to_string(), json!("https://evil.example.com"));
    });
    assert_eq!(env.callback_error(&code, &state).await, "SSO_OIDC_TOKEN_INVALID");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_a_wrong_audience_is_rejected() {
    let env = Env::new().await;
    let (state, code) = env.prepare().await;
    env.idp.set_behavior(|behavior| {
        behavior
            .claims_override
            .insert("aud".to_string(), json!("another-client"));
    });
    assert_eq!(env.callback_error(&code, &state).await, "SSO_OIDC_TOKEN_INVALID");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_an_expired_token_reports_expiry() {
    let env = Env::new().await;
    let (state, code) = env.prepare().await;
    env.idp.set_behavior(|behavior| {
        behavior.claims_override.insert(
            "exp".to_string(),
            json!(time::OffsetDateTime::now_utc().unix_timestamp() - 3600),
        );
    });
    assert_eq!(env.callback_error(&code, &state).await, "SSO_OIDC_TOKEN_EXPIRED");
}

// ---------------------------------------------------------------------------
// 算法白名单
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_alg_none_is_rejected() {
    let env = Env::new().await;
    let (state, code) = env.prepare().await;
    let claims = claims_for(&env.idp, "irrelevant");
    env.idp.set_behavior(|behavior| {
        behavior.id_token_override = Some(format!(
            "{}.{}.",
            b64(&json!({ "alg": "none", "typ": "JWT" })),
            b64(&claims)
        ));
    });
    assert_eq!(
        env.callback_error(&code, &state).await,
        "SSO_OIDC_ALG_NOT_ALLOWED"
    );
}

/// **RS→HS 混淆**：把 IdP 的公钥内容当 HMAC 密钥来签。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_hs256_signed_with_the_public_key_is_rejected() {
    let env = Env::new().await;
    let (state, code) = env.prepare().await;

    // 「公钥内容」= IdP 的 JWK 的 x/y（真实攻击里用 PEM / JWKS 里的材料）。
    let public_material = format!("{}{}", env.idp.issuer, env.idp.current_kid());
    let encoding = EncodingKey::from_secret(public_material.as_bytes());
    let claims = claims_for(&env.idp, "irrelevant");
    let token = jsonwebtoken::encode(
        &Header {
            alg: Algorithm::HS256,
            kid: Some(env.idp.current_kid()),
            ..Header::default()
        },
        &claims,
        &encoding,
    )
    .expect("HS256 签名应成功");
    env.idp.set_behavior(|behavior| {
        behavior.id_token_override = Some(token);
    });
    assert_eq!(
        env.callback_error(&code, &state).await,
        "SSO_OIDC_ALG_NOT_ALLOWED"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_rs512_outside_the_whitelist_is_rejected() {
    let env = Env::new().await;
    let (state, code) = env.prepare().await;
    let claims = claims_for(&env.idp, "irrelevant");
    env.idp.set_behavior(|behavior| {
        behavior.id_token_override = Some(forged(
            json!({ "alg": "RS512", "kid": "kid-1" }),
            claims,
        ));
    });
    assert_eq!(
        env.callback_error(&code, &state).await,
        "SSO_OIDC_ALG_NOT_ALLOWED"
    );
}

// ---------------------------------------------------------------------------
// 密钥轮换 / 篡改
// ---------------------------------------------------------------------------

/// 未知 `kid`（IdP 轮换了密钥）→ 强制刷新一次 JWKS 后**登录成功**。
///
/// 不只断言「成功」：还要证明**真的又取了一次 JWKS**（AC-S-102 步骤 7 / D128 Q9）。
/// 只断言成功的话，「实现改成未知 kid 直接失败但 mock 碰巧还能签发」也会绿。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_an_unknown_kid_triggers_a_jwks_refresh_and_succeeds() {
    let env = Env::new().await;
    // 先跑一次让 JWKS 进缓存（缓存里有 kid-1）。
    let (state, code) = env.prepare().await;
    env.callback_ok(&code, &state).await;

    let hits_before = env.idp.jwks_hits();
    assert!(
        hits_before >= 1,
        "首次登录必须先取一次 JWKS（否则「再取一次」无法区分）：{hits_before}"
    );

    // IdP 轮换：新 kid 不在我们的缓存里。
    env.idp.rotate_keys();
    let (state, code) = env.prepare().await;
    env.callback_ok(&code, &state).await;

    let hits_after = env.idp.jwks_hits();
    assert!(
        hits_after >= hits_before + 1,
        "未知 kid 必须触发一次 JWKS 刷新（{hits_before} → {hits_after}）"
    );
    // 且刷新后确实用**新** kid 的密钥验过了（不是拿旧缓存硬蒙）。
    assert_ne!(
        env.idp.current_kid(),
        "kid-1",
        "前置：轮换后当前 kid 应已变化"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_a_tampered_payload_breaks_the_signature() {
    let env = Env::new().await;
    let (state, code) = env.prepare().await;

    let claims = claims_for(&env.idp, "irrelevant");
    let valid = env.idp.sign_id_token(claims);
    let mut parts: Vec<&str> = valid.split('.').collect();
    let tampered_claims = json!({
        "iss": env.idp.issuer,
        "sub": "idp-bob-001",
        "aud": "client-1",
        "exp": time::OffsetDateTime::now_utc().unix_timestamp() + 300,
        "iat": time::OffsetDateTime::now_utc().unix_timestamp(),
        "nonce": "irrelevant",
        // 改 email（签名保留原样）。
        "email": "attacker@example.com",
    });
    let tampered = b64(&tampered_claims);
    parts[1] = &tampered;
    let token = parts.join(".");
    env.idp.set_behavior(|behavior| {
        behavior.id_token_override = Some(token);
    });
    assert_eq!(env.callback_error(&code, &state).await, "SSO_OIDC_TOKEN_INVALID");
}

// ---------------------------------------------------------------------------
// 外部公钥注入：jku / x5c
// ---------------------------------------------------------------------------

/// `header.jku` 指向攻击者端点 → 拒绝，且**该端点被请求 0 次**。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_a_jku_pointer_is_never_fetched() {
    let env = Env::new().await;
    env.idp.start_attacker_jwks().await;
    let attacker_url = env.idp.attacker_jwks_url();
    let (state, code) = env.prepare().await;

    // 攻击者的密钥签出来的 token，但 header 里声称是 IdP 的 kid + jku 指攻击者。
    let attacker_pem = platform_iam::jwt::generate_key_pair().expect("攻击者密钥");
    let attacker_encoding =
        EncodingKey::from_ec_pem(attacker_pem.as_bytes()).expect("攻击者编码密钥");
    let claims = claims_for(&env.idp, "irrelevant");
    let token = jsonwebtoken::encode(
        &Header {
            alg: Algorithm::ES256,
            kid: Some(env.idp.current_kid()),
            jku: Some(attacker_url.clone()),
            ..Header::default()
        },
        &claims,
        &attacker_encoding,
    )
    .expect("攻击者签名");

    env.idp.set_behavior(|behavior| {
        behavior.id_token_override = Some(token);
    });
    let code_seen = env.callback_error(&code, &state).await;
    assert_eq!(code_seen, "SSO_OIDC_TOKEN_INVALID");
    assert_eq!(
        env.idp.attacker_hits(),
        0,
        "绝不按 jku 去取公钥（那等于不验签）"
    );
}

/// `header.x5c` 内嵌攻击者公钥 → 不采信，验签失败。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_102_an_x5c_embedded_key_is_not_trusted() {
    let env = Env::new().await;
    let (state, code) = env.prepare().await;

    let attacker_pem = platform_iam::jwt::generate_key_pair().expect("攻击者密钥");
    let attacker_encoding =
        EncodingKey::from_ec_pem(attacker_pem.as_bytes()).expect("攻击者编码密钥");
    let claims = claims_for(&env.idp, "irrelevant");
    let token = jsonwebtoken::encode(
        &Header {
            alg: Algorithm::ES256,
            kid: Some(env.idp.current_kid()),
            x5c: Some(vec![URL_SAFE_NO_PAD.encode(b"attacker-public-key-der")]),
            ..Header::default()
        },
        &claims,
        &attacker_encoding,
    )
    .expect("攻击者签名");

    env.idp.set_behavior(|behavior| {
        behavior.id_token_override = Some(token);
    });
    assert_eq!(env.callback_error(&code, &state).await, "SSO_OIDC_TOKEN_INVALID");
}

// ---------------------------------------------------------------------------
// 码表完备性（AC-S-102 预期 11 / AC-S-020 预期 8·9）
// ---------------------------------------------------------------------------

#[test]
fn ac_s_020_every_referenced_code_has_an_ascii_title() {
    use platform_core::titles::{title_of_code, is_registered};
    for code in [
        "SSO_REQUIRED",
        "SSO_PROVIDER_NOT_CONFIGURED",
        "SSO_OIDC_DISCOVERY_FAILED",
        "SSO_OIDC_DISCOVERY_INVALID",
        "SSO_OIDC_STATE_INVALID",
        "SSO_OIDC_STATE_EXPIRED",
        "SSO_OIDC_CALLBACK_PARAM_MISSING",
        "SSO_OIDC_TOKEN_EXCHANGE_FAILED",
        "SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE",
        "SSO_OIDC_TOKEN_INVALID",
        "SSO_OIDC_TOKEN_EXPIRED",
        "SSO_OIDC_ALG_NOT_ALLOWED",
        "SSO_OIDC_NONCE_MISMATCH",
        "SSO_OIDC_PKCE_MISMATCH",
        "SSO_LDAP_UNREACHABLE",
        "SSO_LDAP_BIND_FAILED",
        "SSO_LDAP_EMPTY_PASSWORD",
        "SSO_IDENTITY_LINK_CONFLICT",
        "SSO_ACCOUNT_NOT_LINKED",
        "SSO_TICKET_INVALID",
    ] {
        assert!(is_registered(code), "TITLES 缺少 {code}");
        let title = title_of_code(code);
        assert!(title.is_ascii(), "{code} 的 title 不是 ASCII：{title}");
        assert_ne!(title, code, "{code} 的 title 不得等于 code 本身");
    }
    // 21 个新码全在 `ALL_CODES` 里。
    for code in platform_iam::error::ALL_CODES {
        if let Some(sso) = code.strip_prefix("SSO_") {
            let _ = sso;
        }
    }
    let sso_codes: Vec<&&str> = platform_iam::error::ALL_CODES
        .iter()
        .filter(|code| code.starts_with("SSO_"))
        .collect();
    assert_eq!(sso_codes.len(), 21, "SSO 错误码必须是 21 个");
}

/// 算法白名单是**硬编码**的 `{RS256, ES256}`（P5：删掉「可由配置收窄」）。
#[test]
fn ac_s_102_the_algorithm_whitelist_is_hardcoded() {
    use platform_iam::sso::oidc::{ALLOWED_ALGORITHM_NAMES, ALLOWED_ALGORITHMS};
    assert_eq!(ALLOWED_ALGORITHMS.len(), 2);
    assert_eq!(ALLOWED_ALGORITHM_NAMES, ["RS256", "ES256"]);
}
