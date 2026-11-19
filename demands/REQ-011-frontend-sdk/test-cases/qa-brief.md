# REQ-011「前端 SDK 骨架」—— QA 任务书摘要（给 Dev）

- **配套文件**: [index.md](index.md)（用例索引 + §8 挑刺裁定表，40 条问题）｜用例 JSON 5 个文件
- **基线**: `code/` @ `develop` @ `a3fae66`；验收基线 `acceptance.md` rev 1（**12 条 AC 全部需修订**，见 index.md §8.7）
- **一句话结论**: **按现稿不能开工**。先让 BA 出 `acceptance.md` rev 2（index.md §8.1 的 11 条高severity），再按本任务书实现。

---

## 1. 必测 AC 与判定要点

| AC | 必测用例 | 判定要点（一句话） | 最容易踩的坑 |
|---|---|---|---|
| **AC-S-064** | TC-011-001 ~ 005 | 响应体是**对象** `{permissions:[...]}`（不是裸数组）；集合**整体相等**；无令牌 → 401 `UNAUTHENTICATED` | ① 拿**内置 `platform-admin`** 当夹具 → 权限集是注册表全集，「不含 `rbac:role:write`」直接不成立；② 忘了通配展开 —— `rbac:role:*` 会展开出 `rbac:role:write`；③ 新接口**不要新增权限点** → 会触发 D27 启动对账（进程起不来） |
| **AC-S-065** | TC-011-006 ~ 010 | 登录响应 `permissions` 非空且与 `/me/permissions` **集合相等**；无角色用户 → `[]`（不是 `null`、不是缺字段） | ① 两个入口是**两次 DB 读**，测试期间不能改授权，否则「逐项相等」偶发失败；② 忘了 `IamService` 里 `permissions: Vec::new()` 的两处硬编码（`service.rs:539` / `:665`）都要改；③ `platform-iam` **不能**依赖 `platform-rbac`（会成环）→ 走 `platform-core` 的 trait 注入，且 `build_iam_state` 在 `build_rbac_state` **之前**，注入要用后绑定槽位 |
| **AC-S-066** | TC-011-011 ~ 015 | `can()` 纯集合判定；空集合 → **false**（fail closed）；与后端判定一致靠**金标准夹具** | ① 「未登记权限点 → false」是集合判定的自然结果，别为了它去查注册表；② 与后端比一致**不能**在单测里调 `require_permission` —— 它是 axum Layer，只能靠**真实路由探针**；③ 别让 `can()` 在权限集合**加载中**时返回 true |
| **AC-S-067** | TC-011-016 ~ 018 | 有权限 → 子内容在 DOM；无权限 → **`innerHTML === ''`**（不是 `display:none`） | ⚠️ **本条有个硬伤**（index.md #7）：现有构建链是 `tsc` only（`packages/*/package.json` 的 `build` + `include: ["src/**/*.ts"]`），**`.svelte` 组件不会被编译进 `dist/`**。`<Can>` 要么改成框架无关的渲染函数，要么加 `svelte-package`，要么放 `apps/shell` —— **先跟 BA 裁定** |
| **AC-S-068** | TC-011-019 ~ 022 | 包级：`guard()` 返回 `{allowed:false, redirectTo:'/403'}`，且**深链与程序化跳转判定一致**；端到端：落 403 且**不发起**目标接口请求 | ⚠️ **本条没有实现主体**（index.md #8）：`apps/shell` 无路由、无登录页、无 403 页。另外**别漏了「未登录 → 登录页」**这条（别让未登录用户看到 403） |
| **AC-S-069** | TC-011-023 ~ 026 | `error.code` **精确等于**响应体的 `code`（不是 `title`、不是 `detail`）；非 JSON → `INTERNAL_ERROR`；网络失败 → `NETWORK_ERROR`；**错误对象不含原始响应体** | ① 后端真实 403 的 `title` 是「**权限不足**」，不是「未认证」（AC 举例写错了）；② `NETWORK_ERROR` 是**客户端合成码**，后端码表里没有；③ 「不塞原始响应体」要落到**具体字段**（`message`/`detail`/`title`/整体序列化），别塞进 `error.response` 之类的新字段 |
| **AC-S-070** | TC-011-027 ~ 030 | 401 → 刷新 → 重放 → 调用方拿到 **200**；并发只刷新 **1 次**；刷新失败即抛错、**不重试**；重放**请求体逐字节相同** | ⚠️ **两个坑**：① 「5 个并发」**必须**在 fake fetch 层用受控 pending Promise 构造 —— 对真实后端并发打 refresh 会触发 `TokenReuseDetected` 并**作废整个会话 family**；② 「过期的 access token」别靠等 15 分钟（默认 TTL 900s），用测试密钥签一个 `exp` 在过去的 JWT |
| **AC-S-071** | TC-011-031 ~ 034 | 写方法带 `idempotency-key`（**UUID v4**）；GET/HEAD **不带**；两次写**键不同**；**重放用新键** | ⚠️ **本条是全需求最危险的组合点**（index.md #1）：首次请求 401 时幂等键已被 claim 且 `status=0`（后端只在**成功**时 complete），重放若沿用旧键 → **409 CONFLICT**，用户点保存没反应。**重放必须换新键**。另：后端去重键是 `(tenant_id, idempotency_key)`，**不含 path** → 键不能由路径派生 |
| **AC-S-072** | TC-011-035 ~ 037 | 配置了才带 `x-tenant-id`（**未配置时头不存在**，不是空串）；`x-space-id` 同理 | 「不带」写成 `headers.get(...) === null`，别写成「不等于某个值」。另外：**登录 / 刷新请求也要带租户头** —— 后端 `login`/`refresh` 缺租户上下文直接 401 |
| **AC-S-073** | TC-011-038 ~ 040、043 ~ 044 | 根元素上有**约定的 9 个** CSS 变量；暗色 = **等于**暗色期望值（不是「不等于亮色值」）；`@base/ui` 源码**零**颜色字面量 | ⚠️ **两条硬伤**（index.md #5 / #6）：① AC 没给暗色期望值 —— 本任务书 §5 给了 **QA 提案**，等 BA 固化；② 令牌体系必然要颜色字面量，`@base/theme` 的字面量必须**只允许**出现在一个指定的令牌定义文件里（路径待 BA 指定），否则「源码无颜色字面量」与「令牌体系」互斥 |
| **AC-S-074** | TC-011-041 ~ 042 | `--base-primary` **等于**接口返回值；非法主色 → **等于** `currentColor` | ① `currentColor` 兜底要**由主题包自己写**到根元素上，不能指望宿主样式表；② 扫描「无品牌色字面量」时**必须排除 `test/` 与 `*.test.ts`** —— 否则测试自己的 `#0055aa` 期望值就违规；③ REQ-010 已在 `apps/shell` 实现了同一件事（`app.css:4` + `branding.ts:26,60-64`），**必须让 shell 委托 `@base/theme`**，别留两份实现 |
| **AC-S-075** | TC-011-045 ~ 048 | 依赖方向**双向**断言（禁止项 + 规定项）；dist 只 import 自己的依赖；sdk 产物无组件代码；ui 不依赖 hooks 且 Table 只吃 props | ⚠️ AC 只写「不依赖」→ **今天就假绿**（7 个包的 `package.json` 都没有 `dependencies`）。另外「五个包」与 `packages/*` 通配不一致（实际 7 个包，`i18n`/`schema` 属 REQ-021/REQ-012）；`Table` 与 `hooks` 的分页/表格状态要有**组合契约**（props 传参，不引 hooks） |

