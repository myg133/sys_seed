//! SMTP 通道：**自己写的**最小 SMTP 客户端（REQ-015，D305 / D307）。
//!
//! # 为什么不用现成 SMTP 库
//!
//! 只需要一条路径（STARTTLS + `AUTH LOGIN` + 单条文本邮件），
//! 拉一个全功能库的传递依赖不划算（D305）。协议交互是本文件的核心，
//! 因此**每一步都显式写出来**并有对应测试。
//!
//! # 协议顺序（严格按序，任何一步的应答码不符即失败）
//!
//! ```text
//! ① TCP 连接                 220
//! ② EHLO <host>              250（多行：`250-` 续行，`250 ` 结尾）
//! ③ STARTTLS                 220 → rustls 握手
//! ④ EHLO <host>              250  ⚠️ TLS 之后必须重发（RFC 3207）
//! ⑤ AUTH LOGIN               334 → base64(user) → 334 → base64(pass) → 235
//! ⑥ MAIL FROM:<from>         250
//! ⑦ RCPT TO:<r>（逐个）       250  —— 任一被拒即整条失败
//! ⑧ DATA                     354 → 头 + 空行 + 正文 → "." 单行 → 250
//! ⑨ QUIT                     221
//! ```
//!
//! # 七条硬约束
//!
//! 1. **头注入防御**：`title` 进 `Subject` 前在第一个 `\r` / `\n` 处**截断** ——
//!    否则 `Subject: x\r\nBcc: attacker@evil` 会给攻击者加收件人。
//! 2. **base64 正文每 76 字符换行**（RFC 2045），用 `\r\n` 连接。
//! 3. **多行 250**：`250-` 是续行，`250 ` 是最后一行，必须读完整再判断。
//! 4. **`provider_message_id`**：从 `DATA` 之后的 `250` 应答里取第三段
//!    （`250 2.0.0 QUEUE-ID` → `QUEUE-ID`），取不到就 `None` 但仍 `delivered`。
//! 5. **超时**：整个投递包在 `tokio::time::timeout` 里，超时的错误文案含 `timeout`。
//! 6. **`ServerName` 来自 `config.host`**（不是 IP —— `SmtpConfig::from_lookup` 已拒绝 IP）。
//! 7. **错误文案带服务端应答码**：`AUTH` 回 `535` / `RCPT` 回 `550` 都必须在文案里出现。
//!
//! # 构造失败不得让服务起不来（D211 同款口径）
//!
//! 信任根的 `ClientConfig` 构造失败 → 存 `None`，投递时返回 `Err`，
//! **不 panic**：通知失败绝不能把调用方的主体流程带崩。

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use platform_core::RequestContext;
use platform_core::extension::notification::{
    DeliveryResult, NotificationMessage, NotificationSender,
};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::config::SmtpConfig;
use crate::model::channel::SMTP;

/// 默认整条投递的超时（D305）。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// 邮件通道的投递端。
pub struct SmtpSender {
    /// 连接与凭据。
    config: SmtpConfig,
    /// 整条投递的超时（测试可注入更短的值）。
    timeout: Duration,
    /// 客户端 TLS 配置；构造失败时为 `None` → 投递一律返回 `Err`（不 panic）。
    tls: Option<Arc<rustls::ClientConfig>>,
}

impl std::fmt::Debug for SmtpSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 复用 `SmtpConfig` 的 `Debug`（密码已打码）。
        f.debug_struct("SmtpSender")
            .field("config", &self.config)
            .field("timeout", &self.timeout)
            .field("tls", &self.tls.is_some())
            .finish()
    }
}

impl SmtpSender {
    /// 用生产配置构造（信任公网根，超时 10 秒）。
    pub fn new(config: SmtpConfig) -> Self {
        // 构造失败只记 `None`：让投递阶段给出可读错误，而不是 panic 掉装配层。
        let tls = match default_client_config() {
            Ok(config) => Some(config),
            Err(err) => {
                tracing::warn!(error = %err, "SMTP 客户端 TLS 配置构造失败，邮件通道将不可用");
                None
            }
        };
        Self::with_parts(config, DEFAULT_TIMEOUT, tls)
    }

    /// 显式指定超时与 TLS 配置（测试注入用；生产走 [`Self::new`]）。
    fn with_parts(
        config: SmtpConfig,
        timeout: Duration,
        tls: Option<Arc<rustls::ClientConfig>>,
    ) -> Self {
        Self {
            config,
            timeout,
            tls,
        }
    }
}

impl NotificationSender for SmtpSender {
    fn channel(&self) -> &str {
        SMTP
    }

    fn send(
        &self,
        _ctx: &RequestContext,
        msg: &NotificationMessage,
    ) -> impl std::future::Future<Output = Result<DeliveryResult>> + Send {
        // 先把要用的东西搬进 future：`send` 拿的是 `&self`，await 期间不能继续借它。
        let config = self.config.clone();
        let timeout = self.timeout;
        let tls = self.tls.clone();
        let message = msg.clone();

        async move {
            // ⑤ 超时：整条投递共用一个预算（不是每步一个）。
            match tokio::time::timeout(timeout, deliver(&config, tls, &message)).await {
                Ok(result) => result,
                Err(_) => Err(anyhow!(
                    "smtp timeout：{timeout:?} 内未完成与 {}:{} 的投递",
                    config.host,
                    config.port
                )),
            }
        }
    }
}

