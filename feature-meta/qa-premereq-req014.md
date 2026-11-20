# REQ-014 审计日志 —— 合并前复验报告

- **被验对象**：`/home/myg133/Projects/system_seed/feature-REQ-014`，分支 `feature/REQ-014`，HEAD `5f28c0c`
- **复验基线**：`BA/demands/REQ-014-audit/acceptance.md` **rev 3**（13 条 AC）/
  `decisions.md` **rev 3**（D130~D183）/ `design-summary.md` rev 2 /
  `BA/contracts/api-conventions/v1.md` / `BA/plan/dev-guide-low-spec.md` §9（12 条出口标准）
- **任务卡**：`BA/feature-meta/req014-premerge-brief.md`
- **复验人**：QA 复验子 Agent（**不采信 Dev 自验报告**；所有数字由本次复验自己重跑、自己用 awk 算）
- **复验日期**：2026-11-20
- **代码状态**：Dev 已停手、代码冻结；复验前后 `git status --short` 均为空

---

## 1. 结论

# **FAIL**

**2 条阻塞级问题**，其中两条都落在 dev-guide §9.12 的**前三档**（主体不存在 / 物理不可达）——
「按错的规格实现 + 按错的规格验收」的自洽闭环，本轮被**打破**：

| # | 问题 | 档位 | 后果 |
|---|---|---|---|
| B-01 | 游标分页的 `nextCursor` **恒为 `null`** → 翻页不存在 | 主体不存在 / 物理不可达 | AC-S-130 步骤 1~2、AC-S-132 步骤 2 不可达；E2E 2 条红 |
| B-02 | 过滤子句拼不出合法 SQL → **任何带过滤的查询 503** | 主体不存在（SQL 不可执行） | AC-S-130 步骤 3、AC-S-132 步骤 3 不可达；E2E 2 条红 |

两条都在**后端 `platform-audit` 的查询层**，都在**契约 C7 的核心口径**上，
都**不被仓库里任何一条用例覆盖**（假绿），都必须靠**变异验证**才能暴露。

### 分档汇总

| 档位 | 条数 | 编号 |
|---|---|---|
| **阻塞** | **2** | B-01、B-02 |
| **高** | **1** | H-01 |
| **中** | **3** | M-01、M-02、M-03 |
| **低** | **4** | L-01 ~ L-04 |
| 合计 | **10** | |

---

## 2. 逐条问题

> **可判定性分档**用 dev-guide §9.12 的五档：主体不存在 / 物理不可达 / 不可判定 / 口径不一致 / 表述。

---

### B-01 【阻塞】游标分页的 `nextCursor` 恒为 `null` —— 翻页功能不存在

- **关联 AC**：AC-S-130 步骤 1（`nextCursor` 非 `null`）、步骤 2（用 `nextCursor` 翻页）；AC-S-132 步骤 2（翻页后记录不重复）
- **关联决策**：D145、C7、D169
- **可判定性分档**：**主体不存在**（翻页链路不存在） + **物理不可达**（步骤 2 的前置「步骤 1 返回的 `nextCursor`」取不到）
- **根因**：`crates/platform-audit/src/query.rs:419-437`

  ```rust
  sql.push_str(&format!(" ORDER BY {} LIMIT ?", render_order(&params.sort)));
  binds.push(BindValue::Int(limit as i64));          // ← 绑的是 limit，不是 limit + 1
  ...
  let mut rows: Vec<AuditEventRow> = tx.fetch_all(&query).await...;
  // 多取一条用来判断「还有下一页」。
  let has_more = rows.len() > limit;                // ← rows.len() 恒 <= limit → 恒假
  if has_more { rows.truncate(limit); }
  let next_cursor = if has_more { ... } else { None };   // ← 恒 None
  ```

  注释写的是「**多取一条**用来判断『还有下一页』」，但 SQL 绑的是 `LIMIT limit`，
  **没有多取那一条** → `has_more` 恒为 `false` → `next_cursor` 恒为 `None`。

- **证据（HTTP 黑盒，服务实跑在 `:8080`，夹具 13 条）**：

  ```
  GET /api/audit/events?limit=2    -> items=2  total=13 nextCursor=null
  GET /api/audit/events?limit=1    -> items=1  total=13 nextCursor=null
  GET /api/audit/events?limit=200  -> items=13 total=13 nextCursor=null   ← 有下一页却给 null
  GET /api/audit/events            -> items=20 total=13 nextCursor=null
  ```

  原始响应体（`?limit=2`）末行：`"total": 13, "nextCursor": null`

- **前端连带后果**（`apps/shell/src/pages/Audit.svelte`）：
  - `:340` `disabled={nextCursor === null}` → **「下一页」按钮恒为禁用态**
  - `:210` `if (nextCursor === null) { return; }` → `handleNextPage()` 空转
  - → D169 设计的「游标栈页码指示」**永远停在第 1 页**
- **E2E 实证（红）**：`npx playwright test e2e/` → **4 failed**，其中 2 条正是翻页：

  ```
  1) [chromium] › e2e/audit.spec.ts:182:1 › TC-014-113 / TC-014-114：翻页 → 请求带 cursor…  FAILED
  2) [chromium] › e2e/audit.spec.ts:221:1 › TC-014-113（回退）：「上一页」→ 请求回到第 1 页   FAILED
  ```
- **测试盲区**：`grep -rn "next_cursor|nextCursor" crates/ --include=*.rs` →
  只有 `platform-audit/src/query.rs`（生产）与 `platform-server/src/audit.rs`（字段透传），
  **没有任何一条断言 `next_cursor.is_some()`** → 「0 命中 = 测试没覆盖」与「行为正确」输出一样。
- **建议改法**：
  1. `query.rs` 的 SQL 绑 `limit + 1`，取回后：
     ```rust
     let has_more = rows.len() > limit;
     if has_more { rows.truncate(limit); }
     ```
     （即把 `binds.push(BindValue::Int(limit as i64))` 改成 `limit as i64 + 1`，
     并让 `has_more` 判定在截断**之前**取）
  2. 补一条**库级**断言：「`total > limit` 时 `next_cursor` 必非 `None`」，
     并断言第 2 页与第 1 页 id **无交集」——否则这类回归还会假绿。
  3. 同步确认 `platform-server/src/audit.rs:402` 的 `next_cursor` 透传（**无需改**，字段已在）。

