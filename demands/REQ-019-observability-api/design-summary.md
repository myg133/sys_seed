# 设计摘要：可观测性与 API 治理（REQ-019）

- **需求编号**: REQ-019
- **基线代码**: `develop` @ `0b9697b`
- **状态**: rev 1（2026-11-22）

---

## 1. 新增 crate 与文件布局

```
crates/platform-observability/
├── Cargo.toml
├── manifest.json              # 配置项 + 权限点（若有）
└── src/
    ├── lib.rs                 # 导出 + 版本
    ├── error.rs               # 错误码（本 crate 自有的）
    ├── rate_limit/
    │   ├── mod.rs             # RateLimiter trait + 三级维度枚举
    │   ├── pg_window.rs       # PG 滑动窗口实现（AC-S-031/032/033）
    │   └── middleware.rs      # axum 中间件（读配置 → 判定 → 429）
    ├── trace/
    │   ├── mod.rs             # trace_id 生成/校验（AC-S-029/030）
    │   └── layer.rs           # tracing Layer（把 trace_id 注入每行日志）
    ├── otel.rs                # OTel SDK 装配（AC-S-037）
    └── openapi.rs             # utoipa 文档聚合（AC-S-035）
```

⚠️ **`platform-observability` 只被 `platform-server` 依赖** ——
不放进 `platform-core`，否则每个 crate 都要编译 OTel（D343）。

---

## 2. 数据表

### 2.1 `platform_rate_limit` —— 滑动窗口计数（**租户级**，RLS）

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | `BIGSERIAL` | 自增（比 UUID 省空间，这张表行数最多） |
| `tenant_id` | `TEXT` | RLS 维度 |
| `dimension` | `TEXT` | `tenant` / `user` / `endpoint` |
| `key` | `TEXT` | 见 D337 的键形态 |
| `occurred_at` | `TIMESTAMPTZ` | 请求时刻 |

**索引**（唯一一个，必须精确匹配查询）：
```sql
CREATE INDEX platform_rate_limit_lookup_idx
    ON platform_rate_limit (tenant_id, dimension, key, occurred_at DESC);
```

⚠️ 查询是 `WHERE tenant_id AND dimension AND key AND occurred_at > ...` ——
索引列**顺序必须**与它一致，否则会退化成全表扫。

**RLS**：进 manifest 的 `tenantScoped.tables`（必须，否则 A 租户的计数会影响 B 租户）。

### 2.2 清理（**必做，D337**）

```sql
DELETE FROM platform_rate_limit
 WHERE occurred_at < now() - (max_window_seconds * 2 || ' seconds')::interval;
```

实现形态：`xtask observability prune-rate-limit`，可被 cron / 现有的
`xtask storage worker` 同款循环驱动。

⚠️ **不实现清理就是埋雷**：每请求一行，表几周内变性能瓶颈。

---

## 2.3 未认证请求怎么办（挑刺 M7，**明码不做**）

`platform_rate_limit` 是 **RLS 租户表**，`TenantDb::begin` 在**无租户**时
直接 `DbError::MissingTenant`（`crates/platform-db/src/tenant.rs:180`）。

→ 登录 / 注册 / `/health` 等**认证之前**的请求**写不进限流表**。

**本轮决定：不做**（→ **TD-072**）。理由：
1. 要做得正确，需要「按 IP 的独立维度」或「sentinel 租户 + 非 RLS 路径」——
   前者要新表（IP 不是租户数据），后者要在 RLS 上开后门（风险大）；
2. 现有 IAM 登录**已有**自己的 IP 限流（`platform-iam/src/service.rs:837`
   `check_ip_rate_limit`），所以暴力破解**不是完全裸露**；
3. 本需求的 9 条 AC 全部只覆盖**已认证**请求 —— 这是**明码的范围**，不是遗漏。

⚠️ **实现时必须有这一步**：限流中间件遇到「无租户上下文」时
**直接放行**（而不是报错）—— 否则登录端点会因为拿不到租户而 500。

