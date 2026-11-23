# 决策记录：REQ-020 底座可用性验证

- **需求编号**: REQ-020
- **状态**: rev 1
- **更新时间**: 2026-11-23

> 编号接续 REQ-019 的 D346。

---

## D347: 审批流 = **业务层自实现三态**，**不声称复用工作流引擎**（用户裁定）

### 问题

本需求原文要求「一个实体 + 列表 + 表单 + **一个审批流**」，
父 AC-029 也要求「用户 / 权限 / 租户 / 通知 / 审计 / **工作流**全部复用」。

但实测（2026-11-23）：

```console
$ grep -rn "on_transition\|WorkflowHandler" --include=*.rs crates/ modules/ | grep -v workflow.rs
crates/xtask/src/capabilities.rs:50:    "WorkflowHandler",   ← 能力索引字符串
crates/xtask/src/catalog.rs:79-80:  ...                 ← 目录条目
crates/platform-core/src/lib.rs:22:  ...                 ← re-export
crates/platform-core/src/extension/mod.rs:80:  ...       ← re-export
```

- **REQ-017（工作流引擎）已取消**（D121，用户裁定「我应该没有说要加这个吧」）；
- `WorkflowHandler::on_transition` 在平台侧**零调用点**，只有测试里的 `Demo` 实现
  → 它是**孤儿 trait**；
- 要让「工作流复用」成立，必须**给平台加一处调用链** ——
  而那恰好违反父 AC-029 的「**不修改平台核心任何一行**」。

**三者不可兼得。**

### 候选方案

| 方案 | 改动面 | 能否满足 AC-029 | 代价 |
|---|---|---|---|
| ① 业务层自实现三态，平台只做审计+通知 | 只碰 `modules/leave/` | ✅ 成立 | 「工作流复用」这半句不承担 |
| ② 先补工作流接缝（改平台），再验复用 | 改 `crates/platform-core` + 调用方 | ❌ 破了「不改任何一行」 | 需求范围扩大一轮 |
| ③ 去掉审批流，只验其他能力 | 只碰 `modules/leave/` | ✅ 成立 | 需求缩水，违背原文 |

### 决定

**用户 2026-11-23 裁定：方案 ①。**

- 审批流是**业务逻辑**（请假单的状态机：`draft → pending → approved / rejected`），
  由 `modules/leave/` 自己实现；
- 平台提供的是**流转的横切能力**：每次状态变更 → 平台审计（`AuditEvent`）+
  平台通知（`NewDelivery`）；
- 本需求**不声称**复用了工作流引擎；AC 对照表里「工作流复用」明确标为
  **本轮不承担**；
- `WorkflowHandler` 保持孤儿状态，**记入技术债**（TD-085），
  等真正需要流程引擎时再决定「复活 REQ-017」还是「删掉这个 trait」。

### ⚠️ 为什么这是诚实的选择

反过来说：如果为了让 AC 好看而给 `WorkflowHandler` 硬接一条调用链，
那只是**制造一个假消费方** —— 平台从未有流程引擎，接一条调用链不等于有工作流能力。
**「验证」不能变成「演戏」。**

### 代价（明码记账）

- 父 AC-029 的「工作流复用」在本轮**未验证**；
- 审批流的状态机是**业务代码**，计入 AC-S-187 的 500 行预算；
- 新增 **TD-085**（`WorkflowHandler` 孤儿）。

---

## D348: 消费方选型 = **`modules/leave`（请假审批）**

### 问题

需要一个「最小业务系统」来验证。名字与领域会影响：
- 500 行预算能否达成（实体越简单越容易）；
- 是否能自然覆盖「列表 + 表单 + 审批流」三件套。

### 候选

| 候选 | 实体复杂度 | 审批流自然度 | 与现有模块冲突 |
|---|---|---|---|
| `leave`（请假） | 低（申请人 / 起止 / 理由 / 状态） | 高（申请→审批→通过/驳回） | 无 |
| `order`（订单） | 中（多明细行） | 中 | 无（仅父 AC 举例用过 `order:invoice:approve`） |
| `ticket`（工单） | 中 | 中 | 无 |

### 决定

选 **`modules/leave`**。

理由：
1. **实体最简单** —— 单表、无明细行，500 行预算最容易验证「复用是否真的省代码」；
2. **审批流最自然** —— 天然的两角色（申请人 / 审批人）+ 三态；
3. 与 `modules/sandbox`（扩展示例）职责不重叠，不会混淆「示例」与「真实消费方」。

