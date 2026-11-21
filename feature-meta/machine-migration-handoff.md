# 换开发电脑交接（2026-11-21）

> **已推送到** `git@github.com:myg133/sys_seed.git`（**公开仓库**，12 个分支 / 323 提交 / 27 MB）
> 本文是**新机器从零接上**的完整步骤 + 换机前必须知道的事。

---

## 0. 一句话

**代码与规格全部在 GitHub 上了。新机器上 clone 一次 + 重建 worktree + 起数据库，就能接着干。**

⚠️ 但有三样**不在** Git 里、必须手工重建：**worktree 指针**（绝对路径）、**`.aw/` 本地状态**、**`.mcp.json`**。

---

## 1. 已推送的内容（核实过的）

| 项 | 值 |
|---|---|
| remote | `git@github.com:myg133/sys_seed.git` |
| 可见性 | **公开**（用户 2026-11-21 确认） |
| 分支 | **12 个**，逐个比对本地/远端 SHA → **12/12 一致** |
| 提交数 | 323 |
| 仓库体积 | 27 MB（`.git`） |
| 默认分支（远端 HEAD） | ⚠️ **`main`** —— 它只是占位提交（落后 `develop` **147** 个提交）|

### 12 个分支

```
develop      ← 主线（REQ-022 已合并 @ aa5b305）
demand       ← BA 仓库（规格 / 决策 / 用例 / 复盘）
deploy       ← 部署配置
workspace    ← workspace 根（只有 README + .gitignore）
main         ← ⚠️ 占位，落后 develop 147 个提交
feature/REQ-011  feature/REQ-012  feature/REQ-013  feature/REQ-014
feature/REQ-016  feature/REQ-021  feature/REQ-022
```

⚠️ **远端默认分支是 `main`（占位提交）**。新机器 clone 后**看到的是空壳** —— 这是最容易踩的坑。
→ 建议把 GitHub 默认分支改成 **`develop`**（Settings → Branches → Default branch）。

---

## 2. 换机前扫描结果（**公开仓库，所以扫得更严**）

| 扫描项 | 结果 | 阳性对照 |
|---|---|---|
| 密钥/凭据文件名（`*.pem` / `*.key` / `secret` / `credential`） | **0 命中** | ✅ 验证过会命中 |
| 被跟踪的 `.env` / `.env.*` | **0 命中**（`.env.example` 是全空值 + 注释） | ✅ 验证过会命中 |
| PEM 私钥头 `BEGIN ... PRIVATE KEY` | **0 命中** | ✅ 验证过会命中 |
| 已知密钥格式（AWS `AKIA…` / `sk-…` / `ghp_…` / `xox…`） | **0 命中** | ✅ 验证过 |
| 硬编码口令字面量 | **0 命中** | ✅ |
| 真实邮箱（非 `example.com`） | **0 命中** | ✅ 验证过会命中 |
| 内网 IP（`10.` / `172.16-31.` / `192.168.`） | **0 命中** | ✅ 验证过会命中 |
| 本地绝对路径（`/home/<name>` / `/Users/<name>`） | **0 命中** | ✅ 验证过会命中 |
| 客户数据（`*.csv` / `*.xlsx` / `backup`） | **0 命中** | — |
| 构建产物（`target/` / `node_modules/`） | **0 命中**（`.gitignore` 覆盖） | — |

**Git 历史也扫了**（323 个提交的 `--diff-filter=A` 全文件清单）：
- 唯一命中的「敏感」文件名是 `.env.example`（**空值**）与几个名字里带 `secret` 的**测试/迁移**文件
- 历史里出现的 PEM 字样只有 `crates/platform-iam/src/config.rs` 的**测试固件假密钥**
  （`-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMG\n...` 与 `changeme`）→ **不是真密钥**

**结论：可以公开。** 但注意：**业务设计规格（22 个需求的完整决策记录）都在里面** —— 这是你自己选择公开的。

---

## 3. 新机器从零接上（**按顺序**）

### 3.1 前置工具（版本要对齐）

