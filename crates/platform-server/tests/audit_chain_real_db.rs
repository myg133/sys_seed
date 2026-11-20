//! REQ-014 AC-S-129 步骤 2/3 的**装配层**验证：org 与 config 两条链路的事件
//! 是否**真的进了** `platform_audit_log`（D188 / 复验报告 M-02）。
//!
//! # 这个文件的判据缺口是什么
//!
//! 复验报告 M-02：`platform-org` 与 `platform-config` 的审计埋点**只用 mock sink**
//! 测过（`RecordingSink` / `RecordingAuditRecorder`）—— 断言的是「事件被**构造**出来」
//! 的形状，**没有一条**断言它落进 `platform_audit_log`。
//!
//! 而 `main.rs:320` 构造的是**真实** `PgAuditSink`，并经
//! `build_org_state(..., audit_sink.clone())` 与 `ConfigService::with_audit_sink`
//! 接进生产链路 → **生产链路是通的，缺的是判据**。
//!
//! # 为什么断言放在 `platform-server`
//!
//! 它是唯一**同时**依赖 org / config / audit / db 的 crate → 不需要给
//! org / config 加 dev-dependency（D188）。
//!
//! # ⚠️ 必须走**生产适配器**
//!
//! - org：`platform_org::audit::PlatformSinkAuditRecorder`（生产桥接实现）
//! - config：`ConfigService::with_audit_sink`（生产注入点）
//!
//! 自己造一个「等价实现」等于**验证生产代码的拷贝** —— 那不算验证。

use std::sync::Arc;

use platform_audit::PgAuditSink;
use platform_core::RequestContext;
use platform_org::audit::PlatformSinkAuditRecorder;
use platform_org::{NewNode, NodeType, OrgService, OrgStore};
use serde_json::{Value, json};
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;

mod support;

use support::*;

/// 应用角色连接串（RLS 前提：**不能**用超管）。
fn app_url_from_admin(admin: &str) -> String {
    let (scheme, rest) = admin.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

/// 管理员连接池（只用于**读回断言**与造数；业务写入走应用角色）。
async fn admin_pool() -> sqlx::postgres::PgPool {
    let url = std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "缺少 DATABASE_URL_TEST：org/config 的审计落库用例必须真的连库跑，不能静默跳过。"
            )
        });
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"))
}

/// 本条用例的租户（唯一 → 与其它测试文件互不干扰）。
fn tenant() -> String {
    unique_tenant("audit-e2e")
}

/// 操作人。
const ACTOR: &str = "u-4001";

