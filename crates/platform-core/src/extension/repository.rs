//! 扩展点 2：数据访问。
//!
//! # 租户隔离靠编译期保证
//!
//! [`Repository`] 的每个方法都强制带 `ctx: &RequestContext`，而租户只存在于 `ctx` 里。
//! 实现方**没有任何别的途径拿到租户**，也就无法绕过 `ctx` 自己拼 `tenant_id` 过滤条件
//! （`api-conventions v1` C3）。契约测试 C-3 再用两个租户的同名数据做行为验证：
//! 漏过滤就直接红。
//!
//! 签名语义与设计摘要 §2 一致；trait 侧展开为 `-> impl Future<Output = ...> + Send`，
//! 实现方仍可用 `async fn` 书写，调用方写法不变（`repo.find(&ctx, &id).await`）。

use std::future::Future;

use anyhow::Result;

use crate::context::RequestContext;

/// 平台默认页大小（`api-conventions v1` C7）。
pub const DEFAULT_PAGE_SIZE: u32 = 20;

/// 平台页大小上限（`api-conventions v1` C7）。
pub const MAX_PAGE_SIZE: u32 = 200;

/// 数据访问扩展点。
pub trait Repository<T: Send + Sync + 'static>: Send + Sync {
    /// 实体主键类型。
    type Id: Send + Sync;

    /// 按主键查询。
    ///
    /// 命中其它租户的数据时必须返回 `None`（不泄露资源存在性，`api-conventions v1` C5）。
    fn find(
        &self,
        ctx: &RequestContext,
        id: &Self::Id,
    ) -> impl Future<Output = Result<Option<T>>> + Send;

    /// 按查询条件列出**当前租户**的数据。
    fn list(
        &self,
        ctx: &RequestContext,
        q: &ListQuery,
    ) -> impl Future<Output = Result<List<T>>> + Send;

    /// 插入一条数据。
    ///
    /// 主键重复时必须返回错误，**不得**覆盖既有数据 —— 一次写操作必须全有或全无
    /// （契约检查 C-4）。
    fn insert(&self, ctx: &RequestContext, item: &T) -> impl Future<Output = Result<T>> + Send;

    /// 更新一条数据。
    fn update(&self, ctx: &RequestContext, item: &T) -> impl Future<Output = Result<T>> + Send;

    /// 删除一条数据。
    fn delete(
        &self,
        ctx: &RequestContext,
        id: &Self::Id,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// 列表查询参数。
///
/// 游标分页为主：`cursor` + `limit`（默认 20，上限 200），`api-conventions v1` C7。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListQuery {
    /// 游标起点；`None` 表示从头开始。
    pub cursor: Option<String>,
    /// 每页条数；`None` 表示用平台默认值。
    pub limit: Option<u32>,
    /// 排序项。
    pub sort: Vec<SortSpec>,
    /// 过滤条件。
    pub filters: Vec<FilterSpec>,
}

impl ListQuery {
    /// 实际生效的页大小：缺省 20，上限 200。
    pub fn effective_limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE)
    }
}

/// 排序项：字段名 + 是否降序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortSpec {
    /// 排序字段（由各模块声明白名单）。
    pub field: String,
    /// 是否降序。
    pub descending: bool,
}

impl SortSpec {
    /// 升序。
    pub fn asc(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            descending: false,
        }
    }

    /// 降序。
    pub fn desc(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            descending: true,
        }
    }
}

/// 过滤算子白名单（`api-conventions v1` C7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    /// 等于。
    Eq,
    /// 不等于。
    Ne,
    /// 属于。
    In,
    /// 包含。
    Contains,
    /// 区间。
    Range,
}

/// 过滤条件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterSpec {
    /// 过滤字段（由各模块声明白名单）。
    pub field: String,
    /// 过滤算子。
    pub op: FilterOp,
    /// 参与比较的值。
    pub values: Vec<String>,
}

impl FilterSpec {
    /// 构造一个过滤条件。
    pub fn new(
        field: impl Into<String>,
        op: FilterOp,
        values: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            field: field.into(),
            op,
            values: values.into_iter().map(Into::into).collect(),
        }
    }
}

/// 一页数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List<T> {
    /// 本页数据。
    pub items: Vec<T>,
    /// 下一页游标；没有下一页时为 `None`。
    pub next_cursor: Option<String>,
}

impl<T> List<T> {
    /// 由数据构造一页结果。
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            next_cursor: None,
        }
    }

    /// 空页。
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// 本页条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空页。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_limit_falls_back_to_default_and_caps_at_max() {
        assert_eq!(ListQuery::default().effective_limit(), DEFAULT_PAGE_SIZE);
        assert_eq!(
            ListQuery {
                limit: Some(5),
                ..Default::default()
            }
            .effective_limit(),
            5
        );
        assert_eq!(
            ListQuery {
                limit: Some(9999),
                ..Default::default()
            }
            .effective_limit(),
            MAX_PAGE_SIZE
        );
    }

    #[test]
    fn sort_spec_builds_ascending_and_descending() {
        assert!(!SortSpec::asc("title").descending);
        assert!(SortSpec::desc("title").descending);
        assert_eq!(SortSpec::asc("title").field, "title");
    }

    #[test]
    fn filter_spec_collects_values() {
        let spec = FilterSpec::new("title", FilterOp::In, ["a", "b"]);
        assert_eq!(spec.field, "title");
        assert_eq!(spec.op, FilterOp::In);
        assert_eq!(spec.values, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn empty_list_has_no_cursor() {
        let list = List::<u32>::empty();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert!(list.next_cursor.is_none());
    }
}
