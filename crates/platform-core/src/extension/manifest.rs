//! `manifest.json` 的解析与校验。
//!
//! # 为什么不引入 `jsonschema`
//!
//! Schema 见 [`crate::extension::manifest_schema::MANIFEST_JSON_SCHEMA`]，
//! 但校验用 Rust 代码实现 —— 低配机冷编译对新增依赖敏感，
//! 而 manifest 的规则只有 6 条，手写校验更好定位错误（能给出字段路径）。
//!
//! # 字段规则
//!
//! | 字段 | 规则 |
//! |------|------|
//! | `id` | `^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$` |
//! | `name` | 非空（去掉首尾空白后不得为空） |
//! | `version` | `^\d+\.\d+\.\d+$` |
//! | `permissions[].code` | `^[a-z][a-z0-9-]*:[a-z][a-z0-9-]*:[a-z][a-z0-9-]*$`（`api-conventions v1` C4） |
//! | `permissions[].name` | 非空（去掉首尾空白后不得为空） |
//! | `menus[].route` | `^/[a-z0-9/-]*$` |
//! | `dependencies[]` | 与 `id` 同规则 |
//! | `tenantScoped.tables[]` | `^[a-z][a-z0-9_]*$`（表名；会被拼进 DDL，不能放宽） |
//! | `tenantScoped.column` | `^[a-z][a-z0-9_]*$`（租户列名，默认 `tenant_id`） |

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::ConfigScope;

/// `manifest.json` 的解析或校验错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// JSON 语法错误或文件读取失败。
    Json(String),
    /// 字段不符合约定。
    Field {
        /// 字段路径，如 `permissions[0].code`。
        field: String,
        /// 面向开发者的英文原因。
        reason: String,
    },
}

impl ManifestError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Json(_) => "MANIFEST_JSON_INVALID",
            Self::Field { .. } => "MANIFEST_FIELD_INVALID",
        }
    }

    /// 出错的字段路径；JSON 语法错误时为空串。
    pub fn field(&self) -> &str {
        match self {
            Self::Json(_) => "",
            Self::Field { field, .. } => field,
        }
    }

    /// 面向开发者的英文原因。
    pub fn reason(&self) -> &str {
        match self {
            Self::Json(reason) => reason,
            Self::Field { reason, .. } => reason,
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(reason) => write!(f, "manifest.json is not valid JSON: {reason}"),
            Self::Field { field, reason } => write!(f, "{field}: {reason}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// 权限点类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionType {
    /// 菜单权限。
    Menu,
    /// 按钮权限。
    Button,
    /// 字段权限。
    Field,
    /// 数据权限。
    Data,
}

/// 权限点声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionSpec {
    /// 权限点编码：`module:resource:action`。
    pub code: String,
    /// 权限点名称。
    pub name: String,
    /// 权限点类型。
    #[serde(rename = "type")]
    pub kind: PermissionType,
}

/// 菜单声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MenuSpec {
    /// 菜单标题。
    pub label: String,
    /// 图标。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// 路由路径。
    pub route: String,
    /// 访问该菜单所需权限点。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perm: Option<String>,
}

/// 配置项类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigType {
    /// 字符串。
    String,
    /// 数字。
    Number,
    /// 布尔。
    Boolean,
    /// 数组。
    Array,
    /// 对象。
    Object,
}

/// 配置项声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigSpec {
    /// 配置项类型。
    #[serde(rename = "type")]
    pub kind: ConfigType,
    /// 默认值。
    pub default: Value,
    /// 作用域：空间 > 租户 > 平台（`api-conventions v1` C9）。
    #[serde(default = "default_scope")]
    pub scope: ConfigScope,
    /// 界面展示名称。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// 租户作用域声明：哪些表按租户隔离。
///
/// 平台据此**统一生成** RLS 策略（`ENABLE ROW LEVEL SECURITY` + policy），
/// 业务模块不手写 policy —— 手写迟早会漏（漏一张表 = 该表完全无隔离）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantScopedSpec {
    /// 租户作用域表的表名（不含 schema）。
    pub tables: Vec<String>,
    /// 租户列名；默认 `tenant_id`。
    #[serde(default = "default_tenant_column")]
    pub column: String,
}

