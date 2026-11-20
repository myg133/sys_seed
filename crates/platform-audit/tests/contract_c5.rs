//! C-5（不依赖 `ContractFixture`）：`platform-audit` 的 3 个权限点各有鉴权调用点。
//!
//! # 为什么必须在这里（REQ-014 D160）
//!
//! C-5 的扫描入口 `check_c5_for(manifest_path, source_dir)`
//! **只扫「该 manifest 所属 crate 自己的 `src/`」**
//! （`crates/platform-contracts/src/checks.rs` 的 `check_c5_for`）。
//! 本需求的 manifest 在 `platform-audit`、HTTP 在 `platform-server`（棒 2）→
//! 若把 `require_permission("audit:events:read")` 直接写在 server 侧，
//! 对 `platform-audit` 跑 C-5 会报「3 个已登记权限点**没有鉴权调用点**」→ 违例。
//!
//! → 所以三个薄封装放在 `crates/platform-audit/src/authz.rs`（调用点与权限点
//! **字面量**同处一地），而**不**引入 `platform-audit → platform-rbac`
//! （鉴权入口在 **core**）。

use std::path::PathBuf;

use platform_contracts::authz_scan::scan_auth_calls;
use platform_contracts::check_c5_for;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_audit_permission_has_an_authorization_call_site() {
    let root = repo_root();
    let manifest = root.join("crates/platform-audit/manifest.json");
    let src = root.join("crates/platform-audit/src");

    let violations = check_c5_for(&manifest, &src).expect("扫描器应能运行");
    assert!(
        violations.is_empty(),
        "C-5 违例：{violations:?}\n  提示：权限点必须在 `crates/platform-audit/src/` 里用字面量调用 require_permission"
    );

    let scan = scan_auth_calls(&src).expect("调用点扫描应成功");
    let expected = ["audit:archive:run", "audit:events:read", "audit:verify:run"];
    for code in expected {
        assert!(
            scan.codes.iter().any(|found| found == code),
            "权限点 `{code}` 没有 `require_permission(\"字面量\")` 调用点；实际扫描到：{:?}",
            scan.codes
        );
    }
    assert_eq!(
        scan.codes.len(),
        3,
        "调用点集合应恰好是 3 个：{:?}",
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
#[test]
fn the_audit_manifest_is_valid() {
    let root = repo_root();
    let raw = std::fs::read_to_string(root.join("crates/platform-audit/manifest.json"))
        .expect("manifest 应可读");
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).expect("manifest 应合法");
    assert_eq!(manifest.permission_codes().len(), 3);
    assert_eq!(manifest.id, "audit");
}
