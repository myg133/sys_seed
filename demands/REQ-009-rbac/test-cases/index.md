# REQ-009「RBAC 权限与数据范围」—— 测试用例总索引

- **需求编号**: REQ-009「RBAC 权限与数据范围」
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: AC-001（权限点注册表驱动权限判定）、AC-002（数据范围隔离生效）
- **来源**: [acceptance.md](../acceptance.md)（AC-S-033 ~ AC-S-048）、[design-summary.md](../design-summary.md)、[api-conventions v1](../../../contracts/api-conventions/v1.md)、[extension-points v2](../../../contracts/extension-points/v2.md)
- **格式基准**: [REQ-001 用例总索引](../../REQ-001-platform-foundation/test-cases/index.md)（同构：概览 / 文件清单 / 夹具 / 命名约定 / AC→TC 矩阵 / 外部依赖 / 待确认约定 / 执行顺序）
- **状态**: 待实现（用例本身尚未执行，**不得据此判定需求通过**）

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **59**（TC-009-001 ~ TC-009-060，编号连续无跳号） |
| 覆盖 AC | **16 / 16**（AC-S-033 ~ AC-S-048，无遗漏） |
| 优先级分布 | P0 = 55、P1 = 4、P2 = 0 |
| 自动化分布 | 可自动化 = 57、半自动 = 2、手工 = 0 |
| 用例文件 | 4 个（另本索引 1 个） |
| 每条用例字段 | 标题 / 关联 AC / 前置条件 / 测试步骤 / 预期结果 / 类型 / 优先级 / 自动化 / 设计方法（**9 项全部齐备**，脚本校验 59/59 通过） |

> 每条 AC 在矩阵中至少映射 1 条 TC；**类型含「安全」的 13 条 AC**（AC-S-033、034、036、037、039、040、042、043、044、045、046、047、048）**全部 ≥ 3 条**，且每条都配有「正常路径 + 异常/攻击路径」。
> 覆盖条数最少的 AC：**AC-S-034**（2 条）、**AC-S-043**（3 条）、**AC-S-048**（3 条）—— 三条均含「正常 + 越权/跨租」两路，条数少是因为 AC 本身只描述一件事。

## 2. 文件清单

| 文件 | 能力域 | AC 范围 | 用例数 |
|---|---|---|---|
| [A-registry-role.md](A-registry-role.md) | 权限点注册表与角色模型 | AC-S-033 ~ AC-S-037 | 14 |
| [B-data-scope.md](B-data-scope.md) | 数据范围求值（5 种 scope / 兼岗 / fail closed / DSL / 属性级） | AC-S-038 ~ AC-S-043 | 24 |
| [C-authz-middleware.md](C-authz-middleware.md) | 鉴权中间件、错误体与 SQL 层过滤 | AC-S-044、AC-S-045、AC-S-048 | 12 |
| [D-contract-cache.md](D-contract-cache.md) | 契约检查 C-5 与缓存主动失效 | AC-S-046、AC-S-047 | 9 |

---

## 3. 测试夹具（造数方式）

> **原则**：组织侧夹具一律基于 **REQ-008 已真实落库的表**（`org_node` / `user_org`，迁移 `20261118000700_org_schema.up.sql`）—— 兼岗 = `user_org` 多行、`is_primary` 标主部门；**不另造影子表**。
> 业务侧夹具表（`sandbox_scope_item`）是 REQ-009 需要新增的（见 §7 待确认约定 #8）。
> 账号由 REQ-007 的 IAM 体系提供（`platform_iam_user.id` 是 `TEXT`），统一初始密码 `Passw0rd!`。

### 3.1 租户

| ID | tenant_id | 用途 |
|---|---|---|
| T1 | `tenant-a` | 主测试租户（全部数据范围 / 角色用例都在这里） |
| T2 | `tenant-b` | 跨租对照租户（存在性泄露、跨租 id、租户头不一致） |

### 3.2 组织树（T1，基于真实表 `org_node`）

```text
dept-hq                  总部          company  parent_id = NULL
├── dept-a          部门 A        dept     parent_id = dept-hq
│   └── dept-a-1    部门 A 子节点   dept     parent_id = dept-a
├── dept-b          部门 B        dept     parent_id = dept-hq
│   └── dept-b-1    部门 B 子节点   dept     parent_id = dept-b
└── dept-c          部门 C        dept     parent_id = dept-hq
```

- `path_labels` 是加速字段（根到本节点，含本节点），如 `dept-a-1` = `["总部","部门 A"]`；**权威来源是 `parent_id`**。
- T2 另有：`dept-t2-hq`（company）、`dept-t2-a`、`dept-t2-b`（形状与 T1 镜像，用于「兼岗默认行为不随租户变化」的对照）。
- `dept-a` 是**子树递归**的验证点（`dept-a-1` 是子节点、`dept-b-1` 是**别的分支**，不得被包含）。

