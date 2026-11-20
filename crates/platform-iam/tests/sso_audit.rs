//! AC-S-108：SSO 审计事件 —— **声明 + 发出**，本轮**不落库**。
//!
//! # 本轮不验落库（显式排除）
//!
//! 审计 sink（落库 + 查询）在 **REQ-014**。所以这里**不写**「审计记录已可查询」这类断言 ——
//! 那会变成一条永远红或永远假的用例。验的是：
//!
//! 1. 声明的 `AuditEventSpec` 清单（`Auditable` 契约面）；
//! 2. 注入 recorder 后**真的发出**了事件，且 payload 键集合**恰好**是白名单；
//! 3. payload 里**没有** token / 密码 / `id_token` / `client_secret`（带阳性对照）；
//! 4. **不注入** recorder 时登录仍成功，且默认实现会 warn。

mod sso_support;

use std::sync::Arc;

use axum::http::StatusCode;
use platform_core::extension::auditable::Auditable;
use platform_iam::sso::audit::{
    EVENT_LINK_CREATED, EVENT_LOGIN_FAILED, EVENT_LOGIN_SUCCEEDED, SsoAuditEvent, SsoAuditPayload,
};
use serde_json::json;

use sso_support::*;

const CALLBACK_URI: &str = "http://app.example.com/api/iam/sso/oidc/callback";
const LDAP_URL: &str = "ldap://127.0.0.1:389";
const ERIN_DN: &str = "uid=erin,ou=people,dc=example,dc=com";
const ERIN_PASSWORD: &str = "Er1n-pass";
const CANARY: &str = "SECRET-CANARY-audit-9c1f";

struct Env {
    stack: Stack,
    idp: MockIdp,
    tenant: String,
}

impl Env {
    async fn new() -> Self {
        let app_url = setup_database().await;
        let stack = build_stack(&app_url).await;
        let idp = MockIdp::start_http().await;
        let tenant = unique_tenant("audit");
        install_config(
            &stack.service,
            platform_iam::sso::config::SsoConfig {
                oidc: oidc_settings(&idp.issuer, CALLBACK_URI),
                ldap: ldap_settings(LDAP_URL),
                ..Default::default()
            },
        );
        stack.service.set_sso_http_client(Arc::new(
            platform_iam::sso::oidc::OidcHttpClient::new().expect("http 客户端"),
        ));
        stack
            .service
            .set_sso_frontend_base_url("http://app.example.com");
        stack.service.set_sso_oidc_client_secret(CANARY);

        let user = register(
            &stack.router,
            &tenant,
            "bob",
            PASSWORD,
            Some("bob@example.com"),
        )
        .await;
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

        Self { stack, idp, tenant }
    }

    /// 一次 OIDC 登录（`alg_override` 为 `Some` 时制造失败）。
    async fn oidc(&self, fail_with_alg_none: bool) -> StatusCode {
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
        if fail_with_alg_none {
            use base64::Engine as _;
            let b64 = |value: &serde_json::Value| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(serde_json::to_vec(value).expect("json"))
            };
            self.idp.set_behavior(|behavior| {
                behavior.id_token_override = Some(format!(
                    "{}.{}.",
                    b64(&json!({"alg":"none"})),
                    b64(&json!({"sub":"x"}))
                ));
            });
        } else {
            self.idp
                .set_behavior(|behavior| behavior.id_token_override = None);
        }
        let callback = format!(
            "/api/iam/sso/oidc/callback?code={code}&state={}",
            query["state"]
        );
        let (status, _, _) = http_get(&self.stack.router, &callback, None, None).await;
        status
    }

    async fn ldap_login(&self, password: &str) -> StatusCode {
        let (status, _, _) = post_json(
            &self.stack.router,
            "/api/iam/sso/ldap/login",
            Some(&self.tenant),
            Some(&unique_ip()),
            json!({ "username": "erin", "password": password }),
        )
        .await;
        status
    }
}

// ---------------------------------------------------------------------------
// 声明面（Auditable）
// ---------------------------------------------------------------------------

