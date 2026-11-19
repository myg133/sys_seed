//! OIDC 授权码流程 + PKCE（REQ-013 AC-S-100 ~ AC-S-102）。
//!
//! # 验签的三条铁律
//!
//! 1. **算法白名单硬编码** `{RS256, ES256}`（P5）。白名单**不可由配置收窄**——
//!    design §7 也没有这个键。
//! 2. **绝不按 token 头里的 `alg` 决定验签方式**：先读 header 的 `alg`，
//!    不在白名单里就**先拒**（`none` / `HS*` / `RS512` 一律
//!    `SSO_OIDC_ALG_NOT_ALLOWED`）。这同时封死 RS→HS 混淆：
//!    用 IdP 公钥当 HMAC 密钥伪造的 token，`alg=HS256` 在第一关就被拒。
//! 3. **只从 `jwks_uri` 取公钥**：`header.jku` / `header.x5c` **从不读取**
//!    —— 从 token 自身取公钥等于不验签。AC-S-102 步骤 9/11 用
//!    「攻击者 JWKS 端点被请求 0 次」守这条。
//!
//! # PKCE 篡改要在**本地**可发现
//!
//! `code_verifier` 存在 `oidc_state` 里；如果只把 verifier 发给 IdP、
//! 等 IdP 回 `invalid_grant`，那「verifier 被篡改」与「code 无效」在响应上**不可区分**。
//! 所以库里同时存了 `code_challenge`，回调时**先本地重算** challenge 比对，
//! 不一致 → `400 SSO_OIDC_PKCE_MISMATCH`（AC-S-101 步骤 6）。
//!
//! # 远端故障要能分锅（AC-S-020）
//!
//! | 症状 | 码 | HTTP |
//! |------|----|------|
//! | 拿不到发现文档（连不上 / 非 2xx） | `SSO_OIDC_DISCOVERY_FAILED` | 503 |
//! | 拿到了但非法 / 缺端点 | `SSO_OIDC_DISCOVERY_INVALID` | 502 |
//! | token 端点 5xx / 超时 | `SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE` | 502 |
//! | token 端点 4xx（code 伪造 / 已用） | `SSO_OIDC_TOKEN_EXCHANGE_FAILED` | 400 |

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::IamError;

use super::config::OidcSettings;

/// `state` / `ticket` 的 TTL（**代码常量**，不新增配置键 —— P9）。
pub const OIDC_STATE_TTL: Duration = Duration::from_secs(300);

/// 允许的签名算法（**硬编码**，P5）。
pub const ALLOWED_ALGORITHMS: [Algorithm; 2] = [Algorithm::RS256, Algorithm::ES256];

/// 允许的算法名（ASCII，用于错误日志与断言）。
pub const ALLOWED_ALGORITHM_NAMES: [&str; 2] = ["RS256", "ES256"];

/// 允许的时钟偏移（秒）—— **显式**给 60，不用隐式默认。
pub const CLOCK_SKEW_SECONDS: u64 = 60;

/// 发现文档 / JWKS 的缓存 TTL（小时级：这些端点极少变）。
const DISCOVERY_TTL: Duration = Duration::from_secs(3600);

/// 出站 HTTP 的单次超时。
const HTTP_TIMEOUT: Duration = Duration::from_secs(8);

/// token 的 `provider` 取值（`oidc_state.provider`）。
pub const PROVIDER: &str = "oidc";

// ---------------------------------------------------------------------------
// 出站 HTTP：继续用 hyper + hyper-util，**只换 connector**（不引 reqwest）
// ---------------------------------------------------------------------------

type HttpsConnector =
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>;
type HttpsClient = hyper_util::client::legacy::Client<HttpsConnector, Full<Bytes>>;

/// 出站 HTTP 的传输层错误（**不含 URL 细节之外的任何内容**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// 连不上 / 超时。
    Unreachable,
}

/// 面向 `https://` 与 `http://` **都要能用**的客户端（`https_or_http()`）。
///
/// - 生产：webpki 公网根（[`OidcHttpClient::new`]）
/// - 测试：注入测试根（[`OidcHttpClient::with_root_certificate_der`]）；
///   `with_webpki_roots()` **不信任自签根**，所以必须有这条路径（P10 / S13）
pub struct OidcHttpClient {
    client: HttpsClient,
}

