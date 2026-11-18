//! 组织树的**纯逻辑**部分：祖先链（`path_labels`）的计算与校验。
//!
//! # 权威来源是 `parent_id`，`path_labels` 只是加速字段（D5）
//!
//! `path_labels` 存一份物化的「祖先 id 列表」（从根到本节点，含本节点），
//! 写时维护、读时可用。**万一与 `parent_id` 不一致，以 `parent_id` 为准并可重建。**
//!
//! 本模块提供两件事：
//!
//! 1. [`expected_path`] —— 按 `parent_id` 链算出**应该**是什么；
//! 2. [`inconsistent_nodes`] —— 比对实际值与期望值，指出不一致的节点。
//!
//! # 为什么不用 ltree（D1）
//!
//! 私有化部署常常装不了 PostgreSQL 扩展 —— `CREATE EXTENSION ltree` 需要超级用户。
//! 邻接表 + 递归 CTE 是 **PG 原生能力**，零扩展依赖。
//! 组织树深度通常 < 10，递归 CTE 性能足够。

use std::collections::{BTreeMap, BTreeSet};

/// 祖先链的编码格式：JSON 数组（`["root-id","dept-id","team-id"]`）。
///
/// 用 JSON 而不是逗号分隔的裸串：ID 里出现分隔符时不会歧义，
/// 且可以直接被 PostgreSQL 的 `jsonb` 函数处理。
pub const EMPTY_PATH: &str = "[]";

/// 组织树的最大深度（防御性上限）。
///
/// 真实组织树深度 < 10；这个上限只用来**终止病态数据**（比如手改出来的环），
/// 防止校验逻辑自己跑成死循环。
pub const MAX_DEPTH: usize = 64;

/// 把祖先链编码成数据库里存的形式。
pub fn encode(path: &[String]) -> String {
    serde_json::to_string(path).unwrap_or_else(|_| EMPTY_PATH.to_string())
}

/// 解析数据库里存的祖先链。
///
/// 解析不了（历史脏数据 / 手工改坏）时返回**空链**而不是报错 ——
/// 加速字段读坏了不该让整棵树不可用；调用方按「与 `parent_id` 不一致」处理，
/// 重建即可。
pub fn decode(text: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(text).unwrap_or_default()
}

/// 子节点的祖先链 = 父节点的祖先链 + 自身 ID。
pub fn child_path(parent_path: &[String], node_id: &str) -> Vec<String> {
    let mut path = parent_path.to_vec();
    path.push(node_id.to_string());
    path
}

/// 按 `parent_id` 链算出某节点**应该**有的祖先链。
///
/// `links` 是「节点 ID → 父节点 ID」的映射（同一租户的全部节点）。
///
/// 返回的链从根到本节点（含本节点），与 [`child_path`] 的约定一致。
/// 链上有环时（病态数据）**就地停止**并返回已收集的部分 ——
/// 该节点随后会被 [`inconsistent_nodes`] 报成不一致，重建时人工处理。
pub fn expected_path(node_id: &str, links: &BTreeMap<String, Option<String>>) -> Vec<String> {
    let mut chain = vec![node_id.to_string()];
    let mut seen = BTreeSet::new();
    seen.insert(node_id.to_string());

    let mut current = links.get(node_id).cloned().flatten();
    let mut depth = 0usize;
    while let Some(parent_id) = current {
        depth += 1;
        if depth >= MAX_DEPTH {
            break;
        }
        if !seen.insert(parent_id.clone()) {
            // 环：停在这里，不再往上走。
            break;
        }
        chain.push(parent_id.clone());
        current = links.get(&parent_id).cloned().flatten();
    }
    chain.reverse();
    chain
}

/// 某条祖先链是否包含 `ancestor_id`。
///
/// 这是「本部门及下级」判定的**加速**路径（REQ-009 用）：
/// 命中即说明该节点在 `ancestor_id` 的子树里。
/// 但它**不是**权威判定 —— 权威判定走递归 CTE。
pub fn contains_ancestor(path: &[String], ancestor_id: &str) -> bool {
    path.iter().any(|id| id == ancestor_id)
}

