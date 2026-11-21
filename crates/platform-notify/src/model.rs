//! 数据模型：**3 张表的行类型** + 跨栈 DTO（REQ-015，design §2 / §3.1）。
//!
//! # 字段名（契约 C13）
//!
//! 所有**响应体 / 请求体**字段一律 **camelCase**（`templateId` / `templateCode` /
//! `titleTemplate` / `attemptCount` / `nextAttemptAt` / `renderedTitle` …）。
//!
//! # 时间格式（D142）
//!
//! 响应里的时间**一律** UTC、RFC3339、**微秒 6 位**
//! （`platform_core::extension::audit::format_timestamp_micros_utc` ——
//! 与审计记录同一个出口，不另写一份格式化）。
//!
//! # ⚠️ 投递表**恰好 18 列**（D299，AC-S-173 步骤 7 逐列钉死）
//!
//! 判据必须写成「与一个**显式列出的** 18 元素集合相等」，
//! **不许**写成「列数 = 18」（列数对但列错，那种断言会放过改名列）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// 模板表（**租户级**；RLS 由 `cargo xtask tenant-policies` 生成）。
pub const TEMPLATE_TABLE: &str = "platform_notify_template";
/// 订阅偏好表（**租户级**）。
pub const PREFERENCE_TABLE: &str = "platform_notify_preference";
/// 投递记录表（**租户级**）。
pub const DELIVERY_TABLE: &str = "platform_notify_delivery";

/// 投递记录的**状态**（D298 的状态机）。
///
/// ```text
/// pending ──tick──▶ delivering ──成功──▶ delivered
///                       │
///                       ├──失败且还有次数──▶ pending（next_attempt_at = now + backoff）
///                       └──失败且次数用尽──▶ dead_letter
/// dead_letter ──人工重投──▶ pending（attempt_count = 0）
/// ```
pub mod status {
    /// 待投递（`next_attempt_at` 到期才会被 worker 取）。
    pub const PENDING: &str = "pending";
    /// 投递中（**崩溃残留状态**：超过 `notify.deliveringTimeoutSeconds` 会被回收）。
    pub const DELIVERING: &str = "delivering";
    /// 投递成功（**终态**，不参与出队）。
    pub const DELIVERED: &str = "delivered";
    /// 死信（次数用尽 / 未配置通道 / 用户已退订；**只能**人工重投）。
    pub const DEAD_LETTER: &str = "dead_letter";

    /// 全部状态取值（列约束与测试共用）。
    pub const ALL: &[&str] = &[PENDING, DELIVERING, DELIVERED, DEAD_LETTER];
}

/// 通知**分类**（D295：退订按分类，不按模板）。
pub mod category {
    /// 强制通知（密码过期 / 账号被锁 / 安全告警）—— **不可退订**。
    pub const SYSTEM: &str = "system";
    /// 营销类（可退订）。
    pub const MARKETING: &str = "marketing";
    /// 事务类（订单 / 审批结果等，可退订）。
    pub const TRANSACTIONAL: &str = "transactional";

    /// 全部分类取值（列约束与测试共用）。
    pub const ALL: &[&str] = &[SYSTEM, MARKETING, TRANSACTIONAL];

    /// 是否**可退订**（`system` 不可退订 —— D295 / AC-S-169 步骤 5）。
    pub fn is_optional(value: &str) -> bool {
        !matches!(value, SYSTEM)
    }
}

/// 通道名（D292：3 个真实现 + 4 个显式未配置）。
pub mod channel {
    /// 站内信（落库，无外部依赖）。
    pub const IN_APP: &str = "in_app";
    /// 邮件（**自己写的 SMTP 客户端**，D305）。
    pub const SMTP: &str = "smtp";
    /// Webhook（`hyper` + `hyper-rustls`）。
    pub const WEBHOOK: &str = "webhook";
    /// 短信（**显式未配置**）。
    pub const SMS: &str = "sms";
    /// 钉钉（**显式未配置**）。
    pub const DINGTALK: &str = "dingtalk";
    /// 企业微信（**显式未配置**）。
    pub const WECOM: &str = "wecom";
    /// 飞书（**显式未配置**）。
    pub const FEISHU: &str = "feishu";

