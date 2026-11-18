# 验证报告 —— 修复复核（Recheck）

- 需求: REQ-004 AI 引导体系与能力索引
- 分支: feature/REQ-004 @ `f45ef8826eccc1c65c301a676118e1b070b98e1e`
- 首轮报告: `feature-meta/qa-report.md`（结论 PASS-WITH-RISK，@ `f670299`）
- 复核范围: 3 个必修点 + 一次回归（不重做全量验证）
- 验证者: QA Agent（独立复现，未采信 Dev 报告中的任何数字与结论）
- 工具链: rustc 1.98.1 / cargo 1.98.1 / jq 1.8.2 / pnpm 12.19.0 / bun 1.4.4 / Python 3 + PyYAML 6.0.3
- 日期: 2026-11-18
- 结论: **PASS**（3 个必修点全部独立复现为真；遗留 2 项低severity 残留风险，均已如实登记，未被当成已解决）

> 核心结论：首轮点名的 **高 1 / 中 1 / 中 2 三个必修点，本轮全部独立复现为真**。
> CI 触发器已补齐且 jq 审批判定在 7 组样本上全部正确；prompts 全部 14 条命令逐条实跑，
> 每条 `cargo test` 都真的跑到用例（6 个），假绿写法反向验证确实存在（0 用例 + exit 0）；
> 契约正本唯一（快照与正本逐字节相同，手改必失败）。
> 首轮 3 项低severity（#4 多余文件清理、#5 条数 17→18、#6 「逐行一致」措辞）**也一并修好了**。
> 本轮新发现 2 项**低severity 残留风险**（均不阻塞合并），已在「仍存在的问题」登记。

---

## 逐项复核

