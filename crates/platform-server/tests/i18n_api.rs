//! `/api/i18n/*` 的**路由层**行为（REQ-021 / AC-S-090、AC-S-093、AC-S-096、AC-S-097）。
//!
//! 用 `ServiceExt::oneshot` 直接驱动 axum 路由：不连库、不起端口。
//! 三个接缝（令牌校验 / 配置读取 / 汇率快照）用内存实现替换，
//! 但**协商与换算本身走的是生产代码**（`platform_server::i18n` 的公开函数）——
//! 测试与生产不复制逻辑（D85）。
//!
//! 这里验的是**路由层才能验的东西**：认证、请求头/查询参数怎么进协商、
//! 响应体的字段与 `source`、错误体的 HTTP 码与 `code`。
//! 纯函数侧（8 步优先级、舍入、负数对称）在 `src/i18n.rs` 的单元测试里。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use platform_config::error::ConfigError;
use platform_config::http::{TokenVerifier, VerifiedToken};
use platform_core::RequestContext;
use platform_core::extension::config::ConfigSource;
use platform_core::money::{CurrencyCode, FxRate};
use platform_server::i18n::{
    BoxFuture, CONTEXT_PATH, CONVERT_PATH, FxRates, I18nSettings, I18nSettingsSource, I18nState,
    Setting, router,
};
use serde_json::Value;
use tower::ServiceExt;

const TENANT_T1: &str = "tenant-t1";
const TENANT_T2: &str = "tenant-t2";
/// D119：**平台层 + 租户层都配了**的租户（租户层赢）。
const TENANT_T3: &str = "tenant-t3";
/// D119：**只有平台层配了**的租户（平台层生效）。
const TENANT_T4: &str = "tenant-t4";
const USER: &str = "u-1001";

// ---------------------------------------------------------------------------
// 接缝实现（内存）
// ---------------------------------------------------------------------------

/// 令牌校验：非空令牌都有效，但**令牌决定租户**（D119 的用例要切租户）。
struct FakeTokens;

impl TokenVerifier for FakeTokens {
    fn verify(&self, token: &str) -> Result<VerifiedToken, ConfigError> {
        let tenant_id = match token {
            "good" => TENANT_T1,
            "good-t3" => TENANT_T3,
            "good-t4" => TENANT_T4,
            _ => return Err(ConfigError::Unauthenticated),
        };
        Ok(VerifiedToken {
            user_id: USER.to_string(),
            tenant_id: tenant_id.to_string(),
        })
    }
}

/// 配置读取：
///
/// - **T1**：租户层显式配置（原有场景）；
/// - **T2**：谁都没配 → manifest 默认值;
/// - **T3**：平台层与租户层都配了 → 解析器给出**租户层**的值（D119 的第 3 级）;
/// - **T4**：**只有平台层**配了 → 解析器给出**平台层**的值（D119 的第 4 级）。
struct FakeSettings;

fn setting(value: &str, source: ConfigSource, default: &str) -> Setting {
    Setting {
        value: value.to_string(),
        source,
        manifest_default: default.to_string(),
    }
}

impl I18nSettingsSource for FakeSettings {
    fn settings<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> BoxFuture<'a, Result<I18nSettings, ConfigError>> {
        Box::pin(async move {
            match ctx.tenant_id() {
                Some(TENANT_T2) => {
                    return Ok(I18nSettings {
                        locale: Setting::default_only("zh-CN"),
                        timezone: Setting::default_only("Asia/Shanghai"),
                        currency: Setting::default_only("CNY"),
                    });
                }
                // D119：租户层覆盖平台层 → 解析器给出的是**租户层**的值。
                Some(TENANT_T3) => {
                    return Ok(I18nSettings {
                        locale: setting("ja-JP", ConfigSource::Tenant, "zh-CN"),
                        timezone: setting("Europe/London", ConfigSource::Tenant, "Asia/Shanghai"),
                        currency: setting("JPY", ConfigSource::Tenant, "CNY"),
                    });
                }
                // D119：**只有平台层**配了 → 解析器给出的是**平台层**的值。
                Some(TENANT_T4) => {
                    return Ok(I18nSettings {
                        locale: setting("en-US", ConfigSource::Platform, "zh-CN"),
                        timezone: setting(
                            "America/New_York",
                            ConfigSource::Platform,
                            "Asia/Shanghai",
                        ),
                        currency: setting("USD", ConfigSource::Platform, "CNY"),
                    });
                }
                _ => {}
            }
            Ok(I18nSettings {
                locale: setting("en-US", ConfigSource::Tenant, "zh-CN"),
                timezone: setting("America/New_York", ConfigSource::Tenant, "Asia/Shanghai"),
                currency: setting("USD", ConfigSource::Tenant, "CNY"),
            })
        })
    }
}

