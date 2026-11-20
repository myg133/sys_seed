# REQ-022 Dev 任务卡 —— 棒 2（前端）

> **本卡是 BA 写给 Dev 子 Agent 的施工单**。你是**棒 2**，与**棒 1（后端）并行**干活。
> 本卡**自包含**：不需要看对话历史就能开工。
>
> ⚠️ **你不需要等棒 1**。你要用的**全部**接口形状、字段名、错误码、
> 路由路径都**已经钉死**在本卡 §3 里 —— **照抄，不要猜**。

---

## 0. 抬头

| 项 | 值 |
|---|---|
| **需求** | **REQ-022 出站 IdP 与单点登出**（父需求 REQ-001） |
| **需求来源** | 父需求 REQ-001 派生，由**平台底座路线图**规划（`BA/plan/roadmap.md` Sprint 5）→ **不是**用户临时的个人需求 |
| **你的棒** | **棒 2 —— 前端**：4 个页面 + API 层 + 路由 + 登录回跳 + i18n 词库 |
| **worktree** | `/home/myg133/Projects/system_seed/feature-REQ-022`（分支 `feature/REQ-022`，基线 `develop@310e4d3`） |
| **规格版本** | `BA/demands/REQ-022-outbound-idp/acceptance.md` **rev 2 + rev 3 补记**（**22 条 AC**）<br>`decisions.md` **rev 2 + rev 3**（D196~D258）<br>`design-summary.md` **rev 3**（§9 前端交付 / §4 时序） |
| **契约** | `BA/contracts/api-conventions/v3.md`（**C13** 字段名 camelCase、**C14** 协议端点错误体） |
| **git** | **不要提交**。改动留在工作区，由 **BA 统一提交** |

### 0.1 必读（**按顺序**）

1. `BA/demands/REQ-022-outbound-idp/acceptance.md` —— 重点读 **AC-S-137 / 146 / 148 / 150** 与全局约定
2. `BA/demands/REQ-022-outbound-idp/decisions.md` —— 重点读 **D201 / D203 / D204 / D212 / D216 / D218 / D221 / D222 / D225 / D233 / D257 / D258**
3. `BA/demands/REQ-022-outbound-idp/design-summary.md` —— **§4（时序）** / **§9（前端交付）**
4. `BA/demands/REQ-022-outbound-idp/test-cases/` —— `authorization-code.md` / `consent-grant.md` / `logout.md` / `device-code.md`
5. `BA/plan/dev-guide-low-spec.md` —— **§9.1 ~ §9.14**（尤其 **§9.2** / **§9.11** / **§9.13** / **§9.14**）
6. **既有前端范式**（**照抄，别自创**）：
   - `apps/shell/src/pages/Audit.svelte`（`$props` / `$state` / `$derived` / `$effect` 的写法）
   - `apps/shell/src/lib/audit-api.ts`（API 层：纯函数 + 常量 + 白名单）
   - `apps/shell/src/lib/routes.ts`（路由表）
   - `apps/shell/src/lib/router.ts`（**只读**，你**不改**它 —— 见 §1.2）
   - `apps/shell/src/pages/Login.svelte` / `App.svelte`（你要改的两个）

---

## 1. 你的文件集（**严格** —— 与棒 1 不相交，D256）

### 1.1 你**拥有**（可以改）

```
apps/shell/src/pages/OAuthAuthorize.svelte      ← 新建：同意页 / 授权页
apps/shell/src/pages/OAuthDevice.svelte         ← 新建：设备码批准页
apps/shell/src/pages/OAuthGrants.svelte         ← 新建：「我的授权」页
apps/shell/src/pages/OAuthLogout.svelte         ← 新建：登出页（渲染 front-channel iframe）
apps/shell/src/lib/oauth-api.ts                 ← 新建：API 层（纯函数 + 常量 + 白名单）
apps/shell/src/lib/routes.ts                    ← 改：加 4 条路由
apps/shell/src/pages/Login.svelte               ← 改：支持 ?return_to=（D221）
apps/shell/src/App.svelte                       ← 改：登录后走 return_to（D221）
apps/shell/test/oauth*.test.ts                  ← 新建：前端单测
packages/i18n/src/locales/zh-CN.json            ← 改：+ error.OAUTH_*（9）+ oauth.*
packages/i18n/src/locales/en-US.json            ← 改：同上
packages/i18n/test/i18n.test.ts                 ← 改：登记 9 个码 + 计数断言
```

