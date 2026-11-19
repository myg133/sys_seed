# 修复轮复核报告 —— REQ-011（前端 SDK 骨架）

- **需求**: REQ-011「前端 SDK 骨架」
- **被验提交**: `4ac5074`（`[Dev] 修复壳层租户来源与会话持久化`）+ `4292c4e`（BA 修 prettier 门禁）
- **首轮报告**: [`qa-report.md`](./qa-report.md)（结论 **FAIL**：HIGH-1 + AC-S-068 步骤 3）
- **验收基线**: `BA/demands/REQ-011-frontend-sdk/acceptance.md` **rev 3**（AC-S-068 及其「租户与会话的来源」补充）
- **裁定依据**: D98（登录页租户输入）/ D99（`sessionStorage`）
- **验证者**: QA Agent（**独立复核 —— 未参与本轮修复；Dev 自验数字一律不采信**）
- **日期**: 2026-11-19
- **结论**: **PASS-WITH-RISK**

> **核心结论**
>
> - **HIGH-1 已真修好**：真机 + 真浏览器（Playwright/Chromium）实测登录请求带 `x-tenant-id`，
>   值**等于用户填的租户**（填 `tenant-qa-probe` 头就是 `tenant-qa-probe`，不是硬编码），登录 **200**。
> - **MED-1 / AC-S-068 步骤 3 已达成**：`u-perm-read` 直接敲 `/admin/roles` → 落 **`/403`**，
>   目标接口 `/api/rbac/roles` 请求计数 **0**；`page.reload()` 后**仍**是 `/403`；
>   写权限用户深链 → 正常进页面。
> - **两条告警（本轮新引入，均未破坏 AC）**：
>   1. **MED-NEW-1（用户可见）**：登录失败时页面显示 **`登录失败（TOKEN_REVOKED）`**，
>      而不是 `INVALID_CREDENTIALS` 对应的「用户名或密码不正确」 —— 因为 SDK 对登录的 401
>      也发起了一次**注定失败**的 refresh，refresh 的错误把登录错误**覆盖**了。
>      Dev 报告声称的「按码分派文案」在真机上**不成立**。
>   2. **MED-NEW-2（测试资产）**：`e2e/shell-auth.spec.ts` 依赖的三个夹具**不可复现** ——
>      跑过 `./scripts/test-integration.sh` 之后，`u-perm-write@example.com` **不存在**、
>      `u-perm-read@example.com` 有 **6 个同名用户**（登录 401），E2E **实测 5 failed**。
> - **3 处变异全部被我的用例抓红**（M1 去租户 / M2 关 sessionStorage / M3 我自选的「先守卫后恢复」），
>   变异全部**逐字节还原**（`sha256 -c` 通过、`git diff -- apps/ packages/ crates/ e2e/` 为空）。
> - **全量回归两轮全绿**：后端 **550 passed ×2**、前端 **141 passed**、`xtask --all-targets` **49 passed**、
>   `clippy` **0 告警**、`capabilities --check` **13 文件一致**、`eslint` **0 问题**、`prettier --check .` **全绿**。

---

## 1. HIGH-1 与 MED-1 逐条复核

### 1.1 HIGH-1：壳层没有租户来源 → 登录 100% 失败

| 复核项 | 首轮的问题 | 本轮实测（真机 + 真浏览器） | 结论 |
|---|---|---|---|
| 登录请求带租户头 | `createAppApi()` 不传 `tenantId` → 请求无 `x-tenant-id` → **401 `TENANT_CONTEXT_MISSING`** | 登录请求头 `x-tenant-id: "tenant-a"`，**值等于表单填的租户** | **PASS** |
| 租户值不是硬编码 | —— | 填 `tenant-qa-probe` → 头就是 `"tenant-qa-probe"`；响应 `401 INVALID_CREDENTIALS`（不是 500、不是 `TENANT_CONTEXT_MISSING`） | **PASS** |
| 登录成功 | 100% 失败 | `POST /api/iam/login` → **200**，落 `/` 并渲染「已就绪」 | **PASS** |
| 后续请求沿用租户 | —— | 登录后所有经 SDK 的请求（`/api/iam/*`、`/api/rbac/*`）**全部**带 `x-tenant-id: tenant-a`，缺失 0 条 | **PASS** |
| **refresh 也带租户头** | —— | 把 `sessionStorage` 里的 access token 改成垃圾后重载：`POST /api/iam/refresh` **确实带** `x-tenant-id: tenant-a`，刷新 **200**，重放成功 | **PASS** |
| 空租户不发请求 | —— | 租户留空点登录 → 页面提示「请填写租户标识」，`/api/iam/login` 请求计数 **0**，仍停在 `/login` | **PASS** |
| 恢复会话时也设租户 | —— | 整页重载后 `GET /api/rbac/me/permissions` **带** `x-tenant-id: tenant-a` + `Authorization: Bearer …` | **PASS** |