---

## 2. 三个「单测各自测不出来」的组合缺陷 —— 必须专门写组合用例

| 组合 | 现象 | 守门用例 |
|---|---|---|
| **AC-S-070 × AC-S-071** | 刷新后重放沿用旧幂等键 → 后端 **409 CONFLICT**，保存静默失败 | **TC-011-034** |
| **AC-S-065 × AC-S-064** | 两个入口在不同时刻读库；中间改授权 → 「逐项相等」偶发红 | **TC-011-008**（前置：两次调用之间状态不变） |
| **AC-S-073 × AC-S-074** | 品牌非法时若主题包不写兜底 → `--base-primary` 是**空串**而不是 `currentColor` | **TC-011-042** |

---

## 3. 用例写死的 SDK 接口假设（**若 design 改了形状，本用例集需回填**）

```ts
// @base/sdk（design §3.2 的参数 + QA 补的请求方法形状）
const client = createApiClient({
  baseUrl: string,
  fetch?: typeof fetch,                 // 可注入 → 单测不用 MSW
  getAccessToken?: () => string | undefined,
  refreshToken?: () => Promise<string | undefined>,
  tenantId?: string,
  spaceId?: string,
  onUnauthorized?: () => void,
});
await client.request({ method, path, body?, headers? });
//   → resolve：解析后的 JSON；reject：ApiError { code: string; status: number; title?: string; detail?: string }

// @base/auth（design §3.3）
const auth = createAuthStore(client);
auth.state.permissions: readonly string[];
auth.can(permission: string): boolean;
auth.guard(permission: string): { allowed: boolean; redirectTo?: string };

// @base/theme（QA 提案，BA 固化）
applyTheme(tokens, { target }): void;
setThemeMode('light' | 'dark', { target }): void;
applyBranding(branding, { target }): void;
```

> `guard()` 的返回形状、`request()` 的签名与异常类型在 `design-summary.md` 里**没有钉死**（index.md #32）。上面的形状是本用例集的**假设**，一旦 design 定稿，只需回填 §5 与各用例的 `steps` 里的调用名。

---

## 4. 用例落点建议