---

### B-02 【阻塞】过滤子句拼不出合法 SQL —— 任何带过滤的查询 503

- **关联 AC**：AC-S-130 步骤 3（`?filter[actorId][eq]=…` 等 4 种过滤）、AC-S-132 步骤 3
- **关联决策**：D145、C7
- **可判定性分档**：**主体不存在**（拼出的 SQL 不可执行）
- **根因**：`crates/platform-audit/src/query.rs:482-529` `render_filters()`

  ```rust
  let mut clause = String::new();
  for filter in filters {
      if !clause.is_empty() {          // ← 只在「过滤器之间」补 " AND "
          clause.push_str(" AND ");
      }
      ...
      clause.push_str(&format!("{column} = ?{}", ...));   // 第 1 个过滤器没有分隔符
  }
  ```

  两个调用点都把 clause **直接**贴在 `{tenant}` 后面（`query.rs:391-392`、`:401-405`）：

  ```rust
  format!("SELECT count(*) FROM platform_audit_log WHERE tenant_id = {{tenant}}{clause}")
  "… FROM platform_audit_log WHERE tenant_id = {{tenant}}{clause}"
  ```

  → 拼成 `WHERE tenant_id = {tenant}actor_id = ?`
  → `crates/platform-db/src/query.rs:273-281` 渲染：`{tenant}` → `$1`，`?` → `$2`
  → `WHERE tenant_id = $1actor_id = $2` → **Postgres 解析失败**

- **证据（实测）**：

  ```
  GET /api/audit/events?filter[actorId][eq]=e2e-actor-01
  -> HTTP 503
  {"code":"AUDIT_SINK_UNAVAILABLE",
   "detail":"the audit sink is unavailable: the audit query could not be executed:
              数据库访问失败：error returned from database:
              trailing junk after parameter at or near \"$1actor_id\""}
  ```

  （同一接口**不带**过滤时 200 → 证明问题出在过滤子句的拼接，不在其它地方。）

- **E2E 实证（红）**：

  ```
  3) [chromium] › e2e/audit.spec.ts:242:1 › TC-014-115 / TC-014-117：按操作人过滤…  FAILED
       expect(actors.length).toBeGreaterThan(0)  → Received: 0
  4) [chromium] › e2e/audit.spec.ts:289:1 › TC-014-116：按动作过滤…              FAILED
       expect(actions.length).toBeGreaterThan(0) → Received: 0
  ```

  （E2E 断言「行数 > 0」拿到 0 行，是因为前端把 503 当成空结果渲染，
  **不是**后端返回了空集 —— 错误被前端吞成了「没有匹配的记录」。）

- **测试盲区**：
  - `crates/platform-audit/src/query.rs` 的单测只测 `AuditQueryParams::parse`（**参数解析**），
    **从不**把过滤条件跑到库上；
  - `grep -rn "filter\[" crates/platform-audit/tests/` → **0 命中**；
  - → 「过滤功能」在仓库里**只有解析层断言，没有执行层断言** → 典型的假绿。
- **建议改法**（二选一，推荐前者）：
  1. `render_filters` 的**第一个**过滤器也要带 ` AND `（把 `if !clause.is_empty()` 改成
     先 push 一个前导 ` AND `，或让调用点写 `WHERE tenant_id = {tenant} AND {clause}` 且 clause 不自带前缀）；
  2. 补一条**库级**用例：「写入 5 条 → `filter[actorId][eq]=…` → 返回恰好 1 条且
     `total == 1`」，并对 `eq/ne/in/contains/range` 五个 op 各覆盖一次
     （`range` 的两个值、`contains` 的转义都要钉住）。
- **附带建议**：这条查询失败被映射成 **503 `AUDIT_SINK_UNAVAILABLE`**，
  而根因是「SQL 拼错」—— 错误码**误导运维**（像是 DB 挂了）。
  建议在 `query.rs` 里把「渲染后的 SQL 非法」与「DB 不可达」区分开，
  前者应是 400 `AUDIT_INVALID_FILTER`（与 D153 的「非法值 → 400」口径一致）。

---

### H-01 【高】D182 未落地：`audit.page.defaultSize` 登记值仍是 50，运行时行为是 20

- **关联决策**：**D182**（明文裁决「**把登记值改成 20**」）、D181、C7、C9
- **关联检查**：任务卡 §3.2 第 11 条（🔑「登记值与行为不一致 = **登记在骗人**」）
- **可判定性分档**：**口径不一致**（同一规则两处写法不同：登记 50 / 行为 20）
- **证据**：

  | 位置 | 内容 |
  |---|---|
  | `crates/platform-config/manifest.json:210-215` | `"audit.page.defaultSize": { "type": "number", "default": 50, …, "label": "审计列表默认每页条数（登记值；契约 C7 的默认口径仍是 20）" }` |
  | `migrations/20261120000400_audit_config_seed.up.sql:48` | `SELECT platform_config_upsert('audit.page.defaultSize', '50'::jsonb);` |
  | `crates/platform-audit/src/query.rs:50` | `pub const DEFAULT_LIMIT: usize = 20;`（**运行时行为 = 20**） |
  | `apps/shell/src/lib/audit-api.ts:59` | `export const AUDIT_DEFAULT_LIMIT = 20;` |

  → **登记 50、行为 20**，正是 D182 要消除的「登记在骗人」。
  而且 **label 里那句补丁说明还在**——D182 原文明写
  「`manifest.json` 的 label **去掉**「契约 C7 的默认口径仍是 20」这句补丁说明（既然已经一致，就不需要解释差异）」。
  → 代码里**自己写着**「真实值是 20」却仍登记 50 = 自我声明的违约。

- **影响**：运维在配置面板看到「默认每页 50」，实际接口默认返回 20 条 → 排查时走错方向；
  D182 的原话就是「登记本身在骗人」。
- **建议改法**：
  1. `crates/platform-config/manifest.json:210` 的 `default` 改 **20**，label 去掉括号里的补丁说明；
  2. `migrations/20261120000400_audit_config_seed.up.sql:48` 改 `'20'::jsonb`；
  3. 按 D27（漏 INSERT → 启动失败）同步既有 `platform_config` 行
     （`platform_config_upsert` 是幂等的，重跑即可）。

