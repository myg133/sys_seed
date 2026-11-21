//! 数据访问（REQ-016，design §2）。
//!
//! # 租户作用域
//!
//! 两张表都带 `tenant_id` → 语句里**必须**有 `tenant_id = {tenant}`
//! （RLS 兜底 + 应用层兜底）。
//!
//! # 事务（D148）
//!
//! ⚠️ 每一次访问都由 `TenantDb` **自己**开事务 → `set_config('app.tenant_id', …, true)`
//! → 执行 → 提交，租户上下文**只在事务内有效**。业务层拿不到裸连接。
//!
//! 需要「多步原子」的地方（UPSERT 访问记录并**回读累加结果**）由本层用
//! `TenantDb::begin` 在**同一个事务**里做完 —— 否则「写」和「读」分属两个事务，
//! 并发下读到的可能不是自己刚写的那一行。
//!
//! # `platform_oauth_client` 的读取（平台级表）
//!
//! 它是**平台级**表（无 `tenant_id`、不启用 RLS，D219）→ 沿用
//! `platform-config` / `platform-db::fx` 的**伪租户**模式：上下文租户固定为
//! [`PLATFORM_SCOPE_TENANT`]，语句里写 `{tenant} IS NOT NULL`（恒真）以满足
//! `TenantQuery` 的「必须出现租户占位符」规则。
//!
//! **只读**：本 crate 从不写 client 表（`ON DELETE CASCADE` 由数据库做，D264）。

use std::collections::BTreeMap;

use platform_core::RequestContext;
use platform_db::{BindValue, DbResult, TenantDb, TenantQuery};
use platform_oauth::model::CLIENT_TABLE;

use crate::model::{
    ACCESS_TABLE, APP_TABLE, AppAccessRow, AppRow, AccessCountRow, app_columns, status,
};

/// 访问平台级表时用的伪租户（与 `platform-config` / `platform-db::fx` 同一约定）。
pub const PLATFORM_SCOPE_TENANT: &str = "platform";

/// 待写入的应用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewApp {
    /// 应用 ID（平台生成）。
    pub id: String,
    /// 引用的 client。
    pub client_id: String,
    /// 展示名。
    pub display_name: String,
    /// 描述。
    pub description: Option<String>,
    /// 分类。
    pub category: String,
    /// 图标 URL。
    pub icon_url: Option<String>,
    /// 入口 URL。
    pub launch_url: Option<String>,
    /// 排序。
    pub sort_order: i32,
    /// 状态。
    pub status: String,
    /// 创建人。
    pub created_by: String,
}

/// 应用的可选改动（`None` = 不改）。
///
/// ⚠️ `launch_url` 的「置空」**不在这里** —— 它走专用方法
/// [`AppStore::clear_launch_url`]（D283：`Option<String>` 下 `null` 与缺键同义）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppUpdate {
    /// 新的展示名。
    pub display_name: Option<String>,
    /// 新的描述。
    pub description: Option<String>,
    /// 新的分类。
    pub category: Option<String>,
    /// 新的图标 URL。
    pub icon_url: Option<String>,
    /// 新的入口 URL。
    pub launch_url: Option<String>,
    /// 新的排序。
    pub sort_order: Option<i32>,
    /// 新的状态。
    pub status: Option<String>,
}

impl AppUpdate {
    /// 是否没有任何改动（**空 PATCH 不该发一条 UPDATE**）。
    pub fn is_empty(&self) -> bool {
        self.display_name.is_none()
            && self.description.is_none()
            && self.category.is_none()
            && self.icon_url.is_none()
            && self.launch_url.is_none()
            && self.sort_order.is_none()
            && self.status.is_none()
    }
}

/// 应用注册表的数据访问。
#[derive(Debug, Clone)]
pub struct AppStore {
    db: TenantDb,
}

impl AppStore {
    /// 用平台数据访问层构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 平台级访问用的请求上下文（伪租户）。
    fn platform_ctx() -> RequestContext {
        RequestContext::new().with_tenant(PLATFORM_SCOPE_TENANT)
    }

    // -----------------------------------------------------------------------
    // 应用（租户数据）
    // -----------------------------------------------------------------------

