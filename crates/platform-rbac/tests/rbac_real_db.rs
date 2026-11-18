//! RBAC 的**真实库层**（REQ-009）。
//!
//! 覆盖的验收标准：
//!
//! | AC | 用例 |
//! |----|------|
//! | AC-S-033 | 权限点注册表双向对账（一致 → 通过；缺一条 → 逐条报出） |
//! | AC-S-034 | 角色 / 授权 / 绑定的租户维度与 RLS 兜底 |
//! | AC-S-035 | 角色继承展开 + 成环被拒（**指出是哪两个角色**） |
//! | AC-S-036 | 通配语义 + `*:*` 与跨模块通配被拒 |
//! | AC-S-037 | 内置角色不可删降权；`platform-admin` = 注册表全集 |
//! | AC-S-038 | 五种 scope × **五个探针用户**（逐条列出记录 id） |
//! | AC-S-039 | 兼岗默认主部门；取并集必须显式开启 |
//! | AC-S-040 | 主部门缺失 / 歧义 → **fail closed 到 `SELF`** |
//! | AC-S-041 | 多角色取最宽松；权限点级覆盖角色级 |
//! | AC-S-047 | 撤销在**下一个请求**即生效（两个独立实例） |
//! | AC-S-048 | 数据范围过滤在 **SQL 层**生效 |
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-rbac --test rbac_real_db -- --test-threads=2 --include-ignored
//! ```
//!
//! 缺 `DATABASE_URL_TEST` → **失败**（不是「跳过」—— 跳过也算通过 = 假绿）。

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use common::*;
use platform_core::RequestContext;
use platform_core::extension::permission::{DataScope, PermissionChecker, ScopeFilter};
use platform_db::TenantQuery;
use platform_rbac::entity::BUILTIN_ADMIN;
use platform_rbac::scope::ScopePolicy;
use platform_rbac::{RbacChecker, RbacService, RbacStore};

/// 五个探针用户（**各自的主部门 / 本人记录不同** —— 同一个用户没法独立验五种 scope）。
///
/// ⚠️ 这些是**标签**，不是最终的用户 ID：每个用例跑在自己的用户上
/// （`{标签}-{用例后缀}`）—— 否则上一轮留下的角色绑定会污染下一轮的结果。
const TAG_ALL: &str = "u-all";
const TAG_OWN: &str = "u-own";
const TAG_BELOW: &str = "u-below";
const TAG_SELF: &str = "u-self";
const TAG_CUSTOM: &str = "u-custom";
/// 兼岗用户（主部门 dept-a + 非主部门 dept-b）。
const TAG_DUAL: &str = "u-dual";
/// 没有主部门的用户。
const TAG_NO_PRIMARY: &str = "u-no-primary";
/// 有两个主部门的用户（歧义）。
const TAG_DUAL_PRIMARY: &str = "u-dual-primary";
/// 多角色用户。
const TAG_MULTI: &str = "u-multi";

/// 一个用例用到的全部用户。
struct Users {
    all: String,
    own: String,
    below: String,
    own_self: String,
    custom: String,
    dual: String,
    no_primary: String,
    dual_primary: String,
    multi: String,
}

/// 按「租户 + 用例后缀」生成**互不重叠**的用户 ID。
///
/// ⚠️ `platform_iam_user` 的主键是**单列 `id`** —— 用户 ID 必须带上租户，
/// 否则上一轮（另一个租户）留下的同名用户会让插入被 `ON CONFLICT` 跳过，
/// 外键随即失败。
fn users(tenant: &str, tag: &str) -> Users {
    let id = |base: &str| format!("{base}-{tenant}-{tag}");
    Users {
        all: id(TAG_ALL),
        own: id(TAG_OWN),
        below: id(TAG_BELOW),
        own_self: id(TAG_SELF),
        custom: id(TAG_CUSTOM),
        dual: id(TAG_DUAL),
        no_primary: id(TAG_NO_PRIMARY),
        dual_primary: id(TAG_DUAL_PRIMARY),
        multi: id(TAG_MULTI),
    }
}

/// 权限点（**注册表里真实存在**的）。
const PERM_READ: &str = "sandbox:scope-item:read";
const PERM_APPROVE: &str = "sandbox:scope-item:approve";
const PERM_EXPORT: &str = "sandbox:scope-item:export";

/// 某个租户的上下文。
fn ctx_for(tenant: &str) -> RequestContext {
    RequestContext::new().with_tenant(tenant)
}

/// 造服务 + 判定器。
fn build(fixture: &Fixture) -> (Arc<RbacService>, Arc<RbacChecker>) {
    let service = Arc::new(RbacService::new(
        RbacStore::new(fixture.app_db.clone()),
        ScopePolicy::default(),
    ));
    let checker = Arc::new(RbacChecker::new(service.clone()));
    (service, checker)
}

/// 唯一的夹具 ID 后缀（**用例并行**时角色 ID 不能撞）。
fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("{:04}", COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// 本轮运行的标识（**跨运行**也不能撞 —— 库不会每次都清空）。
///
/// ⚠️ 没有它的话：第二轮跑用例时计数器从 0 重来 → 租户名与角色 ID 与上一轮
/// 完全相同 → 主键冲突（而且报的是「重复键」，看不出是夹具撞了）。
fn run_id() -> &'static str {
    use std::sync::OnceLock;
    static RUN_ID: OnceLock<String> = OnceLock::new();
    RUN_ID.get_or_init(|| uuid::Uuid::new_v4().to_string()[..8].to_string())
}