---

### M-01 【中】`audit.*` 配置项的登记值被 `platform-config` 测试夹具清空，且**无自愈路径**

- **关联决策**：D178 / D181 / D182；D27
- **可判定性分档**：**口径不一致**（登记无持久性）
- **证据**：
  - `crates/platform-config/tests/common/mod.rs:233-241`：
    ```rust
    pub async fn reset_fixture(pool: &PgPool) {
        sqlx::query("DELETE FROM platform_config")   // ← 整表删，不只删自己管的键
            .execute(pool).await.expect("平台级配置应能清空");
        for (key, value) in platform_fixture() { … }  // ← 只补回自己的夹具键
    }
    ```
  - **实测（两次独立复现）**：
    - 第一轮逐 crate 全量跑完后，`select count(*) from platform_config where key like 'audit.%'`
      → **0**（4 个登记值全没了）；按迁移里同样的 `platform_config_upsert` 手工补回 4 行后，
      **重启服务不会删** → 排除「启动自检删」→ 唯一删除者是这个测试夹具；
    - 第二轮（变异还原后的复跑）**确定性复现**：`platform-config` crate 于 12:03:28 跑完，
      `platform_config` 的全部键 = **恰好 6 个**，与 `crates/platform-config/tests/common/mod.rs:292-301`
      的 `platform_fixture()` **逐个相同**（`logo` / `favicon` / `primaryColor` /
      `conflictStrategy` / `scimBaseUrl` / `cacheTtlSeconds`），
      而 `audit.*` 的 4 个键 **count = 0** → 删除者**锁定**为该夹具，且**可重复**。
  - 迁移 `20261120000400` 已记入 `_sqlx_migrations`（`success=true`，
    checksum 与文件 `sha384` 逐字一致）→ **不会再跑一次** → **没人补回**。
- **影响边界（要说准）**：**生产不受影响**（全新库/升级库上迁移插一次即持久）；
  受影响的是**共享测试库**与任何「跑过 `platform-config` 测试」的库 —— 登记值会静默消失，
  而**没有任何测试会因此变红**（本仓没有断言「manifest 的 `config` 段每个键在 `platform_config` 里有行」）。
- **建议改法**：
  1. 夹具复位改成**只删自己管的键**：`DELETE FROM platform_config WHERE key = ANY($1)`，
     或把 4 个 `audit.*` 键加进 `platform_fixture()`；
  2. 补一条**启动自检**断言：manifest `config` 段的每个键在 `platform_config` 里都有行
     → 不一致就 fail-fast（对齐 D27「漏登记 → 启动失败」的思路）。

---

### M-02 【中】AC-S-129「事件都出现在审计表里」只有**平台写入通道**被真库验证；四个业务模块的埋点只用 mock sink

- **关联 AC**：AC-S-129 步骤 1~3、步骤 6
- **可判定性分档**：**不可判定**（AC 的判据「事件出现在审计表里」在仓库里没有机械断言）
- **证据**：
  - `grep -rln "platform_audit_log" crates/*/tests/*.rs` →
    `platform-audit/tests/audit_chain.rs`、`platform-audit/tests/audit_real_db.rs`、
    `platform-iam/tests/sso_audit.rs`；
    **`platform-iam/tests/audit_events.rs`、`platform-rbac/tests/audit_events.rs`、
    `platform-config/tests/audit_events.rs` 都不在列表里** ——
    它们用的是 `RecordingSink`（mock），断言的是「事件被**构造**出来」的形状，
    **没有一条**断言事件真的进了 `platform_audit_log`；
  - `grep -rn "org.node.create" crates/*/tests/*.rs` → **0 命中**
    → `platform-org` 的事件只由 `crates/platform-org/src/audit.rs:283` 的
    `the_platform_sink_recorder_forwards_the_event`（**mock sink**）覆盖。
- **本次复验补上的部分（黑盒）**：我用**真实 HTTP 接口**验了 iam 与 rbac 两条链路**真的落库**：
  - 登录 → `iam.login.succeeded` 1 条，`ip` 逐字 `203.0.113.9`、`userAgent` 逐字 `audit-test-agent`；
  - 建角色/授权/撤权/删角色 → `rbac.role.create/grant/revoke/delete` 各 1 条，
    `before`/`after` 形状与 D163 **逐字相同**（见 §4.3）。
  - **org 与 config 两条链路未验**（见 §5）。
- **建议改法**：给 `platform-org` / `platform-config` 各补一条**真库**用例
  （走 `PgAuditSink` → 查 `platform_audit_log`），把 AC-S-129 步骤 1~3 的判据变成机械可判。

---

### M-03 【中】`crates/platform-audit/src/query.rs:21` 的注释仍写「违反契约 C6 的 `application/problem+json` 要求」

- **可判定性分档**：**表述** / **口径不一致**（同一规则两处写法不同）
- **证据**：
  - `crates/platform-audit/src/query.rs:18-21`：
    ```
    //! axum 0.8 的 `QueryRejection` 是**纯文本** 400 → 违反契约 C6 的
    //! `application/problem+json` 要求。本模块只吃
    ```
  - 同一含义在 `crates/platform-server/src/audit.rs:18-21` **已经写对**：
    `违反契约 C6 的 `ProblemDetails` 要求`；
  - **D183 已裁决**：「契约 C6 **没有**规定 Content-Type……判据改为『响应体是 RFC 9457
    Problem Details 形状』」，并明文说「**已同步修正**：`acceptance.md`…`decisions.md`…」
    —— 但**代码注释漏了**（D183 的同步清单里没列代码）。
  - 全仓 `grep -rn "problem\+json"` → 命中 3 处，其中 2 处是 Dev 自验报告/历史报告，
    **1 处是生产代码注释**（即本条）。
- **实测无影响**：Content-Type 实测确为 `application/json`（D183 口径），
  400/403 响应体形状实测为 RFC 9457 Problem Details → **行为正确，只有注释错**。
- **建议改法**：把 `query.rs:21` 改成
  「违反契约 C6 的 `ProblemDetails` 形状要求（Content-Type 跟随平台现状 `application/json`，D183）」。

---

### L-01 【低】`crates/platform-audit/tests/audit_chain.rs:1` 有 2 个 U+FFFD 替换字符

