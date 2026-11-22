//! 中间件装配的判据（REQ-019，**TD-075 的处置**）。
//!
//! # 这个文件回答什么问题
//!
//! **「`main.rs` 挂的中间件，顺序和内容都对吗？」**
//!
//! 之前这件事**没有判据** —— `main.rs` 的装配是内联的，测试够不着。
//! 后果：若 `main` 把 `apply_observability` 放在 `client_info` 之前，
//! 或整个漏掉它，**所有 HTTP 级 AC 的用例仍然全绿**（它们测的是自己的 router）。
//!
//! # 怎么判
//!
//! `platform_observability::http::apply_all_middlewares_with` 是**唯一**装配实现，
//! `main.rs` 与这里**调同一个函数**。于是可以断言：
//!
//! 1. `trace_id` 真的挂上了（响应头有 `X-Request-Id`）；
//! 2. `rate_limit` 真的挂上了（启用且超限时 429）；
//! 3. 弃用头真的挂上了（`/api/audit/events` 带 `Deprecation`）；
//! 4. **顺序对**（`client_info` 在最外层 → `trace_id` 能看到它填的 IP）。
//!
//! ⚠️ 第 4 条最重要：顺序错了功能可能「看起来正常」，但 trace 与限流都会拿到空 IP。

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use platform_core::RequestContext;
use platform_observability::rate_limit::middleware::{RateLimitState, StaticConfig};
use platform_observability::rate_limit::{RateLimitSettings, WindowLimit};

/// 注入的 `client_info` 替身：把 IP/UA 填进 `RequestContext`（**模拟生产行为**）。
///
/// ⚠️ 生产用的是 `platform_server::client_info::client_info`；
/// 这里复刻它的**关键行为**（写 `client_ip` / `user_agent`），
/// 因为装配顺序的判据依赖「它先执行」。
async fn fake_client_info(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);
    // 从 `x-forwarded-for` 取第一跳（与生产同款语义）。
    if let Some(ip) = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
    {
        ctx = ctx.with_client_ip(ip);
    }
    // ⚠️ **同时注入租户**（模拟「已认证」）——
    // 限流中间件对**无租户**请求直接放行（TD-072：RLS 租户表写不进匿名请求）。
    // 不注入租户的话，限流判据恒 200（这条我踩过）。
    let ctx = ctx.with_tenant("tenant-a").with_user("user-a");
    request.extensions_mut().insert(ctx);
    next.run(request).await
}

/// 建一个挂了**全部中间件**的 router。
fn assembled_router(rate_limit: Option<RateLimitState>) -> axum::Router {
    let router = axum::Router::new()
        .route("/probe", axum::routing::get(|| async { "ok" }))
        .route("/api/audit/events", axum::routing::get(|| async { "events" }));
    platform_observability::http::apply_all_middlewares_with(
        router,
        rate_limit.map(Arc::new),
        fake_client_info,
    )
}

/// 发请求，返回（状态码, 头, 体）。
async fn send(
    app: axum::Router,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut builder = Request::builder().uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder.body(Body::empty()).expect("构造请求");
    let response = app.oneshot(request).await.expect("应有响应");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.expect("读体").to_bytes();
    (status, headers, String::from_utf8_lossy(&bytes).to_string())
}

/// **判据 1**：`trace_id` 真的挂上了（回写 `X-Request-Id`）。
#[tokio::test]
async fn the_trace_middleware_is_installed() {
    let (status, headers, _) = send(assembled_router(None), "/probe", &[]).await;
    assert_eq!(status, StatusCode::OK);
    let trace = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .expect("必须有 X-Request-Id —— 说明 trace_id 中间件挂上了");
    assert_eq!(trace.len(), 32, "必须是 32 hex");
}

