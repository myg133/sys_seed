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
| 系统 lld | **不需要装** —— rustup 自带 `ld.lld`，且 rustc 1.98 默认就用它（见 §2.1） |
| mold | 可选，`sudo pacman -S mold`（pacman extra 有 3.0.0-4）；但默认 LLD 链接器峰值才 2MB，换 mold 收益有限 |

---

## 2. Rust 编译：把内存和等待时间压下来

### 2.1 项目级配置（写进仓库，REQ-002 落地）

**`.cargo/config.toml`**（放仓库根，所有人共享）：

```toml
[build]
# 不要开满 8 核：并行度换不来速度，但峰值内存线性上涨
jobs = 3

# 链接器：**什么都不配**。
# rustc 1.98 的 x86_64-unknown-linux-gnu target spec 默认就用 rustup 自带的
# ld.lld（LLD 22.1.8），经 gcc 驱动调用，链接器进程峰值仅 2 MB。
# 详见下方「链接器的真相」。
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

> ### 链接器的真相（2026-11-18 经 QA 两轮独立实测确认）
>
> **结论：不需要任何链接器配置，默认就是最优解。**
>
> | 配置 | 结果 |
> |---|---|
> | **不配（默认）** | ✅ 使用 **LLD 22.1.8**（rustup 自带的 `ld.lld`，经 gcc 驱动调用） |
> | `linker = "rust-lld"` + `link-self-contained=+linker` | ❌ **cargo 直接报错退出**：`+` 是 unstable 写法，stable 只接受 `y/yes/on/n/no/off/-linker` |
> | `linker = "rust-lld"` + `link-self-contained=-linker` | ❌ **链接失败**：绕过 gcc 驱动后找不到系统库 `unable to find library -lc / -lm / -ldl` |
> | 强制 `-C link-arg=-fuse-ld=bfd`（GNU bfd） | ⚠️ 能过，但不必要 |
>
> **为什么默认就是 LLD**：rustc 的 `x86_64-unknown-linux-gnu` target spec 自带 lld，
> 通过 gcc 驱动调用它 —— 既拿到 LLD 的行为，又保留 gcc 的库搜索路径。
>
> **⚠️ 内存差异没有想象中大**（这一点被两轮测量反复修正，务必注意）：
>
> | 口径 | LLD | GNU bfd | 差异 |
> |---|---|---|---|
> | 真实仓库二进制（内核 `VmHWM` + `wait4 ru_maxrss` 互证，无采样缺口） | **132.0 MB** | **135.5 MB** | **2.6%** |
> | 极小探针 crate（同一口径） | 71.5 MB | 30.0 MB | **方向相反** |
>
> - `ld.lld --version` 的**空载地板就是 32.8 MB**（libLLVM 固定占用），bfd 空载仅 7.6 MB
> - 「LLD 省 8 倍内存」是**错的** —— 那是瞬时 `VmRSS` 采样与内核高水位口径混淆的结果
> - **选 LLD 的真正理由是「stable rustc 的默认行为，零配置」，不是省内存**
>
> **可复现的验证方法**：
> ```bash
> # 默认构建，打印链接器版本
> RUSTFLAGS="-C link-arg=-Wl,--version" cargo build -v 2>&1 | grep -i "linker:"
> # 强制 bfd 对比
> RUSTFLAGS="-C link-arg=-fuse-ld=bfd -C link-arg=-Wl,--version" cargo build -v 2>&1 | grep -i "linker:"
> ```
>
> **教训**：早先指南推荐显式配 `rust-lld`，两处都是错的（unstable flag + 绕过 gcc 驱动）。
> 以后遇到「工具链默认行为不确定」的问题，**先实测默认值**，别急着覆盖。
> 另注：早先「本机未安装 ld.lld」的结论也不准确 —— rustup 自带 `ld.lld`，
> 只是不在 `/usr/bin/` 而在 toolchain 目录里。

### 2.2 日常命令

```bash
cargo check                          # ★ 主力命令：只类型检查，不生成不链接
cargo clippy --all-targets           # 静态检查（比 check 稍慢，提交前跑）
cargo test -- --test-threads=2       # 限并行，控内存
cargo build --tests                  # 只在要跑测试时 build
```

**为什么 `cargo check` 是主力**：跳过代码生成和链接，在这台机器上比 `cargo build` **快 40-50%，内存低 60%**。
日常改一行代码跑 `cargo check`，攒够一批改动再 `cargo build --tests`。

### 2.3 时间预期（**实测校正**，2026-11-18）

REQ-002 骨架的实测数据（`cargo clean` 后冷编译，RSS 采样）：

| 操作 | 冷（清空 target/） | 增量（改一行） |
|---|---|---|
| `cargo check --workspace --all-targets` | **17 秒 / 峰值 1071 MB** | **0.2 - 0.6 秒** |
| `cargo test --workspace`（含链接） | **16 秒 / 峰值 994 MB** | **< 3 秒** |

**为什么这么快**：依赖集被刻意压到最小。

| | crate 数 | 说明 |
|---|---|---|
| Sprint 1 骨架（实测） | **88 个唯一 crate** | axum / tokio / serde / serde_json / tracing / tracing-subscriber / clap / thiserror / anyhow / uuid / time |
| 含 sqlx + reqwest 后 | 约 170 - 210 个 | 后续 Sprint 再引入 |
| 含 aws-sdk / opendal / saml 全套 | 300+ | 推迟到真正需要时 |

> 早先估计「250 crates / 冷编译 6-12 分钟」是按全套依赖算的，**实际最小集只要 88 个 crate**，
> 冷编译 17 秒。**最小依赖集带来的收益远超预期** —— 这是低配机上最有效的一条策略。

**注意**：上面的冷编译是**注册表缓存已热**的情况（`~/.cargo/registry` 已 877MB）。
在全新机器上首次拉取 + 编译这 88 个 crate，按 rsproxy 的速度估算需 **3-8 分钟**（含下载）。
CI 上同理。

**磁盘代价**：

| 项 | 实测 |
|---|---|
| `~/.cargo/registry`（依赖源码缓存） | 877 MB |
| `target/`（构建产物） | 349 MB |
| `node_modules/` | 137 MB |
| Playwright chromium 缓存（仓库外） | 620 MB |
| **合计** | **约 2 GB** |

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

### 5.3 ⚠️ cargo test 的双 `--` 会假绿（2026-11-18 由 REQ-008 的 Dev 抓出）

libtest 只认**一个** `--`（分隔 cargo 参数与测试二进制参数）。写成两个：

```bash
# ❌ 错：双 `--` → libtest 把后面的 flag 当「测试名过滤串」→ 0 个用例被执行
cargo test --workspace -- --include-ignored -- --test-threads=2
# → test result: ok. 0 passed; 0 failed    ← exit 0，假绿！

