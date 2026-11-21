//! 通知系统的**平台错误码**（REQ-015，D303 / D125 的 8 处登记 / D226 的「无死码」纪律）。
//!
//! # 归属
//!
//! 与 `platform-oauth` / `platform-audit` / `platform-app` 同款：码的**常量**住在本 crate，
//! 而**标题**的唯一来源是 `platform_core::titles::TITLES`（D113）——
//! 面向用户的中文文案由前端 i18n 按 `code` 渲染（契约 C6）。
//!
//! 本模块提供：
//!
//! - **8 个**平台错误码的常量与枚举（`NOTIFY_*`，大写下划线）；
//! - 本 crate 的**完备性清单** [`ALL_CODES`]（`platform-server` 的
//!   `titles_cover_every_code` 会汇总它 → `TITLES` 必须 ⊇ 本清单）；
//! - 把 [`NotifyError`] 转成统一错误体（RFC 9457）。
//!
//! # ⚠️ 恰好 8 个 —— 每个都有一条 AC 会**真的触发**它（D303）
//!
//! | 码 | HTTP | 触发它的 AC |
//! |----|------|-------------|
//! | `NOTIFY_TEMPLATE_NOT_FOUND` | 404 | AC-S-167 步骤 10 / AC-S-168 步骤 4 与 5 |
//! | `NOTIFY_TEMPLATE_DUPLICATED` | 409 | AC-S-167 步骤 9 |
//! | `NOTIFY_TEMPLATE_MISSING_VARIABLE` | 400 | AC-S-167 步骤 7 |
//! | `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER` | 400 | AC-S-167 步骤 6 与 11 |
//! | `NOTIFY_CHANNEL_NOT_CONFIGURED` | 400 | AC-S-166 步骤 3 与 4 |
//! | `NOTIFY_CATEGORY_MANDATORY` | 400 | AC-S-169 步骤 5 |
//! | `NOTIFY_NOT_DEAD_LETTER` | 409 | AC-S-172 步骤 2 与 3 |
//! | `NOTIFY_SENSITIVE_PAYLOAD` | 400 | AC-S-173 步骤 4 |
//!
//! # ⚠️ **不**新增「投递记录不存在」专用码（AC-S-172 步骤 4 的说明）
//!
//! 重投一个不存在的 `deliveryId` → **404 平台通用 `NOT_FOUND`**。
//! 理由：一个**只有一条 AC 的 404 分支**不值得占 `TITLES` 一行 + 两条 i18n 词条。
//! 反之亦然 —— **每个错误码必须有 AC 真的触发它**（D226）。
//!
//! # ⚠️ 4 个 `notify.*` 审计事件码**不进** [`ALL_CODES`]
//!
//! 事件码不是 HTTP 错误码 → 不进 `TITLES`（D223 同款口径，见 [`crate::events`]）。

use std::collections::BTreeMap;

use axum::http::StatusCode;
use platform_core::RequestContext;
use platform_core::problem::ProblemDetails;

/// 稳定的机器可读错误码（平台侧，`NOTIFY_*`）。
pub mod code {
    /// 模板不存在（含「不属于当前租户」—— 两者对外不可区分，避免探测）。
    pub const TEMPLATE_NOT_FOUND: &str = "NOTIFY_TEMPLATE_NOT_FOUND";
    /// 同 `(code, channel, locale)` 已有模板（D294 的唯一约束）。
    pub const TEMPLATE_DUPLICATED: &str = "NOTIFY_TEMPLATE_DUPLICATED";
    /// 渲染时 `payload` 缺少模板引用的变量。
    pub const TEMPLATE_MISSING_VARIABLE: &str = "NOTIFY_TEMPLATE_MISSING_VARIABLE";
    /// `{{...}}` 里是非法变量名（含条件 / 循环 / 表达式语法，D293）。
    pub const TEMPLATE_INVALID_PLACEHOLDER: &str = "NOTIFY_TEMPLATE_INVALID_PLACEHOLDER";
    /// 通道未配置（短信 / IM 四类，D292）。
    pub const CHANNEL_NOT_CONFIGURED: &str = "NOTIFY_CHANNEL_NOT_CONFIGURED";
    /// 试图退订 `system` 类（强制通知不可退订，D295）。
    pub const CATEGORY_MANDATORY: &str = "NOTIFY_CATEGORY_MANDATORY";
    /// 重投一个**不是**死信的记录（D298 的状态机守卫）。
    pub const NOT_DEAD_LETTER: &str = "NOTIFY_NOT_DEAD_LETTER";
    /// `payload` 含敏感键（复用 REQ-014 的 `scan_sensitive_keys`，D299）。
    pub const SENSITIVE_PAYLOAD: &str = "NOTIFY_SENSITIVE_PAYLOAD";
}

