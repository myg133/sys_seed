//! 应用接入的**平台错误码**（REQ-016，D213 的 8 处登记 / D226 的「无死码」纪律）。
//!
//! # 归属
//!
//! 与 `platform-oauth` / `platform-audit` 同款：码的**常量**住在本 crate，
//! 而**标题**的唯一来源是 `platform_core::titles::TITLES`（D113）——
//! 面向用户的中文文案由前端 i18n 按 `code` 渲染（契约 C6）。
//!
//! 本模块提供：
//!
//! - **6 个**平台错误码的常量与枚举（`APP_*`，大写下划线）；
//! - 本 crate 的**完备性清单** [`ALL_CODES`]（`platform-server` 的
//!   `titles_cover_every_code` 会汇总它 → `TITLES` 必须 ⊇ 本清单）；
//! - 把 [`AppError`] 转成统一错误体（RFC 9457）。
//!
//! # ⚠️ 恰好 6 个 —— 每个都有一条 AC 会**真的触发**它
//!
//! | 码 | HTTP | 触发它的 AC |
//! |----|------|-------------|
//! | `APP_NOT_FOUND` | 404 | AC-S-156 步骤 6 |
//! | `APP_DUPLICATED` | 409 | AC-S-156 路径 2 |
//! | `APP_CLIENT_INVALID` | 400 | AC-S-158 路径 1/2 |
//! | `APP_LAUNCH_URL_INVALID` | 400 | AC-S-156 路径 3 |
//! | `APP_ICON_URL_INVALID` | 400 | AC-S-156 路径 4 |
//! | `APP_INVALID_REQUEST` | 400 | AC-S-156 路径 5 |
//!
//! rev 3 曾列 **8** 个，删掉的两个是**死码**（没有任何 AC 能触发它）：
//!
//! - `APP_REGISTRY_MISMATCH`：启动对账失败时**进程直接退出**，根本没有 HTTP 响应
//!   → 它不可能是 HTTP 码（AC-S-165 步骤 2 的判据是「退出码非 0 + 日志列编码」）；
//! - `APP_INVALID_SCOPE_PROJECTION`：scope 是**只读投影**（D266），应用表根本不存它
//!   → 「投影与 client 不符」这个状态**构造不出来**。
//!
//! → **规则**：每个错误码必须有一个 AC 会真的触发它，否则不许登记（D226）。

use std::collections::BTreeMap;

use axum::http::StatusCode;
use platform_core::RequestContext;
use platform_core::problem::ProblemDetails;

/// 稳定的机器可读错误码（平台侧，`APP_*`）。
pub mod code {
    /// 应用不存在（含「不属于当前租户」—— 两者对外不可区分，避免探测）。
    pub const NOT_FOUND: &str = "APP_NOT_FOUND";
    /// 同租户内 `client_id` 或 `display_name` 重复。
    pub const DUPLICATED: &str = "APP_DUPLICATED";
    /// 引用的 client 不存在 / 已停用 / 已过期。
    pub const CLIENT_INVALID: &str = "APP_CLIENT_INVALID";
    /// `launch_url` 的 scheme 非法（含 `javascript:` / `data:`）。
    pub const LAUNCH_URL_INVALID: &str = "APP_LAUNCH_URL_INVALID";
    /// `icon_url` 的 scheme 非法。
    pub const ICON_URL_INVALID: &str = "APP_ICON_URL_INVALID";
    /// 缺必填字段 / 类型错 / 字段长度非法。
    pub const INVALID_REQUEST: &str = "APP_INVALID_REQUEST";
}

/// 本 crate 的全部**平台**错误码（**完备性测试的输入**，D113）。
///
/// ⚠️ 这里**只有 6 个**：3 个 `app.*` 审计事件码**不进**本清单
/// （事件码不进 `TITLES` —— D223 同款口径）。
pub const ALL_CODES: &[&str] = &[
    code::NOT_FOUND,
    code::DUPLICATED,
    code::CLIENT_INVALID,
    code::LAUNCH_URL_INVALID,
    code::ICON_URL_INVALID,
    code::INVALID_REQUEST,
];

