//! 租户作用域的数据访问入口。
//!
//! # 公开 API 表面上拿不到裸连接
//!
//! [`TenantDb`] 的公开方法**全部**要带 [`RequestContext`]：
//!
//! | 方法 | 形态 | 说明 |
//! |------|------|------|
//! | `fetch_all` / `fetch_optional` / `fetch_one` / `execute` | 单次操作 | 内部开事务 → 设租户 → 执行 → 提交 |
//! | `begin` | 多操作 | 拿到 [`TenantTx`]，在同一个事务里做多件事 |
//!
//! 裸连接（`PgPool` / `PgConnection` / `Transaction`）**不出现在任何公开签名里** ——
//! 业务模块拿不到它，也就绕不过租户条件。
//!
//! # 为什么是 `SET LOCAL` 而不是 `SET`
//!
//! 连接池会把连接**借出去再还回来**。租户上下文跟着连接走，就有两个坑：
//!
//! | 写法 | 连接归还连接池后 | 后果 |
//! |------|------------------|------|
//! | `SET app.tenant_id = 'a'` | **仍然带着 `a`** | 下一个请求借到这条连接时，租户是上一个租户 → **串租户** |
//! | `SET LOCAL app.tenant_id = 'a'` | 自动清除 | 连接归还时是干净的 |
//!
//! `SET LOCAL` 只在**当前事务**内有效，事务结束（提交或回滚）就自动失效。
//! 因此本层的每一次访问都在**显式事务**里执行，并在事务内设置租户：
//!
//! ```sql
//! SELECT set_config('app.tenant_id', $1, true);   -- true = 事务局部（等价于 SET LOCAL）
//! ```
//!
//! 为什么用 `set_config(..., true)` 而不是字面的 `SET LOCAL app.tenant_id = $1`：
//! **`SET` 命令不接受绑定参数**（`$1` 在 `SET` 里不是参数而是语法错误），
//! 而 `set_config` 的第三个参数 `is_local => true` 与 `SET LOCAL` **语义完全一致**
//! （事务结束即失效），并且能安全地走绑定参数 —— 租户 ID 不需要拼进 SQL。
//!
//! 这条「事务局部」的要求是本层**最容易被做错、后果最严重**的一处：
//! 一旦用了普通 `SET`，串租户这类 bug 极难复现（取决于连接池的调度顺序）。

use std::str::FromStr;
use std::time::Duration;

use platform_core::RequestContext;
use sqlx::postgres::{PgArguments, PgConnectOptions, PgPool, PgPoolOptions, PgRow, Postgres};
use sqlx::query::{Query, QueryAs};
use sqlx::{FromRow, Transaction};

use crate::error::{DbError, DbResult};
use crate::query::{BindValue, TenantQuery};

/// 连接池默认上限。低配机与私有化单机场景够用。
pub const DEFAULT_MAX_CONNECTIONS: u32 = 5;

/// 默认 schema 名。
///
/// **不把 L1 的假设写死**：schema 从配置来（将来切 L2「独立 schema」时改配置即可）。
pub const DEFAULT_SCHEMA: &str = "public";

/// 连接池**获取连接**的超时。
///
/// 与 `platform-server` 的探针超时无关（那边是 2s，为了让 readiness 的 503 语义成立）；
/// 这里管的是业务数据访问等不到连接的场景。
pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// 数据访问层配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbOptions {
    /// 连接池上限。
    pub max_connections: u32,
    /// 表所在的 schema 名（不硬编码 `public`）。
    pub schema: String,
}

impl Default for DbOptions {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_MAX_CONNECTIONS,
            schema: DEFAULT_SCHEMA.to_string(),
        }
    }
}

/// 租户作用域的数据访问入口。
///
/// 业务模块**只**通过它访问数据；它不暴露裸连接。
#[derive(Debug, Clone)]
pub struct TenantDb {
    /// 连接池（**不公开** —— 公开了就等于把「不带租户的连接」交给业务模块）。
    pub(crate) pool: PgPool,
    schema: String,
}

