# QA Pre-merge 验证报告：REQ-021 国际化与本地化

- **需求**：REQ-021（i18n / 时区 / 币种）
- **被验对象**：`/home/myg133/Projects/system_seed/feature-REQ-021`，分支 `feature/REQ-021`，**未提交**（改动全在工作区）
- **验收标准**：`BA/demands/REQ-021-i18n-localization/acceptance.md`（rev 2，AC-S-088 ~ AC-S-099 共 12 条）
- **设计摘要**：同目录 `design-summary.md`（rev 2）
- **用例集**：同目录 `test-cases/`（95 条，TC-021-001~095）
- **QA 判定时间**：2026-11-19
- **结论**：**PASS-WITH-RISK**（12 条 AC 全部通过；1 条中等**规格**问题需 BA 裁定；6 条低severity 观察项；**0 条必须修才能合并的实现缺陷**）

> **本报告的纪律**：Dev 的三份报告（`req-021-dev-backend.md` / `req-021-dev-frontend.md` / `req-021-fix1.md`）
> 一律当作**待验证的声明**。下面每条结论都有**我自己跑出来的命令与输出原文**；
> 凡是我没有亲自跑过的，一律在「未覆盖」里写明。
> 我**没有修改任何实现代码**，没有 `git commit`；临时脚本全部在 `/tmp/qa021/`，未留在仓库内。
> 对 `base_test` 库做过写入（仅测试夹具与恢复），未触碰 `base_dev`；结束前已把库恢复到进入时的状态。

---

## 0. 环境与可复现前提

```bash
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
cd /home/myg133/Projects/system_seed/feature-REQ-021
```

工具链实测：`cargo 1.98.1 (797e8a9bc)` / `rustc 1.98.1` / `node v26.13.1` / `pnpm 12.19.0`；
`podman ps` → `base-postgres Up (healthy) 127.0.0.1:5432`、`base-redis Up`。

> **⚠️ 我第一次跑 Rust 测试时忘了导出 `DATABASE_URL_TEST`**，结果 3 个套件红了 27 条。
> 逐条看下来全部是同一个 panic：
> `缺少 DATABASE_URL_TEST：真实库用例必须真的连库跑，不能静默跳过。`
> —— **这本身是个正面信号**：这些用例**拒绝 skip**，不会拿「静默跳过」冒充通过。
> 补上环境变量后全部通过，下面的数字是补齐环境后的。

---

## 1. 12 条 AC 的逐条判定

| AC | 判定 | 核心证据（我跑的） |
|---|---|---|
| AC-S-088 后端错误体零中文 | **PASS** | TITLES 58 行全 ASCII（我读全表 + 仓库 3 条测试）；**我自写扫描器**扫 119 个非测试 .rs 文件 → 253 处中文命中全在日志/启动期文案；仓库词法级扫描器 145 文件 / modules 16 文件 / **命中 0**；运行时抓 **8 类**真实错误体，`title/detail/type/instance/code` **零 CJK** |
| AC-S-089a params 完整 | **PASS** | 403 实测 `params.required="rbac:role:read"`；`LOGIN_RATE_LIMITED` 的 `retryAfterSeconds` 与前端占位符逐字对齐；6 类 code 全非空且 `^[A-Z][A-Z0-9_]*$` |
| AC-S-089b 前端 ApiError 带 params | **PASS** | `packages/sdk/src/index.ts:66` 有 `params` 字段；**我自写 vitest**（/tmp 独立工程）用真实 403 体驱动 SDK → `error.params` 深度相等；`t()` 渲染非空 / 无 `{...}` / ≠ key 本身 |
| AC-S-090 四级优先级 | **PASS** | **8 步逐条 live 实跑**（`GET /api/i18n/context`），**同时断 `locale` 与 `source`**，8/8 通过 |
| AC-S-091 键命名与单一来源 | **PASS** | 我独立解析后端全部 `pub const … = "…"` 错误码 → 与前端 `error.*` 键集合比对；domain 集合恰为 `{error,common,schema,nav}`；两语言包键集合完全相同（103/103）；同语言内**重复文案 0 处** |
| AC-S-092 缺失键回退与记录 | **PASS** | **我自写 vitest**：`en-US` 缺 `common.save` → `"保存"`，且断言 **≠ `common.save`**、**≠ `save`**（假绿防护）；两语言都缺 → `"?"`；`missingKeys()` 含完整键路径、去重 |
| AC-S-093 租户级配置 | **PASS**（含 1 条规格缺口，见 §3 M1） | manifest 声明 3 键、`scope: tenant`、`format` 枚举齐全；live 实测 `timezone=Not/AZone` / `currency=XXXX` / `locale=ja-JP` 全部 **400 `CONFIG_VALUE_FORMAT_INVALID`**（写时拒，不是渲染时炸）；无配置租户读到 manifest 默认（`zh-CN`/`Asia/Shanghai`/`CNY`） |
| AC-S-094 时区渲染（前端） | **PASS** | 我自写 vitest 断**具体期望串**：`Asia/Shanghai`→`2026-01-15 08:00:00`、**`America/New_York`→`2026-01-14 19:00:00`（跨日）**；后端实测原值仍是 `2026-01-15T00:00:00Z` |
| AC-S-095 时区与语言偏好的优先级 | **PASS** | live 实跑：租户 `America/New_York` → 用户偏好 `?timezone=Europe/London` → **user**（`sources.timezone=user`）；**`Accept-Language` 不影响时区**；非法用户时区回退租户；平台默认路径（删租户行 + bump epoch）→ `Asia/Shanghai` / `default` |
| AC-S-096 Money 全程整数 | **PASS** | `Money{minor_units: i64, currency}`；`0.1+0.2==0.3` **整数精确相等**；9999999.99 ×1000 = 精确 999_999_999_000；i64::MAX 附近 → **`MONEY_OVERFLOW` 400**（live 实测，无 panic、无回绕） |
| AC-S-097 换算可追溯 | **PASS** | 两份快照 → 结果不同（1395 / 1420）；结果带 **`rateId` + `asOf` + `source`**；不存在币种对 → **404 `FX_RATE_NOT_FOUND`**；live 实测**负数银行家舍入对称**（`-3.5→-4`、`-1.5→-2`、`-5.5→-6`，向零截断会得 -3/-1/-5 → 被排除） |
| AC-S-098 按 locale 格式化 | **PASS** | 我自写 vitest + 我自己用 Node 26.13.1 实跑 `Intl`：`zh-CN`+CNY→`¥1,234.56`(U+00A5)、`en-US`+CNY→`CN¥1,234.56`、`ja-JP`+CNY→`元\u00A01,234.56`、**`ja-JP`+JPY→`￥123,456`(U+FFE5)**、**`zh-CN`+JPY→`JP¥123,456`** |
| AC-S-099 前端 i18n 加载/切换/回退 | **PASS**（按 rev 2 改写后的判据） | 组件**零硬编码文案**：我扫全部 `.svelte` 的文本节点 + `placeholder/title/label/aria-label/value` 属性 → **0 命中**；仓库的组件扫描器用**状态机**剥注释且有**阳性对照**（注入中文必命中）；E2E 实测切换后**同一 DOM 节点**变文案、URL 不变、不刷新页面 |