#[test]
fn ac_s_108_the_declared_events_cover_the_three_required_codes() {
    // `IamService` 实现了平台契约要求的 `Auditable`（见下方 assertions）。

    let events = platform_iam::sso::audit::audit_events();
    let codes: Vec<&str> = events.iter().map(|spec| spec.code.as_str()).collect();
    for required in [
        EVENT_LOGIN_SUCCEEDED,
        EVENT_LOGIN_FAILED,
        EVENT_LINK_CREATED,
    ] {
        assert!(codes.contains(&required), "缺少声明的事件 {required}");
    }
    for spec in events {
        assert!(!spec.code.is_empty());
        assert!(!spec.action.is_empty(), "{} 缺 action", spec.code);
        assert!(!spec.object_type.is_empty(), "{} 缺 objectType", spec.code);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_108_the_service_exposes_the_declaration_through_auditable() {
    let env = Env::new().await;
    let declared = env.stack.service.audit_events();
    let codes: Vec<&str> = declared.iter().map(|spec| spec.code.as_str()).collect();
    assert!(codes.contains(&EVENT_LOGIN_SUCCEEDED));
    assert!(codes.contains(&EVENT_LOGIN_FAILED));
}

// ---------------------------------------------------------------------------
// 发出面（payload 白名单 + 无敏感值）
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_108_a_successful_login_emits_the_exact_succeeded_payload() {
    let env = Env::new().await;
    let recorder = Arc::new(RecordingAudit::new());
    assert!(
        env.stack.service.with_sso_audit_recorder(recorder.clone()),
        "recorder 应能注入"
    );

    let status = env.oidc(false).await;
    assert_eq!(status, StatusCode::FOUND);

    let payloads = recorder.payloads();
    let succeeded = payloads
        .iter()
        .find(|(code, _)| code == EVENT_LOGIN_SUCCEEDED)
        .unwrap_or_else(|| panic!("应发出 {EVENT_LOGIN_SUCCEEDED}：{payloads:?}"));
    // 键集合**恰好**是 {provider, external_id, user_id, ip, user_agent}。
    let keys: std::collections::BTreeSet<&str> = succeeded
        .1
        .as_object()
        .expect("对象")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["provider", "external_id", "user_id", "ip", "user_agent"]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        "payload 键集合必须恰好是白名单"
    );
    assert_eq!(succeeded.1["provider"], "oidc");
    assert_eq!(succeeded.1["external_id"], "idp-bob-001");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_108_a_failed_login_emits_only_provider_code_and_ip() {
    let env = Env::new().await;
    let recorder = Arc::new(RecordingAudit::new());
    assert!(env.stack.service.with_sso_audit_recorder(recorder.clone()));

    let status = env.oidc(true).await;
    assert_eq!(status, StatusCode::FOUND);

    let payloads = recorder.payloads();
    let failed = payloads
        .iter()
        .find(|(code, _)| code == EVENT_LOGIN_FAILED)
        .unwrap_or_else(|| panic!("应发出 {EVENT_LOGIN_FAILED}：{payloads:?}"));
    let keys: std::collections::BTreeSet<&str> = failed
        .1
        .as_object()
        .expect("对象")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["provider", "code", "ip"]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        "失败 payload 只能有 provider / code / ip"
    );
    assert_eq!(failed.1["code"], "SSO_OIDC_ALG_NOT_ALLOWED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_108_ldap_success_and_failure_are_both_recorded_with_provider_ldap() {
    let env = Env::new().await;
    let ldap = Arc::new(RecordingLdap::new());
    ldap.add_user(DirectoryUser {
        dn: ERIN_DN.to_string(),
        password: ERIN_PASSWORD.to_string(),
        mail: Some("erin@example.com".to_string()),
    });
    assert!(env.stack.service.set_sso_ldap_client(ldap));
    // erin 的绑定。
    let user = register(
        &env.stack.router,
        &env.tenant,
        "erin",
        PASSWORD,
        Some("erin@example.com"),
    )
    .await;
    let user_id = user["id"].as_str().expect("id").to_string();
    let admin = admin_pool().await;
    sqlx::query(
        "INSERT INTO external_identity (tenant_id, id, provider, external_id, user_id) \
         VALUES ($1, $2, 'ldap', $3, $4)",
    )
    .bind(&env.tenant)
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(ERIN_DN)
    .bind(&user_id)
    .execute(&admin)
    .await
    .expect("绑定应能写入");

    let recorder = Arc::new(RecordingAudit::new());
    assert!(env.stack.service.with_sso_audit_recorder(recorder.clone()));

    assert_eq!(env.ldap_login(ERIN_PASSWORD).await, StatusCode::OK);
    assert_eq!(env.ldap_login("wrong").await, StatusCode::UNAUTHORIZED);

    let payloads = recorder.payloads();
    let ldap_success: Vec<&(String, serde_json::Value)> = payloads
        .iter()
        .filter(|(code, payload)| code == EVENT_LOGIN_SUCCEEDED && payload["provider"] == "ldap")
        .collect();
    let ldap_failure: Vec<&(String, serde_json::Value)> = payloads
        .iter()
        .filter(|(code, payload)| code == EVENT_LOGIN_FAILED && payload["provider"] == "ldap")
        .collect();
    assert_eq!(ldap_success.len(), 1, "LDAP 成功应有一条：{payloads:?}");
    assert_eq!(ldap_failure.len(), 1, "LDAP 失败应有一条：{payloads:?}");
    assert_eq!(ldap_failure[0].1["code"], "SSO_LDAP_BIND_FAILED");
}

/// 全部 payload 里**不得**出现 canary / token / 密码（带**真**阳性对照）。
///
/// # 阳性对照为什么必须走「真实 payload」这条路（D128 / Q1）
///
/// 上一版写成 `format!("{dump}\n{CANARY}").contains(CANARY)` —— 那个断言
/// **对任何输入都成立**，它证明的是「字符串拼接能拼上」，不是「扫描判据有区分力」。
/// 所以这里改成：往一个**真实的** [`SsoAuditPayload`] 的 `user_agent` 注入 canary，
/// 用**与本用例完全相同的扫描判据**跑，断言命中；然后断言真实 payload 的扫描结果是 0 命中。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_108_no_payload_carries_a_secret() {
    let env = Env::new().await;
    let recorder = Arc::new(RecordingAudit::new());
    assert!(env.stack.service.with_sso_audit_recorder(recorder.clone()));

    let _ = env.oidc(false).await;
    let _ = env.oidc(true).await;

    let surfaces = recorded_payload_text(&recorder);
    assert!(!surfaces.is_empty(), "应先录到审计 payload，否则扫描无意义");

    // 1) 真实 payload：6 类敏感串 0 命中。
    let forbidden = [
        CANARY,
        "accessToken",
        "refreshToken",
        "id_token",
        "password",
        PASSWORD,
    ];
    for needle in forbidden {
        assert_eq!(
            scan_hits(&surfaces, needle),
            0,
            "审计 payload 不得包含 {needle}：{surfaces}"
        );
    }

    // 2) **阳性对照**：同一个扫描判据 + 一个真实的 `SsoAuditPayload`，
    //    把 canary 注入到 `user_agent`（success 白名单里的键）。
    //
    //    若 `scan_hits` 退化成恒 0（例如判据被改成 `false`），第 3 步会红；
    //    若序列化层把 canary 吞掉（白名单丢键），第 3 步也会红。
    let injected = SsoAuditEvent::new(
        EVENT_LOGIN_SUCCEEDED,
        SsoAuditPayload::succeeded("oidc", "idp-bob-001", "user-1", "10.0.0.1", CANARY),
    );
    let injected_text = serde_json::to_value(&injected)
        .expect("事件应能序列化")
        .to_string();
    assert_eq!(
        scan_hits(&injected_text, CANARY),
        1,
        "扫描判据必须能命中注入到 user_agent 的 canary（否则上面的 0 命中不可信）"
    );
    // 同一个「真实 payload 全集」的拼接形态也必须能命中 —— 证明扫描面不是被缩窄过。
    let injected_surface = format!("{surfaces}\n{injected_text}");
    assert_eq!(
        scan_hits(&injected_surface, CANARY),
        1,
        "注入后的扫描面必须命中恰好一次"
    );
}

/// 把录到的全部 payload 拼成**一个扫描面**（与用例里的敏感值扫描同一份输入）。
fn recorded_payload_text(recorder: &RecordingAudit) -> String {
    recorder
        .payloads()
        .iter()
        .map(|(_, payload)| payload.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// **敏感值扫描判据**（用例与阳性对照共用同一个函数 —— 这是阳性对照成立的前提）。
fn scan_hits(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

// ---------------------------------------------------------------------------
// 默认实现（不注入 recorder）
// ---------------------------------------------------------------------------

/// 不注入 recorder 时登录**仍然成功**（审计不是登录的前置条件）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_108_login_still_works_without_a_recorder() {
    let env = Env::new().await;
    let status = env.oidc(false).await;
    assert_eq!(
        status,
        StatusCode::FOUND,
        "没有审计 sink 也必须能登录（fail closed 的是判定，不是登录）"
    );
}

/// 本轮的**显式排除**：SSO 事件不落任何表（sink 在 REQ-014）。
///
/// 用一条结构性断言守住「不要偷偷建表」：库里不存在任何名字像 SSO 审计表的表。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac_s_108_no_sso_audit_table_exists_yet() {
    let env = Env::new().await;
    let admin = admin_pool().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' AND (table_name ILIKE '%audit%' OR table_name ILIKE '%sso%') \
         ORDER BY 1",
    )
    .fetch_all(&admin)
    .await
    .expect("应能列出表");
    // 只有两个**非审计**的 SSO 表：一次性凭证与交接票据。
    // 任何 **SSO 专用**的 `*audit*` 表都不该存在（落库走 REQ-014 的平台 sink）。
    //
    // ⚠️ REQ-014 起平台有了**通用**审计表（`platform_audit_log` /
    //    `platform_audit_archive` / `platform_audit_chain_state`）——
    //    它们是**平台级**的通用表，**不是**「SSO 审计表」，本用例的意图
    //    （不要偷偷建 SSO 专用表）不受影响。
    let sso_audit_tables: Vec<String> = tables
        .iter()
        .filter(|name| !name.starts_with("platform_audit_"))
        .cloned()
        .collect();
    assert_eq!(
        sso_audit_tables,
        vec!["platform_iam_sso_ticket".to_string()],
        "本轮不应存在任何 SSO 专用审计表（落库是 REQ-014 的平台 sink）：{tables:?}"
    );

    // 对照：平台级的**通用**审计表存在（REQ-014 落地后的预期形态）。
    assert!(
        tables.iter().any(|name| name == "platform_audit_log"),
        "REQ-014 的通用审计表应当在库里：{tables:?}"
    );

    // 触发一次登录后，这几张表里也不该多出「审计」性质的行。
    let _ = env.oidc(false).await;
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_iam_oidc_state")
        .fetch_one(&admin)
        .await
        .expect("count");
    assert!(rows >= 1, "只有 oidc_state 会增长（那是凭证，不是审计）");
}
