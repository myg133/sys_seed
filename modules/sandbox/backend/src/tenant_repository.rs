//! 沙箱模块的**租户作用域**仓储：走 `platform-db` 的租户入口。
//!
//! # 这是业务方照着抄的标准范例
//!
//! 三条规矩，每条都对应平台的一道保险：
//!
//! 1. **租户只从 `ctx` 取** —— 实现里没有任何别的途径拿到租户；
//! 2. **每条语句都带 `{tenant}`** —— 平台强制：少了它直接编译不过（运行时报错）；
//! 3. **拿不到裸连接** —— `platform-db` 的公开 API 里没有「拿连接」的方法，
//!    所以想绕过租户条件也绕不过。
//!
//! 数据库层还有一道 RLS 兜底（policy 由平台按 manifest 的 `tenantScoped` 段统一生成，
//! 模块不手写）—— 应用层与数据库层**双保险**，任一层失效都不至于跨租户泄漏。

use anyhow::{Result, anyhow};
use platform_core::context::RequestContext;
use platform_core::extension::repository::{List, ListQuery, Repository, SortSpec};
use platform_db::{TenantDb, TenantQuery};

/// 租户作用域表名。
pub const TENANT_ITEM_TABLE: &str = "sandbox_tenant_item";

/// 允许排序的字段白名单。
pub const SORTABLE_FIELDS: [&str; 2] = ["id", "title"];

/// 租户作用域的沙箱实体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantItem {
    /// 归属租户。
    pub tenant_id: String,
    /// 主键（租户内唯一）。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 版本号：用于判断一次写操作是否真的生效。
    pub revision: i32,
}

// 手写 `FromRow`（`platform-db` 的声明宏）：不需要 sqlx 的 `macros` feature。
platform_db::tenant_row!(TenantItem {
    tenant_id: "tenant_id",
    id: "id",
    title: "title",
    revision: "revision",
});

/// 走 `platform-db` 的租户作用域仓储。
#[derive(Debug, Clone)]
pub struct PgTenantRepository {
    db: TenantDb,
}

impl PgTenantRepository {
    /// 用平台数据访问层构造仓储。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 表名（带 schema 前缀；schema 来自配置，不硬编码 `public`）。
    fn table(&self) -> String {
        self.db.table(TENANT_ITEM_TABLE)
    }

    /// 取出上下文里的租户；没有租户就是上下文不完整。
    fn tenant_of(ctx: &RequestContext) -> Result<&str> {
        ctx.tenant_id()
            .ok_or_else(|| anyhow!("the tenant is not bound in the request context"))
    }
}

impl Repository<TenantItem> for PgTenantRepository {
    type Id = String;

    async fn find(&self, ctx: &RequestContext, id: &Self::Id) -> Result<Option<TenantItem>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, title, revision FROM {table} \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table()
        ))
        .bind(id.clone());
        self.db
            .fetch_optional(ctx, &query)
            .await
            .map_err(|err| anyhow!("find() failed: {err}"))
    }

    async fn list(&self, ctx: &RequestContext, query: &ListQuery) -> Result<List<TenantItem>> {
        let mut statement = format!(
            "SELECT tenant_id, id, title, revision FROM {table} WHERE tenant_id = {{tenant}}",
            table = self.table()
        );
        let mut binds: Vec<String> = Vec::new();

        for spec in &query.filters {
            if spec.field != "title" {
                continue;
            }
            if spec.values.is_empty() {
                continue;
            }
            let placeholders = vec!["?"; spec.values.len()].join(", ");
            statement.push_str(&format!(" AND title IN ({placeholders})"));
            binds.extend(spec.values.iter().cloned());
        }

        if let Some(spec) = query.sort.first() {
            if !SORTABLE_FIELDS.contains(&spec.field.as_str()) {
                return Err(anyhow!("sort field {} is not sortable", spec.field));
            }
            let direction = if spec.descending { "DESC" } else { "ASC" };
            statement.push_str(&format!(" ORDER BY {} {direction}", spec.field));
        }
        statement.push_str(&format!(" LIMIT {}", query.effective_limit()));

        let mut tenant_query = TenantQuery::new(statement);
        for bind in binds {
            tenant_query = tenant_query.bind(bind);
        }

        let items = self
            .db
            .fetch_all(ctx, &tenant_query)
            .await
            .map_err(|err| anyhow!("list() failed: {err}"))?;
        Ok(List::new(items))
    }

    async fn insert(&self, ctx: &RequestContext, item: &TenantItem) -> Result<TenantItem> {
        let tenant = Self::tenant_of(ctx)?;
        if item.tenant_id != tenant {
            return Err(anyhow!(
                "the record tenant does not match the tenant in the request context"
            ));
        }
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, title, revision) \
             VALUES ({{tenant}}, ?, ?, ?) \
             RETURNING tenant_id, id, title, revision",
            table = self.table()
        ))
        .bind(item.id.clone())
        .bind(item.title.clone())
        .bind(item.revision);
        self.db
            .fetch_one(ctx, &query)
            .await
            .map_err(|err| anyhow!("insert() failed: {err}"))
    }

    async fn update(&self, ctx: &RequestContext, item: &TenantItem) -> Result<TenantItem> {
        let tenant = Self::tenant_of(ctx)?;
        if item.tenant_id != tenant {
            return Err(anyhow!(
                "the record tenant does not match the tenant in the request context"
            ));
        }
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET title = ?, revision = ? \
             WHERE tenant_id = {{tenant}} AND id = ? \
             RETURNING tenant_id, id, title, revision",
            table = self.table()
        ))
        .bind(item.title.clone())
        .bind(item.revision)
        .bind(item.id.clone());
        self.db
            .fetch_one(ctx, &query)
            .await
            .map_err(|err| anyhow!("update() failed: {err}"))
    }

    async fn delete(&self, ctx: &RequestContext, id: &Self::Id) -> Result<()> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table()
        ))
        .bind(id.clone());
        self.db
            .execute(ctx, &query)
            .await
            .map_err(|err| anyhow!("delete() failed: {err}"))?;
        Ok(())
    }
}

/// 排序项白名单（供契约测试与文档引用）。
pub fn is_sortable(field: &str) -> bool {
    SORTABLE_FIELDS.contains(&field)
}

/// 构造一个升序排序项（示例用法）。
pub fn ascending(field: &str) -> SortSpec {
    SortSpec::asc(field)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_whitelist_only_accepts_declared_fields() {
        assert!(is_sortable("id"));
        assert!(is_sortable("title"));
        assert!(!is_sortable("tenant_id"));
        assert!(!is_sortable("unknown"));
    }
}
