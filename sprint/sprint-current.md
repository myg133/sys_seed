# 当前迭代计划

- **迭代编号**: Sprint 1 —— 地基
- **时间范围**: 待定（待确认人力与开工时间）
- **父需求**: [REQ-001 通用项目底座](../demands/REQ-001-platform-foundation/demand.md)
- **路线图**: [roadmap.md](../plan/roadmap.md)
- **更新时间**: 2026-11-17 23:45

## 迭代目标

> 打通「二次开发友好」这条主线：**用 `create-module` 生成一个业务模块 → AI 按提示词补全代码 → 契约测试通过 → 平台核心零改动 → 能部署起来。**

这条链路跑通后，后续 18 个子需求都是在稳定的接缝上填内容。

## 需求列表

| 需求编号 | 名称 | 优先级 | 状态 | 负责人 | Worktree |
|---------|------|--------|------|--------|----------|
| REQ-002 | 平台骨架与工程基线 | P0 | 已就绪 | 待认领 | — |
| REQ-003 | 扩展点框架与契约测试 | P0 | 已就绪 | 待认领 | — |
| REQ-004 | AI 引导体系与能力索引 | P0 | 已就绪 | 待认领 | — |
| REQ-005 | 交付基线与发布 | P1 | 已就绪 | 待认领 | — |

**承接父 AC**：AC-031~AC-039、AC-009 的基础设施部分（租户上下文机制的桩）。

## 进度

- 总需求: 4 | 已完成: 0 | 进行中: 0 | 待开始: 4

## 依赖顺序

```text
REQ-002 平台骨架
   ├─→ REQ-003 扩展点框架 ──→ REQ-004 AI 引导体系
   └─→ REQ-005 交付基线
```

- **REQ-002 必须先做**，其余三个都依赖它
- REQ-003 与 REQ-005 可并行
- REQ-004 依赖 REQ-003（能力索引要收录扩展点签名）

## 迭代出口标准

全部满足才算 Sprint 1 结束：

- [ ] `bun run dev` 一条命令起全栈（前端 + 后端 + PostgreSQL + Redis + 种子数据）
- [ ] `cargo build --workspace` 通过，CI 五个 job 全绿
- [ ] `cargo xtask create-module demo` 生成的模块立即可编译运行
- [ ] 故意漏掉租户上下文的模块实现，被 `platform-contracts` 契约测试拦截
- [ ] `cargo xtask capabilities` 自动生成能力索引；手改索引后 CI 失败
- [ ] 仓库级 AGENTS.md + 模块级 AGENTS.md + `prompts/` 齐备
- [ ] 用 `prompts/01-new-crud-module.md` 生成一个模块，**一次通过 CI**
- [ ] `docker compose up` 可起完整环境；Helm 可在测试集群部署
- [ ] 数据库迁移与回滚演练通过

## 风险

| # | 风险 | 应对 |
|---|------|------|
| R1 | REQ-003 的首批扩展点签名定不下来，后续全部返工 | Sprint 1 内先只定 5 个核心 trait（Repository / Service / Auditable / NotificationSender / WorkflowHandler），够用即可；签名冻结后走版本号 |
| R2 | REQ-004 的能力索引做成手工维护，必然过期 | 索引必须由 `xtask capabilities` 从 manifest + trait + OpenAPI 生成，CI 校验一致性 |
| R3 | 私有化部署与 K8s 环境不具备 | 降级为 compose + 基础 Helm 模板，K8s 实测放到验收阶段 |

## 下一步

1. 派 Dev 认领 REQ-002（`claim next`），其余三个按依赖顺序认领
2. QA 依据各子需求的 `acceptance.md` 细化用例（父需求用例已生成，见 `REQ-001-platform-foundation/test-cases/`）
3. 每个子需求派单时附上对应的父 AC 编号与 ADR 条款，便于追溯