/// 造一个角色（返回角色 ID；**每个用例的角色 ID 唯一** —— 用例并行时不互相踩）。
async fn seed_role(
    service: &RbacService,
    ctx: &RequestContext,
    id: &str,
    code: &str,
    scope: DataScope,
    use_all_memberships: bool,
) -> String {
    let suffix = unique_suffix();
    let id = format!("{id}-{suffix}");
    let role = platform_rbac::entity::Role {
        tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
        id: id.clone(),
        code: format!("{code}-{suffix}"),
        name: code.to_string(),
        data_scope: scope,
        custom_filter: None,
        use_all_memberships,
        builtin: false,
    };
    service
        .create_role(ctx, &role)
        .await
        .expect("角色应当能建成");
    id
}

/// 造一个带自定义条件的角色。
async fn seed_custom_role(
    service: &RbacService,
    ctx: &RequestContext,
    id: &str,
    code: &str,
    filter: &str,
) -> String {
    let suffix = unique_suffix();
    let id = format!("{id}-{suffix}");
    let role = platform_rbac::entity::Role {
        tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
        id: id.clone(),
        code: format!("{code}-{suffix}"),
        name: code.to_string(),
        data_scope: DataScope::Custom,
        custom_filter: Some(filter.to_string()),
        use_all_memberships: false,
        builtin: false,
    };
    service
        .create_role(ctx, &role)
        .await
        .expect("自定义范围的角色应当能建成");
    id
}

/// 授权 + 绑定。
async fn seed_grant_and_bind(
    service: &RbacService,
    ctx: &RequestContext,
    role_id: &str,
    permission: &str,
    permission_scope: Option<DataScope>,
    user_id: &str,
) {
    service
        .grant_permission(ctx, role_id, permission, permission_scope)
        .await
        .expect("授权应当成功");
    service
        .bind_user_role(ctx, user_id, role_id)
        .await
        .expect("绑定应当成功");
}

/// 灌入组织树 + 用户 + 记录（**所有用例共用的基础夹具**）。
async fn seed_base_fixture(fixture: &Fixture, users: &Users, tenant: &str) {
    // 组织树（租户 A）：
    //   dept-hq
    //   ├── dept-a ── dept-a-1 ── dept-a-2
    //   ├── dept-b ── dept-b-1
    //   └── dept-c
    for (id, parent) in [
        ("dept-hq", None),
        ("dept-a", Some("dept-hq")),
        ("dept-a-1", Some("dept-a")),
        ("dept-a-2", Some("dept-a-1")),
        ("dept-b", Some("dept-hq")),
        ("dept-b-1", Some("dept-b")),
        ("dept-c", Some("dept-hq")),
    ] {
        seed_org_node(fixture, tenant, id, parent).await;
    }
    // 跨租对照租户的组织节点。
    seed_org_node(fixture, tenant, "dept-t2-hq", None).await;

    // 账号（`platform_user_role` 的外键目标）。
    for user in [
        &users.all,
        &users.own,
        &users.below,
        &users.own_self,
        &users.custom,
        &users.dual,
        &users.no_primary,
        &users.dual_primary,
        &users.multi,
    ] {
        seed_iam_user(fixture, tenant, user).await;
    }

    // 成员关系。
    for (user, node, primary) in [
        (&users.all, "dept-a", true),
        (&users.own, "dept-a", true),
        (&users.below, "dept-a", true),
        (&users.own_self, "dept-c", true),
        (&users.custom, "dept-a", true),
        (&users.dual, "dept-a", true),
        (&users.dual, "dept-b", false),
        (&users.no_primary, "dept-a", false),
        (&users.multi, "dept-a", true),
    ] {
        seed_membership(fixture, tenant, user, node, primary).await;
    }

    // 记录（13 条里的 10 条 —— 另 3 条由各用例自己加）。
    let records: Vec<(&str, &str, &str, i64)> = vec![
        ("item-a1", "dept-a", "u-other", 5000),
        ("item-a2", "dept-a-1", "u-other", 5000),
        ("item-a3", "dept-a-2", "u-other", 5000),
        ("item-b1", "dept-b", "u-other", 5000),
        ("item-b2", "dept-b-1", "u-other", 5000),
        ("item-c1", "dept-c", "u-other", 5000),
        ("item-self1", "dept-c", users.own_self.as_str(), 5000),
        ("item-self2", "dept-b", users.own_self.as_str(), 5000),
        ("item-big", "dept-a", users.own_self.as_str(), 20000),
        ("item-multi", "dept-c", users.multi.as_str(), 5000),
    ];
    for (id, node, creator, amount) in records {
        seed_scope_item(fixture, tenant, id, node, creator, amount).await;
    }
}

/// 某用户在某权限点下**看得见**的记录 id（**经由 SQL 层过滤**）。
async fn visible_ids(
    fixture: &Fixture,
    checker: &RbacChecker,
    tenant_id: &str,
    user_id: &str,
    permission: &str,
) -> Vec<String> {
    let ctx = RequestContext::new()
        .with_tenant(tenant_id)
        .with_user(user_id);
    checker
        .ensure_fresh(&ctx)
        .await
        .expect("判定缓存应当能刷新");
    let filter = checker.effective_scope(&ctx, permission);
    let query = TenantQuery::new(
        "SELECT id FROM sandbox_scope_item WHERE tenant_id = {tenant} ORDER BY id",
    )
    .with_scope(&filter)
    .expect("范围条件应当能渲染");
    let rows: Vec<(String,)> = fixture
        .app_db
        .fetch_all(&ctx, &query)
        .await
        .expect("记录应当能读出");
    rows.into_iter().map(|(id,)| id).collect()
}

/// 把 id 列表转成集合（便于比较）。
fn set(ids: &[String]) -> BTreeSet<String> {
    ids.iter().cloned().collect()
}