/// 本 crate 的全部**平台**错误码（**完备性测试的输入**，D113）。
///
/// ⚠️ 这里**只有 8 个**：4 个 `notify.*` 审计事件码**不进**本清单
/// （事件码不进 `TITLES` —— D223 同款口径）。
pub const ALL_CODES: &[&str] = &[
    code::TEMPLATE_NOT_FOUND,
    code::TEMPLATE_DUPLICATED,
    code::TEMPLATE_MISSING_VARIABLE,
    code::TEMPLATE_INVALID_PLACEHOLDER,
    code::CHANNEL_NOT_CONFIGURED,
    code::CATEGORY_MANDATORY,
    code::NOT_DEAD_LETTER,
    code::SENSITIVE_PAYLOAD,
];

/// 通知系统的平台错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyError {
    /// 模板不存在。
    TemplateNotFound,
    /// 同 `(code, channel, locale)` 重复。
    TemplateDuplicated,
    /// 渲染时缺少变量。
    ///
    /// `variable` 是**模板里写的变量名**（已通过白名单校验，只含 `[a-z0-9_]`），
    /// 可以回显 —— 用户需要知道该补哪个变量（AC-S-167 步骤 7 要求「点名」）。
    TemplateMissingVariable {
        /// 缺失的变量名。
        variable: String,
    },
    /// `{{...}}` 里是非法变量名。
    TemplateInvalidPlaceholder {
        /// 面向开发者的英文原因（固定短语，**不得**回显原始占位符内容 ——
        /// 它可能含 SSTI 载荷，回显会让载荷进入响应体）。
        reason: &'static str,
    },
    /// 通道未配置。
    ChannelNotConfigured {
        /// 通道名（`sms` / `dingtalk` / `wecom` / `feishu`）。
        channel: &'static str,
    },
    /// 试图退订强制类。
    CategoryMandatory {
        /// 分类名（固定为 `system`）。
        category: &'static str,
    },
    /// 重投一个非死信的记录。
    NotDeadLetter {
        /// 该记录当前的状态（`pending` / `delivering` / `delivered`）。
        status: &'static str,
    },
    /// `payload` 含敏感键。
    ///
    /// `keys` 是**命中的键名**（不含值），可用于定位。
    SensitivePayload {
        /// 命中的敏感键（已归一化，如 `password` / `client_secret`）。
        keys: Vec<String>,
    },
}

