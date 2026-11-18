//! `manifest.json` 的 JSON Schema（正式版）。
//!
//! 同一份内容导出到 `capabilities/schema/manifest.schema.json`
//! （REQ-004 的能力索引收录）。
//!
//! 校验由 `crate::extension::manifest` 里的 Rust 代码完成 —— **不引入** `jsonschema`
//! 之类的校验库，保持依赖最小（低配机冷编译敏感）。
//!
//! # 与设计摘要 §5 的差异（REQ-003 修复轮）
//!
//! 本 schema 收紧到与平台实现**完全一致**，避免第三方按 schema 校验的结果与平台判定不同：
//!
//! | 字段 | 设计摘要 §5 | 本 schema | 原因 |
//! |------|-------------|-----------|------|
//! | `id` | `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` | `^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$` | 与平台实现 `is_module_id` 等价（分段首字符必须是字母） |
//! | `dependencies[]` | 无 pattern | 与 `id` 同 pattern | 平台按 id 规则校验依赖 ID |
//! | `name` | 只有 `minLength: 1` | 追加 `pattern: \S` | 平台判「去掉首尾空白后为空」即非法 |
//! | `permissions[].name` | 无 minLength | `minLength: 1` + `pattern: \S` | 同上 |
//!
//! 设计摘要 §5 需要 BA 同步这几处（见 `feature-meta/verification-report.md`）。

/// `manifest.json` 的 JSON Schema（draft 2020-12）。
pub const MANIFEST_JSON_SCHEMA: &str = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Base Platform Module Manifest",
  "type": "object",
  "required": ["id", "name", "version", "enabled", "permissions", "menus", "dependencies"],
  "additionalProperties": false,
  "properties": {
    "id":     { "type": "string", "pattern": "^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$" },
    "name":   { "type": "string", "minLength": 1, "pattern": "\\S" },
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
          "name": { "type": "string", "minLength": 1, "pattern": "\\S" },
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
    "dependencies": { "type": "array", "items": { "type": "string", "pattern": "^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$" } },
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
    },
    "tenantScoped": {
      "type": "object",
      "required": ["tables"],
      "additionalProperties": false,
      "properties": {
        "tables": {
          "type": "array",
          "minItems": 1,
          "items": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$" }
        },
        "column": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$", "default": "tenant_id" }
      }
    }
  }
}"##;

#[cfg(test)]
mod tests {
    use super::MANIFEST_JSON_SCHEMA;
    use crate::extension::manifest::{Manifest, is_module_id};
    use serde_json::Value;

    /// id 样本表：`(样本, 平台实现与 schema 必须给出的同一个判定)`。
    const MODULE_ID_SAMPLES: [(&str, bool); 15] = [
        ("sandbox", true),
        ("demo", true),
        ("demo2", true),
        ("a-b-c", true),
        ("a-b1", true),
        ("a1-b", true),
        ("sandbox-1demo", false),
        ("a-1", false),
        ("sandbox-1", false),
        ("demo-2", false),
        ("Demo", false),
        ("demo_x", false),
        ("", false),
        ("demo-", false),
        ("a--b", false),
    ];

    /// 权限点名称样本表：`(样本, 平台实现与 schema 必须给出的同一个判定)`。
    const PERMISSION_NAME_SAMPLES: [(&str, bool); 7] = [
        ("Read", true),
        ("读取", true),
        ("  x  ", true),
        ("", false),
        (" ", false),
        ("\t", false),
        ("   ", false),
    ];

    /// 解析内置 schema。
    fn schema_json() -> Value {
        serde_json::from_str(MANIFEST_JSON_SCHEMA).expect("schema 必须是合法 JSON")
    }

