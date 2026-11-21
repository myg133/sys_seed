//! C-5（不依赖 `ContractFixture`）：`platform-notify` 的 3 个权限点各有鉴权调用点。
//!
//! # 为什么必须在这里（REQ-015 D300 / D240 同款）
//!
//! 契约检查 C-5 的入口 `check_c5_for(manifest_path, source_dir)`
//! **只扫「该 manifest 所属 crate 自己的 `src/`」**
//! （`crates/platform-contracts/src/checks.rs` 的 `check_c5_for`）。
//! 本需求的 manifest 在 `platform-notify`、HTTP 也**在同一个 crate** 里 →
//! 调用点与权限点编码**字面量**同处一地，C-5 扫得到。
//!
//! ⚠️ **真 C-5 命令**是 `cargo test -p platform-notify --test contract_c5` ——
//! `cargo xtask capabilities --check` **不做** C-5（它只比对索引产物与磁盘）。
//!
//! # ⚠️ 命名禁区（D216）—— 通知域**极易踩**
//!
//! `crates/platform-notify/src/` 里**不得**出现这 8 个标识符：
//!
//! ```text
//! check_permission   check_perm     has_permission   require_perm
//! authorize          ensure_permission  verify_permission  permission_check
//! ```
//!
//! 通知域里「**是否允许给这个用户发**」听起来很像鉴权 —— 但它**不是**，
//! 那是**业务规则**（订阅偏好）。那种函数叫 `is_recipient_opted_out` /
//! `is_recipient_eligible`，**不叫** `authorize_*`。

use std::path::PathBuf;

use platform_contracts::authz_scan::scan_auth_calls;
use platform_contracts::check_c5_for;

/// 仓库根目录。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 本需求登记的 3 个权限点（**逐字**，D300）。
const EXPECTED_CODES: [&str; 3] = [
    "notify:template:read",
    "notify:template:write",
    "notify:delivery:read",
];

#[test]
fn every_notify_permission_has_an_authorization_call_site() {
    let root = repo_root();
    let manifest = root.join("crates/platform-notify/manifest.json");
    let src = root.join("crates/platform-notify/src");

    let violations = check_c5_for(&manifest, &src).expect("扫描器应能运行");
    assert!(
        violations.is_empty(),
        "C-5 违例：{violations:?}\n  提示：权限点必须在 `crates/platform-notify/src/` 里用字面量调用 require_permission"
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

/// manifest 本身必须合法，且 **3 张表**都声明在 `tenantScoped` 里。
///
/// ⚠️ **D311 / S-04 的判据**：原先只有 `delivery` 一张表有 AC 验 RLS。
/// `tenantScoped` 是 RLS 策略的**唯一来源**（D227）—— 漏声明某张表，
/// `cargo xtask tenant-policies --check` **会绿**（没声明就不生成也不校验）
/// → 那张表**完全无隔离**。
#[test]
fn the_notify_manifest_is_valid() {
    let root = repo_root();
    let raw = std::fs::read_to_string(root.join("crates/platform-notify/manifest.json"))
        .expect("manifest 应可读");
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).expect("manifest 应合法");
    assert_eq!(manifest.id, "notify");
    assert_eq!(manifest.permission_codes().len(), EXPECTED_CODES.len());
    for code in EXPECTED_CODES {
        assert!(
            manifest.permission_codes().contains(&code),
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
            "platform_notify_delivery".to_string(),
            "platform_notify_preference".to_string(),
            "platform_notify_template".to_string(),
        ],
        "3 张租户表**全部**必须声明在 tenantScoped 里（D311：漏一张 = 该表完全无隔离）"
    );
}

/// ⚠️ **D304 的判据**：**5 个**配置键**都**是 `scope: Platform`，且是 `ConfigSpec` 对象。
///
/// `ConfigSpec.scope` 的 serde 默认值是 **`Tenant`** →
/// 漏写 `"scope": "platform"` 就落到租户作用域 → 平台级读写端点直接拒
/// → 退避参数**永不生效**，且**只有运行时静默过滤**（REQ-016 的 S-01 教训）。
#[test]
fn every_notify_config_key_is_platform_scoped() {
    use platform_core::config::ConfigScope;

    let root = repo_root();
    let raw = std::fs::read_to_string(root.join("crates/platform-notify/manifest.json"))
        .expect("manifest 应可读");
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).expect("manifest 应合法");
    let config = manifest.config.expect("manifest 必须声明 config 段（D304）");

    assert_eq!(config.len(), 5, "恰好 5 个配置键（D304 rev 2）");
    for key in [
        "notify.maxAttempts",
        "notify.retryBaseSeconds",
        "notify.retryMultiplier",
        "notify.retryMaxSeconds",
        "notify.deliveringTimeoutSeconds",
    ] {
        let spec = config.get(key).unwrap_or_else(|| panic!("缺少配置键 {key}"));
        assert_eq!(
            spec.scope,
            ConfigScope::Platform,
            "配置键 `{key}` 的 scope 必须是 Platform（D304：漏写会落到 Tenant → 平台端点直接拒）"
        );
        assert!(
            spec.label
                .as_deref()
                .is_some_and(|label| !label.trim().is_empty()),
            "配置键 `{key}` 必须有非空 label（配置界面要标题）"
        );
    }

    // 默认值与 D297 / D313 逐字一致。
    assert_eq!(
        config["notify.maxAttempts"].default,
        serde_json::json!(5),
        "`notify.maxAttempts` 默认必须是 5"
    );
    assert_eq!(
        config["notify.retryBaseSeconds"].default,
        serde_json::json!(30),
        "`notify.retryBaseSeconds` 默认必须是 30"
    );
    assert_eq!(
        config["notify.retryMultiplier"].default,
        serde_json::json!(2),
        "`notify.retryMultiplier` 默认必须是 2"
    );
    assert_eq!(
        config["notify.retryMaxSeconds"].default,
        serde_json::json!(3600),
        "`notify.retryMaxSeconds` 默认必须是 3600"
    );
    assert_eq!(
        config["notify.deliveringTimeoutSeconds"].default,
        serde_json::json!(300),
        "`notify.deliveringTimeoutSeconds` 默认必须是 300（D313）"
    );

    // 键名逐段 lowerCamelCase（契约 C9：带 `_` 会让服务起不来）。
    for key in config.keys() {
        for segment in key.split('.') {
            let first = segment.chars().next().expect("段非空");
            assert!(
                first.is_ascii_lowercase(),
                "配置键 `{key}` 的段 `{segment}` 必须以小写字母开头（契约 C9）"
            );
            assert!(
                segment.chars().all(|c| c.is_ascii_alphanumeric()),
                "配置键 `{key}` 的段 `{segment}` 只能含 ASCII 字母数字 —— 带 `_` 会让服务起不来"
            );
        }
    }
}

