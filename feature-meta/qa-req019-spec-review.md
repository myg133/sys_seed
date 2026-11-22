# REQ-019 规格阶段挑刺

> 挑刺者：独立 verifier（非规格作者）
> 挑刺对象：demand/acceptance/decisions/design-summary rev 1
> 状态：已完成（对抗性审查）
> 基线代码：`feature-REQ-019` worktree = `develop` @ `0b9697b`（本文件所有代码引用均在该基线核实，未跑 `cargo test`）

## 审查快照（**重要：规格在审查期间被并发修改**）

我在 22:05 首次读取四件套，22:07–22:13 期间四个文件被改动（`ls -l` 的 mtime 可证），部分改动正好命中我正要报的问题。以下结论**已按最终快照逐条重新核对**：

| 文件 | 行数 | sha256（前 16 位） |
|---|---|---|
| demand.md | 91 | `74aafeb602bbe66d` |
| acceptance.md | 269 | `6bca6a0b48185e72` |
| decisions.md | 333 | `bce33b2013e76f22` |
| design-summary.md | 334 | `6489fcb60f000efd` |

审查期间已被作者自行修好、**我从缺陷清单撤回**的 4 项：
1. `demand.md` 原写「AC-S-029 ~ AC-S-036（8 条）」→ 已改为「029 ~ 037（9 条）」；实测 `acceptance.md` 确为 9 条。
2. `design-summary.md:112-138` 原「不能用 `len()` 判字节」却又用 `raw.len()` 的自相矛盾 → 已加推演说明（该写法**功能上确实安全**：能过 `is_ascii_hexdigit()` 的字符必为 ASCII，字节数 == 字符数）。
3. 新增 `design-summary.md:142-175`（§4.2 日志捕获）→ 部分回应了我的「日志怎么抓」问题（但见 M4，仍有两处硬伤）。
4. 新增 `design-summary.md:196-211`（§5 InMemorySpanExporter）与 `decisions.md:303-321`（D344 的配置降级实测）→ 部分回应了 B10 与「读不到配置怎么办」（见 B10 残留项、NIT-2）。

> 过程提醒：规格在挑刺期间被改，会让「审的是哪一版」说不清。建议挑刺期间冻结文件（或改动后重开一轮），并把 rev 号 + 指纹写进 status.md。

## 挑刺清单

### BLOCKER（判据不可执行 / 与既有已验收口径自相矛盾）

