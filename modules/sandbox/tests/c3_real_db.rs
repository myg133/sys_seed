//! C-3 契约检查的**真实库层**（REQ-006）。
//!
//! | 层 | 数据源 | 验什么 | 位置 |
//! |----|--------|--------|------|
//! | 内存层（已有） | `InMemoryRepository` | 契约语义：实现方拿不到别的租户数据 | `tests/contracts.rs` |
//! | **真实库层（本期）** | `base_test` 库 | RLS 实际生效、应用层与数据库层双重隔离 | 本文件 |
//!
//! 三条断言：
//!
//! 1. 以 tenant_a 的上下文 `list()` → 只返回 tenant_a 的行；
//! 2. 以 tenant_a 的上下文 `find(tenant_b 的 id)` → 返回 `None`；
//! 3. **绕过应用层**（不设 `app.tenant_id`）→ **查不到任何行**。
//!
//! 第 3 条最关键 —— 它证明「**即使应用层完全失守，数据库层仍不泄漏**」。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p sandbox --test c3_real_db -- --test-threads=2
//! ```
//!
//! 未设置 `DATABASE_URL_TEST` 时**判为失败**，不是「跳过」——
//! 见 [`required_test_database_url`] 的说明（跳过也算通过 = 假绿）。
//! 数据库名不是 `base_test` 时**直接失败** —— `base_dev` 是开发库，测试绝不能碰。

use std::path::{Path, PathBuf};

use platform_core::RequestContext;
use platform_core::extension::manifest::Manifest;
use platform_core::extension::repository::{ListQuery, Repository};
use platform_db::{DbOptions, TenantDb, apply_policies, require_isolation, unscoped_row_count};
use sandbox::tenant_repository::{PgTenantRepository, TenantItem};
use sqlx::postgres::PgPoolOptions;

/// 租户 A。
const TENANT_A: &str = "tenant-a";
/// 租户 B。
const TENANT_B: &str = "tenant-b";

/// 沙箱表名。
const TABLE: &str = "sandbox_tenant_item";

/// 部署准备用的 advisory lock 键（两个测试并行时不互相踩）。
const SETUP_LOCK: i64 = 6_006_006;

/// 防假绿护栏的子进程标记（防止护栏自己递归拉起子进程）。
const GUARD_CHILD: &str = "C3_GUARD_CHILD";

/// 测试夹具：管理员连接（建表 / 建策略）+ 应用角色连接（业务数据访问）。
struct Fixture {
    admin_db: TenantDb,
    app_db: TenantDb,
    repo: PgTenantRepository,
}

/// 测试库地址：只允许 `base_test`。
///
/// **缺 `DATABASE_URL_TEST` 是失败，不是「跳过」。**
///
/// 曾经这里「安静地返回 `None`」，于是断言一条都没跑、测试框架却报
/// `ok`（"3 passed"）—— 任何形如 `cargo test -- --include-ignored` 的 CI
/// 都会把「真实库层」当成通过。这就是**假绿**：结果绿了，护栏没生效。
///
/// 现在缺环境变量直接 **panic**：结果非绿，护栏不会骗人。
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
///
/// 应用角色必须是**不绕过 RLS** 的普通角色：超级用户与 `BYPASSRLS` 角色会绕过 RLS，
/// 租户隔离会 fail open（数据库层兜底失效）。
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

