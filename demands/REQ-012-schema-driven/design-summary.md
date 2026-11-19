# 设计摘要：REQ-012 元数据驱动渲染

- **需求编号**: REQ-012
- **状态**: 草稿（2026-11-19 15:25）
- **验收标准**: [acceptance.md](acceptance.md)（AC-S-076 ~ AC-S-087）
- **基线**: `develop` @ `a7f7579`（REQ-011 合并点 + CI 修复）
- **用户裁定**（2026-11-19 15:00）：
  1. 范围：**第一棒渲染闭环 + 字段级权限**，CRUD 脚手架**延后**
  2. 字段级权限：**平台唯一序列化入口 + 契约测试扫违规**

---

## 1. 本需求的定位

REQ-009 给了服务端鉴权，REQ-011 给了前端 SDK。但「**新增一个 CRUD 页面**」至今仍是
**纯手写**：建实体、建表、写 Repository、写 5 个 handler、写 5 个前端页面。

本需求要证明一件事：**新增同类页面只写元数据，不写前端代码**。

如果做不到这一点，这个需求就没有存在的理由。

### 1.1 现状核实（起草时逐个核过）

| 能力 | 现状 |
|---|---|
| manifest 的 schema 段 | **不存在**（`manifest.rs:185-207` 只有 permissions/menus/dependencies/config/tenantScoped） |
| 元数据读取接口 | **不存在**（`capabilities/api.json` = `{"status":"not-ready","paths":[]}`） |
| 字段级权限 | **不存在任何机制**；业务 handler 直接 `Json(...)` 输出 |
| `@base/schema` | 只有 `FieldMeta{key,label,type,required}` + 2 个工具函数 |
| `crud_scaffolding` | 在 `not_yet_provided` 里 —— **本轮仍不做**（用户拍板延后） |

---

## 2. 元数据形状（manifest 的 `schema` 段）

### 2.1 形状

```json
"schema": {
  "demoItem": {
    "label": "示例条目",
    "api": {
      "list":   "/api/sandbox/demo-items",
      "create": "/api/sandbox/demo-items",
      "update": "/api/sandbox/demo-items/{id}",
      "delete": "/api/sandbox/demo-items/{id}"
    },
    "fields": [
      { "key": "id",     "label": "ID",   "type": "text",  "readonly": true },
      { "key": "title",  "label": "标题", "type": "text",  "required": true, "maxLength": 200 },
      { "key": "amount", "label": "金额", "type": "number", "min": 0 },
      { "key": "paid",   "label": "已付款", "type": "boolean" },
      { "key": "dueAt",  "label": "到期日", "type": "date" },
      { "key": "ownerId","label": "负责人", "type": "reference",
        "reference": { "module": "org", "entity": "user" } },
      { "key": "secret", "label": "内部备注", "type": "text",
        "permission": "sandbox:demo:secret" }
    ]
  }
}
```

### 2.1.1 `api` 子段（rev 2 新增，D102）

| 键 | 必填 | 含义 |
|---|---|---|
| `list` | ✅ | 列表接口（前端 `SchemaTable` 向它取数据） |
| `create` | ✅ | 创建接口 |
| `update` | — | 更新接口（不声明 → 前端不渲染「编辑」） |
| `delete` | — | 删除接口（不声明 → 前端不渲染「删除」） |

- `{id}` 是**唯一**占位符，渲染时替换为记录的 `id`
- **启动校验**：每个路径必须以 `/api/` 开头
  （**安全约束**：防止声明外部 URL 让前端把请求发到站外）

> **为什么显式而不是约定式**（`/api/<module>/<entityPlural>`）：
> 复数化规则要么靠猜（`item→items` 还行，`person→people` 就错），要么强制命名。
> 显式只多几行，但**永远不会猜错**。

### 2.1.2 权限点段数（rev 2 修正）

⚠️ **权限点编码必须是恰好 3 段**（`module:resource:action`）——
`manifest.rs:367-370` 的 `is_permission_code` 强制 `parts.len() == 3`。