| # | 问题 | Dev 声称 | 复核结果 | 证据 |
|---|------|---------|---------|------|
| **高 1** | **CI 缺 `pull_request_review` 触发器 → 强制评审门禁永远无法变绿** | `on:` 补 `pull_request_review: [submitted, edited, dismissed]`，`pull_request` 补 `review_requested` | ✅ **复现为真** | `.github/workflows/ci.yml:12-13` 已有 `pull_request_review: types: [submitted, edited, dismissed]`；`.github/workflows/ci.yml:9` 的 `pull_request.types` 为 `[opened, synchronize, reopened, review_requested]`。**触发器名与类型对照 GitHub 官方文档**：`github/docs` 源文件 `content/actions/reference/workflows-and-actions/events-that-trigger-workflows.md:662-679` 明确 `pull_request_review` 的 activity types 正是 `submitted` / `edited` / `dismissed`（三者完全一致，无非法类型）。 |
| **高 1b** | **审批判定的 jq 表达式在 review 事件下成立** | 3 组样本验证过 | ✅ **7 组样本全部正确（表达式从 `ci.yml` 逐字提取，未手打）** | 用 Python 从 YAML 里取 `core-change-review` 的 `run:` 原文，只把 `gh api .../reviews` 换成 `cat "$SAMPLE"`，其余**逐字不动**；再用真实 SHA（`BASE_SHA=4711b75`、`HEAD_SHA=ae482a9`，二者间确实改了 `crates/platform-core/`，走的是「改了核心」分支而非早退分支）喂样本：<br>① 非作者批准 → **exit 0**（输出「平台核心变更已由非作者评审者批准 ✓」）<br>② 作者自批 → **exit 1**<br>③ 批准的是旧 commit → **exit 1**<br>④ 非作者批准旧 commit + 作者批准当前 HEAD 混合 → **exit 1**<br>⑤ 仅 `CHANGES_REQUESTED`/`COMMENTED` → **exit 1**<br>⑥ 旧 commit 批准 + 当前 HEAD 的 `DISMISSED` → **exit 1**<br>⑦ 空评审数组 → **exit 1**<br>⑧ 反向：PR 未改平台核心 → **exit 0**（「本次 PR 未修改平台核心，无需核心评审。」早退分支正确） |
| **高 1c** | **`pull_request_review` payload 里 job 用到的字段全部存在** | 未单独声称 | ✅ **成立** | job 用到的 4 个上下文字段：`github.event.pull_request.base.sha` / `.head.sha` / `.number` / `.user.login`（`ci.yml:220-221,283-286` 提取）。<br>① GitHub 官方 webhook payload 文档（`docs.github.com/en/webhooks/webhook-events-and-payloads`）的 `pull_request_review` 段列出 `pull_request object Required` —— 即 review 事件携带完整 PR 对象；<br>② GitHub 官方 REST 文档 `rest/pulls/pulls` 的 pull-request schema 原文含 `"base": { ... "sha": "6dcb09b5..." }`、`"head": { ... "sha": "6dcb09b5..." }`、`"number": 1347`、`"user": { "login": "octocat" }` → 4 个字段逐一存在。<br>③ 关键细节：`pull_request_review` 事件**顶层没有 `number`**，job 用的是 `github.event.pull_request.number`（对象内取），路径正确（顶层 `number` 只属于 `pull_request` 事件）。 |
| **高 1d** | **YAML 可解析** | — | ✅ | `yaml.safe_load('.github/workflows/ci.yml')` 成功；9 个 job（lint/test/build/dependency-audit/contract-check/e2e/guardrails/core-change-label/core-change-review）全部可枚举；`pull_request_review` 键存在且 types = `['submitted','edited','dismissed']` |
| **高 1e** | **分支保护属仓库设置，「强制人工评审」要真生效还需人工配置 —— 是否被如实说明** | 已如实登记 | ✅ **如实说明，未被当成已解决** | `.github/workflows/ci.yml:263-267` 用 ⚠️ 明确写出「以下两步**在仓库设置里、不在代码里**，必须人工配置，代码无法代劳：1. 把 `core-change-review` 设为 required check；2. 打开「Require review from Code Owners」」，并补充「`pull_request_review` 事件只在工作流文件已存在于默认分支时触发，所以本文件合并进默认分支之后评审触发的门禁才会生效」。`feature-meta/verification-report.md:257-258` 与 `:392`（登记为「未验证（需人工配置）」）口径一致。**没有任何地方把该门禁写成「已解决/已生效」**。 |
| **中 1** | **prompts 02/07 的命令多一个 `--` → 跑 0 个用例却 exit 0，假绿** | 已删除多余的 `--`；坑写进 AGENTS.md 硬规则第 8 条 | ✅ **复现为真** | `prompts/02-add-permission.md:35` = `cargo test -p <模块名> -- --test-threads=2 --nocapture`（单 `--`）<br>`prompts/07-write-test-cases.md:18` = `cargo test -p <模块名> -- --test-threads=2 --list`（单 `--`）<br>`crates/xtask/src/scaffold.rs:770` 注释同步改为「需要看到时加 `--nocapture`（注意：`cargo test` 的参数分隔符 `--` 只写一次）」<br>**全仓扫描无残留坏命令**：`grep -rE '\-\- +--'` 命中项全是正确的单 `--` 形式、AGENTS.md:54 的 ❌ 反例、以及验证报告里对修复的历史记录。 |
| **中 1b** | **硬规则里已写明「多一个 `--` 会假绿」** | 已写进第 8 条 | ✅ | `AGENTS.md:50-55` 硬规则第 8 条：「验收命令必须真的跑到用例：`cargo test` 的参数分隔符 `--` **只写一次**……结果是**一个用例都没跑、但退出码仍是 0** —— 假绿，验收形同虚设」，并给出 ✅/❌ 对照与「验收前先看输出里的『N passed』，**N 必须 > 0**」 |
| **中 2** | **契约引用对只 clone 代码仓库的人也可操作** | 方案 (c)：正本 `git show demand:contracts/...` + 仓库内快照 `docs/contracts/` | ✅ **复现为真** | `AGENTS.md:169-170` 参考段指向 `docs/contracts/api-conventions/v1.md`（快照）+ `git show demand:contracts/api-conventions/v1.md`（正本）；`AGENTS.md:174-180` 明确说明「`BA/` 是 workspace 里 `demand` 分支的 worktree，契约正文在它的 `contracts/` 下」并给出 `git fetch origin demand`。**首轮的 `BA/contracts/...` 失效引用在 `AGENTS.md` / `prompts/` 中已全部消失**（全仓剩余命中只在验证报告的历史记录与 `docs/contracts/README.md:11` 的说明性文字里）。`prompts/03-new-sso-protocol.md:21` 已改为 `git show demand:contracts/api-conventions/v1.md`。 |
| **中 2b** | **`scripts/sync-contracts.sh --sync` 能跑通，生成的快照与正本逐字节相同** | 声称一致 | ✅ **独立复现为真** | `./scripts/sync-contracts.sh --sync` → exit 0，`✓ 已同步 docs/contracts/api-conventions/v1.md`、`✓ 已同步 docs/contracts/extension-points/v1.md`。随后 `cmp` 逐字节比对：<br>`api-conventions`：正本 `git show demand:contracts/api-conventions/v1.md` vs 快照 → **逐字节相同**，sha256 `911f6ae22a99a352b7d9b77863a13f266bc41252f42713211b0b9226cfbfed3e`<br>`extension-points`：**逐字节相同**，sha256 `9c6a6999ea0db83a3951894359efcab4c4a6810a9350df9c32c9457eb2949c23`<br>`--sync` 后 `git status -- docs/contracts/` 为空（无漂移）。**Dev 对正本路径的更正确认**：分支内真实路径是 `contracts/<name>/<version>.md`，`BA/` 只是 worktree 目录 —— `git ls-tree -r demand --name-only` 显示 `contracts/api-conventions/v1.md`，**没有 `BA/contracts/...`**。 |
| **中 2c** | **手改快照 → 校验脚本与 CI 步骤必须失败 → 再 `--sync` 恢复** | 声称会失败 | ✅ **独立复现为真** | 往 `docs/contracts/api-conventions/v1.md` 追加一行注释后跑 `./scripts/sync-contracts.sh`（CI `ci.yml:197` 跑的就是这一条）：<br>`✗ 快照与 BA 正本不一致：docs/contracts/api-conventions/v1.md` → **exit 1** ✅<br>再跑 `--sync` → exit 0，`cmp` 确认**已恢复为正本**，`git status -- docs/contracts/` 为空。 |
| **中 2d** | **正本唯一：仓库内没有第二份可编辑的契约正本** | `capabilities/` 是生成的快照 | ✅ **成立** | `capabilities/extension-points.json` 虽含契约派生信息，但其 `"generated_by": "cargo xtask capabilities"`、`"source": "crates/platform-core/src/extension/"` → 是**从 Rust 源码扫描生成的产物**，由 `cargo xtask capabilities --check` 在 CI 强制校验（`ci.yml:189`），**不是手工维护的第二份**。`capabilities/` 下的内容均无「契约条款正文」，只有 trait 名/签名摘要。仓库内 `docs/contracts/` 是唯一的契约正文副本，且被脚本 + CI 双向锁死。 |
| **中 2e** | **`AGENTS.md` 的契约引用路径现在真的能打开** | 声称可操作 | ✅ **成立（纯代码克隆实测）** | 用 `git clone --branch feature/REQ-004 --single-branch` 做了一份**只有代码、没有 demand 分支**的干净克隆：<br>`docs/contracts/api-conventions/v1.md` ✓ 存在（可 `head` 读出正文）<br>`docs/contracts/extension-points/v1.md` ✓ 存在<br>`modules/sandbox/`、`capabilities/index.json`、`prompts/` ✓ 存在<br>纯代码克隆里 `git show demand:...` 不可用（无该分支），但 AGENTS.md 同时给了「`git fetch origin demand`」的补齐指引 → **两条路径至少有一条永远可用**。 |
| **低 #4** | **`--check` 报「多余文件」时提示的修复方式无效** | `write_files` 已改为按 check 同口径剪除 | ✅ **已修好** | 注入 `capabilities/sdk/stale-pkg.json` → `--check` exit 1 报「多余文件」；按提示跑 `cargo xtask capabilities` → 输出「✓ 已清理多余文件：capabilities/sdk/stale-pkg.json」；再 `--check` → exit 0「13 个文件与源码一致」；`git status -- capabilities/` 为空（已还原）。新增单测 `capabilities::tests::write_files_prunes_stale_artifacts` 在 99 个用例里跑通。 |
| **低 #5** | **Dev 自验文档把 `already_provided` 写成 17，实际 18** | 已更正 | ✅ **已更正** | `feature-meta/traceability.md:25` 与 `:28`、`feature-meta/verification-report.md:201` 均写 **18 条**（+ `not_yet_provided` 7 条）；实测 `jq '.already_provided\|length' capabilities/index.json` = **18**、`.not_yet_provided\|length` = **7** |
| **低 #6** | **追溯矩阵称 AGENTS.md 与 index.json「逐行一致」，实际顺序不同** | 措辞已更正 | ✅ **已更正** | `AGENTS.md:96-99` 写「**内容完全一致**（18 条，key 与说明逐条相同）；**行的顺序不同** —— 下表按『后端接缝 → 前端包』的语义序，索引按字典序输出」；`feature-meta/traceability.md:25`、`feature-meta/verification-report.md:205`（「这叫『内容一致』，**不是『逐行一致』**」）、`feature-meta/status.md:40` 口径一致 |

