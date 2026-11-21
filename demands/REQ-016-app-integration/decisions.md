# 决策记录：应用接入与应用注册表（REQ-016）

- **需求编号**: REQ-016
- **父需求**: REQ-001
- **状态**: rev 1（2026-11-21 —— BA 规格阶段；本需求此前只有草稿，本轮补齐四件套）
- **基线代码**: `develop` @ `aa5b305`（REQ-022 已合并）
- **依赖**: REQ-007 ✅ / REQ-013 ✅ / **REQ-022 ✅**（三项前置全部就绪）

---

## 0. 需求来源披露（D122 / dev-guide §9.1）

**REQ-016 的需求来源是「用户原话」，不是 BA 自补。**

> 9. 预留应用接入能力
>
> —— 用户 2026-11-17 的 9 项需求清单（首个会话快照）

**与 REQ-022 同源**：同一句原话拆成两半 ——
REQ-022 承接 **IdP 侧**（协议级 client / 同意 / 撤销 / 令牌），
本需求承接 **应用侧**（业务级注册表 / 工作台 / 前端运行时路由）。

⚠️ **原话极简（9 个字）**，所以本需求的**具体形态全部是 BA 的设计**，
每一条都要有决策依据，不能凭空发明。见下。

---

## D262: 应用注册表 = **新表 `platform_app`**，**租户级**，**引用**（不复制）`platform_oauth_client`

### 问题

`demand.md` 写「应用注册表：… 归属租户」，但 REQ-022 的 `platform_oauth_client`
是**平台级**表（无 `tenant_id`、不启用 RLS，D219）。两者不在同一个隔离层级上。

### 实测证据（`develop` @ `aa5b305`）

```
platform_oauth_client 的 13 个列：
  client_id / client_secret_hash / client_type / name / redirect_uris /
  allowed_grant_types / allowed_scopes / backchannel_logout_uri /
  frontchannel_logout_uri / status / expires_at / created_at / created_by
→ 无 tenant_id（平台级）
```

### 决定

1. **新建 `platform_app`**，**租户级**（`tenant_id` + RLS，由 manifest 的 `tenantScoped` 段生成策略）；
2. **`client_id` 是外键**，引用 `platform_oauth_client(client_id)`；
3. **不复制任何敏感字段**（secret / 回调地址 / 授权类型 / scope 白名单 / 有效期）
   —— 那些一律**读 REQ-022 的 client 表**；
4. `platform_app` 只存**业务属性**：展示名 / 描述 / 分类 / 图标 / 排序 / 状态。

### 理由

| 方案 | 为什么不要 |
|---|---|
| 直接给 `platform_oauth_client` 加 `tenant_id` | 会**推翻 D219**（client 是「应用」不是「租户资产」）；且一个 client 可能被多个租户接入 |
| 复制 client 的字段到 `platform_app` | 两份真值会漂移（secret 改了一份、另一份没改）—— REQ-022 的 D200 已经明确「secret 只存摘要、只显示一次」 |
| 应用表平台级（所有租户共享一个市场） | 与 `demand.md` 的「归属租户」冲突；且租户 A 接入的应用不该出现在租户 B 的工作台 |

### 一个必须写清的语义区别

| 字段 | 在哪 | 含义 |
|---|---|---|
| `platform_oauth_client.name` | REQ-022 | **同意页**展示的应用名（协议级，跨租户同一份） |
| `platform_app.display_name` | 本需求 | **该租户工作台**展示的应用名（业务级，可按租户改） |

→ 允许两者不同。同意页永远用 client 的 name（用户同意的是「哪个应用」，不能按租户名骗人）。

---

## D263: 图标用 **URL**，不做文件上传

- `platform_app.icon_url` 存 URL（`text`，可空）。
- **理由**：文件存储与上传是 **REQ-018** 的范围（`demand.md` 明写「文件存储与导入导出」）。
  本需求引入上传会**越界**，且要处理存储、病毒扫描、配额、CDN —— 全是 REQ-018 的事。
- **校验**：注册/修改时校验 scheme（`https`；本地开发允许 `http`）—— 与 REQ-022 的
  `backchannel_logout_uri` 同款口径（那里已经这么做了）。

---

## D264: `client` 被删除时应用**级联删除**（`ON DELETE CASCADE`）

### 问题

`platform_app.client_id` 引用 `platform_oauth_client`。删除 client 时应用怎么办？

### 一个必须避开的坑（实测）

**REQ-022 的 AC-S-135 步骤 7 已经交付并验收了**：
> `DELETE /api/oauth/clients/{client_id}` 后 `GET` 返回 **404**。

如果本需求用 `ON DELETE RESTRICT`，那么**有应用引用的 client 就删不掉了**
→ 改动 REQ-022 已交付的行为 → **AC-S-135 会红**。