// ---------------------------------------------------------------------------
// AC-S-033：权限点注册表双向对账
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_registry_matches_the_manifests_and_a_difference_is_reported() {
    let fixture = setup().await;
    let (service, _checker) = build(&fixture);

    let manifest_permissions =
        platform_rbac::collect_manifest_permissions(&repo_root()).expect("manifest 应当能扫出来");
    assert!(
        !manifest_permissions.is_empty(),
        "manifest 侧必须有权限点（否则对账是空转）"
    );

    // 先把库恢复到「与 manifest 一致」的状态（**上一轮用例可能改过它**）。
    sqlx::query("DELETE FROM platform_permission WHERE code = 'ghost:perm:read'")
        .execute(&fixture.admin_pool)
        .await
        .expect("清理应当成功");
    for entry in &manifest_permissions {
        sqlx::query(
            "INSERT INTO platform_permission (code, name, kind, condition, source) \
             VALUES ($1, $2, 'button', $3, $4) \
             ON CONFLICT (code) DO UPDATE SET name = EXCLUDED.name, \
                 condition = EXCLUDED.condition, source = EXCLUDED.source",
        )
        .bind(&entry.code)
        .bind(&entry.name)
        .bind(entry.condition.as_deref())
        .bind(&entry.source)
        .execute(&fixture.admin_pool)
        .await
        .expect("注册表应当能写入");
    }

    // 一致 → 通过。
    let report = platform_rbac::reconcile(service.store(), &manifest_permissions)
        .await
        .expect("对账应当能跑");
    assert!(
        report.is_consistent(),
        "manifest 与库应当一致，差异：{:?}",
        report.differences()
    );

    // 库里多一个权限点 → **逐条列出**。
    // （平台级表没有 `tenant_id` → 用管理员裸连接写。）
    sqlx::query(
        "INSERT INTO platform_permission (code, name, kind, condition, source) \
         VALUES ('ghost:perm:read', '幽灵', 'button', NULL, 'nowhere') \
         ON CONFLICT (code) DO NOTHING",
    )
    .execute(&fixture.admin_pool)
    .await
    .expect("幽灵权限点应当能插入");

    // **启动路径**必须因此失败（不是告警）：逐条列出差异。
    let error = platform_rbac::registry::require_consistent(service.store(), &manifest_permissions)
        .await
        .expect_err("对账不一致必须让启动失败（不是告警）");
    assert_eq!(error.code(), "REGISTRY_MISMATCH");
    let detail = error.detail();
    assert!(
        detail.contains("ghost:perm:read"),
        "差异必须逐条列出：{detail}"
    );

    let report = platform_rbac::reconcile(service.store(), &manifest_permissions)
        .await
        .expect("对账应当能跑");
    assert!(!report.is_consistent(), "库里多出来的权限点必须被报出来");
    assert!(
        report
            .extra_in_database
            .contains(&"ghost:perm:read".to_string()),
        "差异必须逐条列出：{:?}",
        report.differences()
    );

    // 库里少一个权限点 → **逐条列出，且带来源 manifest 路径**。
    sqlx::query("DELETE FROM platform_permission WHERE code = 'sandbox:demo:read'")
        .execute(&fixture.admin_pool)
        .await
        .expect("删除应当成功");

    let report = platform_rbac::reconcile(service.store(), &manifest_permissions)
        .await
        .expect("对账应当能跑");
    assert!(!report.is_consistent());
    let missing = report
        .missing_in_database
        .iter()
        .find(|entry| entry.code == "sandbox:demo:read")
        .expect("缺失的权限点必须被报出来");
    assert_eq!(missing.source, "modules/sandbox/manifest.json");
    let differences = report.differences();
    assert!(
        differences
            .iter()
            .any(|line| line.contains("sandbox:demo:read")
                && line.contains("modules/sandbox/manifest.json")),
        "差异必须逐条列出编码与来源 manifest 路径：{differences:?}"
    );

    // 还原（别把库留在坏状态 —— 其它用例还要用这个注册表）。
    sqlx::query(
        "INSERT INTO platform_permission (code, name, kind, condition, source) \
         VALUES ('sandbox:demo:read', '查看沙箱示例', 'menu', NULL, 'modules/sandbox/manifest.json') \
         ON CONFLICT (code) DO UPDATE SET name = EXCLUDED.name, source = EXCLUDED.source",
    )
    .execute(&fixture.admin_pool)
    .await
    .expect("还原应当成功");
    sqlx::query("DELETE FROM platform_permission WHERE code = 'ghost:perm:read'")
        .execute(&fixture.admin_pool)
        .await
        .expect("清理幽灵权限点应当成功");
}

