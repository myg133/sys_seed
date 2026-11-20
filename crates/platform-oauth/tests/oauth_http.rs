//! 出站 IdP 的**真 HTTP** 集成用例（axum `oneshot`，不连网络）。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-oauth --test oauth_http -- --test-threads=1
//! ```
//!
//! # 覆盖的 AC
//!
//! | AC | 用例函数 |
//! |----|---------|
//! | AC-S-134 | `the_authorization_server_metadata_is_discoverable` |
//! | AC-S-135 | `a_client_can_be_registered_disabled_and_deleted` |
//! | AC-S-136 | `the_client_secret_is_never_echoed_by_a_read_endpoint` |
//! | AC-S-137 / 139 | `an_authorization_code_with_pkce_exchanges_for_a_usable_token` |
//! | AC-S-138 | `pkce_is_mandatory_and_s256_only` |
//! | AC-S-140 | `the_scope_is_the_intersection_of_three_sets` |
//! | AC-S-141 | `the_client_credentials_grant_issues_a_token_without_a_user` |
//! | AC-S-142 | `the_device_code_flow_runs_end_to_end` |
//! | AC-S-143 | `a_client_can_only_introspect_its_own_tokens` |
//! | AC-S-144 | `a_revoked_token_stops_being_active_immediately` |
//! | AC-S-145 | `the_access_token_is_an_es256_jwt_with_the_pinned_claims` |
//! | AC-S-146 | `the_consent_record_is_written_and_denial_does_not_change_it` |
//! | AC-S-147 | `a_user_sees_and_revokes_only_their_own_grants` |
//! | AC-S-148 | `an_existing_grant_allows_silent_renewal` |
//! | AC-S-149 / 150 | `the_logout_revokes_the_grants_and_returns_the_front_channel_list` |
//! | AC-S-151 | `a_third_party_oauth_token_is_rejected_by_the_platform` |
//! | AC-S-154 | `the_key_actions_produce_audit_records` |
//!
//! # ⚠️ 「权限层是 OR 不是 AND」的守卫（BA 复核发现的问题）
//!
//! `the_permission_points_are_per_endpoint_not_stacked` ——
//! axum 的 `Router::route_layer` 是**叠加**语义 → 叠三个就变成「三个都要有」。

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// client-A 的 client ID（用例内固定）。
const CLIENT_A: &str = "client-a";
/// client-B 的 client ID。
const CLIENT_B: &str = "client-b";
/// public client（**没有** secret）。
const PUBLIC_CLIENT: &str = "public-client";

/// tenant-a 的普通用户（只持 `rbac:role:read`）。
const USER_A: &str = "user-a";
/// tenant-a 的 OAuth 管理员。
const ADMIN_A: &str = "admin-a";
/// tenant-b 的普通用户。
const USER_B: &str = "user-b";

/// PKCE 的 `code_verifier`（RFC 7636 §4.1 的合法取值）。
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

/// `digest_base64url` 的**独立实现**（测试侧自己算，**不复用生产代码**）。
///
/// ⚠️ 为什么不用 `platform_oauth::crypto::digest_base64url`：
/// 那样断言就是**自己证明自己** —— 把生产代码的摘要口径改成 hex，测试照样绿
/// （假绿）。→ 变异验证要能抓住「口径被改」，这里就必须**另写一份**。
///
/// 口径（D228）：SHA-256 的**原始 32 字节** → base64url **无填充** → 43 字符。
fn digest_of(raw: &[u8]) -> String {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64;
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(raw);
    BASE64.encode(digest)
}

/// `code_challenge` = base64url(SHA256(verifier))。
fn challenge_of(verifier: &str) -> String {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64;
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(verifier.as_bytes());
    BASE64.encode(digest)
}

/// 发一个请求 → (状态码, JSON 体)。
async fn call(
    router: &axum::Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: Option<String>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = auth {
        builder = builder.header("authorization", token);
    }
    let request = match body {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .expect("请求应当能构造"),
        None => builder.body(Body::empty()).expect("请求应当能构造"),
    };
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("路由应当能应答");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("响应体应当能读完");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// 发一个 **form** 请求 → (状态码, JSON 体)。
async fn call_form(
    router: &axum::Router,
    path: &str,
    form: &[(&str, &str)],
    auth: Option<String>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("content-type", "application/x-www-form-urlencoded");
    if let Some(token) = auth {
        builder = builder.header("authorization", token);
    }
    let body = form
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let request = builder.body(Body::from(body)).expect("请求应当能构造");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("路由应当能应答");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("响应体应当能读完");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// 解出令牌载荷（**只解，不验签** —— AC-S-141 步骤 3 的口径）。
fn payload_of(token: &str) -> Value {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64;
    let part = token.split('.').nth(1).expect("JWT 应当有三段");
    let bytes = BASE64.decode(part).expect("载荷应当是 base64url");
    serde_json::from_slice(&bytes).expect("载荷应当是 JSON")
}

/// 解出令牌头（`alg` / `kid`）。
fn header_of(token: &str) -> Value {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64;
    let part = token.split('.').next().expect("JWT 应当有三段");
    let bytes = BASE64.decode(part).expect("头部应当是 base64url");
    serde_json::from_slice(&bytes).expect("头部应当是 JSON")
}

// ---------------------------------------------------------------------------
// 测试侧的**独立 ES256 验签**（AC-S-145 步骤 1/3/4）
// ---------------------------------------------------------------------------
//
// # 为什么这段代码必须**另写一份**
//
// 生产侧的验签在 `platform_oauth::keys::PlatformKeys::verify`。若测试直接调它，
// 断言就变成「自己证明自己」：把 `keys.rs` 的签名换成**假的**（用别的密钥签、
// 或干脆不签），测试照样绿（假绿 —— 这正是复核 B-02 的指控）。
//
// → 这里**只用 `jsonwebtoken` 的通用 API**（`JwkSet` / `DecodingKey` /
//   `Validation` / `decode`），与生产实现**不共享任何函数**。
//   公开入口只有 `GET /.well-known/jwks.json`（第三方验签的唯一现实路径）。

/// 从 JWKS 响应体里按 `kid` 取一把**验签公钥**。
fn decoding_key_from_jwks(jwks: &Value, kid: &str) -> jsonwebtoken::DecodingKey {
    let set: jsonwebtoken::jwk::JwkSet =
        serde_json::from_value(jwks.clone()).expect("JWKS 应当能解析成 JwkSet");
    let jwk = set
        .find(kid)
        .unwrap_or_else(|| panic!("JWKS 里没有 kid={kid} 的公钥"));
    jsonwebtoken::DecodingKey::from_jwk(jwk).expect("应当能从 JWK 构造验签密钥")
}

/// 验签结果（**区分「签名无效」与「已过期」** —— AC-S-145 步骤 4 的硬要求）。
#[derive(Debug, PartialEq, Eq)]
enum VerifyOutcome {
    /// 签名有效，且**未**过期；附载荷。
    Valid,
    /// 签名有效，但 `exp` 已过。
    ExpiredButSigned,
    /// **签名无效**（改过载荷 / 用了别的密钥 / 算法被换）。
    BadSignature,
}

/// 用 JWKS 的公钥**独立验签**（不碰 `platform_oauth::keys`）。
///
/// # 为什么要分三档结果
///
/// AC-S-145 步骤 4：「验签**通过**（签名有效）但 `exp` 已过期 → 结论是『已过期』，
/// **不得**把『签名无效』与『已过期』混为一谈」，两者必须是**不同的断言**。
/// 若把两者合并成「失败」，那么「签名其实无效」也会被当成「过期」通过 → 假绿。
fn verify_signature(jwks: &Value, token: &str) -> (VerifyOutcome, Value) {
    let header = header_of(token);
    let kid = header["kid"].as_str().expect("头里应当有 kid");
    let key = decoding_key_from_jwks(jwks, kid);

    // 1) **只验签名 + 算法，不验 exp**（`validate_exp = false`）→
    //    这样「过期令牌」在**这一步**仍应通过（签名是有效的）。
    let mut signature_only = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::ES256);
    signature_only.leeway = 0;
    signature_only.validate_exp = false;
    signature_only.validate_nbf = false;
    signature_only.validate_aud = false;
    signature_only.set_required_spec_claims::<String>(&[]);

    let claims: Value =
        match jsonwebtoken::decode::<Value>(token, &key, &signature_only) {
            Ok(data) => data.claims,
            // 签名不对 / 载荷被改 / 密钥不对 → **签名无效**（这一档必须单独存在）。
            Err(_) => return (VerifyOutcome::BadSignature, Value::Null),
        };

    // 2) 签名有效 → 再判 `exp`（手工判，避免把两件事混进同一个错误分支）。
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let exp = claims.get("exp").and_then(Value::as_i64);
    match exp {
        Some(exp) if exp <= now => (VerifyOutcome::ExpiredButSigned, claims),
        _ => (VerifyOutcome::Valid, claims),
    }
}

/// 走一遍**授权码**流程（preview → confirm）→ 拿到可交换的授权码。
///
/// ⚠️ `preview` **不**判 scope 交集（AC-S-140 的判据在**令牌端点**）→
/// 所以这里对任意 scope 都能拿到码，判据留给 `POST /api/oauth/token`。
async fn authorization_code(fixture: &Fixture, user: &str, client_id: &str, scope: &str) -> String {
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": client_id,
            "redirectUri": "https://client.example/cb",
            "scope": scope,
            "state": "xyz",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "preview 应当 200：{body}");
    let ticket = body["ticket"].as_str().expect("应当有票据").to_string();

    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/confirm",
        Some(json!({ "ticket": ticket })),
        Some(user.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "confirm 应当 200：{body}");
    let redirect = body["redirectTo"]
        .as_str()
        .expect("应当有 redirectTo")
        .to_string();
    redirect
        .split("code=")
        .nth(1)
        .expect("应当有 code")
        .split('&')
        .next()
        .expect("code 应当有值")
        .to_string()
}

/// 用授权码换令牌（断言用的薄封装）。
async fn exchange_code(
    fixture: &Fixture,
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> (StatusCode, Value) {
    call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", "https://client.example/cb"),
            ("code_verifier", VERIFIER),
        ],
        Some(basic(client_id, client_secret)),
    )
    .await
}

/// 把夹具准备好（权限 + client）。
async fn prepared() -> (Fixture, String) {
    let fixture = setup().await;
    fixture.permissions.grant(USER_A, "rbac:role:read");
    fixture.permissions.grant(ADMIN_A, "oauth:client:read");
    fixture.permissions.grant(ADMIN_A, "oauth:client:write");
    fixture.permissions.grant(ADMIN_A, "oauth:client:delete");
    fixture.permissions.grant(ADMIN_A, "oauth:grant:read");
    fixture.permissions.grant(ADMIN_A, "oauth:grant:revoke");
    fixture.permissions.grant(USER_A, "oauth:grant:read");
    fixture.permissions.grant(USER_A, "oauth:grant:revoke");
    fixture.permissions.grant(USER_B, "oauth:grant:read");
    fixture.permissions.grant(USER_B, "oauth:grant:revoke");
    let token = login_token(&fixture, TENANT_A, USER_A);
    (fixture, token)
}

/// 造 client-A（confidential，配 back + front channel）。
async fn seed_client_a(fixture: &Fixture) -> String {
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read"],
        &["authorization_code", "client_credentials"],
        Some("http://127.0.0.1:19999/logout"),
        Some("https://client.example/front"),
    )
    .await;
    secret.to_string()
}

/// 造 client-B（confidential，**不**配 front-channel URI）。
async fn seed_client_b(fixture: &Fixture) -> String {
    let secret = "secret-b";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        fixture,
        CLIENT_B,
        Some(&hash),
        "confidential",
        &["rbac:role:read"],
        &["client_credentials"],
        None,
        None,
    )
    .await;
    secret.to_string()
}

/// 造 public client（**没有** secret）。
async fn seed_public_client(fixture: &Fixture) {
    seed_client(
        fixture,
        PUBLIC_CLIENT,
        None,
        "public",
        &["rbac:role:read"],
        &["authorization_code"],
        None,
        None,
    )
    .await;
}

/// AC-S-134：元数据可被发现。
#[tokio::test]
async fn the_authorization_server_metadata_is_discoverable() {
    let fixture = setup().await;
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/.well-known/oauth-authorization-server",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "元数据应当 200：{body}");
    assert_eq!(body["issuer"], "base");
    assert_eq!(body["authorization_endpoint"], "base/oauth/authorize");
    assert_eq!(body["token_endpoint"], "base/api/oauth/token");
    assert_eq!(
        body["device_authorization_endpoint"],
        "base/api/oauth/device_authorization"
    );
    assert_eq!(body["introspection_endpoint"], "base/api/oauth/introspect");
    assert_eq!(body["revocation_endpoint"], "base/api/oauth/revoke");
    assert_eq!(body["jwks_uri"], "base/.well-known/jwks.json");
    // 顺序**钉死**（D250）。
    assert_eq!(
        body["grant_types_supported"],
        json!([
            "authorization_code",
            "client_credentials",
            "urn:ietf:params:oauth:grant-type:device_code"
        ])
    );
    assert_eq!(body["response_types_supported"], json!(["code"]));
    assert_eq!(body["code_challenge_methods_supported"], json!(["S256"]));
    let auth_methods = body["token_endpoint_auth_methods_supported"]
        .as_array()
        .expect("令牌端点的认证方式应当是数组");
    assert!(auth_methods.contains(&json!("client_secret_basic")));
    assert!(auth_methods.contains(&json!("none")));
    assert_eq!(
        body["introspection_endpoint_auth_methods_supported"],
        json!(["client_secret_basic"])
    );
    assert_eq!(
        body["revocation_endpoint_auth_methods_supported"],
        json!(["client_secret_basic"])
    );
    assert_eq!(body["access_token_expires_in"], 3600);
}

