//! 鉴权中间件的**三种拒绝语义**与错误体形状（REQ-009）。
//!
//! | AC | 用例 |
//! |----|------|
//! | AC-S-044 | 401 / 403 / 404 三种拒绝语义 + RFC 9457 错误体形状 |
//! | AC-S-045 | 404 **不泄露资源存在性**（无权资源 vs 不存在资源） |
//! | AC-S-048 | 数据范围过滤在 **SQL 层**生效（渲染出的 SQL 含范围条件） |
//!
//! # 这里的判定实现是**测试替身**
//!
//! 真实判定实现（`platform-rbac`）在它自己的真实库用例里验证；
//! 本文件验证的是**业务模块这一侧**：路由怎么绑权限点、越权怎么表现、
//! 数据范围怎么进 SQL。
//!
//! ⚠️ 业务模块**不依赖** `platform-rbac`（AGENTS.md 硬规则 2）——
//! 判定实现由**平台**通过请求扩展注入（`Arc<dyn PermissionChecker>`）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use platform_core::RequestContext;
use platform_core::extension::expr::CompiledCondition;
use platform_core::extension::permission::{DataScope, PermissionChecker, ScopeFilter};
use platform_db::{DbOptions, TenantDb, TenantQuery, apply_policies};
use sandbox::http::{SandboxState, router};
use sandbox::scope_item::PgScopeItemRepository;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

/// 租户 A。
const TENANT_A: &str = "tenant-a";
/// 主部门。
const DEPT_A: &str = "dept-a";
/// 子部门。
const DEPT_A_1: &str = "dept-a-1";
/// 别的部门。
const DEPT_B: &str = "dept-b";
/// 当前用户。
const USER: &str = "u-1001";
/// 范围之内的记录。
const ITEM_VISIBLE: &str = "item-a2";
/// 范围之外的记录（落在别的部门分支）。
const ITEM_HIDDEN: &str = "item-b1";
/// 永远不存在的 id。
const ITEM_MISSING: &str = "item-not-exist-9999";

/// 部署准备用的 advisory lock 键。
const SETUP_LOCK: i64 = 6_009_009;

/// **测试替身**的判定实现。
///
/// 它只按「权限点 → 有没有 / 什么范围 / 什么条件」回答问题，
/// 不碰数据库 —— 真实的判定在 `platform-rbac` 的真实库用例里验。
struct StubChecker {
    /// 该用户拥有的权限点。
    granted: Vec<String>,
    /// 数据范围过滤条件。
    filter: ScopeFilter,
    /// 属性条件。
    condition: Option<CompiledCondition>,
}

impl PermissionChecker for StubChecker {
    fn has(&self, _ctx: &RequestContext, permission: &str) -> bool {
        self.granted.iter().any(|code| code == permission)
    }

    fn scope(&self, _ctx: &RequestContext, scope: DataScope) -> ScopeFilter {
        match scope {
            DataScope::All => ScopeFilter::All,
            DataScope::OnlySelf => ScopeFilter::Owner {
                column: "created_by".to_string(),
                user_id: USER.to_string(),
            },
            _ => self.filter.clone(),
        }
    }

    fn effective_scope(&self, _ctx: &RequestContext, _permission: &str) -> ScopeFilter {
        self.filter.clone()
    }

    fn condition(&self, _ctx: &RequestContext, _permission: &str) -> Option<CompiledCondition> {
        self.condition.clone()
    }
}

/// 造一个只允许看到 `dept-a` 子树的判定实现。
fn stub_checker(granted: &[&str]) -> Arc<dyn PermissionChecker> {
    Arc::new(StubChecker {
        granted: granted.iter().map(|code| code.to_string()).collect(),
        filter: ScopeFilter::OrgNodes {
            column: "org_node_id".to_string(),
            nodes: vec![DEPT_A.to_string(), DEPT_A_1.to_string()],
        },
        condition: None,
    })
}

/// 测试库地址：只允许 `base_test`。
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
    assert_eq!(
        db, "base_test",
        "集成测试只允许跑在 base_test 库上（当前：{db}）"
    );
    url
}

/// 把管理员连接串换成**应用角色**的连接串。
fn app_role_url(admin_url: &str) -> String {
    let (scheme, rest) = admin_url.split_once("://").expect("连接串必须带 scheme");
    let (_userinfo, host) = rest.split_once('@').expect("连接串必须带用户信息");
    format!("{scheme}://base_app:base@{host}")
}