---

## 3. 限流判定流程

```
请求进来
  ↓
① 读配置（rateLimit.enabled？三个维度的 requests / windowSeconds）
  ↓ ⚠️ 读配置**失败 / 键未登记 / 缓存无快照** → 一律视为 disabled
     （**fail open** + 一条 WARN），见下方「读配置的失败语义」
  ↓ enabled=false → 直接放行（零开销，不查库）
  ↓ 无租户上下文（未认证）→ 直接放行（见 §2.3）
② 依次判定三个维度（tenant → user → endpoint）
  ↓ 任一维度超限 → 429 + Retry-After + params.dimension，**直接返回**
③ 三个维度都通过 → 各写一行（同一个事务）→ 放行
```

### ⚠️ 读配置的失败语义（挑刺 M8）—— **必须写死，否则 1496 个测试会炸**

实测 `DbConfigResolver::resolve` 在两种情况下都返回 `None`
（`crates/platform-config/src/resolver.rs:237-239`）：
① 键**未登记**；② **缓存快照未 refresh**。

现有 1496 个测试的装配里**不会**登记 `rateLimit.*` 这些新键 → 中间件拿到 `None`。

**决定**：
```rust
// 三种情况一律 fail open（放行）+ WARN：
//   ① 读失败（DB 错误） ② 键未登记（None） ③ enabled 键不存在
let enabled = read_bool(cfg, "rateLimit.enabled").await.unwrap_or(false);
if !enabled {
    tracing::debug!("rate limiting is disabled");
    return next.run(request).await;   // 零查库开销
}
```

⚠️ **绝不能**「读不到就报错」——那会让 19 个已完成需求的 1496 个测试**集体变红**。
这是实现阶段最容易踩的坑，写在这里防止忘记。

⚠️ 配置读取走 `ConfigService::read_*`（**含 refresh**），不是裸 `resolve` ——
否则新建的键在缓存 refresh 之前永远读不到。

### ⚠️ 三个必须注意的顺序/原子性

1. **先判定、后记账**：如果先写行再判定，「超限的那次请求」也会被计入 →
   配额实际少 1 次。判据：上限 N 时，**第 N 次**必须 200、**第 N+1 次**才 429。
2. **记账必须在同一事务里**：三个维度各写一行，中途失败会导致
   「租户维度记了、用户维度没记」→ 配额漂移。
3. **`Retry-After` 的计算**：窗口是滑动的，最早的一条记录过期时就会释放配额。
   ```text
   retry_after = ceil(最旧一条记录的 occurred_at + windowSeconds - now())
   ```
   ⚠️ 必须是**正整数**（`max(1, ...)`），不能是 0 或负数（AC-S-032 判据）。

---

## 4. `trace_id` 流程

```
入站请求
  ↓
① 读 X-Request-Id → 校验（32 hex，大写归一）
  ↓ 合法 → 沿用；非法/缺失 → 生成（16 字节随机 → 32 hex）
② 写入 RequestContext.trace_id
③ 开一个 tracing span，带 trace_id 字段
  ↓ 该 span 内**所有**日志自动带 trace_id
④ 响应头回写 X-Request-Id
⑤ 错误响应体 .traceId / .trace_id 用同一个值
⑥ 审计事件 trace_id 用同一个值
```

### ⚠️ 校验的正则（AC-S-030）

```rust
fn normalize_trace_id(raw: &str) -> Option<String> {
    // 必须恰好 32 字符且全是 hex；大写归一为小写
    if raw.len() != 32 { return None; }
    if !raw.chars().all(|c| c.is_ascii_hexdigit()) { return None; }
    Some(raw.to_ascii_lowercase())
}
```

⚠️ **为什么 `raw.len() == 32`（字节数）在这里是安全的**：

`str::len()` 返回**字节数**，不是字符数 —— 一个含多字节 UTF-8 的串
（如 `"é"` 占 2 字节）可能**恰好** 32 字节但只有 31 个字符。

