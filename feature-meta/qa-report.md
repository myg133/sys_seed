# 验证报告 —— Pre-merge

- 需求: REQ-004 AI 引导体系与能力索引
- 分支: feature/REQ-004 @ `f67029944fef29a54253eafd01b302317093df67`
- 基线: develop @ `c8236b0`（REQ-002 工程基线 + REQ-003 扩展点框架）
- 验证者: QA Agent（独立复现，未采信 Dev 自验报告的数字与结论）
- 工具链: rustc 1.98.1 / cargo 1.98.1 / pnpm 12.19.0 / bun 1.4.4
- 日期: 2026-11-18
- 结论: **PASS-WITH-RISK**（问题 #1 高severity 建议合并前修复；#2/#3 中severity）

> 核心结论：Dev 的**关键数字与结论全部复现为真** —— 咬合点、索引不可手改、索引两段真实性、
> 零新增依赖、回归数字，逐条独立跑通。扣分项不在核心功能，而在**门禁与提示词里的命令**：
> CI 的「强制人工评审」门禁缺一个触发器导致它无法变绿（#1），
> 两份提示词里的测试命令多一个 `--` 会**跑 0 个用例却 exit 0**（#2），
> AGENTS.md 指向仓库里不存在的契约路径（#3）。

---

## 追溯

| AC | 实现 | 测试 | 结论 |
|----|------|------|------|
| **AC-S-004** 索引自动生成不可手改 | `crates/xtask/src/capabilities.rs`（`generate`/`check`/`write_files`）、`crates/xtask/src/main.rs:89-119`（`capabilities --check`）、`.github/workflows/ci.yml:169-183`（`guardrails` job） | 4 种篡改场景全部检出（改 summary / 改 `source_fingerprint` / 多余文件 / 缺失文件），均 exit 1 + `E_CAPABILITIES_OUTDATED`；连续两次生成 13 文件 sha256 逐一相同；重新生成后 `git status` 为空（入库产物 == 生成产物） | **PASS** |
| **AC-035** 引导文件完整且与实际一致 | `AGENTS.md`（157 行）、`prompts/`（8 套）、`capabilities/`（13 文件）、`crates/xtask/src/scaffold.rs::AGENTS_MD` | AGENTS.md「已有能力 / 尚未提供」两段与 `index.json` 的 `already_provided`/`not_yet_provided` **内容集合完全相等**（18 + 7 条，key 与 value 逐条相等），机器守卫 `catalog::tests::agents_doc_matches_catalog` 实测 ok；AGENTS.md 内 13 条命令逐条实跑通过 | **PASS-WITH-RISK**（见 #3：`BA/contracts/...` 路径在仓库内不存在） |
| **AC-036** 提示词可直接产出合规代码 | `prompts/01..08-*.md` | 8 套文件名与设计摘要 §2 完全一致；结构统一（目标/输入/步骤/约束/验收）；步骤引用的命令/目录/包名/API 抽查 15+ 处全部真实存在；提示词第 2 步的脚手架产出实测直接过 `check-modules` + C-1~C-4 | **PASS-WITH-RISK**（等价验证；字面 AI 生成未做，且 #2 的命令会假绿） |
| **AC-037** 脚手架开箱即用 | `crates/xtask/src/scaffold.rs`、`crates/xtask/src/cli.rs:53-64` | `create-module demo-qa-check` 与 `create-module order-management`（长连字符名）**两个名字各自**：13 文件生成 → `check-modules` exit 0 → C-1~C-4 **4/4 全绿** → `cargo fmt --all --check` 通过 → `cargo clippy -p <name> --all-targets -- -D warnings` 0 告警；`--perm-prefix` 分支生成 `shared:item:read/write` 并如实告警前缀不一致，两道关卡仍通过 | **PASS** |
| **AC-038** 能力索引防重造 | `capabilities/index.json`（`already_provided` 18 条 + `not_yet_provided` 7 条）、`permissions.json`、`extension-points.json`、`sdk/*.json` | `already_provided` **18 条逐条 grep 到真实源码**（`RequestContext`/`Repository`/`ListQuery`/`register_platform_module!`/`Manifest`/`Auditable`/`NotificationSender`/`WorkflowHandler`/`TransactionalWrite`/`ConfigScope`/`translate`/`mergeTheme`/`toneClass`/`FieldMeta`/`createApiClient`/`isAuthenticated`/`createToggle`/`platform_module_contract`）；`not_yet_provided` **7 条逐条确认真未实现**（`@base/ui` 仅 Tone/TONES/toneClass/RadiusTokens/defaultRadius，**无 Table/分页**；`@base/auth` **无 can()**；`@base/schema` **无渲染器**；无 HTTP 层 / DB 层）—— 索引**没有把未实现的能力写成已有** | **PASS（口径修正，见 #5）** |

