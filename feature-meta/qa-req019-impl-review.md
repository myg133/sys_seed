# REQ-019 实现阶段独立复核

> 复核者：独立 verifier（非实现者）
> 复核对象：`feature/REQ-019` @ `561bb01`
> 状态：**已完成**

## ⚠️ 复核快照说明（必读）

复核期间**实现者在并发提交**。我开始时 HEAD 是 `39be311`，复核过程中被推进到
`561bb01`（+3 个自查补丁：`068c3d2` 失败响应体 trace_id、`0b2c912` 三维度独立、
`561bb01` prune 边界）。

**本复核以 `561bb01` 为判据基线**。若实现者继续提交，结论需重新基线化。

---

## 已确认的环境事实（先固定，避免争议）

| 项 | 实测 |
|---|---|
| `cargo test -p platform-observability --all-targets` | 59 + 14 + 5 + 5 + 20 + 2 = **105 passed / 0 failed**（真库连接正常） |
| `cargo xtask openapi --check`（干净树） | 退出码 **0**，输出「OpenAPI 产物与源码一致」 |
| 阳性对照（改产物 `info.title` → `--check`） | 退出码 **1**，输出差异行 ✅ 成立 |
| `Cargo.lock` 包数 | develop **329** → 当前 **360**，净增 **31** |
| 其中 `platform-observability` 本身 | 1 个（本需求新 crate） |
| 其中 `utoipa` 家族 | **2**（`utoipa` + `utoipa-gen`）✅ 与 D338 一致 |
| 其余（OTel + 传递依赖） | **28** |
| `openapi.json` 的 `components.schemas` | **空**（`[]`），schema 全部**内联**展开 |
| `openapi.json` 覆盖端点 | **1 个**（`/api/observability/rate-limit/status`），与 D345 声明一致 |
| `platform_rate_limit` 表列 | `id/tenant_id/dimension/key/occurred_at`（**无** `window_start`、无计数列）✅ 与 B3 修正一致 |

---

## FINDING 清单

| # | 严重级 | 位置 | 问题 | 判据 |
|---|---|---|---|---|
| F1 | **BLOCKER** | 见下 | AC-S-035 步骤 3 不可执行：要改的 DTO 不在 OpenAPI 产物里 | 见下 |
| F2 | **BLOCKER** | 见下 | 限流清理任务（`prune`）在生产中**无任何调用点** | 见下 |
| F3 | **BLOCKER** | 见下 | AC-S-034 的 `limit` 错误码在两个端点**不一致**（规格明文要求 `INVALID_REQUEST`）| 见下 |
| F4 | **MAJOR** | 见下 | AC-S-033 缺规格明文要求的 `assert_ne!(pool_a.id(), pool_b.id())` | 见下 |
| F5 | **BLOCKER** | 见下 | AC-S-038 的 503 路径与「关闭限流仍 200」**只有单测，无端到端** | 见下 |
| F6 | **MAJOR** | 见下 | AC-S-032 的 429 响应体（`params` 闭集 / `detail` 黑名单）只有单测，无 HTTP 层断言 | 见下 |
| F7 | **MAJOR** | 见下 | D339 声称净增 18~24，实测 **28**（含 `tonic` gRPC 栈）| 见下 |
| F8 | **MAJOR** | 见下 | AC-S-029 日志侧的 subscriber 与生产**不是同一个函数** | 见下 |
| F9 | MINOR | 见下 | 注释与代码不一致（`apply_observability` 顺序注释）| 见下 |
| F10 | MINOR | 见下 | `span.set_parent` 返回值未处理，触发 `unused_must_use` 警告 | 见下 |
| F11 | **BLOCKER** | 见下 | AC-S-031 步骤 5「query 不绕桶」判据是**循环论证**，且 endpoint 键**不含 METHOD** | 见下 |
| F12 | **BLOCKER** | 见下 | AC-S-030 步骤 4/5 的「三处逐处断言」只实现了一处 | 见下 |
| F13 | **MAJOR** | 见下 | endpoint 桶混淆 GET / DELETE / POST（D337 要求含 METHOD） | 见下 |
| F14 | **MAJOR** | 见下 | AC-S-030 步骤 2（大写归一）无 HTTP 级断言 | 见下 |
| F15 | **BLOCKER** | 见下 | **已实证**：按生产装配顺序，顶层限流中间件**一次都不执行**（limiter 被调用 0 次）| 见下 |
| F16 | MINOR | 见下 | `status.md` 的完成度表与实际断言覆盖不符 | 见下 |
| F17 | **BLOCKER** | 见下 | AC-S-037 步骤 2 的「时延 < 1 秒」判据完全没触发导出路径 | 见下 |
| F18 | MAJOR | 见下 | AC-S-037 步骤 1「span 至少 2 层」无断言 | 见下 |
| F19 | MINOR | 见下 | AC-S-034 新端点只有 3 行数据，分页翻不动 | 见下 |
| F20 | MINOR | 见下 | 装配顺序文档图漏了 deprecation 层 | 见下 |
| F21 | NIT | 见下 | 构建产生 2 条警告（`dead_code` + `unused_must_use`）；crate 声明了 `#![warn(clippy::pedantic)]` | 见下 |
| F22 | NIT | 见下 | 规格自身数错：实际 **10 条** AC，规格反复写「9 条」 | 见下 |

---

## F1 · BLOCKER · AC-S-035 步骤 3 **不可执行**

**位置**：`BA/demands/REQ-019-observability-api/acceptance.md:265-267`（规格）、
`openapi.json`（产物）、`crates/platform-observability/src/http.rs:120-215`

**规格要求**（步骤 3）：

> 「改一个 DTO 字段名（`AppView.display_name` → `displayNameProbe`）→ 重新编译 →
> 跑 `--check` → **改回来**」，预期「退出码非 0，且输出含**差异行**」。

**实测**：

1. `openapi.json` 的 `components.schemas` 是**空对象** `[]`；
2. 全产物里 **0 处** 出现 `AppView` / `display_name` / `displayName`；
3. `ApiDoc`（`http.rs:206-215`）的 `paths(...)` **只**列了 `rate_limit_status` 一个 handler；
   `AppView` 是 `platform-app` 的 DTO，**从未**进入 `ApiDoc` 的 schema 集合。

**为什么不可接受**：这条步骤的判据是「改一个**已进入产物**的 DTO 字段名 → `--check` 必须非 0」。
`AppView` 未进入产物 → 改它**不会**改变 `render()` 的输出 → `--check` 仍返回 0
→ 按规格预期「非 0」执行时**必然红**；反过来说，任何按规格去做的人都得不到预期结果。

判据在这个实现上**无法执行**，而 `status.md:93` 把它记为「达成」——
声称的是「机制成立」，但 AC 文本里的**步骤 3 主语不存在**。

