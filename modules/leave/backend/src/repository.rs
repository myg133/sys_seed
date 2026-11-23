//! 数据访问：**只**走 [`platform_db::TenantDb`] + [`TenantQuery`]。
//!
//! # 为什么没有裸 SQL 调用（`sqlx` 的 query 系列一律不许用）
//!
//! `TenantQuery` 要求语句里有**恰好一个** `{tenant}` 占位符，平台把它绑成 `$1`
//! （值只来自 `RequestContext`）。于是「忘了加租户条件」这种错误**写不出来** ——
//! 这是运行期硬校验，不是约定（AC-S-188 ②）。
//!
//! 语句构造是**纯函数**（[`list_statement`] 等），所以「占位符与绑定值是否对得上」
//! 可以脱库单测：`TenantQuery::render()` 不连库。
//!
//! 列的语义见 `migrations/*_leave_request.up.sql`；RLS policy 由
//! `cargo xtask tenant-policies` 按 manifest 的 `tenantScoped` 段生成，模块**不手写**。

use anyhow::{Result, anyhow};
use platform_core::context::RequestContext;
use platform_db::{TenantDb, TenantQuery};
use time::OffsetDateTime;

use crate::entity::{LeaveRequest, Status};

/// 请假单表名。
pub const TABLE: &str = "leave_request";

/// 读语句的列清单（与 [`LeaveRequest`] 的 `FromRow` 逐字对应）。
pub const COLUMNS: &str = "tenant_id, id, applicant_id, starts_on, ends_on, reason, status, \
                           decided_by, decided_at, created_at, updated_at";

/// 一次状态流转要写进表里的内容。
#[derive(Debug, Clone)]
pub struct StatusUpdate<'a> {
    /// 流转的目标状态。
    pub next: Status,
    /// 期望的当前状态（**乐观锁**：对不上就是有人抢先流转了）。
    pub expected: Status,
    /// 审批人；`submit` 不写这个字段时为 `None`。
    pub decided_by: Option<&'a str>,
    /// 审批时间；`submit` 时为 `None`。
    pub decided_at: Option<OffsetDateTime>,
}

/// 列表语句（按创建时间倒序；`limit` 是页大小）。
///
/// ⚠️ 本批次只做 `limit`：`cursor` 的编码形态平台侧还没有统一编解码器
/// （`List::next_cursor` 只是个字段），先不自己发明一套。
pub fn list_statement(table: &str, limit: u32) -> TenantQuery {
    let statement = format!(
        "SELECT {COLUMNS} FROM {table} WHERE tenant_id = {{tenant}} \
         ORDER BY created_at DESC, id DESC LIMIT ?"
    );
    TenantQuery::new(statement).bind(i64::from(limit))
}

/// 详情语句。
pub fn find_statement(table: &str, id: &str) -> TenantQuery {
    let statement =
        format!("SELECT {COLUMNS} FROM {table} WHERE tenant_id = {{tenant}} AND id = ?");
    TenantQuery::new(statement).bind(id.to_string())
}

/// 建单语句。
///
/// `starts_on` / `ends_on` 是 `DATE` 列：绑定值是文本，靠 `::date` 让 PostgreSQL
/// 按日期解析（`BindValue` 没有日期变体，不为一个日期去扩平台 API）。
pub fn insert_statement(table: &str, request: &LeaveRequest) -> TenantQuery {
    let statement = format!(
        "INSERT INTO {table} \
         (tenant_id, id, applicant_id, starts_on, ends_on, reason, status, \
          decided_by, decided_at, created_at, updated_at) \
         VALUES ({{tenant}}, ?, ?, ?::date, ?::date, ?, ?, ?, ?, ?, ?)"
    );
    TenantQuery::new(statement)
        .bind(request.id.clone())
        .bind(request.applicant_id.clone())
        .bind(request.starts_on.to_string())
        .bind(request.ends_on.to_string())
        .bind(request.reason.clone())
        .bind(request.status.as_str())
        .bind(request.decided_by.clone())
        .bind(request.decided_at)
        .bind(request.created_at)
        .bind(request.updated_at)
}

/// 状态流转语句（**条件更新**：`AND status = ?` 是乐观锁）。
///
/// 两个并发请求同时审批时，第二个匹配到 0 行 —— 调用方据此返回
/// `LEAVE_INVALID_TRANSITION`，而不是把后到的判定悄悄覆盖上去。
pub fn status_update_statement(
    table: &str,
    id: &str,
    update: &StatusUpdate<'_>,
    now: OffsetDateTime,
) -> TenantQuery {
    let statement = format!(
        "UPDATE {table} SET status = ?, decided_by = ?, decided_at = ?, updated_at = ? \
         WHERE tenant_id = {{tenant}} AND id = ? AND status = ?"
    );
    TenantQuery::new(statement)
        .bind(update.next.as_str())
        .bind(update.decided_by.map(str::to_string))
        .bind(update.decided_at)
        .bind(now)
        .bind(id.to_string())
        .bind(update.expected.as_str())
}

/// 租户作用域的请假单仓储。
#[derive(Debug, Clone)]
pub struct PgLeaveRepository {
    db: TenantDb,
}

