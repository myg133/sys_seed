//! AC-S-104（LDAP bind 认证：成功 / 失败 / 不可达 / 空密码 / filter 注入）与
//! AC-S-105（LDAP 与本地账号共存）。另含 AC-S-020 步骤 5~7。
//!
//! # 观测面来自 `SsoLdapClient` 的**可计数实现**（P12）
//!
//! `ldap3` 没有服务端实现，所以「空密码必须 0 次 connect / 0 次 bind」只能靠
//! trait 替身计数。见 `sso_support::RecordingLdap`。

mod sso_support;

use axum::http::StatusCode;
use serde_json::json;

use sso_support::*;

const LDAP_URL: &str = "ldap://127.0.0.1:389";
const ERIN_DN: &str = "uid=erin,ou=people,dc=example,dc=com";
const ERIN_PASSWORD: &str = "Er1n-pass";

struct Env {
    stack: Stack,
    ldap: std::sync::Arc<RecordingLdap>,
    tenant: String,
    ip: String,
    admin: sqlx::PgPool,
}

impl Env {
    async fn new() -> Self {
        let app_url = setup_database().await;
        let stack = build_stack(&app_url).await;
        let tenant = unique_tenant("ldap");
        install_config(
            &stack.service,
            platform_iam::sso::config::SsoConfig {
                ldap: ldap_settings(LDAP_URL),
                ..Default::default()
            },
        );
        let ldap = std::sync::Arc::new(RecordingLdap::new());
        ldap.add_user(DirectoryUser {
            dn: ERIN_DN.to_string(),
            password: ERIN_PASSWORD.to_string(),
            mail: Some("erin@example.com".to_string()),
        });
        assert!(stack.service.set_sso_ldap_client(ldap.clone()));

        let admin = admin_pool().await;
        // 前置：`erin` 已绑定（认证路径不做 JIT 时的真实形态）。
        let user = register(&stack.router, &tenant, "erin", PASSWORD, Some("erin@example.com")).await;
        let user_id = user["id"].as_str().expect("id").to_string();
        sqlx::query(
            "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
             VALUES ($1, $2, 'ldap', $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(&tenant)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(ERIN_DN)
        .bind(&user_id)
        .execute(&admin)
        .await
        .expect("绑定应能写入");

        Self {
            stack,
            ldap,
            tenant,
            ip: unique_ip(),
            admin,
        }
    }

    async fn login(&self, username: &str, password: &str) -> (StatusCode, serde_json::Value) {
        let (status, _, bytes) = post_json(
            &self.stack.router,
            "/api/iam/sso/ldap/login",
            Some(&self.tenant),
            Some(&self.ip),
            json!({ "username": username, "password": password }),
        )
        .await;
        (status, json_of(&bytes))
    }
}

// ---------------------------------------------------------------------------
// AC-S-104
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_104_a_valid_bind_returns_a_login_response() {
    let env = Env::new().await;
    let (status, body) = env.login("erin", ERIN_PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["tokenType"], "Bearer");
    assert!(body["expiresIn"].as_u64().is_some_and(|value| value > 0));
    // `permissions` 恒为数组（AC-S-100 预期 4）。这里顺带钉住**元素类型**：
    // 只断言 `is_array()` 连 `[null]` / `[1,2]` 也放行。
    // （「元素非空」的要求属于 AC-S-100 步骤 5，由 `platform-server` 的
    //  `sso_me_permissions_real_db.rs` 用真实 RBAC 验 —— 本用例没注入权限点提供者。）
    assert!(
        body["permissions"]
            .as_array()
            .is_some_and(|items| items.iter().all(serde_json::Value::is_string)),
        "permissions 必须是字符串数组：{body}"
    );

    // 计数期望（P13）：connect=1 / search=1 / bind=2（服务账号 1 + 用户 1）。
    assert_eq!(env.ldap.connects(), 1);
    assert_eq!(env.ldap.searches(), 1);
    assert_eq!(env.ldap.binds(), 2, "bind = 服务账号 1 + 用户 1");

    // bind 只带用户 DN 与密码（服务账号那次是空 DN，见 P13 的实现说明）。
    let calls = env.ldap.bind_calls();
    assert_eq!(calls[1].0, ERIN_DN);
    assert_eq!(calls[1].1, ERIN_PASSWORD);
}

/// 密码错与用户不存在**必须完全一致**（防账号枚举）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_104_a_wrong_password_and_an_unknown_user_are_indistinguishable() {
    let env = Env::new().await;
    let (wrong_status, wrong_body) = env.login("erin", "wrong-password").await;
    assert_eq!(wrong_status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_body["code"], "SSO_LDAP_BIND_FAILED");
    // 不泄露 DN / 目录错误原文。
    let raw = wrong_body.to_string();
    assert!(!raw.contains("uid=erin"), "响应不得泄露 DN：{raw}");
    assert!(!raw.contains("ou=people"), "响应不得泄露 DN：{raw}");

    let (ghost_status, ghost_body) = env.login("ghost", "whatever").await;
    assert_eq!(ghost_status, StatusCode::UNAUTHORIZED);
    // `traceId` 是**每个请求**随机生成的追踪 ID，不是判别信号 —— 其余字段必须完全相同
    // （AC-S-020 步骤 7：同码同 title）。
    for field in ["code", "title", "status", "detail", "type"] {
        assert_eq!(
            ghost_body[field], wrong_body[field],
            "「用户不存在」与「密码错」的 {field} 必须相同"
        );
    }
    let keys = |value: &serde_json::Value| -> std::collections::BTreeSet<String> {
        value.as_object().expect("应是对象").keys().cloned().collect()
    };
    assert_eq!(keys(&ghost_body), keys(&wrong_body));
}

/// 空密码 / 纯空白 **必须在发请求之前**拦截：connect == 0、bind == 0。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_104_an_empty_password_never_touches_the_directory() {
    let env = Env::new().await;
    for username in ["erin", "ghost"] {
        for password in ["", "   "] {
            let (status, body) = env.login(username, password).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(body["code"], "SSO_LDAP_EMPTY_PASSWORD");
            assert_eq!(
                env.ldap.connects(),
                0,
                "空密码必须在 connect 之前拦住（用户名 {username}）"
            );
            assert_eq!(env.ldap.binds(), 0, "空密码不得发出任何 bind");
            assert_eq!(env.ldap.searches(), 0, "空密码不得发出 search");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_104_an_unreachable_directory_is_503() {
    let env = Env::new().await;
    env.ldap.set_unreachable(true);
    let (status, body) = env.login("erin", ERIN_PASSWORD).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "SSO_LDAP_UNREACHABLE");
}

/// 连续失败满 `ip_limit` 次后，第 `ip_limit + 1` 次是 429。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_104_the_ip_rate_limit_applies_to_ldap_failures() {
    let env = Env::new().await;
    let limit = 20u32; // 与 `IamConfig::login.ip_limit` 一致（测试配置）
    for _ in 0..limit {
        let (status, _) = env.login("erin", "wrong-password").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, body) = env.login("erin", "wrong-password").await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "第 {} 次应被限流：{body}",
        limit + 1
    );
    assert_eq!(body["code"], "LOGIN_RATE_LIMITED");
}