**实现落点（我逐行读过）**：
- `apps/shell/src/lib/api.ts:168-180` —— `login()` 先 `session.setTenant()` 再 `client.setContext({tenantId})`，**然后**才发登录请求；
- `apps/shell/src/lib/api.ts:132-161` —— `restoreSession()` 先 `client.setContext({tenantId})` 再请求权限点；
- `apps/shell/src/lib/api.ts:110` —— `createApiClient({ tenantId: session.state.tenantId })`，构造期就用已恢复的租户；
- `apps/shell/src/pages/Login.svelte:84-94` —— 空租户 `trim()` 后直接 return，**不发请求**；
- `apps/shell/src/pages/Login.svelte:116-122` —— 租户输入框（`name="tenant"`），无 `import.meta.env` / `VITE_*` 读取（`grep` 无命中）。

**后端契约侧证（curl，同一台机器）**：
```
POST /api/iam/login 无 x-tenant-id          → 401 {"code":"TENANT_CONTEXT_MISSING"}
POST /api/iam/login x-tenant-id: tenant-a   → 200 permissions=["rbac:role:read"]
```

### 1.2 MED-1 / AC-S-068 步骤 3：整页重载丢令牌 → 深链落 `/login` 而不是 `/403`

| 步骤 | 首轮的问题 | 本轮实测 | 结论 |
|---|---|---|---|
| 2 程序化跳转 / 深链（已登录、无权限） | 已登录无权限 → 403 ✓ | `u-perm-read` 敲 `/admin/roles` → 最终 URL **`/403`**，`[data-testid=forbidden]` 可见 | **PASS** |
| 3 **整页重载后深链** | **落 `/login`**（令牌只在内存） | 同上，**落 `/403`**；在 `/403` 上再 `reload()` **仍**是 `/403` | **PASS** |
| 4 目标接口未被请求 | 计数 0 ✓ | `/api/rbac/roles` 请求计数 **0**（导航前被守卫拒绝，重载后仍 0） | **PASS** |
| 3' 有写权限用户深链 | —— | `u-perm-write` 敲 `/admin/roles` → 落在该页，`/api/rbac/roles` **200**，重载后仍在页 | **PASS** |
| 5 未登录深链 | 落登录页、无「403」字样 ✓ | 新上下文直接敲 `/admin/roles` → **`/login`**，页面正文**不含**「403」 | **PASS** |
| 补充：无权限用户 | —— | `u-perm-none` 登录 **200** 且 `permissions === []`；首页 `<Can>` 落 fallback「无权限」；深链 → `/403` | **PASS** |
| 补充：新标签页不继承 | —— | 同 context 新开页敲 `/admin/roles` → `/login`（`sessionStorage` 语义） | **PASS** |
| 补充：会话内容 | —— | `sessionStorage["base.shell.session.v1"]` 键**恰好** `accessToken,refreshToken,tenantId`；**不含** `permissions` | **PASS** |

**实现落点**：`apps/shell/src/lib/session.ts:25,101-197`（键 + 探测 + 降级 + 落盘）、
`apps/shell/src/App.svelte:95-100`（**先 `await restoreSession()` 再 `router.start()`**）、
`apps/shell/src/App.svelte:113-115`（恢复完成前不渲染页面）。

> **说明（我自己的断言自审）**：`B2`（「重载后回到首页」）**不具区分度** ——
> 首页是公开页，未登录也渲染「已就绪」。真正守住 MED-1 的是 `B1`（深链必落 403）
> 与 `B2b`（`/403` 上重载仍 403）；M2 变异下这两条都变红（见 §3），说明判据有效。

