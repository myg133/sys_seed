# 修复轮复核报告 —— REQ-010（轻量）

- **需求**: REQ-010「配置、品牌与 Feature Flag」
- **被验提交**: `f0f0ed0`（HIGH） + `a4af6ee`（MEDIUM/LOW）· 上一轮 HEAD `96e4cb6`
- **首轮报告**: [`qa-report.md`](./qa-report.md)（结论 FAIL，§12 列出 12.1 HIGH / 12.2 MEDIUM / 12.3 LOW / 12.4 LOW）
- **验证者**: QA Agent（**独立复跑；Dev 自报数字一律不采信**）
- **日期**: 2026-11-19
- **结论**: **PASS-WITH-RISK** —— 12.1（HIGH）与 12.2（MEDIUM）**确认真修好**，12.4 已同步，回归干净；
  **12.3（LOW）未真修好**：新快照断言在结构上抓不到它本该抓的泄漏（见 §3），另新增 1 条测试脆弱性（§6.1）。

> 本轮只做修复轮复核：逐条复验首轮 §12 的问题 + 回归；15 条 AC 的其余证据沿用首轮。
> 本轮变异全部逐字节还原，`sha256sum -c` 通过，`git status` 干净（仅 `feature-meta/qa-report.md`
> 是进入本轮前就存在的未提交改动，未被我触碰）。

---

## 0. 逐条结论

| # | 首轮问题 | 本轮结论 | 一句话证据 |
|---|---|---|---|
| **12.1** | HIGH：默认生产日志过滤吞掉 `platform_config` 的全部 WARN | **PASS（真修好）** | 真实服务、`BASE_LOG_LEVEL=info`、不设 `RUST_LOG`：`grep "not registered"` = **1 条**（修复前 0）；脏开关 WARN 同样 1 条 |
| **12.2** | MEDIUM：E2E 只断言 `color !== '#1677ff'`（否定式假绿） | **PASS（真修好）** | 已断言 `toBe(hexToRgb(接口 primaryColor))`；变异 `#123456` → E2E **1 failed**（期望 rgb(17,17,17)，实得 rgb(18,52,86)）；还原后 2 passed，sha256 一致 |
| **12.3** | LOW：步骤 8 行数断言对夹具合成行不敏感 | **FAIL（未真修好）** | 快照改为 `count + max(value)`，但 **`before_snapshot` 取在两次越级写之后**，且不含 `updated_at`；「同值覆盖」与「租户级新增行」两种泄漏变异均 **通过**（断言不红） |
| **12.4** | LOW：用例/文档漂移 | **PASS** | TC-010-013 → `platform-entries`；TC-010-030 → `newinvoice` + kebab 400 说明；TC-010-005 步骤 8 → 快照比对 |
| **回归** | —— | **PASS** | 集成 **3 轮** 各 540 passed / 0 failed / 0 ignored；clippy 0 warning；`bun run test` EXIT=0；E2E 2 passed |

---

## 1. 12.1（HIGH）—— 默认日志过滤器：真修好

### 1.1 形态是「全局默认级别 + 噪音库降级」（不是补清单）

`crates/platform-core/src/log.rs:45-51`：

```rust
pub fn log_filter(level: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let mut directives = vec![level.to_string()];          // ← 无 target 的全局默认
        directives.extend(NOISY_TARGETS.iter().map(|t| format!("{t}=info")));
        EnvFilter::new(directives.join(","))
    })
}
```

- 首条 directive **无 target** → 任何未被更具体 directive 命中的 crate 都按 `{level}` 输出。
  新增 crate **不需要改清单**，根因（「漏列 target 就静默丢日志」）被消除。
- `NOISY_TARGETS = ["tower_http", "hyper", "sqlx"]`（`log.rs:40`）只做**降级**，不做白名单。
- 守护测试 `log_filter_covers_a_crate_that_nobody_listed`（`crates/platform-server/src/main.rs:789`）
  用 `target: "some_future_crate::module"` 断言「未列出的 crate」也能出日志 —— 这条判据在旧实现下必然失败。

### 1.2 `platform-server` 复用同一份过滤器（无本地重复定义）

- `crates/platform-server/src/main.rs:26`：`use platform_core::log_filter;`
- `main.rs:229-233`：`init_tracing` → `.with_env_filter(log_filter(level))`
- 全仓 `grep "fn log_filter"` 只剩 `crates/platform-core/src/log.rs:45`（其余是测试函数名）。
  旧的 `LOG_TARGETS` 常量与本地 `fn log_filter` 已删除。

### 1.3 端到端：默认配置下 WARN 真的出来了（自起真实服务）