/// 建一个信任公网根、钉 ring provider 的客户端配置。
///
/// 显式钉 provider：即使别的依赖也编进了 aws-lc-rs，
/// `builder_with_provider` 也不会因「没有进程级 provider」而 panic（同 `platform-oauth/outbound.rs`）。
fn default_client_config() -> Result<Arc<rustls::ClientConfig>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .context("rustls 不认默认协议版本")?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(config))
}

/// 走完 ①~⑨ 的完整投递。
async fn deliver(
    config: &SmtpConfig,
    tls: Option<Arc<rustls::ClientConfig>>,
    msg: &NotificationMessage,
) -> Result<DeliveryResult> {
    let tls = tls.ok_or_else(|| anyhow!("smtp 客户端 TLS 配置不可用（构造失败）"))?;
    // ⑥ `ServerName` 必须是主机名：`SmtpConfig::from_lookup` 已拒绝 IP 字面量。
    let server_name = ServerName::try_from(config.host.clone())
        .map_err(|_| anyhow!("smtp 主机名不能作为 TLS ServerName：{}", config.host))?;

    // ① TCP 连接。
    let stream = TcpStream::connect((config.host.as_str(), config.port))
        .await
        .with_context(|| format!("连接 SMTP 服务器 {}:{} 失败", config.host, config.port))?;
    let mut conn = Connection::new(Wire::Plain(stream));
    conn.expect(220, "连接").await?;

    // ② EHLO（STARTTLS 之前；应答可能是多行）。
    conn.ehlo(&config.host).await?;

    // ③ STARTTLS → TLS 握手。
    conn.write_line("STARTTLS").await?;
    conn.expect(220, "STARTTLS").await?;
    let mut conn = conn
        .upgrade_to_tls(&TlsConnector::from(tls), server_name)
        .await?;

    // ④ ⚠️ TLS 之后必须重发 EHLO（RFC 3207：TLS 前的扩展列表不算数）。
    conn.ehlo(&config.host).await?;

    // ⑤ AUTH LOGIN：用户名 / 密码各自 base64，密码永不出现在错误文案里。
    conn.write_line("AUTH LOGIN").await?;
    conn.expect(334, "AUTH LOGIN").await?;
    conn.write_line(&BASE64.encode(config.username.as_bytes()))
        .await?;
    conn.expect(334, "AUTH 用户名").await?;
    conn.write_line(&BASE64.encode(config.password.as_bytes()))
        .await?;
    conn.expect(235, "AUTH 密码").await?;

    // ⑥ MAIL FROM。
    let from = sanitize_inline(&config.from);
    conn.write_line(&format!("MAIL FROM:<{from}>")).await?;
    conn.expect(250, "MAIL FROM").await?;

    // ⑦ RCPT TO：任一被拒 → 整条失败（否则会给部分收件人重复投递）。
    for recipient in &msg.recipients {
        let recipient = sanitize_inline(recipient);
        conn.write_line(&format!("RCPT TO:<{recipient}>")).await?;
        conn.expect(250, "RCPT TO").await?;
    }

    // ⑧ DATA。
    conn.write_line("DATA").await?;
    conn.expect(354, "DATA").await?;
    conn.write_raw(&render_message(config, msg)).await?;
    // 正文以单独一行 "." 结束。
    conn.write_line(".").await?;
    let ack = conn.expect(250, "DATA 正文").await?;

    // ④ `provider_message_id`：`250 2.0.0 QUEUE-ID` 的第三段。
    let provider_message_id = ack.text.split_whitespace().nth(2).map(str::to_owned);

    // ⑨ QUIT：消息已在 DATA 的 250 被服务端接收，QUIT 出问题**不**改判失败 ——
    // 否则一次网络抖动会让已投递的邮件被重试成重复邮件。
    if conn.write_line("QUIT").await.is_ok() {
        let _ = conn.expect(221, "QUIT").await;
    }

    Ok(DeliveryResult {
        delivered: true,
        provider_message_id,
        error: None,
    })
}

/// 渲染整封邮件（头 + 空行 + base64 正文）。
fn render_message(config: &SmtpConfig, msg: &NotificationMessage) -> Vec<u8> {
    let mut headers = String::new();
    headers.push_str(&format!("From: <{}>\r\n", sanitize_inline(&config.from)));
    let to: Vec<String> = msg
        .recipients
        .iter()
        .map(|r| format!("<{}>", sanitize_inline(r)))
        .collect();
    headers.push_str(&format!("To: {}\r\n", to.join(", ")));
    // ⚠️ ① 头注入防御：`title` 在第一个 CR/LF 处截断，绝不可能落地成第二行头。
    headers.push_str(&format!("Subject: {}\r\n", sanitize_subject(&msg.title)));
    headers.push_str("MIME-Version: 1.0\r\n");
    headers.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    headers.push_str("Content-Transfer-Encoding: base64\r\n");
    headers.push_str("\r\n");

    let mut payload = headers.into_bytes();
    // ② base64 每 76 字符折行（RFC 2045）。
    let encoded = wrap_base64(&BASE64.encode(msg.body.as_bytes()));
    payload.extend_from_slice(encoded.as_bytes());
    payload.extend_from_slice(b"\r\n");
    payload
}

