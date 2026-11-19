# 修复报告 —— HIGH-1（壳层租户来源）+ MED-1（会话持久化）

- **需求**: REQ-011「前端 SDK 骨架」
- **分支**: `feature/REQ-011`（未合入 `develop`）
- **提交**: `[Dev] 修复壳层租户来源与会话持久化 (关联: REQ-011)`
- **依据**: `BA/demands/REQ-011-frontend-sdk/acceptance.md` **rev 3**（AC-S-068 及其
  「补充：租户与会话的来源」）、`feature-meta/qa-report.md`（HIGH-1 / MED-1）、
  `BA/demands/REQ-001-platform-foundation/decisions.md`（**D98 / D99**）
- **改动范围**: **仅 `apps/shell/` 与 `e2e/`**；`crates/` 与 `packages/` **零改动**
  （`git diff --stat` 可验）
- **日期**: 2026-11-19

---

## 1. 两个问题的修法

### HIGH-1：登录页加租户输入（D98）

| 环节 | 实现位置 | 做了什么 |
|---|---|---|
| 租户输入 | `apps/shell/src/pages/Login.svelte:117` | 登录页第一个字段 `<Input label="租户" name="tenant" placeholder="例如 tenant-a">`，用户填租户标识 |
| 空租户拦截 | `apps/shell/src/pages/Login.svelte:84-92` | `submit()` 先 `trim()` 校验；为空 → 显示「请填写租户标识」并 **return（不发请求）** |
| 绑定到 SDK 上下文 | `apps/shell/src/lib/api.ts:168-181` | `auth.login({tenantId, username, password})`：先 `session.setTenant(tenantId)` + `client.setContext({ tenantId })`，**再**发登录请求 |
| 租户随会话持久化 | `apps/shell/src/lib/api.ts:118-123`（`onLogin` → `session.setTokens`） | 登录成功后租户与令牌一起写入 `sessionStorage` |
| 登录失败文案按码分派 | `apps/shell/src/pages/Login.svelte:61-82` | `INVALID_CREDENTIALS` / `TENANT_CONTEXT_MISSING` / `ACCOUNT_LOCKED` / `ACCOUNT_DISABLED` / `LOGIN_RATE_LIMITED` 各有文案，其余 `登录失败（<code>）` —— 不再把所有失败都说成「用户名或密码不正确」 |

**没有**硬编码租户，**没有**从 `import.meta.env` / `VITE_*` 读死（违反多租户底座）。

### MED-1：会话（令牌 + 租户）存 `sessionStorage`（D99）

| 环节 | 实现位置 | 做了什么 |
|---|---|---|
| 存储键与结构 | `apps/shell/src/lib/session.ts:25` | `base.shell.session.v1`，只存 `{accessToken, refreshToken, tenantId}` |
| 可用性探测 + 降级 | `apps/shell/src/lib/session.ts:101-119` | `detectStorage()`：不存在 / 读写抛错 → **返回 null（仅内存）** + `console.warn`，**不抛未捕获异常** |
| 反序列化校验 | `apps/shell/src/lib/session.ts:68-99` | 损坏 JSON / 缺 `accessToken` / **缺 `tenantId`** → 不恢复，并清掉旧键 |
| 写入 | `apps/shell/src/lib/session.ts:124-196` | `setTenant` / `setTokens` / `clear` 每次变更都同步落盘；写失败降级为内存 |
| 启动时恢复 | `apps/shell/src/lib/api.ts:132-163` | 有令牌 + 租户 → **先 `client.setContext({tenantId})`** → `GET /api/rbac/me/permissions` **现取**权限点 |
| 恢复顺序（关键） | `apps/shell/src/App.svelte:95-100` | `await restoreSession()` → `booted = true` → **`router.start()`**；恢复完成前不渲染任何页面（`App.svelte:113`） |

**三条约束的落点**：

1. **权限点不进 `sessionStorage`** —— 存储里只有 3 个键（真机实测见 §3），
   恢复时现取 `GET /api/rbac/me/permissions`（`api.ts:141`）。