### 1.2 你**禁止**改（归棒 1 —— 碰了就是并行冲突）

```
crates/**            ← 全部（包括 platform-core/src/titles.rs）
migrations/**        ← 全部
scripts/**           ← 全部
e2e/**               ← 全部（e2e/oauth.spec.ts 归棒 1）
Cargo.toml / Cargo.lock
capabilities/**
apps/shell/src/lib/router.ts   ← ⚠️ **特别禁止**（见下）
```

> ⚠️ **`apps/shell/src/lib/router.ts` 不许动**（D221）：
> 改它会动 REQ-011 已交付的路由核心 —— `resolve()` 用带 query 的字符串 `match()` 会失败
> （`router.ts:204-207`）。
> **回跳的丢 query 问题用「公开路由 + `return_to`」绕开，不改路由器。**

### 1.3 同一 worktree 的**并行纪律**（两个棒共用 `feature-REQ-022`）

| 规矩 | 原因 |
|---|---|
| **禁止** `git commit` / `git checkout` / `git restore` / `git stash` / `git reset` | 会**清掉棒 1 的改动** |
| **禁止** `git add -A` / `git add .` | 会把棒 1 的文件也暂存起来 |
| 只改 §1.1 的文件 | D256 |
| **不要**跑 `cargo test` / `cargo fmt` / `cargo xtask` | 那是棒 1 的活；`cargo xtask capabilities` 会重写 `capabilities/**`（棒 1 的） |
| **不要**碰数据库 | 棒 1 独占 `scripts/seed-e2e.sh` 与 `base_test` |

---

## 2. 你负责的 AC（**你的活**）

| AC | 落在哪个文件 |
|---|---|
| **AC-S-137** 步骤 3（未登录 → 登录 → **回到原 URL 且 query 还在**） | `OAuthAuthorize.svelte` + `Login.svelte` + `App.svelte` |
| **AC-S-146** 步骤 1（同意页**展示** client 的 name 与**全部**权限点码） | `OAuthAuthorize.svelte` |
| **AC-S-146** 步骤 4（拒绝 → `redirectTo` 的 query 里 `error=access_denied` + `state`） | `OAuthAuthorize.svelte` |
| **AC-S-148**（免登：无同意页直接跳；`consent_required` 透出到授权页 query） | `OAuthAuthorize.svelte` |
| **AC-S-150** 步骤 2（**前端渲染**的 front-channel iframe：数量 / `src` / `sandbox`） | `OAuthLogout.svelte` |
| **AC-S-142** 步骤 4（设备码页：输入 `user_code` → 点「允许」） | `OAuthDevice.svelte` |
| **AC-S-147** 步骤 1/3（「我的授权」列表 + 撤销） | `OAuthGrants.svelte` |
| **AC-S-152** 步骤 3（两个 locale 各自覆盖 9 个码） | `packages/i18n/**` |

> 其余 AC 是棒 1 的活。**你不要**去实现后端逻辑，也不要在 Rust 侧加测试。

---

## 3. 钉死的耦合契约（**照抄，别自创**）

### 3.1 平台错误码 —— **恰好 9 个**（棒 1 写的，你写词库）

> **两个 locale 各写 9 条 `error.OAUTH_*`**。
> 键名 = `error.` + 码。**码逐字照抄下表**（棒 1 的 Rust 侧用的是同一批字符串）。

| # | 码 | 中文文案（`zh-CN`） | 英文文案（`en-US`） |
|---|---|---|---|
| 1 | `OAUTH_CLIENT_NOT_FOUND` | OAuth 客户端不存在 | OAuth client not found |
| 2 | `OAUTH_CLIENT_DISABLED` | OAuth 客户端已停用 | OAuth client is disabled |
| 3 | `OAUTH_CLIENT_EXPIRED` | OAuth 客户端已过期 | OAuth client has expired |
| 4 | `OAUTH_INVALID_REQUEST` | OAuth 请求参数非法 | Invalid OAuth request |
| 5 | `OAUTH_INVALID_REDIRECT_URI` | 回调地址不在白名单内 | Redirect URI is not registered |
| 6 | `OAUTH_INVALID_SCOPE` | OAuth 权限范围非法 | Invalid OAuth scope |
| 7 | `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` | 不支持的 PKCE 挑战方法 | Unsupported code challenge method |
| 8 | `OAUTH_TICKET_INVALID` | OAuth 同意票据无效 | OAuth consent ticket is invalid |
| 9 | `OAUTH_GRANT_NOT_FOUND` | OAuth 授权记录不存在 | OAuth grant not found |

