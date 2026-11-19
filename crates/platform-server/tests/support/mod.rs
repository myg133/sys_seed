//! REQ-013 在 `platform-server` 侧的测试夹具（真实库 + 注册表）。
//!
//! 与 `platform-iam/tests/sso_support` 的分工：那边测的是 IAM 内部的行为，
//! 这里测的是**装配**（真实 `ConfigService` 作为 SSO 配置源）。

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use sqlx::postgres::PgPoolOptions;

/// 仓库根目录。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn admin_url() -> String {
    std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "缺少 DATABASE_URL_TEST：SSO 的真实库用例必须真的连库跑，不能静默跳过。\n  \
                 本地开发：先起库（bun run db:up），再跑 scripts/test-integration.sh。"
            )
        })
}

/// 应用角色连接串（RLS 前提）。
fn app_url(admin: &str) -> String {
    let (scheme, rest) = admin.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

/// 准备真实库（跑迁移 + 应用角色脚本），返回**应用角色**连接串。
pub async fn setup_database() -> String {
    let url = admin_url();
    let db = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    assert_eq!(db, "base_test", "只允许跑在 base_test 上（当前：{db}）");

    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}\n  先执行 'bun run db:up'"));

    let root = repo_root();
    let mut migrator = sqlx::migrate::Migrator::new(Path::new(&root.join("migrations")))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&admin).await.expect("平台迁移应能应用");

    let role_sql = std::fs::read_to_string(root.join("deploy/seed/02-app-role.sql"))
        .expect("应用角色脚本应可读");
    sqlx::raw_sql(&role_sql)
        .execute(&admin)
        .await
        .expect("应用角色准备失败");

    app_url(&url)
}

/// 一个每次运行都不同的租户 ID。
pub fn unique_tenant(label: &str) -> String {
    format!("srv-{label}-{}", uuid::Uuid::new_v4().simple())
}

/// 一个唯一的 IP（限流桶是全局的）。
pub fn unique_ip() -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
}
