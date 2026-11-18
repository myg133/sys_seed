//! 组织架构的**真实库层**（REQ-008）。
//!
//! | 层 | 数据源 | 验什么 |
//! |----|--------|--------|
//! | 单元层 | 纯函数 | 映射、位运算、祖先链计算 |
//! | **真实库层（本文件）** | `base_test` 库 | RLS 实际生效、跨表写的事务、离职撤销会话 |
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-org --test org_real_db -- --test-threads=2
//! ```
//!
//! 未设置 `DATABASE_URL_TEST` 时**判为失败**，不是「跳过」——
//! 见 [`required_test_database_url`] 的说明（跳过也算通过 = 假绿）。
//! 数据库名不是 `base_test` 时**直接失败** —— `base_dev` 是开发库，测试绝不能碰。

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use platform_core::RequestContext;
use platform_core::extension::manifest::{Manifest, TenantScopedSpec};
use platform_core::extension::notification::{DeliveryResult, NotificationMessage};
use platform_db::{
    DbOptions, TenantDb, TenantQuery, apply_policies, require_isolation, unscoped_row_count,
};
use platform_iam::store::{IamStore, RefreshRecord, STATUS_ACTIVE, SessionRecord};
use platform_org::audit::RecordingAuditRecorder;
use platform_org::entity::NodeType;
use platform_org::ldap::{InMemoryLdapSource, LdapEntry, LdapError, LdapSource, LdapSourceConfig};
use platform_org::notify::Notifier;
use platform_org::service::{NewMembership, NewNode, OrgService};
use platform_org::store::OrgStore;
use platform_org::sync::ConflictStrategy;
use sqlx::postgres::PgPoolOptions;

/// 租户 A。
const TENANT_A: &str = "tenant-a";
/// 租户 B。
const TENANT_B: &str = "tenant-b";

/// 组织节点表名。
const ORG_NODE_TABLE: &str = "org_node";

/// 部署准备用的 advisory lock 键（两个测试并行时不互相踩）。
const SETUP_LOCK: i64 = 6_008_001;

/// 库结构准备（迁移 + RLS 策略）—— **整个测试二进制只做一次**。
///
/// # 为什么必须只做一次（这是间歇性 `deadlock detected` 的根因）
///
/// 准备过程里的 `apply_policies` 跑的是 **DDL**（`ALTER TABLE … ENABLE ROW LEVEL SECURITY`），
/// 它要目标表的 **AccessExclusiveLock**；而**另一个用例的用例体**此刻可能正在
/// `INSERT INTO user_org` —— 外键检查要 `org_node` 的 **RowShareLock**。
/// 两条路径的**加锁顺序相反**（DDL：先 `org_node` 后 `user_org`；
/// 写入：先 `user_org` 后 `org_node`）→ **锁环** → `deadlock detected`（间歇性出现）。
///
/// 锁环里只有 `user_org` ↔ `org_node`（第一轮就有的 `user_org_node_fk`），
/// 不是新外键引入的；但**用例体直接写 `user_org`** 让这条既有竞态变成了可见的随机失败。
///
/// # 为什么「只做一次」就够了
///
/// 每个用例都要先过 `setup()` 才能进用例体，而 `setup()` 里等着这次准备 ——
/// 于是 **DDL 一定发生在任何用例体开始之前**，DDL 与数据写入**不再并发**，
/// 竞态被**消除**（不是用「调大超时」或「重试几次」把它盖住）。
/// 用例体之间仍然**真并发**（`--test-threads=2`），其它真实竞态照样暴露。
static SCHEMA_READY: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

/// 防假绿护栏的子进程标记（防止护栏自己递归拉起子进程）。
const GUARD_CHILD: &str = "ORG_GUARD_CHILD";

/// 记录通知内容的替身。
#[derive(Debug, Default)]
struct RecordingNotifier {
    sent: std::sync::Mutex<Vec<NotificationMessage>>,
}

impl Notifier for RecordingNotifier {
    fn notify<'a>(
        &'a self,
        _ctx: &'a platform_core::RequestContext,
        message: &'a NotificationMessage,
    ) -> platform_org::notify::BoxFuture<'a, anyhow::Result<DeliveryResult>> {
        let mut sent = self.sent.lock().unwrap_or_else(|err| err.into_inner());
        sent.push(message.clone());
        Box::pin(async { Ok(DeliveryResult::delivered("recording")) })
    }
}

/// 测试库地址：只允许 `base_test`。
///
/// **缺 `DATABASE_URL_TEST` 是失败，不是「跳过」。**
///
/// 曾经这里「安静地返回 `None`」，于是断言一条都没跑、测试框架却报 `ok`（"N passed"）——
/// 任何形如 `cargo test -- --include-ignored` 的 CI 都会把「真实库层」当成通过。
/// 这就是**假绿**：结果绿了，护栏没生效。
fn required_test_database_url() -> String {
    let url = std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "缺少 DATABASE_URL_TEST：真实库用例必须真的连库跑，不能静默跳过。\n  \
                 本地开发：先起库（bun run db:up），再跑 scripts/test-integration.sh。\n  \
                 库名必须恰好是 base_test（base_dev 是开发库，测试绝不能碰）。"
            )
        });
    let db = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_string();
    assert_eq!(
        db, "base_test",
        "集成测试只允许跑在 base_test 库上（当前：{db}）—— base_dev 是开发库，测试绝不能碰"
    );
    url
}