impl std::fmt::Debug for OidcHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OidcHttpClient")
    }
}

impl OidcHttpClient {
    /// 生产：信任公网根。
    ///
    /// 显式传 ring provider（`with_provider_and_webpki_roots`）：
    /// `with_webpki_roots()` 走的是 `ClientConfig::builder()`（**进程默认 provider**），
    /// 一旦测试构建里同时编进了 aws-lc-rs，它就会因「无法确定默认 provider」而 panic。
    pub fn new() -> Result<Self, IamError> {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_provider_and_webpki_roots(Arc::new(rustls::crypto::ring::default_provider()))
            .map_err(|_| IamError::SsoConfigInvalid {
                reason: "the tls client configuration is invalid",
            })?
            .https_or_http()
            .enable_http1()
            .build();
        Ok(Self {
            client: hyper_util::client::legacy::Client::builder(
                hyper_util::rt::TokioExecutor::new(),
            )
            .build(connector),
        })
    }

    /// 测试：只信任给定的（自签）根证书 DER。
    pub fn with_root_certificate_der(der: &[u8]) -> Result<Self, IamError> {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(der.to_vec()))
            .map_err(|_| IamError::SsoConfigInvalid {
                reason: "the test root certificate is invalid",
            })?;
        // 显式钉 ring provider：即使别的依赖也编进了 aws-lc-rs，
        // `ClientConfig::builder()` 也不会因「没有安装进程级 provider」而 panic。
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| IamError::SsoConfigInvalid {
            reason: "the tls client configuration is invalid",
        })?
        .with_root_certificates(roots)
        .with_no_client_auth();

        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_or_http()
            .enable_http1()
            .build();
        Ok(Self {
            client: hyper_util::client::legacy::Client::builder(
                hyper_util::rt::TokioExecutor::new(),
            )
            .build(connector),
        })
    }

    async fn send(
        &self,
        request: http::Request<Full<Bytes>>,
    ) -> Result<(u16, Option<serde_json::Value>), TransportError> {
        let response = match tokio::time::timeout(HTTP_TIMEOUT, self.client.request(request)).await
        {
            Ok(Ok(response)) => response,
            // 超时 / 连接错误 —— 对上层都是「远端不可达」。
            Ok(Err(_)) | Err(_) => return Err(TransportError::Unreachable),
        };
        let status = response.status().as_u16();
        let bytes = match tokio::time::timeout(HTTP_TIMEOUT, response.into_body().collect()).await {
            Ok(Ok(collected)) => collected.to_bytes(),
            Ok(Err(_)) | Err(_) => return Err(TransportError::Unreachable),
        };
        // 解析失败不算传输失败 —— 由调用方按 `None` 判「非法响应」。
        Ok((status, serde_json::from_slice(&bytes).ok()))
    }

    /// GET 一个 JSON 文档。
    pub async fn get_json(
        &self,
        url: &str,
    ) -> Result<(u16, Option<serde_json::Value>), TransportError> {
        let request = http::Request::builder()
            .method("GET")
            .uri(url)
            .header(http::header::ACCEPT, "application/json")
            .body(Full::new(Bytes::new()))
            .map_err(|_| TransportError::Unreachable)?;
        self.send(request).await
    }

    /// POST 一个表单（`application/x-www-form-urlencoded`）。
    pub async fn post_form(
        &self,
        url: &str,
        form: &[(String, String)],
    ) -> Result<(u16, Option<serde_json::Value>), TransportError> {
        let body = form
            .iter()
            .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
            .collect::<Vec<_>>()
            .join("&");
        let request = http::Request::builder()
            .method("POST")
            .uri(url)
            .header(
                http::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(http::header::ACCEPT, "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|_| TransportError::Unreachable)?;
        self.send(request).await
    }
}

// ---------------------------------------------------------------------------
// 发现文档 + JWKS（带缓存）
// ---------------------------------------------------------------------------

/// 发现文档里我们用到的三个端点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// 授权端点。
    pub authorization_endpoint: String,
    /// token 端点。
    pub token_endpoint: String,
    /// JWKS 端点（**公钥的唯一来源**）。
    pub jwks_uri: String,
}

