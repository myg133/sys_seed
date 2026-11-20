# REQ-014 审计日志 —— 修复轮**独立复核**报告

- **被验对象**：`/home/myg133/Projects/system_seed/feature-REQ-014`，分支 `feature/REQ-014`
  - HEAD 仍为 `5f28c0c`（**修复轮未提交**）；全部改动在**工作区**，共 **12 改 + 2 新增**
- **验收基线**：`BA/demands/REQ-014-audit/acceptance.md` **rev 3**（13 条 AC）
- **决策基线**：`BA/demands/REQ-014-audit/decisions.md` **rev 5**（D130~D195）
- **上一轮复验**：`BA/feature-meta/qa-premereq-req014.md`（FAIL：阻塞 2 / 高 1 / 中 3 / 低 4）
- **修复轮报告**：`req-014-dev-fixround.md`（主）+ `req-014-dev-fixround-f10.md`（F10 补）
- **复核人**：QA 复核子 Agent（**不采信 Dev 自验**；所有数字自己重跑、自己用 awk 算；
  阻塞项全部自己注入变异验证）
- **复核日期**：2026-11-20
- **复核前后** `git status --short` 均与 Dev 交付状态**逐文件相同**（无残留、无 MUTATION）

---

## 1. 结论

# **FAIL**

**0 条阻塞**；**1 条高**；**2 条中**；**3 条低**。

两条原阻塞（B-01 `nextCursor` 恒 null、B-02 过滤查询 503）**已确认修好**，
且**新补的断言经我独立注入变异验证全部能抓红** —— 这是本轮最重要的正面结论。

判 FAIL 的原因是**一条 E2E 用例在干净库上必然红**（Dev 报「35 passed / 0 failed」
不可复现），且它是一条**隐藏依赖脏数据的用例** —— 正是本轮要防的「假绿」同族问题
（只在数据被历史运行「喂胖」时绿，干净环境必红）。

### 分档汇总

| 档位 | 条数 | 编号 |
|---|---|---|
| **阻塞** | **0** | — |
| **高** | **1** | H-01 |
| **中** | **2** | M-01、M-02 |
| **低** | **3** | L-01、L-02、L-03 |
| 合计 | **6** | |

### A / B / C / D / E 五组判定一览

| 组 | 项 | 判定 |
|---|---|---|
| **A1** | `nextCursor` 不再恒 null（黑盒） | ✅ **通过**（10 页 19 条零重叠，末页 null） |
| **A2** | 过滤查询不再 503（黑盒，5 个 op） | ✅ **通过**（全部 200，行数正确） |
| **B1** | `limit + 1` 改回 `limit` → 断言必须红 | ✅ **4 条红**（库级 3 + HTTP 1） |
| **B2** | 去掉 ` AND ` 前缀 → 断言必须红 | ✅ **7 条红**（库级 6 + HTTP 1） |
| **B3** | 删 en-US 一条 `error.*` → 必须红 | ✅ **2 条红** |
| **B4** | 只删 zh-CN 一条 → 必须红 | ✅ **2 条红**（英文完好仍红 ⇒ 中文确实被查） |
| **B5** | org 桥接不进 sink → F5 必须红 | ✅ **1 条红**（`left:0 / right:1`） |
| **C1** | `defaultSize` 三处一致 + label 删补丁 | ⚠️ **值全对；但「三处」实为「两处 + 硬编码常量」**（M-01） |
| **C2** | 夹具只删自己的 6 个键 | ✅ **通过**（跑完 audit.* 4 键仍在，共 10 行） |
| **C3** | org / config 走生产适配器真落表 | ✅ **通过**（变异 B5 证明确实走到 sink） |
| **C4** | `query.rs` 注释改 D183 口径 | ✅ **通过**（全仓 `problem+json` 0 命中） |
| **C5** | `audit_chain.rs:1` U+FFFD | ✅ **通过**（计数 0） |
| **C6** | `TokenVerifyError` 带类型；其它 trait 未波及；无新依赖 | ✅ **通过** |
| **C7** | 迁移注释改 D181 口径 | ✅ **通过** |
| **C8** | i18n 断言是包含关系 + 两 locale + 来源标签 + 文件缺失 panic | ✅ **通过**（`TITLES` 84 行一行未改） |
| **D1** | 改动范围 == D1 清单 | ✅ **完全一致**（零越界、零遗漏） |
| **D2** | 逐 crate 全量 11 个 | ✅ **927 / 0 failed / 0 ignored**（自己算；> 基线 912） |
| **D3** | 前端 3 包 + build + svelte-check | ✅ **206 / 43 / 38 / 通过 / 0 error** |
| **D4** | E2E 全绿 | ❌ **34 passed / 1 failed**（Dev 报 35/0 不可复现）→ **H-01** |
| **D5** | clippy 新增 0 / fmt 新增 0 | ✅ **2 条存量 / 36 处存量，新增 0** |
| **D6** | `xtask capabilities --check` / `aw verify` | ✅ **14 文件一致 / 0 FAIL 0 WARN** |
| **E-U03** | F5 是否真覆盖 org/config | ✅ org + config 已覆盖；⚠️ SSO 登录审计仍无真库用例（L-02） |
| **E-U04** | 全租户 `xtask audit verify` | ✅ **已补验**（3 租户 23 条，0 断裂） |
| **E-U06** | `deploy/seed/` 其余脚本 | ✅ **已补验**（`00-init`/`01-seed` 重跑幂等，append-only 未破） |

---

## 2. 逐条问题

> 可判定性分档按 dev-guide §9.12：主体不存在 / 物理不可达 / 不可判定 / 口径不一致 / 表述。

---

### H-01 【高】E2E「上一页」用例在**干净库**上必红 —— 隐藏依赖「tenant-a 记录数 > 20」

- **关联 AC**：AC-S-132 步骤 2（翻页不重复 / 分页可用）
- **关联检查**：任务卡 §6 **D4**（E2E 必须全绿，Dev 报 35/0）；D193（skip 不得掩盖）
- **可判定性分档**：**不可判定**（同一用例在两种数据状态下红绿相反 → 结果不反映被测行为）
  + **口径不一致**（Dev 报告的结果与干净环境实测不一致）
