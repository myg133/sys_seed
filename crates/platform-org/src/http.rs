//! HTTP 层：`/api/org/*` 路由 + 统一错误体。
//!
//! # 路径约定（`api-conventions v1` C1 / C2）
//!
//! ```text
//! /api/org/<resource>            列表 / 详情
//! /api/org/<resource>/<action>   动作（动词放路径**末段**）
//! ```
//!
//! | 路由 | 说明 |
//! |------|------|
//! | `GET /api/org/org-node` | 列组织节点 |
//! | `POST /api/org/org-node` | 建组织节点 |
//! | `GET /api/org/org-node/{id}` | 节点详情 |
//! | `POST /api/org/org-node/{id}/move` | 移动节点 |
//! | `POST /api/org/org-node/{id}/rename` | 重命名节点 |
//! | `DELETE /api/org/org-node/{id}` | 删节点（成员必须先处理） |
//! | `GET /api/org/org-node/{id}/subtree` | 子树 + 成员 |
//! | `GET /api/org/user-org` | 列成员关系 |
//! | `POST /api/org/user-org` | 加成员 |
//! | `DELETE /api/org/user-org/{id}` | 移出成员 |
//! | `GET /api/org/user-position` | 列岗位 |
//! | `POST /api/org/user-position` | 加岗位 |
//! | `POST /api/org/user/{id}/manager` | 设直属上级 |
//! | `GET /api/org/sync-conflict` | 列冲突 |
//! | `POST /api/org/sync-conflict/{id}/resolve` | 裁决冲突 |
//! | `POST /api/org/scim/full-sync` | 触发 SCIM 全量对账（**入队即返回**） |
//! | `GET /api/org/sync-state` | 同步状态可见性 |
//!
//! # 错误体（契约 C6）
//!
//! RFC 9457（Problem Details）：`type` / `title` / `status` / `detail` / `instance` /
//! `code` / `trace_id` / `params`。**`detail` 必须是英文 ASCII** ——
//! 面向用户的中文文案由前端 i18n 按 `code` 渲染。
//!
//! # 认证
//!
//! 租户上下文由**平台中间件**注入（`X-Tenant-Id` → `RequestContext`），
//! 本层**不手工解析请求头**（契约 C3）。

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use platform_core::RequestContext;
use serde::{Deserialize, Serialize};

use crate::entity::{ConflictStatus, NodeType};
use crate::error::OrgError;
use crate::service::{NewMembership, NewNode, NewPosition, OrgService};

/// HTTP 层共享的状态。
#[derive(Clone)]
pub struct OrgState {
    /// 业务服务。
    pub service: OrgService,
    /// 令牌校验（复用 REQ-007 的 access token）。
    pub tokens: Arc<dyn TokenVerifier>,
}

impl std::fmt::Debug for OrgState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrgState")
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

/// 令牌校验接缝（可对象化）。
///
/// 组织数据是**租户数据** —— 只靠 `X-Tenant-Id` 请求头就能读别人的组织树是**越权**。
/// 所以每个接口都要验令牌，且**令牌里的租户**必须与上下文一致（不一致 → 404）。
pub trait TokenVerifier: Send + Sync {
    /// 校验 access token，返回其中的身份。
    fn verify(&self, token: &str) -> Result<VerifiedToken, OrgError>;
}

/// 令牌里的身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**，不来自请求头）。
    pub tenant_id: String,
}

/// 平台 `IamService` 的适配器（只转发验签，不重写语义）。
#[derive(Debug, Clone)]
pub struct IamTokenVerifier {
    inner: Arc<platform_iam::service::IamService>,
}

impl IamTokenVerifier {
    /// 包一个平台的认证服务。
    pub fn new(inner: Arc<platform_iam::service::IamService>) -> Self {
        Self { inner }
    }
}

impl TokenVerifier for IamTokenVerifier {
    fn verify(&self, token: &str) -> Result<VerifiedToken, OrgError> {
        let claims = self
            .inner
            .verify_access_token(token)
            .map_err(|_| OrgError::Unauthenticated)?;
        Ok(VerifiedToken {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
        })
    }
}

