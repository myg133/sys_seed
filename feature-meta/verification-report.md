# REQ-004 验证报告

- **需求**: REQ-004 AI 引导体系与能力索引
- **分支**: `feature/REQ-004`
- **基线**: `develop` @ `c8236b0`（REQ-002 工程基线 + REQ-003 扩展点框架已合并）
- **设计依据**: `BA/demands/REQ-004-ai-guidance/design-summary.md`
- **契约**: api-conventions v1、extension-points v1（均已 ratified，已在 `feature-meta/manifest.json` pin）
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: **第二轮（修复轮）** —— 首轮 QA 结论 PASS-WITH-RISK，6 项核心检查全部复现为真，
  问题集中在门禁与提示词命令；本轮修 3 个必修 + 3 个建议项，详见 §5.4

---

## 0. 摘要

| 项 | 结果 |
|----|------|
| 改动 | **42 文件 / +5400 -455 行**（全量交付，基线 `c8236b0` → 本轮）；其中**修复轮 17 文件 / +910 -49 行**（`f670299` → 本轮） |
| 新增第三方依赖 | **无**（`git diff c8236b0 -- Cargo.lock` 为空，**0 行**） |
| Rust 测试 | **99/99 通过**（首轮 98 个全绿，修复轮 +1；原有 **73 个全绿**，只增不减） |
| 前端测试 | **23/23**（pnpm -r） |
| E2E | **1/1**（Playwright chromium） |
| lint | `cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` ✅ 0 告警 / `pnpm exec eslint .` ✅ / `pnpm exec prettier --check .` ✅ |
| ★ 咬合点 | `create-module demo-check` → `check-modules` ✅ + 契约测试 C-1 ~ C-4 **4/4** ✅（验完已删除） |
| 能力索引 | 生成 ✅ / `--check` ✅ / 手改检测 ✅ / **多余产物自动清理 ✅**（修复轮新增） |
| 提示词命令 | **12 条逐条实跑，用例数全部 > 0**（修复轮新增，见 §5.4.5） |
| CI 触发器 | `pull_request_review: [submitted, edited, dismissed]` ✅（修复轮新增，见 §5.4.1） |
| 契约引用 | 正本 `git show demand:contracts/...` + 仓库内快照 `docs/contracts/`（脚本生成 + CI 校验，见 §5.4.3） |

**核心结论**：本需求的设计前提是「**提示词解决「怎么写」，能力索引解决「不要重写」**」。
本轮把两半都落成了机器可校验的产物：提示词里的每条命令都实跑过，
索引里的每条能力都对应真实存在的代码，且索引**逐字节可复现**（手改即被 CI 发现）。

---

## 1. 交付物清单（设计摘要 §6）

| # | 交付项 | 位置 | 状态 |
|---|--------|------|------|
| A | 仓库级 `AGENTS.md` | `AGENTS.md` | ✅ 180 行（修复轮后），agents.md 开放规范 |
| B | `prompts/` 8 套提示词 | `prompts/01..08-*.md` | ✅ 统一结构（目标 / 输入 / 步骤 / 约束 / 验收） |
| C | `capabilities/` 能力索引 + 生成器 | `capabilities/`（13 文件）、`crates/xtask/src/capabilities.rs`、`rustscan.rs` | ✅ 自动生成 + `--check`（修复轮：会清理多余产物） |
| D | `cargo xtask create-module` | `crates/xtask/src/scaffold.rs` | ✅ 咬合点实测通过 |
| E | 模块级 `AGENTS.md` 模板 | `scaffold.rs::AGENTS_MD` | ✅ 由 create-module 生成 |
| F | CI 护栏 | `.github/workflows/ci.yml`、`.github/CODEOWNERS` | ✅ 3 个新 job（修复轮：补 `pull_request_review` 触发器） |
| G | `feature-meta/manifest.json` 契约 pin | `feature-meta/manifest.json` | ✅ 已补 `contracts` / `adrs` / `delivered_by` |
| H（修复轮） | 契约快照 + 同步脚本 | `docs/contracts/`、`scripts/sync-contracts.sh` | ✅ 快照与 BA 正本逐字节一致，CI 强制校验 |

---

## 2. 自验清单（逐条实跑）

