//! 审计查询：分页、过滤、排序（REQ-014，契约 C7 口径）。
//!
//! # 口径（契约 C7 / REQ-014 D145 / D153 / D155）
//!
//! | 参数 | 形态 | 默认 / 上限 |
//! |------|------|------------|
//! | `cursor` | 不透明游标（base64url） | 与 `page` **互斥** |
//! | `limit` | 正整数 | 默认 **20**，上限 **200**（⚠️ 超上限**拒绝**，REQ-019 D341） |
//! | `page` / `page_size` | 正整数 | 与 `cursor` **互斥** |
//! | `sort` | `?sort=-createdAt,actorId` | 白名单：`createdAt` / `actorId` / `action` / `objectType` / `objectId` |
//! | `filter[field][op]` | `?filter[actorId][eq]=…` | op 白名单：`eq` / `ne` / `in` / `contains` / `range` |
//!
//! # 非法值与超上限**都是** 400（⚠️ REQ-019 D341 的破坏性修正）
//!
//! - `limit=abc` / `limit=0` → **400** `AUDIT_INVALID_FILTER`；
//! - `limit=1000` → **400** `AUDIT_INVALID_FILTER`（原行为是「夹到 200 + 不报错」）。
//!
//! ⚠️ 改的理由：静默截断会让调用方以为拿到了 1000 条 → **数据静默丢失**。
//! 上限**保持 200**（不改 100）以减小破坏面。
//!
//! # 为什么参数要**自解析**
//!
//! axum 0.8 的 `QueryRejection` 是**纯文本** 400 → 违反契约 C6 的
//! **Problem Details 形状**要求（Content-Type 跟随平台现状 `application/json`，D183）。
//! 本模块只吃
//! `HashMap<String, String>`，自己校验、自己报错（错误由调用方转成
//! [`ProblemDetails`](platform_core::problem::ProblemDetails)）。
//!
//! # 游标的编码与排序的对应关系
//!
//! 游标 = `base64url("{createdAt}|{id}")`，其中 `createdAt` 是
//! **规范时间戳文本**（微秒 6 位，UTC）。
//!
//! 它对应的是**默认排序** `ORDER BY created_at DESC, id DESC`：
//! 翻页条件是 `(created_at, id) < (游标的 created_at, 游标的 id)`
//! （元组比较，**与排序方向一致**）。
//!
//! ⚠️ 因此**游标只对默认排序有效** —— 指定了别的 `sort` 时带游标 → **400**
//! （否则游标与排序对不上，翻页会漏/重）。这比「悄悄给出一个错的页」安全。
//!
//! ⚠️ 契约 C7 只钉了「`cursor` 与 `page` 互斥」，**没有**钉「游标 + 排序」
//! 这条 → 本仓的这条是**收紧**（D179 已裁决**保留**收紧：宁可 400 不给错页）。

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use platform_core::RequestContext;
use platform_core::extension::audit::{AuditError, format_timestamp_micros_utc};
use platform_db::{BindValue, TenantDb, TenantQuery};
use time::OffsetDateTime;

/// 默认每页条数（契约 C7）。
pub const DEFAULT_LIMIT: usize = 20;
/// 每页条数上限（契约 C7）。
pub const MAX_LIMIT: usize = 200;

/// 可排序 / 可过滤的字段白名单。
///
/// ⚠️ 返回的是**固定白名单里的常量**（不是用户输入）→ 拼进 SQL 是安全的。
fn column_of(field: &str) -> Option<&'static str> {
    match field {
        "actorId" | "actor_id" => Some("actor_id"),
        "action" => Some("action"),
        "objectType" | "object_type" => Some("object_type"),
        "objectId" | "object_id" => Some("object_id"),
        "createdAt" | "created_at" => Some("created_at"),
        _ => None,
    }
}

/// 排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// 升序。
    Asc,
    /// 降序。
    Desc,
}

/// 一个排序项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortField {
    /// 列名（白名单里的常量）。
    pub column: &'static str,
    /// 方向。
    pub direction: SortDirection,
}

