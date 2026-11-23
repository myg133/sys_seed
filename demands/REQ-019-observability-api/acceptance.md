# 验收标准：可观测性与 API 治理

- **需求编号**: REQ-019
- **父需求**: REQ-001
- **状态**: 已就绪（**rev 2** —— 独立挑刺 31 条后全面修正）
- **更新时间**: 2026-11-22

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
>
> ⚠️ **判定口径**（§9.15）：每条判据必须能指到一个**具体的 `assert!` + 函数名**，
> 不许写「测试通过」。AC 里任何数字必须有实测命令或 `file:line` 支撑（D275）。

## rev 2 相对 rev 1 的重大修正（挑刺后）

| 挑刺 | 原判据的问题 | rev 2 怎么改 |
|---|---|---|
| **B1** | AC-S-031 步骤 4 要求「`user-b` 打同一端点仍 200」，但 endpoint 键**不含 user 段** → **实现正确时这条必红**（判据在奖励错误实现） | 步骤 4 改为**跨租户隔离**：`tenant-b` 的用户打 `tenant-a` 已打满的**同一路径** → 200 |
| **B2** | AC-S-029 步骤 4 断言 `platform_audit_log.trace_id`，**该列不存在**（契约 C12 是 13 个冻结字段） | **移除**审计表断言。父 AC-028 只要求「日志 / 链路 / **异步消息**」，不含审计表。审计带 trace_id → **TD-071**（要动 C12 + 哈希公式，断历史链） |
| **B3** | AC-S-033 步骤 3 断言 `window_start` 列与「计数」，滑动窗口表**两者都没有** | 改用 `occurred_at` 的**行数**；并明确「被拒请求**不落行**」 |
| **B4** | `limit=1000 → 400` 与已验收的 **D153**（夹到 200 返回 200，有单测锁死）正面冲突，且未声明 | **声明契约 C7 升版**，列出要改的既有单测；上限保持 200（不动），只把「超限」从夹取改为 400 |
| **B5** | AC-S-034 依赖「本需求新增的统一样式端点」，但设计端点清单里**没有** | 设计新增一个真实分页端点 `GET /api/observability/rate-limit/status` |
| **B6** | 写成 `/api/health/live` | 实际是 **`/health/live`** / `/health/ready`（`main.rs:290-291`） |
| **B7** | 要求 `application/problem+json` | 实际是 **`application/json`**，且 **D183** 已明确把这条判据修正掉 |
| **B8** | 要求成功响应体带 `.traceId`，但 `AppView` 无此字段（要改 REQ-011 契约） | 改测**失败响应体**的 `.trace_id`（`ProblemDetails` 已有） |
| **B9** | 宣称「9 条功能要求 → 9 条 AC 无遗漏」，实际 demand 只有 **7 条**，且就绪探针**没有 AC** | 按 7 条逐条对账；就绪探针补 **AC-S-038** |
| **B10** | AC-S-037 要观测 span 与 `is_enabled()`，设计**没有任何可注入/可观测的缝** | 设计加 `init_otel(cfg, exporter_override)` + 明确 `trace_id → OTel TraceId` 的实现路径 |

---

## 承接的父 AC

| 父 AC | 本需求怎么承接 |
|---|---|
| **AC-027** 限流三级生效 | **AC-S-031**（三级各自独立）+ **AC-S-032**（429 + Retry-After 语义）+ **AC-S-033**（跨实例准确） |
| **AC-028** trace_id 全链路贯穿 | **AC-S-029**（日志 / 响应体 / 链路 三处同值）+ **AC-S-030**（入站沿用与安全校验） |

⚠️ 父 AC-028 的原文是「**日志、链路、异步消息**携带同一 `trace_id`」——
**不含审计表**。rev 1 擅自把审计表写进判据，属过度设计（挑刺 B2）。

---

## 本需求自有 AC（**10 条**：AC-S-029 ~ AC-S-038）

> ⚠️ **rev 4 修正**：原写「9 条」是**数错**（029~038 是闭区间 **10 条**）。
> `grep -c "^### AC-S-0" acceptance.md` → 10。见 TD-080。

---

### AC-S-029: `trace_id` 在**日志 / 响应体 / OTel span** 里是**同一个值**