**环境**：`BASE_LOG_LEVEL=info`、**未设 `RUST_LOG`**、`DATABASE_URL=postgres://base_app:base@…`、
`SERVER_PORT=18080`、`BASE_MANIFEST_ROOT=.`、`setsid … & disown` 后台启动。

| 步骤 | 观察结果 |
|---|---|
| `GET /api/feature-flags/newinvoice`（有令牌，合法但未登记） | `HTTP=200 {"flag":"newinvoice","enabled":false,"source":"default"}` |
| 调用后日志新增行 | **1 行**，target = `platform_config::resolver`，`WARN … the feature flag is not registered; returning disabled (fail closed) flag=newinvoice` |
| `grep -c "not registered"`（默认配置） | **1**（首轮为 **0**） |
| `grep -c "WARN"`（默认配置） | **1** |
| 连调 3 次 | 新增 `not registered` **3 条**（每次请求 1 条，非进程级只报一次） |

**AC-S-057 步骤 6（读路径脏数据 → enabled=false + WARN）** 也复验：

- 管理员 SQL 注入 `platform_tenant_config (tenant-a, beta) = "dirty"` 并递增 `tenant:tenant-a` epoch；
- `GET /api/feature-flags/beta` → `200 {"enabled":false,"source":"tenant"}`；
- 默认日志新增 **1 条**：`WARN platform_config::resolver: the stored feature-flag switch is not a boolean; failing closed flag=beta actual="string"`。
- 注入行已删除（`DELETE 1` → 余 0）。

### 1.4 测试侧不再「自己放行」

`crates/platform-config/tests/config_real_db.rs:850-853`：

```rust
fn subscriber(buffer: &LogBuffer) -> impl tracing::Subscriber + Send + Sync + 'static {
    tracing_subscriber::fmt()
        .with_env_filter(platform_core::log_filter("info"))   // ← 生产同一函数
```

首轮的 `EnvFilter::new("platform_config=warn")` 已删除。`unknown_flags_warn_once…` 用例
（`:866`）现在走生产过滤器，「测试放行但生产吞掉」在结构上不再可能。

### 1.5 与 Dev 自报数字的差异

Dev 提交信息写「日志出现 **2 条**结构化 WARN」。我实测**单次请求 = 1 条**（多次请求=多次 1 条）。
不影响结论（任务书要求就是「必须是 1 条」），但记录该数字未被复现。

---

## 2. 12.2（MEDIUM）—— E2E 主色断言：真修好

### 2.1 断言已改为「等于接口值换算的 rgb」

`e2e/shell.spec.ts:26-36`：

```ts
const primaryColor = typeof branding.primaryColor === 'string' ? branding.primaryColor : '';
expect(primaryColor).toMatch(/^#[0-9a-fA-F]{6}$/);
const expected = hexToRgb(primaryColor);          // 从接口值换算
const color = await page.getByRole('heading', { name: title })
  .evaluate((element) => getComputedStyle(element).color);
expect(color).toBe(expected);                     // ← 等式断言
```

### 2.2 基线 E2E

`bunx playwright test e2e/shell.spec.ts` → **2 passed (4.9s)**，EXIT=0。

### 2.3 变异（非默认色，不是 `#1677ff`）

把 `apps/shell/src/app.css:18` 的 `color: var(--base-primary);` 改为 `color: #123456;`
（sha256 记录：`2acb0b64…87ed`），重跑：

```text
1) [chromium] › e2e/shell.spec.ts:9 › shell 从品牌接口读取标题并渲染（同源）
   Expected: "rgb(17, 17, 17)"     ← 接口 primaryColor = #111111
   Received: "rgb(18, 52, 86)"     ← 硬编码 #123456
   expect(color).toBe(expected)  ← 断言命中
1 failed, 1 passed   EXIT=1
```

**变异被抓到。** 旧的否定式断言 `color !== 'rgb(22,119,255)'` 在 `rgb(18,52,86)` 下**会通过** ——
这条变异正好复现首轮说的假绿，新断言能拦住。

### 2.4 还原

`cp` 回原文件后 `sha256sum -c` 通过（`2acb0b64…87ed`），`git status` 该文件为空；
重跑 E2E → **2 passed (6.0s)**，EXIT=0。

---

## 3. 12.3（LOW）—— 越级写快照：**未真修好**

### 3.1 已做的改动

`crates/platform-config/tests/config_real_db.rs:368-382` 新增 `conflict_strategy_snapshot`：

