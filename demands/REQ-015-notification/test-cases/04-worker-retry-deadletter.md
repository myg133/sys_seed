# 04 异步 worker、重试退避、死信重投（AC-S-170 / AC-S-171 / AC-S-172）

> 测试落点：`crates/platform-notify/tests/notify_worker.rs`（170）+
> `crates/platform-notify/src/worker.rs` 内联单测 + `tests/notify_retry.rs`（171）+
> `tests/notify_resend.rs`（172）。全部需**真实库**（`base_test`），`base` 连接造数。
> worker 是**数据库驱动**（`xtask notify worker` / `tick()`，D296）—— 用例里一律直接调
> `tick(db, ctx, batch)`，**不用**进程内队列，也不 sleep 等后台任务。

## AC-S-170：`FOR UPDATE SKIP LOCKED` + 多 worker 不重复不遗漏

> ⭐ **本节的核心判据是「两个数字同时断言」**：入队 100 → 全部 `delivered`（**不遗漏**）
> **且** provider 调用次数**恰好 100**（**不重复**）。
> 计数来源同 01：`CountingSender`（`Arc<AtomicUsize>`，每次 `send()` +1）。
> 只断言「100 条全 delivered」会漏过「投两次但状态只记一次」的重复投递 —— 那类坏实现
> 的 DB 状态看起来完美，只有计数器能抓。

### TC-015-058 主路径：100 条 + 4 并发 tick（各取 30）→ 全 delivered 且计数恰好 100
- **AC**: AC-S-170 步骤 1-3 ｜ **类型**: 功能/可靠性 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库，并发）｜ **方法**: 等价类 + 并发
- **前置**: `base_test` 库已建；`base` 连接可直连造数；`in_app` 位挂 `CountingSender`（初始计数 0）；100 条 `status='pending'` 且 `next_attempt_at <= now()`
- **步骤**: 1) 用 `base` 直连插入 **100 条**可立即投递的记录（`next_attempt_at = now() - interval '1 second'`）2) `tokio::join!` 并发跑 **4 个** `tick(db, ctx, 30)`（**各自独立事务 + 独立连接**）3) 汇总：查 100 条的状态分布与 `CountingSender.count()`
- **预期**: ① **`delivered` 的行数 == 100**（**不遗漏**；逐字等于 100，不是 ≥100）② **`CountingSender.count() == 100`（恰好 100，不重复）** ③ `pending` / `delivering` 行数均为 **0**；④ `dead_letter` 行数 **0**；⑤ 每行 `attempt_count == 1`（**100 行全部逐行为 1** —— 若某行是 2，说明它被取了两次）；⑥ **两个数字缺一不可**：只断言①时，「同一行被投两次、状态仍是 delivered、attempt_count 仍是 1」的坏实现会假绿

### TC-015-059 4 个 worker 拿到的行集合两两不相交（SKIP LOCKED 真的生效）
- **AC**: AC-S-170 步骤 2（`SKIP LOCKED`）｜ **类型**: 可靠性 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库，并发）｜ **方法**: 因果
- **前置**: 同 TC-015-058 的 100 条数据；让每个 `tick()` 把「本次认领到的 `id` 集合」写进一个共享的 `Mutex<Vec<Vec<Uuid>>>`（测试专用注入点）或从 `TickStats` 读 `claimed_ids`
- **步骤**: 1) 并发 4 个 `tick(..., 30)` 2) 收集 4 个批次各自的 `claimed_ids` 3) 求两两交集
- **预期**: ① 4 个批次的行数之和 **== 100**（4×30 = 120 上限，但只有 100 条 → 合计恰好 100）；② **任意两个批次的交集为空**（`A∩B == ∅`、`A∩C`、`A∩D`、`B∩C`、`B∩D`、`C∩D` 六组全为空）—— 这是 `FOR UPDATE SKIP LOCKED` 的直接判据；③ 若实现漏了 `SKIP LOCKED`，两个批次会拿到同一批行 → 交集非空 → 红（且 TC-015-058 的计数会 > 100）；④ 每个批次 `len() <= 30`（batch 上限生效）