/// 已认证的调用方。
#[derive(Debug, Clone)]
pub struct Authenticated {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**）。
    pub tenant_id: String,
}

impl FromRequestParts<Arc<OrgState>> for Authenticated {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &Arc<OrgState>,
    ) -> Result<Self, Self::Rejection> {
        let instance = parts.uri.path().to_string();
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);

        let token = bearer_token(&parts.headers)
            .ok_or_else(|| problem_response(&OrgError::Unauthenticated, &ctx, &instance))?;
        let verified = state
            .tokens
            .verify(&token)
            .map_err(|error| problem_response(&error, &ctx, &instance))?;

        // 租户必须一致：请求头与令牌绑定的租户不一致 → **404**（不泄露资源存在性，契约 C5）。
        if let Some(tenant) = ctx.tenant_id() {
            if tenant != verified.tenant_id {
                return Err(problem_response(
                    &OrgError::NotFound {
                        resource: "resource",
                    },
                    &ctx,
                    &instance,
                ));
            }
        }

        Ok(Self {
            user_id: verified.user_id,
            tenant_id: verified.tenant_id,
        })
    }
}

/// 从 `Authorization: Bearer <token>` 里取令牌。
fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let trimmed = raw.trim();
    let token = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// 统一错误响应体（RFC 9457，契约 C6）。
#[derive(Debug, Clone, Serialize)]
pub struct ProblemDetails {
    /// 错误类型 URI。
    #[serde(rename = "type")]
    pub kind: String,
    /// 简短标题。
    pub title: String,
    /// HTTP 状态码。
    pub status: u16,
    /// 面向开发者的**英文**描述。
    pub detail: String,
    /// 出错的接口路径。
    pub instance: String,
    /// 稳定的机器可读错误码。
    pub code: String,
    /// 追踪 ID。
    pub trace_id: String,
    /// 补充参数（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, serde_json::Value>>,
}

/// 构造错误响应体。
pub fn problem_details(error: &OrgError, ctx: &RequestContext, instance: &str) -> ProblemDetails {
    let params = error.params();
    ProblemDetails {
        kind: format!(
            "https://errors.base.example/{}",
            error.code().to_ascii_lowercase().replace('_', "-")
        ),
        title: title_of(error).to_string(),
        status: error.status().as_u16(),
        detail: error.detail(),
        instance: instance.to_string(),
        code: error.code().to_string(),
        trace_id: ctx.trace_id().to_string(),
        params: if params.is_empty() {
            None
        } else {
            Some(params)
        },
    }
}

/// 把错误变成 HTTP 响应。
pub fn problem_response(error: &OrgError, ctx: &RequestContext, instance: &str) -> Response {
    let body = problem_details(error, ctx, instance);
    // 运维视角的中文说明放**日志**，不进响应体（契约 C6）。
    tracing::warn!(
        code = body.code,
        status = body.status,
        trace_id = %body.trace_id,
        "组织架构请求失败（面向用户的中文文案由前端按 code 渲染）"
    );
    (
        StatusCode::from_u16(body.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(body),
    )
        .into_response()
}

/// 时间点 → RFC 3339 字符串（UTC）。
fn format_timestamp(moment: time::OffsetDateTime) -> String {
    // `time` 的 serde 特性没开（省编译时间），所以这里手工格式化。
    let utc = moment.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        utc.year(),
        u8::from(utc.month()),
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second()
    )
}

