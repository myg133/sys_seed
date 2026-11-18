//! 测试公用件：**SCIM 2.0 mock 服务**（一个真的 HTTP 服务）。
//!
//! # 为什么用真的 HTTP 服务而不是内存替身
//!
//! SCIM 出站是本期**自己实现**的（不引 SCIM 库，D6）——
//! 那段 HTTP 代码（URL 拼装、请求体、状态码、响应解析）**必须**被真的跑一遍，
//! 否则「自己实现的协议」就等于「没人验证过的协议」。
//!
#![allow(dead_code)]
//!
//! mock 服务按 **SCIM 的 PUT 语义**实现：同一个 id 推两次只有一条记录 ——
//! 这正是幂等性（AC-S-012）要验的东西。
//!
//! # 它能做什么
//!
//! - 记录收到的每个请求（路径 + 请求体）；
//! - 模拟失败（前 N 次调用失败 → 验重试）；
//! - 模拟慢响应（验「推送不阻塞本地请求」）。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde_json::{Value, json};

/// mock 服务的内部状态。
#[derive(Debug, Default)]
pub struct MockState {
    /// 收到的 User（按 SCIM id）。
    pub users: Mutex<BTreeMap<String, Value>>,
    /// 收到的 Group（按 SCIM id）。
    pub groups: Mutex<BTreeMap<String, Value>>,
    /// 收到的请求（`PUT /Users/<id>` 这样的标记）。
    pub calls: Mutex<Vec<String>>,
    /// 收到���请求体（按调用顺序）。
    pub bodies: Mutex<Vec<Value>>,
    /// 每个响应前的延迟（验「推送不阻塞本地请求」）。
    pub delay: Mutex<Duration>,
    /// 接下来失败的次数（验重试）。
    pub fail_next: Mutex<u32>,
}

/// SCIM mock 服务。
#[derive(Debug)]
pub struct MockScimServer {
    /// 目标地址（`http://127.0.0.1:<port>`）。
    pub base_url: String,
    /// 内部状态（断言用）。
    pub state: Arc<MockState>,
}

/// 启动一个 mock SCIM 服务（绑定到随机端口）。
pub async fn start_mock_scim_server() -> MockScimServer {
    let state = Arc::new(MockState::default());
    let app = Router::new()
        .route("/Users", get(list_users))
        .route("/Users/{id}", put(put_user).patch(patch_user))
        .route("/Groups", get(list_groups))
        .route("/Groups/{id}", put(put_group))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("应当能监听随机端口");
    let addr = listener.local_addr().expect("应当能拿到地址");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    MockScimServer {
        base_url: format!("http://{addr}"),
        state,
    }
}

impl MockScimServer {
    /// User 条数。
    pub fn user_count(&self) -> usize {
        self.state.users.lock().unwrap().len()
    }

    /// Group 条数。
    pub fn group_count(&self) -> usize {
        self.state.groups.lock().unwrap().len()
    }

    /// 收到的请求标记。
    pub fn calls(&self) -> Vec<String> {
        self.state.calls.lock().unwrap().clone()
    }

    /// 收到的请求体。
    pub fn bodies(&self) -> Vec<Value> {
        self.state.bodies.lock().unwrap().clone()
    }

    /// 读一条 User。
    pub fn user(&self, id: &str) -> Option<Value> {
        self.state.users.lock().unwrap().get(id).cloned()
    }

    /// 让接下来 N 次调用失败。
    pub fn fail_next(&self, count: u32) {
        *self.state.fail_next.lock().unwrap() = count;
    }

    /// 每个响应前先等这么久。
    pub fn set_delay(&self, delay: Duration) {
        *self.state.delay.lock().unwrap() = delay;
    }
}

/// 记一次调用（必要时失败）。
async fn record(state: &Arc<MockState>, marker: &str, body: Option<Value>) -> Option<Response> {
    let delay = *state.delay.lock().unwrap();
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    state.calls.lock().unwrap().push(marker.to_string());
    if let Some(body) = body {
        state.bodies.lock().unwrap().push(body);
    }
    let mut fail_next = state.fail_next.lock().unwrap();
    if *fail_next > 0 {
        *fail_next -= 1;
        return Some(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "the mock scim target is down",
            )
                .into_response(),
        );
    }
    None
}

/// PUT 一个 User（**幂等**：同一个 id 只有一条）。
async fn put_user(
    State(state): State<Arc<MockState>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Some(response) = record(&state, &format!("put:Users/{id}"), Some(body.clone())).await {
        return response;
    }
    let mut users = state.users.lock().unwrap();
    let schemas = body["schemas"].clone();
    users.insert(id.clone(), body);
    (
        StatusCode::OK,
        Json(json!({ "id": id, "schemas": schemas })),
    )
        .into_response()
}

/// PATCH 一个 User（`active` 变更）。
async fn patch_user(
    State(state): State<Arc<MockState>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Some(response) = record(&state, &format!("patch:Users/{id}"), Some(body.clone())).await {
        return response;
    }
    let mut users = state.users.lock().unwrap();
    if let Some(payload) = users.get_mut(&id) {
        if let Some(operations) = body["Operations"].as_array() {
            for operation in operations {
                let path = operation["path"].as_str().unwrap_or_default();
                if path == "active" {
                    if let Some(object) = payload.as_object_mut() {
                        object.insert("active".to_string(), operation["value"].clone());
                    }
                }
            }
        }
    }
    (StatusCode::OK, Json(json!({ "id": id }))).into_response()
}

/// PUT 一个 Group（**幂等**）。
async fn put_group(
    State(state): State<Arc<MockState>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Some(response) = record(&state, &format!("put:Groups/{id}"), Some(body.clone())).await {
        return response;
    }
    let mut groups = state.groups.lock().unwrap();
    groups.insert(id.clone(), body);
    (StatusCode::OK, Json(json!({ "id": id }))).into_response()
}

/// 列出 Users（支持 `userName eq "xxx"` 过滤 —— 真的 SCIM 服务也是服务端过滤）。
async fn list_users(
    State(state): State<Arc<MockState>>,
    axum::extract::Query(query): axum::extract::Query<std::collections::BTreeMap<String, String>>,
) -> Response {
    if let Some(response) = record(&state, "list:Users", None).await {
        return response;
    }
    let filter = query.get("filter").cloned().unwrap_or_default();
    let users = state.users.lock().unwrap();
    let resources: Vec<Value> = users
        .values()
        .filter(|value| matches_user_filter(value, &filter))
        .cloned()
        .collect();
    (
        StatusCode::OK,
        Json(json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:ListResponse"],
            "totalResults": resources.len(),
            "Resources": resources,
        })),
    )
        .into_response()
}

/// 判断一条 User 是否命中 SCIM 过滤器（只支持 `userName eq "xxx"`）。
fn matches_user_filter(user: &Value, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let Some((attribute, value)) = filter.split_once(" eq ") else {
        return true;
    };
    let value = value.trim().trim_matches('"');
    match attribute.trim() {
        "userName" => user["userName"].as_str() == Some(value),
        "id" => user["id"].as_str() == Some(value),
        _ => true,
    }
}

/// 列出 Groups。
async fn list_groups(State(state): State<Arc<MockState>>) -> Response {
    if let Some(response) = record(&state, "list:Groups", None).await {
        return response;
    }
    let groups = state.groups.lock().unwrap();
    let resources: Vec<Value> = groups.values().cloned().collect();
    (
        StatusCode::OK,
        Json(json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:ListResponse"],
            "totalResults": resources.len(),
            "Resources": resources,
        })),
    )
        .into_response()
}
