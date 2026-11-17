# 验证报告 —— 修复复核（Recheck）

- 需求: REQ-002 平台骨架与工程基线
- 分支: feature/REQ-002 @ `cbb877c31d9a1704822732dffe0f46d61a7bf83c`
- 首轮报告: `feature-meta/qa-report.md`（@ `c532718`，结论 PASS-WITH-RISK，3 中 + 5 低）
- 复核范围: 仅 8 个改动点 + 一次回归（首轮已确认成立的 7 条 AC 未重跑）
- 验证者: QA（轻量复核轮）
- 结论: **PASS-WITH-RISK**（首轮 3 中 + 5 低全部修复并复验通过；新发现 2 个**低**级问题，均不阻塞合并）

> 纪律说明：本轮为只读验证。除唯一交付物本文件外，未修改任何实现文件。
> 临时夹具（`crates/platform-core/tests/` 两个失败用例、`.local/qa-linker-probe` 探针 crate、podman 探针容器、`base_probe` 临时库、`base_dev` 标记表）**全部已清理**；
> 结束时 `git status --short` 为空，`podman ps -a` 仅剩 `base-postgres` / `base-redis`，`pg_database` 仅剩 `base_dev / base_test / postgres / template0 / template1`。
> 唯一触碰实现文件的是 `crates/platform-core/src/bin/platform-core.rs` 的 **mtime**（用于复现真实链接负载），**内容未改**（`git status` 为空即证）。

---

## 逐项复核