/// 错误码 → 简短标题（**类别的名字**，不是面向用户的句子）。
fn title_of(error: &OrgError) -> &'static str {
    match error {
        OrgError::TenantContextMissing => "缺少租户上下文",
        OrgError::Unauthenticated => "未认证",
        OrgError::NotFound { .. } => "资源不存在",
        OrgError::OrgCycle { .. } => "组织结构冲突",
        OrgError::OrgNodeNotEmpty { .. } => "节点仍有下级",
        OrgError::OrgNodeHasMembers { .. } => "节点仍有成员",
        OrgError::OrgCodeDuplicated { .. } => "节点编码重复",
        OrgError::OrgNodeTypeInvalid { .. } => "节点类型非法",
        OrgError::InvalidArgument { .. } => "参数不合法",
        OrgError::Conflict { .. } => "状态冲突",
        OrgError::SyncConflictPending { .. } => "同步冲突待裁决",
        OrgError::SyncConflictStrategy { .. } => "同步策略不允许",
        OrgError::SyncPushFailed { .. } => "同步推送失败",
        OrgError::NotImplemented { .. } => "尚未实现",
        OrgError::Internal { .. } => "服务内部错误",
    }
}

/// 用**令牌里的租户**构造请求上下文（契约 C3：请求头缺失时由令牌推断）。
///
/// 租户只认令牌 —— 请求头只是用来跟令牌**比对**（不一致 → 404），
/// 绝不作为租户来源。
fn authenticated_context(auth: &Authenticated) -> RequestContext {
    RequestContext::new().with_tenant(auth.tenant_id.clone())
}

/// 路由。
///
/// 返回类型的状态参数是通用的：调用方（`platform-server`）可以把它
/// `nest` 进自己的路由里 —— 组织架构的状态由 `Arc<OrgState>` 自带。
pub fn router<S2>(state: Arc<OrgState>) -> Router<S2> {
    Router::new()
        .route("/org-node", get(list_org_nodes).post(create_org_node))
        .route("/org-node/{id}", get(get_org_node).delete(delete_org_node))
        .route("/org-node/{id}/move", post(move_org_node))
        .route("/org-node/{id}/rename", post(rename_org_node))
        .route("/org-node/{id}/subtree", get(get_subtree))
        .route("/user-org", get(list_user_org).post(add_user_org))
        .route("/user-org/{id}", delete(remove_user_org))
        .route(
            "/user-position",
            get(list_user_position).post(add_user_position),
        )
        .route("/user/{id}/manager", post(set_manager))
        .route("/sync-conflict", get(list_sync_conflict))
        .route("/sync-conflict/{id}/resolve", post(resolve_sync_conflict))
        .route("/scim/full-sync", post(full_scim_sync))
        .route("/sync-state", get(get_sync_state))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// 组织节点
// ---------------------------------------------------------------------------

/// 列组织节点的查询参数。
#[derive(Debug, Clone, Deserialize)]
pub struct ListOrgNodesQuery {
    /// 只列某个父节点的直接下级。
    #[serde(rename = "parentId")]
    parent_id: Option<String>,
}

/// 组织节点的响应体。
#[derive(Debug, Clone, Serialize)]
pub struct OrgNodeView {
    /// 节点 ID。
    pub id: String,
    /// 父节点 ID。
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    /// 节点类型。
    #[serde(rename = "nodeType")]
    pub node_type: String,
    /// 节点编码。
    pub code: String,
    /// 节点名称。
    pub name: String,
    /// 祖先链（加速字段）。
    #[serde(rename = "pathLabels")]
    pub path_labels: Vec<String>,
}

impl From<crate::entity::OrgNode> for OrgNodeView {
    fn from(node: crate::entity::OrgNode) -> Self {
        Self {
            id: node.id,
            parent_id: node.parent_id,
            node_type: node.node_type.as_str().to_string(),
            code: node.code,
            name: node.name,
            path_labels: node.path_labels,
        }
    }
}

/// 新建节点的请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct CreateOrgNodeRequest {
    /// 父节点 ID（不给 = 根节点）。
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    /// 节点类型（`company` / `dept` / `team`）。
    #[serde(rename = "nodeType")]
    pub node_type: String,
    /// 节点编码。
    pub code: String,
    /// 节点名称。
    pub name: String,
}

/// 列组织节点。
async fn list_org_nodes(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Query(query): Query<ListOrgNodesQuery>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state
        .service
        .list_nodes(&ctx, query.parent_id.as_deref())
        .await
    {
        Ok(nodes) => {
            let views: Vec<OrgNodeView> = nodes.into_iter().map(OrgNodeView::from).collect();
            (StatusCode::OK, Json(views)).into_response()
        }
        Err(error) => problem_response(&error, &ctx, "/api/org/org-node"),
    }
}

