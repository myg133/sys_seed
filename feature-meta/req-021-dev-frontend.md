# REQ-021 前端 Dev 交付报告（国际化与本地化）

- **日期**: 2026-11-19
- **基线**: `feature/REQ-021`（`develop` @ `88b7ca3`）
- **范围**: `@base/i18n` 全量实现、翻译资源、`@base/sdk` 的 `ApiError.params`、`apps/shell` 壳层
- **未提交**: 按要求**没有** `git commit / push / checkout / stash / reset / restore`，也**没有**跑任何 `cargo` 命令
- **零新增依赖**: 格式化全部用内建 `Intl`；`package.json` 未改

---

## 1. 交付摘要

### 1.1 `@base/i18n`（AC-S-091/092/098/099）

| 能力 | 文件 | 说明 |
|---|---|---|
| `createI18n` / `t` / `setLocale` / `subscribe` / `formatDate` / `formatMoney` / `missingKeys` / `onMissingKey` | `packages/i18n/src/index.ts` | 全量实现；保留 REQ-011 的 `translate` / `hasMessage` 兼容层 |
| 翻译资源 | `packages/i18n/src/locales/zh-CN.json`（基准）、`en-US.json` | 目录里**只有**这两个语言文件；文件名 = 已注册语言清单（裁定 #16） |
| 单测 | `packages/i18n/test/i18n.test.ts` | **43 passed** |

关键实现点：

- **回退链**：目标语言 → `zh-CN` → `"?"`；缺目标语言时**记录缺失**再回退基准语言；两边都缺返回 `"?"`，不回落到 key、也不回落到「键的最后一段」。
- **缺失键两条记录（裁定 #18）**：`onMissingKey(key, locale)`（默认 `console.warn`，每次缺失都调）+ 模块级 `missingKeys()`（去重、返回副本）。另有 `resetMissingKeys()` 仅用于测试/运维的干净起点。
- **`t()` 纯函数**：同输入同输出，不读 DOM / `navigator`；唯一副作用是缺失键记录。
- **占位符**：`{name}` 用 `params[name]` 替换；参数缺失替换为 `"?"`（不把 `{required}` 泄露给用户）。
- **`formatDate`**：`Intl.DateTimeFormat(..., { timeZone })` + `formatToParts` 拼成固定 `yyyy-MM-dd HH:mm:ss`（24h，`hourCycle:'h23'`）。**没有**任何手写偏移（源码无 `getTimezoneOffset` / `3600000` / `+8*60`）。
- **`formatMoney`**：小数位由**货币 exponent** 决定（CNY/USD/EUR=2，JPY/KRW=0，其余 2）；金额缩放用**十进制字符串拆整数位/小数位**，不做 `toFixed`、不乘 `0.01`、不 `/100`；千分位与符号交给 `Intl.NumberFormat`。
- **构造 vs 切换**：`setLocale` 只接受**已注册语言**（否则 `RangeError`）；构造函数允许合法 BCP-47 标签（如 `ja-JP`）用于纯格式化 —— AC-S-098 需要 `ja-JP` 格式化 CNY/JPY。

### 1.2 `@base/sdk` 的 `ApiError.params`（AC-S-089b / **D109 跨需求改动**）

> ⚠️ **这是修改 REQ-011 已合并的交付物**（`packages/sdk/src/index.ts`）。BA 提交时必须写进 commit message。

- `ApiErrorParams` / `ApiError` 新增 `readonly params?: Record<string, unknown>`。
- 从响应体 `params` 透传；**只接受扁平对象**（数组 / `null` / 标量视为没参数）。
- 没参数时是 `undefined`，不是 `{}`。
- `message`：由 `API 请求失败（code=…）` 改为 **ASCII** `API request failed (code=…)`，并加注释「dev-facing，**不得**用于用户可见文案」。
  **选择「改」而不是只加注释**的理由：它在任何错误路径上都会被 `console.warn` / 日志捕获，含中文容易被误当成 UI 文案；ASCII 化零成本，且仓库内没有任何断言依赖旧文案（已验证）。
- SDK fixture 的中文 `title` 改为英文 `Insufficient Permission`（design §2.5）。