| # | 问题 | Dev 声称 | 复核结果 | 证据 |
|---|------|---------|---------|------|
| **M1** | 链接器文档纠正 | 「stable rustc 1.98 默认已用 LLD 22.1.8，无需显式配置」；未引入真配置 | ✅ **修复成立**（措辞 + 无真配置 + 探针可复现）<br>⚠️ 但文档里的**内存对比表不可复现**（见问题 #1） | 措辞：`docs/dev-environment.md` §2.1「### 2.1 链接器：stable rustc 默认已用 LLD，无需显式配置」；`feature-meta/verification-report.md:141`「本轮**没有**新增任何 linker 键」。<br>无真配置：`.cargo/config.toml` 去注释后仅 `[build] jobs = 3` / `[target.x86_64-unknown-linux-gnu]`（**空段**）/ `[alias] xtask`；`grep -nE "linker\|link-arg\|link-self-contained\|rustflags\|linker-flavor" .cargo/config.toml` 仅命中 3 行**注释**（:9/:10/:13）；全仓只有这一个 `.cargo/config.toml`；`~/.cargo/config.toml` 无 linker 键；环境无 `RUSTFLAGS` / `CARGO_TARGET_*_LINKER` / `LINKER`。<br>探针可复现：`RUSTFLAGS="-C link-arg=-Wl,--version" cargo build` → `warning: linker stdout: LLD 22.1.8 (/checkout/src/llvm-project/llvm 52ed14fcd56afc30f9cccd8ca8ce237c2eef7e04) (compatible with GNU linkers)`；强制 `-C link-arg=-fuse-ld=bfd` → `warning: linker stdout: GNU ld (GNU Binutils) 2.47`。**“默认即 LLD” 的结论独立复现成立。** |
| **M2** | 测试库重置 | `reset_test_db()` 只作用于 `base_test`、用 `00-init.sql` 重建基线表、对 `base_dev` 有硬拒绝闸 | ✅ **修复成立**（4 条路径 + base_dev 零污染 + `\connect` 过滤 + seed 可用）<br>⚠️ 闸门是**黑名单**而非白名单（见问题 #2） | **路径 1（正常）**：`DATABASE_URL_TEST=…/base_test ./scripts/test-integration.sh` → `重置测试库：postgres://base:base@127.0.0.1:5432/base_test（库 base_test）`；base_test public 表由 `platform_seed_log,qa_marker_test` 变为 **`platform_seed_log`（标记表被清）**；`platform_seed_log` 3 列齐全；24/24 通过；`容器数量比对通过：2 → 2`；exit 0。<br>**路径 2（base_dev 硬拒绝）**：`DATABASE_URL_TEST=…/base_dev` → **exit 2**，stderr `错误：DATABASE_URL_TEST 指向 base_dev，拒绝重置开发库（postgres://base:base@127.0.0.1:5432/base_dev）`；`grep -c "test result:" = 0`（**cargo test 未执行**）；base_dev schema+data 指纹（nonce 剥离后 sha256）前后**逐字节相同**（schema `af5d7903…e17c9`、data `0b5d04c7…29ced`）；标记表 `1\|QA-RECHECK-LEFTOVER-MUST-SURVIVE` 原样保留。<br>**路径 3（非本机连接串）**：`…@10.0.0.99:5432/base_test` → `跳过测试库重置：主机未安装 psql，且 DATABASE_URL_TEST 不指向本机 127.0.0.1:5432` + 提示 `需要集成测试时先执行 'bun run db:up'`；base_dev 指纹不变；exit 0。<br>**路径 4（容器未运行）**：用 podman shim 让 `podman ps` 不列出 `base-postgres` → `跳过测试库重置：主机未安装 psql，且数据库容器未运行` + 同一条提示；比对仍执行（`容器数量比对通过：1 → 1`）；exit 0。**未静默失败。**<br>**`\connect` 过滤**：源文件仅 `deploy/seed/00-init.sql:19` 一处 `\connect base_test`；`grep -v '^\\connect'` 后 21 行 → 20 行且**不再含 `\connect`**。行为级验证：对临时库 `base_probe` 执行重置后，**表落在 `base_probe`**（`base_probe` 表 = `platform_seed_log`）而非被 `\connect` 重定向到 `base_test` → 过滤生效。<br>**seed 可用**：重置后 `bun run seed` → exit 0，`已灌入种子：base_dev` / `已灌入种子：base_test`，两库 `platform_seed_log` 均有 `baseline`。Dev「第一版冲掉 `platform_seed_log`」的问题**已修复**（重置后该表仍在、3 列齐全）。 |
| **M3** | 断言可达 | `status=0; cargo test … \|\| status=$?` → 比对照常执行 → `exit "$status"` | ✅ **修复成立**（A/B 两场景 + 额外 C 场景） | **场景 A（测试失败 且 期间新建容器）**：临时用例 `crates/platform-core/tests/qa_recheck_tmp_container.rs` 内 `podman run -d --name qa-recheck-probe …` 后 panic → 比对**执行并报错**：`集成测试期间容器数量发生变化（2 → 3），不允许新建容器`；**exit 101**（反映测试失败）。<br>**场景 B（测试失败但没新建容器）**：临时用例 `qa_recheck_tmp.rs` 直接 panic → 比对**执行并通过**：`容器数量比对通过：2 → 2（未新建容器）`；**exit 101**。<br>**场景 C（额外自加：测试通过但新建容器）** → `集成测试期间容器数量发生变化（2 → 3），不允许新建容器`；**exit 1**（`status=0` 时置 1，`:136-138`）。<br>无残留 `set -e` 提前退出路径：`scripts/test-integration.sh:125-126` 的 `status=0` + `\|\| status=$?` 使 `cargo test` 失败不触发 errexit，其后 `:128` 的 `after="$(container_count)"` 与 `:131-141` 的比对在 A/B/C 三场景中**均实测执行**；`:143` `exit "$status"`。`bash -n scripts/test-integration.sh` 通过。 |
| **L1** | 未使用依赖清理 | `platform-core` / `@base/auth` `@base/hooks` `@base/schema` `@base/theme` 已删，构建与测试仍全绿 | ✅ **成立** | `crates/platform-contracts/Cargo.toml` 现仅 `thiserror.workspace = true`；`grep -rn "platform_core" crates/` 仅命中 platform-core 自身（`src/bin/platform-core.rs:7,29`）。`apps/shell/package.json` 现仅 `@base/i18n` `@base/sdk` `@base/ui` —— 与 `apps/shell/src/App.svelte:2-4` 的三条实际 import **完全一致**；`apps/shell/vite.config.ts` 的 alias 仍直指 `../../packages/<name>/src/index.ts`（**不经 node_modules**），故删依赖**不断链**（`@base/auth` 等 alias 仍可解析，但当前无用）。<br>绿灯证据：`cargo build`/`clippy`/`test` 全过；`pnpm install --frozen-lockfile` → `Lockfile is up to date`；`pnpm -r build` → 7 包 tsc + `apps/shell build: ✓ 113 modules transformed`；`pnpm -r test` → 23/23；`bun install --frozen-lockfile --dry-run` → exit 0。 |
| **L2** | CI 补 E2E job | `e2e` job：playwright install chromium + `bun run test:e2e` | ✅ **成立** | `.github/workflows/ci.yml:143-164`：`e2e:` job（`runs-on: ubuntu-latest`）→ `pnpm/action-setup@v4` + `actions/setup-node@v4` + `oven-sh/setup-bun@v2 (bun-version 1.4.4)` → `pnpm install --frozen-lockfile` → `pnpm exec playwright install --with-deps chromium` → `bun run test:e2e`。本机 `bun run test:e2e` 实跑 1/1 通过。 |
| **L3** | CI 删无效 `token:` 入参 + 镜像前缀 | `token:` 已删；CI 用官方镜像 | ✅ **成立** | `.github/workflows/ci.yml:96-97` 现为 `- uses: rustsec/audit-check@v2` + 注释 `# rustsec/audit-check@v2 不接受 token 输入；需要鉴权走 RUSTEC_AUDIT_TOKEN 环境变量`，**无 `token:` 键**。<br>镜像：`ci.yml` 全文 `grep -nEi "registry\.\|mirrors\.\|docker\.io\|dockerproxy\|aliyun\|tsinghua\|ustc\|tuna\|npmmirror"` **零命中**；服务镜像为官方 `image: postgres:17-alpine`（`:116`）。<br>YAML 解析：`python3 -c "import yaml;yaml.safe_load(open('.github/workflows/ci.yml'))"` → `YAML OK, jobs: ['lint','test','build','dependency-audit','contract-check','e2e']`（6 job）。<br>（`compose.yaml` 仍用 `docker.m.daocloud.io/library/*` —— 那是**本机**低配环境文件，CI 只对它跑 `docker compose config --quiet` 不拉镜像，符合“CI 在境外用官方镜像”的要求。） |
| **L4** | 报告更新 AC-S-009 | 「全新卷首启未实跑」→ 已验证并附证据 | ✅ **成立** | `feature-meta/verification-report.md:22`：`AC-S-009 … **本轮实跑**：rootless、双库连通、mem_limit、端口绑定、内存合计、**全新卷启种子路径**` / 结论 `**PASS**（上一轮记为「部分验证」是误判，本轮已实跑补齐）`；`:143-173` 给出第一手证据（容器标签 `com.docker.compose.project.config_files=compose.yaml`、卷挂载、`StartedAt=2026-11-18 00:30:26` vs 种子 `applied_at=…16:30:28`）。首轮问题 #8 的处理记录见 `:226`。 |
| **L5** | 撤除「链接器偏离」+ 追溯同步 | 原问题条目撤除；`traceability.md` 已同步 | ✅ **成立** | `feature-meta/verification-report.md:215` 原问题条目已删除线 + `**撤除**：该偏离不存在…`；`:250` 偏离清单第 1 条 `~~.cargo/config.toml 未写 linker = "rust-lld"~~ **已撤除**`。`feature-meta/traceability.md` 已同步：AC-S-010 行改判 PASS 并列出 4 条分支路径实测；新增「本轮（QA 退回修复轮）问题处理对照」表（#1–#8 逐条列出处理与落地位置）；「低配基线」行改写为「stable rustc 1.98 默认 `linker-flavor = gnu-lld-cc`，默认即 LLD 22.1.8，**无需显式配置**，非偏离」。 |