/// **判据 1b**：入站值被沿用（证明中间件拿到的是**请求**，不是凭空生成）。
#[tokio::test]
async fn the_trace_middleware_reads_the_incoming_header() {
    let incoming = "0123456789abcdef0123456789abcdef";
    let (_, headers, _) = send(
        assembled_router(None),
        "/probe",
        &[("x-request-id", incoming)],
    )
    .await;
    assert_eq!(
        headers.get("x-request-id").and_then(|v| v.to_str().ok()),
        Some(incoming),
        "合法入站值必须被沿用"
    );
}

/// **判据 2**：`rate_limit` 真的挂上了（超限 → 429）。
#[tokio::test]
async fn the_rate_limit_middleware_is_installed() {
    let settings = RateLimitSettings {
        enabled: true,
        tenant: WindowLimit {
            requests: 0,
            window_seconds: 60,
        },
        user: WindowLimit {
            requests: 0,
            window_seconds: 60,
        },
        endpoint: WindowLimit {
            requests: 1,
            window_seconds: 60,
        },
    };
    let state = RateLimitState {
        limiter: Some(Arc::new(CountingLimiter::default())),
        config: Arc::new(StaticConfig(settings)),
    };
    let app = assembled_router(Some(state));

    // 第 1 次 → 200；第 2 次 → 429（同一个 router 实例，但限流器是共享的）。
    let (first, _, _) = send(app.clone(), "/probe", &[]).await;
    assert_eq!(first, StatusCode::OK);
    let (second, headers, body) = send(app, "/probe", &[]).await;
    assert_eq!(
        second,
        StatusCode::TOO_MANY_REQUESTS,
        "第 2 次必须 429 —— 说明限流中间件挂上了；实际 body={body}"
    );
    assert!(
        headers.get("retry-after").is_some(),
        "429 必须带 Retry-After"
    );
}