### 1.3 壳层（AC-S-094/095/099）

| 文件 | 改动 |
|---|---|
| `apps/shell/src/lib/i18n.ts`（新） | 运行时：偏好读写（`sessionStorage`）、四级取值、`refreshContext()`（`GET /api/i18n/context`，带 `?locale=` / `?timezone=`）、Svelte store 契约、`errorMessageKey` / `toMessageParams` |
| `apps/shell/src/components/LanguageSwitcher.svelte`（新） | 语言切换器（`data-testid="language-switcher"`），切换走 `subscribe`，不刷新页面 |
| `apps/shell/src/components/LocaleInfo.svelte`（新） | 时区 / 当前时间（`formatDate`）/ 金额格式（`formatMoney`）的真实使用点 |
| `apps/shell/src/App.svelte` | 顶栏 + 启动文案 + 会话恢复后/登录后各拉一次租户上下文 |
| `apps/shell/src/lib/nav.ts` | 菜单只存 `labelKey`，`localizeNav(items, t)` 渲染；`filterNav` 语义不变 |
| `apps/shell/src/lib/routes.ts` | 路由 `title` 改存**翻译键**（不是文案） |
| `apps/shell/src/pages/*.svelte` | Home / Login / Forbidden / ProtectedRoles / SchemaPage 全部文案走 `t()` |
| `apps/shell/src/components/{SchemaForm,SchemaTable,Table,Dialog}.svelte` | 同上（含 `aria-label`） |
| `apps/shell/src/lib/schema-api.ts` | `describeError(error, translate)` 按稳定错误码 + `params` 渲染；内部 `Error` 改为 ASCII（dev-facing） |
| `apps/shell/src/app.css` | 顶栏 / 语言切换器 / 本地化信息的最小样式 |

取值顺序（§7.2）：

```
locale:   sessionStorage → navigator.language → GET /api/i18n/context → zh-CN
timezone: sessionStorage → GET /api/i18n/context → Asia/Shanghai
currency: sessionStorage → GET /api/i18n/context → CNY
```

- 未注册的值一律视为「没设」（与后端协商一致）。
- **不**把浏览器时区自动写成用户偏好 —— 否则租户时区永远不生效（AC-S-095 会验不出来）。
- 上下文接口**需认证**，所以 `refreshContext()` 在**会话恢复后**与**登录成功后**各调一次（匿名时拿 401 不阻塞启动）。
- 请求会带上用户显式偏好（D116），`source === "user"` 才有机会出现。

---

## 2. `Intl` 实测表（Node v26.13.1，与 QA 同口径）

命令：

```bash
node -e '
const ex={CNY:2,USD:2,EUR:2,JPY:0,KRW:0};
for (const loc of ["zh-CN","en-US","ja-JP"]) for (const cur of ["CNY","JPY","USD"]) {
  const e=ex[cur];
  console.log(loc, cur, JSON.stringify(new Intl.NumberFormat(loc,{style:"currency",currency:cur,minimumFractionDigits:e,maximumFractionDigits:e}).format(123456/10**e)));
}'
```

| locale | 货币 | exponent | **实际输出（原文）** |
|---|---|---:|---|
| `zh-CN` | CNY | 2 | `"¥1,234.56"` → `"\u00a51,234.56"`（U+00A5） |
| `zh-CN` | JPY | 0 | `"JP¥123,456"` → `"JP\u00a5123,456"` |
| `zh-CN` | USD | 2 | `"US$1,234.56"` |
| `en-US` | CNY | 2 | `"CN¥1,234.56"` → `"CN\u00a51,234.56"` |
| `en-US` | JPY | 0 | `"¥123,456"` → `"\u00a5123,456"` |
| `en-US` | USD | 2 | `"$1,234.56"` |
| `ja-JP` | CNY | 2 | `"元 1,234.56"` → **`"\u5143\u00a01,234.56"`**（分隔符是 NBSP U+00A0） |
| `ja-JP` | JPY | 0 | `"￥123,456"` → **`"\uffe5123,456"`**（U+FFE5 全角） |
| `ja-JP` | USD | 2 | `"$1,234.56"` |