/// `Subject` 取值：在第一个 `\r` 或 `\n` 处截断。
///
/// 截断（而不是替换成空格）后残留字符不可能是新的一行，
/// 因此 `x\r\nBcc: a@b` 只会得到 `Subject: x`。
fn sanitize_subject(title: &str) -> String {
    match title.find(['\r', '\n']) {
        Some(index) => title[..index].trim_end().to_string(),
        None => title.trim_end().to_string(),
    }
}

/// 剥掉行内非法字符（`\r` / `\n`）：用于地址等**必须单行**的取值。
///
/// 地址里混入 CRLF 会拼出额外的 SMTP 命令；剥除后服务端只会把它当成一个坏地址拒掉。
fn sanitize_inline(value: &str) -> String {
    value.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

/// 把 base64 文本按 76 字符折行，行间用 `\r\n`（RFC 2045）。
fn wrap_base64(encoded: &str) -> String {
    let chars: Vec<char> = encoded.chars().collect();
    let mut out = String::with_capacity(chars.len() + chars.len() / 76 * 2 + 2);
    for (index, chunk) in chars.chunks(76).enumerate() {
        if index > 0 {
            out.push_str("\r\n");
        }
        out.extend(chunk.iter());
    }
    out
}

// -- 连接与会话 ------------------------------------------------------------

/// 当前使用的传输层：STARTTLS 前后的两种**客户端**流。
enum Wire {
    /// STARTTLS 之前。
    Plain(TcpStream),
    /// STARTTLS 之后（**客户端侧**流 —— 生产代码只用这一种）。
    ///
    /// 装箱：`TlsStream` 有一千多字节（含 rustls 的读写缓冲），
    /// 直接内联会让整个枚举（以及每个 `Connection`）膨胀数倍。
    Tls(Box<TlsStream<TcpStream>>),
    /// STARTTLS 之后（**服务端侧**流 —— 只有测试里的假服务器用）。
    ///
    /// ⚠️ 为什么需要单独的变体：`tokio_rustls` 的 `client::TlsStream` 与
    /// `server::TlsStream` 是**两个不同的类型**，而假服务器用的是
    /// `TlsAcceptor`（服务端）→ 产不出 `client::TlsStream`。
    /// 加一个变体比让测试另写一套 `Connection` 更省，且**生产路径永远不会构造它**
    /// （`#[cfg(test)]` 让它连编译期都不存在于发布产物）。
    #[cfg(test)]
    ServerTls(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}

impl Wire {
    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf).await,
            Self::Tls(stream) => stream.read(buf).await,
            #[cfg(test)]
            Self::ServerTls(stream) => stream.read(buf).await,
        }
    }

    async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.write_all(buf).await,
            Self::Tls(stream) => stream.write_all(buf).await,
            #[cfg(test)]
            Self::ServerTls(stream) => stream.write_all(buf).await,
        }
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush().await,
            Self::Tls(stream) => stream.flush().await,
            #[cfg(test)]
            Self::ServerTls(stream) => stream.flush().await,
        }
    }
}

/// 一条 SMTP 应答（已把多行合并）。
#[derive(Debug, Clone)]
struct Reply {
    /// 服务端应答码。
    code: u16,
    /// 全部应答行（`\n` 连接）。
    text: String,
}

/// 带行缓冲的会话：**自己缓冲**而不是 `BufReader`，因为 STARTTLS 升级要把裸
/// `TcpStream` 交出去握手（`BufReader` 持有它就无法移动）。
struct Connection {
    wire: Wire,
    /// 已读出但还没按行切走的字节。
    buffer: Vec<u8>,
}

impl Connection {
    fn new(wire: Wire) -> Self {
        Self {
            wire,
            buffer: Vec::new(),
        }
    }

    /// 发一行（自动补 `\r\n`）。
    async fn write_line(&mut self, line: &str) -> Result<()> {
        let mut bytes = Vec::with_capacity(line.len() + 2);
        bytes.extend_from_slice(line.as_bytes());
        bytes.extend_from_slice(b"\r\n");
        self.write_raw(&bytes).await
    }

    /// 发原始字节。
    async fn write_raw(&mut self, bytes: &[u8]) -> Result<()> {
        self.wire
            .write_all(bytes)
            .await
            .context("写入 SMTP 命令失败")?;
        self.wire.flush().await.context("刷新 SMTP 连接失败")?;
        Ok(())
    }