    /// 插入一个应用。
    pub async fn insert_app(&self, ctx: &RequestContext, app: &NewApp) -> DbResult<AppRow> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, client_id, display_name, description, category, icon_url, \
              launch_url, sort_order, status, created_at, created_by) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?, ?, ?, now(), ?) \
             RETURNING {columns}",
            table = self.db.table(APP_TABLE),
            columns = app_columns()
        ))
        .bind(app.id.clone())
        .bind(app.client_id.clone())
        .bind(app.display_name.clone())
        .bind(app.description.clone())
        .bind(app.category.clone())
        .bind(app.icon_url.clone())
        .bind(app.launch_url.clone())
        .bind(i64::from(app.sort_order))
        .bind(app.status.clone())
        .bind(app.created_by.clone());
        self.db.fetch_one(ctx, &query).await
    }

    /// 按 ID 取一个（**当前租户**；RLS + 应用层双保险）。
    pub async fn get_app(&self, ctx: &RequestContext, app_id: &str) -> DbResult<Option<AppRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.db.table(APP_TABLE),
            columns = app_columns()
        ))
        .bind(app_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 列出全部应用（**管理视角**：含 `hidden` 的；按 `(category, sort_order, display_name)` 稳定序）。
    pub async fn list_apps(&self, ctx: &RequestContext) -> DbResult<Vec<AppRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} \
             ORDER BY category, sort_order, display_name, id",
            table = self.db.table(APP_TABLE),
            columns = app_columns()
        ));
        self.db.fetch_all(ctx, &query).await
    }

    /// 列出**active** 的应用（工作台 / 导航用）。
    ///
    /// ⚠️ **不按 `launch_url` 过滤**（D279 / S-04）—— 工作台是「接入了哪些应用」的
    /// **总览**，含尚未配入口的；只有**导航**才要求 `launch_url` 非空。
    ///
    /// 排序在**服务层**再排一次（不依赖数据库返回顺序），这里给出稳定的起点。
    pub async fn list_active_apps(&self, ctx: &RequestContext) -> DbResult<Vec<AppRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {table} WHERE tenant_id = {{tenant}} AND status = ?",
            table = self.db.table(APP_TABLE),
            columns = app_columns()
        ))
        .bind(status::ACTIVE.to_string());
        self.db.fetch_all(ctx, &query).await
    }

    /// 改一个应用（**只改传了的字段**）；返回改后的行。
    ///
    /// ⚠️ 用 `COALESCE(?, 列)`：`None` 绑成 SQL `NULL` → 保留原值。
    /// 对 `launch_url` 来说这**不能**表达「置空」→ 见 [`Self::clear_launch_url`]。
    pub async fn update_app(
        &self,
        ctx: &RequestContext,
        app_id: &str,
        update: &AppUpdate,
    ) -> DbResult<Option<AppRow>> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET \
               display_name = COALESCE(?, display_name), \
               description  = COALESCE(?, description), \
               category     = COALESCE(?, category), \
               icon_url     = COALESCE(?, icon_url), \
               launch_url   = COALESCE(?, launch_url), \
               sort_order   = COALESCE(?, sort_order), \
               status       = COALESCE(?, status) \
             WHERE tenant_id = {{tenant}} AND id = ? \
             RETURNING {columns}",
            table = self.db.table(APP_TABLE),
            columns = app_columns()
        ))
        .bind(update.display_name.clone())
        .bind(update.description.clone())
        .bind(update.category.clone())
        .bind(update.icon_url.clone())
        .bind(update.launch_url.clone())
        .bind(update.sort_order.map(i64::from))
        .bind(update.status.clone())
        .bind(app_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 把 `launch_url` 置 **NULL**（**专用路径**，D283 / S-08）；返回受影响行数。
    ///
    /// ⚠️ 为什么需要它：`launch_url` 是 `Option<String>`，而 `PATCH` 请求体里
    /// `null` 与「省略键」在 serde 里**等价**（都是 `None`）→「显式置空」与「不改」
    /// **分不开**。专用端点（`DELETE /api/apps/{appId}/launch-url`）把它变成可表达的。
    pub async fn clear_launch_url(&self, ctx: &RequestContext, app_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET launch_url = NULL WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.db.table(APP_TABLE)
        ))
        .bind(app_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 删除一个应用；返回受影响行数（0 = 不存在 / 不属于本租户）。
    pub async fn delete_app(&self, ctx: &RequestContext, app_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.db.table(APP_TABLE)
        ))
        .bind(app_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 统计与某字段冲突的行数（`client_id` / `display_name` 的唯一性判定）。
    ///
    /// ⚠️ `column` 只能是白名单里的两个字面量之一 —— 它会被拼进 SQL，
    /// 所以**不接受调用方传任意字符串**。
    pub async fn count_conflicts(
        &self,
        ctx: &RequestContext,
        column: UniqueColumn,
        value: &str,
        exclude_app_id: Option<&str>,
    ) -> DbResult<i64> {
        let column = column.as_str();
        // 排除自身（PATCH 时改回原值不算冲突）。
        let (statement, binds): (String, Vec<BindValue>) = match exclude_app_id {
            Some(id) => (
                format!(
                    "SELECT count(*) AS n FROM {table} \
                     WHERE tenant_id = {{tenant}} AND {column} = ? AND id <> ?",
                    table = self.db.table(APP_TABLE)
                ),
                vec![
                    BindValue::Text(value.to_string()),
                    BindValue::Text(id.to_string()),
                ],
            ),
            None => (
                format!(
                    "SELECT count(*) AS n FROM {table} \
                     WHERE tenant_id = {{tenant}} AND {column} = ?",
                    table = self.db.table(APP_TABLE)
                ),
                vec![BindValue::Text(value.to_string())],
            ),
        };
        let mut query = TenantQuery::new(statement);
        for bind in binds {
            query = query.bind(bind);
        }
        let row: CountRow = self.db.fetch_one(ctx, &query).await?;
        Ok(row.n)
    }

    // -----------------------------------------------------------------------
    // 访问记录（租户数据）
    // -----------------------------------------------------------------------

    /// 记一次访问（**UPSERT + 累加**），返回累加后的次数。
    ///
    /// # ⚠️ `access_count` **必须累加**（D268 / design §4.3）
    ///
    /// 正确写法：`access_count = {table}.access_count + 1`
    ///
    /// ❌ 错写法：`access_count = EXCLUDED.access_count` —— `EXCLUDED` 是**本次要插入的值**（1）
    /// → 每次访问都把计数**重置成 1**，常用应用排名永远是「谁最后点谁第一」。
    ///
    /// # 为什么在整个 `RETURNING` 里回读
    ///
    /// 写与读在**同一个事务**里（`TenantDb::execute` 开事务 → 执行 → 提交）→
    /// 回读到的就是本次累加后的值，不受并发影响。
    pub async fn record_access(
        &self,
        ctx: &RequestContext,
        client_id: &str,
    ) -> DbResult<AccessCountRow> {
        let table = self.db.table(ACCESS_TABLE);
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, user_id, client_id, last_accessed_at, access_count) \
             VALUES ({{tenant}}, ?, ?, now(), 1) \
             ON CONFLICT (tenant_id, user_id, client_id) DO UPDATE \
             SET last_accessed_at = now(), \
                 access_count = {table}.access_count + 1 \
             RETURNING access_count",
            table = table
        ))
        .bind(ctx.user_id().unwrap_or_default().to_string())
        .bind(client_id.to_string());
        self.db.fetch_one(ctx, &query).await
    }

    /// 最近访问：按 `last_accessed_at DESC` 取前 N 个**在册且 active** 的应用。
    ///
    /// ⚠️ **join `platform_app`**：孤儿访问记录（client 还在、但没有应用行）
    /// **不可见**（D268 / AC-S-161 步骤 5 / TC-016-041）。
    pub async fn recent_apps(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        limit: usize,
    ) -> DbResult<Vec<AppAccessRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {app} a \
             JOIN {access} x \
               ON x.tenant_id = a.tenant_id AND x.client_id = a.client_id \
             WHERE a.tenant_id = {{tenant}} AND a.status = ? AND x.user_id = ? \
             ORDER BY x.last_accessed_at DESC, a.sort_order, a.display_name, a.id \
             LIMIT ?",
            app = self.db.table(APP_TABLE),
            access = self.db.table(ACCESS_TABLE),
            columns = prefixed_columns("a")
        ))
        .bind(status::ACTIVE.to_string())
        .bind(user_id.to_string())
        .bind(i64::try_from(limit).unwrap_or(i64::MAX));
        self.db.fetch_all(ctx, &query).await
    }

    /// 常用应用：按 `access_count DESC, last_accessed_at DESC` 取前 N 个**在册且 active** 的应用。
    pub async fn frequent_apps(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        limit: usize,
    ) -> DbResult<Vec<AppAccessRow>> {
        let query = TenantQuery::new(format!(
            "SELECT {columns} FROM {app} a \
             JOIN {access} x \
               ON x.tenant_id = a.tenant_id AND x.client_id = a.client_id \
             WHERE a.tenant_id = {{tenant}} AND a.status = ? AND x.user_id = ? \
             ORDER BY x.access_count DESC, x.last_accessed_at DESC, a.sort_order, a.display_name, a.id \
             LIMIT ?",
            app = self.db.table(APP_TABLE),
            access = self.db.table(ACCESS_TABLE),
            columns = prefixed_columns("a")
        ))
        .bind(status::ACTIVE.to_string())
        .bind(user_id.to_string())
        .bind(i64::try_from(limit).unwrap_or(i64::MAX));
        self.db.fetch_all(ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // client（平台级；**只读**）
    // -----------------------------------------------------------------------

    /// 按 client ID 取**可用性视图**（`status` + `expires_at` + `allowed_scopes`）。
    ///
    /// 只读 client 表的**三列**（`status` / `expires_at` / `allowed_scopes`）——
    /// 不取 secret 摘要、不取回调地址（本 crate 用不到，取来只会增加泄漏面）。
    pub async fn get_client_view(&self, client_id: &str) -> DbResult<Option<ClientViewRow>> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT client_id, status, expires_at, allowed_scopes \
             FROM {table} WHERE {{tenant}} IS NOT NULL AND client_id = ?",
            table = self.db.table(CLIENT_TABLE)
        ))
        .bind(client_id.to_string());
        self.db.fetch_optional(&ctx, &query).await
    }

    /// 批量取多个 client 的 `allowed_scopes`（列表 / 工作台的 **scope 投影**用）。
    ///
    /// ⚠️ 返回 `BTreeMap` —— 查不到某个 client 时**没有那个键**
    /// （调用方据此决定 `scopes` 是空数组还是报错）。
    pub async fn client_scopes(
        &self,
        client_ids: &[String],
    ) -> DbResult<BTreeMap<String, Vec<String>>> {
        if client_ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT client_id, allowed_scopes FROM {table} \
             WHERE {{tenant}} IS NOT NULL AND client_id = ANY(?)",
            table = self.db.table(CLIENT_TABLE)
        ))
        .bind(client_ids.to_vec());
        let rows: Vec<ClientScopesRow> = self.db.fetch_all(&ctx, &query).await?;
        Ok(rows
            .into_iter()
            .map(|row| (row.client_id, row.allowed_scopes))
            .collect())
    }
}

