//! 沙箱模块的业务服务层。
//!
//! 约定：业务模块里**只有** service 层可以调用 Repository；
//! 租户过滤由 Repository 按 `ctx` 完成，service 不再自己拼 `tenant_id` 条件。

use anyhow::{Result, anyhow};
use platform_core::context::RequestContext;
use platform_core::extension::repository::{List, ListQuery, Repository};

use crate::entity::DemoItem;
use crate::repository::InMemoryRepository;

/// 列出当前租户的示例记录。
pub async fn list_items(
    repo: &InMemoryRepository,
    ctx: &RequestContext,
    query: &ListQuery,
) -> Result<List<DemoItem>> {
    repo.list(ctx, query).await
}

/// 新建一条示例记录（主键由调用方给出）。
pub async fn create_item(
    repo: &InMemoryRepository,
    ctx: &RequestContext,
    id: &str,
    title: &str,
) -> Result<DemoItem> {
    let tenant = tenant_of(ctx)?;
    let item = DemoItem {
        id: id.to_string(),
        tenant_id: tenant.to_string(),
        title: title.to_string(),
        revision: 0,
    };
    repo.insert(ctx, &item).await
}

/// 改标题（视为一次新版本写入）。
pub async fn rename_item(
    repo: &InMemoryRepository,
    ctx: &RequestContext,
    id: &str,
    title: &str,
) -> Result<DemoItem> {
    let tenant = tenant_of(ctx)?;
    let item = DemoItem {
        id: id.to_string(),
        tenant_id: tenant.to_string(),
        title: title.to_string(),
        revision: 1,
    };
    repo.update(ctx, &item).await
}

/// 删除一条示例记录。
pub async fn delete_item(repo: &InMemoryRepository, ctx: &RequestContext, id: &str) -> Result<()> {
    repo.delete(ctx, &id.to_string()).await
}

fn tenant_of(ctx: &RequestContext) -> Result<&str> {
    ctx.tenant_id()
        .ok_or_else(|| anyhow!("the tenant is not bound in the request context"))
}
