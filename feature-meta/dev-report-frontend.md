# REQ-012 前端部分 —— Dev 自验报告

- **需求**: REQ-012 元数据驱动渲染（**前端部分**：AC-S-082 / 083 / 084 / 085 / 086 / 087）
- **分支 / worktree**: `feature/REQ-012` @ `/home/myg133/Projects/system_seed/feature-REQ-012`
- **基线**: `develop` @ `a7f7579`（REQ-011 合并点 + CI 修复）
- **实施人**: Dev（前端棒）
- **日期**: 2026-11-19
- **规格正本**: `BA/demands/REQ-012-schema-driven/{acceptance.md(rev2), design-summary.md, acceptance-rev2-ruling.md, test-cases/*.json}`
- **后端棒**: 另一个 Dev 并行（本报告**未动** `crates/` / `modules/` / `capabilities/` / `AGENTS.md`）

---

## 1. 交付概览

| # | 内容 | 落点 |
|---|------|------|
| 1 | `@base/schema` 纯逻辑（元数据形状 + 渲染 / 校验 / 查询构造） | `packages/schema/src/index.ts` |
| 2 | `:param` 通配路由（D104，精确优先，向后兼容） | `apps/shell/src/lib/router.ts` |
| 3 | 通配路由注册 + 页面接入 | `apps/shell/src/lib/routes.ts`、`apps/shell/src/App.svelte` |
| 4 | `SchemaTable`（**不复用** `@base/ui` 的 Table，D106） | `apps/shell/src/components/SchemaTable.svelte` |
| 5 | `SchemaForm`（5 种类型 + readonly + reference 只读） | `apps/shell/src/components/SchemaForm.svelte` |
| 6 | `SchemaPage`（元数据 + 列表 + 增删改 + 游标分页） | `apps/shell/src/pages/SchemaPage.svelte` |
| 7 | 请求装配（元数据 / 列表 / create / update / delete） | `apps/shell/src/lib/schema-api.ts` |
| 8 | E2E | `e2e/schema-driven.spec.ts` |
| 9 | 单元 / 组件测试 | `packages/schema/test/`、`apps/shell/test/{router-params,schema-api,schema-form,schema-table,zero-code}.test.ts` |

---

## 2. AC 落点（逐条）

### AC-S-082 —— `SchemaTable` 按元数据渲染列表

| 要求 | 实现 | 证据 |
|---|---|---|
| 表头文本与顺序 = 元数据 `fields.label` | `SchemaTable.svelte` 遍历 `visibleFields()`，行操作列**不用 `th`**（否则表头多一列） | `schema-table.test.ts`「表头文本与顺序精确等于…」；真机 `headers = ["ID","标题","金额","已付款","到期日","负责人"]` |
| `boolean` → `是`/`否`；`date` → `YYYY-MM-DD`（不换算时区）；空值 → 空串；`reference` → id 原值 | `formatCell()`（`packages/schema/src/index.ts`） | `schema.test.ts` 的 `formatCell` 全组；真机 `row1 cells = ["item-1","示例","12.5","是","2026-11-19","u-1001",…]` |
| 下一页请求带上一页的 `nextCursor`；`nextCursor === null` → 按钮禁用 | `buildListQuery()`（cursor 在前、limit 在后）+ `SchemaPage` 游标栈 | 真机：`?limit=20` → `?cursor=cursor-page-2&limit=20`，末页 `next disabled = true` |
| 空态**不渲染**表头空壳，文案非空 | 空数据只渲染 `<p class="base-schema-empty">暂无数据</p>` | `schema-table.test.ts` 空态组；真机 `empty: thead count = 0`、`text = "暂无数据"` |
| **不复用** `@base/ui` 的 Table | 自渲染 `<table>` + 游标分页（无 `total`） | `SchemaTable.svelte` 文件头注释（D106） |

### AC-S-083 —— `SchemaForm` 按元数据渲染表单

| 要求 | 实现 | 证据 |
|---|---|---|
| `text`→文本框；`number`→数字；`boolean`→复选框；`date`→日期；`reference`→**只读 id 文本**（D105）；`readonly` 不渲染控件 | `SchemaForm.svelte` | `schema-form.test.ts`；真机 `number/date/checkbox type`、`reference has input = 0`、`readonly id has input = 0` |
| 校验失败**不发请求**，错误在字段旁 + `aria-invalid="true"` | 提交先走 `prepareSubmit()`；有错误只渲染错误、不调 `onSubmit` | 真机：必填留空后 `writes = []`、`error text = "标题不能为空"`、`aria-invalid = true`、`aria-describedby = schema-error-title` |
| 提交 body 只含元数据声明的可写字段 | `prepareSubmit()`（排除 `readonly`、排除内部键；`number` 转数值） | 真机 `POST body = {"title":"示例条目","amount":12.5,"paid":false}`（无 `id`、无 `$`/`_` 键） |

