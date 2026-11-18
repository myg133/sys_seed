# 低配开发环境指南

本文是 BA 低配开发指南（正本在 BA 分支 `demand`，用 `git show demand:plan/dev-guide-low-spec.md` 读）的仓库正式版，供所有开发者按同一套参数工作。

## 0. 目标机器画像

| 项 | 实测 | 判断 |
|---|---|---|
| CPU | 8 线程 @1.8GHz（i7-8550U，4 核） | 单核性能一般，**编译是时间瓶颈** |
| 内存 | 7.7G 总 / 2.8G 已用 / **4.9G 可用** | **真正的瓶颈** |
| Swap | 4G zram + 4G swapfile = 8G | 压缩内存交换，比磁盘 swap 快，是安全垫 |
| 磁盘 | 126G 可用 | 充足，但 `target/` 需控制在 8G 以内 |
| 桌面常驻 | minimax-code + v2rayN + herdr-app + Xorg ≈ 1.5G | 编译前可关 v2rayN/herdr-app 腾出 ~800M |

**核心策略：同一时刻只跑一个「重」任务。** 编译、Storybook、Playwright 三者不要同时在场。

### 内存预算表

| 组件 | 常驻内存 | 何时开 |
|---|---|---|
| 桌面 + 系统 + agent | 2.8G | 一直在 |
| PostgreSQL（容器，mem_limit 256M） | 120-180M | 常驻（迁移与集成测试要用） |
| Redis（容器，mem_limit 96M） | 15-30M | 常驻 |
| `cargo check` 峰值 | 0.8-1.5G | 写 Rust 时 |
| `cargo build` 峰值 | 1.5-2.5G | **链接期是最高点** |
| Vite dev server | 300-450M | 写前端时 |
| `svelte-check --watch` | 500M-1G | **不要常开** |
| Storybook | 1-1.5G | **不要常开** |
| Playwright chromium | 300-500M | 验收时串行跑 |

**验算**：桌面 2.8G + PG 0.15G + Vite 0.4G + `cargo check` 1.5G ≈ **4.85G** → 安全。
危险组合（桌面 2.8G + Storybook 1.5G + Playwright 0.5G + `cargo build` 2.5G）≈ **7.3G** → 触顶，开始疯狂吃 swap。

## 1. 工具链锁定

| 工具 | 版本 | 锁定文件 |
|---|---|---|
| Rust | 1.98.1 | `rust-toolchain.toml` |
| Node | 26.13.1 | `.node-version` / `.nvmrc` |
| pnpm | 12.19.0 | `package.json` 的 `packageManager` |
| Bun | 1.4.4 | `bun.lock`（bun 仅作脚本 runner） |
| PostgreSQL | 17 | `compose.yaml` |
| Redis | 7 | `compose.yaml` |

新克隆仓库后：

```bash
pnpm install          # 安装前端依赖（pnpm 硬链接共享，别用 npm）
cargo check           # 自动按 rust-toolchain.toml 拉取指定 Rust 版本
```

**不装 `cargo install` 任何工具**（cargo-nextest / sqlx-cli / sccache 每个都要编译 10-25 分钟且占盘），改用原生替代：

| 想用 | 先用替代 |
|---|---|
| `cargo nextest` | `cargo test -- --test-threads=2` |
| `cargo sqlx prepare` | 开发期让 PG 常驻，直接用编译期校验；`SQLX_OFFLINE=true` 留给 CI |
| `sccache` | Cargo 自带 incremental 已够（单机场景收益小） |

## 2. 仓库级编译配置

`.cargo/config.toml`：

```toml
[build]
jobs = 3

[alias]
xtask = "run --package xtask --"
```

`Cargo.toml` 的 profile：

```toml
[profile.dev]
debug = "line-tables-only"
split-debuginfo = "unpacked"
incremental = true

[profile.dev.package."*"]
opt-level = 0
debug = 0

[profile.release]
lto = "thin"
codegen-units = 16
strip = "debuginfo"
```

### 2.1 链接器：stable rustc 默认已用 LLD，无需显式配置

