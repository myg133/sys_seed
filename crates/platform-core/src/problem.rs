//! 统一错误响应体：RFC 9457（Problem Details，契约 `api-conventions v1` C6）。
//!
//! # 为什么放在 `platform-core`
//!
//! 平台里有三处要产出错误体：
//!
//! | 位置 | 谁产出 |
//! |------|--------|
//! | `platform-core` 的鉴权中间件（`require_permission`） | 401 / 403 |
//! | `platform-iam` | 账号体系的错误 |
//! | `platform-rbac` | 角色与权限体系的错误 |
//!
//! 三处各写一份 → 字段名迟早漂移（前端按 `code` 分支，一处少个字段就是线上事故）。
//! 所以**形状只有这一份**，各 crate 只提供「错误码 / 状态码 / 英文描述 / 参数」。
//!
//! # 契约 C6 的硬约束
//!
//! - `code`：稳定的机器可读错误码（大写 + 下划线）；
//! - `detail`：**面向开发者的英文 ASCII** —— 面向用户的中文文案由前端 i18n 渲染，
//!   后端**不得**在 `detail` 里拼中文句子；
//! - `trace_id`：必带（串联日志）；
//! - `instance`：出错的接口路径。

use std::collections::BTreeMap;

use axum::Json;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::context::RequestContext;
use crate::titles::title_of_code;

/// 稳定的机器可读错误码：需要认证（缺令牌 / 令牌无效）。
pub const CODE_UNAUTHENTICATED: &str = "UNAUTHENTICATED";
/// 稳定的机器可读错误码：权限不足（动作类越权，契约 C5）。
pub const CODE_INSUFFICIENT_PERMISSION: &str = "INSUFFICIENT_PERMISSION";
/// 稳定的机器可读错误码：资源不存在（资源类越权，契约 C5；不泄露存在性）。
pub const CODE_NOT_FOUND: &str = "NOT_FOUND";
/// 稳定的机器可读错误码：服务内部错误。
pub const CODE_INTERNAL_ERROR: &str = "INTERNAL_ERROR";
/// 稳定的机器可读错误码：元数据里的**模块**不存在（REQ-012）。
///
/// 与 [`CODE_SCHEMA_ENTITY_NOT_FOUND`] **分开**是有意的：模块名通常是常量（写错就知道），
/// 实体名来自 URL（用户可随便填），分开报更容易定位。
pub const CODE_SCHEMA_MODULE_NOT_FOUND: &str = "SCHEMA_MODULE_NOT_FOUND";
/// 稳定的机器可读错误码：模块存在，但**实体**没声明元数据（REQ-012）。
pub const CODE_SCHEMA_ENTITY_NOT_FOUND: &str = "SCHEMA_ENTITY_NOT_FOUND";
/// 稳定的机器可读错误码：审计载荷含敏感键，写入被**硬拒绝**（REQ-014 D134 / D156）。
///
/// ⚠️ 这是**拒绝**，不是「悄悄把字段删掉再写」—— 静默剥离会让调用方以为已经记下来了。
pub const CODE_AUDIT_SENSITIVE_PAYLOAD_REJECTED: &str = "AUDIT_SENSITIVE_PAYLOAD_REJECTED";
/// 稳定的机器可读错误码：审计链完整性校验发现断裂（REQ-014 D146）。
pub const CODE_AUDIT_CHAIN_BROKEN: &str = "AUDIT_CHAIN_BROKEN";
/// 稳定的机器可读错误码：单条审计记录不存在（REQ-014）。
pub const CODE_AUDIT_EVENT_NOT_FOUND: &str = "AUDIT_EVENT_NOT_FOUND";
/// 稳定的机器可读错误码：审计查询的过滤 / 分页 / 排序参数非法（REQ-014 D153）。
pub const CODE_AUDIT_INVALID_FILTER: &str = "AUDIT_INVALID_FILTER";
/// 稳定的机器可读错误码：审计写入失败（REQ-014 D157）。
///
/// 语义是「**宁可不登录，不可无审计地登录**」：写不进去时调用方应当拒绝这次操作。
pub const CODE_AUDIT_SINK_UNAVAILABLE: &str = "AUDIT_SINK_UNAVAILABLE";

/// 错误类型 URI 的前缀。
pub const PROBLEM_TYPE_BASE: &str = "https://errors.base.example";

