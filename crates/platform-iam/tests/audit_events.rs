//! 登录审计埋点的**真实库层**（REQ-014 AC-S-127 / D157 / D163）。
//!
//! # 覆盖的断言
//!
//! | # | 断言 | 判据 |
//! |---|------|------|
//! | 1 | 登录**成功**产生 **1 条** `iam.login.succeeded` | 事件码 + `object_type = iam_session` |
//! | 2 | 登录**失败**产生 **1 条** `iam.login.failed` | 三条失败路径各 1 条 |
//! | 3 | 失败事件**不含账号名** | `actor_id` / `object_id` / `before` / `after` 全空 |
//! | 4 | 「账号不存在」与「密码错误」**条数相同** | 不可枚举（AC-S-127 步骤 5） |
//! | 5 | 审计写失败 → **拒绝登录**（503 `AUDIT_SINK_UNAVAILABLE`） | D157 |
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-iam --test audit_events -- --test-threads=1 --include-ignored
//! ```
//!
//! 缺 `DATABASE_URL_TEST` → **失败**（不是「跳过」—— 跳过也算通过 = 假绿）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use platform_core::RequestContext;
use platform_core::extension::audit::{AuditError, AuditEvent, AuditSink};
use platform_db::{DbOptions, TenantDb};
use platform_iam::config::{
    Argon2Settings, IamConfig, LoginSettings, PasswordPolicySettings, TokenSettings, TotpSettings,
};
use platform_iam::error::code;
use platform_iam::service::{IamService, LoginRequest, RegisterRequest, RequestMeta};
use platform_iam::store::IamStore;
use sqlx::postgres::PgPoolOptions;

/// 测试用的密码（满足策略：长度 ≥ 12、不在常见口令词库里）。
const PASSWORD: &str = "correct-horse-battery-staple";

/// 测试库地址：只允许 `base_test`。
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
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
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

/// 准备真实库环境（迁移 + 应用角色）。
async fn setup() -> String {
    let url = required_test_database_url();
    let admin_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));

    let root = repo_root();
    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&admin_pool).await.expect("平台迁移应能应用");

    let role_sql = read_file(&root.join("deploy/seed/02-app-role.sql"));
    sqlx::raw_sql(&role_sql)
        .execute(&admin_pool)
        .await
        .expect("应用角色准备失败");
    app_role_url(&url)
}

/// 测试用的 IAM 配置（Argon2 用**低配值** —— 详见 `iam_real_db.rs` 的同款说明）。
fn test_config() -> IamConfig {
    let pem = platform_iam::jwt::generate_key_pair().expect("测试密钥应当能生成");
    IamConfig {
        jwt: platform_iam::config::JwtSettings {
            issuer: "base".to_string(),
            audience: "base-api".to_string(),
            private_key_pem: pem,
            kid: "kid-test".to_string(),
            previous_private_key_pem: None,
            previous_kid: None,
        },
        tokens: TokenSettings {
            access_ttl: Duration::from_secs(900),
            refresh_ttl: Duration::from_secs(30 * 24 * 3600),
            reset_ttl: Duration::from_secs(3600),
        },
        argon2: Argon2Settings {
            memory_kib: 8192,
            iterations: 2,
            parallelism: 1,
        },
        password_policy: PasswordPolicySettings {
            min_length: 12,
            max_age_days: Some(180),
            history: 5,
        },
        login: LoginSettings {
            ip_limit: 20,
            ip_window: Duration::from_secs(300),
            account_failure_limit: 5,
            lockout_duration: Duration::from_secs(900),
        },
        totp: TotpSettings {
            interval: 30,
            digits: 6,
            tolerance_steps: 1,
            recovery_code_count: 10,
        },
        secret_key: [3u8; 32],
    }
}

/// 记录事件的**假 sink**（不落库 —— 本用例只断言**埋点契约**）。
#[derive(Debug, Default)]
struct RecordingSink(Mutex<Vec<AuditEvent>>);

