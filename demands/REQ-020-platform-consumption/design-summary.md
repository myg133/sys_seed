# 设计摘要：REQ-020 底座可用性验证

- **需求编号**: REQ-020
- **状态**: **rev 2**（rev 1 经独立挑刺：6 BLOCKER / 10 MAJOR / 10 MINOR，本轮逐条处置）
- **更新时间**: 2026-11-23
- **关联**: [demand.md](demand.md) / [acceptance.md](acceptance.md) / [decisions.md](decisions.md)

> 本文件是**设计契约**，不是实现说明。实现在 `feature/REQ-020` 分支上做。

## ⚠️ rev 2 的核心修正（挑刺实测）

rev 1 有四处「照文档应该能，打开代码不能」，rev 2 全部改为**如实登记缺口**：

| # | rev 1 的假设 | 实测事实 | rev 2 处置 |
|---|---|---|---|
| B1 | 模块能被自动加载 | `main.rs:553-586` 是**硬编码 match**，未加臂只 warn + 404 | C 类改动 + TD-086（D355） |
| B4 | 业务能发落库通知 | `NotificationSender` 只有 `send()`；`enqueue` 在禁止依赖的 crate | 平台补 `DbNotificationSender`（D354） |
| M7 | shell 已有模块页面装配机制 | `apps/shell/src` 里**零** `modules/` 引用；通配页无审批动作 | 自定义页 + shell 路由（C 类）+ TD-087（D356） |
| B6 | 平台改动 = `crates/**` | 漏掉根 `Cargo.toml` / `capabilities/**` / `apps/` / `packages/` | 改「全仓 diff 减 `modules/leave/`」 |

---

## 1. 消费方设计（`modules/leave`，D348）

### 1.1 领域模型（单表）

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | UUID | 主键 |
| `tenant_id` | TEXT | **租户列**（RLS 强制） |
| `applicant_id` | TEXT | 申请人（来自 `ctx.user_id()`） |
| `starts_on` / `ends_on` | DATE | 起止日期 |
| `reason` | TEXT | 请假理由 |
| `status` | TEXT | 闭集 **4 个**：`draft` / `pending` / `approved` / `rejected` |
| `decided_by` | TEXT NULL | 审批人 |
| `decided_at` | TIMESTAMPTZ NULL | 审批时间 |
| `created_at` / `updated_at` | TIMESTAMPTZ | 平台惯例 |

⚠️ **单表、无明细行** —— 这是 500 行预算能成立的前提（D348）。
⚠️ **但预算仍大概率不成立**：骨架已占 458 行（见 §7 与 D357）。

### 1.2 状态机（`modules/leave` 自实现，D347）

```text
draft ──submit──► pending ──approve──► approved   （终态）
                    │
                    └──reject───► rejected        （终态）
```

**可达的非法流转**（只有 `submit` / `decide` 两个动作端点，所以只列这两类）：

| 当前状态 | 动作 | 期望 |
|---|---|---|
| `approved` | `submit` | **409** `LEAVE_INVALID_TRANSITION` |
| `approved` | `decide` | **409** 同上 |
| `rejected` | `submit` | **409** 同上 |
| `rejected` | `decide` | **409** 同上 |
| `pending` | `submit` | **409** 同上 |

⚠️ rev 2 修正（N1）：rev 1 写了「`pending → draft`」是非法流转，
但**没有端点能做这个动作**（没有 `withdraw`）→ 不可验，已删除。

**为什么状态机在业务层**：REQ-017 已取消，平台从来没有流程引擎（D347）。

### 1.3 HTTP 端点（4 个）

| 方法 | 路径 | 权限点 | 说明 |
|---|---|---|---|
| `GET` | `/api/leave/requests` | `leave:request:read` | 列表（cursor 分页） |
| `POST` | `/api/leave/requests` | `leave:request:write` | 建单（`draft`） |
| `POST` | `/api/leave/requests/{id}/submit` | `leave:request:write` | `draft → pending` |
| `POST` | `/api/leave/requests/{id}/decide` | `leave:request:approve` | `pending → approved/rejected` |

⚠️ 每个**路由组**必须挂 `route_layer(require_permission("..."))` ——
`platform-org` 曾经整个 crate 零鉴权（QA 实测只读用户 201 Created），
本模块的 C-5 契约测试必须覆盖到，且要做**变异验证**（删一层 → 必须红）。

### 1.4 复用清单（AC-S-188 的靶子，已按挑刺修正）

