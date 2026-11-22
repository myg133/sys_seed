# 用例：三级限流（AC-S-031 / AC-S-032 / AC-S-033）

> 覆盖 TC-019-014 ~ 034（21 条）
> 层次：集成（真库）。**每个场景开头必须 truncate `platform_rate_limit`**

---

## AC-S-031：三级各自独立（10 条）

**公共前置**: `rateLimit.enabled = true`；四个端点
`GET /api/rbac/roles`、`GET /api/apps`、`GET /api/audit/events`、
`GET /api/observability/rate-limit/status`；用户 `user-a`/`user-b`（tenant-a）、
`user-c`（tenant-b）。

### TC-019-014 接口维度：第 2 次 429（dimension=endpoint）

- **配置**: `endpoint.requests=1`，`user=0`，`tenant=100`，window 60
- **步骤**: truncate → user-a 连打 `/api/rbac/roles` 2 次
- **判据**: 第 1 次 200；第 2 次 429 且 `params.dimension == "endpoint"`
- **断言**: `assert_eq!(r1, 200)` / `assert_eq!(r2, 429)` / `assert_eq!(dim, "endpoint")`

### TC-019-015 用户维度：第 3 次 429（dimension=user）

- **配置**: `endpoint.requests=0`（**禁用**），`user.requests=2`，`tenant=100`
- **步骤**: truncate → user-a 打 3 个**不同**端点各 1 次
- **判据**: 前 2 次 200；第 3 次 429 且 `dimension == "user"`
- **区分力**: 若 endpoint 维度没真禁用 → 第 2 次就会因 endpoint 429 → `dimension` 不匹配 → 红

### TC-019-016 用户维度不因换端点而重置

- **配置**: 同 015
- **步骤**: user-a 打 `/api/apps` → `/api/rbac/roles`（两个不同桶）
- **判据**: 第 3 次（任一端点）429，说明用户维度是**跨端点累计**的
- **断言**: `assert_eq!(third, 429)` + `dimension == "user"`

### TC-019-017 租户维度：第 4 次 429（dimension=tenant）

- **配置**: `tenant.requests=3`，`user=0`，`endpoint=0`
- **步骤**: truncate → user-a 与 user-b 交替打**互不相同**的端点共 5 次
- **判据**: 第 1~3 次 200；第 4 次起 429，`dimension == "tenant"`
- **区分力**: 若另两个维度没禁用 → 可能提前 429 且 dimension 不对 → 红

### TC-019-018 跨租户隔离：tenant-b 不受 tenant-a 限流影响（**B1 修正的核心**）

- **配置**: `endpoint.requests=1`，`user=0`，`tenant=100`
- **步骤**: truncate → user-a 打满 tenant-a 的 `/api/rbac/roles`（2 次，第 2 次 429）
  → **tenant-b 的 user-c** 打同一路径 1 次
- **判据**: user-c **200**
- **断言**: `assert_eq!(user_c, 200)`
- **⚠️ 这是 rev 2 的关键修正**: rev 1 要求「同租户 user-b 仍 200」，
  但 endpoint 桶本就按租户共享 → **实现正确时必红**（挑刺 B1）

### TC-019-019 同租户不同用户共享 endpoint 桶（**确认 B1 的语义**）

- **配置**: 同 018
- **步骤**: truncate → user-a 打 1 次（200）→ user-b 打同一端点 1 次
- **判据**: user-b **429**，`dimension == "endpoint"`
- **断言**: `assert_eq!(user_b, 429)`
- **⚠️ 这条是「反向判据」**: 明确记录「endpoint 维度是租户内共享的」这个语义。
  如果实现把 user_id 拼进 endpoint 键 → 这条红（说明键设计错了）

### TC-019-020 query 不绕桶：?x=1 与 ?x=2 共用

- **配置**: `endpoint.requests=1`，其余禁用
- **步骤**: truncate → user-a 打 `/api/rbac/roles?x=1` → `/api/rbac/roles?x=2`
- **判据**: 第 2 次 **429**
- **断言**: `assert_eq!(second, 429)`
- **区分力**: 用完整 URI 做键 → 第 2 次 200 → 红
- **可行性**: `MatchedPath` 已实测（`?x=1` → `/api/rbac/roles`）

### TC-019-021 不同方法算不同桶

- **配置**: `endpoint.requests=1`
- **步骤**: truncate → `GET /api/apps` 1 次 → `POST /api/apps` 1 次
- **判据**: 两次都 200（方法不同=不同桶）
- **断言**: `assert_eq!(get, 200)` / `assert_eq!(post != 429)`
- **⚠️ 注意**: POST 可能因业务原因非 200，判据写「**不是 429**」而非「== 200」

### TC-019-022 enabled=false 时完全不限流

- **配置**: `enabled=false`，三个维度都设成 1
- **步骤**: truncate → 连打同一端点 5 次
- **判据**: 全部**不是 429**；且 `platform_rate_limit` **零行**（证明没查库/没记账）
- **断言**: `assert!(!results.contains(&429))` + `assert_eq!(count_rows, 0)`

### TC-019-023 键未登记时 fail open（M8）