    /// **有真实现**的通道（3 个）。
    pub const IMPLEMENTED: &[&str] = &[IN_APP, SMTP, WEBHOOK];
    /// **显式未配置**的通道（4 个）。
    ///
    /// ⚠️ 它们**仍在注册表里**（必须存在以返回明确错误），
    /// 但 `send()` **必须返回 `Err`**（D292 的铁律）。
    pub const UNCONFIGURED: &[&str] = &[SMS, DINGTALK, WECOM, FEISHU];
    /// 全部通道（AC-S-166 步骤 2 要求的**恰好 7 个**）。
    pub const ALL: &[&str] = &[IN_APP, SMTP, WEBHOOK, SMS, DINGTALK, WECOM, FEISHU];
}

// ---------------------------------------------------------------------------
// 行类型
// ---------------------------------------------------------------------------

/// `platform_notify_template` 的一行。
///
/// 唯一约束 `UNIQUE (tenant_id, code, channel, locale)`（D294：多语言按行存）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRow {
    /// 模板 ID（平台生成）。
    pub id: String,
    /// 租户 ID。
    pub tenant_id: String,
    /// 模板编码（`^[a-z][a-z0-9_.]*$`）。
    pub code: String,
    /// 通道名。
    pub channel: String,
    /// BCP-47 语言标签（`zh-CN` / `en-US`）。
    pub locale: String,
    /// 通知分类（`system` / `marketing` / `transactional`）。
    pub category: String,
    /// 标题模板（`{{var}}` 语法）。
    pub title_template: String,
    /// 正文模板。
    pub body_template: String,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 更新时间。
    pub updated_at: OffsetDateTime,
}

/// `platform_notify_preference` 的一行。
///
/// 主键 `(tenant_id, user_id, category)`（D295）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferenceRow {
    /// 租户 ID。
    pub tenant_id: String,
    /// 用户 ID。
    pub user_id: String,
    /// 通知分类。
    pub category: String,
    /// 是否已退订。
    pub opted_out: bool,
    /// 更新时间。
    pub updated_at: OffsetDateTime,
}

/// `platform_notify_delivery` 的一行（**恰好 18 列**，D299）。
///
/// ⚠️ `rendered_title` / `rendered_body` 是**快照**：
/// 渲染一次落库，模板后续修改**不影响**已投递记录（AC-S-173 步骤 2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryRow {
    /// 投递 ID。
    pub id: String,
    /// 租户 ID。
    pub tenant_id: String,
    /// 来源模板的 `code`（**不是**外键 —— 模板删除后记录仍可追溯）。
    pub template_code: String,
    /// 通道名。
    pub channel: String,
    /// 通知分类。
    pub category: String,
    /// 接收人（站内信是 `user_id`；webhook 是 URL；邮件是邮箱地址）。
    pub recipient: String,
    /// 渲染用的语言。
    pub locale: String,
    /// 渲染变量（`jsonb`）。
    pub payload: serde_json::Value,
    /// 渲染结果标题（**快照**）。
    pub rendered_title: String,
    /// 渲染结果正文（**快照**）。
    pub rendered_body: String,
    /// 状态（见 [`status`]）。
    pub status: String,
    /// 已尝试次数（含首次）。
    pub attempt_count: i32,
    /// 下次尝试时间。
    pub next_attempt_at: OffsetDateTime,
    /// 最后一次失败的原文（成功时为 `None`）。
    pub last_error: Option<String>,
    /// 通道返回的消息 ID（成功时由 provider 给出）。
    pub provider_message_id: Option<String>,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 更新时间。
    pub updated_at: OffsetDateTime,
    /// 投递成功时间（未成功时 `None`）。
    pub delivered_at: Option<OffsetDateTime>,
}

/// `platform_notify_delivery` 的**列清单**（**恰好 18 列**，AC-S-173 步骤 7）。
///
/// ⚠️ 判据要用这个常量做**双向相等**断言，**不许**写「列数 = 18」。
pub const DELIVERY_COLUMNS: &[&str] = &[
    "id",
    "tenant_id",
    "template_code",
    "channel",
    "category",
    "recipient",
    "locale",
    "payload",
    "rendered_title",
    "rendered_body",
    "status",
    "attempt_count",
    "next_attempt_at",
    "last_error",
    "provider_message_id",
    "created_at",
    "updated_at",
    "delivered_at",
];

// ---------------------------------------------------------------------------
// 跨栈 DTO（camelCase）
// ---------------------------------------------------------------------------