/// 新建组织节点。
async fn create_org_node(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    axum::Json(body): axum::Json<CreateOrgNodeRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    let node_type = match NodeType::parse(&body.node_type) {
        Some(node_type) => node_type,
        None => {
            let error = OrgError::OrgNodeTypeInvalid {
                value: body.node_type.clone(),
            };
            return problem_response(&error, &ctx, "/api/org/org-node");
        }
    };
    let input = NewNode {
        id: None,
        parent_id: body.parent_id,
        node_type,
        code: body.code,
        name: body.name,
    };
    match state.service.create_node(&ctx, input).await {
        Ok(node) => (StatusCode::CREATED, Json(OrgNodeView::from(node))).into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/org-node"),
    }
}

/// 节点详情。
async fn get_org_node(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state.service.get_node(&ctx, &id).await {
        Ok(node) => (StatusCode::OK, Json(OrgNodeView::from(node))).into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/org-node"),
    }
}

/// 移动节点的请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct MoveOrgNodeRequest {
    /// 新的父节点 ID（不给 = 变成根节点）。
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
}

/// 移动节点。
async fn move_org_node(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<MoveOrgNodeRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state
        .service
        .move_node(&ctx, &id, body.parent_id.as_deref())
        .await
    {
        Ok(node) => (StatusCode::OK, Json(OrgNodeView::from(node))).into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/org-node"),
    }
}

/// 重命名节点的请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct RenameOrgNodeRequest {
    /// 新名称。
    pub name: String,
}

/// 重命名节点。
async fn rename_org_node(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<RenameOrgNodeRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state.service.rename_node(&ctx, &id, &body.name).await {
        Ok(node) => (StatusCode::OK, Json(OrgNodeView::from(node))).into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/org-node"),
    }
}

/// 删除节点的请求体。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DeleteOrgNodeRequest {
    /// 把成员迁到哪个节点（不给 = 拒绝删除仍有成员的节点）。
    #[serde(rename = "moveMembersTo")]
    pub move_members_to: Option<String>,
}

/// 删除节点。
async fn delete_org_node(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Path(id): Path<String>,
    Query(query): Query<DeleteOrgNodeRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state
        .service
        .delete_node(&ctx, &id, query.move_members_to.as_deref())
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/org-node"),
    }
}

/// 子树（含成员）的响应体。
#[derive(Debug, Clone, Serialize)]
pub struct SubtreeView {
    /// 子树上的节点。
    pub nodes: Vec<OrgNodeView>,
    /// 子树上的成员关系。
    pub members: Vec<UserOrgView>,
}