/// 把管理员连接串换成**应用角色**的连接串。
fn app_role_url(admin_url: &str) -> String {
    let (scheme, rest) = admin_url.split_once("://").expect("连接串必须带 scheme");
    let (userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    let _ = userinfo;
    format!("{scheme}://base_app:base@{host}")
}

/// 仓库根目录。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 读一个文件（缺失就 panic —— 夹具坏了要立刻暴露）。
fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()))
}

/// 测试夹具：管理员连接（建表 / 建策略）+ 应用角色连接（业务数据访问）。
struct Fixture {
    admin_db: TenantDb,
    /// 管理员连接（**仅测试用**：查 `pg_class` / `pg_policies` 这类系统表）。
    admin_pool: sqlx::PgPool,
    app_db: TenantDb,
    store: OrgStore,
    iam: IamStore,
    notifier: Arc<RecordingNotifier>,
    audit: Arc<RecordingAuditRecorder>,
}

impl Fixture {
    /// 构造服务（用记录型通知与审计替身）。
    fn service(&self) -> OrgService {
        OrgService::new(
            self.store.clone(),
            self.iam.clone(),
            self.notifier.clone(),
            self.audit.clone(),
        )
    }

    /// 已发出的通知。
    fn notifications(&self) -> Vec<NotificationMessage> {
        self.notifier
            .sent
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// 已产生的审计事件码。
    fn audit_codes(&self) -> Vec<String> {
        self.audit
            .events()
            .into_iter()
            .map(|event| event.code)
            .collect()
    }
}

/// 准备真实库环境。
///
/// 顺序：**库结构准备（整个测试二进制只做一次）** → 应用角色连接。
async fn setup() -> Fixture {
    let url = required_test_database_url();

    let admin_pool = match PgPoolOptions::new().max_connections(2).connect(&url).await {
        Ok(pool) => pool,
        Err(err) => {
            panic!("测试库不可达（{url}）：{err}\n  需要集成测试时先执行 'bun run db:up'");
        }
    };

    let root = repo_root();
    let manifest = Manifest::from_path(&root.join("crates/platform-org/manifest.json"))
        .expect("组织架构 manifest 应可读");
    let spec = manifest
        .tenant_scoped
        .clone()
        .expect("组织架构 manifest 必须声明 tenantScoped");

    // 库结构**只准备一次**：DDL 与数据写入不再并发（根因修复，见 SCHEMA_READY）。
    SCHEMA_READY
        .get_or_init(|| prepare_schema(&admin_pool, &root, &spec))
        .await;

    // 应用角色连接（**不绕过 RLS** 的普通角色）。
    let app_url = app_role_url(&url);
    let app_db = TenantDb::connect(
        &app_url,
        DbOptions {
            // 只开一条连接 → 「串租户」用例能确定复用的是**同一条物理连接**。
            max_connections: 1,
            ..DbOptions::default()
        },
    )
    .await
    .expect("应用角色连接串应能解析");

    // 隔离前提自检：角色不绕过 RLS + 声明的表真的有 policy。
    require_isolation(&app_db, &[spec])
        .await
        .expect("RLS 隔离前提必须成立（角色不绕过 RLS + 表有 policy）");

    let store = OrgStore::new(app_db.clone());
    let iam = IamStore::new(app_db.clone());
    Fixture {
        admin_db: TenantDb::from_pool(admin_pool.clone(), DbOptions::default()),
        admin_pool: admin_pool.clone(),
        app_db,
        store,
        iam,
        notifier: Arc::new(RecordingNotifier::default()),
        audit: Arc::new(RecordingAuditRecorder::new()),
    }
}

/// 准备库结构：应用角色 → 平台迁移 → **平台按 manifest 生成 RLS 策略**。
///
/// **整个测试二进制只跑一次**（理由见 [`SCHEMA_READY`]）。
async fn prepare_schema(admin_pool: &sqlx::PgPool, root: &Path, spec: &TenantScopedSpec) {
    // 整段准备过程串行化（跨进程也不互相踩 DDL / policy）。
    let mut conn = admin_pool.acquire().await.expect("应当能取到连接");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能拿到部署锁");

    // 1) 应用角色（RLS 前提：应用连接不能是超级用户）。
    let role_sql = read_file(&root.join("deploy/seed/02-app-role.sql"));
    sqlx::raw_sql(&role_sql)
        .execute(&mut *conn)
        .await
        .expect("应用角色准备失败");

    // 2) 平台迁移（平台表 + 组织架构的表）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    // 3) **平台按 manifest 的 tenantScoped 段统一生成 RLS 策略**（模块不手写）。
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    apply_policies(&admin_db, spec)
        .await
        .expect("RLS 策略生成应能应用");

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
}

/// 造一个本地账号（ID 与用户名都带唯一后缀 —— 测试共享同一个库）。
///
/// 返回 `(用户 ID, 用户名)` —— 目录条目要按**用户名**匹配本地账号。
async fn seed_user(
    fixture: &Fixture,
    ctx: &RequestContext,
    id: &str,
    username: &str,
) -> (String, String) {
    let id = &format!("{id}-{}", unique());
    let username = &format!("{username}-{}", unique());
    fixture
        .iam
        .insert_user(
            ctx,
            &platform_iam::store::UserRecord {
                id: id.to_string(),
                tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
                username: username.to_string(),
                email: Some(format!("{username}@example.com")),
                // 测试账号不会被登录；这里放一个**不可用**的占位哈希。
                password_hash: "$argon2id$v=19$m=1,t=1,p=1$c29tZXNhbHRzb21lc2FsdA$\
                                RdescudvJCsgt3ub+b+dWRWJTmaaJObG"
                    .to_string(),
                status: STATUS_ACTIVE.to_string(),
                mfa_enabled: false,
                must_change_password: false,
                failed_attempts: 0,
                locked_until: None,
                password_changed_at: time::OffsetDateTime::now_utc(),
                display_name: None,
                manager_id: None,
                departed_at: None,
                created_at: time::OffsetDateTime::now_utc(),
                updated_at: time::OffsetDateTime::now_utc(),
            },
        )
        .await
        .expect("账号应能插入");
    (id.clone(), username.clone())
}

/// 唯一后缀（测试共享同一个库 → 编码必须互不相同）。
fn unique() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 造一棵 company → dept → team 的组织树，返回三个节点 ID。
async fn seed_tree(service: &OrgService, ctx: &RequestContext) -> (String, String, String) {
    let suffix = unique();
    let company = service
        .create_node(
            ctx,
            NewNode {
                id: None,
                parent_id: None,
                node_type: NodeType::Company,
                code: format!("company-{suffix}"),
                name: "Example Co".to_string(),
            },
        )
        .await
        .expect("公司应能建");
    let dept = service
        .create_node(
            ctx,
            NewNode {
                id: None,
                parent_id: Some(company.id.clone()),
                node_type: NodeType::Dept,
                code: format!("dept-{suffix}"),
                name: "Engineering".to_string(),
            },
        )
        .await
        .expect("部门应能建");
    let team = service
        .create_node(
            ctx,
            NewNode {
                id: None,
                parent_id: Some(dept.id.clone()),
                node_type: NodeType::Team,
                code: format!("team-{suffix}"),
                name: "Platform".to_string(),
            },
        )
        .await
        .expect("小组应能建");
    (company.id, dept.id, team.id)
}

/// AC-S-011：组织树操作 —— 移动后子树查询立即反映新结构，成员关系不被破坏。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn moving_a_node_updates_the_subtree_immediately_and_keeps_memberships() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();
    let (company, dept, team) = seed_tree(&service, &ctx).await;

