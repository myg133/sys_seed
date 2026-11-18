# B. 数据范围求值 —— 测试用例（AC-S-038 ~ AC-S-043）

- **需求**: REQ-009「RBAC 权限与数据范围」
- **来源**: [acceptance.md](../acceptance.md) §AC-S-038 ~ AC-S-043、[design-summary.md](../design-summary.md) §4 ~ §5
- **用例数**: 24（TC-009-015 ~ TC-009-038）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## B.0 本域约定

- **五种 scope**：`ALL` / `OWN_ORG` / `OWN_ORG_AND_BELOW` / `SELF` / `CUSTOM`（design-summary §4）。
- **判定口径**：**以「可见数据集合」为准** —— 数据范围最终必须体现为业务列表/动作接口返回的记录集合。平台若另提供「求值结果只读接口」，其返回必须与集合判定一致（不一致即判失败）。
- **业务查询入口**（契约 C2 `/api/<module>/<resource>/<action>`，夹具模块 `modules/demo`）：

  | 路由 | 所需权限点 | 用途 |
  |---|---|---|
  | `GET /api/demo/invoice` | `demo:invoice:read` | 列表 —— 观察数据范围 |
  | `GET /api/demo/invoice/{id}` | `demo:invoice:read` | 详情 —— 观察范围外记录 |
  | `POST /api/demo/invoice/approve` | `demo:invoice:approve` | 属性级校验（条件 `$record.amount <= 10000`） |
  | `POST /api/demo/invoice/export` | `demo:invoice:export` | 权限点级 scope 收紧观察 |

- **夹具业务表** `rbac_scope_item`（REQ-009 迁移新增；列名见 [index.md](index.md) §3.5）：`tenant_id` / `id` / `title` / `amount` / `dept_id` / `created_by`。
- **组织树**（REQ-008 真实表 `org_node`）：`org-hq` → {`org-dept-a` → `org-dept-a-sub`, `org-dept-b` → `org-dept-b-sub`, `org-dept-c`}。
- **主部门判定**：来自 `user_org.is_primary = TRUE`（REQ-008 真实表）。
- **错误码**：`NOT_FOUND`(404，资源类越权) / `INSUFFICIENT_PERMISSION`(403，动作类越权)。
- **跑法**：真实库用例用 `base_test`（应用连接用普通角色 `base_app`）；DSL 解析用例为**单元测试**（`crates/platform-rbac` 内）。

---

### TC-009-015: `ALL` 返回全部记录，不加任何范围过滤
- **关联 AC**: AC-S-038
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1010`（`allscope@example.com`）绑定角色 `rbac-all`（`data_scope = ALL`），主部门 `org-dept-a`。
  - 夹具业务数据见 [index.md](index.md) §3.5，`tenant-a` 共 **13** 条 `rbac_scope_item`。
- **测试步骤**:
  1. 以 `u-1010` 登录 `tenant-a`，调 `GET /api/demo/invoice`（`limit=200`）。
  2. 记录返回的 id 集合 S 与条数。
  3. 用管理员连接直接查 `SELECT id FROM rbac_scope_item WHERE tenant_id = 'tenant-a'` 得全集 U。
- **预期结果**:
  1. 接口返回 HTTP 200。
  2. S 的条数 = 13，|S ∩ U| = |U| = 13（S ⊇ U，且两者都是 `tenant-a` 全部记录）。
  3. S **不含** `tenant-b` 的记录 `item-t2-1`（租户条件仍生效，`ALL` 只放宽**租户内**的范围，不放宽租户）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（scope = ALL）+ 集合相等断言

### TC-009-016: `OWN_ORG` 只返回主部门的数据
- **关联 AC**: AC-S-038
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1001`（`owner@example.com`）绑定角色 `rbac-owner`（`data_scope = OWN_ORG`），`user_org` 中 `org-dept-a` 行 `is_primary = TRUE`（唯一一条主部门）。
  - 夹具业务数据见 [index.md](index.md) §3.5。
- **测试步骤**:
  1. 以 `u-1001` 调 `GET /api/demo/invoice`（`limit=200`），记录 id 集合 S。
  2. 以管理员连接查 `SELECT id, dept_id FROM rbac_scope_item WHERE tenant_id='tenant-a' AND dept_id='org-dept-a'` 得期望集合 E。
  3. 逐条核对 S 中每条的 `dept_id`。