> **文案钉死说明**：AC 只说「文案指出『必填』」，用例集（TC-012-042/043）钉死 `<label>不能为空`。
> 实现取用例集的字面量（`标题不能为空` / `金额必须是数字`），并在必填字段 label 旁另渲染 `必填` 标记 —— 两种判据都能读。

### AC-S-084 —— 按元数据跳过无权字段

前端**不做权限判断**，只忠实渲染后端给的元数据：`visibleFields()` 不会读数据里元数据没有的键，也不会自造字段。
`prepareSubmit()` 只遍历元数据声明的字段 → 无权字段不可能进 body。
（权限正确性在 AC-S-079/080；本条的 DOM 取证见 E2E，尚未跑通，详见 §5。）

### AC-S-085 —— 新增同类页面无需新增前端代码

| 前提 | 实现 |
|---|---|
| router 支持 `:param`（D104） | `router.ts`：先精确匹配，再按声明顺序匹配含 `:param` 的路由；参数进 `RouteResolution.params`；非法百分号编码保留原文不抛错 |
| 一条通配路由服务所有实体 | `routes.ts` 注册 `/schema/:moduleId/:entityKey`（`requiresAuth: true`），`App.svelte` 用 `resolution.params` 渲染 `SchemaPage` |
| 前端源码不含实体特化 | `zero-code.test.ts` 扫描 `packages/schema/src/**` + `apps/shell/src/**`：`demoItem` / `demoNote` / `demo-notes` / `备注簿` 命中数均为 0，并用「扫描能命中 `暂无数据`」做对照自检 |

### AC-S-086 —— 零代码 CRUD 闭环

`SchemaPage`：`api.list` 取数 → 新建 `api.create` → 编辑 `api.update`（`{id}` 替换）→ 删除 `api.delete`（确认对话框）。
`api.update` / `api.delete` 未声明 → 对应按钮不渲染。
E2E 用例已写（`e2e/schema-driven.spec.ts`），但**尚未跑通**（后端阻塞，见 §5）—— **不写「通过」**。

### AC-S-087 —— 元数据演进容错

| 场景 | 行为 | 证据 |
|---|---|---|
| 新增字段 | 正常多一列；数据缺该键 → 空串 | `schema-table.test.ts`「新增字段」 |
| 删除字段（数据仍有该键） | 该列不渲染，多余键被忽略、不报错 | 同上「删除字段」 |
| 未知 `type` | 降级纯文本 + `console.warn` **一次**（文案含类型名与字段 key） | 同上「未知类型」；ssr 断言 warn 次数 = 1 |
| 数据缺非必填字段 | 空串（不出现 `undefined` / `null` / `NaN`） | 同上「数据缺非必填字段」 |

---

## 3. 关键实现决策 / 与任务书的差异（都写在这里，不藏）

1. **更新动词**：元数据没有声明 HTTP 方法，后端 `modules/sandbox/backend/src/http.rs` 用的是 **PUT**。
   前端先发 `PATCH`，**只在 405** 时退回 `PUT`（405 = 路径存在但方法不允许，服务端没处理过，重试安全）；
   其它错误（500 等）**不重试**，避免把失败变成两次写入。见 `schema-api.ts` + `schema-api.test.ts`。
2. **通配路由没有静态权限点**：任务书写「通配路由声明模块级权限点」，但 `permission` 是**静态**字段，
   而模块来自 URL 参数；硬写 `sandbox:demo:read` 会让其它模块的实体一律 403（零代码当场破功）。
   实现改为 `requiresAuth: true`（未登录落登录页），字段级可见性由后端过滤元数据/数据负责 —— 与 design R3 一致。
   这是对 `RouteDefinition` 的**增量**字段，现有路由行为不变。
3. **`SchemaTable` 行操作列用 `<td>` 作表头单元格**：AC-S-082 断言 `th` 文本数组**精确等于**字段 label 数组；
   多一个空 `th` 就多一列。因此操作列表头不生成 `th`（组件里有注释）。