- **可判定性分档**：**表述**
- **证据**（hexdump）：

  ```
  00000040  e5 b8 8d ef bf bd ef bf  bd e5 8f 89 e3 80 81 e7 af      并发不��叉、篡改
  ```

  应为 `并发不分叉`（`分` 被写成了 2 个 U+FFFD）。
- **与基线比对**：基线 `develop@0683115` 全树扫描有 **2 处**同类
  （`crates/platform-org/tests/common/mod.rs:40`、`crates/platform-config/src/error.rs:87`，
  **均为存量**）；feature 树有 **3 处** → **本次新增 1 处**。
- **建议改法**：把 2 个 `U+FFFD` 改回 `分`（不影响编译，但会污染源码与后续检索）。

---

### L-02 【低】clippy 新增 **8** 条 warning（基线 2 条，均为存量）—— 出口标准 §9 说「新增问题必须为 0」

- **可判定性分档**：**表述**（风格类 lint，不影响行为）
- **证据**（`cargo clippy --workspace --all-targets --message-format=short`，feature vs 基线 `0683115` 差集）：

  | 位置 | lint |
  |---|---|
  | `crates/platform-config/src/service.rs:468` | `unnecessary use of to_string` |
  | `crates/platform-config/tests/audit_events.rs:73` | `called filter(..).next_back() on a DoubleEndedIterator` |
  | `crates/platform-iam/src/service.rs:671` | `needless borrow`（`&tenant` → `tenant`） |
  | `crates/platform-iam/src/service.rs:692` | 同上 |
  | `crates/platform-iam/src/service.rs:707` | 同上 |
  | `crates/platform-iam/src/service.rs:743` | 同上 |
  | `crates/platform-rbac/tests/audit_events.rs:78` | `called filter(..).next_back() …` |
  | `crates/platform-server/src/audit.rs:55` | `this returns a Result<_, ()>`（`TokenVerifier::verify` 的 `Result<VerifiedToken, ()>`） |

  基线的 2 条（`platform-iam/src/sso/config.rs:153`、`platform-iam/src/sso/oidc.rs:234`）
  在 feature 里**原样存在** → 存量，未被修。
- **`cargo fmt --all --check`：通过**。feature 的 36 处 fmt diff 与基线**完全同文件、同行号**
  （11 个存量文件）→ **新增 0 处**。
- **建议改法**：8 条全部是机械可修的风格 lint，一次 `cargo clippy --fix` 即可；
  `platform-server/src/audit.rs:55` 的 `Result<_, ()>` 涉及**公开 trait 签名**，
  建议改成 `Result<VerifiedToken, TokenVerifyError>`（新错误类型）而非 `()`。

---

### L-03 【低】配置种子迁移的注释仍写「待 BA 确认」——D181 已关闭该问题

- **可判定性分档**：**表述**
- **证据**：`migrations/20261120000400_audit_config_seed.up.sql:28-29`

  ```
  -- ⚠️ **这是与规格的字面不一致**，已记入本轮自验报告「与规格不一致的发现」，
  -- 待 BA 确认（要么改 D178 正文，要么改契约 C9）。
  ```

  但 **D181 已裁定**：「**采纳 camelCase**，**改 D178 正文**（契约 C9 是对的，D178 是 BA 写错的）」
  → 该「待确认」**已关闭**，键名也确实是 camelCase（实测 4 个键全在 `platform_config`）。
  注释留在生产迁移里会误导后来的维护者。
- **建议改法**：改成「⚠️ D178 正文写的是 snake_case，**与契约 C9 冲突**；D181 已裁定改用 camelCase，
  D178 正文的 snake_case 写法作废」。

---

### L-04 【低】`TITLES`（84）与 i18n 错误域（84）之间**只对计数**，不做集合相等断言

- **可判定性分档**：**不可判定**（计数相等掩盖「少一个、多一个」的互换）
- **证据**：
  - `crates/platform-server/tests/i18n_titles.rs:126-133`
    `the_title_table_has_the_pinned_number_of_rows` 断言 `TITLES.len() == 58 + 21 + 5 = 84` ✓；
    `titles_cover_every_code` 断言 `TITLES` 覆盖 `ALL_CODES` ✓（**集合级**）；
  - `packages/i18n/test/i18n.test.ts:192-193`
    `expect(Object.keys(catalog).filter(k => k.startsWith('error.'))).toHaveLength(codes.length)`
    —— `codes` 是**手写抽样列表**（本次实测 84 项、唯一 84、与 catalog 双向零差集）；
  - → **TITLES ↔ i18n catalog 之间没有任何集合级断言**：把 `AUDIT_CHAIN_BROKEN`
    换成 `AUDIT_CHAIN_BROKEN_X` 并同步改手写列表，两侧计数仍是 84 → **全绿**。
- **建议改法**：加一条跨栈断言：`TITLES` 的 code 集合 == `en-US.json` 的 `error.*` 键集合
  （Rust 侧读 `packages/i18n/src/locales/en-US.json` 即可，不引依赖）。

---

## 3. 我实测的逐 crate 数字（自己跑、自己算）

> 口径（任务卡 §4.1）：
> `cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored`
> `| grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'`
> 串行跑；`XDG_RUNTIME_DIR` / `CARGO_TARGET_DIR` / `DATABASE_URL_TEST` 均按任务卡设置。

| crate | passed | failed | ignored | rc |
|---|---|---|---|---|
| platform-core | **153** | 0 | 0 | 0 |
| platform-db | **46** | 0 | 0 | 0 |
| platform-iam | **178** | 0 | 0 | 0 |
| platform-org | **93** | 0 | 0 | 0 |
| platform-rbac | **83** | 0 | 0 | 0 |
| platform-config | **88** | 0 | 0 | 0 |
| platform-schema | **20** | 0 | 0 | 0 |
| platform-server | **121** | 0 | 0 | 0 |
| platform-audit | **46** | 0 | 0 | 0 |
| sandbox | **33** | 0 | 0 | 0 |
| xtask | **51** | 0 | 0 | 0 |
| **合计** | **912** | **0** | **0** | — |

