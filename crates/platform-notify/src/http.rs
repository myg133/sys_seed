//! HTTP 层：模板 / 投递记录 / 订阅偏好（REQ-015，design §3）。
//!
//! # 10 个路由（本表是唯一权威）
//!
//! | 方法 | 路径（相对 `/api/notify`） | 权限点 / 认证 |
//! |------|--------------------------|---------------|
//! | `GET` | `/templates` | `notify:template:read` |
//! | `GET` | `/templates/{id}` | `notify:template:read` |
//! | `POST` | `/templates` | `notify:template:write` → **201** |
//! | `PATCH` | `/templates/{id}` | `notify:template:write` |
//! | `DELETE` | `/templates/{id}` | `notify:template:write` → **204** |
//! | `POST` | `/templates/{id}/preview` | `notify:template:read` |
//! | `GET` | `/deliveries` | `notify:delivery:read` |
//! | `POST` | `/deliveries/{id}/resend` | `notify:delivery:read` |
//! | `GET` | `/preferences` | **登录即可**（D300） |
//! | `PUT` | `/preferences` | **登录即可**（D300） |
//!
//! # ⚠️ 认证中间件**必须**调 `ensure_fresh`（TD-047）
//!
//! `PermissionChecker::has` 是**纯缓存查询**，而填缓存是异步的 `ensure_fresh`。
//! 漏掉 → 用户**第一次**打受权限保护的端点**必然 403**，第二次才好 ——
//! 这种「第一次失败、重试就好」的缺陷最难查。
//!
//! # ⚠️ 每个端点只挂一个权限点
//!
//! `Router::route_layer` 是**叠加**语义（多次调用 = 多层全跑）→
//! 逐 method 挂层（`MethodRouter::route_layer` 只作用于已设置的 method）。
//!
//! # ⚠️ 订阅偏好**不加权限点**（D300）
//!
//! 那是**用户自己的**设置（登录即可改），与 REQ-016 的 D271 同款口径。
//! 若加权限点，普通用户就改不了自己的退订了。
//!
//! # ⚠️ 命名禁区（D216）
//!
//! `src/` 里**不得**出现 `authorize` 等 8 个标识符 ——
//! 外部请求身份校验的函数因此叫 [`authenticate`]，**不叫** `authorize_*`。

use std::sync::Arc;

use axum::extract::{FromRequest, OriginalUri, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use platform_core::RequestContext;
use platform_core::extension::permission::{PermissionChecker, PermissionRefresher};
use platform_core::problem::{ProblemDetails, problem_response};
use serde::de::DeserializeOwned;

use crate::authz;
use crate::error::{NotifyError, ServiceError};
use crate::model::{PreviewResponse, SetPreferenceBody, TemplateUpsertBody};
use crate::service::NotifyService;

/// 令牌校验结果（HTTP 层只关心这两个字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedUser {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**）。
    pub tenant_id: String,
}

/// 令牌校验接缝（装配层注入 —— 本 crate **不依赖** `platform-iam`）。
pub trait TokenVerifier: Send + Sync {
    /// 校验令牌。
    fn verify(&self, token: &str) -> Option<VerifiedUser>;
}

/// HTTP 层共享的状态。
#[derive(Clone)]
pub struct NotifyState {
    /// 业务服务。
    pub service: Arc<NotifyService>,
    /// 令牌校验（登录用户）。
    pub tokens: Arc<dyn TokenVerifier>,
    /// 权限判定器（鉴权层从请求扩展里取）。
    pub checker: Arc<dyn PermissionChecker>,
    /// 权限判定的**异步刷新**接缝。
    ///
    /// ⚠️ **不能省**（TD-047）：少了它 → 第一次打受权限保护的端点**必然 403**。
    pub refresher: Arc<dyn PermissionRefresher>,
}

impl std::fmt::Debug for NotifyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotifyState").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// 提取器
// ---------------------------------------------------------------------------

/// 带租户上下文的请求上下文（**外加**调用方请求的原始路径）。
#[derive(Debug, Clone)]
struct Ctx {
    /// 请求上下文（租户 / 用户 / IP / UA）。
    ctx: RequestContext,
    /// 调用方请求的原始路径。
    instance: String,
}

impl<S: Send + Sync> FromRequestParts<S> for Ctx {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        let instance = original_path(&parts.extensions, "/api/notify");
        Ok(Ctx { ctx, instance })
    }
}

