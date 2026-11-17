# G. 二次开发体验与 AI 辅助开发 —— 测试用例（AC-031 ~ AC-039）

- **需求**: REQ-001 通用项目底座（Base Platform）
- **来源**: [acceptance.md](../acceptance.md) §G、[design-summary.md](../design-summary.md)、[ADR-0001](../../decisions/ADR-0001-底座技术架构基线.md) D7 / D8
- **用例数**: 19（TC-G-001 ~ TC-G-019）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## G.0 本域约定

- 目录（沿用 design-summary.md §1）：`crates/platform-core/`（平台核心，**禁止修改**）、`crates/platform-contracts/`（契约测试套件）、`modules/<name>/`（业务模块）、`prompts/`（提示词库）、`capabilities/`（能力索引，**自动生成，禁止手改**）。
- 模块目录标准结构：`manifest.json` + `backend/{mod.rs,entity.rs,repository.rs,service.rs,permission.rs}` + `frontend/{routes.ts,pages/,components/}` + `migrations/` + `tests/contracts.rs` + `AGENTS.md` + `README.md`。
- 契约测试接入：`use platform_contracts::prelude::*;` + `#[platform_module]`（design-summary.md §2）。
- 能力索引结构（design-summary.md §3）：`capabilities/index.json`（含 `already_provided`）、`permissions.json`、`extension-points.json`、`schema/metadata.schema.json`、`sdk/{ui,sdk,auth,schema}.json`、`api.json`。
- 命令：`cargo xtask create-module {name}`、`cargo xtask capabilities`、`bun run dev`、`cargo test --workspace`、`bun run test:e2e -- {module}`。
- `manifest.json` 字段：`id` / `name` / `version` / `enabled` / `permissions[]` / `menus[]` / `dependencies[]` / `config{}`。

---

### TC-G-001: 排除全部业务模块后平台核心仍可独立编译
- **关联 AC**: AC-031
- **前置条件**:
  - 仓库中已存在至少两个业务模块 `modules/order/` 与 `modules/invoice/`（或 `sandbox`），且平台核心不引用它们。
  - 具备能单独构建平台核心的构建配置（如 workspace 成员排除或独立 cargo 项目）。
- **测试步骤**:
  1. 执行只包含 `crates/platform-core/` 与 `crates/platform-contracts/` 的构建（排除全部 `modules/*`）。
  2. 执行 `cargo build -p platform-core -p platform-contracts`（或等价命令）。
  3. 记录退出码与编译产物。
- **预期结果**:
  1. 两次构建均 exit code 0，无编译错误。
  2. 产物中不含任何业务模块的 crate / 模块名（如 `order` / `invoice` / `sandbox`）；`cargo tree -p platform-core` 输出中无业务模块依赖。
  3. 平台核心在无任何业务模块的情况下仍能启动（`bun run dev` 起得来，健康检查通过）。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 依赖方向断言

### TC-G-002: 平台核心依赖图中不出现任何业务模块
- **关联 AC**: AC-031
- **前置条件**:
  - 业务模块 `modules/order/` 已实现平台扩展点 trait 并完成编译期注册。
- **测试步骤**:
  1. 执行 `cargo tree -p platform-core --edges normal,dev`。
  2. 在输出中检索每个业务模块名（`order` / `invoice` / `sandbox`）。
  3. 执行反向检查 `cargo tree -p order --edges normal`，确认业务模块依赖指向平台扩展点 crate 而非平台内部实现。
- **预期结果**:
  1. 平台核心依赖图中业务模块名出现次数 = 0。
  2. 依赖方向恒为 `平台 → 扩展点(trait) ← 业务`：业务模块依赖 `platform-contracts` / 扩展点定义，而平台核心不依赖任何 `modules/*`。
  3. 平台核心不得通过 `cfg` 特性或可选依赖间接拉入业务模块（依赖图与特性图均无业务项）。
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 依赖图静态断言

