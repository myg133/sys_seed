# REQ-021「国际化与本地化」—— 测试用例总索引

- **需求编号**: REQ-021「国际化与本地化」
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: F12 国际化
- **验收基线**: [acceptance.md](../acceptance.md) —— **AC-S-088 ~ AC-S-099（12 条）**，本用例集只覆盖这一段
- **设计依据**: [design-summary.md](../design-summary.md)（rev 1，2026-11-19 19:30）
- **需求原文**: [demand.md](../demand.md)
- **代码基线**: `code/` @ `develop` @ `88b7ca3`（REQ-012 合并点）—— **本需求尚未实现**
- **状态**: 待实现（用例本身**尚未执行**，不得据此判定需求通过）
- **版本**: 第 1 版（rev 1）—— QA 独立复核 acceptance.md rev 1，逐条核对后端源码与运行时 `Intl` 行为

> ⚠️ **本文件同时是「用例集」与「挑刺报告」**。§8 是 QA 对 12 条 AC 的**逐条挑刺裁定表** ——
> **12 条 AC 全部需要修订**，其中 **15 条高严重度问题**、**10 条是硬伤**。
> **建议先出 `acceptance.md` rev 2 再开工**（结论见 §8.7）。

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **95**（TC-021-001 ~ TC-021-095，编号连续无跳号、无重复） |
| 覆盖 AC | **12 / 12**（AC-S-088 ~ AC-S-099，无遗漏、无多余） |
| 挑出问题数 | **42**（高 **15** / 中 **15** / 低 **12**） |
| 自动化分布 | 可自动化 = **95**、暂不可自动化 = **0**（但 **6 条是「守卫用例」，今天必红**，见 §6） |
| 层分布 | frontend-unit = **42**、rust-integration = **30**、rust-unit = **20**、e2e = **3** |
| 类型分布 | 功能 = **68**、安全 = **16**、可维护性 = **11** |
| 优先级分布 | P0 = **54**、P1 = **29**、P2 = **12** |
| 用例文件 | 6 个（另本索引 1 个） |
| 每条用例字段 | id / ac / title / preconditions / steps / expected / type / layer / automatable / priority / notes |

### 1.1 本轮最危险的三个结论（先看这三条）

1. **AC-S-088 的「静态扫描」会 100% 假绿**（挑刺 #1）。
   AC 要求扫「`ProblemDetails::new(` / `from_context(` 等构造点的**字符串字面量参数**」。
   但实测 **70 处中文 title 里有 54 处根本不经过字面量** —— 它们由
   `title_of(&error)` / `error.title()` 函数产出
   （`platform-iam/src/http.rs:97,134-153`、`platform-rbac/src/http.rs:279,294-306`、
   `platform-org/src/http.rs:216,263-280`、`platform-config/src/error.rs:163-178,412`）。
   构造点拿到的是**函数调用**，扫描器看不见。**照 AC 实现，扫完 0 命中、全部通过、漏洞原封不动。**

2. **「8 处中文 title、5 个文件」是错的，实测 70 处、7 个文件**（挑刺 #2）。
   逐函数计数：iam 17 + rbac 10 + org 15 + config 12 = **54** 处由 title 函数产出；
   另有 **13** 处直接字面量（sandbox 10 处、`extension/permission.rs:387`、
   `platform-config/src/http.rs:210`、`platform-server/src/main.rs:632`）；
   加 core 的 3 处（`problem.rs:125/146/167`）= **70**。
   而 AC 的缺口清单指向 `:185` 的 `"请求被拒绝（…）"` —— 那是 **tracing 日志（允许中文）**，
   真正的第三个中文 title 是 **`:167` 的 `"资源不存在"`**（挑刺 #3）。**按 AC 修完仍有中文 title。**

3. **AC-S-089 的两个前置今天都不成立**（挑刺 #4）。
   ① **前端没有翻译资源文件**：`packages/i18n/src/index.ts` 只有 `translate(dict,key)` / `hasMessage`，
   字典由调用方传入；唯一调用点 `apps/shell/src/pages/Home.svelte:13` 用的是**组件内联对象**。
   ② **SDK 把 `params` 丢了**：`packages/sdk/src/index.ts:46-64` 的 `ApiError` 只有
   `code/status/title/detail`，**没有 `params`**。AC-S-089 步骤 3「翻译条目占位符与 `params` 键一一对应」
   在前端**拿不到 `params`**，必须改 REQ-011 已合并的包 —— 而 demand.md 的依赖里没写这件事。

---

## 2. 文件清单

| 文件 | 能力域 | AC 范围 | 用例数 |
|---|---|---|---|
| [error-no-chinese.json](error-no-chinese.json) | 后端错误体零中文 / code+params 可渲染 | AC-S-088、AC-S-089 | 16（001 ~ 016） |
| [locale-negotiation.json](locale-negotiation.json) | 四级语言协商 / 键命名 / 缺失键回退 | AC-S-090、AC-S-091、AC-S-092 | 23（017 ~ 039） |
| [tenant-config.json](tenant-config.json) | 租户级 locale/timezone/currency 配置 | AC-S-093 | 9（040 ~ 048） |
| [timezone.json](timezone.json) | 时区渲染 / 时区优先级 | AC-S-094、AC-S-095 | 12（049 ~ 060） |
| [money.json](money.json) | `Money` 整数运算 / 汇率快照 / 按 locale 格式化 | AC-S-096、AC-S-097、AC-S-098 | 26（061 ~ 086） |
| [frontend-i18n.json](frontend-i18n.json) | `@base/i18n` 加载/切换/回退/CSR | AC-S-099 | 9（087 ~ 095） |

---

## 3. 夹具约定

> **原则 1**：本需求**没有任何 i18n 基础设施**（后端零 locale、零 Money、零汇率表；
> 前端无资源文件、无切换 UI）。§3.1 ~ §3.6 是 **QA 提出的夹具**，
> 标「**待钉死**」的项必须由 BA 写进 acceptance.md / design-summary.md，
> 否则 Dev 只能自选、用例的期望值就失去依据。
>
> **原则 2**：造数走管理员连接（`base_test`），业务访问走 `base_app`
> —— 与 REQ-012 的约定一致，理由见 `main.rs:256` 的租户隔离自检。
>
> **原则 3**：造数**必须**用独立幂等播种脚本（D101）；E2E 的 `skip` 只允许「系统未就绪」，
> 403/500 必须 `fail`（D108）。

### 3.1 租户 / 用户 / 语言

| 标识 | 配置 | 用途 |
|---|---|---|
| 平台默认 | locale `zh-CN`、timezone `Asia/Shanghai`、currency `CNY` | 兜底层 |
| 租户 `T0` | 什么都不配 | 验「回退到平台默认」 |
| 租户 `T1` | locale `en-US`、timezone `America/New_York` | 验「租户层」 |
| 租户 `T2` | timezone `America/New_York`、locale `en-US` | 时区 AC-S-094 的第二租户 |
| 用户 `u0` | 无任何偏好 | 基线 |
| 用户 `u1` | 无偏好，属 T1 | 验请求头 / 租户 |
| 用户 `u2` | locale 偏好 `ja-JP`（**未注册**） | 验「未注册即视为没设」 |
| 用户 `u4` | locale 偏好 `en-US`（已注册） | 验「用户偏好压过请求头」 |
| 用户 `u5` | timezone 偏好 `Europe/London` | 验「用户 > 租户」 |