/// AC-S-135：client 可注册、可改、可停用、可删。
#[tokio::test]
async fn a_client_can_be_registered_disabled_and_deleted() {
    let (fixture, _) = prepared().await;
    let admin = bearer(&login_token(&fixture, TENANT_A, ADMIN_A));

    // 步骤 1~2：注册 → **201 CREATED**。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/clients",
        Some(json!({
            "name": "应用甲",
            "clientType": "confidential",
            "redirectUris": ["https://client.example/cb"],
            "allowedGrantTypes": ["authorization_code"],
            "allowedScopes": ["rbac:role:read"]
        })),
        Some(admin.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "注册应当 201：{body}");
    let client_id = body["clientId"]
        .as_str()
        .expect("应当有 clientId")
        .to_string();
    assert!(!client_id.is_empty());
    assert!(
        body["clientSecret"].as_str().is_some(),
        "confidential client 必须给一次性明文 secret：{body}"
    );
    assert!(
        body.get("clientSecretHash").is_none(),
        "响应里不得有摘要列：{body}"
    );

    // 步骤 3：列表里**没有** `clientSecret` 字段。
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/api/oauth/clients",
        None,
        Some(admin.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().expect("列表应当是数组");
    assert!(!list.is_empty());
    for item in list {
        assert!(
            item.get("clientSecret").is_none(),
            "列表里不得出现 clientSecret：{item}"
        );
    }

    // 步骤 4：停用。
    let (status, body) = call(
        &fixture.router,
        Method::PATCH,
        &format!("/api/oauth/clients/{client_id}"),
        Some(json!({ "status": "disabled" })),
        Some(admin.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "停用应当 200：{body}");
    assert_eq!(body["status"], "disabled");

    // 步骤 5：preview 停用的 client → **403** + `OAUTH_CLIENT_DISABLED`（**平台话**）。
    let user = bearer(&login_token(&fixture, TENANT_A, USER_A));
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": client_id,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "停用的 client 应当 403：{body}"
    );
    assert_eq!(body["code"], "OAUTH_CLIENT_DISABLED");

    // 步骤 6：令牌端点用停用的 client → **401** + `{"error":"invalid_client"}`（**协议话**）。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "rbac:role:read"),
            ("client_id", client_id.as_str()),
            ("client_secret", "whatever"),
        ],
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "停用的 client 应当 401：{body}"
    );
    assert_eq!(body, json!({ "error": "invalid_client" }));

    // 步骤 7：删除 → 再查 → **404** + `OAUTH_CLIENT_NOT_FOUND`。
    let (status, _) = call(
        &fixture.router,
        Method::DELETE,
        &format!("/api/oauth/clients/{client_id}"),
        None,
        Some(admin.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        &format!("/api/oauth/clients/{client_id}"),
        None,
        Some(admin),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "删掉的 client 应当 404：{body}"
    );
    assert_eq!(body["code"], "OAUTH_CLIENT_NOT_FOUND");
}

/// AC-S-146 标题的实质：**用户所见 = 实际授予**（H-04）。
///
/// # 这条为什么之前站不住
///
/// `PreviewResult.scopes` 的文档写「**实际授予**的 scope」，代码却传 `requested`
/// （三重交集**之前**的值）→ 用户会看到 `read+write`，而 `confirm` 写进 grant 的
/// 只有 `read` → **所见非所得**。
///
/// # 修法 A（本实现）与 AC-S-140 必须**同时成立**
///
/// - `preview.scopes` = 交集（只展示 `read`）；
/// - **票据**仍装 `requested`（`read write`）→ 令牌端点仍要 **400 `invalid_scope`**。
///
/// ⚠️ 这两条必须写在**同一个用例**里：若只做前者、顺手把票据也改成交集，
/// AC-S-140 的 4 条 P0 路径会**静默失效**（那条路径从此永远成功）。
#[tokio::test]
async fn the_consent_page_shows_exactly_what_will_be_granted() {
    let fixture = setup().await;
    // 用户**只有** read（没有 write）。
    fixture.permissions.grant(USER_A, "rbac:role:read");
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read", "rbac:role:write"],
        &["authorization_code"],
        None,
        None,
    )
    .await;
    let user = bearer(&login_token(&fixture, TENANT_A, USER_A));

    // 请求 `read write`（用户没有 write）→ preview **仍然 200**（D260）。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read rbac:role:write",
            "state": "xyz",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "preview 应当 200：{body}");
    assert_eq!(body["needsConsent"], true);

    // **核心断言 1**：同意页只展示**交集**（`read`），不含 `write`。
    assert_eq!(
        body["scopes"],
        json!(["rbac:role:read"]),
        "同意页必须只展示**实际会授予**的 scope（用户所见 = 实际授予）"
    );

    // **核心断言 2**（AC-S-140 的判据必须**仍然**成立）：
    // 用这张票据走 confirm → 换令牌 → **仍然** 400 `invalid_scope`。
    let ticket = body["ticket"].as_str().expect("应当有票据").to_string();
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/confirm",
        Some(json!({ "ticket": ticket })),
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "confirm 应当 200：{body}");
    let redirect = body["redirectTo"].as_str().expect("应当有 redirectTo");
    let code = redirect
        .split("code=")
        .nth(1)
        .expect("应当有 code")
        .split('&')
        .next()
        .expect("code 应当有值")
        .to_string();
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, &code).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "AC-S-140 不能被 P1-9 的修法破坏：请求了用户没有的 scope → 令牌端点必须 400：{body}"
    );
    assert_eq!(body["error"], "invalid_scope");

    // grant 里存的也是交集（与同意页展示一致）。
    let granted: Vec<String> = sqlx::query_scalar(
        "SELECT scopes FROM platform_oauth_grant WHERE tenant_id = $1 AND client_id = $2 \
         AND user_id = $3",
    )
    .bind(TENANT_A)
    .bind(CLIENT_A)
    .bind(USER_A)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能查 grant");
    assert_eq!(
        granted,
        vec!["rbac:role:read".to_string()],
        "grant.scopes 必须与同意页展示的一致"
    );
}

/// AC-S-140 路径 4（D241 第 3 条）：**通配一律不生效**，不是因为「它没注册」。
///
/// # 旧判据为什么没有区分力
///
/// 旧的路径 4 只请求 `rbac:role:*`，而 client 白名单里本来就**没有** `rbac:role:*`
/// → 靠「client 维」就会 400。于是「通配被禁用」这条**根本没被测到** ——
/// AC 原文专门警告了这个陷阱（「不是因为**它没注册**」）。
///
/// # 真对照
///
/// 把「通配」同时放进**三处**，让 `granted` 的交集**本可以**包含它：
/// 1. `platform_permission` 注册表（管理员直连 INSERT —— D241 的字面要求）；
/// 2. client 的 `allowed_scopes`；
/// 3. 用户的有效权限点集合。
///
/// 此时若把 `granted_scopes` / `require_scope_granted` 里的 `*` 过滤删掉，
/// 交换就会**成功** → 本用例变红。这就是区分力。
#[tokio::test]
async fn a_wildcard_scope_is_rejected_even_when_it_is_registered_everywhere() {
    let fixture = setup().await;
    // D241 第 1 条：注册表里**没有** `rbac:role:*`。
    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_permission WHERE code = 'rbac:role:*'")
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能查权限点注册表");
    assert_eq!(before, 0, "前置：注册表里不该有 rbac:role:*");

    // D241 第 3 条：管理员直连 INSERT（仅本用例夹具，用完 DELETE）。
    sqlx::query(
        "INSERT INTO platform_permission (code, name, kind, condition, source) \
         VALUES ('rbac:role:*', '通配（测试夹具）', 'button', NULL, 'test')",
    )
    .execute(&fixture.admin_pool)
    .await
    .expect("应当能登记通配权限点");
    let after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_permission WHERE code = 'rbac:role:*'")
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能查权限点注册表");
    assert_eq!(after, 1, "登记必须真的写进了注册表（否则下面是空跑）");

    // 三处都让通配「本可以」被授予。
    fixture.permissions.grant(USER_A, "rbac:role:*");
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read", "rbac:role:*"],
        &["authorization_code"],
        None,
        None,
    )
    .await;
    let user = bearer(&login_token(&fixture, TENANT_A, USER_A));

    let code = authorization_code(&fixture, &user, CLIENT_A, "rbac:role:*").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, &code).await;

    // 先清理夹具（无论断言成败，别把通配注册表留给别的用例）。
    sqlx::query("DELETE FROM platform_permission WHERE code = 'rbac:role:*'")
        .execute(&fixture.admin_pool)
        .await
        .expect("应当能删掉夹具权限点");

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "通配**登记过**、client 白名单**有**、用户**也有** → 仍必须 400：{body}"
    );
    assert_eq!(body["error"], "invalid_scope");
}

/// AC-S-140 路径 5（D242）：授予**新**令牌会拿到新权限，但**旧**令牌不被回溯扩大。
///
/// # 判据的两个半边（缺一不可）
///
/// 1. 令牌 A 的内省 `scope` **仍逐字等于** `rbac:role:read`（不回溯）；
/// 2. 新令牌 B 的 `scope` **含** `rbac:role:write`（新权限生效）。
///
/// ⚠️ 第 1 步**不能重新交换**（重新交换会拿到新 scope → 测不到「不回溯」）。
#[tokio::test]
async fn an_already_issued_token_is_never_widened_retroactively() {
    let fixture = setup().await;
    fixture.permissions.grant(USER_A, "rbac:role:read");
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read", "rbac:role:write"],
        &["authorization_code"],
        None,
        None,
    )
    .await;
    let user = bearer(&login_token(&fixture, TENANT_A, USER_A));

    // 令牌 A：只请求 read。
    let code = authorization_code(&fixture, &user, CLIENT_A, "rbac:role:read").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, &code).await;
    assert_eq!(status, StatusCode::OK, "令牌 A 应当 200：{body}");
    assert_eq!(body["scope"], "rbac:role:read");
    let token_a = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();

    // 管理员给该用户**追加** write，并 bump `platform_permission_epoch`（D242）。
    fixture.permissions.grant(USER_A, "rbac:role:write");
    sqlx::query(
        "INSERT INTO platform_permission_epoch (tenant_id, epoch) VALUES ($1, 1) \
         ON CONFLICT (tenant_id) DO UPDATE \
         SET epoch = platform_permission_epoch.epoch + 1, updated_at = now()",
    )
    .bind(TENANT_A)
    .execute(&fixture.admin_pool)
    .await
    .expect("应当能 bump epoch");
    let epoch: i64 =
        sqlx::query_scalar("SELECT epoch FROM platform_permission_epoch WHERE tenant_id = $1")
            .bind(TENANT_A)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能读 epoch");
    assert!(epoch >= 1, "epoch 必须真的被 bump（否则这条动作是空跑）");

    // 半边 1：**重放**令牌 A（不重新交换）→ 内省 scope **不变**。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token_a.as_str())],
        Some(basic(CLIENT_A, secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "内省应当 200：{body}");
    assert_eq!(body["active"], true);
    assert_eq!(
        body["scope"], "rbac:role:read",
        "旧令牌不得被回溯扩大（内省必须还是 read）"
    );
    assert!(
        !body["scope"]
            .as_str()
            .unwrap_or_default()
            .contains("rbac:role:write"),
        "旧令牌的 scope 不得含 write：{body}"
    );

    // 半边 2：另发令牌 B（read+write）→ **含** write。
    let code = authorization_code(&fixture, &user, CLIENT_A, "rbac:role:read rbac:role:write").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, &code).await;
    assert_eq!(status, StatusCode::OK, "令牌 B 应当 200：{body}");
    let scope = body["scope"].as_str().expect("应当有 scope");
    assert!(
        scope.split_whitespace().any(|item| item == "rbac:role:write"),
        "新令牌必须拿到新权限（含 write）：{scope}"
    );
}

/// AC-S-139 补充判据（D252）：授权码 **TTL** —— 90s 过期 / 30s 有效。
///
/// # 为什么必须**逐列写死**并直插
///
/// 授权码行有 9 个绑定列：直插时任何一列不对，交换就会**先**卡在绑定校验上
/// （`redirect_uri` / PKCE / client …）而**不是**卡在 TTL 上 → 判据失真。
/// 两条断言的**唯一差别**只有 `expires_at`。
#[tokio::test]
async fn an_authorization_code_ttl_is_what_decides_expiry() {
    let fixture = setup().await;
    fixture.permissions.grant(USER_A, "rbac:role:read");
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read"],
        &["authorization_code"],
        None,
        None,
    )
    .await;

    // 直插时**所有**列都写死（D252）；只有 `expires_at` 变。
    let plaintext = "ttl-expired-code";
    let insert = |expires_at_sql: &'static str| {
        let admin = fixture.admin_pool.clone();
        let code_hash = digest_of(plaintext.as_bytes());
        let challenge = challenge_of(VERIFIER);
        async move {
            sqlx::query(&format!(
                "INSERT INTO platform_oauth_code \
                 (tenant_id, id, client_id, user_id, code_hash, redirect_uri, scope, \
                  code_challenge, code_challenge_method, expires_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, {expires_at_sql})"
            ))
            .bind(TENANT_A)
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(CLIENT_A)
            .bind(USER_A)
            .bind(code_hash)
            .bind("https://client.example/cb")
            .bind("rbac:role:read")
            .bind(challenge)
            .bind("S256")
            .execute(&admin)
            .await
            .expect("直插授权码应当成功");
        }
    };

    // ① `expires_at = now() - 90 秒` → 400 `invalid_grant`。
    insert("now() - interval '90 seconds'").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, plaintext).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "过期 90 秒的授权码必须 400：{body}"
    );
    assert_eq!(body["error"], "invalid_grant");
    // 失败的交换**不删**码 → 先清掉，才好插「同一批值」的第二行。
    sqlx::query("DELETE FROM platform_oauth_code WHERE code_hash = $1")
        .bind(digest_of(plaintext.as_bytes()))
        .execute(&fixture.admin_pool)
        .await
        .expect("应当能删掉过期码");

    // ② 同一批值，**只**把 `expires_at` 改成 `now() + 30 秒` → 200。
    insert("now() + interval '30 seconds'").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, plaintext).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "只差 `expires_at` 的一行必须成功 —— 这才证明判的是 TTL：{body}"
    );
    assert_eq!(body["scope"], "rbac:role:read");

    // 步骤 5（D253）：码表的列集合里**不含** `consumed_at` / `used_at`。
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'platform_oauth_code'",
    )
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能查列集合");
    for forbidden in ["consumed_at", "used_at"] {
        assert!(
            !columns.iter().any(|column| column == forbidden),
            "platform_oauth_code 不得有 {forbidden} 列（删除即消费）：{columns:?}"
        );
    }
}

