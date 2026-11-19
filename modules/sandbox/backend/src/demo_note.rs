//! `demoNote` 实体：**元数据驱动渲染的第二个活样本**（REQ-012 D107）。
//!
//! # 为什么需要第二个实体
//!
//! AC-S-085（「新增同类页面**无需新增前端代码**」）是 REQ-012 的立身之本。
//! 只有一个实体时它能被「渲染器只认这一个形状」蒙混过去；有第二个之后，
//! 判据才真正成立：
//!
//! - 前端源码里出现 `demoNote` 这个键 → 有人写了特化代码（AC-S-085 会红）；
//! - 不出现 → 渲染器按元数据工作，实体键对它无意义。
//!
//! 字段组合**故意**与 `demoItem` 不同（见 `modules/sandbox/manifest.json`）：
//! 没有 `amount`（number）、没有 `dueAt`（date）、没有 `ownerId`（reference）、
//! 没有字段级权限点。若渲染器偷偷按 `demoItem` 的字段名写死，这里就会露馅。
//!
//! # 与 `demoItem` 的差异不是「复制粘贴的偷懒」
//!
//! 两个实体的**HTTP 形状完全一致**（列表信封、游标分页、投影、404 语义），
//! 差异只在列集合 —— 这正是「同类页面」的定义。共用权限点也是刻意的：
//! 同一模块的两个实体的读写语义相同，硬造 `sandbox:demo-note:read` 只会让
//! 权限表变长而不增加任何表达能力。

use anyhow::{Result, anyhow};
use platform_core::context::RequestContext;
use platform_db::{TenantDb, TenantQuery};

/// `demoNote` 的表名（与 manifest 的 `tenantScoped.tables` 一致）。
pub const DEMO_NOTE_TABLE: &str = "sandbox_demo_note";

/// `demoNote` 的持久化形态。
///
/// 列集合与元数据声明的字段**一一对应**（没有 `demoItem` 那种「故意多一列」的设计）——
/// 白名单语义已由 `demoItem.internal_note` 证明，这里不需要重复。
///
/// 序列化仍**不得**直接 `to_value(self)`：必须经
/// [`platform_core::extension::schema::SchemaProjector`]（契约 C-6 扫这个）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoNoteRecord {
    /// 归属租户。
    pub tenant_id: String,
    /// 主键（租户内唯一）。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 正文。
    pub content: String,
    /// 是否置顶。
    pub pinned: bool,
    /// 标签（逗号分隔）。
    pub tags: Option<String>,
}

// 手写 `FromRow`（`platform-db` 的声明宏）：不需要 sqlx 的 `macros` feature。
platform_db::tenant_row!(DemoNoteRecord {
    tenant_id: "tenant_id",
    id: "id",
    title: "title",
    content: "content",
    pinned: "pinned",
    tags: "tags",
});

impl DemoNoteRecord {
    /// 序列化成实体的完整形态。
    ///
    /// 返回它之后**必须**再过 [`platform_core::extension::schema::SchemaProjector::project`]。
    /// 这里的键与元数据一一对应，所以投影是**恒等**的 —— 仍然要调用：
    /// 少一次调用，C-6 的调用点就会从「有」变成「没有」，
    /// 而未来给本实体加字段级权限时会忘了补。
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "title": self.title,
            "content": self.content,
            "pinned": self.pinned,
            "tags": self.tags,
        })
    }
}

/// 走 `platform-db` 的租户作用域仓储。
#[derive(Debug, Clone)]
pub struct PgDemoNoteRepository {
    db: TenantDb,
}

impl PgDemoNoteRepository {
    /// 用平台数据访问层构造仓储。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 表名（带 schema 前缀；schema 来自配置，不硬编码 `public`）。
    fn table(&self) -> String {
        self.db.table(DEMO_NOTE_TABLE)
    }

    /// 所有列（顺序与 `tenant_row!` 一致）。
    const COLUMNS: &'static str = "tenant_id, id, title, content, pinned, tags";

    /// 列表（**游标分页**：`cursor` 是上一页最后一条的 `id`）。
    ///
    /// 按 `id` 升序；多查一条用来判断「还有没有下一页」。
    pub async fn list(
        &self,
        ctx: &RequestContext,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<DemoNoteRecord>, Option<String>)> {
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
            .fetch_all::<DemoNoteRecord>(ctx, &query)
            .await
            .map_err(|err| anyhow!("list() failed: {err}"))?;

        let has_more = rows.len() > limit as usize;
        if has_more {
            rows.truncate(limit as usize);
        }
        let next_cursor =
            has_more.then(|| rows.last().map(|row| row.id.clone()).unwrap_or_default());
        Ok((rows, next_cursor))
    }

    /// 详情（**查不到 = 不可见**：不存在与越权是同一个结果）。
    pub async fn find(&self, ctx: &RequestContext, id: &str) -> Result<Option<DemoNoteRecord>> {
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
    pub async fn insert(&self, ctx: &RequestContext, note: &DemoNoteRecord) -> Result<()> {
        let table = self.table();
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, title, content, pinned, tags) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?)"
        ))
        .bind(note.id.clone())
        .bind(note.title.clone())
        .bind(note.content.clone())
        .bind(note.pinned)
        .bind(note.tags.clone());
        self.db
            .execute(ctx, &query)
            .await
            .map_err(|err| anyhow!("insert() failed: {err}"))?;
        Ok(())
    }

    /// 更新一条；受影响行数为 0 → 记录不存在（或不在本租户）。
    pub async fn update(&self, ctx: &RequestContext, note: &DemoNoteRecord) -> Result<()> {
        let table = self.table();
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET title = ?, content = ?, pinned = ?, tags = ? \
             WHERE tenant_id = {{tenant}} AND id = ?"
        ))
        .bind(note.title.clone())
        .bind(note.content.clone())
        .bind(note.pinned)
        .bind(note.tags.clone())
        .bind(note.id.clone());
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
