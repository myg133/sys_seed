# 开发指南：低配环境下的开发与测试

- **适用**: REQ-001 全部子需求的开发
- **目标机器**: 8 线程 i7-8550U / 7.7G 内存 / 8G swap（4G zram + 4G swapfile）/ 126G 可用磁盘
- **工具链现状**: rustc 1.98.1 / bun 1.4.4 / node 26.13.1 / pnpm 12.19.0 / docker 27.2.1（daemon 未运行）/ podman 6.2.0
- **未安装**: cargo-nextest / cargo-sqlx / sccache / cargo-insta / 系统 lld / mold / 本地 postgres
- **更新时间**: 2026-11-17 23:55

---

## 0. 资源画像与结论

| 项 | 实测 | 判断 |
|---|---|---|
| CPU | 8 线程 @1.8GHz（2018 U 系列，4 核） | 单核性能一般，**编译是时间瓶颈** |
| 内存 | 7.7G 总 / 2.8G 已用 / **4.9G 可用** | **真正的瓶颈** |
| Swap | 4G zram + 4G swapfile = 8G | zram 是压缩内存交换，比磁盘 swap 快得多，是安全垫 |
| 磁盘 | 126G 可用 | 充足。`target/` 涨到 15-25G 也没问题 |
| 桌面常驻 | minimax-code 490M + v2rayN 483M + herdr-app 319M + Xorg 137M ≈ **1.5G** | 编译前可关 v2rayN/herdr-app 腾出 ~800M |

**核心策略：同一时刻只跑一个「重」任务。** 编译、Storybook、Playwright 三者不要同时在场。

### 内存预算表

| 组件 | 常驻内存 | 何时开 |
|---|---|---|
| 桌面 + 系统 + agent | 2.8G | 一直在 |
| postgresql（原生） | 120-180M | 常驻（迁移与集成测试要用） |
| `cargo check` 峰值 | 800M - 1.5G | 写 Rust 时 |
| `cargo build` 峰值 | 1.5G - 2.5G | **链接期是最高点** |
| Vite dev server | 300-450M | 写前端时 |
| Rust 后端 `cargo run` | 80-200M | 需要联调时 |
| `svelte-check --watch` | 500M - 1G | **不要常开** |
| Storybook | 1-1.5G | **不要常开**，写组件时临时开 |
| Playwright chromium | 300-500M | 验收时串行跑 |

**验算**：
- 常驻组合（桌面 2.8G + PG 0.15G + Vite 0.4G）+ `cargo check` 1.5G ≈ **4.85G** → 安全
- 危险组合（桌面 2.8G + Storybook 1.5G + Playwright 0.5G + `cargo build` 2.5G）≈ **7.3G** → 触顶，开始疯狂吃 swap

---

## 1. 一次性准备（已实测完成）

### 1.1 容器运行时：podman（不用 docker）

**实测结论：本机 podman 6.2.0 完全可用，是最优选。**

| 检查项 | 结果 |
|---|---|
| rootless | ✅ true |
| 存储驱动 | overlay（文件级，非虚拟机） |
| podman-compose | ✅ 1.6.0 已装 |
| subuid / subgid | ✅ `myg133:100000:65536` |
| newuidmap | ✅ 存在 |
| podman play（内置 kube provider） | ✅ 可用 |

**为什么 podman 而不是 docker**：

| | 预估内存 | 实测 |
|---|---|---|
| docker | dockerd 常驻 400-600M + PG 150M + Redis 15M ≈ **570M** | 未实测（本机 daemon 未运行） |
| podman | 无 daemon，容器由用户态进程直接管理 | ✅ **实测 75MB**（PG 16.6M + Redis 8.8M + conmon/runc 开销约 50M） |

rootless podman 每个容器就是一个受用户态进程管理的普通进程，没有常驻守护进程 ——
这正好对上低配机最缺的那一项资源。**实测比预估还省 60%。**

