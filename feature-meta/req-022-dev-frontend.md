# REQ-022 出站 IdP 与单点登出（前端）—— Dev 自验报告

- **需求**: REQ-022「出站 IdP 与单点登出」（父需求 REQ-001 派生）
- **棒**: **棒 2（前端）** —— 4 个页面 + API 层 + 路由 + 登录回跳 + i18n 词库
- **分支**: `feature/REQ-022`（基线 `develop@310e4d3`）｜工作区 `feature-REQ-022/`
- **我负责的 AC**: **AC-S-137**（步骤 3）、**AC-S-142**（步骤 4）、**AC-S-146**（步骤 1 / 4）、**AC-S-147**（步骤 1 / 3）、**AC-S-148**、**AC-S-150**（步骤 2）、**AC-S-152**（步骤 3）
- **规格基线**: `BA/demands/REQ-022-outbound-idp/acceptance.md`（**rev 2 + rev 3 补记**，22 条 AC）、`decisions.md`（**rev 2 + rev 3**，D196~D258）、`design-summary.md`（**rev 3** §4 / §9）
- **契约**: `BA/contracts/api-conventions/v3.md`（**C13** camelCase、**C14** 两套错误体）
- **文件边界**: 只碰 `apps/shell/**` + `packages/i18n/**` + 本报告；**未碰** `crates/**` / `migrations/**` / `scripts/**` / `e2e/**` / `Cargo.*` / `capabilities/**` / **`apps/shell/src/lib/router.ts`**
- **未 commit**（按卡的硬规矩；由 **BA 统一提交**）

> ⚠️ **同一 worktree 的另一个棒（棒 1 / 后端）正在改 `crates/platform-oauth/`、`Cargo.toml`、`Cargo.lock`** ——
> `git status` 里那三项**全部是他的**，不是我的。我的文件集与他的**不相交**（见 §9）。

---

## 1. 交付清单（文件 → AC）

| 文件 | 性质 | 对应 AC |
|---|---|---|
| `apps/shell/src/lib/oauth-api.ts` | **新增**（970 行） | 全部：路径常量 / 9 个错误码白名单 / `return_to` 的**安全校验** / 授权与设备码的**状态机** / front-channel 的 `src` 拼接 / 响应解析 |
| `apps/shell/src/pages/OAuthAuthorize.svelte` | **新增** | **AC-S-146**（步骤 1 / 4）、**AC-S-148**、**AC-S-137**（步骤 3） |
| `apps/shell/src/pages/OAuthDevice.svelte` | **新增** | **AC-S-142**（步骤 4） |
| `apps/shell/src/pages/OAuthGrants.svelte` | **新增** | **AC-S-147**（步骤 1 / 3） |
| `apps/shell/src/pages/OAuthLogout.svelte` | **新增** | **AC-S-150**（步骤 2） |
| `apps/shell/src/lib/routes.ts` | 改（+59 行） | 4 条路由：3 条**公开** + 1 条受保护（`oauth:grant:read`） |
| `apps/shell/src/App.svelte` | 改（+58 行） | 挂 4 个页面；`handleLoggedIn()` 走 `return_to`（**AC-S-137** 步骤 3） |
| `apps/shell/src/pages/Login.svelte` | 改（+22 行） | 有 `return_to` 时渲染「登录后将返回原页面」提示 |
| `packages/i18n/src/locales/zh-CN.json` | 改（+44 键） | **9 个** `error.OAUTH_*` + **33 个** `oauth.*` |
| `packages/i18n/src/locales/en-US.json` | 改（+44 键） | 同上 |
| `packages/i18n/test/i18n.test.ts` | 改（+2 条 `it`） | **AC-S-152**（步骤 3）：9 个码逐 locale 齐全；`oauth.*` 键两 locale 一致 |
| `apps/shell/test/oauth-api.test.ts` | **新增**（30 条） | `readReturnTo` 安全校验 / 端点与路由常量 / 响应解析 / 编码解码成对 |
| `apps/shell/test/oauth-authorize.test.ts` | **新增**（13 条） | 授权页状态机（**AC-S-146 / 148 / 137**） |
| `apps/shell/test/oauth-device.test.ts` | **新增**（8 条） | 设备码页（**AC-S-142**） |
| `apps/shell/test/oauth-grants.test.ts` | **新增**（8 条） | 授权列表 + 撤销 + 守卫（**AC-S-147**） |
| `apps/shell/test/oauth-logout.test.ts` | **新增**（7 条） | front-channel iframe（**AC-S-150**） |
| `apps/shell/test/oauth-login-return.test.ts` | **新增**（7 条） | 登录回跳（**AC-S-137** 步骤 3）——见 §7 偏离 2 |
| `feature-meta/req-022-dev-frontend.md` | **新增** | 本报告 |

**规模**：17 个文件（**11 个新增** + 5 个修改 + 1 个报告），前端代码约 **3000 行**（含测试）。

---

## 2. 前端测试数字（**自己跑出来的**）

### 2.1 统计口径

- 命令：`pnpm --filter @base/shell test`（= `vitest run`）与 `pnpm --filter @base/i18n test`。
- 口径 = **vitest 的 `Tests N passed / N failed` 行**（**用例**数，不是**文件**数）；
  **skipped = 0**（本轮没有任何用例被 skip）。
- **基线**是**本次改动前**在同一 worktree 跑出来的同一命令（不是引用的历史数字）。

| 包 | 基线（改动前） | 本次改动后 | 增量 |
|---|---|---|---|
| `@base/shell` | **206 passed / 0 failed**（17 个测试文件） | **279 passed / 0 failed**（23 个测试文件） | **+73**（6 个新文件：`oauth-api` 30 + `oauth-authorize` 13 + `oauth-device` 8 + `oauth-grants` 8 + `oauth-logout` 7 + `oauth-login-return` 7） |
| `@base/i18n` | **43 passed / 0 failed**（1 个测试文件） | **45 passed / 0 failed**（1 个测试文件） | **+2**（`it` 级新增：9 个码逐 locale 齐全 / `oauth.*` 键两 locale 一致） |

