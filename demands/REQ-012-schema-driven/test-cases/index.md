# REQ-012「元数据驱动渲染」—— 测试用例总索引

- **需求编号**: REQ-012「元数据驱动渲染」
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: AC-018（schema 驱动零代码 CRUD）
- **验收基线**: [acceptance.md](../acceptance.md) —— **AC-S-076 ~ AC-S-087（12 条）**，本用例集只覆盖这一段
- **设计依据**: [design-summary.md](../design-summary.md)（rev 1，2026-11-19 15:25）
- **需求原文**: [demand.md](../demand.md)
- **契约**: [api-conventions v2](../../../contracts/api-conventions/v2.md)、[extension-points v2](../../../contracts/extension-points/v2.md)
- **代码基线**: `code/` @ `develop` @ `a7f7579`（REQ-011 合并点 + CI 修复）—— **本需求尚未实现**
- **状态**: 待实现（用例本身**尚未执行**，不得据此判定需求通过）
- **版本**: 第 1 版（rev 1）—— QA 独立复核 acceptance.md rev 1，逐条核对后端源码

> ⚠️ **本文件同时是「用例集」与「挑刺报告」**。§8 是 QA 对 12 条 AC 的**逐条挑刺裁定表** ——
> **12 条 AC 全部需要修订**，其中 **7 条含高严重度硬伤**。建议先出 `acceptance.md` rev 2 再开工（§8.7）。

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **60**（TC-012-001 ~ TC-012-060，编号连续无跳号） |
| 覆盖 AC | **12 / 12**（AC-S-076 ~ AC-S-087，无遗漏、无多余） |
| 挑出问题数 | **40**（高 **12** / 中 **15** / 低 **13**） |
| 自动化分布 | 可自动化 = **60**、暂不可自动化 = **0**（但有 2 条依赖 BA 先钉死前提，见 §6） |
| 层分布 | rust-integration = **27**、e2e = **20**、rust-unit = **10**、frontend-unit = **3** |
| 类型分布 | 功能 = **33**、安全 = **17**、可维护性 = **10** |
| 优先级分布 | P0 = **45**、P1 = **15** |
| 用例文件 | 6 个（另本索引 1 个） |
| 每条用例字段 | id / ac / title / preconditions / steps / expected / type / layer / automatable / priority / notes |

> **比照前几轮**：REQ-009 挑出 22 条、REQ-010 36 条、REQ-011 40 条。本轮 **40 条**，其中
> **2 条是 BA 自己写错的**（#11 权限点四段、#1 校验顺序与既有实现冲突）、
> **6 条是纯硬伤**（#1 #2 #5 #6 #11 #12）。

### 1.1 本轮最危险的三个结论（先看这三条）

1. **元数据里没有「数据接口地址」**（#5）—— `schema` 段只有 label/fields，`demand.md` 又把「通用数据接口」明确排除。
   `SchemaTable` 该向哪个 URL 取列表？`SchemaForm` 该 POST 到哪？**全篇没有答案**。
   AC-S-082 / AC-S-086 的每一条都建立在「有这样一个接口」之上。
2. **服务端没有任何 demo 实体的 CRUD，sandbox 也没挂进平台服务**（#6）—— 现有 E2E 夹具里
   `/api/sandbox/*` 是 404。AC-S-086 的「打开列表页 → 新建 → 编辑 → 删除」今天无法执行。
3. **「零代码」依赖的 `/schema/:moduleId/:entityKey` 通配路由，现有路由器做不到**（#12）——
   `router.ts:101` 是精确字符串匹配。做这条 AC 必须先改造 REQ-011 已合并的代码，AC/design 都没承认。

---

## 2. 文件清单

| 文件 | 能力域 | AC 范围 | 用例数 |
|---|---|---|---|
| [manifest-schema.json](manifest-schema.json) | manifest 的 schema 段解析与启动校验 / 能力索引自描述 | AC-S-076、AC-S-077 | 13（001 ~ 013） |
| [schema-api.json](schema-api.json) | 元数据读取接口 / 按权限过滤元数据 | AC-S-078、AC-S-079 | 11（014 ~ 024） |
| [field-permission.json](field-permission.json) | 序列化器剔除无权字段 / 契约 C-6 | AC-S-080、AC-S-081 | 11（025 ~ 035） |
| [frontend-render.json](frontend-render.json) | `SchemaTable` / `SchemaForm` / 前端不判权限 | AC-S-082、AC-S-083、AC-S-084 | 12（036 ~ 047） |
| [zero-code.json](zero-code.json) | 新增实体零前端代码 / 零代码 CRUD 闭环 | AC-S-085、AC-S-086 | 8（048 ~ 055） |
| [evolution.json](evolution.json) | 元数据演进容错（M1 ~ M5） | AC-S-087 | 5（056 ~ 060） |

---

## 3. 夹具约定

> **原则 1**：本需求**没有任何 demo 实体**。§3.1 ~ §3.4 是 **QA 提出的夹具**，
> 其中**加粗标「待钉死」的项必须由 BA 写进 acceptance.md / design-summary.md**，
> 否则 Dev 只能自选，用例的期望值就失去依据（对应 §8 的 #5、#8、#9、#18、#19、#25、#27、#32）。
>
> **原则 2**：造数走管理员连接（`base_test`），业务访问走 `base_app`（超管会绕过 RLS，
> `base serve` 的启动自检会直接拒绝 —— `main.rs:256`）。
>
> **原则 3**：用例不得依赖执行顺序；造数**必须**用独立的幂等播种脚本（D101）。

### 3.1 实体与字段（模块 `sandbox`，实体 `demoItem`）

字段顺序即元数据声明顺序（表头顺序依赖它）：

| # | key | label | type | 其它属性 |
|---|---|---|---|---|
| 0 | `id` | `ID` | `text` | `readonly: true` |
| 1 | `title` | `标题` | `text` | `required: true`、`maxLength: 200` |
| 2 | `amount` | `金额` | `number` | `min: 0` |
| 3 | `paid` | `已付款` | `boolean` | —— |
| 4 | `dueAt` | `到期日` | `date` | —— |
| 5 | `ownerId` | `负责人` | `reference` | `reference: { module: "org", entity: "user" }` |
| 6 | `secret` | `内部备注` | `text` | `permission: "sandbox:demo:secret"` |

实体级：`label: "示例条目"`。