### 1.1.1 已验证可用的启动方式

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)   # rootless 必需，每次新终端都要设
cd <repo-root>
podman-compose -f compose.yaml up -d
```

**实测结果（2026-11-17）**：

```
base-postgres | Up | 0.0.0.0:5432->5432/tcp |  16.64 MB / 268.4 MB
base-redis    | Up | 0.0.0.0:6379->6379/tcp |   8.79 MB / 100.7 MB
```

- PostgreSQL **17.12** 正常启动，`base_dev` 与 `base_test` 双库已建好
- Redis `PING` 返回 `PONG`
- podman 相关进程总占用 **75MB**
- 宿主机内存：起栈前后 4.9G → 4.6G 可用（Dev 编译同时进行时仍有余量）

**PG 低内存参数**（已生效）：

```
-c shared_buffers=64MB -c work_mem=4MB -c max_connections=30 -c synchronous_commit=off
```

> `synchronous_commit=off` 仅用于**开发环境**：断电可能丢最后几笔事务，但不会损坏数据一致性。
> 生产环境（REQ-005）必须去掉这一项。

### 1.1.2 连接串

```bash
DATABASE_URL=postgres://base:base@localhost:5432/base_dev
DATABASE_URL_TEST=postgres://base:base@localhost:5432/base_test
REDIS_URL=redis://localhost:6379
```

### 1.2 镜像加速（必做，否则直接超时）

**实测：`registry-1.docker.io` 直连超时**，必须走镜像。

| 源 | 连通性 |
|---|---|
| registry-1.docker.io | ❌ 超时 |
| docker.m.daocloud.io | ✅ 0.12s |
| docker.1ms.run | ✅ 0.33s |
| docker.1panel.live | ✅ 2.43s |

已写入 `~/.config/containers/registries.conf`，但**实测按 `location` 替换未生效**，
compose 里必须用**显式镜像地址前缀**才稳定：

```yaml
services:
  postgres:
    image: docker.m.daocloud.io/library/postgres:17-alpine   # ← 显式前缀
  redis:
    image: docker.m.daocloud.io/library/redis:7-alpine        # ← 显式前缀
```

### 1.3 Rust 依赖加速

**实测：crates.io 稀疏索引可达（0.47s），但 .crate 下载仅 265KB/s，明显偏慢。**
rsproxy.cn 索引 0.23s，更快。

已写入**用户级** `~/.cargo/config.toml`（不进仓库，保持仓库可移植）：

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

npm 侧本机已有 `registry=https://registry.npmmirror.com`，无需改动。

### 1.4 免编译的省内存替代品

`cargo-nextest` / `cargo-sqlx` / `sccache` 都得从源码编译，在这台机器上每个 10-25 分钟。
**初期全部不要装**，用原生能力替代：

| 想用 | 先用替代 |
|---|---|
| `cargo nextest` | `cargo test -- --test-threads=2` |
| `cargo sqlx prepare` | 开发期让 PG 常驻，直接用编译期校验；`SQLX_OFFLINE=true` 留给 CI |
| `sccache` | Cargo 自带 incremental 已够（单机场景 sccache 收益小） |
| 系统 lld | rustc 自带 `rust-lld`，无需装包（见 §2.1） |
| mold | 可选，`sudo pacman -S mold`，比 rust-lld 更省内存 |

---

## 2. Rust 编译：把内存和等待时间压下来

### 2.1 项目级配置（写进仓库，REQ-002 落地）

**`.cargo/config.toml`**（放仓库根，所有人共享）：

