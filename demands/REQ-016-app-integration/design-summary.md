# 设计摘要：应用接入与应用注册表（REQ-016）

- **需求编号**: REQ-016
- **状态**: **rev 2**（2026-11-21 —— QA 挑刺 39 条裁决落地：**D276~D285**）
- **配套**: [demand.md](demand.md) / [acceptance.md](acceptance.md) rev 4 / [decisions.md](decisions.md) rev 2
- **基线代码**: `develop` @ `aa5b305`（REQ-022 已合并）

> ⚠️ 本文件里的每个 `file:line` 都是在 `aa5b305` 上**实测**的（dev-guide §9.11）。
> Dev 若发现行号漂移，**以符号名为准**，并在报告里注明。

---

## 1. 架构与依赖

```
                    ┌──────────────────────────────────────────────┐
   浏览器 ────────▶ │  apps/shell（SPA）                            │
                    │  /workbench（工作台）· 导航来自后端下发         │
                    └───────────────┬──────────────────────────────┘
                                    │ Bearer（platform-iam 登录令牌）
                                    ▼
   ┌────────────────────────────────────────────────────────────────┐
   │ platform-server（装配层）                                       │
   │  · nest("/api/apps", platform_app::http::router(state))        │
   │  · 注入 PermissionChecker / PermissionRefresher / AuditSink     │
   └───────────────┬──────────────────────────┬─────────────────────┘
                   ▼                          ▼
   ┌───────────────────────────┐   ┌──────────────────────────┐
   │ platform-app（**新 crate**）│   │ platform-oauth（既有）    │
   │  · 2 张表 + 11 个端点      │──▶│  · platform_oauth_client  │
   │  · 3 个权限点              │只读│    （**引用**，不复制）     │
   │  · 3 个审计事件码          │   │                          │
   └───────────┬───────────────┘   └──────────────────────────┘
               ▼
        platform-core / platform-db
```

- **依赖方向单向，无环**：`platform-app → {platform-core, platform-db, platform-oauth}`
- `platform-app` **不依赖** `platform-rbac`（用 `platform-core` 的
  `PermissionChecker` / `PermissionRefresher` trait，装配层注入）
- **新增第三方依赖：0 个**

---

## 2. 数据模型（2 张表）

### 2.1 `platform_app` —— 应用注册表（**租户级**，启用 RLS）

| 列 | 类型 | 约束 |
|---|---|---|
| `id` | `text` PK | |
| `tenant_id` | `text` | NOT NULL |
| `client_id` | `text` | NOT NULL, **FK → platform_oauth_client(client_id) ON DELETE CASCADE** |
| `display_name` | `text` | NOT NULL |
| `description` | `text` | NULL |
| `category` | `text` | NOT NULL DEFAULT `'general'` |
| `icon_url` | `text` | NULL |
| `launch_url` | `text` | NULL |
| `sort_order` | `int4` | NOT NULL DEFAULT `0` |
| `status` | `text` | NOT NULL DEFAULT `'active'`（`active` / `hidden`） |
| `created_at` | `timestamptz` | NOT NULL |
| `created_by` | `text` | NOT NULL |

**⚠️ 恰好 12 列**（以 D265 的表为准）。

- **UNIQUE (tenant_id, client_id)** —— 同租户不能重复接入同一 client
- **UNIQUE (tenant_id, display_name)** —— 工作台上两个同名应用没法区分
- **索引**：`(tenant_id, status, category, sort_order)`（工作台查询走这条）
- **⚠️ 不含**任何 client 的敏感字段（secret / redirect_uris / allowed_* / logout URI）

### 2.2 `platform_app_access` —— 访问记录（**租户级**，启用 RLS）

| 列 | 类型 | 说明 |
|---|---|---|
| `tenant_id` | `text` | |
| `user_id` | `text` | |
| `client_id` | `text` | **不是 app_id**（D268 的理由：app 行会被级联删） |
| `last_accessed_at` | `timestamptz` | |
| `access_count` | `int8` | |

- **PK (tenant_id, user_id, client_id)**
- **索引**：`(tenant_id, user_id, last_accessed_at DESC)`（最近访问）
  + `(tenant_id, user_id, access_count DESC)`（常用）

### 2.3 RLS

`crates/platform-app/manifest.json` 的 `tenantScoped` 段声明这 2 张表：

```json
"tenantScoped": {
  "tables": ["platform_app", "platform_app_access"],
  "column": "tenant_id"
}
```

