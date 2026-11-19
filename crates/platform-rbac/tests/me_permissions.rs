//! 「当前用户的有效权限点」的后端出口（REQ-011 / AC-S-064 / AC-S-065）。
//!
//! 10 条用例（TC-011-001 ~ TC-011-010）逐条对应一个 `#[tokio::test]`。
//!
//! | 用例 | 断言 |
//! |------|------|
//! | TC-011-001 | 200；响应体是**对象**且恰好一个键 `permissions` |
//! | TC-011-002 | 集合整体相等 `["rbac:role:read"]`（不是「包含」） |
//! | TC-011-003 | 不带令牌 → 401 `UNAUTHENTICATED` |
//! | TC-011-004 | 请求头租户 ≠ 令牌租户 → 404 `NOT_FOUND` |
//! | TC-011-005 | 升序 + 去重 |
//! | TC-011-006 | 登录响应的 `permissions` |
//! | TC-011-007 | 无角色用户 → `permissions == []` |
//! | TC-011-008 | 两个入口给出**同一**答案 |
//! | TC-011-009 | 通配授权被展开成具体权限点 |
//! | TC-011-010 | 未注入 provider → 登录**仍然成功**，`permissions == []` |
//!
//! # 为什么要真实 IAM + 真实 RBAC
//!
//! TC-011-008 要求「登录响应」与「`/api/rbac/me/permissions`」**逐项相等** ——
//! 那只有真的 IAM + 真的 RBAC 才验得了：替身会把「两套展开逻辑」这件事藏起来。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-rbac --test me_permissions -- --test-threads=2 --include-ignored
//! ```
//!
//! 或 `scripts/test-integration.sh`（它会先重置 `base_test` 再带上 `--include-ignored`）。

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use platform_core::RequestContext;
use platform_core::extension::permission::{DataScope, PermissionProvider};
use platform_iam::config::{
    Argon2Settings, IamConfig, JwtSettings, LoginSettings, PasswordPolicySettings, TokenSettings,
    TotpSettings,
};
use platform_iam::service::{
    IamService, LoginRequest, LoginResponse, RegisterRequest, RequestMeta,
};
use platform_iam::store::IamStore;
use platform_rbac::entity::Role;
use platform_rbac::http::{RbacState, TokenVerifier, VerifiedToken, router};
use platform_rbac::{RbacChecker, RbacError, RbacService, RbacStore, ScopePolicy};
use serde_json::Value;
use tower::ServiceExt;

use common::*;

/// 权限点：只读角色。
const PERM_ROLE_READ: &str = "rbac:role:read";
/// 权限点：改角色。
const PERM_ROLE_WRITE: &str = "rbac:role:write";

/// 测试用密码（满足策略：长度 ≥ 12、不在常见口令词库里）。
const PASSWORD: &str = "Tr0ub4dor&3xKq";

/// 「当前用户的有效权限点」这条路由。
const ME_PERMISSIONS_PATH: &str = "/api/rbac/me/permissions";

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 令牌校验替身：把令牌映射到用户（**租户只有一个**）。
///
/// ⚠️ 租户与用户**每个用例独有**：用例并行时不能共用租户
/// （epoch 行会互相抢锁 → 死锁），也不能共用角色 id（主键冲突）。
#[derive(Clone)]
struct Tokens {
    tenant: String,
    users: Arc<Mutex<BTreeMap<String, String>>>,
}

impl TokenVerifier for Tokens {
    fn verify(&self, token: &str) -> Result<VerifiedToken, RbacError> {
        let users = self.users.lock().expect("锁不该被毒化");
        let user_id = users.get(token).ok_or(RbacError::Unauthenticated)?.clone();
        Ok(VerifiedToken {
            user_id,
            tenant_id: self.tenant.clone(),
        })
    }
}

/// 一个用例的「世界」：租户 + 账号 + 角色 + IAM / RBAC 两套服务 + 路由。
struct World {
    tenant: String,
    iam: Arc<IamService>,
    rbac: Arc<RbacService>,
    app: axum::Router,
    tokens: Arc<Mutex<BTreeMap<String, String>>>,
}

