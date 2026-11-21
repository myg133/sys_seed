//! Webhook 通道的真实现（REQ-015，D302）。
//!
//! # 与 `platform-oauth` 的 `OutboundClient` 的关系
//!
//! 两者**同款技术栈**（`hyper` + `hyper-util` + `hyper-rustls`），但**语义不同**：
//! 登出出站只 POST 一个 form、只关心状态码；通知 Webhook 要 POST **JSON 载荷**、
//! 要按收件人（每个收件人一个 URL）分别投递、还要把错误原因带回编排层。
//! 因此**自己写一份薄的**，不去耦合登出路径（D302）。
//!
//! # 收件人语义
//!
//! `NotificationMessage::recipients` 的每一条都是**一个独立的 URL** ——
//! Webhook 没有「账号」概念，投递对象就是接收端地址。
//!
//! # ⚠️ 构造失败**不得** panic
//!
//! 与 `platform-oauth` 的 `OutboundClient` 同款：TLS connector 构造失败时
//! `client` 为 `None`，`send()` 一律返回「投递失败」，**不让通知拖垮装配**。
//!
//! # ⚠️ URL 必须是**绝对的** http(s)
//!
//! 相对地址（`/hook`）在发请求**之前**就被拒 —— 出站地址只能来自租户配置，
//! 在这里再校一次是纵深防御。

use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use platform_core::RequestContext;
use platform_core::extension::notification::{DeliveryResult, NotificationMessage};

use crate::error::ServiceError;

/// Webhook 单次投递的超时（**5 秒**）。
///
/// 通知是**异步**投递，但它占着 worker 的一次 tick —— 不能被一个挂住的
/// 接收端拖住队列。
pub const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(5);

type HttpsConnector = hyper_rustls::HttpsConnector<HttpConnector>;
type HttpsClient = hyper_util::client::legacy::Client<HttpsConnector, Full<Bytes>>;

/// Webhook 投递端。
///
/// `Clone` 是廉价的（`hyper` 的 client 本身就在 `Arc` 里共享）。
#[derive(Clone)]
pub struct WebhookSender {
    /// 构造失败时为 `None` → 投递一律判「客户端不可用」。
    client: Option<HttpsClient>,
    /// 单次投递超时。
    timeout: Duration,
}

impl std::fmt::Debug for WebhookSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebhookSender")
            .field("client_available", &self.client.is_some())
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl WebhookSender {
    /// 构造（信任公网根）。
    ///
    /// # 失败语义
    ///
    /// TLS connector 构造失败 → `Err(ServiceError::Internal)`。
    /// **装配层不应因此起不来** —— 用 [`Self::default`] 拿到一个
    /// 「永远投递失败」的实例即可（与 `OutboundClient` 的 `Default` 同款）。
    pub fn new() -> Result<Self, ServiceError> {
        // 显式钉 ring provider：即使别的依赖也编进了 aws-lc-rs，
        // `ClientConfig::builder()` 也不会因「没有安装进程级 provider」而 panic。
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_provider_and_webpki_roots(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .map_err(|_| ServiceError::Internal {
                reason: "the webhook tls connector could not be built".to_string(),
            })?
            .https_or_http()
            .enable_http1()
            .build();
        Ok(Self {
            client: Some(
                hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(connector),
            ),
            timeout: WEBHOOK_TIMEOUT,
        })
    }

    /// 一个**没有** client 的实例（只用于验证「构造降级」这条路径）。
    #[cfg(test)]
    fn without_client() -> Self {
        Self {
            client: None,
            timeout: WEBHOOK_TIMEOUT,
        }
    }

    /// 换一个更短的超时（测试用，免得真等 5 秒）。
    #[cfg(test)]
    fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// POST 一次，返回远端状态码。
    ///
    /// 失败**不**返回 `Err` —— 通知通道的失败就是
    /// `DeliveryResult { delivered: false }`，由编排层决定重试。
    async fn post_one(&self, url: &str, body: &Bytes) -> Result<u16, String> {
        if !is_absolute_http_url(url) {
            return Err("the webhook recipient is not an absolute http or https url".to_string());
        }
        let Some(client) = self.client.as_ref() else {
            return Err("the webhook http client is unavailable".to_string());
        };

        let request = hyper::Request::builder()
            .method(http::Method::POST)
            .uri(url)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Full::new(body.clone()))
            .map_err(|_| {
                "the webhook recipient is not an absolute http or https url".to_string()
            })?;

        match tokio::time::timeout(self.timeout, client.request(request)).await {
            Ok(Ok(response)) => Ok(response.status().as_u16()),
            // 超时与连接错误对上层都是「接收端不可达」。
            // ⚠️ 不把 hyper 的错误原文带回 —— 它可能含接收端的内部地址。
            Ok(Err(_)) => Err("the webhook receiver is unreachable".to_string()),
            Err(_) => Err("the webhook receiver timed out".to_string()),
        }
    }
}

