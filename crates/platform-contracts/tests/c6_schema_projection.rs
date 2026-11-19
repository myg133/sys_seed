//! 契约 C-6 的**负向用例**（AC-S-081，TC-012-031 ~ 035）。
//!
//! 判据（design §4.4 已选定）：**若某模块的 manifest 声明了任何带 `permission` 的字段，
//! 则该模块源码里必须出现 `SchemaProjector::project` 的调用。**
//!
//! # 为什么必须有负向用例
//!
//! 只跑「合规状态通过」证明不了检查真的在看源码 —— 把扫描器写成 `Ok(())` 也能过。
//! 负向用例是唯一能证明「它抓到违规」的方式（AC 步骤 2/3/4 就是这三条）。
//!
//! # 夹具怎么造
//!
//! 把**真实的** `modules/sandbox/{manifest.json,backend/src}` 复制到临时目录再改坏：
//! 在仓库里改会污染其它测试（`the_permission_registry_matches_every_manifest` 之类的
//! 全局扫描），而且仓库里的文件必须保持合规。

use std::path::{Path, PathBuf};

use platform_contracts::check_c6_for;

/// 仓库根（`crates/platform-contracts` 往上两级）。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/platform-contracts 应有两级父目录")
        .to_path_buf()
}

/// 临时目录（Drop 时清理）。
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "base-c6-{label}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录应可建");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 复制 sandbox 的 manifest 与 backend 源码到临时目录。
///
/// 返回 `(manifest 路径, 源码目录)`。
fn sandbox_copy(label: &str) -> (TempDir, PathBuf, PathBuf) {
    let root = repo_root();
    let temp = TempDir::new(label);

    let manifest_path = temp.path().join("manifest.json");
    std::fs::copy(root.join("modules/sandbox/manifest.json"), &manifest_path)
        .expect("manifest 应能复制");

    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("源码目录应可建");
    let backend = root.join("modules/sandbox/backend/src");
    for entry in std::fs::read_dir(&backend).expect("backend 目录应可读").flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            std::fs::copy(&path, src_dir.join(entry.file_name()))
                .expect("源码文件应能复制");
        }
    }

    (temp, manifest_path, src_dir)
}

/// 把源码目录里所有文件中的某段文本替换掉（可以替换成空）。
fn rewrite_all(dir: &Path, from: &str, to: &str) {
    for entry in std::fs::read_dir(dir).expect("目录应可读").flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("源码应可读");
        if source.contains(from) {
            std::fs::write(&path, source.replace(from, to)).expect("源码应可写");
        }
    }
}

/// TC-012-031 / 合规状态：真实副本 → 0 违例。
#[test]
fn a_compliant_module_passes() {
    let (_temp, manifest, src) = sandbox_copy("compliant");
    let violations = check_c6_for(&manifest, &src).expect("检查应能跑");
    assert!(violations.is_empty(), "violations = {violations:?}");
}

/// TC-012-032 造违规 1：声明了字段权限，但源码里没有投影调用 → 失败并点名模块/实体/字段。
#[test]
fn a_module_without_a_project_call_site_fails_and_names_module_entity_field() {
    let (_temp, manifest, src) = sandbox_copy("missing-call");
    // 把真实调用换成不存在的函数名（保留其余代码形状）。
    rewrite_all(&src, "SchemaProjector::project", "NoSuchProjector::nowhere");

    let violations = check_c6_for(&manifest, &src).expect("检查应能跑");
    assert_eq!(violations.len(), 1, "violations = {violations:?}");
    assert_eq!(violations[0].check_id, "C-6");
    let message = &violations[0].message;
    for expected in ["sandbox", "demoItem", "secret", "no project call site"] {
        assert!(
            message.contains(expected),
            "消息必须点名 `{expected}`：{message}"
        );
    }
}

/// TC-012-033 判据 = 「至少一处」：只保留一处调用 → **通过**。
///
/// 这条用例的价值是把**局限**固化成一条会红的测试：将来若有人把判据改强
/// （要求每个 handler 都调用），这条会红，从而逼迫同步改 design §4.4。
#[test]
fn a_single_call_site_is_enough() {
    let (_temp, manifest, src) = sandbox_copy("single-call");
    // 把所有真实调用（含 `project_items`）都抹掉，只留**一处**：
    // 这正是「3 个 handler 只有 1 个调用」的形态 —— 判据是「至少一处」，所以它过。
    rewrite_all(&src, "SchemaProjector::project", "NoSuchProjector::nowhere");
    assert_eq!(
        check_c6_for(&manifest, &src).expect("检查应能跑").len(),
        1,
        "抹掉全部调用后必须失败（否则下面的「一处就够」证明不了任何事）"
    );

    std::fs::write(
        src.join("one_call.rs"),
        r#"
fn project_one(raw: serde_json::Value) -> serde_json::Value {
    SchemaProjector::project(raw, &schema, &ctx, checker)
}
"#,
    )
    .expect("文件应可写");

    let violations = check_c6_for(&manifest, &src).expect("检查应能跑");
    assert!(
        violations.is_empty(),
        "判据是「至少一处」：{violations:?}"
    );
}

/// TC-012-034：注释与字符串里的文本**不算**调用点。
#[test]
fn text_in_comments_and_strings_is_not_a_call_site() {
    let (_temp, manifest, src) = sandbox_copy("comment-only");
    rewrite_all(&src, "SchemaProjector::project", "NoSuchProjector::nowhere");
    // 只把文本写进注释与字符串字面量。
    std::fs::write(
        src.join("zz_doc.rs"),
        r#"
//! 用 `SchemaProjector::project` 剔除无权字段。
// SchemaProjector::project(raw, &schema, &ctx, checker)
fn documentation() -> &'static str {
    "SchemaProjector::project(raw)"
}
"#,
    )
    .expect("文档文件应可写");

    let violations = check_c6_for(&manifest, &src).expect("检查应能跑");
    assert_eq!(violations.len(), 1, "violations = {violations:?}");
    assert_eq!(violations[0].check_id, "C-6");
    assert!(violations[0].message.contains("secret"));
}

/// TC-012-035：未声明任何字段权限 → C-6 **不误报**（无声明即无约束）。
#[test]
fn a_module_without_field_permissions_is_not_reported() {
    let (_temp, manifest, src) = sandbox_copy("no-field-permission");
    rewrite_all(&src, "SchemaProjector::project", "NoSuchProjector::nowhere");

    let source = std::fs::read_to_string(&manifest).expect("manifest 应可读");
    // 用**另一个合法属性**替换掉 permission（换回 label 会造成重复键）。
    let without_permission = source.replace(
        r#""permission": "sandbox:demo:secret""#,
        r#""maxLength": 500"#,
    );
    std::fs::write(&manifest, without_permission).expect("manifest 应可写");

    let violations = check_c6_for(&manifest, &src).expect("检查应能跑");
    assert!(violations.is_empty(), "violations = {violations:?}");
}