    // 另建一个部门，作为移动的目标。
    let suffix = unique();
    let other = service
        .create_node(
            &ctx,
            NewNode {
                id: None,
                parent_id: Some(company.clone()),
                node_type: NodeType::Dept,
                code: format!("dept-other-{suffix}"),
                name: "Operations".to_string(),
            },
        )
        .await
        .expect("部门应能建");

    // 成员在 team 上。
    let (user_id, _username) = seed_user(&fixture, &ctx, "u-move", "alice-move").await;
    let membership = service
        .add_member(
            &ctx,
            NewMembership {
                id: None,
                user_id: user_id.clone(),
                org_node_id: team.clone(),
                role: None,
                is_primary: true,
            },
        )
        .await
        .expect("加成员应成功");

    // 移动前：team 在 dept 下。
    let before = service
        .subtree(&ctx, &company)
        .await
        .expect("子树查询应成功");
    let mine = |nodes: &[platform_org::entity::OrgNode]| -> usize {
        nodes
            .iter()
            .filter(|node| {
                node.id == company || node.id == dept || node.id == team || node.id == other.id
            })
            .count()
    };
    assert_eq!(mine(&before), 4, "本轮建的四个节点应都在子树里");
    assert!(
        before
            .iter()
            .any(|node| node.id == team && node.parent_id.as_deref() == Some(dept.as_str())),
        "移动前 team 应挂在 dept 下"
    );

    // **移动** dept → other（连带 team 一起走）。
    service
        .move_node(&ctx, &dept, Some(&other.id))
        .await
        .expect("移动应成功");

    // 断言 1：子树查询**立即**反映新结构（权威是 parent_id）。
    let after = service
        .subtree(&ctx, &company)
        .await
        .expect("子树查询应成功");
    let moved_dept = after
        .iter()
        .find(|node| node.id == dept)
        .expect("部门应还在");
    assert_eq!(moved_dept.parent_id.as_deref(), Some(other.id.as_str()));
    let moved_team = after
        .iter()
        .find(|node| node.id == team)
        .expect("小组应还在");
    assert_eq!(
        moved_team.parent_id.as_deref(),
        Some(dept.as_str()),
        "team 的父节点没跟着变"
    );

    // 断言 2：旧的子树视图已经变了 —— team 不再是 dept 的子孙。
    let old_branch = service
        .subtree(&ctx, &other.id)
        .await
        .expect("子树查询应成功");
    assert!(old_branch.iter().any(|node| node.id == team));

    // 断言 3：`path_labels` 与 `parent_id` **保持一致**。
    let broken = service
        .verify_path_labels(&ctx)
        .await
        .expect("一致性校验应成功");
    assert!(
        broken.is_empty(),
        "移动后 path_labels 应与 parent_id 一致：{broken:?}"
    );

