# BA 裁定：REQ-012 验收标准 rev 1 → rev 2

- **裁定者**: myg133-ba-1 (BA)
- **时间**: 2026-11-19 15:40
- **输入**: [test-cases/index.md](test-cases/index.md) §8 挑刺裁定表（**40 条**：高 12 / 中 15 / 低 13）
- **结论**: **39 条接受，1 条不采纳**（有证据）。rev 1 的 12 条 AC **逐条都要改**。

---

## 0. 总览

| 类别 | 条数 | 处置 |
|---|---|---|
| 高 | 12 | **全部接受**，其中 3 条是「主体不存在」（见 §1） |
| 中 | 15 | **全部接受** |
| 低 | 13 | **12 条接受，1 条不采纳**（#13，见 §4） |
| 合计 | 40 | **接受 39 / 不采纳 1** |

**其中 3 条是 BA 自己写错的**（#1 校验顺序、#4 全文匹配、#11 权限点段数）——
与前几轮同一个形状：**BA 写的规格「看起来对」但做不了。**

---

## 1. ★ 三个「主体不存在」（决定本需求能否成立）

QA 的判断是对的：这三条不是措辞问题，是**需求能不能站住**的问题。

### 1.1 #5：元数据里没有数据接口地址 → 零代码闭环无数据源

**问题**：`design §2.1` 的实体只有 `label` + `fields`，
前端拿到元数据后**不知道该向哪个接口要数据**。
`buildListQuery()` 只造参数、不造路径。

**裁定**：**接受**。schema 的实体加 **`api` 子段**（显式声明，不用约定式）：

```json
"demoItem": {
  "label": "示例条目",
  "api": {
    "list":   "/api/sandbox/demo-items",
    "create": "/api/sandbox/demo-items",
    "update": "/api/sandbox/demo-items/{id}",
    "delete": "/api/sandbox/demo-items/{id}"
  },
  "fields": [ ... ]
}
```

- `list` / `create` **必填**；`update` / `delete` **可选**（不声明 → 前端不渲染对应按钮）
- **启动校验**：每个路径必须以 `/api/` 开头（防止声明外部 URL 被前端直接请求）
- `{id}` 是唯一的占位符；渲染时替换为记录的 `id`

> **为什么显式而不是约定式**（`/api/<module>/<entityPlural>`）：
> 复数化规则要么靠猜（`item→items` 还行，`person→people` 就错），
> 要么强制命名。显式声明只多几行，但**永远不会猜错**。

**新增决策 D102**。

### 1.2 #6：服务端没有可挂载的业务模块，也没有 demo 实体 CRUD

**问题**：
- `crates/platform-server/Cargo.toml` **不依赖** `modules/sandbox`
- `main.rs:295-312` 只挂 `iam` / `org` / `rbac` / `config`
- `sandbox/backend/src/http.rs:157-175` 只有 5 条路由，**没有** create/update/delete

**裁定**：**接受**，方案选「**把 sandbox 挂进 platform-server**」。

**依据（我核实过）**：
- `modules/sandbox/backend/src/mod.rs:39` **已实现** `impl PlatformModule`
- `:70` **已注册** `register_platform_module!(SANDBOX_MODULE)`
- 平台已有 `discover_enabled()`（`registry.rs:39`）

**所以挂载是「让已经写好的参考实现真的跑起来」，不是新建能力。**

**改造范围**（写进 design）：
1. `platform-server/Cargo.toml` 加 `sandbox` 依赖
2. `main.rs` 启动时 `discover_enabled()` 并挂载各模块的 router
3. `modules/sandbox` 新增实体 `demoItem` 的完整 CRUD（list / create / update / delete）
4. **不改** `scope-item`（那是 REQ-009 的验收资产）

> **为什么挂 sandbox 而不是新建模块**：sandbox 的定位就是「参考实现」。
> 一个挂在服务里、能被前端真正调用的 sandbox，比一个只在测试里存在的 sandbox
> 更有价值 —— 它同时成为「业务模块怎么接平台」的活样例。

**新增决策 D103**。

### 1.3 #12：路由器只做精确匹配，没有通配

**问题**：`apps/shell/src/lib/router.ts:101` 的 `match` 是
`routes.find((route) => route.path === path)` —— **没有 `:param` 支持**。
而我在 design §5.2 说「前端只有一条通配路由 `/schema/:moduleId/:entityKey`」，
这是「零代码」成立的前提。

