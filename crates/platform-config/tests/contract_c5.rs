//! C-5（不依赖 `ContractFixture`）：`platform-config` 的 4 个权限点各有鉴权调用点。
//!
//! # 为什么需要新入口（D58）
//!
//! `ContractFixture` 要求 `Repository` + `PlatformModule` —— **平台 crate 没有也不该有**。
//! 所以 `platform_contracts::check_c5_for(manifest_path, src_dir)` 是本需求新增的扫描入口。
//!
//! # 覆盖
//!
//! 1. 真实 crate：4 个权限点**逐个**有 `require_permission("字面量")` 调用点；
//! 2. 负向：复制到临时目录，删掉任一调用点 → **失败并点名**（不降级为告警）；
//! 3. 基线快照：`platform-org`（9 个登记 / 0 调用点）、`platform-rbac`
//!    （4 个登记 / 3 调用点，`rbac:permission:read` 缺）—— **差值 ≥ 0**，不扩围。

use std::path::{Path, PathBuf};

use platform_contracts::authz_scan::scan_auth_calls;
use platform_contracts::check_c5_for;
use platform_core::extension::manifest::Manifest;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).expect("临时目录应能创建");
    for entry in std::fs::read_dir(source).expect("源目录应可读") {
        let entry = entry.expect("目录项");
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &target.join(path.file_name().unwrap()));
        } else {
            std::fs::copy(&path, target.join(path.file_name().unwrap())).expect("复制应成功");
        }
    }
}

fn remove_dir(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

fn scan_violations(manifest: &Path, src: &Path) -> Vec<platform_contracts::ContractViolation> {
    check_c5_for(manifest, src).expect("扫描器应能运行（不依赖 ContractFixture）")
}

#[test]
fn every_config_permission_has_an_authorization_call_site() {
    let root = repo_root();
    let manifest = root.join("crates/platform-config/manifest.json");
    let src = root.join("crates/platform-config/src");

    let violations = scan_violations(&manifest, &src);
    assert!(
        violations.is_empty(),
        "C-5 违例：{violations:?}\n  提示：权限点必须在**路由层**用字面量调用 require_permission"
    );

    let scan = scan_auth_calls(&src).expect("调用点扫描应成功");
    let expected = [
        "config:entry:write",
        "config:flag:write",
        "config:platform:write",
        "config:platform:read",
    ];
    for code in expected {
        assert!(
            scan.codes.iter().any(|found| found == code),
            "权限点 `{code}` 没有 `require_permission(\"字面量\")` 调用点；实际扫描到：{:?}",
            scan.codes
        );
    }
    assert_eq!(
        scan.codes.len(),
        4,
        "调用点集合应恰好是 4 个：{:?}",
        scan.codes
    );
}

#[test]
fn removing_one_call_site_fails_and_names_the_permission() {
    let root = repo_root();
    let suffix = uuid::Uuid::new_v4().to_string().replace('-', "");
    let temp = std::env::temp_dir().join(format!("c5-config-{}", &suffix[..12]));
    copy_dir(&root.join("crates/platform-config/src"), &temp.join("src"));
    std::fs::copy(
        root.join("crates/platform-config/manifest.json"),
        temp.join("manifest.json"),
    )
    .expect("复制 manifest");
    let manifest = temp.join("manifest.json");
    let src = temp.join("src");

    // 1) 未改动 → 0 违例。
    assert!(scan_violations(&manifest, &src).is_empty());

    // 2) 删掉 `config:flag:write` 的调用点 → 恰好 1 条，点名该权限点。
    let http = src.join("http.rs");
    let source = std::fs::read_to_string(&http).expect("读 http.rs");
    let mutated = source.replace(
        "require_permission(\"config:flag:write\")",
        "not_a_call(\"config:flag:write\")",
    );
    assert_ne!(source, mutated, "负向用例没改到调用点（夹具失效）");
    std::fs::write(&http, &mutated).expect("写临时源文件");

    let violations = scan_violations(&manifest, &src);
    assert_eq!(violations.len(), 1, "违例：{violations:?}");
    assert_eq!(violations[0].check_id, "C-5");
    assert!(
        violations[0].message.contains("config:flag:write"),
        "必须点名被删的权限点：{}",
        violations[0].message
    );
    assert!(
        violations[0].message.contains("no authorization call site"),
        "必须是「没有调用点」：{}",
        violations[0].message
    );

    // 3) 再删 `config:platform:read` → 两条，分别点名。
    let mutated = std::fs::read_to_string(&http).unwrap().replace(
        "require_permission(\"config:platform:read\")",
        "not_a_call(\"config:platform:read\")",
    );
    std::fs::write(&http, mutated).unwrap();
    let violations = scan_violations(&manifest, &src);
    assert_eq!(violations.len(), 2, "违例：{violations:?}");
    let text = violations
        .iter()
        .map(|violation| violation.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("config:flag:write"));
    assert!(text.contains("config:platform:read"));
    // **不得**把另外两个也误报。
    assert!(!text.contains("config:entry:write"));
    assert!(!text.contains("config:platform:write"));

    remove_dir(&temp);
}

#[test]
fn the_existing_platform_crate_baseline_does_not_get_worse() {
    let root = repo_root();

    // --- platform-org：9 个登记、0 个调用点（既有技术债，本需求**不扩围**）。
    let org_manifest = root.join("crates/platform-org/manifest.json");
    let org_raw = std::fs::read_to_string(&org_manifest).expect("org manifest");
    let org = Manifest::parse_checked(&org_raw).expect("org manifest 应合法");
    assert_eq!(org.permission_codes().len(), 9, "org 的登记数不得变化");
    let org_scan = scan_auth_calls(&root.join("crates/platform-org/src")).expect("org 扫描");
    // 基线快照：0 个调用点。`== 0` 比「差值 ≥ 0」更严：既不许恶化，也不许在本需求里
    // 顺手扩围（扩了就要按 C-5 全量修 9 个点，超出本需求范围）。
    assert_eq!(org_scan.codes.len(), 0, "基线是 0；本需求不应顺手改它");

    // --- platform-rbac：4 个登记、3 个调用点；`rbac:permission:read` 仍是缺口。
    let rbac_manifest = root.join("crates/platform-rbac/manifest.json");
    let rbac_raw = std::fs::read_to_string(&rbac_manifest).expect("rbac manifest");
    let rbac = Manifest::parse_checked(&rbac_raw).expect("rbac manifest 应合法");
    assert_eq!(rbac.permission_codes().len(), 4, "rbac 的登记数不得变化");
    let rbac_scan = scan_auth_calls(&root.join("crates/platform-rbac/src")).expect("rbac 扫描");
    assert!(
        rbac_scan.codes.len() >= 3,
        "调用点数不得低于基线 3（当前：{:?}）",
        rbac_scan.codes
    );
    assert!(
        !rbac_scan
            .codes
            .iter()
            .any(|code| code == "rbac:permission:read"),
        "基线快照：`rbac:permission:read` 仍无调用点（登记为技术债）"
    );

    // --- sandbox 的契约测试**不能**替代平台 crate 的 C-5：
    //     它的 manifest 里没有任何 `config:` 权限点。
    let sandbox_raw =
        std::fs::read_to_string(root.join("modules/sandbox/manifest.json")).expect("sandbox");
    let sandbox = Manifest::parse_checked(&sandbox_raw).expect("sandbox manifest 应合法");
    assert!(
        !sandbox
            .permission_codes()
            .iter()
            .any(|code| code.starts_with("config:")),
        "sandbox 不应登记 config 权限点"
    );
}
