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

/// 没有入站头时，日志里带的是**新生成**的 32 hex（不是空）。
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
