# 状态：REQ-019 可观测性与 API 治理

- **状态**: ⚠️ **实现部分完成**（**3 条 AC 达成 / 7 条部分**，未合并 —— 见下方「实现完成度」）
- **需求来源**: ⚠️ **BA 补充**（用户原始需求未提可观测性）——
  用户 2026-11-19 裁定「出了工作流，其他的继续做」→ **本需求继续做**（D121 / D122）
- **更新者**: myg133-ba-1
- **更新时间**: 2026-11-22

## 规格四件套

| 文件 | 状态 |
|---|---|
| [demand.md](demand.md) | ✅ rev 1（含**实测现状基线** 9 项 + 需求来源披露） |
| [acceptance.md](acceptance.md) | ✅ rev 1（***9 条 AC**：AC-S-029 ~ AC-S-037） |
| [decisions.md](decisions.md) | ✅ rev 1（**D337~D344**，8 条） |
| [design-summary.md](design-summary.md) | ✅ rev 1（**1 表** / 4 端点 / 12 交互点） |
| `test-cases/` | ✅ **67 条**（TC-019-001~067，9/9 AC 覆盖，含 6 条对照类） |
| 独立挑刺 | ✅ **31 条**（BLOCKER 10 / MAJOR 14 / MINOR 6 / NIT 1）**全部处置** |

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-17 23:50 | 草稿 | 草稿 | myg133-ba-1 | 依赖浅，调整到 Sprint 6 |
| 2026-11-22 | 草稿 | **已就绪** | myg133-ba-1 | 规格从 **1 条空泛 AC → 9 条**；用户裁定三项技术口径（D337~D339）；含实测现状基线 |
| 2026-11-22 | 已就绪 | **已就绪（rev 2）** | myg133-ba-1 | 独立挑刺 **31 条**全处置：B1 判据奖励错误实现 → 改跨租户；B2 审计表无 trace_id 列 → TD-071；B4 limit 与 D153 冲突 → 声明契约升版；B5 分页端点不存在 → 补；B6/B7/B8 三处事实错误；B9 谎报覆盖 → 补 AC-S-038；B10 OTel 无注入缝 → 加缝 |

## 本轮的关键裁决（用户 2026-11-22）

| # | 裁决 | 依据 |
|---|---|---|
| 1 | **限流 = PG 滑动窗口**（存时间戳） | 用户裁定；零新依赖 + 多实例准确 → **D337** |
| 2 | **OpenAPI = 引入 `utoipa`**（**第三次依赖破例**） | 实测净增 **2** 包（`utoipa` + `utoipa-gen`）→ **D338** |
| 3 | **OTel = 真接 OTLP**（**第四次依赖破例**） | 实测净增 **18~27** 包 → **D339** |

## 基线实测（2026-11-22，不是推测）

| 项 | 实测现状 |
|---|---|
| `trace_id` 贯穿 | ⚠️ **部分**：`RequestContext` 有、响应体带，**日志里没有** |
| 结构化日志 | ⚠️ **部分**：`tracing_subscriber::fmt()` 已用，缺 trace_id 关联 |
| RFC 9457 错误格式 | ✅ **已有且扎实**（`ProblemDetails`，22 个 crate 在用） |
| 健康检查 / 就绪探针 | ✅ **已有**（`/health/live` + `/health/ready`） |
| **三级限流** | ❌ **完全没有**（只有 IAM 登录单点防护，无租户维度） |
| 分页 / 排序 / 过滤 | ⚠️ **不统一**（audit 有 cursor；rbac / org 完全没有） |
| **OpenAPI** | ❌ **完全没有** |
| **弃用头** | ❌ **完全没有** |
| **OpenTelemetry** | ❌ **完全没有** |

## 依赖破例实测（`cargo generate-lockfile`，基线 329 包）

| 配置 | 净增包数 |
|---|---|
| `utoipa = "5"` | **2**（`utoipa` + `utoipa-gen`） |
| OTel 默认（全 features） | **24** |
| OTel `http-proto` | **18** |
| OTel + `with-serde` | **27** |
| **实现后实测（rev 2）** | **24 个第三方**（总包数 329 → 360） |

## 阻塞项