- **前置条件**: 沿用 `crates/platform-server/tests/app_real_stack.rs` 的全栈装配
  （真实 IAM token + ADMIN 权限 + 真实 audit sink + 真实 PG）；
  `tenant-a` 有一个可写的业务对象。
- **测试步骤**:
  1. 发一个**不带** `X-Request-Id` 的请求：`POST /api/apps`（合法 body）；
  2. 从响应头取 `X-Request-Id`（记为 `T`）；
  3. 让该请求**失败**一次（如 `POST /api/apps` 缺必填字段）→ 读响应体 `.trace_id`；
  4. 用 `capture_logs`（见 design §4.2）捕获该请求期间的日志，检索 `trace_id=T`；
  5. 用 `InMemorySpanExporter`（见 design §5.1）取该请求的 span，读其 `trace_id`。
- **预期结果**:
  - 步骤 2：响应头**必有** `X-Request-Id`，且是 **32 位小写 hex**（无连字符）；
  - 步骤 3：失败响应体 `.trace_id` **逐字等于** `T`
    （⚠️ **用失败响应而不是成功响应** —— `ProblemDetails.trace_id` 已存在
    （`problem.rs:104`），而业务成功 DTO（如 `AppView`）**没有** trace 字段，
    加它要改 REQ-011 已验收的响应契约，见挑刺 B8）；
  - 步骤 4：捕获的日志里**至少一行**含 `trace_id=T`
    （⚠️ 断言带等号的**具体形态** `trace_id=<T>`，不是只断言 `contains(T)`）；
  - 步骤 5：span 的 `trace_id` **逐字等于** `T`
    （⚠️ 这是「HTTP 层与 OTel 用同一个 trace」的判据，不是「都有 trace」）；
  - ⚠️ **审计表不在本 AC 的判据里**：`platform_audit_log` 是契约 C12 的
    13 个冻结字段，**没有 `trace_id` 列**（实测确认）。加列要动在线表 + 归档表 +
    `platform_audit_append` 签名 + **哈希公式**（会让历史链校验断裂，破坏
    REQ-014 已验收的 AC）。→ **TD-071**。
- **类型**: 功能 / 可维护性

---

### AC-S-030: 入站 `X-Request-Id` 被**沿用**，非法值被**拒绝**（不能原样透传）

- **前置条件**: 同 AC-S-029。
- **测试步骤**（**5 条独立路径**）:
  1. 带 `X-Request-Id: 0123456789abcdef0123456789abcdef`（合法 32 hex）→ 期望**沿用**；
  2. 带 `X-Request-Id: ABCDEF0123456789ABCDEF0123456789`（**大写**）→ 期望沿用并**归一为小写**；
  3. 带 `X-Request-Id: abc`（太短）→ 期望**不用它**，生成新的；
  4. 带 `X-Request-Id: ../../etc/passwd`（**路径穿越字符**）→ 期望**不用它**；
  5. 带 `X-Request-Id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\nFAKE LOG LINE"`
     （**含 CRLF**，日志注入向量）→ 期望**不用它**。
- **预期结果**:
  - 步骤 1：响应头 `X-Request-Id` **逐字等于** 请求给的值；
  - 步骤 2：响应头是**小写版**（`abcdef01...`），⚠️ 不是原样大写、也不是新生成的；
  - 步骤 3/4/5：响应头是**新生成的合法值**（32 小写 hex），且**不等于**请求里那个非法值；
  - **⚠️ 安全判据（步骤 4/5）**：非法值**不得**出现在以下**三处**（逐处断言）：
    - ① 响应头 `X-Request-Id`（不相等）；
    - ② `capture_logs` 捕获的日志全文（**不含**该非法串，
      特别是步骤 5 的 `FAKE LOG LINE` 不得成为独立一行）；
    - ③ 失败响应体 `.trace_id`（不相等）。
    ⚠️ **不再声称验审计表** —— 审计表没有 trace_id 列（TD-071），
    对它断言该串「不出现」是**真空成立**（挑刺 M11）。
  - **判据独立性**：步骤 2 若实现直接透传（不归一）→ 红；
    步骤 3/4/5 若实现原样透传 → 红；步骤 5 若实现只过滤 `/` 而放过 `\r\n` → 红
    （⚠️ 这正是「路径穿越」与「日志注入」是**两个不同向量**的原因，挑刺 M11）。