**REQ-011 已交付的既有用例全部仍然通过**（design R10 的要求）：
`api`(11) / `audit-api`(38) / `audit-page`(10) / `branding`(6) / `can`(5) / `i18n`(23) / `i18n-text`(4) /
`nav`(6) / `router-params`(9) / `router`(8) / `schema-api`(16) / `schema-form-submit`(3) /
`schema-form`(10) / `schema-table`(15) / `session`(9) / `sso`(30) / `zero-code`(3) —— **共 206 条，无一改动、无一失败**。

⚠️ **本轮没跑 E2E**（卡的 §7.1：棒 1 的后端还没落地；E2E 归棒 1 写文件、BA 在终版统一跑）。

### 2.2 其它命令

| 命令 | 结果 |
|---|---|
| `pnpm --filter @base/shell check`（`svelte-check`） | **0 errors / 0 warnings** ✅ |
| `pnpm --filter @base/i18n check`（`tsc --noEmit`） | **通过**（无输出） ✅ |
| `pnpm --filter @base/shell build`（`vite build`） | **通过**（`✓ built in 1.60s`；`dist/index.html` + `dist/assets/index-*.js` 150.93 kB） ✅ |
| `npx eslint apps/shell/src apps/shell/test packages/i18n/src` | **0 问题** ✅ |
| `npx prettier --check apps/shell/src apps/shell/test packages/i18n/src` | **全部符合** ✅ |

---

## 3. 变异验证证据（**8 条**，全部「改生产那一行 → 变红 → 逐字节还原」）

> 口径（卡的 §7.3 + dev-guide §9.4）：改**生产代码**（不是测试）→ 跑**相关测试文件**确认**变红**（vitest 退出码 **1**）
> → 用**备份副本**还原（`cp`，**不用 `cp -p`**）→ **`touch`**（防陈旧产物假绿）→ **`sha256sum -c` 逐字节相同**
> → **`grep -c 'MUTATION' <file>` = 0**。
> 变异源文件：**全部在最终代码上重跑过一次**（§2 之后的最后一轮改动已包含在内）。

| # | 变异点（生产代码） | 变红的测试名 | 数字 | 还原校验 |
|---|---|---|---|---|
| 1 | `isSafeReturnTo`：去掉「`//` 开头」检查**与** URL-origin 判定（= 整条协议相对防线） | 协议相对 URL（`//evil.com`）→ 拒绝 / 反斜杠伪装（`/\evil.com`）→ 拒绝 / 控制字符伪装（`/%09/evil.com`）→ 拒绝 / `isSafeReturnTo` 与 `readReturnTo` 口径一致 | **4 failed** / 26 passed（exit 1） | `sha256sum -c` **成功**；`grep -c MUTATION` = **0** |
| 2 | `currentTarget`：只带 `pathname`（丢掉 `search`） | 登录页地址里的 `return_to` = 完整 URL（pathname + search，一个参数都不能少）/ 设备码页的 `return_to` **保留** `?user_code=` | **2 failed** / 11 passed（exit 1） | 同上 ✅ |
| 3 | `initialAuthorizeOutcome`：`prompt=none` 分支改成「也走登录并继续」 | `prompt=none` → query 出现 `error=login_required`，且**不跳登录页** | **1 failed** / 12 passed（exit 1） | 同上 ✅ |
| 4 | `OAuthLogout.svelte`：`sandbox="allow-scripts"` → `sandbox=""` | `sandbox` **含** `allow-scripts` 且**不含** `allow-same-origin`（安全要求） | **1 failed** / 6 passed（exit 1） | 同上 ✅ |
| 5 | `frontChannelSrc`：少拼 `?logout_token=` | `src` **逐字** = logoutUrl + `?logout_token=` + logoutToken | **1 failed** / 6 passed（exit 1） | 同上 ✅ |
| 6 | `approveDeviceCode`：请求体漏掉 `action` | 「允许」→ 请求体 `{ userCode, action: "approve" }` / 「拒绝」→ 请求体 `{ userCode, action: "deny" }` | **2 failed** / 6 passed（exit 1） | 同上 ✅ |
| 7 | `revokeOAuthGrant`：请求体漏掉 `clientId` | 撤销 → 请求体 `{ clientId }` | **1 failed** / 7 passed（exit 1） | 同上 ✅ |
| 8 | `resolveLoginRedirect`：忽略 `return_to`（永远走路由内跳转） | 有合格的 `return_to` → 整页 `replace`（query 一个不丢） | **1 failed** / 6 passed（exit 1） | 同上 ✅ |

**合计：8 条变异，每条都「变红 → 还原」；8/8 的 `sha256sum -c` 全过、8/8 的 `grep -c 'MUTATION'` 全 0。**

### 3.1 一次**如实记录**的阴性结果（变异 1b）

卡建议的变异点是「`readReturnTo` 的「`//` 开头」检查去掉」。我**先按字面**做了这条：

| 变异 1b | 结果 |
|---|---|
| **只**去掉 `if (value.startsWith('//')) return false;` 一行（origin 判定仍在） | **30 passed / 0 failed（exit 0）—— 仍然绿** |

**原因（诚实说明）**：我的 `isSafeReturnTo` 是**两层**防线 —— 显式的「`//` 开头」检查 **+** URL 解析器的 origin 判定。
`//evil.com` 会被 **origin 判定**挡下（origin 变成 `https://evil.com`）→ 单删显式那行**不会**让行为改变。

→ 因此**变异 1** 改的是「**整条**协议相对防线」（显式检查 + origin 判定一起去掉）→ **4 条用例变红**，
这才真正证明「开放重定向被挡住」这条断言**不是空跑**。
→ **发现**：卡 §5.2 点名的「`//` 开头」检查，在我的实现里是**纵深防御**（与 origin 判定重复）。
**它不是无害的冗余**（origin 判定对 `/\evil.com`、控制字符等更广），但**单删它确实不会让测试变红** —— 如实记录。