# ✅ 对：只有一个 `--`
cargo test --workspace -- --include-ignored --test-threads=2
# → 真实跑出全部用例（含 ignored 的真实库用例）
```

**为什么危险**：退出码是 0、输出是 "ok"，看起来完全正常 —— 但**一个用例都没跑**。
如果你拿它当验收门禁，就会把「没跑」当成「通过」。

**自查**：任何 `cargo test` 输出的用例数若为 **0**，先怀疑双 `--`，别当成「全部通过」。

> 同一个坑还有另一个形态：`cargo test -- --nocapture` 是**对的**（只有一个 `--`），
> 但 `cargo test --workspace -- --test-threads=2 -- --nocapture` 就错了。

### 5.4 E2E 的跑法

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

## 8.5 机器重启后的恢复（踩过的坑）

**2026-11-18 断电重启后实测**：git 仓库与 worktree 拓扑**完好无损**，
但**两个后台进程都没了**，而且不会有任何提示 —— 只会在你下一次发消息时才发现。

### 必须手动恢复的两样东西

```bash
cd /home/myg133/Projects/system_seed

# 1) bus watcher —— 不恢复的话，bus 消息无人投递，认领/消息链路静默失效
export XDG_RUNTIME_DIR=/run/user/$(id -u)
python /home/myg133/.agents/skills/agent-workspace/scripts/aw.py watch --ensure --interval 5 --wake-dir .aw/wake
# → WATCH_STARTED pid=...；用 aw.py watch --status 确认

