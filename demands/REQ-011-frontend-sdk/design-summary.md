# 设计摘要：REQ-011 前端 SDK 骨架

- **需求编号**: REQ-011
- **状态**: 草稿（2026-11-19 10:50）
- **验收标准**: [acceptance.md](acceptance.md)（AC-S-064 ~ AC-S-075）
- **基线**: `develop` @ `a3fae66`（REQ-010 合并点）

---

## 1. 本需求的定位

REQ-009 落地了服务端的鉴权，但**前端怎么用**一直空着 —— `packages/auth/src/index.ts`
至今只有 `AuthState` 形状和 `isAuthenticated`，**没有 `can()`**。
而 `packages/*/src/index.ts` 七个包全是占位：一个 `Tone` 类型、一个 `createApiClient`、
一个 `mergeTheme`。

本需求把这五个包从占位变成**能真实跑通**的实现，并且**补上后端缺失的数据出口**。

### 1.1 为什么本需求要动后端

`can()` 需要「当前用户拥有哪些权限点」。核过后端代码，这个数据**没有任何出口**：

| 候选 | 现状 |
|---|---|
| 登录响应的 `permissions` | `crates/platform-iam/src/service.rs:539` 写死 `Vec::new()` |
| `GET /api/rbac/permissions` | 返回**注册表**（全体已登记权限点），不是「我拥有的」 |
| `RbacService::effective_permissions` | **有实现**（`service.rs:780`），但**无 HTTP 路由暴露** |

所以本需求必须新增一个后端端点。这不是「顺手扩围」—— **不做它，本需求的
`can()`、`<Can>`、路由守卫三条 AC 全部落空**。

---

## 2. 后端设计

### 2.1 `GET /api/rbac/me/permissions`

**路由**：`/api/rbac/me/permissions`，挂在 `platform_rbac::http::router` 上。

**豁免规则**：与 `/api/rbac/permissions` 注册表一样**豁免权限点要求**
（任何已认证用户可读），但**要求已认证**（未认证 → 401）。

> **为什么豁免**：这个接口的用途就是让前端判 `can()`。若它自己需要权限点，
> 就会出现「要先有权限才能问有没有权限」的循环 —— 用户第一次进页面就拿不到权限点，
> 只能靠登录响应（而登录响应里也是空的）。豁免是**唯一自洽**的选择。

**响应形状**：`{ "permissions": ["rbac:role:read", ...] }`

> **为什么是对象不是裸数组**：后续要加 `dataScope`（前端可能想按范围提示用户）
> 或 `registryVersion` 时不必改响应形状 —— 改形状是破坏性变更。
> 与契约 C9 的 `{source, value}` 做法同源。

**实现**：`RbacService::effective_permissions(ctx, user_id)` 已存在，直接复用。
它内部走 `expand_user_permissions`（展开通配），与 `RbacChecker` 的判定**同一套逻辑**
—— 保证「前端 `can()` 说有」与「后端 `require_permission` 放行」一致。

> ⚠️ **必须复用 `effective_permissions`，不得另写一套展开逻辑** ——
> 两套逻辑一旦分叉，就会出现「前端显示按钮、后端 403」的经典不一致。

### 2.2 登录响应填 `permissions`

`IamService::login` 的返回里，`permissions` 从 `Vec::new()` 改为**真实权限点集合**。

**问题**：`platform-iam` **不能**依赖 `platform-rbac`（依赖方向是
`platform-rbac → platform-iam`；反过来会成环，且违反 AGENTS.md 规则 2）。

**解法**：用**扩展点 trait** 注入 —— 在 `platform-core` 定义

```rust
pub trait PermissionProvider: Send + Sync + 'static {
    /// 当前用户的有效权限点集合（展开通配）。
    fn effective_permissions(&self, ctx: &RequestContext, user_id: &str)
        -> Pin<Box<dyn Future<Output = Result<Vec<String>, String>> + Send + '_>>;
}
```

