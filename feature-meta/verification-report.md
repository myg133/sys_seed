# 验证报告

- **需求**: REQ-002 平台骨架与工程基线
- **阶段**: Pre-merge（第 2 轮 —— QA 退回修复后重自验）
- **验证者**: myg133-dev-1（Dev Agent，自验）
- **分支 / commit**: feature/REQ-002 @ `6968882`（本轮修复 4 个 commit：`a639df4` 链接器文档 / `3c891fe` 集成脚本 / `2040f86` 清依赖 / `6968882` CI；下面第 5 个 commit 只更新本报告与追溯矩阵）
- **结论**: PASS（AC 7 条全部 PASS；上一轮 3 个中等 + 5 个低级问题已全部处理）

> 本轮为 **退回修复轮**，范围严格限定在 QA 报告（`feature-meta/qa-report.md`）列出的 8 条问题，
> 不扩大 scope。AC-S-008 的验收范围已由 BA 收窄为「前端链路不依赖 Rust 后端常驻」，
> 本轮据���验的 E2E 1/1 + Rust 进程采样 0 仍然成立，无需补做 MSW 与列表页。

## 追溯

| AC | 实现位置 | 测试位置 | 结论 |
|----|---------|---------|------|
| AC-S-001 | `crates/xtask/src/modules.rs`、`crates/xtask/src/main.rs` | `crates/xtask/src/modules.rs` 单测 7 个 + 手工跑 `cargo xtask check-modules` | PASS |
| AC-S-002 | `rust-toolchain.toml`、`.node-version`、`.nvmrc`、`package.json`、`pnpm-lock.yaml`、`bun.lock`、`compose.yaml` | 实测 `cargo check`（自动拉 1.98.1）、`pnpm install --frozen-lockfile` | PASS |
| AC-S-006 | `.cargo/config.toml`、`Cargo.toml` profiles | 冷编译实测（RSS 采样） | PASS |
| AC-S-007 | 同上 | 增量实测 | PASS |
| AC-S-008 | `apps/shell/`、`packages/*`、`playwright.config.ts` | vitest 23 个 + Playwright 1 个（无 Rust 进程） | PASS（按 BA 收窄后的口径） |
| AC-S-009 | `compose.yaml`、`deploy/seed/00-init.sql` | **本轮实跑**：rootless、双库连通、mem_limit、端口绑定、内存合计、**全新卷首启种子路径** | **PASS**（上一轮记为「部分验证」是误判，本轮已实跑补齐） |
| AC-S-010 | `scripts/test-integration.sh` | **本轮实跑**：脚本含**测试库重置** + 容器数量比对；4 条分支路径逐条实跑 | **PASS**（上一轮的「未实现重置」已补） |

完整逐条对照见 `feature-meta/traceability.md`。

## 测试结果

### Rust

| 命令 | 结果 |
|------|------|
| `cargo check --workspace --all-targets` | 通过 |
| `cargo test --workspace -- --test-threads=2` | **24/24 通过**（platform-core 3 + platform-contracts 14 + xtask 7） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 无 error、无 warning |
| `cargo fmt --all --check` | 通过 |
| `cargo xtask check-modules` | 通过（输出「无业务模块」，退出码 0） |

### 前端

| 命令 | 结果 |
|------|------|
| `pnpm install --frozen-lockfile` | 通过（删依赖后 lockfile 已同步） |
| `pnpm -r build` | 通过（7 个包 tsc + shell vite build，113 modules transformed） |
| `pnpm -r check` | 通过（7 个包 `tsc --noEmit` + shell `svelte-check` 0 error 0 warning） |
| `pnpm -r test` | **23/23 通过**（ui 3 / sdk 2 / auth 3 / schema 3 / theme 2 / i18n 3 / hooks 3 / shell 4） |
| `pnpm exec eslint .` | 通过（0 problem） |
| `pnpm exec prettier --check .` | 通过 |
| `bun run test:e2e`（Playwright 1.58.0 + chromium 1208） | **1/1 通过**（3.6s） |
| `bash -n scripts/test-integration.sh` | 通过 |

### 集成测试脚本（本轮重点，AC-S-010）

```text
$ ./scripts/test-integration.sh
重置测试库：postgres://base:base@127.0.0.1:5432/base_test（库 base_test）
DROP SCHEMA
CREATE SCHEMA
CREATE TABLE
... cargo test --workspace --tests -- --test-threads=2 ... 24/24 通过 ...
容器数量比对通过：2 → 2（未新建容器）
exit=0
```