- **预期结果**:
  1. HTTP 200，S = {`item-a1`, `item-big`}，条数 = 2，且 S = E（集合相等）。
  2. E 恰为 {`item-a1`, `item-big`}。
  3. S 中每条 `dept_id` 恒等于 `org-dept-a`；S **不含** `item-a2`（子节点，`OWN_ORG` 不含下级）、**不含** `item-b1`/`item-b2`/`item-c1`（他部门）、**不含** `item-s1`/`item-s2`/`item-s3`（他人创建，即使同在本部门也**不在** `OWN_ORG` 范围内）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（scope = OWN_ORG）+ 集合相等断言

### TC-009-017: `OWN_ORG_AND_BELOW` 返回主部门及其**子节点**的数据（递归，非单层）
- **关联 AC**: AC-S-038
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1008`（`other@example.com`）绑定角色 `rbac-owner-below`（`data_scope = OWN_ORG_AND_BELOW`），主部门 `org-dept-a`（`is_primary = TRUE`）。
  - `org_node` 中 `org-dept-a-sub` 的 `parent_id = org-dept-a`（子节点），`org-dept-b-sub` 的 `parent_id = org-dept-b`（**别的分支**，不得被包含）。
- **测试步骤**:
  1. 以 `u-1008` 调 `GET /api/demo/invoice`（`limit=200`），记录 id 集合 S。
  2. 打开该次请求的 SQL 日志，查看数据范围条件的形态。
  3. 在 `org_node` 上临时再插一层：`org-dept-a-sub2`（`parent_id = org-dept-a-sub`），并插一条 `item-a3`（`dept_id = org-dept-a-sub2`），重跑步骤 1。
- **预期结果**:
  1. 第 1 次：S = {`item-a1`, `item-a2`, `item-big`}，条数 = 3 —— **含子节点** `item-a2`（证明是递归子树，不是单层）。
  2. SQL 中数据范围条件为**递归 CTE**（`WITH RECURSIVE`）或等价的子树集合查询，**不得**是只取直接子节点的 `IN (SELECT org_node_id FROM user_org …)` 形式。
  3. 第 3 次：S 变为 {`item-a1`, `item-a2`, `item-a3`, `item-big`}（条数 4）—— **孙节点**也被包含；S 仍**不含** `item-b1`/`item-b2`（他分支子树）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（子树深度 1 / 深度 2）+ 递归语义断言

### TC-009-018: `CUSTOM` 返回条件命中的数据
- **关联 AC**: AC-S-038
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1009`（`custom@example.com`）绑定角色 `rbac-custom`（`data_scope = CUSTOM`，条件 `$record.amount <= 10000`）。
  - 夹具业务数据中 `item-big` 的 `amount = 20000`，其余 12 条 `amount = 5000`。
- **测试步骤**:
  1. 以 `u-1009` 调 `GET /api/demo/invoice`（`limit=200`），记录 id 集合 S。
  2. 用管理员连接查 `SELECT id FROM rbac_scope_item WHERE tenant_id='tenant-a' AND amount <= 10000` 得期望集合 E。
  3. 核对 `item-big` 是否在 S 中。
- **预期结果**:
  1. HTTP 200，S 的条数 = 12。
  2. S = E（集合相等），且 S **不含** `item-big`。
  3. `item-big` **不在** S 中（条件不满足 → 不可见）；同时 S 中 12 条全部满足 `amount <= 10000`，**无一条** `amount > 10000`。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（scope = CUSTOM）+ 集合相等断言

### TC-009-019: 五种 scope 的结果互不相同（夹具有效性自检）
- **关联 AC**: AC-S-038
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；五个探针账号：`u-1010`（ALL）、`u-1001`（OWN_ORG）、`u-1008`（OWN_ORG_AND_BELOW）、`u-1007`（SELF）、`u-1009`（CUSTOM），五者主部门**同为** `org-dept-a`。
  - 夹具业务数据见 [index.md](index.md) §3.5。
- **测试步骤**:
  1. 依次用五个账号调 `GET /api/demo/invoice`，得到集合 S_all / S_own / S_below / S_self / S_custom。
  2. 两两比对（10 组组合）：既比对**集合是否相同**，也比对**条数**。
  3. 把 5 个集合与管理员连接查出的全集 U（13 条）做包含关系核对。
