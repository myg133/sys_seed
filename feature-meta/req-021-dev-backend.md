# REQ-021 后端开发报告（Dev A）

- **需求**: REQ-021 国际化与本地化（后端全部）
- **分支 / 基线**: `feature/REQ-021` / `develop` @ `88b7ca3`
- **规格**: `acceptance.md` rev 2（AC-S-088~099）、`design-summary.md` rev 2、`acceptance-rev2-ruling.md`（42 条）、D109~D116
- **提交**: **未提交**（按规格 §11.3：两端完成由 BA 统一提交）
- **日期**: 2026-11-19

---

## 0. 结论速览

| 项 | 结果 |
|---|---|
| `TITLES` 集中表 | **53 行**，零 CJK；`title_of_code` 未登记 → ASCII 兜底 `"Error"`（不是 code、不是中文） |
| 5 个 `title` 提供函数 | **全部只剩转发**，函数体内中文字面量 **0 条**（改前 54 条） |
| `ALL_CODES` | 7 份清单共 **66 项 / 去重 53 项**；完备性 + 反向孤儿 + 「清单覆盖每个变体」三类测试均通过 |
| `Money` / FX | 整数 `i64` 最小单位、`i128` 中间量、银行家舍入（含**负数对称**）；19 条单测 |
| `format` 校验 | 三值固定枚举；`CONFIG_VALUE_FORMAT_INVALID`（400）在写入时拒绝；6 条单测 + 3 条真实库用例 |
| 语言协商 | 四级优先级 + `source` 观测；AC-S-090 八步**逐条实测**（见 §4） |
| 汇率快照 | 新迁移可逆（up→down→up 实测）；`resolve_rate` **精确匹配 asOf**，不取最新（有反例用例） |
| 后端新增第三方依赖 | **0**（`tower` / `http-body-util` 只加到 `platform-server` 的 **dev-dependencies**，且都已在锁文件里） |
| 测试 | 7 个后端 crate 全绿；另有 contracts/schema/sandbox/xtask 复跑全绿（数字见 §6） |

---

## 1. 中文 `title` 处数的**重新统计**（与 BA 的 70 处逐文件对照）

统计口径：排除 `//` 注释；① 追踪 `fn title` / `fn title_of` 函数体内的中文字符串字面量；② `ProblemDetails` 构造点上的中文 `title` 实参（`title: "…"` 与 `from_context(<CODE>, "…")`）。

### 1.1 函数产出（BA: 55，我测: 54 条中文 + notify 0 条）

| 文件 | 函数 | BA | 我测（改前） | 差异 |
|---|---|---|---|---|
| `crates/platform-config/src/error.rs` | `title()` | 12 | **12** | — |
| `crates/platform-iam/src/http.rs` | `title_of()` | 17 | **17** | — |
| `crates/platform-org/src/http.rs` | `title_of()` | 15 | **15** | — |
| `crates/platform-rbac/src/http.rs` | `title_of()` | 10 | **10** | — |
| `crates/platform-org/src/notify.rs` | `title()` | 1 | **0** | **−1**：该函数 6 个字面量**全是英文**（`organization membership added` …），本来就没有中文 |
| 小计 | | 55 | **54** | −1 |

> 需要区分两种口径：config 的 `match` 有 15 个 arm，但只有 12 个**不同**的中文字面量（3 组共用）。按「字面量」计 54；按「arm」计 57。BA 的 55 是「四个 crate 的字面量 + notify 1」，其中 notify 那一项不是中文。

### 1.2 构造点直接字面量（BA: 15，我测: **19**，差 +4）

| # | 文件:行（改前） | 字面量 | 归属 |
|---|---|---|---|
| 1 | `crates/platform-core/src/problem.rs:123` | 未认证 | 本需求文件边界内 |
| 2 | `crates/platform-core/src/problem.rs:144` | 权限不足 | 同上 |
| 3 | `crates/platform-core/src/problem.rs:165` | 资源不存在 | 同上 |
| 4 | `crates/platform-core/src/extension/permission.rs:385` | 服务内部错误 | **边界外（见 §8.1）** |
| 5 | `crates/platform-config/src/http.rs:208` | 服务内部错误 | **边界外（见 §8.1）** |
| 6 | `crates/platform-server/src/main.rs:631` | 服务内部错误 | 边界内 |
| 7 | `crates/platform-server/src/main.rs:903` | 服务未就绪 | 边界内 |
| 8 | `crates/platform-schema/src/http.rs:185` | 服务内部错误 | **不在我的边界（未改）** |
| 9 | `crates/platform-schema/src/http.rs:243` | 元数据不存在 | **不在我的边界（未改）** |
| 10 | `modules/sandbox/backend/src/demo_note_http.rs:98` | 服务内部错误 | **不在我的边界（未改）** |
| 11 | `modules/sandbox/backend/src/demo_note_http.rs:186` | 服务内部错误 | 同上 |
| 12 | `modules/sandbox/backend/src/demo_item_http.rs:128` | 服务内部错误 | 同上 |
| 13 | `modules/sandbox/backend/src/demo_item_http.rs:221` | 服务内部错误 | 同上 |
| 14 | `modules/sandbox/backend/src/http.rs:192` | 服务内部错误 | 同上 |
| 15 | `modules/sandbox/backend/src/http.rs:318` | 服务内部错误 | 同上 |
| 16 | `modules/sandbox/backend/src/http.rs:346` | 服务内部错误 | 同上 |
| 17 | `modules/sandbox/backend/src/http.rs:386` | 服务内部错误 | 同上 |
| 18 | `modules/sandbox/backend/src/http.rs:401` | 服务内部错误 | 同上 |
| 19 | `modules/sandbox/backend/src/http.rs:442` | 服务内部错误 | 同上 |
| 合计 | | | **19**（BA 记 15，差 +4） |

