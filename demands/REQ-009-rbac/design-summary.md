# 设计摘要：RBAC 权限与数据范围（REQ-009）

- **需求编号**: REQ-009
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **依赖**: [REQ-006 多租户与隔离](../REQ-006-tenant/design-summary.md)、[REQ-007 认证](../REQ-007-auth-account/design-summary.md)、[REQ-008 用户与组织架构](../REQ-008-user-org/design-summary.md)
- **契约**: [api-conventions v1（C2 越权响应码、C4 权限点编码）](../../contracts/api-conventions/v1.md)、[extension-points v1（C-2 权限点声明完整）](../../contracts/extension-points/v1.md)
- **架构决策**: [ADR-0001 A6（自研 RBAC + 可插拔决策点）](../../decisions/ADR-0001-底座技术架构基线.md)
- **性质**: 需求细化附件 —— **本迭代第一次真正实现「权限判定」**，设计决策在此定死
- **更新时间**: 2026-11-18 19:10

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

业务模块侧只需要一个能力接口（放在 `platform-core` 的扩展点里）：

```rust
pub trait PermissionChecker: Send + Sync + 'static {
    /// 当前用户在 ctx 指定的租户下是否拥有该权限点
    fn has(&self, ctx: &RequestContext, permission: &str) -> bool;
    /// 数据范围：把 scope 解析成可执行的过滤条件
    fn scope(&self, ctx: &RequestContext, scope: DataScope) -> ScopeFilter;
}
```

**依赖方向**：`platform-core`（定义 trait）← `platform-rbac`（实现）← `platform-server`（装配）
业务模块 ← `platform-core`（只用 trait）。

---

## 2. 权限点注册表 —— **复用已有的自动生成物**

REQ-004 已经把「所有 manifest 的权限点」汇总成 `capabilities/permissions.json`
（模块与平台 crate 都在内）。**不要另建一套**。

| 来源 | 已就位 |
|---|---|
| `modules/*/manifest.json` → `permissions[].code` | ✅ REQ-004 已生成 |
| `crates/platform-*/manifest.json` → 同上 | ✅ REQ-004 已生成 |

平台侧新增一张 `platform_permission` 表（登记在库里的权限点），与生成物做**一致性校验**：
manifest 里有但库里没有 → 启动时告警；库里有但 manifest 里没有 → 启动时告警。
**双向对账**，避免「改了 manifest 忘了同步」或反之。

---

## 3. 角色模型

| 概念 | 表 | 说明 |
|---|---|---|
| 角色 | `platform_role` | `id` / `tenant_id` / `code` / `name` / `data_scope` / `builtin` |
| 角色-权限 | `platform_role_permission` | `role_id` / `permission_code` |
| 角色继承 | `platform_role_inherit` | `role_id` / `parent_role_id`（自引用） |
| 用户-角色 | `platform_user_role` | `user_id` / `role_id` |

### 3.1 权限点通配

`order:*` 匹配 `order:` 下所有；`*:*` 不允许（避免全通过）。

### 3.2 角色继承的展开

继承链**在查询时展开**并缓存（按租户 + 用户维度，TTL 60s，Redis 或进程内）。
继承链**不得成环** —— 写入时检测，成环则拒绝。

### 3.3 内置角色

| code | 权限 | data_scope |
|---|---|---|
| `platform-admin` | `*`（全权限点） | `ALL` |
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

### 4.1 兼岗的处理 —— **默认按主部门，可显式取并集**

用户在多个节点（兼岗）时：

- **默认**：`OWN_ORG` / `OWN_ORG_AND_BELOW` 只按**主部门**（`is_primary = true`）计算
- **可选**：数据范围配置里 `use_all_memberships = true` 时，取**所有成员节点的子树并集**

**为什么默认主部门**：并集会让「本部门及下级」的可见范围被兼职部门撑大 —— 在审批、薪资这类场景里是数据泄漏。
需要并集的场景（跨部门协作）显式开启。

**主部门缺失时**（用户还没被分配主部门）：`OWN_ORG` / `OWN_ORG_AND_BELOW` 退化为 `SELF`（fail closed），
**不是** `ALL`。

### 4.2 数据范围挂在哪

- **角色级**：角色的默认 `data_scope`
- **权限点级**：某个权限点可覆盖角色的 scope（如「查看本部门」的角色里，`export` 权限点单独收紧为 `SELF`）
- **优先级**：权限点级 > 角色级 > 用户默认（`SELF`）
- 多个角色的 scope **取最宽松的**（并集语义）—— 与权限点「任一满足即通过」一致

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

---

## 6. 权限点 ↔ HTTP 路由的绑定

### 6.1 路由声明所需权限点

```rust
router.route("/api/order/invoice/approve", post(approve))
    .route_layer(require_permission("order:invoice:approve"));
```

### 6.2 鉴权中间件

统一中间件，行为：