- **预期结果**:
  1. 五个请求均返回 HTTP 200。
  2. **任意两组集合都不相等**（`Si ≠ Sj`），且条数分别为 13 / 2 / 3 / 4 / 12，**互不相同**。
  3. 五个集合**都**是 U 的子集；S_all = U；S_own ⊂ S_below ⊂ S_all；S_self 与 S_own **互不包含**（各有对方没有的元素）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（scope × 期望集合）+ 全对组合不交断言

### TC-009-020: 兼岗默认按主部门：`OWN_ORG` 不含兼职部门任何数据
- **关联 AC**: AC-S-039
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1002`（`parttime@example.com`）绑定角色 `rbac-owner`（`OWN_ORG`）。
  - `user_org` 中 `u-1002` 有**两行**：`org-dept-a`（`is_primary = TRUE`）、`org-dept-b`（`is_primary = FALSE`）。
  - `org-dept-b` 下有数据 `item-b1`（`dept_id = org-dept-b`，`created_by = u-1002`）。
- **测试步骤**:
  1. 以 `u-1002` 调 `GET /api/demo/invoice`（`limit=200`），记录 id 集合 S。
  2. 核对 S 中是否出现 `dept_id = org-dept-b` 的记录。
  3. 用管理员连接查 `user_org` 中 `u-1002` 的全部行。
- **预期结果**:
  1. HTTP 200，S = {`item-a1`, `item-big`}（条数 2）—— 与**单部门**用户 `u-1001` 的结果**完全相同**。
  2. S 中**没有任何** `dept_id = org-dept-b` 的记录：`item-b1` **不在** S；`item-s2`（`dept_id = org-dept-b`，他人创建）**不在** S；`item-b2`（`org-dept-b` 子节点）**不在** S。
  3. `user_org` 中 `u-1002` 仍有 2 行（`is_primary` 一真一假），即**默认行为不是靠夹具消除兼岗实现的**。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（默认取并集 → 可见范围被撑大）+ 对照组（单部门用户）

### TC-009-021: 兼岗默认 `OWN_ORG_AND_BELOW` 只含主部门子树，不含兼职部门子树
- **关联 AC**: AC-S-039
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1002`（`parttime@example.com`），角色 `rbac-owner-below`（`OWN_ORG_AND_BELOW`）；`user_org` 同 TC-009-020。
  - `org-dept-b-sub`（`org-dept-b` 的子节点）下有数据 `item-b2`。
- **测试步骤**:
  1. 以 `u-1002` 调 `GET /api/demo/invoice`（`limit=200`），记录 id 集合 S。
  2. 核对 S 中是否出现 `org-dept-b` 及其子树的记录。
- **预期结果**:
  1. HTTP 200，S = {`item-a1`, `item-a2`, `item-big`}（条数 3）—— 只含 `org-dept-a` 及其子节点 `org-dept-a-sub`。
  2. `item-b1`（`org-dept-b`）、`item-b2`（`org-dept-b-sub`）、`item-s2`（`org-dept-b`）**均不在** S 中。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（子树并集被兼职撑大）+ 对照组

### TC-009-022: 显式开启 `use_all_memberships = true` 后取并集
- **关联 AC**: AC-S-039
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1002`（`parttime@example.com`）。
  - 角色 `rbac-union` 已建：`data_scope = OWN_ORG`、并开启 `use_all_memberships = true`（配置项存储位置见 [index.md](index.md) §7 待确认约定 #4）。
  - `user_org` 同 TC-009-020。
- **测试步骤**:
  1. 把 `u-1002` 绑定到角色 `rbac-union`（并解绑 `rbac-owner` / `rbac-owner-below`），调 `GET /api/demo/invoice`，记录 id 集合 S。
  2. 把 `u-1002` 的绑定改回角色 `rbac-owner-below`（`OWN_ORG_AND_BELOW`，`use_all_memberships = false`），再调同一接口，记录 S'。
  3. 核对 S 与 S' 的差集。
- **预期结果**:
  1. HTTP 200，S = {`item-a1`, `item-big`, `item-b1`, `item-s2`}（条数 4）—— `dept_id ∈ {org-dept-a, org-dept-b}`，**含** `item-b1`（`org-dept-b` 直属）与 `item-s2`。
  2. S' = {`item-a1`, `item-a2`, `item-big`}（条数 3）。
  3. S \ S' = {`item-b1`, `item-s2`}、S' \ S = {`item-a2`} —— 开关**确实生效且只影响并集行为**；`item-b2`（`org-dept-b` 子节点）在 `OWN_ORG` 口径下**仍不可见**（并集也只按「本部门」而非「本部门及下级」）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（开关关 → 开）+ 差集断言

### TC-009-023: 兼岗的默认行为不随租户 / 角色 / 缓存状态变化
- **关联 AC**: AC-S-039
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1002`（`parttime@example.com`，租户 `tenant-a`）。
  - `user_org` 中 `u-1002` 在 `org-dept-a`（主）与 `org-dept-b`（非主）各一行。
  - 平台进程内缓存已存在 `u-1002` 的权限/范围缓存（先调一次列表接口生成缓存）。
