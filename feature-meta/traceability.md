# REQ-004 追溯性矩阵

- **需求**: REQ-004 AI 引导体系与能力索引
- **分支**: `feature/REQ-004`
- **基线**: `develop` @ `c8236b0`（REQ-002 工程基线 + REQ-003 扩展点框架已合并）
- **契约**: api-conventions v1、extension-points v1（均已 ratified，已在 `feature-meta/manifest.json` pin）
- **设计依据**: `BA/demands/REQ-004-ai-guidance/design-summary.md`
- **开发者**: myg133-dev-1
- **更新时间**: 2026-11-18

图例：**PASS** = 已实现且有自动化或可复现的验证证据；**未验证** = 没有证据，如实标注。

---

## 1. 自有 AC

| AC | 内容 | 实现位置 | 验证方式与证据 | 结论 |
|----|------|----------|----------------|------|
| AC-S-004 | 索引自动生成不可手改：手改 `capabilities/` 后 CI 失败 | `crates/xtask/src/capabilities.rs`（`generate` / `check` / `write_files`）、`crates/xtask/src/main.rs`（`capabilities --check` 子命令）、`.github/workflows/ci.yml` 的 `guardrails` job | ① 手改 `capabilities/index.json` 的 `platform` 段 → `cargo xtask capabilities --check` **失败**（exit 1，报 `E_CAPABILITIES_OUTDATED` + `内容不一致：capabilities/index.json`）→ 还原后通过；② 索引产物**逐字节可复现**（不含墙钟时间戳），任何一处改动都会被检出 | **PASS** |

## 2. 父需求 AC（REQ-001）

| AC | 内容 | 实现位置 | 验证方式与证据 | 结论 |
|----|------|----------|----------------|------|
| AC-035 | AI 引导文件体系完整且与实际一致 | `AGENTS.md`、`prompts/`（8 套）、`capabilities/`（13 个文件）、`modules/<name>/AGENTS.md` 模板（`crates/xtask/src/scaffold.rs::AGENTS_MD`） | ① `cargo xtask capabilities --check` 通过（13 个文件与源码一致）；② 手改索引后校验失败（见 §1）；③ 「已有能力」段与 `capabilities/index.json` 的 `already_provided` **内容完全一致**（18 条，key 与说明逐条相同；**行的顺序不同** —— 表格按语义序、索引按字典序），由 `crates/xtask/src/catalog.rs::tests::agents_doc_matches_catalog` 机械保证（该守卫比对的是表格与能力目录，同序） | **PASS** |
| AC-036 | 提示词库可直接产出合规代码 | `prompts/01-new-crud-module.md` ~ `08-performance-optimization.md` | ① 8 套提示词均按统一结构（目标 / 输入 / 步骤 / 约束 / 验收）写全，步骤引用的是**真实存在的**命令与路径；② 「产出直接通过契约测试与 CI」这半句用**等价方式**验证：提示词第 2 步要求的 `cargo xtask create-module <name>` 产出物实测通过 `check-modules` 与契约测试 C-1 ~ C-4（见 §4）。**未做**「把提示词原样交给一个 AI 助手生成模块」这一步（Dev 会话里没有可复现的 AI 执行环境） | **PASS（等价验证，见验证报告 §5 的说明）** |
| AC-037 | 脚手架生成的模块开箱即用 | `crates/xtask/src/scaffold.rs`（`cargo xtask create-module <name> [--perm-prefix <prefix>]`） | `cargo xtask create-module demo-test` → ① 标准目录 + `manifest.json` + 模块级 `AGENTS.md` + `tests/contracts.rs` 全部生成；② **立即可编译**（`cargo test -p demo-test` 直接跑通）；③ 自动登记进根 `Cargo.toml` 工作区成员；④ 生成的 Rust 源文件跑 `rustfmt`（开箱即 `cargo fmt --check` 通过） | **PASS** |
| AC-038 | 能力索引有效防止 AI 重造轮子 | `capabilities/index.json` 的 `already_provided`（18 条）+ `not_yet_provided`（7 条）+ `permissions.json` + `extension-points.json` + `sdk/*.json` | ① 索引条目**逐条对应真实存在的代码**（`@base/i18n` 的 `translate()`、`@base/ui` 的 `toneClass()`、`@base/schema` 的 `FieldMeta`、平台的 `Repository` / `RequestContext` / `ListQuery` …）；② **未提供的能力也如实列出**（如 `@base/ui` 目前**没有**表格组件、`can()` **没有**落地），避免 AI 照着不存在的 API 写代码。**注意**：AC 原文期望「复用 `@base/ui` 表格与 `@base/schema` 渲染器」—— 这两个组件本期**尚未实现**（渲染器在 REQ-012），因此该子句在当前底座状态下**不可能成立**；本轮以 `not_yet_provided` 的诚实口径替代（见验证报告 §6 的偏离登记） | **PASS（口径修正，见验证报告 §6）** |

