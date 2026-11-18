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
use platform_core::extension::expr::{CompiledCondition, ConditionValue};
use platform_core::extension::permission::ScopeFilter;

/// 租户占位符：语句里必须出现且只出现一次。
pub const TENANT_TOKEN: &str = "{tenant}";

/// 可绑定的参数值。
///
/// 只支持常见标量类型 —— 够用、且不会让调用方拼 SQL 片段。
#[derive(Debug, Clone, PartialEq)]
pub enum BindValue {
    /// 文本。
    Text(String),
    /// 文本数组（`text[]`）：数据范围的「子树 id 集合」用它。
    TextArray(Vec<String>),
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
    /// 可空文本（`None` → SQL `NULL`）。
    NullableText(Option<String>),
    /// 时间点（`timestamptz`）。
    ///
    /// 契约 C13：一律 UTC 存储 —— `OffsetDateTime` 带偏移，绑定时按 UTC 归一。
    Timestamp(time::OffsetDateTime),
    /// 可空时间点（`None` → SQL `NULL`）。
    NullableTimestamp(Option<time::OffsetDateTime>),
}

impl From<Vec<String>> for BindValue {
    fn from(value: Vec<String>) -> Self {
        Self::TextArray(value)
    }
}

/// 把表达式里的字面值转成可绑定的参数值。
///
/// **字面值一律走绑定** —— 生成的 SQL 文本里看不到它们（契约 C6 / AC-S-042）。
fn bind_from_condition_value(value: &ConditionValue) -> BindValue {
    match value {
        ConditionValue::Text(text) => BindValue::Text(text.clone()),
        ConditionValue::Int(number) => BindValue::Int(*number),
        ConditionValue::Bool(flag) => BindValue::Bool(*flag),
        ConditionValue::Float(number) => BindValue::Float(*number),
    }
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

impl From<time::OffsetDateTime> for BindValue {
    fn from(value: time::OffsetDateTime) -> Self {
        Self::Timestamp(value)
    }
}

impl From<Option<String>> for BindValue {
    fn from(value: Option<String>) -> Self {
        Self::NullableText(value)
    }
}

impl From<Option<time::OffsetDateTime>> for BindValue {
    fn from(value: Option<time::OffsetDateTime>) -> Self {
        Self::NullableTimestamp(value)
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

    /// **叠加数据范围条件**（契约 C7：数据范围过滤由平台自动叠加）。
    ///
    /// # 为什么业务模块绕不过去
    ///
    /// - 条件在 **SQL 层**生效（不是查出来后在内存里筛）；
    /// - 条件里的值**一律走参数绑定**（SQL 文本里不出现字面值）；
    /// - 本 crate **不提供**「跳过范围」的方法或参数 ——
    ///   想跳过就只能绕过 [`TenantQuery`]，而那会丢掉租户条件（`{tenant}` 占位符强制）。
    ///
    /// [`ScopeFilter::All`] 不追加任何条件（= 不限范围）。
    pub fn with_scope(mut self, scope: &ScopeFilter) -> DbResult<Self> {
        match scope {
            ScopeFilter::All => Ok(self),
            ScopeFilter::OrgNodes { column, nodes } => {
                let column = safe_column(column)?;
                let condition = format!("{column} = ANY(?)");
                self.statement = insert_where_condition(&self.statement, &condition);
                self.args.push(BindValue::TextArray(nodes.clone()));
                Ok(self)
            }
            ScopeFilter::Owner { column, user_id } => {
                let column = safe_column(column)?;
                let condition = format!("{column} = ?");
                self.statement = insert_where_condition(&self.statement, &condition);
                self.args.push(BindValue::Text(user_id.clone()));
                Ok(self)
            }
            ScopeFilter::Custom { conditions } => {
                for condition in conditions {
                    self = self.with_condition(condition)?;
                }
                Ok(self)
            }
        }
    }

    /// **叠加属性级校验条件**（RBAC + ABAC 混合）。
    ///
    /// 与 [`Self::with_scope`] 一样：SQL 层生效 + 参数绑定。
    pub fn with_condition(mut self, condition: &CompiledCondition) -> DbResult<Self> {
        if condition.placeholder_count() != condition.values.len() {
            return Err(DbError::InvalidQuery {
                reason: format!(
                    "the condition has {} placeholders but {} values are bound",
                    condition.placeholder_count(),
                    condition.values.len()
                ),
            });
        }
        self.statement = insert_where_condition(&self.statement, &format!("({})", condition.sql));
        for value in &condition.values {
            self.args.push(bind_from_condition_value(value));
        }
        Ok(self)
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

/// 校验一个列名是安全的 SQL 标识符（**不合法就拒绝**，绝不拼进 SQL）。
///
/// 列名来自业务侧的表结构 —— 但它会被拼进 SQL，所以**必须**校验。
/// 规则与平台的其它标识符规则一致：`^[a-z][a-z0-9_]*$`。
fn safe_column(column: &str) -> DbResult<String> {
    let mut chars = column.chars();
    let valid = match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        _ => false,
    };
    if !valid {
        return Err(DbError::InvalidQuery {
            reason: format!("the column name `{column}` is not a safe SQL identifier"),
        });
    }
    Ok(column.to_string())
}

/// 把一个条件插进语句的 **WHERE 子句末尾**。
///
/// # 为什么不能直接拼到语句末尾
///
/// 业务模块的语句常带 `ORDER BY` / `LIMIT`：
///
/// ```text
/// SELECT id FROM t WHERE tenant_id = {tenant} ORDER BY id
/// ```
///
/// 直接往后拼会得到 `... ORDER BY id AND <条件>` —— **语法错误**，
/// 而更危险的是「条件被挤到 ORDER BY 后面」这种**看起来对、其实错**的语句。
///
/// 这里的做法：找到顶层 `WHERE`，再找到 WHERE 子句的**结束位置**
/// （第一个顶层 `ORDER` / `GROUP` / `HAVING` / `LIMIT` / `UNION` … 或语句末尾），
/// 把条件插在它之前。
///
/// 扫描时**跳过字符串字面量与带引号的标识符**，并且只认**顶层**（括号深度 0）的关键字 ——
/// 子查询里的 `ORDER BY` 不会干扰判断。
fn insert_where_condition(statement: &str, condition: &str) -> String {
    let Some(where_end) = where_clause_end(statement) else {
        // 没有 WHERE → 整个语句就是一个条件（`SELECT ... `）。
        return format!("{statement} WHERE {condition}");
    };
    let prefix = statement[..where_end].trim_end();
    let suffix = &statement[where_end..];
    let mut out = String::with_capacity(statement.len() + condition.len() + 8);
    out.push_str(prefix);
    out.push_str(" AND ");
    out.push_str(condition);
    if !suffix.is_empty() && !suffix.starts_with(char::is_whitespace) {
        out.push(' ');
    }
    out.push_str(suffix);
    out
}

/// WHERE 子句的结束位置（字节偏移）。
///
/// - 没有 `WHERE` → `None`；
/// - 有 → WHERE 子句末尾的偏移（= 第一个顶层结束关键字的起点，或语句末尾）。
fn where_clause_end(statement: &str) -> Option<usize> {
    let bytes: Vec<char> = statement.chars().collect();
    let mut index = 0usize;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut in_identifier = false;
    let mut where_start: Option<usize> = None;

    while index < bytes.len() {
        let ch = bytes[index];
        if in_string {
            if ch == '\'' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if in_identifier {
            if ch == '"' {
                in_identifier = false;
            }
            index += 1;
            continue;
        }
        match ch {
            '\'' => {
                in_string = true;
                index += 1;
                continue;
            }
            '"' => {
                in_identifier = true;
                index += 1;
                continue;
            }
            '(' => {
                depth += 1;
                index += 1;
                continue;
            }
            ')' => {
                depth = depth.saturating_sub(1);
                index += 1;
                continue;
            }
            _ => {}
        }
        if depth != 0 || !ch.is_ascii_alphabetic() {
            index += 1;
            continue;
        }
        // 读一个单词。
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == '_') {
            index += 1;
        }
        let word: String = bytes[start..index].iter().collect();
        let word_start = start;
        let upper = word.to_ascii_uppercase();

        if where_start.is_none() {
            if upper == "WHERE" {
                where_start = Some(index);
            }
            continue;
        }
        // WHERE 之后：第一个顶层结束关键字就是子句末尾。
        if matches!(
            upper.as_str(),
            "ORDER"
                | "GROUP"
                | "HAVING"
                | "LIMIT"
                | "OFFSET"
                | "FETCH"
                | "UNION"
                | "INTERSECT"
                | "EXCEPT"
                | "RETURNING"
                | "WINDOW"
        ) {
            return Some(word_start);
        }
    }
    where_start.map(|_| bytes.len())
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

    // -- 数据范围（AC-S-048：过滤在 SQL 层生效） --------------------------

    /// 数据范围条件被**渲染进 SQL 文本**（不是查出来后在内存里筛）。
    #[test]
    fn the_org_node_scope_is_rendered_into_the_statement() {
        let scope = ScopeFilter::OrgNodes {
            column: "org_node_id".to_string(),
            nodes: vec!["dept-a".to_string(), "dept-a-1".to_string()],
        };
        let query = TenantQuery::new("SELECT id FROM t WHERE tenant_id = {tenant}")
            .with_scope(&scope)
            .expect("应当能叠加");
        let (sql, values) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(
            sql,
            "SELECT id FROM t WHERE tenant_id = $1 AND org_node_id = ANY($2)"
        );
        assert_eq!(
            values[1],
            BindValue::TextArray(vec!["dept-a".to_string(), "dept-a-1".to_string()])
        );
    }

    /// 范围条件里的值**走参数绑定** —— SQL 文本里不出现字面值。
    #[test]
    fn scope_values_are_bound_not_inlined() {
        let scope = ScopeFilter::Owner {
            column: "created_by".to_string(),
            user_id: "u-1001".to_string(),
        };
        let query = TenantQuery::new("SELECT id FROM t WHERE tenant_id = {tenant}")
            .with_scope(&scope)
            .expect("应当能叠加");
        let (sql, values) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(
            sql,
            "SELECT id FROM t WHERE tenant_id = $1 AND created_by = $2"
        );
        assert!(!sql.contains("u-1001"), "SQL 文本里不得出现字面值：{sql}");
        assert_eq!(values[1], BindValue::Text("u-1001".to_string()));
    }

    /// 不限范围 = 不追加任何条件。
    #[test]
    fn the_all_scope_adds_no_condition() {
        let query = TenantQuery::new("SELECT id FROM t WHERE tenant_id = {tenant}")
            .with_scope(&ScopeFilter::All)
            .expect("应当能叠加");
        let (sql, values) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(sql, "SELECT id FROM t WHERE tenant_id = $1");
        assert_eq!(values.len(), 1);
    }

    /// 非法列名**拒绝**（列名会被拼进 SQL）。
    #[test]
    fn an_unsafe_scope_column_is_rejected() {
        let scope = ScopeFilter::Owner {
            column: "created_by; DROP TABLE t".to_string(),
            user_id: "u-1001".to_string(),
        };
        let error = TenantQuery::new("SELECT id FROM t WHERE tenant_id = {tenant}")
            .with_scope(&scope)
            .expect_err("非法列名必须被拒绝");
        assert_eq!(error.code(), "INVALID_QUERY");
    }

    /// 属性级条件：SQL 文本里**只有占位符**（AC-S-042）。
    #[test]
    fn a_condition_is_rendered_with_placeholders_only() {
        let condition = CompiledCondition {
            sql: "amount <= ?".to_string(),
            values: vec![ConditionValue::Int(10000)],
        };
        let query = TenantQuery::new("SELECT id FROM t WHERE tenant_id = {tenant}")
            .with_condition(&condition)
            .expect("应当能叠加");
        let (sql, values) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(
            sql,
            "SELECT id FROM t WHERE tenant_id = $1 AND (amount <= $2)"
        );
        assert!(!sql.contains("10000"), "SQL 文本里不得出现字面值：{sql}");
        assert_eq!(values[1], BindValue::Int(10000));
    }

    /// 自定义范围（多个条件是**交集**）。
    #[test]
    fn a_custom_scope_renders_every_condition() {
        let scope = ScopeFilter::Custom {
            conditions: vec![
                CompiledCondition {
                    sql: "amount <= ?".to_string(),
                    values: vec![ConditionValue::Int(10000)],
                },
                CompiledCondition {
                    sql: "dept_id = ?".to_string(),
                    values: vec![ConditionValue::Text("dept-a".to_string())],
                },
            ],
        };
        let query = TenantQuery::new("SELECT id FROM t WHERE tenant_id = {tenant}")
            .with_scope(&scope)
            .expect("应当能叠加");
        let (sql, values) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(
            sql,
            "SELECT id FROM t WHERE tenant_id = $1 AND (amount <= $2) AND (dept_id = $3)"
        );
        assert_eq!(values.len(), 3);
    }

    /// 占位符与绑定值数量对不上 → 拒绝（不渲染出半截 SQL）。
    #[test]
    fn a_condition_with_mismatched_values_is_rejected() {
        let condition = CompiledCondition {
            sql: "amount <= ? AND dept_id = ?".to_string(),
            values: vec![ConditionValue::Int(10000)],
        };
        let error = TenantQuery::new("SELECT id FROM t WHERE tenant_id = {tenant}")
            .with_condition(&condition)
            .expect_err("占位符与绑定值对不上必须拒绝");
        assert_eq!(error.code(), "INVALID_QUERY");
    }

    /// 范围条件必须插进 **WHERE 子句里** —— 不能被挤到 `ORDER BY` 后面。
    #[test]
    fn the_scope_condition_lands_inside_the_where_clause() {
        let scope = ScopeFilter::Owner {
            column: "created_by".to_string(),
            user_id: "u-1001".to_string(),
        };
        let query = TenantQuery::new(
            "SELECT id FROM t WHERE tenant_id = {tenant} AND title = ? ORDER BY id LIMIT 20",
        )
        .bind("x")
        .with_scope(&scope)
        .expect("应当能叠加");
        let (sql, values) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(
            sql,
            "SELECT id FROM t WHERE tenant_id = $1 AND title = $2 AND created_by = $3 \
             ORDER BY id LIMIT 20"
        );
        assert_eq!(values.len(), 3);
    }

    /// 没有 WHERE 的语句 → 补一个（平台自己的语句总有 WHERE，这条是兜底）。
    #[test]
    fn a_statement_without_a_where_clause_gets_one() {
        let sql = insert_where_condition("SELECT id FROM t", "created_by = ?");
        assert_eq!(sql, "SELECT id FROM t WHERE created_by = ?");
    }

    /// 子查询里的 `ORDER BY` 不得干扰 WHERE 子句的判定。
    #[test]
    fn a_subquery_order_by_does_not_confuse_the_insertion_point() {
        let scope = ScopeFilter::Owner {
            column: "created_by".to_string(),
            user_id: "u-1001".to_string(),
        };
        let query = TenantQuery::new(
            "SELECT id FROM t WHERE tenant_id = {tenant} \
             AND id IN (SELECT id FROM u ORDER BY id) ORDER BY id",
        )
        .with_scope(&scope)
        .expect("应当能叠加");
        let (sql, _) = query.render("tenant-a").expect("应当能渲染");
        assert_eq!(
            sql,
            "SELECT id FROM t WHERE tenant_id = $1 \
             AND id IN (SELECT id FROM u ORDER BY id) AND created_by = $2 ORDER BY id"
        );
    }
}
