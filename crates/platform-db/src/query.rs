//! 租户作用域的语句构造。
//!
//! # 为什么语句里必须写 `{tenant}`
//!
//! 平台**不替业务模块拼 WHERE 条件** —— 它没法知道业务表的过滤条件长什么样。
//! 平台做的是另一件事：**保证每条语句都带租户条件，且那个租户一定来自
//! [`RequestContext`](platform_core::RequestContext)**。
//!
//! 规则：
//!
//! | 写法 | 含义 |
//! |------|------|
//! | `{tenant}` | 租户占位符。语句里**必须**出现且只出现一次；平台把它绑定成上下文里的租户 ID（`$1`） |
//! | `?` | 业务自己的参数占位符；平台按出现顺序改写成 `$2`、`$3`…… |
//!
//! ```text
//! SELECT id, title FROM order_item
//!   WHERE tenant_id = {tenant} AND id = ?
//!            ↓ 渲染
//! SELECT id, title FROM order_item
//!   WHERE tenant_id = $1 AND id = $2
//! ```
//!
//! 于是「忘了加租户条件」这种错误**写不出来**：少了 `{tenant}`，平台直接报错。
//!
//! # 为什么不是 `WHERE tenant_id = ?` + 平台自己绑租户
//!
//! 那样平台就得猜「哪个 `?` 是租户」，猜错就会把别的参数当租户绑上去 ——
//! 静默错绑比报错更危险。显式的 `{tenant}` 没有歧义。

use crate::error::{DbError, DbResult};

/// 租户占位符：语句里必须出现且只出现一次。
pub const TENANT_TOKEN: &str = "{tenant}";

/// 可绑定的参数值。
///
/// 只支持常见标量类型 —— 够用、且不会让调用方拼 SQL 片段。
#[derive(Debug, Clone, PartialEq)]
pub enum BindValue {
    /// 文本。
    Text(String),
    /// 64 位整数。
    Int(i64),
    /// 32 位整数。
    Int4(i32),
    /// 布尔。
    Bool(bool),
    /// 可空 64 位整数（`None` → SQL `NULL`）。
    NullableInt(Option<i64>),
    /// 双精度浮点。
    Float(f64),
}

impl From<String> for BindValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for BindValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<i64> for BindValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<i32> for BindValue {
    fn from(value: i32) -> Self {
        Self::Int4(value)
    }
}

impl From<Option<i64>> for BindValue {
    fn from(value: Option<i64>) -> Self {
        Self::NullableInt(value)
    }
}

impl From<bool> for BindValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<f64> for BindValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

/// 租户作用域的语句（查询 / 插入 / 更新 / 删除都走它）。
#[derive(Debug, Clone, PartialEq)]
pub struct TenantQuery {
    statement: String,
    args: Vec<BindValue>,
}

impl TenantQuery {
    /// 新建一条租户作用域语句。
    ///
    /// `statement` 里必须有且只有一个 `{tenant}`。
    pub fn new(statement: impl Into<String>) -> Self {
        Self {
            statement: statement.into(),
            args: Vec::new(),
        }
    }

    /// 追加一个绑定值（对应语句里的下一个 `?`）。
    pub fn bind(mut self, value: impl Into<BindValue>) -> Self {
        self.args.push(value.into());
        self
    }

    /// 语句原文。
    pub fn statement(&self) -> &str {
        &self.statement
    }

    /// 绑定值数量。
    pub fn arg_count(&self) -> usize {
        self.args.len()
    }