# 2) 数据库容器 —— podman 不会随机器重启自动拉起容器
cd code && podman-compose -f compose.yaml up -d
podman ps                       # 两个都应 Up (healthy)
```

### 各中断点的恢复成本

| 东西 | 会不会丢 | 怎么恢复 | 代价 |
|---|---|---|---|
| git 仓库 / worktree / 分支 | **不会** | 无需恢复 | 0 |
| 已提交的工作 | **不会** | 无需恢复 | 0 |
| **未提交的工作** | **不会**（在文件里），但**执行它的 Agent 会死** | 重新派 Agent 接管，复核它的半成品 | 一次额外的上下文成本 |
| podman 容器 | 停了 | `podman-compose up -d` | 几秒 |
| bus watcher | 没了 | `aw.py watch --ensure` | 几秒 |
| `~/.cargo` 缓存 | 不会 | 无需恢复 | 0 |

### ⚠️ 容器可能起不来：seed 挂载指向已删除的目录

`compose.yaml` 把 `./deploy/seed` 挂进 postgres 容器的 `/docker-entrypoint-initdb.d`。
podman 在**创建容器时**把这个相对路径解析成**绝对路径**记进容器元数据。
如果当初是从一个**后来被回收的 worktree**（如 `feature-REQ-005/`）里起的 compose，
那个目录一删，容器的挂载源就没了 —— 容器**再也起不来**，报错：

```
crun: cannot stat `/home/.../feature-REQ-005/deploy/seed`: No such file or directory
```

**症状**：podman 报容器起不来，但 `podman ps -a` 看容器还在（Exited）。

**修复**（数据在 volume 里，不会丢）：

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
cd /home/myg133/Projects/system_seed/code     # ← 必须是稳定的 develop worktree
podman rm -f base-postgres
podman-compose -f compose.yaml up -d
```

**预防**：**永远从 `code/`（develop worktree）起 compose**，不要从 feature worktree 起。
`code/` 是常驻 worktree,挂载源指向它就永远有效。

> 已在 REQ-002 的 compose 里踩过两次。详见 roadmap R14。

---

## 9. 出口标准：每轮需求开工前必读

> 这一节不是「建议」，是**每轮必须逐条对照的判定标准**。
> 全部来自已合并需求的复盘，每条都对应一次真实事故。
> 建立于 REQ-021 复盘；由 `req-021-retro.md` §6 的「新增出口标准」沉淀而来。

### 9.1 需求来源必须预披露（D122）

**开工前在回复里明说：本轮需求是用户原话第几项 / 用户批准的补充 / 还是 BA 自己补的。**

事故：REQ-017（工作流）是 BA 补的需求，用户从未提过，却进了排期、写了规格，
直到用户主动质疑「我应该没有说要加这个吧」才发现。**12 个需求里有 4 个是这种**。

**引述纪律**：引用用户原话时，**不要引用自己写的 demand 摘要**。
曾把 demand 里 BA 写的「不重复造…通知、审计、文件、流程」当成用户原话转述 —— 那是引述污染。

### 9.2 静态扫描必须带阳性对照

**扫「0 命中」之前，先证明扫描器会命中。**

```rust
// 好：注入一处，断言能命中
#[test]
fn scanner_detects_injected_chinese() {
    let sample = "fn title() { \"中文\" }";
    assert!(scanner_hits(sample), "扫描器必须能命中中文，否则 0 命中不可信");
    assert_eq!(scanner_hits(clean_source()), 0);
}
```

事故：REQ-021 的「零中文 title」验收，我 / QA / 复核者**三个人各踩一次** ——
0 命中和「扫描器根本没生效」在输出上完全一样。

