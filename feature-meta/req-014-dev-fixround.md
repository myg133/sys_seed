# REQ-014 修复轮报告（Dev）

- **工作目录**：`/home/myg133/Projects/system_seed/feature-REQ-014`（分支 `feature/REQ-014`，HEAD `5f28c0c`）
- **任务卡**：`BA/feature-meta/req014-fixround-brief.md`（F1~F10）
- **裁决基线**：`BA/demands/REQ-014-audit/decisions.md` **rev 4**（D184~D193）
- **复验报告**：`BA/feature-meta/qa-premereq-req014.md`（FAIL：阻塞 2 / 高 1 / 中 3 / 低 4）
- **代码状态**：**未提交**（按任务卡 §1 纪律）；`git status --short` 只含本报告列出的文件
- **日期**：2026-11-20

---

## 0. 结论摘要

| # | 问题 | 档位 | 本轮处置 | 结果 |
|---|---|---|---|---|
| F1 | `nextCursor` 恒 `null` | 阻塞 | 修 + 补 4 条真库断言 + 变异自证 | ✅ 完成 |
| F2 | 过滤子句拼不出合法 SQL | 阻塞 | 修 + 补 6 条真库断言 + HTTP 断言 + 变异自证 | ✅ 完成 |
| F3 | `audit.page.defaultSize` 登记 50 | 高 | manifest + 迁移 + 新增一致性断言 | ✅ 完成 |
| F4 | 夹具清空整张 `platform_config` | 中 | 只删自己管的 6 个键 | ✅ 完成 |
| F5 | org/config 缺真库端到端断言 | 中 | 新增 2 条**走生产适配器**的真库用例 | ✅ 完成 |
| F6 | `query.rs` 注释口径 | 中 | 按 D183 改写 | ✅ 完成 |
| F7 | `audit_chain.rs:1` 2 个 U+FFFD | 低 | 改回「分」 | ✅ 完成 |
| F8 | clippy 新增 8 条 | 低 | `TokenVerifyError` + 7 条风格 lint | ✅ 完成（新增 0） |
| F9 | 迁移注释「待 BA 确认」 | 低 | 按 D181 改写 | ✅ 完成 |
| F10 | `TITLES` ↔ i18n 只对计数 | 低 | **未落地** | ⛔ **裁决与源码事实冲突**（见 §12） |

> **F10 是本轮唯一未完成项**，原因是任务卡的字面口径（`TITLES` code 集合 == `en-US.json` 的
> `error.*` 键集合）在当前代码结构下**不可能成立**，且该不一致**在基线就已存在**。
> 按任务卡 §6「发现裁决与源码事实冲突 → 停下来，在报告里指出冲突 + 证据，不要自己拍板」处置。

---

## 1. F1 —— `nextCursor` 恒为 `null`（D184，阻塞）

### 改了什么

`crates/platform-audit/src/query.rs:423`（原 `:420`）

```rust
sql.push_str(&format!(" ORDER BY {} LIMIT ?", render_order(&params.sort)));
// 多取一条用来判断「还有下一页」（D184）—— 少取这一条的话
// `rows.len() > limit` 恒假 → `next_cursor` 恒 `None`，翻页功能不存在。
binds.push(BindValue::Int(limit as i64 + 1));
```

**只改这一处**：

- `has_more = rows.len() > limit` 与 `truncate(limit)` 的**原有顺序本来就是对的**
  （判定在截断之前），缺陷**只在绑值**上 —— 所以不需要动它们；
- `next_cursor` 取 `rows.last()` 也在 `truncate` **之后**，取到的正是「本页最后一行」的
  `(created_at, id)`，符合 D184 的「取截断后最后一行」；
- `count_sql` **未动**（它不带 `LIMIT`）。

### 补的库级断言（新文件 `crates/platform-audit/tests/query_real_db.rs`，8 条）

| 用例 | 断言（D184 要求） |
|---|---|
| `has_more_pages_iff_the_total_exceeds_the_limit` | `limit` ∈ {1,2,4} 且 `total=5` → `nextCursor` 非 `null`；`limit=5`（== total）与 `limit=20`（> total）→ `null` |
| `two_consecutive_pages_share_no_ids` | `limit=2` 翻 3 页：两两 `id` **无交集**、末页 `null`、三页并集 == 写入的 5 条 |
| `the_total_used_by_the_cursor_check_is_the_filtered_total` | **带过滤**时同一等价式成立（`total=4`、`limit=2` → 有下一页；`limit=4` → 无） |

⚠️ 前两条覆盖任务卡 §F1 要求的第 1、2、3 点；第三条额外钉住「`total` 是**带过滤**的总数」
（`count_sql` 复用同一 `clause` → 等价式在带过滤时也成立）。

### 额外的 HTTP 层断言（`crates/platform-server/tests/audit_chain_real_db.rs`）

