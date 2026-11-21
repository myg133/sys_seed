//! 编排层（REQ-015，design §4）。
//!
//! # 职责
//!
//! 把「模板 + payload + 收件人」变成一条 `pending` 的投递记录，并做**投递前的全部校验**：
//!
//! ```text
//! 入队（enqueue）
//!   ① locale 三级回退（D294）            → 找不到 → TEMPLATE_NOT_FOUND
//!   ② 偏好检查（D295）                   → 已退订 → 直接 dead_letter（attempt_count = 0）
//!   ③ 渲染（D293）                        → 缺变量 / 非法占位符 → 对应错误
//!   ④ 敏感键扫描（D299，复用 REQ-014）    → 命中 → SENSITIVE_PAYLOAD
//!   ⑤ 落库（快照 rendered_*）
//! ```
//!
//! # ⚠️ 偏好检查必须在**投递尝试之前**（D312）
//!
//! 「用户已退订」是**确定性终态** —— 重试 5 次不会改变结论。
//! 所以退订的条目**直接进死信且 `attempt_count == 0`**（AC-S-169 步骤 3）。
//!
//! # ⚠️ 渲染结果是**快照**（D299）
//!
//! `rendered_title` / `rendered_body` 落库后，模板再改**不影响**已入队的记录
//! （AC-S-173 步骤 2）。

use std::collections::BTreeMap;

use platform_core::RequestContext;

use crate::config::NotifyConfig;
use crate::error::NotifyError;
use crate::model::{DeliveryRow, PreferenceRow, TemplateRow, category, status};
use crate::store::{DeliveryOutcome, NewDelivery, NewTemplate, NotifyStore, TemplateUpdate};
use crate::template;

/// 入队请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnqueueRequest {
    /// 模板编码。
    pub template_code: String,
    /// 通道名。
    pub channel: String,
    /// 接收人（站内信是 user_id；邮件是邮箱；webhook 是 URL）。
    pub recipient: String,
    /// 接收人的语言偏好（三级回退的**第 1 级**）。
    pub locale: Option<String>,
    /// 渲染变量。
    pub payload: BTreeMap<String, String>,
}

/// 入队结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnqueueOutcome {
    /// 落库的投递记录。
    pub delivery: DeliveryRow,
    /// 是否因退订而直接进死信（AC-S-169 步骤 3）。
    pub opted_out: bool,
}

/// 通知系统的编排服务。
pub struct NotifyService {
    store: NotifyStore,
    config: NotifyConfig,
}

impl NotifyService {
    /// 构造。
    pub fn new(store: NotifyStore, config: NotifyConfig) -> Self {
        Self { store, config }
    }

    // -- 模板 ---------------------------------------------------------------

    /// 建模板。
    ///
    /// 唯一约束冲突（同 `(code, channel, locale)`）由 store 的 `DbError` 冒上来 →
    /// 调用方（http 层）把它翻译成 `NOTIFY_TEMPLATE_DUPLICATED`。
    pub async fn create_template(
        &self,
        ctx: &RequestContext,
        new: &NewTemplate,
    ) -> Result<TemplateRow, crate::error::ServiceError> {
        // 先校验模板语法：非法占位符要在**建模板时**就拒，
        // 而不是等到投递时才炸（否则管理员会以为保存成功了）。
        template::extract_variables(&new.title_template)?;
        template::extract_variables(&new.body_template)?;
        Ok(self.store.insert_template(ctx, new).await?)
    }

    /// 改模板。
    pub async fn update_template(
        &self,
        ctx: &RequestContext,
        template_id: &str,
        update: &TemplateUpdate,
    ) -> Result<TemplateRow, crate::error::ServiceError> {
        if let Some(body) = &update.body_template {
            template::extract_variables(body)?;
        }
        if let Some(title) = &update.title_template {
            template::extract_variables(title)?;
        }
        self.store
            .update_template(ctx, template_id, update)
            .await?
            .ok_or(NotifyError::TemplateNotFound.into())
    }

