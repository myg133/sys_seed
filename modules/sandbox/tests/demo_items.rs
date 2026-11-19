//! `demoItem` 的字段级权限（AC-S-080）与元数据一致性（REQ-012）。
//!
//! | 断言 | 为什么必须这样断言 |
//! |------|--------------------|
//! | 无权字段**键不存在**（`hasOwnProperty == false`） | 只断言 `record.secret == null` 的话，「把字段设成 null」的实现也会过 —— 而那是**不一样的语义** |
//! | 有权限时值**等于库里的值** | 掩码成 `***` 也满足「有这个键」，只有等值断言能区分 |
//! | 元数据未声明的字段（`internalNote` / `createdAt`）**不出现** | 白名单语义是 fail closed：少显示而不是多泄露 |
//! | 信封的 `nextCursor` 不被投影剔掉 | 对信封整体投影会让前端永远只有一页 |
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p sandbox --test demo_items -- --include-ignored
//! ```
//!
//! 这些用例是 `#[ignore]` 的（需要真库），`scripts/test-integration.sh` 会用
//! `--include-ignored` 真的跑到 —— 「跳过也算通过」在本仓是明确的假绿。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use platform_core::RequestContext;
use platform_core::extension::expr::CompiledCondition;
use platform_core::extension::manifest::Manifest;
use platform_core::extension::permission::{DataScope, PermissionChecker, ScopeFilter};
use platform_db::{DbOptions, TenantDb, TenantQuery, apply_policies};
use sandbox::http::{SandboxState, router};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

/// 当前用户。
const USER: &str = "u-012";
/// 部署准备用的 advisory lock 键。
const SETUP_LOCK: i64 = 6_012_012;
/// 有内部备注的样本。
const ITEM_WITH_SECRET: &str = "demo-1";
/// 库里的内部备注值（断言「值等于库里的值」）。
const SECRET_VALUE: &str = "内部-A";

/// 只按「给了哪些权限点」回答问题的测试替身。
struct StubChecker {
    granted: Vec<String>,
}

impl PermissionChecker for StubChecker {
    fn has(&self, _ctx: &RequestContext, permission: &str) -> bool {
        self.granted.iter().any(|code| code == permission)
    }

    fn scope(&self, _ctx: &RequestContext, _scope: DataScope) -> ScopeFilter {
        ScopeFilter::All
    }

    fn effective_scope(&self, _ctx: &RequestContext, _permission: &str) -> ScopeFilter {
        ScopeFilter::All
    }

    fn condition(&self, _ctx: &RequestContext, _permission: &str) -> Option<CompiledCondition> {
        None
    }
}

fn stub_checker(granted: &[&str]) -> Arc<dyn PermissionChecker> {
    Arc::new(StubChecker {
        granted: granted.iter().map(|code| (*code).to_string()).collect(),
    })
}

fn required_test_database_url() -> String {
    let url = std::env::var("DATABASE_URL_TEST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "缺少 DATABASE_URL_TEST：真实库用例必须真的连库跑，不能静默跳过。\n  \
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
    assert_eq!(db, "base_test", "集成测试只允许跑在 base_test 库上（当前：{db}）");
    url
}

fn app_role_url(admin_url: &str) -> String {
    let (scheme, rest) = admin_url.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()))
}

