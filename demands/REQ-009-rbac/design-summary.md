# 设计摘要：RBAC 权限与数据范围（REQ-009）

- **需求编号**: REQ-009
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **依赖**: [REQ-006 多租户与隔离](../REQ-006-tenant/design-summary.md)、[REQ-007 认证](../REQ-007-auth-account/design-summary.md)、[REQ-008 用户与组织架构](../REQ-008-user-org/design-summary.md)
- **契约**: [api-conventions v1（C4 权限点编码、C5 越权响应码、C6 错误体、C7 数据范围自动叠加）](../../contracts/api-conventions/v1.md)、[extension-points v2（C-5 鉴权调用点检查）](../../contracts/extension-points/v2.md)
- **架构决策**: [ADR-0001 A6（自研 RBAC + 可插拔决策点）](../../decisions/ADR-0001-底座技术架构基线.md)
- **性质**: 需求细化附件 —— **本迭代第一次真正实现「权限判定」**，设计决策在此定死
- **更新时间**: 2026-11-18 20:40

---

## 0. 一句话

把「谁能看到什么、能做什么」从**业务代码里抽出来**，变成**平台统一判定的数据**。
REQ-009 交付：角色与权限点模型、数据范围求值、鉴权中间件、以及把权限点绑到 HTTP 路由的机制。

**判定逻辑只在这里写一次** —— REQ-008 交付的组织结构与汇报关系是它的**输入**，不是它的副本。

---

## 1. 放在哪：`crates/platform-rbac`（平台 crate）

与 `platform-org` / `platform-iam` 同级。理由同 REQ-008：
- 鉴权中间件要挂在 `platform-server` 上 → 平台 crate 才能被 server 依赖（`check-deps` 禁止 server 依赖业务模块）
- 业务模块**通过扩展点**用权限能力，不直接依赖 `platform-rbac` 的内部

业务模块侧只需要一个能力接口。**它放在 `platform-core`，但不放在 `platform-core::extension` 下**：

```rust
pub trait PermissionChecker: Send + Sync + 'static {
    /// 当前用户在 ctx 指定的租户下是否拥有该权限点
    fn has(&self, ctx: &RequestContext, permission: &str) -> bool;
    /// 数据范围：把 scope 解析成可执行的过滤条件
    fn scope(&self, ctx: &RequestContext, scope: DataScope) -> ScopeFilter;
}
```

**为什么不是第 6 个扩展点**：扩展点的方向是「**业务模块实现、平台调用**」
（`PlatformModule` / `Repository` / `Auditable` / `NotificationSender` / `WorkflowHandler` 全是这个方向）。
`PermissionChecker` 的方向正好相反 —— **平台实现、业务模块调用**。
它属于「平台能力 API」而不是「业务 ↔ 平台的接缝」，
放进 `platform-core::extension` 会改变已冻结的扩展点集合的形状，逼所有消费方改契约。
**`extension-points v2` 因此不动那 5 个 trait**。

**依赖方向**：`platform-core`（定义 trait）← `platform-rbac`（实现）← `platform-server`（装配）。
业务模块 ← `platform-core`（只用 trait）。

> `check-deps` 的平台 crate 集合是「`PLATFORM_CRATES` 显式清单 ∪ `crates/` 下的工作区成员」。
> `PLATFORM_CRATES` 清单当前只有 4 项（`platform-core` / `platform-contracts` /
> `platform-contracts-macros` / `platform-db`），**漏了 `platform-iam` / `platform-org` / `platform-server`** ——
> 靠第二条兜底规则才没漏检。`platform-rbac` 同理**不需要改清单**，
> 但要有**破坏性测试**证明「新 crate 确实被门禁覆盖」（见 §10 R7）。

---

## 2. 权限点注册表 —— **复用已有的自动生成物，双向对账**

REQ-004 已经把「所有 manifest 的权限点」汇总成 `capabilities/permissions.json`
（模块与平台 crate 都在内，生成器在 `crates/xtask/src/capabilities.rs::permissions_json`，
扫描 `modules/*/manifest.json` 与 `crates/*/manifest.json` 两处）。**不要另建一套**。