`platform-iam` 只依赖 `platform-core` 的 trait（编译期不依赖 rbac），
`platform-rbac` 提供实现，`platform-server` 装配时注入。

> **为什么不走 `PermissionChecker` trait**：那个 trait 的方法是**同步**的
> （`has` / `scope` / `effective_scope` / `condition`），且语义是「判一个权限点」，
> 不是「列出全部」。复用它会得到 `Vec<PermissionCode>` 的逐个判定，
> 而权限点数量不定 —— 逐个 `has()` 要 N 次判定，且拿不到「全集」这个语义。

> **为什么 trait 放 `platform-core`**：契约「依赖单向 `平台 → trait ← 业务`」
> 要求 trait 定义在被依赖方。`platform-core` 是所有平台 crate 的公共底座，
> 放这里 `platform-iam` 与 `platform-rbac` 都能用，且**互不依赖**。

**降级**：未注入实现时（平台未启用 RBAC），`permissions` 为**空数组**，
登录**仍然成功** —— 不能因为权限服务没起就登不了录（fail closed 的是「判定」，
不是「登录」）。

### 2.3 一致性保证

登录响应与 `/api/rbac/me/permissions` 必须给出**相同**的答案（AC-S-065 步骤 2 要求逐项相等）。
两者都走 `effective_permissions` → 天然一致。

---

## 3. 前端包设计

### 3.1 包依赖方向（单向）

```
@base/theme   （无内部依赖）
@base/ui   →  @base/theme
@base/hooks   （无内部依赖）
@base/sdk     （无内部依赖）
@base/auth →  @base/sdk
```

**规则**：`@base/sdk` **不依赖** `@base/ui`（AC-S-075）。
理由：SDK 是纯数据层，引入 UI 会把组件代码拖进任何用 SDK 的产物 ——
这正是父 AC-016 的核心诉求。

### 3.2 `@base/sdk` —— API client

**核心接口**：

```ts
createApiClient({
  baseUrl: string,
  fetch?: typeof fetch,          // 可注入 → 单测不引 MSW
  getAccessToken?: () => string | undefined,
  refreshToken?: () => Promise<string | undefined>,
  tenantId?: string,
  spaceId?: string,
  onUnauthorized?: () => void,
})
```

**错误模型**（AC-S-069）：
- 后端错误体是 RFC 9457（`platform-core/src/problem.rs`），有稳定的 `code` 字段
- SDK 把响应解析成 `ApiError { code, status, title, detail }`
- **`code` 优先**：`INSUFFICIENT_PERMISSION` / `UNAUTHENTICATED` / `NOT_FOUND` …
- **不把原始响应体塞进错误信息**（可能含服务端内部细节）

**客户端合成码表**（D94 —— rev 2 固化，QA 挑刺 #19）：
后端码**原样透传**，SDK 只在「根本没有后端响应」时合成：

| 合成码 | 触发条件 |
|---|---|
| `client.NETWORK_ERROR` | fetch 抛错（网络层失败、连接被拒） |
| `client.INTERNAL_ERROR` | 收到响应但**无法解析为错误体**（非 JSON / 空体 / 截断） |

> **为什么带 `client.` 前缀**：与后端码表（`platform-core/src/problem.rs:31-38`、
> `platform-iam/src/error.rs:32-64`）**不冲突** —— 前端可以用
> `code.startsWith('client.')` 区分「客户端合成」与「后端给的码」。
> **为什么不合成 `UNAUTHENTICATED`**：刷新失败时后端给的是
> `TOKEN_EXPIRED` / `TOKEN_REVOKED` / `TOKEN_REUSE_DETECTED` ——
> 合成会丢掉「为什么被踢下线」的直接线索。

**token 刷新**（AC-S-070）：
- 401 → 调 refresh → 重放原请求
- **并发去重**：多个请求同时 401 时，**只发一次** refresh（共享一个 in-flight Promise）
  > **为什么必须去重**：REQ-007 的 refresh 是**轮换 + 重放检测** ——
  > 同一个 refresh token 用两次会被判为重放，**整个会话失效**。
  > 5 个并发请求 → 5 次 refresh → 用户被踢下线。这是**真实事故**，不是理论风险。