/// 过滤操作符（契约 C7 的 op 白名单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FilterOp {
    /// 等于。
    Eq,
    /// 不等于。
    Ne,
    /// 属于集合。
    In,
    /// 包含子串。
    Contains,
    /// 区间（`from,to`）。
    Range,
}

impl FilterOp {
    fn parse(op: &str) -> Option<Self> {
        match op {
            "eq" => Some(Self::Eq),
            "ne" => Some(Self::Ne),
            "in" => Some(Self::In),
            "contains" => Some(Self::Contains),
            "range" => Some(Self::Range),
            _ => None,
        }
    }
}

/// 一个过滤条件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// 列名（白名单里的常量）。
    pub column: &'static str,
    /// 操作符。
    pub op: FilterOp,
    /// 值（`in` 是集合；`range` 是 `[from, to]`）。
    pub values: Vec<String>,
}

/// 审计查询参数（已校验）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditQueryParams {
    /// 游标（与 `page` 互斥）。
    pub cursor: Option<String>,
    /// 每页条数（1..=200）。
    pub limit: usize,
    /// 页码（1 起；与 `cursor` 互斥）。
    pub page: Option<usize>,
    /// 每页条数（页码模式）。
    pub page_size: usize,
    /// 排序项（空 = 默认排序）。
    pub sort: Vec<SortField>,
    /// 过滤条件。
    pub filters: Vec<Filter>,
}

impl Default for AuditQueryParams {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: DEFAULT_LIMIT,
            page: None,
            page_size: DEFAULT_LIMIT,
            sort: Vec::new(),
            filters: Vec::new(),
        }
    }
}