| # | 命令 | 结果 |
|---|------|------|
| 1 | `cargo check --workspace --all-targets` | ✅ 通过 |
| 2 | `cargo test --workspace -- --test-threads=2` | ✅ **99 passed / 0 failed**（原有 73 个全绿，只增不减） |
| 3 | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 0 告警 |
| 4 | `cargo fmt --all --check` | ✅ 通过 |
| 5 | `cargo xtask capabilities` | ✅ 生成 13 个文件并落盘 |
| 6 | `cargo xtask capabilities --check` | ✅ 通过（13 个文件与源码一致） |
| 7 | 手改 `capabilities/index.json` → `--check` | ✅ **失败**（exit 1，`E_CAPABILITIES_OUTDATED`，列出「内容不一致：capabilities/index.json」）→ 还原后通过 |
| 8 | `cargo xtask create-module demo-check` → `cargo xtask check-modules` | ✅ `模块目录校验通过：2 个模块`（✓ demo-check / ✓ sandbox） |
| 9 | `cargo test -p demo-check -- --test-threads=2` | ✅ **4/4**（C-1 / C-2 / C-3 / C-4 全 ok） |
| 10 | `cargo xtask check-modules`（对 `modules/sandbox/`） | ✅ 通过（1 个模块） |
| 11 | `pnpm -r test` | ✅ **23/23**（7 个包 + shell 各 1 个测试文件） |
| 12 | `bun run test:e2e` | ✅ **1/1**（`e2e/shell.spec.ts`） |
| 13 | `pnpm exec eslint .` / `pnpm exec prettier --check .` | ✅ 均通过 |
| 14 | `cargo xtask check-deps` | ✅ `依赖方向校验通过：平台 crate 未反向依赖业务模块` |
| 15 | `prompts/` 8 个文件里的 12 条命令（去重）逐条实跑 | ✅ 全部 exit 0，**执行的用例数全部 > 0**（见 §5.4.5） |
| 16 | 手改 `capabilities/sdk/stale-pkg.json` → `--check` → `cargo xtask capabilities` → `--check` | ✅ 修复轮：exit 1（多余文件）→ 自动清理 → exit 0（13 个文件一致） |
| 17 | `scripts/sync-contracts.sh`（校验） / `--sync`（同步） | ✅ 快照与 BA 正本逐字节一致；手改快照 → exit 1 → `--sync` 恢复 |
| 18 | `.github/workflows/ci.yml` YAML 解析 | ✅ 可解析；`on:` 含 `pull_request_review: types: [submitted, edited, dismissed]` |

**测试数变化明细**（Rust）：

| crate | 基线 | 首轮 | 修复轮 | 差 |
|-------|------|------|--------|-----|
| platform-core | 41 | 41 | 41 | 0（**未改一个字节**） |
| platform-contracts | 8 | 8 | 8 | 0（**未改一个字节**） |
| platform-contracts-macros | 0 | 0 | 0 | 0 |
| sandbox | 6 | 6 | 6 | 0 |
| xtask | 18 | 43 | **44** | **+26** |
| **合计** | **73** | **98** | **99** | **+26** |

---

## 3. ★ 咬合点验证（核心验收）

需求的核心验收点是「**业务方照着 create-module 就能得到一个合规模块**」。
实测记录：

```text
$ cargo xtask create-module demo-test
模块骨架已生成：/home/myg133/Projects/system_seed/feature-REQ-004/modules/demo-test
  ✓ .../Cargo.toml        ✓ .../manifest.json
  ✓ .../backend/src/mod.rs        ✓ .../backend/src/entity.rs
  ✓ .../backend/src/permission.rs  ✓ .../backend/src/repository.rs
  ✓ .../backend/src/service.rs
  ✓ .../frontend/routes.ts         ✓ .../frontend/pages/DemoTestPage.svelte
  ✓ .../migrations/README.md       ✓ .../tests/contracts.rs
  ✓ .../AGENTS.md                  ✓ .../README.md
  ! 已把 `modules/demo-test` 登记进根 Cargo.toml 的工作区成员

$ cargo xtask check-modules
模块目录校验通过：2 个模块
  ✓ demo-test
  ✓ sandbox                                     (exit 0)

$ cargo test -p demo-test -- --test-threads=2
running 4 tests
test demo_test_contract::contract_c1_manifest_matches_implementation ... ok
test demo_test_contract::contract_c2_permission_points_are_declared ... ok
test demo_test_contract::contract_c3_tenant_isolation_holds ... ok
test demo_test_contract::contract_c4_transaction_boundary_holds ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo xtask capabilities && jq '.modules[]' capabilities/permissions.json
demo-test: demo-test:item:read, demo-test:item:write
sandbox: sandbox:demo:read, sandbox:demo:write
```

**附加验证**：

- **长模块名**：用 `order-management`（标识符更长 → rustfmt 折行决策不同）复跑，两道关卡同样通过；
- **开箱即过 lint**：生成后**立即** `cargo fmt --all --check` 通过、clippy 0 告警
  （模板对生成的 Rust 源文件跑 `rustfmt` —— 模板不可能预知最终标识符长度，
  rustfmt 的折行决策与长度相关，所以格式化交给工具本身）；