**但这里安全**，因为两个条件**同时**成立才通过：
1. `raw.len() == 32`（字节数 32）；
2. `raw.chars().all(|c| c.is_ascii_hexdigit())` —— **每个字符都必须是 ASCII hex**。

一条含任何非 ASCII 字符的串，第 2 条**必然**false（`is_ascii_hexdigit()` 对非 ASCII 返回 false）
→ 直接返回 `None`。

⚠️ **不要**把这两个条件拆开用（比如只留 `len() == 32`）——
那样 `"é"` 重复 16 次（32 字节、16 字符）会被当成合法 trace_id。
更稳妥的写法是先判 ASCII 再判长度，但当前顺序**在逻辑上也正确**，
实现时建议写成「先 `is_ascii()` 再 `len()`」以免读者误解。

---

## 4.2 测试怎么捕获日志（AC-S-029 步骤 5 的支撑）

⚠️ **这一步必须写清，否则 AC-S-029 步骤 5 不可执行。**

`platform-server/src/main.rs:1825` **已有**现成机制，直接复用：

```rust
fn capture_logs(filter: EnvFilter, emit: impl FnOnce()) -> String {
    let buf = BufWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(buf.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, emit);
    String::from_utf8(buf.0.lock().unwrap().clone()).unwrap()
}
```

用法（AC-S-029 步骤 5）：
```rust
let logs = capture_logs(log_filter("info"), || {
    // 在 with_default 的同步闭包里发请求 —— ⚠️ 不能 await
    // → 用 `tokio::runtime::Handle::current().block_on(...)` 或
    //   把请求逻辑放进 `std::thread::spawn` + join
});
assert!(logs.contains(&format!("trace_id={T}")));
```

⚠️ **注意点**（实现时容易踩）：
- `with_default` 是**同步**的，里面不能 `await` → 需要 `block_on` 或另起线程；
- 断言必须是 `trace_id=<T>` 这种**带等号的具体形态**（`fmt` subscriber 的输出格式），
  不能只断言 `logs.contains(&T)`（那样「日志里恰好有个 32 hex 串」也会绿）。

---

## 5. OTel 装配（AC-S-037）

### 5.1 可注入缝（挑刺 B10）—— **没有它 AC-S-037 不可执行**

```rust
/// `exporter_override` 为 `None` 时用生产 OTLP exporter；
/// 测试传 `Some(InMemorySpanExporter)` → 能取回 span。
pub fn init_otel(
    cfg: &OtelConfig,
    exporter_override: Option<Box<dyn SpanExporter>>,
) -> Option<OtelGuard> { ... }

/// 句柄：`is_enabled()` 供 AC-S-037 步骤 3 断言。
pub struct OtelGuard { /* provider */ }
impl OtelGuard {
    pub fn is_enabled(&self) -> bool { ... }
}
```

### 5.2 入站 `X-Request-Id` → OTel `TraceId`（挑刺 B10 的后半）

⚠️ 本需求要求「span 的 `trace_id` **逐字等于** 响应头 `X-Request-Id`」（AC-S-029 步骤 5）。
这**必须显式实现**，`tracing` + `tracing-opentelemetry` 默认**不**做这件事：

```rust
// ① 把 32 hex 解析成 OTel TraceId
let trace_id = opentelemetry::trace::TraceId::from_hex(hex_32)?;
// ② 用远端父上下文把入站值设为本次 trace 的根
let parent = Context::new().with_remote_span_context(SpanContext::new(
    trace_id, SpanId::INVALID, Sampler::AlwaysOn, None, false,
));
// ③ 在该 Context 里开 span
let span = tracer.build_with_context(Span::builder("http.request"), &parent).start();
```

⚠️ **不用 `SpanId::INVALID` 的替代方案**：如果实现直接开新 span（不设父上下文），
OTel 会**自己生成**一个 `trace_id` → 与 `X-Request-Id` 不等 → AC-S-029 步骤 5 红。
这是本需求**最容易做错**的一处。

---