| 能力 | 复用方式 | **不得**做的事 |
|---|---|---|
| 租户隔离 | `platform_db::TenantDb` + `TenantQuery` | 裸 `sqlx::query` |
| 鉴权 | `require_permission("leave:...")` | 自己解析 JWT |
| 审计 | `AuditEvent` + 平台 `AuditSink` | 出现 `platform_audit_log` / `platform_audit_append` |
| 通知 | **`NotificationSender` trait**（见 §1.6） | 依赖 `platform-notify` |
| 分页 | 平台 cursor 契约 | 自己定信封 |
| 前端表格/表单 | `packages/ui` + `packages/schema` | 自己写渲染 |

⚠️ rev 2 修正（M2）：rev 1 的 grep 模式 `NewDelivery|notify` 有两处错：
`NewDelivery` 在禁止依赖的 crate 里；`notify` 大小写敏感**不匹配** `NotificationSender`。

### 1.5 前端（1 个自定义页面 + 1 条 shell 路由，**C 类改动**）

⚠️ rev 2 修正（M7）：rev 1 说「路由注册走 shell 的既有机制」—— **不存在该机制**。

实测：
- `apps/shell/src/lib/routes.ts:47`：「`/schema/:moduleId/:entityKey` 是**唯一**的
  数据页面路由：新增实体只改后端 manifest，`apps/shell/src/**` 一行不改」；
- 但通配页动作列只有编辑/删除（`SchemaTable.svelte:131-148`），
  manifest 的 `api` 只有 4 个键（`list`/`create`/`update`/`delete`）
  → **没有「审批」这种行内动作**；
- `grep -rn "modules/" apps/shell/src` → **无命中**。

→ 必须：
1. 写 `modules/leave/frontend/pages/LeavePage.svelte`（列表 + 表单 + 审批按钮）；
2. 在 `apps/shell/src/**` 加**一条**硬编码路由指向它（C 类，登记）。

**明确禁止**：用通配表单直接把 `status` 改成 `approved` ——
那绕过了状态机与 409 校验（AC-S-189 的安全判据被 UI 旁路）。

### 1.6 通知落库（D354 的落地）

```text
业务 handler  →  NotificationSender::send(ctx, msg)   （trait 注入，业务不依赖 notify）
                       ↓
              DbNotificationSender（平台新增，D 类改动）
                       ↓
              PlatformNotifyService::enqueue  →  platform_notify_delivery 表
```

- 业务侧只依赖 `platform_core` 的 trait（AC-S-193 成立）；
- 落库由平台适配器完成（AC-S-189 的通知计数可查真表）。

---

## 2. 升级演示设计（AC-S-190，D349 + D352）

### 2.1 升级对象：`GET /api/rbac/me/permissions`

**为什么选它**：消费方前端要用 `can('leave:request:approve')` 决定是否显示审批按钮。

⚠️ rev 2 修正（N4）：rev 1 说 `can()` 在 `packages/sdk` —— **错的**，
实测在 `packages/auth/src/index.ts:387`（`packages/auth/src/index.ts:235-238` 的
`PermissionsResponse` 定义了响应形状）。`packages/sdk` 没有该函数。

```console
# 实现时贴实测输出
$ grep -rn "me/permissions" packages/ --include=*.ts | head -3
```

### 2.2 版本化方式：**路径前缀**（D352）

| 版本 | 路径 | 响应体 |
|---|---|---|
| **v1**（现状，**保持不动**） | `GET /api/rbac/me/permissions` | `{"permissions": ["rbac:role:read", ...]}`（**对象**，恰好一个键） |
| **v2**（新增） | `GET /api/rbac/v2/me/permissions` | `{"permissions": [...], "sources": {"<perm>": ["<role>", ...] }}` |

⚠️ rev 2 修正（B3）：rev 1 写「v1 是裸数组」——**事实错误**。
实测 `crates/platform-rbac/src/http.rs:420-431` 的注释：
「# 为什么是**对象**而不是裸数组 … 改形状是破坏性变更」；
且 `crates/platform-rbac/tests/me_permissions.rs:331` 有已 ratified 的断言：
`assert!(body.is_object(), "响应体必须是对象（不是裸数组）")`。

→ v2 的语义变化改为**新增 `sources` 键**（按来源分组）：
- 对「按开放对象消费」的客户端：**零改动**（忽略未知键）；
- 对「严格穷举键」的客户端：会被打断 → 这正是「破坏性变更需要提前通知」的场景。

**这条变化有验证价值**，因为它能区分「写得脆的客户端」与「写得稳的客户端」。

### 2.3 弃用登记

v1 必须进 `default_deprecations()`（`crates/platform-observability/src/http.rs:388`）：

```rust
// 真实写法（rev 2 修正 N10：rev 1 写的 Deprecation::new(2027-06-30) 不是合法 Rust）
(
    "/api/rbac/me/permissions".to_string(),
    crate::deprecation::Deprecation::new(
        time::macros::datetime!(2027-06-30 00:00:00 UTC),
    ),
)
```