impl PgLeaveRepository {
    /// 用平台数据访问层构造仓储。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 表名（带 schema 前缀；schema 来自配置，不硬编码 `public`）。
    pub fn table(&self) -> String {
        self.db.table(TABLE)
    }

    /// 列出当前租户的请假单。
    pub async fn list(&self, ctx: &RequestContext, limit: u32) -> Result<Vec<LeaveRequest>> {
        let query = list_statement(&self.table(), limit);
        self.db
            .fetch_all(ctx, &query)
            .await
            .map_err(|error| anyhow!("listing leave requests failed: {error}"))
    }

    /// 按主键查当前租户的请假单（别的租户的单子 = `None`，不泄露存在性）。
    pub async fn find(&self, ctx: &RequestContext, id: &str) -> Result<Option<LeaveRequest>> {
        let query = find_statement(&self.table(), id);
        self.db
            .fetch_optional(ctx, &query)
            .await
            .map_err(|error| anyhow!("looking up leave request {id} failed: {error}"))
    }

    /// 插入一条请假单。
    pub async fn insert(&self, ctx: &RequestContext, request: &LeaveRequest) -> Result<()> {
        let query = insert_statement(&self.table(), request);
        self.db
            .execute(ctx, &query)
            .await
            .map_err(|error| anyhow!("creating leave request {} failed: {error}", request.id))?;
        Ok(())
    }

    /// 条件更新状态；返回 `false` 表示「期望的当前状态对不上」。
    pub async fn transition(
        &self,
        ctx: &RequestContext,
        id: &str,
        update: &StatusUpdate<'_>,
        now: OffsetDateTime,
    ) -> Result<bool> {
        let query = status_update_statement(&self.table(), id, update, now);
        let affected = self
            .db
            .execute(ctx, &query)
            .await
            .map_err(|error| anyhow!("updating leave request {id} failed: {error}"))?;
        Ok(affected == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_db::BindValue;
    use time::{Date, Month};

    const QUALIFIED: &str = "public.leave_request";

    fn request() -> LeaveRequest {
        let day = Date::from_calendar_date(2026, Month::December, 1).unwrap();
        LeaveRequest::new(
            "tenant-a",
            "user_a@example.com",
            day,
            day,
            "family matters",
            OffsetDateTime::UNIX_EPOCH,
        )
    }

    /// 每条语句都必须渲染成功：恰好一个 `{tenant}`、`?` 与绑定值数量相等、
    /// 且 `$1` 恒为租户。
    fn assert_renders(query: &TenantQuery) -> (String, Vec<BindValue>) {
        let (sql, values) = query
            .render("tenant-a")
            .expect("语句必须只含一个 {tenant} 且占位符与绑定值数量相等");
        assert_eq!(
            values.first(),
            Some(&BindValue::Text("tenant-a".to_string()))
        );
        assert_eq!(values.len(), query.arg_count() + 1);
        assert!(!sql.contains('?'), "渲染后不得残留 `?`：{sql}");
        (sql, values)
    }

    #[test]
    fn list_statement_scopes_to_the_tenant_and_caps_the_page() {
        let query = list_statement(QUALIFIED, 20);
        let (sql, values) = assert_renders(&query);
        assert!(sql.contains("WHERE tenant_id = $1"), "sql = {sql}");
        assert_eq!(values[1], BindValue::Int(20));
    }

    #[test]
    fn find_statement_binds_the_id_after_the_tenant() {
        let query = find_statement(QUALIFIED, "lr-1");
        let (sql, values) = assert_renders(&query);
        assert!(sql.contains("AND id = $2"), "sql = {sql}");
        assert_eq!(values[1], BindValue::Text("lr-1".to_string()));
    }

    #[test]
    fn insert_statement_parses_dates_on_the_database_side() {
        let item = request();
        let query = insert_statement(QUALIFIED, &item);
        let (sql, values) = assert_renders(&query);
        // 日期是 DATE 列，绑定值走文本 + `::date` 让 PostgreSQL 按日期解析。
        assert_eq!(sql.matches("::date").count(), 2, "sql = {sql}");
        assert_eq!(values[1], BindValue::Text(item.id.clone()));
        assert_eq!(values[3], BindValue::Text("2026-12-01".to_string()));
        assert_eq!(values[6], BindValue::Text("draft".to_string()));
    }

    #[test]
    fn status_update_is_a_compare_and_swap() {
        let update = StatusUpdate {
            next: Status::Approved,
            expected: Status::Pending,
            decided_by: Some("user_b@example.com"),
            decided_at: Some(OffsetDateTime::UNIX_EPOCH),
        };
        let query = status_update_statement(QUALIFIED, "lr-1", &update, OffsetDateTime::UNIX_EPOCH);
        let (sql, values) = assert_renders(&query);
        // 乐观锁：`AND status = <期望状态>` 必须在，且是最后一个绑定值。
        assert!(sql.contains("AND status = $7"), "sql = {sql}");
        assert_eq!(values.last(), Some(&BindValue::Text("pending".to_string())));
        assert_eq!(
            values[2],
            BindValue::NullableText(Some("user_b@example.com".to_string())),
            "审批人是可空列（submit 不写它）→ 必须按可空类型绑定"
        );
    }
}
