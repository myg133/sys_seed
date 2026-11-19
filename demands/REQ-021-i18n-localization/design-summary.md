# 设计摘要：REQ-021 国际化与本地化

- **需求编号**: REQ-021
- **状态**: 草稿（2026-11-19 20:05，**rev 2** —— 按 42 条裁定重写）
- **验收标准**: [acceptance.md](acceptance.md)（AC-S-088 ~ AC-S-099，rev 2）
- **裁定记录**: [acceptance-rev2-ruling.md](acceptance-rev2-ruling.md)
- **基线**: `develop` @ `88b7ca3`（REQ-012 合并点）
- **用户裁定**（2026-11-19 19:00）：
  1. 币种：**只做工具 + 汇率表，不动业务表**
  2. 通知/导出：**明确延后**

> **rev 1 → rev 2 的重写说明**：rev 1 的这份 design 里有多处**已被裁定推翻**的内容
> （「8 处中文 title」「回退到键的最后一段」「CSR/SSR 一致」「后端做时区渲染」）。
> rev 2 逐条改掉，**不保留旧写法作为「备选」** —— 留着就会被照着实现。

---

## 1. 本需求的定位

**国际化不是加个语言切换按钮** —— 它是「**同一份数据，在不同语言/时区/币种下都有唯一正确的展示**」。

本需求最重的一块不是新功能，而是**修一个既有缺口**：

> **错误体的 `title` 是中文**，且**不是 8 处，是 70 处**。
>
> 这**直接违反**「接口不返回面向用户的中文句子」——
> 前端 i18n 做得再好，也有一半文案来自后端。**修它是本需求的立身之本。**

### 1.1 现状核实（rev 2 用独立脚本重算）

| 能力 | 现状 |
|---|---|
| 错误体 `title` | **中文**，**70 处**（**55 处由函数产出 + 15 处构造点字面量**），跨 core/iam/rbac/org/config |
| `detail` | 英文 ASCII（**已对**，契约 C6） |
| `code` + `params` | **已有**（RFC 9457 扩展）；**四个 crate 都有 `pub fn code() -> &'static str`** |
| 时间 | RFC 3339 UTC 字符串（**已对**，契约 C13） |
| 用户表语言/时区字段 | **无** |
| 租户级 locale/tz/currency 配置 | **无**（但 REQ-010 的 config 能承载，`scope: tenant`） |
| `Money` / 汇率表 | **完全不存在** |
| `@base/i18n` | 2 个函数（`translate` / `hasMessage`） |
| 后端 tzdb | **无**（`time` crate 只启用 `formatting`） |
| 协商结果观测点 | **无**（全仓 `locale` 0 命中、无 `Content-Language`） |

#### 1.1.1 ★ 70 处的统计方法与落点（rev 1 在这里错得最严重）

rev 1 写「**8 处 / 5 文件**」，实际 **70 处**（差近 9 倍）；
更致命的是 rev 1 的**验收方式**是「扫 `ProblemDetails` 构造点的字符串字面量」——

> **55 处由 `title_of()` / `title()` 函数产出**，构造点拿到的是**函数调用**。
> 照 rev 1 的写法**扫完 0 命中、全部通过**。这不是数错，是**必然漏报**。

**rev 2 的统计口径**（排除 `//` 注释，追踪 `fn *title*` 函数体 + 构造点）：

```
fn title      platform-config/src/error.rs     12
fn title_of   platform-iam/src/http.rs         17
fn title_of   platform-org/src/http.rs         15
fn title_of   platform-rbac/src/http.rs        10
fn title      platform-org/src/notify.rs        1
                                    函数产出合计  55
                             构造点直接字面量    15
                                        总计    70
```

**结论**：不能靠「扫构造点」，只能靠**收敛到一张表**（§2）。见 D113。

---

## 2. 后端零翻译：`title` 收敛到 `TITLES` 表（D113）

### 2.1 唯一来源

```rust
// crates/platform-core/src/titles.rs（新文件）
/// 稳定错误码 → **英文**标题。
///
/// **唯一来源**：任何 `ProblemDetails` 的 `title` 都必须经 `title_of_code` 从这里取。
/// 表内只允许 ASCII（AC-S-088 静态判据）。
pub const TITLES: &[(&str, &str)] = &[
    ("UNAUTHENTICATED", "Unauthenticated"),
    ("INSUFFICIENT_PERMISSION", "Insufficient Permission"),
    ("NOT_FOUND", "Not Found"),
    // … 覆盖全部 code
];

/// 查表；未登记的 code 返回 ASCII 兜底 `"Error"`（**不返回 code 本身**）。
pub fn title_of_code(code: &str) -> &'static str;
```