### 3.3 用户、成员关系与角色（真实表 `user_org` + `platform_user_role`）

| user_id | 账号 | `user_org` 行（`org_node_id` / `is_primary`） | 绑定角色（`data_scope`） | 用途 |
|---|---|---|---|---|
| `u-1001` | owner@example.com | `dept-a` / **TRUE** | `rbac-owner`（OWN_ORG） | OWN_ORG 基线、撤销即时生效、主部门变更 |
| `u-1002` | parttime@example.com | `dept-a` / **TRUE**；`dept-b` / FALSE | `rbac-owner`（OWN_ORG）、`rbac-owner-below`、`rbac-union` | **兼岗用户**（AC-S-039） |
| `u-1003` | noprimary@example.com | `dept-a` / FALSE（**无任何主部门**） | `rbac-owner`（OWN_ORG） | **主部门缺失**（AC-S-040） |
| `u-1004` | dualprimary@example.com | `dept-a` / **TRUE**；`dept-c` / **TRUE** | `rbac-owner`（OWN_ORG） | **主部门歧义**（AC-S-040） |
| `u-1005` | multirole@example.com | `dept-a` / TRUE | `rbac-multi-a` + `rbac-multi-b` + `rbac-multi-c` | 多角色并集 / 权限点级覆盖（AC-S-041） |
| `u-1006` | admin@example.com | `dept-hq` / TRUE | `platform-admin`（ALL，内置） | 内置角色不可删降权（AC-S-037） |
| `u-1007` | noperm@example.com | `dept-a` / TRUE | `rbac-self`（SELF，**不含** `sandbox:scope-item:approve`） | 缺权限点 → 403（AC-S-043/044） |
| `u-1008` | other@example.com | `dept-a` / TRUE | `rbac-owner-below`（OWN_ORG_AND_BELOW） | 子树递归（AC-S-038） |
| `u-1009` | custom@example.com | `dept-a` / TRUE | `rbac-custom`（CUSTOM） | CUSTOM scope（AC-S-038/042） |
| `u-1010` | allscope@example.com | `dept-a` / TRUE | `rbac-all`（ALL） | ALL scope（AC-S-038） |
| `u-1011` | norole@example.com | `dept-a` / TRUE | **无角色** | 用户默认 `SELF`（AC-S-041） |
| `u-2001` | t2user@example.com（T2） | `dept-t2-hq` / TRUE | `rbac-self` | 跨租对照 |

- `user_org.id` 取 `uo-1001` 起（`TEXT` 主键）；`role` 列（岗位性角色，如「部门经理」）一律填 `NULL` —— 它**不是**平台的权限角色。
- **兼岗三种形态各自独立成夹具**：`u-1002`（兼岗，主+非主）、`u-1003`（无主部门）、`u-1004`（两条主部门）。
- `u-1004` 的歧义状态与 REQ-009 新增的部分唯一索引互斥 —— 见 §7 待确认约定 #4。

### 3.4 角色（`platform_role` + `platform_role_permission`）

| 角色 code | `data_scope` | `builtin` | 权限点 | 用途 |
|---|---|---|---|---|
| `platform-admin` | ALL | **true** | 注册表**全部 17 个** | 内置角色保护（AC-S-037） |
| `platform-user` | SELF | **true** | 无 | 内置角色保护（AC-S-037） |
| `rbac-owner` | OWN_ORG | false | `sandbox:scope-item:read`、`sandbox:scope-item:approve` | OWN_ORG 基线 |
| `rbac-owner-below` | OWN_ORG_AND_BELOW | false | `sandbox:scope-item:read` | 递归子树 |
| `rbac-self` | SELF | false | `sandbox:scope-item:read` | SELF |
| `rbac-all` | ALL | false | `sandbox:scope-item:read` | ALL |
| `rbac-custom` | CUSTOM | false | `sandbox:scope-item:read` | CUSTOM 条件 `$record.amount <= 10000` |
| `rbac-union` | OWN_ORG + `use_all_memberships=true` | false | `sandbox:scope-item:read` | 兼岗并集（AC-S-039） |
| `rbac-multi-a` | OWN_ORG | false | `sandbox:scope-item:read`、`sandbox:scope-item:export` | 多角色（AC-S-041） |
| `rbac-multi-b` | OWN_ORG | false | `sandbox:scope-item:read`、`sandbox:scope-item:export`（**权限点级 scope = SELF**） | 权限点级覆盖 |
| `rbac-multi-c` | OWN_ORG_AND_BELOW | false | `sandbox:scope-item:read` | 最宽松 scope |
| `rbac-wildcard` | OWN_ORG | false | `sandbox:*` / `sandbox:demo:*`（分两条用例写入） | 通配语义（AC-S-036） |
| `rbac-inh-a` / `rbac-inh-b` / `rbac-inh-c` | OWN_ORG | false | 各自 1 个不同权限点 | 继承展开（AC-S-035） |
| `rbac-cycle-a` / `rbac-cycle-b` / `rbac-self-inherit` / `rbac-tri-a` / `rbac-tri-b` / `rbac-tri-c` | OWN_ORG | false | 无 | 成环检测（AC-S-035） |
| `rbac-custom-a` / `rbac-custom-b` | CUSTOM | false | `sandbox:scope-item:read` | 多 CUSTOM 取交集（AC-S-041） |
| `rbac-tmp` / `rbac-t2-only` | OWN_ORG / SELF | false | — | 角色 CRUD 与跨租（AC-S-034） |

