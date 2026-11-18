# 任务：定义一个审批流程

## 目标

给业务模块接上一个审批流程：业务侧实现流程处理逻辑，流程引擎由平台提供。

## 输入

- `<模块名>`：业务模块名
- `<流程 key>`：流程定义 key（如 `invoice-approval`，与模块 id 一起保证全局唯一）
- `<状态集合>`：状态列表（如 `draft → pending → approved / rejected`）
- `<流转事件>`：触发流转的动作（如 `submit` / `approve` / `reject`）
- `<业务载荷字段>`：流程实例上要带的业务字段

## 步骤

1. **先读** `AGENTS.md` 与 `capabilities/index.json`：
   - `already_provided.workflow_handler` —— 平台**已经**提供了
     `WorkflowHandler` trait 与 `TransitionEvent` 类型，**不要自己写流程引擎**；
   - `not_yet_provided.workflow_engine` —— 流程定义与实例的落地在 REQ-017，
     本期只做「业务侧接缝」。
2. **建模块或改模块**：新流程属于某个业务模块。新模块用
   `cargo xtask create-module <模块名>`；已有模块直接改。
3. **实现处理器**：`modules/<模块名>/backend/src/` 下实现
   `platform_core::extension::workflow::WorkflowHandler`：
   - `workflow_key()` 返回 `<流程 key>`，必须与平台侧的流程定义 key 一致；
   - `on_transition(&self, ctx: &RequestContext, ev: &TransitionEvent)`
     执行业务动作。**状态变更要落在模块自己的仓储里**，
     走 `Repository`（每个方法都带 `ctx`，租户隔离自动成立）。
4. **声明权限点**：审批 / 驳回 / 提交是独立权限点
   （`<模块名>:<资源名>:approve` 等），在 `manifest.json` 登记 +
   `backend/src/permission.rs` 加同名常量（C-2 会核对）。
5. **写边界**：如果一次流转要写多张表（业务单 + 流程日志），
   必须在夹具的 `write_boundary()` 里**如实声明跨表**并显式走平台事务
   （`TransactionalWrite` / `WriteBoundary`，C-4 会判定）。
6. **写契约测试**：在 `tests/contracts.rs` 的夹具里补一个「流转后状态正确」的用例；
   C-3 仍要求两个租户的同名数据。
7. **更新能力索引与文档**：`cargo xtask capabilities`，
   更新 `modules/<模块名>/AGENTS.md`（把 `WorkflowHandler` 列进「用到的平台扩展点」）。
8. **自验**：
   ```bash
   cargo test -p <模块名> -- --test-threads=2
   cargo xtask check-modules
   cargo xtask capabilities --check
   ```

## 约束

- 不修改 `crates/platform-core/` 与 `crates/platform-contracts/`；
  `WorkflowHandler` 的签名是 `extension-points v1` 冻结的。
- 不自建流程引擎 / 状态机库：平台侧才是引擎（REQ-017）。
- 流程实例的租户归属只能来自 `ctx`。
- 审批动作要有对应的权限点，不要靠「谁点的」隐式判断。
- 时间存 UTC；金额用最小单位整数。

## 验收

- [ ] 实现了 `WorkflowHandler`，`workflow_key()` 与 `<流程 key>` 一致
- [ ] 状态变更走 `Repository`（带 `ctx`），没有手写租户过滤
- [ ] 审批权限点已在 manifest 声明，C-2 无告警
- [ ] 跨表写已在 `write_boundary()` 里如实声明，C-4 通过
- [ ] `cargo test -p <模块名> -- --test-threads=2` 全绿
- [ ] `modules/<模块名>/AGENTS.md` 的「用到的平台扩展点」含 `WorkflowHandler`