/// 汇率快照（内存）：键是 `(from, to, as_of)`。
struct FakeRates {
    rows: Vec<FxRate>,
    /// 为 `true` 时查询返回 `Err`（模拟数据访问层故障）。
    fail: bool,
}

impl FxRates for FakeRates {
    fn resolve<'a>(
        &'a self,
        from: CurrencyCode,
        to: CurrencyCode,
        as_of: &'a str,
    ) -> BoxFuture<'a, Result<Option<FxRate>, String>> {
        Box::pin(async move {
            if self.fail {
                return Err("the data access layer failed".to_string());
            }
            Ok(self
                .rows
                .iter()
                .find(|row| row.from == from && row.to == to && row.as_of_rfc3339() == as_of)
                .cloned())
        })
    }
}

fn currency(code: &str) -> CurrencyCode {
    CurrencyCode::parse(code).expect("测试用币种必须是现行 ISO 4217 代码")
}

fn state(rates: FakeRates) -> Arc<I18nState> {
    state_with_locales(rates, &["zh-CN", "en-US"])
}

/// 换一个**已注册语言集合**。
///
/// D119 的用例需要 `ja-JP` 作为租户层的值 —— 三个值
/// （租户层 / 平台层 / manifest 默认）必须**两两不同**，才判得出是哪一层生效。
/// 真实产品只注册了 `zh-CN` / `en-US`（见 `packages/i18n/src/locales/`），
/// 这里多注册一个纯粹是测试替身。
fn state_with_locales(rates: FakeRates, locales: &[&str]) -> Arc<I18nState> {
    Arc::new(I18nState {
        settings: Arc::new(FakeSettings),
        rates: Arc::new(rates),
        tokens: Arc::new(FakeTokens),
        locales: Arc::new(
            locales
                .iter()
                .map(|code| (*code).to_string())
                .collect::<BTreeSet<_>>(),
        ),
    })
}

fn cny_usd_snapshot(id: i64, rate_micros: i64, as_of: &str) -> FxRate {
    FxRate {
        id,
        from: currency("CNY"),
        to: currency("USD"),
        rate_micros,
        as_of: parse_as_of(as_of),
        source: "manual".to_string(),
    }
}

/// 把测试里的 RFC 3339 文本换成 `OffsetDateTime`。
///
/// 测试只用一个**固定**时刻（`2026-01-01T00:00:00Z` = unix 1767225600），
/// 避免为了造夹具引入时间解析（`time` 的 `parsing` 特性没开，见 D114）。
fn parse_as_of(text: &str) -> platform_core::money::OffsetDateTime {
    assert_eq!(text, "2026-01-01T00:00:00Z", "夹具只支持这一个时刻");
    platform_core::money::OffsetDateTime::from_unix_timestamp(1_767_225_600).expect("合法时间")
}

// ---------------------------------------------------------------------------
// 请求工具
// ---------------------------------------------------------------------------

async fn call(app: axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.oneshot(request).await.expect("路由应当能处理请求");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("应当能读取响应体")
        .to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            panic!(
                "响应体不是 JSON（{err}）：{}",
                String::from_utf8_lossy(&bytes)
            )
        })
    };
    (status, body)
}