- 请求体**必须可重放**：`fetch` 的 `RequestInit.body` 若传的是 `ReadableStream`，
  重放时已失效 → 必须**先缓冲成字符串/字节**再发。
- 刷新失败 → 抛**后端返回的**码（D94：透传，不合成），**不无限重试**

**幂等键**（AC-S-071）：
- 写方法（`POST`/`PUT`/`DELETE`）自动生成 `idempotency-key`（UUID v4）
- **`GET`/`HEAD` 不带**
- **每次请求新键**，不复用
  > **为什么不能复用**：后端幂等中间件（`platform-iam/src/http.rs:295`）按
  > `(tenant_id, idempotency_key)` 去重。同一个键的第二次请求会**直接返回首次结果**，
  > 新数据**不落库** —— 表现为「点了保存没反应」。这是个**静默丢数据**的坑。
- 键**不得由路径派生**、**不得跨路径复用**（后端去重键**不含** method/path）
- ★ **401 刷新后的重放请求不带幂等键**（D92）
  > 401 意味着「请求**没有执行**」；没执行的请求不该留幂等声明。
  > 沿用旧键 → 后端命中 `status=0` 的已声明键 → **返回 409 CONFLICT**，
  > 表现为「点保存没反应」。带新键 → 表里留两个半截键。
  > **唯一干净的解法是重放时不注入该头**。

**上下文头**（AC-S-072）：
- `x-tenant-id` / `x-space-id`（后端约定，契约 C3）
- **未配置时不带头**（空串 ≠ 不带）
  > 后端在请求头缺租户时**从令牌推断**租户
  > （`platform-rbac/src/http.rs:191-194`）—— 所以空串反而更糟。
- 注入于**所有**请求（**含 login / refresh**）——
  后端 login / refresh 都要求租户上下文，缺则 401 `TENANT_CONTEXT_MISSING`

#### 3.2.1 ★ SDK 公开 API 钉死（rev 2 固化 —— QA 挑刺 #32）

用例要能在 API 定型前写死，就必须把签名写死。

```ts
/** API 错误。`code` 是稳定错误码（后端码透传 / `client.*` 合成码）。 */
export interface ApiError extends Error {
  readonly code: string;
  readonly status: number;
  readonly title?: string;
  readonly detail?: string;
}

/** 请求选项。 */
export interface RequestOptions {
  readonly method?: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH' | 'HEAD';
  readonly path: string;
  readonly body?: unknown;              // 会被 JSON 序列化
  readonly query?: Record<string, string | number | boolean | undefined>;
  readonly headers?: Record<string, string>;
  readonly idempotent?: boolean;        // 默认：写方法 true / 读方法 false
}

/** API 客户端。 */
export interface ApiClient {
  readonly baseUrl: string;
  /** 发起请求；失败抛 `ApiError`。 */
  request<T = unknown>(options: RequestOptions): Promise<T>;
  /** GET 快捷方式。 */
  get<T = unknown>(path: string, options?: Omit<RequestOptions, 'path' | 'method'>): Promise<T>;
  /** POST 快捷方式。 */
  post<T = unknown>(path: string, body?: unknown, options?: Omit<RequestOptions, 'path' | 'method'>): Promise<T>;
  /** PUT 快捷方式。 */
  put<T = unknown>(path: string, body?: unknown, options?: Omit<RequestOptions, 'path' | 'method'>): Promise<T>;
  /** DELETE 快捷方式。 */
  delete<T = unknown>(path: string, options?: Omit<RequestOptions, 'path' | 'method'>): Promise<T>;
  /** 更新租户 / 空间上下文。 */
  setContext(context: { tenantId?: string; spaceId?: string }): void;
}

export function createApiClient(options: ApiClientOptions): ApiClient;
```