**修改建议**：把步骤 3 的变更对象改成产物里**真实存在**的 DTO
（例如 `RateLimitRow.window_seconds` → 改名，它在产物里以 `windowSeconds` 内联出现），
或明确写明「本轮 DTO 变更探针用 `RateLimitRow`」。

**注**：这是规格实现后的**新增事实**（D345 只处理了「覆盖端点少」，没处理
「步骤 3 的探针 DTO 不在产物里」）。规格阶段的 31 条挑刺**没有**覆盖这条。

---

## F2 · BLOCKER · 限流清理任务在生产中**从不执行**

**位置**：`crates/platform-observability/src/rate_limit/mod.rs:251-272`（`prune`）、
`migrations/20261123000100_rate_limit.up.sql:8`（注释）、
`crates/xtask/src/cli.rs:15-34`（命令清单）

**事实**：

- 迁移文件自己写着：「代价：**每请求一行** → 必须配清理任务
  （`xtask observability prune-rate-limit`）。⚠️ 没有清理会在几周内变成性能瓶颈
  —— **这不是可选项**」；
- D337 也写了「清理**不是**可选项」「不实现就等于埋雷」；
- 实测 `grep -rn "prune" crates/`：`PgRateLimiter::prune` 的调用点**只有测试**
  （`rate_limit_real_db.rs:413`、测试文档）；**生产代码零调用**；
- `xtask/src/cli.rs` 的 `Command` 枚举里**没有** `observability` / `prune-rate-limit`
  子命令（只有 `CheckModules/CheckDeps/Capabilities/CreateModule/TenantPolicies/Audit/Notify/Storage/Openapi`）；
- `main.rs` 里也**没有**任何后台清理线程/定时任务。

**为什么不可接受**：这是**表会无限增长**的确定性缺陷 —— `platform_rate_limit`
每请求一行、**没有任何删除路径**。迁移注释承诺的命令**不存在**。
按 D337 自己的话，这属于「不实现就等于埋雷」，而现在的实现是
「实现了函数但没有人调用」，比完全没写更具误导性（有函数、有测试、绿灯）。

这是一个**运营层面的阻塞项**，既不属任何 AC 的判据，也没有记进 TD。
`status.md` 对此**只字未提**。

**修改建议**：接一个 `xtask observability prune-rate-limit` 子命令（或复用
`worker` 形态），并记进 `design-summary.md` 的「定期任务」表；若本轮不做，
必须**明码记 TD** 并在迁移注释里改为「尚未接入」。

---

## F3 · BLOCKER · AC-S-034 的 `limit` 越界错误码**两个端点不一致**

**位置**：`crates/platform-audit/src/query.rs:324-328`（`invalid()`）、
`crates/platform-audit/src/query.rs:656-664`、
`crates/platform-observability/src/http.rs:100-114`（`problem()`）

**规格要求**（`acceptance.md:248-250`）：

> 「步骤 4：`limit=0` → **400**；`limit=1000` → **400**；
> 两者 `code == "INVALID_REQUEST"`」

**实测**：

| 端点 | `limit=1000` 的 `code` | 依据 |
|---|---|---|
| `GET /api/audit/events` | **`AUDIT_INVALID_FILTER`** | `query.rs:324-328` 的 `invalid()` 一律返回 `AuditError::InvalidFilter`；`query.rs:656-664` 的 `a_zero_limit_is_rejected` **显式断言** `assert_eq!(error.code(), "AUDIT_INVALID_FILTER")` |
| `GET /api/observability/rate-limit/status` | **`INVALID_REQUEST`** | `http.rs:104-114` 的 `problem()` 硬编码 `"INVALID_REQUEST"` |

**为什么不可接受**：AC-S-034 的**全部意义**是「分页契约**统一**」。
现在两个端点在**同一个错误场景下返回不同错误码** —— 调用方无法用一套逻辑处理。
规格明文要求 `INVALID_REQUEST`，audit 侧**没改**，而且**新测试把旧码锁死了**
（`query.rs:657` / `:664` 都断言 `AUDIT_INVALID_FILTER`）。

`status.md:92` 说「audit 侧：`limit` 超限改 **400**」—— 400 确实改了（状态码对），
但 `code` 没改，而 AC 明文要求的是 `code == "INVALID_REQUEST"`。
这是**判据被稀释**：只满足了「400」，没满足「同一个 code」。

**判据**：AC-S-034 步骤 4 后半句「两者 `code == "INVALID_REQUEST"`」在 audit 侧**不成立**。

**注**：任务单 C 的猜测方向是对的。规格阶段挑刺 B4 只发现了「夹取 vs 400」的冲突，
没发现「错误码也要统一」这一层。

---

## F4 · MAJOR · AC-S-033 缺规格**明文要求**的池身份断言

**位置**：`crates/platform-observability/tests/rate_limit_real_db.rs:314-332`

**规格要求**（`acceptance.md:184-185`，M1 的修正结论）：

> 「⚠️ **必须断言两池确实不同**：`assert_ne!(pool_a.id(), pool_b.id())`」

**实测**：`the_fixture_can_build_two_independent_handles` 只做了
`is_reachable` 两次（`rate_limit_real_db.rs:330-331`），**没有**任何池身份断言。
测试自己的注释（`:321-322`）承认「本用例只断言『能建两个』」。

**为什么不可接受**：规格**点名**了这条断言（它是 M1 修复的产物，用来排除
「复用同一个池」）。`TenantDb::connect` 若被改成「内部复用全局池」
（缓存 pool 的常见优化），`is_reachable` 两次照样都返回 true → 这条**恒绿**。

规格的兜底逻辑是「区分力交给步骤 3（DB 里 1 行）」，这一点实现满足，
但**规格明文要求的断言被替换成了无区分力的代理断言** —— 这正是 M1 当初否掉的模式
（用 `max_connections` 不同做代理），只是换了个形式。

**判据**：AC-S-033 前置条件里的「必须断言两池确实不同」**没有对应断言**。

---

## F5 · BLOCKER · AC-S-038 的关键判据（503 / 关闭仍 200）**没有端到端断言**

**位置**：`crates/platform-observability/tests/rate_limit_real_db.rs:497-577`、
`crates/platform-observability/src/http.rs:232-245`、
`crates/platform-server/src/main.rs:1986-2030`

**规格要求**（`acceptance.md:360-372`）：

- 步骤 1：`GET /health/ready` → **200**，`checks` 里限流项为 `ok`；
- 步骤 3：**断开限流表** → `GET /health/ready` **503**，且响应体指出**限流存储**不可达；
- 步骤 4：`rateLimit.enabled = false` 时重复步骤 2/3 → **200**。

**实测**：

- `grep "Some(false)" crates/platform-observability/` 只命中 `http.rs:223` 的**注释**；
  测试里**没有任何一处**让 `rate_limit_readiness` 返回 `Some(false)`；
