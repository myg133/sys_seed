//! `/api/rbac/*` 管理接口的**自举防护**（AC-S-034 的 ★ 条款）。
//!
//! | 情形 | 期望 |
//! |------|------|
//! | 普通用户（只有内置角色 `platform-user`）调写接口 | **403** `INSUFFICIENT_PERMISSION`，**且不落库** |
//! | 普通用户调读接口（读角色列表） | **403** `INSUFFICIENT_PERMISSION` |
//! | 普通用户读**权限点注册表** | **200**（豁免：前端 `can()` 靠它） |
//! | 有管理权限点的用户调写接口 | **201 / 204**（对照组：证明不是接口坏了） |
//!
//! # 为什么这条最重要
//!
//! 不挡住的话，**任何登录用户都能给自己授权 `platform-admin`** ——
//! 整个权限系统被架空。所以「被拒的写入不得部分生效」也要断言：
//! 403 之后库里**不能**出现那条角色。
//!
//! # 判定实现是**真的**
//!
//! 这里的 `/api/rbac` 用的是真实的 `RbacChecker` + 真实的库 ——
//! 令牌校验是替身（只把令牌映射到用户），其余链路与线上一致。

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use platform_core::RequestContext;
use platform_rbac::http::{RbacState, TokenVerifier, VerifiedToken, router};
use platform_rbac::{RbacChecker, RbacService, RbacStore, ScopePolicy};
use serde_json::Value;
use tower::ServiceExt;

use common::*;

/// 管理权限点。
const PERM_ROLE_READ: &str = "rbac:role:read";
const PERM_ROLE_WRITE: &str = "rbac:role:write";

/// 普通用户的令牌。
const TOKEN_PLAIN: &str = "token-plain";
/// 管理员的令牌。
const TOKEN_MANAGER: &str = "token-manager";

/// 把令牌映射到用户的校验替身。
///
/// ⚠️ 租户与用户都是**每个用例独有**的：用例并行时不能共用租户
/// （epoch 行会互相抢锁 → 死锁），也不能共用角色 id（主键冲突）。
struct Tokens {
    tenant: String,
    plain_user: String,
    manager_user: String,
}

impl TokenVerifier for Tokens {
    fn verify(&self, token: &str) -> Result<VerifiedToken, platform_rbac::RbacError> {
        let user_id = match token {
            TOKEN_PLAIN => &self.plain_user,
            TOKEN_MANAGER => &self.manager_user,
            _ => return Err(platform_rbac::RbacError::Unauthenticated),
        };
        Ok(VerifiedToken {
            user_id: user_id.clone(),
            tenant_id: self.tenant.clone(),
        })
    }
}

/// 本轮运行的标识（跨运行也不能撞）。
fn run_id() -> &'static str {
    use std::sync::OnceLock;
    static RUN_ID: OnceLock<String> = OnceLock::new();
    RUN_ID.get_or_init(|| uuid::Uuid::new_v4().to_string()[..8].to_string())
}

/// 一个用例的租户与用户。
struct Fixture2 {
    tenant: String,
    plain_user: String,
    manager_user: String,
    role_id: String,
}

/// 造一个用例的夹具（每个用例一套，互不重叠）。
fn fixture2() -> Fixture2 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let tag = format!("{}-{:04}", run_id(), COUNTER.fetch_add(1, Ordering::SeqCst));
    Fixture2 {
        tenant: format!("tenant-{tag}"),
        plain_user: format!("u-plain-{tag}"),
        manager_user: format!("u-manager-{tag}"),
        role_id: format!("rbac-manager-{tag}"),
    }
}

/// 造一个真实的 `/api/rbac` 路由。
fn app(db: platform_db::TenantDb, fixture: &Fixture2) -> axum::Router {
    let service = Arc::new(RbacService::new(RbacStore::new(db), ScopePolicy::default()));
    let checker = Arc::new(RbacChecker::new(service.clone()));
    let state = Arc::new(RbacState {
        service,
        checker,
        tokens: Arc::new(Tokens {
            tenant: fixture.tenant.clone(),
            plain_user: fixture.plain_user.clone(),
            manager_user: fixture.manager_user.clone(),
        }),
    });
    // 平台把 `/api/rbac` 挂载在根路由下（业务模块的路由不由平台挂载）。
    axum::Router::new().nest("/api/rbac", router(state))
}

