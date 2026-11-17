//! 契约测试夹具：模块侧提供多租户数据与被测仓储。
//!
//! 夹具是业务模块与契约套件之间**唯一**的连接点 —— 套件不关心模块的业务语义，
//! 只要求模块能提供：两个租户的同名数据、被测仓储、内容指纹与写边界声明。

use std::path::PathBuf;

use anyhow::Result;

use platform_core::extension::manifest::Manifest;
use platform_core::extension::platform_module::PlatformModule;
use platform_core::extension::repository::Repository;
use platform_core::extension::transaction::WriteBoundary;

/// 一条契约夹具数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seeded<T, I> {
    /// 归属租户。
    pub tenant_id: String,
    /// 空间；`None` 表示不限空间。
    pub space_id: Option<String>,
    /// 数据归属人。
    pub user_id: String,
    /// 主键。
    pub id: I,
    /// 实体。
    pub item: T,
}

/// 契约测试夹具。
pub trait ContractFixture: Send + Sync {
    /// 实体类型。
    type Item: Send + Sync + 'static;

    /// 主键类型。
    type Id: Send + Sync + Clone + 'static;

    /// 被测仓储。
    type Repository: Repository<Self::Item, Id = Self::Id> + Send + Sync;

    /// 本模块 `manifest.json` 的路径。
    fn manifest_path(&self) -> PathBuf;

    /// 本模块后端源码目录（C-2 扫描权限点用）。
    fn backend_dir(&self) -> PathBuf;

    /// 造多租户数据集：两个租户的同名数据，**唯一区别是租户**。
    fn seed(&self) -> Result<Vec<Seeded<Self::Item, Self::Id>>>;

    /// 用种子构建被测仓储。
    fn repository(&self, seeds: Vec<Seeded<Self::Item, Self::Id>>) -> Self::Repository;

    /// 实体归属的租户。
    fn tenant_of<'a>(&self, item: &'a Self::Item) -> &'a str;

    /// 实体内容的指纹 —— 用于判断一次写操作是否真的生效。
    fn fingerprint(&self, item: &Self::Item) -> String;

    /// 写边界声明（事务边界）。
    fn write_boundary(&self) -> WriteBoundary;

    /// 造一条用于写操作测试的实体。
    ///
    /// 约定：同一个 `tenant_id` 下主键固定，`revision` 只改内容不改主键 ——
    /// 这样 C-4 才能验证「先插入、再更新」是同一条记录。
    fn fresh_item(&self, tenant_id: &str, revision: u32) -> Result<(Self::Id, Self::Item)>;
}

/// 契约被测对象。
pub struct ContractSubject<F: ContractFixture> {
    /// 被测模块实现。
    pub module: Box<dyn PlatformModule>,
    /// 被测模块的夹具。
    pub fixture: F,
}

impl<F: ContractFixture> ContractSubject<F> {
    /// 读取被测模块的 `manifest.json`。
    pub fn manifest(&self) -> Result<Manifest> {
        let path = self.fixture.manifest_path();
        Manifest::from_path(&path).map_err(anyhow::Error::from)
    }
}
