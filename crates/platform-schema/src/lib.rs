//! 元数据读取能力（REQ-012）。
//!
//! # 职责
//!
//! | 模块 | 内容 |
//! |------|------|
//! | [`registry`] | 扫全部 `manifest.json` → 实体元数据索引 + **阶段 2 全局校验** |
//! | [`http`] | `GET /api/schema/<moduleId>/<entityKey>`（已认证 + 字段级过滤） |
//!
//! # 依赖方向
//!
//! ```text
//! platform-core (元数据形状 + SchemaProjector)
//!       ↑
//! platform-schema (注册表 + 元数据接口)
//!       ↑
//! platform-server (装配：注入令牌校验与权限判定)
//! ```
//!
//! 本 crate **不被**任何业务模块依赖 —— 业务模块只需要
//! [`platform_core::extension::schema::SchemaProjector`]（那是核心里的类型）。
//!
//! # 为什么不放进 `platform-rbac`
//!
//! `platform-rbac` 是**权限模块**（角色、授权、数据范围）。元数据是**界面描述**，
//! 只是恰好要用权限判定来过滤字段 —— 把两者塞进一个 crate 会让
//! 「权限实现」与「元数据实现」互相牵制，也会让 `platform-rbac` 的依赖面变宽。
//!
//! # 启动即失败（阶段 2）
//!
//! 跨文件的一致性（字段权限点是否已登记、`reference` 指向的实体是否存在）
//! **只能**在所有 manifest 都加载完之后判 —— 见 [`registry::SchemaRegistry::from_manifest_root`]。
//! 失败 → 进程起不来，错误逐条点名「谁的哪个引用指向了不存在的东西」。

pub mod http;
pub mod registry;

pub use http::{AuthError, SchemaState, TokenVerifier, VerifiedToken, router};
pub use registry::{EntityLocation, ManifestIssue, SchemaLoadError, SchemaRegistry};