→ policy 由 `cargo xtask tenant-policies` **生成**（**不手写**，D227 同款），
命名 `<table>_tenant_isolation`（`crates/platform-db/src/rls.rs:36`）。

---

## 3. 端点清单（**11 个**，一律 Problem Details）

| 方法 | 路径 | 权限点 / 认证 | 作用 |
|---|---|---|---|
| `GET` | `/api/apps` | `app:registry:read` | 列出应用（管理视角） |
| `POST` | `/api/apps` | `app:registry:write` | 注册应用 → **201** |
| `GET` | `/api/apps/{appId}` | `app:registry:read` | 读单个 |
| `PATCH` | `/api/apps/{appId}` | `app:registry:write` | 修改 |
| `DELETE` | `/api/apps/{appId}` | `app:registry:delete` | 删除 |
| `GET` | `/api/apps/workspace` | **登录即可**（工作台） | 按 category 分组 + 确定性排序 |
| `GET` | `/api/apps/recent` | **登录即可** | 最近访问 |
| `GET` | `/api/apps/frequent` | **登录即可** | 常用应用 |
| `POST` | `/api/apps/{appId}/access` | **登录即可** | 记一次访问（UPSERT） |
| `GET` | `/api/apps/navigation` | **登录即可** | **导航项**（module + app 合并，按权限点过滤） |
| `DELETE` | `/api/apps/{appId}/launch-url` | `app:registry:write` | **把 `launch_url` 置 NULL** → **204**（D283 / S-08） |

> **共 11 个端点**（本表是唯一权威）。
> ⚠️ `DELETE …/launch-url` 是 **D283 补的**：`Option<String>` 下 `PATCH` 的 `null` 与「省略键」
> 在 serde 里**等价** → 「显式置空」表达不出来，必须用专用端点。

### 3.1 跨栈 JSON 字段（**D258 同款，钉死**）

| 端点 | 请求体 | 响应体（200） |
|---|---|---|
| `POST /api/apps` | `{ clientId, displayName, description?, category?, iconUrl?, launchUrl?, sortOrder? }` | `{ appId, clientId, displayName, description, category, iconUrl, launchUrl, sortOrder, status, scopes: string[], createdAt, createdBy }` |
| `PATCH /api/apps/{id}` | 上述可选字段的任意子集 | 单个应用（同上形状） |
| `GET /api/apps` | — | `[{ …同上… }]`（**数组**，不是 `{ apps: [] }`） |
| `GET /api/apps/workspace` | — | `{ groups: [{ category, apps: [{ …同上… }] }] }` |
| `GET /api/apps/recent` / `frequent` | — | `[{ …同上… }]` |
| `POST /api/apps/{id}/access` | — | `{ status: "recorded", accessCount }` |
| `GET /api/apps/navigation` | — | `{ items: [NavigationItem] }` |

**`NavigationItem`** —— ⚠️ **按来源分别定义**（D284 / S-09）：

`MenuSpec` 只有 **4 个字段**（`label` / `icon` / `route` / `perm`，`manifest.rs:120-133`）
→ **没有** `labelKey` / `category` / `sortOrder` / `key`。

```jsonc
// source="module" —— 只能用 MenuSpec 真有的字段
{
  "source": "module",
  "label":  "<menus[].label，中文文案>",
  "icon":   "<menus[].icon | null>",
  "target": "<menus[].route>",
  "kind":   "internal",
  "requiredPermission": "<menus[].perm | null>"
}

// source="app" —— 用 platform_app 的字段
{
  "source": "app",
  "appId":  "<platform_app.id>",
  "label":  "<platform_app.display_name>",
  "icon":   "<platform_app.icon_url | null>",
  "target": "<platform_app.launch_url>",
  "kind":   "external",
  "requiredPermission": null
}
```

- ⚠️ **`module` 来源用 `label`（中文文案），不是 `labelKey`** —— 前端**直接渲染**、不过 `t()`。
  → **代价**：模块菜单**不随语言切换** → **TD-049**（要修得给 `MenuSpec` 加 `labelKey`，跨需求）。
- ⚠️ **`category` / `sortOrder` 只影响工作台**（`platform_app` 有这两列），**不影响导航排序**。
- ⚠️ **没有 `key` 字段**（原来写的「manifest 的 `menus[].label`」与 `label` 重复）。
- ⚠️ `kind` **必须**有：`internal` 走 SPA 路由（`router.navigate`）、`external` 整页跳（`location.href`）。