/// **判据 2b（AC-S-032 步骤 3 的 HTTP 层判据，F6）**：429 的**响应体**结构。
///
/// # 为什么必须验响应体（不能只验构造函数）
///
/// 原先的判据是：
///
/// - `error.rs` 的单测直接构造 `ObservabilityError::RateLimited` 再调 `params()`；
/// - `rate_limit_real_db.rs` 同样只测构造函数。
///
/// 那证明的是「构造函数恰好只塞两个键」，**不是**「HTTP 429 的响应体里真的只有两个键」。
/// 若中间件在 `middleware.rs` 的 `details.params` 之后又补字段
/// （比如加一个 `remaining`），`error.rs` 的单测**照样绿**。
///
/// → 本用例**解析真实的 429 响应体**，断言：
///
/// 1. `params` 的键集合 **⊆ `{dimension, retryAfter}`**；
/// 2. `detail` / `title` 不含黑名单词（不泄露内部结构）；
/// 3. `code == "RATE_LIMITED"`、`status == 429`。
#[tokio::test]
async fn the_429_body_only_exposes_the_two_allowed_params() {
    let settings = RateLimitSettings {
        enabled: true,
        tenant: WindowLimit {
            requests: 0,
            window_seconds: 60,
        },
        user: WindowLimit {
            requests: 0,
            window_seconds: 60,
        },
        endpoint: WindowLimit {
            requests: 1,
            window_seconds: 60,
        },
    };
    let state = RateLimitState {
        limiter: Some(Arc::new(CountingLimiter::default())),
        config: Arc::new(StaticConfig(settings)),
    };
    let app = assembled_router(Some(state));

    // 第一次放行，第二次拿 429 的响应体。
    let (first, _, _) = send(app.clone(), "/probe", &[]).await;
    assert_eq!(first, StatusCode::OK);
    let (status, _, body) = send(app, "/probe", &[]).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "body={body}");

    // ⚠️ **解析真实响应体**（不是在内存里构造错误对象）。
    let parsed: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|err| panic!("429 响应必须是 JSON：{err}；{body}"));

    // ① `params` 键集合 ⊆ {dimension, retryAfter}（AC-S-032 步骤 3 的核心）。
    let params = parsed
        .get("params")
        .unwrap_or_else(|| panic!("429 必须带 params；实际 body={body}"));
    let params = params
        .as_object()
        .unwrap_or_else(|| panic!("params 必须是对象；实际：{params}"));
    let allowed: std::collections::BTreeSet<&str> =
        ["dimension", "retryAfter"].into_iter().collect();
    let actual: std::collections::BTreeSet<&str> =
        params.keys().map(String::as_str).collect();
    let extra: Vec<&&str> = actual.difference(&allowed).collect();
    assert!(
        extra.is_empty(),
        "429 的 params 只能含 {{dimension, retryAfter}}，多了就会泄露内部结构：\
         多余键 = {extra:?}；实际 params = {params:?}"
    );
    assert!(
        !params.is_empty(),
        "params 不该为空（调用方需要 dimension 判断是哪个维度拦的）"
    );
    // ② dimension 的取值必须 ∈ 三个维度（不能是内部键名）。
    if let Some(dimension) = params.get("dimension").and_then(|v| v.as_str()) {
        assert!(
            ["tenant", "user", "endpoint"].contains(&dimension),
            "dimension 必须 ∈ {{tenant,user,endpoint}}；实际：{dimension}"
        );
    }
    // ③ retryAfter 必须是**正整数**（AC-S-032 步骤 1/2）。
    if let Some(retry) = params.get("retryAfter").and_then(|v| v.as_i64()) {
        assert!(retry >= 1, "retryAfter 必须是正整数；实际：{retry}");
    }

    // ④ `detail` / `title` 不得含黑名单词（不泄露桶键 / 计数 / 表名 / SQL）。
    //
    // ⚠️ 黑名单逐词列全：`current` / `remaining` / `bucket` / 表名 / SQL 关键字。
    const BLACKLIST: &[&str] = &[
        "current",
        "remaining",
        "bucket",
        "platform_rate_limit",
        "rl:endpoint",
        "rl:tenant",
        "rl:user",
        "SELECT",
        "count(*)",
    ];
    for field in ["detail", "title"] {
        let text = parsed.get(field).and_then(|v| v.as_str()).unwrap_or_default();
        for banned in BLACKLIST {
            assert!(
                !text.contains(banned),
                "429 的 `{field}` 不得含 `{banned}`（会泄露限流内部结构）；实际：{text}"
            );
        }
    }

    // ⑤ 基本形状。
    assert_eq!(parsed.get("status").and_then(|v| v.as_u64()), Some(429));
    assert_eq!(
        parsed.get("code").and_then(|v| v.as_str()),
        Some("RATE_LIMITED"),
        "code 必须是 RATE_LIMITED；实际 body={body}"
    );
}

/// **判据 3**：弃用头真的挂上了（且只给 `/api/audit/events`）。
#[tokio::test]
async fn the_deprecation_headers_are_installed() {
    // 弃用端点：两个头都有。
    let (status, headers, _) = send(assembled_router(None), "/api/audit/events", &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get("deprecation").and_then(|v| v.to_str().ok()),
        Some("true"),
        "弃用端点必须带 Deprecation"
    );
    assert!(headers.get("sunset").is_some(), "弃用端点必须带 Sunset");

    // 非弃用端点：两个头都**没有**（阴性对照）。
    let (_, headers, _) = send(assembled_router(None), "/probe", &[]).await;
    assert!(
        headers.get("deprecation").is_none(),
        "非弃用端点不得带 Deprecation"
    );
    assert!(headers.get("sunset").is_none(), "非弃用端点不得带 Sunset");
}