fn default_tenant_column() -> String {
    "tenant_id".to_string()
}

/// 业务模块 manifest。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    /// 模块 ID，必须与目录名一致。
    pub id: String,
    /// 模块名称。
    pub name: String,
    /// 语义化版本。
    pub version: String,
    /// 是否启用。
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 权限点声明。
    pub permissions: Vec<PermissionSpec>,
    /// 菜单声明。
    pub menus: Vec<MenuSpec>,
    /// 依赖的其它模块 ID。
    pub dependencies: Vec<String>,
    /// 配置项声明。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<BTreeMap<String, ConfigSpec>>,
    /// 租户作用域声明：平台据此统一生成 RLS 策略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_scoped: Option<TenantScopedSpec>,
}

fn default_enabled() -> bool {
    true
}

fn default_scope() -> ConfigScope {
    ConfigScope::Tenant
}

impl Manifest {
    /// 解析 `manifest.json` 文本（只做反序列化，不做字段校验）。
    pub fn parse_str(raw: &str) -> Result<Self, ManifestError> {
        serde_json::from_str(raw).map_err(|err| ManifestError::Json(err.to_string()))
    }

    /// 从文件读取并解析 `manifest.json`。
    pub fn from_path(path: &Path) -> Result<Self, ManifestError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|err| ManifestError::Json(format!("cannot read file: {err}")))?;
        Self::parse_str(&raw)
    }

    /// 解析并校验；返回**全部**校验问题（不是只返回第一个）。
    pub fn parse_checked(raw: &str) -> Result<Self, Vec<ManifestError>> {
        let manifest = Self::parse_str(raw).map_err(|err| vec![err])?;
        let issues = manifest.validate();
        if issues.is_empty() {
            Ok(manifest)
        } else {
            Err(issues)
        }
    }

    /// 校验全部字段规则，返回所有问题。
    pub fn validate(&self) -> Vec<ManifestError> {
        let mut issues = Vec::new();

        if !is_module_id(&self.id) {
            issues.push(field_error(
                "id",
                "must match ^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$",
            ));
        }
        if self.name.trim().is_empty() {
            issues.push(field_error("name", "must not be empty"));
        }
        if !is_semver(&self.version) {
            issues.push(field_error("version", "must match ^\\d+\\.\\d+\\.\\d+$"));
        }

        for (index, permission) in self.permissions.iter().enumerate() {
            if !is_permission_code(&permission.code) {
                issues.push(field_error(
                    &format!("permissions[{index}].code"),
                    "must match ^[a-z][a-z0-9-]*:[a-z][a-z0-9-]*:[a-z][a-z0-9-]*$",
                ));
            }
            if permission.name.trim().is_empty() {
                issues.push(field_error(
                    &format!("permissions[{index}].name"),
                    "must not be empty",
                ));
            }
        }

        for (index, menu) in self.menus.iter().enumerate() {
            if !is_route(&menu.route) {
                issues.push(field_error(
                    &format!("menus[{index}].route"),
                    "must match ^/[a-z0-9/-]*$",
                ));
            }
        }

        for (index, dependency) in self.dependencies.iter().enumerate() {
            if !is_module_id(dependency) {
                issues.push(field_error(
                    &format!("dependencies[{index}]"),
                    "must match ^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$",
                ));
            }
        }

        if let Some(spec) = &self.tenant_scoped {
            if spec.tables.is_empty() {
                issues.push(field_error("tenantScoped.tables", "must not be empty"));
            }
            for (index, table) in spec.tables.iter().enumerate() {
                if !is_sql_identifier(table) {
                    issues.push(field_error(
                        &format!("tenantScoped.tables[{index}]"),
                        "must match ^[a-z][a-z0-9_]*$",
                    ));
                }
            }
            if !is_sql_identifier(&spec.column) {
                issues.push(field_error(
                    "tenantScoped.column",
                    "must match ^[a-z][a-z0-9_]*$",
                ));
            }
        }

        issues
    }

    /// 声明过的权限点编码。
    pub fn permission_codes(&self) -> Vec<&str> {
        self.permissions
            .iter()
            .map(|permission| permission.code.as_str())
            .collect()
    }
}

