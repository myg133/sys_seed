# REQ-016「应用接入与应用注册表」—— 前端开发报告（棒 2）

- **需求编号**: REQ-016
- **角色**: 棒 2（前端 Dev），与棒 1（后端）并行
- **worktree**: `feature-REQ-016`（`feature/REQ-016` @ `5aba742`，基线 `develop` @ `aa5b305`）
- **规格版本**: `acceptance.md` **rev 4** / `decisions.md` rev 2 / `design-summary.md` rev 2
- **日期**: 2026-11-21
- **报告口径**: 按 `dev-guide §9.15 / §9.15.1` —— 每句「已覆盖」都指到 `file:line` + **函数名**，
  且经过**变异验证**（改坏生产代码 → 判据变红）。

---

## 0. 结论速览

| 项 | 结果 |
|---|---|
| 前端单测（shell） | **345 passed / 345**（26 文件；基线 279 → 新增 **66**） |
| 前端单测（i18n） | **45 passed / 45** |
| 前端单测（sdk，回归） | **36 passed / 2 skipped**（未改动，确认没弄坏） |
| `svelte-check` | **0 errors, 0 warnings** |
| `@base/i18n check`（`tsc --noEmit`） | **通过** |
| `@base/shell build`（vite） | **成功** |
| `router.ts` 零改动 | ✅ `git diff --stat aa5b305..HEAD -- apps/shell/src/lib/router.ts` **输出为空**；工作区 diff 也为空 |
| 变异验证 | **13 条，全部打红，全部还原干净**（还原后 345/345 复绿） |
| AC 覆盖 | AC-S-160 / 161（展示面）/ 162（前端面）/ 163（三条判据全达标）+ AC-S-156 的前端面 |

---

## 1. 交付清单（文件 → AC）

| 文件 | 动作 | 承担的 AC / 判据 |
|---|---|---|
| `apps/shell/src/lib/app-api.ts` | **新建**（26501 B） | AC-S-156 字段/白名单面、AC-S-160 排序、AC-S-161 记录面、AC-S-162 导航项类型与点击处置 |
| `apps/shell/src/lib/nav.ts` | **改**：新增 `routesFromNavigation`（**保留 `NAV_ITEMS`**） | **AC-S-163 步骤 1/2**（函数级） |
| `apps/shell/src/lib/routes.ts` | **改**：加 `/workbench` + `/admin/apps` 两条路由 + `loadAppsFirstPage` + 第 5 个可选参数 `onApps` | AC-S-160（页面可达）、AC-S-156（`app:registry:read` 守卫） |
| `apps/shell/src/App.svelte` | **改**：2 个渲染分支 + 登录后/SSO 后/会话恢复后取导航 + 注册表首屏回调 | AC-S-163 步骤 3（入口可达）、AC-S-162（导航下发消费） |
| `apps/shell/src/pages/Workbench.svelte` | **新建** | AC-S-160（分组/排序/卡片）、AC-S-161（最近/常用展示）、**AC-S-163 步骤 3** |
| `apps/shell/src/pages/AppRegistry.svelte` | **新建** | AC-S-156 前端面（列表/注册/修改/删除/清空入口）、权限点按钮 |
| `apps/shell/src/lib/router.ts` | ⚠️ **未改**（一行未动） | **AC-S-163 步骤 4** |
| `packages/i18n/src/locales/{zh-CN,en-US}.json` | **改**：新增 `app.*` 25 条 + `nav.appRegistry` | 界面文案（**未加任何 `error.APP_*`** —— 归棒 1） |
| `packages/i18n/test/i18n.test.ts` | **改**：domain 白名单加 `app`（与 REQ-022 加 `oauth` 同款） | 让 `app.*` 通过既有键体系检查 |
| `apps/shell/test/app-api.test.ts` | **新建**（33 tests） | 纯函数判据 |
| `apps/shell/test/app-navigation.test.ts` | **新建**（19 tests） | **AC-S-163 三条判据** + 工作台渲染 |
| `apps/shell/test/app-registry.test.ts` | **新建**（13 tests） | 端点方法/路径、路由守卫、`<Can>` 区分力 |
| `apps/shell/test/fixtures/AppRegistryWithAuth.svelte` | **新建**（测试夹具） | 让 `<Can>` 能对照「只读 vs 管理员」 |

**改动行数**：`git status` 显示只有 6 个已改 + 7 个新增，**全部在 `apps/shell/**` 与
`packages/i18n/**` 内**（见 §9）。

---

## 2. 前端测试数字（自己跑的）

