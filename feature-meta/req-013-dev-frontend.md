# REQ-013 前端开发自验报告（Dev · 前端）

- **需求**: REQ-013 SSO 入站（OIDC + LDAP）与强制 SSO —— **前端部分**
- **分支 / 工作区**: `feature/REQ-013`（worktree `/home/myg133/Projects/system_seed/feature-REQ-013`）
- **规格**: `acceptance.md` **rev 3**（12 条 AC，前端涉及 AC-S-021 / AC-S-100 / AC-S-106 / AC-S-109）、
  `design-summary.md` §3 端点设计 + §8 前端落点、`demand.md` §C、`dev-guide-low-spec.md` §9
- **提交**: **未提交**（按约定由 BA 统一提交）
- **边界**: 只改 `apps/**`、`packages/**`、`e2e/**`、`playwright.config.ts`。
  **未碰** `crates/**`、`migrations/**`（后端 Dev 正在同一 worktree 里改这几个目录，
  工作期间 `git status` 里已出现 `crates/platform-iam/src/sso/**` 与两张迁移，均非本会话产物）

---

## 0. 结论速览

| 项 | 结果 |
|---|---|
| `@base/shell` 测试 | **158 passed / 0 failed / 0 skipped**（15 个文件；新增 `test/sso.test.ts` 30 条） |
| `@base/i18n` 测试 | **43 passed / 0 failed / 0 skipped** |
| 其它前端包（`pnpm -r test`） | hooks 17 · schema 38 · sdk **38**（构建后那 2 条 `skipIf(!built)` 已真跑）· theme 12 · auth 26 · ui 14 —— 全绿 |
| `pnpm -r build` | **通过**（`apps/shell/dist` 118.15 kB / gzip 40.89 kB） |
| `pnpm --filter @base/shell check`（svelte-check） | **0 errors / 0 warnings** |
| eslint / prettier（改动文件） | 干净 |
| E2E（真浏览器） | **8 / 8 桩用例通过**（Chromium + `vite preview` + `page.route`）；真后端用例 **未跑**（后端未就绪） |

---

## 1. 交付内容（逐文件）

| 文件 | 动作 | 说明 |
|---|---|---|
| `apps/shell/src/lib/sso-api.ts` | **新建** | `listProviders` / `ldapLogin` / `exchangeTicket` / `readTicket` / `authorizeNavigationUrl` / `startOidcAuthorize` + 形状归一化（`normalizeProviders` / `providersFromParams`）。全部复用 `@base/sdk` 的 `ApiClient`，**没有自建 fetch 层** |
| `apps/shell/src/lib/sso-callback.ts` | **新建** | 回调页状态机 `completeSsoCallback` + `describeSsoCallbackError`（网络错误 vs 后端错误分开） |
| `apps/shell/src/components/SsoButtons.svelte` | **新建** | 按 `type` 渲染 OIDC 按钮 / LDAP 表单；**空数组整块不渲染** |
| `apps/shell/src/pages/SsoCallback.svelte` | **新建** | `/sso/callback` 页面：处理中 / 缺 ticket / 401 / 网络 / 成功 |
| `apps/shell/src/pages/Login.svelte` | 改 | 租户输入后防抖拉 providers；强制 SSO 提示 + `params.ssoProviders` 渲染；LDAP 提交；OIDC 整页跳转 |
| `apps/shell/src/lib/api.ts` | 改 | `AppAuth` 新增 `loginWithLdap` / `loginWithTicket`；共用 `applySsoLogin`（**先 setTenant 再 setTokens**） |
| `apps/shell/src/lib/routes.ts` | 改 | 新增公开路由 `/sso/callback`（**不要求登录态**） |
| `apps/shell/src/App.svelte` | 改 | 回调页分支 + `handleSsoLoggedIn`（`replace` 跳首页，抹掉地址栏 ticket） |
| `apps/shell/src/lib/router.ts` | **改（bug 修复）** | 同路径写回地址时保留 `search` / `hash` —— 见 §3.1 |
| `apps/shell/test/sso.test.ts` | **新建** | 30 条：providers 渲染 0/1/2、请求头、强制 SSO、OIDC 导航、回调四分支、会话落库、token 不进 URL |
| `apps/shell/test/router.test.ts` | 改 | 新增 1 条回归：`/sso/callback?ticket=` 不被抹掉 |
| `e2e/sso.spec.ts` | **新建** | 9 条（8 条桩用例可在无后端时跑，1 条真后端端到端） |
| `packages/i18n/src/locales/{zh-CN,en-US}.json` | 改 | 各 +30 键（21 个 `error.SSO_*` + 9 个 `common.sso*` / `common.backToLogin`）；中英键集合严格相等（133 = 133） |
| `packages/i18n/test/i18n.test.ts` | 改 | REQ-021 的「错误键 == 后端 code 常量」清单补 21 个 SSO 码（D125：新码必须登记，否则该完备性测试反而失败） |