- **类型**: 安全

---

### AC-S-031: 三级限流**各自独立**（租户 / 用户 / 接口）

- **前置条件**（**每个步骤都是独立场景** —— 挑刺 M14）:
  - 每个步骤开始前 **truncate `platform_rate_limit`**（否则上一步的桶会污染下一步）；
  - 配置（每步按需调整，**默认**如下）：
    `rateLimit.enabled = true` /
    `rateLimit.tenant.requests = 100`（**先调高**，除非该步骤专测租户维度）/
    `rateLimit.user.requests = 2` / `rateLimit.endpoint.requests = 1`；
    三个维度的 `windowSeconds` **全部 60**；
  - **四个具体端点**（后续步骤从这里选，不写省略号）：
    `GET /api/rbac/roles`、`GET /api/apps`、`GET /api/audit/events`、
    `GET /api/observability/rate-limit/status`；
  - 用户：`user-a` / `user-b`（**同一** `tenant-a`）、`user-c`（**另一个** `tenant-b`）。
- **测试步骤**（**5 条独立路径**）:
  1. **接口维度**：truncate 后，`user-a` 连打 `GET /api/rbac/roles` **2 次**；
  2. **用户维度**：truncate 后，把 `endpoint.requests` 调成 **0**（= 禁用该维度），
     `user-a` 打 **3 个不同端点**各 1 次（`/api/rbac/roles`、`/api/apps`、`/api/audit/events`）；
     ⚠️ 接口维度已禁用 → 每桶都不限 → 只有用户维度会拦截；
  3. **租户维度**：truncate 后，把 `tenant.requests` 设为 **3**、`user.requests = 0`、
     `endpoint.requests = 0`（只留租户维度），
     `user-a` 与 `user-b` 交替打**互不相同**的端点共 **5 次**；
  4. **跨租户隔离**：truncate 后（配置同步骤 1），
     `user-a` 把 `tenant-a` 的 `/api/rbac/roles` 桶打满（2 次，第 2 次 429）；
     然后 `tenant-b` 的 `user-c` 打 **同一路径** `/api/rbac/roles` **1 次**；
  5. **query 不绕桶**：truncate 后，`user-a` 打
     `GET /api/rbac/roles?x=1` 然后 `GET /api/rbac/roles?x=2`（接口上限 1）。
- **预期结果**:
  - 步骤 1：第 1 次 **200**、第 2 次 **429**，`params.dimension == "endpoint"`；
  - 步骤 2：前 2 次 **200**、第 3 次 **429**，`params.dimension == "user"`
    （⚠️ **不是** `endpoint` —— 接口维度已禁用，这条专门隔离出用户维度）；
  - 步骤 3：**恰好第 4 次**开始 **429**，`params.dimension == "tenant"`；
    ⚠️ 只留租户维度时，`user.requests = 0` 与 `endpoint.requests = 0`
    必须**确实不限流**（否则这条会被别的维度抢先拦截）；
  - 步骤 4：**`user-c` 仍然 200**
    （⚠️ **rev 2 的核心修正**：endpoint 键是 `rl:endpoint:<METHOD>:<路径模板>`，
    **不含 user 段** —— 所以同租户内 `user-b` 打同一端点**必然** 429，
    rev 1 那条判据在**实现正确时反而红**（挑刺 B1）。
    改为**跨租户**验证：`tenant_id` 是表的列，`tenant-b` 的桶与 `tenant-a` 完全独立
    → 真正的判据是「别的租户的限流不影响我」）；
  - 步骤 5：第 1 次 **200**、第 2 次 **429**
    （⚠️ `MatchedPath` 已实测可用：`/api/rbac/roles?x=1` → `/api/rbac/roles`，
    所以两个 query 共用同一个桶。若实现用完整 URI 做键 → 第 2 次 200 → 红）；
  - 所有 429 的 `params.dimension` 逐字 ∈ `{"tenant","user","endpoint"}`，
    且响应体 `code == "RATE_LIMITED"`。
- **类型**: 功能 / 安全

---

### AC-S-032: 429 的**响应语义**：`Retry-After` + 不泄露内部结构