### 3.5 业务数据（`sandbox_scope_item`，T1 共 **13** 条）

| id | title | amount | org_node_id | created_by |
|---|---|---|---|---|
| `item-a1` | A 部门直属单 | 5000 | `dept-a` | `u-1001` |
| `item-a2` | A 子节点单 | 5000 | `dept-a-1` | `u-1002` |
| `item-b1` | B 部门直属单 | 5000 | `dept-b` | `u-1002` |
| `item-b2` | B 子节点单 | 5000 | `dept-b-1` | `u-1008` |
| `item-c1` | C 部门单 | 5000 | `dept-c` | `u-1008` |
| `item-s1` | u-1007 自建单 | 5000 | `dept-c` | `u-1007` |
| `item-s2` | u-1007 自建单 2 | 5000 | `dept-b` | `u-1007` |
| `item-s3` | u-1007 自建单 3 | 5000 | `dept-c` | `u-1007` |
| `item-big` | 大额单 | **20000** | `dept-a` | `u-1007` |
| `item-v1` | u-1003 自建单 | 5000 | `dept-c` | `u-1003` |
| `item-w1` | u-1004 自建单 | 5000 | `dept-c` | `u-1004` |
| `item-m1` | u-1005 自建单 | 5000 | `dept-a` | `u-1005` |
| `item-n1` | u-1011 自建单 | 5000 | `dept-a` | `u-1011` |

T2 另有 `item-t2-1`（`tenant-b`、`dept-t2-hq`、`u-2001`）。

> 另有两个**刻意构造**的 id：`item-not-exist-9999`（**永不落库**，用于 AC-S-045 的「存在 vs 不存在」存在性泄露对照）；`item-a3`（TC-009-017 中**临时插入**的孙节点记录，`org_node_id = dept-a-1`，用于验证递归深度 ≥ 2）。

**五种 scope 的期望集合**（探针用户主部门均为 `dept-a`，用于 TC-009-019 的「互不相同」自检）：

| scope | 探针 | 期望集合 | 条数 |
|---|---|---|---|
| ALL | `u-1010` | 全部 13 条 | 13 |
| CUSTOM（`amount <= 10000`） | `u-1009` | 除 `item-big` 外 12 条 | 12 |
| SELF | `u-1007` | `item-s1` / `item-s2` / `item-s3` / `item-big` | 4 |
| OWN_ORG_AND_BELOW | `u-1008` | `item-a1` / `item-a2` / `item-big` | 3 |
| OWN_ORG | `u-1001` | `item-a1` / `item-big` | 2 |

> 五个集合**两两不相等**、条数也互不相同 —— 满足 AC-S-038「若某两种在夹具下结果相同，视为夹具无效」的判据。
> `item-v1` / `item-w1` 的存在是为了让「主部门缺失 / 歧义 → 退化为 SELF」**不是空集**（否则「看到空」会蒙混过关，见 TC-009-024 / TC-009-025）。

### 3.6 权限点注册表（`platform_permission`，共 **17** 个）

| 权限点 code | 来源 manifest | 说明 |
|---|---|---|
| `sandbox:scope-item:read` | `modules/sandbox/manifest.json` | **需新增**的夹具模块 |
| `sandbox:scope-item:approve` | `modules/sandbox/manifest.json` | 携带 `condition: "$record.amount <= 10000"`（AC-S-043 原文的权限点） |
| `sandbox:scope-item:export` | `modules/sandbox/manifest.json` | AC-S-041 的权限点级 scope 载体 |
| `sandbox:demo:read` | `modules/sandbox/manifest.json` | **需新增**的夹具模块（AC-S-036 原文的权限点） |
| `sandbox:demo:write` | `modules/sandbox/manifest.json` | 同上 |
| `sandbox:scope-item:read` | `modules/sandbox/manifest.json` | 同上（跨 resource，验证 `sandbox:demo:*` 不越界） |
| `sandbox:demo:read` / `sandbox:demo:write` | `modules/sandbox/manifest.json` | **仓库现状唯一业务模块**的真实权限点 |
| `org:org-node:read`、`org:org-node:write`、`org:user-org:read`、`org:user-org:write`、`org:user-position:read`、`org:user-position:write`、`org:sync-conflict:read`、`org:sync-conflict:resolve`、`org:scim-sync:run` | `crates/platform-org/manifest.json` | 9 个平台 crate 权限点（真实存在） |