补充（TC-021-083 的 5 币种表）：

| locale | 货币 | 实际输出 |
|---|---|---|
| `en-US` | EUR | `"€1,234.56"`（`\u20ac`） |
| `en-US` | KRW | `"₩123,456"`（`\u20a9`，0 位小数） |

断言里对上述字符串使用**精确相等**（`toBe`），并显式写 `\u00a5` / `\uffe5` / `\u00a0`，避免「看起来一样却 fail」。
测试名：`zh-CN × CNY → ¥1,234.56（U+00A5，2 位小数）`、`ja-JP × CNY → 元 + NBSP + 1,234.56（2 位，exponent 跟随币种）`、`ja-JP × JPY → ￥123,456（U+FFE5 全角，0 位小数）` 等（共 11 条，见第 7 节）。

---

## 3. `formatDate` 跨日实测

同一 `2026-01-15T00:00:00Z`：

| timeZone | 实际输出（`i18n.formatDate`） |
|---|---|
| `Asia/Shanghai` | `2026-01-15 08:00:00` |
| `America/New_York` | **`2026-01-14 19:00:00`**（日期变了） |
| `Europe/London` | `2026-01-15 00:00:00`（1 月 = GMT） |
| `Europe/London` @ `2026-07-15T00:00:00Z` | `2026-07-15 01:00:00`（夏令时 +01:00） |

- 三个时区结果**两两不同**，且都不等于 UTC 原值 `2026-01-15T00:00:00Z`（AC-S-094 步骤 4）。
- 不传 `timezone` 时**钉死为运行环境系统时区**（断言 `formatDate(iso) === formatDate(iso, 系统时区)`），不静默用 UTC。
- 非法 ISO / 非法 IANA → `RangeError`，不返回空串。

> **与 BA 追加上下文的差异（如实报告）**：那份实测表写 `2026-01-15 08:00`（到分）。
> `acceptance.md` AC-S-094 与 `test-cases/timezone.json`（TC-021-049~058）**钉死到秒**
> （`2026-01-15 08:00:00`）。**按 AC / 用例实现到秒**；BA 表的差异只在于省略了秒字段。

---

## 4. 回退链与缺键记录（AC-S-092，裁定 #7 / #18）

测试（`packages/i18n/test/i18n.test.ts`）：

1. `en-US 缺 common.save → 回退 zh-CN 的「保存」，不是空串、不是 key、不是「save」`

```ts
const i18n = createI18n({ locale: 'en-US', messages: { 'en-US': { 'common.cancel': 'Cancel' } } });
const text = i18n.t('common.save');
expect(text).toBe('保存');
expect(text).not.toBe('');
expect(text).not.toBe('common.save');   // ← 防「回落到 key 本身」的假绿
expect(text).not.toBe('save');          // ← 防 rev 1 的「最后一段」回退
```

2. `两种语言都缺 → "?"（不回落到键的最后一段，不泄露 key）`

```ts
const i18n = createI18n({ locale: 'en-US', messages: { 'en-US': {}, 'zh-CN': {} } });
expect(i18n.t('common.save')).toBe('?');
expect(i18n.t('nodot')).toBe('?');
expect(i18n.t('common.save')).not.toContain('.');
```

3. `缺失被记录：missingKeys() 含完整键路径，且去重`

```ts
const keys = missingKeys();
expect(keys.has('common.save')).toBe(true);
expect(keys.has('common.delete')).toBe(true);
expect(keys.has('save')).toBe(false);   // 必须是完整路径
expect(keys.size).toBe(2);              // 两次 t('common.save') 只记一次
```

4. `onMissingKey 每次缺失都调用，参数是 (key, locale)`

```ts
expect(onMissingKey).toHaveBeenCalledTimes(1);
expect(onMissingKey).toHaveBeenNthCalledWith(1, 'common.save', 'en-US');
i18n.t('common.save');
expect(onMissingKey).toHaveBeenCalledTimes(2);   // 不去重
```