- **测试步骤**:
  1. 调一次 `GET /api/demo/invoice`（生成缓存），记录 S。
  2. 在 `tenant-b` 用同样的组织形状为 `u-1002` 的同租户镜像账号建成员关系（`org-t2-dept-a` 主 / `org-t2-dept-b` 非主），以该账号调同一接口，记录 S2。
  3. 给 `u-1002` 换绑另一个角色 `rbac-other-scope`（`OWN_ORG`，同样 `use_all_memberships = false`），再调一次，记录 S3。
  4. 重启平台进程，再调一次，记录 S4。
- **预期结果**:
  1. S = {`item-a1`, `item-big`}（条数 2）。
  2. S2 在 `tenant-b` 的镜像数据下也**只含主部门**记录（`tenant-b` 的 `org-t2-dept-b` 下的记录**不出现**）。
  3. S3 = {`item-a1`, `item-big`}（与 S **相同**）—— 换角色不改变默认口径。
  4. S4 = {`item-a1`, `item-big`}（与 S **相同**）—— 重启（缓存重建）不改变默认口径。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（跨租户 / 换角色 / 缓存态）+ 不变量断言

### TC-009-024: 主部门缺失 → `OWN_ORG` / `OWN_ORG_AND_BELOW` 退化为 `SELF`
- **关联 AC**: AC-S-040
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1003`（`noprimary@example.com`）绑定角色 `rbac-owner`（`OWN_ORG`）。
  - `user_org` 中 `u-1003` 只有一行：`org-dept-a`，`is_primary = FALSE`（**无任何主部门**）。
  - `item-v1`（`created_by = u-1003`、`dept_id = org-dept-c`、`amount = 5000`）；`item-a1`（`dept_id = org-dept-a`，他人创建）。
- **测试步骤**:
  1. 以 `u-1003` 调 `GET /api/demo/invoice`（`limit=200`），记录 id 集合 S。
  2. 把 `u-1003` 换绑到角色 `rbac-owner-below`（`OWN_ORG_AND_BELOW`），重调同一接口，记录 S'。
  3. 核对 S、S' 是否含 `item-a1` / `item-big`（`org-dept-a` 的记录）。
- **预期结果**:
  1. HTTP 200，S = {`item-v1`}（条数 **1**，非空 —— 因此「看到空」不能蒙混过关）。
  2. S' = {`item-v1`}（与 S 相同 —— 两种 scope 都退化为 `SELF`）。
  3. `item-a1`、`item-big`（`org-dept-a` 的记录）**均不在** S 与 S' 中；**不得**退化为 `ALL`（不得看到 13 条），**不得**退化为 `OWN_ORG`（不得看到 `org-dept-a` 的 2 条）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径（主部门缺失）+ fail closed 断言（集合精确相等）

### TC-009-025: 主部门歧义（两行 `is_primary = TRUE`）→ 同样退化为 `SELF`
- **关联 AC**: AC-S-040
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）。
  - 账号 `u-1004`（`dualprimary@example.com`）在 `user_org` 中有**两行** `is_primary = TRUE`：`org-dept-a` 与 `org-dept-c`。
  - ⚠️ 该歧义状态必须由 **REQ-009 迁移加索引之前**的种子数据造出；若索引已存在，见 [index.md](index.md) §7 待确认约定 #3（测试内临时 `DROP INDEX user_org_one_primary_idx` 造出歧义，测完重建）。
  - `item-w1`（`created_by = u-1004`、`dept_id = org-dept-c`）；`item-a1`（`org-dept-a`，他人创建）；`item-c1`（`org-dept-c`，他人创建）。
- **测试步骤**:
  1. 以 `u-1004` 调 `GET /api/demo/invoice`（`limit=200`），记录 id 集合 S。
  2. 把 `u-1004` 换绑到角色 `rbac-owner-below`，重调同一接口，记录 S'。
  3. 核对 S、S' 与管理员连接查出的 `user_org` 歧义行。
- **预期结果**:
  1. HTTP 200，S = {`item-w1`}（条数 **1**）—— 不猜哪一个是主部门，直接退化为 `SELF`。
  2. S' = {`item-w1`}（与 S 相同）。
  3. `item-a1`（`org-dept-a`）、`item-c1`（`org-dept-c`）**均不在** S 与 S' 中；不得出现「取第一条主部门」的结果（那会得到 `org-dept-a` 的 2 条）或「两个主部门取并集」的结果（那会得到 `org-dept-a` + `org-dept-c` 的记录）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径（歧义不可猜）+ fail closed 断言（集合精确相等）

### TC-009-026: 部分唯一索引挡住第二个主部门行（数据库拒绝）
- **关联 AC**: AC-S-040
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；REQ-009 的迁移已建立部分唯一索引 `user_org_one_primary_idx ON user_org (tenant_id, user_id) WHERE is_primary`。
  - `u-1001` 当前有且仅有一行 `is_primary = TRUE`（`org-dept-a`）。
- **测试步骤**:
  1. 用管理员连接执行 `INSERT INTO user_org (tenant_id, id, user_id, org_node_id, role, is_primary) VALUES ('tenant-a','uo-9001','u-1001','org-dept-c',NULL,TRUE)`。
  2. 记录数据库返回的错误码与错误信息。
  3. 再查 `user_org` 中 `u-1001` 的行数与 `is_primary` 分布。
  4. 换用普通角色 `base_app` 通过平台接口尝试加第二条主部门成员关系（`POST /api/org/user-org`，`isPrimary = true`）。
- **预期结果**:
  1. 第 1 步**被数据库拒绝**：PostgreSQL 错误码 `23505`（unique_violation），约束名指向 `user_org_one_primary_idx`（或等价的「至多一个主部门」约束）。
  2. 错误信息**不含**任何业务数据（不含组织名、用户邮箱等），只表达唯一约束冲突。
  3. `u-1001` 的 `is_primary = TRUE` 行仍为 **1** 条；新增行**未落库**。
  4. 第 4 步接口返回 HTTP 409 `CONFLICT` 或 4xx（**不得** 2xx），错误体说明「已有主部门」这一约束，且响应体不含其他租户/用户的信息。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（唯一约束）+ 负向用例（写入必须失败）

### TC-009-027: 多角色的 scope 取**最宽松**（并集语义）
- **关联 AC**: AC-S-041
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1005`（`multirole@example.com`），主部门 `org-dept-a`。
  - 绑定三个角色：`rbac-multi-a`（`OWN_ORG`）、`rbac-multi-b`（`OWN_ORG`）、`rbac-multi-c`（`OWN_ORG_AND_BELOW`）；三者均授予普通读权限点 `demo:invoice:read`。
  - `item-m1`（`created_by = u-1005`、`dept_id = org-dept-a`、`amount = 5000`）；`item-a2`（`dept_id = org-dept-a-sub`）。
