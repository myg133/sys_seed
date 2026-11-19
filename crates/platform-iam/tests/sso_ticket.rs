//! P4 / AC-S-100 步骤 8：一次性 `loginTicket` —— 一次性、短命、绑租户，
//! **且 token 不进 URL / 不进数据库**。

mod sso_support;

use std::str::FromStr;

use axum::http::StatusCode;
use serde_json::json;

use sso_support::*;

const FRONTEND_BASE: &str = "http://app.example.com";
const CALLBACK_URI: &str = "http://app.example.com/api/iam/sso/oidc/callback";

struct Env {
    stack: Stack,
    idp: MockIdp,
    tenant: String,
    admin: sqlx::PgPool,
}

impl Env {
    async fn new() -> Self {
        let app_url = setup_database().await;
        let stack = build_stack(&app_url).await;
        let idp = MockIdp::start_http().await;
        let tenant = unique_tenant("ticket");
        install_config(
            &stack.service,
            platform_iam::sso::config::SsoConfig {
                oidc: oidc_settings(&idp.issuer, CALLBACK_URI),
                ..Default::default()
            },
        );
        stack
            .service
            .set_sso_http_client(std::sync::Arc::new(
                platform_iam::sso::oidc::OidcHttpClient::new().expect("http 客户端"),
            ));
        stack.service.set_sso_frontend_base_url(FRONTEND_BASE);
        stack.service.set_sso_oidc_client_secret("SECRET-CANARY-oidc");

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
            admin,
        }
    }

    /// 走一次回调，返回 302 的完整 Location。
    async fn callback(&self) -> String {
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
        let (status, headers, bytes) = http_get(&self.stack.router, &callback, None, None).await;
        assert_eq!(status, StatusCode::FOUND, "{}", String::from_utf8_lossy(&bytes));
        headers
            .get(axum::http::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("302 必须带 Location")
            .to_string()
    }

    async fn exchange(&self, ticket: &str, tenant: Option<&str>) -> (StatusCode, serde_json::Value) {
        let (status, _, bytes) = post_json(
            &self.stack.router,
            "/api/iam/sso/exchange",
            tenant,
            None,
            json!({ "ticket": ticket }),
        )
        .await;
        (status, json_of(&bytes))
    }
}