> ⚠️ **`permission` 必须是 3 段**（`manifest.rs:367-370`：`parts.len() == 3`）。
> design §2.1 写的 `sandbox:demo:secret:read` 是 **4 段 → 平台会拒绝，进程起不来**（挑刺 #11）。
> 本用例集统一用 `sandbox:demo:secret`，并在 sandbox 的 `permissions[]` 里以 `type: "field"` 登记。

### 3.2 数据接口（**待钉死** —— 挑刺 #5）

AC 与 design 都没有规定元数据对应的数据接口。为了让 AC-S-080/082/086 可执行，本用例集**假定**以下形状，
**BA 必须把它写进 design-summary.md（或写进 schema 段）**：

| 用途 | 方法 + 路径 | 响应 |
|---|---|---|
| 列表 | `GET /api/sandbox/items?cursor=<opaque>&limit=<n>` | `{ "items": [...], "nextCursor": "<string \| null>" }` |
| 创建 | `POST /api/sandbox/items` | 201 + 完整记录（含服务端生成的 `id`） |
| 更新 | `PATCH /api/sandbox/items/{id}` | 200 + 完整记录 |
| 删除 | `DELETE /api/sandbox/items/{id}` | 204 |

> - 参数名钉死为 `cursor` / `limit`；信封键钉死为 `items` / `nextCursor`（挑刺 #19）。
> - 实体键 `demoItem` → 路径段 `items` 的**转换规则未定义**（挑刺 #5）。若 BA 选择「路径直接用实体键」，
>   则改成本 `/api/sandbox/demoItem`，用例里的 URL 正则同步改。
> - 记录字段：`id / title / amount / paid / dueAt / ownerId / createdAt`，另有一个**未写进元数据**的
>   `internalNote`（专供 TC-012-028 验白名单 fail closed）。

### 3.3 页面路由（**待钉死** —— 挑刺 #12）

`/schema/sandbox/demoItem`、`/schema/sandbox/demoNote`。

> 现有路由器只做精确匹配（`router.ts:101`），且 `RouteDefinition.permission` 是**静态**权限点（`router.ts:31`），
> `App.svelte:116-120` 按 `resolution.path` 精确分派。通配路由 + 动态段取值是本需求**必须先做**的改造。

### 3.4 权限点与用户

| ID | 说明 |
|---|---|
| `sandbox:demo:read` | 已存在（`sandbox/manifest.json`） |
| `sandbox:demo:write` | 已存在 |
| **`sandbox:demo:secret`** | **新增**，`type: "field"` —— 必须走 D67 三件套：manifest 声明 + **权限点种子迁移** + 重生成 `capabilities/`（挑刺 #20） |

| 用户 | 角色 | 用途 |
|---|---|---|
| U0 | 无任何角色 | 「已认证即可读元数据」（TC-012-021） |
| U1 | 有 `sandbox:demo:read`、**无** `sandbox:demo:secret` | 字段级权限主用例（TC-012-022 ~ 027、046 ~ 047） |
| U2 | 有 `sandbox:demo:read` + `sandbox:demo:secret` | 全字段可见基线（TC-012-014 ~ 016、036 起） |

> 授权动作**必须**走 `/api/rbac/*` 接口（会 bump DB epoch）；**禁止**直接 INSERT —— 否则
> 「立即生效」永远不成立而实现是对的（挑刺 #20）。

### 3.5 元数据快照（AC-S-087 专用，**全部是前端合成，不经后端**）

| 快照 | 差异 |
|---|---|
| `M1` | §3.1 的 7 字段 |
| `M2` | M1 + `{key:"memo",label:"备注",type:"text"}`（末尾追加） |
| `M3` | M1 去掉 `secret`（6 字段） |
| `M4` | M1，但 `amount.type` 改成 `"currency"` |
| `M5` | M1（数据行缺值，元数据不变） |

> `M4` **不能**来自运行中的后端：AC-S-076 会让任何含 `currency` 的 manifest 启动失败（挑刺 #22）。

### 3.6 播种数据

| 场景 | 数据 |
|---|---|
| 列表/分页（TC-012-025、039） | 5 条：`item-1` ~ `item-5`，`item-1.secret = "内部-A"` |
| 空态（TC-012-040） | 0 条 |
| 演进（TC-012-056 ~ 060） | 纯前端 fixture，不落库 |

---

## 4. 执行层次与命令

| layer | 命令 | 说明 |
|---|---|---|
| `rust-unit` | `cargo test -p platform-core` | schema 段解析/校验、`SchemaProjector`、xtask 产物断言 |
| `rust-integration` | `cargo test -p platform-server` / `-p sandbox` / `-p xtask` + `scripts/test-integration.sh` | 元数据接口、字段级权限、C-6、进程级 fail-fast |
| `frontend-unit` | `pnpm --filter @base/schema test` | `formatCell` / `validateForm` / `buildListQuery` 纯函数 |
| `e2e` | `pnpm test:e2e`（Playwright，`reuseExistingServer`） | 渲染、空态、CRUD 闭环、DOM 断言、哈希对比 |

> **为什么 DOM 断言全在 e2e**：本仓 `devDependencies` 没有 `jsdom` / `@testing-library/svelte`（`package.json`），
> design R6 又要求「前端零新增依赖」。组件级 DOM 只能真浏览器验（挑刺 #24）。
>
> **e2e 前提**：`DATABASE_URL_TEST`（管理员，库名 `base_test`）+ `scripts/seed-e2e.sh` 幂等播种。
> 服务端必须能提供 `/api/sandbox/*` —— 见挑刺 #6，**今天做不到**。

---

## 5. 本需求的假绿清单（每条都有对应挑刺）