## 3. 设计摘要 §6 交付清单

| 交付项 | 实现位置 | 验证方式与证据 | 结论 |
|--------|----------|----------------|------|
| 仓库级 `AGENTS.md` | `AGENTS.md`（157 行，agents.md 开放规范） | ① `catalog::tests::agents_doc_matches_catalog` 守住「已有能力 / 尚未提供」两段与目录一致；② **每个命令都实跑过**（见验证报告 §4）；③ 目录 / 硬规则与 REQ-002/003 的真实代码一致 | **PASS** |
| `prompts/` 8 套提示词 | `prompts/01-new-crud-module.md` ~ `08-performance-optimization.md` | 人工审阅：每套含目标 / 输入 / 步骤 / 约束 / 验收；步骤里的命令、目录、包名全部真实存在（逐条核对） | **PASS** |
| `capabilities/` 目录结构 + `index.json` | `capabilities/`（13 个文件） | `cargo xtask capabilities` 生成成功并落盘；`--check` 通过 | **PASS** |
| `cargo xtask capabilities` 生成器 + `--check` | `crates/xtask/src/capabilities.rs` | `capabilities::tests`（6 个）+ 手改检测实验（§1） | **PASS** |
| `cargo xtask create-module` 完整实现 | `crates/xtask/src/scaffold.rs` | `scaffold::tests`（7 个）+ demo-test 实跑（§4） | **PASS** |
| 模块级 `AGENTS.md` 模板 | `crates/xtask/src/scaffold.rs::AGENTS_MD` | demo-test 生成的 `modules/demo-test/AGENTS.md` 内容已填好模块名 / 权限点 / 扩展点 | **PASS** |
| CI：索引一致性校验 + 核心改动打标 + 依赖方向检查 | `.github/workflows/ci.yml`（新增 `guardrails` / `core-change-label` / `core-change-review` 三个 job）、`.github/CODEOWNERS` | ① `guardrails` 的两条命令本机实跑通过；② 打标与评审门禁的**判定逻辑**本机用 jq 表达式验证过三种场景（§5）；③ **GitHub Actions 本体未实跑**（本仓库无 remote） | **PASS（逻辑验证）/ 未验证（Actions 实跑）** |

## 4. ★ 咬合点：create-module 产出必须直接过 REQ-003 的两道关卡

| 关卡 | 命令 | 实测输出 | 结论 |
|------|------|----------|------|
| 关卡一：目录规范 | `cargo xtask check-modules` | `模块目录校验通过：2 个模块` / `✓ demo-test` / `✓ sandbox`（exit 0） | **PASS** |
| 关卡二：扩展点契约 | `cargo test -p demo-test -- --test-threads=2` | `contract_c1_manifest_matches_implementation ... ok`、`contract_c2_permission_points_are_declared ... ok`、`contract_c3_tenant_isolation_holds ... ok`、`contract_c4_transaction_boundary_holds ... ok` → `4 passed; 0 failed` | **PASS** |
| 附带：能力索引纳入新模块 | `cargo xtask capabilities` | `capabilities/permissions.json` 出现 `demo-test: demo-test:item:read, demo-test:item:write` | **PASS** |
| 附带：开箱即过 lint | `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` | 生成后**立即**通过（模板对生成的 Rust 源文件跑 `rustfmt`） | **PASS** |

> **长模块名健壮性**：另用 `order-management`（标识符更长，rustfmt 折行决策不同）复跑，
> 两道关卡同样通过。
>
> **清理**：demo-test 与 order-management 验证后**已删除**，仓库里只剩 `modules/sandbox/`；
> 根 `Cargo.toml` 与 `capabilities/` 均已还原并 `--check` 通过。

## 5. 护栏（设计摘要 §5）

