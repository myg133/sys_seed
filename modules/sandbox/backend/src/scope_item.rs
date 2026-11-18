//! 数据范围示例：实体 + **带数据范围**的仓储。
//!
//! # 这是业务方照着抄的标准范例
//!
//! 三条规矩，每条都对应平台的一道保险：
//!
//! 1. **数据范围由平台叠加** —— 业务模块**不写**任何范围条件，
//!    只把平台算好的 [`ScopeFilter`] 交给 `TenantQuery::with_scope`；
//! 2. **范围条件在 SQL 层生效** —— 不是查出来后在内存里筛；
//! 3. **没有「跳过范围」的开关** —— 平台的构造器 API 上根本没有这个入口。
//!
//! 业务模块**不知道**范围是怎么算出来的（那是平台的事），
//! 也**不能**自己实现一套（契约 C7：数据范围过滤由平台自动叠加）。

use anyhow::{Result, anyhow};
use platform_core::context::RequestContext;
use platform_core::extension::permission::ScopeFilter;
use platform_db::{DbResult, TenantDb, TenantQuery};

/// 数据范围示例表名。
pub const SCOPE_ITEM_TABLE: &str = "sandbox_scope_item";

/// 数据范围示例实体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeItem {
    /// 归属租户。
    pub tenant_id: String,
    /// 主键（租户内唯一）。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 金额（最小单位整数）。
    pub amount: i64,
    /// 归属组织节点。
    pub org_node_id: String,
    /// 创建人。
    pub created_by: String,
}

// 手写 `FromRow`（`platform-db` 的声明宏）：不需要 sqlx 的 `macros` feature。
platform_db::tenant_row!(ScopeItem {
    tenant_id: "tenant_id",
    id: "id",
    title: "title",
    amount: "amount",
    org_node_id: "org_node_id",
    created_by: "created_by",
});

/// 走 `platform-db` 的租户作用域仓储（**范围条件由平台叠加**）。
#[derive(Debug, Clone)]
pub struct PgScopeItemRepository {
    db: TenantDb,
}

impl PgScopeItemRepository {
    /// 用平台数据访问层构造仓储。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 表名（带 schema 前缀；schema 来自配置，不硬编码 `public`）。
    fn table(&self) -> String {
        self.db.table(SCOPE_ITEM_TABLE)
    }

    /// **构造带数据范围的列表语句**（公开 —— 测试要断言渲染出的 SQL 文本）。
    ///
    /// 业务模块**只**提供范围条件，数据范围本身由平台算好后传进来。
    pub fn build_list_query(&self, filter: &ScopeFilter) -> DbResult<TenantQuery> {
        let statement = format!(
            "SELECT tenant_id, id, title, amount, org_node_id, created_by FROM {table} \
             WHERE tenant_id = {{tenant}}",
            table = self.table()
        );
        TenantQuery::new(statement).with_scope(filter)
    }

    /// **构造带数据范围的详情语句**。
    pub fn build_find_query(&self, id: &str, filter: &ScopeFilter) -> DbResult<TenantQuery> {
        let statement = format!(
            "SELECT tenant_id, id, title, amount, org_node_id, created_by FROM {table} \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table()
        );
        TenantQuery::new(statement)
            .bind(id.to_string())
            .with_scope(filter)
    }

    /// 列表（**已被数据范围过滤**）。
    pub async fn list(&self, ctx: &RequestContext, filter: &ScopeFilter) -> Result<Vec<ScopeItem>> {
        let query = self.build_list_query(filter)?;
        self.db
            .fetch_all(ctx, &query)
            .await
            .map_err(|err| anyhow!("list() failed: {err}"))
    }

    /// 详情（**查不到 = 不可见**：不存在与越权是同一个结果）。
    pub async fn find(
        &self,
        ctx: &RequestContext,
        id: &str,
        filter: &ScopeFilter,
    ) -> Result<Option<ScopeItem>> {
        let query = self.build_find_query(id, filter)?;
        self.db
            .fetch_optional(ctx, &query)
            .await
            .map_err(|err| anyhow!("find() failed: {err}"))
    }
}
