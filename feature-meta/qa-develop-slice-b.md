# develop 切片 B（数据与事务）独立 QA

> **基线**：`develop @ 164e32a`，`git status --short` 全程干净（末次确认见文末）。
> **范围**：`crates/platform-{db,storage,audit,notify}/src`（约 18400 行）。
> **方法**：定向 grep 找高危模式 + 真库（`base_test`，PG 17.12）实测。
> 所有造数用 `qa-*` 租户前缀，每条命令末尾清理；审计/通知/存储表已终检为 0 残留。
> 应用角色为 `base_app`（`rolsuper=f`、`rolbypassrls=f`），RLS 真实生效。
> 无法改生产代码 → 唯一的源码变异已还原（`tenant.rs`，见 F7）。

## 结论

**整体风险：不可放行。** 四个 crate 的**静态设计**（`{tenant}` 占位符强制、
`SET LOCAL` 语义、成功走 `SECURITY DEFINER` 写通道、append-only 靠 REVOKE）质量很高，
四个 crate 的测试也**全绿**（`platform-db` 38+5、`storage` 19、`audit` 8+2、`notify` 13+7）。
但「测试绿」掩盖了三类真实缺陷：

1. **最严重 —— 审计链在并发归档下产生错误告警（F4，BLOCKER）**：
   `platform_audit_append` 持 `pg_advisory_xact_lock`，而 `platform_audit_archive_move`
   **完全不持锁**却读改写 `head_hash`/`tip_hash`。实测用仓库**自己的** `ChainVerifier`
   在并发归档后得到 `ok=false` + `error` 级告警 —— 链本身完好（`link_ok=true`），
   是基准（`tip_hash`）被陈旧值覆盖。**3/3 稳定复现**。
2. **`batch` 上限失效（F2/F3，MAJOR）**：通知 `claim_batch` 与存储 `claim_jobs`
   都用 `id IN (SELECT … LIMIT n FOR UPDATE SKIP LOCKED)`。在 nested-loop 重扫计划下
   `LIMIT` 不生效：实测存储侧 3000 行队列、`LIMIT 10` **认领 3000 行（全部）**。
3. **关键判据无测试覆盖（F7，MAJOR 测试盲区）**：把 `set_config(..., true)` 变异成
   `false`（= 普通 `SET`，会**串租户**）后，`platform-db` 38 个测试**全绿**。

## FINDING 清单

| # | 严重级 | 位置(file:line) | 问题 | 证据 |
|---|---|---|---|---|
| F1 | MAJOR | `migrations/20261120000200_audit.up.sql:416-526` | `platform_audit_append` 是四个 `SECURITY DEFINER` 审计函数里**唯一缺租户守卫**的；可跨租户写链 | 真库实测：上下文 `tenant-VICTIM` → 成功写入 `tenant-ATTACKER` |
| F2 | MAJOR | `crates/platform-notify/src/store.rs:459-475` | `claim_batch` 的 `LIMIT` 在重扫计划下失效，`batch` 不是上限 | 陈旧统计 + 3000 行队列，`LIMIT 10` → 认领 **60** |
| F3 | MAJOR | `crates/platform-storage/src/store.rs:337-356` | `claim_jobs` 同源缺陷，实测可**吞下整条队列** | 2000 行 → 认领 2000；3000 行 → 认领 3000（`LIMIT 10`） |
| F4 | BLOCKER | `migrations/20261120000200_audit.up.sql:608-687` | 归档不持 advisory lock，覆盖 `tip_hash` → 校验器误报断链 | 真实 `ChainVerifier`：`ok=false`，`hash_mismatch`，3/3 复现 |
| F5 | MAJOR | `crates/platform-audit/src/verify.rs:234-242` | `verify` 无分页、全量载入，无并发上限 | 实测 5 万条 = 29 MB 载荷一次性进内存 |
| F6 | MINOR | `crates/platform-notify/src/store.rs`（`insert_delivery`）+ 无 prune | 每次 enqueue 写一行且**无任何清理路径**；同项目限流表有 `prune` 对照 | `grep DELETE/prune` 在 notify crate 为 0 命中 |
| F7 | MAJOR | `crates/platform-db/src/tenant.rs:187` | 最关键的 `SET LOCAL` 判据**无测试覆盖**（变异全绿） | 变异 `true`→`false`，38 测试全绿；且实测会串租户 |
| F8 | MINOR | `crates/platform-audit/src/query.rs:426-444` | `total` 与 `items` 在 READ COMMITTED 下两次读，可不一致 | 代码结构证据（未实测出不一致） |
| F9 | NIT | `crates/platform-storage/src/store.rs`（job/job_error 表） | 任务表与错误表无清理、无 FK CASCADE | `DELETE FROM` 在 storage store 仅 1 处（对象） |
## 逐条详情

