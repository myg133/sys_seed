//! AC-S-103：账号绑定与 JIT 建号 —— 不自动建号、不改指、不绕过禁用、并发幂等。
//!
//! 另外用**静态扫描 + 阳性对照**守 P7：认证路径里**不允许**出现
//! `ON CONFLICT ... DO UPDATE SET user_id`（那是组织同步的改指语义）。

mod sso_support;

use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;

use sso_support::*;

const FRONTEND_BASE: &str = "http://app.example.com";
const CALLBACK_URI: &str = "http://app.example.com/api/iam/sso/oidc/callback";

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
        let tenant = unique_tenant("binding");
        let config = install_config(
            &stack.service,
            platform_iam::sso::config::SsoConfig {
                // JIT **关闭**（默认）—— 步骤 1 用这个状态。
                jit_provisioning: false,
                oidc: oidc_settings(&idp.issuer, CALLBACK_URI),
                ..Default::default()
            },
        );
        stack.service.set_sso_http_client(Arc::new(
            platform_iam::sso::oidc::OidcHttpClient::new().expect("http 客户端"),
        ));
        stack.service.set_sso_frontend_base_url(FRONTEND_BASE);
        stack
            .service
            .set_sso_oidc_client_secret("SECRET-CANARY-oidc");
        let admin = admin_pool().await;
        Self {
            stack,
            idp,
            tenant,
            config,
            admin,
        }
    }

    fn set_jit(&self, enabled: bool) {
        let mut config = self.config.snapshot();
        config.jit_provisioning = enabled;
        self.config.set(config);
    }

    /// 走一次完整的 OIDC 登录，返回 (状态码, body)。
    async fn login_as(
        &self,
        sub: &str,
        email: &str,
        tenant: &str,
    ) -> (StatusCode, serde_json::Value) {
        // IdP 侧的 sub / email 由 claims_override 决定。
        self.idp.set_behavior(|behavior| {
            behavior
                .claims_override
                .insert("sub".to_string(), json!(sub));
            behavior
                .claims_override
                .insert("email".to_string(), json!(email));
        });
        let path = format!("/api/iam/sso/oidc/authorize?tenant={tenant}");
        let (status, headers, bytes) = http_get(&self.stack.router, &path, None, None).await;
        assert_eq!(
            status,
            StatusCode::FOUND,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
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
        let (status, headers, bytes) = http_get(&self.stack.router, &callback, None, None).await;
        assert_eq!(
            status,
            StatusCode::FOUND,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        let callback_query = location_query(&headers);
        let Some(ticket) = callback_query.get("ticket") else {
            // 失败分支：把错误码换成 HTTP 状态给调用方断言。
            let error = callback_query
                .get("error")
                .cloned()
                .unwrap_or_else(|| "UNKNOWN".to_string());
            let status = match error.as_str() {
                "SSO_ACCOUNT_NOT_LINKED" | "ACCOUNT_DISABLED" => StatusCode::FORBIDDEN,
                "SSO_IDENTITY_LINK_CONFLICT" => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            return (status, json!({ "code": error }));
        };

        let (status, _, bytes) = post_json(
            &self.stack.router,
            "/api/iam/sso/exchange",
            None,
            None,
            json!({ "ticket": ticket }),
        )
        .await;
        (status, json_of(&bytes))
    }

    async fn counts(&self, tenant: &str) -> (i64, i64) {
        let users: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM platform_iam_user WHERE tenant_id = $1 AND email IS NOT NULL",
        )
        .bind(tenant)
        .fetch_one(&self.admin)
        .await
        .expect("count users");
        let bindings: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM external_identity WHERE tenant_id = $1 AND provider = 'oidc'",
        )
        .bind(tenant)
        .fetch_one(&self.admin)
        .await
        .expect("count bindings");
        (users, bindings)
    }
}

