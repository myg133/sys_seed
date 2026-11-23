//! **装配判据**：`main.rs` 的每个挂载点都必须存在（**TD-076**）。
//!
//! # ⚠️ 这个文件防的是什么（QA 复查的变异实证）
//!
//! `main.rs` 有 18 处 `.nest()` / `.merge()` 把各子系统的 router 挂到服务上。
//! 这些挂载**内联在 500+ 行的装配段里**，测试够不着。
//!
//! QA 做了变异实验：**把 `/api/notify` 整条 `.nest()` 删掉**，
//! `platform-server` 的 **136 个测试全部通过** —— 也就是说：
//!
//! > **当前装配是对的，但没有任何测试能发现它变错。**
//!
//! 后果：某次重构误删一个 nest → 整组路由**静默 404** →
//! 所有单元测试、集成测试、真库测试**全绿** → 上线才发现。
//!
//! # 怎么判（**为什么用源码扫描而不是 `oneshot`**）
//!
//! 理想做法是「对每个预期路径打一次请求，断言不是 404」——
//! 但那需要**完整的 `AppState`**（PG + IAM + 全部子系统 state + 迁移），
//! 在集成测试里构造它等于把 `main` 重写一遍（且会因外部依赖脆弱）。
//!
//! → 采用**源码契约断言**：断言 `main.rs` 里存在每个预期的挂载调用。
//! 这抓住了「整条 nest 被删」这个**实际发生过的风险形态**（QA 的变异正是这种）。
//!
//! # ⚠️ 本判据的**能力边界**（如实记录）
//!
//! - ✅ 抓「某个 nest / merge 被删」
//! - ✅ 抓「挂载路径被改成拼写错误」（路径是断言的一部分）
//! - ❌ **不抓**「router 内部少了某个 route」（那是各 crate 自己的测试职责）
//! - ❌ **不抓**「state 构造错了」（那需要真实依赖）
//!
//! 更完整的做法（构造真实 AppState + 逐路径探活）留给**TD-076 的后续**，
//! 但那需要先把 `build_router` 从 `main` 抽出来 —— 属独立的架构改动。

use std::path::Path;

/// `main.rs` 必须包含的**每一个挂载点**。
///
/// ⚠️ 新增子系统时必须同步加进来 —— 否则新子系统没有装配判据。
/// 这份清单本身就是「平台有哪些面」的声明。
const REQUIRED_MOUNTS: &[(&str, &str)] = &[
    // (说明, 源码里必须出现的片段)
    // ⚠️ needle 用**单行短片段**（不含换行）—— 若写成多行，
    // rustfmt 一重排就会假红（QA 复查时踩过）。
    ("IAM 路由", r#".nest("/api/iam""#),
    ("IAM JWKS（根路径）", "platform_iam::http::jwks_router("),
    ("IAM SSO 路由", "platform_iam::http::sso_router("),
    ("组织架构", r#".nest("/api/org""#),
    ("RBAC", r#".nest("/api/rbac""#),
    ("审计", r#".nest("/api/audit""#),
    ("元数据 schema", r#".nest("/api/schema""#),
    ("沙箱模块", r#".nest("/api/sandbox""#),
    ("配置（merge）", "platform_config::http::router("),
    ("应用注册表", r#".nest("/api/apps""#),
    ("通知", r#".nest("/api/notify""#),
    ("OAuth 元数据", "platform_oauth::http::metadata_router("),
    ("国际化（merge）", "platform_server::i18n::router("),
    ("可观测性", r#".nest("/api/observability""#),
];

/// 定位仓库根（`crates/platform-server` → 上两级）。
fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/platform-server 应当有两级父目录")
        .to_path_buf()
}

/// **每个挂载点都必须出现在 `main.rs` 里**。
///
/// 变异验证（QA 做的）：删掉 `/api/notify` 的 `.nest()` → 这条会红。
#[test]
fn every_subsystem_is_mounted_in_main() {
    let main_rs = repo_root().join("crates/platform-server/src/main.rs");
    let source = std::fs::read_to_string(&main_rs)
        .unwrap_or_else(|err| panic!("读不到 {}：{err}", main_rs.display()));

    let mut missing = Vec::new();
    for (label, needle) in REQUIRED_MOUNTS {
        if !source.contains(needle) {
            missing.push(format!("  - {label}：期望源码含 `{needle}`"));
        }
    }
    assert!(
        missing.is_empty(),
        "`main.rs` 缺少 {} 个挂载点（整组路由会**静默 404**，而所有测试可能仍全绿）：\n{}",
        missing.len(),
        missing.join("\n")
    );
}

/// **阴性对照**：上面的断言确实能失败（防止 needle 写成空串导致恒真）。
///
/// ⚠️ 没有这条，「每个挂载点都存在」可能因为 `needle` 为空而**永远为真**。
/// 这是本仓记过的教训：判据本身也要有阳性/阴性对照。
#[test]
fn the_mount_check_is_not_vacuous() {
    let main_rs = repo_root().join("crates/platform-server/src/main.rs");
    let source = std::fs::read_to_string(&main_rs).expect("读 main.rs");

    // 一个**肯定不存在**的挂载 → 必须被判定为缺失。
    let definitely_absent = r#".nest("/api/this-does-not-exist", some_router::router(""#;
    assert!(
        !source.contains(definitely_absent),
        "阴性对照失败：`main.rs` 里居然存在一个不存在的挂载？"
    );

    // 反向确认：清单里每条 needle 都**非空**（空串会让 contains 恒真）。
    for (label, needle) in REQUIRED_MOUNTS {
        assert!(!needle.trim().is_empty(), "`{label}` 的 needle 不得为空");
    }
}