    // 断言 4：**成员关系不被移动破坏**。
    let members = service
        .members_of_subtree(&ctx, &other.id)
        .await
        .expect("子树成员查询应成功");
    assert!(
        members.iter().any(|item| item.id == membership.id),
        "移动节点后成员关系丢了"
    );
    let members = service
        .list_members(&ctx, Some(&user_id), None)
        .await
        .expect("成员查询应成功");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].org_node_id, team);
}

/// AC-S-011：`path_labels` 与 `parent_id` 不一致时**以 parent_id 为准并可重建**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn path_labels_are_detected_and_rebuilt_from_the_authoritative_parent_id() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();
    let (company, _dept, team) = seed_tree(&service, &ctx).await;

    // 正常状态下没有不一致。
    assert!(
        service
            .verify_path_labels(&ctx)
            .await
            .expect("校验应成功")
            .is_empty()
    );

    // 人为破坏一个节点的 path_labels（模拟历史脏数据）。
    fixture
        .store
        .update_node_path(&ctx, &team, &["stale".to_string()])
        .await
        .expect("应当能改 path_labels");

    // 校验**报得出**不一致。
    let broken = service.verify_path_labels(&ctx).await.expect("校验应成功");
    assert_eq!(broken, vec![team.clone()], "应报出 path_labels 损坏的节点");

    // 重建：以 parent_id 为准。
    let fixed = service.rebuild_path_labels(&ctx).await.expect("重建应成功");
    assert_eq!(fixed, 1);
    assert!(
        service
            .verify_path_labels(&ctx)
            .await
            .expect("校验应成功")
            .is_empty()
    );

    // 重建后的链与 parent_id 一致。
    let node = service.get_node(&ctx, &team).await.expect("节点应能查到");
    assert_eq!(
        node.path_labels,
        vec![company, node.path_labels[1].clone(), team]
    );
}

/// AC-S-018：租户隔离 —— 跨租户 id 访问被拒，RLS 实际生效。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn organization_data_is_isolated_per_tenant() {
    let fixture = setup().await;
    let ctx_a = RequestContext::new().with_tenant(TENANT_A);
    let ctx_b = RequestContext::new().with_tenant(TENANT_B);
    let service = fixture.service();

    // 两个租户各有**同名**节点（唯一区别是租户）。
    for ctx in [&ctx_a, &ctx_b] {
        service
            .create_node(
                ctx,
                NewNode {
                    id: None,
                    parent_id: None,
                    node_type: NodeType::Company,
                    code: format!("same-code-{}", unique()),
                    name: "Same Name".to_string(),
                },
            )
            .await
            .expect("节点应能建");
    }

    let nodes_a = service.list_nodes(&ctx_a, None).await.expect("列表应成功");
    let nodes_b = service.list_nodes(&ctx_b, None).await.expect("列表应成功");
    assert!(!nodes_a.is_empty() && !nodes_b.is_empty());
    for node in &nodes_a {
        assert_eq!(node.tenant_id, TENANT_A, "tenant_a 读到了别的租户的数据");
    }
    for node in &nodes_b {
        assert_eq!(node.tenant_id, TENANT_B, "tenant_b 读到了别的租户的数据");
    }

    // 跨租户 id 访问 → 查不到（不是 403，是「不存在」）。
    let id_b = nodes_b[0].id.clone();
    let leaked = fixture
        .store
        .find_node(&ctx_a, &id_b)
        .await
        .expect("查询应成功");
    assert!(
        leaked.is_none(),
        "tenant_a 读到了 tenant_b 的节点（跨租户泄漏）"
    );

    // **绕过应用层**（不设 app.tenant_id）→ 一行都查不到。
    let admin_rows = unscoped_row_count(&fixture.admin_db, ORG_NODE_TABLE)
        .await
        .expect("对照查询应成功");
    assert!(admin_rows > 0, "对照组：管理员连接应当能看到数据");
    let app_rows = unscoped_row_count(&fixture.app_db, ORG_NODE_TABLE)
        .await
        .expect("绕过应用层的查询应成功");
    assert_eq!(
        app_rows, 0,
        "RLS 必须 fail closed：不设 app.tenant_id 时一行都查不到（实际 {app_rows} 行）"
    );
}

/// AC-S-018：组织表按 `tenantScoped` 声明生成了 policy，且 RLS 已打开。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn every_organization_table_has_a_tenant_policy() {
    let fixture = setup().await;

    let rows: Vec<(String, bool, i64)> = sqlx::query_as(
        "SELECT c.relname, c.relrowsecurity, \
         (SELECT count(*) FROM pg_policies p WHERE p.schemaname = c.relnamespace::regnamespace::text \
          AND p.tablename = c.relname) \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'public' AND c.relname IN \
         ('org_node', 'user_org', 'user_position', 'external_identity', 'sync_conflict', 'sync_source_state')",
    )
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("查询 pg_class 应成功");

    assert_eq!(rows.len(), 6, "六张组织表都应存在：{rows:?}");
    for (name, row_level_security, policies) in rows {
        assert!(row_level_security, "{name} 没有开启 RLS");
        assert!(policies > 0, "{name} 没有 policy");
    }
}

