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

/// **每个写端点都必须挂权限层**（QA 复查 B1 的 HTTP 层判据）。
///
/// # ⚠️ 为什么需要这条（C-5 之外）
///
/// C-5 只验「**已登记的权限点**有没有调用点」。它抓不住「**端点忘了挂**」——
/// 比如某天有人新增一个 `/user/{id}/roles` 路由但忘了 `route_layer`，
/// 只要那个权限点在别处有调用点，C-5 就是绿的。
///
/// QA 实测过这个缺口的真实后果：`platform-org` 的 17 个 handler 全都只取
/// `Authenticated`，**零个** `require_permission` → 只读用户能 `201 Created` 建组织节点。
///
/// # 判据
///
/// 源码里的**每个 `.route(` 必须落在某个挂了 `route_layer(require_permission(...))`
/// 的分组内**。用「分组」而非「逐行配对」是因为 `route_layer` 作用于整个
/// `Router` 构造表达式。
///
/// ⚠️ 能力边界：这是**源码级**判据，不验证运行时行为。
/// 运行时的 403 语义由 `require_permission` 自身的单元测试覆盖
/// （它在 `platform-core`，与本 crate 无关）。
#[test]
fn every_route_group_has_a_permission_layer() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/http.rs");
    let source = std::fs::read_to_string(&src).expect("读 http.rs");

    // 按 `let <name> = Router::new()` 切组。
    let mut groups: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("let ") {
            if rest.contains("= Router::new()") {
                let name = rest.split_whitespace().next().unwrap_or("?").to_string();
                if let Some(done) = current.take() {
                    groups.push(done);
                }
                current = Some((name, String::new()));
                continue;
            }
        }
        if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(done) = current.take() {
        groups.push(done);
    }

    let mut unguarded = Vec::new();
    for (name, body) in &groups {
        // 只关心**挂了路由**的组（`Router::new()` 后紧跟 `.route(...)`）。
        if !body.contains(".route(") {
            continue;
        }
        // 聚合组（`.merge(...)` 多个已鉴权的子 router）不直接挂 route → 已被上面过滤。
        if !body.contains("route_layer(require_permission(") {
            // 计一下有几条 .route(，便于定位。
            let routes = body.matches(".route(").count();
            unguarded.push(format!("  - 组 `{name}`：{routes} 条 .route，但没有 route_layer(require_permission(...))"));
        }
    }

    assert!(
        unguarded.is_empty(),
        "以下路由组**没有权限层**（任何已认证用户都能调）：\n{}\n\n\
         ⚠️ 若某个组确实该豁免（如登录类端点），请在本测试里显式登记理由，\
         不要靠「忘了挂」蒙混过去。",
        unguarded.join("\n")
    );
}