### F4 · BLOCKER · 归档与写入并发时 `tip_hash` 被陈旧值覆盖，真实校验器误报断链

**规格要求**：`migrations/20261120000200_audit.up.sql:33-40` 明确写：

> ⚠️ **禁用** `SELECT … ORDER BY created_at DESC LIMIT 1 FOR UPDATE` 锁上一条：
> READ COMMITTED（sqlx 默认）下它**防不住**链分叉 …… → 用
> `pg_advisory_xact_lock(hashtext('audit:' || tenant_id))` 串行化同租户写入

`crates/platform-audit/src/archive.rs:10-16` 声称「归档**不破坏链**（D159）」。

**实测代码**：

- `platform_audit_append` **有**锁 —— `audit.up.sql:457`：
  `PERFORM pg_advisory_xact_lock(hashtext('audit:' || p_tenant_id));`
- `platform_audit_archive_move`（`audit.up.sql:608-687`）**完全没有**这把锁
  （对 608-690 行 `grep advisory` 为 0 命中），但它在同一个事务里做了：
  - `628-633` 读**在线表尾** hash → `v_tail_hash`
  - `640-654` `DELETE` 在线段 + `INSERT` 归档表
  - `669-673` 读**在线表新尾**
  - `664-683` `INSERT … ON CONFLICT (tenant_id) DO UPDATE SET head_hash=…, tip_hash=…`

写方持锁、归档方不持锁 → 归档在「读新尾」与「写 chain_state」之间，
写入方可以提交一条新记录，而归档随后用**自己读到的旧值**覆盖 `tip_hash`。

**实测证据（不是推断）**

1. SQL 层交错，**3/3 稳定复现**：
```
online = [old3, brand_new]      -- brand_new 已成功写入
head   = 52e23463               -- ✓ 正确（= 被搬走段的尾）
tip    = a4b16d82               -- ✗ 陈旧（应为 brand_new 的 f54ced80）
```

2. **用仓库自己的 `ChainVerifier` 跑真库**（临时集成测试，已删除、已还原）：
```
QA_STATE  online=["qa.old3","qa.brand_new"]  tip=b5b4e5d5
QA_REPORT ok=false checked=2
  broken=[(hash_mismatch,
           expected="b5b4e5d5…"   ← 被覆盖的 tip_hash
           actual  ="43a95bad…")] ← 真实链尖
```
即 `verify.rs:294-307` 的末尾截断检测命中，并按 `verify.rs:315-324`
打 **error 级**结构化告警。

3. **可达性**：归档入口是 `POST /api/audit/archive`
（`crates/platform-server/src/audit.rs:483-488` → `archive_older_than`）
与 `xtask audit archive`；写入是常态流量 → **重叠是必然，不是巧合**。
函数内也没有任何外层加锁（`grep advisory` 在整个 `platform-audit/src` 只有 sink 注释命中）。

**为什么不可接受**：链**本身完好**（`first_prev_ok=true`、`link_ok=true`），
只有基准被覆盖就报「断链」。合规场景下「审计链校验失败」是必须上报的事件；
运维收到 error 级告警后，真正的篡改告警会被淹没。

**已确认的缓解（不改变定级）**：竞态后**再来一次 append** 会把 `tip_hash`
重写为正确值（实测 `tip_ok_after_append=true`）。所以是「窗口内的错误告警 +
窗口内校验结论错误」，非永久损坏。但校验是随时可被运维/合规调用的 GET 接口，
窗口命中即误报。

**建议**：`platform_audit_archive_move` 在租户守卫之后、**任何读写之前**
加与 append 同一把锁：`PERFORM pg_advisory_xact_lock(hashtext('audit:' || p_tenant_id));`
并补真库并发用例：「归档与写入并发 N 轮，每轮 `ChainVerifier::verify` 必须 `ok=true`」。

