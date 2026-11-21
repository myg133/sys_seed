//! REQ-016 的**真库 + 真 HTTP** 集成用例（AC-S-156 / 157 / 158 / 159 / 160 / 161 / 162 / 164）。
//!
//! # 设计要点
//!
//! - 每条断言都打在**端点**上（不是函数）—— 「端点默认拒绝 X」必须真的打端点；
//! - 造数 / 篡改走**管理员连接**，业务访问走**应用角色**（RLS 生效）；
//! - 用例之间不共享夹具名（每次 `setup()` 清空 3 张表）。

mod common;

use common::*;
use serde_json::{Value, json};

/// 管理员账号（持 3 个 `app:*` 权限点）。
const ADMIN_USER: &str = "u-perm-app@example.com";
/// 只读账号（只持 `app:registry:read`）。
const READ_USER: &str = "u-perm-read@example.com";
/// 无任何权限点的账号。
const NONE_USER: &str = "u-perm-none@example.com";
/// 持 `audit:events:read` 的账号（导航过滤的**阳性**对照）。
const AUDIT_USER: &str = "u-perm-audit@example.com";

/// 给管理员授 3 个权限点；只读账号授 1 个。
fn grant_baseline(fixture: &Fixture) {
    fixture.grant(
        ADMIN_USER,
        &[
            "app:registry:read",
            "app:registry:write",
            "app:registry:delete",
        ],
    );
    fixture.grant(READ_USER, &["app:registry:read"]);
    fixture.grant(AUDIT_USER, &["audit:events:read"]);
}

// ---------------------------------------------------------------------------
// AC-S-156：注册表 CRUD
// ---------------------------------------------------------------------------

/// AC-S-156 步骤 1-4：注册 → 201 + 回读逐字一致（列表是**数组**，不是 `{apps:[]}`）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn an_app_can_be_created_and_read_back_verbatim() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture
        .seed_client("client-A", "active", &["rbac:role:read"], None)
        .await;

    let (status, created) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&token_for(TENANT_A, ADMIN_USER)),
            Some(json!({
                "clientId": "client-A",
                "displayName": "报销系统",
                "description": "差旅报销",
                "category": "finance",
                "iconUrl": "https://cdn.example/i.png",
                "launchUrl": "https://expense.example/app",
                "sortOrder": 10,
            })),
        )
        .await;
    assert_eq!(status, 201, "注册必须 201：{created}");
    let app_id = created["appId"].as_str().expect("必须有 appId");
    assert!(!app_id.is_empty() && app_id.len() >= 8, "appId 不得为空");
    assert_eq!(created["clientId"], "client-A");
    assert_eq!(created["displayName"], "报销系统");
    // ⚠️ `description` 是**用户可见字段**，必须逐字回读断言（独立复核 FINDING-1：
    // 原来 11 行 AC 全绿、全部测试全绿，但把这个字段改坏也测不出来）。
    assert_eq!(
        created["description"], "差旅报销",
        "description 必须逐字回读：{created}"
    );
    assert_eq!(created["category"], "finance");
    assert_eq!(created["iconUrl"], "https://cdn.example/i.png");
    assert_eq!(created["launchUrl"], "https://expense.example/app");
    assert_eq!(created["sortOrder"], 10);
    assert_eq!(created["status"], "active");
    assert_eq!(created["createdBy"], ADMIN_USER);
    assert!(created["scopes"].is_array(), "scopes 必须是数组");
    // 响应**不含** client 的敏感字段（引用而非复制）。
    for forbidden in [
        "clientSecret",
        "clientSecretHash",
        "redirectUris",
        "allowedScopes",
        "backchannelLogoutUri",
        "frontchannelLogoutUri",
    ] {
        assert!(
            created.get(forbidden).is_none(),
            "响应体不得含 client 字段 `{forbidden}`：{created}"
        );
    }
    // createdAt 是 RFC3339 + 微秒 6 位。
    let created_at = created["createdAt"].as_str().expect("必须有 createdAt");
    assert!(created_at.ends_with('Z'), "createdAt 必须是 UTC：{created_at}");
    assert_eq!(
        created_at.len(),
        "2026-11-21T00:00:00.000000Z".len(),
        "createdAt 必须是微秒 6 位：{created_at}"
    );

    // 列表是**数组**（不是 `{apps:[]}`）。
    let (status, list) = fixture
        .call(
            "GET",
            "/api/apps",
            Some(&token_for(TENANT_A, READ_USER)),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let items = list.as_array().expect("列表必须是数组");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0], created, "列表项必须与创建响应逐字相同");

    // 单个读：与列表项逐字相同。
    let (status, single) = fixture
        .call(
            "GET",
            &format!("/api/apps/{app_id}"),
            Some(&token_for(TENANT_A, READ_USER)),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(single, created, "单个响应必须与创建响应逐字相同");
}

/// AC-S-156 步骤 5：PATCH 改 `displayName` + `sortOrder` → 200，其它字段不变。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn patching_an_app_changes_only_the_given_fields() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture
        .seed_client("client-A", "active", &["rbac:role:read"], None)
        .await;
    fixture
        .seed_app(
            "app-patch",
            TENANT_A,
            "client-A",
            "原名字",
            "finance",
            10,
            "active",
            Some("https://a.example/"),
        )
        .await;

    let (status, patched) = fixture
        .call(
            "PATCH",
            "/api/apps/app-patch",
            Some(&token_for(TENANT_A, ADMIN_USER)),
            Some(json!({ "displayName": "报销系统 v2", "sortOrder": 99 })),
        )
        .await;
    assert_eq!(status, 200, "PATCH 必须 200：{patched}");
    assert_eq!(patched["displayName"], "报销系统 v2");
    assert_eq!(patched["sortOrder"], 99);
    // **其它字段不变**。
    assert_eq!(patched["launchUrl"], "https://a.example/");
    assert_eq!(patched["category"], "finance");
    assert_eq!(patched["clientId"], "client-A");
}

/// AC-S-156 步骤 6：DELETE → 204；再 GET → 404 + `APP_NOT_FOUND`；再 DELETE → 404（**不是 500**）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn deleting_an_app_then_reading_it_yields_app_not_found() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture
        .seed_client("client-A", "active", &["rbac:role:read"], None)
        .await;
    fixture
        .seed_app(
            "app-del",
            TENANT_A,
            "client-A",
            "要删的",
            "general",
            0,
            "active",
            None,
        )
        .await;

    let admin = token_for(TENANT_A, ADMIN_USER);
    let (status, _) = fixture
        .call("DELETE", "/api/apps/app-del", Some(&admin), None)
        .await;
    assert_eq!(status, 204, "删除必须 204");

    let (status, body) = fixture
        .call("GET", "/api/apps/app-del", Some(&admin), None)
        .await;
    assert_eq!(status, 404);
    assert_eq!(body["code"], "APP_NOT_FOUND", "必须是 APP_NOT_FOUND：{body}");

    // ⚠️ 第二次删除也是 404 + APP_NOT_FOUND（**不是** 500）。
    let (status, body) = fixture
        .call("DELETE", "/api/apps/app-del", Some(&admin), None)
        .await;
    assert_eq!(status, 404, "二次删除必须 404：{body}");
    assert_eq!(body["code"], "APP_NOT_FOUND");
}