fn ticket_of(location: &str) -> String {
    let query = location.split_once('?').map(|(_, query)| query).unwrap_or("");
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == "ticket")
        .map(|(_, value)| value.to_string())
        .unwrap_or_else(|| panic!("Location 里没有 ticket：{location}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p4_the_callback_location_carries_only_a_ticket() {
    let env = Env::new().await;
    let location = env.callback().await;
    assert!(
        location.starts_with("http://app.example.com/sso/callback?"),
        "必须落在前端回调页：{location}"
    );
    assert!(location.contains("ticket="), "{location}");
    for forbidden in ["accessToken", "access_token", "refreshToken", "refresh_token", "id_token"] {
        assert!(
            !location.contains(forbidden),
            "token 不得进 URL（§8）：{location}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p4_a_ticket_can_only_be_exchanged_once() {
    let env = Env::new().await;
    let ticket = ticket_of(&env.callback().await);

    let (status, body) = env.exchange(&ticket, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["accessToken"].is_string());

    // 第二次 → 401（AC-S-100 步骤 8）。
    let (status, body) = env.exchange(&ticket, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["code"], "SSO_TICKET_INVALID");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p4_an_expired_ticket_is_rejected() {
    let env = Env::new().await;
    let ticket = ticket_of(&env.callback().await);

    sqlx::query(
        "UPDATE platform_iam_sso_ticket SET expires_at = now() - interval '1 second' WHERE ticket = $1",
    )
    .bind(&ticket)
    .execute(&env.admin)
    .await
    .expect("制造过期应成功");

    let (status, body) = env.exchange(&ticket, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["code"], "SSO_TICKET_INVALID");
}

/// 票据绑租户：用别人的租户头换票 → 401（不泄露票据是否存在）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p4_a_ticket_is_bound_to_its_tenant() {
    let env = Env::new().await;
    let ticket = ticket_of(&env.callback().await);
    let other = unique_tenant("ticket-other");

    let (status, body) = env.exchange(&ticket, Some(&other)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["code"], "SSO_TICKET_INVALID");
}

/// 票据表**只存 `session_id`**，不存任何 token。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p4_the_ticket_table_stores_no_tokens() {
    let env = Env::new().await;
    let ticket = ticket_of(&env.callback().await);

    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'platform_iam_sso_ticket' ORDER BY 1",
    )
    .fetch_all(&env.admin)
    .await
    .expect("应能读到列名");
    assert_eq!(
        columns,
        vec![
            "consumed_at".to_string(),
            "created_at".to_string(),
            "expires_at".to_string(),
            "session_id".to_string(),
            "tenant_id".to_string(),
            "ticket".to_string(),
            "user_id".to_string(),
        ],
        "票据表只应有这 7 列（没有 token 列）"
    );

    // 行里只有 session_id；会话表里才有令牌（且只存 refresh 的**摘要**）。
    let session_id: String =
        sqlx::query_scalar("SELECT session_id FROM platform_iam_sso_ticket WHERE ticket = $1")
            .bind(&ticket)
            .fetch_one(&env.admin)
            .await
            .expect("应能读到票据");
    assert!(!session_id.is_empty());

    let hashes: Vec<String> = sqlx::query_scalar(
        "SELECT token_hash FROM platform_iam_refresh_token WHERE tenant_id = $1 AND session_id = $2",
    )
    .bind(&env.tenant)
    .bind(&session_id)
    .fetch_all(&env.admin)
    .await
    .expect("应能读到 refresh 记录");
    assert!(
        hashes.is_empty(),
        "回调阶段只建会话、不签 refresh token（token 只在 exchange 的响应体里出现）"
    );
}

// ---------------------------------------------------------------------------
// 迁移可逆性（design §2.2：down 只 `DROP COLUMN`，**不删既有表**）
// ---------------------------------------------------------------------------

/// up / down / up 三次都成功，且 down **不删** `platform_iam_session`。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sso_migration_is_reversible() {
    let _ = Env::new().await; // 确保迁移已在 base_test 上应用过
    let root = repo_root();
    let up = std::fs::read_to_string(root.join("migrations/20261120000100_iam_sso.up.sql"))
        .expect("up 迁移应可读");
    let down = std::fs::read_to_string(root.join("migrations/20261120000100_iam_sso.down.sql"))
        .expect("down 迁移应可读");

    // 静态判据：down 不得删既有表。
    assert!(
        !down.contains("DROP TABLE IF EXISTS platform_iam_session")
            && !down.contains("DROP TABLE platform_iam_session"),
        "down 不得删既有表 platform_iam_session"
    );
    assert!(
        down.contains("DROP COLUMN IF EXISTS auth_provider"),
        "down 必须可逆地移除新增列"
    );
    assert!(up.contains("GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_oidc_state"));
    assert!(up.contains("GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_sso_ticket"));

    // 动态判据：在**独立数据库**里跑 up → down → up（不碰 base_test 的 public schema）。
    let admin_url = std::env::var("DATABASE_URL_TEST").expect("DATABASE_URL_TEST");
    let suffix = uuid::Uuid::new_v4().to_string().replace('-', "");
    let name = format!("sso_mig_{}", &suffix[..12]);
    let admin = sqlx::PgPool::connect(&admin_url).await.expect("管理员连接");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("应能建测试库");

    let probe = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(
            sqlx::postgres::PgConnectOptions::from_str(&admin_url)
                .expect("连接串应可解析")
                .database(&name),
        )
        .await
        .expect("应能连测试库");

    // 先造出既有表（模拟 REQ-007 的形态）。
    sqlx::raw_sql(
        "CREATE TABLE platform_iam_session (\
           id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, user_id TEXT NOT NULL, \
           device_label TEXT NOT NULL DEFAULT '', ip TEXT NOT NULL DEFAULT '', \
           user_agent TEXT NOT NULL DEFAULT '', family_id TEXT NOT NULL, \
           created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
           last_used_at TIMESTAMPTZ NOT NULL DEFAULT now(), revoked_at TIMESTAMPTZ); \
         INSERT INTO platform_iam_session (id, tenant_id, user_id, family_id) \
           VALUES ('s1','t1','u1','f1');",
    )
    .execute(&probe)
    .await
    .expect("既有表应能建");

    sqlx::raw_sql(&up).execute(&probe).await.expect("up 应成功");
    let column: Option<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'platform_iam_session' AND column_name = 'auth_provider'",
    )
    .fetch_optional(&probe)
    .await
    .expect("查列");
    assert_eq!(column.as_deref(), Some("auth_provider"));
    // 既有行的默认值是 local（不是 NULL）。
    let provider: String =
        sqlx::query_scalar("SELECT auth_provider FROM platform_iam_session WHERE id = 's1'")
            .fetch_one(&probe)
            .await
            .expect("既有行应还在");
    assert_eq!(provider, "local");

    sqlx::raw_sql(&down).execute(&probe).await.expect("down 应成功");
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name IN \
           ('platform_iam_oidc_state','platform_iam_sso_ticket')",
    )
    .fetch_one(&probe)
    .await
    .expect("查表");
    assert_eq!(tables, 0, "down 应删掉本轮新建的两张表");
    let session_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_iam_session")
        .fetch_one(&probe)
        .await
        .expect("既有表必须还在");
    assert_eq!(session_rows, 1, "down 不得丢既有数据");

    sqlx::raw_sql(&up).execute(&probe).await.expect("再次 up 应成功");

    probe.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("应能删测试库");
}