| 工具 | 版本 | 来源 |
|---|---|---|
| Rust | **1.98.1**（含 clippy + rustfmt） | `code/rust-toolchain.toml` **自动锁**，rustup 会自己装 |
| Node | **v26.13.1** | 需手工装（或 nvm） |
| pnpm | **12.19.0** | `package.json` 的 `packageManager` 字段锁定 |
| bun | 1.4.4 | ⚠️ `code/bun.lock` 存在但主流程用 pnpm |
| podman | **6.2.0** + `podman-compose` | 起 PostgreSQL/Redis |
| PostgreSQL | **17-alpine**（容器内） | `compose.yaml` |
| Playwright 浏览器 | chromium | `npx playwright install chromium` |

### 3.2 clone + 重建 worktree

⚠️ **不要**直接拷旧机器的目录 —— worktree 的 `.git` 是**绝对路径指针**：

```
BA/.git              → gitdir: /home/myg133/Projects/system_seed/.git/worktrees/BA
code/.git            → gitdir: /home/myg133/Projects/system_seed/.git/worktrees/code
Deploy/.git          → gitdir: /home/myg133/Projects/system_seed/.git/worktrees/Deploy
feature-REQ-016/.git → gitdir: /home/myg133/Projects/system_seed/.git/worktrees/feature-REQ-016
```

**新机器的用户名/路径不同 → 这些指针全失效。必须重建。**

```bash
# 1) 在目标位置 clone（workspace 分支）
cd ~/Projects
git clone -b workspace git@github.com:myg133/sys_seed.git system_seed
cd system_seed

# 2) 把其它分支拉下来（clone 只拿一个分支的 HEAD）
git fetch origin

# 3) 重建 4 个 worktree（**注意目录名与分支的对应**）
git worktree add BA              demand
git worktree add Deploy          deploy
git worktree add code            develop
git worktree add feature-REQ-016 feature/REQ-016

# 4) 核实拓扑（应与本文 §1 的表一致）
git worktree list
```

⚠️ **分支名 ≠ 目录名**：
- `BA/` ↔ `demand`
- `Deploy/` ↔ `deploy`
- `code/` ↔ `develop`
- `feature-REQ-016/` ↔ `feature/REQ-016`

### 3.3 重建本地状态（**不在 Git 里**）

| 东西 | 怎么重建 |
|---|---|
| **`.aw/`**（agent-workspace 本地状态 + 14 个需求的认领归档） | ⚠️ **重建需要 `aw.py`**。见下 |
| **`.mcp.json`**（MCP 的 `aw` server 配置） | 里面写死了 `command: /home/myg133/.../aw-mcp.sh` → **路径必须改成新机器的** |
| **`target/`**（编译产物） | `cargo build` 重建（25G 里绝大部分） |
| **`node_modules/`** | `pnpm install` 重建 |
| **`apps/shell/dist`** | `pnpm --filter @base/shell build` |

**`.aw/` 与 `.mcp.json` 的确切内容**（旧机器上）：
```
.mcp.json:
{
  "mcpServers": {
    "aw": { "type": "stdio", "enabled": true,
            "command": "/home/myg133/Projects/system_seed/.aw/mcp/aw-mcp.sh", "args": [] }
  }
}

.aw/ 目录:
  watch.log / watch-state.json / locks/{claims,worktree,watch}.lock
  archive/REQ-{002,003,004,005,006,007,008,009,010,011,012,013,014,021,022}-*.json  ← 14 个归档
  mcp/aw-mcp.sh
```

⚠️ **`.aw/` 的归档是「历史记录的本地副本」** —— 需求状态的真值在 **`demand` 分支的 `BA/demands/*/status.md`** 里，
所以**丢了不致命**（可重建空的 `.aw/`）。但如果你想要历史归档，换机前**手工拷一份 `.aw/archive/`**。

### 3.4 起数据库

```bash
cd code
export XDG_RUNTIME_DIR=/run/user/$(id -u)     # rootless podman 必须
./scripts/db-up.sh                            # podman-compose up -d
# 等健康后：建 base_test 库 + 应用角色（见 deploy/seed/）
```

⚠️ **本机没有 `psql` 客户端**（旧机器就是）→ 用
`podman exec base-postgres psql -U base -d base_test -tAc "…"`

### 3.5 验证接上了