    /// 读一行（不含行尾 `\r\n`）。
    async fn read_line(&mut self) -> Result<String> {
        loop {
            if let Some(index) = self.buffer.windows(2).position(|window| window == b"\r\n") {
                let line = String::from_utf8_lossy(&self.buffer[..index]).into_owned();
                self.buffer.drain(..index + 2);
                return Ok(line);
            }
            let mut chunk = [0u8; 512];
            let read = self
                .wire
                .read(&mut chunk)
                .await
                .context("读取 SMTP 应答失败")?;
            if read == 0 {
                bail!("smtp connection closed：服务端在应答中途关闭了连接");
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }

    /// 读一条完整应答：`250-` 是续行，`250 `（第 4 字节为空格）是最后一行。
    ///
    /// ③ **必须读完整**：只读首行会让后续命令读到残留的续行，整条会话错位。
    async fn read_reply(&mut self) -> Result<Reply> {
        let mut lines: Vec<String> = Vec::new();
        loop {
            let line = self.read_line().await?;
            let code = line
                .get(..3)
                .and_then(|prefix| prefix.parse::<u16>().ok())
                .ok_or_else(|| anyhow!("smtp 应答格式非法：{line}"))?;
            // 第 4 字节：`-` 表示还有续行，空格（或行太短）表示结束。
            let continued = line.as_bytes().get(3) == Some(&b'-');
            lines.push(line);
            if !continued {
                return Ok(Reply {
                    code,
                    text: lines.join("\n"),
                });
            }
        }
    }

    /// 读应答并要求应答码相符。
    ///
    /// 不符时**把服务端应答码写进错误文案**（`AUTH` 535 / `RCPT` 550 必须可见）。
    async fn expect(&mut self, expected: u16, step: &str) -> Result<Reply> {
        let reply = self.read_reply().await?;
        if reply.code != expected {
            bail!(
                "smtp {step} 被拒绝：服务端应答 {}（期望 {expected}）：{}",
                reply.code,
                reply.text
            );
        }
        Ok(reply)
    }

    /// 发 `EHLO <host>` 并吃掉整个（可能多行的）250 应答。
    async fn ehlo(&mut self, host: &str) -> Result<()> {
        self.write_line(&format!("EHLO {host}")).await?;
        self.expect(250, "EHLO").await?;
        Ok(())
    }

    /// STARTTLS 升级：交出裸 `TcpStream` 完成握手，返回一条基于 TLS 的新会话。
    async fn upgrade_to_tls(
        self,
        connector: &TlsConnector,
        server_name: ServerName<'static>,
    ) -> Result<Self> {
        let stream = match self.wire {
            Wire::Plain(stream) => stream,
            _ => bail!("STARTTLS 只能在明文连接上发起"),
        };
        // 220 已被 `expect(220, "STARTTLS")` 完整读走；这里的缓冲区随 `self` 一起丢弃。
        let tls = connector
            .connect(server_name, stream)
            .await
            .context("SMTP STARTTLS 握手失败")?;
        Ok(Self::new(Wire::Tls(Box::new(tls))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio_rustls::TlsAcceptor;

    /// 测试用**自签 CA**（`CN=platform-notify test CA`，`CA:TRUE`，DER 的 base64）。
    ///
    /// 内嵌而不是 `include_bytes!`：测试要能独立跑，不依赖工作目录或外部文件。
    /// 必须是真实的 CA→终端证书链：rustls 会拒绝「拿 CA 证书当终端证书」
    /// （`CaUsedAsEndEntity`），所以不能用 `openssl req -x509` 直接签服务端证书。
    const TEST_CA_DER_B64: &str = concat!(
        "MIIDFjCCAf6gAwIBAgIUcwwOPLHjs7ZVB1KgreNmciCYe20wDQYJKoZIhvcNAQELBQAwIjEg",
        "MB4GA1UEAwwXcGxhdGZvcm0tbm90aWZ5IHRlc3QgQ0EwIBcNMjYxMTIxMTU0NDIxWhgPMjEy",
        "NjEwMjgxNTQ0MjFaMCIxIDAeBgNVBAMMF3BsYXRmb3JtLW5vdGlmeSB0ZXN0IENBMIIBIjAN",
        "BgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAx0Lnt3dCGLLQy+ppbSHSE+FIyvdgYb/G0IFw",
        "FqAE8lRMwUoFwhiLyqhYrvTCVDKH+2xQJ4Ym+47O3H16A6MH9eC+sWvKxwft00PcpDTvG7p4",
        "7saHK7AlXKqHKBlYpra6vaOZfD8YLTtawLUXWxiTOIwwODYz5xni0jGCt45j9QdFpuJsw5oK",
        "CeWHzXFVnMcFOcigI5WH4PcvViwYD5O5RNMm8Kme1aIb/548M9yp9NlYLSnbT1CWxMBZrjnP",
        "3RUiCujLhfihHAfCLZODEBRvuEKzC4Bd2t5gGXKokFcinCXLxmto1nZsfjWI96hsr16ghioH",
        "sTm/bA6w+ciRYbLvcwIDAQABo0IwQDAdBgNVHQ4EFgQU2LprBijUG/GudZqfXW7i4+TYM6Iw",
        "DwYDVR0TAQH/BAUwAwEB/zAOBgNVHQ8BAf8EBAMCAQYwDQYJKoZIhvcNAQELBQADggEBAEmX",
        "xwx4nDOcQXfGbcFSHuJAzF80G8pjrV09Fh3SrQVI3vpVDrp+5AHyvpRrrgw5BjSKZKZMV+xA",
        "ZTCB6QSFuQNwq2ojLWKefDKh6A2NPrrd2bP1rMWnIYQzwgK/jqE9YRJBmf7sgRvwiriJco5N",
        "ngsDIbKHXCMQi69vrV0QJsyHSyOMg9xpjMaYz1dUxURqCJKL/splZFnBSZtDCA5m/50pSrY6",
        "kLzu3i/gOh1nozKLw8Mae43q1vNJ54cIaSyFFffRHQrBWCrXvWys/4TTYXtI/i/Bqnkvzh9x",
        "SsT99FNAFPura6ip1m7kjTHdUeDT2Hka6mnxl2ipzF8e4UBXqjw=",
    );

    /// 由上面那张 CA 签发的**服务端**证书（`CN=localhost`，SAN `DNS:localhost`）。
    const TEST_SERVER_CERT_DER_B64: &str = concat!(
        "MIIDETCCAfmgAwIBAgIUO8bCt6LiWDCTnI8td/6OyG1+7k8wDQYJKoZIhvcNAQELBQAwIjEg",
        "MB4GA1UEAwwXcGxhdGZvcm0tbm90aWZ5IHRlc3QgQ0EwIBcNMjYxMTIxMTU0NDIxWhgPMjEy",
        "NjEwMjgxNTQ0MjFaMBQxEjAQBgNVBAMMCWxvY2FsaG9zdDCCASIwDQYJKoZIhvcNAQEBBQAD",
        "ggEPADCCAQoCggEBAOWf0KKsKTrBkTzQoWojwPrCZksAEBHGQ3ztKcuC9cBjQxdC7h3qKwtu",
        "GRkq9N6HtKJnldYGcyo/2F5o7NEXULqPUoKoFmBP69vNlKDVRVhf2IxqjpSP39fQA/9viXeJ",
        "ml7MBIkeTH5Xr/xTkZbLr1R8qD4gJzXWMUsfnw90o+PuhB5C5VInIFbum6tWN30KMLtw6kfB",
        "NeFi1RM+lHc0pEsoj4Q7VWzOA2IgeFtheAximBivMnNahirP11mgPpc2qQdgAbrhlZ4xatci",
        "R2G6pwApU926qfExdehxwKvGgGx7z2ZXg/sHHEAQ0wjyRPSWLXXU4x8n2ztGCbWBGV4ZGxEC",
        "AwEAAaNLMEkwDAYDVR0TAQH/BAIwADAOBgNVHQ8BAf8EBAMCBaAwEwYDVR0lBAwwCgYIKwYB",
        "BQUHAwEwFAYDVR0RBA0wC4IJbG9jYWxob3N0MA0GCSqGSIb3DQEBCwUAA4IBAQAKyQmeUVyu",
        "jYgTGflqOStpoO56DqH9hdFAbY0xbZLVNMjvkZnDSQX7iurjKUpRIUzfYmGuuJXKShS1ngPB",
        "JBDbm7dGq0ZtnW3QW9T5gnWUfTVHWJHY6LW11ERSkzF7bE//aTAU+heGONjE31wt2zxR04fR",
        "//+qaKKL55oHA9TdqIQBBQVPNvyZnOuLbnBY27S6DEwEWFepcK1gptfVOLC7SQWLJSBAxDhm",
        "/o8crzT1dGDSaXS1PqpMHpPeIi2giX5oQNLnQ41UPjxePN0yGqO7t13f8NLcJ5bNf9WQDIRt",
        "bBcHVUla+6dvfmr8dKcE7bnH5y095/aOCTcDJt52LiDJ",
    );

    /// 服务端证书对应的私钥（PKCS#8 DER 的 base64）。
    const TEST_SERVER_KEY_DER_B64: &str = concat!(
        "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDln9CirCk6wZE80KFqI8D6",
        "wmZLABARxkN87SnLgvXAY0MXQu4d6isLbhkZKvTeh7SiZ5XWBnMqP9heaOzRF1C6j1KCqBZg",
        "T+vbzZSg1UVYX9iMao6Uj9/X0AP/b4l3iZpezASJHkx+V6/8U5GWy69UfKg+ICc11jFLH58P",
        "dKPj7oQeQuVSJyBW7purVjd9CjC7cOpHwTXhYtUTPpR3NKRLKI+EO1VszgNiIHhbYXgMYpgY",
        "rzJzWoYqz9dZoD6XNqkHYAG64ZWeMWrXIkdhuqcAKVPduqnxMXXoccCrxoBse89mV4P7BxxA",
        "ENMI8kT0li111OMfJ9s7Rgm1gRleGRsRAgMBAAECggEAD7HD/CUG5GHaisI6Zhtvfd7Pnbdc",
        "DHr1SJKagAhsF5bb57kcC/tiBHP6pIJlHi3ppg1XwRB2dUrL1RgYJ5UPtXtE88fdT6k8Flfk",
        "yAwLo1CvElnYyO//KAGrDvMRUumayCPq+MbotNygG1Ty5yWgztt+UaYM8rG4c3QD1nXtxbmk",
        "isoTXjUPB/MYy5sc+AnJlkBRQP4WWlEdjZ/D+AqOe5OvJ+QjVmjRWO75Do9nBFnfwypYMmgO",
        "Vw+KNQXx1ztWuCBH0hze/2ZGHko5WkMCEL6WMRaAPuHg2yDxu7DJnAlfEdjgzamZQlKHS7o7",
        "jC5G5yaOopRd+8oyQI9R5XZEYQKBgQDyyD4An0LVuPR1gflHdnsRzsICJBAG7kDu3KGulU6O",
        "VJKE6UH8SM4qV+dqpPg/+nbm4MgO7HiMEh2fRs9xGqqxjVoS+zpjxyh3e75hQhSAfywKUumv",
        "jkmM7TJG3JxRExDcfZcA7V0tzt7ZEXN0dhSafDcQKCuAso9NZZm9ehj/fQKBgQDyIC9xbWjP",
        "1iFtN7t4oLXQjHvhPpdGKw6NKtctcmxwEHJzjz2+zPfSifT3wUgYbBe6/S7rUr8IqdRStkKF",
        "yiEFL3qQAd78tjvOBiORIX7PwyInTuHRk9IEW0YOXdKrSFSifBdYRfQ2r53cpw2etfP3odzk",
        "sWTyUa+R+hY8LyBGJQKBgQCPWIm7Rk1o8T5tTyOG/T6nAgPszawk23G9Vsmj0gZC5U5paWwL",
        "cFVCDQrXHbZx2AlyQk/898QpN90DmQSMTGlyUfpX8LvGA+Yxvf9nX2PJhszS/6d4fzOQINCp",
        "pEIrYG15MjIBWyTEEjvjwHkBz2JU4HiGcqFU6JjR4uZGSJwP5QKBgEIrFYwT8E7MH/pu/qaq",
        "lMVXfwtBRaoBXkqp6IAdAxFlDUITZTZdYWSCGxYSFhfGgn2xbz8UL+y4LLQl5uwVHw0Buqbc",
        "PAon6xQUjWe+FZ/FnfHMFt5lXUGfh/epIEd6ztZQvzh6dZD0UVnZtVkoNHH6JBnDeOjX9fkY",
        "tTbhUhqZAoGBAJqRd+vqKy0nWe+YYYBeuyB5GqxiPbUuD9pUeFVwNej43pLr5ar8JyeSpRdS",
        "7gglO40kNDPHqvWw4N3L7ZGGkAEadGF2Ec87Inx/UfNbrz1wAKh/50/j87Ts7JCpZCMre5uz",
        "KT/0BZtG06yq5O3xBZxKx+NzzS9/7UDXm8Df6lJS",
    );

    /// 假服务器的一次会话行为。
    #[derive(Clone, Copy)]
    struct Behaviour {
        /// `AUTH` 的最终应答码（`235` 成功 / `535` 失败）。
        auth_code: u16,
        /// 第几个 `RCPT`（1 起）回 `550`。
        rcpt_fail_at: Option<usize>,
        /// 只在 TLS 之后的 EHLO 应答里宣告 `AUTH`。
        auth_only_after_tls: bool,
        /// EHLO 应答用多行（`250-` 续行）。
        multiline_ehlo: bool,
        /// accept 之后什么都不写（触发客户端超时）。
        silent: bool,
    }

    impl Default for Behaviour {
        fn default() -> Self {
            Self {
                auth_code: 235,
                rcpt_fail_at: None,
                auth_only_after_tls: false,
                multiline_ehlo: false,
                silent: false,
            }
        }
    }

    fn decode(value: &str) -> Vec<u8> {
        BASE64.decode(value).expect("内嵌的 base64 必须可解")
    }

    fn tls_acceptor() -> TlsAcceptor {
        // 只发终端证书；客户端用 CA 验证（CA 不必进证书链）。
        let certs = vec![CertificateDer::from(decode(TEST_SERVER_CERT_DER_B64))];
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(decode(TEST_SERVER_KEY_DER_B64)));
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("默认协议版本可用")
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("测试证书与私钥必须配对");
        TlsAcceptor::from(Arc::new(config))
    }

    /// 只信任内嵌测试 CA 的客户端配置（生产用公网根，测试用这张）。
    fn trusting_client_config() -> Arc<rustls::ClientConfig> {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from(decode(TEST_CA_DER_B64)))
            .expect("测试 CA 必须能作为信任锚");
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("默认协议版本可用")
        .with_root_certificates(roots)
        .with_no_client_auth();
        Arc::new(config)
    }

    /// 假服务器句柄。
    struct FakeServer {
        /// 监听端口（主机名固定 `localhost` —— 证书 SAN 就是它）。
        port: u16,
        /// `EHLO` 收到的次数。
        ehlo_count: Arc<AtomicUsize>,
        /// 捕获到的 `DATA` 正文（含头）。
        data: Arc<Mutex<String>>,
    }

    /// 起一个假 SMTP 服务器，按 `Behaviour` 回应一轮完整会话。
    async fn spawn_server(behaviour: Behaviour) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("应能绑定回环端口");
        let port = listener.local_addr().expect("应有本地地址").port();
        let ehlo_count = Arc::new(AtomicUsize::new(0));
        let data = Arc::new(Mutex::new(String::new()));
        let acceptor = tls_acceptor();

        let (done_tx, done_rx) = oneshot::channel::<()>();
        let counts = ehlo_count.clone();
        let captured = data.clone();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("应能 accept 一个连接");
            if behaviour.silent {
                // 什么都不写：让客户端的超时生效。
                let _ = done_rx.await;
                return;
            }
            let mut conn = Connection::new(Wire::Plain(socket));
            conn.write_line("220 fake ESMTP ready")
                .await
                .expect("应能发问候");

            // ② 第一次 EHLO（明文）。
            let line = conn.read_line().await.expect("应收到 EHLO");
            assert!(line.starts_with("EHLO "), "期望 EHLO，收到 {line}");
            counts.fetch_add(1, Ordering::SeqCst);
            send_ehlo(&mut conn, behaviour, false).await;

            // ③ STARTTLS + 握手。
            let line = conn.read_line().await.expect("应收到 STARTTLS");
            assert_eq!(line, "STARTTLS");
            conn.write_line("220 ready to start TLS")
                .await
                .expect("应能同意 STARTTLS");
            let stream = match conn.wire {
                Wire::Plain(stream) => stream,
                _ => panic!("STARTTLS 前必须是明文连接"),
            };
            let tls = acceptor.accept(stream).await.expect("TLS 握手应成功");
            let mut conn = Connection::new(Wire::ServerTls(Box::new(tls)));

            // ④ TLS 之后必须再来一次 EHLO。
            let line = conn.read_line().await.expect("应收到 TLS 后的 EHLO");
            assert!(line.starts_with("EHLO "), "期望 EHLO，收到 {line}");
            counts.fetch_add(1, Ordering::SeqCst);
            send_ehlo(&mut conn, behaviour, true).await;

            // ⑤ AUTH LOGIN。
            let line = conn.read_line().await.expect("应收到 AUTH LOGIN");
            assert_eq!(line, "AUTH LOGIN");
            conn.write_line("334 VXNlcm5hbWU6")
                .await
                .expect("应发用户名提示");
            let username = conn.read_line().await.expect("应收到 base64 用户名");
            assert!(!username.is_empty(), "用户名不得为空");
            conn.write_line("334 UGFzc3dvcmQ6")
                .await
                .expect("应发密码提示");
            let password = conn.read_line().await.expect("应收到 base64 密码");
            assert!(!password.is_empty(), "密码不得为空");
            if behaviour.auth_code == 235 {
                conn.write_line("235 2.7.0 accepted")
                    .await
                    .expect("应认证通过");
            } else {
                let code = behaviour.auth_code;
                conn.write_line(&format!("{code} 5.7.8 authentication failed"))
                    .await
                    .expect("应拒认证");
                let _ = done_rx.await;
                return;
            }

            // ⑥ MAIL FROM。
            let line = conn.read_line().await.expect("应收到 MAIL FROM");
            assert!(line.starts_with("MAIL FROM:<"), "收到 {line}");
            conn.write_line("250 2.1.0 ok").await.expect("应接受发件人");

            // ⑦ RCPT TO（逐个）。
            let mut index = 0usize;
            loop {
                let line = conn.read_line().await.expect("应收到 RCPT 或 DATA");
                if let Some(expected_fail) = behaviour.rcpt_fail_at {
                    assert!(
                        !line.starts_with("DATA"),
                        "客户端在收件人被拒后仍在继续（第 {expected_fail} 个应失败）"
                    );
                }
                if line.starts_with("RCPT TO:<") {
                    index += 1;
                    if behaviour.rcpt_fail_at == Some(index) {
                        conn.write_line("550 5.1.1 no such user")
                            .await
                            .expect("应拒收件人");
                        // 客户端会在这一步失败并断开：不再往后读。
                        let _ = done_rx.await;
                        return;
                    }
                    conn.write_line("250 2.1.5 ok").await.expect("应接受收件人");
                    continue;
                }
                assert_eq!(line, "DATA", "未预期的命令 {line}");
                break;
            }

            // ⑧ DATA。
            conn.write_line("354 end with <CRLF>.<CRLF>")
                .await
                .expect("应接受 DATA");
            let mut body = String::new();
            loop {
                let line = conn.read_line().await.expect("应收到正文行");
                if line == "." {
                    break;
                }
                body.push_str(&line);
                body.push_str("\r\n");
            }
            *captured.lock().unwrap_or_else(|err| err.into_inner()) = body;
            conn.write_line("250 2.0.0 QUEUE-ID")
                .await
                .expect("应接收正文");

            // ⑨ QUIT。
            let line = conn.read_line().await.expect("应收到 QUIT");
            assert_eq!(line, "QUIT");
            conn.write_line("221 bye").await.expect("应告别");
            let _ = done_tx.send(());
        });

        FakeServer {
            port,
            ehlo_count,
            data,
        }
    }