**重置边界实测**（在 `base_test` 与 `base_dev` 各埋一张标记表，跑完脚本后核对）：

| 库 | 重置前 | 重置后 | 判定 |
|----|--------|--------|------|
| `base_test` | `platform_seed_log`、`reset_probe_marker` | `platform_seed_log`（由 `deploy/seed/00-init.sql` 重建） | 标记表被清掉 ✅ |
| `base_dev` | `platform_seed_log`、`dev_untouched_probe` | `platform_seed_log`、`dev_untouched_probe`（标记表原样保留） | **开发库未受影响** ✅ |

### M3 断言可达性实测（上一轮不可达的断言）

临时插入一个**故意失败**的测试（验证后已删除），跑 `./scripts/test-integration.sh`：

```text
# 场景 A：测试失败 + 测试期间新建了容器（QA 点名的「最该拦住」的场景）
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
error: test failed, to rerun pass `-p platform-core --test tmp_m3_probe`
集成测试期间容器数量发生变化（2 → 3），不允许新建容器
exit=101

# 场景 B：测试失败，但没有新建容器
test result: FAILED. 0 passed; 1 failed; ...
error: test failed, to rerun pass `-p platform-core --test tmp_m3_probe`
容器数量比对通过：2 → 2（未新建容器）
exit=101
```

→ **两种情况下容器数量比对都执行了**；最终 exit code = 101（cargo test 的失败码），反映测试结果。
→ 修复前该断言在 `set -e` 下**永远不会执行**（`cargo test` 一失败整段就退出）。
→ 验证用的临时用例与临时容器均已删除（`git status --short` 干净，容器只剩 `base-postgres` / `base-redis`）。

### 测试库重置的 4 条分支路径（逐条实跑）

| 场景 | 实际输出 | exit |
|------|----------|------|
| 正常（DB 在跑） | `重置测试库：…（库 base_test）` → `DROP SCHEMA` / `CREATE SCHEMA` / `CREATE TABLE` | 0 |
| `DATABASE_URL_TEST` 指向 `base_dev` | `错误：DATABASE_URL_TEST 指向 base_dev，拒绝重置开发库（…）` | **2**（硬拒绝，不执行任何 SQL） |
| `DATABASE_URL_TEST` 指向非本机（如 `db.example.com`） | `跳过测试库重置：主机未安装 psql，且 DATABASE_URL_TEST 不指向本机 127.0.0.1:5432` + `需要集成测试时先执行 'bun run db:up'，或安装 psql 客户端` | 继续跑测试 |
| 数据库容器未运行 | `跳过测试库重置：主机未安装 psql，且数据库容器未运行` + `本地开发不常开数据库；需要集成测试时先执行 'bun run db:up'` | 继续跑测试 |

→ 跳过路径**不是静默失败**：有明确提示，且后续测试照常执行、退出码仍反映测试结果。

### 链接器复核（本轮第一手实测，纠正上一轮的记账错误）

**上一轮把「未按 BA 指南配 `linker = "rust-lld"`」记成「偏离」是错的 —— 这个偏离并不存在。**

```console
$ RUSTC_BOOTSTRAP=1 rustc -Z unstable-options --print target-spec-json --target x86_64-unknown-linux-gnu
"linker-flavor": "gnu-lld-cc",

$ rustc --print link-args probe.rs --edition 2021 -o /tmp/probe | tr ' ' '\n' | grep -iE 'fuse-ld|lld'
"-B/home/myg133/.rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld"
"-fuse-ld=lld"
```

真实链接过程中链接器自报家门（探针 crate 建在 `.local/` 下，测完已删）：

```console
# 默认（仓库现状：不设任何 linker 配置）
$ RUSTFLAGS="-C link-arg=-Wl,--version" cargo build
warning: linker stderr: ... /bin/gcc-ld/ld.lld -plugin ... -m elf_x86_64 ... -o ...
warning: linker stdout: LLD 22.1.8 (/checkout/src/llvm-project/llvm 52ed14fcd...) (compatible with GNU linkers)

# 强制走 GNU bfd
$ RUSTFLAGS="-C link-arg=-fuse-ld=bfd -C link-arg=-Wl,--version" cargo build
warning: linker stderr: ... /usr/bin/ld.bfd -plugin ... -m elf_x86_64 ... -o ...
warning: linker stdout: GNU ld (GNU Binutils) 2.47
```

