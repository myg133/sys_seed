# REQ-019 复核 FINDING 逐条亲验

> 验证者：BA（**同时是 REQ-019 实现者** —— 独立性不足，如实标注，见 dev-guide 的「BA 不自己实现代码」规矩）
> 复核报告：`qa-req019-impl-review.md`（终版 **22 条**：BLOCKER 8 / MAJOR 7 / MINOR 5 / NIT 2）
> 验证基线：`feature/REQ-019` @ `561bb01`（工作区干净）
> 结论：**21 条成立 / 1 条不成立（F7 口径错误）**，另有 **1 条建议不可执行（F4）**
>
> ⚠️ 复核在收尾时把 F3/F5/F15 从 MAJOR 升为 BLOCKER，并新增 F20/F21/F22，
> 还补了 F15 与 AC-S-034 的两项**实证**（含对照组）。我逐条复核了这些升级与新证据，
> **全部认同** —— 详见下文「对升级与新证据的核验」。

---

## 总表

| # | 复核级 | 我的判定 | 处置 |
|---|---|---|---|
| F1 | BLOCKER | ✅ 成立 | 改 AC-S-035 步骤 3 的探针 DTO（规格修订 + 补测试） |
| F2 | BLOCKER | ✅ 成立 | 加 `xtask observability prune-rate-limit` 子命令 |
| F3 | MAJOR | ✅ 成立 | audit 侧改 `INVALID_REQUEST`，改锁死旧码的测试 |
| F4 | MAJOR | ⚠️ **成立但建议不可执行** | `sqlx::PgPool` **没有 `id()`** → 改为可执行且有区分力的等价断言 |
| F5 | MAJOR | ✅ 成立 | 把 `ready` handler 的 503 分支移到可测位置 + 端到端断言 |
| F6 | MAJOR | ✅ 成立 | 429 响应体在中间件层解析并断言 `params` 键集 |
| F7 | MAJOR | ❌ **不成立** | 复核者用「条目数」口径，D339 用「唯一名」口径，两者都对但不同口径 |
| F8 | MAJOR | ✅ 成立（但可辩） | 生产 `init_tracing` 手工重建 subscriber，未复用 `build_subscriber` |
| F9 | MINOR | ✅ 成立 | 注释有两处错误 + 一句重复病句 |
| F10 | MINOR | ✅ 成立 | `set_parent` 返回值未处理（编译警告实测有） |
| F11 | BLOCKER | ✅ 成立 | 步骤 5 判据是恒真式，且无任何带 query 的中间件级测试 |
| F12 | BLOCKER | ✅ 成立 | 三处断言只做了一处（①有，②③没有） |
| F13 | MAJOR | ✅ 成立 | `bucket_key` 的 endpoint 分支不含 METHOD，违反 D337 |
| F14 | MAJOR | ✅ 成立 | 大写归一无 HTTP 层断言 |
| F15 | MAJOR | ✅ 成立（静态链完整） | 顶层无填租户 → 生产限流恒放行 |
| F16 | MINOR | ✅ 成立 | `status.md` 完成度表夸大 |
| F17 | BLOCKER | ✅ 成立（实测复现） | 导出线程 panic，测试仍绿 |
| F18 | MAJOR | ✅ 成立 | 「至少 2 层 span」零断言 |
| F19 | MINOR | ✅ 成立 | 新端点只有 3 行数据，翻不动页 |
| F20 | MINOR | ✅ 成立 | 装配顺序文档图漏了 deprecation 层（我复核确认） |
| F21 | NIT | ✅ 成立 | 2 条 lib 警告（`dead_code` + `unused_must_use`） |
| F22 | NIT | ✅ 成立 | 规格自身数错：实际 10 条 AC，写「9 条」 |

---

## 对升级与新证据的核验（复核收尾新增）

### 升级 F3 → BLOCKER：认同，且我找到了**更硬的证据**

复核不仅指出错误码不一致，还发现 **AC-S-034 的跨 crate 断言打在错的类型上**。
我独立验证并做了**变异实证**：

