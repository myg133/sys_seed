# 需求：底座可用性验证

- **需求编号**: REQ-020
- **父需求**: [REQ-001 通用项目底座](../REQ-001-platform-foundation/demand.md)
- **优先级**: P0
- **状态**: 草稿
- **所属迭代**: Sprint 7
- **更新时间**: 2026-11-23

## 需求来源（§9.1 / D122 强制披露）

| 项 | 来源 | 说明 |
|---|---|---|
| 父 AC-029（新业务系统只在业务目录写代码） | **用户原话** | REQ-001 的「② 底座可用性」两条之一 |
| 父 AC-030（底座升级不破坏业务） | **用户原话** | 同上 |
| **REQ-020 这个子需求的拆分** | **BA 补** | roadmap 标注「BA 补（验证手段）」 |
| 度量可复现（现 AC-S-191） | **BA 补** | 原 AC 只有「度量指标取数」一句，可复现性要求是 BA 加的 |

⚠️ 引用用户原话时以 `REQ-001/demand.md` 的原文为准，**不引用本文件的转述**。

## 需求描述

用**一个真实业务系统**证明底座达成核心目标：**只写业务，不写通用功能**。

本需求是「证伪」性质的：
若底座不好用（缺扩展点、缺复用能力、处处要改平台），**这里会先暴露**。
因此本需求的产出不是「再写一批平台能力」，而是**一个消费方 + 一份可复算的度量**。

## 前置依赖（实测核对，2026-11-23）

| 需求 | 状态 | 实测依据 |
|---|---|---|
| REQ-002~016 / 018 / 019 / 021 / 022 | ✅ 已合入 `develop` | `git log --merges develop` 逐条可查 |
| **REQ-017 工作流引擎** | ❌ **已取消**（D121，用户裁定「我没说要加这个」） | `BA/demands/REQ-017-workflow/status.md` |

### ⚠️ 取消 REQ-017 带来的直接后果（本需求的核心约束）

本需求原文要求「一个实体 + 列表 + 表单 + **一个审批流**」，父 AC-029 也要求
「用户 / 权限 / 租户 / 通知 / 审计 / **工作流**全部复用」。

但实测：

```console
$ grep -rn "on_transition\|WorkflowHandler" --include=*.rs crates/ modules/ | grep -v workflow.rs
crates/xtask/src/capabilities.rs:50:    "WorkflowHandler",      ← 只是能力索引的字符串
crates/xtask/src/catalog.rs:79-80:  ...                    ← 只是目录条目
crates/platform-core/src/lib.rs:22:  ...                    ← 只是 re-export
crates/platform-core/src/extension/mod.rs:80:  ...          ← 只是 re-export
```

→ **`WorkflowHandler::on_transition` 在平台侧没有任何调用点**。
它是 REQ-017 取消后留下的**孤儿 trait**（只有定义 + 测试里的 `Demo` 实现）。

**因此「复用工作流」在本轮不可达成**（详见 D347 的裁决）：
要做到它，必须**给平台加一处调用链** —— 而那恰好违反父 AC-029 的
「**不修改平台核心任何一行**」。三者不可兼得，用户 2026-11-23 裁定：
**审批流由业务层自实现三态状态机，平台只提供审计 / 通知的横切复用**，
本需求**不声称**复用了工作流引擎。

## 现状基线（实测，2026-11-23，`develop` @ `1be47f5`）

### 已有可复用的面

| 能力 | 消费入口 | 实测 |
|---|---|---|
| 业务模块骨架 | `cargo xtask create-module <name>` | `crates/xtask/src/scaffold.rs`，生成 backend/tests/manifest/frontend 骨架 |
| 扩展点 | `platform_core::extension::*` | `PlatformModule` / `Repository` / `Auditable` / `NotificationSender` / `WorkflowHandler`（孤儿） |
| 租户作用域数据访问 | `platform_db::TenantDb` + `TenantQuery` | `{tenant}` 占位符强制恰好一个（`render()`） |
| 鉴权 | `platform_core::extension::permission::require_permission` | 唯一入口，C-5 扫描 |
| 审计 | `platform_core::extension::audit::AuditEvent` + `PgAuditSink` | 哈希链、append-only |
| 通知 | `platform_notify` 的 `NewDelivery` + `tick()` | 队列 + 重试 + 死信 |
| 前端 SDK / UI / 表单渲染 | `packages/sdk`、`packages/ui`、`packages/schema` | `SchemaForm.svelte` / `SchemaTable.svelte` 已存在 |
| 契约检查 | `#[platform_module_contract]` 宏 | C-1~C-6 |

