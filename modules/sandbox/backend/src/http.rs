//! 沙箱模块的 HTTP 层：**参考实现**（REQ-009）。
//!
//! # 这是二次开发照着抄的标准范例
//!
//! | 要点 | 怎么做的 |
//! |------|----------|
//! | 路由怎么绑权限点 | `Router::route_layer(require_permission("<code>"))` |
//! | 权限点从哪来 | `manifest.json` 的 `permissions[]`（**不硬编码角色名**） |
//! | 数据范围怎么用 | 问平台要 [`ScopeFilter`]，交给 `TenantQuery::with_scope` |
//! | 属性条件怎么用 | 问平台要 `CompiledCondition`，一起交给查询构造器 |
//! | 越权怎么表现 | 缺权限点 → **403**；记录不可见 → **404**（不泄露存在性） |
//!
//! # 依赖
//!
//! 只有 `platform-core`（`require_permission` / `PermissionChecker` / `ScopeFilter`）
//! 与 `platform-db`（`TenantDb` / `TenantQuery`）——
//! **不依赖 `platform-rbac`**（那是平台内部模块，AGENTS.md 硬规则 2）。
//!
//! 判定实现由**平台**通过请求扩展注入（`Arc<dyn PermissionChecker>`）。

use std::path::Path as FsPath;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use axum::extract::{FromRequestParts, OriginalUri, Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use platform_core::RequestContext;
use platform_core::extension::manifest::Manifest;
use platform_core::extension::permission::{PermissionChecker, ScopeFilter, require_permission};
use platform_core::extension::schema::EntitySchema;
use platform_core::problem::{ProblemDetails, problem_response};
use platform_db::TenantDb;
use serde::Serialize;

use crate::demo_item_http;
use crate::demo_note_http;
use crate::permission::{
    PERMISSION_DEMO_READ, PERMISSION_DEMO_WRITE, PERMISSION_SCOPE_ITEM_APPROVE,
    PERMISSION_SCOPE_ITEM_EXPORT, PERMISSION_SCOPE_ITEM_READ,
};
use crate::scope_item::{PgScopeItemRepository, ScopeItem};

/// `demoItem` 实体的键（元数据驱动渲染的样本实体）。
pub const DEMO_ITEM_ENTITY: &str = "demoItem";

/// `demoNote` 实体的键（**第二个**样本实体，D107：证明零前端代码）。
///
/// 它与 `demoItem` 的字段组合不同 —— 前端源码里**不得**出现这个键
/// （AC-S-085 的核心断言）；这里出现是因为后端本来就是它的实现方。
pub const DEMO_NOTE_ENTITY: &str = "demoNote";

/// 模块自己的 `manifest.json`（**编译期**读入）。
///
/// # 为什么可以编译期读
///
/// 这份 manifest 与模块代码同源（同一个仓库、同一个提交），
/// 而**运行期**的 `BASE_MANIFEST_ROOT` 版本由平台在启动时校验（阶段 1 + 阶段 2）。
/// 两者不一致的窗口只在「部署时换了另一份 manifest」—— 那种情况下平台会在启动时
/// 因为所有者不同而 fail-fast。
///
/// 生产装配走 [`SandboxState::from_manifest`]（读**运行期**那一份，单一来源）；
/// 这个常量只给**测试**与 [`SandboxState::new`] 用，并由
/// `tests/demo_items.rs` 的 `the_baked_schema_matches_the_manifest_on_disk` 守住一致性。
const BAKED_MANIFEST: &str = include_str!("../../manifest.json");

/// HTTP 层共享的状态。
#[derive(Debug, Clone)]
pub struct SandboxState {
    /// 租户作用域的数据访问层。
    pub db: TenantDb,
    /// `demoItem` 的元数据（字段级权限判定的依据）。
    ///
    /// 与 `GET /api/schema/sandbox/demoItem` 返回的是**同一份**声明 ——
    /// 元数据说可见、数据却剔掉（或反过来）这种分叉在代码层面就该不存在。
    pub demo_item_schema: Arc<EntitySchema>,
    /// `demoNote` 的元数据（同一份 manifest 的另一个实体，D107）。
    ///
    /// 它没有字段级权限点，投影因此是恒等的 —— 仍然走 `SchemaProjector`：
    /// 「有没有权限」不该决定「走不走唯一序列化入口」。
    pub demo_note_schema: Arc<EntitySchema>,
}

impl SandboxState {
    /// 构造状态（元数据取自**编译期**的 manifest —— 测试用）。
    ///
    /// # Panics
    ///
    /// manifest 里没有 `schema.demoItem` / `schema.demoNote` 时 panic：那是模块自身的
    /// 编译期资产，缺了说明有人把契约改坏了（`tests/demo_items.rs` 会先红）。
    pub fn new(db: TenantDb) -> Self {
        let demo_item = baked_entity_schema(DEMO_ITEM_ENTITY)
            .expect("modules/sandbox/manifest.json 必须声明 schema.demoItem");
        let demo_note = baked_entity_schema(DEMO_NOTE_ENTITY)
            .expect("modules/sandbox/manifest.json 必须声明 schema.demoNote");
        Self {
            db,
            demo_item_schema: Arc::new(demo_item),
            demo_note_schema: Arc::new(demo_note),
        }
    }

    /// 构造状态：元数据取自**运行期**的 manifest（装配层用 —— 单一来源）。
    pub fn from_manifest(db: TenantDb, manifest_root: &FsPath) -> Result<Self> {
        let path = manifest_root.join("modules/sandbox/manifest.json");
        let manifest = Manifest::from_path(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let schemas = manifest
            .schema
            .as_ref()
            .with_context(|| format!("{} declares no schema section", path.display()))?;
        let entity = |key: &str| -> Result<EntitySchema> {
            schemas
                .get(key)
                .cloned()
                .with_context(|| format!("{} declares no schema.{key} entity", path.display()))
        };
        Ok(Self {
            db,
            demo_item_schema: Arc::new(entity(DEMO_ITEM_ENTITY)?),
            demo_note_schema: Arc::new(entity(DEMO_NOTE_ENTITY)?),
        })
    }

    /// 数据范围示例仓储。
    fn scope_items(&self) -> PgScopeItemRepository {
        PgScopeItemRepository::new(self.db.clone())
    }
}

/// 从编译期读入的 manifest 取 `schema.<entity>`。
fn baked_entity_schema(entity: &str) -> Result<EntitySchema> {
    let manifest = Manifest::parse_str(BAKED_MANIFEST)
        .map_err(|error| anyhow::anyhow!("the baked manifest is invalid: {error}"))?;
    manifest
        .schema
        .as_ref()
        .and_then(|schemas| schemas.get(entity))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("the baked manifest declares no schema.{entity}"))
}

/// 带租户上下文的请求上下文提取器。
#[derive(Debug, Clone)]
pub struct TenantContext(pub RequestContext);

impl<S: Send + Sync> FromRequestParts<S> for TenantContext {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        Ok(TenantContext(ctx))
    }
}