/// AC-S-156 步骤 7 / AC-S-159 步骤 5：`platform_app` **恰好 12 列**，且**不含** client 的列与任何 scope 列。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn the_platform_app_table_has_exactly_twelve_columns() {
    let fixture = setup().await;

    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'platform_app' \
         ORDER BY column_name",
    )
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("列集合应能读出");

    let expected: Vec<String> = [
        "category",
        "client_id",
        "created_at",
        "created_by",
        "description",
        "display_name",
        "icon_url",
        "id",
        "launch_url",
        "sort_order",
        "status",
        "tenant_id",
    ]
    .iter()
    .map(|value| value.to_string())
    .collect();
    assert_eq!(columns, expected, "列集合必须**恰好**是 D265 的 12 列（双向相等）");

    // **不含** client 的敏感字段（引用而非复制）。
    for forbidden in [
        "client_secret_hash",
        "redirect_uris",
        "allowed_grant_types",
        "allowed_scopes",
        "backchannel_logout_uri",
        "frontchannel_logout_uri",
    ] {
        assert!(
            !columns.iter().any(|column| column == forbidden),
            "platform_app 不得含 client 的列 `{forbidden}`"
        );
    }
    // **不含**任何名称里带 `scope` 的列（D266 / AC-S-159 步骤 5）。
    let scope_columns: Vec<&String> = columns
        .iter()
        .filter(|column| column.contains("scope"))
        .collect();
    assert!(
        scope_columns.is_empty(),
        "platform_app 不得有任何 scope 列（只读投影，D266）：{scope_columns:?}"
    );
}

// ---------------------------------------------------------------------------
// AC-S-156 路径 2~5：错误码
// ---------------------------------------------------------------------------

/// AC-S-156 路径 2：同一个 `clientId` 再 POST → 409 `APP_DUPLICATED`；
/// 换 client 但同名 → 也 409 `APP_DUPLICATED`。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn a_duplicate_client_or_display_name_is_rejected_with_conflict() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture
        .seed_client("client-A", "active", &[], None)
        .await;
    fixture
        .seed_client("client-B", "active", &[], None)
        .await;
    fixture
        .seed_app(
            "app-first",
            TENANT_A,
            "client-A",
            "报销系统",
            "general",
            0,
            "active",
            None,
        )
        .await;

    let admin = token_for(TENANT_A, ADMIN_USER);

    // 2a：同一个 clientId（**唯一的 displayName**，否则测的是另一个冲突）。
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&admin),
            Some(json!({ "clientId": "client-A", "displayName": "另一个名字" })),
        )
        .await;
    assert_eq!(status, 409, "重复 clientId 必须 409：{body}");
    assert_eq!(body["code"], "APP_DUPLICATED");
    assert_eq!(body["params"]["field"], "clientId");

    // 2b：换 client，但 displayName 重复。
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&admin),
            Some(json!({ "clientId": "client-B", "displayName": "报销系统" })),
        )
        .await;
    assert_eq!(status, 409, "重复 displayName 必须 409：{body}");
    assert_eq!(body["code"], "APP_DUPLICATED");
    assert_eq!(body["params"]["field"], "displayName");

    // 对照：唯一 client + 唯一名字 → 201（证明 409 不是「一律拒绝」）。
    fixture
        .seed_client("client-C", "active", &[], None)
        .await;
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&admin),
            Some(json!({ "clientId": "client-C", "displayName": "第三个名字" })),
        )
        .await;
    assert_eq!(status, 201, "唯一 client + 唯一名字必须 201：{body}");
}

/// AC-S-156 路径 3：`launch_url` 的 scheme。
///
/// 5 种非法**全部 400 `APP_LAUNCH_URL_INVALID`** + **阳性对照**（https → 201）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn an_invalid_launch_url_scheme_is_rejected() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    let admin = token_for(TENANT_A, ADMIN_USER);

    let illegal = [
        "not-a-url",
        "javascript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "ftp://x.example/a",
        "http://x.example/a", // 默认 app.icon.allowHttp = false
    ];
    for (index, launch_url) in illegal.iter().enumerate() {
        let client = format!("client-probe-url-{index}");
        fixture.seed_client(&client, "active", &[], None).await;
        let (status, body) = fixture
            .call(
                "POST",
                "/api/apps",
                Some(&admin),
                Some(json!({
                    "clientId": client,
                    "displayName": format!("探针-{index}"),
                    "launchUrl": launch_url,
                })),
            )
            .await;
        assert_eq!(status, 400, "`{launch_url}` 必须 400：{body}");
        assert_eq!(
            body["code"], "APP_LAUNCH_URL_INVALID",
            "`{launch_url}` 必须报 APP_LAUNCH_URL_INVALID：{body}"
        );
    }

    // **阳性对照**：https 必须 201（否则「一律拒绝」的实现也能过）。
    fixture
        .seed_client("client-probe-url-ok", "active", &[], None)
        .await;
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&admin),
            Some(json!({
                "clientId": "client-probe-url-ok",
                "displayName": "探针-合法",
                "launchUrl": "https://x.example/a",
            })),
        )
        .await;
    assert_eq!(status, 201, "https 必须 201（阳性对照）：{body}");
}

/// AC-S-156 路径 4：`icon_url` 的 scheme（与路径 3 同款，但报 `APP_ICON_URL_INVALID`）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn an_invalid_icon_url_scheme_is_rejected() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    let admin = token_for(TENANT_A, ADMIN_USER);

    let illegal = [
        "not-a-url",
        "javascript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "ftp://x.example/a",
        "http://x.example/a",
    ];
    for (index, icon_url) in illegal.iter().enumerate() {
        let client = format!("client-probe-icon-{index}");
        fixture.seed_client(&client, "active", &[], None).await;
        let (status, body) = fixture
            .call(
                "POST",
                "/api/apps",
                Some(&admin),
                Some(json!({
                    "clientId": client,
                    "displayName": format!("图标探针-{index}"),
                    "launchUrl": "https://x.example/a",
                    "iconUrl": icon_url,
                })),
            )
            .await;
        assert_eq!(status, 400, "`{icon_url}` 必须 400：{body}");
        assert_eq!(
            body["code"], "APP_ICON_URL_INVALID",
            "`{icon_url}` 必须报 APP_ICON_URL_INVALID：{body}"
        );
    }

    // **阳性对照**。
    fixture
        .seed_client("client-probe-icon-ok", "active", &[], None)
        .await;
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&admin),
            Some(json!({
                "clientId": "client-probe-icon-ok",
                "displayName": "图标探针-合法",
                "iconUrl": "https://cdn.example/i.png",
            })),
        )
        .await;
    assert_eq!(status, 201, "https 图标必须 201（阳性对照）：{body}");
}