```sql
SELECT 'tenant', tenant_id, NULL, count(*)::bigint, max(value::text)
  FROM platform_tenant_config WHERE key='conflictStrategy' GROUP BY tenant_id
UNION ALL
SELECT 'space', tenant_id, space_id, count(*)::bigint, max(value::text)
  FROM platform_space_config WHERE key='conflictStrategy' GROUP BY tenant_id, space_id
```

调用点 `:299`（before）与 `:355-359`（after）做 `assert_eq!`。
字面上确实从「行数=0」升级成了快照比对，**但有两个结构性问题使它对目标场景失效。**

### 3.2 问题 A：`before_snapshot` 取在两次越级写**之后**

- 步骤 1（租户级写 `conflictStrategy` → 期望 400）在 `:268-280`；
- 步骤 2（空间级写同一键 → 期望 400）在 `:282-291`；
- **`before_snapshot` 在 `:299`** —— 即两次越级写都跑完之后才拍「写前」快照。

于是：**若步骤 1/2 的越级写被放行并落库，污染已经包含在「before」里**，
before 与 after 自然相等 → 断言不红。

**变异 B2（精确复现）**：在 `service.rs` 的作用域校验分支里，仅对
`scope == Tenant && key == "conflictStrategy"` 先执行写入、再返回原错误
（模拟「报了 400 但行已写进去」），跑该用例：

```text
test scope_escalation_is_rejected_and_leaves_no_rows ... ok
test result: ok. 1 passed
```

库侧确认泄漏**真的发生了**：

```text
tenant-a | conflictStrategy | "remote_wins" | 2026-11-18 18:30:23.488582+00
```

（该行是变异写入的；已 `DELETE 1` 清理，复原后余 0）

### 3.3 问题 B：快照不含 `updated_at`（注释与实现不符）

- `:353` 注释写「对 `count` + `value` + **`updated_at`** 做写前/写后对比，全部必须不变」；
- 实际 SQL（`:371-381`）只选出 `count(*)` 与 `max(value::text)`，**没有 `updated_at`**。

**变异 B（同值覆盖）**：仅对 `scope == Space && key == "conflictStrategy"` 先写入再返回错误
（写的是夹具里已有的同值 `remote_wins`）。用例：

```text
test scope_escalation_is_rejected_and_leaves_no_rows ... ok    ← 快照断言没红
```

事务内直接验证这种泄漏在库侧的可见性：

```text
count+value 口径:  count_before=1  count_after=1  max_before="remote_wins"  max_after="remote_wins"  → 相等
updated_at 口径:   before=18:16:13.934683+00   after=18:21:36.305474+00     → 不相等
```

即：**「同一行被覆盖成同一个值」只改 `updated_at`**，而快照不取该列 → 抓不到。
这正是首轮 §12.3 描述的那个夹具行（`(tenant-a, space-sales-1, conflictStrategy='remote_wins')`）
与用例写入值相同所导致的盲区，**未消解**。

### 3.4 任务书要求的变异（整体删掉校验）

把 `service.rs:313-324` 的「第 5 步：作用域」整段删除后跑该用例：

```text
panicked at crates/platform-config/tests/config_real_db.rs:276:6:
租户级越级写应被拒: 1
test result: FAILED. 0 passed
```

测试确实变红 —— **但红在步骤 1 的 `expect_err`（`:276`），不是快照断言**。
也就是说「必须快照断言变红」这一验收动作**没有被满足**；快照断言在该变异下从未被执行到。
（首轮 §12.3 已指出这一点：主断言严格，松的是步骤 8 的库侧前后对比；本轮修复没有改变这个结构。）

### 3.5 还原

`service.rs` 逐字节还原，`sha256sum -c` 通过（`78d330d7…0842`），`git status` 该文件为空；
复原后重跑该用例 → **1 passed**（`--test-threads=1`）。
说明：还原时 `cp -a` 保留了旧 mtime，cargo 一度复用变异二进制导致「还原后仍红」；
`touch` 后重编即转绿 —— 这是我复验流程的坑，不是交付物问题，`git diff` 始终为空。

---

## 4. 12.4（LOW）—— 用例漂移：已按 D77/D78/D84 同步

| 文件 | 现状 |
|---|---|
| `test-cases/B-branding.md`（TC-010-013 步骤 4） | 已改为 `GET /api/config/platform-entries?keys=…`，并明写「通用路由会整请求 **403**（rev 2 已过时）」 |
| `test-cases/C-feature-flags.md`（TC-010-030 步骤 4） | 已改为读 `newinvoice`（全小写）→ 200 + false；并注明 kebab `new-invoice` 按 D77 是**非法名 → 400 `FLAG_NAME_INVALID`** |
| `test-cases/A-config-scope.md`（TC-010-005 步骤 8） | 已改为「租户级与空间级两张表取**写前/写后快照**」；预期结果写「既没多行、也没改值」 |