/// 发现文档 + JWKS 的内存缓存（**性能优化，不是正确性依赖**）。
///
/// `Clone` 是**共享**语义（内部全是 `Arc`）：`IamService` 的每个克隆看到同一份缓存。
#[derive(Clone, Debug, Default)]
pub struct OidcRuntime {
    http: Arc<OnceLock<Arc<OidcHttpClient>>>,
    discovery: Arc<Mutex<HashMap<String, (Instant, Discovery)>>>,
    jwks: Arc<Mutex<HashMap<String, (Instant, BTreeMap<String, DecodingKey>)>>>,
}

impl OidcRuntime {
    /// 空运行时（HTTP 客户端由 [`OidcRuntime::install_http`] 注入）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注入 HTTP 客户端（测试要注入带测试根的客户端）。
    ///
    /// 返回 `false` = 已注入过（不覆盖）。
    pub fn install_http(&self, http: Arc<OidcHttpClient>) -> bool {
        self.http.set(http).is_ok()
    }

    /// 出站 HTTP 客户端（懒建生产客户端）。
    pub fn http(&self) -> Result<Arc<OidcHttpClient>, IamError> {
        if let Some(client) = self.http.get() {
            return Ok(client.clone());
        }
        // 生产默认：webpki 公网根。懒建 —— 未装配 SSO 的部署不会付这个初始化成本。
        let client = Arc::new(OidcHttpClient::new()?);
        let _ = self.http.set(client.clone());
        self.http.get().cloned().ok_or(IamError::Internal {
            reason: "the outbound http client is not installed".to_string(),
        })
    }

    /// 拿发现文档（缓存命中直接用；`force` 时重新拉）。
    pub async fn discovery(&self, issuer: &str, force: bool) -> Result<Discovery, IamError> {
        if !force {
            if let Some(hit) = self.cached_discovery(issuer) {
                return Ok(hit);
            }
        }
        let http = self.http()?;
        let document = fetch_discovery(&http, issuer).await?;
        if let Ok(mut guard) = self.discovery.lock() {
            guard.insert(issuer.to_string(), (Instant::now(), document.clone()));
        }
        Ok(document)
    }

    fn cached_discovery(&self, issuer: &str) -> Option<Discovery> {
        let guard = self.discovery.lock().ok()?;
        let (at, document) = guard.get(issuer)?;
        (at.elapsed() < DISCOVERY_TTL).then(|| document.clone())
    }

    /// 拿签名公钥（按 `kid` 索引）。
    pub async fn signing_keys(
        &self,
        issuer: &str,
        jwks_uri: &str,
        force: bool,
    ) -> Result<BTreeMap<String, DecodingKey>, IamError> {
        if !force {
            if let Some(hit) = self.cached_keys(issuer) {
                return Ok(hit);
            }
        }
        let http = self.http()?;
        let keys = fetch_jwks(&http, jwks_uri).await?;
        if let Ok(mut guard) = self.jwks.lock() {
            guard.insert(issuer.to_string(), (Instant::now(), keys.clone()));
        }
        Ok(keys)
    }

    fn cached_keys(&self, issuer: &str) -> Option<BTreeMap<String, DecodingKey>> {
        let guard = self.jwks.lock().ok()?;
        let (at, keys) = guard.get(issuer)?;
        (at.elapsed() < DISCOVERY_TTL).then(|| keys.clone())
    }
}

/// 拉发现文档。
///
/// 判据（AC-S-020 步骤 1/2）：
/// - 传输失败 / 非 2xx → `SSO_OIDC_DISCOVERY_FAILED`（503）
/// - JSON 非法 / 缺端点 → `SSO_OIDC_DISCOVERY_INVALID`（502）
pub async fn fetch_discovery(http: &OidcHttpClient, issuer: &str) -> Result<Discovery, IamError> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let (status, body) = http
        .get_json(&url)
        .await
        .map_err(|_| IamError::SsoOidcDiscoveryFailed)?;
    if !(200..300).contains(&status) {
        return Err(IamError::SsoOidcDiscoveryFailed);
    }
    let Some(body) = body else {
        return Err(IamError::SsoOidcDiscoveryInvalid);
    };
    let authorization_endpoint = body
        .get("authorization_endpoint")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(IamError::SsoOidcDiscoveryInvalid)?;
    let token_endpoint = body
        .get("token_endpoint")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(IamError::SsoOidcDiscoveryInvalid)?;
    let jwks_uri = body
        .get("jwks_uri")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(IamError::SsoOidcDiscoveryInvalid)?;
    Ok(Discovery {
        authorization_endpoint: authorization_endpoint.to_string(),
        token_endpoint: token_endpoint.to_string(),
        jwks_uri: jwks_uri.to_string(),
    })
}