→ 响应带 `Deprecation: true` + `Sunset: <IMF-fixdate>`（D342 的格式，
`deprecation.rs:43-52` 手写而非用 `time` 的 Rfc7231）。

### 2.4 「零改动」的验证

| 实体 | 升级后是否改代码 |
|---|---|
| `modules/leave/**`（消费方业务） | **零改动**（`BASE_UPGRADE` 后 `git diff` 为空） |
| `packages/auth`（平台前端） | **允许改**（负责兼容两版本） |
| `crates/platform-rbac` | **允许改**（升级演示本身，B 类） |

---

## 3. 平台改动清单（AC-S-186 + D353 的判据）

> ⚠️ 实现时**逐项填实**。判定：`git diff --name-only BASE_IMPL | grep -v '^modules/leave/'`
> 的输出必须与**本表逐项相等**（多一个 → A 类不为 0 → AC-S-186 不成立）。

| # | 类 | 文件 | 改什么 | 为什么必需（删掉它哪条 AC 判不了） |
|---|---|---|---|---|
| 1 | **C** | 根 `Cargo.toml` | members 加 `"modules/leave"` | 不加则 crate 不在工作区，编译不过 |
| 2 | **C** | `crates/platform-server/Cargo.toml` | 加 `leave.workspace = true` | 不加则 `leave::mount` 调不到 |
| 3 | **C** | `crates/platform-server/src/main.rs` | `"leave" => {...}` 装配臂 + 认证层 | 不加则 `/api/leave/*` 全 404（B1） |
| 4 | **B** | `crates/platform-rbac/src/http.rs` | v2 端点 + v2 路由 | 没有 v2 就验不了升级（D352） |
| 4b | **B** | `crates/platform-rbac/src/service.rs` | `permission_sources`（v2 的 `sources` 要真算角色来源） | ⚠️ **实现期补登记**：`load_graph` / `user_role_ids` 是 `service` 模块私有，http 层拿不到角色图 → 不改这里只能返回占位 `{}`，「升级」就验不出东西 |
| 5 | **B** | `crates/platform-observability/src/http.rs` | `default_deprecations()` 加 v1 路径 | 没有登记就没有弃用头 |
| 6 | **D** | 通知落库适配器（文件实现时定） | `DbNotificationSender` 实现 `NotificationSender` | 没有它通知落不了库，AC-S-189 判不了（D354） |
| 7 | **D** | `crates/platform-core/src/titles.rs` | 注册 `LEAVE_INVALID_TRANSITION` 的标题 | 不注册则 `title_of_code` 落兜底 `"Error"`，违反契约 C6 |
| 8 | **D** | `crates/platform-core/src/problem.rs` | 该码加入 `ALL_CODES` | 不加则完备性测试红 + 静默走兜底 |
| 9 | **C** | `apps/shell/src/**` | 加一条 `leave` 页面路由 | 不加则 E2E 打不开页面（M7/D356） |
| 8 | **C** | `packages/auth` | 兼容 v2 的 `sources` 键（可选） | 若不改也能跑（忽略未知键），则**不该改** → 不改就删掉本行 |
| ? | — | （实现时按实际补充） | | |

**生成物不算手工改动**：`capabilities/**` 由 `cargo xtask capabilities` 重新生成，
必须跑（否则 `--check` 红），但它不是「人改的代码」—— 在清单里单列一行标注「生成物」。

**契约文档不算平台改动**（M8 的修正）：
`docs/contracts/**` 是**快照**（`docs/contracts/README.md:3,34`：「这里的文件是快照，
不是正本」「**不要手改本目录下的文件** —— 下次同步会被覆盖，CI 会因快照与正本不一致而失败」）。
→ 正本在 BA `demand` 分支的 `BA/contracts/`，改它要走
`scripts/sync-contracts.sh --sync`，且**不计入**平台改动。

---

## 3b. 部署前提（实现期发现，必须写明）

### 通知模板**必须由租户自己种**

实测（批次 3）：`platform_notify_template` 是**租户级表**
（`tenant_id NOT NULL`，唯一键 `(tenant_id, code, channel, locale)`），
`resolve_template` 的 SQL 按 `tenant_id = {ctx.tenant}` 过滤。

→ **迁移里没有租户可写**，写 `'*'` 也永远匹配不上。
REQ-015 本身也**没有任何**模板种子迁移 —— 平台约定是租户通过
`POST /api/notify/templates` 自建。

**影响**：若部署时租户没建 `leave_decision` 模板（in_app，zh-CN + en-US），
审批通知会返回 `NOTIFY_TEMPLATE_NOT_FOUND`。
⚠️ 该失败**只 warn、不回滚流转**（设计如此：通知是旁路）。