/// 准备真库环境 + 幂等播种，返回**应用角色**连接。
///
/// `tenant` 让每个用例用**自己的租户**：播种先清掉该租户的 `sandbox_demo_item`，
/// 再插 3 条固定记录 —— 用例之间因此互不影响（并行跑也不会互相清数据）。
///
/// # 播种为什么在管理员连接上做
///
/// 应用角色的连接带着 RLS，插数据需要 `app.tenant_id`；用管理员连接造夹具
/// 是既有测试的同一套做法（`http_authz.rs` 的 `setup()`）。
/// **幂等**：先删后插（D101：不得依赖其它测试的副作用）。
async fn setup(tenant: &str) -> TenantDb {
    let url = required_test_database_url();
    let admin_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap_or_else(|err| panic!("测试库不可达（{url}）：{err}"));

    let root = repo_root();
    let mut conn = admin_pool.acquire().await.expect("应当能取到连接");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能拿到部署锁");

    sqlx::raw_sql(&read_file(&root.join("deploy/seed/02-app-role.sql")))
        .execute(&mut *conn)
        .await
        .expect("应用角色准备失败");

    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    let mut up_files: Vec<PathBuf> = std::fs::read_dir(root.join("modules/sandbox/migrations"))
        .expect("模块迁移目录应可读")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".up.sql"))
        })
        .collect();
    up_files.sort();
    for path in up_files {
        sqlx::raw_sql(&read_file(&path))
            .execute(&mut *conn)
            .await
            .expect("模块迁移应能应用");
    }

    // 平台按 manifest 的 tenantScoped 段统一生成 RLS 策略（模块不手写）。
    let manifest = Manifest::from_path(&root.join("modules/sandbox/manifest.json"))
        .expect("沙箱 manifest 应可读");
    let spec = manifest
        .tenant_scoped
        .clone()
        .expect("沙箱 manifest 必须声明 tenantScoped");
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    apply_policies(&admin_db, &spec)
        .await
        .expect("RLS 策略生成应能应用");

    // 幂等播种：先清掉本租户的数据（用例之间互不影响），再插 3 条记录。
    let ctx = RequestContext::new().with_tenant(tenant);
    admin_db
        .execute(
            &ctx,
            &TenantQuery::new("DELETE FROM sandbox_demo_item WHERE tenant_id = {tenant}"),
        )
        .await
        .expect("清理本租户的夹具数据应当成功");
    for (id, secret) in [
        (ITEM_WITH_SECRET, Some(SECRET_VALUE)),
        ("demo-2", None),
        ("demo-3", None),
    ] {
        let query = TenantQuery::new(
            "INSERT INTO sandbox_demo_item \
               (tenant_id, id, title, amount, paid, due_at, owner_id, secret, internal_note, created_at) \
             VALUES ({tenant}, ?, ?, 100, FALSE, '2026-12-31', 'demo-1', ?, '不应输出', '2026-11-19T00:00:00Z') \
             ON CONFLICT (tenant_id, id) DO UPDATE SET secret = EXCLUDED.secret",
        )
        .bind(id.to_string())
        .bind(format!("标题 {id}"))
        .bind(secret.map(str::to_string));
        admin_db
            .execute(&ctx, &query)
            .await
            .expect("夹具记录应当能写入");
    }

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    TenantDb::connect(&app_role_url(&url), DbOptions::default())
        .await
        .expect("应用角色连接应当能建立")
}

/// 把沙箱模块的路由挂到它自己的前缀下。
fn mount(db: TenantDb) -> axum::Router {
    axum::Router::new().nest("/api/sandbox", router(Arc::new(SandboxState::new(db))))
}

/// 发出的请求（`body` 为 `None` 时是空体）。
async fn call(
    app: &axum::Router,
    tenant: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
    checker: Arc<dyn PermissionChecker>,
) -> (StatusCode, String) {
    let ctx = RequestContext::new().with_tenant(tenant).with_user(USER);
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, "Bearer test-token");
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&value).expect("请求体应可序列化"))
        }
        None => Body::empty(),
    };
    let mut request = builder.body(body).expect("请求应当能构造");
    request.extensions_mut().insert(ctx);
    request.extensions_mut().insert(checker);

    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("请求应当能完成");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 256)
        .await
        .expect("响应体应当能读");
    (
        status,
        String::from_utf8(bytes.to_vec()).expect("响应体是 UTF-8"),
    )
}

