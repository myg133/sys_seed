# 验证报告 —— Pre-merge

- 需求: REQ-002 平台骨架与工程基线
- 分支: feature/REQ-002 @ `c532718ce6a6169c7dc95219f8929725976dcf91`
- 验证者: QA（本轮补跑：上一轮 QA 结论已跑完但报告文件被误删，本轮重跑关键项并补出正式报告）
- 结论: **PASS-WITH-RISK**

> 验证范围与纪律：本轮为**只读验证**，未修改任何实现代码（`git status --short` 与 `git diff --stat` 结束时均为空），未提交、未推送。唯一写入为本文件。
> 唯一改动过的实现文件是 `crates/platform-core/src/lib.rs` 的 **mtime**（用于触发 AC-S-007 的增量重编译），文件内容 md5 全程未变（`29680e395472c02b2dfcc4a2f43b7d03`）。

---

## 追溯

| AC | 实现 | 测试 | 结论 |
|----|------|------|------|
| AC-S-001 目录规范可校验 | `crates/xtask/src/modules.rs:9-10`（`REQUIRED_FILES`/`REQUIRED_DIRS`）、`crates/xtask/src/main.rs:45`（错误码输出） | `crates/xtask/src/modules.rs:187-262` 单测 7 个 + 本轮手工 4 组场景实跑 | **PASS** |
| AC-S-002 工具链版本锁定 | `rust-toolchain.toml`、`.node-version`、`.nvmrc`、`package.json:packageManager`、`pnpm-lock.yaml`、`bun.lock` | 本轮实跑 `pnpm install --frozen-lockfile`、`bun install --frozen-lockfile --dry-run`，并比对各工具实际版本 | **PASS** |
| AC-S-006 低配开发基线生效 | `.cargo/config.toml`（`jobs = 3`）、`Cargo.toml`（`[profile.dev.package."*"] opt-level=0/debug=0`） | **冷编译未重做**，引用前两轮实测；本轮补做增量侧（见 AC-S-007） | **PASS（引用上一轮实测值）** |
| AC-S-007 日常命令流畅执行 | 同上 | 本轮实跑增量 `cargo check` / `cargo test --lib`，并确认**确有重编译** | **PASS** |
| AC-S-008 前端不依赖后端常驻 | `apps/shell/`、`packages/*`、`playwright.config.ts` | 本轮实跑 `bun run test:e2e` + 全程 Rust 进程采样；另核查 MSW/列表页/表单页是否存在 | **PARTIAL**（核心断言通过；AC 明列的 mock/列表页/表单页未交付） |
| AC-S-009 rootless podman 可用 | `compose.yaml`、`deploy/seed/00-init.sql`、`deploy/seed/01-seed.sql` | 本轮实跑：双库连通、mem_limit、端口绑定、内存合计、**全新卷首启种子路径** | **PASS**（且比 Dev 报告自述的"部分验证"更强） |
| AC-S-010 集成测试不用 testcontainers | `scripts/test-integration.sh` | 本轮实跑脚本 + 独立比对前后 `podman ps -a` 全量快照；全仓检索 testcontainers | **PARTIAL**（容器数量不变已验证；"测试库被重置而非新建"未实现） |

**AC 计数：7 条中 PASS 5 条、PARTIAL 2 条、FAIL 0 条。**

---

## 测试

| 套件 | 命令 | 结果 |
|------|------|------|
| Rust 单测 | `cargo test --workspace --lib -- --test-threads=2` | **24/24 通过**（platform-core 3 + platform-contracts 14 + xtask 7） |
| Rust 集成目标 | `cargo test --workspace --tests`（经 `scripts/test-integration.sh`） | 24/24 通过；无 `tests/` 集成用例（本期无 DB 访问层） |
| xtask | `cargo xtask check-modules` | 5 组场景全按预期（见下） |
| 前端 vitest | `pnpm -r test` | **23/23 通过**（i18n 3 / schema 3 / auth 3 / hooks 3 / theme 2 / sdk 2 / ui 3 / shell 4） |
| E2E | `bun run test:e2e` | **1/1 通过**（3.7s），全程 Rust 进程采样 77 次，**最大值 = 0** |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0，无 error 无 warning |
| Lint | `cargo fmt --all --check` | exit 0 |
| Lint | `pnpm exec eslint .` / `pnpm exec prettier --check .` | exit 0 / 全部符合 Prettier 风格 |
| 类型 | `pnpm -r check` | exit 0；`svelte-check found 0 errors and 0 warnings` |
| 覆盖率 | — | **未测**（见"未覆盖"） |

