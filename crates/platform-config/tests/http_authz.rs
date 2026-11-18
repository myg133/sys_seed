//! REQ-010 的 HTTP 鉴权矩阵（真实库 + 真实服务 + **替身令牌 / 判定器**）。
//!
//! 令牌校验与权限判定用替身，其余链路（注册表、解析、三层读写、白名单、错误码）
//! 全部与线上一致 —— 这样可以在不铸造 200 个 JWT 的前提下逐档验鉴权。

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use platform_config::ConfigError;
use platform_config::error::{
    CODE_FLAG_NAME_INVALID, CODE_FLAG_USE_FLAG_ENDPOINT, CODE_KEY_INVALID, CODE_KEY_NOT_REGISTERED,
    CODE_SCOPE_NOT_ALLOWED, CODE_TYPE_MISMATCH,
};
use platform_config::http::{ConfigState, TokenVerifier, VerifiedToken};
use platform_core::RequestContext;
use platform_core::extension::expr::CompiledCondition;
use platform_core::extension::permission::{
    DataScope, PermissionChecker, PermissionRefresher, ScopeFilter,
};
use serde_json::{Value, json};
use tower::ServiceExt;

use common::*;

const TOKEN_M: &str = "token-m";
const TOKEN_A: &str = "token-a";
const TOKEN_P: &str = "token-p";
const TOKEN_O: &str = "token-o";
const TOKEN_R: &str = "token-r";
const TOKEN_T2: &str = "token-t2";

const USER_O: &str = "u-2004";
const USER_R: &str = "u-2006";
const USER_T2: &str = "u-2005";

const PERM_ENTRY: &str = "config:entry:write";
const PERM_FLAG: &str = "config:flag:write";
const PERM_PLATFORM_WRITE: &str = "config:platform:write";
const PERM_PLATFORM_READ: &str = "config:platform:read";

/// 令牌 → 身份。
struct Tokens;

impl TokenVerifier for Tokens {
    fn verify(&self, token: &str) -> Result<VerifiedToken, ConfigError> {
        let (user_id, tenant_id) = match token {
            TOKEN_M => (USER_M, TENANT_A),
            TOKEN_A => (USER_A, TENANT_A),
            TOKEN_P => (USER_P, TENANT_A),
            TOKEN_O => (USER_O, TENANT_A),
            TOKEN_R => (USER_R, TENANT_A),
            TOKEN_T2 => (USER_T2, TENANT_B),
            _ => return Err(ConfigError::Unauthenticated),
        };
        Ok(VerifiedToken {
            user_id: user_id.to_string(),
            tenant_id: tenant_id.to_string(),
        })
    }
}

/// 用户 → 权限点集合。
struct Checker;

impl Checker {
    fn permissions(user: &str) -> BTreeSet<&'static str> {
        match user {
            USER_A => [PERM_ENTRY, PERM_FLAG].into_iter().collect(),
            USER_P => [
                PERM_ENTRY,
                PERM_FLAG,
                PERM_PLATFORM_WRITE,
                PERM_PLATFORM_READ,
            ]
            .into_iter()
            .collect(),
            USER_O => [PERM_PLATFORM_WRITE].into_iter().collect(),
            USER_R => [PERM_PLATFORM_READ].into_iter().collect(),
            // 普通用户 / T2 的调用者：没有任何 config 权限点。
            _ => BTreeSet::new(),
        }
    }
}

