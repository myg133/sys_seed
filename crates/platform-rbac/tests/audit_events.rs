//! RBAC 权限变更的**审计埋点**用例（REQ-014 AC-S-128 / D163）。
//!
//! # 覆盖的事件码与形状（**照抄** D163）
//!
//! | 事件 | 事件码 | object_type | before | after |
//! |------|--------|-------------|--------|-------|
//! | 角色创建 | `rbac.role.create` | `role` | NULL | `{"permissions":[]}` |
//! | 角色删除 | `rbac.role.delete` | `role` | `{"permissions":[…]}` | NULL |
//! | 授予权限 | `rbac.role.grant` | `role` | `{"permissions":[…]}` | `{"permissions":[…+p]}` |
//! | 撤销权限 | `rbac.role.revoke` | `role` | `{"permissions":[…+p]}` | `{"permissions":[…]}` |
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-rbac --test audit_events -- --test-threads=1 --include-ignored
//! ```
//!
//! 缺 `DATABASE_URL_TEST` → **失败**（不是「跳过」—— 跳过也算通过 = 假绿）。

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use platform_core::RequestContext;
use platform_core::extension::audit::{AuditError, AuditEvent, AuditSink};
use platform_core::extension::permission::DataScope;
use platform_rbac::entity::Role;
use platform_rbac::scope::ScopePolicy;
use platform_rbac::{RbacService, RbacStore};

/// 注册表里真实存在的权限点。
const PERM_READ: &str = "sandbox:scope-item:read";
const PERM_EXPORT: &str = "sandbox:scope-item:export";
const PERM_APPROVE: &str = "sandbox:scope-item:approve";

/// 记录事件的**假 sink**（不落库 —— 本用例只断言**埋点契约**）。
#[derive(Debug, Default)]
struct RecordingSink(Mutex<Vec<AuditEvent>>);

impl RecordingSink {
    fn events(&self) -> Vec<AuditEvent> {
        self.0.lock().unwrap_or_else(|err| err.into_inner()).clone()
    }
}

impl AuditSink for RecordingSink {
    fn record<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        event: &'a AuditEvent,
    ) -> platform_core::extension::audit::BoxFuture<'a, Result<(), AuditError>> {
        self.0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(event.clone());
        Box::pin(async { Ok(()) })
    }
}

/// 造服务 + 假 sink（**先注入 sink 再操作**）。
fn build(fixture: &Fixture) -> (Arc<RbacService>, Arc<RecordingSink>) {
    let service = Arc::new(RbacService::new(
        RbacStore::new(fixture.app_db.clone()),
        ScopePolicy::default(),
    ));
    let sink = Arc::new(RecordingSink::default());
    assert!(
        service.with_audit_sink(sink.clone() as Arc<dyn AuditSink>),
        "sink 应当能注入一次"
    );
    (service, sink)
}

/// 取**最后一条**某事件码的记录。
fn last_of(sink: &RecordingSink, action: &str) -> AuditEvent {
    sink.events()
        .into_iter()
        .rev()
        .find(|event| event.action == action)
        .unwrap_or_else(|| panic!("应当有一条 `{action}` 事件"))
}

/// 读 `{"permissions": [...]}` 里的权限点数组。
fn permissions_of(value: &Option<serde_json::Value>) -> Vec<String> {
    value
        .as_ref()
        .and_then(|value| value.get("permissions"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .expect("权限点应当是字符串")
                })
                .collect()
        })
        .expect("载荷应当含 `permissions` 数组")
}

