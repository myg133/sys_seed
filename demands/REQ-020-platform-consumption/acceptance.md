# 验收标准：底座可用性验证

- **需求编号**: REQ-020
- **父需求**: [REQ-001](../REQ-001-platform-foundation/acceptance.md)
- **状态**: **rev 2**（rev 1 经独立挑刺：6 BLOCKER / 10 MAJOR / 10 MINOR，本轮逐条处置）
- **更新时间**: 2026-11-23

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。

## ⚠️ rev 2 的定位变化（必读）

rev 1 把「复用平台能力」当成**既成事实**去写判据。独立挑刺实测后发现：
**有四处「照文档应该能，打开代码不能」**（B1 装配点 / B4 通知落库 / M7 前端动作 /
B6 capabilities 生成物）。

→ rev 2 的定位改为：**本需求如实产出「底座缺口清单」**，而不是假装底座已完备。
判据从「零改动」改为「**分类登记 + 逐项归因 + 与全仓 diff 相等**」。

**这不是放水，而是把「不可达成的判据」换成「可判定的判据」**：
rev 1 的「平台改动 = 0 个文件」在任何实现下都恒红（B1），等于没有判据。

---

## 承接的父 AC 与拆解

| 父 AC | 原文要点 | 拆成 |
|---|---|---|
| **AC-029** | 新业务系统只在业务目录写代码；不修改平台核心任何一行；用户/权限/租户/通知/审计/工作流全部复用 | AC-S-186 / 187 / 188 / 192 / 193 |
| **AC-030** | 底座升级不破坏业务；破坏性变更通过版本化 API + 弃用头提前通知 | AC-S-190 |
| 父 AC 的度量要求 | 度量指标取数 | AC-S-191 |

⚠️ **父 AC-029 的「工作流复用」本轮不承担**：REQ-017 已取消，
`WorkflowHandler::on_transition` 零调用点（实测见 demand.md）。用户 2026-11-23 裁定
审批流由业务层自实现，**不声称**复用工作流引擎（D347）。

---

## 两个基线（rev 2 新增，解决 B2）

rev 1 让 AC-S-186 与 AC-S-190 共用「实现前」的 `BASE`，
导致 AC-S-190 的「`modules/leave/` diff 为空」**必然红**（模块本身是新增的）。

→ rev 2 拆成两个，且**要求提交顺序**：

| 基线 | 含义 | 用于 |
|---|---|---|
| **`BASE_IMPL`** | 实现开始前的 `develop` commit | AC-S-186（平台改动分类）、AC-S-187/188/193 |
| **`BASE_UPGRADE`** | **`modules/leave` 已提交、升级尚未开始**的那个 commit | AC-S-190（升级零改动） |

⚠️ **提交顺序硬要求**：消费方先提交（形成 `BASE_UPGRADE`），升级改动**后**提交。
交付摘要里必须写死**两个 sha**，否则 AC-S-190 无法判定。

---

## 本需求自有 AC

### AC-S-186: 平台改动**分类登记**，A 类必须为 0（承接 AC-029）

- **前置条件**:
  - 消费方 `modules/leave/` 已建（D348）；
  - `BASE_IMPL` 已确定并写进交付摘要。
- **测试步骤**:
  ```bash
  # ① 全仓改动面（rev 2 修正：不再只量 crates/**，见 B6）
  git diff --name-only BASE_IMPL | grep -v '^modules/leave/' | sort > /tmp/actual.txt
  # ② 从 design-summary.md §3 提取清单里的文件路径
  #    （人工或脚本，逐项列成 /tmp/declared.txt）
  # ③ 比对
  diff /tmp/actual.txt /tmp/declared.txt
  ```
- **预期结果**:
  - 步骤 ③ **无差异**（实际改动集合 == 登记清单集合，**逐项相等**）；
  - 清单里**A 类条目数为 0**（A 类 = 可避免的改动，定义见 D353）；
  - 每个 B / C / D 类条目都有**归因**（「删掉它，哪条 AC 就判不了」）。