### 决定

**`ON DELETE CASCADE`** —— client 删除 → 应用自动下线（行被删）。

### 理由

1. **不改已交付行为**（D109）：REQ-022 的删除路径保持可用；
2. **语义正确**：应用依附于 client，client 没了应用无法工作（拿不到令牌）；
3. 代价是「业务属性静默丢失」→ 但应用本来就是 client 的**展示层投影**，不是独立资产。

### 必须测的判据

- 删除 client 后，`platform_app` 里对应行**也没了**（`count == 0`）；
- 删除**不报错**（不因外键受阻）；
- 其它租户引用**同一个** client 的应用行**也**被级联删（因为 client 是平台级、跨租户共享）。

---

## D265: `platform_app` 的列

| 列 | 类型 | 约束 | 说明 |
|---|---|---|---|
| `id` | `text` PK | | 应用 ID（平台生成） |
| `tenant_id` | `text` | NOT NULL | 租户（RLS 依据） |
| `client_id` | `text` | NOT NULL, **FK → platform_oauth_client ON DELETE CASCADE** | 引用 REQ-022 的 client |
| `display_name` | `text` | NOT NULL | 该租户工作台展示名（D262 的语义区别） |
| `description` | `text` | NULL | |
| `category` | `text` | NOT NULL, DEFAULT `'general'` | 分类（工作台分组用） |
| `icon_url` | `text` | NULL | D263 |
| `launch_url` | `text` | NULL | **应用入口 URL**（D269）；为空 → 不出现在导航/工作台 |
| `sort_order` | `int4` | NOT NULL, DEFAULT `0` | 升序排列 |
| `status` | `text` | NOT NULL, DEFAULT `'active'` | `active` / `hidden` |
| `created_at` | `timestamptz` | NOT NULL | |
| `created_by` | `text` | NOT NULL | 操作者 |

**共 12 列**（本表是唯一权威；design-summary §2.1 与 AC-S-156 步骤 7 都以本表为准）。

- **UNIQUE (tenant_id, client_id)** → 同一租户不能重复接入同一个 client（幂等 upsert）。
- **UNIQUE (tenant_id, display_name)** → 同一租户内展示名唯一（工作台上两个同名应用没法区分）。

---

## D266: 应用级权限声明 = **client.allowed_scopes 的只读投影**，**不得超出**

### 问题

`demand.md` 写「应用级权限声明（向用户展示这个应用要什么权限）」。

### 决定

`GET /api/apps` 的响应里带 `scopes: string[]` —— 值**直接取自**
`platform_oauth_client.allowed_scopes`（**只读**，应用表**不存** scope）。

### 理由

- 真正的授权范围在 REQ-022 的 `platform_oauth_grant.scopes`（用户实际授予的）；
- `client.allowed_scopes` 是「这个应用**最多**能要什么」；
- 应用表再存一份 = **第三份真值** → 必然漂移。

### 必须测的判据

应用表**没有** `scopes` / `allowed_scopes` 列（查 `information_schema.columns`）；
响应里的 `scopes` 与 client 表的值**逐字相等**。

---

## D267: 工作台清单 = 租户内 `status = 'active'` 的应用，按 `category` 分组 + `sort_order` 升序

`GET /api/apps/workspace` 响应：

```json
{
  "groups": [
    { "category": "general", "apps": [ { "appId": "...", "clientId": "...", ... } ] }
  ]
}
```

- 只含 `status = 'active'` 的应用（`hidden` 的不出现）。
- 组内按 `sort_order` 升序、`display_name` 升序（确定性排序，不依赖数据库返回顺序）。
- 组按 `category` 字典序升序（**确定性**，否则每次刷新顺序都变）。

---

## D268: 常用应用 / 最近访问 = 新表 `platform_app_access`（UPSERT）

| 列 | 类型 | 说明 |
|---|---|---|
| `tenant_id` | `text` | |
| `user_id` | `text` | |
| `client_id` | `text` | |
| `last_accessed_at` | `timestamptz` | |
| `access_count` | `int8` | |

- **PK (tenant_id, user_id, client_id)**
- 打开应用时 `POST /api/apps/{app_id}/access` → `INSERT … ON CONFLICT DO UPDATE SET
  last_accessed_at = now(), access_count = access_count + 1`
- **最近访问** = 按 `last_accessed_at DESC` 取前 N（N 由配置 `app.recentCount` 控制，默认 6）
- **常用应用** = 按 `access_count DESC, last_accessed_at DESC` 取前 N（**同样的 N**）