```console
$ grep -n "pub struct AuditPage" -A 10 crates/platform-audit/src/query.rs
#[derive(Debug, Clone, PartialEq, Eq)]     ← 没有 Serialize
pub struct AuditPage { ... }

$ grep -n "pub struct AuditPageView" -A 5 crates/platform-server/src/audit.rs
#[derive(Debug, Clone, Serialize)]          ← 这个才上线
#[serde(rename_all = "camelCase")]
pub struct AuditPageView { ... }
```

**变异实证**（我把 `AuditPageView.next_cursor` 改名为 `next_cursor_probe`，
然后跑那条「跨 crate 字段一致」的判据）：

```console
$ cargo test -p platform-observability --test http_contract the_pagination_envelope_matches
test the_pagination_envelope_matches_the_audit_field_names ... ok     ← 仍然绿！
test result: ok. 1 passed; 0 failed
```

→ **线上响应字段会从 `nextCursor` 变成 `nextCursorProbe`，判据无任何反应。**
这正是「测试全绿但判据没实现」的复现。改动已还原（`git status` 干净）。

### 升级 F15 → BLOCKER（实证）：认同

复核用**计数器 limiter** 做了带对照组的实验：

| 组 | 差异 | limiter 被调用次数 | 全部响应 |
|---|---|---|---|
| 实验组 | `client_info` 只填 IP/UA（**生产等价**） | **0 次** | 200 ×5 |
| 对照组 | 额外注入租户（**测试夹具做法**） | 3 次 | — |

唯一差异是「是否注入租户」。生产路径不注入 → `ctx.tenant_id()` 为 `None`
→ `middleware.rs:110-115` 走 TD-072 放行 → **limiter 的 `check()` 一次都没被调用**。

这比我的静态论证强得多 —— 它排除「某个我没看到的顶层中间件会填租户」的可能
（复核也核了：顶层 `layer` 只有一处，`client_info.rs` 里 `with_tenant` 出现 0 次）。

### 升级 F5 → BLOCKER：认同

`Some(false)` 在全 crate 只出现在注释里（我复核过）。503 分支一行都没被测。

### F20 · 成立

`http.rs:351-358` 的文档图写 `client_info → trace_id → rate_limit`，
实际挂载（`:430-433`）是 `client_info → deprecation → trace_id → rate_limit`。
**文档漏了一整层**，且这是唯一的装配实现。

### F21 · 成立

`cargo test --no-run` 实测 2 条 lib 警告，我已复现（见 F10 的输出）。

### F22 · 成立

```console
$ grep -c "^### AC-S-0" acceptance.md
10
```

`029~038` 是闭区间 **10 条**，而 `acceptance.md:42/392/427` 写「9 条」，
`status.md:14` 更写成「AC-S-029 ~ AC-S-037」（把 038 漏了）。
→ **「达成 9 / 部分 0」这个汇总的分母本身是错的**。


---

## 逐条证据（我亲自跑的命令与输出）

### F1 · 成立

```console
$ grep -c "AppView" openapi.json
0
$ python3 -c "import json; d=json.load(open('openapi.json')); print(d['components']['schemas'].keys())"
dict_keys([])
$ python3 -c "... print(d['paths'].keys())"
dict_keys(['/api/observability/rate-limit/status'])
```

`AppView` 是 `platform-app` 的 DTO，**从未**进 `ApiDoc`。按 AC-S-035 步骤 3
原文执行（改 `AppView.display_name`）→ 产物不变 → `--check` 仍 0 → **判据必红**。
D345 只处理了「覆盖端点少」，没处理「探针 DTO 不在产物里」。

### F2 · 成立

```console
$ grep -rn "prune" crates/ --include=*.rs | grep -v "rate_limit/mod.rs"
crates/xtask/src/main.rs:209:    let pruned = capabilities::write_files(root, &generated)?;
crates/xtask/src/capabilities.rs:174:    let pruned = prune_extra_files(...)       ← 无关（capabilities 的清理）
crates/platform-observability/tests/rate_limit_real_db.rs:413:  limiter.prune(...)   ← 测试
```