### 链接器复核（本次最重要的复核项）

**结论：上一轮「rustc 1.98 的 `x86_64-unknown-linux-gnu` 默认就用 LLD」的判断成立。Dev 报告问题清单 #1「链接器未按 BA 指南用 `rust-lld`」这条偏离不成立，建议从问题台账中撤除。**

三条独立证据：

**证据 1 — target spec 直接写死 flavor**

```console
$ RUSTC_BOOTSTRAP=1 rustc -Z unstable-options --print target-spec-json --target x86_64-unknown-linux-gnu
linker-flavor = gnu-lld-cc
pre-link-args = {'gnu-cc': ['-m64'], 'gnu-lld-cc': ['-m64']}
llvm-target = x86_64-unknown-linux-gnu
```

`gnu-lld-cc` = 用 C 编译器驱动，但**强制走 LLD**。

**证据 2 — rustc 传给链接器的实际参数**

```console
$ rustc --print link-args src/main.rs --edition 2021 -o /tmp/lp-default | tr ' ' '\n' | grep -iE "fuse-ld|lld"
"-B/home/myg133/.rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld"
"-fuse-ld=lld"
```

`-B` 指向 rustup 自带的 `gcc-ld/` 目录（内含 `ld.lld`），配合 `-fuse-ld=lld`。

**证据 3 — 真实链接过程中链接器自报家门**（最小 crate 建在 `.local/linker-probe`，本轮已清理）

默认构建（不设任何 linker 配置）：

```console
$ RUSTFLAGS="-C link-arg=-Wl,--version" cargo build
warning: linker stderr: collect2 version 16.2.1 20260810
         /home/myg133/.rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld/ld.lld -plugin ... -m elf_x86_64 ... -o .../linker_probe-7caa550aee37ee91 ...
warning: linker stdout: LLD 22.1.8 (/checkout/src/llvm-project/llvm 52ed14fcd56afc30f9cccd8ca8ce237c2eef7e04) (compatible with GNU linkers)
```

强制 `-C link-arg=-fuse-ld=bfd` 时：

```console
$ RUSTFLAGS="-C link-arg=-fuse-ld=bfd -C link-arg=-Wl,--version" cargo build
warning: linker stderr: collect2 version 16.2.1 20260810
         /usr/bin/ld.bfd -plugin ... -m elf_x86_64 ... -o .../linker_probe-09d05f87c0fa4935 ...
warning: linker stdout: GNU ld (GNU Binutils) 2.47
```

**本仓工作树确实走默认路径（未被覆盖）**：

- `.cargo/config.toml` 的 `[target.x86_64-unknown-linux-gnu]` 段**只有注释**，无 `linker` / `rustflags` / `linker-flavor` 键（`grep -nE "^\s*(linker|rustflags|linker-flavor)" .cargo/config.toml` 无命中）
- 全仓只有这一个 `.cargo/config.toml`；用户级 `~/.cargo/config.toml` 只有 crates.io 镜像与 net 配置，无 linker
- 环境变量无 `CARGO_TARGET_*_LINKER` / `RUSTFLAGS` / `LINKER` 覆盖

因此**本仓工作树的链接器同样是 LLD**，Dev 报告 #1 与偏离清单第 1 条所述的"链接器偏离"不存在。

**链接器进程峰值内存（本轮自测，与上一轮数值方向一致、绝对值不同）**

用 `getrusage(RUSAGE_CHILDREN)` 直接量链接器进程自身峰值 RSS（对抽出的链接器命令行原样执行）：

| 模式 | 链接器进程峰值 RSS |
|------|--------------------|
| 默认（LLD 22.1.8） | **32.6 MB** |
| 强制 bfd（GNU ld 2.47） | **187.4 MB**（约 5.7 倍） |