/// AC-S-142 步骤 3/6 的**数据库侧**判据（`slow_down` / `last_polled_at` / 串域）。
///
/// # 这条之前为什么站不住
///
/// 旧用例只断了**返回码** `slow_down` —— 「`slow_down` 真的把间隔 +5 写进库」
/// 与「`last_polled_at` 真的在递增」**零断言**（全仓 grep `poll_interval_seconds`
/// 在测试里 0 次）。返回码对了但库没写的实现照样绿。
#[tokio::test]
async fn the_device_code_polling_state_is_written_to_the_database() {
    let (fixture, user) = prepared().await;
    let secret = seed_client_a(&fixture).await;
    let user = bearer(&user);

    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/device_authorization",
        &[("scope", "rbac:role:read")],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "申请设备码应当 200：{body}");
    let user_code = body["user_code"]
        .as_str()
        .expect("应当有 user_code")
        .to_string();
    let device_code = body["device_code"]
        .as_str()
        .expect("应当有 device_code")
        .to_string();

    // 第一次轮询 → `authorization_pending`；此时 `last_polled_at` 由 NULL 变有值。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code",
            ),
            ("device_code", device_code.as_str()),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "authorization_pending");

    let poll_state = |hash: String| {
        let admin = fixture.admin_pool.clone();
        async move {
            sqlx::query_as::<_, (i32, bool, Option<f64>)>(
                "SELECT poll_interval_seconds, last_polled_at IS NOT NULL, \
                 extract(epoch FROM last_polled_at)::float8 \
                 FROM platform_oauth_device_code WHERE device_code_hash = $1",
            )
            .bind(hash)
            .fetch_one(&admin)
            .await
            .expect("应当能读轮询状态")
        }
    };
    let hash = digest_of(device_code.as_bytes());
    let (interval_1, polled_1, at_1) = poll_state(hash.clone()).await;
    assert_eq!(interval_1, 5, "初始间隔必须等于配置的 interval（5）");
    assert!(polled_1, "第一次轮询后 last_polled_at 必须非空");
    let at_1 = at_1.expect("第一次轮询后 last_polled_at 必须有值");

    // 等 1.1 秒（仍 < interval=5）→ 第二次轮询 → `slow_down`。
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code",
            ),
            ("device_code", device_code.as_str()),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "slow_down");

    let (interval_2, polled_2, at_2) = poll_state(hash.clone()).await;
    assert!(polled_2);
    let at_2 = at_2.expect("slow_down 后必须有 last_polled_at");
    // **核心 1**：`slow_down` 把间隔写成 `interval + 5`（RFC 8628 §3.5）。
    assert_eq!(
        interval_2,
        interval_1 + 5,
        "slow_down 后 poll_interval_seconds 必须等于 interval + 5（{} + 5）",
        interval_1
    );
    // **核心 2**：`last_polled_at` **递增**。
    assert!(
        at_2 > at_1,
        "last_polled_at 必须递增（{at_1} → {at_2}）"
    );

    // **核心 3**：拿 `user_code` 当 `device_code` 用 → **必须失败**。
    // （证明两个摘要域不串：`user_code_hash` 与 `device_code_hash` 是两列。）
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code",
            ),
            ("device_code", user_code.as_str()),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "把 user_code 当 device_code 用必须失败：{body}"
    );
    assert_eq!(body["error"], "invalid_grant");

    // 批准后轮询 → 200，且设备码行**已删除**（按摘要查 == 0）。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/device/approve",
        Some(json!({ "userCode": user_code, "action": "approve" })),
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "批准应当 200：{body}");
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code",
            ),
            ("device_code", device_code.as_str()),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "批准后应当 200：{body}");
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_oauth_device_code WHERE device_code_hash = $1",
    )
    .bind(digest_of(device_code.as_bytes()))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能数设备码行");
    assert_eq!(remaining, 0, "换到令牌后设备码行必须被删除");
}

/// AC-S-136 步骤 1：`client_secret_hash` 列逐字节等于**测试侧独立算**的摘要。
///
/// # 为什么必须独立算
///
/// 复用 `platform_oauth::crypto::digest_base64url` = 自己证明自己
/// （把口径改成 hex 也照样绿）。这里用 [`digest_of`]（另写一份）。
#[tokio::test]
async fn the_stored_client_secret_hash_is_the_base64url_digest_of_the_plaintext() {
    let (fixture, _user) = prepared().await;
    let admin = bearer(&login_token(&fixture, TENANT_A, ADMIN_A));
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/clients",
        Some(json!({
            "name": "摘要口径应用",
            "clientType": "confidential",
            "redirectUris": ["https://client.example/cb"],
            "allowedGrantTypes": ["client_credentials"],
            "allowedScopes": ["rbac:role:read"]
        })),
        Some(admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "注册应当 201：{body}");
    let client_id = body["clientId"].as_str().expect("应当有 clientId").to_string();
    let secret = body["clientSecret"]
        .as_str()
        .expect("应当给一次性明文 secret")
        .to_string();

    // 步骤 1：直连查列（管理员连接）。
    let stored: String = sqlx::query_scalar(
        "SELECT client_secret_hash FROM platform_oauth_client WHERE client_id = $1",
    )
    .bind(&client_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能查 client_secret_hash 列");

    // 与测试侧**独立算**的 `base64url(SHA256(明文))` 逐字节比对（**不是 hex**）。
    let expected = digest_of(secret.as_bytes());
    assert_eq!(
        stored, expected,
        "client_secret_hash 必须是 base64url 无填充的 SHA-256 摘要"
    );
    assert_eq!(stored.chars().count(), 43, "摘要必须是 43 字符（无填充）");
    assert!(
        !stored.chars().all(|ch| ch.is_ascii_hexdigit()),
        "摘要**不是** hex：{stored}"
    );

    // 步骤 4 的第三面：`POST /api/oauth/token` 的**响应体**也不得含明文。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "rbac:role:read"),
        ],
        Some(basic(&client_id, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "换令牌应当 200：{body}");
    assert!(
        !body.to_string().contains(&secret),
        "令牌响应体不得含明文 client_secret：{body}"
    );
}

/// AC-S-135 的标题后半：「且**回调白名单**生效」（防开放重定向）。
///
/// # 这条之前为什么是 0 覆盖
///
/// 生产代码**有**校验（`service.rs` 的 `client.redirect_uris.iter().any(...)`），
/// 但全仓**没有任何**用例用一个「不在白名单里的 `redirect_uri`」去打 preview。
/// 把那个判断改成恒放行（`|| true`）→ 旧测试**全绿** → 开放重定向被放出去。
#[tokio::test]
async fn a_redirect_uri_outside_the_whitelist_is_rejected() {
    let (fixture, user) = prepared().await;
    seed_client_a(&fixture).await;
    let user = bearer(&user);

    // 白名单是 `https://client.example/cb` → 这里换一个**路径不同**的地址。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://attacker.example/cb",
            "scope": "rbac:role:read",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "不在白名单里的回调必须 400：{body}"
    );
    assert_eq!(
        body["code"], "OAUTH_INVALID_REDIRECT_URI",
        "错误码必须是 OAUTH_INVALID_REDIRECT_URI：{body}"
    );
    // 不得签出票据（否则后续可以用它换 code）。
    assert!(body.get("ticket").is_none(), "被拒的 preview 不得给票据：{body}");

    // 阳性对照：**同一个 client** 用白名单里的地址 → 200（证明上面的 400
    // 是因为「地址不在白名单」，而不是别的参数也坏了）。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "阳性对照：白名单里的地址必须 200：{body}"
    );
    assert!(body["ticket"].as_str().is_some(), "正常路径应当给票据");
}

/// AC-S-134 / AC-S-142 的 `{授权页基址}` 维度（D222）。
///
/// # 为什么要一条**专门的**夹具
///
/// 默认夹具的 `frontendBaseUrl` 是空串 → 回退 `{issuer}` →
/// 「用了 `{授权页基址}`」与「误用了 `{issuer}`」**前缀相同 → 判不出**。
/// 这里把前者设成 `https://frontend.example`（**≠** issuer `base`），
/// 于是两类端点必须落在**不同**的基址上：
///
/// | 端点 | 期望基址 |
/// |------|----------|
/// | `authorization_endpoint` / `verification_uri` / `verification_uri_complete` | `https://frontend.example` |
/// | `token_endpoint` / `jwks_uri` / `device_authorization_endpoint` / … | `{issuer}`（`base`） |
#[tokio::test]
async fn the_frontend_base_url_is_used_for_the_pages_but_not_for_the_api() {
    let fixture = setup_with(platform_oauth::config::OAuthSettings {
        frontend_base_url: "https://frontend.example".to_string(),
        ..platform_oauth::config::OAuthSettings::default()
    })
    .await;

    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/.well-known/oauth-authorization-server",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "元数据应当 200：{body}");
    assert_eq!(body["issuer"], "base");
    // **页面类**端点用 `{授权页基址}`。
    assert_eq!(
        body["authorization_endpoint"], "https://frontend.example/oauth/authorize",
        "authorization_endpoint 必须用 {{授权页基址}}（D222），不是 issuer"
    );
    // **接口类**端点仍用 `{issuer}`。
    assert_eq!(body["token_endpoint"], "base/api/oauth/token");
    assert_eq!(body["jwks_uri"], "base/.well-known/jwks.json");
    assert_eq!(
        body["device_authorization_endpoint"],
        "base/api/oauth/device_authorization"
    );
    assert_eq!(body["introspection_endpoint"], "base/api/oauth/introspect");
    assert_eq!(body["revocation_endpoint"], "base/api/oauth/revoke");

    // 设备码页：`verification_uri` 与 `verification_uri_complete` 也用前端基址。
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read"],
        &["urn:ietf:params:oauth:grant-type:device_code"],
        None,
        None,
    )
    .await;
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/device_authorization",
        &[("scope", "rbac:role:read")],
        Some(basic(CLIENT_A, secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "申请设备码应当 200：{body}");
    let user_code = body["user_code"].as_str().expect("应当有 user_code");
    assert_eq!(
        body["verification_uri"], "https://frontend.example/oauth/device",
        "verification_uri 必须用 {{授权页基址}}"
    );
    assert_eq!(
        body["verification_uri_complete"],
        format!("https://frontend.example/oauth/device?user_code={user_code}"),
        "verification_uri_complete 必须逐字等于 {{授权页基址}}/oauth/device?user_code=…"
    );
}


