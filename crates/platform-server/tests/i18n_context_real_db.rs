//! `/api/i18n/context` 的**端到端**护栏（REQ-021 修复轮 3 / 独立复核 F-1）。
//!
//! # 这条用例守的是什么
//!
//! D119 修好了「平台配置层被静默丢弃」，但**没有任何一条自动化测试守住这个行为**：
//!
//! - `crates/platform-server/tests/i18n_settings_real_db.rs`（真库）只断言
//!   `setting.is_explicit()` / `setting.value` —— 它**只测「配置层」，从不调用协商函数**；
//! - `crates/platform-server/tests/i18n_api.rs`（HTTP 层）用 `FakeSettings`（内存伪造）——
//!   伪造的 `Setting.source` 是**手写的**，不是从 DB 解析出来的。
//!
//! ⇒ 于是「DB 里的平台行 → `/api/i18n/context` 返回 `source=platform`」这条路径
//! **没有任何护栏**。独立复核的变异验证 M-B（把平台层误报成 `tenant`）证明了这一点：
//! 真库用例全绿。
//!
//! # 本文件走的是**完整链路**
//!
//! ```text
//! 真实 DB（base_test）
//!   → 真实配置解析器（platform_config::ConfigService + DbConfigResolver）
//!   → 真实协商函数（platform_server::i18n::{negotiate_locale, negotiate_timezone,
//!                                      resolve_currency}）
//!   → 真实 HTTP 响应（platform_server::i18n::router + axum）
//! ```
//!
//! **唯一的替身是令牌校验**（`StubTokens`）—— 它只回答「你是谁、属于哪个租户」，
//! 与本条用例要守的「配置层 → 协商 → 响应体」无关。配置读取、协商、HTTP 路由
//! **全部是生产代码**，没有任何手写的 `Setting`。
//!
//! # 夹具必须自足
//!
//! 每条用例**开始前**与**结束后**都清掉：
//!
//! - 平台层的三个 i18n 键（`platform_config` 里的 `locale` / `timezone` / `currency`）——
//!   它们对**所有租户**可见，留着会污染别的用例；
//! - 本夹具租户自己的对应行（`platform_tenant_config`）。
//!
//! ⚠️ 清理 `platform_config` **必须用管理员连接**：应用角色 `base_app` 对它只有
//! `SELECT`（直接 DML → 42501）。管理员连接同时绕过 RLS —— 造数是夹具，不是业务访问。
//!
//! 未设置 `DATABASE_URL_TEST` 时**判为失败**，不是「跳过」（跳过也算通过 = 假绿）。

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use platform_config::error::ConfigError;
use platform_config::http::{TokenVerifier, VerifiedToken};
use platform_config::{ConfigRegistry, ConfigService, WriteEntry, WriteRequest, WriteRoute};
use platform_core::RequestContext;
use platform_db::{DbOptions, TenantDb};
use platform_server::i18n::{CONTEXT_PATH, I18nState, router};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

/// 本夹具独占的租户（不复用别的用例的租户 → 互不干扰）。
const TENANT: &str = "tenant-i18n-context-e2e";
/// 夹具用户。
const USER: &str = "u-i18n-context-e2e";
/// 夹具令牌。
const TOKEN: &str = "i18n-context-e2e-token";
/// 三个 i18n 配置键。
const I18N_KEYS: [&str; 3] = ["locale", "timezone", "currency"];

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

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