- **前置条件**（**独立于 AC-S-031**，挑刺 M13）:
  - truncate `platform_rate_limit`；
  - `rateLimit.enabled = true` / `endpoint.requests = 1` / `endpoint.windowSeconds = 2`；
  - `tenant.requests = 0` / `user.requests = 0`（**禁用另两个维度**，隔离变量）；
  - 端点：`GET /api/rbac/roles`。
- **测试步骤**:
  1. 第 1 次打 → 200；第 2 次打 → **429**，读响应头与响应体；
  2. 等 **3 秒**（窗口 2 秒已过）后再打一次；
  3. 对 429 的响应体做**结构检查**（见下）。
- **预期结果**:
  - 步骤 1：**429**；响应头 `Retry-After` **必有**，是**正整数秒**（十进制，无单位后缀）；
  - 步骤 1：`params.dimension == "endpoint"`；`code == "RATE_LIMITED"`；
  - **⚠️ `Retry-After` 上界**（挑刺 m5）：`1 ≤ retryAfter ≤ windowSeconds + 1`
    （窗口 2 秒 → 只能是 1/2/3）。⚠️ 不设上界的话，返回 `999999` 也能绿；
  - 步骤 2：**200**（⚠️ 窗口过期后**立即恢复**，不是「永久封禁」）；
  - 步骤 3（**键白名单**，挑刺 M2）：
    - 解析响应体，断言 `params` 的**键集合 ⊆ `{"dimension", "retryAfter"}`**
      （⚠️ 不是「包含某几个键」，是**不许有额外键** —— 这是唯一能抓住
      「多返回一个 `current` / `remaining` / `bucket`」的写法）；
    - 断言 `detail` 与 `title` 不得含以下**有限黑名单**（小写化后检查）：
      `platform_`、`select `、`postgres`、`sqlx`、`occurred_at`、`tenant_id`；
  - ⚠️ **不写「PG 相关字样」这种无界集合**（挑刺 M2 指出它写不成断言）。
- **类型**: 安全

---

### AC-S-033: 限流**跨实例准确**（PG 共享计数，不是每进程一份）

- **前置条件**:
  - 两个 `RateLimiter`，各自持有**独立的 `sqlx::Pool`**
    （`pool_a` 与 `pool_b` 用 `PgPoolOptions::new().connect(url)` **分别建立**）；
  - ⚠️ **必须断言两池确实不同**（挑刺 M1 指出 rev 1 的「`max_connections` 不同」是
    **无区分力的代理断言**）；
    ⚠️ **rev 4 修正（复核 F4）**：rev 3 原文写的是
    `assert_ne!(pool_a.id(), pool_b.id())` —— **这行代码写不出来**：
    1. `sqlx::PgPool` **没有 `.id()` 方法**；
    2. `TenantDb.pool` 是 `pub(crate)`（`platform-db/src/tenant.rs:88`），
       测试**拿不到** `PgPool`。
    → 改为**可执行且仍有区分力**的等价断言：
    两个池**并发各持有一条连接**，断言它们拿到的
    `pg_backend_pid()` **不同**（各自连到一条独立的后端进程）。
    若 `TenantDb::connect` 内部改成复用同一个全局池，两次必然拿到**同一个**后端 pid → 红。
    ⚠️ 实测确认该写法可行（两个独立池 → 两个不同 pid）。
  - `endpoint.requests = 1`、`windowSeconds = 60`，另两个维度禁用。
- **测试步骤**:
  1. truncate 后，实例 A 打 1 次 → 期望放行；
  2. 实例 B 打 1 次（**同一个键**）→ 期望 **429**；
  3. 查 `platform_rate_limit` 的行数与内容。
- **预期结果**:
  - 步骤 2：**429**
    （⚠️ 若实现是**每实例内存**，A 计 1、B 见 0 → 200 → 红，判据成立）；
  - 步骤 3（**核心判据**，挑刺 M1/B3）：`platform_rate_limit` 里
    该 `(tenant_id, dimension, key)` 在窗口内**恰好 1 行**
    （⚠️ 只有 1 行因为被拒的请求**不落行** —— 这是 design §3「先判定、后记账」的语义，
    必须在本 AC 的前置里写明）；
  - 步骤 3：该行的 `dimension == "endpoint"`、`key` 含 `GET` 与路径模板、
    `occurred_at` 在最近 60 秒内；
  - ⚠️ **不存在的列不写**：滑动窗口表**没有** `window_start`、**没有**计数列
    （rev 1 断言它们，挑刺 B3）。
  - ⚠️ **已知盲区（明码记录）**：若实现用「**进程内全局 static 计数器**」
    （不是每实例字段），两个 `RateLimiter` 会共享同一个 static → 步骤 2 仍 429
    → **假绿**。本 AC 无法区分「全局 static」与「PG 共享」，
    但步骤 3（DB 里真有行）能区分 → 所以判据重心在步骤 3。
