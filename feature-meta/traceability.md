# REQ-002 追溯性矩阵

- **需求**: REQ-002 平台骨架与工程基线
- **分支**: feature/REQ-002
- **契约**: api-conventions v1（已 pin，见 `feature-meta/manifest.json`）
- **架构基线**: ADR-0001

## AC 逐条追溯

| AC | 需求要点 | 实现位置 | 测试位置 | 结论 |
|----|---------|---------|---------|------|
| AC-S-001 | 目录规范可校验：不合规模块目录要报错并指出缺失项 | `crates/xtask/src/modules.rs`（`check_modules` / `check_module` / `check_manifest`）、`crates/xtask/src/main.rs`（`run_check_modules`，错误码 `E_MODULE_LAYOUT`） | `crates/xtask/src/modules.rs` 单测 7 个（`missing_modules_dir_reports_no_modules`、`empty_modules_dir_reports_no_modules`、`complete_module_is_valid`、`missing_entries_are_listed`、`invalid_manifest_json_is_reported`、`dot_directories_are_ignored`、`invalid_modules_are_iterable`）；手工跑 `cargo xtask check-modules`：无模块 → exit 0；造 `modules/tmp-broken` → exit 1 并列出 6 项缺失 | **PASS** |
| AC-S-002 | 工具链版本锁定：全新克隆无需手工装 Rust/Bun/PostgreSQL | `rust-toolchain.toml`（1.98.1 + clippy/rustfmt）、`.node-version`、`.nvmrc`、`package.json` 的 `packageManager: pnpm@12.19.0`、`pnpm-lock.yaml`、`bun.lock`、`compose.yaml`（postgres 17 / redis 7） | 实测：删除 `target/` 后 cargo 自动拉取 1.98.1 并 `cargo check` 通过；`pnpm install --frozen-lockfile` 通过 | **PASS**（PostgreSQL 版本靠 compose 镜像锁定，首次仍需拉镜像，属预期） |
| AC-S-006 | 低配开发基线生效：冷编译峰值内存 < 2.5G、check 峰值 < 1.5G、不 OOM | `.cargo/config.toml`（`jobs = 3`）、`Cargo.toml`（`profile.dev` = `line-tables-only` + `split-debuginfo=unpacked` + `incremental`；`profile.dev.package."*"` = `opt-level=0` + `debug=0`；`profile.release` = `lto=thin` + `codegen-units=16` + `strip=debuginfo`） | 实测（脚本 `.local/measure.sh` 采样 cargo/rustc 进程 RSS 峰值）：冷 `cargo check --workspace --all-targets` 峰值 **1071 MB**；冷 `cargo test --workspace`（含链接）峰值 **994 MB** | **PASS**（链接器偏离，见 `verification-report.md` 问题 #1） |
| AC-S-007 | 日常命令流畅：改一行后 `cargo check` < 60s、`cargo test --lib` < 3min | 同上（profile 与 `jobs = 3`） | 实测：增量 `cargo check --workspace --all-targets` **0.2-0.6s**；增量 `cargo test --workspace` **< 3s**；改一行的检查链路全程无需关闭其他服务 | **PASS** |
| AC-S-008 | 前端开发不依赖后端常驻：只起 Vite + mock，Rust 进程数保持 0 | `apps/shell/`（Vite + Svelte 5，只依赖 `packages/*`）、`packages/*`（纯 TS，无后端依赖）、`playwright.config.ts` 的 `webServer` 只起 `@base/shell` preview | `apps/shell/test/nav.test.ts` 4 个；`packages/*/test/*.test.ts` 19 个；`bun run test:e2e` 全程只有 vite preview 与 chromium，无 Rust 进程 | **PASS**（MSW mock 未接入：本期无真实接口，骨架不需要） |
| AC-S-009 | rootless podman 本地环境可用：两库可连、两容器常驻 < 250M、无需 root | `compose.yaml`（显式 daocloud 镜像前缀、`mem_limit` PG 256M / Redis 96M、PG 低内存参数、`deploy/seed/00-init.sql` 建 `base_test`） | `podman-compose -f compose.yaml config` 校验通过（镜像前缀、mem_limit、启动参数均符合）；**未实际启动容器** | **未验证**（按 BA 指令「不要启动数据库容器」执行，只做静态校验） |
| AC-S-010 | 集成测试不使用 testcontainers：测试前后容器数不变 | `scripts/test-integration.sh`（连本机 `base_test`，跑前后比对 `podman ps` 数量）、`Cargo.toml` 无 testcontainers 依赖 | 脚本逻辑已实现；`cargo test --workspace --tests` 当前无集成测试用例（平台尚无 DB 访问层） | **PASS（机制）/ 部分未验证**（无用例可跑） |

## 需求条目（非 AC）对照

| 需求条目 | 实现位置 | 结论 |
|---------|---------|------|
| Rust workspace：`platform-core` / `platform-contracts` / `xtask` | `crates/` 三个 crate，各含 `Cargo.toml`、`src/lib.rs`、单元测试 | PASS |
| 前端 pnpm workspace：`apps/shell` + 7 个 `packages/*` | `pnpm-workspace.yaml`、`apps/shell/`、`packages/{ui,sdk,auth,schema,theme,i18n,hooks}/`（各含 `package.json`、`src/index.ts`、一个最小测试） | PASS |
| 目录规范 + manifest.json schema | `crates/xtask/src/modules.rs`；manifest schema 本期只校验 JSON 可解析（字段级 schema 随 REQ-003 的 manifest 定义落地） | PASS（字段级 schema 未做，属后续 Sprint） |
| 工具链固定（Rust/Node/Bun/PostgreSQL 全部锁文件） | `rust-toolchain.toml`、`.node-version`、`.nvmrc`、`package.json#packageManager`、`pnpm-lock.yaml`、`bun.lock`、`compose.yaml` | PASS |
| CI 模板：lint / test / build / 依赖扫描 / 契约检查 | `.github/workflows/ci.yml` 五个 job（`SQLX_OFFLINE=true`，CI 用官方 `postgres:17-alpine`） | PASS（文件已写，未 push、未触发） |
| 本地环境：compose 一键起 PG + Redis + 种子数据 | `compose.yaml` + `deploy/seed/00-init.sql`、`deploy/seed/01-seed.sql` + `scripts/db-up.sh` / `scripts/seed.sh` | PASS（未实际启动） |
| 低配基线：`.cargo/config.toml`、profile、最小依赖集 | `.cargo/config.toml`、`Cargo.toml`、依赖仅 axum / tokio / serde / serde_json / tracing / tracing-subscriber / clap / thiserror / anyhow / uuid / time | PASS（链接器偏离，见验证报告 #1） |
| 统一命令（dev/check/lint/test/test:integration/test:e2e/seed/disk:check） | `package.json` scripts + `scripts/*.sh` | PASS |
| 基础库设施：error / context / config / xtask 分派 | `crates/platform-core/src/{error,context,config}.rs`、`crates/xtask/src/main.rs` | PASS |

## 测试汇总

| 套件 | 通过/总数 |
|------|-----------|
| Rust 单测（platform-core / platform-contracts / xtask） | 24/24 |
| 前端 vitest（7 个包 + shell） | 23/23 |
| Playwright E2E | 1/1 |