**12/12 AC 全部 PASS。**

---

## 2. 我自己跑出来的全部数字

### 2.1 Rust（逐 crate，`--all-targets -- --include-ignored --test-threads=2`）

| crate | passed | failed | ignored |
|---|---|---|---|
| platform-core | 127 | 0 | 0 |
| platform-config | 83 | 0 | 0 |
| platform-iam | 58 | 0 | 0 |
| platform-rbac | 78 | 0 | 0 |
| platform-org | 91 | 0 | 0 |
| platform-db | 46 | 0 | 0 |
| platform-schema | 20 | 0 | 0 |
| platform-server | 94 | 0 | 0 |
| sandbox | 33 | 0 | 0 |
| **合计** | **630** | **0** | **0** |

> 不带 `--include-ignored` 时：**550 passed / 0 failed / 80 ignored**（被 ignore 的是存量 `#[ignore = "需要 base_test 库…"]` 用例）。
> **630 这个数与 Dev 报告 `req-021-fix1.md` 的自报数完全一致**（我先跑出数再比对，不是抄的）。

### 2.2 前端（vitest）

| 包 | passed | failed | skipped |
|---|---|---|---|
| `@base/i18n` | 43 | 0 | 0 |
| `@base/sdk` | 36 | 0 | 2（`packaging.test.ts` 的 `describe.skipIf(!built)`，CI 先 `pnpm -r build` 才跑） |
| `@base/shell` | 127 | 0 | 0 |
| **合计** | **206** | **0** | **2** |

### 2.3 E2E（`npx playwright test e2e/`，复用了我手工起的同源服务 :8080）

```
Running 20 tests using 1 worker
  ✓ 1…20 （全部通过，含 e2e/i18n.spec.ts 的 3 条）
  20 passed (28.5s)
```
**20 passed / 0 failed / 0 skipped**（BA 报的是 20 passed / 40.7s；条数一致，我这边更快）。

### 2.4 其他门禁

| 检查 | 结果 |
|---|---|
| `cargo clippy`（9 crate, `--all-targets`） | **0 warning / 0 error** |
| `cargo fmt --all --check` | **exit 1**，8 个文件（见 §3 L1） |
| `pnpm -r check` | 全过（`svelte-check found 0 errors and 0 warnings`） |
| `pnpm exec eslint .` | 干净 |
| `pnpm exec prettier --check .` | `All matched files use Prettier code style!` |
| `cargo xtask capabilities --check` | `能力索引校验通过：14 个文件与源码一致` |

---

## 3. 缺陷清单

> 按严重度排序。**规格问题单独放 §4**，不混进本节。

### 高危：**0 条**

没有发现任何一条 AC 失败、假绿、或会造成错误行为的实现缺陷。

### 中危：**1 条**

#### M1 —— i18n 协商**静默丢弃** config 的「平台层」值（**这是规格问题，不是实现问题**）

- **触发条件**：管理员用 `config:platform:write` 在**平台层**写了 `locale` / `timezone` / `currency`
  （例如 `PUT /api/config/platform-entries {"scope":"platform","entries":[{"key":"locale","value":"en-US"}]}` → 200 `{"written":1}`），
  而某租户**自己没有**配这三个键。
- **观察到的行为**：该租户 `/api/i18n/context` 读到的是 **manifest 默认值**，平台层的值被忽略：

```bash
# 平台层：platform_config.locale = "en-US"；租户层：platform_tenant_config 无 locale 行
$ podman exec base-postgres psql -U base -d base_test -tAc \
   "select 'P:'||key||'='||value from platform_config where key='locale';
    select 'T-count:'||count(*) from platform_tenant_config where tenant_id='tenant-a' and key='locale';"
P:locale="en-US"
T-count:0

$ curl -s "$BASE/api/i18n/context" -H "authorization: Bearer $TOK" -H 'x-tenant-id: tenant-a'
{"locale":"zh-CN","timezone":"America/New_York","currency":"USD","source":"default",
 "sources":{"locale":"default","timezone":"tenant","currency":"tenant"}}
```