| # | 严重级 | 位置 | 问题 | 为什么不可接受 | 建议怎么改 |
|---|---|---|---|---|---|
| B1 | BLOCKER | `acceptance.md:81-85,91-92` vs `decisions.md:80` | AC-S-031 步骤 4 要求「`user-b` 在 `user-a` 被**接口维度**限流后，打**同一个**端点仍 200」（现版本只补了「先把租户维度调高」） | D337 的 endpoint 键是 `rl:endpoint:<METHOD>:<路径模板>`，**不含 user/tenant 段** → 同一租户内 `/api/rbac/roles` 只有一个桶。`user-a` 把这个桶打到上限 1 之后，`user-b` 打同一路径**必然** 429（`dimension=endpoint`）。**把 `tenant.requests` 调到 100 完全不影响这个桶**——作者 22:11 的补丁改错了维度。结果：**实现正确则判据必红，实现错误（endpoint 键里塞 user_id）才绿** | 步骤 4 改为测**用户维度隔离**：`endpoint.requests = 0`（D344 的 0=禁用）、`user.requests = 1`，先让 `user-a` 在用户维度超限，再验 `user-b` 打同端点 200。若坚持测接口维度，则必须改成「另一个**租户**」而不是另一个用户 |
| B2 | BLOCKER | `acceptance.md:34,39-40` | AC-S-029 步骤 4 断言 `platform_audit_log.trace_id` 逐字等于 `T`，但该表**没有 `trace_id` 列** | `platform_audit_log` 是「契约 C12 的 **13 个固定字段，不增不减**」（`migrations/20261120000200_audit.up.sql:7,45-59`；C12 见 `docs/contracts/api-conventions/v1.md:165-171`）。`AuditEvent`（`crates/platform-core/src/extension/audit.rs:101-120`）无 trace 字段，全仓 **17 处**构造点；`platform_audit_append` 是 9 个 TEXT 参数、哈希覆盖 `prev_hash + 11 字段`（同 SQL `:416-425,485-497`）。加这列要同时改在线表 + 归档表 + 函数签名 + **哈希公式**（改公式后历史行必然断链，直接砸 REQ-014 已验收的链校验 AC）。而 `design-summary.md:333-334` 仍写「platform-audit 的**分页**改动是**唯一**会动已验收需求的地方」 | 二选一并写死：(a) 升版契约 C12（14 字段）+ 迁移 + 哈希版本化 + 旧行兼容，并列进 §12 跨需求改动；(b) 不动 C12，改为旁挂关联（如 `platform_audit_trace(audit_id, trace_id)`）；同时把 AC-S-029 步骤 4 从「断言列名」改成「能按 trace_id 反查该审计行」 |
| B3 | BLOCKER | `acceptance.md:137,140-141` | AC-S-033 步骤 3 断言表里 `(tenant_id, key, **window_start**)` 的行「恰好 1 行，且**计数**反映两次请求」 | 三处不成立：① 滑动窗口表**没有 `window_start` 列**（`design-summary.md:38-44` 只有 `id/tenant_id/dimension/key/occurred_at`），SQL 直接报错；② 表里**没有计数列**（D337 明确弃用固定窗口的计数方案），「计数」概念不存在；③ 设计 §3（`design-summary.md:85-86`）是「先判定、后记账、超限直接返回」→ 被拒请求**不落行**，于是「1 行反映两次请求」永远不成立；而规格从未写明「被拒请求记不记账」 | 改成「按 `occurred_at > now() - window` 的**行数**判定」，并写明「**被拒请求不落行**」（这是设计 §3 已定的语义，必须进 AC 前置）；删掉 `window_start` |
| B4 | BLOCKER | `acceptance.md:160,165-168` | AC-S-034 要求 `limit=1000` → **400**（「超上限 100」）、`limit=0` → 400 且 `code=INVALID_REQUEST`；且步骤 URL 写成 `GET ...`（省略号），**没说跑哪个端点** | 与**已验收决策 D153 正面冲突**：今天 `limit=1000` 是**夹到 200 并返回 200**，且被单测锁死（`crates/platform-audit/src/query.rs:646-652`，文档 `:8,16`，`MAX_LIMIT=200` `:53`）；`limit=0` 今天是 `AUDIT_INVALID_FILTER`（`:292-301`）；契约 C7 写的上限也是 **200**（`docs/contracts/api-conventions/v1.md:118`）。规格零处声明这是破坏性修正。更糟的是省略号让判据两难：跑 audit → 打破已验收行为；不跑 audit → 「统一契约」对 audit 根本没验 | 二选一：(a) 显式声明 D153/C7 升版（上限 100、超限 400），列入 §12 并点名要改的既有测试；(b) 保持 D153，边界断言只用于新端点，audit 的不一致记 TD。并且**每个步骤写死具体 URL** |
| B5 | BLOCKER | `acceptance.md:154-155` vs `design-summary.md:296-306` | AC-S-034 前置要求「`GET /api/audit/events` 与**本需求新增的统一样式端点**」，但设计 §10 的 4 个端点里**没有任何新增的分页列表端点** | 步骤 1/3/4/5 与「跨 crate 一致性」全依赖这个不存在的端点 → 判据没有主语；也无法证明「契约统一」（只剩一个老端点可测） | 在 §10 加一个真实的新分页列表端点（选风险最低的只读列表），写明路径/数据规模/字段；或把 AC 降级为「只对齐 audit，一个端点不宣称跨 crate 统一」 |
| B6 | BLOCKER | `acceptance.md:191`；`design-summary.md:300` | AC-S-035 要求 `paths` 里含 `/api/health/live` | 实际路由是 **`/health/live`**（`crates/platform-server/src/main.rs:290`、`lib.rs:6`、`deploy/RUNBOOK.md:513`），全仓无 `/api/health/*`；设计 §10 也把 readiness 写成 `/api/health/ready`。按此实现，**正确产物这条必红** | 改为 `/health/live`、`/health/ready`；同步修 design §10 |
| B7 | BLOCKER | `acceptance.md:192` | AC-S-035 要求「每个 path 的响应里含 `application/problem+json`」 | 平台错误响应实际是 **`application/json`**（`crates/platform-core/src/problem.rs:296-308`，`Json(details)`），且 **D183（REQ-014）已明确把「要求 problem+json」当作 BA 写错的判据修正掉**（`BA/demands/REQ-014-audit/decisions.md:468,873,967`），C6 也未规定该媒体类型。照此写，产物**文档化一个从未发出的媒体类型**，而 AC 只看文档 → 假绿；若真改成 problem+json，则是 22 个 crate 的破坏性变更，设计完全没提 | 判据改为「声明与实际一致」：或把 AC 改成 `application/json`（与 D183 一致），或立项全平台切 problem+json 并声明 D109 破坏面。不能只改文档 |
| B8 | BLOCKER | `acceptance.md:31,38`；`design-summary.md:317` | AC-S-029 步骤 3 要求成功响应（`POST /api/apps` → 201）体里有 `.traceId` | `create_app` 返回 `AppView`（`crates/platform-app/src/http.rs:289-296`），该结构（`crates/platform-app/src/model.rs:253-273`）**没有任何 trace 字段**。满足它必须给业务响应体加 `traceId` = 改 REQ-011 已验收的响应契约；而 §12（`design-summary.md:322-331`）跨需求改动表里**没有这一项**，§11 反而写「业务响应体用 traceId。**不改现有契约**」——自相矛盾 | 二选一：(a) 步骤 3 改用**失败响应**（`ProblemDetails.trace_id` 已存在，`problem.rs:104`），不强制业务成功体带 traceId；(b) 真要做，写成独立跨需求改动（列 crate 清单 + D109 + 受影响的既有 DTO 测试） |
| B9 | BLOCKER | `acceptance.md:244-258` vs `demand.md:52-65` | 「AC 覆盖核对」宣称「**9 条功能要求** → 9 条 AC，**无遗漏**」 | 事实相反：demand 的功能要求只有 **7 条**（1~7），覆盖表的 9 行是把 trace/限流/429 拆开数的，并非 9 条功能要求。真正的问题是**第 3 条（就绪探针覆盖限流存储）没有任何 AC**，而设计 §10 第 1 行计划改 readiness，AC 列却填「AC-S-031 前置」（限流三维度与就绪探针无关，纯误引用） | 覆盖表按 demand 的 7 条逐条对账；给 readiness 改动补 AC（限流表不可达 → `/health/ready` 503 + 明确 `code`），或明确移出本期 |