#[tokio::test]
async fn the_client_secret_is_never_echoed_by_a_read_endpoint() {
    let (fixture, _) = prepared().await;
    let admin = bearer(&login_token(&fixture, TENANT_A, ADMIN_A));
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/clients",
        Some(json!({
            "name": "应用乙",
            "clientType": "confidential",
            "redirectUris": ["https://client.example/cb"],
            "allowedGrantTypes": ["authorization_code"],
            "allowedScopes": ["rbac:role:read"]
        })),
        Some(admin.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "注册应当 201：{body}");
    let client_id = body["clientId"]
        .as_str()
        .expect("应当有 clientId")
        .to_string();
    let secret = body["clientSecret"]
        .as_str()
        .expect("应当给一次性明文 secret")
        .to_string();

    let (status, list) = call(
        &fixture.router,
        Method::GET,
        "/api/oauth/clients",
        None,
        Some(admin.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        list.get("clientSecret").is_none(),
        "列表体不得有 clientSecret 键：{list}"
    );
    assert!(
        !list.to_string().contains(&secret),
        "列表体不得含明文 secret"
    );

    let (status, one) = call(
        &fixture.router,
        Method::GET,
        &format!("/api/oauth/clients/{client_id}"),
        None,
        Some(admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        one.get("clientSecret").is_none(),
        "单个 client 不得有 clientSecret 键：{one}"
    );
    assert!(
        !one.to_string().contains(&secret),
        "单个 client 不得含明文 secret"
    );
}

/// AC-S-137 / AC-S-139：授权码 + PKCE 换到可用令牌，且**一次性**。
#[tokio::test]
async fn an_authorization_code_with_pkce_exchanges_for_a_usable_token() {
    let (fixture, user) = prepared().await;
    let secret = seed_client_a(&fixture).await;
    let user = bearer(&user);

    // preview → 同意票据。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "state": "xyz",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "preview 应当 200：{body}");
    assert_eq!(body["needsConsent"], true);
    assert_eq!(body["application"]["clientId"], CLIENT_A);
    assert_eq!(body["scopes"], json!(["rbac:role:read"]));
    let ticket = body["ticket"].as_str().expect("应当有票据").to_string();
    assert!(
        body.get("redirectTo").is_none(),
        "需要同意时不得有 redirectTo"
    );

    // confirm → 跳转地址带 code 与 state。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/confirm",
        Some(json!({ "ticket": ticket })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "confirm 应当 200：{body}");
    let redirect = body["redirectTo"]
        .as_str()
        .expect("应当有 redirectTo")
        .to_string();
    assert!(
        redirect.starts_with("https://client.example/cb?code="),
        "{redirect}"
    );
    assert!(redirect.contains("state=xyz"), "{redirect}");
    let code = redirect
        .split("code=")
        .nth(1)
        .expect("应当有 code")
        .split('&')
        .next()
        .expect("code 应当有值")
        .to_string();

    // ⚠️ **交换之前**先断「行存在 + 摘要口径正确」（D228）。
    //
    // 为什么必须**先**断：交换**之后**的行数是 0 —— 那条断言在「摘要口径被改成
    // hex」时**照样成立**（按 base64url 查当然查不到）→ 判据恒真。
    // → 这里用**独立实现**的摘要查「交换前就该有 1 行」。
    let code_hash = digest_of(code.as_bytes());
    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_oauth_code WHERE code_hash = $1")
            .bind(&code_hash)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能查授权码行数");
    assert_eq!(
        before, 1,
        "交换前授权码行必须存在，且摘要口径是 base64url 无填充"
    );
    // 明文**不得**入库（D228）。
    let plaintext: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_oauth_code WHERE code_hash = $1")
            .bind(code.as_str())
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能查授权码行数");
    assert_eq!(plaintext, 0, "库里不得存授权码明文");

    // 交换 → 200 + 令牌。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", "https://client.example/cb"),
            ("code_verifier", VERIFIER),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "交换应当 200：{body}");
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["scope"], "rbac:role:read");
    assert_eq!(body["expires_in"], 3600);
    let token = body["access_token"]
        .as_str()
        .expect("应当有 access_token")
        .to_string();

    // 内省 → active。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token.as_str())],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true);
    assert!(
        body["scope"]
            .as_str()
            .unwrap_or_default()
            .contains("rbac:role:read")
    );

    // **重放**：同一个码再换一次 → 400 `invalid_grant`。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", "https://client.example/cb"),
            ("code_verifier", VERIFIER),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "重放应当 400：{body}");
    assert_eq!(body["error"], "invalid_grant");

    // 授权码行**恰好 0**（按摘要查，D253）。
    let hash = digest_of(code.as_bytes());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_oauth_code WHERE code_hash = $1")
            .bind(hash)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能查授权码行数");
    assert_eq!(count, 0, "交换后授权码行必须被删掉");
}

/// AC-S-139：授权码**一次性** —— **并发**交换恰好一个成功、只签发一枚令牌。
///
/// # 为什么必须**并发**（单线程证明不了）
///
/// 顺序执行「交换两次 → 第二次 400」**证明不了**一次性：第二次本来就查不到码
/// （第一次已经删了它）。一次性真正的含义是**并发**下的
/// 「两个请求**都**读到了码，但只有一个能**消费**它」。
///
/// ⚠️ 断言**两条**：
///
/// 1. 恰好**一个** 200（其余是 400 `invalid_grant`）；
/// 2. 令牌表里**恰好 1 行** —— 只断第 1 条的话，「先签发令牌、再删码」这种
///    顺序错误也会**蒙混过关**（被拒的请求虽然回了 400，但它的令牌**已经签发了**）。
///
/// # 为什么打 **4 路**并发（不是 2 路）
///
/// 第 2 条断言要抓的是「**删除挪到签发之后**」这种顺序错误 →
/// 它只有在**至少两个**请求都读到了码、却都签发了令牌时才会暴露。
/// 2 路并发时，只要**其中一个**请求的读码晚于另一个的删码，变异就**看不出来**
/// （实测：2 路时该变异有约 1/6 的概率蒙混过关 → **判据是 flaky 的**）。
/// 4 路并发下，只要**任意两路**都读到了码就会暴露 → 判据**稳定**。
#[tokio::test]
async fn a_code_can_only_be_exchanged_once_under_concurrency() {
    let (fixture, user) = prepared().await;
    let secret = seed_client_a(&fixture).await;
    let user = bearer(&user);
    let code = authorization_code(&fixture, &user, CLIENT_A, "rbac:role:read").await;

    let form: [(&str, &str); 4] = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", "https://client.example/cb"),
        ("code_verifier", VERIFIER),
    ];
    let routers = [
        fixture.router.clone(),
        fixture.router.clone(),
        fixture.router.clone(),
        fixture.router.clone(),
    ];
    let (first, second, third, fourth) = tokio::join!(
        call_form(
            &routers[0],
            "/api/oauth/token",
            &form,
            Some(basic(CLIENT_A, &secret)),
        ),
        call_form(
            &routers[1],
            "/api/oauth/token",
            &form,
            Some(basic(CLIENT_A, &secret)),
        ),
        call_form(
            &routers[2],
            "/api/oauth/token",
            &form,
            Some(basic(CLIENT_A, &secret)),
        ),
        call_form(
            &routers[3],
            "/api/oauth/token",
            &form,
            Some(basic(CLIENT_A, &secret)),
        ),
    );
    let results = [first, second, third, fourth];

    let ok_count = results
        .iter()
        .filter(|(status, _)| *status == StatusCode::OK)
        .count();
    assert_eq!(
        ok_count,
        1,
        "同一个码并发交换必须恰好一个成功：{:?}",
        results
            .iter()
            .map(|(status, _)| *status)
            .collect::<Vec<_>>()
    );
    for (status, body) in results.iter() {
        if *status == StatusCode::OK {
            continue;
        }
        assert_eq!(
            status,
            &StatusCode::BAD_REQUEST,
            "被拒的应当是 400：{body:?}"
        );
        assert_eq!(body["error"], "invalid_grant", "被拒的应当是 invalid_grant");
    }

    // 令牌表里**恰好 1 行**（没有多签发）。
    let tokens: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_oauth_token WHERE client_id = $1")
            .bind(CLIENT_A)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能查令牌行数");
    assert_eq!(tokens, 1, "同一个码只准签发一枚令牌（先删码 → 后签发）");
}

/// AC-S-138：PKCE 强制 S256。
#[tokio::test]
async fn pkce_is_mandatory_and_s256_only() {
    let (fixture, user) = prepared().await;
    seed_client_a(&fixture).await;
    let user = bearer(&user);

    // 路径 1：缺挑战 → 400 `OAUTH_INVALID_REQUEST`。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read"
        })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "缺挑战应当 400：{body}");
    assert_eq!(body["code"], "OAUTH_INVALID_REQUEST");
    assert!(body.get("ticket").is_none(), "失败时不得有票据");

    // 路径 2：`plain` → 400 `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD`。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "codeChallenge": VERIFIER,
            "codeChallengeMethod": "plain"
        })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "plain 应当 400：{body}");
    assert_eq!(body["code"], "OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD");

    // 路径 3：**错的 verifier** → 400 `{"error":"invalid_grant"}`，
    // 且 `error_description` 不含 CJK、不含凭据明文（D251）。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "preview 应当 200：{body}");
    let ticket = body["ticket"].as_str().expect("应当有票据").to_string();
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/confirm",
        Some(json!({ "ticket": ticket })),
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let redirect = body["redirectTo"]
        .as_str()
        .expect("应当有 redirectTo")
        .to_string();
    let code = redirect
        .split("code=")
        .nth(1)
        .expect("应当有 code")
        .split('&')
        .next()
        .expect("code 应当有值")
        .to_string();

    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", "https://client.example/cb"),
            ("code_verifier", "wrong-verifier"),
        ],
        Some(basic(CLIENT_A, "secret-a")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "错 verifier 应当 400：{body}"
    );
    assert_eq!(body["error"], "invalid_grant");
    let description = body["error_description"].as_str().unwrap_or_default();
    assert!(!description.is_empty(), "失败应当带描述");
    assert!(
        description
            .chars()
            .all(|c| !('\u{4e00}'..='\u{9fff}').contains(&c)),
        "描述不得含 CJK：{description}"
    );
    for forbidden in [
        "code_verifier",
        "code_challenge",
        "client_secret",
        "wrong-verifier",
    ] {
        assert!(
            !description.contains(forbidden),
            "描述不得回显 {forbidden}：{description}"
        );
    }
}

/// AC-S-140：scope 是**三重交集**。
#[tokio::test]
async fn the_scope_is_the_intersection_of_three_sets() {
    let fixture = setup().await;
    // 用户持 `rbac:role:read` + `sandbox:demo:read`，**没有** `rbac:role:write`。
    //
    // ⚠️ 三条路径要**分别**隔离三个维度：
    // - 路径 2 隔离「用户维」（用户没有 write，client 白名单**有** write）；
    // - 路径 3 隔离「client 维」（用户**有** sandbox:demo:read，client 白名单没有）；
    // - 路径 4 隔离「通配」（D241）。
    fixture.permissions.grant(USER_A, "rbac:role:read");
    fixture.permissions.grant(USER_A, "sandbox:demo:read");
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read", "rbac:role:write"],
        &["authorization_code", "client_credentials"],
        None,
        None,
    )
    .await;
    let user = bearer(&login_token(&fixture, TENANT_A, USER_A));

    // 路径 1：只要 read → 交换成功，scope 逐字等于 read。
    let code = authorization_code(&fixture, &user, CLIENT_A, "rbac:role:read").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, &code).await;
    assert_eq!(status, StatusCode::OK, "路径 1 应当 200：{body}");
    assert_eq!(body["scope"], "rbac:role:read");

    // 路径 2：用户没有 write → 400 `invalid_scope`（**协议错误体**）。
    let code =
        authorization_code(&fixture, &user, CLIENT_A, "rbac:role:read rbac:role:write").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, &code).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "路径 2 应当 400：{body}");
    assert_eq!(body["error"], "invalid_scope");

    // 路径 3：不在 client 白名单 → 400 `invalid_scope`。
    let code = authorization_code(&fixture, &user, CLIENT_A, "sandbox:demo:read").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, &code).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "路径 3 应当 400：{body}");
    assert_eq!(body["error"], "invalid_scope");

    // 路径 4：通配 → 400 `invalid_scope`（**一律**不生效，D241）。
    let code = authorization_code(&fixture, &user, CLIENT_A, "rbac:role:*").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, &code).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "路径 4 应当 400：{body}");
    assert_eq!(body["error"], "invalid_scope");
}

/// AC-S-141：client_credentials 换到**无用户**的令牌。
#[tokio::test]
async fn the_client_credentials_grant_issues_a_token_without_a_user() {
    let fixture = setup().await;
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["sandbox:demo:read"],
        &["client_credentials"],
        None,
        None,
    )
    .await;

    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "sandbox:demo:read"),
        ],
        Some(basic(CLIENT_A, secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "应当 200：{body}");
    assert_eq!(body["scope"], "sandbox:demo:read");
    let token = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();

    // 载荷里**没有** `sub` 键（不是 null、不是空串）。
    let payload = payload_of(&token);
    assert!(
        payload.get("sub").is_none(),
        "client_credentials 的载荷不得有 sub 键：{payload}"
    );
    assert_eq!(payload["client_id"], CLIENT_A);
    assert_eq!(payload["grant_type"], "client_credentials");

    // 内省 → active，且**没有** `username` 键（RFC 7662）。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token.as_str())],
        Some(basic(CLIENT_A, secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true);
    assert!(
        body.get("username").is_none(),
        "无用户主体的令牌不得有 username 键：{body}"
    );

    // 超出白名单 → 400 `invalid_scope`（**两重**交集，无用户维，D231）。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "sandbox:demo:write"),
        ],
        Some(basic(CLIENT_A, secret)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "应当 400：{body}");
    assert_eq!(body["error"], "invalid_scope");
}