---

## prompts 命令逐条实测

从 `prompts/` 全部 8 个文件里**逐条抽出** shell 命令（fenced bash 块 + 行内反引号，共 54 处引用 → **14 条去重命令**），
`<模块名>` / `<name>` 一律代入真实模块名 `sandbox` 逐条实跑。

| 命令 | 用例数 | exit | 判定 |
|------|--------|------|------|
| `cargo xtask check-modules` | n/a（2 个模块） | 0 | ✅ PASS |
| `cargo test -p sandbox -- --test-threads=2` | **6**（2 lib + 4 契约） | 0 | ✅ PASS（首轮点名项，**用例数 > 0**） |
| `cargo fmt --all --check` | n/a | 0 | ✅ PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | n/a | 0 | ✅ PASS（**0 告警**） |
| `cargo xtask capabilities --check` | n/a（13 文件） | 0 | ✅ PASS |
| `cargo xtask capabilities`（裸生成） | n/a（13 文件） | 0 | ✅ PASS（幂等：生成后 `git status -- capabilities/` 为空） |
| `pnpm exec prettier --check .` | n/a | 0 | ✅ PASS |
| `pnpm exec eslint .` | n/a | 0 | ✅ PASS |
| `cargo test -p sandbox -- --test-threads=2 --nocapture`（**首轮点名项**） | **6**（`2 passed` + `4 passed`） | 0 | ✅ PASS **假绿已消除** |
| `cargo test -p sandbox -- --test-threads=2 --list`（**首轮点名项**） | **列出 6 个** | 0 | ✅ PASS **假绿已消除** |
| `git show demand:contracts/api-conventions/v1.md` | n/a | 0 | ✅ PASS（正本可读） |
| `cargo xtask create-module <name>` | 4（C-1~C-4） | 0 | ✅ PASS（以真实模块名 `demo-qa` 实跑；验完已删） |
| `cargo xtask create-module sso-<协议名>` | — | — | ⚪ **占位符未实跑**（`<协议名>` 无实参；`create-module` 路径已由上一行 `demo-qa` 覆盖） |
| `cargo test --workspace -- --test-threads=2` | **99** | 0 | ✅ PASS |