`the_http_page_carries_a_usable_next_cursor`：走**生产 `router()`**，`?limit=2` →
响应体里 `nextCursor` 是**非 null 字符串**，用它翻页两页 `id` 无交集。
补它的理由是库级断言证不到「字段有没有被序列化吃掉」。

### 变异自证

见 §9.1。

---

## 2. F2 —— 过滤子句拼不出合法 SQL（D185，阻塞）

### 改了什么

`crates/platform-audit/src/query.rs:484-534`（`render_filters`，函数体 `:495` 起）

按 D185 **钉死的改法**：每一段**自带 ` AND ` 前缀**，调用点
`WHERE tenant_id = {tenant}{clause}` **保持不变**。

- 删掉 `if !clause.is_empty() { clause.push_str(" AND "); }`；
- 5 个分支（`eq` / `ne` / `in` / `contains` / `range`）的片段全部改成 `" AND {column} …"`；
- ⚠️ 两个 `continue` 分支（时间戳 + `contains`、长度不为 2 的 `range`）**在前缀之前**判定
  → **不可能**留下一个空的 ` AND `（代码顺序已保证，doc comment 里也写明了）。

一处改，`count_sql` 与主查询**两个调用点同时**修好（它们共用同一 `clause`）。

### 补的库级断言（同一新文件，6 条）

| op | 用例 | 断言 |
|---|---|---|
| `eq` | `the_eq_filter_returns_only_the_matching_rows` | `actorId=u-3001` → `total=3`、`id` 集合 == 独立 SQL 查出的期望集合 |
| `ne` | `the_ne_filter_excludes_the_matching_rows` | `action != demo.item.alpha` → `total=2`、结果**不含**该动作 |
| `in` | `the_in_filter_matches_any_of_the_values` | `action in (beta, absent)` → `total=2`、全部是 `beta` |
| `contains` | `the_contains_filter_matches_substrings` | `action contains "item.a"` → `total=3`（`LIKE` 转义路径被执行到） |
| `range` | `the_range_filter_matches_the_timestamp_window` | `createdAt` 全覆盖 → `total=5`；下半段 → `id` 集合 == 手工筛选结果（含端点） |
| 等价式 | `the_total_used_by_the_cursor_check_is_the_filtered_total` | 带过滤时 `total`/`nextCursor` 一致性（与 F1 共用） |

**每一条都从 `AuditQueryParams::parse` 出发**（不是直接构造结构体）——
B-02 的缺陷正是「解析没问题、拼 SQL 才炸」，只构造结构体测不到拼 SQL 那一段。

### 额外的 HTTP 层断言

`a_filtered_query_over_http_is_not_a_server_error`：走生产 `router()`，
`?filter[actorId][eq]=…` → **200**（不是 5xx），`total=1`、`items[0].actorId` 匹配、
`nextCursor` 为 `null`。这直接对应任务卡「**且 HTTP 状态不是 5xx**」那一条。

### 变异自证

见 §9.2。

---

## 3. F3 —— `audit.page.defaultSize` 登记值仍是 50（D186，高）

### 改了什么（3 处，与任务卡一一对应）

1. `crates/platform-config/manifest.json:210-215`
   - `"default": 50` → **`20`**
   - label：`"审计列表默认每页条数（登记值；契约 C7 的默认口径仍是 20）"` →
     **`"审计列表默认每页条数"`**（按 D182 删掉补丁说明）
2. `migrations/20261120000400_audit_config_seed.up.sql`
   - `platform_config_upsert('audit.page.defaultSize', '50'::jsonb)` → **`'20'::jsonb`**
3. **既有 DB 行同步**：测试库已重建（`DROP SCHEMA public CASCADE` + `00-init.sql`）后
   由迁移重新写入 → 实测 `audit.page.defaultSize | 20`（见 §8.3）。
   - ⚠️ 生产/既有库上迁移不会重跑（已进 `_sqlx_migrations`）→ 需 BA 在部署步骤里
     手工执行一次 `SELECT platform_config_upsert('audit.page.defaultSize', '20'::jsonb);`
     （幂等）。**这一步我在本轮无法对生产库执行**，已记入 §11。

### 额外补的断言（防回归）

`crates/platform-config/tests/config_real_db.rs` 新增
`the_audit_page_default_size_is_the_same_everywhere`：

- manifest 登记值 == `20`
- **登记值 == 契约 C7 的默认口径 20**（这条正是 H-01 的判据：「登记在骗人」）
- DB 里的行 == `20`

> ⚠️ **偏离说明**：该用例**没有**直接引用 `platform_audit::DEFAULT_LIMIT` ——
> `platform-config` **不依赖** `platform-audit`（刻意的分层），本轮也不许加依赖。
> 改为在用例里写常量 `CONTRACT_DEFAULT_LIMIT = 20` 并注明；
> 「运行时默认 == 20」由 `platform-audit` 自己的单测
> `the_defaults_are_the_contract_defaults`（`query.rs:611`）继续钉住。

