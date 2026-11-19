//! `demoItem` 的 HTTP 层：**元数据驱动渲染的活样本**（REQ-012）。
//!
//! | 方法 + 路径 | 权限点 | 响应 |
//! |---|---|---|
//! | `GET /api/sandbox/demo-items` | `sandbox:demo:read` | `{ "items": [...], "nextCursor": string \| null }` |
//! | `POST /api/sandbox/demo-items` | `sandbox:demo:write` | 201 + 一条记录 |
//! | `PUT /api/sandbox/demo-items/{id}` | `sandbox:demo:write` | 200 + 一条记录 |
//! | `DELETE /api/sandbox/demo-items/{id}` | `sandbox:demo:write` | 204 |
//!
//! # 每条响应都经过 `SchemaProjector`
//!
//! 这是契约 C-6 要求的调用点，也是**唯一**允许把实体变成 JSON 的路径：
//!
//! ```text
//! DemoItemRecord::to_json()  --(含 internalNote / createdAt / secret)-->  SchemaProjector::project  -->  Json
//! ```
//!
//! 少了这一步，`secret`（字段级权限）与 `internalNote`（未写进元数据）都会泄露。
//!
//! # 信封为什么不整体投影
//!
//! `SchemaProjector` 的白名单只认**元数据声明的字段**。对信封整体投影会把
//! `nextCursor` 也剔掉 —— 前端永远只有一页。所以投影的粒度是**记录**
//! （[`SchemaProjector::project_items`]）。

use std::sync::Arc;

use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use platform_core::RequestContext;
use platform_core::extension::schema::SchemaProjector;
use platform_core::problem::{ProblemDetails, problem_response};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::demo_item::{DemoItemRecord, PgDemoItemRepository};
use crate::http::{Checker, SandboxState, TenantContext};

/// 列表的查询参数（游标分页，契约 C7）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListParams {
    /// 游标（不透明；缺省 = 从头开始）。
    #[serde(default)]
    pub cursor: Option<String>,
    /// 每页条数（缺省 20，上限 200）。
    #[serde(default)]
    pub limit: Option<u32>,
}

/// 列表响应信封（`nextCursor` 为 `null` 表示没有下一页）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEnvelope {
    /// 本页记录（**已逐条投影**）。
    pub items: Vec<Value>,
    /// 下一页游标。
    pub next_cursor: Option<String>,
}

/// 创建 / 更新的请求体。
///
/// `id` **不**出现在体里：创建时由服务端生成，更新时只在 URL 里。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DemoItemBody {
    /// 标题。
    pub title: String,
    /// 金额。
    #[serde(default)]
    pub amount: i64,
    /// 是否已付款。
    #[serde(default)]
    pub paid: bool,
    /// 到期日（`YYYY-MM-DD`）。
    #[serde(default)]
    pub due_at: Option<String>,
    /// 负责人 id。
    #[serde(default)]
    pub owner_id: String,
    /// 内部备注（写侧不做字段级权限校验 —— 本期只做读侧）。
    #[serde(default)]
    pub secret: Option<String>,
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
    let repository = PgDemoItemRepository::new(state.db.clone());
    match repository
        .list(&ctx, params.cursor.as_deref(), limit)
        .await
    {
        Ok((rows, next_cursor)) => {
            let mut items: Vec<Value> = rows.iter().map(DemoItemRecord::to_json).collect();
            // **逐条**投影：信封里的 `nextCursor` 不能被白名单剔掉。
            SchemaProjector::project_items(
                &mut items,
                &state.demo_item_schema,
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
    Json(body): Json<DemoItemBody>,
) -> Response {
    let tenant = match ctx.tenant_id() {
        Some(tenant) => tenant.to_string(),
        None => {
            return problem_response(&ProblemDetails::from_context(
                platform_core::problem::CODE_INTERNAL_ERROR,
                "服务内部错误",
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "the tenant is not bound in the request context",
                &ctx,
                uri.path(),
                None,
            ));
        }
    };
    let record = DemoItemRecord {
        tenant_id: tenant,
        id: new_id(),
        title: body.title,
        amount: body.amount,
        paid: body.paid,
        due_at: body.due_at,
        owner_id: body.owner_id,
        secret: body.secret,
        internal_note: None,
        created_at: now_rfc3339(),
    };

    let repository = PgDemoItemRepository::new(state.db.clone());
    if let Err(error) = repository.insert(&ctx, &record).await {
        return internal_error(&ctx, uri.path(), &error);
    }
    let projected = SchemaProjector::project(
        record.to_json(),
        &state.demo_item_schema,
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
    Json(body): Json<DemoItemBody>,
) -> Response {
    let repository = PgDemoItemRepository::new(state.db.clone());
    let existing = match repository.find(&ctx, &id).await {
        Ok(Some(record)) => record,
        // 「不在数据范围内」与「不存在」是同一个结果（404）。
        Ok(None) => return problem_response(&ProblemDetails::not_found(&ctx, uri.path(), "demo item")),
        Err(error) => return internal_error(&ctx, uri.path(), &error),
    };
    let updated = DemoItemRecord {
        title: body.title,
        amount: body.amount,
        paid: body.paid,
        due_at: body.due_at,
        owner_id: body.owner_id,
        // 请求体没给 `secret` → 保留原值（不因为一次更新把内部备注清空）。
        secret: body.secret.or(existing.secret),
        ..existing
    };
    if let Err(error) = repository.update(&ctx, &updated).await {
        return internal_error(&ctx, uri.path(), &error);
    }
    let projected = SchemaProjector::project(
        updated.to_json(),
        &state.demo_item_schema,
        &ctx,
        checker.as_ref(),
    );
    Json(projected).into_response()
}

/// 删除。
pub async fn delete(
    State(state): State<Arc<SandboxState>>,
    Path(id): Path<String>,
    TenantContext(ctx): TenantContext,
    uri: OriginalUri,
) -> Response {
    let repository = PgDemoItemRepository::new(state.db.clone());
    match repository.delete(&ctx, &id).await {
        Ok(0) => problem_response(&ProblemDetails::not_found(&ctx, uri.path(), "demo item")),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => internal_error(&ctx, uri.path(), &error),
    }
}

/// 服务内部错误（面向开发者的英文描述；中文文案由前端按 `code` 渲染）。
fn internal_error(ctx: &RequestContext, instance: &str, error: &anyhow::Error) -> Response {
    tracing::error!(error = %error, "the demo item request failed");
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
    format!("item-{}", uuid::Uuid::new_v4())
}

/// 当前 UTC 时刻的 RFC 3339 文本（契约 C13：时间一律 UTC 存）。
fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_unique() {
        assert_ne!(new_id(), new_id());
        assert!(new_id().starts_with("item-"));
    }

    #[test]
    fn unknown_body_keys_are_rejected() {
        let raw = r#"{ "title": "t", "unknown": 1 }"#;
        assert!(serde_json::from_str::<DemoItemBody>(raw).is_err());
    }

    #[test]
    fn the_body_only_requires_a_title() {
        let body: DemoItemBody =
            serde_json::from_str(r#"{ "title": "标题" }"#).expect("只给 title 应当合法");
        assert_eq!(body.amount, 0);
        assert!(!body.paid);
        assert!(body.secret.is_none());
    }
}