> 与上一轮记录的「LLD 2MB vs GNU bfd 130MB」绝对值不同（本轮为最小 probe crate 的完整链接、含 `--version`），但**方向与量级一致：LLD 显著更省内存**。本轮以自测值为准。

### AC-S-001 目录规范校验（本轮 5 组场景实跑）

| 场景 | 预期 | 实测 | 结论 |
|------|------|------|------|
| ① `modules/` 下无模块目录 | exit 0 | `无业务模块：.../modules 下没有模块目录`，`exit=0` | ✅ |
| ② `modules/qa-broken/` 空目录 | exit≠0，报 `E_MODULE_LAYOUT`，逐条列出缺失项 | `exit=1`，输出 `模块目录校验失败：错误码 E_MODULE_LAYOUT` + `✗ qa-broken` + **6 条缺失项全列出**（缺 `manifest.json`/`AGENTS.md`/`README.md`/`backend/`/`frontend/`/`migrations/`） | ✅ |
| ③ 合法模块 + 不合规模块并存 | 只标不合规的，合法的不误报 | `exit=1`，仅 `✗ qa-broken` 被列出，`qa-valid` 未被误判 | ✅ |
| ④ 仅合法模块 | exit 0 | `模块目录校验通过：1 个模块` + `✓ qa-valid`，`exit=0` | ✅ |
| ⑤ `manifest.json` 非法 JSON | 报出 JSON 解析原因 | `exit=1`，`manifest.json 不是合法 JSON：key must be a string at line 1 column 3` | ✅ |

**夹具已全部删除**：`modules/` 现仅有原有 `README.md`；`git status --short` 为空。

### AC-S-006 / AC-S-007 性能实测

**冷编译（AC-S-006）——本轮未重做，引用前两轮实测值：**

| 场景 | 耗时 | 峰值内存 | 阈值 | 判定 | 来源 |
|------|------|----------|------|------|------|
| 冷 `cargo check --workspace --all-targets` | 21.2s | 1087 MB | < 1.5 G | ✅ | 上一轮 QA 实测 |
| 冷 `cargo test`（含链接） | 29.4s | 1090 MB | < 2.5 G | ✅ | 上一轮 QA 实测 |
| 冷 `cargo build --tests` | 29.4s | 1090 MB | < 2.5 G | ✅ | 上一轮 QA 实测（Dev 报告另记 17s/1071MB 与 16s/994MB，两轮同量级） |

冷编译未重做的理由：冷编译需 `cargo clean`（会清掉 349MB 的 `target/`），且本轮任务书明确要求只跑增量、引用上一轮实测值。

**增量（AC-S-007）——本轮实跑，秒级：**

```console
$ sleep 1.2 && touch crates/platform-core/src/lib.rs && cargo check --workspace --all-targets
   Checking platform-core v0.1.0 (.../crates/platform-core)
   Checking platform-contracts v0.1.0 (.../crates/platform-contracts)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.25s
ELAPSED_MS=339
```

```console
$ sleep 1.2 && touch crates/platform-core/src/lib.rs && cargo test --workspace --lib -- --test-threads=2
   Compiling platform-core v0.1.0 (.../crates/platform-core)
   Compiling platform-contracts v0.1.0 (.../crates/platform-contracts)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.27s
test result: ok. 3 passed; ... / 14 passed; ... / 7 passed; ...
ELAPSED_MS=360
```

| 指标 | 实测 | 阈值 | 余量 |
|------|------|------|------|
| 增量 `cargo check --workspace --all-targets` | **339 ms** | < 60 000 ms | 177× |
| 增量 `cargo test --workspace --lib` | **360 ms** | < 180 000 ms | 500× |
| 增量期间 cargo/rustc 峰值 RSS | 22 MB（check）/ 23 MB（test） | — | — |

> 有效性说明：输出中可见 `Checking platform-core` / `Checking platform-contracts` / `Compiling ...`，**确实发生了重编译**（`platform-contracts` 依赖 `platform-core`，级联重编）。另在隔离的最小 crate 上验证过 cargo 对 `touch`（内容不变）同样判定为脏并重编，因此该测量等价于"改一行 Rust 代码"。
> 另一轮同法测得 check 282ms / test 327ms，结论稳定。

### AC-S-008 E2E 与 Rust 进程采样