```rust
// 只为说明结构，实现细节见代码
pub fn init_otel(cfg: &OtelConfig) -> Option<OtelGuard> {
    if cfg.disabled { return None; }                    // OTEL_SDK_DISABLED=true
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()                                     // HTTP/protobuf，不是 gRPC
        .with_timeout(Duration::from_secs(3))            // ⚠️ 必须有限超时
        .build().ok()?;                                  // 失败 → warn，不 panic
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)                   // ⚠️ 批量，有界队列
        .build();
    // tracing-opentelemetry Layer 挂进 subscriber
    Some(OtelGuard(provider))
}
```

### ⚠️ 测试怎么断言 span 结构（AC-S-037 步骤 1）

用 **`opentelemetry_sdk` 的 `InMemorySpanExporter`**（`opentelemetry_sdk::testing`）：

```rust
let exporter = InMemorySpanExporter::default();
let provider = SdkTracerProvider::builder()
    .with_simple_exporter(exporter.clone())   // 测试用 simple，不是 batch
    .build();
let spans = exporter.get_finished_spans().unwrap();
// 断言：至少 2 个 span；根 span 的 trace_id 是 32 hex；
//       根 span 的 trace_id == 响应头 X-Request-Id
```

⚠️ 这是**测试专用**路径（`with_simple_exporter`），生产用 `with_batch_exporter`。
规格里必须区分这两者，否则实现会拿 simple 上生产（失去有界队列保护）。

### ⚠️ 三条硬约束

1. **必须有限超时**：collector 不可达时无限等会让请求挂住（AC-S-037 步骤 2）；
2. **必须批量 + 有界队列**：满了丢弃并 warn，不能阻塞业务；
3. **失败只打日志**：不 `panic`、不把错误返回给调用方。

---

## 6. OpenAPI 生成（AC-S-035）

```rust
// platform-observability/src/openapi.rs
#[derive(OpenApi)]
#[openapi(
    info(title = "通用项目底座 API", version = env!("CARGO_PKG_VERSION")),
    paths(...),          // 各 crate 的 #[utoipa::path] 汇总
    components(schemas(...))
)]
pub struct ApiDoc;
```

`cargo xtask openapi` 子命令：
- 无参数 → 写 `openapi.json`（格式化后）；
- `--check` → 生成到内存，与磁盘文件**逐字节比较**，不一致则非零退出并指出差异。

### ⚠️ 为什么 `--check` 要逐字节比

如果只比「文件是否存在」或「能否解析」，
**代码改了但产物没更新**的情况会绿 —— 那 `--check` 就白加了（AC-S-035 步骤 5 专门验）。

---

## 7. 弃用头（AC-S-036）

```rust
/// 端点弃用标记（由路由层注入响应头）。
pub struct Deprecation {
    pub sunset: OffsetDateTime,   // → IMF-fixdate
}

// 中间件：遇到带 Deprecation 扩展的路由 → 加两个头
//   Deprecation: true
//   Sunset: Sun, 31 Dec 2026 23:59:59 GMT
```

⚠️ IMF-fixdate 手写格式化（`time` crate 有 `Rfc7231` 格式，正好是 IMF-fixdate）：
```rust
sunset.format(&time::format_description::well_known::Rfc7231)
// → "Sun, 31 Dec 2026 23:59:59 GMT"
```

---

## 8. 分页契约（AC-S-034）

```rust
pub struct PageQuery {
    pub limit: Option<i64>,      // 1..=100，默认 20
    pub cursor: Option<String>,  // 不透明
}

pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,  // serde: nextCursor
    pub total: i64,
}
```

⚠️ `limit` 校验在**反序列化之后、查询之前**：
`limit <= 0 || limit > 100` → `INVALID_REQUEST`（400），**不静默截断**。

---

## 9. 定期任务清单（**新增的必须记账**）

| 任务 | 频率 | 为什么 | 实现 |
|---|---|---|---|
| 限流记录清理 | 每小时 | 每请求一行，不清理会爆（D337） | `xtask observability prune-rate-limit` |

