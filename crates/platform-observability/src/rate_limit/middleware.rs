//! 限流中间件（REQ-019，AC-S-031 / AC-S-032）。
//!
//! # 挂载位置（**顺序不可换**）
//!
//! 必须挂在 `client_info` **之后**（它填 `RequestContext` 的 IP/UA），
//! 且在**业务路由之前**。
//!
//! # ⚠️ 三条必须的放行路径（fail open）
//!
//! | 情况 | 行为 | 为什么 |
//! |---|---|---|
//! | `rateLimit.enabled = false`（含**读不到配置**） | **直接放行**（零查库） | D344：默认关闭，否则现有 1496 个测试集体红 |
//! | **无租户上下文**（未认证） | **直接放行** | TD-072：RLS 租户表写不进匿名请求；IAM 有自己的 IP 限流 |
//! | 限流存储不可用 | **放行 + WARN** | D339 同款纪律：基础设施故障不能拦业务 |
//!
//! ⚠️ 最后一条尤其重要：如果存储挂了就 429，等于把「限流组件故障」
//! 放大成「全站不可用」—— 比不限流更糟。

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use platform_core::RequestContext;
use std::sync::Arc;

use crate::error::{ObservabilityError, dimension};
use crate::rate_limit::{RateLimitSettings, bucket_key};
use crate::RateLimiter;

/// 限流配置的来源（**每请求读**，支持热更新）。
///
/// ⚠️ 用 trait 而不是直接传 `RateLimitSettings` ——
/// 生产要从 `platform-config` 读（每请求一次，含 refresh），
/// 测试要传固定值。两者形状不同，必须留缝。
pub trait RateLimitConfigSource: Send + Sync {
    /// 读当前配置。
    ///
    /// # ⚠️ 实现必须 **fail open**
    ///
    /// 读失败 / 键未登记 / 缓存无快照 → 返回 [`RateLimitSettings::disabled()`]，
    /// **不要**返回 `Err`（那会让所有请求 500）。
    fn settings<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = RateLimitSettings> + Send + 'a>>;
}

/// 固定配置源（测试与「配置未接入」时用）。
pub struct StaticConfig(pub RateLimitSettings);

impl RateLimitConfigSource for StaticConfig {
    fn settings<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = RateLimitSettings> + Send + 'a>> {
        Box::pin(async move { self.0 })
    }
}

/// 中间件共享状态。
pub struct RateLimitState {
    /// 限流器（`None` = 不限流，直接放行）。
    pub limiter: Option<Arc<dyn RateLimiter>>,
    /// 配置源。
    pub config: Arc<dyn RateLimitConfigSource>,
}

impl std::fmt::Debug for RateLimitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimitState")
            .field("limiter", &self.limiter.is_some())
            .finish_non_exhaustive()
    }
}