---

## 4. F4 —— `platform-config` 夹具清空整张表（D187，中）

### 改了什么

`crates/platform-config/tests/common/mod.rs:233-247`（`reset_fixture`）

```rust
// 只删本夹具管的键（D187）
let platform_keys: Vec<String> = platform_fixture().into_iter().map(|(key, _)| key.to_string()).collect();
sqlx::query("DELETE FROM platform_config WHERE key = ANY($1)").bind(&platform_keys)…
```

即**只删 `platform_fixture()` 里那 6 个键**（`logo` / `favicon` / `primaryColor` /
`conflictStrategy` / `scimBaseUrl` / `cacheTtlSeconds`）。

### 「有没有用例依赖整表为空」的检查结果

逐条查过 `crates/platform-config/tests/`：

| 检查 | 结果 |
|---|---|
| `DELETE FROM platform_config`（整表）出现次数 | 仅夹具这一处（已改） |
| `SELECT count(*) FROM platform_config`（不带 WHERE） | 1 处：`config_real_db.rs:1307`，断言 `> 0`（**不要求整表为空**）→ 不受影响 |
| 断言键集合/行数 == 6 的用例 | **0 处** |
| `assert_eq!(rows.0, 0, "{table} 不应有未登记键的行")` | `config_real_db.rs:491`，条件带 `WHERE key = 'notRegistered'` → 与本次改动无关 |

→ **没有任何用例依赖「整表为空」**，无需改动其它用例。

### 实测效果（复验的 M-01 现象已消失）

- 复验时：跑完 `platform-config` 测试后 `platform_config` 恰好剩 **6 行**、`audit.*` **count = 0**。
- 本轮（改后，跑完 platform-config 全量）：**10 行** = 6 个夹具键 + **4 个 `audit.*` 键**，
  `audit.*` 全部保留（`defaultSize=20` / `maxSize=200` / `archiveDays=365` / `onlineDays=90`）。
- 按 D187 裁决，**未**添加「启动自检 manifest↔DB 配置键」（那是平台级改动，已记 TD-036）。

---

## 5. F5 —— org / config 两条链路缺真库端到端断言（D188，中）

### 改了什么

新增 `crates/platform-server/tests/audit_chain_real_db.rs`（4 条用例）。
放在 `platform-server` 正是按 D188 —— 它同时依赖 org / config / audit / db，
**不需要**给 org / config 加 dev-dependency。

| 用例 | 链路 | 走的**生产**组件 | 断言 |
|---|---|---|---|
| `an_org_write_really_lands_in_the_audit_table` | org | `PgAuditSink` + **`PlatformSinkAuditRecorder`**（生产桥接）+ `OrgService::create_node` | `action=org.node.create` / `object_type=org_node` / `object_id` == 新建节点 id / `actor_id` == 操作人 |
| `a_config_write_really_lands_in_the_audit_table` | config | `PgAuditSink` + **`ConfigService::with_audit_sink`**（生产注入点）+ `write_entries` | ①`action=config.entries.write` ②`object_type=config_entry` ③`after` == `{"title":"审计端到端"}` ④首次写 `before == {"title":null}` ⑤**第二次写 `before == {"title":"审计端到端"}`**（证明读的是**写入前**那一层，不是写后） |
| `a_filtered_query_over_http_is_not_a_server_error` | — | 生产 `audit::router()` | F2 的 HTTP 判据（见 §2） |
| `the_http_page_carries_a_usable_next_cursor` | — | 生产 `audit::router()` | F1 的 HTTP 判据（见 §1） |

**「必须走生产适配器」已满足**：org 用的是 `platform_org::audit::PlatformSinkAuditRecorder`
（`main.rs:648` 同一类型），config 用的是 `ConfigService::with_audit_sink`（`main.rs` 同一注入点）。
**没有**自己造等价实现。

### 一处**行为事实**的记录（不是缺陷）

config 首次写入某键时，`before` 是 **`{"title": null}`** 而**不是** `{}` ——
这是 `ConfigService::layer_values` 的既定语义（其 doc comment 原文：
「键 → 值；该层没有这个键 → `null`」）。我的第一版断言写成 `{}` 因此变红，
已按**源码语义**（而非我的直觉）改成 `{"title": null}`。这不是放宽断言：
它比 `{}` 更严格地钉住了「键存在、值为 null」这个形状。

### 变异自证（第 3 处，额外做的）