---

## 2. 本轮新引入的问题

### 2.1 【MEDIUM】MED-NEW-1：登录失败文案被 refresh 的错误覆盖（用户可见）

- **触发条件**：登录返回 **401**（最常见：密码错 / 用户名不存在 / 租户填错）。
- **观察到的行为**（真浏览器，`u-perm-wildcard@example.com` + 故意错密码）：
  ```
  REQ POST /api/iam/login
  REQ POST /api/iam/refresh                 ← 多出来的一次请求（必然失败）
  RES 401 /api/iam/refresh code=TOKEN_REVOKED
  RES 401 /api/iam/login   code=INVALID_CREDENTIALS
  页面文案: 登录失败（TOKEN_REVOKED）        ← ✗ 不是「用户名或密码不正确」
  URL: /login
  ```
- **根因链**：
  1. `packages/auth/src/index.ts:278` —— 登录走 `client.post(loginPath, …)`，与普通请求同一条通道；
  2. `packages/sdk/src/index.ts:314-327` —— 任何 `attempt === 0` 的 **401** 都会尝试 refresh
     （`RequestOptions` 没有「跳过刷新」的开关，`packages/sdk/src/index.ts` 的 `RequestOptions` 只有
     `method/path/body/query/headers/idempotent`）；
  3. 登录时 `session.state.refreshToken` 为 `undefined`，回调拿空串去刷 → 后端 `TOKEN_REVOKED`；
  4. SDK 把**刷新错误**抛出，登录错误被替换；
  5. `apps/shell/src/pages/Login.svelte:61-82` 的 `messageFor()` 按 `code` 分派，于是显示 `TOKEN_REVOKED`。
- **这是本轮引入的**：首轮版本 `Login.svelte`（`efc5084:apps/shell/src/pages/Login.svelte:40-42`）
  对所有失败**固定**显示「登录失败：用户名或密码不正确」—— 虽然笼统，但对最常见的密码错是**对的**。
  本轮把「按码分派」写上去后，最常见路径反而变成**误导性文案**。
  同时 `dev-report-fix.md:25` 声称「`INVALID_CREDENTIALS` / `TENANT_CONTEXT_MISSING` / … 各有文案」
  —— **真机上不成立**（登录 401 的任何码都到不了这段 switch）。
- **影响**：用户看到「会话已撤销」而实际是密码错；每次登录失败**多打一次** `POST /api/iam/refresh`
  （无谓的登录限流计数与日志噪声）。**不是安全漏洞**（无令牌泄漏、无放行）。
- **建议**（本轮未改，只提建议）：登录 / 刷新请求不触发自动刷新
  （在 `RequestOptions` 增 `skipRefresh`，或 SDK 对 `loginPath` 特判）；
  或 `messageFor` 优先用首个错误；并补一条**真浏览器**用例断言错密码时的文案。

### 2.2 【MEDIUM】MED-NEW-2：E2E 夹具不可复现（干净库上 E2E 必红）

- **触发条件**：按本轮验收命令的顺序跑 —— 先 `./scripts/test-integration.sh`，再跑 E2E。
- **实测**（集成两轮之后、`base serve` 仍在此库上）：
  ```
  $ BASE_E2E_URL=http://127.0.0.1:8097 npx playwright test e2e/
    5 failed / 4 passed
  ```
  库内实际情况：
  ```
  username=u-perm-read@example.com  count=6      ← 重名，登录 401 INVALID_CREDENTIALS
  username=u-perm-none@example.com  count=2
  username=u-perm-write@example.com count=0      ← 根本不存在
  ```
  `curl` 复验：`POST /api/iam/login {username:"u-perm-read@example.com"}` → **401 INVALID_CREDENTIALS**。
- **根因**：
  - E2E 硬编码三个账号（`e2e/shell-auth.spec.ts:16-19,27-40`），但**没有任何播种脚本**创建它们；
  - 唯一的创建者是 `crates/platform-rbac/tests/me_permissions.rs:110-124,331`，
    它把 `username` 当邮箱再拼一次域名（`email = format!("{username}@example.com")`
    → 库里出现 `u-perm-read@example.com@example.com`），且**同一次套件内重复注册 6 次**；
  - `u-perm-write@example.com` **没有任何测试创建**（`grep -rn u-perm-write --include=*.rs crates/` 无命中）。
