# REQ-021 独立复核报告 —— 修复轮 2 的 D119 复核

- **复核者**：Verifier（独立复核，**不采信** Dev 报告 / BA 记录 / 已有测试的自证）
- **复核时间**：2026-11-19 22:00 ~ 22:20（CST）
- **被审对象**：`/home/myg133/Projects/system_seed/feature-REQ-021`（分支 `feature/REQ-021`，**未提交**，改动在工作区）
- **唯一焦点**：**D119 —— 「平台配置层」是否真的加入 i18n 协商链**
- **对照规格**：`BA/demands/REQ-021-i18n-localization/acceptance.md`（rev 3，AC-S-088~099）、`design-summary.md`、`test-cases/`（97 条）、`REQ-001-platform-foundation/decisions.md`（D109~D120）
- **待验证声明**：`feature-meta/req-021-fix2.md`

---

## 0. 结论摘要

> **VERDICT: PASS-WITH-RISK**
>
> **D119 本身：判定为「真的修好了」** —— 8/8 判定点全部 PASS，其中 6 点由我**独立起真服务器 + 真库**取样得出（不依赖任何既有测试）。
> **PASS-WITH-RISK 的唯一原因**：**自动化测试没有覆盖「DB 里平台行 → `/api/i18n/context` 返回 `source=platform`」这条端到端链**（见 F-1，中危）。行为今天是对的（我实测过），但**下一次重构可以把它改坏而无人报警**。

| 维度 | 结果 |
|---|---|
| 8 个判定点 | **8 PASS / 0 FAIL** |
| 变异验证（M-A 平台层失效） | **6 条用例变红**（单元 3 / HTTP 2 / 真库 1）→ 测试**有牙齿** |
| 变异验证（M-B 平台层误报 tenant，我加做） | **5 条变红，但真库测试仍全绿** → 暴露 F-1 |
| 后端回归 | **637 passed / 0 failed / 0 ignored**（9 crate，逐 crate） |
| 前端回归 | **208 passed / 0 failed / 0 skipped**（i18n 43 + sdk 38 + shell 127） |
| E2E | **20 passed / 0 failed / 0 skipped** |
| 实现缺陷 | **0 个**（未发现任何功能性缺陷） |
| 测试覆盖缺陷 | **1 个中危**（F-1） |
| 规格/文档问题 | **4 个低危**（F-2~F-6，非 D119 引入） |
| 工作区状态 | **与进场逐行一致**（机械 diff 为空） |

---

## 1. 判定点逐条结论（8 点）

每一行的证据都是**我自己跑出来的**。命令与输出原文见 §3。

| # | 验什么 | 判定 | 我的独立证据（真库 + 真服务器） |
|---|---|---|---|
| 1 | 平台层真的生效 | **PASS** | 平台层写 `locale=en-US`、租户未配 → `/api/i18n/context` = `{"locale":"en-US",…,"source":"platform"}` |
| 2 | 租户仍覆盖平台 | **PASS** | 平台 `en-US` + 租户 `zh-CN` → `{"locale":"zh-CN",…,"source":"tenant"}`（时区/币种同步为 tenant） |
| 3 | 三个字段都修了 | **PASS** | 平台层 `locale/timezone/currency` 三键同写 → 响应三字段全部来自平台层，`sources` 三项全为 `"platform"` |
| 4 | `source` 如实反映层级 | **PASS** | **只有平台配** → `source == "platform"`（**不是** `tenant`、不是 `default`）；并用变异 M-B 证明「误报成 tenant」会立刻让 5 条用例变红 |
| 5 | 请求头仍压过平台层 | **PASS** | `Accept-Language: zh-CN` + 平台 `en-US` → `{"locale":"zh-CN","source":"accept-language"}` |
| 6 | 用户偏好仍最优先 | **PASS** | `?locale=zh-CN` + 平台 `en-US` → `{"locale":"zh-CN","source":"user"}`；叠加 `Accept-Language: en-US` 仍是 `user` |
| 7 | `source` 是 5 值且序列化正确 | **PASS** | 线上 JSON 实测为**小写字符串** `"platform"`（既非 `"Platform"`，也非 `1`）；单元测试另钉死 5 值集合与 serde |
| 8 | 旧行为没被破坏 | **PASS** | 什么都没配 → `{"locale":"zh-CN","timezone":"Asia/Shanghai","currency":"CNY","source":"default"}`，三个 `sources` 全为 `"default"` |