**无。** 前置需求（REQ-002）早已完成。

## 已知代价（明码记账）

- **TD-069**：无 collector → **真导出无法端到端验证**
- **TD-070**：分页只做样板，其余 crate 按需迁移（跨需求改动）
- 限流每请求一次 SELECT + INSERT → 对 PG 有额外压力（D337 记了取舍）

## 下一步

1. 派**独立 verifier** 对四件套做对抗性挑刺（**不自己判自己** —— REQ-018 的教训）
2. 挑刺问题全修
3. 产出用例集
4. 实现


---

## 实现完成度（2026-11-22 实测）

> ⚠️ **本需求尚未通过验收。** 下面逐条说明，不拿「跑绿了 483 条测试」当完成证据。

| AC | 结论（**rev 4，复核后**） | 判据（`file:line` + 函数名） |
|---|---|---|
| **AC-S-029 trace_id 三处同值** | **达成**（rev 4 补齐 F8） | span：`otel_in_memory::the_exported_span_carries_the_incoming_trace_id`（变异验证）；日志：`trace_logging::the_log_line_carries_the_resolved_trace_id`；失败响应体已有。**rev 4**：新增 `build_subscriber_with_layer`，生产的 `init_tracing` 与测试**走同一条构造路径**（原先生产手工重建 subscriber） |
| **AC-S-030 入站校验** | **达成**（rev 4 补齐） | `trace` 单测 5 路径 + `http_contract::the_trace_middleware_echoes_...`；**rev 4 新增**：`middleware_assembly::an_uppercase_incoming_trace_id_is_lowercased_on_the_wire`（步骤 2 的 HTTP 层判据，F14）+ `trace_logging::an_illegal_incoming_trace_id_never_reaches_the_logs`（**三处逐处断言**之②③，F12；变异验证：加一行 `raw=?incoming` → 红）+ `an_illegal_incoming_trace_id_never_reaches_the_error_body`（之③） |
| **AC-S-031 三级独立** | **达成**（rev 5 修复 F15） | 真库 15 条扎实（三级各自 + 跨租户 + 禁用不记账）；**rev 4**：`the_query_string_does_not_bypass_the_rate_limit_bucket`（步骤 5 真判据，F11；变异：改完整 URI → 红）+ endpoint 键**含 METHOD**（F13）；**rev 5（D346，用户裁定方案①）**：新增 `TokenTenantResolver` + `RateLimitState.token_resolver`，中间件在 ctx 无租户时**验令牌**拿身份 → `the_rate_limit_works_under_the_production_client_info`（**三组对照**：A 有令牌→**429**+limiter 被调用／B 无令牌→200+不调用／C 有令牌无解析器=修复前→200+不调用；变异：解析器置 None → **A 红**）+ `an_invalid_token_is_treated_as_unauthenticated` + `an_existing_tenant_context_skips_the_token_resolver` |
| **AC-S-032 429 语义** | **达成**（rev 4 补齐 F6） | retry_after 正整数 + 上界、params 键闭集、窗口过期恢复；**rev 4 新增**：`middleware_assembly::the_429_body_only_exposes_the_two_allowed_params`（**HTTP 层解析真实 429 响应体**；变异验证：加 `remaining` 键 → 红） |
| **AC-S-033 跨实例** | **达成**（rev 4 补齐 F4/F13） | 两个独立池 + DB 里恰好 1 行；**rev 4**：`the_fixture_can_build_two_independent_handles` 现在含**池身份断言**（并发持连比较 `pg_backend_pid()` —— 规格原写的 `pool_a.id()` **写不出来**，规格已同步修正）+ `the_endpoint_key_differs_per_http_method` |
| **AC-S-034 分页契约** | ⚠️ **部分**（TD-079） | audit 翻页零重叠 + `limit` 超限 400；**rev 4**：错误码从 `AUDIT_INVALID_FILTER` 改为 **`INVALID_REQUEST`**（新增 `AuditError::InvalidPagination`，F3）+ `the_audit_http_response_uses_the_shared_field_names`（解析**真实 HTTP 响应**；复核已实证原「编译期断言 `AuditPage`」改名不红）+ `this_endpoint_has_at_most_three_rows_so_it_cannot_page`（锁能力边界，F19）。⚠️ **新端点恒 3 行、翻不动页** → TD-079 |
| **AC-S-035 OpenAPI** | **达成**（rev 4 修正规程 F1） | 机制成立（生成 + `--check` 逐字节 + 阳性对照）+ `xtask` 5 条单测；**rev 4**：步骤 3 探针 DTO 从 `AppView`（**不在产物里**）改为 `RateLimitRow.windowSeconds`（**实测**：加 `#[serde(rename)]` → 退出码 **1**，输出第 62/74 行差异；还原 → 0） |
| **AC-S-036 弃用头** | **达成** | `http_contract` 5 条：正向 + 阴性对照 + 子路径不继承 + 空清单零开销；变异验证 |
| **AC-S-037 OTel** | ❌ **不成立**（TD-078） | 步骤 1：trace_id 相等（变异验证）+ **rev 4 新增** `the_exported_spans_are_at_least_two_levels_deep`（**至少 2 层**，断言子 span 的 `parent_span_id` == 根的 `span_id`；变异验证：去掉子 span → 红，F18）；步骤 3 已验。⚠️ 步骤 2 的时延判据**原版三重失效**（秒拒地址 + 导出线程 panic + 单次上界量级太宽）；rev 4 改黑洞地址 + 总耗时上界，但**导出超时场景仍无法端到端复现**（BatchProcessor 在普通 OS 线程上 `reqwest` panic）→ TD-078 |
| **AC-S-038 就绪探针** | **达成**（rev 4 补齐 F5） | **rev 4 新增** `tests/readiness.rs` **7 条**：判定逻辑抽成可测函数 `readiness_rate_limit_outcome`，覆盖步骤 1（200 + `checks.rate_limit=ok`）/ 步骤 3（**503 + `code=RATE_LIMIT_UNAVAILABLE`**）/ 步骤 4（**关闭时存储不可达仍 200 且不查库**）。变异验证：`None` 误判为 Err → **3 条红** |