- 这 11 个 crate 在**变异全部还原后**又完整跑了一遍（上表即还原后的复跑结果）→ 还原干净。
- **与 BA / Dev 报的数字的差异**（说明差异来源，不作为问题）：
  - Dev 报 `platform-core` 143 → **实测 153**；`platform-iam` 99 → **实测 178**；
    `platform-audit` 44 → **实测 46**（Dev 的数字**偏差更大**）。
  - BA 报 `platform-core` 150 / `platform-iam` 168 / `platform-audit` 44 → 实测 153 / 178 / 46。
    差 3 / 10 / 2 —— 与 BA 当时测的 commit 不同（`5f28c0c` 的提交时间是 11:37，
    BA 的实测更早）。**以本表为准。**

### 前端与 E2E

| 项 | 结果 | 判据 | 判定 |
|---|---|---|---|
| `pnpm --filter @base/shell test` | **206 passed / 0 failed**（17 files） | ≥ 206 | ✅ |
| `pnpm --filter @base/i18n test` | **43 passed / 0 failed** | 43 | ✅ |
| `pnpm --filter @base/sdk test` | **38 passed / 0 failed** | 38 | ✅ |
| `pnpm -r build` | 全部 `tsc` + `vite build` 通过（`apps/shell/dist` 产出正常） | 通过 | ✅ |
| `svelte-check --threshold error` | **0 errors and 0 warnings** | 0 error | ✅ |
| `npx playwright test e2e/` | **31 passed / 4 failed / 0 skipped** | 全绿 | ❌ **红**（4 条全是 `e2e/audit.spec.ts`） |

**E2E 4 条红的归因**（全部由 B-01/B-02 直接造成，不是夹具/授权问题）：

| 用例 | 位置 | 归因 |
|---|---|---|
| TC-014-113 / 114 翻页 | `e2e/audit.spec.ts:182` | **B-01**（`nextCursor` 恒 null → 「下一页」按钮恒禁用 → 点击超时） |
| TC-014-113 回退「上一页」 | `e2e/audit.spec.ts:221` | **B-01** |
| TC-014-115 / 117 按操作人过滤 | `e2e/audit.spec.ts:242` | **B-02**（过滤查询 503 → 表格 0 行） |
| TC-014-116 按动作过滤 | `e2e/audit.spec.ts:289` | **B-02** |

**skip 纪律（D108）**：本轮 E2E **0 skipped**；`e2e/audit.spec.ts` 里 4 处 `test.skip`
的条件**全部**是 `!(await auditApiReady(request))`（后端未就绪）→ **符合 D108** ✅。
夹具核对通过：`u-perm-audit` 账号已授 `audit:events:read`（授权核对 `read=2/write=5/none=0/audit=1` 通过），
审计夹具 11 条就位 → **不存在「夹具不对/授权没配好」的 skip**。

---

## 4. 变异验证（**5 处**，自己注入、自己还原；覆盖任务卡建议的**全部 5 个**变异点）

> 全程用任务卡 §4.1 的规程：`cp -p` 备份 → `sha256sum` 基线 → 注入**生产那一行** →
> 跑测试确认**变红** → 还原 **+ `touch`** → `sha256sum -c` → `grep -c 'MUTATION'` → `git status --short`。
> ⚠️ 未跟踪/DB 内的产物另记 sha256 与逐字节比对证据（见 M4）。

### 4.1 M1 —— `prev_hash_mismatch` 判定改成**永不命中**（建议变异点 #3）

| 步 | 证据 |
|---|---|
| 备份 sha256 | `5416ffa22999e49d4d0221edba1bfb3ddc3a4e14d9e8cabdf4c5e79a189fc6d9  crates/platform-audit/src/verify.rs` |
| 注入 | `verify.rs:272` → `if row.prev_hash.is_empty() && row.prev_hash != previous_hash {`（`prev_hash` 永不为空 → 条件恒假） |
| **变红** | `tampering_with_the_stored_hash_breaks_the_next_link_too ... FAILED` —— `broken.len()` `left: 1 / right: 2`（断言 `audit_chain.rs:264`「改存储 hash 会连带断掉下一条」） |
| 还原 | `cp x.bak` + `touch` → `sha256sum -c` → **成功** |
| 残留 | `grep -c 'MUTATION'` = **0**；`git status --short` = **空** |
| 复跑 | `… ok`（绿）✅ |

### 4.2 M2 —— `tip_hash` 的比较改成**恒真**（建议变异点 #5）

| 步 | 证据 |
|---|---|
| 备份 sha256 | `5416ffa22999e49d4d0221edba1bfb3ddc3a4e14d9e8cabdf4c5e79a189fc6d9  crates/platform-audit/src/verify.rs`（与 M1 同一文件，还原后 sha256 一致） |
| 注入 | `verify.rs:299` → `if actual_tail.len() >= 0 {`（恒真） |
| **变红** | `a_truncated_online_tail_is_detected_after_the_archive ... FAILED` —— `归档之后链校验必须仍然通过`（`audit_real_db.rs:575`） |
| 还原 | `sha256sum -c` → **成功**；`grep -c 'MUTATION'` = **0**；`git status --short` = **空** |
| 复跑 | `… ok`（绿）✅ |

### 4.3 M3 —— 敏感键的**驼峰拆词**那一步去掉（建议变异点 #4）

| 步 | 证据 |
|---|---|
| 备份 sha256 | `dc9810fcf5078c13219733eb6f4a8375cc1cc03781b018bb4a8913cef10e4f33  crates/platform-core/src/extension/audit.rs` |
| 注入 | `audit.rs:504` → 驼峰边界条件追加 `&& split.len() == usize::MAX`（永不插 `_`）→ `apiKey` 归一化成 `apikey` |
| **变红** | `the_db_and_rust_normalization_rules_are_identical ... FAILED` —— `两侧对 apiKey 的归一化必须逐字相同（D172）`：`left: "api_key" / right: "apikey"` |
| 还原 | `sha256sum -c` → **成功**；`grep -c 'MUTATION'` = **0**；`git status --short` = **空** |
| 复跑 | `2 passed`（绿）✅ |

### 4.4 M4 —— `pg_advisory_xact_lock` 那行**去掉**（建议变异点 #1）★**并发分叉**★