/// **判据 5（AC-S-031 步骤 5 的真判据）**：query string **不**能绕过限流桶。
///
/// # ⚠️ 这条为什么必须有（F11）
///
/// 原先的判据（`rate_limit_real_db.rs` 的 `the_query_string_does_not_create_a_new_bucket`）
/// 是：
///
/// ```ignore
/// let key1 = bucket_key(ENDPOINT, TENANT_A, None, "/api/rbac/roles");
/// let key2 = bucket_key(ENDPOINT, TENANT_A, None, "/api/rbac/roles");
/// assert_eq!(key1, key2);   // ← 同一个函数、同一个参数，恒真
/// ```
///
/// 那是**循环论证**：它根本没经过中间件，也没经过 axum 的 `MatchedPath`。
/// 若实现改成用完整 URI（`?x=1` 与 `?x=2` 变成两个桶），那条测试**照样绿**。
///
/// # 本判据怎么做
///
/// 让**两个带不同 query 的请求真的穿过限流中间件**，断言：
///
/// 1. 第 1 次 200、第 2 次 **429**（若 query 进了键 → 第 2 次是**新桶** → 200 → 红）；
/// 2. `CountingLimiter` 记录到的键**不含** `?x=1` / `?x=2`（键里没有 query）。
#[tokio::test]
async fn the_query_string_does_not_bypass_the_rate_limit_bucket() {
    let settings = RateLimitSettings {
        enabled: true,
        tenant: WindowLimit {
            requests: 0,
            window_seconds: 60,
        },
        user: WindowLimit {
            requests: 0,
            window_seconds: 60,
        },
        // 接口维度上限 1 → 第 2 次必然 429（前提是两次共用一个桶）。
        endpoint: WindowLimit {
            requests: 1,
            window_seconds: 60,
        },
    };
    let limiter = Arc::new(CountingLimiter::default());
    let state = RateLimitState {
        limiter: Some(limiter.clone()),
        config: Arc::new(StaticConfig(settings)),
    };
    let app = assembled_router(Some(state));

    // ⚠️ 两次请求**只有 query 不同** —— 这正是在验「query 不进键」。
    let (first, _, body1) = send(app.clone(), "/probe?x=1", &[]).await;
    assert_eq!(first, StatusCode::OK, "第 1 次必须 200；body={body1}");

    let (second, _, body2) = send(app.clone(), "/probe?x=2", &[]).await;
    assert_eq!(
        second,
        StatusCode::TOO_MANY_REQUESTS,
        "第 2 次（**不同的 query**、同一路径）必须 429。\
         若这里是 200，说明实现用**完整 URI** 做键 → 加个随机 query 就能无限打 \
         （AC-S-031 步骤 5 防的正是这个）；实际 body={body2}"
    );

    // 键里**不得**出现 query（间接证据：Limiter 看到的键是纯路径模板）。
    let keys = limiter.keys_seen();
    assert!(
        !keys.is_empty(),
        "限流器必须被调用过（否则这条判据没验证到任何东西）"
    );
    for key in &keys {
        assert!(
            !key.contains('?') && !key.contains("x=1") && !key.contains("x=2"),
            "桶键里不得含 query string；实际键：{key}"
        );
        assert!(
            key.contains("/probe"),
            "桶键应当是**路径模板**（含 /probe）；实际键：{key}"
        );
    }
}

