# REQ-014 审计日志 —— 修复轮**第 2 轮聚焦复核**报告

- **被验对象**：`/home/myg133/Projects/system_seed/feature-REQ-014`，分支 `feature/REQ-014`，HEAD `5f28c0c`
  - 修复轮 1 + 2 **均未提交**；实际状态为 `git status --short` 的 **13 M + 4 ??（共 17 个文件）**
- **验收基线**：`BA/demands/REQ-014-audit/acceptance.md` rev 3
- **决策基线**：`BA/demands/REQ-014-audit/decisions.md` rev 5（**D194**；D195 在 `req014-fixround-brief.md` 附录 A.3，见 §2 I-02）
- **任务卡**：`BA/feature-meta/req014-recheck2-brief.md`
- **上一轮复核**：`BA/feature-meta/qa-recheck-req014.md`（FAIL：0 阻塞 / 1 高 / 2 中 / 3 低）
- **第 2 轮 Dev 自述**：`BA/feature-meta/req-014-dev-fixround2.md`（**未被采信**，全部数字自己重跑）
- **复核人**：QA 复核子 Agent（独立注入 7 处变异/对照，全部逐字节还原）
- **复核日期**：2026-11-20
- **复核前后** `git status --short` 与 Dev 交付状态**逐文件相同**；17 个文件 sha256 **全部逐字节相同**

---

## 1. 结论

# **PASS-WITH-RISK**

**0 条阻塞 / 0 条高 / 0 条中 / 3 条低**。

第 2 轮的四件事（F11~F14）**全部落地且经我独立验证**：

- **F11（上轮的 H-01）已彻底收口** —— 我从零重建测试库（6 步，含 `e2e-server.sh` 缺的 3 步），
  起点 `tenant-a = 11 < 默认 limit 20`，全量 E2E **35 passed / 0 failed / 0 skipped**；
  再把修复行删掉、**在同一个干净库**上重跑，逐字复现「按钮 disabled → 点击超时」的
  **1 failed / 5 passed**。**红绿由这一行决定，不是环境偶然 favorable。**
- **F12** 的跨 crate 三处比对**不是死代码**：4 路变异（运行时默认 / manifest 断言 / manifest 实际值 / DB 行）
  全部能抓红；注释已改为与代码相符的「两处 + 契约口径常量」。
- **F13** 主报告已如实订正（925→927、125→127、F10 已落地），**明确留痕「这不是当初就写对了」**，
  `req-014-dev-fixround-f10.md` **未被改动**。
- **F14** 用例驱动的是**生产写入路径**（`complete_oidc_callback` → `record_audit` → 注入的 sink），
  不是桥接器；事件码 / `object_type` / `tenant_id` / `ip` / 成功分支的 `user_agent` 均有断言，
  两处变异能抓红。

判 **PASS-WITH-RISK** 而非纯 PASS，是因为 F14 的**失败分支没有断言 `user_agent`** ——
我用反向变异证明「把失败分支的 UA 置空，用例仍然绿」（§4.7），
这是 AC-S-127 步骤 3「`ip` / `userAgent` 逐字透传」的一处窄缺口。它**不阻塞合并**
（成功分支已钉住、代码确实透传、只是失败分支缺一行断言），建议下一轮顺手补 1 行。

### 分档汇总

| 档位 | 条数 | 编号 |
|---|---|---|
| **阻塞** | **0** | — |
| **高** | **0** | — |
| **中** | **0** | — |
| **低** | **3** | L-01（失败分支 UA 未断言）、L-02（config 用例名仍称 `is_the_same_everywhere`）、L-03（TD-040 真库落表判据仍缺） |
| 说明性（不算问题） | 2 | I-01（任务卡 D4「15 个」与所列 17 项不符）、I-02（D195 不在 decisions.md） |

### A / B / C / D / E 五组判定一览

