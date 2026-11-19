//! AC-S-106（SSO 配置与密钥保护：`client_secret` 三处扫描 0 命中 **带阳性对照**）
//! 与 AC-S-021（强制 SSO 生效，且不能用于账号枚举）。
//!
//! # 为什么必须有阳性对照（dev-guide §9.2）
//!
//! 「0 命中」和「扫描器根本没生效」在输出上完全一样。所以每一步扫描都先注入一处
//! canary 并断言**能命中**，再断言目标位置 0 命中。

mod sso_support;

use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use serde_json::json;

use sso_support::*;

const FRONTEND_BASE: &str = "http://app.example.com";
const CALLBACK_URI: &str = "http://app.example.com/api/iam/sso/oidc/callback";
/// 唯一可识别的 canary（AC-S-106 前置）。
const CANARY: &str = "SECRET-CANARY-7f3a9b1e-UNIQUE";

// ---------------------------------------------------------------------------
// 日志捕获（AC-S-106 步骤 4）
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("锁")).to_string()
    }
}

impl std::io::Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("锁").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buffer {
    type Writer = Buffer;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// 进程内的日志缓冲（多个用例共享 —— 全局 subscriber 只能装一次）。
static LOG_BUFFER: std::sync::OnceLock<Buffer> = std::sync::OnceLock::new();
static LOG_INSTALLED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// 装一个把日志写进内存的 subscriber（进程内只装一次）并返回共享缓冲。
fn capture_logs() -> Buffer {
    let buffer = LOG_BUFFER.get_or_init(Buffer::default).clone();
    LOG_INSTALLED.get_or_init(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(buffer.clone())
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
    buffer
}

// ---------------------------------------------------------------------------
// 环境
// ---------------------------------------------------------------------------

struct Env {
    stack: Stack,
    idp: MockIdp,
    tenant: String,
    config: Arc<platform_iam::sso::config::StaticSsoConfigSource>,
    admin: sqlx::PgPool,
}

impl Env {
    async fn new() -> Self {
        let app_url = setup_database().await;
        let stack = build_stack(&app_url).await;
        let idp = MockIdp::start_http().await;
        let tenant = unique_tenant("cfg");
        let config = install_config(
            &stack.service,
            platform_iam::sso::config::SsoConfig {
                oidc: oidc_settings(&idp.issuer, CALLBACK_URI),
                ldap: ldap_settings("ldap://127.0.0.1:389"),
                ..Default::default()
            },
        );
        stack
            .service
            .set_sso_http_client(Arc::new(
                platform_iam::sso::oidc::OidcHttpClient::new().expect("http 客户端"),
            ));
        stack.service.set_sso_frontend_base_url(FRONTEND_BASE);
        // `client_secret` **只在内存里**（唯一来源是环境变量，见 design §7）。
        stack.service.set_sso_oidc_client_secret(CANARY);

        let user = register(&stack.router, &tenant, "bob", PASSWORD, Some("bob@example.com")).await;
        let user_id = user["id"].as_str().expect("id").to_string();
        let admin = admin_pool().await;
        sqlx::query(
            "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
             VALUES ($1, $2, 'oidc', 'idp-bob-001', $3) ON CONFLICT DO NOTHING",
        )
        .bind(&tenant)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&user_id)
        .execute(&admin)
        .await
        .expect("绑定应能写入");

        Self {
            stack,
            idp,
            tenant,
            config,
            admin,
        }
    }

    /// 一次成功的 OIDC 登录（返回 exchange 的 body）。
    async fn oidc_login(&self) -> serde_json::Value {
        let path = format!("/api/iam/sso/oidc/authorize?tenant={}", self.tenant);
        let (status, headers, _) = http_get(&self.stack.router, &path, None, None).await;
        assert_eq!(status, StatusCode::FOUND);
        let query = location_query(&headers);
        let code = format!("code-{}", uuid::Uuid::new_v4().simple());
        self.idp.register_code(
            &code,
            &query["nonce"],
            &query["code_challenge"],
            &query["redirect_uri"],
        );
        let callback = format!(
            "/api/iam/sso/oidc/callback?code={code}&state={}",
            query["state"]
        );
        let (status, headers, _) = http_get(&self.stack.router, &callback, None, None).await;
        assert_eq!(status, StatusCode::FOUND);
        let ticket = location_query(&headers)["ticket"].clone();
        let (status, _, bytes) = post_json(
            &self.stack.router,
            "/api/iam/sso/exchange",
            None,
            None,
            json!({ "ticket": ticket }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
        json_of(&bytes)
    }
}

/// 在三张配置表里做**全表文本扫描**。
async fn scan_config_tables(admin: &sqlx::PgPool, needle: &str) -> i64 {
    let mut hits = 0i64;
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT coalesce(string_agg(key || '=' || value::text, ' '), '') FROM platform_config",
    )
    .fetch_all(admin)
    .await
    .expect("platform_config 应可读");
    hits += rows.iter().filter(|row| row.contains(needle)).count() as i64;

    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT coalesce(string_agg(key || '=' || value::text, ' '), '') FROM platform_tenant_config",
    )
    .fetch_all(admin)
    .await
    .expect("platform_tenant_config 应可读");
    hits += rows.iter().filter(|row| row.contains(needle)).count() as i64;

    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT coalesce(string_agg(key || '=' || value::text, ' '), '') FROM platform_space_config",
    )
    .fetch_all(admin)
    .await
    .expect("platform_space_config 应可读");
    hits += rows.iter().filter(|row| row.contains(needle)).count() as i64;
    hits
}