---

### F1 · MAJOR · `platform_audit_append` 缺租户守卫（四个 SECURITY DEFINER 函数里唯一一个）

**规格要求**：同文件的三个兄弟函数都有显式守卫 ——
`platform_audit_head_hash`（`audit.up.sql:546-549`）、
`platform_audit_tip_hash`（`580-583`）、
`platform_audit_archive_move`（`622-625`），文案一致：

```sql
IF p_tenant_id IS DISTINCT FROM current_setting('app.tenant_id', true) THEN
    RAISE EXCEPTION '… does not match the request context' USING ERRCODE = 'insufficient_privilege';
END IF;
```

`platform_audit_append`（`416-526`）**没有这一段**。而 `SECURITY DEFINER`
会绕过 RLS —— 注释本身在 `570` 行写明了这个前提：「`SECURITY DEFINER` 绕过 RLS，
作用域必须自己限定住（D174）」。**同一份文件、同一个作者，三个函数做了、一个没做。**

**实测证据（真库，事务内 ROLLBACK）**：
```sql
SET LOCAL ROLE base_platform_app;
SELECT set_config('app.tenant_id','tenant-VICTIM',true);   -- 上下文 = 受害者
SELECT platform_audit_append('tenant-ATTACKER', 'actor-evil', 'audit.forge', …);
→ 63809535-ef13-4567-823d-14158315e039                  -- 写入成功
→ APPEND_SUCCEEDED_CROSS_TENANT
```
对照：`base_platform_app` 直接 `UPDATE platform_audit_log` 被拒
（`ERROR: permission denied for table platform_audit_log`），append-only 本身有效。

**为什么定 MAJOR 而不是 BLOCKER**：**当前 HTTP 路径不可利用** ——
`sink.rs:115` 把 `{tenant}` 占位符填在 `p_tenant_id` 位置，
`TenantQuery::render` 用上下文租户替换，所以应用代码永远传不出「别人的租户」。
缺口在**授权层**：任何持有 `base_platform_app` 角色、能直接连库的执行体
（迁移脚本、运维脚本、未来的批处理 worker、注入/凭据泄漏）都能给对方租户的链**伪造记录**，
且写出的记录哈希自洽 → `verify` 会认为是**合法记录**。
这是「最小权限 + 纵深防御」的真实破口，不是风格问题。

**建议**：补上与三个兄弟函数同款的租户守卫；并加真库用例
「上下文租户 ≠ 参数租户 → SQLSTATE 42501」。

---

### F2 · MAJOR · `claim_batch` 的 `LIMIT` 在重扫计划下失效（`batch` 不是上限）

**规格要求**：`claim_batch(ctx, batch, delivering_timeout_seconds)`
一次最多认领 `batch` 条 —— `worker.rs:91` 传入 `batch`，注释
（`store.rs:437`）写「`LIMIT batch`」，`xtask notify worker` 用它控制单轮工作量。

**实测代码** `crates/platform-notify/src/store.rs:459-475`：
```sql
UPDATE {table} SET status = ?, attempt_count = attempt_count + 1, updated_at = now()
 WHERE id IN (
   SELECT id FROM {table}
    WHERE tenant_id = {tenant}
      AND ((status = ? AND next_attempt_at <= now())
        OR (status = ? AND updated_at <= now() - make_interval(secs => ?)))
    ORDER BY next_attempt_at
    LIMIT ?
    FOR UPDATE SKIP LOCKED
 )
RETURNING …
```

`LIMIT` 挂在 `IN (子查询)` 上。当规划器把 `IN (…)` 降级成 **semi-join 且内层无缓冲
节点**（无 `Materialize` / `Hash` / `Unique`）时，内层子查询**按外层每一行重跑一次**
（`EXPLAIN ANALYZE` 实测 `loops=N`），每轮都取「当前前 N 条」，`id IN (…)` 的并集
**远超 N**。

**实测证据（真库，`base_platform_app`，事务内 ROLLBACK）**