⚠️ **`client_id` 而不是 `app_id`**：因为 `platform_app` 行会被级联删除（D264），
而访问记录属于「用户用过哪个应用」这件事 —— 用 client_id 更稳定。
→ 但查询时**要 join `platform_app`**，只返回**当前租户仍在册且 active** 的应用。

---

## D269: 菜单下发 = **统一「导航项」概念**，两个来源合并

### 问题（实测确认的真缺口）

`manifest.json` 的 `menus[]` 段**有 schema 校验**（`crates/platform-core/src/extension/manifest.rs:120` `MenuSpec`），
但 **`develop` @ `aa5b305` 上没有任何 API 下发它**：

```bash
$ grep -rn "menus" crates/*/src/*.rs | grep -v manifest.rs
→ 只有测试夹具里的 "menus": []
```

而前端 `apps/shell/src/lib/nav.ts` 是**硬编码占位**，它的注释自己写着：
> 「默认菜单（**本期为占位数据，后续由应用注册表下发**）」

→ **本需求就是那个「后续」**。

### 决定：`GET /api/apps/navigation` 返回**两类来源的并集**

```json
{
  "items": [
    { "source": "module", "key": "audit", "labelKey": "nav.audit",
      "target": "/admin/audit", "kind": "internal", "icon": null,
      "category": "platform", "sortOrder": 0, "requiredPermission": "audit:events:read" },
    { "source": "app", "key": "app-01H...", "label": "第三方报销",
      "target": "https://expense.example/app", "kind": "external", "icon": "https://…",
      "category": "finance", "sortOrder": 10, "requiredPermission": null }
  ]
}
```

| 字段 | `module` 来源 | `app` 来源 |
|---|---|---|
| `source` | `"module"` | `"app"` |
| `key` | manifest 的 `menus[].label` | `platform_app.id` |
| `labelKey` | **翻译键**（`nav.*`） | — |
| `label` | — | `platform_app.display_name`（**已渲染的文案**，不是键） |
| `target` | `menus[].route`（**站内路径**） | `client` 的**外部 URL** |
| `kind` | `"internal"` | `"external"` |
| `requiredPermission` | `menus[].perm` | `null`（应用本身不是权限点） |

⚠️ **`kind` 是必须的**（`internal` / `external`）：两者的导航语义完全不同 ——
`internal` 走 SPA 路由（`router.navigate`），`external` 必须整页跳出去（`location.href`）。
前端若混用，内部路由会拿外部 URL 去 `match()` → 静默失败。

### **按权限点过滤**（必须）

`module` 来源的项：调用者**没有** `menus[].perm` 那个权限点 → **不下发**。
（否则低权限用户看到一堆点进去就是 403 的菜单。）

⚠️ `menus[].perm` 为 `null` 时（如「工作台」首页）→ **总是下发**。

### 应用的外部 URL 从哪来

`platform_oauth_client` **没有**「应用首页 URL」这一列（它只有 `redirect_uris` / 两个 logout URI）。

→ **两种选择**，BA 取后者：

| 方案 | 问题 |
|---|---|
| 复用 `redirect_uris[0]` | 那是**回调地址**，不是应用入口；很多应用的回调与入口不是一回事 |
| **新增 `platform_app.launch_url`**（`text`，可空） | 语义清晰；可由管理员在各租户自己填 |

→ **`platform_app.launch_url`**。为空时该应用**不出现在导航**里（没入口点了没意义）。

---

## D270: 前端路由**运行时扩展** —— 改 `createRoutes` 的**签名**，不改其语义

### 现状（实测）

```ts
export function createRoutes(
  client: ApiClient,
  onRoles?: (roles: unknown) => void,
  onAudit?: (result: AuditLoadResult) => void,
  onOAuthGrants?: (result: OAuthGrantsLoadResult) => void,
): readonly RouteDefinition[]     // ← 同步、编译期构造
```

ADR A4 要求：「应用清单在运行时可扩展（新增应用 = 加一条注册记录 + 一个路由，**不改底座代码**）」。

### 决定

1. `createRoutes` **新增第 5 个可选参数** `extraRoutes?: readonly RouteDefinition[]`
   —— 既有 4 个参数与返回类型**不变** → REQ-011/013/014/022 的既有用例**不受影响**；
2. **`internal` 的模块菜单**：由 `createRoutes` 在**构造时**从导航响应里挑 `kind === 'internal'` 的项，
   生成路由（`path` + `title`）。
   ⚠️ 但 `createRoutes` 是**同步**的、且 `routes` 在 `App.svelte` 里**只构造一次** → 导航是异步拿的
   → **不能**在 `createRoutes` 里等导航。
   → **改法**：`App.svelte` 拿到导航后，`routes = [...createRoutes(...), ...routesFromNavigation(nav)]`。
   新增纯函数 `routesFromNavigation(items)`（放 `lib/nav.ts` 或新文件）→ **可单测**。