BA 指南写的是「链接器用 `rust-lld` + `-C link-self-contained=+linker`」。
**这条在 stable rustc 1.98 上不需要显式配置** —— 因为 rustc 1.98 的
`x86_64-unknown-linux-gnu` target spec 默认就是：

```console
$ RUSTC_BOOTSTRAP=1 rustc -Z unstable-options --print target-spec-json --target x86_64-unknown-linux-gnu
"linker-flavor": "gnu-lld-cc",
```

`gnu-lld-cc` = 用 C 编译器驱动，但**强制走 LLD**。rustc 实际传给链接器的参数：

```console
$ rustc --print link-args probe.rs --edition 2021 -o /tmp/probe | tr ' ' '\n' | grep -iE 'fuse-ld|lld'
"-B/home/myg133/.rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld"
"-fuse-ld=lld"
```

`-B` 指向 rustup 自带的 `gcc-ld/` 目录（内含 `ld.lld`），配合 `-fuse-ld=lld`。
真实链接过程中链接器自报家门（探针 crate 建在 `.local/` 下，测完即删）：

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

**结论：仓库里一个 linker 键都不写，默认就已经在用 LLD。**

链接器进程峰值内存 —— **口径：内核 `VmHWM` 高水位 + `wait4` 的 `ru_maxrss` 双仪器互证**（无采样缺口），
负载是 `platform-core` 真实仓库二进制（cargo 自己生成的 rustc 命令行原样重放）：

| 模式 | 链接器 | 峰值内存 |
|------|--------|---------|
| **默认（仓库现状）** | LLD 22.1.8 | **132.0 MB**（`VmHWM` 135144 KB / `ru_maxrss` 130.9 MB） |
| 强制 `-C link-arg=-fuse-ld=bfd` | GNU ld 2.47 | **135.5 MB**（`VmHWM` 138772 KB / `ru_maxrss` 135.5 MB） |

两者只差 **2.6%**。另注：rustup 自带的 `ld.lld` 动态链接 `libLLVM`，空载地板就有 **32.8 MB**
（`ld.lld --version` 的 `ru_maxrss`；`ld.bfd` 空载仅 7.6 MB）。

> ⚠️ 上一版写的「LLD 16.1 MB / bfd 134.0 MB（约 8 倍）」**已作废**：那是用**瞬时 `VmRSS` 采样**
> （会漏掉峰值）配**合成探针 crate** 得出的 —— `16.1 MB` 甚至低于 `ld.lld` 的空载地板，物理上不可能。
> 现表是复核轮在真实仓库二进制上的无采样缺口口径实测值。

**结论不变，但理由要改**：选 LLD 是因为它是 stable rustc 1.98 的**默认行为**
（`linker-flavor = gnu-lld-cc`），**不需要任何配置** —— 而**不是**因为省内存，实测内存差异只有 2.6%。
（AC-S-006 卡的是端到端编译峰值，量级在 1 GB 左右，见 §2.3 与验证报告，与本表的链接器单进程占用不是一回事。）

> ⚠️ **显式配置反而会把链接搞坏**，所以不要加 `linker = "rust-lld"` 或 `link-self-contained`：
>
> - `+linker` 是 unstable 写法，stable rustc 1.98 直接拒绝，cargo 一启动就退出：
>
>   ```text
>   error: only `-C link-self-contained` values `y`/`yes`/`on`/`n`/`no`/`off`/`-linker` are stable,
>   the `-Z unstable-options` flag must also be passed to use the unstable values
>   ```
>
> - 即便改成 stable 的 `-linker`（直连 rust-lld），lld 会按自身所在路径推导 sysroot，
>   在 Arch 上找不到 `/usr/lib/gcc/x86_64-pc-linux-gnu/<ver>/libgcc_s.so`
>   （系统 glibc 只在 `/usr/lib` 提供 `libc.so`、`libm.so`，其余 `-lpthread/-ldl/-lrt/-lutil` 只有 `.a`），
>   链接必然失败：
>
>   ```text
>   rust-lld: error: unable to find library -lc
>   rust-lld: error: unable to find library -lgcc_s
>   ```
>
>   要修好得把 gcc 的版本化目录写死进仓库配置（例如 `-L/usr/lib/gcc/x86_64-pc-linux-gnu/16`），
>   那样 CI（Ubuntu 镜像路径不同）与其他开发者都会挂。

