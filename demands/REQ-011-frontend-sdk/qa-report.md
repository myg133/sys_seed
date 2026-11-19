# 验证报告 —— Pre-merge

- **需求**: REQ-011「前端 SDK 骨架」
- **被验分支**: `feature/REQ-011` @ `3bf8c3f`（含 `4dda8e8` 后端 + `752791e` 前端 + `5518921`/`82a0365`/`cd50e1b`/`3bf8c3f` BA 补正）
- **基线**: `develop` @ `a3fae66`（REQ-010 合并点）
- **验收基线**: `BA/demands/REQ-011-frontend-sdk/acceptance.md` **rev 2**（AC-S-064 ~ AC-S-075，12 条）
- **验证者**: QA Agent（**独立复现；Dev / BA 自验报告里的每个数字都重跑过，未采信**）
- **环境**: rustc/cargo 1.98.1 · node v26.13.1 · pnpm 12.19.0 · podman + postgres 17-alpine（`base-postgres` / `base-redis` healthy）· `CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target`
- **日期**: 2026-11-19
- **结论**: **FAIL** —— 1 条 HIGH（壳层登录不可用）+ 1 条 AC 未达成（AC-S-068 步骤 3）

> **核心结论**
>
> - **11 / 12 条 AC 通过**（AC-S-064 ~ AC-S-067、AC-S-069 ~ AC-S-075），全部有实跑证据。
> - **AC-S-068 失败**：步骤 3「直接输入该路径 URL → 同样落在 403 页」**实测落到登录页**（不是 403）。
> - **HIGH**：`apps/shell` **没有任何租户来源** → 登录请求不带 `x-tenant-id` → 后端 `401 TENANT_CONTEXT_MISSING`
>   → **壳层根本登不进去**，AC-S-067 / AC-S-068 的应用级验收在交付物上**不可达**。
> - **8 处变异全部被测试抓红**（含 D92 的**双点**变异与刷新去重的**双点**变异），
>   变异全部**逐字节还原**（`git diff --exit-code HEAD` 通过）。
> - **AC-S-066 步骤 5（前后端一致性）**此前**无人跑过**（Dev 自报"没跑"）；
>   我用**自建真机**（真实 `base serve` + `base_app` 连接 + `base_test` 库）**实测 12/12 一致**，
>   并据此确认 BA 新增的金标准表 `can_consistency.test.ts` 的期望值**属实**。
> - **D92 的必要性已在真机上复现**：401 后沿用同一幂等键重放 → 后端**实测 409 CONFLICT**；
>   不带键重放 → **不是 409**。

---

## 1. 逐 AC 结论表