2. **刷新响应不覆盖权限点** —— `refreshToken` 回调只写令牌（`api.ts:97-109`），
   单测 `test/api.test.ts`「401 → 刷新 → 重放」用**带 `permissions: []` 的刷新响应**
   证明权限点仍是登录时的集合。
3. **租户跟着会话持久化** —— 见 `session.ts:178-184` 与恢复路径 `api.ts:137-138`。

---

## 2. 端到端验证（**真服务 + 真浏览器**，不是单测）

服务：`SERVER_PORT=8098` + `base_test`（`base_app` 连接）+ `BASE_STATIC_DIR=apps/shell/dist`，
迁移已跑（`base migrate up` → 无待应用迁移），`/health/live` = **200**。

### 2.1 HIGH-1 的根因与修复（同一台机器、同一个库）

```bash
# 修复前的问题形态（QA 证据链）等价复现：不带租户头 → 后端拒绝
$ curl -o /dev/null -w "%{http_code}\n" -X POST .../api/iam/login -d '{...}'          # 无 x-tenant-id
401
$ curl -X POST .../api/iam/login -H 'x-tenant-id: tenant-a' -d '{...}'
{"accessToken":"eyJ0eXAiOiJKV1Qi...","refreshToken":"...","permissions":[...]}
```

真浏览器（Playwright / Chromium，`BASE_E2E_URL=http://127.0.0.1:8098`）：

```
[green] 登录请求 x-tenant-id = "tenant-a"      ← 登录请求确实带上了租户头
[green] 登录完成 URL = /
[green] sessionStorage 键 = accessToken,refreshToken,tenantId |
        tenantId = tenant-a | accessToken 长度 = 473 | 含 permissions? = false
[green] 整页重载后 URL = / | 登录表单 = 0        ← 会话跨整页重载存活
[green] 深链后 URL = /403 | 403元素 = 1 | 目标接口请求数 = 0   ← AC-S-068 步骤 3
```

（第 3 行的 `含 permissions? = false` 就是约束 1 的实测：权限点**没有**被持久化。）

### 2.2 E2E 用例结果（7 条，全部真浏览器）

```
✓  1 登录页填租户 → 登录请求带 x-tenant-id，租户随会话持久化
✓  2 租户为空 → 给出提示且不发登录请求
✓  3 只读用户直接敲 /admin/roles → 403 页，目标接口不被请求（重载后仍 403）
✓  4 有写权限的用户直接敲 /admin/roles → 落在该页面并加载数据
✓  5 无任何权限的用户：登录成功但权限点为空，受保护页仍拦
✓  6 未登录直接敲受保护 URL → 落登录页，页面不出现 403 字样
✓  7 新开标签页不继承会话（sessionStorage 语义）
  7 passed (8.5s)
```

其中既有 REQ-010 的 2 条 E2E 也一起跑过：`e2e/` 全量 **8 passed (7.0s)**（6 条新 + 2 条旧；
第 7 条是之后补的，单跑 `shell-auth.spec.ts` 为 7 passed）。

用例覆盖的硬断言（不是「看起来对」）：

- 登录请求头 `x-tenant-id === 'tenant-a'`（`e2e/shell-auth.spec.ts:42`）；
- `sessionStorage` 里 `tenantId === 'tenant-a'`、`accessToken` 非空（`:42`）；
- 只读用户深链：落点 `/403`、`forbidden` 元素可见、`/api/rbac/roles` 请求数 **0**（`:93`）；
- 写用户深链：落点 `/admin/roles`、`角色管理` 可见、`/api/rbac/roles` 返回 **200**（`:119`）；
- 无权限用户：**登录响应的 `permissions` 严格等于 `[]`**、首页写入口 0 个、fallback「无权限」可见、深链落 `/403`（`:137`）；
- 新标签页：落 `/login`（`:165`）。

---

## 3. 变异验证（**确认测试能抓住这两个回归**）