⚠️ 不用 `order` 的理由：父 AC 里的 `order:invoice:approve` 只是**权限点命名的举例**，
不是要求本需求建订单模块。避免让人误以为要复用那个举例。

---

## D349: 「升级零改动」= 造一次**真实 v1→v2**（用户裁定）

### 问题

父 AC-030 要求「底座升级一个次要版本并升级业务依赖 → 业务无需改代码即可运行；
破坏性变更通过版本化 API + 弃用头提前通知」。

但现状：弃用头机制已就绪（REQ-019 D342），**却从未演练过一次真实升级** ——
`default_deprecations()` 只登记了 `/api/audit/events` 一条，且没有 v2 端点。

### 候选

| 方案 | 能验到什么 | 代价 |
|---|---|---|
| ① 造真实 v1→v2（加弃用头） | 真验：旧路径可用 + 弃用头 + 业务零改动 | 平台要改一处（1~2 个文件） |
| ② 只静态断言消费方只依赖稳定面 | 验到「依赖面干净」 | **验不到**「升级后仍可跑」 |

### 决定

**用户 2026-11-23 裁定：方案 ①。**

- 选一个 `modules/leave` 会依赖的平台接口，造出 v2（**语义真变**，不是改名字）；
- 旧路径保留并登记 `Deprecation` + `Sunset`；
- 消费方**业务代码零改动**，升级后所有测试与 e2e 仍然通过。

### ⚠️ 与「不改平台任何一行」的张力（由 D351 解决）

方案 ① 必然改 `crates/**`。这不与 AC-029 冲突，因为**两件事的主体不同**：
- AC-029 说的是「**消费方的开发过程**不改平台」；
- 升级演示是「**底座维护者**做的一次演进」。

→ 见 D351 对口径的精确化。

---

## D350: 度量指标 = **只留可机械复现的**（用户裁定）

### 问题

需求原文的度量指标包含「**上手时间**」「**AI 一次过 CI 比例**」。
而 AC-S-028 要求「度量方法与数据集随验证报告一并交付，**换人复测结论一致**」。

这两者**直接冲突**：
- 「上手时间」取决于人的经验、机器性能、当时的干扰 —— 换人**必然不同**；
- 「AI 一次过 CI 比例」取决于用哪个模型、什么提示、什么随机性 —— **不可复现**。

### 候选

| 方案 | 与 AC-S-028 一致 | 信息量 |
|---|---|---|
| ① 只留可机械复现的指标 | ✅ | 少，但每个数都能核 |
| ② 保留但标为「观察项」，不作判据 | ✅（不作判据） | 多，但不可核 |
| ③ 用可测代理（步骤数 / 命令数） | 部分（代理仍需主观界定） | 中 |

### 决定

**用户 2026-11-23 裁定：方案 ① —— 只留可机械复现的指标。**

纳入（每条都有取数命令，见 AC-S-191）：

| 指标 | 取数方式 |
|---|---|
| 业务代码行数 | `wc -l`（三条命令） |
| 平台改动文件数 | `git diff --name-only BASE -- crates/ \| wc -l` |
| 业务目录外改动 | `git diff --name-only BASE -- modules/ \| grep -v '^modules/leave/' \| wc -l` |
| 依赖面 | `grep -E '^platform-' modules/leave/Cargo.toml` |
| 契约检查 | `cargo xtask check-deps` / `capabilities --check` 退出码 |
| 测试通过数 | `test result: ok. N passed` |

**剔除**：上手时间、AI 一次过率、代码审美评分。

### ⚠️ 判据

报告里出现**任何一个没有取数命令的数字** → AC-S-191 判不成立。
「我觉得大概花了半天」这类描述可以写在附录，但**不得出现在度量表里**。

---

## D351: 「不改平台」的**精确口径**

### 问题

AC-029 说「不修改平台核心任何一行」，D349 又要造一次平台升级。
字面上冲突，必须精确化，否则 AC-S-186 无法判定。

### 决定

~~把「平台改动」分成两类~~ → **rev 2 改为四类**（D353），原因见该决策。