| 陷阱 | 会发生什么 | 正确写法 | 挑刺 |
|---|---|---|---|
| **子串匹配「字段名不出现」** | 响应必有 `"label"` 键，`b` 永远命中 → 要么永远红，要么被改成恒真 | 用**带引号边界的 JSON token** + 顶层键集合精确断言 | #4 |
| **先跑生成器再跑 `--check`** | 生成器会写盘并 prune → `--check` 必然 0 | 前置断言 `git status --porcelain` 为空 | #13 |
| **只比 `apps/shell/src` 的哈希** | 在 `packages/schema` 里为 E2 加特判 → 哈希不变 | 范围含 `packages/schema/src/**` + `package.json`，另加全文检索 | #17 |
| **`record.b == null` 当「字段不存在」** | 置 null 的实现也过 | `hasOwnProperty === false` + 原文不含 `"secret":` | #25 |
| **「不等于第一页」当分页生效** | 换页取到空数组也过 | 断言**精确 id 数组** | TC-012-039 |
| **`console.warn` 存在即算「有告警」** | `console.warn("oops")` 也过 | 断言**恰好 1 次**且参数含类型名与字段 key | TC-012-059 |
| **请求集合用中文名当路径** | 无法执行；或把基线请求也算进去 → 必红 | 正则白名单 + 基线请求集剔除 | #10 |
| **C-6 用「字面文本出现」判定** | 注释/字符串里抄一遍就过 | 剥注释后按调用形态匹配（照 `authz_scan` 写法） | TC-012-034 |
| **「页面有内容」当「没崩」** | 一个字段降级把整表清空也算有内容 | 断言其余列与基线**逐列相等** | TC-012-059 |
| **新权限点只改 manifest** | 进程起不来（对账失败） | D67 三件套：manifest + 种子迁移 + 重生成 | #20 |

---

## 6. 依赖 BA 先钉死的前提（否则用例写不下去）

| # | 待钉死项 | 影响用例 | 挑刺 |
|---|---|---|---|
| 1 | 元数据 → 数据接口的映射（路径/方法/信封键/实体键→路径段的转换） | 025 ~ 030、039、048 ~ 055 | #5 #19 |
| 2 | 通配路由与它如何绑定权限点 | 036 起全部 e2e | #12 |
| 3 | `reference` 下拉的选项接口（路径/形状/权限/上限） | 041 | #8 |
| 4 | boolean 文案、date 格式与 locale、错误文案字面量 | 037、042、043 | #18 #27 |
| 5 | 「未知 type」的元数据只能来自前端 fixture | 056 ~ 060 | #22 |
| 6 | 平台服务要不要挂业务模块（sandbox） | 048 ~ 055 | #6 |

> 上述 6 项中 1、2、6 属于**范围/架构决策**，不是措辞问题：`acceptance.md` rev 2 必须先回答它们。

---

## 7. 挑刺口径

| 严重度 | 定义 |
|---|---|
| **高** | 照这条做会做出错的东西，或这条根本无法成立 |
| **中** | 不可判定、可能假绿、覆盖不足 |
| **低** | 表述问题、边界不清、与其他条重叠 |

| 类型 | 含义 |
|---|---|
| **硬伤** | AC 描述的行为与后端现状 / 现有构建链冲突，按字面实现必错 |
| **不可判定** | 步骤与预期无法一一对应，或缺可机械断言的判据 |
| **假绿** | 断言方式会让错误实现通过（否定式、开放集合、子串、空集） |
| **矛盾** | 与同需求的其他 AC、或与 design / 后端契约互相打架 |
| **覆盖不足** | 前置条件缺失、失败路径漏测 |
| **边界不清** | 同一件事在 AC/design/代码里三处表述不一致 |

---

## 8. 挑刺裁定表（40 条）

> 证据行号基于 `code/` @ `a7f7579`。`acceptance.md:xx` 指 rev 1 原文行号。

### 8.1 高（12 条 —— 硬伤 / 不可判定 / 假绿 / 矛盾）