- **影响**：Dev 报告的「7 passed / 9 passed」只在**手工准备好的库状态**下成立；
  任何人照「跑回归 → 跑 E2E」的顺序都会得到红。**产品本身没问题**（我的 34 条断言在同一套件
  于正确夹具状态下全绿），红的是**测试资产的可复现性**。
- **建议**：给 E2E 加显式播种（`scripts/e2e-seed.sh` 或 Playwright 的 `globalSetup`），
  用**独立的用户名**（不与后端测试共用），并在 `playwright.config.ts` 里串起来。

### 2.3 已核对、**未**被本轮破坏的项（C 节逐项）

| 项 | 怎么验的 | 结果 |
|---|---|---|
| **权限点不被持久化** | 真机读 `sessionStorage`：键恰好 3 个、全文不含 `permission`；重载后**现取** `/api/rbac/me/permissions`（计数 1 次） | **PASS** |
| **刷新不覆盖权限点** | 代码：`api.ts:97-109` 的 `refreshToken` 回调**只** `session.setTokens(...)`，不碰权限点；单测 `apps/shell/test/api.test.ts:211-250` 用带 `permissions: []` 的刷新响应断言权限点仍是登录时的集合 | **PASS（代码 + 单测级，见 §4 局限）** |
| **降级路径** | 真浏览器 `addInitScript` 让 `window.sessionStorage` 的 getter 抛错：**0 个 pageerror**，console 有「`[session] sessionStorage 不可用，退回内存模式`」告警，**仍能登录并渲染首页** | **PASS** |
| **D92（重放不带幂等键）** | `packages/sdk/src/index.ts:286-294`：`attempt > 0` 时 `headers.delete('idempotency-key')`，且首轮的双点变异仍能抓红对应单测 | **PASS** |
| **AC-S-067 `<Can>`** | 真浏览器：无权限用户首页原始 `innerHTML` **不含**「新建角色」，也不含 `display:none` / `visibility:hidden`；fallback「无权限」可见 | **PASS** |
| **AC-S-073/074 主题** | 真浏览器：9 个令牌变量全在；`--base-primary = #111111`（**等于**品牌接口值）；`color-scheme`/`data-theme` = `light`；本轮 `git diff` **未触碰** `packages/theme`、`packages/ui`、`apps/shell/src/lib/branding.ts` | **PASS** |

> **INFO（非本轮引入）**：会话中途失效时 `onUnauthorized` 会把 `api.ts` 内部的 `current` 置为
> anonymous，但 `App.svelte` 的 `authState` 只在登录/登出/启动时同步（`efc5084` 的版本同样是这个模式），
> 页面会按「已登录」渲染到下一次导航为止。服务端仍会 401，不构成越权，登记备查。

---

## 3. 变异验证表

> 纪律：**改 → `pnpm --filter @base/shell build`（E2E 打的是 dist）→ 跑真浏览器用例 →
> `cp` 原文件回来 → `sha256sum -c` + `git diff --exit-code -- apps/ packages/ crates/ e2e/` → 重建 → 复跑确认绿**。
> 三次变异后源码树与 `HEAD` 逐字节一致。

| # | 变异点 | 预期变红的用例 | **实际是否变红** | 结论 |
|---|---|---|---|---|
| **M1** | 登录前不绑租户：删 `apps/shell/src/lib/api.ts:177` 的 `client.setContext({tenantId})` | 我的 A1（头部）/ A2（200）/ A2b / C1 | **变红 ✓**：`A1 x-tenant-id=undefined`、`A2 status=401`、`A2b url=/login`、`C1 keys=`（4 条）；仓库 E2E **5 failed / 4 passed** | 抓到 |
| **M2** | 关掉 `sessionStorage`：`apps/shell/src/lib/session.ts:103` 令 `const storage = undefined`（`detectStorage()` 恒返回 `null`） | B1（深链 → 403）/ B2b / B4c / C1 | **变红 ✓**：`B1 url=/login`、`B1b forbidden=0`、`B2b url=/login`、`B4c url=/login`、`C1 keys=`；仓库 E2E **4 failed** | 抓到（**深链确实退回登录页**） |
| **M3**（我自选） | 启动顺序反过来：`apps/shell/src/App.svelte:96-99` 把 `router.start()` 提到 `await restoreSession()` **之前** | B1 / B2b / B4c | **变红 ✓**：`B1 url=/login`、`B1b`、`B2b`、`B4c` 全红（22/26） | 抓到（证明「先恢复再守卫」是**承重**的） |

