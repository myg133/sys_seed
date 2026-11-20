//! AC-S-127 **步骤 3**：走一次 SSO 登录 → `iam.sso.login.succeeded` / `iam.sso.login.failed`
//! 各 1 条，且 `ip` / `user_agent` **逐字**透传（REQ-014 / 复核 L-02）。
//!
//! # 补的是哪个判据缺口
//!
//! 复核 L-02：`iam.sso.login.succeeded` / `iam.sso.login.failed` 这两个事件码
//! 在 `crates/*/tests/` 里**一条断言都没有**（`grep -rl "iam.sso.login.succeeded" crates/*/tests/*.rs`
//! → 0 命中）→ AC-S-127 步骤 3 **没有机械判据**。
//!
//! # ⚠️ 这两个事件码**不由** `PlatformSinkSsoRecorder` 桥接（与任务卡的假设不同）
//!
//! `crates/platform-iam/src/sso/bridge.rs:96` **明确跳过**这两个码：
//!
//! ```text
//! fn record(&self, event: &SsoAuditEvent) {
//!     // 服务自己写的那两个事件码 → **不重复**写（否则一次登录两条记录）。
//!     if Self::is_written_by_the_service(event.code) { return; }   // ← :53 / :96
//! ```
//!
//! 原因（见 `bridge.rs:12-22`）：**D157 要求「审计写不进去 → 拒绝登录（503）」**，
//! 而 `SsoAuditRecorder::record` 是**同步**的、表达不了「写失败要拒绝」
//! → 两个**登录**事件码由 `IamService` 自己 `await` 写（`service.rs:962-986`）。
//!
//! → 所以本文件驱动的是**真正的生产写入路径**
//! （`IamService::complete_oidc_callback` → `record_audit` → 注入的 `AuditSink`），
//! 而不是桥接器。桥接器**不该**写这两个码这件事已由
//! `bridge.rs::tests::the_login_codes_are_left_to_the_service` 钉住。
//!
//! 装配层（`crates/platform-server/src/main.rs:514`）把 `PgAuditSink` 交给
//! `with_sso_audit_recorder(PlatformSinkSsoRecorder::new(...))`，
//! 同时 `with_audit_sink(audit_sink)` 交给同一个 sink —— 两处是**同一个 sink**，
//! 所以这里只注入 `AuditSink` 就等于验证了生产落库的那一路。
//!
//! # 为什么要真连库
//!
//! OIDC 回调要读 `platform_oidc_state`、要验签、要建会话 —— 全是真实路径。
//! mock IdP 是**进程内真端口**（`sso_support::MockIdp`），不依赖外网。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-iam --test sso_login_audit -- --test-threads=1 --include-ignored
//! ```
//!
//! 缺 `DATABASE_URL_TEST` → **失败**（不是「跳过」—— 跳过也算通过 = 假绿）。

mod sso_support;

use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use platform_core::RequestContext;
use platform_core::extension::audit::{AuditError, AuditEvent, AuditSink};
use serde_json::json;
use sso_support::*;

const CALLBACK_URI: &str = "http://app.example.com/api/iam/sso/oidc/callback";

/// AC-S-127 步骤 1/2 指定的头（步骤 3 要求**逐字**透传）。
const FORWARDED_FOR: &str = "203.0.113.9";
const USER_AGENT: &str = "audit-test-agent";

/// 事件码（与 `platform_iam::audit` 的常量一致 —— 这里写**字面量**：
/// 本用例要断言的正是「线上真的发出这个字符串」，引用常量会让
/// 「常量被改错」这类缺陷在两个地方同时错、仍然绿）。
const SSO_LOGIN_SUCCEEDED: &str = "iam.sso.login.succeeded";
const SSO_LOGIN_FAILED: &str = "iam.sso.login.failed";

/// 记录事件的**假 sink**（与 `tests/audit_events.rs` 同一个模式）。
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

/// 一套装好的环境：真实库 + mock IdP + 已注入的 sink。
struct Env {
    stack: Stack,
    idp: MockIdp,
    tenant: String,
    sink: Arc<RecordingSink>,
}