→ 部署清单必须包含：「租户初始化时种入 `leave_decision` 模板」。
→ 测试里用 `create_template` 在真表种了两条（计数仍查真表）。

---

## 4. 度量报告结构（AC-S-191）

交付物：`BA/demands/REQ-020-platform-consumption/metrics-report.md`（实现后产出）

```markdown
# REQ-020 度量报告

- BASE_IMPL: <sha>
- BASE_UPGRADE: <sha>
- 被测 commit: <sha>

| 指标 | 值 | 取数命令 |
|---|---|---|
| 后端业务代码 | ? 行 | `find modules/leave/backend/src -name '*.rs' \| xargs wc -l \| tail -1` |
| 前端业务代码 | ? 行 | `find modules/leave/frontend -type f \( -name '*.svelte' -o -name '*.ts' \) \| xargs wc -l \| tail -1` |
| SQL 迁移 | ? 行 | `find modules/leave/migrations -name '*.sql' \| xargs wc -l \| tail -1` |
| 平台改动（全仓） | ? 个 | `git diff --name-only <BASE_IMPL> \| grep -v '^modules/leave/' \| wc -l` |
| 其中 A 类 | ? 个 | 逐条审视清单后计数 |
| 依赖面 | ? | `grep -E '^platform-' modules/leave/Cargo.toml` |
| 契约检查 | 退出码 ? | `cargo xtask check-modules && cargo xtask check-deps && cargo xtask capabilities --check` |
| 全量测试 | ? passed | 逐 crate 循环（见 acceptance AC-S-191） |
| E2E skipped | ? | `npx playwright test e2e/leave.spec.ts --reporter=json \| jq '...'` |

## 超过 500 行时的归因（AC-S-187）

| 文件 | 行数 | 必需样板 | 业务逻辑 |
|---|---|---|---|
| ... | | | |

## 底座缺口清单（本需求的核心产出）

| # | 缺口 | 实测证据 | 影响 | 建议 |
|---|---|---|---|---|
| 1 | 业务模块装配是硬编码 match | `main.rs:553-586` | 每个新模块都要改平台 3 个文件 | TD-086 |
| 2 | 通知无落库适配器 | `NotificationSender` 只有 `send()` | 业务发通知落不了库 | 已补（D354） |
| 3 | manifest schema 无行内动作 | `schema.rs:138-141` 只有 4 个 api 键 | 带审批的业务要写自定义页 + 改 shell | TD-087 |
| 4 | 样板代码偏重 | 骨架 458 行 | 500 行预算结构性紧张 | 见归因表 |
```

---

## 5. 定期任务

**无。** 本需求不引入新的周期任务。

---

## 6. 测试策略

| 层 | 内容 | 关键要求 |
|---|---|---|
| 契约检查 | `#[platform_module_contract]` C-1~C-6 | 必须挂上 + **变异验证**（删一层 route_layer → C-5 红） |
| 单元测试 | 状态机合法/非法流转 | 非法流转断言**具体错误码** `LEAVE_INVALID_TRANSITION` |
| 真库集成 | 租户隔离 + 审计链 + 通知投递 | **应用角色**连接（RLS 才生效）+ 查真表 |
| HTTP 层 | 4 个端点 × 权限点 | **打端点**，不测 service（§9.15） |
| E2E | 列表 → 表单 → 审批 → 刷新 | `e2e/leave.spec.ts` 单文件 `skipped == 0` |
| 升级回归 | v1 可用 + 弃用头 + 零改动 | `BASE_UPGRADE` |

---

## 7. 已知风险（rev 2 已实测）

| # | 风险 | 实测证据 | 缓解 |
|---|---|---|---|
| R1 | **500 行预算结构性不可达** | 骨架已占 **458 行**（411 后端 + 47 前端） | 如实判不成立 + 归因（D357） |
| R2 | 平台改动**必然不为 0** | C 类 5 项 + D 类 1 项 | 四分类登记（D353），A 类必须为 0 |
| R3 | 消费方可能不知不觉依赖内部 crate | `NewDelivery` 在 `platform-notify` | AC-S-193 grep + `check-deps` |
| R4 | C-5 可能扫不到新模块 | C-5 曾只覆盖 `modules/sandbox`（TD-003） | 显式挂契约宏 + 变异验证 |
| R5 | 前端审批动作可能被迫绕过状态机 | 通配页只有编辑/删除 | 禁止用表单改 `status`；必须走 `decide` 端点 |
| R6 | `WorkflowHandler` 仍与需求结论矛盾 | `catalog.rs:79-81` 写「勿自建流程引擎」 | 已记 TD-085；文案冲突需后续处理 |