4. **空的 `date` / `reference` 不下发空串**：后端解析日期列会因 `""` 直接 400。空文本仍下发（清空文本是有效编辑）。
5. **空态文案**钉死 `暂无数据`（TC-012-040 的精确文本）。
6. **`@base/schema` 的 `FieldMeta.type` 声明为 `string`**（不是 `FieldType` 联合）：运行时可能拿到未知类型，
   联合类型会在编译期堵死「降级渲染」这条 AC 要求的路径。

---

## 4. 验证（实测输出）

### 4.1 类型 / 构建 / 测试

```
$ pnpm -r check
packages/* check: Done（全部）
apps/shell check: svelte-check found 0 errors and 0 warnings

$ pnpm -r build
apps/shell build: ✓ built in 1.49s（dist 产出正常）

$ pnpm -r test
packages/hooks  17 passed
packages/schema 38 passed
packages/sdk    34 passed（packaging.test.ts 5 条**实际执行**，不是 skip）
packages/theme  12 passed
packages/auth   26 passed
packages/ui     14 passed
apps/shell      90 passed
packages/i18n    3 passed
合计 234 passed / 0 failed（每包均 > 0）
```

```
$ pnpm exec eslint .
（无输出 = 通过）

$ npx prettier --check .
[warn] feature-meta/manifest.json      ← **基线既有问题**：该文件在 HEAD 里就缺文件末尾换行，
                                          本棒未修改它（git status 干净），也不属于本棒范围
（本棒新增/修改的文件全部通过）
```

### 4.2 变异验证（4 项，全部按要求变红后**逐字节还原**）

| # | 变异 | 结果 | 还原校验 |
|---|---|---|---|
| 1 | `formatCell` 对 `boolean` 返回原值 | `schema.test.ts` 1 failed（boolean → 是/否） | sha256 与变异前一致 |
| 2 | `validateForm` 恒返回 `{}` | 5 failed（必填 / 数字 / 上下界 / 长度正则 / prepareSubmit 不发请求） | sha256 一致 |
| 3 | `SchemaTable` 空数据仍渲染表头 | `schema-table.test.ts` 1 failed（`expected … not to contain '<thead'`） | sha256 一致 |
| 4 | 去掉 `match` 的 `:param` 支持 | `router-params.test.ts` 5 failed | sha256 一致 |

### 4.3 真机（真实浏览器 + 真实后端 + 真路由）

后端由另一个 Dev 并行实现，本棒用**共享 target 构建的 `base` 二进制**在 `127.0.0.1:8093` 起服务
（`base_test` + 既有 E2E 夹具），并用 Playwright 真浏览器验证：

| 检查 | 实测 |
|---|---|
| 深链 `/schema/sandbox/demoItem`（**通配路由**） | `url = /schema/sandbox/demoItem`；页面标题 = 元数据 `label`（`示例条目`）；无 page error |
| 元数据接口 | `GET /api/schema/sandbox/demoItem` → 200，且响应**含 `api` 子段**（D102），无 `secret` 字段（当前用户无该字段权限） |
| 列表/表单/分页/空态（用 Playwright 路由桩注入夹具，验证客户端行为） | 表头 6 列、`boolean → 是`、`date → 2026-11-19`；下一页请求 `?cursor=cursor-page-2&limit=20`、末页禁用；空态 `thead count = 0` + `暂无数据`；页面零错误 |
| 表单校验 | 必填留空：**零写请求**、错误文案、`aria-invalid="true"`、`aria-describedby` 指向错误节点 |
| 合法提交 | **一个** POST，body `{"title":"示例条目","amount":12.5,"paid":false}`（无 `id`、无内部键） |

> ★ **真机抓到并已修复一个 SSR 单测抓不到的缺陷**：`<input type="number">` 默认 `step=1`，
> 填 `12.5` 时浏览器原生校验把表单判成 `:invalid`，**静默拦截提交**（既没有请求、也没有字段级错误，
> 因为错误文案来自我们自己的 `validateForm`）。修法：`<form novalidate>` + 数字控件 `step="any"`，
> 校验唯一权威是 `validateForm`。已加回归断言（`schema-form.test.ts`：`novalidate` + `step="any"`）。
> 这正是「单测绿 ≠ 能用」的那一类问题。

---

## 5. 未完成 / 阻塞（**E2E 待后端就绪后复跑，不写「通过」**）

`BASE_E2E_URL=http://127.0.0.1:8093 npx playwright test e2e/schema-driven.spec.ts` 实测：

```
6 skipped
```

原因（三条都是**后端 / 夹具**侧，不是前端行为失败）：