impl Env {
    async fn new() -> Self {
        let app_url = setup_database().await;
        let stack = build_stack(&app_url).await;
        let idp = MockIdp::start_http().await;
        let tenant = unique_tenant("sso-audit");
        install_config(
            &stack.service,
            platform_iam::sso::config::SsoConfig {
                oidc: oidc_settings(&idp.issuer, CALLBACK_URI),
                ..Default::default()
            },
        );
        stack.service.set_sso_http_client(Arc::new(
            platform_iam::sso::oidc::OidcHttpClient::new().expect("http 客户端应能构造"),
        ));
        stack
            .service
            .set_sso_frontend_base_url("http://app.example.com");
        stack
            .service
            .set_sso_oidc_client_secret("SECRET-CANARY-sso-audit");

        // 🔑 生产注入点：`main.rs:340` 也是这样把同一个 `PgAuditSink` 交给服务的。
        let sink = Arc::new(RecordingSink::default());
        assert!(
            stack
                .service
                .with_audit_sink(sink.clone() as Arc<dyn AuditSink>),
            "sink 应当能注入一次"
        );

        let env = Self {
            stack,
            idp,
            tenant,
            sink,
        };
        env.bind_bob().await;
        env
    }

    /// 建本地 `bob` 并把 `sub = idp-bob-001` 绑上去（成功登录的前置）。
    async fn bind_bob(&self) -> String {
        let user = register(
            &self.stack.router,
            &self.tenant,
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
        .bind(&self.tenant)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&user_id)
        .execute(&admin)
        .await
        .expect("绑定应能写入");
        user_id
    }

    /// 发起 `/authorize`，返回回调需要的 `(code, state)`。
    async fn authorize(&self) -> (String, String) {
        let path = format!("/api/iam/sso/oidc/authorize?tenant={}", self.tenant);
        let (status, headers, _) = http_get(&self.stack.router, &path, None, None).await;
        assert_eq!(status, StatusCode::FOUND, "authorize 应当 302");
        let query = location_query(&headers);
        let code = format!("code-{}", uuid::Uuid::new_v4().simple());
        self.idp.register_code(
            &code,
            &query["nonce"],
            &query["code_challenge"],
            &query["redirect_uri"],
        );
        (code, query["state"].clone())
    }

    /// 走一次 OIDC 回调（**带租户头与 AC 指定的两个头** —— 步骤 3 的口径）。
    ///
    /// `fail_with_alg_none = true` → mock IdP 返回 `alg: none` 的 id_token → 验签失败。
    async fn callback(&self, fail_with_alg_none: bool) -> StatusCode {
        let (code, state) = self.authorize().await;
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
        let path = format!("/api/iam/sso/oidc/callback?code={code}&state={state}");
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(&path)
            // 租户头：失败分支靠它拿到租户（成功分支也能从 state 解析出来）。
            .header("x-tenant-id", &self.tenant)
            .header("x-forwarded-for", FORWARDED_FOR)
            .header("user-agent", USER_AGENT)
            .body(axum::body::Body::empty())
            .expect("请求应能构造");
        let (status, _, _) = send(&self.stack.router, request).await;
        status
    }
}