impl AuditQueryParams {
    /// 自解析查询参数（**不用** axum 的 `Query` —— 它的 `QueryRejection`
    /// 是纯文本 400，违反契约 C6）。
    ///
    /// # `tenant_id` 查询参数
    ///
    /// 租户**只**来自认证上下文 + RLS。查询参数里的 `tenant_id` 一律
    /// **忽略并记一条 warn 日志**（D162）—— 400 会把「该参数不被支持」
    /// 这个信息暴露给探测方。
    pub fn parse(params: &HashMap<String, String>) -> Result<Self, AuditError> {
        let mut parsed = Self::default();

        if params.contains_key("tenant_id") {
            tracing::warn!(
                "the audit query received a `tenant_id` parameter; it is ignored \
                 (the tenant comes from the authentication context and the row level security policy)"
            );
        }

        // -- limit ------------------------------------------------------------
        //
        // ⚠️ **REQ-019 D341：超上限从「夹到上限」改为「拒绝」**（契约 C7 升版）。
        //
        // 理由：静默截断会让调用方以为拿到了 1000 条 → **数据静默丢失**。
        // 这是「API 治理」的核心内容，值得一次破坏性修正。
        //
        // ⚠️ 上限**保持 200**（不改 100）以减小破坏面。
        if let Some(raw) = params.get("limit") {
            let value = parse_positive(raw, "limit")?;
            if value > MAX_LIMIT {
                return Err(invalid(format!(
                    "the `limit` parameter must not exceed {MAX_LIMIT}; got {value}"
                )));
            }
            parsed.limit = value;
        }

        // -- page / page_size -------------------------------------------------
        if let Some(raw) = params.get("page") {
            parsed.page = Some(parse_positive(raw, "page")?);
        }
        if let Some(raw) = params.get("page_size") {
            let value = parse_positive(raw, "page_size")?;
            if value > MAX_LIMIT {
                return Err(invalid(format!(
                    "the `page_size` parameter must not exceed {MAX_LIMIT}; got {value}"
                )));
            }
            parsed.page_size = value;
        }

        // -- cursor -----------------------------------------------------------
        if let Some(raw) = params.get("cursor") {
            if !raw.is_empty() {
                parsed.cursor = Some(raw.clone());
            }
        }

        // 游标与页码**互斥**（契约 C7）。
        if parsed.cursor.is_some() && parsed.page.is_some() {
            return Err(invalid("cursor and page are mutually exclusive"));
        }

        // -- sort -------------------------------------------------------------
        if let Some(raw) = params.get("sort") {
            for item in raw.split(',') {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                let (field, direction) = match item.strip_prefix('-') {
                    Some(field) => (field, SortDirection::Desc),
                    None => (item, SortDirection::Asc),
                };
                let column = column_of(field)
                    .ok_or_else(|| invalid(format!("`{field}` is not a sortable field")))?;
                parsed.sort.push(SortField { column, direction });
            }
        }

        // 游标只对**默认排序**有效（D179 的裁决，见模块文档「游标的编码与排序的对应关系」）。
        //
        // 理由：游标里编码的是 `(created_at, id)` 这个**排序键**，翻页条件
        // `(created_at, id) < (游标的 created_at, 游标的 id)` 也只在这个
        // 排序方向下成立。换成别的 `sort`（比如 `action`）后，游标与新排序
        // 对不上 → 翻页会**漏行或重复行**。
        //
        // → 这里**宁可 400**（`AUDIT_INVALID_FILTER`）也不给出一个错的页：
        // 「静默返回错数据」比「明确拒绝」危险得多。
        //
        // ⚠️ 契约 C7 只钉了「`cursor` 与 `page` 互斥」，**没有**写这条
        // 「游标 + 排序互斥」的约束 → 属于本仓的**收紧**（D179 已裁决保留）。
        if parsed.cursor.is_some() && !parsed.sort.is_empty() {
            return Err(invalid(
                "the cursor is only valid with the default sort order",
            ));
        }

        // -- filter[field][op] ------------------------------------------------
        let mut filters: Vec<Filter> = Vec::new();
        for (key, raw) in params {
            let Some((field, op)) = parse_filter_key(key) else {
                continue;
            };
            let column = column_of(field)
                .ok_or_else(|| invalid(format!("`{field}` is not a filterable field")))?;
            let op = FilterOp::parse(op)
                .ok_or_else(|| invalid(format!("`{op}` is not a supported filter operator")))?;
            let values = match op {
                FilterOp::In => raw
                    .split(',')
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect::<Vec<_>>(),
                FilterOp::Range => raw
                    .split(',')
                    .map(|item| item.trim().to_string())
                    .collect::<Vec<_>>(),
                _ => vec![raw.clone()],
            };
            if values.is_empty() {
                return Err(invalid(format!("the filter `{key}` has no value")));
            }
            // 时间戳没有「包含」语义 → 直接拒绝，不做近似（近似 = 悄悄给错结果）。
            if column == "created_at" && op == FilterOp::Contains {
                return Err(invalid(
                    "`createdAt` does not support the `contains` operator",
                ));
            }
            if op == FilterOp::Range && values.len() != 2 {
                return Err(invalid(
                    "the `range` operator takes exactly two comma separated values",
                ));
            }
            filters.push(Filter { column, op, values });
        }
        filters.sort_by(|left, right| (left.column, left.op).cmp(&(right.column, right.op)));
        parsed.filters = filters;

        Ok(parsed)
    }
}

/// 解析 `filter[field][op]` 形式的键。
fn parse_filter_key(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix("filter[")?;
    let (field, rest) = rest.split_once("][")?;
    let op = rest.strip_suffix(']')?;
    if field.is_empty() || op.is_empty() {
        return None;
    }
    Some((field, op))
}

fn parse_positive(raw: &str, field: &str) -> Result<usize, AuditError> {
    let value: usize = raw
        .trim()
        .parse()
        .map_err(|_| invalid(format!("`{field}` must be a positive integer")))?;
    if value == 0 {
        return Err(invalid(format!("`{field}` must be greater than zero")));
    }
    Ok(value)
}

fn invalid(reason: impl Into<String>) -> AuditError {
    AuditError::InvalidFilter {
        reason: reason.into(),
    }
}

/// `count(*)` 的返回值。
struct TotalCount {
    total: i64,
}

platform_db::tenant_row!(TotalCount { total: "count" });