rev 1 的示例 `sandbox:demo:secret:read` 是 **4 段，非法**。正确写法：**`sandbox:demo:secret`**。

`PermissionType` 枚举**已含 `Field` 变体**（`manifest.rs:86-95`），
所以字段级权限点直接写 `"type": "field"`，**不需要扩枚举**。

### 2.2 字段类型的允许集合（**写死**）

| type | 含义 | 前端控件 | 类型专属校验参数 |
|---|---|---|---|
| `text` | 文本 | 文本框 | `maxLength` / `minLength` / `pattern` |
| `number` | 数字 | 数字输入 | `min` / `max` |
| `boolean` | 布尔 | 开关 | — |
| `date` | 日期 | 日期选择 | — |
| `reference` | 引用其它实体 | **只读 id 文本**（本期降级，D105） | `reference: { module, entity }` |

> ⚠️ **`reference` 本期不做下拉**（rev 2，D105）。
> 选项接口需要定义路径 / 形状 / 权限 / 上限 / 分页 / 搜索 —— 是一整块能力。
> **降级比「定义了接口却实现不了」诚实**：让 AC 如实描述能做的事。

> **为什么只有 5 种**：本轮不做「富文本 / 附件 / 枚举 / 多值」——
> 它们各自需要控件与校验语义，等真实需求出现再加。
> **加类型是向后兼容的**（老渲染器遇到新类型按 AC-S-087 降级为文本）。

### 2.3 字段的通用属性

| 键 | 必填 | 含义 |
|---|---|---|
| `key` | ✅ | 字段名，camelCase，`^[a-z][a-zA-Z0-9]*$` |
| `label` | ✅ | 展示名，非空 |
| `type` | ✅ | 见上表 |
| `required` | — | 默认 `false` |
| `permission` | — | 字段级权限点；**声明后无权用户看不到、接口也不返回** |
| `readonly` | — | 只读（表单里不渲染输入控件） |

### 2.4 校验时机与顺序（**rev 2 重大修正**）

#### 2.4.1 是**聚合**，不是「首个失败即退出」

rev 1 写的「首个失败即退出」**与现有实现冲突**：
`manifest.rs:230-243` 的 `parse_checked` 返回 `Vec<ManifestError>`，
`validate()` 收集**全部**问题；三处调用点（`platform-config/src/registry.rs:118`、
`platform-rbac/src/registry.rs:130`、`platform-db/src/scopes.rs:83`）都依赖这个 `Vec`。

**改成首个失败是破坏性改动，不做。** 保留聚合 —— 对开发者更友好（一次看到所有问题）。

验收侧对应：单条违规断言 `issues.len() == 1`，两条违规断言 `== 2`。

#### 2.4.2 两阶段校验（rev 2 新增）

有些规则**需要全局上下文**，不可能在单文件解析时判出：

| 阶段 | 时机 | 内容 |
|---|---|---|
| **阶段 1（单文件）** | 每个 manifest 解析时（`validate()`） | 实体键格式、`fields` 非空、字段 `key`/`label`/`type` 合法、同实体内 `key` 不重复、`api` 路径以 `/api/` 开头 |
| **阶段 2（全局）** | **所有** manifest 加载完后 | 字段的 `permission` **已登记**（在权限点注册表里）、`reference.module/entity` **存在** |

阶段 2 失败 → 同样**进程起不来**，点名「谁的哪个引用指向了不存在的东西」。

> 这与 REQ-009 的 D27（启动时权限点对账）同源：
> **跨文件一致性只能在所有文件都加载后才能判**。

#### 2.4.3 阶段 1 内部顺序（写死）

1. 实体键符合 `^[a-z][a-zA-Z0-9]*$`
2. `fields` **非空**
3. 每个字段：`key` 格式 → `label` 非空 → `type` 在允许集合内
4. 同一实体内 `key` **不重复**
5. `api` 的每个已声明路径以 `/api/` 开头

> **为什么顺序写死**：与 REQ-010 的 D64 同源。不写顺序时，
> 「被拦住」可能来自任意规则，无法证明**具体规则**生效。

---

## 3. 元数据读取接口