⚠️ 这张表是给运维看的 —— 新增定期任务**必须**登记在这里，否则没人知道要配 cron。

---

## 10. 端点清单（本需求新增/修改）

| # | 方法 | 路径 | 说明 | AC |
|---|---|---|---|---|
| 1 | `GET` | **`/health/ready`** | **修改**：就绪检查加「限流表可达」（⚠️ 是 `/health/*` 不是 `/api/health/*`） | **AC-S-038** |
| 2 | `GET` | `/api/audit/events` | **修改**：对齐分页契约 + **标注弃用**（见 AC-S-036） | AC-S-034 / AC-S-036 |
| 3 | `GET` | `/api/openapi.json` | **新增**：产出 OpenAPI 文档 | AC-S-035 |
| 4 | `GET` | **`/api/observability/rate-limit/status`** | **新增**：限流配置与当前用量（**这是 AC-S-034 依赖的「新分页端点」**） | AC-S-034 |
| 5 | `GET` | `/api/rbac/roles` | **修改**：加 `#[utoipa::path]` + 作为弃用头的**阴性对照** | AC-S-035 / AC-S-036 |

⚠️ **rev 2 修正（挑刺 B5 / B6 / M10）**：
- 原第 1 行路径写成 `/api/health/ready` → 实际是 `/health/ready`（`main.rs:290-291`）；
- 原端点清单里**没有**任何「新增的分页列表端点」，但 AC-S-034 依赖它 → 补第 4 行；
- 删掉「测试专用端点 `/api/_deprecated-probe` + 考虑是否在生产排除」——
  冻结的规格里留「考虑」等于执行时无人能判对错。改为**在真实端点
  `/api/audit/events` 上标注弃用**（理由：分页契约升版给了它合理的弃用理由）。

### 10.1 `/api/observability/rate-limit/status` 的响应

```json
{
  "items": [
    { "dimension": "endpoint", "key": "GET:/api/rbac/roles",
      "limit": 1, "windowSeconds": 60, "used": 1 }
  ],
  "nextCursor": null,
  "total": 3
}
```

⚠️ 这个端点**本身也受限流保护**（它走同一个中间件）——
所以在 AC-S-034 里测分页时，必须先把 `rateLimit.enabled` 设为 `false`
或用足够高的上限，否则会被自己的限流拦掉。

---

## 11. 跨栈字段（camelCase，D258）

| JSON | 说明 |
|---|---|
| `items` | 分页数据 |
| `nextCursor` | 下一页游标（`null` = 没有下一页） |
| `total` | 总数 |
| `trace_id` | ⚠️ **只有这一种**：错误响应体（`ProblemDetails.trace_id`，`problem.rs:104`）用它。**业务成功响应体不加任何 trace 字段**（那要改 REQ-011 已验收的 DTO 契约 → 挑刺 B8）。rev 1 写的「业务响应体用 `traceId`。不改现有契约」**自相矛盾**（要加字段就是改契约） |
| `dimension` | 限流的维度（`params.dimension`） |

---

## 11.5 **必须新增**：可测的路由装配（挑刺 M5）

### 问题

`crates/platform-server/src/lib.rs:9-20` 只导出模块，**整个 `Router` 在
`main.rs:289` 起内联装配近 1500 行**。既有全栈测试是**手工复刻**，
并自己注释「与 `main.rs` 相同的挂法」（`tests/app_real_stack.rs:227`）。

→ 6 条 HTTP 级 AC（029/030/031/032/036/037）跑在**测试自造的栈**上，
**中间件漏装 / 顺序错都能绿** —— 这正是 `platform-core/src/log.rs:1-20`
写过的教训（「测过的东西与线上跑的不是同一份代码 → 假绿」）。

### 决定

抽出：

```rust
// platform-server/src/app.rs
pub struct AppDeps { /* 各 service、config、限流器、otel guard… */ }

/// 装配**完整**路由（含全部中间件，顺序与生产一致）。
pub fn build_router(deps: AppDeps) -> axum::Router { ... }
```

