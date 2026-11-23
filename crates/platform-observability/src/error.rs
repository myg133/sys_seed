//! 可观测性与 API 治理的错误码（REQ-019）。
//!
//! # 为什么本 crate 只有 2 个码
//!
//! 大部分失败路径**复用**平台既有的码：
//! - 非法 `limit` / `cursor` → `INVALID_REQUEST`（`platform-core::problem`，已存在）
//! - 未认证 → `UNAUTHENTICATED`（已存在）
//!
//! 只有两件事是**本 crate 独有**的语义，才需要自己的码（D226：不给死码留位置）。

use axum::http::StatusCode;
use platform_core::RequestContext;
use platform_core::problem::ProblemDetails;

/// 稳定的机器可读错误码（平台侧，`OBSERVABILITY_*`）。
pub mod code {
    /// 触发了限流（AC-S-031 / AC-S-032）。
    pub const RATE_LIMITED: &str = "RATE_LIMITED";
    /// 限流后端的存储不可用（就绪探针与中间件的降级路径）。
    pub const RATE_LIMIT_UNAVAILABLE: &str = "RATE_LIMIT_UNAVAILABLE";
}

/// 本 crate 的全部错误码（**完备性测试的输入**，D113）。
///
/// ⚠️ 只有 2 个：其余路径复用平台已有的 `INVALID_REQUEST` 等。
pub const ALL_CODES: &[&str] = &[code::RATE_LIMITED, code::RATE_LIMIT_UNAVAILABLE];

/// 限流的维度（`params.dimension` 的取值域）。
pub mod dimension {
    /// 租户维度：该租户的**所有**请求共享一个桶。
    pub const TENANT: &str = "tenant";
    /// 用户维度：该租户内**某个用户**的所有请求共享一个桶。
    pub const USER: &str = "user";
    /// 接口维度：该租户内**某个端点**的所有请求共享一个桶
    /// （⚠️ 键里**没有** user 段 —— 同租户内所有用户共享）。
    pub const ENDPOINT: &str = "endpoint";

    /// 全部取值。
    pub const ALL: &[&str] = &[TENANT, USER, ENDPOINT];
}

/// 可观测性域的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservabilityError {
    /// 触发限流。
    RateLimited {
        /// 哪个维度被触发（`tenant` / `user` / `endpoint`）。
        dimension: String,
        /// 建议等待多少秒（正整数）。
        retry_after: i64,
    },
    /// 限流存储不可用。
    RateLimitUnavailable {
        /// 内部原因（**只进日志，不进响应体** —— AC-S-032 步骤 3 不许泄露实现细节）。
        reason: String,
    },
}

impl ObservabilityError {
    /// 错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::RateLimited { .. } => code::RATE_LIMITED,
            Self::RateLimitUnavailable { .. } => code::RATE_LIMIT_UNAVAILABLE,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            // ⚠️ 429 是限流的标准码（RFC 6585）。
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            // 存储不可用时中间件**不应**拦业务（fail open），
            // 这个码只用于**就绪探针**（AC-S-038 步骤 3）。
            Self::RateLimitUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// 面向开发者的英文 ASCII 描述（契约 C6）。
    ///
    /// # ⚠️ AC-S-032 步骤 3：不得泄露实现细节
    ///
    /// `RateLimitUnavailable` 的 `reason` 可能含表名 / SQL —— 所以这里
    /// **只输出一句固定文案**，`reason` 只进日志（`tracing::warn!`）。
    pub fn detail(&self) -> String {
        match self {
            Self::RateLimited {
                dimension,
                retry_after,
            } => format!(
                "rate limit exceeded on the {dimension} dimension; retry after {retry_after} seconds"
            ),
            Self::RateLimitUnavailable { .. } => {
                "the rate limit store is unavailable".to_string()
            }
        }
    }

    /// 补充参数（**键白名单**，AC-S-032 步骤 3）。
    ///
    /// ⚠️ `RateLimited` 的 `params` **只允许** `dimension` 与 `retryAfter` 两个键 ——
    /// 多返回 `current` / `remaining` / `bucket` 会泄露限流配置
    /// （判据是**子集断言**，见 TC-019-027）。
    pub fn params(&self) -> Option<std::collections::BTreeMap<String, serde_json::Value>> {
        match self {
            Self::RateLimited {
                dimension,
                retry_after,
            } => {
                let mut map = std::collections::BTreeMap::new();
                map.insert(
                    "dimension".to_string(),
                    serde_json::Value::from(dimension.clone()),
                );
                map.insert(
                    "retryAfter".to_string(),
                    serde_json::Value::from(*retry_after),
                );
                Some(map)
            }
            // ⚠️ 不可用路径**不带任何 params** —— 理由同上（不泄露）。
            Self::RateLimitUnavailable { .. } => None,
        }
    }

