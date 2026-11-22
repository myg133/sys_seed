//! HTTP 层：对象 / 上传会话 / 导入导出任务 / 预签名下载（REQ-018，design §3）。
//!
//! # 12 个路由（本表是唯一权威）
//!
//! | 方法 | 路径（相对 `/api/storage`） | 权限点 / 认证 |
//! |------|--------------------------|---------------|
//! | `PUT` | `/objects/{key}` | `storage:file:write` |
//! | `GET` | `/objects/{key}` | `storage:file:read` |
//! | `DELETE` | `/objects/{key}` | `storage:file:write` → **204** |
//! | `GET` | `/objects` | `storage:file:read` |
//! | `POST` | `/objects/{key}/presign` | `storage:file:read` |
//! | `POST` | `/uploads` | `storage:file:write` → **201** |
//! | `PUT` | `/uploads/{id}/parts/{n}` | `storage:file:write` |
//! | `POST` | `/uploads/{id}/complete` | `storage:file:write` |
//! | `DELETE` | `/uploads/{id}` | `storage:file:write` → **204** |
//! | `POST` | `/imports` · `/exports` | `storage:file:write` → **202** |
//! | `GET` | `/jobs` · `/jobs/{id}` · `/jobs/{id}/errors` | `storage:job:read` |
//! | `GET` | `/download/{token}` | **免鉴权**（靠签名，D322） |
//!
//! # ⚠️ `multipart` 是第二次依赖破例（D321）
//!
//! 大文件走 `multipart/form-data`；小文件也接受裸 body（`PUT /objects/{key}` 直接收字节）。
//!
//! # ⚠️ `download/{token}` 是**免鉴权**的
//!
//! 它靠 HMAC 签名（含 `object_key` + `expires_at`），**不读会话**。
//! 这正是预签名的用途：把「临时、限时、单资源」的访问权交给不持有会话的客户端
//! （如 `<img src>` / 浏览器直下）。
//!
//! # ⚠️ 认证中间件**必须**调 `ensure_fresh`（TD-047）
//!
//! `PermissionChecker::has` 是纯缓存查询；漏调 → 用户**第一次**打受保护端点**必然 403**。

use std::sync::Arc;

use axum::extract::{FromRequest, Multipart, OriginalUri, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use bytes::Bytes;
use platform_core::RequestContext;
use platform_core::extension::permission::{PermissionChecker, PermissionRefresher};
use platform_core::problem::{ProblemDetails, problem_response};
use serde::de::DeserializeOwned;

use crate::authz;
use crate::error::ServiceError;
use crate::model::{
    CompleteUploadBody, CreateJobBody, CreateUploadBody, PresignBody, PresignView, view_of_job,
    view_of_job_error, view_of_object,
};
use crate::service::StorageService;

/// 令牌校验结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedUser {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**）。
    pub tenant_id: String,
}

/// 令牌校验接缝（装配层注入 —— 本 crate 不依赖 `platform-iam`）。
pub trait TokenVerifier: Send + Sync {
    /// 校验令牌。
    fn verify(&self, token: &str) -> Option<VerifiedUser>;
}

/// HTTP 层共享的状态。
#[derive(Clone)]
pub struct StorageState {
    /// 业务服务。
    pub service: Arc<StorageService>,
    /// 令牌校验。
    pub tokens: Arc<dyn TokenVerifier>,
    /// 权限判定器。
    pub checker: Arc<dyn PermissionChecker>,
    /// 权限判定的异步刷新接缝（**不能省**，TD-047）。
    pub refresher: Arc<dyn PermissionRefresher>,
}

impl std::fmt::Debug for StorageState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageState").finish_non_exhaustive()
    }
}

/// 带租户上下文的请求上下文 + 原始路径。
#[derive(Debug, Clone)]
struct Ctx {
    /// 请求上下文。
    ctx: RequestContext,
    /// 原始路径。
    instance: String,
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Ctx {
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
        let instance = original_path(&parts.extensions, "/api/storage");
        Ok(Ctx { ctx, instance })
    }
}