---

## 4. 关键流程

### 4.1 导航下发（AC-S-162）

```
GET /api/apps/navigation   (Bearer)
1. 读当前租户下 status='active' 的 platform_app（RLS 自动限定）
2. 对每个 app：launch_url 为 NULL → 跳过（没入口）
              join platform_oauth_client 取 allowed_scopes（**只读投影**）
              → 产出 source="app", kind="external"
3. 读全部 manifest 的 menus[]（平台模块自带的功能菜单）
   → 按 menus[].perm 过滤（调用者没那个权限点 → 跳过；perm 为 null → 总是下发）
   → 产出 source="module", kind="internal"
4. 合并 → 确定性排序（source 分组：module 在前？**不** —— 按 category 字典序，再 sortOrder，再 key）
   → 200 { items: [...] }
```

⚠️ **确定性排序**：`(category, sortOrder, key)` 字典序/数值序，**不依赖**数据库返回顺序。

### 4.2 工作台（AC-S-160）

```
GET /api/apps/workspace
1. 取 status='active' 的应用（RLS 限定）—— ⚠️ **不**按 `launch_url` 过滤（D279 / S-04：
   工作台是「接入了哪些应用」的总览，含尚未配入口的；只有**导航**才要求 `launch_url` 非空）
2. 应用 app.workspace.maxApps 上限（**超了截断**，按排序后的前 N）
3. 按 category 分组；组按 category 字典序；组内按 (sortOrder, displayName)
4. 200 { groups: [...] }
```

### 4.3 访问记录（AC-S-161）

```
POST /api/apps/{appId}/access
1. 校验 app 存在且属于当前租户（RLS 保证）且 active 且 launch_url 非空
2. INSERT INTO platform_app_access (tenant_id, user_id, client_id, last_accessed_at, access_count)
   VALUES (…, now(), 1)
   ON CONFLICT (tenant_id, user_id, client_id)
   DO UPDATE SET last_accessed_at = now(), access_count = platform_app_access.access_count + 1
3. 返回 { status: "recorded", accessCount }
```

⚠️ **`access_count` 必须累加**（不能用 `EXCLUDED.access_count` —— 那是 1，会把计数重置）。

### 4.4 前端运行时路由（AC-S-163 / D278 —— **rev 2 重写**）

🔴 **原设计（rev 1）是死路，已推翻**。四个实测事实：

| 事实 | 证据 |
|---|---|
| `RouteDefinition` **无 component 字段** | `apps/shell/src/lib/router.ts:41-62` |
| `createRouter` **捕获数组引用** | `router.ts:156` `const { routes } = options` |
| `Router` **无 `addRoute` / `setRoutes`** | `router.ts:113` |
| 页面渲染**逐路径硬编码** | `App.svelte:221-266`，**10 个** `{:else if}`，兜底 `<Home/>` |

→ 「运行时加路由就能渲染新页面」**做不到**；要真做到必须改 `router.ts`（违反 D270）。

**根因**：第三方应用是**独立站点**（`launch_url`），**没有本 SPA 的组件**
→ 天生**不该**进 SPA 路由表。

**rev 2 的交付（三条判据，见 AC-S-163）**：

```ts
// 新增纯函数（可单测）—— 只处理 internal
export function routesFromNavigation(items: readonly NavigationItem[]): readonly RouteDefinition[]
//   internal → [{ path: target, title: label }]
//   external → []（不进 SPA 路由表）
```

- **`external` 应用**：在工作台页面渲染成**整页跳转**的入口（`<a href>` / `location.href`），
  **不经过** SPA 路由。
- **`router.ts` 零改动**（D270，用 `git diff --stat <baseline>..HEAD` 钉死）。
- ❌ **明确不做**：「点导航项落到 SPA 内新页面」→ **TD-048**（要做需三件事：加 `component` 槽、
  `App.svelte` 改查表渲染、`Router` 加 `addRoute`）。

---

## 5. 错误码（**6 个**）

见 [acceptance.md](acceptance.md) §全局约定 的表。登记按 **D213 的 8 处**：
常量 / enum 变体 / `code()` / `status()` / `detail()` / `params()` / `ALL_CODES` / 测试的两个数组。