同源应用：**敏感值扫描（如 `client_secret`）也必须带阳性对照**（REQ-013 的 AC-S-106 已内置）。

### 9.3 划边界前先引用 AC 原文

**按「AC 的射程」划边界，不按「哪个目录看着像别人的」划。**

事故：REQ-021 我把 `modules/**` 划成「绝对不要碰」，但 AC-S-088 写的是「跨全部模块」，
**12 处 title 全在 `modules/sandbox/backend/`** —— 是 BA 划错边界，不是 Dev 漏活。

### 9.4 还原文件后必须 `touch`

**`cp -p` 保留旧 mtime → cargo 判定「没变」→ 复用陈旧二进制 → 变异验证结果无效。**

事故：REQ-021 复核者用「还原文件再跑测试」验证变异，测出来的是**变异前**的结果。

配套：**untracked 文件 `git checkout` 还原不了** → 新建文件做变异性验证前**必须备份**，
并给出 `sha256` 基线，否则无法证明「中途没有变异残留」。

### 9.5 不可逆运算的关键参数必须必填

**让错误用法在类型上不可表达。**

事故：REQ-021 的汇率换算若接口是 `convert(from, to)` 而没有 `asOf`，
调用方无法知道拿到的是哪个时点的汇率 → 往返必然不等 → 早晚有人拿它做校验。
裁定（D120）：`asOf` **必填**，缺了就编译不过 / 400。

### 9.6 引用平台行为要带自测基准值

**不要靠推断写「平台会怎样」。**

事故：REQ-021 我写「货币小数位由 locale 决定」，实测 `Intl.NumberFormat` 发现
**由货币的 exponent 决定**（`ja-JP` 显示 CNY 仍是 2 位小数）。
→ 所有引用平台行为的规格，都要附上自己实测出来的基准值。

### 9.7 AC 的前置必须在真界面上试一遍

事故：REQ-012 写「`input[type=number]` 填 `abc` 后提交」，但浏览器**根本不接受** `abc`
（`value` 变成空串）→ 该前置**物理不可达**。

### 9.8 逐 crate 跑测试（技术强制）

```bash
# 必须这样（--all-targets 不能省，否则 lib 里的测试一个都不跑、退出码仍 0）
for c in platform-core platform-config platform-iam platform-rbac platform-org \
         platform-db platform-schema platform-server platform-audit \
         platform-contracts sandbox xtask; do
  cargo test -p "$c" --all-targets -- --test-threads=2 --include-ignored
done
```

- `cargo test --workspace` **在本仓必失败** → 必须逐 crate
- `cargo test` 只能有**一个** `--`（双 `--` → 0 passed 却 exit 0）
- `--include-ignored` 是 test-binary flag，必须在 `--` **之后**
- ⚠️ **清单要自己数，别手抄**（REQ-014 的教训）：
  - workspace 共 **13** 个成员，其中 `platform-contracts-macros` 是 proc-macro（**无测试**）
    → **12 个有测试的 crate**。REQ-014 整个周期都写「11 crate」，**漏了 `platform-contracts`** ——
    而它有 `tests/c6_schema_projection.rs`（**C6 错误响应契约**），
    偏偏那个需求就在改错误响应。
    → 每次开工前用 `sed -n '/^\[workspace\]/,/^\[/p' Cargo.toml` 数一遍。
- ⚠️ **包名 ≠ 目录名**：`modules/sandbox` 的包名是 `sandbox`。
  写成 `-p platform-sandbox` → `cargo test` 直接 **rc=101、0 条 result** ——
  **看起来像「这个 crate 没测试」，实际是整个 crate 没跑**。

### 9.9 测试资产要当交付物维护