---

## 4. 静态扫描（**必须带阳性对照**，dev-guide §9.2）

扫描器：`/tmp/scan.py` —— 复用 `apps/shell/test/i18n-text.test.ts` 的**同款注释剥离状态机**
（否则「0 命中」会被注释里的词骗过 —— 我第一版就踩了这个：注释里的 `sessionStorage` / `verification_uri_complete` 都被命中了）。

| # | 扫描目标 | 真实代码命中 | 阳性对照 | 判定 |
|---|---|---|---|---|
| A | 前端不得出现 `client_secret` / `clientSecret` 明文（D200：secret 只显示一次、只存摘要） | **0** | 临时副本注入 `const leaked = "client_secret";` → **命中 1** | ✅ |
| B | 同意票据不得落 `localStorage` / `sessionStorage`（ticket **只在页面内存**，卡 §8 第 7 问） | **0** | 注入 `sessionStorage.setItem("ticket", ticket);` → **命中 1** | ✅ |
| C | 平台端点的**请求体**不得用协议端点的 snake_case 字段（D258：平台 camelCase / 协议 snake_case） | **0** | 注入 `const body = { "client_id": "x" };` → **命中 1** | ✅ |

**扫描范围**：`apps/shell/src/**` + `packages/i18n/src/**`（A）、`oauth-api.ts` + 4 个 OAuth 页面（B）、
`oauth-api.ts`（C）。**每条扫描都先证明扫描器会命中，再说真实代码 0 命中。**

---

## 5. 开工前必答项（卡 §8 的 10 条）逐条回答

**1. 怎么判断「已登录」？**
平台的会话在 `sessionStorage`（`src/lib/session.ts`，键 `base.shell.session.v1`）→
`src/lib/api.ts` 的 `createAppApi()` 用它装配 SDK（`getAccessToken`）+ `@base/auth` 的 `createAuthStore` →
**权威的登录态是 `auth.state`（`AuthState`）**，判据是 `AuthState.status === 'authenticated'`
（`@base/auth` 的 `isAuthenticated()`）。
**页面不自己读 `sessionStorage`**：`App.svelte` 已经有 `authState`（启动时 `restoreSession()` 恢复过）→
把它当 prop 传给页面（`authenticated={authState.authenticated}`）。
→ 「未登录」时 `App.svelte` 不会把页面藏起来（路由是**公开**的），是**页面自己**处理（见第 2 问）。

**2. `/oauth/authorize` 是公开路由 → 守卫不会把它改写到登录页 → 未登录时谁负责跳登录页？**
**页面自己**。`OAuthAuthorize.svelte` 的首屏判定（`initialAuthorizeOutcome`）：
- `prompt != 'none'` → 渲染「**登录并继续**」按钮，**不调 preview**（preview 需要 Bearer）；
  点击 → `location.assign('/login?return_to=<编码后的完整 URL>')`。
- `prompt == 'none'` → **不跳登录页**（`prompt=none` 不许交互）→ `history.replaceState` 把 URL 的 query 加上
  `error=login_required`（D233）→ 渲染「需要登录」提示。
守卫**完全不参与**（路由不声明 `permission`、不声明 `requiresAuth`）。

**3. `return_to` 的编码：用 `encodeURIComponent` 还是 `URLSearchParams`？**
**`URLSearchParams`（编码与解码成对）**：`buildLoginUrl` 用 `new URLSearchParams().set('return_to', target).toString()`
（`application/x-www-form-urlencoded` 序列化），`readReturnTo` 用 `new URLSearchParams(search).get('return_to')` 解码。
两者**成对** → `+` 与 `%2B` 不会互相串味（`+` 编码成 `%2B`，解码回 `+`；空格编码成 `+`，解码回空格）——
**有测试**（`oauth-api.test.ts`「编码 → 解码后拿到原值」「`+` 与 `%2B` 不会互相串味」）。
⚠️ 若改用 `encodeURIComponent` 编码，就**不能**用 `URLSearchParams` 解码（`+` 的含义相反）→ 必须成对。

**4. `location.replace(returnTo)` 会不会把 `return_to` 自己也带上？**
**不会**。回跳目标是**授权页 / 设备码页的原始 URL**（`pathname + search`），它**本身不含** `return_to`
（授权页从不生成 `return_to`）→ 解码后 `new URLSearchParams(target).get('return_to')` 为 `null`（**有测试**）。
登录页的地址 `/login?return_to=…` 只是**中转站**，`replace` 跳的是**目标**，不是登录页自己。
（即便有人手工构造 `return_to=/login?return_to=x`，也只是再回到登录页一次，**不会成环**。）

**5. `prompt` 缺省 vs `prompt=none` 在代码里怎么表达？其它值怎么处理？**
`isPromptNone(prompt)` = `prompt === 'none'`。
⚠️ **`prompt` 可能是 `none` / `login` / `consent` / `select_account`（RFC 6749 §4.1.1）**：
**本需求只实现 `none` 与「缺省」；其它值一律当缺省处理（= 允许交互）**。理由：
- `prompt=consent` → 允许交互 = 走同意页 → 语义**正好**对上；
- `prompt=login` → 允许交互 = 未登录时引导登录 → **不比** `none` 更宽松；
- `prompt=select_account` → 本需求**不支持**选账号 → 落到「允许交互」是**更保守**的一侧
  （**绝不会**静默签发授权码）。
「缺省 = 允许交互」是 AC-S-148 步骤 5 的硬判据（缺省时**一定**走同意页，不是静默直发）。
`prompt` 的原值会**原样透传**给 `preview`（后端自己裁决）。

**6. `history.replaceState` 加 `error=` 会不会把原有的授权参数覆盖掉？**
**不会**。用 `URLSearchParams.set('error', code)` → **只改 `error` 这一个键**，其余键原样保留。
**有测试**（断言 `client_id` / `state` / `code_challenge` / `code_challenge_method` / `redirect_uri` / `scope` 逐个仍在）。