/// 唯一后缀。
///
/// ⚠️ **必须跨运行唯一**：角色表**不会**被测试清空（`common::setup()` 不复位角色）
/// → 只用进程内计数器的话，第二轮跑就会撞 `platform_role_pkey`。
/// （`rbac_real_db.rs` 用 `run_id()` 是同一个原因。）
fn unique_suffix() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// 角色创建 → `rbac.role.create`（`before = NULL` / `after = {"permissions":[]}`）。
#[tokio::test]
async fn creating_a_role_produces_one_create_event() {
    let fixture = setup().await;
    let (service, sink) = build(&fixture);
    let ctx = RequestContext::new()
        .with_tenant(TENANT_A)
        .with_user("u-2001");
    let suffix = unique_suffix();
    let role = Role {
        tenant_id: TENANT_A.to_string(),
        id: format!("r-audit-{suffix}"),
        code: format!("rbac-audit-{suffix}"),
        name: "audit".to_string(),
        data_scope: DataScope::OwnOrg,
        custom_filter: None,
        use_all_memberships: false,
        builtin: false,
    };
    service
        .create_role(&ctx, &role)
        .await
        .expect("角色应当能建成");

    let recorded = sink.events();
    let events: Vec<&AuditEvent> = recorded
        .iter()
        .filter(|event| event.action == "rbac.role.create")
        .collect();
    assert_eq!(events.len(), 1, "建角色 = **一条**审计事件");
    let event = events[0];
    assert_eq!(event.object_type, "role");
    assert_eq!(event.object_id.as_deref(), Some(role.id.as_str()));
    assert_eq!(event.tenant_id, TENANT_A);
    assert_eq!(event.actor_id.as_deref(), Some("u-2001"));
    assert_eq!(event.before, None, "建角色没有变更前");
    assert_eq!(
        event.after,
        Some(serde_json::json!({ "permissions": [] })),
        "建角色的变更后 = 空权限集合"
    );
}

/// 授权 → `rbac.role.grant`（`before` / `after` 是**排序后**的权限点数组）。
#[tokio::test]
async fn granting_a_permission_produces_a_grant_event_with_both_sides() {
    let fixture = setup().await;
    let (service, sink) = build(&fixture);
    let ctx = RequestContext::new()
        .with_tenant(TENANT_A)
        .with_user("u-2001");
    let suffix = unique_suffix();
    let role_id = format!("r-audit-{suffix}");
    let role = Role {
        tenant_id: TENANT_A.to_string(),
        id: role_id.clone(),
        code: format!("rbac-audit-{suffix}"),
        name: "audit".to_string(),
        data_scope: DataScope::OwnOrg,
        custom_filter: None,
        use_all_memberships: false,
        builtin: false,
    };
    service
        .create_role(&ctx, &role)
        .await
        .expect("角色应当能建成");

    service
        .grant_permission(&ctx, &role_id, PERM_READ, None)
        .await
        .expect("授权应当成功");

    let event = last_of(&sink, "rbac.role.grant");
    assert_eq!(event.object_type, "role");
    assert_eq!(event.object_id.as_deref(), Some(role_id.as_str()));
    assert_eq!(
        permissions_of(&event.before),
        Vec::<String>::new(),
        "授权前 = 空权限集合"
    );
    assert_eq!(
        permissions_of(&event.after),
        vec![PERM_READ.to_string()],
        "授权后 = 多了那个权限点"
    );
}

/// 撤权 → `rbac.role.revoke`（**方向与授权相反**）。
#[tokio::test]
async fn revoking_a_permission_produces_a_revoke_event_with_both_sides() {
    let fixture = setup().await;
    let (service, sink) = build(&fixture);
    let ctx = RequestContext::new()
        .with_tenant(TENANT_A)
        .with_user("u-2001");
    let suffix = unique_suffix();
    let role_id = format!("r-audit-{suffix}");
    let role = Role {
        tenant_id: TENANT_A.to_string(),
        id: role_id.clone(),
        code: format!("rbac-audit-{suffix}"),
        name: "audit".to_string(),
        data_scope: DataScope::OwnOrg,
        custom_filter: None,
        use_all_memberships: false,
        builtin: false,
    };
    service
        .create_role(&ctx, &role)
        .await
        .expect("角色应当能建成");
    service
        .grant_permission(&ctx, &role_id, PERM_READ, None)
        .await
        .expect("授权应当成功");

    service
        .revoke_permission(&ctx, &role_id, PERM_READ)
        .await
        .expect("撤权应当成功");

    let event = last_of(&sink, "rbac.role.revoke");
    assert_eq!(event.object_type, "role");
    assert_eq!(
        permissions_of(&event.before),
        vec![PERM_READ.to_string()],
        "撤权前 = 少了那个权限点"
    );
    assert_eq!(
        permissions_of(&event.after),
        Vec::<String>::new(),
        "撤权后 = 空权限集合"
    );
}

