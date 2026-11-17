# 状态：REQ-002 平台骨架与工程基线

- **状态**: 已退回
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-18 00:50
- **Worktree**: `/home/myg133/Projects/system_seed/feature-REQ-002`（分支 `feature/REQ-002`）

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 已就绪 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-18 00:30 | 已就绪 | 进行中 | dev | Dev 交回：88 文件 / +7278 行，Rust 24/24 + 前端 23/23 + E2E 1/1 |
| 2026-11-18 00:48 | 进行中 | 待验证 | ba | 派 QA Pre-merge |
| 2026-11-18 00:50 | 待验证 | 已退回 | ba | QA 结论 PASS-WITH-RISK，3 中 + 5 低问题需修 |

## 退回原因

QA Pre-merge 报告：`/home/myg133/Projects/system_seed/feature-REQ-002/feature-meta/qa-report.md`
（结论 PASS-WITH-RISK；AC 7 条 → PASS 5 / PARTIAL 2 / FAIL 0）

### 必须修（3 项，中等）

| # | 问题 | 修法 |
|---|------|------|
| M1 | **Dev 报告的「链接器偏离」不成立** —— rustc 1.98 的 `x86_64-unknown-linux-gnu` target spec 默认 `linker-flavor = gnu-lld-cc`，默认即用 rustup 自带 LLD 22.1.8（链接器进程峰值 2MB；强制 GNU bfd 则 130MB）。文档把它记成「偏离」会误导后续开发，甚至可能有人据此加 `linker = "rust-lld"` 把链接搞坏 | 修正 `docs/dev-environment.md` 与 `feature-meta/verification-report.md` 的措辞：不是偏离，是「stable rustc 默认已用 LLD，无需显式配置」。`.cargo/config.toml` 注释同步简化。**不要**真的去加 linker 配置 |
| M2 | **AC-S-010 的「测试库被重置而非新建」完全没实现** —— 全仓无 `reset` / `truncate` / `DROP SCHEMA` / 迁移回滚逻辑，`scripts/test-integration.sh` 只是连 `base_test` 跑测试。后续 DAO 落地后测试会互相污染 | 在 `scripts/test-integration.sh` 的测试前加重置（`DROP SCHEMA public CASCADE; CREATE SCHEMA public;`），并确保只影响 `base_test`，**绝不碰 `base_dev`** |
| M3 | **`scripts/test-integration.sh` 的容器数量断言在测试失败时不可达** —— `set -euo pipefail` 让 `cargo test` 一失败就整段退出，其后的 `podman ps` 前后比对永远不会执行；「测试新建了容器且测试同时失败」这种最该拦的场景反而放行 | 改为 `status=0; cargo test ... \|\| status=$?`，再做容器比对，最后 `exit "$status"` |

### 建议修（5 项，低）

| # | 问题 | 修法 |
|---|------|------|
| L1 | 2 处未使用依赖：`crates/platform-contracts/Cargo.toml` 的 `platform-core`（全 crate 无 `platform_core` 引用）；`apps/shell/package.json` 的 `@base/auth` `@base/hooks` `@base/schema` `@base/theme`（src/test 无 import，只在 vite alias 里配了） | 删掉，或在注释里标注「为后续 Sprint 预留」。推荐**删掉**（vite alias 与 package 依赖是两回事，alias 已够用） |
| L2 | CI 五个 job 都不跑 E2E，AC-S-008 无 CI 回归护栏 | 增补 e2e job：`pnpm exec playwright install --with-deps chromium` + `bun run test:e2e` |
| L3 | CI `dependency-audit` job 的 `token: ${{ secrets.GITHUB_TOKEN }}` 是无效入参 —— `rustsec/audit-check@v2` 认的是 `RUSTEC_AUDIT_TOKEN` 环境变量 | 删掉该入参，或改用环境变量 |
| L4 | Dev 报告称「AC-S-009 全新卷首启未实跑」与事实不符 —— 容器标签、卷挂载、种子时间戳证明该路径已跑通 | 修正 `feature-meta/verification-report.md` 的「未验证」章节 |
| L5 | — | 修完后同步更新 `feature-meta/traceability.md` 与 `feature-meta/verification-report.md` |

### BA 侧已自行修正（不需要 Dev 做）

- **AC-S-008 收窄**（见 acceptance.md 的「范围说明」）：MSW mock、列表页、表单页下推到 REQ-011 / REQ-012。
  本期只验「前端链路不依赖后端常驻」。原 AC 超范围 —— 要求实现它们等于提前做 REQ-011/012 的活。

## 阻塞项

等待 Dev 修完 M1~M3 + L1~L4，重新自验后交回 BA。修完由 BA 再派 QA 做一次**轻量复核**（只查改动点）。

## 附：QA 复核确认成立的事实（Dev 报告可直接引用）

- 冷编译：check 21.2s / 峰值 1087MB；test 29.4s / 峰值 1090MB（AC-S-006 阈值 2.5G / 1.5G，均达标）
- 真增量：check 0.67s；test --lib 0.68s（AC-S-007 阈值 60s / 3min，均达标）
- AC-S-001：不合规模块 → exit 1 + `E_MODULE_LAYOUT` + 逐条列出缺失项；合法目录无误报
- AC-S-008：E2E 1/1 通过，**77 次采样 Rust 进程数恒为 0**
- AC-S-009：rootless 生效、双库可连、`mem_limit` 256M/96M 生效、端口只绑 127.0.0.1、两容器合计 35.9MB
- AC-S-010：容器列表前后**逐字节一致**，无 testcontainers 依赖
- clippy(-D warnings) / rustfmt / eslint / prettier / tsc / svelte-check 全绿
- 无 TODO/FIXME、无调试输出、无硬编码密钥、无 `assert(true)` 类无意义断言