---

## 2. 三块功能的实现方式

### 2.1 providers 渲染（AC-S-109）

- 登录页 `Login.svelte` 在**租户输入变化**时（250ms 防抖 + 请求代号防乱序）调
  `GET /api/iam/sso/providers`，**显式带 `x-tenant-id`**（此刻没有会话，SDK 上下文里也没有租户）。
- 渲染规则：
  - `providers` 为空数组 → **不渲染区块**（`SsoButtons` 内部也有一道 `{#if}`，调用方忘了判空同样不显示）；
  - 请求失败（500 / 网络 / 形状不对）→ **静默隐藏**，本地登录完全不受影响；
  - `type: "oidc"` → 一个按钮，点击 `window.location.assign(...)`（**顶层导航**，不能走 fetch）；
  - `type: "ldap"` → 用户名 / 密码表单，提交 `POST /api/iam/sso/ldap/login`；
  - 未知 `type`（如 `saml`）→ 不渲染（不给一个点不动的入口）。
- 按钮文案按 `type` 取本语言翻译，**不用后端的 `label`**（D113：后端不出用户可见文案；
  用 `label` 会让英文界面出现后端语言）。

### 2.2 强制 SSO（AC-S-021）

- 本地登录返回 `403 SSO_REQUIRED` 时：
  - 错误文案 = `error.SSO_REQUIRED`「该组织要求使用企业账号登录」，
    **不是** `error.INVALID_CREDENTIALS`（单测与 E2E 都断言两者不同）；
  - `params.ssoProviders`（数组或 JSON 字符串都认）优先覆盖当前列表 →
    同一套 `SsoButtons` 渲染成可点入口；
  - `params` 里没有 `ssoProviders` 时保留之前拉到的列表（不会退化成「只有文案没有入口」）；
  - LDAP 登录失败同样走这条映射（例如 `SSO_LDAP_BIND_FAILED`）。

### 2.3 回调交接（AC-S-100 预期 3 / P4）

```text
IdP → GET /api/iam/sso/oidc/callback?code&state
    → 后端 302 /sso/callback?ticket=…
    → SsoCallback 页：readTicket → POST /api/iam/sso/exchange {ticket}
    → applySsoLogin（sessionStorage，D99）→ replace 跳首页
```

- 缺 ticket → 「登录链接无效或已过期」+ 回登录页按钮，**不发** exchange 请求；
- `401 SSO_TICKET_INVALID` → 「登录链接已失效，请重新登录」+ 回登录页；
- **网络错误与后端错误分开**：`client.NETWORK_ERROR` → 「网络不可用，请检查连接」；
  后端错误 → `error.<CODE>`；未知码 → 通用失败文案（不显示 `?`）；
- 处理中有明确文案「正在完成企业登录…」（不白屏），成功路径用 E2E 卡住 exchange 实测过；
- token **只进 `sessionStorage`**；成功后 `router.navigate('/', { replace: true })`，
  地址栏里的 `?ticket=` 被替换掉（会话与 URL 都用 E2E 断言过）。