- **⚠️ 为什么改成「全仓 diff 减 modules/leave/」（B6 的修正）**
  rev 1 只量 `crates/**`，漏掉了：
  - 根 `Cargo.toml`（members 是显式列表，不加就没有 leave crate）；
  - `crates/platform-server/Cargo.toml`（`sandbox.workspace = true` 证明要加依赖）；
  - `capabilities/permissions.json`（`capabilities --check` **会强制**重新生成 —— 见 B6）；
  - `apps/shell/**`（前端页面装配 —— 见 M7）；
  - `packages/**`（SDK 兼容层）。
  → 漏掉这些等于给了**无上限逃生通道**：任意多的前端逻辑可以塞进 `packages/**` 而不触发判据。
- **⚠️ 改动分类（D353）**
  | 类 | 含义 | 允许 |
  |---|---|---|
  | **A 可避免** | 纯粹为了让消费方跑起来而改，且有平台既有机制可用 | ❌ **必须为 0** |
  | **B 升级演示** | AC-S-190 必需的 v2 + 弃用登记 | ✅ 登记 |
  | **C 装配必需** | 让新业务模块能被服务加载所必需（B1/B4/M7 实测的缺口） | ✅ 登记 |
  | **D 缺口修复** | 顺带补上的平台能力（如通知落库适配器） | ✅ 登记 |
- **类型**: 功能 / 可维护性

---

### AC-S-187: 业务代码量**实测并归因**（口径与结构性风险同时交付）

- **前置条件**: `modules/leave/` 已完成（AC-S-192 通过）。
- **测试步骤**（**三条命令都必须贴出实际输出**）:
  ```bash
  # ① 后端业务代码（排除测试、迁移、生成产物）
  find modules/leave/backend/src -name '*.rs' | xargs wc -l | tail -1
  # ② 前端业务代码
  find modules/leave/frontend -type f \( -name '*.svelte' -o -name '*.ts' \) | xargs wc -l | tail -1
  # ③ 非代码资产（单独列出，不计入 ①②）
  find modules/leave/migrations -name '*.sql' | xargs wc -l | tail -1
  ```
- **预期结果**:
  - **① + ②** 合计 **< 500 行**（需求原文的硬指标，**不放宽**）；
  - ③（SQL 迁移）**单独列出**，不混入 500 行口径；
  - **若实测 ≥ 500 行**：判 **不成立**，并在度量报告里给出「超了多少行 + 逐项归因」。
- **⚠️ rev 2 新增：必须预先披露结构性风险（B5）**
  实测脚手架骨架就占：

  | 文件 | 行数 |
  |---|---|
  | `REPOSITORY_RS` | 194 |
  | `SERVICE_RS` | 66 |
  | `BACKEND_MOD_RS` | 57 |
  | `HTTP_RS` | 56 |
  | `PERMISSION_RS` | 21 |
  | `ENTITY_RS` | 17 |
  | **后端骨架合计** | **411** |
  | 前端骨架（`FRONTEND_ROUTES_TS` 27 + `PAGE_SVELTE` 20） | 47 |
  | **总计** | **458** |

  （取数命令：见 `crates/xtask/src/scaffold.rs` 的 `const` 常量，逐块 `wc -l`）
  → **骨架已占 458 行，预算只剩 42 行给全部业务逻辑**（三态状态机 + 4 个端点 +
  审计/通知埋点 + 参数校验）。**结论：500 行大概率不成立。**
  → 但**不得**因此修改口径。若超标，判不成立并归因 ——
  这本身就是本需求该产出的结论：**底座样板代码偏重**。
- **类型**: 可维护性

---

### AC-S-188: 复用平台能力，不自造轮子（grep 模式与阳性对照已修正）

- **前置条件**: `modules/leave/backend/src/` 已完成。
- **测试步骤**（**每条都必须同时给出阳性对照命中数**，§9.2）:

  | # | 断言 | 期望 | **阳性对照**（实测值） |
  |---|---|---|---|
  | ① | `grep -rn "TenantDb\|TenantQuery" modules/leave/backend/src/ \| wc -l` | > 0 | — |
  | ② | `grep -rn "sqlx::query\|sqlx::query_as" modules/leave/backend/src/ \| wc -l` | **== 0** | `crates/platform-db/src/` → **10** |
  | ③ | `grep -rn "require_permission(" modules/leave/backend/src/ \| wc -l` | > 0 | — |
  | ④ | `grep -rniE "jwt\|jsonwebtoken\|argon2" modules/leave/backend/src/ \| wc -l` | **== 0** | `crates/platform-iam/src/` → **163**（rev 2 曾记 159，实现期实测修正，见独立复核 F-12；两种都 >0，判定不受影响） |
  | ⑤ | `grep -rn "AuditEvent\|AuditSink" modules/leave/backend/src/ \| wc -l` | > 0 | — |
  | ⑥ | `grep -rn "platform_audit_log\|platform_audit_append" modules/leave/backend/src/ \| wc -l` | **== 0** | `crates/platform-audit/src/sink.rs` → **> 0** |
  | ⑦ | `grep -rn "NotificationSender\|NotificationMessage" modules/leave/backend/src/ \| wc -l` | > 0 | — |
  | ⑧ | `grep -rn "platform-notify" modules/leave/Cargo.toml \| wc -l` | **== 0** | —（这是「不得有」，不需要阳性对照） |