| 场景 | 队列行数 | `LIMIT` | 实际认领 |
|---|---|---|---|
| 统计新鲜（ANALYZE 后） | 3000 | 10 | 10 ✅ |
| **突发插入、未 analyze** | **500** | **10** | **500（全部）** ❌ |
| 统计陈旧 | 3000 | 10 | 60 ❌ |
| 8 行小表（另一计划） | 8 | 2 | 8 ❌ |

判定性计划片段（`EXPLAIN ANALYZE`）：
```
->  Nested Loop Semi Join (rows=60 loops=1)
      ->  Index Scan … (rows=3000 loops=1)
      ->  Subquery Scan on "ANY_subquery" (rows=0 loops=3000)   ← 内层按外层行数重扫
            ->  Limit (rows=0 loops=3000)
```
对照：计划里出现 `Materialize`/`HashAggregate`/`Unique` 时 `loops=1`、`LIMIT` 正确
→ **是否超额取决于运行时计划选择，不可控**。
最贴近生产的触发条件是「**突发队列**」（新行刚插入、autovacuum 还没 analyze），
实测 500 行即可 100% 复现。

**为什么不可接受**：`batch` 是运维用来控制单轮工作量的旋钮，实测**根本不是上限**。
超额后：认领行一次性进内存、逐条 `deliver_one` 串行发网络请求；且与
`worker.rs:24-31` 宣称的「短事务认领 → 事务外投递」隔离意图相悖。

**已确认不是重复投递**：超额认领的行**互不重复**（实测 `rows=8 distinct=8`、
`max(attempt_count)=1`），`SKIP LOCKED` 保证多 worker 不撞同一行 →
是**配置失效 + 资源失控**，不是重复发送。

**建议**：改成 `id = ANY(ARRAY(SELECT … LIMIT n FOR UPDATE SKIP LOCKED))`
（实测 `loops=1`、`LIMIT` 严格生效），或
`WITH picked AS (SELECT id … LIMIT n FOR UPDATE SKIP LOCKED) UPDATE … FROM picked`。
并补「认领数 == min(batch, 就绪条数)」的真库用例 —— 当前**没有任何测试断言批量上限**。

---

### F3 · MAJOR · `claim_jobs` 同源缺陷，实测可吞下整条队列

**实测代码** `crates/platform-storage/src/store.rs:337-356`：与 F2 同构
（`id IN (SELECT … ORDER BY created_at LIMIT ? FOR UPDATE SKIP LOCKED)`），
谓词更简单 → 更容易选到无缓冲重扫计划。

**实测证据**

| 队列行数 | `LIMIT` | 实际认领 |
|---|---|---|
| 2000（陈旧统计） | 10 | **2000（全部）** |
| 3000（陈旧统计） | 10 | **3000（全部）** |
| 3000（ANALYZE 后） | 10 | 10 ✅ |
| **500（突发、未 analyze）** | **10** | **500（全部）** |
| 3000 + `= ANY(ARRAY(…))` 写法 | 10 | 10 ✅（修复验证） |

**影响**（`crates/platform-storage/src/worker.rs:82-93`）：认领后**串行逐条**
`run_import`/`run_export`。整队认领 = 单次 `tick()` 串行跑完全部导入导出任务；
一批数千行 CSV 可能跑几分钟到几十分钟，期间全部被置为 `running`。
进程崩溃 → **整批**停留 `running`，要等超时回收才能重跑。

**建议**：同 F2（`= ANY(ARRAY(…))`），并补批量上限回归用例。

---

### F5 · MAJOR · `ChainVerifier::verify` 无分页全量载入，且无并发上限

**实测代码** `crates/platform-audit/src/verify.rs:233-242`：
```rust
// 全部记录，按创建顺序（`created_at ASC, id ASC`）。
let rows: Vec<ChainRow> = tx.fetch_all(&TenantQuery::new(
    "SELECT id, tenant_id, …, before, after, ip, user_agent, created_at, prev_hash, hash \
     FROM platform_audit_log WHERE tenant_id = {tenant} ORDER BY created_at ASC, id ASC"
)).await …;
```
`grep -c LIMIT verify.rs` → **0**。整条链一次性进 `Vec<ChainRow>`，
每行含 `before`/`after` 的 `String` 全量副本。