- ⚠️ **只有 9 个**。**没有**第 10 个（设备码查不到 → 也是 `OAUTH_INVALID_REQUEST`，D257）。
- ⚠️ `packages/i18n/test/i18n.test.ts` 里那个硬编码的 codes 数组**要加上这 9 个**，
  并且断言 `Object.keys(catalog).filter(k => k.startsWith('error.')).toHaveLength(codes.length)`
  的**左值会随数组自动变成 93** —— 你**只加数组项**，**不要**改那个 `toHaveLength` 的形状。
- ⚠️ 词库是**扁平结构**（顶层直接 `error.XXX` / `oauth.xxx`，**不是**嵌套对象）。

### 3.2 页面文案（`oauth.*` 命名空间）

- 键名前缀 **`oauth.`**（design §9）。路由标题也放这里：`oauth.nav.authorize` /
  `oauth.nav.device` / `oauth.nav.grants` / `oauth.nav.logout`。
- 至少要能表达：同意页（标题 / 应用名 / 权限点列表 / 允许 / 拒绝 / 登录并继续）、
  设备码页（输入 `user_code` / 允许 / 拒绝 / 已批准 / 已拒绝 / 设备码无效）、
  授权列表页（标题 / 撤销 / 空态 / 状态文案）、
  登出页（已登出 / 正在通知其它应用）。
- **两个 locale 的键集合必须一致**（既有 i18n 测试会查）。

### 3.3 前端路由（**恰好 4 条**，路径逐字）

| 路由 | 公开 / 受保护 | 守卫声明 | 页面 |
|---|---|---|---|
| `/oauth/authorize` | **公开** | **不**声明 `permission`、**不**声明 `requiresAuth` | `OAuthAuthorize.svelte` |
| `/oauth/device` | **公开** | 同上 | `OAuthDevice.svelte` |
| `/oauth/grants` | **受保护** | `permission: 'oauth:grant:read'` | `OAuthGrants.svelte` |
| `/oauth/logout` | **公开** | 同上 | `OAuthLogout.svelte` |

- ⚠️ **`/oauth/authorize` 与 `/oauth/device` 必须是公开路由**（D221）：
  平台的会话是 **Bearer + sessionStorage**，浏览器**顶层导航不会带 `Authorization` 头**
  → 路由守卫若要求登录，未登录时会被改写到登录页，而守卫记的 `intendedPath`
  **只有 pathname**（`router.ts:261`）→ **授权参数与 `user_code` 全丢**。
  → 页面**自己**处理未登录（渲染「登录并继续」）。
- ⚠️ `/oauth/device` 尤其重要：它的 query 里带 **`user_code`**
  （`verification_uri_complete` = `{授权页基址}/oauth/device?user_code=…`）
  → 任何「重定向丢 query」的路径都会让设备码流程**不可用**。
- ⚠️ `manifest.json`（棒 1 的）的 `menus[].route` **逐字** = **`/oauth/grants`**
  → 你的路由表里也必须是这个字符串（D256 ③）。
- 公开路由的范式照抄 `apps/shell/src/lib/routes.ts:70-76` 的 `SSO_CALLBACK_ROUTE_PATH`。

### 3.4 后端接口形状（**棒 1 实现，你照抄** —— D258）

- **平台端点字段名 = camelCase**（契约 C13）；**协议端点 = RFC 的 snake_case**。
  你只调**平台端点**（协议端点是第三方应用调的）。
- 逐字的请求 / 响应字段见 `BA/demands/REQ-022-outbound-idp/decisions.md` **D258** 的表格。
  **下面这份是它的摘要，但以 D258 原文为准**：

