//! 元数据读取接口的**契约行为**（AC-S-078 / AC-S-079）。
//!
//! # 为什么不需要数据库
//!
//! 元数据是**界面的描述**，不读业务表。所以这一层用真 manifest（仓库里的
//! `modules/sandbox/manifest.json`）+ 测试替身的令牌校验与权限判定即可 ——
//! 「真库 + 真 RBAC」的部分由 `scripts/test-integration.sh` 后的真机验证覆盖
//! （`/api/schema` + `/api/sandbox/demo-items` 两态对比）。
//!
//! # 断言口径（rev 2）
//!
//! - 字段清单：`fields.map(key)` **深度等于**期望数组（顺序也算）；
//! - 顶层键：`Object.keys(body).sort()` **深度等于**期望集合 ——
//!   **不含** `hiddenFields` / `deniedFields` / `total`；
//! - 被剔除的字段：**作为 JSON 键**不出现（不是「作为子串不出现」——
//!   响应里必有 `label` 键，子串匹配会必然误报）。

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::future::Future;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use platform_core::RequestContext;
use platform_core::extension::expr::CompiledCondition;
use platform_core::extension::permission::{
    DataScope, PermissionChecker, PermissionRefresher, ScopeFilter,
};
use platform_schema::{
    AuthError, SchemaRegistry, SchemaState, TokenVerifier, VerifiedToken, router,
};
use serde_json::Value;
use tower::ServiceExt;

/// 令牌主体。
const USER: &str = "u-012";
/// 令牌租户。
const TENANT: &str = "tenant-a";

/// 令牌校验替身：任何非空令牌都算这个主体。
struct StubTokens;

impl TokenVerifier for StubTokens {
    fn verify(&self, _token: &str) -> Result<VerifiedToken, AuthError> {
        Ok(VerifiedToken {
            user_id: USER.to_string(),
            tenant_id: TENANT.to_string(),
        })
    }
}

/// 权限判定替身：只回答「给了哪些权限点」。
struct StubChecker {
    granted: Vec<String>,
}

impl PermissionChecker for StubChecker {
    fn has(&self, _ctx: &RequestContext, permission: &str) -> bool {
        self.granted.iter().any(|code| code == permission)
    }

    fn scope(&self, _ctx: &RequestContext, _scope: DataScope) -> ScopeFilter {
        ScopeFilter::All
    }

    fn effective_scope(&self, _ctx: &RequestContext, _permission: &str) -> ScopeFilter {
        ScopeFilter::All
    }

    fn condition(&self, _ctx: &RequestContext, _permission: &str) -> Option<CompiledCondition> {
        None
    }
}

/// 判定缓存刷新替身（同步实现，永远成功）。
struct StubRefresher;

impl PermissionRefresher for StubRefresher {
    fn ensure_fresh<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// 仓库根（`crates/platform-schema` 往上两级）。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crates/platform-schema 应有两级父目录")
        .to_path_buf()
}

/// 用仓库里的真 manifest 造一个元数据服务。
fn app(granted: &[&str]) -> Router {
    let registry = SchemaRegistry::from_manifest_root(&repo_root()).expect("仓库 manifest 应能建注册表");
    let state = Arc::new(SchemaState {
        registry: Arc::new(registry),
        tokens: Arc::new(StubTokens),
        checker: Arc::new(StubChecker {
            granted: granted.iter().map(|code| (*code).to_string()).collect(),
        }),
        permissions: Arc::new(StubRefresher),
    });
    Router::new().nest("/api/schema", router(state))
}

/// 发一个 GET（`token` 为 `None` 时不带令牌）。
async fn get(app: &Router, path: &str, token: Option<&str>) -> (StatusCode, String) {
    let mut builder = Request::builder().method("GET").uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = builder.body(Body::empty()).expect("请求应能构造");
    let response = app.clone().oneshot(request).await.expect("请求应能完成");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 256)
        .await
        .expect("响应体应能读");
    (
        status,
        String::from_utf8(bytes.to_vec()).expect("响应体是 UTF-8"),
    )
}

/// 顶层键集合（排序）。
fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("响应是对象")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// 字段键列表（顺序即声明顺序）。
fn field_keys(value: &Value) -> Vec<String> {
    value["fields"]
        .as_array()
        .expect("fields 是数组")
        .iter()
        .map(|field| field["key"].as_str().expect("字段有 key").to_string())
        .collect()
}

const ALL_FIELDS: [&str; 7] = ["id", "title", "amount", "paid", "dueAt", "ownerId", "secret"];

