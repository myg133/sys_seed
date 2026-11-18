# REQ-009「RBAC 权限与数据范围」—— 测试用例总索引

- **需求编号**: REQ-009「RBAC 权限与数据范围」
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: AC-001（权限点注册表驱动权限判定）、AC-002（数据范围隔离生效）
- **来源**: [acceptance.md](../acceptance.md)（AC-S-033 ~ AC-S-048，**2026-11-18 21:40 已按 QA 用例评审修订**）、[design-summary.md](../design-summary.md)、[api-conventions v1](../../../contracts/api-conventions/v1.md)、[extension-points v2](../../../contracts/extension-points/v2.md)
- **格式基准**: [REQ-001 用例总索引](../../REQ-001-platform-foundation/test-cases/index.md)（同构：概览 / 文件清单 / 夹具 / 命名约定 / AC→TC 矩阵 / 外部依赖 / 已裁定约定 / 执行顺序 / 修订记录）
- **状态**: 待实现（用例本身尚未执行，**不得据此判定需求通过**）
- **版本**: 第 2 版 —— 对齐 BA 对 12 条 AC 问题与 16 条待确认约定的裁定

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **61**（TC-009-001 ~ TC-009-061，编号连续无跳号） |
| 覆盖 AC | **16 / 16**（AC-S-033 ~ AC-S-048，无遗漏） |
| 优先级分布 | P0 = 57、P1 = 4、P2 = 0 |
| 自动化分布 | 可自动化 = 59、半自动 = 2、手工 = 0 |
| 用例文件 | 4 个（另本索引 1 个） |
| 每条用例字段 | 标题 / 关联 AC / 前置条件 / 测试步骤 / 预期结果 / 类型 / 优先级 / 自动化 / 设计方法（**9 项全部齐备**，脚本校验 60/60 通过） |

> 每条 AC 在矩阵中至少映射 1 条 TC；**类型含「安全」的 13 条 AC**（AC-S-033、034、036、037、039、040、042、043、044、045、046、047、048）**全部 ≥ 3 条**，且每条都配有「正常路径 + 异常/攻击路径」。
> 覆盖条数最少的 AC：**AC-S-034**（3 条）、**AC-S-040**（3 条）、**AC-S-043**（3 条）、**AC-S-048**（3 条）—— 条数少是因为 AC 本身只描述一件事，且均含「正常 + 越权/跨租」两路。
> **第 2 版变化**：① 新增 **TC-009-030**（AC-S-041「解绑 R3 后退为 `OWN_ORG`」—— 区分并集与交集的判别步骤）；② 新增 **TC-009-005**（AC-S-034 ★「管理接口自身受保护 / 自举防护」—— BA 在 21:40 后追加的条款）；为腾出编号，原 TC-009-030 ~ TC-009-059 顺延为 031 ~ 060。

## 2. 文件清单

| 文件 | 能力域 | AC 范围 | 用例数 |
|---|---|---|---|
| [A-registry-role.md](A-registry-role.md) | 权限点注册表与角色模型 | AC-S-033 ~ AC-S-037 | 15 |
| [B-data-scope.md](B-data-scope.md) | 数据范围求值（5 种 scope / 兼岗 / fail closed / DSL / 属性级） | AC-S-038 ~ AC-S-043 | 25 |
| [C-authz-middleware.md](C-authz-middleware.md) | 鉴权中间件、错误体与 SQL 层过滤 | AC-S-044、AC-S-045、AC-S-048 | 12 |
| [D-contract-cache.md](D-contract-cache.md) | 契约检查 C-5 与缓存主动失效 | AC-S-046、AC-S-047 | 9 |

---

## 3. 测试夹具（造数方式）

> **原则**：组织侧夹具一律基于 **REQ-008 已真实落库的表**（`org_node` / `user_org`，迁移 `20261118000700_org_schema.up.sql`）—— 兼岗 = `user_org` 多行、`is_primary` 标主部门；**不另造影子表**。
> 业务侧夹具表为 **`sandbox_scope_item`**（BA 裁定：放在 `modules/sandbox/migrations/`，**不新建业务模块**）。
> 账号由 REQ-007 的 IAM 体系提供（`platform_iam_user.id` 是 `TEXT`），统一初始密码 `Passw0rd!`。

### 3.1 租户

| ID | tenant_id | 用途 |
|---|---|---|
| T1 | `tenant-a` | 主测试租户（全部数据范围 / 角色用例都在这里） |
| T2 | `tenant-b` | 跨租对照租户（存在性泄露、跨租 id、租户头不一致） |

### 3.2 组织树（T1，基于真实表 `org_node`）

```text
dept-hq                总部          company  parent_id = NULL
├── dept-a             部门 A        dept     parent_id = dept-hq
│   └── dept-a-1         部门 A 子节点   dept     parent_id = dept-a
├── dept-b             部门 B        dept     parent_id = dept-hq
│   └── dept-b-1         部门 B 子节点   dept     parent_id = dept-b
└── dept-c             部门 C        dept     parent_id = dept-hq
```

- 节点 id 与 AC-S-038 / AC-S-039 原文的 `dept-a` / `dept-a-1` / `dept-b` / `dept-b-1` **一致**。
- `path_labels` 是加速字段（根到本节点，含本节点）；**权威来源是 `parent_id`**。
- T2 另有：`dept-t2-hq`（company）、`dept-t2-a`、`dept-t2-b`（形状与 T1 镜像，用于「兼岗默认行为不随租户变化」的对照）。
- `dept-a` 是**子树递归**的验证点（`dept-a-1` 是子节点、`dept-b-1` 是**别的分支**，不得被包含）。

### 3.3 用户、成员关系与角色（真实表 `user_org` + `platform_user_role`）