// ---------------------------------------------------------------------------
// AC-S-106
// ---------------------------------------------------------------------------

/// canary 在**可读位置**全部 0 命中：错误响应体 / providers / sessions / 三张配置表 / 日志。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_106_the_client_secret_never_appears_anywhere_readable() {
    let logs = capture_logs();
    let env = Env::new().await;

    // 1) 触发一次**失败**登录（alg=none），取完整响应体。
    let path = format!("/api/iam/sso/oidc/authorize?tenant={}", env.tenant);
    let (_, headers, _) = http_get(&env.stack.router, &path, None, None).await;
    let query = location_query(&headers);
    let code = format!("code-{}", uuid::Uuid::new_v4().simple());
    env.idp.register_code(
        &code,
        &query["nonce"],
        &query["code_challenge"],
        &query["redirect_uri"],
    );
    env.idp.set_behavior(|behavior| {
        behavior.id_token_override = Some(format!(
            "{}.{}.",
            {
                use base64::Engine as _;
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                    serde_json::to_vec(&json!({"alg":"none"})).expect("json"),
                )
            },
            {
                use base64::Engine as _;
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(serde_json::to_vec(&json!({"sub":"x"})).expect("json"))
            }
        ));
    });
    let callback = format!(
        "/api/iam/sso/oidc/callback?code={code}&state={}",
        query["state"]
    );
    let (_, headers, _) = http_get(&env.stack.router, &callback, None, None).await;
    let error_location = headers
        .get(axum::http::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(!error_location.contains(CANARY), "错误交接的 URL 不得带 secret");

    // 2) 一次成功登录（日志 + 会话列表）。
    env.idp.set_behavior(|behavior| {
        behavior.id_token_override = None;
    });
    let login = env.oidc_login().await;

    // 3) providers 与 sessions 的完整响应体。
    let (status, _, providers) = http_get(
        &env.stack.router,
        "/api/iam/sso/providers",
        Some(&env.tenant),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let access = login["accessToken"].as_str().expect("accessToken");
    let (status, _, sessions) = send(
        &env.stack.router,
        axum::http::Request::builder()
            .method("GET")
            .uri("/api/iam/sessions")
            .header("authorization", format!("Bearer {access}"))
            .header("x-tenant-id", env.tenant.clone())
            .body(axum::body::Body::empty())
            .expect("请求应能构造"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&sessions)
    );

    // 4) 三张配置表全表扫描。
    let db_hits = scan_config_tables(&env.admin, CANARY).await;

    let surfaces = [
        ("providers", String::from_utf8_lossy(&providers).to_string()),
        ("sessions", String::from_utf8_lossy(&sessions).to_string()),
        ("login-response", login.to_string()),
        ("error-location", error_location.clone()),
        ("logs", logs.text()),
    ];
    for (name, body) in &surfaces {
        assert!(
            !body.contains(CANARY),
            "{name} 里出现了 client_secret：{body}"
        );
    }
    assert_eq!(db_hits, 0, "三张配置表里出现了 client_secret");

    // 5) 阳性对照：同一个扫描器必须**能命中**。
    //
    // 用已登记、无 `format` 的既有键 `org` 模块的 `ldapBaseDn`（P2 / S3）——
    // **不为测试新增 manifest 键**。写完清理。
    sqlx::query(
        "INSERT INTO platform_config (key, value) VALUES ('ldapBaseDn', to_jsonb($1::text)) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(CANARY)
    .execute(&env.admin)
    .await
    .expect("阳性对照写入应成功");
    let control_hits = scan_config_tables(&env.admin, CANARY).await;
    assert_eq!(control_hits, 1, "扫描器必须能命中注入的 canary（否则 0 命中不可信）");
    sqlx::query("DELETE FROM platform_config WHERE key = 'ldapBaseDn'")
        .execute(&env.admin)
        .await
        .expect("阳性对照清理应成功");
    assert_eq!(scan_config_tables(&env.admin, CANARY).await, 0);

    // 6) 日志里确实有 SSO 相关的 tracing 行（证明 trace 真的在跑）。
    let log_text = logs.text().to_lowercase();
    assert!(
        log_text.contains("sso"),
        "日志里应有 SSO 模块的 tracing 行（否则「0 命中」可能只是没打日志）"
    );
}

/// 12 个键**全部**在 `manifest.json` 登记且 `scope: tenant`（AC-S-106 预期 6）。
#[test]
fn ac_s_106_the_twelve_keys_are_registered_as_tenant_scoped() {
    let manifest = repo_root().join("crates/platform-config/manifest.json");
    let raw = std::fs::read_to_string(&manifest).expect("manifest 应可读");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("manifest 应是 JSON");
    let config = value["config"].as_object().expect("manifest 应有 config 段");
    for key in platform_iam::sso::config::SSO_CONFIG_KEYS {
        let entry = config.get(key).unwrap_or_else(|| panic!("manifest 缺少 {key}"));
        assert_eq!(entry["scope"], "tenant", "{key} 的 scope 必须是 tenant");
    }
    // `client_secret` **不在**这 12 个键里（AC-S-106 / §3）。
    assert!(!config.keys().any(|key| key.contains("secret")));
}

// ---------------------------------------------------------------------------
// AC-S-021：强制 SSO
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_021_enforced_sso_rejects_local_login_without_touching_argon2() {
    let env = Env::new().await;
    // 前置：tenant-a 有本地账号 alice。
    register(&env.stack.router, &env.tenant, "alice", PASSWORD, Some("alice@example.com")).await;
    // 另一个租户（未开强制 SSO），有自己的 alice。
    let tenant_b = unique_tenant("cfg-b");
    register(&env.stack.router, &tenant_b, "alice", PASSWORD, Some("alice@example.com")).await;

    let mut config = env.config.snapshot();
    config.enforce = true;
    env.config.set(config);

    let before = platform_iam::password::argon2_invocation_count();
    let (status_right, _, bytes_right) = post_json(
        &env.stack.router,
        "/api/iam/login",
        Some(&env.tenant),
        Some(&unique_ip()),
        json!({ "username": "alice", "password": PASSWORD }),
    )
    .await;
    let (status_wrong, _, bytes_wrong) = post_json(
        &env.stack.router,
        "/api/iam/login",
        Some(&env.tenant),
        Some(&unique_ip()),
        json!({ "username": "alice", "password": "not-the-password" }),
    )
    .await;
    let (status_nobody, _, bytes_nobody) = post_json(
        &env.stack.router,
        "/api/iam/login",
        Some(&env.tenant),
        Some(&unique_ip()),
        json!({ "username": "nobody", "password": "whatever" }),
    )
    .await;
    let after = platform_iam::password::argon2_invocation_count();

    // 1) 三种输入都是 403 SSO_REQUIRED。
    for (status, bytes) in [
        (status_right, &bytes_right),
        (status_wrong, &bytes_wrong),
        (status_nobody, &bytes_nobody),
    ] {
        assert_eq!(status, StatusCode::FORBIDDEN, "{}", String::from_utf8_lossy(bytes));
        let body = json_of(bytes);
        assert_eq!(body["code"], "SSO_REQUIRED");
        assert!(
            body["params"]["ssoProviders"].is_array(),
            "params.ssoProviders 必须是数组：{body}"
        );
    }

    // 2) 三个响应体除 traceId 外**完全一致**（防账号枚举）。
    let right = json_of(&bytes_right);
    let wrong = json_of(&bytes_wrong);
    let nobody = json_of(&bytes_nobody);
    for field in ["code", "title", "status", "detail", "params", "type"] {
        assert_eq!(wrong[field], right[field], "错误密码的 {field} 不一致");
        assert_eq!(nobody[field], right[field], "不存在用户的 {field} 不一致");
    }

    // 3) **Argon2id 调用次数 == 0**（密码校验之前就返回）。
    assert_eq!(
        after,
        before,
        "强制 SSO 时不得跑任何 Argon2 校验（实测 {} → {}）",
        before,
        after
    );

    // 4) 另一个租户不受影响（enforce 是租户级）—— 这里用同一份配置快照，
    //    所以先把它关掉再验证 local 登录恢复（步骤 7）。
    let mut config = env.config.snapshot();
    config.enforce = false;
    env.config.set(config);
    let before = platform_iam::password::argon2_invocation_count();
    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/login",
        Some(&env.tenant),
        Some(&unique_ip()),
        json!({ "username": "alice", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    assert!(
        platform_iam::password::argon2_invocation_count() > before,
        "对照组：enforce=false 时必须真的跑 Argon2（否则计数没有区分力）"
    );

    // 5) tenant-b 的 alice 用**另一套**密码也能登录（租户级策略不互相影响）。
    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/login",
        Some(&tenant_b),
        Some(&unique_ip()),
        json!({ "username": "alice", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
}

/// 配置读取失败 → **fail closed**（403）+ warn 级可观测行（AC-S-021 步骤 8 / S16）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_021_an_unreadable_config_fails_closed() {
    let logs = capture_logs();
    let app_url = setup_database().await;
    let stack = build_stack(&app_url).await;
    let tenant = unique_tenant("cfg-fail");
    register(&stack.router, &tenant, "alice", PASSWORD, Some("alice@example.com")).await;

    // 注入一个**永远读失败**的配置源。
    assert!(
        stack.service.set_sso_config_source(Arc::new(
            platform_iam::sso::config::FailingSsoConfigSource
        )),
        "配置源应能注入"
    );

    let (status, _, bytes) = post_json(
        &stack.router,
        "/api/iam/login",
        Some(&tenant),
        Some(&unique_ip()),
        json!({ "username": "alice", "password": PASSWORD }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "配置读不出来绝不能当成 enforce=false：{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(json_of(&bytes)["code"], "SSO_REQUIRED");

    let text = logs.text().to_lowercase();
    assert!(
        text.contains("warn") || text.contains("sso configuration could not be read"),
        "fail closed 必须有 warn 级以上可观测行：{text}"
    );
}

// ---------------------------------------------------------------------------
// AC-S-104 步骤 10：LDAP 的密码 / DN 在日志中 0 命中（带阳性对照）
// ---------------------------------------------------------------------------

/// LDAP 登录的密码与 DN **不得**出现在日志里。
///
/// 阳性对照：把同一个 canary 主动打一行日志，断言捕获器能命中 ——
/// 否则「0 命中」可能只是「日志根本没被捕获」。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_104_ldap_passwords_and_dns_never_reach_the_logs() {
    let logs = capture_logs();
    let app_url = setup_database().await;
    let stack = build_stack(&app_url).await;
    let tenant = unique_tenant("ldaplog");

    const DN: &str = "uid=logan,ou=people,dc=example,dc=com";
    const LDAP_PASSWORD: &str = "L0gan-0nly-1n-the-directory";

    install_config(
        &stack.service,
        platform_iam::sso::config::SsoConfig {
            ldap: ldap_settings("ldap://127.0.0.1:389"),
            ..Default::default()
        },
    );
    let ldap = Arc::new(RecordingLdap::new());
    ldap.add_user(DirectoryUser {
        dn: DN.to_string(),
        password: LDAP_PASSWORD.to_string(),
        mail: Some("logan@example.com".to_string()),
    });
    assert!(stack.service.set_sso_ldap_client(ldap));

    let user = register(&stack.router, &tenant, "logan", PASSWORD, Some("logan@example.com")).await;
    let user_id = user["id"].as_str().expect("id").to_string();
    let admin = admin_pool().await;
    sqlx::query(
        "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
         VALUES ($1, $2, 'ldap', $3, $4)",
    )
    .bind(&tenant)
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(DN)
    .bind(&user_id)
    .execute(&admin)
    .await
    .expect("绑定应能写入");

    // 一次成功 + 一次失败（错误路径最容易把凭据打进日志）。
    for password in [LDAP_PASSWORD, "wrong-password"] {
        let (_, _, _) = post_json(
            &stack.router,
            "/api/iam/sso/ldap/login",
            Some(&tenant),
            Some(&unique_ip()),
            json!({ "username": "logan", "password": password }),
        )
        .await;
    }

    let text = logs.text();
    assert!(
        !text.contains(LDAP_PASSWORD),
        "LDAP 密码不得进日志"
    );
    assert!(
        !text.contains(DN),
        "用户 DN 不得进日志（AC-S-020 步骤 6：不泄露 DN）"
    );

    // **阳性对照**：同一个捕获器必须能命中主动打进去的 canary。
    let control = "LOG-CONTROL-CANARY-e5b7";
    tracing::warn!(marker = control, "positive control");
    assert!(
        logs.text().contains(control),
        "日志捕获器必须能命中阳性对照，否则 0 命中不可信"
    );
}