### 3.1 对「变异没变红」的自审

- **M2 下 `B2`（重载后回首页）仍是绿的** —— 这是我的断言**不具区分度**，不是实现漏了：
  首页 `/` 是**公开路由**，未登录也渲染「已就绪」。真正的判据是 `B1`/`B2b`/`B4c`（全部变红）。
  报告已如实标注，不把这条当作「通过」。
- **M1 只删「登录前 setContext」就足以打红**：因为 `createApiClient` 构造时读的是
  `session.state.tenantId`（新会话为空），不存在第二道闸门。
- 我**没有**去变异「恢复会话时的 `setContext`」（`api.ts:139`）：后端在缺 `x-tenant-id` 时会
  **从令牌推断租户**（AC-S-072 步骤 2 的原因），所以那一行是**防御性**的，单点变异预期不红 ——
  这是分析结论，不是我实测过的（未列入上表）。

---

## 4. 没验到的（**不要**当成通过）

| 项 | 状态 | 原因 / 说明 |
|---|---|---|
| **「刷新不覆盖权限点」真机隔离验证** | **仅代码 + 单测级** | 要让真机在**会话中途**触发 refresh，需要篡改内存里的 access token；我不改实现就做不到。代码上 `api.ts:97-109` 明确只写令牌，且 `current.permissions` 只在登录/恢复时赋值 —— 结论可靠但**不是端到端实测**。另外我注意到该单测的 `expect(api.auth.state.permissions)` 断言**偏弱**（`current` 是登录时的快照，即便回调覆盖了 store 的权限点也不一定翻红）；真正的保护来自代码结构。 |
| **AC-S-073 暗色模式切换** | **只跑了包级单测** | `apps/shell` 没有暗色切换入口（`App.svelte:104` 固定 `setThemeMode('light')`）；本轮**未改** `packages/theme`。`pnpm -r test` 中 theme **12 passed**。真机只验了亮色令牌与品牌主色。 |
| **AC-S-070 并发刷新只 1 次** | **只跑了既有单测** | AC 明确要求该步骤是前端单测（禁止对真后端并发 refresh），我未改判据。 |
| **断网 / 离线时的 `restoreSession` 行为** | **未验** | 只验了 `sessionStorage` 不可用的降级，没做网络层断连。Dev 自述「保留会话、按未登录渲染」，我**未复现**。 |
| **跨浏览器** | **未验** | 只用 Chromium（项目配置只有一个 project）。 |
| **新标签页继承的边界（复制标签页 vs 新开）** | **部分** | 只验了 `context.newPage()`（同一 context 的新页）→ 不继承。浏览器「复制标签页」会复制 `sessionStorage`，未验。 |
| **`429 / 账号锁定` 等非 401 登录失败的文案** | **未验** | 需要构造限流/锁定状态；从代码看非 401 不会被 refresh 覆盖，但**未实测**。 |

---

## 5. 实跑数字（全部我自己跑，未采信 Dev 报告）

### 5.1 环境

```
真实服务：cargo run -p platform-server --bin base -- serve
  SERVER_PORT=8097  DATABASE_URL=postgres://base_app:base@127.0.0.1:5432/base_test
  BASE_STATIC_DIR=apps/shell/dist  BASE_MANIFEST_ROOT=.  BASE_LOG_LEVEL=warn
  PKCS#8 EC 私钥 + base64url secret（/tmp/jwt-qa3.pem / /tmp/secret-qa3.txt）
  /health/live → 200，/ → 200
浏览器：Playwright + Chromium（node v26.13.1 / pnpm 12.19.0 / rustc 1.98.1）
```