- `grep "SERVICE_UNAVAILABLE\|503" crates/platform-observability/tests/` → **零命中**；
- 被测的是 `rate_limit_readiness()` 这个**纯函数**的返回枚举
  （`Some(true)/None`），**没有**任何测试驱动 `/health/ready` 这个 handler，
  更没有「把表改名 → 拿到 503 + 体里有 rate limit」的断言；
- `ready` handler 在 `main.rs`（**bin crate**），测试够不着 —— 这一点
  `status.md:96` 只写了「`main` 的 ready handler 用它」，没说 503 路径**没测过**。

**为什么不可接受**：AC-S-038 的**判据主体是 HTTP 响应码与响应体**。
现在验的是「辅助函数在被喂了特定 state 时返回什么」，**不是**「探针真的会 503」。
`ready` handler 里那段 `if rate_limit_ok == Some(false) { ... 503 ... }`
（`main.rs:2017-2030`）**一行都没有被测过** —— 写错了（比如条件写成 `!= Some(true)`）
测试全绿。

这属于「判据稀释」：AC 要「`/health/ready` 返回 503」，实现只测了「函数返回 false」。

---

## F6 · MAJOR · AC-S-032 的 429 **响应体**判据只有单测，无 HTTP 层断言

**位置**：`crates/platform-observability/tests/middleware_assembly.rs:126-162`、
`crates/platform-observability/src/error.rs:189-203`

**规格要求**（`acceptance.md:168-173`）：步骤 3 要对 **429 的响应体**做结构检查：
`params` 的**键集合 ⊆ `{"dimension","retryAfter"}`**，且 `detail`/`title`
不得含黑名单词。

**实测**：

- `middleware_assembly.rs:152-161` 拿到的是 `body`（String），只断言了
  `status == 429` 与 `headers.get("retry-after").is_some()`；
  **没有**解析 body，**没有**断言 `params` 键集；
- `params` 闭集的断言在 `error.rs:189-203` 与 `rate_limit_real_db.rs:264-279`，
  两者都是**直接构造 `ObservabilityError::RateLimited` 再调 `params()`**；
- `detail` 黑名单断言在 `error.rs:206-226`，同样只测**构造函数输出**，
  不是「HTTP 429 响应体里真的没有泄露」。

**为什么不可接受**：AC-S-032 步骤 3 防的是「**响应体**多返回一个 `current`/`bucket`」。
现在验的是「`ObservabilityError::params()` 恰好只塞两个键」——
若中间件在 `middleware.rs:149` 之后又往 `details.params` 里补字段
（比如加一个 `remaining`），`error.rs` 的单测**照样绿**。

判据链路中间**有一层（中间件→ProblemDetails）没有断言**。

---

## F7 · MAJOR · D339 的依赖数字与实际不符，且引入 gRPC 栈

**位置**：`BA/demands/REQ-019-observability-api/decisions.md:151-218`（D339）、
`crates/platform-observability/src/lib.rs:20-23`、`status.md:56-59`

**声称**：D339「OTel 净增 **18~27**」；`lib.rs:23` 写「净增 **18** 包」；
`status.md:59` 写「**24 个第三方**（329 → 360）」。

**实测**（`Cargo.lock` 对比 `develop`）：329 → 360，净增 **31** 个条目；
扣掉新 crate 自身 1 个 = **30**；其中 `utoipa` 家族 2 个 → **OTel 相关 28 个**。

且 `status.md:59` 自己写的「24 个」与它同一行给的「329 → 360」**算术不符**
（360-329=31）。`lib.rs:23` 的「18」也与任何实测值不符。

**另一件事**：`cargo tree -i tonic` 显示引入了 **`tonic` 0.14.9 + `tonic-prost`**
（gRPC 栈），而 D339 明确选的是「**HTTP/protobuf（不是 gRPC）**」。
`opentelemetry-otlp` 只在 `default-features = false` + `http-proto` 下仍会经
`opentelemetry-proto` 拉进 `tonic`（proto 定义的传递依赖）。

**为什么不可接受**：D338/D339 是**用户裁定的破例**，其边界靠「净增包数」定义。
实现后实测值（28 或 30）**超出** D339 声明的上限（27），且 `status.md` 内部的
「24 / 329→360」自相矛盾。破例的账**记错了**，而这是唯一能约束破例的东西。

**注**：`tonic` 是否属「计划外夹带」需判定 —— 它不是 `platform-observability`
直接声明的依赖，是 `opentelemetry-otlp` 的传递依赖，属**可解释**；
但「净增 28 vs 声称 ≤27」是硬数字差异。

---

## F8 · MAJOR · AC-S-029 的日志侧测试与生产**不是同一个 subscriber**

**位置**：`crates/platform-observability/tests/trace_logging.rs:54`、
`crates/platform-server/src/main.rs:254-286`（`init_tracing`）

**规格要求**（`acceptance.md:63-64`）：日志里至少一行含 `trace_id=T`。
规格的设计（M4 修正）明确要求「生产 `init_tracing` 改成 `build_subscriber(level, writer)`
与测试共用」。

**实测**：

- `trace_logging.rs:54` 用的是 `platform_core::log::build_subscriber(...)`；
- 生产 `init_tracing`（`main.rs:254-286`）**没有**调用 `build_subscriber` ——
  它自己 `registry().with(fmt_layer).with(otel_layer).with(filter).init()`，
  手工重建了一遍；
- 两者共用的是 `log_filter`（过滤器），**不是**整个 subscriber。

**为什么不可接受**：M4 的核心诉求是「**测过的东西与线上跑的是同一份代码**」
（`log.rs` 开头的教训原文）。现在测的是 `build_subscriber`（fmt-only），
生产跑的是 `init_tracing`（fmt + otel layer + 不同构造路径）。
生产路径若把 `trace_id` 字段丢了（比如 `span` 名/字段拼错但测试用的 subscriber
恰好能显示），测试**发现不了**。

「共用了 `log_filter`」只是**部分**满足，但规格与注释都写着「共用 `build_subscriber`」。

---

## F9 · MINOR · 注释与代码不一致（本项目硬规矩）

**位置**：`crates/platform-observability/src/lib.rs:162-181`

**实测**：

- 注释（`lib.rs:166-168`）写：「这个顺序我踩过：反着写时，限流中间件先执行，
  它读到的 `RequestContext` 里**没有租户**（client_info 填的租户还没写进去，
  **还没写进去**）」；
- 但 **`client_info` 从不填租户**（`platform-server/src/client_info.rs` 全文只有
  `with_client_ip` / `with_user_agent`，**没有** `with_tenant`）；
- 括号内容本身也是重复的病句（「还没写进去，还没写进去」）。

**为什么不可接受**：本项目把「注释与代码一致」列为硬规矩。这条注释把一个
**错误的心智模型**（client_info 会填租户）写进了唯一的装配实现里。
后续维护者会据此判断顺序，而实际租户是在**子系统内层**鉴权中间件里填的。