链接期内存继续由 `profile.dev` 的调试信息策略控制
（`debug = "line-tables-only"` + `split-debuginfo = "unpacked"` + 依赖 `opt-level = 0`）。
`jobs = 3` 与全部 profile 取值保持 BA 指南原样。

### 2.2 日常命令

```bash
cargo check                          # ★ 主力命令：只类型检查，不生成不链接
cargo clippy --all-targets           # 静态检查（比 check 稍慢，提交前跑）
cargo test -- --test-threads=2       # 限并行，控内存
cargo build --tests                  # 只在要跑测试时 build
```

`cargo check` 跳过代码生成与链接，在本机比 `cargo build` 快 40-50%、内存低 60%。

### 2.3 本机实测（REQ-006 交付时）

| 操作 | 耗时 | 峰值内存 |
|---|---|---|
| 冷 `cargo check --workspace --all-targets`（**179 个 crate**，其中第三方 172） | **63.7s / 66.0s**（两次实测） | **925 MB / 940 MB** |

依赖的 `opt-level = 0` + `debug = 0` 是这两个数字能这么低的主要原因。

> **依赖树的变化（91 → 175 → 179）**
>
> | 阶段 | 工作区成员 | 第三方 crate | 变化 |
> |------|-----------|------------|------|
> | REQ-002 工程基线 | 5 | 91 | — |
> | REQ-005 交付基线（引入 sqlx） | 6 | 172 | **+81**（sqlx 及其传递依赖） |
> | REQ-006 多租户（新建 `platform-db`） | 7 | **172** | **+0 第三方**（只多了工作区成员自己） |
>
> 口径：`cargo tree --workspace --prefix none --all-features | sed 's/ (.*//' | sort -u | wc -l`
> （含工作区成员；第三方数 = 总数 − 工作区成员数）。
>
> **REQ-006 没有引入任何新的第三方依赖**：复用 REQ-005 引入的 `sqlx`，
> **不开 `macros` feature**（`sqlx-macros` 不参与编译）——
> `FromRow` 用 `platform_db::tenant_row!` 声明宏手写实现。
>
> **峰值内存 925–940 MB < 1.5 GB** → `.cargo/config.toml` 的 `jobs` **保持 3**（无需下调）。
> 口径：采样 `cargo` 及其 `rustc` 子进程的 `VmHWM` 求和（偏保守的上界）。
> 本机没有 `/usr/bin/time`（`time` 是 bash 内建），所以用 `VmHWM` 采样口径。

## 3. 容器与镜像加速

