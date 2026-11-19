//! AC-S-106 的**装配层**验证 + AC-S-100/107/109 的真实库端到端（design §8 的
//! `platform-server/tests/sso_real_db.rs`）。
//!
//! # 这一层验的是「装配是否真的接通」
//!
//! 单元/组件测试用的是注入的静态配置源；这里用**真实的 `ConfigService`**：
//!
//! ```text
//! platform_tenant_config  →  ConfigService::read_entries(12 个键)  →  SsoConfig
//! ```
//!
//! 所以它守的是两条只有装配层才会犯的错：
//!
//! 1. `GET /api/config/entries` 必须**按精确键名**读（P1 / S1）——
//!    「全量导出配置」是**不存在**的能力；
//! 2. 12 个键必须在 `manifest.json` 登记，否则读出来 404（D53）。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use platform_core::RequestContext;
use platform_iam::config::{
    Argon2Settings, IamConfig, JwtSettings, LoginSettings, PasswordPolicySettings, TokenSettings,
    TotpSettings,
};
use platform_iam::http::IamState;
use platform_iam::service::IamService;
use platform_iam::sso::config::SsoBoxFuture;
use platform_iam::store::IamStore;
use serde_json::{Value, json};
use tower::ServiceExt;

mod support;

use support::*;

// ---------------------------------------------------------------------------
// 装配：真实 `ConfigService` 作为 SSO 配置源
// ---------------------------------------------------------------------------

/// 与 `platform-server/src/main.rs` 的 `ConfigServiceSsoSource` 等价的实现。
///
/// 放在测试里（而不是复用私有类型）是为了让「按精确键名读」这件事在测试里
/// **可读地**呈现：`read_entries(&ctx, &SSO_CONFIG_KEYS)`。
struct RealConfigSource {
    config: Arc<platform_config::ConfigService>,
}

impl platform_iam::sso::config::SsoConfigSource for RealConfigSource {
    fn read(
        &self,
        ctx: &RequestContext,
    ) -> SsoBoxFuture<
        '_,
        Result<platform_iam::sso::config::SsoConfig, platform_iam::sso::config::SsoConfigError>,
    > {
        let config = self.config.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            let keys: Vec<String> = platform_iam::sso::config::SSO_CONFIG_KEYS
                .iter()
                .map(|key| (*key).to_string())
                .collect();
            let entries = config
                .read_entries(&ctx, &keys)
                .await
                .map_err(|_| platform_iam::sso::config::SsoConfigError)?;
            Ok(map_entries(&entries))
        })
    }
}

fn map_entries(entries: &[platform_core::ResolvedConfig]) -> platform_iam::sso::config::SsoConfig {
    use platform_iam::sso::config::{LdapSettings, OidcSettings, SsoConfig};
    let text = |key: &str| -> Option<String> {
        entries
            .iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.value.as_str())
            .map(str::to_string)
    };
    let flag = |key: &str| -> Option<bool> {
        entries
            .iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.value.as_bool())
    };
    let defaults = SsoConfig::default();
    SsoConfig {
        enforce: flag("sso.enforce").unwrap_or(defaults.enforce),
        jit_provisioning: flag("sso.jitProvisioning").unwrap_or(defaults.jit_provisioning),
        oidc: OidcSettings {
            enabled: flag("sso.oidc.enabled").unwrap_or(defaults.oidc.enabled),
            issuer: text("sso.oidc.issuer").unwrap_or(defaults.oidc.issuer),
            client_id: text("sso.oidc.clientId").unwrap_or(defaults.oidc.client_id),
            scopes: text("sso.oidc.scopes").unwrap_or(defaults.oidc.scopes),
            redirect_uri: text("sso.oidc.redirectUri").unwrap_or(defaults.oidc.redirect_uri),
        },
        ldap: LdapSettings {
            enabled: flag("sso.ldap.enabled").unwrap_or(defaults.ldap.enabled),
            url: text("sso.ldap.url").unwrap_or(defaults.ldap.url),
            base_dn: text("sso.ldap.baseDn").unwrap_or(defaults.ldap.base_dn),
            user_search_filter: text("sso.ldap.userSearchFilter")
                .unwrap_or(defaults.ldap.user_search_filter),
            mail_attribute: text("sso.ldap.mailAttribute")
                .unwrap_or(defaults.ldap.mail_attribute),
        },
    }
}

