//! `demoNote` 的 HTTP 层（REQ-012 D107）。
//!
//! | 方法 + 路径 | 权限点 | 响应 |
//! |---|---|---|
//! | `GET /api/sandbox/demo-notes` | `sandbox:demo:read` | `{ "items": [...], "nextCursor": string \| null }` |
//! | `POST /api/sandbox/demo-notes` | `sandbox:demo:write` | 201 + 一条记录 |
//! | `PUT /api/sandbox/demo-notes/{id}` | `sandbox:demo:write` | 200 + 一条记录 |
//! | `DELETE /api/sandbox/demo-notes/{id}` | `sandbox:demo:write` | 204 |
//!
//! # 与 `demoItem` 的 HTTP 层是**同款**
//!
//! 列表信封、游标分页、404 语义、投影位置全部一致 —— 因为这就是「同类页面」。
//! 元数据驱动的价值不在「少写几行后端」，而在**前端一行不改**就能多出一个页面。
//!
//! # 权限点复用而不是新建
//!
//! `sandbox:demo:read` / `sandbox:demo:write` 表达的是「沙箱示例数据的读 / 写」，
//! 第二个实体属于同一个语义面。为它另造 `sandbox:demo-note:*` 只会让权限表
//! 变长，使用者还得为「本来一回事」的权限配两遍。
//!
//! # 每条响应都经过 `SchemaProjector`
//!
//! 契约 C-6 的调用点。本实体的字段与元数据一一对应，投影是恒等的 ——
//! 依然要调用：这是**唯一**允许把实体变成 JSON 的路径，跳过它就等于给未来
//! 加字段级权限的人埋了个泄露口。

use std::sync::Arc;

use axum::Json;
use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use platform_core::RequestContext;
use platform_core::extension::schema::SchemaProjector;
use platform_core::problem::{ProblemDetails, problem_response};
use serde::Deserialize;
use serde_json::Value;

use crate::demo_item_http::{ListEnvelope, ListParams};
use crate::demo_note::{DemoNoteRecord, PgDemoNoteRepository};
use crate::http::{Checker, SandboxState, TenantContext};

/// 创建 / 更新的请求体。
///
/// `id` **不**出现在体里：创建时由服务端生成，更新时只在 URL 里。
/// `title` / `content` 是元数据里的 `required: true` —— 缺了就是 422（axum 的拒绝），
/// 不是「悄悄存成空串」。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DemoNoteBody {
    /// 标题。
    pub title: String,
    /// 正文。
    pub content: String,
    /// 是否置顶。
    #[serde(default)]
    pub pinned: bool,
    /// 标签（逗号分隔）。
    #[serde(default)]
    pub tags: Option<String>,
}

/// 列表。
pub async fn list(
    State(state): State<Arc<SandboxState>>,
    TenantContext(ctx): TenantContext,
    Checker(checker): Checker,
    Query(params): Query<ListParams>,
    uri: OriginalUri,
) -> Response {
    let limit = params.limit.unwrap_or(20).clamp(1, 200);
    let repository = PgDemoNoteRepository::new(state.db.clone());
    match repository.list(&ctx, params.cursor.as_deref(), limit).await {
        Ok((rows, next_cursor)) => {
            let mut items: Vec<Value> = rows.iter().map(DemoNoteRecord::to_json).collect();
            // **逐条**投影：信封里的 `nextCursor` 不能被白名单剔掉。
            SchemaProjector::project_items(
                &mut items,
                &state.demo_note_schema,
                &ctx,
                checker.as_ref(),
            );
            Json(ListEnvelope { items, next_cursor }).into_response()
        }
        Err(error) => internal_error(&ctx, uri.path(), &error),
    }
}