### TC-G-003: 关闭 order 模块后不加载、菜单不显示、接口报模块未启用
- **关联 AC**: AC-032
- **前置条件**:
  - 平台装载 `order` 与 `invoice` 两个模块，两者 `manifest.json` 的 `enabled` 初始均为 `true`。
  - 账号 `owner@example.com` 是 T1 管理员。
- **测试步骤**:
  1. 打开平台菜单，确认存在「订单」入口。
  2. 管理员把 `modules/order/manifest.json` 的 `enabled` 改为 `false`（或通过模块管理开关关闭），重启平台。
  3. 打开菜单并请求 `GET /api/order/invoice`。
- **预期结果**:
  1. 重启后菜单中**不出现**「订单」入口；`invoice` 相关菜单正常存在。
  2. `GET /api/order/invoice` 返回 HTTP 404 或 403，`code` 明确为「模块未启用」（如 `module_disabled`），**不得**返回 500，也不得返回正常业务数据。
  3. 启动日志中出现 `order` 模块被跳过/未加载的记录，且未注册其路由与权限点（`GET /api/rbac/permissions` 中不含 `order:*`）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（启用 → 禁用）+ 异常路径

### TC-G-004: 关闭 order 不影响 invoice 模块
- **关联 AC**: AC-032
- **前置条件**:
  - 同 TC-G-003 的最终状态（`order` 已禁用）。
  - `modules/invoice/manifest.json` 声明权限点 `invoice:record:read`，`modules/order/manifest.json` 声明 `order:invoice:read`。
- **测试步骤**:
  1. 请求 `GET /api/invoice/record` 与其前端页面。
  2. 检查权限点注册表与菜单。
  3. 检查 `invoice` 模块自己的契约测试与单元测试。
- **预期结果**:
  1. `invoice` 接口 HTTP 200 且返回自身数据，页面正常渲染。
  2. 权限点注册表中含 `invoice:record:read`、**不含** `order:invoice:read`；菜单含「发票」不含「订单」。
  3. `cargo test -p invoice` exit code 0，与 `order` 的启停状态无关。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 隔离性断言

### TC-G-005: 漏掉租户上下文的模块实现被契约测试拦截
- **关联 AC**: AC-033
- **前置条件**:
  - 平台已分发 `platform-contracts` crate；`modules/order/tests/contracts.rs` 已按 design-summary.md §2 接入 `#[platform_module]`。
  - 准备一个**故意错误**的模块实现 `modules/badmodule/`：其 `repository.rs` 直接用裸 SQL 查询 `order_invoice` 表，**不**经过平台租户上下文（漏 `tenant_id` 过滤）。
- **测试步骤**:
  1. 将 `badmodule` 接入契约测试并执行 `cargo test -p badmodule --test contracts`。
  2. 记录失败输出。
  3. 打开 `modules/badmodule/tests/contracts.rs` 与 CI 日志。
- **预期结果**:
  1. 契约测试 **失败**（exit code ≠ 0），且失败信息**明确指出缺失项**为「租户上下文 / tenant_id 过滤」。
  2. 错误信息中指出具体违规位置（文件与行号或具体 trait 方法名），而不只是泛化的「测试失败」。
  3. 该失败在 CI 的契约检查阶段同样复现（CI 与本地用同一套套件），不允许被跳过或标记为可忽略。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 故障注入（故意违规）+ 攻击面枚举

### TC-G-006: 补齐租户上下文后契约测试通过
- **关联 AC**: AC-033
- **前置条件**:
  - 同 TC-G-005，`badmodule` 当前处于「漏租户上下文」状态。
- **测试步骤**:
  1. 把 `badmodule/backend/repository.rs` 改为通过平台租户上下文执行查询（使用平台提供的上下文注入 API）。
  2. 再次执行 `cargo test -p badmodule --test contracts`。
  3. 额外执行一次跨租户请求验证（用 T1 令牌查询，断言不返回 T2 数据）。
- **预期结果**:
  1. 契约测试 exit code 0，全部用例通过。
  2. 跨租验证返回的数据不含 T2 记录（租户隔离在契约之外也被运行时验证）。
  3. 补齐实现后**不需要**修改平台核心或契约套件本身（对比 `git diff --stat`，`crates/` 下改动为 0）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 修复路径回归

