//! 数据访问（REQ-015，design §2）。
//!
//! # 租户作用域
//!
//! 三张表都带 `tenant_id` → 语句里**必须**有 `tenant_id = {tenant}`
//! （RLS 兜底 + 应用层兜底）。
//!
//! # 事务（D148）
//!
//! ⚠️ 每一次访问都由 `TenantDb` **自己**开事务 → `set_config('app.tenant_id', …, true)`
//! → 执行 → 提交，租户上下文**只在事务内有效**。业务层拿不到裸连接。
//!
//! 需要「多步原子」的地方（出队认领 + 逐个更新）由本层用 `TenantDb::begin`
//! 在**同一个事务**里做完 —— 否则「取」和「标记」分属两个事务，多实例下会重复投递。
//!
//! # ⚠️ 三处**必须**用「影响行数」做判据（D298 / AC-S-172 步骤 5）
//!
//! [`NotifyStore::resend_dead_letter`] 的 `UPDATE … WHERE status = 'dead_letter'`
//! **必须**返回 `rows_affected`，调用方据此区分「成功」与「不是死信」。
//! **不许**写成「先 SELECT 查状态、再 UPDATE」：
//! 并发下两条请求都读到 `dead_letter` → **两条都成功**。

use std::collections::BTreeMap;

use platform_core::RequestContext;
use platform_db::{DbResult, TenantDb, TenantQuery};
use time::OffsetDateTime;

use crate::model::{
    DELIVERY_TABLE, DeliveryRow, PREFERENCE_TABLE, PreferenceRow, TEMPLATE_TABLE, TemplateRow,
    status,
};

/// 模板表的列清单。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn template_columns() -> &'static str {
    "id, tenant_id, code, channel, locale, category, title_template, body_template, \
     created_at, updated_at"
}

/// 查询用的模板列清单（`id` 是 `uuid` 列，转成 `text` 以匹配 `String` 字段）。
///
/// # ⚠️ 为什么要有两份清单
///
/// `template_columns()` 是**库层真列名**（`contract` 测试比对它与
/// `information_schema.columns` —— 带 `::text` 会比对失败）。
/// 而**查询**返回给 `TemplateRow`（`id: String`）时，sqlx 要求类型匹配
/// （`uuid` 列解成 `String` 会报 `not compatible with SQL type UUID`）。
/// → 查询用本函数，DDL 断言用 `template_columns()`。
fn template_select_columns() -> &'static str {
    "id::text AS id, tenant_id, code, channel, locale, category, title_template, \
     body_template, created_at, updated_at"
}

/// 偏好表的列清单。
pub(crate) fn preference_columns() -> &'static str {
    "tenant_id, user_id, category, opted_out, updated_at"
}

/// 查询用的投递列清单（`id` 转 `text`，理由同 [`template_select_columns`]）。
fn delivery_select_columns() -> &'static str {
    "id::text AS id, tenant_id, template_code, channel, category, recipient, locale, payload, \
     rendered_title, rendered_body, status, attempt_count, next_attempt_at, \
     last_error, provider_message_id, created_at, updated_at, delivered_at"
}

/// 投递表的列清单（**恰好 18 列**，D299）—— **库层真列名**，用于 DDL 断言。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn delivery_columns() -> &'static str {
    "id, tenant_id, template_code, channel, category, recipient, locale, payload, \
     rendered_title, rendered_body, status, attempt_count, next_attempt_at, \
     last_error, provider_message_id, created_at, updated_at, delivered_at"
}

/// 待写入的模板。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTemplate {
    /// 模板 ID（平台生成）。
    pub id: String,
    /// 模板编码。
    pub code: String,
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
}

/// 模板的部分更新（`None` = 不改）。
///
/// ⚠️ 可直接从 JSON 反序列化（`PATCH` 的请求体）：字段全可选，
/// 缺省即「不改」。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateUpdate {
    /// 标题模板。
    #[serde(default)]
    pub title_template: Option<String>,
    /// 正文模板。
    #[serde(default)]
    pub body_template: Option<String>,
    /// 通知分类。
    #[serde(default)]
    pub category: Option<String>,
}

