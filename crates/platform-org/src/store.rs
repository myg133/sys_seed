//! 组织架构的数据访问层：走 `platform-db` 的租户作用域入口。
//!
//! # 三条规矩（每条都对应平台的一道保险）
//!
//! 1. **租户只从 `ctx` 取** —— 实现里没有任何别的途径拿到租户；
//! 2. **每条语句都带 `{tenant}`** —— 平台强制：少了它直接报错；
//! 3. **拿不到裸连接** —— `platform-db` 的公开 API 里没有「拿连接」的方法。
//!
//! 数据库层还有一道 RLS 兜底（policy 由平台按 manifest 的 `tenantScoped` 段统一生成，
//! 模块不手写）—— 应用层与数据库层**双保险**。
//!
//! # 两个入口，单份实现
//!
//! 每个操作都有两种入口：
//!
//! | 入口 | 用途 |
//! |------|------|
//! | `store.find_node(ctx, id)` | 单条语句：内部开事务 → 执行 → 提交 |
//! | `OrgStore::find_node_in(&mut tx, id)` | 跨表写：在**调用方**的事务里执行 |
//!
//! 实现只有一份（`_in` 那个），单条语句的入口是它的薄包装 ——
//! 跨表写必须在一个事务里（契约 C4），而单条语句不必。
//!
//! # 递归 CTE 的写法
//!
//! 语句里 `{tenant}` **只能出现一次**（平台强制），所以租户条件先抽成一个 CTE：
//!
//! ```sql
//! WITH RECURSIVE t AS (SELECT {tenant}::text AS tenant_id),
//!      sub AS ( ... JOIN t ON n.tenant_id = t.tenant_id ... )
//! ```
//!
//! 这样既保证「每个递归步都带租户条件」，又不用写第二遍 `{tenant}`。

use std::collections::BTreeMap;

use platform_core::RequestContext;
use platform_db::{DbError, DbResult, TenantDb, TenantQuery, TenantTx};

use crate::entity::{
    ConflictResolution, ConflictStatus, ExternalIdentity, OrgNode, OrgNodeRow, SyncConflict,
    SyncConflictRow, SyncSourceState, UserOrg, UserPosition,
};

/// 组织节点表名。
pub const ORG_NODE_TABLE: &str = "org_node";
/// 用户 ↔ 节点成员关系表名。
pub const USER_ORG_TABLE: &str = "user_org";
/// 岗位表名。
pub const USER_POSITION_TABLE: &str = "user_position";
/// 外部身份映射表名。
pub const EXTERNAL_IDENTITY_TABLE: &str = "external_identity";
/// 同步冲突表名。
pub const SYNC_CONFLICT_TABLE: &str = "sync_conflict";
/// 同步状态表名。
pub const SYNC_SOURCE_STATE_TABLE: &str = "sync_source_state";

/// 组织架构的数据访问层。
#[derive(Debug, Clone)]
pub struct OrgStore {
    db: TenantDb,
}

impl OrgStore {
    /// 用平台数据访问层构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 平台数据访问层（供上层开事务用）。
    pub fn db(&self) -> &TenantDb {
        &self.db
    }

    /// 开一个事务（跨表写必须走它 —— 契约 C4）。
    pub async fn begin(&self, ctx: &RequestContext) -> DbResult<TenantTx> {
        self.db.begin(ctx).await
    }

    // -----------------------------------------------------------------------
    // 组织节点
    // -----------------------------------------------------------------------

    /// 按 ID 查组织节点。
    pub async fn find_node(&self, ctx: &RequestContext, id: &str) -> DbResult<Option<OrgNode>> {
        let mut tx = self.db.begin(ctx).await?;
        let node = Self::find_node_in(&mut tx, id).await?;
        tx.commit().await?;
        Ok(node)
    }