**裁定**：**接受**。扩 `router.ts` 支持 `:param` 段匹配。

**要求**：
- **向后兼容**：精确路径仍按精确匹配（现有 REQ-011 的路由不受影响）
- 匹配优先级：**精确 > 含 `:param` 的**（避免通配抢走精确路由）
- 参数提取：`match('/schema/a/b')` → `{ route, params: { moduleId: 'a', entityKey: 'b' } }`
- 权限：`RouteDefinition.permission` 仍按静态声明（通配路由声明**模块级**权限点）

**这是对 REQ-011 产物的扩展，不是破坏** —— 必须在 design 里写明。

**新增决策 D104**。

---

## 2. BA 自己的 3 条错（逐条认）

### 2.1 #1：我写「按固定顺序、首个失败即退出」—— 与现有实现冲突

**证据**：`crates/platform-core/src/extension/manifest.rs:230-243` 的
`parse_checked` 返回 `Vec<ManifestError>`，`validate()` 收集**全部**问题。
三处调用点（`platform-config/src/registry.rs:118`、`platform-rbac/src/registry.rs:130`、
`platform-db/src/scopes.rs:83`）都把它 join 成一行显示。

**裁定**：**接受**。**保留聚合校验**，AC-S-076 改为：

- 去掉「首个失败即退出」
- 每条违规场景断言「**错误信息里点名了该违规**」
- 额外断言：**单条违规时 `issues.len() == 1`**（证明没有误报别的规则）
- **一次造 2 条违规**时，断言 `issues.len() == 2`（证明是聚合，不是只报一个）

> **为什么不能改成首个失败**：那是**破坏性改动** —— 三处调用点都依赖聚合。
> 而且聚合对开发者更友好（一次看到所有问题）。
> 我写「首个失败」是**凭直觉**，没核实现有实现 —— 这是 BA 的错。

### 2.2 #4：我写「响应体全文不出现字段名」—— **必然**误报

**证据**：响应里每个 field 都有 `label` 键，而 `label` 这个词本身就含字母 `b`。
只要字段 key 是 `b`，全文匹配就**必然**命中 `label`。

**裁定**：**接受**。改成**结构化判据**：

1. `fields.map(f => f.key)` **深度等于**期望数组（不含被过滤的字段）
2. 顶层键集合 `Object.keys(body).sort()` **深度等于**期望集合
   （**不含** `hiddenFields` / `deniedFields` / `total` 等清单或计数）
3. 响应体里**不出现带引号边界的字段名 token**：`/"b"\s*:/` 不匹配
   （即：作为 **JSON 键**不出现，而不是作为子串）
4. 被过滤字段的 `label` 文本**不出现在任何字段的 `label` 值里**

> 第 3 条是关键：**「作为键不出现」≠「作为子串不出现」**。
> 前者才是安全属性，后者会误报。

### 2.3 #11：示例权限点 `sandbox:demo:secret:read` 是 **4 段**，非法

**证据**：`manifest.rs:367-370` 的 `is_permission_code` 要求
`parts.len() == 3`。我写的 4 段码**照抄就会让启动失败**。

**裁定**：**接受**。统一改为 **`sandbox:demo:secret`**（3 段）。

**顺带确认**：`PermissionType` 枚举**已有 `Field` 变体**
（`manifest.rs:86-95`：`Menu` / `Button` / `Field` / `Data`）——
字段级权限点不需要扩枚举，直接 `"type": "field"`。

**新增**：manifest 的 `permissions` 段要加一条 `{ code: "sandbox:demo:secret",
name: "查看示例内部备注", type: "field" }`，并由种子迁移同步（D27 启动对账）。

---

## 3. 其余高危（全部接受）

### #2 跨 manifest 规则放不进单文件校验

**问题**：design §2.4 第 5/6 条（`permission` 必须是已登记权限点、
`reference` 的实体必须存在）需要**全局上下文**，而三个扫描点都是**逐文件** parse。

**裁定**：**接受**。design 改成**两阶段校验**：

| 阶段 | 时机 | 内容 |
|---|---|---|
| **阶段 1（单文件）** | 每个 manifest 解析时 | 格式、类型、重复 key、非空 —— 全部可在文件内判定 |
| **阶段 2（全局）** | 所有 manifest 加载完后 | `permission` 是否在注册表里、`reference.module/entity` 是否存在 |

