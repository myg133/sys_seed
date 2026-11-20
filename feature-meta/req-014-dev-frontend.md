# REQ-014 审计查询界面（前端）—— Dev 自验报告

- **需求**: REQ-014「审计日志」
- **分支**: `feature/REQ-014`（基线 `develop@0683115`）｜工作区 `feature-REQ-014/`
- **我负责的 AC**: **AC-S-132**（审计查询界面：列表、分页、过滤、字段展示）
- **规格基线**: `BA/demands/REQ-014-audit/acceptance.md`（**rev 3**）、`decisions.md`（rev 2，D145 / D155 / D162 / D167）、`design-summary.md`（rev 2 §5）、`test-cases/frontend.md`（TC-014-111 ~ 120）
- **契约**: `api-conventions v1`（C4 / C7 / C13）
- **文件边界**: 只碰 `apps/shell/**` 与 `e2e/**`；**另加** `packages/i18n/src/locales/*.json`（审计页文案的唯一落点，见 §5-偏离 1）
- **未 commit**（按铁律 1，交给 BA 统一提交）

> 另一 Dev 正在同一 worktree 改 `crates/**`（`git status` 里 `crates/**`、`migrations/**`、`Cargo.toml`、
> `deploy/seed/02-app-role.sql` 的改动**全部是他的**，不是我的）。我的文件集与他的**不相交**。

---

## 1. 交付清单（文件 → AC-S-132 的哪一步）

| 文件 | 性质 | 对应 AC-S-132 |
|---|---|---|
| `apps/shell/src/lib/audit-api.ts` | **新增** | 请求封装：C7 参数（`cursor`+`limit` / `page`+`page_size` / `sort` / `filter[field][op]`）、响应解析、`before`/`after` 文本解析与降级、展示行 |
| `apps/shell/src/pages/Audit.svelte` | **新增** | 步骤 1（列表 + 五列同时显示）、步骤 2（**服务端**分页）、步骤 3（按操作人 / 动作过滤）、过滤框与页大小控件 |
| `apps/shell/src/lib/routes.ts` | 改 | 步骤 4 / 5：`/admin/audit` 路由 + 权限点 `audit:events:read` + **路由 `load` 取首屏**（守卫不放行 → `load` 不被调用 → 请求数 0） |
| `apps/shell/src/App.svelte` | 改 | 挂载审计页；首屏结果从路由 `load` 交给页面（`initial` prop） |
| `apps/shell/src/lib/i18n.ts` | 改 | `describeError` **搬家**到这里（它本来就是通用的「错误码 → 文案」helper，与 `errorMessageKey` / `toMessageParams` 一套） |
| `apps/shell/src/lib/schema-api.ts` | 改 | 原地 **re-export** `describeError`（既有 import 路径与既有测试不变） |
| `packages/i18n/src/locales/zh-CN.json` / `en-US.json` | 改 | 审计页的**全部**面向用户文案（15 个键） |
| `apps/shell/test/audit-api.test.ts` | **新增** | 30 条单测（请求参数 / 解析 / 降级 / 行） |
| `apps/shell/test/audit-page.test.ts` | **新增** | 7 条单测（五列文案 + 路由 + 守卫 → 403 且请求数 0） |
| `e2e/audit.spec.ts` | **新增** | TC-014-111 ~ 120 的 E2E（403 用例不需后端；数据用例按「未就绪」跳过） |
| `feature-meta/req-014-dev-frontend.md` | **新增** | 本报告 |
| `feature-meta/verification-report.md` | **追加** | AC-S-132 一行（不覆盖别人的内容） |

**diff 规模**：12 个文件（10 个代码 + 2 个报告），其中新增 5 个、修改 7 个。

---

## 2. AC-S-132 五步 → 实现位置 → 证据