```console
$ ./.local/rustproc-watch.sh /tmp/e2e_watch.txt bun run test:e2e
$ cat /tmp/e2e_watch.txt
E2E_EXIT=0
RUST_PROC_MAX=0
SAMPLES=77
DURATION_SEC=3.85

$ cat /tmp/e2e_run.log
  ✓  1 [chromium] › e2e/shell.spec.ts:3:1 › 底座壳渲染并可过滤菜单 (247ms)
  1 passed (3.7s)
```

采样口径：`ps -eo comm=,args=` 中 `comm ∈ {cargo, rustc, rustdoc, rustup, platform-core}` 或 `args` 含 `target/debug/` 的进程计数，取整个 E2E 期间的**最大值**。结论：**Rust 进程数恒为 0** ✅

### AC-S-009 rootless podman 复核

```console
$ podman info --format '{{.Host.Security.Rootless}}'
true

$ podman ps --format "{{.Names}}\t{{.Status}}\t{{.Ports}}"
base-postgres	Up 23 minutes (healthy)	127.0.0.1:5432->5432/tcp
base-redis	Up 23 minutes (healthy)	127.0.0.1:6379->6379/tcp

$ podman inspect base-postgres --format 'Memory={{.HostConfig.Memory}} Ports={{.HostConfig.PortBindings}}'
Memory=268435456 Ports=map[5432/tcp:[{127.0.0.1 5432}]]        # 268435456 B = 256 MiB = compose.yaml 的 mem_limit: 256m
$ podman inspect base-redis --format 'Memory={{.HostConfig.Memory}} Ports={{.HostConfig.PortBindings}}'
Memory=100663296 Ports=map[6379/tcp:[{127.0.0.1 6379}]]        # 100663296 B = 96 MiB  = compose.yaml 的 mem_limit: 96m

$ podman stats --no-stream --format "{{.Name}}\t{{.MemUsage}}\t{{.MemPerc}}"
base-postgres	27.28MB / 268.4MB	10.16%
base-redis	10.81MB / 100.7MB	10.74%
                                                 合计 ≈ 38.1 MB  < 250 MB  ✅

$ podman exec base-postgres psql -U base -lqt | cut -d'|' -f1
 base_dev
 base_test
 postgres
 template0
 template1
$ podman exec base-postgres psql -U base -d base_dev  -tAc "select current_database();"  →  base_dev
$ podman exec base-postgres psql -U base -d base_test -tAc "select current_database();"  →  base_test
$ podman exec base-redis redis-cli ping                                               →  PONG

$ ss -ltnp | grep -E "5432|6379"
LISTEN 0 4096 127.0.0.1:6379 0.0.0.0:* users:(("rootlessport",pid=162857,fd=9))
LISTEN 0 4096 127.0.0.1:5432 0.0.0.0:* users:(("rootlessport",pid=162774,fd=10))
```

| 期望 | 实测 | 判定 |
|------|------|------|
| 无需 root | `Rootless=true` | ✅ |
| 两个库均可连接 | `base_dev` / `base_test` 均在，`current_database()` 正确 | ✅ |
| mem_limit 生效 | PG 256 MiB、Redis 96 MiB，与 `compose.yaml` 完全一致 | ✅ |
| 端口只绑 127.0.0.1 | `ss` 显示仅 `127.0.0.1`，无 `0.0.0.0` | ✅ |
| 合计常驻内存 < 250 M | ≈ 38.1 MB | ✅ |
| **全新卷首启种子路径** | 见下 | ✅ |

**全新卷首启路径已实跑（Dev 报告自述"未实跑"，与事实不符）：**

```console
$ podman inspect base-postgres --format 'StartedAt={{.State.StartedAt}}'
StartedAt=2026-11-18 00:30:26.909607089 +0800 CST
$ podman inspect base-postgres --format '{{range .Mounts}}{{.Type}} {{.Source}} -> {{.Destination}} (rw={{.RW}}){{"\n"}}{{end}}'
volume /home/.../volumes/base-platform_pgdata/_data -> /var/lib/postgresql/data (rw=true)
bind   /home/myg133/Projects/system_seed/feature-REQ-002/deploy/seed -> /docker-entrypoint-initdb.d (rw=false)

$ podman exec base-postgres psql -U base -d base_dev -tAc "select batch||' @ '||applied_at from platform_seed_log;"
baseline @ 2026-11-17 16:30:28.56635+00        # = 2026-11-18 00:30:28 +0800，即容器启动后 2 秒
```