/// 子树 + 成员。
async fn get_subtree(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Response {
    let ctx = authenticated_context(&auth);
    let nodes = match state.service.subtree(&ctx, &id).await {
        Ok(nodes) => nodes,
        Err(error) => return problem_response(&error, &ctx, "/api/org/org-node"),
    };
    let members = match state.service.members_of_subtree(&ctx, &id).await {
        Ok(members) => members,
        Err(error) => return problem_response(&error, &ctx, "/api/org/org-node"),
    };
    let view = SubtreeView {
        nodes: nodes.into_iter().map(OrgNodeView::from).collect(),
        members: members.into_iter().map(UserOrgView::from).collect(),
    };
    (StatusCode::OK, Json(view)).into_response()
}

// ---------------------------------------------------------------------------
// 成员关系与岗位
// ---------------------------------------------------------------------------

/// 成员关系的响应体。
#[derive(Debug, Clone, Serialize)]
pub struct UserOrgView {
    /// 关系 ID。
    pub id: String,
    /// 用户 ID。
    #[serde(rename = "userId")]
    pub user_id: String,
    /// 组织节点 ID。
    #[serde(rename = "orgNodeId")]
    pub org_node_id: String,
    /// 在该节点上的角色。
    pub role: Option<String>,
    /// 是否主部门。
    #[serde(rename = "isPrimary")]
    pub is_primary: bool,
}

impl From<crate::entity::UserOrg> for UserOrgView {
    fn from(item: crate::entity::UserOrg) -> Self {
        Self {
            id: item.id,
            user_id: item.user_id,
            org_node_id: item.org_node_id,
            role: item.role,
            is_primary: item.is_primary,
        }
    }
}

/// 列成员关系的查询参数。
#[derive(Debug, Clone, Deserialize)]
pub struct ListUserOrgQuery {
    /// 按用户过滤。
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    /// 按节点过滤。
    #[serde(rename = "orgNodeId")]
    pub org_node_id: Option<String>,
}

/// 列成员关系。
async fn list_user_org(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Query(query): Query<ListUserOrgQuery>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state
        .service
        .list_members(&ctx, query.user_id.as_deref(), query.org_node_id.as_deref())
        .await
    {
        Ok(items) => {
            let views: Vec<UserOrgView> = items.into_iter().map(UserOrgView::from).collect();
            (StatusCode::OK, Json(views)).into_response()
        }
        Err(error) => problem_response(&error, &ctx, "/api/org/user-org"),
    }
}

/// 新增成员关系的请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct AddUserOrgRequest {
    /// 用户 ID。
    #[serde(rename = "userId")]
    pub user_id: String,
    /// 组织节点 ID。
    #[serde(rename = "orgNodeId")]
    pub org_node_id: String,
    /// 在该节点上的角色。
    pub role: Option<String>,
    /// 是否主部门。
    #[serde(rename = "isPrimary")]
    pub is_primary: Option<bool>,
}

/// 加成员。
async fn add_user_org(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    axum::Json(body): axum::Json<AddUserOrgRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    let input = NewMembership {
        id: None,
        user_id: body.user_id,
        org_node_id: body.org_node_id,
        role: body.role,
        is_primary: body.is_primary.unwrap_or(false),
    };
    match state.service.add_member(&ctx, input).await {
        Ok(item) => (StatusCode::CREATED, Json(UserOrgView::from(item))).into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/user-org"),
    }
}

/// 移出成员。
async fn remove_user_org(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state.service.remove_member(&ctx, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/user-org"),
    }
}

/// 岗位的响应体。
#[derive(Debug, Clone, Serialize)]
pub struct UserPositionView {
    /// 记录 ID。
    pub id: String,
    /// 用户 ID。
    #[serde(rename = "userId")]
    pub user_id: String,
    /// 岗位编码。
    #[serde(rename = "positionCode")]
    pub position_code: String,
    /// 挂靠的组织节点。
    #[serde(rename = "orgNodeId")]
    pub org_node_id: Option<String>,
}

impl From<crate::entity::UserPosition> for UserPositionView {
    fn from(item: crate::entity::UserPosition) -> Self {
        Self {
            id: item.id,
            user_id: item.user_id,
            position_code: item.position_code,
            org_node_id: item.org_node_id,
        }
    }
}

/// 列岗位的查询参数。
#[derive(Debug, Clone, Deserialize)]
pub struct ListUserPositionQuery {
    /// 按用户过滤。
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
}

/// 列岗位。
async fn list_user_position(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Query(query): Query<ListUserPositionQuery>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state
        .service
        .list_positions(&ctx, query.user_id.as_deref())
        .await
    {
        Ok(items) => {
            let views: Vec<UserPositionView> =
                items.into_iter().map(UserPositionView::from).collect();
            (StatusCode::OK, Json(views)).into_response()
        }
        Err(error) => problem_response(&error, &ctx, "/api/org/user-position"),
    }
}

/// 新增岗位的请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct AddUserPositionRequest {
    /// 用户 ID。
    #[serde(rename = "userId")]
    pub user_id: String,
    /// 岗位编码。
    #[serde(rename = "positionCode")]
    pub position_code: String,
    /// 挂靠的组织节点。
    #[serde(rename = "orgNodeId")]
    pub org_node_id: Option<String>,
}

