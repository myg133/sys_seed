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
    ("INVALID_REQUEST", "Invalid Request"),
    ("INSUFFICIENT_PERMISSION", "Insufficient Permission"),
    ("NOT_FOUND", "Not Found"),
    ("INTERNAL_ERROR", "Internal Server Error"),
    ("SCHEMA_MODULE_NOT_FOUND", "Schema Module Not Found"),
    ("SCHEMA_ENTITY_NOT_FOUND", "Schema Entity Not Found"),
    // -- platform-audit（REQ-014，5 个）--------------------------------------
    (
        "AUDIT_SENSITIVE_PAYLOAD_REJECTED",
        "Audit Payload Contains Sensitive Keys",
    ),
    ("AUDIT_CHAIN_BROKEN", "Audit Chain Integrity Check Failed"),
    ("AUDIT_EVENT_NOT_FOUND", "Audit Event Not Found"),
    ("AUDIT_INVALID_FILTER", "Invalid Audit Query"),
    ("AUDIT_SINK_UNAVAILABLE", "Audit Sink Unavailable"),
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
    // -- platform-iam 的 SSO 入站（REQ-013，21 个）--------------------------
    ("SSO_REQUIRED", "SSO Login Required"),
    ("SSO_PROVIDER_NOT_CONFIGURED", "SSO Provider Not Configured"),
    ("SSO_CONFIG_INVALID", "SSO Configuration Invalid"),
    (
        "SSO_OIDC_DISCOVERY_FAILED",
        "SSO Discovery Endpoint Unreachable",
    ),
    (
        "SSO_OIDC_DISCOVERY_INVALID",
        "SSO Discovery Document Invalid",
    ),
    ("SSO_OIDC_STATE_INVALID", "SSO State Invalid"),
    ("SSO_OIDC_STATE_EXPIRED", "SSO State Expired"),
    (
        "SSO_OIDC_CALLBACK_PARAM_MISSING",
        "SSO Callback Parameter Missing",
    ),
    (
        "SSO_OIDC_TOKEN_EXCHANGE_FAILED",
        "SSO Token Exchange Failed",
    ),
    (
        "SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE",
        "SSO Token Endpoint Unavailable",
    ),
    ("SSO_OIDC_TOKEN_INVALID", "SSO ID Token Invalid"),
    ("SSO_OIDC_TOKEN_EXPIRED", "SSO ID Token Expired"),
    (
        "SSO_OIDC_ALG_NOT_ALLOWED",
        "SSO ID Token Algorithm Not Allowed",
    ),
    ("SSO_OIDC_NONCE_MISMATCH", "SSO Nonce Mismatch"),
    ("SSO_OIDC_PKCE_MISMATCH", "SSO PKCE Verification Failed"),
    ("SSO_LDAP_UNREACHABLE", "LDAP Server Unreachable"),
    ("SSO_LDAP_BIND_FAILED", "LDAP Authentication Failed"),
    ("SSO_LDAP_EMPTY_PASSWORD", "LDAP Password Must Not Be Empty"),
    (
        "SSO_IDENTITY_LINK_CONFLICT",
        "SSO Identity Conflicts With An Existing Account",
    ),
    ("SSO_ACCOUNT_NOT_LINKED", "SSO Account Not Linked"),
    ("SSO_TICKET_INVALID", "SSO Login Ticket Invalid"),
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
    // -- platform-oauth（REQ-022，9 个）-------------------------------------
    // ⚠️ **只有** 9 个：8 个 `oauth.*` 审计**事件码不进本表**（D223）——
    //    事件码不进响应体 → 没有「面向用户的标题」这回事。
    ("OAUTH_CLIENT_NOT_FOUND", "OAuth Client Not Found"),
    ("OAUTH_CLIENT_DISABLED", "OAuth Client Disabled"),
    ("OAUTH_CLIENT_EXPIRED", "OAuth Client Expired"),
    ("OAUTH_INVALID_REQUEST", "Invalid OAuth Request"),
    ("OAUTH_INVALID_REDIRECT_URI", "Invalid Redirect URI"),
    ("OAUTH_INVALID_SCOPE", "Invalid OAuth Scope"),
    (
        "OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD",
        "Unsupported Code Challenge Method",
    ),
    ("OAUTH_TICKET_INVALID", "OAuth Consent Ticket Invalid"),
    ("OAUTH_GRANT_NOT_FOUND", "OAuth Grant Not Found"),
    // -- platform-app（REQ-016，6 个）--------------------------------------
    // ⚠️ **只有** 6 个：3 个 `app.*` 审计**事件码不进本表**（D223）——
    //    事件码不进响应体 → 没有「面向用户的标题」这回事。
    // ⚠️ 6 个码**逐个**都有 AC 会真的触发它（无死码，D226 同款口径）：
    //    NOT_FOUND=AC-S-156 步6 / DUPLICATED=路径2 / CLIENT_INVALID=AC-S-158 路1+2 /
    //    LAUNCH_URL_INVALID=路径3 / ICON_URL_INVALID=路径4 / INVALID_REQUEST=路径5。
    ("APP_NOT_FOUND", "App Not Found"),
    ("APP_DUPLICATED", "App Duplicated"),
    ("APP_CLIENT_INVALID", "App OAuth Client Invalid"),
    ("APP_LAUNCH_URL_INVALID", "App Launch URL Invalid"),
    ("APP_ICON_URL_INVALID", "App Icon URL Invalid"),
    ("APP_INVALID_REQUEST", "Invalid App Request"),
    // -- platform-notify（REQ-015，8 个错误码）-------------------------------
    // ⚠️ 8 个码**逐个**都有 AC 会真的触发它（无死码，D303 / D226 同款口径）：
    //    TEMPLATE_NOT_FOUND=AC-S-167 步10 / TEMPLATE_DUPLICATED=步9 /
    //    TEMPLATE_MISSING_VARIABLE=步7 / TEMPLATE_INVALID_PLACEHOLDER=步6+11 /
    //    CHANNEL_NOT_CONFIGURED=AC-S-166 步3+4 / CATEGORY_MANDATORY=AC-S-169 步5 /
    //    NOT_DEAD_LETTER=AC-S-172 步2+3 / SENSITIVE_PAYLOAD=AC-S-173 步4。
    // ⚠️ 4 个 `notify.*` 审计**事件码不进本表**（D223）—— 事件码不是 HTTP 错误码。
    //    与 `org.*` 的 6 行**不冲突**：那 6 个是「通知标题的文案来源」，
    //    由 `OrgEventKind::title()` 显式查表；`notify.*` 是审计的 `action`，没人查标题。
    (
        "NOTIFY_TEMPLATE_NOT_FOUND",
        "Notification Template Not Found",
    ),
    (
        "NOTIFY_TEMPLATE_DUPLICATED",
        "Notification Template Duplicated",
    ),
    (
        "NOTIFY_TEMPLATE_MISSING_VARIABLE",
        "Notification Template Missing Variable",
    ),
    (
        "NOTIFY_TEMPLATE_INVALID_PLACEHOLDER",
        "Notification Template Invalid Placeholder",
    ),
    (
        "NOTIFY_CHANNEL_NOT_CONFIGURED",
        "Notification Channel Not Configured",
    ),
    (
        "NOTIFY_CATEGORY_MANDATORY",
        "Notification Category Mandatory",
    ),
    (
        "NOTIFY_NOT_DEAD_LETTER",
        "Notification Delivery Not Dead Letter",
    ),
    (
        "NOTIFY_SENSITIVE_PAYLOAD",
        "Notification Payload Contains Sensitive Keys",
    ),
    // -- platform-storage 的错误码（REQ-018）--------------------------------
    // 错误码（不是事件码）：每个都是 `platform-storage` 可能返回给客户端的
    // HTTP 失败形态 —— 前端按标题展示，因此文案是给**终端用户**看的。
    (
        "STORAGE_OBJECT_NOT_FOUND",
        "Storage Object Not Found",
    ),
    (
        "STORAGE_PRESIGN_EXPIRED",
        "Storage Presigned Link Expired",
    ),
    (
        "STORAGE_PRESIGN_INVALID",
        "Storage Presigned Link Invalid",
    ),
    (
        "STORAGE_PART_MISSING",
        "Storage Upload Part Missing",
    ),
    (
        "STORAGE_CHECKSUM_MISMATCH",
        "Storage Checksum Mismatch",
    ),
    (
        "STORAGE_UPLOAD_TOO_LARGE",
        "Storage Upload Too Large",
    ),
    (
        "STORAGE_UNSUPPORTED_FORMAT",
        "Storage Unsupported Format",
    ),
    (
        "STORAGE_FILE_INFECTED",
        "Storage File Infected",
    ),
    (
        "STORAGE_TTL_OUT_OF_RANGE",
        "Storage Ttl Out Of Range",
    ),
    (
        "STORAGE_UNKNOWN_ENTITY",
        "Storage Unknown Entity",
    ),
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