    /// 转成平台统一的 `ProblemDetails`。
    pub fn problem_details(&self, ctx: &RequestContext, instance: &str) -> ProblemDetails {
        ProblemDetails::from_context(
            self.code(),
            platform_core::titles::title_of_code(self.code()),
            self.status().as_u16(),
            self.detail(),
            ctx,
            instance,
            self.params(),
        )
    }
}

impl std::fmt::Display for ObservabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail())
    }
}

impl std::error::Error for ObservabilityError {}

impl From<ObservabilityError> for ProblemDetails {
    /// 无 `RequestContext` 时的兜底转换（`trace_id` 为空）。
    fn from(error: ObservabilityError) -> Self {
        // ⚠️ 无 `RequestContext` 时用 `ProblemDetails::new`（`trace_id` 留空）。
        ProblemDetails::new(
            error.code(),
            platform_core::titles::title_of_code(error.code()),
            error.status().as_u16(),
            error.detail(),
            "",
            "",
            error.params(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL_CODES` **恰好 2 条**且无重复。
    #[test]
    fn the_code_list_has_exactly_two_distinct_codes() {
        assert_eq!(ALL_CODES.len(), 2, "本 crate 只有 2 个自有码（其余复用平台码）");
        let unique: std::collections::BTreeSet<&str> = ALL_CODES.iter().copied().collect();
        assert_eq!(unique.len(), 2, "ALL_CODES 不得有重复");
    }

    /// 2 个变体各映射到**互不相同**的码。
    #[test]
    fn each_variant_maps_to_a_distinct_code() {
        let errors = sample_errors();
        let codes: std::collections::BTreeSet<&str> =
            errors.iter().map(ObservabilityError::code).collect();
        assert_eq!(codes.len(), 2);
        assert_eq!(codes.len(), ALL_CODES.len());
    }

    /// 429 的 `params` **只有** `dimension` 与 `retryAfter`（AC-S-032 步骤 3）。
    ///
    /// ⚠️ 这条是**子集断言** —— 多一个键就红。
    #[test]
    fn the_rate_limited_params_are_a_closed_whitelist() {
        let err = ObservabilityError::RateLimited {
            dimension: dimension::ENDPOINT.to_string(),
            retry_after: 2,
        };
        let params = err.params().expect("429 必须带 params");
        let keys: std::collections::BTreeSet<&str> = params.keys().map(String::as_str).collect();
        let allowed: std::collections::BTreeSet<&str> =
            ["dimension", "retryAfter"].into_iter().collect();
        assert!(
            keys.is_subset(&allowed),
            "params 的键必须是 {{dimension, retryAfter}} 的子集（多一个就泄露内部结构）；实际：{keys:?}"
        );
    }

    /// 不可用路径**不带** params，且 `detail` 不泄露 `reason`。
    #[test]
    fn the_unavailable_error_never_leaks_the_internal_reason() {
        let err = ObservabilityError::RateLimitUnavailable {
            reason: "SELECT count(*) FROM platform_rate_limit failed: postgres error".to_string(),
        };
        assert!(err.params().is_none(), "不可用路径不得带 params");
        let detail = err.detail();
        for leak in [
            "SELECT",
            "platform_rate_limit",
            "postgres",
            "count(*)",
        ] {
            assert!(
                !detail.contains(leak),
                "detail 不得含实现细节 {leak:?}；实际：{detail:?}"
            );
        }
        // 但 detail 必须说明是「限流存储」不可用（运维要能分辨）。
        assert!(detail.contains("rate limit"), "必须说明是限流存储：{detail}");
    }

    /// 限流码的 detail 是 ASCII（契约 C6）。
    #[test]
    fn the_rate_limited_detail_is_ascii() {
        let err = ObservabilityError::RateLimited {
            dimension: dimension::USER.to_string(),
            retry_after: 60,
        };
        assert!(err.detail().is_ascii(), "detail 必须是 ASCII：{}", err.detail());
    }

    /// 状态码：429 / 503。
    #[test]
    fn the_status_codes_are_rate_limited_and_service_unavailable() {
        assert_eq!(
            ObservabilityError::RateLimited {
                dimension: dimension::TENANT.to_string(),
                retry_after: 1
            }
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            ObservabilityError::RateLimitUnavailable {
                reason: "x".to_string()
            }
            .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    /// 三个维度常量齐备。
    #[test]
    fn the_dimension_domain_has_three_values() {
        assert_eq!(dimension::ALL.len(), 3);
        for d in dimension::ALL {
            assert!(!d.is_empty());
        }
    }

    fn sample_errors() -> Vec<ObservabilityError> {
        vec![
            ObservabilityError::RateLimited {
                dimension: dimension::ENDPOINT.to_string(),
                retry_after: 2,
            },
            ObservabilityError::RateLimitUnavailable {
                reason: "db down".to_string(),
            },
        ]
    }
}