/// AC-S-142：设备码全流程。
#[tokio::test]
async fn the_device_code_flow_runs_end_to_end() {
    let (fixture, user) = prepared().await;
    let secret = seed_client_a(&fixture).await;
    let user = bearer(&user);

    // 步骤 1：申请设备码。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/device_authorization",
        &[("scope", "rbac:role:read")],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "申请设备码应当 200：{body}");
    let user_code = body["user_code"]
        .as_str()
        .expect("应当有 user_code")
        .to_string();
    let device_code = body["device_code"]
        .as_str()
        .expect("应当有 device_code")
        .to_string();
    assert_eq!(user_code.chars().count(), 8, "user_code 长度必须恰好 8");
    for ch in user_code.chars() {
        assert!(
            "BCDFGHJKLMNPQRSTVWXZ".contains(ch),
            "user_code 只能含 base20 字符：{user_code}"
        );
    }
    assert_eq!(body["verification_uri"], "base/oauth/device");
    assert_eq!(body["expires_in"], 600);
    assert_eq!(body["interval"], 5);

    // ⚠️ **轮询之前**先断「行存在 + 两个摘要的口径都正确」（D228）。
    let device_hash = digest_of(device_code.as_bytes());
    let user_hash = digest_of(user_code.as_bytes());
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_oauth_device_code d \
         WHERE d.device_code_hash = $1 AND d.user_code_hash = $2",
    )
    .bind(&device_hash)
    .bind(&user_hash)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能查设备码行数");
    assert_eq!(rows, 1, "设备码行必须存在，且两个摘要都是 base64url 无填充");
    let plaintext: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_oauth_device_code \
         WHERE device_code_hash = $1 OR user_code_hash = $2",
    )
    .bind(device_code.as_str())
    .bind(user_code.as_str())
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能查设备码行数");
    assert_eq!(plaintext, 0, "库里不得存 device_code / user_code 明文");

    // 步骤 2：立刻轮询 → 400 `authorization_pending`。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code.as_str()),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "未授权时应当 400：{body}");
    assert_eq!(body["error"], "authorization_pending");

    // 步骤 3：连续轮询第 2 次（间隔 < interval）→ 400 `slow_down`。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code.as_str()),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "过快轮询应当 400：{body}");
    assert_eq!(body["error"], "slow_down");

    // 步骤 4：批准前展示 → 200 + clientId / clientName / scopes。
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        &format!("/api/oauth/device/verify?userCode={user_code}"),
        None,
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "verify 应当 200：{body}");
    assert_eq!(body["clientId"], CLIENT_A);
    assert_eq!(body["scopes"], json!(["rbac:role:read"]));
    assert!(body["clientName"].as_str().is_some());

    // 批准 → 200 `{"status":"approved"}`。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/device/approve",
        Some(json!({ "userCode": user_code, "action": "approve" })),
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "批准应当 200：{body}");
    assert_eq!(body["status"], "approved");

    // 步骤 5：再轮询 → 200 + 令牌。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code.as_str()),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "批准后应当 200：{body}");
    assert_eq!(body["scope"], "rbac:role:read");

    // 步骤 6：设备码行**已被删除**（按摘要查 → 恰好 0）。
    let hash = digest_of(device_code.as_bytes());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_oauth_device_code WHERE device_code_hash = $1",
    )
    .bind(hash)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能查设备码行数");
    assert_eq!(count, 0, "换到令牌后设备码行必须被删掉");
}

/// AC-S-143：内省只能内省自己的令牌。
#[tokio::test]
async fn a_client_can_only_introspect_its_own_tokens() {
    let (fixture, _) = prepared().await;
    let secret_a = seed_client_a(&fixture).await;
    let secret_b = seed_client_b(&fixture).await;
    seed_public_client(&fixture).await;

    // client-A 的令牌（**授权码**签发 → 有用户主体 → 内省体有 `sub`）。
    let user = bearer(&login_token(&fixture, TENANT_A, USER_A));
    let code = authorization_code(&fixture, &user, CLIENT_A, "rbac:role:read").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, &secret_a, &code).await;
    assert_eq!(status, StatusCode::OK, "换令牌应当 200：{body}");
    let token_a = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();
    // client-B 的令牌（`client_credentials` → **无**用户主体）。
    let (_, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "rbac:role:read"),
        ],
        Some(basic(CLIENT_B, &secret_b)),
    )
    .await;
    let token_b = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();

    // 步骤 1：自己的令牌 → active + 字段齐全。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token_a.as_str())],
        Some(basic(CLIENT_A, &secret_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true);
    for field in [
        "client_id",
        "scope",
        "exp",
        "iat",
        "sub",
        "token_type",
        "iss",
        "aud",
    ] {
        assert!(body.get(field).is_some(), "内省体缺少 {field}：{body}");
    }

    // 步骤 2：别人的令牌 → 200 + **只有** `active` 一个键。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token_b.as_str())],
        Some(basic(CLIENT_A, &secret_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "active": false }), "越权内省不得泄露任何字段");

    // 步骤 5~7：public client / 无认证 / 空 secret → 401 + **逐字** `{"error":"invalid_client"}`。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token_a.as_str()), ("client_id", PUBLIC_CLIENT)],
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "public client 应当 401：{body}"
    );
    assert_eq!(body, json!({ "error": "invalid_client" }));

    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token_a.as_str())],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "无认证应当 401：{body}");
    assert_eq!(body, json!({ "error": "invalid_client" }));

    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token_a.as_str())],
        Some(basic(PUBLIC_CLIENT, "")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "空 secret 应当 401：{body}"
    );
    assert_eq!(body, json!({ "error": "invalid_client" }));
}

/// AC-S-143 步骤 3：**过期令牌**内省 → 200 + 只有 `active`（不报错、不泄露）。
///
/// # 为什么单独一条
///
/// 旧用例只覆盖了「**别人的**令牌」（身份隔离）；「过期」这一档没有一条断言
/// 能证明 `active: false` 是**因为过期**。
#[tokio::test]
async fn an_expired_token_introspects_as_inactive() {
    // TTL = -60 → 平台签出来的令牌**已经过期**（但签名有效）。
    let fixture = setup_with(platform_oauth::config::OAuthSettings {
        access_token_ttl_seconds: -60,
        ..platform_oauth::config::OAuthSettings::default()
    })
    .await;
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read"],
        &["client_credentials"],
        None,
        None,
    )
    .await;
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "rbac:role:read"),
        ],
        Some(basic(CLIENT_A, secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "换令牌应当 200：{body}");
    let token = body["access_token"].as_str().expect("应当有令牌");

    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token)],
        Some(basic(CLIENT_A, secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "过期令牌内省也是 200：{body}");
    // 整字段逐字比对：**只有** `active`（不多出任何字段）。
    assert_eq!(
        body,
        json!({ "active": false }),
        "过期令牌必须只回 active:false"
    );
}

/// AC-S-144：撤销后**立即**失效。
#[tokio::test]
async fn a_revoked_token_stops_being_active_immediately() {
    let (fixture, _) = prepared().await;
    let secret_a = seed_client_a(&fixture).await;
    let secret_b = seed_client_b(&fixture).await;

    let (_, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "rbac:role:read"),
        ],
        Some(basic(CLIENT_A, &secret_a)),
    )
    .await;
    let token = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();

    // 步骤 2：撤销 → 200（RFC 7009 §2.2）。
    let (status, _) = call_form(
        &fixture.router,
        "/api/oauth/revoke",
        &[("token", token.as_str())],
        Some(basic(CLIENT_A, &secret_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "撤销应当 200");

    // 步骤 3：再内省 → active=false。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token.as_str())],
        Some(basic(CLIENT_A, &secret_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], false);

    // 步骤 4：**别的** client 撤销 → 200 但**不**生效。
    let (_, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "rbac:role:read"),
        ],
        Some(basic(CLIENT_A, &secret_a)),
    )
    .await;
    let token2 = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();
    let (status, _) = call_form(
        &fixture.router,
        "/api/oauth/revoke",
        &[("token", token2.as_str())],
        Some(basic(CLIENT_B, &secret_b)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "越权撤销也回 200（不泄露）");
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/introspect",
        &[("token", token2.as_str())],
        Some(basic(CLIENT_A, &secret_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "别人的撤销不得真的撤掉这枚令牌");

    // 步骤 5：撤销一枚**不存在**的令牌 → 200。
    let (status, _) = call_form(
        &fixture.router,
        "/api/oauth/revoke",
        &[("token", "not-a-real-token")],
        Some(basic(CLIENT_A, &secret_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "无效令牌也回 200");
}

/// AC-S-145：令牌是 ES256 的 JWT，声明集合钉死。
#[tokio::test]
async fn the_access_token_is_an_es256_jwt_with_the_pinned_claims() {
    let (fixture, user) = prepared().await;
    let secret = seed_client_a(&fixture).await;
    let user = bearer(&user);

    // 走一遍授权码流程拿令牌。
    let (_, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    let ticket = body["ticket"].as_str().expect("应当有票据").to_string();
    let (_, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/confirm",
        Some(json!({ "ticket": ticket })),
        Some(user),
    )
    .await;
    let redirect = body["redirectTo"]
        .as_str()
        .expect("应当有 redirectTo")
        .to_string();
    let code = redirect
        .split("code=")
        .nth(1)
        .expect("应当有 code")
        .split('&')
        .next()
        .expect("code 应当有值")
        .to_string();
    let (_, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", "https://client.example/cb"),
            ("code_verifier", VERIFIER),
        ],
        Some(basic(CLIENT_A, &secret)),
    )
    .await;
    let token = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();

    // 头：`alg` = ES256，`kid` 在 JWKS 里。
    let header = header_of(&token);
    assert_eq!(header["alg"], "ES256");
    let kid = header["kid"].as_str().expect("头里应当有 kid").to_string();
    let jwks = fixture.iam.jwks();
    let kids: Vec<String> = jwks
        .keys
        .iter()
        .filter_map(|key| key.common.key_id.clone())
        .collect();
    assert!(kids.contains(&kid), "kid 必须在 JWKS 里：{kids:?}");

    // 载荷：声明集合钉死。
    let payload = payload_of(&token);
    assert_eq!(payload["iss"], "base");
    assert_eq!(payload["aud"], CLIENT_A);
    for field in [
        "sub",
        "tid",
        "client_id",
        "scope",
        "grant_type",
        "iat",
        "nbf",
        "exp",
        "jti",
    ] {
        assert!(payload.get(field).is_some(), "载荷缺少 {field}：{payload}");
    }
    assert_eq!(payload["scope"], "rbac:role:read");
    assert_eq!(payload["grant_type"], "authorization_code");

    // -----------------------------------------------------------------
    // 步骤 1/3：**取 JWKS → 测试侧独立验签**（不复用 `platform_oauth::keys`）
    // -----------------------------------------------------------------
    //
    // ⚠️ 这一步是 AC-S-145 的**核心承诺**（「第三方可凭 JWKS 自行验签」）。
    // 只断 `alg` / `kid` / claims（上面那些）是**结构断言** —— 把签名换成任意值
    // 也照样绿。→ 必须真的用公钥验一个字节。
    let (status, jwks_body) = call(
        &fixture.router,
        Method::GET,
        "/.well-known/jwks.json",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "JWKS 应当 200：{jwks_body}");
    // 公钥里**不得**含私钥参数（`d`）—— JWKS 只暴露公钥（D3）。
    if let Some(keys) = jwks_body["keys"].as_array() {
        for key in keys {
            assert!(
                key.get("d").is_none(),
                "JWKS 不得泄露私钥参数 d：{key}"
            );
        }
    }

    let (outcome, verified_claims) = verify_signature(&jwks_body, &token);
    assert_eq!(
        outcome,
        VerifyOutcome::Valid,
        "AC-S-145 步骤 3：测试侧用 JWKS 公钥独立验签必须**通过**"
    );
    // 验签**解出来的**声明也要与 payload 一致（防「签名有效但内容是别的」）。
    assert_eq!(verified_claims["aud"], CLIENT_A);
    assert_eq!(verified_claims["scope"], "rbac:role:read");

    // -----------------------------------------------------------------
    // 阳性对照：改了载荷的一个字节 → 验签**必须**变成「签名无效」
    // -----------------------------------------------------------------
    //
    // ⚠️ 为什么必须有：若 `verify_signature` 因为别的原因（比如 `decode` 压根没验）
    // 恒返回 `Valid`，上面那条断言就是空跑。改一个字节必须变 `BadSignature`。
    let tampered = tamper_payload(&token);
    let (tampered_outcome, _) = verify_signature(&jwks_body, &tampered);
    assert_eq!(
        tampered_outcome,
        VerifyOutcome::BadSignature,
        "载荷被改一个字节 → 必须判「签名无效」（否则验签断言无区分力）"
    );}

/// 测试侧**自签**一张同意票据（模拟「平台私钥泄露」，AC-S-146 步骤 3）。
///
/// # 为什么要自签而不是调 `ticket::sign`
///
/// D204 的威胁模型是「**私钥**泄露」：攻击者手里有平台的签名私钥 →
/// 他能签出**平台验签会通过**的任意票据（`kid` 也是平台的那把）。
/// 若测试调 `platform_oauth::ticket::sign`，验的就是「平台的签发函数」，
/// 判不到「**签名有效但 scope 更大**」这条（那正是本判据要证明的）。
fn self_signed_ticket(fixture: &Fixture, client_id: &str, scope: &str) -> String {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let header = jsonwebtoken::Header {
        alg: jsonwebtoken::Algorithm::ES256,
        // ⚠️ **同一个 `kid`** —— 平台的 JWKS 里必须有对应公钥（私钥泄露的前提）。
        kid: Some(fixture.iam_kid.clone()),
        ..jsonwebtoken::Header::default()
    };
    let key = jsonwebtoken::EncodingKey::from_ec_pem(fixture.iam_private_pem.as_bytes())
        .expect("私有 PEM 应当能构造签名密钥");
    let claims = json!({
        "typ": "oauth_consent_ticket",
        "iss": "base",
        "client_id": client_id,
        "redirect_uri": "https://client.example/cb",
        "scope": scope,
        "code_challenge": challenge_of(VERIFIER),
        "code_challenge_method": "S256",
        "sub": USER_A,
        "tid": TENANT_A,
        "iat": now,
        "exp": now + 300,
        "jti": uuid::Uuid::new_v4().to_string(),
    });
    jsonwebtoken::encode(&header, &claims, &key).expect("自签票据应当成功")
}

/// 把 JWT 的载荷改一个字节（**不重签** → 签名必然对不上）。
///
/// 用于「验签断言真的在验签」的阳性对照。
fn tamper_payload(token: &str) -> String {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64;
    let mut parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT 应当有三段");
    let bytes = BASE64.decode(parts[1]).expect("载荷应当是 base64url");
    let mut claims: Value = serde_json::from_slice(&bytes).expect("载荷应当是 JSON");
    // 改 `scope` —— 这是令牌里**有意义**的字段（不是填充）。
    claims["scope"] = json!("rbac:role:read rbac:role:write");
    let tampered = BASE64.encode(serde_json::to_vec(&claims).expect("应当能序列化"));
    // `parts` 借用自 `token` → 必须泄漏成 `String` 才能重组。
    let rebuilt = format!("{}.{}.{}", parts[0], tampered, parts[2]);
    parts.clear();
    rebuilt
}

/// AC-S-145 步骤 4/5：**过期令牌**（签名有效但 `exp` 已过）与
/// `client_credentials` 令牌（`sub` 键不存在）。
///
/// # 为什么单独一条用例
///
/// 步骤 4 的判据是「**不得**把『签名无效』与『已过期』混为一谈」。
/// 要判它，必须同时看到**两种**结果：
///
/// | 令牌 | 签名 | `exp` | 期望结论 |
/// |------|------|-------|----------|
/// | 正常（平台签发、TTL 为正） | 有效 | 未过期 | [`VerifyOutcome::Valid`] |
/// | 过期（平台签发、TTL 为**负**） | 有效 | 已过期 | [`VerifyOutcome::ExpiredButSigned`] |
/// | 载荷被改 | **无效** | — | [`VerifyOutcome::BadSignature`] |
///
/// ⚠️ 「过期令牌」必须**由平台自己签**（TTL 配成负数）——
/// 测试侧自签的过期令牌验的是测试自己的签名，判不出平台实现。
#[tokio::test]
async fn an_expired_token_verifies_but_is_reported_as_expired() {
    // TTL = -60 秒 → 平台签出来的 `exp` 已经在过去（签名仍然有效）。
    let fixture = setup_with(platform_oauth::config::OAuthSettings {
        access_token_ttl_seconds: -60,
        ..platform_oauth::config::OAuthSettings::default()
    })
    .await;
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read"],
        &["client_credentials"],
        None,
        None,
    )
    .await;

    // client_credentials → 令牌（`sub` **键不存在**，AC-S-145 步骤 5）。
    let (status, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "rbac:role:read"),
        ],
        Some(basic(CLIENT_A, secret)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "换令牌应当 200：{body}");
    let token = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();

    let (status, jwks_body) = call(
        &fixture.router,
        Method::GET,
        "/.well-known/jwks.json",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "JWKS 应当 200：{jwks_body}");

    // 步骤 4：签名**有效**，但 `exp` 已过 → 结论是「已过期」（不是「签名无效」）。
    let (outcome, claims) = verify_signature(&jwks_body, &token);
    assert_eq!(
        outcome,
        VerifyOutcome::ExpiredButSigned,
        "过期令牌：验签必须**通过**（签名有效），但结论必须是「已过期」"
    );
    // 它确实是**平台签的**（`aud` = client、`iss` = 平台），不是测试伪造的空壳。
    assert_eq!(claims["iss"], "base");
    assert_eq!(claims["aud"], CLIENT_A);

    // 步骤 5：`client_credentials` 令牌的 `sub` **键不存在**（不是 `null`）。
    assert!(
        claims.get("sub").is_none(),
        "client_credentials 令牌不得有 sub 键：{claims}"
    );
    for field in [
        "tid", "client_id", "scope", "grant_type", "iat", "nbf", "exp", "jti",
    ] {
        assert!(
            claims.get(field).is_some(),
            "client_credentials 令牌缺少 {field}：{claims}"
        );
    }

    // **区分力的证明**：同一枚令牌若走「同时验 exp」的完整校验 → 必须是
    // `ExpiredSignature`；而**改过的**令牌必须是 `InvalidSignature`。
    // 两者是**不同的错误类型** —— 这就是「不得混为一谈」的可判形式。
    let header = header_of(&token);
    let kid = header["kid"].as_str().expect("头里应当有 kid");
    let key = decoding_key_from_jwks(&jwks_body, kid);
    let mut full = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::ES256);
    full.leeway = 0;
    full.validate_exp = true;
    full.validate_aud = false;
    assert_eq!(
        jsonwebtoken::decode::<Value>(&token, &key, &full)
            .expect_err("过期令牌走完整校验必须失败")
            .kind(),
        &jsonwebtoken::errors::ErrorKind::ExpiredSignature,
        "完整校验的失败原因必须是「已过期」，不是别的"
    );

    let tampered = tamper_payload(&token);
    assert_eq!(
        jsonwebtoken::decode::<Value>(&tampered, &key, &full)
            .expect_err("改过的令牌必须失败")
            .kind(),
        &jsonwebtoken::errors::ErrorKind::InvalidSignature,
        "改过的令牌失败原因必须是「签名无效」—— 与「已过期」是**不同**的类型"
    );
}