- **测试步骤**:
  1. 以 `u-1005` 调 `GET /api/demo/invoice`，记录 id 集合 S。
  2. 把 `u-1005` 的绑定改为**只留** `rbac-multi-c`（`OWN_ORG_AND_BELOW`），重调同一接口，记录 S'。
  3. 把 `u-1005` 的绑定改为**只留** `rbac-multi-a`（`OWN_ORG`），重调同一接口，记录 S''。
- **预期结果**:
  1. HTTP 200，S = {`item-a1`, `item-a2`, `item-big`, `item-m1`}（条数 4）—— 取 `OWN_ORG_AND_BELOW`（最宽松），**含子节点** `item-a2`。
  2. S' = {`item-a1`, `item-a2`, `item-big`, `item-m1`}（与 S 相同 —— 最宽松者仍在）。
  3. S'' = {`item-a1`, `item-big`, `item-m1`}（条数 3，**不含** `item-a2`）—— 若实现把多角色 scope 取**交集**或取「最严」，第 1 步会少掉 `item-a2`，测试必须失败。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（绑定组合）+ 集合差集断言（区分并集/交集/最严）

### TC-009-028: 权限点级 scope 覆盖角色级 scope
- **关联 AC**: AC-S-041
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；账号 `u-1005` 绑定 `rbac-multi-a` / `rbac-multi-b` / `rbac-multi-c`（同 TC-009-027）。
  - 角色 `rbac-multi-b` 授予 `demo:invoice:export`，且该权限点级 scope 被收紧为 `SELF`；其余角色的 `export` 授权被移除。
  - `item-m1` 由 `u-1005` 创建；`item-a1` / `item-a2` 由他人创建。
