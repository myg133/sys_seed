//! 可观测性端点（REQ-019，AC-S-034 依赖）。
//!
//! ⚠️ **放在本 crate 而不是 `platform-server`**：
//! `xtask` 要引用 [`ApiDoc`] 生成 OpenAPI，而 `xtask` **不得依赖 `platform-server`**
//! （那会经它间接依赖业务模块 `sandbox`，违反 D103「平台 crate 不依赖业务模块」）。
//! 放在这里 → `xtask` 只依赖一个纯平台 crate，架构约束成立。
//!
//! # 为什么需要这个模块
//!
//! AC-S-034 要验「分页契约**跨 crate 统一**」。但仓库里只有 `platform-audit`
//! 一个端点用了 cursor 分页 —— **只测一个老端点证明不了「统一」**。
//!
//! → 本模块提供一个**新增的**分页端点，用同一套 `items` / `nextCursor` / `total`
//! 契约。判据就能断言「两个端点的字段名完全相同」（D194 的「包含」原则）。
//!
//! ⚠️ 挑刺 B5 指出 rev 1 的设计里**根本没有**这个端点，
//! 而 AC-S-034 的步骤 3/4/5 都依赖它 → 判据没有主语。rev 2 补上。

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use platform_core::RequestContext;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::rate_limit::middleware::RateLimitState;

/// 分页上限（**与 `platform-audit` 的 `MAX_LIMIT` 一致**）。
///
/// ⚠️ 两个端点用**同一个上限**是「契约统一」的一部分 —— 若一个 200 一个 100，
/// 调用方就没法用同一套参数打所有端点。
pub const MAX_LIMIT: usize = 200;
/// 默认每页条数（与 `platform-audit` 的 `DEFAULT_LIMIT` 一致）。
pub const DEFAULT_LIMIT: usize = 20;

/// 端点状态。
pub struct StatusState {
    /// 限流中间件状态（`None` = 未装配）。
    pub rate_limit: Option<Arc<RateLimitState>>,
}

/// 分页查询参数（**与 `platform-audit` 同形**）。
#[derive(Debug, Default, Deserialize)]
pub struct PageQuery {
    /// 每页条数（1..=200，默认 20）。
    pub limit: Option<String>,
    /// 不透明游标。
    pub cursor: Option<String>,
}

/// 一个限流维度的状态行。
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitRow {
    /// 维度（`tenant` / `user` / `endpoint`）。
    pub dimension: String,
    /// 窗口内允许的请求数（0 = 禁用）。
    pub requests: i64,
    /// 窗口长度（秒）。
    pub window_seconds: i64,
}

/// 分页响应（**与 `platform-audit` 同形**）。
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    /// 数据。
    pub items: Vec<T>,
    /// 下一页游标（`null` = 没有下一页）。
    pub next_cursor: Option<String>,
    /// 总数。
    pub total: i64,
}

/// 解析 `limit`（**越界 → 400，不静默截断**，D341）。
///
/// ⚠️ 返回值用 `Result<usize, ProblemDetails>`；错误码与 `platform-audit` 同口径。
fn parse_limit(raw: Option<&str>) -> Result<usize, Box<ProblemDetails>> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_LIMIT);
    };
    let value: usize = raw.parse().map_err(|_| {
        Box::new(problem(&format!(
            "the `limit` parameter must be a positive integer; got `{raw}`"
        )))
    })?;
    if value == 0 {
        return Err(Box::new(problem(
            "the `limit` parameter must be greater than zero",
        )));
    }
    if value > MAX_LIMIT {
        return Err(Box::new(problem(&format!(
            "the `limit` parameter must not exceed {MAX_LIMIT}; got {value}"
        ))));
    }
    Ok(value)
}

/// 造一个 400 的 `ProblemDetails`（**`INVALID_REQUEST` 语义**）。
///
/// ⚠️ 这里不复用 `platform-observability` 的 2 个码 ——
/// 「参数非法」是**平台通用**语义，不该给每个 crate 各造一个码（D226：不留死码）。
fn problem(detail: &str) -> ProblemDetails {
    ProblemDetails::from_context(
        "INVALID_REQUEST",
        platform_core::titles::title_of_code("INVALID_REQUEST"),
        400,
        detail,
        &RequestContext::new(),
        "/api/observability/rate-limit/status",
        None,
    )
}