**反向验证（假绿确实存在过）** —— 用错误的双 `--` 形式跑同一条命令：

| 命令 | 用例数 | exit | 判定 |
|------|--------|------|------|
| `cargo test -p sandbox -- --test-threads=2 -- --nocapture` | **0**（`0 passed; 2 filtered out` + `0 passed; 4 filtered out`） | **0** | ⛔ **假绿复现**（0 用例却 exit 0） |
| `cargo test -p sandbox -- --test-threads=2 -- --list` | **0**（列出 0 个） | **0** | ⛔ **假绿复现** |

> 结论：**首轮点名的两条命令修复前后行为对照成立** —— 正确写法 6 个用例 / exit 0，
> 错误写法 0 个用例 / exit 0（假绿）。该坑已被写进 `AGENTS.md:50-55` 硬规则第 8 条。

---

## 回归

- **Rust 99/99**（`cargo test --workspace -- --test-threads=2`：passed=**99**, failed=0；另 3 个 doctest ignored）
  - 分解：platform-contracts 8 / platform-contracts-macros 0 / platform-core 41 / platform-core bin 0 / sandbox 2+4=6 / xtask 44
  - **REQ-003 的 73 个仍全绿**：基线 `c8236b0` 的 xtask `#[test]` 计数实测 **18**，非 xtask 基线 55（8+41+6）→ 55+18 = **73**；
    且 `git diff c8236b0 HEAD -- crates/platform-core crates/platform-contracts crates/platform-contracts-macros modules/` **为空**（平台核心与业务模块一个字节未动）