3. **`external` 的应用**：**不进** SPA 路由表，由工作台页面的链接直接 `location.href`。

⚠️ **不动 `apps/shell/src/lib/router.ts`**（与 REQ-022 的 D221 同款理由：它是 REQ-011 已交付的路由核心）。

---

## D271: 权限点 **3 个**

| 权限点 | 用途 |
|---|---|
| `app:registry:read` | 查看应用注册表（管理视角） |
| `app:registry:write` | 注册 / 修改应用 |
| `app:registry:delete` | 删除应用 |

- **工作台 / 导航不进权限点**：它们是**登录用户都能看**的（导航项已按权限点过滤）。
  → 若给它们加权限点，普通用户就打不开工作台了 —— 与「统一入口」的目标冲突。
- **C-5 合规**：3 个权限点各要有 `require_permission("<字面量>")` 调用点，**在本 crate 内**
  （D216 / D240 的口径：C-5 只扫 manifest 所属 crate 的 `src/`）。
  → 真 C-5 命令：`cargo test -p platform-app --test contract_c5`。

---

## D272: AC 编号 **AC-S-156 ~ AC-S-165**（+ 既有的 AC-S-024）

实测：全仓最大 AC-S 编号 = **155**（REQ-022）；`156`~`165` **未被任何文件占用**。

| AC | 主题 |
|---|---|
| **AC-S-024** | （既有，rev 2）撤销后内省校验的令牌**立即**失效 |
| **AC-S-156** | 应用注册表可增删改查 + **引用**而非复制 client 的敏感字段 |
| **AC-S-157** | 应用注册表的**租户隔离**（读侧 + 写侧 `WITH CHECK`） |
| **AC-S-158** | 应用**必须绑定存在且可用的** client；client 删除 → 应用**级联删除** |
| **AC-S-159** | 应用级权限声明 = client `allowed_scopes` 的**只读投影**，不得超出 |
| **AC-S-160** | 工作台清单：`active` 过滤 + 确定性排序 + 分组 |
| **AC-S-161** | 常用应用 / 最近访问（UPSERT + 只返回在册且 active 的） |
| **AC-S-162** | 菜单下发 API：两类来源合并 + **按权限点过滤** + `kind` 区分 |
| **AC-S-163** | 前端**运行时**路由注册表：新增应用**不改底座代码** |
| **AC-S-164** | 应用注册/修改/删除产生**审计记录** |
| **AC-S-165** | 权限点三处齐 + 启动对账 fail-fast + C-5 |

**共 11 条**（10 新 + 1 既有）。

---

## D273: 新 crate `platform-app`（不塞进 `platform-oauth`）

### 决定

新建 `crates/platform-app`，依赖 `platform-core` + `platform-db` + `platform-oauth`（**只读它的 client 表**）。

### 为什么不塞进 `platform-oauth`

| 理由 | 说明 |
|---|---|
| **职责不同** | `platform-oauth` = OAuth2 授权服务器（协议层）；`platform-app` = 应用接入（业务层） |
| **依赖方向** | `platform-app → platform-oauth` 单向；反过来会成环 |
| **零新增依赖** | 与 REQ-022 同款：只用树里已有的 |

⚠️ `platform-app` **不依赖** `platform-rbac`（用 `PermissionChecker` / `PermissionProvider` trait，装配层注入）。

---

## D274: 5 个配置项（登记在 `crates/platform-app/manifest.json` 的 `config{}` 段）

| 键 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `app.recentCount` | `number` | `6` | 「最近访问」与「常用应用」各取前 N |
| `app.workspace.maxApps` | `number` | `60` | 工作台最多展示多少应用（防一屏塞爆） |
| `app.icon.allowHttp` | `boolean` | `false` | 图标 URL 是否允许 `http`（仅本地开发设 true） |

- ⚠️ 键名 **lowerCamelCase**（契约 C9）；⚠️ **两处登记**：manifest 的 `config{}` +
  迁移里的 `platform_config_upsert(...)`（范式见 `migrations/20261120000700_oauth_config_seed.up.sql`）。
- `app.recentCount` 与 `app.workspace.maxApps` 的取值要能被 AC 断言**观察到**
  （即：改了配置后响应里的条数真的变）。

---

## 未决 / 待用户复核

| # | 事项 | BA 的倾向 |
|---|---|---|
| 1 | **应用注册表做成租户级**（D262）—— 与 REQ-022 的平台级 client 不在同一隔离层级，靠外键连接 | 按 `demand.md` 的「归属租户」走 |
| 2 | **client 删除 → 应用级联删除**（D264）—— 业务属性静默丢失 | 为了避免改坏 REQ-022 的 AC-S-135 |
| 3 | **应用入口 URL 存 `platform_app.launch_url`**（D269）—— 而不是复用 client 的 `redirect_uris` | 语义清晰 |
| 4 | **工作台/导航不加权限点**（D271）—— 登录即可看，靠导航项级过滤控可见性 | 与「统一入口」目标一致 |