**7. `ticket` 放在哪？**
**只在页面内存**（`$state`）—— `OAuthAuthorize.svelte` 的 `outcome.preview.ticket`，
随组件卸载而消失。**不落** `localStorage` / `sessionStorage` / URL / cookie。
**有扫描**（§4 扫描 B：真实代码 0 命中，阳性对照命中）。

**8. `/oauth/grants` 的首屏 `load`：怎么保证「守卫不放行 → 请求计数为 0」？**
照抄 `AUDIT_ROUTE_PATH` 的同一套机制：`routes.ts` 里该路由声明 `permission: 'oauth:grant:read'` +
`load: async () => { const result = await loadOAuthGrants(client); onOAuthGrants?.(result); }`。
- 守卫不放行 → 地址改写成 `/403` → **路由的 `load` 不会被调用**（`router.ts:244-250`）
  → `GET /api/oauth/grants` 的请求计数为 **0**；
- `App.svelte` 另有 `resolution.allowed` 的**第二道**保护（页面也不渲染）。
⚠️ **避开了既有短路坑**：`onAudit?.(await …)` 会在没有回调时把整个表达式**短路**掉 → 请求根本不发 →
本路由的写法是**先取数据、再回调**（`const result = await …; onOAuthGrants?.(result);`）。
**有测试**（`oauth-grants.test.ts`：无权限 → 落 `/403` 且请求计数 0；有权限 → 请求 1 次；未登录 → 落 `/login` 且计数 0）。

**9. i18n 的 `oauth.*` 键怎么组织？两个 locale 的键集合一致吗？**
**扁平**结构（顶层直接 `oauth.xxx`，**不是**嵌套对象）——与既有的 `error.*` / `common.*` / `nav.*` 同一套。
共 **33 个** `oauth.*` 键 = 4 条路由标题（`oauth.nav.authorize` / `.device` / `.grants` / `.logout`）
+ 同意页 9 + 设备码页 9 + 授权列表页 9 + 登出页 3（其中 `oauth.logoutSummary` 带 `{grants}` / `{sessions}` 插值）。
**两个 locale 的键集合完全一致**（**有测试**：`oauth.*` 键两 locale 相等，且总键数 ≥ 30）。
⚠️ 「同一语言内没有『值相同但键不同』的条目」这条既有约束**卡了我一次**：
`oauth.signInRequiredHint` 与 `oauth.loginRequiredNotice` 的英文初稿都是
`"Sign in to continue this authorization"` → 已改成两句不同的话（中文侧本来就不同）。

**10. `OAuthLogout.svelte` 的 iframe：`sandbox` 是安全要求，确认实现了？**
**实现了**：`sandbox="allow-scripts"`（**含** `allow-scripts`、**不含** `allow-same-origin`）——
`src` 是**第三方域**（`logoutUrl`），`allow-same-origin` 会让被沙箱的文档拿到自己的 origin。
**有测试**（断言 `sandbox` 含 `allow-scripts` 且**不含** `allow-same-origin`）**+ 有变异验证**（变异 4：去掉 `allow-scripts` → 变红）。

---

## 6. AC 覆盖（我负责的那几条）

| AC | 卡 §2 指定的落点 | 实现位置 | 判据（自测 / 单测） |
|---|---|---|---|
| **AC-S-137** 步骤 3（未登录 → 登录 → **回到原 URL 且 query 还在**） | `OAuthAuthorize.svelte` + `Login.svelte` + `App.svelte` | `currentTarget()`（pathname+search）→ `buildLoginUrl()` → `readReturnTo()`（**校验**）→ `resolveLoginRedirect()` → `location.replace` | `oauth-authorize.test.ts`「登录页地址里的 `return_to` = 完整 URL」+「设备码页的 `return_to` 保留 `?user_code=`」；`oauth-login-return.test.ts`「有合格的 `return_to` → 整页 `replace`」；**变异 2 / 8** |
| **AC-S-146** 步骤 1（同意页**展示** client 的 name 与**全部**权限点码） | `OAuthAuthorize.svelte` | `preview.application.name` + `preview.scopes` **逐字**渲染 | 「渲染同意页，页面文本里**逐字出现**权限点码」（断言 `rbac:role:read` 与 `audit:events:read` **包含**在页面文本里） |
| **AC-S-146** 步骤 4（拒绝 → `redirectTo` 的 query 里 `error=access_denied` + `state`） | `OAuthAuthorize.svelte` + `oauth-api.ts` | `submitConsentDecision(client, env, ticket, 'deny')` → `POST /api/oauth/authorize/deny` → `env.assign(redirectTo)` | 「拒绝 → 请求体 `{ ticket }`，跳到返回的 `redirectTo`」 |
| **AC-S-148**（免登：无同意页直接跳；`consent_required` 透出到授权页 query） | `OAuthAuthorize.svelte` + `oauth-api.ts` | `needsConsent === false` → `env.assign(redirectTo)`；`needsConsent === true` + `prompt=none` → `replaceState(error=consent_required)` **且仍渲染同意页** | 「免登直发 → 直接跳 `redirectTo`」+「`prompt=none` + `needsConsent=true` → query 出现 `error=consent_required`，**仍然渲染同意页**」；**变异 3** |
| **AC-S-150** 步骤 2（**前端渲染**的 front-channel iframe：数量 / `src` / `sandbox`） | `OAuthLogout.svelte` | `{#each result.frontChannel as item}` → `<iframe src={frontChannelSrc(item)} sandbox="allow-scripts">` | 「iframe 数量**恰好** = `frontChannel.length`」+「`src` **逐字**」+「`sandbox` **含** `allow-scripts` **且不**含 `allow-same-origin`」+「空数组 → 一个都不渲染」；**变异 4 / 5** |
| **AC-S-142** 步骤 4（设备码页：输入 `user_code` → 点「允许」） | `OAuthDevice.svelte` | `GET /api/oauth/device/verify?userCode=…` → `POST /api/oauth/device/approve {userCode, action}` | 「query 里有 `user_code` → 自动查」+「没有 → 渲染输入框」+「『允许』→ 请求体 `{ userCode, action: 'approve' }`」；**变异 6** |
| **AC-S-147** 步骤 1 / 3（「我的授权」列表 + 撤销） | `OAuthGrants.svelte` | 路由 `load` 取首屏（守卫不放行 → 计数 0）→ 列表 → `POST /api/oauth/grants/revoke {clientId}` → 刷新 | 「渲染 client 名 / 权限点码 / 授权时间 / 状态」+「撤销 → 请求体 `{ clientId }`」+「无权限 → 落 403 且请求计数 0」；**变异 7** |
| **AC-S-152** 步骤 3（两个 locale 各自覆盖 9 个码） | `packages/i18n/**` | 两个语言包各 **9** 条 `error.OAUTH_*` | `packages/i18n/test/i18n.test.ts`「REQ-022 的 9 个 OAuth 码在**两个 locale** 里各自齐全」+ 既有「错误域条目数 == codes.length」（**84 → 93**，与 D223 的 `TITLES` 行数 93 一致） |

