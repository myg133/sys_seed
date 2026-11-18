//! 极简 HTTP 探针：给镜像的 `HEALTHCHECK` 用。
//!
//! 不引入 HTTP 客户端依赖（reqwest 之类会拖进一大堆传递依赖，低配机与
//! 镜像体积都吃亏）—— 这里只需要「GET 一次，看状态码」。

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 探针错误。
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    /// URL 解析失败。
    #[error("URL 非法：{0}")]
    Url(String),
    /// 连接或读取失败。
    #[error("探针请求失败：{0}")]
    Io(#[from] std::io::Error),
    /// 响应不是合法的 HTTP 状态行。
    #[error("响应不是合法的 HTTP 状态行")]
    BadResponse,
}

/// 解析后的探针目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// 主机名或 IP。
    pub host: String,
    /// 端口。
    pub port: u16,
    /// 请求路径。
    pub path: String,
}

/// 解析 `http://host:port/path` 形式的地址。
pub fn parse_url(url: &str) -> Result<Target, ProbeError> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| ProbeError::Url(format!("{url}（只支持 http://）")))?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (
            host.to_string(),
            port.parse::<u16>()
                .map_err(|_| ProbeError::Url(format!("{url}（端口非法）")))?,
        ),
        None => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return Err(ProbeError::Url(format!("{url}（缺少主机名）")));
    }
    Ok(Target {
        host,
        port,
        path: path.to_string(),
    })
}

/// 发起一次 GET，返回 HTTP 状态码。
pub async fn get_status(url: &str) -> Result<u16, ProbeError> {
    let target = parse_url(url)?;
    let mut stream = TcpStream::connect((target.host.as_str(), target.port)).await?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nUser-Agent: base-healthcheck\r\nConnection: close\r\n\r\n",
        target.path, target.host, target.port
    );
    stream.write_all(request.as_bytes()).await?;

    // 只读头部附近的数据：健康响应很小，限制读取量避免异常响应撑爆内存
    let mut buffer = Vec::new();
    let read = stream.take(4096).read_to_end(&mut buffer).await?;
    if read == 0 {
        return Err(ProbeError::BadResponse);
    }
    parse_status_line(&buffer)
}

/// 从响应首行解析状态码（`HTTP/1.1 200 OK` → 200）。
fn parse_status_line(raw: &[u8]) -> Result<u16, ProbeError> {
    let text = String::from_utf8_lossy(raw);
    let first_line = text.lines().next().ok_or(ProbeError::BadResponse)?;
    let mut parts = first_line.split_whitespace();
    let version = parts.next().ok_or(ProbeError::BadResponse)?;
    if !version.starts_with("HTTP/") {
        return Err(ProbeError::BadResponse);
    }
    parts
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or(ProbeError::BadResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_with_port_and_path_is_parsed() {
        let target = parse_url("http://127.0.0.1:8080/health/ready").expect("应解析成功");
        assert_eq!(target.host, "127.0.0.1");
        assert_eq!(target.port, 8080);
        assert_eq!(target.path, "/health/ready");
    }

    #[test]
    fn url_without_path_defaults_to_root() {
        let target = parse_url("http://127.0.0.1:8080").expect("应解析成功");
        assert_eq!(target.path, "/");
    }

    #[test]
    fn url_without_port_defaults_to_80() {
        let target = parse_url("http://localhost/health/live").expect("应解析成功");
        assert_eq!(target.host, "localhost");
        assert_eq!(target.port, 80);
    }

    #[test]
    fn non_http_url_is_rejected() {
        assert!(parse_url("https://example.com").is_err());
        assert!(parse_url("127.0.0.1:8080").is_err());
    }

    #[test]
    fn status_line_is_parsed() {
        assert_eq!(parse_status_line(b"HTTP/1.1 200 OK\r\n\r\n").unwrap(), 200);
        assert_eq!(
            parse_status_line(b"HTTP/1.1 503 Service Unavailable\r\n").unwrap(),
            503
        );
        assert!(parse_status_line(b"garbage").is_err());
        assert!(parse_status_line(b"").is_err());
    }
}
