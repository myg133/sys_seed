//! 租户配额的**真实库层**（REQ-006）。
//!
//! 验三件事（QA 首轮问题 #6：配额没有自动化用例）：
//!
//! 1. **计数**：用量累加**落库**（`add_quota_usage` 返回累加后的值）；
//! 2. **超限判定**：设了限额后 `is_exceeded` 随用量变化；
//! 3. **重启后计数仍在**：换一个**全新的连接池**（内存态全丢，等价于进程重启）
//!    再读，用量不变 —— 证明计数在 DB 里，不在内存里。
//!
//! # 为什么「重启后仍在」必须真的验
//!
//! 内存计数**重启就丢**：服务重启、扩容、滚动发布都会把用量清零 ——
//! 配额就变成了「形同虚设的累计值」。所以用量必须落在
//! `platform_tenant_quota` 表里（见 `platform_db::quota` 模块的说明）。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-db --test quota_real_db -- --test-threads=2
//! ```
//!
//! 未设置 `DATABASE_URL_TEST` 时**判为失败**，不是「跳过」——
//! 跳过也算通过 = 假绿（见文件末尾的护栏用例）。
//! 数据库名不是 `base_test` 时**直接失败** —— `base_dev` 是开发库，测试绝不能碰。

use std::path::{Path, PathBuf};

use platform_core::RequestContext;
use platform_db::{DbOptions, QUOTA_TABLE, TenantDb};
use sqlx::postgres::PgPoolOptions;

/// 配额用例的租户 A（专用名，不与其它用例的夹具数据混在一起）。
const TENANT_A: &str = "quota-test-a";
/// 配额用例的租户 B（对照组）。
const TENANT_B: &str = "quota-test-b";
/// 被计量的指标名。
const METRIC: &str = "api_calls";
/// 防假绿护栏的子进程标记（防止护栏自己递归拉起子进程）。
const GUARD_CHILD: &str = "QUOTA_GUARD_CHILD";

/// 测试库地址：只允许 `base_test`。
///
/// **缺 `DATABASE_URL_TEST` 是失败，不是「跳过」。**
///
/// 「安静地返回」会让断言一条都没跑、测试框架却报 `ok` —— 任何形如
/// `cargo test -- --include-ignored` 的 CI 都会把「真实库层」当成通过。
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

/// 准备真实库环境，返回**应用角色**的连接串。
///
/// 顺序：平台迁移（建表）→ 应用角色脚本（建角色 + 授权）→ 清夹具数据。
///
/// ⚠️ 顺序是**故意**的：迁移先建表，角色脚本后跑 —— 这样角色脚本走的是
/// 「**存量表**授权」那条路径（`GRANT … ON ALL TABLES IN SCHEMA public`），
/// 而不是靠 `ALTER DEFAULT PRIVILEGES` 兜住未来新建的表。
/// 两条路径都要能用（见 deploy/RUNBOOK.md §3.4.3）。
///
/// 缺 `DATABASE_URL_TEST` 时**直接失败**（见 [`required_test_database_url`]）。
async fn setup() -> String {
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

    // 1) 平台迁移（建表 + 表的 RLS 策略）。
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&admin_pool).await.expect("平台迁移应能应用");

    // 2) 应用角色（RLS 前提）+ 存量表授权（表已经建好了）。
    let role_sql = read_file(&root.join("deploy/seed/02-app-role.sql"));
    sqlx::raw_sql(&role_sql)
        .execute(&admin_pool)
        .await
        .expect("应用角色准备失败");

    // 3) 清掉本用例的夹具数据（幂等：反复跑不会把计数叠加上去）。
    sqlx::query("DELETE FROM platform_tenant_quota WHERE tenant_id IN ($1, $2)")
        .bind(TENANT_A)
        .bind(TENANT_B)
        .execute(&admin_pool)
        .await
        .expect("清理配额夹具数据应成功");

    app_role_url(&url)
}

/// 配额的三个断言：计数 + 超限判定 + 重启后计数仍在。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn quota_usage_is_counted_in_the_database_and_survives_a_restart() {
    let app_url = setup().await;
    let ctx_a = RequestContext::new().with_tenant(TENANT_A);
    let ctx_b = RequestContext::new().with_tenant(TENANT_B);

    let db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    // 1) 计数：两次累加 → 15。
    let used = db
        .add_quota_usage(&ctx_a, METRIC, 10)
        .await
        .expect("第一次记账应成功");
    assert_eq!(used, 10, "第一次记账后的用量应当是 10");
    let used = db
        .add_quota_usage(&ctx_a, METRIC, 5)
        .await
        .expect("第二次记账应成功");
    assert_eq!(used, 15, "第二次记账后的用量应当是 15（10 + 5）");

    // 2) 超限判定：限额 20 → 15 未超；再加 10 → 25 超限。
    db.set_quota_limit(&ctx_a, METRIC, Some(20))
        .await
        .expect("设限额应成功");
    let usage = db.quota_usage(&ctx_a, METRIC).await.expect("查用量应成功");
    assert_eq!(usage.used, 15, "设限额不该把已记的用量清零");
    assert_eq!(usage.limit, Some(20));
    assert!(!usage.is_exceeded(), "15 没超过限额 20，不该判超限");
    assert_eq!(usage.remaining(), Some(5));

    let used = db
        .add_quota_usage(&ctx_a, METRIC, 10)
        .await
        .expect("第三次记账应成功");
    assert_eq!(used, 25, "第三次记账后的用量应当是 25");
    let usage = db.quota_usage(&ctx_a, METRIC).await.expect("查用量应成功");
    assert!(usage.is_exceeded(), "25 超过限额 20，应当判超限");
    assert_eq!(usage.remaining(), Some(-5));

    // 3) 重启后计数仍在：换一个**全新的连接池**（= 内存态全丢，等价于进程重启）。
    let restarted = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("重启后的连接串应可解析");
    let usage = restarted
        .quota_usage(&ctx_a, METRIC)
        .await
        .expect("重启后查用量应成功");
    assert_eq!(
        usage.used, 25,
        "计数必须落在 DB 里：换新连接池（= 重启）后用量不能丢"
    );
    assert_eq!(usage.limit, Some(20), "限额同样必须落在 DB 里");

    // 4) 隔离：另一个租户看不到 A 的计数（配额也是租户维度的）。
    let other = restarted
        .quota_usage(&ctx_b, METRIC)
        .await
        .expect("查他租户用量应成功");
    assert_eq!(
        other.used, 0,
        "配额计数必须按租户隔离：租户 B 不该看到租户 A 的用量"
    );

    // 顺带确认表名没写错（配额表必须存在，否则上面的 0 行是「表不存在」的假象）。
    assert_eq!(QUOTA_TABLE, "platform_tenant_quota");
}

/// 防假绿护栏：缺 `DATABASE_URL_TEST` 时**不得**报通过。
///
/// 守的是「真实库层」最容易犯的错：缺环境变量时夹具「安静地返回」→
/// 断言一条没跑 → 框架却报 `ok`。任何形如
/// `cargo test -- --include-ignored` 的 CI 都会把「真实库层」当成通过。
///
/// 做法：把**自己这个测试二进制**再拉起一个子进程，**去掉** `DATABASE_URL_TEST`，
/// 只跑真实库用例（`--skip` 掉本护栏，避免递归），然后断言：
///
/// 1. 子进程**退出码非 0**（真正承重的一条：结果必须非绿）；
/// 2. 子进程**一个用例都没报通过**（`0 passed`）。
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
}