    /// 在既有事务里按 ID 查组织节点。
    pub async fn find_node_in(tx: &mut TenantTx, id: &str) -> DbResult<Option<OrgNode>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, parent_id, node_type, code, name, path_labels \
             FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            table = tx_table(tx, ORG_NODE_TABLE)
        ))
        .bind(id);
        let row: Option<OrgNodeRow> = tx.fetch_optional(&query).await?;
        row.map(decode_node).transpose()
    }

    /// 按编码查组织节点（编码在租户内唯一）。
    pub async fn find_node_by_code(
        &self,
        ctx: &RequestContext,
        code: &str,
    ) -> DbResult<Option<OrgNode>> {
        let mut tx = self.db.begin(ctx).await?;
        let node = Self::find_node_by_code_in(&mut tx, code).await?;
        tx.commit().await?;
        Ok(node)
    }

    /// 在既有事务里按编码查组织节点。
    pub async fn find_node_by_code_in(tx: &mut TenantTx, code: &str) -> DbResult<Option<OrgNode>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, parent_id, node_type, code, name, path_labels \
             FROM {table} WHERE tenant_id = {{tenant}} AND code = ?",
            table = tx_table(tx, ORG_NODE_TABLE)
        ))
        .bind(code);
        let row: Option<OrgNodeRow> = tx.fetch_optional(&query).await?;
        row.map(decode_node).transpose()
    }

    /// 插入组织节点。
    pub async fn insert_node(&self, ctx: &RequestContext, node: &OrgNode) -> DbResult<()> {
        let mut tx = self.db.begin(ctx).await?;
        Self::insert_node_in(&mut tx, node).await?;
        tx.commit().await?;
        Ok(())
    }

    /// 在既有事务里插入组织节点。
    pub async fn insert_node_in(tx: &mut TenantTx, node: &OrgNode) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, parent_id, node_type, code, name, path_labels) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?)",
            table = tx_table(tx, ORG_NODE_TABLE)
        ))
        .bind(node.id.clone())
        .bind(node.parent_id.clone())
        .bind(node.node_type.as_str())
        .bind(node.code.clone())
        .bind(node.name.clone())
        .bind(crate::tree::encode(&node.path_labels));
        tx.execute(&query).await?;
        Ok(())
    }

    /// 列出组织节点（可按父节点过滤；`None` 表示全部）。
    pub async fn list_nodes(
        &self,
        ctx: &RequestContext,
        parent_id: Option<&str>,
    ) -> DbResult<Vec<OrgNode>> {
        let mut tx = self.db.begin(ctx).await?;
        let nodes = Self::list_nodes_in(&mut tx, parent_id).await?;
        tx.commit().await?;
        Ok(nodes)
    }

    /// 在既有事务里列出组织节点。
    pub async fn list_nodes_in(
        tx: &mut TenantTx,
        parent_id: Option<&str>,
    ) -> DbResult<Vec<OrgNode>> {
        let statement = match parent_id {
            Some(_) => format!(
                "SELECT tenant_id, id, parent_id, node_type, code, name, path_labels \
                 FROM {table} WHERE tenant_id = {{tenant}} AND parent_id = ? \
                 ORDER BY node_type, code",
                table = tx_table(tx, ORG_NODE_TABLE)
            ),
            None => format!(
                "SELECT tenant_id, id, parent_id, node_type, code, name, path_labels \
                 FROM {table} WHERE tenant_id = {{tenant}} ORDER BY node_type, code",
                table = tx_table(tx, ORG_NODE_TABLE)
            ),
        };
        let mut query = TenantQuery::new(statement);
        if let Some(parent) = parent_id {
            query = query.bind(parent.to_string());
        }
        let rows: Vec<OrgNodeRow> = tx.fetch_all(&query).await?;
        rows.into_iter().map(decode_node).collect()
    }

    /// 列出租户内的**全部**组织节点（重建 `path_labels` 时用）。
    pub async fn all_nodes(&self, ctx: &RequestContext) -> DbResult<Vec<OrgNode>> {
        self.list_nodes(ctx, None).await
    }

    /// 更新节点的父节点、名称与祖先链。
    pub async fn update_node(
        &self,
        ctx: &RequestContext,
        id: &str,
        parent_id: Option<&str>,
        name: &str,
        path_labels: &[String],
    ) -> DbResult<u64> {
        let mut tx = self.db.begin(ctx).await?;
        let affected = Self::update_node_in(&mut tx, id, parent_id, name, path_labels).await?;
        tx.commit().await?;
        Ok(affected)
    }

    /// 在既有事务里更新节点的父节点、名称与祖先链。
    pub async fn update_node_in(
        tx: &mut TenantTx,
        id: &str,
        parent_id: Option<&str>,
        name: &str,
        path_labels: &[String],
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET parent_id = ?, name = ?, path_labels = ? \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = tx_table(tx, ORG_NODE_TABLE)
        ))
        .bind(parent_id.map(str::to_string))
        .bind(name.to_string())
        .bind(crate::tree::encode(path_labels))
        .bind(id.to_string());
        tx.execute(&query).await
    }

    /// 只更新节点的祖先链（移动子树后逐个重算）。
    pub async fn update_node_path(
        &self,
        ctx: &RequestContext,
        id: &str,
        path_labels: &[String],
    ) -> DbResult<u64> {
        let mut tx = self.db.begin(ctx).await?;
        let affected = Self::update_node_path_in(&mut tx, id, path_labels).await?;
        tx.commit().await?;
        Ok(affected)
    }

    /// 在既有事务里只更新节点的祖先链。
    pub async fn update_node_path_in(
        tx: &mut TenantTx,
        id: &str,
        path_labels: &[String],
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET path_labels = ? WHERE tenant_id = {{tenant}} AND id = ?",
            table = tx_table(tx, ORG_NODE_TABLE)
        ))
        .bind(crate::tree::encode(path_labels))
        .bind(id.to_string());
        tx.execute(&query).await
    }

    /// 删除组织节点（调用方负责先处理子节点与成员）。
    pub async fn delete_node(&self, ctx: &RequestContext, id: &str) -> DbResult<u64> {
        let mut tx = self.db.begin(ctx).await?;
        let affected = Self::delete_node_in(&mut tx, id).await?;
        tx.commit().await?;
        Ok(affected)
    }

    /// 在既有事务里删除组织节点。
    pub async fn delete_node_in(tx: &mut TenantTx, id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            table = tx_table(tx, ORG_NODE_TABLE)
        ))
        .bind(id.to_string());
        tx.execute(&query).await
    }

    /// 查某个节点的**子树**（递归 CTE，D1）。
    ///
    /// 返回结果按「深度 → 编码」排序：根在前，同层按编码稳定排序。
    pub async fn subtree(&self, ctx: &RequestContext, root_id: &str) -> DbResult<Vec<OrgNode>> {
        let mut tx = self.db.begin(ctx).await?;
        let nodes = Self::subtree_in(&mut tx, root_id).await?;
        tx.commit().await?;
        Ok(nodes)
    }

    /// 在既有事务里查子树（递归 CTE）。
    pub async fn subtree_in(tx: &mut TenantTx, root_id: &str) -> DbResult<Vec<OrgNode>> {
        let query = TenantQuery::new(format!(
            "WITH RECURSIVE t AS (SELECT {{tenant}}::text AS tenant_id), \
             sub AS ( \
               SELECT n.tenant_id, n.id, n.parent_id, n.node_type, n.code, n.name, n.path_labels, \
                      1 AS depth \
                 FROM {table} n JOIN t ON n.tenant_id = t.tenant_id \
                WHERE n.id = ? \
               UNION ALL \
               SELECT c.tenant_id, c.id, c.parent_id, c.node_type, c.code, c.name, c.path_labels, \
                      s.depth + 1 \
                 FROM {table} c \
                 JOIN sub s ON c.parent_id = s.id \
                 JOIN t ON c.tenant_id = t.tenant_id \
             ) \
             SELECT tenant_id, id, parent_id, node_type, code, name, path_labels \
               FROM sub ORDER BY depth, code",
            table = tx_table(tx, ORG_NODE_TABLE)
        ))
        .bind(root_id.to_string());
        let rows: Vec<OrgNodeRow> = tx.fetch_all(&query).await?;
        rows.into_iter().map(decode_node).collect()
    }

    /// 子节点数量。
    pub async fn count_children(&self, ctx: &RequestContext, id: &str) -> DbResult<i64> {
        let mut tx = self.db.begin(ctx).await?;
        let count = Self::count_children_in(&mut tx, id).await?;
        tx.commit().await?;
        Ok(count)
    }

    /// 在既有事务里数子节点。
    pub async fn count_children_in(tx: &mut TenantTx, id: &str) -> DbResult<i64> {
        let query = TenantQuery::new(format!(
            "SELECT count(*) AS total FROM {table} WHERE tenant_id = {{tenant}} AND parent_id = ?",
            table = tx_table(tx, ORG_NODE_TABLE)
        ))
        .bind(id.to_string());
        let row: CountRow = tx.fetch_one(&query).await?;
        Ok(row.total)
    }

    // -----------------------------------------------------------------------
    // 成员关系
    // -----------------------------------------------------------------------

    /// 插入成员关系。
    pub async fn insert_user_org(&self, ctx: &RequestContext, item: &UserOrg) -> DbResult<()> {
        let mut tx = self.db.begin(ctx).await?;
        Self::insert_user_org_in(&mut tx, item).await?;
        tx.commit().await?;
        Ok(())
    }

    /// 在既有事务里插入成员关系。
    pub async fn insert_user_org_in(tx: &mut TenantTx, item: &UserOrg) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, user_id, org_node_id, role, is_primary) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?)",
            table = tx_table(tx, USER_ORG_TABLE)
        ))
        .bind(item.id.clone())
        .bind(item.user_id.clone())
        .bind(item.org_node_id.clone())
        .bind(item.role.clone())
        .bind(item.is_primary);
        tx.execute(&query).await?;
        Ok(())
    }

    /// 按 ID 查成员关系。
    pub async fn find_user_org(&self, ctx: &RequestContext, id: &str) -> DbResult<Option<UserOrg>> {
        let mut tx = self.db.begin(ctx).await?;
        let item = Self::find_user_org_in(&mut tx, id).await?;
        tx.commit().await?;
        Ok(item)
    }

    /// 在既有事务里按 ID 查成员关系。
    pub async fn find_user_org_in(tx: &mut TenantTx, id: &str) -> DbResult<Option<UserOrg>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, user_id, org_node_id, role, is_primary \
             FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            table = tx_table(tx, USER_ORG_TABLE)
        ))
        .bind(id.to_string());
        let row: Option<UserOrg> = tx.fetch_optional(&query).await?;
        Ok(row)
    }

    /// 列出成员关系（可按用户 / 节点过滤）。
    pub async fn list_user_org(
        &self,
        ctx: &RequestContext,
        user_id: Option<&str>,
        org_node_id: Option<&str>,
    ) -> DbResult<Vec<UserOrg>> {
        let mut tx = self.db.begin(ctx).await?;
        let items = Self::list_user_org_in(&mut tx, user_id, org_node_id).await?;
        tx.commit().await?;
        Ok(items)
    }

    /// 在既有事务里列出成员关系。
    pub async fn list_user_org_in(
        tx: &mut TenantTx,
        user_id: Option<&str>,
        org_node_id: Option<&str>,
    ) -> DbResult<Vec<UserOrg>> {
        let mut filters: Vec<String> = Vec::new();
        if user_id.is_some() {
            filters.push("user_id = ?".to_string());
        }
        if org_node_id.is_some() {
            filters.push("org_node_id = ?".to_string());
        }
        let mut statement = format!(
            "SELECT tenant_id, id, user_id, org_node_id, role, is_primary \
             FROM {table} WHERE tenant_id = {{tenant}}",
            table = tx_table(tx, USER_ORG_TABLE)
        );
        if !filters.is_empty() {
            statement.push_str(&format!(" AND {}", filters.join(" AND ")));
        }
        statement.push_str(" ORDER BY user_id, org_node_id");

        let mut query = TenantQuery::new(statement);
        if let Some(user) = user_id {
            query = query.bind(user.to_string());
        }
        if let Some(node) = org_node_id {
            query = query.bind(node.to_string());
        }
        let rows: Vec<UserOrg> = tx.fetch_all(&query).await?;
        Ok(rows)
    }

    /// 列出**一组节点**的成员（子树成员查询用）。
    pub async fn members_of_nodes(
        &self,
        ctx: &RequestContext,
        node_ids: &[String],
    ) -> DbResult<Vec<UserOrg>> {
        let mut tx = self.db.begin(ctx).await?;
        let items = Self::members_of_nodes_in(&mut tx, node_ids).await?;
        tx.commit().await?;
        Ok(items)
    }

    /// 在既有事务里列出**一组节点**的成员。
    pub async fn members_of_nodes_in(
        tx: &mut TenantTx,
        node_ids: &[String],
    ) -> DbResult<Vec<UserOrg>> {
        if node_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = vec!["?"; node_ids.len()].join(", ");
        let statement = format!(
            "SELECT tenant_id, id, user_id, org_node_id, role, is_primary \
             FROM {table} WHERE tenant_id = {{tenant}} AND org_node_id IN ({placeholders}) \
             ORDER BY user_id, org_node_id",
            table = tx_table(tx, USER_ORG_TABLE)
        );
        let mut query = TenantQuery::new(statement);
        for id in node_ids {
            query = query.bind(id.clone());
        }
        let rows: Vec<UserOrg> = tx.fetch_all(&query).await?;
        Ok(rows)
    }

    /// 删除成员关系。
    pub async fn delete_user_org(&self, ctx: &RequestContext, id: &str) -> DbResult<u64> {
        let mut tx = self.db.begin(ctx).await?;
        let affected = Self::delete_user_org_in(&mut tx, id).await?;
        tx.commit().await?;
        Ok(affected)
    }

    /// 在既有事务里删除成员关系。
    pub async fn delete_user_org_in(tx: &mut TenantTx, id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            table = tx_table(tx, USER_ORG_TABLE)
        ))
        .bind(id.to_string());
        tx.execute(&query).await
    }

    /// 清掉某用户的**其它**主部门关系（保证一个用户只有一个主部门）。
    pub async fn clear_primary_membership(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        keep_id: &str,
    ) -> DbResult<u64> {
        let mut tx = self.db.begin(ctx).await?;
        let affected = Self::clear_primary_membership_in(&mut tx, user_id, keep_id).await?;
        tx.commit().await?;
        Ok(affected)
    }

    /// 在既有事务里清掉某用户的其它主部门关系。
    pub async fn clear_primary_membership_in(
        tx: &mut TenantTx,
        user_id: &str,
        keep_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET is_primary = FALSE \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND id <> ? AND is_primary",
            table = tx_table(tx, USER_ORG_TABLE)
        ))
        .bind(user_id.to_string())
        .bind(keep_id.to_string());
        tx.execute(&query).await
    }

    /// 删除某用户的**全部**成员关系（离职解绑用）。
    pub async fn delete_user_org_by_user(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<u64> {
        let mut tx = self.db.begin(ctx).await?;
        let affected = Self::delete_user_org_by_user_in(&mut tx, user_id).await?;
        tx.commit().await?;
        Ok(affected)
    }

    /// 在既有事务里删除某用户的全部成员关系。
    pub async fn delete_user_org_by_user_in(tx: &mut TenantTx, user_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND user_id = ?",
            table = tx_table(tx, USER_ORG_TABLE)
        ))
        .bind(user_id.to_string());
        tx.execute(&query).await
    }

    /// 某用户的成员关系条数。
    pub async fn count_user_org(&self, ctx: &RequestContext, user_id: &str) -> DbResult<i64> {
        let mut tx = self.db.begin(ctx).await?;
        let query = TenantQuery::new(format!(
            "SELECT count(*) AS total FROM {table} WHERE tenant_id = {{tenant}} AND user_id = ?",
            table = tx_table(&tx, USER_ORG_TABLE)
        ))
        .bind(user_id.to_string());
        let row: CountRow = tx.fetch_one(&query).await?;
        tx.commit().await?;
        Ok(row.total)
    }

    // -----------------------------------------------------------------------
    // 岗位
    // -----------------------------------------------------------------------

    /// 插入岗位。
    pub async fn insert_user_position(
        &self,
        ctx: &RequestContext,
        item: &UserPosition,
    ) -> DbResult<()> {
        let mut tx = self.db.begin(ctx).await?;
        Self::insert_user_position_in(&mut tx, item).await?;
        tx.commit().await?;
        Ok(())
    }

    /// 在既有事务里插入岗位。
    pub async fn insert_user_position_in(tx: &mut TenantTx, item: &UserPosition) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, user_id, position_code, org_node_id) \
             VALUES ({{tenant}}, ?, ?, ?, ?)",
            table = tx_table(tx, USER_POSITION_TABLE)
        ))
        .bind(item.id.clone())
        .bind(item.user_id.clone())
        .bind(item.position_code.clone())
        .bind(item.org_node_id.clone());
        tx.execute(&query).await?;
        Ok(())
    }

    /// 列出岗位（可按用户过滤）。
    pub async fn list_user_position(
        &self,
        ctx: &RequestContext,
        user_id: Option<&str>,
    ) -> DbResult<Vec<UserPosition>> {
        let mut tx = self.db.begin(ctx).await?;
        let statement = match user_id {
            Some(_) => format!(
                "SELECT tenant_id, id, user_id, position_code, org_node_id \
                 FROM {table} WHERE tenant_id = {{tenant}} AND user_id = ? ORDER BY position_code",
                table = tx_table(&tx, USER_POSITION_TABLE)
            ),
            None => format!(
                "SELECT tenant_id, id, user_id, position_code, org_node_id \
                 FROM {table} WHERE tenant_id = {{tenant}} ORDER BY user_id, position_code",
                table = tx_table(&tx, USER_POSITION_TABLE)
            ),
        };
        let mut query = TenantQuery::new(statement);
        if let Some(user) = user_id {
            query = query.bind(user.to_string());
        }
        let rows: Vec<UserPosition> = tx.fetch_all(&query).await?;
        tx.commit().await?;
        Ok(rows)
    }

    /// 删除某用户的**全部**岗位（离职解绑用）。
    pub async fn delete_user_position_by_user(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<u64> {
        let mut tx = self.db.begin(ctx).await?;
        let affected = Self::delete_user_position_by_user_in(&mut tx, user_id).await?;
        tx.commit().await?;
        Ok(affected)
    }

    /// 在既有事务里删除某用户的全部岗位。
    pub async fn delete_user_position_by_user_in(
        tx: &mut TenantTx,
        user_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND user_id = ?",
            table = tx_table(tx, USER_POSITION_TABLE)
        ))
        .bind(user_id.to_string());
        tx.execute(&query).await
    }

    // -----------------------------------------------------------------------
    // 外部身份
    // -----------------------------------------------------------------------

    /// 写入外部身份映射（幂等：同 `(provider, external_id)` 覆盖）。
    pub async fn upsert_external_identity(
        &self,
        ctx: &RequestContext,
        item: &ExternalIdentity,
    ) -> DbResult<()> {
        let mut tx = self.db.begin(ctx).await?;
        Self::upsert_external_identity_in(&mut tx, item).await?;
        tx.commit().await?;
        Ok(())
    }

    /// 在既有事务里写入外部身份映射。
    pub async fn upsert_external_identity_in(
        tx: &mut TenantTx,
        item: &ExternalIdentity,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, provider, external_id, user_id, last_synced_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?) \
             ON CONFLICT (tenant_id, provider, external_id) \
             DO UPDATE SET user_id = EXCLUDED.user_id, last_synced_at = EXCLUDED.last_synced_at",
            table = tx_table(tx, EXTERNAL_IDENTITY_TABLE)
        ))
        .bind(item.id.clone())
        .bind(item.provider.clone())
        .bind(item.external_id.clone())
        .bind(item.user_id.clone())
        .bind(item.last_synced_at);
        tx.execute(&query).await?;
        Ok(())
    }

    /// 按 `(provider, external_id)` 查外部身份。
    pub async fn find_external_identity(
        &self,
        ctx: &RequestContext,
        provider: &str,
        external_id: &str,
    ) -> DbResult<Option<ExternalIdentity>> {
        let mut tx = self.db.begin(ctx).await?;
        let item = Self::find_external_identity_in(&mut tx, provider, external_id).await?;
        tx.commit().await?;
        Ok(item)
    }

    /// 在既有事务里按 `(provider, external_id)` 查外部身份。
    pub async fn find_external_identity_in(
        tx: &mut TenantTx,
        provider: &str,
        external_id: &str,
    ) -> DbResult<Option<ExternalIdentity>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, provider, external_id, user_id, last_synced_at \
             FROM {table} WHERE tenant_id = {{tenant}} AND provider = ? AND external_id = ?",
            table = tx_table(tx, EXTERNAL_IDENTITY_TABLE)
        ))
        .bind(provider.to_string())
        .bind(external_id.to_string());
        let row: Option<ExternalIdentity> = tx.fetch_optional(&query).await?;
        Ok(row)
    }

    // -----------------------------------------------------------------------
    // 同步冲突
    // -----------------------------------------------------------------------

    /// 写入冲突（字段级）。
    ///
    /// 同一个 `(source, external_id, field)` 只保留一条 —— 重复同步不会堆出一堆同样的冲突。
    pub async fn upsert_conflict(&self, ctx: &RequestContext, item: &SyncConflict) -> DbResult<()> {
        let mut tx = self.db.begin(ctx).await?;
        Self::upsert_conflict_in(&mut tx, item).await?;
        tx.commit().await?;
        Ok(())
    }

    /// 在既有事务里写入冲突（字段级）。
    pub async fn upsert_conflict_in(tx: &mut TenantTx, item: &SyncConflict) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, source, external_id, field, local_value, remote_value, status) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (tenant_id, source, external_id, field) \
             DO UPDATE SET local_value = EXCLUDED.local_value, \
                           remote_value = EXCLUDED.remote_value, \
                           status = EXCLUDED.status",
            table = tx_table(tx, SYNC_CONFLICT_TABLE)
        ))
        .bind(item.id.clone())
        .bind(item.source.clone())
        .bind(item.external_id.clone())
        .bind(item.field.clone())
        .bind(item.local_value.clone())
        .bind(item.remote_value.clone())
        .bind(item.status.as_str());
        tx.execute(&query).await?;
        Ok(())
    }

    /// 按 ID 查冲突。
    pub async fn find_conflict(
        &self,
        ctx: &RequestContext,
        id: &str,
    ) -> DbResult<Option<SyncConflict>> {
        let mut tx = self.db.begin(ctx).await?;
        let conflict = Self::find_conflict_in(&mut tx, id).await?;
        tx.commit().await?;
        Ok(conflict)
    }

    /// 在既有事务里按 ID 查冲突。
    pub async fn find_conflict_in(tx: &mut TenantTx, id: &str) -> DbResult<Option<SyncConflict>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, source, external_id, field, local_value, remote_value, \
             status, resolution, resolved_by, resolved_at \
             FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            table = tx_table(tx, SYNC_CONFLICT_TABLE)
        ))
        .bind(id.to_string());
        let row: Option<SyncConflictRow> = tx.fetch_optional(&query).await?;
        row.map(decode_conflict).transpose()
    }

    /// 列出冲突（可按状态过滤）。
    pub async fn list_conflicts(
        &self,
        ctx: &RequestContext,
        status: Option<ConflictStatus>,
    ) -> DbResult<Vec<SyncConflict>> {
        let mut tx = self.db.begin(ctx).await?;
        let conflicts = Self::list_conflicts_in(&mut tx, status).await?;
        tx.commit().await?;
        Ok(conflicts)
    }

    /// 在既有事务里列出冲突。
    pub async fn list_conflicts_in(
        tx: &mut TenantTx,
        status: Option<ConflictStatus>,
    ) -> DbResult<Vec<SyncConflict>> {
        let statement = match status {
            Some(_) => format!(
                "SELECT tenant_id, id, source, external_id, field, local_value, remote_value, \
                 status, resolution, resolved_by, resolved_at \
                 FROM {table} WHERE tenant_id = {{tenant}} AND status = ? \
                 ORDER BY source, external_id, field",
                table = tx_table(tx, SYNC_CONFLICT_TABLE)
            ),
            None => format!(
                "SELECT tenant_id, id, source, external_id, field, local_value, remote_value, \
                 status, resolution, resolved_by, resolved_at \
                 FROM {table} WHERE tenant_id = {{tenant}} \
                 ORDER BY source, external_id, field",
                table = tx_table(tx, SYNC_CONFLICT_TABLE)
            ),
        };
        let mut query = TenantQuery::new(statement);
        if let Some(state) = status {
            query = query.bind(state.as_str());
        }
        let rows: Vec<SyncConflictRow> = tx.fetch_all(&query).await?;
        rows.into_iter().map(decode_conflict).collect()
    }

    /// 记录裁决结果。
    pub async fn resolve_conflict(
        &self,
        ctx: &RequestContext,
        id: &str,
        resolution: ConflictResolution,
        resolved_by: &str,
        resolved_at: time::OffsetDateTime,
    ) -> DbResult<u64> {
        let mut tx = self.db.begin(ctx).await?;
        let affected =
            Self::resolve_conflict_in(&mut tx, id, resolution, resolved_by, resolved_at).await?;
        tx.commit().await?;
        Ok(affected)
    }

    /// 在既有事务里记录裁决结果。
    pub async fn resolve_conflict_in(
        tx: &mut TenantTx,
        id: &str,
        resolution: ConflictResolution,
        resolved_by: &str,
        resolved_at: time::OffsetDateTime,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = ?, resolution = ?, resolved_by = ?, resolved_at = ? \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = tx_table(tx, SYNC_CONFLICT_TABLE)
        ))
        .bind(ConflictStatus::Resolved.as_str())
        .bind(resolution.as_str())
        .bind(resolved_by.to_string())
        .bind(resolved_at)
        .bind(id.to_string());
        tx.execute(&query).await
    }

    // -----------------------------------------------------------------------
    // 同步状态
    // -----------------------------------------------------------------------

    /// 写入同步状态（幂等）。
    pub async fn upsert_sync_state(
        &self,
        ctx: &RequestContext,
        item: &SyncSourceState,
    ) -> DbResult<()> {
        let mut tx = self.db.begin(ctx).await?;
        Self::upsert_sync_state_in(&mut tx, item).await?;
        tx.commit().await?;
        Ok(())
    }

    /// 记录一次**同步失败**（错误计数递增 + 写入错误摘要）。
    ///
    /// `last_sync_at`（上次**成功**同步的时间）**保持不变** ——
    /// 它是增量同步的起点（`service.rs::sync_ldap` 拿它拼过滤器），
    /// 失败时往前挪会**漏掉**失败期间变更的条目。
    ///
    /// 成功路径走 [`OrgStore::upsert_sync_state`]：计数**清零**、错误摘要**清空**。
    pub async fn record_sync_failure(
        &self,
        ctx: &RequestContext,
        source: &str,
        reason: &str,
    ) -> DbResult<()> {
        let mut tx = self.db.begin(ctx).await?;
        Self::record_sync_failure_in(&mut tx, source, reason).await?;
        tx.commit().await?;
        Ok(())
    }

    /// 在既有事务里记录一次同步失败。
    pub async fn record_sync_failure_in(
        tx: &mut TenantTx,
        source: &str,
        reason: &str,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, source, last_sync_at, last_error, error_count) \
             VALUES ({{tenant}}, ?, NULL, ?, 1) \
             ON CONFLICT (tenant_id, source) \
             DO UPDATE SET last_error = EXCLUDED.last_error, \
                           error_count = {table}.error_count + 1",
            table = tx_table(tx, SYNC_SOURCE_STATE_TABLE)
        ))
        .bind(source.to_string())
        .bind(reason.to_string());
        tx.execute(&query).await?;
        Ok(())
    }

    /// 在既有事务里写入同步状态。
    pub async fn upsert_sync_state_in(tx: &mut TenantTx, item: &SyncSourceState) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, source, last_sync_at, last_error, error_count) \
             VALUES ({{tenant}}, ?, ?, ?, ?) \
             ON CONFLICT (tenant_id, source) \
             DO UPDATE SET last_sync_at = EXCLUDED.last_sync_at, \
                           last_error = EXCLUDED.last_error, \
                           error_count = EXCLUDED.error_count",
            table = tx_table(tx, SYNC_SOURCE_STATE_TABLE)
        ))
        .bind(item.source.clone())
        .bind(item.last_sync_at)
        .bind(item.last_error.clone())
        .bind(item.error_count);
        tx.execute(&query).await?;
        Ok(())
    }

    /// 读同步状态。
    pub async fn get_sync_state(
        &self,
        ctx: &RequestContext,
        source: &str,
    ) -> DbResult<Option<SyncSourceState>> {
        let mut tx = self.db.begin(ctx).await?;
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, source, last_sync_at, last_error, error_count \
             FROM {table} WHERE tenant_id = {{tenant}} AND source = ?",
            table = tx_table(&tx, SYNC_SOURCE_STATE_TABLE)
        ))
        .bind(source.to_string());
        let row: Option<SyncSourceState> = tx.fetch_optional(&query).await?;
        tx.commit().await?;
        Ok(row)
    }
}