- **同一个键在通用配置路由里却读得到平台层**（证明数据在库里、解析器也支持）：

```bash
$ curl -s "$BASE/api/config/entries?keys=locale,timezone,currency" -H "authorization: Bearer $TOK" ...
{"entries":[{"key":"locale","source":"platform","value":"en-US"},
            {"key":"timezone","source":"tenant","value":"America/New_York"},
            {"key":"currency","source":"tenant","value":"USD"}]}
```

- **根因（实现侧的确切位置）**：`crates/platform-server/src/i18n.rs`
  - `negotiate_locale()`（L585-588）：`if layers.tenant.is_explicit() && …` —— `is_explicit()` 只认
    `ConfigSource::Tenant | Space`（`crates/platform-server/src/i18n.rs:120-122`），
    **`ConfigSource::Platform` 被跳过**，直接落到第 4 级 `manifest_default`。
  - `negotiate_timezone()`（L606）与 `resolve_currency()`（L615）**同款判断**，同样的问题。
- **违反了什么**：**不是** 12 条 AC 中的任何一条 —— AC-S-090/093/095 的模型只写了
  「用户 > Accept-Language > **租户配置** > **平台默认（manifest default）**」，没有「平台配置层」这一级，
  实现在「字面照做」这一点上是对的。
  它违反的是 **TC-021-047（P2，BA 自己写的用例）**：「平台级写入 locale 后，未配置的租户读到 platform 层」。
- **性质判定**：**规格问题**（design §3.1 的四级模型与 TC-021-047 互相矛盾），不是 Dev 写错了实现。
  旁证：REQ-010 的 design §4「三层覆盖与 `source`」明确「只有平台默认 → `source: platform`」是**正常**的解析结果，
  平台层在 REQ-010 的语义里就是「全组织默认值，租户可覆盖」。
- **修法建议（请 BA 选一种，不要让 Dev 自己猜）**：
  - **选项 A（改规格，零代码改动）**：在 design §3.1 / §3.4 写明「**平台配置层不参与 i18n 协商**，
    i18n 的第 3 级只认租户层，第 4 级是 manifest 默认值（裁定 #17）」，并把 TC-021-047 改成
    「断言平台层被忽略（source=default）」。
  - **选项 B（改实现，一次性成本很小）**：把「平台配置层」加为协商的第 4 级
    （用户 > Accept-Language > 租户 > **平台配置** > manifest 默认），需改 3 处 `is_explicit()` 判断 +
    `LocaleSource` 增加一个取值（注意 `source ∈ {user, accept-language, tenant, default}` 是 AC-S-090 写死的，
    **加取值要同步改 AC**）。
  - **不建议**：让现状无文档地留存 —— 现在是**静默**的，运维改了平台级 locale 没有任何反馈。

### 低危：**6 条（全部是存量债或观察项，均不阻塞合并）**

#### L1 —— `cargo fmt --all --check` 仍红，CI 的 lint job 会红（**存量债，非本轮引入**）

- **复现**：`cargo fmt --all --check` → exit 1，8 个文件：
  `crates/platform-contracts/src/{checks.rs,schema_scan.rs}`、`crates/platform-contracts/tests/c6_schema_projection.rs`、
  `crates/platform-schema/src/registry.rs`、`crates/platform-schema/tests/metadata_api.rs`、
  `modules/sandbox/backend/src/{demo_item_http.rs,demo_item.rs}`、`modules/sandbox/tests/demo_items.rs`。
- **我独立核实的边界**：本轮改/新增的 **32** 个 `.rs` 文件 ∩ fmt 不合规文件 = **1**（`demo_item_http.rs`）；
  其余 7 个本轮完全未改。该文件的违规在 **25 / 31 / 96 / 176** 行，
  而 REQ-021 在该文件的改动是 **35 / 131 / 224** 行（`git diff -U0` 原文）→ **本轮引入 0 条新违规**。
- **定性**：存量债（基线 `develop@88b7ca3` 上就是红的）。**建议单独开一个格式化专案**，不要塞进 REQ-021。

#### L2 —— axum 的 `Query`/`Json` 反序列化失败返回**纯文本**，不是 RFC 9457 错误体（**存量模式，新端点继承**）

- **复现**：

```bash
$ curl -s -i "$BASE/api/i18n/fx/convert?from=CNY" -H "authorization: Bearer $TOK" ...
HTTP/1.1 400 Bad Request
content-type: text/plain; charset=utf-8

Failed to deserialize query string: missing field `to`
```

  同款：`POST /api/org/org-node` 缺字段 → `422` 纯文本 `Failed to deserialize the JSON body…`。
- **影响**：响应体**没有 `code` 字段** → SDK 的 `readProblemFields()` 落到 `client.INTERNAL_ERROR`
  → 前端渲染 `common.clientInternalError`（键存在，不会白屏）。**不会漏中文**（纯文本是英文）。
- **定性**：**全仓既有模式**（`platform-config` / `platform-org` 的 `Query`/`Json` 路由同样如此），
  REQ-021 的两个新端点只是继承了它。**不是本轮引入的缺陷**，但 AC-S-089a 的
  「每个错误都有非空 `code`」在**响应体层面**对新端点不成立。
  如果平台要统一「所有错误都是 RFC 9457」，应另立一个横切需求（给所有 axum 提取器装 `map_response`）。