/// 判定实现的提取器（平台通过请求扩展注入）。
#[derive(Clone)]
pub struct Checker(pub Arc<dyn PermissionChecker>);

impl std::fmt::Debug for Checker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checker").finish_non_exhaustive()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Checker {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        let checker = parts
            .extensions
            .get::<Arc<dyn PermissionChecker>>()
            .cloned()
            .ok_or_else(|| {
                problem_response(&ProblemDetails::from_context(
                    platform_core::problem::CODE_INTERNAL_ERROR,
                    "服务内部错误",
                    500,
                    "the permission checker is not installed on this service",
                    &ctx,
                    parts.uri.path(),
                    None,
                ))
            })?;
        Ok(Checker(checker))
    }
}

/// 数据范围示例的响应体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeItemResponse {
    /// 主键。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 金额。
    pub amount: i64,
    /// 归属组织节点。
    pub org_node_id: String,
    /// 创建人。
    pub created_by: String,
}

impl From<ScopeItem> for ScopeItemResponse {
    fn from(item: ScopeItem) -> Self {
        Self {
            id: item.id,
            title: item.title,
            amount: item.amount,
            org_node_id: item.org_node_id,
            created_by: item.created_by,
        }
    }
}

/// 构造沙箱模块的路由。
///
/// **每个路由都绑定自己的权限点** —— 一组路由一个 `route_layer`。
///
/// ⚠️ **参数必须是字面量**（不是常量）：契约检查 C-5 要能**静态确认**
/// 「这个权限点真的有鉴权调用点」—— 参数是常量就没法确认，判为**失败**
/// （无法确认 = 不放行，fail closed）。
/// 一个权限点绑到不相关的路由上 = 权限模型失真（用户问「谁能读导出」，
/// 答案会变成「能读沙箱示例的人」）。
pub fn router<S2>(state: Arc<SandboxState>) -> Router<S2> {
    // `sandbox:demo:read`
    let demo_read_routes = Router::new().route("/demo", get(demo_read));

    // `sandbox:demo:write`
    let demo_write_routes = Router::new().route("/demo", post(demo_write));

    // `sandbox:demo:read` —— 元数据驱动渲染的列表（**响应经 SchemaProjector**）。
    let demo_item_read_routes = Router::new().route("/demo-items", get(demo_item_http::list));

    // `sandbox:demo:write` —— 元数据的 create / update / delete 三个动作。
    let demo_item_write_routes = Router::new()
        .route("/demo-items", post(demo_item_http::create))
        .route("/demo-items/{id}", put(demo_item_http::update))
        .route("/demo-items/{id}", delete(demo_item_http::delete));

    // `sandbox:demo:read` —— **第二个**实体（D107）：同一套权限点、同一套语义。
    let demo_note_read_routes = Router::new().route("/demo-notes", get(demo_note_http::list));

    // `sandbox:demo:write` —— 第二个实体的写侧。
    let demo_note_write_routes = Router::new()
        .route("/demo-notes", post(demo_note_http::create))
        .route("/demo-notes/{id}", put(demo_note_http::update))
        .route("/demo-notes/{id}", delete(demo_note_http::delete));

    // `sandbox:scope-item:read`
    let scope_read_routes = Router::new()
        .route("/scope-item", get(list_scope_items))
        .route("/scope-item/{id}", get(get_scope_item));

    // `sandbox:scope-item:approve`
    let approve_routes = Router::new().route("/scope-item/approve", post(approve_scope_item));

    // `sandbox:scope-item:export`
    let export_routes = Router::new().route("/scope-item/export", post(export_scope_items));

    Router::new()
        .merge(demo_read_routes.route_layer(require_permission("sandbox:demo:read")))
        .merge(demo_item_read_routes.route_layer(require_permission("sandbox:demo:read")))
        .merge(demo_note_read_routes.route_layer(require_permission("sandbox:demo:read")))
        .merge(demo_write_routes.route_layer(require_permission("sandbox:demo:write")))
        .merge(demo_item_write_routes.route_layer(require_permission("sandbox:demo:write")))
        .merge(demo_note_write_routes.route_layer(require_permission("sandbox:demo:write")))
        .merge(scope_read_routes.route_layer(require_permission("sandbox:scope-item:read")))
        .merge(approve_routes.route_layer(require_permission("sandbox:scope-item:approve")))
        .merge(export_routes.route_layer(require_permission("sandbox:scope-item:export")))
        .with_state(state)
}