/// 跑一次请求。
async fn call(
    app: &axum::Router,
    tenant: &str,
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        let bytes = serde_json::to_vec(&body).expect("请求体应当能序列化");
        let mut request = builder.body(Body::from(bytes)).expect("请求应当能构造");
        request
            .extensions_mut()
            .insert(RequestContext::new().with_tenant(tenant));
        let response = app.clone().oneshot(request).await.expect("请求应当能完成");
        return finish(response).await;
    }
    let request = builder.body(Body::empty()).expect("请求应当能构造");
    let response = app.clone().oneshot(request).await.expect("请求应当能完成");
    finish(response).await
}

/// 读状态码与响应体。
async fn finish(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("响应体应当能读");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// 库里有没有某个角色的编码。
async fn role_exists(fixture: &Fixture, tenant: &str, code: &str) -> bool {
    let ctx = RequestContext::new().with_tenant(tenant);
    let query = platform_db::TenantQuery::new(
        "SELECT id FROM platform_role WHERE tenant_id = {tenant} AND code = ?",
    )
    .bind(code.to_string());
    let rows: Vec<(String,)> = fixture
        .admin_db
        .fetch_all(&ctx, &query)
        .await
        .expect("角色应当能读出");
    !rows.is_empty()
}

/// 造两个用户：一个**普通用户**，一个**管理员**。
async fn seed_users(fixture: &Fixture, service: &RbacService, ids: &Fixture2) {
    let ctx = RequestContext::new().with_tenant(&ids.tenant);
    service
        .ensure_builtin_roles(&ctx)
        .await
        .expect("内置角色应当能建成");

    // 普通用户：只有内置角色 `platform-user`（**没有任何**管理权限点）。
    seed_iam_user(fixture, &ids.tenant, &ids.plain_user).await;
    service
        .bind_user_role(&ctx, &ids.plain_user, "builtin-platform-user")
        .await
        .expect("绑定应当成功");

    // 管理员：持有 `rbac:role:read` + `rbac:role:write`。
    seed_iam_user(fixture, &ids.tenant, &ids.manager_user).await;
    let role = platform_rbac::entity::Role {
        tenant_id: ids.tenant.clone(),
        id: ids.role_id.clone(),
        code: ids.role_id.clone(),
        name: "权限管理员".to_string(),
        data_scope: platform_core::extension::permission::DataScope::All,
        custom_filter: None,
        use_all_memberships: false,
        builtin: false,
    };
    service
        .create_role(&ctx, &role)
        .await
        .expect("角色应当能建成");
    service
        .grant_permission(&ctx, &ids.role_id, PERM_ROLE_READ, None)
        .await
        .expect("授权应当成功");
    service
        .grant_permission(&ctx, &ids.role_id, PERM_ROLE_WRITE, None)
        .await
        .expect("授权应当成功");
    service
        .bind_user_role(&ctx, &ids.manager_user, &ids.role_id)
        .await
        .expect("绑定应当成功");
}

/// **普通用户**调管理接口 → 403，且**不落库**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_plain_user_cannot_reach_the_management_api() {
    let fixture = setup().await;
    let ids = fixture2();
    let service = RbacService::new(
        RbacStore::new(fixture.app_db.clone()),
        ScopePolicy::default(),
    );
    seed_users(&fixture, &service, &ids).await;
    let app = app(fixture.app_db.clone(), &ids);

    // 1) 建角色 → 403。
    let (status, body) = call(
        &app,
        &ids.tenant,
        "POST",
        "/api/rbac/roles",
        TOKEN_PLAIN,
        Some(serde_json::json!({
            "code": "m-made",
            "name": "M 建的",
            "dataScope": "ALL"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "普通用户不得建角色：{body}");
    assert_eq!(body["code"], "INSUFFICIENT_PERMISSION");
    assert_eq!(body["status"], 403);
    // 被拒的写入**不得部分生效**。
    assert!(
        !role_exists(&fixture, &ids.tenant, "m-made").await,
        "被拒的写入不得落库（否则任何登录用户都能给自己提权）"
    );

    // 2) 读角色列表 → 同样 403（读接口也要管理权限点）。
    let (status, body) = call(
        &app,
        &ids.tenant,
        "GET",
        "/api/rbac/roles",
        TOKEN_PLAIN,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "普通用户不得读角色列表：{body}"
    );
    assert_eq!(body["code"], "INSUFFICIENT_PERMISSION");

    // 3) **豁免**：权限点注册表任何已认证用户都能读（前端 `can()` 靠它）。
    let (status, body) = call(
        &app,
        &ids.tenant,
        "GET",
        "/api/rbac/permissions",
        TOKEN_PLAIN,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "注册表查询是豁免的：{body}");
    let permissions = body.as_array().expect("响应是数组");
    assert!(
        permissions
            .iter()
            .any(|item| item["code"] == PERM_ROLE_WRITE),
        "注册表里必须有管理权限点：{body}"
    );
}

/// **有管理权限点**的用户调管理接口 → 放行（对照组）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_manager_can_reach_the_management_api() {
    let fixture = setup().await;
    let ids = fixture2();
    let service = RbacService::new(
        RbacStore::new(fixture.app_db.clone()),
        ScopePolicy::default(),
    );
    seed_users(&fixture, &service, &ids).await;
    let app = app(fixture.app_db.clone(), &ids);

    let (status, body) = call(
        &app,
        &ids.tenant,
        "POST",
        "/api/rbac/roles",
        TOKEN_MANAGER,
        Some(serde_json::json!({
            "code": "manager-made",
            "name": "管理员建的",
            "dataScope": "OWN_ORG"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "有权限点就必须放行：{body}");
    assert_eq!(body["code"], "manager-made");
    assert!(
        role_exists(&fixture, &ids.tenant, "manager-made").await,
        "放行的写入必须落库"
    );

    // 读接口也放行。
    let (status, body) = call(
        &app,
        &ids.tenant,
        "GET",
        "/api/rbac/roles",
        TOKEN_MANAGER,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "有权限点就必须放行：{body}");
    let roles = body.as_array().expect("响应是数组");
    assert!(
        roles.iter().any(|item| item["code"] == "manager-made"),
        "列表里必须有刚建的角色：{body}"
    );
}

/// 跨租户的角色 id → **404**（不泄露存在性）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn a_role_of_another_tenant_is_not_found() {
    let fixture = setup().await;
    let ids = fixture2();
    let service = RbacService::new(
        RbacStore::new(fixture.app_db.clone()),
        ScopePolicy::default(),
    );
    seed_users(&fixture, &service, &ids).await;
    let app = app(fixture.app_db.clone(), &ids);

    // 在**另一个租户**建一个角色（管理员连接，绕过 RLS）。
    let ctx_b = RequestContext::new().with_tenant(format!("tenant-{}-other", run_id()));
    let query = platform_db::TenantQuery::new(
        "INSERT INTO platform_role (tenant_id, id, code, name, data_scope, custom_filter, \
                use_all_memberships, builtin) \
         VALUES ({tenant}, 'role-t2-1', 'rbac-t2-only', '租户 B 的角色', 'SELF', NULL, FALSE, FALSE)",
    );
    fixture
        .admin_db
        .execute(&ctx_b, &query)
        .await
        .expect("租户 B 的角色应当能插入");

    // 以租户 A 的身份读它 → 404（与「根本不存在」无法区分）。
    let (status, body) = call(
        &app,
        &ids.tenant,
        "GET",
        "/api/rbac/roles/role-t2-1",
        TOKEN_MANAGER,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "跨租户 id 必须是 404：{body}"
    );
    assert_eq!(body["code"], "NOT_FOUND");

    let (missing_status, missing_body) = call(
        &app,
        &ids.tenant,
        "GET",
        "/api/rbac/roles/role-not-exist-9999",
        TOKEN_MANAGER,
        None,
    )
    .await;
    assert_eq!(missing_status, status, "两个 404 的状态码必须相同");
    assert_eq!(missing_body["code"], body["code"]);
    let hidden_keys: Vec<&String> = body.as_object().expect("错误体是对象").keys().collect();
    let missing_keys: Vec<&String> = missing_body
        .as_object()
        .expect("错误体是对象")
        .keys()
        .collect();
    assert_eq!(hidden_keys, missing_keys, "两个 404 的字段集合必须相同");

    // 租户 B 的角色**没有被删**。
    let query = platform_db::TenantQuery::new(
        "SELECT id FROM platform_role WHERE tenant_id = {tenant} AND id = ?",
    )
    .bind("role-t2-1".to_string());
    let rows: Vec<(String,)> = fixture
        .admin_db
        .fetch_all(&ctx_b, &query)
        .await
        .expect("角色应当能读出");
    assert_eq!(rows.len(), 1, "跨租户的 404 不得删掉别人的数据");
}
