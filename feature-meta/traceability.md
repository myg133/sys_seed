# REQ-002 追溯性矩阵

- **需求**: REQ-002 平台骨架与工程基线
- **分支**: feature/REQ-002（@ `cbb877c`，QA 复核轮微修前的自验）
- **契约**: api-conventions v1（已 pin，见 `feature-meta/manifest.json`）
- **架构基线**: ADR-0001
- **轮次**: 第 3 轮（QA 复核轮结论 PASS-WITH-RISK；首轮 8 条问题已全部修完，本轮只修复复核轮新发现的 2 个低级问题）

## AC 逐条追溯

| AC | 需求要点 | 实现位置 | 测试位置 | 结论 |
|----|---------|---------|---------|------|
| AC-S-001 | 目录规范可校验：不合规模块目录要报错并指出缺失项 | `crates/xtask/src/modules.rs`（`check_modules` / `check_module` / `check_manifest`）、`crates/xtask/src/main.rs`（`run_check_modules`，错误码 `E_MODULE_LAYOUT`） | `crates/xtask/src/modules.rs` 单测 7 个（`missing_modules_dir_reports_no_modules`、`empty_modules_dir_reports_no_modules`、`complete_module_is_valid`、`missing_entries_are_listed`、`invalid_manifest_json_is_reported`、`dot_directories_are_ignored`、`invalid_modules_are_iterable`）；手工跑 `cargo xtask check-modules`：无模块 → exit 0；造 `modules/tmp-broken` → exit 1 并列出 6 项缺失 | **PASS** |
| AC-S-002 | 工具链版本锁定：全新克隆无需手工装 Rust/Bun/PostgreSQL | `rust-toolchain.toml`（1.98.1 + clippy/rustfmt）、`.node-version`、`.nvmrc`、`package.json` 的 `packageManager: pnpm@12.19.0`、`pnpm-lock.yaml`、`bun.lock`、`compose.yaml`（postgres 17 / redis 7） | 实测：删除 `target/` 后 cargo 自动拉取 1.98.1 并 `cargo check` 通过；`pnpm install --frozen-lockfile` 通过 | **PASS**（PostgreSQL 版本靠 compose 镜像锁定，首次仍需拉镜像，属预期） |
| AC-S-006 | 低配开发基线生效：冷编译峰值内存 < 2.5G、check 峰值 < 1.5G、不 OOM | `.cargo/config.toml`（`jobs = 3`）、`Cargo.toml`（`profile.dev` = `line-tables-only` + `split-debuginfo=unpacked` + `incremental`；`profile.dev.package."*"` = `opt-level=0` + `debug=0`；`profile.release` = `lto=thin` + `codegen-units=16` + `strip=debuginfo`） | 实测（脚本 `.local/measure.sh` 采样 cargo/rustc 进程 RSS 峰值）：冷 `cargo check --workspace --all-targets` 峰值 **1071 MB**；冷 `cargo test --workspace`（含链接）峰值 **994 MB** | **PASS** |
| AC-S-007 | 日常命令流畅：改一行后 `cargo check` < 60s、`cargo test --lib` < 3min | 同上（profile 与 `jobs = 3`） | 实测：增量 `cargo check --workspace --all-targets` **0.2-0.6s**；增量 `cargo test --workspace` **< 3s**；改一行的检查链路全程无需关闭其他服务 | **PASS** |
| AC-S-008 | 前端开发不依赖后端常驻：只起 Vite + mock，Rust 进程数保持 0 | `apps/shell/`（Vite + Svelte 5，只依赖 `packages/*`）、`packages/*`（纯 TS，无后端依赖）、`playwright.config.ts` 的 `webServer` 只起 `@base/shell` preview | `apps/shell/test/nav.test.ts` 4 个；`packages/*/test/*.test.ts` 19 个；`bun run test:e2e` 全程只有 vite preview 与 chromium，无 Rust 进程 | **PASS**（按 BA 收窄后的口径：本期只验「前端链路不依赖 Rust 后端常驻」；MSW mock / 列表页 / 表单页已由 BA 下推到 REQ-011 / REQ-012，本期不交付） |
| AC-S-009 | rootless podman 本地环境可用：两库可连、两容器常驻 < 250M、无需 root | `compose.yaml`（显式 daocloud 镜像前缀、`mem_limit` PG 256M / Redis 96M、PG 低内存参数、`deploy/seed/00-init.sql` 建 `base_test`） | **本轮第一手实跑**：`podman info` → Rootless=true；容器标签 `com.docker.compose.project.config_files=compose.yaml` + `working_dir` 指向本仓；`podman inspect` → PG Memory=268435456（256 MiB）/ Redis=100663296（96 MiB），端口只绑 `127.0.0.1`；`podman stats` → 29.76MB + 11.07MB ≈ **40.8 MB < 250 MB**；卷挂载 `base-platform_pgdata` + `deploy/seed → /docker-entrypoint-initdb.d`；容器 `StartedAt=2026-11-18 00:30:26` 而种子 `applied_at=2026-11-18 00:30:28`（晚 2 秒）→ **全新卷首启种子路径已实跑**；`base_dev`/`base_test` 双库 psql 连通、`redis-cli ping` → PONG | **PASS**（上一轮记为「部分验证」是误判，本轮已补齐） |
| AC-S-010 | 集成测试不使用 testcontainers：测试前后容器数不变 | `scripts/test-integration.sh`（连本机 `base_test`、测试前**重置测试库**（**白名单闸门**：只有库名恰好 `base_test` 才允许重置，其它库名明确报错并 exit 2）、跑前后比对 `podman ps` 数量、测试失败也照常比对、退出码反映测试结果）、`Cargo.toml` 无 testcontainers 依赖 | **本轮实跑**：脚本 exit 0，重置执行（`DROP SCHEMA` / `CREATE SCHEMA` / 基线表重建），容器数量 2 → 2；**边界实测**：`base_test` 标记表被清、`base_dev` 标记表原样保留；**白名单拒绝实测**：`base_dev` / `base_probe`（库确实存在）/ `postgres` 三条路径均 exit 2、报错清晰、**一条 SQL 都没执行**、被拒库 schema 与数据逐字节未变、`cargo test` 未运行；**断言可达性实测**：人为失败用例下比对仍执行（场景 A 捕获 2 → 3、场景 B 打印「容器数量比对通过」），exit=101；**6 条分支路径**：正常 / 3 条白名单拒绝 / 非本机连接串跳过 / 容器未运行跳过，均有清晰提示 | **PASS**（机制 + 边界 + 白名单 + 失败路径均已实跑；本期仍无 DB 访问层，故无集成用例可跑） |