**约定**：
- 成功响应：2xx → 按 `content-type` 解析 JSON；**空体** → 返回 `undefined`
- 失败响应：非 2xx → 抛 `ApiError`
- **401** → 触发刷新 → 重放（最多**一次**）；刷新仍失败 → 抛 `ApiError`
- `query` 中值为 `undefined` 的键**不拼进** query string

### 3.3 `@base/auth` —— 登录态与权限

**核心接口**：

```ts
createAuthStore(client) → {
  state: { status, user, permissions, tenantId },
  login(username, password): Promise<void>,
  logout(): Promise<void>,
  can(permission: string): boolean,
  guard(permission: string): RouteGuard,   // 路由守卫
}
```

**`can()` 语义**（AC-S-066）：
- 权限点集合**为空** → 一律 `false`（fail closed）
- 集合里**没有**该权限点 → `false`
- **未登记**的权限点 → `false`（不是 true、不是抛错）

**`<Can>` 组件**（AC-S-067）：
- **组件落在 `apps/shell`**（rev 2 裁定 #7 —— 包内构建链为 `tsc` only，`.svelte` 进不了产物）
- `@base/auth` 提供**框架无关**的判定：`can()` + 一个条件渲染辅助
- 语义：有权限 → 渲染 children；无权限 → **不渲染** children
  （不是 `display:none` —— 内容仍在 DOM 里）；可选 `fallback`

**路由守卫**（AC-S-068）：
- `@base/auth` 提供**框架无关的纯判定**：

```ts
/** 守卫判定结果。 */
export interface GuardResult {
  /** 是否放行。 */
  readonly allowed: boolean;
  /** 拒绝时的落点（403 页路径）；放行时为 `undefined`。 */
  readonly redirectTo?: string;
}

/** 未登录时的落点（登录页）。 */
export interface GuardOptions {
  /** 未认证时跳哪（默认 `/login`）。 */
  readonly loginPath?: string;
  /** 拒绝时跳哪（默认 `/403`）。 */
  readonly forbiddenPath?: string;
}

export function guard(
  state: AuthState,
  permission: string,
  options?: GuardOptions,
): GuardResult;
```

- **未登录** → `redirectTo = loginPath`（**不是** 403 —— 未认证与无权限要分清）
- 已登录但**无权限** → `redirectTo = forbiddenPath`
- 已登录且**有权限** → `{ allowed: true }`

> **路由能力从哪来**：本仓前端**没有**路由库依赖（无 `svelte-routing` / `@sveltejs/kit`）。
> 本期用 **History API + 极简路由**实现，**不新增 npm 依赖**。
> 服务端已支持 SPA fallback（`crates/platform-server/src/static_files.rs:18`：
> 路径无扩展名 → 回落 `index.html`），所以深链接能正常到达前端路由。
>
> **「403 页不发起目标接口的请求」由谁保证**：守卫在**导航前**完成判定，
> 拦截时**不调用**目标页面的数据加载函数 → 该接口的请求计数为 **0**。
> 这是**应用侧**（`apps/shell`）的责任 —— 路由元数据接到 `guard()` 上，
> 受保护页面的加载函数**在守卫放行后才被调用**。

### 3.4 `@base/theme` —— 设计令牌

**令牌集**（CSS 变量）：见 §3.4.1 令牌契约表（**精确集合**）。

**输出方式**：应用令牌 → 写根元素的 `style` 属性（inline style 优先级高于
样式表，切换即时生效，无需重渲染）。

**暗色模式**（AC-S-073）：
- 切换 → 根元素 `data-theme="dark"` + `color-scheme: dark` + 换一组颜色变量
- 断言「**等于**暗色期望值」，不是「不等于亮色值」（D83）

**品牌主色**（AC-S-074）：
- 主色来自 REQ-010 的 `GET /api/config/branding`
- **只接受合法十六进制**（`#rgb` / `#rrggbb`），非法 → 保持中性兜底
- 无品牌 / 品牌非法时，**主题包自己**把 `--base-primary` 写成 `currentColor`
  （**不依赖宿主样式表** —— 否则宿主没有 `:root{--base-primary:currentColor}` 时
  读到的是**空串**而不是 `currentColor`）