impl Default for WebhookSender {
    /// 构造失败时退回到一个**永远失败**的实例 —— 通知通道不可用不得让装配失败。
    fn default() -> Self {
        match Self::new() {
            Ok(sender) => sender,
            Err(_) => Self {
                client: None,
                timeout: WEBHOOK_TIMEOUT,
            },
        }
    }
}

impl platform_core::extension::notification::NotificationSender for WebhookSender {
    fn channel(&self) -> &str {
        crate::model::channel::WEBHOOK
    }

    fn send(
        &self,
        _ctx: &RequestContext,
        msg: &NotificationMessage,
    ) -> impl std::future::Future<Output = anyhow::Result<DeliveryResult>> + Send {
        // 先在同步段算出载荷：`{"title": ..., "body": ..., "payload": {...}}`。
        let payload = serde_json::to_vec(&serde_json::json!({
            "title": msg.title,
            "body": msg.body,
            "payload": msg.payload,
        }))
        .ok()
        .map(Bytes::from);
        let recipients = msg.recipients.clone();

        async move {
            if recipients.is_empty() {
                return Ok(DeliveryResult::failed("the notification has no recipients"));
            }
            let Some(body) = payload else {
                return Ok(DeliveryResult::failed(
                    "the webhook payload could not be serialized",
                ));
            };

            // 逐条投递：任何一个接收端失败，整条通知就判失败（不静默吞掉）。
            let mut statuses = Vec::with_capacity(recipients.len());
            for recipient in &recipients {
                match self.post_one(recipient, &body).await {
                    Ok(status) if (200..300).contains(&status) => statuses.push(status),
                    Ok(status) => {
                        return Ok(DeliveryResult::failed(format!(
                            "the webhook receiver returned a non-success status: {status}"
                        )));
                    }
                    Err(reason) => return Ok(DeliveryResult::failed(reason)),
                }
            }

            // 成功时把远端状态码拼成消息 ID
            // （接收端不保证回 ID，状态码是唯一可核对的证据）。
            let id = statuses
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(",");
            Ok(DeliveryResult::delivered(format!("webhook:{id}")))
        }
    }
}