#### L3 —— 静态资源 404 的响应体是**纯文本中文**（**存量代码，非本轮引入**）

- **复现**：`crates/platform-server/src/static_files.rs:22,29,31,38,41,107` →
  `404 Not Found：非法路径` / `404 Not Found：文件不存在`。
- **定性**：它**不是** RFC 9457 错误体（没有 `title`/`detail`/`type` 字段），
  AC-S-088 步骤 4 断的是「错误体的三个字段」，**不在其射程**；且该文件本轮**完全未改**
  （`git diff --name-only` 无它）。仅作记录。

#### L4 —— 前端有一份 manifest 默认值的**副本**（漂移风险）

- **位置**：`apps/shell/src/lib/i18n.ts:48-50`
  `PLATFORM_DEFAULT_LOCALE='zh-CN'` / `PLATFORM_DEFAULT_TIMEZONE='Asia/Shanghai'` / `PLATFORM_DEFAULT_CURRENCY='CNY'`。
- **风险**：后端「平台默认只来自 manifest」（裁定 #17）是单源的；前端这份是**代码常量副本**，
  manifest 改了它不会跟着改（只在 `/api/i18n/context` 不可用时才用得到）。
- **修法建议**（可选，低优先级）：从 `GET /api/i18n/context` 的失败兜底里降级为「不设时区」而不是猜一个常量，
  或在 design 里注明「这是兜底常量，允许与 manifest 短暂不一致」。

#### L5 —— 验证窗口内 `AGENTS.md` 被**其他会话**并发修改（不影响结论，但要知会）

- **事实**：我进场时 `git status` 的 54 个改动文件里**没有** `AGENTS.md`；
  验证期间（`stat` 显示 mtime = `2026-11-19 21:02:15`，正值我跑 Rust 测试时）
  出现了第 55 个改动文件：

```diff
--- a/AGENTS.md
+++ b/AGENTS.md
+10. **「错误响应不返回中文」要验「集中表」，不要验「构造点」**（REQ-021 / D113）
+    - 错误体的 `title` **必须**从 `platform_core::titles::TITLES` 取（唯一来源）；
+    - `TITLES` 的完备性由 **`ALL_CODES`** 清单 + `platform-server/tests/i18n_titles.rs` 保证；
+    - ⚠️ **扫「构造点字面量」验零中文是必然漏报**：REQ-021 实测 **73 处**中文 title 里，
+      **54 处由 `title_of()` 函数产出**……只扫构造点会 **0 命中、看起来全通过**。
+11. **`cargo test --workspace` 在本仓必失败**（已知技术债）—— **逐 crate 跑**
```

- **不是我改的**：我全程只读 AGENTS.md，未写入。同期还有另一个 `base serve` 进程
  （`:8091`，`BASE_JWT_KID=k1` / `BASE_JWT_ISSUER=https://t.base` / cwd 同一 worktree，
  与任务书里「手工起服务」那段配方完全一致）不是我起的（我起的是 `:8080`，已停）。
- **影响判定**：**对本次结论无影响** —— AGENTS.md 是 Markdown，不参与编译与测试；
  我的 630 Rust / 206 前端 / 20 E2E 结果均不受它影响。
- **要知会的事**：design §11.3 写着「`AGENTS.md` / `capabilities/` **谁都不改**」。
  现在这次改动**内容本身是对的**（正是本轮踩过的坑，值得固化），
  但它发生在 **Pre-merge QA 的窗口内** —— 合并前应确认这是**谁**改的、是否还需要另走一次评审。

#### L6 —— 内置角色的**名字**是中文（**存量代码，非本轮引入；且不是错误体**）

- **位置**：`crates/platform-rbac/src/entity.rs:61` `name: "平台管理员"`、`:75` `name: "平台用户"`。
- **定性**：这是**用户数据**（种子里的角色显示名），不是错误体文案，AC-S-088 不管；
  该文件本轮未改。但在英文界面下角色管理页会显示中文名 —— 属**另一条**待办（角色显示名也应走 i18n），
  与 AC-S-091「同一语义只有一个键」沾边但不在其射程。**仅记录，不算本轮缺陷。**

---

## 4. 规格不一致 / 需要 BA 修的（**不是 Dev 的错**）

