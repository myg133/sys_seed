# 状态：REQ-004 AI 引导体系与能力索引

- **状态**: 已完成
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-18 09:30
- **分支**: `develop` @ `37a93c5`（已合并）

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 已就绪 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-18 03:10 | 已就绪 | 进行中 | dev | Dev 交回：37 文件 / +4563 行，Rust 98/98，零新依赖 |
| 2026-11-18 03:25 | 进行中 | 待验证 | ba | 派 QA Pre-merge |
| 2026-11-18 03:40 | 待验证 | 已退回 | ba | QA 结论 PASS-WITH-RISK：1 高 + 2 中 + 3 低（6 项核心检查全部复现为真） |
| 2026-11-18 04:10 | 已退回 | 进行中 | dev | 修复轮：高 1 + 中 2 + 低 3 全修，Rust 99/99 |
| 2026-11-18 09:10 | 进行中 | 待验证 | ba | QA 轻量复核 **PASS**，3 个必修点全部独立复现为真 |
| 2026-11-18 09:30 | 待验证 | 已完成 | ba | 合并到 develop @ 37a93c5，worktree 与分支已回收 |

## 完成记录

| 项 | 结果 |
|---|---|
| 合并 | `develop` @ `37a93c5`，`--no-ff` |
| 改动 | 6 个 Dev 提交 + 1 个 BA 归档提交 / 42 文件 / +5400 -455 行 |
| 回收 | worktree 与分支已删除；两份 QA 报告随合并保留在 `develop` |
| 回归 | Rust 99/99、clippy 0 告警、前端 23/23、E2E 1/1、**Cargo.lock 零新第三方包** |

## ★ 本需求的核心验收点

`cargo xtask create-module <name>` 生成的模块**直接通过 REQ-003 的两道关卡**：

```
cargo xtask check-modules    →  2 个模块通过校验，exit 0
cargo test -p <module>       →  契约 C-1~C-4 全绿（4/4）
```

QA 用 `demo-qa-check` 与带连字符的长名 `order-management` 各跑一遍，
并额外做了**负向测试**：C-1 / C-2 在真实违规下确实 FAILED，非空跑。

**这证明了底座的核心目标** —— 业务方照着脚手架就能得到一个合规模块，不需要手写目录、
不需要知道契约细节、不会漏声明权限点。

## QA 两轮发现的问题与处置

| 级别 | 问题 | 处置 |
|---|---|---|
| **高** | CI 缺 `pull_request_review` 触发器 → 评审批准后 workflow 不重跑，「强制人工评审」门禁永远无法变绿 | 已修：补 `pull_request_review: [submitted, edited, dismissed]`；QA 用**从 ci.yml 逐字提取**的 jq 表达式喂 7 组样本全部正确 |
| 中 | prompts 02/07 命令多一个 `--` → **跑 0 个用例却 exit 0**（假绿） | 已修；QA 反向验证假绿确实存在过（0 用例 + exit 0）；该坑已写入 AGENTS.md 硬规则第 8 条 |
| 中 | AGENTS.md 的 `BA/contracts/...` 路径在仓库内不存在（`BA/` 被 gitignore） | 已修：正本在 `demand` 分支的 `contracts/`，仓库内放**快照** `docs/contracts/`，由 `scripts/sync-contracts.sh --sync` 生成、CI 强制校验；**正本唯一，漂移无法漏检** |
| 低 | 多余文件时修复提示无效 | 已修：生成器改为清理陈旧产物 |
| 低 | 验证报告数字不准（17 实为 18） | 已修 |
| 低 | 「逐行一致」表述不准（实为集合相等） | 已修 |
| — | **Dev 纠正了 BA 的一个路径错误** | `BA/` 是 worktree 目录，不是分支内子目录；分支内真实路径是 `contracts/...` |

## BA 侧处置

- **AC-038 已修订**（[父需求 acceptance.md](../REQ-001-platform-foundation/acceptance.md)）：
  原 AC 期望 AI 复用 `@base/ui` 表格与 `@base/schema` 渲染器，
  但两者本期尚不存在（属 REQ-011 / REQ-012）。收窄为「索引如实区分 `already_provided` 与 `not_yet_provided`」——
  Dev 实施时发现设计摘要把未实现能力写成已有会让 AI 照着不存在的 API 写代码，已拆成两段，**这一判断是对的**。
  等 REQ-011 / REQ-012 落地后该 AC 扩展回原意。

## 遗留技术债

| # | 债务 | 建议归属 |
|---|------|----------|
| 1 | `sync-contracts.sh` 拿不到 `demand` 分支时**静默跳过 exit 0**，CI 门禁可能空转 | REQ-005（CI 交付基线）一并修 |
| 2 | required check + Code Owners 需人工在仓库设置里配置，代码解决不了 | 运维操作，已在 `ci.yml` 注释与文档如实登记 |
| 3 | `core-change-review` 用 `pull_request.base.sha` 做 diff，该 SHA 不随 base 前进 → 偏严（不会漏放） | REQ-005 或后续优化 |
| 4 | U+FEFF / U+0085 两个 Unicode 空白码位，schema 求值器与平台实现判定分歧（REQ-003 遗留） | REQ-005 或 REQ-011 |

## 阻塞项

无 —— 依赖 REQ-002 / REQ-003 已完成，REQ-004 已合并。