**外加**：
- `crates/platform-core/src/titles.rs` —— **+6** 条 ASCII 标题（**93 → 99**，已实测 93 是本轮基线）
- `packages/i18n/src/locales/{zh-CN,en-US}.json` —— 6 个 `error.APP_*`
- `crates/platform-server/tests/i18n_titles.rs` —— `all_codes()` 加一行 +
  `const REQ_016_NEW_CODES: usize = 6;`（断言 **99**）

> ⚠️ **rev 3 修正**：从 8 个删到 6 个（删 `APP_REGISTRY_MISMATCH` / `APP_INVALID_SCOPE_PROJECTION`）。
> **理由同 REQ-022 的 D226**：没有任何 AC 会触发它的码 = 死码。
> 详见 [acceptance.md](acceptance.md) §全局约定 的修正说明。

---

## 6. 权限点与 manifest

`crates/platform-app/manifest.json`（结构照抄 `crates/platform-oauth/manifest.json`）：

```json
{
  "id": "app",
  "name": "应用接入",
  "version": "0.1.0",
  "enabled": true,
  "permissions": [
    { "code": "app:registry:read",   "name": "查看应用注册表", "type": "menu" },
    { "code": "app:registry:write",  "name": "管理应用注册表", "type": "button" },
    { "code": "app:registry:delete", "name": "删除应用",       "type": "button" }
  ],
  "menus": [
    { "label": "工作台", "icon": "grid", "route": "/workbench", "perm": null }
  ],
  "tenantScoped": {
    "tables": ["platform_app", "platform_app_access"],
    "column": "tenant_id"
  },
  "dependencies": [],
  "config": {
    "app.recentCount": {
      "type": "number", "default": 6, "scope": "platform",
      "label": "最近访问与常用应用各取前 N 条"
    },
    "app.workspace.maxApps": {
      "type": "number", "default": 60, "scope": "platform",
      "label": "工作台最多展示的应用数"
    },
    "app.icon.allowHttp": {
      "type": "boolean", "default": false, "scope": "platform",
      "label": "图标 URL 是否允许 http（仅本地开发）"
    }
  }
}
```

- ✅ **已实测确认**：`MenuSpec.perm` 是 **`Option<String>`**
  （`crates/platform-core/src/extension/manifest.rs:120-132`，带
  `#[serde(default, skip_serializing_if = "Option::is_none")]`）
  → **省略 `perm` 键 = 登录即可见**。
  ⚠️ 该结构体有 `deny_unknown_fields` → **写 `"perm": null` 虽然能反序列化成 `None`，但请直接省略这个键**
  （少一种写法少一处风险）。
- ⚠️ 种子迁移的 `source` 必须逐字 = `'crates/platform-app/manifest.json'`。
- **C-5**：3 个权限点各要有字面量 `require_permission("<字面量>")` 调用点，**在 `crates/platform-app/src/` 内**
  → 真 C-5 命令 `cargo test -p platform-app --test contract_c5`。

---

## 7. 迁移（3 个 + down）

| 文件 | 内容 |
|---|---|
| `migrations/20261121000100_app_schema.{up,down}.sql` | 2 张表 + 索引 + FK CASCADE（**RLS 由 xtask 生成，不手写**） |
| `migrations/20261121000200_app_permission_seed.{up,down}.sql` | 3 个权限点，`source` 逐字 |
| `migrations/20261121000300_app_config_seed.{up,down}.sql` | 3 个配置键（**lowerCamelCase**） |

⚠️ 文件名编号接着 REQ-022 的 `20261120000700` 往后。

---

## 8. 前端交付

| 文件 | 动作 |
|---|---|
| `apps/shell/src/pages/Workbench.svelte` | **新建**：工作台（分组卡片 + 常用 + 最近） |
| `apps/shell/src/pages/AppRegistry.svelte` | **新建**：应用注册表管理（增删改查） |
| `apps/shell/src/lib/app-api.ts` | **新建**：API 层（纯函数 + 常量） |
| `apps/shell/src/lib/nav.ts` | **改**：`NAV_ITEMS` 硬编码 → 加 `routesFromNavigation`；**保留** `NAV_ITEMS` 作兜底 |
| `apps/shell/src/lib/routes.ts` | **改**：加 2 条路由（`/workbench` 已存在于守卫豁免？需确认；`/admin/apps`） |
| `apps/shell/src/App.svelte` | **改**：登录后拉导航 → `routes = [...既有, ...routesFromNavigation(...)]` |
| ⚠️ `apps/shell/src/lib/router.ts` | **禁止改**（D270） |
| `packages/i18n/**` | 8 个 `error.APP_*` + `app.*` 命名空间 |