用例文案口径与实现一致（实现侧行为另见 §5 的实机验证）。

**残留（不计本轮验收项，仅登记）**：
`test-cases/C-feature-flags.md:18` 仍写「⚠️ 授权边界未定义（§8.9 N14）」。
D70 已裁定「通用写路由收到 flag 命名空间 → 400」，实现也确实有 `FlagEndpointRequired`
（`crates/platform-config/src/service.rs:299-305`）→ 该注记**已过时**，建议 BA 删除。

---

## 5. BA 裁定项实机核对（不判缺陷）

| 裁定 | 实测 |
|---|---|
| **D77**：`new-invoice`（kebab）→ 400 `FLAG_NAME_INVALID` | `HTTP=400`，`code=FLAG_NAME_INVALID`，detail 给出 `^[a-z][a-zA-Z0-9]{0,63}$` ✅ |
| **D77**：`newinvoice`（全小写）→ 200 + false，不归一化 | `HTTP=200 {"flag":"newinvoice","enabled":false,"source":"default"}` ✅ |
| **D78**：平台级键走通用读路由 → 403 | `GET /api/config/entries?keys=conflictStrategy` → `HTTP=403`，detail 明确「use GET /api/config/platform-entries」✅ |
| **D78**：独立路由 | `GET /api/config/platform-entries` → 403（当前测试用户无 `config:platform:read`，属权限正确；「独立路由才 200」需带权用户，首轮已验） |
| **D79/D80/D81** | 本轮未产生相反证据；沿用首轮结论 |

---

## 6. 便携性检查（`platform-core/src/log.rs` 是新增公共模块）

### 6.1 测试覆盖在哪（任务书问项 1）

| 层 | 位置 | 测什么 |
|---|---|---|
| **形态** | `crates/platform-core/src/log.rs:60-81`（2 条） | 有全局默认 directive；噪音库被压到 info |
| **行为** | `crates/platform-server/src/main.rs:770/789/801`（3 条） | `platform_config` WARN 能出；未列出的 crate 能出；`BASE_LOG_LEVEL` 级别仍生效 |
| **集成** | `crates/platform-config/tests/config_real_db.rs:850` + `:866` | 用**生产**过滤器捕获「恰好 1 条 WARN」 |

结论：核心行为测试留在 `platform-server`，合理（`capture_logs` 辅助在那边）；
集成层也用同一函数，覆盖链是通的。

**新增脆弱性（LOW）**：`log_filter` 按设计优先 `RUST_LOG`，因此**只要环境里设了 `RUST_LOG`，上述 5 条 Rust 测试会红**。实测：

```text
$ RUST_LOG='base=info' cargo test -p platform-core log::
  2 failed（filter_has_a_global_default_directive / noisy_targets_are_pinned_to_info）
$ RUST_LOG='base=info' cargo test -p platform-server --bin base log_filter
  3 failed
$ RUST_LOG='base=info' cargo test -p platform-config --test config_real_db unknown_flags_warn_once
  FAILED（WARN 计数 left: 0, right: 1）
```

CI 未设 `RUST_LOG`（`.github` 无该变量），所以现在不红；但开发者 shell 里若导出 `RUST_LOG`，
会出现与产品行为无关的 3 处假红。建议测试内显式 `EnvFilter` 构造或先 `remove_var("RUST_LOG")`。
**这是测试健壮性问题，不是产品缺陷，不判失败。**

### 6.2 `NOISY_TARGETS` 的三个库是否真的会产生噪音（任务书问项 2）

| target | 是否本仓依赖 | 是否会 emit tracing | 判断 |
|---|---|---|---|
| `tower_http` | **不是**（`Cargo.lock` 中 0 条，全仓除 `log.rs` 外无任何引用） | 否 | **死配置** |
| `hyper` | 是（`hyper 1.12.0`） | **否** —— 其 `tracing` 是可选 feature，锁文件里 hyper 的依赖不含 `tracing` | **死配置** |
| `sqlx` | 是（`sqlx-core` 依赖 `tracing`） | **是**，target 为 `sqlx::query`，默认 `statements_level=Debug` | **有效**：`sqlx=info` 确实压掉了逐条 SQL 日志 |

- 影响判断：`tower_http` / `hyper` 两条 directive 目前是**空转**，不会「压错导致该看的日志看不到」
  （它们本来就不发事件）。真正的 SQL 噪音由 `sqlx=info` 压住，方向正确。