---

## 核心检查项结果

| # | 检查项 | Dev 声称 | 复核结果 | 证据 |
|---|--------|---------|---------|------|
| 1 | **create-module 咬合点** | `create-module demo-test` → `check-modules` ✅ + 契约测试 C-1~C-4 4/4 ✅ | **✅ 复现为真**（用 `demo-qa-check` 与 `order-management` 两个名字各跑一遍） | `模块目录校验通过：2 个模块 / ✓ demo-qa-check / ✓ sandbox` exit 0；`contract_c1_manifest_matches_implementation ... ok`、`c2 ... ok`、`c3 ... ok`、`c4 ... ok` → `4 passed; 0 failed`；`cargo fmt --all --check` exit 0；`cargo clippy -p order-management --all-targets -- -D warnings` exit 0 |
| 1b | 契约测试**非空跑**（本轮自加的失效路径抽查） | 未声称 | **✅ 会咬人** | 删掉 manifest 里 `demo-qa-check:item:write` → `contract_c2 ... FAILED`、`3 passed; 1 failed`；把 manifest `id` 改成 `tampered-id` → `contract_c1 ... FAILED`。非 `assert True` 式假绿 |
| 2 | **capabilities `--check` 手改检测** | 手改 → 失败并报 `E_CAPABILITIES_OUTDATED` | **✅ 复现为真，且覆盖 4 种场景** | ① 改 `already_provided` 一条 summary → `✗ 内容不一致：capabilities/index.json`；② 只改 `source_fingerprint` → 同上；③ 塞多余文件 `capabilities/sdk/rogue.json` → `✗ 多余文件（源码里已无对应产物）`；④ 删 `capabilities/sdk/ui.json` → `✗ 缺失`。**4/4 均 exit 1 + `E_CAPABILITIES_OUTDATED`** |
| 2b | **索引逐字节可复现** | 用 `source_fingerprint` 取代墙钟时间戳，使 check 稳定 | **✅ 复现为真** | 连跑两次 `cargo xtask capabilities`，13 个文件 sha256 **逐一相同**（`diff /tmp/cap_run1.txt /tmp/cap_run2.txt` 无输出）；生成后 `git status` 为空 ⇒ 入库产物 == 生成产物。`index.json` 顶层键为 `already_provided/artifacts/entry_points/generated_by/not_yet_provided/platform/source_fingerprint`，**确认无 `generated_at`** |
| 2c | **`already_provided` 每项真实存在** | 17 条（实为 **18** 条） | **✅ 全部真实存在** | 逐条 grep 源码全部命中（详见追溯表 AC-038） |
| 2d | **`not_yet_provided` 每项确未实现** | 7 条，逐条未实现 | **✅ 全部确认未实现** | `@base/ui` 导出仅 `Tone/TONES/toneClass/RadiusTokens/defaultRadius`，**无 Table/分页**；`packages/`、`apps/` 全仓**无 `can()`**；`packages/schema/src/` **无 render/Renderer**；无 HTTP 层 |
| 3 | **索引生成器零新依赖** | `git diff c8236b0 -- Cargo.lock` 为空 | **✅ 复现为真** | `git diff c8236b0 HEAD -- Cargo.lock \| wc -l` = **0**；`crates/xtask/Cargo.toml` 未改动（依赖仍为 anyhow/clap/serde/serde_json/platform-core）。注：`Cargo.lock` 里的 `syn`/`quote`/`proc-macro2`/`tokio` **基线已存在**（`git show c8236b0:Cargo.lock` 证实），非本需求引入。trait 扫描为**手写字符串扫描**（`crates/xtask/src/rustscan.rs:73` `find("pub trait ")`、`:261` `starts_with("fn ")`、`:155` 括号配对），**未引入 syn/quote/proc-macro2/regex** |
| 4 | **AGENTS.md 命令真实可用** | 逐条跑过 | **✅ 13 条全部实跑通过** | `create-module` ✅ / `create-module --perm-prefix` ✅ / `capabilities` ✅ / `capabilities --check` ✅ / `check-modules` ✅ / `check-deps`（`依赖方向校验通过：平台 crate 未反向依赖业务模块`）✅ / `cargo check` ✅ / `fmt --all --check` ✅ / `clippy --workspace` ✅ / `cargo test --workspace` ✅ / `pnpm -r test` ✅ / `bun run test:e2e` ✅ / `bun run dev` ✅（后端 `HTTP 200 {"status":"ok","version":"0.1.0"}`、前端 `HTTP 200`） |
| 4b | **AGENTS.md 目录/包名与仓库一致** | 一致 | **✅ 一致** | `crates/platform-core`/`platform-contracts`/**`platform-contracts-macros`**/`xtask`/`modules`/`packages`/`apps/shell`/`prompts`/`capabilities` **9 项全部存在**（含任务点名的 `platform-contracts-macros`）；硬规则提到的 `platform-core`/`platform-contracts` 真实；契约条款引用准确（`api-conventions v1` C4=权限点、C6=统一错误响应体、C13=命名与编码约定，逐条比对 `BA/contracts/api-conventions/v1.md` 标题相符） |
| 4c | **AGENTS.md「已有能力」段 == index.json `already_provided`** | 逐行一致 | **✅ 内容完全一致**（顺序不同，见 #6） | Python 独立比对：18 条 key+value 集合**完全相等**；`not_yet_provided` 7 条同样相等。机器守卫 `test catalog::tests::agents_doc_matches_catalog ... ok` |
| 5 | **prompts/ 8 套提示词** | 8 套，统一结构，引用真实 | **✅ 数量/文件名/结构/引用均复现为真** | 8 个文件名与设计摘要 §2 **逐一相符**；每套均含 目标/输入/步骤/约束/验收；占位符统一 `<...>`（不与 Rust 泛型冲突）。抽查引用全部真实：`platform_core::extension::repository::Repository` ✅、`platform_core::register_platform_module!`（`#[macro_export]`，crate root 可达）✅、`NotificationMessage::new/with_recipient/with_payload` ✅、`DeliveryResult::failed` ✅、`RequestContext::new().with_tenant()` ✅、`@base/auth` 的 `AuthState{authenticated,userId,tenantId}` ✅、`@base/sdk` 的 `createApiClient` ✅、`modules/sandbox/backend/src/repository.rs` 的 `block_on` ✅ / `SORTABLE_FIELDS` ✅ |
| 6 | **CI 护栏** | 3 个新 job + CODEOWNERS | **⚠️ 部分成立；门禁有高severity缺陷（#1）** | `guardrails` job 内含 `cargo xtask capabilities --check` + `cargo xtask check-deps` ✅；依赖方向检查**确实复用** REQ-003 实现（`crates/xtask/src/deps.rs` 与 `modules.rs` **diff 为空**，`check-deps` 只是 `main.rs:74-86` 的 CLI 包装，走 `cargo metadata` 图遍历，**未重写成文本匹配**）；REQ-004 新增 CI 段落**无国内镜像前缀**（`.npmrc` 的 `registry.npmmirror.com` 是**基线既有**、非本需求引入）；`.github/CODEOWNERS` 内容合理，`@myg133` 占位已在文件头注明；**分支保护属仓库设置、不在代码里**，已由 `ci.yml:245-246` 注释与 Dev 报告如实记录 ✅ |
| 6b | **CI jq 判定逻辑** | 3 组样本验证过 | **✅ 独立复测通过** | 非作者批准当前 HEAD → `1`（放行）；作者自批 → `0`（拒绝）；批准旧 commit → `0`（拒绝） |
| 7 | **回归** | Rust 98/98、前端 23/23、E2E 1/1 | **✅ 全部复现为真** | Rust `test result` 汇总 **passed=98 failed=0**（+3 ignored 为 doctest）；REQ-003 的 73 个仍全绿：platform-core 41 / platform-contracts 8 / sandbox 6 / macros 0 / xtask 18→43，且 `git diff c8236b0 HEAD -- crates/platform-core crates/platform-contracts crates/platform-contracts-macros modules/` **为空**（平台核心一个字节未动）；clippy `--workspace --all-targets -D warnings` **0 告警**；`cargo fmt --all --check` 通过；`pnpm -r test` **23/23**（8 个包）；`bun run test:e2e` **1/1** |
| 7b | **manifest.json 契约 pin** | 含 contracts + adrs | **✅ 复现为真** | `"contracts": {"api-conventions": "v1", "extension-points": "v1"}`、`"adrs": ["ADR-0001"]` —— **两份契约都 pin** |
| 8 | **通用清单** | 无 TODO/调试输出/密钥 | **✅ 复现为真** | `crates/xtask/src/` + `prompts/` + `AGENTS.md` 内无 `TODO/FIXME/XXX/HACK/dbg!/todo!/unimplemented!`（唯一命中 `catalog.rs:15` 的散文「有 XXX」）；25 处 `println!` 全在 CLI 回显；无硬编码密钥；`capabilities/` **在 `.prettierignore` 内**（生成物不与 prettier 冲突，`prettier --check .` 全绿）且**未被 `.gitignore` 忽略**（`git check-ignore` 未命中，13 个文件均入库）；`eslint .` exit 0 |