| 端点 | 请求体 | 响应体（200） |
|---|---|---|
| `POST /api/oauth/authorize/preview` | `{ responseType, clientId, redirectUri, scope, state?, codeChallenge, codeChallengeMethod, prompt? }`<br>（`scope` 是**空格分隔的字符串**；`prompt` 可选） | `{ needsConsent, ticket?, application: { clientId, name }, scopes: string[], redirectTo? }`<br>（`ticket` 仅 `needsConsent === true` 时有；`redirectTo` 仅 `needsConsent === false` 时有） |
| `POST /api/oauth/authorize/confirm` | `{ ticket }` | `{ redirectTo }` |
| `POST /api/oauth/authorize/deny` | `{ ticket }` | `{ redirectTo }` |
| `GET /api/oauth/grants` | — | `[{ clientId, clientName?, scopes: string[], grantedAt, status, revokedAt? }]` |
| `POST /api/oauth/grants/revoke` | `{ clientId }` | `{ status: 'revoked' }` |
| `POST /api/oauth/logout` | — | `{ loggedOut: { grants, sessions }, frontChannel: [{ clientId, logoutUrl, logoutToken }] }` |
| `GET /api/oauth/device/verify?userCode=…` | 查询参数 | `{ clientId, clientName, scopes: string[] }`（**D257 补**） |
| `POST /api/oauth/device/approve` | `{ userCode, action: 'approve' \| 'deny' }` | `{ status: 'approved' \| 'denied' }`（**D257 补**） |

- 平台端点的**错误**是 **RFC 9457 Problem Details** → `code` 字段是 `OAUTH_*`（大写）。
  用 `errorMessageKey(caught.code)`（`apps/shell/src/lib/i18n.ts` 既有函数）→ `error.<CODE>`。
- `frontChannel` **可能为空数组**（不是 `null`）。

---

## 4. 逐页行为规格（**照抄实现**）

### 4.1 `OAuthAuthorize.svelte`（授权页 / 同意页）

**读 URL 的 query**（页面自己读 `window.location.search`，**不靠路由**）：
`response_type` / `client_id` / `redirect_uri` / `scope` / `state` /
`code_challenge` / `code_challenge_method` / `prompt`

**状态机**：

| # | 条件 | 动作 |
|---|---|---|
| 1 | **未登录** 且 `prompt != 'none'` | 渲染「**登录并继续**」按钮。**不调 preview**（preview 需要 Bearer）。<br>点击 → 把 **`location.pathname + location.search`（完整 URL，query 一个都不能少）** 编码进 `?return_to=` → 跳 `/login?return_to=<encoded>` |
| 2 | **未登录** 且 `prompt == 'none'` | **不跳登录页**（`prompt=none` 不许交互）→ 用 `history.replaceState` 把 URL 的 query 加上 **`error=login_required`**（D233）→ 渲染「需要登录」提示 |
| 3 | **已登录** | 调 `POST /api/oauth/authorize/preview`（Bearer），body 用上面的 query 参数（`scope` 原样传，**空格分隔的字符串**） |
| 3a | preview → **200 + `needsConsent === false`** | **`location.href = redirectTo`**（免登直发，带 `code` 与 `state`） |
| 3b | preview → **200 + `needsConsent === true`** | 若 `prompt == 'none'` → `history.replaceState` 把 URL 的 query 加上 **`error=consent_required`**（D233）→ **仍然渲染同意页**（D233 步骤 4：「页面显示同意页 + URL query 里 `error=consent_required`」）。<br>**渲染同意页**：client 的 `name` + 请求的**全部**权限点码（**逐字显示**，AC-S-146 步骤 1） |
| 3c | preview → **400 Problem Details** | 渲染错误（`error.<CODE>` 的 i18n 文案） |
| 4 | 同意页点「**允许**」 | `POST /api/oauth/authorize/confirm` body `{ ticket }` → `location.href = redirectTo` |
| 5 | 同意页点「**拒绝**」 | `POST /api/oauth/authorize/deny` body `{ ticket }` → `location.href = redirectTo`（query 里有 `error=access_denied` 与 `state`） |

⚠️ **`prompt` 缺省 = 允许交互**（AC-S-148 步骤 5）→ 缺省时**一定**走 3b（同意页），
**不是**静默直发。后端只在 `prompt == 'none'` 时才可能返回 `needsConsent === false`。