---

## F10 · MINOR · `span.set_parent` 返回值未处理

**位置**：`crates/platform-observability/src/trace/middleware.rs:78`

**实测**：`cargo test --no-run` 产出警告：
`unused Result that must be used` → `span.set_parent(context);`，
建议改成 `let _ = span.set_parent(context);`。

**为什么不可接受**：`set_parent` 返回 `Result`（父上下文设置失败时会 Err）。
现在**静默丢弃**。虽然失败时 OTel 会自己生成 trace_id（判据仍能红），
但这条警告说明「返回值没被有意处理过」，而它是 AC-S-029 步骤 5 的**关键一步**。

---

---

## F11 · BLOCKER · AC-S-031 步骤 5 的判据是**循环论证**（恒真）

**位置**：`crates/platform-observability/tests/rate_limit_real_db.rs:182-203`

**规格要求**（`acceptance.md:127-128,142-144`）：

> 步骤 5：`user-a` 打 `GET /api/rbac/roles?x=1` 然后 `?x=2`（接口上限 1）

> 预期：「第 1 次 **200**、第 2 次 **429**
> （⚠️ `MatchedPath` 已实测可用：`/api/rbac/roles?x=1` → `/api/rbac/roles`，
> 所以两个 query 共用同一个桶。**若实现用完整 URI 做键 → 第 2 次 200 → 红**）」

**实测**：

```rust
let key1 = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/api/rbac/roles");
let key2 = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/api/rbac/roles");
assert_eq!(key1, key2);          // ← 同一字面量调用两次，恒真
```

**为什么不可接受**：这条 AC 要验的是「**中间件从 `MatchedPath` 取值，而不是从完整 URI 取值**」。
测试却**直接调 `bucket_key` 并传入同一个不带 query 的字面量** ——
`key1 == key2` 是与被测代码无关的**恒等式**（同一个函数、同一个参数）。

若实现改成 `request.uri().path_and_query()`（完整 URI），这个测试**照样绿** ——
因为它根本没经过中间件，也没经过 axum 的 `MatchedPath`。

规格明文写了「若实现用完整 URI 做键 → 第 2 次 200 → 红」，而现在**这个变异不会被抓到**。
全仓**没有任何**测试驱动带 query 的请求穿过 `rate_limit` 中间件
（`middleware_assembly.rs` 只打 `/probe` 与 `/api/audit/events`，都不带 query）。

**判据**：AC-S-031 步骤 5 的「query 不绕桶」**没有实现判据**；`assert_eq!(key1,key2)`
是无效断言（等号两边是同一个表达式）。

---

## F12 · BLOCKER · AC-S-030 步骤 4/5 的「**三处**逐处断言」只做了一处

**位置**：`acceptance.md:89-93`（规格）、
`crates/platform-observability/tests/otel_in_memory.rs:164-170`、
`crates/platform-observability/tests/http_contract.rs:288-292`

**规格要求**（原文）：

> 「**⚠️ 安全判据（步骤 4/5）**：非法值**不得**出现在以下**三处**（**逐处断言**）：
> ① 响应头 `X-Request-Id`（不相等）；
> ② `capture_logs` 捕获的日志全文（**不含**该非法串，
>   特别是步骤 5 的 `FAKE LOG LINE` 不得成为独立一行）；
> ③ 失败响应体 `.trace_id`（不相等）。」

**实测**：

| 处 | 有断言吗 | 证据 |
|---|---|---|
| ① 响应头 | ✅ 有 | `otel_in_memory.rs:169`、`http_contract.rs:292` |
| ② 日志全文 | ❌ **没有** | `grep "FAKE\\|injection\\|注入" trace_logging.rs` → **零命中**；`trace_logging.rs` 全文只有 2 个测试，都验「合法值出现」，**没有**验「非法值不出现」 |
| ③ 失败响应体 `.trace_id` | ❌ **没有** | `http_contract.rs` 的 `the_error_body_carries_the_same_trace_id_as_the_header` 用的是**合法**入站值；无任何测试用非法值验证响应体 |

**为什么不可接受**：AC-S-030 的**安全判据**是本 AC 的核心。规格明确指出
「路径穿越」与「日志注入」是**两个不同向量**（`acceptance.md:97-98`），
并点名要求日志侧的断言。现在：

- 实现确实在 `normalize_trace_id` 里挡住了（单测有：`trace.rs:156-174`）；
- 但「**日志里真的没有出现非法串**」这条**端到端断言不存在**。

判据缺口在于：若将来有人在 `middleware.rs` 里把原始 `incoming` 值也打进日志
（比如 `tracing::debug!(raw = %incoming)` 用于排障），**所有测试仍然全绿** ——
而 `FAKE LOG LINE` 就会成为独立日志行，这正是这条 AC 要防的攻击。

`status.md:88` 把 AC-S-030 记为「达成」，但只兑现了 5 条路径中的校验与 1 处回显断言。

---

## F13 · MAJOR · endpoint 桶**不含 HTTP 方法**，GET 与 DELETE 共用配额

**位置**：`crates/platform-observability/src/rate_limit/mod.rs:121-129`

**规格/决策要求**：

- D337（`decisions.md:70`）明文：`endpoint` 键形态 = `rl:endpoint:<METHOD>:<路径模板>`
  例：`rl:endpoint:GET:/api/rbac/roles`；
- AC-S-033 步骤 3（`acceptance.md:198`）：该行的
  「`key` **含 `GET`** 与路径模板」。

**实测**：

```rust
dimension::ENDPOINT => format!("rl:endpoint:{endpoint}"),   // ← 没有 METHOD
```

中间件传进来的 `endpoint` 只来自 `MatchedPath::as_str()`（`middleware.rs:125-129`），
**不含方法**。全仓 `grep "method" crates/platform-observability/src/rate_limit/` → 无命中。

**为什么不可接受**：

1. **实际后果**：`GET /api/rbac/roles`（列表）与 `DELETE /api/rbac/roles/{id}`
   若路径模板不同则无影响，但同一路径的多方法路由（如 `/api/apps` 同时挂 GET 与 POST）
   **共享一个配额桶**。写接口先到先得会挤掉读请求 —— 与「按接口限流」的语义不符；
2. **判据层面**：AC-S-033 步骤 3 明文要求 `key` 含 `GET`。实现产出的 key
   是 `rl:endpoint:/api/rbac/roles`（**无 `GET`**）→ 按规格执行该断言**必红**；
   而现有测试**没有**这条断言（见 F4），所以没被发现。

这是「规格与实现不一致，且判据没兜住」的典型形态。

### 更糟：现有测试把**错误形态锁死**成了「正确」

`crates/platform-observability/src/rate_limit/mod.rs:396-402`：