5. `默认 onMissingKey 会写 console.warn（含键与 locale）`：断言 `warn` 调用 1 次且消息含 `common.save` / `en-US`。
6. `缺失记录跨语言切换累计，不因 setLocale 清空`：`missingKeys().size === 2`。

> **用例集残留冲突（请 BA 更新）**：`test-cases/error-no-chinese.json` 的 `TC-021-035` 仍写
> 「回退到键的最后一段（`common.save` → `save`）」。该行为已被 `acceptance-rev2-ruling.md` 裁定 #7
> **推翻**，rev 2 的 `acceptance.md` / `design-summary.md` 都是 `"?"`。实现按 rev 2 做，
> 建议把 TC-021-035 的期望改成 `"?"`。

---

## 5. 键清单与错误码覆盖（AC-S-091 / 089b）

### 5.1 规模

| 语言 | 总键数 | `error.*` | `common.*` | `schema.*` | `nav.*` |
|---|---:|---:|---:|---:|---:|
| `zh-CN` | **103** | 58 | 37 | 1 | 7 |
| `en-US` | **103** | 58 | 37 | 1 | 7 |

- 两份文件的**键集合与顺序完全一致**（有测试断言 `en` 的排序后键数组 `toEqual` `zh` 的）。
- 每个语言内**没有重复值**（「同一语义只有一个键」的机械判据；有测试）。
- domain 固定四值 `error` / `common` / `schema` / `nav`；`error.*` 键名 = `^[A-Z][A-Z0-9_]*$`，其余 = camelCase（有测试）。

### 5.2 错误码对账（读 Rust 源码，只读）

读取来源：

- `crates/platform-core/src/problem.rs` 的 `CODE_*`
- `crates/platform-core/src/money.rs` 的 `CODE_MONEY_OVERFLOW` / `CODE_FX_RATE_NOT_FOUND`
- `crates/platform-core/src/error.rs`（`INVALID_CONFIG_KEY` / `INVALID_IDENTIFIER` / `IO_ERROR`）
- `crates/platform-db/src/error.rs`（`DATABASE_ERROR` / `INVALID_QUERY` / `RECORD_NOT_FOUND` / `RLS_POLICY_MISSING` / `RLS_BYPASSED_BY_ROLE`）
- `crates/platform-server/src/{health,config}.rs`（`DATABASE_UNAVAILABLE` / `MIGRATIONS_PENDING` / `MIGRATION_DIRTY` / `CONFIG_MISSING_REQUIRED` / `CONFIG_INVALID_VALUE` / `CONFIG_FORBIDDEN_COMBINATION`）
- `crates/platform-{iam,org,rbac,config}/src/error.rs`（各自的 `code` 常量模块 / `CODE_*`）

对账脚本输出（在报告生成时点）：

```
rust codes: 58 | error keys: 58
missing: []
extra: []
```

即：**后端 58 个 code ↔ 前端 58 个 `error.<CODE>` 键，双向差集都为空**（含本需求新增的
`MONEY_OVERFLOW`、`FX_RATE_NOT_FOUND`、以及后端 D111 新增的 `CONFIG_VALUE_FORMAT_INVALID`）。

> **注意时序**：后端 Dev 仍在同一 worktree 里改 `crates/**`。若他之后再新增错误码，
> 前端需要补键；对账脚本的输入是**当时工作区**的 Rust 源码。当前时点无缺失。

### 5.3 可能有遗漏的键（如实列出）

- 本报告生成时点，**没有**发现后端有、前端缺的 code。
- 若后端继续新增（例如 FX / Money 的其它错误分支），需要按 `error.<CODE>` 补键；
  这是机械可推的，前端不再需要人工查表（AC-S-091 的初衷）。
- SDK 的客户端合成码 `client.NETWORK_ERROR` / `client.INTERNAL_ERROR` **不是**后端 code，
  因此放在 `common.clientNetworkError` / `common.clientInternalError`，保证 `error.*` 与后端 code 集合严格一一对应。
- `packages/schema` 里的渲染器文案（`${label}不能为空`、`unknownTypeWarning` 等）**不在本 Dev 的文件边界内**，
  仍是中文（见第 9 节风险）。

---