- 注册表与 `capabilities/permissions.json`（`cargo xtask capabilities` 生成）必须**逐条相等** —— 这正是 AC-S-033 的对账对象。
- ⚠️ AC-S-036 提到的 `iam:` 权限点**当前不存在**（`crates/platform-iam/` 没有 `manifest.json`），见 §6.3。

### 3.7 平台表与接口

| 表 | 租户维度 | 说明 |
|---|---|---|
| `platform_permission` | **无**（平台级） | 权限点登记（含 `condition`，见 §7 #7） |
| `platform_role` / `platform_role_permission` / `platform_role_inherit` / `platform_user_role` | **有** | 必须有 `tenant_id` + RLS policy |
| `platform_permission_epoch` | 有（主键 `tenant_id`） | `epoch` / `updated_at`，写操作时 +1 |

- 业务数据表 `sandbox_scope_item`：`tenant_id` / `id` / `title` / `amount` / `org_node_id` / `created_by`。
- 夹具模块 `modules/sandbox` 的路由（契约 C2 `/api/<module>/<resource>/<action>`）：`GET /api/sandbox/scope-item`、`GET /api/sandbox/scope-item/{id}`、`POST /api/sandbox/scope-item/approve`、`POST /api/sandbox/scope-item/export`。

---

## 4. 命名与接口约定（REQ-009 的实际取值）

| 约定项 | 取值 | 来源 |
|---|---|---|
| 权限点编码 | `module:resource:action`，全小写连字符；允许通配 `module:*` / `module:resource:*`，**禁止** `*:*` 与 `*:resource:action` | 契约 C4 + design §3.1 |
| 平台 API 前缀 | `/api/rbac`（角色、授权、绑定、权限点注册表查询） | 契约 C1 + design §8 |
| 平台 API 子路径 | `/api/rbac/roles`、`/api/rbac/roles/{id}`、`/api/rbac/roles/{id}/permissions`、`/api/rbac/roles/{id}/inherit`、`/api/rbac/users/{user_id}/roles`、`/api/rbac/permissions` | design §8 + `platform-org` 同构风格（**待确认**，见 §7 #1） |
| 业务 API 路径 | `/api/<module>/<resource>/<action>`，动作放**末段** | 契约 C2 |
| 鉴权入口 | `require_permission("<code>")` —— 平台**唯一**入口，参数必须字面量 | design §6.3 + extension-points v2 §4 |
| 越权响应码 | 资源类 **404**（`NOT_FOUND`）／动作类 **403**（`INSUFFICIENT_PERMISSION`）／未认证 **401**（`UNAUTHENTICATED`） | 契约 C5 + design §6.2 |
| 错误体 | RFC 9457 `application/problem+json`：`type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id` / `params`；`detail` **必须英文 ASCII**；`trace_id` 必带 | 契约 C6 + `platform-iam/src/http.rs`（`ProblemDetails`） |
| 租户 / 空间上下文头 | `X-Tenant-Id` / `X-Space-Id`；请求头租户与令牌租户不一致 → **404** | 契约 C3 + `platform-iam/src/http.rs` |
| 数据范围取值 | `ALL` / `OWN_ORG` / `OWN_ORG_AND_BELOW` / `SELF` / `CUSTOM` | design §4 |
| 主部门判定 | `user_org.is_primary = TRUE`（真实列名） | REQ-008 迁移 |
| 租户查询构造器 | `TenantQuery`：语句里**必须**有且仅有一个 `{tenant}`（渲染为 `$1`），业务参数用 `?`（渲染为 `$2`…）；`BindValue` 目前**无数组变体**（`OWN_ORG_AND_BELOW` 的 `= ANY($n)` 需新增 `TextArray`） | `platform-db/src/query.rs` + design §8.1 |
| 契约检查 | C-1 ~ C-5 同一套件；C-5 新增于 extension-points **v2** | `platform-contracts/src/checks.rs` + v2 §4 |
| 隔离级别 | L1 共享表 + `tenant_id` + PostgreSQL RLS（默认） | ADR-0001 D2 |
| 跑法 | `cargo test --workspace -- --test-threads=2`（`cargo test` **只能有一个 `--`**）；真实库层 `DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test`，**应用连接必须用普通角色 `base_app`**（超管会被 `require_isolation` 拒绝启动） | `platform-org/tests/org_real_db.rs` + `platform-server/src/isolation.rs` |

---

## 5. AC → TC 覆盖矩阵（16 / 16）