- **预期结果**: 八条全部满足，且每条「== 0」断言的阳性对照**都 > 0**。
  任何一条阳性对照为 0 → **该断言的结论作废**（不是「通过」）。
- **⚠️ rev 2 修正的三处（M2 / M3 / M4）**
  1. **⑦ 原写 `grep "NewDelivery\|notify"`** —— 两个问题：
     - `NewDelivery` 定义在 `platform-notify`（正是 ⑧ 禁止依赖的 crate）；
     - `notify` 大小写敏感，**不匹配** `NotificationSender`（实测 `grep -c` 得 0）→ 假红。
     → 改为 `NotificationSender|NotificationMessage`。
  2. **⑥ 原写 `INSERT INTO.*audit`** —— 无区分力且可绕过：
     - 平台自己的审计写入走 `TenantQuery::new("SELECT platform_audit_append(...)")`，
       **不匹配**该模式；
     - 业务可用 `TenantQuery::new("INSERT INTO platform_audit_log\n …")` 绕过
       （不触发 `sqlx::query`、多行不匹配点号）。
     → 改为「不得出现审计表名 / 审计函数名」，阳性对照取 `sink.rs`。
  3. **② 的阳性对照原写「对 `modules/sandbox` 跑」** —— sandbox 全走 `TenantQuery`，
     `sqlx::query` 实测 **0** → 该对照**永远处于「结论不可信」状态**。
     → 改取 `crates/platform-db/src/`（实测 10）。
- **类型**: 功能 / 可维护性

---

### AC-S-189: 审批流状态机成立，且流转被平台审计覆盖

- **前置条件**:
  - 实体 `LeaveRequest` 有 `status` 字段，取值闭集 **4 个**：
    `draft` / `pending` / `approved` / `rejected`（⚠️ rev 2 修正 N1：rev 1 说「三态」
    却列了 4 个取值，措辞矛盾）；
  - 已登录用户 A（申请人）与 B（审批人），B 持 `leave:request:approve`。
- **测试步骤**（**必须通过 HTTP 端点驱动**，不得直接调 service，§9.15）:
  1. A 建单 → `draft`；
  2. A 提交 → `pending`；
  3. B 审批通过 → `approved`；
  4. 另建一单走驳回 → `rejected`；
  5. 查审计链（`object_id == 本单`）；
  6. 查通知投递（`platform_notify_delivery`，本单相关）；
  7. **非法流转**：对已 `approved` 的单**再 submit**、**再 decide**（rev 2 补后者，N2）。