平台侧新增一张 `platform_permission` 表登记「已在库里生效的权限点」，与生成物做**双向对账**。

| 表 | 租户维度 | 说明 |
|---|---|---|
| `platform_permission` | **无**（平台级） | 权限点编码来自代码（manifest），全平台一致 → 不带 `tenant_id`、不建 RLS（同 `platform_meta` 的处理） |
| `platform_role` / `platform_role_permission` / `platform_role_inherit` / `platform_user_role` | **有** | 角色与绑定是租户数据 → `tenant_id` + RLS policy |

### 2.1 对账结果：**启动时断言失败**，不是告警

| 情况 | 判定 |
|---|---|
| manifest 有、库里没有 | ❌ **启动失败**（退出码非 0），逐条列出缺失编码与来源 manifest |
| 库里有、manifest 没有 | ❌ **启动失败**，逐条列出多余编码 |
| 两边一致 | ✅ 正常启动 |

**为什么不是告警**：告警 = 静默放行的一种。权限点缺失时角色授了也判不出来，
表现为「配了权限却不生效」，排查成本极高且容易被误判成业务 bug。
Sprint 2 复盘 34 个 QA 问题里 9 个属于「静默放行」类 —— 这一条不能再走告警。
前提是「库里的权限点」由**迁移**写入（manifest 变更走迁移 + 启动对账），
不是运行时自动改库（运行时自动改库会让对账失去意义）。

---

## 3. 角色模型

| 概念 | 表 | 说明 |
|---|---|---|
| 角色 | `platform_role` | `tenant_id` / `id` / `code` / `name` / `data_scope` / `builtin` |
| 角色-权限 | `platform_role_permission` | `tenant_id` / `role_id` / `permission_code` |
| 角色继承 | `platform_role_inherit` | `tenant_id` / `role_id` / `parent_role_id`（自引用） |
| 用户-角色 | `platform_user_role` | `tenant_id` / `user_id` / `role_id` |

### 3.1 权限点通配

`order:*` 匹配 `order:` 下所有；`*:*` 不允许（避免全通过）。
允许的通配形态只有 `module:*` 与 `module:resource:*`。

### 3.2 角色继承的展开

继承链**在查询时展开**（按租户 + 用户维度缓存，见 §7）。
继承链**不得成环** —— 写入时检测，成环则拒绝并给出**具体是哪两个角色构成的环**。

### 3.3 内置角色

| code | 权限 | data_scope |
|---|---|---|
| `platform-admin` | 全部权限点（通配 `*`） | `ALL` |
| `platform-user` | 无特殊权限 | `SELF` |

`platform-admin` **不能被删除或降权**（`builtin = true`）。

---

## 4. 数据范围（DataScope）

**这是本需求最容易算错的地方**，多节点兼岗必须算对。

| scope | 语义 | 实现 |
|---|---|---|
| `ALL` | 全部 | 无过滤 |
| `OWN_ORG` | 本部门 | 用户**主部门**节点（`user_org.is_primary = true`） |
| `OWN_ORG_AND_BELOW` | 本部门及下级 | 主部门节点的**子树** → REQ-008 的 `org_node` **递归 CTE** |
| `SELF` | 仅本人 | `created_by = 当前用户` |
| `CUSTOM` | 自定义过滤 | §5 的表达式 DSL |

> 已核实的库结构（REQ-008 迁移 `20261118000700_org_schema.up.sql`）：
> `user_org(tenant_id, id, user_id, org_node_id, role, is_primary)`，
> `user_org_uq UNIQUE (tenant_id, user_id, org_node_id)` → **兼岗是可能的**（一个 user 多行）。
> `org_node(tenant_id, id, parent_id, node_type, code, name, path_labels)`，`id` 与 `user_id` 都是 `TEXT`。

### 4.1 兼岗的处理 —— **默认按主部门，可显式取并集**

用户在多个节点（兼岗）时：