`PgRateLimiter::prune` **生产零调用**。`xtask/src/cli.rs:15-34` 的 `Command` 枚举
只有 `CheckModules/CheckDeps/Capabilities/CreateModule/TenantPolicies/Audit/Notify/Storage/Openapi`
—— **没有** `observability`。迁移注释（`20261123000100_rate_limit.up.sql:8`）承诺的
`xtask observability prune-rate-limit` **不存在**。`tech-debt.md` 里也**没有**对应 TD。

D337 原文：「清理**不是**可选项」「不实现就等于埋雷」。
→ 当前形态（有函数、有测试、绿灯、无人调用）比完全没写更误导。

### F3 · 成立

```console
$ sed -n '325,330p' crates/platform-audit/src/query.rs
fn invalid(reason: impl Into<String>) -> AuditError {
    AuditError::InvalidFilter { reason: reason.into() }
}
```

audit 的 `limit` 越界 → `AUDIT_INVALID_FILTER`；
observability 的 → `INVALID_REQUEST`（`http.rs:104-114` 硬编码）。
AC-S-034 步骤 4 明文「两者 `code == "INVALID_REQUEST"`」→ audit 侧不成立。
且 `query.rs:657/664` 的新测试**把旧码锁死**。

### F4 · **成立，但规格建议不可执行**

复核说「缺 `assert_ne!(pool_a.id(), pool_b.id())`」—— 事实是**规格自己写了不可执行的断言**：

```console
$ grep -n "pub(crate) pool" crates/platform-db/src/tenant.rs
88:    pub(crate) pool: PgPool,
```

1. `TenantDb.pool` 是 `pub(crate)` → 测试**拿不到** `PgPool`；
2. `sqlx::PgPool` **根本没有 `.id()` 方法**（那是 sqlx 0.8+ 的 `PoolConnection` 才有概念）。

→ 规格点名的那行代码**写不出来**。这不是实现的错，是**规格的错**（M1 修正引入的伪代码
没经过编译验证）。但「缺池身份断言」这个**实质问题成立**：
现有 `is_reachable` 两次是无区分力代理断言（若 `TenantDb::connect` 内部改成复用全局池，
两次照样 true）。

**处置**：改成可执行且有区分力的等价断言 —— 用两个池各自的
`SELECT pg_backend_pid()` 在**并发持有连接**时取到不同后端 pid
（同一池的两次查询可能复用同一连接，但两池并发持有必然是两个后端进程）。
同时修订规格原文。

### F5 · 成立

```console
$ grep -rn "503\|SERVICE_UNAVAILABLE" crates/platform-observability/tests/
（零命中）
$ grep -rn "Some(false)" crates/platform-observability/
crates/platform-observability/src/http.rs:223:  （注释里出现）
```

`ready` handler 的 503 分支在 `main.rs:2016-2033`（**bin crate**），测试够不着。
被测的只是 `rate_limit_readiness()` 纯函数。若把条件写成 `!= Some(true)`，
测试全绿。

### F6 · 成立

`middleware_assembly.rs:152-161` 只断言 `status == 429` 与 `retry-after` 存在；
`params` 闭集断言在 `error.rs:189-203`（直接构造错误对象）。
中间件 → `ProblemDetails` 这一层**没有断言**。

### F7 · **不成立（复核者口径错误）**

```console
$ # 按唯一包名
唯一名净增: 25 → 第三方: 24
$ # 按 Cargo.lock 条目数
总条目: 360 唯一名: 334 → 基线 329 条目
```

| 口径 | 基线 | 当前 | 净增 | 第三方 |
|---|---|---|---|---|
| **唯一包名**（D339 用的） | — | — | **25** | **24** |
| Cargo.lock 条目数（复核用的） | 329 | 360 | 31 | 30 |

D339 rev 2 原文（`decisions.md:163-174`）**明写**：

> 「基线 **329 包** → 本分支 **360 包**，净增 **25**，其中第三方 **24**」

且列出的 24 个名字与实测**逐个吻合**。复核者按「条目数」算出 31/30 是因为
`Cargo.lock` 里有多版本包（`getrandom` ×3、`windows-sys` ×3、`rand_core` ×3…共 23 个多版本名），
**rev 2 的实测早已注明口径是唯一名**。

