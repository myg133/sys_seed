//! `demoItem` 实体：**元数据驱动渲染的活样本**（REQ-012）。
//!
//! # 这个实体为什么长这样
//!
//! `modules/sandbox/manifest.json` 的 `schema.demoItem` 声明了 7 个字段：
//!
//! ```text
//! id / title / amount / paid / dueAt / ownerId / secret
//! ```
//!
//! 表里还多两列 —— 它们**故意**不写进元数据：
//!
//! | 列 | 为什么存在 |
//! |----|------------|
//! | `created_at` | 服务端造的时间戳，前端不需要编辑 |
//! | `internal_note` | **用来证明白名单语义**：实体里多出来的字段（元数据没声明的）默认不输出 |
//!
//! 白名单语义是 **fail closed**：少显示而不是多泄露。加字段忘了写元数据时，
//! 表现是「前端看不到」而不是「把内部字段泄给所有人」。
//!
//! # 与 `scope_item` 的关系
//!
//! `scope_item` 是 REQ-009 的验收资产（数据范围 + 属性级校验），**本需求不动它**。
//! 本实体只承担一件事：**按元数据渲染 + 字段级权限**。

use anyhow::{Result, anyhow};
use platform_core::context::RequestContext;
use platform_db::{TenantDb, TenantQuery};

/// `demoItem` 的表名（与契约夹具的 `WriteBoundary` 声明一致）。
pub const DEMO_ITEM_TABLE: &str = "sandbox_demo_item";

/// `demoItem` 的持久化形态。
///
/// ⚠️ 这里的字段比元数据**多**（`internal_note` / `created_at`）——
/// 序列化**不得**直接 `to_value(self)`，必须经
/// [`platform_core::extension::schema::SchemaProjector`]（契约 C-6 扫这个）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoItemRecord {
    /// 归属租户。
    pub tenant_id: String,
    /// 主键（租户内唯一）。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 金额（最小单位整数）。
    pub amount: i64,
    /// 是否已付款。
    pub paid: bool,
    /// 到期日（`YYYY-MM-DD` 文本；**不做时区换算**）。
    pub due_at: Option<String>,
    /// 负责人（引用另一个实体，本期只读 id 文本）。
    pub owner_id: String,
    /// 内部备注（**字段级权限**：无 `sandbox:demo:secret` 的用户看不到）。
    pub secret: Option<String>,
    /// 未写进元数据的内部字段（**永不出现在响应里**）。
    pub internal_note: Option<String>,
    /// 创建时间（服务端生成，未写进元数据）。
    pub created_at: String,
}

// 手写 `FromRow`（`platform-db` 的声明宏）：不需要 sqlx 的 `macros` feature。
platform_db::tenant_row!(DemoItemRecord {
    tenant_id: "tenant_id",
    id: "id",
    title: "title",
    amount: "amount",
    paid: "paid",
    due_at: "due_at",
    owner_id: "owner_id",
    secret: "secret",
    internal_note: "internal_note",
    created_at: "created_at",
});

impl DemoItemRecord {
    /// 序列化成**实体的完整形态**（含元数据未声明的键）。
    ///
    /// 返回它之后**必须**再过 [`platform_core::extension::schema::SchemaProjector::project`] ——
    /// 直接 `Json(...)` 输出就是泄露。
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "title": self.title,
            "amount": self.amount,
            "paid": self.paid,
            "dueAt": self.due_at,
            "ownerId": self.owner_id,
            "secret": self.secret,
            // 下面两个键**不在**元数据里 → 投影后必然消失（白名单语义）。
            "internalNote": self.internal_note,
            "createdAt": self.created_at,
        })
    }
}

/// 走 `platform-db` 的租户作用域仓储。
#[derive(Debug, Clone)]
pub struct PgDemoItemRepository {
    db: TenantDb,
}

impl PgDemoItemRepository {
    /// 用平台数据访问层构造仓储。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 表名（带 schema 前缀；schema 来自配置，不硬编码 `public`）。
    fn table(&self) -> String {
        self.db.table(DEMO_ITEM_TABLE)
    }