/// 拉 JWKS，转成 `kid → DecodingKey`。
async fn fetch_jwks(
    http: &OidcHttpClient,
    jwks_uri: &str,
) -> Result<BTreeMap<String, DecodingKey>, IamError> {
    let (status, body) = http
        .get_json(jwks_uri)
        .await
        .map_err(|_| IamError::SsoOidcDiscoveryFailed)?;
    if !(200..300).contains(&status) {
        return Err(IamError::SsoOidcDiscoveryFailed);
    }
    let Some(body) = body else {
        return Err(IamError::SsoOidcDiscoveryInvalid);
    };
    let set: jsonwebtoken::jwk::JwkSet =
        serde_json::from_value(body).map_err(|_| IamError::SsoOidcDiscoveryInvalid)?;
    let mut keys = BTreeMap::new();
    for jwk in &set.keys {
        let Some(kid) = jwk.common.key_id.clone() else {
            continue;
        };
        if let Ok(key) = DecodingKey::from_jwk(jwk) {
            keys.insert(kid, key);
        }
    }
    if keys.is_empty() {
        return Err(IamError::SsoOidcDiscoveryInvalid);
    }
    Ok(keys)
}

// ---------------------------------------------------------------------------
// state / nonce / PKCE
// ---------------------------------------------------------------------------