/// 读沙箱示例。
async fn demo_read(State(_state): State<Arc<SandboxState>>) -> Response {
    Json(serde_json::json!({ "ok": true, "permission": PERMISSION_DEMO_READ })).into_response()
}

/// 写沙箱示例。
async fn demo_write(State(_state): State<Arc<SandboxState>>) -> Response {
    Json(serde_json::json!({ "ok": true, "permission": PERMISSION_DEMO_WRITE })).into_response()
}

/// 列出数据范围示例（**已被数据范围过滤**）。
async fn list_scope_items(
    State(state): State<Arc<SandboxState>>,
    TenantContext(ctx): TenantContext,
    Checker(checker): Checker,
    uri: OriginalUri,
) -> Response {
    let filter = checker.effective_scope(&ctx, PERMISSION_SCOPE_ITEM_READ);
    match state.scope_items().list(&ctx, &filter).await {
        Ok(items) => {
            let body: Vec<ScopeItemResponse> =
                items.into_iter().map(ScopeItemResponse::from).collect();
            Json(body).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "the scope item list failed");
            problem_response(&ProblemDetails::from_context(
                platform_core::problem::CODE_INTERNAL_ERROR,
                "服务内部错误",
                500,
                "the data access layer failed",
                &ctx,
                uri.path(),
                None,
            ))
        }
    }
}