### 3.1 路由

```
GET /api/schema/<moduleId>/<entityKey>
```

**不新增权限点** —— 走「已认证即可读」+ **字段级过滤**。
理由：元数据是**界面的描述**，不是数据。要求额外权限点会让前端在拿到元数据前
就要先知道该要什么权限（循环依赖）。

但**不是免费的**：无权字段在元数据里就被剔掉（AC-S-079），
所以「读元数据」不泄露任何用户本来就无权知道的东西。

### 3.2 响应形状

```json
{
  "module": "sandbox",
  "entity": "demoItem",
  "label": "示例条目",
  "fields": [
    { "key": "title", "label": "标题", "type": "text", "required": true, "maxLength": 200 }
  ]
}
```

- `fields` 的**顺序 = manifest 声明顺序**（前端表头顺序依赖它）
- 未声明的可选属性**省略该键**（不是 `null`）
- **没有任何**「被过滤掉的字段」的痕迹（不含 `hiddenFields` / `total` 之类的计数）

### 3.3 错误码

| 情形 | HTTP | code |
|---|---|---|
| 未认证 | 401 | `UNAUTHENTICATED` |
| 模块不存在 | 404 | `SCHEMA_MODULE_NOT_FOUND` |
| 实体不存在 | 404 | `SCHEMA_ENTITY_NOT_FOUND` |

> 模块存在但实体不存在 → `SCHEMA_ENTITY_NOT_FOUND`；
> 模块根本不存在 → `SCHEMA_MODULE_NOT_FOUND`。
> **区分两者**是有意的：模块名通常是常量（写错就知道），
> 实体名来自 URL（用户可随便填），分开报更容易定位。

---

## 4. 字段级权限：唯一序列化入口（本轮的技术核心）

### 4.1 问题

业务模块现在的输出是 `Json(serde_json::to_value(&item)?)` —— 实体有哪些字段就输出哪些。
要按权限剔除字段，有两条路：

- 每个 handler 自己写过滤逻辑 → **重复劳动，且忘写就是泄露**
- 平台提供一个**唯一入口**，业务模块必须走它

用户拍板走第二条。

### 4.2 入口设计

```rust
// crates/platform-core/src/extension/schema.rs（新增）
/// 元数据驱动的**唯一**响应序列化入口。
///
/// 业务模块**不得**直接 `Json(entity)` 输出带字段级权限的实体 ——
/// 必须经它，否则无权字段会泄露（契约 C-6 扫这个）。
pub struct SchemaProjector;

impl SchemaProjector {
    /// 按元数据 + 当前用户的字段级权限，剔除无权字段。
    ///
    /// # 语义（**必须严格**）
    ///
    /// | 情形 | 结果 |
    /// |---|---|
    /// | 字段无 `permission` 声明 | 保留 |
    /// | 有 `permission`，且 `checker.has(ctx, code)` | 保留 |
    /// | 有 `permission`，且判定不通过 | **整个键从对象里删除** |
    ///
    /// **不是**置 `null`、**不是**置空串、**不是**掩码成 `"[REDACTED]"` ——
    /// 三者都保留「这里有东西」的信息。
    pub fn project(
        raw: serde_json::Value,
        schema: &EntitySchema,
        ctx: &RequestContext,
        checker: &dyn PermissionChecker,
    ) -> serde_json::Value;
}
```

**为什么是 `serde_json::Value` 而不是泛型 `T: Serialize`**：
- 泛型的实现要么走 serde 的 `serialize_map` 插桩（复杂且容易漏嵌套），
  要么要求每个实体 derive 一个特殊 trait（侵入业务代码）
- 先 `to_value` 再按 schema 的字段清单重建对象，**逻辑只有一处**，
  且对「实体多出未声明字段」天然 fail closed（未声明 → 不在白名单 → 删除）

> ⚠️ **白名单语义**：投影后的对象**只含元数据声明过、且当前用户有权的字段**。
> 实体里多出来的字段（比如后来加的、还没写进元数据的）**默认不输出**。
> 这是 fail closed —— 加字段忘了写元数据时，**少显示**而不是**多泄露**。