| 用例组 | 建议文件 | 运行方式 |
|---|---|---|
| TC-011-001 ~ 010 | `crates/platform-rbac/tests/http_me_permissions.rs`（沿用 `tests/common/mod.rs` 的夹具模式） | `cargo test -p platform-rbac` |
| TC-011-011 ~ 022 | `packages/auth/test/*.test.ts` | `pnpm --filter @base/auth test`（Vitest） |
| TC-011-023 ~ 037 | `packages/sdk/test/*.test.ts` | `pnpm --filter @base/sdk test` |
| TC-011-038 ~ 044 | `packages/theme/test/*.test.ts` | `pnpm --filter @base/theme test` |
| TC-011-045 ~ 048 | `packages/sdk/test/packaging.test.ts`（或独立脚本） | 需先 `pnpm -r build` |
| TC-011-021 / 022 | `e2e/` —— **待 BA 裁定路由与登录页归属后再补** | Playwright |

**单测不引 MSW**（磁盘预算，见 design §4.1 / R6）：用注入式 fake fetch：

```ts
const fakeFetch = async (req: Request) => new Response(JSON.stringify(body), { status });
```

---

## 5. 令牌契约表（**QA 提案，待 BA 固化**）

index.md #5 指出 AC-S-073 步骤 2 要求「等于暗色期望值」却**没给期望值**。以下是 QA 提案的数值（依据 design §3.4 的令牌集，并沿用 `apps/shell/src/app.css` 现有的 `#d4380d` / `#8c8c8c`）：

| CSS 变量 | 亮色 | 暗色 | 模式相关 |
|---|---|---|---|
| `--base-primary` | `currentColor` | `currentColor` | 品牌有效时被覆盖 |
| `--base-primary-hover` | `currentColor` | `currentColor` | 品牌有效时被覆盖 |
| `--base-danger` | `#d4380d` | `#ff7875` | ✅ |
| `--base-text` | `#1f1f1f` | `#e6e6e6` | ✅ |
| `--base-text-secondary` | `#595959` | `#a6a6a6` | ✅ |
| `--base-bg` | `#ffffff` | `#141414` | ✅ |
| `--base-border` | `#d9d9d9` | `#303030` | ✅ |
| `--base-radius` | `4px` | `4px` | ❌ 模式无关 |
| `--base-font-family` | `system-ui, -apple-system, "Segoe UI", sans-serif` | 同左 | ❌ 模式无关 |

配套规则：

1. 变量名集合**恰好 9 个**，不多不少（对应 TC-011-038 的精确集合断言）。
2. `--base-radius` 带 `px` 单位 —— 后端 `theme.radius` 是 **number**（默认 4），映射在主题包内完成。
3. `--base-font-family` 对应后端 `theme.fontStack`（**array**，默认 `["system-ui"]`）→ 映射关系要写进 design（AC 的变量名与后端键名**不同名**）。
4. 暗色模式同时切 `data-theme="dark"` 与根元素 `color-scheme: dark`。
5. `--base-primary` 的中性兜底由**主题包自己**写 `currentColor`（不依赖宿主样式表）。

---

## 6. 本任务书的阻塞项（需要 BA / 人工回答）

| # | 问题 | 影响 | 需要谁回答 |
|---|---|---|---|
| B1 | `<Can>` 的落点：框架无关函数 / 给 auth 包加 svelte 构建 / 放应用侧（index.md #7） | AC-S-067 能否落地 | BA + Dev |
| B2 | 路由守卫的落点：把「路由 + 登录页 + 403 页」纳入 REQ-011，还是把端到端移到 REQ-012/REQ-020（index.md #8） | AC-S-068 + 2 条 e2e 用例 | BA |
| B3 | 亮/暗令牌契约表（§5）是否采纳 | TC-011-038 ~ 040 的期望值 | BA |
| B4 | `@base/theme` 里**唯一**允许存放颜色字面量的文件路径（index.md #6） | TC-011-044 的期望值 | BA + Dev |
| B5 | 刷新失败时 SDK 的 `code` 取合成 `UNAUTHENTICATED` 还是透传后端 `TOKEN_*`（index.md #4） | TC-011-029 的期望值 | BA |
| B6 | SDK 的 `request()` / `guard()` 公开 API 形状（index.md #32） | 全部前端用例的调用名 | Dev 提案 → BA 确认 |
| B7 | 权限矩阵金标准夹具的格式与生成方式（index.md #11） | TC-011-015 | Dev |

---

## 7. QA 的一句话提醒

**不要把「按现稿实现」当成「按 AC 实现」。** 本用例集的每条 `notes` 都标明了它守的是**现稿**还是 **index.md §8 的修订稿**；AC 修订后按 `notes` 回填期望值即可，**不需要重写用例**。但 §6 的 B1~B7 没有答案之前，**AC-S-067 / AC-S-068 这两条的用例没法真正跑起来** —— 这不是测试的问题，是需求主体的问题。