把 `PlatformSinkAuditRecorder::record` 改成「只构造事件、不进 sink」→
`an_org_write_really_lands_in_the_audit_table` **FAILED**（`恰好 1 条审计记录`：`left: 0, right: 1`）。
该变异只用于验证断言有效，未出现在最终 diff 中（sha256 已恢复，见 §9.3）。

---

## 6. F6 —— `query.rs` 模块注释（D189，中）

`crates/platform-audit/src/query.rs:20-23`

```diff
-//! axum 0.8 的 `QueryRejection` 是**纯文本** 400 → 违反契约 C6 的
-//! `application/problem+json` 要求。本模块只吃
+//! axum 0.8 的 `QueryRejection` 是**纯文本** 400 → 违反契约 C6 的
+//! **Problem Details 形状**要求（Content-Type 跟随平台现状 `application/json`，D183）。
+//! 本模块只吃
```

与 `crates/platform-server/src/audit.rs:19-21` 的既有写法对齐。

---

## 7. F7 / F8 / F9

### F7 —— U+FFFD（D190，低）

`crates/platform-audit/tests/audit_chain.rs:1`

- 原：`并发不` + **U+FFFD × 2** + `叉`（hexdump 已确认 `ef bf bd ef bf bd`）
- 现：`并发不分叉`
- 实测：该文件 `U+FFFD` 计数由 **2 → 0**。

### F8 —— clippy 新增 8 条（D191，低）

**① `Result<_, ()>`（D191 钉死的那条）**：`crates/platform-server/src/audit.rs`

新增 `TokenVerifyError`（`audit.rs:52-75`），`TokenVerifier::verify` 改为
`Result<VerifiedToken, TokenVerifyError>`；同步改 `IamTokenVerifier::verify`、
`authenticate` 的 `Err(())` → `Err(TokenVerifyError::Invalid)`、测试里的 `FixedTokens`。
**未引新依赖**；影响面**仅限**该文件（已核：其它 crate 的 `TokenVerifier` 是各自独立的 trait）。

**② 其余 7 条（风格 lint）**：

| 位置 | lint | 改法 |
|---|---|---|
| `platform-config/src/service.rs:468` | unnecessary `to_string` | `&scope.as_str().to_string()` → `scope.as_str()` |
| `platform-iam/src/service.rs:671` | needless borrow | `login_event(&tenant, …)` → `login_event(tenant, …)` |
| `platform-iam/src/service.rs:692` | 同上 | 同上 |
| `platform-iam/src/service.rs:707` | 同上 | 同上 |
| `platform-iam/src/service.rs:743` | 同上 | 同上 |
| `platform-config/tests/audit_events.rs:73` | `filter(..).next_back()` | → `.rev().find(..)` |
| `platform-rbac/tests/audit_events.rs:78` | 同上 | → `.rev().find(..)` |

**实测**：`cargo clippy --workspace --all-targets` → 只剩**基线的 2 条存量**
（`platform-iam/src/sso/config.rs:153`、`platform-iam/src/sso/oidc.rs:234`）→ **新增 0**。

> 过程记录（诚实披露）：我先用脚本批量替换，误把 `:981` / `:1197` 两处也改了 ——
> 那里 `tenant` 是 `String`，按值传入 `&str` 参数**编译不过**。已**回退**那两处，
> 最终 diff 只有 clippy 实际报告的 4 处（`git diff --stat` 显示 `4 insertions / 4 deletions`）。

### F9 —— 迁移注释（D190，低）

`migrations/20261120000400_audit_config_seed.up.sql:28-29`

```diff
--- ⚠️ **这是与规格的字面不一致**，已记入本轮自验报告「与规格不一致的发现」，
--- 待 BA 确认（要么改 D178 正文，要么改契约 C9）。
+-- ⚠️ D178 正文的 snake_case 写法**与契约 C9 冲突**，**D181 已裁定改用 camelCase**，
+-- D178 的 snake_case 写法**作废**。
```

---

## 8. 实测数字

> 环境：`XDG_RUNTIME_DIR=/run/user/$(id -u)`、`CARGO_TARGET_DIR=…/code/target`、
> `DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test`；
> 口径 = 任务卡 §1 的 awk（`p+=$4; f+=$6; i+=$8`），**逐 crate、串行、`--all-targets`**。

### 8.1 逐 crate（11 个，全部自跑自算）

| crate | passed | failed | ignored | 复验基线 | 差 |
|---|---|---|---|---|---|
| platform-core | **153** | 0 | 0 | 153 | 0 |
| platform-db | **46** | 0 | 0 | 46 | 0 |
| platform-iam | **178** | 0 | 0 | 178 | 0 |
| platform-org | **93** | 0 | 0 | 93 | 0 |
| platform-rbac | **83** | 0 | 0 | 83 | 0 |
| platform-config | **89** | 0 | 0 | 88 | **+1** |
| platform-schema | **20** | 0 | 0 | 20 | 0 |
| platform-server | **125** | 0 | 0 | 121 | **+4** |
| platform-audit | **54** | 0 | 0 | 46 | **+8** |
| sandbox | **33** | 0 | 0 | 33 | 0 |
| xtask | **51** | 0 | 0 | 51 | 0 |
| **合计** | **925** | **0** | **0** | 912 | **+13** |