/// AC-S-015：离职自动禁用 —— 账号禁用 + **旧 refresh token 立即失效** + 解绑 + 审计 + 通知。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn departure_disables_the_account_and_revokes_every_session() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();
    let (_company, _dept, team) = seed_tree(&service, &ctx).await;

    let (user_id, _username) = seed_user(&fixture, &ctx, "u-depart", "alice-depart").await;
    service
        .add_member(
            &ctx,
            NewMembership {
                id: None,
                user_id: user_id.clone(),
                org_node_id: team.clone(),
                role: None,
                is_primary: true,
            },
        )
        .await
        .expect("加成员应成功");

    // 造一个会话与一个 refresh token（模拟「之前拿到的 refresh token」）。
    let now = time::OffsetDateTime::now_utc();
    let session_id = format!("s-{}", unique());
    let refresh_id = format!("r-{}", unique());
    let token_hash = format!("hash-of-the-old-refresh-token-{}", unique());
    fixture
        .iam
        .insert_session(
            &ctx,
            &SessionRecord {
                id: session_id.clone(),
                tenant_id: TENANT_A.to_string(),
                user_id: user_id.clone(),
                device_label: "laptop".to_string(),
                ip: "127.0.0.1".to_string(),
                user_agent: "test".to_string(),
                family_id: "family-1".to_string(),
                created_at: now,
                last_used_at: now,
                revoked_at: None,
            },
        )
        .await
        .expect("会话应能插入");
    fixture
        .iam
        .insert_refresh_token(
            &ctx,
            &RefreshRecord {
                id: refresh_id,
                tenant_id: TENANT_A.to_string(),
                user_id: user_id.clone(),
                session_id: session_id.clone(),
                family_id: "family-1".to_string(),
                token_hash: token_hash.clone(),
                issued_at: now,
                expires_at: now + time::Duration::days(30),
                used_at: None,
                revoked_at: None,
                replaced_by: None,
            },
        )
        .await
        .expect("refresh token 应能插入");

    // 离职。
    let outcome = service
        .disable_departed_user(&ctx, &user_id, "directory-disabled")
        .await
        .expect("离职禁用应成功");

    // 断言 1：账号被禁用。
    let user = fixture
        .iam
        .find_user_by_id(&ctx, &user_id)
        .await
        .expect("查询应成功")
        .expect("账号应还在");
    assert_eq!(user.status, "disabled");
    assert!(user.departed_at.is_some());

    // 断言 2：**旧的 refresh token 立即失效**（不是等 access TTL 到期）。
    assert_eq!(outcome.refresh_tokens_revoked, 1);
    assert_eq!(outcome.sessions_revoked, 1);
    let token = fixture
        .iam
        .find_refresh_by_hash_for_update(&ctx, &token_hash)
        .await
        .expect("查询应成功");
    let token = token.expect("记录应还在（已作废）");
    assert!(
        token.revoked_at.is_some(),
        "旧的 refresh token 必须立即作废"
    );
    let sessions = fixture
        .iam
        .list_sessions(&ctx, &user_id)
        .await
        .expect("查询应成功");
    assert!(sessions.iter().all(|item| item.revoked_at.is_some()));

    // 断言 3：成员关系已解绑。
    assert_eq!(outcome.memberships_removed, 1);
    let members = service
        .list_members(&ctx, Some(&user_id), None)
        .await
        .expect("成员查询应成功");
    assert!(members.is_empty(), "离职后成员关系应被解绑");

    // 断言 4：产生审计事件。
    assert!(
        fixture
            .audit_codes()
            .iter()
            .any(|code| code == "org.user.departed"),
        "离职应产生审计事件：{:?}",
        fixture.audit_codes()
    );

    // 断言 5：发出通知。
    assert!(!fixture.notifications().is_empty(), "离职应发出通知");
}

/// AC-S-014：冲突是**字段级**的，裁决后按裁决生效，**不静默覆盖**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn conflicts_are_recorded_per_field_and_applied_after_adjudication() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();

    let (user_id, username) = seed_user(&fixture, &ctx, "u-conflict", "alice-conflict").await;

    // 目录里同一个人的邮箱**不一样**。
    let source_name = format!("corp-ad-{}", unique());
    let remote_email = format!("remote-{}@example.com", unique());
    let config = LdapSourceConfig::from_json(&format!(
        r#"{{
          "source": "{source_name}",
          "baseDn": "DC=corp",
          "filter": "(objectClass=user)",
          "attributes": {{
            "external_id": "objectGUID",
            "username": "sAMAccountName",
            "email": "mail",
            "disabled": {{ "op": "bit_and", "attr": "userAccountControl", "mask": 2 }}
          }}
        }}"#
    ))
    .expect("配置应能解析");
    let source = InMemoryLdapSource::new(vec![LdapEntry::new(
        "CN=alice-conflict,DC=corp",
        &[
            ("objectGUID", vec!["guid-1"]),
            ("sAMAccountName", vec![username.as_str()]),
            ("mail", vec![remote_email.as_str()]),
            ("userAccountControl", vec!["512"]),
        ],
    )]);

    // 同步（默认策略 manual）。
    let summary = service
        .sync_ldap(&ctx, &config, &source, ConflictStrategy::Manual)
        .await
        .expect("同步应成功");
    assert_eq!(summary.fetched, 1);
    assert_eq!(summary.conflicts, 1, "应产出一个字段级冲突");

    // 断言 1：冲突是**字段级**的（不是整条记录）。
    let conflicts = service
        .list_conflicts(&ctx, None)
        .await
        .expect("冲突查询应成功");
    let conflict = conflicts
        .iter()
        .find(|item| item.field == "email" && item.source == source_name)
        .expect("应有 email 字段的冲突");
    assert_eq!(conflict.source, source_name);
    assert_eq!(conflict.external_id, "guid-1");
    assert_eq!(conflict.local_value, format!("{username}@example.com"));
    assert_eq!(conflict.remote_value, remote_email);
    assert_eq!(
        conflict.status,
        platform_org::entity::ConflictStatus::Pending
    );

    // 断言 2：**没有静默覆盖** —— 本地邮箱还是本地的。
    let user = fixture
        .iam
        .find_user_by_id(&ctx, &user_id)
        .await
        .expect("查询应成功")
        .expect("账号应还在");
    assert_eq!(
        user.email.as_deref(),
        Some(format!("{username}@example.com").as_str()),
        "manual 策略下不得覆盖本地值"
    );

    // 断言 3：人工裁决「采用外部值」后生效。
    let resolved = service
        .resolve_conflict(
            &ctx,
            &conflict.id,
            platform_org::entity::ConflictResolution::TakeRemote,
            None,
            "admin@example.com",
        )
        .await
        .expect("裁决应成功");
    assert_eq!(
        resolved.resolution,
        Some(platform_org::entity::ConflictResolution::TakeRemote)
    );
    let user = fixture
        .iam
        .find_user_by_id(&ctx, &user_id)
        .await
        .expect("查询应成功")
        .expect("账号应还在");
    assert_eq!(user.email.as_deref(), Some(remote_email.as_str()));

    // 断言 4：再同步一次**不再**产冲突（两边已一致）。
    let summary = service
        .sync_ldap(&ctx, &config, &source, ConflictStrategy::Manual)
        .await
        .expect("同步应成功");
    assert_eq!(summary.conflicts, 0, "裁决后应无冲突");
}