| 组 | 项 | 判定 |
|---|---|---|
| **A.1** | 从零重建库（6 步全跑） | ✅ 播种后 `all=11 / tenant-a=11 / fixture=11`；播种前 `0` |
| **A.2** | 干净库全量 E2E | ✅ **35 passed / 0 failed / 0 skipped**（共 35 条 = 全跑） |
| **A.2** | 审计页 6 条逐条实跑 | ✅ 6/6 ✓，**0 skipped**（`--list` 共 35 条，实跑 35 条） |
| **A.3** | 负向对照（删 `selectOption('10')`） | ✅ **1 failed / 5 passed**，`<button disabled>` → 点击超时，逐字复现 H-01 |
| **B1** | `DEFAULT_LIMIT` 20→30 | ✅ 新断言 **FAILED**（`left: Number(20) / right: Number(30)`） |
| **B2** | 断言期望 `json!(20)`→`json!(21)` | ✅ **FAILED**（`left: 20 / right: 21`） |
| **B2'** | （加强）manifest **实际值** 20→21 | ✅ **FAILED**（`left: 21 / right: 20`）→ 断言真的读 manifest |
| **B3** | DB 行值 20→21 | ✅ **FAILED**（`left: Some(21) / right: Some(20)`） |
| **B4** | config 侧 doc 与代码相符 | ✅ 「两处（不是三处）」+ 明写第 2 处是契约口径常量；危险说法 0 命中 |
| **C1** | F13 订正 + 留痕 | ✅ 925→927 / 125→127 / F10 已落地 / 原结论保留并标注；f10 文件 mtime 仍 12:56:57 |
| **C2** | F14 驱动生产路径 | ✅ 真实路由 → `http.rs:507` → `complete_oidc_callback` → `record_audit`；非桥接器 |
| **C3-1** | 成功分支 `ip` → `"0.0.0.0"` | ✅ 用例 1 **FAILED**（`left: "0.0.0.0" / right: "203.0.113.9"`） |
| **C3-2** | 失败分支 sso 标志 → false | ✅ 用例 2 **FAILED**（`实际记录的事件码：["iam.login.failed"]`） |
| **C3-3** | （反向）失败分支 UA → `""` | ⚠️ **仍绿** → 覆盖缺口（L-01） |
| **D1** | 逐 crate 11 个 | ✅ **930 / 0 failed / 0 ignored**（自己 awk 算；比上轮 927 **+3**，无下降） |
| **D2** | 前端 3 包 + build + svelte-check | ✅ **206 / 43 / 38**；build 通过；**0 errors 0 warnings** |
| **D3** | clippy / fmt | ✅ clippy 只剩基线 **2 条**、新增 **0**；fmt 两侧 **11 文件**、新增 **0** / 消失 **0** |
| **D4** | 越界 | ✅ **恰好**任务卡所列的 **17 个**（卡面写「15 个」→ 见 I-01） |
| **D5** | capabilities / aw verify | ✅ 14 文件一致；**0 FAIL / 0 WARN** |
| **E** | TD-039 / TD-040 | ✅ TD-039 属实（脚本确实缺 3 步，我补跑才起来）；TD-040 依赖图与 BA 所述一致，**不成环** |

---

## 2. 逐条问题

> 可判定性分档按 dev-guide §9.12：主体不存在 / 物理不可达 / **不可判定** / **口径不一致** / **表述**。

### L-01 【低】F14 的**失败分支**没有断言 `user_agent` —— AC-S-127 步骤 3 的「逐字透传」只钉住了一半

- **关联 AC**：AC-S-127 步骤 3（`ip` / `userAgent` **逐字**透传）
- **关联检查**：任务卡 §5 **C2**（要求断言 ip / user_agent）
- **可判定性分档**：**不可判定**（该行为在仓库里没有机械断言）
- **事实**：`crates/platform-iam/tests/sso_login_audit.rs`
  - 用例 1（成功，`:233`）断言 `ip`（`:259`）**和** `user_agent`（`:264`）✅
  - 用例 2（失败，`:286`）只断言了 `ip`（`:319`），**没有** `event.user_agent` 断言