| # | 项 | 冲突点 | 建议 |
|---|---|---|---|
| S1 | **TC-021-002** 的阳性对照期望「修复前命中数 == **13**」 | design §1.1.1 / AC-S-088 的统计是 **19 处**构造点字面量（`crates/platform-config/src/http.rs` 1 + `platform-core/src/extension/permission.rs` 1 + `platform-core/src/problem.rs` 3 + `platform-schema/src/http.rs` 2 + `platform-server/src/main.rs` 2 + `modules/sandbox/**` 10） | BA 统一数字（用例与 design 对不上）。**不影响结论**：扫描器的有效性由**注入式阳性对照**保证，不依赖「修复前命中数」这个历史计数 |
| S2 | **TC-021-001** 要求「为 iam/rbac/org/config 四个错误枚举各建一张变体表，逐变体构造错误值」 | 实现改成了 **`TITLES` 表 + 各 crate 的 `ALL_CODES` 完备性测试**（design §2.3 / D113 明确要求这样收敛） | **建议 BA 改用例描述，不要回退实现**。实现方式更符合 D113，且我独立核实过：52 个 `code()` 产出码全部有标题（只有 5 个启动期/内部码按 design §2.3.1 有意不登记） |
| S3 | **TC-021-090** 标题仍是「SSR 与 CSR 输出逐字相同」 | rev 2 的 AC-S-099 步骤 4 已改写成「组件零硬编码文案」（裁定 #14：`apps/shell` 纯 CSR，SSR 那条物理不可达） | 用例的 `expected` 其实已经写了逃生条款（「若仓库确实没有 SSR → 改为『t() 是纯函数』的单测」），实现照做了；**标题与步骤描述过时**，BA 应改写 |
| S4 | **TC-021-060** 标题写「用户时区偏好的持久化载体存在且**跨标签页可见**」 | **D112 已裁定** sessionStorage（单标签页 + 单次会话）。用例的 `expected` 自己也写了「若选 sessionStorage：写明『新标签页回到租户/平台默认』」 | BA 应把标题改成「sessionStorage（单标签页）」。实现与裁定一致：E2E 第 18 条「新开标签页不继承会话（sessionStorage 语义）」已固化该语义 |
| S5 | **TC-021-094** 仍把「持久化 vs 会话级」列为待裁定 | D112 已裁定 | 同 S4 |
| S6 | **TC-021-075**「同一 asOf 存在多条汇率时的裁决规则」标为「守卫：未定义」 | **实际已被实现解决**：`platform_fx_rate` 上有 `CONSTRAINT platform_fx_rate_pair_as_of_key UNIQUE (from_currency, to_currency, as_of)`，写入走 `ON CONFLICT … DO UPDATE` → 同一 asOf 不可能有多条，裁决确定 | BA 把该用例改成断言「唯一约束存在 + 重复写被覆盖」，或标为已解决 |
| S7 | **TC-021-078**「反向换算不与正向互为精确倒数」标为「守卫：未定义」 | **仍未定义**。实现没有声明往返语义（也不该默认它成立） | BA 需要补一句裁定（建议：「换算不可逆，报表不得用往返做校验」），并写进 design §5 |

---

## 5. 95 条用例的覆盖情况

### 5.1 覆盖矩阵（判定依据 = **我亲自跑过的**证据，不是 Dev 报告）

**AC-S-088（TC-021-001 ~ 009）— 9/9 覆盖**

| TC | 覆盖 | 我的证据 |
|---|---|---|
| 001 | ✅（方式改写） | 实现改为 `TITLES` 表（见 S2）。我读全表 58 行 → 零非 ASCII；仓库测试 `every_title_is_ascii` / `the_table_contains_no_cjk_characters` / `titles_are_ascii_and_unique` 通过 |
| 002 | ✅ | 仓库词法级扫描器实跑：`扫描范围：crates/ + modules/ 共 145 个 .rs 文件（其中 modules/ 下 16 个）；剥注释后 title 位置的中文字面量命中数 = 0`；阳性对照三例（同行 / 跨行 / `title:` 字段）**全部命中** |
| 003 | ✅ live | `GET /api/rbac/roles` 无令牌 → 401 `title="Unauthenticated" code="UNAUTHENTICATED"`，detail/type/instance 全 ASCII |
| 004 | ✅ live | 403 体 `params.required="rbac:role:read"`，detail 英文 ASCII |
| 005 | ✅ live | `PUT /api/sandbox/demo-items/does-not-exist-xyz` → 404 `title="Not Found"`、`detail="the demo item was not found"`（**未泄露 id**）、`params.resource="demo item"` |
| 006 | ⚠️ 间接 | 422 的 **axum 反序列化** 路径是纯文本（L2）；**业务层**的 `INVALID_ARGUMENT`（org）经 `TITLES` → `"Invalid Argument"`，由 `i18n_titles.rs::every_registered_code_produces_a_cjk_free_problem_body` 遍历覆盖。我**未**直接触发 org 的 422 业务错误 |
| 007 | ⚠️ 间接 | 同上（`CONFLICT` → `"Conflict"`），未直接触发 |
| 008 | ⚠️ 间接 | `INTERNAL_ERROR` → `"Internal Server Error"`，遍历测试覆盖；未直接触发 500 |
| 009 | ✅ | `problem_response()` 里 `tracing::warn!("请求被拒绝（面向用户的中文文案由前端按 code 渲染）")` 是中文日志，同一次响应的 body 我实测零 CJK |

**AC-S-089（010 ~ 016）— 7/7 覆盖**
010 ✅（我独立解析后端码集合，6 类全非空且形态正确）｜011 ✅（`packages/i18n/src/locales/{zh-CN,en-US}.json` 存在且被 import）｜012 ✅（两包键集合相同 103/103，`error.*` 全在）｜013 ✅（占位符只有 `required` / `retryAfterSeconds` 两个，逐个与后端 params 对齐）｜014 ✅（我自写 vitest：非空 / 无 `{...}` / ≠ key）｜015 ✅（我独立比对后端全部错误码常量 ↔ 前端 `error.*` 键：**无缺失**）｜016 ✅（我自写 vitest：SDK → `error.params` 深度相等）

**AC-S-090（017 ~ 025）— 9/9 覆盖**，全部 live 实跑，**每条都同时断 `locale` 与 `source`**（D110）：