/// 模板的响应体（camelCase，契约 C13）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateView {
    /// 模板 ID。
    pub template_id: String,
    /// 模板编码。
    pub template_code: String,
    /// 通道名。
    pub channel: String,
    /// 语言标签。
    pub locale: String,
    /// 通知分类。
    pub category: String,
    /// 标题模板。
    pub title_template: String,
    /// 正文模板。
    pub body_template: String,
    /// 创建时间（RFC3339 微秒 6 位，D142）。
    pub created_at: String,
    /// 更新时间。
    pub updated_at: String,
}

/// 投递记录的响应体（camelCase）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryView {
    /// 投递 ID。
    pub delivery_id: String,
    /// 模板编码。
    pub template_code: String,
    /// 通道名。
    pub channel: String,
    /// 通知分类。
    pub category: String,
    /// 接收人。
    pub recipient: String,
    /// 语言。
    pub locale: String,
    /// 渲染结果标题（**快照**）。
    pub rendered_title: String,
    /// 渲染结果正文（**快照**）。
    pub rendered_body: String,
    /// 状态。
    pub status: String,
    /// 已尝试次数。
    pub attempt_count: i32,
    /// 下次尝试时间。
    pub next_attempt_at: String,
    /// 最后一次失败原因。
    pub last_error: Option<String>,
    /// 通道返回的消息 ID。
    pub provider_message_id: Option<String>,
    /// 创建时间。
    pub created_at: String,
    /// 更新时间。
    pub updated_at: String,
    /// 投递成功时间。
    pub delivered_at: Option<String>,
}

/// 订阅偏好的响应体（camelCase）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceView {
    /// 通知分类。
    pub category: String,
    /// 是否已退订。
    pub opted_out: bool,
    /// 更新时间。
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// `FromRow` 实现（用平台声明的宏，**不直接依赖 sqlx**）
// ---------------------------------------------------------------------------
//
// 低配机的冷编译对 proc-macro 编译链敏感（sqlx 的 `macros` feature 会拉进
// syn / quote / proc-macro2），所以平台用声明宏手写 —— 见
// `crates/platform-db/src/lib.rs:100` 的 `tenant_row!`。

platform_db::tenant_row!(TemplateRow {
    id: "id",
    tenant_id: "tenant_id",
    code: "code",
    channel: "channel",
    locale: "locale",
    category: "category",
    title_template: "title_template",
    body_template: "body_template",
    created_at: "created_at",
    updated_at: "updated_at",
});

platform_db::tenant_row!(PreferenceRow {
    tenant_id: "tenant_id",
    user_id: "user_id",
    category: "category",
    opted_out: "opted_out",
    updated_at: "updated_at",
});

platform_db::tenant_row!(DeliveryRow {
    id: "id",
    tenant_id: "tenant_id",
    template_code: "template_code",
    channel: "channel",
    category: "category",
    recipient: "recipient",
    locale: "locale",
    payload: "payload",
    rendered_title: "rendered_title",
    rendered_body: "rendered_body",
    status: "status",
    attempt_count: "attempt_count",
    next_attempt_at: "next_attempt_at",
    last_error: "last_error",
    provider_message_id: "provider_message_id",
    created_at: "created_at",
    updated_at: "updated_at",
    delivered_at: "delivered_at",
});

// ---------------------------------------------------------------------------
// 请求体 / 预览（camelCase）
// ---------------------------------------------------------------------------

/// 建 / 改模板的请求体。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateUpsertBody {
    /// 模板编码（`^[a-z][a-z0-9_.]*$`）。
    #[serde(default)]
    pub template_code: String,
    /// 通道名。
    #[serde(default)]
    pub channel: String,
    /// 语言标签。
    #[serde(default)]
    pub locale: String,
    /// 通知分类。
    #[serde(default)]
    pub category: String,
    /// 标题模板。
    #[serde(default)]
    pub title_template: String,
    /// 正文模板。
    #[serde(default)]
    pub body_template: String,
}

impl TemplateUpsertBody {
    /// 转成待写入的模板（ID 由平台生成）。
    pub fn into_new(&self) -> crate::store::NewTemplate {
        crate::store::NewTemplate {
            id: uuid::Uuid::new_v4().to_string(),
            code: self.template_code.clone(),
            channel: self.channel.clone(),
            locale: self.locale.clone(),
            category: self.category.clone(),
            title_template: self.title_template.clone(),
            body_template: self.body_template.clone(),
        }
    }
}

/// 预览请求体。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBody {
    /// 渲染变量。
    #[serde(default)]
    pub payload: BTreeMap<String, String>,
}

/// 预览响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResponse {
    /// 渲染后的标题。
    pub title: String,
    /// 渲染后的正文。
    pub body: String,
}

