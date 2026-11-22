//! 日志侧的 trace_id 验证（REQ-019，AC-S-029 步骤 4）。
//!
//! # 这条测试补什么
//!
//! `otel_in_memory.rs` 证明了「span 的 trace_id == 入站值」；
//! 这里补另一半：**日志行里真的带 `trace_id=<T>`**。
//!
//! 两者都成立，「贯穿」才算完整 —— 只有 span 没有日志，
//! 运维 `grep` 时还是找不到那条请求。
//!
//! # ⚠️ 为什么不用 `#[tokio::test]`
//!
//! `tracing::subscriber::with_default` 是**线程局部**的 ——
//! 若在 async fn 里用，`await` 换线程后订阅者就失效了。
//!
//! → 用**同步**测试 + 自建 `current_thread` runtime：
//! `with_default` 包住整个 `block_on`，请求全程在同一个线程。

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;

/// 一个可克隆的内存 writer（把日志收进 String）。
#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("锁").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buf {
    type Writer = Buf;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// **AC-S-029 步骤 4**：日志行里带 `trace_id=<T>`（**带等号的具体形态**）。
#[test]
fn the_log_line_carries_the_resolved_trace_id() {
    let buffer = Buf::default();
    let incoming = "0123456789abcdef0123456789abcdef";

    // ⚠️ 用 `platform-core` 的 `build_subscriber`（生产与测试共用，挑刺 M4）。
    let subscriber = platform_core::log::build_subscriber("info", buffer.clone());

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 runtime");

    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            let app: Router = Router::new()
                // ⚠️ handler 里**主动打一条日志** —— 中间件开的 span 会让它自动带 trace_id。
                .route(
                    "/probe",
                    get(|| async {
                        tracing::info!("handling the probe request");
                        "ok"
                    }),
                )
                .layer(axum::middleware::from_fn(
                    platform_observability::trace::middleware::trace_id,
                ));

            let request = axum::http::Request::builder()
                .uri("/probe")
                .header("x-request-id", incoming)
                .body(Body::empty())
                .expect("构造请求");
            let response = app.oneshot(request).await.expect("应有响应");
            assert_eq!(response.status(), 200);
        });
    });

    let logs = String::from_utf8(buffer.0.lock().expect("锁").clone()).expect("UTF-8");

    // ⚠️ 断言**带等号的具体形态** —— 只断言 `logs.contains(incoming)` 的话，
    // 「日志里恰好有个 32 hex 串」也会绿（那不是 trace 贯通）。
    assert!(
        logs.contains(&format!("trace_id={incoming}")),
        "日志必须带 `trace_id={incoming}`；实际日志：\n{logs}"
    );
    assert!(
        logs.contains("handling the probe request"),
        "handler 的日志本身必须出现（否则上面的断言可能命中的是别处）；实际：\n{logs}"
    );
}

/// **AC-S-030 步骤 4/5 之②：非法值不得出现在日志里**（F12 的判据）。
///
/// # ⚠️ 这条防的是什么
///
/// 「**日志注入**」是与「路径穿越」并列的独立攻击向量（`acceptance.md:97-98`）：
/// 调用方把 `X-Request-Id` 设成 `FAKE LOG LINE`（或带 CRLF）→
/// 若实现把它原样打进日志，日志里就会出现一条**伪造的行**，
/// 运维 `grep` 时会被误导、日志解析器会多出一条假记录。
///
/// # 为什么必须**端到端**验（不能只验 `normalize_trace_id`）
///
/// `trace.rs` 的单测证明「非法值被拒」—— 但**没有**证明
/// 「中间件不会把原始值另外打一遍日志」。
/// 若将来有人在 `middleware.rs` 里加
/// `tracing::debug!(raw = %incoming)`（排障时很容易这么做），
/// 单测**照样全绿**，而注入成立。
///
/// → 本用例让非法值真的穿过中间件，然后断言**整段日志里不含该串**。
#[test]
fn an_illegal_incoming_trace_id_never_reaches_the_logs() {
    let buffer = Buf::default();
    // ⚠️ 三种非法向量各验一遍（AC-S-030 步骤 4 + 步骤 5）：
    // ① 路径穿越；② 日志注入（独立成行）；③ 大写 + 非 hex 的混合垃圾。
    let illegal = [
        "../../etc/passwd",
        "FAKE LOG LINE",
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
    ];

    let subscriber = platform_core::log::build_subscriber("info", buffer.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 runtime");

    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            for (index, bad) in illegal.iter().enumerate() {
                let app: Router = Router::new()
                    .route(
                        &format!("/probe{index}"),
                        get(|| async {
                            tracing::info!("handling a probe with an illegal trace id");
                            "ok"
                        }),
                    )
                    // ⚠️ 每个请求一个 router：`MatchedPath` 需要对应路由存在。
                    .layer(axum::middleware::from_fn(
                        platform_observability::trace::middleware::trace_id,
                    ));
                let request = axum::http::Request::builder()
                    .uri(format!("/probe{index}"))
                    .header("x-request-id", *bad)
                    .body(Body::empty())
                    .expect("构造请求");
                let response = app.oneshot(request).await.expect("应有响应");
                // 非法入站值**不影响**请求成功（只是被替换成一个新 trace_id）。
                assert_eq!(
                    response.status(),
                    200,
                    "非法 trace_id 不得让请求失败（{bad}）"
                );
                // ① 响应头也不能回显非法值（AC-S-030 的「三处」之一）。
                let header = response
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .expect("必须有 X-Request-Id");
                assert_ne!(header, *bad, "响应头不得回显非法值：{bad}");
            }
        });
    });

    let logs = String::from_utf8(buffer.0.lock().expect("锁").clone()).expect("UTF-8");

    // ① 前置自我校验：日志**确实**有内容（否则下面的「不含」恒真、毫无意义）。
    assert!(
        logs.contains("handling a probe with an illegal trace id"),
        "前置：handler 的日志必须出现，否则「不含非法串」是空断言；实际：\n{logs}"
    );

    // ② 三处非法串**逐处**不得出现（AC-S-030 的「逐处断言」）。
    for bad in illegal {
        assert!(
            !logs.contains(bad),
            "非法 trace_id `{bad}` 出现在日志里 —— 日志注入成立！实际日志：\n{logs}"
        );
    }

    // ③ `FAKE LOG LINE` **不得成为独立一行**（日志注入的经典判据）。
    assert!(
        !logs.lines().any(|line| line.trim() == "FAKE LOG LINE"),
        "`FAKE LOG LINE` 不得成为独立日志行；实际日志：\n{logs}"
    );

    // ④ 日志里仍然有**合法形态**的 trace_id（证明「替换」而非「丢弃」）。
    assert!(
        logs.contains("trace_id="),
        "日志仍须带一个合法的 trace_id；实际：\n{logs}"
    );
}