- **`--perm-prefix`**：`create-module demo-test --perm-prefix shared` 生成
  `shared:item:read` / `shared:item:write`，并在 CLI 里**如实提示**：
  「权限点前缀与模块 id 不一致：契约检查 C-2 按模块 id 扫描源码，这些权限点不会被核对
  （只会产生告警）」。实测该场景下契约测试仍全绿（只有告警，不失败）。
- **清理**：demo-test 与 order-management 验证后**已删除**，根 `Cargo.toml` 与
  `capabilities/` 均已还原；还原后 `cargo xtask capabilities --check` 通过。

---

## 4. AGENTS.md 里的命令逐条实测

| 命令 | 实测输出 |
|------|----------|
| `cargo xtask create-module <name>` | ✅ 生成 13 个文件 + 登记工作区成员 |
| `cargo xtask create-module <name> --perm-prefix <prefix>` | ✅ 生成 + 打印前缀不一致告警 |
| `cargo xtask capabilities` | ✅ `能力索引已生成：13 个文件` |
| `cargo xtask capabilities --check` | ✅ `能力索引校验通过：13 个文件与源码一致` |
| `cargo xtask check-modules` | ✅ `模块目录校验通过：1 个模块` |
| `cargo xtask check-deps` | ✅ `依赖方向校验通过：平台 crate 未反向依赖业务模块` |
| `cargo check` | ✅ 通过 |
| `cargo fmt --all --check` | ✅ 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 0 告警 |
| `cargo test --workspace -- --test-threads=2` | ✅ 98/98 |
| `pnpm -r test` | ✅ 23/23 |
| `bun run test:e2e` | ✅ 1/1 |
| `bun run dev` | ✅ 后端 `http://127.0.0.1:8080/api/health` 返回 `{"status":"ok",...}`，前端 `http://127.0.0.1:5173/` 返回 HTTP 200（用完已停） |

> `bun run db:up` 在 AGENTS.md 里只作为**注释**出现（「数据库容器需已启动：bun run db:up」），
> 它是 `package.json` 里既有的真实脚本（`scripts/db-up.sh`），本机容器本就在运行，未重复执行。

---

## 5. 能力索引的防重造与防幻觉

### 5.1 `already_provided` 的每一条都对应真实代码

| 能力 | 对应的真实代码 |
|------|----------------|
| `request_context` | `crates/platform-core/src/context.rs::RequestContext` |
| `repository` | `crates/platform-core/src/extension/repository.rs::Repository` |
| `list_query` | 同文件的 `ListQuery` / `SortSpec` / `FilterSpec`（`DEFAULT_PAGE_SIZE=20`、`MAX_PAGE_SIZE=200`） |
| `module_registry` | `crates/platform-core/src/extension/registry.rs`（`register_platform_module!` / `discover()`） |
| `manifest` | `crates/platform-core/src/extension/manifest.rs` + `capabilities/schema/manifest.schema.json` |
| `contract_suite` | `crates/platform-contracts/` + `crates/platform-contracts-macros/` |
| `audit_declaration` | `extension/auditable.rs::Auditable` / `AuditEventSpec` |
| `notification_sender` | `extension/notification.rs::NotificationSender` / `NotificationMessage` / `DeliveryResult` |
| `workflow_handler` | `extension/workflow.rs::WorkflowHandler` / `TransitionEvent` |
| `transaction_boundary` | `extension/transaction.rs::TransactionalWrite` / `WriteBoundary` |
| `config` | `crates/platform-core/src/config.rs`（`ConfigScope` / `ConfigResolver` / `AppConfig`） |
| `error_code_i18n` | `packages/i18n/src/index.ts`（`translate` / `hasMessage`） |
| `theme` | `packages/theme/src/index.ts`（`defaultTheme` / `mergeTheme`） |
| `ui_token` | `packages/ui/src/index.ts`（`toneClass` / `TONES` / `defaultRadius`） |
| `schema_field_meta` | `packages/schema/src/index.ts`（`FieldMeta` / `describeField` / `findField`） |
| `sdk_client` | `packages/sdk/src/index.ts`（`createApiClient`） |
| `auth_state` | `packages/auth/src/index.ts`（`isAuthenticated` / `tenantId`） |
| `hooks_toggle` | `packages/hooks/src/index.ts`（`createToggle`） |

### 5.2 `not_yet_provided` 防止 AI 幻觉