- **测试步骤**:
  1. 以 `u-1005` 调 `POST /api/demo/invoice/export`（需 `demo:invoice:export`），记录返回的可见记录集合 E。
  2. 以 `u-1005` 调 `GET /api/demo/invoice`（需 `demo:invoice:read`），记录集合 R。
  3. 核对 E 与 R 的包含关系。
- **预期结果**:
  1. HTTP 200，E = {`item-m1`}（条数 **1**）—— 权限点级 `SELF` 生效，**不得**是角色级的 `OWN_ORG_AND_BELOW`（那会得到 4 条）。
  2. HTTP 200，R = {`item-a1`, `item-a2`, `item-big`, `item-m1`}（条数 4）。
  3. E ⊊ R（严格子集），且 R \ E = {`item-a1`, `item-a2`, `item-big`} —— 证明权限点级覆盖确实生效，而不是「所有权限点都退化成 SELF」。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（角色级 × 权限点级）+ 包含关系断言

### TC-009-029: 角色级 scope 优先于用户默认（`SELF`）
- **关联 AC**: AC-S-041
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）。
  - 账号 `u-1011`（`norole@example.com`）**没有任何角色绑定**，主部门 `org-dept-a`；其自建记录 `item-n1`（`created_by = u-1011`、`dept_id = org-dept-a`）。
  - 账号 `u-1001` 绑定 `rbac-owner`（`OWN_ORG`），主部门 `org-dept-a`；记录 `item-a1` / `item-big` 的 `dept_id = org-dept-a`。
- **测试步骤**:
  1. 以 `u-1011` 调 `GET /api/demo/invoice`，记录集合 N。
  2. 以 `u-1001` 调同一接口，记录集合 O。
  3. 核对 N 与 O。
- **预期结果**:
  1. HTTP 200，N = {`item-n1`}（条数 **1**）—— 无角色时落到**用户默认** `SELF`。
  2. HTTP 200，O = {`item-a1`, `item-big`}（条数 2）。
  3. |O| > |N| 且 `N ≠ O`：同一主部门下，**角色级 `OWN_ORG` 严格宽于用户默认 `SELF`** —— 若用户默认被当成 `OWN_ORG`，N 会含 `item-a1`/`item-big`（2 条），测试必须失败。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 优先级判定表（权限点级 > 角色级 > 用户默认）

### TC-009-030: 多个 `CUSTOM` 条件同时存在时取交集，并在响应中标明使用了自定义范围
- **关联 AC**: AC-S-041
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）。
  - 账号 `u-1005` 绑定两个 `CUSTOM` 角色：`rbac-custom-a`（条件 `$record.amount <= 10000`）与 `rbac-custom-b`（条件 `$record.dept_id == 'org-dept-a'`），两者都授予 `demo:invoice:read`。
  - 夹具：`item-a1`（`amount=5000`, `dept_id=org-dept-a`）、`item-a2`（`amount=5000`, `dept_id=org-dept-a-sub`）、`item-big`（`amount=20000`, `dept_id=org-dept-a`）。
- **测试步骤**:
  1. 以 `u-1005` 调 `GET /api/demo/invoice`，记录集合 S。
  2. 查看响应体（或平台返回的范围标注字段）是否标明「使用了自定义范围」。
  3. 核对 S 与两个条件各自的命中集。
- **预期结果**:
  1. HTTP 200，S = {`item-a1`}（条数 **1**）—— 两个条件**都要满足**（交集），**不是**并集（并集会是 3 条）。
  2. 响应体含一个**明确的**自定义范围标记字段（如 `customScopeUsed: true` 或 `scope: "CUSTOM"`），取值为真 / `CUSTOM`；**不得**返回 `ALL` 或 `OWN_ORG` 之类的普通 scope 值冒充。
  3. `item-a2`（`amount` 满足、`dept_id` 不满足）与 `item-big`（`dept_id` 满足、`amount` 不满足）**都不在** S 中。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（CUSTOM 不参与宽松比较）+ 交集语义断言

