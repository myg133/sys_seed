//! 前端静态资源托管。
//!
//! 设计摘要 §2.1 决定「前端静态文件由 Rust 侧托管」：少一个进程、少一份内存，
//! 私有化部署（企业内网单机）尤其受益。

use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::Response;

/// 根路径默认回落到 `index.html`（单页应用路由）。
const INDEX_FILE: &str = "index.html";

/// 按 URI 解析出要返回的文件内容。
///
/// - 路径逃逸（`..`）一律拒绝
/// - 文件不存在且路径不带扩展名时，回落到 `index.html`（前端路由）
/// - 文件不存在且路径带扩展名时，返回 404
pub async fn serve(root: &Path, uri: &Uri) -> Response {
    let Some(relative) = safe_relative_path(uri.path()) else {
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
        assert_eq!(
            safe_relative_path("/"),
            Some(PathBuf::from("index.html"))
        );
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
}