**M1 的原始症状已消除**：同一个键在两处现在给**同一个答案**。

```
/api/i18n/context      → {"locale":"en-US",…,"source":"platform",…}
/api/config/entries    → {"entries":[{"key":"locale","source":"platform","value":"en-US"},…]}
```

---

## 2. 变异验证（假绿自查）

### 2.0 方法与一个必须记录的方法论坑

- 判定点：`crates/platform-server/src/i18n.rs:143` 的
  `ConfigSource::Platform => Some(LocaleSource::Platform),`
- 备份：`cp -p crates/platform-server/src/i18n.rs /tmp/req021-verify/i18n.rs.orig`
  （该文件是 **untracked** → `git checkout` **无法**还原，备份是唯一还原手段）
- 进场 sha256：`b25eb0a576ce206aba6f68b268e4f1d52f55d2e6534e650b2aceb137624f54c2`

> ⚠️ **方法论坑（我自己踩了，值得记录）**：`cp -p` 会**保留旧 mtime**。还原后第一次重跑仍然全红 —— 不是没还原，而是 **cargo 依据 mtime 判定源文件「未变」，直接复用了含变异的陈旧二进制**。
> 加上 `touch` 强制重编后才恢复全绿。**任何「变异 → 还原 → 重跑」的流程都必须 bump mtime**，否则会得出「还原失败」或「测试是红的」的错误结论。

### 2.1 变异 M-A：平台层判定退回 `None`（= D119 修复前的行为）

```diff
-            ConfigSource::Platform => Some(LocaleSource::Platform),
+            ConfigSource::Platform => None,  // MUTATION-A: 平台层失效
```

`cargo test -p platform-server --all-targets --no-fail-fast -- --test-threads=2 --include-ignored` 的实际结果：

```
     Running unittests src/lib.rs
test i18n::tests::currency_has_tenant_platform_and_default_tiers ... FAILED
test i18n::tests::d119_each_config_layer_maps_to_its_own_source ... FAILED
test i18n::tests::tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it ... FAILED
test result: FAILED. 58 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out

     Running tests/i18n_api.rs
test tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it ... FAILED
test the_platform_layer_still_loses_to_the_accept_language_header ... FAILED
test result: FAILED. 15 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out

     Running tests/i18n_settings_real_db.rs
test the_platform_layer_takes_part_in_the_negotiation_and_the_tenant_wins_over_it ... FAILED
test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

**6 条用例变红，横跨 3 个测试目标（单元 / HTTP / 真库）** → **测试有牙齿，不是假绿**。

失败断言原文（节选）：

```
thread 'i18n::tests::tc021_096_...' panicked at crates/platform-server/src/i18n.rs:1433:9:
assertion `left == right` failed: 平台级的语言必须生效
  left: "zh-CN"
  right: "en-US"

thread 'tc021_096_...' panicked at crates/platform-server/tests/i18n_api.rs:436:5:
assertion `left == right` failed: 平台级的语言必须生效
  left: String("zh-CN")

thread 'the_platform_layer_takes_part_in_the_negotiation_and_the_tenant_wins_over_it'
panicked at crates/platform-server/tests/i18n_settings_real_db.rs:299:9:
locale 来自平台层 = 显式配置
```

> **注意真库那条红在哪一行**：`i18n_settings_real_db.rs:299` 是
> `assert!(setting.is_explicit(), "{key} 来自平台层 = 显式配置")` ——
> 它**不是**在断言协商结果，而是在断言 `is_explicit()`。这一点在 M-B 下会暴露（见 2.2）。

### 2.2 变异 M-B（我加做）：平台层被**误报成 `tenant`**

> 动机：判定点 4 明确要求「**特别查**：平台层的值有没有被误报成 `tenant`？那等于把 M1 换了个形式」。Dev 的报告只做了「退回 `None`」，**没做**这个方向的变异。

```diff
-            ConfigSource::Platform => Some(LocaleSource::Platform),
+            ConfigSource::Platform => Some(LocaleSource::Tenant),  // MUTATION-B: 平台层误报成 tenant
```

实际结果：

```
     Running unittests src/lib.rs