| user_id | 账号 | `user_org` 行（`org_node_id` / `is_primary`） | 绑定角色（`data_scope`） | 用途 |
|---|---|---|---|---|
| `u-1001` | owner@example.com | `dept-a` / **TRUE** | `rbac-owner`（OWN_ORG）+ `rbac-manager`（管理权限点） | OWN_ORG 基线、撤销即时生效、主部门变更 |
| `u-1002` | parttime@example.com | `dept-a` / **TRUE**；`dept-b` / FALSE | `rbac-owner`（OWN_ORG）、`rbac-owner-below`、`rbac-union`、`rbac-union-below` | **兼岗用户**（AC-S-039） |
| `u-1003` | noprimary@example.com | `dept-a` / FALSE（**无任何主部门**） | `rbac-owner`（OWN_ORG） | **主部门缺失**（AC-S-040 防御层） |
| `u-1004` | dualprimary@example.com | `dept-a` / **TRUE**；`dept-c` / **TRUE**（歧义由测试内临时移除索引造出） | `rbac-owner`（OWN_ORG） | **主部门歧义**（AC-S-040 防御层） |
| `u-1005` | multirole@example.com | `dept-a` / TRUE | `rbac-multi-a` + `rbac-multi-b` + `rbac-multi-c` | 多角色并集 / 权限点级覆盖（AC-S-041） |
| `u-1006` | admin@example.com | `dept-hq` / TRUE | `platform-admin`（ALL，内置） | 内置角色不可删降权（AC-S-037） |
| `u-1007` | noperm@example.com | `dept-a` / TRUE | `rbac-self`（SELF，**不含** `sandbox:scope-item:approve`） | 缺权限点 → 403（AC-S-043/044）；SELF scope 探针 |
| `u-1008` | other@example.com | `dept-a` / TRUE | `rbac-owner-below`（OWN_ORG_AND_BELOW） | 子树递归（AC-S-038） |
| `u-1009` | custom@example.com | `dept-a` / TRUE | `rbac-custom`（CUSTOM） | CUSTOM scope（AC-S-038/042） |
| `u-1010` | allscope@example.com | `dept-a` / TRUE | `rbac-all`（ALL） | ALL scope（AC-S-038） |
| `u-1011` | norole@example.com | `dept-a` / TRUE | **无角色** | 用户默认 `SELF`（AC-S-041） |
| `u-1012` | scope-owner@example.com | `dept-a-1` / TRUE | `rbac-owner`（OWN_ORG） | AC-S-045 的「范围内资源」探针（见 §9 新问题 #4） |
| `u-1013` | plain@example.com | `dept-a` / TRUE | **只有内置角色 `platform-user`** | **普通用户 M** —— 自举防护用例（AC-S-034 ★，TC-009-005） |
| `u-1014` | rbacadmin@example.com | `dept-hq` / TRUE | `rbac-manager`（ALL，含 `rbac:role:read` / `rbac:role:write`） | **管理用户 A** —— 自举防护用例的对照组（AC-S-034 ★，TC-009-005） |
| `u-2001` | t2user@example.com（T2） | `dept-t2-hq` / TRUE | `rbac-self` | 跨租对照 |

- `user_org.id` 取 `uo-1001` 起（`TEXT` 主键）；`role` 列（岗位性角色，如「部门经理」）一律填 `NULL` —— 它**不是**平台的权限角色。
- **兼岗三种形态各自独立成夹具**：`u-1002`（兼岗，主+非主）、`u-1003`（无主部门）、`u-1004`（两条主部门）。
- `u-1004` 的歧义状态由**测试内临时 `DROP INDEX user_org_one_primary_idx`** 造出（AC-S-040 第 3 步），夹具本身不预置两条主部门行。

### 3.4 角色（`platform_role` + `platform_role_permission`）

| 角色 code | `data_scope` | `builtin` | 权限点 | 用途 |
|---|---|---|---|---|
| `platform-admin` | ALL | **true** | **注册表全集（动态得出，不写死）** | 内置角色保护（AC-S-037） |
| `platform-user` | SELF | **true** | 无 | 内置角色保护（AC-S-037） |
| `rbac-manager` | ALL | false | `rbac:role:read`、`rbac:role:write` | **管理权限点载体** —— `/api/rbac/*` 的调用者默认绑定它（AC-S-034 ★ 自举防护） |
| `rbac-owner` | OWN_ORG | false | `sandbox:scope-item:read`、`sandbox:scope-item:approve` | OWN_ORG 基线 |
| `rbac-owner-below` | OWN_ORG_AND_BELOW | false | `sandbox:scope-item:read` | 递归子树 |
| `rbac-self` | SELF | false | `sandbox:scope-item:read` | SELF |
| `rbac-all` | ALL | false | `sandbox:scope-item:read` | ALL |
| `rbac-custom` | CUSTOM | false | `sandbox:scope-item:read`（`custom_filter = $record.amount <= 10000`） | CUSTOM 条件 |
| `rbac-union` | OWN_ORG + `use_all_memberships=true` | false | `sandbox:scope-item:read` | 兼岗并集（AC-S-039） |
| `rbac-union-below` | OWN_ORG_AND_BELOW + `use_all_memberships=true` | false | `sandbox:scope-item:read` | 兼岗并集（子树口径） |
| `rbac-multi-a` | OWN_ORG | false | `sandbox:scope-item:read`、`sandbox:scope-item:export` | 多角色（AC-S-041） |
| `rbac-multi-b` | OWN_ORG | false | `sandbox:scope-item:read`、`sandbox:scope-item:export`（**权限点级 `data_scope = SELF`**） | 权限点级覆盖 |
| `rbac-multi-c` | OWN_ORG_AND_BELOW | false | `sandbox:scope-item:read` | 最宽松 scope |
| `rbac-wildcard` | OWN_ORG | false | `sandbox:*` | 模块级通配（AC-S-036） |
| `rbac-wildcard-res` | OWN_ORG | false | `sandbox:demo:*` | 资源级通配（AC-S-036） |
| `rbac-wildcard-bad` | OWN_ORG | false | 无（两条通配写入**均须被拒**） | 禁止全通过（AC-S-036） |
| `rbac-other-scope` | OWN_ORG | false | `sandbox:scope-item:read` | 换绑不改变兼岗默认口径（AC-S-039） |
| `rbac-inh-a` / `rbac-inh-b` / `rbac-inh-c` | OWN_ORG | false | 各自 1 个不同权限点 | 继承展开（AC-S-035） |
| `rbac-cycle-a` / `rbac-cycle-b` / `rbac-self-inherit` / `rbac-tri-a` / `rbac-tri-b` / `rbac-tri-c` | OWN_ORG | false | 无 | 成环检测（AC-S-035） |
| `rbac-custom-a` / `rbac-custom-b` | CUSTOM | false | `sandbox:scope-item:read` | 多 CUSTOM 取交集（AC-S-041） |
| `rbac-tmp` / `rbac-t2-only` | OWN_ORG / SELF | false | — | 角色 CRUD 与跨租（AC-S-034） |