| 步 | 证据 |
|---|---|
| 备份 sha256（文件） | `e9d88ade1ab30a28cc9de323889f3fdb0bc88765b7db9e0a2c4c894cfa5d878b  migrations/20261120000200_audit.up.sql` |
| 备份 sha256（DB 内函数） | `pg_get_functiondef('platform_audit_append(text,…,text)')` 全量落盘 `/tmp/req014qa/m4/live_fn_original.sql`（103 行） |
| 注入 | 文件 `20261120000200_audit.up.sql:457` 的 `PERFORM pg_advisory_xact_lock(hashtext('audit:' \|\| p_tenant_id));` → 注释掉；**同一处改动**用 `CREATE OR REPLACE` 施加到**线上函数**（DB 里住着生产代码，文件改了不会自动重跑迁移） |
| ⚠️ 第一次尝试是**假红** | 文件改了但函数没改 → 测试红在 `VersionMismatch(20261120000200)`（**迁移 checksum 不符 → setup 中止**），**不是**真分叉 → 已识别并作废 |
| **变红（正确原因）** | 文件还原后（checksum 对上）再跑：`concurrent_writes_to_one_tenant_do_not_fork_the_chain ... FAILED` —— `排序后第 2 条的 prev_hash 必须是第 1 条的 hash（无分叉）`：`left: "0000…0" / right: "fa2cfc3b…"` → **两条都拿到 genesis = 链分叉**（D161 描述的场景） |
| 还原（文件） | `cp x.bak` + `touch` → `sha256sum -c` → **成功**；`grep -c 'MUTATION'` = **0**；`git status --short` = **空** |
| 还原（线上函数） | 用备份的**原始函数定义** `CREATE OR REPLACE` 回去 → `pg_get_functiondef` 与备份 **逐字节相同**（`diff` → IDENTICAL）；`pg_advisory_xact_lock` 重新出现 |
| 复跑 | `… ok`（绿）✅ |

### 4.5 M5 —— `REVOKE UPDATE, DELETE` 那行**去掉**（建议变异点 #2）★**append-only**★

| 步 | 证据 |
|---|---|
| 备份 sha256 | `704073765930746f47c460ac2ba39e77bf0243512a216e0672f7e3b09d3bed92  deploy/seed/02-app-role.sql` |
| 注入 | 种子脚本的审计表例外块（`:84` / `:89`）里 `REVOKE UPDATE, DELETE` → `GRANT UPDATE, DELETE`（D147 的例外块被破掉） |
| **变红** | 两条同时红：<br>① `the_app_role_cannot_update_or_delete_audit_rows ... FAILED` —— `应用角色的 UPDATE 必须失败: PgQueryResult { rows_affected: 0 }`（**UPDATE 竟然成功了**）<br>② `the_append_only_guarantee_survives_a_seed_script_rerun ... FAILED` —— `重跑种子脚本后 append-only 仍然成立`：`left: "true\|true\|true\|true" / right: "true\|true\|false\|false"`（**UPDATE/DELETE 变成允许**） |
| 还原 | `cp x.bak` + `touch` → `sha256sum -c` → **成功**；`grep -c 'MUTATION'` = **0**；`git status --short` = **空** |
| 线上态还原 | 重跑**未改动的** `deploy/seed/02-app-role.sql` → `platform_audit_log` 的 ACL 回到 `{base=arwdDxtm/base,base_platform_app=ar/base}`（`base_app` 有效权限 `SELECT=t, INSERT=t, UPDATE=f, DELETE=f`） |
| 复跑 | `2 passed`（绿）✅ |

### 4.6 还原后的总校验

- `git status --short`（feature worktree）→ **空**（全部 5 次变异还原后）
- `grep -rc 'MUTATION'`（4 个被改过的文件）→ 全部 **0**
- 4 个被改文件的 `sha256sum -c` → 全部 **成功**
- 线上 `platform_audit_append` 函数与**未改动的迁移文件**逐字节一致
- **变异全部还原后，11 个 crate 重新跑完整量回归 → 912 passed / 0 failed / 0 ignored**
  （第二轮复跑于 11:59:41~12:04:50 完成，逐 crate 数字与 §3 表**逐项一致**）

### 4.7 变异验证**没能**证明的（重要）

上面 5 处变异**全部**能被现有用例抓红 → 说明「append-only / 并发不分叉 / 篡改连锁 / 截断检出 /
敏感键驼峰」这 5 条**已有判据是有效的**。
但 **B-01 与 B-02 对应的两条路径，没有任何一次变异能让它们变红**——
因为仓库里**根本没有**「`nextCursor` 非 null」与「过滤查询打到库上」的断言。
→ 这两条是「**判据缺失**」而不是「实现错误」，只能靠本报告的黑盒证据定案。

---

## 5. 未验证项清单（如实列出，不写「应该没问题」）

| # | 未验证项 | 原因 | 影响 |
|---|---|---|---|
| U-01 | **AC-S-127 步骤 3：SSO 登录审计**（`iam.sso.login.succeeded` 1 条、ip/UA 逐字相等） | 本机**没有**已配置的 OIDC/LDAP provider（`BASE_SSO_*` 未注入） | SSO 侧登录审计**完全未跑**；只验了本地登录的成功/失败两步 |
| U-02 | **D157：审计写失败 → 拒绝登录 503** | 需要构造「审计 sink 写不进」（DB 不可达 / 表被锁 / 权限被撤）；本轮未构造该故障注入 | 代码路径存在（`platform-core/src/extension/audit.rs` 的 `SinkUnavailable`），但**未跑通** |
| U-03 | **AC-S-129 步骤 2/3：`platform-org` 与 `platform-config` 的事件落到真实 `platform_audit_log`** | 这两条链路的埋点只用 **mock sink** 测（见 M-02）；本轮未找到可驱动 org 写入 + config 写入且带审计权限的 HTTP 路径 | org / config 的**端到端落库**未验；iam / rbac 由本轮**黑盒补验**通过 |
| U-04 | **`cargo xtask audit verify`（不带 `--tenant`，校验全部租户）** | 需要**管理员连接** + 跨租户可见性；本轮只跑了 `--tenant` 单租户形态 | 「全租户校验」入口未验 |
| U-05 | **`config.entries.write` / `config.flag.write` 的审计事件码与 before/after 形状** | 需要有配置写权限的操作者；本轮用的 E2E 账号只有 `rbac:*` 与 `sandbox:demo:*` | D163 里这两行的形状**未核** |
| U-06 | **`deploy/seed/` 下其余脚本**（`01-*.sql` 等） | 本轮只重跑了 `02-app-role.sql`（AC-S-123/D147 只要求这一个） | 其余部署脚本未纳入本轮 |
| U-07 | **i18n 的 5 个新错误码在真实 UI 上的中文渲染** | i18n 单测覆盖了键**存在性**与 84 计数，但**未逐个**断言 5 个 `AUDIT_*` 键的中文文案非空且语义正确 | 低风险；但 `packages/i18n/src/locales/zh-CN.json` 的 5 条文案**未人工校对** |
| U-08 | **`svelte-check` 在 `packages/*`（非 `apps/shell`）** | 只有 shell 有 Svelte 组件；其余包是 TS，`tsc` 已在 `pnpm -r build` 中通过 | 低风险 |
| U-09 | **`platform_audit_chain_state` 表在「跨租户」下的 RLS 行为**（本轮只验了在线表与归档表） | — | 该表只被两个特权函数读写，应用角色只读 → 风险低 |

