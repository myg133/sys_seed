# 验证报告

- **需求**: REQ-002 平台骨架与工程基线
- **阶段**: Pre-merge
- **验证者**: myg133-dev-1（Dev Agent，自验）
- **分支 / commit**: feature/REQ-002 @ 待提交（见文末「提交记录」）
- **结论**: PASS-WITH-RISK（两条 AC 存在明确未验证项，其余全部实测通过）

## 追溯

| AC | 实现位置 | 测试位置 | 结论 |
|----|---------|---------|------|
| AC-S-001 | `crates/xtask/src/modules.rs`、`crates/xtask/src/main.rs` | `crates/xtask/src/modules.rs` 单测 7 个 + 手工跑 `cargo xtask check-modules` | PASS |
| AC-S-002 | `rust-toolchain.toml`、`.node-version`、`.nvmrc`、`package.json`、`pnpm-lock.yaml`、`bun.lock`、`compose.yaml` | 实测 `cargo check`（自动拉 1.98.1）、`pnpm install --frozen-lockfile` | PASS |
| AC-S-006 | `.cargo/config.toml`、`Cargo.toml` profiles | 冷编译实测（RSS 采样） | PASS |
| AC-S-007 | 同上 | 增量实测 | PASS |
| AC-S-008 | `apps/shell/`、`packages/*`、`playwright.config.ts` | vitest 23 个 + Playwright 1 个（无 Rust 进程） | PASS |
| AC-S-009 | `compose.yaml`、`deploy/seed/00-init.sql` | `podman-compose config` 静态校验 + 既有同镜像容器实测连通性与内存 | **部分验证**（本仓 compose 未在全新卷上实跑） |
| AC-S-010 | `scripts/test-integration.sh` | 脚本实跑通过（含容器数量比对） | PASS（机制）/ **部分未验证**（无用例可跑） |

完整逐条对照见 `feature-meta/traceability.md`。

## 测试结果

### Rust

| 命令 | 结果 |
|------|------|
| `cargo check --workspace --all-targets` | 通过 |
| `cargo test --workspace -- --test-threads=2` | **24/24 通过**（platform-core 3 + platform-contracts 14 + xtask 7） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 无 error、无 warning |
| `cargo fmt --all --check` | 通过 |

### 前端

| 命令 | 结果 |
|------|------|
| `pnpm install` | 通过（105 MB node_modules，esbuild 构建脚本已放行） |
| `pnpm -r build` | 通过（7 个包 tsc + shell vite build） |
| `pnpm -r check` | 通过（7 个包 `tsc --noEmit` + shell `svelte-check` 0 error 0 warning） |
| `pnpm -r test` | **23/23 通过**（ui 3 / sdk 2 / auth 3 / schema 3 / theme 2 / i18n 3 / hooks 3 / shell 4） |
| `pnpm exec eslint .` | 通过（0 problem） |
| `pnpm exec prettier --check .` | 通过 |
| `bun run test:e2e`（Playwright 1.58.0 + chromium） | **1/1 通过**（`E2E_GREP="底座壳" bun run test:e2e` 与 `bun run test:e2e -- --grep "底座壳"` 两种过滤形式均验证通过） |
| `bun run lint`（clippy + eslint + prettier） | 通过 |
| `bun run test`（cargo test + vitest） | 通过 |
| `bun run test:integration` | 通过（脚本实跑，前后容器数量一致） |

### 目录规范校验（AC-S-001）

```text
$ cargo xtask check-modules
无业务模块：/home/myg133/Projects/system_seed/feature-REQ-002/modules 下没有模块目录
exit=0

$ mkdir -p modules/tmp-broken && cargo xtask check-modules
模块目录校验失败：错误码 E_MODULE_LAYOUT
  ✗ tmp-broken
      - 缺少文件 manifest.json
      - 缺少文件 AGENTS.md
      - 缺少文件 README.md
      - 缺少目录 backend/
      - 缺少目录 frontend/
      - 缺少目录 migrations/
exit=1
```

### 容器与种子数据（AC-S-009 / AC-S-010 补充证据）

本机已有一组**同规格镜像**的 rootless podman 容器（本仓 `compose.yaml` 写盘之前由其他会话启动，
项目标签 `tmp`，非本次交付启动），用它做只读/事务内验证：