/// 一个高熵的一次性串（base64url(32B) → 43 字符，满足 `^[A-Za-z0-9_-]{43,}$`）。
pub fn random_token() -> String {
    let bytes = rand::random::<[u8; 32]>();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// PKCE 的 `code_verifier`（43 字符，符合 RFC 7636 的 43~128）。
pub fn new_code_verifier() -> String {
    random_token()
}

/// `code_challenge = base64url(SHA256(verifier))`（`S256`）。
pub fn code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// 编码一个 query 值。
pub fn percent_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// 拼出授权跳转 URL。
pub fn authorization_url(
    endpoint: &str,
    settings: &OidcSettings,
    state: &str,
    nonce: &str,
    challenge: &str,
) -> String {
    let separator = if endpoint.contains('?') { '&' } else { '?' };
    let params = [
        ("response_type", "code"),
        ("client_id", settings.client_id.as_str()),
        ("redirect_uri", settings.redirect_uri.as_str()),
        ("scope", settings.scopes.as_str()),
        ("state", state),
        ("nonce", nonce),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    let query = params
        .iter()
        .map(|(key, value)| format!("{key}={}", percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{endpoint}{separator}{query}")
}

// ---------------------------------------------------------------------------
// id_token 验签
// ---------------------------------------------------------------------------

/// `id_token` 里我们需要的声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdTokenClaims {
    /// issuer。
    pub iss: String,
    /// subject（外部身份标识）。
    pub sub: String,
    /// audience（字符串或数组 —— 交给 `jsonwebtoken` 判）。
    #[serde(default)]
    pub aud: serde_json::Value,
    /// 过期时间。
    pub exp: i64,
    /// 签发时间。
    #[serde(default)]
    pub iat: Option<i64>,
    /// nonce。
    #[serde(default)]
    pub nonce: Option<String>,
    /// 邮箱。
    #[serde(default)]
    pub email: Option<String>,
    /// 显示名。
    #[serde(default)]
    pub name: Option<String>,
    /// 用户名（部分 IdP 用）。
    #[serde(default)]
    pub preferred_username: Option<String>,
}

/// 解析出来的 JWT header（**只取我们需要的两个字段**）。
#[derive(Debug, Clone, Deserialize)]
pub struct JwtHeader {
    /// 签名算法（**白名单外一律拒绝**）。
    #[serde(default)]
    pub alg: Option<String>,
    /// 密钥 ID（用来在 `jwks_uri` 的结果里选公钥）。
    #[serde(default)]
    pub kid: Option<String>,
}

fn decode_segment(segment: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(segment)
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE
                .decode(segment)
                .ok()
        })
        .or_else(|| {
            base64::engine::general_purpose::STANDARD
                .decode(segment)
                .ok()
        })
}

/// 读 JWT header 的 `alg` / `kid`。
///
/// **不做任何网络访问** —— `jku` / `x5c` 因此永远不会触发取公钥（AC-S-102 步骤 9/11）。
pub fn decode_jwt_header(token: &str) -> Result<JwtHeader, IamError> {
    let mut parts = token.split('.');
    let header = parts.next().ok_or(IamError::SsoOidcTokenInvalid)?;
    if parts.next().is_none() || parts.next().is_none() || parts.next().is_some() {
        return Err(IamError::SsoOidcTokenInvalid);
    }
    let bytes = decode_segment(header).ok_or(IamError::SsoOidcTokenInvalid)?;
    serde_json::from_slice(&bytes).map_err(|_| IamError::SsoOidcTokenInvalid)
}

/// header 的 `alg` → 白名单内的 [`Algorithm`]（白名单外 → `SSO_OIDC_ALG_NOT_ALLOWED`）。
pub fn allowed_algorithm(alg: Option<&str>) -> Result<Algorithm, IamError> {
    match alg {
        Some("RS256") => Ok(Algorithm::RS256),
        Some("ES256") => Ok(Algorithm::ES256),
        // `none` / `HS256` / `RS512` / 缺失 —— 一律拒绝（**先于任何验签动作**）。
        _ => Err(IamError::SsoOidcAlgNotAllowed),
    }
}

/// 验签 + 校验 claims。
///
/// `keys` 是 `kid → 公钥`（**只来自 `jwks_uri`**）。
/// `kid` 未知时返回 [`IamError::SsoOidcTokenInvalid`]，调用方应**强制刷新一次 JWKS** 再试。
pub fn verify_id_token(
    token: &str,
    keys: &BTreeMap<String, DecodingKey>,
    issuer: &str,
    audience: &str,
) -> Result<IdTokenClaims, IamError> {
    verify_id_token_with(token, keys, issuer, audience, None)
}

/// 带 `nonce` 校验的验签（`expected_nonce = None` 时跳过 nonce 比对）。
pub fn verify_id_token_with(
    token: &str,
    keys: &BTreeMap<String, DecodingKey>,
    issuer: &str,
    audience: &str,
    expected_nonce: Option<&str>,
) -> Result<IdTokenClaims, IamError> {
    let header = decode_jwt_header(token)?;
    let algorithm = allowed_algorithm(header.alg.as_deref())?;

    let key = match header.kid.as_deref() {
        Some(kid) => keys.get(kid).ok_or(IamError::SsoOidcTokenInvalid)?,
        // 没有 kid：退化为「唯一一把钥匙」的情形（单密钥 IdP）。
        None if keys.len() == 1 => keys.values().next().expect("长度已判"),
        None => return Err(IamError::SsoOidcTokenInvalid),
    };

    let mut validation = Validation::new(algorithm);
    // ⚠️ 显式声明**这一个**算法：**绝不** `Algorithm::from_str(header.alg)`。
    //
    // 为什么不是 `ALLOWED_ALGORITHMS.to_vec()`：`jsonwebtoken` 10.4 的
    // `verify_signature_body` 要求 `algorithms` 里**每一个**算法与验签器的
    // **family 相同**（RS256=RSA / ES256=EC）—— 混合列表会被判成
    // `InvalidAlgorithm`。白名单的准入判定在 [`allowed_algorithm`]（硬编码
    // `{RS256, ES256}`，`none` / `HS*` / `RS512` 在那里就被拒），
    // 这里再把**实际使用的那一个**钉死，两道关都在。
    validation.algorithms = vec![algorithm];
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    // skew 必须**显式**给（隐式大值等于放宽过期判定）。
    validation.leeway = CLOCK_SKEW_SECONDS;
    validation.validate_exp = true;

    let decoded = jsonwebtoken::decode::<IdTokenClaims>(token, key, &validation).map_err(
        |error| match error.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => IamError::SsoOidcTokenExpired,
            jsonwebtoken::errors::ErrorKind::InvalidAlgorithm => IamError::SsoOidcAlgNotAllowed,
            _ => IamError::SsoOidcTokenInvalid,
        },
    )?;

    let claims = decoded.claims;

    // `iat` 不得在未来（带 skew）。
    if let Some(iat) = claims.iat {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        if iat > now + CLOCK_SKEW_SECONDS as i64 {
            return Err(IamError::SsoOidcTokenInvalid);
        }
    }

    if let Some(expected) = expected_nonce {
        if claims.nonce.as_deref() != Some(expected) {
            return Err(IamError::SsoOidcNonceMismatch);
        }
    }

    if claims.sub.trim().is_empty() {
        return Err(IamError::SsoOidcTokenInvalid);
    }
    Ok(claims)
}