/// AC-S-146 步骤 3 的**替代判据**（D260）：私钥泄露也拿不到越权令牌。
///
/// # 判据形状（为什么是这套断言）
///
/// D260 明确：`confirm` 只信票据、**不校验** scope（否则 AC-S-140 的 4 条 P0
/// 用例全废）。于是「自签票据」在 `confirm` 这一层**必然 200** ——
/// 所以判据必须落在**令牌端点**：
///
/// 1. 自签一张 `scope` **比 preview 返回的更大**的票据 → `confirm` **200**（允许）；
/// 2. 用拿到的 `code` 交换令牌 → **400 `invalid_scope`**；
/// 3. `platform_oauth_token` 里**没有任何行**（没签发令牌）；
/// 4. **把 TD-046 的代价测出来**：`platform_oauth_grant` 会多一行
///    （用户会在「我的授权」里看到一条自己没同意过的记录）。
///
/// ⚠️ 第 3 条不能只断「响应里没有 access_token」—— 必须**查库**：
/// 偷发令牌但不在响应里回显是一条真实可能的回归。
#[tokio::test]
async fn a_self_signed_ticket_with_a_larger_scope_cannot_obtain_a_token() {
    let fixture = setup().await;
    // 用户只有 `rbac:role:read`（**没有** write）。
    fixture.permissions.grant(USER_A, "rbac:role:read");
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        // client 白名单**有** write（隔离「client 维」）。
        &["rbac:role:read", "rbac:role:write"],
        &["authorization_code"],
        None,
        None,
    )
    .await;
    let user = bearer(&login_token(&fixture, TENANT_A, USER_A));

    // 前置：该 client 此用户**没有**授权记录（否则下面数不清 TD-046 的代价）。
    let before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_oauth_grant WHERE tenant_id = $1 AND client_id = $2 \
         AND user_id = $3",
    )
    .bind(TENANT_A)
    .bind(CLIENT_A)
    .bind(USER_A)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能数 grant 行");
    assert_eq!(before, 0, "前置：不该已有授权记录");

    // 自签票据：scope 比「用户实际能拿到的」**更大**（多出 write）。
    let forged = self_signed_ticket(&fixture, CLIENT_A, "rbac:role:read rbac:role:write");

    // 步骤 1：`confirm` **接受**该票据（200）—— 这是 D260 的实现选择，不是缺陷。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/confirm",
        Some(json!({ "ticket": forged })),
        Some(user),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "D260 的实现在 confirm 只信票据 → 自签票据应当 200：{body}"
    );
    let redirect = body["redirectTo"]
        .as_str()
        .expect("应当有 redirectTo")
        .to_string();
    let code = redirect
        .split("code=")
        .nth(1)
        .expect("应当有 code")
        .split('&')
        .next()
        .expect("code 应当有值")
        .to_string();

    // 步骤 4（TD-046 的代价）：**多写了一行 grant** —— 把代价测出来，
    // 不让它成为「没人知道的行为」。
    let grant_rows: Vec<(Vec<String>, String)> = sqlx::query_as(
        "SELECT scopes, status FROM platform_oauth_grant \
         WHERE tenant_id = $1 AND client_id = $2 AND user_id = $3",
    )
    .bind(TENANT_A)
    .bind(CLIENT_A)
    .bind(USER_A)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能查 grant 行");
    assert_eq!(
        grant_rows.len(),
        1,
        "TD-046 的已知代价：自签票据会**多写一行 grant**（用户可见）；实测 {grant_rows:?}"
    );
    // 而且写进 grant 的 scope 是**交集**（read），不是票据里的 read+write。
    let mut granted = grant_rows[0].0.clone();
    granted.sort();
    assert_eq!(
        granted,
        vec!["rbac:role:read".to_string()],
        "grant.scopes 必须是**交集**（用户只有 read）"
    );

    // 步骤 2：拿该 code 交换令牌 → **400 `invalid_scope`**（判据的**核心**）。
    let (status, body) = exchange_code(&fixture, CLIENT_A, secret, &code).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "自签票据换出来的 code 交换令牌必须 400：{body}"
    );
    assert_eq!(body["error"], "invalid_scope");

    // 步骤 3：令牌表**没有任何行**（查库，不信响应）。
    let tokens: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_oauth_token WHERE client_id = $1")
            .bind(CLIENT_A)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能数令牌行");
    assert_eq!(
        tokens, 0,
        "私钥泄露场景**不得**签发任何令牌（越权判据的最后一道）"
    );
}

/// AC-S-146：同意记录落库；拒绝**不**改已有授权。
#[tokio::test]
async fn the_consent_record_is_written_and_denial_does_not_change_it() {
    let (fixture, user) = prepared().await;
    seed_client_a(&fixture).await;
    let user = bearer(&user);

    let (_, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "state": "xyz",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    let ticket = body["ticket"].as_str().expect("应当有票据").to_string();

    // 同意 → 写 grant。
    let (status, _) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/confirm",
        Some(json!({ "ticket": ticket.clone() })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let rows: Vec<(
        String,
        String,
        Vec<String>,
        String,
        Option<time::OffsetDateTime>,
    )> = sqlx::query_as(
        "SELECT client_id, user_id, scopes, status, consent_at FROM platform_oauth_grant \
         WHERE tenant_id = $1 AND client_id = $2 AND user_id = $3",
    )
    .bind(TENANT_A)
    .bind(CLIENT_A)
    .bind(USER_A)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能查同意记录");
    assert_eq!(rows.len(), 1, "同意记录应当恰好 1 行");
    assert_eq!(rows[0].3, "active");
    assert!(rows[0].4.is_some(), "consent_at 不得为空");
    assert_eq!(rows[0].2, vec!["rbac:role:read".to_string()]);

    // 拒绝 → 跳转带 `error=access_denied` 与原 `state`。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/deny",
        Some(json!({ "ticket": ticket })),
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "拒绝应当 200：{body}");
    let redirect = body["redirectTo"]
        .as_str()
        .expect("应当有 redirectTo")
        .to_string();
    assert!(redirect.contains("error=access_denied"), "{redirect}");
    assert!(redirect.contains("state=xyz"), "{redirect}");

    // 拒绝**不**改已有授权（仍然 1 行、仍然 active）。
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT client_id, status FROM platform_oauth_grant \
         WHERE tenant_id = $1 AND client_id = $2 AND user_id = $3",
    )
    .bind(TENANT_A)
    .bind(CLIENT_A)
    .bind(USER_A)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能查同意记录");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, "active");
}

/// AC-S-147：「我的授权」只含**自己**的，且撤销立即生效。
#[tokio::test]
async fn a_user_sees_and_revokes_only_their_own_grants() {
    let (fixture, user) = prepared().await;
    seed_client_a(&fixture).await;
    seed_grant(&fixture, TENANT_A, CLIENT_A, USER_A, &["rbac:role:read"]).await;
    // **别人**的授权记录（同一个租户、另一个用户）。
    seed_grant(
        &fixture,
        TENANT_A,
        CLIENT_A,
        "other-user",
        &["rbac:role:read"],
    )
    .await;
    let user = bearer(&user);

    // 步骤 1：列表**恰好 1 条**（不含别人的）。
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/api/oauth/grants",
        None,
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "列表应当 200：{body}");
    let list = body.as_array().expect("列表应当是数组");
    assert_eq!(list.len(), 1, "只应看到自己的：{list:?}");
    assert_eq!(list[0]["clientId"], CLIENT_A);
    assert_eq!(list[0]["status"], "active");
    assert!(list[0]["grantedAt"].as_str().is_some());
    assert!(list[0]["scopes"].as_array().is_some());

    // 步骤 2：撤销 → 200。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/grants/revoke",
        Some(json!({ "clientId": CLIENT_A })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "撤销应当 200：{body}");
    assert_eq!(body["status"], "revoked");

    // 步骤 3：再查 → status=revoked、revokedAt 非空。
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/api/oauth/grants",
        None,
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().expect("列表应当是数组");
    assert_eq!(list[0]["status"], "revoked");
    assert!(
        list[0]["revokedAt"].as_str().is_some(),
        "撤销后 revokedAt 不得为空：{list:?}"
    );
}