> **已注册语言集合 = `{zh-CN, en-US}`**。
> ⚠️ **它的来源未定义**（manifest？语言包文件？常量表？）—— 挑刺 #24 / #35，**必须钉死**。

### 3.2 错误样本（AC-S-088 / 089）

| 类 | 状态 | 触发方式 | 期望 `code` |
|---|---|---|---|
| 未认证 | 401 | `GET /api/rbac/roles` 不带令牌 | `UNAUTHENTICATED` |
| 无权限 | 403 | `u1` POST `/api/rbac/roles` | `INSUFFICIENT_PERMISSION` |
| 不存在 | 404 | GET 不存在的角色 ID | `NOT_FOUND` |
| 校验失败 | 422 | POST `/api/org/nodes` 且 `nodeType` 非法 | `INVALID_ARGUMENT` |
| 冲突 | 409 | 重复创建同 code 的角色 | `CONFLICT` |
| 内部 | 500 | 故障注入（DB 断开 / 权限缓存刷新失败） | `INTERNAL_ERROR` |

> ⚠️ **422 只有 org/rbac 会产生**（`platform-org/src/error.rs:162-163`），core 没有 422 ——
> AC 步骤 2 只写「422 校验失败」没指定端点（挑刺 #16）。

### 3.3 翻译夹具（AC-S-089 / 091 / 092 / 099）

**待钉死**：资源文件路径建议 `packages/i18n/src/locales/{zh-CN,en-US}.json`。

| 键 | zh-CN | en-US |
|---|---|---|
| `common.save` | 保存 | Save |
| `common.cancel` | 取消 | Cancel |
| `error.UNAUTHENTICATED` | 请先登录 | Please sign in |
| `error.INSUFFICIENT_PERMISSION` | 需要权限：{required} | Permission required: {required} |
| `error.NOT_FOUND` | 资源不存在：{resource} | Not found: {resource} |
| `common.btnSave` | **不存在** | **不存在**（供 TC-021-027 的重复键检查） |

> 「两种语言都缺」的键（TC-021-035）在生产资源里**构造不出来**（键是常量）——
> 只能用测试专用 messages 对象注入（挑刺 #36）。

### 3.4 时区样本（AC-S-094 / 095）

| 时段 | UTC 原值 | Asia/Shanghai | America/New_York | Europe/London |
|---|---|---|---|---|
| 跨日（主用例） | `2026-01-15T00:00:00Z` | `2026-01-15 08:00:00` | `2026-01-14 19:00:00` | `2026-01-15 00:00:00` |
| DST 开始 | `2026-03-08T07:00:00Z` | — | `2026-03-08 03:00:00` | — |
| DST 结束 | `2026-11-01T05:30:00Z` | — | `2026-11-01 01:30:00` | — |

> 同一天三个时区值两两不同（08:00 / 前一日 19:00 / 00:00），**优先级可区分**。
> 输出格式 **已钉死**为 `yyyy-MM-dd HH:mm:ss`（24 小时制、无时区后缀）—— AC 没写格式（挑刺 #25），
> 但 `TC-021-027` 已把它钉死，**前端实现必须按到秒输出**（`formatToParts` 取数字部件再拼）。
>
> ⚠️ **「Europe/London 在 1 月 = UTC」**（GMT，无夏令时）—— 若把平台默认改成 `UTC`，
> 上表第 2、4 列会**相同**，「两两不同」就失效。断言请用**具体期望串**，别用相对判据。

### 3.5 汇率样本（AC-S-097）

| # | from → to | `rate_micros` | `as_of` | `source` |
|---|---|---|---|---|
| 1 | CNY → USD | `140000`（0.14） | `2026-01-01T00:00:00Z` | `manual` |
| 2 | CNY → USD | `150000`（0.15） | `2026-06-01T00:00:00Z` | `manual` |
| 3 | CNY → USD | `1500000`（1.5，**合成**） | `2026-12-01T00:00:00Z` | `manual` |

> #3 只用于银行家舍入的 `.5` 边界（TC-021-076），不是真实汇率。

### 3.6 金额样本（AC-S-096 / 098）

| 场景 | 值 | 说明 |
|---|---|---|
| 浮点陷阱 | `0.1 + 0.2 == 0.3` | 字符串解析后应得 10 + 20 = 30 minor |
| 大额累加 | `9_999_999.99 × 1000` | = `999_999_999_000` minor（≈ 100 亿元，远低于 i64 上限） |
| 浮点判据 | `2^53 + 1 = 9_007_199_254_740_993` | f64 无法精确表示，往返即暴露 |
| 格式化（CNY） | `minorUnits: 123456` | zh-CN → `¥1,234.56`；en-US → `CN¥1,234.56` |
| 格式化（JPY） | `minorUnits: 123456` | ja-JP → `￥123,456`（**币种必须换成 JPY**，见挑刺 #13） |

---

## 4. 执行层次与命令

| 层 | 内容 | 命令（待 BA 确认） |
|---|---|---|
| `rust-unit` | `Money` 运算/溢出/舍入、`Accept-Language` 解析、title 全量 ASCII | `cargo test -p platform-core`、`-p platform-config`、`-p platform-iam` 等 |
| `rust-integration` | 6 类错误体、租户配置读写、时区协商 | `./scripts/test-integration.sh`（需 `DATABASE_URL_TEST` 指向 `base_test`） |
| `frontend-unit` | `t()` 回退链、`missingKeys()`、`formatMoney`/`formatDate`、`Intl` 静态检查 | `pnpm --filter @base/i18n test`、`pnpm --filter @base/shell test` |
| `e2e` | 语言切换后组件更新、跨标签页偏好 | `pnpm playwright test`（`scripts/e2e-server.sh` 起服务） |

> ⚠️ **前端单测环境缺口**：`apps/shell` 与各 package 的 devDependencies 里**没有** `jsdom` /
> `happy-dom` / `@testing-library/svelte`（已 grep 确认）。当前前端测试是纯逻辑单测（vitest，node 环境）。
> 需要 DOM 的用例（TC-021-091 的 E2E 部分）只能走 Playwright —— 挑刺 #30。

---

## 5. 本需求的假绿清单（每条都有对应挑刺）

| 陷阱 | 为什么在本需求特别危险 | 正确写法 |
|---|---|---|
| 扫「构造点字面量」当「扫全部 title」 | title 全经 `title_of()` 产出，扫不到 | 表驱动遍历**错误枚举的每个变体**（TC-021-001） |
| 断言「响应体非空」 | 现有 fallback 就是回落到 key 本身 | 断「== 基准语言的值」且「!= key」（TC-021-032/087） |
| 断言「`¥` 或 `CN¥`」 | `¥` 是 `CN¥` 的子串，恒真 | 钉死精确串 `CN¥1,234.56`（TC-021-080） |
| 用同一个 `currency` 测三个 locale | locale 不改 exponent，JPY 的例子必须换币种 | 显式分 CNY / JPY 两组（TC-021-079/081） |
| 时区用同日例子 | 时区没生效时日期也一样 | 必须跨日（`01-15T00:00Z` → 纽约 `01-14`） |
| 只断「两个时区结果不同」 | 两者都显示 UTC 时「不同」仍可能成立（若恰好差一天） | 加 UTC 对照三方比较（TC-021-052） |
| 金额断「差值 < eps」 | 那是浮点思维 | 断**整数精确相等**，并用 2^53+1 证明不是浮点（TC-021-061） |
| 用「8 处」当完成判据 | 实际 70 处 | 用「错误枚举变体数」与「直接字面量数」双指标（TC-021-001/002） |
| 把 `skip` 当通过 | D108 的教训 | 6 条守卫用例今天必红，必须红着进 rev 2 |