### MAJOR（区分力不足 / 可执行性有坑）

| # | 严重级 | 位置 | 问题 | 为什么不可接受 | 建议怎么改 |
|---|---|---|---|---|---|
| M1 | MAJOR | `acceptance.md:139,143-148` | AC-S-033 声称「只有两个独立连接池才能区分」，并把「`pool_a.max_connections` 与 `pool_b` 不同」当补充判据 | 我逐条推演过：**每实例内存**实现下 A 计 1、B 见 0 → 200，判据确实成立（这点规格没错）。但「**进程内全局 static 计数器**」这个更省事的偷懒实现共享同一 static → B 也看到 1 → **429 假绿**；`max_connections` 不同根本证明不了是两个池。真正的区分力在步骤 3（DB 里有没有行），而步骤 3 现在是坏的（B3） | 判据重心移到步骤 3；补「第二个 `sqlx::Pool` 连不同连接串」或直接断言两池身份不同；删掉 `max_connections` 这条无区分力断言 |
| M2 | MAJOR | `acceptance.md:121-125` | AC-S-032 步骤 3 的泄露判据是「对响应体全文做小写**包含**检查」，检索项写作 `"current": 2`、`platform_rate_limit`、「**PG 相关字样**」 | ① 检索字面量带空格带具体数值 → 输出 `"current":2` 或 `"current": 3` 就漏；② 「PG 相关字样」是**无界集合**，写不成断言；③ 该防的是「`params` 里多出实现细节字段」，包含检查对它天然无效（多一个 `remaining`/`limit`/`bucket` 都抓不到）——任务单猜的「params 多一个 current」**正好是这个判据的盲区** | 改成**键白名单**：解析 429 体，断言 `params` 的键集合 ⊆ `{"dimension","retryAfter"}`；再对 `detail` 用有限黑名单（`platform_`、`SELECT`、`postgres`） |
| M3 | MAJOR | `acceptance.md:234-235` | AC-S-037 步骤 2 只断言「服务不崩、请求照常 **200**」 | D339 自己写的风险是「collector 不可达 → 内部重试阻塞 → **请求变慢甚至超时**」（`decisions.md:162-168`）。慢 3 秒仍返回 200 → 这条判据对**它要防的坑零区分力** | 加时延上界（如 20 次请求 p95 < 500ms 或单次 < 1s），与「导出超时 3s」形成可区分对照 |
| M4 | MAJOR | `acceptance.md:35,41`；`design-summary.md:142-175` | §4.2 说日志捕获「`platform-server/src/main.rs:1825` **已有现成机制，直接复用**」，并建议「用 `Handle::current().block_on(...)` 或另起线程」 | 两处硬伤：① `capture_logs` 在 main.rs 的 **`#[cfg(test)] mod tests`** 里（`main.rs:1794-1834`）——二进制 crate 的 `cfg(test)` 项**无法被 `tests/` 集成测试链接复用**，所谓「直接复用」不成立；② `Handle::current().block_on()` 在 async 测试里**会 panic**（tokio 文档：不能在 runtime 线程内 block）。规格给的用法本身就是踩坑 | 把 `capture_logs`（+`BufWriter`）移到 `platform-server/src/lib.rs` 的 `pub mod test_support`（非 cfg(test)），生产 `init_tracing` 改成 `build_subscriber(level, writer)` 与测试共用；日志 AC 明确用 **`current_thread` runtime + `with_default`**，禁止在 async 上下文 `block_on` |
| M5 | MAJOR | `acceptance.md:27-240`（6 条 HTTP 级 AC）；`design-summary.md:322-331` | AC-S-029/030/031/032/036/037 全是 HTTP 级判据，但 `platform-server` **没有可测的路由构建函数** | `crates/platform-server/src/lib.rs:9-20` 只导出模块；整个 `Router` 在 `main.rs:289` 起内联装配近 1500 行。既有全栈测试是**手工复刻**并自注「与 `main.rs` 相同的挂法」（`crates/platform-server/tests/app_real_stack.rs:207,227`）→ 判据测的是**测试自造的栈**，中间件漏装/顺序错都能绿 | 抽出 `platform_server::app::build_router(state) -> Router`，`main.rs` 与所有 HTTP 级测试共用；列进 §12 |
| M6 | MAJOR | `acceptance.md:37`；`design-summary.md:326` | AC-S-029 要求生成值「32 位小写 hex（无连字符）」，但现有生成器是 `Uuid::new_v4().to_string()`（**36 字符带连字符**，`crates/platform-core/src/context.rs:134-136`），还有单测**断言 `len()==36`**（`context.rs:156`） | 改格式要同时改生成器 + 这条既有断言；`design-summary.md:326` 却把 RequestContext 改动定级为「**低（新增方法，不改签名）**」——事实是改输出格式 + 改既有测试，且 `/health/live`、`/health/ready` 对外可见的 traceId 形态随之变化（`main.rs:1732-1741`）。风险定级错会让实现漏改 | §12 补两行：改 `new_trace_id()` 为 32 hex；同步 `context.rs:156` 的断言与 RUNBOOK 示例；风险改「中（可见格式变化）」 |
| M7 | MAJOR | `acceptance.md:66-104`；`design-summary.md:36-55` | 三级限流**未设计未认证请求**：`platform_rate_limit` 是 RLS 租户表，而 `TenantDb::begin` 在无租户时直接 `DbError::MissingTenant`（`crates/platform-db/src/tenant.rs:180`） | 登录/注册、公开 branding 等**认证之前**的请求没有租户 → 读不到写不了计数 → **登录暴力破解恰好落在限流覆盖不到的地方**。AC-S-031/032 只测已认证的 `user-a`/`user-b`，这个缺口判不出来 | 明确「无租户时用哪个桶」（IP 维度，或 sentinel 租户 + 非 RLS 路径），并补一条 AC：未认证请求超限也 429 |
| M8 | MAJOR | `acceptance.md:183,188,195-196` | AC-S-035 步骤 5 的「阳性对照」（注释掉 `#[utoipa::path]` 后 `--check` 退出码非 0）**可被编译失败满足** | utoipa 的 `#[derive(OpenApi)]` + `paths(...)` 引用 handler 生成的 `__path_*` 符号；注释掉属性后 `cargo xtask openapi --check` **编译失败**并以非 0 退出 → 判据红绿与 `--check` 的比对逻辑无关，证明不了它有区分力（只有步骤 3 是有效对照） | 步骤 5 改成「能编译但产物该变」的变更（改 DTO 字段名 / 加一条不重新生成的路由），并断言输出里出现**差异行**而不是任意非 0 |
| M9 | MAJOR | `acceptance.md:202-216`；`design-summary.md:303-306` | AC-S-036 只靠新造的 `/api/_deprecated-probe` 验弃用头，而设计自己留了未决问题「**考虑是否在生产构建中排除**」 | ① 冻结规格里留「考虑是否」= 执行时无人能判对错；② 若排除 → 生产构建里弃用机制**完全未验证**；③ 若保留 → 生产多一个公开测试端点；④ 弃用能力在**任何真实端点**上都没验过 | 二选一并写死：给真实端点标注 `Sunset`（推荐），或该端点用 `#[cfg(feature="test-routes")]` 且 AC 说明「在启用该 feature 的构建里验证」。删掉「考虑」 |
| M10 | MAJOR | `acceptance.md:54,59-60` | AC-S-030 步骤 4 只验「不回显到**响应头**」，却声称「日志、审计表任何一处」 | ① 日志/审计表的检查方法没给；② 审计表**没有 trace_id 列**（B2），这条对审计**真空成立**，等于没验；③ 测试值是 `../../etc/passwd`（路径穿越），而 D340 的威胁是**换行日志注入**（`decisions.md:190-196`）——**测的不是它声称要防的向量** | 测试值补 CRLF/控制字符（`abc\r\nFAKE LOG`、含 `\x1b`）；给出「日志全文 + 该请求写入的审计行文本」的可执行检查；审计侧等 B2 定案 |
| M11 | MAJOR | `decisions.md:206-233`；`acceptance.md:169-171` | D341 定「`nextCursor` / 上限 100 / **不提供 offset**」，与 C7 不一致，却只字未提改契约 | `docs/contracts/api-conventions/v1.md:118-119` 明写「响应含 `next_cursor`、**上限 200**，兼容分页 `?page=N&page_size=M`，二者互斥」；audit 现实现也确实同时提供 `limit`(≤200) 与 `page/page_size`（`crates/platform-audit/src/query.rs:128-141,178-186`）。「不提供 offset」与 C7 在效条款直接冲突，规格没声明契约升版，也没说 audit 的 `page/page_size` 是删是留 | 声明 C7 升版（字段名/上限/是否保留兼容分页），并在 AC-S-034 加一条「`?page=` 的行为」（保留或 400） |
| M12 | MAJOR | `acceptance.md:110,113,119` | AC-S-032 前置写「同 AC-S-031」，但步骤 2 要 `windowSeconds = 2` 等 3 秒；AC-S-031 前置明说三个维度**全部** `windowSeconds = 60` | 两段前置冲突：按 60 要等 61 秒（不可接受）；按 2 则 AC-S-031 的 60 不成立 | AC-S-032 写独立前置：「`endpoint.requests=1`、`endpoint.windowSeconds=2`，其余维度 `requests=0`」 |
| M13 | MAJOR | `acceptance.md:73-90` | AC-S-031 的 4 个步骤共用**同一个 60 秒窗口**，但步骤间没有重置；步骤 1 已把 `/api/rbac/roles` 的桶打满，步骤 2 又**第一个就打** `/api/rbac/roles`（`acceptance.md:76`）；步骤 3 还要 5 次请求 | 步骤 2 的首次请求就会撞上步骤 1 留下的 endpoint 桶（已 = 上限 1）→ 返回 429 `dimension=endpoint`，而 AC 期望「前 2 次 200」→ **同一测试内的判据互相污染**。步骤 3 的 5 次也需从空表开始才数得准 | 每个步骤写成独立场景（各自 truncate `platform_rate_limit` 或各用独立租户），并给出每步具体的 4~5 个端点清单 |
| M14 | MAJOR | `acceptance.md:226-237`；`design-summary.md:196-211` | AC-S-037 步骤 1 的一半判据仍无实现路径：①「`trace_id` 逐字等于响应头 `X-Request-Id`」；② 步骤 3 的 `is_enabled()` 观测者 | §5 新增的 InMemorySpanExporter 解决了「怎么拿到 span」，但它是**测试自建 provider**——生产 `init_otel` 没有注入缝，测试绕过了生产装配（与 M5 同类：测的不是生产栈）。更关键：**没有任何机制把入站头设成 OTel trace id**（需要 `TraceId::from_hex` + `SpanBuilder::with_trace_id` 或远端父上下文），D340 只写「可以直接用」；`is_enabled()` 也没说是谁的方法、由谁暴露 | ① `init_otel(cfg, exporter_override)` 或返回 provider 句柄，让测试走**生产装配**；② 明确 trace_id → OTel 的实现路径并写进 §4；③ 指定 `is_enabled` 的观测对象（如 `OtelGuard::is_enabled()`） |

