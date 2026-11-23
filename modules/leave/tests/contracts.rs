//! 契约测试：C-1 ~ C-6 由 `#[platform_module_contract]` 宏自动生成。
//!
//! 跑法：
//!
//! ```bash
//! cargo test -p leave --test contracts -- --test-threads=1
//! ```
//!
//! C-2 的告警（manifest 声明了代码没用到的权限点）默认被测试框架捕获，
//! 需要看到时加 `-- --nocapture`。
//!
//! # 为什么夹具里另写一个内存仓储
//!
//! C-3 / C-4 要的是 `Repository` trait 的**行为**（租户隔离 / 事务边界），
//! 与「表怎么建」无关。契约套件不连数据库，所以夹具给一个内存实现；
//! 真库行为由第三批的集成测试覆盖（应用角色 + RLS）。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::{Result, anyhow};
use platform_contracts::prelude::*;
use platform_core::context::RequestContext;
use platform_core::extension::repository::{List, ListQuery, Repository};
use time::{Date, Month, OffsetDateTime};

use leave::LeaveModule;
use leave::entity::LeaveRequest;

/// 内存仓储：按 `ctx` 里的租户严格过滤（与沙箱同一个范式）。
#[derive(Debug, Default)]
struct InMemoryLeaveRepository {
    items: RwLock<BTreeMap<String, LeaveRequest>>,
}

impl InMemoryLeaveRepository {
    fn new() -> Self {
        Self::default()
    }

    fn load(&self, items: impl IntoIterator<Item = LeaveRequest>) {
        let mut guard = self.write();
        for item in items {
            guard.insert(item.id.clone(), item);
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, BTreeMap<String, LeaveRequest>> {
        self.items.read().unwrap_or_else(|error| error.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, BTreeMap<String, LeaveRequest>> {
        self.items
            .write()
            .unwrap_or_else(|error| error.into_inner())
    }
}

impl Repository<LeaveRequest> for InMemoryLeaveRepository {
    type Id = String;

    async fn find(&self, ctx: &RequestContext, id: &Self::Id) -> Result<Option<LeaveRequest>> {
        let tenant = tenant_of(ctx)?;
        Ok(self
            .read()
            .get(id)
            .filter(|item| item.tenant_id == tenant)
            .cloned())
    }

    async fn list(&self, ctx: &RequestContext, query: &ListQuery) -> Result<List<LeaveRequest>> {
        let tenant = tenant_of(ctx)?;
        let limit = query.effective_limit() as usize;
        let mut rows: Vec<LeaveRequest> = self
            .read()
            .values()
            .filter(|item| item.tenant_id == tenant)
            .cloned()
            .collect();
        rows.truncate(limit);
        Ok(List::new(rows))
    }

    async fn insert(&self, ctx: &RequestContext, item: &LeaveRequest) -> Result<LeaveRequest> {
        let tenant = tenant_of(ctx)?;
        if item.tenant_id != tenant {
            return Err(anyhow!(
                "the record tenant does not match the tenant in the request context"
            ));
        }
        let mut items = self.write();
        if items.contains_key(&item.id) {
            // C-4：主键重复必须报错，**不得**覆盖既有数据。
            return Err(anyhow!("duplicate primary key: {}", item.id));
        }
        items.insert(item.id.clone(), item.clone());
        Ok(item.clone())
    }

    async fn update(&self, ctx: &RequestContext, item: &LeaveRequest) -> Result<LeaveRequest> {
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

/// 租户只从 `ctx` 取（没有第二条途径）。
fn tenant_of(ctx: &RequestContext) -> Result<&str> {
    ctx.tenant_id()
        .ok_or_else(|| anyhow!("the tenant is not bound in the request context"))
}

/// 请假模块的契约夹具：两个租户、**同名**数据，唯一区别是租户。
struct LeaveFixture;

impl ContractFixture for LeaveFixture {
    type Item = LeaveRequest;
    type Id = String;
    type Repository = InMemoryLeaveRepository;

    fn manifest_path(&self) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
    }

    fn backend_dir(&self) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("backend")
    }

    fn seed(&self) -> Result<Vec<Seeded<LeaveRequest, String>>> {
        Ok(vec![
            seeded("tenant-a", "user_a@example.com", "leave-a"),
            seeded("tenant-b", "user_b@example.com", "leave-b"),
        ])
    }

    fn repository(&self, seeds: Vec<Seeded<LeaveRequest, String>>) -> InMemoryLeaveRepository {
        let repository = InMemoryLeaveRepository::new();
        repository.load(seeds.into_iter().map(|seed| seed.item));
        repository
    }

    fn tenant_of<'a>(&self, item: &'a LeaveRequest) -> &'a str {
        &item.tenant_id
    }

    fn fingerprint(&self, item: &LeaveRequest) -> String {
        format!(
            "{}|{}|{}|{}",
            item.id,
            item.reason,
            item.status.as_str(),
            item.starts_on
        )
    }

    fn write_boundary(&self) -> WriteBoundary {
        // 单表存储：每次写操作本身就是一个事务。
        WriteBoundary::new(["leave_request"], false)
    }

    fn fresh_item(&self, tenant_id: &str, revision: u32) -> Result<(String, LeaveRequest)> {
        let id = format!("leave-{tenant_id}-new");
        let day = Date::from_calendar_date(2026, Month::December, 1)
            .map_err(|error| anyhow!("invalid fixture date: {error}"))?;
        let mut item = LeaveRequest::new(
            tenant_id,
            format!("{tenant_id}-applicant"),
            day,
            day,
            format!("write test {revision}"),
            OffsetDateTime::UNIX_EPOCH,
        );
        // `LeaveRequest::new` 会生成随机 UUID；夹具要求主键**固定**（C-4 才能
        // 验证「先插再改是同一条」）。
        item.id = id.clone();
        Ok((id, item))
    }
}

/// 造一条夹具数据（**两个租户的同名数据**，只有 id 不同）。
fn seeded(tenant_id: &str, user_id: &str, id: &str) -> Seeded<LeaveRequest, String> {
    let day = Date::from_calendar_date(2026, Month::December, 1).expect("合法日期");
    let mut item = LeaveRequest::new(
        tenant_id,
        user_id,
        day,
        day,
        "same-named record",
        OffsetDateTime::UNIX_EPOCH,
    );
    item.id = id.to_string();
    Seeded {
        tenant_id: tenant_id.to_string(),
        space_id: None,
        user_id: user_id.to_string(),
        id: id.to_string(),
        item,
    }
}

#[platform_module_contract]
mod leave_contract {
    use super::*;

    /// 契约被测对象：模块实现 + 夹具。
    fn contract_subject() -> ContractSubject<LeaveFixture> {
        ContractSubject {
            module: Box::new(LeaveModule),
            fixture: LeaveFixture,
        }
    }
}