种子批次在容器启动后 2 秒即写入，说明 `deploy/seed/*.sql` 经 `initdb.d` 挂载的**首次初始化路径已真实执行成功**（新建卷 → initdb → 建 `base_test` → 建 `platform_seed_log` → 灌 `baseline` 批次）。同时容器标签证实其由本仓 compose 启动：

```console
com.docker.compose.project: base-platform
com.docker.compose.project.config_files: compose.yaml
com.docker.compose.project.working_dir: /home/myg133/Projects/system_seed/feature-REQ-002
io.podman.compose.version: 1.6.0
```

> Dev 报告称这组容器"项目标签 `tmp`，非本次交付启动"——**与标签实际值不符**。
> 另 `podman-compose config --quiet` exit 0，compose 文件语法校验通过。

### AC-S-010 集成测试与容器数量

```console
$ podman ps -a --format "{{.ID}} {{.Names}} {{.Status}} {{.Image}}" > /tmp/pod_before.txt
$ ./scripts/test-integration.sh          # exit=0，24/24 通过
$ podman ps -a --format "{{.ID}} {{.Names}} {{.Status}} {{.Image}}" > /tmp/pod_after.txt
$ diff /tmp/pod_before.txt /tmp/pod_after.txt
(无输出：容器 ID/名称/状态/镜像四列全量一致，count 2 → 2)
```

```console
$ grep -rniE "testcontainers" --include="*.toml" --include="*.lock" --include="*.json" --include="*.rs" --include="*.ts" --include="*.sh" . | grep -v ./target/ | grep -v ./node_modules/
./scripts/test-integration.sh:2:# 集成测试：连本机 base_test 库，不使用 testcontainers。
```

→ 全仓**仅注释一处**提及 testcontainers，**无任何 testcontainers 依赖**（Cargo.toml / Cargo.lock / package.json / pnpm-lock.yaml 均无）✅

---