/// 待入队的投递记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDelivery {
    /// 投递 ID。
    pub id: String,
    /// 来源模板的 code（**不是**外键）。
    pub template_code: String,
    /// 通道名。
    pub channel: String,
    /// 通知分类。
    pub category: String,
    /// 接收人。
    pub recipient: String,
    /// 渲染用的语言。
    pub locale: String,
    /// 渲染变量（已过敏感键扫描）。
    pub payload: serde_json::Value,
    /// 渲染结果标题（快照）。
    pub rendered_title: String,
    /// 渲染结果正文（快照）。
    pub rendered_body: String,
    /// 首次尝试时间。
    pub next_attempt_at: OffsetDateTime,
}

/// 一个 worker 认领到的待投递项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedDelivery {
    /// 投递 ID。
    pub id: String,
    /// 通道名。
    pub channel: String,
    /// 接收人。
    pub recipient: String,
    /// 渲染结果标题。
    pub rendered_title: String,
    /// 渲染结果正文。
    pub rendered_body: String,
    /// 渲染变量。
    pub payload: BTreeMap<String, String>,
    /// 当前已尝试次数（**已经**包含本次认领前的值）。
    pub attempt_count: i32,
}

/// 投递结果（写回用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// 成功。
    Delivered {
        /// 通道返回的消息 ID。
        provider_message_id: Option<String>,
    },
    /// 失败且还有重试机会。
    Retry {
        /// 失败原因。
        error: String,
        /// 下次尝试时间。
        next_attempt_at: OffsetDateTime,
    },
    /// 失败且次数用尽（→ 死信）。
    DeadLetter {
        /// 失败原因。
        error: String,
    },
}

/// 通知系统的数据访问。
pub struct NotifyStore {
    db: TenantDb,
}

impl NotifyStore {
    /// 用连接池构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 底层 `TenantDb`（装配层用它开事务）。
    pub fn db(&self) -> &TenantDb {
        &self.db
    }

    // -- 模板 ---------------------------------------------------------------