/// AC-S-156 路径 5：缺必填 / 类型错 → 400 `APP_INVALID_REQUEST`（Problem Details，不是纯文本）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn a_malformed_create_body_is_rejected_with_invalid_request() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    let admin = token_for(TENANT_A, ADMIN_USER);
    fixture.seed_client("client-A", "active", &[], None).await;

    let illegal = [
        // 省略 clientId
        json!({ "displayName": "无 client" }),
        // 省略 displayName
        json!({ "clientId": "client-A" }),
        // sortOrder 类型错
        json!({ "clientId": "client-A", "displayName": "类型错", "sortOrder": "abc" }),
        // sortOrder 溢出 int4
        json!({ "clientId": "client-A", "displayName": "溢出", "sortOrder": 1e100 }),
    ];
    for body in illegal {
        let (status, response) = fixture
            .call("POST", "/api/apps", Some(&admin), Some(body.clone()))
            .await;
        assert_eq!(status, 400, "非法请求必须 400：{body} → {response}");
        assert_eq!(
            response["code"], "APP_INVALID_REQUEST",
            "必须报 APP_INVALID_REQUEST：{body} → {response}"
        );
        // Problem Details 形状（不是 axum 的纯文本）。
        assert!(
            response["title"].is_string(),
            "必须是 Problem Details（有 title）：{response}"
        );
    }
}

/// TC-016-014 / D280：**校验顺序钉死** —— 同请求多个错误时报**最靠前**的那个。
///
/// 顺序：必填/类型 → client 可用 → URL scheme → 唯一性。
/// 用**已占用的 client-A** + `javascript:` + 已存在的 `displayName` → 必须报
/// `APP_LAUNCH_URL_INVALID`（不是 `APP_DUPLICATED`）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn the_validation_order_is_pinned() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture.seed_client("client-A", "active", &[], None).await;
    fixture
        .seed_app(
            "app-order",
            TENANT_A,
            "client-A",
            "已占用",
            "general",
            0,
            "active",
            None,
        )
        .await;

    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&token_for(TENANT_A, ADMIN_USER)),
            Some(json!({
                "clientId": "client-A",
                "displayName": "已占用",
                "launchUrl": "javascript:alert(1)",
            })),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(
        body["code"], "APP_LAUNCH_URL_INVALID",
        "URL scheme 必须**先于**唯一性被判定（TC-016-014）：{body}"
    );

    // 不存在的 client + 非法 URL → 必须报 CLIENT_INVALID（client 判定先于 URL）。
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&token_for(TENANT_A, ADMIN_USER)),
            Some(json!({
                "clientId": "no-such-client",
                "displayName": "顺序探针",
                "launchUrl": "javascript:alert(1)",
            })),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(
        body["code"], "APP_CLIENT_INVALID",
        "client 可用性必须**先于** URL scheme（TC-016-014）：{body}"
    );
}

// ---------------------------------------------------------------------------
// AC-S-157：租户隔离
// ---------------------------------------------------------------------------

/// AC-S-157 步骤 1/2/3/4：读侧隔离（应用角色 + `set_config`）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn tenant_isolation_hides_other_tenants_rows() {
    let fixture = setup().await;
    fixture
        .seed_client("client-a-only", "active", &[], None)
        .await;
    fixture
        .seed_client("client-b-only", "active", &[], None)
        .await;
    fixture
        .seed_app(
            "app-a",
            TENANT_A,
            "client-a-only",
            "A 的应用",
            "general",
            0,
            "active",
            None,
        )
        .await;
    fixture
        .seed_app(
            "app-b",
            TENANT_B,
            "client-b-only",
            "B 的应用",
            "general",
            0,
            "active",
            None,
        )
        .await;

    // 步骤 3（对照，先做）：以 `base` 能看到**两租户**的行 —— 证明下面不是「表本来就空」。
    let all = fixture
        .scalar("SELECT count(DISTINCT tenant_id) FROM platform_app")
        .await;
    assert_eq!(all, 2, "超管必须能看到两个租户的行（非空对照）");

    // 步骤 1/2：应用角色 + 单事务内 set_config。
    let ctx_a = platform_core::RequestContext::new().with_tenant(TENANT_A);
    let ctx_b = platform_core::RequestContext::new().with_tenant(TENANT_B);
    let rows_a: Vec<String> = fixture
        .app_db
        .fetch_all::<TenantIdRow>(
            &ctx_a,
            &platform_db::TenantQuery::new(
                "SELECT tenant_id FROM platform_app WHERE tenant_id = {tenant}",
            ),
        )
        .await
        .expect("应能查询")
        .into_iter()
        .map(|row| row.tenant_id)
        .collect();
    assert_eq!(rows_a, vec![TENANT_A.to_string()], "tenant-a 只看到自己的行");

    let rows_b: Vec<String> = fixture
        .app_db
        .fetch_all::<TenantIdRow>(
            &ctx_b,
            &platform_db::TenantQuery::new(
                "SELECT tenant_id FROM platform_app WHERE tenant_id = {tenant}",
            ),
        )
        .await
        .expect("应能查询")
        .into_iter()
        .map(|row| row.tenant_id)
        .collect();
    assert_eq!(rows_b, vec![TENANT_B.to_string()], "tenant-b 只看到自己的行");
}

/// 租户 ID 的一列（RLS 用例专用）。
#[derive(Debug, Clone)]
struct TenantIdRow {
    tenant_id: String,
}

platform_db::tenant_row!(TenantIdRow { tenant_id: "tenant_id" });

/// AC-S-157 步骤 4（**写侧 fail closed**）：应用角色**不设上下文** → 0 行。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn the_app_role_sees_nothing_without_a_tenant_context() {
    let fixture = setup().await;
    fixture
        .seed_client("client-iso", "active", &[], None)
        .await;
    fixture
        .seed_app(
            "app-iso",
            TENANT_A,
            "client-iso",
            "隔离探针",
            "general",
            0,
            "active",
            None,
        )
        .await;

    // 用 `rls::unscoped_row_count`（不设租户上下文直查）→ 必须 **0**（fail closed）。
    let visible = platform_db::unscoped_row_count(&fixture.app_db, "platform_app")
        .await
        .expect("自检查询应能执行");
    assert_eq!(visible, 0, "不设租户上下文时应用角色必须看到 0 行（fail closed）");
}

