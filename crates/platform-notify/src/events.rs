//! 通知系统的**审计事件码**（REQ-015，D223 同款口径）。
//!
//! # ⚠️ 事件码**不进** `TITLES`，**也不进** [`crate::error::ALL_CODES`]
//!
//! 「审计事件码不是 HTTP 错误码」—— 与 REQ-022 的 8 个 `oauth.*`、
//! REQ-016 的 3 个 `app.*` 同一口径（D223）。
//!
//! ⚠️ **看起来矛盾的先例**：`platform_core::titles::TITLES` 里**确实有** 6 个
//! `org.*` 事件码（`titles.rs:195-200`，注释写「platform-org 的通知标题」）。
//! 那**不是**反例：
//!
//! | | `org.*`（在 `TITLES` 里） | `notify.*`（不在） |
//! |---|---|---|
//! | 用途 | **通知标题的文案来源** | **审计记录的 `action` 字段** |
//! | 谁查 | `OrgEventKind::title()` 显式调 `title_of_code()` | 没人查 |
//! | 前端消费 | 是（按事件码渲染中文文案） | 否 |
//!
//! → 本模块的 4 个码若被塞进 `TITLES`，
//! `platform-server/tests/i18n_titles.rs` 的「钉死行数」断言会红
//! （`TITLES.len()` 会变成 111 而不是 AC-S-174 步骤 1c 要求的 **107**）。
//!
//! # 命名：三段式 `notify.<对象>.<动作>`
//!
//! 与已交付的 `app.registry.*` / `oauth.client.*` 一致（D215 同款）。

/// 建模板。
pub const TEMPLATE_CREATED: &str = "notify.template.created";
/// 改模板。
pub const TEMPLATE_UPDATED: &str = "notify.template.updated";
/// 删模板。
pub const TEMPLATE_DELETED: &str = "notify.template.deleted";
/// 死信人工重投（**必须**留痕：谁在什么时候把哪条重新投了）。
pub const DEAD_LETTER_RESENT: &str = "notify.dead_letter.resent";

/// 本 crate 的全部审计事件码（**完备性测试的输入**）。
///
/// ⚠️ 这 4 个码**不得**出现在 `platform_core::titles::TITLES` 里（D223）。
pub const ALL_CODES: &[&str] = &[
    TEMPLATE_CREATED,
    TEMPLATE_UPDATED,
    TEMPLATE_DELETED,
    DEAD_LETTER_RESENT,
];

/// 事件对应的**审计对象类型**（契约 C12 的 `object_type`，用**表名语义**）。
///
/// ⚠️ 与已交付的 `oauth_client` / `platform_app` 同款：用表名，不用模块名。
pub const OBJECT_TYPE_TEMPLATE: &str = "platform_notify_template";
/// 投递记录的表名语义。
pub const OBJECT_TYPE_DELIVERY: &str = "platform_notify_delivery";

#[cfg(test)]
mod tests {
    use super::*;

    /// 恰好 4 条且无重复。
    #[test]
    fn the_event_code_list_has_exactly_four_distinct_codes() {
        assert_eq!(ALL_CODES.len(), 4, "AC-S-174 步骤 5 要求恰好 4 个事件码");
        let unique: std::collections::BTreeSet<&str> = ALL_CODES.iter().copied().collect();
        assert_eq!(unique.len(), 4, "事件码不得重复");
    }

    /// 全部是**三段式** `notify.<对象>.<动作>`（D215 同款命名规范）。
    #[test]
    fn every_event_code_is_three_segments() {
        for code in ALL_CODES {
            let segments: Vec<&str> = code.split('.').collect();
            assert_eq!(
                segments.len(),
                3,
                "事件码必须是三段式（notify.<对象>.<动作>）：{code} → {segments:?}"
            );
            assert_eq!(segments[0], "notify", "第一段必须是 notify：{code}");
        }
    }

    /// 全部是**纯 ASCII 小写 + 点 + 连字符 + 下划线**（审计 `action` 字段的约束）。
    ///
    /// ⚠️ **允许下划线**：`notify.dead_letter.resent` 里的 `dead_letter`
    /// 就是状态名（`model::status::DEAD_LETTER`）—— 换行成 `deadletter` 会与
    /// 代码里的常量名对不上，反而更难追溯。
    ///
    /// 这是**允许**而非**必需**：已交付的 `app.registry.*` / `oauth.client.*`
    /// 都是纯字母段，那只是巧合（它们的词本身不需要下划线）。
    /// 审计表的 `action` 列是 `TEXT`、**没有**字符集约束
    /// （`migrations/20261120000200_audit.up.sql:49`）。
    #[test]
    fn every_event_code_is_ascii_lower_snake() {
        for code in ALL_CODES {
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase()
                    || c == '.'
                    || c == '-'
                    || c == '_'
                    || c.is_ascii_digit()),
                "事件码必须是 ASCII 小写（点/连字符/下划线分隔）：{code}"
            );
            assert!(
                !code.contains(|c: char| c.is_whitespace()),
                "事件码不得含空白：{code}"
            );
        }
    }

    /// 事件码里**下划线只能出现在段内**（不能以 `_` 开头/结尾，不能 `..`）。
    #[test]
    fn underscores_only_appear_inside_segments() {
        for code in ALL_CODES {
            for segment in code.split('.') {
                assert!(!segment.is_empty(), "事件码 {code} 有空段");
                assert!(
                    !segment.starts_with('_') && !segment.ends_with('_'),
                    "段 {segment} 不得以 _ 开头或结尾（{code}）"
                );
            }
        }
    }

    /// 事件码**不得**与任何 `NOTIFY_*` 错误码相同（两套命名空间不能混）。
    #[test]
    fn event_codes_are_disjoint_from_error_codes() {
        for event in ALL_CODES {
            assert!(
                !crate::error::ALL_CODES.contains(event),
                "事件码不得出现在错误码清单里：{event}"
            );
        }
    }

    /// `object_type` 用**表名语义**（与 `oauth_client` / `platform_app` 同款）。
    #[test]
    fn object_types_are_table_names() {
        assert_eq!(OBJECT_TYPE_TEMPLATE, "platform_notify_template");
        assert_eq!(OBJECT_TYPE_DELIVERY, "platform_notify_delivery");
        for name in [OBJECT_TYPE_TEMPLATE, OBJECT_TYPE_DELIVERY] {
            assert!(
                name.starts_with("platform_notify_"),
                "object_type 必须是本 crate 的表名：{name}"
            );
        }
    }
}
