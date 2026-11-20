//! 审计链上的错误码（REQ-014）。
//!
//! # 归属
//!
//! 5 个错误码的**常量**住在 `platform-core`（[`platform_core::problem`]）——
//! 错误码表是平台级的唯一来源（契约 C6），各 crate 只提供「错误码 / 状态码 /
//! 英文描述 / 参数」。
//!
//! 本模块负责：
//!
//! - 把 core 的 5 个码**再导出**一次（业务模块引 `platform-audit` 就够了，
//!   不用同时引 core 与 audit）；
//! - 给出本 crate 的**完备性清单** [`ALL_CODES`]（`platform-server` 的
//!   `titles_cover_every_code` 会汇总它）；
//! - 把 [`AuditError`] 转成统一错误体（RFC 9457）。

use platform_core::RequestContext;
use platform_core::extension::audit::AuditError;
use platform_core::problem::{
    CODE_AUDIT_CHAIN_BROKEN, CODE_AUDIT_EVENT_NOT_FOUND, CODE_AUDIT_INVALID_FILTER,
    CODE_AUDIT_SENSITIVE_PAYLOAD_REJECTED, CODE_AUDIT_SINK_UNAVAILABLE, ProblemDetails,
};

pub use platform_core::extension::audit::AuditError as Error;

/// 本 crate 的全部错误码（**完备性测试的输入**，D113）。
pub const ALL_CODES: &[&str] = &[
    CODE_AUDIT_SENSITIVE_PAYLOAD_REJECTED,
    CODE_AUDIT_CHAIN_BROKEN,
    CODE_AUDIT_EVENT_NOT_FOUND,
    CODE_AUDIT_INVALID_FILTER,
    CODE_AUDIT_SINK_UNAVAILABLE,
];

/// 把审计错误转成统一错误体（RFC 9457，契约 C6）。
pub fn problem_details(error: &AuditError, ctx: &RequestContext, instance: &str) -> ProblemDetails {
    error.problem_details(ctx, instance)
}

/// 错误码 → 稳定标题（供调用方拼自定义错误体时复用）。
pub fn title_of(code: &str) -> &'static str {
    platform_core::titles::title_of_code(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_list_has_exactly_five_entries() {
        assert_eq!(ALL_CODES.len(), 5);
        for code in ALL_CODES {
            assert!(
                platform_core::titles::is_registered(code),
                "`{code}` 必须在 TITLES 里登记（D125）"
            );
        }
    }

    #[test]
    fn every_code_is_in_the_platform_core_list() {
        for code in ALL_CODES {
            assert!(
                platform_core::problem::ALL_CODES.contains(code),
                "`{code}` 必须同时登记进 platform_core::problem::ALL_CODES"
            );
        }
    }
}