---

## D275: 写规格时**不许凭直觉断数字** —— 本轮当场纠了一个

### 经过

BA 在写 AC-S-165 时，凭「新增一个 Rust crate → 索引多一个文件」的直觉写了
「`capabilities/` 下**恰好 15 个文件**（14 + 1）」。

**实测推翻**（`crates/xtask/src/capabilities.rs:57-79` 的 `generate`）：

```rust
files.insert("permissions.json", ...);
files.insert("extension-points.json", ...);
files.insert("schema/manifest.schema.json", ...);
files.insert("schema/metadata.schema.json", ...);
files.insert("schema/entity.schema.json", ...);
for (name, content) in sdk_files(root)? { files.insert(name, content); }   // 前端包各一个
files.insert("api.json", ...);
files.insert("index.json", ...);
```

→ 生成物是**按类别**组织的，**不是每 crate 一个文件**。
新增 Rust crate 只让 `permissions.json` 里多一个 `modules[]` 条目 → **文件数仍是 14**。

### 决定

判据改为「**恰好 14 个文件（不增不减）**」，并**把实测依据写进 AC 正文**。

### 为什么记这一条

这是 REQ-022 那个教训（`dev-guide §9.15`）的**同款形态**：
**凭想当然写一个数，然后没人会去验它** —— 而它一旦进了 AC 就是**验收判据**，
Dev 照着实现、BA 照着判定，错的那个数会被「通过」。

→ 规则：**AC 里出现的任何数字**（文件数 / 记录数 / 行数 / 状态码 / TTL），
**必须有一个 `file:line` 或一条实测命令支撑**，并把支撑写在 AC 正文里。

同理，本需求 AC 里已有的其它数字也要能用同样方式验：
- `TITLES` **93 → 99**（58+21+5+9+**6**）—— 依据：REQ-022 已把行数推到 93（实测），本轮 +6 个 `APP_*` 码
  （**rev 3 修正**：原写 8 个码 → 删了 2 个死码，见 acceptance §全局约定）；
- `capabilities/permissions.json` 的 `total` **32 → 35** —— 依据：REQ-022 后是 32，本轮 +3 个 `app:*`；
- `platform_app` 的**11 列** —— 依据：D265 的表（**逐列写死**，Dev 照着建）。


---

## rev 2（2026-11-21）：**QA 挑刺 39 条的逐条裁决**（D276~D285）

> **39 条全部接受**（无一驳回）。其中 **9 条阻塞级**，本 rev 逐条给修法。
> BA 已**独立核实** 6 条阻塞（S-01 / S-02 / S-03 / S-04 / S-09 / S-10），**全部成立**。

### rev 2 裁决总览

| 编号 | 一句话 | 级别 | 处置 |
|---|---|---|---|
| S-01 | 3 个 `app.*` 配置键**没写 `scope: platform`** → 落到 tenant 作用域 → 平台级读写直接拒 → AC-S-160/161 的「配置生效」永不可达 | 阻塞 | **D276** |
| S-02 | design §6 的 `config` 段是**裸值** → 非法 manifest schema → 解析失败 → **服务起不来** | 阻塞 | **D277** |
| S-03 | AC-S-163 的「运行时路由」在现有壳层**没有落点**（`routes` const / `createRouter` 捕获数组 / 无 `component` / 硬编码 if/else） | 阻塞 | **D278**（重写 AC-S-163） |
| S-04 | 播种没给 `launch_url` + design 要求「非空才可见」→ 照字面实现得到 **0 组** | 阻塞 | **D279** |
| S-05 | AC-S-156 步骤 8~11 前置不闭合（需未被占用的 client / 唯一名） | 阻塞 | **D280** |
| S-06 | AC-S-158 路径 3 **删的是共享夹具 `client-A`** → 打掉 4 条 AC 的数据 | 阻塞 | **D281** |
| S-07 | 「把配置改成 N」**没有执行主体与合法通道** | 阻塞 | **D282** |
| S-08 | 「把 `launch_url` 置空」在 `Option<String>` 下**表达不出来** | 阻塞 | **D283** |
| S-09 | D269 的导航字段**凭空多出 3 个**（`MenuSpec` 只有 label/icon/route/perm） | 阻塞 | **D284** |
| S-10 | AC-S-024 的「`exp` 距撤销时刻 ≤ TTL」**数学恒真** | 高 | **D285** |
| 其余 29 条 | 见 `qa-req016-spec-review.md` 逐条 | 高/中/低 | 随本 rev 一并改规格 |