- 源码里**不许**有品牌颜色字面量

#### 3.4.1 ★ 令牌契约表（rev 2 固化 —— QA 挑刺 #5 / #6 / #26）

**模式无关**变量（亮/暗**取值相同**，不参与「变了」的判定）：

| CSS 变量 | 取值 | 来源 |
|---|---|---|
| `--base-radius` | `4`（**无单位**，与后端 `theme.radius` 的 number 一致） | `theme.radius` |
| `--base-font-family` | `system-ui, -apple-system, "Segoe UI", sans-serif` | `theme.fontStack`（后端是 **array**，前端 join(', ')） |

**模式相关**变量（亮/暗**取值不同**）：

| CSS 变量 | 亮色 | 暗色 |
|---|---|---|
| `--base-primary` | `#1677ff` | `#4096ff` |
| `--base-primary-hover` | `#4096ff` | `#69b1ff` |
| `--base-danger` | `#d4380d` | `#ff7875` |
| `--base-text` | `#1f1f1f` | `#e6e6e6` |
| `--base-text-secondary` | `#8c8c8c` | `#a6a6a6` |
| `--base-bg` | `#ffffff` | `#141414` |
| `--base-border` | `#d9d9d9` | `#303030` |

**唯一允许出现颜色字面量的文件**（D95）：
`packages/theme/src/tokens.ts` —— **只此一个文件**。
`packages/ui/src/**` 与其余位置**一律禁止**颜色字面量。

**扫描规则**（AC-S-073 步骤 4，参数写死）：
- 文件集：本期五个包的 `src/**/*.{ts,svelte.ts}`
- 正则：`#[0-9a-fA-F]{3,8}\b` 与 `rgb(` / `hsl(` / `oklch(`
- 排除集：`test/`、`*.test.ts`、`packages/theme/src/tokens.ts`

**落点单一化**（QA 挑刺 #27）：`apps/shell/src/lib/branding.ts`
（REQ-010 已实现的品牌应用）**改为委托 `@base/theme`** ——
两份实现会让 REQ-010 的既有 E2E（AC-S-055）因漂移而红。

### 3.5 `@base/ui` —— 基础组件

本期组件：**Button、Input、Card、Table、Dialog**（+ token 消费）。

**硬规则**：组件**只消费 CSS 变量**，不硬编码颜色字面量（AC-S-073 步骤 4 / D95）。

> **为什么不做完整组件库**：shadcn-svelte + 完整组件集的依赖树会吃掉磁盘预算；
> 且本期需求的价值在「权限闭环」，不在组件数量。

#### 3.5.1 ★ 组件的落地形态（rev 2 裁定 #7 —— 已改）

**裁定**：组件**落在 `apps/shell`**；`@base/ui` 提供**框架无关**的逻辑与样式契约。

**事实依据**（BA 已实测）：
- `packages/*/package.json` 的 build = `tsc -p tsconfig.json`，
  `tsconfig.json` 的 `include` 只有 `["src/**/*.ts"]` → **`.svelte` 文件不会被编译进产物**
- 仓里**没有** `@sveltejs/package`（`svelte-package`），引它=**新增依赖**（违反本期零新依赖约束）
- `apps/shell` **已支持** `.svelte`：`tsconfig.json` 的 `include` 含 `src/**/*.svelte`，
  vite 已装 `@sveltejs/vite-plugin-svelte` 5.1.1
- BA 实测：`.svelte.ts` 能被 vitest + tsc 处理，但**纯 TS 函数无法渲染 DOM** ——
  Svelte 组件的渲染需要 `.svelte` 模板或编译期宏

**分工**：

| 位置 | 内容 |
|---|---|
| `packages/ui/src/**` | **逻辑与样式契约**：变体名、类名、props 类型、行为函数 |
| `packages/ui/src/*.css` 或样式常量 | 消费 CSS 变量的样式（**零颜色字面量**） |
| `apps/shell/src/components/*.svelte` | **Svelte 组件**（薄封装，调用包的逻辑） |
| `apps/shell/src/components/Can.svelte` | `<Can>` 组件（AC-S-067） |