- **根因**：`e2e/audit.spec.ts:221-240` 的「上一页」用例
  **没有调小页大小**，直接点「下一页」：

  ```ts
  // e2e/audit.spec.ts:221
  await login(page, AUDITOR);
  await page.goto('/admin/audit');
  const targetCalls = trackAuditRequests(page);
  await page.getByTestId('audit-next-page').click();   // ← 默认 limit=20
  ```

  而 `scripts/seed-e2e.sh` 只播 **11 条**审计夹具（`FOR v_index IN 1..11`）。
  干净库上 tenant-a = **11 条 < 默认 limit 20** → `nextCursor` 为 `null`
  → 「下一页」按钮 `disabled`（`apps/shell/src/pages/Audit.svelte:340`）→ 点击超时。

  **对照**：同一文件第 182 行的翻页用例**显式**把页大小调到 10，所以它不受影响：

  ```ts
  // e2e/audit.spec.ts:193-194（第 3 条用例）
  await page.getByLabel('每页条数').selectOption('10');
  ```

  该文件头注释甚至写明了这个前提（`:38-40`）：
  > 「默认页大小是 20；夹具里的审计记录通常不到 20 条 → 把页大小调到 10 才可能有两页。」

  → **第 221 行这条用例漏做了同一件事**。

- **证据 1 —— 干净库（重建 + 只跑 seed）必红**：

  ```bash
  # 清空 tenant-a 审计记录后重播 seed（幂等）→ 恰好 11 条
  $ psql ... -c "delete from platform_audit_log where tenant_id='tenant-a';"
  $ DATABASE_URL_TEST=... ./scripts/seed-e2e.sh
  ✓ 审计夹具：11 条（object_type = e2e_fixture）

  $ npx playwright test e2e/audit.spec.ts --reporter=list
  ✓ 1 TC-014-118 / 119  无权限 → 403 页，请求数 0
  ✓ 2 TC-014-111 / 112  列出记录
  ✓ 3 TC-014-113 / 114  翻页 → 带 cursor，且不重复
  ✘ 4 TC-014-113（回退）：「上一页」→ 回到第 1 页        ← 30.1s 超时
  ✓ 5 TC-014-115 / 117  按操作人过滤
  ✓ 6 TC-014-116        按动作过滤
  1 failed, 5 passed
  ```

  失败详情（逐字）：
  ```
  Error: locator.click: Test timeout of 30000ms exceeded.
  Call log:
    - waiting for getByTestId('audit-next-page')
    - locator resolved to <button disabled ... data-testid="audit-next-page">下一页</button>
    - element is not enabled
  ```

- **证据 2 —— 同一用例在「脏库」上绿**：库被前面 E2E 的登录事件
  （`iam_session` 累积到 27 条）喂到 tenant-a = **43 条 > 20** 时，
  单独跑该文件 **6 passed / 0 failed**。
  → 红绿由**历史运行留下的数据**决定，不由被测代码决定。

- **证据 3 —— 全量套件（脏库起点）34 passed / 1 failed**，同样只有这一条红。

- **为什么这不是「产品缺陷」**：`total(11) < limit(20)` 时 `nextCursor == null`
  **是正确行为**（契约 C7；A1 黑盒已证 `limit=200 > total=19` 时 `nextCursor=null`，
  且 10 页翻完 19 条零重复）。缺陷在**用例缺少前置**，不在实现。

- **为什么仍判「高」而不是「低」**：
  1. 出口标准要求 **E2E 全绿**，而干净环境下是红的 → **D4 判据未满足**；
  2. Dev 报「**35 passed / 0 failed / 0 skipped**」，**我无法复现**；
  3. 这类「只在脏库上绿」的用例是**下一次回归的定时炸弹**：
     任何重建测试库的 CI（`scripts/test-integration.sh` 会 `DROP SCHEMA public CASCADE`）
     都会让它红，而本地反复跑的人永远看不到。

- **建议改法**（一行，与第 3 条用例对齐）：
  ```ts
  // e2e/audit.spec.ts:228 之后补一行
  await page.getByLabel('每页条数').selectOption('10');
  ```
  或把「翻页 / 回退」两条合并成一条、共用同一次「调页大小」的步骤。
  ⚠️ **不建议**改夹具到 >20 条来「迁就」用例 —— 那会让用例继续依赖「总量够多」
  这个与判据无关的前提。

---

### M-01 【中】F3 的一致性断言自称「三处」，实际是「两处 + 一个硬编码常量」——对**运行时默认漂移**是盲的

- **关联决策**：D182（登记值必须与行为一致）、D186（F3）
- **关联检查**：任务卡 §5 **C1**
- **可判定性分档**：**口径不一致**（注释/断言文字说「三处一致」，覆盖范围只有两处 + 常量）
  + **不可判定**（「登记值 == 运行时默认」这条真实不变量**没有机械断言**）
- **实测值本身全对**（这部分 ✅）：

  | 来源 | 值 | 证据 |
  |---|---|---|
  | `crates/platform-config/manifest.json` | **20** | `default: 20`，label 已删补丁说明 → `"审计列表默认每页条数"` |
  | `migrations/20261120000400_audit_config_seed.up.sql:48` | **`'20'::jsonb`** | 文件 grep |
  | `platform_config` 表实际行 | **`audit.page.defaultSize \| 20`** | `psql -tAc "select key,value …"` |

- **缺陷在断言的覆盖范围**：`crates/platform-config/tests/config_real_db.rs:1846`

  ```rust
  // 2) 契约 C7 的默认口径（20）。
  // ⚠️ 不在这里引用 `platform_audit::DEFAULT_LIMIT` —— `platform-config`
  // **不依赖** `platform-audit`（刻意的分层），本轮也不许加依赖。
  const CONTRACT_DEFAULT_LIMIT: i64 = 20;                 // ← 硬编码常量
  assert_eq!(registered.default, json!(CONTRACT_DEFAULT_LIMIT), "…登记在骗人…");
  ```

  于是「登记值 == 运行时默认」**退化成了**「登记值 == 我写死的 20」。
  用例名 `the_audit_page_default_size_is_the_same_everywhere` 与 doc 里的
  「**三处**来源」（`config_real_db.rs:1808-1817` 明确列了
  「2. `platform_audit::DEFAULT_LIMIT`（运行时默认）」）**与实际断言不符**。