    /// 平台实现对某个权限点名称的判定。
    fn platform_accepts_permission_name(name: &str) -> bool {
        let raw = format!(
            r#"{{
              "id": "demo",
              "name": "Demo",
              "version": "0.1.0",
              "enabled": true,
              "permissions": [ {{ "code": "demo:demo:read", "name": {name:?}, "type": "menu" }} ],
              "menus": [],
              "dependencies": []
            }}"#
        );
        Manifest::parse_checked(&raw).is_ok()
    }

    /// schema 对某个权限点名称的判定（按 `minLength` + `pattern`）。
    fn schema_accepts_permission_name(name: &str) -> bool {
        let schema = schema_json();
        let rule = &schema["properties"]["permissions"]["items"]["properties"]["name"];
        let min_length = rule["minLength"].as_u64().unwrap_or_default() as usize;
        let pattern = rule["pattern"].as_str().expect("name 必须声明 pattern");
        name.chars().count() >= min_length && pattern::matches(pattern, name)
    }

    #[test]
    fn schema_is_valid_json() {
        let parsed = schema_json();
        assert_eq!(
            parsed["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(parsed["title"], "Base Platform Module Manifest");
    }

    #[test]
    fn schema_declares_the_required_top_level_fields() {
        let parsed = schema_json();
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
        let exported: Value =
            serde_json::from_str(&exported).expect("导出的 schema 必须是合法 JSON");
        let constant: Value =
            serde_json::from_str(MANIFEST_JSON_SCHEMA).expect("内置 schema 必须是合法 JSON");
        assert_eq!(
            exported, constant,
            "capabilities/schema/manifest.schema.json 与内置常量不一致"
        );
    }

    #[test]
    fn schema_declares_the_tenant_scoped_section() {
        let parsed = schema_json();
        let spec = &parsed["properties"]["tenantScoped"];
        assert_eq!(spec["type"], "object");
        assert_eq!(spec["additionalProperties"], serde_json::json!(false));
        let required = spec["required"].as_array().expect("required 必须是数组");
        assert!(
            required.iter().any(|value| value == "tables"),
            "tenantScoped 必须要求声明 tables"
        );
        let table_pattern = spec["properties"]["tables"]["items"]["pattern"]
            .as_str()
            .expect("tables[] 必须声明 pattern");
        let column_pattern = spec["properties"]["column"]["pattern"]
            .as_str()
            .expect("column 必须声明 pattern");
        for sample in ["order_item", "t1"] {
            assert!(
                pattern::matches(table_pattern, sample),
                "schema 的表名 pattern 对 `{sample}` 的判定与样本不符"
            );
        }
        for sample in ["Order_Item", "order-item", "1order"] {
            assert!(
                !pattern::matches(table_pattern, sample),
                "schema 的表名 pattern 不该接受 `{sample}`"
            );
        }
        assert!(
            pattern::matches(column_pattern, "tenant_id"),
            "schema 的列名 pattern 必须接受 tenant_id"
        );
    }

    /// 平台实现与 schema 对标识符的判定必须一致（表名 / 列名）。
    #[test]
    fn schema_and_implementation_agree_on_sql_identifier_samples() {
        use crate::extension::manifest::is_sql_identifier;
        let parsed = schema_json();
        let table_pattern =
            parsed["properties"]["tenantScoped"]["properties"]["tables"]["items"]["pattern"]
                .as_str()
                .expect("tables[] 必须声明 pattern");
        let column_pattern =
            parsed["properties"]["tenantScoped"]["properties"]["column"]["pattern"]
                .as_str()
                .expect("column 必须声明 pattern");
        for sample in ["order_item", "t1", "Order_Item", "order-item", "1order", ""] {
            let expected = is_sql_identifier(sample);
            assert_eq!(
                pattern::matches(table_pattern, sample),
                expected,
                "schema 的表名 pattern 对 `{sample}` 的判定与平台实现不符"
            );
            assert_eq!(
                pattern::matches(column_pattern, sample),
                expected,
                "schema 的列名 pattern 对 `{sample}` 的判定与平台实现不符"
            );
        }
    }

    #[test]
    fn schema_and_implementation_agree_on_module_id_samples() {
        let schema = schema_json();
        let id_pattern = schema["properties"]["id"]["pattern"]
            .as_str()
            .expect("id 必须声明 pattern");
        let dependency_pattern = schema["properties"]["dependencies"]["items"]["pattern"]
            .as_str()
            .expect("dependencies[] 必须声明 pattern");

        for (sample, expected) in MODULE_ID_SAMPLES {
            assert_eq!(
                is_module_id(sample),
                expected,
                "平台实现对 `{sample}` 的判定与样本表不符"
            );
            assert_eq!(
                pattern::matches(id_pattern, sample),
                expected,
                "schema 的 id pattern 对 `{sample}` 的判定与样本表不符"
            );
            assert_eq!(
                pattern::matches(dependency_pattern, sample),
                expected,
                "schema 的 dependencies pattern 对 `{sample}` 的判定与样本表不符"
            );
        }
    }

    #[test]
    fn schema_and_implementation_agree_on_permission_name_samples() {
        for (sample, expected) in PERMISSION_NAME_SAMPLES {
            assert_eq!(
                platform_accepts_permission_name(sample),
                expected,
                "平台实现对权限点名称 `{sample}` 的判定与样本表不符"
            );
            assert_eq!(
                schema_accepts_permission_name(sample),
                expected,
                "schema 对权限点名称 `{sample}` 的判定与样本表不符"
            );
        }
    }

    /// 极简正则求值器：只支持本 schema 用到的构造
    /// （`^` / `$` / 字面量 / `[a-z]` / `[a-z0-9]` / `\S` / `*` / `+` / `(...)`）。
    ///
    /// 为什么自己写：仓库不引入 `regex` / `jsonschema` 依赖（低配机冷编译敏感），
    /// 但「已发布的 schema 与平台实现必须给出相同判定」这条约束需要被测到 ——
    /// 用一个**独立于平台实现**的求值器去读 schema 里的 pattern，
    /// 两边对同一批样本给出不同结论时测试就会红。
    mod pattern {
        /// 正则节点。
        #[derive(Debug, Clone, PartialEq, Eq)]
        enum Node {
            /// 字面量字符。
            Literal(char),
            /// 字符类（闭区间列表）。
            Class(Vec<(char, char)>),
            /// 非空白字符（`\S`）。
            NonWhitespace,
            /// 行首锚点（`^`）。
            StartAnchor,
            /// 行尾锚点（`$`）。
            EndAnchor,
            /// 连续节点。
            Concat(Vec<Node>),
            /// 重复节点（至少 `min` 次，无上限）。
            Repeat { node: Box<Node>, min: usize },
        }

        /// 整个串是否匹配该 pattern（ECMA-262 的「存在一处匹配」语义）。
        pub fn matches(pattern: &str, sample: &str) -> bool {
            let nodes = parse(pattern);
            let input: Vec<char> = sample.chars().collect();
            (0..=input.len()).any(|start| {
                positions(&nodes, &input, start)
                    .iter()
                    .any(|end| *end > start)
            })
        }

        /// 解析 pattern。
        fn parse(pattern: &str) -> Vec<Node> {
            let chars: Vec<char> = pattern.chars().collect();
            let mut pos = 0;
            parse_sequence(&chars, &mut pos)
        }

        /// 解析一段节点（遇到 `)` 或结尾停止）。
        fn parse_sequence(chars: &[char], pos: &mut usize) -> Vec<Node> {
            let mut nodes = Vec::new();
            while *pos < chars.len() {
                match chars[*pos] {
                    '^' => {
                        *pos += 1;
                        nodes.push(Node::StartAnchor);
                    }
                    '$' => {
                        *pos += 1;
                        nodes.push(Node::EndAnchor);
                    }
                    ')' => break,
                    '(' => {
                        *pos += 1;
                        let inner = parse_sequence(chars, pos);
                        *pos += 1;
                        nodes.push(Node::Concat(inner));
                    }
                    '[' => {
                        *pos += 1;
                        nodes.push(Node::Class(parse_class(chars, pos)));
                    }
                    '\\' => {
                        *pos += 1;
                        let escaped = chars.get(*pos).copied().unwrap_or('\\');
                        *pos += 1;
                        nodes.push(match escaped {
                            'S' => Node::NonWhitespace,
                            other => Node::Literal(other),
                        });
                    }
                    '*' | '+' => {
                        let min = usize::from(chars[*pos] == '+');
                        *pos += 1;
                        apply_repeat(&mut nodes, min);
                    }
                    literal => {
                        *pos += 1;
                        nodes.push(Node::Literal(literal));
                    }
                }
            }
            nodes
        }

        /// 解析字符类 `[...]`。
        fn parse_class(chars: &[char], pos: &mut usize) -> Vec<(char, char)> {
            let mut ranges = Vec::new();
            while *pos < chars.len() && chars[*pos] != ']' {
                let start = chars[*pos];
                *pos += 1;
                let is_range =
                    chars.get(*pos) == Some(&'-') && chars.get(*pos + 1).is_some_and(|c| *c != ']');
                if is_range {
                    *pos += 1;
                    let end = chars[*pos];
                    *pos += 1;
                    ranges.push((start, end));
                } else {
                    ranges.push((start, start));
                }
            }
            *pos += 1;
            ranges
        }

        /// 把重复标记套到最后一个节点上。
        fn apply_repeat(nodes: &mut Vec<Node>, min: usize) {
            if let Some(last) = nodes.pop() {
                nodes.push(Node::Repeat {
                    node: Box::new(last),
                    min,
                });
            }
        }

        /// 节点序列在 `pos` 处能到达的结束位置（贪心优先，回溯兜底）。
        fn positions(nodes: &[Node], input: &[char], pos: usize) -> Vec<usize> {
            match nodes.split_first() {
                None => vec![pos],
                Some((first, rest)) => {
                    let mut result = Vec::new();
                    for next in node_positions(first, input, pos) {
                        result.extend(positions(rest, input, next));
                    }
                    result
                }
            }
        }

        /// 单个节点在 `pos` 处能到达的结束位置。
        fn node_positions(node: &Node, input: &[char], pos: usize) -> Vec<usize> {
            match node {
                Node::Literal(expected) => {
                    if input.get(pos) == Some(expected) {
                        vec![pos + 1]
                    } else {
                        Vec::new()
                    }
                }
                Node::Class(ranges) => {
                    let hit = input
                        .get(pos)
                        .is_some_and(|c| ranges.iter().any(|(start, end)| start <= c && c <= end));
                    if hit { vec![pos + 1] } else { Vec::new() }
                }
                Node::NonWhitespace => {
                    if input.get(pos).is_some_and(|c| !c.is_whitespace()) {
                        vec![pos + 1]
                    } else {
                        Vec::new()
                    }
                }
                Node::StartAnchor => {
                    if pos == 0 {
                        vec![pos]
                    } else {
                        Vec::new()
                    }
                }
                Node::EndAnchor => {
                    if pos == input.len() {
                        vec![pos]
                    } else {
                        Vec::new()
                    }
                }
                Node::Concat(nodes) => positions(nodes, input, pos),
                Node::Repeat { node, min } => repeat_positions(node, *min, input, pos),
            }
        }

        /// 重复节点能到达的结束位置（次数从多到少，贪心优先）。
        fn repeat_positions(node: &Node, min: usize, input: &[char], pos: usize) -> Vec<usize> {
            let mut levels: Vec<Vec<usize>> = vec![vec![pos]];
            loop {
                let current = levels.last().expect("至少有一层").clone();
                let mut next = Vec::new();
                for start in &current {
                    for end in node_positions(node, input, *start) {
                        if end > *start {
                            next.push(end);
                        }
                    }
                }
                next.sort_unstable();
                next.dedup();
                if next.is_empty() {
                    break;
                }
                levels.push(next);
            }
            let mut result = Vec::new();
            for level in (min..levels.len()).rev() {
                result.extend(levels[level].iter().copied());
            }
            result
        }
    }
}