impl RecordingSink {
    fn events(&self) -> Vec<AuditEvent> {
        self.0.lock().unwrap_or_else(|err| err.into_inner()).clone()
    }

    fn of(&self, action: &str) -> Vec<AuditEvent> {
        self.events()
            .into_iter()
            .filter(|event| event.action == action)
            .collect()
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

/// 总是失败的 sink（用来验 D157）。
struct FailingSink;

impl AuditSink for FailingSink {
    fn record<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _event: &'a AuditEvent,
    ) -> platform_core::extension::audit::BoxFuture<'a, Result<(), AuditError>> {
        Box::pin(async {
            Err(AuditError::SinkUnavailable {
                reason: "boom".to_string(),
            })
        })
    }
}

/// 一个测试租户 ID（每次运行都不同 → 用例之间不互相污染）。
fn tenant_id(label: &str) -> String {
    format!("iam-audit-{label}-{}", uuid::Uuid::new_v4().simple())
}

/// 造服务（**注入假 sink**）。
async fn build_service(db_url: &str) -> (Arc<IamService>, Arc<RecordingSink>) {
    let tenant_db = TenantDb::connect(db_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let service =
        Arc::new(IamService::new(IamStore::new(tenant_db), test_config()).expect("服务应当能构造"));
    let sink = Arc::new(RecordingSink::default());
    assert!(
        service.with_audit_sink(sink.clone() as Arc<dyn AuditSink>),
        "sink 应当能注入一次"
    );
    (service, sink)
}

/// 来源元信息（**不同用例用不同 IP** —— 限流桶是全局的）。
fn meta(ip: &str) -> RequestMeta {
    RequestMeta {
        ip: ip.to_string(),
        user_agent: "audit-test-agent".to_string(),
        device_label: "audit-test-device".to_string(),
    }
}

/// 登录成功 → **1 条** `iam.login.succeeded`。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_successful_login_produces_exactly_one_success_event() {
    let db_url = setup().await;
    let (service, sink) = build_service(&db_url).await;
    let tenant = tenant_id("ok");
    let ctx = RequestContext::new().with_tenant(&tenant);
    service
        .register(
            &ctx,
            &RegisterRequest {
                username: "audit-ok".to_string(),
                password: PASSWORD.to_string(),
                email: None,
            },
        )
        .await
        .expect("注册应当成功");

    service
        .login(
            &ctx,
            &LoginRequest {
                username: "audit-ok".to_string(),
                password: PASSWORD.to_string(),
                mfa_code: None,
            },
            &meta("203.0.113.1"),
        )
        .await
        .expect("登录应当成功");

    let events = sink.of("iam.login.succeeded");
    assert_eq!(events.len(), 1, "登录成功 = **一条**成功事件");
    let event = &events[0];
    assert_eq!(event.object_type, "iam_session");
    assert_eq!(event.tenant_id, tenant);
    assert!(event.actor_id.is_some(), "成功事件带操作人");
    assert_eq!(event.ip.as_deref(), Some("203.0.113.1"));
    assert_eq!(event.user_agent.as_deref(), Some("audit-test-agent"));
    assert_eq!(event.before, None);
    assert_eq!(event.after, None);
    assert_eq!(sink.of("iam.login.failed").len(), 0, "成功时不得有失败事件");
}

