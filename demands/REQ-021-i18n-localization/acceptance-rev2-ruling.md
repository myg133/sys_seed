# BA 裁定：REQ-021 验收标准 rev 1 → rev 2

- **裁定者**: myg133-ba-1 (BA)
- **时间**: 2026-11-19 19:40
- **输入**: [test-cases/index.md](test-cases/index.md) §8（**42 条**：高 15 / 中 15 / 低 12）
- **结论**: **42 条全部接受**。rev 1 的 12 条 AC **逐条都要改**。

---

## 0. 我这次的错，比前几轮严重得多

| 我写的 | 实际 | 差多少 |
|---|---|---|
| 中文 title **8 处**、**5 个文件** | **70 处**、**7~12 个文件** | **近 9 倍** |
| 扫描方式：「检查 `ProblemDetails` 构造点的**字符串字面量**」 | **55 处**由 `title_of()` / `title()` **函数产出**，构造点拿到的是**函数调用** | **照我的写法扫完 0 命中、全部通过** |

**我用独立脚本核实了**（排除 `//` 注释后，追踪 `fn *title*` 函数体 + `ProblemDetails` 构造点）：

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

**这不只是「数错了」，是「验收方式本身假绿」** ——
照 rev 1 修完再跑我的检查，会显示「通过」，而 55 处中文原封不动。

> 这是本仓历史上**最严重的一次规格错误**。
> 前几轮的错误是「不可判定」或「必然误报」；**这次是「必然漏报」** ——
> 更危险，因为漏报不会有人发现。

---

## 1. 高 severity（15 条，全部接受）

### 1.1 ★ #1 + #2：扫描方式假绿 + 数字严重低估

**裁定**：**接受**，并且**改的不只是数字，是**结构**。

**新方案：把 title 收敛到一张集中映射表**

```rust
// crates/platform-core/src/problem.rs（或独立模块）
/// 稳定错误码 → **英文**标题。
///
/// **唯一来源**：任何 `ProblemDetails` 的 `title` 都必须从这里取。
pub const TITLES: &[(&str, &str)] = &[
    (CODE_UNAUTHENTICATED, "Unauthenticated"),
    (CODE_INSUFFICIENT_PERMISSION, "Insufficient Permission"),
    (CODE_NOT_FOUND, "Not Found"),
    // …
];
```

**各 crate 的 `title_of()` / `title()` 改成从这张表取**（或直接删除，构造点用 `title_of_code(code)`）。

**为什么这是对的**：
- 55 处分散在 5 个函数的 `match` 里 → **扫不干净**
- 收敛成**一张表** → **扫一张表就能验**，且**新增错误码只能往表里加**
- 表里只允许 ASCII → 机械可验

**AC-S-088 的新判据**（改写见 rev 2）：
1. **静态**：`TITLES` 表的**每一行**都不含 CJK；且**全仓**再没有第二处 title 文案来源
   （`grep` 确认 `title_of` / `fn *title*` 只剩转发）
2. **运行时**：遍历 `TITLES` 的**每一个 code**，触发对应错误，抓响应体 → 逐个断言无 CJK
   （比静态更可靠，但没有触发条件的 code 只能静态验）

### 1.2 #3：清单指向错行

**接受**。`:185` 是 `tracing` 日志（AC 自己允许中文），真正的第三个 title 是 `:167`「资源不存在」。

**落点**：rev 2 的缺口表改对行号，并**不再手工列清单**（改用「统计方法」描述，避免再次数错）。

### 1.3 #4：AC-S-089 的两个前置都不成立

**接受**：
- **无翻译资源文件**（前端还没有 locales）
- **`ApiError` 没有 `params` 字段** —— `packages/sdk/src/index.ts:46-64` 只有
  `code` / `status` / `title` / `detail`。前端**拿不到** `params.required`

**裁定**：
1. **AC-S-089 拆成两半**：
   - **089a（后端）**：错误体**必须**带 `params`，且参数完整（能验）
   - **089b（前端）**：`ApiError` 加 `params` 字段 + 翻译资源条目 → **需要改 `@base/sdk`**