fn test_config() -> IamConfig {
    let pem = platform_iam::jwt::generate_key_pair().expect("测试密钥");
    IamConfig {
        jwt: JwtSettings {
            issuer: "base".to_string(),
            audience: "base-api".to_string(),
            private_key_pem: pem,
            kid: "kid-test".to_string(),
            previous_private_key_pem: None,
            previous_kid: None,
        },
        tokens: TokenSettings {
            access_ttl: std::time::Duration::from_secs(900),
            refresh_ttl: std::time::Duration::from_secs(30 * 24 * 3600),
            reset_ttl: std::time::Duration::from_secs(3600),
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
            ip_window: std::time::Duration::from_secs(300),
            account_failure_limit: 5,
            lockout_duration: std::time::Duration::from_secs(900),
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

/// 用**真实注册表 + 真实 ConfigService** 建一套栈。
async fn build_real_stack(app_url: &str) -> (Arc<IamService>, Arc<platform_config::ConfigService>, Router) {
    let tenant_db = platform_db::TenantDb::connect(app_url, platform_db::DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    // 注册表根是**仓库根**（它会去扫 `crates/*/manifest.json` 与 `modules/*/manifest.json`）。
    let registry = platform_config::ConfigRegistry::from_manifest_root(&repo_root())
        .expect("配置注册表应能加载");
    let config_service = Arc::new(platform_config::ConfigService::new(
        tenant_db.clone(),
        Arc::new(registry),
    ));

    let service = Arc::new(
        IamService::new(IamStore::new(tenant_db), test_config()).expect("服务应当能构造"),
    );
    assert!(
        service.set_sso_config_source(Arc::new(RealConfigSource {
            config: config_service.clone(),
        })),
        "配置源应能注入"
    );
    service.set_sso_frontend_base_url("http://app.example.com");

    let state = Arc::new(IamState {
        service: service.clone(),
    });
    let router = Router::new()
        .nest("/api/iam", platform_iam::http::router(state.clone()))
        .nest("/api/iam", platform_iam::http::sso_router(state.clone()));
    (service, config_service, router)
}

/// 用租户级配置服务写一批键（真实写入路径，不是直接 SQL）。
async fn write_config(
    service: &platform_config::ConfigService,
    ctx: &RequestContext,
    entries: Vec<(&str, Value)>,
) {
    let request = platform_config::WriteRequest {
        scope: "tenant".to_string(),
        entries: entries
            .into_iter()
            .map(|(key, value)| platform_config::WriteEntry {
                key: key.to_string(),
                value,
            })
            .collect(),
    };
    service
        .write_entries(ctx, &request, platform_config::WriteRoute::Scoped)
        .await
        .expect("写入配置应成功");
}

/// AC-S-106 的装配层断言：12 个键按精确键名读出来，`entries` 长度 == 12。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_106_the_twelve_keys_come_back_from_the_real_config_service() {
    let app_url = setup_database().await;
    let (service, config_service, _router) = build_real_stack(&app_url).await;
    let tenant = unique_tenant("realcfg");
    let ctx = RequestContext::new().with_tenant(tenant.clone());

    write_config(
        &config_service,
        &ctx,
        vec![
            ("sso.enforce", json!(false)),
            ("sso.jitProvisioning", json!(true)),
            ("sso.oidc.enabled", json!(true)),
            ("sso.oidc.issuer", json!("https://idp.example.com")),
            ("sso.oidc.clientId", json!("client-1")),
            ("sso.oidc.scopes", json!("openid email profile")),
            (
                "sso.oidc.redirectUri",
                json!("https://app.example.com/api/iam/sso/oidc/callback"),
            ),
            ("sso.ldap.enabled", json!(true)),
            ("sso.ldap.url", json!("ldap://ldap.example.com:389")),
            ("sso.ldap.baseDn", json!("dc=example,dc=com")),
            ("sso.ldap.userSearchFilter", json!("(uid={username})")),
            ("sso.ldap.mailAttribute", json!("mail")),
        ],
    )
    .await;

    // 按精确键名读 —— 长度恰好 12（**不是**「全量导出」）。
    let keys: Vec<String> = platform_iam::sso::config::SSO_CONFIG_KEYS
        .iter()
        .map(|key| (*key).to_string())
        .collect();
    let entries = config_service
        .read_entries(&ctx, &keys)
        .await
        .expect("读取应成功");
    assert_eq!(entries.len(), 12, "entries 长度必须恰好是 12");

    // `source` 只允许 {platform, tenant, default}（D53：scope=tenant 意味着 space 不可写）。
    for entry in &entries {
        let source = entry.source.as_str();
        assert!(
            ["platform", "tenant", "default"].contains(&source),
            "{} 的 source 不得是 {source}",
            entry.key
        );
    }

    // 经 SSO 配置读取路径也拿到同一份值。
    let providers = service.sso_providers(&ctx).await.expect("应能读 provider");
    assert_eq!(providers.len(), 2, "OIDC + LDAP 都应可用");
    let kinds: Vec<&str> = providers.iter().map(|view| view.kind.as_str()).collect();
    assert_eq!(kinds, vec!["oidc", "ldap"]);
}

/// 未登记键写入 404 / 读取 404（D53）—— 这也是「不得为测试新增 manifest 键」的护栏。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p2_an_unregistered_key_cannot_be_written() {
    let app_url = setup_database().await;
    let (_service, config_service, _router) = build_real_stack(&app_url).await;
    let tenant = unique_tenant("unregistered");
    let ctx = RequestContext::new().with_tenant(tenant);

    let request = platform_config::WriteRequest {
        scope: "tenant".to_string(),
        entries: vec![platform_config::WriteEntry {
            key: "sso.testOnlyKey".to_string(),
            value: json!("x"),
        }],
    };
    let outcome = config_service
        .write_entries(&ctx, &request, platform_config::WriteRoute::Scoped)
        .await;
    assert!(outcome.is_err(), "未登记键写入必须失败（D53）");
}

/// `sso.enforce = true` 经真实配置读路径生效：本地登录 403，且 `params.ssoProviders` 是数组。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_021_enforcement_works_through_the_real_config_service() {
    let app_url = setup_database().await;
    let (_service, config_service, router) = build_real_stack(&app_url).await;
    let tenant = unique_tenant("realcfg-enforce");
    let ctx = RequestContext::new().with_tenant(tenant.clone());

    // 先建账号（enforce 还没开）。
    let register_body = json!({
        "username": "alice",
        "password": "correct-horse-battery-staple",
        "email": "alice@example.com",
    });
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/register")
                .header("x-tenant-id", &tenant)
                .header("content-type", "application/json")
                .body(Body::from(register_body.to_string()))
                .expect("请求"),
        )
        .await
        .expect("请求应能发出");
    assert_eq!(response.status(), StatusCode::CREATED);

    write_config(
        &config_service,
        &ctx,
        vec![
            ("sso.enforce", json!(true)),
            ("sso.oidc.enabled", json!(true)),
            ("sso.oidc.issuer", json!("https://idp.example.com")),
            ("sso.oidc.clientId", json!("client-1")),
            ("sso.oidc.scopes", json!("openid email profile")),
            (
                "sso.oidc.redirectUri",
                json!("https://app.example.com/api/iam/sso/oidc/callback"),
            ),
            // AC-S-021 前置：`tenant-a` 配好 **OIDC + LDAP** —— 两个入口都要可用，
            // 否则下面的 `ssoProviders` 只能断言到一个（Q5）。
            ("sso.ldap.enabled", json!(true)),
            ("sso.ldap.url", json!("ldap://ldap.example.com:389")),
            ("sso.ldap.baseDn", json!("dc=example,dc=com")),
            ("sso.ldap.userSearchFilter", json!("(uid={username})")),
            ("sso.ldap.mailAttribute", json!("mail")),
        ],
    )
    .await;

    let before = platform_iam::password::argon2_invocation_count();
    let body = json!({ "username": "alice", "password": "correct-horse-battery-staple" });
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/iam/login")
                .header("x-tenant-id", &tenant)
                .header("x-forwarded-for", unique_ip())
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("请求"),
        )
        .await
        .expect("请求应能发出");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("响应体");
    let problem: Value = serde_json::from_slice(&bytes).expect("应是 JSON");
    assert_eq!(problem["code"], "SSO_REQUIRED");
    // AC-S-021 预期 1（rev 3.3 / Q5）：**可用 provider 列表**，不是「恰好是数组」。
    let kinds: Vec<&str> = problem["params"]["ssoProviders"]
        .as_array()
        .expect("params.ssoProviders 必须是数组")
        .iter()
        .map(|provider| provider["type"].as_str().expect("每个 provider 都要有 type"))
        .collect();
    assert_eq!(
        kinds,
        vec!["oidc", "ldap"],
        "params.ssoProviders 必须列出两个可用入口：{problem}"
    );
    assert_eq!(
        platform_iam::password::argon2_invocation_count(),
        before,
        "强制 SSO 时不得跑 Argon2"
    );
}