### TC-G-007: 权限点未在 manifest 登记 / trait 方法未实现齐全均被拦截
- **关联 AC**: AC-033
- **前置条件**:
  - 准备模块 `modules/halfdone/`：`backend/permission.rs` 中使用了权限点 `halfdone:item:approve`，但 `manifest.json` 的 `permissions[]` **未**声明它；且某个 trait 方法用默认空实现代替（未实现完整）。
- **测试步骤**:
  1. 执行 `cargo test -p halfdone --test contracts`。
  2. 记录失败项清单。
- **预期结果**:
  1. 契约测试失败，且失败项**分别**指出：
     (a) 权限点 `halfdone:item:approve` 未在 `manifest.json` 的 `permissions[]` 中登记；
     (b) trait `XxxRepository`（或对应 trait）的必需方法未实现。
  2. 错误信息可定位到具体权限点编码与具体 trait 名称。
  3. 两类问题可分别识别（补齐其中一类后，剩余一类仍报错），不得只报笼统的「契约不满足」。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 故障注入 + 等价类（两类违规）

### TC-G-008: 平台小版本升级后全部业务模块契约测试通过
- **关联 AC**: AC-034
- **前置条件**:
  - 已存在 2 个实现扩展点的业务模块 `modules/order/` 与 `modules/invoice/`，二者当前契约测试全绿。
  - 平台有可升级的小版本（如 `0.1.0` → `0.1.1`，不破坏扩展点签名）。
- **测试步骤**:
  1. 记录两个模块的契约测试基线（均通过）。
  2. 将平台升级到小版本 `0.1.1`。
  3. 执行 `cargo test --workspace`（含两个模块的 `tests/contracts.rs`）。
- **预期结果**:
  1. 升级后 `cargo test --workspace` exit code 0，两个模块的契约测试**全部通过**。
  2. 没有任何模块出现编译错误或 trait 方法签名不匹配。
  3. 升级过程中扩展点的向后兼容承诺被兑现：若确有签名变更，必须走大版本（并按 AC-030 走弃用流程），小版本不得变更。
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 版本兼容回归

### TC-G-009: 平台小版本升级过程业务模块代码零改动
- **关联 AC**: AC-034
- **前置条件**:
  - 同 TC-G-008；升级前后可取 `git diff`。
- **测试步骤**:
  1. 执行平台小版本升级。
  2. 执行 `git diff --stat` 与 `git status --porcelain`，限定到 `modules/` 路径。
  3. 检查业务模块的依赖声明（`Cargo.toml`）是否被迫改动。
- **预期结果**:
  1. `modules/` 目录下改动文件数 = 0；`git diff --stat -- modules/` 输出为空。
  2. 业务模块 `Cargo.toml` 中的平台依赖版本约束**不需要**修改即可通过编译（约束应允许小版本范围，如 `0.1`）。
  3. 业务模块无需任何人工修补即可构建与测试通过。
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 零改动断言

### TC-G-010: 引导文件体系校验通过
- **关联 AC**: AC-035
- **前置条件**:
  - 仓库含根 `AGENTS.md`、`prompts/`（含 `01-new-crud-module.md` 等 8 份）、`capabilities/`、各模块 `AGENTS.md`（如 `modules/order/AGENTS.md`）。
  - `capabilities/` 由 `cargo xtask capabilities` 生成且与当前源码一致。
- **测试步骤**:
  1. 执行 CI 的引导文件校验任务（能力索引 vs manifest 汇总、扩展点签名 vs 实际 trait、目录结构合规）。
  2. 执行 `cargo xtask capabilities` 后用 `git diff --stat` 检查是否产生变更。
  3. 抽查 `capabilities/permissions.json` 与所有 `modules/*/manifest.json` 的权限点并集。