/// 一条审计记录（查询层视角）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEventRow {
    /// 记录 ID。
    pub id: String,
    /// 操作人（未认证时为 `None`）。
    pub actor_id: Option<String>,
    /// 动作名。
    pub action: String,
    /// 对象类型。
    pub object_type: String,
    /// 对象 ID。
    pub object_id: Option<String>,
    /// 变更前（**列里存的那段文本**，不是解析后的 JSON）。
    pub before: Option<String>,
    /// 变更后（**列里存的那段文本**）。
    pub after: Option<String>,
    /// 来源 IP（空串 = 未知）。
    pub ip: String,
    /// 设备（空串 = 未知）。
    pub user_agent: String,
    /// 创建时刻（UTC）。
    pub created_at: OffsetDateTime,
    /// 上一条的哈希（本租户首条为 genesis）。
    pub prev_hash: String,
    /// 本条的哈希。
    pub hash: String,
}

platform_db::tenant_row!(AuditEventRow {
    id: "id",
    actor_id: "actor_id",
    action: "action",
    object_type: "object_type",
    object_id: "object_id",
    before: "before",
    after: "after",
    ip: "ip",
    user_agent: "user_agent",
    created_at: "created_at",
    prev_hash: "prev_hash",
    hash: "hash",
});

/// 一页审计记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditPage {
    /// 本页记录。
    pub items: Vec<AuditEventRow>,
    /// 该租户的记录总数（**不受分页影响**）。
    pub total: i64,
    /// 下一页的游标（没有下一页时为 `None`）。
    pub next_cursor: Option<String>,
}

/// 审计查询服务。
#[derive(Debug, Clone)]
pub struct AuditQueryService {
    db: TenantDb,
}

impl AuditQueryService {
    /// 用已有的租户作用域数据入口构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 查一页审计记录。
    pub async fn query(
        &self,
        ctx: &RequestContext,
        params: &AuditQueryParams,
    ) -> Result<AuditPage, AuditError> {
        let (clause, filter_binds) = render_filters(&params.filters);

        // -- 总数 ------------------------------------------------------------
        let count_sql =
            format!("SELECT count(*) FROM platform_audit_log WHERE tenant_id = {{tenant}}{clause}");
        let mut count_query = TenantQuery::new(count_sql);
        for bind in &filter_binds {
            count_query = count_query.bind(bind.clone());
        }
        let mut tx = self.db.begin(ctx).await.map_err(unavailable)?;
        let total: TotalCount = tx.fetch_one(&count_query).await.map_err(unavailable)?;

        // -- 本页 ------------------------------------------------------------
        let mut sql = format!(
            "SELECT id, actor_id, action, object_type, object_id, \"before\", \"after\", \
             ip, user_agent, created_at, prev_hash, hash \
             FROM platform_audit_log WHERE tenant_id = {{tenant}}{clause}"
        );
        let mut binds = filter_binds;

        if let Some(cursor) = &params.cursor {
            let (created_at, id) = decode_cursor(cursor)?;
            sql.push_str(" AND (created_at, id) < (?::timestamptz, ?)");
            binds.push(BindValue::Text(created_at));
            binds.push(BindValue::Text(id));
        }

        let (limit, offset) = match params.page {
            Some(page) => (params.page_size, (page - 1) * params.page_size),
            None => (params.limit, 0),
        };
        sql.push_str(&format!(" ORDER BY {} LIMIT ?", render_order(&params.sort)));
        // 多取一条用来判断「还有下一页」（D184）—— 少取这一条的话
        // `rows.len() > limit` 恒假 → `next_cursor` 恒 `None`，翻页功能不存在。
        binds.push(BindValue::Int(limit as i64 + 1));
        if params.page.is_some() {
            sql.push_str(" OFFSET ?");
            binds.push(BindValue::Int(offset as i64));
        }

        let mut query = TenantQuery::new(sql);
        for bind in &binds {
            query = query.bind(bind.clone());
        }

        let mut rows: Vec<AuditEventRow> = tx.fetch_all(&query).await.map_err(unavailable)?;
        // 多取一条用来判断「还有下一页」。
        let has_more = rows.len() > limit;
        if has_more {
            rows.truncate(limit);
        }
        let next_cursor = if has_more {
            rows.last()
                .map(|row| encode_cursor(&row.created_at, &row.id))
        } else {
            None
        };

        tx.commit().await.map_err(unavailable)?;

        Ok(AuditPage {
            items: rows,
            total: total.total,
            next_cursor,
        })
    }
    /// 查**单条**审计记录（`GET /api/audit/events/{id}`）。
    ///
    /// # 找不到 → `None`（**不是**错误）
    ///
    /// 路由层据此返回 **404** `AUDIT_EVENT_NOT_FOUND`。
    /// ⚠️ **别的租户的记录同样查不到**（RLS + `tenant_id` 过滤）→
    /// 「不存在」与「不是你的」**无法区分**（契约 C5 的资源类口径）。
    pub async fn find_by_id(
        &self,
        ctx: &RequestContext,
        id: &str,
    ) -> Result<Option<AuditEventRow>, AuditError> {
        let mut tx = self.db.begin(ctx).await.map_err(unavailable)?;
        let row: Option<AuditEventRow> = tx
            .fetch_optional(
                &TenantQuery::new(
                    "SELECT id, actor_id, action, object_type, object_id, \"before\", \"after\", \
                 ip, user_agent, created_at, prev_hash, hash \
                 FROM platform_audit_log WHERE tenant_id = {tenant} AND id = ?",
                )
                .bind(id.to_string()),
            )
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(row)
    }
}