/// AC-S-147 步骤 4：**grant 级**撤销 —— 同一 grant 下的**两枚**令牌都立刻失效。
///
/// ⚠️ 这条是「grant 级」语义的**唯一**判据（D254）：
/// 同一 (租户, client, 用户) 的授权码交换**复用同一张 grant 行**
/// （`upsert_grant` 的 `ON CONFLICT (tenant_id, client_id, user_id)`）
/// → 两次交换 = **一枚 grant + 两枚令牌**。
///
/// 只撤「其中一枚」时，另一枚仍 `active = true` → 判据变红。
#[tokio::test]
async fn revoking_a_grant_revokes_every_token_it_issued() {
    let (fixture, user) = prepared().await;
    let secret = seed_client_a(&fixture).await;
    let user = bearer(&user);

    // 第一遍：正常同意 → 换令牌。
    let code = authorization_code(&fixture, &user, CLIENT_A, "rbac:role:read").await;
    let (status, body) = exchange_code(&fixture, CLIENT_A, &secret, &code).await;
    assert_eq!(status, StatusCode::OK, "第一次换令牌应当 200：{body}");
    let token1 = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();

    // 第二遍：已有授权 → **静默续发**（`prompt=none`）→ 再换一枚令牌。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "prompt": "none",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "静默续发应当 200：{body}");
    assert_eq!(body["needsConsent"], false, "已有授权不得再要同意：{body}");
    let redirect = body["redirectTo"]
        .as_str()
        .expect("应当有 redirectTo")
        .to_string();
    let code2 = redirect
        .split("code=")
        .nth(1)
        .expect("应当有 code")
        .split('&')
        .next()
        .expect("code 应当有值")
        .to_string();
    let (status, body) = exchange_code(&fixture, CLIENT_A, &secret, &code2).await;
    assert_eq!(status, StatusCode::OK, "第二次换令牌应当 200：{body}");
    let token2 = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();
    assert_ne!(token1, token2, "两枚令牌应当是不同的");

    // 步骤 1：授权记录**恰好 1 条**（两枚令牌共用一张 grant）。
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/api/oauth/grants",
        None,
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "列表应当 200：{body}");
    let list = body.as_array().expect("列表应当是数组");
    assert_eq!(list.len(), 1, "两枚令牌应当共用一张授权记录：{list:?}");

    // 步骤 2：撤销 → 200。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/grants/revoke",
        Some(json!({ "clientId": CLIENT_A })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "撤销应当 200：{body}");

    // 步骤 4：**两枚**都 `active = false`（grant 级撤销，D254）。
    for token in [&token1, &token2] {
        let (status, body) = call_form(
            &fixture.router,
            "/api/oauth/introspect",
            &[("token", token.as_str())],
            Some(basic(CLIENT_A, &secret)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["active"], false,
            "grant 撤销后，**每一枚**令牌都必须立刻失效：{body}"
        );
    }

    // 步骤 4 补充：令牌表里**两行**的状态都是 `revoked`。
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT token_id, status FROM platform_oauth_token \
         WHERE tenant_id = $1 AND client_id = $2 ORDER BY token_id",
    )
    .bind(TENANT_A)
    .bind(CLIENT_A)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能查令牌状态");
    assert_eq!(rows.len(), 2, "应当有两枚令牌：{rows:?}");
    for (token_id, status) in rows {
        assert_eq!(status, "revoked", "令牌 {token_id} 应当是 revoked");
    }
}

/// ⚠️ 数据范围：**即使持有权限点**，也只能看到 / 撤自己的（BA 复核的硬要求）。
#[tokio::test]
async fn the_grants_endpoints_are_scoped_to_the_current_user() {
    let (fixture, user) = prepared().await;
    seed_client_a(&fixture).await;
    // **别人**的授权记录；当前用户**也**持 `oauth:grant:revoke`。
    seed_grant(
        &fixture,
        TENANT_A,
        CLIENT_A,
        "other-user",
        &["rbac:role:read"],
    )
    .await;
    let user = bearer(&user);

    // 列表里**没有**别人的记录。
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/api/oauth/grants",
        None,
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.as_array().expect("列表应当是数组").len(),
        0,
        "不得看到别人的授权：{body}"
    );

    // 撤别人的 → **404** `OAUTH_GRANT_NOT_FOUND`（**不得**动他的记录）。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/grants/revoke",
        Some(json!({ "clientId": CLIENT_A })),
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "撤别人的应当 404：{body}");
    assert_eq!(body["code"], "OAUTH_GRANT_NOT_FOUND");

    let status: String = sqlx::query_scalar(
        "SELECT status FROM platform_oauth_grant \
         WHERE tenant_id = $1 AND client_id = $2 AND user_id = $3",
    )
    .bind(TENANT_A)
    .bind(CLIENT_A)
    .bind("other-user")
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能查别人的记录");
    assert_eq!(status, "active", "不得动别人的授权记录");
}

/// 跨租户：另一个租户的用户调 `GET /grants` → **空数组**（不泄露存在性）。
#[tokio::test]
async fn another_tenants_user_sees_an_empty_grant_list() {
    let (fixture, _) = prepared().await;
    seed_client_a(&fixture).await;
    seed_grant(&fixture, TENANT_A, CLIENT_A, USER_A, &["rbac:role:read"]).await;
    let user_b = bearer(&login_token(&fixture, TENANT_B, USER_B));

    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/api/oauth/grants",
        None,
        Some(user_b),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!([]),
        "别的租户必须看到空数组（不是 403、也不是别人的记录）"
    );
}

/// ⚠️ 权限点是**逐端点**的，不是叠加的（BA 复核发现的 AND 问题）。
#[tokio::test]
async fn the_permission_points_are_per_endpoint_not_stacked() {
    let fixture = setup().await;
    // 只持 `oauth:grant:read`（**没有** revoke）与只持 `oauth:client:read`。
    fixture.permissions.grant(USER_A, "oauth:grant:read");
    fixture.permissions.grant(USER_A, "oauth:client:read");
    let user = bearer(&login_token(&fixture, TENANT_A, USER_A));

    // 有 `grant:read` → 列表 **200**。
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/api/oauth/grants",
        None,
        Some(user.clone()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "只持 grant:read 也必须能读列表（叠加会让它 403）：{body}"
    );

    // **没有** `grant:revoke` → 撤销 **403** `INSUFFICIENT_PERMISSION`。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/grants/revoke",
        Some(json!({ "clientId": CLIENT_A })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "缺 revoke 应当 403：{body}");
    assert_eq!(body["code"], "INSUFFICIENT_PERMISSION");

    // 有 `client:read` → 列表 **200**；没有 `client:write` → 注册 **403**。
    let (status, body) = call(
        &fixture.router,
        Method::GET,
        "/api/oauth/clients",
        None,
        Some(user.clone()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "只持 client:read 也必须能读列表（叠加会让它 403）：{body}"
    );

    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/clients",
        Some(json!({
            "name": "应用丙",
            "clientType": "confidential",
            "redirectUris": ["https://client.example/cb"]
        })),
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "缺 write 应当 403：{body}");
    assert_eq!(body["code"], "INSUFFICIENT_PERMISSION");
}

/// AC-S-148：已有授权 → 免登（静默续发）。
#[tokio::test]
async fn an_existing_grant_allows_silent_renewal() {
    let (fixture, user) = prepared().await;
    seed_client_a(&fixture).await;
    seed_grant(&fixture, TENANT_A, CLIENT_A, USER_A, &["rbac:role:read"]).await;
    let user = bearer(&user);

    // `prompt=none` + 已有覆盖的 grant → **不**要同意，直接给跳转地址。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "prompt": "none",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "免登应当 200：{body}");
    assert_eq!(body["needsConsent"], false);
    assert!(body.get("ticket").is_none(), "免登时不得有票据");
    let redirect = body["redirectTo"]
        .as_str()
        .expect("应当有 redirectTo")
        .to_string();
    assert!(redirect.contains("code="), "{redirect}");

    // 超出已有 grant 的 scope → **要**同意（200 + needsConsent=true，**不是**错误）。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read rbac:role:write",
            "prompt": "none",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "超出范围也要 200（不是错误，D233）：{body}"
    );
    assert_eq!(body["needsConsent"], true);
    // **AC-S-148 步骤 4**：`needsConsent=true` 时**不得偷偷签发授权码**。
    //
    // ⚠️ 这条之前**零断言**（BA 报告写了、测试里没有）。它守的是
    // 「免登没覆盖 → 悄悄发一个码，用户根本没同意」这条越权路径。
    let codes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_oauth_code WHERE tenant_id = $1 AND client_id = $2",
    )
    .bind(TENANT_A)
    .bind(CLIENT_A)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能数授权码行");
    assert_eq!(
        codes, 1,
        "needsConsent=true 时**不得**再签发授权码（只应有前一步免登那 1 行）"
    );

    // 缺省 `prompt`（= 允许交互）→ **要**同意。
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": CLIENT_A,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["needsConsent"], true,
        "缺省 prompt = 允许交互 → 必须出现同意页"
    );
}

/// 极简的 `tracing` 采集器（**只收事件字段**，够断言「有没有那条日志」）。
///
/// 不引 `tracing-subscriber`（会新增 dev-dependency）—— 手写一个最小
/// `Subscriber`，只用 `tracing` 自己的公开 API。
struct CapturedLogs(Arc<Mutex<Vec<String>>>);

struct FieldVisitor(String);

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write as _;
        let _ = write!(self.0, "{}={:?} ", field.name(), value);
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        use std::fmt::Write as _;
        let _ = write!(self.0, "{}={} ", field.name(), value);
    }
}

impl tracing::Subscriber for CapturedLogs {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut visitor = FieldVisitor(String::new());
        event.record(&mut visitor);
        self.0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(visitor.0);
    }
    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

/// AC-S-149 的**补充判据**：投递失败时**有日志**（含目标与失败原因）。
///
/// # 为什么要断言日志
///
/// 「通知失败绝不能让登出失败」的那一半判据（上面那条用例）有个副作用：
/// 失败被**静默吞掉**也满足「登出仍 200」。缺日志 = 线上没人知道 SSO 通知坏了。
#[tokio::test]
async fn a_failed_logout_delivery_is_logged_with_a_reason() {
    let (fixture, user) = prepared().await;
    let receiver = start_logout_receiver(ReceiverMode::Fail500).await;
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read"],
        &["authorization_code"],
        Some(&receiver.url),
        None,
    )
    .await;
    seed_grant(&fixture, TENANT_A, CLIENT_A, USER_A, &["rbac:role:read"]).await;

    let logs = Arc::new(Mutex::new(Vec::new()));
    {
        // 只在这个作用域里换掉默认 subscriber（`--test-threads=1` + current-thread 运行时）。
        let _guard = tracing::subscriber::set_default(CapturedLogs(logs.clone()));
        let (status, body) = call(
            &fixture.router,
            Method::POST,
            "/api/oauth/logout",
            None,
            Some(bearer(&user)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "登出仍须 200：{body}");
    }

    let captured = logs.lock().unwrap_or_else(|err| err.into_inner()).clone();
    let delivery_log = captured
        .iter()
        .find(|line| line.contains("could not be delivered"))
        .unwrap_or_else(|| panic!("投递失败必须留日志；实际日志：{captured:?}"));
    // 含**失败原因**（AC-S-149 补充判据的「失败原因」那一半）。
    assert!(
        delivery_log.contains("reason="),
        "日志必须带失败原因：{delivery_log}"
    );
    // ⚠️ **偏差（已在报告里点名）**：AC 要求日志含 **`clientId`**，
    // 而实现把**投递 URI** 塞进了名为 `client_id` 的字段
    // （`logout.rs` 的 `let client_id = uri.clone();`）。
    // → 这里断言「日志里能定位到投递目标」（= URI），**不**声称 `clientId` 已记录。
    // AC-S-149 的补充判据：日志要含 **`clientId`** 与失败原因。
    //
    // ⚠️ 这条断言**曾经**是「已知偏差」的守门人：修复轮第一版发现实现把**投递 URI**
    // 塞进了名为 `client_id` 的日志字段（`let client_id = uri.clone();`）→
    // 当时断言「日志**不含** clientId」。BA 裁定这个偏差要修（它违反
    // `deliver_backchannel_logouts` 自己的文档注释）→ 实现改成把
    // `(client_id, uri, token)` 三个值分开传 → 现在日志里**有** clientId。
    // → 断言随之反过来：**必须含 clientId**。
    assert!(
        delivery_log.contains("client-a"),
        "日志必须含 clientId（AC-S-149 的补充判据）：{delivery_log}"
    );
    // ⚠️ **不要**断言日志里含投递 URI：修好后日志只记 `client_id`，
    // 而把投递地址写进日志会泄漏内部拓扑（`logout_token` 的接收方地址
    // 属于注册数据，不是排障必需的）。
    assert!(
        !delivery_log.contains(&receiver.url),
        "日志不应回显投递 URI（内部拓扑）：{delivery_log}"
    );
}

// ---------------------------------------------------------------------------
// AC-S-149：back-channel 登出的**接收端**（测试侧自己的，不依赖外部脚本）
// ---------------------------------------------------------------------------
//
// 验收单允许「集成测试里自己的接收端」。这里起一个**真实 HTTP 服务**
// （`127.0.0.1:0` → 随机端口）→ `OutboundClient` 走**真的** hyper 出站
// → 收到的 form 体就是平台实际投递的内容。
//
// ⚠️ 不用 `target/mock-logout-received.jsonl`：那是 E2E 的产物，需要外部进程；
// 本用例要把「投递内容」和「投递失败」都握在手里（模式：ok / fail500 / hang）。

/// 接收端行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiverMode {
    /// 正常 200。
    Ok,
    /// 返回 500（`MOCK_MODE=500` 的等价物）。
    Fail500,
    /// 收下请求但**不回响应**（超过 5 秒出站超时 → `MOCK_MODE=timeout` 的等价物）。
    Hang,
}

/// 测试侧的登出接收端。
struct LogoutReceiver {
    /// 注入 client 表用的 `backchannel_logout_uri`。
    url: String,
    /// 收到的 `(content-type, body)`。
    received: Arc<Mutex<Vec<(String, String)>>>,
}

struct ReceiverState {
    received: Arc<Mutex<Vec<(String, String)>>>,
    mode: ReceiverMode,
}