| AC | 标题 | 我实际执行的检查 | 结论 |
|---|---|---|---|
| **AC-S-064** | 后端能返回当前用户的有效权限点集合 | **真机**（真实 `base serve` + 应用角色连接）：200 / 响应体恰好一个键 `permissions` / 元素均为 string / 升序去重 / 含 `rbac:role:read` 不含 `rbac:role:write` / 无令牌 → 401 `UNAUTHENTICATED` / 跨租户头 → 404 `NOT_FOUND` 且响应体**无** `permissions` 字段 | **PASS** |
| **AC-S-065** | 登录响应带上权限点 | **真机** 3 个用户：登录响应 `permissions` 非空 / 升序去重；**登录响应 vs 端点逐用户深度相等**（read / none / write 三组全等）；无角色用户 → `permissions === []`（`hasOwnProperty` 为 true、非 null）；provider 未注入的降级路径由 `tc_011_010` 单测覆盖（通过） | **PASS** |
| **AC-S-066** | `can()` 前端判定且与后端一致 | `can()` 空集合 → false / 未登记权限点 → false 且不抛错 / 集合外 → false；**步骤 5 用自建真机做真实路由探针**（`GET /api/rbac/roles` 验 `rbac:role:read`、`POST /api/rbac/roles` 验 `rbac:role:write`），前端用**同一份金标准夹具**调 `can()` → **12/12 逐条一致** | **PASS** |
| **AC-S-067** | 无权限时内容不渲染 | ① SSR 探针（自建 vite `ssrLoadModule`）：有权限 → 渲染子内容；无权限 → 剔除 Svelte 5 锚点后**精确等于空串** + 原始 HTML 不含子内容文本 + 不含 `display:none` / `visibility:hidden`；fallback 渲染且不含 children ② **真浏览器**（Playwright + Chromium）：WRITE 用户 → `a.base-home-action` 在 DOM；READ 用户 → 不在 DOM、fallback「无权限」在 DOM；无角色用户 → fail closed | **PASS**（但应用级可达性受 HIGH-1 影响） |
| **AC-S-068** | 路由守卫拦截无权访问 | ① 包级 `guard()` → `{allowed:false, redirectTo:'/403'}` ✓ ② 应用级程序化跳转 → **真浏览器 403 页，目标接口请求计数 = 0** ✓ ③ **应用级直接敲 URL → 实测落到 `/login`，不是 403** ✗ ④ 目标接口未被请求（计数 0）✓ ⑤ 未登录 → 登录页、页面无「403」字样 ✓ | **FAIL**（步骤 3） |
| **AC-S-069** | SDK 统一解析 RFC 9457 错误体 | 自建探针加载**构建产物** `packages/sdk/dist/index.js`：`code === 'INSUFFICIENT_PERMISSION'`（不是 title / detail）、`status === 403`、不是裸 `TypeError`；HTTP 错误 + 非 JSON → `client.INTERNAL_ERROR`；2xx + 非 JSON → `client.INTERNAL_ERROR`；2xx + 空体 → 返回空值不抛错；错误状态 + 空体 → `client.INTERNAL_ERROR`；网络层失败 → `client.NETWORK_ERROR` 且不抛裸 `TypeError`；**原始响应体 4 个字符串**（`message` / `detail` / `title` / `JSON.stringify(error)`）均不含密文，且错误对象无 `response` / `raw` / `body` 等新字段 | **PASS** |
| **AC-S-070** | 过期自动刷新并重放，并发只刷新一次 | ① D92：过期 token + 写请求 → **重放请求头里没有 `idempotency-key`**（断言 `k2 === null`，不是断言"键不同"）② **同时**发 5 个请求（受控 pending Promise）→ 刷新接口**被调 1 次**（窗口内 = 1、总计 = 1、业务路径 10 次、5 个全部 resolve）③ 刷新失败 → 抛**后端码** `TOKEN_REVOKED`（不是 `UNAUTHENTICATED`、不是 `client.*`）、不无限重试（业务 1 次 / 刷新 1 次）④ 重放请求体 **`new Uint8Array(await request.clone().arrayBuffer())` 逐字节比对**（含中文与 emoji，len 67 = 67，逐字节相等） | **PASS** |
| **AC-S-071** | 写操作注入幂等键，读操作不注入 | POST/PUT/DELETE 均带 `idempotency-key` 且都是 **UUID v4**；GET **不带**；两次写键不同、**不是由路径派生**；**真机端到端**：同一键同一写请求发两次 → 第二次返回**首次的结果**、**只落库 1 条**；★ **D92 现场复现**：401 后沿用同一键重放 → **409 CONFLICT "the idempotent request is still being processed"**；不带键重放 → **不是 409** | **PASS** |
| **AC-S-072** | SDK 注入租户与空间上下文头 | 配置 `tenantId` → `x-tenant-id` 等于配置值；**未配置 → 头根本不存在**（`headers.has('x-tenant-id') === false`，不是空串）；配置 `spaceId` → `x-space-id`；**login / refresh 也带租户头**（两条都验） | **PASS**（SDK 层）→ 应用层缺陷见 **HIGH-1** |
| **AC-S-073** | 主题令牌以 CSS 变量输出，暗色可切换 | 根元素变量名清单 = 契约表**精确 9 个**（`color-scheme` 是 AC 步骤 2 另行要求的标准属性，不计入令牌清单）；亮色取值**逐个等于**契约表；暗色取值**逐个等于**契约表**暗色值**（不是"不等于亮色值"）；`color-scheme === 'dark'`、`data-theme === 'dark'`；**切回亮色可逆**、`color-scheme === 'light'`；模式无关变量（radius / font-family）不参与判定 | **PASS** |
| **AC-S-073 步骤 4** | 颜色字面量扫描（D95，参数写死） | 文件集 = 五包 `src/**/*.ts`；排除 `test/`、`*.test.ts`、`packages/theme/src/tokens.ts`；正则 `#[0-9a-fA-F]{3,8}\b` + `rgb(` / `hsl(` / `oklch(` → **排除后 5 个文件，零命中**；`@base/ui` **零**字面量；`apps/shell/src` 零命中；字面量**只**出现在 `packages/theme/src/tokens.ts` | **PASS** |
| **AC-S-074** | 主题主色接品牌接口，不硬编码 | `--base-primary === '#0055aa'`（接口值）；**非法值** 8 种（`#zzz` / `red` / `rgb(1,2,3)` / `#12345` / `''` / `notacolor` / 注入串）→ **全部 `currentColor`**；**无品牌** → `currentColor`；兜底由**主题包自己写**（`root.props.has('--base-primary')` 为 true，不依赖宿主样式表）；`apps/shell/src/lib/branding.ts` **已委托** `@base/theme`（无第二份实现） | **PASS** |
| **AC-S-075** | 五个包无交叉依赖，各自产物独立 | **双向**断言 12/12：禁止项 5 条全不存在（`sdk→ui/theme/hooks`、`auth→ui`、`ui→hooks`）、规定项 2 条全存在（`auth→sdk`、`ui→theme`）且均为 `workspace:*`；`packages/sdk/dist/**` 无 `@base/(ui\|theme\|hooks)` 导入、**无** `document.` / `window.` / `HTMLElement` / `customElements`；`packages/sdk/dist` 不含组件代码；`Cargo.lock` **未变**、仍 **295** 包 | **PASS** |