| # | AC | 问题 | 严重度 | 建议修订 | 类型 |
|---|---|---|---|---|---|
| 1 | AC-S-076 + design §2.4 | **「按固定顺序、首个失败即退出」与现有 `Manifest::validate()` 的聚合语义冲突。** `parse_checked` 返回 `Vec<ManifestError>`、`validate()` 收集**全部**问题（`manifest.rs:230-243`），三处调用点把它们 `join("; ")` 成一行（`platform-config/src/registry.rs:118-126`、`platform-rbac/src/registry.rs:130-137`、`platform-db/src/scopes.rs:83-90`）。design §2.4 与 status.md 都把它写成「关键设计选择」，但它**既不可实现也不可验证** | 高 | 二选一并写死：① 保留聚合语义 → design 删掉「首个失败即退出」，AC 只在**单条违规**场景断言（本用例集已按此写：`issues.len() == 1`）；② 真做短路 → 要改 `validate()` 的返回类型与三个调用点（属于对已合并代码的破坏性改动，必须写明） | 硬伤 |
| 2 | AC-S-076 + design §2.4 第 5/6 条 | **跨 manifest 的规则放不进 `validate(&self)`。** 校验函数没有上下文（`manifest.rs:242`），而三个扫描点都是**逐文件** `parse_checked`（`registry.rs:108-137`、`rbac/registry.rs:121-137`、`scopes.rs:76-99`）。于是「`permission` 必须是已登记权限点」「`reference` 的 module/entity 必须存在」只能退化成「只查本文件」—— 跨模块引用会**误报**；要正确判定就必须新增一个「先收集全部 manifest 再校验」的注册表阶段，AC 与 design 一个字都没写 | 高 | 在 design 里加一节「schema 注册表」：先建全量实体索引，再做跨文件校验；`validate()` 只保留单文件规则（格式/类型/重复 key）。AC 的步骤要区分「单文件校验」与「注册表校验」两类失败 | 硬伤 |
| 3 | AC-S-076 步骤 4/6/8 | **「进程起不来」没有可执行的形状，且步骤 2 自相矛盾。** `serve()` 在解析 manifest **之前**先跑租户隔离自检（`main.rs:256`，需要真库且连接角色不得绕过 RLS）；`cargo test` 侧没有 spawn 服务二进制的先例（唯一的 spawn 先例是拉起**测试二进制**做防假绿护栏，`platform-db/tests/quota_real_db.rs:229-262`）。另外步骤 2 同时要「退出码 0」与「服务可达」—— 常驻进程无法同时观测这两件事 | 高 | ① 步骤 2 改为「`/health/ready` 返回 200，随后 SIGTERM，退出码 ∈ {0,143}」；② 步骤 4/6/8 写明：`BASE_MANIFEST_ROOT=<临时副本>` + `DATABASE_URL_TEST`，退出码 == 1（`EXIT_RUNTIME`，`main.rs:45`），stderr 含字段路径；③ 若不想依赖真库，可用 `base tenant-policies --check`（`platform-server/src/policies.rs:39-44` 先 `collect_scopes` 解析 manifest，**解析失败发生在连库之前**） | 不可判定 |
| 4 | AC-S-079 步骤 3 | **「响应体全文不出现字段名 `b`」必然误报。** 响应体一定含 `"label"` 键（design §3.2 每个 field 都有 label；`capabilities/schema/metadata.schema.json` 的 required 就含 `label`）—— 子串 `b` 命中 `label`。若为了让它过而改成「不出现 label 文本」，就会漏掉「不得有 `hiddenFields` 清单」这半条 | 高 | 拆成 4 条机械判据：① `fields.map(f=>f.key)` 深度相等；② 顶层键集合精确为 `[entity,fields,label,module]`；③ `"<被剔除的 key>"` 作为**带引号边界的 JSON token** 不出现；④ 不含 `hiddenFields`/`deniedFields`/`omitted`/`total`。并在夹具约定里写明「label 文案用中文且互不为子串」 | 假绿 |
| 5 | AC-S-082 步骤 4 + AC-S-086 全条 | **元数据里没有「数据接口地址」，零代码闭环没有数据源。** `schema` 段只有 `label`/`fields`（design §2.1），§3 只有元数据接口，而 `demand.md` 明确把「通用数据接口」排除（数据访问由业务 `Repository` 承担）。于是 `SchemaTable` 该向哪个 URL 取列表、`SchemaForm` 该 POST 到哪，**全篇没有答案**；`buildListQuery(cursor, limit)` 只造了参数，没有路径 | 高 | 在 schema 段增加 `api: { list, create, update, delete }`（模板化路径），**或**在 design 里写死约定 `/api/<moduleId>/<路径段>` 并给出 `demoItem → items` 的转换规则。二者必选其一，否则 AC-S-082/085/086 都无法实现 | 硬伤 |
| 6 | AC-S-086 | **服务端没有 demo 实体的 CRUD，sandbox 也没挂进平台服务。** sandbox 只有 5 条路由（`modules/sandbox/backend/src/http.rs:157-175`：demo read/write、scope-item list/get/approve/export），**没有 create/update/delete**；列表直出裸数组（`:195-207`，不是游标信封）；`platform-server` 只挂 iam/org/rbac/config（`main.rs:295-312`），`Cargo.toml` 也不依赖 sandbox → 现有 E2E 夹具（`scripts/e2e-server.sh`）里 `/api/sandbox/*` 是 **404** | 高 | BA 必须拍板：① 本期在 `platform-server` 里挂业务模块（跨需求改造，且要回答「为什么生产二进制要挂示例模块」）；② 或把 AC-S-086 的 E2E 主体改为「模块级装配的见证服务」，并承认它不是真实部署形态 | 硬伤 / 越界 |
| 7 | AC-S-082 步骤 4/5 | **与 `@base/ui` Table 的契约直接冲突。** Table 恒渲染 `<thead>`（`Table.svelte:26-34`），空数据时渲染一行 `emptyText`（`:46`）—— 而 AC 要求空态**不渲染表头 + 0 行**；分页控件只在 `pageCount > 1` 时出现（`:52`），而 `pageInfo` 需要 `total`（`packages/ui/src/index.ts:287-295`），**游标分页没有 total** → 下一页按钮不会出现。同时 `capabilities/index.json` 写着「勿自造表格渲染」 | 高 | 二选一：① `SchemaTable` **不使用** `@base/ui` Table（则同步改 `capabilities/index.json` 的 `ui_table_component` 措辞，明确「Table 是只读分页表格，SchemaTable 是独立渲染器」）；② 给 Table 增加 cursor 模式（改 REQ-011 的组件契约 + 用 `nextCursor` 是否存在来决定按钮可用性） | 矛盾 |
| 8 | AC-S-083 步骤 2 | **`reference` 的数据源不存在。** design §2.1 只声明 `{module, entity}`；§7 R5 说「由业务模块提供选项接口」，但**没有路径、没有形状、没有权限点、没有条数上限/分页**；`capabilities/api.json` 仍是 `paths: []`；**没有任何 AC 覆盖这个接口**。所以「下拉（数据源来自元数据声明）」这句无法判定 —— 元数据里根本没声明「怎么拿到选项」 | 高 | 要么把 `reference` 本期降级为「只读 id 文本」（并改 AC-S-083 步骤 2），要么补一条 AC 定义选项接口（路径模板、响应形状、权限点、上限）。不可保留「由业务模块提供」这种无主体表述 | 不可判定 |
| 9 | AC-S-080 前置条件 | **「列表接口」未指名，而现有唯一的列表接口无法照用。** `GET /api/sandbox/scope-item`（`http.rs:164-166`）直出 `ScopeItemResponse`（`:195-207`），该实体**没有任何字段级权限**，且属于**已合并的 REQ-009** 范围 —— 改造它就是动别人的交付物 | 高 | 在 AC 里钉死「新建一个 demo 实体的列表接口（§3.2）」，并写一句「不改造 `scope-item`」 | 不可判定 / 越界 |
| 10 | AC-S-086 步骤 8 | **「请求路径集合 ⊆ {元数据接口、列表、创建、更新、删除}」不可执行且必然误报。** 五个中文名不是路径；元数据路径含动态段；而「全程」至少还包含 `/api/config/branding`（`App.svelte:61` 启动即取）、`/api/rbac/me/permissions`、`/health/*` 与静态资源 → 照字面实现必红 | 高 | 改为**正则白名单 + 基线请求集剔除**：`^/api/schema/sandbox/demoItem$`、`^/api/sandbox/items$`、`^/api/sandbox/items/[^/]+$`；基线排除 branding/health/rbac/iam/静态资源（TC-012-055 已给出） | 不可判定 |
| 11 | design §2.1（BA 自错） | **示例权限点是 4 段，平台要求恰好 3 段。** design 写 `"permission": "sandbox:demo:secret:read"`；`is_permission_code` 要求 `parts.len() == 3`（`manifest.rs:367-370`），JSON Schema 的 pattern 也是 3 段（`manifest_schema.rs:41`）→ **照抄 design 的样例，启动就失败**。AC-S-079/080 又换成 `demo:secret:read`（3 段但缺模块前缀，与仓内 22 个权限点的 `<module>:<resource>:<action>` 惯例不一致） | 高 | 统一为 `sandbox:demo:secret`，`type: "field"`；并明确「本模块的字段权限点必须在本模块 `permissions[]` 登记」（这条同时是 #2 的输入） | 硬伤（BA 自错） |
| 12 | AC-S-085、AC-S-086 | **「零代码」依赖的通配路由，现有路由器做不到。** `router.ts:101` 是 `routes.find(route => route.path === path)` 精确匹配；`RouteDefinition.permission` 是**静态**权限点（`router.ts:31`），动态段无法声明；`App.svelte:116-120` 按 `resolution.path` 精确分派。design §5.2 的「前端只有一条通配路由」今天不存在 | 高 | 在 design 里承认这是对 REQ-011 交付物的**语义改造**（路由匹配 + 动态段取值 + 动态路由的权限策略），并写清 `/schema/:moduleId/:entityKey` 的权限怎么判（design R3 只写了「沿用 REQ-011 守卫」，但守卫要一个静态权限点） | 硬伤 / 越界 |

