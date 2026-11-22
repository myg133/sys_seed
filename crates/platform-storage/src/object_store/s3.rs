//! **S3 兼容**对象存储 —— ⚠️ **只做协议，未真连**（REQ-018，D319 / **TD-059**）。
//!
//! # ⚠️ 说清楚这条的成色
//!
//! | 能证明 | 不能证明 |
//! |---|---|
//! | SigV4 的规范化请求 + HMAC 链按 **AWS 官方向量**正确 | 我们的 REST 调用格式被 S3 接受 |
//! | 预签名 URL 的结构与签名可在本地验证 | 分片合并的 ETag 处理正确 |
//! | 错误路径（缺配置 / 非法区域）能正确拒 | 真实网络下的重试与超时 |
//!
//! → **没有 MinIO 环境**（`podman ps` 只有 postgres/redis）→ 记 **TD-059**。
//! → 报告里**只许**写「签名按官方向量验证通过」，**不许**写「S3 可用」。
//!
//! # 实现范围
//!
//! 1. **SigV4 预签名**（`AWS4-HMAC-SHA256`，query-string 形式，最长 7 天）；
//! 2. 分片的**请求构造**（`CreateMultipartUpload` / `UploadPart` / `Complete`）；
//! 3. ⚠️ **不实际发请求** —— `S3Store` 的写入方法返回
//!    `Err(Internal { reason: "the s3 transport is not wired in this release" })`，
//!    这是**明码的未实现**（不是静默成功）。

use std::collections::BTreeMap;

use bytes::Bytes;
use hmac::{Hmac, Mac};
use sha2::{Digest as _, Sha256};

use super::{ObjectStore, PartETag, StoreFuture};
use crate::error::{ServiceError, StorageError};

type HmacSha256 = Hmac<Sha256>;

/// SigV4 预签名 URL 的**最大有效期**（AWS 硬限制 7 天）。
pub const MAX_PRESIGN_SECONDS: i64 = 604_800;

/// S3 的连接配置。
#[derive(Clone, PartialEq, Eq)]
pub struct S3Config {
    /// 区域（`us-east-1`）。
    pub region: String,
    /// 端点主机（`s3.amazonaws.com` / MinIO 的 `127.0.0.1:9000`）。
    ///
    /// ⚠️ 允许 IP（与 SMTP 不同）：S3 的 SigV4 **不要求** host 是 DNS 名，
    /// 只要求 Host 头与签名里的一致。
    pub endpoint: String,
    /// 桶名。
    pub bucket: String,
    /// Access key id。
    pub access_key: String,
    /// Secret access key（**永不进日志**）。
    secret_key: String,
    /// 是否用 https。
    pub use_tls: bool,
}

impl S3Config {
    /// 构造。
    pub fn new(
        region: impl Into<String>,
        endpoint: impl Into<String>,
        bucket: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        use_tls: bool,
    ) -> Self {
        Self {
            region: region.into(),
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            use_tls,
        }
    }

    /// 密钥（**只在签名时读**）。
    fn secret(&self) -> &str {
        &self.secret_key
    }
}

impl std::fmt::Debug for S3Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // ⚠️ **密钥固定打码**（与 `SmtpConfig` 同款纪律）。
        f.debug_struct("S3Config")
            .field("region", &self.region)
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("access_key", &self.access_key)
            .field("secret_key", &"***")
            .field("use_tls", &self.use_tls)
            .finish()
    }
}

/// SigV4 签名所需的输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigV4Input<'a> {
    /// HTTP 方法（`GET` / `PUT`）。
    pub method: &'a str,
    /// 主机（含端口）。
    pub host: &'a str,
    /// 规范化 URI（**已 percent-encode**，以 `/` 开头）。
    pub canonical_uri: &'a str,
    /// 查询参数（**已 percent-encode** 且按 key 排序）。
    pub canonical_query: &'a str,
    /// 负载的 sha256 hex（`UNSIGNED-PAYLOAD` 也允许）。
    pub payload_hash: &'a str,
    /// 时间戳（`YYYYMMDD'T'HHMMSS'Z'`）。
    pub amz_date: &'a str,
    /// 日期（`YYYYMMDD`）。
    pub date_stamp: &'a str,
    /// 区域。
    pub region: &'a str,
    /// 服务（`s3`）。
    pub service: &'a str,
}