---

## D276: 3 个配置键**必须写 `"scope": "platform"`**

### 实测证据

- `crates/platform-core/src/extension/manifest.rs:196-197` —— `ConfigSpec` 的 `scope` 有 `#[serde(default = "default_scope")]`；
- 同文件 `:264-266` —— `fn default_scope() -> ConfigScope { ConfigScope::Tenant }`
  → **不写 `scope` = 落到 `Tenant`**；
- `crates/platform-config/src/service.rs:280` —— 平台级**写**端点：
  `if spec.scope != ConfigScope::Platform { Err(ScopeNotAllowed{...}) }`；
- 同文件 `:238-246` —— 平台级**读**也只取 `scope == Platform` 的键；
- 对照已交付范式：`crates/platform-oauth/manifest.json` 的 5 个键**每条都带 `"scope": "platform"`**。

### 决定

`app.recentCount` / `app.workspace.maxApps` / `app.icon.allowHttp` **三个键全部写**
`"scope": "platform"` —— 因为它们是**整个底座**的策略（不是某租户的偏好）。

### 为什么这很危险（必须记住）

漏了 `scope` → manifest **能解析**、迁移**能写库**、编译**能过**，
**只有运行时静默过滤** → 失败现象是「**配置改了不生效**」，最像业务 bug。

### 判据

AC-S-160 步骤 4 / AC-S-161 步骤 6 的前置里**显式断言**：
`SELECT count(*) FROM platform_config WHERE key LIKE 'app.%' AND scope = 'platform'` == **3**。

---

## D277: `config` 段必须是 **`ConfigSpec` 对象**，不是裸值

### 实测证据

- `crates/platform-core/src/extension/manifest.rs:189-207` —— `ConfigSpec { kind(`type`) / default / scope / label? / format? }`；
  `type` 与 `default` **无 serde 默认** → **必填**；
- 同文件 `:247-248` —— `pub config: Option<BTreeMap<String, ConfigSpec>>`；
- `:229` 有 `deny_unknown_fields`；
- `Manifest::parse_checked` 的调用点：`crates/xtask/src/capabilities.rs:241`、
  `crates/platform-rbac/src/registry.rs:130`（**启动对账**）。

### 决定

design §6 的 `config` 段改成（**照抄 `crates/platform-oauth/manifest.json` 的形状**）：

```json
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
```

### 后果（若不改）

`cargo xtask capabilities` 报「invalid type: integer, expected struct ConfigSpec」→ **AC-S-165 步骤 1a 第一秒就红**；
更糟：**启动对账也会因同一 manifest 解析失败而挂**，日志却写「权限点注册表对账失败」→ **误导排查方向**。

---

## D278: **重写 AC-S-163** —— 第三方应用**不进 SPA 路由**

### 问题（S-03，BA 已核实全部 4 个事实）

| 事实 | 证据 |
|---|---|
| `RouteDefinition` **无 component 字段** | `apps/shell/src/lib/router.ts:41-62`（只有 `path` / `title` / `permission?` / `requiresAuth?` / `load?`） |
| `createRouter` **捕获数组引用** | `router.ts:156` `const { routes, ... } = options` → 闭包持有 |
| `Router` **没有 `addRoute` / `setRoutes`** | `router.ts:113` 的 `export interface Router` |
| 页面渲染是**逐路径硬编码** | `App.svelte:221-266`，**10 个** `{:else if resolution.path === ...}`，兜底是 `<Home/>` |

→ design §4.4 的 `routes = [...createRoutes(...), ...routesFromNavigation(nav)]`：
① 重赋值对路由器**零影响**；② 即使生效，新路径也**落到 `<Home/>`**，不可能「落到对应页面」；
③ 要真生效**必须改 `router.ts`** → 违反 D270。

**根因**：`ADR-0001 D4` 的「应用 = 路由」**成立，但那条路由必须是「外部 URL 的整页跳转」**。
第三方应用是**独立 bundle / 独立站点**（`launch_url`），不是本 SPA 的页面 ——
它**没有本 SPA 的 `component`**，天生不该进 SPA 路由表。

### 决定

**AC-S-163 重写为三条独立判据**（「宁可低风险交付，不交一个外形对、实质没用的东西」）：

1. **函数级（必须有）**：`routesFromNavigation(items)` 对 `kind="internal"` 的项产出
   `{ path: target, title: labelKey }`；对 `kind="external"` 的项产出 **0 条**（空数组）。
   → 这是**纯函数单测**，有区分力，可变异。