| AC 步骤 | 实现 | 证据 |
|---|---|---|
| 步骤 1：列出记录，**同时显示**操作人 / 时间 / 动作 / 对象 / 来源 IP | `Audit.svelte` 的 `columns`（8 列：ID + 5 项必显 + 变更前 / 变更后） | `audit-page.test.ts`「同时给出操作人 / 时间 / 动作 / 对象 / 来源 IP 五列」✓（列标签走 `t()`，**不是**硬编码中文） |
| 步骤 2：**服务端**分页，翻页后记录不重复 | `audit-api.ts` 的 `buildAuditQuery`（页码模式 → `page` + `page_size`）；`Audit.svelte` 的 `handlePageChange` → **每次翻页一次新请求**；`total` 取自服务端（**不是** `rows.length`） | `audit-api.test.ts`「页码分页 → page + page_size」「请求打在 /api/audit/events，页码进 query（不是前端切片）」✓；**没有**照抄 `ProtectedRoles.svelte` 的 `total={roles.length}` + 前端切片 |
| 步骤 3：按操作人与动作过滤生效，结果与后端一致 | `auditFilters()` → `filter[actorId][eq]` / `filter[action][eq]`（**下发到后端**，不是前端过滤已加载的那一页） | `audit-api.test.ts`「过滤 → filter[field][op]」「界面输入 → 过滤条件」✓；E2E「过滤下发到后端，且与后端接口结果一致」（未跑，见 §4） |
| 步骤 4：无权限账号 → 落 **403 页**，且 403 页**不是空白页** | 路由 `/admin/audit` 声明 `permission: 'audit:events:read'` → 守卫不放行 → 地址改写成 `/403` → 渲染既有的 `Forbidden.svelte`（**复用，未另做**） | `audit-page.test.ts`「无权限账号 → 落 403 页，且目标接口的请求计数为 0」✓；E2E「TC-014-118 / TC-014-119」（未跑，见 §4） |
| 步骤 5：无权限时**目标接口请求数 == 0** | 守卫不放行 → **路由的 `load` 不会被调用**（`router.ts:244-250`）**且**页面不被渲染（`App.svelte` 的 `resolution.allowed`）→ **两道**保护 | `audit-page.test.ts`「目标接口的请求计数为 0」✓（假客户端记录 `/api/audit/events` 的调用数 = 0） |

**403 页不是空白页**：复用 `Forbidden.svelte`（`data-testid="forbidden"` + `<h2>403</h2>` + `common.forbiddenMessage`）→ 该条**既有实现已满足**，我没有另做页面（QA 的 TC-014-119 也这么说）。

---

## 3. 独立实现的 QA 用例（TC-014-111 ~ 120）与判据

> 「独立实现」= 前端侧自己写的判据（单测 / E2E），**不依赖**后端这一轮的实现。

| 用例 | 我实现的部分 | 判据（前端侧） | 状态 |
|---|---|---|---|
| TC-014-111 | 路由 + 页面 + 首屏加载 | 页面渲染出行；空态文案「暂无审计记录」**不出现**；请求数 ≥ 1 | 单测覆盖「列 + 空态」；「有记录」需后端 → E2E 跳过 |
| TC-014-112 | 8 列（含 5 项必显） | 列标签集合含 操作人 / 时间 / 动作 / 对象 / 来源 IP；第一行这 5 格**都非空** | ✅ 单测（列标签）+ E2E（首行非空，跳过） |
| TC-014-113 | **服务端**分页 | 翻页请求带 `page=2`（页码**进 query**）；页码指示 1 → 2 | ✅ 单测（query 形状）+ E2E（跳过） |
| TC-014-114 | 前端不切片、不追加 | 每页一次**新请求**（query 带页码）；行**整体替换**（不 append）→ 前端不会自己造重复 | ✅ 单测（query 形状）；「三页 id 两两无交集」由后端排序保证（`createdAt DESC, id DESC`），E2E 跳过 |
| TC-014-115 | 操作人过滤 | 请求带 `filter[actorId][eq]=…`；行的操作人与输入**逐字相同** | ✅ 单测（参数）+ E2E（跳过） |
| TC-014-116 | 动作过滤 | 请求带 `filter[action][eq]=…`；行的动作与输入**逐字相同** | ✅ 单测（参数）+ E2E（跳过） |
| TC-014-117 | 前端 = 后端 | 前端发的过滤条件与直接调接口用的**同形**；E2E 直接比对两边的 id 集合与 total | E2E 写了（跳过）；前端侧保证 = 参数同形（单测） |
| TC-014-118 | 守卫 → 403 + 请求数 0 | 最终地址 `/403`；`data-testid="forbidden"` 存在；`/api/audit/events` 请求数 **0** | ✅ **单测**（假客户端计数）+ E2E（不需后端） |
| TC-014-119 | 403 页非空白 | 可见文本非空 + `<h2>403</h2>` + 不含任何审计记录内容 | ✅ E2E（复用既有 `Forbidden.svelte`）；单测侧由既有路由用例保证落点 |
| TC-014-120 | E2E 的 skip 纪律 | 全文件只有**一种** skip 理由：`'审计接口尚未实现（被测系统未就绪）'`；**没有**针对「无权限」「403」「数据为空」的 skip | ✅ 人工核对（见 §4 清单） |