/// AC-S-078 步骤 1/2/3：全字段可见 → 200，形状与顺序精确。
#[tokio::test]
async fn metadata_for_a_fully_privileged_user_has_the_declared_shape() {
    let app = app(&["sandbox:demo:read", "sandbox:demo:secret"]);
    let (status, raw) = get(&app, "/api/schema/sandbox/demoItem", Some("token")).await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");

    let body: Value = serde_json::from_str(&raw).expect("响应是 JSON");
    assert_eq!(
        keys(&body),
        vec![
            "api".to_string(),
            "entity".to_string(),
            "fields".to_string(),
            "label".to_string(),
            "module".to_string()
        ],
        "顶层键集合必须精确（没有 hiddenFields / deniedFields / total）"
    );
    assert_eq!(body["module"], "sandbox");
    assert_eq!(body["entity"], "demoItem");
    assert_eq!(body["label"], "示例条目");
    assert_eq!(field_keys(&body), ALL_FIELDS.to_vec());
    assert_eq!(body["fields"].as_array().expect("数组").len(), 7);
    // 数据接口地址（D102）：前端据此取数。
    assert_eq!(body["api"]["list"], "/api/sandbox/demo-items");
}

/// AC-S-078 步骤 2：必填 / 只读 / 类型专属参数如实透传；未声明的可选属性**省略该键**。
#[tokio::test]
async fn declared_attributes_are_passed_through_and_undeclared_ones_are_omitted() {
    let app = app(&["sandbox:demo:secret"]);
    let (status, raw) = get(&app, "/api/schema/sandbox/demoItem", Some("token")).await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");
    let body: Value = serde_json::from_str(&raw).expect("响应是 JSON");
    let fields = body["fields"].as_array().expect("fields 是数组");

    let title = &fields[1];
    assert_eq!(title["required"], true);
    assert_eq!(title["maxLength"], 200);
    let mut title_keys: Vec<String> = title
        .as_object()
        .expect("字段是对象")
        .keys()
        .cloned()
        .collect();
    title_keys.sort();
    assert_eq!(
        title_keys,
        vec![
            "key".to_string(),
            "label".to_string(),
            "maxLength".to_string(),
            "required".to_string(),
            "type".to_string()
        ],
        "未声明的可选属性不得出现（也不是 null）"
    );

    let id = &fields[0];
    assert_eq!(id["readonly"], true);
    let id_object = id.as_object().expect("字段是对象");
    assert!(!id_object.contains_key("required"), "id = {id}");
    assert!(!id_object.contains_key("permission"), "id = {id}");

    let secret = &fields[6];
    assert_eq!(secret["permission"], "sandbox:demo:secret");

    // 全文不出现 `null` 值：未声明 = 键不存在。
    assert!(!raw.contains("null"), "raw = {raw}");
}

/// AC-S-079：无权字段**从 fields 里被剔除**，且不留下任何痕迹。
#[tokio::test]
async fn fields_without_permission_are_removed_without_a_trace() {
    let app = app(&["sandbox:demo:read"]);
    let (status, raw) = get(&app, "/api/schema/sandbox/demoItem", Some("token")).await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");

    let body: Value = serde_json::from_str(&raw).expect("响应是 JSON");
    assert_eq!(
        field_keys(&body),
        vec!["id", "title", "amount", "paid", "dueAt", "ownerId"],
        "无权字段必须**不在**列表里（不是 hidden 标记）"
    );
    assert_eq!(body["fields"].as_array().expect("数组").len(), 6);
    assert_eq!(
        keys(&body),
        vec![
            "api".to_string(),
            "entity".to_string(),
            "fields".to_string(),
            "label".to_string(),
            "module".to_string()
        ],
        "顶层不得出现 hiddenFields / deniedFields / omitted / total"
    );
    for (token, label) in [
        ("\"hiddenFields\"", "清单键"),
        ("\"deniedFields\"", "清单键"),
        ("\"total\"", "计数键"),
        ("\"secret\"", "字段名（作为 JSON token）"),
        ("内部备注", "被剔除字段的 label"),
    ] {
        assert!(
            !raw.contains(token),
            "响应里出现了{label} `{token}`：{raw}"
        );
    }
}

/// AC-S-079 步骤 4：授予权限后**下一个请求**就包含该字段（缓存刷新在中间件里）。
#[tokio::test]
async fn granting_the_permission_takes_effect_on_the_next_request() {
    let without = app(&["sandbox:demo:read"]);
    let (_, raw) = get(&without, "/api/schema/sandbox/demoItem", Some("token")).await;
    let body: Value = serde_json::from_str(&raw).expect("JSON");
    assert!(!field_keys(&body).contains(&"secret".to_string()));

    let with = app(&["sandbox:demo:read", "sandbox:demo:secret"]);
    let (_, raw) = get(&with, "/api/schema/sandbox/demoItem", Some("token")).await;
    let body: Value = serde_json::from_str(&raw).expect("JSON");
    assert_eq!(field_keys(&body), ALL_FIELDS.to_vec());
}

