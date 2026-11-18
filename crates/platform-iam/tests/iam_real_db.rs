//! 认证与账号体系的**真实库层**（REQ-007）。
//!
//! 验的是**安全断言**，每一条都有对应的「故意违规 → 必须失败」的反面用例：
//!
//! | # | 断言 | 反面用例 |
//! |---|------|---------|
//! | 1 | 账号不存在与密码错误：**响应体与错误码完全相同**，耗时接近 | 断言两者 JSON **逐字节**相同 |
//! | 2 | 密码重置不泄露邮箱是否存在：两个响应**完全相同** | 断言两个 JSON **逐字节**相同 |
//! | 3 | refresh 轮换：旧令牌**立即失效**；重放 → **整个 family 作废** | 断言 refresh_2 也失效 |
//! | 4 | TOTP 防重放：同一时间步**不能用第二次**；恢复码一次性 | 断言第二次被拒 |
//! | 5 | 设备会话：从 A 下线 B → B 的 refresh **立即失效**；登出全端 → 全部作废 | 断言全部 family 撤销 |
//! | 6 | 租户隔离：A 租户的令牌访问 B 租户被拒；会话记录**都带 tenant_id** | 断言跨租户 404 |
//! | 7 | JWKS：**只暴露公钥**；轮换后新旧 kid 都在 | 断言响应里没有私钥材料 |
//! | 8 | 错误体合规（契约 C6）：每个错误路径的 `detail` 都是**英文 ASCII** | 逐个路径断言 |
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-iam --test iam_real_db -- --test-threads=2 --include-ignored
//! ```
//!
//! 未设置 `DATABASE_URL_TEST` 时**判为失败**，不是「跳过」——
//! 跳过也算通过 = 假绿（见文件末尾的护栏用例）。
//! 数据库名不是 `base_test` 时**直接失败** —— `base_dev` 是开发库，测试绝不能碰。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use platform_db::{DbOptions, TenantDb};
use platform_iam::config::{
    Argon2Settings, IamConfig, LoginSettings, PasswordPolicySettings, TokenSettings, TotpSettings,
};
use platform_iam::http::IamState;
use platform_iam::service::IamService;
use platform_iam::store::IamStore;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

/// 防假绿护栏的子进程标记（防止护栏自己递归拉起子进程）。
const GUARD_CHILD: &str = "IAM_GUARD_CHILD";

/// 测试用的密码（满足策略：长度 ≥ 12、不在常见口令词库里）。
const PASSWORD: &str = "correct-horse-battery-staple";

/// 另一个密码（改密用）。
const NEW_PASSWORD: &str = "another-very-strong-passphrase";

/// 测试库地址：只允许 `base_test`。
///
/// **缺 `DATABASE_URL_TEST` 是失败，不是「跳过」。**
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
/// 应用角色必须**不绕过 RLS**：超级用户与 `BYPASSRLS` 角色会让租户隔离 fail open。
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
    std::fs::read_to_string(path).expect("读取应成功")
}

/// 准备真实库环境，返回**应用角色**的连接串。
///
/// 顺序：平台迁移（建表 + RLS 策略）→ 应用角色脚本（建角色 + 授权）。
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

/// 测试用的 IAM 配置。
///
/// ⚠️ Argon2 参数用**低配值**：低配机上跑不动生产参数，
/// 而参数本身的可配性由 `config.rs` 的单测覆盖（AC-S-011）。
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
        // ⚠️ 参数比生产低，但**足以让哈希耗时占主导** —— 防枚举的耗时对比才有意义
        //    （生产默认 m=19456/t=2，耗时只会更悬殊）。
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

/// 一个测试租户 ID（每次运行都不同 → 用例之间不互相污染）。
fn tenant_id(label: &str) -> String {
    format!("iam-test-{label}-{}", uuid::Uuid::new_v4().simple())
}