**跨栈断言口径（D194）**：全部写「**包含**」，不写「相等」——
例：「页面文本**包含** `rbac:role:read`」✓；「URL 的 query **包含** `error=login_required`」✓；
「请求体**等于** `{ ticket }`」是**同一个后端契约的请求体**（不是跨栈比较）→ 允许精确断言。

---

## 7. 与规格不一致的发现 / 偏离

### 偏离 1（**必须**改 `packages/i18n/test/i18n.test.ts` 的一处，不止「只加数组项」）

- 卡 §6 说「把 9 个码加进那个硬编码的 `codes` 数组（**只加数组项**）；**不要**改 `toHaveLength` 的形状」。
- **但同一个文件里还有一条既有断言会红**：
  `it('键格式 <domain>.<KEY>，domain 是四值枚举…')` 里的
  `const domains = new Set(['error', 'common', 'schema', 'nav'])` —— 我加的 `oauth.*` 键的 domain 是 `oauth`
  → **不在枚举里 → 那条测试必红**。
- → 我**加了 `'oauth'`**（并把用例标题里的「四值枚举」改成「五值枚举」）。
- **「只加数组项、不改 `toHaveLength` 的形状」这条我严格遵守**：
  `expect(Object.keys(catalog).filter(k => k.startsWith('error.'))).toHaveLength(codes.length)` **一个字没动**
  → 左值随数组自动变成 **93**（84 + 9），与 D223 的 `TITLES` 行数 93 一致。
- **不需要 BA 拍板**（该文件归棒 2，改它不与棒 1 冲突）；**但请 BA 知悉**这一处是卡的指令之外的必要改动。

### 偏离 2（**多了一个测试文件** + **两处可注入 props**）

- 卡 §7.2 列了 5 个测试文件；我**多加了 1 个** `apps/shell/test/oauth-login-return.test.ts`（匹配卡的 `oauth*.test.ts` 通配）。
- 原因：**登录回跳是 AC-S-137 步骤 3 的核心判据，却没有任何可断言的落点**——
  `App.svelte` 在模块顶层就碰 `window`（`initialResolution(window.location.pathname)`）→ **在 Node 里 import 不了**，
  `svelte/server` 的 `render` 又不能交互。
- → 我把「登录后跳哪儿」抽成**纯函数** `resolveLoginRedirect(search, intendedPath)`（在 `oauth-api.ts`），
  `App.svelte` 只做「按判定结果 `location.replace` 还是 `router.navigate`」；
  并给 `Login.svelte` 加了一个**可选** prop `search`（缺省 = 读浏览器地址栏）→ 提示那条也能断言。
- **这是本轮唯一的「自创」接口**（卡 §5.2 给的是内联写法）。取舍：不抽 → AC-S-137 的核心判据**没有单测**；
  抽 → 多一个纯函数（12 行）+ 两个可选 prop。**请 BA 复核这个取舍。**

### 发现 1（⚠️ **交互断言的覆盖缺口**，如实报告）

- `svelte/server` 的 `render` **只能渲染首帧**：不能点击、不能等 Promise。
- 仓里**没有** DOM 测试环境：`jsdom` / `happy-dom` 在 `pnpm-lock.yaml` 里**只是 vitest 的可选 peerDependencies**，
  **没有安装**；pnpm store（v11 152M / v3 985M）里**也没有**可离线安装的副本 →
  **无法**在不动根 `pnpm-lock.yaml`（不在我的文件集）的前提下加装 → **做不了「模拟点击」的组件测试**。
- → 我的取舍：**把「点了会怎样」的逻辑放进 `oauth-api.ts` 的模块级函数**，在那里精确断言：
  `submitConsentDecision()`（允许 / 拒绝 → `env.assign(redirectTo)`）、
  `approveDeviceCode()`（请求体 `{userCode, action}`）、`revokeOAuthGrant()`（请求体 `{clientId}`）。
  组件侧断言「按钮**渲染出来了**」+「数据**渲染出来了**」。
- **残留缺口（如实说）**：「按钮的 `onclick` 是否真的接到了那个函数」这一根线**没有**被端到端断言
  （只被类型检查 + 代码审读保证）。**终版 E2E（棒 1 写）应当覆盖这一根线。**

### 发现 2（`oauth.nav.*` 是三段键，与 `oauth.*` 并存）

- 卡 §3.2 逐字点名路由标题是 `oauth.nav.authorize` / `.device` / `.grants` / `.logout` → 我**照抄**。
- 但既有 i18n 测试的键格式检查只取 `key.split('.')` 的**前两段** → `oauth.nav.authorize` 被判成
  domain=`oauth`、name=`nav` → **通过**（`nav` 是合法 camelCase）。