/// 唯一性判定用的列（**白名单**，拼进 SQL 前已固定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniqueColumn {
    /// `client_id`。
    ClientId,
    /// `display_name`。
    DisplayName,
}

impl UniqueColumn {
    /// 列名（**只可能是这两个字面量之一**）。
    fn as_str(self) -> &'static str {
        match self {
            Self::ClientId => "client_id",
            Self::DisplayName => "display_name",
        }
    }

    /// 面向调用方的**字段名**（camelCase，进 Problem Details 的 `params`）。
    pub fn field(self) -> &'static str {
        match self {
            Self::ClientId => "clientId",
            Self::DisplayName => "displayName",
        }
    }
}

/// client 的**可用性视图**（只读投影，**不含** secret / 回调 / 授权类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientViewRow {
    /// client ID。
    pub client_id: String,
    /// `active` / `disabled`。
    pub status: String,
    /// 过期时刻；`None` = 不过期。
    pub expires_at: Option<time::OffsetDateTime>,
    /// 允许的 scope（**唯一来源**；应用的 `scopes` 就是它的只读投影，D266）。
    pub allowed_scopes: Vec<String>,
}

platform_db::tenant_row!(ClientViewRow {
    client_id: "client_id",
    status: "status",
    expires_at: "expires_at",
    allowed_scopes: "allowed_scopes",
});