impl NotifyError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::TemplateNotFound => code::TEMPLATE_NOT_FOUND,
            Self::TemplateDuplicated => code::TEMPLATE_DUPLICATED,
            Self::TemplateMissingVariable { .. } => code::TEMPLATE_MISSING_VARIABLE,
            Self::TemplateInvalidPlaceholder { .. } => code::TEMPLATE_INVALID_PLACEHOLDER,
            Self::ChannelNotConfigured { .. } => code::CHANNEL_NOT_CONFIGURED,
            Self::CategoryMandatory { .. } => code::CATEGORY_MANDATORY,
            Self::NotDeadLetter { .. } => code::NOT_DEAD_LETTER,
            Self::SensitivePayload { .. } => code::SENSITIVE_PAYLOAD,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::TemplateNotFound => StatusCode::NOT_FOUND,
            Self::TemplateDuplicated | Self::NotDeadLetter { .. } => StatusCode::CONFLICT,
            Self::TemplateMissingVariable { .. }
            | Self::TemplateInvalidPlaceholder { .. }
            | Self::ChannelNotConfigured { .. }
            | Self::CategoryMandatory { .. }
            | Self::SensitivePayload { .. } => StatusCode::BAD_REQUEST,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述（契约 C6）。
    ///
    /// ⚠️ 这里**不允许**出现中文：面向用户的中文文案由前端 i18n 按 `code` 渲染。
    /// ⚠️ **不得**回显请求里的原始字符串（模板原文 / payload 值 / 占位符内容）——
    /// 它们可能含 SSTI 载荷或敏感值。
    pub fn detail(&self) -> String {
        match self {
            Self::TemplateNotFound => "the notification template was not found".to_string(),
            Self::TemplateDuplicated => {
                "a template with the same code, channel and locale already exists in this tenant"
                    .to_string()
            }
            Self::TemplateMissingVariable { variable } => {
                format!("the template references a variable absent from the payload: {variable}")
            }
            Self::TemplateInvalidPlaceholder { reason } => {
                format!("the template contains an invalid placeholder: {reason}")
            }
            Self::ChannelNotConfigured { channel } => {
                format!("the notification channel is not configured: {channel}")
            }
            Self::CategoryMandatory { category } => {
                format!("the notification category cannot be unsubscribed: {category}")
            }
            Self::NotDeadLetter { status } => {
                format!("the delivery is not in the dead_letter state (current: {status})")
            }
            Self::SensitivePayload { keys } => {
                format!(
                    "the notification payload contains sensitive keys: {}",
                    keys.join(", ")
                )
            }
        }
    }

    /// 错误码之外的补充参数（契约 C6 的 `params`）。
    pub fn params(&self) -> BTreeMap<String, serde_json::Value> {
        let mut params = BTreeMap::new();
        match self {
            Self::TemplateMissingVariable { variable } => {
                params.insert(
                    "variable".to_string(),
                    serde_json::Value::from(variable.clone()),
                );
            }
            Self::TemplateInvalidPlaceholder { reason } => {
                params.insert("reason".to_string(), serde_json::Value::from(*reason));
            }
            Self::ChannelNotConfigured { channel } => {
                params.insert("channel".to_string(), serde_json::Value::from(*channel));
            }
            Self::CategoryMandatory { category } => {
                params.insert("category".to_string(), serde_json::Value::from(*category));
            }
            Self::NotDeadLetter { status } => {
                params.insert("status".to_string(), serde_json::Value::from(*status));
            }
            Self::SensitivePayload { keys } => {
                params.insert("keys".to_string(), serde_json::Value::from(keys.clone()));
            }
            _ => {}
        }
        params
    }

    /// 把错误转成统一错误体（RFC 9457，契约 C6）。
    pub fn problem_details(&self, ctx: &RequestContext, instance: &str) -> ProblemDetails {
        let params = self.params();
        ProblemDetails::from_context(
            self.code(),
            platform_core::titles::title_of_code(self.code()),
            self.status().as_u16(),
            self.detail(),
            ctx,
            instance,
            if params.is_empty() {
                None
            } else {
                Some(params)
            },
        )
    }
}

impl std::fmt::Display for NotifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for NotifyError {}

/// 服务层的错误：**通知专有错误** + **平台通用错误**。
///
/// # 为什么要分开
///
/// `NOTIFY_*` **恰好 8 个** —— 那是通知**专有**的失败。
/// 「没带令牌」「数据库挂了」这类失败**不是**通知专有的 →
/// 复用平台**既有**的 `UNAUTHENTICATED` / `INTERNAL_ERROR`
/// （它们早已在 `TITLES` 与 i18n 词库里登记，D125）。
///
/// ⚠️ **不得**为它们新增第 9 个 `NOTIFY_*` 码 —— 死码就是「永远不可能出现
/// 却要占一行标题、一条词库」的负担（D226 的原话）。
#[derive(Debug)]
pub enum ServiceError {
    /// 通知专有错误。
    Notify(NotifyError),
    /// 需要认证（缺令牌 / 令牌无效）→ **401 `UNAUTHENTICATED`**。
    Unauthenticated,
    /// 内部错误（含数据访问层失败）→ **500 `INTERNAL_ERROR`**
    /// （⚠️ **不把内部细节漏给调用方** —— 与 `platform-app` 同款）。
    Internal {
        /// 面向开发者的英文原因（**不含** SQL 原文 / 凭据）。
        reason: String,
    },
}