/// AC-S-157 步骤 5（**写侧 WITH CHECK**）：跨租户 INSERT 被 RLS 拒（SQLSTATE 42501）。
///
/// ⚠️ 判据必须是 **42501 / row-level security** —— **排除** 23503（外键）/ 23502（非空）
/// （QA 的 S-23：原始 AC 的 INSERT 缺必填列，会因 FK 失败而非 RLS）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn a_cross_tenant_insert_is_rejected_by_row_level_security() {
    let fixture = setup().await;
    fixture
        .seed_client("client-wc-a", "active", &[], None)
        .await;
    fixture
        .seed_client("client-wc-b", "active", &[], None)
        .await;

    // 在 tenant-a 的上下文里插一行 tenant_id='tenant-b' → 必须被 WITH CHECK 拒。
    //
    // ⚠️ 语句里必须带 `{tenant}`占位符（平台的 TenantQuery 强制）——
    // 它渲染成「上下文租户」，而**行里的** `tenant_id` 由 `?` 绑成 `tenant-b`
    // → 两者不一致 → RLS 的 `WITH CHECK` 拒绝。
    let ctx = platform_core::RequestContext::new().with_tenant(TENANT_A);
    let insert_b = platform_db::TenantQuery::new(
        "INSERT INTO platform_app \
         (id, tenant_id, client_id, display_name, created_at, created_by) \
         SELECT ?, ?, ?, ?, now(), ? WHERE {tenant} IS NOT NULL",
    )
    .bind("app-cross".to_string())
    .bind(TENANT_B.to_string())
    .bind("client-wc-b".to_string())
    .bind("跨租户探针".to_string())
    .bind("probe".to_string());

    let error = fixture
        .app_db
        .execute(&ctx, &insert_b)
        .await
        .expect_err("跨租户 INSERT 必须被拒");
    let message = format!("{error}");
    let is_rls = message.contains("row-level security")
        || message.contains("42501")
        || message.contains("new row violates row-level security policy");
    assert!(
        is_rls,
        "拒绝理由必须是 RLS（42501 / row-level security），不是外键/非空：{message}"
    );

    // 对照：本租户的 INSERT **成功**。
    let insert_a = platform_db::TenantQuery::new(
        "INSERT INTO platform_app \
         (id, tenant_id, client_id, display_name, created_at, created_by) \
         SELECT ?, ?, ?, ?, now(), ? WHERE {tenant} IS NOT NULL",
    )
    .bind("app-own".to_string())
    .bind(TENANT_A.to_string())
    .bind("client-wc-a".to_string())
    .bind("本租户探针".to_string())
    .bind("probe".to_string());
    fixture
        .app_db
        .execute(&ctx, &insert_a)
        .await
        .expect("本租户 INSERT 必须成功");
}

// ---------------------------------------------------------------------------
// AC-S-158：client 生命周期
// ---------------------------------------------------------------------------

/// AC-S-158 路径 1：绑定不存在的 client → 400 `APP_CLIENT_INVALID`。
/// 路径 2：绑定 disabled 的 client → 400 `APP_CLIENT_INVALID`。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn an_app_must_bind_to_an_existing_and_usable_client() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    let admin = token_for(TENANT_A, ADMIN_USER);

    // 路径 1：不存在的 client。
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&admin),
            Some(json!({ "clientId": "no-such-client", "displayName": "孤儿探针" })),
        )
        .await;
    assert_eq!(status, 400, "不存在的 client 必须 400：{body}");
    assert_eq!(body["code"], "APP_CLIENT_INVALID");

    // 路径 2：停用的 client（**专用** client，不碰共享夹具，D281）。
    fixture
        .seed_client("client-lifecycle-probe", "disabled", &[], None)
        .await;
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&admin),
            Some(json!({
                "clientId": "client-lifecycle-probe",
                "displayName": "停用 client 探针",
            })),
        )
        .await;
    assert_eq!(status, 400, "停用的 client 必须 400：{body}");
    assert_eq!(body["code"], "APP_CLIENT_INVALID");

    // 路径 2b：**过期**的 client 也不能接入。
    fixture
        .seed_client("client-expired-probe", "active", &[], Some(-60))
        .await;
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&admin),
            Some(json!({
                "clientId": "client-expired-probe",
                "displayName": "过期 client 探针",
            })),
        )
        .await;
    assert_eq!(status, 400, "过期的 client 必须 400：{body}");
    assert_eq!(body["code"], "APP_CLIENT_INVALID");
}

/// AC-S-158 路径 3/4/5：删除 client **不被外键阻塞** + 应用被**级联删除** + **不碰共享夹具**。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn deleting_a_client_cascades_to_its_apps() {
    let fixture = setup().await;
    // 共享夹具的替身：证明「别的 client 不受影响」。
    fixture
        .seed_client("client-shared", "active", &[], None)
        .await;
    fixture
        .seed_app(
            "app-shared",
            TENANT_A,
            "client-shared",
            "共享夹具的应用",
            "general",
            0,
            "active",
            None,
        )
        .await;

    // 一次性 client + 绑定它的应用（D281：**不删共享夹具**）。
    fixture
        .seed_client("client-cascade-probe", "active", &[], None)
        .await;
    fixture
        .seed_app(
            "app-cascade",
            TENANT_A,
            "client-cascade-probe",
            "级联探针",
            "general",
            0,
            "active",
            None,
        )
        .await;

    // 路径 3：删除 client **成功**（不因外键受阻）。
    let deleted = fixture
        .app_db
        .execute(
            &platform_core::RequestContext::new().with_tenant(platform_app::store::PLATFORM_SCOPE_TENANT),
            &platform_db::TenantQuery::new(
                "DELETE FROM platform_oauth_client WHERE {tenant} IS NOT NULL AND client_id = ?",
            )
            .bind("client-cascade-probe".to_string()),
        )
        .await
        .expect("删除 client 不得因外键受阻（ON DELETE CASCADE）");
    assert_eq!(deleted, 1, "必须真的删掉 1 行 client");

    // 路径 4：应用行**已被级联删除**。
    let remaining = fixture
        .scalar("SELECT count(*) FROM platform_app WHERE id = 'app-cascade'")
        .await;
    assert_eq!(remaining, 0, "client 删除后应用行必须被级联删除（D264）");

    // 路径 5：**别的** client 及其应用**仍在**（没有误伤）。
    let shared = fixture
        .scalar("SELECT count(*) FROM platform_app WHERE id = 'app-shared'")
        .await;
    assert_eq!(shared, 1, "其它 client 的应用不得被删除");
}

/// AC-S-158（D264 的后果）：**跨租户**引用同一 client → 两租户的行**都被删**。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn deleting_a_shared_client_removes_apps_in_every_tenant() {
    let fixture = setup().await;
    fixture
        .seed_client("client-shared-probe", "active", &[], None)
        .await;
    fixture
        .seed_app(
            "app-t-a",
            TENANT_A,
            "client-shared-probe",
            "A 的接入",
            "general",
            0,
            "active",
            None,
        )
        .await;
    fixture
        .seed_app(
            "app-t-b",
            TENANT_B,
            "client-shared-probe",
            "B 的接入",
            "general",
            0,
            "active",
            None,
        )
        .await;

    fixture
        .execute("DELETE FROM platform_oauth_client WHERE client_id = 'client-shared-probe'")
        .await;

    let remaining = fixture
        .scalar("SELECT count(*) FROM platform_app WHERE client_id = 'client-shared-probe'")
        .await;
    assert_eq!(remaining, 0, "client 是平台级、跨租户共享 → 两租户的行都必须被删");
}

