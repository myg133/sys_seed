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

业务模块侧只需要一个能力接口。它放在 **`crates/platform-core/src/extension/permission.rs`**：

```rust
pub trait PermissionChecker: Send + Sync + 'static {
    /// 当前用户在 ctx 指定的租户下是否拥有该权限点
    fn has(&self, ctx: &RequestContext, permission: &str) -> bool;
    /// 数据范围：把一种 scope 解析成可执行的过滤条件
    fn scope(&self, ctx: &RequestContext, scope: DataScope) -> ScopeFilter;
    /// 某个权限点的**有效**数据范围（多角色取并集；权限点级 scope 覆盖角色级）
    fn effective_scope(&self, ctx: &RequestContext, permission: &str) -> ScopeFilter;
    /// 某个权限点携带的属性条件（ABAC）；没有则 `None`
    fn condition(&self, ctx: &RequestContext, permission: &str) -> Option<CompiledCondition>;
}
```

> 初版只写了 2 个方法（`has` / `scope`）。Dev 实现时发现**少两个通道**：
> 「权限点级 scope 覆盖角色级」需要 `effective_scope`，
> 「属性级校验（ABAC）」需要 `condition`。**已接受扩展**（D38）。
> 4 个方法**全部同步** → trait 仍**dyn 兼容**，`Arc<dyn PermissionChecker>` 成立。

**它不是「第 6 个扩展点」，但与扩展点同目录**：

| | 扩展点（5 个） | `PermissionChecker` |
|---|---|---|
| 谁实现 | **业务模块** | **平台**（`platform-rbac`） |
| 谁调用 | **平台** | **业务模块** |
| 分类 | `kind: "extension-point"` | `kind: "platform-capability"` |

- **同目录有先例**：`crates/platform-core/src/extension/transaction.rs` 的 `TransactionalWrite`
  已经是「平台能力声明面」而不是扩展点（它不在 `EXTENSION_POINT_TRAITS` 清单里）。
- **能力索引会收录它**：`cargo xtask capabilities` 扫 `platform-core/src/extension/` 下的 trait，
  按 `EXTENSION_POINT_TRAITS` 分类 —— `PermissionChecker` 进 `capabilities/extension-points.json`
  且 `kind = "platform-capability"`，`extension_point_count` **仍是 5**。
  这对 AI 引导很重要（D8：AI 不知道底座已有什么就会重写轮子）——
  权限能力**必须出现在能力索引里**，否则 AI 会自己写一套 RBAC。
- **5 个扩展点一个不动**，`extension-points v2` 不需要为它升版本。

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

| 概念 | 表 | 关键列 |
|---|---|---|
| 权限点注册 | `platform_permission` | `code` / `name` / `condition`（**平台级，无 `tenant_id`**） |
| 角色 | `platform_role` | `tenant_id` / `id` / `code` / `name` / `data_scope` / `custom_filter` / `use_all_memberships` / `builtin` |
| 角色-权限 | `platform_role_permission` | `tenant_id` / `role_id` / `permission_code` / `data_scope`（**可空 = 不覆盖角色级**） |
| 角色继承 | `platform_role_inherit` | `tenant_id` / `role_id` / `parent_role_id`（自引用） |
| 用户-角色 | `platform_user_role` | `tenant_id` / `user_id` / `role_id` |

> `platform_permission` **不带 `tenant_id`** —— 权限点来自代码（manifest），全平台一致，
> 同 `platform_meta` 的处理。其余四张表都是**租户数据**，带 `tenant_id` + RLS policy。

### 3.1 权限点通配

`sandbox:*` 匹配 `sandbox:` 下所有；`*:*` 不允许（避免全通过）。
允许的通配形态只有 `module:*` 与 `module:resource:*`。

> 示例用**注册表里真实存在**的模块前缀（当前是 `sandbox:` 与 `org:`），不要拿
> 尚未存在的 `order:` 当例子 —— 例子写虚了，实现就会照着虚的去写。

### 3.2 角色继承的展开