### MINOR / NIT

| # | 严重级 | 位置 | 问题 | 为什么不可接受 | 建议怎么改 |
|---|---|---|---|---|---|
| m1 | MINOR | `acceptance.md:206,214` | AC-S-036 阴性对照没指定**用哪个**未弃用端点，也没要求与探针在**同一 router** | 若实现「按 router 整体加头」，取另一个 router 的端点做对照仍会绿 | 指定具体端点（如 `/api/rbac/roles`）并注明同一装配层级 |
| m2 | MINOR | `acceptance.md:116` | `Retry-After` 只断言「正整数秒」，没断言上界 | 返回 `999999` 也能绿，而「窗口 2 秒却让人等 11 天」是实际错误 | 补「≤ windowSeconds（向上取整，允许 +1 秒抖动）」 |
| m3 | MINOR | `acceptance.md:29` | AC-S-029 前置「服务已启动；`tenant-a` 有一个可写的业务对象」未说明**认证与权限准备**（token、`app:registry:write` 等） | 现有全栈测试为此要 ~200 行装配（`tests/app_real_stack.rs:160-250`）；前置不写清，容易被实现成「换个免权限端点」，弱化判据 | 前置写成「沿用 `app_real_stack` 的装配（IAM token + admin 权限 + 真实 audit sink）」并给出复用点 |
| n1 | NIT | `design-summary.md:300` | §10 第 1 行把就绪探针改动的 AC 列填成「AC-S-031 前置」 | 误引用：AC-S-031 是三维度限流，与 readiness 无关；且 B9 指出这条改动**没有 AC** | 补 AC 编号或标注「无 AC（B9）」 |
| n2 | NIT | `design-summary.md:34,57` | 任务单里「design 说 2 表」的推断**不成立**——§2「数据表」下只有 1 张表，2.2 是清理任务（`status.md` 也写「**1 表**」） | 前提有误（我不报为缺陷）；但把清理编号 2.2 挂在「数据表」下，读起来像第二张表 | 2.2 改名「清理任务（非表）」或并入 §9 |
| n3 | NIT | `design-summary.md:76-77`；`decisions.md:303-321` | 「读不到配置怎么办」已由 D344 新增段实测回答（降级到代码默认、不报错），但 §3 的判定流程仍只写「读配置」 | 两处口径未互指，实现者可能只看 §3 | §3 第 ① 步回指 D344 的降级模式 |