- **预期结果**:
  1. 校验任务 exit code 0，报告「索引与源码一致」。
  2. 重新生成能力索引后 `git diff` 为空（索引与源码同步，索引未过期）。
  3. `permissions.json` 中的权限点集合 = 各模块 manifest `permissions[].code` 的并集（完全一致，无多无少）；`extension-points.json` 中的 trait 签名与实际 trait 定义逐字段一致。
  4. `modules/order/AGENTS.md` 存在且其「本模块权限点」与 manifest 声明一致。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 一致性断言（索引 vs 事实源）

### TC-G-011: 人为删除或篡改能力索引后校验失败
- **关联 AC**: AC-035
- **前置条件**:
  - 同 TC-G-010 的基线状态（校验通过）。
  - 具备修改 `capabilities/` 内容的权限（用于负向测试，测完必须还原）。
- **测试步骤**:
  1. 从 `capabilities/permissions.json` 中删除一条权限点（如 `order:invoice:read`），保存后执行校验任务。
  2. 恢复后，篡改 `capabilities/extension-points.json` 中某个 trait 的方法签名（改参数类型），执行校验任务。
  3. 记录两次失败输出。
- **预期结果**:
  1. 第 1 步校验**失败**（exit code ≠ 0），错误信息明确指出「权限点索引与 manifest 汇总不一致」并点名缺失的权限点编码。
  2. 第 2 步校验**失败**，错误信息指出「扩展点签名与实际 trait 不一致」并定位到具体 trait 与方法。
  3. 两次失败均阻断 CI（不得为 WARN 继续放行），且失败信息可读、非泛化报错。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 故障注入（负向）+ 边界

### TC-G-012: 提示词库产出合规模块并直接通过 CI
- **关联 AC**: AC-036
- **前置条件**:
  - `prompts/01-new-crud-module.md` 存在且内容完整（目标、上下文、约束、验收齐全）。
  - 具备可用的 AI 编码助手环境，且该助手能读取仓库根 `AGENTS.md` 与 `capabilities/index.json`。
  - 使用一个干净的工作区副本（避免历史代码干扰）。
- **测试步骤**:
  1. 将 `prompts/01-new-crud-module.md` 的内容**原样**（不改写、不补充）交给 AI 助手，要求生成一个 CRUD 业务模块 `aicrm`（输入字段 `{name}` / `{description}` / `{fields}` 填为示例实体 "Customer"）。
  2. 检查产出的目录结构与 `manifest.json`。
  3. 执行 `cargo test --workspace`、契约测试与 `bun run lint`（即 CI 同一套检查）。
- **预期结果**:
  1. 产出目录为 `modules/aicrm/`，含 `manifest.json` + `backend/` + `frontend/` + `migrations/` + `tests/contracts.rs` + `AGENTS.md` + `README.md`，与 design-summary.md §1 模板一致。
  2. `manifest.json` 声明了权限点（如 `aicrm:customer:read` / `aicrm:customer:write`）与菜单，且权限点编码符合 `module:resource:action`。
  3. CI 全部通过（exit code 0），**无需人工重排目录结构**；契约测试一次性通过。
  4. 产出代码未重复实现分页/表格/权限判定（引用 `@base/schema` / `@base/ui` / `@base/auth`）。
- **类型**: 功能 / 可维护性
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 端到端链路（提示词 → 产出 → CI）

### TC-G-013: create-module 生成标准目录与 manifest / AGENTS.md / 契约测试接入
- **关联 AC**: AC-037
- **前置条件**:
  - 已安装脚手架（`cargo xtask` 可用）；当前无 `modules/gift/` 目录。
- **测试步骤**:
  1. 执行 `cargo xtask create-module gift`，按交互提示填写模块名 `gift` 与权限点前缀 `gift`（生成权限点前缀 `gift:*`）。
  2. 检查生成的目录树与关键文件内容。
  3. 检查 `manifest.json` 的字段与 `modules/gift/tests/contracts.rs` 的接入内容。