---

## 3. 偏离规格 / 需要后端确认的地方

### 3.1 ⚠️ **阻塞（跨端契约）**：OIDC `authorize` 的租户传不过去

design §3 要求 `GET /api/iam/sso/oidc/authorize` 带 `x-tenant-id`，
但 `type: "oidc"` 的点击是**顶层浏览器导航**（必须如此，否则 fetch 会把 IdP 登录页当 JSON 解析），
**顶层导航无法设置请求头**。这与 P4（回调带不上租户头）是同一类缺口，只是发生在入口侧。

- 前端做法：`authorizeNavigationUrl()` 对**相对地址**（我们自己的端点）补 `?tenant=<id>`；
  绝对地址（IdP 自己的地址）原样保留。
- **需要后端**：`authorize` 在没有 `x-tenant-id` 头时接受 `?tenant=`（优先级建议：头 > 查询参数）。
  若后端只认头，OIDC 入口在真实浏览器里**物理不可用**（而 AC 用脚本客户端带头发请求，恰好覆盖不到）。
- 这是本会话唯一需要跨端补一行契约的地方，**请 BA 转给后端 Dev 确认**。

### 3.2 修复了 REQ-011 交付物里的一个真 bug：路由会吞掉查询串

`router.ts` 的 `apply()` 在 `start()` 时无条件 `history.replaceState(null, '', pathname)`
→ `/sso/callback?ticket=abc` 被改写成 `/sso/callback`，回调页永远读不到 ticket。
**这是浏览器实测（E2E）发现的**，单测只喂 `pathname` 时完全看不到。

修法：`RouterEnvironment.location` 增加可选 `search` / `hash`，
同路径写回时原样带回；不同路径（含守卫重定向）仍只写路径。
既有 7 条路由用例语义不变（写回行为保留），新增 1 条回归用例。

### 3.3 其它（不改规格，但做了取舍）

| 取舍 | 理由 |
|---|---|
| LDAP 表单不折叠，直接可见 | 强制 SSO 时用户已在找入口，多一次点击只增加失败率 |
| 目录账号标签用「目录用户名 / 目录密码」 | 登录页已有「用户名 / 密码」，同名会让可访问名称出现歧义（E2E 的 `getByLabel` 会 strict 冲突） |
| providers 在**租户输入后**才拉 | 租户由用户填（D98），空租户拉必然失败；防抖 250ms 避免逐字符请求 |
| `packages/i18n/test/i18n.test.ts` 补 21 个码 | 该测试断言 `error.*` 键数 == 后端码清单（REQ-021 的完备性检查）；新增码必须登记，否则测试反而红 |

---

## 4. AC 逐条结论（前端射程内）

| AC | 前端结论 | 证据 |
|---|---|---|
| **AC-S-021**（强制 SSO） | ✅ 前端部分完成：`403 SSO_REQUIRED` → 组织级提示 + `params.ssoProviders` 可点入口；**不**显示成「用户名或密码错误」。步骤 1~8 的**后端**行为（Argon2 次数、fail-closed）不在前端射程 | `sso.test.ts`「强制 SSO」3 条；`e2e/sso.spec.ts` 第 4 条（桩 403） |
| **AC-S-100**（OIDC 端到端） | ✅ 前端部分：预期 3（302 落点页能消费 ticket）与预期 4/8 的接口调用完成；预期 3 的「不是 200+JSON」由后端保证。⚠️ 入口的租户传递见 §3.1 | `sso-callback` 4 条 + E2E 回调 4 条（含 401、网络、成功） |
| **AC-S-106**（不泄露密钥） | ✅ 前端没有泄露面：不读配置接口、不渲染 `issuer` / `clientId` / LDAP `url`；providers 只渲染 `type` 生成的按钮；源码扫描 0 处 `localStorage`（含阳性对照） | `sso.test.ts`「token 不进 URL / localStorage」2 条；E2E 成功用例断言 localStorage 无 token |
| **AC-S-109**（providers 列表） | ✅ 步骤 1 的前端部分：0 / 1 / 2 个 provider 的渲染、空列表与请求失败都隐藏区块、带 `x-tenant-id`。步骤 8（响应不含配置值）是后端职责 | `sso.test.ts` 渲染 6 条 + 请求 4 条；E2E 前 3 条 |
| **AC-S-104**（LDAP bind） | ✅ 前端部分：表单 → `POST /api/iam/sso/ldap/login`（带租户头、`skipAuthRetry`）、空输入先拦、错误码文案齐全且 **`SSO_LDAP_BIND_FAILED` 文案不区分「用户不存在 / 密码错」** | `sso.test.ts` LDAP 2 条；文案断言见 §5 |