| AC | 标题 | 覆盖 TC | 条数 | 外部依赖 | 最高优先级 |
|---|---|---|---|---|---|
| AC-S-033 | 权限点注册表双向对账且不一致即启动失败 | TC-009-001, TC-009-002, TC-009-003 | 3 | 真实库 `base_test` + **夹具模块 `modules/sandbox`** | P0 |
| AC-S-034 | 角色、授权与绑定可用且带租户维度 | TC-009-004, TC-009-005 | 2 | 真实库 `base_test` | P0 |
| AC-S-035 | 角色继承展开且成环被拒 | TC-009-006, TC-009-007, TC-009-008 | 3 | 无 | P0 |
| AC-S-036 | 权限点通配语义正确且禁止全通过 | TC-009-009, TC-009-010, TC-009-011 | 3 | **夹具模块 `modules/sandbox`** | P0 |
| AC-S-037 | 内置角色不可删除也不可降权 | TC-009-012, TC-009-013, TC-009-014 | 3 | 真实库 `base_test` | P0 |
| AC-S-038 | 数据范围五种语义求值正确 | TC-009-015, TC-009-016, TC-009-017, TC-009-018, TC-009-019 | 5 | 真实库 `base_test` + **表 `sandbox_scope_item`** | P0 |
| AC-S-039 | 兼岗默认按主部门，取并集必须显式开启 | TC-009-020, TC-009-021, TC-009-022, TC-009-023 | 4 | 真实库 `base_test` | P0 |
| AC-S-040 | 主部门缺失或歧义时 fail closed 到 SELF | TC-009-024, TC-009-025, TC-009-026 | 3 | 真实库 `base_test` + **夹具时序**（见 §6.2） | P0 |
| AC-S-041 | 数据范围优先级与多角色组合 | TC-009-027, TC-009-028, TC-009-029, TC-009-031 | 4 | 真实库 `base_test` | P0 |
| AC-S-042 | 受限表达式 DSL 拒绝越界写法且不产生拼接 SQL | TC-009-032, TC-009-033, TC-009-034, TC-009-035, TC-009-036 | 5 | 单元测试 + 真实库（注入串那条） | P0 |
| AC-S-043 | 属性级校验（RBAC + ABAC 混合）生效 | TC-009-037, TC-009-038, TC-009-039 | 3 | 真机 HTTP + 真实库 | P0 |
| AC-S-044 | 鉴权中间件的三种拒绝语义与错误体形状 | TC-009-040, TC-009-041, TC-009-042, TC-009-043, TC-009-044 | 5 | 真机 HTTP | P0 |
| AC-S-045 | 越权拦截不依赖前端，且 404 不泄露资源存在性 | TC-009-045, TC-009-046, TC-009-047, TC-009-048 | 4 | 真机 HTTP | P0 |
| AC-S-046 | 已登记权限点必须有鉴权调用点，且鉴权入口唯一 | TC-009-052, TC-009-053, TC-009-054, TC-009-055, TC-009-056 | 5 | **夹具模块 `modules/sandbox`**（第 55 条另需 `modules/sandbox` 接上鉴权） | P0 |
| AC-S-047 | 权限撤销在下一个请求即生效，不等 TTL | TC-009-057, TC-009-058, TC-009-059, TC-009-060 | 4 | 真实库 + 真机 HTTP（第 59 条需**两个进程实例**） | P0 |
| AC-S-048 | 数据范围过滤在 SQL 层生效且业务模块无法绕过 | TC-009-049, TC-009-050, TC-009-051 | 3 | 真实库 `base_test` + 真机 HTTP | P0 |

**合计 16 条 AC、59 条 TC，无遗漏 AC、无悬空 TC。**

---

## 6. 当前无法立即执行的 AC

> 前提：**所有**用例都要等 REQ-009 的实现落地后才可能执行。下表标的是「除平台实现之外，还需要哪些前置依赖」。

### 6.1 需要 Dev 先补夹具的（**不补就只能写脚本、跑不出判定**）

| AC | 缺失的夹具 | 影响 TC | 备注 |
|---|---|---|---|
| AC-S-033 / AC-S-046 | 模块 `modules/sandbox`（权限点 `sandbox:scope-item:read` / `sandbox:scope-item:approve` / `sandbox:scope-item:export` + 四条路由） | TC-009-001, 003, 036~038, 051~055 | AC 原文用了 `sandbox:scope-item:approve`，但仓库现状**没有** `demo` 模块 |
| AC-S-036 | 模块 `modules/sandbox`（权限点 `sandbox:demo:read` / `sandbox:demo:write` / `sandbox:scope-item:read`） | TC-009-009, 010 | AC 原文用了 `order:` 权限点，仓库现状**没有** `order` 模块 |
| AC-S-038 / 039 / 040 / 041 / 048 | 表 `sandbox_scope_item`（`org_node_id` / `created_by` / `amount`） | TC-009-015 ~ 030, 048~050 | 现有 `sandbox_tenant_item` 只有 `tenant_id` / `id` / `title` / `revision`，**没有** `org_node_id` 与 `created_by`，无法承载 `OWN_ORG` / `SELF` 语义 |
| AC-S-046 | `modules/sandbox` 接入真实 `require_permission(...)` 调用点 | TC-009-056 | 现状 `backend/src/permission.rs` 只定义常量、从不鉴权 —— v2 下 C-5 必然失败，这是**预期效果**（design §10 R8） |