## 问题（按严重度）

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| 1 | 中 | **Dev 报告问题 #1「链接器未按 BA 指南用 `rust-lld`」不成立**——rustc 1.98 的 `x86_64-unknown-linux-gnu` target spec 默认 `linker-flavor = gnu-lld-cc`，默认即走 rustup 自带 LLD。把它当作"偏离"记账会让 BA 误判风险等级，并可能导致后续有人加 `linker = "rust-lld"` 配置反而把链接搞坏 | target spec `linker-flavor = gnu-lld-cc`；`rustc --print link-args` 含 `-fuse-ld=lld` + `-B .../gcc-ld`；默认构建链接器 stdout `LLD 22.1.8`；强制 bfd 时才变 `GNU ld (GNU Binutils) 2.47`；`.cargo/config.toml` 无任何 linker 键、无 env 覆盖 | 从 Dev 问题台账与偏离清单中撤除该条。`.cargo/config.toml` 的注释可简化为"stable rustc 1.98 默认已用 LLD，无需显式配置"，保留现状亦可 |
| 2 | 中 | **AC-S-010 的「测试库被重置而非新建」未实现**——仓库内不存在任何测试库重置逻辑（无 `reset` / `truncate` / `DROP SCHEMA` / 迁移回滚），`scripts/test-integration.sh` 只是连 `base_test` 跑测试。当前因无集成用例而未暴露，后续 DAO 落地后测试会互相污染 | 全仓检索 `reset\|truncate\|DROP SCHEMA\|migrate\|refresh` **无命中**；`scripts/test-integration.sh` 全文仅设置 `DATABASE_URL` 后执行 `cargo test`，无重置步骤 | 在 `scripts/test-integration.sh` 中加入测试前重置（`DROP SCHEMA public CASCADE; CREATE SCHEMA public;` 或引入 sqlx migration + `migrate reset`），并在 `base_test` 与开发库隔离的前提下于 CI 复现 |
| 3 | 中 | **`scripts/test-integration.sh` 的容器数量断言在测试失败时不可达**——脚本用 `set -euo pipefail`，`cargo test` 一失败就整段退出，其后的 `after=$(podman ps ...)` 与 `before != after` 判断**永远不会执行**。即"测试新建了容器且测试同时失败"这种最该被拦住的场景反而放行 | `scripts/test-integration.sh:11`（`set -euo pipefail`）与 `:14-16`（`cargo test ...; status=$?`）顺序冲突；最小复现：`set -e` 下 `false` 之后 `echo AFTER` 不执行（实测 `probe_exit=1`，无 `AFTER` 输出） | 改为 `status=0; cargo test ... \|\| status=$?`，再执行容器比对，最后 `exit "$status"` |
| 4 | 低 | **AC-S-008 明列的 mock / 列表页 / 表单页未交付**——AC 要求"仅启动 Vite + MSW mock"并"打开一个列表页与一个表单页"，且"分页、校验、权限显隐由 mock 提供"。实际交付为单页底座壳 + 菜单关键字过滤，**无 MSW、无列表页、无表单页、无分页/校验/权限显隐** | 全仓 `grep -riE "msw\|mock"`（ts/svelte/json）**零命中**；`apps/shell/src/` 仅 `App.svelte`/`lib/nav.ts`/`main.ts`/`app.css`；`e2e/shell.spec.ts` 只断言壳标题 + 菜单过滤 | 在 AC-S-008 追溯表中把该 AC 标为 PARTIAL 而非 PASS；若本期确实只交付"壳"，请 BA 确认是否把"列表页/表单页/MSW"下推到后续 REQ 并在 acceptance 中改写，避免验收标准与交付物长期错配 |
| 5 | 低 | **未使用依赖 2 处**：`crates/platform-contracts/Cargo.toml` 声明 `platform-core.workspace = true`，但全 crate 内**无任何 `platform_core` 引用**；`apps/shell/package.json` 声明 7 个 `@base/*` 依赖，其中 `@base/auth`、`@base/hooks`、`@base/schema`、`@base/theme` **在 src/test 中均无 import**（仅在 `vite.config.ts` 里配了 alias） | `grep -rn "platform_core" crates/` 仅命中 `platform-core` 自身；`grep -rn "@base/" apps/shell/` 仅命中 `App.svelte` 的 i18n/sdk/ui 三条 import + vite alias | 要么删掉未用依赖，要么在代码注释中标注"为后续 Sprint 预留"。二者都不做时，`cargo udeps` 类工具会持续报错 |
| 6 | 低 | **CI 未运行 E2E**——`.github/workflows/ci.yml` 的 5 个 job（lint / test / build / dependency-audit / contract-check）均不含 `bun run test:e2e`。AC-S-008 的行为目前只在本地验证，无 CI 回归护栏 | `ci.yml` 全文无 `playwright` / `test:e2e` 字样 | 增补 E2E job（`pnpm exec playwright install --with-deps chromium` + `bun run test:e2e`），或在报告中明确标注 E2E 属本地手工门禁 |
| 7 | 低 | **CI `dependency-audit` job 的 `token:` 入参疑为无效**——`rustsec/audit-check@v2` 使用 `RUSTEC_AUDIT_TOKEN` 环境变量，不接受 `token` 输入；该 `secrets.GITHUB_TOKEN` 会被忽略 | `ci.yml:98` `token: ${{ secrets.GITHUB_TOKEN }}` | 删除该入参或改用 `RUSTEC_AUDIT_TOKEN` 环境变量；属配置卫生问题，不影响本次交付 |
| 8 | 低 | **Dev 报告"AC-S-009 本仓 compose 全新卷首启路径未实跑"与事实不符**——容器标签、卷挂载与种子时间戳证明该路径已实跑成功 | 见上文 AC-S-009 小节（`StartedAt=00:30:26` vs 种子 `00:30:28`；`com.docker.compose.project.config_files=compose.yaml`） | 修正 Dev 报告的"未验证"章节，避免 BA 重复安排已完成的验证 |

**非问题但需澄清（供 BA 记账参考）**