| 类别 | 内容 | 判定 |
|---|---|---|
| **A. 可避免** | 有平台既有机制却不用，非要改平台 | ❌ **必须为 0** |
| **B. 升级演示** | AC-S-190 必需的 v2 端点 + 弃用登记 | ✅ 逐项登记 |
| **C. 装配必需** | 让新模块能被服务加载所必需（实测缺口） | ✅ 逐项登记 |
| **D. 缺口修复** | 顺带补上的平台能力（如通知落库适配器） | ✅ 逐项登记 |

### 登记要求

`design-summary.md` 的「平台改动清单」必须：
1. 逐文件列出（路径 + 改了什么 + 为什么是 AC-S-190 必需的）；
2. 与 `git diff BASE -- crates/` 的实际输出**逐项对应**
   （多一个文件 → AC-S-186 不成立）；
3. 每个文件都要能回答：「不删它，AC-S-190 还能验吗？」答案是「不能」才算必需。

### ⚠️ 为什么必须这么严

否则「升级零改动」这条 AC 会变成**自我实现**：
只要允许随便改平台，消费方当然能跑。
**这条口径的作用是把「改平台」的额度锁死在升级演示之内。**

### 代价

- 若实现中发现底座真的缺能力（必须改平台才能跑消费方），
  这不是「违规」，而是**本需求最有价值的产出** ——
  要写进度量报告的「底座缺口」一节，而不是偷偷改掉。

---

## D352: API 版本化机制 = **路径前缀**（`/api/<module>/v2/...`）

### 问题

D349 要造一次真实 v1→v2 升级，但**契约里没有任何版本化约定**：

```console
$ grep -n "^## C" docs/contracts/api-conventions/v1.md
22: ## C1 平台 API 分组前缀      ← 只规定 /api/<module>
44: ## C2 业务 API 路径形式
...
173: ## C13 命名与编码约定          ← C1~C13 全无「版本」条目

$ grep -rn "Accept\|application/json; version\|/v2/" docs/contracts/   → 无输出
```

rev 1 的 `design-summary.md:98-99` 写「`docs/contracts/api-conventions/v2.md`
已有约定 → 以它为准」—— **这是错的**（挑刺 M6 实测）：
`v2.md` 是**契约文档本身的第 2 版**（C9/C10 两处修订，自述 `breaking: no`），
**完全不含 HTTP 端点版本协商规则**。

### 候选

| 方案 | 优点 | 缺点 |
|---|---|---|
| **路径前缀** `/api/rbac/v2/me/permissions` | 简单、可静态检查；**与弃用机制天然配合**（`apply_deprecation_router` 本来就是按**路径**精确匹配） | 路径变长 |
| `Accept: application/json; version=2` 协商 | 更 RESTful | 同一路径要按版本区分弃用状态 → 弃用中间件要改（那是 B 类改动之外的新增复杂度） |

### 决定

选**路径前缀**。

关键理由：`crates/platform-observability/src/deprecation.rs:137-143` 的实现是
```rust
request.uri().path() == path      // 精确字符串相等
```
→ 弃用登记是**按完整路径**做的。若用 `Accept` 协商，同一路径会同时是「v1 已弃用」
和「v2 正常」，弃用中间件必须知道当前请求用的是哪个版本 —— 那是额外的复杂度。

### 落地

- v1：`GET /api/rbac/me/permissions`（保持不变 → 现有 `tc_011_001` 仍绿）
- v2：`GET /api/rbac/v2/me/permissions`
- v1 在 `default_deprecations()` 登记 → 响应带 `Deprecation` / `Sunset`

### ⚠️ v2 的语义变化必须**重选**（B3 的连带修正）

rev 1 写「v1 = 裸数组 → v2 = 对象」—— **事实错误**：v1 本来就是对象
（`platform-rbac/src/http.rs:420-431`，且 `me_permissions.rs:331` 有已 ratified 的
「不能是裸数组」断言）。

→ v2 的语义变化改选：**`permissions` 从「已展开的扁平字符串数组」改为
「按来源分组的结构」**：

```json
// v1
{ "permissions": ["rbac:role:read", "leave:request:write"] }

// v2
{ "permissions": ["rbac:role:read", "leave:request:write"],
  "sources": { "rbac:role:read": ["role:admin"], "leave:request:write": ["role:manager"] } }
```

为什么这个变化**有验证价值**：
- 消费方如果**穷举**了响应键（TS 的严格类型、Rust 的 `match` 带 `_` 除外）
  → 会因为多了一个 `sources` 键而需要改代码；