设计摘要的示例把「分页、权限判定、表单渲染、审计埋点、租户过滤、通知发送、工作流发起、配置下发」
统统写进 `already_provided`。**但当前底座里这些并不都存在** ——
`@base/ui` 只有色调与圆角 token（**没有 Table / 分页组件**），
`@base/auth` 只有登录态形状（**没有 `can()`**），`@base/schema` 只有字段元数据
（**渲染器在 REQ-012**），HTTP 层与 DB 层都还没落地。

照抄那份示例等于让 AI 去调用不存在的 API —— **比没有索引更危险**。
因此本轮把清单拆成两段：`already_provided`（18 条，有就是有）+ `not_yet_provided`
（7 条，明确告诉 AI「这个还没有，别照着写」）。

### 5.3 「别写两套」是机器保证的

`AGENTS.md` 的「已有能力 / 尚未提供」两段与 `capabilities/index.json` 的
`already_provided` / `not_yet_provided` **内容完全一致**（18 + 7 条，key 与说明逐条相同），
由 `crates/xtask/src/catalog.rs::tests::agents_doc_matches_catalog` 守住 ——
改了一边不改另一边，测试立刻红。

> **口径澄清（修复轮更正）**：这叫「内容一致」，**不是「逐行一致」**。
> `AGENTS.md` 的表格按「后端接缝 → 前端包」的语义序排列，`index.json` 的
> `already_provided` 因为用 `BTreeMap` 而按**字典序**输出 —— 两者的**行的顺序不同**，
> 但每一行的 key 与说明逐条相同。机器守卫比对的也是「表格 vs 能力目录」（同序），
> 不覆盖 `index.json` 的排序。

---

## 5.4 修复轮：门禁与提示词命令（QA PASS-WITH-RISK 的 6 条）

QA 首轮结论 **PASS-WITH-RISK**：6 项核心检查全部独立复现为真，问题集中在
**门禁与提示词里的命令**。本轮逐条修复，每条都带可复现证据。

### 5.4.1 高：CI 缺 `pull_request_review` 触发器 → 「强制人工评审」永远无法变绿

**QA 实测**：workflow 的 `on:` 只监听 `pull_request`（默认类型 opened/synchronize/reopened），
**没有 `pull_request_review`**。而 `core-change-review` 的 `if:` 却判断
`github.event_name == 'pull_request' || github.event_name == 'pull_request_review'` ——
后半段是**永远走不到的死代码**。

后果：PR 改了平台核心 → `pull_request` 事件跑该 job → 此刻还没有批准 → FAIL；
评审者随后批准（`pull_request_review` 事件）→ **workflow 根本不触发** → 状态仍是 FAIL；
而唯一能触发 workflow 的推新 commit 又会让旧批准的 `commit_id` 与新 HEAD 对不上 → 再次 FAIL。

**修复**（`.github/workflows/ci.yml`）：

```yaml
on:
  push:
    branches: [main, develop, 'feature/**']
  pull_request:
    branches: [main, develop]
    types: [opened, synchronize, reopened, review_requested]
  pull_request_review:
    types: [submitted, edited, dismissed]
```

**复核判定逻辑在新增的 review 触发路径下依然成立**（`.local/verify-review-gate.sh`，
用与 job 里**逐字相同**的 jq 表达式跑三组样本）：

| 样本 | approved 计数 | 结论 | 期望 | 结果 |
|------|--------------|------|------|------|
| A 非作者批准当前 HEAD | 1 | 放行 | 放行 | ✅ |
| B 作者自批当前 HEAD | 0 | 拒绝 | 拒绝 | ✅ |
| C 非作者批准旧 commit | 0 | 拒绝 | 拒绝 | ✅ |

同时核对了 `pull_request_review` 的 payload 里 job 实际用到的四个字段
（`pull_request.base.sha` / `head.sha` / `number` / `user.login`）**全部存在**，
`if:` 条件在 `pull_request_review` 下为真 —— 那段死代码现在是活的。

**仍然是人工配置、代码解决不了的**（已在 `ci.yml` 的注释里如实写明）：

1. 把 `core-change-review` 设为 **required check**；
2. 打开 **Require review from Code Owners**（配合 `.github/CODEOWNERS`）；
3. GitHub 规定 `pull_request_review` 事件**只在工作流文件已存在于默认分支时触发** ——
   本文件合并进 `main` 之后，评审触发的门禁才会生效。

### 5.4.2 中：提示词里的 `cargo test` 命令多一个 `--` → 跑 0 个用例却 exit 0（假绿）