阶段 2 失败 → 同样**进程起不来**，错误信息点名「谁的哪个引用指向了不存在的东西」。

> 这与 REQ-009 的 D27（启动时权限点对账）同源：
> **跨文件的一致性只能在所有文件都加载后才能判**。

### #3 「进程起不来」怎么测

**问题**：`serve` 启动前要跑 `require_isolation`（需真库 + 非超管角色），
且仓库没有 spawn **服务**二进制的先例（唯一先例是 spawn 测试二进制）。

**裁定**：**接受**。AC-S-076 给**两条**可行路径，任选其一即可判：

| 路径 | 做法 | 适用 |
|---|---|---|
| **A（进程层）** | `BASE_MANIFEST_ROOT=<临时副本>` + 真库 + `base_app` 连接 → spawn `base serve` → 断言**退出码非 0** + stderr 含违规点名 | 需要库 |
| **B（无库替代）** | `base tenant-policies --check`（它**先解析 manifest 再连库**，`policies.rs:39-44`）→ 断言**退出码非 0** | 不需要库 |

且 AC-S-076 步骤 2（成功场景）改为：**health `/health/live` 返回 200**，然后 SIGTERM 收尾。

### #7 与 `@base/ui` 的 Table 契约冲突

**问题**：`Table.svelte` **恒渲染 thead**、空态**渲染一行**、分页需要 `total`；
而本需求要「空态不渲染空壳表」+ **游标分页没有 total**。

**裁定**：**接受**。**`SchemaTable` 不用 `@base/ui` 的 Table**，自己渲染。

**理由**：
- Table 的契约（`packages/ui/src/index.ts:287-295`）是「只读展示 + 页码分页」，
  与游标分页**语义不同**
- 改 Table 会动 REQ-011 已通过的契约（AC-S-075 双向断言）

**同步要求**：`capabilities/index.json` 的 `ui_table_component` 说明要更新为
「`@base/ui` 的 Table 是**页码分页**的只读表格；**游标分页**请用
`apps/shell` 的 `SchemaTable`」—— 否则下一个人会拿 Table 去做游标分页。

### #8 `reference` 没有数据源

**问题**：元数据只声明 `reference: { module, entity }`，
**没有任何「怎么拿到下拉选项」的约定**。

**裁定**：**接受，本期降级**。

**`reference` 在本期 = 只读的 id 文本**（不渲染下拉、不提供选项接口）。

**理由**：选项接口需要定义路径 / 形状 / 权限 / 上限 / 分页 / 搜索 ——
这是一整块能力，本轮范围已经很大。降级比「定义了接口但实现不了」诚实。

**验收影响**：AC-S-083 步骤 2 的 `reference → 下拉` 改为
`reference → 只读文本（展示 id 原值）`。

### #9 「列表接口」是谁

**裁定**：**接受**。钉死为**新建的 `demoItem` 列表接口**（`GET /api/sandbox/demo-items`），
**不改** `scope-item`（那是 REQ-009 的验收资产）。

### #10 「请求路径集合 ⊆ 5 个中文名」不可执行

**裁定**：**接受**。改为：

1. **正则白名单**：`^/api/(schema|sandbox)/` 与 `^/(health|assets)/`
2. **基线剔除**：先记录页面加载前已有的请求（`branding`、`health` 等），从断言集合里减去
3. 断言「**元数据里未声明的路径**一个都没有」

---

## 4. 不采纳（1 条，有证据）

### #13 「xtask 写盘让 `--check` 恒绿」—— **不成立**

**QA 的说法**：xtask 的 `--check` 会先写盘再校验，导致检查恒绿。

**我核实的结果**：

```rust
// crates/xtask/src/main.rs:94-117
fn run_capabilities(args: &CapabilitiesArgs) -> Result<ExitCode> {
    let root = ...;
    let generated = capabilities::generate(root)?;   // 只生成内存里的数据

    if args.check {
        let report = capabilities::check(root, &generated)?;   // 只比较，不写盘
        if report.is_consistent() { return Ok(SUCCESS); }
        ... // 列 missing / changed / extra
        return Ok(ExitCode::FAILURE);
    }

    let pruned = capabilities::write_files(root, &generated)?;   // ← 只在非 check 时才写
    ...
}
```