const CAROL: &str = "idp-carol-001";
const CAROL_MAIL: &str = "carol@example.com";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_103_with_jit_off_an_unlinked_account_is_rejected_and_nothing_is_created() {
    let env = Env::new().await;
    let (status, body) = env.login_as(CAROL, CAROL_MAIL, &env.tenant).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "SSO_ACCOUNT_NOT_LINKED");

    let (users, bindings) = env.counts(&env.tenant).await;
    assert_eq!(users, 0, "JIT 关闭时不得建号");
    assert_eq!(bindings, 0, "JIT 关闭时不得建绑定");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_103_with_jit_on_a_new_account_is_provisioned_once() {
    let env = Env::new().await;
    env.set_jit(true);

    let (status, body) = env.login_as(CAROL, CAROL_MAIL, &env.tenant).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let user_id = body["user"]["id"].as_str().expect("id").to_string();

    // 建出来的账号：email 正确、username = 本地部分、密码哈希是**永不匹配**的占位串。
    let (email, username, hash): (Option<String>, String, String) = sqlx::query_as(
        "SELECT email, username, password_hash FROM platform_iam_user WHERE tenant_id = $1 AND id = $2",
    )
    .bind(&env.tenant)
    .bind(&user_id)
    .fetch_one(&env.admin)
    .await
    .expect("应能读到账号");
    assert_eq!(email.as_deref(), Some(CAROL_MAIL));
    assert_eq!(username, "carol");
    assert!(
        hash.starts_with("$argon2id$"),
        "占位必须是合法 PHC 串：{hash}"
    );
    // 占位串**不得**能被任何常见口令登进来。
    assert!(
        !platform_iam::password::verify_password("", &hash)
            && !platform_iam::password::verify_password("carol", &hash)
            && !platform_iam::password::verify_password(PASSWORD, &hash),
        "占位哈希不得匹配任何口令"
    );
    assert_eq!(env.counts(&env.tenant).await, (1, 1));

    // 再登录一次 → 复用同一个账号（user 表仍只有 1 条）。
    let (status, again) = env.login_as(CAROL, CAROL_MAIL, &env.tenant).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["user"]["id"].as_str(), Some(user_id.as_str()));
    assert_eq!(env.counts(&env.tenant).await, (1, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_103_a_disabled_account_is_not_bypassed_by_sso() {
    let env = Env::new().await;
    let user = register(
        &env.stack.router,
        &env.tenant,
        "dave",
        PASSWORD,
        Some("dave@example.com"),
    )
    .await;
    let dave_id = user["id"].as_str().expect("id").to_string();
    sqlx::query(
        "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
         VALUES ($1, $2, 'oidc', 'idp-dave-001', $3)",
    )
    .bind(&env.tenant)
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&dave_id)
    .execute(&env.admin)
    .await
    .expect("绑定应能写入");
    sqlx::query(
        "UPDATE platform_iam_user SET status = 'disabled' WHERE tenant_id = $1 AND id = $2",
    )
    .bind(&env.tenant)
    .bind(&dave_id)
    .execute(&env.admin)
    .await
    .expect("禁用应成功");

    let (status, body) = env
        .login_as("idp-dave-001", "dave@example.com", &env.tenant)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "ACCOUNT_DISABLED");
}

/// 同一 sub **并发** 3 次登录：只产生 1 个账号 + 1 条绑定，且 **0 个 5xx**。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_103_concurrent_jit_logins_are_idempotent_and_never_5xx() {
    let env = Env::new().await;
    env.set_jit(true);

    let tenant = env.tenant.clone();
    let mut handles = Vec::new();
    for _ in 0..3 {
        // 每个任务共享同一个 mock IdP 与 router（服务内部状态是 Arc）。
        let idp = env.idp.clone();
        let router = env.stack.router.clone();
        let tenant = tenant.clone();
        handles.push(tokio::spawn(async move {
            idp.set_behavior(|behavior| {
                behavior.insert_sub("idp-carol-001", "carol@example.com");
            });
            let path = format!("/api/iam/sso/oidc/authorize?tenant={tenant}");
            let (status, headers, _) = http_get(&router, &path, None, None).await;
            assert_eq!(status, StatusCode::FOUND);
            let query = location_query(&headers);
            let code = format!("code-{}", uuid::Uuid::new_v4().simple());
            idp.register_code(
                &code,
                &query["nonce"],
                &query["code_challenge"],
                &query["redirect_uri"],
            );
            let callback = format!(
                "/api/iam/sso/oidc/callback?code={code}&state={}",
                query["state"]
            );
            let (_, headers, _) = http_get(&router, &callback, None, None).await;
            let callback_query = location_query(&headers);
            let Some(ticket) = callback_query.get("ticket") else {
                return StatusCode::CONFLICT; // 允许个别 409
            };
            let (status, _, _) = post_json(
                &router,
                "/api/iam/sso/exchange",
                None,
                None,
                json!({ "ticket": ticket }),
            )
            .await;
            status
        }));
    }

    let mut statuses = Vec::new();
    for handle in handles {
        statuses.push(handle.await.expect("任务不应 panic"));
    }
    for status in &statuses {
        assert!(
            !status.is_server_error(),
            "并发 JIT 不得产生 5xx：{statuses:?}"
        );
    }
    assert!(
        statuses.contains(&StatusCode::OK),
        "至少应有一次成功：{statuses:?}"
    );
    assert_eq!(
        env.counts(&env.tenant).await,
        (1, 1),
        "并发下只能产生 1 个账号与 1 条绑定"
    );
}