---

## 2. 问题清单

### 高

#### **HIGH-1：`apps/shell` 没有任何租户来源 → 壳层登录 100% 失败**

- **问题**：`apps/shell/src/App.svelte:30` 调用 `createAppApi()` **不传任何参数**；
  `apps/shell/src/lib/api.ts:43` 的签名是 `createAppApi(options: { tenantId?: string } = {})`，
  `:63` 把 `tenantId: options.tenantId`（= `undefined`）交给 SDK。
  SDK 按 AC-S-072 步骤 2 正确地**不带** `x-tenant-id` 头 →
  后端 `crates/platform-iam/src/service.rs:382` 的 `ctx.tenant_id().ok_or(IamError::TenantContextMissing)?`
  触发 → **401 `TENANT_CONTEXT_MISSING`**。
- **证据**（真浏览器抓包）：
  ```
  REQ POST http://127.0.0.1:8080/api/iam/login
      headers={... "content-type":"application/json"}          ← 无 x-tenant-id
  RES 401 body={"type":".../tenant-context-missing","title":"缺少租户上下文",
                "status":401,"code":"TENANT_CONTEXT_MISSING",...}
  页面上的错误提示 = ["登录失败：用户名或密码不正确"]               ← 文案误导
  最终 URL = http://127.0.0.1:8080/login
  ```
  **壳层没有任何别的租户来源**（已穷举）：
  - `grep -rn "import.meta.env\|VITE_" apps/shell/src/` → **无命中**
  - `grep -rn "localStorage\|sessionStorage\|cookie" apps/shell/src/ packages/*/src/` → **无命中**
  - `grep -rn "DEFAULT_TENANT\|default_tenant\|BASE_TENANT" crates/ --include=*.rs` → **无默认租户机制**
- **影响**：
  1. 交付的「最小壳」**不可用** —— 用户永远登不进去。
  2. **AC-S-067 / AC-S-068 的应用级验收在交付物上不可达**：
     我用 Playwright **只在网络层补一个 `x-tenant-id` 头**（不改任何源码），
     补上之后 AC-S-067 / AC-S-068 的断言**全部成立** → 证明问题**只有这一个根因**，
     也证明**其它**实现（`can()` / `guard()` / `<Can>` / 路由守卫）本身是对的。
  3. **AC-S-072 步骤 4 的警告正是这个**：「若 SDK 不在登录请求上带租户头，
     `tenantId` 未配置时**根本登不进去**」（`acceptance.md:293-296`）——
     AC 早就写明了这个失败模式，但壳层至今没有接上租户。
- **复现步骤**：
  ```bash
  cd feature-REQ-011
  pnpm --filter @base/shell build
  ./scripts/e2e-server.sh          # 真机
  # 浏览器打开 /login，任意账号登录 → 抓包可见无 x-tenant-id → 401 TENANT_CONTEXT_MISSING
  ```
- **建议修法**（三选一，需 BA 拍板租户来源）：
  1. **登录页带租户输入**：`createAppApi({ tenantId })` 的 `tenantId` 由登录表单提供（最小壳推荐 —— 与"租户切换器下推"的范围声明一致）；
  2. **构建期注入**：`import.meta.env.VITE_TENANT_ID`（需在 `.env.example` 增一行）；
  3. **后端兜底**：给 IAM 的 login 增加"单租户部署模式"（`BASE_IAM_DEFAULT_TENANT`），缺省时用配置租户。
  > 另：登录失败提示恒为「登录失败：用户名或密码不正确」，
  > 但实际是 `TENANT_CONTEXT_MISSING` —— 建议按 `code` 分派文案（D94 的精神）。

### 中

#### **MED-1：AC-S-068 步骤 3「直接敲 URL → 403」实测落到 `/login`**

- **问题**：AC 要求「**直接输入该路径 URL**（绕过前端路由跳转）→ **同样**落在 403 页」。
  实测（已注入租户头、用户**确实已登录**）：`page.goto('/admin/roles')` 后
  **最终 URL = `/login`**、403 元素数 = 0。