impl World {
    /// 租户上下文。
    fn ctx(&self) -> RequestContext {
        RequestContext::new().with_tenant(&self.tenant)
    }

    /// 注册一个账号（**真的走 IAM 的注册** → 真的有密码哈希）。
    async fn register_user(&self, username: &str) -> String {
        let user = self
            .iam
            .register(
                &self.ctx(),
                &RegisterRequest {
                    username: username.to_string(),
                    password: PASSWORD.to_string(),
                    email: Some(format!("{username}@example.com")),
                },
            )
            .await
            .expect("账号应当能注册");
        user.id
    }

    /// 登录（**真的走 IAM 的登录路径**）。
    async fn login(&self, username: &str) -> LoginResponse {
        self.iam
            .login(
                &self.ctx(),
                &LoginRequest {
                    username: username.to_string(),
                    password: PASSWORD.to_string(),
                    mfa_code: None,
                },
                &RequestMeta::default(),
            )
            .await
            .expect("登录应当成功")
    }

    /// 造一个**自定义**角色（**不是内置角色**）并授权。
    async fn role_with(&self, id: &str, codes: &[&str]) {
        let role = Role {
            tenant_id: self.tenant.clone(),
            id: id.to_string(),
            code: id.to_string(),
            name: id.to_string(),
            data_scope: DataScope::All,
            custom_filter: None,
            use_all_memberships: false,
            builtin: false,
        };
        self.rbac
            .create_role(&self.ctx(), &role)
            .await
            .expect("角色应当能建成");
        for code in codes {
            self.rbac
                .grant_permission(&self.ctx(), id, code, None)
                .await
                .expect("授权应当成功");
        }
    }

    /// 绑定用户与角色。
    async fn bind(&self, user_id: &str, role_id: &str) {
        self.rbac
            .bind_user_role(&self.ctx(), user_id, role_id)
            .await
            .expect("绑定应当成功");
    }

    /// 给用户发一个令牌（HTTP 层用它代替 JWT 验签）。
    fn token_for(&self, user_id: &str) -> String {
        let token = format!("token-{user_id}");
        self.tokens
            .lock()
            .expect("锁不该被毒化")
            .insert(token.clone(), user_id.to_string());
        token
    }
}

/// 造一个用例的世界。
///
/// 装配顺序与 `platform-server` 一致：先 IAM，**再**注入权限点提供者
/// （`platform-iam` 不能依赖 `platform-rbac` —— 依赖方向是 rbac → iam）。
async fn world() -> World {
    let fixture = setup().await;

    // 租户与账号 id 每个用例独有（并行时不能撞）。
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let tenant = format!("tenant-me-{tag}");

    let rbac = Arc::new(RbacService::new(
        RbacStore::new(fixture.app_db.clone()),
        ScopePolicy::default(),
    ));
    rbac.ensure_builtin_roles(&RequestContext::new().with_tenant(&tenant))
        .await
        .expect("内置角色应当能建成");

    let iam = Arc::new(
        IamService::new(IamStore::new(fixture.app_db.clone()), test_config())
            .expect("IAM 应当能构造"),
    );

    // 注入权限点提供者（REQ-011 的装配动作）。
    let provider: Arc<dyn PermissionProvider> = rbac.clone();
    assert!(
        iam.set_permission_provider(provider),
        "权限点提供者应当注入成功"
    );

    let tokens = Arc::new(Mutex::new(BTreeMap::new()));
    let state = Arc::new(RbacState {
        service: rbac.clone(),
        checker: Arc::new(RbacChecker::new(rbac.clone())),
        tokens: Arc::new(Tokens {
            tenant: tenant.clone(),
            users: tokens.clone(),
        }),
    });

    // 平台把 `/api/rbac` 挂载在根路由下（与 `platform-server` 同一套挂法）。
    let app = axum::Router::new().nest("/api/rbac", router(state));

    World {
        tenant,
        iam,
        rbac,
        app,
        tokens,
    }
}