纪律：**改 → 跑真实用例 → 逐字节还原（`git diff --exit-code` / sha256）→ 复跑确认变绿**。

### M1 —— 「租户不传」（HIGH-1 的回归）

变异点：`apps/shell/src/lib/api.ts` 删掉 `session.setTenant(...)` 与 `client.setContext({tenantId})`。

| 层 | 实际结果 |
|---|---|
| 单测 | **红**：`test/api.test.ts` → `Tests 2 failed | 5 passed`（`登录请求带 x-tenant-id…`、`401 → 刷新 → 重放…`） |
| 真机 E2E | **红 4 / 绿 2**：登录类用例失败（`expected header x-tenant-id` → `null`），三个需要登录的深链用例因登录 401 全部失败 |

还原：`sha256(api.ts) = ade16675…f179`（与变异前一致），`git diff --exit-code HEAD -- apps/shell/src/lib/api.ts` **通过**。

### M2 —— 「去掉 sessionStorage」（MED-1 的回归）

变异点：`apps/shell/src/lib/session.ts:101` `detectStorage()` 直接 `return null`（默认退回内存）。

真机实测落点（与 QA 的 MED-1 现象一致）：

```
[M2] 登录后 URL = /
[M2] sessionStorage = null
[M2] 深链后 URL = /login | 403元素 = 0 | 目标接口请求数 = 0
```

| 层 | 实际结果 |
|---|---|
| 单测 | **红**：`test/session.test.ts` → `不传存储时默认用 globalThis.sessionStorage…`（`expected false to be true`）；E2E 层面 4 条红 |
| 真机 E2E | **红 4 / 绿 2**（`只读用户深链` 的断点是 `getByTestId('forbidden')` 不存在 —— 因为落在登录页） |

还原：`git diff --exit-code HEAD -- apps/shell/src/lib/session.ts` **通过**，重建 dist 后复跑 **7 passed**。

> 说明：**M2 的浏览器级回归只能由 E2E 抓住**（jsdom/Node 里没有真 `sessionStorage`，
> 单测只能测「注入的存储能恢复」）。为此我补了一条**不传存储**的单测
> （`session.test.ts` 最后一条，伪造 `globalThis.sessionStorage`），
> 所以 M2 现在**单测 + E2E 双线都会变红**。

---

## 4. 门禁与验证命令（真实数字）

```bash
cd apps/shell && npx svelte-check --tsconfig ./tsconfig.json
# → svelte-check found 0 errors and 0 warnings

pnpm -r check          # exit 0，svelte-check 0 errors / 0 warnings
pnpm -r test           # exit 0，各包合计 141 passed / 0 failed（基线 126，新增 15 条）
#   schema 3 · i18n 3 · hooks 17 · sdk 30 · theme 12 · auth 25 · ui 14 · shell 37
pnpm -r build          # exit 0（shell：132 modules → dist/assets/index-CpyjVosp.js 64.09 kB）
pnpm exec eslint .     # exit 0（0 problem）
pnpm exec prettier --check apps/ e2e/   # exit 0
```

**新增单测**（15 条）：`apps/shell/test/session.test.ts`（8）、`apps/shell/test/api.test.ts`（7）。

> ⚠️ `pnpm exec prettier --check apps/ packages/` **exit 1**，唯一告警是
> `packages/auth/test/can_consistency.test.ts`。这是 **BA 的 `3bf8c3f` 引入的既有问题**：
> `git show HEAD:packages/auth/test/can_consistency.test.ts | npx prettier --check --stdin-filepath …`
> 在 **HEAD 上就 exit 1**，且该文件 `git status` 干净（我未改 `packages/`）。
> 一条 `prettier --write` 即可修，但按本次任务约束「只改 `apps/shell/`」我**没有**动它 ——
> 需要 BA / 父会话决定由谁改。

---

## 5. 其它改动（顺手修，均在同一缺陷类内）

- `App.svelte:10-12`、`Login.svelte:9-11` 的过时注释（QA LOW-1）：改成
  「`.svelte` 脚本块**可以**写 TS（根 `eslint.config.js` 已配 TS parser，D97）」；