- **决定性证据（我注入的反向变异）**：把失败分支的 `&meta.user_agent` 置为 `""`
  （`crates/platform-iam/src/service.rs:981`）→ 用例 2 **仍然 `ok`**：

  ```
  test a_failed_sso_login_produces_exactly_one_sso_failure_event ... ok
  test a_successful_sso_login_produces_exactly_one_sso_success_event ... ok
  test result: ok. 2 passed; 0 failed; 0 ignored
  ```

  生产代码确实把 `&meta.user_agent` 传进去了（`:981`），**行为是对的**；
  缺的只是「失败分支 UA 被写错」时能抓红的判据。
- **建议改法**（1 行，与用例 1 对齐）：
  ```rust
  // sso_login_audit.rs:319 之后
  assert_eq!(event.user_agent.as_deref(), Some(USER_AGENT), "失败事件的 userAgent 也必须逐字透传");
  ```
- **不阻塞**：成功分支已钉住、实现无缺陷；这是判据密度问题。

### L-02 【低】config 侧用例名仍叫 `the_audit_page_default_size_is_the_same_everywhere`，与「只覆盖两处 + 常量」不完全一致

- **关联决策**：D182 / D186
- **可判定性分档**：**表述**（doc 已准确，用例名仍略夸大）
- **证据**：`crates/platform-config/tests/config_real_db.rs:1837`
  - doc 已如实写「本用例实际覆盖的**两处**（不是三处）」，并指向跨 crate 断言 ✅
  - 但函数名仍是 `…_is_the_same_everywhere` —— 读者看 `cargo test` 列表仍会以为它一次覆盖三处
- **建议改法**（可选）：改名为 `the_audit_page_default_size_registration_matches_db`。
  **不影响判据**，故仅记「低」。

### L-03 【低】SSO 登录审计仍**只在 sink 边界**断言，没有「真库落表」判据（TD-040 未关闭）

- **可判定性分档**：**不可判定**（`platform_audit_log` 里那 1 行没有机械断言）
- **说明**：F14 注入的是 `RecordingSink`（与 `tests/audit_events.rs` 同模式），断言到
  `AuditEvent` 为止；`PgAuditSink` → `platform_audit_log` 这一段未在本用例读表。
  装配层确认是**同一个 sink**（`main.rs:339-341` 与 `:511-516`），所以风险低。
- **已知项**：Dev 已列为 U-2、BA 已记 **TD-040**，本轮**不修**。
- **补充事实纠正（我核实）**：Dev 说「加 `platform-audit` dev-dep 会成环」→ **不成环**。
  `platform-iam` 依赖 `platform-core` + `platform-db`（无 `platform-audit`）；
  `platform-audit` 依赖 `platform-core` + `platform-db`（无 `platform-iam`）；
  `platform-contracts` 依赖 `platform-core` + `platform-contracts-macros` → **两条线无交点**。
  BA 的纠正成立。

### I-01（说明性）任务卡 D4 写「恰好这 15 个」，但实际列出并核对的是 **17 个**

- `git status --porcelain` 实际为 **13 M + 4 ?? = 17 个**，与任务卡**逐项列出的 17 个路径完全一致**，
  双向比对无越界、无遗漏。
- 结论：**交付无越界**；「15 个」是任务卡笔误（计数与清单不符），不算交付问题。
  建议 BA 修订任务卡模板避免下一轮再对不上号。

### I-02（说明性）任务卡要求读 `decisions.md` rev 5 的 **D194 / D195**，但该文件 rev 5 只有 **D194**

- `decisions.md` 的 rev 5 段落只有 D194（集合相等作废 → 包含关系）。
- **D195**（`page` 模式下继续返回 `nextCursor`）实际写在
  `BA/feature-meta/req014-fixround-brief.md` 附录 **A.3**，且是「**不改代码**」的决策。