- **类型**: 可靠性

---

### AC-S-034: 分页契约**统一**（`limit` / `cursor` / `nextCursor` / `total`）

> ⚠️ **本 AC 声明对契约 C7 / 决策 D153 的破坏性修正**（挑刺 B4 / M12）
>
> | 项 | 现状（D153 + C7） | 本需求改为 |
> |---|---|---|
> | `limit` 上限 | 200（`query.rs:53` `MAX_LIMIT`） | **200（不变）** |
> | `limit` 超上限 | **夹到 200 返回 200**（`query.rs:178`，单测 `:646-652` 锁死） | **400 + `INVALID_REQUEST`** |
> | `limit <= 0` | `AUDIT_INVALID_FILTER` | **400 + `INVALID_REQUEST`** |
> | `?page=&page_size=` | 支持（与 `limit` 互斥） | **保留**（不破坏已验收行为） |
>
> **要改的既有测试**（必须在本需求的提交里点名）：
> `crates/platform-audit/src/query.rs:646-652`
> `an_oversized_limit_is_clamped_instead_of_rejected` → 改为
> `an_oversized_limit_is_rejected`。
>
> 理由：静默截断会让调用方以为拿到了 1000 条 → **数据静默丢失**。
> 这是「API 治理」的核心内容，值得一次破坏性修正。
>
> ⚠️ 上限保持 200（不改 100）是为了**减少**破坏面 —— C7 的 200 继续有效。

- **前置条件**:
  - `platform_audit_log` 里有 **25 行**（`object_type = 'e2e_fixture'`）；
  - 新端点 `GET /api/observability/rate-limit/status`（**本需求新增**，见 design §10）
    也有 ≥25 行可分页的数据（限流配置项列表）；
  - ⚠️ rev 1 的「新增统一样式端点」在设计里**不存在**（挑刺 B5），rev 2 已补上。
- **测试步骤**:
  1. `GET /api/audit/events?limit=10` → 读响应；
  2. `GET /api/audit/events?limit=10&cursor=<步骤 1 的 nextCursor>` → 读响应；
  3. `GET /api/observability/rate-limit/status?limit=10` → 读响应（**新端点**）；
  4. `GET /api/audit/events?limit=0` 与 `?limit=1000`（越界）；
  5. `GET /api/audit/events?cursor=not-a-valid-cursor`。
- **预期结果**:
  - 步骤 1：响应含 `items`（**恰好 10 条**）、`nextCursor`（**非空**）、`total`（= 25）；
  - 步骤 2：返回**第 11~20 条**，与第 1 页**零重叠**（逐条比对 id）；
  - 步骤 3：新端点响应含 `items` / `nextCursor` / `total`
    （⚠️ 断言写「**包含**这些字段」—— D194，允许额外字段；
    但字段名必须**完全相同**，这是「契约统一」的实质）；
  - 步骤 4：`limit=0` → **400**；`limit=1000` → **400**；
    两者 `code == "INVALID_REQUEST"`；
    ⚠️ **不是静默截断**（见上方契约升版声明）；
  - 步骤 5：**400**，`code == "INVALID_REQUEST"`，`detail` 说明 cursor 非法
    （⚠️ **不是 500** —— 非法输入不是服务器错误）；
  - ⚠️ `?page=&page_size=` **仍然可用**（回归判据）：
    `GET /api/audit/events?page=1&page_size=10` → 200 且含 10 条。
- **类型**: 功能 / 可维护性

---

### AC-S-035: OpenAPI 产物与代码**一致**（`--check` 守卫）