### 8.2 中（15 条）

| # | AC | 问题 | 严重度 | 建议修订 | 类型 |
|---|---|---|---|---|---|
| 13 | AC-S-077 步骤 1→3 | **步骤 1 会让步骤 3 恒绿。** 不带 `--check` 的 `cargo xtask capabilities` 会 `write_files` + prune **写盘**（`xtask/src/main.rs:94-127`）→ 先跑步骤 1，磁盘已被重写成生成结果，步骤 3 的 `--check` 必然退出 0 | 中 | 步骤 3 前置：「`git status --porcelain` 为空」或「先 `git checkout -- capabilities/`」。本用例集已加此前置（TC-012-013） | 假绿 |
| 14 | AC-S-077 步骤 4 | **「（或等价命名）」不可判定，且与已存在的 `metadata.schema.json` 混淆。** `capabilities/schema/metadata.schema.json` 已存在，是**字段级** schema（title「Base Platform Field Metadata」，由 `capabilities.rs:344-372` 从 TS 生成）；而本条要的是**实体级** schema。同时 `prune_extra_files`（`capabilities.rs:179-195`）会删除未注册的产物 → 新文件名必须进 `generate()` 的列表，否则 `--check` 报「多余文件」 | 中 | 钉死 `capabilities/schema/entity.schema.json`，并写进 `generate()`；同时说明它与 `metadata.schema.json` 的分工 | 不可判定 |
| 15 | AC-S-077 步骤 2 | **「与 Rust 实现完全一致」少一条边。** 字段类型枚举的生成来源是 **TS**：`metadata_schema_json` 调 `string_union(source, "FieldType")` 读 `packages/schema/src/index.ts`（`capabilities.rs:344-372`）。Rust 侧目前没有 `FieldType` 枚举（design 要新增）。现有测试 `manifest_schema_matches_the_platform_constant`（`capabilities.rs:773`）只比「常量 vs 由同一常量生成的产物」—— 对 Rust↔schema 是**恒真** | 中 | 增加一条三方一致性测试：Rust `FieldType` 变体集 == TS 联合 == 生成 JSON 的 enum（排序后逐项相等） | 假绿 |
| 16 | AC-S-081 步骤 3 | **条件是「待定」，不是结论。** AC 自己写「实施前必须在 design 里选定一种并写死，不允许含糊」—— 但 AC 是**验收判据**，不能留待定项。而且 design §4.4 已经选定「至少一处」（`design-summary.md:211-218`）并写了局限（:220-227），AC 没同步 → 照 AC 字面无法判定通过还是失败 | 中 | 步骤 3 的预期改成固定的「**通过**」；把「3 个 handler 只有 1 个调用也能过」写成一条**显式**用例（TC-012-033），并在 AC 里保留一句局限说明（C-6 抓不到这种情形，由 AC-S-080 的详情路径用例兜底） | 不可判定 |
| 17 | AC-S-085 步骤 5 | **哈希范围过窄。** 渲染器的**纯逻辑**在 `packages/schema/src/**`（design §5.1），只比 `apps/shell/src/**` 时，在那个包里为 E2 加特判不会被发现；另外 `apps/shell` 当前**没有** `@base/schema` 依赖（`apps/shell/package.json:13-20`）→ 实现时必然修改 `package.json`（不在 `src/**` 内，但属于前端改动）。补充：`dist/` **不必**比（比的是源码，构建产物天然会变）—— 你的担心 3 的真正问题不是 `dist`，是**范围** | 中 | 范围写成 `apps/shell/src/**` + `packages/schema/src/**` + `apps/shell/package.json`；再加一条更强的判据：前端源码全文不含 E2 的实体键/label（TC-012-051） | 假绿 |
| 18 | AC-S-082 步骤 3 | **格式化文案/格式/locale 全未定。** 只写「`boolean` → 是/否；`date` → 本地化日期」。仓库没有文案表（`packages/i18n/src/index.ts` 只有 `translate/hasMessage`，字典由调用方提供），现有组件是硬编码中文（`Table.svelte:46,59-65`）；`@base/ui` 的 `cellValue` 直接 `String(value)`（`packages/ui/src/index.ts:350-357`）→ boolean 输出 `true` | 中 | 写死：`true → "是"`、`false → "否"`、date 固定 `YYYY-MM-DD` 且**不做时区换算**、number 用十进制无分组。实现上必须走 `formatCell`，不得沿用 `cellValue` | 不可判定 |
| 19 | AC-S-082 步骤 4 | **`nextCursor` 的名字与信封形状未定。** `List<T>` 只有 Rust 字段 `next_cursor`（`repository.rs:155-160`），没有 JSON 形状；现有列表实现返回 `Vec`（`scope_item.rs:105-111`），根本不是 `List`。仓储层与 HTTP 层没有任何一处定义过 `{items, nextCursor}` | 中 | 钉死信封 `{ "items": [...], "nextCursor": string \| null }` 与查询参数 `cursor` / `limit`（TC-012-038 已写死） | 不可判定 |
| 20 | AC-S-079 步骤 4、AC-S-080 步骤 3 | **「立即生效」缺授权动作与迁移前提。** RBAC 的失效主机制是 DB epoch（`platform-rbac/src/checker.rs:1-12,113-142`），只有走接口授权才 bump；直接写库则「立即生效」永远不成立。另外新权限点必须满足 **D67 三件套**（manifest + 种子迁移 + 重生成 capabilities），否则 `reconcile_permissions` 让进程起不来（`main.rs:308`、`:558-580`），既有测试 `config_real_db.rs:1599` 也会红 | 中 | 前置条件补两条：①「授权走 `/api/rbac/*`」；②「`sandbox:demo:secret` 已建种子迁移」。否则 Dev 一改 manifest 就发现服务起不来 | 覆盖不足 |
| 21 | AC-S-078 前置条件 | **与 design §3.1 矛盾。** AC 前置写「用户 U1 拥有该模块的读权限点」，design §3.1 写「**不新增权限点** —— 走已认证即可读 + 字段级过滤」，理由还写得很清楚（否则前端在拿到元数据前要先知道该要什么权限）。而且没有任何用例验「无模块权限的已认证用户也能 200」 | 中 | 二选一并写死。若选 design 版本：删掉 AC 的前置条件，补一条用例（TC-012-021）。若要求权限点：design §3.1 整段作废，并且要回答「前端怎么知道该请求哪个权限」 | 矛盾 |
| 22 | AC-S-087 步骤 4 | **「未知 type = currency」与 AC-S-076 冲突。** 真实 manifest 写 `currency` 会在启动时被拒（AC-S-076 步骤 3）→ M4 只能来自前端合成元数据；AC 没说数据来源。design §2.2 又说「加类型是向后兼容的」，但没说加类型要**同时**改 Rust 枚举 + TS 联合 + 生成 schema（否则 AC-S-077 的枚举断言会红） | 中 | 写明「M1~M5 全部是前端 fixture，不经后端」；design §2.2 补一句「新增类型的改动清单：Rust 枚举 + `@base/schema` 的 `FieldType` + manifest schema 的 enum（三处同 PR）」 | 矛盾 |
| 23 | AC-S-087 步骤 3 | **「数据里仍可能带该键」与白名单投影冲突。** design §4.2 明写投影后「只含元数据声明过、且当前用户有权的字段」，未声明字段默认不输出（`design-summary.md:196-199`）→ 后端**根本不会下发**被删掉的字段，这个场景在真实链路上不存在 | 中 | 改成「**前端合成数据**里带该键」；并在 AC 里说明这条守的是前端容错，不是后端行为 | 矛盾 |
| 24 | AC-S-084 | **是同义反复，且 DOM 断言缺环境。** 前置「元数据已由后端按权限过滤」→ 输入本身就是过滤后的，断言「渲染时没有 b」只能证明渲染器不多造字段，**不能证明权限链路**。另外步骤 4 要 DOM 断言，但 `package.json` 的 devDependencies 没有 `jsdom` / `@testing-library/svelte`，design R6 又要求「前端零新增」→ 只能走 Playwright | 中 | 在「说明」里写清依赖链：AC-S-079（后端过滤）→ 元数据 → 渲染器忠实渲染；后端过滤的 bug 由 AC-S-079/080 的用例负责，本条不承担。并把 layer 标为 e2e | 假绿 |
| 25 | AC-S-080 | **投影对列表/信封的语义未定义。** design §4.2 的签名是 `project(raw: Value, schema, ctx, checker) -> Value`，只描述**单对象**；列表返回 `Vec`（`scope_item.rs:105-111`）或信封时，谁负责逐条 map、`nextCursor` 会不会被白名单剔掉，都没写 | 中 | 补 `project_list` 或写明「调用方对 `items` 逐条投影，信封其余键原样保留」（TC-012-029 守这条） | 不可判定 |
| 26 | AC-S-078 步骤 2 | **「未声明时字段可以省略，但不得是 null」是二选一。** 两个实现（省略 / 出现但非 null）都满足字面，期望值不同。design §3.2 已写「省略该键」 | 中 | 统一为「**省略该键**」，并明确「`required` 缺省为 false 时也不下发 `required: false`」（否则「未声明」与「显式 false」不可区分） | 不可判定 |
| 27 | AC-S-083 步骤 4 | **「文案指出「必填」」没有字面量。** 「必填」两个字、`标题不能为空`、`此字段为必填项` 都算「指出必填」→ 无法机械断言 | 中 | 钉死 `"<label>不能为空"`（本用例集按此写），或改为「错误文本包含字段 label 且包含『不能为空』」，二选一 | 不可判定 |