2. **demand.md 的依赖补 `REQ-011`**（已写）**并新增说明**：
   「本需求**会修改** REQ-011 交付的 `@base/sdk`（加 `params` 字段）」——
   这是**跨需求改动**，必须在规格里声明，不能偷偷改

**新增决策 D109**：跨需求的包改动必须在 demand 的依赖段显式声明。

### 1.4 #5：无协商结果观测通道

**接受**。全仓 `locale` 0 命中、无 `Content-Language` —— 7 步「断言协商结果」**没有观测点**。

**裁定**：**新增一个可观测通道**。两个选项取其一（design 选定）：
- A：响应头 `Content-Language: <协商结果>`
- B：`GET /api/i18n/context` 返回 `{ locale, timezone, currency, source }`

**我选 B**（信息更多，且 `source` 能让「哪一级生效」可判定 ——
光有 `locale` 值区分不出「用户偏好生效」还是「请求头生效」，如果两者恰好相同）。

**新增决策 D110**：语言协商的结果必须有一个**带 `source`** 的观测通道。

### 1.5 #6：核心裁定未被守住

**接受**。rev 1 没有一步验「**用户偏好已注册 + 请求头相反**」→ 区分不出优先级。

**裁定**：AC-S-090 加一步：
> **用户偏好 `en-US`（已注册）+ 请求头 `Accept-Language: zh-CN`**（两者相反）
> → 断言 `locale === "en-US"` 且 `source === "user"`
>
> 这一步是**优先级的唯一有效判据** —— 两者相同时无法区分谁生效。

### 1.6 #7：「回退到最后一节」是坏设计

**接受**。`error.INSUFFICIENT_PERMISSION` 的最后一段就是错误码本身；且与
「不含完整路径」的判据打架。

**裁定**：回退链改为
```
目标语言 → 基准语言 → "?" 
```
**去掉「键的最后一段」这一级**。

**理由**：英文界面里蹦出个 `PERMISSION` 或 `save` 毫无意义，
反而泄露实现细节。`?` 诚实且不泄露。

### 1.7 #8：AC-S-093 的值校验不可实现

**接受**。manifest 的 `config` 段只有 `type` / `default` / `scope` / `label`，
**没有 enum / pattern 校验器** → `timezone=Not/AZone` 会 200 落库。

**裁定**：**新增配置值的格式校验**（这是本需求要补的能力，design 必须写）。

最小实现：`ConfigSpec` 增加两个**可选**字段
```json
"locale":   { "type": "string", "scope": "tenant", "default": "zh-CN",
              "format": "locale" }
"timezone": { "type": "string", "scope": "tenant", "default": "Asia/Shanghai",
              "format": "iana-timezone" }
"currency": { "type": "string", "scope": "tenant", "default": "CNY",
              "format": "iso4217" }
```
`format` 是**固定枚举**（`locale` / `iana-timezone` / `iso4217`），校验器按它分派。

**为什么不用任意正则**：正则可写但**不保证语义正确**（`^[A-Z]{3}$` 会放行 `XXX`）。
固定枚举的语义校验是**收敛的**。

**新增决策 D111**。

### 1.8 #9：后端无 IANA 时区能力

**接受**。`time` crate 只启用 `formatting`，**无 tzdb**；加 tzdb（`time-tz` 或 `chrono-tz`）
会违反「零新依赖」。

**裁定**：**时区渲染只在前端**。

- **后端**：只做时区**值的校验与存储**（D111 的 `iana-timezone` 格式）
- **后端不渲染**：继续返回 RFC 3339 UTC（契约 C13）
- **前端**：用 `Intl.DateTimeFormat(..., { timeZone })` 渲染（浏览器自带 tzdb）

**AC-S-094 改写**：验**前端**渲染（两个租户的时区 → 同一 UTC 时刻 → 不同的本地时间），
**不验**后端渲染。