/// 读数据范围示例（**不可见 = 不存在**：404，不泄露存在性）。
async fn get_scope_item(
    State(state): State<Arc<SandboxState>>,
    Path(id): Path<String>,
    TenantContext(ctx): TenantContext,
    Checker(checker): Checker,
    uri: OriginalUri,
) -> Response {
    let filter = checker.effective_scope(&ctx, PERMISSION_SCOPE_ITEM_READ);
    match state.scope_items().find(&ctx, &id, &filter).await {
        Ok(Some(item)) => Json(ScopeItemResponse::from(item)).into_response(),
        // 「不在数据范围内」与「不存在」**必须**是同一个结果（404）。
        Ok(None) => problem_response(&ProblemDetails::not_found(&ctx, uri.path(), "scope item")),
        Err(error) => {
            tracing::error!(error = %error, "the scope item lookup failed");
            problem_response(&ProblemDetails::from_context(
                platform_core::problem::CODE_INTERNAL_ERROR,
                "服务内部错误",
                500,
                "the data access layer failed",
                &ctx,
                uri.path(),
                None,
            ))
        }
    }
}

/// 审批数据范围示例（**属性级校验**：RBAC + ABAC 混合）。
///
/// 判定顺序：
///
/// 1. 有没有权限点 → 没有就是 **403**（`require_permission` 干的）；
/// 2. 记录在不在数据范围内 → 不在就是 **404**；
/// 3. 属性条件（`$record.amount <= 10000`）满不满足 → 不满足就是 **403**。
async fn approve_scope_item(
    State(state): State<Arc<SandboxState>>,
    TenantContext(ctx): TenantContext,
    Checker(checker): Checker,
    uri: OriginalUri,
) -> Response {
    let filter = checker.effective_scope(&ctx, PERMISSION_SCOPE_ITEM_APPROVE);
    let repository = state.scope_items();

    // 2) 数据范围：不可见 → 404（与「不存在」无法区分）。
    let Ok(Some(item)) = repository.find(&ctx, "item-under-review", &filter).await else {
        return problem_response(&ProblemDetails::not_found(&ctx, uri.path(), "scope item"));
    };

    // 3) 属性条件：不满足 → 403（动作类越权，契约 C5）。
    if let Some(condition) = checker.condition(&ctx, PERMISSION_SCOPE_ITEM_APPROVE) {
        let strict = match repository.build_find_query(&item.id, &filter) {
            Ok(query) => query,
            Err(error) => {
                tracing::error!(error = %error, "the scope filter could not be rendered");
                return problem_response(&ProblemDetails::from_context(
                    platform_core::problem::CODE_INTERNAL_ERROR,
                    "服务内部错误",
                    500,
                    "the scope filter could not be rendered",
                    &ctx,
                    uri.path(),
                    None,
                ));
            }
        };
        let strict = match strict.with_condition(&condition) {
            Ok(query) => query,
            Err(error) => {
                tracing::error!(error = %error, "the condition could not be rendered");
                return problem_response(&ProblemDetails::from_context(
                    platform_core::problem::CODE_INTERNAL_ERROR,
                    "服务内部错误",
                    500,
                    "the condition could not be rendered",
                    &ctx,
                    uri.path(),
                    None,
                ));
            }
        };
        let matched: Option<ScopeItem> =
            state.db.fetch_optional(&ctx, &strict).await.ok().flatten();
        if matched.is_none() {
            return problem_response(&ProblemDetails::insufficient_permission(
                &ctx,
                uri.path(),
                PERMISSION_SCOPE_ITEM_APPROVE,
            ));
        }
    }

    Json(serde_json::json!({ "approved": item.id })).into_response()
}

/// 导出数据范围示例。
async fn export_scope_items(
    State(state): State<Arc<SandboxState>>,
    TenantContext(ctx): TenantContext,
    Checker(checker): Checker,
    uri: OriginalUri,
) -> Response {
    let filter = checker.effective_scope(&ctx, PERMISSION_SCOPE_ITEM_EXPORT);
    match state.scope_items().list(&ctx, &filter).await {
        Ok(items) => {
            let body: Vec<ScopeItemResponse> =
                items.into_iter().map(ScopeItemResponse::from).collect();
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "the scope item export failed");
            problem_response(&ProblemDetails::from_context(
                platform_core::problem::CODE_INTERNAL_ERROR,
                "服务内部错误",
                500,
                "the data access layer failed",
                &ctx,
                uri.path(),
                None,
            ))
        }
    }
}

/// 让 `Request` 的导入不被判为未用（扩展注入在平台侧完成）。
#[allow(dead_code)]
fn _request_type(request: &Request) -> &Request {
    request
}

/// 空的范围（平台没注入判定器时的兜底 —— **一行都匹配不到**）。
#[allow(dead_code)]
fn _empty_filter() -> ScopeFilter {
    ScopeFilter::OrgNodes {
        column: "org_node_id".to_string(),
        nodes: Vec::new(),
    }
}