### 8.3 低（13 条）

| # | AC | 问题 | 严重度 | 建议修订 | 类型 |
|---|---|---|---|---|---|
| 28 | AC-S-078 步骤 3 | 与 AC-S-079 叠加时，「字段 key 的顺序等于声明顺序」应写成「**过滤后的子序列**保持相对顺序」—— 否则无权限场景下这条自相矛盾 | 低 | 加「过滤后」限定 | 边界不清 |
| 29 | AC-S-078 步骤 4/5 | 新错误码 `SCHEMA_ENTITY_NOT_FOUND` / `SCHEMA_MODULE_NOT_FOUND` 未在任何清单登记（现有只有 4 个码，`platform-core/src/problem.rs:33-39`）；且 `enabled: false` 的模块算「存在」还是「不存在」未定义（`manifest.rs:192-194`） | 低 | 在 problem 常量旁登记新码；明确禁用模块返回 `SCHEMA_MODULE_NOT_FOUND` | 覆盖不足 |
| 30 | AC-S-076 步骤 4/6/8 | 错误可能被外层标题误导：IAM 已配置时走 `reconcile_permissions` 包装成「✗ 权限点注册表与数据库不一致」（`main.rs:573`），未配置时走「配置键注册表加载失败」（`main.rs:471-476`）。schema 错误与权限对账**共用同一次 manifest 扫描**，标题会张冠李戴 | 低 | AC 只断言「消息体含字段路径」，并补一句「不得依赖外层标题」；建议把 schema 校验放进独立的启动阶段并给独立标题 | 边界不清 |
| 31 | AC-S-082 步骤 5/6 | 两步是同一件事（空态渲染 / 空态文案），拆成两步会让人以为要两套断言 | 低 | 合并为「空态 DOM + 文案」一条 | 重叠 |
| 32 | AC-S-083 步骤 6 | 「body 的键只含元数据声明的字段」—— 但 `id` 也是声明字段（readonly）。创建时是否提交 `id`、更新时 `id` 在 body 还是 URL，AC 未区分 | 低 | 写明：创建不带 `id`（服务端生成）、更新 `id` 只在 URL、`readonly` 字段不进 body | 边界不清 |
| 33 | AC-S-086 步骤 6 | 「点『删除』→ 确认」未指定组件与按钮文案；仓库已有 `Dialog`（`apps/shell/src/components/Dialog.svelte`） | 低 | 钉死「复用 Dialog，确认按钮文案为『确认』」 | 覆盖不足 |
| 34 | AC-S-080 步骤 2 | 判据 `hasOwnProperty.call(record, 'b')` 里的 `record` 指哪一层没写（裸数组元素 / 信封的 `items[i]`） | 低 | 随 #19 一起钉死信封形状，并写明 `record` = `body.items[i]` | 边界不清 |
| 35 | AC-S-085 步骤 1/5 | 「sha256」的算法与归一化没写：文件枚举方式、路径排序、换行（CRLF/LF）是否归一化 → 跨平台会假红 | 低 | 写死：sha256 + 相对路径字典序 + **不**做换行归一化（要求 `.gitattributes` 统一 LF） | 边界不清 |
| 36 | AC-S-077 步骤 2 | 把「manifest 的 JSON Schema 里包含 `schema` 属性定义」与「字段类型枚举与 Rust 完全一致」混成一条：前者在 `properties.schema`，后者在 `properties.schema.additionalProperties.fields.items.properties.type.enum`；而实体 schema 是**另一个产物** | 低 | 拆成两条预期（TC-012-011 与 TC-012-012 已拆） | 边界不清 |
| 37 | AC-S-079 步骤 3 | 「也不出现 b 的 label」：若 label 是其它 label 的子串（如「备注」与「备注明细」），子串断言会误报 | 低 | 夹具约定里写「label 互不为子串」 | 边界不清 |
| 38 | AC-S-078 步骤 2 | 只列了 `key/label/type/required/permission`，但 design §3.2 的示例还带 `maxLength`，design §2.3 还有 `readonly` → 这些属性算不算「如实出现」没写 | 低 | 补一句「其余已声明的属性（`readonly`/`maxLength`/`min`/`reference`…）原样透传」 | 覆盖不足 |
| 39 | AC-S-082 步骤 1 | 「不写任何业务组件」在单条内无法判定（它是 AC-S-085 的哈希判据）；本条只该验渲染结果 | 低 | 本条删掉该限定，零代码由 AC-S-085 单点负责 | 重叠 |
| 40 | AC-S-078 步骤 2 | 响应体是否含 `module` 两处不一致：design §3.2 有，AC 的形状描述没有 | 低 | 以 design 为准，AC 补 `module` | 矛盾 |

