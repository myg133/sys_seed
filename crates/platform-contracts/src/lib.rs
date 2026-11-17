//! 扩展点契约测试套件骨架。
//!
//! 本期只提供**框架**：业务模块实现同一套检查，即视为通过平台契约。
//! 具体的扩展点 trait 由 REQ-003 定义，本 crate 不预先规定业务语义。

pub mod suite;

pub use suite::{ContractCheck, ContractSuite, ContractViolation, SuiteReport};