// ---------------------------------------------------------------------------
// AC-S-159：scope 只读投影
// ---------------------------------------------------------------------------

/// AC-S-159 步骤 1/4 + TC-016-029：改 client 的 `allowed_scopes` → 响应**立刻**跟着变
/// （单个与列表两条读取路径都要覆盖）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn the_app_scopes_are_a_live_projection_of_the_client() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture
        .seed_client("client-A", "active", &["rbac:role:read"], None)
        .await;
    fixture
        .seed_app(
            "app-scope",
            TENANT_A,
            "client-A",
            "投影探针",
            "general",
            0,
            "active",
            None,
        )
        .await;
    let admin = token_for(TENANT_A, ADMIN_USER);

    // 步骤 1：`scopes` 逐字等于 client 的 allowed_scopes。
    let (status, body) = fixture
        .call("GET", "/api/apps/app-scope", Some(&admin), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["scopes"],
        json!(["rbac:role:read"]),
        "scopes 必须逐字等于 client.allowed_scopes：{body}"
    );

    // 步骤 3：改 client 的 allowed_scopes。
    fixture
        .execute(
            "UPDATE platform_oauth_client \
             SET allowed_scopes = ARRAY['rbac:role:read','rbac:role:write'] \
             WHERE client_id = 'client-A'",
        )
        .await;

    // 步骤 4：单个读**跟着变**（证明是投影不是拷贝）。
    let (status, body) = fixture
        .call("GET", "/api/apps/app-scope", Some(&admin), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["scopes"],
        json!(["rbac:role:read", "rbac:role:write"]),
        "改 client 后 scopes 必须跟着变（投影非拷贝）：{body}"
    );

    // 列表路径**同样**跟着变（TC-016-029）。
    let (status, list) = fixture
        .call("GET", "/api/apps", Some(&admin), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        list[0]["scopes"],
        json!(["rbac:role:read", "rbac:role:write"]),
        "列表的 scopes 也必须来自 client：{list}"
    );
}

// ---------------------------------------------------------------------------
// 权限点（AC-S-165 的运行时那一半）
// ---------------------------------------------------------------------------

/// 只读账号**不能**写（403），但**能**读；无权限账号连读也不行。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn the_registry_endpoints_enforce_their_permissions() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture.seed_client("client-A", "active", &[], None).await;

    // 只读账号：读 200。
    let (status, _) = fixture
        .call(
            "GET",
            "/api/apps",
            Some(&token_for(TENANT_A, READ_USER)),
            None,
        )
        .await;
    assert_eq!(status, 200, "持 read 权限点必须能读");

    // 只读账号：写 403。
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&token_for(TENANT_A, READ_USER)),
            Some(json!({ "clientId": "client-A", "displayName": "越权写" })),
        )
        .await;
    assert_eq!(status, 403, "不持 write 权限点必须 403：{body}");
    assert_eq!(body["code"], "INSUFFICIENT_PERMISSION");

    // 无权限账号：连读都 403。
    let (status, body) = fixture
        .call(
            "GET",
            "/api/apps",
            Some(&token_for(TENANT_A, NONE_USER)),
            None,
        )
        .await;
    assert_eq!(status, 403, "无任何权限点必须 403：{body}");

    // 未认证：401。
    let (status, body) = fixture.call("GET", "/api/apps", None, None).await;
    assert_eq!(status, 401, "无令牌必须 401：{body}");
    assert_eq!(body["code"], "UNAUTHENTICATED");
}

/// **TD-047 的阴性对照**：`ensure_fresh` 失败 → 受权限点保护的端点**500**（fail closed）。
///
/// ⚠️ 这条证明「中间件**真的调了** `ensure_fresh`」—— 删掉那次调用，
/// 本用例会变成 200/403（不再是 500）→ 变红。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn a_failing_permission_refresh_fails_the_protected_endpoint_closed() {
    let mut fixture = setup().await;
    grant_baseline(&fixture);
    let refresher = std::sync::Arc::new(FailingOnceRefresher::default());

    // 重建路由，把刷新器换成「一定失败」的那个。
    let store = std::sync::Arc::new(platform_app::store::AppStore::new(fixture.app_db.clone()));
    let service = std::sync::Arc::new(platform_app::service::AppService::new(
        store,
        std::sync::Arc::new(platform_app::config::StaticAppConfigSource::default()),
        std::sync::Arc::new(fixture.sink.clone()),
        std::sync::Arc::new(default_manifests()),
        std::sync::Arc::new(UserPermissionChecker {
            table: fixture.permissions.clone(),
        }),
        std::sync::Arc::new(MappedTokenVerifier),
    ));
    let state = std::sync::Arc::new(platform_app::http::AppState {
        service,
        checker: std::sync::Arc::new(UserPermissionChecker {
            table: fixture.permissions.clone(),
        }),
        refresher: refresher.clone(),
    });
    fixture.router = axum::Router::new().nest("/api/apps", platform_app::http::router(state));

    let (status, body) = fixture
        .call(
            "GET",
            "/api/apps",
            Some(&token_for(TENANT_A, READ_USER)),
            None,
        )
        .await;
    assert_eq!(
        status, 500,
        "刷新失败必须 fail closed（500），不是 403/200：{body}"
    );
    assert_eq!(body["code"], "INTERNAL_ERROR");
    assert!(
        refresher.calls() >= 1,
        "证明 ensure_fresh 真的被调用了（TD-047）"
    );
}

// ---------------------------------------------------------------------------
// AC-S-160：工作台
// ---------------------------------------------------------------------------

/// 工作台夹具：A~E 五个应用（E 是 hidden）。
async fn seed_workspace(fixture: &Fixture) {
    for client in ["client-A", "client-B", "client-C", "client-D", "client-E"] {
        fixture.seed_client(client, "active", &[], None).await;
    }
    // ⚠️ **不给 `launch_url`**（D279：工作台**不**按它过滤）。
    //
    // ⚠️ **id 与 displayName 的顺序刻意相反**（"app-z" 叫 "C 应用"、"app-a" 叫 "D 应用"）：
    // 这样「按 id 兜底排序」的实现会得到 D → C（错），只有**按 displayName** 才是 C → D（对）
    // → 否则次级排序键删掉也测不出来（夹具无区分力，dev-guide §9.15 的「无区分力夹具」）。
    fixture
        .seed_app("app-a", TENANT_A, "client-A", "差旅报销", "finance", 20, "active", None)
        .await;
    fixture
        .seed_app("app-b", TENANT_A, "client-B", "费用申请", "finance", 10, "active", None)
        .await;
    fixture
        .seed_app("app-z", TENANT_A, "client-C", "C 应用", "general", 0, "active", None)
        .await;
    fixture
        .seed_app("app-a2", TENANT_A, "client-D", "D 应用", "general", 0, "active", None)
        .await;
    fixture
        .seed_app("app-e", TENANT_A, "client-E", "E 应用", "general", 0, "hidden", None)
        .await;
}