继承链**在查询时展开**（按租户 + 用户维度缓存，见 §7）。
继承链**不得成环** —— 写入时检测，成环则拒绝并给出**具体是哪两个角色构成的环**。

### 3.3 内置角色

| code | 权限 | data_scope |
|---|---|---|
| `platform-admin` | **注册表的全部权限点**（动态得出，见下） | `ALL` |
| `platform-user` | 无特殊权限 | `SELF` |

**`platform-admin` 的权限集合是「注册表全集」，不是通配 `*`。**

初版这里写的是「权限 = `*`（全权限点）」，**与 §3.1「`*:*` 不允许」自相矛盾** —— 已改正。
正确语义：查询 `platform-admin` 的权限集合时，**动态取注册表当前的全部权限点**。
好处：新增一个权限点，`platform-admin` **自动**拥有它，不需要人工补授权行。

**内置角色按租户物化**：租户初始化时在**该租户**下创建这两条角色行（`builtin = true`）。
不搞「平台级模板 + 租户级引用」两套 —— 角色体系统一是租户数据，规则只有一套。

`builtin = true` 的角色**不能被删除，也不能降权**（改 `data_scope` / 改权限点都被拒）。

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
- **可选**：**在角色上**开启 `use_all_memberships = true`（`platform_role.use_all_memberships`，
  默认 `false`）时，取**所有成员节点的子树并集**

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

- **角色级**：角色的默认 `data_scope`（`platform_role.data_scope`），
  `CUSTOM` 的表达式存 `platform_role.custom_filter`
- **权限点级**：某个权限点可覆盖角色的 scope（如「查看本部门」的角色里，`export` 权限点单独收紧为 `SELF`）
  → `platform_role_permission.data_scope`（**可空 = 不覆盖**）
- **优先级**：权限点级 > 角色级 > 用户默认（`SELF`）
- 多个角色的 scope **取最宽松的**（并集语义）—— 与权限点「任一满足即通过」一致
- **兼岗取并集的开关是角色级的**：`platform_role.use_all_memberships`（默认 `false`）

### 4.4 多角色合并：**取过滤条件的并集**，不比较「宽松度」

初版这里写的是「`CUSTOM` 不参与宽松比较：取 `CUSTOM` 的那一路」，
**这条是错的**（Dev 忠实实现后发现并上报，BA 已裁定修订）——
「取 CUSTOM 那一路」**既不是并集也不是交集**：
用户同时有「本部门」角色和「CUSTOM 条件」角色时，
**本部门里不满足 CUSTOM 条件的记录反而看不见了** → 加一个角色**缩小**了可见范围，
与 §4.3「多角色取最宽松（并集语义）」直接矛盾。

**正确的语义**：每个角色给一份授权（权限 + 一份可见范围过滤条件），
**多角色的可见范围 = 这些过滤条件的并集** —— 与权限点「任一满足即通过」完全一致。

| 情况 | 结果 |
|---|---|
| 任一角色是 `ALL` | **`ALL`** |
| 否则有 `CUSTOM` | **CUSTOM 的条件 OR 其它角色的最宽松范围** |
| 否则有 `OWN_ORG_AND_BELOW` | `OWN_ORG_AND_BELOW` |
| 否则有 `OWN_ORG` | `OWN_ORG` |
| 否则 | `SELF` |

- **多个 `CUSTOM` 角色：条件之间取 OR**（并集）。
  初版「两个 CUSTOM 条件不同 → 两者都要满足（交集）」**一并作废** —— 同样与并集原则矛盾。
- 「最宽松」这个说法只适用于**包含型** scope（`ALL` / `OWN_ORG_AND_BELOW` / `OWN_ORG` / `SELF`），
  它们构成一条包含链，取最宽的那个即可；`CUSTOM` 不在这条链上，
  所以**不能靠比较宽松度来合并它**，只能靠 OR。