- **证据（决定性实验）**：把运行时的 `DEFAULT_LIMIT` 从 20 改成 **30**（模拟「行为漂了」）：

  ```
  $ sed -i 's/pub const DEFAULT_LIMIT: usize = 20;/pub const DEFAULT_LIMIT: usize = 30;/' \
      crates/platform-audit/src/query.rs

  $ cargo test -p platform-audit --lib the_defaults_are_the_contract_defaults
  test result: FAILED. 0 passed; 1 failed          ← platform-audit 自己的单测抓到 ✅

  $ cargo test -p platform-config --test config_real_db the_audit_page_default_size
  test the_audit_page_default_size_is_the_same_everywhere ... ok
  test result: ok. 1 passed; 0 failed              ← config 侧断言**仍然是绿的** ❌
  ```

  → **H-01 那一类「登记 50 / 行为 20」的原始缺陷，如果以「行为漂移」的形式复现，
  这条新断言抓不到**。它只能抓「manifest 或 DB 被改成非 20」。
  （已还原：`sha256sum -c` 通过，`git status` 干净。）

- **定性**：这**不是**放宽了既有断言（原本就没有这条），所以不算违规；
  但**doc / 用例名宣称的覆盖面大于实际覆盖面**，且 Dev 的 D-3 声明把它说成
  「运行时默认仍由 `platform-audit` 自己的单测钉住」——
  那句话本身是**真的**（我上面第一段实验就是它红的），
  但**读者会以为「三处都在一条用例里比对」**，而实际分散在两处、
  config 侧那条对运行时是盲的。

- **建议改法**（二选一，都很小）：
  1. **改文档不改代码**：把用例名改成 `the_audit_page_default_size_registration_matches_db`，
     doc 的「三处来源」删掉第 2 项，并在注释里写明
     「运行时默认由 `platform-audit::query::tests::the_defaults_are_the_contract_defaults` 钉住
     —— 本用例**不**覆盖它（`platform-config` 不依赖 `platform-audit`）」；
  2. **真做三处**：在 `platform-server/tests/`（它同时依赖 config 与 audit）新增一条
     把 `manifest.default`、`platform_audit::DEFAULT_LIMIT`、DB 行三者直接比对 ——
     这才是 D182「登记在骗人」的**完整**判据。

---

### M-02 【中】Dev 两份报告对 F10 给出**互相矛盾**的结论 —— 交付声明不可信

- **关联检查**：任务卡 §8 第 6 项（对 Dev 偏离声明的逐条判定）
- **可判定性分档**：**表述** / **口径不一致**（同一事实两个说法）
- **证据**：

  | 文件 | 写入时间 | 对 F10 的结论 |
  |---|---|---|
  | `req-014-dev-fixround.md` | 12:45:56 | §0 表「**未落地**」；§12「现状：`i18n_titles.rs` **保持原样（未修改）**」 |
  | `req-014-dev-fixround-f10.md` | 12:56:57（晚 11 分钟） | 「**只改了一个文件**：`crates/platform-server/tests/i18n_titles.rs`（**+214 行**）」 |

- **工作区事实**（以文件为准）：`git diff --stat` → `i18n_titles.rs | 214 +++++++++++++`，
  且我实测两条新断言存在、能被变异抓红（B3/B4）。
  → **小报告是对的，主报告 §12 的「未修改」已过时且为假**。

- **为什么算问题而不是「无伤大雅的时序」**：
  主报告是**主交付文档**，BA 读它会把 F10 判成「未落地、需裁决」，
  而实际上**已经落地**。上轮复验已经证伪过 Dev 的数字表（报 143/99，实测 150/168），
  本轮**主报告**再次出现与工作区不一致的核心结论 → 报告的可信度需要单独打折。
  主报告也**没有**「本报告已被 f10 补正」的交叉引用。

- **建议改法**：主报告 §0/§12/§13 更新为「F10 已按 D194 落地，见
  `req-014-dev-fixround-f10.md`」，并给主报告的 925 总数加一行
  「含 F10 的 +2 后为 927」（见 L-01）。

---

### L-01 【低】主报告逐 crate 表 `platform-server = 125`、总 **925**，与实测 **127 / 927** 不一致

- **可判定性分档**：**口径不一致**（报告内自洽，但漏了 F10 新增的 2 条）
- **证据**：主报告 §8.1 表合计 925（我按其表逐项相加校验：153+46+178+93+83+89+20+125+54+33+51 = 925 ✅ 表内自洽），
  但 f10 报告 §2 实测 `platform-server = 127`（125 + 2 条新断言），
  我实测也是 **127** → 真实合计 **927**。
- **影响**：不影响「总数上升、无用例被删」这个结论（927 > 912）；只是主报告的 925 是
  F10 落地**之前**的快照，未更新。BA 若拿 925 去和 CI 对账会差 2。
- **建议改法**：主报告 §8.1 的 `platform-server` 改 127、合计改 927，并注明「含 F10 的 +2」。

---

### L-02 【低】SSO 登录审计（`iam.sso.login.succeeded`）仍**无任何真库用例**覆盖

- **关联 AC**：AC-S-127 步骤 3（走一次 SSO 登录 → 1 条 `iam.sso.login.succeeded`）
- **关联检查**：任务卡 §7 **E 组 U-03**
- **可判定性分档**：**不可判定**（AC 的判据在仓库里没有机械断言）
- **证据**：
  ```bash
  $ grep -rl "iam.sso.login.succeeded" crates/*/tests/*.rs
  （0 命中）
  ```
  对照：`org.node.create` → `platform-server/tests/audit_chain_real_db.rs` ✅；
  `config.entries.write` → 同文件 + `platform-config/tests/audit_events.rs` ✅；
  `iam.login.succeeded` → `platform-iam/tests/audit_events.rs`（mock sink，非真库）⚠️。
- **说明**：本轮 F5（D188）**只要求** org 与 config 两条链路，SSO 不在其范围内 →
  **不构成本轮修复的失职**，但它仍是 AC-S-127 步骤 3 的判据缺口（上轮 U-01 未关闭）。
- **建议改法**：与 U-01（需要真实 OIDC/LDAP provider）一起另立一轮，或至少在
  测试卡里显式标注「AC-S-127 步骤 3 无可执行判据」。

---

### L-03 【低】上一轮 U-01 / U-02 / U-05 / U-07 / U-09 仍未验（本轮未涉及，如实列出）

- **可判定性分档**：**不可判定**（未执行）
- **明细**（沿用 `qa-premereq-req014.md` §5 的编号）：

  | # | 未验证项 | 本轮状态 |
  |---|---|---|
  | U-01 | AC-S-127 步骤 3：SSO 登录审计（ip/UA 逐字） | **仍未验**（无 provider）→ 见 L-02 |
  | U-02 | D157：审计写失败 → 拒绝登录 **503** | **仍未验**（未构造 sink 故障注入） |
  | U-05 | `config.flag.write` 的事件码与 before/after 形状 | **仍未验**（F5 只测了 `entries.write`） |
  | U-07 | 5 个 `AUDIT_*` 中文文案的人工校对 | **仍未验**（只验了键存在 + 非空） |
  | U-09 | `platform_audit_chain_state` 的跨租户 RLS | **仍未验** |
  | ~~U-03~~ | org / config 落真表 | ✅ **本轮已补**（F5 + 我的 B5 变异） |
  | ~~U-04~~ | 全租户 `xtask audit verify` | ✅ **本轮已补验**（见 §5） |
  | ~~U-06~~ | `deploy/seed/` 其余脚本 | ✅ **本轮已补验**（见 §5） |