⚠️ **`history.replaceState` 而不是重新导航** —— 重新导航会把整页重载，
授权页的 `ticket` 状态就没了。**也不要把 `error=` 加到 `redirect_uri` 上**。

### 4.2 `OAuthDevice.svelte`（设备码批准页）

| # | 条件 | 动作 |
|---|---|---|
| 1 | 未登录 | **同 4.1 的分支 1/2**（`return_to` 必须**保留 `?user_code=`**） |
| 2 | 已登录 + URL 里有 `user_code` | 直接进入第 3 步 |
| 3 | 已登录 + URL 里**没有** `user_code` | 渲染输入框，让用户**手输** 8 位码 → 第 4 步 |
| 4 | 查设备码信息 | `GET /api/oauth/device/verify?userCode=<code>` → 200：显示 `clientName` + `scopes`（逐字）+ 「允许」/「拒绝」按钮；400 `OAUTH_INVALID_REQUEST`：显示错误 |
| 5 | 点「**允许**」 | `POST /api/oauth/device/approve` body `{ userCode, action: 'approve' }` → 200 `{ status: 'approved' }` → 渲染「已批准，可以在原设备继续」 |
| 6 | 点「**拒绝**」 | 同上，`action: 'deny'` → `{ status: 'denied' }` → 渲染「已拒绝」 |

⚠️ `user_code` 的字符集是 **base20**：`BCDFGHJKLMNPQRSTVWXZ`（**8 位**，无易混字符）。
输入时**不要**自动补连字符（后端存的是**无分隔**的 8 位）。

### 4.3 `OAuthGrants.svelte`（「我的授权」页）

| # | 内容 |
|---|---|
| 1 | **首屏数据在路由的 `load` 里取**（与 `AUDIT_ROUTE_PATH` **同一套机制**）：<br>守卫不放行 → `load` **不会**被调用 → `GET /api/oauth/grants` 的请求计数为 **0**。<br>⚠️ 既有坑：`onAudit?.(await …)` 会在没有回调时短路 → 请求根本不发 → 数据永远不来。**你的写法要避免这个坑**。 |
| 2 | 列表显示：client 名 / scope 列表（权限点码**逐字**）/ 授权时间 / 状态 |
| 3 | 撤销按钮 → `POST /api/oauth/grants/revoke` body `{ clientId }` → 200 → **刷新列表** |
| 4 | 空态：没有授权记录时显示空态提示（**不是**空白页） |

⚠️ **`GET /api/oauth/grants` 只返回当前用户自己的**（后端保证）→ 前端**不做**租户过滤。

### 4.4 `OAuthLogout.svelte`（登出页 + front-channel 渲染）

| # | 内容 |
|---|---|
| 1 | `POST /api/oauth/logout`（Bearer）→ 200 `{ loggedOut, frontChannel }` |
| 2 | **渲染 front-channel 的 iframe**（D225 / AC-S-150 步骤 2）：<br>• iframe 数量 **恰好** = `frontChannel.length`（**没有就一个都不渲染**）<br>• `src` **逐字** = `logoutUrl` + `?logout_token=` + `logoutToken`<br>• `sandbox` 属性**含** `allow-scripts`、**不含** `allow-same-origin`<br>• 每个 `frontChannel` 项**一个** iframe（`clientId` 不同的项各一个） |
| 3 | 渲染「已登出」提示 |

⚠️ **后端只返回 JSON，iframe 是前端渲染的**（D225）—— 不要让后端返回 HTML。

---

## 5. 登录回跳（D221）—— **`Login.svelte` + `App.svelte`**

### 5.1 问题

平台的 `intendedPath` **只有 pathname**（`router.ts:261`），
`write()` 在 path 变化时**丢 `search`** → 未登录回跳会**丢掉全部授权参数**。

### 5.2 方案（**不动 `router.ts`**）

1. `apps/shell/src/lib/oauth-api.ts` 里加一个**纯函数**：
   ```ts
   /** 从 query 里取并**校验** return_to；不合格返回 undefined。 */
   export function readReturnTo(search: string): string | undefined
   ```
   **校验规则**（**安全要求**，必须实现）：
   - 必须以 **`/`** 开头；
   - **不得**以 `//` 开头（那是协议相对 URL → **开放重定向**）；
   - **不得**含 scheme（`http:` / `https:` 等）；
   - 不合格 → 返回 `undefined` → **保持现行为**（跳首页）。