---

## 6. 依赖 BA 先钉死的前提（否则用例写不下去）

**这 9 项不钉死，对应用例只能写成「行为待定」= 不可判定。**

| # | 待钉死项 | 影响用例 | 建议取值 |
|---|---|---|---|
| P1 | **协商结果的观测通道**（响应头？字段？） | TC-021-017~025、056~059 | 响应头 `Content-Language` + `RequestContext::locale()` |
| P2 | **「已注册语言」的来源** | TC-021-017~024、092 | manifest 的 `config.locale` 白名单，或 `locales/*.json` 的文件名集合 |
| P3 | **平台默认值来源**（manifest default？常量？env？） | TC-021-040/046 | manifest 的 `default`（AC-S-093 步骤 5 已倾向此） |
| P4 | **翻译资源文件路径** | TC-021-011~016、026~039 | `packages/i18n/src/locales/{zh-CN,en-US}.json` |
| P5 | **占位符语法** | TC-021-013/014 | `{key}`（单花括号） |
| P6 | **`from_major` 的签名与超精度行为** | TC-021-063/067 | `from_major(&str, CurrencyCode) -> Result`；超精度拒绝 |
| P7 | **溢出错误的具体形状** | TC-021-065/077 | `Result<Money, MoneyError>`，`MoneyError::Overflow` |
| P8 | **`formatDate` 的输出格式** | TC-021-049~058 | `yyyy-MM-dd HH:mm:ss`（24 小时制） |
| P9 | **语言/时区偏好的持久化载体** | TC-021-057/060/094 | 二选一：sessionStorage（会话级）或 后端存储（真持久化） |

> 另有 8 条**守卫用例**（TC-021-011、015、016、025、044、045、055、090）
> 断言的是「这个前提还不存在」—— 它们今天必红，是**开工前必须补的交付物清单**，
> 不是实现 bug。报告里必须明确区分「红因为没做」与「红因为做错」。

---

## 7. 挑刺口径

对每条 AC 问 8 个问题：

1. **能判定吗？** 断言有没有固定的字面量 / 固定的形状？
2. **步骤与预期一一对应吗？** 有没有「步骤 3 说 A，预期只写 B」？
3. **有矛盾/重叠吗？** 同一条 AC 内、AC 与 design 之间、AC 与既有代码之间。
4. **漏了失败路径吗？** 只写 happy path 的 AC 一定有问题。
5. **断言会假绿吗？** 是否存在「实现错也能过」的断言形状。
6. **是不是在描述一个不存在的东西？** —— **必须核源码**，不能凭 AC 的自我描述。
7. **前提成立吗？** 前置条件里的实体（资源文件、UI、接口、字段）真的存在吗？
8. **严重度**：高 = 不修订就无法开工 / 无法判定 / 会放过真实缺陷；中 = 会误解或覆盖不足；低 = 表述与边界。

---

## 8. 挑刺裁定表（42 条）

> 证据行号基于 `code/` @ `88b7ca3`。`acceptance.md:xx` 指 rev 1 原文行号。

### 8.1 高（15 条 —— 硬伤 / 不可判定 / 假绿 / 矛盾）