- **预期结果**:
  1. 退出码 0，生成目录 `modules/gift/` 且结构为标准结构（`manifest.json` / `backend/{mod.rs,entity.rs,repository.rs,service.rs,permission.rs}` / `frontend/{routes.ts,pages/}` / `migrations/` / `tests/contracts.rs` / `AGENTS.md` / `README.md`）。
  2. `manifest.json` 为合法 JSON，字段含 `id="gift"` / `name` / `version` / `enabled` / `permissions[]`（权限点带 `gift:` 前缀）/ `menus[]` / `dependencies[]` / `config{}`。
  3. `AGENTS.md` 骨架包含「领域模型 / 用到的平台扩展点 / 本模块权限点 / 如何验证 / 注意」等段落，占位符为待填写标记。
  4. `tests/contracts.rs` 已含 `use platform_contracts::prelude::*;` 与 `#[platform_module]` 接入骨架。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 产物结构断言

### TC-G-014: 脚手架生成的模块立即可编译运行
- **关联 AC**: AC-037
- **前置条件**:
  - 刚执行完 TC-G-013，模块为脚手架生成状态（业务代码为骨架占位）。
- **测试步骤**:
  1. 执行 `cargo build --workspace`。
  2. 执行 `cargo test --workspace`。
  3. 启动平台（`bun run dev`），访问模块声明的页面/接口路径。
- **预期结果**:
  1. 编译 exit code 0（骨架本身可编译，不需先手工修任何文件）。
  2. 测试 exit code 0（契约测试对骨架状态给出明确「未实现」提示或跳过，但**不得**因脚手架缺失导致编译失败）。
  3. 平台可启动并正常响应健康检查；模块的占位接口返回明确的「未实现」语义响应（如 501 或带提示的业务响应），而不是 500 崩溃。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径 + 开箱即用断言

### TC-G-015: AI 实现订单列表时复用 @base/ui 表格与 @base/schema 渲染器
- **关联 AC**: AC-038
- **前置条件**:
  - `capabilities/index.json` 的 `already_provided` 中明确列出：`pagination`（`@base/ui` Table 内置分页）、`permission_check`（`@base/auth` 的 `can()`）、`crud_form`（`@base/schema` 渲染器）、`audit`、`tenant_filter`。
  - 具备可用的 AI 编码助手环境，工作区为干净副本。
- **测试步骤**:
  1. 向 AI 提出任务：「实现带分页、排序、权限控制的订单列表」。
  2. 检查产出的前端代码中的组件 import 与表格/分页实现方式。
  3. 检查产出代码中是否存在自研的分页逻辑、表格组件或权限判断分支。
- **预期结果**:
  1. 产出代码 import 了 `@base/ui` 的表格组件（如 `DataTable` / `Table`）与 `@base/schema` 的渲染器，未手写 `<table>` + 自研分页控件。
  2. 权限判定通过 `@base/auth` 的 `can()` 完成，代码中**不存在** `if (role === 'admin')` 之类的角色名硬编码分支。
  3. 产出文件数与行数合理（不出现成百上千行的自研表格实现），并能通过 `bun run lint` 与类型检查。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 复用性断言 + 禁用模式扫描

### TC-G-016: AI 产出未重复实现分页 / 表格 / 权限判定
- **关联 AC**: AC-038
- **前置条件**:
  - 同 TC-G-015 的产出。
- **测试步骤**:
  1. 在 AI 产出目录中检索自研分页关键词（如 `pageSize` 状态自维护、`slice(` 分页、`上一页`/`下一页` 自绘按钮）。
  2. 检索自研表格关键词（如原生 `<table` 标签、直接渲染行数组）。
  3. 检索自研权限判定关键词（如 `role ===`、`permissions.includes` 的手写判断、角色名常量表）。
  4. 运行产出模块的契约测试与 lint。
- **预期结果**:
  1. 步骤 1 命中数为 0（分页由 `@base/ui` Table 内置提供，或由 `@base/hooks` 的分页 hook 提供，二者均可，但不得自研一套）。
  2. 步骤 2 命中数为 0；步骤 3 命中数为 0。
  3. 契约测试与 lint 通过，产出可直接合并。
  4. 若发现任一自研实现，视为本用例**失败**（这正是 AC-038 要防止的失败模式），须在报告中标为缺陷。
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 禁用模式扫描（负向断言）