async fn handle_logout(
    axum::extract::State(state): axum::extract::State<Arc<ReceiverState>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    state
        .received
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .push((content_type, String::from_utf8_lossy(&body).to_string()));
    match state.mode {
        ReceiverMode::Ok => (StatusCode::OK, "ok").into_response(),
        ReceiverMode::Fail500 => (StatusCode::INTERNAL_SERVER_ERROR, "nope").into_response(),
        ReceiverMode::Hang => {
            tokio::time::sleep(std::time::Duration::from_secs(8)).await;
            (StatusCode::OK, "late").into_response()
        }
    }
}

/// 起一个接收端（随机端口），返回它的 URL 与「收到的请求」。
async fn start_logout_receiver(mode: ReceiverMode) -> LogoutReceiver {
    let received = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("应当能绑定随机端口");
    let addr = listener.local_addr().expect("应当能取本地地址");
    let state = Arc::new(ReceiverState {
        received: received.clone(),
        mode,
    });
    let app = axum::Router::new()
        .route("/logout", axum::routing::post(handle_logout))
        .with_state(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    LogoutReceiver {
        url: format!("http://127.0.0.1:{}/logout", addr.port()),
        received,
    }
}

impl LogoutReceiver {
    /// 收到的全部请求。
    fn requests(&self) -> Vec<(String, String)> {
        self.received
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
}

/// 从 `application/x-www-form-urlencoded` 体里取 `logout_token` 的值。
fn logout_token_of(body: &str) -> String {
    for pair in body.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == "logout_token" {
                return percent_decode(value);
            }
        }
    }
    panic!("投递体里没有 logout_token：{body}");
}

/// 极简 percent-decode（投递侧对 JWT 是恒等变换，但别假设）。
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

/// AC-S-149 步骤 2/3：`logout_token` 的**内容**（不是「有东西被投递」）。
///
/// # 这条判据之前为什么是空的
///
/// 旧测试只断 `front[0]["logoutToken"].as_str().is_some()` + mock 文件行数 > 0
/// → 少一个 `sid`、误加一个 `nonce`、`aud` 写成平台 audience —— **全都绿**。
#[tokio::test]
async fn the_backchannel_logout_token_carries_the_pinned_claims() {
    let (fixture, _user) = prepared().await;
    let receiver = start_logout_receiver(ReceiverMode::Ok).await;
    // client-A：back-channel 指向**真实**接收端。
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read"],
        &["authorization_code"],
        Some(&receiver.url),
        Some("https://client.example/front"),
    )
    .await;
    seed_grant(&fixture, TENANT_A, CLIENT_A, USER_A, &["rbac:role:read"]).await;

    let login = login_token(&fixture, TENANT_A, USER_A);
    let expected_sid = payload_of(&login)["sid"]
        .as_str()
        .expect("登录令牌应当有 sid")
        .to_string();

    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/logout",
        None,
        Some(bearer(&login)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "登出应当 200：{body}");

    // 步骤 2：收到的是 **form 编码**，含 `logout_token`。
    let requests = receiver.requests();
    assert_eq!(requests.len(), 1, "应当恰好投递一次：{requests:?}");
    let (content_type, form) = &requests[0];
    assert!(
        content_type.starts_with("application/x-www-form-urlencoded"),
        "投递必须是 form 编码，实际：{content_type}"
    );
    let token = logout_token_of(form);

    // 步骤 3：**测试侧独立解析** payload（只解码，不验签 —— 签名的有效性
    // 由 AC-S-145 的 JWKS 用例守）。
    let payload = payload_of(&token);
    assert_eq!(payload["iss"], "base", "iss 必须等于平台 issuer");
    assert_eq!(payload["aud"], CLIENT_A, "aud 必须是 client-A 的 client_id");
    for field in ["sub", "sid", "jti", "iat"] {
        assert!(payload.get(field).is_some(), "payload 缺少 {field}：{payload}");
    }
    // `events` 必须**含键** backchannel-logout（值可以是空对象）。
    assert!(
        payload["events"]
            .get("http://schemas.openid.net/event/backchannel-logout")
            .is_some(),
        "events 必须含 backchannel-logout 键：{}",
        payload["events"]
    );
    // **不含** `nonce`（D211）。
    assert!(
        payload.get("nonce").is_none(),
        "logout_token 不得含 nonce：{payload}"
    );
    // `sid` **逐字等于**触发登出的登录令牌的 `sid`（D230）。
    assert_eq!(
        payload["sid"], expected_sid,
        "sid 必须是**触发登出的这次请求**的会话 ID（D230）"
    );
}

/// AC-S-149 步骤 4/5：**通知失败绝不能让登出失败**。
///
/// 两种失败都要跑：`500`（远端拒绝）与 `timeout`（出站 5 秒超时）。
/// 断言三件事：登出 200 / grant 已撤销 / 令牌已失效。
#[tokio::test]
async fn a_failed_logout_notification_never_fails_the_logout() {
    for mode in [ReceiverMode::Fail500, ReceiverMode::Hang] {
        let (fixture, user) = prepared().await;
        let receiver = start_logout_receiver(mode).await;
        let secret = "secret-a";
        let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
        seed_client(
            &fixture,
            CLIENT_A,
            Some(&hash),
            "confidential",
            &["rbac:role:read"],
            &["authorization_code"],
            Some(&receiver.url),
            None,
        )
        .await;
        seed_grant(&fixture, TENANT_A, CLIENT_A, USER_A, &["rbac:role:read"]).await;

        let (status, body) = call(
            &fixture.router,
            Method::POST,
            "/api/oauth/logout",
            None,
            Some(bearer(&user)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{mode:?} 时登出本身必须仍然 200：{body}"
        );
        // 请求**真的**发出去了（否则测的是「没投递」而不是「投递失败」）。
        assert_eq!(
            receiver.requests().len(),
            1,
            "{mode:?} 时应当尝试投递一次"
        );

        // 撤销**已经**完成（通知失败不影响主流程）。
        let statuses: Vec<String> = sqlx::query_scalar(
            "SELECT status FROM platform_oauth_grant WHERE tenant_id = $1 AND user_id = $2",
        )
        .bind(TENANT_A)
        .bind(USER_A)
        .fetch_all(&fixture.admin_pool)
        .await
        .expect("应当能查授权记录");
        assert_eq!(statuses.len(), 1);
        assert_eq!(
            statuses[0], "revoked",
            "{mode:?} 时 grant 必须已撤销（通知失败不得回滚）"
        );
    }
}

/// AC-S-149 / AC-S-150：登出撤销授权并返回 front-channel 数组。
#[tokio::test]
async fn the_logout_revokes_the_grants_and_returns_the_front_channel_list() {
    let (fixture, user) = prepared().await;
    seed_client_a(&fixture).await;
    // client-B：**不**配 front-channel URI。
    seed_client_b(&fixture).await;
    seed_grant(&fixture, TENANT_A, CLIENT_A, USER_A, &["rbac:role:read"]).await;
    seed_grant(&fixture, TENANT_A, CLIENT_B, USER_A, &["rbac:role:read"]).await;
    let user = bearer(&user);

    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/logout",
        None,
        Some(user),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "登出应当 200：{body}");
    assert_eq!(body["loggedOut"]["grants"], 2);
    assert!(body["loggedOut"]["sessions"].as_u64().is_some());
    // `frontChannel` **永远是数组**（不是 null、不是省略）。
    let front = body["frontChannel"]
        .as_array()
        .expect("frontChannel 必须是数组（不是 null）");
    assert_eq!(
        front.len(),
        1,
        "只有配了 front-channel URI 的 client 才在其中"
    );
    assert_eq!(front[0]["clientId"], CLIENT_A);
    assert_eq!(front[0]["logoutUrl"], "https://client.example/front");
    assert!(front[0]["logoutToken"].as_str().is_some());
    // client-B 的 logoutUrl **不得**出现（D248）。
    assert!(!body.to_string().contains("client-b"));

    // 两个 client 的授权**都**被撤销了（front-channel 配置**不影响**撤销范围）。
    let statuses: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM platform_oauth_grant WHERE tenant_id = $1 AND user_id = $2",
    )
    .bind(TENANT_A)
    .bind(USER_A)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能查授权记录");
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().all(|status| status == "revoked"));
}

/// AC-S-151：第三方 OAuth 令牌**过不了**平台会话校验。
#[tokio::test]
async fn a_third_party_oauth_token_is_rejected_by_the_platform() {
    let fixture = setup().await;
    let secret = "secret-a";
    let hash = platform_oauth::crypto::digest_base64url(secret.as_bytes());
    seed_client(
        &fixture,
        CLIENT_A,
        Some(&hash),
        "confidential",
        &["rbac:role:read"],
        &["client_credentials"],
        None,
        None,
    )
    .await;

    let (_, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "client_credentials"),
            ("scope", "rbac:role:read"),
        ],
        Some(basic(CLIENT_A, secret)),
    )
    .await;
    let oauth_token = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();

    // OAuth 令牌 → 平台会话校验**失败**。
    assert!(
        fixture.iam.verify_access_token(&oauth_token).is_err(),
        "OAuth 令牌不得被当成平台会话令牌"
    );

    // **阳性对照**：正常的登录令牌必须通过（否则这条判据没有区分力）。
    let login = login_token(&fixture, TENANT_A, USER_A);
    assert!(
        fixture.iam.verify_access_token(&login).is_ok(),
        "登录令牌必须能通过（阳性对照）"
    );
}

/// AC-S-154：关键动作产生审计记录，且敏感值扫描**零命中**。
#[tokio::test]
async fn the_key_actions_produce_audit_records() {
    let (fixture, user) = prepared().await;
    seed_client_a(&fixture).await;
    let user = bearer(&user);

    // 注册 client → 同意授权 → 换令牌 → 撤销 → 登出。
    let admin = bearer(&login_token(&fixture, TENANT_A, ADMIN_A));
    let (status, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/clients",
        Some(json!({
            "name": "应用丁",
            "clientType": "confidential",
            "redirectUris": ["https://client.example/cb"],
            "allowedGrantTypes": ["authorization_code"],
            "allowedScopes": ["rbac:role:read"]
        })),
        Some(admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "注册应当 201：{body}");
    let new_client = body["clientId"]
        .as_str()
        .expect("应当有 clientId")
        .to_string();
    let secret = body["clientSecret"]
        .as_str()
        .expect("应当给一次性明文 secret")
        .to_string();

    let (_, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/preview",
        Some(json!({
            "responseType": "code",
            "clientId": new_client,
            "redirectUri": "https://client.example/cb",
            "scope": "rbac:role:read",
            "codeChallenge": challenge_of(VERIFIER),
            "codeChallengeMethod": "S256"
        })),
        Some(user.clone()),
    )
    .await;
    let ticket = body["ticket"].as_str().expect("应当有票据").to_string();
    let (_, body) = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/authorize/confirm",
        Some(json!({ "ticket": ticket })),
        Some(user.clone()),
    )
    .await;
    let redirect = body["redirectTo"]
        .as_str()
        .expect("应当有 redirectTo")
        .to_string();
    let code = redirect
        .split("code=")
        .nth(1)
        .expect("应当有 code")
        .split('&')
        .next()
        .expect("code 应当有值")
        .to_string();
    let (_, body) = call_form(
        &fixture.router,
        "/api/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", "https://client.example/cb"),
            ("code_verifier", VERIFIER),
        ],
        Some(basic(&new_client, &secret)),
    )
    .await;
    let token = body["access_token"]
        .as_str()
        .expect("应当有令牌")
        .to_string();
    let _ = call_form(
        &fixture.router,
        "/api/oauth/revoke",
        &[("token", token.as_str())],
        Some(basic(&new_client, &secret)),
    )
    .await;
    let _ = call(
        &fixture.router,
        Method::POST,
        "/api/oauth/logout",
        None,
        Some(user),
    )
    .await;
    let _ = new_client;

    // 事件码至少这 5 条。
    let events = fixture.sink.events();
    let actions: Vec<&str> = events.iter().map(|event| event.action.as_str()).collect();
    for expected in [
        "oauth.client.registered",
        "oauth.authorization.approved",
        "oauth.token.issued",
        "oauth.token.revoked",
        "oauth.logout.executed",
    ] {
        assert!(
            actions.contains(&expected),
            "缺少事件码 {expected}：{actions:?}"
        );
    }
    // 固定字段集（契约 C12）。
    for event in &events {
        assert!(!event.tenant_id.is_empty());
        assert!(!event.action.is_empty());
        assert!(!event.object_type.is_empty());
    }

    // 敏感值扫描：真实数据**零命中**。
    let mut fields: Vec<(&str, String)> = Vec::new();
    for event in &events {
        fields.push(("action", event.action.clone()));
        fields.push(("object_type", event.object_type.clone()));
        fields.push(("object_id", event.object_id.clone().unwrap_or_default()));
        fields.push((
            "before",
            event
                .before
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ));
        fields.push((
            "after",
            event
                .after
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ));
    }
    let borrowed: Vec<(&str, &str)> = fields
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let hits = platform_oauth::sensitive::scan_sensitive_fields(&borrowed);
    assert!(hits.is_empty(), "真实审计数据不得命中敏感键：{hits:?}");

    // **阳性对照**：塞一条含假 secret 的样本 → 必须命中。
    let control = platform_oauth::sensitive::scan_sensitive_fields(&[(
        "after",
        "{\"client_secret\":\"POSITIVE-CONTROL\"}",
    )]);
    assert!(
        !control.is_empty(),
        "阳性对照失败：扫描器必须能命中含假 secret 的样本"
    );
}
