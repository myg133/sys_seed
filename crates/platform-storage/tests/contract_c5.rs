//! C-5：`platform-storage` 的 3 个权限点各有鉴权调用点 + manifest 合法
//! + **D318 同款的源码 grep 断言**（REQ-018，D332）。

use std::path::PathBuf;

use platform_contracts::authz_scan::scan_auth_calls;
use platform_contracts::check_c5_for;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 本需求登记的 3 个权限点（**逐字**，D332）。
const EXPECTED_CODES: [&str; 3] = [
    "storage:file:read",
    "storage:file:write",
    "storage:job:read",
];

#[test]
fn every_storage_permission_has_an_authorization_call_site() {
    let root = repo_root();
    let manifest = root.join("crates/platform-storage/manifest.json");
    let src = root.join("crates/platform-storage/src");

    let violations = check_c5_for(&manifest, &src).expect("扫描器应能运行");
    assert!(
        violations.is_empty(),
        "C-5 违例：{violations:?}\n  提示：权限点必须在 `crates/platform-storage/src/` 里用字面量调用 require_permission"
    );

    let scan = scan_auth_calls(&src).expect("调用点扫描应成功");
    for code in EXPECTED_CODES {
        assert!(
            scan.codes.iter().any(|found| found == code),
            "权限点 `{code}` 没有 `require_permission(\"字面量\")` 调用点；实际：{:?}",
            scan.codes
        );
    }
    assert_eq!(
        scan.codes.len(),
        EXPECTED_CODES.len(),
        "调用点应恰好 3 个：{:?}",
        scan.codes
    );
    assert!(
        scan.non_literal_calls.is_empty(),
        "调用点参数必须是字面量：{:?}",
        scan.non_literal_calls
    );
    assert!(
        scan.custom_entries.is_empty(),
        "不得自建鉴权入口：{:?}",
        scan.custom_entries
    );
}

/// manifest 合法 + **4 张表全部**在 `tenantScoped` 里（AC-S-183 步骤 4）。
#[test]
fn the_storage_manifest_declares_all_five_tables() {
    let root = repo_root();
    let raw = std::fs::read_to_string(root.join("crates/platform-storage/manifest.json"))
        .expect("manifest 应可读");
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).expect("manifest 应合法");
    assert_eq!(manifest.id, "storage");
    assert_eq!(manifest.permission_codes().len(), EXPECTED_CODES.len());

    let scope = manifest
        .tenant_scoped
        .as_ref()
        .expect("必须声明 tenantScoped（D227）");
    assert_eq!(scope.column, "tenant_id");
    let mut tables = scope.tables.clone();
    tables.sort();
    assert_eq!(
        tables,
        vec![
            "platform_storage_job".to_string(),
            "platform_storage_job_error".to_string(),
            "platform_storage_object".to_string(),
            "platform_storage_probe_row".to_string(),
            "platform_storage_upload".to_string(),
        ],
        "**恰好 5 张**租户表（⚠️ 不是「包含」，是双向相等 —— 漏一张 = 该表完全无隔离）。\n\
         ⚠️ 第 5 张 `platform_storage_probe_row` 是 D333 新增的**导入目标表** ——\n\
         它存的是**业务数据**（不是元数据），漏隔离的后果比原来更严重：\n\
         A 租户导入的数据会出现在 B 租户的导出里。"
    );
}

/// **6 个**配置键都是 `scope: Platform` + 默认值逐字（D331）。
#[test]
fn every_storage_config_key_is_platform_scoped() {
    use platform_core::config::ConfigScope;

    let root = repo_root();
    let raw = std::fs::read_to_string(root.join("crates/platform-storage/manifest.json"))
        .expect("manifest 应可读");
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).expect("manifest 应合法");
    let config = manifest.config.expect("必须声明 config 段（D331）");

    assert_eq!(config.len(), 6, "恰好 6 个配置键");
    let expected = [
        ("storage.maxUploadBytes", 104_857_600_i64),
        ("storage.partSizeBytes", 8_388_608),
        ("storage.presignTtlSeconds", 900),
        ("storage.importBatchSize", 1_000),
        ("storage.maxRowBytes", 65_536),
        ("storage.uploadSessionTtlSeconds", 86_400),
    ];
    for (key, default) in expected {
        let spec = config
            .get(key)
            .unwrap_or_else(|| panic!("缺少配置键 {key}"));
        assert_eq!(
            spec.scope,
            ConfigScope::Platform,
            "配置键 `{key}` 的 scope 必须是 Platform（D331：漏写会落到 Tenant → 平台端点直接拒）"
        );
        assert_eq!(
            spec.default,
            serde_json::json!(default),
            "配置键 `{key}` 的默认值不对"
        );
        assert!(
            spec.label.as_deref().is_some_and(|l| !l.trim().is_empty()),
            "配置键 `{key}` 必须有非空 label"
        );
    }

    // 键名逐段 lowerCamelCase（契约 C9：带 `_` 会让服务起不来）。
    for key in config.keys() {
        for segment in key.split('.') {
            let first = segment.chars().next().expect("段非空");
            assert!(
                first.is_ascii_lowercase(),
                "段 {segment} 必须以小写字母开头"
            );
            assert!(
                segment.chars().all(|c| c.is_ascii_alphanumeric()),
                "段 {segment} 只能含 ASCII 字母数字（带 _ 会让服务起不来）"
            );
        }
    }
}

/// ⚠️ **D318 同款**：`all_codes()` 必须引用本 crate 的 `ALL_CODES`。
///
/// **判据必须是读源码** —— 漏加时 `titles_cover_every_code` **恰好是绿的**。
#[test]
fn the_server_test_registers_the_storage_error_codes() {
    let root = repo_root();
    let source = std::fs::read_to_string(root.join("crates/platform-server/tests/i18n_titles.rs"))
        .expect("i18n_titles.rs 应可读");

    // 阳性对照：既有的 app / notify 那两行必须找得到（证明模式有效）。
    assert!(
        source.contains("platform_app::error::ALL_CODES"),
        "阳性对照失败：连既有的 platform_app 都没找到"
    );
    assert!(
        source.contains("platform_notify::error::ALL_CODES"),
        "阳性对照失败：连既有的 platform_notify 都没找到"
    );
    assert!(
        source.contains("platform_storage::error::ALL_CODES"),
        "`all_codes()` 必须加 `platform_storage::error::ALL_CODES` —— \
         漏加时 `titles_cover_every_code` **恰好是绿的**（D318）"
    );
    assert!(
        source.contains("const REQ_018_NEW_CODES: usize = 9;"),
        "钉死行数必须计入 `REQ_018_NEW_CODES = 9`（107 + 9 = 116）"
    );
}