```toml
[build]
# 不要开满 8 核：并行度换不来速度，但峰值内存线性上涨
jobs = 3

[target.x86_64-unknown-linux-gnu]
# 用 rustc 自带的 rust-lld，不需要装系统 lld 包
# 链接器内存比 GNU ld 低 30-50%，速度也更快
linker = "rust-lld"
rustflags = ["-C", "link-self-contained=+linker"]

# 若装了 mold，改用下面这行（更省内存）：
# rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

**`Cargo.toml`**：

```toml
[profile.dev]
# 调试信息只留行号表：对象文件小一大截，链接期内存大降
debug = "line-tables-only"
# 调试信息单独成文件，链接时不加载进内存（Linux 上效果显著）
split-debuginfo = "unpacked"
incremental = true

[profile.dev.package."*"]
# 第三方依赖一律不优化：省下大量编译时间
opt-level = 0
debug = 0

[profile.release]
lto = "thin"          # 不要 "fat"，内存翻倍
codegen-units = 16
strip = "debuginfo"
```

### 2.2 日常命令

```bash
cargo check                          # ★ 主力命令：只类型检查，不生成不链接
cargo clippy --all-targets           # 静态检查（比 check 稍慢，提交前跑）
cargo test -- --test-threads=2       # 限并行，控内存
cargo build --tests                  # 只在要跑测试时 build
```

**为什么 `cargo check` 是主力**：跳过代码生成和链接，在这台机器上比 `cargo build` **快 40-50%，内存低 60%**。
日常改一行代码跑 `cargo check`，攒够一批改动再 `cargo build --tests`。

### 2.3 时间预期（诚实估计，含冷编译）

| 操作 | 首次（含依赖编译） | 增量（改一行） |
|---|---|---|
| `cargo check` | 6 - 12 分钟 | **20 - 60 秒** |
| `cargo build` | 14 - 25 分钟 | 1 - 3 分钟（链接是瓶颈） |
| `cargo test`（全部） | 20 - 35 分钟 | 2 - 5 分钟 |

**依赖量是第一个杠杆**：Sprint 1 的依赖集要刻意保持最小（axum + tokio + serde + tracing + clap 约 250 crates），
把重量级的（sqlx、reqwest、aws-sdk/opendal、jsonwebtoken、samael）推到后续 Sprint。
冷编译 250 crates vs 450 crates，差距是 8 分钟起步。

---

## 3. 数据库与 sqlx 的取舍

### 3.1 编译期校验需要活库

`sqlx::query!()` 宏在**编译期**要连数据库校验 SQL。开发期保持 PG 常驻（120-180M，很便宜）就没问题。

### 3.2 离线缓存（提交进仓库）

```bash
# 改了迁移之后，重新生成离线缓存
cargo install sqlx-cli --no-default-features --features rustls,postgres   # 一次性，约 10-20 分钟
cargo sqlx prepare --workspace -- --all-targets
git add .sqlx
```

CI 里设 `SQLX_OFFLINE=true` 即可不依赖活库编译。

### 3.3 一个待定的架构影响（需要拍板）

| 方案 | 优点 | 代价 |
|---|---|---|
| **A. 保留 `query!` 宏**（推荐） | SQL 错误编译期就暴露，安全价值高 | 编译期需要 PG 常驻；改迁移后要跑 `cargo sqlx prepare`（约 10-20 分钟） |
| B. 改运行时检查 `query_as` | 编译期完全不连库，省掉整个 `.sqlx` 流程 | 失去编译期 SQL 校验，只能靠集成测试兜 |

**默认选 A**：PG 常驻成本很低，编译期校验的价值更高。
但如果实际开发中被 `cargo sqlx prepare` 的等待烦到，**降级到 B 是合理的**——
那时 DAO 层本来就要为 L2/L3 隔离做抽象，运行时查询也更容易跨库复用。

> 注：L1/L2/L3 三档隔离都是 PostgreSQL，不存在方言问题；
> 只有将来真要支持 MySQL 时，`query!` 宏才会成为障碍（宏绑定单一 SQL 方言）。

---

## 4. 前端：别让 dev server 吃掉内存

```bash
pnpm install          # 用 pnpm（12.19.0 已装）；bun 做脚本 runner 也行，但 Vite 跑在 node 上最稳
pnpm dev              # Vite dev server，Svelte 5，约 300-450M
pnpm build
pnpm test             # vitest
```

**三条规矩**：

1. **`svelte-check --watch` 不要常开**（500M-1G）。
   开发时靠编辑器提示，类型检查改成手动 `pnpm check` 或放 CI。
2. **Storybook 不常开**（1-1.5G）。只在真正写组件时临时开，写完关掉。
3. **前端开发用 MSW mock，不连 Rust 后端**。
   这条最省事 —— 前端开发期间 Rust 进程根本不用起，省掉 200M 和整个后端启动等待。

---

## 5. 测试策略：分层 + 分界

| 层 | 工具 | 内存 | 频率 | 跑在哪 |
|---|---|---|---|---|
| 单元测试 | `cargo test --lib` | <500M | 每次提交前 | 本地 |
| 契约测试 | `cargo test -p platform-contracts` | <500M | 每次提交前 | 本地 |
| 前端单测 | `pnpm test`（vitest） | <500M | 每次提交前 | 本地 |
| 集成测试 | 本机 `base_test` 库 | ~1G | 合并前 | 本地 |
| 越权/安全测试 | 独立 test profile | ~1G | 合并前 | 本地 + CI |
| E2E | Playwright | ~1.5G | **验收时串行** | 本地（不常驻） |
| 全量回归 | 全部 | ~2G | 发布前 | CI |

### 5.1 集成测试不用 testcontainers

testcontainers 会为每个测试环境起新容器 → 额外 200-400M 内存 + 启动等待。

**改用同一个 PG 实例的独立库**：

```rust
// 每个测试套件开始前重置
sqlx::query("DROP SCHEMA public CASCADE; CREATE SCHEMA public;").execute(&pool).await?;
sqlx::migrate!("../../migrations").run(&pool).await?;   // 跑迁移
```

库名 `base_test`，与开发库 `base_dev` 完全隔离。测试连接串固定 `DATABASE_URL=postgres://.../base_test`。