- **默认**：`OWN_ORG` / `OWN_ORG_AND_BELOW` 只按**主部门**（`is_primary = true`）计算
- **可选**：数据范围配置里 `use_all_memberships = true` 时，取**所有成员节点的子树并集**

**为什么默认主部门**：并集会让「本部门及下级」的可见范围被兼职部门撑大 ——
在审批、薪资这类场景里是数据泄漏。需要并集的场景（跨部门协作）显式开启。

### 4.2 主部门缺失或歧义 → **fail closed 到 `SELF`**

`user_org` 的唯一约束是 `(tenant_id, user_id, org_node_id)`，**没有约束「一个用户至多一个主部门」**。
两种异常都要处理：

| 情况 | 处理 |
|---|---|
| 该用户**没有** `is_primary = true` 的行 | `OWN_ORG` / `OWN_ORG_AND_BELOW` 退化为 `SELF` |
| 该用户**有多个** `is_primary = true` 的行 | 同上退化为 `SELF`（歧义不可猜） |

**都不是 `ALL`**。并在本需求的迁移里加**部分唯一索引**把「至多一个主部门」变成数据库约束：

```sql
CREATE UNIQUE INDEX user_org_one_primary_idx
    ON user_org (tenant_id, user_id) WHERE is_primary;
```

（已核对当前 `base_test` 数据：无任何用户存在多条主部门记录，加该索引不会破坏既有数据。）

### 4.3 数据范围挂在哪

- **角色级**：角色的默认 `data_scope`
- **权限点级**：某个权限点可覆盖角色的 scope（如「查看本部门」的角色里，`export` 权限点单独收紧为 `SELF`）
- **优先级**：权限点级 > 角色级 > 用户默认（`SELF`）
- 多个角色的 scope **取最宽松的**（并集语义）—— 与权限点「任一满足即通过」一致

### 4.4 「最宽松」的序：需要**全序**，不能靠感觉

五种 scope 不是天然有序（`CUSTOM` 与 `ALL` 之间无法比较）。本需求规定**比较规则**：

| 规则 | 说明 |
|---|---|
| `ALL` 最宽松 | 任何含 `ALL` 的角色组合 → 结果 `ALL` |
| `SELF` 最严格 | 只有 `SELF` → 结果 `SELF` |
| `OWN_ORG` ⊂ `OWN_ORG_AND_BELOW` | 同为「本部门」时，后者更宽松 |
| `CUSTOM` | **不参与宽松比较**：与其它 scope 同时出现时，**取 `CUSTOM` 的那一路**（并集无法用单一序表达），且必须在响应里标明「使用了自定义范围」 |
| 多角色无交集判定 | 两个 `CUSTOM` 条件不同 → 两者**都要满足**（交集），因为「最宽松」在自定义条件下没有定义 |

> `CUSTOM` 的这一条是**故意不做全序** —— 强行给 `CUSTOM` 排序会造出「看起来有顺序、实际可能放大可见范围」的假象。
> 本期把 `CUSTOM` 限定为**单一路由生效**（见 AC-S-041），并把多 `CUSTOM` 的组合显式判为交集。

---

## 5. 自定义过滤表达式：受限 DSL，**不是任意 SQL**

`CUSTOM` scope 与属性级校验共用一套表达式。**必须可解析成表达式树**，禁止拼 SQL 字符串。

### 5.1 语法

```
expr    := or
or      := and ( "or" and )*
and     := not ( "and" not )*
not     := "not" not | primary
primary := "(" expr ")" | comparison
comparison := operand op operand
op      := "==" | "!=" | ">" | ">=" | "<" | "<=" | "in" | "contains"
operand := literal | field | property
field   := 字段名（必须在白名单内）
property:= "$user.id" | "$record.amount" | "$ctx.tenant_id" ...
literal := 字符串 / 数字 / 布尔 / 数组
```

### 5.2 三条硬约束