```
$ pnpm --filter @base/shell test
 Test Files  26 passed (26)
      Tests  345 passed (345)

$ pnpm --filter @base/i18n test
 Test Files  1 passed (1)
      Tests  45 passed (45)

$ pnpm --filter @base/sdk test          # 回归（未改动 sdk）
 Test Files  2 passed (2)
      Tests  36 passed | 2 skipped (38)
```

基线对照（开工前在**同一 worktree** 跑的）：
- `@base/shell`：23 files / **279 tests** passed
- `@base/i18n`：1 file / **45 tests** passed

→ 增量：**+3 个测试文件、+65 条用例**，且**既有 279 条一条未红**（改 `App.svelte` /
`routes.ts` / `nav.ts` 会影响 REQ-011~022，已跑全量兜住）。

---

## 3. `svelte-check` 结果

```
$ pnpm --filter @base/shell check
$ svelte-check --tsconfig ./tsconfig.json
svelte-check found 0 errors and 0 warnings
```

`@base/i18n check`（`tsc --noEmit -p tsconfig.json`）无输出 → 通过。

⚠️ 开工初期**曾有 1 个 error**：`apps/shell/test/app-api.test.ts` 导入了 `NavigationItem`
却未使用（`noUnusedLocals`）。已删掉该导入（那份类型由 `app-navigation.test.ts` 的
`moduleItem` / `appItem` 用）。**当前 0 error 是修完后的状态**，不是一直如此。

---

## 4. 变异验证证据（11 条，逐条 `file:line` + 函数名）

做法：先备份 → 改坏生产代码 → 跑 `pnpm --filter @base/shell test` → 记录红了几条 →
`cp` 还原 → 复跑确认 345/345。**全部变异均已还原**（§4.12 给了还原后的复验输出）。

| # | 变异内容 | 变异点（file:line · 函数名） | 打红的判据（file:line · 测试名） | 红数 |
|---|---|---|---|---|
| 1 | `routesFromNavigation` **不再过滤 `external`** | `apps/shell/src/lib/nav.ts:76-78` · `routesFromNavigation` | `test/app-navigation.test.ts:113`（恰好 2 条）、`:121`（只传 external → `[]`）、`:143`（路由表无 http 条目） | **3** |
| 2 | `sortApps` 去掉 **`displayName` 次级排序** | `apps/shell/src/lib/app-api.ts:458-465` · `sortApps` | `test/app-api.test.ts:215`、`test/app-navigation.test.ts:349/354`（渲染顺序） | **2** |
| 3 | `navigationClickTarget` 把 `external` 也当**站内路径** | `apps/shell/src/lib/app-api.ts:560-565` · `navigationClickTarget` | `test/app-api.test.ts:278`、`:303`（两类形态互斥） | **2** |
| 4 | `appOpenAction` 在 `launch_url` 为空时**仍给 link** | `apps/shell/src/lib/app-api.ts:526-529` · `appOpenAction` | `test/app-api.test.ts:332`、`:341`（scheme 非法） | **2** |
| 5 | `safeLaunchUrl` **不校验 scheme**（放行 `javascript:`） | `apps/shell/src/lib/app-api.ts:495-511` · `safeLaunchUrl` | `test/app-api.test.ts:256`、`:268`、`:341`；`test/app-navigation.test.ts:389/410`（渲染层） | **4** |
| 6 | `toNavigationItem` **采信响应里的 `kind`** | `apps/shell/src/lib/app-api.ts:377-415` · `toNavigationItem` | `test/app-api.test.ts:183/192`（module 声称 external 仍规整成 internal） | **1** |
| 7 | `buildCreateBody` **不再过滤空串** | `apps/shell/src/lib/app-api.ts:598-612` · `buildCreateBody` | `test/app-api.test.ts:342`（空串不发） | **1** |
| 8 | 注册表路由**去掉 `permission`** | `apps/shell/src/lib/routes.ts:201-203` · `createRoutes` | `test/app-navigation.test.ts:281`；`test/app-registry.test.ts:166`、`:179`、`:193`（403/登录页） | **4** |
| 9 | 注册表 `load` 引入**短路坑**（`onApps?.(await …)`） | `apps/shell/src/lib/routes.ts:205-208` · `createRoutes` 的 `load` | `test/app-registry.test.ts:207`（有读权限 → 请求 `GET /api/apps`） | **1** |
| 10 | 注册表单的 `<Can>` 用**读权限点** | `apps/shell/src/pages/AppRegistry.svelte:300` · 模板 | `test/app-registry.test.ts:222`（只读账号 → 表单不进 DOM） | **1** |
| 11 | 导航外部项的 `href` **不过 scheme 白名单** | `apps/shell/src/pages/Workbench.svelte:192-194` · `externalHref` | `test/app-navigation.test.ts:223/239`（`href` 里绝不出现 `javascript:`） | **1** |
| 12 | `sortWorkspace` **不再纠正顺序**（原样返回后端顺序） | `apps/shell/src/lib/app-api.ts:476-481` · `sortWorkspace` | `test/app-api.test.ts:264`（新用例）、`:233`、`test/app-navigation.test.ts:312` | **3** |
| 13 | `sortWorkspace` **原地排序**（改掉入参） | `apps/shell/src/lib/app-api.ts:476-481` · `sortWorkspace` | `test/app-api.test.ts:264`（**仅**新用例的不可变性断言红） | **1** |