```rust
let with = bucket_key(dimension::ENDPOINT, "tenant-a", None, "/api/rbac/roles");
assert_eq!(with, "rl:endpoint:/api/rbac/roles");   // ← 硬编码「没有 GET」的形态
```

这条测试**断言了 D337 明确规定的形态之外的东西**。若有人按 D337 把 METHOD
加进键（`rl:endpoint:GET:/api/rbac/roles`），这条测试会**红** ——
即：**正确的实现会被测试拦住**。这与 rev 2 挑刺 B1 的类型完全相同
（判据在奖励错误实现 / 惩罚正确实现），只是发生在实现阶段的测试里。

---

## F14 · MAJOR · AC-S-030 步骤 2（大写归一）**没有 HTTP 层断言**

**位置**：`acceptance.md:86-87`（规格）、`crates/platform-observability/tests/`

**规格要求**：

> 步骤 2：带 `X-Request-Id: ABCDEF0123456789ABCDEF0123456789`（**大写**）
> → 期望响应头是**小写版**（`abcdef01...`），⚠️ **不是**原样大写、也不是新生成的。

**实测**：`grep "ABCDEF0123\|大写" crates/platform-observability/tests/*.rs` → **零命中**。

归一逻辑只在 `trace.rs:133-138` 的**单测**里验（直接调 `normalize_trace_id`）。
HTTP 层没有任何测试用大写头。

**为什么不可接受**：`http_contract.rs:231-270` 的中间件级测试只验了「合法全小写 → 沿用」
与「非法 → 替换」。**大写 → 小写**这条路径在中间件层没验。
若中间件实现改成 `HeaderValue::from_str(raw)`（原样回写，不做归一），
单测仍绿（`normalize_trace_id` 没被改），但 HTTP 行为违反 AC。
规格明确说这条要「不是原样大写」，而它恰恰是最容易在中间件层写错的一处。

---

## F15 · BLOCKER · 限流中间件在**顶层**执行，而租户由**子系统内层**填充 → **生产限流恒不生效（已实证）**

**位置**：`crates/platform-server/src/main.rs:819-825`（顶层装配）、
`crates/platform-server/src/client_info.rs:55-62`、
`crates/platform-observability/src/rate_limit/middleware.rs:106-115`

**装配结构（实测）**：

```text
main.rs:819  apply_all_middlewares_with(app, rate_limit_state, client_info)
               └─ client_info       ← 只填 client_ip / user_agent，**不填 tenant**
                    └─ trace_id
                         └─ rate_limit   ← 这里读 ctx.tenant_id()
                              └─ nest("/api/rbac", platform_rbac::http::router(...))
                                   └─ authenticate（**内层**，这里才 with_tenant）
```

`rate_limit` 中间件的第 ④ 步（`middleware.rs:110-115`）：

```rust
let Some(tenant_id) = ctx.tenant_id() else {
    tracing::debug!("no tenant context ... skipped (TD-072)");
    return next.run(request).await;      // ← 直接放行
};
```

而 `client_info`（唯一的顶层上下文中间件）**从不调用 `with_tenant`**
（`client_info.rs` 只有 `with_client_ip` / `with_user_agent`）。
租户是在各子系统的内层 `authenticate` 中间件里才填的
（`platform-rbac/src/http.rs:215,226`、`main.rs:1690,1699`、`platform-server/src/audit.rs:347`）。

**为什么不可接受**：按装配顺序，`rate_limit` 执行时 `ctx.tenant_id()` **必然为 `None`**
→ 恒定走 TD-072 的「无租户 → 放行」分支 → **三级限流在生产路径上永远不生效**。

测试为什么没抓到：`middleware_assembly.rs:58-62` 的 `fake_client_info` **主动注入租户**
```rust
let ctx = ctx.with_tenant("tenant-a").with_user("user-a");
```
注释自己写着「不注入租户的话，限流判据恒 200（这条我踩过）」——
即测试夹具为了让它绿，**做了一个生产 `client_info` 不做的事**。
这正是「测的不是生产装配」的经典假绿（本仓 `log.rs` 开头就记过同类教训）。

### ✅ 我做了实验证（不是推理，有对照组）

写了一个临时集成测试（复刻**生产装配顺序**），用**计数器 limiter** 记录
`check()` 被调用次数与看到的租户：

**实验组**（`client_info` 等价实现：**只填 IP/UA**，与生产 `client_info.rs` 一致）：

```
statuses = [200, 200, 200, 200, 200]
limiter 被调用 0 次；看到的 tenant = []
```

**对照组**（同一个 `client_info`，但**额外注入租户**——即测试夹具 `fake_client_info` 的做法）：

```
对照组 limiter 被调用 3 次；tenant = [Some("tenant-a"), Some("tenant-a"), Some("tenant-a")]
```

**结论**：唯一差异是「是否注入租户」。生产路径不注入 →
`rate_limit` 中间件里 `ctx.tenant_id()` 为 `None` →
`middleware.rs:110-115` 走 TD-072 的「无租户 → 放行」→
**limiter 的 `check()` 一次都没被调用**，连「查库判定」都没发生。

**影响面**：AC-S-031 / AC-S-032 / AC-S-033 在**生产路径**上**全部空转**。
真库测试全绿是真的（它们直接调 `limiter.check()`），
但它们验的不是生产链路。`middleware_assembly.rs:58-62` 的
`fake_client_info` 主动注入租户（注释自承「不注入租户的话，限流判据恒 200」），
正是这个注入掩盖了缺陷。

> ⚠️ **我的验证覆盖到「顶层中间件链」为止**：我用的是真实的
> `apply_all_middlewares_with` + 生产等价的 `client_info`，
> **没有**起完整 `main.rs`（含 IAM/PG/全部 subsystem）。
> 唯一可能推翻它的情形是：某个**顶层**中间件（`client_info` 之外）会填租户 ——
> 实测 `main.rs` 的顶层 `layer` 只有 `apply_all_middlewares_with` 一处，
> 且 `client_info.rs` 内 `with_tenant` 出现 **0 次**。

---

## F16 · MINOR · `status.md` 的完成度表与实际断言覆盖不符

**位置**：`BA/demands/REQ-019-observability-api/status.md:85-98`

**实测差异**（逐条）：

| `status.md` 声称 | 实际 |
|---|---|
| 「AC-S-030 **达成**」 | 5 条路径里 2 条（大写归一、日志不回显）**无 HTTP 层断言**（F12/F14）|
| 「AC-S-031 **达成**」 | 步骤 5 的判据是恒真式（F11）；endpoint 键不含 METHOD（F13）|
| 「AC-S-033 **达成**」 | 缺规格点名的 `assert_ne!(pool.id())`（F4）；`key` 含 `GET` 未验（F13）|
| 「AC-S-038 **达成**」 | 503 路径与「关闭仍 200」**无端到端断言**（F5）|
| 「AC-S-032 **达成**」 | 429 响应体结构只有单测，无 HTTP 断言（F6）|
| 「AC-S-035 **达成**」 | 步骤 3 的探针 DTO 不在产物里（F1）|
| 「依赖净增 **24**（329 → 360）」 | 360-329 = **31**，自相矛盾（F7）|
| 未提及 | 限流清理任务无调用点（F2）|