- **预期结果**（**计数表，rev 2 新增，解决 M5**）:

  | # | 事件 | 审计条数 | 通知条数 | 收件人 |
  |---|---|---|---|---|
  | 1 | 建单（`draft`） | **1** | 0 | — |
  | 2 | 提交（`→ pending`） | **1** | **0** | —（⚠️ 见下方不可达说明） |
  | 3 | 审批通过（`→ approved`） | **1** | **1** | 申请人 A |
  | 4 | 驳回（`→ rejected`） | **1** | **1** | 申请人 A |
  | 5 | 非法流转（步骤 7） | **0** | **0** | — |

  ⚠️ **rev 2 实现期修正（第 2 行通知数 1 → 0）**：
  原设想「提交时通知审批人 B」，但实测平台**没有「按权限点反查用户」的能力** ——
  `PermissionChecker` 只有 `has` / `scope` / `effective_scope`
  （`crates/platform-core/src/extension/permission.rs:169-178`），
  业务无法知道「谁持有 `leave:request:approve`」。
  → 改为**提交不发通知**；审批结果出来时通知**申请人**（业务自己知道 `applicant_id`）。
  → 这仍能证明「通知复用」成立（2 条真实落库通知），并如实记下缺口 **TD-088**。

  - ⚠️ **「建单算不算流转」必须写死**：本表规定**算 1 条审计、0 条通知**
    （创建是状态初始化，不需要通知任何人）。rev 1 的「每次流转恰好一条」与 TC 的
    「3 次状态变更 → 3 条审计」互相矛盾（M5）。
  - 步骤 7 返回 **409**，且 `code == "LEAVE_INVALID_TRANSITION"`
    （⚠️ rev 2 修正 N3：rev 1 只说「409 或 422 + 明确错误码」，
    而契约 C6 要求 `code` 是稳定的大写下划线码 —— 必须写死具体值）。
  - ⚠️ **rev 2 实现期补充**：该码必须**注册进 `platform-core`**
    （`titles.rs` 的 `TITLES` + `problem.rs` 的 `ALL_CODES`），否则
    `title_of_code()` 会落到兜底 `"Error"`（`titles.rs:292-298` 实测）→ 违反契约 C6。
    这是**平台改动**（D 类，见 design-summary §3 第 7/8 行）。
    ⚠️ 已有的 `modules/sandbox` **没有**自定义错误码（全用平台码）——
    说明「业务模块注册自有码」这条路在示例里没走通过，本轮是**首次**。
- **⚠️ 通知落库的处置（B4 的修正，D354）**
  rev 1 要求「查真实的 `platform_notify_delivery` 表」，但实测：
  - `NotificationSender`（`platform-core/src/extension/notification.rs:81`）
    **只有 `send()` / `channel()`**，没有入队方法；
  - `platform_notify_delivery` 的唯一写入入口是 `platform-notify/src/service.rs:191`
    的 `enqueue`，而业务**不得依赖** `platform-notify`（AC-S-193）。
  → rev 2 要求平台补一个**落库适配器**（`DbNotificationSender`，实现 `NotificationSender`，
  内部调 `enqueue`），**登记为 D 类改动**。
  业务侧只调 trait，不碰 `platform-notify` —— 两条 AC 同时成立。
- **类型**: 功能 / 安全

---

### AC-S-190: 平台 v1→v2 升级，消费方业务代码零改动（承接 AC-030）

- **前置条件**:
  - `modules/leave` **已提交**（形成 `BASE_UPGRADE`）；
  - 升级对象 = `GET /api/rbac/me/permissions`（消费方前端经 `can()` 真实依赖）；
  - v2 语义按 **D352** 定义（**不是** rev 1 说的「裸数组 → 对象」）。
- **测试步骤**:
  1. `git diff --name-only BASE_UPGRADE -- modules/leave/` → 记录；
  2. 升级后跑消费方全部测试 + e2e；
  3. 打 v1 → 检查 `Deprecation` / `Sunset`；
  4. 打 v2 → 走新语义；
  5. 检查消费方是否改了业务代码。
- **预期结果**:
  - 步骤 1：**空输出**（消费方业务目录零改动）；
  - 步骤 2：全部通过；
  - 步骤 3：两个头都在，`Sunset` 严格 IMF-fixdate；
  - 步骤 4：新语义生效；
  - 步骤 5：零改动。若被迫改 → **AC 不成立**，且这是「破坏性变更」的实证。
- **⚠️ rev 2 修正的事实错误（B3）**
  rev 1 写「v1 是裸数组 `["rbac:role:read"]`」—— **错的**。实测：
  ```rust
  // crates/platform-rbac/src/http.rs:420-431
  /// # 为什么是**对象**而不是裸数组
  /// 改形状是破坏性变更。
  pub struct MyPermissionsResponse { pub permissions: Vec<String> }
  ```
  且 `crates/platform-rbac/tests/me_permissions.rs:331` 有已 ratified 的断言
  「`assert!(body.is_object())` / 「不能是裸数组」」。
  → v2 **不能**改成裸数组（那会打红 REQ-011 的用例）。
  v2 的语义变化必须按 D352 重选（见该决策）。
- **类型**: 可维护性

---

### AC-S-191: 度量报告可复算（每个数字都有可执行命令）

- **前置条件**: AC-S-186 ~ 190 已跑完。
- **测试步骤**: 按报告里的命令**逐条重跑**，比对数值。
- **预期结果**:
  - 报告里**每个数字**都附**可执行命令**（不是「输出模式」）；
  - 同一 commit 上换人重跑，结果**逐字相同**；
  - 只纳入可机械复现的指标（D350）。