- 我已读 A.3 并据此核对：`page` 模式返回 `nextCursor` 的行为与 D195 一致（E2E 第 3、4 条通过）。
- 结论：**无交付影响**，属文档口径问题，建议 BA 把 D195 补进 `decisions.md` 或修正引用。

---

## 3. 我实测的逐 crate 数字（自己跑、自己算）

> 环境：`XDG_RUNTIME_DIR=/run/user/$(id -u)`、
> `CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target`、
> `DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test`
>
> 口径（任务卡 §2，**自己跑、自己 awk 算**，不引用任何旧报告）：
> ```bash
> cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored \
>   | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n",p,f,i}'
> ```
> 11 个 crate **串行**，且全部在**变异还原之后**跑。

| crate | 我实测 passed | failed | ignored | rc | 上轮复核 | 差 | Dev 自述 | 是否一致 |
|---|---|---|---|---|---|---|---|---|
| platform-core | **153** | 0 | 0 | 0 | 153 | 0 | 153 | ✅ |
| platform-db | **46** | 0 | 0 | 0 | 46 | 0 | 46 | ✅ |
| platform-iam | **180** | 0 | 0 | 0 | 178 | **+2**（F14） | 180 | ✅ |
| platform-org | **93** | 0 | 0 | 0 | 93 | 0 | 93 | ✅ |
| platform-rbac | **83** | 0 | 0 | 0 | 83 | 0 | 83 | ✅ |
| platform-config | **89** | 0 | 0 | 0 | 89 | 0 | 89 | ✅ |
| platform-schema | **20** | 0 | 0 | 0 | 20 | 0 | 20 | ✅ |
| platform-server | **128** | 0 | 0 | 0 | 127 | **+1**（F12） | 128 | ✅ |
| platform-audit | **54** | 0 | 0 | 0 | 54 | 0 | 54 | ✅ |
| sandbox | **33** | 0 | 0 | 0 | 33 | 0 | 33 | ✅ |
| xtask | **51** | 0 | 0 | 0 | 51 | 0 | 51 | ✅ |
| **合计** | **930** | **0** | **0** | — | 927 | **+3** | 930 | ✅ |

**「+3」的构成已逐条核实，且无下降 ⇒ 没有用例被误删**：

- `platform-iam +2` = `tests/sso_login_audit.rs` 两条（日志里实跑 `... ok`）
- `platform-server +1` = `tests/audit_default_size_real_db.rs` 一条（日志里实跑 `... ok`）

**新增用例确实真跑（不是「0 命中 = 没覆盖」）**：

```
platform-iam.log:
  test a_failed_sso_login_produces_exactly_one_sso_failure_event ... ok
  test a_successful_sso_login_produces_exactly_one_sso_success_event ... ok
platform-server.log:
  test the_audit_page_default_size_is_identical_in_runtime_manifest_and_db ... ok
platform-config.log:
  test the_audit_page_default_size_is_the_same_everywhere ... ok
```

**全部 crate 的 `ignored` 均为 0**（`grep -h '^test result' *.log | grep -v '0 ignored'` → 空）。

### 3.1 前端 / 构建 / 静态检查

| 项 | 我实测 | 判据 | 判定 |
|---|---|---|---|
| `pnpm --filter @base/shell test` | **206 passed / 0 failed**（17 files） | ≥ 206 | ✅ |
| `pnpm --filter @base/i18n test` | **43 passed / 0 failed** | 43 | ✅ |
| `pnpm --filter @base/sdk test` | **38 passed / 0 failed**（2 files） | 38 | ✅ |
| `pnpm -r build` | 全通过（`dist/assets/index-CzJOhtSa.js` 128.54 kB） | 通过 | ✅ |
| `npx svelte-check --threshold error` | **0 errors and 0 warnings** | 0 error | ✅ |
| `cargo clippy --workspace --all-targets` | 仅 2 条基线存量（`platform-iam/src/sso/config.rs:153`、`sso/oidc.rs:234`） | 新增 0 | ✅ |
| `cargo fmt --all --check` | base **11** / feat **11**，`comm` 双向均空 | 新增 0 | ✅ |
| `cargo xtask capabilities --check` | 「能力索引校验通过：**14 个文件**与源码一致」 | 一致 | ✅ |
| `aw verify`（工区 `feature-REQ-014`） | 全项 `[OK]`，**结论: 0 FAIL / 0 WARN** | 0/0 | ✅ |