/// 一个节点的祖先链与实际存储值不一致时，返回它的 ID。
///
/// 入参：
///
/// - `links`：节点 ID → 父节点 ID；
/// - `stored`：节点 ID → 数据库里存的 `path_labels`（已解码）。
pub fn inconsistent_nodes(
    links: &BTreeMap<String, Option<String>>,
    stored: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let mut broken = Vec::new();
    for node_id in links.keys() {
        let expected = expected_path(node_id, links);
        let actual = stored.get(node_id).cloned().unwrap_or_default();
        if expected != actual {
            broken.push(node_id.clone());
        }
    }
    broken.sort();
    broken
}

/// 按 `parent_id` 链重算某个节点的祖先链（重建用）。
///
/// 与 [`expected_path`] 等价，但只算一个节点 —— 移动子树时逐个重算。
pub fn rebuild_path(node_id: &str, links: &BTreeMap<String, Option<String>>) -> Vec<String> {
    expected_path(node_id, links)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(pairs: &[(&str, Option<&str>)]) -> BTreeMap<String, Option<String>> {
        pairs
            .iter()
            .map(|(id, parent)| ((*id).to_string(), parent.map(str::to_string)))
            .collect()
    }

    #[test]
    fn a_root_node_path_contains_only_itself() {
        let map = links(&[("root", None)]);
        assert_eq!(expected_path("root", &map), vec!["root".to_string()]);
    }

    #[test]
    fn a_three_level_tree_path_runs_from_the_root_to_the_node() {
        let map = links(&[
            ("root", None),
            ("dept", Some("root")),
            ("team", Some("dept")),
        ]);
        assert_eq!(
            expected_path("team", &map),
            vec!["root".to_string(), "dept".to_string(), "team".to_string()]
        );
    }

    #[test]
    fn moving_a_node_changes_the_expected_path_of_its_descendants() {
        let before = links(&[
            ("root", None),
            ("dept-a", Some("root")),
            ("dept-b", Some("root")),
            ("team", Some("dept-a")),
        ]);
        let after = links(&[
            ("root", None),
            ("dept-a", Some("root")),
            ("dept-b", Some("root")),
            ("team", Some("dept-b")),
        ]);
        assert_eq!(
            expected_path("team", &before),
            vec!["root", "dept-a", "team"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            expected_path("team", &after),
            vec!["root", "dept-b", "team"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn encoding_round_trips() {
        let path = vec!["a".to_string(), "b".to_string()];
        assert_eq!(decode(&encode(&path)), path);
        assert_eq!(encode(&[]), EMPTY_PATH);
        assert_eq!(decode("not json"), Vec::<String>::new());
    }

    #[test]
    fn a_cycle_does_not_hang_the_path_computation() {
        let map = links(&[("a", Some("b")), ("b", Some("a"))]);
        let path = expected_path("a", &map);
        assert!(path.len() <= MAX_DEPTH);
        assert!(path.contains(&"a".to_string()));
    }

    #[test]
    fn inconsistent_nodes_reports_only_the_broken_ones() {
        let map = links(&[
            ("root", None),
            ("dept", Some("root")),
            ("team", Some("dept")),
        ]);
        let mut stored = BTreeMap::new();
        stored.insert("root".to_string(), vec!["root".to_string()]);
        stored.insert(
            "dept".to_string(),
            vec!["root".to_string(), "dept".to_string()],
        );
        // team 的 path_labels 还停留在旧结构上（缺 dept）→ 不一致。
        stored.insert(
            "team".to_string(),
            vec!["root".to_string(), "team".to_string()],
        );

        let broken = inconsistent_nodes(&map, &stored);
        assert_eq!(broken, vec!["team".to_string()]);
    }

    #[test]
    fn a_consistent_tree_reports_nothing() {
        let map = links(&[
            ("root", None),
            ("dept", Some("root")),
            ("team", Some("dept")),
        ]);
        let stored = BTreeMap::from([
            ("root".to_string(), vec!["root".to_string()]),
            (
                "dept".to_string(),
                vec!["root".to_string(), "dept".to_string()],
            ),
            (
                "team".to_string(),
                vec!["root".to_string(), "dept".to_string(), "team".to_string()],
            ),
        ]);
        assert!(inconsistent_nodes(&map, &stored).is_empty());
    }

    #[test]
    fn ancestor_containment_supports_the_descendant_check() {
        let path = vec!["root".to_string(), "dept".to_string(), "team".to_string()];
        assert!(contains_ancestor(&path, "dept"));
        assert!(!contains_ancestor(&path, "other"));
    }
}
