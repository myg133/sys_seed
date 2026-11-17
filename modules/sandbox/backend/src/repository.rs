//! 沙箱模块的数据访问实现（内存存储，**不引入 sqlx**）。
//!
//! # 租户隔离怎么保证
//!
//! 租户只从 `ctx` 里取（`RequestContext::tenant_id`），实现里没有任何别的途径拿到租户，
//! 因此**不可能**绕过 `ctx` 自己拼 `tenant_id` 过滤条件 —— 这是编译期保证。
//! 契约检查 C-3 再用两个租户的同名数据做行为验证：漏过滤就直接红。
//!
//! # 事务边界
//!
//! 单表存储，每次 `insert` / `update` / `delete` 都是一个原子操作
//! （先校验、后落库，失败时状态不变）—— 这是契约检查 C-4 的行为口径。

use std::collections::BTreeMap;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::{Result, anyhow};
use platform_core::context::RequestContext;
use platform_core::extension::repository::{
    FilterOp, FilterSpec, List, ListQuery, Repository, SortSpec,
};

use crate::entity::DemoItem;

/// 允许排序的字段白名单。
pub const SORTABLE_FIELDS: [&str; 3] = ["id", "title", "revision"];

/// 允许过滤的字段白名单。
pub const FILTERABLE_FIELDS: [&str; 2] = ["id", "title"];

/// 内存仓储：按 `ctx` 里的租户严格过滤。
#[derive(Debug, Default)]
pub struct InMemoryRepository {
    items: RwLock<BTreeMap<String, DemoItem>>,
}

impl InMemoryRepository {
    /// 空存储。
    pub fn new() -> Self {
        Self::default()
    }

    /// 灌入初始数据（演示与测试用）。
    pub fn load(&self, items: impl IntoIterator<Item = DemoItem>) {
        let mut guard = self.write();
        for item in items {
            guard.insert(item.id.clone(), item);
        }
    }

    /// 某租户下的记录条数。
    pub fn tenant_count(&self, tenant_id: &str) -> usize {
        self.read()
            .values()
            .filter(|item| item.tenant_id == tenant_id)
            .count()
    }

    fn read(&self) -> RwLockReadGuard<'_, BTreeMap<String, DemoItem>> {
        self.items.read().unwrap_or_else(|err| err.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, BTreeMap<String, DemoItem>> {
        self.items.write().unwrap_or_else(|err| err.into_inner())
    }
}

impl Repository<DemoItem> for InMemoryRepository {
    type Id = String;

    async fn find(&self, ctx: &RequestContext, id: &Self::Id) -> Result<Option<DemoItem>> {
        let tenant = tenant_of(ctx)?;
        let items = self.read();
        Ok(items
            .get(id)
            .filter(|item| item.tenant_id == tenant)
            .cloned())
    }

    async fn list(&self, ctx: &RequestContext, query: &ListQuery) -> Result<List<DemoItem>> {
        let tenant = tenant_of(ctx)?;
        let items = self.read();
        let mut rows: Vec<DemoItem> = items
            .values()
            .filter(|item| item.tenant_id == tenant)
            .filter(|item| matches_filters(item, &query.filters))
            .cloned()
            .collect();
        sort_rows(&mut rows, &query.sort)?;
        rows.truncate(query.effective_limit() as usize);
        Ok(List::new(rows))
    }

    async fn insert(&self, ctx: &RequestContext, item: &DemoItem) -> Result<DemoItem> {
        let tenant = tenant_of(ctx)?;
        if item.tenant_id != tenant {
            return Err(anyhow!(
                "the record tenant does not match the tenant in the request context"
            ));
        }
        let mut items = self.write();
        if items.contains_key(&item.id) {
            return Err(anyhow!("duplicate primary key: {}", item.id));
        }
        items.insert(item.id.clone(), item.clone());
        Ok(item.clone())
    }

    async fn update(&self, ctx: &RequestContext, item: &DemoItem) -> Result<DemoItem> {
        let tenant = tenant_of(ctx)?;
        if item.tenant_id != tenant {
            return Err(anyhow!(
                "the record tenant does not match the tenant in the request context"
            ));
        }
        let mut items = self.write();
        match items.get(&item.id) {
            Some(existing) if existing.tenant_id == tenant => {}
            _ => return Err(anyhow!("record not found: {}", item.id)),
        }
        items.insert(item.id.clone(), item.clone());
        Ok(item.clone())
    }