> fmt 方法：`code`（`develop` @ `0683115`）与 `feature-REQ-014` 各跑一次，
> 把 `Diff in /abs/path/X:LINE` 归一化为相对路径后 `comm -13` / `comm -23` 双向比对 → 两侧均空。

### 3.2 干净库 E2E（本轮最重要的产出）

**「干净」是怎么确认的**（不是「我删了几行」，而是**整库重建**）：

| 步 | 动作 | 实测 |
|---|---|---|
| [1/6] | `DROP SCHEMA public CASCADE` + `CREATE SCHEMA public` + 基线 DDL（`deploy/seed/00-init.sql`，去掉 `\connect`） | cascade 清掉 **48 张表/函数**（含 `_sqlx_migrations`、`platform_audit_log`） |
| [2/6] | `cargo run -p platform-server --bin base -- migrate up` | 应用 **16 个版本**（`20261118000100`…`20261120000400`） |
| [3/6] | `deploy/seed/02-app-role.sql` | 建 `base_app` + 表授权 + append-only REVOKE |
| [4/6] | `modules/sandbox/migrations/*.up.sql` | 4 张 sandbox 表 + 索引 |
| [5/6] | `base tenant-policies` | 3 个模块、**15 张表**的 RLS 策略 |
| [6/6] | `./scripts/seed-e2e.sh` | 4 个账号 + **11 条审计夹具** |

**行数证据（干净性）**：

| 时点 | `all_audit_rows` | `tenant-a` | `tenant-a` 的 `e2e_fixture` |
|---|---|---|---|
| E2E 前（重建 + 播种后） | **11** | **11** | **11** |
| E2E 后 | 33（`tenant-a` 32 + `tenant-b` 1） | 32 | 11（登录事件，属正常） |

→ **起点 11 < 默认 `limit` 20**，正是 H-01 描述的必红条件。

**全量 E2E（干净库 + 修复版）**：

```
Running 35 tests using 1 worker
  ✓   1 audit.spec.ts:127  TC-014-118 / 119：无权限账号 → 403 页，请求数 0
  ✓   2 audit.spec.ts:153  TC-014-111 / 112：列出记录（操作人/时间/动作/对象/来源 IP）
  ✓   3 audit.spec.ts:182  TC-014-113 / 114：翻页 → 带 cursor，且不重复
  ✓   4 audit.spec.ts:221  TC-014-113（回退）：「上一页」→ 回到第 1 页
  ✓   5 audit.spec.ts:250  TC-014-115 / 117：按操作人过滤
  ✓   6 audit.spec.ts:297  TC-014-116：按动作过滤
  …
  35 passed (47.9s)
```

- `npx playwright test --list e2e/` → **Total: 35 tests in 6 files** ⇒ `35 passed` 意味着 **0 skipped**
- 审计页 **6 条逐条实跑**，无一条 skip；5 处 `test.skip(auditApiReady(...))` 都是
  「被测系统未就绪」口径（D108），本次**均未触发**。

---

## 4. 变异 / 对照验证（**7 处**，全部自己注入、自己还原）

> 规程：`cp`（**不用 `cp -p`**，避免保留旧 mtime）备份 → `sha256sum` 基线 → 注入 →
> 跑测试确认红/绿 → `cp` 还原 + **`touch`** → `sha256sum -c` → `grep -c MUTATION` → `git status`。
> 全部 7 处还原后：**17 个文件 sha256 与基线逐字节相同、`git status` 与基线逐文件相同、MUTATION 残留 0**。