---

## 5. i18n 文案（21 个错误码 + 9 个 UI 键）

- 21 个 `error.SSO_*` 全部落地，中英键集合**严格相等**（各 133 键）。
- 至少这些在 UI 里**真实用到**：`SSO_REQUIRED`、`SSO_PROVIDER_NOT_CONFIGURED`、`SSO_CONFIG_INVALID`、
  `SSO_OIDC_STATE_INVALID`、`SSO_OIDC_STATE_EXPIRED`、`SSO_TICKET_INVALID`、`SSO_LDAP_BIND_FAILED`、
  `SSO_LDAP_EMPTY_PASSWORD`、`SSO_LDAP_UNREACHABLE`、`SSO_ACCOUNT_NOT_LINKED`、
  `SSO_IDENTITY_LINK_CONFLICT`、`SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE`
  （全部经 `errorMessageKey(code)` → `error.<CODE>` 由登录页 / 回调页渲染，无需逐个硬编码分支）。
- 防枚举：`SSO_LDAP_BIND_FAILED` 中英都只说「目录账号或密码不正确 / The directory account or password
  is incorrect」，不含「不存在 / not found」。
- `SSO_LDAP_EMPTY_PASSWORD` 明说「密码不能为空」。
- 值唯一性：同一语言内没有重复文案（i18n 包的既有约束），已用脚本校验。

---

## 6. 测试与验证（实际跑过的）

```bash
pnpm install --frozen-lockfile --offline     # 本 worktree 之前没有 node_modules
pnpm --filter @base/shell test               # 158 passed
pnpm --filter @base/i18n test                # 43 passed
pnpm -r test                                 # 8 个前端包全绿（含 sdk 38 passed，0 skipped）
pnpm -r build                                # 通过
pnpm --filter @base/sdk test                 # 构建后 38 passed（2 条 skipIf(!built) 已真跑）
pnpm --filter @base/shell check              # svelte-check 0 errors / 0 warnings
npx prettier --check <改动文件>               # 干净
npx eslint <改动文件>                         # 干净
```

数字明细：

| 包 | passed | failed | skipped |
|---|---|---|---|
| `@base/shell` | **157** | 0 | 0 |
| `@base/i18n` | **43** | 0 | 0 |
| `@base/sdk` | 38 | 0 | 0 |
| `@base/schema` | 38 | 0 | 0 |
| `@base/auth` | 26 | 0 | 0 |
| `@base/hooks` | 17 | 0 | 0 |
| `@base/ui` | 14 | 0 | 0 |
| `@base/theme` | 12 | 0 | 0 |

---

## 7. E2E 状态

**跑了（部分）**：8 条**前端桩用例**在**真 Chromium** 上通过。

- 做法：临时配置只起 `pnpm --filter @base/shell preview`（静态 `dist`），
  不起后端；`/api/**` 用 `page.route` 拦截（登录 / providers / exchange）。
  这样验的是前端对规格的落实，不依赖租户真的配了 IdP。