/// 准备真实库环境。
///
/// 顺序：应用角色 → 平台迁移 → 模块建表 → **平台按 manifest 生成 RLS 策略** → 应用角色连接。
///
/// ⚠️ 缺 `DATABASE_URL_TEST` 时**直接失败**（见 [`required_test_database_url`]）——
/// 「安静地跳过」会让这一层变成假绿。
async fn setup() -> Fixture {
    let url = required_test_database_url();

    let admin_pool = match PgPoolOptions::new().max_connections(2).connect(&url).await {
        Ok(pool) => pool,
        Err(err) => {
            // 设了 DATABASE_URL_TEST 就说明「这一轮就是要跑真实库」——
            // 库不可达是**环境坏了**，必须失败，不能假绿。
            panic!("测试库不可达（{url}）：{err}\n  需要集成测试时先执行 'bun run db:up'");
        }
    };

    let root = repo_root();

    // 整段准备过程串行化（两个测试并行时不会互相踩 DDL / policy）。
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

    // 2) 平台迁移（平台表 + 平台表的 RLS 策略）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    // 3) 模块建表（模块的表由模块自己的迁移负责；policy 不在这里 —— 见下一步）。
    let module_migrations = root.join("modules/sandbox/migrations");
    let mut up_files: Vec<PathBuf> = std::fs::read_dir(&module_migrations)
        .expect("模块迁移目录应可读")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".up.sql"))
        })
        .collect();
    up_files.sort();
    for path in up_files {
        let sql = read_file(&path);
        sqlx::raw_sql(&sql)
            .execute(&mut *conn)
            .await
            .expect("模块迁移应能应用");
    }

    // 4) **平台按 manifest 的 tenantScoped 段统一生成 RLS 策略**（模块不手写）。
    let manifest = Manifest::from_path(&root.join("modules/sandbox/manifest.json"))
        .expect("沙箱 manifest 应可读");
    let spec = manifest
        .tenant_scoped
        .clone()
        .expect("沙箱 manifest 必须声明 tenantScoped");
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    apply_policies(&admin_db, &spec)
        .await
        .expect("RLS 策略生成应能应用");

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    // 5) 应用角色连接（**不绕过 RLS** 的普通角色）。
    let app_url = app_role_url(&url);
    let app_db = TenantDb::connect(
        &app_url,
        DbOptions {
            // 只开一条连接 → 后面的「串租户」用例能确定复用的是**同一条物理连接**。
            max_connections: 1,
            ..DbOptions::default()
        },
    )
    .await
    .expect("应用角色连接串应可解析");

    // 隔离前提自检：角色不绕过 RLS + 声明的表真的有 policy。
    require_isolation(&app_db, &[spec])
        .await
        .expect("RLS 隔离前提必须成立（角色不绕过 RLS + 表有 policy）");

    let repo = PgTenantRepository::new(app_db.clone());
    Fixture {
        admin_db,
        app_db,
        repo,
    }
}

/// 灌入两个租户的**同名**数据（同一个标题，主键不同）。
async fn seed(repo: &PgTenantRepository, id_a: &str, id_b: &str) {
    let ctx_a = RequestContext::new().with_tenant(TENANT_A);
    let ctx_b = RequestContext::new().with_tenant(TENANT_B);
    for (ctx, id, tenant) in [(&ctx_a, id_a, TENANT_A), (&ctx_b, id_b, TENANT_B)] {
        // 先删后插（幂等）：主键是 (tenant_id, id)。
        repo.delete(ctx, &id.to_string())
            .await
            .unwrap_or_else(|err| panic!("删除夹具数据失败（tenant={tenant}, id={id}）：{err}"));
        repo.insert(
            ctx,
            &TenantItem {
                tenant_id: tenant.to_string(),
                id: id.to_string(),
                title: "同名记录".to_string(),
                revision: 0,
            },
        )
        .await
        .unwrap_or_else(|err| panic!("插入夹具数据失败（tenant={tenant}, id={id}）：{err}"));
    }
}

/// C-3 真实库层的三条断言。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn c3_real_db_isolates_tenants() {
    let fixture = setup().await;
    let ctx_a = RequestContext::new().with_tenant(TENANT_A);
    let id_a = "c3-a";
    let id_b = "c3-b";
    seed(&fixture.repo, id_a, id_b).await;

    // 断言 1：以 tenant_a 的上下文 list() → 只返回 tenant_a 的行。
    let list = fixture
        .repo
        .list(&ctx_a, &ListQuery::default())
        .await
        .expect("list() 应成功");
    assert!(
        !list.items.is_empty(),
        "tenant_a 应当能读到自己的数据（夹具没灌进去？）"
    );
    for item in &list.items {
        assert_eq!(
            item.tenant_id, TENANT_A,
            "list() 返回了别的租户的数据：{item:?}"
        );
    }
    assert!(
        list.items.iter().any(|item| item.id == id_a),
        "tenant_a 应当能读到自己的那条记录"
    );
    assert!(
        !list.items.iter().any(|item| item.id == id_b),
        "list() 返回了 tenant_b 的记录"
    );

    // 断言 2：以 tenant_a 的上下文 find(tenant_b 的 id) → 返回 None。
    let leaked = fixture
        .repo
        .find(&ctx_a, &id_b.to_string())
        .await
        .expect("find() 应成功");
    assert!(
        leaked.is_none(),
        "find() 命中了 tenant_b 的记录（跨租户泄漏）：{leaked:?}"
    );
    // 对照：自己的记录能查到。
    let own = fixture
        .repo
        .find(&ctx_a, &id_a.to_string())
        .await
        .expect("find() 应成功");
    assert!(own.is_some(), "tenant_a 应当能查到自己的记录");

    // 断言 3：**绕过应用层**（不设 app.tenant_id）→ 查不到任何行。
    //
    // 对照组用管理员连接（超级用户，绕过 RLS）—— 它能看到全部数据，
    // 证明「下面的 0 行」不是因为表里没数据，而是 RLS 挡住了。
    let admin_rows = unscoped_row_count(&fixture.admin_db, TABLE)
        .await
        .expect("对照查询应成功");
    assert!(
        admin_rows > 0,
        "对照组：管理员连接应当能看到数据（表里没有数据的话这条断言没有意义）"
    );
    let app_rows = unscoped_row_count(&fixture.app_db, TABLE)
        .await
        .expect("绕过应用层的查询应成功");
    assert_eq!(
        app_rows, 0,
        "RLS 必须 fail closed：不设 app.tenant_id 时一行都查不到（实际 {app_rows} 行）"
    );
}