> **为什么不引 tzdb**：前端浏览器**已经有完整 tzdb**，后端再引一份是重复且要维护；
> 且「存 UTC + 展示时转」是本仓既定口径（契约 C13）。

### 1.9 #10：用户时区无后端存储

**接受**。用户表无该列，design 只存 `sessionStorage`。

**裁定**：**明确「用户偏好 = 前端存储」**，并把这个限制写进 AC。

- **用户级 locale / timezone / currency** 存前端（`sessionStorage`，与 REQ-011 的 D99 一致）
- **后端只有租户级**（config）
- **因此**：AC-S-090 / 095 的「用户 > 租户」**在前端可验，在后端不可验** —— AC 要写清楚

**新增决策 D112**：用户级 i18n 偏好**本轮只在前端**（不建用户表列）；
后端协商只看请求头 + 租户配置。若要「用户偏好跨设备」需另立需求。

### 1.10 #11：Money 溢出的错误形状未定

**接受**。裁定：
- `Money` 的算术返回 `Result<Money, MoneyError>`
- `MoneyError::Overflow { .. }`
- 映射到 HTTP 时用 `ProblemDetails` 的 **`INTERNAL_ERROR`**？（不对，那是服务端错）
- → 用 **`MONEY_OVERFLOW`**（新错误码，400），并登记到 `TITLES` 表

**AC-S-096 补**：溢出返回 `code = "MONEY_OVERFLOW"`，**不是** panic、**不是**静默回绕。

### 1.11 #12：舍入缺算法与中间量宽度

**接受**。裁定：
- **中间量用 `i128`**：`a( i64) × rate_micros(i64)` 可能到 ~8.5×10^37，
  **i64 会溢出**（上限 9.2×10^18）
- **算法**（银行家舍入，整数版）：
  ```
  numerator   = a_minor as i128 * rate_micros as i128
  quotient    = numerator / 1_000_000
  remainder   = numerator % 1_000_000
  若 remainder*2 > 1_000_000 → quotient + 1
  若 remainder*2 == 1_000_000 → 偶数化（quotient 为奇数才 +1）
  ```
- **最终结果再 `try_into::<i64>()`**，溢出则 `MONEY_OVERFLOW`

**AC-S-097 补**：`a` 取 `i64::MAX` 附近时**不 panic**，返回错误。

### 1.12 #13：AC-S-098 步骤 3 实测为假

**接受**（QA 用 Node 26 实跑 `Intl`）：

| 输入 | 我的预期 | **实际** |
|---|---|---|
| `ja-JP` + **CNY** + `123456` minor | `￥1,234` | **`元 1,234.56`** |
| `ja-JP` + **JPY** + `123456` minor | — | `￥123,456` |

**裁定**：**按实测值改写 AC**。`ja-JP` 显示 CNY 时用的是 `元` 后缀 + **两位小数**
（因为 CNY 的 exponent 是 2，与 locale 无关 —— **exponent 由货币决定，不是 locale**）。

**这修正了我的一个错误认知**：「按 locale 格式化」不等于「小数位数随 locale 变」——
**小数位数由货币的 exponent 决定**。这是 AC-S-098 的核心，我原文没写清。

### 1.13 #14：SSR 不存在

**接受**。`apps/shell` 是**纯 CSR**（`mount()` + 无 Kit + 无 ssr 配置）。

**裁定**：AC-S-099 步骤 4 的「SSR 与 CSR 逐字相同」**删掉**，改为：
> 用**同一个** `t()` 实现做两件事：① SSR 风格的纯函数调用（`renderToString` 等价物）；
> ② 在浏览器里通过组件渲染 → 断言两者**同源**（都走 `I18n.t`）。
>
> 实际上：**只要 `t()` 是纯函数、且组件不自己拼字符串**，就不存在两套。
> 验收方式改为：**`grep` 确认组件里没有硬编码的文案字面量**（全部走 `t()`）。

**这比「SSR/CSR 一致」更可判定**：因为 SSR 不存在，那条永远无法验。

### 1.14 #15：「现有 8 处断言」不存在

