//! OTel 的端到端验证（REQ-019，AC-S-037）。
//!
//! # 这个文件回答一个问题
//!
//! **「span 的 `trace_id` 真的等于入站 `X-Request-Id` 吗？」**
//!
//! 这是 AC-S-037 步骤 1 的核心判据，也是 TD-074 要解决的问题：
//! `tracing_opentelemetry` **默认会自己生成** trace_id ——
//! 必须显式 `set_parent(远端上下文)` 才会沿用入站值。
//!
//! # 为什么用 `InMemorySpanExporter`
//!
//! 本机**没有 collector**（TD-069）→ 不能用真导出验。
//! 内存导出器能拿到 finished spans，足以断言「span 的 trace_id 是什么」。
//!
//! ⚠️ 本文件验的是**能验的部分**，不声称验过「span 到了 Jaeger」。

use std::sync::Arc;

use axum::body::Body;
use axum::routing::get;
use axum::Router;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use tower::ServiceExt;

/// 造一个挂了 OTel layer 的 subscriber + 内存导出器。
///
/// ⚠️ 返回的 `provider` 必须活到测试结束（提前 drop 会 flush 掉）。
fn setup_otel() -> (InMemorySpanExporter, SdkTracerProvider) {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        // ⚠️ 测试用 **simple** 导出器（立即导出）；
        // 生产用 `batch`（有界队列 + 不影响业务）。
        .with_simple_exporter(exporter.clone())
        .build();
    (exporter, provider)
}

/// **AC-S-037 步骤 1**：span 的 `trace_id` **逐字等于** 入站 `X-Request-Id`。
#[tokio::test(flavor = "current_thread")]
async fn the_exported_span_carries_the_incoming_trace_id() {
    let (exporter, provider) = setup_otel();
    let tracer = provider.tracer("test");

    // 只挂 trace_id 中间件 + 一个最简 handler。
    let app: Router = Router::new()
        .route("/probe", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(
            platform_observability::trace::middleware::trace_id,
        ));

    let incoming = "0123456789abcdef0123456789abcdef";

    // ⚠️ 用 `with_default` 装 subscriber（线程局部）→ 必须 `current_thread`
    // runtime，且整个请求在同一个线程里跑完。
    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(tracer));
    let _guard = tracing::subscriber::set_default(subscriber);

    let request = axum::http::Request::builder()
        .uri("/probe")
        .header("x-request-id", incoming)
        .body(Body::empty())
        .expect("构造请求");
    let response = app.oneshot(request).await.expect("应有响应");
    assert_eq!(response.status(), 200);

    // 响应头必须是入站值（中间件回写）。
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some(incoming),
        "响应头必须回写入站值"
    );

    // ⚠️ 关键断言：导出的 span 的 trace_id == 入站值。
    let spans = exporter.get_finished_spans().expect("应能取到 span");
    assert!(
        !spans.is_empty(),
        "必须至少导出 1 个 span（否则 otel layer 没挂上）"
    );
    let http_span = spans
        .iter()
        .find(|s| s.name == "http.request")
        .expect("必须有 http.request span");
    assert_eq!(
        http_span.span_context.trace_id().to_string(),
        incoming,
        "span 的 trace_id 必须等于入站 X-Request-Id —— \
         若不等，说明没有 set_parent（OTel 会自己生成一个）"
    );
}

/// **AC-S-037 步骤 3**：`OTEL_SDK_DISABLED=true` 时 provider 不创建。
#[tokio::test]
async fn a_disabled_sdk_yields_an_disabled_guard() {
    let cfg = platform_observability::otel::OtelConfig {
        disabled: true,
        ..platform_observability::otel::OtelConfig::default()
    };
    let guard = platform_observability::otel::init_otel(&cfg);
    assert!(!guard.is_enabled(), "禁用的 SDK 不该创建 provider");
    assert!(guard.provider().is_none());
}

/// **未挂 OTel layer 时 `set_parent` 是 no-op**（不会 panic）。
///
/// ⚠️ 这条保护「未启用 OTel 的部署」—— 中间件里无条件调 `set_parent`，
/// 若它在没有 layer 时 panic，那些部署就全挂了。
#[tokio::test(flavor = "current_thread")]
async fn a_request_works_without_an_otel_layer() {
    let app: Router = Router::new()
        .route("/probe", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(
            platform_observability::trace::middleware::trace_id,
        ));

    let request = axum::http::Request::builder()
        .uri("/probe")
        .header("x-request-id", "abcdef0123456789abcdef0123456789")
        .body(Body::empty())
        .expect("构造请求");
    let response = app.oneshot(request).await.expect("应有响应");
    assert_eq!(response.status(), 200, "没有 otel layer 也必须正常工作");
}

/// 非法入站值**不会**进入 span（也不回显）。
#[tokio::test(flavor = "current_thread")]
async fn an_invalid_trace_id_is_not_propagated_to_the_span() {
    let (exporter, provider) = setup_otel();
    let tracer = provider.tracer("test");

    let app: Router = Router::new()
        .route("/probe", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(
            platform_observability::trace::middleware::trace_id,
        ));

    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(tracer));
    let _guard = tracing::subscriber::set_default(subscriber);

    // ⚠️ **实测发现**：含 `\r\n` 的值**根本进不了 HTTP header** ——
    // axum/hyper 的 `HeaderValue` 会返回 `InvalidHeaderValue`。
    // → 「CRLF 日志注入」在**传输层**就被挡住了，到不了我们的校验。
    //
    // 所以这里用「长度对但含非 hex 字符」的值来验中间件的校验路径
    // （CRLF 那一路由 `trace::tests::a_crlf_injection_trace_id_is_rejected` 单测覆盖，
    //  那是直接调 `normalize_trace_id`，不经过 HTTP header 解析）。
    let bad = "gggggggggggggggggggggggggggggggg";
    let request = axum::http::Request::builder()
        .uri("/probe")
        .header("x-request-id", bad)
        .body(Body::empty())
        .expect("构造请求");
    let response = app.oneshot(request).await.expect("应有响应");

    // 响应头是新生成的值，不是非法值。
    let echoed = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert_ne!(echoed, bad, "非法值不得回显");
    assert_eq!(echoed.len(), 32, "回显的必须是新生成的 32 hex");

    // span 的 trace_id 也必须是新值（不是非法值解析出来的）。
    let spans = exporter.get_finished_spans().expect("取 span");
    if let Some(http_span) = spans.iter().find(|s| s.name == "http.request") {
        assert_eq!(
            http_span.span_context.trace_id().to_string(),
            echoed,
            "span 的 trace_id 必须与回显的新值一致"
        );
    }
}

/// 清理：显式 shutdown provider（避免测试结束时残留导出线程）。
#[allow(dead_code)]
fn shutdown(provider: SdkTracerProvider) {
    let _ = Arc::new(provider).shutdown();
}
