//! 前端静态资源托管。
//!
//! 设计摘要 §2.1 决定「前端静态文件由 Rust 侧托管」：少一个进程、少一份内存，
//! 私有化部署（企业内网单机）尤其受益。

use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::Response;
use platform_core::RequestContext;
use platform_core::problem::{ProblemDetails, problem_response};

/// 根路径默认回落到 `index.html`（单页应用路由）。
const INDEX_FILE: &str = "index.html";

/// API 前缀（**不回落**成 SPA —— 见 [`is_api_path`]）。
const API_PREFIX: &str = "/api";

/// 该路径是否是 **API 路径**（`/api` 本身，或 `/api/**`）。
///
/// # 为什么 API 路径**不回落**成 SPA（D180）
///
/// 回落条件是「路径**没有扩展名**」→ `/api/audit/events` 没有扩展名 → 回落
/// `index.html` → 未实现的端点返回 **200 + `text/html`**。
///
/// 三个后果：
///
/// 1. 客户端按 JSON 解析会炸，且**错误码语义完全错**（200 而不是 404）；
/// 2. 契约 C6 要求错误响应是**机器可读的 ProblemDetails**（而不是 HTML）；
/// 3. 「就绪探测」只能靠「响应是不是 JSON」这种**间接**判据 —— 脆弱。
///
/// → API 前缀的未命中一律 **404**（带 `ProblemDetails`）。
/// ⚠️ **只改 API 前缀**的回落行为 —— 静态资源（`/assets/**`、`/`、`index.html`）
/// 的既有回落逻辑**一字不改**。
fn is_api_path(path: &str) -> bool {
    path == API_PREFIX || path.starts_with("/api/")
}

/// 按 URI 解析出要返回的文件内容。
///
/// - 路径逃逸（`..`）一律拒绝
/// - 文件不存在且路径不带扩展名时，回落到 `index.html`（前端路由）
/// - 文件不存在且路径带扩展名时，返回 404
/// - **文件不存在且路径在 `/api/` 前缀下时，返回 404**（**不回落**成 SPA，D180）
pub async fn serve(root: &Path, uri: &Uri) -> Response {
    let path = uri.path();
    let Some(relative) = safe_relative_path(path) else {
        return not_found("非法路径");
    };

    let candidate = root.join(&relative);
    match tokio::fs::metadata(&candidate).await {
        Ok(meta) if meta.is_file() => match tokio::fs::read(&candidate).await {
            Ok(bytes) => ok_response(bytes, content_type(&relative)),
            Err(err) => not_found(&format!("读取失败：{err}")),
        },
        Ok(_) => not_found("不是文件"),
        Err(_) => {
            // 🔑 API 前缀**不回落**成 SPA（D180）：未实现的端点必须 404。
            if is_api_path(path) {
                return api_not_found(path);
            }
            // 前端路由：路径无扩展名 → 回落 index.html
            if relative.extension().is_none() {
                let index = root.join(INDEX_FILE);
                match tokio::fs::read(&index).await {
                    Ok(bytes) => ok_response(bytes, content_type(Path::new(INDEX_FILE))),
                    Err(err) => not_found(&format!("入口文件不存在：{err}")),
                }
            } else {
                not_found("文件不存在")
            }
        }
    }
}

/// 把 URI 路径转成安全的相对路径；含路径逃逸时返回 `None`。
fn safe_relative_path(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim_start_matches('/');
    let mut relative = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if relative.as_os_str().is_empty() {
        relative.push(INDEX_FILE);
    }
    Some(relative)
}

/// 按扩展名猜 Content-Type。
fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("ico") => "image/x-icon",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("txt") => "text/plain; charset=utf-8",
        Some("map") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// 200 响应。
fn ok_response(bytes: Vec<u8>, content_type: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .expect("静态响应构造不应失败")
}

/// 404 响应。
fn not_found(reason: &str) -> Response {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(format!("404 Not Found：{reason}")))
        .expect("静态响应构造不应失败")
}

/// API 路径未命中时的 **404**（RFC 9457 错误体，契约 C6 的形状）。
///
/// ⚠️ 与静态资源的 404 **形态不同**：API 路径的未命中是**接口语义**上的
/// 「端点不存在」→ 必须是机器可读的错误体（契约 C6），
/// 而不是一段纯文本。
///
/// ⚠️ **Content-Type 跟随平台统一口径**（`application/json`）——
///    平台**所有**错误响应都走 `platform_core::problem::problem_response`
///    （同一个函数、同一张表）→ 这里**不另立一套**。
///    契约 C6 钉的是**错误体的形状**（`code` / `status` / `detail` / `instance` /
///    `trace_id`），不是 Content-Type。
fn api_not_found(path: &str) -> Response {
    let ctx = RequestContext::new();
    problem_response(&ProblemDetails::not_found(&ctx, path, "endpoint"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_paths_are_kept() {
        assert_eq!(
            safe_relative_path("/assets/app.js"),
            Some(PathBuf::from("assets/app.js"))
        );
    }

    #[test]
    fn root_path_falls_back_to_index() {
        assert_eq!(safe_relative_path("/"), Some(PathBuf::from("index.html")));
    }

    #[test]
    fn parent_traversal_is_rejected() {
        assert_eq!(safe_relative_path("/../etc/passwd"), None);
        assert_eq!(safe_relative_path("/assets/../../secret"), None);
    }

    #[test]
    fn content_types_cover_common_assets() {
        assert_eq!(
            content_type(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("app.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("style.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("logo.svg")), "image/svg+xml");
        assert_eq!(
            content_type(Path::new("unknown.bin")),
            "application/octet-stream"
        );
    }

    /// D180：API 前缀**不回落**成 SPA。
    #[test]
    fn api_paths_are_recognized() {
        assert!(is_api_path("/api"));
        assert!(is_api_path("/api/"));
        assert!(is_api_path("/api/audit/events"));
        assert!(is_api_path("/api/audit/events/123"));
        // 相似但**不是** API 前缀的路径 → 仍然按前端路由回落。
        assert!(!is_api_path("/apidocs"));
        assert!(!is_api_path("/admin/audit"));
        assert!(!is_api_path("/"));
        assert!(!is_api_path("/assets/app.js"));
    }

    /// D180：未实现的 API 端点 → **404**（不是 200 + HTML）。
    #[tokio::test]
    async fn an_unknown_api_path_returns_404_instead_of_the_spa() {
        let root = std::env::temp_dir().join("base-static-fallback-test");
        tokio::fs::create_dir_all(&root)
            .await
            .expect("应当能建临时目录");
        tokio::fs::write(root.join(INDEX_FILE), b"<html>spa</html>")
            .await
            .expect("应当能写入口文件");

        let uri: Uri = "/api/audit/events".parse().expect("URI 应当合法");
        let response = serve(&root, &uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // 错误体是**机器可读**的（契约 C6 的形状），不是一段纯文本。
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("应当能读出响应体");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("错误体应当是 JSON");
        assert_eq!(value["code"], "NOT_FOUND");
        assert_eq!(value["status"], 404);
        assert_eq!(value["instance"], "/api/audit/events");
        assert!(
            !value["trace_id"].as_str().unwrap_or_default().is_empty(),
            "错误体必须带 trace_id（契约 C6）"
        );

        // 前端路由**仍然**回落成 SPA（既有行为一字不改）。
        let spa: Uri = "/admin/audit".parse().expect("URI 应当合法");
        let spa_response = serve(&root, &spa).await;
        assert_eq!(spa_response.status(), StatusCode::OK);

        tokio::fs::remove_dir_all(&root).await.ok();
    }
}