/// **AC-S-030 步骤 4/5 之③：失败响应体里的 `.trace_id` 也不得是非法值**（F12）。
///
/// # 为什么单独一条
///
/// 上面那条验的是**成功**路径（handler 返回 200）。失败响应体是**另一条**代码路径
/// （`ProblemDetails` 的 `.trace_id` 字段），实现里可能各写一遍。
/// AC 明文要求「**三处**逐处断言」，这是第三处。
#[test]
fn an_illegal_incoming_trace_id_never_reaches_the_error_body() {
    let buffer = Buf::default();
    let bad = "FAKE LOG LINE";
    let subscriber = platform_core::log::build_subscriber("info", buffer.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 runtime");

    let (status, header_value, body) = tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            let app: Router = Router::new()
                // ⚠️ handler 主动返回 400 → 走**失败**路径。
                .route(
                    "/fails",
                    get(|| async {
                        (
                            axum::http::StatusCode::BAD_REQUEST,
                            axum::Json(serde_json::json!({ "problem": "probe" })),
                        )
                    }),
                )
                .layer(axum::middleware::from_fn(
                    platform_observability::trace::middleware::trace_id,
                ));
            let request = axum::http::Request::builder()
                .uri("/fails")
                .header("x-request-id", bad)
                .body(Body::empty())
                .expect("构造请求");
            let response = app.oneshot(request).await.expect("应有响应");
            let status = response.status();
            let header_value = response
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .expect("读体");
            (status, header_value, String::from_utf8_lossy(&bytes).to_string())
        })
    });

    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    // ① 失败响应体**不得**含非法值。
    assert!(
        !body.contains(bad),
        "失败响应体不得含非法 trace_id `{bad}`；实际：{body}"
    );
    // ② 失败响应头**不得**回显非法值。
    assert_ne!(header_value, bad, "失败响应头不得回显非法值");
    // ③ 头里的值必须是**合法形态**（32 hex）—— 证明是「替换」而不是「留空」。
    assert_eq!(
        header_value.len(),
        32,
        "失败响应的 X-Request-Id 必须是 32 hex；实际：{header_value:?}"
    );
    assert!(
        header_value.bytes().all(|b| b.is_ascii_hexdigit()),
        "必须是 hex：{header_value}"
    );
}
#[test]
fn a_generated_trace_id_also_appears_in_the_logs() {
    let buffer = Buf::default();
    let subscriber = platform_core::log::build_subscriber("info", buffer.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 runtime");

    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            let app: Router = Router::new()
                .route(
                    "/probe",
                    get(|| async {
                        tracing::info!("no incoming header");
                        "ok"
                    }),
                )
                .layer(axum::middleware::from_fn(
                    platform_observability::trace::middleware::trace_id,
                ));
            let request = axum::http::Request::builder()
                .uri("/probe")
                .body(Body::empty())
                .expect("构造请求");
            app.oneshot(request).await.expect("响应");
        });
    });

    let logs = String::from_utf8(buffer.0.lock().expect("锁").clone()).expect("UTF-8");
    // 提取 `trace_id=...` 的值，断言它是 32 hex。
    let value = logs
        .split("trace_id=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default();
    assert_eq!(
        value.len(),
        32,
        "日志里的 trace_id 必须是 32 hex；实际：{value:?}\n完整日志：\n{logs}"
    );
    assert!(
        value.bytes().all(|b| b.is_ascii_hexdigit()),
        "必须是 hex：{value}"
    );
}