- `lib/auth-context.ts:7-11` **同一段错话**（不在 QA 点名范围内，但属同一缺陷）一并对齐；
- Login 的 props 补类型 `LoginProps`（`Login.svelte:29-36`），遵守 D97「props 必须有类型」。

---

## 6. 遇到的问题 / 取舍（不只报好消息）

1. **会话恢复后的登录态没有 `userId`**：`GET /api/rbac/me/permissions` 不返回用户信息，
   `restoreSession()` 只能给出 `{status, authenticated, tenantId, permissions}`。
   守卫与 `<Can>` 只看租户 + 权限点，当前功能不受影响；若后续页面要展示用户名，
   需要新增接口或改 `packages/auth`（**需要另立需求**，本期未做）。
2. **恢复期间有一小段「正在恢复会话…」**（`App.svelte:113`）：为了不先闪一下目标页/首页，
   恢复完成前不渲染页面。代价是多一次权限点请求（一次网络往返），且离线时会先看到告警再落登录页。
3. **`restoreSession` 的失败分级**：401/403 → 清空会话（令牌确实失效）；
   网络层失败（离线）→ **保留**会话，只按未登录渲染（重载还能再试）。
   这条行为有单测（`test/api.test.ts` 最后一条），但**没有**真机断网验证。
4. **单测里的假 fetch 读到的是 `Request` 对象**：SDK 把 URL / 头 / 体都封装进
   `new Request(...)` 再交给 `fetch`，只读 `init.headers` 会全部拿到 `null`（我第一版就踩了）。
   真机 E2E 说明了头部确实在 —— 但这也提醒：**只断言 `init` 的单测可能假绿**。
5. **`sessionStorage` 的 XSS 面**：D99 已接受该取舍（比 `localStorage` 窗口小）；
   token 仍在 JS 可读范围，本轮未加 CSP / HttpOnly 方案（不在范围内）。
6. **E2E 依赖真服务与夹具**：`BASE_E2E_URL=http://127.0.0.1:8098` + `base_test` 三个夹具用户；
   `playwright.config.ts` 的默认 `baseURL` 仍是 8080（未改），跑 REQ-011 的 E2E 需显式传
   `BASE_E2E_URL` 或把服务起在 8080。
7. **工作区里原本就有两处未提交改动**：`feature-meta/dev-report-frontend.md`、
   `feature-meta/qa-report.md`（**不是我改的**）。为避免夹带，我只 `git add` 了本次交付的文件。
8. **QA LOW-3**（`dev-report-frontend.md:367` 把 `GET /api/rbac/roles` 的权限点写成
   `rbac:role:write`）**未修**：它在 `feature-meta/` 且已有未提交改动，留给 BA / 后续整理，
   避免和别人的改动撞车。

---

## 7. 变更文件清单

| 文件 | 类型 |
|---|---|
| `apps/shell/src/lib/session.ts` | 新增（会话持久化 + 降级） |
| `apps/shell/src/lib/api.ts` | 改（租户绑定、会话恢复、刷新只写令牌） |
| `apps/shell/src/App.svelte` | 改（先恢复会话再过守卫、启动期不渲染、过时注释） |
| `apps/shell/src/pages/Login.svelte` | 改（租户输入、空租户拦截、按错误码文案、props 类型） |
| `apps/shell/src/lib/auth-context.ts` | 改（过时注释） |
| `apps/shell/test/session.test.ts` | 新增（8 条） |
| `apps/shell/test/api.test.ts` | 新增（7 条） |
| `e2e/shell-auth.spec.ts` | 新增（7 条 REQ-011 E2E，**入库**） |

---

# 修复报告（追加）—— REQ-012 Pre-merge FAIL：2 HIGH + 1 MED + 4 LOW

- **需求**: REQ-012「schema 驱动的渲染闭环」
- **分支**: `feature/REQ-012`
- **裁定依据**: `BA/demands/REQ-012-schema-driven/qa-fail-ruling.md`（BA，2026-11-19 17:35）、
  `feature-meta/qa-report.md`（QA Pre-merge **FAIL**）、
  `BA/demands/REQ-001-platform-foundation/decisions.md`（**D106 / D108**）