- 夹具必须**独立、幂等、可复现**（D101）
- E2E 的 `skip` **只允许「被测系统未就绪」**；夹具 / 授权不对必须 **fail**（D108）
- 播种脚本要**自证授权**，不只自证「用户数」——「脚本成功」≠「夹具可用」
- ⚠️ **skip ≠ 已验证**（**D193**，REQ-014 的教训）：
  REQ-014 的 Dev 报「E2E 30 passed / 5 skipped / 0 failed」，
  那 5 条 skip **正好包含**审计页的关键用例 → 两个阻塞级缺陷**在交付时完全没被发现**。
  → 报告必须把「真跑通过」与「skip」**分开列**；
  **处于 skip 状态的功能不得计入「已验证」**；交付前必须让系统就绪后**真跑一遍**。
- ⚠️ **E2E 必须在「干净起点」上跑**（REQ-014 的教训）：
  「全绿」可能是**脏库**给的 —— REQ-014 的 E2E 在脏库（累积 42 条）上全绿，
  在干净库（夹具 11 条 < 默认 limit 20）上**必然失败**（一条用例偷偷依赖「记录数 > 20」）。
  → 报告要写明**起点状态**（如 `tenant-a = 11`）；
  依赖数据量的断言**要么自造数据、要么显式把阈值调小**。
- ⚠️ **修复的证据是「没有它就会红」**，不是「跑出来是绿的」：
  修复后要附**负向对照**（把修复撤掉 → 同一前提下复现失败 → 还原）。

### 9.10 依赖预算

**默认零新增。** 唯一破例是 TLS（**D123**，用户 2026-11-19 批准，为支持 https 的 OIDC）。

要加依赖时的流程：**先实测证明没有它做不成** → 上报用户 → 获批后登记决策 →
在自验报告里给出**包数增量与编译耗时**。**不得自行加依赖。**

### 9.11 ★ 写规格时，凡是引用既有 API，必须**打开源码确认**（不只是「记得」）

**这条是 9 条教训里代价最高的一条，因为它在 REQ-021 和 REQ-013 连续犯了两次。**

REQ-021：我写「中文 `title` 有 8 处」，实际 **73 处**；验收方式「扫构造点字面量」必然漏报 →
**扫完 0 命中、全部通过**。

REQ-013：三处更严重——

| 我在规格里写的 | 源码里的真实情况 | 照字面实现的后果 |
|---|---|---|
| 「`ldap3::Ldap::simple_bind` 可直接校验密码」 | 返回 `Ok(LdapResult)`，**密码错误时外层仍是 `Ok`**（`rc=49`），必须再 `.success()` | **任意密码都能登录** |
| 「`GET /api/config/entries` 取全量配置」 | **必须带 `keys`**，不给返回空数组；`?scope=tenant` 参数根本不存在 | 测试「0 命中 + 什么也没读到」= **三重假绿** |
| 「照抄 `upsert_external_identity`」 | 它是 `ON CONFLICT … DO UPDATE SET user_id`（**改指**语义） | 绑定可被**静默改到另一个用户** = 账号接管 |

**共同点：我读的是自己上一版的设计，不是源码。**
「我记得这个 API 是这样」不是证据 —— 尤其是**返回值语义**（`Ok` 不等于成功）、
**参数是否必填**（`Option` 不等于可选）、**冲突时的行为**（`DO UPDATE` 不等于幂等）。

> **最要命的一点：REQ-013 我其实做过调研，查了 9 项前置 —— 但查的是「存在性」，不是「语义」。**
>
> | 我查的 | 结论 | 我没查的 |
> |---|---|---|
> | `simple_bind` 存在吗 | ✅ 在 `ldap.rs:238` | 它**返回什么**、`Ok` 意味着什么 |
> | config 有 `format` 校验吗 | ✅ 有 | `read_entries` **要不要参数** |
> | `external_identity` 与 upsert 存在吗 | ✅ 有 | 那条 SQL 的**冲突分支**写的是什么 |
>
> **「这个能力存在」和「这个能力的行为是我想的那样」是两件事。**
> 前者一个 `grep` 就够，后者必须**读实现**。

**必须核对的四类东西**：