/// `count(*)` 的行类型。
struct CountRow {
    total: i64,
}

platform_db::tenant_row!(CountRow { total: "total" });

/// 事务里的表名（带 schema 前缀 —— schema 来自配置，不硬编码 `public`）。
fn tx_table(tx: &TenantTx, name: &str) -> String {
    format!("{}.{}", tx.schema(), name)
}

/// 行 → 实体（`path_labels` 在库里是 JSON 文本，实体里是 `Vec<String>`）。
///
/// 库里存了**未知**的节点类型 / 冲突状态时**报错**，不静默兜底成某个默认值 ——
/// 静默兜底会把「数据被改坏了」变成「看起来正常但结果错了」。
fn decode_node(row: OrgNodeRow) -> DbResult<OrgNode> {
    OrgNode::try_from_row(row).ok_or_else(|| {
        DbError::Database(platform_db::SqlxError::Protocol(
            "the organization node row has an unknown node type".to_string(),
        ))
    })
}

/// 行 → 冲突实体。
fn decode_conflict(row: SyncConflictRow) -> DbResult<SyncConflict> {
    SyncConflict::try_from_row(row).ok_or_else(|| {
        DbError::Database(platform_db::SqlxError::Protocol(
            "the sync conflict row has an unknown status".to_string(),
        ))
    })
}