### 4.1 A.3 负向对照 —— 删掉 F11 修复行，**同一个干净库**上必须红

| 步 | 证据 |
|---|---|
| 起点 | 整库重建后 `tenant-a before = 11` |
| 注入 | `sed -i '235d' e2e/audit.spec.ts`（只删第 235 行 `selectOption('10')`；第 192 行**保留**） |
| **结果** | **`1 failed / 5 passed (50.8s)`** —— 失败者恰是「回退」用例 |
| 失败原文（逐字） | `locator resolved to <button disabled … data-testid="audit-next-page">下一页</button>` → `element is not enabled` → 等待 30s 超时（`e2e/audit.spec.ts:238`） |
| 还原 | `cp` + `touch` → `sha256 = edc03d6c…198ba86`（**与基线相同**） |
| 复验 | `git status` 与基线 `diff` **空**；`grep -c MUTATION` = **0** |

→ 证明「全绿」是**这一行修复**带来的，不是干净环境恰好 favorable。

### 4.2 B1 —— `platform_audit::DEFAULT_LIMIT` 20 → 30

| 步 | 证据 |
|---|---|
| 备份 sha256 | `fd4dc6122c93ef43…89c83  crates/platform-audit/src/query.rs` |
| 注入 | `query.rs:51` → `= 30;` |
| **变红** | `the_audit_page_default_size_is_identical_in_runtime_manifest_and_db … FAILED`（`audit_default_size_real_db.rs:123`）<br>`assertion left == right failed: manifest 登记的 audit.page.defaultSize（20）与**运行时**默认 platform_audit::DEFAULT_LIMIT（30）不一致`<br>`left: Number(20) / right: Number(30)` |
| 还原 | `cp` + `touch` → sha256 **逐字节相同**；MUTATION 0 |
| 复跑 | `1 passed`（绿）✅ |

### 4.3 B2 —— manifest 侧比对（按任务卡 + 加强版各一次）

| 步 | 证据 |
|---|---|
| **B2（卡面做法）** | `config_real_db.rs:1849` 的 `json!(20)` → `json!(21)` → **FAILED**（`left: Number(20) / right: Number(21)`，`:1847`） |
| 还原 | `cp` + `touch` → sha256 = `231881fa…a418`（与基线相同） |
| **B2'（加强，更真实）** | `manifest.json:212` 的 `"default": 20` → `21` → **FAILED**（`left: Number(21) / right: Number(20)`）→ 证明断言真的**读 manifest**，不是只比自己的字面量 |
| 还原 | `cp` + `touch` → sha256 = `8db0f0fb…32588`（与基线相同） |

### 4.4 B3 —— `platform_config` 表的行值 20 → 21

| 步 | 证据 |
|---|---|
| 注入 | `UPDATE platform_config SET value='21'::jsonb WHERE key='audit.page.defaultSize'` |
| **变红** | `…is_the_same_everywhere … FAILED`（`config_real_db.rs:1875`）<br>`DB 里的 audit.page.defaultSize 必须是 20（迁移种子 20261120000400）`<br>`left: Some(Number(21)) / right: Some(Number(20))` |
| 还原 | `UPDATE … value='20'::jsonb` → 复跑 config 侧 `1 passed`、server 侧 `1 passed` ✅ |

### 4.5 C3-1 —— 成功分支 `ip` 改成 `"0.0.0.0"`

| 步 | 证据 |
|---|---|
| 备份 sha256 | `8ea4e3ff18f369d2…69c28  crates/platform-iam/src/service.rs` |
| 注入 | `service.rs:978` 的 `&meta.ip` → `"0.0.0.0"` |
| **变红** | `a_successful_sso_login_produces_exactly_one_sso_success_event … FAILED`（`sso_login_audit.rs:258`）<br>`assertion left == right failed: ip 必须**逐字**等于 x-forwarded-for 的第一跳`<br>`left: Some("0.0.0.0") / right: Some("203.0.113.9")` |
| 旁证 | 同一次跑里失败用例仍 `ok` → 变异只命中成功分支，定位精确 |