- **配置**: 数据库里**没有** `rateLimit.enabled` 这个键（模拟新键未迁移）
- **步骤**: 连打同一端点 5 次
- **判据**: 全部**不是 429**（fail open），且日志里有 WARN
- **断言**: `assert!(!results.contains(&429))` + `assert!(logs.contains("WARN"))`
- **区分力**: 若实现「读不到就报错」→ 5 次全 500 → 红。
  **这条保护了现有 1496 个测试**

---

## AC-S-032：429 响应语义（7 条）

**公共前置**: `endpoint.requests=1`、`endpoint.windowSeconds=2`、
`tenant=0`、`user=0`（另两维度禁用），端点 `/api/rbac/roles`

### TC-019-024 Retry-After 存在且是正整数

- **步骤**: 第 2 次请求 → 读头
- **判据**: `Retry-After` 存在，解析为整数 ≥ 1
- **断言**: `assert!(v.parse::<u64>().unwrap() >= 1)`

### TC-019-025 Retry-After 有上界（m5）

- **步骤**: 同 024
- **判据**: `1 ≤ retryAfter ≤ windowSeconds + 1`（窗口 2 → 只能是 1/2/3）
- **断言**: `assert!(retry <= 3)`
- **区分力**: 返回 999999 也「是正整数」→ 无上界就漏

### TC-019-026 code == RATE_LIMITED

- **判据**: 响应体 `code == "RATE_LIMITED"`
- **断言**: `assert_eq!(body.code, "RATE_LIMITED")`

### TC-019-027 params 只有 dimension 与 retryAfter（M2 键白名单）

- **步骤**: 解析 429 响应体
- **判据**: `params` 的**键集合** ⊆ `{"dimension", "retryAfter"}`
- **断言**: `assert!(keys.is_subset(&expected))`
- **⚠️ 必须用「子集」而不是「包含」** —— 这是唯一能抓住
  「多返回 `current`/`remaining`/`bucket`」的写法

### TC-019-028 detail/title 不含实现细节黑名单（M2）

- **判据**: `detail` 与 `title` 小写化后**不含**：
  `platform_` / `select ` / `postgres` / `sqlx` / `occurred_at` / `tenant_id`
- **断言**: 逐个 `assert!(!lower.contains(x))`
- **⚠️ 不写「PG 相关字样」这种无界集合**（写不成断言）

### TC-019-029 窗口过期后立即恢复

- **步骤**: 第 2 次 429 → 等 3 秒 → 第 3 次
- **判据**: 第 3 次 **200**
- **断言**: `assert_eq!(third, 200)`
- **区分力**: 「限流」与「封禁」的分界 —— 永久封禁的实现 → 红

### TC-019-030 429 响应不带任何跨请求的状态残留

- **步骤**: 同一场景跑两遍（中间 truncate）
- **判据**: 两遍的 429 响应体结构**完全一致**（键集合相同）
- **断言**: `assert_eq!(keys_run1, keys_run2)`
- **区分力**: 实现若把「已用次数」塞进 params，两遍可能不同 → 红

---

## AC-S-033：跨实例准确（4 条）

**公共前置**: 两个**独立** `PgPool`（`pool_a` / `pool_b`）；`endpoint.requests=1`，window 60

### TC-019-031 两个池确实是不同的池（M1）

- **判据**: `pool_a.id() != pool_b.id()`
- **断言**: `assert_ne!(pool_a.id(), pool_b.id())`
- **⚠️ 这是前置的自我校验**: rev 1 用「`max_connections` 不同」当判据，
  那是**无区分力的代理断言**（同一个池也不能有两个 max_connections）

### TC-019-032 实例 A 打 1 次 → 200

- **步骤**: truncate → `limiter_a.check(key)`
- **判据**: 放行
- **断言**: `assert!(result.is_ok())`

### TC-019-033 实例 B 打同一键 → 429（**核心判据**）

- **步骤**: 紧接 032，`limiter_b.check(同一 key)`
- **判据**: **被拒**
- **断言**: `assert!(result_b.is_err() && code == RATE_LIMITED)`
- **区分力**: 每实例内存实现 → B 见 0 → 放行 → 红
- **⚠️ 已知盲区**: 「进程内全局 static」实现会共享 → 也 429 → 假绿。
  所以判据重心在 034

### TC-019-034 DB 里该键恰好 1 行（**真正的判据**，B3 修正）

- **步骤**: 紧接 033，查 `platform_rate_limit`
- **判据**:
  - `(tenant_id, dimension, key)` 在窗口内**恰好 1 行**
  - 该行 `dimension == "endpoint"`、`occurred_at` 在最近 60 秒内
- **断言**: `assert_eq!(count_rows_in_window, 1)`
- **⚠️ 不存在的列不写**: 表**没有** `window_start`、**没有**计数列（B3）
- **⚠️ 为什么是 1 行不是 2 行**: 被拒的请求**不落行**（design §3「先判定后记账」）

---

## 判据校准清单（实现后回填）

| 用例段 | 目标文件（预设） |
|---|---|
| TC-019-014~030 | `crates/platform-observability/tests/rate_limit_real_db.rs` |
| TC-019-031~034 | 同上（双池场景） |
| 键生成逻辑单测 | `crates/platform-observability/src/rate_limit/mod.rs` |