/// ⚠️ **D318 / S-11 的判据**：`platform-server` 的 `all_codes()` **必须**引用
/// `platform_notify::error::ALL_CODES`。
///
/// # 为什么这条必须**读源码**而不能只看测试绿
///
/// `all_codes()`（`crates/platform-server/tests/i18n_titles.rs:59`）是**逐 crate
/// 手工列举**的。漏加本 crate 的 `ALL_CODES` → 本 crate 的 8 个码**不在清单里**
/// → `titles_cover_every_code` **检查不到它们**（表里缺了也不报）
/// → **恰好就是绿的**。
///
/// → 判据必须是「那行代码**存在**」，不是「那个测试**是绿的**」。
#[test]
fn the_server_test_registers_the_notify_error_codes() {
    let root = repo_root();
    let path = root.join("crates/platform-server/tests/i18n_titles.rs");
    let source = std::fs::read_to_string(&path).expect("i18n_titles.rs 应可读");

    // 阳性对照：证明这个 grep 模式本身有效（已经存在的 app 那一行找得到）。
    assert!(
        source.contains("platform_app::error::ALL_CODES"),
        "阳性对照失败：连既有的 `platform_app::error::ALL_CODES` 都没找到 —— \
         说明扫的不是真文件或模式写错了"
    );

    assert!(
        source.contains("platform_notify::error::ALL_CODES"),
        "`crates/platform-server/tests/i18n_titles.rs` 的 `all_codes()` 必须加 \
         `platform_notify::error::ALL_CODES` —— 漏加时 `titles_cover_every_code` \
         **恰好是绿的**（D318 / S-11）"
    );
}

/// ⚠️ **钉死行数**必须计入 `REQ_015_NEW_CODES = 8`。
///
/// `the_title_table_has_the_pinned_number_of_rows`（同文件 `:174`）硬编码了
/// 各需求的增量常量。不加这一行 → `TITLES.len()` 实际 107 而断言仍是 99 → **红**。
#[test]
fn the_pinned_row_count_includes_the_notify_codes() {
    let root = repo_root();
    let source =
        std::fs::read_to_string(root.join("crates/platform-server/tests/i18n_titles.rs"))
            .expect("i18n_titles.rs 应可读");

    // 阳性对照：既有的 REQ_016 那行必须在。
    assert!(
        source.contains("REQ_016_NEW_CODES"),
        "阳性对照失败：既有的 `REQ_016_NEW_CODES` 都没找到"
    );

    assert!(
        source.contains("REQ_015_NEW_CODES"),
        "钉死行数的断言必须加 `REQ_015_NEW_CODES`（否则 TITLES 实际 107 而断言仍是 99 → 红）"
    );
    assert!(
        source.contains("const REQ_015_NEW_CODES: usize = 8;"),
        "`REQ_015_NEW_CODES` 必须**恰好是 8**（AC-S-174 步骤 1c：99 + 8 = 107）"
    );
}