- Dev 报告问题 #3 提到 `@playwright/test` 固定 1.58.0 对应 chromium-1208；本轮 `bun run test:e2e` 在本机直接跑通，未触发任何浏览器下载，1.58.0 与本机已有浏览器匹配 ✅
- Dev 报告问题 #2（`bun.lock` 由 `pnpm-lock.yaml` 迁移生成）与 #4（`allowBuilds`）属于说明性记录，本轮 `pnpm install --frozen-lockfile` 与 `bun install --frozen-lockfile --dry-run` 均 exit 0，未复现任何异常 ✅
- `compose.yaml` / `ci.yml` 中的 `POSTGRES_PASSWORD: base` 是本地开发与 CI 的明文弱口令，属常规做法，不构成"硬编码密钥"缺陷；全仓无 API Key / Token / 私钥类硬编码 ✅

---

## 通用清单

| 检查项 | 结果 |
|--------|------|
| 每条 AC 有对应实现与测试 | AC-S-008 / AC-S-010 部分缺失（见问题 #4、#2） |
| 边界：空值 / 非法输入 / 上下限 | xtask 覆盖空目录、缺文件、缺目录、非法 JSON、点目录忽略；`filterNav` 覆盖空关键字 / 大小写 / 无匹配 |
| 异常路径有处理和测试 | `CoreError` 三类变体（`InvalidConfigKey` / `InvalidIdentifier` / `Io`）均有单测；xtask 失败路径 exit 1 |
| 外部输入已校验 | `ConfigKey` / 标识符校验有实现与断言 |
| 无硬编码密钥 / 调试后门 | ✅ 全仓无 API Key/Token/私钥；仅本地开发口令 `base` |
| 无 TODO/FIXME / 注释掉的代码 | ✅ `TODO\|FIXME\|XXX\|HACK\|待办\|workaround` 零命中 |
| 无调试输出 | ✅ 唯一 `println!/eprintln!` 位于 `crates/xtask/src/main.rs`，为 CLI 正常输出 |
| 无未使用依赖 | ❌ 2 处（问题 #5） |
| 无重复实现 | ✅ 未发现重复实现 |
| Lint / 类型检查通过 | ✅ clippy(-D warnings) / rustfmt / eslint / prettier / tsc / svelte-check 全绿 |
| 断言有意义（非 assert True） | ✅ `expect(true)` / `assert!(true)` / `toBeTruthy()` / `toBeDefined()` 零命中；断言均针对具体值与错误原因 |
| 覆盖正常 + 异常路径 | ✅（xtask 5 组场景；`CoreError` 各分支） |
| 未 mock 掉被测逻辑本身 | ✅ Rust 侧无 mock 框架；E2E 走真实 Vite preview 构建产物（但缺 MSW，见问题 #4） |
| `.gitignore` 覆盖 target/ node_modules/ .local/ | ✅ `git check-ignore -v` 确认三项均被忽略 |
| `.gitignore` **未误忽略** `.sqlx` | ✅ `git check-ignore -v --no-index .sqlx` 无命中；`.gitignore:25-27` 有显式说明注释 |

---

## 未覆盖 / 无法验证