### TC-015-060 路径 2：`next_attempt_at = now()+1h` 的行不被取出
- **AC**: AC-S-170 步骤 4 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 边界值
- **前置**: 库中仅有 1 条 `status='pending'`、`next_attempt_at = now() + interval '1 hour'`
- **步骤**: 1) 记 `CountingSender.count()` 2) `tick()` 一次 3) 查该行 4) 再读计数
- **预期**: ① 该行 `status` **仍是 `"pending"`**（逐字，"pending"）；② `attempt_count` **仍是 0**；③ `CountingSender.count()` **不增长**（仍等于步骤 1 的值）；④ `TickStats.claimed == 0`（本次一条都没认领）；⑤ `next_attempt_at` 值**逐字未变**（未被提前改写）

### TC-015-061 路径 3：`delivering` 残留 10 分钟 → 被回收并投递成功
- **AC**: AC-S-170 步骤 5 ｜ **类型**: 可靠性 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: 1 条记录 `status='delivering'`、`updated_at = now() - interval '10 minutes'`（模拟方案 A 的崩溃残留）
- **步骤**: 1) `tick()` 一次 2) 查该行 3) 读计数
- **预期**: ① 该行最终 `status == "delivered"`（**不再卡在 `delivering`**）；② `CountingSender.count()` **+1**（**恰好 1 次**投递，不是 0 次也不是 2 次 —— 0 次说明没回收，2 次说明回收后还重复取了一次）；③ `attempt_count` 从原值 +1；④ 判据含义：`delivering` 只由超时回收，**不是**终态（design §4.3 方案 A 的必需配套）

### TC-015-062 路径 3 补强：`delivering` 才 5 秒（未超阈值）→ 不抢，仍 `delivering`
- **AC**: AC-S-170 步骤 5（阈值边界的另一侧）｜ **类型**: 可靠性 ｜ **优先级**: P1 ｜ **自动化**: 集成（真实库）｜ **方法**: 边界值
- **前置**: 1 条 `status='delivering'`、`updated_at = now() - interval '5 seconds'`
- **步骤**: 1) `tick()` 2) 查该行 3) 读计数
- **预期**: ① 该行 `status` **仍是 `"delivering"`**（未超阈值 → 不回收）；② `CountingSender.count()` **不增长**；③ 与 TC-015-061 合起来证明「回收有阈值」而不是「见到 delivering 就抢」—— 若无阈值的实现，本条会把一个**正在投递中**的行抢走 → 双投递（计数 +1 → 红）

### TC-015-063 路径 4：对已 delivered 的行再 tick 3 次 → 计数不再增加（幂等）
- **AC**: AC-S-170 步骤 6 ｜ **类型**: 功能/可靠性 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: 1 条 `status='delivered'` 的记录，记下此时 `CountingSender.count() = N`
- **步骤**: 1) 连续 `tick()` **3 次** 2) 读计数 3) 查该行
- **预期**: ① 3 次后 `CountingSender.count()` **仍是 N**（**逐字相等**，不增长）；② 该行 `status` **仍是 `"delivered"`**；③ `attempt_count` **不变**；④ `delivered_at` **逐字节不变**（终态不被重写）；⑤ `TickStats.claimed == 0` × 3（`delivered` 不参与出队 —— 出队条件是 `status IN ('pending','delivering')`）

