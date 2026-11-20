# REQ-014 修复轮任务卡（Dev 子 Agent）

- **需求**: REQ-014 审计日志
- **工作目录**: `/home/myg133/Projects/system_seed/feature-REQ-014`（分支 `feature/REQ-014`，HEAD `5f28c0c`）
- **触发**: Pre-merge 复验 **FAIL**（阻塞 2 / 高 1 / 中 3 / 低 4）
- **复验报告**: `/home/myg133/Projects/system_seed/BA/feature-meta/qa-premereq-req014.md`
- **裁决**: `BA/demands/REQ-014-audit/decisions.md` **rev 4**（**D184~D193**）
- **本卡由 BA（myg133-ba-1）于 2026-11-20 写下**
- ⚠️ **本卡的「钉死的改法」是 BA 逐条读源码后定的**，**不是建议** —— 不要另起方案。

---

## 0. 需求来源披露（D122）

REQ-014 的需求来源是**用户原话**「对，审计和国际化也是需要的。」（与 REQ-021 同源），不是 BA 自补。

---

## 1. 环境（铁律）

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
```

- `cargo test --workspace` 在本仓**必失败** → **逐 crate 跑**：
  `platform-core platform-db platform-iam platform-org platform-rbac platform-config platform-schema platform-server platform-audit sandbox xtask`
- `--all-targets` 不能省；**只能有一个** `--`；`--include-ignored` 在 `--` **之后**。
- 统计口径（**自己算，不许引用 Dev 旧报告**）：
  ```bash
  cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored \
    | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
  ```
- **串行**跑（8 线程 / 7.7G 低配机；`platform-org`/`platform-iam` 并发下有 CPU 竞争 flake）。
- 应用连接必须用 **`base_app`**（超管 `base` 带 `BYPASSRLS` → 用它验 RLS/append-only **必假绿**）。
- 本机**无 psql 客户端** → `podman exec base-postgres psql -U base -d base_test -tAc "…"`。
- **禁止 `cargo install`**；**禁止新增第三方依赖**（唯一例外是 REQ-013 批的 TLS，本轮无关）。
- **不要 `git commit`** —— Dev 不提交，由 BA 统一提交。

---

## 2. 本轮要修的 10 件事

### 🔴 F1 —— `nextCursor` 恒为 `null`（D184，阻塞）

- **现象**：任意 `limit` 下 `/api/audit/events` 的 `nextCursor` 永远是 `null` → 翻页功能不存在。
- **根因（BA 已读码复核）**：`crates/platform-audit/src/query.rs` 的主查询
  ```rust
  sql.push_str(&format!(" ORDER BY {} LIMIT ?", render_order(&params.sort)));
  binds.push(BindValue::Int(limit as i64));      // ← 绑的是 limit，不是 limit + 1
  ```
  下一行 `let has_more = rows.len() > limit;` —— 只取回 `limit` 行，`rows.len()`
  **永远不可能 > `limit`** → `has_more` 恒假 → `next_cursor` 恒 `None`。
  上一行注释「多取一条用来判断『还有下一页』」与 SQL **不符**。
- **钉死的改法**：主查询绑 **`limit + 1`**；`has_more` 在 `truncate(limit)` **之前**判定；
  `next_cursor` 取截断后最后一行的 `(created_at, id)`。
  ⚠️ `count_sql` **不需要**动（它不带 `LIMIT`）。
- **必须补的库级断言**（缺了就等于没修 —— 这是本条的重点）：
  1. `total > limit` ⟺ `nextCursor` 非 `null`；
     （`total` 是**带过滤**的总数：`count_sql` 复用同一 `clause` → 等价式成立）
  2. 连续两页的 `id` 集合**无交集**（用 `limit=2` 翻两页验证）；
  3. 末页 `nextCursor` 为 `null`（不足一页时）。

### 🔴 F2 —— 过滤子句拼不出合法 SQL（D185，阻塞）

- **现象**：任何带 `filter[...]` 的查询 → PG `trailing junk after parameter at or near "$1actor_id"`
  → HTTP **503 `AUDIT_SINK_UNAVAILABLE`**。
- **根因（BA 已读码复核）**：`render_filters` 只在过滤器**之间**补 `AND`
  ```rust
  if !clause.is_empty() { clause.push_str(" AND "); }   // ← 第一个过滤器没有 AND
  ```
  → 第一个过滤器贴死在 `WHERE tenant_id = {tenant}` 后面。
- **钉死的改法**：**`render_filters` 产出的每一段自带 ` AND ` 前缀**，
  调用点 `WHERE tenant_id = {tenant}{clause}` **保持不变**。
  - 为什么不选「调用点写 `AND {clause}`」：零过滤器时会拼出 `WHERE tenant_id = $1AND ` → **更坏**。
  - 一处改，两个调用点（`count_sql` 与主查询）**同时**修好。
- **必须补的库级断言**：五个 op（`eq` / `ne` / `in` / `contains` / `range`）**各一条打到真库**的用例，
  断言返回行数与 `id` 集合正确，**且 HTTP 状态不是 5xx**。
  ⚠️ 当前 `crates/platform-audit/tests/` 里 `filter[` **零命中** —— 这就是它当初假绿的直接原因。

### 🟠 F3 —— `audit.page.defaultSize` 登记值仍是 50（D186，高）

- **钉死的改法**（3 处，缺一不可）：
  1. `crates/platform-config/manifest.json` 里 `audit.page.defaultSize` 的 `default` → **20**；
  2. **删掉** label 里的补丁说明「（登记值；契约 C7 的默认口径仍是 20）」——
     D182 明文要求删（既然一致了，就不用解释差异）；
  3. `migrations/20261120000400_audit_config_seed.up.sql` 里
     `SELECT platform_config_upsert('audit.page.defaultSize', '50'::jsonb);` → **`'20'::jsonb`**。
- ⚠️ 迁移已进 `_sqlx_migrations` **不会重跑** → 必须同步既有行
  （`platform_config_upsert` 幂等，重跑即可）。

### 🟡 F4 —— `platform-config` 测试夹具清空整张 `platform_config` 表（D187，中）

- **根因**：`crates/platform-config/tests/common/mod.rs` 的 `reset_fixture` 里
  `DELETE FROM platform_config`（**整表**）→ 把 `audit.*` 的登记值也删了，
  而迁移不会重跑 → **没人补回**，登记值在共享测试库里静默消失。
- **钉死的改法**：改成**只删自己管的键**（`platform_fixture()` 里那 6 个：
  `logo` / `favicon` / `primaryColor` / `conflictStrategy` / `scimBaseUrl` / `cacheTtlSeconds`）。
- ⚠️ **必须检查**：有没有 platform-config 自己的用例依赖「整表为空」这个前提
  → 有的话**一并改**，并在报告里说明改了哪几条、为什么。
- ⚠️ **不要**加「启动自检 manifest↔DB 配置键」—— 那是平台级改动，已记 **TD-036**，不在本轮。

### 🟡 F5 —— org / config 两条链路缺真库端到端断言（D188，中）

- **定性**：**判据缺失**，**不是**生产链路有洞（BA 已核 `main.rs:320` 构造真实 `PgAuditSink`，
  并经 `build_org_state(..., audit_sink.clone())` 与 `ConfigService::with_audit_sink`
  接进生产链路 → 生产链路是通的）。
- **缺口**：org / config **没有**「事件真的进了 `platform_audit_log`」的机械断言
  （只有 mock sink 的形状断言）。
- **钉死的改法**：在 **`crates/platform-server/tests/`** 新增一条真库测试
  （platform-server 同时依赖 org / config / audit / db，**不需要**给 org / config 加 dev-dependency）：
  1. 构造**生产** `PgAuditSink`；
  2. 接进**生产适配器**：org 的 `PlatformSinkAuditRecorder::new(sink)`、
     config 的 `ConfigService::with_audit_sink(sink)`；
  3. 触发一次 org 写入 / 一次 config 写入；
  4. 查 `platform_audit_log` → 断言事件码 / `object_type` / `before`·`after` 形状**符合 D163**。
- ⚠️ **必须走生产适配器**，不许自己造一个等价实现（那是「验证生产代码的拷贝」）。
- ⚠️ 若发现某条链路**确实**无法在测试里驱动（如缺权限或缺入口），**不要伪造通过** ——
  在报告里如实写「未验」并说明原因。

### 🟡 F6 —— `query.rs` 模块注释与 D183 不一致（D189，中）

- `crates/platform-audit/src/query.rs` 顶部注释仍写「违反契约 C6 的 `application/problem+json` 要求」。
- **钉死的改法**：改成 D183 口径 ——
  「违反契约 C6 的 **Problem Details 形状**要求（Content-Type 跟随平台现状 `application/json`，D183）」。
  同一含义在 `crates/platform-server/src/audit.rs` 已经写对 → 与它对齐。

### 🟢 F7 —— `audit_chain.rs:1` 的 2 个 U+FFFD（D190，低）

- 应为「并发不**分**叉」，`分` 被写成了 2 个 U+FFFD（`ef bf bd` × 2）。
- 改成正常的「分」。

### 🟢 F8 —— clippy 新增 8 条 warning（D191，低）

- **钉死的改法**：
  1. `crates/platform-server/src/audit.rs` 的 `TokenVerifier::verify`
     从 `Result<VerifiedToken, ()>` 改成**带类型的错误** →
     新增 `TokenVerifyError`（**不引新依赖**）。
     与仓内惯例一致：`platform-rbac` 用 `RbacError`、`platform-schema` 用 `AuthError`；
     **只有** platform-server 的 audit 用 `()`。
     ⚠️ 影响面**仅限** `crates/platform-server/src/audit.rs`
     （`IamTokenVerifier` + 其 `FixedTokens` 测试实现）——
     其他 crate 的同名 trait **是各自独立的 trait**，不是同一个。
  2. 其余 7 条风格 lint 一次 `cargo clippy --fix` 修掉
     （其中 5 条在 `platform-config/src/service.rs` / `platform-iam/src/service.rs` /
     `platform-config/tests/audit_events.rs` / `platform-rbac/tests/audit_events.rs`）。

### 🟢 F9 —— 配置种子迁移的注释仍写「待 BA 确认」（D190，低）

- `migrations/20261120000400_audit_config_seed.up.sql` 里那句「待 BA 确认（要么改 D178 正文，
  要么改契约 C9）」→ 改成「D178 正文的 snake_case 写法**与契约 C9 冲突**，**D181 已裁定改用 camelCase**，
  D178 的 snake_case 写法**作废**」。

### 🟢 F10 —— `TITLES` 与 i18n 错误域只对计数（D192，低）

- **钉死的改法**：在 `crates/platform-server/tests/i18n_titles.rs` 补一条**集合相等**断言：
  `TITLES` 的 code 集合 **==** `packages/i18n/src/locales/en-US.json` 的 `error.*` 键集合。
  - `platform-server` 已有 `serde_json` 依赖 → **不引新依赖**；
  - 用 `env!("CARGO_MANIFEST_DIR")` 定位到仓库根再拼路径；
  - ⚠️ 现有断言是 `TITLES.len() == 84` + `titles_cover_every_code`（对 `ALL_CODES`）——
    **保留**，只是**再加**集合相等这条（计数相等会掩盖「少一个多一个」的互换）。

---

## 3. 硬约束

- ❌ **不许**改 `crates/platform-audit/src/query.rs` 之外的**契约相关**行为（比如别顺手改分页默认值、
  排序白名单、游标编码）—— 本轮只修上面 10 条。
- ❌ **不许**为了让测试变绿而放宽断言 / 改阈值 / 跳过用例 / 注释掉测试。
- ❌ **不许**改 `BA/demands/`、`BA/feature-meta/` 下的任何文件（那是 BA 的地盘）。
  你唯一要写的报告是 `BA/feature-meta/req-014-dev-fixround.md`（见 §5）。
- ❌ **不许** `git commit`。
- ✅ **必须**保留现有全部行为（B-01/B-02 的修复**不得**改变已有合法查询的返回内容）。
- ⚠️ 改 `platform-config` / `platform-iam` 的**测试夹具**时要确认**不破坏别的 crate 的用例**。

---

## 4. 必须跑的验证（报告里要有实际输出）

1. **逐 crate 全量**（11 个 crate，串行，`-- --test-threads=1 --include-ignored`）
   → 期望 **0 failed / 0 ignored**，并用 §1 的 awk 口径**自己算**总数。
   ⚠️ 复验者实测基线是 **912 passed**（core 153 / db 46 / iam 178 / org 93 / rbac 83 /
   config 88 / schema 20 / server 121 / audit 46 / sandbox 33 / xtask 51）——
   你加了新用例，总数**应该上升**；若下降 → 有用例被误删，必须查清。
2. **前端**：`pnpm --filter @base/shell test`（≥206）、`@base/i18n`（43）、`@base/sdk`（38）、
   `pnpm -r build`、`svelte-check`。
3. **E2E**：`npx playwright test e2e/` → 复验者实测 **31 passed / 4 failed**，
   那 4 条红**全部**由 F1/F2 造成 → **修完必须全绿**。
   ⚠️ **D193**：skip 的理由**只允许**「被测系统未就绪」；审计页的用例**必须真跑**，
   不许靠 skip 把红灯变绿。
4. **变异自证（关键）**：把 F1 / F2 的原缺陷**注入回去**，确认**新补的断言会变红**，
   然后还原并确认逐字节相同 —— 否则你补的断言可能仍然是假绿。
   ```bash
   cp -p <file> /tmp/x.bak
   sha256sum <file> > /tmp/sha-before.txt
   # 注入 → 跑对应测试 → 确认红
   cp /tmp/x.bak <file> && touch <file>     # 还原 + touch（cp -p 保留 mtime → cargo 复用陈旧二进制）
   sha256sum -c /tmp/sha-before.txt         # 必须逐字节相同
   git status --short                        # 必须干净
   ```
   - F1 的变异：把 `limit + 1` 改回 `limit` → 新断言必须红。
   - F2 的变异：把 `render_filters` 的 ` AND ` 前缀去掉 → 新断言必须红。
5. `cargo xtask capabilities --check`、`cargo clippy --workspace --all-targets`
   → **新增 warning 必须为 0**（基线 2 条存量不动）。
6. `cargo fmt --all --check` → 与基线 `develop@0683115` 比对**新增 0 处**
   （基线本来就红 11 个存量文件 —— TD-017，**只格式化你改过的文件**）。

---

## 5. 交付物

1. **代码改动**（在 feature worktree，**不提交**）。
2. **`/home/myg133/Projects/system_seed/BA/feature-meta/req-014-dev-fixround.md`** —— 报告：
   - 逐条 F1~F10：改了什么、`file:line`、为什么这么改（**不许**与裁决冲突）；
   - 你实测的逐 crate 数字 + 前端数字 + E2E 结果；
   - **变异自证**的证据链（F1、F2 各一处）；
   - **偏离声明**：凡与本卡不一致的地方，明确列出并说明理由（BA 会逐条裁决）；
   - **未完成/未验证项**：如实写，**不许**写「应该没问题」。
3. 收尾时 `git status --short` 必须**只含你改过的文件**，且 `git diff` 里**没有**调试残留、
   没有 `MUTATION` 标记。

---

## 6. 停止条件

- 某条修不动（环境限制 / 依赖前置）→ **不要伪造通过** → 写进报告的「未完成项」并说明。
- 发现本卡某条裁决与源码事实**冲突** → **停下来**，在报告里指出冲突并给证据，**不要**自己另定方案。
- 发现超出 F1~F10 的新缺陷 → **写进报告**，**不要**顺手改。

---

## 7. 回报格式

1. F1~F10 逐条：改了什么 + `file:line`；
2. 实测数字（逐 crate + 前端 + E2E）；
3. 变异自证证据（F1、F2）；
4. 偏离声明 / 未完成项；
5. 已写入的文件（绝对路径）。

---

# 附录 A（BA 于 2026-11-20 14:05 追加）：F10 的判据订正 + D195

> **F10 的原判据是 BA 写错的** —— D192 作废，由 **D194** 取代。
> 详见 `BA/demands/REQ-014-audit/decisions.md` rev 5 的「rev 5」段。

## A.1 为什么原判据不成立

`TITLES`（`crates/platform-core/src/titles.rs`）**混装**两类键：
**78 个错误码**（`UPPER_SNAKE`）+ **6 个组织事件码**（`org.*`）= 84；
i18n `error.*` 词库 = 那 78 个 + **6 个当前不进 HTTP 响应体**的码 = 84。

**两个 84 是数字巧合，集合差 6↔6。** 两侧的差集**都是有意**的：
- `TITLES` 的 6 个 `org.*` 是**组织事件码**（`titles.rs` 模块注释明写
  「组织事件码沿用既有 `org.*` 形式」）；前端无 org 命名空间，也不消费它们；
- i18n 多出的 6 个（`CONFIG_MISSING_REQUIRED` / `CONFIG_INVALID_VALUE` /
  `CONFIG_FORBIDDEN_COMBINATION` / `INVALID_CONFIG_KEY` / `INVALID_IDENTIFIER` /
  `IO_ERROR`）**当前不进 HTTP 响应体** → 它们在 `TITLES` 缺席是**正确的**。

→ **集合相等永远不成立**，「==」这条断言**不要写**。

## A.2 F10 的正确做法（钉死）

在 `crates/platform-server/tests/i18n_titles.rs` 里新增断言，判据 = D125 的真实不变量
「**每个会进响应体的错误码，`TITLES` 与 i18n 都要登记**」：

1. **（已有，保留）** `titles_cover_every_code` —— `TITLES` ⊇ `all_codes()`；
2. **（新增）** `all_codes()` 里**除组织事件码外**的每个码，i18n 词库都有 `error.<code>` 条目；
   - 组织事件码从哪来是**已知的**：该测试文件里 `all_codes()` 已经显式列了
     `platform_org::notify::OrgEventKind::ALL_CODES` 这一段 → 排除它即可，
     **不要**用正则去猜「哪些是事件码」；
3. **（新增）** `TITLES` 里的**错误码**（= `TITLES` 的键 ∩ 第 2 条的错误码全集）
   都在 i18n 词库里。

### A.2.1 三条断言合起来能抓住什么

- i18n 少一个**后端码** → 断言 2 红；
- i18n 把 A 换成 B（A 是后端码）→ 断言 2 红；
- `TITLES` 有个码 i18n 没译 → 断言 3 红。

### A.2.2 硬要求

- ⚠️ 断言 2 与 3 必须**同时查 `en-US.json` 与 `zh-CN.json`** ——
  只查英文的话，中文词库少一条就抓不到。
- ⚠️ `platform-server` 已有 `serde_json` 依赖 → **不引新依赖**；
  用 `env!("CARGO_MANIFEST_DIR")` 定位仓库根（`../../..`）再拼 `packages/i18n/src/locales/…`。
- ⚠️ **不要**改 `TITLES`（`crates/platform-core/src/titles.rs`）——
  那 6 个码**不该**补标题（见 A.1），补了就是「登记在骗人」的另一面。
- ⚠️ **不要**改 `crates/platform-server/tests/i18n_titles.rs` 里
  `the_title_table_has_the_pinned_number_of_rows` 的钉死值（仍是 84）——
  `TITLES` 一行没加。

## A.3 D195：`page` 模式下 `nextCursor` 的语义

- **决策**：`page` 模式下**继续返回** `nextCursor`（F1 的修法对两种模式一视同仁）。
  - 它在 `page` 模式下不是无意义的：它指向本页最后一行，配合默认排序时，
    用它做游标翻页与 `page+1` 等价；
  - 客户端若改用游标，必须回到**默认排序** —— 这条约束 D179 已经钉了
    （游标 + 非默认 `sort` → 400）。
- **不改代码**，只是把「C7 未定义 page 模式的 `nextCursor`」这件事**钉住**，
  免得下一轮又当成 bug 改一遍。

## A.4 本附录的其余确认（BA 已核实，不必再查）

- 复验报告里的 **U-2「`TITLES` 确实少 6 个码」** → **不是缺陷**：
  那 6 个码当前不进响应体，缺席是**正确的**（A.1）。→ 关闭，已记 **TD-038**。
- **U-3「生产库的 `audit.page.defaultSize` 行需部署时跑一次幂等 upsert」** → **不成立**：
  迁移 `20261120000400` 属于**本条未合并的功能分支**，**没有任何环境**应用过它
  → 新部署跑的就是 `'20'::jsonb`。→ 关闭。