---

## 4. 验证记录（真跑）

### 4.1 前端单测（vitest）

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
pnpm --filter @base/shell test
```

| 轮次 | 结果 |
|---|---|
| 既有基线（本次改动前） | **158 passed** |
| 本次改动后 | **195 passed**（17 个测试文件）→ **+37**（`audit-api.test.ts` 30 + `audit-page.test.ts` 7） |

`pnpm -r test`（全仓前端）：

| 包 | 结果 |
|---|---|
| `apps/shell` | **195 passed** |
| `packages/i18n` | **43 passed**（加了 15 个键后仍全绿：两语言包键集合一致、无重复文案、键名规范） |
| `packages/sdk` | **38 passed**（**先 `pnpm -r build` 之后**才是 38；构建前是 `36 passed | 2 skipped` —— 那 2 条 skip 需要构建产物才真跑） |
| `packages/auth` / `hooks` / `schema` / `theme` / `ui` | 26 / 17 / 38 / 12 / 14 passed |

### 4.2 类型检查 / 构建 / 静态检查

```bash
pnpm --filter @base/shell check     # svelte-check → 0 errors / 0 warnings
pnpm -r build                       # 全部通过（含 apps/shell 的 vite build）
npx eslint apps/shell/src apps/shell/test e2e    # 0 问题
npx prettier --check apps/shell/src apps/shell/test e2e packages/i18n/src   # 全部符合
npx tsc --noEmit --strict e2e/audit.spec.ts      # E2E 规格类型检查通过
```

`pnpm -r build` 实测输出：`packages/{hooks,i18n,schema,sdk,theme,auth,ui} build: Done` + `apps/shell build: ✓ built in 1.77s`（`dist/index.html` + `dist/assets/index-*.js`）。

### 4.3 E2E

**本轮未跑**（两个原因，都是环境原因，不是用例问题）：

1. 后端 `/api/audit/*` 本轮**还没实现**（另一个 Dev 在做）→ 数据类用例按「被测系统未就绪」跳过；
2. 跑 E2E 要起 `platform-server`（`scripts/e2e-server.sh` → `cargo run -p platform-server`），
   **会与另一 Dev 正在跑的 `cargo test -p platform-audit` 抢 CPU**（本仓低配机 8 线程；
   dev-guide 的要求是**串行跑**）。本轮未与其并发。

**skip 清单（逐条；理由必须都是「被测系统未就绪」）**：

| 用例 | skip 位置 | 理由 | 判定 |
|---|---|---|---|
| TC-014-111 / 112（列与记录） | `e2e/audit.spec.ts`「TC-014-111 / TC-014-112」开头 | `审计接口尚未实现（被测系统未就绪）` | ✅ 合规（`auditApiReady()` 探测到 404/501 才 skip） |
| TC-014-113 / 114（翻页） | 同上文件「TC-014-113 / TC-014-114」开头 | 同上 | ✅ 合规 |
| TC-014-115 / 117（操作人过滤 + 交叉验证） | 同上文件「TC-014-115 / TC-014-117」开头 | 同上 | ✅ 合规 |
| TC-014-116（动作过滤） | 同上文件「TC-014-116」开头 | 同上 | ✅ 合规 |
| TC-014-118 / 119（403 页） | **无 skip** | — | ✅ 不需要后端：守卫在请求之前拦下（前端机制），后端在不在都不影响 |

**没有**任何一条 skip 是因为「夹具不对」「授权没配好」（D108：那些必须 **fail**）。

`auditApiReady()` 的判定口径：探测 `GET /api/audit/events` → **404 / 501** = 路由还不存在（被测系统未就绪，可 skip）；
**其它状态（401 / 403 / 200 …）= 路由已在** → 用例**必须跑**，出问题就 **fail**（不许 skip）。

### 4.4 E2E 的夹具依赖（**需要 BA / 后端 Dev 补**）

`e2e/audit.spec.ts` 用两个账号：

| 账号 | 权限点 | 现状 |
|---|---|---|
| `u-perm-none@example.com` | （无） | ✅ 已在 `scripts/seed-e2e.sh` 里 |
| `u-perm-audit@example.com` | `audit:events:read` | ❌ **还没进** `scripts/seed-e2e.sh` |

- `scripts/seed-e2e.sh` **不在本任务的文件集内**（我只许改 `apps/shell/**` 与 `e2e/**`）→ 我没有改它。
- 补上之前，**数据类用例会因登录不上而红** —— 那是**夹具缺失**，按 D108 必须 fail（我没有用 skip 掩盖）。
- 翻页用例还需要夹具里 **≥ 11 条**审计记录（页大小调到 10 才凑得出两页）。
- 另外：`audit:events:read` 这个权限点要进后端的权限点注册表（`crates/platform-audit/manifest.json` 等）→ 属于后端 Dev 的文件集。

---

## 5. 与规格不一致的发现 / 偏离

### 偏离 1：i18n 资源在 `packages/i18n/**`（不在我声明的文件集里）

- 任务卡说「只许改 `apps/shell/**` 和 `e2e/**`」，但**审计页的面向用户文案必须进 i18n 资源**，
  而本仓唯一的 i18n 资源在 `packages/i18n/src/locales/{zh-CN,en-US}.json`（`@base/i18n` 的内置语言包，
  `apps/shell` 没有自己的语言包；键的 domain 白名单只有 `error` / `common` / `schema` / `nav`）。
- → 我加了 **15 个键**（`nav.audit` + `common.audit*`），两个语言包**键集合一致**、无重复文案。
- **不与另一 Dev 冲突**（他在 `crates/**`）。若 BA 认为越界，回退这两处即可（前端会回退成 `"?"`）。

### 偏离 2：`describeError` 从 `schema-api.ts` 搬到 `i18n.ts`

- 原因：它是**通用**的「错误码 → 当前语言文案」helper（审计页也要用），却住在 schema 的模块里。
  搬过去后 `schema-api.ts` **re-export** 同一个函数 → 既有调用点（`SchemaPage.svelte`）
  与既有测试（`apps/shell/test/schema-api.test.ts`）的 import 路径**一个字都不用改**。

### 发现 1（⚠️ 规格内部冲突，需 BA 裁决）：**页码分页 vs 游标分页**

| 来源 | 口径 |
|---|---|
| D145 / 契约 C7 | **游标分页是主口径**（`cursor` + `limit`）；页码是「**需要页码跳转时**」用的兼容路径 |
| QA `test-cases/frontend.md` TC-014-113 | 「后端收到的请求参数含 **`page=2`**」、页码指示 1 → 2 |
| 我的 AC-S-132 硬性判据 3 | 「翻页后记录**不重复**（后端排序是 `createdAt DESC, id DESC`）」 |

**我的取舍**：界面用**页码**模式（`page` + `page_size`），理由：

1. 审计页要显示「第几页 / 共几页」→ 响应里的 `total` 能算出页数；**游标模式算不出页码**
   （响应只有 `nextCursor`）→ 用游标就满足不了 TC-014-113 的「页码指示」；
2. 契约 C7 明确允许 `?page=N&page_size=M`（「需要页码跳转时」）；
3. `buildAuditQuery` **两种模式都实现了**（游标模式发 `cursor` + `limit`），**互斥校验**也在
   （同时给 → 抛错，不发非法请求）。

**残留风险（如实报告）**：D145 给游标的理由是「审计表**持续追加**，按页码翻页**必然**出现重复/遗漏」。
本实现用页码 → 在**记录持续增长**时，第 2 页可能与第 1 页有交集（AC-S-132 步骤 2 的「不重复」）。
**缓解**：① 后端默认排序带 `id` 作 tiebreaker（D155）；② 游标模式已实现，
切成游标只需改 `Audit.svelte` 调 `loadAuditEvents` 时的入参（页面其余部分不动）。
**请 BA 裁决**：是「界面改游标 + 页码指示改成『第 N 次翻页』」，还是「保持页码 + 接受增长期的重复风险」。

### 发现 2：QA 的 `frontend.md` 用例是 rev 1 的口径（与 rev 2 / rev 3 不完全一致）

- 该文件写于 09:21，而 `decisions.md` rev 2 是 09:29、`acceptance.md` rev 3 是 10:20 →
  用例里仍写着 rev 1 的参数名（`page`/`size`、平铺的 `actorId=`/`action=`）。
  我按**rev 2 / rev 3 + 契约 C7** 实现（`page`/`page_size`、`filter[field][op]`）。
- TC-014-115 / 116 的期望「后端请求带上了 `actorId=…` / `action=…`」→
  我按 C7 落成 `filter[actorId][eq]=…` / `filter[action][eq]=…`（**过滤形状以契约为准**）。

### 发现 3：`audit:events:read` 是**菜单级**权限点（design §9）

前端按**静态权限点**声明（`routes.ts` 的 `permission`），与后端鉴权读**同一份注册表**（契约 C4）→ 一致。

### 发现 4：审计相关的 5 个错误码**没有**本地化文案（故意的，交给后端 Dev 同步）

- 后端这一轮新增 5 个错误码（design §7：`AUDIT_INVALID_FILTER` / `AUDIT_SENSITIVE_PAYLOAD_REJECTED` /
  `AUDIT_CHAIN_BROKEN` / `AUDIT_EVENT_NOT_FOUND` / `AUDIT_SINK_UNAVAILABLE`）。
- 我**没有**往 `packages/i18n/src/locales/*.json` 里加这 5 个 `error.*` 键，原因有两条：
  1. **会与后端 Dev 撞同一处编辑**（错误码是他的交付物；MONEY_OVERFLOW / FX_RATE_NOT_FOUND 的先例是
     「后端同期新增 → 前端语言包与测试列表**两侧同时**改」）；
  2. `packages/i18n/test/i18n.test.ts` 有一条**精确计数**断言：
     「错误域条目数 == 抽样列表长度」→ 只改语言包不改列表，**那条测试会红**。
- 现状影响：审计页加载失败时，提示回退成 `"AUDIT_INVALID_FILTER (HTTP 400)"`
  （`describeError` 的既有约定：没有文案时显示技术信息，不显示 `"?"`）→ **不是坏 UI**。
- 要本地化的话：**两侧同时**加（语言包 2 个 + 测试的 `codes` 列表），由后端 Dev 或下一轮一起做。

---

## 6. 未完成 / 待别人做的部分

| 项 | 原因 | 归属 |
|---|---|---|
| E2E 实跑 | 后端审计接口未实现 + 与另一 Dev 的 cargo 构建抢 CPU | 下一轮（后端合入后） |
| E2E 夹具 `u-perm-audit@example.com`（授 `audit:events:read`） | `scripts/seed-e2e.sh` 不在我的文件集 | BA / 后端 Dev |
| 翻页用例的夹具数据（≥ 11 条） | 需要后端能写审计记录 | BA / 后端 Dev |
| 后端 `/api/audit/events` 实现 | 另一个 Dev 的任务 | 后端 Dev |

---

## 7. 复现命令

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-014
export XDG_RUNTIME_DIR=/run/user/$(id -u)
pnpm install --offline --frozen-lockfile   # 本 worktree 首次需要
pnpm --filter @base/shell test             # → 195 passed
pnpm --filter @base/shell check            # → 0 errors / 0 warnings
pnpm -r build                              # → 全绿（sdk 的 2 条 skip 变成真跑）
pnpm -r test                               # → 全仓前端全绿
npx eslint apps/shell/src apps/shell/test e2e
npx prettier --check apps/shell/src apps/shell/test e2e packages/i18n/src
```

> ⚠️ `pnpm -r build` 与 playwright **串行跑**（低配机 8 线程，dev-guide §5 / §9）。