- 本需求要验证的正是「**消费方没被强制改**」—— 它本该按**开放对象**消费（忽略未知键）。
- 若消费方本来写得很脆（严格穷举），这条 AC 会**红** —— 那才是真发现。

---

## D353: 平台改动**四分类**（A/B/C/D）

### 问题

D351 rev 1 只有两类（消费方倒逼 / 升级演示），但挑刺 B1/B4/M7 实测出**第三类**：
「让新业务模块能被服务加载所必需」的改动 —— 它既不是「倒逼的浪费」，
也不是「升级演示」，而是**底座本身的装配缺口**。

### 决定

四类，只有 A 类必须为 0：

| 类 | 定义 | 判定 | 判定方法 |
|---|---|---|---|
| **A 可避免** | 平台已有机制却不用，非要改平台 | ❌ **必须为 0** | 逐条审视：有没有既有扩展点/装配入口可用？ |
| **B 升级演示** | AC-S-190 必需的 v2 + 弃用登记 | ✅ | 「删掉它，AC-S-190 就判不了」 |
| **C 装配必需** | 让新模块能被加载所必需 | ✅ | 「删掉它，模块 404 / 编译不过」 |
| **D 缺口修复** | 补上平台缺失的能力 | ✅ | 「删掉它，某条 AC 的判据无法成立」 |

### ⚠️ 为什么 A 类仍然必须为 0

否则这条 AC 会退化成「随便改平台，然后说『这是必需的』」。
**A 类的存在把「借口」和「真缺口」分开了**：
C/D 类必须给出「不删它哪个 AC 就判不了」的具体答案，A 类给不出。

### 实测的 C/D 类清单（rev 2 已知，实现时可能增加）

| 类 | 文件 | 为什么必需 |
|---|---|---|
| **C** | 根 `Cargo.toml`（members） | 不加 `"modules/leave"`，crate 不在工作区 |
| **C** | `crates/platform-server/Cargo.toml` | 不加依赖，`::mount` 调不到 |
| **C** | `crates/platform-server/src/main.rs` | `"leave" => {...}` 装配臂 + 认证层；不加则全部 404 |
| **C** | `apps/shell/src/**` | 前端页面路由（M7：通配页无审批动作） |
| **D** | `crates/platform-notify` 或 `platform-core` | 补 `DbNotificationSender`（B4） |
| **B** | `crates/platform-rbac/src/http.rs` | v2 端点（D352） |
| **B** | `crates/platform-observability/src/http.rs` | v1 弃用登记 |
| — | `capabilities/**` | **生成物**，由 `capabilities` 命令重新生成，不算手工改动 |

---

## D354: 通知落库缺口 → 平台补 `DbNotificationSender`

### 问题（挑刺 B4 实测）

rev 1 要求「查真实的 `platform_notify_delivery` 表」，但：

1. `platform_core::extension::notification::NotificationSender`（`:81`）
   **只有 `channel()` / `send()`**，没有入队方法；
2. `platform_notify_delivery` 的唯一写入入口是
   `platform-notify/src/service.rs:191` 的 `enqueue`；
3. 而 AC-S-193 禁止业务依赖 `platform-notify`；
4. 现有 4 个 `NotificationSender` 实现全是「通道发送器」，
   其中 InApp 把消息存进**内存 Vec**（`channels/in_app.rs:55-62`），**不写表**。

→ 三条路全堵死：依赖 notify（违 AC-S-193）/ 断言内存 sender（正是「假 sink」）/
业务自己写表（违 AC-S-188 ⑥）。

### 决定

**方案：平台补一个落库适配器 `DbNotificationSender`**（D 类改动，登记）。

- 位置：`crates/platform-notify/src/` 或 `crates/platform-server/src/`（实现时定）；
- 实现 `NotificationSender`，内部调 `PlatformNotifyService::enqueue`；
- 业务侧通过 trait 注入，**不碰** `platform-notify` 的依赖 → AC-S-188 ⑦⑧ 与
  AC-S-193 同时成立。

### ⚠️ 这本身是「底座缺口」的证据

「业务要发通知，但平台没给落库入口」是本需求应产出的结论之一。
补上它是 D 类（缺口修复），不是「倒逼的浪费」（A 类）。

---

## D355: 业务模块装配点缺口 → 登记 C 类 + **TD-086**