**新增 13 条的构成**（与预期一致，无用例被误删）：

- `platform-audit` **+8**：`tests/query_real_db.rs`（F1 的 3 条 + F2 的 5 条）
- `platform-server` **+4**：`tests/audit_chain_real_db.rs`（org / config 真库 + 2 条 HTTP）
- `platform-config` **+1**：`the_audit_page_default_size_is_the_same_everywhere`

**0 ignored**：本仓真实库用例是 `#[tokio::test]`（**不是** `#[ignore]`），
且全部带 `DATABASE_URL_TEST` 真跑；`--include-ignored` 也加上了（口径要求）。

### 8.2 前端

| 项 | 结果 | 判据 | 判定 |
|---|---|---|---|
| `pnpm --filter @base/shell test` | **206 passed / 0 failed**（17 files） | ≥ 206 | ✅ |
| `pnpm --filter @base/i18n test` | **43 passed / 0 failed** | 43 | ✅ |
| `pnpm --filter @base/sdk test` | **38 passed / 0 failed** | 38 | ✅ |
| `pnpm -r build` | 全通过（`apps/shell/dist` 产出：`index-CzJOhtSa.js` 128.54 kB） | 通过 | ✅ |
| `npx svelte-check --threshold error` | **0 errors and 0 warnings** | 0 error | ✅ |

### 8.3 E2E（关键判据）

```
npx playwright test e2e/   →  35 passed (39.7s)   ，0 failed，0 skipped
```

| 项 | 复验时 | 本轮 |
|---|---|---|
| 总数 | 31 passed / **4 failed** | **35 passed / 0 failed / 0 skipped** |
| 失败用例 | 翻页 ×2、过滤 ×2 | 全部转绿 |

**审计页 6 条用例逐条真跑（无 skip）**——单独跑 `e2e/audit.spec.ts --reporter=list` 的输出：

```
✓ 1  TC-014-118 / TC-014-119：无权限账号访问审计页 → 落 403 页，目标接口请求数 == 0
✓ 2  TC-014-111 / TC-014-112：进入审计查询页 → 列出记录，操作人 / 时间 / 动作 / 对象 / 来源 IP 同时显示
✓ 3  TC-014-113 / TC-014-114：翻页 → 请求带 cursor（游标分页，不是 page），且记录不重复   ← 复验时红
✓ 4  TC-014-113（回退）：「上一页」→ 请求回到第 1 页（不带 cursor）                    ← 复验时红
✓ 5  TC-014-115 / TC-014-117：按操作人过滤 → 过滤下发到后端，且与后端接口结果一致        ← 复验时红
✓ 6  TC-014-116：按动作过滤 → 过滤下发到后端                                          ← 复验时红
```

> ⚠️ **D193 纪律**：这 6 条是**真跑通过**（不是 skip），可以和「已验证」并列。
> 服务是**新二进制**（`base serve` 于 04:36 启动，含本轮修复）+ `apps/shell/dist` 新构建。

**黑盒交叉验证**（直接打 HTTP，独立于 E2E）：

```
GET /api/audit/events?limit=2                       → items=2  total=17  nextCursor="MjAyNi0xMS0yMFQwNDozNjoy…"（非 null）
GET /api/audit/events?filter[objectType][eq]=e2e_fixture → HTTP 200  total=11  items=11
```

复验时同一接口是 `nextCursor=null` 与 HTTP **503** `AUDIT_SINK_UNAVAILABLE`。

### 8.4 其它出口标准

| 检查 | 结果 |
|---|---|
| `cargo xtask capabilities --check` | `能力索引校验通过：14 个文件与源码一致` ✅ |
| `cargo clippy --workspace --all-targets` | 只剩基线 2 条存量 → **新增 0** ✅ |
| `cargo fmt --all --check` | 只剩基线 **11 个存量文件**（TD-017）→ **新增 0** ✅ |

---

## 9. 变异自证（证据链）

> 规程（任务卡 §4.4）：`cp -p` 备份 → `sha256sum` 基线 → 注入 → 跑测试确认**红** →
> `cp` 还原 + **`touch`**（`cp -p` 保留旧 mtime → cargo 会复用陈旧二进制）→
> `sha256sum -c` → `grep -c MUTATION` → `git status`。

### 9.1 F1 —— 把 `limit + 1` 改回 `limit`