## 6. `ApiError.params`（AC-S-089b / D109）

测试文件 `packages/sdk/test/sdk.test.ts`，新增 `describe('@base/sdk 的错误参数（AC-S-089b / D109）')`：

```ts
// fixture 的响应体：params: { required: 'rbac:role:write' }
const error = (await client.request({ method: 'GET', path: '/api/rbac/roles' }) ...) as ApiError;

expect(error.params).toEqual({ required: 'rbac:role:write' });   // ← 深度相等
```

其余断言：

- `没有 params 的错误 → params 是 undefined（不是空对象）`：`expect(error.params).toBeUndefined()`。
- `params 非扁平对象（数组 / null）→ 视为没有参数，不把任意响应体透给 UI`：对 `[['a','b'], null, 'text', 42]` 逐个断言 `toBeUndefined()`。
- `params 不进 message`：`expect(error.message).toBe('API request failed (code=INSUFFICIENT_PERMISSION)')` 且不含 `rbac:role:write`。

渲染侧（`apps/shell/test/i18n.test.ts`）：

```ts
const error = new ApiError({
  code: 'INSUFFICIENT_PERMISSION',
  status: 403,
  params: { required: 'rbac:role:write' },
});
const text = createI18n({ locale: 'zh-CN' }).t(errorMessageKey(error.code), toMessageParams(error.params));

expect(text).toContain('rbac:role:write');
expect(text).not.toContain('{');
expect(text).not.toContain('}');
expect(text).not.toBe('error.INSUFFICIENT_PERMISSION');
expect(english).not.toBe(text);                  // 中英两套文案确实不同
```

---

## 7. 测试命令与真实数字（原文）

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-021

pnpm --filter @base/i18n test
#  Test Files  1 passed (1)
#       Tests  43 passed (43)

pnpm --filter @base/sdk test
#  Test Files  2 passed (2)
#       Tests  36 passed | 2 skipped (38)
# 说明：2 条 skip 是 packaging.test.ts 的既有行为（dist 未构建时跳过产物检查），
# 与本次改动无关；不是本次引入的 skip。

pnpm --filter @base/shell test
#  Test Files  14 passed (14)
#       Tests  127 passed (127)

pnpm -r test
#  hooks 17 / schema 38 / sdk 36(+2 skipped) / i18n 43 / auth 26 / theme 12 / ui 14 / shell 127
#  全部 Test Files passed；无 failed

pnpm -r check
#  所有包 tsc --noEmit 通过；apps/shell svelte-check found 0 errors and 0 warnings

pnpm --filter @base/i18n build
#  tsc 成功；dist/locales/{zh-CN,en-US}.json 被产出

pnpm --filter @base/shell build
#  vite build: 145 modules transformed; built in 3.14s

pnpm exec eslint <changed paths>
#  退出码 0（无输出）