impl PermissionChecker for Checker {
    fn has(&self, ctx: &RequestContext, permission: &str) -> bool {
        ctx.user_id()
            .is_some_and(|user| Self::permissions(user).contains(permission))
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

/// 判定缓存刷新：替身（真实实现由 `RbacChecker` 提供）。
struct NoRefresh;

impl PermissionRefresher for NoRefresh {
    fn ensure_fresh<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

fn app(fx: &Fixture) -> Router {
    let state = Arc::new(ConfigState {
        service: fx.service.clone(),
        tokens: Arc::new(Tokens),
        checker: Arc::new(Checker),
        permissions: Arc::new(NoRefresh),
    });
    platform_config::http::router(state)
}

struct Call<'a> {
    method: Method,
    uri: &'a str,
    token: Option<&'a str>,
    tenant: Option<&'a str>,
    space: Option<&'a str>,
    body: Option<Value>,
}

impl<'a> Call<'a> {
    fn get(uri: &'a str) -> Self {
        Self {
            method: Method::GET,
            uri,
            token: None,
            tenant: None,
            space: None,
            body: None,
        }
    }

    fn put(uri: &'a str, body: Value) -> Self {
        Self {
            method: Method::PUT,
            uri,
            token: None,
            tenant: None,
            space: None,
            body: Some(body),
        }
    }

    fn token(mut self, token: &'a str) -> Self {
        self.token = Some(token);
        self
    }

    fn tenant(mut self, tenant: &'a str) -> Self {
        self.tenant = Some(tenant);
        self
    }

    fn space(mut self, space: &'a str) -> Self {
        self.space = Some(space);
        self
    }
}

async fn call(app: &Router, call: Call<'_>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(call.method).uri(call.uri);
    if let Some(token) = call.token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(tenant) = call.tenant {
        builder = builder.header("x-tenant-id", tenant);
    }
    if let Some(space) = call.space {
        builder = builder.header("x-space-id", space);
    }
    let body = match call.body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&value).expect("body 应能序列化"))
        }
        None => Body::empty(),
    };
    let request = builder.body(body).expect("请求应能构造");
    let response = app.clone().oneshot(request).await.expect("路由应能响应");
    let status = response.status();
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .expect("响应体应可读")
        .to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| panic!("响应不是 JSON：{}", String::from_utf8_lossy(&bytes)))
    };
    (status, value)
}

fn code(body: &Value) -> &str {
    body.get("code").and_then(Value::as_str).unwrap_or("")
}

fn param<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    body.get("params")?.get(name)
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn unauthenticated_requests_are_401_and_branding_is_public() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    let protected = [
        (Method::GET, "/api/config/entries?keys=logo"),
        (Method::PUT, "/api/config/entries"),
        (Method::GET, "/api/config/platform-entries?keys=scimBaseUrl"),
        (Method::PUT, "/api/config/platform-entries"),
        (Method::GET, "/api/feature-flags"),
        (Method::GET, "/api/feature-flags/newInvoice"),
        (Method::PUT, "/api/feature-flags/newInvoice"),
    ];
    for (method, uri) in protected {
        let request = Call {
            method: method.clone(),
            uri,
            token: None,
            tenant: Some(TENANT_A),
            space: None,
            body: None,
        };
        let (status, body) = call(&app, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(code(&body), "UNAUTHENTICATED", "{method} {uri}");
    }

    // 公开品牌接口：不带令牌也能渲染。
    let (status, body) = call(&app, Call::get("/api/config/branding").tenant(TENANT_A)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["logo"], json!("/branding/tenant-a.svg"));
}