```bash
cd code
export CARGO_TARGET_DIR=$PWD/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
export XDG_RUNTIME_DIR=/run/user/$(id -u)

cargo test -p platform-core --all-targets -- --test-threads=1     # 应 153 passed
cargo xtask capabilities --check                                   # 应「14 个文件与源码一致」
pnpm --filter @base/shell test                                     # 应 279 passed
```

---

## 4. 换机前的环境快照（旧机器实测）

```
rustc / cargo : 1.98.1
node          : v26.13.1
pnpm          : 12.19.0
bun           : 1.4.4
podman        : 6.2.0
postgres      : 17-alpine（容器 base-postgres，跑在 127.0.0.1:5432）
redis         : 7-alpine（容器 base-redis）
数据库角色    : base（超管/表 owner/BYPASSRLS）/ base_app（应用）/ base_platform_app（NOLOGIN 组角色）
数据库        : base_test（测试）/ base_dev（开发，**不要用**）
```

⚠️ **数据库容器已停**（`Exited`）。里面的数据**没有进 Git**，但**也不需要** —— 全部靠迁移 + seed 脚本重建。

---

## 5. ⚠️ 换机最容易丢的三样

| # | 会丢什么 | 后果 | 预防 |
|---|---|---|---|
| **1** | **`.aw/` 与 `.mcp.json`**（被 `.gitignore` 忽略） | MCP 的 `aw` server 不可用；认领归档丢失 | 换机前手工拷 `.aw/archive/`；`.mcp.json` 按 §3.3 重建（**路径必须改**）|
| **2** | **worktree 的绝对路径指针** | 直接拷目录 → 所有 worktree 报 `not a git repository` | **必须按 §3.2 重建**，不要拷 |
| **3** | **本地未提交的改动** | 丢失 | ✅ **已核实：5 个 worktree 全部干净（未提交 = 0）** |

---

## 6. 当前工作断点（新机器接上后从这里继续）

**停在 REQ-016 的「QA 挑刺完成 → 待裁决轮」。代码零改动。**

详见 **`BA/feature-meta/req-016-handoff.md`**（完整断点文件），摘要：

| 项 | 状态 |
|---|---|
| REQ-016 规格四件套 | ✅ 11 条 AC / D262~D275 / design-summary 新建 |
| QA 挑刺 | ✅ **39 条**（阻塞 9 / 高 16 / 中 9 / 低 5） |
| 用例集 | ✅ **61 条**（TC-016-001~061，11/11 AC 覆盖） |
| **下一步** | ⬜ **裁决轮**（先定 **S-03：运行时路由怎么做**，那是我设计的死路） |
| Dev | ⬜ 未派 |

**Sprint 5 进度**：013 ✅ / 014 ✅ / 022 ✅ / **016 待裁决** / 015 草稿 / 018 草稿

---

## 7. 仓库健康度快照（换机前）

| 项 | 值 |
|---|---|
| 全量测试（14 crate，`--include-ignored`） | **1094 passed / 0 failed / 0 ignored** |
| 前端 | shell **279** / i18n **45** / sdk 36+2 skipped（既有） |
| E2E | **44 passed / 0 failed / 0 skipped** |
| `cargo xtask capabilities --check` | ✅ 14 个文件一致 |
| `cargo xtask tenant-policies --check` | ✅ 4 个模块的表都开了 RLS |
| 技术债 | **47 条**（TD-001~047，**TD-044 已还**） |
| 已知基线红 | `cargo fmt --all --check` 有 **11 个存量文件**（TD-017，**不要顺手修**） |

---

## 8. 建议你现在做（换机前最后 3 步）

```bash
# 1) 把远端默认分支改成 develop（GitHub 网页：Settings → Branches → Default branch）
#    ⚠️ 不改的话新机器 clone 下来看到的是 main 的空壳提交

# 2) 拷一份 .aw/archive（如果想保留 14 个需求的认领归档）
cp -r /home/myg133/Projects/system_seed/.aw/archive ~/aw-archive-backup

# 3) 确认没有遗漏的未提交改动（已核实为 0，但再跑一次）
cd /home/myg133/Projects/system_seed && git status --short
```