/// 设退订状态的请求体。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPreferenceBody {
    /// 通知分类。
    pub category: String,
    /// 是否退订。
    pub opted_out: bool,
}

/// 模板行 → 响应体。
pub fn view_of_template(row: &TemplateRow) -> TemplateView {
    TemplateView {
        template_id: row.id.clone(),
        template_code: row.code.clone(),
        channel: row.channel.clone(),
        locale: row.locale.clone(),
        category: row.category.clone(),
        title_template: row.title_template.clone(),
        body_template: row.body_template.clone(),
        created_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.created_at),
        updated_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.updated_at),
    }
}

/// 投递行 → 响应体。
pub fn view_of_delivery(row: &DeliveryRow) -> DeliveryView {
    DeliveryView {
        delivery_id: row.id.clone(),
        template_code: row.template_code.clone(),
        channel: row.channel.clone(),
        category: row.category.clone(),
        recipient: row.recipient.clone(),
        locale: row.locale.clone(),
        rendered_title: row.rendered_title.clone(),
        rendered_body: row.rendered_body.clone(),
        status: row.status.clone(),
        attempt_count: row.attempt_count,
        next_attempt_at: platform_core::extension::audit::format_timestamp_micros_utc(
            &row.next_attempt_at,
        ),
        last_error: row.last_error.clone(),
        provider_message_id: row.provider_message_id.clone(),
        created_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.created_at),
        updated_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.updated_at),
        delivered_at: row
            .delivered_at
            .as_ref()
            .map(platform_core::extension::audit::format_timestamp_micros_utc),
    }
}

/// 偏好行 → 响应体。
pub fn view_of_preference(row: &PreferenceRow) -> PreferenceView {
    PreferenceView {
        category: row.category.clone(),
        opted_out: row.opted_out,
        updated_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.updated_at),
    }
}