| 约束 | 理由 |
|---|---|
| **字段必须白名单** | 不在白名单的字段名直接拒绝解析 |
| **值必须参数化** | 生成的 SQL 走参数绑定，**不做字符串拼接** |
| **函数调用不允许** | 避免 `now()` / `substring()` 之类绕过白名单 |

**为什么不让业务方写 SQL**：那等于开 SQL 注入面。自定义过滤是「声明式条件」，不是「查询语言」。

### 5.3 属性级校验（ABAC 混合）

权限点可携带条件表达式（用同一套 DSL），例如：

```json
{ "code": "finance:invoice:approve", "condition": "$record.amount <= 10000" }
```

判定时：先过「有无该权限点」，再过「条件是否满足」。
**条件的属性来源必须在白名单**（如 `$record.amount`、`$user.department`）。

> ⚠️ **manifest schema 需要扩展**：`PermissionSpec` 当前只有 `code` / `name` / `type`
> （`crates/platform-core/src/extension/manifest.rs`），**没有 `condition` 字段**。
> 本需求要加**可选**字段 `condition`（不填 = 无条件），并同步
> `crates/platform-core/src/extension/manifest_schema.rs` 与 `capabilities/schema/manifest.schema.json`。
> **可选**字段 = 向后兼容，不破坏既有模块的 manifest。

---

## 6. 权限点 ↔ HTTP 路由的绑定

### 6.1 路由声明所需权限点

```rust
router.route("/api/order/invoice/approve", post(approve))
    .route_layer(require_permission("order:invoice:approve"));
```

### 6.2 鉴权中间件

统一中间件，行为：

| 情形 | HTTP | 错误码（契约 C6） |
|---|---|---|
| 无 token / token 无效 | **401** | `UNAUTHENTICATED` |
| token 有效但缺权限点 | **403** | `INSUFFICIENT_PERMISSION` |
| 权限点有但数据范围不满足 | **404** | `NOT_FOUND` |
| 权限点有 + 范围满足 | 200 | — |

**资源类 404、动作类 403** 是契约 **C5** 已定的口径（不是 C2 —— C2 是「业务 API 路径形式」，
初版设计摘要写错了，这里纠正）。

**错误码取值对齐契约 C6 的示例**：`INSUFFICIENT_PERMISSION`（C6 示例里给的就是这个码，
不是 `PERMISSION_DENIED`）。401 / 404 沿用 `platform-iam` 已有码（`platform-iam/src/error.rs`
的 `code::UNAUTHENTICATED` / `code::NOT_FOUND`），不另造。

**响应体约束**（契约 C6 + 本仓 `docs/contracts/README.md` 的落地口径）：
- 结构 = RFC 9457（`type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id`）
- `detail` **必须是英文 ASCII**（后端不拼中文句子；面向用户的中文由前端 i18n 渲染）
- `title` 可为中文短句（与 `platform-iam` 现有实现一致）
- `trace_id` 必带

### 6.3 ★ 补 C-5 扩展检查（extension-points v1 留下的账）

`extension-points v1` §4「C-2 的口径边界」明写：

> C-2 只验「权限点声明完整性」，不验「服务端是否真的执行了鉴权」……
> 等 REQ-009 落地鉴权中间件后，应补一条「已登记权限点必须有对应鉴权调用点」的检查。

**本需求新增契约检查 C-5**（`extension-points v2`）：

| 情况 | 判定 |
|---|---|
| 已登记且有鉴权调用点 | ✅ |
| 已登记但**无任何鉴权调用点** | ❌ **失败**（登记了却不执行 = 假权限） |
| 有鉴权调用点但未登记 | ❌ 失败（C-2 已覆盖） |
| 鉴权调用点的参数**不是字面量**（无法静态确认） | ❌ 失败（无法确认 = 不放行，fail closed） |

实现方式：平台提供 `require_permission("code")` 的**唯一入口**，
扫描各模块 `backend/**/*.rs` 里对该函数的调用，抽出字面量权限点，与 manifest 比对。

**入口必须唯一** —— 否则扫描会被绕过（例如某模块自己写了个 `check_perm()`）。
这条要有测试守着：`platform-rbac` 之外若再出现第二个鉴权入口，C-5 检查失败。