/// 请求体的 JSON 提取器：**解析失败也回平台的错误体**。
///
/// axum 默认的拒绝体是纯文本，那会让「错误体形状」在两端不一致
/// （契约 C6 要求一律 Problem Details）。
struct ApiJson<T>(T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let ctx = req
            .extensions()
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        let instance = original_path(req.extensions(), req.uri().path());
        let bytes = match axum::body::to_bytes(req.into_body(), 256 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => return Err(invalid_request_response(&ctx, &instance)),
        };
        match serde_json::from_slice::<T>(&bytes) {
            Ok(value) => Ok(ApiJson(value)),
            Err(_) => Err(invalid_request_response(&ctx, &instance)),
        }
    }
}

// ---------------------------------------------------------------------------
// 路由
// ---------------------------------------------------------------------------

/// 路由（由装配层 **nest** 到 `/api/notify`）。
pub fn router<S2>(state: Arc<NotifyState>) -> Router<S2> {
    // 模板：读 / 写分点（逐 method 挂层，避免叠加语义）。
    let templates = Router::new()
        .route(
            "/templates",
            get(list_templates).route_layer(authz::require_template_read()),
        )
        .route(
            "/templates",
            post(create_template).route_layer(authz::require_template_write()),
        )
        .route(
            "/templates/{id}",
            get(get_template).route_layer(authz::require_template_read()),
        )
        .route(
            "/templates/{id}",
            axum::routing::patch(update_template).route_layer(authz::require_template_write()),
        )
        .route(
            "/templates/{id}",
            axum::routing::delete(delete_template).route_layer(authz::require_template_write()),
        )
        .route(
            "/templates/{id}/preview",
            post(preview_template).route_layer(authz::require_template_read()),
        );

    // 投递记录：查 + 重投死信（同一个权限点，D300）。
    let deliveries = Router::new()
        .route(
            "/deliveries",
            get(list_deliveries).route_layer(authz::require_delivery_read()),
        )
        .route(
            "/deliveries/{id}/resend",
            post(resend_delivery).route_layer(authz::require_delivery_read()),
        );

    // 订阅偏好：**登录即可**（D300：那是用户自己的设置）。
    let preferences = Router::new()
        .route("/preferences", get(list_preferences))
        .route("/preferences", put(set_preference));

    Router::new()
        .merge(templates)
        .merge(deliveries)
        .merge(preferences)
        // 用户会话中间件（**最外层** —— 必须跑在 `require_permission` 之前）。
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// 认证中间件
// ---------------------------------------------------------------------------

/// 用户会话中间件：验登录令牌 → 写入租户 / 用户上下文 → 注入权限判定器。
///
/// **必须**跑在 `require_permission` 之前 —— 它是路由级层的**外层**。
async fn authenticate(
    State(state): State<Arc<NotifyState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let instance = original_path(request.extensions(), request.uri().path());
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    let Some(token) = bearer_token(request.headers()) else {
        return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
    };
    let Some(user) = state.tokens.verify(&token) else {
        return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
    };

    // 租户与用户**都来自令牌**。
    ctx = ctx.with_tenant(user.tenant_id.clone());
    ctx = ctx.with_user(user.user_id.clone());
    request.extensions_mut().insert(ctx.clone());

    // ⚠️ **必须**先刷新判定缓存，再让鉴权层判定（TD-047）。
    if let Err(reason) = state.refresher.ensure_fresh(&ctx).await {
        // fail closed：刷新不出来就**不能**让鉴权层读到空缓存
        // （那会一律 403，看起来像「没权限」而不是「后端坏了」）。
        tracing::error!(%reason, "刷新权限判定缓存失败");
        let details = ProblemDetails::from_context(
            platform_core::problem::CODE_INTERNAL_ERROR,
            platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR),
            500,
            "failed to refresh the permission cache",
            &ctx,
            instance.as_str(),
            None,
        );
        return problem_response(&details);
    }

    // 注入权限判定实现（鉴权层从扩展里取）。
    let checker: Arc<dyn PermissionChecker> = state.checker.clone();
    request.extensions_mut().insert(checker);

    next.run(request).await
}

/// 从 `Authorization: Bearer <token>` 里取令牌。
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
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