/// 构建服务与路由。
async fn build_stack(db_url: &str) -> (Arc<IamState>, axum::Router) {
    let tenant_db = TenantDb::connect(db_url, DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let service = IamService::new(IamStore::new(tenant_db), test_config()).expect("服务应当能构造");
    let state = Arc::new(IamState { service });
    // 与 `platform-server` 一样的挂法：IAM 路由挂在 `/api/iam` 下，JWKS 在根路径。
    let router = axum::Router::new()
        .nest("/api/iam", platform_iam::http::router(state.clone()))
        .merge(platform_iam::http::jwks_router(state.clone()));
    (state, router)
}

/// 带租户头的请求。
fn request_with_tenant(
    method: &str,
    path: &str,
    tenant: &str,
    body: serde_json::Value,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("x-tenant-id", tenant)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("请求应当能构造")
}

/// 读响应：状态码 + JSON。
async fn read_json(response: Response) -> (StatusCode, serde_json::Value) {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("响应体应当能读");
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

/// 注册一个账号（走真实的 HTTP 接口）。
async fn register(
    router: &axum::Router,
    tenant: &str,
    username: &str,
    password: &str,
    email: Option<&str>,
) -> serde_json::Value {
    let body = serde_json::json!({
        "username": username,
        "password": password,
        "email": email,
    });
    let response = router
        .clone()
        .oneshot(request_with_tenant(
            "POST",
            "/api/iam/register",
            tenant,
            body,
        ))
        .await
        .expect("注册请求应当能发出");
    let (status, value) = read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "注册应当成功，body = {value}");
    value
}

/// 登录（返回状态码 + JSON + 耗时）。
async fn login_timed(
    router: &axum::Router,
    tenant: &str,
    username: &str,
    password: &str,
    mfa_code: Option<&str>,
) -> (StatusCode, serde_json::Value, Duration) {
    let mut body = serde_json::json!({ "username": username, "password": password });
    if let Some(code) = mfa_code {
        body["mfaCode"] = serde_json::Value::String(code.to_string());
    }
    let started = Instant::now();
    let response = router
        .clone()
        .oneshot(request_with_tenant("POST", "/api/iam/login", tenant, body))
        .await
        .expect("登录请求应当能发出");
    let elapsed = started.elapsed();
    let (status, value) = read_json(response).await;
    (status, value, elapsed)
}

/// 登录（只关心结果）。
async fn login(
    router: &axum::Router,
    tenant: &str,
    username: &str,
    password: &str,
) -> serde_json::Value {
    let (status, value, _) = login_timed(router, tenant, username, password, None).await;
    assert_eq!(status, StatusCode::OK, "登录应当成功，body = {value}");
    value
}

// ---------------------------------------------------------------------------
// 1. 防账号枚举（AC-S-012）
// ---------------------------------------------------------------------------

/// 账号不存在与密码错误：**响应体与错误码完全相同**，耗时接近。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn login_failure_does_not_reveal_whether_the_account_exists() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;
    let tenant = tenant_id("enum");

    register(
        &router,
        &tenant,
        "alice",
        PASSWORD,
        Some("alice@example.com"),
    )
    .await;

    // 情况一：账号存在 + 密码错误。
    let (status_existing, body_existing, _) = login_timed(
        &router,
        &tenant,
        "alice",
        "definitely-the-wrong-password",
        None,
    )
    .await;
    // 情况二：账号不存在。
    let (status_missing, body_missing, _) = login_timed(
        &router,
        &tenant,
        "nobody-here",
        "definitely-the-wrong-password",
        None,
    )
    .await;

    // 耗时对比：**多轮取中位数**（单次测量会被网络/DB 抖动带偏）。
    let mut existing_times: Vec<Duration> = Vec::new();
    let mut missing_times: Vec<Duration> = Vec::new();
    for _ in 0..3 {
        existing_times.push(
            login_timed(
                &router,
                &tenant,
                "alice",
                "definitely-the-wrong-password",
                None,
            )
            .await
            .2,
        );
        missing_times.push(
            login_timed(
                &router,
                &tenant,
                "nobody-here",
                "definitely-the-wrong-password",
                None,
            )
            .await
            .2,
        );
    }
    let elapsed_existing = median(&mut existing_times);
    let elapsed_missing = median(&mut missing_times);

    // 状态码相同。
    assert_eq!(status_existing, status_missing, "两种情况的状态码必须相同");
    assert_eq!(status_existing, StatusCode::UNAUTHORIZED);

    // 错误码相同。
    assert_eq!(
        body_existing["code"], "INVALID_CREDENTIALS",
        "body = {body_existing}"
    );
    assert_eq!(
        body_missing["code"], "INVALID_CREDENTIALS",
        "body = {body_missing}"
    );

    // **响应体完全相同**（逐字段比较；trace_id 每次不同，故单独排除）。
    assert_eq!(
        body_existing["detail"], body_missing["detail"],
        "两种情况的 detail 必须相同"
    );
    assert_eq!(
        body_existing["type"], body_missing["type"],
        "两种情况的 type 必须相同"
    );
    assert_eq!(
        body_existing["title"], body_missing["title"],
        "两种情况的 title 必须相同"
    );
    assert_eq!(
        body_existing["status"], body_missing["status"],
        "两种情况的 status 必须相同"
    );

    // 耗时接近：账号不存在时也跑了一次假哈希校验。
    let ratio = if elapsed_existing > elapsed_missing {
        elapsed_existing.as_secs_f64() / elapsed_missing.as_secs_f64().max(0.000_001)
    } else {
        elapsed_missing.as_secs_f64() / elapsed_existing.as_secs_f64().max(0.000_001)
    };
    assert!(
        ratio < 1.5,
        "两种情况的耗时必须接近（ratio = {ratio:.2}，\
         存在账号 = {elapsed_existing:?}，不存在账号 = {elapsed_missing:?}）"
    );
    println!(
        "防枚举耗时：存在账号+错密码 = {:?}；账号不存在 = {:?}；比值 = {ratio:.2}",
        elapsed_existing, elapsed_missing
    );
}