/// 测试用的 IAM 配置（Argon2 参数用低配值 —— 低配机上跑不动生产参数）。
fn test_config() -> IamConfig {
    let pem = platform_iam::jwt::generate_key_pair().expect("测试密钥应当能生成");
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
            access_ttl: Duration::from_secs(900),
            refresh_ttl: Duration::from_secs(30 * 24 * 3600),
            reset_ttl: Duration::from_secs(3600),
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
            ip_window: Duration::from_secs(300),
            account_failure_limit: 5,
            lockout_duration: Duration::from_secs(900),
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

/// 读状态码与响应体。
async fn finish(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("响应体应当能读");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// 打一次 `GET /api/rbac/me/permissions`。
///
/// `tenant` = 请求头 `x-tenant-id`；`token` = `Authorization: Bearer <token>`。
async fn call_me(
    app: &axum::Router,
    tenant: Option<&str>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method("GET").uri(ME_PERMISSIONS_PATH);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(tenant) = tenant {
        builder = builder.header("x-tenant-id", tenant);
    }
    let request = builder.body(Body::empty()).expect("请求应当能构造");
    let response = app.clone().oneshot(request).await.expect("请求应当能完成");
    finish(response).await
}

/// 读响应里的权限点数组（断言它**真的是数组**）。
fn permissions_of(body: &Value) -> Vec<String> {
    let array = body["permissions"]
        .as_array()
        .expect("permissions 必须是数组");
    array
        .iter()
        .map(|item| item.as_str().expect("权限点必须是字符串").to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// TC-011-001 ~ TC-011-005：GET /api/rbac/me/permissions
// ---------------------------------------------------------------------------

/// TC-011-001：200，响应体是**对象**（不是裸数组），恰好一个键 `permissions`。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_001_the_response_is_an_object_with_exactly_one_key() {
    let world = world().await;
    let user_id = world.register_user("u-perm-read@example.com").await;
    world.role_with("role-perm-read", &[PERM_ROLE_READ]).await;
    world.bind(&user_id, "role-perm-read").await;
    let token = world.token_for(&user_id);

    let (status, body) = call_me(&world.app, Some(&world.tenant), Some(&token)).await;

    assert_eq!(status, StatusCode::OK, "应当是 200：{body}");
    assert!(body.is_object(), "响应体必须是对象（不是裸数组）：{body}");
    assert!(!body.is_array(), "响应体不能是裸数组：{body}");
    let mut keys: Vec<&str> = body
        .as_object()
        .expect("响应体必须是对象")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(keys, vec!["permissions"], "响应体必须恰好一个键：{body}");
    assert!(
        body["permissions"].is_array(),
        "permissions 必须是数组：{body}"
    );
    assert!(
        body["permissions"]
            .as_array()
            .expect("permissions 必须是数组")
            .iter()
            .all(|item| item.is_string()),
        "每一项都必须是字符串：{body}"
    );
}

/// TC-011-002：集合**整体相等** `["rbac:role:read"]`（不是「包含 rbac:role:read」）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_002_the_permission_set_is_exactly_the_granted_one() {
    let world = world().await;
    let user_id = world.register_user("u-perm-read@example.com").await;
    world.role_with("role-perm-read", &[PERM_ROLE_READ]).await;
    world.bind(&user_id, "role-perm-read").await;
    let token = world.token_for(&user_id);

    let (status, body) = call_me(&world.app, Some(&world.tenant), Some(&token)).await;

    assert_eq!(status, StatusCode::OK, "应当是 200：{body}");
    let permissions = permissions_of(&body);
    assert_eq!(
        permissions,
        vec![PERM_ROLE_READ.to_string()],
        "权限点集合必须整体相等（只授予了 rbac:role:read）：{body}"
    );
    assert_eq!(permissions.len(), 1, "长度必须是 1：{body}");
    assert!(
        !permissions.iter().any(|code| code == PERM_ROLE_WRITE),
        "没有授予 rbac:role:write 就不得出现：{body}"
    );
}

/// TC-011-003：不带令牌 → 401 `UNAUTHENTICATED`。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_003_without_a_token_it_is_401_unauthenticated() {
    let world = world().await;

    let (status, body) = call_me(&world.app, Some(&world.tenant), None).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "应当是 401：{body}");
    assert_eq!(body["code"], "UNAUTHENTICATED");
    assert_eq!(body["status"], 401);
    assert_eq!(body["instance"], ME_PERMISSIONS_PATH);
}

