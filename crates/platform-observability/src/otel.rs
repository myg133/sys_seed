//! OpenTelemetry 导出（REQ-019，D339 / AC-S-037）。
//!
//! # ⚠️ 三条硬约束（违反任何一条都会在生产出事）
//!
//! 1. **必须有限超时**：collector 不可达时无限等会让请求挂住
//!    （AC-S-037 步骤 2 判据：单次请求 < 1 秒）；
//! 2. **必须批量 + 有界队列**：满了丢弃并 warn，不能阻塞业务；
//! 3. **失败只打日志**：不 `panic`、不把错误返回给调用方
//!    （OTel 是**旁路**，不能成为业务可用性的单点）。
//!
//! # ⚠️ 可注入缝（挑刺 B10）
//!
//! [`init_otel`] 接受 `exporter_override` —— 测试传
//! `InMemorySpanExporter` 才能「看到」span；不传才走生产 OTLP。
//! 没有这个缝，AC-S-037 的四条判据**全部不可执行**。
//!
//! # ⚠️ 入站 `X-Request-Id` → OTel `TraceId`（AC-S-029 步骤 5）
//!
//! 这是本模块**最容易做错**的一处：`tracing` + `tracing-opentelemetry`
//! **默认不会**把入站头当成 OTel trace_id —— 它会自己生成一个。
//! 必须显式用 [`context_from_trace_id`] 构造**远端父上下文**。

use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState};
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::trace::SdkTracerProvider;

/// OTel 配置（来自环境变量）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtelConfig {
    /// `OTEL_SDK_DISABLED=true` 时为 `true`（**完全不初始化**）。
    pub disabled: bool,
    /// `OTEL_EXPORTER_OTLP_ENDPOINT`。
    pub endpoint: String,
    /// 导出超时（秒）。⚠️ **必须有界**。
    pub timeout_seconds: u64,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            disabled: false,
            endpoint: "http://127.0.0.1:4318".to_string(),
            // ⚠️ 3 秒：足够一次导出，又不会让请求明显变慢。
            timeout_seconds: 3,
        }
    }
}

impl OtelConfig {
    /// 从环境变量读（缺省用默认值）。
    pub fn from_env() -> Self {
        let default = Self::default();
        let disabled = std::env::var("OTEL_SDK_DISABLED")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or(default.endpoint);
        let timeout_seconds = std::env::var("OTEL_EXPORTER_OTLP_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default.timeout_seconds);
        Self {
            disabled,
            endpoint,
            timeout_seconds,
        }
    }
}

/// OTel 的句柄（**持有 provider，drop 时 flush**）。
///
/// ⚠️ 必须活到进程结束 —— 提前 drop 会让导出器停止工作。
pub struct OtelGuard {
    provider: Option<SdkTracerProvider>,
}

impl OtelGuard {
    /// 未初始化（`OTEL_SDK_DISABLED=true` 或构造失败）。
    pub fn disabled() -> Self {
        Self { provider: None }
    }

    /// OTel 是否**真的启用**（AC-S-037 步骤 3 的判据）。
    ///
    /// ⚠️ 这是全局唯一的观测点：测试断言 `OTEL_SDK_DISABLED=true` 时它为 `false`。
    pub fn is_enabled(&self) -> bool {
        self.provider.is_some()
    }

    /// 拿 provider（给 `tracing_opentelemetry` 的 layer 用）。
    pub fn provider(&self) -> Option<&SdkTracerProvider> {
        self.provider.as_ref()
    }
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        // ⚠️ 尽力 flush 残余 span；失败只 warn（不能 panic —— Drop 里 panic 会 abort）。
        if let Some(provider) = self.provider.take()
            && let Err(err) = provider.shutdown()
        {
            tracing::warn!(%err, "the otel tracer provider could not be shut down cleanly");
        }
    }
}