- **根因**：**不是**路由守卫的深链处理有问题 —— 路由器 `apps/shell/src/lib/router.ts:171-179`
  的 `start()` 会对当前地址（深链）走一遍 `apply()` → 守卫，这条**是对的**
  （单测 TC-011-020 覆盖，且我用浏览器验证「程序化跳转 → 403 页、目标接口计数 0」通过）。
  真正的原因是**会话只在内存**：`grep -rn "localStorage\|sessionStorage\|cookie" apps/shell/src/` → 无命中，
  令牌在 `apps/shell/src/lib/api.ts:44` 的模块级 `const session: Session = {}` 里。
  整页重载 → 令牌丢失 → 应用认为未登录 → 落登录页。
- **证据**：
  ```
  [深链] 登录后 URL=http://127.0.0.1:8080/        ← 已登录
  [深链] 最终 URL=http://127.0.0.1:8080/login     ← ✗ 不是 /403
  [深链] 403元素=0，登录表单=1
  [深链] 目标接口计数=0                            ← ✓ 这一条仍成立
  ```
  对照（同一轮、同一用户、程序化跳转）：`[程序化] 403元素=1，URL=/403，目标接口计数=0`。
- **影响**：AC-S-068 **步骤 3 未达成**（步骤 2 / 4 / 5 达成）。
  "直接敲 URL 是最常见的绕过方式"这个诉求，在**页面刷新**场景下没有被守住 ——
  不过要说清楚：**它没有变成"放行"**（目标接口请求计数仍是 0，没有越权），
  只是落点从 403 变成了登录页。**不是安全绕过，是落点与 AC 不符**。
- **建议修法**（需 BA 裁定方向）：
  - **方案 A（推荐，符合 AC 字面）**：会话持久化（`sessionStorage` / `localStorage` 存令牌 + 启动时恢复）
    → 深链时 `router.start()` 拿到已认证状态 → 落 403。**注意**：这会引入 XSS 取令牌的面，需 BA 评估；
  - **方案 B（改 AC）**：AC-S-068 步骤 3 改为「**在已登录会话内**深链（`pushState` + `popstate`）→ 同样落 403」，
    并注明「整页重载后的会话恢复属后续需求」。当前实现**已满足**方案 B 的字面（单测 TC-011-020 覆盖）。

#### **MED-2：BA 的用例集仍是 rev 1，与 acceptance rev 2 有两处互斥**

- **问题**：`BA/demands/REQ-011-frontend-sdk/test-cases/` 的 48 条用例**没有同步到 acceptance rev 2**，
  其中两条的预期与 D92 / D94 **直接互斥**：

  | 用例 | rev 1 的预期 | acceptance rev 2（D92/D94）的预期 |
  |---|---|---|
  | `TC-011-029` | `error.code === 'UNAUTHENTICATED'`（notes 里 BA 自己写了「AC 冲突…本条预期必须同步改写」） | 透传**后端实际码**（`TOKEN_REVOKED` 等） |
  | `TC-011-034` | 标题「重放请求使用**新的**幂等键」 | 重放**不带** `idempotency-key` |

- **证据**：`test-cases/frontend-sdk.json`（`TC-011-029` 的 `expected` 与 `notes`；`TC-011-034` 的 `title`）
  vs `acceptance.md:244-245`（D94）与 `acceptance.md:233-234`（D92）。
  **实现是对的**（我的探针实测重放无键、刷新失败透传 `TOKEN_REVOKED`），
  错的是**用例集本身没跟上**。
- **影响**：用例集是执行层的唯一依据。若后来者照 `TC-011-034` 的标题实现
  （"用新的键"），也能过测试、但会留下**两个半截键**（status=0）—— 与 D92 的修法意图相悖。
- **建议修法**：BA 把 `frontend-sdk.json` 的 `TC-011-029` / `TC-011-034` 改成 rev 2 口径
  （用例 id 与 `acceptance.md` 步骤号一一对应），并同步 `test-cases/index.md` 的状态行。

### 低

#### **LOW-1：`apps/shell` 里两处注释与代码/事实不符（D97 之后未同步）**

`cd50e1b` 修正了 `Can.svelte` 与 `lib/handlers.ts` 的过时注释，但**漏了两处**：

| 位置 | 过时注释 | 实际 |
|---|---|---|
| `apps/shell/src/App.svelte:10-12` | 「⚠️ 脚本块写成 **JS 语法**：本仓的 eslint 配置没有给 `**/*.svelte` 配 TypeScript 解析器，写 TS 注解会让 `eslint .` 报解析错误。」 | 该文件是 `<script lang="ts">`（`:14`），且 `eslint.config.js:28-33` **已配** TS parser |
| `apps/shell/src/pages/Login.svelte:9-11` | 同上 | 同上 |