/// TC-011-004：请求头租户 ≠ 令牌租户 → 404 `NOT_FOUND`（不泄露资源存在性）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_004_another_tenant_in_the_header_is_404_not_found() {
    let world = world().await;
    let user_id = world.register_user("u-perm-read@example.com").await;
    world.role_with("role-perm-read", &[PERM_ROLE_READ]).await;
    world.bind(&user_id, "role-perm-read").await;
    let token = world.token_for(&user_id);

    let (status, body) = call_me(&world.app, Some("tenant-b"), Some(&token)).await;

    assert_eq!(status, StatusCode::NOT_FOUND, "跨租户必须是 404：{body}");
    assert_eq!(body["code"], "NOT_FOUND");
    assert!(
        body.get("permissions").is_none(),
        "404 体不得带 permissions 字段：{body}"
    );
}

/// TC-011-005：升序 + 去重。
///
/// # 为什么多造一个角色
///
/// 用例给的前提是「两个角色**重复授予**同一权限点」—— 那只验得了**去重**。
/// 「升序」在单元素数组上恒真（**空断言**）。所以本用例再加一个授予
/// **不同**权限点的角色，让两条不变量都**不是空断言**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_005_the_permission_array_is_sorted_and_deduplicated() {
    let world = world().await;
    let user_id = world.register_user("u-perm-read@example.com").await;
    // 两个角色重复授予同一个权限点（验去重）+ 一个角色授予另一个权限点（验升序非空断言）。
    world.role_with("role-perm-read-a", &[PERM_ROLE_READ]).await;
    world.role_with("role-perm-read-b", &[PERM_ROLE_READ]).await;
    world.role_with("role-perm-write", &[PERM_ROLE_WRITE]).await;
    world.bind(&user_id, "role-perm-read-a").await;
    world.bind(&user_id, "role-perm-read-b").await;
    world.bind(&user_id, "role-perm-write").await;
    let token = world.token_for(&user_id);

    let (status, body) = call_me(&world.app, Some(&world.tenant), Some(&token)).await;

    assert_eq!(status, StatusCode::OK, "应当是 200：{body}");
    let permissions = permissions_of(&body);
    for pair in permissions.windows(2) {
        assert!(
            pair[0] < pair[1],
            "权限点必须按字节序升序（不得有重复）：{permissions:?}"
        );
    }
    let unique: std::collections::BTreeSet<&String> = permissions.iter().collect();
    assert_eq!(
        unique.len(),
        permissions.len(),
        "权限点不得重复：{permissions:?}"
    );
    assert_eq!(
        permissions,
        vec![PERM_ROLE_READ.to_string(), PERM_ROLE_WRITE.to_string()],
        "去重后的集合必须恰好是两个权限点：{permissions:?}"
    );
}

// ---------------------------------------------------------------------------
// TC-011-006 ~ TC-011-008：登录响应带权限点
// ---------------------------------------------------------------------------

/// TC-011-006：登录响应带**真实**的权限点集合。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_006_the_login_response_carries_the_real_permission_set() {
    let world = world().await;
    let user_id = world.register_user("u-perm-read@example.com").await;
    world.role_with("role-perm-read", &[PERM_ROLE_READ]).await;
    world.bind(&user_id, "role-perm-read").await;

    let response = world.login("u-perm-read@example.com").await;

    assert!(
        !response.permissions.is_empty(),
        "权限点集合不得为空：{response:?}"
    );
    assert_eq!(
        response.permissions,
        vec![PERM_ROLE_READ.to_string()],
        "登录响应的权限点必须是授予的那一份：{response:?}"
    );
    assert!(
        !response
            .permissions
            .iter()
            .any(|code| code == PERM_ROLE_WRITE),
        "没有授予 rbac:role:write 就不得出现：{response:?}"
    );
}

/// TC-011-007：没有任何角色的用户登录 → `permissions` 是**空数组**。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_007_a_user_without_roles_gets_an_empty_permission_array() {
    let world = world().await;
    // 只注册，**不绑定任何角色**（内置 `platform-user` 也不绑）。
    world.register_user("u-perm-none@example.com").await;

    let response = world.login("u-perm-none@example.com").await;

    assert!(
        response.permissions.is_empty(),
        "没有角色就必须是空数组：{response:?}"
    );
}

