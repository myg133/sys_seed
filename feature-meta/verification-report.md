# REQ-004 验证报告

- **需求**: REQ-004 AI 引导体系与能力索引
- **分支**: `feature/REQ-004`
- **基线**: `develop` @ `c8236b0`（REQ-002 工程基线 + REQ-003 扩展点框架已合并）
- **设计依据**: `BA/demands/REQ-004-ai-guidance/design-summary.md`
- **契约**: api-conventions v1、extension-points v1（均已 ratified，已在 `feature-meta/manifest.json` pin）
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: 首轮（自验完成，待 QA 验证）

---

## 0. 摘要

| 项 | 结果 |
|----|------|
| 改动 | **37 文件 / +4564 -479 行**（3 个小步提交，基线 `c8236b0` → `411c74c`） |
| 新增第三方依赖 | **无**（`git diff c8236b0 -- Cargo.lock` 为空） |
| Rust 测试 | **98/98 通过**（原有 **73 个全绿**，只增不减：+25） |
| 前端测试 | **23/23**（pnpm -r） |
| E2E | **1/1**（Playwright chromium） |
| lint | `cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` ✅ 0 告警 / `pnpm exec eslint .` ✅ / `pnpm exec prettier --check .` ✅ |
| ★ 咬合点 | `create-module demo-test` → `check-modules` ✅ + 契约测试 C-1 ~ C-4 **4/4** ✅ |
| 能力索引 | 生成 ✅ / `--check` ✅ / 手改检测 ✅（失败并报明确错误码） |
| 新增 CI job | `guardrails` / `core-change-label` / `core-change-review`（+ `.github/CODEOWNERS`） |

**核心结论**：本需求的设计前提是「**提示词解决「怎么写」，能力索引解决「不要重写」**」。
本轮把两半都落成了机器可校验的产物：提示词里的每条命令都实跑过，
索引里的每条能力都对应真实存在的代码，且索引**逐字节可复现**（手改即被 CI 发现）。

---

## 1. 交付物清单（设计摘要 §6）

| # | 交付项 | 位置 | 状态 |
|---|--------|------|------|
| A | 仓库级 `AGENTS.md` | `AGENTS.md` | ✅ 157 行，agents.md 开放规范 |
| B | `prompts/` 8 套提示词 | `prompts/01..08-*.md` | ✅ 统一结构（目标 / 输入 / 步骤 / 约束 / 验收） |
| C | `capabilities/` 能力索引 + 生成器 | `capabilities/`（13 文件）、`crates/xtask/src/capabilities.rs`、`rustscan.rs` | ✅ 自动生成 + `--check` |
| D | `cargo xtask create-module` | `crates/xtask/src/scaffold.rs` | ✅ 咬合点实测通过 |
| E | 模块级 `AGENTS.md` 模板 | `scaffold.rs::AGENTS_MD` | ✅ 由 create-module 生成 |
| F | CI 护栏 | `.github/workflows/ci.yml`、`.github/CODEOWNERS` | ✅ 3 个新 job |
| G | `feature-meta/manifest.json` 契约 pin | `feature-meta/manifest.json` | ✅ 已补 `contracts` / `adrs` / `delivered_by` |

---

## 2. 自验清单（逐条实跑）

| # | 命令 | 结果 |
|---|------|------|
| 1 | `cargo check --workspace --all-targets` | ✅ 通过 |
| 2 | `cargo test --workspace -- --test-threads=2` | ✅ **98 passed / 0 failed**（原有 73 个全绿，+25） |
| 3 | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 0 告警 |
| 4 | `cargo fmt --all --check` | ✅ 通过 |
| 5 | `cargo xtask capabilities` | ✅ 生成 13 个文件并落盘 |
| 6 | `cargo xtask capabilities --check` | ✅ 通过（13 个文件与源码一致） |
| 7 | 手改 `capabilities/index.json` → `--check` | ✅ **失败**（exit 1，`E_CAPABILITIES_OUTDATED`，列出「内容不一致：capabilities/index.json」）→ 还原后通过 |
| 8 | `cargo xtask create-module demo-test` → `cargo xtask check-modules` | ✅ `模块目录校验通过：2 个模块`（✓ demo-test / ✓ sandbox） |
| 9 | `cargo test -p demo-test -- --test-threads=2` | ✅ **4/4**（C-1 / C-2 / C-3 / C-4 全 ok） |
| 10 | `cargo xtask check-modules`（对 `modules/sandbox/`） | ✅ 通过（1 个模块） |
| 11 | `pnpm -r test` | ✅ **23/23**（7 个包 + shell 各 1 个测试文件） |
| 12 | `bun run test:e2e` | ✅ **1/1**（`e2e/shell.spec.ts`） |
| 13 | `pnpm exec eslint .` / `pnpm exec prettier --check .` | ✅ 均通过 |
| 14 | `cargo xtask check-deps` | ✅ `依赖方向校验通过：平台 crate 未反向依赖业务模块` |

**测试数变化明细**（Rust）：

| crate | 基线 | 本轮 | 差 |
|-------|------|------|-----|
| platform-core | 41 | 41 | 0（**未改一个字节**） |
| platform-contracts | 8 | 8 | 0（**未改一个字节**） |
| platform-contracts-macros | 0 | 0 | 0 |
| sandbox | 6 | 6 | 0 |
| xtask | 18 | 43 | **+25** |
| **合计** | **73** | **98** | **+25** |

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
因此本轮把清单拆成两段：`already_provided`（17 条，有就是有）+ `not_yet_provided`
（7 条，明确告诉 AI「这个还没有，别照着写」）。

### 5.3 「别写两套」是机器保证的

`AGENTS.md` 的「已有能力 / 尚未提供」两段与 `capabilities/index.json` 的
`already_provided` / `not_yet_provided` **逐行一致**，
由 `crates/xtask/src/catalog.rs::tests::agents_doc_matches_catalog` 守住 ——
改了一边不改另一边，测试立刻红。

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

提交信息统一带需求号：`[Dev] ... (关联: REQ-004)`。