- **前端 23/23**（`pnpm -r test`，8 个包：hooks 3 / schema 3 / i18n 3 / auth 3 / sdk 2 / theme 2 / ui 3 / shell 4）
- **E2E 1/1**（`bun run test:e2e`，chromium，`✓ 1 [chromium] › e2e/shell.spec.ts:3:1 › 底座壳渲染并可过滤菜单 (214ms)`，1 passed / 3.6s）
- **Cargo.lock diff 0 行**（`git diff c8236b0 HEAD -- Cargo.lock | wc -l` = **0**）；`git diff c8236b0 HEAD --stat -- '**/Cargo.toml'` = **空** → **零新依赖**
- **clippy** `cargo clippy --workspace --all-targets -- -D warnings` → **0 告警**，exit 0
- **fmt** `cargo fmt --all --check` → 通过，exit 0
- **capabilities** `cargo xtask capabilities --check` → 「能力索引校验通过：13 个文件与源码一致」，exit 0
- **契约快照** `./scripts/sync-contracts.sh` → 「✓ 快照一致」×2，exit 0
- **依赖方向** `cargo xtask check-deps` → 「依赖方向校验通过：平台 crate 未反向依赖业务模块」，exit 0
- **脚手架咬合点** `cargo xtask create-module demo-qa` → 13 文件生成 → `cargo xtask check-modules`「模块目录校验通过：2 个模块 / ✓ demo-qa / ✓ sandbox」exit 0 → `cargo test -p demo-qa` → **C-1/C-2/C-3/C-4 4/4 全 ok**（`4 passed; 0 failed`）→ **已完整删除**（`mavis-trash`），`Cargo.toml`/`Cargo.lock` 已 `git checkout` 还原，`capabilities --check` 复检通过
- **通用清单**：无 TODO/FIXME/XXX/HACK/dbg!/todo!/unimplemented!（`crates/xtask/src/` + `prompts/` + `AGENTS.md` + `scripts/` + `docs/contracts/` 全空）；27 处 `println!/eprintln!` **全在 `crates/xtask/src/main.rs` 的 CLI 回显**里；无硬编码密钥；`docs/contracts/`（3 文件）与 `scripts/sync-contracts.sh` **均在 `git ls-files` 内、未被 `.gitignore` 误伤**（`git check-ignore` 未命中）；**prettier 不会与生成的快照冲突** —— `.prettierignore` 含 `*.md` 与 `capabilities/`，`pnpm exec prettier --check .` exit 0

