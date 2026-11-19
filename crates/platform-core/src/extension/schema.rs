//! 元数据（`manifest.json` 的 `schema` 段）与**唯一序列化入口**。
//!
//! # 这一层解决什么
//!
//! 两件事，都在这里：
//!
//! | 内容 | 谁用 |
//! |------|------|
//! | 实体 / 字段元数据的形状与**阶段 1 校验** | [`crate::extension::manifest`] 解析 manifest 时 |
//! | [`SchemaProjector`]：按字段级权限**剔除**无权字段 | 业务模块的每一个返回实体的 handler |
//!
//! # 两阶段校验（rev 2）
//!
//! | 阶段 | 何时 | 内容 | 在哪 |
//! |------|------|------|------|
//! | **阶段 1** | 每个 manifest 解析时 | 实体键格式、`fields` 非空、字段 `key`/`label`/`type` 合法、同实体内 `key` 不重复、`api` 路径以 `/api/` 开头 | 本文件 [`EntitySchema::validate`] |
//! | **阶段 2** | **所有** manifest 加载完后 | 字段 `permission` 已登记、`reference.module/entity` 存在 | `platform-schema` 的注册表 |
//!
//! 阶段 1 是**聚合**的（返回 `Vec<ManifestError>`，与 `Manifest::validate` 同款）——
//! 不是「首个失败即退出」。三处 `parse_checked` 的调用点都依赖这个 `Vec`。
//!
//! # 为什么 `type` 在结构体里是 `String` 而不是枚举
//!
//! 若用 `FieldType` 直接反序列化，`"currency"` 会在 **serde 阶段**就失败，
//! 报出来的是 `MANIFEST_JSON_INVALID`（且没有字段路径）。
//! 而验收要求点名 `schema.demoItem.fields[4].type` 并说明「不在允许的取值里」——
//! 那只能在**校验阶段**给出。所以结构体存原始字符串，
//! 由 [`FieldType::parse`] 判定，非法时携带路径与允许集合报错。

use std::collections::BTreeSet;
use std::collections::btree_map::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::RequestContext;
use crate::extension::manifest::ManifestError;
use crate::extension::permission::PermissionChecker;

/// 字段类型的**允许集合**（写死 5 种）。
///
/// 加类型是**向后兼容**的（老渲染器遇到未知类型降级为文本），
/// 所以这里一旦扩集合，要同步 `capabilities/schema/manifest.schema.json`
/// （由 `cargo xtask capabilities` 生成，不允许手改）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldType {
    /// 文本。
    Text,
    /// 数字。
    Number,
    /// 布尔。
    Boolean,
    /// 日期（`YYYY-MM-DD`，不做时区换算）。
    Date,
    /// 引用其它实体（本期降级为只读 id 文本，D105）。
    Reference,
}

impl FieldType {
    /// 全部 5 种（顺序固定，用于错误信息与 JSON Schema 生成）。
    pub const ALL: [FieldType; 5] = [
        FieldType::Text,
        FieldType::Number,
        FieldType::Boolean,
        FieldType::Date,
        FieldType::Reference,
    ];