**为什么不可接受**：`status.md` 是验收依据。它给的是「达成 9 / 部分 0」，
而实际有 **5 条 BLOCKER/MAJOR 级判据缺口**（F1/F11/F12/F2 + F5）。
这不是措辞问题 —— 逐条 `file:line` 都可复核。

---

## F17 · BLOCKER · AC-S-037 步骤 2 的「时延 < 1 秒」判据**完全没碰到导出路径**

**位置**：`crates/platform-observability/tests/otel_in_memory.rs:206-271`

**规格要求**（`acceptance.md:328-347`）：

> 前置：「`OTEL_EXPORTER_OTLP_ENDPOINT = http://127.0.0.1:1`（**本机不可达**）」
> 步骤 2：「用**不可达** endpoint 构造 provider → 连发 20 次请求 → 记录每次耗时」
> 预期：「服务不崩、请求照常 200，且**单次耗时 < 1 秒**」

**实测（关键）**：

```
thread 'OpenTelemetry.Traces.BatchProcessor' panicked at ...:
there is no reactor running, must be called from the context of a Tokio 1.x runtime
test an_unreachable_collector_does_not_slow_the_requests_down ... ok
test result: ok. 1 passed; finished in 0.01s
```

1. **整条测试只跑了 0.02~0.01 秒**（20 次请求）。若真的在等一次导出超时
   （`timeout_seconds = 1`），不可能 20 次只花 0.01 秒；
2. 后台导出线程**直接 panic 了**（`no reactor running`）——
   测试用 `#[test]` + 自建 `current_thread` runtime，但 `SdkTracerProvider` 的
   BatchProcessor 是在**别的线程**上跑的，那里没有 tokio reactor；
3. 因此「导出失败」这条路径**根本没被执行到**：panicked 的导出线程不会去连
   `127.0.0.1:1`，也就谈不上「慢」或「快」。

**再叠加一个事实**：`127.0.0.1:1` 在本机是**立即 ECONNREFUSED（0.000s）**，
不是超时（实测：`127.0.0.1:1` → `ConnectionRefusedError in 0.000s`；
真正会超时的地址如 `10.255.255.1:4318` → `TimeoutError in 3.002s`）。

**为什么不可接受**：D339 与 M3 要防的坑是「collector 不可达 → 重试阻塞 → 请求变慢」。
而这条判据：

- 用了一个**立即拒绝**的地址（永远不慢）；
- 导出线程**panic 了**（路径没跑到）；
- batch exporter 本就是**异步**的（就算真连也几乎不会阻塞请求线程）。

三重原因叠加，使这条判据**在任何实现下都会绿**。
把生产代码改成 `with_simple_exporter`（同步导出，M3 点名要防的写法）——
这条测试**仍然绿**，因为导出线程 panic、地址秒拒。

**这条恰好是规格阶段 M3 挑刺的产物**：M3 正确指出「只断言 200 零区分力，要加时延上界」。
实现加了时延断言，但**构造的不可达条件是无效的** → 判据仍然是零区分力。

**修改建议**：用**黑洞地址**（如 `10.255.255.1:4318` 或 `192.0.2.1`，实测 3s 超时），
且**必须在 tokio runtime 内**构造 provider（让 BatchProcessor 有 reactor），
并降低 `timeout_seconds` 使超时可见；断言「20 次总耗时 < 1s」而不是单次
（单次 0.0005s 的失败也会被 `< 1s` 放过）。

---

## F18 · MAJOR · AC-S-037 步骤 1「span **至少 2 层**」无断言

**位置**：`crates/platform-observability/tests/otel_in_memory.rs:41-96`

**规格要求**（`acceptance.md:342`）：

> 「步骤 1：span 层级**至少 2 层**（HTTP 请求 span 是根，其下有子 span）」

**实测**：`the_exported_span_carries_the_incoming_trace_id` 只做了
`spans.iter().find(|s| s.name == "http.request")` 与 trace_id 相等断言。
全文**没有** `parent_span_id` / span 计数 / 层级关系的任何断言
（`grep "parent_span_id\|层级\|2 层"` → 零命中）。

**为什么不可接受**：这条判据要证明「HTTP span 之下真的有子 span」。
现在测试里只有 `trace_id` 中间件开的一个 `http.request` span，
**本来就没有子 span**（handler 是 `|| async { "ok" }`）。按规格执行应当红，
但判据根本没写 —— `status.md:95` 只说「步骤 1：span trace_id == 入站」，
把「至少 2 层」这半句**静默丢了**。

---

## F19 · MINOR · AC-S-034 新端点的 `cursor` 语义不可用（总只有 3 行数据）

**位置**：`crates/platform-observability/src/http.rs:144-186`

**规格要求**（`acceptance.md:243`）：步骤 1 要求
「响应含 `items`（**恰好 10 条**）、`nextCursor`（**非空**）、`total`（=25）」
前提是「新端点也有 **≥25 行**可分页的数据」。

**实测**：新端点返回的是**三个维度硬编码的 3 行**
（`http.rs:150-166` 的 `vec![tenant, user, endpoint]`），`total` 恒为 3。

因此：
- `limit=10` → `items` 恰好 **3** 条（不是 10）；
- `nextCursor` **恒为 `null`**（`start + page.len() == all.len()`）；
- 「翻页零重叠」「total=25」等步骤 1/2 的判据在新端点上**不可能成立**。

**为什么不可接受**：AC-S-034 步骤 3 要求新端点「含 `items`/`nextCursor`/`total`」——
字段名确实有（这条满足），但**步骤 1 的数据规模前提**（≥25 行）不成立。
`status.md:92` 说新端点「7 条（越界 400 / 边界 200 / camelCase / 可反序列化）」
——**没有一条**验「分页真的能翻页」，因为**它翻不动**。
这不是造假，但使「跨 crate 分页契约统一」的验证退化为「字段名一样」。

**建议**：如实写明「新端点数据只有 3 行，分页行为未验证」，或补足数据源。

---

## F20 · MINOR · 装配顺序文档图**漏了 deprecation 层**

**位置**：`crates/platform-observability/src/http.rs:351-358`（文档）、
`crates/platform-observability/src/http.rs:430-433`（实现）

**文档写的执行顺序**：

```text
client_info
  └─ trace_id
       └─ rate_limit
            └─ 业务路由
```

**实际挂载**（`http.rs:430-433`）：

```rust
let router = crate::apply_observability(router, rate_limit);          // rate_limit(内) → trace(外)
let router = crate::deprecation::apply_deprecation_router(router, ..); // deprecation(再外)
router.layer(axum::middleware::from_fn(client_info));                  // client_info(最外)
```