**本机用 podman，不用 docker**：rootless podman 无守护进程，
PG 150M + Redis 15M + conmon/runc 开销 ≈ **30M**，比 dockerd 常驻的 400-600M 省一大截。

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)   # rootless 必需，每次新终端都要设
bun run db:up        # podman-compose -f compose.yaml up -d
bun run db:down      # podman-compose -f compose.yaml down
```

**镜像必须写显式前缀**：`registry-1.docker.io` 直连超时，`~/.config/containers/registries.conf`
里的 `location` 替换实测不生效，只有 compose 里写全地址才稳定。

```yaml
image: docker.m.daocloud.io/library/postgres:17-alpine
image: docker.m.daocloud.io/library/redis:7-alpine
```

实测可达性：

| 源 | 连通性 |
|---|---|
| registry-1.docker.io | ❌ 超时 |
| docker.m.daocloud.io | ✅ 0.12s |
| docker.1ms.run | ✅ 0.33s |

CI 在境外，直接用官方地址 `postgres:17-alpine`，不需要国内镜像。

### 3.1 内存上限

`compose.yaml` 已写死 `mem_limit`：PostgreSQL 256M、Redis 96M，并配了低内存参数
（`shared_buffers=64MB`、`effective_cache_size=128MB`、`work_mem=4MB`、`max_connections=50`），
两个容器常驻内存合计 < 250M（AC-S-009）。

## 4. 数据库与 sqlx 的取舍

- 编译期校验需要活库：开发期保持 PG 常驻（120-180M，很便宜）。
- 离线缓存 `.sqlx` **入库**，CI 里 `SQLX_OFFLINE=true` 编译。

### 4.1 租户隔离的应用角色（RLS 的前提）

⚠️ **PostgreSQL 的 RLS 对超级用户与带 `BYPASSRLS` 属性的角色不生效。**

开发库的 `base` 是**超级用户**（compose 的 `POSTGRES_USER=base`）——
用它连应用，RLS 形同虚设，**租户隔离会 fail open**（数据库层兜底失效）。

所以应用连接必须用**普通角色**：

| 角色 | 用途 |
|------|------|
| `base` | 超级用户：迁移、运维、备份（**不是**应用连接角色） |
| `base_platform_app` | 组角色（NOLOGIN）：持有租户作用域表的权限 |
| `base_app` | 登录角色（开发环境）：`base_app` 是 `base_platform_app` 的成员 |

初始化：`deploy/seed/02-app-role.sql`（幂等；容器首次初始化时自动执行，
存量环境手工执行一次即可）。生产环境由部署流程创建自己的登录角色，
并 `GRANT base_platform_app TO <角色>`。

> **应用连接角色的权限来源**：组角色 + `ALTER DEFAULT PRIVILEGES`
> （之后由 `base` 创建的表自动授权）+ 平台按 manifest 生成的策略里附带的 `GRANT`。

### 4.2 租户作用域策略的生成

```bash
# 生成 / 补齐（按各模块 manifest 的 tenantScoped 段）
DATABASE_URL=postgres://... cargo xtask tenant-policies

# 只校验：有表漏建 policy 就失败（漏建 = 该表完全无隔离）
DATABASE_URL=postgres://... cargo xtask tenant-policies --check
```

⚠️ 策略应用在**一个事务**里完成：`DROP POLICY` + `CREATE POLICY` 分两条语句执行时，
中间有一个「表上没有 policy」的窗口 —— 那一瞬间 RLS 变成**全部拒绝**
（fail closed，不泄漏，但读写全挂）。

```bash
# 改了迁移之后重新生成离线缓存（一次性，约 10-20 分钟）
cargo install sqlx-cli --no-default-features --features rustls,postgres
cargo sqlx prepare --workspace -- --all-targets
git add .sqlx
```

> 依赖集控制是第一个杠杆：Sprint 1 只引入 axum / tokio / serde / serde_json / tracing /
> tracing-subscriber / clap / thiserror / anyhow / uuid / time；
> sqlx / reqwest / aws-sdk / opendal / jsonwebtoken 推到后续 Sprint，
> 冷编译 250 crates 与 450 crates 差距 8 分钟起步。

## 5. 前端：别让 dev server 吃掉内存

```bash
pnpm install
pnpm dev              # Vite dev server，Svelte 5，约 300-450M
pnpm build
pnpm test             # vitest
```

**三条规矩**：

1. **`svelte-check --watch` 不要常开**（500M-1G）。类型检查改成手动 `pnpm check` 或放 CI。
2. **Storybook 不常开**（1-1.5G）。
3. **前端开发用 MSW mock，不连 Rust 后端**（AC-S-008）：前端开发期间 Rust 进程根本不用起。

Playwright 只装 chromium：`pnpm exec playwright install chromium`（约 620M，不要装全套）。

## 6. 测试策略：分层 + 分界

| 层 | 工具 | 内存 | 频率 | 跑在哪 |
|---|---|---|---|---|
| 单元测试 | `cargo test --lib` | <500M | 每次提交前 | 本地 |
| 契约测试 | `cargo test -p platform-contracts` | <500M | 每次提交前 | 本地 |
| 前端单测 | `pnpm test`（vitest） | <500M | 每次提交前 | 本地 |
| 集成测试 | 本机 `base_test` 库 | ~1G | 合并前 | 本地 |
| E2E | Playwright | ~1.5G | **验收时串行** | 本地（不常驻） |
| 全量回归 | 全部 | ~2G | 发布前 | CI |

### 6.1 集成测试不用 testcontainers

testcontainers 会为每个测试环境起新容器 → 额外 200-400M 内存 + 启动等待。
改用同一个 PG 实例的独立库 `base_test`，与开发库 `base_dev` 完全隔离：

```bash
bun run test:integration   # 跑前后对比 podman ps 数量，数量不变才算通过
```

### 6.2 测试数据

- 统一用 `bun run seed` 造数据，不要在测试里手写
- 夹具账号统一用 `alice@example.com` / `bob@example.com` / `both@example.com` / `carol@example.com`
  （账号表由 REQ-007 落地）

### 6.3 E2E 的跑法

```bash
bun run test:e2e                          # 全部
E2E_GREP="登录|权限" bun run test:e2e      # 按标题过滤
bun run test:e2e -- --grep "登录|权限"     # 同上：追加的 --grep 覆盖脚本默认值
```

> 注意：不要用 `bun run test:e2e "登录"` 这种直接追加位置参数的形式 ——
> 脚本自带 `--grep`，追加的位置参数会被 playwright 当成测试文件过滤器而报错。

完整 E2E 回归**只在验收时跑一次**，跑之前先关掉 Storybook / svelte-check / Vite。

## 7. 一天的工作节奏

```text
上午：写 Rust
  └─ 只开 cargo check，不起后端
  └─ 中途起一次 PG（一直常驻）
  └─ 改完跑 cargo check + cargo test --lib