### 3.5 业务数据（`sandbox_scope_item`，T1 共 **14** 条）

| id | name | amount | org_node_id | created_by | 说明 |
|---|---|---|---|---|---|
| `item-a1` | `a` | 5000 | `dept-a` | `u-1001` | 主部门直属 |
| `item-a2` | `a2` | 5000 | `dept-a-1` | `u-1002` | 主部门子节点 |
| `item-b1` | `b1` | 5000 | `dept-b` | `u-1002` | 兼岗部门直属 |
| `item-b2` | `b2` | 5000 | `dept-b-1` | `u-1008` | 兼岗部门子节点 |
| `item-c1` | `c1` | 5000 | `dept-c` | `u-1008` | 他部门 |
| `item-s1` | `s1` | 5000 | `dept-c` | `u-1007` | u-1007 自建 |
| `item-s2` | `s2` | 5000 | `dept-b` | `u-1007` | u-1007 自建 |
| `item-s3` | `s3` | 5000 | `dept-c` | `u-1007` | u-1007 自建 |
| `item-big` | `big` | **20000** | `dept-a` | `u-1007` | 大额单（属性级校验 / CUSTOM 排除） |
| `item-v1` | `v1` | 5000 | `dept-c` | `u-1003` | u-1003 自建（fail closed 非空判据） |
| `item-w1` | `w1` | 5000 | `dept-c` | `u-1004` | u-1004 自建（fail closed 非空判据） |
| `item-m1` | `m1` | 5000 | `dept-a-1` | `u-1005` | u-1005 自建（SELF 判别） |
| `item-n1` | `n1` | 5000 | `dept-c` | `u-1011` | u-1011 自建（用户默认 SELF 判据） |
| `item-inject` | `a' OR '1'='1` | 5000 | `dept-c` | `u-1008` | **注入串字面量样本**（AC-S-042 判据三） |

T2 另有 `item-t2-1`（`tenant-b`、`dept-t2-hq`、`u-2001`）。

> 另有两个**刻意构造**的 id：`item-not-exist-9999`（**永不落库**，AC-S-045 的存在性对照）；`item-a3`（TC-009-018 中**临时插入**的孙节点记录，`org_node_id = dept-a-1-sub`，用于验证递归深度 ≥ 2）。

**五种 scope 的期望结果集**（探针用户主部门均为 `dept-a`；AC-S-038 要求**逐条列出**、且**两两不相等**）：

| scope | 探针 | 期望结果集（逐条） | 条数 |
|---|---|---|---|
| ALL | `u-1010` | 全部 14 条 | 14 |
| CUSTOM（`amount <= 10000`） | `u-1009` | 除 `item-big` 外 13 条 | 13 |
| SELF | `u-1007` | `item-s1`、`item-s2`、`item-s3`、`item-big` | 4 |
| OWN_ORG_AND_BELOW | `u-1008` | `item-a1`、`item-big`、`item-a2`、`item-m1` | 4 |
| OWN_ORG | `u-1001` | `item-a1`、`item-big` | 2 |

> 五个集合**两两不相等**（按**集合元素**判定）。注意 `SELF` 与 `OWN_ORG_AND_BELOW` **条数相同（均 4）但元素不同** —— 这正是 AC-S-038「两两不相等」按集合判定的意义所在（TC-009-020 显式覆盖这一点）。
> `item-v1` / `item-w1` 的存在是为了让「主部门缺失 / 歧义 → 退化为 SELF」**不是空集**（否则「看到空」会蒙混过关，见 TC-009-025 / TC-009-026）。

### 3.6 权限点注册表（`platform_permission`）

| 权限点 code | 来源 manifest | 说明 |
|---|---|---|
| `sandbox:demo:read` / `sandbox:demo:write` | `modules/sandbox/manifest.json` | **仓库现状已有**的真实权限点（AC-S-036 / AC-S-047 用） |
| `sandbox:scope-item:read` | `modules/sandbox/manifest.json` | **需新增**：范围夹具表的读权限点 |
| `sandbox:scope-item:approve` | `modules/sandbox/manifest.json` | **需新增**：携带 `condition: "$record.amount <= 10000"`（属性级校验） |
| `sandbox:scope-item:export` | `modules/sandbox/manifest.json` | **需新增**：AC-S-041 的权限点级 scope 载体 |
| `org:org-node:read`、`org:org-node:write`、`org:user-org:read`、`org:user-org:write`、`org:user-position:read`、`org:user-position:write`、`org:sync-conflict:read`、`org:sync-conflict:resolve`、`org:scim-sync:run` | `crates/platform-org/manifest.json` | 9 个平台 crate 权限点（真实存在） |
| `rbac:role:read` | `crates/platform-rbac/manifest.json`（**需新建**，照 `crates/platform-org/manifest.json` 的先例） | **需新增**：读 `/api/rbac/*` 的管理权限点（编码为本用例集推导，见 §9.2 #J） |
| `rbac:role:write` | `crates/platform-rbac/manifest.json`（**需新建**） | **需新增**：写 `/api/rbac/*` 的管理权限点（AC-S-034 ★ 自举防护原文所举） |

- **不写死权限点总数** —— 注册表随权限点增减而变，所有涉及「全部权限点」的判定一律按**集合逐个比对**（AC-S-033 / AC-S-037 的新口径）。
- 注册表与 `capabilities/permissions.json`（`cargo xtask capabilities` 生成）必须**逐条相等** —— 这正是 AC-S-033 的对账对象。
- ⚠️ **不新建** `modules/demo` / `modules/order`（BA 裁定 #8）；所有夹具权限点都挂在 `modules/sandbox` 的 manifest 上。