- **前置条件**: `cargo xtask openapi` 已生成 `openapi.json`。
- **测试步骤**:
  1. 干净树上跑 `cargo xtask openapi --check`；
  2. 手工改 `openapi.json` 的 `info.title` → 再跑 `--check`；
  3. **改一个 DTO 字段名** → 重新编译 → 跑 `--check` → **改回来**；
     ⚠️ **rev 4 修正（复核 F1）**：rev 3 原文写的是「`AppView.display_name`
     → `displayNameProbe`」，但**那个 DTO 不在 OpenAPI 产物里**
     （`openapi.json` 的 `components.schemas` 是**空对象**，schema 全部内联；
     全产物 **0 处**出现 `AppView` / `display_name`）。
     `AppView` 是 `platform-app` 的 DTO，**从未**进入 `ApiDoc` 的 `paths(...)`。
     → 按 rev 3 字面执行，改 `AppView` **不会**改变 `render()` 的输出
     → `--check` 仍返回 0 → **判据必红**（任何按规格执行的人都得不到预期结果）。
     **改用产物里真实存在的 DTO**：
     `RateLimitRow` 的 `window_seconds`（在产物里以 `windowSeconds` **内联出现 2 处**）。
     改法：给它加 `#[serde(rename = "windowSecondsProbe")]`（**能编译**，
     符合 rev 2 的「能编译但产物该变」要求），跑完 `--check` 后去掉该属性；
     ⚠️ 实测确认（`feature/REQ-019` @ `561bb01` + 该属性）：
     退出码 **1**，输出「第 62 行 / 第 74 行」的差异行，含
     `windowSeconds` → `windowSecondsProbe`。
  4. 检查产物内容。
- **预期结果**:
  - 步骤 1：**退出码 0**，输出含「与源码一致」；
  - 步骤 2：**退出码非 0**，输出指出**哪个字段**不一致；
  - 步骤 3：**退出码非 0**，且输出含**差异行**
    （⚠️ rev 2 修正：rev 1 用「注释掉 `#[utoipa::path]`」，但那会**编译失败**
    而以非 0 退出 → 判据红/绿与 `--check` 的比对逻辑无关，等于没验（挑刺 M9）。
    改成「能编译但产物该变」的变更）；
  - 步骤 4（产物内容）：
    - `openapi` 字段是 `"3.1.0"`（⚠️ 不是 `3.0.x`）；
    - `info.title` 非空、`info.version` 非空；
    - `paths` 里**含**至少一个**已标注**的端点，且该端点的响应 `content` 里含
      **`application/json`**
      （⚠️ rev 2 修正：平台错误响应的实际媒体类型是 `application/json`，
      见 `problem.rs` 用 `Json(details)`；且 **D183** 已把
      「要求 `application/problem+json`」这条判据当成 BA 写错修正掉了。挑刺 B7）；
    - **⚠️ rev 3 重大修正（D345）**：删掉原来的
      「`paths` 里含 `/health/live` 与 `/health/ready`」与「含 `/api/rbac/roles`」——
      **这两条在当前架构下不可达成**：
      1. `/health/live` / `/health/ready` 的路由在 `main.rs`（**bin crate**）内联定义，
         `#[utoipa::path]` 无处可挂（bin crate 不可被测试/xtask 引用）；
      2. `/api/rbac/roles` 在 `platform-rbac` 里，要标注就得改**已验收 crate**的源码
         （跨需求改动 D109），且 `xtask` 依赖它虽可行但会把「API 文档覆盖」
         绑到「逐个 crate 加依赖」上。
      → 本需求的 OpenAPI 判据改为「**机制成立 + 可验证范围明确**」，
      覆盖面作为**渐进事项**记 **TD-073**。
- **类型**: 可维护性

---

### AC-S-036: 弃用策略：`Deprecation` 与 `Sunset` 头

- **前置条件**:
  - 在**一个真实端点**上标注弃用（⚠️ rev 2 修正：rev 1 用新造的
    `/api/_deprecated-probe` 且设计里留了「考虑是否在生产排除」的未决问题 ——
    冻结的规格里留「考虑」等于执行时无人能判对错，挑刺 M10）。
    **本需求选定**：把 `GET /api/audit/events` 标注为弃用（`Sunset` 设为 2027-06-30），
    因为「分页契约升版」正好给了它一个合理的弃用理由（将被 `v2` 取代）；
  - 同一 router 里的未弃用端点：`GET /api/rbac/roles`
    （⚠️ rev 2 补：必须**同一个 router**，否则「按 router 整体加头」的实现在
    另一个 router 的对照上仍会绿，挑刺 m4）。