**实测规模**：造 5 万条（`before`/`after` 各 200 字节）→
**29 MB 载荷**单次返回；Rust 侧再叠加每行结构体/String 开销。
表总大小实测 54 MB。

**可达性**：`GET /api/audit/verify`（`platform-server/src/audit.rs:271-273`）
是用户可重复调用的接口，`grep cache|semaphore|OnceLock` 在 `verify.rs` 为 0 命中
→ 无缓存、无并发上限。多个租户各自 10 万条时，单次校验即可吃掉数百 MB。
审计在线表默认保留 90 天（`archive.rs:39`），单租户累积到 10 万条量级是正常的。

**建议**：分批（keyset）流式校验 —— `(created_at, id) > cursor LIMIT 1000` 循环，
内存恒定；或至少加并发闸门 + 结果缓存。

---

### F6 · MINOR · `platform_notify_delivery` 每 enqueue 写一行且无任何清理路径

**实测代码**：`service.rs:225` 每次 `enqueue` 调 `store.insert_delivery` → 一行。
终态（`delivered` / `dead_letter`）**永不删除**：
`grep -E 'DELETE FROM.*delivery|prune|retention'` 在 `platform-notify/src/*.rs` → **0 命中**。
`notify_config_seed` 只登记了 5 个投递参数，**没有清理/保留配置键**。

**对照**：同项目 `platform-observability/src/rate_limit/mod.rs:8` 自己写着
「代价是**每请求一行**（需要定期清理，见 `prune`）」并实现了 `prune`（`:273`）
—— 这正是本项目「每请求写一行必须配清理」的既有教训；通知投递表缺这一半。

**定 MINOR 的理由**：`delivery` 的行数由「业务入队量」驱动，不是「每请求」；
且它承载投递状态与重试，本身需要短期保留。但**无上限**意味着跑一年后
该表只增不减，列表查询（`list_idx`）与备份窗口会持续劣化。

**建议**：登记 `notify.retention.deliveryDays` 并实现 `prune`（删 `delivered` /
`dead_letter` 超过 N 天的行），与限流表同款。

---

### F7 · MAJOR（测试盲区）· 最关键的 `SET LOCAL` 判据无测试覆盖，变异全绿

**规格要求**：`tenant.rs:15-38` 用整整一节说明为什么必须是 `SET LOCAL`
（`set_config(..., true)`）：普通 `SET` 会在连接归还连接池后**残留租户** →
下一个请求借到这条连接就**串租户**，且「极难复现（取决于连接池调度顺序）」。

**变异实测**（唯一一次源码变异，已还原，`git status` 干净）：
把 `tenant.rs:187` 的
`SELECT set_config('app.tenant_id', $1, true)` → `..., false`：

```
cargo test -p platform-db --all-targets -- --test-threads=1
→ 38 passed; 0 failed
```
**全绿。** 而该变异是**真缺陷**（实测，非推断）：
```sql
BEGIN; SET LOCAL ROLE base_app;
SELECT set_config('app.tenant_id','tenant-a',false);
COMMIT;
SELECT current_setting('app.tenant_id',true);
→ tenant-a            ← 提交后仍残留
SELECT count(*) FROM platform_notify_delivery;
→ 1                   ← 未设上下文的连接看到了 tenant-a 的行
```
把最关键、后果最严重（串租户）的不变量变成「无测试保护」——
任何一次重构都可能静默引入跨租户数据泄漏。

**建议**：补一条真库用例 ——「完成一次租户作用域操作后，从**同一连接池**再取连接、
不设 `app.tenant_id`，可见行数必须为 0」，即直接断言「事务结束后上下文已清除」。

---

### F8 · MINOR · `total` 与 `items` 可能不一致（READ COMMITTED 下两次读）

**实测代码** `crates/platform-audit/src/query.rs:426-444`：`count(*)` 与分页 SELECT
在**同一个事务**但**默认 READ COMMITTED** 下先���执行：
`count` 在 `:437`，页面在 `:468`。两个语句各取一个快照 →
两次读之间有并发插入时，`total` 可能比实际可翻页总数大 1。

**为什么只算 MINOR**：无键集分页的漏行/重复（`next_cursor` 用
`(created_at, id)` 严格小于，`ORDER BY` 与游标同序 → 实测
`two_consecutive_pages_share_no_ids`、`has_more_pages_iff_the_total_exceeds_the_limit`
均通过），问题只在「总数与页内容轻微不同步」。未实测出不一致样本，属结构推断。