- **建议**：U-02（D157 的 503）与 U-05 属于**安全/契约关键路径**，
  建议下一轮优先补；U-07 / U-09 可降级为人工核对项。

---

## 3. 我实测的逐 crate 数字（自己跑、自己算）

> 环境：`XDG_RUNTIME_DIR=/run/user/$(id -u)`、
> `CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target`、
> `DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test`
>
> 口径（任务卡 §2）：
> ```bash
> cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored \
>   | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n",p,f,i}'
> ```
> **11 个 crate 串行**，全部在**变异还原后**跑。

| crate | passed | failed | ignored | rc | 上轮基线 | 差 |
|---|---|---|---|---|---|---|
| platform-core | **153** | 0 | 0 | 0 | 153 | 0 |
| platform-db | **46** | 0 | 0 | 0 | 46 | 0 |
| platform-iam | **178** | 0 | 0 | 0 | 178 | 0 |
| platform-org | **93** | 0 | 0 | 0 | 93 | 0 |
| platform-rbac | **83** | 0 | 0 | 0 | 83 | 0 |
| platform-config | **89** | 0 | 0 | 0 | 88 | **+1** |
| platform-schema | **20** | 0 | 0 | 0 | 20 | 0 |
| platform-server | **127** | 0 | 0 | 0 | 121 | **+6** |
| platform-audit | **54** | 0 | 0 | 0 | 46 | **+8** |
| sandbox | **33** | 0 | 0 | 0 | 33 | 0 |
| xtask | **51** | 0 | 0 | 0 | 51 | 0 |
| **合计** | **927** | **0** | **0** | — | 912 | **+15** |

**逐新增用例确认真跑**（不是「0 命中 = 没覆盖」）：

```
platform-audit/tests/query_real_db.rs（+8，全 ok）
  has_more_pages_iff_the_total_exceeds_the_limit
  two_consecutive_pages_share_no_ids
  the_total_used_by_the_cursor_check_is_the_filtered_total
  the_eq_filter_returns_only_the_matching_rows
  the_ne_filter_excludes_the_matching_rows
  the_in_filter_matches_any_of_the_values
  the_contains_filter_matches_substrings
  the_range_filter_matches_the_timestamp_window

platform-server/tests/audit_chain_real_db.rs（+4，全 ok）
  an_org_write_really_lands_in_the_audit_table
  a_config_write_really_lands_in_the_audit_table
  a_filtered_query_over_http_is_not_a_server_error
  the_http_page_carries_a_usable_next_cursor

platform-server/tests/i18n_titles.rs（+2，全 ok）
  every_error_code_is_registered_in_every_locale
  titles_error_codes_are_registered_in_every_locale

platform-config/tests/config_real_db.rs（+1，ok）
  the_audit_page_default_size_is_the_same_everywhere
```

**与 Dev 数字的差异（说明来源，作为 L-01 的依据）**：

| 来源 | platform-server | 合计 |
|---|---|---|
| Dev 主报告 §8.1 | 125 | 925 |
| Dev f10 小报告 §2 | **127** | （127 单列） |
| **我实测** | **127** | **927** |

主报告的 925 是 F10 落地前的快照 → 差 2（= F10 的两条新断言）。
**总数上升**（927 > 912）→ 无用例被误删 ✅。

### 前端 / E2E / 出口标准

| 项 | 我实测 | 判据 | 判定 |
|---|---|---|---|
| `pnpm --filter @base/shell test` | **206 passed / 0 failed**（17 files） | ≥ 206 | ✅ |
| `pnpm --filter @base/i18n test` | **43 passed / 0 failed** | 43 | ✅ |
| `pnpm --filter @base/sdk test` | **38 passed / 0 failed** | 38 | ✅ |
| `pnpm -r build` | 全通过（`dist/index-CzJOhtSa.js` 128.54 kB） | 通过 | ✅ |
| `npx svelte-check --threshold error` | **0 errors and 0 warnings** | 0 error | ✅ |
| `npx playwright test e2e/` | **34 passed / 1 failed / 0 skipped** | 全绿 | ❌ **H-01** |
| `cargo clippy --workspace --all-targets` | 只剩基线 **2 条**存量<br>（`platform-iam/src/sso/config.rs:153`、`oidc.rs:234`） | 新增 0 | ✅ |
| `cargo fmt --all --check` | base **36** / feat **36**，文件与行号**逐项相同**，新增/消失均为**空** | 新增 0 | ✅ |
| `cargo xtask capabilities --check` | `能力索引校验通过：14 个文件与源码一致` | 一致 | ✅ |
| `aw.py verify`（经 aw MCP） | `结论: 0 FAIL / 0 WARN` | 0/0 | ✅ |

> fmt 比对方法：基线 worktree `code@0683115` 与 feature 各跑一次，
> 把 `Diff in /abs/path/X:LINE` 归一化为相对路径后 `comm -13` / `comm -23` 双向比对 → 两侧均空。
> **13 个改动文件均不在** fmt 失败列表里。

**E2E 那条红的归属（H-01）**：唯一失败是
`e2e/audit.spec.ts:221 › TC-014-113（回退）：「上一页」→ 请求回到第 1 页（不带 cursor）`，
根因是用例未调页大小（默认 20）而夹具只有 11 条 → 按钮 disabled。
**审计页 6 条用例中 5 条真跑通过、1 条因用例缺陷必红**；**0 skipped**（D108/D193 合规）。

---

## 4. 变异验证（**5 处**，全部自己注入、自己还原）

> 规程（任务卡 §4）：`cp`（**不用 `cp -p`**）备份 → `sha256sum` 基线 → 注入 →
> 跑测试确认**红** → `cp` 还原 + **`touch`** → `sha256sum -c` → `grep -c MUTATION` → `git status`。
> 5 处全部还原后，**11 个 crate 重跑一遍** → 927 passed（与 §3 逐项一致）。