    /// 元数据里的写法。
    pub fn as_str(&self) -> &'static str {
        match self {
            FieldType::Text => "text",
            FieldType::Number => "number",
            FieldType::Boolean => "boolean",
            FieldType::Date => "date",
            FieldType::Reference => "reference",
        }
    }

    /// 解析；不在允许集合里返回 `None`（调用方据此报错）。
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == value)
    }

    /// 允许集合的英文枚举串（错误信息用）：`text, number, boolean, date, reference`。
    pub fn allowed_list() -> String {
        Self::ALL
            .iter()
            .map(|candidate| candidate.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `reference` 字段的目标实体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceSpec {
    /// 目标模块 ID。
    pub module: String,
    /// 目标实体键。
    pub entity: String,
}

/// 实体的数据接口地址（D102）。
///
/// # 为什么显式声明而不是约定式
///
/// `/api/<module>/<entityPlural>` 的复数化规则要么靠猜（`item→items` 还行，
/// `person→people` 就错），要么强制命名。显式只多几行，但**永远不会猜错**。
///
/// `list` / `create` 必填，`update` / `delete` 可选（不声明 → 前端不渲染对应按钮）。
/// 每个路径必须以 `/api/` 开头 —— **安全约束**：防止声明外部 URL 让前端把请求发到站外。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntityApi {
    /// 列表接口。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<String>,
    /// 创建接口。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create: Option<String>,
    /// 更新接口（`{id}` 占位）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<String>,
    /// 删除接口（`{id}` 占位）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete: Option<String>,
}

impl EntityApi {
    /// 已声明的路径（键名, 路径），按固定顺序。
    pub fn declared(&self) -> Vec<(&'static str, &str)> {
        [
            ("list", self.list.as_deref()),
            ("create", self.create.as_deref()),
            ("update", self.update.as_deref()),
            ("delete", self.delete.as_deref()),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect()
    }
}

/// 一个字段的元数据。
///
/// **未声明的可选属性一律 `None`** —— 序列化时**省略该键**（不是 `null`）：
/// 「未声明」与「显式 false」在语义上不同，前端要靠键的有无来区分。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldSchema {
    /// 字段名（同时是实体序列化后的 JSON 键）：`^[a-z][a-zA-Z0-9]*$`。
    pub key: String,
    /// 展示名（非空）。
    pub label: String,
    /// 字段类型（原始写法，校验阶段判定）。
    #[serde(rename = "type")]
    pub kind: String,
    /// 是否必填；未声明 = 不出现。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// 字段级权限点：声明后**无权用户看不到、接口也不返回**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
    /// 只读（表单里不渲染输入控件）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    /// 文本最小长度。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u64>,
    /// 文本最大长度。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u64>,
    /// 文本格式正则。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// 数字最小值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<serde_json::Number>,
    /// 数字最大值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<serde_json::Number>,
    /// 引用目标（仅 `type: reference`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<ReferenceSpec>,
}

impl FieldSchema {
    /// 解析后的字段类型；非法时为 `None`（启动阶段已经拦住，运行期不该出现）。
    pub fn field_type(&self) -> Option<FieldType> {
        FieldType::parse(&self.kind)
    }

    /// 字段级权限点（没有声明就是公开字段）。
    pub fn permission(&self) -> Option<&str> {
        self.permission.as_deref()
    }
}

/// 一个实体的元数据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntitySchema {
    /// 实体展示名（非空）。
    pub label: String,
    /// 数据接口地址（D102）。
    #[serde(default)]
    pub api: EntityApi,
    /// 字段清单 —— **顺序即前端表头顺序**。
    pub fields: Vec<FieldSchema>,
}

impl EntitySchema {
    /// 阶段 1 校验（单文件可判定）。返回**全部**问题。
    ///
    /// # 顺序（写死，与 design §2.4.3 一致）
    ///
    /// 1. 实体键格式
    /// 2. `fields` 非空
    /// 3. 每个字段：`key` 格式 → `label` 非空 → `type` 在允许集合内
    /// 4. 同一实体内 `key` 不重复
    /// 5. `api` 的必填项与路径前缀
    ///
    /// **不是**「首个失败即退出」：调用方一次看到全部问题（与 `Manifest::validate` 同款）。
    pub fn validate(&self, entity: &str) -> Vec<ManifestError> {
        let mut issues = Vec::new();
        let path = format!("schema.{entity}");

        if !is_entity_key(entity) {
            issues.push(field_error(
                &path,
                "must match ^[a-z][a-zA-Z0-9]*$ (the entity key is used as a URL segment)",
            ));
        }
        if self.label.trim().is_empty() {
            issues.push(field_error(&format!("{path}.label"), "must not be empty"));
        }
        if self.fields.is_empty() {
            issues.push(field_error(
                &format!("{path}.fields"),
                "must not be empty (an entity without fields renders an empty, submittable form)",
            ));
        }

        // 3) 每个字段：key → label → type（顺序写死）。
        for (index, field) in self.fields.iter().enumerate() {
            if !is_entity_key(&field.key) {
                issues.push(field_error(
                    &format!("{path}.fields[{index}].key"),
                    "must match ^[a-z][a-zA-Z0-9]*$",
                ));
            }
            if field.label.trim().is_empty() {
                issues.push(field_error(
                    &format!("{path}.fields[{index}].label"),
                    "must not be empty",
                ));
            }
            if FieldType::parse(&field.kind).is_none() {
                issues.push(field_error(
                    &format!("{path}.fields[{index}].type"),
                    &format!(
                        "`{}` is not an allowed field type; allowed: {}",
                        field.kind,
                        FieldType::allowed_list()
                    ),
                ));
            }
        }

        // 4) 同实体内的重复 key（**只报后出现的那个**，与「第一个占用者」的读法一致）。
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for (index, field) in self.fields.iter().enumerate() {
            if !seen.insert(field.key.as_str()) {
                issues.push(field_error(
                    &format!("{path}.fields[{index}].key"),
                    &format!(
                        "duplicate field key `{}` (already declared earlier in this entity)",
                        field.key
                    ),
                ));
            }
        }

        // 5) api：必填项 + 路径前缀。
        if self.api.list.as_deref().is_none_or(str::is_empty) {
            issues.push(field_error(
                &format!("{path}.api.list"),
                "must not be empty (the renderer needs a data source)",
            ));
        }
        if self.api.create.as_deref().is_none_or(str::is_empty) {
            issues.push(field_error(
                &format!("{path}.api.create"),
                "must not be empty",
            ));
        }
        for (key, value) in self.api.declared() {
            if !value.starts_with("/api/") {
                issues.push(field_error(
                    &format!("{path}.api.{key}"),
                    &format!(
                        "must start with /api/ (declared external URLs are rejected), got `{value}`"
                    ),
                ));
            }
        }

        issues
    }