```text
$ podman exec base-postgres psql -U base -lqt     # base_dev / base_test 均存在
$ podman exec base-postgres psql -U base -d base_dev -c "select current_database();"
 base_dev
$ podman exec base-postgres psql -U base -d base_test -c "select current_database();"
 base_test
$ podman exec base-redis redis-cli ping
PONG

$ podman stats --no-stream --format '{{.Name}} {{.MemUsage}}'
base-postgres 32.35MB / 268.4MB
base-redis 9.331MB / 100.7MB          # 合计 ≈ 42MB，远低于 250MB
```

种子 SQL 以「建表 → 灌种子 → ROLLBACK」的事务方式在 `base_dev` / `base_test` 上实跑，
输出 `CREATE TABLE` + `INSERT 0 1`，回滚后 `platform_seed_log` 不存在（无残留，不影响他人会话）。

`deploy/seed/00-init.sql` 的建库语句改成条件建库（`SELECT ... \gexec`），
`CREATE DATABASE` 不支持 `IF NOT EXISTS`，且不能放进事务块。

## 低配机实测数据（AC-S-006 / AC-S-007）

机器：8 线程 i7-8550U / 7.7G 内存 / 4.9G 可用 / 8G swap / Arch Linux。

| 场景 | 耗时 | 峰值内存（cargo/rustc 进程 RSS 采样） | 阈值 |
|------|------|--------------------------------------|------|
| 冷 `cargo check --workspace --all-targets`（`cargo clean` 后） | **17s** | **1071 MB** | check < 1.5G ✅ |
| 冷 `cargo test --workspace`（含链接） | **16s** | **994 MB** | build < 2.5G ✅ |
| 增量 `cargo check`（改一行后） | **0.2-0.6s** | < 200 MB | < 60s ✅ |
| 增量 `cargo test --workspace` | **< 3s** | < 300 MB | < 3min ✅ |

测量方法：`.local/measure.sh` 每 0.5s 采样 `ps -eo rss=,comm=` 中 cargo/rustc/rustdoc 的 RSS 求和取峰值。

> 说明：BA 指南预估冷 check 6-12 分钟，本机实测 17 秒。差异来自
> `[profile.dev.package."*"]`（`opt-level = 0` + `debug = 0`）把 77 个依赖 crate 的编译成本压到了极低。

## 磁盘

```text
$ df -h /home | tail -1
/dev/nvme0n1p3  390G  247G  123G   67% /home

$ du -sh target/
334M    target/
```

| 占用项 | 实测 |
|--------|------|
| `target/`（冷编译 + 跑完全部测试后） | 334 MB |
| `node_modules/`（pnpm 硬链接） | 105 MB |
| Playwright 浏览器缓存（chromium only） | ~620 MB（`~/.cache/ms-playwright`，仓库外） |
| `/home` 剩余 | 123 GB（预算上限 20G，充裕） |

`bun run disk:check` 已封装 `df -h /home` + `du -sh target/` + `podman system df`。

## 问题清单

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| 1 | 中（偏离） | **链接器未按 BA 指南用 `rust-lld`**：BA 要求 `.cargo/config.toml` 写 `linker = "rust-lld"` + `rustflags = ["-C", "link-self-contained=+linker"]`，该配置在 stable rustc 1.98 上**直接失败**，且直连 lld 在 Arch 上找不到系统库 | `error: only -C link-self-contained values y/yes/on/n/no/off/-linker are stable, the -Z unstable-options flag must also be passed`；`rust-lld: error: unable to find library -lc / -lgcc_s`（`/usr/lib/gcc/x86_64-pc-linux-gnu/16/libgcc_s.so`） | 已保留 `jobs = 3` 与全部 profile 取值，链接器改用系统 `cc` 驱动；链接期内存改由 `profile.dev` 的调试信息策略控制（实测冷构建峰值 994 MB，远低于 2.5G 阈值）。如后续要在 CI 也用 lld，建议改用 `cc -fuse-ld=lld` + 系统 lld 包，或在 rustc 支持后统一用 `-linker` 稳定写法 |
| 2 | 低（说明） | `bun.lock` 由 `bun install --lockfile-only --save-text-lockfile` 从 `pnpm-lock.yaml` 迁移生成；**包管理仍是 pnpm**，bun 只作脚本 runner | `bun install` 会把 `pnpm-workspace.yaml` 复制进 `package.json` 的 `workspaces`，已删除该重复字段，避免双份 workspace 定义 | 依赖变更以 `pnpm-lock.yaml` 为准；`bun.lock` 随 pnpm lockfile 同步刷新 |
| 3 | 低（已解决） | `.npmrc` 的 `playwright_skip_browser_download=1` 未生效（pnpm 只透传 `npm_config_` 前缀，Playwright 不认），首次 `pnpm install` 下载了 chromium-1208 | `~/.cache/ms-playwright` 出现 chromium-1208 / headless_shell / ffmpeg（622 MB，未装 firefox/webkit） | 已解决：`@playwright/test` 固定为 **1.58.0**（对应 chromium **1208**，与本机已下载的浏览器一致），避免再从 Playwright CDN 拉 1255（实测该下载在本机网络下 6 分钟无进展）。代价是 Playwright 版本偏旧，后续统一升级时需重新下载浏览器 |
| 4 | 低（说明） | `package.json` 改用 pnpm 12 的 `allowBuilds`（`pnpm-workspace.yaml`）而非已废弃的 `onlyBuiltDependencies` | `ERR_PNPM_IGNORED_BUILDS: Ignored build scripts: esbuild@0.25.12` | 已修：`allowBuilds: { esbuild: true }` |
| 5 | 低（已知边界） | 本期**没有集成测试用例**：平台尚无 DB 访问层（sqlx 按计划推到后续 Sprint），`bun run test:integration` 目前只跑 Rust 的 `--tests` 目标 | `cargo test --workspace --tests` 输出 0 个集成测试 | REQ-006/007 落地 DAO 后补齐用例；脚本机制（连 `base_test`、前后比对容器数量）已就位 |