    /// 所有列（顺序与 `tenant_row!` 一致）。
    const COLUMNS: &'static str =
        "tenant_id, id, title, amount, paid, due_at, owner_id, secret, internal_note, created_at";

    /// 列表（**游标分页**：`cursor` 不透明，当前实现是上一页最后一条的 `id`）。
    ///
    /// 按 `id` 升序；多查一条用来判断「还有没有下一页」。
    pub async fn list(
        &self,
        ctx: &RequestContext,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<DemoItemRecord>, Option<String>)> {
        let table = self.table();
        let mut statement = format!(
            "SELECT {} FROM {table} WHERE tenant_id = {{tenant}}",
            Self::COLUMNS
        );
        let mut binds: Vec<String> = Vec::new();
        if let Some(cursor) = cursor {
            statement.push_str(" AND id > ?");
            binds.push(cursor.to_string());
        }
        statement.push_str(&format!(
            " ORDER BY id ASC LIMIT {}",
            limit.saturating_add(1)
        ));

        let mut query = TenantQuery::new(statement);
        for bind in binds {
            query = query.bind(bind);
        }

        let mut rows = self
            .db
            .fetch_all::<DemoItemRecord>(ctx, &query)
            .await
            .map_err(|err| anyhow!("list() failed: {err}"))?;

        let has_more = rows.len() > limit as usize;
        if has_more {
            rows.truncate(limit as usize);
        }
        let next_cursor = has_more.then(|| {
            rows.last()
                .map(|row| row.id.clone())
                .unwrap_or_default()
        });
        Ok((rows, next_cursor))
    }

    /// 详情（**查不到 = 不可见**：不存在与越权是同一个结果）。
    pub async fn find(&self, ctx: &RequestContext, id: &str) -> Result<Option<DemoItemRecord>> {
        let table = self.table();
        let query = TenantQuery::new(format!(
            "SELECT {} FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            Self::COLUMNS
        ))
        .bind(id.to_string());
        self.db
            .fetch_optional(ctx, &query)
            .await
            .map_err(|err| anyhow!("find() failed: {err}"))
    }

    /// 插入一条；主键冲突 → 错误（**不覆盖**既有数据）。
    pub async fn insert(&self, ctx: &RequestContext, item: &DemoItemRecord) -> Result<()> {
        let table = self.table();
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, title, amount, paid, due_at, owner_id, secret, internal_note, created_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ))
        .bind(item.id.clone())
        .bind(item.title.clone())
        .bind(item.amount)
        .bind(item.paid)
        .bind(item.due_at.clone())
        .bind(item.owner_id.clone())
        .bind(item.secret.clone())
        .bind(item.internal_note.clone())
        .bind(item.created_at.clone());
        self.db
            .execute(ctx, &query)
            .await
            .map_err(|err| anyhow!("insert() failed: {err}"))?;
        Ok(())
    }

    /// 更新一条；受影响行数为 0 → 记录不存在（或不在本租户）。
    pub async fn update(&self, ctx: &RequestContext, item: &DemoItemRecord) -> Result<()> {
        let table = self.table();
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET title = ?, amount = ?, paid = ?, due_at = ?, owner_id = ?, secret = ? \
             WHERE tenant_id = {{tenant}} AND id = ?"
        ))
        .bind(item.title.clone())
        .bind(item.amount)
        .bind(item.paid)
        .bind(item.due_at.clone())
        .bind(item.owner_id.clone())
        .bind(item.secret.clone())
        .bind(item.id.clone());
        let affected = self
            .db
            .execute(ctx, &query)
            .await
            .map_err(|err| anyhow!("update() failed: {err}"))?;
        if affected == 0 {
            return Err(anyhow!("the record does not exist in this tenant"));
        }
        Ok(())
    }

    /// 删除一条；受影响行数为 0 → 记录不存在（或不在本租户）。
    pub async fn delete(&self, ctx: &RequestContext, id: &str) -> Result<u64> {
        let table = self.table();
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND id = ?"
        ))
        .bind(id.to_string());
        self.db
            .execute(ctx, &query)
            .await
            .map_err(|err| anyhow!("delete() failed: {err}"))
    }
}