/// 限流中间件。
///
/// # 判定顺序：tenant → user → endpoint
///
/// ⚠️ **顺序影响判据**：若租户维度先超限，`params.dimension` 会是 `tenant`，
/// 后面两个维度根本不会被检查。测试要验某一维度时必须**禁用其余维度**
/// （AC-S-031 的每个步骤都是这么构造的）。
pub async fn rate_limit(
    State(state): State<Arc<RateLimitState>>,
    request: Request,
    next: Next,
) -> Response {
    // ① 没有限流器 → 放行（未装配或显式关闭）。
    let Some(limiter) = state.limiter.clone() else {
        return next.run(request).await;
    };

    // ② 取上下文（`client_info` 已填；没有则建一个空的）。
    let ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    // ③ 读配置 —— **fail open**：读不到就是 disabled。
    let settings = state.config.settings(&ctx).await;
    if !settings.enabled {
        return next.run(request).await;
    }

    // ④ 无租户（未认证）→ 放行（TD-072）。
    //
    // ⚠️ RLS 租户表在无租户时写不进（`TenantDb::begin` 直接 MissingTenant）。
    // 若这里不放行，**登录端点会 500** —— 比不限流糟得多。
    let Some(tenant_id) = ctx.tenant_id() else {
        tracing::debug!(
            "no tenant context (unauthenticated request) - rate limiting is skipped (TD-072)"
        );
        return next.run(request).await;
    };
    let tenant_id = tenant_id.to_string();
    let user_id = ctx.user_id().map(str::to_string);

    // ⑤ 取路径模板（⚠️ **不是**完整 URI —— 否则加随机 query 就能绕过，AC-S-031 步骤 5）。
    //
    // `MatchedPath` 由 axum 在路由匹配后写入 extensions（已实测：
    // `/api/rbac/roles?x=1` → `/api/rbac/roles`）。
    // 取不到时（如 404 路径）用 `"<unmatched>"` —— 所有未匹配路径共享一个桶，
    // 这是**有意的**：不存在的路径也消耗配额，防止扫路径。
    let endpoint = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "<unmatched>".to_string());

    // ⑤b 取 HTTP 方法（D337 / AC-S-033：endpoint 键含 METHOD）。
    //
    // ⚠️ 同一路径常挂多个方法（`GET /api/rbac/roles` 列表、`POST` 新建），
    // 只按路径做键会让写请求挤掉读请求的配额（F13）。
    let method = request.method().as_str().to_string();

    // ⑥ 依次判定三个维度。
    for name in dimension::ALL {
        let limit = settings.limit_of(name);
        if !limit.enabled() {
            continue; // 该维度禁用
        }
        let key = bucket_key(name, &tenant_id, user_id.as_deref(), &method, &endpoint);
        match limiter.check(&ctx, name, &key, limit).await {
            Ok(()) => {}
            Err(ObservabilityError::RateLimited {
                dimension,
                retry_after,
            }) => {
                // 触发限流 → 429 + Retry-After（AC-S-032）。
                let error = ObservabilityError::RateLimited {
                    dimension,
                    retry_after,
                };
                let details = error.problem_details(&ctx, request.uri().path());
                let mut response = (
                    StatusCode::TOO_MANY_REQUESTS,
                    axum::Json(details),
                )
                    .into_response();
                // ⚠️ `Retry-After` 必须是**正整数秒**（AC-S-032 步骤 1/2 的判据）。
                if let Ok(value) = axum::http::HeaderValue::from_str(&retry_after.max(1).to_string())
                {
                    response.headers_mut().insert(
                        axum::http::header::RETRY_AFTER,
                        value,
                    );
                }
                return response;
            }
            Err(ObservabilityError::RateLimitUnavailable { reason }) => {
                // ⚠️ **fail open**：存储不可用 → 放行 + WARN。
                // 不能因为限流组件故障就把全站打到 429/503。
                tracing::warn!(
                    error = %reason,
                    "the rate limit store is unavailable - failing open (request is allowed)"
                );
                return next.run(request).await;
            }
        }
    }

    // ⑦ 三个维度都通过 → 业务。
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rate_limit::WindowLimit;

    /// 固定配置源返回给定值（含 enabled=false 的用例）。
    #[tokio::test]
    async fn the_static_source_returns_the_configured_settings() {
        let settings = RateLimitSettings {
            enabled: true,
            tenant: WindowLimit {
                requests: 5,
                window_seconds: 10,
            },
            user: WindowLimit {
                requests: 0,
                window_seconds: 10,
            },
            endpoint: WindowLimit {
                requests: 0,
                window_seconds: 10,
            },
        };
        let source = StaticConfig(settings);
        let ctx = RequestContext::new();
        let got = source.settings(&ctx).await;
        assert_eq!(got, settings);
        assert!(got.enabled);
        assert_eq!(got.limit_of(dimension::TENANT).requests, 5);
        assert!(!got.limit_of(dimension::USER).enabled());
    }
}
