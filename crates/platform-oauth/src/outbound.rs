//! 出站 HTTP client（REQ-022，D220）。
//!
//! # 为什么**独立实现**（不动 REQ-013 的 OIDC client）
//!
//! `platform-iam` 的 `OidcHttpClient` 带 **JWKS 缓存 + discovery**，
//! 是**入站**专用（拉 IdP 的 metadata / JWKS）；
//! 出站登出需要的是「POST 一个 form 到任意 URL」→ 语义不同，复用反而别扭。
//!
//! 代价：出站 HTTP 逻辑有两份 → 记 **技术债 TD-042**。
//!
//! # 超时：**5 秒**
//!
//! 登出是用户可感知的动作 → 出站投递**不能**把登出拖成长时间等待。
//! 超时后**只记日志**，**绝不影响**登出主流程（D211）。
//!
//! # 安全：**无 SSRF 入口**
//!
//! 本模块**不解析**、**不接受**任何来自请求的 URL ——
//! 调用方只能传**client 表里已登记**的 `backchannel_logout_uri` /
//! `frontchannel_logout_uri`（注册时校验 scheme）。
//! 本模块只负责「把 form POST 到这个 URL」。

use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Bytes as HyperBytes;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

/// 出站投递的单次超时（**5 秒**，D211 / design R6）。
pub const OUTBOUND_TIMEOUT: Duration = Duration::from_secs(5);

type HttpsConnector = hyper_rustls::HttpsConnector<HttpConnector>;
type HttpsClient = hyper_util::client::legacy::Client<HttpsConnector, Full<HyperBytes>>;

/// 出站投递失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundError {
    /// URL 非法（不是绝对的 http(s) URL）。
    InvalidUrl,
    /// 连不上 / 超时。
    Unreachable,
    /// 远端返回了非 2xx。
    Rejected,
}

impl OutboundError {
    /// 面向开发者的英文原因（**不含 URL** —— URL 里可能有租户信息）。
    pub fn reason(&self) -> &'static str {
        match self {
            Self::InvalidUrl => "the logout uri is not an absolute http or https url",
            Self::Unreachable => "the logout receiver is unreachable",
            Self::Rejected => "the logout receiver returned a non-success status",
        }
    }
}

/// 出站 HTTP client（`https://` 与 `http://` 都支持）。
#[derive(Clone)]
pub struct OutboundClient {
    /// 构造失败时为 `None` → 投递一律判「不可达」。
    ///
    /// ⚠️ **不得**让「TLS 构造失败」变成「服务起不来」——
    /// 登出主流程必须照常走完（D211：通知失败绝不能让登出失败）。
    client: Option<HttpsClient>,
}

impl std::fmt::Debug for OutboundClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OutboundClient")
    }
}

impl OutboundClient {
    /// 构造（信任公网根）。
    pub fn new() -> Result<Self, OutboundError> {
        // 显式钉 ring provider：即使别的依赖也编进了 aws-lc-rs，
        // `ClientConfig::builder()` 也不会因「没有安装进程级 provider」而 panic。
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_provider_and_webpki_roots(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .map_err(|_| OutboundError::Unreachable)?
            .https_or_http()
            .enable_http1()
            .build();
        Ok(Self {
            client: Some(
                hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(connector),
            ),
        })
    }

    /// POST 一个 **form 编码**的载荷，返回远端的状态码。
    ///
    /// # 失败语义
    ///
    /// - URL 非法 → [`OutboundError::InvalidUrl`]；
    /// - 连不上 / 超时（**5 秒**）→ [`OutboundError::Unreachable`]；
    /// - 远端返回非 2xx → [`OutboundError::Rejected`]。
    ///
    /// ⚠️ **调用方不得因失败而让登出失败**（D211）。
    pub async fn post_form(&self, url: &str, body: &str) -> Result<u16, OutboundError> {
        if !is_absolute_http_url(url) {
            return Err(OutboundError::InvalidUrl);
        }

        // 构造失败的 client → 一律「不可达」（**不得**让登出起不来）。
        let Some(client) = self.client.as_ref() else {
            return Err(OutboundError::Unreachable);
        };

        let request = hyper::Request::builder()
            .method(http::Method::POST)
            .uri(url)
            .header(
                http::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(Full::new(Bytes::from(body.to_string())))
            .map_err(|_| OutboundError::InvalidUrl)?;

        let response = match tokio::time::timeout(OUTBOUND_TIMEOUT, client.request(request)).await {
            Ok(Ok(response)) => response,
            // 超时 / 连接错误 —— 对上层都是「远端不可达」。
            Ok(Err(_)) | Err(_) => return Err(OutboundError::Unreachable),
        };
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(OutboundError::Rejected);
        }
        Ok(status)
    }
}

/// 是否**绝对**的 http(s) URL（且带 host）。
///
/// ⚠️ 相对地址一律拒绝：出站 URL **只能**取自 client 表里**已登记**的值，
/// 而登记时就会用这条规则校验 → 出站侧再校一次是**纵深防御**。
fn is_absolute_http_url(raw: &str) -> bool {
    let value = raw.trim();
    let rest = match value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
    {
        Some(rest) => rest,
        None => return false,
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    !host.is_empty()
}

impl Default for OutboundClient {
    /// 构造失败时退回到一个**永远失败**的 client —— 登出**不得**因此起不来。
    fn default() -> Self {
        match Self::new() {
            Ok(client) => client,
            Err(_) => Self { client: None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timeout_is_five_seconds() {
        assert_eq!(OUTBOUND_TIMEOUT, Duration::from_secs(5));
    }

    /// 非绝对的 / 非 http(s) 的 URL → `InvalidUrl`（**没有 SSRF 入口**）。
    #[tokio::test]
    async fn an_invalid_url_is_rejected_before_any_request() {
        let client = OutboundClient::new().expect("client 应当能构造");
        for url in [
            "",
            "not-a-url",
            "/logout",
            "file:///etc/passwd",
            "ftp://example.com/logout",
        ] {
            assert_eq!(
                client.post_form(url, "logout_token=x").await,
                Err(OutboundError::InvalidUrl),
                "URL 必须在发请求之前被拒：{url}"
            );
        }
    }
}