### TC-009-031: DSL 合法表达式解析成功并走参数绑定
- **关联 AC**: AC-S-042
- **前置条件**:
  - **执行位置**: **单元测试**（`crates/platform-rbac` 内）。
  - 白名单字段集：`$record.amount`、`$record.dept_id`（`$user.id`、`$ctx.tenant_id` 亦在白名单）。
- **测试步骤**:
  1. 解析表达式 `$record.amount <= 10000`。
  2. 把解析结果渲染成 SQL 条件片段。
  3. 检查渲染出的 SQL 文本与绑定参数列表。
- **预期结果**:
  1. 解析**成功**，得到一棵可求值的表达式树（不是字符串）。
  2. 渲染出的 SQL 条件形如 `amount <= $n`，**参数序号**与业务参数连号。
  3. SQL 文本中**不出现字面量 `10000`**；绑定参数列表中第 n 项为整型 `10000`（**参数绑定**，不是拼接）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径 + 参数化断言（语句里看不到字面量值）

### TC-009-032: 白名单外字段 → 解析失败并给出明确原因
- **关联 AC**: AC-S-042
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-rbac` 内）；白名单 = `$record.amount` / `$record.dept_id` / `$user.id` / `$ctx.tenant_id`。
- **测试步骤**:
  1. 解析 `$record.secret == "x"`。
  2. 解析 `$record.amount <= 10000 AND $record.password == "p"`。
  3. 检查错误信息里给出的原因分类。
- **预期结果**:
  1. 解析**失败**（返回 `Err`），**不得**返回「解析成功但忽略该字段」。
  2. 第 2 步同样失败。
  3. 错误信息**明确**指出「字段不在白名单」并**点名该字段**（`secret` / `password`）；错误原因分类可与「函数调用」「多语句」区分开。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 负向用例（白名单外）+ 错误可区分性断言

### TC-009-033: 函数调用 → 解析失败
- **关联 AC**: AC-S-042
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-rbac` 内）。
- **测试步骤**:
  1. 解析 `now() < $record.created_at`。
  2. 解析 `substring($record.name, 1, 2) == "ab"`。
  3. 解析 `lower($record.code) == "x"`。
- **预期结果**:
  1~3. 三次解析**全部失败**；错误信息明确指出「不允许函数调用」并**点名函数名**（`now` / `substring` / `lower`）。
  4. 错误原因分类与「字段不在白名单」「不允许多语句」**可区分**。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（函数调用绕过白名单）+ 负向用例