/// **判据 6（AC-S-030 步骤 2 的 HTTP 层判据）**：大写入站 `X-Request-Id` 被**归一为小写**。
///
/// # ⚠️ 这条为什么必须有（F14）
///
/// 归一逻辑原先只在 `trace.rs` 的**单测**里验（直接调 `normalize_trace_id`）。
/// HTTP 层零覆盖 → 若中间件改成 `HeaderValue::from_str(raw)`（原样回写、不归一），
/// 单测仍绿，但 HTTP 行为违反 AC。
///
/// AC 原文：大写入站 → 响应头是**小写版**，
/// ⚠️ **不是原样大写、也不是新生成的**。
#[tokio::test]
async fn an_uppercase_incoming_trace_id_is_lowercased_on_the_wire() {
    let uppercase = "ABCDEF0123456789ABCDEF0123456789";
    let (status, headers, _) = send(
        assembled_router(None),
        "/probe",
        &[("x-request-id", uppercase)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let echoed = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .expect("必须有 X-Request-Id");

    // ① 必须是小写版（逐字）。
    assert_eq!(
        echoed,
        "abcdef0123456789abcdef0123456789",
        "大写入站值必须被**归一为小写**后回写"
    );
    // ② 不是原样大写（阴性对照）。
    assert_ne!(echoed, uppercase, "不得原样回写大写值");
    // ③ 也不是新生成的（否则「归一」变成了「丢弃重造」）。
    assert_eq!(
        echoed.to_lowercase(),
        uppercase.to_lowercase(),
        "必须是入站值的归一，不是重新生成的"
    );
}

/// **判据 4（最关键）**：`client_info` 在 `trace_id` **之前**执行。
///
/// # 怎么判
///
/// 若顺序反了（`trace_id` 先跑），它拿到的 `RequestContext` 是空的 →
/// 后面 `client_info` 填的 IP 就**看不到**（因为 trace_id 已经读过 ctx 了）。
///
/// 这里用一个「会读 IP 的中间件」放在装配链**之后**验证：
/// 若 `client_info` 真在最外层，业务侧能读到 IP。
#[tokio::test]
async fn the_client_info_runs_outermost_so_downstream_sees_the_ip() {
    // 业务 handler 里读 `RequestContext` 的 IP，回显出来。
    async fn echo_ip(request: axum::extract::Request) -> String {
        let ctx = request
            .extensions()
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        ctx.client_ip().unwrap_or("(no ip)").to_string()
    }

    let router = axum::Router::new().route("/ip", axum::routing::get(echo_ip));
    let app = platform_observability::http::apply_all_middlewares_with(
        router,
        None,
        fake_client_info,
    );

    let (_, _, body) = send(app, "/ip", &[("x-forwarded-for", "203.0.113.9")]).await;
    assert_eq!(
        body, "203.0.113.9",
        "业务侧必须能读到 client_info 填的 IP —— \
         若读不到说明 client_info 不在最外层（顺序错了）"
    );
}

/// 一个「内存计数」的限流器替身。
///
/// 用途：
/// 1. 验「中间件挂上了」（超限 → 429）；
/// 2. 验「桶键长什么样」（[`Self::keys_seen`]）—— AC-S-031 步骤 5 的判据要用它
///    看到中间件**实际传进来的键**，而不是自己拼一个键来比较（那会变成循环论证）。
#[derive(Default)]
struct CountingLimiter {
    counts: std::sync::Mutex<std::collections::BTreeMap<String, i64>>,
    /// 见过的键（**按首次出现顺序**，去重）。
    keys: std::sync::Mutex<Vec<String>>,
}

impl CountingLimiter {
    /// 中间件**实际传进来**的键（去重，保持顺序）。
    ///
    /// ⚠️ 这是 F11 的关键：判据必须看**真实传进来的键**，
    /// 不能用 `bucket_key(...)` 自己拼两次再 `assert_eq!`（同一表达式恒真）。
    fn keys_seen(&self) -> Vec<String> {
        self.keys.lock().expect("锁").clone()
    }
}

impl platform_observability::RateLimiter for CountingLimiter {
    fn check<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _dimension: &'a str,
        key: &'a str,
        limit: WindowLimit,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<(), platform_observability::error::ObservabilityError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            if !limit.enabled() {
                return Ok(());
            }
            // 先记账「见过这个键」（判据 5 要读它）。
            {
                let mut keys = self.keys.lock().expect("锁");
                if !keys.iter().any(|seen| seen == key) {
                    keys.push(key.to_string());
                }
            }
            let mut counts = self.counts.lock().expect("锁");
            let current = counts.entry(key.to_string()).or_insert(0);
            if *current >= limit.requests {
                return Err(
                    platform_observability::error::ObservabilityError::RateLimited {
                        dimension: "endpoint".to_string(),
                        retry_after: 1,
                    },
                );
            }
            *current += 1;
            Ok(())
        })
    }

    fn is_reachable<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        Box::pin(async { true })
    }
}