/// 把管理员连接串换成**应用角色**的连接串。
fn app_role_url(admin_url: &str) -> String {
    let (scheme, rest) = admin_url.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

/// 同一测试二进制内的用例串行执行（它们共用平台层的 i18n 行）。
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

fn ctx() -> RequestContext {
    RequestContext::new().with_tenant(TENANT)
}

/// 令牌校验替身：**只回答身份**，不参与配置解析。
struct StubTokens;

impl TokenVerifier for StubTokens {
    fn verify(&self, token: &str) -> Result<VerifiedToken, ConfigError> {
        if token != TOKEN {
            return Err(ConfigError::Unauthenticated);
        }
        Ok(VerifiedToken {
            user_id: USER.to_string(),
            tenant_id: TENANT.to_string(),
        })
    }
}

/// 真实库夹具。
struct Fixture {
    /// 管理员连接池（造数 / 清理夹具 —— 应用角色对 `platform_config` 只有 SELECT）。
    admin: sqlx::PgPool,
    /// **真实**配置服务（真实 DB + 真实注册表 + 真实 `DbConfigResolver`）。
    service: ConfigService,
    /// HTTP 层共享状态：`settings` 是**真实配置服务的适配器**，不是 `FakeSettings`。
    state: Arc<I18nState>,
    /// 收尾清理守卫（Drop 时清掉平台层与租户层的 i18n 行）。
    ///
    /// 这个字段**只为了它的 Drop 副作用**存在 —— 没有别的用途，所以显式
    /// `allow(dead_code)`（否则编译器报「never read」）。
    #[allow(dead_code)]
    cleanup: Cleanup,
}

impl Fixture {
    /// 登记表的 manifest 默认值（平台默认值的唯一来源，裁定 #17）。
    fn manifest_default(&self, key: &str) -> String {
        self.service
            .registry()
            .get(key)
            .and_then(|spec| spec.default.as_str())
            .unwrap_or_default()
            .to_string()
    }
}

/// 准备真实库环境，并把平台层 / 租户层的 i18n 行恢复到「什么都没配」。
async fn setup() -> Fixture {
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

    // 夹具自足：先清干净平台层与租户层的 i18n 行。
    clear_i18n(&admin).await;

    let app_url = app_role_url(&url);
    let app_db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let fx_db = TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let registry = Arc::new(
        ConfigRegistry::from_manifest_root(&root).expect("配置注册表应能从 manifest 建立"),
    );
    let service = ConfigService::new(app_db, registry.clone());

    let state = Arc::new(I18nState {
        // ★ 真实配置服务 —— `platform_server::i18n` 里为 `ConfigService` 实现的
        //   `I18nSettingsSource` 适配器：它调用**真实**的 `read_entries`
        //   （→ 真实 `DbConfigResolver` → 真实 `platform_config` /
        //   `platform_tenant_config` 解析）。**不是** `FakeSettings`。
        settings: Arc::new(service.clone()),
        // 汇率接缝同样用**真实**实现（本用例不碰它，但不留假实现）。
        rates: Arc::new(platform_db::fx::FxRateStore::new(fx_db)),
        tokens: Arc::new(StubTokens),
        // 「已注册语言」的唯一来源（裁定 #16）：翻译包目录的文件名。
        locales: Arc::new(registry.locales().clone()),
    });

    Fixture {
        admin: admin.clone(),
        service,
        state,
        cleanup: Cleanup {
            admin_url: url,
        },
    }
}

/// 收尾清理守卫：夹具被丢弃时清掉平台层与租户层的 i18n 行。
///
/// # 为什么用 Drop 而不是「用例结尾显式清理」
///
/// 断言失败会 panic → 显式清理被跳过 → **平台层的残留会污染别的用例**
/// （复核者就抓到过真库夹具在租户级留了 3 行残留）。
/// Drop 守卫在**展开（unwind）期间**也会跑 → 失败路径同样干净。
///
/// # 为什么在独立线程 + 独立 runtime 里清
///
/// - 不能在当前 runtime 里 `block_on`（嵌套 runtime → panic）；
/// - **不能复用**当前 runtime 建出来的连接池：sqlx 的连接绑定了创建它的
///   runtime，跨 runtime 使用会卡到 `PoolTimedOut`（实测踩过）。
///
/// 所以这里**另建一个连接池**（用管理员连接串），在独立线程里同步清完再 `join()`。
struct Cleanup {
    admin_url: String,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let url = self.admin_url.clone();
        let _ = std::thread::Builder::new()
            .name("i18n-context-fixture-cleanup".to_string())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    let Ok(pool) = PgPoolOptions::new()
                        .max_connections(1)
                        .acquire_timeout(std::time::Duration::from_secs(10))
                        .connect(&url)
                        .await
                    else {
                        return;
                    };
                    clear_i18n(&pool).await;
                    pool.close().await;
                });
            })
            .map(|handle| handle.join());
    }
}