### 6.2 需要夹具时序配合的

| AC | 问题 | 影响 TC | 处理建议 |
|---|---|---|---|
| AC-S-040 | REQ-009 新增的部分唯一索引 `user_org_one_primary_idx ON user_org (tenant_id, user_id) WHERE is_primary` 会让「用户 W 有两条 `is_primary = TRUE`」的夹具**在迁移后无法存在** | TC-009-025 | 歧义夹具由**迁移前**的种子数据造出；或测试内临时 `DROP INDEX` 造出歧义、测完重建（仅限 `base_test`）。此矛盾已列入 §8 的 AC 问题清单 |

### 6.3 AC 引用了「当前不存在」的对象

| AC | 引用 | 现状 | 影响 TC | 本用例集的处置 |
|---|---|---|---|---|
| AC-S-036 | `iam:` 权限点 | `crates/platform-iam/` **没有** `manifest.json`，注册表里不存在任何 `iam:` 权限点 | TC-009-009, 010 | 改用**真实存在**的 `sandbox:` / `org:` 权限点做「跨模块不通配」的对照（`order:` 三点仍按 AC 原文保留） |
| AC-S-038 | 「同一张表」 | AC 未指名具体表 | TC-009-015 ~ 019 | 按 §3.5 的 `sandbox_scope_item` |
| AC-S-043 / AC-S-044 | `sandbox:scope-item:approve` 所属模块 | 未指名 | TC-009-037 ~ 043 | 按夹具模块 `modules/sandbox`（`/api/sandbox/scope-item/approve`） |

### 6.4 无外部依赖、平台实现落地即可跑的部分

AC-S-035、AC-S-042、AC-S-046（负向部分）共 **19 条 TC** 可纯单元测试跑；其余 **40 条** 需要真实库 `base_test`（应用连接用普通角色 `base_app`），其中 **18 条**还需要起真机 HTTP 服务（`base serve`），**1 条**（TC-009-060）需要同时起两个实例连同一个 `base_test`。

---

## 7. 假设与待 BA/Dev 确认的约定

> 以下条目在既有文档中**没有**写死，本用例集按最合理方式取值并在此登记；**不当作既定事实**。如与实现冲突，以 BA 裁定为准，用例相应调整。