### 8.4 按类型统计

| 类型 | 条数 | 编号 |
|---|---|---|
| 硬伤 | 6 | #1、#2、#5、#6、#11、#12 |
| 不可判定 | 11 | #3、#8、#9、#10、#14、#16、#18、#19、#25、#26、#27 |
| 假绿 | 5 | #4、#13、#15、#17、#24 |
| 矛盾 | 5 | #7、#21、#22、#23、#40 |
| 覆盖不足 | 4 | #20、#29、#33、#38 |
| 边界不清 | 8 | #28、#30、#32、#34、#35、#36、#37 |
| 重叠 | 2 | #31、#39 |

> 一条问题可命中多类型（#6 硬伤+越界、#12 硬伤+越界、#9 不可判定+越界），故分项之和 > 40。

### 8.5 按 AC 统计（哪些 AC 必须改）

| AC | 问题数 | 其中高 | 结论 |
|---|---|---|---|
| AC-S-076 | 4 | 3 | **必须改**（#1、#2、#3、#30） |
| AC-S-077 | 4 | 0 | **必须改**（#13、#14、#15、#36） |
| AC-S-078 | 6 | 0 | **必须改**（#21、#26、#28、#29、#38、#40） |
| AC-S-079 | 3 | 1 | **必须改**（#4、#20、#37） |
| AC-S-080 | 4 | 1 | **必须改**（#9、#20、#25、#34） |
| AC-S-081 | 1 | 0 | **必须改**（#16） |
| AC-S-082 | 5 | 2 | **必须改**（#5、#7、#18、#19、#31） |
| AC-S-083 | 3 | 1 | **必须改**（#8、#27、#32） |
| AC-S-084 | 1 | 0 | **必须改**（#24） |
| AC-S-085 | 3 | 1 | **必须改**（#12、#17、#35） |
| AC-S-086 | 4 | 3 | **必须改**（#5、#6、#10、#33） |
| AC-S-087 | 2 | 0 | **必须改**（#22、#23） |
| 全局/design | 1 | 1 | **必须改**（#11 权限点四段落在 design；#1、#2 已计入 AC-S-076，不重复计） |

> 同一条问题可命中多条 AC（#5 同时属于 AC-S-082 与 AC-S-086；#20 同时属于 AC-S-079 与 AC-S-080），
> 故本表「问题数」之和为 **41** > 40；另 **#39** 是跨 AC 的重叠项（AC-S-082 步骤 1 与 AC-S-085 的判据重复），
> 不归属于任何单条 AC。

> **12 条 AC 全部需要修订。** 这与前三轮的结论一致：不是「写得不好看」，而是**照字面做不出来或验不了**。

### 8.6 已核实为「成立、不需要改」（避免过度挑刺）

| 事项 | 核实结论 | 证据 |
|---|---|---|
| 「字段类型只有 5 种」与前端现状一致 | **成立** —— `@base/schema` 的 `FieldType` 联合今天就是这 5 个，本需求是「加属性」（`permission`/`readonly`/`min`/`max`/`reference`），不是改枚举 | `packages/schema/src/index.ts:8` |
| 「能力索引里能收录 manifest 的 schema 段」 | **成立** —— `capabilities/schema/manifest.schema.json` 由 Rust 常量生成，加属性即可 | `manifest_schema.rs:23`；`xtask/src/capabilities.rs:336-342` |
| 「`cargo xtask capabilities --check` 能发现手改」 | **成立** —— 缺失/内容不一致/多余文件三类比对都在 | `xtask/src/capabilities.rs:87-121` |
| 「fail-fast 与 REQ-010 的 config 段同源」 | **成立** —— manifest 在启动时被**运行时**读取并 `parse_checked`（两处），把 schema 校验放进 `validate()` 就能让进程起不来，**不需要重新编译** | `platform-config/src/registry.rs:118`；`platform-rbac/src/registry.rs:130` |
| 「不带令牌 → 401 `UNAUTHENTICATED`」 | **成立** —— 码与错误体形状都有先例 | `platform-core/src/problem.rs:33,115-120`；`platform-rbac/src/http.rs:174-179` |
| 「元数据接口不新增权限点、只要已认证」在实现上可行 | **成立** —— 只需 authenticate 中间件，不需要 `require_permission`；RBAC 的注册表路由就是同款先例 | `platform-rbac/src/http.rs:162-199`（REQ-011 QA 已核实） |
| 「字段级权限与数据范围（行级）不冲突」 | **成立** —— 行级 scope 由 `checker.effective_scope` 在查询前叠加（`sandbox/http.rs:200`），字段投影发生在序列化之后，两者是不同阶段 | `sandbox/backend/src/http.rs:195-207` |
| 「C-6 可以照 C-5 的写法」 | **成立** —— `authz_scan` 提供剥注释 + 扫描调用形态的先例，`check_c5_for(manifest, src_dir)` 是不依赖 `ContractFixture` 的入口模板（D58） | `platform-contracts/src/authz_scan.rs:71,214`；`checks.rs:470-535` |
| 「AC 条数 = 12」 | **成立** —— AC-S-076 ~ AC-S-087 连续 12 条，无跳号（D23 的全局唯一段） | `acceptance.md:15` |