> ⚠️ **管理员须知**：`CUSTOM` 条件可以让可见范围**宽于**任何内置 scope
> （例如 `$record.amount <= 10000` 会命中本部门之外的记录）。
> 这是管理员配置的责任 —— 平台**不阻止**，但**必须在响应里标明「使用了自定义范围」**。

### 4.5 同一条 grant 命中多条时：**精确优先于通配**

一个角色可能同时授予 `order:*`（宽泛默认）与 `order:invoice:read`（显式意图），
两条都匹配权限点 `order:invoice:read`。**取哪一条？**

**规则**（D43）：

| 维度 | 优先级 | 说明 |
|---|---|---|
| **匹配具体度** | 精确 `module:resource:action` > `module:resource:*` > `module:*` | 管理员写精确的那条是**显式意图**，写通配的是**宽泛默认**；显式覆盖默认 |
| 同具体度 | 取**更严格**的 scope | 与「显式意图优先」同向，且**不会放大**可见范围 |

- **跨角色仍然是并集**（§4.4）—— 这条只管**同一个角色内部**的多条 grant。
- ⚠️ 初版实现是「取排序里最后一条匹配的」—— 因为 `'*' < 小写字母`，
  直接授权时恰好是精确的赢，**但那只是排序的副产物**；
  经角色继承时，胜者由**角色 ID 的字典序**决定（角色 ID 是 UUID → 实际等同随机）。
  **随机是不合格的** —— 同一份配置在两次部署里可能给出不同的可见范围。必须显式实现。

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

### 5.4 条件非法时的表示：**返回永不匹配的条件**，不是 `None`

`PermissionChecker::condition()` 的返回值有两个含义，必须**区分开**：

| 返回值 | 含义 | 调用方动作 |
|---|---|---|
| `None` | 这个权限点**根本没有**条件 | 直接放行 |
| `Some(永不匹配的条件)` | 条件**非法**（引用白名单外属性 / 编译失败） | **判不通过**（fail closed） |

**为什么非法条件不能返回 `None`**：`None` 只该表示「无条件」。
若非法条件也返回 `None`，调用方无法区分「无条件」与「条件非法」——
要么把**所有**无条件权限点一起误拒，要么把非法条件**静默放行**。两者都是错的。

所以：**非法 → `Some(永不匹配)`**，调用方应用它之后**一行都看不到**（数据范围是 `ALL` 也看不到）。
fail closed 成立，且不牺牲「无条件」的正常路径。

---

## 6. 权限点 ↔ HTTP 路由的绑定

### 6.1 路由声明所需权限点

```rust
router.route("/api/order/invoice/approve", post(approve))
    .route_layer(require_permission("order:invoice:approve"));
```

**`require_permission` 放在 `platform-core`**（`src/extension/permission.rs`），
**不放在 `platform-rbac`**。

理由：**业务模块的运行期依赖只有 `platform-core` + `platform-db`**（AGENTS.md 硬规则 2）。
鉴权入口如果住在 `platform-rbac`，业务模块就得依赖平台内部模块 → 违反规则 2。
`platform-core` **本来就依赖 axum**（`Cargo.toml` 里已有），所以入口放得下。

入口怎么拿到判定实现：**不靠泛型状态，靠请求扩展**。
`platform-server` 的中间件把 `Arc<dyn PermissionChecker>` 放进请求扩展，
`require_permission` 返回的 layer 从扩展里取出来用。
`PermissionChecker` 的方法都是同步的（无 RPITIT）→ **dyn 兼容**，可以 `Arc<dyn ...>`。

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

**403 与 404 的分工（这是最容易写错的地方）**：

| 情形 | 谁判 | 结果 |
|---|---|---|
| 用户**根本没有**这个权限点 | `require_permission` 层 | **403** |
| 用户**有**权限点，但目标记录**不在其数据范围内** | 数据层的范围过滤（查不到 → 当不存在） | **404** |

所以「无权访问的资源」与「不存在的资源」都表现为 **404**（用户有权限点的前提下），
**存在性不泄露**；而「缺操作权限」是 **403**（契约 C5 明确的动作类语义，不是泄露）。