---

## 回归（本轮只跑一遍）

| 套件 | 命令 | 结果 |
|------|------|------|
| Rust | `cargo test --workspace -- --test-threads=2` | **24/24 通过**（platform-core 3 + platform-contracts 14 + xtask 7），exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0，**无 error 无 warning** |
| 前端 | `pnpm -r test` | **23/23 通过**（i18n 3 / hooks 3 / auth 3 / schema 3 / ui 3 / theme 2 / sdk 2 / shell 4），exit 0 |
| E2E | `bun run test:e2e` | **1/1 通过**（3.6s），exit 0 |

补充（L1 需要“构建”而非仅测试）：`pnpm install --frozen-lockfile` exit 0；`pnpm -r build` exit 0（`apps/shell build: ✓ 113 modules transformed`）。

---

## 仍存在的问题

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| 1 | 低 | **文档里的链接器内存对比表不可复现，且方向与实测相反。** `docs/dev-environment.md` §2.1 与 `feature-meta/verification-report.md:136-137` 写「默认(LLD) 峰值 16.1 MB / 强制 bfd 峰值 134.0 MB（约 8 倍）」。本轮用**无采样缺口**的口径（内核 `VmHWM` 高水位 + `wait4` 的 `ru_maxrss`，两种独立仪器互相印证）在**真实仓库二进制**（`platform-core` bin，cargo 自己生成的 rustc 命令行原样重放）上复测：**LLD 135144 KB = 132.0 MB、bfd 138772 KB = 135.5 MB**，两者只差 2.6%，**不是 8 倍**。另在合成探针 crate（4 MB static + 300 函数）上复测：**LLD 71.5 MB vs bfd 30.0 MB，方向相反**。Dev 的 bfd 值（134.0）基本可复现（我测 135.5），但 **LLD 值 16.1 MB 在物理上不可能**：单独执行 `ld.lld --version` 的基线 RSS 已是 **32.8 MB**（`ld.bfd` 基线 7.6 MB）——因为 rustup 自带的 `ld.lld` 动态链接 `libLLVM`，光启动就占 32.8 MB，低于此值的“峰值”只能来自**瞬时 `VmRSS` 采样**（Dev 自述“采样 `/proc/<pid>/status` 的 `VmRSS` 取峰值”）而非峰值。三轮记录（LLD 2 / 16.1 / 32.6 MB；bfd 130 / 134.0 / 187.4 MB）互不一致，根因就是**口径**（瞬时采样 vs 内核高水位）+ **负载**（探针 crate vs 真实二进制）不同。 | 本轮实测（真实二进制，同一条 rustc 命令行）：<br>`comm=rust-lld pid=285442 VmHWM=135144 KB (132.0 MB)` / `cc-tree ru_maxrss=134060 KB (130.9 MB)`<br>`comm=ld.bfd pid=287026 VmHWM=138772 KB (135.5 MB)` / `cc-tree ru_maxrss=138768 KB (135.5 MB)`<br>基线：`ld.lld --version → ru_maxrss=33560 KB (32.8 MB)`；`ld.bfd --version → ru_maxrss=7796 KB (7.6 MB)`<br>探针 crate：LLD `VmHWM=73240 KB (71.5 MB)` vs bfd `VmHWM=30752 KB (30.0 MB)`<br>文档现文：`docs/dev-environment.md` §2.1 表格「默认（仓库现状） LLD 22.1.8 **16.1 MB** / 强制 bfd **134.0 MB**（约 8 倍）」 | 该表**不影响** M1 的结论（“默认即 LLD、不要加 `linker = "rust-lld"`”已被独立复现，且加配置确实会坏事）。但“省 8 倍内存”是**未经复现且与实测相反**的数据点，且它是首轮“链接器偏离”误记账的源头。建议：<br>① 删掉倍数结论，只保留“默认即 LLD 22.1.8（`linker-flavor = gnu-lld-cc`）”这一可复现事实；<br>② 内存一栏改为引用**端到端冷编译峰值**（首轮实测 1087–1090 MB，阈值 1.5G/2.5G）——那才是 AC-S-006 真正卡的量；<br>③ 若要保留链接器单进程数据，必须写明口径（`/proc/<pid>/status` 的 `VmHWM` 或 `getrusage(RUSAGE_CHILDREN).ru_maxrss`），并注明 LLD 有 ~33 MB 的 `libLLVM` 固定地板。 |
| 2 | 低 | **测试库重置的闸门是“黑名单 `*base_dev*`”而不是“白名单 `base_test`”** —— `scripts/test-integration.sh:60-65` 只拒绝库名**含** `base_dev` 的连接串；`DATABASE_URL_TEST` 指向**任何其他**库名时，`DROP SCHEMA IF EXISTS public CASCADE; CREATE SCHEMA public;` 会照常执行。Dev 的表述“只作用于 `base_test`”因此只对默认路径成立。实测：对临时库 `base_probe` 执行后，`base_probe` 的 public schema 被重建。真实风险是**误配**（例如把 `DATABASE_URL_TEST` 写成 `…/postgres` 这个维护库，或指向别的应用的库）→ 该库 public schema 被清空，且当前没有任何“只允许 base_test”的兜底。 | `scripts/test-integration.sh:60-65`（`case "$DATABASE_URL_TEST" in *base_dev*) … return 2`）；实测：`DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_probe ./scripts/test-integration.sh` → `重置测试库：…（库 base_probe）`，`base_probe` 表 = `platform_seed_log`（原本为 `(none)`） | 改成**白名单**：库名 ≠ `base_test` 时一律拒绝（或加显式 `ALLOW_RESET_ANY_DB=1` 之类的开关）；保留现有 `base_dev` 黑名单作为二次保险。当前实现依赖“人不会手滑把测试库指向开发库”这一假设，DAO 落地后值得收紧。 |