/// **AC-S-021 步骤 4 / 预期 4（rev 3.3，D128 / Q6）**：`enforce` 是**租户级** ——
/// 「`tenant-a` 开、`tenant-b` 关」必须**并存**。
///
/// 原用例用 `StaticSsoConfigSource`，它的 `read(&self, _ctx)` **忽略租户上下文**
/// （一份全局快照），只能靠手动 `set(false)` 切回 —— 那验的是「关掉后能登录」，
/// **不是**「两个租户并存时各自生效」。这条用**真实 `ConfigService`** 验：
/// 只给 `tenant-a` 写 `sso.enforce=true`，`tenant-b` 一个键都不写。
///
/// 会红的情形：
/// - 配置读取把 `enforce` 当成**全局**开关 → `tenant-b` 也 403；
/// - 读配置时用错租户（例如忽略 `ctx` 读平台级默认值）→ 两侧对调；
/// - 强制 SSO 判定跑到 Argon2 **之后** → a 侧计数不为 0；
/// - `tenant-b` 因别的原因被拒（配置缺失被当成 fail closed）→ b 侧不是 200。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_021_enforcement_is_tenant_scoped_through_the_real_config_service() {
    let app_url = setup_database().await;
    let (_service, config_service, router) = build_real_stack(&app_url).await;

    let tenant_a = unique_tenant("scoped-a");
    let tenant_b = unique_tenant("scoped-b");
    let ctx_a = RequestContext::new().with_tenant(tenant_a.clone());

    let password = "correct-horse-battery-staple";
    for tenant in [&tenant_a, &tenant_b] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/iam/register")
                    .header("x-tenant-id", tenant.as_str())
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "username": "alice",
                            "password": password,
                            "email": "alice@example.com",
                        })
                        .to_string(),
                    ))
                    .expect("请求"),
            )
            .await
            .expect("请求应能发出");
        assert_eq!(
            response.status(),
            StatusCode::CREATED,
            "{tenant} 的账号应先能建成"
        );
    }

    // **只给 tenant-a 开** enforce；tenant-b 一个键都不写（走 manifest 默认值）。
    write_config(
        &config_service,
        &ctx_a,
        vec![
            ("sso.enforce", json!(true)),
            ("sso.oidc.enabled", json!(true)),
            ("sso.oidc.issuer", json!("https://idp.example.com")),
            ("sso.oidc.clientId", json!("client-1")),
            ("sso.oidc.scopes", json!("openid email profile")),
            (
                "sso.oidc.redirectUri",
                json!("https://app.example.com/api/iam/sso/oidc/callback"),
            ),
            ("sso.ldap.enabled", json!(true)),
            ("sso.ldap.url", json!("ldap://ldap.example.com:389")),
            ("sso.ldap.baseDn", json!("dc=example,dc=com")),
            ("sso.ldap.userSearchFilter", json!("(uid={username})")),
            ("sso.ldap.mailAttribute", json!("mail")),
        ],
    )
    .await;

    let login = |tenant: &str| {
        let router = router.clone();
        let tenant = tenant.to_string();
        let body = json!({ "username": "alice", "password": password }).to_string();
        async move {
            let response = router
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/iam/login")
                        .header("x-tenant-id", &tenant)
                        .header("x-forwarded-for", unique_ip())
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .expect("请求"),
                )
                .await
                .expect("请求应能发出");
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("响应体");
            (status, serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null))
        }
    };

    // a → 403，且**一次 Argon2 都不跑**。
    let before_a = platform_iam::password::argon2_invocation_count();
    let (status_a, body_a) = login(&tenant_a).await;
    let after_a = platform_iam::password::argon2_invocation_count();
    assert_eq!(status_a, StatusCode::FORBIDDEN, "{body_a}");
    assert_eq!(body_a["code"], "SSO_REQUIRED");
    assert_eq!(
        after_a, before_a,
        "tenant-a 开了 enforce：必须在密码校验之前返回（{before_a} → {after_a}）"
    );

    // b → 200，且**真的**跑了 Argon2（对照组，让「计数 == 0」有区分力）。
    let before_b = platform_iam::password::argon2_invocation_count();
    let (status_b, body_b) = login(&tenant_b).await;
    let after_b = platform_iam::password::argon2_invocation_count();
    assert_eq!(
        status_b,
        StatusCode::OK,
        "tenant-b 没开 enforce，本地登录必须仍然可用：{body_b}"
    );
    assert!(
        after_b > before_b,
        "对照组：tenant-b 必须真的跑 Argon2（{before_b} → {after_b}）"
    );

    // 并存性：a 的写入没有影响 b，重打 a 仍然 403。
    let (status_a_again, _) = login(&tenant_a).await;
    assert_eq!(
        status_a_again,
        StatusCode::FORBIDDEN,
        "两个租户的策略必须并存，a 不能因为 b 通而放行"
    );
}