    /// 发 EHLO 应答。`after_tls` 决定是否宣告 `AUTH`。
    async fn send_ehlo(conn: &mut Connection, behaviour: Behaviour, after_tls: bool) {
        let advertise_auth = !behaviour.auth_only_after_tls || after_tls;
        if behaviour.multiline_ehlo {
            // 多行应答：`250-` 续行、`250 ` 结尾。
            conn.write_raw(
                b"250-fake.example greets you\r\n\
                  250-SIZE 10485760\r\n\
                  250-8BITMIME\r\n",
            )
            .await
            .expect("应发多行续行");
            let tail = if advertise_auth {
                "250 AUTH LOGIN\r\n"
            } else {
                "250 HELP\r\n"
            };
            conn.write_raw(tail.as_bytes()).await.expect("应发多行结尾");
        } else if advertise_auth {
            conn.write_line("250 AUTH LOGIN").await.expect("应发 EHLO");
        } else {
            conn.write_line("250 HELP").await.expect("应发 EHLO");
        }
    }

    fn config(port: u16) -> SmtpConfig {
        SmtpConfig {
            host: "localhost".to_string(),
            port,
            username: "notify@example.com".to_string(),
            password: "s3cr3t-value".to_string(),
            from: "noreply@example.com".to_string(),
        }
    }