- `main.rs` 只负责「读配置 → 构造 deps → build_router → serve」；
- 所有 HTTP 级测试调 `build_router(测试 deps)` —— **与生产同一条装配路径**。

⚠️ 这是对 `platform-server` 的**结构性改动**（不是加一行），
必须列进 §12 的跨需求改动表并标「**中**」风险。

---

## 12. 与现有代码的交互点（**跨需求改动，全部声明 D109**）

⚠️ **rev 2 修正（挑刺 B2 / E）**：rev 1 曾写「`platform-audit` 的分页改动是
**唯一**会动到已验收需求的地方」—— **这是假话**。实际至少 **6 处**动到已验收需求：

| # | 改动 | 影响的 crate | 风险 | 会动到哪个已验收需求 |
|---|---|---|---|---|
| 1 | **`build_router` 抽取**（§11.5） | `platform-server` | **中**（结构改动） | 全部 19 个（改装配路径） |
| 2 | **`limit` 超上限从夹取改为 400**（D341） | `platform-audit` | **中** | **REQ-014**（单测 `query.rs:646-652` 锁死了夹取行为） |
| 3 | **`new_trace_id()` 从 36 字符 uuid 改为 32 hex** | `platform-core` | **中**（对外可见格式变化） | 全部（`context.rs:156` 有单测断言 `len()==36`） |
| 4 | **`init_tracing` 抽出 `build_subscriber(level, writer)`** | `platform-server` | 低 | REQ-002（启动路径） |
| 5 | **限流中间件挂进 server** | `platform-server` | **中**（默认关闭 + fail open） | 全部（若 fail open 写错会集体红） |
| 6 | **各 crate 路由加 `#[utoipa::path]`** | `platform-app` / `rbac` / `audit` 等 | 低（纯标注） | 无行为变化，但改源码 |

### ⚠️ 明码**不动**的已验收契约（rev 1 曾误写成会动）

| 项 | rev 1 怎么说 | rev 2 决定 | 为什么 |
|---|---|---|---|
| **审计表加 `trace_id`** | 要求断言 `platform_audit_log.trace_id` | **不动** → **TD-071** | 契约 C12 是 13 个冻结字段；加列要改在线表 + 归档表 + `platform_audit_append` 签名 + **哈希公式** → 断历史链，破坏 REQ-014 的链校验 |
| **业务成功响应体加 `traceId`** | 要求 `POST /api/apps` 的 201 体带 `.traceId` | **不动**（改用失败响应体的 `ProblemDetails.trace_id`） | `AppView` 等 DTO 无 trace 字段，加它要改 **REQ-011** 已验收的响应契约 |
| **错误响应媒体类型改 `problem+json`** | 要求产物里含 `application/problem+json` | **不动**，改为断言 `application/json` | 实际就是 `application/json`（`problem.rs:305`），且 **D183** 已把「要求 problem+json」这条判据**当成 BA 写错修正掉了** |
| **`limit` 上限 200 → 100** | 隐含 | **不动**，保持 200 | 减少破坏面；只改「超上限」的行为 |

### 实现时必须一起改的既有测试（逐个点名，不许漏）

| 文件:行 | 函数 | 怎么改 |
|---|---|---|
| `crates/platform-audit/src/query.rs:646-652` | `an_oversized_limit_is_clamped_instead_of_rejected` | 改名为 `an_oversized_limit_is_rejected`，断言 400 |
| `crates/platform-core/src/context.rs:156` | 断言 `trace_id.len() == 36` | 改为 `== 32` 且断言全 hex |
| `docs/contracts/api-conventions/v1.md:118-119`（C7） | 「超上限夹到 200」 | 改为「超上限 400」 |
| `deploy/RUNBOOK.md` 里的 trace_id 示例 | 36 字符 uuid 形态 | 改为 32 hex |

⚠️ 这四处在**实现阶段的提交信息里必须点名** —— 它们是「改了已验收行为」的证据。