/// 三个注入载荷都必须被 RFC 4515 转义 → 不可能「匹配到任意用户」。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_104_filter_injection_payloads_are_escaped() {
    let env = Env::new().await;
    for payload in ["*", ")(uid=*", "admin*"] {
        let (status, body) = env.login(payload, "whatever").await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "注入载荷 {payload} 不得登录成功：{body}"
        );
        assert_eq!(body["code"], "SSO_LDAP_BIND_FAILED");
    }

    // 转义看得到：发送给目录的 filter 里没有裸 `*` / 多余括号。
    let filters: Vec<String> = env
        .ldap
        .search_calls()
        .into_iter()
        .map(|(_, filter, _)| filter)
        .collect();
    assert!(!filters.is_empty());
    for filter in &filters {
        assert!(!filter.contains("uid=*"), "filter 未转义：{filter}");
        assert!(
            filter.matches('(').count() == 1 && filter.matches(')').count() == 1,
            "filter 出现多余括号（注入未消除）：{filter}"
        );
    }
    assert!(
        filters.iter().any(|filter| filter.contains("\\2a")),
        "载荷里的 `*` 必须被转义成 \\2a：{filters:?}"
    );
}

/// AC-S-020 步骤 5：LDAP 不可达 → 503（与「凭据错 401」可区分）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_020_ldap_unreachable_is_503_while_bad_credentials_are_401() {
    let env = Env::new().await;
    let (unreachable_status, unreachable_body) = {
        env.ldap.set_unreachable(true);
        env.login("erin", ERIN_PASSWORD).await
    };
    assert_eq!(unreachable_status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(unreachable_body["code"], "SSO_LDAP_UNREACHABLE");

    env.ldap.set_unreachable(false);
    let (bad_status, bad_body) = env.login("erin", "nope").await;
    assert_eq!(bad_status, StatusCode::UNAUTHORIZED);
    assert_eq!(bad_body["code"], "SSO_LDAP_BIND_FAILED");
    assert_ne!(
        unreachable_body["code"], bad_body["code"],
        "基础设施故障与凭据错必须可区分"
    );
}