1. **返回类型与语义** —— `Ok(Result{rc})` 这类「外层成功、内层失败」的包装，必须看内层判定
2. **参数是否必填** —— `Option<T>` 常常意味着「不给就返回空」，不是「有默认值」
3. **冲突/重复时的行为** —— `ON CONFLICT DO UPDATE` / `DO NOTHING` 的语义差别很大
4. **加字段的连带影响** —— `tenant_row!` 按列名 `try_get`，加 struct 字段不改 SQL 是**运行期**错

**做法**：写进 AC 之前，对每个「复用既有能力」的点，`grep` 出实现并在规格里**标注 file:line**。
标注了的，QA 能复验；没标注的，QA 只能猜（而 QA 猜对了，说明我该早点去查）。

### 9.12 挑刺报告必须能落到「不可判定」这一档

QA / 复核者的报告要按**可判定性**分级，不能只按「严重程度」：

| 档位 | 含义 | 处置 |
|---|---|---|
| **主体不存在** | AC 要求的东西在代码里没有 | 必须改规格或补实现 |
| **物理不可达** | 前置在真实环境做不到 | 必须改规格 |
| **不可判定** | 「行为正确」这类没法验真假 | 必须改成可机械判定的判据 |
| **口径不一致** | 同一规则两处写法不同 | 必须统一 |
| **表述** | 措辞问题 | 可批量修 |

REQ-013 的 51 条里，**阻塞 14 条全落在前三档** —— 这类问题**不会自己暴露**，
因为「按错的规格实现 + 按错的规格验收」是自洽的。

### 9.13 ★ 判据（断言）必须与实现**同批交付**，并附**敏感度证据**

**这条来自 REQ-014 的两条阻塞级缺陷。**

- **现象**：REQ-014 的 B-01（`nextCursor` 恒 null）与 B-02（过滤拼出非法 SQL），
  **代码缺陷本身只有 2 行**；真正的问题是
  **仓库里没有任何一条断言能抓到它们** ——
  `crates/platform-audit/tests/` 里 `filter[` **零命中**，`nextCursor` 只在前端 vitest 的
  **mock** 里出现过。
  → 「按错的规格实现 + 按错的规格验收」是**自洽闭环**：谁都不会报错。
- **规矩**：
  1. **实现与它的判据必须同一批交付**。只交实现不交判据 = 缺陷没修，只是没被发现。
  2. 判据要打在**真界面上**（真库 / 真 HTTP），不是 mock 后面 ——
     mock 只能证明「事件被构造出来了」，不能证明「它真的落库了」。
  3. **交付时附敏感度证据**：把实现改坏 → 对应断言变红 → 还原后逐字节相同。
     只有「跑出来是绿的」**不算**证据。
  4. **反向变异**（把某处改坏而断言**仍绿**）能挖出**覆盖缺口** ——
     REQ-014 的反向变异就挖出了「失败分支没断言 `user_agent`」。
  5. 跨栈断言写「**包含**」不写「**相等**」，并**逐 locale** 检查 ——
     两侧**数字相等**完全可能是**两个不同集合**（REQ-014：两侧都是 84，集合差 6↔6）。

### 9.14 ★ 测试节奏：**开发期增量测试 → 终版集中全量 + E2E**

> **2026-11-20 用户明确要求。** 低配机（8 线程 / 7.7G）上，
> 「每改一行就跑一遍全量」是**浪费**（一次全量 = 12 个 crate 串行，几十分钟），
> 而「只在最后跑全量」会**太晚**发现跨 crate 的破坏。分成两段才对。

#### 阶段一：开发期 —— **增量测试**

| 场景 | 跑什么 |
|---|---|
| 改了 `crates/platform-oauth/**` | `cargo test -p platform-oauth --all-targets -- --test-threads=1` |
| 改了 `crates/platform-iam/**` | `cargo test -p platform-iam --all-targets -- --test-threads=1` |
| 改了 `crates/platform-core/**` | `cargo test -p platform-core --all-targets -- --test-threads=1`（**错误码改动会在这里红**） |
| 改了 `apps/shell/**` | `pnpm --filter @base/shell test` |
| 改了 `packages/i18n/**` | `pnpm --filter @base/i18n test` |
| 改了迁移 / manifest | `cargo xtask capabilities --check` + `cargo xtask tenant-policies --check` |
| 改了 `crates/platform-server/src/main.rs`（装配） | `cargo test -p platform-server --all-targets -- --test-threads=1` |