/// 本模块登记进 [`crate::titles::TITLES`] 的错误码清单。
///
/// `code()` 是 `match` 里的 `&'static str`，运行时**无法自动枚举** ——
/// 没有这份显式清单，「`TITLES` 漏了一项」永远不会被测出来（D113）。
/// 完备性测试在 `platform-server/tests/i18n_titles.rs`。
pub const ALL_CODES: &[&str] = &[
    CODE_UNAUTHENTICATED,
    CODE_INSUFFICIENT_PERMISSION,
    CODE_NOT_FOUND,
    CODE_INTERNAL_ERROR,
    CODE_SCHEMA_MODULE_NOT_FOUND,
    CODE_SCHEMA_ENTITY_NOT_FOUND,
    CODE_AUDIT_SENSITIVE_PAYLOAD_REJECTED,
    CODE_AUDIT_CHAIN_BROKEN,
    CODE_AUDIT_EVENT_NOT_FOUND,
    CODE_AUDIT_INVALID_FILTER,
    CODE_AUDIT_SINK_UNAVAILABLE,
];

/// 统一错误响应体（RFC 9457）。
///
/// 字段与契约 C6 的示例**逐个对应**；`params` 为空时不出现（`skip_serializing_if`）。
#[derive(Debug, Clone, Serialize)]
pub struct ProblemDetails {
    /// 错误类型 URI。
    #[serde(rename = "type")]
    pub kind: String,
    /// 简短标题（错误类别的名字，**不是**面向用户的句子）。
    pub title: String,
    /// HTTP 状态码。
    pub status: u16,
    /// 面向开发者的**英文 ASCII** 描述。
    pub detail: String,
    /// 出错的接口路径。
    pub instance: String,
    /// 稳定的机器可读错误码。
    pub code: String,
    /// 追踪 ID（串联日志）。
    pub trace_id: String,
    /// 补充参数（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, serde_json::Value>>,
}

impl ProblemDetails {
    /// 按字段构造错误体。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        code: &str,
        title: impl Into<String>,
        status: u16,
        detail: impl Into<String>,
        instance: &str,
        trace_id: &str,
        params: Option<BTreeMap<String, serde_json::Value>>,
    ) -> Self {
        Self {
            kind: format!("{PROBLEM_TYPE_BASE}/{}", kebab_case(code)),
            title: title.into(),
            status,
            detail: detail.into(),
            instance: instance.to_string(),
            code: code.to_string(),
            trace_id: trace_id.to_string(),
            params,
        }
    }

    /// 用请求上下文里的追踪 ID 构造错误体。
    pub fn from_context(
        code: &str,
        title: impl Into<String>,
        status: u16,
        detail: impl Into<String>,
        ctx: &RequestContext,
        instance: &str,
        params: Option<BTreeMap<String, serde_json::Value>>,
    ) -> Self {
        Self::new(
            code,
            title,
            status,
            detail,
            instance,
            ctx.trace_id(),
            params,
        )
    }

    /// 构造「需要认证」错误体（401）。
    pub fn unauthenticated(ctx: &RequestContext, instance: &str) -> Self {
        Self::from_context(
            CODE_UNAUTHENTICATED,
            title_of_code(CODE_UNAUTHENTICATED),
            401,
            "authentication is required",
            ctx,
            instance,
            None,
        )
    }

    /// 构造「权限不足」错误体（403）。
    ///
    /// `detail` **可以**含被拒的权限点编码（那是开发者信息），
    /// 但**不得**含被拒资源的业务细节（标题 / 金额 / 归属人 / 组织节点）。
    pub fn insufficient_permission(ctx: &RequestContext, instance: &str, permission: &str) -> Self {
        let mut params = BTreeMap::new();
        params.insert(
            "required".to_string(),
            serde_json::Value::from(permission.to_string()),
        );
        Self::from_context(
            CODE_INSUFFICIENT_PERMISSION,
            title_of_code(CODE_INSUFFICIENT_PERMISSION),
            403,
            format!("the current role does not grant the permission `{permission}`"),
            ctx,
            instance,
            Some(params),
        )
    }

    /// 构造「资源不存在」错误体（404）。
    ///
    /// 「无权访问的资源」与「不存在的资源」用**同一个**错误体 ——
    /// 两者**必须**无法区分（契约 C5 的资源类口径）。
    pub fn not_found(ctx: &RequestContext, instance: &str, resource: &str) -> Self {
        let mut params = BTreeMap::new();
        params.insert(
            "resource".to_string(),
            serde_json::Value::from(resource.to_string()),
        );
        Self::from_context(
            CODE_NOT_FOUND,
            title_of_code(CODE_NOT_FOUND),
            404,
            format!("the {resource} was not found"),
            ctx,
            instance,
            Some(params),
        )
    }

    /// 构造「审计载荷含敏感键」错误体（400，REQ-014 D134 / D156）。
    ///
    /// ⚠️ `keys` 里**只有键名**，**没有值** —— 回显值等于再泄漏一次。
    pub fn audit_sensitive_payload_rejected(
        ctx: &RequestContext,
        instance: &str,
        keys: &[String],
    ) -> Self {
        let mut params = BTreeMap::new();
        params.insert("keys".to_string(), serde_json::Value::from(keys.to_vec()));
        Self::from_context(
            CODE_AUDIT_SENSITIVE_PAYLOAD_REJECTED,
            title_of_code(CODE_AUDIT_SENSITIVE_PAYLOAD_REJECTED),
            400,
            format!(
                "the audit payload contains sensitive keys: {}",
                keys.join(", ")
            ),
            ctx,
            instance,
            Some(params),
        )
    }

    /// 构造「审计链断裂」错误体（409，REQ-014 D146）。
    pub fn audit_chain_broken(ctx: &RequestContext, instance: &str, broken: usize) -> Self {
        let mut params = BTreeMap::new();
        params.insert("broken".to_string(), serde_json::Value::from(broken));
        Self::from_context(
            CODE_AUDIT_CHAIN_BROKEN,
            title_of_code(CODE_AUDIT_CHAIN_BROKEN),
            409,
            format!("the audit chain integrity check found {broken} broken entries"),
            ctx,
            instance,
            Some(params),
        )
    }

    /// 构造「审计记录不存在」错误体（404，REQ-014）。
    pub fn audit_event_not_found(ctx: &RequestContext, instance: &str) -> Self {
        Self::from_context(
            CODE_AUDIT_EVENT_NOT_FOUND,
            title_of_code(CODE_AUDIT_EVENT_NOT_FOUND),
            404,
            "the audit event was not found",
            ctx,
            instance,
            None,
        )
    }

    /// 构造「审计查询参数非法」错误体（400，REQ-014 D153）。
    pub fn audit_invalid_filter(ctx: &RequestContext, instance: &str, reason: &str) -> Self {
        let mut params = BTreeMap::new();
        params.insert("reason".to_string(), serde_json::Value::from(reason));
        Self::from_context(
            CODE_AUDIT_INVALID_FILTER,
            title_of_code(CODE_AUDIT_INVALID_FILTER),
            400,
            format!("the audit query is invalid: {reason}"),
            ctx,
            instance,
            Some(params),
        )
    }

    /// 构造「审计写入不可用」错误体（503，REQ-014 D157）。
    pub fn audit_sink_unavailable(ctx: &RequestContext, instance: &str, reason: &str) -> Self {
        let mut params = BTreeMap::new();
        params.insert("reason".to_string(), serde_json::Value::from(reason));
        Self::from_context(
            CODE_AUDIT_SINK_UNAVAILABLE,
            title_of_code(CODE_AUDIT_SINK_UNAVAILABLE),
            503,
            format!("the audit sink is unavailable: {reason}"),
            ctx,
            instance,
            Some(params),
        )
    }
}