// ---------------------------------------------------------------------------
// AC-S-105
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_105_ldap_and_local_login_reach_the_same_account() {
    let env = Env::new().await;

    // 1) 本地密码登录。
    let (status, _, bytes) = post_json(
        &env.stack.router,
        "/api/iam/login",
        Some(&env.tenant),
        Some(&unique_ip()),
        json!({ "username": "erin", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    let local = json_of(&bytes);
    let local_id = local["user"]["id"].as_str().expect("id").to_string();

    // 2) LDAP 密码登录。
    let (status, ldap_login) = env.login("erin", ERIN_PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{ldap_login}");

    // 3) 同一个 user（不产生影子账号）。
    assert_eq!(
        ldap_login["user"]["id"].as_str(),
        Some(local_id.as_str()),
        "两条路径必须落到同一个账号"
    );

    // 4) 本地密码哈希**未被覆盖**（仍是 Argon2id 的 PHC 串）。
    let hash: String = sqlx::query_scalar(
        "SELECT password_hash FROM platform_iam_user WHERE tenant_id = $1 AND id = $2",
    )
    .bind(&env.tenant)
    .bind(&local_id)
    .fetch_one(&env.admin)
    .await
    .expect("应能读到哈希");
    assert!(
        hash.starts_with("$argon2id$"),
        "LDAP 登录不得改写本地密码哈希：{hash}"
    );
}

/// LDAP 侧改密码 → 旧密码失败、新密码成功（密码只在目录里）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_105_changing_the_directory_password_takes_effect_immediately() {
    let env = Env::new().await;
    assert_eq!(env.login("erin", ERIN_PASSWORD).await.0, StatusCode::OK);

    // 目录侧改密码。
    env.ldap.set_password(ERIN_DN, "N3w-Er1n-pass");
    assert_eq!(
        env.login("erin", ERIN_PASSWORD).await.0,
        StatusCode::UNAUTHORIZED,
        "旧密码必须立即失效"
    );
    assert_eq!(env.login("erin", "N3w-Er1n-pass").await.0, StatusCode::OK);
}

/// DN 不变、`mail` 变了 → **仍落到原账号**（P6：`external_id` 优先）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_105_a_changed_mail_attribute_does_not_change_the_account() {
    let env = Env::new().await;
    let first = env.login("erin", ERIN_PASSWORD).await;
    assert_eq!(first.0, StatusCode::OK);
    let user_id = first.1["user"]["id"].as_str().expect("id").to_string();

    // 目录侧把 mail 改成一个**没有任何本地账号**用过的地址。
    env.ldap.set_mail(ERIN_DN, Some("erin.moved@example.com"));
    let second = env.login("erin", ERIN_PASSWORD).await;
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);
    assert_eq!(
        second.1["user"]["id"].as_str(),
        Some(user_id.as_str()),
        "external_id（DN）命中时不得改指"
    );
}

/// `mail` 撞上一个**已被别的外部身份绑定**的本地账号 → 409，不静默接管。
///
/// AC-S-105 步骤 6 的**当前措辞（rev 3.3，D128 裁定）**：
/// 「用**未绑定**的 LDAP 身份 `henry`（`mail = gina@example.com`，该邮箱已被 `gina`
/// 在该 provider 下绑定）→ 登录 → `409`」。
///
/// 原步骤写的是「把已绑定的 `frank` 的 mail 改成 `gina` 的邮箱」，那因
/// **P6「`external_id`（DN）优先」而物理不可达**（DN 未变 → 第一步就命中 `frank`）。
/// D128 已把规格改写为本用例的场景，所以这里不再有「AC 与测试不一致」的注记。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_105_a_mail_colliding_with_another_bound_account_is_409() {
    let env = Env::new().await;

    // `gina` 是一个本地账号，且已绑到**别的** DN。
    let gina = register(
        &env.stack.router,
        &env.tenant,
        "gina",
        PASSWORD,
        Some("gina@example.com"),
    )
    .await;
    let gina_id = gina["id"].as_str().expect("id").to_string();
    sqlx::query(
        "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
         VALUES ($1, $2, 'ldap', 'uid=gina,ou=people,dc=example,dc=com', $3)",
    )
    .bind(&env.tenant)
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&gina_id)
    .execute(&env.admin)
    .await
    .expect("gina 的绑定应能写入");

    // 未绑定的 LDAP 身份 `henry`：DN 没有绑定，mail 却是 gina 已用的地址 ——
    // 按 mail 会匹配到 gina，而 gina 已经绑了别的 DN → 409。
    env.ldap.add_user(DirectoryUser {
        dn: "uid=henry,ou=people,dc=example,dc=com".to_string(),
        password: "H3nry-pass".to_string(),
        mail: Some("gina@example.com".to_string()),
    });
    let (status, body) = env.login("henry", "H3nry-pass").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "SSO_IDENTITY_LINK_CONFLICT");

    // 且**没有**静默把 henry 绑到 gina 头上（绑定行仍是 gina 的旧 DN）。
    let bound: String = sqlx::query_scalar(
        "SELECT external_id FROM external_identity \
         WHERE tenant_id = $1 AND provider = 'ldap' AND user_id = $2",
    )
    .bind(&env.tenant)
    .bind(&gina_id)
    .fetch_one(&env.admin)
    .await
    .expect("gina 的绑定行应仍在");
    assert_eq!(
        bound, "uid=gina,ou=people,dc=example,dc=com",
        "冲突时不得改写既有绑定"
    );
}