- **证据**：`eslint.config.js:22-34` 明确给 `**/*.svelte` 配了 `parserOptions.parser: ts.parser`；
  `eslint .` 实跑 **0 问题**；`svelte-check` 实跑 **0 errors and 0 warnings**。
- **影响**：仅注释漂移，**无功能影响**。但它会误导下一个读代码的人
  （"`.svelte` 不能写 TS" → 会把类型契约继续往外赶，恰好与 D97 的方向相反）。
- **建议修法**：照 `cd50e1b` 对 `handlers.ts` 的处理方式，把这两处注释改成
  「`.svelte` 的脚本块**可以**写 TS（根 `eslint.config.js` 已配 TS parser）；
  但模板内联箭头函数缺上下文类型，带参数的处理器仍放 `.ts`」。

#### **LOW-2：`packages/theme/dist/index.js` 用无扩展名的相对导入，Node 原生 ESM 加载不了**

- **证据**：`packages/theme/dist/index.js:32`
  `import { ... } from './tokens';`（**缺 `.js`**）→ Node ESM 报 `ERR_MODULE_NOT_FOUND`；
  另外 4 个包的 `dist/index.js` **无任何 import**（`auth` / `hooks` / `sdk` / `ui`）。
- **影响**：**当前无实际影响** —— 消费方 `apps/shell` 走 Vite（bundler 解析），
  单测走 vitest，都不受影响。AC-S-075 步骤 2 的字面要求（"每个包的 dist 只 import 自己的依赖"）**未被违反**。
  但该产物**不能**被 Node 原生 ESM 直接 import（未来 SSR / Node 侧消费会踩）。
- **建议修法**：要么在 `packages/theme/tsconfig.json` 里让 `tsc` 产出带 `.js` 后缀的相对导入，
  要么在能力索引里注明"本包产物仅供 bundler 消费"。

#### **LOW-3：Dev 交接笔记里 `GET /api/rbac/roles` 所需权限点写错了**

- **证据**：`feature-meta/dev-report-frontend.md:367` 写
  「`GET /api/rbac/roles` 需要 `rbac:role:write`」；
  实际 `crates/platform-rbac/src/http.rs:460` 是 `require_permission("rbac:role:read")`，
  `:464-466` 的 POST 才是 `rbac:role:write`。
  真机实测：`u-perm-read`（只有 `rbac:role:read`）→ `GET /api/rbac/roles` = **200**；
  同用户 `POST /api/rbac/roles` = **403 `INSUFFICIENT_PERMISSION`**。
- **影响**：仅交接文档错误。BA 新增的金标准表是对的（用 `rbac:role:read`）。
- **建议修法**：Dev 修 `dev-report-frontend.md` 那一行。

#### **LOW-4：`prettier --check .` 退出码 1（2 个文件）—— 既有、非本期引入、但门禁长期红着**

- **证据**：`pnpm exec prettier --check .` → **exit 1**，
  `[warn] crates/platform-rbac/manifest.json`、`[warn] feature-meta/manifest.json`。
  在**基线 `a3fae66`** 上跑同样两个文件 → **同样 exit 1**（我另开 worktree 复核过）。
  两个文件本期都**不是新问题**：`platform-rbac/manifest.json` 本期**未改**（`git diff a3fae66 HEAD` 为空）；
  `feature-meta/manifest.json` 本期改了内容，但**缺行尾换行**这一形态基线已有。
- **影响**：**非本期引入**，但它在本期的验收命令清单里 → 门禁长期红着，
  会掩盖后续真正的新增格式问题。Dev 的自验报告也承认了这一点
  （`dev-report-frontend.md:346`「仅剩 2 个**既有** prettier 告警（不在本次范围）」）—— 属实。
- **建议修法**：本轮顺手 `prettier --write` 这两个文件并提交；
  或登记进 `BA/plan/tech-debt.md`（目前该清单**没有**这一条）。

#### **LOW-5：`iam_real_db.rs:706` 的时序断言对机器负载敏感**

- **证据**：我第 2 轮回归时**自己起的 `base serve` 进程**在跑（占 CPU），
  该用例失败：
  ```
  panicked at crates/platform-iam/tests/iam_real_db.rs:706:
  三条失败路径的耗时必须接近（ratio = 1.74，错密码(未锁) = 540.5ms，
  错密码(已锁) = 777.1ms，账号不存在 = 447.6ms）
  ```
  **停掉我的服务后重跑同一套件 → 550 passed / 0 failed**（见 §5）。
  该用例在**基线就存在**（REQ-007），**本期未改**该文件。
- **影响**：**非产品缺陷**，是既有测试在低配机 + 并发负载下的偶发红。
  但它会让"连跑 2 轮"的要求变得不可靠（我因此多跑了一轮）。