/// JSON 提取器：解析失败也回平台的错误体。
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
        let bytes = match axum::body::to_bytes(req.into_body(), 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => return Err(invalid_request_response(&ctx, &instance)),
        };
        match serde_json::from_slice::<T>(&bytes) {
            Ok(value) => Ok(ApiJson(value)),
            Err(_) => Err(invalid_request_response(&ctx, &instance)),
        }
    }
}

/// 路由（由装配层 **nest** 到 `/api/storage`）。
pub fn router<S2>(state: Arc<StorageState>) -> Router<S2> {
    // ⚠️ 免鉴权端点必须在**最外层**（不经过 `authenticate` 中间件）。
    let download = Router::new().route("/download/{token}", get(download_by_token));

    let authed = Router::new()
        .route(
            "/objects",
            get(list_objects).route_layer(authz::require_file_read()),
        )
        .route(
            "/objects/{key}",
            put(put_object).route_layer(authz::require_file_write()),
        )
        .route(
            "/objects/{key}",
            get(get_object).route_layer(authz::require_file_read()),
        )
        .route(
            "/objects/{key}",
            axum::routing::delete(delete_object).route_layer(authz::require_file_write()),
        )
        .route(
            "/objects/{key}/presign",
            post(presign_object).route_layer(authz::require_file_read()),
        )
        .route(
            "/uploads",
            post(create_upload).route_layer(authz::require_file_write()),
        )
        .route(
            "/uploads/{id}/parts/{n}",
            put(upload_part).route_layer(authz::require_file_write()),
        )
        .route(
            "/uploads/{id}/complete",
            post(complete_upload).route_layer(authz::require_file_write()),
        )
        .route(
            "/uploads/{id}",
            axum::routing::delete(abort_upload).route_layer(authz::require_file_write()),
        )
        .route(
            "/imports",
            post(create_import).route_layer(authz::require_file_write()),
        )
        .route(
            "/exports",
            post(create_export).route_layer(authz::require_file_write()),
        )
        .route(
            "/jobs",
            get(list_jobs).route_layer(authz::require_job_read()),
        )
        .route(
            "/jobs/{id}",
            get(get_job).route_layer(authz::require_job_read()),
        )
        .route(
            "/jobs/{id}/errors",
            get(list_job_errors).route_layer(authz::require_job_read()),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ));

    download.merge(authed).with_state(state)
}

/// 用户会话中间件。
async fn authenticate(
    State(state): State<Arc<StorageState>>,
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

    ctx = ctx.with_tenant(user.tenant_id.clone());
    ctx = ctx.with_user(user.user_id.clone());
    request.extensions_mut().insert(ctx.clone());

    // ⚠️ **必须**先刷新判定缓存（TD-047）。
    if let Err(reason) = state.refresher.ensure_fresh(&ctx).await {
        tracing::error!(%reason, "刷新权限判定缓存失败");
        return problem_response(&ProblemDetails::from_context(
            platform_core::problem::CODE_INTERNAL_ERROR,
            platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR),
            500,
            "failed to refresh the permission cache",
            &ctx,
            instance.as_str(),
            None,
        ));
    }

    let checker: Arc<dyn PermissionChecker> = state.checker.clone();
    request.extensions_mut().insert(checker);
    next.run(request).await
}

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