1. **模块路由没有注入 `RequestContext`**：同一个有效 bearer 令牌，
   `GET /api/schema/sandbox/demoItem` → **200**，而 `GET /api/sandbox/demo-items?limit=1` → **401 `UNAUTHENTICATED`**。
   `modules/sandbox` 的 `require_permission` 从请求扩展取 `RequestContext`，但 `/api/sandbox` 的挂载没有认证中间件
   （`crates/platform-server/src/main.rs:385` 只 `sandbox::mount(...)`）。因此列表 / 增删改全部 401。**需要后端棒接线**。
2. **E2E 夹具没有 sandbox 权限点**：`scripts/seed-e2e.sh` 的 `u-perm-read/write` 只有 `rbac:role:*`，
   没有 `sandbox:demo:read` / `sandbox:demo:write`（也没有 `sandbox:demo:secret` 的差异用户）。
   我的 spec 支持用 `SCHEMA_USER_WRITE` / `SCHEMA_USER_READ` 指向有权限的账号；种子里补授权后即可跑。
3. **没有第二个实体**：`modules/sandbox/manifest.json` 目前只有 `demoItem`；AC-S-085 的「两个实体」E2E 用例
   （默认 `demoNote`，可用 `SCHEMA_SECOND_ENTITY` 覆盖）会 skip。

另外两处**未做**（范围外，如实登记）：

- **AC-S-085 步骤 5 的源码哈希前后比对（TC-012-050）**：需要在「加 E2 前后」各算一次哈希，
  依赖后端 E2 就绪。本棒用更强的判据替代：`zero-code.test.ts` 断言前端源码**完全不含**实体键/实体专属文案
  （哈希只能证明文件没变，证明不了没有按实体分支）。
- **`capabilities/index.json` 的 `ui_table_component` 说明更新**（D106 同步要求）：
  `capabilities/` 被明确划给后端棒，本棒**未改**。需要后端棒重生成能力索引时带上这句话：
  「`@base/ui` 的 Table 是**页码分页**的只读表格；**游标分页**请用 `apps/shell` 的 `SchemaTable`」。

> 说明：本棒为验证在 `base_test` 上跑过 `migrate up`（新增的 demo 迁移）与 `seed-e2e.sh`（幂等）；
> 验证用的 `8093` 服务已停止（未占用端口）。

---

## 6. 文件清单（本棒改动）

```
 M packages/schema/src/index.ts
 M packages/schema/test/schema.test.ts
 M apps/shell/package.json                 （+ @base/schema workspace 依赖）
 M apps/shell/src/App.svelte
 M apps/shell/src/app.css                  （只加类名 + CSS 变量，零颜色字面量）
 M apps/shell/src/lib/router.ts            （:param 扩展，D104；现有路由语义不变）
 M apps/shell/src/lib/routes.ts
 M pnpm-lock.yaml                          （仅 workspace 链接，+3 行）
 ?? apps/shell/src/lib/schema-api.ts
 ?? apps/shell/src/components/SchemaTable.svelte
 ?? apps/shell/src/components/SchemaForm.svelte
 ?? apps/shell/src/pages/SchemaPage.svelte
 ?? apps/shell/test/router-params.test.ts
 ?? apps/shell/test/schema-api.test.ts
 ?? apps/shell/test/schema-form.test.ts
 ?? apps/shell/test/schema-table.test.ts
 ?? apps/shell/test/zero-code.test.ts
 ?? e2e/schema-driven.spec.ts
 ?? feature-meta/dev-report-frontend.md    （本文件）
```

**未改**：`crates/`、`modules/`、`capabilities/`、`AGENTS.md`。

---

## 7. 假设

1. `GET /api/schema/<module>/<entity>` 的响应**包含 `api` 子段**（D102）。实测后端已如此；
   前端对缺 `api` 的响应**显式报错**，不用约定式路径去猜（D102 的理由：猜不对 `person→people`）。
2. 列表响应为 `{ items, nextCursor }`；`nextCursor` 为 `null` 表示末页。
   前端同时容忍裸数组与 `next_cursor`（Rust 字段名直译），形状不对时显示空页而不是白屏。
3. 列表按后端稳定顺序返回；新建后前端刷新**当前页**（不保证新记录一定在当前页 —— E2E 用唯一标识查找）。
4. 无新增 npm 依赖：`schema` 包零依赖；`apps/shell` 只新增 `@base/schema` 的 **workspace** 链接（非外部包）。