/// TC-011-008：登录响应与 `/me/permissions` **两个入口给出同一答案**。
///
/// # 前置冻结条件
///
/// 两次调用之间**不发生任何**角色绑定 / 解绑 / 授权变更（本用例自己保证：
/// 两次调用之前不做任何写操作）。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_008_both_entry_points_give_the_same_answer() {
    let world = world().await;
    let user_id = world.register_user("u-perm-read@example.com").await;
    world.role_with("role-perm-read", &[PERM_ROLE_READ]).await;
    world.bind(&user_id, "role-perm-read").await;

    let response = world.login("u-perm-read@example.com").await;
    let token = world.token_for(&user_id);
    let (status, body) = call_me(&world.app, Some(&world.tenant), Some(&token)).await;

    assert_eq!(status, StatusCode::OK, "应当是 200：{body}");
    let from_endpoint = permissions_of(&body);
    assert_eq!(
        response.permissions, from_endpoint,
        "两个入口必须给出同一答案（都走 effective_permissions）"
    );
    assert_eq!(
        response.permissions,
        vec![PERM_ROLE_READ.to_string()],
        "两个入口的答案都必须是授予的那一份"
    );
}

/// TC-011-009：通配授权被**展开**成具体权限点。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_009_a_wildcard_grant_is_expanded() {
    let world = world().await;
    let user_id = world.register_user("u-perm-wildcard@example.com").await;
    world
        .role_with("role-perm-wildcard", &["rbac:role:*"])
        .await;
    world.bind(&user_id, "role-perm-wildcard").await;
    let token = world.token_for(&user_id);

    let (status, body) = call_me(&world.app, Some(&world.tenant), Some(&token)).await;

    assert_eq!(status, StatusCode::OK, "应当是 200：{body}");
    let permissions = permissions_of(&body);
    assert_eq!(
        permissions,
        vec![PERM_ROLE_READ.to_string(), PERM_ROLE_WRITE.to_string()],
        "通配必须展开成注册表里的具体权限点：{permissions:?}"
    );
    assert_eq!(permissions.len(), 2, "长度必须是 2：{permissions:?}");
}

// ---------------------------------------------------------------------------
// TC-011-010：降级路径
// ---------------------------------------------------------------------------

/// TC-011-010：**未注入** `PermissionProvider` 时，登录**仍然成功**，
/// `permissions` 是**空数组**（不是 `null`、不是缺字段）。
///
/// # 为什么单独造一个 IAM 服务
///
/// 降级路径的前提就是「provider 没装上」—— 与其它用例的装配**相反**，
/// 所以这里**不**调用 `set_permission_provider`。
#[tokio::test]
#[ignore = "需要 base_test 库：scripts/test-integration.sh 会用 --include-ignored 跑到"]
async fn tc_011_010_without_a_provider_login_still_succeeds_with_an_empty_array() {
    let fixture = setup().await;
    let tenant = format!("tenant-me-{}", uuid::Uuid::new_v4().simple());
    let ctx = RequestContext::new().with_tenant(&tenant);

    // **不注入** provider。
    let iam = IamService::new(IamStore::new(fixture.app_db.clone()), test_config())
        .expect("IAM 应当能构造");
    iam.register(
        &ctx,
        &RegisterRequest {
            username: "u-perm-none@example.com".to_string(),
            password: PASSWORD.to_string(),
            email: Some("u-perm-none@example.com".to_string()),
        },
    )
    .await
    .expect("账号应当能注册");

    let response = iam
        .login(
            &ctx,
            &LoginRequest {
                username: "u-perm-none@example.com".to_string(),
                password: PASSWORD.to_string(),
                mfa_code: None,
            },
            &RequestMeta::default(),
        )
        .await
        .expect("未注入 provider 时登录**仍然成功**");

    assert_eq!(
        response.permissions,
        Vec::<String>::new(),
        "未注入 provider 时权限点必须是空数组：{response:?}"
    );
}