/// 记录里的键（排序）。
fn record_keys(record: &Value) -> Vec<String> {
    let mut keys: Vec<String> = record
        .as_object()
        .expect("记录是对象")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// **AC-S-080 步骤 1/2/5**：无权用户 → 每条记录都**没有** `secret` 键。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_list_response_has_no_secret_key_for_an_unauthorized_user() {
    let tenant = "tenant-demo-list";
    let app = mount(setup(tenant).await);
    let (status, raw) = call(
        &app,
        tenant,
        "GET",
        "/api/sandbox/demo-items",
        None,
        stub_checker(&["sandbox:demo:read"]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");

    let body: Value = serde_json::from_str(&raw).expect("响应是 JSON");
    let items = body["items"].as_array().expect("items 是数组");
    assert_eq!(items.len(), 3, "body = {body}");

    for record in items {
        let object = record.as_object().expect("记录是对象");
        assert!(
            !object.contains_key("secret"),
            "无权字段必须是**键不存在**（不是 null / 空串 / 掩码）：{record}"
        );
        assert_eq!(
            record_keys(record),
            vec![
                "amount".to_string(),
                "dueAt".to_string(),
                "id".to_string(),
                "ownerId".to_string(),
                "paid".to_string(),
                "title".to_string()
            ],
            "白名单：元数据声明过且当前用户有权的字段"
        );
    }

    // 全文口径：不是「置成 null」、不是掩码、不是哨兵值。
    assert!(!raw.contains(r#""secret""#), "raw = {raw}");
    assert!(!raw.contains("REDACTED"), "raw = {raw}");
    assert!(!raw.contains("***"), "raw = {raw}");
    assert!(!raw.contains(r#""secret":null"#), "raw = {raw}");
    assert!(!raw.contains(r#""secret":"""#), "raw = {raw}");
}

/// **AC-S-080 步骤 3/4**：授予权限后**下一个请求**就包含 `secret`，值是库里的值。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn an_authorized_user_sees_the_secret_value_from_the_database() {
    let tenant = "tenant-demo-secret";
    let app = mount(setup(tenant).await);
    let (status, raw) = call(
        &app,
        tenant,
        "GET",
        "/api/sandbox/demo-items",
        None,
        stub_checker(&["sandbox:demo:read", "sandbox:demo:secret"]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");

    let body: Value = serde_json::from_str(&raw).expect("响应是 JSON");
    let items = body["items"].as_array().expect("items 是数组");
    let item = items
        .iter()
        .find(|record| record["id"] == ITEM_WITH_SECRET)
        .expect("应当有 demo-1");
    assert!(
        item.as_object().expect("对象").contains_key("secret"),
        "有权限时字段必须在：{item}"
    );
    assert_eq!(
        item["secret"], SECRET_VALUE,
        "值必须**等于**库里的值（不是掩码、不是占位、不是空串）"
    );
    // 其他字段没有被一起丢掉。
    assert_eq!(item["title"], format!("标题 {ITEM_WITH_SECRET}"));
}

/// **白名单语义**：元数据未声明的字段（`internalNote` / `createdAt`）**总是**不出现。
///
/// 即使拥有全部权限点也不出现 —— 白名单不以权限为转移（design §4.2）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn undeclared_entity_fields_never_appear_even_with_all_permissions() {
    let tenant = "tenant-demo-undeclared";
    let app = mount(setup(tenant).await);
    let (status, raw) = call(
        &app,
        tenant,
        "GET",
        "/api/sandbox/demo-items",
        None,
        stub_checker(&["sandbox:demo:read", "sandbox:demo:secret"]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");
    assert!(!raw.contains("internalNote"), "raw = {raw}");
    assert!(!raw.contains("不应输出"), "内部字段的值泄露了：{raw}");
    assert!(!raw.contains("createdAt"), "raw = {raw}");
}

/// **信封**：只投影 `items[]`，`nextCursor` 不能被白名单剔掉。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_list_envelope_survives_projection_and_paginates() {
    let tenant = "tenant-demo-envelope";
    let app = mount(setup(tenant).await);
    let (status, raw) = call(
        &app,
        tenant,
        "GET",
        "/api/sandbox/demo-items?limit=2",
        None,
        stub_checker(&["sandbox:demo:read"]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");

    let body: Value = serde_json::from_str(&raw).expect("响应是 JSON");
    let mut keys: Vec<String> = body
        .as_object()
        .expect("信封是对象")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(keys, vec!["items".to_string(), "nextCursor".to_string()]);
    assert_eq!(body["items"].as_array().expect("数组").len(), 2);
    assert!(
        body["nextCursor"].as_str().is_some_and(|cursor| !cursor.is_empty()),
        "还有下一页时 nextCursor 必须是**非空字符串**：{body}"
    );

    // 第二页：游标真的起作用，且最后一条 `nextCursor` 是 null。
    let cursor = body["nextCursor"].as_str().expect("非空").to_string();
    let (status, raw) = call(
        &app,
        tenant,
        "GET",
        &format!("/api/sandbox/demo-items?limit=2&cursor={cursor}"),
        None,
        stub_checker(&["sandbox:demo:read"]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");
    let body: Value = serde_json::from_str(&raw).expect("响应是 JSON");
    assert_eq!(body["items"].as_array().expect("数组").len(), 1);
    assert_eq!(body["nextCursor"], Value::Null);
}

/// **创建 / 更新 / 删除**闭环，且创建与更新的响应**同样**经过投影。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn created_and_updated_responses_are_projected_too() {
    let tenant = "tenant-demo-crud";
    let app = mount(setup(tenant).await);
    let none = stub_checker(&["sandbox:demo:read", "sandbox:demo:write"]);

    // 创建：201 + 记录（无权用户看不到 secret）。
    let (status, raw) = call(
        &app,
        tenant,
        "POST",
        "/api/sandbox/demo-items",
        Some(serde_json::json!({
            "title": "新建的条目",
            "amount": 42,
            "paid": true,
            "dueAt": "2026-12-01",
            "ownerId": "demo-1",
            "secret": "新建的内部备注"
        })),
        none.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "raw = {raw}");
    let created: Value = serde_json::from_str(&raw).expect("JSON");
    assert!(
        !created.as_object().expect("对象").contains_key("secret"),
        "创建响应也必须投影：{created}"
    );
    assert!(!raw.contains("新建的内部备注"), "raw = {raw}");
    let id = created["id"].as_str().expect("有 id").to_string();

    // 更新：200 + 记录。
    let (status, raw) = call(
        &app,
        tenant,
        "PUT",
        &format!("/api/sandbox/demo-items/{id}"),
        Some(serde_json::json!({ "title": "改过的标题", "amount": 7 })),
        none.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");
    let updated: Value = serde_json::from_str(&raw).expect("JSON");
    assert_eq!(updated["title"], "改过的标题");
    assert!(!updated.as_object().expect("对象").contains_key("secret"));

    // 有权限的用户在更新响应里能看到原值（请求体没给 secret → 保留原值）。
    let (status, raw) = call(
        &app,
        tenant,
        "PUT",
        &format!("/api/sandbox/demo-items/{id}"),
        Some(serde_json::json!({ "title": "再改一次" })),
        stub_checker(&["sandbox:demo:write", "sandbox:demo:secret"]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "raw = {raw}");
    let updated: Value = serde_json::from_str(&raw).expect("JSON");
    assert_eq!(
        updated["secret"], "新建的内部备注",
        "请求体没给 secret 时不得把已有值清空"
    );

    // 删除：204；再删 → 404（不可见 = 不存在）。
    let (status, raw) = call(
        &app,
        tenant,
        "DELETE",
        &format!("/api/sandbox/demo-items/{id}"),
        None,
        none.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "raw = {raw}");
    let (status, raw) = call(
        &app,
        tenant,
        "DELETE",
        &format!("/api/sandbox/demo-items/{id}"),
        None,
        none,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "raw = {raw}");
}

/// 无 `sandbox:demo:write` 的用户不能创建（403，走 `require_permission`）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn writes_require_the_write_permission() {
    let tenant = "tenant-demo-write";
    let app = mount(setup(tenant).await);
    let (status, raw) = call(
        &app,
        tenant,
        "POST",
        "/api/sandbox/demo-items",
        Some(serde_json::json!({ "title": "不该成功" })),
        stub_checker(&["sandbox:demo:read"]),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "raw = {raw}");
    let body: Value = serde_json::from_str(&raw).expect("JSON");
    assert_eq!(body["code"], "INSUFFICIENT_PERMISSION");
}

/// **编译期读入的元数据必须与盘上的一致**（否则元数据接口与投影会用两份声明）。
///
/// `SandboxState::new` 用 `include_str!` 的 manifest（测试与单测方便），
/// 生产装配用 `SandboxState::from_manifest`（读运行期那一份）。这条用例守住两份不会分叉。
#[test]
fn the_baked_schema_matches_the_manifest_on_disk() {
    let root = repo_root();
    let path = root.join("modules/sandbox/manifest.json");
    let raw = read_file(&path);

    let mut migrator_less = Manifest::parse_str(&raw).expect("manifest 应能解析");
    let on_disk = migrator_less
        .schema
        .take()
        .and_then(|mut schemas| schemas.remove("demoItem"))
        .expect("manifest 必须声明 schema.demoItem");

    let baked = Manifest::parse_str(include_str!("../manifest.json"))
        .expect("编译期 manifest 应能解析")
        .schema
        .and_then(|mut schemas| schemas.remove("demoItem"))
        .expect("编译期 manifest 必须声明 schema.demoItem");

    assert_eq!(baked, on_disk, "编译期与盘上的元数据必须一致");
    assert_eq!(baked.fields.len(), 7);
    assert_eq!(
        baked.fields.last().and_then(|field| field.permission()),
        Some("sandbox:demo:secret")
    );
}