fn field_error(field: &str, reason: &str) -> ManifestError {
    ManifestError::Field {
        field: field.to_string(),
        reason: reason.to_string(),
    }
}

/// 模块 ID / 依赖 ID：`^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$`。
///
/// 与 `MANIFEST_JSON_SCHEMA` 里的 `id` / `dependencies[]` pattern **完全等价**
/// （每一段的首字符必须是字母），两边不允许出现不同的判定。
pub(crate) fn is_module_id(value: &str) -> bool {
    let mut segments = value.split('-');
    let Some(first) = segments.next() else {
        return false;
    };
    is_lower_alnum_segment(first) && segments.all(is_lower_alnum_segment)
}

/// 语义化版本：`^\d+\.\d+\.\d+$`。
fn is_semver(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// 权限点编码：`^[a-z][a-z0-9-]*:[a-z][a-z0-9-]*:[a-z][a-z0-9-]*$`。
fn is_permission_code(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 3 && parts.iter().all(|part| is_permission_segment(part))
}

/// 权限点的一段：`^[a-z][a-z0-9-]*$`。
fn is_permission_segment(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }
        _ => false,
    }
}

/// 路由路径：`^/[a-z0-9/-]*$`。
fn is_route(value: &str) -> bool {
    value.starts_with('/')
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '/' || c == '-')
}

/// 一段小写字母数字（首字符必须是字母）：`^[a-z][a-z0-9]*$`。
fn is_lower_alnum_segment(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        }
        _ => false,
    }
}

