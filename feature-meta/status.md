# REQ-004 状态

- **需求**: REQ-004 AI 引导体系与能力索引
- **分支**: feature/REQ-004
- **基线**: develop @ `c8236b0`（REQ-002 工程基线 + REQ-003 扩展点框架已合并）
- **状态**: **待验证**（修复轮：首轮 QA 结论 PASS-WITH-RISK，3 个必修 + 3 个建议项已修，自验通过，等待 QA 复验）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`

## 本轮交付

按设计摘要 §6 的 7 项交付清单完成（A 仓库级 AGENTS.md / B 提示词库 /
C 能力索引与生成器 / D create-module / E 模块级 AGENTS.md 模板 / F CI 护栏 /
G 契约 pin），另加 `feature-meta/manifest.json` 的 `contracts` / `adrs` /
`delivered_by` 补回。

| 交付项 | 位置 |
|--------|------|
| 仓库级 `AGENTS.md`（agents.md 开放规范） | `AGENTS.md` |
| 8 套任务级提示词 | `prompts/01..08-*.md` |
| 能力索引（13 个文件，自动生成） | `capabilities/` |
| 能力索引生成器 + `--check` | `crates/xtask/src/capabilities.rs`、`rustscan.rs` |
| `create-module` 脚手架（含模块级 AGENTS.md 模板） | `crates/xtask/src/scaffold.rs` |
| `check-deps` 子命令（复用 REQ-003 的依赖图遍历） | `crates/xtask/src/deps.rs` + `cli.rs` |
| CI 护栏（索引一致性 / 核心改动打标 / 强制人工评审） | `.github/workflows/ci.yml`、`.github/CODEOWNERS` |

## 修复轮（首轮 QA PASS-WITH-RISK）

QA 首轮结论 **PASS-WITH-RISK**：6 项核心检查（咬合点 / 手改检测 / 零新依赖 / 索引两段真实性 /
AGENTS.md 命令 / 回归 98-98）全部独立复现为真，问题集中在**门禁与提示词命令**。本轮逐条修完：

| # | 级别 | 问题 | 修复 | 位置 |
|---|------|------|------|------|
| 1 | 高 | CI 缺 `pull_request_review` 触发器 →「强制人工评审」永远无法变绿 | `on:` 补 `pull_request_review: types: [submitted, edited, dismissed]` + `pull_request` 补 `review_requested`；三组 jq 样本复核结论不变 | `.github/workflows/ci.yml` |
| 2 | 中 | 提示词里的 `cargo test` 多一个 `--` → 跑 0 个用例却 exit 0（假绿） | 修正 02/07 两处命令 + 脚手架注释；8 个文件 12 条命令逐条实跑，用例数全部 > 0；坑写进 `AGENTS.md` 硬规则第 8 条 | `prompts/02`、`prompts/07`、`crates/xtask/src/scaffold.rs`、`AGENTS.md` |
| 3 | 中 | `AGENTS.md` 指向仓库内不存在的 `BA/contracts/...` 路径 | 选 (c)：正本用 `git show demand:contracts/...` 取；另在 `docs/contracts/` 放**快照**（由 `scripts/sync-contracts.sh --sync` 生成、由脚本 + CI 校验一致性） | `AGENTS.md`、`prompts/03`、`docs/contracts/`、`scripts/sync-contracts.sh`、`.github/workflows/ci.yml` |
| L1 | 低 | `--check` 报「多余文件」时的修复建议无效 | `write_files` 按 `check` 同一口径剪除多余文件 + 清空目录；提示语补「多余文件会被自动删除」；新增单测 | `crates/xtask/src/capabilities.rs`、`main.rs` |
| L2 | 低 | 报告把 `already_provided` 写成 17 条（实为 18） | 三处更正为 18 条 | `verification-report.md`、`traceability.md`、本文件 |
| L3 | 低 | 称 AGENTS.md 与 index.json「逐行一致」（实为集合相等、顺序不同） | 措辞改为「内容完全一致；行的顺序不同（表格语义序 vs 索引字典序）」 | `AGENTS.md`、`traceability.md`、`verification-report.md` |

## 自验结论

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **98/98**（原 73 个全绿，+25；修复轮 +1 → **99/99**）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ 0 告警
- `cargo fmt --all --check` ✅
- `cargo xtask capabilities` ✅ 生成 13 个文件；`--check` ✅
- **手改 `capabilities/index.json` → `--check` 失败** ✅（`E_CAPABILITIES_OUTDATED`），验完还原
- **手改 `capabilities/sdk/stale-pkg.json` → `--check` 失败 → `cargo xtask capabilities` 自动清理 → `--check` 通过** ✅（L1 修复，验完还原）
- **★ `cargo xtask create-module demo-check`** ✅
  - `cargo xtask check-modules` → `模块目录校验通过：2 个模块`（✓ demo-check / ✓ sandbox）
  - `cargo test -p demo-check -- --test-threads=2` → **4/4**（C-1 / C-2 / C-3 / C-4 全 ok）
  - 生成后立即 `cargo fmt --check` / clippy 通过；**验完已删除**（仓库只剩 `modules/sandbox/`）
- `cargo xtask check-modules`（对 `modules/sandbox/`）✅
- **`prompts/` 8 个文件里的 12 条命令逐条实跑** ✅ 全部 exit 0，且**执行的用例数 > 0**
  （详见 `verification-report.md` §5.4.5）
- **`scripts/sync-contracts.sh`** ✅ 快照与 BA 正本逐字节一致；手改快照 → 退出码 1 → `--sync` 恢复
- `pnpm -r test` ✅ 23/23
- `bun run test:e2e` ✅ 1/1
- `pnpm exec eslint .` ✅ / `pnpm exec prettier --check .` ✅
- `bun run dev` ✅ 后端 `/api/health` 返回 `{"status":"ok"}`，前端 5173 返回 HTTP 200

## 新增依赖

- 第三方：**无**（`Cargo.lock` 与基线逐字节一致）
- workspace crate：新增 0 个
- 未引入 `syn` / `quote` / `proc-macro2` / `async-trait` / `sqlx` / `regex` /
  `sha2` / 任何基准库或 mock 库

## 偏离（详见验证报告 §6）

8 条，核心 4 条：

1. `index.json` 用 `source_fingerprint` 取代 `generated_at`（墙钟时间戳会让
   `--check` 永远失败，与 AC-S-004「手改即失败」冲突）；
2. `sdk/` 生成 7 个文件（设计摘要列 4 个）—— 仓库真实有 7 个前端包；
3. 清单拆成 `already_provided`（18 条）+ `not_yet_provided`（7 条）——
   照抄设计摘要会把「还没实现的能力」写成已有，AI 会照着不存在的 API 写代码；
4. `entry_points` / `platform.modules` 全部指向**真实**路径与**已存在**的模块。

## 待 QA 验证 / 剩余风险

- **GitHub Actions 实跑**（本仓库无 remote，跑不起来）：3 个新 job 的判定逻辑已本机验证
- **分支保护规则**需人工配置（把 `core-change-review` 设为 required check +
  打开「Require review from Code Owners」），这一步在仓库设置里、不在代码里
- **CODEOWNERS 的维护者账号**当前用 `@myg133` 占位，需按实际情况调整
- **AC-036 的字面测试**（把提示词原样交给 AI 生成模块）无本机可复现环境，已做等价验证
- **AC-038 的原文需要 BA 修订**：它期望复用的 `@base/ui` 表格与 `@base/schema`
  渲染器本期尚不存在（渲染器在 REQ-012）
- 覆盖率数字未测量（本机无覆盖率工具，按低配机约束不 `cargo install`）
- 冷编译峰值内存未测量（约束禁止 `cargo clean`）
