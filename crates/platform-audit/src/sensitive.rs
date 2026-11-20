//! 敏感键扫描器：Rust 侧与 DB 侧的**同源**视图（REQ-014 D156）。
//!
//! # 为什么两侧都要做
//!
//! | 侧 | 角色 | 为什么 |
//! |----|------|--------|
//! | DB（`platform_audit_sensitive_keys`） | **权威** | 真正的「写入前」拦截 —— 绕过应用直接调函数也会被拒 |
//! | Rust（本模块 / core） | **提前拦截 + 可单独调用** | AC-S-133 步骤 4 要求「用一个**可单独调用**的扫描器扫一段样例串」→ 必须是 Rust 侧 |
//!
//! # 为什么必须有一致性测试
//!
//! 两边各写一套键集合 → 迟早漂移（DB 少一个键 = 那一个键能绕过防护）。
//! → [`db_sensitive_keys`] 从**迁移 SQL 里**抽出 DB 侧的键数组，
//! 与 Rust 侧的 [`SENSITIVE_KEYS`] 逐项比对（一致性测试在 `tests/` 下）。

use std::collections::BTreeSet;
use std::path::Path;

pub use platform_core::extension::audit::{
    SENSITIVE_KEYS, normalize_sensitive_key, scan_sensitive_keys,
};

/// 迁移 SQL 里 DB 侧键数组的**标记**（成对出现）。
///
/// ⚠️ 改这两行标记会让一致性测试**取不到数组** → 测试直接失败（不是静默通过）。
pub const BASELINE_BEGIN_MARKER: &str = "-- SENSITIVE-KEY-BASELINE-BEGIN";
/// 迁移 SQL 里 DB 侧键数组的**结束**标记。
pub const BASELINE_END_MARKER: &str = "-- SENSITIVE-KEY-BASELINE-END";

/// 从迁移 SQL 里抽出 DB 侧的敏感键集合。
///
/// 抽的是**标记之间**的所有单引号字符串 —— 即 DB 函数里那个 `ARRAY[...]` 的内容。
///
/// # 失败即 panic
///
/// 标记找不到 / 抽不出键 → **panic**（不是返回空集合）：
/// 返回空集合会让「两边一致」变成「两边都空」→ **假绿**。
pub fn db_sensitive_keys(migration_sql: &str) -> BTreeSet<String> {
    let begin = migration_sql
        .find(BASELINE_BEGIN_MARKER)
        .unwrap_or_else(|| panic!("the migration is missing the `{BASELINE_BEGIN_MARKER}` marker"));
    let after_begin = &migration_sql[begin..];
    let end = after_begin
        .find(BASELINE_END_MARKER)
        .unwrap_or_else(|| panic!("the migration is missing the `{BASELINE_END_MARKER}` marker"));
    let block = &after_begin[..end];

    let keys: BTreeSet<String> = block
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .collect();
    assert!(
        !keys.is_empty(),
        "the DB-side baseline array is empty — the extraction is broken"
    );
    keys
}

/// 读迁移文件并抽出 DB 侧的敏感键集合。
pub fn db_sensitive_keys_from_file(migration_path: &Path) -> BTreeSet<String> {
    let sql = std::fs::read_to_string(migration_path).unwrap_or_else(|error| {
        panic!("cannot read {}: {error}", migration_path.display());
    });
    db_sensitive_keys(&sql)
}

/// Rust 侧的敏感键集合（归一化后的键名）。
pub fn rust_sensitive_keys() -> BTreeSet<String> {
    SENSITIVE_KEYS.iter().map(|key| key.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rust_side_has_fourteen_keys() {
        assert_eq!(rust_sensitive_keys().len(), 14);
    }

    #[test]
    fn the_extraction_finds_the_keys_between_the_markers() {
        let sql = "-- SENSITIVE-KEY-BASELINE-BEGIN\nARRAY['password','api_key']\n-- SENSITIVE-KEY-BASELINE-END\n";
        let keys = db_sensitive_keys(sql);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains("password"));
        assert!(keys.contains("api_key"));
    }
}