| # | AC | 问题 | 严重度 | 建议修订 | 类型 |
|---|---|---|---|---|---|
| 1 | AC-S-088 步骤 1 | **「扫构造点的字符串字面量」100% 假绿。** 70 处中文 title 里有 **54 处不经过字面量**：`title: title_of(error)`（`platform-iam/src/http.rs:97`）、`title_of(error)`（`platform-rbac/src/http.rs:279`）、`title: title_of(error).to_string()`（`platform-org/src/http.rs:216`）、`error.title()`（`platform-config/src/http.rs:412`）。构造点拿到的是**函数调用**，扫描器看不见 → 照 AC 扫完 0 命中、全部通过、缺陷原封不动 | 高 | 把判据从「扫字面量」改成「**表驱动遍历错误枚举的每个变体并断言 `title()` 返回值是 ASCII**」。必须用 exhaustive `match` 保证覆盖（iam 17 / rbac 10 / org 15 / config 12 个变体），字面量扫描只作为**补充**判据（只管 13 处直接字面量） | 假绿 / 硬伤 |
| 2 | AC-S-088（缺口表 + design §2.2） | **「8 处、5 个文件」是错的，实测 70 处、7 个文件。** 逐函数计数：`title_of` iam **17**（`http.rs:136-152`）+ rbac **10**（`:296-305`）+ org **15**（`:265-279`）+ config `title()` **12**（`error.rs:165-176`）= 54；直接字面量 **13**（sandbox `http.rs:194,320,348,388,403,444`、`demo_item_http.rs:130,223`、`demo_note_http.rs:100,188`、`extension/permission.rs:387`、`config/http.rs:210`、`server/main.rs:632`）+ core **3** = 70。文件是 **7** 个（core/iam/rbac/org/config/sandbox/server），不是 5 | 高 | 缺口表改成「**70 处 / 7 个文件 / 6 个生产函数**」，并列出每个函数的变体数。完成判据也用这个数，否则「改完 8 处」会留下 62 处 | 硬伤（事实错误） |
| 3 | AC-S-088（缺口表第 1 行） | **缺口清单指向了错的行，且漏了真正的第三个 title。** AC 写 `problem.rs:125`（未认证）、`:146`（权限不足）、`:185`（请求被拒绝）。但 **`:185` 是 `tracing::warn!` 的日志文案**（`problem_response` 里），AC 自己在「⚠️ 允许」里说日志可有中文 —— 拿它当缺陷是自相矛盾。**真正的第三个中文 title 是 `:167` 的 `"资源不存在"`（`not_found`）**，AC 完全没提 | 高 | 缺口表改为 `:125` / `:146` / `:167` 三行（并注明 `:185` 是允许的日志）。修完必须断言 `not_found` 的 title == `"Not Found"`（TC-021-005） | 硬伤（事实错误） |
| 4 | AC-S-089 步骤 2/3 | **两个前置今天都不成立。** ① **无翻译资源文件**：`packages/i18n/src/index.ts` 只有 `translate(dict,key,fallback)` / `hasMessage`，字典由调用方传入；唯一调用点 `apps/shell/src/pages/Home.svelte:13` 是**组件内联对象** `{SHELL_READY:'已就绪'}`。② **SDK 丢了 `params`**：`packages/sdk/src/index.ts:46-64` 的 `ApiError` 只有 `code/status/title/detail`，**没有 `params`** → 前端拿不到 `params.required`，步骤 3 无法执行。补 `params` 要改 **REQ-011 已合并的包**，demand.md 的依赖里没写 | 高 | ① 在「范围内」明确列一条交付物「**新建 `locales/{zh-CN,en-US}.json` + `createI18n()`**」；② 在「依赖」里显式加上「**修改 `@base/sdk` 的 `ApiError` 增加 `params`**」，并说明这是跨需求改动；③ 若不想动 SDK，则 AC-S-089 降级为「**后端** `code`+`params` 自洽」，前端渲染部分移到 REQ-022 | 硬伤 / 越界 |
| 5 | AC-S-090（全条 7 步） | **没有「协商结果」的观测通道。** 7 个步骤全部写「断言协商结果」，但**全篇没说结果出现在哪里**。实测：`RequestContext` 只有 `tenant_id/space_id/user_id/trace_id/entered_at`（`platform-core/src/context.rs:20-92`），**无 locale**；全仓 `grep locale` 在 `crates/ modules/` 的 .rs 中 **0 命中**；`grep Content-Language` **0 命中**。没有通道 = 整条 AC 不可判定 | 高 | 钉死一个通道并写进 AC：**响应头 `Content-Language: <协商结果>`**（推荐，因为它对 401/403 也成立），并在 `RequestContext` 增加 `locale()`。AC 步骤改为「断言 `resp.headers['content-language'] == 'zh-CN'`」 | 不可判定 / 硬伤 |
| 6 | AC-S-090 步骤 1~7 | **核心裁定「用户偏好 > 请求头」没有被任何一步守住。** 步骤 2 只验到「请求头 > 租户」；步骤 4 的用户偏好是 `en-US` 且**不带请求头** —— 无法区分「用户偏好优先」与「请求头优先」两种实现。另外 **q 值与未注册语言的组合未定义**：`fr-FR;q=1.0, en-US;q=0.8` 时是「跳过 fr 取 en」还是「整体回退租户」？两种解释都符合 design §3.2 的字面 | 高 | ① 补一步「用户偏好 `en-US` + 请求头 `zh-CN` → **`en-US`**」（TC-021-020）；② 钉死「跳过未注册项、取下一个已注册」并写进 design §3.2；③ 步骤 5 的「`en-US`（租户）**或** `zh-CN`（平台默认）」必须二选一（挑刺 #20） | 覆盖不足 / 矛盾 |
| 7 | AC-S-092 步骤 4/5 + design §6.4 | **「回退到键的最后一段」是个坏设计，且与步骤 5 自相矛盾。** ① `common.save` → `save` 对用户无意义；更糟的是 `error.INSUFFICIENT_PERMISSION` 的最后一段就是 `INSUFFICIENT_PERMISSION` —— **等于把错误码甩给用户**，比 `?` 更差。② 步骤 5 要求「不含完整 key 路径」，步骤 4 却允许回落到 key 的一段 —— 判据必须写成「不含带 domain 前缀的完整路径」才不打架。③ design 是 **4 级**链（目标 → 基准 → 最后一段 → `?`），AC 只测 2 级 | 高 | 二选一：① **删掉「最后一段」**，回退链改为「目标 → 基准 → `?`」（TC-021-035/039 同步改）；② 保留但限定为「只对 `common/schema/nav` 域生效，`error` 域一律落 `?`」。无论哪种都要把第 4 级 `?` 写进 AC | 设计缺陷 / 矛盾 |
| 8 | AC-S-093 步骤 4 | **「写入 `timezone=Not/AZone` → 400」不可实现。** config 的写入校验**只做 JSON 类型匹配**：`platform-config/src/service.rs:465` `value_matches(value, spec.kind)`，而 `spec` 只有 `type/default/scope/label` 四个属性（`platform-core/src/extension/manifest_schema.rs:63-75` 的 `additionalProperties` 只允许这四个）。**没有任何 enum/pattern/自定义校验机制** → 今天写 `"Not/AZone"` 会 200 成功落库 | 高 | 在 design 里新增「config 值校验器」设计：给 manifest 的 `config` 段加 `enum`（locale/currency 用）与 `format: "iana-timezone"`（timezone 用）两个可选字段，并定义对应的校验与错误码。**这是一块新增能力，AC 与 design 都没承认** | 硬伤 / 不可实现 |
| 9 | AC-S-094 步骤 1/2 | **前后端归属不清，且后端没有 IANA 时区能力。** AC 写「以 T1 身份**取**该时间」→ 若指 HTTP，后端返回的仍是 UTC（步骤 5 自己也这么说）→ 步骤 1/2 的期望值（08:00 / 19:00）只能在前端成立。而**后端根本做不了时区换算**：`time` crate 只启用 `formatting` feature（`Cargo.toml:49`），**不含 IANA tzdb**；要加是**新依赖**，违反 design R6「零新 Rust 依赖」 | 高 | ① 把 AC-S-094 的 layer 明确为 **frontend-unit**（纯 `formatDate`），删除「以 T1 身份取」的表述；② 后端只保留步骤 5 的「UTC 原值不变」（TC-021-053）。若确实要 E2E，必须写明承载时间的页面/组件与选择器 | 不可判定 / 矛盾 |
| 10 | AC-S-095 步骤 2 | **「用户时区偏好」在后端没有存储。** 用户表无时区列（`migrations/20261118000400_iam_auth.up.sql:21-40` 只有 id/tenant_id/username/email/password_hash/status/mfa_enabled/…）；design §7.2 只存 **sessionStorage**（前端）。→ 后端**拿不到**「用户偏好」，AC 步骤 2「加上用户偏好 → 伦敦时间」在 `rust-integration` 层不可执行 | 高 | 二选一并写进 design：① **加用户偏好存储**（用户表加 `locale/timezone/currency` 三列，或 config 增加 `user` 作用域）→ 走后端；② **纯前端**（sessionStorage）→ AC-S-095 的 layer 改 frontend-unit，并删掉「用户 > 租户」在后端可验的暗示 | 硬伤 / 不可判定 |
| 11 | AC-S-096「溢出」 | **「返回错误」的具体形状没定。** 只写「`i64::MAX` 附近做加法 → 返回**错误**，不是静默回绕」。但：错误是 `Result<Money, MoneyError>` 还是 panic？`MoneyError` 的变体名？能否序列化成 ProblemDetails（若不能，API 层只能 500）？`+` 运算符若实现，溢出是 panic 还是无法表达？**验收判据写不出断言** | 高 | 写死：`fn checked_add(self, rhs) -> Result<Money, MoneyError>`、`MoneyError::Overflow` / `CurrencyMismatch` / `TooManyDecimals` / `FxRateNotFound`；`+` 运算符 `#[allow]` 不实现，或实现为 panic 并禁用 clippy 检查。错误要能被 `IntoResponse` 映射成 `MONEY_OVERFLOW` | 不可判定 |
| 12 | AC-S-097「舍入」 | **只写了规则名，没写算法，也没写中间量宽度。** design §5.4 给了 `2.5→2 / 3.5→4 / 0.5→0 / 1.5→2`，但没写：**作用在哪个量上**（除法商的余数比较）、**中间积用什么宽度**。实际公式 `result = round_half_even(a × rate_micros × 10^to_exp / (10^from_exp × 10^6))` 里，`a × rate_micros` 两个 **i64** 相乘，只要金额 > 约 9.2e12 minor（≈ 920 亿元）就溢出 → **必须用 i128 中间量**，design 只写「整数」 | 高 | 在 design §5.4 写死三段：① 公式（分子/分母显式）；② 中间量类型 **i128**（结果再降到 i64，溢出则 `Err(Overflow)`）；③ 半值判定用 `2*rem == den` 且比较商的奇偶。AC 补一条「i128 中间量」的静态断言 | 硬伤 / 不可判定 |
| 13 | AC-S-098 步骤 3 | **★ 事实错误：`ja-JP` + `CNY` 不会产出 `￥1,234`。** AC 前置固定 `Money { minorUnits: 123456, currency: "CNY" }`，步骤 3 却期望「含 `￥` 与 `1,234`」。**实测（Node 26 / ICU）**：`Intl.NumberFormat('ja-JP',{style:'currency',currency:'CNY'}).format(1234.56)` → **`"元\u00A01,234.56"`**（符号是「元」、分隔符是 U+00A0、仍是 2 位小数）。只有 `currency:'JPY'` 才得 `"￥123,456"`。**这条用例永远红，且会误导 Dev 去自造符号映射** | 高 | 步骤 1~3 拆成**两组变量**：① 固定 `currency: CNY`，只变 locale（zh-CN `¥1,234.56` / en-US `CN¥1,234.56` / ja-JP `元 1,234.56`）；② 固定 `locale: ja-JP`，只变 currency（CNY → 2 位 / JPY → 0 位 `￥123,456`）。**货币代码与 locale 是两个正交维度，不能混** | 硬伤（事实错误） |
| 14 | AC-S-099 步骤 4 | **「SSR 与 CSR 逐字相同」——本仓没有 SSR。** 实测：`apps/shell/src/main.ts` 是 `mount(App, {target})`（Svelte 5 客户端挂载）；`vite.config.ts` 只有 `svelte()` 插件与 dev/preview 代理，**没有 ssr 配置、没有 server entry**；`package.json` 无 `@sveltejs/kit`，build 是 `vite build`。→ 这条**无处可测** | 高 | 二选一：① **删除**步骤 4，改为「`t()` 是纯函数（不读 DOM / 不读 `navigator` / 同输入同输出）」的单测；② 新增 SSR 入口（**= 新范围**，必须写进 demand.md 并评估构建改动）。**不得用「跳过」当通过**（D108） | 不可判定 / 越界 |
| 15 | AC-S-088（design §2.2） | **影响面判断错误：「现有 8 处相关断言」不存在。** design §2.2 写「同时要改：断言中文 title 的测试（现有 8 处相关断言）」。实测：`platform-iam/tests/iam_real_db.rs:468` 只断言**两个响应体的 title 彼此相同**（不断言具体值）；`platform-config/tests/http_authz.rs:518,536` 断言的是 **branding 的 title**（`"T1平台"`），不是错误体；唯一出现中文 title 的是 `packages/sdk/test/sdk.test.ts:40` 的 **fixture**（`title: '权限不足'`），不是断言。→ 改 title **不会**让既有测试红 | 高 | design §2.2 改为「**无既有断言依赖中文 title**；只需同步 `packages/sdk/test/sdk.test.ts:40` 的 fixture 与 `docs/contracts` 的示例」。真正要担心的是**契约对外形状**（`title` 变英文后，是否有外部消费者依赖它）—— 这一点 AC 没讨论 | 硬伤（事实错误） |