/// AC-S-160 步骤 1/2/3：2 组 + 组顺序 + 组内排序 + hidden 被过滤 + **3 次逐字节相同**。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn the_workspace_groups_and_orders_apps_deterministically() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    seed_workspace(&fixture).await;

    let token = token_for(TENANT_A, READ_USER);
    let (status, body) = fixture
        .call("GET", "/api/apps/workspace", Some(&token), None)
        .await;
    assert_eq!(status, 200, "工作台必须 200：{body}");

    let groups = body["groups"].as_array().expect("必须有 groups 数组");
    assert_eq!(groups.len(), 2, "**恰好** 2 组：{body}");
    assert_eq!(groups[0]["category"], "finance", "finance 在前（字典序）");
    assert_eq!(groups[1]["category"], "general", "general 在后");

    let finance: Vec<&str> = groups[0]["apps"]
        .as_array()
        .expect("组内 apps 是数组")
        .iter()
        .map(|app| app["displayName"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(finance, vec!["费用申请", "差旅报销"], "sortOrder 10 < 20 → B 在 A 前");

    let general: Vec<&str> = groups[1]["apps"]
        .as_array()
        .expect("组内 apps 是数组")
        .iter()
        .map(|app| app["displayName"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        general,
        vec!["C 应用", "D 应用"],
        "sortOrder 并列 → displayName 升序"
    );

    // hidden 的 E **不在任何组**里；总数 4。
    let all_ids: Vec<&str> = groups
        .iter()
        .flat_map(|group| group["apps"].as_array().expect("数组").iter())
        .map(|app| app["appId"].as_str().unwrap_or_default())
        .collect();
    assert!(!all_ids.contains(&"app-e"), "hidden 应用必须被过滤：{all_ids:?}");
    assert_eq!(all_ids.len(), 4, "总数必须 4（5 个里 1 个 hidden）");

    // 步骤 2：连打 3 次，**逐字节相同**。
    let mut serialized = Vec::new();
    for _ in 0..3 {
        let (status, body) = fixture
            .call("GET", "/api/apps/workspace", Some(&token), None)
            .await;
        assert_eq!(status, 200);
        serialized.push(serde_json::to_string(&body["groups"]).expect("应能序列化"));
    }
    assert_eq!(serialized[0], serialized[1], "3 次响应必须逐字节相同");
    assert_eq!(serialized[1], serialized[2], "3 次响应必须逐字节相同");
}

/// AC-S-160 步骤 4/5：`app.workspace.maxApps = 2` → **恰好 2 条**，且是**全局序的前 2 条**。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn the_workspace_respects_the_max_apps_configuration_immediately() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    seed_workspace(&fixture).await;

    // 步骤 4：默认值 60。
    let configured: serde_json::Value = sqlx::query_scalar(
        "SELECT value FROM platform_config WHERE key = 'app.workspace.maxApps'",
    )
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("配置行应当存在");
    assert_eq!(configured, json!(60), "app.workspace.maxApps 默认必须是 60");

    let token = token_for(TENANT_A, READ_USER);
    let (status, full) = fixture
        .call("GET", "/api/apps/workspace", Some(&token), None)
        .await;
    assert_eq!(status, 200);
    let full_count: usize = full["groups"]
        .as_array()
        .expect("数组")
        .iter()
        .map(|group| group["apps"].as_array().expect("数组").len())
        .sum();
    assert_eq!(full_count, 4, "默认上限下应当有 4 条");

    // 步骤 5：把上限改成 2 → **立刻**（不 sleep）再取。
    //
    // ⚠️ 生产通道是 `platform_config_upsert`（自增 epoch，D282）；
    // 本夹具的配置源是内存的（见 `common::Fixture::settings`），
    // 所以这里替换它来模拟「下一个请求就生效」。
    // 真库通道由 `platform-server` 的 `app_real_stack.rs` 覆盖。
    fixture.settings.set(platform_app::config::AppSettings {
        workspace_max_apps: 2,
        ..platform_app::config::AppSettings::default()
    });

    let (status, limited) = fixture
        .call("GET", "/api/apps/workspace", Some(&token), None)
        .await;
    assert_eq!(status, 200, "改了配置后端点仍必须可用");
    let limited_ids: Vec<&str> = limited["groups"]
        .as_array()
        .expect("数组")
        .iter()
        .flat_map(|group| group["apps"].as_array().expect("数组").iter())
        .map(|app| app["appId"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(limited_ids.len(), 2, "**恰好** 2 条（不是「不超过 2」）：{limited}");
    // 全局 `(category, sortOrder, displayName)` 序的前 2 条 = finance 组的 B、A。
    assert_eq!(
        limited_ids,
        vec!["app-b", "app-a"],
        "必须是全局序的前 2 条（钉死是哪两条，S-19）：{limited}"
    );
}

// ---------------------------------------------------------------------------
// AC-S-161：最近 / 常用
// ---------------------------------------------------------------------------

/// AC-S-161 步骤 1~4：UPSERT 累加 + recent 首条 + frequent 首条。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn recents_and_frequents_track_access_order_and_count() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    for (client, app, name) in [
        ("client-A", "app-a", "A 应用"),
        ("client-B", "app-b", "B 应用"),
        ("client-C", "app-c", "C 应用"),
    ] {
        fixture.seed_client(client, "active", &[], None).await;
        fixture
            .seed_app(app, TENANT_A, client, name, "general", 0, "active", None)
            .await;
    }

    let token = token_for(TENANT_A, READ_USER);
    // 步骤 1：A → B → C → A（**A 访问两次** → `access_count = 2`；三条记录都建出来）。
    //
    // ⚠️ 顺序钉死：最后一条必须是 A，否则 `recent[0]` 不是 A。
    for app in ["app-a", "app-b", "app-c", "app-a"] {
        let (status, body) = fixture
            .call("POST", &format!("/api/apps/{app}/access"), Some(&token), None)
            .await;
        assert_eq!(status, 200, "记访问必须 200：{body}");
    }

    // 步骤 2：recent 首条是 A（最后访问）。
    let (status, recent) = fixture
        .call("GET", "/api/apps/recent", Some(&token), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        recent[0]["appId"], "app-a",
        "recent 首条必须是最后访问的 A：{recent}"
    );

    // 步骤 3：frequent 首条是 A（access_count = 2 最大）。
    let (status, frequent) = fixture
        .call("GET", "/api/apps/frequent", Some(&token), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        frequent[0]["appId"], "app-a",
        "frequent 首条必须是 access_count 最大的 A：{frequent}"
    );

    // 步骤 4：再打一次 A → 响应 accessCount == 3。
    let (status, body) = fixture
        .call(
            "POST",
            "/api/apps/app-a/access",
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "recorded");
    assert_eq!(
        body["accessCount"], 3,
        "UPSERT 必须**累加**（第 3 次 = 3）：{body}"
    );

    // A 那行 access_count == 3；行数仍为 3（**绑定 user_id**，S-28）。
    let count = fixture
        .scalar(
            "SELECT access_count FROM platform_app_access \
             WHERE tenant_id = 'tenant-a' AND user_id = 'u-perm-read@example.com' \
               AND client_id = 'client-A'",
        )
        .await;
    assert_eq!(count, 3, "落库的 access_count 必须是 3");
    let rows = fixture
        .scalar(
            "SELECT count(*) FROM platform_app_access \
             WHERE tenant_id = 'tenant-a' AND user_id = 'u-perm-read@example.com'",
        )
        .await;
    assert_eq!(rows, 3, "行数必须仍是 3（UPSERT 不是插入新行）");
}

/// AC-S-161 步骤 5：把应用改成 `hidden` → 它**不在** recent 里。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn a_hidden_app_disappears_from_recents_and_frequents() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    for (client, app, name) in [
        ("client-A", "app-a", "A 应用"),
        ("client-B", "app-b", "B 应用"),
    ] {
        fixture.seed_client(client, "active", &[], None).await;
        fixture
            .seed_app(app, TENANT_A, client, name, "general", 0, "active", None)
            .await;
    }

    let token = token_for(TENANT_A, READ_USER);
    for app in ["app-a", "app-b"] {
        fixture
            .call("POST", &format!("/api/apps/{app}/access"), Some(&token), None)
            .await;
    }

    // 把 B 改成 hidden。
    fixture
        .execute("UPDATE platform_app SET status = 'hidden' WHERE id = 'app-b'")
        .await;

    let (status, recent) = fixture
        .call("GET", "/api/apps/recent", Some(&token), None)
        .await;
    assert_eq!(status, 200);
    let ids: Vec<&str> = recent
        .as_array()
        .expect("数组")
        .iter()
        .map(|app| app["appId"].as_str().unwrap_or_default())
        .collect();
    assert!(!ids.contains(&"app-b"), "hidden 的 B 不得出现在 recent：{ids:?}");
    assert!(ids.contains(&"app-a"), "A 必须仍在：{ids:?}");
}