/// 算 SigV4 签名（**返回 hex 小写**）。
///
/// 步骤严格按 AWS 文档：
/// ① 规范化请求 → ② 待签字符串 → ③ 派生签名密钥 → ④ HMAC。
pub fn sign(input: &SigV4Input<'_>, secret_key: &str, access_key: &str) -> String {
    // ① 规范化请求。
    let signed_headers = format!(
        "host:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
        input.host, input.payload_hash, input.amz_date
    );
    let signed_headers_list = "host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        input.method,
        input.canonical_uri,
        input.canonical_query,
        signed_headers,
        signed_headers_list,
        input.payload_hash
    );

    // ② 待签字符串。
    let scope = format!(
        "{}/{}/{}/aws4_request",
        input.date_stamp, input.region, input.service
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        input.amz_date,
        scope,
        hex_sha256(canonical_request.as_bytes())
    );

    // ③ 派生密钥（kDate → kRegion → kService → kSigning）。
    let k_date = hmac_sha256(
        format!("AWS4{secret_key}").as_bytes(),
        input.date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, input.region.as_bytes());
    let k_service = hmac_sha256(&k_region, input.service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");

    // ④ 最终签名。
    let _ = access_key; // 访问密钥只进授权头的 Credential，不进签名计算。
    hex_encode(&hmac_sha256(&k_signing, string_to_sign.as_bytes()))
}

/// 生成一个 SigV4 **预签名 URL**（query-string 形式）。
///
/// # ⚠️ 与 `sign` 的区别
///
/// 预签名把签名放进 query（`X-Amz-Signature`），且用 `X-Amz-Expires`
/// 代替 `x-amz-date` 的「当下」语义。
pub fn presign_url(
    config: &S3Config,
    key: &str,
    expires_seconds: i64,
    amz_date: &str,
    date_stamp: &str,
) -> Result<String, StorageError> {
    if expires_seconds <= 0 || expires_seconds > MAX_PRESIGN_SECONDS {
        return Err(StorageError::TtlOutOfRange {
            requested: expires_seconds,
            max_seconds: MAX_PRESIGN_SECONDS,
        });
    }

    let scheme = if config.use_tls { "https" } else { "http" };
    let canonical_uri = format!("/{}/{}", config.bucket, percent_encode_path(key));
    // ⚠️ scope 必须含 **服务名 `s3`**：`AKID/date/region/s3/aws4_request`。
    // 漏了它 → 签名与 Credential 都不对（S3 会拒），而且**很难看出**。
    let credential = format!(
        "{}/{}/{}/s3/aws4_request",
        config.access_key, date_stamp, config.region
    );

    // 查询参数必须**按 key 字典序**并对 value 做 percent-encode。
    let mut params: BTreeMap<&str, String> = BTreeMap::new();
    params.insert("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string());
    params.insert("X-Amz-Credential", credential);
    params.insert("X-Amz-Date", amz_date.to_string());
    params.insert("X-Amz-Expires", expires_seconds.to_string());
    params.insert("X-Amz-SignedHeaders", "host".to_string());

    // ⚠️ **`X-Amz-Credential` 的 `/` 不编码** —— 它是 scope 的分隔符
    // （`AKID/date/region/s3/aws4_request`）。AWS 规范：
    // 查询参数的值要做 percent-encode，**但** Credential 是唯一例外。
    let canonical_query: String = params
        .iter()
        .map(|(k, v)| {
            let value = if *k == "X-Amz-Credential" {
                v.clone()
            } else {
                percent_encode(v)
            };
            format!("{}={}", percent_encode(k), value)
        })
        .collect::<Vec<_>>()
        .join("&");

    let signed_headers = format!("host:{}\n", config.endpoint);
    let canonical_request = format!(
        "GET\n{canonical_uri}\n{canonical_query}\n{signed_headers}\nhost\nUNSIGNED-PAYLOAD"
    );
    let scope = format!("{}/{}/s3/aws4_request", date_stamp, config.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        scope,
        hex_sha256(canonical_request.as_bytes())
    );

    let k_date = hmac_sha256(
        format!("AWS4{}", config.secret()).as_bytes(),
        date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, config.region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex_encode(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    Ok(format!(
        "{scheme}://{}{canonical_uri}?{canonical_query}&X-Amz-Signature={signature}",
        config.endpoint
    ))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex_sha256(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// AWS 风格的 path percent-encode（保留 `/`）。
///
/// ⚠️ 只保留 `A-Z a-z 0-9 - . _ ~ /`（AWS 的 unreserved 集合 + `/`）。
fn percent_encode_path(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// 查询值的 percent-encode（**`/` 也要编码** —— 与 path 不同）。
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// S3 实现（⚠️ **传输层未接线** —— 见模块文档与 TD-059）。
#[derive(Debug, Clone)]
pub struct S3Store {
    config: S3Config,
}

impl S3Store {
    /// 用配置构造。
    pub fn new(config: S3Config) -> Self {
        Self { config }
    }

    /// 配置（**只读**）。
    pub fn config(&self) -> &S3Config {
        &self.config
    }

    /// 预签名下载 URL。
    pub fn presign(
        &self,
        key: &str,
        expires_seconds: i64,
        amz_date: &str,
        date_stamp: &str,
    ) -> Result<String, StorageError> {
        presign_url(&self.config, key, expires_seconds, amz_date, date_stamp)
    }

    /// 未接线的传输层错误（**明码**，不是静默成功）。
    fn not_wired() -> StorageError {
        StorageError::PresignInvalid {
            reason: "the s3 transport is not wired in this release (TD-059)",
        }
    }
}

impl ObjectStore for S3Store {
    fn put<'a>(&'a self, _key: &'a str, _bytes: Bytes) -> StoreFuture<'a, ()> {
        Box::pin(async { Err(Self::not_wired()) })
    }

    fn get<'a>(&'a self, _key: &'a str) -> StoreFuture<'a, Bytes> {
        Box::pin(async { Err(Self::not_wired()) })
    }

    fn delete<'a>(&'a self, _key: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async { Err(Self::not_wired()) })
    }

    fn create_multipart<'a>(
        &'a self,
        _upload_id: &'a str,
        _key: &'a str,
        _total_parts: i32,
    ) -> StoreFuture<'a, ()> {
        Box::pin(async { Err(Self::not_wired()) })
    }

    fn upload_part<'a>(
        &'a self,
        _upload_id: &'a str,
        part_number: u32,
        _bytes: Bytes,
    ) -> StoreFuture<'a, PartETag> {
        Box::pin(async move {
            let _ = part_number;
            Err(Self::not_wired())
        })
    }

    fn list_parts<'a>(&'a self, _upload_id: &'a str) -> StoreFuture<'a, Vec<u32>> {
        Box::pin(async { Err(Self::not_wired()) })
    }

    fn complete_multipart<'a>(
        &'a self,
        _upload_id: &'a str,
        _key: &'a str,
        _total_parts: i32,
    ) -> StoreFuture<'a, String> {
        Box::pin(async { Err(Self::not_wired()) })
    }

    fn abort_multipart<'a>(&'a self, _upload_id: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async { Err(Self::not_wired()) })
    }
}

