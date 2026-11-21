//! 数据模型：**2 张表的行类型** + 跨栈 DTO（REQ-016，design §2 / §3.1）。
//!
//! # 字段名（契约 C13）
//!
//! 所有**响应体 / 请求体**字段一律 **camelCase**（`appId` / `displayName` /
//! `sortOrder` / `requiredPermission` …）。
//!
//! # 时间格式（D142）
//!
//! 响应里的时间**一律** UTC、RFC3339、**微秒 6 位**
//! （`platform_core::extension::audit::format_timestamp_micros_utc` ——
//! 与审计记录同一个出口，不另写一份格式化）。
//!
//! # ⚠️ `scopes` 是**只读投影**（D266），不是本表的列
//!
//! [`AppView::scopes`] 的值**直接取自** `platform_oauth_client.allowed_scopes` →
//! `platform_app` 表**没有**、也**不得有** `scopes` / `allowed_scopes` 列
//! （AC-S-159 步骤 5 查 `information_schema` 钉死这一点）。

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// 应用注册表（**租户级**；RLS 由 `cargo xtask tenant-policies` 生成）。
pub const APP_TABLE: &str = "platform_app";
/// 访问记录（**租户级**）。
pub const ACCESS_TABLE: &str = "platform_app_access";

/// 应用的**状态**取值。
pub mod status {
    /// 在册且可见（工作台 / 最近 / 常用 / 导航都会展示）。
    pub const ACTIVE: &str = "active";
    /// 在册但隐藏（管理视角能看到，工作台 / 导航不展示）。
    pub const HIDDEN: &str = "hidden";
}

/// 导航项的**来源**。
pub mod source {
    /// 平台模块自带的菜单（`manifest.json` 的 `menus[]`）。
    pub const MODULE: &str = "module";
    /// 租户接入的应用（`platform_app`）。
    pub const APP: &str = "app";
}

/// 导航项的**跳转语义**（D269 / D284；前端据此决定走 SPA 路由还是整页跳转）。
pub mod kind {
    /// 站内路径 —— 走 SPA 路由（`router.navigate`）。
    pub const INTERNAL: &str = "internal";
    /// 外部 URL —— **必须**整页跳转（`location.href`）。
    pub const EXTERNAL: &str = "external";
}

// ---------------------------------------------------------------------------
// 行类型
// ---------------------------------------------------------------------------

/// `platform_app` 的一行（**恰好 12 列**，D265）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRow {
    /// 应用 ID（平台生成）。
    pub id: String,
    /// 租户（RLS 依据）。
    pub tenant_id: String,
    /// 引用的 client（FK → `platform_oauth_client`，`ON DELETE CASCADE`，D264）。
    pub client_id: String,
    /// 该租户工作台展示名（**不是** client 的 `name`，D262）。
    pub display_name: String,
    /// 描述。
    pub description: Option<String>,
    /// 分类（工作台分组用）。
    pub category: String,
    /// 图标 URL（可空，D263）。
    pub icon_url: Option<String>,
    /// 应用入口 URL（可空；为空 → **不出现在导航**，D269 / D279）。
    pub launch_url: Option<String>,
    /// 升序排列。
    pub sort_order: i32,
    /// `active` / `hidden`。
    pub status: String,
    /// 创建时刻。
    pub created_at: OffsetDateTime,
    /// 创建人。
    pub created_by: String,
}

platform_db::tenant_row!(AppRow {
    id: "id",
    tenant_id: "tenant_id",
    client_id: "client_id",
    display_name: "display_name",
    description: "description",
    category: "category",
    icon_url: "icon_url",
    launch_url: "launch_url",
    sort_order: "sort_order",
    status: "status",
    created_at: "created_at",
    created_by: "created_by",
});

/// `platform_app.access_count` 的**当前值**（UPSERT 后的累加结果）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessCountRow {
    /// 累加后的访问次数。
    pub access_count: i64,
}

platform_db::tenant_row!(AccessCountRow {
    access_count: "access_count",
});

/// 一条访问记录里**被 join 到的应用行**（最近 / 常用）。
///
/// ⚠️ 用 `platform_app` 的整行 —— 只返回「**当前租户仍在册且 active**」的应用，
/// 孤儿访问记录（client 还在、但没有应用行）**不可见**（D268 / AC-S-161）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppAccessRow {
    /// 应用 ID。
    pub id: String,
    /// 租户。
    pub tenant_id: String,
    /// 引用的 client。
    pub client_id: String,
    /// 展示名。
    pub display_name: String,
    /// 描述。
    pub description: Option<String>,
    /// 分类。
    pub category: String,
    /// 图标 URL。
    pub icon_url: Option<String>,
    /// 入口 URL。
    pub launch_url: Option<String>,
    /// 排序。
    pub sort_order: i32,
    /// 状态。
    pub status: String,
    /// 创建时刻。
    pub created_at: OffsetDateTime,
    /// 创建人。
    pub created_by: String,
}

