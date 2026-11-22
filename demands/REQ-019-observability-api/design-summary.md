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

## 3. 限流判定流程

```
请求进来
  ↓
① 读配置（rateLimit.enabled？三个维度的 requests / windowSeconds）
  ↓ enabled=false → 直接放行（零开销，不查库）
② 依次判定三个维度（tenant → user → endpoint）
  ↓ 任一维度超限 → 429 + Retry-After + params.dimension，**直接返回**
③ 三个维度都通过 → 各写一行（同一个事务）→ 放行
```

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

## 5. OTel 装配（AC-S-037）

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
| 1 | `GET` | `/api/health/ready` | **修改**：就绪检查加「限流表可达」 | AC-S-031 前置 |
| 2 | `GET` | `/api/audit/events` | **修改**：对齐分页契约 | AC-S-034 |
| 3 | `GET` | `/api/openapi.json` | **新增**：产出 OpenAPI 文档 | AC-S-035 |
| 4 | `GET` | `/api/_deprecated-probe` | **新增**：测试用弃用端点（标 `#[deprecated]` + 头） | AC-S-036 |

⚠️ 第 4 个端点是**测试专用**，必须在实现里注明「仅为验弃用头存在」，
并考虑是否在生产构建中排除。

---

## 11. 跨栈字段（camelCase，D258）

| JSON | 说明 |
|---|---|
| `items` | 分页数据 |
| `nextCursor` | 下一页游标（`null` = 没有下一页） |
| `total` | 总数 |
| `traceId` / `trace_id` | ⚠️ **两种都存在**：`ProblemDetails` 用 `trace_id`（现有契约），业务响应体用 `traceId`。**不改现有契约**（D109） |
| `dimension` | 限流的维度（`params.dimension`） |

---

## 12. 与现有代码的交互点（**跨需求改动，必须声明 D109**）

| 改动 | 影响的 crate | 风险 |
|---|---|---|
| `RequestContext` 增加 trace_id 校验入口 | `platform-core` | 低（新增方法，不改签名） |
| `init_tracing` 加 trace_id Layer | `platform-server` | 低 |
| `platform-server` 装配限流中间件 | `platform-server` | **中**（默认关闭，避免打断现有测试） |
| `platform-audit` 的 `QueryPage` 对齐契约 | `platform-audit` | **中**（改响应字段 → 现有测试要跟着改） |
| 各 crate 路由加 `#[utoipa::path]` | 多个 | 低（纯标注，不改行为） |
| `Cargo.lock` 净增 20~29 包 | 全仓 | **依赖破例**（D338/D339） |

⚠️ **`platform-audit` 的分页改动是唯一会动到已验收需求的地方** ——
必须跑它的全量测试并在提交信息里点명。