### 4.1 变异 1（AC-S-163 步骤 2 —— 你点名要求的那条）

```ts
// 变异后 nav.ts:76
return items.map((item) => ({ path: item.target, title: item.label }));  // 不再 .filter(kind==='internal')
```

输出：
```
 ❯ test/app-navigation.test.ts (19 tests | 3 failed)
 FAIL … > 只传 1 个 external 项 → 空数组（这条是关键：…）
      Tests  3 failed | 341 passed (344)
```

→ **「`external` 也进 SPA 路由表」这条变异必然变红**，判据在
`apps/shell/test/app-navigation.test.ts:120-121`（`expect(...).toEqual([])`）。

### 4.2 变异 2 的一次**自我纠错**（重要的方法记录）

第一次跑变异 2 时**只红了 1 条**（`app-api.test.ts:215` 的纯函数用例），
渲染层用例（`app-navigation.test.ts` 的分组顺序）**没红**。

原因：我给 general 组的输入顺序恰好是 `C, D`，与期望输出相同 →
**稳定排序会原样保留输入顺序**，所以「去掉次级排序」看不出区别 ——
**这条渲染判据当时是假的**（零区分力）。

处置：把输入改成 `D, C`（与期望输出相反），并在源码里写清为什么
（`apps/shell/test/app-navigation.test.ts:326-330` 的注释）。重跑变异 2 → **红 2 条**。

> 这正是 dev-guide §9.15 说的「看起来有」：**输入顺序与期望一致时，
> 排序判据等于没写**。若不跑变异，这条会以「已覆盖」进报告。

### 4.3 还原后的复验（干净）

```
$ pnpm --filter @base/shell test
 Test Files  26 passed (26)
      Tests  345 passed (345)
$ pnpm --filter @base/shell check
svelte-check found 0 errors and 0 warnings
$ pnpm --filter @base/i18n test
      Tests  45 passed (45)
```

### 4.4 D286 裁决后补的判据：`sortWorkspace` 纠正 + 不可变性

**背景**：D286 把 AC-S-160 步骤 3 的**真判据**钉在 `GET /api/apps/workspace` 的
**原始 JSON** 上（归棒 1），同时**保留**前端 `sortWorkspace` 的防御性重排。
→ 于是前端这条测试断的**不是**「后端排对了」，而是「**即使后端错序，前端也纠正**」。

**新用例**：`test/app-api.test.ts:264` · 测试名
`后端已错序 → 前端纠正（组 general 在前、组内 D→C→B→A），且不修改入参`

四条断言（每条都在 `:264-301` 的同一个 `it` 体内）：

| 断言 | 行 | 断什么 |
|---|---|---|
| ① 组顺序 | `:282` | `sorted.map(category)` == `['finance','general']`（输入是 `general` 在前） |
| ② 组内顺序 | `:284-285` | finance 按 `sortOrder`（`B`(10) → `A`(20)）；general 并列 0 → 按 `displayName`（`A→B→C→D`，输入是 `D→C→B→A`） |
| ③ **不可变性** | `:287-294` | 入参的组顺序 + 组内顺序**一字未动**（快照前后 `toEqual`） |
| ④ 返回新对象 | `:297-300` | `sorted !== input`、`sorted[0] !== input[1]`、`sorted[0].apps !== input[1].apps` |

**变异证据**：

| 变异 | 打红 |
|---|---|
| **M12** `sortWorkspace` 不纠正（原样返回） | **3 条**（新用例 + `:233` 组排序 + 渲染层 `app-navigation.test.ts:312`） |
| **M13** `sortWorkspace` **原地排序**（`groups.sort(...)`，改掉入参） | **1 条 —— 只有新用例的不可变性断言** |

M13 的价值：它证明「不可变性」这条判据是**独立承重**的 —— 把 `[...groups]` 换成原地排序，
排序**结果**仍然正确（②③ 之外的断言全绿），**只有**「入参未被修改」这一条红。
即：没有这条断言，`[...groups]` 可以被悄悄删掉而不被发现。