/// 删掉**平台层**与**本夹具租户**的 i18n 行，并推进两边的 epoch。
///
/// ⚠️ 平台层的行对**所有租户**可见 → 必须清理，否则会污染别的用例
/// （复核者就抓到过真库夹具在租户级留了 3 行残留）。
///
/// ⚠️ 直接删除绕过了写通道的 epoch 自增 → 这里**必须自己补上**，
/// 否则别处（以及本进程内）的缓存不失效、还留着旧值。
async fn clear_i18n(pool: &sqlx::PgPool) {
    sqlx::query("DELETE FROM platform_config WHERE key = ANY($1)")
        .bind(I18N_KEYS.iter().map(|key| key.to_string()).collect::<Vec<_>>())
        .execute(pool)
        .await
        .expect("清理平台级 i18n 夹具应成功");
    sqlx::query("DELETE FROM platform_tenant_config WHERE tenant_id = $1 AND key = ANY($2)")
        .bind(TENANT)
        .bind(I18N_KEYS.iter().map(|key| key.to_string()).collect::<Vec<_>>())
        .execute(pool)
        .await
        .expect("清理租户级 i18n 夹具应成功");
    bump_epoch(pool, "platform").await;
    bump_epoch(pool, &format!("tenant:{TENANT}")).await;
}

/// 推进某个作用域的缓存 epoch（直接 DML 绕过写通道时必须补上）。
async fn bump_epoch(pool: &sqlx::PgPool, scope_key: &str) {
    sqlx::query(
        "INSERT INTO platform_config_epoch (scope_key, epoch, updated_at) \
         VALUES ($1, 1, now()) \
         ON CONFLICT (scope_key) DO UPDATE \
         SET epoch = platform_config_epoch.epoch + 1, updated_at = now()",
    )
    .bind(scope_key)
    .execute(pool)
    .await
    .expect("推进 epoch 应成功");
}

/// 写**平台层**的三个键（D53：登记为 `scope: tenant` 的键，平台层可以写）。
async fn write_platform(service: &ConfigService, entries: [(&str, &str); 3]) {
    service
        .write_entries(
            &ctx(),
            &WriteRequest {
                scope: "platform".to_string(),
                entries: entries
                    .iter()
                    .map(|(key, value)| WriteEntry {
                        key: (*key).to_string(),
                        value: serde_json::json!(value),
                    })
                    .collect(),
            },
            WriteRoute::Platform,
        )
        .await
        .expect("平台层写 i18n 三个键应成功（D53：平台层可以写 tenant 作用域的键）");
}

/// 写**租户层**的三个键。
async fn write_tenant(service: &ConfigService, entries: [(&str, &str); 3]) {
    service
        .write_entries(
            &ctx(),
            &WriteRequest {
                scope: "tenant".to_string(),
                entries: entries
                    .iter()
                    .map(|(key, value)| WriteEntry {
                        key: (*key).to_string(),
                        value: serde_json::json!(value),
                    })
                    .collect(),
            },
            WriteRoute::Scoped,
        )
        .await
        .expect("租户层写 i18n 三个键应成功");
}

// ---------------------------------------------------------------------------
// HTTP 调用（真实 axum 路由）
// ---------------------------------------------------------------------------

/// 打一次 `/api/i18n/context`（真实路由 + 真实处理器）。
async fn call(state: Arc<I18nState>, query: &str, headers: &[(&str, &str)]) -> (StatusCode, Value) {
    let app = router(state);
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("{CONTEXT_PATH}{query}"))
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header("x-tenant-id", TENANT);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder.body(Body::empty()).expect("请求应能构造");
    let response = app.oneshot(request).await.expect("路由应当能处理请求");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("应当能读取响应体")
        .to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            panic!(
                "响应体不是 JSON（{err}）：{}",
                String::from_utf8_lossy(&bytes)
            )
        })
    };
    (status, body)
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