→ 真实执行顺序是 `client_info → **deprecation** → trace_id → rate_limit → 业务`。
文档图把 deprecation **完全省略**了。

**为什么不可接受**：本项目把「注释与代码一致」列为硬规矩，而这是**唯一的装配实现**。
`apply_all_middlewares`（`:366-380`）的文档同样漏了它。
后果：弃用头中间件在 `trace_id` **之外**运行 —— 意味着它打在响应上的头
**晚于** trace 回写（无害，但文档没说明），且「哪些层在哪些层之内」这件事
在两个公开函数的文档里都是错的。

（同一函数的注释里另有一处已修的病句：`lib.rs:167` 由
「client_info 填的租户」改成了「client_info 填的租户还没写进去」——
但 `client_info` **根本不填租户**，见 F9。）

---

## F21 · NIT · 构建警告（`dead_code` + `unused_must_use`）

**位置**：`crates/platform-observability/src/rate_limit/mod.rs`（`CountRow` 被报 dead_code）
与 `crates/platform-observability/src/trace/middleware.rs:78`

实测 `cargo test --no-run` 输出 2 条 lib 警告：

1. `CountRow` has a derived impl for `Debug`, but this is intentionally ignored during
   dead code analysis（`dead_code`）；
2. `unused Result that must be used`（见 F10）。

**为什么记**：`platform-observability/src/lib.rs:26` 声明了 `#![warn(clippy::pedantic)]`，
说明该 crate 对整洁度有要求；但这两条警告在每次构建都出现。
若合入前要跑 `cargo clippy -- -D warnings`（本仓门禁口径不明），会阻断。

---

## 逐条 AC 判据兑现度对账（回答「哪些判据没真兑现」）

| AC | `status.md` | 我的判定 | 没兑现的具体判据 |
|---|---|---|---|
| **AC-S-029** | 达成 | ⚠️ **部分** | 日志侧用 `build_subscriber`，生产用 `init_tracing`（**不是同一份**，F8）。三处同值本身达成了（日志/响应体/span 各有断言）|
| **AC-S-030** | 达成 | ❌ **不成立** | 规格明文「三处逐处断言」只做了响应头一处；日志侧、响应体侧**没有**非法值断言（F12）。步骤 2 大写归一在 HTTP 层**无断言**（F14）|
| **AC-S-031** | 达成 | ❌ **不成立** | 步骤 5 判据是恒真式（F11：`assert_eq!(key1,key2)` 同一表达式）。步骤 1/2/3/4 的真库断言扎实；步骤 3 的三维度隔离由 `0b2c912` 补齐 ✅。但**生产路径已实证恒放行**（F15：limiter 被调用 0 次）|
| **AC-S-032** | 达成 | ⚠️ **部分** | `retry_after` 正整数+上界 ✅ 有真库断言；但 `params` 闭集与 `detail` 黑名单只在**构造函数**上断言，HTTP 429 响应体**没验**（F6）|
| **AC-S-033** | 达成 | ⚠️ **部分** | DB 里恰好 1 行 ✅；跨实例 429 ✅；但规格点名的 `assert_ne!(pool.id())` **缺失**（F4）；`key` 含 `GET` 未验且实现**不含 METHOD**（F13）|
| **AC-S-034** | 达成 | ❌ **不成立** | audit 侧 `limit=1000` 返回 **`AUDIT_INVALID_FILTER`**，规格要求 **`INVALID_REQUEST`**（F3）。新端点只有 3 行、翻不动页（F19）。跨 crate「编译期断言」用的是**无 `Serialize` 的 `AuditPage`**，而线上序列化的是 `AuditPageView`（见下）|
| **AC-S-035** | 达成 | ❌ **不成立** | 步骤 3 的探针 DTO（`AppView`）**不在产物里**（F1）→ 按规格执行必红。步骤 1/2/4 与阳性对照 ✅ 成立 |
| **AC-S-036** | 达成 | ✅ **达成** | `Deprecation: true` 字面量 ✅、`Sunset` 严格 IMF-fixdate 反解 ✅、阴性对照 ✅、子路径不继承 ✅、200 保留 ✅。**这条扎实** |
| **AC-S-037** | 达成 | ❌ **不成立** | 步骤 2 的时延判据**完全没触发导出路径**（F17：导出线程 panic + 127.0.0.1:1 秒拒 + batch 异步）。步骤 1 的「span 至少 2 层」**无断言**（F18）。步骤 1 的 trace_id 相等 ✅ 扎实（有变异验证）|
| **AC-S-038** | 达成 | ❌ **不成立** | 503 路径与「关闭仍 200」**无端到端断言**（F5）|

### 补充：AC-S-034「跨 crate 编译期断言」的能力边界

`http_contract.rs:294-332` 的 `the_pagination_envelope_matches_the_audit_field_names`
**明码承认**（`:310-312`）：「`platform_audit::AuditPage` **没有实现 `Serialize`**
（序列化在 `platform-server::audit` 的独立响应类型里做）」。

实测确认：真正上线序列化的是 **`AuditPageView`**（`platform-server/src/audit.rs:173`，
带 `#[serde(rename_all = "camelCase")]`），而测试断言的是
**`platform_audit::AuditPage`**（`platform-audit/src/query.rs:383`，无 serde 派生）。

**后果**：`platform_server::audit::AuditPageView` 的 `next_cursor` 若被改名成
`nextCursor`（Rust 侧字段名改），**这条断言不会红** —— 它根本没引用 `AuditPageView`。
真正决定线上字段名的是 `AuditPageView` 上的 `#[serde(rename_all)]`，
而**没有任何测试断言它**。

这正是任务单 A 怀疑的第 4 点（「AC-S-034 编译期断言真的能抓住改名吗」）：
**抓不住**。它抓的是 `AuditPage`，不是上线序列化的 `AuditPageView`。

### ✅ 我做了实证（不是推理）

把 `crates/platform-server/src/audit.rs` 里 **`AuditPageView`** 的字段
`next_cursor` 改名为 `nextCursorProbe`（连同构造处 `:433`），然后跑：

```
cargo test -p platform-observability --test http_contract -- --test-threads=1
→ test result: ok. 14 passed; 0 failed
→ the_pagination_envelope_matches_the_audit_field_names ... ok   ← 仍然绿！
```

**线上响应字段会从 `nextCursor` 变成 `nextCursorProbe`，而这条「跨 crate 字段一致」
的判据无任何反应。** 改动已还原（`git status` 干净）。

这就是 REQ-018 那类「测试全绿但判据没实现」的复现。

---

---

## F22 · NIT · 规格自身数错：实际 **10 条** AC，规格写「9 条」

**位置**：`acceptance.md:42`、`:392`、`:427`；`status.md:14`

