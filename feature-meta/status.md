# REQ-002 状态

- **需求**: REQ-002 平台骨架与工程基线
- **分支**: feature/REQ-002（@ `cbb877c`）
- **状态**: 待验证（QA 复核轮的 2 个低级问题已微修并重新自验完毕，等待 QA 复验）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`、`feature-meta/qa-report-recheck.md`

## 本轮（QA 复核轮微修）处理

复核报告 `feature-meta/qa-report-recheck.md` 结论 **PASS-WITH-RISK**：首轮 8 条问题（M1~M3、L1~L5）**全部复验通过**，
只剩 2 个低级问题。本轮只改这 2 处，未动其它任何逻辑。

| 项 | 内容 | 状态 |
|----|------|------|
| R1 | 链接器内存对比表换成复核方的**无采样缺口口径**实测值：LLD **132.0 MB** / bfd **135.5 MB** / 差 **2.6%**；写明测量方法（内核 `VmHWM` 高水位 + `wait4 ru_maxrss` 双仪器互证，真实仓库二进制）；作废「省 8 倍」说法 | ✅ |
| R1 | **结论保留**：「默认即 LLD、显式配置反而会把链接搞坏」仍然成立；理由改为「是 stable rustc 1.98 的默认行为、无需任何配置」，**不是**因为省内存 | ✅ |
| R2 | `scripts/test-integration.sh` 的重置闸门从**黑名单**（`*base_dev*`）改为**白名单**：只有库名**恰好是 `base_test`** 才允许重置，其它库名一律明确报错并 `exit 2`（不静默跳过） | ✅ |

## 上一轮（退回修复轮）处理

QA Pre-merge 结论 PASS-WITH-RISK（3 中 + 5 低），已逐条修完并复验通过：

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

- `bash -n scripts/test-integration.sh` ✅
- 白名单路径实测 ✅：`base_test` 正常重置（exit 0，标记表被清、基线表重建）/ `base_dev` 拒绝（exit 2）/ `base_probe` 拒绝（exit 2）；被拒库 schema 与数据逐字节未变、`cargo test` 未执行
- `cargo test --workspace -- --test-threads=2` ✅ 24/24
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ 无告警
- `cargo fmt --all --check` ✅
- `pnpm -r test` ✅ 23/23
- `bun run test:e2e` ✅ 1/1（chromium）
- `cargo xtask check-modules` ✅ 输出「无业务模块」，退出码 0
- `pnpm exec prettier --check .` ✅
- （本轮未改动任何 Rust / TS 代码，上一轮的 `cargo check --workspace --all-targets` / `pnpm install --frozen-lockfile` /
  `pnpm -r build` / `pnpm -r check` / `eslint .` 未重跑）
- 临时库 `base_probe` 与临时标记表已清理，`pg_database` 回到 `base_dev / base_test / postgres / template0 / template1`；
  种子数据已用 `bun run seed` 补回；容器只剩 `base-postgres` / `base-redis`

## 待 QA 补验

- CI 六个 job：推 remote 后实跑一次（本仓库无 remote，GitHub Actions 跑不起来）
- 冷编译峰值内存与耗时：合并前在干净机器再跑一次留档（AC-S-006 唯一未由本轮直接实测的量化项）
- 集成测试用例：平台尚无 DB 访问层，本期无用例可跑（机制已就位）