**改前总计：54 + 19 = 73**（BA 记 70，净差 **+3**：函数侧 −1、构造点 +4）。
我无法从 BA 的清单反推它漏了哪 4 条（规格只给了数字），所以上面**全量列出**以便对账。

### 1.3 改后（脚本复跑同一口径）

```
== title 提供函数里的中文串 ==
  合计: 0
== 构造点中文 title 字面量（生产代码）==
  crates/platform-schema/src/http.rs:185, :243            （2，边界外）
  modules/sandbox/backend/src/demo_note_http.rs:98, :186  （2，边界外）
  modules/sandbox/backend/src/demo_item_http.rs:128, :221 （2，边界外）
  modules/sandbox/backend/src/http.rs:192,318,386,401,442,346... （6，边界外）
  合计: 12
```

**函数侧已归零**；剩余 12 处全在**我的文件边界之外**（`platform-schema`、`modules/sandbox`），修法是一行（`"服务内部错误"` → `title_of_code(CODE_INTERNAL_ERROR)`，`"元数据不存在"` → `title_of_code(code)`）。**这两处不修，AC-S-088 的运行时判据在这两条路径上仍会看到中文** —— 见 §8.2 风险清单。

---

## 2. `TITLES` / `ALL_CODES` 实测

### 2.1 各 crate 的 `ALL_CODES`

| 文件 | 项数 | 说明 |
|---|---|---|
| `crates/platform-core/src/problem.rs` | **6** | `UNAUTHENTICATED` / `INSUFFICIENT_PERMISSION` / `NOT_FOUND` / `INTERNAL_ERROR` / `SCHEMA_MODULE_NOT_FOUND` / `SCHEMA_ENTITY_NOT_FOUND` |
| `crates/platform-core/src/money.rs` | **3** | `MONEY_OVERFLOW` / `FX_RATE_NOT_FOUND` / `INVALID_ARGUMENT` |
| `crates/platform-iam/src/error.rs` | **17** | 全部变体 |
| `crates/platform-org/src/error.rs` | **15** | 全部变体（事件码另计） |
| `crates/platform-rbac/src/error.rs` | **10** | 全部变体 |
| `crates/platform-config/src/error.rs` | **9** | 自有码（复用的 4 个 core 码由 core 清单认领，不重复登记） |
| `crates/platform-org/src/notify.rs`（`OrgEventKind::ALL_CODES`） | **6** | `org.member-added` 等事件码 |
| **合计 / 去重** | **66 / 53** | `TITLES` = **53 行** |

### 2.2 完备性测试的实际运行输出

```
$ cargo test -p platform-server --test i18n_titles -- --nocapture titles_cover
running 1 test
TITLES = 53 行；各 crate 的 code 合计 69 条（去重后 53 条）
test result: ok. 1 passed; 0 failed; ...
```

> 注：该行「69」是 `all_codes()` 的迭代次数，其中 `platform-server::health` 的 3 个码是**在测试里显式列出**的（`health.rs` 不在我的边界内，没有 `ALL_CODES` 常量）。§2.1 表格中的 66 是 7 份常量清单之和；69 = 66 + 3 个 health 码。