- → 结果：`oauth.nav.*`（4 个路由标题）与 `oauth.*`（29 个界面文案）**两套前缀并存**。
  这是**照抄卡的结果**，不是我发明的；但**若 BA 想要统一的扁平前缀**，把 4 个路由标题改成
  `oauth.authorize` / `oauth.device` / `oauth.grants` / `oauth.logout` 即可（**前端一行**）。
  **需要 BA 拍板（不阻塞）**。

### 发现 3（`readReturnTo` 的「`//` 开头」检查是**纵深防御**，单删不会让测试变红）

见 §3.1 的变异 1b：**单删**那一行 → **仍然绿**（URL-origin 判定挡住了）。
→ 所以变异 1 改的是**整条**协议相对防线才变红。
**结论**：卡 §5.2 的三条规则我**全部实现**，但其中「`//` 开头」这条**与** origin 判定**功能重叠**；
不影响安全性，但**要如实知道**：证明「开放重定向被挡住」时，要动的是**整条**防线。

### 发现 4（`GET /api/oauth/device/verify` 的查询参数名 = `userCode`，而 RFC 8628 的响应里是 `user_code`）

- RFC 8628 的 `device_authorization` **响应**是 snake_case（`user_code` / `verification_uri_complete`）——
  那是**协议端点**（第三方应用调的）。
- 平台端点 `GET /api/oauth/device/verify` 是**平台自有端点**（契约 C13）→ 查询参数**逐字 `userCode`**（D258）。
- → 前端**两处都按各自的契约**处理：读浏览器 query 用 `user_code`（RFC 8628 的
  `verification_uri_complete` 拼出来的），调平台端点用 `userCode`。
- **请棒 1 确认**：`verify` 端点的查询参数**确实**叫 `userCode`（见 §10 的对齐清单）。

### 发现 5（`/oauth/logout` 是**公开路由**，但 `POST /api/oauth/logout` 需要 Bearer）

- 卡 §3.3 明确 `/oauth/logout` 是公开路由（不声明 `permission` / `requiresAuth`）。
- 但登出端点要 Bearer → **未登录**时页面会渲染 `error.UNAUTHENTICATED`（`ApiError` 的 `code` → `error.UNAUTHENTICATED`）。
- 卡 §4.4 **没有**规定未登录时的行为 → 我按「渲染错误提示」实现（不静默、不跳走）。
  **需要 BA / 棒 1 确认**：未登录访问 `/oauth/logout` 应当是「渲染错误」还是「直接落登录页」。

### 发现 6（登出页**不**清平台会话 —— 与 D229 / TD-043 一致）

- `POST /api/oauth/logout` 撤销的是 **OAuth grant** 与 **refresh token**；
  **已签发的登录 access token** 是自包含 JWT（验签不查库）→ 最长延迟一个 `iam.accessTokenTtl` 秒才失效（D229 / TD-043）。
- → 登出页**只**显示「已登出」+「已撤销 N 项授权，作废 M 个会话」，
  **不**额外调 IAM 的登出端点（卡 §4.4 只规定了 3 步；`§10 本轮不做` 也没列它）。
- **需要 BA / 棒 1 确认**：是否要前端再调一次平台会话登出（会让「已登出」名副其实）。

### 发现 7（设备码的 `user_code` 我做了**大小写归一**）

- RFC 8628 的 base20 字符集 `BCDFGHJKLMNPQRSTVWXZ` **全是大写** → 我把用户输入**去空白 + 转大写**后发出
  （`normalizeUserCode`）。**不补连字符**（库里是**无分隔**的 8 位）。
- ⚠️ 若棒 1 的后端**严格按原样**比对（小写输入会被拒），前端已转大写 → 不会踩到。
  但这是**前端多做的一步**，**请棒 1 知悉**。

### 发现 8（`prompt` 的其它取值按缺省处理 —— 已在 §5 第 5 问答清）

见 §5 第 5 问。**需要 BA 确认**（若要求 `prompt=login` 走「强制重新认证」或 `select_account` 报错，
都得**改前端**）。

### 发现 9（AC-S-152 步骤 1 / 4 是棒 1 的活，前端只负责步骤 3）

AC-S-152 步骤 1（`TITLES` 含全部 `ALL_CODES`、行数 == 93）与步骤 4（`all_codes()` 有 `platform_oauth` 那行）
落在 `crates/platform-server/tests/i18n_titles.rs` → **棒 1 的文件集**。
前端只做**步骤 3**（两个 locale 各自覆盖 9 个码）→ 已做 + 有测试。
⚠️ **数字对得上**：前端语言包的 `error.*` 条目数 = **93**（84 + 9），与 D223 的 `TITLES` 行数 93 一致。

### 发现 10（棒 1 的后端**还没到能对照**的阶段 —— 但有一个**当场可核**的点已核过）

- 截至我写这份报告，棒 1 的 `crates/platform-oauth/src/` **还没有 HTTP 层**（无 `http.rs`）→
  8 个平台端点的**线路形状**还没被代码钉死 → §10.1 那张表就是**给棒 1 照抄的**（D258 已经钉死了）。
- **当场可核且已核过**的一点：**9 个错误码逐字一致** ✅（见 §10.5）→
  前端词库的 9 条 `error.OAUTH_*` 与他的 `error.rs` 常量**完全对得上** → 跨栈断言（AC-S-152 步骤 3）会绿。
- **不需要 BA 拍板**；这是**进度事实**，记录在此以便终版对账。

---

## 8. 未完成 / 待别人做的部分

