//! `manifest.json` 的 JSON Schema（正式版）。
//!
//! 与设计摘要 §5 一字不改；同一份内容导出到
//! `capabilities/schema/manifest.schema.json`（REQ-004 的能力索引收录）。
//!
//! 校验由 `crate::extension::manifest` 里的 Rust 代码完成 —— **不引入** `jsonschema`
//! 之类的校验库，保持依赖最小（低配机冷编译敏感）。

/// `manifest.json` 的 JSON Schema（draft 2020-12）。
pub const MANIFEST_JSON_SCHEMA: &str = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Base Platform Module Manifest",
  "type": "object",
  "required": ["id", "name", "version", "enabled", "permissions", "menus", "dependencies"],
  "additionalProperties": false,
  "properties": {
    "id":     { "type": "string", "pattern": "^[a-z][a-z0-9]*(-[a-z0-9]+)*$" },
    "name":   { "type": "string", "minLength": 1 },
    "version":{ "type": "string", "pattern": "^\\d+\\.\\d+\\.\\d+$" },
    "enabled":{ "type": "boolean", "default": true },
    "permissions": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["code", "name", "type"],
        "additionalProperties": false,
        "properties": {
          "code": { "type": "string", "pattern": "^[a-z][a-z0-9-]*:[a-z][a-z0-9-]*:[a-z][a-z0-9-]*$" },
          "name": { "type": "string" },
          "type": { "enum": ["menu", "button", "field", "data"] }
        }
      }
    },
    "menus": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["label", "route"],
        "additionalProperties": false,
        "properties": {
          "label": { "type": "string" },
          "icon":  { "type": "string" },
          "route": { "type": "string", "pattern": "^/[a-z0-9/-]*$" },
          "perm":  { "type": "string" }
        }
      }
    },
    "dependencies": { "type": "array", "items": { "type": "string" } },
    "config": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "required": ["type", "default"],
        "properties": {
          "type":    { "enum": ["string", "number", "boolean", "array", "object"] },
          "default": {},
          "scope":   { "enum": ["platform", "tenant", "space"], "default": "tenant" },
          "label":   { "type": "string" }
        }
      }
    }
  }
}"##;

#[cfg(test)]
mod tests {
    use super::MANIFEST_JSON_SCHEMA;

    #[test]
    fn schema_is_valid_json() {
        let parsed: serde_json::Value =
            serde_json::from_str(MANIFEST_JSON_SCHEMA).expect("schema 必须是合法 JSON");
        assert_eq!(
            parsed["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(parsed["title"], "Base Platform Module Manifest");
    }

    #[test]
    fn schema_declares_the_required_top_level_fields() {
        let parsed: serde_json::Value =
            serde_json::from_str(MANIFEST_JSON_SCHEMA).expect("schema 必须是合法 JSON");
        let required = parsed["required"].as_array().expect("required 必须是数组");
        let names: Vec<&str> = required
            .iter()
            .map(|value| value.as_str().expect("required 元素必须是字符串"))
            .collect();
        for field in [
            "id",
            "name",
            "version",
            "enabled",
            "permissions",
            "menus",
            "dependencies",
        ] {
            assert!(names.contains(&field), "schema 缺少必备字段 {field}");
        }
        assert_eq!(parsed["additionalProperties"], serde_json::json!(false));
    }

    #[test]
    fn exported_schema_file_matches_the_constant() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../capabilities/schema/manifest.schema.json");
        let exported = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()));
        let exported: serde_json::Value =
            serde_json::from_str(&exported).expect("导出的 schema 必须是合法 JSON");
        let constant: serde_json::Value =
            serde_json::from_str(MANIFEST_JSON_SCHEMA).expect("内置 schema 必须是合法 JSON");
        assert_eq!(
            exported, constant,
            "capabilities/schema/manifest.schema.json 与内置常量不一致"
        );
    }
}
