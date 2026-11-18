# 设计摘要：用户与组织架构（REQ-008）

- **需求编号**: REQ-008
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **依赖**: [REQ-007 认证与账号体系](../REQ-007-auth-account/design-summary.md)（账号主体先存在）
- **接口契约**: [api-conventions v1](../../contracts/api-conventions/v1.md)
- **扩展点契约**: [extension-points v1](../../contracts/extension-points/v1.md)
- **性质**: 需求细化附件
- **更新时间**: 2026-11-18 12:00

---

## 0. 一句话

组织架构是**权限模型的一个维度** —— 本期交付的 org 结构与成员关系，
必须能被 REQ-009（RBAC + 数据范围）直接消费，而不是各写一套。

---

## 1. 组织模型：邻接表 + 递归 CTE

**决定**：组织树用**邻接表**（`org_node.parent_id`），查询子树用 PostgreSQL 递归 CTE。

| 方案 | 优点 | 缺点 | 本期 |
|---|---|---|---|
| **邻接表 + 递归 CTE** | 写入简单、无需扩展、移动节点 O(1) | 深度查询递归、无法直接按路径查 | ✅ **采用** |
| 物化路径（ltree） | 子树查询极快 | 需要 `CREATE EXTENSION ltree`（私有化部署常无权限）；移动节点要更新全部后代 | ❌ |
| 闭包表 | 查询快 | 写入要维护闭包行 | ❌ |

**理由**：递归 CTE 是 PG 原生能力，**不依赖任何扩展** —— 私有化部署时这是硬约束
（客户环境常常装不了扩展）。组织树深度通常 < 10，递归 CTE 性能足够。

**不把 L1 的假设写死**：`parent_id` 可空（根节点），类型用 UUID，
将来要换物化路径时数据层可替换。

### 1.1 节点类型

```text
company  公司（顶层，一个租户可有多个）
dept     部门
team     小组
position 岗位（挂在部门下，不算组织层级，但参与数据范围计算）
```

**注意**：`position`（岗位）**不是** org_node 的一种，而是一个**挂在部门上的属性**。
原因：岗位是「人在部门里的角色」，若做成组织节点会污染树结构与汇报关系。

### 1.2 汇报关系

`org_node.manager_id`（直属上级）+ `user_org.membership`（用户在哪个节点、什么角色）。

- **一个人在多个节点**是常态（兼岗）→ 成员关系是**独立表**，不是 org_node 上的字段
- 汇报关系是**人的属性**（`user.manager_id`），不是节点的属性 ——
  否则「部门经理」和「我的上级」会混为一谈

---

## 2. 数据表（租户作用域）

全部带 `tenant_id`，并按 REQ-006 的 `tenantScoped` 声明生成 RLS policy。

| 表 | 关键列 | 说明 |
|---|---|---|
| `org_node` | `id` / `tenant_id` / `parent_id` / `type` / `code` / `name` / `manager_id` / `path_labels` | 组织节点 |
| `user_org` | `user_id` / `org_node_id` / `role` / `is_primary` | **用户 ↔ 节点**多对多 |
| `user_position` | `user_id` / `position_code` / `org_node_id` | 岗位 |
| `external_identity` | `provider` / `external_id` / `user_id` / `last_synced_at` | SSO 身份映射（REQ-007 建，这里扩展） |
| `sync_conflict` | `source` / `external_id` / `field` / `local_value` / `remote_value` / `status` | 同步冲突待处理队列 |

`path_labels` 存一份物化的「祖先 id 列表」（如 `[root, dept-a, team-b]`），
**作为查询加速字段**，不作为权威来源（权威是 `parent_id`）。
写时维护、读时可用；万一与 `parent_id` 不一致，以 `parent_id` 为准并可重建。

---

## 3. SCIM 2.0 出站（底座 → 第三方）

**决定**：**自己实现 SCIM 核心，不引入 SCIM 库**。

SCIM 2.0 的 User / Group 资源 schema 不复杂，核心是：

- `GET /Users?filter=userName eq "xxx"` → 查询
- `POST /Users`、`PUT /Users/{id}` → 创建/更新
- `PATCH /Users/{id}` → 局部更新（`active` 变更用这个）
- `Group` 资源对应组织节点

**为什么不用库**：Rust 生态的 SCIM 库成熟度一般，且会引入不确定的依赖。
自己实现只需 JSON + HTTP（`reqwest` 或 `axum` 的 client），**可控且轻**。

### 3.1 幂等与映射

- 幂等键：`(provider, external_id)`；推送用 **PUT 语义**（幂等）
- 映射表：`external_identity` + 每个 provider 的**字段映射配置**
  （不同 IdP 的用户名属性、姓名字段、邮箱字段位置不同）
- 失败重试：指数退避，最多 N 次；失败进 `sync_conflict` 或死信

### 3.2 出站触发时机

| 时机 | 动作 |
|---|---|
| 用户创建 / 更新 / 禁用 | 推送该用户 + 其所在 Group |
| 组织节点创建 / 移动 / 重命名 | 推送对应 Group |
| **手动全量同步** | 全量对账（`xtask` 或管理操作） |

**推送必须异步** —— 不能在用户请求里同步调第三方（第三方挂了不能拖垮本地）。

---

## 4. LDAP / AD 入站（第三方 → 底座）

**决定**：用 `ldap3` crate（Rust 生态成熟的事实标准）。

### 4.1 同步模式

| 模式 | 说明 |
|---|---|
| **增量**（默认） | 定时拉取 `modifyTimestamp` 在上次同步点之后的条目 |
| **全量对账** | 定期（如每周）全量拉取，与本地比对，产出差异 |

增量用 LDAP 的修改时间戳过滤；全量对账做**双向比对**。

### 4.2 属性映射