- **建议修法**：登记进 `BA/plan/tech-debt.md`；或把该用例的 ratio 阈值放宽 /
  在 `cargo test` 时降低并发（`--test-threads=1`）。

---

## 3. 变异验证表

> 纪律：每个变异**改 → 跑用例 → 逐字节还原（sha256 校验）→ 复跑确认恢复绿**。
> 还原后 `git diff --exit-code HEAD` 通过，工作区**干净**。

| # | 变异点 | 预期变红的用例 | **实际是否变红** | 结论 |
|---|---|---|---|---|
| **M1** | `can()` 恒返回 `true`（`packages/auth/src/index.ts:87`） | TC-011-012 / TC-011-013 / `can_consistency.test.ts` | **变红 ✓**（9 条用例红，含 `can_consistency.test.ts > 未登记权限点一律 false`） | 抓到了 |
| **M2** | 401 重放**带上**幂等键（D92 反面）—— **双点破坏**：`sdk/src/index.ts:286` 的 `attempt === 0` 门控 → `attempt >= 0`，**且** `:292-294` 的 `headers.delete('idempotency-key')` 整段删除 | TC-011-034 | **变红 ✓**（`AssertionError: expected 'b552586e-…' to be null`） | 抓到了 |
| **M3-single** | 刷新**不去重** —— 只破**内层**闸门（`:342` `refreshAccessToken` 的早退） | TC-011-028 | **没变红 ✗** | **变异点选错了**（详见下方分析），**不是**测试的漏洞 |
| **M3-double** | 同上，但**双点**破坏：调用点闸门（`:317` `if (refreshInFlight === null)` → 无条件 `await refreshAccessToken()`）**+** 内层早退 | TC-011-028 | **变红 ✓**（`AssertionError: expected [ …(5) ] to have a length of 1 but got 5`） | 抓到了 |
| **M4** | `guard()` 恒放行（去掉 `can()` 那一道，只留未登录判断）—— `auth/src/index.ts:118` | TC-011-019 / TC-011-020 / 落点可配置 | **变红 ✓** | 抓到了 |
| **M4b** | `guard()` 恒放行（连未登录也放行） | TC-011-019 / TC-011-020 / 未登录落登录页 | **变红 ✓** | 抓到了 |
| **M5** | 品牌非法值时**不回退** `currentColor`（`theme/src/index.ts:145`） | TC-011-042 / 只接受合法十六进制 | **变红 ✓**（`expected '#zzz' to be 'currentColor'`） | 抓到了 |
| **M6** | 暗色模式**不写** `color-scheme`（`theme/src/index.ts:125`） | TC-011-039 / TC-011-040 | **变红 ✓**（`expected undefined to be 'dark'`） | 抓到了 |
| **M7** | 应用侧路由守卫恒放行（`apps/shell/src/lib/router.ts:108-112` 把 `allowed:false` 改成 `allowed:true`） | TC-011-021 / TC-011-020 / TC-011-022 / popstate | **变红 ✓** | 抓到了 |

**结论：8 处有效变异全部被抓红**（要求 ≥ 6）。

### 3.1 M3-single 没变红的原因分析（不是测试漏洞）

刷新去重由**两道闸门**共同保证：

1. **调用点闸门**（`packages/sdk/src/index.ts:317`）：
   ```ts
   if (refreshInFlight === null) { await refreshAccessToken(); }
   else { await refreshInFlight; }        // ← 并发窗口：等正在进行的那一次
   ```
2. **内层闸门**（`packages/sdk/src/index.ts:342`）：`refreshAccessToken()` 开头的
   `if (refreshInFlight !== null) { return refreshInFlight; }`

我第一次只破了**第 2 道**（内层早退）—— 第 1 道仍在，5 个并发请求里只有第 1 个会走到
`refreshAccessToken()`，其余 4 个直接 `await refreshInFlight` → **行为完全没变** → 用例当然不红。
**双点破坏后（M3-double）立刻变红**（5 次 refresh）。

> 这与任务书里点名的"Dev 的教训"是**同一个机制的反面**：
> D92 那边是「`attempt===0` 门控与 `attempt>0` 的 delete 互相掩盖」，
> 刷新去重这边是「调用点闸门与内层早退互相掩盖」。
> **结论：TC-011-028 对"去重行为真的丢了"是敏感的**（M3-double 证明了），
> 单点变异测不出来是因为**实现的防御纵深**，不是测试的盲区。

### 3.2 对 BA 新增金标准用例的有效性验证（BA 明确要求）

