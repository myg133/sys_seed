//! 用户与组织架构（REQ-008）。
//!
//! # 交付边界（design-summary §9）
//!
//! **做**
//!
//! - 组织树（邻接表 + 递归 CTE）、成员关系、岗位、汇报关系；
//! - SCIM 2.0 **出站**（User / Group，幂等 PUT，**异步**推送）；
//! - LDAP / AD **入站**（增量 + 全量对账，字段级映射，**位运算**离职判定）；
//! - 同步冲突队列（**字段级**）+ 人工裁决；
//! - 离职自动禁用（含**撤销全部会话**）；
//! - 组织变更事件 → `NotificationSender`。
//!
//! **不做**
//!
//! - 同步的管理界面（数据层 + API 先做）；
//! - SCIM **入站**；
//! - 组织架构的多版本历史；
//! - 跨租户的组织架构共享；
//! - **任何权限判定逻辑**（判定在 REQ-009，D11）。
//!
//! # 关键决策（不可自行更改）
//!
//! | # | 决策 |
//! |---|------|
//! | D1 | 组织树 = 邻接表（`parent_id`），查子树用 PostgreSQL 递归 CTE（**不用 ltree**） |
//! | D2 | 节点类型只有 `company` / `dept` / `team`；**岗位不是节点**，是挂在部门上的属性 |
//! | D3 | **汇报关系是人的属性**（`user.manager_id`）；用户 ↔ 节点是多对多 |
//! | D5 | `path_labels` 是**加速字段**，权威是 `parent_id`；不一致时以 `parent_id` 为准并可重建 |
//! | D6 | SCIM 出站**自己实现**，**PUT 幂等**，**推送必须异步** |
//! | D7 | LDAP 入站用 `ldap3`；映射**外置为配置**；离职检测用**位运算表达式** |
//! | D8 | 冲突策略默认 `manual`；冲突记录到**字段级**；**绝不静默覆盖** |
//! | D9 | 离职自动禁用：禁用账号 + **撤销全部 session** + 解绑 + 审计 + 通知 |
//! | D10 | 组织变更事件 → REQ-003 的 `NotificationSender` 扩展点 |
//! | D11 | 本模块**不实现任何权限判定逻辑** |

pub mod audit;
pub mod config;
pub mod entity;
pub mod error;
pub mod http;
pub mod ldap;
pub mod notify;
pub mod scim;
pub mod service;
pub mod store;
pub mod sync;
pub mod tree;

pub use audit::{AUDIT_EVENTS, AuditRecorder, NoopAuditRecorder, OrgAuditEvent};
pub use entity::{
    ConflictResolution, ConflictStatus, NodeType, OrgNode, SyncConflict, SyncSummary, UserOrg,
    UserPosition,
};
pub use error::{OrgError, code as error_code};
pub use notify::{NoopNotifier, Notifier, OrgEvent, OrgEventKind, PlatformNotifier};
pub use scim::{
    ScimGateway, ScimGroup, ScimJob, ScimJobEntry, ScimJobOutcome, ScimOutbox, ScimProviderConfig,
    ScimUser, ScimUserDraft,
};
pub use service::{DepartureOutcome, NewMembership, NewNode, NewPosition, OrgService};
pub use store::OrgStore;
pub use sync::{ConflictStrategy, SyncField};

use anyhow::Result;
use platform_core::extension::auditable::{AuditEventSpec, Auditable};
use platform_core::extension::platform_module::{ModuleContext, PlatformModule};

/// 组织架构模块的实现。
#[derive(Debug, Clone, Copy, Default)]
pub struct OrgModule;

impl PlatformModule for OrgModule {
    /// 必须与 `manifest.json` 的 `id` 一致（契约检查 C-1）。
    fn id(&self) -> &'static str {
        "org"
    }

    /// 必须与 `manifest.json` 的 `version` 一致（契约检查 C-1）。
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// 依赖的其它模块 ID。
    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    /// 启动时调用一次。
    fn setup(&self, ctx: &ModuleContext) -> Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// 是否启用。
    fn enabled(&self) -> bool {
        true
    }
}

impl Auditable for OrgModule {
    /// 本模块产生的审计事件（平台据此自动埋点）。
    fn audit_events(&self) -> &[AuditEventSpec] {
        &AUDIT_EVENTS
    }
}

/// 组织架构模块实例（编译期注册用）。
static ORG_MODULE: OrgModule = OrgModule;

platform_core::register_platform_module!(ORG_MODULE);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_module_id_matches_the_manifest() {
        assert_eq!(ORG_MODULE.id(), "org");
    }

    #[test]
    fn the_audit_events_are_declared() {
        let events = OrgModule.audit_events();
        assert!(!events.is_empty());
        assert!(events.iter().all(|event| event.code.starts_with("org.")));
    }

    #[test]
    fn the_module_is_discoverable_through_the_platform_registry() {
        let modules = platform_core::extension::registry::discover();
        assert!(modules.iter().any(|module| module.id() == "org"));
    }
}