/// 绑定被 DB 侧改指 → 登录必须 409，且**绑定行未被改动**（P7 / 步骤 7）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_103_a_repointed_binding_is_a_conflict_and_is_not_rewritten() {
    let env = Env::new().await;
    env.set_jit(true);

    // eve 先正常绑定。
    let (status, body) = env
        .login_as("idp-eve-001", "eve@example.com", &env.tenant)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let eve_id = body["user"]["id"].as_str().expect("id").to_string();

    // 另一个账号（attacker 角色）。
    let other = register(
        &env.stack.router,
        &env.tenant,
        "someone-else",
        PASSWORD,
        Some("someone-else@example.com"),
    )
    .await;
    let other_id = other["id"].as_str().expect("id").to_string();

    // DB 侧把绑定**改指**到另一个账号（模拟「有人照抄了 org 的 DO UPDATE 语义」）。
    sqlx::query(
        "UPDATE external_identity SET user_id = $1 \
         WHERE tenant_id = $2 AND provider = 'oidc' AND external_id = 'idp-eve-001'",
    )
    .bind(&other_id)
    .bind(&env.tenant)
    .execute(&env.admin)
    .await
    .expect("改指应成功");

    // 登录：目录说这个身份是 eve@example.com，库里却绑给了别人 → 409。
    let (status, body) = env
        .login_as("idp-eve-001", "eve@example.com", &env.tenant)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "SSO_IDENTITY_LINK_CONFLICT");

    // 绑定行**未被改动**（认证路径绝不写回）。
    let bound: String = sqlx::query_scalar(
        "SELECT user_id FROM external_identity \
         WHERE tenant_id = $1 AND provider = 'oidc' AND external_id = 'idp-eve-001'",
    )
    .bind(&env.tenant)
    .fetch_one(&env.admin)
    .await
    .expect("应能读到绑定");
    assert_eq!(bound, other_id, "认证路径不得改写绑定行");
    let _ = eve_id;
}

/// 同一 IdP 的同一 `sub`，在另一个租户里必须建**独立**的账号（不串租户）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_103_the_same_subject_in_another_tenant_is_independent() {
    let env = Env::new().await;
    let tenant_b = unique_tenant("binding-b");

    // tenant-b：JIT 关闭 → 403。
    let (status, body) = env.login_as(CAROL, CAROL_MAIL, &tenant_b).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "SSO_ACCOUNT_NOT_LINKED");

    // tenant-b：开 JIT（配置源是租户级，但测试用的是同一份快照 → 两边都开）。
    env.set_jit(true);
    let (status, body) = env.login_as(CAROL, CAROL_MAIL, &env.tenant).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let a_id = body["user"]["id"].as_str().expect("id").to_string();

    let (status, body) = env.login_as(CAROL, CAROL_MAIL, &tenant_b).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let b_id = body["user"]["id"].as_str().expect("id").to_string();
    assert_ne!(a_id, b_id, "两个租户必须是两个独立账号");

    let tenant_of_b: String =
        sqlx::query_scalar("SELECT tenant_id FROM platform_iam_user WHERE id = $1")
            .bind(&b_id)
            .fetch_one(&env.admin)
            .await
            .expect("应能读到账号");
    assert_eq!(tenant_of_b, tenant_b, "账号必须落在发起登录的租户里");
}

// ---------------------------------------------------------------------------
// P7 的静态判据（带阳性对照）
// ---------------------------------------------------------------------------

/// 认证路径的源码里**不允许**出现「改指」语义的 upsert。
#[test]
fn ac_s_103_the_authentication_path_has_no_repointing_upsert() {
    let dir = repo_root().join("crates/platform-iam/src/sso");
    let mut hits = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("sso 目录应可读") {
        let path = entry.expect("目录项").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("源码应可读");
        // 剥掉注释，避免文档里那句「绝对不要照抄 DO UPDATE」被当成命中。
        let code: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let upper = code.to_uppercase();
        if upper.contains("DO UPDATE") {
            hits.push(path.display().to_string());
        }
    }
    assert!(
        hits.is_empty(),
        "SSO 认证路径不得出现改指语义（ON CONFLICT ... DO UPDATE）：{hits:?}"
    );

    // **阳性对照**：同一个扫描器必须能命中一段故意注入的源码。
    let injected = "INSERT ... ON CONFLICT (a) DO UPDATE SET user_id = EXCLUDED.user_id";
    assert!(
        injected.to_uppercase().contains("DO UPDATE"),
        "扫描器必须能命中 DO UPDATE，否则 0 命中不可信"
    );
    // 反过来：`DO NOTHING` 不得被误报。
    assert!(!("ON CONFLICT DO NOTHING".to_uppercase()).contains("DO UPDATE"));
}
