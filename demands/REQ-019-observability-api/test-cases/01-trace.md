# 用例：trace_id 贯穿与安全校验（AC-S-029 / AC-S-030）

> 覆盖 TC-019-001 ~ 013（13 条）
> 层次：单元（校验逻辑）+ 集成（全栈，`app_real_stack` 同款装配）

---

## AC-S-029：trace_id 在日志 / 响应体 / OTel span 三处同值（5 条）

### TC-019-001 生成的 trace_id 是 32 位小写 hex

- **前置**: 无（纯函数）
- **步骤**: 调 `new_trace_id()` 100 次
- **判据**:
  - 每次长度都 == 32 且全为 `0-9a-f`
  - 100 次两两不同（无碰撞；16 字节随机下碰撞概率可忽略）
- **断言形态**: `assert_eq!(t.len(), 32) && assert!(t.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))`
- **⚠️ 注意**: 现有 `context.rs:156` 断言 `len()==36`（uuid 形态），**本用例会与之冲突**
  → 实现时那条既有断言必须同步改成 32（`design-summary.md` §12 已点名）

### TC-019-002 响应头必有 X-Request-Id

- **前置**: 全栈装配；`POST /api/apps` 合法 body
- **步骤**: 发请求（不带任何 X-Request-Id）
- **判据**: 响应头 `X-Request-Id` 存在，且是 32 位小写 hex
- **断言形态**: `assert_eq!(header.len(), 32)` + hex 检查

### TC-019-003 失败响应体的 .trace_id 等于响应头

- **前置**: 同 002
- **步骤**: 发 `POST /api/apps` 缺必填字段 → 得 4xx
- **判据**: `response.trace_id == header["X-Request-Id"]`（**逐字相等**）
- **为什么用失败响应**: `ProblemDetails.trace_id` 已存在（`problem.rs:104`）；
  业务成功 DTO（`AppView`）无 trace 字段，加它要改 REQ-011 契约
- **断言形态**: `assert_eq!(body.trace_id, T)`

### TC-019-004 日志里带 trace_id=<T>

- **前置**: 用 `build_subscriber(level, writer)` + `capture_logs`
- **步骤**: 在 `with_default` 内发一次请求（`current_thread` runtime，避免跨线程丢 span）
- **判据**: 捕获的日志里**至少一行**含 `trace_id=<T>`
- **断言形态**: `assert!(logs.contains(&format!("trace_id={T}")))`
- **⚠️ 不能只断言** `logs.contains(&T)` —— 那样「日志里恰好有个 32 hex 串」也会绿

### TC-019-005 OTel span 的 trace_id 等于响应头

- **前置**: `init_otel(cfg, Some(InMemorySpanExporter))`
- **步骤**: 发一次请求 → 从导出器取 span
- **判据**:
  - 至少 1 个 span；其 `trace_id.to_string()` == `header["X-Request-Id"]`
  - span 层级 ≥ 2 层（HTTP 根 span + 子 span）
- **断言形态**: `assert_eq!(span.span_context.trace_id().to_string(), T)`
- **⚠️ 实现要点**: 必须用 `TraceId::from_hex` + 远端父上下文，
  否则 OTel 会自己生成 id → 本用例必红（`design-summary.md` §5.2）

---

## AC-S-030：入站 X-Request-Id 沿用与安全校验（8 条）

### TC-019-006 合法 32 hex 被沿用

- **步骤**: 带 `0123456789abcdef0123456789abcdef`
- **判据**: 响应头 **逐字相等** 该值
- **断言形态**: `assert_eq!(header, "0123456789abcdef0123456789abcdef")`

### TC-019-007 大写被归一为小写

- **步骤**: 带 `ABCDEF0123456789ABCDEF0123456789`
- **判据**: 响应头 == `abcdef0123456789abcdef0123456789`
- **断言形态**: `assert_eq!(header, tiny)` 且 `assert_ne!(header, raw)`
- **区分力**: 若实现直接透传 → `assert_ne!` 红

### TC-019-008 太短的值被替换

- **步骤**: 带 `abc`
- **判据**: 响应头 != `abc`，且是合法 32 hex（新生成）
- **断言形态**: `assert_ne!(header, "abc")` + hex 检查

### TC-019-009 路径穿越字符被拒绝

- **步骤**: 带 `../../etc/passwd`
- **判据**: 响应头 != 该值，且是合法 32 hex
- **断言形态**: `assert_ne!(header, bad)` + hex 检查

### TC-019-010 CRLF 注入被拒绝（**安全核心**）

- **步骤**: 带 `"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\nFAKE LOG LINE"`
- **判据**:
  - 响应头 != 该值
  - 捕获日志全文**不含** `FAKE LOG LINE`
- **断言形态**: `assert_ne!(header, bad)` + `assert!(!logs.contains("FAKE LOG LINE"))`
- **⚠️ 与 009 是两个不同向量**: 009 防路径穿越，010 防日志注入。
  只过滤 `/` 的实现能过 009 但挂在 010

### TC-019-011 含非法字符的 32 长度值被拒绝

- **步骤**: 带 32 个 `g`（长度对、但不是 hex）
- **判据**: 响应头 != 该值（新生成）
- **断言形态**: `assert_ne!(header, "g"*32)`
- **区分力**: 只判长度的实现会通过 → 本用例抓它

### TC-019-012 含多字节 UTF-8 的值被拒绝

- **步骤**: 带 `"é".repeat(16)`（32 **字节**但 16 字符）
- **判据**: 响应头 != 该值
- **断言形态**: `assert_ne!(header, bad)`
- **⚠️ 这条专门验 D340 的「先判 ASCII 再判长度」**：
  只判 `len()==32` 的实现会让它通过 → 红

### TC-019-013 成功与失败响应都回写头

- **步骤**: 分别发一个 2xx 与一个 4xx
- **判据**: 两者的 `X-Request-Id` 都存在且格式合法
- **断言形态**: 两次 `assert!(header exists)` + hex 检查
- **区分力**: 只在成功路径写头的实现 → 4xx 那次红

---

## 判据校准清单（实现后回填真实行号）

| 用例 | 目标文件（预设） | 真实行号 |
|---|---|---|
| TC-019-001 | `crates/platform-observability/src/trace/mod.rs` | 待填 |
| TC-019-002~005 | `crates/platform-server/tests/trace_real_stack.rs` | 待填 |
| TC-019-006~013 | `crates/platform-observability/src/trace/mod.rs`（单测） | 待填 |