### 已有消费方样板：`modules/sandbox`

| 度量 | 实测值 |
|---|---|
| backend | 2223 行 |
| tests | 1623 行 |
| migrations | 155 行 |
| frontend | **19 行**（单个 `DemoPage.svelte`） |

⚠️ **它是「扩展点示例」，不是「业务消费方」**：
- `README.md` 明写「配套类型（`Auditable` / `NotificationSender` / `WorkflowHandler`）
  本期只在平台侧定义，消费方分别落在 REQ-015 / **REQ-017**」；
- 前端只有 19 行 —— **没有列表、没有表单、没有审批流**；
- backend 2223 行里大量是「演示多个扩展点」的内容，不是「最小业务」。

→ **本需求需要新建一个真正的消费方**，不能拿 sandbox 充数。

### 版本化现状

```console
$ ls docs/contracts/api-conventions/
v1.md  v2.md
$ grep -n "default_deprecations" -A 5 crates/platform-observability/src/http.rs
388: pub fn default_deprecations() -> ... {
389:     vec![("/api/audit/events", crate::deprecation::Deprecation::new(
391-394:      time::macros::datetime!(2027-06-30 00:00:00 UTC)))]
```

→ **弃用头机制已就绪**（REQ-019 D342），但**只登记了 1 条路径**，
且**从未真正演练过一次「升级」**（没有 v2 端点、没有消费方迁移）。

## 功能要求

1. **新建一个最小业务系统作为消费方**：一个实体 + 列表 + 表单 + 一个审批流；
2. **统计业务自研代码量与目录结构**（口径见 AC-S-187）；
3. **平台同大版本升级，验证业务零改动**（v1→v2 + 弃用头，见 AC-S-190）；
4. **度量指标取数**：只保留可机械复现的指标（AC-S-191）。

## 裁决摘要（详见 [decisions.md](decisions.md)）

| 决策 | 内容 |
|---|---|
| **D347** | 审批流 = **业务层自实现三态状态机**；平台只提供审计 + 通知横切；**不声称复用工作流引擎**；`WorkflowHandler` 保持孤儿状态并记 TD |
| **D348** | 消费方选型 = **`modules/leave`（请假审批）**；理由见决策 |
| **D349** | 「升级零改动」= 造一次**真实 v1→v2**（平台侧），消费方**业务目录零改动** |
| **D350** | 度量指标 = **只留可机械复现的**（行数 / 目录 / 依赖 / 命令），剔除「上手时间」「AI 一次过率」 |
| **D351** | 「不改平台」的**精确口径**（rev 2 改为四分类，见 D353） |
| **D352** | API 版本化 = **路径前缀** `/api/<module>/v2/...`（契约里无任何版本化约定，须新定） |
| **D353** | 平台改动**四分类** A/B/C/D，只有 A 类必须为 0 |
| **D354** | 通知落库缺口 → 平台补 `DbNotificationSender`（D 类改动） |
| **D355** | 业务模块装配点缺口（硬编码 match）→ C 类 + **TD-086** |
| **D356** | 前端动作缺口（manifest 无行内动作）→ 自定义页 + shell 路由 + **TD-087** |
| **D357** | 500 行预算：**如实实测 + 归因，不改口径**（骨架已占 458 行） |

## 不在范围内

- 消费方自身的业务功能（仅用于验证，不属于底座）
- 真正的流程引擎（REQ-017 已取消，不在本轮复活）
- 性能/容量压测（属其他需求）
- 前端视觉设计（只验「能渲染出列表与表单」，不验美观）

## 完成定义

- 业务代码 **< 500 行**（口径见 AC-S-187）
- 核心改动 **0 次**（消费方开发过程，见 AC-S-186）
- 升级**零改动**（消费方业务目录，见 AC-S-190）
- **度量报告交付**且换人可复算（AC-S-191）

## 关联验收标准

父需求 AC：AC-029、AC-030

本需求自有 AC：**AC-S-186 ~ AC-S-193**（8 条，见 [acceptance.md](acceptance.md)）