/// AC-S-078 步骤 4：实体不存在 → 404 `SCHEMA_ENTITY_NOT_FOUND`。
#[tokio::test]
async fn an_unknown_entity_is_reported_as_schema_entity_not_found() {
    let app = app(&[]);
    let path = "/api/schema/sandbox/notARealEntity";
    let (status, raw) = get(&app, path, Some("token")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "raw = {raw}");
    let body: Value = serde_json::from_str(&raw).expect("JSON");
    assert_eq!(body["code"], "SCHEMA_ENTITY_NOT_FOUND");
    assert_eq!(body["status"], 404);
    assert_eq!(body["instance"], path);
    assert!(
        body.get("fields").is_none(),
        "404 不得带 fields（返回 `fields: []` 会更难发现）"
    );
}

/// AC-S-078 步骤 5：模块不存在 → 404 `SCHEMA_MODULE_NOT_FOUND`（与实体不存在可区分）。
#[tokio::test]
async fn an_unknown_module_is_reported_as_schema_module_not_found() {
    let app = app(&[]);
    let (status, raw) = get(&app, "/api/schema/notAModule/demoItem", Some("token")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "raw = {raw}");
    let body: Value = serde_json::from_str(&raw).expect("JSON");
    assert_eq!(body["code"], "SCHEMA_MODULE_NOT_FOUND");

    // 对照组：同一个实体在正确模块下是 200。
    let (status, _) = get(&app, "/api/schema/sandbox/demoItem", Some("token")).await;
    assert_eq!(status, StatusCode::OK);
}

/// AC-S-078 步骤 6：不带令牌 → 401 `UNAUTHENTICATED`，且**不含任何字段名**。
#[tokio::test]
async fn a_request_without_a_token_is_unauthenticated_and_leaks_nothing() {
    let app = app(&["sandbox:demo:secret"]);
    let (status, raw) = get(&app, "/api/schema/sandbox/demoItem", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "raw = {raw}");
    let body: Value = serde_json::from_str(&raw).expect("JSON");
    assert_eq!(body["code"], "UNAUTHENTICATED");
    assert_eq!(body["status"], 401);
    assert_eq!(
        keys(&body),
        vec![
            "code".to_string(),
            "detail".to_string(),
            "instance".to_string(),
            "status".to_string(),
            "title".to_string(),
            "trace_id".to_string(),
            "type".to_string()
        ],
        // ⚠️ 键名是 `trace_id`（api-conventions v1 C6 与既有 401 都是蛇形）；
        // TC-012-019 写的 `traceId` 与契约不符 —— 本实现跟契约走，不跟用例走。
        "错误体形状与既有 401 一致（没有多带业务键）"
    );
    assert!(!raw.contains("\"demoItem\""), "raw = {raw}");
    // ⚠️ 不能断言「全文不出现 `"title"`」：RFC 9457 的错误体**本身**就有 `title` 键
    // （TC-012-019 的那条断言在任何错误响应上都必然失败 —— 与 QA #4 同一形状）。
    // 该验的是「业务字段的**值/键**没有泄露」，这里验 `demoItem` 与字段值。
    assert!(!raw.contains("示例条目"), "raw = {raw}");
    assert!(!raw.contains("\"fields\""), "raw = {raw}");
}

/// AC-S-078（design §3.1）：**已认证但对本模块无任何权限点** → 200，只是看不到无权字段。
#[tokio::test]
async fn an_authenticated_user_without_any_permission_still_reads_the_metadata() {
    let app = app(&[]);
    let (status, raw) = get(&app, "/api/schema/sandbox/demoItem", Some("token")).await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");
    let body: Value = serde_json::from_str(&raw).expect("JSON");
    assert!(!field_keys(&body).contains(&"secret".to_string()));
}

/// 模块存在但**没有声明** `schema` 段 → 404（不是 200 + 空 fields）。
#[tokio::test]
async fn a_module_without_metadata_returns_entity_not_found() {
    let app = app(&[]);
    // `org` 有 manifest 但没有 schema 段。
    let (status, raw) = get(&app, "/api/schema/org/demoItem", Some("token")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "raw = {raw}");
    let body: Value = serde_json::from_str(&raw).expect("JSON");
    assert_eq!(body["code"], "SCHEMA_ENTITY_NOT_FOUND");
}