| 项 | 原因 | 归属 |
|---|---|---|
| **E2E 实跑** | 棒 1 的后端端点还没落地（E2E 文件归棒 1 写）；本轮按卡 §7.1 **不跑 E2E** | 棒 1 + BA（终版统一跑） |
| **「模拟点击」的组件测试** | 仓里没有 DOM 测试环境，`jsdom`/`happy-dom` 不在 lockfile、pnpm store 里也没有 → 离线装不了（装它要动根 `pnpm-lock.yaml`，**不在我的文件集**）→ 见 §7 发现 1 | **需要 BA 拍板**：接受「逻辑在模块级断言 + 组件断言渲染」，或授权加 DOM 依赖 |
| **`App.svelte` 的 `location.replace(returnTo)` 那一行本身** | `App.svelte` 在模块顶层碰 `window` → Node 里 import 不了 → 只能断言它调用的**纯判定** `resolveLoginRedirect` | 同上（E2E 覆盖） |
| **AC-S-152 步骤 1 / 4**（`TITLES` 行数 93 / `all_codes()` 有 `platform_oauth` 那行） | 在 `crates/platform-server/tests/i18n_titles.rs` | 棒 1 |
| **跨栈 `i18n_titles` 测试实跑** | 卡 §1.3 明令「**不要**跑 `cargo test` / `cargo fmt` / `cargo xtask`」（棒 1 的活 + 低配机 CPU 争抢）→ 我**只做了**静态分析：该测试对词库只断言「**覆盖**」与「**不空**」，**没有**「词库条目数 == 某常量」的精确计数 → 我加的 9 条 `error.OAUTH_*` **不会**让它变红（前提：棒 1 的 `ALL_CODES` 与我的 9 个码一致 —— **已核过，逐字一致** ✅） | **BA 终版跑**（`cargo test -p platform-server --test i18n_titles`） |
| **`POST /api/oauth/clients` 等 client 管理界面** | D218：归 REQ-016；卡 §10 明确不做 | REQ-016 |
| **底座业务端点接受第三方 OAuth 令牌** | D209 / TD-041：默认拒绝，前端无需配合 | 棒 1 |

---

## 9. 对棒 2 文件集之外的任何改动

**（按卡的 §9 第 8 条：应该为空 —— 实测为空）**

`git status --porcelain`（同一 worktree，棒 1 与我共用）：

```
?? apps/shell/src/lib/oauth-api.ts                ← 我的
?? apps/shell/src/pages/OAuthAuthorize.svelte     ← 我的
?? apps/shell/src/pages/OAuthDevice.svelte        ← 我的
?? apps/shell/src/pages/OAuthGrants.svelte        ← 我的
?? apps/shell/src/pages/OAuthLogout.svelte        ← 我的
?? apps/shell/test/oauth-api.test.ts              ← 我的
?? apps/shell/test/oauth-authorize.test.ts        ← 我的
?? apps/shell/test/oauth-device.test.ts           ← 我的
?? apps/shell/test/oauth-grants.test.ts           ← 我的
?? apps/shell/test/oauth-logout.test.ts           ← 我的
?? apps/shell/test/oauth-login-return.test.ts     ← 我的（新增的第 6 个测试文件，见 §7 偏离 2）
?? crates/platform-oauth/                         ← ⚠️ **棒 1 的**，不是我的
 M apps/shell/src/App.svelte                      ← 我的
 M apps/shell/src/lib/routes.ts                   ← 我的
 M apps/shell/src/pages/Login.svelte              ← 我的
 M Cargo.lock                                    ← ⚠️ **棒 1 的**，不是我的
 M Cargo.toml                                    ← ⚠️ **棒 1 的**，不是我的
 M packages/i18n/src/locales/en-US.json          ← 我的
 M packages/i18n/src/locales/zh-CN.json          ← 我的
 M packages/i18n/test/i18n.test.ts               ← 我的
```

- **我改的文件全部落在 `apps/shell/**` + `packages/i18n/**`**（卡 §1.1）+ 本报告（`feature-meta/**`，卡 §9 指定）。
- **我没有碰**：`crates/**`、`migrations/**`、`scripts/**`、`e2e/**`、`Cargo.toml`、`Cargo.lock`、`capabilities/**`、
  **以及特别禁止的 `apps/shell/src/lib/router.ts`**（D221）。
- `crates/platform-oauth/`、`Cargo.toml`、`Cargo.lock` 三项**是棒 1 的改动**，我**没动过**（也没 `git add` / `commit` / `stash` / `restore`）。
- ⚠️ `node_modules/` 与 `apps/shell/dist/` 是我跑测试 / 构建产生的（`.gitignore` 覆盖，不在 `git status` 里）。

---

## 10. 给棒 1 的接口对齐清单（**终版前请逐条对照**）

> **平台端点 = camelCase；协议端点 = RFC 的 snake_case；两套不混。**（契约 C13 / D258）
> 前端**只调平台端点**。

### 10.1 我实际调用的 8 个端点 + 实际用到的每个字段名

| # | 端点 | 请求（前端发出） | 响应（前端读） |
|---|---|---|---|
| 1 | `POST /api/oauth/authorize/preview` | `{ responseType?, clientId?, redirectUri?, scope?, state?, codeChallenge?, codeChallengeMethod?, prompt? }`<br>（字段名**逐字** camelCase；**query 里没有的字段就不出现在体里**；`scope` 是**空格分隔的字符串**） | `{ needsConsent: boolean, ticket?: string, application: { clientId: string, name: string }, scopes: string[], redirectTo?: string }`<br>（`ticket` 仅 `needsConsent === true` 时读；`redirectTo` 仅 `needsConsent === false` 时读） |
| 2 | `POST /api/oauth/authorize/confirm` | `{ ticket: string }` | `{ redirectTo: string }` |
| 3 | `POST /api/oauth/authorize/deny` | `{ ticket: string }` | `{ redirectTo: string }`（query 里有 `error=access_denied` 与 `state`） |
| 4 | `GET /api/oauth/grants` | 无请求体 | `[{ clientId: string, clientName?: string, scopes: string[], grantedAt: string, status: string, revokedAt?: string }]`<br>（`clientName` / `revokedAt` **可缺**；前端做了兜底） |
| 5 | `POST /api/oauth/grants/revoke` | `{ clientId: string }` | `{ status: 'revoked' }`（**当前前端不读这个字段**，只判 200） |
| 6 | `POST /api/oauth/logout` | 无请求体 | `{ loggedOut: { grants: number, sessions: number }, frontChannel: [{ clientId: string, logoutUrl: string, logoutToken: string }] }`<br>（`frontChannel` **可能是空数组**，不是 `null`） |
| 7 | `GET /api/oauth/device/verify` | 查询参数 **`userCode`**（camelCase！） | `{ clientId: string, clientName: string, scopes: string[] }` |
| 8 | `POST /api/oauth/device/approve` | `{ userCode: string, action: 'approve' \| 'deny' }` | `{ status: 'approved' \| 'denied' }` |