→ F7 的「D339 账簿记错」**不成立**。但复核提出的**另一半成立**：
`lib.rs:23` 写「净增 **18** 包」是**抄了 D339 的预测值**，与 rev 2 实测的 24 冲突
（这是 MINOR 级的注释不一致，不是 MAJOR 级的账目错误）。

### F8 · 成立

```console
$ grep -n "build_subscriber" crates/platform-server/src/main.rs
（零命中）
```

`init_tracing`（`main.rs:254-286`）自己 `registry().with(fmt_layer).with(otel_layer).with(filter).init()`；
测试用 `platform_core::log::build_subscriber(...)`。共用的只有 `log_filter`。

规格 M4 的诉求原文是「生产 `init_tracing` 改成 `build_subscriber(level, writer)` 与测试共用」。
→ 部分满足（过滤器共用），但「同一个 subscriber」未做到。

### F9 · 成立

`lib.rs:166-168`：
- 说「`client_info` 填的租户」—— `client_info.rs` 全文只有 `with_client_ip` / `with_user_agent`，**不填租户**；
- 「还没写进去，还没写进去」是重复病句。

### F10 · 成立（编译警告实测）

```console
warning: unused `Result` that must be used
  --> crates/platform-observability/src/trace/middleware.rs:78:9
   |
78 |         span.set_parent(context);
   |         ^^^^^^^^^^^^^^^^^^^^^^^^
```

### F11 · 成立

`rate_limit_real_db.rs:196-197`：

```rust
let key1 = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/api/rbac/roles");
let key2 = bucket_key(dimension::ENDPOINT, TENANT_A, None, "/api/rbac/roles");
assert_eq!(key1, key2);   // ← 同一函数同一参数，恒真
```

全仓**没有**任何测试驱动**带 query** 的请求穿过 `rate_limit` 中间件。
若实现改成 `uri().path_and_query()`，此测试仍绿 → 变异不被抓。

### F12 · 成立

```console
$ grep -rn "FAKE\|injection\|注入" crates/platform-observability/tests/*.rs
（只有注释，无断言）
$ grep -n "^fn\|#\[test\]" crates/platform-observability/tests/trace_logging.rs
48:#[test]  49:fn the_log_line_carries_the_resolved_trace_id()
101:#[test] 102:fn a_generated_trace_id_also_appears_in_the_logs()
```

两个测试都验「合法值出现」，**没有**验「非法值不出现」。
响应头 ① 有断言（`otel_in_memory.rs:169`），② ③ 没有。

### F13 · 成立

`rate_limit/mod.rs:121-129`：

```rust
dimension::ENDPOINT => format!("rl:endpoint:{endpoint}"),
```

D337 原文（`decisions.md:70`）要求的形态是 `rl:endpoint:<METHOD>:<路径模板>`，
例 `rl:endpoint:GET:/api/rbac/roles`。AC-S-033 步骤 3 明文要求「`key` 含 `GET`」。
实测产出的 key **无 `GET`** → 按规格执行必红。

### F14 · 成立

```console
$ grep -rn "ABCDEF\|abcdef01" crates/platform-observability/tests/*.rs
（全是小写，无大写用例）
```

归一逻辑只在 `trace.rs:133-138` 单测里验。HTTP 层零覆盖。

### F15 · 成立（静态链路完整）

```console
$ grep -n "with_tenant" crates/platform-server/src/client_info.rs
（零命中 —— 只有 :53 的 with_client_ip / with_user_agent）
$ grep -n "with_tenant" crates/platform-rbac/src/http.rs
215:        ctx = ctx.with_tenant(tenant);
226:            ctx = ctx.with_tenant(verified.tenant_id.clone());
```

装配（`http.rs:421-433`）：`client_info`（最外） → `trace_id` → `rate_limit` → `nest("/api/rbac", ...)`
→ `authenticate`（**内层**才 `with_tenant`）。

`middleware.rs:109-115`：无租户 → `return next.run(request).await`（放行）。
→ **生产路径上 `ctx.tenant_id()` 恒为 `None`** → 三级限流恒不生效。