/// 仓库根目录。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 读一个文件（缺失就 panic）。
fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()))
}

/// 准备真实库环境（平台迁移 + 模块建表 + RLS 策略）。
async fn setup() -> TenantDb {
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

    let role_sql = read_file(&root.join("deploy/seed/02-app-role.sql"));
    sqlx::raw_sql(&role_sql)
        .execute(&mut *conn)
        .await
        .expect("应用角色准备失败");

    let mut migrator = sqlx::migrate::Migrator::new(root.join("migrations"))
        .await
        .expect("平台迁移目录应可读");
    migrator.set_ignore_missing(true);
    migrator.run(&mut *conn).await.expect("平台迁移应能应用");

    let module_migrations = root.join("modules/sandbox/migrations");
    let mut up_files: Vec<PathBuf> = std::fs::read_dir(&module_migrations)
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
        let sql = read_file(&path);
        sqlx::raw_sql(&sql)
            .execute(&mut *conn)
            .await
            .expect("模块迁移应能应用");
    }

    // ⚠️ **只在策略缺失时才执行 DDL**（`ALTER TABLE ... ENABLE ROW LEVEL SECURITY`
    // 要 AccessExclusiveLock，与另一个用例的 INSERT 互等 → 死锁，实测复现）。
    let admin_db = TenantDb::from_pool(admin_pool.clone(), DbOptions::default());
    let scopes = platform_db::collect_scopes(&root).expect("租户作用域声明应能收集");
    let specs: Vec<platform_core::extension::manifest::TenantScopedSpec> =
        scopes.iter().map(|scope| scope.spec.clone()).collect();
    let report = platform_db::verify_policies(&admin_db, &specs)
        .await
        .expect("隔离状态应当能查出");
    let policies_ready = report.tables.iter().all(|table| table.rls_enabled)
        && report.tables.iter().all(|table| table.policy_count > 0);
    if !policies_ready {
        for scope in &scopes {
            apply_policies(&admin_db, &scope.spec)
                .await
                .expect("RLS 策略生成应能应用");
        }
    }

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut *conn)
        .await
        .expect("应当能释放部署锁");
    drop(conn);

    // 夹具数据：两条记录（一条在范围内、一条在范围外）。
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    for (id, node) in [(ITEM_VISIBLE, DEPT_A_1), (ITEM_HIDDEN, DEPT_B)] {
        let query = TenantQuery::new(
            "INSERT INTO sandbox_scope_item (tenant_id, id, title, amount, org_node_id, created_by) \
             VALUES ({tenant}, ?, ?, 5000, ?, 'someone-else') \
             ON CONFLICT (tenant_id, id) DO UPDATE SET org_node_id = EXCLUDED.org_node_id",
        )
        .bind(id.to_string())
        .bind(format!("标题 {id}"))
        .bind(node.to_string());
        admin_db
            .execute(&ctx, &query)
            .await
            .expect("夹具记录应当能插入");
    }

    let app_url = app_role_url(&url);
    TenantDb::connect(&app_url, DbOptions::default())
        .await
        .expect("应用角色连接应当能建立")
}

/// 把沙箱模块的路由挂到它自己的前缀下（业务模块的路由**不由平台挂载**）。
fn mount(db: TenantDb) -> axum::Router {
    axum::Router::new().nest("/api/sandbox", router(Arc::new(SandboxState::new(db))))
}

/// 造一个请求（带上下文与判定实现）。
fn request(method: &str, path: &str, checker: Arc<dyn PermissionChecker>) -> Request<Body> {
    let ctx = RequestContext::new().with_tenant(TENANT_A).with_user(USER);
    let mut builder = Request::builder().method(method).uri(path);
    if path.starts_with("/api/sandbox/scope-item") {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let mut request = builder.body(Body::empty()).expect("请求应当能构造");
    request.extensions_mut().insert(ctx);
    request.extensions_mut().insert(checker);
    request
}

/// 跑一次请求，返回（状态码, 响应头, 响应体）。
async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    checker: Arc<dyn PermissionChecker>,
) -> (StatusCode, Vec<(String, String)>, Value) {
    let response = app
        .clone()
        .oneshot(request(method, path, checker))
        .await
        .expect("请求应当能完成");
    let status = response.status();
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 64)
        .await
        .expect("响应体应当能读");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, body)
}

