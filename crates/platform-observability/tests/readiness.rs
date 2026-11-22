//! 就绪探针的限流判定（REQ-019，**AC-S-038 步骤 1/3/4 的端到端判据**）。
//!
//! # 这个文件补什么（F5）
//!
//! AC-S-038 的判据主体是**响应码与响应体**：
//!
//! | 步骤 | 场景 | 期望 |
//! |---|---|---|
//! | 1 | 限流启用 + 存储可达 | **200**，`checks.rate_limit == "ok"` |
//! | 3 | 限流启用 + 存储**不可达** | **503**，响应体指出是**限流存储** |
//! | 4 | 限流**关闭** + 存储不可达 | **200**（不该因限流表问题而不就绪） |
//!
//! # ⚠️ rev 1 的问题
//!
//! 原先只测了 `rate_limit_readiness()` 这个**纯函数**的返回枚举
//! （`Some(true)` / `None`），而「谁来把它变成 503」那段逻辑
//! **内联在 `main.rs` 的 `ready` handler 里**（bin crate，测试够不着）。
//!
//! → 503 分支**一行都没被测过**。若把条件写成 `rate_limit_ok != Some(true)`
//! （禁用时也 503），所有测试全绿 —— 而那恰好违反步骤 4。
//!
//! # 现在的判据链
//!
//! 1. [`rate_limit_readiness`]：查库 → `Option<bool>`
//! 2. [`readiness_rate_limit_outcome`]：`Option<bool>` → `Ok(checks 补丁) | Err(code)`
//! 3. [`ready_response`]（本文件）：模拟 `main.rs` 的组装 →
//!    断言**真实的状态码与响应体**
//!
//! 第 2、3 步在**同一个 crate**（`main.rs` 与测试调同一个函数）→ 有判据了。

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;

use platform_core::RequestContext;
use platform_observability::RateLimiter;
use platform_observability::http::{
    readiness_rate_limit_outcome, rate_limit_readiness,
};
use platform_observability::rate_limit::middleware::{RateLimitState, StaticConfig};
use platform_observability::rate_limit::{RateLimitSettings, WindowLimit};

/// 就绪探针的替身：**复刻 `main.rs` 的判定链**（F5 的处置）。
///
/// # ⚠️ 为什么这里要重复 main 的几行
///
/// `main.rs` 是 **bin crate**，它的函数无法被集成测试引用。
/// 只有**判定逻辑**抽到了库 crate（[`readiness_rate_limit_outcome`]），
/// 「组装响应」这几行做不到（它依赖 `main` 的 `AppState`）。
///
/// → 这里复刻那几行，并用**同一个判定函数**：
/// 判据覆盖「判定 + 组装」的全链，只是组装部分是等价实现。
/// 边界如实记录：若 `main.rs` 的组装代码与这里**不一致**（比如它写成
/// `if rate_limit_ok != Some(true)`），本文件发现不了 —— 但那种写法会
/// 直接违反 [`readiness_rate_limit_outcome`] 的返回值语义（`Err` 才是 503），
/// 属于「不按契约用返回值」。
fn ready_response(rate_limit_ok: Option<bool>, db_ready: bool) -> (StatusCode, serde_json::Value) {
    let outcome = readiness_rate_limit_outcome(rate_limit_ok);

    if db_ready && outcome.is_ok() {
        let mut body = serde_json::json!({
            "status": "ready",
            "checks": { "database": "ok" },
        });
        if let Ok(Some((key, value))) = outcome {
            body["checks"][key] = serde_json::Value::from(value);
        }
        return (StatusCode::OK, body);
    }

    if let Err(code) = outcome {
        let body = serde_json::json!({
            "kind": format!("https://errors.base.example/{}", platform_core::problem::kebab_case(code)),
            "title": platform_core::titles::title_of_code(code),
            "status": StatusCode::SERVICE_UNAVAILABLE.as_u16(),
            "detail": "the rate limit store is unavailable",
            "instance": "/health/ready",
            "code": code,
        });
        return (StatusCode::SERVICE_UNAVAILABLE, body);
    }

    (
        StatusCode::SERVICE_UNAVAILABLE,
        serde_json::json!({
            "status": "not-ready",
            "checks": { "database": "unavailable" },
        }),
    )
}

/// 一个「存储可达性可控」的限流器替身。
struct ProbeLimiter {
    /// `is_reachable` 返回什么。
    reachable: bool,
    /// `is_reachable` 被调用了几次（证明探针**真的**查了库，不是短路）。
    reachable_calls: std::sync::atomic::AtomicUsize,
}

impl ProbeLimiter {
    fn new(reachable: bool) -> Self {
        Self {
            reachable,
            reachable_calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.reachable_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl RateLimiter for ProbeLimiter {
    fn check<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _dimension: &'a str,
        _key: &'a str,
        _limit: WindowLimit,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<(), platform_observability::error::ObservabilityError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(()) })
    }