### 2.2 各 crate 的改法

**四个 crate 都已经有 `pub fn code(&self) -> &'static str`**（`platform-iam/src/error.rs:147`、
`platform-org/src/error.rs:132`、`platform-org/src/notify.rs:126`、`platform-rbac/src/error.rs:107`、
`platform-config/src/error.rs:130`）→ 收敛在结构上**可行**：

| 现状 | 改成 |
|---|---|
| `fn title_of(error: &IamError) -> String { match … }` | `fn title_of(error: &IamError) -> &'static str { title_of_code(error.code()) }` |
| `fn title_of(error: &OrgError) -> &'static str { match … }` | 同上 |
| `fn title_of(error: &RbacError) -> &'static str { match … }` | 同上 |
| `fn title(&self) -> &'static str { match … }`（config / org-notify） | 改为 `title_of_code(self.code())` |

**5 个 `match` 里的 55 条中文字面量全部删除**，只剩转发。

### 2.3 ★ 防「表漏项」的机械保障（这是 D113 的关键补充）

只把 55 处搬到一张表里，**并不能保证一个不漏** —— 漏了就是未登记的 code 静默走 ASCII 兜底。
所以要求每个 crate **导出自己的 code 清单**，并加一个**跨 crate 的完备性测试**：

```rust
// 每个 crate 的 error 模块
pub const ALL_CODES: &[&str] = &[ /* 该 crate 全部 code */ ];
```

```rust
// platform-core 的测试（或 platform-server 的集成测试）
#[test]
fn titles_cover_every_code() {
    for code in IAM_CODES.iter().chain(ORG_CODES).chain(RBAC_CODES).chain(CONFIG_CODES) {
        assert!(TITLES.iter().any(|(k, _)| k == code), "缺 title: {code}");
    }
}
#[test]
fn titles_are_ascii_and_unique() {
    // ① 每个 title 值 is_ascii()；② 没有重复 key
}
```

> **为什么必须有 `ALL_CODES`**：`code()` 是 `match` 里的 `&'static str`，
> 运行时无法自动枚举。没有显式清单，**「表漏一项」永远不会被测出来** ——
> 那正是本轮要消灭的那类假绿。

### 2.4 字段约定

| 字段 | 约定 | 理由 |
|---|---|---|
| `title` | **英文 ASCII**（从 `TITLES` 取） | RFC 9457 的 `title` 是「短的人类可读摘要」，**不是**特定语言的本地化文案 |
| `detail` | 保持英文 ASCII | 已对 |
| `code` | 保持稳定码 | 前端按它渲染 |
| `params` | 已有，需补全 | 前端按 `code` + `params` 渲染 |

### 2.5 影响面（rev 1 这里也错了）

rev 1 写「同时要改**现有 8 处断言**」—— **不存在**。既有测试只比较两个响应的 title **相等**，
不断言具体中文。实际影响面：

| 位置 | 影响 |
|---|---|
| `crates/platform-iam/tests/iam_real_db.rs:468` | 比较两响应 title **相等** → 改后仍成立，**不需要改** |
| `packages/sdk/test/*.ts` | fixture 里有中文 title → **需要更新为英文** |
| E2E | 需 `grep` 确认没有断言中文 title（R2） |

### 2.6 允许中文的地方（避免过度整改）

| 位置 | 允许中文 | 理由 |
|---|---|---|
| `tracing` 日志 | ✅ | 契约 C6：运维说明放日志 |
| `params` 里的用户数据 | ✅ | 那是**数据**不是**文案** |
| 代码注释与文档 | ✅ | — |
| **响应体任何字段** | ❌ | 本轮核心 |

---

## 3. 语言协商

### 3.1 ★ 优先级（写死）

```
用户显式偏好  >  Accept-Language  >  租户配置  >  平台默认
```

> **为什么用户偏好排在 `Accept-Language` 前面**（rev 1 写反了）：
> **用户在我们系统里显式设的偏好最权威** —— 他设了 English 就该看英文，
> 不管浏览器语言是什么。`Accept-Language` 只是「用户没设时的最佳猜测」。
>
> 把浏览器语言排在用户显式选择之上，等于**用户的选择随时被浏览器覆盖**。