### 4.3 业务模块怎么用

```rust
let raw = serde_json::to_value(&item).map_err(...)?;
let out = SchemaProjector::project(raw, schema, &ctx, checker.as_ref());
Json(out)
```

### 4.4 契约 C-6

**判据（在 AC-S-081 步骤 3 里要求选定，这里选定）**：

> **若某模块的 manifest 声明了任何带 `permission` 的字段，
> 则该模块的源码里必须出现 `SchemaProjector::project` 的调用。**

**这个判据是「至少一处」，不是「每个 handler 都调用」** ——
静态判定「哪个 handler 返回哪个实体」需要类型推导，在 Rust 源码扫描里不可靠
（会误报，进而被人关掉）。

**它的局限必须写明**：
- 「3 个 handler 只有 1 个调用」**能过 C-6**
- 缓解：AC-S-080 的用例覆盖「列表接口」这条主路径；
  真实项目里列表/详情/创建/更新的响应通常共用一个投影函数

> **为什么不做得更强**：C-5 的经验是 —— 判据太脆会被人关掉，
> 关掉之后**比没有更坏**（有个绿色的检查在那儿，让人以为守住了）。
> 宁可要一个**弱但真实执行**的检查 + 诚实的局限说明。

---

## 5. 前端

### 5.1 分工（沿用 REQ-011 裁定 #7）

| 位置 | 内容 |
|---|---|
| `packages/schema/src/index.ts` | 类型 + **纯逻辑**：`EntityMeta` / `FieldMeta`（扩展）/ `visibleFields()` / `validateForm()` / `buildListQuery()` / `formatCell()` |
| `apps/shell/src/components/SchemaTable.svelte` | 列表组件（薄封装，调用包里的逻辑） |
| `apps/shell/src/components/SchemaForm.svelte` | 表单组件 |
| `apps/shell/src/pages/SchemaPage.svelte` | **一条通配路由**的页面 |

> ⚠️ **`SchemaTable` 不复用 `@base/ui` 的 Table**（rev 2，D106）。
> `Table.svelte` **恒渲染 `thead`**、空态**渲染一行**、分页需要 `total`；
> 而本需求要「空态不渲染空壳表」+ **游标分页没有 `total`** —— 语义不同。
> 改 Table 会动 REQ-011 已通过的契约（AC-S-075 双向断言）。
> **同步要求**：`capabilities/index.json` 的 `ui_table_component` 说明要更新为
> 「Table 是**页码分页**的只读表格；**游标分页**请用 `SchemaTable`」。

### 5.2 ★ 零代码的关键：通配路由（rev 2：需要先扩 router）

```
/schema/:moduleId/:entityKey
```

前端**只有一条**路由。新增实体时：
- manifest 加 `schema` 段 → 重启后端 → 访问 `/schema/<module>/<entity>` → 自动渲染
- **`apps/shell/src/**` 一行不改**

> **这是 AC-S-085 能成立的原因**。如果每加一个实体就要在路由表里注册一行，
> 那「零前端代码」就是假的。
> 导航入口（菜单）来自 manifest 的 `menus` 段（REQ-002 已有），也不需要改前端。

#### 5.2.1 ⚠️ 前提：router 目前**不支持** `:param`（rev 2，D104）

`apps/shell/src/lib/router.ts:101` 现在是：
```ts
const match = (path) => routes.find((route) => route.path === path);
```
**只做精确匹配** —— 通配路由**不存在**。

**必须扩它**，要求：
- **向后兼容**：精确路径仍按精确匹配（REQ-011 的现有路由不受影响）
- **匹配优先级**：精确 **>** 含 `:param` 的（避免通配抢走精确路由）
- **参数提取**：`match('/schema/a/b')` → `{ route, params: { moduleId: 'a', entityKey: 'b' } }`
- **权限**：`RouteDefinition.permission` 仍按静态声明；通配路由声明**模块级**权限点

**这是对 REQ-011 产物的扩展，不是破坏** —— 但必须在提交信息与复盘里写明。