2. `App.svelte` 的 `handleLoggedIn()`：
   ```ts
   function handleLoggedIn() {
     authState = auth.state;
     const returnTo = readReturnTo(window.location.search);
     if (returnTo !== undefined) {
       window.location.replace(returnTo);   // 整页跳转 → query 一个不丢
       return;
     }
     router.navigate(resolution.intendedPath ?? '/');
     void loadI18nContext();
   }
   ```
   ⚠️ 用 **`location.replace`**（不是 `href`）→ 回跳地址**不进浏览器历史**，
   用户按「后退」不会回到授权页（安全约束 §8 的同款理由：SSO 回调的 `?ticket=` 就不该留历史）。

3. `Login.svelte`：
   - 读 `return_to`，**存在**时渲染一条提示（「登录后将返回原页面」）；
   - **不**改登录流程本身（租户 / 账号 / 密码 / SSO 入口都照旧）；
   - 登录成功后仍调 `props.onLoggedIn()`（由 `App.svelte` 决定跳哪儿）。

4. **没有** `return_to` → **保持现行为**（跳首页 / `intendedPath`）。

### 5.3 ⚠️ 你**必须**跑的回归

改 `Login.svelte` / `App.svelte` 会影响 **REQ-011 已交付**的前端用例 →
**必须跑前端全量回归**（design R10）：

```bash
pnpm --filter @base/shell test
```

既有用例文件（**都要过**）：`api` / `audit-api` / `audit-page` / `branding` / `can` /
`i18n` / `i18n-text` / `nav` / `router-params` / `router` / `schema-api` /
`schema-form-submit` / `schema-form` / `schema-table` / `session` / `sso` / `zero-code`。

---

## 6. i18n 词库（`packages/i18n/**` —— **归你，棒 1 不碰**）

| 文件 | 改什么 |
|---|---|
| `packages/i18n/src/locales/zh-CN.json` | + **9** 条 `error.OAUTH_*`（文案见 §3.1）+ `oauth.*` 命名空间的界面文案 |
| `packages/i18n/src/locales/en-US.json` | 同上（英文文案见 §3.1） |
| `packages/i18n/test/i18n.test.ts` | 把 9 个码加进那个硬编码的 `codes` 数组（**只加数组项**）；**不要**改 `toHaveLength` 的形状 |

- 词库是**扁平**结构（顶层 `error.XXX` / `oauth.xxx`），**不是**嵌套对象。
- 两个 locale 的**键集合必须一致**（既有测试会查）。
- ⚠️ **9 个 `error.OAUTH_*` 的键名 = 码逐字**（`error.` + 码）——
  棒 1 的 Rust 侧用同一批字符串，改一个就要改两处。

---

## 7. 测试与自验（**§9.14 阶段一：增量测试**）

> ⚠️ **本阶段不跑 E2E**（棒 1 的后端还没落地，E2E 归棒 1 写、BA 在终版统一跑）。

### 7.1 你要跑的命令

```bash
pnpm --filter @base/shell test        # vitest run
pnpm --filter @base/i18n test        # vitest run
pnpm --filter @base/shell check      # svelte-check --tsconfig ./tsconfig.json
pnpm --filter @base/i18n check       # tsc --noEmit
pnpm --filter @base/shell build      # vite build（产物给棒 1 的 E2E 用）
```

> ⚠️ **只跑前端三个包**（`@base/shell` / `@base/i18n` / `@base/sdk`）——
> **不要**跑 `pnpm -r test`（会带上一堆无关包，慢）。
> ⚠️ **`svelte-check` 必须过**（TS 类型错误会让终版全量红）。

### 7.2 你要写的测试