### 3.2 未注册语言的回退

```
请求的 locale 未注册 → 视为「没设」，落到下一级
```

**「已注册」的判定来源钉死**（裁定 #16）：`packages/i18n/src/locales/*.json` 的文件名
（如 `zh-CN.json` / `en-US.json` → 已注册 `zh-CN` / `en-US`）。不用代码常量、不用数据库。

**不允许**返回未注册的 locale（否则前端加载不到翻译包，全是回退）。

**`Accept-Language` 解析规则**（钉死）：

| 输入 | 结果 |
|---|---|
| `zh-CN;q=0.9, en-US;q=0.8` | 按 `q` **降序**取第一个已注册的 |
| `*` | → **平台默认**（**不做**「任意匹配」），裁定 #23 |
| `q=0` | 该语言被**明确拒绝**，跳过，裁定 #24 |
| 未带该头 | 视为没设 |

### 3.3 ★ 观测通道（D110，rev 1 完全没有）

```
GET /api/i18n/context  →  { locale, timezone, currency, source }
source ∈ { user, accept-language, tenant, default }
```

> **为什么必须带 `source`**：rev 1 的 7 步全写「断言协商结果」，但
> ① 全仓**没有观测点**（`locale` 0 命中、无 `Content-Language`）；
> ② **光有 `locale` 值区分不出「哪一级生效」** —— 如果用户偏好与请求头恰好相同，
> 值一样但优先级没被验证过。`source` 让「哪一级生效」机械可判。

**时区的 `source`**：时区协商**不含请求头**（§3.4），故其 `source ∈ { user, tenant, default }`
（**永不出现 `accept-language`**）。

### 3.4 时区协商（**不含请求头**）

```
用户偏好 > 租户配置 > 平台默认
```

> **为什么不看 `Accept-Language`**：那是**语言**头，不含时区。
> 用 `Intl.DateTimeFormat().resolvedOptions().timeZone` 猜测是**前端的**事，
> 后端**不猜** —— 猜错会让「跨时区报表口径」出问题。
> 前端可以在**首次**把浏览器时区作为**用户偏好的初值**（一次性的），
> 之后以用户显式选择为准。

### 3.5 ★ 用户偏好只在**前端**（D112 / 裁定 #10）

| 层 | 存什么 |
|---|---|
| **后端** | 只有**租户级**（config）+ 平台默认。**协商只看请求头 + 租户配置** |
| **前端** | 用户级 `locale` / `timezone` / `currency` 存 `sessionStorage`（与 D99 一致） |

**用户表不加列** —— 要「跨设备同步」需另立需求（建列 + 迁移）。

**因此**：AC-S-090 步骤 4/8 与 AC-S-095 步骤 2 的「用户 > 租户」——
**精确地说**：后端接受一个**显式传参**（如 `?locale=` 或请求体字段）作为「用户偏好」这一级，
**但服务端不持久化、不跨请求记忆**。前端负责把 `sessionStorage` 的偏好带上来。

> ⚠️ 这条**必须两端一致**：如果后端不做「显式传参」这一级，
> 则 `source === "user"` 永远不可能出现，AC-S-090 步骤 8 **永远无法通过**。
> **实现时选一种并写进代码注释**：推荐**查询参数 `?locale=` / `?timezone=`**（简单、可测、不持久化）。

---

## 4. 时区：只影响展示，且**只在前端渲染**（D114）

| 层 | 约定 |
|---|---|
| 存储 | `timestamptz`（UTC）—— **已对** |
| 传输 | RFC 3339 UTC 字符串（`2026-01-15T00:00:00Z`）—— **已对** |
| 校验 | 后端只校验**值的格式**（D111 的 `iana-timezone`） |
| 展示 | **只在前端**，用 `Intl.DateTimeFormat(..., { timeZone })` |

> **为什么后端不渲染**（rev 1 曾打算后端也做）：`time` crate **只启用 `formatting`，无 tzdb**；
> 加 tzdb（`time-tz` / `chrono-tz`）**违反零新依赖**；而**浏览器自带完整 tzdb**，
> 后端再引一份是重复且要维护。且「存 UTC + 展示时转」是本仓既定口径（契约 C13）。

**关键**：时区**不改后端返回的值**。后端永远返回 UTC →
「跨时区报表口径一致」是**天然的**，只有展示层不同。