---

## 仍存在的问题

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| 1 | 低 | **契约快照校验在「拿不到 demand 分支」时静默跳过（exit 0），CI 门禁可能空转。** `scripts/sync-contracts.sh:38-42` 在找不到 `demand` / `origin/demand` 时打印「跳过：本地没有 BA 分支……」并 **exit 0**。在真实 GitHub CI 中 `ci.yml:182` 的 `fetch-depth: 0` 会拉全部分支、`origin/demand` 存在，所以正常路径是好的；但一旦 BA 分支被改名/删除、或 CI 的 checkout 配置变化导致拿不到正本，这条「防漂移」校验会**空过**，快照与正本漂移将无人发现。`ci.yml:195` 的注释已如实说明这一行为，但**行为本身仍是「静默通过」**。 | 纯代码克隆实测：`./scripts/sync-contracts.sh` → 「跳过：本地没有 BA 分支（demand / origin/demand），无法校验契约快照。」→ **exit=0**（`scripts/sync-contracts.sh:38-42`） | CI 里把「跳过」判为失败（或至少 `::warning::` + 让 required check 感知），例如加 `CI=true` 时缺 demand 即 exit 1；本地手工跑保持跳过友好。 |
| 2 | 低（残留风险，非缺陷） | **「强制人工评审」要真正生效仍需 2 项仓库设置 + 1 个前提条件，且 `pull_request_review` 触发器要等本文件进入默认分支才生效。** 这三点代码无法代劳，Dev 已如实登记为「需人工配置 / 未验证」，本轮确认登记属实、**未被当成已解决**。 | `.github/workflows/ci.yml:263-267`（⚠️ 注释：required check + Require review from Code Owners + 「workflow 文件合并进默认分支后才生效」）；`feature-meta/verification-report.md:257-258`、`:392` | 合并后由 BA/维护者到仓库设置里把 `core-change-review` 设为 required check 并打开「Require review from Code Owners」；本条为**人工待办**，不是代码缺陷。 |

> 除以上 2 项低severity外，**未发现任何可复现的高/中severity 问题**。3 个必修点全部独立复现为真，首轮 3 项低severity 亦已修好。

---

## 未验证

| 项 | 状态 | 原因 |
|----|------|------|
| **GitHub Actions 的真实运行** | **未验证** | 本仓库**无 remote**（`git remote -v` 为空），Actions 跑不起来。本轮验证的是：触发器名与 activity types 对照 GitHub 官方文档源**逐字相符**；jq 审批判定用**从 `ci.yml` 逐字提取的脚本**喂 7 组样本；payload 字段对照 GitHub 官方 webhook payload 文档 + REST schema 确认存在；YAML 可被 PyYAML 解析。**真实 Actions 调度行为（含 required check 生效、并发去重、fork PR 行为）本轮未跑过**。本机也无 `actionlint`/`zizmor`，且按约束不安装新工具。 |
| **分支保护规则 / `core-change-review` 作为 required check 的实际效果** | **未验证（需人工配置）** | 属仓库设置、不在代码里。已确认该登记在 `ci.yml:263-267` 与验证报告中**如实存在**，未被写成已解决。 |
| **CODEOWNERS 维护者账号 `@myg133`** | **未验证** | 无 remote，无法确认 GitHub 用户名；占位状态与首轮一致（文件头已注明需按实际情况调整），非本轮引入。 |
| **`pull_request_review` 事件在真实 GitHub 上的触发频率与并发行为** | **未验证** | 需真实 PR 与真实 Actions。静态层面已确认触发器与 activity types 合法、job 读取的字段在 payload 中存在。 |
| **`gh api repos/.../pulls/N/reviews --paginate` 的真实分页输出** | **等价验证** | 本机无 remote / 无 token，无法调真 API。已用 `--paginate` + `jq -s` 的**同一段管道**喂多页形态的评审数组（样本 s4/s6 含多条记录，含 `DISMISSED` 态），结论与单页一致。 |
| **AC-036 的字面测试（把提示词原样交给 AI 产出模块）** | **未验证（等价验证）** | 与首轮同：本机无可复现的 AI 执行环境。本轮验证的是更强的等价命题 —— **提示词里 14 条命令逐条实跑、每条都真跑到用例**；且 `prompts/01` 要求的 `create-module` 产出物实测直接过 `check-modules` + C-1~C-4。 |
| **AC-038 的字面口径** | **需 BA 修订（与首轮同）** | AC 原文期望「复用 `@base/ui` 表格与 `@base/schema` 渲染器」，但两者本期确实不存在（`not_yet_provided` 已如实登记）。本轮**未重做**该项全量验证，沿用首轮结论。 |
| **测试覆盖率 / 冷编译峰值内存** | **未测量** | 按低配机约束不 `cargo install` 任何工具、不 `cargo clean`。 |