### 4.6 C3-2 —— 失败分支 sso 标志位 `true` → `false`

| 步 | 证据 |
|---|---|
| 注入 | `service.rs:981` → `login_event(&tenant, None, false, **false**, &meta.ip, &meta.user_agent)` |
| **变红** | `a_failed_sso_login_produces_exactly_one_sso_failure_event … FAILED`（`sso_login_audit.rs:297`）<br>`一次 SSO 登录失败 = **恰好一条** iam.sso.login.failed；实际记录的事件码：["iam.login.failed"]`<br>`left: 0 / right: 1` |
| 还原 | `cp` + `touch` → sha256 = `8ea4e3ff…69c28`（**逐字节相同**）；复跑 `2 passed` ✅ |

### 4.7 C3-3（反向）—— 失败分支 `user_agent` → `""`

| 步 | 证据 |
|---|---|
| 注入 | `service.rs:981` 的 `&meta.user_agent` → `""` |
| **结果** | **`2 passed`（仍然绿）** → 失败分支的 UA 没有判据（= **L-01**） |
| 还原 | `cp` + `touch` → sha256 与基线相同；复跑 `2 passed` ✅ |

### 4.8 还原后的总校验

```bash
$ git status --porcelain=v1 | diff /tmp/qa2/git-status-baseline.txt -   → 无差异
$ for f in $(git status --porcelain | awk '{print $2}'); do sha256sum "$f"; done \
    | diff /tmp/qa2/sha256-baseline.txt -                                → 无差异（17/17 逐字节相同）
$ grep -rn "MUTATION" crates/ packages/ migrations/ e2e/ | wc -l         → 0
```

→ 变异注入**未改变**被测代码的最终状态；受影响 crate 复跑后数字与 §3 全量**逐项一致**。

---

## 5. 未验证项清单（如实列出，**没有**「应该没问题」这类表述）

| # | 未验证项 | 原因 | 影响 |
|---|---|---|---|
| **V-01** | SSO 登录事件的**真库落表**（`platform_audit_log` 里的那 1 行） | F14 注入的是 mock sink，断言到 `AuditEvent`；读表需要 `platform-audit` 的 `PgAuditSink` 进 `platform-iam` 测试 | = **L-03 / TD-040**，本轮不修 |
| **V-02** | D157「审计写失败 → 拒绝登录 503」的**故障注入**复跑 | 未做 sink 故障注入（锁表 / 撤权限 / 断连接）；本轮范围是 F11~F14 | 代码路径存在（`record_audit(...)?`），**本轮未跑通**；Dev 称已由 `audit_events.rs:380-420` 覆盖，我**未独立复核该断言** |
| **V-03** | `config.flag.write` 的事件码与 `before`/`after` 形状 | 不在 F11~F14 范围 | 沿用上轮 U-05，**仍未验** |
| **V-04** | 5 个 `AUDIT_*` 中文文案的语义校对 | 只验键存在 + 非空 | 沿用上轮 U-07，**仍未验** |
| **V-05** | `platform_audit_chain_state` 的跨租户 RLS | 未构造租户上下文场景 | 沿用上轮 U-09，**仍未验** |
| **V-06** | 生产/既有环境 `audit.page.defaultSize` 行 | 我只有 `base_test` 访问权 | 迁移属本功能分支、无环境应用过 → 新部署即 `'20'`；**本条不阻塞** |
| **V-07** | 非 chromium 浏览器 | `playwright.config.ts` 只配 chromium | 与上轮口径一致，**未覆盖** |
| **V-08** | `scripts/test-integration.sh` 整链（`cargo test --workspace`） | 任务卡明确该命令在本仓**必失败**，只能逐 crate 跑 | 我用 **11 个 crate 串行**替代，未跑该脚本本身 |

---