> 另：**E2E 的 4 条红**中，「翻页」两条（B-01）与「过滤」两条（B-02）**已归因**，
> 不属未验证；但**修复后需重跑 E2E 全量**才能确认转绿（本轮无法在不改代码的前提下验证修复效果）。

---

## 6. 任务卡 §3 必查清单 —— 逐条对照

| # | 查什么 | 判定 | 证据摘要 |
|---|---|---|---|
| 1 | append-only 能否被部署脚本破功 | ✅ | 重跑 `deploy/seed/02-app-role.sql` 后 `base_app` 的 UPDATE/DELETE **均 permission denied**，行数不变；ACL 仍 `base_platform_app=ar` |
| 2 | 并发是否分叉 | ✅ | 两个**独立连接**并发写 → 两条 `prev_hash` **互不相同**，第 2 条 == 第 1 条 `hash`，链校验 ok；**变异 M4** 证明该用例能抓红 |
| 3 | 敏感键的驼峰绕过口 | ✅ | DB 侧 `apiKey/APIKey/clientSecret/api-key/嵌套` **全命中**；`secretary/tokenizer` **不命中**；写路径拒绝且未落库；**变异 M3** 证明一致性测试有效 |
| 4 | `tip_hash` 截断检出 | ✅ | 删掉在线表末尾一条 → 校验报出 1 条断裂；**变异 M2** 证明该用例能抓红 |
| 5 | 链校验的 reason 取值 | ✅ | 改 `after` → 恰好 1 条 `hash_mismatch`；改存储 `hash` → **恰好 2 条**：第 2 条 `hash_mismatch` + 第 3 条 `prev_hash_mismatch`（HTTP `/api/audit/verify` 实测）；**变异 M1** 证明有效 |
| 6 | 查询接口的分页口径 | ❌ | `limit=2/1/200` 的 `nextCursor` **恒 null**（**B-01**）；`?cursor`+`?page`→400 ✅；`limit=0/abc`→400 `AUDIT_INVALID_FILTER` ✅；`limit=1000`→夹到 200 ✅；`sort=unknownField`→400 ✅；`tenant_id`→忽略+warn ✅；游标+非默认 sort→400 ✅；**过滤全坏**（**B-02**） |
| 7 | 400/403 的响应体形态 | ✅ | 400/403 均为 RFC 9457 Problem Details（`code/status/detail/instance/trace_id`），Content-Type `application/json`（D183 口径）；无 `text/plain` 泄漏 |
| 8 | 未注册 `/api/*` 的回落 | ✅ | `/api/does-not-exist` → **404** + JSON ProblemDetails（**不是** 200+HTML）；`/`→200 text/html、`/assets/**`→200 正常回落 |
| 9 | C-5 契约检查 | ✅ | `crates/platform-audit/src/authz.rs` 3 个 `require_permission("字面量")` 调用点；`cargo test -p platform-audit --test contract_c5` → **2 passed** |
| 10 | 权限点三处齐 | ✅ | manifest `permissions[]` 3 个 + 迁移 INSERT 3 个 + DB `source` == `crates/platform-audit/manifest.json`（实测 3 行全对） |
| 11 | 配置项登记 | ❌ | 键名 camelCase ✅（4 键全在 `platform_config`）；**登记值 `defaultSize` = 50 ≠ 20**（**H-01**）；且登记值会被测试夹具清空且无自愈（**M-01**） |
| 12 | `TITLES` 条数 == 84 | ✅ | 多行感知计数 = **84**（唯一 84）；i18n `error.*` = 84；抽样列表 = 84，双向零差集。⚠️ 任务卡给的正则 `\(\s*"…"\s*,\s*` 在 GNU grep -E 下**只数到 74**（`\s` 不跨行，10 条多行条目被漏）——**这是任务卡口径问题，不是代码问题**（已按「多行感知」口径复核） |
| 13 | 13 个错误码的 8 处编辑点 | ✅ | ①code 常量 ②enum 变体 ③`code()` ④`detail()` ⑤`ALL_CODES` ⑥`TITLES` ⑦`variants` 数组 ⑧`errors` 数组 —— 全在（`platform-core/src/problem.rs`） |
| 14 | 既有 recorder trait 签名未被破坏 | ✅ | `platform-org::AuditRecorder` 与 `platform-iam::SsoAuditRecorder` 的 `fn record` 签名与基线**逐字相同**；`platform-org`/`platform-iam` 既有测试全绿 |
| 15 | `sso_audit.rs` 那条断言 | ✅ | 意图保留（「不得存在 **SSO 专用** 审计表」，排除 `platform_audit_*`）+ **加了阳性对照**（通用审计表应存在） |
| 16 | 登录审计 | ✅（部分） | 成功 1 条 / 失败 1 条 / 不存在账号 1 条；`ip` 逐字 `203.0.113.9`、`userAgent` 逐字 `audit-test-agent`；全列文本**不含**密码（0 命中）；失败记录 `actor_id` 为 NULL 且**不含**账号名。⚠️ **SSO 一步未验**（U-01） |
| 17 | 权限变更审计 | ✅ | 授予/撤销/创建/删除**各 1 条**；事件码 / `object_type` / `before`·`after` **逐字**符合 D163；撤销方向相反 ✅；`actorId` == 操作者 ✅ |
| 18 | 归档 | ✅ | 跨租户调用**被拒**（`insufficient_privilege`）；幂等（第 2 次 moved=0）；归档表 UPDATE/DELETE **被拒**；列集合 == 在线表 ∪ `{archived_at}`（`archive_only=archived_at`, `log_only=∅`）；`hash`/`prev_hash` 原样保留；归档后链校验仍 ok |
| 19 | RLS | ✅ | **单事务内** `set_config` → 看得见本租户 1 条、看不见别租户 0 条；不设上下文 → **0 行不报错**；写侧 `WITH CHECK` 报 `new row violates row-level security policy`；归档表同样受约束 |
| 20 | 敏感键硬拒绝 | ✅ | `{"apiKey":"x"}` 写入**被拒**（DB 侧 `check_violation`；Rust 侧 `SensitivePayloadRejected` → `AUDIT_SENSITIVE_PAYLOAD_REJECTED` / HTTP **400**，非 500）且**未落库**；正常事件正常落库 |
| 21 | 业务模块不写审计表 | ✅ | `crates/**/*.rs` → **0 命中**；`migrations/**/*.sql` → **恰好 1 处**（`20261120000200_audit.up.sql:499`）；**两条扫描都配了阳性对照**（同一扫描器扫样例必然命中 1） |
| 22 | 逐 crate 全量回归 | ✅ | 912 passed / 0 failed / 0 ignored（11 crate，变异还原后复跑） |
| 23 | 前端 | ✅ | shell **206** / i18n **43** / sdk **38**；`pnpm -r build` 通过；`svelte-check` **0 error 0 warning** |
| 24 | E2E | ❌ | **31 passed / 4 failed / 0 skipped**；4 条红全部由 B-01/B-02 造成；skip 纪律符合 D108 |
| 25 | fmt / clippy | ⚠️ | fmt **通过**（与基线**同 11 个文件同 36 处**，新增 0）；clippy **新增 8 条**（基线 2 条存量）→ 出口标准「新增问题必须为 0」**未满足**（L-02） |
| 26 | `xtask capabilities --check` | ✅ | `能力索引校验通过：14 个文件与源码一致`；`capabilities/permissions.json` 含 3 个新权限点 |
| 27 | `aw.py verify` | ✅ | `结论: 0 FAIL / 0 WARN` |