| 步 | 证据 |
|---|---|
| 基线 sha256 | `fd4dc6122c93ef43201432e081dfbfc532db81c63977613e3eec281625d89c83  crates/platform-audit/src/query.rs` |
| 注入 | `query.rs:423` → `binds.push(BindValue::Int(limit as i64)); // MUTATION-F1` |
| **变红（库级 3 条）** | `has_more_pages_iff_the_total_exceeds_the_limit` FAILED —— `total(5) > limit(1) 时 nextCursor 必须非 null（D184）`<br>`two_consecutive_pages_share_no_ids` FAILED —— `第 1 页有下一页 → nextCursor 必须非 null`<br>`the_total_used_by_the_cursor_check_is_the_filtered_total` FAILED —— `过滤后仍有下一页`<br>→ `test result: FAILED. 5 passed; 3 failed` |
| **变红（HTTP 层 1 条）** | `the_http_page_carries_a_usable_next_cursor` FAILED —— `5 条、limit=2 → nextCursor 必须是非 null 字符串（D184）`，响应体实测 `"nextCursor":null,"total":5` |
| 还原 | `cp` + `touch` → `sha256sum -c` → **成功** |
| 残留 | `grep -c MUTATION` = **0** |
| 复跑 | `8 passed / 0 failed` + `4 passed / 0 failed`（绿） ✅ |

### 9.2 F2 —— 去掉 `render_filters` 的 ` AND ` 前缀

| 步 | 证据 |
|---|---|
| 基线 sha256 | 同上（`fd4dc612…`，与 F1 同一文件） |
| 注入 | 恢复「只在过滤器之间补 AND」的原写法（`if !clause.is_empty() { clause.push_str(" AND "); } // MUTATION-F2`）+ 5 段去掉前缀 |
| **变红（库级 6 条）** | 5 个 op 的用例 + 等价式用例全部 FAILED，错误**逐字**是复验报告里那条：<br>`SinkUnavailable { reason: "… trailing junk after parameter at or near \"$1actor_id\"" }`<br>（`$1action` / `$1created_at` 同形）→ `test result: FAILED. 2 passed; 6 failed` |
| **变红（HTTP 层 1 条）** | `a_filtered_query_over_http_is_not_a_server_error` FAILED —— `left: 503 / right: 200`，响应体 `"code":"AUDIT_SINK_UNAVAILABLE"` + `trailing junk after parameter at or near "$1actor_id"` → **与复验报告 B-02 的证据逐字一致** |
| 还原 | `cp` + `touch` → `sha256sum -c` → **成功** |
| 残留 | `grep -c MUTATION` = **0** |
| 复跑 | `8 passed / 0 failed` + `4 passed / 0 failed`（绿） ✅ |

### 9.3 F5 —— 让 org 桥接「只构造事件、不进 sink」（额外一处）

| 步 | 证据 |
|---|---|
| 基线 sha256 | `6edd9f9172dac80b65fe07292f304db71f330a00196d2b05b1e0c98ad325f25c  crates/platform-org/src/audit.rs` |
| 注入 | `PlatformSinkAuditRecorder::record` 在 `translate` 后提前 `return Ok(())` |
| **变红** | `an_org_write_really_lands_in_the_audit_table` FAILED —— `一次 org 写入 = 恰好 1 条审计记录`：`left: 0, right: 1` |
| 还原 | `cp` + `touch` → `sha256sum -c` → **成功**；`grep -c MUTATION` = **0** |

### 9.4 还原后的总校验

- `sha256sum -c` → 两个被改文件**逐字节相同**（`成功`）
- `grep -rc MUTATION`（两个被改文件）→ 全部 **0**
- `git status --short` → **只含** §10 列出的文件（无 `.bak`、无临时文件）
- 变异还原后**重跑**：`query_real_db` 8 passed / `audit_chain_real_db` 4 passed（全绿）

---

## 10. 已写入的文件

**代码（feature worktree，未提交）**

| 文件 | 类型 | 对应 |
|---|---|---|
| `crates/platform-audit/src/query.rs` | 改 | F1、F2、F6 |
| `crates/platform-audit/tests/audit_chain.rs` | 改 | F7 |
| `crates/platform-audit/tests/query_real_db.rs` | **新增** | F1 + F2 的库级断言（8 条） |
| `crates/platform-config/manifest.json` | 改 | F3 |
| `crates/platform-config/src/service.rs` | 改 | F8 |
| `crates/platform-config/tests/audit_events.rs` | 改 | F8 |
| `crates/platform-config/tests/common/mod.rs` | 改 | F4 |
| `crates/platform-config/tests/config_real_db.rs` | 改 | F3（一致性断言） |
| `crates/platform-iam/src/service.rs` | 改 | F8 |
| `crates/platform-rbac/tests/audit_events.rs` | 改 | F8 |
| `crates/platform-server/src/audit.rs` | 改 | F8（`TokenVerifyError`） |
| `crates/platform-server/tests/audit_chain_real_db.rs` | **新增** | F5 + F1/F2 的 HTTP 断言（4 条） |
| `migrations/20261120000400_audit_config_seed.up.sql` | 改 | F3 + F9 |