## 6. 对第 2 轮 Dev 4 条偏离声明（D-1~D-4）的逐条判定

| # | Dev 的声明 | 我的判定 | 依据 |
|---|---|---|---|
| **D-1** | **F14 的落点从 `PlatformSinkSsoRecorder` 改为 `IamService::complete_oidc_callback`** —— 任务卡与源码冲突 | **接受（BA 已独立复核属实）** | 我逐行核实：`bridge.rs:12-22` 写明 D157 要「写失败即 503」而 `record` 是同步的；`:53` / `:96` 的 `is_written_by_the_service` 跳过两个登录码（否则一次登录两条 → 破 AC-S-127 的「1 条」）；生产写入在 `service.rs:944-990`（`complete_oidc_callback` → `record_audit`）；路由确实调到它（`http.rs:507`）。**任务卡的字面前提是错的，Dev 按事实驱动是对的。** |
| **D-2** | F14 放在**新文件** `tests/sso_login_audit.rs`，不追加进 `tests/audit_events.rs` | **接受** | 任务卡 §F14 明写「或同 crate 的新测试文件」；新文件用 `mod sso_support;` 避免与 `audit_events.rs` 的夹具函数名冲突。`git status` 显示 `audit_events.rs` **未被改**（`platform-iam/tests/` 下只有新文件）。 |
| **D-3** | 干净库重建比 `e2e-server.sh` **多做 3 步**（`02-app-role.sql` / sandbox 迁移 / `tenant-policies`） | **接受（并已独立复现必要性）** | 我读 `scripts/e2e-server.sh` 确认它**只**做 `migrate up` + `seed-e2e.sh` + `serve`；我自己重建时**先补 3 步**才起得来（不补第 1 步 → `base serve` 会 `permission denied for table platform_permission`）。已记 **TD-039**，本轮不修。 |
| **D-4** | F12 附带把**行内断言消息**「与契约 C7 的运行时默认必须相等」改成「口径（20）」 | **接受** | 原句本身就是 M-01 指出的错误表述（代码比的是常量，不是运行时）；只改 doc 不改它会留下同一处不一致。现消息（`config_real_db.rs:1866`）与代码一致。 |

**另：Dev 的 U-2 中「加 `platform-audit` dev-dep 会成环」这一说法 —— 拒绝（BA 的纠正成立）。**
依赖图实测：`platform-iam` → `{platform-core, platform-db}`；`platform-audit` → `{platform-core, platform-db}`；
`platform-contracts` → `{platform-core, platform-contracts-macros}` → **两条线无交点，不成环**。
所以 V-01 的阻塞理由**不是**「会成环」，而是「本轮未做」——建议按 TD-040 另立。

---

## 7. 本次复核的写入与还原声明

- **唯一写入的文件**：本报告 `/home/myg133/Projects/system_seed/BA/feature-meta/qa-recheck2-req014.md`
- **未修改** feature worktree 的任何生产代码 / 测试代码 / 迁移 / 配置
  （§4 的 7 处变异/对照全部按规程还原：**17 个文件 sha256 逐字节相同**、
  `git status --short` 与基线**逐文件相同**、`grep -c MUTATION` = **0**）
- **未 `git commit`**；未改 `BA/demands/`、`BA/plan/`
- **数据库侧**：我多次 `DROP SCHEMA public CASCADE` 重建 `base_test`（测试库是可弃资源）；
  最后一次重建后已恢复为干净起点（`all=11 / tenant-a=11 / fixture=11`），
  `platform_config.audit.page.defaultSize = 20`；**未触碰 `base_dev`**，
  **未重启/未新建容器**（`base-postgres` / `base-redis` 始终在线）

---

*每条结论都附可复现命令输出或 `file:line`；所有数字由复核者自己重跑、自己 awk 算；
F11 的红→绿由负向对照闭环，F12 的四路变异、F14 的两处变异与一处反向变异均由我独立注入；
Dev 的自述数字未被采信。*