    async fn delete(&self, ctx: &RequestContext, id: &Self::Id) -> Result<()> {
        let tenant = tenant_of(ctx)?;
        let mut items = self.write();
        match items.get(id) {
            Some(existing) if existing.tenant_id == tenant => {
                items.remove(id);
                Ok(())
            }
            _ => Err(anyhow!("record not found: {id}")),
        }
    }
}

/// 从请求上下文取租户；没有租户就是上下文不完整，直接报错。
fn tenant_of(ctx: &RequestContext) -> Result<&str> {
    ctx.tenant_id()
        .ok_or_else(|| anyhow!("the tenant is not bound in the request context"))
}

/// 按排序项排序；白名单外的字段直接报错（`api-conventions v1` C7）。
///
/// 排序字段必须先声明再使用 —— 静默忽略会让调用方以为排序生效了。
fn sort_rows(rows: &mut [DemoItem], sort: &[SortSpec]) -> Result<()> {
    for spec in sort.iter().rev() {
        if !SORTABLE_FIELDS.contains(&spec.field.as_str()) {
            return Err(anyhow!("sort field {} is not sortable", spec.field));
        }
        rows.sort_by(|left, right| {
            let ordering = match spec.field.as_str() {
                "id" => left.id.cmp(&right.id),
                "title" => left.title.cmp(&right.title),
                "revision" => left.revision.cmp(&right.revision),
                _ => std::cmp::Ordering::Equal,
            };
            if spec.descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    Ok(())
}

/// 所有过滤条件都命中才算通过。
fn matches_filters(item: &DemoItem, filters: &[FilterSpec]) -> bool {
    filters.iter().all(|filter| matches_filter(item, filter))
}

/// 单个过滤条件；白名单外的字段被忽略。
fn matches_filter(item: &DemoItem, filter: &FilterSpec) -> bool {
    if !FILTERABLE_FIELDS.contains(&filter.field.as_str()) {
        return true;
    }
    let value = match filter.field.as_str() {
        "id" => item.id.as_str(),
        "title" => item.title.as_str(),
        _ => return true,
    };
    match filter.op {
        FilterOp::Eq | FilterOp::In => filter.values.iter().any(|candidate| candidate == value),
        FilterOp::Ne => filter.values.iter().all(|candidate| candidate != value),
        FilterOp::Contains => filter
            .values
            .iter()
            .any(|candidate| value.contains(candidate.as_str())),
        FilterOp::Range => in_range(value, &filter.values),
    }
}

/// 区间过滤：取最小值与最大值做闭区间比较。
fn in_range(value: &str, values: &[String]) -> bool {
    let (Some(low), Some(high)) = (values.first(), values.last()) else {
        return true;
    };
    value >= low.as_str() && value <= high.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::extension::repository::SortSpec;
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    /// 极简阻塞等待：把 future 跑完（本模块不引入异步运行时）。
    fn block_on<F: Future>(future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::hint::spin_loop(),
            }
        }
    }

    /// 带租户的请求上下文。
    fn context() -> RequestContext {
        RequestContext::new().with_tenant("tenant-a")
    }

    /// 一条属于 `tenant-a` 的记录。
    fn item(id: &str, title: &str) -> DemoItem {
        DemoItem {
            id: id.to_string(),
            tenant_id: "tenant-a".to_string(),
            title: title.to_string(),
            revision: 0,
        }
    }

    #[test]
    fn list_rejects_a_sort_field_outside_the_whitelist() {
        let repository = InMemoryRepository::new();
        repository.load([item("demo-1", "B"), item("demo-2", "A")]);
        let query = ListQuery {
            sort: vec![SortSpec::asc("unknown")],
            ..ListQuery::default()
        };

        let error = block_on(repository.list(&context(), &query))
            .expect_err("白名单外的排序字段必须报错，不能静默忽略");
        assert!(
            error
                .to_string()
                .contains("sort field unknown is not sortable"),
            "error = {error}"
        );
    }

    #[test]
    fn list_sorts_by_a_declared_field() {
        let repository = InMemoryRepository::new();
        repository.load([item("demo-1", "B"), item("demo-2", "A")]);
        let query = ListQuery {
            sort: vec![SortSpec::asc("title")],
            ..ListQuery::default()
        };

        let list = block_on(repository.list(&context(), &query)).expect("白名单内的排序字段可用");
        let ids: Vec<&str> = list.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["demo-2", "demo-1"]);
    }
}