    fn sender(port: u16, timeout: Duration) -> SmtpSender {
        SmtpSender::with_parts(config(port), timeout, Some(trusting_client_config()))
    }

    fn message() -> NotificationMessage {
        NotificationMessage::new("构建完成", "你的构建已成功")
            .with_recipient("alice@example.com")
            .with_recipient("bob@example.com")
    }

    #[tokio::test]
    async fn a_full_delivery_returns_the_queue_id() {
        let server = spawn_server(Behaviour::default()).await;
        let result = sender(server.port, Duration::from_secs(5))
            .send(&RequestContext::new(), &message())
            .await
            .expect("投递应成功");

        assert!(result.delivered, "应判为已投递");
        assert_eq!(result.provider_message_id.as_deref(), Some("QUEUE-ID"));
        assert!(result.error.is_none());
        // STARTTLS 前后各一次 EHLO。
        assert_eq!(server.ehlo_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn the_auth_failure_carries_the_server_code() {
        let server = spawn_server(Behaviour {
            auth_code: 535,
            ..Behaviour::default()
        })
        .await;
        let error = sender(server.port, Duration::from_secs(5))
            .send(&RequestContext::new(), &message())
            .await
            .expect_err("535 必须失败");

        let text = error.to_string();
        assert!(text.contains("535"), "错误文案必须带应答码，实际：{text}");
        // 密码绝不出现在错误里。
        assert!(!text.contains("s3cr3t-value"), "密码不得进错误文案");
    }

    #[tokio::test]
    async fn a_rejected_recipient_fails_the_whole_message() {
        let server = spawn_server(Behaviour {
            rcpt_fail_at: Some(2),
            ..Behaviour::default()
        })
        .await;
        let error = sender(server.port, Duration::from_secs(5))
            .send(&RequestContext::new(), &message())
            .await
            .expect_err("第 2 个收件人被拒必须整条失败");

        let text = error.to_string();
        assert!(text.contains("550"), "错误文案必须带应答码，实际：{text}");
    }

    #[tokio::test]
    async fn auth_is_only_sent_after_the_second_ehlo() {
        let server = spawn_server(Behaviour {
            auth_only_after_tls: true,
            ..Behaviour::default()
        })
        .await;
        let result = sender(server.port, Duration::from_secs(5))
            .send(&RequestContext::new(), &message())
            .await
            .expect("TLS 后才宣告 AUTH 也应成功");

        assert!(result.delivered);
        assert_eq!(
            server.ehlo_count.load(Ordering::SeqCst),
            2,
            "TLS 之后必须重发 EHLO（不是只发一次）"
        );
    }

    #[tokio::test]
    async fn a_silent_server_times_out() {
        let server = spawn_server(Behaviour {
            silent: true,
            ..Behaviour::default()
        })
        .await;
        let started = std::time::Instant::now();
        let error = sender(server.port, Duration::from_millis(200))
            .send(&RequestContext::new(), &message())
            .await
            .expect_err("对端沉默必须超时");

        let text = error.to_string();
        assert!(
            text.contains("timeout"),
            "错误文案必须含 timeout，实际：{text}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "必须按注入的超时（200ms）返回，而不是等满 10 秒"
        );
    }

    #[tokio::test]
    async fn a_multi_line_ehlo_reply_is_read_completely() {
        let server = spawn_server(Behaviour {
            multiline_ehlo: true,
            ..Behaviour::default()
        })
        .await;
        let result = sender(server.port, Duration::from_secs(5))
            .send(&RequestContext::new(), &message())
            .await
            .expect("多行 250 必须被读完整，后续命令才不会错位");

        assert!(result.delivered);
        assert_eq!(result.provider_message_id.as_deref(), Some("QUEUE-ID"));
    }

    #[tokio::test]
    async fn the_subject_cannot_inject_headers() {
        let server = spawn_server(Behaviour::default()).await;
        // 注入企图：CRLF 之后跟一条伪 Bcc。
        let message = NotificationMessage::new("构建完成\r\nBcc: attacker@evil.example", "正文")
            .with_recipient("alice@example.com");

        sender(server.port, Duration::from_secs(5))
            .send(&RequestContext::new(), &message)
            .await
            .expect("截断标题后仍应投递成功");

        let body = server
            .data
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        assert!(
            !body.to_lowercase().contains("bcc:"),
            "标题里的 Bcc 不得落地成收件人头：\n{body}"
        );
        assert!(
            body.contains("Subject: 构建完成\r\n"),
            "标题应在 CRLF 处被截断：\n{body}"
        );
    }

    #[test]
    fn the_base64_body_wraps_at_76_columns() {
        // 200 字节 → base64 是 268 字符，必然折成 4 行（76/76/76/40）。
        let encoded = BASE64.encode(vec![b'a'; 200]);
        let wrapped = wrap_base64(&encoded);

        assert_eq!(wrapped.len(), encoded.len() + 3 * 2, "三处换行各占 2 字节");
        let lines: Vec<&str> = wrapped.split("\r\n").collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].len(), 76);
        assert_eq!(lines[1].len(), 76);
        assert_eq!(lines[2].len(), 76);
        assert_eq!(lines[3].len(), 40);
        // 折行不得改动内容：拼回去必须与原编码一致。
        assert_eq!(lines.concat(), encoded);
    }

    #[test]
    fn a_short_body_stays_on_one_line() {
        let encoded = BASE64.encode(b"short");
        assert_eq!(wrap_base64(&encoded), encoded);
    }
}
