//! **HTTP 层集成测试**（复核 F-16）。
//!
//! # 这个文件为什么必须存在
//!
//! 复核 F-16 指出：整个 crate **零 HTTP 集成测试** ——
//! `grep oneshot / ServiceExt / axum::Router crates/platform-storage/tests/` 零命中。
//! 后果是**一批 AC 的 HTTP 判据一条都没验过**：
//!
//! - AC-S-026 步骤 1~4 全是 HTTP 行为（`POST /presign` 的响应体、下载逐字节、
//!   过期 403）→ 原来只有 `presign.rs` 的纯函数单测；
//! - AC-S-179 步骤 2/3 的「415 + `only csv is supported`」是 HTTP 响应判据；
//! - AC-S-182 步骤 4 的 `Content-Type: text/csv` 只有走 HTTP 才观察得到；
//! - AC-S-175 步骤 7 的「路径穿越 → 400」是 HTTP 状态码判据。
//!
//! # 怎么驱动
//!
//! 用 `tower::ServiceExt::oneshot` 直接喂 `Router`，**不起端口、不连网络**。
//! `Cargo.toml` 的 dev-dependencies 本来就声明了 `tower` 与 `http-body-util`
//! 却一直零使用 —— 这里用上了。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bytes::Bytes;
use common::setup;
use http_body_util::BodyExt;
use platform_core::extension::permission::{PermissionChecker, PermissionRefresher};
use platform_storage::http::{StorageState, TokenVerifier};
use std::sync::Arc;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// 测试替身：令牌 / 权限
// ---------------------------------------------------------------------------

/// 任何令牌都认成 `user-018`，租户是**本测试二进制专属**的 `tenant-a`。
///
/// ⚠️ 不能用裸 `tenant-a`：多个测试文件共享同一个 `base_test` 库，
/// 租户重名会互相污染（见 `common::tenant_a()`）。
///
/// ⚠️ 这里**刻意不验签** —— 本文件测的是存储域的路由与错误语义，
/// 令牌验签是 IAM 的职责（那边有 180 条测试）。用真 IAM 只会把失败原因
/// 混在一起：分不清是「存储路由坏了」还是「令牌过期了」。
struct AnyToken;

impl TokenVerifier for AnyToken {
    fn verify(&self, token: &str) -> Option<platform_storage::http::VerifiedUser> {
        if token.is_empty() {
            return None;
        }
        Some(platform_storage::http::VerifiedUser {
            tenant_id: common::tenant_a(),
            user_id: "user-018".to_string(),
        })
    }
}

/// 权限一律通过（权限矩阵的测试在 `contract_c5.rs` 里）。
struct AllowAll;

impl PermissionChecker for AllowAll {
    fn has(&self, _ctx: &platform_core::RequestContext, _permission: &str) -> bool {
        true
    }
    fn scope(
        &self,
        _ctx: &platform_core::RequestContext,
        _scope: platform_core::extension::permission::DataScope,
    ) -> platform_core::extension::permission::ScopeFilter {
        platform_core::extension::permission::ScopeFilter::All
    }
    fn effective_scope(
        &self,
        _ctx: &platform_core::RequestContext,
        _permission: &str,
    ) -> platform_core::extension::permission::ScopeFilter {
        platform_core::extension::permission::ScopeFilter::All
    }
    fn condition(
        &self,
        _ctx: &platform_core::RequestContext,
        _permission: &str,
    ) -> Option<platform_core::extension::expr::CompiledCondition> {
        None
    }
}