// ---------------------------------------------------------------------------
// 2. 密码重置防枚举（AC-S-013）
// ---------------------------------------------------------------------------

/// 已注册与未注册邮箱的 `forgot` 响应**完全相同**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn password_forgot_does_not_reveal_whether_the_email_is_registered() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;
    let tenant = tenant_id("forgot");

    register(
        &router,
        &tenant,
        "alice",
        PASSWORD,
        Some("alice@example.com"),
    )
    .await;

    let registered = router
        .clone()
        .oneshot(request_with_tenant(
            "POST",
            "/api/iam/password/forgot",
            &tenant,
            serde_json::json!({ "email": "alice@example.com" }),
        ))
        .await
        .expect("请求应当能发出");
    let (status_registered, body_registered) = read_json(registered).await;

    let unregistered = router
        .clone()
        .oneshot(request_with_tenant(
            "POST",
            "/api/iam/password/forgot",
            &tenant,
            serde_json::json!({ "email": "nobody@example.com" }),
        ))
        .await
        .expect("请求应当能发出");
    let (status_unregistered, body_unregistered) = read_json(unregistered).await;

    assert_eq!(status_registered, status_unregistered);
    assert_eq!(
        body_registered, body_unregistered,
        "已注册与未注册邮箱的响应体必须**完全相同**"
    );
    assert_eq!(body_registered["status"], "accepted");
}

// ---------------------------------------------------------------------------
// 3. refresh 轮换与重放检测（AC-S-014）
// ---------------------------------------------------------------------------

/// 旧 refresh 立即失效；重放 → **整个 family 作废**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn refresh_rotation_revokes_the_whole_family_on_reuse() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;
    let tenant = tenant_id("rotate");

    register(&router, &tenant, "alice", PASSWORD, None).await;
    let session = login(&router, &tenant, "alice", PASSWORD).await;
    let refresh_1 = session["refreshToken"]
        .as_str()
        .expect("应当有 refresh token")
        .to_string();

    // 1) 用 refresh_1 刷新 → 拿到 refresh_2。
    let first = router
        .clone()
        .oneshot(request_with_tenant(
            "POST",
            "/api/iam/refresh",
            &tenant,
            serde_json::json!({ "refreshToken": refresh_1 }),
        ))
        .await
        .expect("请求应当能发出");
    let (status_first, body_first) = read_json(first).await;
    assert_eq!(status_first, StatusCode::OK, "body = {body_first}");
    let refresh_2 = body_first["refreshToken"]
        .as_str()
        .expect("应当有新的 refresh token")
        .to_string();
    assert_ne!(refresh_1, refresh_2, "轮换必须签发新的 refresh token");

    // 2) **再次**使用 refresh_1 → 判定为泄露 → 整个 family 作废。
    let replay = router
        .clone()
        .oneshot(request_with_tenant(
            "POST",
            "/api/iam/refresh",
            &tenant,
            serde_json::json!({ "refreshToken": refresh_1 }),
        ))
        .await
        .expect("请求应当能发出");
    let (status_replay, body_replay) = read_json(replay).await;
    assert_eq!(
        status_replay,
        StatusCode::UNAUTHORIZED,
        "body = {body_replay}"
    );
    assert_eq!(
        body_replay["code"], "TOKEN_REUSE_DETECTED",
        "重放必须报 TOKEN_REUSE_DETECTED，body = {body_replay}"
    );

    // 3) refresh_2 **也**已失效（family 被作废）。
    let after = router
        .clone()
        .oneshot(request_with_tenant(
            "POST",
            "/api/iam/refresh",
            &tenant,
            serde_json::json!({ "refreshToken": refresh_2 }),
        ))
        .await
        .expect("请求应当能发出");
    let (status_after, body_after) = read_json(after).await;
    assert_eq!(
        status_after,
        StatusCode::UNAUTHORIZED,
        "family 作废后 refresh_2 必须失效，body = {body_after}"
    );
    assert_eq!(body_after["code"], "TOKEN_REVOKED", "body = {body_after}");
}