## 结论

**总条数 29：BLOCKER 9 / MAJOR 14 / MINOR 3 / NIT 3。**

### 最严重的 3 条

1. **B1（`acceptance.md:81-85`）AC-S-031 步骤 4 与 D337 的 endpoint 键设计互斥**——`rl:endpoint:<METHOD>:<路径模板>` 不含 user 段，`user-a` 打满后 `user-b` 打同一端点**必然** 429。作者 22:11 的补丁只把**租户**维度调高，**没有动被撞的那个桶**，所以补丁无效。这是最坏的一类判据缺陷：**实现正确则红，实现错误（键里塞 user_id）才绿**。
2. **B2（`acceptance.md:34,39`）AC-S-029 步骤 4 要求审计表 `trace_id`，而该表是 C12 的 13 个冻结字段、无此列**——加列要动在线表 + 归档表 + 追加函数签名 + **哈希公式**（断历史链），而 `design-summary.md:333` 还声称「platform-audit 的分页是**唯一**会动已验收需求的地方」。这条同时是**判据不可执行**与**跨需求破坏未声明**。
3. **B4（`acceptance.md:160,165`）AC-S-034 的 `limit=1000 → 400` 与已验收的 D153 正面冲突**——今天行为是「夹到 200 返回 200」且被单测锁死（`query.rs:646-652`），契约 C7 上限也是 200。规格零处声明这是破坏性修正，且步骤 URL 写成 `GET ...` 没写端点，导致「跑 audit 破坏历史行为 / 不跑 audit 没验统一契约」两难。

### 「判据不可执行」清单（最致命的一类）