### TC-015-064 崩溃在「已发送、未写回」之间 → 重投一次（at-least-once 的明码边界）
- **AC**: AC-S-170（方案 A 的语义边界）｜ **类型**: 可靠性 ｜ **优先级**: P1 ｜ **自动化**: 集成（真实库）｜ **方法**: 错误推测
- **前置**: 模拟「投递成功但写回事务失败」：用测试注入让 `CountingSender` 成功返回后，写回 UPDATE 抛错并回滚；该行留在 `delivering` 且 `updated_at` 置为 10 分钟前
- **步骤**: 1) 触发一次（计数 +1，库状态未变）2) 把 `updated_at` 改到 10 分钟前 3) `tick()` 一次 4) 读计数与该行
- **预期**: ① `CountingSender.count()` **变成 2**（一次真发了、一次回收后重发）—— 这是 at-least-once 的**已知代价**，不是缺陷；② 该行最终 `status == "delivered"`；③ 本条的作用：**把这个边界写成断言**，防止后人看「计数 == 100」就以为系统是 exactly-once 的；④ 若实现承诺 exactly-once，本条会红 → 说明设计变了，必须回报 BA

## AC-S-171：重试退避 —— 指数 + 上限 + 次数用尽进死信

> ⚠️ **最能区分「写死」与「读配置」的一条**：TC-015-068 把 `maxAttempts` 改成 **2**，
> 断言**第 2 次**失败后就 `dead_letter`。写死 5 次的实现会活到第 5 次 → 红。

### TC-015-065 主路径：连续 5 次失败 → 第 1~4 次 pending，第 5 次 dead_letter
- **AC**: AC-S-171 步骤 1 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: 配置为默认值（`maxAttempts=5` / `retryBaseSeconds=30` / `retryMultiplier=2` / `retryMaxSeconds=3600`）；`FakeSmtp` **恒定失败**（`MAIL FROM` 回 `451`）
- **步骤**: 1) 入队 1 条邮件通知 2) 连续 `tick()` 5 次，每次前把 `next_attempt_at` 置为已到期（或直接测 `tick` 的「忽略退避只取到期」分支后手动拨表）3) 每次后读 `status` / `attempt_count`
- **预期**: ① 第 1 次后 `status=="pending"`、`attempt_count==1`；② 第 2 次后 `pending` / **2**；③ 第 3 次后 `pending` / **3**；④ 第 4 次后 `pending` / **4**；⑤ **第 5 次**后 `status=="dead_letter"`、`attempt_count==5`（**不是第 6 次** —— `maxAttempts` 含首次）；⑥ `FakeSmtp.call_count("MAIL") == 5`（与 `attempt_count` 严格一致）；⑦ `last_error` 非空且含 `451`

### TC-015-066 步骤 2：`backoff(attempt)` 对 1..4 恰好返回 `[30,60,120,240]` 秒
- **AC**: AC-S-171 步骤 2 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数，不跑 worker 不连库）｜ **方法**: 等价类
- **前置**: 直接调纯函数 `backoff(attempt, base=30, multiplier=2, max=3600)`
- **步骤**: 对 `attempt = 1, 2, 3, 4` 各调一次，取 `as_secs()`
- **预期**: ① 结果**恰好** `[30, 60, 120, 240]`（**4 个值**，按序逐字比对数组相等）② **负向断言：不是 5 个值** —— 第 5 次失败直接进死信，不再算延迟，所以 `backoff(5)` **不该**被调用；若实现返回 5 个值说明它对死信那次也算退避了 → 与 AC-S-171 步骤 1 的「第 5 次即死信」矛盾；③ `backoff(0)` 的返回值与 `backoff(1)` 相同（首字母语义，边界不可 panic）

### TC-015-067 路径 2：base=2000、multiplier=2、max=3600 → attempt=2 恰好 3600
- **AC**: AC-S-171 步骤 3 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数）｜ **方法**: 边界值
- **前置**: `backoff(2, base=2000, multiplier=2, max=3600)`
- **步骤**: 调一次，取 `as_secs()`
- **预期**: ① 结果 **恰好 `3600`**；② **区分力全部来自上限**：`2000 * 2 = 4000 > 3600` → 被 clamp 到 3600；若没有上限（或上限写错成 4000）结果会是 **4000** → 红；③ 追加断言：`backoff(1, 2000, 2, 3600) == 2000`（第一个值未触顶，证明 clamp 不是无脑常量）；④ `backoff(3, 2000, 2, 3600) == 3600`（8000 也被截断）