| 文件 | 至少覆盖 |
|---|---|
| `apps/shell/test/oauth-api.test.ts` | `readReturnTo` 的**安全校验**：合法相对路径 / `//evil.com`（协议相对）→ **拒绝** / `https://evil.com` → **拒绝** / 空 → `undefined`；API 层的路径常量与白名单 |
| `apps/shell/test/oauth-authorize.test.ts` | 授权页的**状态机**：未登录 + `prompt` 缺省 → 渲染「登录并继续」且**不调 preview**；`prompt=none` + 未登录 → query 出现 `error=login_required` 且**不跳登录页**；`needsConsent=true` → 渲染同意页且页面文本里**逐字出现**权限点码；`prompt=none` + `needsConsent=true` → query 出现 `error=consent_required` **且仍渲染同意页**；确认 → 用返回的 `redirectTo` 跳转；拒绝 → 同样跳 `redirectTo` |
| `apps/shell/test/oauth-device.test.ts` | 设备码页：`user_code` 在 query 里 → 自动查；不在 → 渲染输入框；「允许」→ 发 `{ userCode, action: 'approve' }` |
| `apps/shell/test/oauth-logout.test.ts` | front-channel iframe：**数量恰好** = `frontChannel.length`；`src` 逐字 = `logoutUrl + '?logout_token=' + logoutToken`；`sandbox` **含** `allow-scripts` **且不**含 `allow-same-origin`；**空数组 → 一个 iframe 都没有** |
| `apps/shell/test/oauth-grants.test.ts` | 授权列表渲染 + 撤销调用体 `{ clientId }` |
| `packages/i18n/test/i18n.test.ts` | 9 个码的登记（**既有文件里加项**）+ `oauth.*` 键两个 locale 一致 |

- 组件测试用 `svelte/server` 的 `render`（既有范式见 `apps/shell/test/audit-page.test.ts`）。
- ⚠️ **跨栈断言写「包含」不写「相等」**（D194）：
  例如「页面文本**包含** `rbac:role:read`」✓；「页面文本**等于** `rbac:role:read`」✗。

### 7.3 变异验证（**必须做**）

每条变异：**改生产那一行** → 跑相关测试确认**变红** → 用**备份副本**还原 →
**`touch`**（`cp -p` 保留旧 mtime → 复用陈旧产物 → 假绿）→ `sha256sum -c` 逐字节相同 →
`grep -c 'MUTATION' <file>` → **0**。

```bash
cp <file> /tmp/x.bak            # 备份（用 cp 不用 cp -p）
sha256sum <file> > /tmp/sha-before.txt
# 注入最小变异 → 跑测试 → 确认红
cp /tmp/x.bak <file> && touch <file>
sha256sum -c /tmp/sha-before.txt
grep -c 'MUTATION' <file>        # 必须 0
```

**建议至少覆盖**：

| 变异点 | 它该证明哪条断言不是空跑 |
|---|---|
| `readReturnTo` 的「`//` 开头」检查去掉 | 开放重定向被挡住（5.2） |
| 授权页的 `return_to` 改成**只带 pathname**（不带 `search`） | D221 的核心判据（query 还在）真的被断言 |
| `prompt=none` 分支改成「也跳登录页」 | `prompt=none` 不许交互被断言 |
| front-channel iframe 的 `sandbox` 去掉 `allow-scripts` | AC-S-150 步骤 2 的 `sandbox` 断言不是摆设 |
| iframe 的 `src` 少拼 `?logout_token=` | `src` 的逐字断言不是摆设 |

### 7.4 静态扫描 —— **必须带阳性对照**（出口标准 9.2）

- 若你做「前端不出现 `client_secret` 明文」之类的扫描 → **必须**先证明扫描器**会命中**
  （在临时副本里塞一个 → 必须命中），再说真实代码 **0 命中**。

---

## 8. 开工前必答项（**报告里逐条回答**）

1. **怎么判断「已登录」？** 平台会话在 **sessionStorage**。
   `apps/shell/src/lib/session.ts` / `auth-context.ts` 里既有机制是什么？用哪个？
2. **`/oauth/authorize` 是公开路由** → 守卫**不会**把它改写到登录页 →
   **未登录时是谁负责跳登录页？** 答案必须是「页面自己」→ 具体怎么做（§4.1 分支 1）。
3. **`return_to` 的编码**：用 `encodeURIComponent` 还是 `URLSearchParams`？
   两者对 `+` / `%` 的处理**不一样** → 你的编码与解码**必须成对**，并有测试。