### 问题（挑刺 B1 实测）

```rust
// crates/platform-server/src/main.rs:553-586
match *module_id {
    "sandbox" => { ... app = app.nest("/api/sandbox", router); ... }
    "org" => {}
    other => {
        tracing::warn!(module = other, "模块已启用但没有 HTTP 装配点：its routes are not served");
    }
}
```

→ 新模块 `leave` **必须**加一条 match 臂才能挂载，否则启动只有一行 warn + 全 404。
认证层也在装配臂里加（`:562-572`）。

**这是父 AC-029「不修改平台核心任何一行」的硬骨头**：
不加臂 → 模块永远 404（AC-S-189/192 无从谈起）；加臂 → 改 `crates/**`。

### 决定

短期：**登记为 C 类改动**（D353），在 `design-summary.md` §3 逐项列出（3 个文件：
根 `Cargo.toml` / `platform-server/Cargo.toml` / `main.rs`）。

长期：**记 TD-086** —— 「业务模块装配是硬编码 match，新模块必须改平台」。
理想形态是「模块自注册路由」（`discover_enabled()` 已经能发现模块，
但**挂载**仍要手写）。这是底座的设计缺陷，不是本需求能解决的。

---

## D356: 前端页面动作缺口 → 自定义页面 + shell 路由（C 类）+ **TD-087**

### 问题（挑刺 M7 实测）

- `apps/shell/src/lib/routes.ts:47`：「`/schema/:moduleId/:entityKey` 是**唯一**的数据页面路由：
  新增实体只改后端 manifest，`apps/shell/src/**` 一行不改」；
- 但通配页的动作列只有编辑/删除（`SchemaTable.svelte:131-148`），
  manifest 的 `api` 只有 4 个键（`list`/`create`/`update`/`delete`，
  `platform-core/src/extension/schema.rs:138-141`）→ **没有「审批」这类行内动作**；
- `grep -rn "modules/" apps/shell/src` → **无命中**（连 `modules/sandbox/frontend`
  都没被引用，`DemoPage.svelte` 是孤儿）。

→ 「前后端只写业务」在**有自定义动作**的场景下不成立。

### 决定

选 **① 写自定义页面 + 在 `apps/shell/src/**` 加一条硬编码路由**（C 类改动，登记）。

**明确禁止**第三种做法：用通配表单直接把 `status` 字段改成 `approved`。
那会**绕过状态机与 409 校验**（AC-S-189 的安全判据被 UI 旁路）——
这种「看起来能用」的实现是本需求要防的。

长期：**记 TD-087** —— 「manifest schema 无行内动作能力，带审批/流转的业务要写自定义页 +
改 shell」。要解决得扩展 schema 的 `actions` 能力（平台改动，不在本轮）。

---

## D357: 500 行预算 = **如实实测 + 归因，不改口径**

### 问题（挑刺 B5 实测）

脚手架骨架本身就占：

| 常量 | 行数 |
|---|---|
| `REPOSITORY_RS` | 194 |
| `SERVICE_RS` | 66 |
| `BACKEND_MOD_RS` | 57 |
| `HTTP_RS` | 56 |
| `PERMISSION_RS` | 21 |
| `ENTITY_RS` | 17 |
| **后端骨架** | **411** |
| `FRONTEND_ROUTES_TS` + `FRONTEND_PAGE_SVELTE` | 47 |
| **总计** | **458** |

→ 500 行预算只剩 **42 行**给：三态状态机 + 4 个端点 + 审计/通知埋点 + 参数校验。
**结构上不可达。**

### 决定

**不放宽口径，不提前改 AC。** 按以下处理：

1. AC-S-187 保留「< 500 行」硬指标；
2. 在 `acceptance.md` 与 `design-summary.md` **预先披露**骨架占 458 行（附取数方式）；
3. 实现后**如实测量并判定**：
   - < 500 → 达成；
   - ≥ 500 → **判不成立**，在度量报告里给出「超 N 行 + 逐项归因
     （必需样板 vs 业务逻辑）」。
4. 归因结果本身就是本需求的结论：**底座样板代码偏重**。

### ⚠️ 为什么不能「提前把 AC 改成 < 800」

那正是本项目反复禁止的「修改口径凑达标」（§9.15）。
一个**判不成立**的真实结论，比一个**被改到能过**的假结论有价值得多。