platform_db::tenant_row!(AppAccessRow {
    id: "id",
    tenant_id: "tenant_id",
    client_id: "client_id",
    display_name: "display_name",
    description: "description",
    category: "category",
    icon_url: "icon_url",
    launch_url: "launch_url",
    sort_order: "sort_order",
    status: "status",
    created_at: "created_at",
    created_by: "created_by",
});

impl From<AppRow> for AppAccessRow {
    fn from(row: AppRow) -> Self {
        Self {
            id: row.id,
            tenant_id: row.tenant_id,
            client_id: row.client_id,
            display_name: row.display_name,
            description: row.description,
            category: row.category,
            icon_url: row.icon_url,
            launch_url: row.launch_url,
            sort_order: row.sort_order,
            status: row.status,
            created_at: row.created_at,
            created_by: row.created_by,
        }
    }
}

// ---------------------------------------------------------------------------
// 请求体（camelCase，D258 同款钉死）
// ---------------------------------------------------------------------------

/// `POST /api/apps` 的请求体。
///
/// ⚠️ 每个字段都是 `Option<T>` + `#[serde(default)]`：
/// **缺键** = 没传 → 由服务层按「必填」规则判 `APP_INVALID_REQUEST`
/// （显式 `null` 与缺键在这里**同义** —— 对创建来说两者都意味着「没给」）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAppBody {
    /// 引用的 client（**必填**）。
    #[serde(default)]
    pub client_id: Option<String>,
    /// 展示名（**必填**）。
    #[serde(default)]
    pub display_name: Option<String>,
    /// 描述。
    #[serde(default)]
    pub description: Option<String>,
    /// 分类（缺省 `general`）。
    #[serde(default)]
    pub category: Option<String>,
    /// 图标 URL。
    #[serde(default)]
    pub icon_url: Option<String>,
    /// 入口 URL。
    #[serde(default)]
    pub launch_url: Option<String>,
    /// 排序（缺省 0）。
    ///
    /// ⚠️ `i32` 而不是 `f64`：传 `"abc"` 或 `1e100` → **反序列化失败** →
    /// 由 HTTP 层统一回 `APP_INVALID_REQUEST`（AC-S-156 路径 5）。
    #[serde(default)]
    pub sort_order: Option<i32>,
}

/// `PATCH /api/apps/{appId}` 的请求体。
///
/// ⚠️ **只改传了的字段**（`Option<Option<T>>` 的语义在这里用不上 ——
/// 「把 `launch_url` 置空」走专用端点 `DELETE /api/apps/{appId}/launch-url`，
/// D283 / S-08）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppBody {
    /// 新的展示名。
    #[serde(default)]
    pub display_name: Option<String>,
    /// 新的描述。
    #[serde(default)]
    pub description: Option<String>,
    /// 新的分类。
    #[serde(default)]
    pub category: Option<String>,
    /// 新的图标 URL。
    #[serde(default)]
    pub icon_url: Option<String>,
    /// 新的入口 URL（**不能**用 `null` 表达「置空」—— 见 D283）。
    #[serde(default)]
    pub launch_url: Option<String>,
    /// 新的排序。
    #[serde(default)]
    pub sort_order: Option<i32>,
    /// 新的状态（`active` / `hidden`）。
    #[serde(default)]
    pub status: Option<String>,
}

// ---------------------------------------------------------------------------
// 响应体（camelCase，design §3.1）
// ---------------------------------------------------------------------------

/// 单个应用的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    /// 应用 ID。
    pub app_id: String,
    /// 引用的 client。
    pub client_id: String,
    /// 该租户工作台展示名。
    pub display_name: String,
    /// 描述。
    pub description: Option<String>,
    /// 分类。
    pub category: String,
    /// 图标 URL。
    pub icon_url: Option<String>,
    /// 入口 URL。
    pub launch_url: Option<String>,
    /// 排序。
    pub sort_order: i32,
    /// 状态。
    pub status: String,
    /// **只读投影**：`platform_oauth_client.allowed_scopes`（D266）。
    pub scopes: Vec<String>,
    /// 创建时刻（RFC3339、UTC、微秒 6 位）。
    pub created_at: String,
    /// 创建人。
    pub created_by: String,
}