**QA 实测**：`prompts/02-add-permission.md` 与 `prompts/07-write-test-cases.md` 里的
`cargo test -p <模块名> -- --test-threads=2 -- --nocapture` / `-- --list` 多了一个 `--`，
libtest 把它当成「选项与过滤串的分隔符」，后面的 flag 变成过滤器的一部分 →
**所有用例被过滤掉，但命令仍返回 0**。

本机复现（修复前）：

```
$ cargo test -p sandbox -- --test-threads=2 -- --nocapture
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out
exit=0                       ← 假绿：一个用例都没跑，退出码却是 0

$ cargo test -p sandbox -- --test-threads=2 -- --list
（列出 0 个用例）
```

**修复**：

| 文件 | 位置 | 修复前 | 修复后 |
|------|------|--------|--------|
| `prompts/02-add-permission.md` | :35（自验） | `-- --test-threads=2 -- --nocapture` | `-- --test-threads=2 --nocapture` |
| `prompts/02-add-permission.md` | :53（验收） | 「加 `-- --nocapture` 确认」 | 「加 `--nocapture` 确认」 |
| `prompts/07-write-test-cases.md` | :18（步骤 2） | `-- --test-threads=2 -- --list` | `-- --test-threads=2 --list` |
| `crates/xtask/src/scaffold.rs` | :770（脚手架注释） | 「需要看到时加 `-- --nocapture`」 | 「需要看到时加 `--nocapture`（注意：`cargo test` 的参数分隔符 `--` 只写一次）」 |

**全量排查**：`prompts/` 8 个文件里引用的**每一条** shell 命令都过了一遍
（去重后 12 条，见 §5.4.5），确认没有第二处同类问题
（多一个 `--` / flag 拼错 / 参数顺序错 / 引用不存在的子命令）。

**留档**：这个坑写进了 `AGENTS.md` 的**硬规则第 8 条**（含正误对照），
因为它会让「验收项形同虚设」——AI 照着假绿命令办事，不会发现。

### 5.4.3 中：`AGENTS.md` 指向仓库内不存在的 `BA/contracts/...` 路径

**QA 实测**：`BA/` 整个目录被 gitignore，单独 clone 仓库后
`BA/contracts/api-conventions/v1.md` 不存在。AGENTS.md 正是 AI 进入仓库读到的**第一份**文件，
让它指向不存在的路径，AI 可能因此臆造契约内容。

**方案：选 (c) 两者都做**，理由如下：

| 方案 | 是否采纳 | 理由 |
|------|----------|------|
| (a) 只改成 `git show demand:...` | 部分采纳 | 契约是**同一个 git 仓库**的 `demand` 分支上的文件，`git clone` 默认带全部分支，所以「正本」永远可取 —— 但**单分支克隆 / 只拿到 `develop` 导出包**的人仍然拿不到，引用依然悬空 |
| (b) 只快照进仓库 | 部分采纳 | 快照让「只有代码仓库的人」能读到正文，但会多出一份可能与正本漂移的副本 —— 单靠「不手改」是纪律，不是机制 |
| **(c) 两者都做 + 机器校验漂移** | **✅ 采纳** | 正本唯一（BA 分支）、快照唯一（`docs/contracts/`，由脚本从正本生成）、漂移由脚本 + CI 机械拦住 —— 满足「不能造成双份正本互相漂移」 |

落地：

| 项 | 内容 |
|----|------|
| 正本 | BA 分支 `demand` 的 `contracts/<name>/<version>.md`（workspace 里即 `BA/contracts/…`） |
| 取正本 | `git show demand:contracts/api-conventions/v1.md`（本地没 `demand` 分支先 `git fetch origin demand`） |
| 快照 | `docs/contracts/<name>/<version>.md`（与正本**逐字节相同**，已 `diff` 核对） |
| 生成 | `scripts/sync-contracts.sh --sync` |
| 校验 | `scripts/sync-contracts.sh`（CI `guardrails` job 强制执行） |
| 清单来源 | `feature-meta/manifest.json` 的 `contracts` pin（当前 `api-conventions v1` + `extension-points v1`） |

> **正本路径的更正**：任务书里举例的是 `git show demand:BA/contracts/api-conventions/v1.md`，
> 但本仓库的 `BA/` 是 **`demand` 分支的 worktree 目录**、不是分支里的子目录 ——
> 分支里的真实路径是 `contracts/api-conventions/v1.md`（`git ls-tree demand` 已确认）。
> 所以正确写法是 `git show demand:contracts/api-conventions/v1.md`。
> 用 git 对象路径也顺带绕开了相对路径的坑：worktree 叫 `code/` 还是 `feature-REQ-004/`
> 都不影响它能不能取到。