### 4.1 B1 —— `limit + 1` 改回 `limit`（F1 的原缺陷）

| 步 | 证据 |
|---|---|
| 备份 sha256 | `fd4dc6122c93ef43201432e081dfbfc532db81c63977613e3eec281625d89c83  crates/platform-audit/src/query.rs` |
| 注入 | `query.rs:423` → `binds.push(BindValue::Int(limit as i64)); // MUTATION-B1` |
| **库级变红（3 条）** | `test result: FAILED. 5 passed; 3 failed`<br>• `has_more_pages_iff_the_total_exceeds_the_limit` → `total(5) > limit(1) 时 nextCursor 必须非 null（D184）`（`query_real_db.rs:103`）<br>• `two_consecutive_pages_share_no_ids` → `第 1 页有下一页 → nextCursor 必须非 null`（`:150`）<br>• `the_total_used_by_the_cursor_check_is_the_filtered_total` → `过滤后仍有下一页`（`:201`） |
| **HTTP 层变红（1 条）** | `the_http_page_carries_a_usable_next_cursor` FAILED —— 响应体实测 `{"items":[…2 条…],"nextCursor":null,"total":5}`（`audit_chain_real_db.rs:571`） |
| 还原 | `cp` + `touch` → `sha256sum -c` → **成功**；`grep -c MUTATION` = **0** |
| 复跑 | 库级 `8 passed`、HTTP `4 passed`（绿）✅ |

### 4.2 B2 —— 去掉 `render_filters` 的 ` AND ` 前缀（F2 的原缺陷）

| 步 | 证据 |
|---|---|
| 备份 sha256 | 同上（**与 B1 同一文件**，还原后 sha256 一致 → 互不污染） |
| 注入 | 恢复「只在过滤器之间补 AND」的原写法（5 个 op 去前缀 + 循环头补 `if !clause.is_empty() { … }`），共 6 处 `MUTATION-B2*` 标记 |
| **库级变红（6 条）** | `test result: FAILED. 2 passed; 6 failed`；错误**逐字**复现复验报告的 B-02：<br>`SinkUnavailable { reason: "… trailing junk after parameter at or near \"$1actor_id\"" }`<br>（`$1action` / `$1created_at` 同形）<br>红的 6 条 = eq / ne / in / contains / range + 等价式 |
| **HTTP 层变红（1 条）** | `a_filtered_query_over_http_is_not_a_server_error` FAILED —— `left: 503 / right: 200`，响应体 `"code":"AUDIT_SINK_UNAVAILABLE"` + `trailing junk … "$1actor_id"` |
| 还原 | `cp` + `touch` → `sha256sum -c` → **成功**；`grep -c MUTATION` = **0**；`git diff --stat` 回到 `30 ++--`（与 Dev 原始改动量一致） |
| 复跑 | 库级 `8 passed`、HTTP `4 passed`（绿）✅ |

### 4.3 B3 —— 删 `en-US.json` 的 `error.AUDIT_CHAIN_BROKEN`

| 步 | 证据 |
|---|---|
| 备份 sha256 | `34b4d6528fccea6b4cedd5b11a5ca7f7b6fab77a2cfdeeceb93a372271cfb823  packages/i18n/src/locales/en-US.json`（与 Dev 记录一致） |
| 注入 | 删除该键 → `grep -c '"error\.'` 由 **84 → 83**（zh 仍 84） |
| **变红（2 条）** | `test result: FAILED. 7 passed; 2 failed`<br>• `every_error_code_is_registered_in_every_locale` → `78 个错误码在词库里没登记… en-US 缺 1 条：["AUDIT_CHAIN_BROKEN"]`（`:392`）<br>• `titles_error_codes_are_registered_in_every_locale` → 同上（`:439`） |
| 还原 | `cp` + `touch` → `sha256sum -c` → **成功**（en / zh 双通过） |

### 4.4 B4 —— **只**删 `zh-CN.json` 的 `error.AUDIT_EVENT_NOT_FOUND`（英文**完好**）★关键★

| 步 | 证据 |
|---|---|
| 备份 sha256 | `ee3cc4f251785ac4784786bb0f5c47c9914221948f56579cb0d4dfcc452177a9  packages/i18n/src/locales/zh-CN.json` |
| 注入 | 删中文该键 → `en: 84  zh: 83`（**英文词库完好**） |
| **变红（2 条）** | `test result: FAILED. 7 passed; 2 failed`<br>• `every_error_code_is_registered_in_every_locale` → `zh-CN 缺 1 条：["AUDIT_EVENT_NOT_FOUND"]`<br>• `titles_error_codes_are_registered_in_every_locale` → 同上 |
| **意义** | 英文侧一条不缺、**只少中文那一条仍然红** → 直接证明 `zh-CN.json` **真的被检查**（若只查 en-US，这一条会假绿）✅ |
| 还原 | `cp` + `touch` → `sha256sum -c` → **成功**；`git status packages/i18n/src/locales/` = **空** |

### 4.5 B5 —— org 桥接「只构造事件、不进 sink」（验证 F5 断言有效）

| 步 | 证据 |
|---|---|
| 备份 sha256 | `6edd9f9172dac80b65fe07292f304db71f330a00196d2b05b1e0c98ad325f25c  crates/platform-org/src/audit.rs`（与 Dev 记录一致） |
| 注入 | `PlatformSinkAuditRecorder::record` 在 `translate(...)` 后 `let _ = translated; Ok(())`（不调 `self.sink.record`） |
| **变红（1 条）** | `an_org_write_really_lands_in_the_audit_table` FAILED —— `一次 org 写入 = 恰好 1 条审计记录`：`left: 0 / right: 1`（`audit_chain_real_db.rs:182`） |
| **意义** | 证明这条用例**真的**断言「事件进了 `platform_audit_log`」，不是「sink 被调过」的同义反复 —— 即 F5 走的确实是生产适配器链路 ✅ |
| 还原 | `cp` + `touch` → `sha256sum -c` → **成功**；`grep -c MUTATION` = **0**；`git status crates/platform-org/` = **空**（该文件**不在**修复轮改动清单里 → 彻底还原） |

### 4.6 第 6 处（**自选**）—— `DEFAULT_LIMIT` 20 → 30（用于验证 M-01）

| 步 | 证据 |
|---|---|
| 注入 | `pub const DEFAULT_LIMIT: usize = 30; // MUTATION-DFLT` |
| 结果 | `platform-audit --lib the_defaults_are_the_contract_defaults` → **FAILED** ✅<br>`platform-config --test config_real_db the_audit_page_default_size` → **ok**（**仍是绿的**）❌ |
| 意义 | 这是 M-01 的**决定性证据**：F3 的新断言对「运行时默认漂移」是盲的（详见 §2 M-01） |
| 还原 | `cp` + `touch` → `sha256sum -c` → **成功** |

