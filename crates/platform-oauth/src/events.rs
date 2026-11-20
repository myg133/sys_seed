//! 审计**事件码**（REQ-022，D215）。
//!
//! # ⚠️ 事件码**不进** `TITLES`，也**不进** i18n 的 `error.*`（D223）
//!
//! 事件码**不进响应体** —— 它们只出现在审计表里 → 没有「面向用户的标题」这回事。
//!
//! D215 原本写「事件码走 `platform-core/src/titles.rs` 的 `TITLES` 末尾那一段」，
//! 但那样会让 `platform-server/tests/i18n_titles.rs` 的三个既有断言**必红**：
//!
//! 1. `codes_follow_their_namespace_convention` 只允许 `UPPER_SNAKE` 或 `org.` 前缀；
//! 2. `every_error_code_is_registered_in_every_locale` 只排除来源标签
//!    `platform-org::notify` → 任何**新增来源**都会被算成「错误码」→ 要求 i18n 有
//!    `error.oauth.*`；
//! 3. `no_orphan_titles` 要求每个 `TITLES` 键都被某条 `ALL_CODES` 认领。
//!
//! → 事件码改为**本 crate 的内部常量**，由**本模块的单测**覆盖其取值与用法。
//!
//! # 命名法
//!
//! `<域>.<对象>.<动作>`（D163），与 `org.*` 同款。

/// 审计事件码（`oauth.*`）。
pub mod code {
    /// 注册 OAuth 客户端。
    pub const CLIENT_REGISTERED: &str = "oauth.client.registered";
    /// 修改 OAuth 客户端。
    pub const CLIENT_UPDATED: &str = "oauth.client.updated";
    /// 删除 OAuth 客户端。
    pub const CLIENT_DELETED: &str = "oauth.client.deleted";
    /// 用户**同意**授权。
    pub const AUTHORIZATION_APPROVED: &str = "oauth.authorization.approved";
    /// 用户**拒绝**授权。
    pub const AUTHORIZATION_DENIED: &str = "oauth.authorization.denied";
    /// 签发令牌。
    pub const TOKEN_ISSUED: &str = "oauth.token.issued";
    /// 撤销令牌 / 授权记录。
    pub const TOKEN_REVOKED: &str = "oauth.token.revoked";
    /// 单点登出。
    pub const LOGOUT_EXECUTED: &str = "oauth.logout.executed";
}

/// 全部事件码（**完备性清单**）。
///
/// ⚠️ 这 8 个**不在** `crate::error::ALL_CODES` 里 —— 事件码不是 HTTP 错误码。
pub const ALL_CODES: [&str; 8] = [
    code::CLIENT_REGISTERED,
    code::CLIENT_UPDATED,
    code::CLIENT_DELETED,
    code::AUTHORIZATION_APPROVED,
    code::AUTHORIZATION_DENIED,
    code::TOKEN_ISSUED,
    code::TOKEN_REVOKED,
    code::LOGOUT_EXECUTED,
];

/// 审计事件的**对象类型**（`object_type` 列）。
pub mod object_type {
    /// OAuth 客户端。
    pub const CLIENT: &str = "oauth_client";
    /// 同意记录 / 授权记录。
    pub const GRANT: &str = "oauth_grant";
    /// OAuth 令牌。
    pub const TOKEN: &str = "oauth_token";
    /// 会话（登出）。
    pub const SESSION: &str = "oauth_session";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_event_code_list_has_exactly_eight_entries() {
        assert_eq!(ALL_CODES.len(), 8);
    }

    /// 事件码**不重复**。
    #[test]
    fn the_event_codes_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for code in ALL_CODES {
            assert!(seen.insert(code), "事件码重复：{code}");
        }
    }

    /// 命名法：`<域>.<对象>.<动作>`，域固定是 `oauth`。
    #[test]
    fn every_event_code_follows_the_three_part_naming() {
        for code in ALL_CODES {
            let parts: Vec<&str> = code.split('.').collect();
            assert_eq!(parts.len(), 3, "事件码必须是三段：{code}");
            assert_eq!(parts[0], "oauth", "事件码的域必须是 oauth：{code}");
            for part in parts {
                assert!(
                    part.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                    "事件码的每一段只能是小写字母或下划线：{code}"
                );
                assert!(!part.is_empty(), "事件码不得有空段：{code}");
            }
        }
    }

    /// ⚠️ 事件码**不得**出现在 `TITLES` 里（D223）——
    /// 进了会让 `i18n_titles.rs` 的三个既有断言必红。
    #[test]
    fn event_codes_are_not_registered_as_titles() {
        for code in ALL_CODES {
            assert!(
                !platform_core::titles::is_registered(code),
                "事件码 `{code}` 不得进 TITLES（D223）"
            );
        }
    }

    /// ⚠️ 事件码**不得**出现在平台错误码清单里。
    #[test]
    fn event_codes_are_not_platform_error_codes() {
        for code in ALL_CODES {
            assert!(
                !crate::error::ALL_CODES.contains(&code),
                "事件码 `{code}` 不得进平台错误码清单"
            );
        }
    }

    /// AC-S-154 步骤 1 要求**至少**这 5 条事件码全部出现。
    #[test]
    fn the_five_mandatory_event_codes_are_registered() {
        for code in [
            code::CLIENT_REGISTERED,
            code::AUTHORIZATION_APPROVED,
            code::TOKEN_ISSUED,
            code::TOKEN_REVOKED,
            code::LOGOUT_EXECUTED,
        ] {
            assert!(ALL_CODES.contains(&code), "缺少必需的事件码：{code}");
        }
    }
}