/// 加岗位。
async fn add_user_position(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    axum::Json(body): axum::Json<AddUserPositionRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    let input = NewPosition {
        id: None,
        user_id: body.user_id,
        position_code: body.position_code,
        org_node_id: body.org_node_id,
    };
    match state.service.add_position(&ctx, input).await {
        Ok(item) => (StatusCode::CREATED, Json(UserPositionView::from(item))).into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/user-position"),
    }
}

/// 设置直属上级的请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct SetManagerRequest {
    /// 直属上级的用户 ID（不给 = 取消）。
    #[serde(rename = "managerId")]
    pub manager_id: Option<String>,
}

/// 设置直属上级（汇报关系是**人的属性**，D3）。
async fn set_manager(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<SetManagerRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state
        .service
        .set_manager(&ctx, &id, body.manager_id.as_deref())
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/user"),
    }
}

// ---------------------------------------------------------------------------
// 同步
// ---------------------------------------------------------------------------

/// 冲突的响应体。
#[derive(Debug, Clone, Serialize)]
pub struct SyncConflictView {
    /// 冲突 ID。
    pub id: String,
    /// 来源。
    pub source: String,
    /// 外部 ID。
    #[serde(rename = "externalId")]
    pub external_id: String,
    /// 冲突字段。
    pub field: String,
    /// 本地值。
    #[serde(rename = "localValue")]
    pub local_value: String,
    /// 外部值。
    #[serde(rename = "remoteValue")]
    pub remote_value: String,
    /// 状态。
    pub status: String,
    /// 裁决结果。
    pub resolution: Option<String>,
    /// 裁决人。
    #[serde(rename = "resolvedBy")]
    pub resolved_by: Option<String>,
}

impl From<crate::entity::SyncConflict> for SyncConflictView {
    fn from(conflict: crate::entity::SyncConflict) -> Self {
        Self {
            id: conflict.id,
            source: conflict.source,
            external_id: conflict.external_id,
            field: conflict.field,
            local_value: conflict.local_value,
            remote_value: conflict.remote_value,
            status: conflict.status.as_str().to_string(),
            resolution: conflict.resolution.map(|item| item.as_str().to_string()),
            resolved_by: conflict.resolved_by,
        }
    }
}

/// 列冲突的查询参数。
#[derive(Debug, Clone, Deserialize)]
pub struct ListSyncConflictQuery {
    /// 按状态过滤（`pending` / `resolved` / `ignored`）。
    pub status: Option<String>,
}

/// 列冲突。
async fn list_sync_conflict(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Query(query): Query<ListSyncConflictQuery>,
) -> Response {
    let ctx = authenticated_context(&auth);
    let status = match &query.status {
        Some(raw) => match ConflictStatus::parse(raw) {
            Some(status) => Some(status),
            None => {
                let error = OrgError::InvalidArgument {
                    field: "status",
                    reason: format!("`{raw}` is not a valid conflict status"),
                };
                return problem_response(&error, &ctx, "/api/org/sync-conflict");
            }
        },
        None => None,
    };
    match state.service.list_conflicts(&ctx, status).await {
        Ok(items) => {
            let views: Vec<SyncConflictView> =
                items.into_iter().map(SyncConflictView::from).collect();
            (StatusCode::OK, Json(views)).into_response()
        }
        Err(error) => problem_response(&error, &ctx, "/api/org/sync-conflict"),
    }
}

/// 裁决冲突的请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct ResolveSyncConflictRequest {
    /// 裁决：`keep_local` / `take_remote` / `manual`。
    pub resolution: String,
    /// `manual` 裁决时的值。
    pub value: Option<String>,
    /// 裁决人（不给就用请求里的操作者）。
    #[serde(rename = "resolvedBy")]
    pub resolved_by: Option<String>,
}