### 5.3 包内的纯逻辑（可单测，不依赖 DOM）

```ts
/** 过滤出当前元数据里可见的字段（后端已过滤，前端只是忠实渲染）。 */
export function visibleFields(meta: EntityMeta): FieldMeta[];

/** 表单校验：返回「字段 → 错误文案」的映射；空映射表示通过。 */
export function validateForm(meta: EntityMeta, values: Record<string, unknown>): Record<string, string>;

/** 按类型格式化单元格（未知类型 → 原样文本 + 标记）。 */
export function formatCell(field: FieldMeta, value: unknown): { text: string; unknownType: boolean };

/** 构造列表查询参数（游标分页）。 */
export function buildListQuery(cursor: string | undefined, limit?: number): Record<string, string>;
```

### 5.4 元数据演进容错（AC-S-087）

| 情形 | 行为 |
|---|---|
| 元数据**新增**字段 | 正常渲染 |
| 元数据**删除**字段，数据里还有该键 | 忽略该键（不渲染成「未知列」） |
| `type` 是**不认识**的值 | 降级为纯文本 + `console.warn` 一次（**整页不崩**） |
| 数据**缺**非必填字段 | 展示为空（不是 `undefined` 字样） |

> **为什么容器级容错而非报错**：一个未识别的字段类型让整页白屏，
> 等于**元数据演进的代价是页面不可用** —— 那就没人敢加新类型了。
> 降级 + 一次告警，是「可用性优先 + 问题可发现」的平衡。

---

## 6. 测试策略

### 6.1 后端

- schema 段解析与校验：**单元测试**（各种非法输入）
- 元数据接口：**集成测试**（真库 + 真 RBAC）
- `SchemaProjector`：**单元测试**（各种权限组合）
- C-6 扫描器：**负向用例**（造违规，确认失败）

### 6.2 前端

- 包内纯逻辑：**vitest**（`visibleFields` / `validateForm` / `formatCell`）
- 组件：`apps/shell` 的 vitest（jsdom）
- 端到端：**Playwright**（真服务 + 真浏览器）
  > REQ-011 的教训：**单测全绿 ≠ 能用**。AC-S-085 / 086 必须真浏览器验

### 6.3 本仓的假绿清单（REQ-011 教训，全部适用）

| 陷阱 | 正确写法 |
|---|---|
| 否定式断言 | 断「**等于**期望值」 |
| 断言 `=== null` 当作「字段不存在」 | 断 `hasOwnProperty === false` |
| 只跑包级单测就宣布可用 | 应用级行为必须真浏览器 |
| 测试夹具依赖别的测试副作用 | 独立幂等播种（REQ-011 的 D101） |

---

## 7. 风险

| # | 风险 | 缓解 |
|---|---|---|
| R1 | `SchemaProjector` 用白名单语义 → 加了字段忘写元数据，数据「消失」 | 这是**有意 fail closed**（少显示 vs 多泄露）；用 AC-S-087 步骤 2 保证加字段的正常路径 |
| R2 | C-6 判据弱（至少一处） | 已在 §4.4 写明局限；用例覆盖主路径 |
| R3 | 前端通配路由与 REQ-011 的 `guard()` 怎么配合 | 权限点由元数据里**实体级** `permission` 声明（本轮不加，用模块级菜单权限）；路由守卫沿用 REQ-011 |
| R4 | 元数据接口被高频调用（每个页面一次） | 本轮不做缓存（元数据很小，一次几百字节）；若成瓶颈再加 ETag |
| R5 | `reference` 类型的数据源（下拉选项）从哪来 | 本轮只支持「引用某个实体的 id 列表」，由业务模块提供选项接口；**不在本轮做联表查询** |
| R6 | 依赖预算 | **零新 Rust 依赖**（`serde_json` 已在树）；前端零新增 |
| R7 | 低配机编译慢 | 复用 `code/target`；`jobs = 3` |

---

## 8. 不做（与 demand.md 一致）

CRUD 代码脚手架、通用数据接口、元数据管理界面、复杂布局、导入导出、
字段级权限的**写侧**校验。