### TC-G-017: 能力索引缺失或过期时 CI 拦截 AI 产出
- **关联 AC**: AC-038
- **前置条件**:
  - 能力索引可被删除或篡改（测试副本）；`modules/order/manifest.json` 新增一个权限点 `order:invoice:void`，但未重新生成 `capabilities/permissions.json`。
- **测试步骤**:
  1. 在副本中删除 `capabilities/index.json`，执行 CI 的引导文件校验任务。
  2. 恢复文件后，执行步骤 1 中「manifest 新增权限点但索引未更新」的场景，再次执行校验任务。
  3. 检查 AI 助手在索引缺失时的行为（读不到能力索引时是否被引导去查证 / 是否被 CI 拦截）。
- **预期结果**:
  1. 索引缺失时校验**失败**（exit code ≠ 0），错误指出缺失的 `capabilities/index.json`。
  2. 索引过期（权限点不一致）时校验**失败**，并点名缺失的 `order:invoice:void`。
  3. 两种情况下 CI 均阻断，不存在「索引缺失就跳过校验」或降级为 WARN 的路径；AI 产出必须过同一套 CI（与人工代码相同）。
- **类型**: 可维护性
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 故障注入（负向护栏）

### TC-G-018: 一条命令起全栈
- **关联 AC**: AC-039
- **前置条件**:
  - 一份**全新克隆**的仓库（`git clone` 到空目录，无本地缓存、无已建 Docker 镜像、无 `.env`）。
  - 本机具备 Docker / Compose（底座要求业务方无需自行安装 Rust toolchain / pnpm / PostgreSQL）。
- **测试步骤**:
  1. 在新克隆目录执行约定的启动命令 `bun run dev`（或 `docker compose up` + `bun run dev` 的组合入口，二选一以实现为准）。
  2. 等待启动完成，记录耗时与过程输出。
  3. 检查前端、后端、PostgreSQL、Redis 四者的可达性：前端首页、API 健康检查 `GET /health`、数据库连接、Redis 连接。
- **预期结果**:
  1. 退出码 0，全栈启动完成，过程输出中无需要人工执行的初始化脚本提示。
  2. 四项服务全部就绪：前端页面可访问；`GET /health` 返回 200；数据库可查询到迁移后的表；Redis 可 `PING` 返回 `PONG`。
  3. 启动过程中自动完成数据库迁移与种子数据装载，**无需**手工执行任何 SQL 或初始化脚本。
  4. 从命令执行到全栈就绪的耗时在可接受范围内（记录实测值；若已定义 SLA 则以 SLA 为准，否则仅记录）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 端到端链路（干净环境）

### TC-G-019: 种子数据就绪且可重置
- **关联 AC**: AC-039
- **前置条件**:
  - 已按 TC-G-018 启动全栈；种子数据定义包含平台管理员 `owner@example.com`、示例租户 T1/T2、示例角色与示例业务数据。
- **测试步骤**:
  1. 登录 `owner@example.com`，确认租户、角色、权限点、示例业务数据均已就绪。
  2. 在种子数据上做一次破坏性修改（新建一条业务数据、改一个角色权限）。
  3. 执行重置命令（如 `bun run db:reset` 或 `bun run seed --reset`），再次登录查看数据。
- **预期结果**:
  1. 种子数据可列举且齐全：至少含 2 个租户、3 个角色、示例订单数据、`order:invoice:*` 权限点；关键实体 id 与夹具文档一致。
  2. 重置命令执行成功（exit code 0），且耗时可接受。
  3. 重置后数据回到初始种子状态：步骤 2 新建的业务数据消失，被改的权限恢复原值；种子数据 id 保持稳定（重置是幂等的）。
  4. 重置过程不需手工 SQL，且不破坏平台核心迁移结果（迁移仍处于已应用状态）。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 幂等性 + 数据一致性