/// 密码错误 → **1 条** `iam.login.failed`，且**不含账号名**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_wrong_password_produces_exactly_one_failure_event_without_the_account_name() {
    let db_url = setup().await;
    let (service, sink) = build_service(&db_url).await;
    let tenant = tenant_id("wrong");
    let ctx = RequestContext::new().with_tenant(&tenant);
    service
        .register(
            &ctx,
            &RegisterRequest {
                username: "audit-wrong".to_string(),
                password: PASSWORD.to_string(),
                email: None,
            },
        )
        .await
        .expect("注册应当成功");

    let error = service
        .login(
            &ctx,
            &LoginRequest {
                username: "audit-wrong".to_string(),
                password: "wrong-password-entirely".to_string(),
                mfa_code: None,
            },
            &meta("203.0.113.2"),
        )
        .await
        .expect_err("密码错误应当被拒");
    assert_eq!(error.code(), code::INVALID_CREDENTIALS);

    let events = sink.of("iam.login.failed");
    assert_eq!(events.len(), 1, "密码错误 = **一条**失败事件");
    let event = &events[0];
    assert_eq!(event.object_type, "iam_session");
    // 🔑 失败事件**不得含账号名**（AC-S-127 步骤 5）。
    assert!(event.actor_id.is_none(), "失败事件不得有操作人");
    assert!(event.object_id.is_none(), "失败事件不得有对象 ID");
    assert_eq!(event.before, None);
    assert_eq!(event.after, None);
    assert_eq!(event.ip.as_deref(), Some("203.0.113.2"));
}

/// 「账号不存在」与「密码错误」**条数相同** → 审计表不是枚举预言机。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_two_failure_paths_produce_the_same_number_of_events() {
    let db_url = setup().await;
    let (service, sink) = build_service(&db_url).await;
    let tenant = tenant_id("enum");
    let ctx = RequestContext::new().with_tenant(&tenant);
    service
        .register(
            &ctx,
            &RegisterRequest {
                username: "audit-enum".to_string(),
                password: PASSWORD.to_string(),
                email: None,
            },
        )
        .await
        .expect("注册应当成功");

    // 密码错误。
    let before_wrong_password = sink.of("iam.login.failed").len();
    service
        .login(
            &ctx,
            &LoginRequest {
                username: "audit-enum".to_string(),
                password: "wrong-password-entirely".to_string(),
                mfa_code: None,
            },
            &meta("203.0.113.3"),
        )
        .await
        .expect_err("密码错误应当被拒");
    let wrong_password_delta = sink.of("iam.login.failed").len() - before_wrong_password;

    // 账号不存在。
    let before_unknown_account = sink.of("iam.login.failed").len();
    service
        .login(
            &ctx,
            &LoginRequest {
                username: "audit-does-not-exist".to_string(),
                password: "wrong-password-entirely".to_string(),
                mfa_code: None,
            },
            &meta("203.0.113.4"),
        )
        .await
        .expect_err("账号不存在应当被拒");
    let unknown_account_delta = sink.of("iam.login.failed").len() - before_unknown_account;

    assert_eq!(wrong_password_delta, 1, "密码错误产生 1 条失败事件");
    assert_eq!(
        unknown_account_delta, wrong_password_delta,
        "「账号不存在」与「密码错误」的失败事件**条数必须相同**（不可枚举）"
    );
}

/// 审计写不进去 → **拒绝登录**（503 `AUDIT_SINK_UNAVAILABLE`，D157）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_failing_sink_rejects_the_login_with_503() {
    let db_url = setup().await;
    let tenant_db = TenantDb::connect(&db_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let service =
        Arc::new(IamService::new(IamStore::new(tenant_db), test_config()).expect("服务应当能构造"));
    assert!(
        service.with_audit_sink(Arc::new(FailingSink) as Arc<dyn AuditSink>),
        "sink 应当能注入一次"
    );
    let tenant = tenant_id("sink-down");
    let ctx = RequestContext::new().with_tenant(&tenant);
    service
        .register(
            &ctx,
            &RegisterRequest {
                username: "audit-sink-down".to_string(),
                password: PASSWORD.to_string(),
                email: None,
            },
        )
        .await
        .expect("注册应当成功");

    let error = service
        .login(
            &ctx,
            &LoginRequest {
                username: "audit-sink-down".to_string(),
                password: PASSWORD.to_string(),
                mfa_code: None,
            },
            &meta("203.0.113.5"),
        )
        .await
        .expect_err("审计写失败必须拒绝登录");
    assert_eq!(error.code(), code::AUDIT_SINK_UNAVAILABLE);
    assert_eq!(error.status().as_u16(), 503);
}