- **⚠️ rev 2 修正（M9 / M10）：把「输出模式」换成确切命令**
  rev 1 的「测试通过数」写的是「全量输出的 `test result: ok. N passed`」——
  那是**输出模式**，不是命令，且没说要不要 `--include-ignored`
  → 同一 commit 能报出两个 N。rev 2 写死：

  | 指标 | 确切命令 |
  |---|---|
  | 后端业务代码 | `find modules/leave/backend/src -name '*.rs' \| xargs wc -l \| tail -1` |
  | 前端业务代码 | `find modules/leave/frontend -type f \( -name '*.svelte' -o -name '*.ts' \) \| xargs wc -l \| tail -1` |
  | SQL 迁移 | `find modules/leave/migrations -name '*.sql' \| xargs wc -l \| tail -1` |
  | 平台改动（全仓） | `git diff --name-only <BASE_IMPL> \| grep -v '^modules/leave/' \| wc -l` |
  | 依赖面 | `grep -E '^platform-' modules/leave/Cargo.toml` |
  | 契约检查 | `cargo xtask check-modules && cargo xtask check-deps && cargo xtask capabilities --check`（三个退出码） |
  | 全量测试 | 见下方固定循环（逐 crate，**必带** `--include-ignored`） |
  | E2E skipped | `npx playwright test e2e/leave.spec.ts --reporter=json \| jq '.stats.skipped'`（实测输出 `0`）<br>⚠️ 原写法 `[..\|.skipped?//empty\|select(.!=0)]\|length` 在 **jq 1.8.2 上是语法错误**（`?//` 被解析成解构备选运算符）——实现期实测修正 |

  全量测试的固定循环（**逐 crate 跑，本仓 `cargo test --workspace` 必然失败**）：
  ```bash
  for c in platform-core platform-audit platform-config platform-contracts \
           platform-contracts-macros platform-app platform-notify platform-storage \
           platform-observability platform-db platform-iam platform-oauth platform-org \
           platform-rbac platform-schema platform-server xtask sandbox; do
    cargo test -p $c --all-targets -- --test-threads=1 --include-ignored 2>&1 \
      | grep 'test result' | awk -v c=$c '{p+=$4;f+=$6;i+=$8} END{printf "%s: %d/%d/%d\n", c, p, f, i}'
  done
  ```
  （`leave` 模块跑通后要加进这个列表。）
- **判据**: 报告里出现**一个没有可执行命令的数字** → 该条判不成立。
- **类型**: 可维护性

---

### AC-S-192: 消费方端到端可用（范围写死）

- **前置条件**: 前端页面已装配（⚠️ 见下方 M7 的处置）。
- **测试步骤**（**范围写死，rev 2 修正 M10**）:
  ```bash
  npx playwright test e2e/leave.spec.ts --reporter=json
  ```
  1. 打开列表页 → 渲染出已有单据；
  2. 新建 → 填表 → 提交 → 列表出现新行；
  3. 切审批人账号 → 审批通过 → 状态变为「已通过」；
  4. **刷新页面** → 状态保持。
- **预期结果**:
  - 四条全通过；
  - **`e2e/leave.spec.ts` 这一个文件的 `skipped == 0`**（不是全量 suite ——
    实测其他 spec 有条件 skip：`e2e/audit.spec.ts:157,186,225,254,298`、
    `e2e/schema-driven.spec.ts:137`，用全量会「换个范围就能红变绿」）；
  - 步骤 4 必须**刷新后再查**。
- **⚠️ 前端页面装配的处置（M7，D356）**
  rev 1 写「前置条件：前端 shell 已装配 `modules/leave` 的页面」—— 实测**不存在**：
  - `apps/shell/src/lib/routes.ts:47`：「`/schema/:moduleId/:entityKey` 是**唯一**的数据页面路由：
    新增实体只改后端 manifest，`apps/shell/src/**` 一行不改」；
  - `grep -rn "modules/" apps/shell/src` → **无命中**（连 `modules/sandbox/frontend`
    都没被引用，`DemoPage.svelte` 是孤儿）；
  - 通配页的动作列只有编辑/删除（`SchemaTable.svelte:131-148`），
    manifest schema 的 `api` 只有 4 个键（`platform-core/src/extension/schema.rs:138-141`）
    → **没有「审批」这种行内动作**。
  → 两条路：① 写自定义页面 + 在 `apps/shell/src/**` 加硬编码路由（**登记为 C 类改动**）；
  ② 扩展 manifest 的行内动作能力（那是平台改动，**登记为 D 类**）。
  rev 2 选 **①**（成本更低），并**禁止**第三种做法：用通配表单直接改 `status` 字段
  —— 那会绕过状态机与 409 校验，AC-S-189 的安全判据被 UI 旁路。