- **关键区分**: 12 条 AC 的**功能行为全部复现通过**；FAIL 来自**测试资产**（假绿 + 用例互相踩数据）
- **日期**: 2026-11-19
- **范围**: 只改测试资产 + 文档/能力索引 + 4 处 LOW 的实现细节；**没有改业务语义**

---

## 1. H-1 自带夹具不授 `sandbox:demo:*` → 6 条 E2E 全 skip、exit 0

两条**都**修（缺一不可）。

| # | 位置 | 改动 |
|---|---|---|
| ① | `scripts/seed-e2e.sh:167-173` | `e2e-role-read` + `sandbox:demo:read`；`e2e-role-write` + `sandbox:demo:read` + `sandbox:demo:write` + **`sandbox:demo:secret`**（只授给写用户：AC-S-084 要的就是两个用户字段不同） |
| ② | `e2e/schema-driven.spec.ts:13-30`（规则注释）、`:133`（`requireMeta`）、`:171`（`expectListReadable`） | **403 从 skip 改成 fail** |

D108 判定表已写进 spec 顶部注释，并机械落在两个 helper 上：

- `requireMeta()`：**只有 404 才 `test.skip`**；其它状态一律 `expect(status).toBe(200)`；
- `expectListReadable()`：`expect(status).toBe(200)`，失败信息点名「403 = 夹具缺 `sandbox:demo:*` 授权，先跑 `scripts/seed-e2e.sh`」；
- 顺带把「元数据未声明 `api.create/update/delete`」「夹具用户之间没有字段级权限差异」这些**原本的 skip** 也改成 fail —— 它们同样是配置/实现缺陷，不是「系统没起」。

## 2. H-2 空态用例 vs 零代码用例数据前置互斥

原则：**每条用例自己管数据前置**，任意顺序 / 任意初始状态都独立通过。

| 用例（行号） | 前置 | 收尾 |
|---|---|---|
| 表头顺序（`:250`） | 自己造 **3 条** `demoItem`（`hdr-<ts>-N`） | `finally` 删除自己造的 id |
| 空态（`:273`） | `clearEntity()`（`:224`）**先把 `demoNote` 删空**，并断言删完确实为 0 | 无需恢复（该实体是测试示例数据） |
| 字段级权限（`:295`） | 写用户自己造 1 条 `demoItem` | `finally` 用写用户登录后删除 |
| 零代码（`:336`） | **两个实体各造 1 条**（空表不渲染 `thead`，不能靠残留） | `finally` 删除 |
| CRUD（`:366`） | 用 UI 新建自己的记录（`e2e-<ts>`） | `finally` 按 marker 从列表兜底找回并删除 |
| 游标分页（`:412`） | 自己播 **21 条**（一页 20），不再「看首页有没有 nextCursor 再决定」 | 删除 |

## 3. M-1 `AGENTS.md` 缺「装配层例外」，与 `deps.rs` 矛盾

`AGENTS.md:85-95`：把「一旦反向依赖业务模块即失败」改为
「…即失败（**装配层 `platform-server` 除外**）」，并补一张表：

| 谁 | 能否依赖业务模块 |
|---|---|
| `platform-core` / `platform-db` / `platform-iam` / `platform-org` / `platform-rbac` / `platform-schema`（及 `platform-config`、`platform-contracts*`） | ❌ 不能 |
| `platform-server` | ✅ 可以（唯一例外，`crates/xtask/src/deps.rs:64` 的 `ASSEMBLY_CRATES` 白名单） |

并写明「加进白名单等于放弃这条硬规则，只有可执行交付物有资格」。

## 4. LOW