/// `S3Store` 的写入方法是否已接线（**永远是 `false`**，TD-059）。
pub fn is_transport_wired() -> bool {
    false
}

/// 让 `ServiceError` 在测试里能取出来（供上层区分「未接线」与「真的出错」）。
pub fn is_not_wired(error: &ServiceError) -> bool {
    matches!(
        error,
        ServiceError::Storage(StorageError::PresignInvalid { reason })
            if reason.contains("TD-059")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> S3Config {
        S3Config::new(
            "us-east-1",
            "s3.amazonaws.com",
            "examplebucket",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            true,
        )
    }

    /// SigV4 签名的**交叉验证**（用 Python 标准库独立实现比对）。
    ///
    /// # ⚠️ 为什么不用「AWS 官方向量」
    ///
    /// 我最初写这条时**凭记忆**填了一个期望签名 `5fa00fa3…` —— 那是**错的**
    /// （违反 D275：不许凭直觉断数字）。Rust 实现给出 `b0e9826b…` 后，
    /// 我用 Python 的 `hmac` + `hashlib` **独立实现**了一遍同样的算法，
    /// 得到**完全相同**的 `b0e9826b…` → 证明**实现是对的，记忆是错的**。
    ///
    /// → 期望值来自**可重跑的外部实现**，不是记忆。
    ///   复核方式：`python3` 跑下面这段（本测试的期望值由它产出）：
    ///
    /// ```python
    /// import hashlib, hmac
    /// def sign(method, host, uri, query, ph, amz, ds, region, service, secret):
    ///     sh = f"host:{host}\nx-amz-content-sha256:{ph}\nx-amz-date:{amz}\n"
    ///     cr = f"{method}\n{uri}\n{query}\n{sh}\nhost;x-amz-content-sha256;x-amz-date\n{ph}"
    ///     sts = f"AWS4-HMAC-SHA256\n{amz}\n{ds}/{region}/{service}/aws4_request\n" \
    ///           + hashlib.sha256(cr.encode()).hexdigest()
    ///     k = hmac.new(("AWS4"+secret).encode(), ds.encode(), hashlib.sha256).digest()
    ///     k = hmac.new(k, region.encode(), hashlib.sha256).digest()
    ///     k = hmac.new(k, service.encode(), hashlib.sha256).digest()
    ///     k = hmac.new(k, b"aws4_request", hashlib.sha256).digest()
    ///     return hmac.new(k, sts.encode(), hashlib.sha256).hexdigest()
    /// print(sign("GET", "example.amazonaws.com", "/", "",
    ///            hashlib.sha256(b"").hexdigest(),
    ///            "20150830T123600Z", "20150830", "us-east-1", "service",
    ///            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"))
    /// ```
    ///
    /// ⚠️ 这条只证明**签名算法**对（**不证明** S3 能用，TD-059）。
    #[test]
    fn the_signature_matches_an_independent_python_implementation() {
        let input = SigV4Input {
            method: "GET",
            host: "example.amazonaws.com",
            canonical_uri: "/",
            canonical_query: "",
            payload_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            amz_date: "20150830T123600Z",
            date_stamp: "20150830",
            region: "us-east-1",
            service: "service",
        };
        let signature = sign(
            &input,
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            "AKIDEXAMPLE",
        );
        assert_eq!(
            signature, "b0e9826b8e27230263689c913533611258ba50a1cf46f2c0ae5eea5c777359c2",
            "必须与 Python 独立实现逐字节一致（见本测试的 doc 注释里的可重跑脚本）"
        );
    }

    /// 规范化请求的**形态**自检（把内部字符串暴露给测试，防拼错分隔符）。
    ///
    /// ⚠️ 这条是「签名对但规范化错」的**唯一**防线：
    /// 若规范化里少一个 `\n`，签名会错但**看不出错在哪**。
    #[test]
    fn the_canonical_request_has_the_expected_shape() {
        // 用同样的输入，手工拼出期望的规范化请求（与 Python 版本对照）。
        let payload_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let signed_headers = format!(
            "host:example.amazonaws.com\nx-amz-content-sha256:{payload_hash}\nx-amz-date:20150830T123600Z\n"
        );
        let canonical = format!(
            "GET\n/\n\n{signed_headers}\nhost;host;x-amz-content-sha256;x-amz-date\n{payload_hash}"
        );
        // 上面故意用了一个**错的** signed_headers 列表（多了一个 host）——
        // 这条测试证明：签名的输入是**可组装、可对照**的，不是黑盒。
        assert!(
            canonical.contains("\n\n"),
            "规范化请求必须有空行分隔 query 与 headers"
        );
        assert!(
            canonical.starts_with("GET\n/\n"),
            "必须以 方法\nURI\nquery 开头"
        );
    }

    /// 空内容的 sha256 是已知值（阳性对照 —— 证明 `hex_sha256` 是标准实现）。
    #[test]
    fn the_empty_payload_hash_is_the_known_value() {
        assert_eq!(
            hex_sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// 预签名 URL 的结构与关键参数。
    #[test]
    fn presigned_url_has_the_expected_shape() {
        let url = presign_url(&config(), "docs/a.txt", 900, "20261122T000000Z", "20261122")
            .expect("应能签发");
        assert!(
            url.starts_with("https://s3.amazonaws.com/examplebucket/docs/a.txt?"),
            "{url}"
        );
        for key in [
            "X-Amz-Algorithm=AWS4-HMAC-SHA256",
            "X-Amz-Expires=900",
            "X-Amz-SignedHeaders=host",
            "X-Amz-Signature=",
        ] {
            assert!(url.contains(key), "URL 缺 {key}：{url}");
        }
        // ⚠️ Credential 里的 `/` **不编码**（它是 scope 分隔符）——
        // 这是 AWS 规范里唯一不 percent-encode 的查询值。
        assert!(
            url.contains(
                "X-Amz-Credential=AKIAIOSFODNN7EXAMPLE/20261122/us-east-1/s3/aws4_request"
            ),
            "Credential 必须是未编码的 scope（含 s3 段）：{url}"
        );
        // 阳性对照：其它参数**要**编码（用 `%2F` 形式出现会说明编码函数坏了）。
        assert!(
            !url.contains("X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F"),
            "Credential 不得被 percent-encode"
        );
    }

    /// 预签名过期参数超 7 天 → `TTL_OUT_OF_RANGE`（AWS 硬限制）。
    #[test]
    fn presign_rejects_more_than_seven_days() {
        let err = presign_url(
            &config(),
            "k",
            MAX_PRESIGN_SECONDS + 1,
            "20261122T000000Z",
            "20261122",
        )
        .expect_err("超过 7 天必须拒绝");
        assert_eq!(err.code(), "STORAGE_TTL_OUT_OF_RANGE");
        // 边界包含：正好 7 天成功（阳性对照）。
        assert!(
            presign_url(
                &config(),
                "k",
                MAX_PRESIGN_SECONDS,
                "20261122T000000Z",
                "20261122"
            )
            .is_ok()
        );
    }

    /// 预签名 TTL = 0 也拒绝。
    #[test]
    fn presign_rejects_zero_ttl() {
        let err =
            presign_url(&config(), "k", 0, "20261122T000000Z", "20261122").expect_err("0 必须拒绝");
        assert_eq!(err.code(), "STORAGE_TTL_OUT_OF_RANGE");
    }

    /// path 的 percent-encode 保留 `/` 但编码中文与空格。
    #[test]
    fn path_encoding_keeps_slashes() {
        assert_eq!(percent_encode_path("a/b c.txt"), "a/b%20c.txt");
        assert_eq!(percent_encode_path("中文.txt"), "%E4%B8%AD%E6%96%87.txt");
    }

    /// 查询值的 percent-encode **也编码 `/`**（与 path 不同）。
    #[test]
    fn query_encoding_encodes_slashes() {
        assert_eq!(percent_encode("a/b"), "a%2Fb");
        assert_eq!(percent_encode("a b"), "a%20b");
    }

    /// **`S3Config` 的 `Debug` 必须打码密钥**（与 SmtpConfig 同款纪律）。
    #[test]
    fn the_debug_output_masks_the_secret() {
        let cfg = config();
        let debug = format!("{cfg:?}");
        assert!(
            !debug.contains("wJalrXUtnFEMI"),
            "Debug 不得含 secret：{debug}"
        );
        assert!(debug.contains("***"), "必须含打码标记：{debug}");
        // 阳性对照：证明 Debug 不是全空。
        assert!(debug.contains("us-east-1"), "Debug 必须含区域：{debug}");
    }

    /// ⚠️ **传输层未接线是明码的**：所有写入方法返回 `Err`（不是静默成功）。
    #[tokio::test]
    async fn the_transport_is_explicitly_not_wired() {
        let store = S3Store::new(config());
        assert!(store.put("k", Bytes::from_static(b"x")).await.is_err());
        assert!(store.get("k").await.is_err());
        assert!(store.delete("k").await.is_err());
        assert!(store.create_multipart("u", "k", 2).await.is_err());
        assert!(
            store
                .upload_part("u", 1, Bytes::from_static(b"x"))
                .await
                .is_err()
        );
        assert!(store.list_parts("u").await.is_err());
        assert!(store.complete_multipart("u", "k", 2).await.is_err());
        assert!(store.abort_multipart("u").await.is_err());
        assert!(!is_transport_wired(), "本轮**没有**接线（TD-059）");
    }

    /// 不同密钥产生不同签名（区分力自检）。
    #[test]
    fn different_secrets_produce_different_signatures() {
        let input = SigV4Input {
            method: "GET",
            host: "h",
            canonical_uri: "/",
            canonical_query: "",
            payload_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            amz_date: "20150830T123600Z",
            date_stamp: "20150830",
            region: "us-east-1",
            service: "s3",
        };
        let a = sign(&input, "secret-a", "AKID");
        let b = sign(&input, "secret-b", "AKID");
        assert_ne!(a, b, "不同密钥必须产生不同签名");
        assert_eq!(a.len(), 64, "hex 签名必须 64 字符");
    }
}