**AC-S-094 必须用跨日的例子**（`2026-01-15T00:00:00Z` → 纽约是 `01-14 19:00`），
否则「时区生效」与「时区没生效但日期相同」无法区分。

---

## 5. 币种：`Money` + 汇率快照

### 5.1 `Money` 类型

```rust
/// 金额。**整数最小单位**，零浮点。
pub struct Money {
    /// 最小单位（如人民币的「分」、日元的「円」）。
    minor_units: i64,
    /// ISO 4217 三位代码（如 CNY / USD / JPY）。
    currency: CurrencyCode,
}
```

**硬规则**：
- 字段是 `i64`，**不是** `f64` / `rust_decimal`
- 运算路径上**没有**浮点
- 算术返回 `Result<Money, MoneyError>`；溢出 → `MoneyError::Overflow` → HTTP **`MONEY_OVERFLOW`（400）**，
  **不是** panic、**不是**静默回绕（裁定 #11）
- 从 `major`（`"12.34"`）解析时用**字符串解析**，不走 `parse::<f64>()`

> **为什么不引 `rust_decimal`**：它要新增第三方依赖（违反零新依赖）；
> 而「最小单位整数」已经能精确表达任何货币金额 —— **整数就是最精确的小数**。
>
> **上限**：`i64` 约 9.22×10^18 最小单位 ≈ 9.2×10^16 元。超限返回错误，文档写明上限（R3）。

### 5.2 货币指数（`exponent`）—— 钉死（裁定 #20）

| 货币 | exponent | 示例 |
|---|---|---|
| CNY / USD / EUR | 2 | `123456` → `1,234.56` |
| JPY / KRW | 0 | `123456` → `123,456` |
| 其余 | **默认 2** | — |

**必须按 `exponent` 转换** —— 用错会在 JPY 上多两位小数（AC-S-098 的判据）。

> **★ rev 2 核心认知（裁定 #13）**：**小数位数由货币的 `exponent` 决定，不是 locale**。
> `ja-JP` 显示 **CNY** 仍是 **2 位小数**（实测 `"元 1,234.56"`）；
> 显示 **JPY** 才是 0 位（`"￥123,456"`）。rev 1 写「`ja-JP` → `￥1,234`」是**错的**。

### 5.3 汇率快照表

```sql
platform_fx_rate (
  id, from_currency, to_currency,
  rate_micros bigint,     -- 汇率 × 10^6，整数存储
  as_of timestamptz,      -- 汇率生效时刻（快照标识）
  source text,            -- 来源（如 "ecb" / "manual"）
  created_at timestamptz
)
```

- 汇率也用**整数**（`rate_micros`），避免浮点
- 换算结果带 `asOf` + `source`（**可追溯**，AC-S-097 步骤 4）
- **不存在的币种对 → `FX_RATE_NOT_FOUND`，404**（裁定 #21 定 404，不是 400）—— **不是** 0、**不是**原值
- 换算时**显式传 `asOf`**，不隐式取「最新」（R4：并发下不自觉）

### 5.4 ★ 舍入规则（写死，裁定 #12）

**银行家舍入**（round half to even）：`2.5 → 2`、`3.5 → 4`、`0.5 → 0`、`1.5 → 2`

**整数版算法（必须逐字照做）**：

```rust
let numerator = a_minor as i128 * rate_micros as i128;   // ← 必须 i128！
let quotient  = numerator / 1_000_000;
let remainder = numerator % 1_000_000;
let mut out = quotient;
if remainder * 2 > 1_000_000 { out += 1; }
else if remainder * 2 == 1_000_000 { /* 偶数化：quotient 为奇数才 +1 */ if out % 2 != 0 { out += 1; } }
let result: i64 = out.try_into().map_err(|_| MoneyError::Overflow)?;
```

> ⚠️ **中间量必须是 `i128`**：两个 `i64` 相乘可达 ~8.5×10^37，
> **`i64` 会溢出**（上限 9.2×10^18）。rev 1 没写宽度，Dev 会踩。
>
> ⚠️ **负数**：`/` 与 `%` 在 Rust 里向零截断，负数余数为负 →
> `remainder * 2 > 1_000_000` 对负数**永不成立** → 「偶数化」分支也不会走到 →
> **负数会系统性向零舍入（偏向零侧）**。必须先断言/归一化符号，
> 或对 `remainder` 取绝对值再判定后按符号回填。**这条必须有用例**。