- **BA 问 1（金标准期望值来自真机）** → **我用自己的真机独立复核了**：
  自建 `base serve`（真实进程 + `base_app` 普通角色连接 + `base_test` 库），
  三个夹具用户实测：

  | 用户 | `/api/rbac/me/permissions` | `GET /api/rbac/roles` | `POST /api/rbac/roles` |
  |---|---|---|---|
  | `u-perm-read` | `["rbac:role:read"]` | **200** ✓ | **403** ✓ |
  | `u-perm-write` | `["rbac:role:read","rbac:role:write"]` | **200** ✓ | **201**（带合法 body）✓ |
  | `u-perm-none` | `[]` | **403** ✓ | **403** ✓ |

  → **BA 金标准表的 3 行期望值全部属实**。我的探针比 BA 的表**多验一个权限点**
  （`rbac:role:write` × 3 个用户），**12/12 逐条一致**。
- **BA 问 2（金标准用例对 `can()` 变异会红）** → **会红**，但**不是**它的头条用例红：
  - `can()` 恒 `true` → 变红的是 `can_consistency.test.ts` 的**第 2 条**
    （「未登记权限点一律 false」）+ `auth.test.ts` 的 9 条；
    **第 1 条**（「一一对应」）**不红** —— 因为 `u-perm-none` 的空集合被 `can()` 的
    提前返回挡住，另外两人的后端本来就放行 → 恒 true 恰好与期望一致。
  - `can()` 恒 `false` → **第 1 条变红** ✓。
  → **该文件整体对 `can()` 双向变异敏感**，但**单看第 1 条是单边的**（只抓 false 方向）。
  建议（可选，LOW）：在第 1 条里补一条「`u-perm-read` 对 `rbac:role:write` 必须 false」
  —— 那样第 1 条也能抓 true 方向。
- **BA 问 3（登录响应与 `/me/permissions` 一致）** → **真机三个用户全验过**：
  read `['rbac:role:read']` == 端点；none `[]` == 端点；
  write `['rbac:role:read','rbac:role:write']` == 端点。**三组深度相等**。

---

## 4. 没验到的（明确列出，不写成"通过"）

| 项 | 原因 | 备注 |
|---|---|---|
| **合并后的 CI** | 本仓**无 git remote**（TD-006，既有问题） | 6 个 job 从未真跑过，本轮同样**没跑** |
| **`cargo test --workspace`** | TD-004 既有陷阱（`platform-config` 缺 `DATABASE_URL_TEST` 即 panic） | 按任务书用 `./scripts/test-integration.sh` 代替；**未**跑过 `--workspace` 形态 |
| **`pnpm -r test` 在 Windows / Node 22 上的表现** | 本机只有 Node 26.13.1 | `engines.node >= 22`，未在 Node 22 实测 |
| **`packages/theme` 被 Node 原生 ESM 加载** | 它**加载不了**（LOW-2） | 故未把它列入 PASS 项；只在 Vite 解析下验证 |
| **Playwright E2E 作为**仓库内**的提交物** | 我把自己的 spec 放在 `/tmp/qa-req011/e2e/`（不污染仓库） | 仓库里的 `e2e/shell.spec.ts` **仍只覆盖 REQ-010 的品牌**，**没有** REQ-011 的登录/403 E2E |
| **AC-S-071 步骤 4 在 `/api/iam/*` 之外的路由** | D93 已声明其他路由无幂等支持 | 按 D93 口径，不在验收范围 |
| **多租户并发 / 压测 / 性能** | AC 未要求 | — |
| **父 AC（AC-016 / AC-017 / AC-019）的完整承载** | 本期 AC 已按 `acceptance.md:45-59` **收窄** | 收窄声明本身我核对无误，但收窄后的**完整**父 AC 由 REQ-020 承担 |
| **`platform-org` 的租户缺口（TD-001）是否该修** | 本期范围外 | 我**只做了自查**：确认它**未被顺手改掉**（静态 + 运行时，见 §5.3） |

---

## 5. 实跑数字

### 5.1 后端回归（`./scripts/test-integration.sh`）

| 轮次 | 条件 | 结果 |
|---|---|---|
| **第 1 轮** | 无并发负载 | **exit 0** · **550 passed / 0 failed**（29 个测试二进制全绿，含新增 `me_permissions` **10 条**） |
| **第 2 轮（作废）** | **我自己起的 `base serve` 在跑**（占 CPU） | **FAILED** —— `iam_real_db` 的时序用例红（ratio 1.74）；中止于 254 passed |
| **第 2 轮（重跑）** | 停掉我的服务，**无并发负载** | **exit 0** · **550 passed / 0 failed** · 容器数比对 2 → 2 |

> 第 2 轮（作废）的那次**不是产品缺陷** —— 见 LOW-5。两轮**干净**运行都是 550/0/0。

### 5.2 前端与工具链