/// TC-016-041 / D268：孤儿访问记录（应用行不存在）**不可见**。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn orphan_access_rows_are_not_returned() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture
        .seed_client("client-real", "active", &[], None)
        .await;
    fixture
        .seed_app(
            "app-real",
            TENANT_A,
            "client-real",
            "真应用",
            "general",
            0,
            "active",
            None,
        )
        .await;
    fixture
        .seed_client("client-orphan", "active", &[], None)
        .await;
    // 只造**访问记录**，**不造** platform_app 行（模拟 client 被删后残留，S-13）。
    fixture
        .seed_access(TENANT_A, READ_USER, "client-orphan", 99, 0)
        .await;
    fixture
        .seed_access(TENANT_A, READ_USER, "client-real", 1, 60)
        .await;

    let token = token_for(TENANT_A, READ_USER);
    for endpoint in ["/api/apps/recent", "/api/apps/frequent"] {
        let (status, body) = fixture.call("GET", endpoint, Some(&token), None).await;
        assert_eq!(status, 200);
        let ids: Vec<&str> = body
            .as_array()
            .expect("数组")
            .iter()
            .map(|app| app["appId"].as_str().unwrap_or_default())
            .collect();
        assert!(
            !ids.contains(&"app-real") || ids.len() == 1,
            "孤儿行不得出现（长度不受影响）：{endpoint} → {ids:?}"
        );
        assert_eq!(ids.len(), 1, "只应返回**在册**的那个应用：{endpoint} → {ids:?}");
        assert_eq!(ids[0], "app-real");
    }
}

// ---------------------------------------------------------------------------
// AC-S-162：导航
// ---------------------------------------------------------------------------

/// AC-S-162 步骤 1/2/4/5：有权限 → module + app 都在；无 audit → audit 消失但 app 仍在；
/// 字段集按来源钉死；`target` 逐字等于 `launch_url`。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn navigation_merges_modules_and_apps_and_filters_by_permission() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture
        .seed_client("client-A", "active", &["rbac:role:read"], None)
        .await;
    fixture
        .seed_app(
            "app-nav",
            TENANT_A,
            "client-A",
            "第三方报销",
            "finance",
            10,
            "active",
            Some("https://expense.example/app"),
        )
        .await;

    // 步骤 1：**持** audit 权限 → module 的 audit 项 + app 项都在。
    let (status, body) = fixture
        .call(
            "GET",
            "/api/apps/navigation",
            Some(&token_for(TENANT_A, AUDIT_USER)),
            None,
        )
        .await;
    assert_eq!(status, 200, "导航必须 200：{body}");
    let items = body["items"].as_array().expect("必须有 items 数组");

    let module_targets: Vec<&str> = items
        .iter()
        .filter(|item| item["source"] == "module")
        .map(|item| item["target"].as_str().unwrap_or_default())
        .collect();
    let app_items: Vec<&Value> = items
        .iter()
        .filter(|item| item["source"] == "app")
        .collect();
    assert!(
        module_targets.contains(&"/admin/audit"),
        "持 audit 权限时必须下发 audit 项：{module_targets:?}"
    );
    assert_eq!(app_items.len(), 1, "app 项必须存在：{items:?}");

    // 步骤 4：module 项的字段集**恰好 6 个**；app 项**恰好 7 个**。
    let module_item = items
        .iter()
        .find(|item| item["target"] == "/admin/audit")
        .expect("必须有 audit 项");
    let mut module_keys: Vec<&str> = module_item
        .as_object()
        .expect("对象")
        .keys()
        .map(String::as_str)
        .collect();
    module_keys.sort_unstable();
    assert_eq!(
        module_keys,
        vec!["icon", "kind", "label", "requiredPermission", "source", "target"],
        "module 项必须恰好 6 个键（没有 appId / labelKey / category / sortOrder）"
    );
    assert_eq!(module_item["kind"], "internal", "module 项 kind 必须是 internal");
    assert_eq!(module_item["requiredPermission"], "audit:events:read");
    assert_eq!(module_item["label"], "审计日志", "label 必须是 manifest 的中文文案");

    let app_item = app_items[0];
    let mut app_keys: Vec<&str> = app_item
        .as_object()
        .expect("对象")
        .keys()
        .map(String::as_str)
        .collect();
    app_keys.sort_unstable();
    assert_eq!(
        app_keys,
        vec![
            "appId",
            "icon",
            "kind",
            "label",
            "requiredPermission",
            "source",
            "target"
        ],
        "app 项必须恰好 7 个键"
    );
    assert_eq!(app_item["kind"], "external", "app 项 kind 必须是 external");
    assert!(
        app_item["requiredPermission"].is_null(),
        "app 项的 requiredPermission 必须是 null"
    );
    // 步骤 5：target **逐字等于** launch_url。
    assert_eq!(
        app_item["target"], "https://expense.example/app",
        "app 项 target 必须逐字等于 launch_url"
    );

    // 步骤 2：**不持** audit 权限 → audit 项消失，但 app 项仍在。
    let (status, body) = fixture
        .call(
            "GET",
            "/api/apps/navigation",
            Some(&token_for(TENANT_A, READ_USER)),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let items = body["items"].as_array().expect("数组");
    assert!(
        !items.iter().any(|item| item["target"] == "/admin/audit"),
        "不持 audit 权限时 audit 项必须消失：{items:?}"
    );
    assert!(
        items.iter().any(|item| item["source"] == "app"),
        "app 项与权限点无关，必须仍在：{items:?}"
    );

    // 排序：module 在前、app 在后。
    let first_app_index = items
        .iter()
        .position(|item| item["source"] == "app")
        .expect("有 app 项");
    let last_module_index = items
        .iter()
        .rposition(|item| item["source"] == "module")
        .expect("有 module 项");
    assert!(
        last_module_index < first_app_index,
        "module 项必须在 app 项之前（D284）"
    );
}