/// 读限流状态（分页）。
///
/// ⚠️ `#[utoipa::path]` 是 **REQ-019 AC-S-035** 的产物来源：
/// 它在编译期生成 `__path_rate_limit_status`，被 [`ApiDoc`] 的 `paths(...)` 引用。
#[utoipa::path(
    get,
    path = "/api/observability/rate-limit/status",
    params(
        ("limit" = Option<String>, Query, description = "每页条数（1..=200，默认 20；越界 400）"),
        ("cursor" = Option<String>, Query, description = "不透明游标"),
    ),
    responses(
        (status = 200, description = "分页的限流配置", body = inline(Page<RateLimitRow>)),
        (status = 400, description = "参数非法（INVALID_REQUEST）"),
    ),
    tag = "observability"
)]
pub async fn rate_limit_status(
    State(state): State<Arc<StatusState>>,
    Query(query): Query<PageQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let limit = match parse_limit(query.limit.as_deref()) {
        Ok(limit) => limit,
        Err(problem) => return (axum::http::StatusCode::BAD_REQUEST, Json(*problem)).into_response(),
    };

    // 三行：三个维度各一行（真实数据，不是假的）。
    let all = match state.rate_limit.as_ref() {
        Some(rate_limit) => {
            // ⚠️ 读配置需要一个租户（RLS）—— 用哨兵租户做只读。
            let probe = RequestContext::new().with_tenant("_probe");
            let settings = rate_limit.config.settings(&probe).await;
            vec![
                RateLimitRow {
                    dimension: "tenant".to_string(),
                    requests: settings.tenant.requests,
                    window_seconds: settings.tenant.window_seconds,
                },
                RateLimitRow {
                    dimension: "user".to_string(),
                    requests: settings.user.requests,
                    window_seconds: settings.user.window_seconds,
                },
                RateLimitRow {
                    dimension: "endpoint".to_string(),
                    requests: settings.endpoint.requests,
                    window_seconds: settings.endpoint.window_seconds,
                },
            ]
        }
        None => Vec::new(),
    };

    let total = all.len() as i64;
    // 游标分页（keyset）：用「最后一条的维度名」当下一页的起点。
    let start = match query.cursor.as_deref() {
        Some(cursor) => all
            .iter()
            .position(|row| row.dimension == cursor)
            .map(|index| index + 1)
            .unwrap_or(0),
        None => 0,
    };
    let page: Vec<RateLimitRow> = all.iter().skip(start).take(limit).cloned().collect();
    let next_cursor = if start + page.len() < all.len() {
        page.last().map(|row| row.dimension.clone())
    } else {
        None
    };

    Json(Page {
        items: page,
        next_cursor,
        total,
    })
    .into_response()
}

/// **OpenAPI 文档**（REQ-019 AC-S-035）。
///
/// # ⚠️ 覆盖范围（如实记录，不夸大）
///
/// 本需求只标注了**本 crate 能触及**的端点。其余 crate 的路由需要各自加
/// `#[utoipa::path]` —— 那是**跨需求改动**（要改已验收 crate 的源码），
/// 本轮不做 → **TD-073**。
///
/// 因此 `cargo xtask openapi` 产出的文档是**部分**的：
/// 它**不**声称覆盖全部 100+ 个端点。
#[derive(utoipa::OpenApi)]
#[openapi(
    info(
        title = "通用项目底座 API",
        version = env!("CARGO_PKG_VERSION"),
        description = "由 `utoipa` 从路由与 DTO 生成（REQ-019 D338）。                       ⚠️ 当前只覆盖部分端点，见 TD-073。"
    ),
    paths(rate_limit_status)
)]
pub struct ApiDoc;