```
$ cargo test -p platform-server --test i18n_titles
running 6 tests
test codes_follow_their_namespace_convention ... ok
test every_registered_code_produces_a_cjk_free_problem_body ... ok
test no_orphan_titles ... ok
test titles_are_ascii_and_unique ... ok
test titles_cover_every_code ... ok
test unknown_codes_fall_back_to_an_ascii_placeholder ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

六个用例各自防一种假绿：

| 用例 | 防什么 |
|---|---|
| `titles_cover_every_code` | 表漏一项 → 直接点名 `source: CODE` |
| `titles_are_ascii_and_unique` | 中文回流 / 重复键 / title 等于 code |
| `no_orphan_titles` | 反向：表里加了没被任何清单认领的键（拼错也抓得到） |
| `codes_follow_their_namespace_convention` | 两套命名空间（`UPPER_SNAKE` 错误码 / `org.kebab-case` 事件码） |
| `every_registered_code_produces_a_cjk_free_problem_body` | **运行时**：每个 code 装进真实 `ProblemDetails` 序列化，`title/detail/type/instance/code` 五字段查 CJK |
| `unknown_codes_fall_back_to_an_ascii_placeholder` | 兜底不得是 code 本身 |

另：每个 crate 还有 `all_codes_lists_every_variant`（IAM/ORG/RBAC/CONFIG）与 `every_event_kind_has_a_stable_code`（org notify），把**枚举变体**逐个 `code()` 回来反查清单 —— 完备性测试只验「表 ⊇ 清单」，如果**清单自己漏了一项**它照样绿，这几条正是补那个洞。

`TITLES` 表本身（53 行）：core 6 + iam 14（17 减去 core 重叠的 3）+ org 8 + rbac 4 + config 9 + money 2 + health 3 + 事件 6 + 复用码去重 = 53。

---

## 3. `format` 校验（D111）：三个 format 的实现与**局限**

实现位置：`platform-core/src/extension/manifest.rs`（`ConfigFormat` 枚举 + `ConfigSpec::format`）+ `platform-config/src/registry.rs`（校验器）+ `platform-config/src/service.rs`（写入路径调用）。

**唯一入口** `platform_config::registry::check_format_value(format, value, registered_locales)`：写入路径与启动自检（manifest 的 `default`）调**同一个函数**（D85）。

### 3.1 `locale`

- **实现**：① 形状 `is_locale_tag`（`xx` / `xx-YY` / `xx-Hant` / `xx-Hant-TW`；首段 2~3 个 ASCII 字母，后续段 2~8 个 ASCII 字母数字）；② **必须在已注册语言集合里**。
- **已注册语言的来源**（裁定 #16）：`packages/i18n/src/locales/*.json` 的**文件名**，由 `platform_config::registry::registered_locales(repo_root)` 扫描；目录不存在或为空 → 回退 `{"zh-CN"}` + `tracing::warn!`。
- **局限**：① 与前端目录**强耦合**（如实记录，见 §8.4）；② 不校验 `zh-cn` vs `zh-CN` 的规范化（BCP 47 大小写不敏感，但我们按字面匹配 → `zh-cn` 会被判未注册）；③ 不支持 `x-` 私有子标签与 `und`（形状规则拒掉）。
- 实测：`registered: en-US, zh-CN`（运行时从仓库目录读到，说明前端产物已被正确发现）；`xx-YY` 形状合法但未注册 → `CONFIG_VALUE_FORMAT_INVALID`。

### 3.2 `iana-timezone`（**后端没有 tzdb**，D114）

- **实现**（`registry::is_iana_timezone`）：
  1. `UTC` 单独放行（`Etc/UTC` 走正常规则）；
  2. 必须含 `/`，可多段（`America/Argentina/Buenos_Aires`）；
  3. 第一段必须在**已知区域白名单**（`Africa America Antarctica Arctic Asia Atlantic Australia Brazil Canada Chile Etc Europe Indian Mexico Pacific US`）且**大小写敏感**；
  4. 每段字符集 = ASCII 字母 / 数字 / `_` / `-` / `+`（`Etc/GMT+8`），不得有空段。
- **能拒**：`Not/AZone`（区域不在白名单）、`Asia`（无 `/`）、`Asia/`、`/Shanghai`、`asia/shanghai`（大小写）、`Mars/Olympus`、`+08:00`（这是偏移量不是 IANA 名）、`Asia/Shang hai`、空串。
- **拒不了（必须知道）**：区域正确但**城市拼错**的（`Asia/Shanghia` 会被放行）；区域不在白名单的历史别名；已从当前 tzdata 移除的区名。要精确判定必须内嵌 tzdb —— 本轮**明确不做**（D114）。此局限在代码注释、单测（`assert!(is_iana_timezone("Asia/Shanghia"))` 显式断言「放行」）与本报告三处如实记录。

### 3.3 `iso4217`

- **实现**：`platform_core::money::is_iso4217` → 对 `ISO_4217_CURRENCIES`（约 150 个**现行**代码）做白名单匹配。
- **局限**：① 只收现行代码（废止码被拒）；② 不含贵金属与基金记账单位（`XAU` / `XAG` / `XDR` / `XSU` / `XUA` / `XBA`…）—— 它们不是可支付货币，放行会让 exponent 与舍入失去意义；③ 新货币需改这张表（**刻意**：错放行比错拒绝更难发现）。
- 实测：`XXXX` / `XXX` / `XTS` / `XAU` / `cny` / `CN` 全部被拒；`CNY/USD/JPY/EUR/KRW/BRL/XAF` 放行。

### 3.4 写入路径的实测（AC-S-093 步骤 3/4/5，真实库）

```
tenant-a: locale="en-US"(tenant) timezone="America/New_York"(tenant) currency="USD"(tenant)
tenant-b: locale="zh-CN"(default) timezone="Asia/Shanghai"(default) currency="CNY"(default)
rejected timezone="Not/AZone" → CONFIG_VALUE_FORMAT_INVALID (400) detail=the value `Not/AZone` of `timezone` is not a valid `iana-timezone` value: ...
rejected currency="XXXX"      → CONFIG_VALUE_FORMAT_INVALID (400) ...
rejected currency="XXX"       → CONFIG_VALUE_FORMAT_INVALID (400) ...
rejected locale="xx-YY"       → CONFIG_VALUE_FORMAT_INVALID (400) ... registered: en-US, zh-CN
```

被拒的写**不留痕**：用例在拒绝前后各读一次三层配置，`assert_eq!(before, after)`（D84 的快照判据）。

`crates/platform-config/manifest.json` 新增三个键，**都是 `scope: tenant`**，默认值来自 manifest（裁定 #17）：

```json
"locale":   { "type": "string", "default": "zh-CN",        "scope": "tenant", "format": "locale" }
"timezone": { "type": "string", "default": "Asia/Shanghai", "scope": "tenant", "format": "iana-timezone" }
"currency": { "type": "string", "default": "CNY",           "scope": "tenant", "format": "iso4217" }
```

另有一条启动自检：注册表建立时对**每个带 `format` 的键校验其 manifest `default`**，非法则 `ConfigError::Internal` → 进程起不来（默认值错了没人写过，写入路径永远拦不到）。

---

## 4. `Accept-Language` 协商：AC-S-090 八步**逐条实测**

前置（用例内的夹具）：平台默认 `zh-CN`；租户 T1 配 `en-US`；已注册语言 `{zh-CN, en-US}`；用户偏好用 `?locale=`（D116，不持久化）。

`cargo test -p platform-server --lib i18n::tests::ac090 -- --nocapture` 的原始输出：

```
步骤1: locale=zh-CN source=AcceptLanguage
步骤2: locale=zh-CN source=AcceptLanguage
步骤3: locale=zh-CN source=AcceptLanguage
步骤4: locale=en-US source=User
步骤5: locale=en-US source=Tenant
步骤6: locale=zh-CN source=AcceptLanguage
步骤7: locale=zh-CN source=Default
步骤8: locale=en-US source=User
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 50 filtered out
```

| 步骤 | 输入 | AC 期望 | 实测 `locale` | 实测 `source` | 判据 |
|---|---|---|---|---|---|
| 1 | 仅 `Accept-Language: zh-CN` | `zh-CN`（请求头） | `zh-CN` | `accept-language` | ✅ |
| 2 | 租户 `en-US` + 头 `zh-CN` | `zh-CN`（头 > 租户） | `zh-CN` | `accept-language` | ✅ 优先级顺序成立 |
| 3 | 用户 `ja-JP`（未注册）+ 头 `zh-CN` | `zh-CN`，**不是** ja-JP、**不是** en-US | `zh-CN` | `accept-language` | ✅ 未注册偏好视为没设 |
| 4 | 用户 `en-US`，**无** `Accept-Language` | `en-US` | `en-US` | `user` | ✅ |
| 5 | 头 `fr-FR`（未注册）+ 租户 `en-US` | `en-US`（租户）或 `zh-CN`（默认），**不是** fr-FR | `en-US` | `tenant` | ✅ |
| 6 | `zh-CN;q=0.9, en-US;q=0.8` | 按 q 最高 → `zh-CN` | `zh-CN` | `accept-language` | ✅ |
| 7 | `Accept-Language: *` | **平台默认**（不做任意匹配） | `zh-CN` | `default` | ✅（`*` 直接落到默认，**不**取租户的 en-US） |
| 8 ★ | 用户 `en-US`（已注册）+ 头 `zh-CN`（相反） | `locale === "en-US"` 且 `source === "user"` | `en-US` | `user` | ✅ 唯一有效的优先级判据 |

补充规则的两条实测：

- `q=0` = 明确拒绝：`zh-CN;q=0, en-US;q=0.5` → `en-US` / `accept-language`（跳过被拒语言，不把它当兜底）。
- 「结果必须是已注册语言」：5 组（含租户配了未注册值 `fr-FR` 的坏情况）全部断言 `registered.contains(locale)`，通过。

**时区**（§3.4）：`negotiate_timezone` 的输入只有「用户参数 / 租户配置」，签名里**没有** `Accept-Language`；用例对三种情形（user/tenant/default）逐一断言 `source != AcceptLanguage`。HTTP 层另有一条用例断言 `sources.timezone = "tenant"` 而 `source = "accept-language"`（语言与时区**分开观测**）。

**观测通道**（D110）实测（`crates/platform-server/tests/i18n_api.rs`，真实路由 + 内存接缝）：

```
GET /api/i18n/context?locale=en-US      Authorization: Bearer good   Accept-Language: zh-CN
→ 200 { "locale":"en-US", "source":"user", "sources":{"locale":"user", ...} }
GET /api/i18n/context  (无令牌)
→ 401 { "code":"UNAUTHENTICATED", "title":"Unauthenticated" }
GET /api/i18n/context  (令牌属于 T1，X-Tenant-Id: tenant-t2)
→ 404 { "code":"NOT_FOUND" }（契约 C3，不泄露资源存在性）
```

响应形状钉死：`{locale, timezone, currency, source, sources{locale,timezone,currency}}`。`source` = **语言的**来源（AC-S-090 的判据）；`sources.*` 让时区/币种的「哪一级生效」也机械可判（`source` 只有一个值，装不下三个字段的不同来源）。

---

## 5. `Money` / 汇率：测试名与断言

### 5.1 溢出（`MONEY_OVERFLOW` → 400，不 panic、不回绕）

| 测试 | 断言 |
|---|---|
| `addition_overflow_is_reported_not_wrapped` | `i64::MAX + 1` → `code == "MONEY_OVERFLOW"`、`status == 400`、`detail == "the amount does not fit into i64 minor units"`；`i64::MIN - 1`、`MAX × 2` 同码 |
| `from_major_reports_overflow_instead_of_wrapping` | `"99999999999999999999"` → `MONEY_OVERFLOW`；`"92233720368547758.08"`（= i64::MAX + 1 分）→ `MONEY_OVERFLOW`；**边界值** `"92233720368547758.07"` 恰好等于 `i64::MAX` |
| `the_intermediate_product_uses_i128` | `convert(i64::MAX, 1_000_000)` 精确等于 `i64::MAX`（用 `i64` 做中间量必错）；`× 2_000_000` → `MONEY_OVERFLOW`（正负两侧） |
| `non_positive_rates_are_rejected` | `rate = 0 / -1 / -1_000_000` → `INVALID_ARGUMENT` |

### 5.2 银行家舍入（`.5` 边界，`rate_micros = 500_000` 即 ×0.5）

| 测试 | 断言（整数精确相等） |
|---|---|
| `half_cases_round_to_even` | `5→2`（2.5→2）、`7→4`（3.5→4）、`1→0`（0.5→0）、`3→2`（1.5→2）、`9→4`（4.5→4）、`11→6`（5.5→6） |
| `ordinary_fractions_round_to_nearest` | `2×0.2=0.4→0`、`3×0.2=0.6→1`、1:1 恒等、`0×1.234567→0` |

### 5.3 ★ 负数舍入（规格点名的坑）

| 测试 | 断言 | 为什么这条能区分实现 |
|---|---|---|
| `negative_amounts_round_symmetrically` | `-5×0.5=-2.5→-2`；**`-7×0.5=-3.5→-4`**（向零截断会得 `-3`）；`-1×0.5=-0.5→0`；**`-3×0.5=-1.5→-2`**（截断得 `-1`）；**`-11×0.5=-5.5→-6`**（截断得 `-5`）；`-13×0.2=-2.6→-3`；`-12×0.2=-2.4→-2` | 加粗三条在「按原算法直接跑（负数余数为负）」的实现下必然失败 |
| `rounding_is_symmetric_for_a_range_of_rates` | 6 个 rate × 8 个金额，逐对断言 `convert(-a) == -convert(a)` | 一般判据，覆盖非 `.5` 的对称性 |

实现方式：先取 `numerator.unsigned_abs()` 做「半偶」判定，再按符号回填（`RNE(-x) == -RNE(x)`），代码注释写明原因。

### 5.4 其它

| 测试 | 断言 |
|---|---|
| `money_is_integer_minor_units_plus_a_currency` | 字段是 `i64` + 币种（不是 f64 / Decimal） |
| `decimal_addition_is_exact` | `0.1 + 0.2 == 0.3`（精确相等，不是 eps） |
| `large_accumulation_has_no_drift` | `9999999.99 × 1000` = `999999999000` 分，与 `"9999999990.00"` 精确相等 |
| `currency_exponents_are_pinned` | CNY/USD/EUR=2、JPY/KRW=0、其余=2 |
| `iso4217_validation_rejects_non_currencies` | 见 §3.3 |
| `different_currencies_cannot_be_combined` | CNY + USD → `INVALID_ARGUMENT`（不隐式混算） |
| `from_major_scales_by_the_currency_exponent` | CNY 两位 / JPY 零位；`"1.5"` + JPY 被拒（不静默截断） |
| `from_major_rejects_malformed_text` | `"" "-" "+" ".5" "1." "1.2.3" "1,5" " 1" "1 " "abc" "--1"` 全部 `INVALID_ARGUMENT` |
| `snapshot_timestamps_render_as_utc_rfc3339` | `as_of` → `2026-01-01T00:00:00Z`（契约 C13） |
| `fx_rate_converts_and_checks_the_source_currency` | 100.00 CNY @139500 → 1395 USD 分；源币种不符 → `INVALID_ARGUMENT` |
| `different_snapshots_produce_different_results` | 两份快照 → 1350 vs 1420 分 + `id`/`as_of` 不同 |

### 5.5 汇率快照（真实库，`platform-db/tests/fx_real_db.rs`）

```
$ cargo test -p platform-db --test fx_real_db -- --nocapture two_snapshots
2026-01-01: rateMicros=139500 minorOut=1395 rateId=37 source=manual
2026-06-01: rateMicros=142000 minorOut=1420 rateId=38 source=manual
```

| 测试 | 断言 |
|---|---|
| `two_snapshots_give_two_results_and_carry_the_snapshot_identity` | 100.00 CNY 在两个快照下 **1395 vs 1420**（不同）；结果带 `rateId` / `asOf` / `source`（可追溯，AC-S-097 步骤 4） |
| `resolving_an_unsnapshotted_instant_does_not_fall_back_to_another_row` | 2026-03-01（两份快照之间）→ `None`；2025-12-31 → `None`。**取「最近一条」的实现会在这里红**（R4） |
| `an_unknown_currency_pair_resolves_to_nothing` | 反向币种对、另一时刻 → `None`（HTTP 层 → 404 `FX_RATE_NOT_FOUND`） |
| `inserting_the_same_snapshot_twice_updates_one_row` | 同 `(from,to,as_of)` 两次写 → 同一 `id`、值被更新、行数 `count(*) = 1`（D101 幂等） |
| `non_positive_rates_are_rejected_by_the_database` | `rate_micros = 0 / -1` 被 `CHECK` 拒绝，且**不留行** |
| `the_fx_rate_migration_is_reversible` | 临时库上 `up → down → up`：表存在→消失→存在且为空；唯一约束与 `CHECK` 都生效；down 幂等；down 不触碰别人的表 |

HTTP 层（`i18n_api.rs`）：`{from,to,minor,convertedMinor,rateMicros,rateId,asOf,source}` 字段集合钉死；缺 `asOf` → 400；非法币种/`asOf` → 400 `INVALID_ARGUMENT` + `params.param`；溢出 → 400 `MONEY_OVERFLOW`；无快照 → 404 `FX_RATE_NOT_FOUND`；store 故障 → 500 `INTERNAL_ERROR`。

---

## 6. 测试执行记录（命令原文 + 数字）

环境：

```bash
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
export XDG_RUNTIME_DIR=/run/user/$(id -u)
```

### 6.1 任务书要求的命令（`--all-targets`，不带 `--include-ignored`）

| 命令 | 结果（逐二进制） |
|---|---|
| `cargo test -p platform-core --all-targets` | lib **120 passed**；bin 0；`authz_entry` **7 passed** |
| `cargo test -p platform-config --all-targets` | lib **38**；`branding_static` 4；`config_real_db` **24 passed / 1 ignored**；`contract_c5` 3；`http_authz` 10；`i18n_config_real_db` **3** |
| `cargo test -p platform-iam --all-targets` | lib **42**；`iam_real_db` **1 passed / 15 ignored** |
| `cargo test -p platform-rbac --all-targets` | lib **43**；`http_admin` 3 ignored；`me_permissions` 10 ignored；`rbac_real_db` 22 ignored |
| `cargo test -p platform-org --all-targets` | lib **69**；`org_real_db` 1 passed / **14 ignored**；`scim_outbound` 7 |
| `cargo test -p platform-db --all-targets` | lib **37**；`fx_migration_reversible` **1**；`fx_real_db` **5**；`quota_real_db` 1 passed / 1 ignored |
| `cargo test -p platform-server --all-targets` | lib **58**；bin `base` **8**；`i18n_api` **14**；`i18n_settings_real_db` **2**；`i18n_titles` **6** |

### 6.2 仓库约定的真实库跑法（`--include-ignored --test-threads=2`，`scripts/test-integration.sh` 同款）

| 命令 | 结果 |
|---|---|
| `cargo test -p platform-config --all-targets -- --include-ignored --test-threads=2` | 38 / 4 / **25** / 3 / 10 / 3 —— 全 ok，0 failed |
| `cargo test -p platform-iam --all-targets -- --include-ignored --test-threads=2` | 42 / **16** —— 全 ok，0 failed |
| `cargo test -p platform-rbac --all-targets -- --include-ignored --test-threads=2` | 43 / 3 / 10 / **22** —— 全 ok，0 failed |
| `cargo test -p platform-org --all-targets -- --include-ignored --test-threads=2` | 69 / **15** / 7 —— 全 ok，0 failed |
| `cargo test -p platform-db --all-targets -- --include-ignored --test-threads=2` | 37 / 1 / 5 / **2** —— 全 ok，0 failed |
| `cargo test -p platform-server --all-targets -- --include-ignored --test-threads=2` | 58 / 8 / 14 / 2 / 6 —— 全 ok，0 failed |

**合计：7 个后端 crate 570 条通过、0 失败。**

### 6.3 未改但复跑（确认没有连带破坏）

| 命令 | 结果 |
|---|---|
| `cargo test -p platform-contracts --all-targets -- --include-ignored --test-threads=2` | 27 + 5，0 failed |
| `cargo test -p platform-schema --all-targets -- --include-ignored --test-threads=2` | 11 + 9，0 failed |
| `cargo test -p sandbox --all-targets -- --include-ignored --test-threads=2` | 10 + 4 + 6 + 7 + 6，0 failed |
| `cargo test -p xtask --all-targets -- --include-ignored --test-threads=2` | 51，0 failed |

### 6.4 静态检查

| 命令 | 结果 |
|---|---|
| `cargo clippy -p platform-core -p platform-db -p platform-config -p platform-iam -p platform-org -p platform-rbac -p platform-server --all-targets -- -D warnings` | **无输出**（exit 0） |
| `cargo fmt --check` | 我改动的文件**全部干净**；仍有 12 个文件的 diff 全部是**基线既有**（`platform-contracts` ×3、`platform-schema` ×3、`modules/sandbox` ×3 等），我一个都没碰 |
| `cargo xtask capabilities --check` | **失败**：`E_CAPABILITIES_OUTDATED` —— `capabilities/index.json`、`capabilities/sdk/i18n.json` 与源码不一致。**见 §8.3** |

### 6.5 一次 flake（如实记录）

`platform-iam` 的 `the_three_failure_paths_have_similar_latency`（**延迟比值**断言）在第一次 `--all-targets -- --include-ignored` 全量跑时 **FAILED**（并发负载下比值超阈值），单跑与随后以 `--test-threads=2` 重跑**均通过**（16/16）。该用例与本需求改动无关（我只改了 title 文案），不是 TD-001，但同属「时间敏感断言」。**未掩盖，供 QA 判断是否登记为技术债。**

---

## 7. 新增错误码清单（全部已登记进 `TITLES`）

| 码 | HTTP | 来源 | 触发 |
|---|---|---|---|
| `MONEY_OVERFLOW` | **400** | `platform_core::money::CODE_MONEY_OVERFLOW` | 金额/换算结果超出 `i64` 最小单位（裁定 #11） |
| `FX_RATE_NOT_FOUND` | **404** | `platform_core::money::CODE_FX_RATE_NOT_FOUND` | `(from,to,asOf)` 没有快照（裁定 #21：404，不是 400） |
| `CONFIG_VALUE_FORMAT_INVALID` | **400** | `platform_config::error::CODE_VALUE_FORMAT_INVALID` | 值不满足 manifest 的 `format`（D111） |
| `INVALID_ARGUMENT` | 400 | `platform_core::money::CODE_INVALID_ARGUMENT`（core 里新定义、复用于 i18n） | `from`/`to` 非现行 ISO 4217；`asOf` 非 RFC 3339；汇率非正；币种混算 |
| `DATABASE_UNAVAILABLE` / `MIGRATIONS_PENDING` / `MIGRATION_DIRTY` | 503 | `platform_server::health`（**既有码**，新登记进 `TITLES`） | `/health/ready` 的错误体原来用中文 `"服务未就绪"` |

同时新增（不是错误码，但进了同一张表）：`org.member-added` / `org.member-removed` / `org.node-moved` / `org.node-renamed` / `org.node-deleted` / `org.user-departed` —— 组织通知的 6 个**事件码**，原来在 `notify.rs` 里有 6 条独立英文字面量，现收敛到 `TITLES`。

---

## 8. 未做 / 做不到 / 有疑虑

### 8.1 ⚠️ 文件边界外的必要改动（3 处，请 BA 追认或回退）

| 文件 | 改动 | 为什么动了边界外 |
|---|---|---|
| `crates/platform-core/src/extension/permission.rs:387` | `"服务内部错误"` → `title_of_code(CODE_INTERNAL_ERROR)` | 这是**所有** `require_permission` 路由的 500 错误体；不改则 401/403/500 三条通用路径里仍有一处中文，AC-S-088 必然失败。同 crate、一行 |
| `crates/platform-config/src/http.rs:208` | 同上 | 配置路由认证中间件的 500 错误体；不改则 `/api/config/*` 仍中文。同 crate、一行 |
| `crates/platform-server/Cargo.toml` | 加 `[dev-dependencies] tower` + `http-body-util` | 路由层行为测试必须驱动 axum 路由（`ServiceExt::oneshot`）。两者**已在工作区锁文件**、**dev-only**、不改变生产依赖树。副作用：`Cargo.lock` 多了两行（仅依赖列表，无新包） |

若 BA 要求严格回退，前两处回退后 AC-S-088 在对应路径上会重新出现中文（需在后续环节统一改）。

### 8.2 ❌ 未修的中文 title：12 处，全在边界外

`crates/platform-schema/src/http.rs:185`（服务内部错误）、`:243`（元数据不存在），以及 `modules/sandbox/backend/**` 的 10 处（都是「服务内部错误」）。

修法（每种一行）：
- `"服务内部错误"` → `platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR)`
- `"元数据不存在"` → `platform_core::titles::title_of_code(code)`（`code` 是 `SCHEMA_MODULE_NOT_FOUND` / `SCHEMA_ENTITY_NOT_FOUND`，两者都已在表里）

**影响**：AC-S-088 的运行时判据在这两个模块的错误路径上仍会看到中文。`TITLES` 表本身零 CJK、函数侧已归零，所以**静态 ①/②** 通过；**静态 ② 的全仓口径**（`fn *title*` 只剩转发）已满足（这 12 处不是函数，是构造点字面量）。

### 8.3 ❌ `cargo xtask capabilities --check` 失败（`E_CAPABILITIES_OUTDATED`）

```
✗ 内容不一致：capabilities/index.json
✗ 内容不一致：capabilities/sdk/i18n.json
```

- `capabilities/sdk/i18n.json` 来自 `packages/i18n/src/index.ts` —— **前端 Dev 的改动**；
- `capabilities/index.json` 带 `source_fingerprint`（源码指纹），任何一侧改动都会让它失配；
- 任务书明确「不要改 `capabilities/**`」，所以**我没有重新生成**。需要在两端都完成后由 BA 走单独一环执行 `cargo xtask capabilities`，否则 CI 的该步骤会红。**这不是我的代码错误，但会 red CI，必须处理。**

### 8.4 ⚠️ 已注册语言与前端目录的耦合（已知、有意）

- 后端读 `packages/i18n/src/locales/*.json` 的**文件名**（裁定 #16），目录缺失 → 回退 `{"zh-CN"}` + WARN。
- 当前工作区里前端已产出 `en-US.json` / `zh-CN.json`，所以实测 `registered: en-US, zh-CN`。
- **风险**：如果前端把文件名改成 `en_US.json` 或换目录，后端会静默退回只有 `zh-CN`（有 WARN，但没有硬失败）。前端 Dev 必须保持「文件名 = BCP 47 语言标签」。
- 我的 `i18n_config_real_db` 用例**不硬编码** `en-US`：它遍历 `registry.locales()` 断言「注册表说有的都必须可写」，另加 `xx-YY`（必然未注册）必须被拒 —— 所以前端产物变化不会让它假绿。

### 8.5 ⚠️ `?locale=` 的用户偏好不持久化（D116 的设计选择，不是遗漏）

后端每次请求都从查询参数取，不写库、不记忆。前端必须把 `sessionStorage` 的偏好**显式带上**，否则 `source` 永远是 `accept-language` / `tenant` / `default`。AC-S-090 步骤 4/8 的后端可测性依赖这一点，已用 HTTP 用例证明。

### 8.6 ⚠️ `Accept-Language` 的两个边角语义（我选的解释，需 QA 认可）

| 情形 | 我的实现 | 依据 / 风险 |
|---|---|---|
| `*;q=0.9`（通配且有正权重） | **直接落到平台默认**（`source = default`），不先看租户配置 | 裁定 #23 字面：「`*` → 平台默认（**不做**任意匹配）」。若 QA 期望「`*` 视为没设 → 落到租户」，需要改一行（这是唯一有解释空间的地方，已写死并有单测） |
| `*;q=0` | 忽略该范围，继续下一级（视为没匹配上） | `q=0` = 明确拒绝（裁定 #24）与该语言「没被提到」不同，但通配被拒绝没有可落地的语义 |

### 8.7 ⚠️ `accept-language` 的语法容错

语法坏掉的范围**整条丢弃**（`zh-CN;q=abc, en-US;q=1.5, !!, fr-FR` → 只剩 `fr-FR`）。我没有实现 `Accept-Language: zh-CN;q=0.9,en-US` 之外的全部 RFC 4647 细节（`language-range` 前缀匹配 `zh-*` 不支持 —— 按「必须是已注册语言」的字面执行）。规格没要求前缀匹配，如需请追加裁定。

### 8.8 其它如实说明

| 项 | 状态 |
|---|---|
| 汇率写入 HTTP 接口 | **按 D115 不做**；只有 store API，测试自己播种 |
| 时区渲染 | **按 D114 不在后端做**；后端只校验与存储（`timestamp` 仍是 UTC，契约 C13） |
| 用户级偏好存储 | **按 D112 不做**（不建列、不建迁移）；只有 D116 的查询参数 |
| `MANIFEST_JSON_SCHEMA` 未加 `format` | JSON Schema 只是文档产物（`xtask` 生成 `capabilities/`），运行时不消费它；加它要改 `manifest_schema.rs`（边界外）。**已记录为不一致**，建议与 §8.3 的 capabilities 重生成一起做 |
| `platform-core::money::OffsetDateTime` 重新导出 | 为了让调用方不必自己依赖 `time`（`FxRate::as_of` 是公开字段）。不是新依赖 |
| `api-conventions` 的 `params` 完整性（AC-S-089a） | i18n 的新错误码都带 `params`（`param` / `from,to,asOf` / `key,format,value,reason`）；**未做**「遍历全部 code 校验文案占位符」的通用工具（规格要求的是「文案里的 `{key}` 都能在 `params` 找到」，那需要前端翻译资源，属前端 Dev 的 089b） |

---

## 9. 文件清单（我的改动，**未提交**）

**新建**

| 文件 | 内容 |
|---|---|
| `crates/platform-core/src/titles.rs` | `TITLES`（53 行）+ `title_of_code` + `is_registered` + 零 CJK/唯一键/兜底 5 条单测 |
| `crates/platform-core/src/money.rs` | `CurrencyCode` / `Money` / `MoneyError` / `FxRate` / `convert_minor_units` / `format_rfc3339_utc` / `ISO_4217_CURRENCIES` + 19 条单测 |
| `crates/platform-db/src/fx.rs` | `FxRateStore`（`insert_rate` / `resolve_rate`）+ `NewFxRate` / `FxRateRow` |
| `crates/platform-server/src/i18n.rs` | `/api/i18n/context` + `/api/i18n/fx/convert`、认证中间件、协商纯函数、`I18nSettingsSource` / `FxRates` 接缝、20 条单测 |
| `migrations/20261119000200_i18n_fx_rate.up.sql` / `.down.sql` | `platform_fx_rate`（唯一约束 + `CHECK` + 索引 + 授权；down 可逆） |
| `crates/platform-server/tests/i18n_titles.rs` | 完备性 6 条（含反向孤儿、运行时无 CJK） |
| `crates/platform-server/tests/i18n_api.rs` | 路由层 14 条（认证 / 四源 / 404 / 400 / 溢出 / 形状钉死） |
| `crates/platform-server/tests/i18n_settings_real_db.rs` | 配置→i18n 接缝 2 条（默认来自 manifest / 租户覆盖） |
| `crates/platform-db/tests/fx_real_db.rs` | 快照存取 5 条（含「不隐式取最新」反例） |
| `crates/platform-db/tests/fx_migration_reversible.rs` | 迁移 up→down→up |
| `crates/platform-config/tests/i18n_config_real_db.rs` | 租户隔离 + 三类非法值 + 不留痕 + 注册语言全覆盖 |

**修改**

`crates/platform-core/src/{lib.rs, problem.rs, extension/mod.rs, extension/manifest.rs, extension/permission.rs}`；
`crates/platform-db/src/lib.rs`；
`crates/platform-config/src/{error.rs, registry.rs, service.rs, http.rs}` + `manifest.json`；
`crates/platform-iam/src/{error.rs, http.rs}`；
`crates/platform-org/src/{error.rs, http.rs, notify.rs}`；
`crates/platform-rbac/src/{error.rs, http.rs}`；
`crates/platform-server/src/{lib.rs, main.rs}` + `Cargo.toml`（dev-deps）；
`Cargo.lock`（两行，dev-deps 引起的依赖列表）。

**不是我改的**（同一 worktree 的其它会话）：`crates/platform-core/src/extension/schema.rs` 出现一处**纯格式化** diff（换行折行），内容零变化。我没有回退它（避免覆盖他人工作）。

---

## 10. 剩余风险（按严重度）

1. **`capabilities --check` 会红 CI**（§8.3）—— 必须由 BA 在两端完成后统一重新生成。
2. **12 处中文 `title` 在边界外未修**（§8.2）—— AC-S-088 在 `platform-schema` / `modules/sandbox` 的错误路径上仍会看到中文。
3. **3 处边界外必要改动待追认**（§8.1）。
4. **已注册语言依赖前端文件名**（§8.4）—— 有 WARN，无硬失败。
5. **`*` / `q=0` 通配语义是我按裁定字面选定的**（§8.6）—— 若 QA 有不同解释，改动量一行 + 一条单测。
6. **`iana-timezone` 无法识别拼错的城市名**（§3.2）—— D114 的已知代价，已在代码断言里显式记录为「局限」。
7. **IAM 的延迟比值用例易 flake**（§6.5）。