/// AC-S-015 / AC-S-016：AD 的**位运算**离职判定会触发自动禁用。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_ad_disabled_bit_triggers_the_departure_flow() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();

    let (user_id, username) = seed_user(&fixture, &ctx, "u-ad", "alice-ad").await;

    let source_name = format!("corp-ad-{}", unique());
    let config = LdapSourceConfig::from_json(&format!(
        r#"{{
          "source": "{source_name}",
          "baseDn": "DC=corp",
          "filter": "(objectClass=user)",
          "attributes": {{
            "external_id": "objectGUID",
            "username": "sAMAccountName",
            "disabled": {{ "op": "bit_and", "attr": "userAccountControl", "mask": 2 }}
          }}
        }}"#
    ))
    .expect("配置应能解析");
    // 514 = 512 | 2（ACCOUNTDISABLE）
    let source = InMemoryLdapSource::new(vec![LdapEntry::new(
        "CN=alice-ad,DC=corp",
        &[
            ("objectGUID", vec!["guid-ad"]),
            ("sAMAccountName", vec![username.as_str()]),
            ("userAccountControl", vec!["514"]),
        ],
    )]);

    let summary = service
        .sync_ldap(&ctx, &config, &source, ConflictStrategy::Manual)
        .await
        .expect("同步应成功");
    assert_eq!(summary.departures, 1, "离职位应触发自动禁用");

    let user = fixture
        .iam
        .find_user_by_id(&ctx, &user_id)
        .await
        .expect("查询应成功")
        .expect("账号应还在");
    assert_eq!(user.status, "disabled");
}

/// AC-S-017：节点删除前成员必须先处理（不留悬空成员）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn deleting_a_node_with_members_requires_handling_them_first() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();
    let (_company, dept, team) = seed_tree(&service, &ctx).await;

    let (user_id, _username) = seed_user(&fixture, &ctx, "u-del", "alice-del").await;
    service
        .add_member(
            &ctx,
            NewMembership {
                id: None,
                user_id: user_id.clone(),
                org_node_id: team.clone(),
                role: None,
                is_primary: false,
            },
        )
        .await
        .expect("加成员应成功");

    // 有成员的节点：**拒绝**删除。
    let error = service
        .delete_node(&ctx, &team, None)
        .await
        .expect_err("有成员的节点不应被删掉");
    assert_eq!(error.code(), platform_org::error_code::ORG_NODE_HAS_MEMBERS);

    // 指定迁移目标 → 成员被迁走，节点才删得掉。
    service
        .delete_node(&ctx, &team, Some(&dept))
        .await
        .expect("迁移成员后应能删除");
    let members = service
        .list_members(&ctx, Some(&user_id), None)
        .await
        .expect("成员查询应成功");
    assert_eq!(members.len(), 1, "成员应被迁到别的节点");
    assert_eq!(members[0].org_node_id, dept);
    let nodes = service.list_nodes(&ctx, None).await.expect("列表应成功");
    assert!(!nodes.iter().any(|node| node.id == team), "节点应已删除");
}