### 3.7 平台表与路由

| 表 | 关键列 |
|---|---|
| `platform_permission` | `code` / `name` / `condition`（**平台级，无 `tenant_id`**） |
| `platform_role` | `tenant_id` / `id` / `code` / `name` / `data_scope` / `custom_filter` / `use_all_memberships` / `builtin` |
| `platform_role_permission` | `tenant_id` / `role_id` / `permission_code` / `data_scope`（**可空 = 不覆盖角色级**） |
| `platform_role_inherit` | `tenant_id` / `role_id` / `parent_role_id` |
| `platform_user_role` | `tenant_id` / `user_id` / `role_id` |
| `platform_permission_epoch` | `tenant_id`（PK） / `epoch` / `updated_at` |

- 业务数据表 `sandbox_scope_item`（`modules/sandbox/migrations/`）：`tenant_id` / `id` / `name` / `amount` / **`org_node_id`** / **`created_by`**。
- `modules/sandbox` 的路由（契约 C2 `/api/<module>/<resource>/<action>`）：`GET /api/sandbox/scope-item`、`GET /api/sandbox/scope-item/{id}`、`POST /api/sandbox/scope-item/approve`、`POST /api/sandbox/scope-item/export`、`GET /api/sandbox/demo`。
- 路由**不由 platform-server 挂载**，sandbox 的用例在自己的测试里用 `ServiceExt::oneshot` **进程内驱动**；`/api/rbac/*` 由 `platform-server` 挂载，走真机 HTTP。

---

## 4. 命名与接口约定（REQ-009 的实际取值）

| 约定项 | 取值 | 来源 |
|---|---|---|
| 权限点编码 | `module:resource:action`，全小写连字符；允许通配 `module:*` / `module:resource:*`，**禁止** `*:*` 与 `*:resource:action` | 契约 C4 + design §3.1 |
| 平台 API 前缀 | `/api/rbac`（角色、授权、绑定、权限点注册表查询） | 契约 C1 + design §8 |
| 平台 API 子路径 | `/api/rbac/roles`、`/api/rbac/roles/{id}`、`/api/rbac/roles/{id}/permissions`、`/api/rbac/roles/{id}/inherit`、`/api/rbac/users/{user_id}/roles`、`/api/rbac/permissions` | design §8 + `platform-org` 同构风格（**BA 已裁定采纳**） |
| 业务 API 路径 | `/api/<module>/<resource>/<action>`，动作放**末段** | 契约 C2 |
| 鉴权入口 | `require_permission("<code>")` —— 平台**唯一**入口，**定义在 `crates/platform-core/src/extension/permission.rs`**（不在 `platform-rbac`），参数必须字面量 | design §6.1 / §6.3 + R10 |
| 越权响应码 | 未认证 **401**（`UNAUTHENTICATED`）／**缺权限点** **403**（`INSUFFICIENT_PERMISSION`）／**有权限点但记录不在范围** **404**（`NOT_FOUND`） | 契约 C5 + design §6.2（403 / 404 分工） |
| 错误体 | RFC 9457 `application/problem+json`：`type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id` / `params`；`detail` **必须英文 ASCII**；`trace_id` 必带 | 契约 C6 + `platform-iam/src/http.rs` |
| `detail` 的边界 | **可以**含被拒的**权限点编码**；**不得**含被拒**资源**的业务细节（标题 / 金额 / 归属人 / `org_node_id`） | AC-S-044（2026-11-18 修订） |
| 租户 / 空间上下文头 | `X-Tenant-Id` / `X-Space-Id`；请求头租户与令牌租户不一致 → **404** | 契约 C3 + `platform-iam/src/http.rs` |
| 数据范围取值 | `ALL` / `OWN_ORG` / `OWN_ORG_AND_BELOW` / `SELF` / `CUSTOM` | design §4 |
| 主部门判定 | `user_org.is_primary = TRUE`（真实列名） | REQ-008 迁移 |
| 并集开关 | `platform_role.use_all_memberships`（**角色级**，默认 `false`） | design §4.3（BA 裁定） |
| 权限点级 scope | `platform_role_permission.data_scope`（**可空 = 不覆盖角色级**） | design §4.3（BA 裁定） |
| CUSTOM 条件 | `platform_role.custom_filter` | design §4.3（BA 裁定） |
| 权限点条件 | `platform_permission.condition` | design §3（BA 裁定） |
| 内置角色 | **按租户物化**（租户初始化时在该租户下建 `platform-admin` / `platform-user` 两行，`builtin = true`） | design §3.3（BA 裁定） |
| `platform-admin` 的权限集合 | **注册表当前全部权限点，动态得出**（不写死、不抽样、不是通配 `*`） | design §3.3（BA 裁定）+ AC-S-037 |
| 租户查询构造器 | `TenantQuery`：语句里**必须**有且仅有一个 `{tenant}`（渲染为 `$1`），业务参数用 `?`（渲染为 `$2`…）；新增 `with_scope()`；`BindValue` 需新增数组变体（`= ANY($n)`） | `platform-db/src/query.rs` + design §8.1 |
| RLS policy | 名 `<表名>_tenant_isolation`；`USING (tenant_id = current_setting('app.tenant_id', true))` + `WITH CHECK` | `platform-db/src/rls.rs` |
| 契约检查 | C-1 ~ C-5 同一套件；C-5 新增于 extension-points **v2**；**入口唯一性按「定义处」判定** | `platform-contracts/src/checks.rs` + v2 §4 + AC-S-046 第 5 步 |
| 隔离级别 | L1 共享表 + `tenant_id` + PostgreSQL RLS（默认） | ADR-0001 D2 |
| 跑法 | `cargo test --workspace -- --test-threads=2`（`cargo test` **只能有一个 `--`**）；真实库层 `DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test`，**应用连接必须用普通角色 `base_app`**（超管会被 `require_isolation` 拒绝启动） | `platform-org/tests/org_real_db.rs` + `platform-server/src/isolation.rs` |

---

## 5. AC → TC 覆盖矩阵（16 / 16）