---

## 7. 其它实测记录（供 BA 参考）

- **契约 C12 的 13 个字段**：`platform_audit_log` 实测 13 列，与 `AC-S-122`「不增不减」一致；
  `platform_audit_archive` == 在线表 ∪ `{archived_at}`。
- **哈希复算独立性**：测试侧独立实现 sha256 + 长度前缀拼接，直接用列里的 `before`/`after` 文本；
  时间戳用 `format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z")` 手工拼装（未复用仓内两个秒精度 helper）
  —— 与 AC-S-124 的复算口径逐字一致 ✅。
- **链基准表**：`platform_audit_chain_state` 有 `head_hash` / `tip_hash` / `archived_at` / `updated_at` 四列；
  `head_hash` **只在归档时**更新、`tip_hash` **每次写入**更新（D171 的裁决）✅。
- **`GET /api/audit/events/{id}`**：命中返回单条；未命中 → **404 `AUDIT_EVENT_NOT_FOUND`** ✅。

---

## 8. 次交付物

`BA/demands/REQ-014-audit/test-cases/frontend.md` —— 已按 **D169 + C7** 从 rev 1 口径
（`page=2` / 具名过滤参数）修订到 rev 3 口径（**游标 + `filter[field][op]`**），
文件头已注明「⚠️ **本文件已按 D169 + 契约 C7 修订**（rev 1 → rev 3 口径，2026-11-20）」。
逐条改动：

| 用例 | rev 1 | rev 3（本次改后） |
|---|---|---|
| TC-014-113 | 请求带 `page=2` | 请求带 `cursor=<上一页 nextCursor>` + `limit=2`、**不带 `page`**；页码指示 = 「**本次浏览**第 2 页」；并显式写明「后端必须返回非 null 的 `nextCursor`」 |
| TC-014-114 | 3 页 id 并集 | 游标翻页 3 页 id 并集；补「默认排序 `createdAt DESC, id DESC`（D155），`id` 作 tiebreaker」 |
| TC-014-115 | `actorId=…` | **`filter[actorId][eq]=…`** |
| TC-014-116 | `action=…` | **`filter[action][eq]=…`** |
| TC-014-117 | `GET /api/audit/events?actorId=…` | `GET /api/audit/events?filter[actorId][eq]=…` |
| 前置基线 | 「后端 `page`/`size`」 | 「契约 C7 的**游标**口径：`?cursor=&limit=`」；补「游标只对默认排序有效（D179）」 |

**其余用例文件（`index.md` / `query-api.md` / `chain.md` / …）未改动**（按任务卡 §6 只改这一份）。

---

## 9. 给 BA 的处置建议

1. **必须派一轮修复**（B-01 + B-02）—— 两处都在 `crates/platform-audit/src/query.rs`，
   改动量小（各 1~2 行），但**必须同时补两条库级断言**，
   否则下一次改动会以同样的方式假绿通过。
2. **H-01（D182）** 可与上面同一轮改掉（`manifest.json` 1 个数字 + 1 段 label + 迁移 1 行）。
3. **M-01** 建议同轮顺手修夹具复位（只删自己管的键）—— 否则登记值在测试库里会持续消失。
4. **L-02** 建议同轮跑一次 `cargo clippy --fix`（8 条全是机械可修的风格 lint）。
5. 修复轮结束后**必须重跑**：11 个 crate 全量 + `npx playwright test e2e/`（4 条红应当转绿）
   + 本报告的 5 处变异验证（确认修复没有把判据改松）。
6. **任务卡 §3.3 #12 的计数口径**建议顺手订正：
   `\(\s*"([^"]*)"\s*,\s*` 在 GNU grep -E 下漏掉 10 条多行条目（74 ≠ 84），
   建议改用多行感知计数（`perl -0777 -ne '…/gs'`），否则下一轮会误判为「少 10 条」。

---

*本报告的每一条结论都附**可复现的命令输出或 file:line**；所有数字由复验者自己重跑、自己用 awk 算；
Dev 的自验报告未被采信。复验前后 feature worktree 的 `git status --short` 均为空。*