impl TenantDb {
    /// 按连接串建连接池。
    ///
    /// 用 `connect_lazy_with`：进程启动不依赖数据库是否已就绪。
    pub async fn connect(database_url: &str, options: DbOptions) -> DbResult<Self> {
        let connect_options = PgConnectOptions::from_str(database_url)?;
        let pool = PgPoolOptions::new()
            .max_connections(options.max_connections)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect_lazy_with(connect_options);
        Ok(Self {
            pool,
            schema: options.schema,
        })
    }

    /// 用**已有连接池**构造（连接池的所有权转移给平台 —— 调用方拿不回去）。
    ///
    /// 用于「连接池由别处建好」的场景（迁移工具、测试夹具）。
    pub fn from_pool(pool: PgPool, options: DbOptions) -> Self {
        Self {
            pool,
            schema: options.schema,
        }
    }

    /// 当前 schema 名。
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// 带 schema 前缀的表名。
    ///
    /// 拼进 SQL 前请确认表名是安全的标识符（业务模块的表名来自 manifest 校验）。
    pub fn table(&self, name: &str) -> String {
        qualified_table(&self.schema, name)
    }

    /// 单次查询：返回全部行。
    pub async fn fetch_all<T>(&self, ctx: &RequestContext, query: &TenantQuery) -> DbResult<Vec<T>>
    where
        T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
    {
        let mut tx = self.begin(ctx).await?;
        let rows = tx.fetch_all(query).await?;
        tx.commit().await?;
        Ok(rows)
    }

    /// 单次查询：返回第一行（没有则 `None`）。
    pub async fn fetch_optional<T>(
        &self,
        ctx: &RequestContext,
        query: &TenantQuery,
    ) -> DbResult<Option<T>>
    where
        T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
    {
        let mut tx = self.begin(ctx).await?;
        let row = tx.fetch_optional(query).await?;
        tx.commit().await?;
        Ok(row)
    }

    /// 单次查询：必须命中一行。
    pub async fn fetch_one<T>(&self, ctx: &RequestContext, query: &TenantQuery) -> DbResult<T>
    where
        T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
    {
        let mut tx = self.begin(ctx).await?;
        let row = tx.fetch_one(query).await?;
        tx.commit().await?;
        Ok(row)
    }

    /// 单次写操作：返回受影响行数。
    pub async fn execute(&self, ctx: &RequestContext, query: &TenantQuery) -> DbResult<u64> {
        let mut tx = self.begin(ctx).await?;
        let affected = tx.execute(query).await?;
        tx.commit().await?;
        Ok(affected)
    }

    /// 多操作形态：开一个事务，全程带租户上下文。
    ///
    /// 事务内的每一次访问都在**同一个事务**里执行，租户上下文只在事务内有效 ——
    /// 事务结束（`commit` / `rollback`）后连接归还连接池时是干净的。
    pub async fn begin(&self, ctx: &RequestContext) -> DbResult<TenantTx> {
        let tenant_id = ctx.tenant_id().ok_or(DbError::MissingTenant)?.to_string();

        let mut tx = self.pool.begin().await?;

        // SET LOCAL app.tenant_id = $1（事务局部，事务结束自动清除）。
        // 用 set_config(..., true) 而不是字面的 SET LOCAL：SET 不接受绑定参数，
        // 而 set_config 的 is_local => true 与 SET LOCAL 语义一致且能走绑定参数。
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.clone())
            .execute(&mut *tx)
            .await?;

        Ok(TenantTx { tx, tenant_id })
    }
}

/// 租户作用域的一次事务。
///
/// 只能通过 [`TenantDb::begin`] 拿到；它**不公开**底层连接。
#[derive(Debug)]
pub struct TenantTx {
    tx: Transaction<'static, Postgres>,
    tenant_id: String,
}

impl TenantTx {
    /// 本事务的租户 ID。
    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    /// 查询：返回全部行。
    pub async fn fetch_all<T>(&mut self, query: &TenantQuery) -> DbResult<Vec<T>>
    where
        T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
    {
        let (sql, values) = query.render(&self.tenant_id)?;
        let mut statement = sqlx::query_as::<Postgres, T>(&sql);
        for value in values {
            statement = bind_row(statement, value);
        }
        Ok(statement.fetch_all(&mut *self.tx).await?)
    }

