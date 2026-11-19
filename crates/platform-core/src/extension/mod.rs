//! 扩展点：平台与业务模块之间的唯一接缝。
//!
//! # 依赖方向
//!
//! ```text
//! platform-core (定义 trait)
//!       ↑ trait
//! platform-contracts (依赖 platform-core)
//!       ↑ trait
//! 业务模块 modules/<name>/backend (依赖 platform-core + platform-contracts)
//! ```
//!
//! 平台代码永不反向依赖业务模块。
//!
//! # 首批 5 个扩展点
//!
//! | 扩展点 | 文件 | 说明 |
//! |--------|------|------|
//! | [`PlatformModule`] | [`platform_module`] | 模块注册：id / version / dependencies / setup / enabled |
//! | [`Repository`] | [`repository`] | 数据访问：find / list / insert / update / delete |
//! | [`Auditable`] | [`auditable`] | 审计埋点声明 |
//! | [`NotificationSender`] | [`notification`] | 通知投递 |
//! | [`WorkflowHandler`] | [`workflow`] | 工作流接入 |
//!
//! # 关于 `async fn` 的写法
//!
//! 签名语义与设计摘要 §2 一致（调用方写法完全相同），但 trait 侧写成
//! `-> impl Future<Output = ...> + Send` 而不是 `async fn`：
//!
//! - `async fn` 写在**公开 trait** 上会触发 `async_fn_in_trait` lint
//!   （warn 级，`clippy -D warnings` 下即失败），且无法约束返回 future 的 `Send`；
//! - 展开写法是编译器给出的等价建议，实现方**仍然可以用 `async fn` 写实现**，
//!   调用方也仍然写 `repo.find(&ctx, &id).await`。
//!
//! 这样不需要引入 `async-trait` 依赖，同时保证返回的 future 是 `Send`
//! （可安全交给 tokio 任务）。

pub mod auditable;
pub mod config;
pub mod expr;
pub mod manifest;
pub mod manifest_schema;
pub mod notification;
pub mod permission;
pub mod platform_module;
pub mod registry;
pub mod repository;
pub mod transaction;
pub mod workflow;

pub use auditable::{AuditEventSpec, Auditable};
pub use config::{ConfigResolver, ConfigSource, FlagDecision, ResolvedConfig};
pub use expr::{
    CompiledCondition, ConditionValue, EvalContext, Expr, ExprError, FieldBinding, FieldWhitelist,
    Op, Operand, ValueKind,
};
pub use manifest::{
    ConfigSpec, ConfigType, Manifest, ManifestError, MenuSpec, PermissionSpec, PermissionType,
    TenantScopedSpec, is_sql_identifier,
};
pub use notification::{DeliveryResult, NotificationMessage, NotificationSender};
pub use permission::{
    DataScope, PermissionChecker, PermissionLayer, PermissionProvider, PermissionRefresher,
    PermissionService, ScopeFilter, require_permission,
};
pub use platform_module::{ModuleContext, PlatformModule};
pub use registry::{ModuleRegistration, discover, discover_enabled};
pub use repository::{FilterOp, FilterSpec, List, ListQuery, Repository, SortSpec};
pub use transaction::{TransactionalWrite, WriteBoundary};
pub use workflow::{TransitionEvent, WorkflowHandler};
