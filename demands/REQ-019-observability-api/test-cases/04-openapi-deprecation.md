# 用例：OpenAPI 一致性与弃用头（AC-S-035 / AC-S-036）

> 覆盖 TC-019-045 ~ 057（13 条）

---

## AC-S-035：OpenAPI 产物与代码一致（7 条）

### TC-019-045 干净树上 --check 退出码 0

- **前置**: `cargo xtask openapi` 已生成 `openapi.json`
- **步骤**: `cargo xtask openapi --check`
- **判据**: 退出码 0；输出含「与源码一致」
- **断言**: `assert!(output.status.success())` + `assert!(stdout.contains("一致"))`

### TC-019-046 openapi 版本是 3.1.0

- **判据**: 产物 `openapi == "3.1.0"`
- **断言**: `assert_eq!(json["openapi"], "3.1.0")`
- **区分力**: 3.0.x 是旧版，`assert_eq` 会红

### TC-019-047 paths 含 /health/live 与 /health/ready（B6 修正）

- **判据**: `paths` 里含这两个键
- **断言**: `assert!(json["paths"].get("/health/live").is_some())`
- **⚠️ 是 `/health/*` 不是 `/api/health/*`**（实测 `main.rs:290-291`）

### TC-019-048 改产物 → --check 非 0（**阳性对照 1**）

- **步骤**: 手工把 `openapi.json` 的 `info.title` 改掉 → `--check`
- **判据**: 退出码非 0，且输出指出不一致的字段
- **断言**: `assert!(!output.status.success())`
- **⚠️ 为什么必须有**: 证明 `--check` 真在比对，不是恒返回 0

### TC-019-049 改 DTO 字段名 → --check 非 0 且有差异行（**阳性对照 2**，M9 修正）

- **步骤**: 把 `AppView.display_name` 改成 `displayNameProbe` → 编译 → `--check` → **改回**
- **判据**: 退出码非 0，且输出含**差异行**
- **断言**: `assert!(!success)` + `assert!(stdout.contains("---") || stdout.contains("+++"))`
- **⚠️ rev 2 修正**: rev 1 用「注释掉 `#[utoipa::path]`」，但那会**编译失败**
  而以非 0 退出 → 判据红/绿与 `--check` 逻辑无关，等于没验

### TC-019-050 每个 path 的响应含 application/json（B7 修正）

- **判据**: 各 path 的 `responses.*.content` 里含 `application/json`
- **断言**: `assert!(content.get("application/json").is_some())`
- **⚠️ 不是 `application/problem+json`** —— 实际媒体类型是 `application/json`
  （`problem.rs:305`），且 **D183** 已把这条判据修正掉

### TC-019-051 info.title / info.version 非空

- **判据**: 两者都非空字符串
- **断言**: `assert!(!title.is_empty())` / `assert!(!version.is_empty())`

---

## AC-S-036：弃用头（6 条）

**公共前置**: `GET /api/audit/events` 已标注弃用（`Sunset = 2027-06-30`）；
阴性对照 `GET /api/rbac/roles` 在**同一 router**（m4）

### TC-019-052 弃用端点带 Deprecation: true

- **步骤**: `GET /api/audit/events`
- **判据**: 响应头 `Deprecation` **逐字等于** `true`
- **断言**: `assert_eq!(header, "true")`
- **⚠️ D342 统一选 `true`**，不用日期形态

### TC-019-053 Sunset 是严格 IMF-fixdate（RFC 8594）

- **判据**: `Sunset` 形如 `Wed, 30 Jun 2027 00:00:00 GMT`
- **断言**: 用 `time::format_description::well_known::Rfc7231` **反解成功**
  （⚠️ 反解比正则更强：ISO8601 与 Unix 时间戳都解不开 → 红）
- **断言形态**: `assert!(PrimitiveDateTime::parse(&v, &Rfc7231).is_ok())`

### TC-019-054 未弃用端点两个头都不出现（**阴性对照**）

- **步骤**: `GET /api/rbac/roles`
- **判据**: `Deprecation` 与 `Sunset` **都不存在**
- **断言**: `assert!(headers.get("Deprecation").is_none())` / 同 `Sunset`
- **⚠️ 必须同一 router**: 否则「按 router 整体加头」的实现在另一个 router
  的对照上仍会绿

### TC-019-055 弃用端点仍是 200（弃用 ≠ 下线）

- **判据**: 状态码 **200**
- **断言**: `assert_eq!(status, 200)`
- **区分力**: 实现立刻返回 410 → 红

### TC-019-056 Deprecation 不是其它值

- **判据**: 头值**不等于** `true` 之外的任何形态（如 `"1"`、日期串）
- **断言**: `assert_eq!(header, "true")` + `assert_ne!(header, "2027-01-01")`
- **关联**: 与 052 互补，确保实现没换形态

### TC-019-057 两个头只在**标记了弃用**的路由上出现

- **步骤**: 遍历 router 里所有路由，统计带 `Deprecation` 的
- **判据**: 恰好 **1 个**路由带（即 `/api/audit/events`）
- **断言**: `assert_eq!(routes_with_header.len(), 1)`
- **区分力**: 实现若全局加头 → 数量远大于 1 → 红

---

## 判据校准清单

| 用例段 | 目标文件（预设） |
|---|---|
| TC-019-045~051 | `crates/xtask/src/openapi.rs` 的测试 + `crates/platform-server/tests/openapi_product.rs` |
| TC-019-052~057 | `crates/platform-server/tests/deprecation_real_stack.rs` |
| Sunset 格式化 | `crates/platform-observability/src/deprecation.rs`（单测） |