/// 渲染过滤条件（SQL 片段 + 绑定值）。
///
/// # 每一段**自带** ` AND ` 前缀（D185）
///
/// 调用点把结果直接贴在 `WHERE tenant_id = {tenant}{clause}` 后面
/// → 第一个过滤器**也**必须有分隔符，否则拼成
/// `tenant_id = $1actor_id = $2`（PG 报 `trailing junk after parameter`）。
///
/// `continue` 掉的分支（「时间戳 + `contains`」与长度不为 2 的 `range`）
/// 在 `parse` 阶段已被拒 → 这里**先**判断是否跳过，**再**写前缀，
/// 保证不会留下一个空的 ` AND `。
fn render_filters(filters: &[Filter]) -> (String, Vec<BindValue>) {
    let mut clause = String::new();
    let mut binds = Vec::new();
    for filter in filters {
        let column = filter.column;
        let is_timestamp = column == "created_at";
        match filter.op {
            FilterOp::Eq => {
                clause.push_str(&format!(" AND {column} = ?{}", cast_suffix(is_timestamp)));
                binds.push(BindValue::Text(filter.values[0].clone()));
            }
            FilterOp::Ne => {
                clause.push_str(&format!(" AND {column} <> ?{}", cast_suffix(is_timestamp)));
                binds.push(BindValue::Text(filter.values[0].clone()));
            }
            FilterOp::In => {
                clause.push_str(&format!(" AND {column} = ANY(?)"));
                binds.push(BindValue::TextArray(filter.values.clone()));
            }
            FilterOp::Contains => {
                if is_timestamp {
                    // 不可能走到这里：`parse` 已经拒掉了「时间戳 + contains」。
                    continue;
                }
                clause.push_str(&format!(" AND {column} LIKE ? ESCAPE '\\'"));
                binds.push(BindValue::Text(format!(
                    "%{}%",
                    escape_like(&filter.values[0])
                )));
            }
            FilterOp::Range => {
                if filter.values.len() != 2 {
                    continue;
                }
                clause.push_str(&format!(
                    " AND {column} >= ?{cast} AND {column} <= ?{cast}",
                    cast = cast_suffix(is_timestamp)
                ));
                binds.push(BindValue::Text(filter.values[0].clone()));
                binds.push(BindValue::Text(filter.values[1].clone()));
            }
        }
    }
    (clause, binds)
}

/// 时间戳列要显式转型（列是 `timestamptz`，绑定值是文本）。
fn cast_suffix(is_timestamp: bool) -> &'static str {
    if is_timestamp { "::timestamptz" } else { "" }
}