| AC | 标题 | 覆盖 TC | 条数 | 外部依赖 | 最高优先级 |
|---|---|---|---|---|---|
| AC-S-033 | 权限点注册表双向对账且不一致即启动失败 | TC-009-001, TC-009-002, TC-009-003 | 3 | 真实库 `base_test` + **可写库**（注入差异） | P0 |
| AC-S-034 | 角色、授权与绑定可用且带租户维度 | TC-009-004, TC-009-005, TC-009-006 | 3 | 真实库 `base_test` + **`base_app` 应用连接** + **真机 HTTP**（自举防护） | P0 |
| AC-S-035 | 角色继承展开且成环被拒 | TC-009-007, TC-009-008, TC-009-009 | 3 | 无 | P0 |
| AC-S-036 | 权限点通配语义正确且禁止全通过 | TC-009-010, TC-009-011, TC-009-012 | 3 | 无（用真实存在的 `sandbox:` / `org:` 前缀） | P0 |
| AC-S-037 | 内置角色不可删除也不可降权 | TC-009-013, TC-009-014, TC-009-015 | 3 | 真实库 `base_test` | P0 |
| AC-S-038 | 数据范围五种语义求值正确 | TC-009-016, TC-009-017, TC-009-018, TC-009-019, TC-009-020 | 5 | 真实库 `base_test` + **表 `sandbox_scope_item`** | P0 |
| AC-S-039 | 兼岗默认按主部门，取并集必须显式开启 | TC-009-021, TC-009-022, TC-009-023, TC-009-024 | 4 | 真实库 `base_test` | P0 |
| AC-S-040 | 主部门缺失或歧义时 fail closed 到 SELF | TC-009-025, TC-009-026, TC-009-027 | 3 | 真实库 `base_test`（TC-009-026 需临时 `DROP INDEX`） | P0 |
| AC-S-041 | 数据范围优先级与多角色组合 | TC-009-028, TC-009-029, TC-009-030, TC-009-031, TC-009-032 | 5 | 真实库 `base_test` | P0 |
| AC-S-042 | 受限表达式 DSL 拒绝越界写法且不产生拼接 SQL | TC-009-033, TC-009-034, TC-009-035, TC-009-036, TC-009-037 | 5 | 单元测试 + 真实库（注入串那条） | P0 |
| AC-S-043 | 属性级校验（RBAC + ABAC 混合）生效 | TC-009-038, TC-009-039, TC-009-040 | 3 | 真实库 + 模块内进程内 HTTP | P0 |
| AC-S-044 | 鉴权中间件的三种拒绝语义与错误体形状 | TC-009-041, TC-009-042, TC-009-043, TC-009-044, TC-009-045 | 5 | 模块内进程内 HTTP | P0 |
| AC-S-045 | 404 不泄露资源存在性（资源类越权） | TC-009-046, TC-009-047, TC-009-048, TC-009-049 | 4 | 模块内进程内 HTTP | P0 |
| AC-S-046 | 已登记权限点必须有鉴权调用点，且鉴权入口唯一 | TC-009-053, TC-009-054, TC-009-055, TC-009-056, TC-009-057 | 5 | 无（负向用例用临时目录夹具） | P0 |
| AC-S-047 | 权限撤销在下一个请求即生效，不等 TTL | TC-009-058, TC-009-059, TC-009-060, TC-009-061 | 4 | 真实库 + **两个独立 checker 实例**（TC-009-061） | P0 |
| AC-S-048 | 数据范围过滤在 SQL 层生效且业务模块无法绕过 | TC-009-050, TC-009-051, TC-009-052 | 3 | 真实库 `base_test` + 模块内进程内 HTTP | P0 |

**合计 16 条 AC、61 条 TC，无遗漏 AC、无悬空 TC。**

---

## 6. 当前无法立即执行的 AC

> 前提：**所有**用例都要等 REQ-009 的实现落地后才可能执行。下表标的是「除平台实现之外，还需要哪些前置依赖」。

### 6.1 需要 Dev 先补的（**不补就只能写脚本、跑不出判定**）

| AC | 缺失项 | 影响 TC | 备注 |
|---|---|---|---|
| AC-S-038 / 039 / 040 / 041 / 048 | 表 **`sandbox_scope_item`**（`modules/sandbox/migrations/`，列含 `org_node_id` / `created_by`） | TC-009-016 ~ 031、049 ~ 051 | 现有 `sandbox_tenant_item` 只有 `title` / `revision`，**没有** `org_node_id` 与 `created_by`，无法承载 `OWN_ORG` / `SELF` 语义（design R12） |
| AC-S-036 / 037 / 043 / 044 / 045 | `modules/sandbox/manifest.json` 新增 3 个权限点：`sandbox:scope-item:read` / `sandbox:scope-item:approve`（带 `condition`）/ `sandbox:scope-item:export` | TC-009-010 ~ 014、036 ~ 048 | BA 裁定 #8：**不新建业务模块**，权限点加在 sandbox 的 manifest 上 |
| AC-S-046 | `modules/sandbox` 接入真实 `require_permission(...)` 调用点（`backend/src/http.rs`） | TC-009-057 | 现状 `backend/src/permission.rs` 只定义常量、从不鉴权 —— v2 下 C-5 必然失败，这是**预期效果**（design R8） |
| AC-S-046 | C-5 检查落地 + `require_permission` 入口在 `platform-core` | TC-009-053 ~ 057 | 契约检查 C-5（extension-points v2 §4） |
| AC-S-034 ★ | **`crates/platform-rbac/manifest.json`**（新建）声明平台级管理权限点 `rbac:role:read` / `rbac:role:write` | TC-009-005（及 A 域全部 `/api/rbac/*` 调用者） | AC-S-034 ★ 自举防护的前置；该 manifest 不存在则注册表对账（AC-S-033）与 C-5 都过不了（见 §9.2 #J） |

### 6.2 需要夹具时序配合的

| AC | 问题 | 影响 TC | 处理 |
|---|---|---|---|
| AC-S-040 | 部分唯一索引会让「两条主部门」在迁移后**不可达** | TC-009-026 | 按 AC-S-040 第 3 步：测试内临时 `DROP INDEX user_org_one_primary_idx` 造出歧义，验证**防御层**后重建索引（仅限 `base_test`） |