**错误码取值对齐契约 C6 的示例**：`INSUFFICIENT_PERMISSION`（C6 示例里给的就是这个码，
不是 `PERMISSION_DENIED`）。401 / 404 沿用 `platform-iam` 已有码（`platform-iam/src/error.rs`
的 `code::UNAUTHENTICATED` / `code::NOT_FOUND`），不另造。
`INSUFFICIENT_PERMISSION` 是**新增**码，**放在 `platform-rbac` 的错误类型里**，
不塞进 `platform-iam`（那是账号体系的错误集，与权限判定不是一回事）。

**响应体约束**（契约 C6 + 本仓 `docs/contracts/README.md` 的落地口径）：
- 结构 = RFC 9457（`type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id`）
- `detail` **必须是英文 ASCII**（后端不拼中文句子；面向用户的中文由前端 i18n 渲染）
- `title` 可为中文短句（与 `platform-iam` 现有实现一致）
- `trace_id` 必带
- `detail` **可以**含被拒的**权限点编码**（开发者信息），
  但**不得**含被拒**资源**的业务细节（标题 / 金额 / 归属人 / `org_node_id`）

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
- [ ] 内置角色 `platform-admin` / `platform-user`（不可删降权，**按租户物化**）
- [ ] `/api/rbac/*`：角色 CRUD、授权、绑定、权限点注册表查询
      —— **管理接口自身必须受权限点保护**（自举防护，见 AC-S-034），
      **但权限点注册表查询豁免**（任何已认证用户可读，否则前端 `can()` 无从判断）
- [ ] `crates/platform-rbac/manifest.json`：声明平台自己的管理权限点
      （读角色 / 改角色 / 改用户角色绑定；**注册表查询豁免则不需要权限点**）

**不做**

- SSO / OPA / Casbin 接入（ADR A6：自研，保留适配位）
- 权限申请/审批流（需求 F1；流程引擎在 REQ-017）
- 前端权限组件与 `can()` 实现（REQ-011 的 `@base/auth`）
  → 本期只保证「注册表是单一来源 + 有接口可读」，**前端组件不验收**
- 字段级权限的可视化配置界面
- 跨租户授权（平台管理员代客操作）→ 后续需求

### 8.1 需要动到的既有件（Dev 注意，别以为只是加个 crate）

**平台件**

| 位置 | 改什么 | 为什么 |
|---|---|---|
| `crates/platform-db/src/query.rs` | `BindValue` **没有数组变体** | `OWN_ORG_AND_BELOW` 的子树 id 集合要 `= ANY($n)`，需要 `TextArray(Vec<String>)` |
| `crates/platform-db/src/query.rs` | `TenantQuery` 加 `with_scope()` | 数据范围条件必须与 `{tenant}` 一起渲染，且**参数化** |
| `crates/platform-core/src/extension/permission.rs`（**新建**） | `PermissionChecker` trait + `DataScope` + `ScopeFilter` + `require_permission(...)` **唯一入口** | 业务模块只能依赖 `platform-core` + `platform-db`；入口不能住在 `platform-rbac`（§6.1） |
| `crates/platform-core/src/extension/manifest.rs` | `PermissionSpec` 加可选 `condition` | ABAC 属性级校验（§5.3） |
| `crates/platform-core/src/extension/manifest_schema.rs` + `capabilities/schema/manifest.schema.json` | schema 与实现**必须完全一致** | v1 契约 §5 明写「schema 与平台校验器必须完全一致」 |
| `crates/xtask/src/capabilities.rs` | `permissions.json` 带出 `condition`；`extension-points.json` 的 `contract` 字段改 `v2` | 注册表是单一来源；契约版本不能与正本脱节 |
| `crates/platform-contracts/src/checks.rs` + `crates/platform-contracts-macros` | 新增 C-5 检查与对应测试（宏现在生成 4 个测试，要变 5 个） | §6.3 |
| `crates/platform-server/src/main.rs` | 挂载 `/api/rbac` + 注入 checker 的中间件 | C1 前缀约定 |
| `crates/platform-rbac/`（**新建**） | 平台 crate 本体 | — |
| `crates/platform-rbac/migrations/`（**新建**） | `platform_permission` / `platform_role` / `platform_role_permission` / `platform_role_inherit` / `platform_user_role` / `platform_permission_epoch` | §3 |