/// 创建。
pub async fn create(
    State(state): State<Arc<SandboxState>>,
    TenantContext(ctx): TenantContext,
    Checker(checker): Checker,
    uri: OriginalUri,
    Json(body): Json<DemoNoteBody>,
) -> Response {
    let Some(tenant) = ctx.tenant_id().map(str::to_string) else {
        return problem_response(&ProblemDetails::from_context(
            platform_core::problem::CODE_INTERNAL_ERROR,
            "服务内部错误",
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            "the tenant is not bound in the request context",
            &ctx,
            uri.path(),
            None,
        ));
    };
    let record = DemoNoteRecord {
        tenant_id: tenant,
        id: new_id(),
        title: body.title,
        content: body.content,
        pinned: body.pinned,
        tags: body.tags,
    };

    let repository = PgDemoNoteRepository::new(state.db.clone());
    if let Err(error) = repository.insert(&ctx, &record).await {
        return internal_error(&ctx, uri.path(), &error);
    }
    let projected = SchemaProjector::project(
        record.to_json(),
        &state.demo_note_schema,
        &ctx,
        checker.as_ref(),
    );
    (StatusCode::CREATED, Json(projected)).into_response()
}

/// 更新（整体替换）。
pub async fn update(
    State(state): State<Arc<SandboxState>>,
    Path(id): Path<String>,
    TenantContext(ctx): TenantContext,
    Checker(checker): Checker,
    uri: OriginalUri,
    Json(body): Json<DemoNoteBody>,
) -> Response {
    let repository = PgDemoNoteRepository::new(state.db.clone());
    match repository.find(&ctx, &id).await {
        // 「不在数据范围内」与「不存在」是同一个结果（404）。
        Ok(None) => problem_response(&ProblemDetails::not_found(&ctx, uri.path(), "demo note")),
        Err(error) => internal_error(&ctx, uri.path(), &error),
        Ok(Some(existing)) => {
            let updated = DemoNoteRecord {
                title: body.title,
                content: body.content,
                pinned: body.pinned,
                // 请求体没给 `tags` → 保留原值（不因为一次更新把标签清空）；
                // 要给空标签就显式传 `"tags": null`… 由 `Option` 的 `Some("")` 表达空串。
                tags: body.tags.or(existing.tags),
                ..existing
            };
            if let Err(error) = repository.update(&ctx, &updated).await {
                return internal_error(&ctx, uri.path(), &error);
            }
            let projected = SchemaProjector::project(
                updated.to_json(),
                &state.demo_note_schema,
                &ctx,
                checker.as_ref(),
            );
            Json(projected).into_response()
        }
    }
}

/// 删除。
pub async fn delete(
    State(state): State<Arc<SandboxState>>,
    Path(id): Path<String>,
    TenantContext(ctx): TenantContext,
    uri: OriginalUri,
) -> Response {
    let repository = PgDemoNoteRepository::new(state.db.clone());
    match repository.delete(&ctx, &id).await {
        Ok(0) => problem_response(&ProblemDetails::not_found(&ctx, uri.path(), "demo note")),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => internal_error(&ctx, uri.path(), &error),
    }
}

/// 服务内部错误（面向开发者的英文描述；中文文案由前端按 `code` 渲染）。
fn internal_error(ctx: &RequestContext, instance: &str, error: &anyhow::Error) -> Response {
    tracing::error!(error = %error, "the demo note request failed");
    problem_response(&ProblemDetails::from_context(
        platform_core::problem::CODE_INTERNAL_ERROR,
        "服务内部错误",
        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        "the data access layer failed",
        ctx,
        instance,
        None,
    ))
}

/// 新主键（租户内唯一即可；不需要全局有序）。
fn new_id() -> String {
    format!("note-{}", uuid::Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_unique_and_prefixed() {
        assert_ne!(new_id(), new_id());
        assert!(new_id().starts_with("note-"));
    }

    #[test]
    fn unknown_body_keys_are_rejected() {
        let raw = r#"{ "title": "t", "content": "c", "unknown": 1 }"#;
        assert!(serde_json::from_str::<DemoNoteBody>(raw).is_err());
    }

    #[test]
    fn the_body_requires_title_and_content() {
        assert!(serde_json::from_str::<DemoNoteBody>(r#"{ "title": "t" }"#).is_err());
        assert!(serde_json::from_str::<DemoNoteBody>(r#"{ "content": "c" }"#).is_err());
    }

    #[test]
    fn the_body_optional_fields_default() {
        let body: DemoNoteBody = serde_json::from_str(r#"{ "title": "t", "content": "c" }"#)
            .expect("只给 title + content 应当合法");
        assert!(!body.pinned);
        assert!(body.tags.is_none());
    }
}