中午：写前端
  └─ 关掉所有 Rust 进程
  └─ 起 Vite + MSW mock（不连后端）
  └─ 改完跑 pnpm test

下午：联调与收尾
  └─ 起后端（cargo run）跑一次主链路
  └─ 跑集成测试
  └─ 提交前跑 clippy + 契约测试
```

## 8. 磁盘控制

用户明确要求：**不要把硬盘跑满**。`/home` 可用 126G，预算上限 20G。

| 占用项 | 预估 | 控制手段 |
|---|---|---|
| podman 镜像（postgres + redis） | ~150M | 固定两个镜像，不加其他 |
| podman 卷（PG 数据） | < 500M | `podman system prune` 可回收 |
| `target/`（Rust 产物） | 1-3G | `debug = "line-tables-only"` + `split-debuginfo = "unpacked"` |
| `node_modules/` | ~105M | pnpm 硬链接共享，勿用 npm |
| Playwright 浏览器 | ~620M | 只装 chromium，不装全套 |
| **合计** | **< 5G** | 预算上限 20G |

**规矩**：

1. 每个 Sprint 结束跑一次 `podman system prune -af` 与 `cargo clean`（若 `target/` > 8G）
2. 只装 chromium 一个 Playwright 浏览器
3. 不用 `cargo install` 装工具链
4. 开发日志/构建产物不进 Git（`.gitignore` 覆盖 `target/` `node_modules/` `.aw/` `*.log`）

**磁盘水位检查**：`bun run disk:check`（`df -h /home` + `du -sh target/` + `podman system df`）。

## 9. 命令速查

| 命令 | 作用 |
|---|---|
| `bun run dev` | 起全栈（前端 + 后端），数据库就绪时自动灌种子 |
| `bun run check` | `cargo check` + `pnpm -r check`（手动触发，不 watch） |
| `bun run lint` | `cargo clippy` + eslint + prettier check |
| `bun run test` | Rust 单测（`--test-threads=2`）+ 前端 vitest |
| `bun run test:integration` | 连 `base_test` 库，不用 testcontainers |
| `bun run test:e2e` | Playwright，带 `--grep` |
| `bun run seed` | 种子数据 |
| `bun run disk:check` | 磁盘水位检查 |
| `bun run db:up` / `db:down` | 起停本地容器环境 |
| `cargo xtask check-modules` | 校验 `modules/<name>/` 目录规范 |