/// AC-S-162 步骤 3 / TC-016-047：`perm` 缺省（`None`）的项**总是下发**（无权限账号也看得到）。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn a_menu_without_a_permission_is_always_visible() {
    let fixture = setup().await;
    // `NONE_USER` **没有**任何权限点。

    let (status, body) = fixture
        .call(
            "GET",
            "/api/apps/navigation",
            Some(&token_for(TENANT_A, NONE_USER)),
            None,
        )
        .await;
    assert_eq!(status, 200, "登录即可看导航：{body}");
    let items = body["items"].as_array().expect("数组");
    let workbench = items
        .iter()
        .find(|item| item["target"] == "/workbench")
        .expect("perm 缺省的「工作台」项必须下发");
    assert!(
        workbench["requiredPermission"].is_null(),
        "perm 缺省的项 requiredPermission 必须是 null"
    );
    // 反向对照：带 perm 的两条都**不**出现。
    assert!(
        !items.iter().any(|item| item["target"] == "/admin/audit"),
        "无权限时带 perm 的 audit 项不得出现"
    );
    assert!(
        !items.iter().any(|item| item["target"] == "/oauth/grants"),
        "无权限时带 perm 的 oauth 项不得出现"
    );
}

/// AC-S-162 步骤 6：`DELETE /api/apps/{appId}/launch-url` → 204，该应用**不在**导航里。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn clearing_the_launch_url_removes_the_app_from_navigation() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture
        .seed_client("client-A", "active", &[], None)
        .await;
    fixture
        .seed_app(
            "app-nav-clear",
            TENANT_A,
            "client-A",
            "有入口的",
            "finance",
            0,
            "active",
            Some("https://expense.example/app"),
        )
        .await;

    let admin = token_for(TENANT_A, ADMIN_USER);
    // 前置：应用在导航里。
    let (status, body) = fixture
        .call("GET", "/api/apps/navigation", Some(&admin), None)
        .await;
    assert_eq!(status, 200);
    assert!(
        body["items"]
            .as_array()
            .expect("数组")
            .iter()
            .any(|item| item["appId"] == "app-nav-clear"),
        "清空前应用必须在导航里：{body}"
    );

    // 清空 → 204。
    let (status, _) = fixture
        .call(
            "DELETE",
            "/api/apps/app-nav-clear/launch-url",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(status, 204, "清空 launch_url 必须 204");

    // 库里必须真的是 NULL（不是空串）。
    let is_null = fixture
        .scalar(
            "SELECT count(*) FROM platform_app \
             WHERE id = 'app-nav-clear' AND launch_url IS NULL",
        )
        .await;
    assert_eq!(is_null, 1, "launch_url 必须落库为 NULL（D283）");

    // 再调导航：该应用**不出现**。
    let (status, body) = fixture
        .call("GET", "/api/apps/navigation", Some(&admin), None)
        .await;
    assert_eq!(status, 200);
    assert!(
        !body["items"]
            .as_array()
            .expect("数组")
            .iter()
            .any(|item| item["appId"] == "app-nav-clear"),
        "清空入口后应用不得出现在导航里：{body}"
    );

    // 工作台**仍然显示**它（D279：工作台不按 launch_url 过滤）。
    let (status, workspace) = fixture
        .call("GET", "/api/apps/workspace", Some(&admin), None)
        .await;
    assert_eq!(status, 200);
    assert!(
        workspace["groups"]
            .as_array()
            .expect("数组")
            .iter()
            .any(|group| group["apps"]
                .as_array()
                .expect("数组")
                .iter()
                .any(|app| app["appId"] == "app-nav-clear")),
        "工作台仍必须显示「在册但没有入口」的应用（D279）：{workspace}"
    );
}

// ---------------------------------------------------------------------------
// AC-S-164：审计埋点（**假 sink** 只验埋点契约；落库见 app_real_stack.rs）
// ---------------------------------------------------------------------------

/// AC-S-164：注册 / 修改 / 删除各产生**一条**对应事件码，且 `object_type` 恰好是 `platform_app`。
#[tokio::test]
#[ignore = "需要真实库（DATABASE_URL_TEST）"]
async fn registry_mutations_emit_one_audit_event_each() {
    let fixture = setup().await;
    grant_baseline(&fixture);
    fixture
        .seed_client("client-A", "active", &[], None)
        .await;
    let admin = token_for(TENANT_A, ADMIN_USER);

    let (status, created) = fixture
        .call(
            "POST",
            "/api/apps",
            Some(&admin),
            Some(json!({ "clientId": "client-A", "displayName": "审计探针" })),
        )
        .await;
    assert_eq!(status, 201);
    let app_id = created["appId"].as_str().expect("appId").to_string();

    let (status, _) = fixture
        .call(
            "PATCH",
            &format!("/api/apps/{app_id}"),
            Some(&admin),
            Some(json!({ "sortOrder": 7 })),
        )
        .await;
    assert_eq!(status, 200);

    let (status, _) = fixture
        .call("DELETE", &format!("/api/apps/{app_id}"), Some(&admin), None)
        .await;
    assert_eq!(status, 204);

    for action in [
        "app.registry.registered",
        "app.registry.updated",
        "app.registry.deleted",
    ] {
        let events = fixture.sink.with_action(action);
        assert_eq!(events.len(), 1, "`{action}` 必须恰好 1 条：{:?}", fixture.sink.events());
        let event = &events[0];
        assert_eq!(event.object_type, "platform_app", "object_type 必须恰好是 platform_app");
        assert_eq!(event.object_id.as_deref(), Some(app_id.as_str()));
        assert_eq!(event.actor_id.as_deref(), Some(ADMIN_USER));
        assert_eq!(event.tenant_id, TENANT_A);
    }
}
