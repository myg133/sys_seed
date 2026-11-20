# 状态：REQ-014 审计日志

- **状态**: 进行中
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-20 09:40

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-20 09:30 | 草稿 | 已就绪 | myg133-ba-1 | 规格细化：13 条 AC（AC-S-022 + AC-S-122~133）+ D130~D144 + 设计摘要；依赖 REQ-002/006 已完成 |
| 2026-11-20 09:40 | 已就绪 | 进行中 | myg133-ba-1 | BA 认领 REQ-014 → 建 `feature/REQ-014`（基于 develop@0683115）；QA 挑刺 + 用例进行中 |
| 2026-11-20 10:10 | 进行中 | 进行中（spec rev 3） | myg133-ba-1 | QA 挑刺 **36 条（12 阻塞）→ 13 条 AC 全部有问题**；BA **36 条全接受** → 裁决 D145~D168；AC rev 3 + design-summary rev 2 |
| 2026-11-20 10:35 | 进行中（spec rev 3） | 进行中（棒 1 完成） | myg133-ba-1 | **棒 1 完成**：前端（`Audit.svelte` + `audit-api.ts` + 37 单测 + 5 E2E，vitest 158→195）+ 后端（`platform-core` 审计类型 + 新 crate `platform-audit` 8 模块 + 3 张表 + 4 个迁移 + 种子脚本例外块） |
| 2026-11-20 11:05 | 进行中（棒 1 完成） | 进行中（棒 2 进行中） | myg133-ba-1 | 裁决 Dev-A 的 **9 条偏离** → **D171~D178**（含 D171 补 `tip_hash` 列、D172 敏感键驼峰拆词）；**实测校正** Dev 的测试数字表（`platform-core` 143→**150**、`platform-iam` 99→**168**）；棒 2 分两个并行 Dev（后端接入 / 前端收尾） |
| 2026-11-20 11:35 | 进行中（棒 2 进行中） | 进行中（棒 2 前端完成） | myg133-ba-1 | **棒 2 前端完成**：D169 游标取数 + 游标栈页码指示、5 个错误码 i18n（**断言一字未动**，只改 `codes` 数据列表）、E2E 夹具（`u-perm-audit` + 恒 11 条审计记录，链自洽 11/0 断裂）。`@base/shell` **206**（+11）、i18n 43、sdk 38、E2E **30 passed / 5 skipped / 0 failed**（skip 理由全为「被测系统未就绪」）。裁决 **D179**（采纳后端「游标+排序」400 收紧 = 契约 C7 漏写）+ **D180**（未注册 `/api/*` 必须 404 而非 SPA 回落，已追加给后端 Dev） |

## 当前实测基线（BA 亲测，勿用 Dev 的数字表）

| crate | passed | failed | ignored |
|---|---|---|---|
| `platform-core` | **150** | 0 | 0 |
| `platform-iam` | **168** | 0 | 0 |
| `platform-audit`（新） | **44** | 0 | 0 |

> 统计口径：`grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8}'`
> （Dev-A 报 143 / 99 / 44 —— **前两个偏低**，其报告的「新增测试（按文件）」表
> 路径写错且数字不自洽。`platform-iam` 168 与 REQ-013 基线一致 → **没丢用例**。）

## 棒 2 的任务分解（两个并行 Dev，文件集不相交）

| Dev | 文件集 | 内容 |
|---|---|---|
| **B1 后端** | `crates/**`、`migrations/**` | D171 `tip_hash` 列 + 截断检出；D172 驼峰拆词（两侧）；业务模块埋点（iam/rbac/config/org）；sink 装配；`/api/audit` 路由；全局中间件填 ip/UA；xtask 子命令；D178 四个配置项登记 |
| **B2 前端** | `apps/shell/**`、`packages/i18n/**`、`scripts/**`、`e2e/**` | D169 游标取数 + 游标栈页码指示；D170-1 五个错误码 i18n；D170-2 E2E 夹具（`u-perm-audit` + ≥11 条审计记录） |

## 规格摘要

- **需求来源**：用户原话「对，审计和国际化也是需要的。」（见 [decisions.md](decisions.md) §0）
- **验收标准**：13 条（[acceptance.md](acceptance.md)）
- **设计依据**：[design-summary.md](design-summary.md)（含全部 `file:line` 实测证据）
- **决策**：D130~D144（[decisions.md](decisions.md)）

## 阻塞项

**无。** 前置 REQ-002 / REQ-006 均已完成；软依赖 REQ-007 / REQ-009 亦已完成。

## 下一步

1. QA 挑刺验收标准 + 写风险用例（**必须先于 Dev**）→ `test-cases/`
2. BA 认领 REQ-014 → 建 `feature-REQ-014` worktree
3. 派 Dev 实现（按 design-summary 的 3 棒切片）