## 需求条目（非 AC）对照

| 需求条目 | 实现位置 | 结论 |
|---------|---------|------|
| Rust workspace：`platform-core` / `platform-contracts` / `xtask` | `crates/` 三个 crate，各含 `Cargo.toml`、`src/lib.rs`、单元测试 | PASS |
| 前端 pnpm workspace：`apps/shell` + 7 个 `packages/*` | `pnpm-workspace.yaml`、`apps/shell/`、`packages/{ui,sdk,auth,schema,theme,i18n,hooks}/`（各含 `package.json`、`src/index.ts`、一个最小测试） | PASS |
| 目录规范 + manifest.json schema | `crates/xtask/src/modules.rs`；manifest schema 本期只校验 JSON 可解析（字段级 schema 随 REQ-003 的 manifest 定义落地） | PASS（字段级 schema 未做，属后续 Sprint） |
| 工具链固定（Rust/Node/Bun/PostgreSQL 全部锁文件） | `rust-toolchain.toml`、`.node-version`、`.nvmrc`、`package.json#packageManager`、`pnpm-lock.yaml`、`bun.lock`、`compose.yaml` | PASS |
| CI 模板：lint / test / build / 依赖扫描 / 契约检查 | `.github/workflows/ci.yml` 六个 job（`SQLX_OFFLINE=true`，CI 用官方 `postgres:17-alpine`；本轮新增 `e2e`） | PASS（文件已写，未 push、未触发） |
| 本地环境：compose 一键起 PG + Redis + 种子数据 | `compose.yaml` + `deploy/seed/00-init.sql`、`deploy/seed/01-seed.sql` + `scripts/db-up.sh` / `scripts/seed.sh` | PASS（本轮已实跑：容器由本仓 compose.yaml 启动并健康） |
| 低配基线：`.cargo/config.toml`、profile、最小依赖集 | `.cargo/config.toml`、`Cargo.toml`、依赖仅 axum / tokio / serde / serde_json / tracing / tracing-subscriber / clap / thiserror / anyhow / uuid / time | PASS（链接器：stable rustc 1.98 默认 `linker-flavor = gnu-lld-cc`，默认即 LLD 22.1.8，**无需显式配置**，非偏离；实测链接器峰值内存 LLD 132.0 MB vs bfd 135.5 MB，**只差 2.6%**，选它的理由是「默认行为、无需配置」而**不是**省内存） |
| 统一命令（dev/check/lint/test/test:integration/test:e2e/seed/disk:check） | `package.json` scripts + `scripts/*.sh` | PASS |
| 基础库设施：error / context / config / xtask 分派 | `crates/platform-core/src/{error,context,config}.rs`、`crates/xtask/src/main.rs` | PASS |

