# 设计摘要：REQ-021 国际化与本地化

- **需求编号**: REQ-021
- **状态**: 草稿（2026-11-19 19:30）
- **验收标准**: [acceptance.md](acceptance.md)（AC-S-088 ~ AC-S-099）
- **基线**: `develop` @ `88b7ca3`（REQ-012 合并点）
- **用户裁定**（2026-11-19 19:00）：
  1. 币种：**只做工具 + 汇率表，不动业务表**
  2. 通知/导出：**明确延后**

---

## 1. 本需求的定位

**国际化不是加个语言切换按钮** —— 它是「**同一份数据，在不同语言/时区/币种下都有唯一正确的展示**」。

本需求最重的一块其实不是新功能，而是**修一个既有缺口**：

> **错误体的 `title` 是中文**（`problem.rs:125` `"未认证"`、`:146` `"权限不足"`、
> `:185` `"请求被拒绝"`，共 **8 处、5 个文件**）。
>
> 这**直接违反**「接口不返回面向用户的中文句子」——
> 前端 i18n 做得再好，也有一半文案来自后端。**修它是本需求的立身之本。**

### 1.1 现状核实（起草时逐个核过）

| 能力 | 现状 |
|---|---|
| 错误体 `title` | **中文**，8 处，跨 core/iam/rbac/org/config |
| `detail` | 英文 ASCII（**已对**，契约 C6） |
| `code` + `params` | **已有**（RFC 9457 扩展） |
| 时间 | RFC 3339 UTC 字符串（**已对**，契约 C13） |
| 用户表语言/时区字段 | **无** |
| 租户级 locale/tz/currency 配置 | **无**（但 REQ-010 的 config 能承载，`scope: tenant`） |
| `Money` / 汇率表 | **完全不存在** |
| `@base/i18n` | 2 个函数（`translate` / `hasMessage`） |

---

## 2. 后端零翻译：改 `title` 为英文

### 2.1 改法

| 字段 | 改成 | 理由 |
|---|---|---|
| `title` | **英文**（`"Unauthenticated"` / `"Insufficient Permission"`） | RFC 9457 的 `title` 是「短的人类可读摘要」，**不是**特定语言的本地化文案 |
| `detail` | 保持英文 ASCII | 已对 |
| `code` | 保持稳定码 | 前端按它渲染 |

**前端**按 `code` + `params` 渲染面向用户的文案。

### 2.2 影响面（必须全改，不许遗漏）

8 处中文 title，分布在 5 个文件：
`platform-core/src/problem.rs`、`platform-iam/src/http.rs`、
`platform-rbac/src/http.rs`、`platform-org/src/http.rs`、
`platform-config/src/error.rs`

**同时要改**：断言中文 title 的测试（现有 8 处相关断言）。

### 2.3 允许中文的地方（避免过度整改）

| 位置 | 允许中文 | 理由 |
|---|---|---|
| `tracing` 日志 | ✅ | 契约 C6：运维说明放日志 |
| `params` 里的用户数据 | ✅ | 那是**数据**不是**文案** |
| 代码注释与文档 | ✅ | — |
| **响应体任何字段** | ❌ | 本轮核心 |

---

## 3. 语言协商

### 3.1 ★ 优先级（写死，回应 AC-S-090 的待定项）

```
用户显式偏好  >  Accept-Language  >  租户配置  >  平台默认
```

> **为什么用户偏好排在 Accept-Language 前面**（我起草时写反了）：
> **用户在我们系统里显式设的偏好最权威** —— 他设了 English 就该看英文，
> 不管浏览器语言是什么。`Accept-Language` 只是「用户没设时的最佳猜测」。
>
> 把浏览器语言排在用户显式选择之上，等于**用户的选择随时被浏览器覆盖**，
> 那是明显的体验错误。

### 3.2 未注册语言的回退

```
请求的 locale 未注册 → 视为「没设」，落到下一级
```

**不允许**返回未注册的 locale（否则前端加载不到翻译包，全是回退）。

### 3.3 时区协商（**不含请求头**）

```
用户偏好 > 租户配置 > 平台默认
```

> **为什么不看 `Accept-Language`**：那是**语言**头，不含时区。
> 用 `Intl.DateTimeFormat().resolvedOptions().timeZone` 猜测是**前端的**事，
> 后端**不猜** —— 猜错会让「跨时区报表口径」出问题。
> 前端可以在**首次**把浏览器时区作为**用户偏好的初值**（一次性的），
> 之后以用户显式选择为准。

---

## 4. 时区：只影响展示

| 层 | 约定 |
|---|---|
| 存储 | `timestamptz`（UTC）—— **已对** |
| 传输 | RFC 3339 UTC 字符串（`2026-01-15T00:00:00Z`）—— **已对** |
| 展示 | 按 `用户 > 租户 > 平台` 的时区，用 `Intl` 渲染 |