    /// 列出模板。
    pub async fn list_templates(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<TemplateRow>, crate::error::ServiceError> {
        Ok(self.store.list_templates(ctx).await?)
    }

    /// 按 ID 读模板。
    pub async fn find_template(
        &self,
        ctx: &RequestContext,
        template_id: &str,
    ) -> Result<Option<TemplateRow>, crate::error::ServiceError> {
        Ok(self.store.find_template(ctx, template_id).await?)
    }

    /// 删模板。
    pub async fn delete_template(
        &self,
        ctx: &RequestContext,
        template_id: &str,
    ) -> Result<(), crate::error::ServiceError> {
        let affected = self.store.delete_template(ctx, template_id).await?;
        if affected == 0 {
            return Err(NotifyError::TemplateNotFound.into());
        }
        Ok(())
    }

    /// 预览（把模板用给定 payload 渲染一次，**不落库**）。
    pub fn preview(
        &self,
        template: &TemplateRow,
        payload: &BTreeMap<String, String>,
    ) -> Result<(String, String), NotifyError> {
        let title = template::render(&template.title_template, payload)?;
        let body = template::render(&template.body_template, payload)?;
        Ok((title, body))
    }

    // -- locale 三级回退（D294）--------------------------------------------

    /// 按「用户 locale → 租户默认 → 失败」找模板（AC-S-168）。
    ///
    /// ⚠️ **不得**回退到「任意一个存在的 locale」——
    /// 那会让用户收到他看不懂的语言（AC-S-168 步骤 4/5）。
    pub async fn resolve_template(
        &self,
        ctx: &RequestContext,
        code: &str,
        channel: &str,
        user_locale: Option<&str>,
    ) -> Result<TemplateRow, NotifyError> {
        // 第 1 级：用户 locale。
        if let Some(locale) = user_locale {
            if let Some(row) = self
                .store
                .find_template_for_render(ctx, code, channel, locale)
                .await
                .map_err(internal)?
            {
                return Ok(row);
            }
        }
        // 第 2 级：租户默认 locale。
        let tenant_default = self.config.tenant_default_locale(ctx).await;
        if let Some(row) = self
            .store
            .find_template_for_render(ctx, code, channel, &tenant_default)
            .await
            .map_err(internal)?
        {
            return Ok(row);
        }
        // 第 3 级：**失败**（不回退到任意存在的 locale）。
        Err(NotifyError::TemplateNotFound)
    }

    // -- 入队 ---------------------------------------------------------------

    /// 入队一条通知。
    pub async fn enqueue(
        &self,
        ctx: &RequestContext,
        req: &EnqueueRequest,
    ) -> Result<EnqueueOutcome, crate::error::ServiceError> {
        // ① 找模板（三级回退）。
        let tpl = self
            .resolve_template(ctx, &req.template_code, &req.channel, req.locale.as_deref())
            .await?;

        // ④ 敏感键扫描（在渲染前做，避免把敏感值渲染进快照）。
        scan_payload(&req.payload)?;

        // ② 偏好检查（**投递尝试之前**，D312）。
        let opted_out = self
            .is_recipient_opted_out(ctx, &req.recipient, &tpl.category)
            .await?;

        // ③ 渲染（用**模板自己的** locale，不是用户的 —— 回退可能已经换了语言）。
        let (title, body) = self.preview(&tpl, &req.payload)?;

        let new = NewDelivery {
            id: uuid::Uuid::new_v4().to_string(),
            template_code: tpl.code.clone(),
            channel: tpl.channel.clone(),
            category: tpl.category.clone(),
            recipient: req.recipient.clone(),
            locale: tpl.locale.clone(),
            payload: serde_json::to_value(&req.payload)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            rendered_title: title,
            rendered_body: body,
            next_attempt_at: time::OffsetDateTime::now_utc(),
        };
        let mut delivery = self.store.insert_delivery(ctx, &new).await?;

        // 已退订 → **直接进死信**，attempt_count 保持 0（不重试）。
        if opted_out {
            let reason = format!(
                "the recipient has opted out of the `{}` category",
                tpl.category
            );
            self.store
                .record_outcome(
                    ctx,
                    &delivery.id,
                    &DeliveryOutcome::DeadLetter {
                        error: reason.clone(),
                    },
                )
                .await?;
            delivery.status = status::DEAD_LETTER.to_string();
            delivery.last_error = Some(reason);
            // ⚠️ attempt_count 仍是 0（D312）。
        }

        Ok(EnqueueOutcome {
            delivery,
            opted_out,
        })
    }

    /// 收件人是否退订了该分类。
    ///
    /// ⚠️ **`system` 类永不算退订**（D295：强制通知不可退订）——
    /// 即使偏好表里有一行（理论上不该有），也不生效。
    async fn is_recipient_opted_out(
        &self,
        ctx: &RequestContext,
        recipient: &str,
        category_name: &str,
    ) -> Result<bool, crate::error::ServiceError> {
        if !category::is_optional(category_name) {
            // system 类不可退订 → 一律视为未退订。
            return Ok(false);
        }
        Ok(self
            .store
            .is_opted_out(ctx, recipient, category_name)
            .await?)
    }

    // -- 订阅偏好 -----------------------------------------------------------

    /// 设退订状态（**先校验后写**，AC-S-169 步骤 5）。
    ///
    /// ⚠️ 校验必须在**写之前**：若先写后报错，用户的 `system` 就真的被关掉了
    /// （判据要求偏好表里**不得出现** `system` 行）。
    pub async fn set_preference(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        category_name: &str,
        opted_out: bool,
    ) -> Result<PreferenceRow, crate::error::ServiceError> {
        if !category::ALL.contains(&category_name) {
            return Err(NotifyError::CategoryMandatory {
                category: "unknown",
            }
            .into());
        }
        // 强制类：**任何**退订尝试都拒（不只是 opted_out = true）。
        if !category::is_optional(category_name) {
            return Err(NotifyError::CategoryMandatory {
                category: category::SYSTEM,
            }
            .into());
        }
        Ok(self
            .store
            .upsert_preference(ctx, user_id, category_name, opted_out)
            .await?)
    }

    /// 读某用户的偏好。
    pub async fn list_preferences(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> Result<Vec<PreferenceRow>, crate::error::ServiceError> {
        Ok(self.store.list_preferences(ctx, user_id).await?)
    }

    // -- 投递记录 / 重投 ---------------------------------------------------

    /// 读投递记录。
    pub async fn list_deliveries(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<DeliveryRow>, crate::error::ServiceError> {
        Ok(self.store.list_deliveries(ctx).await?)
    }

    /// 重投死信（AC-S-172）。
    ///
    /// ⚠️ **必须先读当前状态**，因为 `NOTIFY_NOT_DEAD_LETTER` 要带上当前状态；
    /// 但**判定成功与否靠 `rows_affected`**，不是靠这次读到的状态 ——
    /// 两次读之间状态可能被别人改了（那正是并发判据要抓的）。
    pub async fn resend_dead_letter(
        &self,
        ctx: &RequestContext,
        delivery_id: &str,
    ) -> Result<DeliveryRow, crate::error::ServiceError> {
        let before = self.store.find_delivery(ctx, delivery_id).await?.ok_or(
            crate::error::ServiceError::Notify(
                NotifyError::TemplateNotFound, // 由 http 层换成通用 NOT_FOUND
            ),
        )?;

        let affected = self.store.resend_dead_letter(ctx, delivery_id).await?;
        if affected == 0 {
            // 影响行数为 0 → 此刻它不是死信（并发下另一个请求已经抢先改了）。
            let current = self.store.find_delivery(ctx, delivery_id).await?;
            let status_now = current.map(|row| row.status).unwrap_or(before.status);
            return Err(NotifyError::NotDeadLetter {
                status: leak_str(status_now),
            }
            .into());
        }
        self.store
            .find_delivery(ctx, delivery_id)
            .await?
            .ok_or(NotifyError::TemplateNotFound.into())
    }

    /// 读底层 store（worker 用）。
    pub fn store(&self) -> &NotifyStore {
        &self.store
    }

    /// 读配置（worker 用）。
    pub fn config(&self) -> &NotifyConfig {
        &self.config
    }
}

/// 把 `DbError` 折成 `NotifyError` 的兜底（只在 `resolve_template` 里用，
/// 因为那里返回的是 `NotifyError` 而不是 `ServiceError`）。
fn internal(err: platform_db::error::DbError) -> NotifyError {
    tracing::error!(error = %err, "通知系统读模板失败");
    NotifyError::TemplateNotFound
}

/// 把运行期的状态字符串**映射回静态字面量**（`NotDeadLetter.status` 是 `&'static str`）。
fn leak_str(status: String) -> &'static str {
    match status.as_str() {
        status::PENDING => status::PENDING,
        status::DELIVERING => status::DELIVERING,
        status::DELIVERED => status::DELIVERED,
        status::DEAD_LETTER => status::DEAD_LETTER,
        _ => "unknown",
    }
}

/// 敏感键扫描（复用 REQ-014 的 `scan_sensitive_keys`，D299）。
///
/// ⚠️ 输入是**扁平化的 `BTreeMap`**，所以包一层 JSON 对象再扫 ——
/// `scan_sensitive_keys` 的契约是「喂 JSON 字符串」。
pub fn scan_payload(payload: &BTreeMap<String, String>) -> Result<(), NotifyError> {
    let value = serde_json::to_value(payload)
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
    let text = value.to_string();
    let hits = platform_core::extension::audit::scan_sensitive_keys(&text);
    if hits.is_empty() {
        return Ok(());
    }
    let mut keys: Vec<String> = hits.into_iter().collect();
    keys.sort();
    Err(NotifyError::SensitivePayload { keys })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 敏感键命中：4 个 camelCase 键逐个拒（AC-S-173 步骤 4）。
    #[test]
    fn the_four_sensitive_keys_are_rejected() {
        for key in ["password", "clientSecret", "apiKey", "authorization"] {
            let mut payload = BTreeMap::new();
            payload.insert(key.to_string(), "x".to_string());
            let err = scan_payload(&payload).expect_err(&format!("{key} 必须被拒"));
            assert_eq!(err.code(), "NOTIFY_SENSITIVE_PAYLOAD", "键 {key}");
        }
    }

    /// 阳性对照：`secretary` **不**命中（完整键名匹配，非子串）。
    #[test]
    fn an_innocent_lookalike_is_not_rejected() {
        let mut payload = BTreeMap::new();
        payload.insert("secretary".to_string(), "x".to_string());
        assert!(
            scan_payload(&payload).is_ok(),
            "secretary 不含敏感键，不得误伤"
        );
    }

    /// 空 payload 通过。
    #[test]
    fn an_empty_payload_passes() {
        assert!(scan_payload(&BTreeMap::new()).is_ok());
    }

    /// 命中时**列出全部**命中的键（不只第一个）。
    #[test]
    fn all_hit_keys_are_reported() {
        let mut payload = BTreeMap::new();
        payload.insert("password".to_string(), "a".to_string());
        payload.insert("apiKey".to_string(), "b".to_string());
        let err = scan_payload(&payload).expect_err("应命中");
        let detail = err.detail();
        assert!(detail.contains("password"), "必须列出 password：{detail}");
        assert!(detail.contains("api_key"), "必须列出 api_key：{detail}");
    }

    /// `leak_str` 把四种合法状态映射回静态字面量，未知值给 `unknown`。
    #[test]
    fn leak_str_maps_known_statuses() {
        assert_eq!(leak_str(status::PENDING.to_string()), "pending");
        assert_eq!(leak_str(status::DELIVERING.to_string()), "delivering");
        assert_eq!(leak_str(status::DELIVERED.to_string()), "delivered");
        assert_eq!(leak_str(status::DEAD_LETTER.to_string()), "dead_letter");
        assert_eq!(leak_str("something-else".to_string()), "unknown");
    }
}