### TC-015-068 ⭐ 路径 3：`maxAttempts=2` → **第 2 次**失败即 dead_letter（证明读配置）
- **AC**: AC-S-171 步骤 4 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 因果
- **前置**: 通过平台配置把 `notify.maxAttempts` 改成 **`2`**（其余保持默认）；`FakeSmtp` 恒定失败
- **步骤**: 1) 确认配置已生效（读配置接口返回 `maxAttempts == 2`）2) 入队 1 条邮件通知 3) 连续 `tick()` 直到死信（最多 3 次）4) 记录每次后的 `status` / `attempt_count`
- **预期**: ① 第 1 次后 `status=="pending"`、`attempt_count==1`；② **第 2 次**后 `status=="dead_letter"`、`attempt_count==2`；③ **关键区分力**：`FakeSmtp.call_count("MAIL") == 2`（**恰好 2 次**）—— 写死 5 次的实现会投 5 次 → 计数 5 → 红；④ 第 3 次 `tick()` 不再投递（计数仍 2）；⑤ 本条与 TC-015-065（配置 5 → 死信在第 5 次）配对：**只改一个配置项，死信点随之移动**，这才证明「配置真的被读了，不是写死」

### TC-015-069 路径 3 补：`retryBaseSeconds=1` 时退避真的变短（配置三项联动）
- **AC**: AC-S-171 步骤 4（配置生效的补充面）｜ **类型**: 功能 ｜ **优先级**: P1 ｜ **自动化**: 单测（纯函数）+ 集成 ｜ **方法**: 因果
- **前置**: 把 `notify.retryBaseSeconds` 改成 `1`（`multiplier=2`、`max=3600`）
- **步骤**: 1) `backoff(1..=3, base=1, mult=2, max=3600)` 2) 入队 1 条失败通知，`tick()` 一次后读 `next_attempt_at`
- **预期**: ① 纯函数返回 `[1, 2, 4]`（**不是** `[30,60,120]` —— 写死 base 的实现会红）；② 库层 `next_attempt_at - updated_at` 约等于 **1 秒**（容差 ±1s）；③ 对照 TC-015-066 的 `[30,60,120,240]`：同一个函数、改一个配置、返回值变 → 配置通路有效

### TC-015-070 路径 4：退避计算不用浮点（`f64` / `as_secs_f64` 命中 0，含阳性对照）
- **AC**: AC-S-171 步骤 5 ｜ **类型**: 可维护性 ｜ **优先级**: P0 ｜ **自动化**: 静态扫描 ｜ **方法**: 边界值
- **前置**: `crates/platform-notify/src/worker.rs` 已交付（本轮代码）
- **步骤**: 1) `grep -c "f64\|as_secs_f64" crates/platform-notify/src/worker.rs` 2) **阳性对照**：`grep -c "Duration::seconds" crates/platform-notify/src/worker.rs` 3) 再扫 `as_secs_f64` 于整个 `crates/platform-notify/src/`
- **预期**: ① 第 1 步结果 **`0`**（逐字 0）；② 阳性对照第 2 步 **`> 0`** —— 证明扫的是**真代码文件**不是空文件（否则 0 命中不可信，dev-guide §9.2）；③ 第 3 步全 crate 范围 `as_secs_f64` 命中 **0**；④ 附加：`backoff` 的返回类型是 `time::Duration`（用 `time` crate，D297），不是 `std::time::Duration` —— 断言源码里 `use time::Duration` 存在

