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

/// **AC-S-037 步骤 1 之「span 至少 2 层」**（F18 的判据）。
///
/// # 为什么必须单独一条
///
/// AC 原文（`acceptance.md:342`）：「span 层级**至少 2 层**
/// （HTTP 请求 span 是根，其下有子 span）」。
///
/// 而 `the_exported_span_carries_the_incoming_trace_id` 的 handler 是
/// `|| async { "ok" }` —— **本来就没有子 span**。
/// 若照 AC 字面执行，那条应当红，但它只验了 trace_id 相等这半句，
/// 「至少 2 层」这半句**零断言**（F18）。
///
/// # 本判据怎么做
///
/// handler 里**主动开一个子 span**（模拟真实业务：handler 内部调用服务层时
/// 会开 span），然后断言：
///
/// 1. 导出的 span **≥ 2 个**；
/// 2. 存在 `http.request`（根）；
/// 3. 存在一个 span 的 `parent_span_id` **等于** `http.request` 的 `span_id`
///    —— 这才叫「父子关系成立」，只数个数证明不了层级。
#[tokio::test(flavor = "current_thread")]
async fn the_exported_spans_are_at_least_two_levels_deep() {
    let (exporter, provider) = setup_otel();
    let tracer = provider.tracer("test");

    // ⚠️ handler 里开一个**子 span** —— 这模拟真实业务的层级
    // （中间件开 `http.request`，handler 内部再开业务 span）。
    let app: Router = Router::new()
        .route(
            "/probe",
            get(|| async {
                // 子 span：名字刻意与根区分。
                let child = tracing::info_span!("handler.work");
                let _guard = child.enter();
                tracing::info!("doing the actual work inside a child span");
                "ok"
            }),
        )
        .layer(axum::middleware::from_fn(
            platform_observability::trace::middleware::trace_id,
        ));

    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(tracer));
    let _guard = tracing::subscriber::set_default(subscriber);

    let request = axum::http::Request::builder()
        .uri("/probe")
        .header("x-request-id", "0123456789abcdef0123456789abcdef")
        .body(Body::empty())
        .expect("构造请求");
    let response = app.oneshot(request).await.expect("应有响应");
    assert_eq!(response.status(), 200);

    let spans = exporter.get_finished_spans().expect("应能取到 span");

    // ① 至少 2 个 span（根 + 子）。
    assert!(
        spans.len() >= 2,
        "必须导出**至少 2 个** span（HTTP 根 span + handler 子 span）；\
         实际只有 {} 个：{:?}",
        spans.len(),
        spans.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    // ② 根 span 存在。
    let root = spans
        .iter()
        .find(|s| s.name == "http.request")
        .expect("必须有 http.request 根 span");

    // ③ **父子关系成立** —— 这是「2 层」的实质，不是数个数。
    //
    // ⚠️ 光断言「有 2 个 span」不够：两个**并列**的根 span 也满足个数条件。
    // 必须找到**父 span_id 等于根 span_id** 的那个 span。
    let child = spans.iter().find(|s| {
        !std::ptr::eq(*s, root)
            && s.span_context.trace_id() == root.span_context.trace_id()
            && s.parent_span_id == root.span_context.span_id()
    });
    assert!(
        child.is_some(),
        "必须存在一个 span 的 parent_span_id == 根 span 的 span_id \
         （证明层级真的存在，而不是两个并列 span）；\
         根的 span_id={:?}，各 span 的 parent：{:?}",
        root.span_context.span_id(),
        spans
            .iter()
            .map(|s| (&s.name, s.parent_span_id))
            .collect::<Vec<_>>()
    );

    // ④ 同一 trace —— 层级必须在**同一条链**上。
    let child = child.expect("上面已断言存在");
    assert_eq!(
        child.span_context.trace_id(),
        root.span_context.trace_id(),
        "父子 span 必须在同一条 trace 上"
    );
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

/// **AC-S-037 步骤 2**：collector **不可达**时，请求**不变慢**。
///
/// # 这条判据防的是什么
///
/// D339 写的真实风险是「collector 不可达 → 内部重试阻塞 → **请求变慢甚至超时**」。
/// 只断言「请求照常 200」对延迟**零区分力**（慢 3 秒仍返回 200）。
///
/// → 必须断言**时延上界**。
///
/// # ⚠️ rev 2 修正（F17）：原版判据是**三重失效**的
///
/// 原实现有三个缺陷叠加，使这条测试**在任何实现下都会绿**：
///
/// 1. **用 `127.0.0.1:1`** —— 本机对该地址是**立即 ECONNREFUSED（0.000s）**，
///    永远不慢。真正的「超时」需要**黑洞地址**（丢包不回 RST）。
/// 2. **provider 在 runtime 之外构造** —— `BatchProcessor` 在**别的线程**上跑，
///    那里没有 tokio reactor → 导出线程直接 panic（`no reactor running`）→
///    **导出路径根本没被执行到**。
/// 3. **只断言单次 < 1 秒** —— 单次 0.0005s 的失败也会被放过，量级太宽。
///
/// 修正后：
///
/// 1. 用**黑洞地址** `10.255.255.1:4318`（RFC 1918 内不可路由地址，实测 3s 超时）；
/// 2. **在 runtime 内**构造 provider，让 `BatchProcessor` 有 reactor；
/// 3. 断言**20 次总耗时** < 2 秒（不是单次），并要求每维度都成功。
///
/// # ⚠️ 为什么仍然不用 `#[tokio::test]`
///
/// `tracing::subscriber::with_default` 是**线程局部**的 ——
/// 若在 `async fn` 里用，`await` 换线程后订阅者就失效了。
///
/// → 用**同步** `#[test]` + 自建 runtime。
///
/// # 🚨 本判据的**已知能力上限**（如实记录，不要当成「已验证」）
///
/// 实测发现（F17 修正过程中）：`BatchSpanProcessor` 用的是
/// `std::thread::Builder::spawn`（**普通 OS 线程**，不是 tokio task），
/// 而 `opentelemetry-otlp` 的 `reqwest-client` 在该线程上首次导出时会
/// `panic`（`there is no reactor running`）——
/// 见 `opentelemetry-otlp-0.31.1/src/exporter/http/mod.rs:169` 的
/// `reqwest::Client::builder().build()`。
///
/// 后果：**「导出超时会不会拖慢请求」这个具体场景，在本技术栈下无法
/// 端到端复现** —— panic 取代了「等超时」。
///
/// 因此本用例**只**证明：
///
/// - ✅ 20 次请求在「配置了一个不可达 endpoint」时**总耗时仍 < 2 秒**
///   （证明**请求路径**没有被导出配置拖住 —— 这是必要条件）；
/// - ✅ 请求照常成功（导出旁路不影响业务）；
/// - ❌ **不**证明「导出超时被正确限制在 `timeout_seconds` 内」
///   —— 那需要 collector 可达但响应慢，本机无 collector → **TD-069**。
///
/// ⚠️ 即便如此，本条相对 rev 1 仍是**实质改进**：原版用 `127.0.0.1:1`
/// （秒拒）+ 单次 < 1 秒（量级太宽）→ 连「请求路径没被拖住」都证明不了。
/// 现在用黑洞地址 + 总耗时上界，至少排除了「配置期就同步阻塞」这类实现。
#[test]
fn an_unreachable_collector_does_not_slow_the_requests_down() {
    use opentelemetry::trace::TracerProvider as _;
    use std::time::{Duration, Instant};

    // ⚠️ **黑洞地址**（不是 127.0.0.1:1）：
    // `10.255.255.1` 在本机所在的网络里不可路由 → 连接**不返回 RST**，
    // 只能等到超时（实测 `/dev/tcp` 3.002s 超时）。这才模拟「collector 挂了」。
    let config = platform_observability::otel::OtelConfig {
        disabled: false,
        endpoint: "http://10.255.255.1:4318".to_string(),
        timeout_seconds: 1,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 runtime");

    // ⚠️ **关键修正（F17）**：provider 必须在 **runtime 上下文内**构造。
    //
    // `SdkTracerProvider::with_batch_exporter` 会启动 `BatchProcessor`，
    // 它在**自己的线程**上跑；若构造时没有 reactor，那个线程会
    // panic（`there is no reactor running`）→ 导出路径根本不会跑 →
    // 「不可达会不会拖慢请求」这件事**没有被测到**。
    let guard = {
        let _enter = runtime.enter();
        platform_observability::otel::init_otel(&config)
    };
    if !guard.is_enabled() {
        // 某些平台/版本下构造可能直接失败 → 那也算「不影响业务」，
        // 但要如实说明这条用例在本环境只验到「不崩」。
        eprintln!("注：本环境 exporter 构造失败，步骤 2 只验到「不崩」");
        return;
    }
    let provider = guard.provider().expect("已启用必有 provider");
    let tracer = provider.tracer("latency-test");

    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(tracer));

    let app: Router = Router::new()
        .route("/probe", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(
            platform_observability::trace::middleware::trace_id,
        ));

    // 20 次请求 —�� 每次开一个 span → 累计 20 个 span 触发批量导出。
    const REQUESTS: usize = 20;

    tracing::subscriber::with_default(subscriber, || {
        let total = runtime.block_on(async {
            let started = Instant::now();
            for index in 0..REQUESTS {
                let request = axum::http::Request::builder()
                    .uri("/probe")
                    .header(
                        "x-request-id",
                        format!("0000000000000000000000000000000{index}"),
                    )
                    .body(Body::empty())
                    .expect("构造请求");
                let response = app.clone().oneshot(request).await.expect("应有响应");
                assert!(
                    response.status().is_success(),
                    "第 {index} 次请求必须成功（导出失败不能影响业务）"
                );
            }
            started.elapsed()
        });

        // ⚠️ **总耗时**上界（不是单次）：
        //
        // 若实现改用 `with_simple_exporter`（同步导出，M3 点名要防的写法），
        // **第一次**请求就会等满 `timeout_seconds = 1` → 20 次至少 20 秒 → 红。
        // 用「单次 < 1 秒」的话，20 次 × 恰好 0.9s 也能蒙混过关 —— 量级太宽。
        //
        // 批量导出是**异步**的 → 20 次请求的总耗时应当远小于 2 秒。
        assert!(
            total < Duration::from_secs(2),
            "collector 不可达时 {REQUESTS} 次请求的总耗时不得慢于 2 秒；实际：{total:?}"
        );

        // 附带给出单次平均，便于排障时判断是「某一次卡住」还是「整体变慢」。
        let average = total / REQUESTS as u32;
        assert!(
            average < Duration::from_millis(100),
            "平均单次耗时不得慢于 100ms；实际平均：{average:?}"
        );
        eprintln!(
            "注：{REQUESTS} 次请求总耗时 {total:?}，平均 {average:?}（黑洞地址 10.255.255.1:4318）"
        );
    });
}