---

## 清理确认

本轮临时创建物已全部清除：

- `modules/demo-qa`（脚手架咬合点验证用）→ 已删除（`mavis-trash`），`Cargo.toml` / `Cargo.lock` 已 `git checkout` 还原
- `capabilities/sdk/stale-pkg.json`（多余文件剪枝验证用）→ 已由 `cargo xtask capabilities` 自动清理，`--check` 复检通过
- `docs/contracts/api-conventions/v1.md`（篡改验证用）→ 已由 `--sync` 还原为正本，逐字节比对通过
- 验证脚本 / 样本 / 克隆目录全部落在 `/tmp/qa-recheck/`，**未进入仓库**

**工作树状态**：

```
$ git status --porcelain --untracked-files=all
 M feature-meta/qa-report-recheck.md
 M feature-meta/qa-report.md
```

**两处 `M` 的来源（均已核实，非本轮误改）**：

| 文件 | 状态 | 说明 |
|------|------|------|
| `feature-meta/qa-report.md` | ` M`（仅工作区） | **首轮 QA 报告**，本轮任务书明确要求不得覆盖 → 本轮**逐字节未触碰**。其「修改」状态来自首轮 QA 写入后未提交；Dev 的 3 个修复提交（`9dba569` / `109f93e` / `f45ef88`）均**未包含**该文件（已用 `git show --stat` 逐一确认）。 |
| `feature-meta/qa-report-recheck.md` | ` M`（仅工作区） | **本轮交付物**。该路径在 HEAD 里**已存在**，内容是 **REQ-003 的修复复核报告**（由 BA 的 `b0a26c9`「[BA] 归档 REQ-003 的 QA 首轮报告与修复复核报告」提交入库）。按本轮任务书指定的路径写入 REQ-004 的复核报告，与首轮 `qa-report.md` 的**滚动槽位**惯例一致（首轮报告已记录：「`feature-meta/qa-report.md` 是**滚动槽位**——BA 自己的 `b0a26c9` 提交即以 REQ-003 的首轮 QA 报告覆盖了 REQ-002 的版本」）。**REQ-003 的原复核报告未丢失**，仍完整保存在 git 历史 `b0a26c9` 中（`git show b0a26c9:feature-meta/qa-report-recheck.md` 可取回）。 |

> 本轮**未 commit / 未 push / 未改动任何源文件**（QA 硬规则：不写代码、不修 bug、不提交）。
> 上述两处 `M` 均为「已跟踪文件的工作区改动」，未进入暂存区。
> 所有验证用临时物（脚本、样本、克隆目录）都在 `/tmp/qa-recheck/`，**不在仓库内**。