> **关键**：时区**不改后端返回的值**。后端永远返回 UTC。
> 这样「跨时区报表口径一致」是**天然的** —— 因为口径是 UTC，
> 只有展示层不同。

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
- 溢出返回**错误**（`checked_add` / `checked_mul`），**不静默回绕**
- 从 `major`（"12.34"）解析时用**字符串解析**，不走 `parse::<f64>()`

> **为什么不引 `rust_decimal`**：它要新增第三方依赖（违反零新依赖）；
> 而「最小单位整数」已经能精确表达任何货币金额 —— **整数就是最精确的小数**。

### 5.2 货币指数（`exponent`）

不同货币的小数位数不同：

| 货币 | exponent | 示例 |
|---|---|---|
| CNY / USD / EUR | 2 | `123456` → `1,234.56` |
| JPY / KRW | 0 | `123456` → `123,456` |

**必须按 `exponent` 转换** —— 用错会在 JPY 上多两位小数（AC-S-098 的判据）。

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
- 换算结果带 `asOf` + `source`（可追溯，AC-S-097）
- **不存在的币种对 → 错误**（`FX_RATE_NOT_FOUND`），不是 0 或原值

### 5.4 舍入规则（写死，回应 AC-S-097 的待定项）

**银行家舍入**（round half to even）：

```
2.5 → 2    3.5 → 4    0.5 → 0    1.5 → 2
```

> **为什么不是「四舍五入」**：`round half up` 在大量累加时会**系统性偏向**（总是向上），
> 银行家舍入是金融行业的默认，因为它在统计上**无偏**。
> 规则写死并有用例覆盖 `.5` 边界。

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

**`domain` 是固定枚举**，不是随手写的字符串。

### 6.2 错误键与后端 `code` 逐字一致

```
后端 code "UNAUTHENTICATED"  →  前端键 "error.UNAUTHENTICATED"
```

这样「新增一个错误码」时，**键名是机械可推的**，不需要查表。

### 6.3 ★ 缺失键的记录（写死，回应 AC-S-092 的待定项）

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

### 6.4 回退链

```
目标语言 → 基准语言（zh-CN）→ 键的最后一段 → "?"
```

**不允许**回落到**完整 key 原文**（`common.save`）——
那向用户泄露实现细节（AC-S-092 步骤 5）。

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
  /** 翻译；缺失时按回退链，并记录。 */
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
- `formatMoney` 必须按 `exponent` 缩放（`minorUnits / 10^exponent` 用整数处理）
- CSR 与 SSR 用**同一个** `t()`（否则会出现「首屏中文、水合后英文」）

### 7.2 语言切换的持久化

- 用户级偏好存 `sessionStorage`（与 REQ-011 的 D99 一致）
- 首次进入时：用户偏好 → `navigator.language` → 租户配置 → 平台默认

---

## 8. 测试策略

| 层 | 内容 |
|---|---|
| Rust 单元 | `Money` 运算（含溢出/舍入）、汇率换算、`Accept-Language` 解析 |
| Rust 集成 | 错误体零中文（6 类错误）、租户配置读写、时区协商 |
| 前端单元 | `t()` 的回退链、`missingKeys()`、`formatMoney` 的 exponent、`formatDate` 跨日 |
| E2E | 语言切换（切换后已渲染的组件更新）、时区渲染（两个租户不同） |

### 8.1 本仓的假绿清单（前几轮教训，全部适用）

| 陷阱 | 正确写法 |
|---|---|
| 断言「非空」当「正确」 | `translate` 的 fallback 也非空 → 断「**不是 key 本身**」 |
| 断言「差值 < eps」 | 金额必须断**整数相等** |
| 时区用同日例子 | 必须**跨日**（`01-15T00:00Z` → 纽约 `01-14`） |
| 只跑单测 | 应用级行为必须真浏览器 |
| 夹具不够/不对 | 独立幂等播种（D101）+ 403 必须 fail（D108） |
| `skip` 当通过 | 同上 |

---

## 9. 风险

| # | 风险 | 缓解 |
|---|---|---|
| R1 | 改 8 处中文 title 会影响既有测试 | 逐处改 + 跑全量；`grep` 复核无残留 |
| R2 | 改 `title` 可能破坏 REQ-011 的 E2E（若断言了中文） | 先 `grep` 找断言点 |
| R3 | `Money` 用 `i64` 有上限（约 9.22×10^18 最小单位 ≈ 9.2×10^16 元） | 超限返回错误；文档写明上限 |
| R4 | 汇率快照的「用哪一条」在并发下可能不一致 | 换算时**显式传** `asOf`，不隐式取「最新」 |
| R5 | 前端 SSR/CSR 不一致导致水合闪烁 | AC-S-099 步骤 4 逐字比对 |
| R6 | 依赖预算 | **零新 Rust 依赖**（`i64` 不需要 `rust_decimal`）；前端零新增（`Intl` 是内建） |
| R7 | 语言包体积（每语言一份 JSON） | 本轮 2 种语言，包很小；按需加载留到有需要时 |

---

## 10. 不做（与 demand.md 一致）

通知/邮件的本地化渲染、导出文件的本地化、业务模块领域术语、第三方翻译服务、
金额进业务表、复数规则、RTL 布局。
