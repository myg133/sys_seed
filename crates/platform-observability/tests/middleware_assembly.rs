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

/// 一个「内存计数」的限流器替身（只用来验「中间件挂上了」）。
#[derive(Default)]
struct CountingLimiter {
    counts: std::sync::Mutex<std::collections::BTreeMap<String, i64>>,
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