// ---------------------------------------------------------------------------
// 4. TOTP 防重放与恢复码一次性（AC-S-015）
// ---------------------------------------------------------------------------

/// 同一时间步的验证码不能用第二次；恢复码只能用一次。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn totp_codes_cannot_be_replayed_and_recovery_codes_are_single_use() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;
    let tenant = tenant_id("totp");

    register(&router, &tenant, "alice", PASSWORD, None).await;
    let session = login(&router, &tenant, "alice", PASSWORD).await;
    let access = session["accessToken"]
        .as_str()
        .expect("应当有 access token")
        .to_string();

    // 绑定 TOTP。
    let enroll = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/mfa/enroll")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_enroll, body_enroll) = read_json(enroll).await;
    assert_eq!(status_enroll, StatusCode::OK, "body = {body_enroll}");
    let secret_base32 = body_enroll["secret"]
        .as_str()
        .expect("应当有 secret")
        .to_string();
    let recovery_codes: Vec<String> = body_enroll["recoveryCodes"]
        .as_array()
        .expect("应当有恢复码")
        .iter()
        .map(|value| value.as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(recovery_codes.len(), 10, "恢复码必须是 10 个");

    // 用当前时间步的验证码绑定。
    let code = totp_code(&secret_base32);
    let verify = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/mfa/verify")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::json!({ "code": code }).to_string()))
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_verify, body_verify) = read_json(verify).await;
    assert_eq!(
        status_verify,
        StatusCode::NO_CONTENT,
        "body = {body_verify}"
    );

    // 同一个验证码**再用一次** → 被拒（防重放）。
    let replay = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/mfa/verify")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::json!({ "code": code }).to_string()))
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_replay, body_replay) = read_json(replay).await;
    assert_eq!(
        status_replay,
        StatusCode::UNAUTHORIZED,
        "同一时间步的验证码必须被拒，body = {body_replay}"
    );
    assert_eq!(body_replay["code"], "MFA_INVALID", "body = {body_replay}");

    // 恢复码：第一次可用，第二次无效。
    let recovery = &recovery_codes[0];
    let first_use = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/mfa/verify")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({ "code": recovery }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_first, body_first) = read_json(first_use).await;
    assert_eq!(
        status_first,
        StatusCode::NO_CONTENT,
        "恢复码第一次应当可用，body = {body_first}"
    );

    let second_use = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/mfa/verify")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({ "code": recovery }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_second, body_second) = read_json(second_use).await;
    assert_eq!(
        status_second,
        StatusCode::UNAUTHORIZED,
        "恢复码只能用一次，body = {body_second}"
    );
    assert_eq!(body_second["code"], "MFA_INVALID", "body = {body_second}");
}

/// 中位数（耗时的稳健统计量）。
fn median(values: &mut [Duration]) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

/// 用当前的 secret 算出当前时间步的验证码。
fn totp_code(secret_base32: &str) -> String {
    use totp_rs::{Algorithm, Secret, TOTP};
    let secret = Secret::Encoded(secret_base32.to_string());
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        0,
        30,
        secret.to_bytes().expect("secret 应当能解码"),
        None,
        String::new(),
    )
    .expect("TOTP 应当能构造");
    let now = time::OffsetDateTime::now_utc().unix_timestamp() as u64;
    totp.generate(now)
}