/// 该租户在 `platform_audit_log` 里的记录（按写入时刻升序）。
///
/// ⚠️ 手工取值（`Row::try_get`），不用 `query_as` + `FromRow` ——
/// 后者需要 sqlx 的 macros/derive feature，本仓没开（`platform-audit` 的
/// `tenant_row!` 宏就是为此存在的）。
async fn events_of(pool: &sqlx::postgres::PgPool, tenant: &str) -> Vec<RecordedEvent> {
    let rows = sqlx::query(
        "SELECT action, object_type, object_id, actor_id, \"before\", \"after\" \
         FROM platform_audit_log WHERE tenant_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(tenant)
    .fetch_all(pool)
    .await
    .expect("应当能读回审计记录");
    rows.into_iter()
        .map(|row| RecordedEvent {
            action: row.try_get("action").expect("action 列"),
            object_type: row.try_get("object_type").expect("object_type 列"),
            object_id: row.try_get("object_id").expect("object_id 列"),
            actor_id: row.try_get("actor_id").expect("actor_id 列"),
            before: row.try_get("before").expect("before 列"),
            after: row.try_get("after").expect("after 列"),
        })
        .collect()
}

/// 读回的一行（只取本文件要断言的列）。
#[derive(Debug)]
struct RecordedEvent {
    action: String,
    object_type: String,
    object_id: Option<String>,
    actor_id: Option<String>,
    before: Option<String>,
    after: Option<String>,
}

impl RecordedEvent {
    /// `before` 解析成 JSON（`None` = 列里是 `NULL`）。
    fn before_json(&self) -> Option<Value> {
        self.before.as_deref().map(|text| {
            serde_json::from_str(text)
                .unwrap_or_else(|error| panic!("before 不是合法 JSON：{error}"))
        })
    }

    /// `after` 解析成 JSON。
    fn after_json(&self) -> Option<Value> {
        self.after.as_deref().map(|text| {
            serde_json::from_str(text)
                .unwrap_or_else(|error| panic!("after 不是合法 JSON：{error}"))
        })
    }
}

/// **org 链路**：建一个组织节点 → 事件真的落进 `platform_audit_log`。
///
/// # 断言口径（D163 / AC-S-129 步骤 2）
///
/// | 字段 | 期望 |
/// |------|------|
/// | `action` | `org.node.create` |
/// | `object_type` | `org_node` |
/// | `object_id` | 新建节点的 id |
/// | `actor_id` | 请求上下文里的操作人 |
///
/// ⚠️ 走**生产适配器** `PlatformSinkAuditRecorder`（不是测试里的等价实现）。
#[tokio::test]
async fn an_org_write_really_lands_in_the_audit_table() {
    let app_url = setup_database().await;
    let pool = admin_pool().await;
    let tenant = tenant();
    let app_url = app_url_from_admin(&app_url);

    // 生产数据访问入口（应用角色 → RLS 生效）。
    let db = platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    // 🔑 生产 sink + 生产桥接适配器（与 `main.rs` 的装配**同一套类型**）。
    let sink = Arc::new(PgAuditSink::new(db.clone()));
    let recorder = Arc::new(PlatformSinkAuditRecorder::new(sink));
    let service = OrgService::new(
        OrgStore::new(db.clone()),
        platform_iam::store::IamStore::new(db),
        Arc::new(platform_org::notify::NoopNotifier),
        recorder,
    );

    let ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(ACTOR)
        .with_client_ip("203.0.113.20")
        .with_user_agent("org-audit-e2e");
    let node = service
        .create_node(
            &ctx,
            NewNode {
                id: None,
                parent_id: None,
                node_type: NodeType::Dept,
                code: "E2E-DEPT".to_string(),
                name: "端到端部门".to_string(),
            },
        )
        .await
        .expect("建节点应当成功");

    // 真的进了表（**不是**「sink 被调用过」）。
    let events = events_of(&pool, &tenant).await;
    assert_eq!(events.len(), 1, "一次 org 写入 = 恰好 1 条审计记录");
    let event = &events[0];
    assert_eq!(event.action, "org.node.create", "事件码（D163 命名法）");
    assert_eq!(event.object_type, "org_node", "object_type");
    assert_eq!(
        event.object_id.as_deref(),
        Some(node.id.as_str()),
        "object_id 必须是新建节点的 id"
    );
    assert_eq!(event.actor_id.as_deref(), Some(ACTOR), "actor_id = 操作人");
}

/// **config 链路**：写一条配置 → `config.entries.write` 真的落进 `platform_audit_log`。
///
/// # 断言口径（D163）
///
/// | 字段 | 期望 |
/// |------|------|
/// | `action` | `config.entries.write` |
/// | `object_type` | `config_entry` |
/// | `before` / `after` | 「键 → 值」的映射（`before` 是**写入前那一层**的值） |
///
/// ⚠️ 走生产注入点 `ConfigService::with_audit_sink`。
#[tokio::test]
async fn a_config_write_really_lands_in_the_audit_table() {
    let app_url = setup_database().await;
    let pool = admin_pool().await;
    let tenant = tenant();
    let app_url = app_url_from_admin(&app_url);

    let db = platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let registry = platform_config::ConfigRegistry::from_manifest_root(&repo_root())
        .expect("配置注册表应能加载");
    let service = platform_config::ConfigService::new(db, Arc::new(registry));

    // 🔑 生产注入点（与 `main.rs` 同一套装配）。
    let sink: Arc<dyn platform_core::extension::audit::AuditSink> = Arc::new(PgAuditSink::new(
        platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
            .await
            .expect("应用角色连接串应可解析"),
    ));
    assert!(service.with_audit_sink(sink), "审计 sink 应当能注入一次");

    let ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(ACTOR)
        .with_client_ip("203.0.113.21")
        .with_user_agent("config-audit-e2e");

    // 第一次写：`before` 应当是「这一层还没有这个键」→ `{}`。
    service
        .write_entries(
            &ctx,
            &platform_config::WriteRequest {
                scope: "tenant".to_string(),
                entries: vec![platform_config::WriteEntry {
                    key: "title".to_string(),
                    value: json!("审计端到端"),
                }],
            },
            platform_config::WriteRoute::Scoped,
        )
        .await
        .expect("写配置应当成功");

    let events = events_of(&pool, &tenant).await;
    assert_eq!(events.len(), 1, "一次配置写入 = 恰好 1 条审计记录");
    let event = &events[0];
    assert_eq!(event.action, "config.entries.write", "事件码");
    assert_eq!(event.object_type, "config_entry", "object_type");
    assert_eq!(event.actor_id.as_deref(), Some(ACTOR), "actor_id = 操作人");

    // `after` 必须是「键 → 新值」。
    assert_eq!(
        event.after_json(),
        Some(json!({"title": "审计端到端"})),
        "after 必须是本次写入的键值映射"
    );
    // `before` 是**写入前那一层**的值。此前该层没有这个键 →
    // `layer_values` 的口径是 `null`（不是「省略这个键」，见其 doc comment：
    // 「该层没有这个键 → `null`」）→ 期望 `{"title": null}`。
    assert_eq!(
        event.before_json(),
        Some(json!({"title": null})),
        "before 必须是写入前那一层的值（首次写入 → 键存在、值为 null）"
    );

    // 第二次写：`before` 必须带上**旧值**（证明它读的是写入前那一层，不是写后）。
    service
        .write_entries(
            &ctx,
            &platform_config::WriteRequest {
                scope: "tenant".to_string(),
                entries: vec![platform_config::WriteEntry {
                    key: "title".to_string(),
                    value: json!("审计端到端-2"),
                }],
            },
            platform_config::WriteRoute::Scoped,
        )
        .await
        .expect("第二次写配置应当成功");

    let events = events_of(&pool, &tenant).await;
    assert_eq!(events.len(), 2, "两次写入 = 两条审计记录");
    let second = &events[1];
    assert_eq!(
        second.before_json(),
        Some(json!({"title": "审计端到端"})),
        "第二次写入的 before 必须是旧值"
    );
    assert_eq!(
        second.after_json(),
        Some(json!({"title": "审计端到端-2"})),
        "第二次写入的 after 必须是新值"
    );
}

/// **config flag 链路**：写一个 flag → `config.flag.write` 真的落进 `platform_audit_log`。
///
/// # 为什么还要这一条（复核 V-03）
///
/// D163 钉了**两个**配置事件码。`config.entries.write` 已由
/// [`a_config_write_really_lands_in_the_audit_table`] 验到真库落表；
/// `config.flag.write` 此前**只有** mock sink 的形状断言
/// （`crates/platform-config/tests/audit_events.rs`）→ **真库落表无判据**。
/// 代码两条码都在发（`platform-config/src/service.rs:462` / `:547`）→
/// 这不是缺陷，是**判据缺失**。
///
/// # 断言口径（D163）
///
/// | 字段 | 期望 |
/// |------|------|
/// | `action` | `config.flag.write` |
/// | `object_type` | `config_flag` |
/// | `object_id` | flag 名（不是作用域） |
/// | `before` / `after` | 该租户层「flag 名 → 值」的映射 |
///
/// ⚠️ 走生产注入点 `ConfigService::with_audit_sink`（与上一条用例同一套装配），
/// **不**自造等价 sink。
#[tokio::test]
async fn a_config_flag_write_really_lands_in_the_audit_table() {
    let app_url = setup_database().await;
    let pool = admin_pool().await;
    let tenant = tenant();
    let app_url = app_url_from_admin(&app_url);

    let db = platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");
    let registry = platform_config::ConfigRegistry::from_manifest_root(&repo_root())
        .expect("配置注册表应能加载");
    let service = platform_config::ConfigService::new(db, Arc::new(registry));

    // 🔑 生产注入点（与上一条 config 用例 / `main.rs` 同一套装配）。
    let sink: Arc<dyn platform_core::extension::audit::AuditSink> = Arc::new(PgAuditSink::new(
        platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
            .await
            .expect("应用角色连接串应可解析"),
    ));
    assert!(service.with_audit_sink(sink), "审计 sink 应当能注入一次");

    let ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(ACTOR)
        .with_client_ip("203.0.113.22")
        .with_user_agent("config-flag-audit-e2e");

    // 第一次写：该租户层此前没有这个 flag → `before` 是 `null`（口径同 `layer_values`）。
    service
        .write_flag(
            &ctx,
            "newInvoice",
            &platform_config::FlagWriteRequest {
                enabled: Some(json!(false)),
                rollout_percent: None,
            },
        )
        .await
        .expect("写 flag 应当成功");

    let events = events_of(&pool, &tenant).await;
    assert_eq!(events.len(), 1, "一次 flag 写入 = 恰好 1 条审计记录");
    let event = &events[0];
    assert_eq!(event.action, "config.flag.write", "事件码（D163）");
    assert_eq!(event.object_type, "config_flag", "object_type（D163）");
    assert_eq!(
        event.object_id.as_deref(),
        Some("newInvoice"),
        "object_id = flag 名（不是作用域 —— 与 config.entries.write 的区别）"
    );
    assert_eq!(event.actor_id.as_deref(), Some(ACTOR), "actor_id = 操作人");
    assert_eq!(
        event.after_json(),
        Some(json!({"newInvoice": false})),
        "after 必须是本次写入的 flag → 值"
    );
    assert_eq!(
        event.before_json(),
        Some(json!({"newInvoice": null})),
        "before 必须是写入前那一层的值（首次写入 → 键存在、值为 null）"
    );

    // 第二次写：`before` 必须带上**旧值**（证明读的是写入前那一层）。
    service
        .write_flag(
            &ctx,
            "newInvoice",
            &platform_config::FlagWriteRequest {
                enabled: Some(json!(true)),
                rollout_percent: None,
            },
        )
        .await
        .expect("第二次写 flag 应当成功");

    let events = events_of(&pool, &tenant).await;
    assert_eq!(events.len(), 2, "两次 flag 写入 = 两条审计记录");
    let second = &events[1];
    assert_eq!(second.action, "config.flag.write", "第二次的事件码");
    assert_eq!(
        second.before_json(),
        Some(json!({"newInvoice": false})),
        "第二次写入的 before 必须是旧值"
    );
    assert_eq!(
        second.after_json(),
        Some(json!({"newInvoice": true})),
        "第二次写入的 after 必须是新值"
    );
}

// ---------------------------------------------------------------------------
// D185：过滤查询的 **HTTP 状态** 不得是 5xx
// ---------------------------------------------------------------------------

/// 一个恒「有权限」的判定器。
///
/// ⚠️ 这里**刻意**用桩：本用例要验的是「**SQL 能不能拼出来**」，
/// 权限判定与 SQL 渲染是正交的，换成真实 `RbacChecker` 只会引入一堆与本题
/// 无关的授权夹具（`platform-server/src/audit.rs` 的单测同样用 `AllowAll`）。
/// 与之相对，F5 的两条用例**必须**走生产适配器 —— 那里验的正是适配器本身。
struct AllowAllPermissions;

impl platform_core::extension::permission::PermissionChecker for AllowAllPermissions {
    fn has(&self, _ctx: &RequestContext, _permission: &str) -> bool {
        true
    }

    fn scope(
        &self,
        _ctx: &RequestContext,
        _scope: platform_core::extension::permission::DataScope,
    ) -> platform_core::extension::permission::ScopeFilter {
        platform_core::extension::permission::ScopeFilter::All
    }

    fn effective_scope(
        &self,
        _ctx: &RequestContext,
        _permission: &str,
    ) -> platform_core::extension::permission::ScopeFilter {
        platform_core::extension::permission::ScopeFilter::All
    }

    fn condition(
        &self,
        _ctx: &RequestContext,
        _permission: &str,
    ) -> Option<platform_core::extension::expr::CompiledCondition> {
        None
    }
}

impl platform_core::extension::permission::PermissionRefresher for AllowAllPermissions {
    fn ensure_fresh<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// 固定令牌（返回本条用例的租户）。
struct FixedToken {
    user: String,
    tenant: String,
}

impl platform_server::audit::TokenVerifier for FixedToken {
    fn verify(
        &self,
        _token: &str,
    ) -> Result<platform_server::audit::VerifiedToken, platform_server::audit::TokenVerifyError>
    {
        Ok(platform_server::audit::VerifiedToken {
            user_id: self.user.clone(),
            tenant_id: self.tenant.clone(),
        })
    }
}

/// `GET /api/audit/events?filter[actorId][eq]=…` → **200**（不是 5xx），且只返回匹配行。
///
/// # 这条断言为什么必须有
///
/// D185 的原缺陷让**任何**带过滤的查询变成
/// `tenant_id = $1actor_id = $2` → PG 报 `trailing junk after parameter`
/// → 错误被映射成 **503 `AUDIT_SINK_UNAVAILABLE`**。
/// 库级用例（`platform-audit/tests/query_real_db.rs`）证明 SQL 能执行；
/// **本用例证明 HTTP 层不会把它变成 5xx** —— 两者缺一不可。
#[tokio::test]
async fn a_filtered_query_over_http_is_not_a_server_error() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use platform_audit::{ArchiveService, AuditQueryService, ChainVerifier};
    use tower::ServiceExt;

    let app_url = setup_database().await;
    let pool = admin_pool().await;
    let tenant = tenant();

    let db = platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    // 造 2 条属于本租户的记录（1 条匹配过滤、1 条不匹配）。
    let sink = PgAuditSink::new(db.clone());
    let ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(ACTOR)
        .with_client_ip("203.0.113.22")
        .with_user_agent("http-audit-e2e");
    for (action, actor) in [
        ("http.filter.match", ACTOR),
        ("http.filter.other", "u-4999"),
    ] {
        sink.record_event(
            &ctx,
            &platform_audit::AuditEvent::new(&tenant, action, "demo_item")
                .with_actor(actor)
                .with_object_id("obj-http")
                .with_before(json!({"v": 0}))
                .with_after(json!({"v": 1})),
        )
        .await
        .expect("写入应当成功");
    }

    // 生产路由（同一个 `router()`），只把两个外部依赖换成桩。
    let state = Arc::new(platform_server::audit::AuditState {
        query: Arc::new(AuditQueryService::new(db.clone())),
        verifier: Arc::new(ChainVerifier::new(db.clone())),
        archive: Arc::new(ArchiveService::new(db)),
        tokens: Arc::new(FixedToken {
            user: ACTOR.to_string(),
            tenant: tenant.clone(),
        }),
        checker: Arc::new(AllowAllPermissions),
        permissions: Arc::new(AllowAllPermissions),
    });
    let app: axum::Router<()> =
        axum::Router::new().nest("/api/audit", platform_server::audit::router(state));

    let request = Request::builder()
        .uri(format!(
            "/api/audit/events?limit=20&filter[actorId][eq]={ACTOR}"
        ))
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .expect("应当能构造请求");
    let response = app.oneshot(request).await.expect("应当能跑完路由");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("应当能读出响应体");

    assert_eq!(
        status,
        StatusCode::OK,
        "带过滤的查询必须是 200（D185：SQL 拼错会变成 503）：{}",
        String::from_utf8_lossy(&body)
    );
    assert!(!status.is_server_error(), "过滤查询不得返回 5xx");

    let view: Value = serde_json::from_slice(&body).expect("响应体应当是 JSON");
    assert_eq!(
        view["total"],
        json!(1),
        "actorId 过滤后应当只有 1 条：{view}"
    );
    assert_eq!(
        view["items"].as_array().map(|items| items.len()),
        Some(1),
        "本页应当只有 1 条：{view}"
    );
    assert_eq!(
        view["items"][0]["actorId"],
        json!(ACTOR),
        "返回行的 actorId 必须匹配过滤条件"
    );
    assert!(
        view["nextCursor"].is_null(),
        "只有 1 条、limit=20 → 没有下一页"
    );

    let _ = pool; // 造数与读回都走应用角色；管理员池只用于其它用例
}

/// `GET /api/audit/events?limit=2` → `nextCursor` **非 null**，且用它翻页拿到不重复的记录。
///
/// # 这条断言为什么必须有（D184 / B-01）
///
/// 原缺陷绑的是 `LIMIT limit` → `has_more` 恒假 → `nextCursor` **恒 `null`** →
/// 前端「下一页」按钮**恒禁用**（`apps/shell/src/pages/Audit.svelte:340`）
/// → 翻页功能不存在。库级用例已覆盖同一件事，本用例额外证明
/// **HTTP 响应体里的 `nextCursor` 字段**真的带值（不是被序列化吃掉）。
#[tokio::test]
async fn the_http_page_carries_a_usable_next_cursor() {
    use axum::http::StatusCode;
    use platform_audit::{ArchiveService, AuditQueryService, ChainVerifier};

    let app_url = setup_database().await;
    let tenant = tenant();
    let db = platform_db::TenantDb::connect(&app_url, platform_db::DbOptions::default())
        .await
        .expect("应用角色连接串应可解析");

    let sink = PgAuditSink::new(db.clone());
    let ctx = RequestContext::new()
        .with_tenant(&tenant)
        .with_user(ACTOR)
        .with_client_ip("203.0.113.23")
        .with_user_agent("http-paging-e2e");
    let mut written = Vec::new();
    for index in 0..5 {
        written.push(
            sink.record_event(
                &ctx,
                &platform_audit::AuditEvent::new(
                    &tenant,
                    format!("http.page.{index}"),
                    "demo_item",
                )
                .with_actor(ACTOR)
                .with_object_id(format!("obj-{index}"))
                .with_before(json!({"v": 0}))
                .with_after(json!({"v": 1})),
            )
            .await
            .expect("写入应当成功"),
        );
    }

    let state = Arc::new(platform_server::audit::AuditState {
        query: Arc::new(AuditQueryService::new(db.clone())),
        verifier: Arc::new(ChainVerifier::new(db.clone())),
        archive: Arc::new(ArchiveService::new(db)),
        tokens: Arc::new(FixedToken {
            user: ACTOR.to_string(),
            tenant: tenant.clone(),
        }),
        checker: Arc::new(AllowAllPermissions),
        permissions: Arc::new(AllowAllPermissions),
    });
    let app: axum::Router<()> =
        axum::Router::new().nest("/api/audit", platform_server::audit::router(state));

    /// 打一次 `/api/audit/events`，返回 `(状态码, 响应体 JSON)`。
    async fn get(app: axum::Router<()>, uri: String) -> (StatusCode, Value) {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        let request = Request::builder()
            .uri(uri)
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .expect("应当能构造请求");
        let response = app.oneshot(request).await.expect("应当能跑完路由");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 16384)
            .await
            .expect("应当能读出响应体");
        (
            status,
            serde_json::from_slice(&body).unwrap_or_else(|error| {
                panic!(
                    "响应体应当是 JSON（{error}）：{}",
                    String::from_utf8_lossy(&body)
                )
            }),
        )
    }

    let (status, first) = get(app.clone(), "/api/audit/events?limit=2".to_string()).await;
    assert_eq!(status, StatusCode::OK, "第 1 页应当 200：{first}");
    assert_eq!(first["total"], json!(5), "total 不受分页影响");
    assert_eq!(first["items"].as_array().map(|v| v.len()), Some(2));
    let cursor = first["nextCursor"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("5 条、limit=2 → nextCursor 必须是非 null 字符串（D184）：{first}")
        })
        .to_string();

    let (status, second) = get(
        app.clone(),
        format!("/api/audit/events?limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "第 2 页应当 200：{second}");

    let first_ids: std::collections::BTreeSet<String> = first["items"]
        .as_array()
        .expect("items 是数组")
        .iter()
        .map(|item| item["id"].as_str().expect("id 是字符串").to_string())
        .collect();
    let second_ids: std::collections::BTreeSet<String> = second["items"]
        .as_array()
        .expect("items 是数组")
        .iter()
        .map(|item| item["id"].as_str().expect("id 是字符串").to_string())
        .collect();
    assert!(
        first_ids.intersection(&second_ids).next().is_none(),
        "两页不得有重复：{first_ids:?} vs {second_ids:?}"
    );
    let written_ids: std::collections::BTreeSet<String> = written.into_iter().collect();
    assert!(
        first_ids.is_subset(&written_ids) && second_ids.is_subset(&written_ids),
        "两页的 id 必须都来自本次写入"
    );
}