/// `client_id` + `allowed_scopes` 两列（scope 投影的批量查询用）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClientScopesRow {
    client_id: String,
    allowed_scopes: Vec<String>,
}

platform_db::tenant_row!(ClientScopesRow {
    client_id: "client_id",
    allowed_scopes: "allowed_scopes",
});

/// `count(*)` 的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
struct CountRow {
    n: i64,
}

platform_db::tenant_row!(CountRow { n: "n" });

/// 给 12 个列名加表别名前缀（join 时要区分 `a.*` 与 `x.*`）。
fn prefixed_columns(alias: &str) -> String {
    app_columns()
        .split(", ")
        .map(|column| format!("{alias}.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AppUpdate` 的空判定（空 PATCH 不该发 UPDATE）。
    #[test]
    fn an_empty_update_is_recognised() {
        assert!(AppUpdate::default().is_empty());
        assert!(
            !AppUpdate {
                sort_order: Some(1),
                ..AppUpdate::default()
            }
            .is_empty()
        );
    }

    /// 唯一性列的字段名是 camelCase（进 Problem Details 的 `params`）。
    #[test]
    fn the_unique_columns_map_to_camel_case_fields() {
        assert_eq!(UniqueColumn::ClientId.field(), "clientId");
        assert_eq!(UniqueColumn::DisplayName.field(), "displayName");
        assert_eq!(UniqueColumn::ClientId.as_str(), "client_id");
        assert_eq!(UniqueColumn::DisplayName.as_str(), "display_name");
    }

    /// join 用的列清单带表别名（**12 列一个不少**）。
    #[test]
    fn the_prefixed_column_list_has_twelve_entries() {
        let columns = prefixed_columns("a");
        let entries: Vec<&str> = columns.split(", ").collect();
        assert_eq!(entries.len(), 12, "12 列一个不少：{columns}");
        assert!(entries.iter().all(|entry| entry.starts_with("a.")));
    }
}