**接受**。既有测试只比较两个响应的 title **相等**，不断言具体中文；
唯一中文 title 是 SDK 的 fixture。

**裁定**：rev 2 删掉「现有 8 处断言」这句错的；改为**实际影响面**：
- `crates/platform-iam/tests/iam_real_db.rs:468`（比较相等，改后仍成立）
- `packages/sdk/test/*.ts`（fixture 里有中文 title，改后需更新）

---

## 2. 中 / 低 severity（27 条，全部接受）

| # | 裁定要点 |
|---|---|
| #16 | 「已注册语言」的来源**钉死**为 `packages/i18n/src/locales/*.json` 的文件名 |
| #17 | 平台默认值来源**钉死**为 manifest 的 `default`（不用代码常量） |
| #18 | AC-S-092 的「记录」方式**选定**：`onMissingKey` + `missingKeys()`（design 已定，AC 要写明） |
| #19 | AC-S-091 的 `domain` 枚举**钉死**为 `error` / `common` / `schema` / `nav` 四值 |
| #20 | AC-S-096 的 `exponent` 表**钉死**（CNY=USD=EUR=2, JPY=KRW=0, 其余默认 2） |
| #21 | AC-S-097 的「不存在的币种对」→ `FX_RATE_NOT_FOUND`（404 还是 400？**定 404**） |
| #22 | AC-S-093 的三个 config 键要**登记到 manifest**（未登记写入会被 400 拒，D53） |
| #23 | AC-S-090 的 `*` 语义**钉死**：`*` → 平台默认（不做「任意匹配」） |
| #24 | AC-S-090 的 `q=0` 语义**钉死**：`q=0` 表示**明确拒绝**该语言，跳过 |
| #25 | AC-S-094 的跨日例子**保留**（QA 确认这是区分「时区生效」的唯一方式） |
| #26 | AC-S-095 的「三个时区两两不同」**保留**（QA 确认有效） |
| #27 | AC-S-099 的「切换后组件更新」用**订阅机制**（`subscribe`）验，不是刷新页面 |
| #28~#42 | 表述精确化 + 边界补充，逐条落进 rev 2，不单列 |

---

## 3. 本轮新增决策

| # | 决策 |
|---|---|
| **D109** | **跨需求的包改动必须在 demand 的依赖段显式声明** —— 本需求要改 REQ-011 的 `@base/sdk`（加 `params`），不能偷偷改 |
| **D110** | **语言协商结果必须有带 `source` 的观测通道** —— `GET /api/i18n/context` 返回 `{locale, timezone, currency, source}`。光有 `locale` 值区分不出「哪一级生效」（两者相同时） |
| **D111** | **配置值新增 `format` 校验**（固定枚举：`locale` / `iana-timezone` / `iso4217`）—— manifest 的 config 段原本**只有 type，没有格式校验**，非法值会 200 落库 |
| **D112** | **用户级 i18n 偏好本轮只在前端**（`sessionStorage`），后端协商只看请求头 + 租户配置。要「跨设备」需另立需求 |
| **D113** | **title 收敛到集中映射表**（`TITLES`）—— 55 处分散在 5 个 `match` 里**扫不干净**；收敛成一张表才可机械验，且新增错误码只能往表里加 |
| **D114** | **时区渲染只在前端** —— 后端 `time` crate 无 tzdb，加则违反零新依赖；浏览器已有完整 tzdb |

---

## 4. 下一棒

1. BA 出 `acceptance.md` **rev 2** + `design-summary.md` 更新
   （重点：§2 的 `TITLES` 表、§3 的观测通道、§5 的 i128 舍入、§6 的回退链、§7 的 SSR 删除）
2. Dev 实现
3. QA Pre-merge
4. 修复轮 → 独立复核 → 合并

**范围提示**：本轮要改**所有模块**的错误体（70 处）+ 新建 Money/汇率 +
新建 i18n 协商与观测 + 前端 i18n。**建议拆两个并行 Dev**（后端 / 前端），
但**错误体的统一改造是全局性的，必须先做**（否则前后端并行会撞）。