### 8.7 总体结论

1. **按现稿不能开工。** 12 条 AC 全部需要修订，其中 **7 条含高严重度问题**：
   AC-S-076（3 条）、AC-S-079、AC-S-080、AC-S-082、AC-S-083、AC-S-085、AC-S-086（3 条）；
   另有 2 条高严重度问题落在 **design** 上（#11 权限点四段、#1 校验顺序）。

2. **最危险的不是措辞，是三个「主体不存在」：**
   - **#5 数据接口地址**：零代码渲染没有数据源，`SchemaTable` 不知道去哪里取数；
   - **#6 服务端 CRUD 与模块装配**：AC-S-086 的闭环今天连一条路由都没有；
   - **#12 通配路由**：「零前端代码」的前提是一个现有路由器不支持的匹配语义。
   这三条都不是「用例写得不够细」，而是**需求范围与现有底座之间的缺口**。它们决定本需求能不能成立，必须先由用户拍板。

3. **#11 是 BA 自己写错的**（4 段权限点），而且后果是**照抄样例就启动失败** —— 与前三轮「BA 自己写错」是同一类。建议在 rev 2 里加一条自检：凡是 design 里出现的权限点，都必须能被 `is_permission_code` 接受。

4. **#4 是典型的「看起来更安全，实际不可执行」**：要求「全文不出现字段名」的初衷是对的（不泄露存在性），但子串匹配让它在**任何**响应上都失败（因为 `label` 含 `b`）。这类断言必须写成**结构化**判据（键集合 + token 边界），不能写成「全文不含某词」。

5. **时间成本提醒**：AC-S-076 的进程级用例（TC-012-010）需要真库 + 非超管角色，且必须用 `BASE_MANIFEST_ROOT` 的临时副本（在仓库里改 manifest 会污染既有测试）。若 CI 没有真库，这条只能降级为「校验器层 + `base tenant-policies --check` 进程层」两段式。

6. **建议顺序**：
   - **第 1 步**：BA 出 `acceptance.md` rev 2，先回答 §6 的 3 个范围问题（数据接口、模块装配、通配路由），再落 §8.1 的 12 条高严重度修订；
   - **第 2 步**：BA 在 design 里钉死 §3 的夹具（实体字段、接口路径、信封键名、格式化文案、错误文案）—— 这些是 60 条用例的期望值来源；
   - **第 3 步**：Dev 按 rev 2 实现；本用例集**可直接使用**——每条 `notes` 都写明了它守的是「现稿」还是「建议修订稿」，AC 修订后只需按 notes 回填，不需要重写。

---

## 9. AC → 用例映射

| AC | 用例 | 条数 |
|---|---|---|
| AC-S-076 | 001 ~ 010 | 10 |
| AC-S-077 | 011 ~ 013 | 3 |
| AC-S-078 | 014 ~ 021 | 8 |
| AC-S-079 | 022 ~ 024 | 3 |
| AC-S-080 | 025 ~ 030 | 6 |
| AC-S-081 | 031 ~ 035 | 5 |
| AC-S-082 | 036 ~ 040 | 5 |
| AC-S-083 | 041 ~ 045 | 5 |
| AC-S-084 | 046 ~ 047 | 2 |
| AC-S-085 | 048 ~ 051 | 4 |
| AC-S-086 | 052 ~ 055 | 4 |
| AC-S-087 | 056 ~ 060 | 5 |
| **合计** | 001 ~ 060 | **60** |

---

## 10. 用例与挑刺的对应关系（哪些用例守的是修订稿）

| 用例 | 守的是 | 依赖的挑刺裁定 |
|---|---|---|
| TC-012-010 | **修订稿**（进程层形状、真库前提、退出码 1） | #3 |
| TC-012-013 | **修订稿**（先断言工作区干净） | #13 |
| TC-012-016 | **修订稿**（未声明属性一律省略） | #26 |
| TC-012-021 | **修订稿**（design §3.1：已认证即可读） | #21 |
| TC-012-023 | **修订稿**（token 边界匹配，不是子串） | #4 |
| TC-012-025 / 029 | **修订稿**（钉死列表接口与信封） | #9 #19 #25 |
| TC-012-033 | **修订稿**（判据=至少一处 → 通过） | #16 |
| TC-012-037 | **修订稿**（是/否、YYYY-MM-DD 写死） | #18 |
| TC-012-040 | **修订稿**（空态不渲染表头 —— 与 Table 契约冲突未解前不可执行） | #7 |
| TC-012-041 | **修订稿**（reference 下拉 —— 数据源不存在） | #8 |
| TC-012-047 | **修订稿**（DOM 断言只能 e2e） | #24 |
| TC-012-048 ~ 051 | **修订稿**（数据接口 + 通配路由 + 哈希范围） | #5 #12 #17 |
| TC-012-052 ~ 055 | **修订稿**（服务端 CRUD + 模块装配 + 正则白名单） | #6 #10 |
| TC-012-056 ~ 060 | **修订稿**（M1~M5 为前端合成） | #22 #23 |
| TC-012-002 / 003 / 004 | 现稿（单条违规场景，不受聚合语义影响） | #1（仅影响 design 的「顺序」表述） |