    /// 渲染成最终 SQL 与绑定值列表。
    ///
    /// 绑定值顺序：`$1` 恒为租户 ID，其后是业务自己的参数。
    pub fn render(&self, tenant_id: &str) -> DbResult<(String, Vec<BindValue>)> {
        let token_count = self.statement.matches(TENANT_TOKEN).count();
        if token_count != 1 {
            return Err(DbError::InvalidQuery {
                reason: format!(
                    "the statement must contain exactly one `{TENANT_TOKEN}` placeholder, found {token_count}: {}",
                    self.statement
                ),
            });
        }

        let placeholder_count = self.statement.matches('?').count();
        if placeholder_count != self.args.len() {
            return Err(DbError::InvalidQuery {
                reason: format!(
                    "the statement has {placeholder_count} `?` placeholders but {} values are bound: {}",
                    self.args.len(),
                    self.statement
                ),
            });
        }

        let mut sql = String::with_capacity(self.statement.len() + 8);
        let mut next = 2usize;
        let mut parts = self.statement.split(TENANT_TOKEN);
        let head = parts.next().unwrap_or_default();
        push_placeholders(&mut sql, head, &mut next);
        sql.push_str("$1");
        for part in parts {
            push_placeholders(&mut sql, part, &mut next);
        }

        let mut values = Vec::with_capacity(self.args.len() + 1);
        values.push(BindValue::Text(tenant_id.to_string()));
        values.extend(self.args.iter().cloned());
        Ok((sql, values))
    }
}

/// 把一段里的 `?` 依次改写成 `$n`。
fn push_placeholders(sql: &mut String, part: &str, next: &mut usize) {
    for ch in part.chars() {
        if ch == '?' {
            sql.push('$');
            sql.push_str(&next.to_string());
            *next += 1;
        } else {
            sql.push(ch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_token_becomes_the_first_parameter() {
        let query =
            TenantQuery::new("SELECT id FROM t WHERE tenant_id = {tenant} AND id = ?").bind("x");
        let (sql, values) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(sql, "SELECT id FROM t WHERE tenant_id = $1 AND id = $2");
        assert_eq!(
            values,
            vec![
                BindValue::Text("tenant-a".to_string()),
                BindValue::Text("x".to_string())
            ]
        );
    }

    #[test]
    fn placeholders_before_and_after_the_token_are_renumbered() {
        let query = TenantQuery::new("SELECT * FROM t WHERE a = ? AND b = {tenant} AND c = ?")
            .bind(1i64)
            .bind(2i64);
        let (sql, values) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(sql, "SELECT * FROM t WHERE a = $2 AND b = $1 AND c = $3");
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], BindValue::Text("tenant-a".to_string()));
    }

    #[test]
    fn a_statement_without_the_tenant_token_is_rejected() {
        let query = TenantQuery::new("SELECT id FROM t WHERE id = ?").bind("x");
        let error = query.render("tenant-a").expect_err("缺租户占位符必须报错");
        assert_eq!(error.code(), "INVALID_QUERY");
    }

    #[test]
    fn a_statement_with_two_tenant_tokens_is_rejected() {
        let query = TenantQuery::new("SELECT * FROM t WHERE a = {tenant} AND b = {tenant}");
        let error = query
            .render("tenant-a")
            .expect_err("重复的租户占位符必须报错");
        assert_eq!(error.code(), "INVALID_QUERY");
    }

    #[test]
    fn placeholder_and_binding_counts_must_match() {
        let query = TenantQuery::new("SELECT * FROM t WHERE a = {tenant} AND b = ?");
        let error = query
            .render("tenant-a")
            .expect_err("占位符与绑定值数量对不上必须报错");
        assert_eq!(error.code(), "INVALID_QUERY");

        let query = TenantQuery::new("SELECT * FROM t WHERE a = {tenant} AND b = ? AND c = ?")
            .bind("x")
            .bind("y");
        assert!(query.render("tenant-a").is_ok());
    }

    #[test]
    fn bind_values_accept_common_scalars() {
        let query = TenantQuery::new(
            "SELECT * FROM t WHERE a = {tenant} AND b = ? AND c = ? AND d = ? AND e = ? AND f = ? AND g = ? AND h = ?",
        )
        .bind("text")
        .bind(String::from("owned"))
        .bind(1i64)
        .bind(2i32)
        .bind(true)
        .bind(1.5f64)
        .bind(Option::<i64>::None);
        assert_eq!(query.arg_count(), 7);
        let (_, values) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(values.len(), 8, "租户占一个位置");
    }
}