引用点全部改完：`AGENTS.md`（参考段 + 目录树 + 命令段）、
`prompts/03-new-sso-protocol.md:19-20`。
另外 `docs/dev-environment.md:3` 的 `../../BA/plan/…` 相对链接同属这一类失效引用，
一并改成 `git show demand:plan/dev-guide-low-spec.md`（本轮顺手修，已在 §1 登记）。

**漂移校验实测**：手改快照一行 → `scripts/sync-contracts.sh` 退出码 1 并指出
「修复方式：运行 scripts/sync-contracts.sh --sync」；`--sync` 后逐字节一致、退出码 0。
本地没有 `demand` 分支时脚本**明确跳过并退出 0**，不会误报。

### 5.4.4 低：三条建议项

| # | 问题 | 修复 | 证据 |
|---|------|------|------|
| L1 | `--check` 报「多余文件」时给出的修复建议**无效**（`write_files` 只写不删） | `capabilities::write_files` 按 `check` 的**同一口径**剪除多余文件并清理空目录；`main.rs` 的提示语补上「多余文件会被自动删除」；新增单测 `write_files_prunes_stale_artifacts` | 注入 `capabilities/sdk/stale-pkg.json` → `--check` exit 1（多余文件）→ `cargo xtask capabilities` 打印「✓ 已清理多余文件：capabilities/sdk/stale-pkg.json」→ `--check` exit 0（13 个文件与源码一致） |
| L2 | 报告里 `already_provided` 写成 17 条，实际 **18** 条 | `feature-meta/verification-report.md` §5.2、`feature-meta/traceability.md` AC-038、`feature-meta/status.md` 偏离第 3 条 —— 三处全部更正为 18 条 | `jq '.already_provided \| length' capabilities/index.json` → 18；AGENTS.md 表格 18 行 |
| L3 | 称 AGENTS.md 与 index.json「逐行一致」，实际是**集合相等、键顺序不同** | `AGENTS.md`（已有能力 / 尚未提供两段）、`feature-meta/traceability.md` AC-035、`feature-meta/verification-report.md` §5.3 —— 措辞改为「内容完全一致（key 与说明逐条相同）；行的顺序不同：表格按语义序、索引按字典序」 | `jq -r '.already_provided \| keys_unsorted[]'` 与表格行序对比：集合相等、顺序不同 |

### 5.4.5 `prompts/` 里每一条命令的逐条实测

**方法**：把 8 个提示词文件里引用的全部 shell 命令去重（12 条），
逐条真跑，记录 **exit code** 与**实际执行的用例数**（`prompts/` 里的 `<模块名>`
一律用仓库里真实存在的 `sandbox` 代入）。
「执行用例数」以 libtest 输出的 `N passed` 为准 —— **0 就是假绿，必须判 FAIL**。

| # | 命令 | 出现在 | exit | 执行的用例数 | 判定 |
|---|------|--------|------|-------------|------|
| 1 | `cargo xtask check-modules` | 01/03/04/05/06/08 | 0 | —（2 个模块通过校验） | ✅ |
| 2 | `cargo test -p sandbox -- --test-threads=2` | 01/02/03/04/05/06/07/08 | 0 | **6**（2 + 4） | ✅ |
| 3 | `cargo fmt --all --check` | 01/07/08 | 0 | —（无差异） | ✅ |
| 4 | `cargo clippy --workspace --all-targets -- -D warnings` | 01/07/08 | 0 | —（0 告警） | ✅ |
| 5 | `cargo xtask capabilities --check` | 01/02/03/04/05/06/08 | 0 | —（13 个文件一致） | ✅ |
| 6 | `pnpm exec prettier --check .` | 01 | 0 | — | ✅ |
| 7 | `pnpm exec eslint .` | 01 | 0 | — | ✅ |
| 8 | `cargo test -p sandbox -- --test-threads=2 --nocapture`（**修复后**，原 02） | 02 | 0 | **6**（2 + 4） | ✅ 修复前是 0（假绿） |
| 9 | `cargo test -p sandbox -- --test-threads=2 --list`（**修复后**，原 07） | 07 | 0 | **列出 6 个** | ✅ 修复前是 0 |
| 10 | `cargo test --workspace -- --test-threads=2` | 08 | 0 | **98** | ✅ |
| 11 | `cargo xtask capabilities` | 01/02/03/04/05 | 0 | —（13 个文件，生成后 `git status` 干净） | ✅ |
| 12 | `cargo xtask create-module <name>` | 01/03/04 | 0 | —（见 §3 咬合点：4/4 契约测试） | ✅ |

> 逐条日志：`.local/prompt-commands.log`（不入库，`.local/` 已被 gitignore）。

---

## 6. 偏离与口径修正（如实登记）