---

## 测试

- **Rust 98/98**（`cargo test --workspace -- --test-threads=2`：passed=98, failed=0, ignored=3（doctest））
  - 其中 REQ-003 原有 73 个**全部仍绿**：platform-core 41 / platform-contracts 8 / sandbox 6 / platform-contracts-macros 0 / xtask 18→43（+25）
- **前端 23/23**（`pnpm -r test`，8 个包：hooks 3 / schema 3 / i18n 3 / auth 3 / sdk 2 / theme 2 / ui 3 / shell 4）
- **E2E 1/1**（`bun run test:e2e`，`e2e/shell.spec.ts` chromium，5.2s）
- **lint**：`cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` 0 告警 ✅ / `pnpm exec eslint .` ✅ / `pnpm exec prettier --check .` ✅
- **咬合点**：`create-module demo-qa-check` 与 `create-module order-management` 各 4/4 契约测试全绿 + `check-modules` exit 0
- **负向测试**（本轮自加）：C-1 / C-2 在真实违规下 FAILED（契约套件非空跑）

---

## 问题（按严重度）

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **高** | **CI「强制人工评审」门禁缺 `pull_request_review` 触发器 → 它无法变绿。** workflow 的 `on:` 只声明了 `push` 与 `pull_request` 两个事件，但 `core-change-review` 的 `if:` 却判断 `github.event_name == 'pull_request' \|\| github.event_name == 'pull_request_review'` —— 后半段是**永远走不到的死代码**。后果：PR 改了平台核心 → `pull_request` 事件跑该 job → 此刻还没有批准 → **FAIL**；评审者随后批准（`pull_request_review` 事件）→ **workflow 根本不触发** → 状态仍是 FAIL；而唯一能触发 workflow 的推新 commit 又会让旧批准的 `commit_id` 与新 HEAD 对不上 → 再次 FAIL。需求「平台核心变更在 PR 中打标强制人工评审」里的「强制评审」环节实际无法达成。人工绕过手段：改 PR 描述触发 `pull_request`(edited) 事件，或在 UI 上手动 Re-run failed jobs。 | `.github/workflows/ci.yml:3-7`（`on:` 仅 push/pull_request）vs `.github/workflows/ci.yml:250`（`if:` 引用 `pull_request_review`）；`grep -n "pull_request_review" .github/workflows/ci.yml` 只有 1 处命中，即 `if:` 行，`on:` 段内 0 处 | 在 `on:` 补上 `pull_request_review: types: [submitted, edited, dismissed]`，使「评审提交」真的能触发该 job |
| **2** | **中** | **两份提示词里的测试命令多了一个 `--`，导致「跑 0 个用例却 exit 0」的假绿。** `prompts/02-add-permission.md:35` 的 `cargo test -p <模块名> -- --test-threads=2 -- --nocapture` 与 `prompts/07-write-test-cases.md:18` 的 `cargo test -p <模块名> -- --test-threads=2 -- --list`：多出的 `--` 被 libtest 当成「选项与过滤串的分隔符」，`--nocapture`/`--list` 变成过滤器的一部分，结果是**所有用例被过滤掉**，命令仍返回 0。影响：`prompts/07` 第 2 步「先看现有测试…不要重复写已经覆盖的场景」会列出**空列表**，AI 会照着重复造用例；`prompts/02` 验收项「全绿，且 C-2 无告警」在 0 用例下也成立 → **验收项形同虚设**。同一问题还由脚手架模板下发到每一个新建模块：`crates/xtask/src/scaffold.rs:770` 的注释「需要看到时加 `-- --nocapture`」与其上方命令 `cargo test -p <name> -- --test-threads=2` 拼接后即为坏命令。 | 实测 `cargo test -p sandbox -- --test-threads=2 -- --nocapture` → `test result: ok. 0 passed; ... 2 filtered out` 与 `0 passed; ... 4 filtered out`，**exit=0**；正确写法 `cargo test -p sandbox -- --test-threads=2 --nocapture` → `2 passed` / `4 passed`。`cargo test -p sandbox -- --test-threads=2 -- --list` → 列出 0 个；正确写法 `-- --list` → 正确列出 4 个契约用例 | 删掉多余的 `--`：`-- --nocapture` / `-- --list`。并同步修 `scaffold.rs:770` 的注释措辞（如「改用 `cargo test -p <name> -- --test-threads=2 --nocapture`」） |
| **3** | **中** | **AGENTS.md 与提示词 03 指向的契约文档路径在仓库内不存在。** `AGENTS.md:154-155` 的「参考」段写 `BA/contracts/api-conventions/v1.md` / `BA/contracts/extension-points/v1.md`，`prompts/03-new-sso-protocol.md:20` 也让 AI 去读 `BA/contracts/api-conventions/v1.md`。但 `BA/` **不在 git 仓库里**（`git ls-files BA/` 为空；`git archive develop` 解出的全新克隆里也没有 `BA/` 目录），合并后的 `code/` 工作树下同样没有 `code/BA/`。AGENTS.md 正是 AI 进入仓库读到的**第一份**文件，让它指向不存在的路径，AI 可能因此臆造契约内容。仓库既有惯例是 `docs/dev-environment.md:3` 用 `../../BA/...`（相对 `docs/` 层级），根目录的 AGENTS.md 应相应写成 `../BA/...`；且无论如何都应注明该目录不在仓库内。 | `ls -d code/BA` → `No such file or directory`；`git ls-files BA/` → 空；`git archive develop \| tar -x -C /tmp/qa_clone` 后 `ls -d /tmp/qa_clone/BA` → `No such file or directory`；`AGENTS.md:154-155`、`prompts/03-new-sso-protocol.md:20` | 改为仓库内可解析的路径（把契约正文固化进 `docs/` 或 `capabilities/` 并引用之），或在 AGENTS.md 明示「契约由 BA 侧维护，不随仓库分发，本地路径 `<workspace>/BA/contracts/...`」 |
| **4** | **低** | **`--check` 报「多余文件」时给出的修复方式无效。** 错误信息统一提示「修复方式：运行 `cargo xtask capabilities` 重新生成」，但 `capabilities.rs:153 write_files` **只写不删**：已被移除的包（如 `packages/foo/` 被删）留下的 `capabilities/sdk/foo.json` 不会被清除，CI 会持续红，直到人工删文件。（**缺失文件**场景可自愈 ✅） | `main.rs:109` 的提示文案 vs `capabilities.rs:153-165` 的 `write_files` 只做 `std::fs::write`；实测注入 `capabilities/sdk/stale-pkg.json` → `--check` 报 `多余文件` → 执行文档所述的 `cargo xtask capabilities` → **该文件仍在** → 再 `--check` 依旧 `exit 1` | 让 `write_files` 剪除「生成器不产出」的存量文件（与 `check` 的 `extra` 口径一致），或把提示改成「多余文件需手动删除」 |
| **5** | **低** | **Dev 自验文档把 `already_provided` 条数写成 17，实际是 18。** 产品本身无误（index.json 与 AGENTS.md 一致），错的是 Dev 报告的数字。 | `capabilities/index.json` 实际 `already_provided` = **18** 条；`feature-meta/verification-report.md:187`「`already_provided`（17 条…）」、`feature-meta/traceability.md:28`「`already_provided`（17 条）+ `not_yet_provided`（7 条）」 | BA 合并前让 Dev 更正这两处数字 |
| **6** | **低** | **追溯矩阵称 AGENTS.md 与 index.json「逐行一致」，实际顺序不同。** AGENTS.md 的「已有能力」按「后端接缝 → 前端包」语义序排列，而 `index.json` 的 `already_provided` 因使用 `BTreeMap` 而按**字母序**输出。**内容集合完全相等**（18 条 key+value 逐条相同），不存在「两套说法」；机器守卫 `agents_doc_matches_catalog` 比对的是 `catalog.rs` 与 AGENTS.md（两者同序），不覆盖 index.json 的排序。 | Python 集合比对：`already_provided set-equal: True (18)`、`not_yet_provided set-equal: True (7)`；顺序 AGENTS.md = `request_context, repository, list_query…` vs index = `audit_declaration, auth_state, config…`；`feature-meta/traceability.md:25`「**逐行一致**」 | 把追溯矩阵措辞改为「内容完全一致（顺序按索引的字典序）」，或让生成器按 catalog 顺序输出数组以真正逐行对齐 |