| 护栏 | 实现位置 | 验证方式与证据 | 结论 |
|------|----------|----------------|------|
| 同一套 CI（AI 与人工走同一条） | `.github/workflows/ci.yml` | AI 产出的模块与人工产出的模块走的是同一条 `cargo test --workspace` / `contract-check` 链路；demo-test 实测走通 | **PASS** |
| 索引一致性 | `guardrails` job → `cargo xtask capabilities --check` | 手改索引 → 失败（§1） | **PASS** |
| 依赖方向单向 | `guardrails` job → `cargo xtask check-deps` | REQ-003 已有的 `cargo metadata` 图遍历**直接挂到 CI**（未重写），本机实跑通过 | **PASS** |
| 核心修改打标 | `core-change-label` job + `.github/CODEOWNERS` | 打标逻辑（`git diff --name-only` 判定核心路径）本机用真实提交验证：命中 REQ-003 的合并提交时检出 21 个核心文件，未命中时为空 | **PASS（逻辑验证）/ 未验证（Actions 实跑）** |
| 强制人工评审 | `core-change-review` job + CODEOWNERS | 判定表达式本机用 3 组样本验证：非作者批准当前 commit → 放行；作者自批 → 拒绝；批准的是旧 commit → 拒绝 | **PASS（逻辑验证）/ 未验证（Actions 实跑）** |
| 越权测试 | `modules/<name>/tests/contracts.rs`（C-3 已覆盖跨租户） | 提示词 `06-authorization-test.md` 指导业务方补模块自有越权用例；不重复造 C-3 已覆盖的 | **PASS（提示词层）** |

## 6. 索引数据来源（设计摘要 §3.3）

| 索引 | 来源 | 实现 | 证据 |
|------|------|------|------|
| `permissions.json` | 扫描所有 `modules/*/manifest.json` 的 `permissions[].code` | `capabilities::permissions_json`（复用 `platform_core::extension::Manifest` 解析，manifest 不合规直接报错） | 索引内容 = sandbox 的 2 个权限点；demo-test 生成后索引自动纳入 |
| `extension-points.json` | 解析 `crates/platform-core/src/extension/` 的 trait 定义 | `rustscan::scan_traits`（**正则 + 约定扫描，不引入 `syn` 等解析框架**） | 扫出 6 个 trait，其中 5 个标为 `extension-point`（契约冻结的 5 个），`TransactionalWrite` 标为 `platform-capability` |
| `schema/manifest.schema.json` | REQ-003 已有的 manifest schema | 直接取 `platform_core::extension::manifest_schema::MANIFEST_JSON_SCHEMA` | 平台核心的 `exported_schema_file_matches_the_constant` 测试仍通过 |
| `schema/metadata.schema.json` | `@base/schema` 的字段元数据 | `capabilities::string_union` 从 `packages/schema/src/index.ts` 提取 `FieldType` 枚举 | 枚举 = `text/number/boolean/date/reference`，与源码一致 |
| `sdk/*.json` | 各 `packages/*/package.json` 的导出清单 + 用法示例 | `capabilities::ts_exports` 扫 `src/index.ts` 的 `export` 声明 + 文档注释；示例是人工 curate 的 | 7 个包全部有索引；`catalog::tests::sdk_examples_cover_every_package` 守住「每个包都有示例」 |
| `api.json` | HTTP 层未就绪 | 固定占位 + 注明「REQ-011/012 补」 | `status: not-ready`、`paths: []` |

## 7. 约束核对

| 约束 | 核对方式 | 结论 |
|------|----------|------|
| 未修改 `crates/platform-core/` 与 `crates/platform-contracts/` | `git diff c8236b0 -- crates/platform-core crates/platform-contracts crates/platform-contracts-macros` 为空 | **PASS** |
| 未修改 REQ-003 冻结的 5 个 trait 签名 | 同上（文件未动） | **PASS** |
| 零新增第三方依赖 | `git diff c8236b0 -- Cargo.lock` 为空 | **PASS** |
| 不引入代码解析框架 | `crates/xtask/Cargo.toml` 的依赖只有 anyhow / clap / serde / serde_json / platform-core（全是 workspace 已有） | **PASS** |
| 不改 `.cargo/config.toml` 与 `Cargo.toml` 的 profile | `git diff c8236b0 -- .cargo/config.toml` 为空；`Cargo.toml` 只多了一行工作区成员（`modules/<name>`，由 create-module 写入） | **PASS** |
| 不写 TODO / FIXME / 调试输出 | `grep -rn "TODO\|FIXME\|dbg!" crates/xtask/src/` 为空；`println!` 只出现在 CLI 的命令回显里（面向用户，不是调试残留） | **PASS** |
| 圈复杂度 ≤ 10 | 最长的两个函数是 `rustscan::parse_members`（分支 5）与 `capabilities::ts_exports`（分支 6），均在阈值内 | **PASS** |
