//! 错误体的 `title`：**唯一来源**（REQ-021 / D113）。
//!
//! # 为什么要有这张表
//!
//! rev 1 的验收标准写「中文 title 8 处」，实际是 **54 处中文字面量分散在 4 个
//! `title_of()` / `title()` 函数的 `match` 里**（config 12 / iam 17 / org 15 /
//! rbac 10）。所以「扫 `ProblemDetails` 构造点的字符串字面量」这种验收方式
//! **扫完 0 命中、全部通过** —— 构造点拿到的是函数调用，不是字面量。
//!
//! 收敛成**一张只含 ASCII 的表**之后，「有没有中文」变成可机械判定的问题；
//! 而「表漏了一项」由各 crate 导出的 [`ALL_CODES`] 与
//! `platform-server` 的完备性测试 `titles_cover_every_code` 兜住
//! （`code()` 是 `match` 里的 `&'static str`，运行时**无法自动枚举** ——
//! 没有显式清单，漏项永远不会被发现）。
//!
//! # 约定
//!
//! | 字段 | 约定 |
//! |------|------|
//! | 键 | 稳定的机器可读码：错误码 `UPPER_SNAKE`；组织事件码沿用既有 `org.*` 形式 |
//! | 值 | **纯 ASCII 的英文短语** —— RFC 9457 的 `title` 是「短的人类可读摘要」，**不是**本地化文案 |
//!
//! 面向用户的中文文案由前端按 `code` 渲染（契约 C6）；
//! 未登记的 code 走 ASCII 兜底 [`FALLBACK_TITLE`]，**不返回 code 本身**
//! （返回 code 会把机器码漏进面向用户的字段，且 `type` 已经带了一份）。
//!
//! # 允许中文的地方（避免过度整改）
//!
//! `tracing` 日志、`params` 里的用户数据、代码注释 —— 这些都不是响应体的文案。

// 每个 crate 的 code 清单（`platform_core::problem` / `platform_iam` /
// `platform_org` / `platform_rbac` / `platform_config` / `platform_core::money`）
// 各自导出一份，由 `platform-server` 的完备性测试汇总后对 `TITLES` 求覆盖。
// 本模块不定义清单本身（core 的清单在 `crate::problem::ALL_CODES`）。

/// 未登记的 code 的兜底标题（ASCII，**不是** code 本身）。
pub const FALLBACK_TITLE: &str = "Error";

/// 稳定错误码 → **英文**标题。
///
/// **唯一来源**：任何 `ProblemDetails` 的 `title` 都必须经 [`title_of_code`]
/// 从这里取。表内只允许 ASCII（AC-S-088 的静态判据）。
///
/// 顺序：core → iam → org → rbac → config → money/fx → 组织事件码。
pub const TITLES: &[(&str, &str)] = &[
    // -- platform-core（统一错误体）-----------------------------------------
    ("UNAUTHENTICATED", "Unauthenticated"),
    ("INSUFFICIENT_PERMISSION", "Insufficient Permission"),
    ("NOT_FOUND", "Not Found"),
    ("INTERNAL_ERROR", "Internal Server Error"),
    ("SCHEMA_MODULE_NOT_FOUND", "Schema Module Not Found"),
    ("SCHEMA_ENTITY_NOT_FOUND", "Schema Entity Not Found"),
    // -- platform-iam ------------------------------------------------------
    ("INVALID_CREDENTIALS", "Invalid Credentials"),
    ("MFA_REQUIRED", "Multi-Factor Authentication Required"),
    ("MFA_INVALID", "Invalid Multi-Factor Authentication Code"),
    ("ACCOUNT_LOCKED", "Account Locked"),
    ("ACCOUNT_DISABLED", "Account Disabled"),
    ("TOKEN_EXPIRED", "Token Expired"),
    ("TOKEN_REVOKED", "Token Revoked"),
    ("TOKEN_REUSE_DETECTED", "Token Reuse Detected"),
    ("PASSWORD_POLICY_VIOLATION", "Password Policy Violation"),
    ("PASSWORD_REUSED", "Password Reused"),
    ("LOGIN_RATE_LIMITED", "Login Rate Limited"),
    ("TENANT_CONTEXT_MISSING", "Tenant Context Missing"),
    ("CONFLICT", "Conflict"),
    ("NOT_IMPLEMENTED", "Not Implemented"),
    // -- platform-org ------------------------------------------------------
    ("ORG_CYCLE", "Organization Cycle Detected"),
    ("ORG_NODE_NOT_EMPTY", "Organization Node Not Empty"),
    ("ORG_NODE_HAS_MEMBERS", "Organization Node Has Members"),
    ("ORG_CODE_DUPLICATED", "Organization Code Duplicated"),
    ("ORG_NODE_TYPE_INVALID", "Invalid Organization Node Type"),
    ("INVALID_ARGUMENT", "Invalid Argument"),
    ("SYNC_CONFLICT_PENDING", "Directory Sync Conflict Pending"),
    (
        "SYNC_CONFLICT_STRATEGY",
        "Directory Sync Strategy Not Allowed",
    ),
    ("SYNC_PUSH_FAILED", "Directory Sync Push Failed"),
    // -- platform-rbac -----------------------------------------------------
    ("BUILTIN_ROLE_PROTECTED", "Built-in Role Protected"),
    ("WILDCARD_FORBIDDEN", "Wildcard Permission Forbidden"),
    ("INHERITANCE_CYCLE", "Role Inheritance Cycle"),
    ("REGISTRY_MISMATCH", "Permission Registry Mismatch"),
    // -- platform-config ---------------------------------------------------
    ("CONFIG_KEY_INVALID", "Invalid Config Key"),
    ("CONFIG_KEY_NOT_REGISTERED", "Config Key Not Registered"),
    ("CONFIG_SCOPE_NOT_ALLOWED", "Config Scope Not Allowed"),
    ("CONFIG_TYPE_MISMATCH", "Config Value Type Mismatch"),
    ("CONFIG_VALUE_OUT_OF_RANGE", "Config Value Out Of Range"),
    ("CONFIG_VALUE_FORMAT_INVALID", "Config Value Format Invalid"),
    ("FLAG_NAME_INVALID", "Invalid Feature Flag Name"),
    (
        "CONFIG_FLAG_USE_FLAG_ENDPOINT",
        "Feature Flag Endpoint Required",
    ),
    ("CONFIG_TENANT_RESERVED", "Reserved Tenant Identifier"),
    // -- platform-server 的健康检查（`/health/ready` 的 503 错误体）----------
    ("DATABASE_UNAVAILABLE", "Database Unavailable"),
    ("MIGRATIONS_PENDING", "Migrations Pending"),
    ("MIGRATION_DIRTY", "Migration Dirty"),
    // -- platform-db（数据访问层）-------------------------------------------
    // 这些码经 `ConfigError::Database(code)` 进**响应体的 `detail` 字段**
    // （`crates/platform-config/src/error.rs`：`the config store is unavailable ({code})`），
    // 且 `DbError::code()` 是稳定机器可读码 → 前端可能按它分支 → 必须有标题。
    ("DATABASE_ERROR", "Database Error"),
    ("INVALID_QUERY", "Invalid Query"),
    ("RECORD_NOT_FOUND", "Record Not Found"),
    ("RLS_BYPASSED_BY_ROLE", "RLS Bypassed By Role"),
    ("RLS_POLICY_MISSING", "RLS Policy Missing"),
    // -- platform-core::money（REQ-021）-------------------------------------
    ("MONEY_OVERFLOW", "Money Overflow"),
    ("FX_RATE_NOT_FOUND", "FX Rate Not Found"),
    // -- platform-org 的通知标题（事件码，不是 HTTP 错误码）------------------
    ("org.member-added", "Organization Membership Added"),
    ("org.member-removed", "Organization Membership Removed"),
    ("org.node-moved", "Organization Node Moved"),
    ("org.node-renamed", "Organization Node Renamed"),
    ("org.node-deleted", "Organization Node Deleted"),
    ("org.user-departed", "User Departed"),
];