/// 工作台的一个分类组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGroup {
    /// 分类名（组按它**字典序**升序）。
    pub category: String,
    /// 组内应用（按 `(sortOrder, displayName)` 升序）。
    pub apps: Vec<AppView>,
}

/// `GET /api/apps/workspace` 的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    /// 分组（**恰好**按 `category` 字典序升序）。
    pub groups: Vec<WorkspaceGroup>,
}

/// `POST /api/apps/{appId}/access` 的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessRecorded {
    /// 固定是 `"recorded"`。
    pub status: &'static str,
    /// UPSERT 之后的累计访问次数。
    pub access_count: i64,
}

/// 导航项（D284：**按来源分别定义**字段集）。
///
/// | 来源 | 字段（**恰好这些**） |
/// |---|---|
/// | `module` | `source` / `label` / `icon` / `target` / `kind` / `requiredPermission` |
/// | `app` | `source` / `appId` / `label` / `icon` / `target` / `kind` / `requiredPermission` |
///
/// → `appId` 用 `skip_serializing_if`：`module` 项**根本不含这个键**（不是 `null`）。
/// ⚠️ `requiredPermission` **不能**跳过 —— `app` 项必须是 **`null`**（不是缺键）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationItem {
    /// `"module"` / `"app"`。
    pub source: &'static str,
    /// 应用 ID（**只有** `source="app"` 才有这个键）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    /// 展示文案（`module` 用 `menus[].label` 的中文文案，`app` 用 `display_name`）。
    pub label: String,
    /// 图标。
    pub icon: Option<String>,
    /// 跳转目标（`module` 是站内路径，`app` 是外部 URL）。
    pub target: String,
    /// `"internal"` / `"external"`。
    pub kind: &'static str,
    /// 需要的权限点（`module` 取 `menus[].perm`；`app` 恒 `None`）。
    pub required_permission: Option<String>,
}

/// `GET /api/apps/navigation` 的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationView {
    /// 导航项（`module` **在前**、`app` **在后**；组内按 `label` 升序 —— D284）。
    pub items: Vec<NavigationItem>,
}

/// 查询 `platform_app` 的 12 列（**唯一**一份列清单，避免两处漂移）。
pub(crate) fn app_columns() -> &'static str {
    "id, tenant_id, client_id, display_name, description, category, icon_url, \
     launch_url, sort_order, status, created_at, created_by"
}

/// 由行 + client 的 `allowed_scopes` 组装响应体。
pub(crate) fn view_of(row: &AppRow, scopes: Vec<String>) -> AppView {
    AppView {
        app_id: row.id.clone(),
        client_id: row.client_id.clone(),
        display_name: row.display_name.clone(),
        description: row.description.clone(),
        category: row.category.clone(),
        icon_url: row.icon_url.clone(),
        launch_url: row.launch_url.clone(),
        sort_order: row.sort_order,
        status: row.status.clone(),
        scopes,
        created_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.created_at),
        created_by: row.created_by.clone(),
    }
}