/// `LIKE` 的元字符转义（`%` / `_` / `\`）。
fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// 渲染排序（默认 `created_at DESC, id DESC`，D155）。
fn render_order(sort: &[SortField]) -> String {
    if sort.is_empty() {
        return "created_at DESC, id DESC".to_string();
    }
    let mut parts: Vec<String> = sort
        .iter()
        .map(|field| {
            let direction = match field.direction {
                SortDirection::Asc => "ASC",
                SortDirection::Desc => "DESC",
            };
            format!("{} {direction}", field.column)
        })
        .collect();
    // `id` 作 tiebreaker —— `created_at` 并列时页码/游标分页会重复或遗漏（D155）。
    parts.push("id DESC".to_string());
    parts.join(", ")
}

/// 编码游标：`base64url("{createdAt}|{id}")`。
pub fn encode_cursor(created_at: &OffsetDateTime, id: &str) -> String {
    let text = format!("{}|{id}", format_timestamp_micros_utc(created_at));
    URL_SAFE_NO_PAD.encode(text.as_bytes())
}

/// 解码游标 → `(createdAt 文本, id)`。
///
/// ⚠️ `createdAt` 以**文本**返回，交给 SQL 显式转型（`?::timestamptz`）——
/// `time` crate **没开** `parsing` feature（根 `Cargo.toml` 只开了 `formatting`），
/// Rust 侧**不能**解析 RFC3339。
pub fn decode_cursor(cursor: &str) -> Result<(String, String), AuditError> {
    let raw = URL_SAFE_NO_PAD
        .decode(cursor.as_bytes())
        .map_err(|_| invalid("the cursor is not a valid cursor token"))?;
    let text =
        String::from_utf8(raw).map_err(|_| invalid("the cursor is not a valid cursor token"))?;
    let (created_at, id) = text
        .split_once('|')
        .ok_or_else(|| invalid("the cursor is not a valid cursor token"))?;
    if created_at.is_empty() || id.is_empty() {
        return Err(invalid("the cursor is not a valid cursor token"));
    }
    Ok((created_at.to_string(), id.to_string()))
}