### 8.2 中（15 条）

| # | AC | 问题 | 严重度 | 建议修订 | 类型 |
|---|---|---|---|---|---|
| 16 | AC-S-088 步骤 2 | **422 没指定端点。** AC 只写「422 校验失败」。实测 core 不产 422：`UNPROCESSABLE_ENTITY` 只在 `platform-org/src/error.rs:157-163`（`OrgCycle`/`OrgNodeNotEmpty`/`InvalidArgument`…）与 rbac 出现。不写端点，Dev/QA 各挑一个，样本不可复现 | 中 | 写明「422 样本用 `POST /api/org/nodes` + 非法 `nodeType` → `INVALID_ARGUMENT`」，并把 6 类的「触发方式」表（§3.2）搬进 AC | 不可判定 |
| 17 | AC-S-088 步骤 3 | **`type` 字段的断言恒真。** `type` 是 `format!("{PROBLEM_TYPE_BASE}/{}", kebab_case(code))`（`problem.rs:89`），`PROBLEM_TYPE_BASE` 是纯 ASCII 常量 → 断言它「不含 CJK」永远为真，没有判别力 | 中 | 保留 `type` 的断言但**降级为形状校验**（`^https://errors\.base\.example/[a-z-]+$`），把判别力集中在 `title` / `detail` | 假绿 |
| 18 | AC-S-088（「响应体」边界） | **「响应体」的范围没定义。** `crates/platform-server/src/static_files.rs:103-108` 的 404 返回 `text/plain` 且 body 是 `404 Not Found：{reason}`，reason 是中文（`"非法路径"` / `"文件不存在"` / `"读取失败：{err}"`）。这**不是 ProblemDetails**、也不是 JSON 接口 —— 算不算「响应体不得含中文」？AC 只扫 `crates/ modules/` 的 Rust 源码且限定在 `ProblemDetails` 构造点，**扫不到它** | 中 | 明确「接口响应体」= `application/problem+json` 与 `application/json`；静态资源的 `text/plain` 404 单列一条（要么也改英文，要么在 AC 里显式排除并说明理由） | 边界不清 |
| 19 | AC-S-089 步骤 1~3 | **只覆盖 6 个错误码 ≈ 全部码的 9%。** 实测错误码总量：iam 17 + rbac 10 + org 15（各自的 `pub mod code`）+ core 6 + config 12 + schema 2 ≈ **62~68**。用户真实会遇到的是 `INVALID_CREDENTIALS` / `ACCOUNT_LOCKED` / `LOGIN_RATE_LIMITED` / `TOKEN_EXPIRED`，而它们今天在前端是 `apps/shell/src/pages/Login.svelte:61-82` 里**硬编码的中文**，不走 i18n | 中 | 把判据从「6 类」改成「**后端全部 code 常量 → 前端必须有 `error.<CODE>`**」（TC-021-015 已按此写）。6 类只作为「运行时抽检」 | 覆盖不足 |
| 20 | AC-S-090 步骤 5 预期 | **「回退到 `en-US`（租户）**或** `zh-CN`（平台默认）」——「或」不可判定。** 两个实现（取租户 / 取平台默认）都满足字面。design §3.2 说「未注册视为没设，落到下一级」→ 应该是租户，但 AC 没写死 | 中 | 预期改成固定的「**`en-US`**（租户层）」，并补一句「因为 `fr-FR` 未注册 → 视为没设 → 落到租户层」 | 不可判定 |
| 21 | AC-S-091 步骤 4 | **位置写错且常量重名。** AC 写「等于后端 **`problem.rs`** 里的 `CODE_*` 常量值」—— 但 iam/rbac/org 的码在各自的 `error.rs` 的 `pub mod code`（`platform-iam/src/error.rs:30-47`、`rbac:24-33`、`org:21-35`），`problem.rs` 只有 **6** 个。另外存在**同名不同值**的常量：`CODE_INVALID` 在 config 是 `"CONFIG_INVALID_VALUE"`、在 iam 是 `"IAM_CONFIG_INVALID_VALUE"`；`CODE_MISSING` 同理 | 中 | 改为「等于后端 `code()` 方法的返回值全集」（跨 6 个 crate）；并要求「错误码全局唯一」——重复值要合并或加前缀 | 硬伤 / 不可判定 |
| 22 | AC-S-092 步骤 3 | **三选一与 design 已定项不同步。** AC 写「至少满足其一 —— ①`console.warn` ②`missingKeys()` ③`onMissingKey`（**实施前在 design 里选定一种并写死**）」。但 design §6.3 **已经选了「②+③ 两条都做」**并给了理由（`console.warn` 测试抓不到）。→ AC 与 design 打架，且 AC 把已定事项写成待定 | 中 | AC 步骤 3 改成固定的「**`missingKeys()` 导出 + `onMissingKey` 回调，两条都必须**」，删掉「三选一」与「实施前选定」的字样（与 REQ-012 #16 同类问题） | 矛盾 |
| 23 | AC-S-093 步骤 2 | **`scope: tenant` 与 D53 的语义、与「为什么不用 space」都没说。** D53 把 `scope` 解释为「**允许覆盖到的最深层**」→ `tenant` 意味着 **platform 层仍可写、space 层不可写**。而 REQ-010 的所有品牌键都是 `space`。多空间租户想「A 空间中文、B 空间英文」在本设计下**做不到**，AC/design 都没承认这个限制 | 中 | AC 补一句「`scope: tenant` 意味着 platform 可写、space 不可写；多空间分语言不在本轮范围」；若需要分空间，改 `scope: space` 并同步改 TC-021-048 | 覆盖不足 |
| 24 | AC-S-090 前置 / 步骤 | **「已注册语言」的来源未定义。** AC 前置只说「存在两个已注册语言 `zh-CN` / `en-US`」，但注册表在哪？manifest？语言包文件名？常量？这决定了步骤 3 的「`ja-JP` 未注册」怎么判、以及「协商结果必须是已注册语言之一」这条护栏怎么实现 | 中 | 钉死来源（建议：`packages/i18n/src/locales/*.json` 的文件名集合，后端从 manifest 的 `config.locale` 白名单读同一份）并写进 design | 不可判定 |
| 25 | AC-S-094 步骤 3 + 全条格式 | **「转回 UTC 相等」在字符串层不可实现，且输出格式未定。** 步骤 1/2 期望的是 `2026-01-15 08:00:00`（**无时区信息的人类可读串**）→ 无法「转回 UTC」。要么 `formatDate` 返回带偏移的 ISO（如 `2026-01-15T08:00:00+08:00`），要么另给 `toInstant()`。另外 `2026-01-15 08:00:00` 只是举例，格式（补位？24 小时制？分隔符？）全没写 → 断言无法机械 | 中 | ① 钉死输出格式 `yyyy-MM-dd HH:mm:ss`；② 步骤 3 改为「用 `formatToParts()` 取部件 + 各自时区重建 epoch 后比较」（TC-021-051 已按此写） | 不可判定 |
| 26 | AC-S-094 / 095 | **夏令时完全没覆盖。** 用「固定偏移表」实现（自己加减 5 小时）在 DST 边界会算错。实测：`2026-03-08T07:00:00Z` 在纽约是 **03:00:00**（跳过了 02:00），`2026-11-01T05:30:00Z` 是 **01:30:00**（重复时段）。AC 的例子（1 月）恰好是标准时间，区分不出「用 tzdb」与「用固定偏移」 | 中 | 补两个 DST 边界样本（TC-021-054 已给精确值），并把「必须用 `Intl`（内部带 tzdb），不得自造偏移表」写进 AC | 覆盖不足 |
| 27 | AC-S-096 步骤 2 / design §5.1 | **`from_major` 的签名与超精度行为未定。** design 只写「从 `major`（`"12.34"`）解析时用**字符串解析**」，没给签名（币种从哪来？），也没说「小数位超过 exponent」时是拒绝、截断还是舍入。`"0.30000000000000004"` 是「有没有走 f64」的关键输入，但 AC 没说该接受还是拒绝 | 中 | 写死 `fn from_major(s: &str, currency: CurrencyCode) -> Result<Money, MoneyError>`，超精度 → `Err(TooManyDecimals)`（TC-021-067 已按此写） | 不可判定 |
| 28 | AC-S-097 步骤 4 | **可追溯字段的「或」让形状不定。** AC 写「带快照标识（`rateId` **或** `asOf` + `source`）」。而 design §5.3 的表有 `id` / `as_of` / `source` 三列 —— 三者都该给。写成「或」则「只给 `asOf` 不给 `rateId`」也算过，反查就断了 | 中 | 写死「**三者全给**：`rateId`（`platform_fx_rate.id`）+ `asOf` + `source`」，并要求「用 `rateId` 能反查到 `rate_micros`」 | 不可判定 |
| 29 | AC-S-098 步骤 1/2 | **断言形状太弱，且跨环境不稳定。** ① 「含 `¥` **或** `CN¥`」—— `¥` 是 `CN¥` 的子串，这个断言**恒真**。② 精确串依赖 ICU 版本：本机 Node 26 下 `ja-JP`+`CNY` 的符号是「元」、分隔符是 **U+00A0（NBSP）**，不同 Node/ICU 版本可能变。用整串精确断言会**跨环境假红** | 中 | ① 钉死精确串：zh-CN `¥1,234.56`、en-US `CN¥1,234.56`；② 符号与数值分开断言（用 `formatToParts()` 或把 NBSP 归一化成普通空格），并记录 CI 的 Node/ICU 版本 | 假绿 / 边界不清 |
| 30 | AC-S-099 步骤 5 | **缺 UI 与测试环境。** 步骤 5 要「切换后已渲染的组件是否更新」，但：① **没有任何语言切换 UI**（`apps/shell/src/pages` 只有 Forbidden/Home/Login/ProtectedRoles/SchemaPage；`nav.ts` 的 4 个菜单项无设置入口）；② 前端单测环境**没有** `jsdom` / `happy-dom` / `@testing-library/svelte`（已 grep 确认 devDependencies）→ 组件级断言只能走 Playwright | 中 | ① 若保留 E2E：AC 补一条「页面必须提供语言切换控件（选择器钉死）」；② 若不做 UI：本步骤降级为 `subscribe()` 被调用的 unit 断言（TC-021-091） | 不可判定 / 覆盖不足 |
| 31 | AC-S-099 步骤 1~3 | **「切换到 `en-US`」的入口不存在，且与后端协商的二次闪烁没考虑。** ① 步骤 2 的「切换」没有任何入口（同 #30）。② design §7.2 的前端初始化顺序含 `navigator.language`，而浏览器会自动把它作为 `Accept-Language` 发出去 —— 后端按 AC-S-090 协商后若结果不同，首屏会「先英文、接上接口后跳中文」**闪烁一次**。AC 完全没考虑这条链路 | 中 | ① 明确切换入口；② AC 增加一句「首屏渲染必须与后端协商结果一致（不得先按 `navigator.language` 渲染再纠正）」，或明确接受「首次闪烁」并说明补偿（如阻塞首屏直到拿到 locale） | 覆盖不足 / 矛盾 |