/// 删角色 → `rbac.role.delete`（`before` 有值 / `after = NULL`）。
#[tokio::test]
async fn deleting_a_role_produces_a_delete_event() {
    let fixture = setup().await;
    let (service, sink) = build(&fixture);
    let ctx = RequestContext::new()
        .with_tenant(TENANT_A)
        .with_user("u-2001");
    let suffix = unique_suffix();
    let role_id = format!("r-audit-{suffix}");
    let role = Role {
        tenant_id: TENANT_A.to_string(),
        id: role_id.clone(),
        code: format!("rbac-audit-{suffix}"),
        name: "audit".to_string(),
        data_scope: DataScope::OwnOrg,
        custom_filter: None,
        use_all_memberships: false,
        builtin: false,
    };
    service
        .create_role(&ctx, &role)
        .await
        .expect("角色应当能建成");
    service
        .grant_permission(&ctx, &role_id, PERM_READ, None)
        .await
        .expect("授权应当成功");

    service
        .delete_role(&ctx, &role_id)
        .await
        .expect("删角色应当成功");

    let event = last_of(&sink, "rbac.role.delete");
    assert_eq!(event.object_type, "role");
    assert_eq!(event.object_id.as_deref(), Some(role_id.as_str()));
    assert_eq!(
        permissions_of(&event.before),
        vec![PERM_READ.to_string()],
        "删角色前 = 该角色的直接授权"
    );
    assert_eq!(event.after, None, "删角色没有变更后");
}

/// 权限点数组必须**排序后**给出（否则同一个角色两次审计的载荷不可比对）。
#[tokio::test]
async fn the_permission_arrays_are_sorted() {
    let fixture = setup().await;
    let (service, sink) = build(&fixture);
    let ctx = RequestContext::new()
        .with_tenant(TENANT_A)
        .with_user("u-2001");
    let suffix = unique_suffix();
    let role_id = format!("r-audit-{suffix}");
    let role = Role {
        tenant_id: TENANT_A.to_string(),
        id: role_id.clone(),
        code: format!("rbac-audit-{suffix}"),
        name: "audit".to_string(),
        data_scope: DataScope::OwnOrg,
        custom_filter: None,
        use_all_memberships: false,
        builtin: false,
    };
    service
        .create_role(&ctx, &role)
        .await
        .expect("角色应当能建成");
    // 先授 `export` 再授 `read` —— 授权顺序与**字典序相反**。
    service
        .grant_permission(&ctx, &role_id, PERM_EXPORT, None)
        .await
        .expect("授权应当成功");
    service
        .grant_permission(&ctx, &role_id, PERM_READ, None)
        .await
        .expect("授权应当成功");
    // 第三次授权：`before` 已经有**两个**权限点（`before` 侧也要是排序后的）。
    service
        .grant_permission(&ctx, &role_id, PERM_APPROVE, None)
        .await
        .expect("授权应当成功");

    let event = last_of(&sink, "rbac.role.grant");
    for (label, side, expected) in [
        ("before", &event.before, 2usize),
        ("after", &event.after, 3usize),
    ] {
        let codes = permissions_of(side);
        let mut sorted = codes.clone();
        sorted.sort();
        assert_eq!(
            codes, sorted,
            "`{label}` 的权限点数组必须**排序后**给出：{codes:?}"
        );
        assert_eq!(
            codes.len(),
            expected,
            "`{label}` 应当有 {expected} 个权限点"
        );
    }
}