/// 是否**绝对**的 http(s) URL（且带 host）。
///
/// 相对地址一律拒绝：出站 URL 只能来自租户配置，这里再校一次是纵深防御。
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;
    use platform_core::RequestContext;
    use platform_core::extension::notification::{NotificationMessage, NotificationSender};
    use tokio::net::TcpListener;

    use super::{WEBHOOK_TIMEOUT, WebhookSender};

    /// 假接收端的状态。
    #[derive(Clone)]
    struct ReceiverState {
        /// 要回的状态码。
        status: u16,
        /// 收到的请求体（按到达顺序）。
        bodies: Arc<Mutex<Vec<String>>>,
    }

    /// 假接收端：把请求体记下来，回固定状态码。
    async fn handle_hook(
        State(state): State<ReceiverState>,
        body: String,
    ) -> (StatusCode, &'static str) {
        state
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(body);
        let status = StatusCode::from_u16(state.status).unwrap_or(StatusCode::OK);
        (status, "ok")
    }

    /// 起一个假接收端（随机端口），返回投递地址与「收到的请求体」。
    async fn start_receiver(status: u16) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("应当能绑定随机端口");
        let addr = listener.local_addr().expect("应当能取本地地址");
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let state = ReceiverState {
            status,
            bodies: bodies.clone(),
        };
        let app = axum::Router::new()
            .route("/hook", post(handle_hook))
            .with_state(state);
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://127.0.0.1:{}/hook", addr.port()), bodies)
    }

    /// 读回记录下来的请求体。
    fn bodies_of(bodies: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        bodies.lock().unwrap_or_else(|err| err.into_inner()).clone()
    }

    /// 假接收端回 200 → `delivered == true`，且体是约定的 JSON 形状。
    #[tokio::test]
    async fn a_2xx_response_is_a_successful_delivery() {
        let (url, bodies) = start_receiver(200).await;
        let sender = WebhookSender::new().expect("client 应当能构造");
        let ctx = RequestContext::new();
        let msg = NotificationMessage::new("标题", "正文")
            .with_recipient(url)
            .with_payload("orderId", "A-1");

        let result = sender.send(&ctx, &msg).await.expect("webhook 不该返回 Err");
        assert!(result.delivered, "2xx 必须判投递成功：{result:?}");
        assert!(
            result.provider_message_id.is_some(),
            "成功投递必须带消息 ID"
        );

        let recorded = bodies_of(&bodies);
        assert_eq!(recorded.len(), 1, "应当恰好收到一次 POST");
        let parsed: serde_json::Value =
            serde_json::from_str(&recorded[0]).expect("请求体必须是 JSON");
        assert_eq!(parsed["title"], "标题");
        assert_eq!(parsed["body"], "正文");
        assert_eq!(parsed["payload"]["orderId"], "A-1");
    }

    /// 假接收端回 500 → `delivered == false`，`error` 里含 `500`。
    #[tokio::test]
    async fn a_non_2xx_response_is_a_failure_naming_the_status() {
        let (url, _bodies) = start_receiver(500).await;
        let sender = WebhookSender::new().expect("client 应当能构造");
        let ctx = RequestContext::new();
        let msg = NotificationMessage::new("标题", "正文").with_recipient(url);

        let result = sender.send(&ctx, &msg).await.expect("webhook 不该返回 Err");
        assert!(!result.delivered, "非 2xx 必须判投递失败");
        let error = result.error.expect("失败必须带原因");
        assert!(error.contains("500"), "错误里要点名状态码：{error}");
    }

    /// 相对 URL → `delivered == false`，且**没有**发出任何请求。
    #[tokio::test]
    async fn a_relative_url_is_rejected_without_any_request() {
        let (_url, bodies) = start_receiver(200).await;
        let sender = WebhookSender::new().expect("client 应当能构造");
        let ctx = RequestContext::new();
        let msg = NotificationMessage::new("标题", "正文").with_recipient("/hook");

        let result = sender.send(&ctx, &msg).await.expect("webhook 不该返回 Err");
        assert!(!result.delivered, "相对地址必须判投递失败");
        assert!(result.error.is_some(), "失败必须带原因：{result:?}");
        // 借一个真实接收端核对「真的没发」—— 相对地址不该落到任何地方。
        assert!(bodies_of(&bodies).is_empty(), "相对地址不得发出请求");
    }

    /// 收件人为空 → `delivered == false`（合法失败，不是 `Err`）。
    #[tokio::test]
    async fn an_empty_recipient_list_is_a_legal_failure() {
        let sender = WebhookSender::new().expect("client 应当能构造");
        let ctx = RequestContext::new();
        let msg = NotificationMessage::new("标题", "正文");

        let result = sender.send(&ctx, &msg).await.expect("webhook 不该返回 Err");
        assert!(!result.delivered);
        assert!(result.error.is_some());
        assert!(result.provider_message_id.is_none());
    }

    /// `client` 为 `None`（TLS 构造降级）→ 投递失败，**不 panic**。
    #[tokio::test]
    async fn a_degraded_sender_fails_instead_of_panicking() {
        let sender = WebhookSender::without_client();
        let ctx = RequestContext::new();
        let msg = NotificationMessage::new("标题", "正文").with_recipient("http://127.0.0.1:1/x");

        let result = sender.send(&ctx, &msg).await.expect("webhook 不该返回 Err");
        assert!(!result.delivered, "没有 client 时必须判失败");
        assert!(result.error.is_some(), "失败必须带原因：{result:?}");
    }

    /// 接收端只接受连接、永不响应 → 超时判失败（用短超时，不真等 5 秒）。
    #[tokio::test]
    async fn a_hanging_receiver_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("应当能绑定随机端口");
        let addr = listener.local_addr().expect("应当能取本地地址");
        // 只接不答：把连接留存住，永不写响应。
        tokio::spawn(async move {
            let mut accepted = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                accepted.push(stream);
            }
        });

        let sender = WebhookSender::new()
            .expect("client 应当能构造")
            .with_timeout(Duration::from_millis(100));
        let ctx = RequestContext::new();
        let msg =
            NotificationMessage::new("标题", "正文").with_recipient(format!("http://{addr}/hook"));

        let result = sender.send(&ctx, &msg).await.expect("webhook 不该返回 Err");
        assert!(!result.delivered, "超时必须判投递失败");
        assert!(result.error.is_some(), "失败必须带原因：{result:?}");
    }

    /// 超时常量就是 **5 秒**（D302 / design R6）。
    #[test]
    fn the_timeout_is_five_seconds() {
        assert_eq!(WEBHOOK_TIMEOUT, Duration::from_secs(5));
    }
}