### 8.3 低（12 条）

| # | AC | 问题 | 严重度 | 建议修订 | 类型 |
|---|---|---|---|---|---|
| 32 | AC-S-091 步骤 2 | **`domain` 枚举写成「`error` / `common` / `schema` / `nav` …」—— 省略号让规则不可机械。** design §6.1 用了「等」字。没有闭合集合，`^(\w+)\.(\w+)$` 就能过，等于没规则 | 低 | 钉死为闭合枚举 `{error, common, schema, nav}`（未来新增走同一份常量表），regex 写成 `^(error|common|schema|nav)\.` | 不可判定 |
| 33 | AC-S-088 步骤 1 | **CJK 正则范围有缺口。** 只写 `U+4E00`~`U+9FFF`，漏了 CJK 扩展 A（`U+3400`–`U+4DBF`）、兼容区（`U+F900`–`U+FAFF`）、以及**全角标点**（`U+FF00`–`U+FFEF`，如 `，` `：` `（`）。现有 `"请求被拒绝（…）"` 就含全角括号 —— 若它落到 title 里，只有扩展区/标点而没有基本区汉字时会漏检 | 低 | 改用 `\p{Han}`（Unicode 属性）或 `[U+2E80-U+9FFF U+F900-U+FAFF U+FF00-U+FFEF]` | 假绿 |
| 34 | AC-S-088 步骤 1 | **构造点清单不全。** 只列 `ProblemDetails::new(` / `from_context(` / `unauthenticated(` / `insufficient_permission(`。漏了：① `not_found(`（`problem.rs:159`，正是第 3 个中文 title）；② **结构体直写字面量** `ProblemDetails { kind, title: title_of(...), ... }`（`platform-iam/src/http.rs:93-105`）—— 它连 `new()` 都不经过 | 低 | 列表补 `not_found(` 与 `ProblemDetails {`，并明确「结构体直写也是构造点」 | 覆盖不足 |
| 35 | AC-S-089 步骤 3 | **占位符语法未定。** AC 只举 `{required}` 一例。若实现用 `{{required}}`、`%s`、`${required}`，`` `` 的正则与断言全废 | 低 | 钉死为单花括号 `{key}`，并在 design §6 写明「`{` / `}` 是保留字符，文案里需要字面花括号时用 `{{` / `}}`」 | 边界不清 |
| 36 | AC-S-092 步骤 4 | **「两种语言都缺」在生产键上构造不出来。** 键是代码里的常量，不可能「两种语言都缺」—— 除非资源表真的漏了。要让这条用例可执行，只能用**测试专用 messages 对象**注入缺失键。AC 没写这一点，Dev 会以为要改生产资源 | 低 | 写明「本场景用测试注入的 `messages` 对象构造，不修改生产资源」；并补一条「键无点号时落到 `?`」的规则（TC-021-035） | 覆盖不足 |
| 37 | AC-S-096 步骤 3 | **「1000 次」没有初值与期望值。** 「大额累加：`9_999_999.99 × 1000` 次」—— 是「加 1000 次」还是「乘以 1000」？期望值也没给。实际值应为 `999_999_999_000` minor（≈ 99.99 亿元） | 低 | 写成「初值 0，循环 1000 次 `acc += from_major("9999999.99")`，期望 `acc.minor_units() == 999_999_999_000`」 | 不可判定 |
| 38 | AC-S-096 步骤 4 | **「源码里是否有浮点类型参与金额运算」不可机械。** 实测 `platform-db/src/query.rs:56` 的 `BindValue::Float(f64)` 与 `extension/expr.rs:54` 的 `Float(f64)` 是**通用**类型，会命中 grep → 误报。不限定文件范围，这条永远红 | 低 | 限定扫描范围到 money 模块的文件白名单，并加阳性对照（TC-021-066） | 不可判定 |
| 39 | AC-S-097 步骤（舍入） | **同一 `asOf` 多条汇率的裁决规则未定义。** design R4 说「显式传 `asOf`」即可，但显式传并不能解决「同一 `asOf` 有两条」的情况；表定义也没写唯一约束 | 低 | 二选一：`(from,to,as_of)` 加唯一索引（写入冲突）；或明确「取 `created_at` 最新」并加 `ORDER BY` | 覆盖不足 |
| 40 | AC-S-099 `setLocale` | **`setLocale` 传未注册 locale 的行为未定义。** 后端规则是「未注册视为没设」（AC-S-090），前端的 `setLocale('ja-JP')` 是抛错、忽略、还是静默设成 `ja-JP` 然后全走回退？三者体验完全不同 | 低 | 写死「抛错」或「忽略并保持原值」，并同步到 design §7.1 的接口注释 | 不可判定 |
| 41 | design §7.2（BA 自错） | **误用 D99 为 `sessionStorage` 背书。** D99 的原文是「**会话（token + 租户）** 存 `sessionStorage`」，理由是「refresh token 长期驻留是 **XSS 的放大器**」。语言/时区偏好**不是凭据**，XSS 拿到它零危害 —— 用 D99 的理由论证偏好存储不成立。而且 `sessionStorage` 是**单标签页**作用域 → 新开标签页就丢偏好，与 demand.md 的「持久化」直接冲突 | 低 | design §7.2 改为独立论证：偏好用 `localStorage`（无 XSS 风险、跨标签）或后端存储；若坚持 sessionStorage，必须把 demand.md 的「持久化」改成「会话内保持」 | 硬伤（BA 自错） |
| 42 | AC-S-099 / AC-S-090 交叉 | **前端 `navigator.language` 与后端 `Accept-Language` 是同一个东西，但两条 AC 各写一套规则。** 不算矛盾，但「前端首屏可能用 `navigator` 猜、后端协商结果回来后再覆盖」这条链路没有被任何 AC 描述 → 实现可能做出「二次闪烁」 | 低 | 在 design 里加一节「首屏 locale 的确定流程」：前端必须在拿到后端协商结果后才渲染文案，或明确接受首次闪烁并加 loading 态 | 覆盖不足 |