1. **`/api/rbac/*` 的子路径**：design-summary §8 只写「角色 CRUD、授权、绑定、权限点注册表查询」。本用例集按 `platform-org` 的同构风格推导为 `/api/rbac/roles`、`/api/rbac/roles/{id}/permissions`、`/api/rbac/roles/{id}/inherit`、`/api/rbac/users/{user_id}/roles`、`/api/rbac/permissions`（见 §4）。**路径一变，全部 A 域与 C 域用例的 URL 需同步替换。**
2. **平台表名与列名**：`platform_role` / `platform_role_permission` / `platform_role_inherit` / `platform_user_role` / `platform_permission` / `platform_permission_epoch` 按 design §2/§3/§7.1；列名按 §3 的描述推导（`tenant_id` / `id` / `code` / `name` / `data_scope` / `builtin` / `role_id` / `parent_role_id` / `user_id` / `permission_code` / `epoch`）。
3. **`use_all_memberships` 的归属与存储位置**：design §4.1 只说「数据范围配置里」。本用例集按**角色级**选项取值（`platform_role.use_all_memberships BOOLEAN NOT NULL DEFAULT FALSE`），因为 §4.3 把数据范围定义在角色级/权限点级。备选：租户级 / 用户级 —— 若改判，TC-009-022、TC-009-023 的操作步骤需换。
4. **歧义用户 W 与部分唯一索引互斥**（见 §6.2）：AC 同时要求「W 有两条主部门」与「新增第二个主部门被数据库拒绝」。本用例集按「歧义状态由迁移前的存量数据造成」取值。
5. **权限点级 scope 的存储位置**：design §3 的 `platform_role_permission` 只有 `tenant_id` / `role_id` / `permission_code`，**没有** scope 列，但 AC-S-041 要求「权限点级覆盖角色级」。本用例集按 `platform_role_permission.data_scope`（可空，空 = 继承角色级）取值。
6. **`CUSTOM` 条件表达式的存储位置**：design 未指明。本用例集按 `platform_role.condition`（仅 `data_scope = CUSTOM` 时有效）取值。备选：`platform_role_permission.condition` 或平台配置表。
7. **`platform_permission` 的条件列**：design §2 只说「权限点编码来自代码」。本用例集按 `platform_permission(code TEXT PRIMARY KEY, name TEXT, type TEXT, condition TEXT NULL)` 取值（`condition` 承接 manifest 的可选字段，extension-points v2 §5）。
8. **数据范围夹具表 `sandbox_scope_item`**：见 §3.5。备选方案是给现有的 `sandbox_tenant_item` 增加 `org_node_id` / `created_by` / `amount` 三列（改动面更小，但会把 REQ-006 的迁移与 REQ-009 耦合）。
9. **夹具模块 id 与路径**：`modules/sandbox`（`/api/sandbox/scope-item*`）与 `modules/sandbox`（`/api/order/*`）。AC 只给了权限点编码，没给模块名。
10. **AC-S-045 的「两次响应状态码相同」**：AC 前置条件写「R2 用户**有权**访问」，但第 3 步预期写「两次响应的状态码相同（都是 404）」—— 二者不能同时成立。本用例集按「**无权资源 vs 不存在资源**的响应完全一致；**有权资源**返回 200」解读（TC-009-045/045/046/047），并已列入 §8 的 AC 问题清单。
11. **`platform-admin` 的「全部权限点」实现口径**：design §3.3 写「全部权限点（通配 `*`）」，但 §3.1 禁止 `*:*`。本用例集按「**展开为注册表全部 17 个权限点**」判定（TC-009-013 逐个比对）。
12. **`INSUFFICIENT_PERMISSION` 错误码的归属**：design §6.2 已定案取契约 C6 的示例值 `INSUFFICIENT_PERMISSION`；但 `platform-iam/src/error.rs` 现有码表里**只有** `UNAUTHENTICATED` / `NOT_FOUND` / `CONFLICT` 等，**没有** `INSUFFICIENT_PERMISSION`。本用例集按「该码在 `platform-rbac` 侧新增、**不改** `platform-iam` 既有码」取值。
13. **内置角色的租户维度**：design §2 写 `platform_role` **有** `tenant_id`，因此本用例集按「**每个租户各有一份** `platform-admin` / `platform-user`」取值。若实现为平台级单份（`platform_role` 不带 `tenant_id`），TC-009-012 ~ TC-009-014 的前置条件需改。
14. **数据范围的「生效口径」**：本用例集按「**同一张表 + 不同 scope 角色 + 不同探针用户**」判定可见集合；若实现另提供「求值结果只读接口」，两者必须一致（不一致即判失败）。
15. **C-5 负向用例的执行形态**：AC 写「跑该模块的契约测试」。本用例集按「在 `crates/platform-contracts` 的测试里用**临时目录**构造违规模块（`manifest.json` + `backend/*.rs`）再跑 C-5 检查」取值 —— 等价于对该模块跑契约测试，但**不改仓库源码**、不污染 CI。
16. **`use_all_memberships` 与数据范围的交互范围**：本用例集按「开关只影响「本部门 / 本部门及下级」是否取所有成员节点并集，**不影响** `SELF` / `ALL` / `CUSTOM`」取值（TC-009-022 第 3 步据此断言）。

---

## 8. 建议执行顺序

| 阶段 | 范围 | 说明 |
|---|---|---|
| **第一批**（Dev 自验 + CI 门禁，纯逻辑） | TC-009-032 ~ 035（DSL）、TC-009-052 ~ 055（C-5）、TC-009-006 ~ 008（继承与成环的纯逻辑部分） | 单元测试，不连库；**先补 `modules/sandbox`**（C-5 的第 55 条另需 `modules/sandbox` 接上鉴权） |
| **第二批**（需真实库 `base_test`） | TC-009-004 ~ 005、012 ~ 014、015 ~ 030、036 ~ 038、048 ~ 050、056 ~ 058 | 需 `sandbox_scope_item` 表与 §3 的全部种子数据；应用连接用普通角色 `base_app` |
| **第三批**（需真机 HTTP / 起进程） | TC-009-001 ~ 003（启动对账）、039 ~ 047（中间件与错误体） | 需起 `base serve`、需要**能改库**（管理员连接）做对账差异注入 |
| **第四批**（需多实例） | TC-009-060（跨进程 epoch 收敛） | 需同时起两个实例连同一个 `base_test`（本机可跑，无外部中间件依赖） |

**建议的 CI 门禁**：第一批全量 + 第二批中不依赖真机 HTTP 的部分。第三批的启动对账用例（TC-009-001 ~ 003）因要改库注入差异，建议只在**夜间 / 手动**任务跑，不进 PR 门禁。

---

## 9. 验收标准本身的问题（提请 BA 复核）

> 以下是 QA 在编写用例时**实际卡住**的地方 —— 不是实现问题，是 AC 文本的歧义 / 缺判据 / 自相矛盾。