#[tokio::test]
async fn write_permission_matrix_has_no_implicit_hierarchy() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    // M（普通用户）：两种写都 403。
    let (status, body) = call(
        &app,
        Call::put(
            "/api/config/entries",
            json!({"scope":"tenant","entries":[{"key":"pageSize","value":30}]}),
        )
        .token(TOKEN_M)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(code(&body), "INSUFFICIENT_PERMISSION");
    assert_eq!(param(&body, "required"), Some(&json!(PERM_ENTRY)));

    let (status, body) = call(
        &app,
        Call::put("/api/feature-flags/newInvoice", json!({"enabled": true}))
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(param(&body, "required"), Some(&json!(PERM_FLAG)));

    // A（entry + flag）：租户级写成功、平台级写 403。
    let (status, _) = call(
        &app,
        Call::put(
            "/api/config/entries",
            json!({"scope":"tenant","entries":[{"key":"pageSize","value":40}]}),
        )
        .token(TOKEN_A)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(
        &app,
        Call::put(
            "/api/config/platform-entries",
            json!({"scope":"platform","entries":[{"key":"logo","value":"/branding/x.svg"}]}),
        )
        .token(TOKEN_A)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(param(&body, "required"), Some(&json!(PERM_PLATFORM_WRITE)));

    // P（4 个全有）：平台级写成功。
    let (status, _) = call(
        &app,
        Call::put(
            "/api/config/platform-entries",
            json!({"scope":"platform","entries":[{"key":"logo","value":"/branding/y.svg"}]}),
        )
        .token(TOKEN_P)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // O（只有 platform:write）：写不了租户级 / 开关。
    let (status, body) = call(
        &app,
        Call::put(
            "/api/config/entries",
            json!({"scope":"tenant","entries":[{"key":"pageSize","value":66}]}),
        )
        .token(TOKEN_O)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(param(&body, "required"), Some(&json!(PERM_ENTRY)));

    let (status, body) = call(
        &app,
        Call::put("/api/feature-flags/newInvoice", json!({"enabled": true}))
            .token(TOKEN_O)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(param(&body, "required"), Some(&json!(PERM_FLAG)));

    // R（只有 platform:read）：读平台键 200，写任何东西 403。
    let (status, body) = call(
        &app,
        Call::get("/api/config/platform-entries?keys=scimBaseUrl")
            .token(TOKEN_R)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["entries"][0]["value"],
        json!("https://scim.example.com")
    );

    let (status, _) = call(
        &app,
        Call::put(
            "/api/config/entries",
            json!({"scope":"tenant","entries":[{"key":"pageSize","value":12}]}),
        )
        .token(TOKEN_R)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_cross_tenant_header_is_404_and_writes_nothing() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    let (status, body) = call(
        &app,
        Call::put(
            "/api/config/entries",
            json!({"scope":"tenant","entries":[{"key":"pageSize","value":77}]}),
        )
        .token(TOKEN_A)
        .tenant(TENANT_B),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(code(&body), "NOT_FOUND");

    let rows: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM platform_tenant_config \
         WHERE tenant_id = 'tenant-b' AND key = 'pageSize'",
    )
    .fetch_one(&fx.admin_pool)
    .await
    .unwrap();
    assert_eq!(rows.0, 0, "跨租头不得写入 T2");

    // 对照：正确租户头 → 200。
    let (status, _) = call(
        &app,
        Call::put(
            "/api/config/entries",
            json!({"scope":"tenant","entries":[{"key":"pageSize","value":77}]}),
        )
        .token(TOKEN_A)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn read_authorization_splits_platform_scoped_keys() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    // 租户级键：普通用户可读。
    let (status, body) = call(
        &app,
        Call::get("/api/config/entries?keys=pageSize")
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["entries"][0]["value"], json!(20));
    assert_eq!(body["entries"][0]["source"], json!("tenant"));

    // 平台级键：403，且不返回值。
    let (status, body) = call(
        &app,
        Call::get("/api/config/entries?keys=scimBaseUrl")
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(code(&body), "INSUFFICIENT_PERMISSION");
    assert_eq!(param(&body, "required"), Some(&json!(PERM_PLATFORM_READ)));
    assert!(body.get("entries").is_none());

    // 混合批量：整请求 403（不返回 pageSize 的部分结果）。
    let (status, body) = call(
        &app,
        Call::get("/api/config/entries?keys=pageSize,scimBaseUrl")
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.get("entries").is_none());
    assert_eq!(param(&body, "forbiddenKeys"), Some(&json!(["scimBaseUrl"])));

    // space 作用域的 favicon 即使回落平台值，也不需要平台读权限（判据是登记 scope）。
    let (status, body) = call(
        &app,
        Call::get("/api/config/entries?keys=favicon")
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["entries"][0]["source"], json!("platform"));

    // 未登记键：404（注册表先于键级授权）。
    let (status, body) = call(
        &app,
        Call::get("/api/config/entries?keys=notRegistered")
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(code(&body), CODE_KEY_NOT_REGISTERED);

    // 独立平台读路由：M 403、P 200。
    let (status, _) = call(
        &app,
        Call::get("/api/config/platform-entries?keys=scimBaseUrl")
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = call(
        &app,
        Call::get("/api/config/platform-entries?keys=scimBaseUrl")
            .token(TOKEN_P)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["entries"][0]["value"],
        json!("https://scim.example.com")
    );
}

#[tokio::test]
async fn branding_over_http_is_whitelisted_and_ignores_the_space_header() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    let (status, t1) = call(&app, Call::get("/api/config/branding").tenant(TENANT_A)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(t1["logo"], json!("/branding/tenant-a.svg"));
    assert_eq!(t1["title"], json!("T1平台"));

    // 空间头不改变公开品牌（未认证请求没有可信空间身份）。
    let (status, with_space) = call(
        &app,
        Call::get("/api/config/branding")
            .tenant(TENANT_A)
            .space(SPACE_S1),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(with_space["logo"], json!("/branding/tenant-a.svg"));
    assert_ne!(with_space["logo"], json!("/branding/space-s1.svg"));

    // 无租户头 → 平台默认。
    let (status, platform) = call(&app, Call::get("/api/config/branding")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(platform["logo"], json!("/branding/platform-a.svg"));
    assert_eq!(platform["title"], json!("示例平台"));

    // T2 → 自己的租户值。
    let (status, t2) = call(&app, Call::get("/api/config/branding").tenant(TENANT_B)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(t2["logo"], json!("/branding/tenant-b.svg"));

    // 键集合 ⊆ 白名单，运维键缺席。
    let object = t1.as_object().expect("扁平 map");
    for key in object.keys() {
        assert!(
            platform_config::is_branding_key(key),
            "公开接口泄露了非白名单键：{key}"
        );
    }
    for forbidden in ["conflictStrategy", "scimBaseUrl", "cacheTtlSeconds"] {
        assert!(!object.contains_key(forbidden));
    }
}

#[tokio::test]
async fn flag_routes_are_gated_and_unknown_flags_degrade_to_disabled() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    let (status, body) = call(
        &app,
        Call::get("/api/feature-flags/newInvoice")
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["flag"], json!("newInvoice"));
    assert_eq!(body["enabled"], json!(true));
    assert_eq!(body["source"], json!("tenant"));

    // T2 未配置 → 200 + false + default（不是 404）。
    let (status, body) = call(
        &app,
        Call::get("/api/feature-flags/newInvoice")
            .token(TOKEN_T2)
            .tenant(TENANT_B),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], json!(false));
    assert_eq!(body["source"], json!("default"));

    // 拼错的合法名 → 200 + false + default。
    let (status, body) = call(
        &app,
        Call::get("/api/feature-flags/newinvoce")
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], json!(false));
    assert_eq!(body["source"], json!("default"));

    // 非法名 → 400 FLAG_NAME_INVALID（含换行 / 超长 / kebab）。
    let long_name = "a".repeat(1024);
    for name in ["bad%0AINJECTED", long_name.as_str(), "new-invoice"] {
        let uri = format!("/api/feature-flags/{name}");
        let (status, body) = call(&app, Call::get(&uri).token(TOKEN_M).tenant(TENANT_A)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "name={name}");
        assert_eq!(code(&body), CODE_FLAG_NAME_INVALID, "name={name}");
    }

    // 批量接口：返回本租户全部已登记开关（按名字排序）。
    let (status, body) = call(
        &app,
        Call::get("/api/feature-flags")
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let flags = body["flags"].as_array().expect("批量响应应有 flags 数组");
    assert!(flags.len() >= 2, "至少应有 beta 与 newInvoice：{body}");
    assert!(
        flags
            .iter()
            .any(|flag| flag["flag"] == "newInvoice" && flag["enabled"] == true),
        "newInvoice 应为 true：{body}"
    );

    // M 不能改开关。
    let (status, _) = call(
        &app,
        Call::put("/api/feature-flags/beta", json!({"enabled": true}))
            .token(TOKEN_M)
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A 可以改（enabled + rolloutPercent 同一条数据）。
    let (status, body) = call(
        &app,
        Call::put(
            "/api/feature-flags/beta",
            json!({"enabled": true, "rolloutPercent": 50}),
        )
        .token(TOKEN_A)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["flag"], json!("beta"));
    let stored = raw_value(
        &fx.admin_pool,
        "platform_tenant_config",
        "flags.beta.rolloutPercent",
    )
    .await;
    assert_eq!(stored.as_deref(), Some("50"));
}

#[tokio::test]
async fn the_generic_write_route_rejects_the_flag_namespace() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    for key in ["beta", "flags.beta.rolloutPercent"] {
        let (status, body) = call(
            &app,
            Call::put(
                "/api/config/entries",
                json!({"scope":"tenant","entries":[{"key":key,"value":true}]}),
            )
            .token(TOKEN_A)
            .tenant(TENANT_A),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "key={key}");
        assert_eq!(code(&body), CODE_FLAG_USE_FLAG_ENDPOINT, "key={key}");
    }
}

#[tokio::test]
async fn scope_type_and_key_errors_keep_the_exact_codes_over_http() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    let cases: Vec<(Value, &str)> = vec![
        // 作用域越级。
        (
            json!({"scope":"tenant","entries":[{"key":"conflictStrategy","value":"remote_wins"}]}),
            CODE_SCOPE_NOT_ALLOWED,
        ),
        // 通用路由不接受 scope=platform。
        (
            json!({"scope":"platform","entries":[{"key":"logo","value":"/x.svg"}]}),
            CODE_SCOPE_NOT_ALLOWED,
        ),
        // 空间级非白名单键。
        (
            json!({"scope":"space","entries":[{"key":"loginBanner","value":"/b.png"}]}),
            CODE_SCOPE_NOT_ALLOWED,
        ),
        // 类型错（数字的字符串形式）。
        (
            json!({"scope":"tenant","entries":[{"key":"pageSize","value":"20"}]}),
            CODE_TYPE_MISMATCH,
        ),
        // 键名格式。
        (
            json!({"scope":"tenant","entries":[{"key":"Theme.radius","value":1}]}),
            CODE_KEY_INVALID,
        ),
        // 未登记。
        (
            json!({"scope":"tenant","entries":[{"key":"notRegistered","value":1}]}),
            CODE_KEY_NOT_REGISTERED,
        ),
    ];

    for (body, expected) in cases {
        let (status, response) = call(
            &app,
            Call::put("/api/config/entries", body.clone())
                .token(TOKEN_A)
                .tenant(TENANT_A)
                .space(SPACE_S1),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
        assert_eq!(code(&response), expected, "body={body} response={response}");
    }

    // 正确类型 → 200（证明不是恒拒）。
    let (status, _) = call(
        &app,
        Call::put(
            "/api/config/entries",
            json!({"scope":"tenant","entries":[{"key":"pageSize","value":20}]}),
        )
        .token(TOKEN_A)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_platform_write_is_visible_on_the_next_request() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    // 先读一次（缓存已填充）。
    let (status, body) = call(
        &app,
        Call::get("/api/config/entries?keys=favicon")
            .token(TOKEN_T2)
            .tenant(TENANT_B),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["entries"][0]["value"], json!("/favicon-platform.svg"));

    // 平台级写（不重启、不清缓存）。
    let (status, _) = call(
        &app,
        Call::put(
            "/api/config/platform-entries",
            json!({"scope":"platform","entries":[{"key":"favicon","value":"/favicon-f2.svg"}]}),
        )
        .token(TOKEN_P)
        .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 立刻再读 → 新值（T1 与 T2 都要看到）。
    for token in [TOKEN_M, TOKEN_T2] {
        let tenant = if token == TOKEN_T2 {
            TENANT_B
        } else {
            TENANT_A
        };
        let (status, body) = call(
            &app,
            Call::get("/api/config/entries?keys=favicon")
                .token(token)
                .tenant(tenant),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["entries"][0]["value"],
            json!("/favicon-f2.svg"),
            "token={token}"
        );
    }
}

#[tokio::test]
async fn invalid_tokens_are_rejected_without_side_effects() {
    let _guard = serial().await;
    let fx = setup().await;
    let app = app(&fx);

    let (status, body) = call(
        &app,
        Call::get("/api/config/entries?keys=logo")
            .token("not-a-real-token")
            .tenant(TENANT_A),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&body), "UNAUTHENTICATED");

    // 错误体字段齐全（C6）。
    for field in [
        "type", "title", "status", "detail", "instance", "code", "trace_id",
    ] {
        assert!(body.get(field).is_some(), "缺少 C6 字段：{field}");
    }
    assert!(body["detail"].as_str().unwrap_or_default().is_ascii());
}