// ---------------------------------------------------------------------------
// AC-S-037：内置角色不可删降权 + `platform-admin` = 注册表全集
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_built_in_roles_cannot_be_deleted_or_downgraded() {
    let fixture = setup().await;
    let (service, checker) = build(&fixture);
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    service
        .ensure_builtin_roles(&ctx)
        .await
        .expect("内置角色应当能建成");

    let admin = service
        .get_role(&ctx, "builtin-platform-admin")
        .await
        .expect("内置管理员应当存在");
    assert!(admin.builtin);
    assert_eq!(admin.data_scope, "ALL");

    // 1) 删不掉。
    let error = service
        .delete_role(&ctx, "builtin-platform-admin")
        .await
        .expect_err("内置角色不得被删除");
    assert_eq!(error.code(), "BUILTIN_ROLE_PROTECTED");
    assert!(error.detail().contains(BUILTIN_ADMIN));

    // 2) 降不了权（改 data_scope）。
    let error = service
        .update_role_scope(
            &ctx,
            "builtin-platform-admin",
            DataScope::OnlySelf,
            None,
            false,
        )
        .await
        .expect_err("内置角色不得被降权");
    assert_eq!(error.code(), "BUILTIN_ROLE_PROTECTED");

    // 3) 减不了权（撤掉一个权限点）。
    let error = service
        .grant_permission(&ctx, "builtin-platform-admin", PERM_READ, None)
        .await
        .expect_err("内置角色的权限不得被手工改");
    assert_eq!(error.code(), "BUILTIN_ROLE_PROTECTED");

    // 4) `platform-admin` 的权限集合 = **注册表全集**（逐个比对）。
    let registry = service.registry_codes().await.expect("注册表应当能读");
    assert!(registry.len() >= 15, "注册表应当有权限点：{registry:?}");
    let admin_user = format!("u-admin-{}-{}", tenant, unique_suffix());
    let user_ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(&admin_user);
    seed_iam_user(&fixture, &tenant, &admin_user).await;
    service
        .bind_user_role(&ctx, &admin_user, "builtin-platform-admin")
        .await
        .expect("绑定应当成功");

    checker
        .ensure_fresh(&user_ctx)
        .await
        .expect("判定缓存应当能刷新");
    for code in &registry {
        assert!(
            checker.has(&user_ctx, code),
            "内置管理员必须拥有注册表里的每一个权限点，缺 `{code}`"
        );
    }

    // 5) 内置普通用户**没有**任何权限点。
    let plain_user = format!("u-plain-{}-{}", tenant, unique_suffix());
    let user_ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(&plain_user);
    seed_iam_user(&fixture, &tenant, &plain_user).await;
    service
        .bind_user_role(&ctx, &plain_user, "builtin-platform-user")
        .await
        .expect("绑定应当成功");
    checker
        .ensure_fresh(&user_ctx)
        .await
        .expect("判定缓存应当能刷新");
    for code in &registry {
        assert!(
            !checker.has(&user_ctx, code),
            "内置普通用户不得拥有任何权限点，却有 `{code}`"
        );
    }
}