/// AC-S-017：节点移动后，该节点下的成员收到通知。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn members_receive_a_notification_when_their_node_moves() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();
    let (company, dept, _team) = seed_tree(&service, &ctx).await;

    let (user_id, _username) = seed_user(&fixture, &ctx, "u-move-notify", "alice-notify").await;
    service
        .add_member(
            &ctx,
            NewMembership {
                id: None,
                user_id: user_id.clone(),
                org_node_id: dept.clone(),
                role: None,
                is_primary: true,
            },
        )
        .await
        .expect("加成员应成功");

    let suffix = unique();
    let other = service
        .create_node(
            &ctx,
            NewNode {
                id: None,
                parent_id: Some(company),
                node_type: NodeType::Dept,
                code: format!("dept-notify-target-{suffix}"),
                name: "Operations".to_string(),
            },
        )
        .await
        .expect("部门应能建");

    service
        .move_node(&ctx, &dept, Some(&other.id))
        .await
        .expect("移动应成功");

    let notifications = fixture.notifications();
    assert!(
        !notifications.is_empty(),
        "节点移动应发出通知（受影响成员）"
    );
}

/// AC-S-012：同步幂等 —— 连续两次全量同步**不产生重复条目**。
///
/// 目标端是**真的 SCIM mock 服务**（`tests/common/mod.rs`），推送走**真的 HTTP**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn two_consecutive_full_syncs_do_not_duplicate_entries() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();
    let (_company, _dept, _team) = seed_tree(&service, &ctx).await;
    seed_user(&fixture, &ctx, "u-scim", "alice-scim").await;

    let server = common::start_mock_scim_server().await;
    let config = platform_org::scim::ScimProviderConfig::new("corp-scim", server.base_url.clone());
    let gateway = platform_org::scim::HttpScimGateway::new();

    // 第一次全量同步。
    let summary = service
        .full_scim_sync(&ctx, &config)
        .await
        .expect("全量同步应成功");
    assert!(summary.fetched >= 2, "应排入用户与组织节点的推送");
    let outcomes = service.drain_outbox(&gateway).await;
    assert!(outcomes.iter().all(|outcome| outcome.ok), "推送应成功");
    let users_after_first = server.user_count();
    let groups_after_first = server.group_count();
    assert!(users_after_first >= 1, "目标端应有 User");
    assert!(
        groups_after_first >= 3,
        "目标端应有 Group（company/dept/team）"
    );

    // 第二次全量同步（**幂等**）。
    let summary = service
        .full_scim_sync(&ctx, &config)
        .await
        .expect("全量同步应成功");
    assert!(summary.fetched >= 4, "第二次应排入同样多的任务");
    let outcomes = service.drain_outbox(&gateway).await;
    assert!(outcomes.iter().all(|outcome| outcome.ok), "推送应成功");

    // 断言：**条目数不变**（PUT 语义）。
    assert_eq!(
        server.user_count(),
        users_after_first,
        "连续两次全量同步不应产生重复条目"
    );
    assert_eq!(
        server.group_count(),
        groups_after_first,
        "连续两次全量同步不应产生重复条目"
    );
}

/// 永远连不上的 LDAP 源（用来制造同步失败）。
struct UnreachableLdapSource;

impl LdapSource for UnreachableLdapSource {
    fn search(
        &self,
        _base_dn: &str,
        _filter: &str,
        _attributes: &[String],
        _since: Option<time::OffsetDateTime>,
    ) -> platform_org::ldap::BoxFuture<'_, Result<Vec<LdapEntry>, LdapError>> {
        Box::pin(async {
            Err(LdapError::Connection(
                "the directory server is unreachable".to_string(),
            ))
        })
    }
}

/// 直接往 `user_org` 插一行（**绕过服务层** —— 证明拒绝发生在**数据库层**）。
///
/// 用**应用角色**（`base_app`，不绕过 RLS）连接，与生产路径同权。
async fn insert_membership_directly(
    fixture: &Fixture,
    ctx: &RequestContext,
    id: &str,
    user_id: &str,
    org_node_id: &str,
) -> Result<u64, platform_db::DbError> {
    let query = TenantQuery::new(
        "INSERT INTO public.user_org (tenant_id, id, user_id, org_node_id) \
         VALUES ({tenant}, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(user_id.to_string())
    .bind(org_node_id.to_string());
    fixture.app_db.execute(ctx, &query).await
}

/// 中 1：同步失败必须**留痕**（错误计数递增 + 错误摘要），成功一次则**清零**。
///
/// 这是「同步状态的可见性（上次同步时间、**同步错误计数**）」的承重用例：
/// 修复前 `upsert_sync_state` **只有成功路径**一个调用点，写的是字面量 `error_count: 0`，
/// 于是运维侧永远看不到同步失败。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_failed_sync_records_the_error_and_a_successful_one_clears_it() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();

    let source_name = format!("ldap-src-{}", unique());
    let config = LdapSourceConfig::from_json(&format!(
        r#"{{
          "source": "{source_name}",
          "baseDn": "DC=corp,DC=example,DC=com",
          "filter": "(objectClass=user)",
          "attributes": {{}}
        }}"#
    ))
    .expect("配置应能解析");

    // 1) 同步失败 → 错误计数递增、错误摘要写入、上次**成功**同步时间不动。
    let error = service
        .sync_ldap(
            &ctx,
            &config,
            &UnreachableLdapSource,
            ConflictStrategy::Manual,
        )
        .await
        .expect_err("目录不可达时同步应失败");
    assert!(
        error.to_string().contains("unreachable"),
        "错误摘要应带根因：{error}"
    );

    let failed = service
        .sync_state(&ctx, &source_name)
        .await
        .expect("应能读同步状态")
        .expect("失败也必须留下同步状态");
    assert!(failed.error_count > 0, "错误计数应递增：{failed:?}");
    assert!(
        failed.last_error.is_some(),
        "错误摘要应写入 last_error：{failed:?}"
    );
    assert!(
        failed.last_sync_at.is_none(),
        "失败时上次成功同步时间不应被改动：{failed:?}"
    );

    // 2) 再成功一次 → 计数清零、错误摘要清空、上次同步时间更新。
    service
        .sync_ldap(
            &ctx,
            &config,
            &InMemoryLdapSource::default(),
            ConflictStrategy::Manual,
        )
        .await
        .expect("目录可达时同步应成功");

    let ok = service
        .sync_state(&ctx, &source_name)
        .await
        .expect("应能读同步状态")
        .expect("成功后同步状态必须存在");
    assert_eq!(ok.error_count, 0, "成功后错误计数应清零：{ok:?}");
    assert!(ok.last_error.is_none(), "成功后错误摘要应清空：{ok:?}");
    assert!(
        ok.last_sync_at.is_some(),
        "成功后上次同步时间应更新：{ok:?}"
    );
}