**输入为什么必须「反着给」**：见 §4.2 —— 输入顺序等于期望顺序时，稳定排序会原样保留，
判据**零区分力**。本用例刻意给 `general` 在前、组内 `D→C→B→A`，并在源码注释
（`test/app-api.test.ts:249-263`）里写清了这条理由。

---

## 5. 开工前必答项：**我怎么确认 `external` 项没进 SPA 路由？**

### 5.1 函数级（判据本体）

`apps/shell/test/app-navigation.test.ts:120-122` · 测试 `routesFromNavigation —— 函数级`：

```ts
it('只传 1 个 external 项 → 空数组（…会让 match() 静默失败）', () => {
  expect(routesFromNavigation([appItem('https://x.example/', 'X')])).toEqual([]);
});
```

被断言的实现：`apps/shell/src/lib/nav.ts:76-78` · `routesFromNavigation`
（`.filter((item) => item.kind === 'internal')`）。
变异证据：§4 变异 1（红 3 条）。

### 5.2 结构级（路由表里没有 http 条目）

`apps/shell/test/app-navigation.test.ts:138-144` · 同一 describe：

```ts
const merged = [...createRoutes(client), ...routesFromNavigation([appItem('https://x.example/', 'X')])];
expect(merged.some((route) => route.path.startsWith('http'))).toBe(false);
```

→ 即使有人把导航产物拼进路由表，**也拼不出一个 `http…` 路径**。

### 5.3 运行时级（路由器拿到外部 URL 会怎样）

`apps/shell/test/app-navigation.test.ts:183-221` · 测试
`整页跳转**不经过** SPA 路由`：把 `https://x.example/` 交给**真实** `createRouter`
（`apps/shell/src/lib/router.ts` 的 `resolve`）→

```ts
expect(resolution.route).toBeUndefined();   // 匹配不到路由
expect(resolution.allowed).toBe(true);      // 默认放行（无路由 = 不拦）
expect(written).toEqual([]);                // 没有一次 pushState
```

→ 外部 URL 既不命中路由、也不写历史 —— 唯一的外部跳转通道是 `env.assign`
（`apps/shell/src/lib/app-api.ts:573-583` · `browserAppEnv` 的 `assign`，内部调
`window.location.assign`，**整页导航**）。

### 5.4 入口级（页面确实渲染成整页跳）

`apps/shell/test/app-navigation.test.ts:152-166`：`external` 项的 `href` **逐字等于**
target，且带 `data-kind="external"`；`apps/shell/src/pages/Workbench.svelte:279-291`
的真实模板里 `onclick` 走 `handleNavigationClick` → `env.assign`（**不是** `router.navigate`）。