### TC-015-071 退避计算的溢出防护：attempt 极大时不 panic、不回绕
- **AC**: AC-S-171 步骤 1（design §4.4 溢出防护）｜ **类型**: 可靠性 ｜ **优先级**: P1 ｜ **自动化**: 单测（纯函数）｜ **方法**: 边界值
- **前置**: 直接调 `backoff(attempt, 30, 2, 3600)`
- **步骤**: 1) `backoff(62, ...)` / `backoff(63, ...)` / `backoff(64, ...)` / `backoff(i32::MAX, ...)` 各调一次（用 `catch_unwind` 包裹）
- **预期**: ① **4 次都不 panic**（`catch_unwind` 全 `Ok`）；② 4 次返回值**都等于 `3600`**（被上限截断，**不是**负数、**不是** 0、**不是**回绕的奇怪值）；③ 判据：`2^63` 在 `i64` 上会溢出 → 实现若直接 `multiplier.pow(attempt)` 就会 panic 或回绕（design §4.4 明写「必须防溢出」）；④ 附加断言：返回值 `> 0`（回绕成负数会红）

## AC-S-172：死信人工重投 —— 只重置死信条目 + 影响行数守卫

### TC-015-072 步骤 1：resend `D_dead` → 200 且 3 个字段全重置
- **AC**: AC-S-172 步骤 1 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: `D_dead.status=='dead_letter'`、`attempt_count==5`、`last_error` 非空、`next_attempt_at` 在未来；管理员持 `notify:delivery:read`
- **步骤**: 1) `POST /api/notify/deliveries/{D_dead}/resend` 2) 读响应 3) 查库回读
- **预期**: ① HTTP **200**；② 响应 `status == "pending"`；③ 响应 `attemptCount == 0`；④ 响应 `nextAttemptAt <= now()`；⑤ 响应 `lastError == null`（**被清空**）；⑥ 库回读与响应**逐字段一致**；⑦ 其余字段不变：`renderedBody` 逐字节不变、`templateCode` 不变、`channel` 不变

### TC-015-073 步骤 2：resend 已 delivered 的 `D_ok` → 409 且 3 字段逐字不变
- **AC**: AC-S-172 步骤 2 ｜ **类型**: 功能/安全 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 错误推测
- **前置**: `D_ok.status=='delivered'`，记下其 `status` / `attempt_count` / `next_attempt_at` / `delivered_at` 四个原值
- **步骤**: 1) `POST /api/notify/deliveries/{D_ok}/resend` 2) 查库回读四个字段
- **预期**: ① HTTP **409**；② `code=="NOTIFY_NOT_DEAD_LETTER"`；③ `status` **逐字仍是 `"delivered"`**；④ `attempt_count` **逐字等于原值**；⑤ `next_attempt_at` **逐字节等于原值**；⑥ `delivered_at` **逐字节等于原值**（`delivered` 是终态，重投不得把它变回 pending）

### TC-015-074 步骤 3：resend 一条 pending → 409 且字段不变
- **AC**: AC-S-172 步骤 3 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 错误推测
- **前置**: `D_pending.status=='pending'`、`attempt_count==2`（已失败过 2 次，正在退避），记下 `attempt_count` 与 `next_attempt_at`
- **步骤**: 1) `POST /api/notify/deliveries/{D_pending}/resend` 2) 回读
- **预期**: ① HTTP **409**；② `code=="NOTIFY_NOT_DEAD_LETTER"`；③ **关键**：`attempt_count` **仍是 2**（**不是 0** —— 若实现无 `status='dead_letter'` 条件，这条会被「重置」成 0，把用户正在退避的通知打回起点）；④ `next_attempt_at` **逐字节不变**；⑤ 与 TC-015-073 合起来：`delivered` 与 `pending` 两种非死信状态都被拒