// ---------------------------------------------------------------------------
// AC-S-038：五种 scope × 五个探针用户
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_five_data_scopes_resolve_to_the_expected_record_sets() {
    let fixture = setup().await;
    let (service, checker) = build(&fixture);
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    // 五个探针，各自带一个 scope 的角色。
    let r_all = seed_role(&service, &ctx, "r-all", "r-all", DataScope::All, false).await;
    let r_own = seed_role(&service, &ctx, "r-own", "r-own", DataScope::OwnOrg, false).await;
    let r_below = seed_role(
        &service,
        &ctx,
        "r-below",
        "r-below",
        DataScope::OwnOrgAndBelow,
        false,
    )
    .await;
    let r_self = seed_role(
        &service,
        &ctx,
        "r-self",
        "r-self",
        DataScope::OnlySelf,
        false,
    )
    .await;
    let r_custom = seed_custom_role(
        &service,
        &ctx,
        "r-custom",
        "r-custom",
        "$record.amount <= 10000",
    )
    .await;

    seed_grant_and_bind(&service, &ctx, &r_all, PERM_READ, None, &users.all).await;
    seed_grant_and_bind(&service, &ctx, &r_own, PERM_READ, None, &users.own).await;
    seed_grant_and_bind(&service, &ctx, &r_below, PERM_READ, None, &users.below).await;
    seed_grant_and_bind(&service, &ctx, &r_self, PERM_READ, None, &users.own_self).await;
    seed_grant_and_bind(&service, &ctx, &r_custom, PERM_READ, None, &users.custom).await;

    // 每种 scope 的**期望集合**（逐条列出记录 id）。
    let expected_all = set(&[
        "item-a1".to_string(),
        "item-a2".to_string(),
        "item-a3".to_string(),
        "item-b1".to_string(),
        "item-b2".to_string(),
        "item-c1".to_string(),
        "item-big".to_string(),
        "item-multi".to_string(),
        "item-self1".to_string(),
        "item-self2".to_string(),
    ]);
    let expected_own = set(&["item-a1".to_string(), "item-big".to_string()]);
    let expected_below = set(&[
        "item-a1".to_string(),
        "item-a2".to_string(),
        "item-a3".to_string(),
        "item-big".to_string(),
    ]);
    let expected_self = set(&[
        "item-big".to_string(),
        "item-self1".to_string(),
        "item-self2".to_string(),
    ]);
    let expected_custom = set(&[
        "item-a1".to_string(),
        "item-a2".to_string(),
        "item-a3".to_string(),
        "item-b1".to_string(),
        "item-b2".to_string(),
        "item-c1".to_string(),
        "item-multi".to_string(),
        "item-self1".to_string(),
        "item-self2".to_string(),
    ]);

    let actual_all = set(&visible_ids(&fixture, &checker, &tenant, &users.all, PERM_READ).await);
    let actual_own = set(&visible_ids(&fixture, &checker, &tenant, &users.own, PERM_READ).await);
    let actual_below =
        set(&visible_ids(&fixture, &checker, &tenant, &users.below, PERM_READ).await);
    let actual_self =
        set(&visible_ids(&fixture, &checker, &tenant, &users.own_self, PERM_READ).await);
    let actual_custom =
        set(&visible_ids(&fixture, &checker, &tenant, &users.custom, PERM_READ).await);

    assert_eq!(actual_all, expected_all, "ALL 应当看到全部记录");
    assert_eq!(actual_own, expected_own, "OWN_ORG 应当只看到主部门的记录");
    assert_eq!(
        actual_below, expected_below,
        "OWN_ORG_AND_BELOW 应当**跨层**看到主部门子树的记录"
    );
    assert_eq!(actual_self, expected_self, "SELF 应当只看到本人创建的记录");
    assert_eq!(
        actual_custom, expected_custom,
        "CUSTOM 应当只看到条件命中的记录"
    );

    // 五个集合**两两不相等**（否则夹具无效）。
    let sets = [
        ("ALL", actual_all),
        ("OWN_ORG", actual_own),
        ("OWN_ORG_AND_BELOW", actual_below),
        ("SELF", actual_self),
        ("CUSTOM", actual_custom),
    ];
    for (left_index, (left_name, left)) in sets.iter().enumerate() {
        for (right_name, right) in sets.iter().skip(left_index + 1) {
            assert_ne!(
                left, right,
                "两种 scope 的结果集必须不相等：`{left_name}` 与 `{right_name}`"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// AC-S-039：兼岗默认按主部门，取并集必须显式开启
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_part_time_membership_defaults_to_the_primary_department() {
    let fixture = setup().await;
    let (service, checker) = build(&fixture);
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    // 默认（只按主部门）。
    let r_own = seed_role(
        &service,
        &ctx,
        "r-dual-own",
        "r-dual-own",
        DataScope::OwnOrg,
        false,
    )
    .await;
    let r_below = seed_role(
        &service,
        &ctx,
        "r-dual-below",
        "r-dual-below",
        DataScope::OwnOrgAndBelow,
        false,
    )
    .await;
    // 显式开启并集。
    let r_union = seed_role(
        &service,
        &ctx,
        "r-dual-union",
        "r-dual-union",
        DataScope::OwnOrgAndBelow,
        true,
    )
    .await;

    // 三个角色都绑给兼岗用户，但**每次只用一个**（否则并集会互相污染）。
    seed_grant_and_bind(&service, &ctx, &r_own, PERM_READ, None, &users.dual).await;

    // 1) 默认 OWN_ORG → 只含主部门 dept-a 的记录（**不含 dept-b**）。
    let own = set(&visible_ids(&fixture, &checker, &tenant, &users.dual, PERM_READ).await);
    assert_eq!(
        own,
        set(&["item-a1".to_string(), "item-big".to_string()]),
        "默认必须只按主部门（dept-a）"
    );
    assert!(
        !own.contains("item-b1"),
        "默认不得看到兼岗部门 dept-b 的记录"
    );

    // 2) 默认 OWN_ORG_AND_BELOW → 主部门的子树（**不含 dept-b / dept-b-1**）。
    service
        .unbind_user_role(&ctx, &users.dual, &r_own)
        .await
        .expect("解绑应当成功");
    seed_grant_and_bind(&service, &ctx, &r_below, PERM_READ, None, &users.dual).await;
    let below = set(&visible_ids(&fixture, &checker, &tenant, &users.dual, PERM_READ).await);
    assert_eq!(
        below,
        set(&[
            "item-a1".to_string(),
            "item-a2".to_string(),
            "item-a3".to_string(),
            "item-big".to_string()
        ]),
        "默认的「本部门及下级」只算主部门的子树"
    );
    assert!(!below.contains("item-b1"), "默认不得看到 dept-b 的记录");
    assert!(!below.contains("item-b2"), "默认不得看到 dept-b-1 的记录");

    // 3) 显式开启并集 → 主部门 ∪ 兼岗部门（各带子树）。
    service
        .unbind_user_role(&ctx, &users.dual, &r_below)
        .await
        .expect("解绑应当成功");
    seed_grant_and_bind(&service, &ctx, &r_union, PERM_READ, None, &users.dual).await;
    let union = set(&visible_ids(&fixture, &checker, &tenant, &users.dual, PERM_READ).await);
    assert_eq!(
        union,
        set(&[
            "item-a1".to_string(),
            "item-a2".to_string(),
            "item-a3".to_string(),
            "item-b1".to_string(),
            "item-b2".to_string(),
            "item-big".to_string(),
            // 兼岗部门 dept-b 下的一条记录（创建人不是兼岗用户，但**归属** dept-b）。
            "item-self2".to_string()
        ]),
        "显式开启后应当取所有成员节点的并集"
    );
    assert_ne!(union, below, "显式开启的结果必须与默认不同");
}

// ---------------------------------------------------------------------------
// AC-S-040：主部门缺失 / 歧义 → fail closed 到 SELF
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_missing_or_ambiguous_primary_department_falls_back_to_self() {
    let fixture = setup().await;
    let (service, checker) = build(&fixture);
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    // 用户 V（无主部门）自己创建了一条记录。
    seed_iam_user(&fixture, &tenant, &users.no_primary).await;
    seed_scope_item(
        &fixture,
        &tenant,
        "item-noprimary",
        "dept-c",
        &users.no_primary,
        5000,
    )
    .await;

    let r_own = seed_role(
        &service,
        &ctx,
        "r-noprimary",
        "r-noprimary",
        DataScope::OwnOrg,
        false,
    )
    .await;
    seed_grant_and_bind(&service, &ctx, &r_own, PERM_READ, None, &users.no_primary).await;

    // 0) 先把「预防层」摆正：清掉上一轮遗留的歧义夹具，确保唯一索引在。
    sqlx::query("DELETE FROM user_org WHERE user_id LIKE 'u-dual-primary-%'")
        .execute(&fixture.admin_pool)
        .await
        .ok();
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS user_org_one_primary_idx \
         ON user_org (tenant_id, user_id) WHERE is_primary",
    )
    .execute(&fixture.admin_pool)
    .await
    .expect("部分唯一索引应当能建立（预防层）");

    // 1) 数据库**拒绝**第二个主部门（部分唯一索引）。
    //
    //    ⚠️ 用**已经有主部门**的用户（`users.own`，主部门 dept-a）来试：
    //    索引防的是「一个用户有两条主部门」，不是「没有主部门的用户补一条」。
    let insert = TenantQuery::new(
        "INSERT INTO user_org (tenant_id, id, user_id, org_node_id, role, is_primary) \
         VALUES ({tenant}, 'uo-second-primary', ?, 'dept-c', NULL, TRUE)",
    )
    .bind(users.own.as_str());
    let mut conn = fixture.admin_db.begin(&ctx).await.expect("事务应当能开");
    let rejected = conn.execute(&insert).await;
    assert!(
        rejected.is_err(),
        "第二个主部门必须被数据库拒绝（部分唯一索引）"
    );
    conn.rollback().await.ok();

    // 2) 主部门缺失 → 退化为 SELF（**只看到本人创建的记录**）。
    let visible =
        set(&visible_ids(&fixture, &checker, &tenant, &users.no_primary, PERM_READ).await);
    assert_eq!(
        visible,
        set(&["item-noprimary".to_string()]),
        "主部门缺失必须退化为 SELF（不得看到本部门或全租户数据）"
    );
    assert!(!visible.contains("item-a1"), "不得退化成更宽的范围");

    // 3) 歧义（两个主部门）→ **同样**退化为 SELF。
    //    歧义状态**绕过数据库约束**造出来（部分唯一索引先摘掉）。
    sqlx::query("DROP INDEX IF EXISTS user_org_one_primary_idx")
        .execute(&fixture.admin_pool)
        .await
        .expect("摘掉索引应当成功");
    seed_iam_user(&fixture, &tenant, &users.dual_primary).await;
    seed_membership(&fixture, &tenant, &users.dual_primary, "dept-a", true).await;
    seed_membership(&fixture, &tenant, &users.dual_primary, "dept-c", true).await;
    seed_scope_item(
        &fixture,
        &tenant,
        "item-dualprimary",
        "dept-c",
        &users.dual_primary,
        5000,
    )
    .await;

    let r_dual = seed_role(
        &service,
        &ctx,
        "r-dualprimary",
        "r-dualprimary",
        DataScope::OwnOrg,
        false,
    )
    .await;
    seed_grant_and_bind(
        &service,
        &ctx,
        &r_dual,
        PERM_READ,
        None,
        &users.dual_primary,
    )
    .await;

    let visible =
        set(&visible_ids(&fixture, &checker, &tenant, &users.dual_primary, PERM_READ).await);
    assert_eq!(
        visible,
        set(&["item-dualprimary".to_string()]),
        "主部门歧义必须退化为 SELF（判定层是防御层，不能只靠数据库约束）"
    );
    assert!(!visible.contains("item-a1"), "歧义时不得看到任何部门数据");

    // 把索引装回去（别把库留在坏状态）：先清掉歧义行，再重建索引。
    sqlx::query("DELETE FROM user_org WHERE user_id = $1")
        .bind(users.dual_primary)
        .execute(&fixture.admin_pool)
        .await
        .expect("清理歧义夹具应当成功");
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS user_org_one_primary_idx \
         ON user_org (tenant_id, user_id) WHERE is_primary",
    )
    .execute(&fixture.admin_pool)
    .await
    .expect("重建索引应当成功");
}

// ---------------------------------------------------------------------------
// AC-S-041：多角色取最宽松 + 权限点级覆盖
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn multiple_roles_take_the_most_permissive_scope() {
    let fixture = setup().await;
    let (service, checker) = build(&fixture);
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    let r1 = seed_role(
        &service,
        &ctx,
        "r-multi-a",
        "r-multi-a",
        DataScope::OwnOrg,
        false,
    )
    .await;
    let r2 = seed_role(
        &service,
        &ctx,
        "r-multi-b",
        "r-multi-b",
        DataScope::OwnOrg,
        false,
    )
    .await;
    let r3 = seed_role(
        &service,
        &ctx,
        "r-multi-c",
        "r-multi-c",
        DataScope::OwnOrgAndBelow,
        false,
    )
    .await;

    seed_grant_and_bind(&service, &ctx, &r1, PERM_READ, None, &users.multi).await;
    seed_grant_and_bind(
        &service,
        &ctx,
        &r2,
        PERM_EXPORT,
        Some(DataScope::OnlySelf),
        &users.multi,
    )
    .await;
    seed_grant_and_bind(&service, &ctx, &r3, PERM_READ, None, &users.multi).await;

    // 1) 普通读权限点 → 取**最宽松**的（R3 的 OWN_ORG_AND_BELOW）。
    let read = set(&visible_ids(&fixture, &checker, &tenant, &users.multi, PERM_READ).await);
    assert_eq!(
        read,
        set(&[
            "item-a1".to_string(),
            "item-a2".to_string(),
            "item-a3".to_string(),
            "item-big".to_string()
        ]),
        "多角色的 scope 取最宽松的"
    );

    // 2) export 权限点 → **权限点级覆盖角色级** → SELF。
    let export = set(&visible_ids(&fixture, &checker, &tenant, &users.multi, PERM_EXPORT).await);
    assert_eq!(
        export,
        set(&["item-multi".to_string()]),
        "权限点级的 scope 覆盖角色级"
    );

    // 3) 解绑 R3 → 退化为 R1 的 OWN_ORG（**与第 1 步结果不同** —— 证明是并集不是交集）。
    service
        .unbind_user_role(&ctx, &users.multi, &r3)
        .await
        .expect("解绑应当成功");
    let read_after = set(&visible_ids(&fixture, &checker, &tenant, &users.multi, PERM_READ).await);
    assert_eq!(
        read_after,
        set(&["item-a1".to_string(), "item-big".to_string()]),
        "解绑最宽松的角色后应当退化为 OWN_ORG"
    );
    assert_ne!(
        read_after, read,
        "解绑前后的结果必须不同（否则说明取的是交集而不是并集）"
    );
}

// ---------------------------------------------------------------------------
// AC-S-035 / AC-S-036：继承展开 + 成环被拒 + 通配语义
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn inheritance_expands_and_cycles_are_rejected() {
    let fixture = setup().await;
    let (service, checker) = build(&fixture);
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    let a = seed_role(&service, &ctx, "r-inh-a", "r-inh-a", DataScope::All, false).await;
    let b = seed_role(&service, &ctx, "r-inh-b", "r-inh-b", DataScope::All, false).await;
    let c = seed_role(&service, &ctx, "r-inh-c", "r-inh-c", DataScope::All, false).await;
    seed_grant_and_bind(&service, &ctx, &a, PERM_READ, None, &users.own).await;
    service
        .grant_permission(&ctx, &b, PERM_APPROVE, None)
        .await
        .expect("授权应当成功");
    service
        .grant_permission(&ctx, &c, PERM_EXPORT, None)
        .await
        .expect("授权应当成功");

    service
        .set_inherit(&ctx, &a, &b)
        .await
        .expect("继承应当成功");
    service
        .set_inherit(&ctx, &b, &c)
        .await
        .expect("继承应当成功");

    let user_ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(users.own);
    checker
        .ensure_fresh(&user_ctx)
        .await
        .expect("判定缓存应当能刷新");
    for code in [PERM_READ, PERM_APPROVE, PERM_EXPORT] {
        assert!(
            checker.has(&user_ctx, code),
            "继承链上的权限点必须生效，缺 `{code}`"
        );
    }

    // 成环 → 被拒，并**指出是哪两个角色构成的环**。
    let error = service
        .set_inherit(&ctx, &c, &a)
        .await
        .expect_err("成环必须被拒");
    assert_eq!(error.code(), "INHERITANCE_CYCLE");
    let detail = error.detail();
    assert!(
        detail.contains("r-inh-a") && detail.contains("r-inh-c"),
        "错误信息必须指出是哪两个角色构成的环：{detail}"
    );
}

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 问到"]
async fn wildcards_work_and_the_full_wildcard_is_rejected() {
    let fixture = setup().await;
    let (service, checker) = build(&fixture);
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    let r = seed_role(&service, &ctx, "r-wild", "r-wild", DataScope::All, false).await;
    // `sandbox:*` 覆盖整个模块。
    service
        .grant_permission(&ctx, &r, "sandbox:*", None)
        .await
        .expect("模块通配应当被接受");
    seed_grant_and_bind(&service, &ctx, &r, PERM_READ, None, &users.own).await;

    let user_ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(users.own);
    checker
        .ensure_fresh(&user_ctx)
        .await
        .expect("判定缓存应当能刷新");
    assert!(checker.has(&user_ctx, PERM_READ));
    assert!(checker.has(&user_ctx, PERM_APPROVE));
    assert!(
        !checker.has(&user_ctx, "org:org-node:read"),
        "`sandbox:*` 不得覆盖 `org:` 下的权限点"
    );

    // `*:*` 与跨模块通配 → **写入被拒**。
    for forbidden in ["*:*", "*:demo:read"] {
        let error = service
            .grant_permission(&ctx, &r, forbidden, None)
            .await
            .expect_err("全平台通配必须被拒");
        assert_eq!(error.code(), "WILDCARD_FORBIDDEN", "{forbidden} 必须被拒");
    }
}

// ---------------------------------------------------------------------------
// AC-S-047：撤销在下一个请求即生效（两个独立实例）
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_revocation_takes_effect_on_the_next_request() {
    let fixture = setup().await;
    let (service, checker) = build(&fixture);
    // **第二个独立实例**（各自持自己的缓存，共享同一个数据库）—— 模拟两个进程。
    let other_checker = Arc::new(RbacChecker::new(service.clone()));
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    let r = seed_role(
        &service,
        &ctx,
        "r-revoke",
        "r-revoke",
        DataScope::All,
        false,
    )
    .await;
    seed_grant_and_bind(&service, &ctx, &r, PERM_READ, None, &users.own).await;

    let user_ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(users.own);
    checker.ensure_fresh(&user_ctx).await.expect("缓存刷新");
    other_checker
        .ensure_fresh(&user_ctx)
        .await
        .expect("缓存刷新");
    assert!(checker.has(&user_ctx, PERM_READ), "撤销前应当有权限");
    assert!(other_checker.has(&user_ctx, PERM_READ), "撤销前应当有权限");

    // 撤销授权（**实例 1** 做的）。
    service
        .revoke_permission(&ctx, &r, PERM_READ)
        .await
        .expect("撤权应当成功");

    // **实例 2** 的下一个请求 → 立刻被拒（失效信号来自共享的数据库 epoch）。
    other_checker
        .ensure_fresh(&user_ctx)
        .await
        .expect("缓存刷新");
    assert!(
        !other_checker.has(&user_ctx, PERM_READ),
        "撤销后另一个实例的下一个请求就必须被拒（不允许出现「撤销后 60 秒内仍可用」）"
    );

    // 实例 1 同样被拒。
    checker.ensure_fresh(&user_ctx).await.expect("缓存刷新");
    assert!(!checker.has(&user_ctx, PERM_READ), "撤销后本实例也必须被拒");
}

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_revoked_data_scope_takes_effect_on_the_next_request() {
    let fixture = setup().await;
    let (service, _checker) = build(&fixture);
    let other_checker = Arc::new(RbacChecker::new(service.clone()));
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    let r = seed_role(
        &service,
        &ctx,
        "r-scope-revoke",
        "r-scope-revoke",
        DataScope::All,
        false,
    )
    .await;
    seed_grant_and_bind(&service, &ctx, &r, PERM_READ, None, &users.own).await;

    let before = set(&visible_ids(&fixture, &other_checker, &tenant, &users.own, PERM_READ).await);
    assert!(before.contains("item-b1"), "ALL 范围应当看到全部记录");

    // 把数据范围**收紧**为 `SELF`（撤销数据范围，不是权限点本身）。
    service
        .update_role_scope(&ctx, &r, DataScope::OnlySelf, None, false)
        .await
        .expect("改范围应当成功");

    let after = set(&visible_ids(&fixture, &other_checker, &tenant, &users.own, PERM_READ).await);
    assert!(
        !after.contains("item-b1"),
        "撤销数据范围后必须立即生效（另一个实例也一样）"
    );
    assert_ne!(before, after, "收紧范围后结果必须变化");
}

// ---------------------------------------------------------------------------
// AC-S-034 / AC-S-048：租户维度 + SQL 层过滤
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_role_tables_are_tenant_scoped_and_rls_protected() {
    let fixture = setup().await;
    let (service, _checker) = build(&fixture);
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let other_tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx_a = ctx_for(&tenant);
    let ctx_b = ctx_for(&other_tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    let r = seed_role(
        &service,
        &ctx_a,
        "r-tenant",
        "r-tenant",
        DataScope::All,
        false,
    )
    .await;
    seed_grant_and_bind(&service, &ctx_a, &r, PERM_READ, None, &users.own).await;
    let role_id = r.clone();

    // 1) 三张表都有 `tenant_id`。
    for table in [
        "platform_role",
        "platform_role_permission",
        "platform_user_role",
    ] {
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = $1 AND column_name = 'tenant_id'",
        )
        .bind(table)
        .fetch_all(&fixture.admin_pool)
        .await
        .expect("应当能查到列");
        assert!(
            !columns.is_empty(),
            "`{table}` 必须有 tenant_id（租户维度）"
        );
    }

    // 2) RLS：应用角色在租户 A 的上下文下**看不到**租户 B 的行。
    let query = TenantQuery::new("SELECT id FROM platform_role WHERE tenant_id = {tenant}");
    let rows_a: Vec<(String,)> = fixture
        .app_db
        .fetch_all(&ctx_a, &query)
        .await
        .expect("租户 A 的角色应当能读出");
    let rows_b: Vec<(String,)> = fixture
        .app_db
        .fetch_all(&ctx_b, &query)
        .await
        .expect("租户 B 的角色应当能读出");
    assert!(
        rows_a.iter().any(|(id,)| id == &role_id),
        "租户 A 应当看到自己的角色"
    );
    assert!(
        !rows_b.iter().any(|(id,)| id == &role_id),
        "租户 B 不得看到租户 A 的角色"
    );

    // 3) `pg_policies` 里能查到这三张表的 policy（**逐张列出**）。
    let policy_tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename FROM pg_policies \
         WHERE schemaname = 'public' AND tablename = ANY($1) ORDER BY tablename",
    )
    .bind(vec![
        "platform_role".to_string(),
        "platform_role_permission".to_string(),
        "platform_user_role".to_string(),
    ])
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能查到 policy");
    for table in [
        "platform_role",
        "platform_role_permission",
        "platform_user_role",
    ] {
        assert!(
            policy_tables.contains(&table.to_string()),
            "`{table}` 必须有 RLS policy（实际有：{policy_tables:?}）"
        );
    }
}

#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_scope_filter_is_rendered_into_the_sql() {
    let fixture = setup().await;
    let (service, checker) = build(&fixture);
    let tenant = format!("tenant-{}-{}", run_id(), unique_suffix());
    let ctx = ctx_for(&tenant);
    let users = users(&tenant, &unique_suffix());
    seed_base_fixture(&fixture, &users, &tenant).await;

    let r = seed_role(&service, &ctx, "r-sql", "r-sql", DataScope::OwnOrg, false).await;
    seed_grant_and_bind(&service, &ctx, &r, PERM_READ, None, &users.own).await;

    let user_ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(users.own);
    checker.ensure_fresh(&user_ctx).await.expect("缓存刷新");
    let filter = checker.effective_scope(&user_ctx, PERM_READ);
    match &filter {
        ScopeFilter::OrgNodes { column, nodes } => {
            assert_eq!(column, "org_node_id");
            assert_eq!(nodes, &vec!["dept-a".to_string()]);
        }
        other => panic!("应当是组织节点范围：{other:?}"),
    }

    // 渲染出的 SQL **含平台追加的范围条件**，且值**走参数绑定**。
    let query = TenantQuery::new(
        "SELECT id FROM sandbox_scope_item WHERE tenant_id = {tenant} ORDER BY id",
    )
    .with_scope(&filter)
    .expect("范围条件应当能渲染");
    let (sql, values) = query.render(&tenant).expect("应当能渲染");
    assert!(
        sql.contains("org_node_id = ANY("),
        "渲染出的 SQL 必须含平台追加的范围条件：{sql}"
    );
    assert!(
        !sql.contains("dept-a"),
        "范围条件的值必须走参数绑定（SQL 文本里不得出现字面值）：{sql}"
    );
    assert_eq!(values.len(), 2, "租户 + 范围各一个参数");
}