- **测试步骤**:
  1. `GET /api/audit/events` → 读响应头；
  2. `GET /api/rbac/roles` → 读响应头（**阴性对照，同一 router**）；
  3. 检查 `Sunset` 的格式。
- **预期结果**:
  - 步骤 1：响应头**必有** `Deprecation`，值**逐字是** `true`
    （D342 统一选 `true`，不用日期形态）；
  - 步骤 1：响应头**必有** `Sunset`，值是 **IMF-fixdate**，
    形如 `Wed, 30 Jun 2027 00:00:00 GMT`
    （⚠️ **严格 RFC 8594**：不接受 ISO8601、不接受 Unix 时间戳）；
  - 步骤 1：响应**状态码仍是 200**（⚠️ 弃用 ≠ 下线 —— 立刻 410 是错的）；
  - 步骤 2：**两个头都不出现**（⚠️ 阴性对照：不能给所有响应都加上）；
  - **判据独立性**：步骤 3 若实现用 ISO8601 → 红；步骤 2 若实现无条件下发头 → 红。
- **类型**: 可维护性

---

### AC-S-037: OpenTelemetry 导出**配置生效**（无 collector 时的可验证部分）

- **前置条件**:
  - `OTEL_EXPORTER_OTLP_ENDPOINT = http://127.0.0.1:1`（**本机不可达**）；
  - `OTEL_SDK_DISABLED = false`；
  - 测试用 `init_otel(cfg, Some(in_memory_exporter))` 注入内存导出器
    （⚠️ rev 2 补的可注入缝，见 design §5.1 —— rev 1 设计里**没有**这个缝，
    导致本 AC 不可执行，挑刺 B10）。
- **测试步骤**:
  1. 构造 provider → 发 1 次请求 → 从内存导出器取 span；
  2. 用**不可达** endpoint 构造 provider → 连发 20 次请求 →
     记录每次耗时；
  3. `OTEL_SDK_DISABLED = true` 时构造 provider。
- **预期结果**:
  - 步骤 1：span **有** `trace_id` / `span_id`，且 `trace_id` **逐字等于**
    响应头 `X-Request-Id`
    （⚠️ 这要求入站头 → OTel `TraceId` 的显式转换，见 design §5.2）；
  - 步骤 1：span 层级**至少 2 层**（HTTP 请求 span 是根，其下有子 span）；
  - 步骤 2：**服务不崩、请求照常 200**，且**单次耗时 < 1 秒**
    （⚠️ rev 2 补：rev 1 只断言「200」，但 D339 要防的坑是
    「collector 不可达 → 重试阻塞 → **请求变慢**」，慢 3 秒仍返回 200 → 零区分力。
    挑刺 M3）；
  - 步骤 2：打 **warn/error** 日志（导出失败的痕迹），但**不影响**响应；
  - 步骤 3：`is_enabled()` 对**返回的 provider 句柄**为 `false`
    （⚠️ rev 2 明确：`is_enabled()` 是 `OtelGuard::is_enabled()`，
    由 `init_otel` 返回的句柄暴露 —— rev 1 没说清是哪个对象的方法，挑刺 B10）。
- **类型**: 功能 / 可维护性
- **⚠️ 明码不做**: **真导出到 collector** 无法验证（本机无 collector）→ **TD-069**。

---

### AC-S-038: 就绪探针覆盖**限流存储**（rev 2 新增，挑刺 B9）

- **前置条件**: 服务已启动；`rateLimit.enabled = true`（⚠️ 只有启用限流时
  就绪探针才检查限流表 —— 否则关闭限流的部署不该因为限流表问题而不就绪）。
- **测试步骤**:
  1. 正常情况下 `GET /health/ready`；
  2. **断开**限流表的访问（模拟：把 `platform_rate_limit` 改名 / 或让连接池指向
     一个不存在的 schema）；
  3. 再 `GET /health/ready`；
  4. `rateLimit.enabled = false` 时重复步骤 2/3。