/// 由 join 出来的访问行组装响应体。
pub(crate) fn view_of_access(row: &AppAccessRow, scopes: Vec<String>) -> AppView {
    AppView {
        app_id: row.id.clone(),
        client_id: row.client_id.clone(),
        display_name: row.display_name.clone(),
        description: row.description.clone(),
        category: row.category.clone(),
        icon_url: row.icon_url.clone(),
        launch_url: row.launch_url.clone(),
        sort_order: row.sort_order,
        status: row.status.clone(),
        scopes,
        created_at: platform_core::extension::audit::format_timestamp_micros_utc(&row.created_at),
        created_by: row.created_by.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row() -> AppRow {
        AppRow {
            id: "app-01".to_string(),
            tenant_id: "tenant-a".to_string(),
            client_id: "client-A".to_string(),
            display_name: "差旅报销".to_string(),
            description: Some("报销".to_string()),
            category: "finance".to_string(),
            icon_url: Some("https://cdn.example/i.png".to_string()),
            launch_url: Some("https://a.example/".to_string()),
            sort_order: 20,
            status: status::ACTIVE.to_string(),
            created_at: time::Date::from_calendar_date(2026, time::Month::November, 21)
                .expect("合法日期")
                .with_hms(0, 0, 0)
                .expect("合法时刻")
                .assume_utc(),
            created_by: "u-perm-app@example.com".to_string(),
        }
    }

    /// 响应体字段名是 **camelCase**（契约 C13）。
    #[test]
    fn the_app_view_uses_camel_case_keys() {
        let view = view_of(&sample_row(), vec!["rbac:role:read".to_string()]);
        let value = serde_json::to_value(&view).expect("应当能序列化");
        for key in [
            "appId",
            "clientId",
            "displayName",
            "description",
            "category",
            "iconUrl",
            "launchUrl",
            "sortOrder",
            "status",
            "scopes",
            "createdAt",
            "createdBy",
        ] {
            assert!(value.get(key).is_some(), "响应体缺少 camelCase 键 `{key}`");
        }
        assert!(
            value.get("app_id").is_none(),
            "响应体不得出现 snake_case 键"
        );
        // ⚠️ 不含任何 client 的敏感字段（**引用而非复制**，D262 / AC-S-156 步骤 7）。
        for forbidden in [
            "clientSecret",
            "clientSecretHash",
            "redirectUris",
            "allowedGrantTypes",
            "allowedScopes",
            "backchannelLogoutUri",
            "frontchannelLogoutUri",
        ] {
            assert!(
                value.get(forbidden).is_none(),
                "响应体不得含 client 的字段 `{forbidden}`"
            );
        }
    }

    /// `createdAt` 的格式：UTC、RFC3339、**微秒 6 位**（D142）。
    #[test]
    fn the_created_at_is_rfc3339_with_six_microsecond_digits() {
        let view = view_of(&sample_row(), Vec::new());
        assert_eq!(view.created_at, "2026-11-21T00:00:00.000000Z");
        assert!(view.created_at.ends_with('Z'), "必须是 UTC");
    }

    /// `module` 来源的导航项**恰好 6 个键**（没有 `appId`，D284 / AC-S-162 步骤 4）。
    #[test]
    fn a_module_navigation_item_has_exactly_six_keys() {
        let item = NavigationItem {
            source: source::MODULE,
            app_id: None,
            label: "审计日志".to_string(),
            icon: None,
            target: "/admin/audit".to_string(),
            kind: kind::INTERNAL,
            required_permission: Some("audit:events:read".to_string()),
        };
        let value = serde_json::to_value(&item).expect("应当能序列化");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("应当是对象")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "icon",
                "kind",
                "label",
                "requiredPermission",
                "source",
                "target"
            ],
            "module 项必须恰好这 6 个键（不含 appId）"
        );
    }

    /// `app` 来源的导航项**恰好 7 个键**，且 `requiredPermission` **是 `null` 而不是缺键**。
    #[test]
    fn an_app_navigation_item_has_exactly_seven_keys_with_a_null_permission() {
        let item = NavigationItem {
            source: source::APP,
            app_id: Some("app-01".to_string()),
            label: "差旅报销".to_string(),
            icon: None,
            target: "https://a.example/".to_string(),
            kind: kind::EXTERNAL,
            required_permission: None,
        };
        let value = serde_json::to_value(&item).expect("应当能序列化");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("应当是对象")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "appId",
                "icon",
                "kind",
                "label",
                "requiredPermission",
                "source",
                "target"
            ],
            "app 项必须恰好这 7 个键"
        );
        assert!(
            value.get("requiredPermission").is_some(),
            "requiredPermission 必须是 null，不是缺键"
        );
        assert!(value["requiredPermission"].is_null());
    }

    /// 创建请求体的字段名也是 camelCase。
    #[test]
    fn the_create_body_accepts_camel_case_keys() {
        let body: CreateAppBody = serde_json::from_str(
            r#"{"clientId":"client-A","displayName":"报销","sortOrder":10,
                "iconUrl":"https://cdn.example/i.png","launchUrl":"https://a.example/"}"#,
        )
        .expect("应当能反序列化");
        assert_eq!(body.client_id.as_deref(), Some("client-A"));
        assert_eq!(body.display_name.as_deref(), Some("报销"));
        assert_eq!(body.sort_order, Some(10));
    }

    /// `sortOrder: "abc"` → **反序列化失败**（HTTP 层据此回 `APP_INVALID_REQUEST`）。
    #[test]
    fn a_wrongly_typed_sort_order_fails_to_deserialize() {
        assert!(
            serde_json::from_str::<CreateAppBody>(r#"{"clientId":"c","displayName":"d","sortOrder":"abc"}"#)
                .is_err(),
            "sortOrder 类型错必须反序列化失败"
        );
    }
}