/// 把错误体变成 HTTP 响应。
///
/// 运维视角的中文说明放**日志**，不进响应体（契约 C6）。
pub fn problem_response(details: &ProblemDetails) -> Response {
    tracing::warn!(
        code = %details.code,
        status = details.status,
        trace_id = %details.trace_id,
        "请求被拒绝（面向用户的中文文案由前端按 code 渲染）"
    );
    (
        axum::http::StatusCode::from_u16(details.status)
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
        Json(details),
    )
        .into_response()
}

/// 错误码 → kebab-case（用于 `type` 字段）。
pub fn kebab_case(code: &str) -> String {
    code.to_ascii_lowercase().replace('_', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new().with_tenant("tenant-a")
    }

    #[test]
    fn the_body_carries_every_field_the_contract_requires() {
        let details = ProblemDetails::unauthenticated(&ctx(), "/api/rbac/roles");
        let value = serde_json::to_value(&details).expect("应当能序列化");
        assert_eq!(value["code"], "UNAUTHENTICATED");
        assert_eq!(value["status"], 401);
        assert_eq!(value["instance"], "/api/rbac/roles");
        assert_eq!(value["type"], "https://errors.base.example/unauthenticated");
        assert!(!value["trace_id"].as_str().unwrap_or_default().is_empty());
        assert!(
            value["detail"].as_str().unwrap_or_default().is_ascii(),
            "detail 必须是英文 ASCII：{value}"
        );
    }

    /// 契约 C6：`detail` **不得**是中文句子。
    #[test]
    fn every_detail_is_english_ascii() {
        let details = [
            ProblemDetails::unauthenticated(&ctx(), "/x"),
            ProblemDetails::insufficient_permission(&ctx(), "/x", "sandbox:demo:read"),
            ProblemDetails::not_found(&ctx(), "/x", "role"),
        ];
        for detail in &details {
            let text = detail.detail.clone();
            assert!(text.is_ascii(), "detail 必须是 ASCII：{text}");
            assert!(!text.is_empty());
        }
    }

    /// 403 的 `detail` 可以带权限点编码，但**不得**带资源细节。
    #[test]
    fn insufficient_permission_names_the_permission_but_not_the_resource() {
        let details =
            ProblemDetails::insufficient_permission(&ctx(), "/api/rbac/roles", "rbac:role:write");
        assert!(details.detail.contains("rbac:role:write"));
        assert_eq!(
            details.params.as_ref().and_then(|p| p.get("required")),
            Some(&serde_json::Value::from("rbac:role:write"))
        );
        // 不得出现资源标识类的细节。
        for forbidden in ["org_node_id", "amount", "title", "created_by"] {
            assert!(
                !details.detail.contains(forbidden),
                "detail 不得出现资源细节 `{forbidden}`：{}",
                details.detail
            );
        }
    }

    /// 「无权访问的资源」与「不存在的资源」的错误体结构必须一致。
    #[test]
    fn not_found_shape_is_stable() {
        let details = ProblemDetails::not_found(&ctx(), "/api/rbac/roles/x", "role");
        assert_eq!(details.status, 404);
        assert_eq!(details.code, "NOT_FOUND");
        assert!(details.detail.contains("role"));
    }

    /// `params` 为空时不出现在响应体里（字段集合稳定）。
    #[test]
    fn empty_params_are_omitted() {
        let details = ProblemDetails::unauthenticated(&ctx(), "/x");
        let value = serde_json::to_value(&details).expect("应当能序列化");
        assert!(value.get("params").is_none());
    }

    /// REQ-014 的 5 个新错误码：**逐个**断言 code / status / detail。
    #[test]
    fn the_audit_problem_details_carry_their_stable_codes() {
        let details = [
            ProblemDetails::audit_sensitive_payload_rejected(
                &ctx(),
                "/api/audit/events",
                &["password".to_string()],
            ),
            ProblemDetails::audit_chain_broken(&ctx(), "/api/audit/verify", 1),
            ProblemDetails::audit_event_not_found(&ctx(), "/api/audit/events/x"),
            ProblemDetails::audit_invalid_filter(
                &ctx(),
                "/api/audit/events",
                "limit must be positive",
            ),
            ProblemDetails::audit_sink_unavailable(
                &ctx(),
                "/api/iam/login",
                "the database is down",
            ),
        ];
        let expected = [
            ("AUDIT_SENSITIVE_PAYLOAD_REJECTED", 400),
            ("AUDIT_CHAIN_BROKEN", 409),
            ("AUDIT_EVENT_NOT_FOUND", 404),
            ("AUDIT_INVALID_FILTER", 400),
            ("AUDIT_SINK_UNAVAILABLE", 503),
        ];
        assert_eq!(details.len(), expected.len());
        for (details, (code, status)) in details.iter().zip(expected) {
            assert_eq!(details.code, code);
            assert_eq!(details.status, status);
            assert!(
                details.detail.is_ascii(),
                "detail must be english ASCII: {}",
                details.detail
            );
            assert_eq!(details.title, title_of_code(code));
        }
    }

    /// 敏感键被拒时，错误体**只含键名**、**不含值**（AC-S-133 step 4）。
    #[test]
    fn the_sensitive_rejection_body_does_not_echo_values() {
        let details = ProblemDetails::audit_sensitive_payload_rejected(
            &ctx(),
            "/api/audit/events",
            &["password".to_string()],
        );
        let value = serde_json::to_value(&details).expect("should serialize");
        assert_eq!(value["code"], "AUDIT_SENSITIVE_PAYLOAD_REJECTED");
        assert_eq!(value["params"]["keys"][0], "password");
        let text = serde_json::to_string(&value).expect("should serialize");
        assert!(
            !text.contains("p@ssw0rd"),
            "the body must not echo the value: {text}"
        );
    }

    /// 5 个新码都进了 [`ALL_CODES`]（完备性清单漏一项 = 静默走兜底）。
    #[test]
    fn every_audit_code_is_registered_in_all_codes() {
        for code in [
            CODE_AUDIT_SENSITIVE_PAYLOAD_REJECTED,
            CODE_AUDIT_CHAIN_BROKEN,
            CODE_AUDIT_EVENT_NOT_FOUND,
            CODE_AUDIT_INVALID_FILTER,
            CODE_AUDIT_SINK_UNAVAILABLE,
        ] {
            assert!(
                ALL_CODES.contains(&code),
                "`{code}` must be registered in ALL_CODES"
            );
        }
    }
}