- **B2**：断言一个不存在的列。
- **B3**：断言不存在的 `window_start` 列 + 不存在的「计数」语义。
- **B5**：AC-S-034 依赖一个**设计里不存在**的「新增统一样式端点」。
- **M14**：AC-S-037 的「OTel trace_id == `X-Request-Id`」与 `is_enabled()` 仍**没有实现路径/观测点**（§5 只解决了「怎么拿到 span」）。
- **M10**：「非法 trace_id 不得进日志/审计表」无检查方法，审计侧因无列而**真空成立**。
- **M2**：「PG 相关字样」是无界集合，写不成断言。
- **M4**：「直接复用 `capture_logs`」不成立（`cfg(test)` 私有），且建议的 `block_on` 会 panic。
- **M5**：6 条 HTTP 级 AC 没有共享的生产路由装配，只能在测试自造的栈上跑（可执行但**不测生产**）。
- **M8**：AC-S-035 步骤 5 的阳性对照可被「编译失败」满足，等于没验 `--check`。

### 对任务单 A~E 的逐条回应

**A. 区分力**

- **AC-S-033（跨实例）**：我独立推演过——**每实例内存**下 A 计 1、B 见 0 → B 放行 200，所以「步骤 2 能区分」**对这一种偷懒实现成立**（你的推演与我一致）。但它**对「进程内全局 static 计数器」不成立**（两实例共享 → 429 假绿）；真正的区分力在步骤 3，而步骤 3 引用了不存在的列（B3）。「两池 `max_connections` 必须不同」是**无区分力的代理断言**（M1）。
- **AC-S-031 跨维度隔离**：**确认成立，且比你描述的更严重**——不是「键设计错了也会绿」，而是「键**按 D337 设计正确**时这条**必红**」（B1）。
- **AC-S-035 阳性对照**：**部分多虑**——「改产物」(步骤 3) 与「改代码」(步骤 5) 两个方向都覆盖了，方向对；但步骤 5 大概率**因编译失败而通过**，证明不了 `--check` 有区分力（M8）。另外产物判据本身有两条事实错误（B6 路径、B7 媒体类型）。
- **AC-S-036 阴性对照**：**确认不足但没到致命**：只验「某端点不带头」且未指定端点与装配层级（m1）；核心问题是机制只在**自造探针**上验、且「是否在生产排除」还是未决（M9）。
- **AC-S-037 步骤 2「不崩」**：**确认不足**——D339 的风险是「重试阻塞 → 延迟」，「请求照常 200」对延迟零区分力（M3）。

**B. 可执行性**

- **AC-S-029 步骤 5 怎么捕获日志**：**确认有问题，且规格 22:12 的新增段没修好**——`capture_logs` 在 main.rs 的 `#[cfg(test)] mod` 内（`main.rs:1794`），集成测试**无法复用**；建议的 `Handle::current().block_on()` 在 async 测试里**会 panic**（M4）。
- **AC-S-037 怎么拿 span**：**部分解决**——§5 已给出 InMemorySpanExporter；但「trace_id 等于入站头」与 `is_enabled()` 仍无路径（M14）。
- **AC-S-031 步骤 2「把接口维度的键换成不同的」**：**已改善但仍不可执行**——22:11 已点名 3 个端点，可步骤 1 已耗尽 `/api/rbac/roles` 的桶、步骤间也不重置（M13）。
- 任务单没问但确实存在的：**AC-S-032 与 AC-S-031 的 `windowSeconds` 直接冲突**（60 vs 2，M12）。

**C. 自相矛盾**

- **8 vs 9 条 AC**：**确认过，但审查期间已被作者修好**（demand 现为「029~037（9 条）」），我从清单撤回。真正留下的问题是**谎报覆盖**：把「7 条功能要求」写成「9 条功能要求 → 无遗漏」，而就绪探针那条真的没 AC（B9）。
- **「2 表」**：**多虑，前提不成立**（§2 只有 1 张表，`status.md` 也写「1 表」）。
- **`/api/openapi.json` 算不算「新增端点用新契约」**：**不是矛盾，但暴露了真问题**——它不是分页列表端点，所以 AC-S-034 的「新增统一样式端点」在清单里**根本不存在**（B5）。另 `nextCursor` vs C7 的 `next_cursor`、`不提供 offset` vs C7 的 `page/page_size` 是**真冲突且未声明契约升版**（M11）。
- **D340「32 字符」vs `raw.len()`**：**原来自相矛盾，但已被作者补上推演。我核实后确认代码功能上是安全的**（能过 `is_ascii_hexdigit()` 的字符必为 ASCII → 字节数==字符数），不动它，撤回。仅建议把「先 `is_ascii()` 再 `len()`」写进代码示例以免读者误判。

**D. 安全判据**

- **AC-S-030 步骤 4**：**确认不够且不对题**——只验响应头；日志/审计无检查法，审计侧因无 `trace_id` 列而真空成立；测试向量是路径穿越而非 D340 声称的 CRLF 注入（M10）。
- **AC-S-032 小写包含检查**：**确认抓不住**——`"current": 2` 这种带空格带值的字面量、以及 `params` 里多一个 `current`/`remaining` 字段都抓不到（M2）。你猜的「params 多一个 current」**正好是这个判据的盲区**。
- **AC-S-031 的 endpoint 键「路径模板」怎么拿**：**这条你多虑了**——axum 0.8.9 确有 `MatchedPath`（`~/.cargo/registry/src/*/axum-0.8.9/src/extract/matched_path.rs:57`），且它在路由匹配后、调用被 layer 包裹的 endpoint **之前**写入 extensions（`src/routing/path_router.rs:394`），`Router::layer` 里也能取到，query 天然不在其中 → 「加随机 query 绕过」的判据可正常实现。**技术障碍不存在。**
- 但**确有安全缺口任务单没列**：未认证请求无租户 → RLS 租户表写不进 → 登录前暴力破解**限流不到**（M7）。