**报告**

- `/home/myg133/Projects/system_seed/BA/feature-meta/req-014-dev-fixround.md`（本文件）

---

## 11. 偏离声明

| # | 与任务卡/裁决不一致处 | 理由 |
|---|---|---|
| D-1 | **F10 未落地**（见 §12） | 裁决的字面口径与源码事实冲突；按任务卡 §6 停下来上报，不自定方案 |
| D-2 | F3 的 DB 行同步**只在测试库**完成 | 生产/既有库需显式跑一次 `platform_config_upsert`（迁移不重跑）。我无生产库访问权 → 见 §13 U-3 |
| D-3 | F3 的一致性断言**不引用** `platform_audit::DEFAULT_LIMIT` | `platform-config` 不依赖 `platform-audit`，本轮不许加依赖；改用常量 + 注释，运行时默认仍由 `platform-audit` 自己的单测钉住 |
| D-4 | F5 的 config 用例把首次写入的 `before` 期望值定为 `{"title":null}` | 这是 `layer_values` 的**既定语义**（doc comment 明写「该层没有这个键 → `null`」）。我最初凭直觉写成 `{}` 导致变红，已按源码事实修正**而非**放宽断言 |
| D-5 | 额外做了第 3 处变异（F5 的 org 桥接） | 任务卡只要求 F1/F2 各一处；新增的真库断言若不验证，等于回到「补了但可能仍是假绿」，故多做一处 |
| D-6 | 额外补了 2 条 HTTP 层断言 | 任务卡要求 F2「**且 HTTP 状态不是 5xx**」，库级断言证不到；F1 的 `nextCursor` 也需要证明「没被序列化吃掉」 |
| D-7 | F8 过程中一次误改（`:981`/`:1197`）已回退 | 如实披露；最终 diff 只含 clippy 实际报告的 4 处 |

---

## 12. ⛔ F10 —— 裁决与源码事实冲突（**需 BA 裁决**）

### 任务卡 / D192 的字面要求

> 在 `crates/platform-server/tests/i18n_titles.rs` 补一条**集合相等**断言：
> `TITLES` 的 code 集合 **==** `packages/i18n/src/locales/en-US.json` 的 `error.*` 键集合。

### 我按字面实现后**变红**（证据）

```
TITLES 与 en-US.json 的 `error.*` 必须集合相等（D192）：
  只在 TITLES 里（6 个）：["org.member-added", "org.member-removed", "org.node-deleted",
                          "org.node-moved", "org.node-renamed", "org.user-departed"]
  只在 i18n 里（6 个）：["CONFIG_FORBIDDEN_COMBINATION", "CONFIG_INVALID_VALUE",
                        "CONFIG_MISSING_REQUIRED", "INVALID_CONFIG_KEY",
                        "INVALID_IDENTIFIER", "IO_ERROR"]
```

### 根因（不是本轮引入，**基线就这样**）

```bash
# 集合规模
i18n  error.* 键 = 84；TITLES = 84（其中 UPPER_SNAKE 错误码 78 + org.* 事件码 6）

# 基线 develop@0683115 —— 同样 6 ↔ 6
git show develop:packages/i18n/src/locales/en-US.json   → 79 个 error.*
git show develop:crates/platform-core/src/titles.rs     → 79 行（含 org.member-added 等）
```

三条结构性原因：

1. **`TITLES` 是一个「多命名空间」表**：除了 HTTP 错误码，它还收**通知事件码**
   （`org.node-created` / `org.member-added` … 6 条，见 `titles_cover_every_code` 里
   `platform_org::notify::OrgEventKind::ALL_CODES` 那一行）。i18n 的 `en-US.json` **只有**
   `error` / `common` / `nav` / `schema` 四个命名空间（实测 `nav` 8 + `common` 62 +
   `error` 84 + `schema` 1 = 155），**没有任何 `org.*` 键** → 这 6 条**永远**只在 `TITLES` 里。
2. **i18n 侧另有 6 个错误码在 `TITLES` 里没有**：
   - `INVALID_CONFIG_KEY` / `INVALID_IDENTIFIER` / `IO_ERROR`
     —— `CoreError::code()`（`platform-core/src/error.rs:30-34`）返回它们，
     但 `platform_core::error` **没有** `ALL_CODES` 常量（所以 `all_codes()` 收不到）；
   - `CONFIG_MISSING_REQUIRED` / `CONFIG_INVALID_VALUE` / `CONFIG_FORBIDDEN_COMBINATION`
     —— `platform-server/src/config.rs:16-20` 的 `pub const`，
     同样**不在** `i18n_titles.rs::all_codes()` 的清单里（`platform-server::health` 的
     3 个码进了清单，这 3 个**没进**）。
   这 6 个是**存量缺口**（不是 REQ-014 引入），且它们真的会出现在 503 启动失败的
   Problem Details 里 → 严格说 `TITLES` 少登记了 6 个码。