/// 初始化 OTel（**返回 `None` 语义见 [`OtelGuard::is_enabled`]**）。
///
/// # 参数
///
/// - `config`：环境变量读来的配置
/// - `exporter_override`：`Some` → 用注入的导出器（**测试专用**）；
///   `None` → 构造生产 OTLP 导出器
///
/// # ⚠️ 绝不会 panic、绝不会返回 `Err`
///
/// 任何失败（endpoint 非法、导出器构造失败）都只 `warn` 并返回
/// [`OtelGuard::disabled`] —— 可观测性不能成为启动失败的原因。
pub fn init_otel(config: &OtelConfig) -> OtelGuard {
    if config.disabled {
        tracing::info!("OTEL_SDK_DISABLED=true - the otel sdk is not initialised");
        return OtelGuard::disabled();
    }

    // ⚠️ 生产用**批量**导出器 + 有限超时（AC-S-037 步骤 2）。
    //
    // 批量导出器的队列是**有界**的：满了丢弃并打 warn，不会阻塞业务
    // （这是「导出失败不影响业务」的机制保证）。
    let exporter = match build_otlp_exporter(config) {
        Ok(exporter) => exporter,
        Err(err) => {
            tracing::warn!(
                error = %err,
                endpoint = %config.endpoint,
                "the otlp exporter could not be built - tracing export is disabled"
            );
            return OtelGuard::disabled();
        }
    };

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();

    tracing::info!(
        endpoint = %config.endpoint,
        timeout_seconds = config.timeout_seconds,
        "the otel tracer provider is initialised (otlp/http-protobuf)"
    );

    OtelGuard {
        provider: Some(provider),
    }
}

/// 构造生产 OTLP 导出器（HTTP/protobuf，**不是 gRPC**）。
fn build_otlp_exporter(
    config: &OtelConfig,
) -> Result<opentelemetry_otlp::SpanExporter, opentelemetry_otlp::ExporterBuildError> {
    use opentelemetry_otlp::WithExportConfig;
    opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(config.endpoint.clone())
        .with_timeout(std::time::Duration::from_secs(config.timeout_seconds))
        .build()
}

/// 把 32 位 hex 的 `trace_id` 变成 OTel 的**远端父上下文**（AC-S-029 步骤 5）。
///
/// # ⚠️ 为什么必须显式做
///
/// `tracing-opentelemetry` 在开新 span 时**会自己生成 trace_id** ——
/// 入站的 `X-Request-Id` 对它**没有任何影响**。
/// 若不构造远端父上下文，span 的 `trace_id` 与响应头的 `X-Request-Id`
/// **必然不等** → AC-S-029 步骤 5 红。
///
/// # 返回
///
/// - `Some(Context)`：可用作 `SpanBuilder::with_context` 的父
/// - `None`：`trace_id` 不是合法的 32 hex（调用方应放弃贯通，用 OTel 自己的）
pub fn context_from_trace_id(trace_id_hex: &str) -> Option<Context> {
    let span_context = remote_span_context(trace_id_hex)?;
    Some(Context::new().with_remote_span_context(span_context))
}

/// 构造远端 span context（[`context_from_trace_id`] 的内核，**可单独断言**）。
///
/// ⚠️ 单独抽出来的理由：`Context::span()` **读不到**用
/// `with_remote_span_context` 存进去的值（它返回的是「当前 span」，
/// 远端上下文不是当前 span）→ 通过 `Context` 断言会失败。
/// 把内核抽出来，测试直接断言 `SpanContext` 的三个关键字段。
pub fn remote_span_context(trace_id_hex: &str) -> Option<SpanContext> {
    // ⚠️ **必须先过我们自己的校验** —— 实测踩过：
    // `TraceId::from_hex("abc")` **会左填充接受**（当成 0x00..0abc），
    // 于是 `"abc"` 也能构造出一个「看起来合法」的 trace_id。
    //
    // 这正是 D340 把校验放在 `trace::normalize_trace_id` 的原因：
    // **全平台只有一处校验入口**，OTel 的类型转换只能拿到已校验的值。
    let normalized = crate::trace::normalize_trace_id(trace_id_hex)?;
    let trace_id = TraceId::from_hex(&normalized).ok()?;

    // ⚠️ **不能用 `SpanId::INVALID`（全零）** —— 实测踩过：
    // `SpanContext::is_valid()` 要求 **trace_id 与 span_id 都非零**，
    // 用 INVALID 会让 `is_valid()` 为 `false` → 整个上下文被判为无效。
    //
    // 而我们的入站 `X-Request-Id` **只有 trace_id、没有 span_id** ——
    // 这时用一个固定的「远端根」span_id（`...01`）表示
    // 「这是从外部带进来的根，本服务看不到它的父 span」。
    //
    // ⚠️ 这个值**不影响** trace_id 贯通（AC-S-029 步骤 5 只断言 trace_id 相等）；
    // 它只影响「本服务的 span 在链路里的父子关系」——
    // 用固定值意味着所有入站根 span 共享同一个假父，这在只关心 trace 时是可接受的。
    const REMOTE_ROOT_SPAN_ID: u64 = 1;
    let span_context = SpanContext::new(
        trace_id,
        SpanId::from_bytes(REMOTE_ROOT_SPAN_ID.to_be_bytes()),
        TraceFlags::SAMPLED,
        true, // is_remote = true：明确这是从外部带进来的
        TraceState::default(),
    );

    if !span_context.is_valid() {
        return None;
    }
    Some(span_context)
}