---

## 未覆盖 / 无法验证

| 项 | 状态 | 原因 |
|----|------|------|
| **GitHub Actions 实际运行** | **未验证** | 本仓库**无 remote**（`git remote -v` 为空），Actions 跑不起来。本轮验证的是各 job 的**判定逻辑**（`capabilities --check`、`check-deps`、`git diff` 核心路径命中、`jq` 批准判定 3 组样本）与 YAML 结构；**YAML 在真实 Actions 上的行为未跑过**（含 #1 的触发器问题只能静态判读） |
| **分支保护规则** | **未验证（需人工配置）** | 「强制人工评审」要求把 `core-change-review` 设为 required check 并打开「Require review from Code Owners」。这一步**在仓库设置里、不在代码里**——Dev 已如实登记（`ci.yml:245-246` 注释 + 报告 §7），本轮确认该登记**属实** |
| **CODEOWNERS 维护者账号** | **未验证** | 无 remote，无法确认 GitHub 用户名；当前为 `@myg133` 占位，CODEOWNERS 文件头已注明需按实际情况调整 |
| **AC-036 的字面测试** | **未验证（等价验证）** | 「把提示词原样交给 AI 助手生成模块」本机无可复现的 AI 执行环境。本轮验证的是等价命题：提示词第 2 步要求的 `cargo xtask create-module <name>` 产出物**实测直接通过** `check-modules` 与 C-1~C-4 |
| **AC-038 的字面口径** | **需 BA 修订** | AC 原文期望「复用 `@base/ui` 表格与 `@base/schema` 渲染器」，但两者本期**确实不存在**（本轮已独立复核：`@base/ui` 仅 tone/radius token，无 Table/分页；`@base/schema` 无渲染器）。本轮以 `not_yet_provided` 的诚实口径达成「防重造 + 防幻觉」，**该 AC 原文需 BA 修订**（Dev 报告 §8 亦已提出，本轮复核认同） |
| **测试覆盖率** | **未测量** | 本机无覆盖率工具，且按低配机约束不 `cargo install` 任何工具 |
| **冷编译峰值内存** | **未测量** | 按约束禁止 `cargo clean`，未做 RSS 采样 |
| **`prompts/` 的提示词效果 A/B 评测** | **未做** | 设计摘要 §6 明确「不做 AI 行为的自动化评测」 |

---

## 清理确认

本轮在 worktree 内临时创建的 `modules/demo-qa-check`、`modules/order-management`、`modules/demo-perm`
三个验证用模块**已全部删除**，根 `Cargo.toml` / `Cargo.lock` / `capabilities/` 已还原，
`cargo xtask capabilities --check` 复检通过（13 个文件与源码一致）。

除本报告（`feature-meta/qa-report.md`，本次唯一交付物）外，**工作树干净**：

```
$ git status --porcelain
$ git status --short --untracked-files=all
（均为空）
```

> 注：`feature-meta/qa-report.md` 是**滚动槽位**——BA 自己的 `b0a26c9` 提交即以 REQ-003 的
> 首轮 QA 报告覆盖了 REQ-002 的版本。本轮按同一惯例写入 REQ-004 的报告。