/// 应用接入的平台错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    /// 应用不存在。
    NotFound,
    /// 同租户内重复（`client_id` 或 `display_name`）。
    Duplicated {
        /// 冲突的字段名（`clientId` / `displayName`；**不带值**）。
        field: &'static str,
    },
    /// 引用的 client 不存在 / 已停用 / 已过期。
    ClientInvalid {
        /// 面向开发者的英文原因（固定短语，**不得**回显 client id）。
        reason: &'static str,
    },
    /// `launch_url` 的 scheme 非法。
    LaunchUrlInvalid,
    /// `icon_url` 的 scheme 非法。
    IconUrlInvalid,
    /// 请求非法（缺必填 / 类型错 / 长度非法）。
    ///
    /// `reason` 是**固定的 ASCII 短语**，**不得**含任何请求值。
    InvalidRequest {
        /// 面向开发者的英文原因。
        reason: &'static str,
    },
}

impl AppError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => code::NOT_FOUND,
            Self::Duplicated { .. } => code::DUPLICATED,
            Self::ClientInvalid { .. } => code::CLIENT_INVALID,
            Self::LaunchUrlInvalid => code::LAUNCH_URL_INVALID,
            Self::IconUrlInvalid => code::ICON_URL_INVALID,
            Self::InvalidRequest { .. } => code::INVALID_REQUEST,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Duplicated { .. } => StatusCode::CONFLICT,
            Self::ClientInvalid { .. }
            | Self::LaunchUrlInvalid
            | Self::IconUrlInvalid
            | Self::InvalidRequest { .. } => StatusCode::BAD_REQUEST,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述（契约 C6）。
    ///
    /// ⚠️ 这里**不允许**出现中文：面向用户的中文文案由前端 i18n 按 `code` 渲染。
    pub fn detail(&self) -> String {
        match self {
            Self::NotFound => "the app was not found".to_string(),
            Self::Duplicated { field } => {
                format!("an app with the same `{field}` already exists in this tenant")
            }
            Self::ClientInvalid { reason } => {
                format!("the referenced oauth client cannot be used: {reason}")
            }
            Self::LaunchUrlInvalid => {
                "the launch url must be an absolute https url (http is not allowed)".to_string()
            }
            Self::IconUrlInvalid => {
                "the icon url must be an absolute https url (http is not allowed)".to_string()
            }
            Self::InvalidRequest { reason } => format!("the app request is invalid: {reason}"),
        }
    }

    /// 错误码之外的补充参数（契约 C6 的 `params`）。
    pub fn params(&self) -> BTreeMap<String, serde_json::Value> {
        let mut params = BTreeMap::new();
        match self {
            Self::Duplicated { field } => {
                params.insert("field".to_string(), serde_json::Value::from(*field));
            }
            Self::ClientInvalid { reason } | Self::InvalidRequest { reason } => {
                params.insert("reason".to_string(), serde_json::Value::from(*reason));
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

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for AppError {}

/// 服务层的错误：**应用专有错误** + **平台通用错误**。
///
/// # 为什么要分开
///
/// `APP_*` **恰好 6 个** —— 那是应用接入**专有**的失败。
/// 「没带令牌」「数据库挂了」这类失败**不是**应用专有的 →
/// 复用平台**既有**的 `UNAUTHENTICATED` / `INTERNAL_ERROR`
/// （它们早已在 `TITLES` 与 i18n 词库里登记，D125）。
///
/// ⚠️ **不得**为它们新增第 7 个 `APP_*` 码 —— 死码就是「永远不可能出现
/// 却要占一行标题、一条词库」的负担（D226 的原话）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceError {
    /// 应用专有错误（6 个 `APP_*` 码之一）。
    App(AppError),
    /// 需要认证（缺令牌 / 令牌无效）→ **401 `UNAUTHENTICATED`**。
    Unauthenticated,
    /// 内部错误 → **500 `INTERNAL_ERROR`**（**不把内部细节漏给调用方**）。
    Internal {
        /// 面向开发者的英文原因。
        reason: String,
    },
}

impl From<AppError> for ServiceError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

impl ServiceError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::App(error) => error.code(),
            Self::Unauthenticated => platform_core::problem::CODE_UNAUTHENTICATED,
            Self::Internal { .. } => platform_core::problem::CODE_INTERNAL_ERROR,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::App(error) => error.status(),
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述。
    pub fn detail(&self) -> String {
        match self {
            Self::App(error) => error.detail(),
            Self::Unauthenticated => "authentication is required".to_string(),
            Self::Internal { reason } => format!("an internal error occurred: {reason}"),
        }
    }

    /// 把错误转成统一错误体（RFC 9457，契约 C6）。
    pub fn problem_details(&self, ctx: &RequestContext, instance: &str) -> ProblemDetails {
        match self {
            Self::App(error) => error.problem_details(ctx, instance),
            Self::Unauthenticated => ProblemDetails::unauthenticated(ctx, instance),
            Self::Internal { .. } => ProblemDetails::from_context(
                platform_core::problem::CODE_INTERNAL_ERROR,
                platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR),
                500,
                self.detail(),
                ctx,
                instance,
                None,
            ),
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

    /// 测试用的**全部变体**（D213 第 8 处：测试的两个数组之一）。
    fn all_variants() -> Vec<AppError> {
        vec![
            AppError::NotFound,
            AppError::Duplicated {
                field: "clientId",
            },
            AppError::ClientInvalid {
                reason: "the oauth client does not exist",
            },
            AppError::LaunchUrlInvalid,
            AppError::IconUrlInvalid,
            AppError::InvalidRequest {
                reason: "the display name is required",
            },
        ]
    }

    #[test]
    fn the_code_list_has_exactly_six_entries() {
        assert_eq!(ALL_CODES.len(), 6);
    }

    /// `ALL_CODES` 必须覆盖**每一个变体**（漏一个变体 → 变红）。
    #[test]
    fn all_codes_lists_every_variant() {
        use std::collections::BTreeSet;

        let variants = all_variants();
        let mut seen = BTreeSet::new();
        for error in &variants {
            assert!(
                ALL_CODES.contains(&error.code()),
                "ALL_CODES 缺少 {} 的码 {}",
                std::any::type_name::<AppError>(),
                error.code()
            );
            seen.insert(error.code());
        }
        assert_eq!(
            seen.len(),
            ALL_CODES.len(),
            "ALL_CODES 里有变体从未产出过的码（清单与 match 不一致）"
        );
    }

    /// 错误码必须是**大写 + 下划线**（i18n 的 `error.<CODE>` 键名），且以 `APP_` 开头。
    #[test]
    fn every_error_code_is_upper_snake_case() {
        for error in all_variants() {
            let code = error.code();
            assert!(!code.is_empty());
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "错误码必须是大写 + 下划线：{code}"
            );
            assert!(
                code.starts_with("APP_"),
                "本 crate 的错误码必须以 APP_ 开头：{code}"
            );
        }
    }

    /// 契约 C6：`detail` 必须是**英文 ASCII** —— 后端不得拼中文句子。
    #[test]
    fn every_detail_is_english_ascii() {
        for error in all_variants() {
            let detail = error.detail();
            assert!(detail.is_ascii(), "detail 必须是 ASCII：{detail}");
            assert!(!detail.is_empty(), "detail 不得为空");
        }
    }

    /// `detail` **不得**回显凭据 / 请求值（D251 同款口径）。
    #[test]
    fn no_detail_echoes_a_credential() {
        for error in all_variants() {
            let detail = error.detail();
            for forbidden in ["client_secret", "secret", "token", "javascript:"] {
                assert!(
                    !detail.contains(forbidden),
                    "detail 不得回显 `{forbidden}`：{detail}"
                );
            }
        }
    }

    /// 每个码都已在 `TITLES` 登记（D125）→ 否则前端会看到裸码。
    #[test]
    fn every_code_is_registered_in_titles() {
        for code in ALL_CODES {
            assert!(
                platform_core::titles::is_registered(code),
                "`{code}` 必须在 TITLES 里登记（D125）"
            );
        }
    }

    /// 状态码逐个钉死（AC-S-156 / AC-S-158）。
    #[test]
    fn the_status_codes_are_pinned() {
        assert_eq!(AppError::NotFound.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            AppError::Duplicated { field: "clientId" }.status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            AppError::ClientInvalid {
                reason: "the oauth client does not exist"
            }
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            AppError::LaunchUrlInvalid.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(AppError::IconUrlInvalid.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            AppError::InvalidRequest {
                reason: "the client id is required"
            }
            .status(),
            StatusCode::BAD_REQUEST
        );
    }

    /// 事件码**不得**进平台错误码清单（D223 同款）。
    #[test]
    fn event_codes_are_not_platform_error_codes() {
        for code in crate::events::ALL_CODES {
            assert!(
                !ALL_CODES.contains(&code),
                "事件码 `{code}` 不得进平台错误码清单"
            );
        }
    }
}
