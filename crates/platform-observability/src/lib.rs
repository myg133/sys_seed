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

pub mod error;
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

/// 用租户作用域入口构造一个 PG 滑动窗口限流器。
///
/// ⚠️ 这是**唯一**的生产构造方式 —— 测试要验「跨实例准确」（AC-S-033）时，
/// 必须用**两个独立的 `TenantDb`**（各自 `connect`，不是 clone 同一个池）。
pub fn pg_rate_limiter(db: TenantDb) -> Arc<dyn RateLimiter> {
    Arc::new(rate_limit::PgRateLimiter::new(db))
}