pnpm exec prettier --check <changed paths>
#  All matched files use Prettier code style!
```

新增的关键用例名（`--reporter=verbose` 原文节选）：

- `两个语言包的键集合完全一致（缺键不能靠回退掩盖）`
- `同一语言内没有「值相同但键不同」的条目（同一语义只有一个键）`
- `错误键逐字等于后端 code 常量值（抽样 + 全部 UPPER_SNAKE）`
- `切到 en-US 后同一键输出不同、正确，且英文界面无 CJK`
- `切回 zh-CN 逐字等于第一次；来回 10 次无状态残留`
- `订阅：一次 setLocale 恰好通知一次；取消订阅后不再通知；多订阅者都收到`
- `未知币种：形态合法时按代码字面渲染，形态不合法时抛错（不静默当 CNY）`
- `大额金额用整数拆分，不丢分组（minorUnits=123456789012 → 1,234,567,890.12）`
- `实现只用 Intl 取时区，没有手写偏移换算`
- `用 Intl.NumberFormat，且不做手写千分位 / toFixed 浮点缩放`
- shell：`切换语言：订阅者立即回放当前值，setLocale 每次通知恰一次`
- shell：`用户时区偏好 > 租户；请求把偏好带给后端（D116，source=user 才可能出现）`
- shell：`剥注释后，组件源码里没有任何中文文案字面量`

---

## 8. 组件硬编码文案自查（AC-S-099 步骤 4）

注释里的中文是**允许**的（design §2.6），所以判据是「**剥掉注释后**没有中文文案」。

原始 `grep`（命中数包含注释）：

```bash
$ grep -rcP '[\x{4e00}-\x{9fff}]' apps/shell/src --include='*.svelte' | sort
apps/shell/src/App.svelte:29
apps/shell/src/components/Button.svelte:3
apps/shell/src/components/Can.svelte:16
apps/shell/src/components/Card.svelte:5
apps/shell/src/components/Dialog.svelte:6
apps/shell/src/components/Input.svelte:3
apps/shell/src/components/LanguageSwitcher.svelte:3
apps/shell/src/components/LocaleInfo.svelte:5
apps/shell/src/components/SchemaForm.svelte:36
apps/shell/src/components/SchemaTable.svelte:34
apps/shell/src/components/Table.svelte:8
apps/shell/src/pages/Forbidden.svelte:4
apps/shell/src/pages/Home.svelte:8
apps/shell/src/pages/Login.svelte:25
apps/shell/src/pages/ProtectedRoles.svelte:14
apps/shell/src/pages/SchemaPage.svelte:19
```

剥注释后的同一扫描（与 `apps/shell/test/i18n-text.test.ts` 使用同一状态机）：

```bash
$ node -e '...stripComments...'
扫描 .svelte 文件数: 16
剥注释后仍含中文的组件: []
```

机械证据是测试 `剥注释后，组件源码里没有任何中文文案字面量`（0 offenders），
并带两条对照：

- 阳性对照：注入 `const label = "保存"; <p>加载中…</p>` 必须命中（证明扫描有牙齿）；
- 阴性对照：`<!-- 中文注释 -->` 必须不命中（证明剥注释有效、且扫描不是恒不命中）。

---

## 9. E2E 状态（**未运行**，如实报告）

- 新增 `e2e/i18n.spec.ts` 3 条：
  1. 切换语言后**同一节点**更新、URL 不变（用 `data-e2e-same-node` 标记证明不是重新挂载）；
  2. 语言偏好存 `sessionStorage`，同标签页刷新后仍生效（新标签页不继承）；
  3. 时区/金额展示走 `formatDate` / `formatMoney`（时间形状 `yyyy-MM-dd HH:mm:ss`、不是原始 UTC 串）。
- 修改 `e2e/shell.spec.ts` 的菜单用例：先显式切到 `zh-CN` 再做中文断言。
- **没有写任何 `test.skip`**。
- 仅做了收集验证：

```bash
$ pnpm exec playwright test --list
Total: 20 tests in 4 files      # 含新增 3 条
```

**未运行的原因**：需要 `platform-server` + 数据库（`scripts/e2e-server.sh`）。当前 worktree 里
`crates/**` / `migrations/**` 有后端 Dev 的并行未提交改动，且本会话**禁止 `cargo`**
（共用 target 目录会互相拖慢）；按任务要求**不启动服务**，报告「等待集成」。
AC-S-094 / 095 / 098 / 099 的判据已由单元测试覆盖（见第 2/3/4 节），E2E 只是应用级补充。

---

## 10. 未做 / 做不到 / 有疑虑（逐条）

| # | 项 | 状态与原因 |
|---|---|---|
| 1 | **E2E 真跑** | **未运行**（见第 9 节）。不写 `skip` 当通过。 |
| 2 | `formatDate` 输出精度 | AC / 用例要求到秒（`08:00:00`），BA 追加上下文写 `08:00`。实现按 AC 到秒；请在下一轮统一文字。 |
| 3 | `TC-021-035`（用例集） | 仍写「回退到键的最后一段」，与 rev 2 裁定 #7 冲突。实现按 rev 2 = `"?"`，**用例需更新**。 |
| 4 | `TC-021-092` 的 BA 二选一 | 选了 **① 抛错**：`setLocale('ja-JP')` → `RangeError`，`i18n.locale` 不变。理由：静默忽略会让 UI 显示的语言与实际文案不一致。 |
| 5 | `TC-021-085` 的 BA 二选一 | 选了**回退分支**：形态合法但未知的币种（`ABC`）由 `Intl` 按代码字面渲染（`ABC 1,234.56`），**不**当 CNY/USD；形态不合法（`cny` / `CN`）抛 `RangeError`。 |
| 6 | `packages/schema` 的校验文案 | `${label}不能为空`、`unknownTypeWarning` 等仍是中文；该包**不在我的文件边界**（属 REQ-012 交付物）。组件层面已零硬编码，但渲染器校验错误仍是中文 —— 需要另开改动或由 BA 明确由谁改。 |
| 7 | 金额 > `Number.MAX_SAFE_INTEGER` | 前端收到的是 JSON number，超过 2^53 时精度**在解析阶段已丢**。当前 `formatMoney` 对非安全整数抛 `RangeError`（宁可不渲染也不显示错金额）。若要支持 i64 全量，需要 API 把 minor units 传成**字符串** —— 这是契约变更，超出本 Dev 权限。 |
| 8 | 首屏「二次切换」 | 顺序按 §7.2（浏览器语言在租户之前）。浏览器 `en-US` + 租户 `zh-CN` 时，首屏英文、上下文返回后保持英文（浏览器优先）—— 按规格如此。QA 挑刺 #31 提到的后端协商覆盖首屏的情况在本实现里不会发生（前端首屏不阻塞等后端）。 |
| 9 | `ApiError.message` 中文→ASCII | 属对外可见字段的文本变化。仓库内无断言依赖旧文案；若仓外消费者按中文 message 匹配需知会。 |
| 10 | `playwright.config.ts` 改了一行 | 加 `use.locale = 'zh-CN'`，否则既有 E2E（断言中文 UI）会在浏览器 `en-US` 下**假红**。该文件不在任务列出的严格边界内，**特此报备**；这是测试环境配置，不改产品行为。 |
| 11 | `apps/shell/src/lib/routes.ts` 的 `title` | 改为翻译键（当前壳层未渲染 `route.title`，页面标题来自品牌配置）。若未来要显示，直接用 `t(route.title)`。 |
| 12 | `login` 的默认错误文案 | 未知错误码 → `common.unexpectedError`（不显示 `"?"`；原始 code 留在 console/网络面板）。`describeError` 对未知 code 则透出 `<CODE> (HTTP n)`（技术信息，不显示 `?`）。 |

---

## 11. 文件改动清单

**新增**

```
packages/i18n/src/locales/zh-CN.json
packages/i18n/src/locales/en-US.json
apps/shell/src/lib/i18n.ts
apps/shell/src/components/LanguageSwitcher.svelte
apps/shell/src/components/LocaleInfo.svelte
apps/shell/test/i18n.test.ts
apps/shell/test/i18n-text.test.ts
e2e/i18n.spec.ts
```

**修改**

```
packages/i18n/src/index.ts
packages/i18n/test/i18n.test.ts
packages/sdk/src/index.ts
packages/sdk/test/sdk.test.ts
apps/shell/src/App.svelte
apps/shell/src/app.css
apps/shell/src/lib/nav.ts
apps/shell/src/lib/routes.ts
apps/shell/src/lib/schema-api.ts
apps/shell/src/components/Dialog.svelte
apps/shell/src/components/SchemaForm.svelte
apps/shell/src/components/SchemaTable.svelte
apps/shell/src/components/Table.svelte
apps/shell/src/pages/Forbidden.svelte
apps/shell/src/pages/Home.svelte
apps/shell/src/pages/Login.svelte
apps/shell/src/pages/ProtectedRoles.svelte
apps/shell/src/pages/SchemaPage.svelte
apps/shell/test/nav.test.ts
apps/shell/test/schema-api.test.ts
apps/shell/test/zero-code.test.ts
e2e/shell.spec.ts
playwright.config.ts   # 仅 test locale（第 10 节 #10 报备）
```

**未触碰**：`crates/**`、`migrations/**`、`modules/**`、`AGENTS.md`、`capabilities/**`、`package.json`（无新依赖）。