fn context_request(query: &str, headers: &[(&str, &str)], token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("{CONTEXT_PATH}{query}"));
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).expect("请求应能构造")
}

fn convert_request(query: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("{CONVERT_PATH}{query}"));
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).expect("请求应能构造")
}

/// 响应体的三个文案字段都不得含 CJK（AC-S-088 的运行时判据）。
fn assert_no_cjk(body: &Value, context: &str) {
    for field in ["title", "detail", "type", "instance"] {
        let text = body.get(field).and_then(Value::as_str).unwrap_or_default();
        assert!(
            !text.chars().any(is_cjk),
            "{context}：`{field}` 不得含 CJK：{text}"
        );
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3000..=0x303F | 0xFF00..=0xFFEF)
}

// ---------------------------------------------------------------------------
// 观测通道：/api/i18n/context
// ---------------------------------------------------------------------------

/// 无令牌 → 401，且错误体是 `UNAUTHENTICATED`（不是「缺权限」）。
#[tokio::test]
async fn context_requires_authentication() {
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let (status, body) = call(app, context_request("", &[], None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "UNAUTHENTICATED");
    assert_eq!(body["title"], "Unauthenticated");
    assert_no_cjk(&body, "未认证");
}

/// AC-S-090 步骤 8（**唯一有效判据**）：用户偏好 `en-US`（已注册）+
/// 请求头 `zh-CN`（相反）→ `locale === "en-US"` 且 `source === "user"`。
#[tokio::test]
async fn context_reports_the_user_tier_when_it_contradicts_the_header() {
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let (status, body) = call(
        app,
        context_request(
            "?locale=en-US",
            &[("accept-language", "zh-CN"), ("x-tenant-id", TENANT_T1)],
            Some("good"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locale"], "en-US");
    assert_eq!(body["source"], "user", "用户偏好必须压过请求头");
    assert_eq!(body["sources"]["locale"], "user");
}

/// 步骤 3：用户偏好未注册 → 视为没设，落到请求头（不是它、也不是租户值）。
#[tokio::test]
async fn context_ignores_an_unregistered_user_locale() {
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let (status, body) = call(
        app,
        context_request(
            "?locale=ja-JP",
            &[("accept-language", "zh-CN"), ("x-tenant-id", TENANT_T1)],
            Some("good"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locale"], "zh-CN");
    assert_eq!(body["source"], "accept-language");
    assert_ne!(body["locale"], "ja-JP");
    assert_ne!(body["locale"], "en-US");
}

/// 步骤 2：请求头压过租户配置；同时**时区的 source 不是** `accept-language`。
#[tokio::test]
async fn context_prefers_the_header_for_locale_but_never_for_timezone() {
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let (status, body) = call(
        app,
        context_request(
            "",
            &[("accept-language", "zh-CN"), ("x-tenant-id", TENANT_T1)],
            Some("good"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locale"], "zh-CN");
    assert_eq!(body["source"], "accept-language");
    assert_eq!(body["timezone"], "America/New_York");
    assert_eq!(body["sources"]["timezone"], "tenant");
    assert_ne!(body["sources"]["timezone"], "accept-language");
    // 币种只有 tenant / default 两级（本轮没有用户级币种覆盖）。
    assert_eq!(body["currency"], "USD");
    assert_eq!(body["sources"]["currency"], "tenant");

    // 用户显式时区（D116）→ user，且与语言来源**分开**观测。
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let (status, body) = call(
        app,
        context_request(
            "?timezone=Europe/London",
            &[("accept-language", "zh-CN"), ("x-tenant-id", TENANT_T1)],
            Some("good"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["timezone"], "Europe/London");
    assert_eq!(body["sources"]["timezone"], "user");
    assert_eq!(body["source"], "accept-language", "语言不受时区参数影响");
}

/// 步骤 5：请求头未注册（`fr-FR`）+ 租户 `en-US` → 落到租户，`source = tenant`。
#[tokio::test]
async fn context_falls_back_to_the_tenant_when_the_header_is_unregistered() {
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let (status, body) = call(
        app,
        context_request(
            "",
            &[("accept-language", "fr-FR"), ("x-tenant-id", TENANT_T1)],
            Some("good"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locale"], "en-US");
    assert_eq!(body["source"], "tenant");

    // T2 没有租户配置 → 平台默认（manifest 的 default，裁定 #17）。
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let (status, body) = call(
        app,
        context_request("", &[("accept-language", "fr-FR;q=1")], Some("good")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // 令牌属于 T1 —— 这里换成 T2 需要一个 T2 的令牌；用 FakeTokens 的语义：
    // 只验证「未注册头 + 无用户偏好时不会返回未注册语言」。
    assert!(
        body["locale"] == "en-US" || body["locale"] == "zh-CN",
        "必须回退到已注册语言，而不是 fr-FR：{}",
        body["locale"]
    );
    assert_ne!(body["locale"], "fr-FR");
}

// ---------------------------------------------------------------------------
// D119：平台配置层加入协商
// ---------------------------------------------------------------------------

/// **TC-021-096**（D119）：**只有平台层**配了、租户没配 →
/// 值来自平台层，`source == "platform"`（**不是** `default`，也不是 `tenant`）。
///
/// 语言 / 时区 / 币种**三个字段**一起断言。
#[tokio::test]
async fn tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it() {
    let app = router(state_with_locales(
        FakeRates {
            rows: vec![],
            fail: false,
        },
        &["zh-CN", "en-US", "ja-JP"],
    ));
    let (status, body) = call(
        app,
        context_request("", &[("x-tenant-id", TENANT_T4)], Some("good-t4")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body = {body}");

    // 语言：平台层的 `en-US` 生效，来源如实报 `platform`。
    assert_eq!(body["locale"], "en-US", "平台级的语言必须生效");
    assert_eq!(body["source"], "platform", "如实报出是平台层生效");
    assert_eq!(body["sources"]["locale"], "platform");
    assert_ne!(body["locale"], "zh-CN", "不能退回 manifest 默认值");

    // 时区：平台层的 `America/New_York` 生效。
    assert_eq!(body["timezone"], "America/New_York", "平台级的时区必须生效");
    assert_eq!(body["sources"]["timezone"], "platform");
    assert_ne!(body["timezone"], "Asia/Shanghai", "不能退回 manifest 默认值");

    // 币种：平台层的 `USD` 生效。
    assert_eq!(body["currency"], "USD", "平台级的币种必须生效");
    assert_eq!(body["sources"]["currency"], "platform");
    assert_ne!(body["currency"], "CNY", "不能退回 manifest 默认值");
}

/// **TC-021-097**（D119）：平台层与租户层都配了 → **租户层赢**，
/// `source == "tenant"`（不是 `platform`）。
///
/// 语言 / 时区 / 币种**三个字段**一起断言；每个字段都断言
/// 「不是平台层的值」—— 否则「取错了层但值恰好相同」会假绿。
#[tokio::test]
async fn tc021_097_the_tenant_layer_wins_over_the_platform_layer() {
    let app = router(state_with_locales(
        FakeRates {
            rows: vec![],
            fail: false,
        },
        &["zh-CN", "en-US", "ja-JP"],
    ));
    let (status, body) = call(
        app,
        context_request("", &[("x-tenant-id", TENANT_T3)], Some("good-t3")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body = {body}");

    assert_eq!(body["locale"], "ja-JP", "租户层的语言必须压过平台层");
    assert_eq!(body["source"], "tenant", "如实报出是租户层生效");
    assert_eq!(body["sources"]["locale"], "tenant");
    assert_ne!(body["locale"], "en-US", "不能取平台层的值");

    assert_eq!(body["timezone"], "Europe/London", "租户层的时区必须压过平台层");
    assert_eq!(body["sources"]["timezone"], "tenant");
    assert_ne!(body["timezone"], "America/New_York", "不能取平台层的值");

    assert_eq!(body["currency"], "JPY", "租户层的币种必须压过平台层");
    assert_eq!(body["sources"]["currency"], "tenant");
    assert_ne!(body["currency"], "USD", "不能取平台层的值");
}

/// D119 的五级链**没有被平台层插队**：`Accept-Language` 仍压过平台层。
#[tokio::test]
async fn the_platform_layer_still_loses_to_the_accept_language_header() {
    let app = router(state_with_locales(
        FakeRates {
            rows: vec![],
            fail: false,
        },
        &["zh-CN", "en-US", "ja-JP"],
    ));
    let (status, body) = call(
        app,
        context_request(
            "",
            &[("accept-language", "zh-CN"), ("x-tenant-id", TENANT_T4)],
            Some("good-t4"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body = {body}");
    assert_eq!(body["locale"], "zh-CN", "请求头仍压过平台层");
    assert_eq!(body["source"], "accept-language");
    // 时区不看请求头 → 仍是平台层。
    assert_eq!(body["sources"]["timezone"], "platform");
}

/// 跨租户请求头：令牌属于 T1、头写 T2 → 404（不泄露资源存在性，契约 C3）。
#[tokio::test]
async fn a_cross_tenant_header_is_not_found() {
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let (status, body) = call(
        app,
        context_request("", &[("x-tenant-id", TENANT_T2)], Some("good")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "NOT_FOUND");
    assert_no_cjk(&body, "跨租户");
}

// ---------------------------------------------------------------------------
// 换算：/api/i18n/fx/convert
// ---------------------------------------------------------------------------

/// AC-S-097 步骤 2~4：结果带 `asOf` + `source` + `rateId`，且换算结果正确。
#[tokio::test]
async fn convert_returns_the_amount_with_traceability() {
    let app = router(state(FakeRates {
        rows: vec![cny_usd_snapshot(7, 139_500, "2026-01-01T00:00:00Z")],
        fail: false,
    }));
    let (status, body) = call(
        app,
        convert_request(
            "?from=CNY&to=USD&minor=10000&asOf=2026-01-01T00:00:00Z",
            Some("good"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body = {body}");
    assert_eq!(body["from"], "CNY");
    assert_eq!(body["to"], "USD");
    assert_eq!(body["minor"], 10_000);
    assert_eq!(body["convertedMinor"], 1_395);
    assert_eq!(body["rateMicros"], 139_500);
    assert_eq!(body["rateId"], 7);
    assert_eq!(body["asOf"], "2026-01-01T00:00:00Z");
    assert_eq!(body["source"], "manual");
}

/// 不存在的快照 → **404 `FX_RATE_NOT_FOUND`**（裁定 #21：不是 400、不是 0、不是原值）。
#[tokio::test]
async fn convert_is_not_found_when_the_snapshot_does_not_exist() {
    let app = router(state(FakeRates {
        rows: vec![cny_usd_snapshot(7, 139_500, "2026-01-01T00:00:00Z")],
        fail: false,
    }));
    // 同一币种对，但换一个时刻 → 必须查不到（不隐式取最新，R4）。
    let (status, body) = call(
        app,
        convert_request(
            "?from=CNY&to=USD&minor=10000&asOf=2026-06-01T00:00:00Z",
            Some("good"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "FX_RATE_NOT_FOUND");
    assert_eq!(body["title"], "FX Rate Not Found");
    assert_eq!(body["params"]["to"], "USD");
    assert_no_cjk(&body, "汇率不存在");
}

/// 溢出 → **400 `MONEY_OVERFLOW`**（不 panic、不静默回绕，裁定 #11）。
#[tokio::test]
async fn convert_reports_overflow_as_a_bad_request() {
    let app = router(state(FakeRates {
        rows: vec![cny_usd_snapshot(7, 2_000_000, "2026-01-01T00:00:00Z")],
        fail: false,
    }));
    let (status, body) = call(
        app,
        convert_request(
            &format!(
                "?from=CNY&to=USD&minor={}&asOf=2026-01-01T00:00:00Z",
                i64::MAX
            ),
            Some("good"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "MONEY_OVERFLOW");
    assert_eq!(body["title"], "Money Overflow");
    assert_no_cjk(&body, "金额溢出");
}

/// 非法币种 / 非法 `asOf` → 400 `INVALID_ARGUMENT`，并点名参数。
#[tokio::test]
async fn convert_rejects_bad_parameters_by_name() {
    for (query, param) in [
        (
            "?from=XXXX&to=USD&minor=1&asOf=2026-01-01T00:00:00Z",
            "from",
        ),
        ("?from=CNY&to=ZZZ&minor=1&asOf=2026-01-01T00:00:00Z", "to"),
        ("?from=CNY&to=USD&minor=1&asOf=not-a-time", "asOf"),
    ] {
        let app = router(state(FakeRates {
            rows: vec![],
            fail: false,
        }));
        let (status, body) = call(app, convert_request(query, Some("good"))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "query = {query}");
        assert_eq!(body["code"], "INVALID_ARGUMENT");
        assert_eq!(body["params"]["param"], param, "query = {query}");
        assert_no_cjk(&body, "参数非法");
    }
}

/// 数据访问层故障 → 500 `INTERNAL_ERROR`，且**不把内部细节**漏成中文。
#[tokio::test]
async fn convert_maps_a_store_failure_to_internal_error() {
    let app = router(state(FakeRates {
        rows: vec![],
        fail: true,
    }));
    let (status, body) = call(
        app,
        convert_request(
            "?from=CNY&to=USD&minor=100&asOf=2026-01-01T00:00:00Z",
            Some("good"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "INTERNAL_ERROR");
    assert_no_cjk(&body, "数据访问失败");
}

/// 缺失 `asOf` → 400（axum 的查询串反序列化拒绝），不是「隐式取最新」。
#[tokio::test]
async fn convert_requires_an_explicit_as_of() {
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let response = app
        .oneshot(convert_request("?from=CNY&to=USD&minor=100", Some("good")))
        .await
        .expect("路由应当能处理请求");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// 响应体的字段集合是钉死的（前端按名字取，少一个就是线上事故）。
#[tokio::test]
async fn the_convert_response_shape_is_pinned() {
    let app = router(state(FakeRates {
        rows: vec![cny_usd_snapshot(1, 1_000_000, "2026-01-01T00:00:00Z")],
        fail: false,
    }));
    let (_, body) = call(
        app,
        convert_request(
            "?from=CNY&to=USD&minor=1&asOf=2026-01-01T00:00:00Z",
            Some("good"),
        ),
    )
    .await;
    let keys: Vec<&str> = body
        .as_object()
        .expect("响应体应是对象")
        .keys()
        .map(String::as_str)
        .collect();
    let mut keys = keys;
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "asOf",
            "convertedMinor",
            "from",
            "minor",
            "rateId",
            "rateMicros",
            "source",
            "to",
        ]
    );
}

/// `context` 的响应形状：`{locale, timezone, currency, source, sources}`。
#[tokio::test]
async fn the_context_response_shape_is_pinned() {
    let app = router(state(FakeRates {
        rows: vec![],
        fail: false,
    }));
    let (_, body) = call(
        app,
        context_request("", &[("x-tenant-id", TENANT_T1)], Some("good")),
    )
    .await;
    let mut keys: Vec<&str> = body
        .as_object()
        .expect("响应体应是对象")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["currency", "locale", "source", "sources", "timezone"]
    );
    let mut sources: BTreeMap<&str, &Value> = BTreeMap::new();
    for (key, value) in body["sources"].as_object().expect("sources 应是对象") {
        sources.insert(key.as_str(), value);
    }
    assert_eq!(
        sources.keys().copied().collect::<Vec<_>>(),
        vec!["currency", "locale", "timezone"]
    );
}