/// 让 `TemplateUpdate` 可从 JSON 反序列化（`PATCH` 用，字段全可选）。
pub fn template_update_from_body(body: &TemplateUpsertBody) -> crate::store::TemplateUpdate {
    crate::store::TemplateUpdate {
        title_template: if body.title_template.is_empty() {
            None
        } else {
            Some(body.title_template.clone())
        },
        body_template: if body.body_template.is_empty() {
            None
        } else {
            Some(body.body_template.clone())
        },
        category: if body.category.is_empty() {
            None
        } else {
            Some(body.category.clone())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 通道集合：**恰好 7 个**，且真实现 3 + 未配置 4（AC-S-166 步骤 2）。
    #[test]
    fn the_channel_sets_add_up_to_seven() {
        assert_eq!(channel::ALL.len(), 7, "AC-S-166 步骤 2 要求恰好 7 个通道");
        assert_eq!(channel::IMPLEMENTED.len(), 3, "3 个真实现");
        assert_eq!(channel::UNCONFIGURED.len(), 4, "4 个显式未配置");
        assert_eq!(
            channel::IMPLEMENTED.len() + channel::UNCONFIGURED.len(),
            channel::ALL.len(),
            "3 + 4 必须等于 7"
        );
    }

    /// 真实现与未配置**互斥**（同一个通道不能两边都在）。
    #[test]
    fn implemented_and_unconfigured_are_disjoint() {
        for ch in channel::IMPLEMENTED {
            assert!(
                !channel::UNCONFIGURED.contains(ch),
                "通道 {ch} 同时出现在真实现与未配置里"
            );
        }
    }

    /// `ALL` 无重复，且每个元素都在 `IMPLEMENTED` 或 `UNCONFIGURED` 里。
    #[test]
    fn every_channel_is_classified_exactly_once() {
        let unique: std::collections::BTreeSet<&str> = channel::ALL.iter().copied().collect();
        assert_eq!(unique.len(), channel::ALL.len(), "ALL 不得有重复");
        for ch in channel::ALL {
            let in_impl = channel::IMPLEMENTED.contains(ch);
            let in_unconf = channel::UNCONFIGURED.contains(ch);
            assert!(
                in_impl ^ in_unconf,
                "通道 {ch} 必须**恰好**归类一次（in_impl={in_impl}, in_unconf={in_unconf}）"
            );
        }
    }

    /// 投递表列清单**恰好 18 列**且无重复（D299）。
    #[test]
    fn the_delivery_column_list_has_exactly_eighteen_distinct_columns() {
        assert_eq!(DELIVERY_COLUMNS.len(), 18, "D299 把基线钉死为 18 列");
        let unique: std::collections::BTreeSet<&str> = DELIVERY_COLUMNS.iter().copied().collect();
        assert_eq!(unique.len(), 18, "列清单不得有重复");
    }

    /// `DeliveryRow` 的字段数必须与列清单一致（结构体漏一个字段 → 这里红）。
    #[test]
    fn the_delivery_row_struct_covers_every_column() {
        // 构造一行，用 `Debug` 输出核对 18 个字段名都出现。
        let row = DeliveryRow {
            id: "d".into(),
            tenant_id: "t".into(),
            template_code: "c".into(),
            channel: channel::IN_APP.into(),
            category: category::SYSTEM.into(),
            recipient: "u".into(),
            locale: "zh-CN".into(),
            payload: serde_json::json!({}),
            rendered_title: "ttl".into(),
            rendered_body: "body".into(),
            status: status::PENDING.into(),
            attempt_count: 0,
            next_attempt_at: OffsetDateTime::UNIX_EPOCH,
            last_error: None,
            provider_message_id: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            delivered_at: None,
        };
        let debug = format!("{row:?}");
        for column in DELIVERY_COLUMNS {
            assert!(
                debug.contains(column),
                "DeliveryRow 缺字段 `{column}`（列清单里有但结构体没有）"
            );
        }
    }

    /// 状态取值**恰好 4 个**（D298 的状态机）。
    #[test]
    fn the_status_set_has_exactly_four_values() {
        assert_eq!(status::ALL.len(), 4);
        let unique: std::collections::BTreeSet<&str> = status::ALL.iter().copied().collect();
        assert_eq!(unique.len(), 4);
    }

    /// `system` 类**不可退订**，其余可退订（D295 / AC-S-169 步骤 5）。
    #[test]
    fn the_system_category_is_mandatory() {
        assert!(
            !category::is_optional(category::SYSTEM),
            "system 类必须不可退订（D295）"
        );
        assert!(
            category::is_optional(category::MARKETING),
            "marketing 类必须可退订"
        );
        assert!(
            category::is_optional(category::TRANSACTIONAL),
            "transactional 类必须可退订"
        );
    }

    /// 分类取值**恰好 3 个**。
    #[test]
    fn the_category_set_has_exactly_three_values() {
        assert_eq!(category::ALL.len(), 3);
    }

    /// DTO 序列化后字段是 **camelCase**（契约 C13）。
    #[test]
    fn the_dtos_serialize_in_camel_case() {
        let view = TemplateView {
            template_id: "t1".into(),
            template_code: "order.shipped".into(),
            channel: channel::SMTP.into(),
            locale: "zh-CN".into(),
            category: category::TRANSACTIONAL.into(),
            title_template: "已发货".into(),
            body_template: "{{name}}，订单 {{order_id}} 已发货".into(),
            created_at: "2026-11-21T00:00:00.000000Z".into(),
            updated_at: "2026-11-21T00:00:00.000000Z".into(),
        };
        let json = serde_json::to_value(&view).expect("序列化");
        assert!(json.get("templateId").is_some(), "必须是 camelCase");
        assert!(json.get("template_id").is_none(), "不得是 snake_case");
        assert!(json.get("titleTemplate").is_some());
    }

    /// 投递 DTO 的 camelCase 字段名逐个核对（design §3.1 的映射表）。
    #[test]
    fn the_delivery_dto_uses_the_documented_field_names() {
        let view = DeliveryView {
            delivery_id: "d1".into(),
            template_code: "c".into(),
            channel: channel::IN_APP.into(),
            category: category::SYSTEM.into(),
            recipient: "u".into(),
            locale: "zh-CN".into(),
            rendered_title: "t".into(),
            rendered_body: "b".into(),
            status: status::PENDING.into(),
            attempt_count: 0,
            next_attempt_at: "2026-11-21T00:00:00.000000Z".into(),
            last_error: None,
            provider_message_id: None,
            created_at: "2026-11-21T00:00:00.000000Z".into(),
            updated_at: "2026-11-21T00:00:00.000000Z".into(),
            delivered_at: None,
        };
        let json = serde_json::to_value(&view).expect("序列化");
        for key in [
            "deliveryId",
            "templateCode",
            "renderedTitle",
            "renderedBody",
            "attemptCount",
            "nextAttemptAt",
            "lastError",
            "providerMessageId",
            "deliveredAt",
        ] {
            assert!(json.get(key).is_some(), "缺 camelCase 字段：{key}");
        }
    }
}