| # | AC | 问题 | 建议 |
|---|---|---|---|
| 1 | **AC-S-045** | 前置条件写「R2 用户**有权**访问」、第 3 步预期却写「两次响应的状态码相同（都是 404）」—— **不能同时成立**（有权访问应为 200）。另「不得出现『无权时返回 403、有权时返回 200』这类把存在性泄露出去的差异」这句话本身把「有无操作权限」与「资源是否存在」混在了一起 | 把第 3 步的对照对象改为「**不存在的**同类型记录」，并把预期改写为「无权资源 vs 不存在资源：状态码、`type`、`code`、响应体字段集合、响应头集合一致」 |
| 2 | **AC-S-040** | 一条 AC 同时要求「用户 W **有两条** `is_primary = TRUE` 的行」与「新增第二个主部门行 → 数据库拒绝」；而 design §4.2 新增的部分唯一索引会让前者**在迁移后不可达** | 明确「歧义分支**只覆盖存量数据**」，并把该 AC 拆成两条：①存量歧义数据 → fail closed ②新写入 → 唯一索引拒绝 |
| 3 | **AC-S-041** | 前置条件说「用户 X **同时有两个**角色 —— R1、R2」，「另有角色 R3」；但步骤 1 的预期「取最宽松的（**R3** 的 `OWN_ORG_AND_BELOW`）」隐含 R3 也绑在 X 上，而步骤 3 又说「去掉 R1、**只留** R3」—— 若 X 初始只绑 R1+R2，步骤 1 不可能取到 R3 的 scope | 明确步骤 1 时 X 的绑定集合；并把步骤 3 改成能产生**可区分**结果的一步（如「解绑 R3 只留 R1+R2」→ 应退化为 `OWN_ORG`） |
| 4 | **AC-S-045** | 「不得出现『无权时返回 403、有权时返回 200』」与契约 C5「资源类越权 404 / 动作类越权 403」在**动作类**场景下直接冲突：动作类无权限时按 C5 就是 403 | 限定该句只适用于**资源类**越权，或明确「403 只在动作类出现且不泄露资源存在性」 |
| 5 | **AC-S-038** | 「五种结果**互不相同**（若某两种在夹具下结果相同，视为夹具无效，测试必须换夹具）」—— **没有规定**哪些结果「相同」算相同（集合相同还是条数相同），也没给最小夹具要求 | 补一句「集合相同即视为相同」；本用例集按**集合相同**判定，并在 §3.5 给出 5 个两两互异的期望集合 |
| 6 | **AC-S-036** | 前置条件引用 `iam:` 权限点，但 `crates/platform-iam/` 没有 `manifest.json`，**注册表里不存在任何 `iam:` 权限点**；「不匹配 `iam:` / `org:` 的」这一判据当前**无法验证** | 换成仓库里真实存在的模块前缀（`sandbox:` / `org:`），或补 `platform-iam/manifest.json` |
| 7 | **AC-S-042** | 第 4 条预期「解析成功但注入串被当作普通字符串值绑定，查询结果与**注入前**一致」—— 「注入前」指的是什么条件？若原值是 `"a"`，注入串 `"a' OR '1'='1"` 与 `"a"` 的结果**必然不同**（除非注入串恰好等于原值） | 改成「注入串被绑定为**一个字面值**，命中集合 = 标题**恰好等于该串**的记录；且**不出现** `title` 等于 `'a'` 的记录」—— 即「注入前后结果不同，且都不等于全集」 |
| 8 | **AC-S-044** | 「`detail` 里**不得**出现被拒资源的任何细节」与契约 C6 示例（`detail` = 「当前角色缺少 `order:invoice:approve`」，含权限点编码）之间需要划界：权限点编码算不算「资源细节」？ | 明确「允许出现权限点编码，**不得**出现资源标识（id / 标题 / 金额 / 归属人）」 |
| 9 | **AC-S-047** | 「该结论**跨进程**成立」未给判定口径；单实例部署下无法证伪 | 明确判定方式：两个实例连同一库，A 实例撤销 → B 实例**不重启**的下一个请求即被拒，且间隔 < 2s |
| 10 | **AC-S-048** | 「平台**自动叠加**数据范围条件」与「业务模块**故意不写**任何数据范围条件」这两条如何同时验证「自动叠加」而不是「业务模块碰巧写了」？AC 未给出区分手段 | 补判据：抓 SQL 文本，数据范围条件**不出现在业务模块源码**中；且不限范围的等价 SQL 命中行数与接口返回行数**不同** |
| 11 | **AC-S-033** | 「逐条列出缺失的权限点编码与来源 manifest 路径」—— 缺失的权限点**没有** manifest（路径来自库侧），来源路径指的是「manifest 里有、库里没有」的那一侧还是「权限点本应来自哪个 manifest」？ | 补一句「来源 manifest 路径 = 该权限点在代码侧被声明的 manifest 文件路径」 |
| 12 | **AC-S-034** | 「角色、授权、绑定三张表都有 `tenant_id` 并生成 RLS policy」—— 校验 RLS policy 的**判据**没写（只要有 policy 就算过？还是要验证 policy 真的按 `tenant_id` 限定） | 补判据：policy 的 `USING` 必须含 `tenant_id = current_setting(...)` 之类的租户限定，且能实际拦下跨租读写 |

---