fn unavailable(error: impl std::fmt::Display) -> AuditError {
    AuditError::SinkUnavailable {
        reason: format!("the audit query could not be executed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(items: &[(&str, &str)]) -> HashMap<String, String> {
        items
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn the_defaults_are_the_contract_defaults() {
        let parsed = AuditQueryParams::parse(&HashMap::new()).expect("应当能解析");
        assert_eq!(parsed.limit, DEFAULT_LIMIT);
        assert_eq!(parsed.limit, 20);
        assert_eq!(MAX_LIMIT, 200);
        assert!(parsed.cursor.is_none());
        assert!(parsed.page.is_none());
        assert!(parsed.sort.is_empty());
    }

    #[test]
    fn a_non_numeric_limit_is_rejected() {
        let error = AuditQueryParams::parse(&params(&[("limit", "abc")]))
            .expect_err("非数值 limit 必须被拒");
        assert_eq!(error.code(), "AUDIT_INVALID_FILTER");
    }

    #[test]
    fn a_zero_limit_is_rejected() {
        let error =
            AuditQueryParams::parse(&params(&[("limit", "0")])).expect_err("零值 limit 必须被拒");
        assert_eq!(error.code(), "AUDIT_INVALID_FILTER");
    }

    /// 超上限**拒绝**（⚠️ **REQ-019 D341 的破坏性修正**，原行为是「夹到上限 + 不报错」）。
    ///
    /// # 为什么改
    ///
    /// 静默截断会让调用方以为拿到了 1000 条 → **数据静默丢失**。
    /// 契约 C7 随之升版（`超上限 → 400`），上限**保持 200** 以减小破坏面。
    ///
    /// ⚠️ 这条测试原来是 `an_oversized_limit_is_clamped_instead_of_rejected` ——
    /// 名字与断言都必须反过来，否则「改了行为但测试还是绿的」。
    #[test]
    fn an_oversized_limit_is_rejected() {
        let error = AuditQueryParams::parse(&params(&[("limit", "1000")]))
            .expect_err("超上限必须拒绝（不再静默截断）");
        // 错误信息必须点出上限与实际值（运维要能一眼看出差多少）。
        let rendered = error.to_string();
        assert!(
            rendered.contains("200") && rendered.contains("1000"),
            "错误信息必须含上限与实际值；实际：{rendered}"
        );
    }

    /// 正好等于上限 → **通过**（边界包含，不是 `>=`）。
    #[test]
    fn a_limit_equal_to_the_maximum_is_accepted() {
        let parsed = AuditQueryParams::parse(&params(&[("limit", "200")]))
            .expect("正好 200 应通过（边界包含）");
        assert_eq!(parsed.limit, MAX_LIMIT);
    }

    /// `page_size` 超上限同样被拒（与 `limit` 同口径）。
    #[test]
    fn an_oversized_page_size_is_rejected() {
        AuditQueryParams::parse(&params(&[("page_size", "1000")]))
            .expect_err("page_size 超上限必须拒绝");
    }

    #[test]
    fn an_unknown_sort_field_is_rejected() {
        let error = AuditQueryParams::parse(&params(&[("sort", "unknownField")]))
            .expect_err("未声明的排序字段必须被拒");
        assert_eq!(error.code(), "AUDIT_INVALID_FILTER");
    }

    #[test]
    fn cursor_and_page_are_mutually_exclusive() {
        let error = AuditQueryParams::parse(&params(&[("cursor", "abc"), ("page", "1")]))
            .expect_err("cursor 与 page 必须互斥");
        assert_eq!(error.code(), "AUDIT_INVALID_FILTER");
    }

    #[test]
    fn the_filters_are_parsed_from_the_bracket_syntax() {
        let parsed = AuditQueryParams::parse(&params(&[
            ("filter[actorId][eq]", "u-1"),
            ("filter[action][contains]", "demo"),
        ]))
        .expect("应当能解析");
        assert_eq!(parsed.filters.len(), 2);
        assert!(
            parsed
                .filters
                .iter()
                .any(|filter| filter.column == "actor_id" && filter.op == FilterOp::Eq)
        );
        assert!(
            parsed
                .filters
                .iter()
                .any(|filter| filter.column == "action" && filter.op == FilterOp::Contains)
        );
    }

    #[test]
    fn an_unknown_filter_operator_is_rejected() {
        let error = AuditQueryParams::parse(&params(&[("filter[action][like]", "x")]))
            .expect_err("未声明的 op 必须被拒");
        assert_eq!(error.code(), "AUDIT_INVALID_FILTER");
    }

    #[test]
    fn the_tenant_id_parameter_is_ignored() {
        let parsed = AuditQueryParams::parse(&params(&[("tenant_id", "tenant-b")]))
            .expect("tenant_id 必须被忽略而不是报错");
        assert!(parsed.filters.is_empty());
    }

    #[test]
    fn the_cursor_round_trips() {
        let moment = OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("合法时刻");
        let cursor = encode_cursor(&moment, "evt-1");
        let (created_at, id) = decode_cursor(&cursor).expect("应当能解码");
        assert_eq!(id, "evt-1");
        assert_eq!(created_at, "2023-11-14T22:13:20.000000Z");
    }

    #[test]
    fn a_broken_cursor_is_rejected() {
        assert!(decode_cursor("!!!not base64!!!").is_err());
        assert!(decode_cursor(&URL_SAFE_NO_PAD.encode(b"no-separator")).is_err());
    }

    #[test]
    fn the_default_order_is_created_at_desc_id_desc() {
        assert_eq!(render_order(&[]), "created_at DESC, id DESC");
    }

    #[test]
    fn a_user_sort_always_gets_the_id_tiebreaker() {
        let order = render_order(&[SortField {
            column: "action",
            direction: SortDirection::Desc,
        }]);
        assert_eq!(order, "action DESC, id DESC");
    }
}