test i18n::tests::d119_each_config_layer_maps_to_its_own_source ... FAILED
test i18n::tests::currency_has_tenant_platform_and_default_tiers ... FAILED
test i18n::tests::tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it ... FAILED
test result: FAILED. 58 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out

     Running tests/i18n_api.rs
test tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it ... FAILED
test the_platform_layer_still_loses_to_the_accept_language_header ... FAILED
test result: FAILED. 15 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out

     Running tests/i18n_settings_real_db.rs
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   ← ★ 仍然全绿
```

失败断言原文（节选）：

```
thread 'i18n::tests::d119_each_config_layer_maps_to_its_own_source' panicked at crates/platform-server/src/i18n.rs:1369:9:
assertion `left == right` failed
  left: Some(Tenant)

thread 'tc021_096_...' panicked at crates/platform-server/tests/i18n_api.rs:437:5:
assertion `left == right` failed: 如实报出是平台层生效
  left: String("tenant")
```

> ★ **这是本轮复核最重要的发现（F-1）**：
> **真库测试在 M-B 下仍然全绿** —— 因为它断言的是**配置层**（`Setting.value` / `Setting.source` / `is_explicit()`），
> **从未调用过协商函数**。它测的是「解析器读到了平台层的值」，
> **不是**「协商把这个值以 `source=platform` 报出去」。
>
> 叠加 HTTP 层测试用的是**内存伪造**的 `FakeSettings`（`tests/i18n_api.rs:99-108` 直接手写
> `Setting { source: ConfigSource::Platform }`），
> ⇒ **自动化套件里不存在任何一条「真库平台行 → 真 HTTP 响应 `source=platform`」的用例。**

### 2.3 还原

```bash
cp -p /tmp/req021-verify/i18n.rs.orig crates/platform-server/src/i18n.rs
sha256sum crates/platform-server/src/i18n.rs
# → b25eb0a576ce206aba6f68b268e4f1d52f55d2e6534e650b2aceb137624f54c2   ← 与进场完全一致
diff -u /tmp/req021-verify/i18n.rs.orig crates/platform-server/src/i18n.rs   # → IDENTICAL
grep -rn "MUTATION-" crates/                                             # → NO MUTATION MARKERS LEFT
touch crates/platform-server/src/i18n.rs   # ← 强制 cargo 重编（见 2.0）
```

还原后重跑：

```
     Running unittests src/lib.rs        test result: ok. 61 passed; 0 failed; 0 ignored
     Running unittests src/main.rs       test result: ok.  8 passed; 0 failed; 0 ignored
     Running tests/i18n_api.rs           test result: ok. 17 passed; 0 failed; 0 ignored
     Running tests/i18n_no_chinese_titles.rs  test result: ok. 6 passed; 0 failed; 0 ignored
     Running tests/i18n_settings_real_db.rs   test result: ok. 3 passed; 0 failed; 0 ignored
     Running tests/i18n_titles.rs        test result: ok.  6 passed; 0 failed; 0 ignored
```

**platform-server 合计 101 passed / 0 failed** —— 恢复全绿。

---

## 3. 独立端到端取样（真服务器 + 真库）

**为什么还要做这一层**：Dev 的 HTTP 测试用内存替身、真库测试只测配置层 ——
**两者之间的接缝没人测**。我自己把整条链跑通，作为不依赖任何既有测试的独立证据。

### 3.1 环境

```bash
# 真服务器：base_app 普通角色 + 临时密钥 + 独立端口 8099
export DATABASE_URL="postgres://base_app:base@127.0.0.1:5432/base_test"
export BASE_MANIFEST_ROOT="."  BASE_STATIC_DIR="apps/shell/dist"  SERVER_PORT=8099
export BASE_JWT_PRIVATE_KEY="$(openssl ecparam -name prime256v1 -genkey -noout | openssl pkcs8 -topk8 -nocrypt)"
export BASE_IAM_SECRET_KEY="$(head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=')"
/home/myg133/Projects/system_seed/code/target/debug/base serve
# → GET /health/live = 200
```

夹具租户 `tenant-b`：**只有 `logo` 一行**，没有任何 i18n 配置（进场时即是如此）：

```
 tenant_id | key  |          value
-----------+------+---------------------------
 tenant-b  | logo | "/branding/tenant-b.svg"