2. **入口级（必须有）**：`external` 项在**工作台页面**被渲染成**整页跳转**的入口
   （`<a href={target}>` 或 `location.href`），**不经过** SPA 路由。
3. **回归级（必须有）**：`apps/shell/src/lib/router.ts` **零改动**
   （`git diff --stat <baseline>..HEAD -- apps/shell/src/lib/router.ts` 输出为空）。

### **明确不做**（并从 AC 里删除）

- ❌ 「点击导航项 → 落到对应页面（渲染新组件）」—— **做不到**，因为 `RouteDefinition` 没有组件槽。
- ❌ 「运行时把新路由注入路由器」—— `createRouter` 捕获数组且无 `addRoute`。

### 如果将来真要「底座内的页面由后端下发」

那需要**三件事同时成立**（**不在本需求范围**）：

1. 给 `RouteDefinition` 加 `component` 字段（或加 `wrapper`）；
2. 把 `App.svelte` 的 10 个硬编码 if/else 改成**按 `route.component` 查表渲染**；
3. 给 `Router` 加 `addRoute` / `routes` 改为 getter。

→ 记 **TD-048**（见下），并写明「本需求**有意不做**，不是漏了」。

---

## D279: 工作台**不按 `launch_url` 过滤**；只有**导航**过滤

### 问题（S-04）

- `design-summary.md:174` / `:184` 要求「`launch_url` 非空才可见/可记访问」；
- 但 AC-S-160 的播种**没给 `launch_url`** → 照字面实现得到 **0 组 / 0 条**。

### 决定（**方案 A**）

| 端点 | 是否要求 `launch_url` 非空 |
|---|---|
| `GET /api/apps/workspace` | **不要求**（工作台是**管理 + 展示**视角，应用可以在册但没有入口） |
| `GET /api/apps/recent` / `frequent` | **不要求**（同工作台） |
| `POST /api/apps/{id}/access` | **不要求**（记录「用户打开过」这件事，与有没有入口无关） |
| `GET /api/apps/navigation` | **要求**（没入口的项点了没意义 → 不下发） |

### 理由

**工作台 ≠ 导航**。工作台是「这个租户接入了哪些应用」的**总览**（含尚未配入口的）；
导航是「点了去哪」的**可点击项**。混为一谈会让「刚注册但还没填入口」的应用**在工作台上消失** ——
正是管理员最需要看到它的时候。

→ AC-S-160 / AC-S-161 的播种**不必须**给 `launch_url`；AC-S-162 的用例才给。

---

## D280: AC-S-156 步骤 8~11 的**前置补闭合**

### 问题（S-05）

步骤 8~11 要验 4 个错误码，但没写「用什么 client / 什么 displayName」→ 会先撞
`APP_DUPLICATED`（409），而不是目标错误码。

### 决定

AC-S-156 的**前置条件**补：**每次调用前先新建一个专用 client**
（`client-probe-<n>`，`n` 递增，用 REQ-022 的 `POST /api/oauth/clients` 建），
**且每次用唯一的 `displayName`**（`探针-<n>`）。

→ 用例侧同理（见 `test-cases/01-registry.md` 的 TC-016-004~014）。

---

## D281: AC-S-158 路径 3 **不删共享夹具 `client-A`**

### 问题（S-06）

`client-A` 是 `seed-e2e.sh` 播种的**全仓共享夹具**，还被 AC-S-024 / 156 / 159 / 162 用着
→ 删它会**级联删掉 4 条 AC 的数据**，且 `client-A` 上带 `backchannel_logout_uri`（AC-S-149 要用）。

### 决定

路径 3 改成：**用 API 新建一个一次性 client** `client-cascade-probe`
（`POST /api/oauth/clients`）→ 建一个应用绑它 → `DELETE` 那个 client → 查应用行。

→ **不碰任何共享夹具**。

---

## D282: 「把配置改成 N」的**执行主体与通道**写死

### 问题（S-07）

AC-S-160 步骤 5 / AC-S-161 步骤 6 写「把 `app.workspace.maxApps` 改成 `2`」，
但**没说谁改、怎么改**。而配置有 **epoch 失效机制**（`crates/platform-config/src/resolver.rs:46-47`）——
直连 `UPDATE` 表**不会 bump epoch** → 改了**不生效**（缓存还在）。

### 决定

一律写成：

```sql
-- 以 base（管理员）直连执行
SELECT platform_config_upsert('app.workspace.maxApps', '2'::jsonb);
```

`platform_config_upsert` 是**已交付的**存储过程（REQ-010），它会**自增 epoch**
→ 下一次读就生效（无需重启、无需等 TTL）。

→ 并断言：改完后**立刻**（不等 sleep）调端点，看到新值。

---

## D283: 「把 `launch_url` 置空」的**手段**写死

