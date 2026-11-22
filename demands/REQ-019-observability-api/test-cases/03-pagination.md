# 用例：分页契约统一（AC-S-034）

> 覆盖 TC-019-035 ~ 044（10 条）
> ⚠️ 本组用例验的是**契约 C7 / D153 升版后**的行为（超上限 400，不再是夹取）

## 公共前置

- `platform_audit_log` 里有 **25 行** `object_type = 'e2e_fixture'`（`tenant-a`）
- 新端点 `GET /api/observability/rate-limit/status` 有 ≥25 行
- **`rateLimit.enabled = false`**（⚠️ 否则新端点会被自己的限流拦掉）

---

### TC-019-035 limit=10 返回恰好 10 条 + nextCursor + total

- **步骤**: `GET /api/audit/events?limit=10`
- **判据**: `items.len() == 10`；`nextCursor` 非空；`total == 25`
- **断言**: `assert_eq!(items.len(), 10)` / `assert!(next_cursor.is_some())` / `assert_eq!(total, 25)`

### TC-019-036 用 nextCursor 翻第 2 页零重叠

- **步骤**: 用 035 的 `nextCursor` 再请求
- **判据**: 返回 10 条，且 id 与第 1 页**零重叠**
- **断言**: `assert!(set1.is_disjoint(&set2))`

### TC-019-037 第 3 页 5 条且 nextCursor 为 null

- **判据**: `items.len() == 5`；`nextCursor == null`
- **断言**: `assert_eq!(items.len(), 5)` / `assert!(next_cursor.is_none())`

### TC-019-038 limit=0 → 400 + INVALID_REQUEST

- **步骤**: `GET /api/audit/events?limit=0`
- **判据**: 400；`code == "INVALID_REQUEST"`
- **⚠️ 与 D153 的差异**: 原来是 `AUDIT_INVALID_FILTER` → 已声明升版
- **断言**: `assert_eq!(status, 400)` / `assert_eq!(code, "INVALID_REQUEST")`

### TC-019-039 limit=1000 → 400（**不再夹取**）

- **步骤**: `GET /api/audit/events?limit=1000`
- **判据**: 400；`code == "INVALID_REQUEST"`
- **⚠️ 这是对 D153 的破坏性修正**: 原实现夹到 200 返回 200
- **断言**: `assert_eq!(status, 400)`
- **⚠️ 必须同时改**: `query.rs:646-652` 的 `an_oversized_limit_is_clamped_instead_of_rejected`
  → 改名 `an_oversized_limit_is_rejected`

### TC-019-040 limit=200 边界（含）

- **步骤**: `?limit=200`
- **判据**: 200（上限 200 **含**边界，不是 `>=`）
- **断言**: `assert_eq!(status, 200)`
- **区分力**: 实现写成 `>= MAX` → 200 被拒 → 红

### TC-019-041 ?page=1&page_size=10 仍可用（**回归判据**）

- **步骤**: `GET /api/audit/events?page=1&page_size=10`
- **判据**: 200，且 `items.len() == 10`
- **断言**: `assert_eq!(status, 200)` / `assert_eq!(items.len(), 10)`
- **⚠️ 为什么必须有**: 证明「统一契约」**没有破坏**已验收的兼容分页方式

### TC-019-042 非法 cursor → 400 不是 500

- **步骤**: `?cursor=not-a-valid-cursor`
- **判据**: 400；`code == "INVALID_REQUEST"`；`detail` 说明 cursor 非法
- **断言**: `assert_eq!(status, 400)` / `assert_ne!(status, 500)`
- **区分力**: 非法输入当成服务器错误 → 500 → 红

### TC-019-043 新端点字段名与 audit 完全一致

- **步骤**: 分别请求两个端点
- **判据**: 两者的响应都**含** `items` / `nextCursor` / `total`
  （⚠️ 写「包含」，D194 —— 允许额外字段，但字段名必须相同）
- **断言**: 对每个端点 `assert!(json.get("items").is_some())` 等三条

### TC-019-044 camelCase 契约

- **判据**: 响应体**不含** `next_cursor`（snake_case）；含 `nextCursor`
- **断言**: `assert!(json.get("next_cursor").is_none())` / `assert!(json.get("nextCursor").is_some())`
- **关联**: D258 跨栈字段规范

---

## 判据校准清单

| 用例段 | 目标文件（预设） |
|---|---|
| TC-019-035~044 | `crates/platform-audit/tests/pagination_contract_real_db.rs` |
| limit 校验单测 | `crates/platform-audit/src/query.rs`（改既有测试） |