| # | 位置 | 改动 |
|---|---|---|
| **L-1** | `crates/xtask/src/catalog.rs:99-101`（唯一来源）→ `AGENTS.md:215` → `capabilities/index.json:26` | `ui_table_component` 说明改为「`@base/ui` 的 Table 是页码分页（依赖 total）的只读表格；游标分页没有 total、且空态不能渲染表头空壳，请用 `apps/shell` 的 `SchemaTable`，勿自造表格渲染」。`cargo xtask capabilities` 重生成，`--check` 通过 |
| **L-2** | `apps/shell/src/lib/schema-api.ts:88-130` | 新增模块级 `updateMethods: Map<路径模板, 'PATCH'\|'PUT'>`：首次 405 之后固定 PUT，不再每次编辑先吃一个必定 405 的 PATCH。`resetUpdateMethodCache()` 仅供测试；`test/schema-api.test.ts` 加 `beforeEach` + 2 条用例（405 后下次只发 PUT；PATCH 成功也记住） |
| **L-4** | `apps/shell/src/components/SchemaTable.svelte:110-118` | `<thead><tr>` 里的 `<td class="base-schema-actions">` → `<th scope="col" class="base-schema-actions">`；`test/schema-table.test.ts` 的表头 helper 改为按 `th:not(.base-schema-actions)` 取值，并新增 1 条回归断言（thead 里不得出现 `<td`）。E2E 的 `headerTexts()` 同步用 `th:not(.base-schema-actions)` |
| **L-5** | `apps/shell/src/lib/schema-submit.ts`（新增）+ `SchemaForm.svelte:131` + `apps/shell/test/schema-form-submit.test.ts`（新增 3 条） | 把「校验失败不发请求」的闸门从组件内联逻辑抽成纯函数 `gateSubmit()`（组件 `handleSubmit` 调它），测试用**假 fetch 计数**断言：必填留空 / number 越界 → `fetchCalls === 0`；合法 → 恰好 1 次且 body 只含声明的可写字段 |

**L-3 / L-6** 按裁定不由本轮修：L-3 登记技术债（属 REQ-011 `<Can>` 范畴）；L-6 由 BA 修 AC（`acceptance.md` 已按 rev 3 改好）。

---

## 5. 验证（全部实际执行）

| 命令 | 结果 |
|---|---|
| `./scripts/test-integration.sh` | exit 0（容器数量 2 → 2，未新建容器） |
| `./scripts/seed-e2e.sh` | ✓ 夹具就绪，read=2 个权限点、write=**5** 个权限点 |
| `pnpm -r check` | svelte-check **0 errors 0 warnings**；其余包 tsc 全通过 |
| `pnpm -r build` | 全部成功（`apps/shell/dist` 已重建，E2E 用它） |
| `pnpm -r test` | `apps/shell` **96 passed (12 files)**；其余包全绿 |
| `pnpm exec eslint .` | 无输出（通过） |
| `npx prettier --check .` | All matched files use Prettier code style |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过（无警告） |
| `cargo xtask capabilities --check` | 「能力索引校验通过：14 个文件与源码一致」 |
| `cargo test -p xtask --all-targets` | **51 passed; 0 failed** |

### E2E（本轮核心）

服务：`SERVER_PORT=8091`、`BASE_STATIC_DIR=apps/shell/dist`、`DATABASE_URL=…base_test`（`base_app`）、
PKCS#8 EC 私钥 + base64url secret；`/health/live` 返回 200。

| 运行 | 结果 |
|---|---|
| 正序 `npx playwright test e2e/` | **17 passed，0 failed，0 skipped**（25.9s） |
| 反序（`--shuffle` 在 Playwright 1.58 不存在 → 逐条按**反序**单独跑 6 条 schema 用例） | 6/6 全绿：`游标分页 → CRUD → 零代码 → 字段级权限 → 空态 → 表头`，每条 `1 passed`、0 skipped |

> 「`skipped` 必须为 0」已满足：17 条用例一条没跳。

---

## 6. 变异验证（两条，均逐字节还原）

