//! 通知系统（REQ-015）。
//!
//! # 这个 crate 解决什么
//!
//! 统一**站内信 / 邮件 / Webhook** 三类通道的投递（短信与 IM 四类显式「未配置」，D292），
//! 让业务方自助维护模板，并提供异步投递 / 重试退避 / 死信重投 / 投递记录。
//!
//! ```text
//! platform-core ← platform-notify ← platform-server（装配层）
//!                     ↑
//!          platform-db / platform-config
//! ```
//!
//! # 三点必须知道的设计约束
//!
//! ## 1. 通道抽象**复用**已冻结的扩展点，不改 trait 签名（D291）
//!
//! [`platform_core::extension::notification::NotificationSender`] 是 REQ-003 定下的
//! 扩展点契约，`crates/xtask/src/capabilities.rs:45-51` 的 `EXTENSION_POINT_TRAITS`
//! 冻了它 → **改签名会让所有 `manifest.json` 的 `dependencies` 跟着动**。
//! 重试 / 退避 / 死信**都在编排层做**，不进 trait。
//!
//! ## 2. 五类通道 → **3 个真实现 + 2 类显式「未配置」**（D292）
//!
//! | 通道 | 状态 |
//! |---|---|
//! | `in_app`（站内信） | ✅ 真实现（落库） |
//! | `smtp`（邮件） | ✅ 真实现（**自己写的 SMTP 客户端**，D305） |
//! | `webhook` | ✅ 真实现（`hyper` + `hyper-rustls`） |
//! | `sms` | ⚠️ 显式未配置 |
//! | `dingtalk` / `wecom` / `feishu` | ⚠️ 显式未配置 |
//!
//! > **未配置 ≠ 可用**。未配置的通道 `send()` **必须返回 `Err`**
//! > （`NOTIFY_CHANNEL_NOT_CONFIGURED`），**不得**返回
//! > `Ok(DeliveryResult { delivered: false })` —— 后者会让 worker 走重试分支。
//!
//! ## 3. 模板只做**受限变量替换**（D293）
//!
//! 只支持 `{{variable}}`，变量名白名单 `^[a-z][a-z0-9_]*$`，未知变量**显式失败**。
//! **不做**条件 / 循环 / 表达式 / 部分引用 —— 租户管理员能编辑模板，
//! 支持表达式等于给他**服务端模板注入（SSTI）** 的入口。
//!
//! # ⚠️ 命名禁区（C-5，D216）
//!
//! `src/` 里**不得**出现这 8 个标识符：
//!
//! ```text
//! check_permission   check_perm     has_permission   require_perm
//! authorize          ensure_permission  verify_permission  permission_check
//! ```
//!
//! ⚠️ 通知域里「是否允许给该用户发」听起来很像鉴权 ——
//! 但它**不是**，要用别的名字（如 `is_recipient_eligible`）。
//!
//! # ⚠️ 异步投递是**数据库驱动的显式 worker**（D296）
//!
//! 不是进程内队列：web 进程重启会丢消息、多实例会各自持队列。
//! 出队靠 `SELECT ... FOR UPDATE SKIP LOCKED`，入口是 `tick()`（可单测、可 E2E 驱动）。

pub mod authz;
pub mod channels;
pub mod config;
// ⚠️ REQ-020 D354：**落库适配器** —— 把 `NotificationSender` 扩展点接到 `enqueue`。
// 平台此前只有「通道发送器」（`InAppSender` 存内存 Vec），业务发通知落不了库。
pub mod db_sender;
pub mod error;
pub mod events;
pub mod http;
pub mod model;
pub mod service;
pub mod store;
pub mod template;
pub mod worker;

pub use error::{ALL_CODES, NotifyError, ServiceError};
pub use events::ALL_CODES as ALL_EVENT_CODES;

/// 通知系统的版本号（与 `Cargo.toml` 中的 `version` 保持一致）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_exposed() {
        assert!(!VERSION.is_empty());
    }

    /// `ALL_CODES` **恰好 8 条**（AC-S-174 步骤 1c：`TITLES` 99 → 107）。
    #[test]
    fn the_error_code_list_is_exported() {
        assert_eq!(ALL_CODES.len(), 8);
    }

    /// 事件码**恰好 4 条**（AC-S-174 步骤 5）。
    #[test]
    fn the_event_code_list_is_exported() {
        assert_eq!(ALL_EVENT_CODES.len(), 4);
    }

    /// 错误码与事件码是**两个互斥的集合**（D223：事件码不是 HTTP 错误码）。
    #[test]
    fn error_and_event_codes_are_disjoint() {
        for event in ALL_EVENT_CODES {
            assert!(
                !ALL_CODES.contains(event),
                "事件码不得出现在错误码清单里：{event}"
            );
        }
    }
}