impl ServiceError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Notify(err) => err.code(),
            Self::Unauthenticated => platform_core::problem::CODE_UNAUTHENTICATED,
            Self::Internal { .. } => platform_core::problem::CODE_INTERNAL_ERROR,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Notify(err) => err.status(),
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 面向开发者的英文 ASCII 描述。
    pub fn detail(&self) -> String {
        match self {
            Self::Notify(err) => err.detail(),
            Self::Unauthenticated => "authentication is required".to_string(),
            Self::Internal { reason } => format!("an internal error occurred: {reason}"),
        }
    }

    /// 把错误转成统一错误体（RFC 9457，契约 C6）。
    pub fn problem_details(&self, ctx: &RequestContext, instance: &str) -> ProblemDetails {
        match self {
            Self::Notify(err) => err.problem_details(ctx, instance),
            Self::Unauthenticated => ProblemDetails::unauthenticated(ctx, instance),
            Self::Internal { .. } => ProblemDetails::from_context(
                self.code(),
                platform_core::titles::title_of_code(self.code()),
                500,
                self.detail(),
                ctx,
                instance,
                None,
            ),
        }
    }
}

impl From<NotifyError> for ServiceError {
    fn from(err: NotifyError) -> Self {
        Self::Notify(err)
    }
}

/// 数据访问层失败 → **500 `INTERNAL_ERROR`**。
///
/// ⚠️ **只带 `DbError::code()`**（稳定机器码），**不带** `DbError` 的 `Display`
/// —— 后者可能含 SQL 片段或表名。这与 `platform-app` 的 `ServiceError::Internal` 同款。
impl From<platform_db::error::DbError> for ServiceError {
    fn from(err: platform_db::error::DbError) -> Self {
        // ⚠️ `reason` 里的**完整细节**只进 `tracing`（运维可查），
        // `detail()` 只回显 `code` —— 不让 SQL 片段 / 表名进响应体。
        tracing::error!(error = %err, "通知系统的数据库操作失败");
        Self::Internal {
            reason: format!("database error ({})", err.code()),
        }
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for ServiceError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL_CODES` **恰好 8 条**且**无重复**（D303）。
    #[test]
    fn the_code_list_has_exactly_eight_distinct_codes() {
        assert_eq!(ALL_CODES.len(), 8, "D303 把基线钉死为 8 个");
        let unique: std::collections::BTreeSet<&str> = ALL_CODES.iter().copied().collect();
        assert_eq!(unique.len(), 8, "ALL_CODES 不得有重复");
    }

    /// 每个码都以 `NOTIFY_` 开头（命名空间一致）。
    #[test]
    fn every_code_is_in_the_notify_namespace() {
        for code in ALL_CODES {
            assert!(
                code.starts_with("NOTIFY_"),
                "错误码必须以 NOTIFY_ 开头：{code}"
            );
        }
    }

    /// 每个码都是**纯 ASCII 大写下划线**（i18n 键与 `TITLES` 的共用约束）。
    #[test]
    fn every_code_is_ascii_upper_snake() {
        for code in ALL_CODES {
            assert!(
                code.chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                "错误码必须是 ASCII 大写下划线：{code}"
            );
        }
    }

    /// 8 个变体各自映射到**互不相同**的码（复制粘贴漏改会让两个错误共用一个码）。
    #[test]
    fn each_variant_maps_to_a_distinct_code() {
        let errors = [
            NotifyError::TemplateNotFound,
            NotifyError::TemplateDuplicated,
            NotifyError::TemplateMissingVariable {
                variable: "name".to_string(),
            },
            NotifyError::TemplateInvalidPlaceholder { reason: "test" },
            NotifyError::ChannelNotConfigured { channel: "sms" },
            NotifyError::CategoryMandatory { category: "system" },
            NotifyError::NotDeadLetter {
                status: "delivered",
            },
            NotifyError::SensitivePayload {
                keys: vec!["password".to_string()],
            },
        ];
        let codes: std::collections::BTreeSet<&str> =
            errors.iter().map(NotifyError::code).collect();
        assert_eq!(codes.len(), 8, "8 个变体必须映射到 8 个不同的码");
        assert_eq!(codes.len(), ALL_CODES.len(), "变体数与 ALL_CODES 必须一致");
    }

    /// 每个码都在 `ALL_CODES` 里（防止新增变体时忘登记 → i18n 测试假绿）。
    #[test]
    fn every_variant_code_is_registered_in_all_codes() {
        let declared = [
            NotifyError::TemplateNotFound,
            NotifyError::TemplateDuplicated,
            NotifyError::TemplateMissingVariable {
                variable: "a".to_string(),
            },
            NotifyError::TemplateInvalidPlaceholder { reason: "test" },
            NotifyError::ChannelNotConfigured { channel: "sms" },
            NotifyError::CategoryMandatory { category: "system" },
            NotifyError::NotDeadLetter { status: "pending" },
            NotifyError::SensitivePayload { keys: vec![] },
        ];
        for err in &declared {
            assert!(
                ALL_CODES.contains(&err.code()),
                "变体 {} 的码没登记进 ALL_CODES",
                err.code()
            );
        }
    }

    /// HTTP 状态码逐条核对（D303 的表格）。
    #[test]
    fn every_code_maps_to_the_documented_status() {
        assert_eq!(
            NotifyError::TemplateNotFound.status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            NotifyError::TemplateDuplicated.status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            NotifyError::TemplateMissingVariable {
                variable: "a".to_string()
            }
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            NotifyError::TemplateInvalidPlaceholder { reason: "x" }.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            NotifyError::ChannelNotConfigured { channel: "sms" }.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            NotifyError::CategoryMandatory { category: "system" }.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            NotifyError::NotDeadLetter {
                status: "delivered"
            }
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            NotifyError::SensitivePayload { keys: vec![] }.status(),
            StatusCode::BAD_REQUEST
        );
    }

    /// `detail()` **全是 ASCII**（契约 C6：面向用户的文案由前端 i18n 渲染）。
    #[test]
    fn every_detail_is_ascii() {
        let errors = [
            NotifyError::TemplateNotFound,
            NotifyError::TemplateDuplicated,
            NotifyError::TemplateMissingVariable {
                variable: "order_id".to_string(),
            },
            NotifyError::TemplateInvalidPlaceholder {
                reason: "not a name",
            },
            NotifyError::ChannelNotConfigured { channel: "feishu" },
            NotifyError::CategoryMandatory { category: "system" },
            NotifyError::NotDeadLetter {
                status: "delivered",
            },
            NotifyError::SensitivePayload {
                keys: vec!["password".to_string()],
            },
        ];
        for err in &errors {
            assert!(
                err.detail().is_ascii(),
                "detail 必须是 ASCII：{} → {}",
                err.code(),
                err.detail()
            );
        }
    }

    /// 缺变量错误**点名**那个变量（AC-S-167 步骤 7：用户需要知道补哪个）。
    #[test]
    fn the_missing_variable_error_names_the_variable() {
        let err = NotifyError::TemplateMissingVariable {
            variable: "order_id".to_string(),
        };
        assert!(
            err.detail().contains("order_id"),
            "缺变量错误必须点名变量：{}",
            err.detail()
        );
        assert_eq!(
            err.params().get("variable").map(|v| v.as_str()),
            Some(Some("order_id")),
            "params 里也要带 variable"
        );
    }

    /// 敏感 payload 错误列出**命中的键**（不含值）。
    #[test]
    fn the_sensitive_payload_error_lists_keys_without_values() {
        let err = NotifyError::SensitivePayload {
            keys: vec!["password".to_string(), "client_secret".to_string()],
        };
        let detail = err.detail();
        assert!(detail.contains("password"), "必须列出命中的键：{detail}");
        assert!(
            detail.contains("client_secret"),
            "必须列出全部命中的键：{detail}"
        );
    }

    /// 非法占位符错误**不回显**原始占位符内容（防止 SSTI 载荷进响应体）。
    #[test]
    fn the_invalid_placeholder_error_does_not_echo_the_payload() {
        // 构造时 reason 是固定的 `&'static str`，**接口上就回显不了**原始模板内容。
        // 这条测试钉住那个设计：reason 的类型是 `&'static str`，不是 `String`。
        let err = NotifyError::TemplateInvalidPlaceholder {
            reason: "the placeholder is not a valid variable name",
        };
        let detail = err.detail();
        assert!(
            !detail.contains("constructor"),
            "不得回显 SSTI 载荷：{detail}"
        );
        assert!(!detail.contains("{{"), "不得回显原始占位符：{detail}");
    }

    /// `code()` 与 `detail()` 在 `Display` 里都出现（日志可读）。
    #[test]
    fn the_display_contains_code_and_detail() {
        let err = NotifyError::TemplateNotFound;
        let text = err.to_string();
        assert!(text.contains(err.code()), "Display 必须含码：{text}");
        assert!(
            text.contains(&err.detail()),
            "Display 必须含 detail：{text}"
        );
    }
}