---

## 9. 测试落点

| 层 | 位置 |
|---|---|
| 单元 | `crates/platform-app/src/**` 的 `mod tests` |
| 集成（真库 + 真 HTTP） | `crates/platform-app/tests/*.rs` |
| 契约 | `crates/platform-app/tests/contract_c5.rs`（**真 C-5**） |
| 跨栈 | `crates/platform-server/tests/i18n_titles.rs`（**99** —— 6 个 `APP_*` 码）+ `crates/platform-server/tests/app_real_stack.rs`（导航 / 工作台的真装配） |
| 前端单测 | `apps/shell/test/app-api.test.ts` / `app-navigation.test.ts` / `workbench.test.ts` |
| E2E | `e2e/apps.spec.ts` |

**夹具**（`scripts/seed-e2e.sh` 要加）：
- `u-perm-app@example.com`（tenant-a，持 `app:registry:read/write/delete`）
- `u-perm-audit@example.com`（**既有**，持 `audit:events:read` —— 用于 AC-S-162 的权限过滤对照）
- `u-perm-read@example.com`（**既有**，**不持** `audit:events:read` —— 对照组）
- tenant-a 的 5 个应用（A~E，AC-S-160 的分组/排序/隐藏判据）
- tenant-b 的 1 个应用（AC-S-157 的跨租户对照）

---

## 10. ⚠️ 命名禁区（C-5，D216）

`crates/platform-app/src/` 里**不得**出现这 8 个标识符：

```
check_permission   check_perm     has_permission   require_perm
authorize          ensure_permission  verify_permission  permission_check
```

- ⚠️ **`authorize` 在「应用接入」语境下极易踩**（比如 `authorize_app_access`）。
- 令牌/会话校验函数用别的名字：`authenticate_bearer` / `verify_session`。

---

## 11. ⚠️ TD-047 的教训（**本需求必须遵守**）

REQ-022 踩过、已记 **TD-047**：
> **每个自带认证中间件的模块都必须自己调 `PermissionRefresher::ensure_fresh`** ——
> 漏了就会「用户第一次打受保护端点必然 403、第二次就好」。

`platform-app` 有 5 个要权限点的端点 → **`authenticate` 中间件必须**：
1. 验登录令牌 → 建 `RequestContext`（tenant + user 来自令牌）；
2. **`refresher.ensure_fresh(&ctx).await`** ← **不能漏**；
3. 注入 `PermissionChecker` 到请求扩展；
4. 再 `next.run(request)`。

范式：`crates/platform-oauth/src/http.rs` 的 `authenticate`（REQ-022 修好后的版本）。

**⚠️ 建议本轮顺带把 TD-047 机制化**（BA 提议，Dev 评估成本）：
在 `platform-core` 提供一个组合中间件 `authenticate_layer(state)`，
把「验令牌 + 建 ctx + ensure_fresh + 注入 checker」封在一起 → 新模块**直接用**，不自己写。
若成本高，至少加一条**契约检查**：扫 `src/**/*.rs`，凡出现 `insert(checker)` 的文件，
同一函数体内必须出现 `ensure_fresh`（**带阳性对照**）。

---

## 12. 已识别的风险

| # | 风险 | 处置 |
|---|---|---|
| R1 | **client 级联删除会静默丢业务属性**（D264） | 记 TD；AC-S-158 路径 4 把它**测出来**（断言行数为 0） |
| R2 | **应用注册表租户级 vs client 平台级** —— 隔离层级不同 | 靠 FK 连接；AC-S-157 只验 `platform_app` 的 RLS；AC-S-158 验跨租户级联 |
| R3 | ~~`menus[].perm` 是否 `Option<String>`~~ | ✅ **已实测确认**（`manifest.rs:120-132`）→ **省略 `perm` 键**即可；不用 `null` |
| R4 | **前端 `createRoutes` 加参数**可能影响 REQ-011/013/014/022 的既有用例 | 加**可选**参数（默认 `undefined`）；棒2 必须跑前端全量回归 |
| R5 | `nav.ts` 的 `NAV_ITEMS` 若直接删掉，既有测试会红 | **保留**它作兜底，只**新增** `routesFromNavigation` |
| R6 | ~~`launch_url` 为空时工作台是否显示~~ | ✅ **已定（D279）**：工作台**显示**、导航**不显示** —— 两者不是一回事 |
