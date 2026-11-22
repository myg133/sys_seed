//! HTTP 层的契约测试（REQ-019，AC-S-034 / AC-S-036）。
//!
//! # 覆盖
//!
//! - **AC-S-034**：分页契约（`items` / `nextCursor` / `total` + `limit` 越界 400）
//! - **AC-S-036**：弃用头（**正向 + 阴性对照**）
//!
//! # ⚠️ 这些测试**不经过** `main.rs` 的完整装配
//!
//! 它们用 `platform_observability` 自己的 router + 中间件 ——
//! 这验证的是「中间件/router 本身的语义」，**不**验证「main 挂对了」。
//!
//! ⚠️ 「main 挂对了」需要挑刺 M5 的 `build_router` 抽取（**未做**）——
//! 那一步没做之前，这几条测试**不能**声称覆盖了生产装配路径。
//! 如实记录在 `status.md`。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use platform_observability::deprecation::{Deprecation, SUNSET_HEADER, apply_deprecation_router};
use platform_observability::http::{Page, RateLimitRow, StatusState};

/// 发一个请求（可带头），返回（状态码, 头, 体）。
async fn send_with(
    app: axum::Router,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
    let mut builder = Request::builder().uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder.body(Body::empty()).expect("构造请求");
    let response = app.oneshot(request).await.expect("应有响应");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("读体")
        .to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, headers, value)
}

/// 发一个不带头的请求（大多数用例用这个）。
async fn send(
    app: axum::Router,
    uri: &str,
) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
    let request = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("构造请求");
    let response = app.oneshot(request).await.expect("应有响应");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("读体")
        .to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, headers, value)
}

/// 构造一个「没有限流器」的可观测性 router（端点仍可用，返回空列表）。
fn status_router() -> axum::Router {
    platform_observability::http::router(Arc::new(StatusState { rate_limit: None }))
}

// ---------------------------------------------------------------------------
// AC-S-034：分页契约
// ---------------------------------------------------------------------------

/// **TC-019-043**：响应**含** `items` / `nextCursor` / `total`（D194 的「包含」）。
#[tokio::test]
async fn the_pagination_envelope_has_the_three_required_fields() {
    let (status, _, body) = send(status_router(), "/rate-limit/status").await;
    assert_eq!(status, StatusCode::OK);
    for field in ["items", "nextCursor", "total"] {
        assert!(
            body.get(field).is_some(),
            "响应必须含 `{field}`；实际：{body}"
        );
    }
}

/// **TC-019-044**：字段是 **camelCase**（不得出现 `next_cursor`）。
#[tokio::test]
async fn the_envelope_uses_camel_case() {
    let (_, _, body) = send(status_router(), "/rate-limit/status").await;
    assert!(
        body.get("next_cursor").is_none(),
        "不得出现 snake_case 的 next_cursor"
    );
    assert!(body.get("nextCursor").is_some());
}

/// **TC-019-039**：`limit=1000` → **400**（不再静默截断，D341）。
#[tokio::test]
async fn an_oversized_limit_is_rejected_with_400() {
    let (status, _, body) = send(status_router(), "/rate-limit/status?limit=1000").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "超上限必须 400");
    assert_eq!(
        body.get("code").and_then(|v| v.as_str()),
        Some("INVALID_REQUEST"),
        "码必须是 INVALID_REQUEST（平台级通用码）"
    );
    // ⚠️ detail 必须含上限与实际值（运维要能一眼看出差多少）。
    let detail = body.get("detail").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        detail.contains("200") && detail.contains("1000"),
        "detail 必须含上限与实际值：{detail}"
    );
}

/// **TC-019-038**：`limit=0` → 400。
#[tokio::test]
async fn a_zero_limit_is_rejected_with_400() {
    let (status, _, body) = send(status_router(), "/rate-limit/status?limit=0").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body.get("code").and_then(|v| v.as_str()),
        Some("INVALID_REQUEST")
    );
}

/// **TC-019-040**：`limit=200`（正好上限）→ 200（边界包含）。
#[tokio::test]
async fn a_limit_equal_to_the_maximum_is_accepted() {
    let (status, _, _) = send(status_router(), "/rate-limit/status?limit=200").await;
    assert_eq!(status, StatusCode::OK, "正好 200 必须通过（不是 >=）");
}