/// 中 2：**幽灵 user_id** 必须被**数据库层**拒绝（不是服务层报错）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_database_refuses_a_ghost_user_id() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();
    let (_company, _dept, team) = seed_tree(&service, &ctx).await;

    let ghost = format!("ghost-user-{}", unique());
    let error =
        insert_membership_directly(&fixture, &ctx, &format!("m-{}", unique()), &ghost, &team)
            .await
            .expect_err("幽灵 user_id 必须被数据库拒绝");

    let message = error.to_string();
    assert!(
        message.contains("user_org_user_fk"),
        "应指出是哪条外键违约：{message}"
    );
    assert!(message.contains("foreign key"), "应是外键违约：{message}");
}

/// 中 2：**跨租户 user_id** 必须被**数据库层**拒绝。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_database_refuses_a_cross_tenant_user_id() {
    let fixture = setup().await;
    let ctx_a = RequestContext::new().with_tenant(TENANT_A);
    let ctx_b = RequestContext::new().with_tenant(TENANT_B);
    let service = fixture.service();
    let (_company, _dept, team) = seed_tree(&service, &ctx_a).await;

    // 租户 B 的**真实**账号（存在，但不属于租户 A）。
    let (user_b, _username_b) = seed_user(&fixture, &ctx_b, "u-other-tenant", "bob-other").await;

    let error =
        insert_membership_directly(&fixture, &ctx_a, &format!("m-{}", unique()), &user_b, &team)
            .await
            .expect_err("跨租户 user_id 必须被数据库拒绝");

    let message = error.to_string();
    assert!(
        message.contains("user_org_user_fk"),
        "应指出是哪条外键违约：{message}"
    );
    assert!(message.contains("foreign key"), "应是外键违约：{message}");
}

/// 信息 1：`add_member` 传**不存在的 user_id** → **4xx**（不是 500）。
///
/// 外键违约 = 「引用的那一行不存在」——这是**客户端错误**：
/// 服务层要把它映射成 404 `NOT_FOUND`，而不是 500 `INTERNAL_ERROR`
/// （500 会把「用户不存在」说成「服务端坏了」）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn adding_a_member_for_an_unknown_user_is_a_client_error() {
    let fixture = setup().await;
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let service = fixture.service();
    let (_company, _dept, team) = seed_tree(&service, &ctx).await;

    let error = service
        .add_member(
            &ctx,
            NewMembership {
                id: None,
                user_id: format!("ghost-user-{}", unique()),
                org_node_id: team,
                role: None,
                is_primary: false,
            },
        )
        .await
        .expect_err("不存在的用户必须被拒绝");

    assert_eq!(error.code(), "NOT_FOUND", "错误码：{error}");
    assert_eq!(
        error.status(),
        axum::http::StatusCode::NOT_FOUND,
        "状态码：{error}"
    );
    assert_eq!(
        error
            .params()
            .get("resource")
            .and_then(|value| value.as_str()),
        Some("user"),
        "应说明缺的是用户：{error}"
    );
}

/// 防假绿护栏：缺 `DATABASE_URL_TEST` 时**不得**报 "N passed"。
///
/// 做法：把**自己这个测试二进制**再拉起一个子进程，**去掉** `DATABASE_URL_TEST`，
/// 只跑真实库用例（`--skip` 掉本护栏，避免递归），然后断言：
///
/// 1. 子进程**退出码非 0**（真正承重的一条：结果必须非绿）；
/// 2. 子进程**一个用例都没报通过**（`0 passed`）。
#[test]
fn missing_database_url_test_does_not_report_success() {
    if std::env::var_os(GUARD_CHILD).is_some() {
        return;
    }

    let exe = std::env::current_exe().expect("应当能拿到当前测试二进制路径");
    let output = std::process::Command::new(exe)
        .args([
            "--include-ignored",
            "--skip",
            "missing_database_url_test_does_not_report_success",
            "--test-threads=1",
        ])
        .env_remove("DATABASE_URL_TEST")
        .env(GUARD_CHILD, "1")
        .output()
        .expect("应当能拉起子进程");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !output.status.success(),
        "缺 DATABASE_URL_TEST 时子进程必须失败（它却成功退出了）：\n{combined}"
    );
    assert!(
        combined.contains("0 passed"),
        "缺 DATABASE_URL_TEST 时一个用例都不该报通过：\n{combined}"
    );
}