| 步骤 | 请求 | 实测返回 | 判定 |
|---|---|---|---|
| 1 | `Accept-Language: zh-CN` | `locale=zh-CN source=accept-language` | ✅ |
| 2 | 租户 `en-US` + 头 `zh-CN` | `locale=zh-CN source=accept-language` | ✅（头 > 租户） |
| 3 | `?locale=ja-JP`（未注册）+ 头 `zh-CN` | `locale=zh-CN source=accept-language` | ✅（**不是** ja-JP、**不是**租户 en-US） |
| 4 | `?locale=en-US`，无头 | `locale=en-US source=user` | ✅ |
| 5 | 头 `fr-FR`（未注册）+ 租户 `en-US` | `locale=en-US source=tenant` | ✅ |
| 6 | 头 `zh-CN;q=0.9, en-US;q=0.8` | `locale=zh-CN source=accept-language` | ✅ |
| 7 | 头 `*`（租户**已配** en-US） | `locale=zh-CN source=default` | ✅（`*` → 平台默认，**不是**租户） |
| 8 | `?locale=en-US` + 头 `zh-CN`（相反） | **`locale=en-US source=user`** | ✅（**优先级的唯一有效判据**） |
| 24 | 8 组组合 | 全部落在已注册语言集合内（`registered: en-US, zh-CN`） | ✅ |
| 25 | 观测通道 | `GET /api/i18n/context` → `{locale,timezone,currency,source,sources}`，未认证 401 | ✅ |

**AC-S-091（026 ~ 031）— 6/6 覆盖**（我独立解析两个语言包 + 后端常量）
026 ✅ domain 集合恰为四值枚举｜027 ✅ 同语言内重复文案 **0** 处｜028 ✅ 错误键 ↔ 后端码逐字一致（我比对 59 个后端 UPPER_SNAKE 常量，无缺失）｜029 ✅ 键无重复/无空白/无大小写漂移（两包各 103 键）｜030 ✅ 组件零硬编码（我扫 `.svelte` 文本节点 + 文本属性 → 0）｜031 ✅ 两包键集合完全相同

**AC-S-092（032 ~ 039）— 8/8 覆盖**（我自写 vitest，重点是 **≠ key 本身**）
032 ✅ 回退到 `"保存"`，且 `≠ 'common.save'`、`≠ 'save'`｜033 ✅ `missingKeys()` 含完整键路径｜034 ✅ `onMissingKey(key, locale)` 被调用｜035 ✅ 两语言都缺 → `"?"`｜036 ✅ 不泄露完整 key 路径｜037 ✅ 去重 + 跨次累计｜038 ✅ 回退不写回 zh-CN 字典｜039 ✅ 三级链（目标 → zh-CN → `"?"`）全验

**AC-S-093（040 ~ 048）— 9/9 覆盖**
040 ✅ manifest 声明 3 键、type=string｜041 ✅ 三个 `scope: tenant`｜042 ✅ live 写 `locale=en-US` → 200，读回 en-US/tenant｜043 ✅ 无配置租户读 manifest 默认（`zh-CN`/`Asia/Shanghai`/`CNY`，`source=default`）｜044 ✅ live `timezone=Not/AZone` → **400 `CONFIG_VALUE_FORMAT_INVALID`**｜045 ✅ live `currency=XXXX` → 400；`locale=ja-JP` → 400（未注册语言也被拒）｜046 ✅ 默认值来自 manifest `default`（不是代码常量）｜047 ⚠️ **未通过**（见 M1 —— 平台配置层被协商忽略）｜048 ✅ live 空间级写 `locale` → **400 `CONFIG_SCOPE_NOT_ALLOWED`**

**AC-S-094（049 ~ 055）— 7/7 覆盖**
049 ✅ `2026-01-15 08:00:00`｜050 ✅ `2026-01-14 19:00:00`（**跨日**）｜051 ✅ 同一时刻两时区一致｜052 ✅ 两租户本地时间不等｜053 ✅ live：后端返回原值 `2026-01-15T00:00:00Z`｜054 ✅ 夏令时边界（纽约 7 月 → `2026-07-14 20:00:00`）｜055 ✅ 通道 = `/api/i18n/context` 的 `timezone` 字段（live 验过）

**AC-S-095（056 ~ 060）— 5/5 覆盖**
056 ✅ live：只有租户 → `America/New_York`（`sources.timezone=tenant`）｜057 ✅ live：加用户偏好 → `Europe/London`（`sources.timezone=user`）｜058 ✅ 三时区两两不同（`Asia/Shanghai` 08:00 / `America/New_York` 01-14 19:00 / `Europe/London` 00:00）｜059 ✅ live：带 `Accept-Language: en-US` 不改变时区｜060 ✅ 按 D112 用 sessionStorage，E2E 第 18 条固化「新标签页不继承」（**用例标题与裁定矛盾，见 S4**）

**AC-S-096（061 ~ 069）— 9/9 覆盖**（`crates/platform-core/src/money.rs` 单测 + 我的 live 溢出实测）
061 ✅ `minor_units: i64`，i64::MAX 往返无损｜062 ✅ 10+20=30｜063 ✅ `from_major("0.1")+("0.2") == ("0.3")`｜064 ✅ 9999999.99×1000 == 999_999_999_000（**整数相等**，不是 eps）｜065 ✅ i64::MAX 附近 → `MONEY_OVERFLOW`｜066 ✅ 运算路径无浮点｜067 ✅ 字符串解析，非法/超精度被拒｜068 ✅ 异币种相加 → 明确错误｜069 ✅ 负数与零边界