### 6.3 无外部依赖、平台实现落地即可跑的部分

AC-S-035、AC-S-036、AC-S-042（单元部分）、AC-S-046 共 **17 条 TC** 可纯单元测试跑；其余 **44 条** 需要真实库 `base_test`（应用连接用普通角色 `base_app`），其中 **12 条**还需要模块内进程内 HTTP（`ServiceExt::oneshot`），**1 条**（TC-009-060）需要两个独立 checker 实例连同一个 `base_test`，**1 条**（TC-009-005）需要真机 `base serve` 打 `/api/rbac/*`。

---

## 7. 已裁定约定（BA 2026-11-18 拍板，不再是待确认）

> 下表把上一轮登记的 16 条「待确认约定」与本次 AC 修订带来的新口径**逐条落地**。用例已按此编写。

| # | 事项 | BA 裁定 | 用例落点 |
|---|---|---|---|
| 1 | `/api/rbac/*` 子路径 | **采纳本用例集推导**（与 `platform-org` 同构风格） | §4 命名约定；A 域全部用例的 URL |
| 2 | 平台表与列 | 以 **design §3 的表**为准 | §3.7；A/B/D 域的库表断言 |
| 3 | `use_all_memberships` 归属 | **角色级**，`platform_role.use_all_memberships`（默认 false） | TC-009-023 / 023 |
| 4 | 权限点级 scope 存储列 | **`platform_role_permission.data_scope`**（可空 = 不覆盖角色级） | TC-009-029 |
| 5 | `CUSTOM` 条件存储位置 | **`platform_role.custom_filter`** | TC-009-019 / 031 |
| 6 | `platform_permission.condition` | **有这列**，存权限点的 ABAC 条件表达式 | TC-009-038 ~ 039 |
| 7 | 数据范围夹具表 | 改名 **`sandbox_scope_item`**，放在 **`modules/sandbox/migrations/`**，列含 `org_node_id` 与 `created_by` | §3.5；B / C 域全部用例 |
| 8 | 夹具业务模块 | **不新建** `modules/demo` / `modules/order`；统一用 **`modules/sandbox`**，需要更多权限点就在 sandbox 的 manifest 里加 | §3.6；A/B/C 域的权限点与路由 |
| 9 | AC-S-045「两次响应状态码相同」 | 已按 AC-S-045 改写：**无权资源 vs 不存在资源**一致；范围内资源 200 | TC-009-046 ~ 047 |
| 10 | `platform-admin` 的权限集合 | **不能写死数字**：= 注册表当前全部权限点，**动态得出**；逐个比对注册表内容（不抽样、不比数量）；注册表新增权限点后自动多出 | TC-009-014、TC-009-003 |
| 11 | `INSUFFICIENT_PERMISSION` 码归属 | 放 **`platform-rbac` 的错误类型**，**不**塞进 `platform-iam` | §4；A / C 域错误码断言 |
| 12 | 内置角色的租户维度 | **按租户物化**（租户初始化时建两条行，`builtin = true`），不做「平台级模板 + 租户引用」 | TC-009-013 / 013 / 014 |
| 13 | C-5 负向用例形态 | **采纳**临时目录夹具 | TC-009-053 ~ 055 |
| 14 | 数据范围的生效口径 | 以「可见数据集合」为准；平台若另提供求值只读接口，两者必须一致 | B 域 §B.0 判定口径 |
| 15 | `use_all_memberships` 的交互范围 | 只影响「本部门 / 本部门及下级」是否取所有成员节点并集，**不影响** `SELF` / `ALL` / `CUSTOM` | TC-009-023 |
| 16 | 夹具 ID 稳定性 | 夹具 id 写死在本文档 §3，Dev 按此造数（不写死**权限点个数**） | §3 全部 |

**本次 AC 修订带来的新口径（用例已对齐）**：

| # | 新口径 | 用例落点 |
|---|---|---|
| N1 | AC-S-033：错误信息每条都带**来源 manifest 的文件路径** | TC-009-001 |
| N2 | AC-S-034：**RLS 兜底可验** —— `base_app` 连接下 `SELECT count(*) FROM platform_role` 在租户 A 上下文只看到租户 A 的行；`pg_policies` 里三张表的 policy **表名逐张列出** | TC-009-004 |
| N3 | AC-S-035：权限集合 **逐个列出**编码，不只比数量 | TC-009-007 |
| N4 | AC-S-036：改用 `sandbox:*` / `sandbox:demo:*`，并要求**对照 `org:` 的权限点不在展开结果里** | TC-009-010 ~ 012 |
| N5 | AC-S-038：**每种 scope 的期望结果集逐条列出记录 id**，且五个结果集**两两不相等** | TC-009-016 ~ 020、§3.5 期望集合表 |
| N6 | AC-S-039：`use_all_memberships` 在**该角色**上开启 | TC-009-023 |
| N7 | AC-S-040：拆成**预防层**（再插一条主部门 → DB 拒绝）+ **防御层**（临时移除索引造歧义 → 判定仍 fail closed 到 SELF） | TC-009-027（预防）/ TC-009-026（防御）/ TC-009-025（缺失） |
| N8 | AC-S-041：X **同时绑定 R1 + R2 + R3**；第 3 步**解绑 R3** → 退为 `OWN_ORG`（**与第 1 步不同**） | TC-009-028（并集）/ TC-009-029（权限点级）/ **TC-009-031（解绑判别）** / TC-009-030（用户默认） |
| N9 | AC-S-042：注入串的**三条判据**（不出现在 SQL / 不全表命中 / 只命中字面量相等行） | TC-009-037 |
| N10 | AC-S-043：第 4 步「条件引用白名单外属性 → 判定不通过」 | TC-009-040 |
| N11 | AC-S-044：`detail` **可以**含权限点编码，**不得**含资源业务字段（含 `org_node_id`） | TC-009-042 / TC-009-045 |
| N12 | AC-S-045：只管**资源类**越权；动作类 403 是 C5 规定行为，**不算**存在性泄露 | TC-009-042 第 4 条预期 |
| N13 | AC-S-046：第 5 步「全仓搜索鉴权入口的**定义处**」 | TC-009-056 |
| N14 | AC-S-047：用**两个独立 checker 实例**（各自缓存、共享 DB）模拟两个进程 | TC-009-061 |
| N15 | AC-S-048：三步判据（结果已被过滤 / SQL 含平台追加的范围条件片段且值走参数绑定 / 不存在跳过开关） | TC-009-050 / 051 / 052 |
| N16 | AC-S-034 ★（BA 21:40 后追加）：**管理接口自身受保护（自举防护）** —— `/api/rbac/*` 写接口要求 `rbac:role:write`、**读接口也要求**管理权限点；普通用户一律 **403 `INSUFFICIENT_PERMISSION`** | **TC-009-005**（新增）；A 域其余用例的调用者默认额外绑定 `rbac-manager` |