**`--check` 分支在 `write_files` 之前 `return`** —— 写盘代码**根本不会执行**。

**实证**：我改过 `catalog.rs` 但忘了重生成时，`cargo xtask capabilities --check`
**确实报了不一致**（REQ-011 期间实测）。

**结论**：**不采纳**。QA 这条是基于对代码的推测，没有实际构造「改了源码不重生成 →
跑 `--check` → 观察是否变绿」的复现。
> 这本身是个教训：**QA 的挑刺也可能有假**。接受前要核证据。

---

## 5. 中 / 低 severity 的裁定（接受，摘要）

| # | 裁定要点 |
|---|---|
| #14 | `entity.schema.json` 的命名**钉死**（不写「或等价命名」）；且 xtask 的 `write_files` 会 prune 未注册产物 → 名字必须在生成器里注册 |
| #15 | 枚举权威链：Rust `PermissionType` 是唯一来源，JSON Schema 由生成器写出，**不允许手改** |
| #16 | AC-S-081 步骤 3 的「实施前必须选定」→ 删掉，预期固定为「**通过**」（design §4.4 已选定「至少一处」） |
| #17 | AC-S-085 的哈希范围扩到 `packages/schema/src` + `apps/shell/src`，**并加「全文不含 E2 实体键」**的更强判据；`package.json` 允许变（可能要加依赖） |
| #18 | 文案钉死：`boolean` → `是`/`否`；`date` → `YYYY-MM-DD`（**不做时区换算**）；空值 → 空串 |
| #19 | `nextCursor` 形状钉死：响应体 `{ items: [...], nextCursor: string \| null }`（`null` 表示没有下一页） |
| #20 | D67 三件套（权限点声明 + 种子迁移 + 启动对账）全部要补 |
| #21 | AC-S-078 前置与 design §3.1 的矛盾 → 统一为「已认证即可读」 |
| #22 | M4（未知类型）与 AC-S-076（启动拒未知类型）的冲突 → 明确：**manifest 声明**未知类型 → 启动失败；**运行时拿到**未知类型（老前端 + 新后端）→ 降级渲染。两者不冲突 |
| #23 | 白名单语义与「数据仍带该键」→ 明确：投影是**重建对象**，未声明键**不出现** |
| #24 | AC-S-084 是同义反复 → 保留但**明确它的意义**：证明「渲染器不会自造字段」，权限正确性由 AC-S-079/080 负责；DOM 断言走 Playwright |
| #25 | 投影对**列表**的语义 → 明确：`items[]` 里每条记录**逐条**投影 |

其余（#26~#40）为表述精确化，逐条落进 rev 2，不单列。

---

## 6. 本轮新增决策

| # | 决策 |
|---|---|
| **D102** | schema 的实体声明 **`api` 子段**（`list`/`create` 必填，`update`/`delete` 可选），路径必须以 `/api/` 开头。**不用约定式复数化**（猜不对 `person→people`） |
| **D103** | **把 `modules/sandbox` 挂进 `platform-server`**，并给它加 `demoItem` 实体的完整 CRUD。sandbox 的定位是「参考实现」，挂起来才成为活样例；`scope-item` 不动 |
| **D104** | **扩 `apps/shell` 的 router 支持 `:param`**（精确优先于通配），这是「零代码」的前提。属对 REQ-011 产物的**扩展**，向后兼容 |
| **D105** | **`reference` 本期降级为只读 id 文本**。选项接口需要路径/形状/权限/上限/分页/搜索，是一整块，本轮不做 —— **降级比「定义了却实现不了」诚实** |
| **D106** | **`SchemaTable` 不复用 `@base/ui` 的 Table**（游标分页 vs 页码分页语义不同），并同步更新能力索引里 `ui_table_component` 的说明 |

---

## 7. 下一棒

1. BA 出 `acceptance.md` **rev 2** + `design-summary.md` 更新（含 §2 的 `api` 子段、
   §2.4 两阶段校验、§4 的投影细节、§5.2 的路由扩展）
2. Dev 实现
3. QA Pre-merge
4. 修复轮 → 独立复核

**范围提示**：本轮工作量比 REQ-011 更大（后端新增 manifest 段 + 元数据接口 +
投影器 + C-6 + 挂载 sandbox + demo CRUD；前端新增渲染器 + 路由扩展）。
**建议拆成两个并行 Dev**（后端 / 前端），文件集不重叠。