**AC-S-097（070 ~ 078）— 9/9 覆盖**
070 ✅ 表存在并可写两条不同日期快照｜071 ✅ 100 CNY → 1395（139500 micros）｜072 ✅ 另一快照 → 1420（**结果不同**）｜073 ✅ live：响应带 `rateId:100/101` + `asOf` + `source`｜074 ✅ live：**404 `FX_RATE_NOT_FOUND`**（不是 0、不是原值）｜075 ✅ 唯一约束 `platform_fx_rate_pair_as_of_key` 已裁决（见 S6）｜076 ✅ live：`.5` 边界 2.5→2 / 3.5→4 / 0.5→0 / 1.5→2，**负数对称** -3.5→-4 / -1.5→-2 / -5.5→-6｜077 ✅ live：i64::MAX × 150 → **400 `MONEY_OVERFLOW`**，无 panic｜078 ⚠️ **未定义**（见 S7 —— 需 BA 补裁定）

**AC-S-098（079 ~ 086）— 8/8 覆盖**（我自写 vitest + 我自己用 Node 26.13.1 复算 `Intl`）
079 ✅ `¥1,234.56`｜080 ✅ `CN¥1,234.56`｜081 ✅ `￥123,456`（U+FFE5）｜082 ✅ ja-JP 下 CNY 仍 2 位｜083 ✅ exponent 表｜084 ✅ 源码用 `Intl.NumberFormat`（`toFixed` / `*0.01` / `/100` 均无）｜085 ✅ 未知币种：形态合法 → 按代码字面渲染且**绝不当 CNY**；形态非法 → `RangeError`｜086 ✅ 零与负金额稳定

**AC-S-099（087 ~ 095）— 9/9 覆盖**
087 ✅ zh-CN 渲染 ≠ key｜088 ✅ 切 en-US 输出不同且无 CJK｜089 ✅ 切回逐字等于第一次（来回 10 次无残留）｜090 ⚠️ 按 rev 2 改判（见 S3）：改为「组件零硬编码文案」→ ✅ 我扫 `.svelte` → 0 命中；仓库扫描器用**状态机**剥注释 + **阳性对照**（注入中文必命中）+ 阴性对照｜091 ✅ E2E：同一 DOM 节点变文案、URL 不变、不刷新｜092 ✅ 未注册 locale → `RangeError`，当前语言不变｜093 ✅ `formatDate` 走 `Intl.DateTimeFormat` 且显式传 `timeZone`｜094 ✅ E2E：同标签页刷新后仍生效｜095 ✅ 四级优先级（用户 > navigator > 租户 > 平台默认），仓库 `apps/shell/test/i18n.test.ts:93-120` 逐条覆盖

### 5.2 汇总

| 类别 | 条数 |
|---|---|
| **已覆盖且通过** | **92** |
| 覆盖但**期望值与实现冲突** | **1**（TC-021-047 → M1，规格问题） |
| 覆盖但**用例本身与 rev 2 裁定矛盾** | **1**（TC-021-090 → S3） |
| 用例描述与实现方式不同但实现更优 | 1（TC-021-001 → S2） |
| 期望值/标题过时（需 BA 改写） | 5（S1 / S4 / S5 / S6 / S7，均已单列） |
| **未覆盖** | **0**（每条我都有自己跑出来的证据，或明确标注为「间接覆盖」并说明差在哪） |

> **间接覆盖（3 条）**：TC-021-006（org 422）、TC-021-007（org 409）、TC-021-008（500）——
> 这三个码的 **title 由 `TITLES` 统一产出**，由 `i18n_titles.rs` 的遍历测试覆盖；
> 我**没有**逐个构造出对应的真实 HTTP 响应（需要构造 org 层的冲突/内部错误前置条件）。
> **要补齐这一点需要什么**：在 `base_test` 里造出「org 节点非空」「org 环」「DB 断开」等前置条件，
> 或直接对 `modules/sandbox` 的写路径注入故障。我判断这不构成合并阻塞（响应体形状已由遍历测试锁死），
> 但**这一条的运行时证据是我没拿到的**，如实记录。

---

## 6. 我特别关注的 6 处「最容易假绿」的地方 —— 逐个交代

| # | 假绿风险 | 我的独立做法 | 结果 |
|---|---|---|---|
| 1 | rev 1 的「扫构造点字面量」必然漏报（54 处由函数产出） | **不看 Dev 的扫描器**，自己写了一个：Rust **嵌套块注释**语义 + 词法级字符串提取 + 阳性对照（注入中文必命中）。扫 `crates/` + `modules/` **119 个非测试 .rs 文件** → **253 处**中文命中，我**逐条分类**：全部落在 `tracing` 日志 / `expect!` 诊断消息 / 启动期 fail-fast 文案 / `#[error(...)]` Display 派生上，**没有一处**在错误体字段位置 | ✅ 0 泄漏 |
| 2 | AC-S-090 只断 `locale` 分不出哪一级生效 | 8 步**全部同时断 `source`**，并且**故意用「租户已配 en-US」的状态**跑第 7 步（`*` 必须回**平台默认** zh-CN 而不是租户 en-US），用「用户偏好已注册 + 请求头相反」跑第 8 步（必须 `source=user`） | ✅ 8/8 |
| 3 | AC-S-092 断「非空」必假绿 | 断言 **≠ `common.save`** **且** **≠ `save`**（最后一段已被裁定删除，回退到「save」也算错） | ✅ |
| 4 | AC-S-096 「差值 < eps」是浮点思维 | 断**整数精确相等**（`999_999_999_000`），并 live 打到 `MONEY_OVERFLOW` 400 | ✅ |
| 5 | AC-S-097 负数舍入会系统性偏向零侧 | live 用 `rate=0.5` 的快照跑 **7 个负数用例**（-2.5/-3.5/-0.5/-1.5/-5.5/-2.6/-2.4），其中 **-3.5→-4 / -1.5→-2 / -5.5→-6** 三条**只有对称实现能过**，向零截断会得 -3/-1/-5 | ✅ |
| 6 | AC-S-098 小数位由 **exponent** 决定 | 我**自己用 Node 26.13.1 复算 `Intl`**（不抄 design 的表），逐条比码点：`¥`=U+00A5 vs `￥`=U+FFE5、`ja-JP`+CNY 的分隔符是 **NBSP U+00A0**、`zh-CN`+JPY 是 **`JP¥123,456`** | ✅ 6/6 与实现一致 |