> **为什么不是「四舍五入」**：`round half up` 在大量累加时会**系统性偏向**（总是向上），
> 银行家舍入是金融行业的默认，因为它在统计上**无偏**。

---

## 6. 翻译键体系

### 6.1 命名规则

```
<domain>.<KEY>
```

| domain | 内容 | 示例 |
|---|---|---|
| `error` | 错误码对应的文案 | `error.UNAUTHENTICATED` |
| `common` | 通用按钮/标签 | `common.save` / `common.cancel` |
| `schema` | 渲染器文案 | `schema.emptyTable` |
| `nav` | 导航 | `nav.workbench` |

**`domain` 是固定枚举**（**四值**，裁定 #19）：`error` / `common` / `schema` / `nav`。
其余一律不合规（不是随手写的字符串）。

### 6.2 错误键与后端 `code` 逐字一致

```
后端 code "UNAUTHENTICATED"  →  前端键 "error.UNAUTHENTICATED"
```

这样「新增一个错误码」时，**键名是机械可推的**，不需要查表。

### 6.3 ★ 缺失键的记录（写死，裁定 #18）

**选「可注入回调 + 导出缺失集合」两条都做**：

```ts
export interface I18nOptions {
  /** 缺失键回调（默认 write 一条 console.warn）。 */
  onMissingKey?: (key: string, locale: string) => void;
}
/** 当前会话内累计的缺失键（去重）。 */
export function missingKeys(): ReadonlySet<string>;
```

**理由**：
- `console.warn` 只满足「人能看到」，**测试抓不到**；
- 导出 `missingKeys()` 让**测试与 CI 能机械断言**；
- 两者都做，成本极低。

### 6.4 ★ 回退链（rev 2 改写，裁定 #7）

```
目标语言 → 基准语言（zh-CN）→ "?"
```

**已删除「键的最后一段」这一级**（rev 1 有，被推翻）：

> `error.INSUFFICIENT_PERMISSION` 的最后一段就是错误码本身；
> 英文界面里蹦出 `PERMISSION` 或 `save` 毫无意义，**反而泄露实现细节**。
> `"?"` 诚实且不泄露。

**不允许**回落到**完整 key 原文**（`common.save`）——
那向用户泄露实现细节（AC-S-092 步骤 5）。

### 6.5 ★ 基准语言

基准语言是 **`zh-CN`**，且它必须是**唯一**的最终回退源（除 `"?"`）。
**「已注册语言」清单**（§3.2）与**「有翻译包的目录」**是同一份东西，不允许两处维护。

---

## 7. 前端

### 7.1 `@base/i18n` 扩展（包出逻辑）

```ts
export interface Locale { readonly code: string; readonly label: string; }
export function createI18n(options: I18nOptions): I18n;
export interface I18n {
  readonly locale: string;
  /** 切换语言（触发订阅者更新）。 */
  setLocale(code: string): void;
  /** 翻译；缺失时按 §6.4 回退链，并记录。 */
  t(key: string, params?: Record<string, string | number>): string;
  /** 按 locale + 时区格式化时间。 */
  formatDate(iso: string, timezone?: string): string;
  /** 按 locale + 货币 exponent 格式化金额。 */
  formatMoney(minorUnits: number, currency: string): string;
  /** 订阅变化（组件重新渲染用）。 */
  subscribe(fn: () => void): () => void;
}
```

**硬规则**：
- 格式化**用 `Intl`**（`Intl.DateTimeFormat` / `Intl.NumberFormat`），**不自造**千分位
- `formatMoney` 必须按 §5.2 的 `exponent` 缩放（`minorUnits / 10^exponent` 用**整数**处理）
- **`t()` 是纯函数**，且**组件不自己拼字符串** —— 这是 AC-S-099 判据的全部（见 §7.3）

### 7.2 语言切换的持久化

- 用户级偏好存 `sessionStorage`（与 REQ-011 的 D99 一致）
- 首次进入时：`sessionStorage` 用户偏好 → `navigator.language` → 租户配置（来自 `GET /api/i18n/context`）→ 平台默认
- 切换后必须**通知订阅者** → 已渲染的组件**立即更新**（AC-S-099 步骤 5）

### 7.3 ★ 删除「SSR/CSR 一致」（裁定 #14）

`apps/shell` 是**纯 CSR**（`mount()` + 无 Kit + 无 ssr 配置）→ 「SSR 与 CSR 逐字相同」
**永远无法验**。rev 1 的这条是**物理不可达的前置**（与 REQ-012 的教训同类）。