- 结果：`8 passed (5.6s)`，包括
  1. providers 非空 → OIDC 按钮 + LDAP 表单，且 providers 请求带 `x-tenant-id`；
  2. providers 空数组 → 区块不渲染，本地登录仍可用；
  3. providers 500 → 静默隐藏、无 alert；
  4. 强制 SSO 403 → 「该组织要求使用企业账号登录」+ 入口可点，且**不等于**「用户名或密码不正确」；
  5. 回调页缺 ticket → 「登录链接无效或已过期」+ 返回登录页；
  6. 回调页 401 `SSO_TICKET_INVALID` → 「登录链接已失效，请重新登录」；
  7. 回调页网络失败 → 「网络不可用，请检查连接」；
  8. 回调页成功（卡住 exchange 验 loading）→ 会话进 `sessionStorage`、URL 里没有 token / ticket、localStorage 为空。
- 临时配置已删除，未留在仓库里。

**没跑**：`npx playwright test e2e/` 全量 + `e2e/sso.spec.ts` 的**真后端**用例。

原因（诚实说明）：
- `playwright.config.ts` 的 `webServer` 会跑 `./scripts/e2e-server.sh`，它需要
  ① `cargo build`（**本机约束明确禁止**：后端 Dev 正在用 target 目录）
  ② `DATABASE_URL_TEST`（`base_test`）与迁移；
- 后端 SSO 路由此刻**还不存在**（`grep -rn "sso/providers" crates/` 为空；工作期间后端 Dev 才刚开始加
  `crates/platform-iam/src/sso/**` 与迁移）。
- 真后端用例的守卫：`/api/iam/sso/providers` 返回 **404 / 405 → `test.skip` 并写明「被测系统未就绪」**（D108）；
  返回 **200 + 空列表 → fail**（那是夹具 / 租户配置没准备好，不能跳过）；
  另外 `scripts/seed-e2e.sh` 目前**没有**写入 12 个 SSO 配置键，后端合入后需要补夹具，
  否则这条用例会按 D108 红。

---

## 8. 还没做好的地方 / 风险

1. **§3.1 的 authorize 租户传递是阻塞项**：需要后端接受 `?tenant=`（或给出别的入口方案）。
   在后端确认前，真实浏览器里的 OIDC 入口无法工作 —— 前端无法单方面解决。
2. **真实端到端未验证**：401 / 302 / IdP 交互、`LoginResponse` 的字段名（`user.tenantId` 是前端
   存会话的依据）都只有契约层证据，没有真后端返回证据。若后端 exchange 响应里没有
   `user.tenantId`，前端会按 `client.INTERNAL_ERROR` 报「响应无法解析」——**这是有意的 fail-closed**，
   但需要在联调时确认。
3. **`SSO_REQUIRED` 的 `params` 形状**假定为 `{ ssoProviders: [...] }`（数组或 JSON 字符串）。
   若后端最终用别的键名（例如 `providers`），前端会只剩「提示文案、无入口」。
   建议后端按 AC-S-021 步骤 1 的 `params.ssoProviders` 逐字实现。
4. **providers 不会在租户不变时自动刷新**：强制 SSO 后若管理员刚开通 IdP，用户需要改一次租户或刷新页面。
   本轮未加轮询（避免无谓请求）。
5. **`SSO_OIDC_*` 的细分文案已就位，但很多码在前端只能出现在「上游失败」路径**：
   登录页/回调页会把它们按码渲染出来；如果后端在回调里直接 302 到前端而**不带错误码**，
   前端只能显示通用文案（P4 只约定了成功交接）。建议后端在失败时也 302 到
   `/sso/callback?error=<CODE>`，前端可加一条分支（本轮规格未要求，未实现）。
6. **E2E 的 `title` 断言依赖浏览器 locale = zh-CN**（`playwright.config.ts` 既有约定），
   切到英文界面后这些断言需要同步改（i18n 本身有 `e2e/i18n.spec.ts` 覆盖）。