| 变异 | 观察 | 还原 |
|---|---|---|
| 删掉 `seed-e2e.sh` 里 `e2e-role-write` 的 `sandbox:demo:read` 并重新播种 | E2E **5 failed / 1 passed / 0 skipped**，失败信息即「demoItem 列表返回 403：… 不是环境前置（D108）」；单独跑 `-g "CRUD"` **exit 1**（证明 CI 会红） | `cp` 回备份，`sha256 = 8f64dd6f…` 与变异前一致；重新播种后 write 恢复 5 个权限点 |
| 让「零代码」用例依赖「表非空」（去掉它自己创建 `demoNote`） | 反序单独跑 → **1 failed / exit 1**（`thead` 等不到），复现了 QA 说的「用例互相踩数据」 | `cp` 回备份，`sha256 = 0f9d3b29…` 与变异前一致 |
| （附）把 `validateForm` 变异成恒返回 `{}` | 新增的 `test/schema-form-submit.test.ts` **2 failed / 1 passed** —— 修复前同一变异下 `apps/shell` 是 10 条全绿（L-5 假绿已消除） | `sha256` 与备份一致 |

---

## 7. 变更文件清单

| 文件 | 类型 |
|---|---|
| `e2e/schema-driven.spec.ts` | 改（D108 skip→fail 判定 + 每条用例自管数据前置 + 表头选择器排除操作列） |
| `e2e/shell-auth.spec.ts` | 改（夹具权限注释与 `seed-e2e.sh` 对齐） |
| `scripts/seed-e2e.sh` | 改（`sandbox:demo:read/write/secret` 授权 + 注释表） |
| `AGENTS.md` | 改（装配层例外表 + `ui_table_component` 行） |
| `crates/xtask/src/catalog.rs` | 改（`ui_table_component` 唯一来源） |
| `capabilities/index.json` | 重生成 |
| `apps/shell/src/lib/schema-api.ts` | 改（更新方法缓存，L-2） |
| `apps/shell/src/lib/schema-submit.ts` | **新增**（提交闸门，L-5） |
| `apps/shell/src/components/SchemaForm.svelte` | 改（`handleSubmit` 走 `gateSubmit`） |
| `apps/shell/src/components/SchemaTable.svelte` | 改（表头操作列 `<th scope="col">`，L-4） |
| `apps/shell/test/schema-api.test.ts` | 改（缓存用例 2 条 + `beforeEach` 重置） |
| `apps/shell/test/schema-table.test.ts` | 改（表头 helper + 1 条 th/td 回归断言） |
| `apps/shell/test/schema-form-submit.test.ts` | **新增**（3 条「校验失败不发请求」） |

## 8. 假设与风险

1. **`--shuffle` 不可用**：Playwright 1.58 的 CLI 没有这个开关（`--help` 实查）。反序用「逐条按反序单独执行」，
   等价于证明「任意顺序 + 任意初始状态都能独立通过」，但**不是**一次进程内的乱序执行。
   若要真正的乱序，需要升 Playwright 或写一个自定义 sequencer —— 不在本轮范围。
2. **空态用例会删光 `demoNote` 的全部记录**（只限 `tenant-a` 的沙箱示例实体）。这是「自己管前置」的代价：
   该实体上没有需要保留的数据。若将来有人往 `demoNote` 里放长期夹具，需要改成一个专用的空态实体。
3. **`updateMethods` 是模块级缓存**：页面进程内有效；刷新页面后重新探测一次 PATCH（可接受）。
   服务端若在某次部署中把方法从 PUT 改回 PATCH，前端要刷新页面才会重新发现 —— 元数据里没有声明方法，
   这是「没有更好信息可用」下的取舍（已在文件注释里写明）。
4. **`sandbox:demo:secret` 授给了 E2E 写用户**：这是为了让 AC-S-084 的「字段差异」可取证；
   它是测试夹具，不影响生产租户。
5. **`u-perm-read` 现在多了 `sandbox:demo:read`**：REQ-011 的 `/admin/roles` 403 断言只依赖
   `rbac:role:write` 的缺失，已复跑通过（`shell-auth.spec.ts` 7 条全绿）。