### 5.2 测试数据

- 统一用 `seed` 脚本造数据（`cargo run -p xtask -- seed`），SQL 插数据，不要在测试里手写
- 夹具账号统一用 `alice@example.com` / `bob@example.com` / `both@example.com` / `carol@example.com`
  （与 `test-cases/` 中已写定的夹具一致，见 QA 生成的用例集）
- 每个测试自带清理逻辑，或统一用独立库重置

### 5.3 E2E 的跑法

```bash
# 只跑关键主链路，不开全量
pnpm test:e2e -- --grep "登录|权限"
```

完整 E2E 回归**只在验收时跑一次**，且跑之前先关掉 Storybook / svelte-check / Vite。

---

## 6. 一天的工作节奏（低配机最优解）

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

**核心心法：写 Rust 和写前端是分开的时间段，不是同时开两个 dev server。**

---

## 7. CI 的现实问题

当前仓库是 **local 模式（没有 origin）**，所以 GitHub Actions 这类云端 CI **跑不了**。

| 方案 | 说明 | 建议 |
|---|---|---|
| A. 加 remote，推到 GitHub / Gitea | 云端 runner 跑完整回归，本机只跑快测 | **推荐**，先解决这个 |
| B. 本机自跑 CI 脚本 | 串行 + 限并发，吃本机资源 | 无 remote 时的退路 |
| C. self-hosted runner 跑在本机 | 同 B，且能复用 CI 配置 | 私有项目不想上云时用，**并发必须限为 1** |

无论哪种，**本机的定位是「快速反馈层」**：单元测试 + 契约测试 + 集成测试在本地跑（分钟级），
完整回归 + E2E + 性能 + 安全扫描交给 CI（小时级，本机不参与）。

---

## 8. 编译期与运行期分离：CI 上的收益

一个可以直接抄的做法：把「编译」与「运行」分开度量。