### 8.4 按类型统计

| 类型 | 条数 | 编号 |
|---|---|---|
| 硬伤（事实错误 / 不可实现 / BA 自错） | **10** | #1、#2、#3、#4、#5、#8、#13、#14、#15、#41 |
| 不可判定 | **11** | #11、#12、#16、#20、#21、#24、#25、#27、#28、#32、#37 |
| 假绿 | **5** | #1、#17、#29、#33、#38 |
| 覆盖不足 | **8** | #6、#19、#23、#26、#30、#34、#36、#39、#42 |
| 矛盾 | **4** | #7、#22、#31、#41 |
| 边界不清 | **3** | #18、#29、#35 |
| 越界 | **2** | #4、#14 |

> 一条问题可命中多类型（#1 假绿+硬伤、#4 硬伤+越界、#41 硬伤+矛盾），故分项之和 > 42。

### 8.5 按 AC 统计（哪些 AC 必须改）

| AC | 问题数 | 其中高 | 结论 |
|---|---|---|---|
| AC-S-088 | 9 | **4** | **必须大改**（#1、#2、#3、#15、#16、#17、#18、#33、#34） |
| AC-S-089 | 3 | **1** | **必须改**（#4、#19、#35） |
| AC-S-090 | 5 | **2** | **必须改**（#5、#6、#20、#24） |
| AC-S-091 | 2 | 0 | 必须改（#21、#32） |
| AC-S-092 | 3 | **1** | **必须改**（#7、#22、#36） |
| AC-S-093 | 2 | **1** | **必须改**（#8、#23） |
| AC-S-094 | 3 | **1** | **必须改**（#9、#25、#26） |
| AC-S-095 | 1 | **1** | **必须改**（#10） |
| AC-S-096 | 4 | **1** | **必须改**（#11、#27、#37、#38） |
| AC-S-097 | 3 | **1** | **必须改**（#12、#28、#39） |
| AC-S-098 | 2 | **1** | **必须改**（#13、#29） |
| AC-S-099 | 5 | **1** | **必须改**（#14、#30、#31、#40、#42） |
| **合计** | **42** | **15** | **12 / 12 条都要改** |