| # | 偏离 | 原因 | 影响 |
|---|------|------|------|
| 1 | `index.json` **没有** `generated_at`，改为 `source_fingerprint`（FNV-1a 64） | 墙钟时间戳会让 `--check` 永远失败（每次生成都不同），要么放弃「逐字节可复现」，要么放弃「手改即失败」。AC-S-004 要求的是后者 | 索引仍能一眼看出对应哪份源码；**任何**手改（含改时间戳）都会被检出 |
| 2 | `sdk/` 下生成 **7 个**文件（auth / hooks / i18n / schema / sdk / theme / ui），设计摘要只列了 4 个 | 设计摘要 §3.3 的来源写的是「各 `packages/*/package.json`」，仓库里真实有 7 个包；少列会让 AI 找不到 `@base/i18n`（而错误文案恰恰靠它渲染） | 索引是设计清单的**超集**，不冲突 |
| 3 | `index.json` 增加 `not_yet_provided` 段 | 见 §5.2：把「还没有的能力」明写出来，防止幻觉 | 多一段信息，不影响已有字段 |
| 4 | `entry_points.xtask` 指向 `crates/xtask/src/`（设计摘要写 `crates/xtask/src/cmd/`） | 仓库里**没有** `cmd/` 目录，子命令在 `crates/xtask/src/` 下 | 指向真实路径 |
| 5 | `platform.modules` 只列**已存在**的 `sandbox`（设计摘要示例列了 `iam` / `rbac` / `tenant`） | 那三个模块还不存在，列出来会让 AI 以为可以依赖 | 如实反映现状 |
| 6 | 新增 `cargo xtask check-deps` 子命令 | REQ-003 已实现依赖图遍历但**没有暴露成子命令**，CI 用不了。实现**直接复用** `deps::dependency_direction_violations`，未重写 | 多一个 5 行的 CLI 包装 |
| 7 | 生成的模块由脚手架**自动登记进根 `Cargo.toml`** 的工作区成员 | 否则 `cargo test -p <name>` 跑不起来，AC-037「立即可编译运行」不成立 | 根 `Cargo.toml` 会多一行成员 |
| 8 | 脚手架对生成的 Rust 源文件跑 `rustfmt` | 模板无法预知最终标识符长度，rustfmt 的折行决策与长度相关。`rustfmt` 是 `rust-toolchain.toml` 里**固定**的组件，缺失只提示不失败 | 生成结果开箱即 `cargo fmt --check` 通过 |

---

## 7. 未验证 / 剩余风险（如实列出）

| 项 | 状态 | 原因 |
|----|------|------|
| **GitHub Actions 实跑** | **未验证** | 本仓库**没有 remote**（`git remote -v` 为空），Actions 跑不起来。CI 的 3 个新 job 的**判定逻辑**已本机验证（见下），但 YAML 在真实 Actions 上的行为未跑过 |
| **分支保护规则** | **未验证（需人工配置）** | 「强制人工评审」需要仓库设置里把 `core-change-review` 设为 required check，并打开「Require review from Code Owners」。这一步**在仓库设置里，不在代码里**，已写进 workflow 注释与 CODEOWNERS 头部 |
| **CODEOWNERS 的维护者账号** | **待确认** | 仓库无 remote，无法确认 GitHub 用户名；当前用 `@myg133` 占位，README 里已注明需按实际情况调整 |
| **AC-036 的字面测试** | **等价验证** | 「把提示词原样交给一个 AI 助手生成模块」这一步在 Dev 会话里**没有可复现的执行环境**。验证的是等价命题：提示词第 2 步的脚手架产出物实测直接通过两道关卡 |
| **AC-038 的字面测试** | **口径修正** | AC 期望「复用 `@base/ui` 表格与 `@base/schema` 渲染器」—— 这两个组件本期**不存在**（渲染器在 REQ-012）。本轮以 `not_yet_provided` 的诚实口径达成「防重造 + 防幻觉」的目的。**该 AC 的原文需要 BA 修订**（见 §8） |
| **覆盖率数字** | **未测量** | 本机无覆盖率工具，且按低配机约束不 `cargo install` 任何工具 |
| **冷编译峰值内存** | **未测量** | 本轮未清 `target/`、未做 RSS 采样（且约束禁止 `cargo clean`） |
| **`prompts/` 的提示词效果** | **未做 A/B 评测** | 设计摘要 §6 明确「不做 AI 行为的自动化评测（AC-038 用静态扫描 + 人工确认判定）」 |
| **`pull_request_review` 触发器在真实 Actions 上的行为** | **未验证** | 本仓库无 remote。已验证：① YAML 可解析、`on:` 含 `pull_request_review: types: [submitted, edited, dismissed]`；② 三组 jq 样本在新增触发路径下结论不变（§5.4.1）。**未验证**的是 GitHub 服务端是否真的按此触发 —— 需合并到默认分支后在真实 PR 上观察 |
| **`pull_request_review` 需要工作流文件在默认分支** | **需人工注意（已写入 ci.yml 注释）** | GitHub 规定该事件只在「工作流文件已存在于默认分支」时触发。本文件合并进 `main` 之前，评审提交**不会**让 workflow 重跑 —— 这一点代码解决不了 |
| **契约快照的 CI 校验依赖 `demand` 分支可达** | **降级但不误报** | `scripts/sync-contracts.sh` 在找不到 `demand` / `origin/demand` 时**明确跳过并退出 0**，不会因缺分支误报；`guardrails` job 因此配了 `fetch-depth: 0`。分支可达时，快照与正本不一致会**让 CI 失败**并给出同步命令 |
| **`core-change-review` 用 `pull_request.base.sha` 做 diff** | **已知边界（未改）** | `pull_request.base.sha` 是 PR 创建时的 base 提交，**不随 base 分支前进**；base 分支前进后 diff 可能把「已合入 base 的核心改动」也算进来 → 门禁可能**误判为需要评审**（偏严，不会漏放）。这是首轮就有的行为，不在本次修复范围，登记为剩余风险 |
| **契约快照会随 BA 契约变更而需要重新同步** | **预期行为** | BA 在 `demand` 上改契约 → CI 的快照校验会失败 → 运行 `scripts/sync-contracts.sh --sync` 即可。这是「不允许双份正本漂移」的代价与保障 |