链接器进程峰值 RSS（对 `platform-core` 真实二进制采样 `/proc/<pid>/status` 的 `VmRSS` 取峰值）：

| 模式 | 链接器 | 峰值 RSS |
|------|--------|---------|
| **默认（仓库现状）** | LLD 22.1.8 | **16.1 MB** |
| 强制 `-C link-arg=-fuse-ld=bfd` | GNU ld 2.47 | **134.0 MB**（约 8 倍） |

> 结论：**默认就是最优解（LLD）**。显式配置 `linker = "rust-lld"` / `link-self-contained` 反而会坏事
> （直连 rust-lld 绕过 gcc 驱动，lld 按自身路径推导 sysroot，在 Arch 上找不到系统库 `-lc` / `-lgcc_s`，链接必然失败）。
> 本轮**没有**新增任何 linker 键，`.cargo/config.toml` 仍只有 `jobs = 3` + `[alias] xtask` + 一段注释。

### 容器与种子数据（AC-S-009，本轮第一手实测）

```text
$ podman info --format '{{.Host.Security.Rootless}}'
true

# 容器确实由本仓 compose.yaml 启动（上一轮报告写「项目标签 tmp，非本次交付启动」与实际值不符）
$ podman inspect base-postgres --format '{{index .Config.Labels "com.docker.compose.project"}} | {{index .Config.Labels "com.docker.compose.project.config_files"}} | {{index .Config.Labels "com.docker.compose.project.working_dir"}}'
base-platform | compose.yaml | /home/myg133/Projects/system_seed/feature-REQ-002

$ podman inspect base-postgres --format 'Memory={{.HostConfig.Memory}} Ports={{.HostConfig.PortBindings}}'
Memory=268435456 Ports=map[5432/tcp:[{127.0.0.1 5432}]]        # 256 MiB = compose.yaml 的 mem_limit
$ podman inspect base-redis --format 'Memory={{.HostConfig.Memory}} Ports={{.HostConfig.PortBindings}}'
Memory=100663296 Ports=map[6379/tcp:[{127.0.0.1 6379}]]        # 96 MiB = compose.yaml 的 mem_limit

$ podman stats --no-stream --format '{{.Name}} {{.MemUsage}}'
base-postgres 29.76MB / 268.4MB
base-redis 11.07MB / 100.7MB          # 合计 ≈ 40.8 MB < 250 MB

$ podman inspect base-postgres --format '{{range .Mounts}}{{.Type}} {{.Source}} -> {{.Destination}}{{"\n"}}{{end}}'
volume .../volumes/base-platform_pgdata/_data -> /var/lib/postgresql/data
bind   /home/myg133/Projects/system_seed/feature-REQ-002/deploy/seed -> /docker-entrypoint-initdb.d

$ podman inspect base-postgres --format '{{.State.StartedAt}}'
2026-11-18 00:30:26.909607089 +0800 CST
$ podman exec base-postgres psql -U base -d base_dev -tAc "select batch||' @ '||applied_at from platform_seed_log;"
baseline @ 2026-11-17 16:30:28.56635+00        # = 2026-11-18 00:30:28 +0800，容器启动后 2 秒
```