### 4.7 还原后的总校验

```bash
$ grep -rn "MUTATION" crates/ packages/ migrations/ | wc -l
0
$ git status --short          # 与 Dev 交付状态逐文件相同（12 M + 2 ??，无 .bak、无临时文件）
$ cargo test -p platform-audit  --all-targets -- --test-threads=1 --include-ignored  → 54 passed
$ cargo test -p platform-server --all-targets -- --test-threads=1 --include-ignored  → 127 passed
$ cargo test -p platform-config --all-targets -- --test-threads=1 --include-ignored  → 89 passed
$ cargo test -p platform-org    --all-targets -- --test-threads=1 --include-ignored  → 93 passed
```

→ 变异注入**未改变**被测代码的最终状态；4 个受影响 crate 复跑数字与首轮全量**逐项一致**。

---

## 5. 必查 A：两条阻塞的黑盒验证（起真实服务 + 真实 token）

> 服务：`scripts/e2e-server.sh`（迁移 → seed → `base serve`，`base_app` 连接，
> `BASE_STATIC_DIR=apps/shell/dist`）；token 由 `POST /api/iam/login` 取得。
> 所有探测都**看响应体**，不只看状态码。

### 5.1 A1 —— `nextCursor` 不再恒 null ✅

```
GET /api/audit/events?limit=2
  HTTP=200  CT=application/json
  total=19  items=2  nextCursor="MjAyNi0xMS0yMFQwNTozMTo1NS4yMDI1NTlafGRlMWU1MTA3…"   ← 非 null
```

**完整翻页链**（`limit=2`，一直翻到 `nextCursor == null`）：

```
page1:  items=2  total=19  nextCursor=yes   overlap_with_previous=[]
page2:  items=2  total=19  nextCursor=yes   overlap_with_previous=[]
…
page9:  items=2  total=19  nextCursor=yes   overlap_with_previous=[]
page10: items=1  total=19  nextCursor=NULL  overlap_with_previous=[]

unique total collected: 19    duplicates: 0
```

- 10 页共取回 **19 条 = total**，**零重复、零遗漏** ✅
- **末页 `nextCursor` 为 null** ✅
- **`total > limit` ⟺ `nextCursor != null`**：page1~9 全部为 yes、page10 为 null ✅
- **不足一页**：`limit=200 > total=19` → `items=19, nextCursor=None` ✅

### 5.2 A2 —— 过滤查询不再 503 ✅

| 探测 | status | content-type | total | 判定 |
|---|---|---|---|---|
| `filter[actorId][eq]=e2e-actor-01` | **200** | application/json | **1** | ✅ |
| `filter[action][eq]=e2e.fixture.item-01` | **200** | application/json | **1** | ✅ |
| `filter[action][ne]=e2e.fixture.item-01` | **200** | application/json | **18** | ✅ |
| `filter[action][in]=…item-01,…item-02` | **200** | application/json | **2** | ✅ |
| `filter[action][contains]=fixture.item` | **200** | application/json | **11** | ✅ |
| `filter[createdAt][range]=2026…,2027…` | **200** | application/json | **19** | ✅ |
| `filter[objectType][eq]=e2e_fixture` | **200** | application/json | **11** | ✅ |

→ **没有任何 5xx**；行数与写入数据一致（11 条 `e2e_fixture` 夹具）。

### 5.3 AC-S-130 其余步骤（顺带复核，上轮已过，确认未回归）

| 步骤 | 实测 | 判定 |
|---|---|---|
| 5 `?cursor=…&page=1` | **400** `AUDIT_INVALID_FILTER`，body keys = `code/status/detail/instance/trace_id` | ✅ |
| 6 `limit=0` | **400** `AUDIT_INVALID_FILTER`（同上形状） | ✅ |
| 6 `limit=abc` | **400** `AUDIT_INVALID_FILTER` | ✅ |
| 6 `limit=1000` | **200**，`items=19`（夹到上限、不报错） | ✅ |
| 6 `sort=unknownField` | **400** `AUDIT_INVALID_FILTER` | ✅ |
| 7 无 `audit:events:read` 令牌 | **403** `INSUFFICIENT_PERMISSION` | ✅ |
| 8 `?tenant_id=tenant-b` | **200**，`total` 与 `id` 集合与不带该参数**完全相同**（被忽略） | ✅ |
| 8 的 warn 日志 | `WARN platform_audit::query: the audit query received a tenant_id parameter; it is ignored …` | ✅ |
| 400/403 的 Content-Type | `application/json`（D183 口径） | ✅ |

---

## 6. 必查 C / D 的证据要点（除已列问题外）

- **C2（F4 夹具）**：跑完 `platform-config` 全量后
  ```sql
  select key, value from platform_config where key like 'audit.%';
  → audit.page.defaultSize|20   audit.page.maxSize|200
    audit.retention.archiveDays|365   audit.retention.onlineDays|90
  select count(*) from platform_config;  → 10   （6 个夹具键 + 4 个 audit.*）
  ```
  → 上轮复验的 `audit.* count = 0` **已消失** ✅。
  「有没有用例依赖整表为空」我独立复查：`DELETE FROM platform_config"`（整表）**0 处**；
  `SELECT count(*) FROM platform_config`（不带 WHERE）1 处（`config_real_db.rs:1307`），
  断言是 `assert!(rows.0 > 0)` → **不要求为空**，不受影响 ✅。

- **C3（F5 生产适配器）**：`main.rs:319-320` 构造 `PgAuditSink`；
  `:647-648` 用 `platform_org::audit::PlatformSinkAuditRecorder::new(audit_sink)`；
  `:485` 用 `config_service.with_audit_sink(audit_sink.clone())` →
  测试里 `audit_chain_real_db.rs:164-172`（org）与 `:222-226`（config）
  用的是**同一套类型**，**没有**自造等价实现 ✅（并由 B5 变异反向证明链路真实）。

- **C6（F8 trait 影响面）**：5 个 crate 各有**独立**的 `TokenVerifier` trait
  （`platform-rbac` → `RbacError`、`platform-schema` → `AuthError`、
  `platform-org` → `OrgError`、`platform-config` → `ConfigError`、
  `platform-server` → 本轮新增 `TokenVerifyError`）→ **互不影响** ✅；
  `git status` 里**无任何 `Cargo.toml`** → **未引新依赖** ✅。

