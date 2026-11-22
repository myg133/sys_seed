//! `trace_id` 中间件（REQ-019，AC-S-029 / AC-S-030）。
//!
//! # 职责（四件事，缺一不可）
//!
//! 1. 读入站 `X-Request-Id` → **校验** → 合法则沿用、非法/缺失则生成；
//! 2. 把结果写进 `RequestContext`（后续所有子系统的日志/审计都用它）；
//! 3. **开一个 tracing span**，把 `trace_id` 作为字段 → 该请求期间**所有**日志自动带它；
//! 4. **回写**响应头 `X-Request-Id`（成功与失败都写）。
//!
//! # ⚠️ 为什么必须在 `client_info` 之后、其它中间件之前
//!
//! `trace_id` 要「贯穿」—— 后续每个中间件打的日志都得带上它。
//! 若挂在里面（更靠近业务），外层中间件的日志就没有 trace_id 了。
//!
//! # ⚠️ 安全：非法值**绝不**回显
//!
//! 不校验就会形成**日志注入**：`X-Request-Id: <32 个 a>\r\nFAKE LOG LINE`
//! 会被原样写进日志（伪造日志行）。
//! 校验（AC-S-030）与**不回显**（AC-S-030 步骤 4/5）是两件不同的事，都要做。

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use platform_core::RequestContext;

use crate::trace::resolve_trace_id;

/// 入站头名（沿用既有约定）。
pub const HEADER_NAME: &str = "x-request-id";

/// trace 中间件。
pub async fn trace_id(request: Request, next: Next) -> Response {
    // ① 取入站值（**只读一次**，后面不再碰原始头）。
    let incoming = request
        .headers()
        .get(HEADER_NAME)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    // ② 校验 / 生成。非法值**在这里被丢弃** → 后续任何地方都拿不到它（不回显）。
    let resolved = resolve_trace_id(incoming.as_deref());

    // ③ 写进上下文（若已有上下文，**替换**它的 trace_id；否则新建一个）。
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);
    ctx = ctx.with_trace_id(resolved.clone());

    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(ctx.clone());
    let request = Request::from_parts(parts, body);

    // ④ 开 span：`trace_id` 作为字段 → 该请求内所有日志自动继承。
    //
    // ⚠️ 用 `tracing::info_span!` 而不是 `#[instrument]` —— 后者要显式列参数，
    // 而这里只需要一个字段。`span.enter()` 在 async 里不安全，
    // 用 `Instrument::instrument` 让它跟随 future。
    let span = tracing::info_span!(
        "http.request",
        trace_id = %resolved,
        method = %request.method(),
        path = %request.uri().path(),
    );

    use tracing::Instrument;
    let mut response = next.run(request).instrument(span).await;

    // ⑤ 回写响应头（**成功与失败都写** —— AC-S-030 步骤 5 要求 2xx 与 4xx 都有）。
    //
    // ⚠️ 这里写的是**已校验过**的值（`resolved`），**不是**入站原值 ——
    // 入站非法值在步骤 ①/② 就被丢弃了，永远不会出现在响应头里。
    if let Ok(value) = HeaderValue::from_str(&resolved) {
        response.headers_mut().insert(
            HeaderName::from_static(HEADER_NAME),
            value,
        );
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::TRACE_ID_HEX_LEN;

    /// 中间件用的头名与常量一致（防拼错）。
    #[test]
    fn the_header_name_is_lowercase_x_request_id() {
        assert_eq!(HEADER_NAME, "x-request-id");
        // `HeaderName` 只接受小写，这条保证 `from_static` 不会 panic。
        assert!(HEADER_NAME.chars().all(|c| !c.is_ascii_uppercase()));
    }

    /// 校验与生成本身的语义在 `trace` 模块的测试里；
    /// 这里只验「中间件依赖的那两个函数能组合出正确结果」。
    #[test]
    fn a_valid_header_value_survives_the_resolve_step() {
        let raw = "0123456789abcdef0123456789abcdef";
        assert_eq!(resolve_trace_id(Some(raw)), raw);
        assert_eq!(raw.len(), TRACE_ID_HEX_LEN);
    }

    /// 非法值在 resolve 阶段被替换（不会进入响应头）。
    #[test]
    fn an_invalid_header_value_is_replaced_before_it_can_be_echoed() {
        let bad = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\nFAKE LOG LINE";
        let resolved = resolve_trace_id(Some(bad));
        assert_ne!(resolved, bad, "非法值必须被替换，不能原样返回");
        assert!(!resolved.contains("FAKE"), "替换后的值不得含注入内容");
        assert_eq!(resolved.len(), TRACE_ID_HEX_LEN);
    }
}