## 本轮（QA 退回修复轮）问题处理对照

| QA 问题 | 级别 | 处理 | 落地位置 |
|---------|------|------|----------|
| #1 链接器「偏离」不成立 | 中 | 撤除该记账；改写文档说明「默认已用 LLD，显式配置反而坏事」；**未新增任何 linker 键** | `docs/dev-environment.md` §2.1、`.cargo/config.toml` 注释、`feature-meta/verification-report.md` |
| #2 测试库未重置 | 中 | 脚本增加 `reset_test_db()`，只重置 `DATABASE_URL_TEST`，带 `base_dev` 硬拒绝闸与 DB 未起跳过提示 | `scripts/test-integration.sh` |
| #3 `set -e` 致断言不可达 | 中 | `status=0; cargo test … \|\| status=$?` → 比对照常执行 → `exit "$status"` | `scripts/test-integration.sh` |
| #4 AC-S-008 的 MSW / 列表页 / 表单页 | 低 | BA 已收窄 AC（本期只验「前端不依赖 Rust 后端常驻」），下推 REQ-011 / REQ-012；本期不补做 | 追溯矩阵 + 验证报告口径更新 |
| #5 未使用依赖 2 处 | 低 | 删 `crates/platform-contracts` 的 `platform-core`、删 `apps/shell` 的 `@base/auth` `@base/hooks` `@base/schema` `@base/theme`；vite alias 直指 `packages/*/src/index.ts`，不依赖 node_modules 链接 | `crates/platform-contracts/Cargo.toml`、`apps/shell/package.json`、`Cargo.lock`、`pnpm-lock.yaml` |
| #6 CI 未跑 E2E | 低 | 新增 `e2e` job（`pnpm exec playwright install --with-deps chromium` + `bun run test:e2e`） | `.github/workflows/ci.yml` |
| #7 CI `token` 入参无效 | 低 | 删除 `token: ${{ secrets.GITHUB_TOKEN }}`（该 action 认的是 `RUSTEC_AUDIT_TOKEN` 环境变量） | `.github/workflows/ci.yml` |
| #8 AC-S-009「全新卷首启未实跑」与事实不符 | 低 | 改为已验证，并补上第一手证据（容器标签 / 卷挂载 / 种子时间戳比容器启动晚 2 秒） | 本矩阵 + `feature-meta/verification-report.md` |

## 本轮（QA 复核轮微修）问题处理对照

复核报告：`feature-meta/qa-report-recheck.md`（结论 PASS-WITH-RISK；首轮 8 条问题全部复验通过，只剩 2 个低级问题）。

| QA 问题 | 级别 | 处理 | 落地位置 |
|---------|------|------|----------|
| R1 链接器内存对比表口径不可复现、方向与实测相反（文档写「LLD 16.1 MB / bfd 134.0 MB，约 8 倍」；16.1 MB 低于 `ld.lld` 空载地板 32.8 MB，物理上不可能） | 低 | 换成**无采样缺口口径**实测值：LLD **132.0 MB** / bfd **135.5 MB** / 差 **2.6%**，并写明测量方法（内核 `VmHWM` 高水位 + `wait4 ru_maxrss` 双仪器互证，负载为真实仓库二进制）；作废「省 8 倍」说法。**结论保留**「默认即 LLD、显式配置反而坏事」，理由改为「是 stable rustc 1.98 的默认行为、无需任何配置」 | `docs/dev-environment.md` §2.1、`feature-meta/verification-report.md`（链接器复核小节 + 问题台账 R1 + 撤除条目 #1） |
| R2 测试库重置闸门是**黑名单**（`*base_dev*`），漏掉其它任何库（实测 `base_probe` 照样被重置） | 低 | 改为**白名单**：从连接串解析库名，只有库名**恰好是 `base_test`** 才允许重置；其它库名（含 `base_dev` / `postgres` / 任意其它库）一律明确报错并 `exit 2`，不静默跳过；解析不出库名同样拒绝 | `scripts/test-integration.sh`（`reset_test_db()` 安全闸 + 顶部注释） |

## 测试汇总

| 套件 | 通过/总数 |
|------|-----------|
| Rust 单测（platform-core / platform-contracts / xtask） | 24/24 |
| 前端 vitest（7 个包 + shell） | 23/23 |
| Playwright E2E | 1/1 |
| 集成测试脚本（重置 + 白名单闸门 + 容器比对 + 失败路径） | 6 条分支路径全部实跑通过（3 条白名单拒绝路径均 exit 2） |