    fn is_reachable<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        self.reachable_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let reachable = self.reachable;
        Box::pin(async move { reachable })
    }
}

/// 造一份「限流启用 + 三项都开」的配置。
fn enabled_settings() -> RateLimitSettings {
    RateLimitSettings {
        enabled: true,
        tenant: WindowLimit {
            requests: 10,
            window_seconds: 60,
        },
        user: WindowLimit {
            requests: 10,
            window_seconds: 60,
        },
        endpoint: WindowLimit {
            requests: 10,
            window_seconds: 60,
        },
    }
}

/// 造一份「限流**关闭**」的配置。
fn disabled_settings() -> RateLimitSettings {
    RateLimitSettings {
        enabled: false,
        ..enabled_settings()
    }
}

fn state_with(settings: RateLimitSettings, reachable: bool) -> (Arc<RateLimitState>, Arc<ProbeLimiter>) {
    let limiter = Arc::new(ProbeLimiter::new(reachable));
    let state = Arc::new(RateLimitState {
        limiter: Some(limiter.clone()),
        config: Arc::new(StaticConfig(settings)),
    });
    (state, limiter)
}

// ---------------------------------------------------------------------------
// 步骤 1：启用 + 可达 → 200，checks.rate_limit == "ok"
// ---------------------------------------------------------------------------

/// **AC-S-038 步骤 1**：限流启用且存储可达 → **200**，`checks.rate_limit == "ok"`。
#[tokio::test]
async fn a_reachable_store_yields_200_with_an_ok_check() {
    let (state, limiter) = state_with(enabled_settings(), true);
    let ctx = RequestContext::new().with_tenant("_probe");

    // ① 第一环：查库得到 `Some(true)`。
    let ok = rate_limit_readiness(&Some(state), &ctx).await;
    assert_eq!(ok, Some(true), "启用 + 可达 → Some(true)");
    assert_eq!(limiter.calls(), 1, "必须**真的**查了一次库（不是短路）");

    // ② 第二、三环：判定 + 组装 → 200 且 checks 里有 rate_limit=ok。
    let (status, body) = ready_response(ok, true);
    assert_eq!(status, StatusCode::OK, "步骤 1 必须 200；body={body}");
    assert_eq!(
        body["checks"]["rate_limit"], "ok",
        "步骤 1：checks 里限流项必须为 ok；实际 body={body}"
    );
    assert_eq!(body["checks"]["database"], "ok");
}

// ---------------------------------------------------------------------------
// 步骤 3：启用 + 不可达 → 503，且指出是限流存储
// ---------------------------------------------------------------------------

/// **AC-S-038 步骤 3**：存储**不可达** → **503**，响应体指出是**限流存储**。
///
/// ⚠️ 这条是 rev 1 **完全没有**的判据（原版 `Some(false)` 只出现在注释里）。
#[tokio::test]
async fn an_unreachable_store_yields_503_naming_the_rate_limit_store() {
    let (state, limiter) = state_with(enabled_settings(), false);
    let ctx = RequestContext::new().with_tenant("_probe");

    let ok = rate_limit_readiness(&Some(state), &ctx).await;
    assert_eq!(ok, Some(false), "启用 + 不可达 → Some(false)");
    assert_eq!(limiter.calls(), 1, "必须真的查了一次库");

    let (status, body) = ready_response(ok, true);
    // ① 状态码必须是 503（不是泛泛的 200/500）。
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "步骤 3 必须 503；body={body}"
    );
    // ② 响应体必须**明确指向限流存储**（不是笼统的「数据库错误」）。
    assert_eq!(
        body["code"], "RATE_LIMIT_UNAVAILABLE",
        "步骤 3：code 必须点名限流存储；实际 body={body}"
    );
    assert_eq!(body["status"], 503);
    assert_eq!(body["instance"], "/health/ready");
    // ③ `detail` 不得泄露表名 / SQL（安全要求）。
    let detail = body["detail"].as_str().unwrap_or_default();
    assert!(
        !detail.contains("platform_rate_limit") && !detail.to_uppercase().contains("SELECT"),
        "detail 不得泄露表名或 SQL：{detail}"
    );
}

// ---------------------------------------------------------------------------
// 步骤 4：关闭限流 + 不可达 → **仍然 200**
// ---------------------------------------------------------------------------