### 5.2 我的独立浏览器脚本（**不是** Dev 的 E2E）

```
BASE_E2E_URL=http://127.0.0.1:8097 node /tmp/qa3-verify.mjs     → 34/34 passed
  覆盖：A1 A2 A2b A3 A3b-i/ii/iii/iv A4 C1 C1b C2 B1 B1b B2 B2b B3 B3b
        B4a B4b B4c B5 B5b B6 B6b B7 C3a C3b C3c C5 C5b C6a C6b C6c
补充脚本（A1b/A1c/A1d/A1e 通过；A5 因库被集成测试重置而**未跑完**，见 §2.2）
```

> ⚠️ **诚实声明**：34/34 是在**集成回归之前**、夹具完好的库状态下取得的（当时 SQL 核对过
> 三个用户都在 `tenant-a`）。跑完 `test-integration.sh` 后夹具被冲掉，**同一份脚本无法复跑** —— 见 MED-NEW-2。

### 5.3 仓库自带 E2E

```
# 夹具完好时（集成回归之前）
BASE_E2E_URL=http://127.0.0.1:8097 npx playwright test e2e/
  → 9 passed (7.1s)      # shell-auth 7 条 + shell 2 条

# 跑过两轮集成回归之后（同一命令）
  → 5 failed / 4 passed (39.2s)    # 夹具被重置，见 MED-NEW-2
```

### 5.4 全量回归（两轮）

| 命令 | 结果 |
|---|---|
| `./scripts/test-integration.sh`（第 1 轮） | **EXIT=0**，合计 **550 passed**（29 个套件，0 failed） |
| `./scripts/test-integration.sh`（第 2 轮） | **EXIT=0**，合计 **550 passed**（0 failed） |
| `pnpm -r check` | **0 errors / 0 warnings**（svelte-check） |
| `pnpm -r test` | **141 passed**（schema 3 · i18n 3 · hooks 17 · sdk 30 · theme 12 · auth 25 · ui 14 · shell 37） |
| `pnpm -r build` | **exit 0**（shell：132 modules → `dist/assets/index-CpyjVosp.js` 64.09 kB） |
| `pnpm exec eslint .` | **exit 0**（无输出 = 0 问题） |
| `npx prettier --check .` | **All matched files use Prettier code style!**（exit 0） |
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0，0 告警** |
| `cargo xtask capabilities --check` | **13 个文件与源码一致**（exit 0） |
| `cargo test -p xtask --all-targets` | **49 passed / 0 failed**（含 `agents_doc_matches_catalog`） |

### 5.5 纪律确认

```
sha256sum -c /tmp/qa3-before.sha256                     → 3 个文件全部「成功」
git diff --exit-code -- apps/ packages/ crates/ e2e/   → 空（源码树与 HEAD 逐字节一致）
git status --porcelain                                  → 只有 feature-meta/qa-report.md（上一轮 QA 的未提交改动，非我所改）+ 本报告
本轮未 commit、未改实现（除已还原的 3 处变异）
```

---

## 6. 结论与建议

**VERDICT: PASS-WITH-RISK**

- **HIGH-1：已修好**（真机验证，非单测）。
- **MED-1 / AC-S-068 步骤 3：已达成**（深链、整页重载、目标接口计数 0 全部实测）。
- **AC-S-068 补充（D98/D99）的 5 条验收要求逐条达成**：
  登录页有租户输入 ✓ / 登录请求带租户头且等于用户填的值 ✓ / 后续请求沿用 ✓ /
  重载后会话仍在 ✓ / 新标签页不继承 ✓。

**合入前建议（不影响 AC 判定，但建议同轮修）**：

1. **MED-NEW-1**（2~3 行）：登录/刷新的 401 不应触发自动刷新，否则「按码分派文案」这句改进
   在真机上是失效的，且每次登录失败多打一个 refresh 请求。**建议加一条真浏览器用例**
   （错密码 → 断言文案含「用户名或密码不正确」），否则下一轮还会「看起来修了实际没修」。
2. **MED-NEW-2**：给 E2E 加**显式播种**（不复用后端测试的账号），否则「跑完回归再跑 E2E」必红，
   下一轮复核会为此重新排查一遍。