### 10.2 前端的错误处理约定

- 平台端点的错误体 = **RFC 9457 Problem Details** → 前端读 `code` 字段（**大写** `OAUTH_*`）
  → `errorMessageKey(code)` → `error.<CODE>`（`apps/shell/src/lib/i18n.ts` 既有函数）。
- 前端**登记**的 9 个码（逐字，与 `crates/platform-oauth/src/error.rs` 必须一致）：
  `OAUTH_CLIENT_NOT_FOUND` / `OAUTH_CLIENT_DISABLED` / `OAUTH_CLIENT_EXPIRED` / `OAUTH_INVALID_REQUEST` /
  `OAUTH_INVALID_REDIRECT_URI` / `OAUTH_INVALID_SCOPE` / `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` /
  `OAUTH_TICKET_INVALID` / `OAUTH_GRANT_NOT_FOUND`。
- **没有**第 10 个（设备码查不到 = `OAUTH_INVALID_REQUEST`，D257）。
- 前端**不**区分 400 / 403 / 404：只看 `code` → 文案；`code` 不认识时回退 `common.unexpectedError`
  （不把 `"?"` 显示给用户）。→ **棒 1 侧**：错误码集合之外的新码，前端会显示通用文案（不会崩）。

### 10.3 前端**不发**的请求（明确边界，避免你实现我不用 / 我用了你没有）

- **不**调 `POST /api/oauth/clients` 等 client 管理端点（D218 → REQ-016）。
- **不**调协议端点（`/token` / `/introspect` / `/revoke` / `/device_authorization`）—— 那是第三方应用调的。
- 设备码的**申请**（`device_authorization`）由第三方应用做；前端**只**做「批准」这一侧。

### 10.4 与后端联动的**路径 / 权限点**约定

| 项 | 前端用的值 | 说明 |
|---|---|---|
| 授权页路由 | `/oauth/authorize` | 前端路由（公开） |
| 设备码页路由 | `/oauth/device` | 前端路由（公开）；`verification_uri_complete` = `{授权页基址}/oauth/device?user_code=…` |
| 我的授权路由 | `/oauth/grants` | 前端路由（受保护，权限点 `oauth:grant:read`）—— **manifest 的 `menus[].route` 必须逐字等于它**（D256 ③） |
| 登出路由 | `/oauth/logout` | 前端路由（公开） |
| 权限点 | `oauth:grant:read` | 与棒 1 的 `manifest.json` / 种子迁移**逐字一致** |

### 10.5 与棒 1 的**当场可核**部分（我已经对着他的代码查过了）

| 项 | 结果 |
|---|---|
| **9 个平台错误码** | ✅ **逐字一致**。`crates/platform-oauth/src/error.rs:34-50` 的 9 个常量值 = 我词库里的 9 个 `error.OAUTH_*`（`CLIENT_NOT_FOUND` / `CLIENT_DISABLED` / `CLIENT_EXPIRED` / `INVALID_REQUEST` / `INVALID_REDIRECT_URI` / `INVALID_SCOPE` / `UNSUPPORTED_CODE_CHALLENGE_METHOD` / `TICKET_INVALID` / `GRANT_NOT_FOUND`）→ 跨栈断言「**每个 locale 包含** `error.<CODE>`」会绿 |
| **8 个平台端点的线路形状** | ⏳ **还核不了**：棒 1 的 crate 目前**没有 HTTP 层**（`crates/platform-oauth/src/` 只有 `authz / client_auth / config / crypto / error / events / keys / lib / model / pkce / protocol / ticket`，**没有 `http.rs`**；`model.rs` 里是**数据库行**结构（snake_case 的 Rust 字段，不是线路格式））→ 线路格式由**还没写**的 HTTP DTO 决定 → **§10.1 那张表就是给棒 1 照抄的** |

### 10.6 需要棒 1 明确回答的 4 个点

1. **`GET /api/oauth/device/verify` 的查询参数名确实是 `userCode`**（camelCase），不是 `user_code`？（见 §7 发现 4）
2. **`preview` 的请求体**：缺字段时前端**不发**那个键（不是发空串）—— 后端要按「缺键 = 没传」处理。
3. **`preview` 返 400 时**，错误体的 `code` 是不是 9 个 `OAUTH_*` 之一？（前端只认这 9 个 + 通用回退）
4. **`POST /api/oauth/logout` 返 200 但 `frontChannel` 为空数组**时，前端渲染「已登出」但**不渲染任何 iframe** —— 与你的实现一致吗？（D248）

---

## 11. 复现命令

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-022
export XDG_RUNTIME_DIR=/run/user/$(id -u)

pnpm install --offline --frozen-lockfile --filter "@base/shell..." --filter "@base/i18n..."

pnpm --filter @base/shell test     # → 279 passed / 0 failed（23 个文件）
pnpm --filter @base/i18n test     # → 45 passed / 0 failed（1 个文件）
pnpm --filter @base/shell check   # → svelte-check: 0 errors / 0 warnings
pnpm --filter @base/i18n check    # → tsc --noEmit：通过
pnpm --filter @base/shell build   # → vite build：通过
npx eslint apps/shell/src apps/shell/test packages/i18n/src
npx prettier --check apps/shell/src apps/shell/test packages/i18n/src
```

> ⚠️ **不要**跑 `pnpm -r test`（会带上一堆无关包，低配机慢）。
> ⚠️ 本轮**未跑 E2E**（棒 1 的后端还没落地；E2E 归棒 1 写、BA 在终版统一跑）。
> ⚠️ **未 commit**（按卡的硬规矩；由 BA 统一提交）。