/// **步骤 3（成功）**：SSO 登录成功 → 恰好 **1 条** `iam.sso.login.succeeded`，
/// 且 `ip` / `user_agent` **逐字**透传。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_successful_sso_login_produces_exactly_one_sso_success_event() {
    let env = Env::new().await;
    let status = env.callback(false).await;
    assert_eq!(status, StatusCode::FOUND, "成功回调应当 302 到前端");

    let events = env.sink.of(SSO_LOGIN_SUCCEEDED);
    assert_eq!(
        events.len(),
        1,
        "一次 SSO 登录成功 = **恰好一条** {SSO_LOGIN_SUCCEEDED}（不是两条 —— 桥接器不重复写）\
         ；实际记录的事件码：{:?}",
        env.sink
            .events()
            .iter()
            .map(|event| event.action.clone())
            .collect::<Vec<_>>()
    );
    let event = &events[0];

    // 事件码本身（字面量比对 —— 见文件头常量说明）。
    assert_eq!(event.action, SSO_LOGIN_SUCCEEDED, "事件码");
    assert_eq!(event.object_type, "iam_sso_session", "object_type（D163）");
    assert_eq!(event.tenant_id, env.tenant, "租户 = 链的边界");

    // AC-S-127 步骤 3 的核心口径：两个头**逐字**透传。
    assert_eq!(
        event.ip.as_deref(),
        Some(FORWARDED_FOR),
        "`ip` 必须**逐字**等于 x-forwarded-for 的第一跳"
    );
    assert_eq!(
        event.user_agent.as_deref(),
        Some(USER_AGENT),
        "`userAgent` 必须**逐字**等于 User-Agent"
    );

    // 成功事件带操作人（已认证）。
    assert!(event.actor_id.is_some(), "成功事件必须带操作人");
    // 登录事件没有变更前/后值。
    assert_eq!(event.before, None, "登录事件不带 before");
    assert_eq!(event.after, None, "登录事件不带 after");
    // 阳性对照：同一次登录**不得**同时记一条失败事件。
    assert_eq!(
        env.sink.of(SSO_LOGIN_FAILED).len(),
        0,
        "成功时不得有失败事件"
    );
}

/// **步骤 3（失败）**：SSO 登录失败 → 恰好 **1 条** `iam.sso.login.failed`，
/// `ip` / `user_agent` **逐字**透传。
///
/// ⚠️ 别把两个载荷搞混：
/// - `SsoAuditPayload`（`sso/audit.rs:28`）的**失败白名单**是 `{provider, code, ip}`；
/// - 本用例断言的是 `AuditEvent`（`login_event` → `AuditSink`），
///   它按 AC-S-122 必须**两个头都在**（与成功分支同款）。
///
/// 补齐前的缺口（复核 L-01）：把失败分支的 `user_agent` 置 `""`，本用例仍绿
/// → 对 UA 是盲的。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_failed_sso_login_produces_exactly_one_sso_failure_event() {
    let env = Env::new().await;
    // `alg: none` → 验签失败 → 回调失败分支。
    let status = env.callback(true).await;
    assert_eq!(
        status,
        StatusCode::FOUND,
        "回调失败也是 302 回前端（带错误码），不是 JSON 错误页"
    );

    let events = env.sink.of(SSO_LOGIN_FAILED);
    assert_eq!(
        events.len(),
        1,
        "一次 SSO 登录失败 = **恰好一条** {SSO_LOGIN_FAILED}；\
         实际记录的事件码：{:?}",
        env.sink
            .events()
            .iter()
            .map(|event| event.action.clone())
            .collect::<Vec<_>>()
    );
    let event = &events[0];

    assert_eq!(event.action, SSO_LOGIN_FAILED, "事件码");
    assert_eq!(
        event.object_type, "iam_sso_session",
        "object_type 与成功事件同一对象类型（D163）"
    );
    assert_eq!(event.tenant_id, env.tenant, "租户 = 链的边界");

    // 失败事件同样要能关联来源（AC-S-127 步骤 3 的 ip 口径不分成功/失败）。
    assert_eq!(
        event.ip.as_deref(),
        Some(FORWARDED_FOR),
        "失败事件的 `ip` 也必须逐字透传"
    );
    // `user_agent` 同理：AC-S-127 步骤 3 要求 ip / userAgent **逐字**透传，
    // 口径不分成功/失败 —— 失败分支漏了这条断言 → 把 UA 置空仍绿（复核 L-01）。
    assert_eq!(
        event.user_agent.as_deref(),
        Some(USER_AGENT),
        "失败事件的 `userAgent` 也必须逐字透传"
    );

    // 失败时用户尚未通过认证 → **没有操作人**（不可枚举判据的一部分）。
    assert_eq!(event.actor_id, None, "失败事件不带操作人");
    // 阳性对照：失败时不得同时记一条成功事件。
    assert_eq!(
        env.sink.of(SSO_LOGIN_SUCCEEDED).len(),
        0,
        "失败时不得有成功事件"
    );
}
