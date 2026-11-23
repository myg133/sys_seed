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

/// **按设计豁免的权限点**（必须能追溯到验收标准）。
///
/// # ⚠️ 为什么需要豁免清单
///
/// C-5 的规则是「登记的权限点必须有鉴权调用点」。但有些权限点是
/// **有意不鉴权**的 —— 若为了过 C-5 而强行加鉴权，会**违反已验收的需求**。
///
/// 本清单每一条都必须写明**书面依据**，否则不许加。
const EXEMPT: &[(&str, &str)] = &[
    (
        "rbac:permission:read",
        "REQ-009 AC 的「★ 豁免：权限点注册表查询」明文规定：\
         `GET /api/rbac/permissions` **不需要**管理权限点 —— \
         任何已认证用户都应能读，否则前端 `can()` 无从判断。\
         依据：`BA/demands/REQ-009-rbac/acceptance.md` 第 67~70 行。\
         该缺口已由 `BA/plan/tech-debt.md` 的 **TD-003** 记录（属「已知且已接受」）。\
         ⚠️ 注意：前端真正依赖的是 `/me/permissions`（只返回本人权限点），\
         那个端点天然不需要权限点；`/permissions`（注册表）的豁免是 REQ-009 的\n         显式决定，不由本 crate 单方面改变。",
    ),
];

/// C-5：本 crate 的每个权限点都必须有 `require_permission("字面量")` 调用点
/// （**除按设计豁免的以外**）。
#[test]
fn contract_c5_declared_permissions_have_authorization_call_sites() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = platform_contracts::check_c5_for(&manifest, &source)
        .expect("C-5 检查应当能运行（manifest 可读 + 源码可扫）");

    // 过滤掉按设计豁免的条目（只按**权限点编码**匹配，不看消息文本 —— 消息会变）。
    let leftovers: Vec<_> = violations
        .iter()
        .filter(|v| {
            !EXEMPT
                .iter()
                .any(|(code, _)| v.message.contains(&format!("`{code}`")))
        })
        .collect();

    assert!(
        leftovers.is_empty(),
        "C-5 失败（{} 条，已排除 {} 个按设计豁免的权限点）：\n{}\n\n\
         若这些是**新登记但没鉴权**的权限点 → 补 `require_permission`；\n\
         若属于**按设计豁免** → 在 `EXEMPT` 里登记并写明书面依据。",
        leftovers.len(),
        EXEMPT.len(),
        leftovers
            .iter()
            .map(|v| format!("  - {}", v.message))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// **豁免清单的自检**：每条都必须有**非空依据**，防止「随手加一行绕过 C-5」。
#[test]
fn every_exemption_has_a_written_justification() {
    for (code, reason) in EXEMPT {
        assert!(!code.trim().is_empty(), "豁免的权限点编码不得为空");
        assert!(
            reason.len() > 40,
            "`{code}` 的豁免依据太短（{} 字符）—— \
             必须写明**为什么按设计不鉴权**与**书面依据在哪**，\
             否则这个清单就成了绕过 C-5 的后门",
            reason.len()
        );
    }
}