---

### F9 · NIT · 存储任务表与错误表无清理、无 FK CASCADE

`platform_storage_job` / `platform_storage_job_error`：
`grep 'DELETE FROM' crates/platform-storage/src/store.rs` 仅 1 处（删对象），
任务表无删除路径；迁移里 `job_error` 的 `job_id` **无 `REFERENCES … ON DELETE CASCADE`**
（`20261122000100_storage_schema.up.sql:91-105`）→ 即便将来删任务，错误行也会孤儿化。
（错误行有 `ON CONFLICT (job_id, row_number, "column") DO UPDATE` 去重，故不随重跑膨胀。）

## 已查确认无问题的项

✅ **已查：`TenantQuery` 的 `{tenant}` 占位符强制**（`query.rs:251-287`）——
`render` 要求恰好一个 `{tenant}`（否则 `InvalidQuery`），`?` 计数必须等于绑定数，
租户恒为 `$1`；`SELECT * FROM t WHERE a = ? AND b = {tenant} AND c = ?` 实测渲染为
`a = $2 AND b = $1 AND c = $3`（单测覆盖）。「忘写租户条件」确实**写不出来**。

✅ **已查：列名注入防护**（`query.rs:307-321` `safe_column`）——
`^[a-z][a-z0-9_]*$` 白名单，非法直接拒绝，不拼进 SQL。`with_scope` 的
`OrgNodes`/`Owner` 全部走 `safe_column`。

✅ **已查：业务参数一律参数化**——四个 crate 的生产代码**没有**把用户输入拼进 SQL
字符串；`format!` 拼接的只有表名（`TenantDb::table`）与列名（`safe_column` 后）。
所有业务值走 `BindValue` → sqlx `bind`。

✅ **已查：业务 crate 拿不到裸连接**——`grep PgPool` 在 storage/audit/notify 的 `src`
只有测试代码命中；`TenantDb.pool` 是 `pub(crate)`；`tenant.rs` 公开签名里无裸连接。

✅ **已查：`SET LOCAL` 语义正确**（`tenant.rs:187` `set_config(..., true)`）——
真库实测 `tenant-a` 看到 1 行、`tenant-b` 看到 0 行；未设上下文 → 0 行（fail closed，不报错）；
跨租户 INSERT 被 `WITH CHECK` 拒（`new row violates row-level security policy`）。
（注意：**其测试覆盖缺失**见 F7。）

✅ **已查：`platform_audit_log` 的 append-only 真实生效**——
以 `base_platform_app` 执行 `UPDATE platform_audit_log` → `ERROR: permission denied`；
`has_table_privilege('base_platform_app','platform_audit_archive','DELETE') = f`。

✅ **已查：审计链哈希接续正确**（顺序场景）——5 条记录 `prev_hash` 与上一条 `hash`
逐条相接（`00000000 → 015944b1 → 605478c8 → 219d28c7 → c1e818a0 → d187801a`）。

✅ **已查：归档在**无并发**时保持链完整**——归档 3 条后
`head_hash = 219d28c7` = 被搬走段的尾，在线首条 `prev_hash = 219d28c7` → `CHAIN_OK_AFTER_ARCHIVE`。
（并发场景见 F4。）

✅ **已查：`prev_hash` 由数据库函数计算，应用不自己算**（`sink.rs:7-8` 与实现一致）——
`platform_audit_append` 用 `pg_advisory_xact_lock` + 拿锁后**用新语句重读**（`audit.up.sql:457-464`），
D161 的两条要求都落实了。

✅ **已查：审计哈希口径与校验器一致**——长度前缀用 `octet_length`（字节数，非字符数）、
`prev_hash` 以 64 字节文本参与、空值为零字节空串；Rust 侧 `compute_hash`（`verify.rs:116-140`）
与 SQL `platform_audit_canonical_hash`（`audit.up.sql:203-248`）逐字段对齐。

✅ **已查：敏感键 DB 侧硬拒绝**——真库侧与 Rust 侧键集合一致性有测试
（`the_db_and_rust_sensitive_keys_are_identical`），错误信息只含键名不含值。