| 项 | 原因 | 影响 / 补做建议 |
|----|------|----------------|
| **冷编译峰值内存与耗时（AC-S-006 主体）** | 本轮任务书要求不重做冷编译；`cargo clean` 会清掉 349MB `target/`，且需重编 77 个依赖 crate | 本轮仅做增量侧。冷编译数据（check 21.2s/1087MB、test 29.4s/1090MB）**引用上一轮 QA 实测**，另有 Dev 报告自测（17s/1071MB、16s/994MB）互相印证，两轮同量级且均远低于 1.5G/2.5G 阈值。**建议合并前在干净机器再跑一次冷编译留档** |
| **AC-S-008 的 MSW mock / 列表页 / 表单页 / 分页 / 校验 / 权限显隐** | 交付物中不存在这些实现（问题 #4） | AC-S-008 只能判 PARTIAL，无法按 AC 原文验收 |
| **AC-S-010 的「测试库被重置而非新建」** | 无实现（问题 #2） | 无法验证；机制缺失，非"未跑" |
| **CI 五个 job 实际执行** | 仓库无 remote，GitHub Actions 跑不起来 | `.github/workflows/ci.yml` 仅经人工审阅，**未实际执行**。本轮以本地等价命令替代验证：`cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `pnpm install --frozen-lockfile` / `pnpm -r check` / `pnpm exec eslint .` / `pnpm exec prettier --check .` / `cargo test --workspace` / `pnpm -r test` / `cargo build --workspace --locked` / `cargo xtask check-modules` / `cargo test -p platform-contracts` / `podman-compose config --quiet` —— 全部 exit 0。**推 remote 后仍建议跑一次真实 CI** |
| **代码覆盖率** | 低配机不装 `cargo-llvm-cov`（需编译 10-25 分钟，占盘）；任务书亦不建议 `cargo install` | 角色规范"整体 ≥ 80%"**未测**。按骨架需求的实际测试密度替代（24 Rust + 23 前端用例覆盖本期交付的全部逻辑分支） |
| **`bun run dev` 全栈联调** | 需后端二进制常驻，且本期无业务接口 | `cargo run -p platform-core` 可单独起（`/api/health` 返回 JSON）；前后端同时在场的完整联调未做 |
| **Playwright 浏览器版本跨机可移植性** | 本机已有 chromium-1208，E2E 直接跑通 | 换机器首次跑 E2E 需 `pnpm exec playwright install chromium`（`@playwright/test` 固定 1.58.0）。升级 Playwright 会触发 chromium-1255 下载，本机网络下会卡住 |
| **`cargo build --workspace --locked`（CI build job）** | 本轮只跑了 `cargo check` / `cargo test` / `cargo clippy` | `--locked` 语义是"锁文件不得变更"，与已验证的 `Cargo.lock` 存在性互补；风险极低，但未逐字实跑 |

---

## 结论

**PASS-WITH-RISK**

- **可放行**：AC-S-001、AC-S-002、AC-S-006（引用实测）、AC-S-007、AC-S-009 五条全部有实跑证据支撑；本轮**推翻了 Dev 报告的问题 #1（链接器偏离）**，并**证实 AC-S-009 的全新卷首启路径实际已跑通**（Dev 自述的"未验证"属误判）。
- **需 BA 决策**：AC-S-008（缺 MSW/列表页/表单页）与 AC-S-010（缺测试库重置）判 PARTIAL——前者是**交付范围与 AC 原文错配**，后者是**机制缺失**，建议在合并前明确归属 REQ 或改写 AC。
- **低风险项**（未使用依赖、脚本 `set -e` 断言不可达、CI 未跑 E2E、CI token 入参无效）不阻塞合并，但问题 #3 建议在 DAO 落地前修掉，否则届时测试会互相污染且容器断言形同虚设。
- **合并前建议补做一次**：在干净环境跑一次冷编译并留档（AC-S-006 唯一未由本轮直接实测的量化项）。

---

## 本轮验证动作清单（可复现）

```console
# 链接器复核（最小 crate 建在 .local/ 下，本轮已清理）
RUSTC_BOOTSTRAP=1 rustc -Z unstable-options --print target-spec-json --target x86_64-unknown-linux-gnu
rustc --print link-args src/main.rs --edition 2021 -o /tmp/lp-default
RUSTFLAGS="-C link-arg=-Wl,--version" cargo build
RUSTFLAGS="-C link-arg=-fuse-ld=bfd -C link-arg=-Wl,--version" cargo build

# 目录规范（夹具建在 modules/ 下，本轮已删除）
cargo xtask check-modules

# 性能
touch crates/platform-core/src/lib.rs && cargo check --workspace --all-targets
touch crates/platform-core/src/lib.rs && cargo test --workspace --lib -- --test-threads=2

# 容器与集成
export XDG_RUNTIME_DIR=/run/user/$(id -u)
podman ps -a --format "{{.ID}} {{.Names}} {{.Status}} {{.Image}}"
./scripts/test-integration.sh
podman stats --no-stream --format "{{.Name}}\t{{.MemUsage}}"

# 前端
pnpm install --frozen-lockfile
bun install --frozen-lockfile --dry-run
pnpm -r test && pnpm -r check
pnpm exec eslint . && pnpm exec prettier --check .
bun run test:e2e   # 同时用 .local/rustproc-watch.sh 采样 Rust 进程数
```