### TC-015-075 步骤 4：resend 不存在的投递 → 404 **NOT_FOUND**（平台通用码）
- **AC**: AC-S-172 步骤 4 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 等价类（非法输入）
- **前置**: 库中不存在该 id
- **步骤**: `POST /api/notify/deliveries/does-not-exist/resend`
- **预期**: ① HTTP **404**；② `code` **逐字为 `"NOT_FOUND"`**（平台通用码，**不是** `NOTIFY_NOT_DEAD_LETTER`、**不是** `NOTIFY_TEMPLATE_NOT_FOUND`）；③ `Content-Type: application/problem+json`；④ 本需求**不新增**「投递记录不存在」专用码（AC-S-172 末尾口径：一个只有一条 AC 的 404 分支不值得占 `TITLES` 一行）—— 若实现抛了专用码 → 红，且须回报 BA

### TC-015-076 ⭐ 步骤 5：并发 2 个 resend 打同一 `D_dead` → 恰好 1 个 200、1 个 409
- **AC**: AC-S-172 步骤 5 ｜ **类型**: 可靠性 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库，并发）｜ **方法**: 并发 + 因果
- **前置**: `D_dead.status=='dead_letter'`；**如何构造并发**—— 两条路任选，必须在用例里写死用哪条：
  - **(a) 双连接 + `tokio::join!`**：两个独立 `TenantDb` 连接（事务各自独立），同时 `join!(post_resend(id), post_resend(id))`。为消除「先到的先跑完」导致第二个必然看到 `pending` 的假并发，在两次 UPDATE 之间用 `pg_advisory_xact_lock`/测试钩子让两个请求**都在 UPDATE 之前**进入（或用 `tokio::sync::Barrier` 在两个 handler 的「进入事务后、执行 UPDATE 前」同步）。
  - **(b) 直接双 SQL 并发**（更稳、不依赖 HTTP 层时序）：两个连接各自 `BEGIN; UPDATE ... WHERE id=$1 AND status='dead_letter' RETURNING *;` 用 `Barrier` 保证两条 UPDATE 同时发出，各自读 `rows_affected`。
  **推荐 (b)**：判据落在「SQL 影响行数」上，正是 AC 要求守卫的位置。
- **步骤**: 1) `Barrier::new(2)`，两个 task 同时执行上面 (b) 的 UPDATE 2) 各自读 `rows_affected` 3) 两条都 `COMMIT` 4) 查 `D_dead`
- **预期**: ① 两个 `rows_affected` 的集合**恰好是 `{1, 0}`**（**1 成功 1 失败**，不是 `{1,1}`、不是 `{0,0}`）；② 转成 HTTP 语义就是**恰好 1 个 200、1 个 409**；③ 最终库中该行 `status == "pending"`、`attempt_count == 0`（只被重置**一次**，不是两次叠加）；④ **判据的反面**：若实现是「先 `SELECT` 查状态、再 `UPDATE`」，两个事务都会读到 `dead_letter` → 两个都成功 → `rows_affected == {1,1}` → 红；⑤ 用 `Barrier` 而不是「连着发两次」，是为了让本条**真的**并发；顺序执行的两次必然一成一败，那样的测试对「SELECT 后 UPDATE」的坏实现**也是绿的**（假绿）

### TC-015-077 并发 resend 后计数只 +1（不因并发产生双投递）
- **AC**: AC-S-172 步骤 5 ｜ **类型**: 可靠性 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库，并发）｜ **方法**: 因果
- **前置**: TC-015-076 的并发已发生（该行已 `pending` / `attempt_count=0`）；`FakeSmtp` 改成成功；记下 `CountingSender`/`FakeSmtp` 当前 `MAIL` 计数 `N`
- **步骤**: 1) `tick()` 一次 2) 读计数 3) 读该行
- **预期**: ① `MAIL` 计数 **变成 `N+1`**（**恰好 +1**，不是 N+2）；② 该行 `status == "delivered"`、`attempt_count == 1`；③ 若并发那次「两个都成功」，重置会叠加但状态仍只有一个 pending 行 —— 计数看不出差别，**所以本条必须与 TC-015-076 的 `rows_affected` 判据配对使用**，单靠本条不足以抓问题（写明这一点，防止后人以为计数能替代行数守卫）