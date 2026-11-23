//! 可观测性端点的装配（REQ-019）。
//!
//! ⚠️ 实现**不在本 crate** —— 它在 `platform_observability::http`。
//! 理由：`xtask` 要引用 `ApiDoc` 生成 OpenAPI，而 `xtask` 不得依赖
//! `platform-server`（会经它间接依赖业务模块 `sandbox`，违反 D103）。
//!
//! 本模块只做**装配**（把 crate 的 router 挂到 server 上）。

pub use platform_observability::http::{router, StatusState};
