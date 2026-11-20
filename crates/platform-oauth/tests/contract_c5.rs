//! C-5（不依赖 `ContractFixture`）：`platform-oauth` 的 6 个权限点各有鉴权调用点。
//!
//! # 为什么必须在这里（REQ-022 D240）
//!
//! C-5 的扫描入口 `check_c5_for(manifest_path, source_dir)`
//! **只扫「该 manifest 所属 crate 自己的 `src/`」**
//! （`crates/platform-contracts/src/checks.rs` 的 `check_c5_for`）。
//! 本需求的 manifest 在 `platform-oauth`、HTTP 也**在同一个 crate** 里 →
//! 调用点与权限点编码**字面量**同处一地，C-5 扫得到。
//!
//! ⚠️ **真 C-5 命令**是 `cargo test -p platform-oauth --test contract_c5` ——
//! `cargo xtask capabilities --check` **不做** C-5（它只比对索引产物与磁盘）。
//!
//! # 为什么调用点写在 `src/authz.rs` 而不是 `src/http.rs`
//!
//! `platform_core::extension::permission::require_permission` 是**路由层**构造器
//! （返回 `PermissionLayer`，供 `.route_layer(...)` 用）→ 调用形态是
//! `.route_layer(require_permission("oauth:client:read"))`。
//!
//! ⚠️ **命名禁区（D216）**：`crates/platform-oauth/src/` 里**不得**出现
//! `check_permission` / `check_perm` / `has_permission` / `require_perm` /
//! `authorize` / `ensure_permission` / `verify_permission` / `permission_check`
//! 这 8 个标识符 —— 写了就是「自建鉴权入口」→ C-5 直接失败。
//! → 6 个薄封装住在 `src/authz.rs`（`require_client_read()` 等），
//! **不**引入 `platform-oauth → platform-rbac`（鉴权入口在 **core**）。

use std::path::PathBuf;

use platform_contracts::authz_scan::scan_auth_calls;
use platform_contracts::check_c5_for;

/// 仓库根目录。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 本需求登记的 6 个权限点（**逐字**，D214）。
const EXPECTED_CODES: [&str; 6] = [
    "oauth:client:read",
    "oauth:client:write",
    "oauth:client:delete",
    "oauth:grant:read",
    "oauth:grant:revoke",
    "oauth:token:introspect",
];

#[test]
fn every_oauth_permission_has_an_authorization_call_site() {
    let root = repo_root();
    let manifest = root.join("crates/platform-oauth/manifest.json");
    let src = root.join("crates/platform-oauth/src");

    let violations = check_c5_for(&manifest, &src).expect("扫描器应能运行");
    assert!(
        violations.is_empty(),
        "C-5 违例：{violations:?}\n  提示：权限点必须在 `crates/platform-oauth/src/` 里用字面量调用 require_permission"
    );

    let scan = scan_auth_calls(&src).expect("调用点扫描应成功");
    for code in EXPECTED_CODES {
        assert!(
            scan.codes.iter().any(|found| found == code),
            "权限点 `{code}` 没有 `require_permission(\"字面量\")` 调用点；实际扫描到：{:?}",
            scan.codes
        );
    }
    assert_eq!(
        scan.codes.len(),
        EXPECTED_CODES.len(),
        "调用点集合应恰好是 6 个：{:?}",
        scan.codes
    );
    assert!(
        scan.non_literal_calls.is_empty(),
        "调用点参数必须是字面量：{:?}",
        scan.non_literal_calls
    );
    assert!(
        scan.custom_entries.is_empty(),
        "不得自建鉴权入口（扫描会被绕过）：{:?}",
        scan.custom_entries
    );
}

/// manifest 本身必须合法（`Manifest::parse_checked` 的字段规则）。
///
/// ⚠️ 顺带覆盖 **6 个权限点**与 **`tenantScoped` 段**（D227）——
/// 后者是 4 张租户表的 RLS 策略的**唯一来源**。
#[test]
fn the_oauth_manifest_is_valid() {
    let root = repo_root();
    let raw = std::fs::read_to_string(root.join("crates/platform-oauth/manifest.json"))
        .expect("manifest 应可读");
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).expect("manifest 应合法");
    assert_eq!(manifest.id, "oauth");
    assert_eq!(manifest.permission_codes().len(), EXPECTED_CODES.len());
    for code in EXPECTED_CODES {
        assert!(
            manifest
                .permission_codes()
                .iter()
                .any(|found| *found == code),
            "manifest 缺少权限点 {code}"
        );
    }

    let scope = manifest
        .tenant_scoped
        .as_ref()
        .expect("manifest 必须声明 tenantScoped（D227）");
    assert_eq!(scope.column, "tenant_id");
    let mut tables = scope.tables.clone();
    tables.sort();
    assert_eq!(
        tables,
        vec![
            "platform_oauth_code".to_string(),
            "platform_oauth_device_code".to_string(),
            "platform_oauth_grant".to_string(),
            "platform_oauth_token".to_string(),
        ],
        "4 张租户表必须声明在 tenantScoped 里（平台级表 platform_oauth_client **不**声明）"
    );
}
