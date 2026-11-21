//! C-5（不依赖 `ContractFixture`）：`platform-app` 的 3 个权限点各有鉴权调用点。
//!
//! # 为什么必须在这里（REQ-016 D271 / D240 同款）
//!
//! 契约检查 C-5 的入口 `check_c5_for(manifest_path, source_dir)`
//! **只扫「该 manifest 所属 crate 自己的 `src/`」**
//! （`crates/platform-contracts/src/checks.rs` 的 `check_c5_for`）。
//! 本需求的 manifest 在 `platform-app`、HTTP 也**在同一个 crate** 里 →
//! 调用点与权限点编码**字面量**同处一地，C-5 扫得到。
//!
//! ⚠️ **真 C-5 命令**是 `cargo test -p platform-app --test contract_c5` ——
//! `cargo xtask capabilities --check` **不做** C-5（它只比对索引产物与磁盘）。
//!
//! # 为什么调用点写在 `src/authz.rs` 而不是 `src/http.rs`
//!
//! 调用形态是 `.route_layer(require_permission("app:registry:read"))`。
//! 3 个薄封装住在 `src/authz.rs`，**不**引入 `platform-app → platform-rbac`
//! （鉴权入口在 **core**）。
//!
//! ⚠️ **命名禁区（D216）**：`crates/platform-app/src/` 里**不得**出现
//! `check_permission` / `check_perm` / `has_permission` / `require_perm` /
//! `authorize` / `ensure_permission` / `verify_permission` / `permission_check`
//! 这 8 个标识符 —— 写了就是「自建鉴权入口」→ C-5 直接失败。
//!
//! ⚠️ **应用接入语境极易踩 `authorize`**（如 `authorize_app_access`）——
//! 本 crate 的令牌校验函数因此叫 `AppService::authenticate`。

use std::path::PathBuf;

use platform_contracts::authz_scan::scan_auth_calls;
use platform_contracts::check_c5_for;

/// 仓库根目录。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 本需求登记的 3 个权限点（**逐字**，D271）。
const EXPECTED_CODES: [&str; 3] = [
    "app:registry:read",
    "app:registry:write",
    "app:registry:delete",
];

#[test]
fn every_app_permission_has_an_authorization_call_site() {
    let root = repo_root();
    let manifest = root.join("crates/platform-app/manifest.json");
    let src = root.join("crates/platform-app/src");

    let violations = check_c5_for(&manifest, &src).expect("扫描器应能运行");
    assert!(
        violations.is_empty(),
        "C-5 违例：{violations:?}\n  提示：权限点必须在 `crates/platform-app/src/` 里用字面量调用 require_permission"
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

/// manifest 本身必须合法（`Manifest::parse_checked` 的字段规则）。
///
/// ⚠️ 顺带覆盖 **3 个权限点** + **`tenantScoped` 段** + **`config` 段**：
///
/// - `tenantScoped` 是 2 张租户表的 RLS 策略的**唯一来源**（D227）；
/// - `config` 段必须是 **`ConfigSpec` 对象**（D277），且三个键都 `scope: platform`（D276）。
#[test]
fn the_app_manifest_is_valid() {
    let root = repo_root();
    let raw = std::fs::read_to_string(root.join("crates/platform-app/manifest.json"))
        .expect("manifest 应可读");
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).expect("manifest 应合法");
    assert_eq!(manifest.id, "app");
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
            "platform_app".to_string(),
            "platform_app_access".to_string(),
        ],
        "2 张租户表必须声明在 tenantScoped 里"
    );
}

/// ⚠️ **D276 / S-01 的判据**：3 个配置键**都**是 `scope: Platform`。
///
/// `ConfigSpec.scope` 的 serde 默认值是 **`Tenant`**
/// （`crates/platform-core/src/extension/manifest.rs` 的 `default_scope`）→
/// 漏写 `"scope": "platform"` 就落到租户作用域 → 平台级读写端点直接拒
/// → AC-S-160 / AC-S-161 的「配置生效」**永不可达**，且**只有运行时静默过滤**。
///
/// ⚠️ **这才是可执行的判据**：AC-S-160 步骤 0 的字面 SQL
/// （`SELECT count(*) FROM platform_config WHERE key LIKE 'app.%' AND scope = 'platform'`）
/// **物理不可执行** —— `platform_config` 表**没有 `scope` 列**
/// （实测：`information_schema.columns` 只有 `key` / `value` / `updated_at`）。
/// 配置的作用域是**manifest 的元数据**，不在库里。已上报 BA 复核。
#[test]
fn every_app_config_key_is_platform_scoped() {
    use platform_core::config::ConfigScope;

    let root = repo_root();
    let raw = std::fs::read_to_string(root.join("crates/platform-app/manifest.json"))
        .expect("manifest 应可读");
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).expect("manifest 应合法");
    let config = manifest.config.expect("manifest 必须声明 config 段（D274）");

    assert_eq!(config.len(), 3, "恰好 3 个配置键");
    for key in [
        "app.recentCount",
        "app.workspace.maxApps",
        "app.icon.allowHttp",
    ] {
        let spec = config.get(key).unwrap_or_else(|| panic!("缺少配置键 {key}"));
        assert_eq!(
            spec.scope,
            ConfigScope::Platform,
            "配置键 `{key}` 的 scope 必须是 Platform（D276：漏写会落到 Tenant → 平台端点直接拒）"
        );
        assert!(
            spec.label.as_deref().is_some_and(|label| !label.trim().is_empty()),
            "配置键 `{key}` 必须有非空 label（配置界面要标题）"
        );
    }

    // 默认值与 D274 逐字一致。
    assert_eq!(
        config["app.recentCount"].default,
        serde_json::json!(6),
        "`app.recentCount` 默认必须是 6"
    );
    assert_eq!(
        config["app.workspace.maxApps"].default,
        serde_json::json!(60),
        "`app.workspace.maxApps` 默认必须是 60"
    );
    assert_eq!(
        config["app.icon.allowHttp"].default,
        serde_json::json!(false),
        "`app.icon.allowHttp` 默认必须是 false"
    );
}
