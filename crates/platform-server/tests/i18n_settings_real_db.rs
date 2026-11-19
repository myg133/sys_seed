//! `ConfigService` → `I18nSettings` 的**接缝**（REQ-021 / AC-S-093 步骤 5、裁定 #17、D119）。
//!
//! `/api/i18n/context` 的三级值（租户配置 / 平台配置 / 平台默认）不是代码常量，而是配置层的
//! 解析结果。这段适配代码是 i18n 与 config 之间**唯一**的接缝，也是最容易
//! 悄悄写错的地方（键名写错 → 落到 `KeyNotRegistered`；空串 → 渲染不出来）。
//!
//! 所以这里用**真实库**验：
//!
//! 1. 没有租户配置、也没有平台配置时，三个值等于 **manifest 的 `default`**，`source = default`；
//! 2. 租户写了值以后，读到的是租户值，`source = tenant`；
//! 3. `manifest_default` 始终是 manifest 的值（`*` 通配要靠它，裁定 #23）；
//! 4. **D119**：只有平台层写了值时，读到的是平台层的值，`source = platform`；
//!    租户再写则**租户赢**（`source = tenant`）—— 这正是 QA Pre-merge 报的 M1 现场。
//!
//! 未设置 `DATABASE_URL_TEST` 时**判为失败**，不是「跳过」。

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use platform_config::{ConfigRegistry, ConfigService};
use platform_core::RequestContext;
use platform_core::extension::config::ConfigSource;
use platform_db::{DbOptions, TenantDb};
use platform_server::i18n::{I18nSettingsSource, Setting};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const TENANT: &str = "tenant-i18n-settings";

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

fn app_role_url(admin_url: &str) -> String {
    let (scheme, rest) = admin_url.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

/// 同一测试二进制内的用例串行执行（它们共用同一个夹具租户）。
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

async fn setup() -> (sqlx::PgPool, ConfigService) {
    let url = required_test_database_url();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));

    let root = repo_root();
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
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

    // 清掉本用例的租户级配置行（幂等）。
    sqlx::query("DELETE FROM platform_tenant_config WHERE tenant_id = $1")
        .bind(TENANT)
        .execute(&admin)
        .await
        .expect("清理夹具应成功");
    sqlx::query("DELETE FROM platform_config_epoch WHERE scope_key = $1")
        .bind(format!("tenant:{TENANT}"))
        .execute(&admin)
        .await
        .expect("清理 epoch 应成功");

    // 清掉**平台级**的 i18n 配置行（幂等）—— D119 之后平台层的值会参与协商，
    // 留着别的用例 / QA 跑过的残留会让断言不成立（夹具必须自足）。
    clear_platform_i18n(&admin).await;

    let app_url = app_role_url(&url);
    let app_db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    // 直接解析连接串建池，避免平台层暴露裸连接（业务访问仍走 `TenantDb`）。
    let options = PgConnectOptions::from_str(&app_url).expect("应用角色连接串应可解析");
    let app_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("应用角色连接池应能建立");
    drop(app_pool);

    let registry = Arc::new(
        ConfigRegistry::from_manifest_root(&root).expect("配置注册表应能从 manifest 建立"),
    );
    let service = ConfigService::new(app_db, registry);
    // 返回**管理员池**：平台级行的直接 DML（清理夹具）只有管理员能做 ——
    // 应用角色对 `platform_config` 只有 SELECT（见 `deploy/seed/02-app-role.sql`）。
    (admin, service)
}

fn ctx() -> RequestContext {
    RequestContext::new().with_tenant(TENANT)
}

/// 删掉**平台级**的 i18n 配置行，并推进平台级 epoch。
///
/// 写通道（`platform_config_upsert`）本来就在同一事务里自增 epoch ——
/// 这里的直接删除必须自己补上，否则别的进程的缓存不失效、还留着旧值。
async fn clear_platform_i18n(pool: &sqlx::PgPool) {
    sqlx::query("DELETE FROM platform_config WHERE key = ANY($1)")
        .bind(vec![
            "locale".to_string(),
            "timezone".to_string(),
            "currency".to_string(),
        ])
        .execute(pool)
        .await
        .expect("清理平台级夹具应成功");
    sqlx::query("UPDATE platform_config_epoch SET epoch = epoch + 1 WHERE scope_key = 'platform'")
        .execute(pool)
        .await
        .expect("推进平台级 epoch 应成功");
}

fn value_of(settings: &platform_server::i18n::I18nSettings, key: &str) -> Setting {
    match key {
        "locale" => settings.locale.clone(),
        "timezone" => settings.timezone.clone(),
        "currency" => settings.currency.clone(),
        other => panic!("未知键 {other}"),
    }
}

/// 没有租户配置 → 三个值等于 manifest 的 `default`（裁定 #17）。
#[tokio::test]
async fn defaults_come_from_the_manifest_not_from_code_constants() {
    let _guard = serial().await;
    let (_pool, service) = setup().await;
    let settings = I18nSettingsSource::settings(&service, &ctx())
        .await
        .expect("读 i18n 设置应成功");

    for (key, expected) in [
        ("locale", "zh-CN"),
        ("timezone", "Asia/Shanghai"),
        ("currency", "CNY"),
    ] {
        let setting = value_of(&settings, key);
        assert_eq!(setting.value, expected, "{key} 的默认值来自 manifest");
        assert_eq!(
            setting.manifest_default, expected,
            "{key} 的 manifest_default 必须带上来（`*` 通配要靠它）"
        );
        assert_eq!(
            setting.source,
            platform_core::extension::config::ConfigSource::Default,
            "{key} 没被配置过，来源必须是 default"
        );
        assert!(!setting.is_explicit());
    }
}