```

登录（真实 IAM）：`POST /api/iam/login` → `HTTP=200`，拿到 JWT。

### 3.2 取样输出原文

**PROBE 0 —— 什么都没配（判定点 8）**

```
=== platform_config i18n rows ===  (0 rows)
GET /api/i18n/context
{"locale":"zh-CN","timezone":"Asia/Shanghai","currency":"CNY","source":"default","sources":{"locale":"default","timezone":"default","currency":"default"}}
GET /api/config/entries?keys=locale
{"entries":[{"key":"locale","source":"default","value":"zh-CN"}]}
```
→ **判定点 8 PASS**，且两处一致。

**PROBE 1 —— 平台层写 3 个键，租户未配（判定点 1 / 3 / 4 / 7）**

```sql
INSERT INTO platform_config (key, value, updated_at) VALUES
  ('locale','"en-US"', now()), ('timezone','"America/New_York"', now()), ('currency','"USD"', now())
ON CONFLICT (key) DO UPDATE SET value=EXCLUDED.value, updated_at=now();
UPDATE platform_config_epoch SET epoch=epoch+1 WHERE scope_key='platform';
```

```
GET /api/i18n/context
{"locale":"en-US","timezone":"America/New_York","currency":"USD","source":"platform","sources":{"locale":"platform","timezone":"platform","currency":"platform"}}

GET /api/config/entries?keys=locale,timezone,currency
{"entries":[{"key":"locale","source":"platform","value":"en-US"},{"key":"timezone","source":"platform","value":"America/New_York"},{"key":"currency","source":"platform","value":"USD"}]}
```
→ **判定点 1 / 3 / 4 / 7 PASS**；**M1 的「同键两答案」症状消除**（两处现在都是 `platform` + 同一个值）。

**PROBE 2 —— 请求头压平台（判定点 5）**

```
Accept-Language: zh-CN  （平台 en-US）
{"locale":"zh-CN","timezone":"America/New_York","currency":"USD","source":"accept-language","sources":{"locale":"accept-language","timezone":"platform","currency":"platform"}}

Accept-Language: en-US  （平台 en-US）
{"locale":"en-US",…,"source":"accept-language",…}
```
→ **判定点 5 PASS**：平台层没有插队；且 `sources.timezone` 仍是 `platform`（时区不看请求头，符合 D114）。

**PROBE 3 —— 用户偏好压一切（判定点 6）**

```
?locale=zh-CN （平台 en-US）
{"locale":"zh-CN","timezone":"America/New_York","currency":"USD","source":"user","sources":{"locale":"user","timezone":"platform","currency":"platform"}}

?locale=zh-CN + Accept-Language: en-US
{"locale":"zh-CN",…,"source":"user",…}

?timezone=Europe/London （平台 America/New_York）
{"locale":"en-US","timezone":"Europe/London","currency":"USD","source":"platform","sources":{"locale":"platform","timezone":"user","currency":"platform"}}
```
→ **判定点 6 PASS**（语言与时区两级用户偏好都对）。

> **自查披露**：我第一次跑 PROBE 3 时把 `?locale=zh-CN` 作为**独立 argv** 传给 curl，
> curl 把它当成第二个 URL，于是实际请求**不带查询串**，得到 `en-US / platform`，
> 看起来像「点 6 FAIL」。复查后确认是**我的探针 bug**，改用完整 URL 后结果为 `zh-CN / user`。
> 记录在此以免后续复核者踩同一个坑。

**PROBE 4 —— 租户覆盖平台（判定点 2）**

```sql
INSERT INTO platform_tenant_config (tenant_id,key,value,updated_at) VALUES
  ('tenant-b','locale','"zh-CN"',now()),('tenant-b','timezone','"Europe/London"',now()),('tenant-b','currency','"JPY"',now())
ON CONFLICT (tenant_id,key) DO UPDATE SET value=EXCLUDED.value, updated_at=now();
UPDATE platform_config_epoch SET epoch=epoch+1 WHERE scope_key='tenant:tenant-b';
```

```
GET /api/i18n/context
{"locale":"zh-CN","timezone":"Europe/London","currency":"JPY","source":"tenant","sources":{"locale":"tenant","timezone":"tenant","currency":"tenant"}}