> **为什么这样分**：组件库发逻辑，宿主应用发视图 —— 这是 Svelte 生态的**常规做法**。
> 要让包直接产出组件，得引 `svelte-package` + 改构建链 + 处理 `svelte` peer 依赖 ——
> 本期不值得。**零新依赖**是硬约束。

**配套要点**：
- `packages/ui` 需要 `svelte` 作为 **peerDependency**（若引用了 snippet 类型）
  与 **devDependency**（本地类型检查）
- `verbatimModuleSyntax: true`（`tsconfig.base.json`）→
  **类型导入必须写 `import type { ... }`**
- `svelte-check` 目前只在 `apps/shell` 跑 —— 包内类型检查靠 `pnpm -r check`（`tsc --noEmit`）

### 3.6 `@base/hooks` —— 通用 hooks

本期：**分页、表格状态、SSE/WebSocket、剪贴板**。

- 分页/表格状态：**纯状态容器**（不绑框架），Svelte 侧用薄封装
- SSE/WebSocket：**重连退避** + **关闭时不重连**（组件卸载后重连会泄漏）
- 剪贴板：不可用时**返回失败**（不静默吞掉 —— 用户会以为复制成功了）

---

## 4. 测试策略

### 4.1 前端单测（Vitest）

**不引 MSW**（磁盘预算）—— 用**可注入的 `fetch` 假件**：
```ts
const fakeFetch = async (req) => new Response(JSON.stringify(body), { status });
```
→ SDK 的 `fetch` 选项正好支持注入。**测试与生产调同一个函数**（D85）。

### 4.2 后端集成测试

复用现有 `crates/platform-rbac/tests/http_admin.rs` 的夹具模式。

### 4.3 E2E（Playwright）

- 复用 `scripts/e2e-server.sh`（同源托管 `apps/shell/dist`）
- 新增：登录 → 拿到权限点 → 无权限按钮不渲染 → 直接访问 URL 被拦到 403

### 4.4 本仓的「假绿」教训（D82/D83/D84/D89）

前端断言**必须断「等于期望值」**：
- 暗色模式 → 断**等于**暗色期望值，不是「不等于亮色值」
- 品牌主色 → 断**等于**接口返回的值
- 403 页 → 断**等于**403 页，不是「不等于目标页」

---

## 5. 风险

| # | 风险 | 缓解 |
|---|---|---|
| R1 | `platform-iam` 依赖方向 —— 加 RBAC 会成环 | 用 `platform-core` 的 trait 注入（§2.2） |
| R2 | refresh 轮换 + 并发重放 → 会话失效 | 并发去重（§3.2） |
| R3 | 请求体不可重放 → 重放发空 body | 发送前缓冲 body |
| R4 | 幂等键复用 → 静默丢数据 | 每次请求新键（§3.2） |
| R5 | 前端 `can()` 与后端判定分叉 | 两边共用 `effective_permissions`（§2.1） |
| R6 | 磁盘预算（20G，已用 4G） | 不引 MSW / Storybook / utoipa |
| R7 | `Cargo.lock` 依赖预算（295 包） | 不加新 Rust 依赖；`PermissionProvider` 用现有 trait 模式 |
| R8 | 低配机编译慢 | 复用 `code/target`；`jobs = 3` |
| R9 | 前端 401 重试放大 → 死循环 | 刷新失败即抛错，不重试（AC-S-070 步骤 3） |
| R10 | 主题令牌与 REQ-010 品牌接口口径不一致 | 主色单一来源 = 品牌接口；token 层不感知品牌 |

---

## 6. 不做（与 demand.md 一致）

OpenAPI 类型生成、Storybook、MSW、重组件（图表/富文本/树/穿梭/上传/编辑器）、
完整 i18n、元数据渲染器。