// ---------------------------------------------------------------------------
// 换 token
// ---------------------------------------------------------------------------

/// 换到的 token 集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenResponse {
    /// `id_token`。
    pub id_token: String,
}

/// 用 `code` + `code_verifier` 换 token。
///
/// 错误分层（AC-S-020 步骤 3/4）：
/// - 传输失败 / 5xx / 超时 → `SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE`（502）
/// - 4xx → `SSO_OIDC_TOKEN_EXCHANGE_FAILED`（400）
pub async fn exchange_code(
    http: &OidcHttpClient,
    token_endpoint: &str,
    settings: &OidcSettings,
    client_secret: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, IamError> {
    let form = vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("code".to_string(), code.to_string()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        ("client_id".to_string(), settings.client_id.clone()),
        ("client_secret".to_string(), client_secret.to_string()),
        ("code_verifier".to_string(), code_verifier.to_string()),
    ];
    let (status, body) = http
        .post_form(token_endpoint, &form)
        .await
        .map_err(|_| IamError::SsoOidcTokenEndpointUnavailable)?;
    if (500..600).contains(&status) {
        return Err(IamError::SsoOidcTokenEndpointUnavailable);
    }
    if !(200..300).contains(&status) {
        return Err(IamError::SsoOidcTokenExchangeFailed);
    }
    let Some(body) = body else {
        return Err(IamError::SsoOidcTokenExchangeFailed);
    };
    let id_token = body
        .get("id_token")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(IamError::SsoOidcTokenExchangeFailed)?;
    Ok(TokenResponse {
        id_token: id_token.to_string(),
    })
}

// ---------------------------------------------------------------------------
// state 的读取与消费（跨租户校验在应用层做）
// ---------------------------------------------------------------------------

/// **回调时的租户归属**（P3 / S12）。
///
/// 浏览器从 IdP 跳回 `/callback` 带不上 `x-tenant-id`。规则：
/// - 请求头**有**租户 → 必须与 `state.tenant_id` 相等，不等 → `400 SSO_OIDC_STATE_INVALID`
/// - 请求头**无**租户 → 采纳 `state.tenant_id`
pub fn resolve_callback_tenant(
    header_tenant: Option<&str>,
    state_tenant: &str,
) -> Result<String, IamError> {
    match header_tenant {
        Some(tenant) if tenant != state_tenant => Err(IamError::SsoOidcStateInvalid),
        _ => Ok(state_tenant.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636 附录 B 的测试向量。
    #[test]
    fn pkce_challenge_matches_the_rfc7636_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            code_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn state_and_nonce_satisfy_the_shape_and_do_not_repeat() {
        use std::collections::BTreeSet;
        let mut seen = BTreeSet::new();
        for _ in 0..20 {
            let token = random_token();
            assert!(token.len() >= 43, "state 太短：{token}");
            assert!(
                token
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'),
                "state 只允许 [A-Za-z0-9_-]：{token}"
            );
            assert!(seen.insert(token), "20 次内出现重复 state");
        }
    }

    #[test]
    fn authorization_url_carries_every_required_parameter() {
        let settings = OidcSettings {
            enabled: true,
            issuer: "http://127.0.0.1:1".to_string(),
            client_id: "client-1".to_string(),
            scopes: "openid email profile".to_string(),
            redirect_uri: "http://app.example.com/api/iam/sso/oidc/callback".to_string(),
        };
        let url = authorization_url(
            "http://idp.example.com/authorize",
            &settings,
            "STATE",
            "NONCE",
            "CHALLENGE",
        );
        for expected in [
            "response_type=code",
            "client_id=client-1",
            "redirect_uri=http%3A%2F%2Fapp.example.com%2Fapi%2Fiam%2Fsso%2Foidc%2Fcallback",
            "scope=openid%20email%20profile",
            "state=STATE",
            "nonce=NONCE",
            "code_challenge=CHALLENGE",
            "code_challenge_method=S256",
        ] {
            assert!(url.contains(expected), "授权 URL 缺少 {expected}：{url}");
        }
    }

    /// 白名单外（含 `none` / `HS256` / `RS512`）必须**先拒**。
    #[test]
    fn algorithm_whitelist_rejects_none_hs_and_rs512() {
        assert_eq!(
            allowed_algorithm(Some("RS256")).expect("RS256 应允许"),
            Algorithm::RS256
        );
        assert_eq!(
            allowed_algorithm(Some("ES256")).expect("ES256 应允许"),
            Algorithm::ES256
        );
        for rejected in [Some("none"), Some("HS256"), Some("RS512"), None] {
            assert_eq!(
                allowed_algorithm(rejected),
                Err(IamError::SsoOidcAlgNotAllowed),
                "算法 {rejected:?} 必须被拒"
            );
        }
    }

    /// 回调租户规则（P3）。
    #[test]
    fn callback_tenant_must_match_or_be_absent() {
        assert_eq!(
            resolve_callback_tenant(None, "tenant-a").expect("无头时应采纳 state 租户"),
            "tenant-a"
        );
        assert_eq!(
            resolve_callback_tenant(Some("tenant-a"), "tenant-a").expect("相同应通过"),
            "tenant-a"
        );
        assert_eq!(
            resolve_callback_tenant(Some("tenant-b"), "tenant-a"),
            Err(IamError::SsoOidcStateInvalid)
        );
    }

    fn unsigned_token(header: serde_json::Value, payload: serde_json::Value) -> String {
        format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).expect("header")),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("payload")),
            URL_SAFE_NO_PAD.encode(b"signature")
        )
    }

    fn claims_payload() -> serde_json::Value {
        serde_json::json!({
            "iss": "http://idp.example.com",
            "sub": "idp-bob-001",
            "aud": "client-1",
            "exp": time::OffsetDateTime::now_utc().unix_timestamp() + 300,
            "iat": time::OffsetDateTime::now_utc().unix_timestamp(),
            "nonce": "nonce-1",
            "email": "bob@example.com",
        })
    }

    #[test]
    fn alg_none_is_rejected_before_any_signature_work() {
        let token = unsigned_token(serde_json::json!({"alg": "none"}), claims_payload());
        let keys: BTreeMap<String, DecodingKey> = BTreeMap::new();
        assert_eq!(
            verify_id_token(&token, &keys, "http://idp.example.com", "client-1"),
            Err(IamError::SsoOidcAlgNotAllowed)
        );
    }

    #[test]
    fn hs256_is_rejected_before_any_signature_work() {
        let token = unsigned_token(
            serde_json::json!({"alg": "HS256", "kid": "k1"}),
            claims_payload(),
        );
        let keys: BTreeMap<String, DecodingKey> = BTreeMap::new();
        assert_eq!(
            verify_id_token(&token, &keys, "http://idp.example.com", "client-1"),
            Err(IamError::SsoOidcAlgNotAllowed)
        );
    }

    /// header 里带 `jku` / `x5c` 时**也不解密取** —— 解析 header 不产生任何网络行为。
    #[test]
    fn jku_and_x5c_are_ignored_by_header_parsing() {
        let header = serde_json::json!({
            "alg": "RS256",
            "kid": "k1",
            "jku": "https://attacker.example.com/jwks.json",
            "x5c": ["AAAA"],
        });
        let token = unsigned_token(header, claims_payload());
        let parsed = decode_jwt_header(&token).expect("header 应能解析");
        assert_eq!(parsed.alg.as_deref(), Some("RS256"));
        assert_eq!(parsed.kid.as_deref(), Some("k1"));
        // 未知 kid → 交给调用方刷新；这里直接是 TOKEN_INVALID。
        let keys: BTreeMap<String, DecodingKey> = BTreeMap::new();
        assert_eq!(
            verify_id_token(&token, &keys, "http://idp.example.com", "client-1"),
            Err(IamError::SsoOidcTokenInvalid)
        );
    }

    #[test]
    fn percent_encoding_escapes_reserved_characters() {
        assert_eq!(percent_encode("a b/c?d=e&f"), "a%20b%2Fc%3Fd%3De%26f");
        assert_eq!(percent_encode("openid"), "openid");
    }
}
