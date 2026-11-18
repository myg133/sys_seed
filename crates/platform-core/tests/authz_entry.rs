//! 鉴权入口的**行为**测试（不连数据库）。
//!
//! 判据是**精确的错误码**（不是「非 2xx 即可」）——
//! 静默放行是最高危缺陷，断言必须精确。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use platform_core::extension::permission::{
    DataScope, PermissionChecker, PermissionLayer, ScopeFilter, require_permission,
};
use platform_core::problem::{
    CODE_INSUFFICIENT_PERMISSION, CODE_INTERNAL_ERROR, CODE_UNAUTHENTICATED,
};
use serde_json::Value;
use tower::ServiceExt;

/// 一个「什么都不给」的判定实现。
struct NoPermissions;

impl PermissionChecker for NoPermissions {
    fn has(&self, _ctx: &platform_core::RequestContext, _permission: &str) -> bool {
        false
    }

    fn scope(&self, _ctx: &platform_core::RequestContext, _scope: DataScope) -> ScopeFilter {
        ScopeFilter::All
    }

    fn effective_scope(
        &self,
        _ctx: &platform_core::RequestContext,
        _permission: &str,
    ) -> ScopeFilter {
        ScopeFilter::All
    }

    fn condition(
        &self,
        _ctx: &platform_core::RequestContext,
        _permission: &str,
    ) -> Option<platform_core::extension::expr::CompiledCondition> {
        None
    }
}

/// 一个「全部给」的判定实现。
struct AllPermissions;

impl PermissionChecker for AllPermissions {
    fn has(&self, _ctx: &platform_core::RequestContext, _permission: &str) -> bool {
        true
    }

    fn scope(&self, _ctx: &platform_core::RequestContext, _scope: DataScope) -> ScopeFilter {
        ScopeFilter::All
    }

    fn effective_scope(
        &self,
        _ctx: &platform_core::RequestContext,
        _permission: &str,
    ) -> ScopeFilter {
        ScopeFilter::All
    }

    fn condition(
        &self,
        _ctx: &platform_core::RequestContext,
        _permission: &str,
    ) -> Option<platform_core::extension::expr::CompiledCondition> {
        None
    }
}

/// 造一个请求（可选地带上已认证的上下文与判定实现）。
fn request(authenticated: bool, checker: Option<Arc<dyn PermissionChecker>>) -> Request<Body> {
    let mut request = Request::builder()
        .method("GET")
        .uri("/api/demo/read")
        .body(Body::empty())
        .expect("请求应当能构造");
    if authenticated {
        request.extensions_mut().insert(
            platform_core::RequestContext::new()
                .with_tenant("tenant-a")
                .with_user("u-1001"),
        );
    }
    if let Some(checker) = checker {
        request.extensions_mut().insert(checker);
    }
    request
}

/// 造一个带鉴权层的路由。
fn app() -> Router {
    Router::new()
        .route("/api/demo/read", get(|| async { "ok" }))
        .route_layer(require_permission("sandbox:demo:read"))
}

/// 跑一次请求，返回（状态码, 错误体）。
async fn call(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("请求应当能完成");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("响应体应当能读");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// **未认证** → 401 `UNAUTHENTICATED`（不是 403）。
#[tokio::test]
async fn an_unauthenticated_request_is_rejected_with_401() {
    let (status, body) = call(&app(), request(false, Some(Arc::new(AllPermissions)))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "未认证必须是 401");
    assert_eq!(body["code"], CODE_UNAUTHENTICATED);
    assert_eq!(body["status"], 401);
}

/// **已认证但缺权限点** → 403 `INSUFFICIENT_PERMISSION`（**精确**错误码）。
#[tokio::test]
async fn a_request_without_the_permission_is_rejected_with_403() {
    let (status, body) = call(&app(), request(true, Some(Arc::new(NoPermissions)))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "缺权限点必须是 403");
    assert_eq!(body["code"], CODE_INSUFFICIENT_PERMISSION);
    assert_eq!(body["status"], 403);
    // 契约 C6：`detail` 必须是英文 ASCII；`trace_id` 必带；`instance` 是路径。
    let detail = body["detail"].as_str().expect("detail 必须是字符串");
    assert!(detail.is_ascii(), "detail 必须是英文 ASCII：{detail}");
    assert!(
        detail.contains("sandbox:demo:read"),
        "detail 应当点出权限点：{detail}"
    );
    assert!(body["trace_id"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(body["instance"], "/api/demo/read");
}

/// **判定实现缺失** → fail closed（绝不放行）。
#[tokio::test]
async fn a_missing_checker_is_rejected_instead_of_allowing() {
    let (status, body) = call(&app(), request(true, None)).await;
    assert!(
        !status.is_success(),
        "判定实现缺失时绝不能放行（实际 {status}）"
    );
    assert_eq!(body["code"], CODE_INTERNAL_ERROR);
}

/// **有权限点** → 放行（200）。
#[tokio::test]
async fn a_request_with_the_permission_is_allowed() {
    let (status, body) = call(&app(), request(true, Some(Arc::new(AllPermissions)))).await;
    assert_eq!(status, StatusCode::OK, "有权限点必须放行：{body}");
}

/// 入口是一个**具名类型**（契约检查 C-5 按它定位定义处）。
#[test]
fn the_entry_point_is_a_named_layer() {
    let layer: PermissionLayer = require_permission("sandbox:demo:read");
    assert_eq!(layer.permission(), "sandbox:demo:read");
}

/// **合并子路由后再 `route_layer`** 时，鉴权层仍然生效。
///
/// （这正是业务模块与平台接口的写法：一组路由一个权限点。）
#[tokio::test]
async fn a_route_layer_on_a_merged_sub_router_still_rejects() {
    let read = Router::new().route("/api/demo/read", get(|| async { "ok" }));
    let write = Router::new().route("/api/demo/write", get(|| async { "ok" }));
    let app = Router::new()
        .merge(read.route_layer(require_permission("sandbox:demo:read")))
        .merge(write.route_layer(require_permission("sandbox:demo:write")));

    let (status, body) = call(&app, request(true, Some(Arc::new(NoPermissions)))).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "合并子路由后的鉴权层必须生效：{body}"
    );
}

/// `with_state` **不会**把鉴权层丢掉（平台的接口就是这么装配的）。
#[tokio::test]
async fn with_state_keeps_the_authorization_layer() {
    let read = Router::new().route("/api/demo/read", get(|| async { "ok" }));
    let write = Router::new().route("/api/demo/write", get(|| async { "ok" }));
    let app = Router::new()
        .merge(read.route_layer(require_permission("sandbox:demo:read")))
        .merge(write.route_layer(require_permission("sandbox:demo:write")))
        .with_state(());

    let (status, body) = call(&app, request(true, Some(Arc::new(NoPermissions)))).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "with_state 之后鉴权层必须仍然生效：{body}"
    );
}