4. **`location.replace(returnTo)` 会不会把 `return_to` 自己也带上？**
   登录页的 URL 是 `/login?return_to=…` → 回跳目标是**原始授权 URL**（不含 `return_to`）→
   确认不会形成「`return_to` 套 `return_to`」。
5. **`prompt` 缺省 vs `prompt=none` 的分支**在代码里怎么表达？
   ⚠️ `prompt` **可能**是 `none` / `login` / `consent` / `select_account`（RFC 6749 §3.1）
   → 本需求只实现 `none` 与「缺省」；**其它值**怎么处理（当缺省？还是报错）？**说清**。
6. **`history.replaceState` 加 `error=`** 会不会把原有的授权参数**覆盖掉**？
   → 必须**保留** `client_id` / `state` / `code_challenge` / `redirect_uri`（AC-S-137 步骤 3 的判据）。
7. **`ticket` 放在哪？** preview 返回的 ticket 只在**页面内存**里（不落 localStorage / sessionStorage）——
   确认你的实现**不持久化**它。
8. **`/oauth/grants` 的首屏 `load`**：怎么保证「守卫不放行 → 请求计数为 0」？
   （照抄 `AUDIT_ROUTE_PATH` 的写法，并**避开**既有那个短路坑。）
9. **i18n 的 `oauth.*` 键**你怎么组织？两个 locale 的键集合一致吗？
10. **`OAuthLogout.svelte` 的 iframe**：iframe 的 `src` 是第三方域 →
    `sandbox` 属性是**安全要求**，不是可选优化 → 确认实现了。

---

## 9. 报告（**写进 `feature-meta/req-022-dev-frontend.md`**）

> 路径：`/home/myg133/Projects/system_seed/feature-REQ-022/feature-meta/req-022-dev-frontend.md`
> （结构照抄 `BA/feature-meta/req-014-dev-frontend.md`，在 `feature/REQ-014` 分支上）

必含章节：

1. **交付清单（文件 → AC）** —— 逐文件一行
2. **前端测试数字** —— **自己跑出来的**：`@base/shell` / `@base/i18n` 各多少 passed / failed，
   并说清统计口径
3. **`svelte-check` 结果**（必须 0 error）
4. **变异验证证据** —— 逐条：变异点 → 变红的测试名 → 数字 → 还原校验结果
5. **开工前必答项（§8 的 10 条）逐条回答**
6. **与规格不一致的发现** —— 现状 / 证据（`file:line`）/ 怎么处理 / **需要 BA 拍板吗**
7. **未完成的部分 + 原因**
8. **对棒 2 文件集之外的任何改动** —— 一律列出来（应该是空的）
9. **给棒 1 的接口对齐清单** —— 你实际调用的**每个**端点 + 实际用到的**每个**字段名
   （棒 1 拿这份对照他实现的形状；**不一致就是要在终版前发现的问题**）

> ⚠️ **注释与代码必须一致**。注释比代码旧**比没有注释更坏**。

---

## 10. 明确的「本轮不做」

| 项 | 依据 |
|---|---|
| 改 `apps/shell/src/lib/router.ts` | D221 —— 改它会动 REQ-011 的路由核心 |
| client 管理界面（注册 / 改 / 停用 / 删 client） | D218 —— 本轮只做「同意页 / 设备码页 / 我的授权」三个页面 + 登出页 |
| 底座业务端点接受第三方 OAuth 令牌 | D209 / TD-041 —— 棒 1 的活（默认拒绝），前端无需配合 |
| refresh token 相关界面 | D217 |
| 改 `packages/i18n/**` 之外的前端包（`@base/sdk` 等） | 不在本需求范围 |

---

## 11. 完工信号

报告写完后，在**你的最终回复**里给出：

- 报告的**绝对路径**
- **前端测试数字**（`@base/shell` / `@base/i18n` 的 passed / failed，自己跑的）
- **`svelte-check` 是否 0 error**
- **变异验证条数** + 是否全部还原干净（`sha256sum -c` 全过 + `grep -c MUTATION` 全 0）
- **AC 覆盖**（你负责的那几条）
- **未完成项清单**（若有）
- **需要 BA 拍板的问题**（若有）
- **给棒 1 的接口对齐清单**（§9 第 9 条）