**实测**：`grep -c "^### AC-S-0" acceptance.md` → **10**
（AC-S-029, 030, 031, 032, 033, 034, 035, 036, 037, 038）。

`029 ~ 038` 是**闭区间 10 条**，而规格写：

- `acceptance.md:42`：「本需求自有 AC（**9 条**：AC-S-029 ~ AC-S-038）」
- `acceptance.md:392`：「**7 条功能要求 → 9 条 AC**」
- `acceptance.md:427`：「本需求的 **9 条** AC 全部只覆盖已认证请求」
- `status.md:14`：「**9 条 AC**：AC-S-029 ~ AC-S-037」（这里又把 038 漏了）

**为什么记**：BA 在挑刺 B9 里专门批评过 rev 1「数错了」。
现在 rev 2 的**修正版自身又数错**（补了 AC-S-038 却没把「9」改成「10」），
且 `status.md:14` 与 `acceptance.md:42` 两处**互相矛盾**（一个到 037，一个到 038）。

这不影响实现，但「达成 9 / 部分 0」这个汇总数字的**分母本身是错的**
（真实分母 10）。凡引用「9 条」的下游文档都要一并修正。

---

## 未验证项（明码记录，不要当成通过）

| 项 | 为什么没验 |
|---|---|
| **F15 的完整服务端到端** | 我用**真实的** `apply_all_middlewares_with` + **生产等价** `client_info` 做了实验证（实验组 0 次调用 / 对照组 3 次调用），但**没有**起完整 `main.rs`（含 IAM/PG/全部 subsystem）。唯一可能推翻它的情形是「`client_info` 之外的某个**顶层**中间件会填租户」——实测 `main.rs` 顶层 `layer` 只有 `apply_all_middlewares_with` 一处，`client_info.rs` 的 `with_tenant` 出现 0 次 |
| 全量 `cargo test`（1560+） | 任务单明确禁止（低配机 + 另一个全量在跑）。我只跑了 `-p platform-observability --all-targets`（105 passed） |
| `platform-server` 集成测试 | 未跑（`app_real_stack` 有 5 个 `#[ignore]`，需真库起全套） |
| 真实 OTLP collector 导出 | TD-069 明码不做，本机无 collector |
| `cargo clippy` 全量 | 未跑（会与并发编译竞争 target 目录） |

---

## 结论

### BA 声称的「达成 9 / 部分 0」**不成立**（且分母本身就是错的：实际 **10** 条 AC，见 F22）

我的判定：**达成 1 / 部分 3 / 不成立 6**（合计 10）。

| 结论 | AC |
|---|---|
| ✅ **达成**（1） | **AC-S-036** 弃用头（正向 + 阴性对照 + 严格 IMF-fixdate 反解 + 200 保留，判据完整、有真库/HTTP 级断言）|
| ⚠️ **部分**（3） | AC-S-029（三处同值已有，但日志侧**不是**生产 subscriber）<br>AC-S-032（retry_after 扎实，429 响应体结构无 HTTP 断言）<br>AC-S-033（DB 行数扎实，缺规格点名的 pool 身份断言）|
| ❌ **不成立**（6） | AC-S-030（三处只做一处）<br>AC-S-031（步骤 5 恒真式；且生产空转 F15）<br>AC-S-034（错误码不一致 + 跨 crate 断言打在错的类型上，**已实证**）<br>AC-S-035（步骤 3 探针 DTO 不在产物里）<br>AC-S-037（步骤 2 判据未触达导出路径 + 步骤 1 层级无断言）<br>AC-S-038（503 路径无端到端断言）|

### 最严重的 3 条

1. **F15（BLOCKER，**已实证**）—— 生产路径上三级限流**完全空转**。**
   实验组（生产等价 `client_info`，只填 IP/UA）5 次请求 → **limiter 被调用 0 次**、
   全部 200；对照组（额外注入租户）→ **被调用 3 次**。
   `rate_limit` 读不到租户 → 恒走 TD-072 放行 → **连查库判定都没发生**。
   测试夹具 `fake_client_info` 的注入掩盖了它。
   → **AC-S-031/032/033 在生产上全部不生效**，而真库测试全绿。

2. **F17（BLOCKER）—— AC-S-037 步骤 2 的时延判据完全没触达导出路径。**
   实测：整条测试 0.01s 跑完、导出线程 panic（`no reactor running`）、
   且 `127.0.0.1:1` 是**立即 ECONNREFUSED（0.000s）**而非超时。
   换成 `with_simple_exporter`（M3 点名要防的写法）这条依然绿。

3. **F34 实证（AC-S-034）—— 「跨 crate 字段一致」判据打在错的类型上。**
   把线上序列化的 `AuditPageView.next_cursor` 改名后，
   `the_pagination_envelope_matches_the_audit_field_names` **仍然全绿**。
   线上字段真的会变，判据无反应。

### 必须澄清的一点

我**不认为**这轮实现是敷衍的。相反：

- 真库限流测试（15 条 + 变异验证）、弃用头（AC-S-036）、`prune` 边界、
  三维度隔离（`0b2c912`）**都是扎实的**，`assert_eq!(key1,key2)` 这类问题
  只出现在少数几条；
- 但**「测试全绿」与「判据兑现」之间仍有系统性落差**，
  而且落差集中在**最容易被『看起来做了』骗过的地方**：
  用辅助函数代替端到端、用构造调用代替中间件、用不存在的不可达条件做延迟断言。

### 建议的下一步（按优先级）

1. **先确认 F15**：起一次真实服务（或抽一个含 `client_info` + 鉴权的
   最小 `build_router`），发一个已认证请求，在 `rate_limit` 里打点看
   `tenant_id` 是否为空。**这条不确认，其余限流相关结论都悬空。**
2. **F2**（清理任务无调用者）—— 这是会随流量线性恶化的问题，且迁移注释
   承诺的命令不存在。要么接上，要么明码记 TD。
3. 修 F11 / F1 / F17 这三条**判据失效**（不是补测试，是换判据构造方式）。
4. AC-S-030/F12、AC-S-034/F3 的错误码统一 —— 属规格与实现的直接冲突，需 BA 裁定。

### 我确认写得扎实、不硬找问题的部分

- `trace::normalize_trace_id`（先 ASCII 后长度、12 条单测覆盖 CRLF/多字节/控制字符）；
- `otel::remote_span_context`（识别出 `TraceId::from_hex` 会左填充、
  `SpanId::INVALID` 会让上下文无效，两处都是踩过的坑且写进了注释）；
- `deprecation::sunset_value`（手写 IMF-fixdate + `Rfc2822` 不适用说明 + 反解验证）；
- `rate_limit::PgRateLimiter::check`（先判定后记账、retry_after 用
  `MIN(occurred_at) + window - now()` 一次查询拿两个值）；
- 装配顺序的两个真 bug（`39be311` 抓到）—— 这类「代码看起来对但功能静默失效」
  的自查是有价值的。