fn original_path(extensions: &axum::http::Extensions, fallback: &str) -> String {
    extensions
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

// ---------------------------------------------------------------------------
// Handler：对象
// ---------------------------------------------------------------------------

/// 列对象。
async fn list_objects(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
) -> Response {
    match state.service.list_objects(&ctx).await {
        Ok(rows) => Json(rows.iter().map(view_of_object).collect::<Vec<_>>()).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 上传对象。
///
/// # ⚠️ 两种 body
///
/// - **裸字节**（`Content-Type` 不是 multipart）→ 直接当文件内容；
/// - **multipart/form-data**（D321）→ 取第一个名为 `file` 的字段。
///
/// 两条路径都走同一套「大小校验 → 扫描 → 落存储 → 落库」。
async fn put_object(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    Path(key): Path<String>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let is_multipart = content_type.starts_with("multipart/form-data");
    let body = if is_multipart {
        // ⚠️ multipart 的解析在这里（`axum::extract::Multipart`）。
        let mut multipart = match Multipart::from_request(request, &state).await {
            Ok(m) => m,
            Err(err) => {
                return service_problem_response(
                    &ServiceError::Internal {
                        reason: format!("the multipart body is malformed: {err}"),
                    },
                    &ctx,
                    instance.as_str(),
                );
            }
        };
        let mut found: Option<Bytes> = None;
        loop {
            match multipart.next_field().await {
                Ok(Some(field)) => {
                    let name = field.name().unwrap_or("").to_string();
                    if name == "file" || found.is_none() {
                        match field.bytes().await {
                            Ok(data) => found = Some(data),
                            Err(err) => {
                                return service_problem_response(
                                    &ServiceError::Internal {
                                        reason: format!(
                                            "reading the multipart field failed: {err}"
                                        ),
                                    },
                                    &ctx,
                                    instance.as_str(),
                                );
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    return service_problem_response(
                        &ServiceError::Internal {
                            reason: format!("reading the multipart body failed: {err}"),
                        },
                        &ctx,
                        instance.as_str(),
                    );
                }
            }
        }
        match found {
            Some(data) => data,
            None => Bytes::new(),
        }
    } else {
        match axum::body::to_bytes(request.into_body(), 256 * 1024 * 1024).await {
            Ok(data) => data,
            Err(_) => {
                return service_problem_response(
                    &ServiceError::Internal {
                        reason: "reading the request body failed".to_string(),
                    },
                    &ctx,
                    instance.as_str(),
                );
            }
        }
    };

    match state
        .service
        .put_object(&ctx, &key, body, &content_type)
        .await
    {
        Ok(row) => Json(view_of_object(&row)).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 读对象（返回裸字节）。
async fn get_object(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    Path(key): Path<String>,
) -> Response {
    match state.service.get_object(&ctx, &key).await {
        Ok((row, bytes)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, row.content_type.clone())],
            bytes,
        )
            .into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 删对象 → **204**。
async fn delete_object(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    Path(key): Path<String>,
) -> Response {
    match state.service.delete_object(&ctx, &key).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 预签名。
async fn presign_object(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    Path(key): Path<String>,
    ApiJson(body): ApiJson<PresignBody>,
) -> Response {
    match state
        .service
        .presign_object(&ctx, &key, Some(body.ttl_seconds))
        .await
    {
        Ok((url, expires_at)) => Json(PresignView {
            url,
            expires_at: platform_core::extension::audit::format_timestamp_micros_utc(&expires_at),
        })
        .into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 免鉴权下载（靠签名）。
async fn download_by_token(
    State(state): State<Arc<StorageState>>,
    Path(token): Path<String>,
) -> Response {
    // ⚠️ 这里**没有** `Ctx` —— 免鉴权端点的租户从「签名验证通过的 key」反推。
    // 但读对象需要一个租户上下文 → 用 platform 伪租户 + 直接走存储层。
    //
    // 真实做法（本轮）：验证 token → 拿到 key → 用**管理员连接**读（绕过 RLS）。
    // ⚠️ 这不是漏洞：token 本身就**是**授权凭据（含 key + 过期时间，且签名覆盖两者）。
    let key = match state.service.verify_presign(&token).await {
        Ok(key) => key,
        Err(error) => {
            let ctx = RequestContext::new();
            return service_problem_response(&error, &ctx, "/api/storage/download");
        }
    };
    match state.service.objects().get(&key).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, download_content_type(&key))],
            bytes,
        )
            .into_response(),
        Err(error) => {
            let ctx = RequestContext::new();
            service_problem_response(&ServiceError::Storage(error), &ctx, "/api/storage/download")
        }
    }
}

/// 预签名下载的 `Content-Type`（**按 key 的扩展名白名单映射**）。
///
/// # 为什么不读 `platform_storage_object.content_type`
///
/// 这个端点**没有租户上下文**（免鉴权，靠签名授权）→ 走不了 `TenantQuery`，
/// 读不到那条元数据。见 **TD-065**。
///
/// # ⚠️ 为什么是白名单而不是「扩展名 → 任意类型」
///
/// 若把 `.html` 映射成 `text/html`，攻击者上传一个 HTML 文件再用预签名链接分享，
/// 就得到一个**同源**的钓鱼页面（存储型 XSS 的经典形态）。
/// 所以这里**只**返回两种类型：CSV（AC-S-182 步骤 4 要求）与通用的二进制流。
/// 其余一律 `application/octet-stream` 强制下载，绝不让浏览器渲染。
fn download_content_type(key: &str) -> String {
    match key.rsplit('.').next() {
        Some(ext) if ext.eq_ignore_ascii_case("csv") => "text/csv".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Handler：上传会话
// ---------------------------------------------------------------------------

/// 建上传会话 → **201**。
async fn create_upload(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<CreateUploadBody>,
) -> Response {
    match state.service.create_upload(&ctx, &body).await {
        Ok(row) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "uploadId": row.upload_id,
                "totalParts": row.total_parts,
                "partSize": row.part_size,
            })),
        )
            .into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 上传一个分片。
async fn upload_part(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    Path((upload_id, part_number)): Path<(String, u32)>,
    request: Request,
) -> Response {
    let bytes = match axum::body::to_bytes(request.into_body(), 256 * 1024 * 1024).await {
        Ok(data) => data,
        Err(_) => {
            return service_problem_response(
                &ServiceError::Internal {
                    reason: "reading the part body failed".to_string(),
                },
                &ctx,
                instance.as_str(),
            );
        }
    };
    match state
        .service
        .upload_part(&ctx, &upload_id, part_number, bytes)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 完成上传。
async fn complete_upload(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    Path(upload_id): Path<String>,
    body: Option<Json<CompleteUploadBody>>,
) -> Response {
    let content_type = body.and_then(|Json(b)| b.content_type);
    match state
        .service
        .complete_upload(&ctx, &upload_id, content_type.as_deref())
        .await
    {
        Ok(row) => Json(view_of_object(&row)).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 中止上传 → **204**。
async fn abort_upload(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    Path(upload_id): Path<String>,
) -> Response {
    match state.service.abort_upload(&ctx, &upload_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Handler：任务
// ---------------------------------------------------------------------------

/// 建导入 → **202**。
async fn create_import(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<CreateJobBody>,
) -> Response {
    match state.service.create_import(&ctx, &body).await {
        Ok(row) => (StatusCode::ACCEPTED, Json(view_of_job(&row, None, None))).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 建导出 → **202**。
async fn create_export(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    ApiJson(body): ApiJson<CreateJobBody>,
) -> Response {
    match state.service.create_export(&ctx, &body).await {
        Ok(row) => (StatusCode::ACCEPTED, Json(view_of_job(&row, None, None))).into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 列任务。
async fn list_jobs(State(state): State<Arc<StorageState>>, Ctx { ctx, instance }: Ctx) -> Response {
    match state.service.list_jobs(&ctx).await {
        Ok(rows) => Json(
            rows.iter()
                .map(|row| view_of_job(row, None, None))
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => service_problem_response(&error, &ctx, instance.as_str()),
    }
}

/// 读单个任务（**含 `downloadUrl`**）。
async fn get_job(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    Path(job_id): Path<String>,
) -> Response {
    let row = match state.service.find_job(&ctx, &job_id).await {
        Ok(row) => row,
        Err(error) => return service_problem_response(&error, &ctx, instance.as_str()),
    };
    // 完成的导出任务 → 签一个下载 URL，并带上产物大小（AC-S-182 步骤 3 要求 `downloadUrl` + `size`）。
    let (download_url, size) = match (&row.object_key, row.status.as_str()) {
        (Some(key), "completed") => {
            let url = state
                .service
                .presign_object(&ctx, key, None)
                .await
                .ok()
                .map(|(u, _)| u);
            // ⚠️ 大小取**对象元数据**里的值（落盘时的真实字节数），
            // 不是任务表的行数 —— 两者含义不同（行数 ≠ 字节数）。
            let size = state
                .service
                .store()
                .find_object(&ctx, key)
                .await
                .ok()
                .flatten()
                .map(|object| object.size_bytes);
            (url, size)
        }
        _ => (None, None),
    };
    Json(view_of_job(&row, download_url, size)).into_response()
}

/// 列某任务的错误回执。
async fn list_job_errors(
    State(state): State<Arc<StorageState>>,
    Ctx { ctx, instance }: Ctx,
    Path(job_id): Path<String>,
) -> Response {
    match state.service.list_job_errors(&ctx, &job_id).await {
        Ok(rows) => Json(rows.iter().map(view_of_job_error).collect::<Vec<_>>()).into_response(),
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

/// 请求体解析失败 → **400**（用最接近的已登记码 + 可诊断的 detail）。
fn invalid_request_response(ctx: &RequestContext, instance: &str) -> Response {
    let error = crate::error::StorageError::PresignInvalid {
        reason: "the request body is not valid json for this endpoint",
    };
    // ⚠️ 语义上「请求体非法」不是预签名问题 —— 但本 crate 的 9 个码都有 AC 触发点，
    // 加第 10 个就是死码（D226 / D330）。这里复用 400 族里最接近的，
    // 并在 detail 里写清真实原因（前端看 code 取文案，看 detail 排障）。
    problem_response(&ProblemDetails::from_context(
        error.code(),
        platform_core::titles::title_of_code(error.code()),
        400,
        "the request body is not valid json for this endpoint",
        ctx,
        instance,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_parsing() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer abc".parse().expect("合法"));
        assert_eq!(bearer_token(&headers), Some("abc".to_string()));
        headers.insert(header::AUTHORIZATION, "bearer xyz".parse().expect("合法"));
        assert_eq!(bearer_token(&headers), Some("xyz".to_string()));
        headers.insert(header::AUTHORIZATION, "Bearer  ".parse().expect("合法"));
        assert_eq!(bearer_token(&headers), None, "空令牌必须拒绝");
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().expect("合法"));
        assert_eq!(bearer_token(&headers), None, "非 Bearer 必须拒绝");
    }

    #[test]
    fn bearer_token_absent() {
        assert_eq!(bearer_token(&HeaderMap::new()), None);
    }

    /// AC-S-182 步骤 4：导出结果下载必须是 `text/csv`（不是 `application/octet-stream`）。
    #[test]
    fn the_download_content_type_is_csv_for_csv_keys() {
        assert_eq!(download_content_type("exports/probe.csv"), "text/csv");
        // 大小写不敏感（浏览器/客户端给什么大小写都可能）。
        assert_eq!(download_content_type("EXPORT.CSV"), "text/csv");
        assert_eq!(download_content_type("a.b.csv"), "text/csv");
    }

    /// ⚠️ **安全判据**：除 CSV 外**一律** `application/octet-stream`，
    /// 绝不让浏览器渲染 —— 否则上传一个 `.html` 就能得到同源的钓鱼页。
    #[test]
    fn the_download_content_type_never_renders_other_types() {
        for key in [
            "evil.html",
            "evil.htm",
            "evil.svg",
            "evil.xml",
            "evil.js",
            "no-extension",
            "",
        ] {
            assert_eq!(
                download_content_type(key),
                "application/octet-stream",
                "{key:?} 绝不能被浏览器当内容渲染"
            );
        }
    }

    /// 路由注册了全部路径（防漏挂端点）。
    #[tokio::test]
    async fn the_router_registers_all_paths() {
        let state = Arc::new(StorageState {
            service: Arc::new(todo_service()),
            tokens: Arc::new(RejectAll),
            checker: Arc::new(AllowAll),
            refresher: Arc::new(NoopRefresher),
        });
        let debug = format!("{:?}", router::<()>(state));
        for path in [
            "/objects",
            "/objects/{key}",
            "/objects/{key}/presign",
            "/uploads",
            "/uploads/{id}/parts/{n}",
            "/uploads/{id}/complete",
            "/uploads/{id}",
            "/imports",
            "/exports",
            "/jobs",
            "/jobs/{id}",
            "/jobs/{id}/errors",
            "/download/{token}",
        ] {
            assert!(debug.contains(path), "路由里缺 {path}；实际：{debug}");
        }
    }

    /// 测试用的服务（**不连库**：只用于路由装配测试）。
    fn todo_service() -> StorageService {
        let db = platform_db::TenantDb::from_pool(
            sqlx::PgPool::connect_lazy("postgres://x/y").expect("lazy pool"),
            platform_db::DbOptions::default(),
        );
        StorageService::new(
            crate::store::StorageStore::new(db),
            crate::config::StorageConfig::new(Arc::new(NoopConfigSource)),
            Arc::new(NoopStore),
            Arc::new(crate::scanner::NoopScanner),
            b"test-secret".to_vec(),
            "k1".to_string(),
        )
    }

    struct NoopStore;
    impl crate::object_store::ObjectStore for NoopStore {
        fn put<'a>(&'a self, _k: &'a str, _b: Bytes) -> crate::object_store::StoreFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }
        fn get<'a>(&'a self, _k: &'a str) -> crate::object_store::StoreFuture<'a, Bytes> {
            Box::pin(async { Ok(Bytes::new()) })
        }
        fn delete<'a>(&'a self, _k: &'a str) -> crate::object_store::StoreFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }
        fn create_multipart<'a>(
            &'a self,
            _u: &'a str,
            _k: &'a str,
            _t: i32,
        ) -> crate::object_store::StoreFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }
        fn upload_part<'a>(
            &'a self,
            _u: &'a str,
            n: u32,
            _b: Bytes,
        ) -> crate::object_store::StoreFuture<'a, crate::object_store::PartETag> {
            Box::pin(async move {
                Ok(crate::object_store::PartETag {
                    part_number: n,
                    etag: String::new(),
                })
            })
        }
        fn list_parts<'a>(&'a self, _u: &'a str) -> crate::object_store::StoreFuture<'a, Vec<u32>> {
            Box::pin(async { Ok(Vec::new()) })
        }
        fn complete_multipart<'a>(
            &'a self,
            _u: &'a str,
            _k: &'a str,
            _t: i32,
        ) -> crate::object_store::StoreFuture<'a, String> {
            Box::pin(async { Ok(String::new()) })
        }
        fn abort_multipart<'a>(&'a self, _u: &'a str) -> crate::object_store::StoreFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    struct RejectAll;
    impl TokenVerifier for RejectAll {
        fn verify(&self, _token: &str) -> Option<VerifiedUser> {
            None
        }
    }

    struct AllowAll;
    impl PermissionChecker for AllowAll {
        fn has(&self, _ctx: &RequestContext, _permission: &str) -> bool {
            true
        }
        fn scope(
            &self,
            _ctx: &RequestContext,
            _scope: platform_core::extension::permission::DataScope,
        ) -> platform_core::extension::permission::ScopeFilter {
            platform_core::extension::permission::ScopeFilter::All
        }
        fn effective_scope(
            &self,
            _ctx: &RequestContext,
            _permission: &str,
        ) -> platform_core::extension::permission::ScopeFilter {
            platform_core::extension::permission::ScopeFilter::All
        }
        fn condition(
            &self,
            _ctx: &RequestContext,
            _permission: &str,
        ) -> Option<platform_core::extension::expr::CompiledCondition> {
            None
        }
    }

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

    struct NoopConfigSource;
    impl crate::config::StorageConfigSource for NoopConfigSource {
        fn read_platform<'a>(
            &'a self,
            _ctx: &'a RequestContext,
            _key: &'a str,
        ) -> crate::config::StorageBoxFuture<'a, Option<i64>> {
            Box::pin(async { None })
        }
    }
}