| 情形 | 响应 |
|---|---|
| 无 token / token 无效 | **401** `UNAUTHENTICATED` |
| token 有效但缺权限点 | **403** `PERMISSION_DENIED`（动作类越权，契约 C2 口径） |
| 权限点有但数据范围不满足 | **404**（资源类越权，契约 C2 口径 —— 不泄露资源存在性） |
| 权限点有 + 范围满足 | 放行 |

**两种越权给不同码**是契约 C2 已定的：资源类 404、动作类 403。

### 6.3 ★ 补 C-2 的扩展检查（extension-points v1 留下的账）

REQ-008 期间 QA 指出：C-2 只验「声明完整性」，**不验「服务端是否真的执行了鉴权」** ——
一个登记了但从不被检查的权限点，C-2 会全绿无告警。

**REQ-009 必须补上这条检查**：扫描每个模块的**鉴权调用点**，与 manifest 登记的权限点比对。

| 情况 | 判定 |
|---|---|
| 已登记且有鉴权调用点 | ✅ |
| 已登记但**无任何鉴权调用点** | ❌ **失败**（登记了却不执行 = 假权限） |
| 有鉴权调用点但未登记 | ❌ 失败（C-2 已覆盖） |

实现方式：平台提供 `require_permission("code")` 的**唯一入口**，
扫描各模块 `backend/**/*.rs` 里对该函数的调用，抽出字面量权限点，与 manifest 比对。
**入口必须唯一** —— 否则扫描会被绕过。这条要有测试守着。

---

## 7. 缓存与失效

| 数据 | 缓存位置 | TTL | 失效触发 |
|---|---|---|---|
| 用户→角色→权限点集合 | Redis（无 Redis 时进程内） | **60s** | 角色/权限变更时主动删；TTL 兜底 |
| 数据范围求值结果（子树 id 集合） | 同上 | 60s | 组织架构变更时主动删 |

**权限变更必须主动失效** —— 只靠 TTL 意味着「撤销权限后 60 秒内仍可用」，这在管理场景不可接受。

---

## 8. 本期交付边界

**做**

- [ ] `crates/platform-rbac`（平台 crate）：角色模型、权限点解析、数据范围求值、表达式 DSL
- [ ] 权限点**注册表双向对账**（manifest ↔ DB），启动时校验
- [ ] 角色继承（成环检测）+ 权限点通配（`*` 不允许全通过）
- [ ] 数据范围 5 种 + **兼岗默认主部门**、主部门缺失 fail closed 到 `SELF`
- [ ] **受限表达式 DSL**（白名单字段 + 参数化 + 禁函数调用）
- [ ] 属性级校验（权限点携带条件）
- [ ] **鉴权中间件** + 路由绑定；401 / 403 / 404 三种语义按契约 C2
- [ ] **补 C-2 扩展检查**（已登记权限点必须有鉴权调用点，且入口唯一）
- [ ] 权限与范围的缓存 + **主动失效**
- [ ] 内置角色 `platform-admin` / `platform-user`（不可删降权）

**不做**

- SSO / OPA / Casbin 接入（ADR A6：自研，保留适配位）
- 权限申请/审批流（需求 F1；流程引擎在 REQ-017）
- 前端权限组件（REQ-011 的 `@base/auth`）
- 字段级权限的可视化配置界面
- 跨租户授权（平台管理员代客操作）→ 后续需求

---

## 9. 资源约束

- 本机 7.7G 内存 / 4.7G 可用；第三方依赖树当前 **273**
- **不引入策略引擎依赖**（Casbin/Opa 都不引）→ 依赖树基本不变
- 表达式解析器**自研**（不用 nom / pest —— 只需 Pratt Parser 级别的几百行；
  但如果 `nom` 已在树里则可复用，**不要为此新增依赖**）
- 日常命令 `cargo check`；测试 `cargo test -- --test-threads=2`
- **测试只用 `base_test`**；**`base_dev` 不要写**
- ⚠️ **应用连接必须用普通角色 `base_app`**（超管会被 `require_isolation` 拒绝启动）
- ⚠️ `cargo test` **只能有一个 `--`**

---

## 10. 风险

| # | 风险 | 应对 |
|---|------|------|
| R1 | **兼岗时数据范围算错**（把并集当默认 → 可见范围被撑大 → 数据泄漏） | 默认主部门；并集必须显式开启；**专门的多节点兼岗测试用例** |
| R2 | 主部门缺失时退化成 `ALL` | 明确 fail closed 到 `SELF`；测试覆盖 |
| R3 | 自定义过滤变成 SQL 注入面 | 受限 DSL：白名单字段 + 参数化 + 禁函数调用；** fuzz/逃逸测试** |
| R4 | 权限撤销后 60 秒内仍可用 | 变更时**主动失效**；测试覆盖 |
| R5 | C-2 扩展检查可被绕过（多个鉴权入口） | 入口唯一性有测试守着 |
| R6 | 权限点通配写错导致越权 | `*:*` 禁止；只允许 `module:*` 与 `module:resource:*` |
| R7 | 平台 crate 引入后 `check-deps` 门禁被绕过 | `platform-rbac` 必须纳入平台 crate 集合；破坏性测试 |