> **首轮 8 条问题的处置结论**：#1（链接器偏离，撤除记账）✅、#2（测试库未重置）✅、#3（`set -e` 致断言不可达）✅、#4（AC-S-008 口径）✅（BA 已收窄 AC 并下推 REQ-011/012，本轮不重判）、#5（未使用依赖 2 处）✅、#6（CI 未跑 E2E）✅、#7（CI 无效 `token:` 入参）✅、#8（AC-S-009 误记“未实跑”）✅ —— **全部修复并复验通过，无一遗留**。
> 本轮新发现 2 个低级别问题（#1 文档数据点、#2 闸门口径），均不阻塞合并，但 #1 建议在合并前或紧随其后修正，以免下一个人继续引用“省 8 倍内存”这个已被实测推翻的说法。

---

## 未验证

| 项 | 原因 | 影响 / 建议 |
|----|------|----------------|
| **CI 六个 job 的实际执行** | 仓库无 remote，GitHub Actions 跑不起来 | 本轮只做 YAML 解析（`yaml.safe_load` 通过，6 job）+ 人工审阅 + 本地等价命令（`cargo fmt/clippy/test`、`pnpm install/build/check/test`、`bun run test:e2e` 全绿）。**推 remote 后仍建议跑一次真实 CI**，特别是新加的 `e2e` job（`playwright install --with-deps chromium` 在境外 runner 上的下载与 `--with-deps` 的 apt 安装路径本机无法验证）。 |
| **冷编译峰值内存与耗时（AC-S-006 主体）** | 任务书明确本轮为轻量复核、不重做冷编译；`cargo clean` 会清掉 `target/` 并重编 77 个依赖 | 沿用首轮实测引用值（冷 check 21.2s/1087 MB、冷 test 29.4s/1090 MB，均远低于 1.5G/2.5G）。本轮未新增冷编译数据。 |
| **链接器内存对比的“真实大二进制”场景** | 本仓二进制是骨架级小体量；`libLLVM` 地板占主导，无法代表 LLD 在超大工程上的优势 | 只影响问题 #1 的**表述口径**，不影响任何 AC 判定。 |
| **代码覆盖率** | 角色规范的“整体 ≥ 80%”未测（需 `cargo-llvm-cov`，本机不装、占盘大） | 与首轮一致，未变；按骨架需求的实际测试密度替代（24 Rust + 23 前端用例）。 |
| **`bun run dev` 全栈联调** | 本期无业务接口，后端二进制需常驻 | 与首轮一致，未变。 |
| **Playwright 浏览器版本跨机可移植性** | 本机已有 chromium-1208，`bun run test:e2e` 直接跑通未触发下载 | 与首轮一致；换机首次跑需 `pnpm exec playwright install chromium`（`@playwright/test` 固定 1.58.0，**不要升级**）。 |