/// 串租户用例：tenant_a 查完，**立刻**用**同一个连接池**查 tenant_b。
///
/// 连接池只开一条连接 → 一定是**同一条物理连接**。
/// 如果租户上下文是用普通 `SET`（会话级）设的，连接归还池子时仍带着 tenant_a，
/// 这一条就会失败 —— 所以这条用例专门验「用的是 `SET LOCAL`」。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tenant_context_does_not_leak_through_the_connection_pool() {
    let fixture = setup().await;
    let ctx_a = RequestContext::new().with_tenant(TENANT_A);
    let ctx_b = RequestContext::new().with_tenant(TENANT_B);
    let id_a = "leak-a";
    let id_b = "leak-b";
    seed(&fixture.repo, id_a, id_b).await;

    // 先用 tenant_a 查一遍（连接带着 tenant_a 的上下文归还池子）。
    let list_a = fixture
        .repo
        .list(&ctx_a, &ListQuery::default())
        .await
        .expect("list() 应成功");
    assert!(
        list_a.items.iter().any(|item| item.id == id_a),
        "tenant_a 应当能读到自己的数据"
    );

    // 立刻用**同一个连接池**查 tenant_b。
    let list_b = fixture
        .repo
        .list(&ctx_b, &ListQuery::default())
        .await
        .expect("list() 应成功");
    assert!(!list_b.items.is_empty(), "tenant_b 应当能读到自己的数据");
    for item in &list_b.items {
        assert_eq!(
            item.tenant_id,
            TENANT_B,
            "连接池串了租户：tenant_b 读到了 tenant {tenant:?} 的数据（{item:?}）",
            tenant = item.tenant_id
        );
    }
    assert!(
        !list_b.items.iter().any(|item| item.id == id_a),
        "tenant_b 读到了 tenant_a 的记录（串租户）"
    );
}

/// 应用角色**不能**绕过 RLS：角色自检必须成立。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_application_role_does_not_bypass_rls() {
    let fixture = setup().await;
    let report = platform_db::verify_policies(&fixture.app_db, &[])
        .await
        .expect("隔离自检应成功");
    assert!(
        !report.role_bypasses_rls,
        "应用角色 {} 绕过了 RLS —— 租户隔离会 fail open",
        report.role
    );
}

/// 防假绿护栏：缺 `DATABASE_URL_TEST` 时**不得**报 "3 passed"。
///
/// 守的是 C-3 真实库层曾经有过的假绿：缺环境变量时夹具「安静地返回」→
/// 断言一条没跑 → 框架却报 `ok`（"3 passed"）。任何形如
/// `cargo test -- --include-ignored` 的 CI 都会把「真实库层」当成通过。
///
/// 做法：把**自己这个测试二进制**再拉起一个子进程，**去掉** `DATABASE_URL_TEST`，
/// 只跑那三条真实库用例（`--skip` 掉本护栏，避免递归），然后断言：
///
/// 1. 子进程**退出码非 0**（真正承重的一条：结果必须非绿）；
/// 2. 子进程**一个用例都没报通过**（`0 passed`）；
/// 3. 子进程输出里**没有** "3 passed"。
#[test]
fn missing_database_url_test_does_not_report_success() {
    // 子进程里不再递归：这一层只验「缺环境变量 → 非零退出」。
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
    assert!(
        !combined.contains("3 passed"),
        "缺 DATABASE_URL_TEST 时不得报 \"3 passed\"（假绿）：\n{combined}"
    );
}
