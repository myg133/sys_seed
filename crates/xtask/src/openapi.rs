//! 生成 / 校验 OpenAPI 文档（REQ-019，AC-S-035 / D338）。
//!
//! # ⚠️ `--check` 必须**逐字节比对**
//!
//! 只比「文件是否存在」或「能否解析」的话，
//! **代码改了但产物没更新**的情况会绿 —— 那 `--check` 就白加了
//! （AC-S-035 步骤 5 专门验这个）。
//!
//! # ⚠️ 覆盖范围（如实记录）
//!
//! 产物由 `platform_observability::http::ApiDoc` 生成，
//! 它**只包含已标注 `#[utoipa::path]` 的端点**。其余 crate 需要各自加标注
//! —— 那是跨需求改动（改已验收 crate 的源码），本轮不做 → **TD-073**。
//!
//! 所以本命令产出的是**部分**文档，不声称覆盖全部端点。

use std::path::{Path, PathBuf};

/// 产物路径（相对仓库根）。
pub const OUTPUT_PATH: &str = "openapi.json";

/// 生成 OpenAPI JSON（格式化后的字符串）。
///
/// ⚠️ 用 `to_pretty_json` 而不是 `to_json`：
/// 产物要**可读、可 diff**（`--check` 报差异时人能看懂）。
pub fn render() -> Result<String, String> {
    use utoipa::OpenApi;

    let doc = platform_observability::http::ApiDoc::openapi();
    let json = doc
        .to_pretty_json()
        .map_err(|err| format!("序列化 OpenAPI 文档失败：{err}"))?;
    // 统一以换行结尾（避免「文件末尾有无换行」造成假差异）。
    Ok(format!("{}\n", json.trim_end()))
}

/// 跑 `openapi` 子命令。
///
/// # 返回
///
/// 进程退出码（0 = 成功 / 已一致；1 = 不一致）。
pub fn run(root: &Path, check: bool) -> Result<i32, String> {
    let path: PathBuf = root.join(OUTPUT_PATH);
    let rendered = render()?;

    if check {
        let existing = std::fs::read_to_string(&path).map_err(|err| {
            format!(
                "读不到产物 `{}`：{err}\n  → 先跑 `cargo xtask openapi` 生成它",
                path.display()
            )
        })?;
        if existing == rendered {
            println!("OpenAPI 产物与源码一致：{OUTPUT_PATH}");
            return Ok(0);
        }
        // ⚠️ 必须指出**哪里不一致** —— 只说「不一致」等于让人自己猜。
        report_difference(&existing, &rendered);
        return Ok(1);
    }

    std::fs::write(&path, &rendered)
        .map_err(|err| format!("写产物 `{}` 失败：{err}", path.display()))?;
    println!("已生成 {OUTPUT_PATH}（{} 字节）", rendered.len());
    Ok(0)
}

/// 打印前几处差异（**带行号**，便于定位）。
fn report_difference(existing: &str, rendered: &str) {
    eprintln!("OpenAPI 产物与源码**不一致**：");
    let old: Vec<&str> = existing.lines().collect();
    let new: Vec<&str> = rendered.lines().collect();
    let mut shown = 0usize;
    for index in 0..old.len().max(new.len()) {
        let a = old.get(index).copied().unwrap_or("<缺失>");
        let b = new.get(index).copied().unwrap_or("<缺失>");
        if a != b {
            eprintln!("  第 {} 行：", index + 1);
            eprintln!("    --- 产物: {a}");
            eprintln!("    +++ 源码: {b}");
            shown += 1;
            if shown >= 10 {
                eprintln!("  …（差异超过 10 处，只显示前 10 处）");
                break;
            }
        }
    }
    if shown == 0 {
        eprintln!("  （行级比对没找到差异，可能是行尾/编码差异）");
    }
    eprintln!("  修复：跑 `cargo xtask openapi` 重新生成。");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 渲染结果是一个**合法且带版本号**的 OpenAPI 文档。
    #[test]
    fn the_rendered_document_is_valid_openapi() {
        let json = render().expect("应能渲染");
        let value: serde_json::Value = serde_json::from_str(&json).expect("必须是合法 JSON");
        assert_eq!(
            value.get("openapi").and_then(|v| v.as_str()),
            Some("3.1.0"),
            "必须是 OpenAPI 3.1.0（不是 3.0.x）"
        );
        assert!(value.get("info").is_some(), "必须有 info");
        assert!(value.get("paths").is_some(), "必须有 paths");
    }

    /// `info.title` 与 `info.version` 非空。
    #[test]
    fn the_info_block_is_populated() {
        let json = render().expect("应能渲染");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let info = value.get("info").expect("有 info");
        assert!(
            info.get("title")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty()),
            "info.title 非空"
        );
        assert!(
            info.get("version")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty()),
            "info.version 非空"
        );
    }

    /// 本需求标注的端点出现在 `paths` 里。
    ///
    /// ⚠️ 只断言**已标注**的那个 —— 其余端点在 TD-073 里明码不做。
    #[test]
    fn the_annotated_endpoint_is_present() {
        let json = render().expect("应能渲染");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let paths = value.get("paths").expect("有 paths");
        assert!(
            paths
                .get("/api/observability/rate-limit/status")
                .is_some(),
            "已标注的端点必须在 paths 里；实际 paths：{paths}"
        );
    }

    /// 渲染是**确定性**的（两次结果相同）。
    ///
    /// ⚠️ 这是 `--check` 能用的前提：若渲染有随机顺序，
    /// 每次 `--check` 都会报「不一致」。
    #[test]
    fn rendering_is_deterministic() {
        let a = render().expect("渲染 1");
        let b = render().expect("渲染 2");
        assert_eq!(a, b, "两次渲染必须逐字节相同（否则 --check 恒红）");
    }

    /// 产物以换行结尾（避免「末尾有无换行」造成假差异）。
    #[test]
    fn the_rendered_output_ends_with_a_newline() {
        assert!(render().expect("渲染").ends_with('\n'));
    }
}
