//! 审计**事件码**（REQ-016，D215 / D223 同款口径）。
//!
//! # ⚠️ 事件码**不进** `TITLES`，也**不进** i18n 的 `error.*`（D223）
//!
//! 事件码**不进响应体** —— 它们只出现在审计表里 → 没有「面向用户的标题」这回事。
//!
//! 进了 `TITLES` 会让 `platform-server/tests/i18n_titles.rs` 的三个既有断言必红：
//!
//! 1. `codes_follow_their_namespace_convention` 只允许 `UPPER_SNAKE` 或 `org.` 前缀；
//! 2. `every_error_code_is_registered_in_every_locale` 只排除来源标签
//!    `platform-org::notify` → 任何**新增来源**都会被算成「错误码」→ 要求 i18n 有
//!    `error.app.*`；
//! 3. `no_orphan_titles` 要求每个 `TITLES` 键都被某条 `ALL_CODES` 认领。
//!
//! → 事件码改为**本 crate 的内部常量**，由**本模块的单测**覆盖其取值与用法。
//!
//! # 命名法（**三段式**，D163）
//!
//! `<域>.<对象>.<动作>` —— 与已交付的 `oauth.client.registered` 同款。
//!
//! ⚠️ 领域是被 QA 挑出的 S-12：`acceptance.md` rev 4 的正文先写「D215 同款命名规范：
//! **三段式** `app.*`」，紧接着列的却是**两段式** `app.registered` / `app.updated` /
//! `app.deleted` —— **自相矛盾**。
//! 已交付实现在 `crates/platform-oauth/src/events.rs` 用的是三段式
//! （`oauth.<对象>.<动作>`，`object_type` 是**表名** `oauth_client`）。
//! → 本 crate 跟随**已交付的约定 + 用例集**（`test-cases/05-audit-permission.md`
//! 的 TC-016-053 / TC-016-054 逐字要求 `app.registry.registered` 与
//! `object_type == "platform_app"`）。已在交付报告中上报 BA 复核。

/// 审计事件码（`app.*`）。
pub mod code {
    /// 注册应用。
    pub const REGISTERED: &str = "app.registry.registered";
    /// 修改应用。
    pub const UPDATED: &str = "app.registry.updated";
    /// 删除应用。
    pub const DELETED: &str = "app.registry.deleted";
}

/// 全部事件码（**完备性清单**）。
///
/// ⚠️ 这 3 个**不在** `crate::error::ALL_CODES` 里 —— 事件码不是 HTTP 错误码。
pub const ALL_CODES: [&str; 3] = [code::REGISTERED, code::UPDATED, code::DELETED];

/// 审计事件的**对象类型**（`object_type` 列）。
///
/// ⚠️ 用**表名**（与已交付的 `oauth_client` / `oauth_grant` 同款）——
/// `acceptance.md` rev 4 写的是 `"app"`，那与已交付口径冲突（QA 的 S-12）；
/// 用例集 TC-016-054 要求 **恰好** `platform_app`。
pub mod object_type {
    /// 应用注册表行。
    pub const APP: &str = "platform_app";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_event_code_list_has_exactly_three_entries() {
        assert_eq!(ALL_CODES.len(), 3);
    }

    /// 事件码**不重复**。
    #[test]
    fn the_event_codes_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for code in ALL_CODES {
            assert!(seen.insert(code), "事件码重复：{code}");
        }
    }

    /// 命名法：`<域>.<对象>.<动作>` —— 恰好两个 `.`，域固定是 `app`。
    #[test]
    fn every_event_code_follows_the_three_part_naming() {
        for code in ALL_CODES {
            let parts: Vec<&str> = code.split('.').collect();
            assert_eq!(parts.len(), 3, "事件码必须是三段：{code}");
            assert_eq!(
                code.matches('.').count(),
                2,
                "事件码必须正好含 2 个 `.`：{code}"
            );
            assert_eq!(parts[0], "app", "事件码的域必须是 app：{code}");
            for part in parts {
                assert!(
                    part.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                    "事件码的每一段只能是小写字母或下划线：{code}"
                );
                assert!(!part.is_empty(), "事件码不得有空段：{code}");
            }
        }
    }

    /// ⚠️ 事件码**不得**出现在 `TITLES` 里（D223）。
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

    /// AC-S-164 要求**恰好**这 3 个事件码（注册 / 修改 / 删除）。
    #[test]
    fn the_three_mandatory_event_codes_are_registered() {
        for code in [code::REGISTERED, code::UPDATED, code::DELETED] {
            assert!(ALL_CODES.contains(&code), "缺少必需的事件码：{code}");
        }
    }

    /// `object_type` 必须是**表名**（与 `oauth_client` 同款口径）。
    #[test]
    fn the_object_type_is_the_table_name() {
        assert_eq!(object_type::APP, "platform_app");
    }
}
