//! C-5 契约检查：**已登记的权限点必须有鉴权调用点**。
//!
//! # 为什么这个文件存在（QA 复查 F2 的修复）
//!
//! 契约宏 `#[platform_module_contract]` 生成 C-1~C-6，但它**只挂在
//! `modules/sandbox` 一处** → 四个核心 crate（iam/rbac/oauth/org）与其余平台 crate
//! **从未被 C-5 扫过**。
//!
//! 后果已实际发生：`platform-org` 登记了 9 个 `org:*` 权限点，
//! 而 `src/` 里**零**个 `require_permission` 调用点 ——
//! 任何已认证用户都能改组织架构（QA 实测 `201 Created`）。
//!
//! → 本文件让 C-5 覆盖到 **每一个有 manifest 的平台 crate**。
//!
//! # ⚠️ 这里为什么直接用 `check_c5_for` 而不是契约宏
//!
//! 契约宏需要 `ContractSubject`（模块 trait 实现 + 夹具），
//! 而平台 crate **不是业务模块**（没有 `PlatformModule` 实现）。
//! `check_c5_for(manifest, src)` 是 C-5 的**纯函数形态**，
//! 只依赖 manifest 与源码目录 —— 正好适用于平台 crate。

/// C-5：本 crate 的每个权限点都必须有 `require_permission("字面量")` 调用点。
#[test]
fn contract_c5_declared_permissions_have_authorization_call_sites() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = platform_contracts::check_c5_for(&manifest, &source)
        .expect("C-5 检查应当能运行（manifest 可读 + 源码可扫）");
    assert!(
        violations.is_empty(),
        "C-5 失败（{} 条）：\n{}",
        violations.len(),
        violations
            .iter()
            .map(|v| format!("  - {}", v.message))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