    /// 当前用户**可见**的字段（顺序 = 声明顺序）。
    ///
    /// 无权字段**不在**返回值里 —— 不是「有但标记隐藏」：
    /// 那等于告诉无权限用户「这里有个字段你看不到」，泄露了字段名。
    pub fn visible_fields<'a>(
        &'a self,
        ctx: &RequestContext,
        checker: &dyn PermissionChecker,
    ) -> Vec<&'a FieldSchema> {
        self.fields
            .iter()
            .filter(|field| field_is_visible(field, ctx, checker))
            .collect()
    }

    /// 声明了字段级权限的字段（C-6 与启动日志用）。
    pub fn permission_fields(&self) -> Vec<&FieldSchema> {
        self.fields
            .iter()
            .filter(|field| field.permission.is_some())
            .collect()
    }
}

/// 字段对当前用户是否可见。
pub fn field_is_visible(
    field: &FieldSchema,
    ctx: &RequestContext,
    checker: &dyn PermissionChecker,
) -> bool {
    match field.permission.as_deref() {
        None => true,
        Some(code) => checker.has(ctx, code),
    }
}

/// 单个实体的元数据集合（`manifest.json` 的 `schema` 段）。
pub type EntitySchemas = BTreeMap<String, EntitySchema>;

/// 元数据驱动的**唯一**响应序列化入口。
///
/// 业务模块**不得**直接 `Json(entity)` 输出带字段级权限的实体 ——
/// 必须经它，否则无权字段会泄露（契约 C-6 扫这个）。
///
/// # 语义（必须严格）
///
/// | 情形 | 结果 |
/// |---|---|
/// | 字段无 `permission` 声明 | 保留 |
/// | 有 `permission`，且 `checker.has(ctx, code)` | 保留 |
/// | 有 `permission`，且判定不通过 | **整个键从对象里删除** |
///
/// **不是**置 `null`、**不是**置空串、**不是**掩码成 `"[REDACTED]"` ——
/// 三者都保留了「这里有东西」的信息。
///
/// # 白名单语义（fail closed）
///
/// 投影后的对象**只含元数据声明过、且当前用户有权的字段**。
/// 实体里多出来的字段（后来加的、还没写进元数据的）**默认不输出** ——
/// 少显示而不是多泄露。
///
/// `raw` 不是 JSON 对象时**原样返回**（没有键可剔）；
/// 数组 / 列表信封由调用方**逐条**投影（见 `platform-schema` 与 sandbox 的用法）。
pub struct SchemaProjector;

impl SchemaProjector {
    /// 按元数据 + 当前用户的字段级权限，重建响应对象。
    pub fn project(
        raw: Value,
        schema: &EntitySchema,
        ctx: &RequestContext,
        checker: &dyn PermissionChecker,
    ) -> Value {
        let Some(object) = raw.as_object() else {
            return raw;
        };
        let mut out = serde_json::Map::new();
        for field in &schema.fields {
            if !field_is_visible(field, ctx, checker) {
                continue;
            }
            if let Some(value) = object.get(&field.key) {
                out.insert(field.key.clone(), value.clone());
            }
        }
        Value::Object(out)
    }

