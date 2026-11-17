//! 沙箱模块契约测试：C-1 ~ C-4 由 `#[platform_module_contract]` 宏自动生成。
//!
//! 跑法：
//!
//! ```bash
//! cargo test -p sandbox -- --test-threads=2
//! ```
//!
//! C-2 的告警（manifest 声明了代码没用到的权限点）默认被测试框架捕获，
//! 需要看到时加 `-- --nocapture`。

use std::path::PathBuf;

use anyhow::Result;
use platform_contracts::prelude::*;
use sandbox::entity::DemoItem;
use sandbox::repository::InMemoryRepository;

/// 沙箱模块的契约夹具：两个租户、**同名**数据，唯一区别是租户。
struct SandboxFixture;

impl ContractFixture for SandboxFixture {
    type Item = DemoItem;
    type Id = String;
    type Repository = InMemoryRepository;

    fn manifest_path(&self) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
    }

    fn backend_dir(&self) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("backend")
    }

    fn seed(&self) -> Result<Vec<Seeded<DemoItem, String>>> {
        Ok(vec![
            seeded("tenant-a", "user_a@example.com", "demo-1", "同名记录"),
            seeded("tenant-b", "user_b@example.com", "demo-2", "同名记录"),
        ])
    }

    fn repository(&self, seeds: Vec<Seeded<DemoItem, String>>) -> InMemoryRepository {
        let repo = InMemoryRepository::new();
        repo.load(seeds.into_iter().map(|seed| seed.item));
        repo
    }

    fn tenant_of<'a>(&self, item: &'a DemoItem) -> &'a str {
        &item.tenant_id
    }

    fn fingerprint(&self, item: &DemoItem) -> String {
        format!("{}|{}|{}", item.id, item.title, item.revision)
    }

    fn write_boundary(&self) -> WriteBoundary {
        // 单表存储：每次写操作本身就是一个事务。
        WriteBoundary::new(["sandbox_demo_item"], false)
    }

    fn fresh_item(&self, tenant_id: &str, revision: u32) -> Result<(String, DemoItem)> {
        let id = format!("demo-{tenant_id}-new");
        let item = DemoItem {
            id: id.clone(),
            tenant_id: tenant_id.to_string(),
            title: format!("写操作测试 {revision}"),
            revision,
        };
        Ok((id, item))
    }
}

/// 造一条夹具数据。
fn seeded(tenant_id: &str, user_id: &str, id: &str, title: &str) -> Seeded<DemoItem, String> {
    Seeded {
        tenant_id: tenant_id.to_string(),
        space_id: None,
        user_id: user_id.to_string(),
        id: id.to_string(),
        item: DemoItem {
            id: id.to_string(),
            tenant_id: tenant_id.to_string(),
            title: title.to_string(),
            revision: 0,
        },
    }
}

#[platform_module_contract]
mod sandbox_contract {
    use super::*;

    /// 契约被测对象：模块实现 + 夹具。
    fn contract_subject() -> ContractSubject<SandboxFixture> {
        ContractSubject {
            module: Box::new(sandbox::SandboxModule),
            fixture: SandboxFixture,
        }
    }
}
