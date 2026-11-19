//! 扩展点契约测试套件。
//!
//! # 业务模块只依赖本 crate 即可获得契约测试
//!
//! 业务模块在自己的 `tests/contracts.rs` 里接入：
//!
//! ```ignore
//! use platform_contracts::prelude::*;
//!
//! #[platform_module_contract]
//! mod order {
//!     fn contract_subject() -> ContractSubject<OrderFixture> {
//!         ContractSubject {
//!             module: Box::new(OrderModule),
//!             fixture: OrderFixture,
//!         }
//!     }
//! }
//! ```
//!
//! 宏会自动跑下列检查：
//!
//! | 检查 | 内容 |
//! |------|------|
//! | C-1 | `manifest.json` 与实现一致（id / version / enabled / dependencies） |
//! | C-2 | 权限点声明完整（代码里用的都在 manifest 里；manifest 多余的只警告） |
//! | C-3 | 租户隔离生效（多租户同名数据集下不串数据） |
//! | C-4 | 事务边界（单次写全有或全无；跨表写必须显式开事务） |
//! | C-5 | 已登记权限点必须有鉴权调用点，且鉴权入口全仓库唯一 |
//! | C-6 | 声明了字段级权限的模块，源码里必须有 `SchemaProjector::project` 调用点 |
//!
//! # C-3 为什么不需要连数据库
//!
//! 平台本期没有 DB 访问层（sqlx 推到 REQ-006/007），所以 C-3 验的是**契约语义**：
//!
//! 1. **编译期保证**：`Repository` 的每个方法都必须传 `ctx: &RequestContext`，
//!    实现方没有任何别的途径拿到租户 —— 漏传就编译不过；
//! 2. **行为验证**：套件提供两个租户的同名数据，用 `ctx` 里指定的租户调 `list()`，
//!    断言只返回该租户的数据；用该 ctx 调 `find()` 查另一个租户的 id，断言返回 `None`。

pub mod authz_scan;
pub mod checks;
pub mod fixture;
pub mod manifest_scan;
pub mod runtime;
pub mod schema_scan;
pub mod suite;

pub use checks::{
    CheckOutcome, assert_compliant, check_c1, check_c2, check_c3, check_c4, check_c5, check_c5_for,
    check_c6, check_c6_for,
};
pub use fixture::{ContractFixture, ContractSubject, Seeded};
pub use runtime::block_on;
pub use suite::{ContractCheck, ContractSuite, ContractViolation, ContractWarning, SuiteReport};

pub use platform_contracts_macros::platform_module_contract;

/// 契约测试常用导入。
pub mod prelude {
    pub use crate::checks::{
        CheckOutcome, assert_compliant, check_c1, check_c2, check_c3, check_c4, check_c5,
        check_c5_for, check_c6, check_c6_for,
    };
    pub use crate::fixture::{ContractFixture, ContractSubject, Seeded};
    pub use crate::runtime::block_on;
    pub use crate::suite::{ContractViolation, ContractWarning};
    pub use platform_contracts_macros::platform_module_contract;
    pub use platform_core::extension::transaction::WriteBoundary;
}