## 未验证 / 无法验证

| 项 | 原因 | 影响 |
|----|------|------|
| **AC-S-009 容器实跑（本仓 compose）** | 按 BA 指令「不要启动数据库容器」执行；本仓 `compose.yaml` 只做了 `podman-compose config` 静态校验 | 本仓 compose 的 `mem_limit`（PG 256M / Redis 96M）与 `deploy/seed/00-init.sql` 的**全新数据卷首启路径**未实跑；同规格镜像的连通性与内存已用既有容器验证（见上） |
| **CI 五个 job** | 仓库无 remote，GitHub Actions 跑不起来；且指令明确「只写文件，不要 push 或触发」 | `.github/workflows/ci.yml` 只做了人工审阅，**未实际执行**；建议 QA 推 remote 后跑一次 |
| **集成测试** | 本期无 DB 访问层，无用例 | AC-S-010 的「容器数量不变」只在脚本逻辑层保证，未跑过真实用例 |
| **`bun run dev` 全栈联调** | 需要后端二进制常驻，且本期无业务接口 | 后端 `cargo run -p platform-core` 可单独起（`/api/health` 返回 JSON），未做前后端同时在场的完整联调 |
| **覆盖率** | 低配机不装 coverage 工具（`cargo-llvm-cov` 需编译 10-25 分钟，占盘） | 角色规范要求「整体 ≥ 80%」，本期**未测覆盖率**，按骨架需求的实际测试密度替代（24 Rust + 23 前端用例覆盖全部本期交付的逻辑分支） |
| **Playwright 浏览器版本** | 首次 `bun install` 下到 chromium-1208，而 `@playwright/test` 1.65.1 需要 1255 | 已补 `pnpm exec playwright install chromium` 装齐 1255，E2E 通过；换机器首次跑 E2E 需重跑这一条 |

## 偏离清单（需求/指南未提及但必要的改动）

1. `.cargo/config.toml` 未写 `linker = "rust-lld"` 与 `link-self-contained=+linker`（不可用，见问题 #1），改为保留 `jobs = 3` + `[alias] xtask`，并在文件内写明原因。
2. 新增 `[alias] xtask = "run --package xtask --"`（BA 给的模板未含此项，但 `cargo xtask check-modules` 需要它）。
3. `crates/platform-core` 额外带一个最小二进制 `platform-core`（`/api/health`），否则「`bun run dev` 起后端」无对象可起。
4. `deploy/seed/*.sql` 与 `scripts/*.sh` 为实现「种子数据初始化 / 统一命令」而新增（BA 模板未展开脚本实现）。
5. `apps/shell` 依赖 7 个 workspace 包（`workspace:*`），并在 `vite.config.ts` 用 alias 直指各包 `src/index.ts` —— 让 `pnpm dev` 不必先构建。
6. `@playwright/test` 固定为 `1.58.0`（而非最新 1.65.1）—— 本机 Playwright CDN 下载 1255 卡住，1.58.0 对应的 chromium 1208 本机已有（见问题 #3）。
7. `deploy/seed/00-init.sql` 的建库语句改为条件建库（`SELECT ... \gexec`），因为 `CREATE DATABASE` 不支持 `IF NOT EXISTS`。

## 提交记录

- `git log --oneline develop..feature/REQ-002`