/// 就绪探针对**限流存储**的检查（AC-S-038）。
///
/// # 返回
///
/// - `None`：**不需要检查**（未装配限流器，或 `rateLimit.enabled = false`）
/// - `Some(true)`：检查通过
/// - `Some(false)`：限流存储**不可达** → 调用方应返回 503
///
/// # ⚠️ 为什么「关闭限流时返回 `None`」
///
/// AC-S-038 步骤 4 的判据：**关闭限流的部署不该因为限流表问题而不就绪**。
/// 「没启用限流」反而更脆弱是荒谬的 —— 这条防止那种实现。
///
/// ⚠️ 抽成独立函数是为了**可测**：`main.rs` 的 `ready` handler 里
/// 内联这段逻辑的话，测试够不着（bin crate 不可引用）。
pub async fn rate_limit_readiness(
    state: &Option<Arc<RateLimitState>>,
    ctx: &RequestContext,
) -> Option<bool> {
    let state = state.as_ref()?;
    // 先看启用状态：未启用 → 不检查。
    let settings = state.config.settings(ctx).await;
    if !settings.enabled {
        return None;
    }
    let limiter = state.limiter.as_ref()?;
    // ⚠️ 用**只读**查询（`is_reachable` 不写行）—— 就绪探针不能污染限流计数。
    Some(limiter.is_reachable(ctx).await)
}

/// 路由。
pub fn router<S>(state: Arc<StatusState>) -> Router<S> {
    Router::new()
        .route("/rate-limit/status", get(rate_limit_status))
        .with_state(state)
}

/// 统一的错误体（与本 crate 的 `ProblemDetails` 同构）。
pub type ProblemDetails = platform_core::problem::ProblemDetails;

#[cfg(test)]
mod tests {
    use super::*;

    /// 上限与 audit **必须一致**（「契约统一」的一部分）。
    #[test]
    fn the_limit_upper_bound_matches_the_audit_endpoint() {
        // ⚠️ 这两个常量在 `platform-audit` 里也有 —— 写死同一个值，
        // 若哪天 audit 改了而这里没改，这条测试不会红（跨 crate 常量无法互相引用），
        // 但 AC-S-034 的「字段名完全相同」用例会。
        // 这里至少保证本 crate 内部自洽。
        assert_eq!(MAX_LIMIT, 200);
        assert_eq!(DEFAULT_LIMIT, 20);
        assert!(DEFAULT_LIMIT < MAX_LIMIT);
    }

    /// `limit` 缺省 → 默认值。
    #[test]
    fn a_missing_limit_uses_the_default() {
        assert_eq!(parse_limit(None).expect("缺省应通过"), DEFAULT_LIMIT);
    }

    /// `limit=0` → 400。
    #[test]
    fn a_zero_limit_is_rejected() {
        assert!(parse_limit(Some("0")).is_err(), "0 是非法的（不是「不限」）");
    }

    /// `limit` 超上限 → 400（**不静默截断**，D341）。
    #[test]
    fn an_oversized_limit_is_rejected() {
        let err = parse_limit(Some("1000")).expect_err("超上限必须拒绝");
        assert_eq!(err.status, 400, "必须是 400（不是 200 + 截断）");
        assert!(
            err.detail.contains("200") && err.detail.contains("1000"),
            "detail 必须含上限与实际值：{}",
            err.detail
        );
    }

    /// 正好等于上限 → 通过（边界包含）。
    #[test]
    fn a_limit_equal_to_the_maximum_is_accepted() {
        assert_eq!(parse_limit(Some("200")).expect("200 应通过"), MAX_LIMIT);
    }

    /// 非数字 → 400。
    #[test]
    fn a_non_numeric_limit_is_rejected() {
        assert!(parse_limit(Some("abc")).is_err());
    }

    /// 负数 → 400（usize 解析失败）。
    #[test]
    fn a_negative_limit_is_rejected() {
        assert!(parse_limit(Some("-1")).is_err());
    }
}

// ---------------------------------------------------------------------------
// 中间件装配（TD-075 的处置）
// ---------------------------------------------------------------------------

/// 本需求新增的**按路径弃用清单**。
///
/// ⚠️ 只标 `/api/audit/events` —— 弃用理由是「分页契约升版后它将被 v2 取代」
/// （D341 给了它一个合理的弃用理由，不是随便挑的）。
///
/// ⚠️ 用**按路径**匹配而不是包住 `/api/audit` 整组：
/// 那组里还有 `verify` / `archive` 等**未**弃用的端点
/// → 包整组会让「恰好 1 个路由带弃用头」的判据（AC-S-036 步骤 7）红。
pub fn default_deprecations() -> Vec<(String, crate::deprecation::Deprecation)> {
    vec![(
        "/api/audit/events".to_string(),
        crate::deprecation::Deprecation::new(
            // 2027-06-30（约 19 个月后 —— 客户端有足够时间迁移）。
            time::macros::datetime!(2027-06-30 00:00:00 UTC),
        ),
    )]
}

