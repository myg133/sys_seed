//! REQ-014 D182 / D186：`audit.page.defaultSize` 的**三处**必须逐字相等。
//!
//! # 这个文件补的是哪个判据缺口
//!
//! 复验报告 M-01（决定性的实验）：把 `platform_audit::query::DEFAULT_LIMIT`
//! 从 `20` 改成 `30`（模拟「行为漂了」）—— `platform-audit` 自己的单测
//! `the_defaults_are_the_contract_defaults` **红了**，但
//! `platform-config/tests/config_real_db.rs` 的
//! `the_audit_page_default_size_is_the_same_everywhere` **仍然是绿的**。
//!
//! 原因：config 侧那条比对的是**硬编码常量** `const CONTRACT_DEFAULT_LIMIT: i64 = 20`，
//! 不是运行时常量 —— 它对**运行时漂移是盲的**。
//! 两条线各钉一半，**接不上**：
//!
//! | 断言 | 看得见运行时默认 | 看得见 manifest | 看得见 DB 行 |
//! |---|---|---|---|
//! | `platform-audit` 的 `the_defaults_are_the_contract_defaults` | ✅ | ❌ | ❌ |
//! | `platform-config` 的 `the_audit_page_default_size_is_the_same_everywhere` | ❌（写死 20） | ✅ | ✅ |
//! | **本文件** | ✅ | ✅ | ✅ |
//!
//! # 为什么断言放在 `platform-server`
//!
//! 它是唯一**同时**依赖 `platform-audit` 与 `platform-config` 的 crate
//! （`platform-config` **不得**依赖 `platform-audit` —— 分层是刻意的，D188 同款理由）。
//!
//! # 三处
//!
//! 1. `platform_audit::DEFAULT_LIMIT`（**运行时**默认，契约 C7）；
//! 2. `platform_config::ConfigRegistry::from_manifest_root(...)` 里
//!    `audit.page.defaultSize` 的**登记值**（manifest）；
//! 3. `platform_config` 表里那一行的**实际值**（迁移种子写进去的）。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-server --test audit_default_size_real_db \
//!   -- --test-threads=1 --include-ignored
//! ```
//!
//! 缺 `DATABASE_URL_TEST` → **失败**（不是「跳过」—— 跳过也算通过 = 假绿）。

use std::path::PathBuf;

use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;

/// 仓库根目录。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 测试库地址（管理员连接：本用例只**读**一行平台配置，不改库）。
///
/// 缺 `DATABASE_URL_TEST` → **失败**，不静默跳过。
fn required_test_database_url() -> String {
    let url = std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "缺少 DATABASE_URL_TEST：本用例要读 `platform_config` 的登记行，\
                 不能静默跳过（跳过 = 假绿）。\n  \
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
    assert_eq!(
        db, "base_test",
        "集成测试只允许跑在 base_test 库上（当前：{db}）—— base_dev 是开发库，测试绝不能碰"
    );
    url
}

/// `audit.page.defaultSize` 的**三处**必须逐字相等。
///
/// 这条是 D182「登记在骗人」的**完整**判据：manifest 说 20、DB 里存 20、
/// 实际接口默认也必须返回 20 —— **任何一处漂了都要红**。
#[tokio::test]
async fn the_audit_page_default_size_is_identical_in_runtime_manifest_and_db() {
    const KEY: &str = "audit.page.defaultSize";

    // 1) 运行时默认（契约 C7）。
    //
    // ⚠️ 这里必须引用**运行时常量**（而不是照抄一个 20）—— 这正是本文件存在的理由：
    // 照抄常量的话，`DEFAULT_LIMIT` 漂到 30 时本断言依然是绿的。
    let runtime = platform_audit::DEFAULT_LIMIT;

    // 2) manifest 登记值（经 `ConfigRegistry` 读，与运维在配置面板看到的是同一处）。
    let registry = platform_config::ConfigRegistry::from_manifest_root(&repo_root())
        .expect("配置注册表应能加载");
    let registered = registry.get(KEY).expect("该键必须已登记");
    let manifest = registered.default.clone();

    // 3) `platform_config` 表里的实际值（迁移种子 20261120000400 写进去的）。
    //
    // 「已进 `_sqlx_migrations` 的迁移不会重跑」→ 这个值**会漂**，必须单独比对。
    let url = required_test_database_url();
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));
    let stored: Option<Value> =
        sqlx::query_scalar("SELECT value FROM platform_config WHERE key = $1")
            .bind(KEY)
            .fetch_optional(&admin)
            .await
            .expect("应当能读 DB 配置行");
    let stored = stored.unwrap_or_else(|| {
        panic!("`platform_config` 里没有 {KEY} 这一行 —— 迁移种子没落库（20261120000400）")
    });

    // 三者逐字相等。断言消息写清楚「哪两处不一致」，否则失败时不知道该改哪边。
    let runtime_json = json!(runtime);
    assert_eq!(
        manifest, runtime_json,
        "manifest 登记的 {KEY}（{manifest}）与**运行时**默认 \
         `platform_audit::DEFAULT_LIMIT`（{runtime}）不一致 —— 运维看到的值会骗人（D182）"
    );
    assert_eq!(
        stored, runtime_json,
        "DB 里的 {KEY}（{stored}）与**运行时**默认 \
         `platform_audit::DEFAULT_LIMIT`（{runtime}）不一致 —— 迁移种子或配置行漂了（D182）"
    );
    assert_eq!(
        stored, manifest,
        "DB 里的 {KEY}（{stored}）与 manifest 登记值（{manifest}）不一致 —— 两处登记互相矛盾"
    );

    // 口径也要是**数**（不是字符串 `"20"`）—— 前端把它当数字用。
    assert!(
        runtime_json.is_number(),
        "运行时默认必须是 JSON 数（拿到 {runtime_json}）"
    );
}