> **C-2 与 C-5 的分工**：C-2 仍按 v1 口径（代码里用到但 manifest 没声明 → 失败；
> manifest 声明了但代码没用到 → 警告）。C-5 是**新增的、更强的一档**，只针对「已登记」的权限点。
> **C-1~C-4 的判定口径不变**，故 `extension-points` 升 **v2**（新增强制检查对消费方是破坏性变更，
> 按契约 §6「旧版保留至少一个 Sprint」保留 v1 文件并标 `deprecated`）。

---

## 7. 缓存与失效

| 数据 | 缓存位置 | TTL | 失效触发 |
|---|---|---|---|
| 用户→角色→权限点集合 | **进程内** | 60s | 变更时主动失效（见下） |
| 数据范围求值结果（子树 id 集合） | 进程内 | 60s | 组织架构变更时主动失效 |

### 7.1 主动失效机制：**数据库 epoch**，不引 Redis 客户端

新增平台级表：

```sql
CREATE TABLE platform_permission_epoch (
    tenant_id  TEXT PRIMARY KEY,
    epoch      BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- 任何**角色 / 权限绑定 / 角色继承 / 数据范围**的写操作，都在同一事务里 `epoch = epoch + 1`
- 读路径：先查 epoch（一次极小的主键查询），与缓存里的 epoch 一致才用缓存；不一致 → 重算并写回缓存

**为什么不引 Redis**：
1. 本仓**当前没有任何 Redis 客户端依赖**（`redis_url` 只是配置串，`Cargo.lock` 里无 `redis` crate），
   `base-redis` 容器目前无人使用；引入 `redis` crate 会增加依赖树（当前 293 个包），
   与「低配机 + 依赖预算」约束相悖（REQ-007 的 D22 已把预算基线定到 235 且不为压数字降级依赖）。
2. epoch 方案**跨进程正确**（DB 是共享真相），而纯进程内广播只在单实例下正确。

**代价**：每个已认证请求多一次主键查询（本机 PG 上约 0.1ms 量级），换取「撤销后立即生效」。

**权限撤销必须主动失效** —— 只靠 TTL 意味着「撤销权限后 60 秒内仍可用」，这在管理场景不可接受。

---

## 8. 本期交付边界

**做**

- [ ] `crates/platform-rbac`（平台 crate）：角色模型、权限点解析、数据范围求值、表达式 DSL
- [ ] 权限点**注册表双向对账**（manifest ↔ DB），**启动时断言失败**
- [ ] 角色继承（成环检测）+ 权限点通配（`*:*` 不允许）
- [ ] 数据范围 5 种 + **兼岗默认主部门**、主部门缺失/歧义 fail closed 到 `SELF`
- [ ] **受限表达式 DSL**（白名单字段 + 参数化 + 禁函数调用）
- [ ] 属性级校验（权限点携带条件）→ **manifest schema 加可选 `condition` 字段**
- [ ] **鉴权中间件** + 路由绑定；401 / 403 / 404 三种语义按契约 C5
- [ ] **契约检查 C-5**（已登记权限点必须有鉴权调用点，且入口唯一）→ `extension-points v2`
- [ ] 权限与范围的**进程内缓存 + DB epoch 主动失效**
- [ ] 内置角色 `platform-admin` / `platform-user`（不可删降权）
- [ ] `/api/rbac/*`：角色 CRUD、授权、绑定、权限点注册表查询

**不做**

- SSO / OPA / Casbin 接入（ADR A6：自研，保留适配位）
- 权限申请/审批流（需求 F1；流程引擎在 REQ-017）
- 前端权限组件与 `can()` 实现（REQ-011 的 `@base/auth`）
  → 本期只保证「注册表是单一来源 + 有接口可读」，**前端组件不验收**
- 字段级权限的可视化配置界面
- 跨租户授权（平台管理员代客操作）→ 后续需求

### 8.1 需要动到的既有平台件（Dev 注意，别以为只是加个 crate）

| 位置 | 改什么 | 为什么 |
|---|---|---|
| `crates/platform-db/src/query.rs` | `BindValue` **没有数组变体** | `OWN_ORG_AND_BELOW` 的子树 id 集合要 `= ANY($n)`，需要 `TextArray(Vec<String>)` |
| `crates/platform-db/src/query.rs` | `TenantQuery` 加 `with_scope()` | 数据范围条件必须与 `{tenant}` 一起渲染，且**参数化** |
| `crates/platform-core/src/extension/manifest.rs` | `PermissionSpec` 加可选 `condition` | ABAC 属性级校验（§5.3） |
| `crates/platform-core/src/extension/manifest_schema.rs` + `capabilities/schema/manifest.schema.json` | schema 与实现**必须完全一致** | v1 契约 §5 明写「schema 与平台校验器必须完全一致」 |
| `crates/xtask/src/capabilities.rs` | `permissions.json` 带出 `condition` | 注册表是单一来源 |
| `crates/platform-contracts/src/checks.rs` + `crates/platform-contracts-macros` | 新增 C-5 检查与对应测试 | §6.3 |
| `crates/platform-server/src/main.rs` | 挂载 `/api/rbac` + 鉴权中间件 | C1 前缀约定 |
| `modules/sandbox` | 补 `require_permission(...)` 调用点 | sandbox 现有权限点只定义常量、从不鉴权；C-5 落地后它必须接上，否则契约测试失败 |

---

## 9. 资源约束

- 本机 7.7G 内存 / 4.7G 可用；`Cargo.lock` 当前 **293** 个包（第三方约 273）
- **不引入策略引擎依赖**（Casbin/Opa 都不引）→ 依赖树基本不变
- **不引入 Redis 客户端**（见 §7.1）→ 依赖树不变
- 表达式解析器**自研**（不用 nom / pest —— 只需 Pratt Parser 级别的几百行）
- 日常命令 `cargo check`；测试 `cargo test -- --test-threads=2`
- **测试只用 `base_test`**；**`base_dev` 不要写**
- ⚠️ **应用连接必须用普通角色 `base_app`**（超管会被 `require_isolation` 拒绝启动）
- ⚠️ `cargo test` **只能有一个 `--`**

---

## 10. 风险

| # | 风险 | 应对 |
|---|------|------|
| R1 | **兼岗时数据范围算错**（把并集当默认 → 可见范围被撑大 → 数据泄漏） | 默认主部门；并集必须显式开启；**专门的多节点兼岗测试用例** |
| R2 | 主部门缺失/歧义时退化成 `ALL` | 明确 fail closed 到 `SELF`；DB 部分唯一索引兜住「多个主部门」；测试覆盖 |
| R3 | 自定义过滤变成 SQL 注入面 | 受限 DSL：白名单字段 + 参数化 + 禁函数调用；**逃逸测试** |
| R4 | 权限撤销后 60 秒内仍可用 | DB epoch **主动失效**；测试覆盖「撤销后下一次请求即被拒」 |
| R5 | C-5 扩展检查可被绕过（多个鉴权入口 / 非字面量参数） | 入口唯一性有测试守着；非字面量参数判为**失败**（fail closed） |
| R6 | 权限点通配写错导致越权 | `*:*` 禁止；只允许 `module:*` 与 `module:resource:*` |
| R7 | 平台 crate 引入后 `check-deps` 门禁被绕过 | 靠「`crates/` 工作区成员」兜底规则覆盖；有**破坏性测试**证明新 crate 被纳入 |
| R8 | **C-5 让既有模块（sandbox）契约测试失败** | 这是**预期效果**（sandbox 确实从不鉴权）；本需求内把 sandbox 接上真实鉴权调用 |
| R9 | 「最宽松 scope」没有全序 → 多角色组合语义含糊 | §4.4 明确规则：`CUSTOM` 不参与比较、多 `CUSTOM` 取交集；API 响应标明用了自定义范围 |