/// **F-1 的核心护栏**：DB 里的**平台层**行 → 真实解析器 → 真实协商 → 真实 HTTP 响应，
/// `source` 必须是 `platform`。
///
/// 变异验证：
///
/// - M-A（`ConfigSource::Platform => None`，= 修复前行为）→ 本用例红；
/// - M-B（`ConfigSource::Platform => Some(LocaleSource::Tenant)`，= 平台层被误报成
///   租户）→ **本用例红**（`source` 变成 `tenant`）。
///
/// 这正是独立复核指出的缺口：修复前**真库用例全绿**，因为它们只断言 `Setting`，
/// 从不调用协商函数、更不看 HTTP 响应体。
#[tokio::test]
async fn the_platform_layer_reaches_the_http_context_as_source_platform() {
    let _guard = serial().await;
    let fx = setup().await;

    // 只有**平台层**配了；租户层**没配**。
    write_platform(
        &fx.service,
        [
            ("locale", "en-US"),
            ("timezone", "America/New_York"),
            ("currency", "USD"),
        ],
    )
    .await;

    let (status, body) = call(fx.state.clone(), "", &[]).await;
    // 先清夹具，再断言：断言失败也不会留下残留（平台层的行对所有租户可见）。
    clear_i18n(&fx.admin).await;
    assert_eq!(status, StatusCode::OK, "body = {body}");
    println!("platform-only → {body}");

    // 语言：平台层的 `en-US` 生效，来源如实报 `platform`。
    assert_eq!(body["locale"], "en-US", "平台级的语言必须生效");
    assert_eq!(
        body["source"], "platform",
        "来源必须如实报 platform（报成 tenant / default 都是 D119 回归）"
    );
    assert_eq!(body["sources"]["locale"], "platform");
    assert_ne!(body["locale"], "zh-CN", "不能退回 manifest 默认值");

    // 时区：平台层的 `America/New_York` 生效。
    assert_eq!(body["timezone"], "America/New_York", "平台级的时区必须生效");
    assert_eq!(body["sources"]["timezone"], "platform");
    assert_ne!(body["timezone"], "Asia/Shanghai", "不能退回 manifest 默认值");

    // 币种：平台层的 `USD` 生效。
    assert_eq!(body["currency"], "USD", "平台级的币种必须生效");
    assert_eq!(body["sources"]["currency"], "platform");
    assert_ne!(body["currency"], "CNY", "不能退回 manifest 默认值");
}

/// **反向断言**：租户层覆盖平台层 → `source == "tenant"`。
#[tokio::test]
async fn the_tenant_layer_wins_over_the_platform_layer_end_to_end() {
    let _guard = serial().await;
    let fx = setup().await;

    write_platform(
        &fx.service,
        [
            ("locale", "zh-CN"),
            ("timezone", "America/New_York"),
            ("currency", "USD"),
        ],
    )
    .await;
    write_tenant(
        &fx.service,
        [
            ("locale", "en-US"),
            ("timezone", "Europe/London"),
            ("currency", "JPY"),
        ],
    )
    .await;

    let (status, body) = call(fx.state.clone(), "", &[]).await;
    // 先清夹具，再断言：断言失败也不会留下残留（平台层的行对所有租户可见）。
    clear_i18n(&fx.admin).await;
    assert_eq!(status, StatusCode::OK, "body = {body}");
    println!("tenant-over-platform → {body}");

    assert_eq!(body["locale"], "en-US", "租户层的语言必须压过平台层");
    assert_eq!(body["source"], "tenant", "如实报出是租户层生效");
    assert_eq!(body["sources"]["locale"], "tenant");
    assert_ne!(body["locale"], "zh-CN", "不能取平台层的值");

    assert_eq!(body["timezone"], "Europe/London", "租户层的时区必须压过平台层");
    assert_eq!(body["sources"]["timezone"], "tenant");
    assert_ne!(body["timezone"], "America/New_York", "不能取平台层的值");

    assert_eq!(body["currency"], "JPY", "租户层的币种必须压过平台层");
    assert_eq!(body["sources"]["currency"], "tenant");
    assert_ne!(body["currency"], "USD", "不能取平台层的值");
}