/// 租户写了值 → 读到租户值，来源 `tenant`；`manifest_default` 仍是 manifest 的值。
#[tokio::test]
async fn tenant_values_override_and_keep_the_manifest_default_visible() {
    let _guard = serial().await;
    let (_pool, service) = setup().await;

    let entries = vec![
        platform_config::WriteEntry {
            key: "timezone".to_string(),
            value: serde_json::json!("America/New_York"),
        },
        platform_config::WriteEntry {
            key: "currency".to_string(),
            value: serde_json::json!("USD"),
        },
    ];
    service
        .write_entries(
            &ctx(),
            &platform_config::WriteRequest {
                scope: "tenant".to_string(),
                entries,
            },
            platform_config::WriteRoute::Scoped,
        )
        .await
        .expect("写时区/币种应成功");

    let settings = I18nSettingsSource::settings(&service, &ctx())
        .await
        .expect("读 i18n 设置应成功");

    let timezone = value_of(&settings, "timezone");
    assert_eq!(timezone.value, "America/New_York");
    assert_eq!(
        timezone.source,
        platform_core::extension::config::ConfigSource::Tenant
    );
    assert!(timezone.is_explicit());
    assert_eq!(
        timezone.manifest_default, "Asia/Shanghai",
        "租户覆盖后，平台默认值仍然可读（协商的第 5 级）"
    );

    let currency = value_of(&settings, "currency");
    assert_eq!(currency.value, "USD");
    assert!(currency.is_explicit());

    // 没被覆盖的语言仍是默认值。
    let locale = value_of(&settings, "locale");
    assert_eq!(locale.value, "zh-CN");
    assert!(!locale.is_explicit());
}

/// **D119 / TC-021-096 / TC-021-097**（真实库）：平台层的值参与协商，
/// 租户层压过平台层。
///
/// 这正是 QA Pre-merge 报的 **M1** 现场：平台级写了 `locale=en-US`、租户没配时，
/// `/api/config/entries?keys=locale` 读得到 `source=platform`，而
/// `/api/i18n/context` 却报 `source=default` —— **同一个键、两处答案不一致**。
#[tokio::test]
async fn the_platform_layer_takes_part_in_the_negotiation_and_the_tenant_wins_over_it() {
    let _guard = serial().await;
    let (pool, service) = setup().await;

    // 平台层写三个键（D53：登记为 `scope: tenant` 的键，**平台层可以写**）。
    service
        .write_entries(
            &ctx(),
            &platform_config::WriteRequest {
                scope: "platform".to_string(),
                entries: vec![
                    platform_config::WriteEntry {
                        key: "locale".to_string(),
                        value: serde_json::json!("en-US"),
                    },
                    platform_config::WriteEntry {
                        key: "timezone".to_string(),
                        value: serde_json::json!("America/New_York"),
                    },
                    platform_config::WriteEntry {
                        key: "currency".to_string(),
                        value: serde_json::json!("USD"),
                    },
                ],
            },
            platform_config::WriteRoute::Platform,
        )
        .await
        .expect("平台层写 i18n 三个键应成功（D53：平台层可以写 tenant 作用域的键）");

    // **TC-021-096**：租户没配 → 读到**平台层**的值，来源 `platform`。
    let settings = I18nSettingsSource::settings(&service, &ctx())
        .await
        .expect("读 i18n 设置应成功");
    for (key, expected) in [
        ("locale", "en-US"),
        ("timezone", "America/New_York"),
        ("currency", "USD"),
    ] {
        let setting = value_of(&settings, key);
        assert_eq!(setting.value, expected, "{key} 应当取平台层的值");
        assert_eq!(
            setting.source,
            ConfigSource::Platform,
            "{key} 的来源必须是 platform（D119）—— 报成 default 就是 M1 没修"
        );
        assert!(setting.is_explicit(), "{key} 来自平台层 = 显式配置");
    }

    // **TC-021-097**：租户再写 → **租户赢**，来源 `tenant`。
    service
        .write_entries(
            &ctx(),
            &platform_config::WriteRequest {
                scope: "tenant".to_string(),
                entries: vec![
                    platform_config::WriteEntry {
                        key: "locale".to_string(),
                        value: serde_json::json!("zh-CN"),
                    },
                    platform_config::WriteEntry {
                        key: "timezone".to_string(),
                        value: serde_json::json!("Europe/London"),
                    },
                    platform_config::WriteEntry {
                        key: "currency".to_string(),
                        value: serde_json::json!("JPY"),
                    },
                ],
            },
            platform_config::WriteRoute::Scoped,
        )
        .await
        .expect("租户层写 i18n 三个键应成功");

    let settings = I18nSettingsSource::settings(&service, &ctx())
        .await
        .expect("读 i18n 设置应成功");
    for (key, expected) in [
        ("locale", "zh-CN"),
        ("timezone", "Europe/London"),
        ("currency", "JPY"),
    ] {
        let setting = value_of(&settings, key);
        assert_eq!(setting.value, expected, "{key} 应当取租户层的值");
        assert_eq!(
            setting.source,
            ConfigSource::Tenant,
            "{key} 的来源必须是 tenant —— 租户层压过平台层"
        );
    }

    // 收尾：把平台层的 i18n 行删掉 —— 平台层的值**对所有租户可见**，
    // 留着会污染别的用例（以及下一轮 QA 的现场）。
    clear_platform_i18n(&pool).await;
}