    /// 建模板。违反 `UNIQUE (tenant_id, code, channel, locale)` → `DbError`（唯一冲突）。
    pub async fn insert_template(
        &self,
        ctx: &RequestContext,
        new: &NewTemplate,
    ) -> DbResult<TemplateRow> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
               (id, tenant_id, code, channel, locale, category, title_template, body_template, \
                created_at, updated_at) \
             VALUES (?::uuid, {{tenant}}, ?, ?, ?, ?, ?, ?, now(), now()) \
             RETURNING {columns}",
            table = self.db.table(TEMPLATE_TABLE),
            columns = template_select_columns()
        ))
        .bind(new.id.clone())
        .bind(new.code.clone())
        .bind(new.channel.clone())
        .bind(new.locale.clone())
        .bind(new.category.clone())
        .bind(new.title_template.clone())
        .bind(new.body_template.clone());
        self.db.fetch_one(ctx, &query).await
    }

    /// 按 ID 读模板。
    pub async fn find_template(
        &self,
        ctx: &RequestContext,
        template_id: &str,
    ) -> DbResult<Option<TemplateRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} AND id = ?::uuid",
            table = self.db.table(TEMPLATE_TABLE),
            columns = template_select_columns()
        ))
        .bind(template_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 按 `(code, channel, locale)` 读模板（**渲染入口**，D294 的三级回退用）。
    pub async fn find_template_for_render(
        &self,
        ctx: &RequestContext,
        code: &str,
        channel: &str,
        locale: &str,
    ) -> DbResult<Option<TemplateRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} \
             WHERE tenant_id = {{tenant}} AND code = ? AND channel = ? AND locale = ?",
            table = self.db.table(TEMPLATE_TABLE),
            columns = template_select_columns()
        ))
        .bind(code.to_string())
        .bind(channel.to_string())
        .bind(locale.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 列出模板（稳定序）。
    pub async fn list_templates(&self, ctx: &RequestContext) -> DbResult<Vec<TemplateRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} \
             ORDER BY code, channel, locale, id",
            table = self.db.table(TEMPLATE_TABLE),
            columns = template_select_columns()
        ));
        self.db.fetch_all(ctx, &query).await
    }

    /// 改模板（**只改传了的字段**，`COALESCE` 保留原值）。
    pub async fn update_template(
        &self,
        ctx: &RequestContext,
        template_id: &str,
        update: &TemplateUpdate,
    ) -> DbResult<Option<TemplateRow>> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET \
               title_template = COALESCE(?, title_template), \
               body_template  = COALESCE(?, body_template), \
               category       = COALESCE(?, category), \
               updated_at     = now() \
             WHERE tenant_id = {{tenant}} AND id = ?::uuid \
             RETURNING {columns}",
            table = self.db.table(TEMPLATE_TABLE),
            columns = template_select_columns()
        ))
        .bind(update.title_template.clone())
        .bind(update.body_template.clone())
        .bind(update.category.clone())
        .bind(template_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 删模板，返回**影响行数**（0 = 不存在）。
    pub async fn delete_template(&self, ctx: &RequestContext, template_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND id = ?::uuid",
            table = self.db.table(TEMPLATE_TABLE)
        ))
        .bind(template_id.to_string());
        self.db.execute(ctx, &query).await
    }

    // -- 订阅偏好 -----------------------------------------------------------

    /// 设/取消退订（UPSERT）。
    ///
    /// ⚠️ **`system` 类的拒绝在服务层做**（先校验后写）——
    /// 不能靠「先写后报错」：那样用户的 `system` 就真的被关掉了
    /// （AC-S-169 步骤 5 要求偏好表里**不得出现** `system` 行）。
    pub async fn upsert_preference(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        category: &str,
        opted_out: bool,
    ) -> DbResult<PreferenceRow> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, user_id, category, opted_out, updated_at) \
             VALUES ({{tenant}}, ?, ?, ?, now()) \
             ON CONFLICT (tenant_id, user_id, category) \
             DO UPDATE SET opted_out = EXCLUDED.opted_out, updated_at = now() \
             RETURNING {columns}",
            table = self.db.table(PREFERENCE_TABLE),
            columns = preference_columns()
        ))
        .bind(user_id.to_string())
        .bind(category.to_string())
        .bind(opted_out);
        self.db.fetch_one(ctx, &query).await
    }

    /// 读某用户的全部偏好。
    pub async fn list_preferences(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<Vec<PreferenceRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} AND user_id = ? \
             ORDER BY category",
            table = self.db.table(PREFERENCE_TABLE),
            columns = preference_columns()
        ))
        .bind(user_id.to_string());
        self.db.fetch_all(ctx, &query).await
    }

    /// 查某用户是否退订了某分类（**投递前的偏好检查**，AC-S-169 步骤 3）。
    pub async fn is_opted_out(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        category: &str,
    ) -> DbResult<bool> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND category = ?",
            table = self.db.table(PREFERENCE_TABLE),
            columns = preference_columns()
        ))
        .bind(user_id.to_string())
        .bind(category.to_string());
        Ok(self
            .db
            .fetch_optional::<PreferenceRow>(ctx, &query)
            .await?
            .map(|row| row.opted_out)
            .unwrap_or(false))
    }

    // -- 投递记录 -----------------------------------------------------------

    /// 入队一条投递。
    pub async fn insert_delivery(
        &self,
        ctx: &RequestContext,
        new: &NewDelivery,
    ) -> DbResult<DeliveryRow> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
               (id, tenant_id, template_code, channel, category, recipient, locale, payload, \
                rendered_title, rendered_body, status, attempt_count, next_attempt_at, \
                created_at, updated_at) \
             VALUES (?::uuid, {{tenant}}, ?, ?, ?, ?, ?, ?::jsonb, ?, ?, ?, 0, ?, now(), now()) \
             RETURNING {columns}",
            table = self.db.table(DELIVERY_TABLE),
            columns = delivery_select_columns()
        ))
        .bind(new.id.clone())
        .bind(new.template_code.clone())
        .bind(new.channel.clone())
        .bind(new.category.clone())
        .bind(new.recipient.clone())
        .bind(new.locale.clone())
        .bind(new.payload.to_string())
        .bind(new.rendered_title.clone())
        .bind(new.rendered_body.clone())
        .bind(status::PENDING.to_string())
        .bind(new.next_attempt_at);
        self.db.fetch_one(ctx, &query).await
    }

    /// 按 ID 读投递记录。
    pub async fn find_delivery(
        &self,
        ctx: &RequestContext,
        delivery_id: &str,
    ) -> DbResult<Option<DeliveryRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} AND id = ?::uuid",
            table = self.db.table(DELIVERY_TABLE),
            columns = delivery_select_columns()
        ))
        .bind(delivery_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 列出投递记录（稳定序：新的在前）。
    pub async fn list_deliveries(&self, ctx: &RequestContext) -> DbResult<Vec<DeliveryRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} \
             ORDER BY created_at DESC, id",
            table = self.db.table(DELIVERY_TABLE),
            columns = delivery_select_columns()
        ));
        self.db.fetch_all(ctx, &query).await
    }

    // -- worker 出队 --------------------------------------------------------

    /// 认领一批待投递项（**`FOR UPDATE SKIP LOCKED`**，D296）。
    ///
    /// # 取什么
    ///
    /// - `status IN ('pending','delivering')` —— `delivering` 是**崩溃残留**，
    ///   超过 `delivering_timeout_seconds` 就要回收（否则永远卡住）；
    /// - `next_attempt_at <= now()`；
    /// - `LIMIT batch`。
    ///
    /// # ⚠️ `SKIP LOCKED` 让多实例不重复
    ///
    /// 两个 worker 同时取：第一个锁住前 N 行，第二个**跳过**已锁的行去取后面的
    /// → 天然支持多实例（AC-S-170 步骤 2）。
    ///
    /// # ⚠️ 认领在**同一个事务**里完成
    ///
    /// 取 + 置 `delivering` + `attempt_count + 1` 必须原子 ——
    /// 否则两个 worker 都取到同一行。
    pub async fn claim_batch(
        &self,
        ctx: &RequestContext,
        batch: i64,
        delivering_timeout_seconds: i64,
    ) -> DbResult<Vec<ClaimedDelivery>> {
        let mut tx = self.db.begin(ctx).await?;
        let table = format!("{}.{}", tx.schema(), DELIVERY_TABLE);
        // ⚠️ `{tenant}` **只能出现一次**（`TenantQuery::render` 强制要求恰好一个）。
        //
        // # ⚠️ `LIMIT` 为什么用 **CTE + `= ANY(...)`**（QA M3 的修复）
        //
        // 原写法是 `UPDATE ... WHERE id IN (SELECT ... LIMIT ? FOR UPDATE SKIP LOCKED)`。
        // `IN (子查询)` 可能被实现成 semi join → 子查询在**外层每一行**上重跑
        // （计划里 `loops = N`）→ `LIMIT` 只约束每次子查询，约束不了 UPDATE 总行数。
        //
        // **实测**：突发 500 条队列上 `claim_batch(batch = 10)` 认领了 **16 条**（超出 60%）；
        // 小表 6 条时 `batch = 2` 认领了 **3 条**。
        //
        // → 改成 **CTE 物化 + `= ANY(数组)`**：子查询只执行一次，
        // 认领出的 id 变成数组，外层 UPDATE 精确匹配 —— 行数由 `LIMIT` 严格决定。
        let query = TenantQuery::new(format!(
            "WITH picked AS ( \
               SELECT id FROM {table} \
               WHERE tenant_id = {{tenant}} \
                 AND ( \
                   (status = ? AND next_attempt_at <= now()) \
                   OR (status = ? AND updated_at <= now() - make_interval(secs => ?)) \
                 ) \
               ORDER BY next_attempt_at \
               LIMIT ? \
               FOR UPDATE SKIP LOCKED \
             ) \
             UPDATE {table} SET \
               status = ?, \
               attempt_count = attempt_count + 1, \
               updated_at = now() \
             WHERE id = ANY(ARRAY(SELECT id FROM picked)) \
             RETURNING id::text AS id, channel, recipient, rendered_title, rendered_body, payload, attempt_count",
            table = table
        ))
        .bind(status::PENDING.to_string())
        .bind(status::DELIVERING.to_string())
        .bind(delivering_timeout_seconds)
        .bind(batch)
        .bind(status::DELIVERING.to_string());

        let rows: Vec<ClaimedRow> = tx.fetch_all(&query).await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(ClaimedDelivery::from).collect())
    }

    /// 写回一次投递的结果。
    ///
    /// ⚠️ 三种结果各自的状态迁移（D298 的状态机）：
    /// - `Delivered` → `delivered` + `delivered_at`
    /// - `Retry` → `pending` + `next_attempt_at`
    /// - `DeadLetter` → `dead_letter`
    pub async fn record_outcome(
        &self,
        ctx: &RequestContext,
        delivery_id: &str,
        outcome: &DeliveryOutcome,
    ) -> DbResult<u64> {
        let query = match outcome {
            DeliveryOutcome::Delivered {
                provider_message_id,
            } => TenantQuery::new(format!(
                "UPDATE {table} SET status = ?, delivered_at = now(), updated_at = now(), \
                   last_error = NULL, provider_message_id = ? \
                 WHERE tenant_id = {{tenant}} AND id = ?::uuid",
                table = self.db.table(DELIVERY_TABLE)
            ))
            .bind(status::DELIVERED.to_string())
            .bind(provider_message_id.clone())
            .bind(delivery_id.to_string()),
            DeliveryOutcome::Retry {
                error,
                next_attempt_at,
            } => TenantQuery::new(format!(
                "UPDATE {table} SET status = ?, next_attempt_at = ?, updated_at = now(), \
                   last_error = ? \
                 WHERE tenant_id = {{tenant}} AND id = ?::uuid",
                table = self.db.table(DELIVERY_TABLE)
            ))
            .bind(status::PENDING.to_string())
            .bind(*next_attempt_at)
            .bind(error.clone())
            .bind(delivery_id.to_string()),
            DeliveryOutcome::DeadLetter { error } => TenantQuery::new(format!(
                "UPDATE {table} SET status = ?, updated_at = now(), last_error = ? \
                 WHERE tenant_id = {{tenant}} AND id = ?::uuid",
                table = self.db.table(DELIVERY_TABLE)
            ))
            .bind(status::DEAD_LETTER.to_string())
            .bind(error.clone())
            .bind(delivery_id.to_string()),
        };
        self.db.execute(ctx, &query).await
    }

    /// 人工重投死信（D298，AC-S-172）。
    ///
    /// # ⚠️ 返回**影响行数**，调用方据此区分「成功」与「不是死信」
    ///
    /// `WHERE … AND status = 'dead_letter'` 的 `rows_affected == 0` → 409
    /// `NOTIFY_NOT_DEAD_LETTER`。
    ///
    /// **不许**写成「先 SELECT 查状态、再 UPDATE」：
    /// 并发下两条请求都读到 `dead_letter` → **两条都成功**（AC-S-172 步骤 5）。
    pub async fn resend_dead_letter(
        &self,
        ctx: &RequestContext,
        delivery_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET \
               status = ?, attempt_count = 0, next_attempt_at = now(), \
               last_error = NULL, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?::uuid AND status = ?",
            table = self.db.table(DELIVERY_TABLE)
        ))
        .bind(status::PENDING.to_string())
        .bind(delivery_id.to_string())
        .bind(status::DEAD_LETTER.to_string());
        self.db.execute(ctx, &query).await
    }
}