**汇总（rev 5）：达成 9 / 部分 1 / 不成立 1。**

> rev 5 变化：**AC-S-031 从不成立 → 达成**（用户 2026-11-23 裁定方案①，D346 修法：顶层验令牌）。
> 仍「部分」的 1 条是 **AC-S-034**（新端点恒 3 行翻不动页 → TD-079）；
> 仍「不成立」的 1 条是 **AC-S-037**（导出超时无法端到端验证 → TD-078，技术栈固有限制）。

> ⚠️ **rev 1~3 声称的「达成 9 / 部分 0 / 不成立 0」是错的。**
> 独立复核产出 **22 条 FINDING**（BLOCKER 8 / MAJOR 7 / MINOR 5 / NIT 2）。
> 我逐条亲验：**21 条成立 / 1 条不成立** ——
> **F7 不成立**：复核按「`Cargo.lock` 条目数」算出净增 31/30，而 D339 rev 2 用的是
> 「**唯一包名**」口径 25/24（两种口径都对，D339 原文已注明口径且 24 个包名逐个吻合）。
> 详见 `BA/feature-meta/qa-req019-review-verify.md`。
>
> **rev 4 时「不成立」的 3 条**（rev 5 后只剩 1 条）：
> - **AC-S-031** → rev 4 不成立（F15/TD-077 实证生产装配下限流恒不生效，limiter 被调用 0 次）；
>   **rev 5 已修复**（D346，用户裁定方案①：顶层验令牌 + 注入租户解析器）→ 转「达成」；
> - **AC-S-037**：步骤 2 的时延判据无法端到端验证（TD-078，技术栈固有限制）→ **仍未解决**。
>
> **工作量**：
> - rev 4：新增/改写 **19 条**测试判据（observability 105 → **122**），
>   其中 **6 条做过变异验证**（F5 / F6 / F11 / F12 / F13 / F18 全部能被对应变异打红）；
> - rev 5：新增 **4 条**（observability 122 → **126**），其中 **1 条做过变异验证**
>   （组 A：解析器置 None → 红）。

## 阻塞项（rev 5）