    /// 列表投影：逐条投影 `items[]`，**信封本身不动**。
    ///
    /// # 为什么要单独一个方法
    ///
    /// 对信封整体投影会让白名单把 `nextCursor` 剔掉 —— 前端永远只有一页。
    /// 投影的粒度是**记录**，不是响应体。
    pub fn project_items(
        items: &mut [Value],
        schema: &EntitySchema,
        ctx: &RequestContext,
        checker: &dyn PermissionChecker,
    ) {
        for item in items.iter_mut() {
            let projected = Self::project(item.clone(), schema, ctx, checker);
            *item = projected;
        }
    }
}

/// 实体 / 字段键：`^[a-z][a-zA-Z0-9]*$`。
///
/// 键直接对应后端实体序列化后的 JSON 键（camelCase）与前端路由参数。
/// 允许下划线会让元数据的 key 与 serde 输出的键对不上 → 前端永远渲染空单元格。
pub fn is_entity_key(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => chars.all(|ch| ch.is_ascii_alphanumeric()),
        _ => false,
    }
}

fn field_error(field: &str, reason: &str) -> ManifestError {
    ManifestError::Field {
        field: field.to_string(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::permission::{DataScope, ScopeFilter};
    use std::collections::BTreeMap;

    /// 一个按名字回答「有没有权限」的判定器。
    struct FixedChecker(Vec<&'static str>);

    impl PermissionChecker for FixedChecker {
        fn has(&self, _ctx: &RequestContext, permission: &str) -> bool {
            self.0.contains(&permission)
        }

        fn scope(&self, _ctx: &RequestContext, _scope: DataScope) -> ScopeFilter {
            ScopeFilter::All
        }

        fn effective_scope(&self, _ctx: &RequestContext, _permission: &str) -> ScopeFilter {
            ScopeFilter::All
        }

        fn condition(
            &self,
            _ctx: &RequestContext,
            _permission: &str,
        ) -> Option<crate::extension::expr::CompiledCondition> {
            None
        }
    }

    fn entity_json() -> &'static str {
        r#"{
          "label": "示例条目",
          "api": {
            "list": "/api/sandbox/demo-items",
            "create": "/api/sandbox/demo-items",
            "update": "/api/sandbox/demo-items/{id}",
            "delete": "/api/sandbox/demo-items/{id}"
          },
          "fields": [
            { "key": "id", "label": "ID", "type": "text", "readonly": true },
            { "key": "title", "label": "标题", "type": "text", "required": true, "maxLength": 200 },
            { "key": "secret", "label": "内部备注", "type": "text", "permission": "sandbox:demo:secret" }
          ]
        }"#
    }

    fn parse_entity(raw: &str) -> EntitySchema {
        serde_json::from_str(raw).expect("实体元数据应能解析")
    }

    #[test]
    fn field_type_allow_list_is_exactly_five() {
        let names: Vec<&str> = FieldType::ALL.iter().map(FieldType::as_str).collect();
        assert_eq!(
            names,
            vec!["text", "number", "boolean", "date", "reference"]
        );
        assert_eq!(FieldType::parse("currency"), None);
        assert_eq!(FieldType::parse("text"), Some(FieldType::Text));
        assert_eq!(
            FieldType::allowed_list(),
            "text, number, boolean, date, reference"
        );
    }

    #[test]
    fn a_valid_entity_has_no_issues() {
        let entity = parse_entity(entity_json());
        assert_eq!(entity.validate("demoItem"), Vec::new());
        assert_eq!(entity.fields.len(), 3);
        assert_eq!(entity.fields[1].max_length, Some(200));
        assert_eq!(entity.fields[1].required, Some(true));
        assert_eq!(entity.fields[0].required, None);
        assert_eq!(entity.fields[2].permission(), Some("sandbox:demo:secret"));
    }

    #[test]
    fn an_unknown_type_is_reported_with_the_field_path_and_allowed_set() {
        let raw = entity_json().replace(
            "\"type\": \"text\", \"required\"",
            "\"type\": \"currency\", \"required\"",
        );
        let issues = parse_entity(&raw).validate("demoItem");
        assert_eq!(issues.len(), 1, "issues = {issues:?}");
        assert_eq!(issues[0].field(), "schema.demoItem.fields[1].type");
        let reason = issues[0].reason();
        assert!(reason.contains("currency"), "reason = {reason}");
        assert!(
            reason.contains("text, number, boolean, date, reference"),
            "reason = {reason}"
        );
    }

    #[test]
    fn a_duplicate_key_is_reported_once_at_the_later_index() {
        let raw = entity_json().replace(
            r#"{ "key": "secret", "label": "内部备注""#,
            r#"{ "key": "title", "label": "内部备注""#,
        );
        let issues = parse_entity(&raw).validate("demoItem");
        assert_eq!(issues.len(), 1, "issues = {issues:?}");
        assert_eq!(issues[0].field(), "schema.demoItem.fields[2].key");
        assert!(issues[0].reason().contains("duplicate"), "{:?}", issues[0]);
    }

    /// **聚合**校验：一次造两条互不重叠的违规，两条都要报出来。
    ///
    /// 这条守卫的是 rev 2 的核心修正：**不是**「首个失败即退出」
    /// （`parse_checked` 的三处调用点都依赖返回 `Vec`）。
    #[test]
    fn two_violations_are_both_reported_aggregation_is_kept() {
        let raw = entity_json().replace(
            r#"{ "key": "secret", "label": "内部备注", "type": "text", "permission": "sandbox:demo:secret" }"#,
            r#"{ "key": "blocked", "label": "币种", "type": "currency" },
               { "key": "title", "label": "重复", "type": "text" }"#,
        );
        let issues = parse_entity(&raw).validate("demoItem");
        assert_eq!(issues.len(), 2, "issues = {issues:?}");
        let fields: Vec<&str> = issues.iter().map(|issue| issue.field()).collect();
        assert!(
            fields.contains(&"schema.demoItem.fields[2].type"),
            "{fields:?}"
        );
        assert!(
            fields.contains(&"schema.demoItem.fields[3].key"),
            "{fields:?}"
        );
    }

    #[test]
    fn empty_fields_are_reported() {
        let raw = entity_json().replace(
            r#""fields": [
            { "key": "id", "label": "ID", "type": "text", "readonly": true },
            { "key": "title", "label": "标题", "type": "text", "required": true, "maxLength": 200 },
            { "key": "secret", "label": "内部备注", "type": "text", "permission": "sandbox:demo:secret" }
          ]"#,
            r#""fields": []"#,
        );
        let issues = parse_entity(&raw).validate("demoItem");
        assert_eq!(issues.len(), 1, "issues = {issues:?}");
        assert_eq!(issues[0].field(), "schema.demoItem.fields");
        assert!(issues[0].reason().contains("must not be empty"));
    }

    #[test]
    fn an_invalid_entity_key_is_reported_on_the_entity_path() {
        for entity in ["DemoItem", "demo_item"] {
            let issues = parse_entity(entity_json()).validate(entity);
            assert_eq!(issues.len(), 1, "entity = {entity}, issues = {issues:?}");
            assert_eq!(issues[0].field(), format!("schema.{entity}"));
            assert!(issues[0].reason().contains("^[a-z][a-zA-Z0-9]*$"));
        }
    }

    #[test]
    fn an_external_api_path_is_rejected() {
        let raw = entity_json().replace(
            "\"list\": \"/api/sandbox/demo-items\"",
            "\"list\": \"https://evil.example/api/sandbox/demo-items\"",
        );
        let issues = parse_entity(&raw).validate("demoItem");
        assert_eq!(issues.len(), 1, "issues = {issues:?}");
        assert_eq!(issues[0].field(), "schema.demoItem.api.list");
        assert!(issues[0].reason().contains("must start with /api/"));
    }

    #[test]
    fn a_missing_required_api_endpoint_is_reported() {
        let raw = entity_json().replace("\"create\": \"/api/sandbox/demo-items\",", "");
        let issues = parse_entity(&raw).validate("demoItem");
        assert_eq!(issues.len(), 1, "issues = {issues:?}");
        assert_eq!(issues[0].field(), "schema.demoItem.api.create");
    }

    /// 未声明的可选属性**省略该键**（不是 `null`）。
    #[test]
    fn undeclared_optional_attributes_are_omitted_from_the_json() {
        let entity = parse_entity(entity_json());
        let value = serde_json::to_value(&entity.fields[0]).expect("字段应可序列化");
        let object = value.as_object().expect("字段是对象");
        assert!(!object.contains_key("required"), "value = {value}");
        assert!(!object.contains_key("permission"));
        assert!(!object.contains_key("minLength"));
        assert!(!object.contains_key("maxLength"));
        assert_eq!(object.get("readonly"), Some(&Value::Bool(true)));
        let raw = serde_json::to_string(&entity).expect("实体应可序列化");
        assert!(!raw.contains("null"), "raw = {raw}");
    }

    /// 字段顺序 = 声明顺序（前端表头依赖它）。
    #[test]
    fn visible_fields_keep_the_declaration_order() {
        let entity = parse_entity(entity_json());
        let ctx = RequestContext::new();
        let all = FixedChecker(vec!["sandbox:demo:secret"]);
        let keys: Vec<&str> = entity
            .visible_fields(&ctx, &all)
            .into_iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(keys, vec!["id", "title", "secret"]);

        let none = FixedChecker(Vec::new());
        let keys: Vec<&str> = entity
            .visible_fields(&ctx, &none)
            .into_iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(keys, vec!["id", "title"]);
    }

    fn record() -> Value {
        serde_json::json!({
            "id": "item-1",
            "title": "示例",
            "secret": "内部-A",
            "internalNote": "不应输出",
        })
    }

    /// 无权字段**整个键被删除** —— 不是 null / 空串 / 掩码。
    #[test]
    fn the_projector_removes_the_key_entirely() {
        let entity = parse_entity(entity_json());
        let ctx = RequestContext::new();
        let none = FixedChecker(Vec::new());
        let out = SchemaProjector::project(record(), &entity, &ctx, &none);
        let object = out.as_object().expect("投影结果是对象");
        assert!(!object.contains_key("secret"), "out = {out}");
        assert_eq!(object.get("title"), Some(&Value::from("示例")));
        let raw = serde_json::to_string(&out).expect("应可序列化");
        assert!(!raw.contains("secret"), "raw = {raw}");
        assert!(!raw.contains("REDACTED"));
        assert!(!raw.contains("null"));
    }

    #[test]
    fn the_projector_keeps_the_field_when_the_permission_holds() {
        let entity = parse_entity(entity_json());
        let ctx = RequestContext::new();
        let granted = FixedChecker(vec!["sandbox:demo:secret"]);
        let out = SchemaProjector::project(record(), &entity, &ctx, &granted);
        let object = out.as_object().expect("投影结果是对象");
        assert_eq!(object.get("secret"), Some(&Value::from("内部-A")));
    }

    /// 白名单语义：元数据没声明的字段**默认不输出**（fail closed）。
    #[test]
    fn undeclared_entity_fields_are_dropped_even_with_all_permissions() {
        let entity = parse_entity(entity_json());
        let ctx = RequestContext::new();
        let granted = FixedChecker(vec!["sandbox:demo:secret"]);
        let out = SchemaProjector::project(record(), &entity, &ctx, &granted);
        assert!(!out.as_object().expect("对象").contains_key("internalNote"));
    }

    /// 列表：逐条投影，信封本身不动。
    #[test]
    fn project_items_projects_each_record_and_leaves_the_envelope_alone() {
        let entity = parse_entity(entity_json());
        let ctx = RequestContext::new();
        let none = FixedChecker(Vec::new());
        let mut items = vec![record(), record()];
        SchemaProjector::project_items(&mut items, &entity, &ctx, &none);
        for item in &items {
            let object = item.as_object().expect("对象");
            assert!(!object.contains_key("secret"));
            assert!(object.contains_key("id"));
        }
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn a_non_object_value_is_returned_unchanged() {
        let entity = parse_entity(entity_json());
        let ctx = RequestContext::new();
        let none = FixedChecker(Vec::new());
        let out = SchemaProjector::project(Value::from(7), &entity, &ctx, &none);
        assert_eq!(out, Value::from(7));
    }

    #[test]
    fn entity_api_declared_paths_are_listed_in_a_stable_order() {
        let entity = parse_entity(entity_json());
        let declared = entity.api.declared();
        assert_eq!(declared.len(), 4);
        assert_eq!(declared[0].0, "list");
        assert_eq!(declared[3].0, "delete");
    }

    #[test]
    fn permission_fields_are_listed() {
        let entity = parse_entity(entity_json());
        let fields: Vec<&str> = entity
            .permission_fields()
            .into_iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(fields, vec!["secret"]);
        let _: BTreeMap<String, EntitySchema> = EntitySchemas::new();
    }
}