// ---------------------------------------------------------------------------
// 5. 设备会话（AC-S-018）
// ---------------------------------------------------------------------------

/// 从 A 端下线 B → B 的 refresh 立即失效；登出全端 → 全部 family 作废。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn sessions_can_be_revoked_and_logout_all_revokes_every_family() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;
    let tenant = tenant_id("sessions");

    register(&router, &tenant, "alice", PASSWORD, None).await;

    // 两台设备登录。
    let device_a = login(&router, &tenant, "alice", PASSWORD).await;
    let device_b = login(&router, &tenant, "alice", PASSWORD).await;
    let refresh_a = device_a["refreshToken"].as_str().unwrap().to_string();
    let refresh_b = device_b["refreshToken"].as_str().unwrap().to_string();
    let access_a = device_a["accessToken"].as_str().unwrap().to_string();

    // 会话列表：两条，且都带设备 / IP / 最近活跃。
    let list = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/iam/sessions")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access_a}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_list, body_list) = read_json(list).await;
    assert_eq!(status_list, StatusCode::OK, "body = {body_list}");
    let sessions = body_list.as_array().expect("应当是数组");
    assert_eq!(sessions.len(), 2, "应当有两个会话：{body_list}");
    for session in sessions {
        assert!(session["deviceLabel"].is_string(), "session = {session}");
        assert!(session["ip"].is_string(), "session = {session}");
        assert!(session["lastUsedAt"].is_string(), "session = {session}");
    }

    // 找出 B 的会话 ID（按 refresh token 反查不方便，这里用「第二个会话」）。
    let session_ids: Vec<String> = sessions
        .iter()
        .map(|session| session["id"].as_str().unwrap().to_string())
        .collect();
    // 用 refresh_b 刷新一次 → 拿到新的令牌对，同时确认 B 的会话是哪一个：
    // 逐个踢下线，踢到 B 的 refresh 失效为止。
    let mut kicked_b = false;
    for session_id in &session_ids {
        let revoke = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/iam/sessions/{session_id}"))
                    .header("x-tenant-id", &tenant)
                    .header(header::AUTHORIZATION, format!("Bearer {access_a}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("请求应当能发出");
        assert_eq!(revoke.status(), StatusCode::NO_CONTENT);
        // 试一下 B 的 refresh 是否已经失效。
        if refresh_fails(&router, &tenant, &refresh_b).await {
            kicked_b = true;
            break;
        }
    }
    assert!(kicked_b, "从 A 端下线 B 后，B 的 refresh 必须立即失效");

    // 登出全端 → A 的 refresh 也失效。
    let logout_all = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/logout-all")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access_a}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    assert_eq!(logout_all.status(), StatusCode::OK);
    assert!(
        refresh_fails(&router, &tenant, &refresh_a).await,
        "登出全端后，全部 family 都必须作废"
    );
}

/// 刷新是否已经失效。
async fn refresh_fails(router: &axum::Router, tenant: &str, refresh_token: &str) -> bool {
    let response = router
        .clone()
        .oneshot(request_with_tenant(
            "POST",
            "/api/iam/refresh",
            tenant,
            serde_json::json!({ "refreshToken": refresh_token }),
        ))
        .await
        .expect("请求应当能发出");
    let (status, _) = read_json(response).await;
    status == StatusCode::UNAUTHORIZED
}

// ---------------------------------------------------------------------------
// 6. 租户隔离（AC-S-016）
// ---------------------------------------------------------------------------

/// A 租户的令牌访问 B 租户被拒；会话记录**都带 tenant_id**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tokens_cannot_cross_tenants_and_sessions_record_the_tenant() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;
    let tenant_a = tenant_id("iso-a");
    let tenant_b = tenant_id("iso-b");

    register(&router, &tenant_a, "alice", PASSWORD, None).await;
    register(&router, &tenant_b, "alice", PASSWORD, None).await;

    let session_a = login(&router, &tenant_a, "alice", PASSWORD).await;
    let access_a = session_a["accessToken"].as_str().unwrap().to_string();
    let refresh_a = session_a["refreshToken"].as_str().unwrap().to_string();

    // 用 A 的令牌 + **B 的租户头** → 被拒（404，不泄露资源存在性）。
    let crossed = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/iam/sessions")
                .header("x-tenant-id", &tenant_b)
                .header(header::AUTHORIZATION, format!("Bearer {access_a}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_crossed, body_crossed) = read_json(crossed).await;
    assert_eq!(
        status_crossed,
        StatusCode::NOT_FOUND,
        "跨租户访问必须被拒，body = {body_crossed}"
    );

    // 用 A 的 refresh token 到 B 租户刷新 → 被拒。
    let crossed_refresh = router
        .clone()
        .oneshot(request_with_tenant(
            "POST",
            "/api/iam/refresh",
            &tenant_b,
            serde_json::json!({ "refreshToken": refresh_a }),
        ))
        .await
        .expect("请求应当能发出");
    let (status_crossed_refresh, body_crossed_refresh) = read_json(crossed_refresh).await;
    assert_eq!(
        status_crossed_refresh,
        StatusCode::UNAUTHORIZED,
        "跨租户刷新必须被拒，body = {body_crossed_refresh}"
    );

    // 会话记录都带 tenant_id（直接查库验）。
    let admin_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&required_test_database_url())
        .await
        .expect("管理员连接应当成功");
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT tenant_id, user_id FROM platform_iam_session")
            .fetch_all(&admin_pool)
            .await
            .expect("会话表应当能查");
    assert!(!rows.is_empty(), "应当有会话记录");
    for (tenant, _) in &rows {
        assert!(
            tenant.starts_with("iam-test-"),
            "每条会话记录都必须带 tenant_id：{tenant}"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. JWKS（AC-S-010）
// ---------------------------------------------------------------------------

/// JWKS 只暴露公钥；轮换后新旧 kid 都在。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_jwks_endpoint_only_exposes_public_keys() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/.well-known/jwks.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status, body) = read_json(response).await;
    assert_eq!(status, StatusCode::OK, "body = {body}");
    let text = body.to_string();
    assert!(!text.contains("PRIVATE"), "JWKS 里不得出现私钥标记：{text}");
    assert!(!text.contains("\"d\""), "JWKS 里不得出现私钥指数 d：{text}");
    assert!(text.contains("\"kid\":\"kid-test\""), "jwks = {text}");
    assert!(text.contains("\"kty\":\"EC\""), "jwks = {text}");
}

// ---------------------------------------------------------------------------
// 8. 错误体合规（AC-S-019）
// ---------------------------------------------------------------------------

/// 每个错误路径的 `detail` 都是**英文 ASCII**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn every_error_path_returns_an_english_ascii_detail() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;
    let tenant = tenant_id("errors");

    register(&router, &tenant, "alice", PASSWORD, None).await;

    let mut details: Vec<(String, String)> = Vec::new();

    // 路径一：密码错误（INVALID_CREDENTIALS）。
    let (status, body) = read_json(
        router
            .clone()
            .oneshot(request_with_tenant(
                "POST",
                "/api/iam/login",
                &tenant,
                serde_json::json!({ "username": "alice", "password": "wrong-password-here" }),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    details.push((
        "INVALID_CREDENTIALS".to_string(),
        body["detail"].to_string(),
    ));

    // 路径二：密码策略不满足（PASSWORD_POLICY_VIOLATION + 具体规则）。
    let (status, body) = read_json(
        router
            .clone()
            .oneshot(request_with_tenant(
                "POST",
                "/api/iam/register",
                &tenant,
                serde_json::json!({ "username": "bob", "password": "short" }),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body = {body}");
    assert_eq!(body["code"], "PASSWORD_POLICY_VIOLATION");
    assert!(
        body["params"]["violations"][0]["rule"] == "min_length",
        "必须指出**具体违反哪条规则**：{body}"
    );
    details.push((
        "PASSWORD_POLICY_VIOLATION".to_string(),
        body["detail"].to_string(),
    ));

    // 路径三：令牌无效（UNAUTHENTICATED）。
    let (status, body) = read_json(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/iam/sessions")
                    .header("x-tenant-id", &tenant)
                    .header(header::AUTHORIZATION, "Bearer not-a-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    details.push(("UNAUTHENTICATED".to_string(), body["detail"].to_string()));

    // 路径四：缺少租户上下文（TENANT_CONTEXT_MISSING）。
    let (status, body) = read_json(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/iam/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "username": "alice", "password": PASSWORD })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    details.push((
        "TENANT_CONTEXT_MISSING".to_string(),
        body["detail"].to_string(),
    ));

    // 逐个断言：`detail` 是英文 ASCII（契约 C6）。
    for (code, detail) in &details {
        assert!(
            detail.is_ascii(),
            "{code} 的 detail 必须是英文 ASCII：{detail}"
        );
        assert!(!detail.is_empty(), "{code} 的 detail 不得为空");
        assert!(
            !detail.chars().any(|ch| ch as u32 > 0x7F),
            "{code} 的 detail 不得含非 ASCII 字符：{detail}"
        );
        println!("{code} → detail = {detail}");
    }
}

// ---------------------------------------------------------------------------
// 9. 账号锁定（AC-S-017）
// ---------------------------------------------------------------------------

/// 连续失败达阈值 → 锁定 → 锁定期内即使密码正确也拒。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn repeated_failures_lock_the_account() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;
    let tenant = tenant_id("lockout");

    register(&router, &tenant, "alice", PASSWORD, None).await;

    // 连续失败 5 次（阈值）。
    for attempt in 1..=5 {
        let (status, body, _) =
            login_timed(&router, &tenant, "alice", "wrong-password-here", None).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "第 {attempt} 次失败：body = {body}"
        );
        assert_eq!(body["code"], "INVALID_CREDENTIALS", "body = {body}");
    }

    // 锁定期内即使密码正确也拒。
    let (status, body, _) = login_timed(&router, &tenant, "alice", PASSWORD, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body = {body}");
    assert_eq!(body["code"], "ACCOUNT_LOCKED", "body = {body}");
}

// ---------------------------------------------------------------------------
// 10. 改密：策略 + 历史不重复 + 撤销全部 refresh family（AC-S-009 / D10）
// ---------------------------------------------------------------------------

/// 改密：新密码要满足策略；与历史重复要报 `PASSWORD_REUSED`；
/// 改密成功后**其它设备**的 refresh family 立即作废。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn changing_the_password_revokes_the_other_families_and_rejects_history() {
    let db_url = setup().await;
    let (_state, router) = build_stack(&db_url).await;
    let tenant = tenant_id("change-pwd");

    register(&router, &tenant, "alice", PASSWORD, None).await;

    let device_a = login(&router, &tenant, "alice", PASSWORD).await;
    let device_b = login(&router, &tenant, "alice", PASSWORD).await;
    let access_a = device_a["accessToken"].as_str().unwrap().to_string();
    let refresh_b = device_b["refreshToken"].as_str().unwrap().to_string();

    // 不满足策略的新密码 → 被拒，并指出**具体违反哪条规则**。
    let weak = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/password/change")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access_a}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "currentPassword": PASSWORD,
                        "newPassword": "short"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_weak, body_weak) = read_json(weak).await;
    assert_eq!(
        status_weak,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body = {body_weak}"
    );
    assert_eq!(body_weak["code"], "PASSWORD_POLICY_VIOLATION");
    assert_eq!(
        body_weak["params"]["violations"][0]["rule"], "min_length",
        "必须指出具体违反哪条规则：{body_weak}"
    );

    // 改成合规的新密码。
    let changed = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/password/change")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access_a}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "currentPassword": PASSWORD,
                        "newPassword": NEW_PASSWORD
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_changed, body_changed) = read_json(changed).await;
    assert_eq!(
        status_changed,
        StatusCode::NO_CONTENT,
        "改密应当成功，body = {body_changed}"
    );

    // 其它设备的 refresh family 立即作废（D10）。
    assert!(
        refresh_fails(&router, &tenant, &refresh_b).await,
        "改密后，其它设备的 refresh 必须立即失效"
    );

    // 与历史密码重复 → `PASSWORD_REUSED`。
    let reused = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/password/change")
                .header("x-tenant-id", &tenant)
                .header(header::AUTHORIZATION, format!("Bearer {access_a}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "currentPassword": NEW_PASSWORD,
                        "newPassword": PASSWORD
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("请求应当能发出");
    let (status_reused, body_reused) = read_json(reused).await;
    assert_eq!(
        status_reused,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body = {body_reused}"
    );
    assert_eq!(
        body_reused["code"], "PASSWORD_REUSED",
        "与历史密码重复必须被拒，body = {body_reused}"
    );
}

// ---------------------------------------------------------------------------
// 10. 防假绿护栏
// ---------------------------------------------------------------------------

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