- **C8（F10 判据）**：我独立算了两侧集合：
  ```
  TITLES = 84（78 错误码 + 6 个 org.* 事件码）
  i18n error.* = 84（两 locale 一致）
  i18n \ TITLES错误码 = [CONFIG_FORBIDDEN_COMBINATION, CONFIG_INVALID_VALUE,
                        CONFIG_MISSING_REQUIRED, INVALID_CONFIG_KEY,
                        INVALID_IDENTIFIER, IO_ERROR]        ← 6 个「不进响应体」的码
  TITLES错误码 \ i18n = []                                   ← 零缺失
  ```
  → 判据确实是**包含**关系（不是集合相等）✅；组织事件码按 `ORG_EVENT_SOURCE`
  来源标签排除，并用 `org_codes.len() == OrgEventKind::ALL_CODES.len()` +
  `codes.is_disjoint(&org_codes)` **自证排除动作真的生效** ✅；
  `TITLES`（`crates/platform-core/src/titles.rs`）**未被改动**（`git status` 空、84 行）✅；
  钉死值仍是 `58 + 21 + 5 = 84` ✅；读不到词库文件直接 `panic!`（不静默返回空集）✅。

- **D1（越界）**：`git status --porcelain` 的 14 项与任务卡 D1 清单
  **`comm` 双向比对，两个方向均为空** → **零越界、零遗漏** ✅。

- **E-U04（全租户 verify）** ✅ 新增补验：
  ```
  $ DATABASE_URL=…base_test cargo run -q -p xtask -- audit verify      # 不给 --tenant
    srv-audit-e2e-531677d8…：通过（检查 2 条，断裂 0 条）
    tenant-a：通过（检查 20 条，断裂 0 条）
    tenant-b：通过（检查 1 条，断裂 0 条）
  审计链校验通过：3 个租户，共 23 条记录            rc=0
  ```

- **E-U06（其余 seed 脚本）** ✅ 新增补验：
  重跑 `deploy/seed/00-init.sql`（`ON_ERROR_STOP`）→ 仅 `NOTICE … already exists`，rc=0；
  重跑 `01-seed.sql` → rc=0（幂等）。**重跑后**：
  ```
  has_table_privilege('base_app','platform_audit_log','UPDATE') = f
  has_table_privilege('base_app','platform_audit_log','DELETE') = f
  has_table_privilege('base_app','platform_audit_log','SELECT') = t
  has_table_privilege('base_app','platform_audit_log','INSERT') = t
  ```
  → append-only **未被这两个脚本破坏**（D147 的例外块只在 `02-app-role.sql`，
  这两个脚本不碰审计表 ACL）。

- **AC-S-129 步骤 4/5 扫描 + 阳性对照**（复核上轮结论未回归）：
  ```bash
  $ grep -rn "INSERT INTO platform_audit_log" --include=*.rs crates/ modules/   → 0 命中
  $ grep -rn "INSERT INTO platform_audit_log" migrations/*.sql                 → 恰好 1 处
       migrations/20261120000200_audit.up.sql:499
  # 阳性对照（同一匹配式扫样例）
  $ grep -c "INSERT INTO platform_audit_log" 样例.rs  → 1
  $ grep -c "INSERT INTO platform_audit_log" 样例.sql → 1
  ```
  ✅ 符合（Rust 0 命中 / 迁移恰好 1 处 / 扫描器有效）。

- **回归抽查**：`cargo test -p platform-audit --test audit_real_db` → **13 passed / 0 failed**，
  含 `the_app_role_cannot_update_or_delete_audit_rows`、
  `the_append_only_guarantee_survives_a_seed_script_rerun`、
  `the_rows_of_another_tenant_are_invisible`、
  `the_archive_moves_old_rows_and_keeps_the_chain_intact`、
  `the_db_and_rust_normalization_rules_are_identical`、
  `the_table_has_exactly_the_thirteen_contract_columns`
  → **修复未把上轮已验证的安全判据改松** ✅。

---

## 7. 未验证项清单（如实列出）

| # | 未验证项 | 原因 | 影响 |
|---|---|---|---|
| **V-01** | **SSO 登录审计**（`iam.sso.login.succeeded`，AC-S-127 步骤 3） | 本机无已配置的 OIDC/LDAP provider；仓库里也没有任何该码的真库用例（`grep` 0 命中） | SSO 侧登录审计**完全无判据**（= L-02） |
| **V-02** | **D157：审计写失败 → 拒绝登录 503** | 未做 sink 故障注入（需锁表 / 撤权限 / 断连接） | 代码路径存在，**未跑通**（= L-03 / U-02） |
| **V-03** | `config.flag.write` 的事件码与 `before`/`after` 形状（**D163 的事件码表**内一行；埋点范围见 D136 第 3 类「配置写入」） | F5 只覆盖 `config.entries.write`；flag 写需特殊权限 | 该事件形状**未核**（= L-03 / U-05） |
| **V-04** | 5 个 `AUDIT_*` 中文文案的**语义**校对 | 只验了键存在 + 非空（`locale_error_codes` 的 blank 检查） | 文案质量**未人工核对**（= L-03 / U-07） |
| **V-05** | `platform_audit_chain_state` 的**跨租户 RLS** | 该表只被两个特权函数读写，未构造租户上下文场景 | 风险低，**未验**（= L-03 / U-09） |
| **V-06** | **生产/既有环境的 `audit.page.defaultSize` 行** | 我只有 `base_test` 访问权；迁移已进 `_sqlx_migrations` 不会重跑 | 既有环境若已有该行，需手工跑一次幂等 `platform_config_upsert(…,'20'::jsonb)`（= Dev D-2；**本条不阻塞**：迁移属本功能分支，无环境应用过 —— 见附录 A.4） |
| **V-07** | E2E 只在 `chromium` 一个 project 跑 | `playwright.config.ts` 仅配置 chromium | 其它浏览器**未覆盖**（与上轮口径一致） |

> 另：**H-01 已复现并定性**（不是「未验证」）；**M-01 已用实验证明**（不是「未验证」）。

---

## 8. 对 Dev 偏离声明的逐条判定

> 来源：`req-014-dev-fixround.md` §11（D-1~D-7）+ `req-014-dev-fixround-f10.md` §5。