### 5.5 你可以自己复跑的命令

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-016
pnpm --filter @base/shell test -- app-navigation      # AC-S-163 三条判据
git diff --stat aa5b305..HEAD -- apps/shell/src/lib/router.ts   # 必须无输出
```

---

## 6. AC 覆盖情况（只写**测试真的断什么**）

> 表格里的每一条都指到 `file:line` + 函数名/测试名。若某条只是「理想结果」而非断言，
> 我在下面明确写「**未覆盖**」。

| AC / 步骤 | 前端这一侧实际断言什么 | 落点（file:line · 函数/测试名） | 变异 |
|---|---|---|---|
| **AC-S-163 步骤 1** | 2 个 `internal` + 1 个 `external` → `routes` 恰好 2 条；`path` 逐字 `/a`/`/b`，`title` 逐字 `A`/`B` | `test/app-navigation.test.ts:107-118` · `routesFromNavigation —— 函数级` | 1 |
| **AC-S-163 步骤 1（label 不是键）** | `title` 是 `审计日志`，且不匹配 `/^[a-z]+\.[a-z]/`（不是 `nav.audit`） | `test/app-navigation.test.ts:124-129` | — |
| **AC-S-163 步骤 2** | 只传 1 个 `external` → **`[]`** | `test/app-navigation.test.ts:120-121` | **1** |
| **AC-S-163 步骤 3（入口级）** | ① `external` 项 `href` 逐字等于 target；② `internal` 项 `href` 以 `/` 开头；③ 外部 URL 交给真路由器 → `route === undefined` 且 `pushState` 0 次；④ `javascript:` 不进 `href` | `:152-166` / `:168-181` / `:183-221` / `:223-243` | 3、5、11 |
| **AC-S-163 步骤 4（回归）** | `router.ts` 无 `addRoute`/`setRoutes`、不 import `nav`/`app-api`、`createRouter` 仍捕获 `routes` 引用 | `test/app-navigation.test.ts:257-268` | — |
| **AC-S-163 步骤 4（命令级）** | `git diff --stat aa5b305..HEAD -- …/router.ts` **输出为空**；工作区 diff 也为空 | 见 §0 / §5.5（实跑输出） | — |
| **AC-S-160（前端展示面）** | 分组按 `category` 码点升序、组内按 `(sortOrder, displayName)`；**输入乱序时也正确** | `test/app-navigation.test.ts:312-354` · 工作台渲染 describe | **2** |
| **AC-S-160 排序纯函数** | finance 组 B(10)→A(20)；general 组 C→D（并列按 displayName） | `test/app-api.test.ts:215-224` · `确定性排序（D267）` | **2** |
| **AC-S-160 步骤 3（前端纠错面，D286）** | 后端错序输入（general 在前、组内 D→C→B→A）→ 输出 finance 前、组内 A→B→C→D；**入参未被修改**；返回**新对象** | `test/app-api.test.ts:264-301` · `后端已错序 → 前端纠正…且不修改入参` | **12、13** |
| **AC-S-160 步骤「不按 launch_url 过滤」（D279）** | **没有入口的应用卡片仍渲染**；只有有入口的那张有 `app-workspace-open`，另一张给「未配置入口」 | `test/app-navigation.test.ts:357-386` | 4 |
| **AC-S-161（前端展示面）** | 最近/常用各自渲染；frequent 空 → 提示文案 | `test/app-navigation.test.ts:414-432` | — |
| **AC-S-161（UPSERT 接口契约）** | `recordAppAccess` → `POST /api/apps/{id}/access`；解析 `{ status, accessCount }` | `test/app-registry.test.ts:53-55`（路径形状）+ `app-api.ts:431`·`parseAccessResult` | — |
| **AC-S-162 字段形状（D284）** | `module` 项 `Object.keys` **恰好** 6 个（`icon`/`kind`/`label`/`requiredPermission`/`source`/`target`）；`app` 项 `requiredPermission` 恒 `null` | `test/app-api.test.ts:141-180` | 6 |
| **AC-S-162 步骤 6（前端对应面）** | `launch_url` 为空 → `appOpenAction` 返回 `{kind:'none'}` → **不渲染可点入口** | `test/app-api.test.ts:332-338` | **4** |
| **AC-S-162 kind 不得混用** | `kind` 由**来源**推出（采信响应会被纠正） | `test/app-api.test.ts:183-192` | **6** |
| **AC-S-156 前端面（字段/白名单）** | 可写字段白名单恰好 7 个，**不含** `appId`/`status`/`scopes`/`createdAt`/`createdBy` | `test/app-api.test.ts:73-85` | 7 |
| **AC-S-156 前端面（错误码白名单）** | 6 个码逐字；**断言不含** `APP_REGISTRY_MISMATCH`/`APP_INVALID_SCOPE_PROJECTION`（死码） | `test/app-api.test.ts:58-71` | — |
| **AC-S-156 前端面（清空入口 = 专用端点）** | `clearAppLaunchUrl` → **`DELETE`** `/api/apps/{id}/launch-url`，**无请求体** | `test/app-registry.test.ts:150-158` | — |
| **AC-S-156 前端面（路由守卫）** | 无 `app:registry:read` → `/403`；未登录 → `/login`；有权限 → 请求 `GET /api/apps` | `test/app-registry.test.ts:162-210` | **8、9** |
| **AC-S-156 前端面（按钮权限）** | 只读账号 → 注册表单**不进 DOM**；管理员 → 进 DOM（对照） | `test/app-registry.test.ts:222-233` | **10** |
| **AC-S-156 步骤 9（安全，前端侧）** | `javascript:` / `data:` / `JavaScript:` / `' javascript:'` / `ftp:` / `not-a-url` 全部 `undefined` | `test/app-api.test.ts:256-275` | **5** |

### 6.1 我**没有**覆盖的（明确列出，不假装）

| 项 | 为什么 |
|---|---|
| **点击链本身**（真的点一下 → 真的换页） | 无 DOM 测试环境（`jsdom`/`happy-dom` 不在 pnpm store）→ 组件只测**渲染**。点击**决策**已抽成纯函数 `navigationClickTarget`（`:560`）并单测；**点击链由终版 E2E 兜底**（`e2e/apps.spec.ts`，BA 统一跑） |
| **AC-S-162 的排序**（module 在前 / app 在后、组内按 label） | 这是**后端**响应体的判据（棒 1 的集成测试）。前端**不重排导航**（重排会掩盖后端错序） |
| **AC-S-160 的「确定性排序」逐字节判据（后端侧）** | 后端响应体判据，归棒 1（D286）。前端只承担「即使后端错序也纠正 + 不改入参」这一条（`test/app-api.test.ts:264`，见 §4.4） |
| **AC-S-156/157/158/159/164/165、AC-S-024** | 后端 / 集成 / 安全面，全部归棒 1 |
| **E2E** | 按任务书：**不跑**（终版 BA 统一跑） |

---

## 7. 与规格不一致的发现（需要 BA 知悉）

### 7.1 ✅ 前端 `sortWorkspace` 会掩盖后端错序 —— **已裁定（D286）：保留重排，真判据归后端**

`apps/shell/src/lib/app-api.ts:476-481` · `sortWorkspace` 在渲染前**又排了一次**
（组按 category、组内按 `sortOrder`/`displayName`）。代价：

> 后端若返回**乱序**的 `groups`，前端照样排好 → **E2E 完全看不见**。

**D286 的处置（BA 已裁定，我据此补了判据）**：

1. **保留**前端重排（防御性设计：不信任后端顺序）；
2. AC-S-160 步骤 3 的**真判据**钉在 `GET /api/apps/workspace` 的**原始 JSON** 上
   （测试侧**不经任何前端函数**）→ 归**棒 1**；
3. 前端补一条判据，断的是「**即使后端错序，前端也纠正**」+ **不修改入参**
   → `test/app-api.test.ts:264`（见 §4.4；变异 M12/M13 证明有区分力）。

→ 源码注释里已写明这个分工（`app-api.ts:465-475`，在 `sortWorkspace` 上方）。

### 7.1b ✅ `POST /api/apps/{id}/access` 的 `launch_url` 条件 —— **已裁定（D289）：按 D279（不要求）**

我按 D279 实现（`access` 不要求 `launch_url` 非空）；`design-summary.md:203` 的残留已由 BA 删除。
另：`Home.svelte` 的占位菜单并存 → **D287 记 TD-052，本轮不统一**；
`createRoutes` 加第 5 个可选参数 → **D288 接受**。以上三处**均无需改代码**。

### 7.2 `nav.ts` 的 `NAV_ITEMS` 与「导航由注册表下发」并存

规格说「导航的正源是后端下发」。我**保留**了 `NAV_ITEMS`（任务书要求保留，且
`nav.test.ts` 依赖它），但它现在**只服务既有首页**（`Home.svelte` 的菜单过滤），
**不参与**工作台的导航渲染（工作台用 `props.navigation`）。

→ 结果：**首页菜单仍是占位数据**，工作台导航才是下发的真实数据。
这是「两套菜单并存」的状态，**没有**在本需求里统一（统一要改 `Home.svelte` 的语义，
会影响 REQ-010 的既有 E2E `a[href="/workbench"]`）。**记 TD？请 BA 定**。

### 7.3 `/` 与 `/workbench` 共用翻译键 `nav.workbench`

`routes.ts:154`（`/`）与 `routes.ts:193`（`/workbench`）的 `title` 都是 `'nav.workbench'`。
这是**有意**的（两个页面都叫「工作台」），但页面上 `Home.svelte` 与 `Workbench.svelte`
是**不同内容** → 同一个标题可能让用户困惑。**未改既有首页**（避免破坏 REQ-010 的 E2E）。

### 7.4 规格 §8 说「`/workbench` 已存在于守卫豁免？需确认」

实测：**不存在**。REQ-016 之前 `/workbench` **只在** `nav.ts` 的占位菜单里出现，
`routes.ts` 里没有这条路由（S-03 的实测结论一致）。本需求**新增**了它
（`routes.ts:83` 的 `WORKBENCH_ROUTE_PATH` + `:190-197` 的路由），声明 `requiresAuth: true`
（登录即可，不声明权限点 —— 与 `design §3` 的「登录即可」一致）。

### 7.5 `createRoutes` 的签名变了（+1 个可选参数）

`apps/shell/src/lib/routes.ts:136-153` 新增第 5 个**可选**参数 `onApps`。
前 4 个参数与返回类型**未变** → 既有 5 处 `createRoutes(client)` 调用全部照旧
（已跑全量 279 条基线用例确认无红）。
但**严格说**这是「改了 REQ-011 的交付物」—— 与 D270 对 `createRoutes` 的处理同款
（D270 原文就是「改 `createRoutes` 的签名，不改其语义」）。**请 BA 确认可接受**。

### 7.6 i18n 的 domain 白名单被改了（测试文件）

`packages/i18n/test/i18n.test.ts:57` 的 `domains` 集合加了 `'app'`。
理由与 REQ-022 加 `'oauth'` 完全同款（那里也改了同一个集合）。
⚠️ 这是**改既有测试的常量**，不是删断言 —— 但会写进 `git diff`，BA 核查时会看到。

---

## 8. 未完成项 + 原因

| 未完成 | 原因 |
|---|---|
| E2E（`e2e/apps.spec.ts`） | 任务书明确「**不跑 E2E**，终版 BA 统一跑」→ 我**没有**新建该文件（它需要种子夹具 + 真库，属棒 1 的 `scripts/seed-e2e.sh`） |
| 「点导航项 → 落到 SPA 内新页面」 | **明确不做**（AC-S-163.b / TD-048）：`RouteDefinition` 无组件槽、`createRouter` 捕获数组、`Router` 无 `addRoute`、`App.svelte` 逐路径硬编码 |
| 工作台的 `module` 导航项点击 → 站内页面 | 同理：`internal` 项渲染成 `<a href>` 是**站内链接**，但那些路径**没有页面组件** → 会落到 `<Home/>` 兜底。这是 AC-S-163.b 的既定边界（不假装做到） |
| `error.APP_*` 的 6 条错误文案 | 按任务书**归棒 1**（我只加了 `app.*` 界面文案）。前端 `describeError` 会自动按码取 `error.<CODE>` → 棒 1 补齐后即生效，前端**无需再改** |

---

## 9. 对棒 1 的接口对齐清单（**我实际调用的每个端点 + 每个字段名**）

### 9.1 端点（方法 · 路径 · 期望形状）

| # | 方法 + 路径 | 代码位置 | 我期望的响应形状 | 我读的字段 |
|---|---|---|---|---|
| 1 | `GET /api/apps` | `app-api.ts:674`·`listApps` | **裸数组** `[AppView]` | `appId, clientId, displayName, description, category, iconUrl, launchUrl, sortOrder, status, scopes, createdAt, createdBy` |
| 2 | `POST /api/apps` | `:716`·`createApp` | `AppView`（**201**） | 同上 |
| 3 | `PATCH /api/apps/{appId}` | `:726`·`updateApp` | `AppView` | 同上 |
| 4 | `DELETE /api/apps/{appId}` | `:740`·`deleteApp` | 我**不看响应体** | — |
| 5 | `GET /api/apps/workspace` | `:680`·`loadWorkspace` | `{ groups: [{ category, apps: [AppView] }] }` | `groups[].category`、`groups[].apps[]` |
| 6 | `GET /api/apps/recent` | `:686`·`loadRecentApps` | **裸数组** `[AppView]` | 同 1 |
| 7 | `GET /api/apps/frequent` | `:692`·`loadFrequentApps` | **裸数组** `[AppView]` | 同 1 |
| 8 | `GET /api/apps/navigation` | `:698`·`loadNavigation` | `{ items: [NavigationItem] }` | 见 §9.2 |
| 9 | `POST /api/apps/{appId}/access` | `:704`·`recordAppAccess` | `{ status, accessCount }` | `status`（文本）、`accessCount`（数） |
| 10 | `DELETE /api/apps/{appId}/launch-url` | `:745`·`clearAppLaunchUrl` | **204**（无体；我**不解析**） | — |

### 9.2 `AppView` 字段（camelCase，我一个不多读）

```ts
{ appId, clientId, displayName, description, category, iconUrl,
  launchUrl, sortOrder, status, scopes, createdAt, createdBy }