- 但 `sqlx=info` 的副作用：`BASE_LOG_LEVEL=debug` 时**也看不到 SQL 语句**。这是刻意的降噪取舍，
  建议在 RUNBOOK 里说明「要看 SQL 需 `RUST_LOG=sqlx=debug`」。
- 另：`hyper-util`（`crates/platform-org` 出站 HTTP 客户端）**有 tracing 依赖但不在 `NOISY_TARGETS`**。
  出站请求路径在 debug 级别可能有 pool 级日志；本轮未走到该路径，**存疑**，不判缺陷。

### 6.3 `debug` 下会不会噪音爆炸（任务书问项 3）

实测（`BASE_LOG_LEVEL=debug`、另起 18081）：10 个请求（5 次 flag + 5 次 branding）**新增 5 行**，
全部来自 `platform_config::resolver`；1 次登录（多次 DB 查询）**新增 1 行**（`platform_iam::http`）。

结论：**当前依赖树下没有爆炸** —— 因为 hyper 未启用 tracing、tower_http 不存在、sqlx 被压到 info。
**风险（不判失败，仅说清）**：
1. 若将来给 `hyper` 打开 `tracing` feature，或真的引入 `tower-http`，`hyper=info` / `tower_http=info`
   这两条**已经在表里**，反而会正确压住，不会爆炸；
2. 真正的风险是**不在表里**的库：`axum`、`tower` 自身都有 tracing 依赖，`hyper-util`（`platform-org`
   的出站客户端）也有。`debug` 级下它们会走全局默认级别。目前实测量级可控（5 行 / 10 请求），
   但出站 HTTP 路径未在本轮触发 → 见 §8「存疑」。

---

## 7. 回归

| 项 | 命令 | 结果 |
|---|---|---|
| 集成（第 1 轮） | `scripts/test-integration.sh` | **540 passed / 0 failed / 0 ignored**，EXIT=0 |
| 集成（第 2 轮） | 同上 | **540 / 0 / 0**，EXIT=0 |
| 集成（第 3 轮，变异还原后） | 同上 | **540 / 0 / 0**，EXIT=0 |
| 两轮逐 suite 对照 | `diff` | 仅耗时不同，pass/fail 数字完全一致 |
| Rust + 前端 | `bun run test` | EXIT=0；Rust **476 passed / 70 ignored**；前端 **28 passed**（shell 9 / ui 3 / hooks 3 / auth 3 / schema 3 / i18n 3 / theme 2 / sdk 2） |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | **0 warning**，EXIT=0 |
| E2E | `bunx playwright test e2e/shell.spec.ts` | 基线 **2 passed**；变异 **1 failed**；还原后 **2 passed** |

基线对比：首轮集成 539 → 本轮 540（+1 为 `log_filter_covers_a_crate_that_nobody_listed` 守护测试），
Rust 单测 475 → 476，符合「只增测试不改行为」的预期。**未发现回归。**

> 说明：`bun run test` 第一次跑报 25 条 `缺少 DATABASE_URL_TEST` —— 是我调用时漏导出环境变量，
> 真实库用例按设计 fail-fast（不是静默跳过）。补上 `DATABASE_URL_TEST` 后全绿；这不是产品回归。

---

## 8. 未覆盖 / 存疑

| 项 | 说明 |
|---|---|
| `hyper-util` 在 debug 下的出站噪音 | 本轮未触发 LDAP/通知出站路径；`NOISY_TARGETS` 未包含它 → 存疑 |
| 「独立路由 200」的带权实测 | 本轮用无 `config:platform:read` 的用户只验到 403；带权 200 沿用首轮证据 |
| 生产日志聚合（Loki/ELK） | 只验 stdout 的 tracing 过滤；12.1 的修复未在真实日志管道复验 |
| 12.3 的「步骤 8 真能抓泄漏」 | **已实测证明不能**（§3.2/§3.3），见下方修复建议 |

---

## 9. 给 BA 的一句话

**12.1（HIGH）与 12.2（MEDIUM）确认真修好，回归干净（集成 3×540、clippy 0、前端 28、E2E 2），
但 12.3（LOW）没有修好** —— 新快照的 `before` 取在两次越级写之后、且不含 `updated_at`，
两种真实泄漏变异（同值覆盖 / 租户级新增行）都能穿过断言。建议：把 `before_snapshot` 移到步骤 1 之前，
并在快照里加入 `max(updated_at)`（或改用一个与夹具不同的值，如 `local_wins`，断言库里仍是 `remote_wins`）。
这是测试强度问题，不阻塞产品行为（越级写确实被 400 拒绝），因此整体判 **PASS-WITH-RISK** 而非 FAIL。