/// 节点 ID → 父节点 ID 的映射（重建 `path_labels` 时用）。
pub fn parent_links(nodes: &[OrgNode]) -> BTreeMap<String, Option<String>> {
    nodes
        .iter()
        .map(|node| (node.id.clone(), node.parent_id.clone()))
        .collect()
}

/// 节点 ID → 已存祖先链的映射（重建 `path_labels` 时用）。
pub fn stored_paths(nodes: &[OrgNode]) -> BTreeMap<String, Vec<String>> {
    nodes
        .iter()
        .map(|node| (node.id.clone(), node.path_labels.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_links_keep_missing_parents_as_none() {
        let nodes = vec![
            OrgNode {
                tenant_id: "t".to_string(),
                id: "root".to_string(),
                parent_id: None,
                node_type: crate::entity::NodeType::Company,
                code: "c".to_string(),
                name: "c".to_string(),
                path_labels: vec!["root".to_string()],
            },
            OrgNode {
                tenant_id: "t".to_string(),
                id: "dept".to_string(),
                parent_id: Some("root".to_string()),
                node_type: crate::entity::NodeType::Dept,
                code: "d".to_string(),
                name: "d".to_string(),
                path_labels: vec!["root".to_string(), "dept".to_string()],
            },
        ];
        let links = parent_links(&nodes);
        assert_eq!(links.get("root"), Some(&None));
        assert_eq!(links.get("dept"), Some(&Some("root".to_string())));
    }

    #[test]
    fn stored_paths_are_kept_as_they_are() {
        let nodes = vec![OrgNode {
            tenant_id: "t".to_string(),
            id: "root".to_string(),
            parent_id: None,
            node_type: crate::entity::NodeType::Company,
            code: "c".to_string(),
            name: "c".to_string(),
            path_labels: vec!["root".to_string()],
        }];
        let paths = stored_paths(&nodes);
        assert_eq!(paths.get("root"), Some(&vec!["root".to_string()]));
    }
}