---

## 8. 建议执行顺序

| 阶段 | 范围 | 说明 |
|---|---|---|
| **第一批**（Dev 自验 + CI 门禁，纯逻辑） | TC-009-033 ~ 036（DSL）、TC-009-053 ~ 056（C-5）、TC-009-007 ~ 008（继承与成环）、TC-009-010 ~ 011（通配） | 单元测试，不连库；**先在 `modules/sandbox` 的 manifest 上补 3 个权限点并接入 `require_permission`** |
| **第二批**（需真实库 `base_test`） | TC-009-004、006、013 ~ 015、016 ~ 032、038 ~ 040、050 ~ 052、058 ~ 060 | 需 `sandbox_scope_item` 表与 §3 的全部种子数据；应用连接用普通角色 `base_app`；TC-009-026 需临时 `DROP INDEX`（测完重建） |
| **第三批**（需起进程 / 观察 stderr / 真机 HTTP） | TC-009-001 ~ 003（启动对账）、TC-009-005（自举防护，真机 HTTP）、TC-009-041 ~ 049（中间件与错误体） | 启动对账需**能改库**（管理员连接）做差异注入；`/api/rbac/*` 走真机 `base serve`，sandbox 路由在模块内 `ServiceExt::oneshot` 驱动 |
| **第四批**（需两个 checker 实例） | TC-009-061（跨实例 epoch 收敛） | 两个独立 checker 实例共享同一个 `base_test`（本机可跑，无外部中间件依赖） |

**建议的 CI 门禁**：第一批全量 + 第二批中不依赖起进程的断言。第三批的启动对账用例（TC-009-001 ~ 003）因要改库注入差异，建议只在**夜间 / 手动**任务跑，不进 PR 门禁。

---

## 9. 验收标准修订记录

### 9.1 BA 已接受并改写的 12 条（QA 提问 → 新 AC 口径）

| # | QA 提的问题 | BA 接受了什么 | 改成了什么（新 AC 文本） | 用例落点 |
|---|---|---|---|---|
| 1 | AC-S-045 自相矛盾（前置说「有权访问」、预期说「都是 404」） | ✅ 接受，改写 AC | 前置改为：用户 P **拥有**权限点但 `item-a1` **在其数据范围之外**；对照 `item-not-exist-9999`（不存在）与 `item-a2`（范围内）。预期：**无权资源 vs 不存在资源**两次响应一致（404 / 结构 / 头）；范围内资源返回 200 | TC-009-046 ~ 048 |
| 2 | AC-S-040 与 design 冲突（两个主部门 vs 部分唯一索引） | ✅ 接受，改写 AC | 拆成**预防层 + 防御层**：① 再插一个 `is_primary=true` 行 → **被 DB 拒绝**；② **临时移除部分唯一索引**造出歧义 → 判定仍 **fail closed 到 SELF** | TC-009-025 ~ 027 |
| 3 | AC-S-041 步骤 1 隐含条件缺失 | ✅ 接受，改写 AC | 前置改为绑定 **R1 + R2 + R3 三个角色**；步骤 3 改为**解绑 R3**后再查，结果应退为 `OWN_ORG`（**与第 1 步不同**） | TC-009-028 / **031** |
| 4 | AC-S-045 与契约 C5 冲突（403 会被误当成泄露） | ✅ 接受，改写 AC | AC-S-045 只管**资源类**；明确「动作类越权 403 是契约 C5 规定的行为，**不算**存在性泄露」 | TC-009-042 第 4 条预期 |
| 5 | AC-S-042 第 4 条判据不成立 | ✅ 接受，改写 AC | 改成三条判据：① SQL 文本里**不出现**该字符串；② 查询**不出现**全表命中的效果；③ 只返回字面量值**恰好等于**该字符串的行 | TC-009-037 |
| 6 | `iam:` 权限点在注册表中不存在 | ✅ 接受 | 注册表当前只有 `sandbox:` 与 `org:`；AC-S-036 改用 `sandbox:*` / `sandbox:demo:*`，并要求**对照 `org:` 的权限点不在展开结果里** | TC-009-010 ~ 012 |
| 7 | AC-S-038 未定义「相同」判据 | ✅ 接受 | 改为**每种 scope 的期望结果集逐条列出记录 id**（表格形式），并要求五种结果集**两两不相等** | TC-009-016 ~ 020 |
| 8 | AC-S-044 未界定 `detail` 能否含权限点编码 | ✅ 接受 | 明确：`detail` **可以**含被拒的**权限点编码**；**不得**含被拒**资源**的业务细节（标题 / 金额 / 归属人 / `org_node_id`） | TC-009-045 |
| 9 | AC-S-047「跨进程成立」缺判定口径 | ✅ 接受 | 明确用**两个独立的 checker 实例**（各自持自己的缓存、共享同一个 DB）：实例 1 撤销 → **实例 2** 立刻调也必须被拒 | TC-009-061 |
| 10 | AC-S-048 缺「自动叠加」的区分手段 | ✅ 接受 | 三步判据：① 结果已被过滤；② **渲染出的 SQL 文本含平台追加的范围条件片段**且值走参数绑定；③ 不存在「跳过范围」的开关 | TC-009-050 ~ 052 |
| 11 | AC-S-033「来源 manifest 路径」指向不明 | ✅ 接受 | 明确：错误信息每条都带**来源 manifest 的文件路径**（形如 `modules/foo/manifest.json`） | TC-009-001 |
| 12 | AC-S-034 缺 RLS policy 校验判据 | ✅ 接受 | 明确：用 `base_app` 连接查 `platform_role` 在租户 A 上下文下**只能看到租户 A 的行**；且 `pg_policies` 里能查到那三张表的 policy（**表名逐张列出**） | TC-009-004 |
| 13（**补记**） | —（BA 主动追加，非 QA 提问） | ✅ 追加条款 | AC-S-034 新增 ★ 条：**管理接口自身受保护（自举防护）** —— 前置加「用户 M 是普通用户（只有 `platform-user`）、用户 A 持 `rbac:role:write`」；步骤 5「以 M 调 `/api/rbac/*` 写接口」；预期 **403 `INSUFFICIENT_PERMISSION`**，且**同样适用于读接口** | **TC-009-005**（新增） |