/// AC-S-044 的第 1 条：**不带令牌** → 401 `UNAUTHENTICATED`。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_request_without_a_token_is_unauthenticated() {
    let db = setup().await;
    let app = mount(db);

    // **不注入**上下文（模拟「没认证」）。
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/sandbox/scope-item")
                .body(Body::empty())
                .expect("请求应当能构造"),
        )
        .await
        .expect("请求应当能完成");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 64)
        .await
        .expect("响应体应当能读");
    let body: Value = serde_json::from_slice(&bytes).expect("响应体应当是 JSON");
    assert_eq!(body["code"], "UNAUTHENTICATED");
    assert_eq!(body["status"], 401, "状态码字段也要一致");
}

/// AC-S-044 的第 2 条：**有令牌但缺权限点** → 403 `INSUFFICIENT_PERMISSION`。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_user_without_the_permission_is_forbidden() {
    let db = setup().await;
    let app = mount(db);

    // 判定实现说：这个用户**没有** `sandbox:scope-item:read`。
    let checker = stub_checker(&[]);
    let (status, _headers, body) = call(&app, "GET", "/api/sandbox/scope-item", checker).await;

    assert_eq!(status, StatusCode::FORBIDDEN, "缺权限点必须是 403");
    assert_eq!(body["code"], "INSUFFICIENT_PERMISSION");
    assert_eq!(body["status"], 403);
    // 契约 C6：`detail` 必须是**英文 ASCII**，`trace_id` 必带，`instance` 是请求路径。
    let detail = body["detail"].as_str().expect("detail 必须是字符串");
    assert!(detail.is_ascii(), "detail 必须是英文 ASCII：{detail}");
    assert!(!detail.is_empty());
    assert!(
        body["trace_id"].as_str().is_some_and(|id| !id.is_empty()),
        "trace_id 必带：{body}"
    );
    assert_eq!(body["instance"], "/api/sandbox/scope-item");
    // `detail` **可以**含被拒的权限点编码（开发者信息）。
    assert!(
        detail.contains("sandbox:scope-item:read"),
        "detail 应当点出被拒的权限点：{detail}"
    );
}

/// AC-S-044 的第 3 条：**有权限点但记录不在数据范围内** → 404 `NOT_FOUND`。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_record_outside_the_data_scope_is_not_found() {
    let db = setup().await;
    let app = mount(db);

    let checker = stub_checker(&["sandbox:scope-item:read"]);
    let (status, _headers, body) = call(
        &app,
        "GET",
        &format!("/api/sandbox/scope-item/{ITEM_HIDDEN}"),
        checker,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "越权的资源必须是 404");
    assert_eq!(body["code"], "NOT_FOUND");
    assert_eq!(body["status"], 404);
    let detail = body["detail"].as_str().expect("detail 必须是字符串");
    assert!(detail.is_ascii(), "detail 必须是英文 ASCII：{detail}");
    // **不得**出现被拒资源的业务细节（标题 / 金额 / 归属人 / 组织节点）。
    for forbidden in ["item-b1", "dept-b", "5000", "someone-else"] {
        assert!(
            !detail.contains(forbidden),
            "detail 不得出现资源细节 `{forbidden}`：{detail}"
        );
    }
}