3. 因此「84 == 84」这个**计数相等是假象**：两侧各缺 6 个、各有 6 个对方没有的。

### 我做了什么

- 按任务卡 §6「发现裁决与源码事实冲突 → **停下来**，在报告里指出冲突并给证据，**不要自己另定方案**」：
  **撤掉了这条断言**（不留在代码里变红、也不改写成「子集相等」这种弱化版本 ——
  那等于替 BA 拍板）。
- 现状：`i18n_titles.rs` **保持原样**（未修改），既有的
  `the_title_table_has_the_pinned_number_of_rows`（84）与 `titles_cover_every_code` 仍绿。

### 给 BA 的三个可选收口（**请裁决其一，我不自行选择**）

| 选项 | 做法 | 代价 |
|---|---|---|
| A（最小） | 断言改成「`TITLES` 的错误码子集 == i18n 的 `error.*` **减去**那 6 个存量缺口」 | 把缺口写进白名单 = 把问题固化 |
| B（治本） | ① `all_codes()` 补上 `CoreError` 那 3 个 + `platform-server::config` 那 3 个（→ `TITLES` 需 +6 行，`TITLES.len()` 变 **90**）；② 断言改成「`TITLES` 的**错误码子集**（排除 `org.*`）== `error.*` 键集合」 | 要动 `titles.rs` 与行数断言（179+6=90），超出 F1~F10 |
| C | 只在 i18n 侧补齐 6 个 `org.*` 通知文案，再断言全量相等 | 改前端文案域，语义上不合适（通知文案不是 error） |

> 我的建议是 **B**（治本，且能同时消掉「`TITLES` 少登记 6 个码」这个真实缺口），
> 但它会改 `TITLES.len()` 的钉死值 → **属于 BA 的裁决范围**，不由我定。

---

## 13. 未完成 / 未验证项（如实列出）

| # | 项 | 原因 | 影响 |
|---|---|---|---|
| U-1 | **F10 未落地** | §12 的冲突 | `TITLES` ↔ i18n 仍**只对计数**；「少一个多一个的互换」仍不可判 |
| U-2 | `TITLES` **确实少登记 6 个错误码**（`INVALID_CONFIG_KEY` / `INVALID_IDENTIFIER` / `IO_ERROR` / `CONFIG_MISSING_REQUIRED` / `CONFIG_INVALID_VALUE` / `CONFIG_FORBIDDEN_COMBINATION`） | §12 的发现；修它要动 `titles.rs` + 行数断言，超出范围 | 这 6 个码运行时走 `FALLBACK_TITLE`（英文兜底，不是中文）→ 已存在的表述缺口 |
| U-3 | **生产/既有库的 `audit.page.defaultSize` 行未同步** | 迁移已进 `_sqlx_migrations` 不重跑；我只有 `base_test` 访问权 | 既有环境里该值仍是 50（需部署时跑一次幂等 `platform_config_upsert`） |
| U-4 | `nextCursor` 在 **`page` 兼容模式**下的语义 | F1 的 `limit+1` 对 `page` 模式同样生效 → 页码模式也会返回 `nextCursor`。契约 C7 只规定游标模式「响应含 `next_cursor`」，**未规定**页码模式该给什么；任务卡 §3 也明令「不许改分页默认值/游标编码等契约行为」 | **未验证、未改动**。前端页码模式不消费 `nextCursor`（`buildAuditQuery` 用 `page`/`page_size`），行为无回退；但这属于「口径未定」，建议 BA 明确 |
| U-5 | 复验报告 §5 的 U-01~U-09（SSO 登录审计、D157 写失败 503、全租户 `xtask audit verify` 等） | 与本轮 F1~F10 无关，未重跑 | 本轮**未**声称它们已验证 |
| U-6 | `E2E` 只在 `chromium` 一个 project 上跑（与复验口径一致） | 仓内 playwright 配置如此 | 未覆盖其它浏览器 |

> **没有**「应该没问题」这类表述：以上 6 项均为**未做**或**未验**，已逐条给原因。

---

## 14. 一句话总结

F1~F9 **全部完成并实测通过**（含 F1/F2/F5 三处变异自证、E2E 从 31/4 变 35/0、clippy 新增 0、fmt 新增 0、
逐 crate 925 passed / 0 failed / 0 ignored）；

**F10 因裁决与源码事实冲突未落地**，冲突证据与三个收口选项已在 §12 呈交 BA 裁决。
