//! 汇率表的迁移**可逆性**（REQ-021 / `20261119000200_i18n_fx_rate`）。
//!
//! 验 `up → down → up` 三步：表建起来、删干净、再建回来，且语义一致。
//!
//! # 为什么在**独立的数据库**里跑
//!
//! 在 `base_test.public` 上 `DROP TABLE platform_fx_rate` 会把其它用例的夹具一起
//! 抹掉（同一个测试二进制里并行跑）。所以这里新建一个临时库 `fx_mig_*`，
//! 只把这两个 SQL 文件跑一遍，最后删库 —— 与 `platform-config` 的迁移用例同一手法。
//!
//! 未设置 `DATABASE_URL_TEST` 时**判为失败**，不是「跳过」。

use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const UP: &str = "migrations/20261119000200_i18n_fx_rate.up.sql";
const DOWN: &str = "migrations/20261119000200_i18n_fx_rate.down.sql";

fn required_test_database_url() -> String {
    let url = std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "缺少 DATABASE_URL_TEST：真实库用例必须真的连库跑，不能静默跳过。\n  \
                 本地开发：先起库（bun run db:up），再跑 scripts/test-integration.sh。"
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
    assert_eq!(db, "base_test", "集成测试只允许跑在 base_test 库上");
    url
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 表是否存在（用 `to_regclass`，不存在时返回 NULL 而不是报错）。
async fn table_exists(pool: &sqlx::PgPool) -> bool {
    let name: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.platform_fx_rate')::text")
            .fetch_one(pool)
            .await
            .expect("查询表存在性应成功");
    name.is_some()
}

#[tokio::test]
async fn the_fx_rate_migration_is_reversible() {
    let admin_url = required_test_database_url();
    let root = repo_root();
    let up = std::fs::read_to_string(root.join(UP)).expect("up 迁移应可读");
    let down = std::fs::read_to_string(root.join(DOWN)).expect("down 迁移应可读");

    // down 必须是「只删本迁移建的表」—— 不能顺手删别的对象。
    assert!(
        down.contains("DROP TABLE IF EXISTS platform_fx_rate"),
        "down 必须删掉本迁移建的表（且幂等）"
    );
    assert!(
        !down.contains("platform_config") && !down.contains("platform_permission"),
        "down 不得触碰别人的表"
    );

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("时钟应正常")
        .as_nanos();
    let name = format!("fx_mig_{}_{}", std::process::id(), suffix);

    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&admin_url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{admin_url}）：{err}"));
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("应能建临时库");

    let options = PgConnectOptions::from_str(&admin_url)
        .expect("管理员连接串应可解析")
        .database(&name);
    let probe = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("应能连临时库");

    // 1) up：建表 + 索引。
    sqlx::raw_sql(&up).execute(&probe).await.expect("up 应成功");
    assert!(table_exists(&probe).await, "up 后表必须存在");

    // 唯一约束在 `(from, to, as_of)` 上（幂等与快照标识的前提）。
    sqlx::raw_sql(
        "INSERT INTO platform_fx_rate (from_currency, to_currency, rate_micros, as_of, source) \
         VALUES ('CNY', 'USD', 139500, '2026-01-01T00:00:00Z', 'manual')",
    )
    .execute(&probe)
    .await
    .expect("合法行应能插入");
    let duplicate = sqlx::raw_sql(
        "INSERT INTO platform_fx_rate (from_currency, to_currency, rate_micros, as_of, source) \
         VALUES ('CNY', 'USD', 140000, '2026-01-01T00:00:00Z', 'manual')",
    )
    .execute(&probe)
    .await;
    assert!(
        duplicate.is_err(),
        "同一 (from,to,as_of) 必须被唯一约束拒绝（否则快照标识不唯一）"
    );

    // 汇率必须为正（CHECK 约束）。
    let zero_rate = sqlx::raw_sql(
        "INSERT INTO platform_fx_rate (from_currency, to_currency, rate_micros, as_of) \
         VALUES ('CNY', 'JPY', 0, '2026-01-01T00:00:00Z')",
    )
    .execute(&probe)
    .await;
    assert!(zero_rate.is_err(), "0 汇率必须被 CHECK 拒绝");

    // 2) down：表必须消失。
    sqlx::raw_sql(&down)
        .execute(&probe)
        .await
        .expect("down 应成功");
    assert!(!table_exists(&probe).await, "down 后表必须消失");

    // down 幂等：再跑一次也不报错。
    sqlx::raw_sql(&down)
        .execute(&probe)
        .await
        .expect("down 应可重复执行");

    // 3) 再次 up：表回来，且是空的（down 已把数据删掉）。
    sqlx::raw_sql(&up)
        .execute(&probe)
        .await
        .expect("再次 up 应成功");
    assert!(table_exists(&probe).await, "再次 up 后表必须存在");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_fx_rate")
        .fetch_one(&probe)
        .await
        .expect("计数应成功");
    assert_eq!(rows, 0, "down 后数据不应残留");

    probe.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("应能删临时库");
    admin.close().await;
}
