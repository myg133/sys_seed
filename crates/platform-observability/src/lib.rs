//! 可观测性与 API 治理（REQ-019）。
//!
//! # 为什么单独一个 crate（D343）
//!
//! `platform-core` 是**所有 crate 的依赖**（轻量、无 IO）。
//! 本 crate 要 PG（限流）与 OTel exporter —— 塞进 core 会让**每个 crate 都编译 OTel**，
//! 编译时间与二进制体积都会涨。
//!
//! 本 crate **只被 `platform-server` 依赖**；其它 crate 通过 `platform-core` 的
//! **trait** 与本 crate 解耦（D150）。
//!
//! # 模块
//!
//! | 模块 | 职责 | AC |
//! |---|---|---|
//! | [`trace`] | `trace_id` 生成/校验/归一 | AC-S-029 / AC-S-030 |
//! | [`rate_limit`] | 三级限流（PG 滑动窗口） | AC-S-031 / AC-S-032 / AC-S-033 |
//! | [`error`] | 本 crate 的 2 个错误码 + 维度常量 | — |
//!
//! # 依赖破例（**两次，都已实测**）
//!
//! - **D338**：`utoipa` —— 净增 **2** 包（OpenAPI 聚合）
//! - **D339**：`opentelemetry*` —— 净增 **18** 包（OTLP HTTP/protobuf）

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod deprecation;
pub mod error;
pub mod http;
pub mod otel;
pub mod rate_limit;
pub mod trace;

/// 本 crate 的版本（供 `/health` 与 OpenAPI 的 `info.version` 用）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 本 crate 的全部**平台**错误码（**完备性测试的输入**，D113）。
///
/// ⚠️ 只有 2 个自有码；其余路径复用平台既有的 `INVALID_REQUEST` /
/// `UNAUTHENTICATED`（见 `error` 模块文档）。
pub use error::ALL_CODES;

#[cfg(test)]
mod tests {
    use super::*;

    /// 版本非空。
    #[test]
    fn the_version_is_exposed() {
        assert!(!VERSION.is_empty());
    }

    /// 错误码**恰好 2 条**且已 re-export。
    #[test]
    fn the_error_code_list_is_exported() {
        assert_eq!(ALL_CODES.len(), 2, "本 crate 只有 2 个自有码");
        assert!(ALL_CODES.contains(&"RATE_LIMITED"));
        assert!(ALL_CODES.contains(&"RATE_LIMIT_UNAVAILABLE"));
    }

    /// 两个错误码都在平台的标题表里（否则前端渲染不出文案）。
    ///
    /// ⚠️ 新增错误码必须改 8 处（D125）；这条是「TITLES 那一处」的守卫。
    #[test]
    fn every_code_has_a_title() {
        for code in ALL_CODES {
            let title = platform_core::titles::title_of_code(code);
            assert!(
                !title.is_empty() && title != *code,
                "错误码 {code} 在 TITLES 里没有标题（D125 的 8 处改动漏了？）"
            );
        }
    }
}

use platform_core::RequestContext;
use platform_db::TenantDb;
use std::sync::Arc;

/// 限流器 trait（**业务层只认它**，D150）。
///
/// ⚠️ 用 `Box<dyn Future>` 而不是 `impl Future` —— 平台 trait 要能 `dyn`
/// （`NotificationSender` 的教训，见 REQ-015 的 D293）。
pub trait RateLimiter: Send + Sync {
    /// 判定一个维度是否放行；放行则记账。
    fn check<'a>(
        &'a self,
        ctx: &'a RequestContext,
        dimension: &'a str,
        key: &'a str,
        limit: rate_limit::WindowLimit,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), error::ObservabilityError>> + Send + 'a,
        >,
    >;

    /// 就绪检查（AC-S-038）。
    fn is_reachable<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>>;
}

impl RateLimiter for rate_limit::PgRateLimiter {
    fn check<'a>(
        &'a self,
        ctx: &'a RequestContext,
        dimension: &'a str,
        key: &'a str,
        limit: rate_limit::WindowLimit,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), error::ObservabilityError>> + Send + 'a,
        >,
    > {
        Box::pin(async move { rate_limit::PgRateLimiter::check(self, ctx, dimension, key, limit).await })
    }

    fn is_reachable<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        Box::pin(async move { rate_limit::PgRateLimiter::is_reachable(self, ctx).await })
    }
}

/// 把本需求的中间件挂到一个 router 上（**唯一实现**，main 与测试共用）。
///
/// # ⚠️ 为什么要有这个函数（挑刺 M5 的处置）
///
/// 6 条 HTTP 级 AC（029/030/031/032/036/037）都依赖「中间件真的挂上了、顺序对」。
/// 若 `main.rs` 与测试**各写一份装配**，中间件漏装 / 顺序错都能绿 ——
/// 那正是 `platform-core/src/log.rs:1-20` 写过的教训。
///
/// → 装配逻辑**只此一份**：`main.rs` 调它，HTTP 测试也调它。
///
/// # 挂载顺序（**不可换**）
///
/// ```text
/// client_info（main.rs，填 IP/UA）
///   └─ trace_id（本函数：生成/校验 → 开 span → 回写头）
///        └─ rate_limit（本函数：三级判定）
///             └─ 业务路由
/// ```
///
/// ⚠️ `trace_id` 必须在 `rate_limit` **之前**：
/// 否则限流自己打的日志（fail open 的 WARN）没有 trace_id。
///
/// # 参数
///
/// - `rate_limit`：`None` 表示不启用限流（连中间件都不挂，零开销）
pub fn apply_observability<S>(
    router: axum::Router<S>,
    rate_limit: Option<std::sync::Arc<rate_limit::middleware::RateLimitState>>,
) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    // ① trace_id（**永远挂**：它没有成本，且是所有排障的基础）。
    let router = router.layer(axum::middleware::from_fn(trace::middleware::trace_id));

    // ② 限流（可选）。
    match rate_limit {
        Some(state) => router.layer(axum::middleware::from_fn_with_state(
            state,
            rate_limit::middleware::rate_limit,
        )),
        None => router,
    }
}

/// 用租户作用域入口构造一个 PG 滑动窗口限流器。
///
/// ⚠️ 这是**唯一**的生产构造方式 —— 测试要验「跨实例准确」（AC-S-033）时，
/// 必须用**两个独立的 `TenantDb`**（各自 `connect`，不是 clone 同一个池）。
pub fn pg_rate_limiter(db: TenantDb) -> Arc<dyn RateLimiter> {
    Arc::new(rate_limit::PgRateLimiter::new(db))
}