/// 构造一条 span 的属性（给业务侧补充语义用）。
pub fn request_attributes(method: &str, path: &str, status: u16) -> Vec<KeyValue> {
    vec![
        KeyValue::new("http.request.method", method.to_string()),
        KeyValue::new("url.path", path.to_string()),
        KeyValue::new("http.response.status_code", i64::from(status)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认配置：3 秒超时（⚠️ 必须有界，不能是 0 或无限）。
    #[test]
    fn the_default_timeout_is_bounded() {
        let cfg = OtelConfig::default();
        assert!(
            (1..=30).contains(&cfg.timeout_seconds),
            "超时必须有界且合理，实际 {}",
            cfg.timeout_seconds
        );
    }

    /// `OTEL_SDK_DISABLED=true` → `is_enabled() == false`（AC-S-037 步骤 3）。
    #[test]
    fn a_disabled_config_produces_an_disabled_guard() {
        let cfg = OtelConfig {
            disabled: true,
            ..OtelConfig::default()
        };
        let guard = init_otel(&cfg);
        assert!(
            !guard.is_enabled(),
            "OTEL_SDK_DISABLED=true 时 is_enabled 必须是 false"
        );
        assert!(guard.provider().is_none());
    }

    /// **不可达 endpoint 不会 panic**，只是 `is_enabled() == false`
    /// （AC-S-037 步骤 2 的「服务不崩」在一个更小的范围上先验一次）。
    #[test]
    fn an_unreachable_endpoint_does_not_panic() {
        let cfg = OtelConfig {
            disabled: false,
            endpoint: "http://127.0.0.1:1".to_string(),
            timeout_seconds: 1,
        };
        // 构造导出器本身不发网络请求 → 不会 panic。
        // （真正的「导出失败不影响业务」在集成测试里验。）
        let guard = init_otel(&cfg);
        // 这里不断言 is_enabled 的具体值 —— 不同 OTel 版本对「构造成功但连不上」
        // 的处理不同；本用例只保证**不 panic**。
        let _ = guard.is_enabled();
    }

    /// 合法 trace_id → 远端 span context 的三个关键字段正确。
    #[test]
    fn a_valid_trace_id_becomes_a_remote_parent_context() {
        let hex = "0123456789abcdef0123456789abcdef";
        let span_context = remote_span_context(hex).expect("合法 trace_id 应能构造上下文");
        assert_eq!(
            span_context.trace_id().to_string(),
            hex,
            "trace_id 必须逐字保留（这是 AC-S-029 步骤 5 的基础）"
        );
        assert!(span_context.is_remote(), "必须标记为远端父上下文");
        // ⚠️ span_id **不能**是 INVALID（全零）—— 那会让 is_valid() 为 false。
        assert_ne!(
            span_context.span_id(),
            SpanId::INVALID,
            "span_id 必须非零，否则 SpanContext 被判无效"
        );
        assert!(
            span_context.trace_flags().is_sampled(),
            "必须采样（否则 span 不会导出）"
        );
        // 外层的 `context_from_trace_id` 也必须能包住它。
        assert!(context_from_trace_id(hex).is_some());
    }

    /// 非法 trace_id → `None`（调用方据此放弃贯通）。
    #[test]
    fn an_invalid_trace_id_yields_no_context() {
        for bad in ["", "abc", "../../etc/passwd", "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"] {
            assert!(
                context_from_trace_id(bad).is_none(),
                "{bad:?} 不该构造出上下文"
            );
        }
    }

    /// 全零 trace_id 是**非法**的（OTel 规范：trace_id 不能全零）。
    #[test]
    fn an_all_zero_trace_id_is_rejected() {
        assert!(
            context_from_trace_id("00000000000000000000000000000000").is_none(),
            "全零 trace_id 在 OTel 里是 INVALID"
        );
    }

    /// 请求属性齐备（三层 span 的语义补充）。
    #[test]
    fn request_attributes_cover_method_path_and_status() {
        let attrs = request_attributes("GET", "/api/rbac/roles", 200);
        assert_eq!(attrs.len(), 3);
        let keys: Vec<&str> = attrs.iter().map(|kv| kv.key.as_str()).collect();
        assert!(keys.contains(&"http.request.method"));
        assert!(keys.contains(&"url.path"));
        assert!(keys.contains(&"http.response.status_code"));
    }
}