/// AC-S-045：**无权访问的资源**与**不存在的资源**必须**无法区分**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn an_unreadable_record_and_a_missing_record_are_indistinguishable() {
    let db = setup().await;
    let app = mount(db);

    let checker = stub_checker(&["sandbox:scope-item:read"]);
    let (hidden_status, hidden_headers, hidden_body) = call(
        &app,
        "GET",
        &format!("/api/sandbox/scope-item/{ITEM_HIDDEN}"),
        checker.clone(),
    )
    .await;
    let (missing_status, missing_headers, missing_body) = call(
        &app,
        "GET",
        &format!("/api/sandbox/scope-item/{ITEM_MISSING}"),
        checker.clone(),
    )
    .await;

    // 1) 状态码相同。
    assert_eq!(hidden_status, missing_status, "两次响应的状态码必须相同");
    assert_eq!(hidden_status, StatusCode::NOT_FOUND);
    // 2) 错误体**结构**相同（字段集合一致）。
    let hidden_keys: Vec<&String> = hidden_body
        .as_object()
        .expect("错误体是对象")
        .keys()
        .collect();
    let missing_keys: Vec<&String> = missing_body
        .as_object()
        .expect("错误体是对象")
        .keys()
        .collect();
    assert_eq!(
        hidden_keys, missing_keys,
        "两次响应的错误体字段集合必须相同"
    );
    assert_eq!(hidden_body["code"], missing_body["code"]);
    // 3) 响应头集合相同（除 trace_id 所在的 body 字段外）。
    let hidden_header_names: Vec<String> = hidden_headers
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    let missing_header_names: Vec<String> = missing_headers
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    assert_eq!(
        hidden_header_names, missing_header_names,
        "两次响应的响应头集合必须相同"
    );

    // 4) 对照组：范围内的记录 → **200**（证明接口本身可用，不是恒 404）。
    let (visible_status, _headers, visible_body) = call(
        &app,
        "GET",
        &format!("/api/sandbox/scope-item/{ITEM_VISIBLE}"),
        checker,
    )
    .await;
    assert_eq!(visible_status, StatusCode::OK, "范围内的记录必须能读到");
    assert_eq!(visible_body["id"], ITEM_VISIBLE);
}

/// AC-S-048：数据范围过滤在 **SQL 层**生效。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn the_data_scope_is_applied_in_the_sql_layer() {
    let db = setup().await;
    let repository = PgScopeItemRepository::new(db.clone());
    let filter = ScopeFilter::OrgNodes {
        column: "org_node_id".to_string(),
        nodes: vec![DEPT_A.to_string(), DEPT_A_1.to_string()],
    };

    // 1) 渲染出的 SQL **含平台追加的范围条件**，且值**走参数绑定**。
    let query = repository
        .build_list_query(&filter)
        .expect("范围条件应当能渲染");
    let (sql, values) = query.render(TENANT_A).expect("应当能渲染");
    assert!(
        sql.contains("org_node_id = ANY("),
        "渲染出的 SQL 必须含范围条件：{sql}"
    );
    assert!(
        !sql.contains(DEPT_A),
        "范围条件的值必须走参数绑定（SQL 文本里不得出现字面值）：{sql}"
    );
    assert!(!sql.contains("dept-a"), "SQL 文本里不得出现节点 id：{sql}");
    assert_eq!(values.len(), 2, "租户 + 范围各一个参数");

    // 2) 结果确实被过滤了（**不是**查出来后在内存里筛）。
    let ctx = RequestContext::new().with_tenant(TENANT_A);
    let items = repository
        .list(&ctx, &filter)
        .await
        .expect("列表应当能读出");
    assert!(
        !items.iter().any(|item| item.id == ITEM_HIDDEN),
        "范围外的记录不得出现：{items:?}"
    );
    assert!(
        items.iter().any(|item| item.id == ITEM_VISIBLE),
        "范围内的记录必须出现：{items:?}"
    );

    // 3) 平台的构造器 API 上**没有**「跳过数据范围」的入口。
    //    （判据：翻遍 `platform-db` 的公开 API 源码，找不到这样的方法。）
    let query_source = read_file(&repo_root().join("crates/platform-db/src/query.rs"));
    let skip_like: Vec<&str> = query_source
        .lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("pub fn ")
                && (lower.contains("skip")
                    || lower.contains("bypass")
                    || lower.contains("ignore")
                    || lower.contains("without_scope"))
        })
        .map(|line| line.trim())
        .collect();
    assert!(
        skip_like.is_empty(),
        "平台的构造器上不得有「跳过数据范围」的入口：{skip_like:?}"
    );
}

/// `require_permission` 是**唯一**的鉴权入口（业务模块只调用平台那一个）。
#[test]
fn the_module_uses_the_platform_authorization_entry_point() {
    let source = read_file(&repo_root().join("modules/sandbox/backend/src/http.rs"));
    assert!(
        source.contains("require_permission(\"sandbox:scope-item:read\")"),
        "必须用平台的鉴权入口绑定权限点（且参数是字面量）"
    );
    assert!(
        !source.contains("fn check_perm"),
        "业务模块不得自建鉴权入口（绕过平台的唯一入口）"
    );
}