```

- `description` / `iconUrl` / `launchUrl` 我按 **`string | null`** 解析（**空串也当 `null`**）。
- `sortOrder` 接受 `number`，也**容忍**数字字符串（`integer()`）。
- `scopes` 我按 `string[]` 解析（只是存着，暂无展示；若棒 1 不发这个键 → 我得到 `[]`）。

### 9.3 `NavigationItem`（**按来源分别解析**，D284）

```ts
// source="module"
{ source: 'module', label, icon: string|null, target, kind: 'internal', requiredPermission: string|null }

// source="app"
{ source: 'app', appId, label, icon: string|null, target, kind: 'external', requiredPermission: null }
```

**三点必须与棒 1 一致**：

1. `label` 是**中文文案**，**不是**翻译键 —— 我直接渲染、**不过 `t()`**（代价 TD-049）；
2. `kind` 我**由 `source` 推出**（`toNavigationItem`，`app-api.ts:377`）：`module`→`internal`、
   `app`→`external`。→ **即使棒 1 把 `kind` 发错，前端也不会把外部 URL 当站内路径**；
3. `app` 项的 `target` 我**逐字**用作 `href` 与 `location.assign` 的入参（不加工、不加前缀）。

### 9.4 请求体（我**只发**这 7 个键，且**只发有值的**）

```
clientId, displayName, description, category, iconUrl, launchUrl, sortOrder
```

- 空串**不发**（用户没填 ≠ 填了空串）；`sortOrder: 0` **会发**（0 是合法值）。
- **清空 `launchUrl` 我一律用 `DELETE …/launch-url`**，从不发 `launchUrl: null`（D283）。

### 9.5 权限点（逐字）

```
app:registry:read     ← /admin/apps 的路由守卫
app:registry:write    ← 注册表单 / 编辑 / 清空入口按钮（<Can>）
app:registry:delete   ← 删除按钮（<Can>）
```

### 9.6 错误码（前端只做**白名单常量**，文案归你）

```
APP_NOT_FOUND / APP_DUPLICATED / APP_CLIENT_INVALID /
APP_LAUNCH_URL_INVALID / APP_ICON_URL_INVALID / APP_INVALID_REQUEST
```

前端渲染失败时走既有 `describeError` → 取 `error.<CODE>` 文案 → **你加进
`packages/i18n/src/locales/*.json` 的 6 条文案直接生效，我不用再改一行**。

⚠️ **两处可能撞车，请确认**：
1. **`packages/i18n/src/locales/*.json` 我们都会改** —— 我只加了 `app.*` 与 `nav.appRegistry`，
   **没有**加任何 `error.APP_*`。你加那 6 条时**只追加、不改我已加的行**即可（json 键不重叠）。
2. **`packages/i18n/test/i18n.test.ts` 我改了一行**（domain 白名单加 `'app'`）。
   若你也要改这个文件的错误码数组（`codes` 里加 6 个 + `expect(...).toHaveLength(codes.length)`），
   **注意别把我的 `'app'` 覆盖掉**。

---

## 10. 对棒 2 文件集之外的改动（核对结果）

`git status --short` 的**我的**部分：

```
 M apps/shell/src/App.svelte
 M apps/shell/src/lib/nav.ts
 M apps/shell/src/lib/routes.ts
 M packages/i18n/src/locales/en-US.json
 M packages/i18n/src/locales/zh-CN.json
 M packages/i18n/test/i18n.test.ts
?? apps/shell/src/lib/app-api.ts
?? apps/shell/src/pages/AppRegistry.svelte
?? apps/shell/src/pages/Workbench.svelte
?? apps/shell/test/app-api.test.ts
?? apps/shell/test/app-navigation.test.ts
?? apps/shell/test/app-registry.test.ts
?? apps/shell/test/fixtures/
```

**两处需要说明（都在 `packages/i18n/**` 内，任务书允许）**：

1. `packages/i18n/test/i18n.test.ts` —— 加 `'app'` 到 domain 白名单（§7.6，理由：不改它
   则新键必然让既有键体系检查变红）；
2. `apps/shell/test/fixtures/AppRegistryWithAuth.svelte` —— 新增**测试夹具**（不是生产代码）。

**完全没碰**：`apps/shell/src/lib/router.ts`（零改动）、`crates/**`、`migrations/**`、
`scripts/**`、`capabilities/**`、`Cargo.toml`、`Cargo.lock`（那些是棒 1 的）。
也**没有**执行任何 `git add` / `commit` / `checkout` / `restore` / `stash` / `reset`

---

## 11. 原待拍板问题 —— **全部已裁定（D286~D289）**

| # | 问题 | 裁定 | 我的动作 |
|---|---|---|---|
| 1 | 前端 `sortWorkspace` 重排掩盖后端错序 | **D286**：保留重排；AC-S-160 步骤 3 真判据落到后端响应体 | 补判据 `test/app-api.test.ts:264`（§4.4）；**不改代码** |
| 2 | `Home.svelte` 占位菜单与真实导航并存 | **D287**：记 **TD-052**，本轮不统一 | **不改代码**（`NAV_ITEMS` 保留作兜底） |
| 3 | `createRoutes` 加第 5 个**可选**参数 | **D288**：接受（D270 只禁改 `router.ts`） | **不改代码**（既有 279 条零红即证据） |
| 4 | `i18n.test.ts` / locale 文件的共同改动面 | 已确认：棒 1 **只追加** `error.APP_*`，不覆盖我的 `app` / `app.*` | **不改代码** |
| 5 | `POST /access` 是否要求 `launch_url` 非空 | **D289**：按 **D279**（不要求）；`design §4.3` 残留已删 | **不改代码**（本就按 D279 实现） |

→ 结论：**D286~D289 全部无需改生产代码**，唯一改动是新增 1 条测试（`test/app-api.test.ts:264`）。