| # | 内容 | 状态 |
|---|---|---|
| ~~**TD-077**~~ | ~~三级限流在生产装配下恒不生效~~ | ✅ **已解决**（D346，用户裁定方案①：顶层验令牌 + 注入租户解析器） |
| **TD-078** | 导出超时无法端到端验证（技术栈固有：BatchProcessor 在普通 OS 线程上跑 reqwest） | 明码记录 |
| **TD-079** | 跨 crate 分页只验字段名、没验行为（新端点恒 3 行） | 明码记录 |
| **TD-080** | 规格自称「9 条 AC」，实际 10 条 | **rev 4 已修正正文** |
| **TD-073** | OpenAPI 只覆盖 1 个端点 | 明码记录 |
| **TD-072** | 未认证请求不受限流保护 | 明码记录 |
| **TD-070 / TD-071** | 分页只做样板 / 审计表无 trace_id | 明码记录 |

### 结构风险已处理（TD-075 → 已解决，遗留 TD-076）

中间件装配已抽成 `apply_all_middlewares_with`（main 与测试**调同一个函数**），
并有 5 条装配契约判据（trace 挂上 / limit 挂上 / 弃用头挂上 / **client_info 在最外层**）。

⚠️ **这个测试立刻抓到 2 个真实的顺序 bug**（都不是「漏挂」，而是「顺序反了」）：
1. `apply_observability` 里先挂 trace 后挂 limit → axum 的 layer 是「后挂在外层」
   → **limit 先执行**、读不到租户 → 按 TD-072 放行 → **限流永久不生效**；
2. `apply_all_middlewares_with` 里先挂 client_info → 它变内层 → trace_id 读不到租户/IP。

→ 两者都是「代码看起来对、功能静默失效」。修正方式：挂载顺序 = 执行顺序的**反向**。

⚠️ **仍未做**：完整 `build_router` 抽取（**路由挂载**那一层，20+ 个 `nest` 仍内联）
→ **TD-076**（从 REQ-002 起就存在的债，不是本需求引入的）。

### 变异验证（本轮累计 5 个，全部命中）

| 变异 | 命中的用例 |
|---|---|
| endpoint 键加 user 段 | 2 条（同租户共享桶 + 跨租户隔离） |
| `retry_after` 固定 999999 | 1 条（上界判据） |
| `>=` 改 `>`（差一） | 8 条 |
| **去掉 `set_parent`** | 2 条（span trace_id + 非法值传播） |
| 弃用头不挂 | 1 条 |
| `limit` 改回静默截断 | 1 条 |

### 本轮实测数字

| 项 | 实测 |
|---|---|
| platform-core | **153** passed |
| platform-observability | **74** passed（59 单测 + 15 真库） |
| platform-audit | **56** passed |
| platform-server | **131** passed（+5 ignored） |
| xtask | **69** passed |
| **合计** | **483 passed / 0 failed** |
| 限流变异验证 | **3 个全部命中**（endpoint 键加 user → 2 条红；retry_after 固定 → 上界红；`>=` 改 `>` → 8 条红） |
| OpenAPI 阳性对照 | 篡改产物 → 退出码 **1**；还原 → **0** |
| 依赖净增 | **24 个第三方包**（329 → 360） |

⚠️ **483 passed 不等于需求通过** —— 缺的 7 条 AC 的**端到端判据**恰恰是最容易出问题的地方
（REQ-018 的教训：测试全绿但判据根本没实现）。

### 未做的结构性项

- **`build_router` 抽取**（挑刺 M5）：`main.rs` 仍是 1900+ 行内联装配，
  6 条 HTTP 级 AC 若要端到端测，仍需先在测试里复刻栈（或先做这一步）。
- **`init_tracing` 的 otel layer**：见 TD-074。

### 下一步（按优先级）

1. **TD-074**（P1）：把 otel layer 挂进 subscriber → AC-S-037 步骤 1 才能达成
2. **`build_router` 抽取**（P1）：不做的话端到端测试测的不是生产装配
3. 补 7 条 AC 的端到端用例（trace / 分页 / 弃用 / 就绪）
4. **TD-073**（P2）：给各 crate 加 utoipa 标注，扩大 OpenAPI 覆盖面