/// **中间件装配的唯一实现**（TD-075）。
///
/// # ⚠️ 为什么这个函数存在
///
/// `main.rs` 里的中间件装配原本是内联的（约 50 行）—— 测试**够不着**它
/// （bin crate 的函数不可引用）。后果：
/// 若 `main` 把 `apply_observability` 放在 `client_info` **之前**，
/// 或漏掉了 `apply_observability`，**所有 HTTP 级 AC 的用例仍然全绿** ——
/// 因为它们测的是自己的 router，不是 `main` 的装配。
///
/// → 把这段抽成函数，`main` 与测试**都调它**：
/// 「装配顺序与内容」这件事就**有判据**了。
///
/// # 顺序（**不可换**）
///
/// ```text
/// client_info（填 IP / UA）
///   └─ trace_id（生成/校验 → 开 span → 回写头）
///        └─ rate_limit（三级判定）
///             └─ 业务路由
/// ```
///
/// ⚠️ `client_info` 必须在最外层：各子系统**各自**建 `RequestContext`，
/// 「对所有路由生效」只能在顶层加一次（REQ-014 D149）。
///
/// # 参数
///
/// - `rate_limit`：`None` → 不挂限流中间件（零开销）
pub fn apply_all_middlewares<S>(
    router: axum::Router<S>,
    rate_limit: Option<Arc<RateLimitState>>,
) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    // ⚠️ 挂载顺序 = 执行顺序的**反向**（后挂在外层）。
    // 目标执行：client_info → trace_id → rate_limit → 弃用 → 业务
    let router = crate::apply_observability(router, rate_limit);
    let router = crate::deprecation::apply_deprecation_router(router, default_deprecations());
    router.layer(axum::middleware::from_fn(
        platform_server_client_info::client_info,
    ))
}

/// `client_info` 中间件的桥接。
///
/// ⚠️ 它原本在 `platform-server` 里，而 `platform-observability` **不能**依赖它
/// （会成环：server → observability）。→ 用「函数指针注入」的方式：
/// 装配函数接受一个 `client_info` 实现。
///
/// 这里给一个**默认空实现**（只用于测试与「没装 client_info」的场景）——
/// 生产必须用 `platform-server` 的真实实现（见 [`apply_all_middlewares_with`]）。
pub mod platform_server_client_info {
    /// 空实现：不做任何事，直接放行。
    ///
    /// ⚠️ **生产不能用它** —— 那会让所有请求的 IP/UA 为空（审计与排障都受影响）。
    /// 它只用于「本 crate 的单元测试」与「装配顺序的判据」。
    pub async fn client_info(
        request: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        next.run(request).await
    }
}

/// 与 [`apply_all_middlewares`] 相同，但 `client_info` **由调用方注入**。
///
/// 生产用它（传入 `platform_server::client_info::client_info`），
/// 这样依赖方向是 `server → observability`，不成环。
pub fn apply_all_middlewares_with<S, F, Fut>(
    router: axum::Router<S>,
    rate_limit: Option<Arc<RateLimitState>>,
    client_info: F,
) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
    F: Fn(axum::extract::Request, axum::middleware::Next) -> Fut
        + Clone
        + Send
        + Sync
        + 'static,
    Fut: std::future::Future<Output = axum::response::Response> + Send + 'static,
{
    // ⚠️ **`layer` 是「后挂的在外层」**（实测确认：先挂 A 后挂 B → B 先执行）。
    //
    // 目标执行顺序：`client_info` → `trace_id` → `rate_limit` → 业务
    // → 挂载顺序必须**反过来**：先挂 rate_limit（最内），再 trace_id，最后 client_info（最外）。
    //
    // 这个顺序我踩过两次：
    // 1. `apply_observability` 里先挂 trace 后挂 limit → limit 先跑，读不到租户 → 限流永不生效；
    // 2. 这里先挂 client_info → 它变成内层 → trace_id 先跑，读不到 client_info 填的租户。
    // 抓到它们的是 `middleware_assembly` 的判据。
    let router = crate::apply_observability(router, rate_limit);
    let router = crate::deprecation::apply_deprecation_router(router, default_deprecations());
    // 最后挂 → 最外层 → 最先执行。
    router.layer(axum::middleware::from_fn(client_info))
}