→ 种子批次在容器启动后 **2 秒**写入，说明 `deploy/seed/*.sql` 经 `initdb.d` 挂载的**首次初始化路径已真实执行成功**
（新建卷 → initdb → 建 `base_test` → 建 `platform_seed_log` → 灌 `baseline` 批次）。
→ `base_dev` / `base_test` 双库 `psql` 连通、`redis-cli ping` 返回 PONG、端口只绑 127.0.0.1。

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
349M    target/
```

| 占用项 | 实测 |
|--------|------|
| `target/`（冷编译 + 跑完全部测试后） | 349 MB |
| `node_modules/`（pnpm 硬链接） | 137 MB |
| Playwright 浏览器缓存（chromium only，仓库外） | ~620 MB（`~/.cache/ms-playwright`） |
| `/home` 剩余 | 123 GB（预算上限 20G，充裕） |

## 问题台账

### 已撤除

| # | 原级别 | 描述 | 处理 |
|---|--------|------|------|
| ~~1~~ | 中（偏离） | ~~链接器未按 BA 指南用 `rust-lld`~~ | **撤除**：该偏离不存在。rustc 1.98 的 `x86_64-unknown-linux-gnu` target spec 默认 `linker-flavor = gnu-lld-cc`，默认构建实测链接器为 `LLD 22.1.8`（峰值 16.1 MB），强制 `-fuse-ld=bfd` 才变成 `GNU ld 2.47`（峰值 134.0 MB）。已改写 `docs/dev-environment.md` §2.1 并简化 `.cargo/config.toml` 注释；**未新增任何 linker 键**（加了反而会让链接失败） |

### 本轮已修

| # | 原级别 | 描述 | 处理 | 验证 |
|---|--------|------|------|------|
| 2 | 中 | AC-S-010「测试库被重置而非新建」未实现 | `scripts/test-integration.sh` 增加 `reset_test_db()`：`DROP SCHEMA IF EXISTS public CASCADE; CREATE SCHEMA public;` + 用 `deploy/seed/00-init.sql` 重建基线表。**只作用于 `DATABASE_URL_TEST`**，带 `base_dev` 硬拒绝闸；DB 没起时给清晰提示并跳过 | 重置边界实测（`base_test` 清空 / `base_dev` 原样）+ 4 条分支路径逐条实跑 |
| 3 | 中 | `set -e` 导致容器数量断言不可达 | 改为 `status=0; cargo test … \|\| status=$?`，比对照常执行，最后 `exit "$status"` | 场景 A/B 实测：测试失败时比对都执行了，exit=101 |
| 5 | 低 | 未使用依赖 2 处 | `crates/platform-contracts/Cargo.toml` 去掉 `platform-core`；`apps/shell/package.json` 去掉 `@base/auth` `@base/hooks` `@base/schema` `@base/theme` | `cargo check` 通过（`Cargo.lock` 同步）；`pnpm install --frozen-lockfile` + `pnpm -r build` + `pnpm -r check` 通过（vite alias 直指 `packages/*/src/index.ts`，不依赖 node_modules 链接） |
| 6 | 低 | CI 未运行 E2E | `ci.yml` 新增 `e2e` job：`pnpm exec playwright install --with-deps chromium` + `bun run test:e2e`（配 `oven-sh/setup-bun@v2` 1.4.4） | YAML 解析通过（6 个 job）；**未实际执行**（仓库无 remote，见「未验证」） |
| 7 | 低 | CI `dependency-audit` 的 `token:` 入参无效 | 删除该入参（`rustsec/audit-check@v2` 认的是 `RUSTEC_AUDIT_TOKEN` 环境变量） | YAML 解析通过；**未实际执行**（同上） |
| 8 | 低 | 「AC-S-009 全新卷首启未实跑」与事实不符 | 改为已验证，并补上本轮第一手证据（容器标签 `com.docker.compose.project.config_files=compose.yaml` / 卷挂载 / 种子时间戳比容器启动晚 2 秒） | 见「容器与种子数据」小节 |

### 保留的说明性记录（非问题）

| # | 描述 | 现状 |
|---|------|------|
| 2' | `bun.lock` 由 `pnpm-lock.yaml` 迁移生成；**包管理仍是 pnpm**，bun 只作脚本 runner | 本轮改依赖后重跑 `bun install --lockfile-only --save-text-lockfile`：`bun.lock` 无变化，`package.json` 未被写入重复的 `workspaces` 字段 |
| 3' | `@playwright/test` 固定 1.58.0 对应 chromium 1208 | 未升级。本轮 `bun run test:e2e` 1/1 通过，未触发任何浏览器下载 |
| 4' | `package.json` 改用 pnpm 12 的 `allowBuilds` | 本轮 `pnpm install --frozen-lockfile` exit 0，未复现 `ERR_PNPM_IGNORED_BUILDS` |
| 5' | 本期没有集成测试用例（平台尚无 DB 访问层） | 机制已就位（重置 + 容器比对 + 退出码），用例待 REQ-006/007 落地 DAO 后补齐 |

## 未验证 / 无法验证

| 项 | 原因 | 影响 / 补做建议 |
|----|------|----------------|
| **CI 六个 job 实际执行** | 仓库无 remote，GitHub Actions 跑不起来；任务书明确「只改文件不触发」 | `.github/workflows/ci.yml` 仅经人工审阅 + YAML 解析校验，**未实际执行**。本轮以本地等价命令替代验证：`cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` / `cargo xtask check-modules` / `pnpm install --frozen-lockfile` / `pnpm -r build` / `pnpm -r check` / `pnpm -r test` / `pnpm exec eslint .` / `pnpm exec prettier --check .` / `bun run test:e2e` —— 全部 exit 0。**推 remote 后仍建议跑一次真实 CI** |
| **冷编译峰值内存与耗时（AC-S-006 主体）** | 本轮是修复轮，未重做冷编译；`cargo clean` 会清掉 349MB `target/`，需重编 77 个依赖 crate | 冷编译数据（check 17s/1071MB、test 16s/994MB）**引用上一轮实测**；另有 QA 两轮实测（21.2s/1087MB、29.4s/1090MB）互相印证，同量级且远低于 1.5G/2.5G 阈值。**建议合并前在干净机器再跑一次冷编译留档** |
| **覆盖率** | 低配机不装 `cargo-llvm-cov`（需编译 10-25 分钟，占盘）；任务书亦不建议 `cargo install` | 角色规范「整体 ≥ 80%」**未测**。按骨架需求的实际测试密度替代（24 Rust + 23 前端用例覆盖本期交付的全部逻辑分支） |
| **`bun run dev` 全栈联调** | 需后端二进制常驻，且本期无业务接口 | `cargo run -p platform-core` 可单独起（`/api/health` 返回 JSON），未做前后端同时在场的完整联调 |
| **Playwright 浏览器版本跨机可移植性** | 本机已有 chromium-1208，E2E 直接跑通 | 换机器首次跑 E2E 需 `pnpm exec playwright install chromium`（`@playwright/test` 固定 1.58.0）。升级 Playwright 会触发 chromium-1255 下载，本机网络下会卡住 |
| **AC-S-008 的 MSW mock / 列表页 / 表单页** | BA 已把该部分收窄下推到 REQ-011 / REQ-012，本期 AC 只验「前端链路不依赖 Rust 后端常驻」 | 本期不需要补做。E2E 1/1 + Rust 进程采样恒为 0 满足收窄后的口径 |

## 偏离清单（需求/指南未提及但必要的改动）

1. ~~`.cargo/config.toml` 未写 `linker = "rust-lld"`~~ **已撤除**（偏离不存在，见问题台账）。
2. 新增 `[alias] xtask = "run --package xtask --"`（BA 给的模板未含此项，但 `cargo xtask check-modules` 需要它）。
3. `crates/platform-core` 额外带一个最小二进制 `platform-core`（`/api/health`），否则「`bun run dev` 起后端」无对象可起。
4. `deploy/seed/*.sql` 与 `scripts/*.sh` 为实现「种子数据初始化 / 统一命令」而新增（BA 模板未展开脚本实现）。
5. `apps/shell` 依赖 **3** 个 workspace 包（`@base/i18n` `@base/sdk` `@base/ui`），并在 `vite.config.ts` 用 alias 直指各包 `src/index.ts` —— 让 `pnpm dev` 不必先构建。上一轮声明了 7 个，其中 4 个（`auth`/`hooks`/`schema`/`theme`）在本期无任何 import，本轮已删。
6. `@playwright/test` 固定为 `1.58.0`（而非最新 1.65.1）—— 本机 Playwright CDN 下载 1255 卡住，1.58.0 对应的 chromium 1208 本机已有。
7. `deploy/seed/00-init.sql` 的建库语句改为条件建库（`SELECT ... \gexec`），因为 `CREATE DATABASE` 不支持 `IF NOT EXISTS`。
8. `scripts/test-integration.sh` 的重置在清空 schema 后，会用 `deploy/seed/00-init.sql` 的 DDL（过滤掉 `\connect`）重建基线表 —— 否则重置后 `base_test` 会丢失 `platform_seed_log`，`bun run seed` 随即失效。

## 提交记录

```text
6968882 [Dev] CI 补 E2E job 并清理无效 token 入参 (关联: REQ-002)
2040f86 [Dev] 清理未使用依赖 (关联: REQ-002)
3c891fe [Dev] 集成测试脚本补测试库重置与可达的容器断言 (关联: REQ-002)
a639df4 [Dev] 修正链接器文档表述 (关联: REQ-002)
c532718 [Dev] 更新验证报告的最终实测数据 (关联: REQ-002)
```

本轮改动规模（相对 QA 复核的 `c532718`）：**8 文件 / +217 -50 行**（另加本报告与追溯矩阵、`CHANGELOG.md`）。
累计规模（相对 `develop`）：88 文件 / +7445 -1 行。