AD 与 OpenLDAP 的属性差异很大 → **映射配置外置**（每个源一份配置）：

```json
{
  "source": "corp-ad",
  "base_dn": "DC=corp,DC=example,DC=com",
  "filter": "(objectClass=user)",
  "attributes": {
    "external_id": "objectGUID",
    "username": "sAMAccountName",
    "email": "mail",
    "display_name": "displayName",
    "department": "department",
    "disabled": { "op": "bit_and", "attr": "userAccountControl", "mask": 2 }
  }
}
```

**离职检测**：AD 用 `userAccountControl` 的某一位；OpenLDAP 用 `nsAccountLock`。
上面的 `"disabled"` 用**位运算表达式**而不是简单布尔 —— 这是 AD 的实际坑。

### 4.3 冲突处理策略（可配）

| 策略 | 行为 |
|---|---|
| `local_wins` | 底座为准，外部改动被忽略（但记录） |
| `remote_wins` | 外部为准，覆盖底座（**危险**：会覆盖底座里用户手动改的字段） |
| `manual` | **进 `sync_conflict` 队列，等人工裁决** |

**默认策略：`manual`** —— 静默覆盖数据是不可接受的。

**字段级粒度**：冲突队列记录到**字段**级（`field` / `local_value` / `remote_value`），
而不是整条记录 —— 否则一个字段不同就得人工处理整条记录。

---

## 5. 离职自动禁用

检测到离职（AD 位 / SCIM `active: false`）时，**一次性做完**：

1. 禁用账号（`user.status = disabled`）
2. **撤销该用户全部 session**（复用 REQ-007 的 refresh family 作废）
3. 解绑所有角色与组织成员关系
4. 产生审计事件
5. 推送给相关方（HR / 直属上级）—— 走 REQ-003 的 `NotificationSender` 扩展点

**第 2 条最容易漏** —— 账号禁用了但旧 token 还能用 15 分钟（access TTL），
刷新令牌也能换新 token。必须显式撤销。

---

## 6. 组织变更事件

组织架构变了，要通知受影响的人。复用 REQ-003 的扩展点：

| 事件 | 通知对象 |
|---|---|
| 用户被加入/移出组织节点 | 该用户 + 新旧节点的相关人 |
| 节点移动 / 重命名 | 该节点下所有成员 |
| 节点删除 | 该节点下所有成员（需先迁移或确认） |

**节点删除必须先处理成员** —— 不能留下悬空成员关系。

---

## 7. 与权限模型（REQ-009）的接口

本期的数据结构是 REQ-009 的**输入**，两者的边界要划清：

| 本期（REQ-008）提供 | REQ-009 消费 |
|---|---|
| `org_node`（层级、类型、编码） | 数据范围的「本部门及下级」判定 |
| `user_org`（多节点成员关系、is_primary） | 数据范围的主部门判定 |
| `user_position`（岗位） | 岗位型权限 |
| `user.manager_id`（汇报关系） | 「我的下属」判定 |
| `external_identity` | SSO 用户的权限挂载点 |

**REQ-008 不实现任何权限判定逻辑** —— 只提供结构。判定在 REQ-009。

---

## 8. 依赖预算

| crate | 用途 | 代价 |
|---|---|---|
| `ldap3` | LDAP/AD 入站 | ~10 crate |
| SCIM 出站 | **不引入库**，用现有 HTTP client + serde_json | 0 |

**认证模块（REQ-007）已引入 argon2 / jsonwebtoken / totp-rs**，
本期只加 `ldap3`。Sprint 2 结束时依赖树预计 175 → 200+ crate，**要重新实测冷编译**。

---

## 9. 本期交付边界

**做**

- [ ] `org_node` / `user_org` / `user_position` 表 + 租户 RLS policy
- [ ] 组织树 CRUD + 递归 CTE 查子树 + `path_labels` 加速字段
- [ ] 用户 ↔ 节点多对多成员关系、岗位、汇报关系
- [ ] **SCIM 2.0 出站**：User / Group 的查询、创建、更新、PATCH；幂等 PUT；异步推送 + 重试
- [ ] **LDAP / AD 入站**：`ldap3` 拉取、增量 + 全量对账、**字段级**映射配置
- [ ] 冲突队列 `sync_conflict` + 人工裁决入口（数据层 + API，界面放后续）
- [ ] 离职自动禁用（含**撤销全部 session**）
- [ ] 组织变更事件 → `NotificationSender`
- [ ] 同步状态的可见性（上次同步时间、同步错误计数）

**不做**

- 同步的**管理界面**（数据层 + API 先做，界面放 REQ-011 前端 SDK 之后）
- WebAuthn（REQ-007 留了接口）
- 跨租户的组织架构共享
- SCIM **入站**（第三方 → 底座）—— 只做出站
- 组织架构的多版本历史

---

## 10. 风险

| # | 风险 | 应对 |
|---|------|------|
| R1 | **`remote_wins` 策略静默覆盖底座数据** | 默认 `manual`；`remote_wins` 需显式配置 |
| R2 | AD 的 `userAccountControl` 位运算搞错 → 离职没检测到 | 映射用表达式而非布尔；有专门的离职检测测试 |
| R3 | 同步阻塞用户请求 → 第三方慢把本地拖垮 | **推送与拉取都异步**，请求路径上不等第三方 |
| R4 | 组织节点删除留下悬空成员 | 删除前必须迁移或确认；契约测试覆盖 |
| R5 | 离职只禁用了账号但 token 仍有效 | 显式撤销全部 refresh family；有测试 |
| R6 | `path_labels` 与 `parent_id` 不一致 | 以 `parent_id` 为权威；提供重建命令 |
| R7 | 递归 CTE 在深层树上慢 | 组织树深度 < 10 够用；`path_labels` 做加速；必要时物化 |