**E. 冲突风险**

- **1496 个测试**：**原判断成立，但作者已自行补上**——`decisions.md:303-321` 新增段实测确认「读不到 → 降级到代码默认值、不报错」，我核实其引用（`crates/platform-storage/src/config.rs:126-146` 的 `read(...).unwrap_or(default)`）**属实**，故降为 NIT（n3：§3 应回指 D344）。另 6 条 HTTP 级 AC 依赖全栈装配而无 router builder（M5），会逼实现去复刻 main.rs，反而加大「测试栈 ≠ 生产栈」风险。
- **「platform-audit 分页是唯一动已验收需求的地方」**：**确认是假话**，本轮最实质的发现之一。至少还有：审计表加 `trace_id`（B2，动 C12 + 哈希链）、错误响应媒体类型（B7，D183）、`limit` 上限与夹取行为（B4，D153 + C7 + 单测锁死）、业务响应体加 `traceId`（B8，REQ-011 DTO）。至于「给各 crate 加 `#[utoipa::path]` 算不算改动」——**算**（行为中性，但确实改已验收 crate 的源码与编译单元），设计已列进 §12 并标「低」，这点规格是对的。

### 写得扎实的（不硬找问题）

- **AC-S-032 的「窗口过期立即恢复 200」**（`acceptance.md:119`）：区分「限流」与「封禁」，方向正确，只差 M12 的配置一致性。
- **AC-S-033 的设计动机**（`:143-145`）：「单实例测试永远发现不了内存实现」这个判断**是对的**，我推演确认 per-instance 内存会被抓住；问题只在步骤 3 的列名与「全局 static」盲区。
- **AC-S-031 的「`?x=1` 与 `?x=2` 共用桶」**（`:98-100`）：真实攻击面，实现上可行（`MatchedPath` 可用），**这条判据有区分力**。
- **AC-S-031 新版「每条 429 都要断言 `dimension` 具体值」**（`:93-94`）：直接消除了「三个维度分不清」的假绿，是这一版最有效的一处改进。
- **AC-S-036 的「弃用 ≠ 下线，仍返回 200」**（`:215`）：抓得准，避免了「一标弃用就 410」的常见破坏。
- **AC-S-034 的「字段名断言写包含、不写等于（D194）」**（`:169-171`）：正确遵循了跨栈断言口径。
- **`design-summary.md:123-138` 对 `len()` 的推演**：自己把坑推演清楚了，比直接删掉更有价值。

---

## 处置回执（BA，2026-11-22）

**31 条全部处置，无一条以「多虑」为由驳回。** 逐条记录：

### BLOCKER（10 条）

| # | 处置 | 改在哪 |
|---|---|---|
| B1 | **改判据**：AC-S-031 步骤 4 从「同租户 `user-b` 打同一端点」改为**跨租户**（`tenant-b` 的 `user-c` 打同一路径仍 200）。D337 补「三个桶的实际作用域」表说明根源 | `acceptance.md` AC-S-031；`decisions.md` D337 |
| B2 | **接受**：AC-S-029 移除审计表断言，改为日志 / 失败响应体 / OTel span 三处。父 AC-028 原文只要求「日志、链路、异步消息」，是我过度设计。审计带 trace_id → **TD-071** | `acceptance.md` AC-S-029；`tech-debt.md` TD-071 |
| B3 | **接受**：AC-S-033 步骤 3 改用 `occurred_at` 的**行数**，删掉 `window_start` 与「计数」，并写明「被拒请求不落行」 | `acceptance.md` AC-S-033 |
| B4 | **接受并声明破坏**：D341 增加契约 C7 / D153 升版对照表（上限保持 200 不动，只把「超上限」从夹取改 400），点名要改的单测 `query.rs:646-652` | `decisions.md` D341；`design-summary.md` §12 |
| B5 | **接受**：设计补真实分页端点 `GET /api/observability/rate-limit/status`（含响应示例） | `design-summary.md` §10 / §10.1 |
| B6 | **接受**：路径改 `/health/live` / `/health/ready`（实测 `main.rs:290-291`） | `acceptance.md` AC-S-035；`design-summary.md` §10 |
| B7 | **接受**：改断言 `application/json`（`problem.rs:305` 实际；D183 已裁定） | `acceptance.md` AC-S-035 |
| B8 | **接受**：改测**失败响应体**的 `ProblemDetails.trace_id`；§11 删掉「业务体用 traceId」的自相矛盾表述 | `acceptance.md` AC-S-029；`design-summary.md` §11 |
| B9 | **接受**：覆盖表按 demand 的 **7 条**逐条对账；就绪探针补 **AC-S-038**（含「关闭限流时不该因限流表不就绪」的判据） | `acceptance.md` AC-S-038 + 覆盖表 |
| B10 | **接受**：设计加 `init_otel(cfg, exporter_override)` 注入缝 + `OtelGuard::is_enabled()`，并写明 `TraceId::from_hex` + 远端父上下文的**具体实现路径**（不用父上下文 OTel 会自己生成 id → AC 必红） | `design-summary.md` §5.1 / §5.2 |