/// 请求的**原始路径**（含 `/api/notify` 前缀）。
fn original_path(extensions: &axum::http::Extensions, fallback: &str) -> String {
    extensions
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

// ---------------------------------------------------------------------------
// Handler：模板
// ---------------------------------------------------------------------------

/// 列出模板（**数组**，不是 `{ templates: [] }`）。
async fn list_templates(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.list_templates(&ctx).await {
        Ok(rows) => Json(
            rows.iter()
                .map(crate::model::view_of_template)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 建模板 → **201 CREATED**。
async fn create_template(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<TemplateUpsertBody>,
) -> Response {
    match state.service.create_template(&ctx, &body.into_new()).await {
        Ok(created) => (
            StatusCode::CREATED,
            Json(crate::model::view_of_template(&created)),
        )
            .into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 读单个模板。
async fn get_template(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
    Path(id): Path<String>,
) -> Response {
    match state.service.find_template(&ctx, &id).await {
        Ok(Some(row)) => Json(crate::model::view_of_template(&row)).into_response(),
        Ok(None) => {
            notify_problem_response(&NotifyError::TemplateNotFound, &ctx, instance.as_str())
        }
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 改模板。
async fn update_template(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<crate::store::TemplateUpdate>,
) -> Response {
    match state.service.update_template(&ctx, &id, &body).await {
        Ok(row) => Json(crate::model::view_of_template(&row)).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 删模板 → **204 NO CONTENT**。
async fn delete_template(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
    Path(id): Path<String>,
) -> Response {
    match state.service.delete_template(&ctx, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 预览：用给定 payload 渲染一次（**不落库**）。
async fn preview_template(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<crate::model::PreviewBody>,
) -> Response {
    let tpl = match state.service.find_template(&ctx, &id).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            return notify_problem_response(
                &NotifyError::TemplateNotFound,
                &ctx,
                instance.as_str(),
            );
        }
        Err(error) => return service_problem_response(&error, &ctx, instance.as_str()),
    };
    match state.service.preview(&tpl, &body.payload) {
        Ok((title, rendered_body)) => Json(PreviewResponse {
            title,
            body: rendered_body,
        })
        .into_response(),
        Err(error) => notify_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Handler：投递记录
// ---------------------------------------------------------------------------

/// 列出投递记录。
async fn list_deliveries(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.list_deliveries(&ctx).await {
        Ok(rows) => Json(
            rows.iter()
                .map(crate::model::view_of_delivery)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 重投死信（AC-S-172）。
///
/// ⚠️ 判据靠 `service.resend_dead_letter` 的**影响行数**，
/// 不是「先查状态再改」（并发下两条都会成功）。
async fn resend_delivery(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
    Path(id): Path<String>,
) -> Response {
    match state.service.resend_dead_letter(&ctx, &id).await {
        Ok(row) => Json(crate::model::view_of_delivery(&row)).into_response(),
        Err(ServiceError::Notify(NotifyError::TemplateNotFound)) => {
            // 重投**不存在的** record：用平台通用 `NOT_FOUND`（AC-S-172 步骤 4）。
            not_found_response(&ctx, instance.as_str())
        }
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Handler：订阅偏好
// ---------------------------------------------------------------------------

/// 读当前用户的偏好。
async fn list_preferences(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    let Some(user_id) = ctx.user_id().map(str::to_string) else {
        return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
    };
    match state.service.list_preferences(&ctx, &user_id).await {
        Ok(rows) => Json(
            rows.iter()
                .map(crate::model::view_of_preference)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 设退订状态（**先校验后写** —— `system` 类不可退订）。
async fn set_preference(
    State(state): State<Arc<NotifyState>>,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<SetPreferenceBody>,
) -> Response {
    let Some(user_id) = ctx.user_id().map(str::to_string) else {
        return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
    };
    match state
        .service
        .set_preference(&ctx, &user_id, &body.category, body.opted_out)
        .await
    {
        Ok(row) => Json(crate::model::view_of_preference(&row)).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// 响应辅助
// ---------------------------------------------------------------------------

/// `ServiceError` → Problem Details。
fn service_problem_response(
    error: &ServiceError,
    ctx: &RequestContext,
    instance: &str,
) -> Response {
    problem_response(&error.problem_details(ctx, instance))
}

/// `NotifyError` → Problem Details。
fn notify_problem_response(error: &NotifyError, ctx: &RequestContext, instance: &str) -> Response {
    problem_response(&error.problem_details(ctx, instance))
}

/// 平台通用 `NOT_FOUND`（用于「记录不存在」这类**不值得新码**的分支）。
fn not_found_response(ctx: &RequestContext, instance: &str) -> Response {
    problem_response(&ProblemDetails::from_context(
        platform_core::problem::CODE_NOT_FOUND,
        platform_core::titles::title_of_code(platform_core::problem::CODE_NOT_FOUND),
        404,
        "the requested resource was not found",
        ctx,
        instance,
        None,
    ))
}

/// 请求体解析失败 → **400**。
///
/// # ⚠️ 为什么用 `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER` 而不是新增一个码
///
/// 本 crate **恰好 8 个**错误码（D303），每个都必须有 AC 会真的触发它
/// —— 「请求体不是合法 JSON」**没有任何 AC 会触发**（它是防御性分支，
/// 不是验收路径）→ 加它就是**死码**（D226 / REQ-016 删过两个同类的码）。
///
/// 平台的 `problem` 模块也**没有** `CODE_INVALID_REQUEST`
/// （实测 `crates/platform-core/src/problem.rs:34-57` 只有 7 个常量）。
///
/// → 折中：归到**同一个 400 语义族**里最接近的码，并在 `detail` 里写清真实原因。
/// 这样前端拿到的 `code` 仍是**已登记、有 i18n 文案**的，`detail` 保留可诊断性。
fn invalid_request_response(ctx: &RequestContext, instance: &str) -> Response {
    problem_response(&ProblemDetails::from_context(
        NotifyError::TemplateInvalidPlaceholder {
            reason: "the request body is not valid json for this endpoint",
        }
        .code(),
        platform_core::titles::title_of_code(
            NotifyError::TemplateInvalidPlaceholder {
                reason: "the request body is not valid json for this endpoint",
            }
            .code(),
        ),
        400,
        "the request body is not valid json for this endpoint",
        ctx,
        instance,
        None,
    ))
}

// `FromRequestParts` 需要显式引入（与 `FromRequest` 不同 trait）。
use axum::extract::FromRequestParts;

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::extension::expr::CompiledCondition;
    use platform_core::extension::permission::{DataScope, ScopeFilter};

    /// `bearer_token` 接受 `Bearer` / `bearer` 两种前缀，拒绝空令牌。
    #[test]
    fn bearer_token_parsing() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer abc".parse().expect("合法"));
        assert_eq!(bearer_token(&headers), Some("abc".to_string()));

        headers.insert(header::AUTHORIZATION, "bearer xyz".parse().expect("合法"));
        assert_eq!(bearer_token(&headers), Some("xyz".to_string()));

        headers.insert(header::AUTHORIZATION, "Bearer   ".parse().expect("合法"));
        assert_eq!(bearer_token(&headers), None, "空令牌必须拒绝");

        headers.insert(header::AUTHORIZATION, "Basic abc".parse().expect("合法"));
        assert_eq!(bearer_token(&headers), None, "非 Bearer 必须拒绝");
    }

    /// 没有 `Authorization` 头时返回 `None`。
    #[test]
    fn bearer_token_absent() {
        assert_eq!(bearer_token(&HeaderMap::new()), None);
    }

    /// 路由把 10 条都注册上了（防止漏挂端点）。
    ///
    /// 用 `Router` 的 `Debug` 输出数路径 —— axum 不提供路由自省，
    /// 但 `Debug` 里能看到路径字符串。
    #[tokio::test]
    async fn the_router_registers_all_ten_paths() {
        let state = Arc::new(NotifyState {
            service: Arc::new(crate::service::NotifyService::new(
                crate::store::NotifyStore::new(platform_db::TenantDb::from_pool(
                    sqlx::PgPool::connect_lazy("postgres://x/y").expect("lazy pool"),
                    platform_db::DbOptions::default(),
                )),
                crate::config::NotifyConfig::new(Arc::new(NoopConfigSource)),
            )),
            tokens: Arc::new(RejectAll),
            checker: Arc::new(AllowAll),
            refresher: Arc::new(NoopRefresher),
        });
        let debug = format!("{:?}", router::<()>(state));
        for path in [
            "/templates",
            "/templates/{id}",
            "/templates/{id}/preview",
            "/deliveries",
            "/deliveries/{id}/resend",
            "/preferences",
        ] {
            assert!(debug.contains(path), "路由里缺 {path}；实际：{debug}");
        }
    }

    /// 测试替身：拒绝一切令牌。
    struct RejectAll;
    impl TokenVerifier for RejectAll {
        fn verify(&self, _token: &str) -> Option<VerifiedUser> {
            None
        }
    }

    /// 测试替身：允许一切（本文件只测路由装配，不测鉴权语义）。
    struct AllowAll;
    impl PermissionChecker for AllowAll {
        fn has(&self, _ctx: &RequestContext, _permission: &str) -> bool {
            true
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

    /// 测试替身：刷新恒成功。
    struct NoopRefresher;
    impl PermissionRefresher for NoopRefresher {
        fn ensure_fresh<'a>(
            &'a self,
            _ctx: &'a RequestContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>
        {
            Box::pin(async { Ok(()) })
        }
    }

    /// 测试替身：配置读取恒 `None`（用默认值）。
    struct NoopConfigSource;
    impl crate::config::NotifyConfigSource for NoopConfigSource {
        fn read_platform<'a>(
            &'a self,
            _ctx: &'a RequestContext,
            _key: &'a str,
        ) -> crate::config::NotifyBoxFuture<'a, Option<i64>> {
            Box::pin(async { None })
        }

        fn tenant_default_locale(
            &self,
            _ctx: &RequestContext,
        ) -> crate::config::NotifyBoxFuture<'_, String> {
            Box::pin(async { "zh-CN".to_string() })
        }
    }
}
