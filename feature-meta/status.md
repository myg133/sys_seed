# REQ-002 状态

- **需求**: REQ-002 平台骨架与工程基线
- **分支**: feature/REQ-002（@ `6968882`）
- **状态**: 待验证（QA 退回修复轮已完成，重新自验完毕，等待 QA 复验）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`

## 本轮（退回修复轮）处理

QA Pre-merge 结论 PASS-WITH-RISK（3 中 + 5 低），已逐条修完：

| 项 | 内容 | 状态 |
|----|------|------|
| M1 | 链接器措辞纠正：stable rustc 1.98 默认已用 LLD（`linker-flavor = gnu-lld-cc`），撤销「偏离」记账；**未新增任何 linker 键** | ✅ |
| M2 | `scripts/test-integration.sh` 增加测试库重置（只重置 `base_test`，绝不碰 `base_dev`） | ✅ |
| M3 | `set -e` 解耦：测试失败时容器数量比对照常执行，退出码反映测试结果 | ✅ |
| L1 | 删未使用依赖：`platform-contracts` 的 `platform-core`；`apps/shell` 的 `@base/auth` `@base/hooks` `@base/schema` `@base/theme` | ✅ |
| L2 | CI 新增 `e2e` job | ✅ |
| L3 | CI `dependency-audit` 删除无效 `token:` 入参 | ✅ |
| L4 | AC-S-009「全新卷首启」改为已验证并补第一手证据 | ✅ |
| L5 | 同步 `traceability.md` / `verification-report.md` / 问题台账 | ✅ |

## 自验结论

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ 24/24
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ 无告警
- `cargo fmt --all --check` ✅
- `pnpm install --frozen-lockfile` / `pnpm -r build` / `pnpm -r check` / `pnpm -r test` ✅ 23/23
- `eslint .` / `prettier --check .` ✅
- `cargo xtask check-modules` ✅ 输出「无业务模块」，退出码 0
- `bun run test:e2e` ✅ 1/1（chromium）
- `bash -n scripts/test-integration.sh` ✅
- `scripts/test-integration.sh` ✅ 重置执行、`base_dev` 未受影响、容器 2 → 2；4 条分支路径逐条实跑

## 待 QA 补验

- CI 六个 job：推 remote 后实跑一次（本仓库无 remote，GitHub Actions 跑不起来）
- 冷编译峰值内存与耗时：合并前在干净机器再跑一次留档（AC-S-006 唯一未由本轮直接实测的量化项）
- 集成测试用例：平台尚无 DB 访问层，本期无用例可跑（机制已就位）