- **只跑「被改动的 crate + 它的直接依赖方」**，不跑无关 crate。
- ⚠️ 增量绿**不等于**全量绿 → **绝不能**据此说「需求完成」。
- 增量测试**必须带** `--all-targets`、**只能有一个 `--`**、**串行**
  （§9.8 的三条硬约束在增量阶段**同样**适用）。
- ⚠️ 增量阶段**不跑 E2E** —— E2E 要起服务、播种夹具、跑浏览器，
  在低配机上是最贵的一档，且夹具往往还没准备好。

#### 阶段二：终版 —— **集中全量 + E2E，一次跑完**

在**代码冻结之后**、**合并之前**，集中跑一次：

| # | 跑什么 | 口径 |
|---|---|---|
| 1 | **全量 Rust 回归** | **12 个有测试的 crate 逐个跑**（§9.8 的清单，**要数 workspace 成员别手抄**）；`--all-targets` + `-- --test-threads=1` + `--include-ignored` |
| 2 | **前端全量** | `pnpm --filter @base/shell test` + `pnpm --filter @base/i18n test` + `pnpm --filter @base/sdk test`（sdk 需先 `pnpm -r build`） |
| 3 | **类型检查** | `svelte-check` 0 error |
| 4 | **能力索引** | `cargo xtask capabilities --check` **必须绿**（红 = 索引过期） |
| 5 | **E2E** | `npx playwright test e2e/` —— **必须从干净库跑**（§9.9 / TD-039 的 3 步补建），**0 skipped** |
| 6 | **契约检查** | `cargo test -p platform-contracts --all-targets` + `aw.py contract check` |

#### 三条硬规矩

1. **全量只跑一次** —— 在「修复轮结束、代码冻结」之后跑。
   修复轮**过程中**只跑增量；**不要**每修一处就跑一次全量。
2. **全量的数字要自己算** —— 用 `grep -E '^test result' | awk` 汇总，
   **不要**抄 Dev 报告里的数字（REQ-014 的 BA 终验就是这么做的）。
3. **全量绿 ≠ 需求通过** —— 全量只证明「**没弄坏别的**」；
   「需求本身做到了」由 AC 的**专项判据**（含 §9.13 的敏感度证据）负责。
   E2E 还要单独看 **0 skipped**（skip ≠ 已验证，D193）。

### 经验

- **子 Agent 的工作产物不会丢，但进程会死** —— 派 Agent 时让它**小步提交**，
  这样重启后最多损失最后一步
- 恢复后的第一件事是 `git status`：它会告诉你上一个 Agent 改到哪了
- 重启前跑一次 `aw.py doctor`，能提前看出 watcher / 认领 / 队列的状态

---

用户明确要求：**不要把硬盘跑满**。当前 /home 可用 126G，预算如下。

| 占用项 | 预估 | 控制手段 |
|---|---|---|
| podman 镜像（postgres + redis） | **实测 300M + 15M** | 固定两个镜像，不加其他 |
| podman 卷（PG 数据） | < 500M | `podman system prune` 可回收 |
| `~/.cargo/registry`（依赖源码） | **实测 877M** | 全局共享，不会随需求线性增长 |
| `target/`（Rust 产物） | **实测 349M** | `debug = "line-tables-only"` + `split-debuginfo = "unpacked"` 已压到很低 |
| `node_modules/` | **实测 137M** | pnpm 硬链接共享，勿用 npm |
| Playwright chromium（仓库外） | **实测 620M** | 只装 chromium，不装全套 |
| **REQ-002 全量实测合计** | **约 2 GB** | 预算上限 20G，余量充足 |

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