- **预期结果**:
  - 步骤 1：**200**，响应体含 `checks` 里限流那一项为 `ok`；
  - 步骤 3：**503**，且响应体明确指出**限流存储不可达**
    （⚠️ 不是笼统的 `database unavailable` —— 运维要能分出是哪个依赖坏了）；
  - 步骤 4：**200**（⚠️ **关键判据**：关闭限流时，限流表的问题**不该**
    让服务不就绪 —— 否则「没启用限流」的部署反而更脆弱）；
  - **判据独立性**：步骤 4 若实现无条件检查限流表 → 红。
- **类型**: 可靠性
- **⚠️ 为什么需要这条**（挑刺 B9）：`demand.md` 的功能要求第 3 条
  「就绪探针覆盖新依赖（限流存储）」在 rev 1 里**没有任何 AC 承接**，
  却在 design §10 的端点清单里计划改 readiness —— 属「声称做了但没判据」。

---

## AC 覆盖核对（按 `demand.md` 的 **7 条**功能要求逐条对账）

| # | `demand.md` 的功能要求 | 承接 AC |
|---|---|---|
| 1 | `trace_id` 全链路贯穿（日志 / 响应体 / 异步消息） | **AC-S-029** + **AC-S-030** |
| 2 | OpenTelemetry（OTLP 导出、span 三层） | **AC-S-037** |
| 3 | 就绪探针覆盖新依赖（限流存储） | **AC-S-038**（rev 2 新增） |
| 4 | 三级限流各自独立 + 429 + Retry-After | **AC-S-031** + **AC-S-032** + **AC-S-033** |
| 5 | 分页/排序/过滤规范统一 | **AC-S-034** |
| 6 | OpenAPI 自动生成 + `--check` 守卫 | **AC-S-035** |
| 7 | 弃用策略（`Deprecation` / `Sunset`） | **AC-S-036** |

**7 条功能要求 → 10 条 AC**（第 1、4 条各拆成多条），**无遗漏、无虚空引用**。

⚠️ rev 1 曾宣称「9 条功能要求 → 9 条 AC 无遗漏」，那是**数错了**
（把 AC 条数当成功能要求条数），且**就绪探针真的没有 AC**（挑刺 B9）。

---

## 明确不做（写进规格，防「后面以为漏了」）

| 不做 | 为什么 | 记在哪 |
|---|---|---|
| ❌ **真导出到 OTel collector** | 本机无 collector | **TD-069** |
| ❌ **审计表加 `trace_id`** | 要动契约 C12（13 个冻结字段）+ 归档表 + 追加函数签名 + **哈希公式**（断历史链，破坏 REQ-014 已验收的链校验）| **TD-071** |
| ❌ **业务成功响应体加 `traceId`** | 要改 REQ-011 已验收的 DTO 契约（`AppView` 等） | 见 AC-S-029（改用失败响应） |
| ❌ **全平台切到 `application/problem+json`** | 22 个 crate 的破坏性变更；且 **D183** 已裁定用 `application/json` | 见 AC-S-035 |
| ❌ 日志聚合 / 链路后端部署 | 部署侧的事 | `demand.md` 不在范围 |
| ❌ 业务自定义指标 | 同上 | 同上 |
| ❌ 限流的**分布式配额**（跨区域） | 单库计数已够 | — |
| ❌ 把**所有** crate 的列表端点改成统一分页 | 跨需求改动（D109） | **TD-070** |
| ❌ 未认证请求的限流 | ⚠️ **见下方「已知缺口」** | **TD-072** |

## 已知缺口（明码记录，不掩盖）

### 未认证请求（登录 / 注册）**不受限流保护**

`platform_rate_limit` 是 RLS 租户表，而 `TenantDb::begin` 在**无租户**时直接
`DbError::MissingTenant`（`crates/platform-db/src/tenant.rs:180`）。
→ 登录 / 注册 / `/health` 等**认证之前**的请求拿不到租户上下文，
**写不进限流表** → 登录暴力破解**恰好落在限流覆盖不到的地方**。

本需求**不解决**这个问题（要解决需要「按 IP 的独立维度」或
「sentinel 租户 + 非 RLS 路径」，是独立设计）→ **TD-072**。

⚠️ 现有 IAM 登录已有**自己的** IP 限流（`platform-iam/src/service.rs:837`
`check_ip_rate_limit`），所以**当前不是裸露的** —— 但那是 IAM 专用，
不是本需求的通用三级限流。本需求的 10 条 AC 全部只覆盖**已认证**请求。