✅ **已查：通知重试无重复发送**——`SKIP LOCKED` 互斥实测（两会话并发，A 取走后 B 取 0 行）；
`four_concurrent_ticks_deliver_each_message_exactly_once` 实测 provider 调用恰好 100 次；
超额认领时行互不重复（`distinct=8, max(attempt_count)=1`）。
`record_outcome` 用 `WHERE tenant_id = {tenant} AND id = ?::uuid`，重投死信用
`AND status = 'dead_letter'` 的原子判据（无 check-then-act）。

✅ **已查：预签名不可篡改**（`presign.rs:108-124`）——**先验签名再解析 `expires_at`**；
`splitn(3, '.')` 正确处理 object_key 内的点号；18 个单测覆盖改时间/改 key/改签名/
换密钥/过期边界。

✅ **已查：对象键路径穿越防护**（`object_store/mod.rs:108-120`）——拒绝空串、绝对路径、
`\0`、反斜杠、控制字符、空段、`.`/`..` 段；本地存储每个入口都过 `is_safe_object_key`
并额外做 `starts_with(root)` 复核。

✅ **已查：`delete_object` 走 `{tenant}` + 参数绑定**（`store.rs:221-229`）。

✅ **已查：配额扣减原子**（`quota.rs:99-107`）——单条
`INSERT … ON CONFLICT DO UPDATE SET used = {table}.used + EXCLUDED.used RETURNING used`，
无 check-then-act。且 `add_quota_usage` 在生产 crate 中**无调用方**（仅测试），
故「扣了配额但没写记录」的半成品风险当前不存在。

✅ **已查：`insert_job_errors` 不会随重跑膨胀**——`ON CONFLICT (job_id, row_number, "column") DO UPDATE`。

✅ **已查：四个 crate 的测试基线全绿**——
`platform-db` 38 + 5（1 ignored 需 `--include-ignored`）、`platform-storage` 19、
`platform-audit` 2 + 8、`platform-notify` 13 + 7，串行执行、0 失败。

## 未验证项

- **F4 的真实 HTTP 并发压力**：我用「真实 `ChainVerifier` + 真库两个连接按真实交错执行」
  证明了后果（`ok=false`），但没有起完整 `platform-server` 并发打
  `POST /api/audit/archive` 与业务写入做端到端压测。**未验证**：生产 HTTP 栈下的触发频率。
- **F2/F3 在生产规模下的表现**：实测触发条件是「突发插入未 analyze」（500 行即 100% 复现）
  与「统计陈旧」。生产 autovacuum（`scale_factor=0.1, threshold=50`）会定期纠正统计，
  但突发窗口真实存在。**未验证**：真实业务写入速率下该窗口的长度与命中率。
- **F8（`total` 与 `items` 不一致）**：仅从代码结构推断（READ COMMITTED 两条语句两个快照），
  **未构造出实际不一致的样本**。
- **`platform-audit/src/query.rs` 的分页在并发写入下的漏行/重行**：只核对了
  keyset 谓词与既有测试（`two_consecutive_pages_share_no_ids` 等通过），
  **未**在「翻页期间并发插入」条件下实测。
- **S3 真实端点**：`s3.rs` 只做了形状/参数校验的单测（无真实 S3），
  签名算法的字节级正确性**未**对真实服务验证。
- **`platform-notify` 的 SMTP/Webhook 通道**：未对外部服务做真实投递验证（超出切片范围）。

## 附：环境与纪律

- 真库：`base-postgres`（PG 17.12），库 `base_test`，应用角色 `base_app`
  （`rolsuper=f`、`rolbypassrls=f`）。
- 所有探针用 `qa-*` 租户前缀，**每条命令末尾统一清理**；终检各表 `qa-*`/VICTIM/ATTACKER
  残留均为 **0**。测试库仅保留探测前已存在的 1 行（`platform_notify_delivery` /
  `tenant-a`，创建于 00:05，早于本次会话）。
- 唯一一次源码变异（`tenant.rs:187` `true`→`false`）**已还原**，
  `git status --short` **为空**；临时测试文件 `zz_qa_tmp_verify.rs` / `zz_qa_mem.rs`
  **已删除**。
- cargo 全程**串行**执行（未并行抢 target 锁）。