/// SQL 标识符（表名 / 列名）：`^[a-z][a-z0-9_]*$`。
///
/// 标识符会被拼进 DDL（`ALTER TABLE` / `CREATE POLICY`），放宽就会变成 SQL 注入面 ——
/// 所以这里比业务侧的命名规则更严：**不允许大写、不允许连字符、不允许以数字开头**。
pub fn is_sql_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest_json() -> String {
        r#"{
          "id": "sandbox",
          "name": "Sandbox",
          "version": "0.1.0",
          "enabled": true,
          "permissions": [
            { "code": "sandbox:demo:read", "name": "Read", "type": "menu" }
          ],
          "menus": [
            { "label": "Sandbox", "route": "/sandbox", "perm": "sandbox:demo:read" }
          ],
          "dependencies": [],
          "config": {
            "pageSize": { "type": "number", "default": 20, "scope": "tenant" }
          }
        }"#
        .to_string()
    }

    #[test]
    fn valid_manifest_parses_and_passes_validation() {
        let manifest =
            Manifest::parse_checked(&valid_manifest_json()).expect("合法 manifest 应通过");
        assert_eq!(manifest.id, "sandbox");
        assert_eq!(manifest.version, "0.1.0");
        assert!(manifest.enabled);
        assert_eq!(manifest.permission_codes(), vec!["sandbox:demo:read"]);
        let config = manifest.config.as_ref().expect("config 应被解析");
        assert_eq!(
            config.get("pageSize").map(|spec| spec.scope),
            Some(ConfigScope::Tenant)
        );
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let raw = valid_manifest_json()
            .replace("\"enabled\": true,", "\"enabled\": true, \"unknown\": 1,");
        let err = Manifest::parse_str(&raw).expect_err("未知字段应被拒绝");
        assert_eq!(err.code(), "MANIFEST_JSON_INVALID");
    }

    #[test]
    fn missing_required_field_is_rejected() {
        let raw = valid_manifest_json().replace("\"menus\":", "\"menusX\":");
        let err = Manifest::parse_str(&raw).expect_err("缺少必备字段应被拒绝");
        assert_eq!(err.code(), "MANIFEST_JSON_INVALID");
    }

    #[test]
    fn invalid_field_values_are_reported_with_field_path() {
        let raw = valid_manifest_json()
            .replace("\"id\": \"sandbox\"", "\"id\": \"Sand_Box\"")
            .replace("\"version\": \"0.1.0\"", "\"version\": \"0.1\"");
        let issues = Manifest::parse_checked(&raw).expect_err("非法字段应被报告");
        let fields: Vec<&str> = issues.iter().map(|issue| issue.field()).collect();
        assert!(fields.contains(&"id"));
        assert!(fields.contains(&"version"));
        for issue in &issues {
            assert_eq!(issue.code(), "MANIFEST_FIELD_INVALID");
        }
    }

    #[test]
    fn invalid_permission_code_and_route_are_reported() {
        let raw = valid_manifest_json()
            .replace("sandbox:demo:read\"", "Sandbox:Demo:Read\"")
            .replace("\"/sandbox\"", "\"/Sandbox\"");
        let issues = Manifest::parse_checked(&raw).expect_err("非法权限点与路由应被报告");
        let fields: Vec<&str> = issues.iter().map(|issue| issue.field()).collect();
        assert!(fields.contains(&"permissions[0].code"));
        assert!(fields.contains(&"menus[0].route"));
    }

    #[test]
    fn invalid_json_is_reported_as_json_error() {
        let err = Manifest::parse_str("{ not json").expect_err("非法 JSON 应被拒绝");
        assert_eq!(err.code(), "MANIFEST_JSON_INVALID");
        assert!(err.field().is_empty());
    }

    /// 租户作用域声明：解析 + 校验。
    #[test]
    fn tenant_scoped_is_parsed_and_validated() {
        let raw = valid_manifest_json().replace(
            r#""config": {"#,
            r#""tenantScoped": { "tables": ["order_item", "order_log"], "column": "tenant_id" },
            "config": {"#,
        );
        let manifest = Manifest::parse_checked(&raw).expect("合法的 tenantScoped 应通过");
        let spec = manifest
            .tenant_scoped
            .as_ref()
            .expect("tenantScoped 应被解析");
        assert_eq!(spec.tables, vec!["order_item", "order_log"]);
        assert_eq!(spec.column, "tenant_id");
    }

    /// 租户列缺省时用 `tenant_id`。
    #[test]
    fn tenant_scoped_column_defaults_to_tenant_id() {
        let raw = valid_manifest_json().replace(
            r#""config": {"#,
            r#""tenantScoped": { "tables": ["order_item"] },
            "config": {"#,
        );
        let manifest = Manifest::parse_checked(&raw).expect("合法的 tenantScoped 应通过");
        let spec = manifest
            .tenant_scoped
            .as_ref()
            .expect("tenantScoped 应被解析");
        assert_eq!(spec.column, "tenant_id");
    }

    /// 空表清单、非法表名、非法列名都要被报出来（附字段路径）。
    #[test]
    fn invalid_tenant_scoped_is_reported_with_field_path() {
        let raw = valid_manifest_json().replace(
            r#""config": {"#,
            r#""tenantScoped": { "tables": ["Order Item"], "column": "Tenant-ID" },
            "config": {"#,
        );
        let issues = Manifest::parse_checked(&raw).expect_err("非法的 tenantScoped 应被报告");
        let fields: Vec<&str> = issues.iter().map(|issue| issue.field()).collect();
        assert!(
            fields.contains(&"tenantScoped.tables[0]"),
            "fields = {fields:?}"
        );
        assert!(
            fields.contains(&"tenantScoped.column"),
            "fields = {fields:?}"
        );
    }

    /// 表清单为空 → 报告（漏一张表 = 该表完全无隔离，不能当没声明）。
    #[test]
    fn empty_tenant_scoped_tables_are_reported() {
        let raw = valid_manifest_json().replace(
            r#""config": {"#,
            r#""tenantScoped": { "tables": [] },
            "config": {"#,
        );
        let issues = Manifest::parse_checked(&raw).expect_err("空表清单应被报告");
        assert!(
            issues
                .iter()
                .any(|issue| issue.field() == "tenantScoped.tables"),
            "issues = {issues:?}"
        );
    }

    /// 标识符规则：只允许小写字母开头的字母数字与下划线。
    #[test]
    fn sql_identifier_rule_is_strict() {
        assert!(is_sql_identifier("order_item"));
        assert!(is_sql_identifier("t1"));
        assert!(!is_sql_identifier("Order_Item"));
        assert!(!is_sql_identifier("order-item"));
        assert!(!is_sql_identifier("1order"));
        assert!(!is_sql_identifier(""));
    }
}