- **类型**: 功能

---

### AC-S-193: 消费方的依赖面只有平台公开面

- **前置条件**: `modules/leave/Cargo.toml` 已定稿。
- **测试步骤**:
  ```bash
  grep -E '^platform-' modules/leave/Cargo.toml
  cargo xtask check-deps
  ```
- **预期结果**:
  - 只出现 `platform-core`、`platform-db`（运行期）、`platform-contracts`（dev）；
  - `check-deps` 退出码 **0**。
- **⚠️ rev 2 修正 M1：`check-deps` 的射程**
  实测 `crates/xtask/src/deps.rs:217-224` 只做
  「平台 crate 是否反向依赖业务模块」，**不校验**「业务是否正确依赖平台内部」。
  → rev 1 把它的退出码解释成「新模块的依赖方向合规」是**过度解读**。
  → rev 2 精确化：
    - `check-deps` 管「**平台不依赖业务**」（本 AC 步骤 2）；
    - 「**业务不依赖平台内部**」由步骤 1 的 grep 管（**逐条列出**禁止的 crate）。
- **类型**: 可维护性

---

## AC 覆盖对照（父 AC → 本需求 AC）

| 父 AC 的每个要求 | 由谁覆盖 |
|---|---|
| 业务代码集中在独立目录 | AC-S-186（全仓 diff 分类） |
| 不修改平台核心任何一行 | AC-S-186 + D353（A 类 = 0，B/C/D 逐项登记） |
| 用户 / 权限复用 | AC-S-188 ③ + AC-S-189 步骤 3 |
| 租户复用 | AC-S-188 ① |
| 通知复用 | AC-S-188 ⑦⑧ + AC-S-189 计数表 + D354 |
| 审计复用 | AC-S-188 ⑤⑥ + AC-S-189 计数表 |
| **工作流复用** | ❌ **本轮不承担**（REQ-017 取消，D347） |
| 升级零改动 | AC-S-190（`BASE_UPGRADE`） |
| 弃用头提前通知 | AC-S-190 步骤 3 |
| 度量指标取数 | AC-S-191 |

## 汇总（rev 2）

**8 条 AC**：AC-S-186 ~ AC-S-193。

**rev 1 → rev 2 的变更**：

| 挑刺 | 处置 |
|---|---|
| B1 装配点硬编码 | AC-S-186 改「分类登记」，装配点归 **C 类**（D355） |
| B2 BASE 共用 | 拆 `BASE_IMPL` / `BASE_UPGRADE`，要求提交顺序 |
| B3 v1 是对象不是数组 | AC-S-190 事实修正 + 重选 v2 语义（D352） |
| B4 通知落库不可达 | AC-S-189 + D354：平台补 `DbNotificationSender`（D 类） |
| B5 500 行不可达 | AC-S-187 预先披露「骨架已占 458 行」，超标则判不成立 + 归因 |
| B6 口径漏改动面 | AC-S-186 改「全仓 diff 减 `modules/leave/`」 |
| M1~M10 | 逐条修正（见各 AC 的 ⚠️ 段） |
| N1~N10 | 逐条修正（计数表 / 错误码 / 悬空引用 / 伪代码 / 统计矛盾等） |

## ⚠️ 本需求的预期结论（诚实预设）

基于 rev 2 的实测，本需求**很可能**得出以下结论之一或多个：

1. **500 行预算不成立**（骨架已占 458 行）；
2. **平台改动不为 0**（装配点 + 通知适配器 + 前端路由，C/D 类）；
3. **`WorkflowHandler` 是孤儿**（REQ-017 取消的遗留）。

**这些不是失败，而是本需求的核心产出** ——
「底座可用性验证」的价值就在于**把「照文档应该能」与「实际能不能」的差距量出来**。
若最终结论是「底座已经很省代码」，那同样要给出证据。