| 命令 | 退出码 | 数字 |
|---|---|---|
| `pnpm -r test`（`3bf8c3f`） | **0** | **126 passed**：sdk 30 · hooks 17 · theme 12 · auth 25 · ui 14 · shell 22 · i18n 3 · schema 3 |
| `pnpm -r test`（`5518921`，早于 BA 补正） | 0 | 123 passed（BA 补正新增 3 条金标准用例 → 126） |
| `pnpm -r check` | **0** | 7 个包 `tsc --noEmit` 全过；`svelte-check` **0 errors and 0 warnings** |
| `pnpm -r build` | **0** | 7 个包 + `apps/shell`（vite build ✓ 131 modules） |
| `pnpm exec eslint .` | **0** | **0 问题** |
| `pnpm exec prettier --check .` | **1** | 2 个文件告警（**基线同样 exit 1**，见 LOW-4） |
| `cargo clippy --workspace --all-targets -- -D warnings` | **0** | **0 告警** |
| `cargo xtask capabilities --check` | **0** | 「能力索引校验通过：13 个文件与源码一致」 |
| `cargo test -p xtask --all-targets` | **0** | **49 passed**（含 `catalog::tests::agents_doc_matches_catalog ... ok` —— **加了 `--all-targets`**） |
| `cargo test -p platform-rbac --all-targets --test me_permissions -- --include-ignored` | **0** | **10 passed**（TC-011-001 ~ 010 逐条绿） |
| `Cargo.lock` | — | **295 个包**，`git diff a3fae66 HEAD -- Cargo.lock` **为空**（未变） |

### 5.3 自查：租户头行为变更的边界（任务书 A5 要求）

- **行为变更确认**：`crates/platform-rbac/src/http.rs:206-216` 现在**读** `x-tenant-id`，
  头租户 ≠ 令牌租户 → **404**（本次新增）。
- **既有接口未被破坏**：全套 **550 passed**（含 `http_authz` / `http_admin` / `org_real_db` 等）。
- **TD-001 未被顺手改掉**（静态 + 运行时双证）：
  ```
  git diff --stat a3fae66 HEAD -- crates/platform-org/     ->  (空，未改)
  crates/platform-org/src/http.rs:288  RequestContext::new().with_tenant(auth.tenant_id.clone())  <- 仍只从令牌取
  运行时：/api/org/org-node     头=tenant-a -> 200
          /api/org/org-node     头=tenant-b -> 200   <- 仍静默忽略（TD-001 保持开放）
          /api/rbac/me/permissions 头=tenant-b -> 404 <- 只有 /api/rbac 改了
  ```

### 5.4 我自建探针的通过率

| 探针 | 断言数 | 通过 |
|---|---|---|
| SDK 探针（加载**构建产物** `packages/sdk/dist/index.js`） | 40 | **40 / 40** |
| SDK 并发去重探针（修正版） | 5 | **5 / 5** |
| SDK 空体 / 非 JSON 探针 | 4 | **4 / 4** |
| 主题探针 | 25 | **25 / 25** |
| 前后端一致性探针（真机） | 12 | **12 / 12** |
| AC-S-075 打包双向断言 | 12 | **12 / 12** |
| D95 颜色字面量扫描 | 3 组 | **全部通过**（排除后 5 文件零命中 / `@base/ui` 零 / `apps/shell` 零） |
| 真浏览器 E2E（Chromium） | 7 spec | **7 passed**（含租户头注入前后的对照） |

### 5.5 工作区最终状态

```
$ git status --short
(空)
$ git diff --exit-code HEAD -- packages/ apps/
  ✓ packages/ 与 apps/ 全部与 HEAD 逐字节一致
```

---

## 6. 合并建议

| 项 | 建议 |
|---|---|
| **后端（`4dda8e8`）** | ✅ **可合** —— AC-S-064 / AC-S-065 全部实测通过，550/550 回归全绿，无新增权限点、不触发 C-5 / D27 对账 |
| **五个包 + SDK（`752791e` / `cd50e1b` / `3bf8c3f`）** | ✅ **可合** —— AC-S-066 ~ AC-S-067、AC-S-069 ~ AC-S-075 全部实测通过，8 处变异全被抓红 |
| **`apps/shell` 最小壳** | ⛔ **建议先修 HIGH-1**（接上租户来源）再合 —— 否则交付的壳层**登不进去**，AC-S-067 / AC-S-068 的应用级验收不可达 |
| **AC-S-068 步骤 3** | ⚠️ **需 BA 裁定**：补会话持久化（方案 A）**或** 修订 AC（方案 B）。二选一，不宜悬着 |
| **MED-2（用例集没同步 rev 2）** | ⚠️ 建议 BA 在本轮一并修 `frontend-sdk.json` 的 `TC-011-029` / `TC-011-034` |
| **LOW-1 ~ LOW-5** | 建议本轮顺手修掉或登记进 `BA/plan/tech-debt.md` |