    /// 查询：返回第一行（没有则 `None`）。
    pub async fn fetch_optional<T>(&mut self, query: &TenantQuery) -> DbResult<Option<T>>
    where
        T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
    {
        let (sql, values) = query.render(&self.tenant_id)?;
        let mut statement = sqlx::query_as::<Postgres, T>(&sql);
        for value in values {
            statement = bind_row(statement, value);
        }
        Ok(statement.fetch_optional(&mut *self.tx).await?)
    }

    /// 查询：必须命中一行。
    pub async fn fetch_one<T>(&mut self, query: &TenantQuery) -> DbResult<T>
    where
        T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
    {
        let (sql, values) = query.render(&self.tenant_id)?;
        let mut statement = sqlx::query_as::<Postgres, T>(&sql);
        for value in values {
            statement = bind_row(statement, value);
        }
        statement
            .fetch_one(&mut *self.tx)
            .await
            .map_err(|err| match err {
                sqlx::Error::RowNotFound => DbError::RowNotFound,
                other => DbError::Database(other),
            })
    }

    /// 写操作：返回受影响行数。
    pub async fn execute(&mut self, query: &TenantQuery) -> DbResult<u64> {
        let (sql, values) = query.render(&self.tenant_id)?;
        let mut statement = sqlx::query(&sql);
        for value in values {
            statement = bind_statement(statement, value);
        }
        Ok(statement.execute(&mut *self.tx).await?.rows_affected())
    }

    /// 提交事务。
    pub async fn commit(self) -> DbResult<()> {
        Ok(self.tx.commit().await?)
    }

    /// 回滚事务。
    pub async fn rollback(self) -> DbResult<()> {
        Ok(self.tx.rollback().await?)
    }
}

/// 带 schema 前缀的表名。
fn qualified_table(schema: &str, name: &str) -> String {
    format!("{schema}.{name}")
}

/// 给行查询绑定一个参数值。
///
/// 不同类型的值要分别调 `bind`（`sqlx` 的绑定是强类型的），所以这里按类型分派。
fn bind_row<'q, T>(
    query: QueryAs<'q, Postgres, T, PgArguments>,
    value: BindValue,
) -> QueryAs<'q, Postgres, T, PgArguments> {
    match value {
        BindValue::Text(text) => query.bind(text),
        BindValue::Int(number) => query.bind(number),
        BindValue::Int4(number) => query.bind(number),
        BindValue::Bool(flag) => query.bind(flag),
        BindValue::NullableInt(number) => query.bind(number),
        BindValue::Float(number) => query.bind(number),
    }
}

/// 给非行查询绑定一个参数值。
fn bind_statement<'q>(
    query: Query<'q, Postgres, PgArguments>,
    value: BindValue,
) -> Query<'q, Postgres, PgArguments> {
    match value {
        BindValue::Text(text) => query.bind(text),
        BindValue::Int(number) => query.bind(number),
        BindValue::Int4(number) => query.bind(number),
        BindValue::Bool(flag) => query.bind(flag),
        BindValue::NullableInt(number) => query.bind(number),
        BindValue::Float(number) => query.bind(number),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_default_to_a_bounded_pool_and_public_schema() {
        let options = DbOptions::default();
        assert_eq!(options.max_connections, DEFAULT_MAX_CONNECTIONS);
        assert_eq!(options.schema, "public");
    }

    #[test]
    fn table_names_are_schema_qualified() {
        assert_eq!(
            qualified_table("tenant_a", "order_item"),
            "tenant_a.order_item"
        );
        assert_eq!(qualified_table("public", "order_item"), "public.order_item");
    }

    #[tokio::test]
    async fn invalid_url_is_rejected_instead_of_panicking() {
        let error = TenantDb::connect("not-a-postgres-url", DbOptions::default())
            .await
            .expect_err("非法连接串必须报错");
        assert_eq!(error.code(), "DATABASE_ERROR");
    }
}
