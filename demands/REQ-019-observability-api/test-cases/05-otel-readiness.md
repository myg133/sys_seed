# 用例：OTel 配置生效与就绪探针（AC-S-037 / AC-S-038）

> 覆盖 TC-019-058 ~ 067（10 条）

---

## AC-S-037：OTel 配置生效（6 条）

**公共前置**: `init_otel(cfg, Some(InMemorySpanExporter))` 可注入内存导出器
（⚠️ rev 2 新增的缝 —— rev 1 没有它，本组用例不可执行，挑刺 B10）

### TC-019-058 span 的 trace_id 等于入站 X-Request-Id

- **步骤**: `init_otel(cfg, Some(in_memory))` → 发一次带 `X-Request-Id: <T>` 的请求
  → 从导出器取 span
- **判据**: span 的 `trace_id.to_string() == T`（**逐字相等**）
- **断言**: `assert_eq!(span.span_context.trace_id().to_string(), T)`
- **⚠️ 实现要点**: 必须 `TraceId::from_hex(T)` + 远端父上下文；
  否则 OTel 自己生成 id → 红（`design-summary.md` §5.2）

### TC-019-059 span 层级至少 2 层

- **判据**: 导出器里 ≥ 2 个 span，且至少一个的 `parent_span_id` 非 `INVALID`
- **断言**: `assert!(spans.len() >= 2)` + `assert!(child.parent_span_id != SpanId::INVALID)`

### TC-019-060 span 有 span_id 且非零

- **判据**: 每个 span 的 `span_id` 非 `SpanId::INVALID`
- **断言**: `assert_ne!(span.span_context.span_id(), SpanId::INVALID)`

### TC-019-061 collector 不可达时请求仍成功（**M3 修正：加时延上界**）

- **前置**: `OTEL_EXPORTER_OTLP_ENDPOINT = http://127.0.0.1:1`（不可达）
- **步骤**: 连发 **20 次**请求，记录每次耗时
- **判据**:
  - 20 次**都不是 5xx**（业务不受影响）
  - **每次耗时 < 1 秒**
- **断言**: `assert!(durations.iter().all(|d| *d < Duration::from_secs(1)))`
- **⚠️ rev 2 修正**: rev 1 只断言「200」，但 D339 要防的坑是
  「重试阻塞 → **请求变慢**」，慢 3 秒仍 200 → 零区分力（挑刺 M3）

### TC-019-062 OTEL_SDK_DISABLED=true 时 is_enabled()==false（**阴性对照**）

- **步骤**: 设 `OTEL_SDK_DISABLED=true` → `init_otel(...)`
- **判据**: 返回的 `OtelGuard::is_enabled() == false`
- **断言**: `assert!(!guard.is_enabled())`
- **⚠️ rev 2 明确**: `is_enabled()` 是 **`OtelGuard`** 的方法（rev 1 没说是哪个对象）

### TC-019-063 导出失败只打日志不 panic

- **步骤**: 同 061，捕获日志
- **判据**: 日志里有 warn/error 级别的导出失败痕迹；进程未 panic
- **断言**: `assert!(logs.to_lowercase().contains("export") || logs.contains("WARN"))`
- **关联**: 与 061 互补（061 验延迟，063 验有痕迹）

---

## AC-S-038：就绪探针覆盖限流存储（4 条，rev 2 新增）

### TC-019-064 正常时 /health/ready 是 200 且含限流检查项

- **前置**: `rateLimit.enabled = true`
- **步骤**: `GET /health/ready`
- **判据**: 200；响应体 `checks` 里有**限流存储**那一项且为 ok
- **断言**: `assert_eq!(status, 200)` + `assert!(checks.rate_limit == "ok")`

### TC-019-065 限流表不可达 → 503 且明确指出是限流存储

- **步骤**: 让限流表不可访问（改 schema / 指向不存在的表）→ `GET /health/ready`
- **判据**: **503**；响应体明确说**限流存储**不可达
- **断言**: `assert_eq!(status, 503)` + `assert!(body.contains("rate_limit"))`
- **⚠️ 不是笼统的 `database unavailable`** —— 运维要能分出哪个依赖坏了

### TC-019-066 关闭限流时，限流表坏了也不影响就绪（**关键判据**）

- **前置**: `rateLimit.enabled = false` + 限流表不可达
- **步骤**: `GET /health/ready`
- **判据**: **200**
- **断言**: `assert_eq!(status, 200)`
- **⚠️ 为什么关键**: 「没启用限流」的部署不该因限流表问题而不就绪 ——
  否则关闭限流反而更脆弱。实现无条件检查限流表 → 红

### TC-019-067 就绪响应里 checks 的键齐备

- **判据**: `checks` 至少含 `database` 与（启用限流时）`rate_limit` 两项
- **断言**: `assert!(checks.get("database").is_some())`
- **区分力**: 实现只返回布尔 `ready: true` → 缺结构 → 红

---

## 判据校准清单

| 用例段 | 目标文件（预设） |
|---|---|
| TC-019-058~063 | `crates/platform-observability/tests/otel_in_memory.rs` |
| TC-019-064~067 | `crates/platform-server/tests/readiness_real_db.rs` |

---

## 明码不做（TD-069）

**真导出到 collector** 无法验证（本机无 collector）。
上面 6 条验的是：① 配置生效 ② span 结构 ③ 入站 id 贯通
④ **失败不影响业务（含延迟）** ⑤ 可关闭 ⑥ 有痕迹。

⚠️ 这**不声称**验过「span 真的到了 Jaeger/Tempo」。