/// 非数字 → 400。
#[tokio::test]
async fn a_non_numeric_limit_is_rejected() {
    let (status, _, _) = send(status_router(), "/rate-limit/status?limit=abc").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 分页信封的类型能被反序列化（结构与 `Page<T>` 一致）。
#[tokio::test]
async fn the_envelope_deserializes_into_the_page_type() {
    let (_, _, body) = send(status_router(), "/rate-limit/status").await;
    let page: Page<RateLimitRow> =
        serde_json::from_value(body).expect("必须能反序列化成 Page<RateLimitRow>");
    assert_eq!(page.total, page.items.len() as i64, "空列表时 total=0");
    assert!(page.next_cursor.is_none(), "单页时 nextCursor 为 null");
}

// ---------------------------------------------------------------------------
// AC-S-036：弃用头
// ---------------------------------------------------------------------------

/// 构造一个「一个端点弃用、一个不弃用」的 router。
fn deprecated_router() -> axum::Router {
    let router = axum::Router::new()
        .route("/old", axum::routing::get(|| async { "old" }))
        .route("/fresh", axum::routing::get(|| async { "fresh" }));
    apply_deprecation_router(
        router,
        vec![(
            "/old".to_string(),
            Deprecation::new(time::macros::datetime!(2027-06-30 00:00:00 UTC)),
        )],
    )
}

/// **TC-019-052 / 053**：弃用端点带 `Deprecation: true` 与合法 `Sunset`。
#[tokio::test]
async fn the_deprecated_endpoint_carries_both_headers() {
    let (status, headers, _) = send(deprecated_router(), "/old").await;
    // **TC-019-055**：弃用 ≠ 下线 —— 仍然 200。
    assert_eq!(status, StatusCode::OK, "弃用端点必须仍然可用（不是 410）");

    assert_eq!(
        headers.get("deprecation").and_then(|v| v.to_str().ok()),
        Some("true"),
        "Deprecation 必须是字面量 true（D342）"
    );

    let sunset = headers
        .get(SUNSET_HEADER)
        .and_then(|v| v.to_str().ok())
        .expect("必须有 Sunset");
    // **TC-019-053**：严格 IMF-fixdate（用 time 的形状反解验证）。
    let format = time::format_description::parse(
        "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT",
    )
    .expect("描述串合法");
    assert!(
        time::PrimitiveDateTime::parse(sunset, &format).is_ok(),
        "Sunset 必须是严格 IMF-fixdate（RFC 8594）；实际：{sunset}"
    );
}

/// **TC-019-054**：未弃用端点**两个头都不出现**（阴性对照）。
#[tokio::test]
async fn the_fresh_endpoint_carries_neither_header() {
    let (status, headers, _) = send(deprecated_router(), "/fresh").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers.get("deprecation").is_none(),
        "未弃用端点不得带 Deprecation（阴性对照）"
    );
    assert!(
        headers.get(SUNSET_HEADER).is_none(),
        "未弃用端点不得带 Sunset（阴性对照）"
    );
}

/// 弃用是**按路径精确匹配**：子路径不继承。
#[tokio::test]
async fn a_subpath_does_not_inherit_the_deprecation() {
    let router = axum::Router::new()
        .route("/old", axum::routing::get(|| async { "old" }))
        .route("/old/extra", axum::routing::get(|| async { "extra" }));
    let router = apply_deprecation_router(
        router,
        vec![(
            "/old".to_string(),
            Deprecation::new(time::macros::datetime!(2027-06-30 00:00:00 UTC)),
        )],
    );
    let (_, headers, _) = send(router, "/old/extra").await;
    assert!(
        headers.get("deprecation").is_none(),
        "子路径不该继承弃用标记（精确匹配）"
    );
}

/// 空的弃用清单 → **不挂中间件**（零开销）。
#[tokio::test]
async fn an_empty_deprecation_list_adds_no_headers() {
    let router =
        apply_deprecation_router(axum::Router::new().route("/x", axum::routing::get(|| async { "x" })), Vec::new());
    let (status, headers, _) = send(router, "/x").await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("deprecation").is_none());
}

// ---------------------------------------------------------------------------
// AC-S-030：trace_id 回写（HTTP 层）
// ---------------------------------------------------------------------------