### 8.6 已核实为「成立、不需要改」（避免过度挑刺）

| 项 | 核实结果 |
|---|---|
| `detail` 是否已经全英文 | **成立**。`problem.rs:225-234` 与 `platform-config/src/error.rs:428-456` 都有 `is_ascii()` 单测；抽查 iam/rbac/org 的 `detail()` 均无中文 |
| `code` + `params` 的形状是否已有 | **成立**。`ProblemDetails.params: Option<BTreeMap<String, Value>>` 已存在（`problem.rs:72-73`），`skip_serializing_if` 处理空值 |
| 时间是否已按 UTC 存储与传输 | **成立**。`timestamptz` + RFC 3339；`platform-core/src/context.rs:87` 用 `OffsetDateTime::now_utc()` |
| 是否已有 `Money` / 汇率表 | **确认不存在**（`grep struct Money|fx_rate|rate_micros` 0 命中）→ AC-S-096~098 是纯新增，不冲突 |
| 是否已有 locale/timezone/currency 字段 | **确认不存在**（migrations 全表 grep 0 命中；用户表 `platform_iam_user` 无相关列）→ AC-S-093/095 是纯新增 |
| `ConfigScope` 是否只有三值 | **成立**。`platform` / `tenant` / `space` 三值，`ConfigSource` 另有 `default`（四值），见 `extension/config.rs:16-96` 的模块文档 |
| 银行家舍入是否合理 | **成立**（金融行业默认、统计无偏）。问题只在「没给算法」（#12），不在「选错规则」 |
| 「用户偏好 > `Accept-Language`」这个裁定 | **成立**（用户显式选择应最权威）。问题只在「没有被任何用例守住」（#6），不在裁定本身 |
| 跨日例子的选择 | **成立且必要**。`2026-01-15T00:00:00Z` → 纽约 `01-14 19:00` 是正确且必需的判别点 |

### 8.7 总体结论

**不建议按 acceptance.md rev 1 直接开工。** 三条理由：

1. **头号任务（AC-S-088）的验收判据本身是坏的。** 「8 处、5 个文件」少算了 62 处；扫描方法漏掉 54 处；
   缺口清单还指向了日志行（`:185`）而不是真正的 title（`:167`）。
   **照 rev 1 修完并跑绿，中文 title 仍会有 62 处残留** —— 这正是 REQ-012 复核时抓到的
   「声称覆盖但其实没覆盖」的同一类问题。

2. **三条 AC 建立在不存在的东西上**：AC-S-089 的「前端翻译资源」（不存在）；
   AC-S-099 步骤 4 的「SSR」（`apps/shell` 是纯 CSR）；AC-S-094 的「以租户身份取时间」（后端无 tzdb）。
   这些不是「实现没做」，是**目标本身不可达**。

3. **两条 AC 会不可逆地误导 Dev**：AC-S-098 步骤 3（`ja-JP` + `CNY` → `￥1,234`）是**实测为假**的期望；
   AC-S-093 步骤 4（非法值 400）要求的能力（config 值校验器）**设计里完全没有**。
   前者会让 Dev 去自造符号映射，后者会让 Dev 卡住或偷偷跳过。

**建议路径**：先出 `acceptance.md` rev 2（按 §8.1 的 15 条高优先级修订，重点是 #1/#2/#3/#4/#5/#13/#14），
再把 §6 的 9 个待钉死项补进 design，然后开工。

> **本轮用例集可直接复用于 rev 2** —— 95 条用例的期望值都是**按源码事实与实测运行时行为**写的，
> 不依赖 rev 1 的错误表述。rev 2 落地后只需同步 §8 的裁定状态，不需要重写用例。

---

## 9. AC → 用例映射

| AC | 用例 | 条数 |
|---|---|---|
| AC-S-088 | TC-021-001 ~ 009 | 9 |
| AC-S-089 | TC-021-010 ~ 016 | 7 |
| AC-S-090 | TC-021-017 ~ 025 | 9 |
| AC-S-091 | TC-021-026 ~ 031 | 6 |
| AC-S-092 | TC-021-032 ~ 039 | 8 |
| AC-S-093 | TC-021-040 ~ 048 | 9 |
| AC-S-094 | TC-021-049 ~ 055 | 7 |
| AC-S-095 | TC-021-056 ~ 060 | 5 |
| AC-S-096 | TC-021-061 ~ 069 | 9 |
| AC-S-097 | TC-021-070 ~ 078 | 9 |
| AC-S-098 | TC-021-079 ~ 086 | 8 |
| AC-S-099 | TC-021-087 ~ 095 | 9 |
| **合计** | **TC-021-001 ~ 095** | **95** |

---

## 10. 用例与挑刺的对应关系（哪些用例守的是修订稿）

| 用例 | 守的是 | 对应的挑刺 |
|---|---|---|
| TC-021-001 | 表驱动全量 ASCII（替代字面量扫描） | #1、#2、#33 |
| TC-021-002 | 13 处直接字面量清零（含阳性对照） | #1、#34 |
| TC-021-005 | `not_found` 的 title 是 `Not Found` | #3 |
| TC-021-009 | 中文只允许在 tracing 日志 | #3、#18 |
| TC-021-011 | 翻译资源文件存在（守卫） | #4 |
| TC-021-015 | 全部错误码都有键 | #19 |
| TC-021-016 | SDK 透出 `params`（守卫） | #4 |
| TC-021-020 | 用户偏好压过请求头 | #6 |
| TC-021-022 | q 值 + 未注册的交互 | #6 |
| TC-021-025 | 协商结果的观测通道（守卫） | #5、#24 |
| TC-021-028 | 键名 == 全部后端 `code()` 返回值 | #21 |
| TC-021-035 | 两种语言都缺 → **"?"**（不回退到最后一段） | #7、#36 |
| TC-021-039 | 回退链 **3 级**全验（目标 → 基准 → "?"） | #7 |
| TC-021-044/045 | 非法 timezone/currency → 400（守卫） | #8 |
| TC-021-048 | space 级写被拒（scope 语义） | #23 |
| TC-021-051 | 用 `formatToParts` 重建 epoch | #25 |
| TC-021-053 | 后端原值仍是 UTC | #9 |
| TC-021-054 | DST 边界 | #26 |
| TC-021-055 | 时区归前端（守卫） | #9 |
| TC-021-057/060 | 用户偏好存储载体（守卫） | #10、#41 |
| TC-021-061 | 2^53+1 证明整数存储 | #11 |
| TC-021-065/077 | 溢出返回 `Err(Overflow)` | #11、#12 |
| TC-021-076 | 银行家舍入 4 个 `.5` 边界 | #12 |
| TC-021-079/080/081/082 | 币种与 locale 正交（修正 AC 的期望值） | #13 |
| TC-021-090 | SSR 不存在（守卫） | #14 |
| TC-021-091 | `subscribe` 触发重渲染 | #30、#31 |
| TC-021-094 | 持久化载体与 D99 的矛盾（守卫） | #41 |