| # | Dev 的声明 | 我的判定 | 依据 |
|---|---|---|---|
| **D-1** | **F10 未落地**（主报告 §12：`i18n_titles.rs` 保持原样未修改） | **拒绝（声明已过时）** | 工作区实测 `i18n_titles.rs +214`、两条新断言存在且 B3/B4 能抓红。**实际已按 D194 落地** → 主报告 §12 作废，以 `f10` 小报告为准（= M-02） |
| **D-2** | F3 的 DB 行同步**只在测试库**完成，生产需手工 upsert | **接受** | 我无生产库访问权；且附录 A.4 已核对「迁移属本功能分支、无环境应用过」→ 新部署跑的就是 `'20'`。**建议**在部署清单里留一行幂等 upsert 作为「既有库」保险 |
| **D-3** | F3 的一致性断言**不引用** `platform_audit::DEFAULT_LIMIT`（分层原因），改用常量 + 注释 | **接受做法，但需补正表述** | 分层理由成立（`platform-config` 不依赖 `platform-audit`）；但「运行时默认仍被钉住」这句话只在 `platform-audit` 侧成立，**config 侧那条对新断言是盲的**（我已用 `DEFAULT_LIMIT→30` 证明它仍绿）→ 见 M-01，建议改文档或补一条三处比对 |
| **D-4** | config 首次写入的 `before` 期望值是 `{"title":null}`（不是 `{}`） | **接受** | 与 `ConfigService::layer_values` 的 doc「该层没有这个键 → `null`」一致；且该用例**第二次**写入断言 `before == {"title":"审计端到端"}` → 比 `{}` 更严，**不是放宽** ✅（实测 ok） |
| **D-5** | 额外做了第 3 处变异（F5 的 org 桥接） | **接受（并已独立复现）** | 我独立注入同一变异 → `an_org_write_really_lands_in_the_audit_table` FAILED（`left:0/right:1`），**确认该断言有效** ✅ |
| **D-6** | 额外补了 2 条 HTTP 层断言（`nextCursor` 序列化、过滤非 5xx） | **接受（并已验证有效）** | B1 让 `the_http_page_carries_a_usable_next_cursor` 红；B2 让 `a_filtered_query_over_http_is_not_a_server_error` 红 → 两条都真正起作用，不是装饰 ✅ |
| **D-7** | F8 过程中一次误改（`platform-iam/src/service.rs:981/1197`）已回退 | **接受** | `git diff crates/platform-iam/src/service.rs` = **4 insertions / 4 deletions**（正是 clippy 报告的 4 处 `needless borrow`），无多余改动 ✅ |
| **f10-2** | `repo_root()` 层级由「三级」改为「二级」（工单附录 A.2.2 写 `../../..`） | **接受** | `env!("CARGO_MANIFEST_DIR")` = `crates/platform-server`，**往上两级**才是仓库根；照工单写三级会指到 workspace 根（其上还有 `code/` 与 `feature-REQ-014/`）。与仓内既有惯例（`i18n_no_chinese_titles.rs:492`）一致 ✅。**工单相对路径为笔误，建议 BA 订正附录 A.2.2** |
| **f10-3** | `error_codes()` 用来源标签排除组织事件码，不改 `all_codes()` 结构 | **接受** | 保持 `all_codes()` 被 `titles_cover_every_code` / `no_orphan_titles` 复用的语义不变；且排除动作有**自证**（`len` 相等 + `is_disjoint`），不是静默排除 ✅ |
| **f10 风险 1** | `LOCALES` 是手写常量，新增语言不会自动覆盖 | **接受，建议记 TD** | 用「扫目录」会让「误删某个 json」反而通过（少一个 locale = 少查一个）→ 手写常量是**正确**的取舍；但需一条 TD 提醒「加语言时同步 `LOCALES`」 |

---

## 9. 给 BA 的处置建议

1. **H-01 必须修**（1 行）：`e2e/audit.spec.ts:228` 后补
   `await page.getByLabel('每页条数').selectOption('10');`；
   修完后**必须在干净库**（`DROP SCHEMA public CASCADE` + `00-init` + seed）上重跑
   `npx playwright test e2e/` 并给出 **35 passed / 0 failed / 0 skipped** 的真实输出。
   ⚠️ **不许**靠「多加点夹具」把红灯盖掉（那会让用例继续依赖与判据无关的数据量）。
2. **M-01 建议同轮改论述**（不改行为）：把 `the_audit_page_default_size_is_the_same_everywhere`
   的 doc「三处来源」与「运行时默认由本用例钉住」的说法**改成两处 + 显式说明运行时由谁钉**；
   或按建议 2 在 `platform-server/tests/` 补一条真正的三处比对。
3. **M-02 要求 Dev 合并两份报告**：主报告 §0/§12/§13 + 925 总数更新为
   「F10 已落地、总数 927」，并交叉引用 f10 小报告。**这是流程项，不是代码项。**
4. **L-01** 顺手改数字（127 / 927）。
5. **L-02 / V-01 / V-02 / V-03** 建议下一轮单独立项（SSO 登录审计 + D157 的 503 +
   `config.flag.write` 形状）—— 这三条都是**安全/契约关键路径**且**当前无判据**。
6. **附录 A.2.2 的相对路径**（`../../..` → `../..`）建议订正，避免下一个 Dev 照抄踩坑。
7. **不需要**重跑 11 个 crate（我已跑，927 全绿）—— 但 H-01 改完后**必须**重跑 E2E 全量。

---

## 10. 本次复核的写入与还原声明

- **唯一写入的文件**：本报告 `/home/myg133/Projects/system_seed/BA/feature-meta/qa-recheck-req014.md`
- **未修改** feature worktree 的任何生产代码 / 测试代码 / 迁移 / 配置
  （除 §4 的 6 处变异，全部按规程还原：`sha256sum -c` 通过、`grep -c MUTATION = 0`、
  `git status --short` 与 Dev 交付状态**逐文件相同**）
- **未 `git commit`**
- 数据库侧：我 DELETE 过 `platform_audit_log` 里 `tenant_id='tenant-a'` 的行
  用于构造「干净库」复现 H-01；随后用 **`seed-e2e.sh` 幂等重播**恢复夹具，
  并用 `platform_config_upsert` 把 `audit.page.defaultSize` 复原为 `20`。
  → **测试库**状态可自愈（`scripts/test-integration.sh` 亦会重置）；**未触碰 `base_dev`**。

---

*本报告的每条结论都附可复现的命令输出或 `file:line`；所有数字由复核者自己重跑、自己用 awk 算；
阻塞级两条与全部新断言都经**独立注入变异**验证；Dev 的自验数字未被采信。*