---

## 本轮验证动作清单（可复现）

```console
# M1 链接器
grep -nE "linker|link-arg|link-self-contained|rustflags|linker-flavor" .cargo/config.toml   # 仅注释
RUSTFLAGS="-C link-arg=-Wl,--version" cargo build            # → LLD 22.1.8
RUSTFLAGS="-C link-arg=-fuse-ld=bfd -C link-arg=-Wl,--version" cargo build   # → GNU ld 2.47
# 峰值口径：/proc/<pid>/status 的 VmHWM（内核高水位，无采样缺口）+ wait4 的 ru_maxrss

# M2 测试库重置
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test  ./scripts/test-integration.sh   # 正常
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_dev   ./scripts/test-integration.sh   # → exit 2
DATABASE_URL_TEST=postgres://base:base@10.0.0.99:5432/base_test   ./scripts/test-integration.sh   # → 跳过
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_probe ./scripts/test-integration.sh   # → 证明 \connect 过滤
podman exec base-postgres psql -U base -d base_dev -c "CREATE TABLE qa_recheck_marker(...)"       # 标记表
podman exec base-postgres pg_dump -U base -d base_dev --schema-only | grep -v '^\\restrict\|^\\unrestrict' | sha256sum
bun run seed

# M3 断言可达（临时用例，跑完已删）
crates/platform-core/tests/qa_recheck_tmp.rs             # 失败、不建容器
crates/platform-core/tests/qa_recheck_tmp_container.rs   # 失败、建容器

# 回归
cargo test --workspace -- --test-threads=2
cargo clippy --workspace --all-targets -- -D warnings
pnpm -r test
bun run test:e2e
```