### TC-009-034: 多语句 → 解析失败
- **关联 AC**: AC-S-042
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-rbac` 内）。
- **测试步骤**:
  1. 解析 `$record.amount <= 10000; DROP TABLE t`。
  2. 解析 `$record.amount <= 10000; SELECT 1`。
  3. 解析含注释的写法 `$record.amount <= 10000 -- comment`。
- **预期结果**:
  1. 第 1 步解析**失败**，错误信息明确指出「不允许多语句」并**原样引用**被拒片段（或指出检测到第二个语句）。
  2. 第 2 步同样失败。
  3. 第 3 步被拒（注释被视为非表达式内容，**不得**被静默截断后当作合法表达式放行）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（SQL 注入 / 语句拼接）+ 负向用例

### TC-009-035: 注入串被当作普通字符串值绑定，查询语义不变
- **关联 AC**: AC-S-042
- **前置条件**:
  - **执行位置**: **真实库（`base_test`）** + 单元断言。
  - 角色 `rbac-custom`（`CUSTOM`，条件 `$record.name == "a"`）；业务表 `rbac_scope_item` 有 `title` 字段。
  - 夹具中存在 `title = 'a'` 的记录 1 条。
- **测试步骤**:
  1. 把条件的字面量改为 `"a' OR '1'='1"`，渲染 SQL 条件并取 SQL 文本与绑定参数。
  2. 用该条件执行一次列表查询，记录返回集合 S。
  3. 把条件改回 `"a"`，再执行一次，记录集合 S0。
  4. 在 SQL 文本中搜索 `OR`、`'1'='1'` 等注入片段。
- **预期结果**:
  1. SQL 文本中**不出现**注入串；绑定参数里出现的是完整字符串 `a' OR '1'='1`（原样，作为一个参数）。
  2. S = ∅（没有记录的字面量等于该注入串）。
  3. S0 = 那 1 条 `title = 'a'` 的记录 —— 与 S **不同**，证明条件确实生效且**改变了语义**（注入串没有把条件变成恒真）。
  4. SQL 文本中 `OR` 不作为逻辑连接符出现（`OR` 只可能出现在普通字符串里）；不得出现 `'1'='1'` 这样的裸比较。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（经典注入串）+ 破坏性对照（注入前后语义必须不同）

### TC-009-036: 属性级条件满足 → 放行
- **关联 AC**: AC-S-043
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP。
  - 权限点 `demo:invoice:approve` 携带条件 `$record.amount <= 10000`（写在 `modules/demo/manifest.json` 的 `permissions[].condition`）。
  - 账号 `u-1001`（`owner@example.com`）绑定角色 `rbac-owner`，含 `demo:invoice:approve`；主部门 `org-dept-a`。
  - `item-a1`（`amount = 5000`、`dept_id = org-dept-a`）在 `u-1001` 的 `OWN_ORG` 范围内。
- **测试步骤**:
  1. 以 `u-1001` 调 `POST /api/demo/invoice/approve`，body `{"id":"item-a1"}`。
  2. 记录状态码与响应体。
- **预期结果**:
  1. HTTP **200**（条件满足 + 范围满足）。
  2. 响应体含审批结果字段（如 `{"approved": true}`），且**不含**该记录的条件字段原值之外的内容（不得回显金额之外的敏感字段）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径（RBAC + ABAC 同时满足）

### TC-009-037: 属性级条件不满足 → 被拒
- **关联 AC**: AC-S-043
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP。
  - 同 TC-009-036，但目标是 `item-big`（`amount = 20000`、`dept_id = org-dept-a`，**在范围内**但**条件不满足**）。
- **测试步骤**:
  1. 以 `u-1001` 调 `POST /api/demo/invoice/approve`，body `{"id":"item-big"}`。
  2. 记录状态码、错误码与响应体全文。
  3. 核对响应体是否泄露金额等被拒资源细节。
- **预期结果**:
  1. HTTP **403**（动作类越权，契约 C5）+ `code = INSUFFICIENT_PERMISSION`；**不得**返回 200。
  2. 错误体 `detail` 为**英文 ASCII**，且**不包含** `20000`、`item-big` 的金额/标题等资源细节（可含权限点编码）。
  3. 响应体中**不出现** `amount` 的实际值或记录标题；只允许出现权限点编码与通用说明。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（超金额审批）+ 精确错误码断言（非 2xx 不足以判定）

### TC-009-038: 缺权限点 → 被拒；条件引用白名单外属性 → fail closed 不通过
- **关联 AC**: AC-S-043
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP。
  - 账号 `u-1007`（`noperm@example.com`）绑定角色 `rbac-self`（`SELF`），**不含** `demo:invoice:approve`；其自建记录 `item-s1`（`amount = 5000`、`created_by = u-1007`，**在其 SELF 范围内**）。
  - `platform_permission` 表可写（管理员连接），用于把 `demo:invoice:approve` 的 `condition` 改成引用白名单外字段的表达式。
- **测试步骤**:
  1. 以 `u-1007` 调 `POST /api/demo/invoice/approve`，body `{"id":"item-s1"}`。
  2. 记录状态码与错误码。
  3. 用管理员连接把 `demo:invoice:approve` 的条件改为 `$record.secret <= 1`（`secret` 不在白名单），等缓存失效后，以 `u-1001` 调 `POST /api/demo/invoice/approve`，body `{"id":"item-a1"}`。
  4. 记录第 3 步的状态码与错误码，并把条件改回 `$record.amount <= 10000`。
- **预期结果**:
  1. 第 1 步返回 HTTP **403** + `code = INSUFFICIENT_PERMISSION`（缺权限点 → 动作类越权），**不得** 200、**不得** 404（记录 `item-s1` 在其范围内，404 会把「缺权限」与「不在范围」混为一谈）。
  2. 错误体 `detail` 为英文 ASCII，且**不出现** `item-s1` 的标题等细节。
  3. 第 3 步返回 HTTP **403** + `code = INSUFFICIENT_PERMISSION`（白名单外属性 → **判定不通过**，fail closed），**不得** 200（fail open）。
  4. 第 4 步后重跑 TC-009-036 的请求恢复 200（说明第 3 步的拒绝源于条件不可用，而非权限点被误撤）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（缺权限点 / 条件不可用）+ fail closed 断言 + 恢复性对照

---