**`modules/sandbox`（唯一的业务模块，本期要改成「参考实现」）**

| 位置 | 改什么 | 为什么 |
|---|---|---|
| `modules/sandbox/backend/src/http.rs`（**新建**） | 加一个最小路由 + `require_permission("...")` 调用点 | C-5 要求「已登记权限点必须有鉴权调用点」；sandbox 现有权限点只定义常量、**从不鉴权** |
| `modules/sandbox/migrations/`（**新建**） | `sandbox_scope_item`（**带 `org_node_id` 与 `created_by` 两列**） | 数据范围用例需要一张能被范围过滤的表；现有 `sandbox_tenant_item` 只有 `title` / `revision`，**没法表达范围** |
| `modules/sandbox/manifest.json` | `tenantScoped.tables` 加 `sandbox_scope_item`；`permissions` 加对应的读写权限点 | RLS policy 由平台按 manifest 生成（§AGENTS 硬规则 4） |
| `modules/sandbox/Cargo.toml` | 若用了 axum，加 `axum` 依赖 | 业务模块可以依赖 axum（不是平台内部模块） |

> ⚠️ **业务模块的路由不由 `platform-server` 挂载** ——
> `check-deps` 禁止平台 crate 依赖业务模块（REQ-008 已踩过这个坑，见 `REQ-008/status.md`）。
> sandbox 的路由**在自己的测试里用 `ServiceExt::oneshot` 驱动**，
> 既是 C-5 的调用点来源，也是「参考实现」的示范。模块挂载机制等 REQ-020。

**仓库基建**

| 位置 | 改什么 | 为什么 |
|---|---|---|
| `feature-meta/manifest.json` | 加 `"contracts": { "api-conventions": "v1", "extension-points": "v2" }` | **现在缺这个 pin，`scripts/sync-contracts.sh` 直接 exit 2 —— 契约门禁在 CI 上是常红的** |
| `docs/contracts/extension-points/v2.md` | 用 `scripts/sync-contracts.sh --sync` 生成快照 | 快照必须与 BA 正本一致 |
| `AGENTS.md` | 补 `platform-rbac` 目录、C-5、硬规则 1 的例外说明 | **AI 引导文件必须与实际一致**（父 AC-035）；否则 AI 会以为 platform-core 永远不能改、或以为 C-1~C-4 就是全部检查 |
| `crates/xtask/src/scaffold.rs` | 生成的模块骨架带上 `require_permission` 用法 + `condition` 字段示例 | 脚手架是二次开发的第一入口（父 AC-037） |

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
| R10 | **`require_permission` 入口放在 `platform-rbac` 会让业务模块违规依赖平台内部模块** | 入口放 `platform-core`（已有 axum 依赖）；靠请求扩展注入 `Arc<dyn PermissionChecker>`，不靠泛型状态 |
| R11 | **AGENTS.md 硬规则 1 说「不得修改 platform-core / platform-contracts」**，而本需求必须改这两个 crate | 规则的意图是「业务方 / AI 不许就地扩平台」，不是「平台自己不许演进」；本需求是**已评审**的平台演进。**必须同步改 AGENTS.md**，否则引导文件与实际脱节（父 AC-035 违规） |
| R12 | 数据范围用例缺夹具表（现有 `sandbox_tenant_item` 没有 `org_node_id` / `created_by`） | 本需求内给 sandbox 加 `sandbox_scope_item`（§8.1） |
| R13 | 契约门禁在 CI 上**常红**（`feature-meta/manifest.json` 缺 `contracts` pin） | 本需求内补 pin + 同步 v2 快照（§8.1「仓库基建」） |