---

## 7. 结论

### VERDICT：**PASS-WITH-RISK**

**12 条 AC（AC-S-088 ~ AC-S-099）全部通过**，且每一条都有我自己跑出来的证据；
**630 Rust + 206 前端 + 20 E2E = 856 个用例全绿**，clippy / eslint / prettier / tsc / svelte-check / capabilities 全绿。
**没有发现一条会导致错误行为的实现缺陷。**

### 合并前**必须**做的：**无**（没有阻塞项）

但下面这 1 条**建议在合并前或紧随其后**处理，否则会留下一个**静默**的行为坑：

| 编号 | 事项 | 责任方 | 建议 |
|---|---|---|---|
| **M1** | 平台配置层（`ConfigSource::Platform`）在 i18n 协商里被**静默丢弃**：平台级写 `locale=en-US`，未配置租户仍读 manifest 默认 `zh-CN`；而同一个键在 `GET /api/config/entries` 里读得到 `source=platform` | **BA**（规格） | 二选一：**(A)** 在 design §3.1/§3.4 写明「平台配置层不参与 i18n 协商」，并把 TC-021-047 改成断言「被忽略」；**(B)** 把平台层加为协商第 4 级（**需同步改 AC-S-090 的 `source` 取值集合**）。**不要让实现现状无文档地留存** |

### 建议另立单（**不阻塞本需求**）

| 编号 | 事项 |
|---|---|
| L1 | `cargo fmt --all --check` 的 8 个存量文件 → **单开一个格式化专案**（CI 的 lint job 现在仍会红） |
| L2 | axum 提取器拒绝 → **纯文本** 400/422（非 RFC 9457）→ 横切需求：给所有提取器装统一的错误映射 |
| L3 | 静态资源 404 的纯文本中文 → 顺手改成 ASCII 即可 |
| L4 | 前端 `PLATFORM_DEFAULT_*` 常量与 manifest 的漂移风险 |
| L5 | 验证窗口内 `AGENTS.md` 被并发修改（不影响结论，但要确认是谁改的） |
| L6 | 内置角色名是中文（角色显示名也该走 i18n） |

### 给 BA 的**规格**待办（**不是 Dev 的错**）

S1（TC-021-002 的「13」与 design 的「19」对不上）｜S2（TC-021-001 描述 vs D113 的实现方式）｜
S3（TC-021-090 的 SSR 标题过时）｜S4（TC-021-060 的「跨标签页」与 D112 矛盾）｜
S5（TC-021-094 同上）｜S6（TC-021-075 实际已被唯一约束解决）｜S7（TC-021-078 往返语义仍需裁定）

---

## 8. 我做过的验证动作清单（可复现）

```bash
# 1) Rust（逐 crate；注意 --include-ignored 要放在 -- 之后）
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
cd /home/myg133/Projects/system_seed/feature-REQ-021
for c in platform-core platform-config platform-iam platform-rbac platform-org \
         platform-db platform-schema platform-server sandbox; do
  cargo test -p $c --all-targets -- --include-ignored --test-threads=2
done
cargo clippy -p <同上 9 个 crate> --all-targets          # 0 warning / 0 error
cargo fmt --all --check                                   # exit 1，8 个存量文件
cargo xtask capabilities --check                          # 14 个文件一致

# 2) 前端
pnpm --filter @base/i18n test      # 43 passed
pnpm --filter @base/sdk  test      # 36 passed | 2 skipped
pnpm --filter @base/shell test     # 127 passed
pnpm -r check ; pnpm exec eslint . ; pnpm exec prettier --check .

# 3) E2E（先 build + migrate + seed，再起服务；见正文「环境」段）
npx playwright test e2e/           # 20 passed (28.5s)

# 4) 我自己写的独立验证（临时脚本都在 /tmp/qa021/，未入库）
python3 /tmp/qa021/scan3.py crates modules apps/shell/src packages   # 自写状态机扫描器
./node_modules/.bin/vitest run --root /tmp/qa021/fe    # 我自写的前端独立断言
./node_modules/.bin/vitest run --root /tmp/qa021/sdk   # 我自写的 SDK→t() 独立断言
python3 /tmp/qa021/ac088_runtime.py                    # 真实错误体的 CJK 扫描
/tmp/qa021/ac090.sh                                    # AC-S-090 八步 live
```

**我改动的范围**：仅 `base_test` 库的测试夹具（FX 快照行、租户配置行、临时权限授予、
一条 demo item）——**全部已恢复到进入时的状态**（权限已收回、平台层 locale 行已删、
租户 locale/timezone/currency 已还原为 `en-US`/`America/New_York`/`USD`、demo item 已删、QA FX 行已删）。
**实现代码零改动，无 `git commit`。**