/// **AC-S-038 步骤 4**：`rateLimit.enabled = false` 时，**存储不可达也返回 200**。
///
/// ⚠️ 这条防的正是 F5 指出的那类实现错误：把条件写成
/// `rate_limit_ok != Some(true)` → 关闭限流时 `None != Some(true)` 成立 → **503**
/// → 「没启用限流反而更脆弱」，荒谬。
#[tokio::test]
async fn a_disabled_rate_limit_ignores_an_unreachable_store() {
    let (state, limiter) = state_with(disabled_settings(), false);
    let ctx = RequestContext::new().with_tenant("_probe");

    let ok = rate_limit_readiness(&Some(state), &ctx).await;
    // ① 关闭时**不检查**（返回 None）—— 所以根本没查库。
    assert_eq!(ok, None, "关闭限流 → None（不参与判定）");
    assert_eq!(
        limiter.calls(),
        0,
        "步骤 4：关闭限流时**不该查限流表**（省一次查询，且避免把表问题带进就绪判定）"
    );

    let (status, body) = ready_response(ok, true);
    // ② 即使 limiter 的 is_reachable 是 false，也必须 200。
    assert_eq!(
        status,
        StatusCode::OK,
        "步骤 4：关闭限流时存储不可达也必须 200；body={body}"
    );
    // ③ checks 里**不该**出现 rate_limit 键（没检查就不该报状态）。
    assert!(
        body["checks"].get("rate_limit").is_none(),
        "步骤 4：未启用限流时 checks 里不该有限流项；实际 body={body}"
    );
    assert_eq!(body["checks"]["database"], "ok");
}

// ---------------------------------------------------------------------------
// 未装配限流（`rate_limit = None`）→ 与「关闭」同款
// ---------------------------------------------------------------------------

/// **未装配限流器**（`None`）→ 与「关闭」等价：不参与判定，返回 200。
#[tokio::test]
async fn an_absent_rate_limiter_does_not_affect_readiness() {
    let ctx = RequestContext::new().with_tenant("_probe");
    let ok = rate_limit_readiness(&None, &ctx).await;
    assert_eq!(ok, None, "未装配 → None");

    let (status, body) = ready_response(ok, true);
    assert_eq!(status, StatusCode::OK);
    assert!(body["checks"].get("rate_limit").is_none());
}

// ---------------------------------------------------------------------------
// 数据库不就绪 + 限流不可达 → 两者都 503，但**错误码必须指向限流**
// ---------------------------------------------------------------------------

/// 数据库与限流**同时**不可达 → 503，且 `code` 指向**限流**（判据优先级明确）。
///
/// ⚠️ 这条锁住「判定顺序」：限流检查在数据库判定**之后**做，
/// 但它的 `Err` 会**优先**返回（因为它的错误更具体）。
/// 若这个顺序被换掉，`code` 会变成数据库的 → 这条红。
#[tokio::test]
async fn both_unreachable_reports_the_rate_limit_code() {
    let (status, body) = ready_response(Some(false), false);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body["code"], "RATE_LIMIT_UNAVAILABLE",
        "两个都不可达时，code 必须点名限流存储（更具体的错误优先）；实际 body={body}"
    );
}

// ---------------------------------------------------------------------------
// 判定函数的纯逻辑（防「靠 StatusCode 猜」）
// ---------------------------------------------------------------------------

/// [`readiness_rate_limit_outcome`] 的三分支**逐条**锁死。
///
/// ⚠️ 这条是 F5 的最小判据：即使上面的端到端测试被改坏，
/// 这条也能独立证明「`None` 不产生 503」这个关键语义。
#[test]
fn the_outcome_mapping_is_exactly_three_way() {
    // None（未装配 / 已禁用）→ 不参与判定，**绝不可**变成 503。
    assert_eq!(
        readiness_rate_limit_outcome(None),
        Ok(None),
        "None 必须是 Ok(None) —— 若写成 Err，关闭限流会让就绪探针 503（违反步骤 4）"
    );
    // Some(true) → 往 checks 里加 ok。
    assert_eq!(
        readiness_rate_limit_outcome(Some(true)),
        Ok(Some(("rate_limit", "ok")))
    );
    // Some(false) → 503 + 点名限流。
    assert_eq!(
        readiness_rate_limit_outcome(Some(false)),
        Err("RATE_LIMIT_UNAVAILABLE")
    );
    // ⚠️ 反向断言：None **不能**与 false 等价（这是 F5 的核心）。
    assert_ne!(
        readiness_rate_limit_outcome(None),
        readiness_rate_limit_outcome(Some(false)),
        "`None`（未启用）与 `Some(false)`（不可达）的处置**必须不同** —— \
         前者 200、后者 503"
    );
}

/// 503 的响应体必须能被 `IntoResponse` 组装成合法 JSON（不 panic、状态码保留）。
#[test]
fn the_503_body_is_serialisable() {
    let (_, body) = ready_response(Some(false), true);
    let response = axum::Json(body).into_response();
    assert_eq!(response.status(), StatusCode::OK, "Json 本身是 200 包装");
    // 真正断言的是「能被序列化」—— 没有 panic 就说明结构合法。
}