/// `claim_batch` 的 RETURNING 行。
#[derive(Debug)]
struct ClaimedRow {
    id: String,
    channel: String,
    recipient: String,
    rendered_title: String,
    rendered_body: String,
    payload: serde_json::Value,
    attempt_count: i32,
}

impl From<ClaimedRow> for ClaimedDelivery {
    fn from(row: ClaimedRow) -> Self {
        let payload = match row.payload {
            serde_json::Value::Object(map) => map
                .into_iter()
                .map(|(k, v)| {
                    let text = match v {
                        serde_json::Value::String(s) => s,
                        other => other.to_string(),
                    };
                    (k, text)
                })
                .collect(),
            _ => BTreeMap::new(),
        };
        Self {
            id: row.id,
            channel: row.channel,
            recipient: row.recipient,
            rendered_title: row.rendered_title,
            rendered_body: row.rendered_body,
            payload,
            attempt_count: row.attempt_count,
        }
    }
}

platform_db::tenant_row!(ClaimedRow {
    id: "id",
    channel: "channel",
    recipient: "recipient",
    rendered_title: "rendered_title",
    rendered_body: "rendered_body",
    payload: "payload",
    attempt_count: "attempt_count",
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DELIVERY_COLUMNS;
    use platform_db::BindValue;

    /// `OffsetDateTime` 能绑成 `BindValue::Timestamp`（`timestamptz` 列要用它）。
    ///
    /// ⚠️ 这条不是凑数：`TenantQuery::bind` 走 `impl Into<BindValue>`，
    /// 若 `OffsetDateTime` 没有对应变体，`next_attempt_at = ?` 就绑不进去 ——
    /// 而那是**重试退避的核心列**（AC-S-171）。
    #[test]
    fn an_offset_date_time_can_be_bound() {
        let value = OffsetDateTime::UNIX_EPOCH;
        let bound: BindValue = value.into();
        assert!(
            matches!(bound, BindValue::Timestamp(_)),
            "OffsetDateTime 必须绑成 Timestamp"
        );
    }

    /// 三张表的列清单与 `model` 的常量**逐字一致**（防止 store 与 model 漂移）。
    #[test]
    fn the_delivery_columns_match_the_model_constant() {
        let listed: Vec<&str> = delivery_columns().split(',').map(|s| s.trim()).collect();
        assert_eq!(
            listed, DELIVERY_COLUMNS,
            "store 的列清单必须与 model::DELIVERY_COLUMNS 逐字一致（顺序也要一致）"
        );
    }

    /// 模板列清单恰好 10 列。
    #[test]
    fn the_template_columns_are_exactly_the_ten_documented() {
        let listed: Vec<&str> = template_columns().split(',').map(|s| s.trim()).collect();
        assert_eq!(listed.len(), 10, "模板表列数");
        for expected in [
            "id",
            "tenant_id",
            "code",
            "channel",
            "locale",
            "category",
            "title_template",
            "body_template",
            "created_at",
            "updated_at",
        ] {
            assert!(
                listed.contains(&expected),
                "模板表缺列 {expected}：{listed:?}"
            );
        }
    }

    /// 偏好列清单恰好 5 列。
    #[test]
    fn the_preference_columns_are_exactly_the_five_documented() {
        let listed: Vec<&str> = preference_columns().split(',').map(|s| s.trim()).collect();
        assert_eq!(
            listed,
            vec![
                "tenant_id",
                "user_id",
                "category",
                "opted_out",
                "updated_at"
            ]
        );
    }

    /// 投递表列数 **18**（D299）。
    #[test]
    fn the_delivery_column_count_is_eighteen() {
        let count = delivery_columns().split(',').count();
        assert_eq!(count, 18, "D299：投递表恰好 18 列");
    }

    /// `ClaimedRow` 的 payload 转换：字符串原样、其它类型转 `to_string()`。
    #[test]
    fn the_claimed_row_payload_is_flattened_to_strings() {
        let row = ClaimedRow {
            id: "d".into(),
            channel: "in_app".into(),
            recipient: "u".into(),
            rendered_title: "t".into(),
            rendered_body: "b".into(),
            payload: serde_json::json!({"name": "张三", "count": 3, "ok": true}),
            attempt_count: 1,
        };
        let claimed: ClaimedDelivery = row.into();
        assert_eq!(claimed.payload.get("name"), Some(&"张三".to_string()));
        assert_eq!(claimed.payload.get("count"), Some(&"3".to_string()));
        assert_eq!(claimed.payload.get("ok"), Some(&"true".to_string()));
        assert_eq!(claimed.attempt_count, 1);
    }

    /// payload 不是对象时 → 空 map（不 panic）。
    #[test]
    fn a_non_object_payload_yields_an_empty_map() {
        let row = ClaimedRow {
            id: "d".into(),
            channel: "in_app".into(),
            recipient: "u".into(),
            rendered_title: "t".into(),
            rendered_body: "b".into(),
            payload: serde_json::json!([1, 2, 3]),
            attempt_count: 0,
        };
        let claimed: ClaimedDelivery = row.into();
        assert!(claimed.payload.is_empty(), "非对象 payload 应得空 map");
    }
}
