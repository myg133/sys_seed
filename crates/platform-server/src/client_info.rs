//! 全局中间件：把来源 IP 与设备填进 `RequestContext`（REQ-014 D149）。
//!
//! # 为什么放在**顶层 Router**
//!
//! 实测（棒 1 §4 第 9 条）：各子系统**各自**建上下文
//! —— `platform-config/src/http.rs:179`、`platform-rbac/src/http.rs:215`、
//! `platform-server/src/i18n.rs:794`、`platform-server/src/main.rs:656`
//! （`authenticate_module`，而且它**只**挂在 `/api/sandbox`）。
//! → 「对所有路由（含 `/api/iam`）生效」只能在**顶层**加一个中间件。
//!
//! # 它怎么与各子系统共存
//!
//! 中间件把 `RequestContext` **放进请求扩展**；各子系统的中间件随后
//! `extensions().get::<RequestContext>().cloned()` **继承**它
//! （它们的既有代码就是这么写的 —— 棒 1 实测）。
//!
//! ⚠️ 子系统中间件会用**令牌**覆盖租户 / 用户，但**不会**覆盖 IP / UA
//! → 继承成立。
//!
//! # ⚠️ `ip` 是**客户端可控的头**，**不是**可信的物理来源（D149）
//!
//! 它取 `x-forwarded-for` 的**第一跳**（与既有惯例
//! `crates/platform-iam/src/http.rs:264-274` 一致）。
//! 本仓**无法**记录可信的物理来源 —— 它的用途是**关联与排查**，
//! **不是取证**。代码注释与规格都**不得**把它写成「真实客户端地址」。
//!
//! 取不到 → 落**空串**（审计记录里不得缺键，AC-S-122）。

use axum::extract::Request;
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use platform_core::RequestContext;

/// `x-forwarded-for` 的头名。
const FORWARDED_FOR_HEADER: &str = "x-forwarded-for";
/// `user-agent` 的头名。
const USER_AGENT_HEADER: &str = "user-agent";

/// 全局中间件：填 `client_ip` / `user_agent` → 放进请求扩展。
///
/// ⚠️ **不覆盖**已有的上下文 —— 它只**补**两个字段。
pub async fn client_info(mut request: Request, next: Next) -> Response {
    let ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    let ip = first_forwarded_for(request.headers()).unwrap_or_default();
    let user_agent = header_value(request.headers(), USER_AGENT_HEADER).unwrap_or_default();

    let ctx = ctx.with_client_ip(ip).with_user_agent(user_agent);
    request.extensions_mut().insert(ctx);
    next.run(request).await
}

/// `x-forwarded-for` 的**第一跳**。
///
/// ⚠️ 该头**由客户端控制** —— 只作关联与排查，**不是取证证据**（D149）。
pub fn first_forwarded_for(headers: &HeaderMap) -> Option<String> {
    header_value(headers, FORWARDED_FOR_HEADER)
        .and_then(|value| value.split(',').next().map(str::trim).map(str::to_string))
        .filter(|value| !value.is_empty())
}

/// 读一个请求头（去空白；空串视为没有）。
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)?
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("头名应当合法"),
                value.parse().expect("头值应当合法"),
            );
        }
        map
    }

    /// `x-forwarded-for` 取**第一跳**（既有惯例）。
    #[test]
    fn the_forwarded_for_takes_the_first_hop() {
        let map = headers(&[("x-forwarded-for", "203.0.113.9, 10.0.0.1, 10.0.0.2")]);
        assert_eq!(
            first_forwarded_for(&map).as_deref(),
            Some("203.0.113.9"),
            "必须取第一跳"
        );
    }

    #[test]
    fn a_missing_or_blank_forwarded_for_is_unknown() {
        assert_eq!(first_forwarded_for(&headers(&[])), None);
        assert_eq!(
            first_forwarded_for(&headers(&[("x-forwarded-for", "   ")])),
            None
        );
        assert_eq!(
            first_forwarded_for(&headers(&[("x-forwarded-for", ", 10.0.0.1")])),
            None,
            "第一跳为空 → 未知"
        );
    }

    /// 中间件把 IP / UA 填进扩展里的上下文（**真的跑一遍中间件**）。
    #[tokio::test]
    async fn the_middleware_fills_the_request_context() {
        use axum::Router;
        use axum::body::Body;
        use axum::extract::Extension;
        use axum::http::StatusCode;
        use axum::routing::get;
        use tower::ServiceExt;

        let app = Router::new()
            .route(
                "/probe",
                get(|Extension(ctx): Extension<RequestContext>| async move {
                    (
                        StatusCode::OK,
                        format!(
                            "{}|{}|{}",
                            ctx.tenant_id().unwrap_or_default(),
                            ctx.client_ip().unwrap_or_default(),
                            ctx.user_agent().unwrap_or_default()
                        ),
                    )
                }),
            )
            .layer(axum::middleware::from_fn(client_info));

        let request = Request::builder()
            .uri("/probe")
            .header("x-forwarded-for", "203.0.113.9, 10.0.0.1")
            .header("user-agent", "audit-test-agent")
            .body(Body::empty())
            .expect("应当能构造请求");
        let response = app.oneshot(request).await.expect("应当能跑完路由");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("应当能读出响应体");
        assert_eq!(
            &body[..],
            b"|203.0.113.9|audit-test-agent",
            "中间件必须把第一跳 IP 与 UA 填进上下文"
        );
    }
}