/// 裁决冲突。
async fn resolve_sync_conflict(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<ResolveSyncConflictRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    let resolution = match body.resolution.as_str() {
        "keep_local" => crate::entity::ConflictResolution::KeepLocal,
        "take_remote" => crate::entity::ConflictResolution::TakeRemote,
        "manual" => crate::entity::ConflictResolution::Manual,
        other => {
            let error = OrgError::InvalidArgument {
                field: "resolution",
                reason: format!("`{other}` is not a valid resolution"),
            };
            return problem_response(&error, &ctx, "/api/org/sync-conflict");
        }
    };
    let resolved_by = body.resolved_by.unwrap_or_else(|| "api".to_string());
    match state
        .service
        .resolve_conflict(&ctx, &id, resolution, body.value.as_deref(), &resolved_by)
        .await
    {
        Ok(conflict) => (StatusCode::OK, Json(SyncConflictView::from(conflict))).into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/sync-conflict"),
    }
}

/// 触发 SCIM 全量对账的请求体。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FullScimSyncRequest {
    /// 目标地址（不给 = 用环境变量里的配置）。
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    /// provider 标识。
    pub provider: Option<String>,
    /// 认证头。
    #[serde(rename = "authHeader")]
    pub auth_header: Option<String>,
}

/// 触发 SCIM 全量对账（**入队即返回** —— 不等第三方）。
async fn full_scim_sync(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    axum::Json(body): axum::Json<FullScimSyncRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    let mut config = match crate::config::scim_provider_from_env() {
        Some(config) => config,
        None => {
            let error = OrgError::NotImplemented {
                feature: "the scim outbound sync (no provider is configured)",
            };
            return problem_response(&error, &ctx, "/api/org/scim/full-sync");
        }
    };
    if let Some(base_url) = body.base_url {
        config.base_url = base_url;
    }
    if let Some(provider) = body.provider {
        config.provider = provider;
    }
    if let Some(auth_header) = body.auth_header {
        config.auth_header = Some(auth_header);
    }
    match state.service.full_scim_sync(&ctx, &config).await {
        Ok(summary) => (StatusCode::ACCEPTED, Json(summary.to_payload())).into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/scim/full-sync"),
    }
}

/// 同步状态的查询参数。
#[derive(Debug, Clone, Deserialize)]
pub struct SyncStateQuery {
    /// 来源标识。
    pub source: String,
}

/// 同步状态（上次同步时间、错误计数）。
async fn get_sync_state(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,
    Query(query): Query<SyncStateQuery>,
) -> Response {
    let ctx = authenticated_context(&auth);
    match state.service.sync_state(&ctx, &query.source).await {
        Ok(Some(item)) => {
            let body = serde_json::json!({
                "source": item.source,
                // 时间一律序列化成 RFC 3339 字符串（契约 C13：UTC 存储）。
                "lastSyncAt": item.last_sync_at.map(format_timestamp),
                "lastError": item.last_error,
                "errorCount": item.error_count,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Ok(None) => (
            StatusCode::OK,
            Json(serde_json::json!({ "source": query.source })),
        )
            .into_response(),
        Err(error) => problem_response(&error, &ctx, "/api/org/sync-state"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_problem_body_carries_the_stable_code() {
        let ctx = RequestContext::new().with_tenant("tenant-a");
        let error = OrgError::NotFound {
            resource: "org_node",
        };
        let body = problem_details(&error, &ctx, "/api/org/org-node");
        assert_eq!(body.code, "NOT_FOUND");
        assert_eq!(body.status, 404);
        assert!(body.detail.is_ascii());
        assert_eq!(body.instance, "/api/org/org-node");
        assert!(!body.trace_id.is_empty());
    }

    #[test]
    fn the_english_detail_never_leaks_a_chinese_sentence() {
        let ctx = RequestContext::new().with_tenant("tenant-a");
        for error in [
            OrgError::OrgCycle {
                reason: "a node cannot be moved under itself",
            },
            OrgError::OrgNodeHasMembers { members: 3 },
            OrgError::InvalidArgument {
                field: "code",
                reason: "must not be empty".to_string(),
            },
        ] {
            let body = problem_details(&error, &ctx, "/api/org/org-node");
            assert!(
                body.detail.is_ascii(),
                "detail 必须是 ASCII：{}",
                body.detail
            );
        }
    }
}