+ Accept-Language: en-US
{"locale":"en-US","timezone":"Europe/London","currency":"JPY","source":"accept-language",…}
```
→ **判定点 2 PASS**：平台 `en-US` 被租户 `zh-CN` 压过，`source=tenant`，三个字段一致。

**PROBE 5 —— 观察项（未注册值）→ 见 F-3**

```
租户 locale 改成 "ja-JP"（形状合法但**未注册**）
GET /api/i18n/context       → {"locale":"zh-CN",…,"source":"default",…}
GET /api/config/entries     → {"entries":[{"key":"locale","source":"tenant","value":"ja-JP"}]}
```

### 3.3 取样后清理（我写的数据已全部撤销）

```sql
DELETE FROM platform_config WHERE key IN ('locale','timezone','currency');
DELETE FROM platform_tenant_config WHERE tenant_id='tenant-b' AND key IN ('locale','timezone','currency');
DELETE FROM platform_iam_user WHERE id='verify-d119-user';   -- 以及 session / refresh_token / login_attempt
```
清理后核对：`platform_config` 无 i18n 行、`tenant-b` 只剩 `logo`、`verify-d119-user` 计数 = 0。
验证用服务器实例已停止（8099 端口连接被拒绝）。

---

## 4. 整体回归数字（我自己跑的）

### 4.1 后端（逐 crate，`--all-targets -- --test-threads=2 --include-ignored`）

| crate | passed | failed | ignored | exit |
|---|---|---|---|---|
| platform-core | 127 | 0 | 0 | 0 |
| platform-config | 83 | 0 | 0 | 0 |
| platform-iam | 58 | 0 | 0 | 0 |
| platform-rbac | 78 | 0 | 0 | 0 |
| platform-org | 91 | 0 | 0 | 0 |
| platform-db | 46 | 0 | 0 | 0 |
| platform-schema | 20 | 0 | 0 | 0 |
| platform-server | 101 | 0 | 0 | 0 |
| sandbox | 33 | 0 | 0 | 0 |
| **合计** | **637** | **0** | **0** | 全部 0 |

`platform-server` 分目标原文：`61 + 8 + 17 + 6 + 3 + 6 = 101`。

**跨 crate 顺序复查**（`platform-server` 之后立刻跑 `platform-config`，防止平台层行污染）：

```
platform-server ok-targets: 6
platform-config passed=83 failed=0 ignored=0
```

### 4.2 前端（vitest，不是 `bun test`）

| 包 | 结果 |
|---|---|
| `@base/i18n` | **43 passed (43)** |
| `@base/sdk` | **38 passed (38)** |
| `@base/shell` | **127 passed (127)** |
| **合计** | **208 passed / 0 failed / 0 skipped** |

### 4.3 E2E（`npx playwright test e2e/`）

```
20 passed (32.7s)
```
→ **20 passed / 0 failed / 0 skipped**（`e2e/i18n.spec.ts` 3 条 + `schema-driven` 6 条 + `shell-auth` 9 条 + `shell` 2 条）。

---

## 5. 发现的问题

### 中危

#### F-1（测试覆盖缺口）：**没有任何一条自动化用例覆盖「DB 平台行 → `/api/i18n/context` 返回 `source=platform`」这条端到端链**

- **触发条件**：任何使「配置解析结果」与「协商结果」之间断链的改动，例如
  `ConfigService::settings` 把 `ConfigSource::Platform` 映射成别的值、
  `context` handler 取错 `Setting` 字段、或 `layer_source()` 与 `Setting.source` 解耦。
- **影响**：D119 的**唯一存在理由**（D119：「通用配置路由与 i18n 协商对同一个键给出不同答案 = 不一致」）
  会**静默回归**，而 CI 全绿。M1 当初就是这样被漏掉的。
- **证据**：
  - 变异 M-B（`ConfigSource::Platform => Some(LocaleSource::Tenant)`）下，
    `tests/i18n_settings_real_db.rs` 目标 **`test result: ok. 3 passed; 0 failed`** —— 真库测试**没有牙齿**；
  - 真库测试断言的是 `setting.value` / `setting.source`（**配置层**）与 `is_explicit()`，
    **从未调用 `negotiate_locale` / `negotiate_timezone` / `resolve_currency`**；
  - HTTP 层的 `tests/i18n_api.rs` 用**内存伪造**的 `FakeSettings`
    （`i18n_api.rs:99-108` 直接手写 `Setting { source: ConfigSource::Platform }`），**不碰数据库**。
- **违反**：TC-021-096（`test-cases/locale-negotiation.json`）的步骤原文
  「**以租户 C 身份 GET /api/i18n/context** … 读响应里的 locale 与 source」，
  `layer: rust-integration`；以及 AC-S-090 步骤 9。
- **根因**：D119 的用例被拆成两半（配置层 / 路由层），**接缝（真实 `ConfigService` 装进真实 router）留空**。
- **修法建议**（不改实现，只补测试）：
  在 `crates/platform-server/tests/` 增加一条集成用例 —— 用**真实库**建 `ConfigService`
  （照抄 `i18n_settings_real_db.rs` 的 `setup()`），把它装进 `I18nState.settings`，
  用 `tower::ServiceExt::oneshot` 驱动**真实路由**打 `/api/i18n/context`，
  断言 `body["locale"] == "en-US"` 且 `body["source"] == "platform"`；
  再写一行租户级配置重打一次，断言 `body["source"] == "tenant"`。
  这条用例在 M-B 下必须变红（否则仍无牙齿）。
- **注意**：**这不阻塞合并**（行为今天是对的，我已实测），但**建议在合并前补上** ——
  否则下一次改配置层的人没有任何护栏。

### 低危

#### F-2（**规格问题**，非实现缺陷）：`design-summary.md` 的 `source` 取值集合自相矛盾

- `design-summary.md` §3.3 仍写：`source ∈ { user, accept-language, tenant, default }`（**4 值**），
  时区那条仍写 `source ∈ { user, tenant, default }`；
  而**同一份文件** §3.1（L292~L305）与 §3.4（L354）**已按 D119 更新**，
  L305 明写「`source` 取值集合从 4 值扩到 **5 值**（`+ platform`）」。
- 文件头仍写「**rev 2** —— 按 42 条裁定重写」（本次任务简报称其为 rev 3）。
- **影响**：规格文档自身对同一个契约给了两个答案；后续读者按 §3.3 写代码会少一个取值。
- **违反**：D119「`source` 集合扩为 5 值」；AC-S-090 rev 3 观测通道段（L141）。
- **归属**：**BA 的规格产物**，不是 Dev 的实现缺陷 —— 明确标注为规格问题，不计入实现缺陷清单。
- **修法建议**：BA 把 §3.3 的两个取值集合改成 5 值 / `{user, tenant, platform, default}`，并更新文件头版本号。

#### F-3（**规格问题**，非 D119 引入）：未注册的 locale 会被协商层丢弃，两条路由再次给出不同答案

- **触发条件**：租户写入**形状合法但未注册**的 locale（如 `ja-JP`）。
  写路径只校验**形状**（`crates/platform-config/src/registry.rs:100-113` 的 `is_locale_tag` 只判 BCP-47 形状），
  协商层则只接受**已注册**（`packages/i18n/src/locales/` 目录文件名，当前仅 `zh-CN` / `en-US`）。
- **实测（PROBE 5）**：
  ```
  GET /api/i18n/context    → {"locale":"zh-CN",…,"source":"default"}
  GET /api/config/entries  → {"entries":[{"key":"locale","source":"tenant","value":"ja-JP"}]}
  ```
  ⇒ **同一个键，两处答案不同** —— 与 M1 **同类**（只是触发条件从「层级」变成「值」）。
- **是否为 D119 引入**：**不是**。D119 之前租户层同样要求「已注册才算数」，行为一致。
- **归属**：**规格问题**。AC-S-093 步骤 4 / D111 只要求**非法（畸形）值**被 400 拒，
  没有任何一条说「写路径必须拒绝合法但未注册的值」；design §3.2 又禁止返回未注册 locale。
  ⇒ 这是**规格没有裁定的缝隙**，需 BA 裁定（写路径要不要按已注册集合收口，或接受该不一致并写明）。
- **修法建议（需 BA 先裁定）**：二选一 —— (a) 写路径增加「已注册」校验（未注册 → 400）；
  (b) 在 AC/设计里写明「配置路由报**存储值**、协商路由报**生效值**，二者本就可能不同」。

#### F-4（**规格问题**，低）：TC-021-097 / AC-S-090 步骤 10 用的 `ja-JP` 在本仓**不是已注册语言**

- `packages/i18n/src/locales/` 实测只有 `en-US.json` 与 `zh-CN.json`。
  而 design §3.2 规定「不允许返回未注册的 locale」→ **生产环境里租户配 `ja-JP` 必然回落到 manifest 默认**
  （PROBE 5 实测 `zh-CN / default`）。
- Dev 的 HTTP 用例通过在测试态**额外注册** `ja-JP` 来满足该期望
  （`tests/i18n_api.rs:162-174` 的 `state_with_locales(..., &["zh-CN","en-US","ja-JP"])`，注释也写明「真实产品只注册了 zh-CN / en-US」）。
- **影响**：AC-S-090 步骤 10 / TC-021-097 的字面期望（`locale == "ja-JP"`）**在生产不可达**；
  用例本身没错，但**规格该用已注册语言重述**（例如：平台 `en-US` + 租户 `zh-CN` → `zh-CN` / `tenant`）。
- **归属**：**规格问题**（BA 侧），不计入实现缺陷。

#### F-5（流程 / 归属，低）：任务简报里「BA 已独立跑出 637」与磁盘上的 BA 记录**对不上**

- `BA/demands/REQ-021-i18n-localization/ba-verification.md`（时间戳 **21:15**）记录的是
  **合计 630 / platform-server 94** —— 那是 **fix2 之前**的基线，**不是**对 637 的背书。
  该文件结论写的是「可以进入合并流程，但必须先过 QA Pre-merge」。
- **637 这个数字由我本次独立跑出**（§4.1），不是由那份 BA 文件担保的。
- 另：该 BA 记录的 §7「语言协商逐场景」只覆盖 4 个场景，**没有平台层场景** —— 与 M1 的存在一致。

#### F-6（测试卫生，低）：真库夹具在**租户级**留下 3 行残留

- `crates/platform-server/tests/i18n_settings_real_db.rs:138-152` 的 `clear_platform_i18n()` 只清**平台级**行；
  用例收尾（L345-347）也只调它。**租户级**行（`tenant-i18n-settings` 的
  `locale=zh-CN` / `timezone=Europe/London` / `currency=JPY`）会留在共享库 `base_test` 里。
- 实测（我跑完回归后）：`platform_tenant_config` 里确有这 3 行。
- **影响**：**自愈**（`setup()` 每次开头都删这 3 行），不影响其它用例；
  但共享库留下状态，后续若有**不做自清理**的用例可能踩到。
- **修法建议**：在用例收尾一并删除租户级行（`DELETE FROM platform_tenant_config WHERE tenant_id=$1`）。

---

## 6. 我**没有**验证 / 无法验证的项

| 项 | 原因 |
|---|---|
| `cargo fmt --all --check` | 已知基线红（TD-017，8 个存量文件）—— 按任务说明**不报为缺陷**，本次也未作为验收项运行 |
| `packages/schema` 的 10 处中文文案 | D117 / TD-015 —— **本轮不验收**（未检查） |
| 金额 JSON 2^53 上限 | D118 / TD-016 —— 已知取舍（未检查） |
| 空间层 `ConfigSource::Space` 的可达性 | 任务说明已判定「空间层写不了这三个 `scope: tenant` 的键」→ 不可达，未专门构造；`layer_source()` 对 `Space` 的分派由单元测试 `d119_each_config_layer_maps_to_its_own_source` 覆盖 |
| 前端 `I18nContextPayload.source` 对新值的兼容性 | 按任务说明为 `readonly source?: unknown` 且前端不按值分支；前端 208 条测试全绿，未另行写类型探针 |
| `base_dev` 库 | 全程只写 `base_test`（真实库用例自带「库名必须恰好是 base_test」的断言，我的取样也只写 `base_test`） |

---

## 7. 工作区状态确认

### 7.1 我动过又还原的文件（**仅 1 个实现文件**）

| 文件 | 改动 | 还原方式 | 还原后校验 |
|---|---|---|---|
| `crates/platform-server/src/i18n.rs` | 两次**临时**变异（M-A / M-B），各改 1 行 | `cp -p /tmp/req021-verify/i18n.rs.orig` + `touch`（强制 cargo 重编） | sha256 = `b25eb0a5…54c2`，与**进场**完全一致；`diff` 为空；`grep -rn "MUTATION-" crates/` 无残留 |

> 该文件是 **untracked**（`?? crates/platform-server/src/i18n.rs`），
> 因此 `git checkout` / `git restore` **无法**还原它 —— 我的还原完全依赖 `/tmp/req021-verify/i18n.rs.orig`。

**未改动任何其它实现代码**；未 `git commit` / `push` / `stash` / `reset` / `restore`。

### 7.2 机械比对（不是肉眼）

进场时 `git status --porcelain=v1` 与收尾时**逐行 diff**：

```
$ diff /tmp/req021-verify/entry_status.txt /tmp/req021-verify/final_status.txt && echo "IDENTICAL"
✅ WORKSPACE IDENTICAL TO ENTRY STATE
entry lines: 77 / final lines: 77
```

→ **除本报告文件（`feature-meta/recheck-req021.md`，任务明确要求的交付物）外，工作区与进场完全一致。**

### 7.3 验证用临时产物（**全部在仓库外**）

| 路径 | 内容 | 状态 |
|---|---|---|
| `/tmp/req021-verify/i18n.rs.orig` | `i18n.rs` 备份 | 保留（便于后续复核者复查） |
| `/tmp/req021-verify/entry_status.txt` | 进场 `git status` | 保留 |
| `/tmp/req021-verify/final_status.txt` | 收尾 `git status` | 保留 |
| `/tmp/req021-verify/login.json`、`token.txt` | 取样用 JWT | 已随验证实例停用而失效 |
| 验证服务器实例（端口 8099） | `base serve` | **已停止**（连接被拒绝） |

### 7.4 测试库 `base_test` 的改动与清理

| 对象 | 我做了什么 | 收尾状态 |
|---|---|---|
| `platform_config` | 写入 `locale` / `timezone` / `currency` 三行 | **已删除**（核对：0 行） |
| `platform_tenant_config`（`tenant-b`） | 写入 3 行 | **已删除**（`tenant-b` 只剩 `logo`，与进场一致） |
| `platform_iam_user` | 写入 `verify-d119-user`（tenant-b） | **已删除**（核对：0 行） |
| `platform_iam_session` / `refresh_token` / `login_attempt` | 登录产生 | **已删除** |
| `platform_config_epoch` | 推进（平台 / tenant:tenant-b） | 保留递增值（缓存失效计数器，非语义状态；测试套件每次运行也会推进） |
| `tenant-i18n-settings` 的 3 行 | **不是我写的** —— 是真库夹具的历史残留 | 未动（见 F-6） |

---

## 8. 最终结论

> ### VERDICT: **PASS-WITH-RISK**
>
> **D119 判定为「真的修好了」**：
> - 8 个判定点 **8 PASS / 0 FAIL**，其中 6 点由我**独立起真服务器 + 真库**取样验证，不依赖任何既有测试；
> - 变异 M-A 让 **6 条用例**（跨单元 / HTTP / 真库 3 个目标）变红 → **测试有牙齿，不是假绿**；
> - 变异 M-B（我加做，Dev 未做）让 **5 条**变红 → **「平台层被误报成 tenant」这个方向也被覆盖**；
> - M1 的原始症状（同键两答案）**已消除**：两处现在给同一个答案；
> - 整体回归 **637 + 208 + 20 全绿，0 failed / 0 ignored / 0 skipped**；
> - Dev 报告 §6.2 / §6.3 的**每一个数字我都能复现**，未发现任何夸大或未兑现的声明。
>
> **唯一的风险（PASS-WITH-RISK 的来源）**：
> **F-1 —— 自动化套件里没有一条「真库平台行 → 真 HTTP `source=platform`」的端到端用例**。
> 真库测试在变异 M-B 下**仍然全绿**。今天的行为是对的（我实测过），
> 但**下一次改配置层或协商层的人没有护栏**，而这正是 M1 当初被漏掉的那一类缺口。
>
> **建议**：合并前补 F-1 的那条集成用例（做法见 §5）。若决定不补，
> 请在 QA 记录里**显式登记**这一条已知缺口，避免下一轮把它当成「已覆盖」。

### 附：与任务简报的两处偏差（如实记录）

1. 「BA 已独立跑出 637」—— 磁盘上的 `ba-verification.md`（21:15）记的是 **630 / platform-server 94**，
   即 **fix2 之前**的基线；**637 由我本次独立跑出**（见 F-5）。
2. 任务简报称 `design-summary.md` 为 rev 3 —— 文件头仍是 **rev 2**，且 §3.3 未随 D119 更新（见 F-2）。