### CI 判定逻辑的本机验证

```text
# 核心路径检测（用 REQ-003 的真实合并提交验证命中）
$ git diff --name-only c8236b0^ c8236b0 -- crates/platform-core crates/platform-contracts crates/platform-contracts-macros
→ 21 个文件（全部是平台核心）    # 命中

# 评审门禁的 jq 表达式（3 组样本）
非作者批准当前 commit  → 1  → 放行
批准人是 PR 作者本人    → 0  → 拒绝
批准的是旧 commit      → 0  → 拒绝
```

---

## 8. 需要 BA 同步的两件事

1. **AC-038 的原文**：父 AC 期望「复用 `@base/ui` 表格与 `@base/schema` 渲染器」，
   但这两个组件本期不存在（渲染器在 REQ-012，`@base/ui` 目前只有 tone / radius token）。
   建议把 AC-038 的判定口径改成「**已有能力不得重写 + 未提供能力不得幻觉**」——
   这正是本轮 `already_provided` + `not_yet_provided` 两段实现的效果。
2. **`BA/demands/REQ-004-ai-guidance/design-summary.md` §3.2 的 `index.json` 示例**：
   示例里的 `already_provided` 描述与底座现状不符（见 §5.2），
   且示例含 `generated_at`（本轮改为 `source_fingerprint`，理由见 §6 第 1 条）。
   建议按本轮实现同步设计摘要。

---

## 9. 新增依赖

- **第三方依赖：无。** `git diff c8236b0 -- Cargo.lock` 为空。
- **workspace crate：新增 0 个。** 新代码全部落在既有的 `crates/xtask` 里
  （`capabilities.rs` / `catalog.rs` / `rustscan.rs` / `scaffold.rs`）。
- **未引入** `syn` / `quote` / `proc-macro2` / `async-trait` / `sqlx` /
  `regex` / `sha2` / 任何基准库或 mock 库。
- trait 声明解析用**正则 + 约定扫描**（设计摘要 §8 的硬约束）；
  指纹用**手写的 FNV-1a**（8 行，零依赖）。

---

## 10. 提交

| # | 提交 | 内容 |
|---|------|------|
| 1 | `aef124a` | 能力索引生成器与仓库级 AGENTS.md |
| 2 | `77fe6b2` | 实现 create-module 脚手架与依赖方向子命令 |
| 3 | `411c74c` | 提示词库、CI 护栏、契约 pin、追溯矩阵、验证报告、状态 |
| 4 | `f670299` | 校准验证报告的改动统计 |
| 5 | `9dba569` | **修复轮**：CI 评审触发器 + 提示词命令 + 契约引用（快照与同步脚本）+ AGENTS.md 口径更正 |
| 6 | `109f93e` | **修复轮**：能力索引多余文件清理（含新单测） |
| 7 | （本提交） | **修复轮**：验证产物、追溯矩阵、状态与变更日志更新 |

提交信息统一带需求号：`[Dev] ... (关联: REQ-004)`。