**改为「单一实现」**：只要 `t()` 是纯函数、且组件不自己拼字符串，**就不存在两套**。
验收方式：`grep` 组件源码确认**没有硬编码文案字面量**（全部走 `t()`）。

### 7.4 `@base/sdk` 的 `params`（D109 跨需求改动）

`ApiError`（`packages/sdk/src/index.ts:46-64`）现在只有 `code` / `status` / `title` / `detail`，
**没有 `params`** → 前端拿不到参数，渲染不出完整文案。

**本轮要加 `params` 字段** —— 这是**修改 REQ-011 的交付物**，
已按 D109 在 `demand.md` 的依赖段声明，且**提交信息里必须写明**。

---

## 8. 测试策略

| 层 | 内容 |
|---|---|
| Rust 单元 | `Money` 运算（含溢出 / 银行家舍入 / **负数**）、汇率换算、`Accept-Language` 解析、`TITLES` 完备性 |
| Rust 集成 | 错误体零中文（遍历 `ALL_CODES`）、租户配置读写 + `format` 校验、`/api/i18n/context` 的 `source` |
| 前端单元 | `t()` 的回退链（断「不是 key 本身」）、`missingKeys()`、`formatMoney` 的 exponent、`formatDate` 跨日 |
| E2E | 语言切换（切换后已渲染的组件更新）、时区渲染（两个租户不同）、`source` 可观测 |

### 8.1 本仓的假绿清单（前几轮教训，全部适用）

| 陷阱 | 正确写法 |
|---|---|
| 断言「非空」当「正确」 | `translate` 的 fallback 也非空 → 断「**不是 key 本身**」 |
| 断言「差值 < eps」 | 金额必须断**整数相等** |
| 时区用同日例子 | 必须**跨日**（`01-15T00:00Z` → 纽约 `01-14`） |
| 只跑单测 | 应用级行为必须真浏览器 |
| 夹具不够/不对 | 独立幂等播种（D101）+ 403 必须 fail（D108） |
| `skip` 当通过 | 同上 |
| **扫「构造点字面量」断言零中文** | **必须扫 `TITLES` 表 + 断言 `ALL_CODES` 全覆盖**（rev 1 的必然漏报） |
| **只断言 `locale` 值** | 必须同时断 `source`（D110） |
| **断言「SSR 与 CSR 一致」** | 纯 CSR 项目里不可达 → 改断「组件无硬编码文案」 |

---

## 9. 风险

| # | 风险 | 缓解 |
|---|---|---|
| R1 | 改 70 处中文 title 会影响既有测试 | 按 §2.5 的实际影响面处理；`grep` 复核无残留；`ALL_CODES` 完备性测试保底 |
| R2 | 改 `title` 可能破坏 REQ-011 的 E2E（若断言了中文） | 先 `grep` 找断言点 |
| R3 | `Money` 用 `i64` 有上限（约 9.22×10^18 最小单位 ≈ 9.2×10^16 元） | 超限返回 `MONEY_OVERFLOW`；文档写明上限 |
| R4 | 汇率快照的「用哪一条」在并发下可能不一致 | 换算时**显式传** `asOf`，不隐式取「最新」 |
| R5 | ~~前端 SSR/CSR 不一致导致水合闪烁~~ → **项目是纯 CSR，此风险不存在** | 改为：`t()` 纯函数 + 组件零硬编码文案 |
| R6 | 依赖预算 | **零新 Rust 依赖**（`i64` 不需要 `rust_decimal`；`i128` 是内建）；前端零新增（`Intl` 是内建） |
| R7 | 语言包体积（每语言一份 JSON） | 本轮 2 种语言，包很小；按需加载留到有需要时 |
| **R8** | **`source === "user"` 若后端不接显式传参，则 AC-S-090 步骤 8 永远不可通过** | §3.5 已钉死：后端支持 `?locale=` / `?timezone=` 作为「用户偏好」级（不持久化） |
| **R9** | **`TITLES` 漏项 → 静默走 ASCII 兜底，看起来「都通过了」** | §2.3 的 `ALL_CODES` 完备性测试 |

---

## 10. 不做（与 demand.md 一致）

通知/邮件的本地化渲染、导出文件的本地化、业务模块领域术语、第三方翻译服务、
金额进业务表、复数规则、RTL 布局、用户 i18n 偏好跨设备同步（需建列 + 迁移）。