### 9.2 本轮复核**新发现**的问题（未自行改 AC，请 BA 复核）

| # | 涉及 | 问题 | 建议 |
|---|---|---|---|
| A | AC-S-043 | AC-S-043 的前置条件仍写 **`demo:invoice:approve`**，但裁定 #8 已决定**不新建 `demo` 模块** —— 该权限点在新夹具里不存在。本用例集改用 **`sandbox:scope-item:approve`**（同样携带 `$record.amount <= 10000`，语义等价） | 把 AC-S-043 里的 `demo:invoice:approve` 同步改为 `sandbox:scope-item:approve`（或把权限点编码写成占位符） |
| B | AC-S-038 | AC-S-038 要求「对**同一张表**分别以五种 scope 查询」，期望结果集表里 `SELF` 行写 `created_by = U`；但**同一个用户**同时持有 5 个 scope 角色会被「取最宽松」合并成 `ALL`，无法分别观察五种语义 | 补一句「五种 scope 可分别由**五个主部门相同、角色 scope 不同**的用户观测」（本用例集按 5 个探针执行，见 §3.5） |
| C | AC-S-045 | 前置里 `item-a1`（范围外）与 `item-a2`（范围内）**未给出与 P 的主部门的关系**。若 P 的主部门就是 `dept-a`，则落在 `dept-a` 的 `item-a1` 必然在 P 的 `OWN_ORG` 范围内，两条前置**互斥** | 补一句「P 的主部门为 `dept-a-1`」或明确两条记录与主部门的归属关系（本用例集用 `u-1012`，主部门 `dept-a-1`，见 §3.3） |
| D | AC-S-046 | 第 5 步「全仓搜索鉴权入口的**定义处**」—— 平台入口自身那**一处**（`crates/platform-core/src/extension/permission.rs`）是合法基线；若按字面「只有一个定义处」判定，会把平台自己判成违规 | 补一句「除平台唯一定义处外，不得有第二个定义处」（本用例集 TC-009-056 按此判定） |
| E | AC-S-034 | RLS 判据写「在**租户 A 的上下文下**」，但没写上下文怎么设定 —— 平台 RLS 用会话变量 `app.tenant_id`（`current_setting('app.tenant_id', true)`），缺这一条判据无法执行 | 补一句「租户上下文 = 会话变量 `app.tenant_id`」（本用例集 TC-009-004 按此判定，并同时校验不设变量时 `count(*) = 0`） |
| F | AC-S-048 | 第 3 步写「业务模块若绕过构造器直接写 SQL，则由 **`check-deps` / 约定**挡住并在评审中判失败」—— `check-deps` 是**依赖方向**门禁，它**不检查**「绕过 `TenantQuery` 直接写 SQL」，把两者并列容易让 Dev 误以为门禁能挡 | 把 `check-deps` 与「代码评审/约定」分开表述（或删掉 `check-deps`）—— 本用例集 TC-009-051 第 5 步如实记录事实并按门禁结论判定 |
| G | design §3.1 | 权限点通配的示例仍写 `order:*` 匹配 `order:` 下所有，但注册表里**没有 `order:` 前缀的权限点**（裁定 #8 后只有 `sandbox:` 与 `org:`） | 把 §3.1 的示例换成 `sandbox:*`（与 AC-S-036 一致），避免 Dev 按不存在的模块实现 |
| H | design §4.1 | 仍写「**数据范围配置里** `use_all_memberships = true`」，而 §4.3 已定为**角色级**（`platform_role.use_all_memberships`） | §4.1 的措辞改为「**角色级**开关 `use_all_memberships`」，与 §4.3 一致（AC-S-039 已写「在**该角色**上开启」，只有 design §4.1 措辞滞后） |
| I | AC-S-044 | 前置写「夹具里有 **3 条记录**」，但没指明是哪张表 | 明确为 `sandbox_tenant_item` 的 3 条（本用例集按此取值） |
| J | AC-S-034 ★ | 追加条款只给了 `rbac:role:write` 一个**举例**，**读接口**该要求哪个权限点编码**没写死**；且这两个平台级权限点的**来源 manifest**（`crates/platform-rbac/manifest.json`）**尚未存在** —— 不新建它，C-5 与 AC-S-033 的对账都过不了 | 明确读接口的权限点编码（建议 `rbac:role:read`），并在 design §8.1 的「仓库基建」里补 `crates/platform-rbac/manifest.json` 的新建项 —— 本用例集按 `rbac:role:read` / `rbac:role:write` 取值（见 §3.6） |
| K | AC-S-034 ★ | 追加条款说「这条同样适用于**读接口**（读角色列表也应要求管理权限点）」，但**没界定**哪些读接口算「管理接口」。若把 `GET /api/rbac/permissions`（权限点注册表）也算进去，前端就拿不到注册表 —— 与 REQ-009「本期只保证『权限点注册表是单一来源 + **有接口可读**』」及 REQ-011 的 `can()` 前置**冲突** | 明确豁免：权限点**注册表查询**（`GET /api/rbac/permissions`）应**只要求已认证**，不要求管理权限点；角色/授权/绑定的读写才要求 `rbac:role:*` |

---