### 问题（S-08）

`launch_url` 是 `Option<String>`，而 `PATCH` 请求体里 `null` 与「省略该键」在 serde 里**等价**
（都是 `None`）→「显式置空」与「不改」**分不开**。

### 决定（**三选一，取最简单的**）

**给 `platform_app` 加一个专用端点**：

```
DELETE /api/apps/{appId}/launch-url    → 204，把 launch_url 置 NULL
```

### 为什么不选另外两个

| 方案 | 为什么不要 |
|---|---|
| `Option<Option<String>>` + `null` | 需要在 `PATCH` 的 DTO 上做**双层 Option**，
且 serde 的 `deserialize_with` 要处理「键缺失 vs 键为 null」→ **容易写错、难测** |
| 用空串 `""` 表示「清空」 | 空串与 NULL 语义不同（前者是「配了但为空」）→ **口径混乱** |

→ 专用端点**最明确、最好测**（一条 `DELETE` → 204 → 查库为 NULL）。

---

## D284: 导航项的字段**收缩到 `MenuSpec` 真有的 4 个**

### 问题（S-09，BA 已核实）

`MenuSpec` 只有 **4 个字段**：`label` / `icon` / `route` / `perm`（`manifest.rs:120-133`）。
我 D269 里写的 `labelKey` / `category` / `sortOrder` / `key` **全部不存在**。

### 决定：`NavigationItem` 的字段**分两种来源分别定义**

```json
// source="module"（来自 manifest 的 menus[]）—— 只能用 MenuSpec 真有的字段
{ "source": "module",
  "label": "<menus[].label，中文文案>",
  "icon": "<menus[].icon | null>",
  "target": "<menus[].route>",
  "kind": "internal",
  "requiredPermission": "<menus[].perm | null>" }

// source="app"（来自 platform_app）—— 用应用表的字段
{ "source": "app",
  "appId": "<platform_app.id>",
  "label": "<platform_app.display_name>",
  "icon": "<platform_app.icon_url | null>",
  "target": "<platform_app.launch_url>",
  "kind": "external",
  "requiredPermission": null }
```

- ⚠️ **`module` 来源用 `label`（中文文案）而不是 `labelKey`** —— 因为 `MenuSpec` 里只有 `label`。
  前端**直接渲染** `label`（不再过 `t()`）。
  → **代价**：模块菜单**不再随语言切换**（`menus[].label` 是写死的文案）。
  → **理由**：菜单下发的**起点**就是 manifest 的 `label`；要做多语言得先给 `MenuSpec` 加
  `labelKey` 字段（**跨需求，D109 要声明**）→ **记 TD-049**。
- ⚠️ **`category` / `sortOrder` 只对 `app` 来源有意义**（`platform_app` 有这两列）；
  `module` 来源**没有** → 排序规则改成：**`module` 在前、`app` 在后**，组内各自按 `label` 升序。
- ⚠️ **取消 `key` 字段**（它原来说「manifest 的 `menus[].label`」，与 `label` 重复）。

---

## D285: AC-S-024 的断言**改成双向等式**

### 问题（S-10，BA 已核实）

原文：「`exp` 距撤销时刻**不超过** `access_token_expires_in` 秒」。
**数学恒真**：`exp = iat + TTL`，而撤销必然晚于签发 →
`exp - t_revoke = TTL - (t_revoke - iat) < TTL` **恒成立** → **零区分力**。

### 决定

改成：

| 判据 | 断言 |
|---|---|
| ① | `exp - iat == access_token_expires_in`（**逐字节相等**）← 这才是「TTL 被正确应用」 |
| ② | 撤销后签名**仍然有效**（`VerifyOutcome::Valid`）← 证明撤销**没改**令牌本身 |
| ③ | 撤销后内省 `active == false`（**立即**，不等待） |

→ 三条**互相独立**，任一条被改坏都会红。

---

## 附：本轮**不做**但记债的 4 条

| 债号 | 内容 |
|---|---|
| **TD-048** | 底座内的页面**不能由后端下发**（`RouteDefinition` 无 `component` 槽 + `App.svelte` 10 处硬编码 if/else + `Router` 无 `addRoute`）→ 见 D278 的「将来真要做的三件事」 |
| **TD-049** | `menus[].label` 是**写死的中文文案** → 模块菜单**不随语言切换**（要修得给 `MenuSpec` 加 `labelKey`，跨需求） |
| **TD-050** | 应用注册**没有数量配额**（一个租户可注册任意多个应用）→ 后续按 `app.workspace.maxApps` 之外的维度加限制 |
| **TD-051** | `PATCH /api/apps/{id}` **无并发控制**（两人同时改会丢更新）→ 后续加 `If-Match` / `version` 列 |