/// 查表；未登记的 code 返回 ASCII 兜底 [`FALLBACK_TITLE`]。
///
/// 兜底**不是** code 本身：`title` 是面向用户的摘要字段，把机器码塞进去等于
/// 把实现细节漏给用户，也让「漏登记」看起来像是「有值」（假绿）。
pub fn title_of_code(code: &str) -> &'static str {
    TITLES
        .iter()
        .find(|(registered, _)| *registered == code)
        .map(|(_, title)| *title)
        .unwrap_or(FALLBACK_TITLE)
}

/// 该 code 是否已在表里登记（完备性测试与运维自检共用）。
pub fn is_registered(code: &str) -> bool {
    TITLES.iter().any(|(registered, _)| *registered == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表里的每个值都必须是 **ASCII**（AC-S-088 的静态判据 ①）。
    #[test]
    fn every_title_is_ascii() {
        for (code, title) in TITLES {
            assert!(
                title.is_ascii(),
                "title 必须是 ASCII（不得是中文）：{code} → {title}"
            );
            assert!(!title.trim().is_empty(), "title 不得为空：{code}");
        }
    }

    /// 键唯一 —— 重复键会让「查表」结果取决于书写顺序。
    #[test]
    fn keys_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for (code, _) in TITLES {
            assert!(seen.insert(*code), "TITLES 出现重复键：{code}");
        }
    }

    /// 未登记的 code → ASCII 兜底，**不是** code 本身。
    #[test]
    fn unknown_codes_fall_back_to_an_ascii_placeholder() {
        assert_eq!(title_of_code("NO_SUCH_CODE"), FALLBACK_TITLE);
        assert!(FALLBACK_TITLE.is_ascii());
        assert_ne!(title_of_code("NO_SUCH_CODE"), "NO_SUCH_CODE");
    }

    /// 已登记的 code 取到的是表里的值（不是兜底）。
    #[test]
    fn registered_codes_resolve_to_their_table_value() {
        assert_eq!(title_of_code("UNAUTHENTICATED"), "Unauthenticated");
        assert_eq!(title_of_code("MONEY_OVERFLOW"), "Money Overflow");
        assert_eq!(title_of_code("FX_RATE_NOT_FOUND"), "FX Rate Not Found");
        assert_eq!(
            title_of_code("CONFIG_VALUE_FORMAT_INVALID"),
            "Config Value Format Invalid"
        );
        assert!(is_registered("MONEY_OVERFLOW"));
        assert!(!is_registered("NO_SUCH_CODE"));
    }

    /// 整张表**零 CJK**（汉字 / 中文标点都算）。这是 AC-S-088 唯一有效的静态判据。
    #[test]
    fn the_table_contains_no_cjk_characters() {
        for (code, title) in TITLES {
            for ch in title.chars() {
                assert!(
                    ch.is_ascii(),
                    "TITLES 里出现非 ASCII 字符 {ch:?}：{code} → {title}"
                );
            }
        }
    }
}