/// **反向断言**：什么都没配 → `source == "default"`（manifest 的平台默认值）。
#[tokio::test]
async fn nothing_configured_falls_back_to_the_manifest_default_end_to_end() {
    let _guard = serial().await;
    let fx = setup().await;

    let (status, body) = call(fx.state.clone(), "", &[]).await;
    // 先清夹具，再断言：断言失败也不会留下残留（平台层的行对所有租户可见）。
    clear_i18n(&fx.admin).await;
    assert_eq!(status, StatusCode::OK, "body = {body}");
    println!("nothing-configured → {body}");

    assert_eq!(body["source"], "default", "谁都没配时来源必须是 default");
    assert_eq!(body["sources"]["locale"], "default");
    assert_eq!(body["sources"]["timezone"], "default");
    assert_eq!(body["sources"]["currency"], "default");

    // 平台默认值来自 manifest（裁定 #17），不是代码常量。
    for (field, key) in [
        ("locale", "locale"),
        ("timezone", "timezone"),
        ("currency", "currency"),
    ] {
        assert_eq!(
            body[field],
            fx.manifest_default(key),
            "{field} 的平台默认值必须来自 manifest"
        );
    }
}

/// **反向断言**：`Accept-Language` 请求头仍然压过平台层（平台层没有插队到请求头之前）。
#[tokio::test]
async fn the_accept_language_header_still_wins_over_the_platform_layer_end_to_end() {
    let _guard = serial().await;
    let fx = setup().await;

    write_platform(
        &fx.service,
        [
            ("locale", "en-US"),
            ("timezone", "America/New_York"),
            ("currency", "USD"),
        ],
    )
    .await;

    let (status, body) = call(fx.state.clone(), "", &[("accept-language", "zh-CN")]).await;
    // 先清夹具，再断言：断言失败也不会留下残留（平台层的行对所有租户可见）。
    clear_i18n(&fx.admin).await;
    assert_eq!(status, StatusCode::OK, "body = {body}");
    println!("accept-language-over-platform → {body}");

    assert_eq!(body["locale"], "zh-CN", "请求头仍压过平台层");
    assert_eq!(body["source"], "accept-language");
    assert_eq!(body["sources"]["locale"], "accept-language");

    // 时区不看请求头 → 仍是平台层。
    assert_eq!(body["sources"]["timezone"], "platform");
    assert_eq!(body["sources"]["currency"], "platform");
}

/// **反向断言**：用户显式偏好（`?locale=` / `?timezone=`）仍然压过平台层。
#[tokio::test]
async fn the_user_preference_still_wins_over_the_platform_layer_end_to_end() {
    let _guard = serial().await;
    let fx = setup().await;

    write_platform(
        &fx.service,
        [
            ("locale", "en-US"),
            ("timezone", "America/New_York"),
            ("currency", "USD"),
        ],
    )
    .await;

    let (status, body) = call(
        fx.state.clone(),
        "?locale=zh-CN&timezone=Europe/London",
        &[],
    )
    .await;
    // 先清夹具，再断言：断言失败也不会留下残留（平台层的行对所有租户可见）。
    clear_i18n(&fx.admin).await;
    assert_eq!(status, StatusCode::OK, "body = {body}");
    println!("user-preference-over-platform → {body}");

    assert_eq!(body["locale"], "zh-CN", "用户偏好必须压过平台层");
    assert_eq!(body["source"], "user");
    assert_eq!(body["sources"]["locale"], "user");
    assert_eq!(body["timezone"], "Europe/London");
    assert_eq!(body["sources"]["timezone"], "user");

    // 币种本轮没有用户级覆盖 → 仍是平台层。
    assert_eq!(body["sources"]["currency"], "platform");
}