测试为什么绿：`middleware_assembly.rs:58-62` 的 `fake_client_info` **主动注入租户**
（`ctx.with_tenant("tenant-a").with_user("user-a")`），并注释「不注入租户的话，限流判据恒 200」。
**夹具做了生产 `client_info` 不做的事** —— 这是典型假绿。

⚠️ 我只做静态论证，**未**启动完整服务（700+ 行装配 + PG + 迁移）实测。这是本轮**未验证项**。

### F16 · 成立

`status.md:85-98` 的「达成 9 / 部分 0」与 F1/F2/F5/F11/F12/F13/F14/F18/F19 的实际覆盖不符。

### F17 · 成立（我复现了 panic）

```console
$ cargo test -p platform-observability --test otel_in_memory an_unreachable_collector -- --nocapture
thread 'OpenTelemetry.Traces.BatchProcessor' panicked at ...:
there is no reactor running, must be called from the context of a Tokio 1.x runtime
test an_unreachable_collector_does_not_slow_the_requests_down ... ok
test result: ok. 1 passed; finished in 0.00s
```

三重失效：① 导出线程 panic（路径没跑到）；② `127.0.0.1:1` 是**立即 ECONNREFUSED**（永不慢）；
③ 整条测试 0.00s（真的等超时不可能这么快）。
→ 若把生产改成 `with_simple_exporter`（M3 点名要防的写法），此测试**仍绿**。

### F18 · 成立

```console
$ grep -rn "parent_span_id\|层级\|2 层" crates/platform-observability/tests/
（零命中）
```

`otel_in_memory.rs:41-96` 只验 `spans.iter().find(|s| s.name == "http.request")` + trace_id 相等。
handler 是 `|| async { "ok" }` → **本来就没有子 span**。AC 要求「至少 2 层」零断言。

### F19 · 成立

`http.rs:150-166` 的 `vec![tenant, user, endpoint]` 是**硬编码 3 行**，`total` 恒为 3。
`limit=10` → `items` 只有 3 条；`nextCursor` 恒 `null`。
AC-S-034 步骤 1 的「恰好 10 条 / total=25」在新端点上**不可能成立**。

---

## 未验证项（明码记录）

| 项 | 为什么没验 |
|---|---|
| F15 的端到端确认 | 需启动完整服务（PG + 迁移 + 700 行装配），本轮未做。只有静态链路论证 |
| 真实 OTLP collector | TD-069 明码不做，本机无 collector |

---

## 处置计划

按「先堵生产缺陷，再补判据」排序：

**修代码（生产缺陷）**
1. **F15**：把租户判定从「顶层读 extensions」改为「无租户时不放行 AND 让限流能拿到租户」——
   最实做法是把 `rate_limit` 中间件下移到各子系统 `authenticate` **之后**（或让顶层用
   令牌解析租户）。这是设计级改动，需先定方案（见 `decisions.md` 新增 D346）。
2. **F2**：加 `xtask observability prune-rate-limit` 子命令 + `design-summary.md` 定期任务表。
3. **F13**：`bucket_key` 的 endpoint 分支加 METHOD。
4. **F3**：audit `invalid()` 改 `INVALID_REQUEST`。
5. **F9/F10**：注释与返回值。

**修判据（测试）**
6. **F11**：步骤 5 改为真驱动中间件（带 query 两次穿过 `rate_limit`）。
7. **F12**：补日志侧与响应体的非法值断言。
8. **F17**：改用黑洞地址 + 在 runtime 内构造 provider + 断言总耗时。
9. **F5**：把 `ready` 的 503 分支抽到可测函数，端到端断言。
10. **F6/F14/F18/F19**：HTTP 层补断言。

**修规格/文档**
11. **F1**：AC-S-035 步骤 3 探针 DTO 改为 `RateLimitRow.window_seconds`。
12. **F4**：AC-S-033 的 `pool_a.id()` 改为可执行断言。
13. **F16**：`status.md` 按实测重写。
14. **F7 的反面**：`lib.rs:23` 的「18」改为「24」。