- **编译层**：`cargo check` 全 workspace 通过 —— 快，秒级到分钟级，本地每次提交前跑
- **契约层**：`cargo test -p platform-contracts` —— 分钟级，本地每次提交前跑
- **集成层**：连 PG 跑 `cargo test --test '*'` —— 十分钟级，本地合并前跑
- **E2E 层**：Playwright 主链路 —— 三十分钟级，只在验收时跑

好处是**失败能被快速定位**：CI 红灯时，先看是不是编译层——那通常是 30 秒内就能复现的问题；
集成层失败才需要起完整环境。

---

## 9. 磁盘控制

用户明确要求：**不要把硬盘跑满**。当前 /home 可用 126G，预算如下。

| 占用项 | 预估 | 控制手段 |
|---|---|---|
| podman 镜像（postgres + redis） | ~150M | 固定两个镜像，不加其他 |
| podman 卷（PG 数据） | < 500M | `podman system prune` 可回收 |
| `target/`（Rust 产物） | 3 - 8G | `debug = "line-tables-only"` + `split-debuginfo = "unpacked"` 显著缩小 |
| `node_modules/` | 500M - 1G | pnpm 硬链接共享，勿用 npm |
| Vite / Storybook 缓存 | 200M - 500M | 可随时删 |
| Playwright 浏览器 | ~500M | 只装 chromium，不装全套 |
| **合计** | **< 12G** | 预算上限 20G |

**规矩**：

1. 每个 Sprint 结束跑一次 `podman system prune -af` 与 `cargo clean`（若 `target/` > 8G）
2. 只装 chromium 一个 Playwright 浏览器：`pnpm exec playwright install chromium`
3. `target/` 超过 8G 时用 `cargo clean`（保留 `~/.cargo` 缓存，重编译慢一次但省磁盘）
4. 不用 `cargo install` 装工具链（每个都编译 10-25 分钟且占盘），优先用系统包或免编译替代
5. 开发日志/构建产物不进 Git（`.gitignore` 覆盖 `target/` `node_modules/` `.aw/`）

**磁盘水位检查**（每个 Sprint 结束前跑）：

```bash
df -h /home | tail -1
du -sh ~/.cargo/registry ~/.rustup /home/myg133/Projects/system_seed/code/target 2>/dev/null
podman system df
```

---

## 10. 落地清单（REQ-002 实现时必须包含）

- [ ] `.cargo/config.toml`：`jobs = 3` + `rust-lld`（或 mold）
- [ ] `Cargo.toml`：`profile.dev` 的 `debug = "line-tables-only"` + `split-debuginfo = "unpacked"`
- [ ] `profile.dev.package."*"`：`opt-level = 0` + `debug = 0`
- [ ] `profile.release`：`lto = "thin"` + `strip = "debuginfo"`
- [ ] `compose.yaml`：**镜像用显式 daocloud 前缀**；`mem_limit` 限制 PG 256M / Redis 96M
- [ ] `bun run dev` 一条命令起全栈（含 PG 检查与种子数据）
- [ ] `bun run check` = `cargo check` + `pnpm check`（**手动触发，不 watch**）
- [ ] `bun run test` = Rust 单测 + 契约测试 + vitest，`--test-threads=2`
- [ ] `bun run test:integration` = 连 `base_test` 库（**不用 testcontainers**）
- [ ] `bun run test:e2e` = Playwright，带 `--grep` 过滤
- [ ] `bun run seed` = 种子数据（与测试夹具账号一致）
- [ ] `.sqlx` 离线缓存入库，CI 设 `SQLX_OFFLINE=true`
- [ ] `.gitignore` 覆盖 `target/` `node_modules/` `.aw/` `*.log`
- [ ] 开发文档写进仓库 `docs/dev-environment.md`（本指南的正式版）
- [ ] 磁盘水位检查脚本（`bun run disk:check`）