### MAJOR（14 条）

| # | 处置 |
|---|---|
| M1 | AC-S-033 删掉「`max_connections` 不同」这条无区分力断言，改为 `assert_ne!(pool_a.id(), pool_b.id())`；重心移到步骤 3（DB 里真有行）；**明码记录盲区**（全局 static 计数器无法与 PG 区分） |
| M2 | AC-S-032 步骤 3 改为**键白名单**（`params` 的键集合 ⊆ `{dimension, retryAfter}`）+ `detail` 的**有限黑名单**；删掉「PG 相关字样」这种无界集合 |
| M3 | AC-S-037 步骤 2 加**时延上界**（单次 < 1 秒），与「导出超时 3s」形成对照 |
| M4 | 设计加 `build_subscriber(level, writer)` 抽出（production 与测试共用），并写明 `with_default` 是线程局部的、跨 `.await` 不可靠 |
| M5 | 设计新增 §11.5：**必须抽 `build_router(deps)`**，`main.rs` 与所有 HTTP 测试共用同一装配路径 |
| M6 | §12 明确 `new_trace_id()` 改 32 hex 会动 `context.rs:156` 的 `len()==36` 断言，风险从「低」改「**中（对外可见格式变化）**」 |
| M7 | 设计新增 §2.3：未认证请求**明码不做**（→ **TD-072**），并要求中间件遇无租户**直接放行**（否则登录端点 500） |
| M8 | 设计新增「读配置的失败语义」：未登记 / 缓存无快照 / 读失败**一律 fail open + WARN**，并写明「读不到就报错会让 1496 个测试集体红」 |
| M9 | AC-S-035 步骤 5 从「注释掉 `#[utoipa::path]`」（会编译失败）改为「**改一个 DTO 字段名**」（能编译但产物该变），并断言输出含**差异行** |
| M10 | 删掉 `/api/_deprecated-probe` 与「考虑是否在生产排除」；改为在**真实端点** `/api/audit/events` 上标注弃用（分页升版给了合理理由） |
| M11 | AC-S-030 补**5 条路径**（含 CRLF 注入向量 `\r\nFAKE LOG LINE`），并把「不得回显」拆成**三处**逐处断言（响应头 / 日志 / 失败响应体）；**不再声称验审计表**（真空成立） |
| M12 | D341 增加与 C7 的关系说明（保留 `page/page_size`；新增端点只用 cursor），并要求实现时**同步更新 C7 文档** |
| M13 | AC-S-032 前置改为**独立**（`endpoint.windowSeconds = 2`，另两维度禁用），不再引用 AC-S-031 的 60 秒 |
| M14 | AC-S-031 改为**5 个独立场景**，每步开头 **truncate**，并列出**四个具体端点**（不写省略号）；步骤 2 用「禁用 endpoint 维度」隔离变量 |

### MINOR / NIT（7 条）

| # | 处置 |
|---|---|
| m1 | `demand.md` 改为「AC-S-029 ~ AC-S-038（9 条）」 |
| m2 | 确认「2 表」是我推断错误（设计本就只有 1 张表）；`design-summary.md` §2.2 改为「清理任务（**非表**）」 |
| m3 | D340 改为「**先判 ASCII 再判长度**」的代码顺序，删掉「不能用 `len()`」那半句，改为澄清「先判 ASCII 后 `len()` 无歧义」 |
| m4 | AC-S-036 阴性对照指定 `GET /api/rbac/roles`，并注明**必须同一 router** |
| m5 | AC-S-032 补 `Retry-After` 上界：`1 ≤ retryAfter ≤ windowSeconds + 1` |
| m6 | §10 第 1 行的 AC 列从「AC-S-031 前置」改为 **AC-S-038** |
| m7 | AC-S-029 前置写明「沿用 `app_real_stack` 的装配（IAM token + ADMIN 权限 + 真实 audit sink）」 |

### 我确认挑刺说对了、但我的任务单猜错的

- **AC-S-031 跨维度隔离**：任务单猜的是「键设计错了也会绿」，实际更严重 ——
  「键**按 D337 设计正确**时这条**必红**」（B1）。挑刺比我判断得准。
- **`MatchedPath` 可用性**：任务单担心「拿不到路径模板」，挑刺查了 axum 源码确认可用
  （`axum-0.8.9/src/extract/matched_path.rs:57` + `path_router.rs:394`），
  我自己也实测过（`?x=1` → `/api/rbac/roles`）。**技术障碍不存在**。
- **「2 表」**：任务单的推断不成立（设计只有 1 张表）。挑刺帮我避免了改错地方。

### 挑刺覆盖了我的盲区（最有价值的部分）

B2（审计表没有 trace_id 列）、B4（limit 与 D153 冲突）、B7（problem+json 与 D183 冲突）、
B8（`AppView` 无 traceId）、M5（无可测 router）、M7（未认证请求写不进 RLS 表）、
M8（读配置 None 的语义）—— **这 7 条我完全没想到**，
其中 B2 / B4 / B7 / B8 都是「我的判据与已验收决策正面冲突且我没意识到」。