/// 中间件回写 `X-Request-Id`，且非法值被替换。
#[tokio::test]
async fn the_trace_middleware_echoes_a_valid_header_and_replaces_an_invalid_one() {
    let app: axum::Router = axum::Router::new()
        .route("/probe", axum::routing::get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(
            platform_observability::trace::middleware::trace_id,
        ));

    // 合法 → 逐字沿用。
    let valid = "0123456789abcdef0123456789abcdef";
    let request = Request::builder()
        .uri("/probe")
        .header("x-request-id", valid)
        .body(Body::empty())
        .expect("构造");
    let response = app.clone().oneshot(request).await.expect("响应");
    assert_eq!(
        response
            .headers()
            .get(header::HeaderName::from_static("x-request-id"))
            .and_then(|v| v.to_str().ok()),
        Some(valid),
        "合法值必须逐字沿用"
    );

    // 非法（长度对、非 hex）→ 新生成。
    let bad = "gggggggggggggggggggggggggggggggg";
    let request = Request::builder()
        .uri("/probe")
        .header("x-request-id", bad)
        .body(Body::empty())
        .expect("构造");
    let response = app.oneshot(request).await.expect("响应");
    let echoed = response
        .headers()
        .get(header::HeaderName::from_static("x-request-id"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert_ne!(echoed, bad, "非法值不得回显");
    assert_eq!(echoed.len(), 32, "必须是新生成的 32 hex");
}

// ---------------------------------------------------------------------------
// AC-S-034：**跨 crate** 字段名一致（本 AC 的独特判据）
// ---------------------------------------------------------------------------

/// **TC-019-043**：新端点与 `platform-audit` 的**分页字段语义对应**。
///
/// # 为什么这条判据必须存在
///
/// 「契约统一」不是「两个端点各自有自己的分页」——
/// 而是**调用方能用同一套字段名解析响应**。
/// 只测一个端点证明不了统一（挑刺 B5 指出 rev 1 的设计里甚至没有第二个端点）。
///
/// # ⚠️ 这条断言的能力边界（如实说）
///
/// `platform_audit::AuditPage` **没有实现 `Serialize`**（序列化在
/// `platform-server::audit` 的独立响应类型里做）。
/// → 无法在**运行时**对比两个 JSON。
///
/// 这里做的是**编译期**断言：两个类型都有 `items` / `total` / `next_cursor`
/// 三个**同名字段**（改名字会编译失败 → 这条红）。
/// 序列化后的 camelCase 形态（`nextCursor`）由上面
/// [`the_envelope_uses_camel_case`] 覆盖。
#[test]
fn the_pagination_envelope_matches_the_audit_field_names() {
    // 新端点的信封键集合（运行时序列化）。
    let ours = serde_json::to_value(Page {
        items: Vec::<RateLimitRow>::new(),
        next_cursor: None,
        total: 0,
    })
    .expect("序列化新端点信封");
    let our_keys: std::collections::BTreeSet<&str> = ours
        .as_object()
        .expect("应是对象")
        .keys()
        .map(String::as_str)
        .collect();
    for field in ["items", "nextCursor", "total"] {
        assert!(
            our_keys.contains(field),
            "新端点缺 `{field}`；实际：{our_keys:?}"
        );
    }
    assert!(
        !our_keys.contains("next_cursor"),
        "不得出现 snake_case（D258）"
    );

    // audit 侧的字段名（**编译期**断言：字段不存在或改名 → 编译失败）。
    let audit = platform_audit::AuditPage {
        items: Vec::new(),
        total: 0,
        next_cursor: None,
    };
    let audit_items_len = audit.items.len();
    let audit_total = audit.total;
    let audit_has_no_next = audit.next_cursor.is_none();
    assert_eq!(audit_items_len, 0);
    assert_eq!(audit_total, 0);
    assert!(audit_has_no_next);
}

// ---------------------------------------------------------------------------
// AC-S-029 第三处：**失败响应体**里的 trace_id
// ---------------------------------------------------------------------------

/// **AC-S-029**：失败响应体（`ProblemDetails`）的 `trace_id` **等于** 响应头。
///
/// # 为什么这条必须单独验
///
/// AC-S-029 要求「日志 / 响应体 / span **三处同一个值**」。
/// 另外两处（日志、span）已有判据，**这一条补上第三处**：
/// 用户拿着错误响应里的 `trace_id` 去找日志时，必须**真能对上**。
///
/// ⚠️ 若中间件只把 trace_id 写进**头**、没写进 `RequestContext` 的
/// `ProblemDetails`，这条会红 —— 那正是「看起来贯穿了、实际查不到」的形态。
#[tokio::test]
async fn the_error_body_carries_the_same_trace_id_as_the_header() {
    // 一个总是返回 400 + ProblemDetails 的 handler（模拟业务侧的错误响应）。
    async fn always_fails(request: axum::extract::Request) -> axum::response::Response {
        use axum::response::IntoResponse;
        let ctx = request
            .extensions()
            .get::<platform_core::RequestContext>()
            .cloned()
            .unwrap_or_else(platform_core::RequestContext::new);
        let problem = platform_core::problem::ProblemDetails::from_context(
            "INVALID_REQUEST",
            platform_core::titles::title_of_code("INVALID_REQUEST"),
            400,
            "the request is invalid",
            &ctx,
            "/fails",
            None,
        );
        (axum::http::StatusCode::BAD_REQUEST, axum::Json(problem)).into_response()
    }

    let app: axum::Router = axum::Router::new()
        .route("/fails", axum::routing::get(always_fails))
        .layer(axum::middleware::from_fn(
            platform_observability::trace::middleware::trace_id,
        ));

    let incoming = "0123456789abcdef0123456789abcdef";
    let (status, headers, body) = send_with(app, "/fails", &[("x-request-id", incoming)]).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        headers.get("x-request-id").and_then(|v| v.to_str().ok()),
        Some(incoming),
        "响应头必须是入站值"
    );
    // ⚠️ **第三处**：响应体里的 trace_id 必须与头**逐字相同**。
    assert_eq!(
        body.get("trace_id").and_then(|v| v.as_str()),
        Some(incoming),
        "失败响应体的 trace_id 必须与响应头相同（否则用户拿它查不到日志）；实际：{body}"
    );
}