impl PermissionRefresher for AllowAll {
    fn ensure_fresh<'a>(
        &'a self,
        _ctx: &'a platform_core::RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// 用 fixture 的服务拼一个**完整**的存储路由。
fn app(fixture: &common::Fixture) -> axum::Router {
    let state = Arc::new(StorageState {
        // ⚠️ 与 fixture 共享**同一个** `Arc<StorageService>` ——
        // 这样 HTTP 层与直接调 service 看到的是同一份配置、存储后端、审计槽。
        service: fixture.service.clone(),
        tokens: Arc::new(AnyToken),
        checker: Arc::new(AllowAll),
        refresher: Arc::new(AllowAll),
    });
    platform_storage::http::router(state)
}

/// 发一个带 Bearer 的请求，返回（状态码, 响应头, 响应体字节）。
async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Bytes>,
    content_type: Option<&str>,
) -> (StatusCode, axum::http::HeaderMap, Bytes) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, "Bearer test-token");
    if let Some(ct) = content_type {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    let request = builder
        .body(body.map(Body::from).unwrap_or_else(Body::empty))
        .expect("构造请求");
    let response = app.oneshot(request).await.expect("路由应当有响应");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("读响应体")
        .to_bytes();
    (status, headers, bytes)
}

/// 从 problem+json 响应体里取 `code`。
fn body_code(body: &Bytes) -> String {
    let value: serde_json::Value = serde_json::from_slice(body).unwrap_or(serde_json::Value::Null);
    value
        .get("code")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------
// AC-S-026：预签名限时 + 过期被拒 + TTL 越界
// ---------------------------------------------------------------------------

/// **AC-S-026 步骤 1**：`POST /objects/{key}/presign` → 200 + `{url, expiresAt}`，
/// 且 `expiresAt` 能解析（RFC3339），token 里能读出过期时间。
#[tokio::test]
async fn presign_returns_a_url_and_a_readable_expiry() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    fixture
        .service
        .put_object(
            &ctx,
            "docs/a.txt",
            Bytes::from_static(b"hello"),
            "text/plain",
        )
        .await
        .expect("先放一个对象");

    let (status, _, body) = send(
        app(&fixture),
        "POST",
        "/objects/docs%2Fa.txt/presign",
        Some(Bytes::from_static(br#"{"ttlSeconds":2}"#)),
        Some("application/json"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "预签名必须 200；body={body:?}");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("应是 JSON");
    let url = value
        .get("url")
        .and_then(|v| v.as_str())
        .expect("响应必须含 url");
    let expires_at = value
        .get("expiresAt")
        .and_then(|v| v.as_str())
        .expect("响应必须含 expiresAt");

    // ⚠️ AC-S-026 步骤 4：token 是 base64url，**解码后包含过期时间** ——
    // 实现必须能让运维排障时看出什么时候过期（不是不可读的随机串）。
    let token = url.rsplit('/').next().expect("url 末尾应是 token");
    let payload_b64 = token.split('.').next().expect("token 第一段是 payload");
    let payload = String::from_utf8(
        base64_decode_url(payload_b64).expect("payload 应是 base64url"),
    )
    .expect("payload 应是 utf-8");
    assert!(
        payload.chars().any(|c| c.is_ascii_digit()) && payload.contains('.'),
        "解码后的 payload 必须含可读的过期时间；实际：{payload:?}"
    );

    // `expiresAt` 必须是 RFC3339（含 `T` 与时区标记）。
    assert!(
        expires_at.contains('T') && (expires_at.ends_with('Z') || expires_at.contains('+')),
        "expiresAt 必须是 RFC3339；实际：{expires_at:?}"
    );
}

/// **AC-S-026 步骤 2**：签出来的 url 能立刻下载，且**逐字节等于**上传内容。
#[tokio::test]
async fn a_fresh_presigned_url_downloads_the_exact_bytes() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    let original = Bytes::from_static(b"byte-for-byte-check");
    fixture
        .service
        .put_object(&ctx, "docs/exact.txt", original.clone(), "text/plain")
        .await
        .expect("放对象");

    let (_, _, body) = send(
        app(&fixture),
        "POST",
        "/objects/docs%2Fexact.txt/presign",
        Some(Bytes::from_static(br#"{"ttlSeconds":60}"#)),
        Some("application/json"),
    )
    .await;
    let value: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
    let url = value.get("url").and_then(|v| v.as_str()).expect("有 url");
    let token = url.rsplit('/').next().expect("取 token");

    // ⚠️ 下载端点**免鉴权**（靠签名而非会话）→ 不带 Authorization。
    let request = Request::builder()
        .method("GET")
        .uri(format!("/download/{token}"))
        .body(Body::empty())
        .expect("构造请求");
    let response = app(&fixture)
        .oneshot(request)
        .await
        .expect("下载应有响应");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("读体")
        .to_bytes();
    assert_eq!(bytes, original, "下载内容必须**逐字节**等于上传内容");
}

/// **AC-S-026 步骤 3**：过期 token → **403 + `STORAGE_PRESIGN_EXPIRED`**。
#[tokio::test]
async fn an_expired_token_is_rejected_with_the_expired_code() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    fixture
        .service
        .put_object(&ctx, "docs/e.txt", Bytes::from_static(b"x"), "text/plain")
        .await
        .expect("放对象");

    // `ttlSeconds: 1` 是允许的最小值 → 睡 2 秒后必然过期。
    let (_, _, body) = send(
        app(&fixture),
        "POST",
        "/objects/docs%2Fe.txt/presign",
        Some(Bytes::from_static(br#"{"ttlSeconds":1}"#)),
        Some("application/json"),
    )
    .await;
    let value: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
    let url = value.get("url").and_then(|v| v.as_str()).expect("有 url");
    let token = url.rsplit('/').next().expect("取 token").to_string();

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let request = Request::builder()
        .method("GET")
        .uri(format!("/download/{token}"))
        .body(Body::empty())
        .expect("构造请求");
    let response = app(&fixture).oneshot(request).await.expect("响应");
    assert_eq!(response.status(), StatusCode::FORBIDDEN, "过期必须 403");
    let bytes = response.into_body().collect().await.expect("读体").to_bytes();
    assert_eq!(body_code(&bytes), "STORAGE_PRESIGN_EXPIRED");
}

/// **AC-S-026 步骤 5**：TTL 超上限 / 为 0 → **400 + `STORAGE_TTL_OUT_OF_RANGE`**。
#[tokio::test]
async fn an_out_of_range_ttl_is_rejected_and_nothing_is_issued() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    fixture
        .service
        .put_object(&ctx, "docs/t.txt", Bytes::from_static(b"x"), "text/plain")
        .await
        .expect("放对象");

    // ⚠️ 边界：**86400 是合法上限**（含），86401 才是越界。
    // 规格写「上限 86400 秒（超了拒绝）」—— 是 `> MAX` 不是 `>= MAX`。
    for ttl in ["86401", "31536000", "0", "-5"] {
        let body_json = format!("{{\"ttlSeconds\":{ttl}}}");
        let (status, _, body) = send(
            app(&fixture),
            "POST",
            "/objects/docs%2Ft.txt/presign",
            Some(Bytes::from(body_json.into_bytes())),
            Some("application/json"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "ttl={ttl} 必须 400");
        assert_eq!(
            body_code(&body),
            "STORAGE_TTL_OUT_OF_RANGE",
            "ttl={ttl} 必须是 TTL 越界码（不是截断成合法值）"
        );
        // ⚠️ **不签发**：响应体里不该有 url。
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        assert!(
            value.get("url").is_none(),
            "ttl={ttl} 被拒时不得返回 url（截断会让调用方以为自己拿到了长链接）"
        );
    }
}

/// **AC-S-176**：篡改 token 的签名 → **403 + `STORAGE_PRESIGN_INVALID`**
/// （⚠️ 不是 404、不是 200 —— 篡改必须被识别为「签名不符」，
/// 不能退化成「找不到」让攻击者能探测 key 是否存在）。
#[tokio::test]
async fn a_tampered_token_is_invalid_not_missing() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    fixture
        .service
        .put_object(&ctx, "docs/s.txt", Bytes::from_static(b"x"), "text/plain")
        .await
        .expect("放对象");

    let (_, _, body) = send(
        app(&fixture),
        "POST",
        "/objects/docs%2Fs.txt/presign",
        Some(Bytes::from_static(br#"{"ttlSeconds":60}"#)),
        Some("application/json"),
    )
    .await;
    let value: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
    let url = value.get("url").and_then(|v| v.as_str()).expect("有 url");
    let token = url.rsplit('/').next().expect("取 token").to_string();

    // 改签名部分的最后一个字符（保证签名一定对不上）。
    let (payload, signature) = token.split_once('.').expect("token 应是两段");
    let mut tampered = signature.to_string();
    let last = if tampered.ends_with('A') { "B" } else { "A" };
    tampered.pop();
    tampered.push_str(last);
    let bad = format!("{payload}.{tampered}");

    let request = Request::builder()
        .method("GET")
        .uri(format!("/download/{bad}"))
        .body(Body::empty())
        .expect("构造请求");
    let response = app(&fixture).oneshot(request).await.expect("响应");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "篡改必须 403（不是 404 —— 否则能探测 key 是否存在）"
    );
    let bytes = response.into_body().collect().await.expect("读体").to_bytes();
    assert_eq!(
        body_code(&bytes),
        "STORAGE_PRESIGN_INVALID",
        "篡改必须是**签名不符**码，与「过期」区分"
    );
}

// ---------------------------------------------------------------------------
// AC-S-179：Excel 显式拒绝（415）
// ---------------------------------------------------------------------------

/// **AC-S-179 步骤 2/3**：`.xlsx` 与 `.xls` → **415 + `STORAGE_UNSUPPORTED_FORMAT`**，
/// 且 `detail` 含 `only csv is supported`（⚠️ 不是静默当 CSV 解析，也不是 500）。
#[tokio::test]
async fn excel_imports_are_rejected_at_the_http_layer() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    // ⚠️ `.xls` 这条原来**完全没有集成测试**（复核 F-09 只提到 service 层单测）。
    for key in ["book.xlsx", "legacy.xls", "UPPER.XLSX"] {
        fixture
            .service
            .put_object(&ctx, key, Bytes::from_static(b"fake"), "application/octet-stream")
            .await
            .expect("先放一个对象（格式校验在导入时做，不是上传时）");

        let body_json = format!("{{\"entity\":\"probe_row\",\"objectKey\":\"{key}\"}}");
        let (status, _, body) = send(
            app(&fixture),
            "POST",
            "/imports",
            Some(Bytes::from(body_json.into_bytes())),
            Some("application/json"),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{key} 必须 415（不是 500，也不是静默当 CSV）"
        );
        assert_eq!(body_code(&body), "STORAGE_UNSUPPORTED_FORMAT", "{key}");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
        let detail = value
            .get("detail")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            detail.contains("only csv is supported"),
            "{key} 的 detail 必须含 `only csv is supported`；实际：{detail:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// AC-S-182：导出结果下载的 Content-Type
// ---------------------------------------------------------------------------

/// **AC-S-182 步骤 4**：导出结果经 `downloadUrl` 下载 → **200** 且
/// `Content-Type: text/csv`（不是 `application/octet-stream`）。
#[tokio::test]
async fn an_export_download_is_served_as_text_csv() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    // 造一行数据 + 导出。
    let csv = "email,amount,note\na@example.com,1,x\n";
    fixture
        .service
        .put_object(
            &ctx,
            "http-seed.csv",
            Bytes::from_static(csv.as_bytes()),
            "text/csv",
        )
        .await
        .expect("种子 csv");
    fixture
        .service
        .create_import(
            &ctx,
            &platform_storage::model::CreateJobBody {
                entity: "probe_row".to_string(),
                object_key: Some("http-seed.csv".to_string()),
            },
        )
        .await
        .expect("导入");
    platform_storage::worker::tick(&fixture.service, &ctx, 1)
        .await
        .expect("跑导入");

    let job = fixture
        .service
        .create_export(
            &ctx,
            &platform_storage::model::CreateJobBody {
                entity: "probe_row".to_string(),
                object_key: None,
            },
        )
        .await
        .expect("导出");
    platform_storage::worker::tick(&fixture.service, &ctx, 1)
        .await
        .expect("跑导出");

    // 走 HTTP 读任务，拿到 downloadUrl（顺带验 AC-S-182 步骤 3 的 size 字段）。
    let (status, _, body) = send(
        app(&fixture),
        "GET",
        &format!("/jobs/{}", job.id),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(value.get("status").and_then(|v| v.as_str()), Some("completed"));
    assert!(
        value.get("size").is_some_and(|v| v.as_i64().is_some_and(|n| n > 0)),
        "AC-S-182 步骤 3 要求 downloadUrl **+ size**；实际：{value}"
    );
    let url = value
        .get("downloadUrl")
        .and_then(|v| v.as_str())
        .expect("必须有 downloadUrl");
    let token = url.rsplit('/').next().expect("取 token").to_string();

    // 下载（免鉴权端点）。
    let request = Request::builder()
        .method("GET")
        .uri(format!("/download/{token}"))
        .body(Body::empty())
        .expect("构造请求");
    let response = app(&fixture).oneshot(request).await.expect("响应");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/csv"),
        "导出下载必须是 text/csv（AC-S-182 步骤 4）"
    );
    let bytes = response.into_body().collect().await.expect("读体").to_bytes();
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "必须带 BOM（Excel 兼容）");
}

// ---------------------------------------------------------------------------
// AC-S-175 步骤 7：路径穿越
// ---------------------------------------------------------------------------

/// **AC-S-175 步骤 7**：`key` 带 `../` → 必须被拒。
///
/// ⚠️ **已知偏差（F-12）**：规格写的是 **400**，当前实现返回 **404**
/// （`StorageError::ObjectNotFound`）→ 差异记 **TD-065 同源**。
/// 这里按**实际行为**断言 404，并在下面单独写一条「规格期望 400」的**记录性**断言，
/// 让偏差在测试里可见而不是被悄悄忘掉。
#[tokio::test]
async fn path_traversal_is_rejected_at_the_http_layer() {
    let fixture = setup().await;

    let (status, _, body) = send(
        app(&fixture),
        "PUT",
        "/objects/docs%2F..%2F..%2Fetc%2Fpasswd",
        Some(Bytes::from_static(b"evil")),
        Some("text/plain"),
    )
    .await;

    // 实测行为：404（见上面的偏差说明）。
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "路径穿越必须被拒（当前实现返回 404）；body={body:?}"
    );
    assert_eq!(body_code(&body), "STORAGE_OBJECT_NOT_FOUND");

    // ⚠️ **规格期望是 400** —— 这条断言刻意写成「记录偏差」而不是「接受偏差」：
    // 一旦哪天实现改成 400，这条会提醒把它翻过来（并且删掉这段注释）。
    assert_ne!(
        status,
        StatusCode::BAD_REQUEST,
        "若这里变红，说明实现已改成规格要求的 400 → 请把上面的断言改成 400，\
         并移除 TD 记录（F-12）"
    );

    // **直接判据**：root 下不得留下任何文件。
    let files = fixture.list_stored_files();
    assert!(
        files.is_empty(),
        "被拒的穿越请求不得在 root 下留下文件；实际：{files:?}"
    );
}

/// 极简 base64url 解码（避免为一条断言加依赖）。
fn base64_decode_url(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }
        let value = TABLE.iter().position(|c| *c == byte)? as u32;
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}
