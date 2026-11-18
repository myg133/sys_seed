# A. 权限点注册表与角色模型 —— 测试用例（AC-S-033 ~ AC-S-037）

- **需求**: REQ-009「RBAC 权限与数据范围」
- **来源**: [acceptance.md](../acceptance.md) §AC-S-033 ~ AC-S-037、[design-summary.md](../design-summary.md) §2 ~ §3、[api-conventions v1 C1/C4/C5/C6](../../../contracts/api-conventions/v1.md)
- **用例数**: 14（TC-009-001 ~ TC-009-014）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## A.0 本域约定

- **权限点编码**：`module:resource:action`，全小写连字符分隔（契约 C4）。本域用到的登记权限点见 [index.md](index.md) §3.6。
- **平台接口**（前缀 `/api/rbac`，契约 C1；BA 已裁定采纳本用例集推导的子路径）：

  | 路由 | 说明 |
  |---|---|
  | `GET /api/rbac/roles` | 列角色（仅本租户） |
  | `POST /api/rbac/roles` | 建角色（body：`code` / `name` / `dataScope`） |
  | `GET /api/rbac/roles/{id}` | 角色详情 |
  | `DELETE /api/rbac/roles/{id}` | 删角色 |
  | `GET /api/rbac/roles/{id}/permissions` | 查角色**展开后**的权限点集合（含继承展开、通配展开） |
  | `POST /api/rbac/roles/{id}/permissions` | 给角色授权（body：`{"permissionCode":"sandbox:scope-item:read"}`） |
  | `DELETE /api/rbac/roles/{id}/permissions/{code}` | 撤权 |
  | `POST /api/rbac/roles/{id}/inherit` | 设角色继承（body：`{"parentRoleId":"<id>"}`） |
  | `POST /api/rbac/users/{user_id}/roles` | 绑定用户到角色（body：`{"roleId":"<id>"}`） |
  | `GET /api/rbac/permissions` | 读权限点注册表（平台级） |

- **错误码**：`NOT_FOUND`(404) / `CONFLICT`(409) / `INSUFFICIENT_PERMISSION`(403) / `UNAUTHENTICATED`(401)；错误体为 RFC 9457 `application/problem+json`。
- **固定夹具**：租户 `tenant-a` / `tenant-b`；内置角色 `platform-admin`（`builtin=true` / `ALL`）与 `platform-user`（`builtin=true` / `SELF`）—— **按租户物化**（租户初始化时在该租户下各建一行）；测试角色 `rbac-owner` / `rbac-owner-below` / `rbac-self` / `rbac-all` / `rbac-custom` / `rbac-multi-a` / `rbac-multi-b` / `rbac-multi-c` / `rbac-wildcard`。
- **平台表与列**（design-summary §3，BA 已裁定）：

  | 表 | 关键列 |
  |---|---|
  | `platform_permission` | `code` / `name` / `condition`（**平台级，无 `tenant_id`**） |
  | `platform_role` | `tenant_id` / `id` / `code` / `name` / `data_scope` / `custom_filter` / `use_all_memberships` / `builtin` |
  | `platform_role_permission` | `tenant_id` / `role_id` / `permission_code` / `data_scope`（**可空 = 不覆盖角色级**） |
  | `platform_role_inherit` | `tenant_id` / `role_id` / `parent_role_id` |
  | `platform_user_role` | `tenant_id` / `user_id` / `role_id` |
  | `platform_permission_epoch` | `tenant_id`（PK） / `epoch` / `updated_at` |

- **RLS policy 口径**（`platform-db/src/rls.rs`）：policy 名 = `<表名>_tenant_isolation`，条件为 `USING (tenant_id = current_setting('app.tenant_id', true))` + `WITH CHECK`；租户上下文由会话变量 `app.tenant_id` 设定。
- **跑法**：真实库用例用 `base_test` 库；应用连接必须用普通角色 `base_app`（超管会被 `require_isolation` 拒绝启动）。`cargo test` 只能有一个 `--`。

---

### TC-009-001: manifest 有权限点、库里没有 → 启动失败并逐条列出缺失编码与来源 manifest 路径
- **关联 AC**: AC-S-033
- **前置条件**:
  - `modules/sandbox/manifest.json` 声明了 `sandbox:scope-item:read` / `sandbox:scope-item:approve` / `sandbox:scope-item:export`。
  - 真实库 `base_test` 的 `platform_permission` 表中**已删除** `sandbox:scope-item:export` 一行（用管理员连接 `postgres://base:base@127.0.0.1:5432/base_test` 执行 `DELETE FROM platform_permission WHERE code = 'sandbox:scope-item:export'`）。
  - `capabilities/permissions.json` 已用 `cargo xtask capabilities` 重新生成，含 `sandbox:scope-item:export`。
  - 平台以普通角色 `base_app` 连接 `base_test`（`BASE_ALLOW_SUPERUSER=false`）。
- **测试步骤**:
  1. 启动平台进程（`base serve` / `cargo run -p platform-server`）。
  2. 记录进程**退出码**与 stderr 全文。
  3. 再查 `platform_permission` 表的内容。
- **预期结果**:
  1. 进程**退出码 ≠ 0**，且在**监听端口之前**就退出（不得先起来再报错）。
  2. stderr 中**逐条**出现缺失编码 `sandbox:scope-item:export`，且**同一条**错误信息里带**来源 manifest 的文件路径** `modules/sandbox/manifest.json`（形如 `modules/foo/manifest.json` 的路径，**不是**模块 id、不是目录）；输出形态是「对账失败」级别的致命错误，**不得**是 warning，也不得只出现一行汇总。
  3. `platform_permission` 表内容与启动前**完全一致**（对账不得在启动时自动补写权限点）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 半自动（需观察进程退出码与 stderr 文本）
- **设计方法**: 等价类（缺失 / 多余 / 一致）+ 状态迁移（可启动 → 拒绝启动）

### TC-009-002: 库里有权限点、manifest 没有 → 启动失败并逐条列出多余编码
- **关联 AC**: AC-S-033
- **前置条件**:
  - `platform_permission` 表中存在一行 `code = 'sandbox:scope-item:void'`，而**任何** manifest（`modules/*/manifest.json` 与 `crates/*/manifest.json`）都**没有**声明 `sandbox:scope-item:void`。
  - `capabilities/permissions.json` 已重新生成，其中**不含** `sandbox:scope-item:void`。
  - 其余权限点两侧一致（排除本条的干扰）。
- **测试步骤**:
  1. 启动平台进程。
  2. 记录退出码与 stderr 全文。
  3. 查 `platform_permission` 表，确认 `sandbox:scope-item:void` 仍在（平台不得自动删库里的行）。
- **预期结果**:
  1. 进程**退出码 ≠ 0**，同样在监听端口前退出。
  2. stderr 中**逐条**出现多余编码 `sandbox:scope-item:void`（措辞与「缺失」情形**可区分**，如 `missing` vs `unknown`/`extra`），且不出现 `sandbox:scope-item:read` 这类实际已登记的编码。
  3. `platform_permission` 中的 `sandbox:scope-item:void` 行**仍存在** —— 平台只报告不自动改库。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 等价类 + 反向差异（manifest → 库 的方向）

### TC-009-003: 两侧一致 → 启动成功，且注册表接口可读（集合相等，不比数字）
- **关联 AC**: AC-S-033
- **前置条件**:
  - `platform_permission` 表的 `code` 集合与 `capabilities/permissions.json` 中所有模块权限点编码的集合**完全相等**。
  - ⚠️ **不写死条数** —— 注册表随权限点增减而变，判定一律按**集合**比。
  - 平台以普通角色 `base_app` 连接 `base_test`。
- **测试步骤**:
  1. 启动平台进程。
  2. 等待端口就绪，用 `owner@example.com` 的 access token 调 `GET /api/rbac/permissions`。
  3. 把返回的权限点编码集合与 `capabilities/permissions.json` 的编码集合做**集合相等**比对（逐个元素比对，不比数量）。
- **预期结果**:
  1. 进程**退出码 = 0**（或持续运行未被退出），stderr 中**没有**权限点对账相关错误。
  2. `GET /api/rbac/permissions` 返回 HTTP 200。
  3. 两个集合**逐元素相等**：返回集合中每个 `code` 都能在 `permissions.json` 中找到，反之亦然；每条含 `code` 与来源 `module`；**不返回**库表里没有的编码（如 `sandbox:scope-item:void`）。**断言里不得出现任何写死的权限点个数**。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 等价类（一致类）+ 集合相等断言（动态、不写死基数）

### TC-009-004: 角色、授权、绑定三件事可经 `/api/rbac/*` 完成，且 RLS 兜底可验
- **关联 AC**: AC-S-034
- **前置条件**:
  - 租户 `tenant-a` 存在账号 `u-1001`（`owner@example.com`）。
  - `platform_role` / `platform_role_permission` / `platform_role_inherit` / `platform_user_role` 四张表已由 REQ-009 的迁移建出，且已按 manifest 的 `tenantScoped` 声明生成 RLS policy。
  - 可用**应用角色 `base_app`** 连接（超管会被 `require_isolation` 拒绝启动）。
- **测试步骤**:
  1. 在 `tenant-a` 上下文调 `POST /api/rbac/roles`，body `{"code":"rbac-tmp","name":"临时角色","dataScope":"OWN_ORG"}`。
  2. 调 `POST /api/rbac/roles/{新角色 id}/permissions`，body `{"permissionCode":"sandbox:scope-item:read"}`。
  3. 调 `POST /api/rbac/users/u-1001/roles`，body `{"roleId":"<新角色 id>"}`。
  4. 用**管理员连接**查三张表中这三行的 `tenant_id` 列。
  5. 用 **`base_app` 连接**执行 `SET app.tenant_id = 'tenant-a'` 后 `SELECT count(*) FROM platform_role`，并用 `SELECT count(*) FROM platform_role`（**不设** `app.tenant_id`）作对照。
  6. 用 `base_app` 连接查 `pg_policies`，检索 `platform_role` / `platform_role_permission` / `platform_user_role` 三张表的 policy。
- **预期结果**:
  1. 第 1 步返回 HTTP 201，响应体含 `id` / `code=rbac-tmp` / `dataScope=OWN_ORG`。
  2. 第 2 步返回 HTTP 204 或 200；`platform_role_permission` 中出现一行 `permission_code = sandbox:scope-item:read`。
  3. 第 3 步返回 HTTP 204 或 200；`platform_user_role` 中出现一行 `user_id = u-1001`。
  4. 三张表这三行的 `tenant_id` 均为 `tenant-a`（**不是**空、不是超管值）。
  5. 设了 `app.tenant_id = 'tenant-a'` 时，`count(*)` = **租户 A 的角色行数**（与管理员连接查到的租户 A 行数相等，**不含**租户 B 的行）；**不设**该变量时 `count(*)` = **0**（`current_setting(..., true)` 返回 NULL → 行不可见，fail closed）。
  6. `pg_policies` 里能查到三张表各自的 policy，**表名逐张列出**：至少存在 `platform_role_tenant_isolation`、`platform_role_permission_tenant_isolation`、`platform_user_role_tenant_isolation` 三条（policy 名形如 `<表名>_tenant_isolation`），且 `qual` / `with_check` 含 `tenant_id = current_setting('app.tenant_id', true)`；**不得**以「有 policy 就行」蒙混过关。`platform_permission` **不得**有租户维度 policy（平台级表）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径 + 数据落地断言（含 RLS **行为**验证，不止于「policy 存在」）

### TC-009-005: 跨租户角色 id 一律 404，不泄露存在性
- **关联 AC**: AC-S-034
- **前置条件**:
  - `tenant-b` 下存在角色 `rbac-t2-only`（id 记为 `role-t2-1`，`dataScope=SELF`）。
  - `tenant-a` 的账号 `owner@example.com` 持有有效 access token（绑定租户 `tenant-a`）。
- **测试步骤**:
  1. 以 `tenant-a` 的令牌调 `GET /api/rbac/roles/role-t2-1`。
  2. 以 `tenant-a` 的令牌调 `DELETE /api/rbac/roles/role-t2-1`。
  3. 以 `tenant-a` 的令牌调 `GET /api/rbac/roles/role-t2-1/permissions`。
  4. 以 `tenant-a` 的令牌调 `GET /api/rbac/roles/role-not-exist-9999`（一个**根本不存在**的 id）作对照。
  5. 用管理员连接查 `tenant-b` 的 `platform_role` 表，确认 `role-t2-1` 仍在。
- **预期结果**:
  1~3. 三个请求均返回 HTTP **404**，`code = NOT_FOUND`；`detail` 为**英文 ASCII**且**不出现** `role-t2-1`、`tenant-b`、角色名等任何存在性细节。
  4. 对照请求同样返回 HTTP 404 + `code = NOT_FOUND`，且响应体**字段集合**与步骤 1~3 的 404 响应**完全一致**（`type`/`title`/`status`/`detail`/`instance`/`code`/`trace_id`），`type` 与 `code` 相同。
  5. `role-t2-1` 仍存在于 `tenant-b`（**未被删除**），且 `tenant-a` 侧不可见。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（跨租户 id）+ 存在性泄露对照（存在 vs 不存在）

### TC-009-006: 角色继承展开为并集（逐个列出权限点编码）
- **关联 AC**: AC-S-035
- **前置条件**:
  - 角色 `rbac-inh-a`（父）、`rbac-inh-b`（子）、`rbac-inh-c`（孙）已建，授权互不相同：`rbac-inh-a` → `sandbox:scope-item:read`；`rbac-inh-b` → `sandbox:scope-item:export`；`rbac-inh-c` → `sandbox:scope-item:approve`。
  - 继承关系：`rbac-inh-b` 继承 `rbac-inh-a`；`rbac-inh-c` 继承 `rbac-inh-b`。
- **测试步骤**:
  1. 调 `GET /api/rbac/roles/{rbac-inh-a id}/permissions`，取展开后的权限点集合 S。
  2. 调 `GET /api/rbac/roles/{rbac-inh-c id}/permissions`，取集合 S'。
  3. 用 `u-1005`（绑定 `rbac-inh-c`）的令牌调 `GET /api/rbac/roles/{rbac-inh-a id}/permissions`，确认该接口按**调用者**权限裁剪还是按角色返回。
- **预期结果**:
  1. S **逐个列出** = `sandbox:scope-item:read`、`sandbox:scope-item:export`、`sandbox:scope-item:approve` 三条（**不是只比数量相等**，三条编码要逐条出现）；不多不少。
  2. S' = 同上三条（孙角色继承链向上展开）。
  3. 返回体中每条权限点带**来源角色**（如 `sourceRoleCode`），能区分直接授权与继承而来；调用者无授权查看他人角色时返回 403 `INSUFFICIENT_PERMISSION` 或 404 `NOT_FOUND`（按平台口径二选一，但**不得**返回 200 且泄露展开集合）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径 + 逐元素集合相等断言（明确禁止只比数量）

### TC-009-007: 角色继承成环被拒，且错误信息指出构成环的两个角色
- **关联 AC**: AC-S-035
- **前置条件**:
  - 已存在继承链 `rbac-cycle-a` → 父 `rbac-cycle-b` → 父 `rbac-cycle-a` **尚未**建立（当前 `rbac-cycle-a` 无父、`rbac-cycle-b` 无父）。
  - 调用者为 `tenant-a` 的管理员账号。
- **测试步骤**:
  1. 调 `POST /api/rbac/roles/{rbac-cycle-a id}/inherit`，body `{"parentRoleId":"<rbac-cycle-b id>"}` —— 建立 a→b。
  2. 调 `POST /api/rbac/roles/{rbac-cycle-b id}/inherit`，body `{"parentRoleId":"<rbac-cycle-a id>"}` —— 试图成环。
  3. 调 `GET /api/rbac/roles/{rbac-cycle-a id}/permissions` 看展开是否仍正常。
  4. 用管理员连接查 `platform_role_inherit` 表的行。
- **预期结果**:
  1. 第 1 步返回 HTTP 204/200，`platform_role_inherit` 中出现 `role_id = rbac-cycle-a` / `parent_role_id = rbac-cycle-b` 一行。
  2. 第 2 步返回 HTTP **409** `CONFLICT`（或 422），错误体中**明确点名 `rbac-cycle-a` 与 `rbac-cycle-b` 两个角色 code**（不是「非法继承」这类泛泛措辞），`detail` 为英文 ASCII。
  3. 第 3 步仍能正常展开，集合 = 两个角色直接授权的并集（逐个列出编码），**不出现死循环或超时**（展开算法必须能终止）。
  4. `platform_role_inherit` 中**不存在** `role_id = rbac-cycle-b` / `parent_role_id = rbac-cycle-a` 的一行（被拒的写入**不得落库**）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径 + 攻击面枚举（成环 → 无限递归 / 自我提权）

### TC-009-008: 自继承与三节点环同样被拒
- **关联 AC**: AC-S-035
- **前置条件**:
  - 角色 `rbac-self-inherit`（无父）、`rbac-tri-a` / `rbac-tri-b` / `rbac-tri-c` 已建，继承关系为空。
- **测试步骤**:
  1. 调 `POST /api/rbac/roles/{rbac-self-inherit id}/inherit`，body `{"parentRoleId":"<rbac-self-inherit id>"}`（自继承）。
  2. 依次调 `POST .../rbac-tri-a/inherit`（父 = c）、`POST .../rbac-tri-b/inherit`（父 = a）、`POST .../rbac-tri-c/inherit`（父 = b）构成三节点环。
  3. 调 `GET /api/rbac/roles/{rbac-tri-a id}/permissions`。
- **预期结果**:
  1. 第 1 步返回 HTTP 409 `CONFLICT`，错误体点名 `rbac-self-inherit`（自身即构成环）。
  2. 前两步成功（a→c、b→a 都不成环），**第三步**（c→b）返回 HTTP 409 `CONFLICT`，错误体点名 `rbac-tri-c` 与 `rbac-tri-b`。
  3. 第 3 步的展开正常返回（集合 = a/b/c 直接授权并集），**不挂起、不栈溢出**。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（环长 1 / 环长 3）+ 等价类（环长不影响判定）

### TC-009-009: `sandbox:*` 匹配 `sandbox:` 下全部权限点，且不匹配 `org:` 的
- **关联 AC**: AC-S-036
- **前置条件**:
  - 注册表里已登记 `sandbox:` 前缀的权限点（`sandbox:demo:read`、`sandbox:demo:write`、`sandbox:scope-item:read`、`sandbox:scope-item:approve`、`sandbox:scope-item:export` —— 具体条数**以注册表实际内容为准，不写死**）与 `org:` 前缀的 9 个权限点（`crates/platform-org/manifest.json`）。
  - 测试角色 `rbac-wildcard` 已建，`dataScope = OWN_ORG`，**当前没有任何授权**。
- **测试步骤**:
  1. 调 `POST /api/rbac/roles/{rbac-wildcard id}/permissions`，body `{"permissionCode":"sandbox:*"}`。
  2. 调 `GET /api/rbac/roles/{rbac-wildcard id}/permissions`，取展开集合 S。
  3. 读注册表（`GET /api/rbac/permissions`），分别取出 `sandbox:` 前缀集合 S_sandbox 与 `org:` 前缀集合 S_org。
- **预期结果**:
  1. 第 1 步返回 HTTP 204/200（**写入被接受**，`sandbox:*` 是允许的通配形态）。
  2. S = S_sandbox —— `sandbox:` 下的权限点**一个不缺**（逐个列出比对），且**一条 `org:` 都不在**（`org:org-node:read`、`org:org-node:write` 等逐个确认不在 S 中）。
  3. S 中**不含**任何 `org:` 前缀编码；S 的条数 = 注册表中 `sandbox:` 前缀权限点的条数（**不得**等于注册表权限点总数）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（模块内通配）+ 集合相等断言（逐个比对，不抽样、不写死数字）

### TC-009-010: `sandbox:demo:*` 只匹配 `sandbox:demo:` 下的权限点
- **关联 AC**: AC-S-036
- **前置条件**:
  - 同 TC-009-009 的注册表夹具。
  - 测试角色 `rbac-wildcard-res` 已建，**无授权**。
- **测试步骤**:
  1. 调 `POST /api/rbac/roles/{rbac-wildcard-res id}/permissions`，body `{"permissionCode":"sandbox:demo:*"}`。
  2. 调 `GET /api/rbac/roles/{rbac-wildcard-res id}/permissions`，取展开集合 S。
  3. 读注册表，取 `sandbox:demo:` 前缀集合 S_demo 与 `sandbox:scope-item:` 前缀集合 S_scope。
- **预期结果**:
  1. 第 1 步返回 HTTP 204/200。
  2. S = S_demo（恰为 `sandbox:demo:read` 与 `sandbox:demo:write` 两条，逐个列出）；**不含** `sandbox:scope-item:read`（同模块**不同 resource** 不得被匹配）、**不含**任何 `sandbox:scope-item:*` 与 `org:*` 编码。
  3. S ⊊ S_sandbox（严格小于模块级通配的结果集）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（resource 维度边界）+ 集合相等断言

### TC-009-011: `*:*` 与跨模块通配 `*:demo:read` 写入被拒
- **关联 AC**: AC-S-036
- **前置条件**:
  - 测试角色 `rbac-wildcard-bad` 已建，**无授权**；注册表夹具同 TC-009-009。
- **测试步骤**:
  1. 调 `POST /api/rbac/roles/{rbac-wildcard-bad id}/permissions`，body `{"permissionCode":"*:*"}`。
  2. 调 `POST /api/rbac/roles/{rbac-wildcard-bad id}/permissions`，body `{"permissionCode":"*:demo:read"}`。
  3. 调 `GET /api/rbac/roles/{rbac-wildcard-bad id}/permissions`。
  4. 用管理员连接查 `platform_role_permission` 中该角色的行。
- **预期结果**:
  1. 第 1 步返回 HTTP 4xx（409 `CONFLICT` 或 422），**不得** 2xx；错误体 `detail` 说明「不允许全平台通配」这一**具体原因**（英文 ASCII）。
  2. 第 2 步同样返回 HTTP 4xx，错误原因可与第 1 步区分（跨模块通配 ≠ 全通配）。
  3. 第 3 步返回的展开集合为**空集**（`[]`）—— 失败的写入**不得**留下任何部分生效的授权。
  4. `platform_role_permission` 中该角色的权限行数为 **0**。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（全通过 / 跨模块提权）+ 负向用例（失败写入不得部分生效）

### TC-009-012: 内置角色 `platform-admin` 不可删除、不可降权、不可移除权限点
- **关联 AC**: AC-S-037
- **前置条件**:
  - 租户 `tenant-a` 初始化时已物化内置角色 `platform-admin`（`builtin = true`、`data_scope = ALL`）与 `platform-user`（`builtin = true`、`data_scope = SELF`）（**按租户物化**：租户 B 下另有一份 `platform-admin`）。
  - 调用者为 `tenant-a` 的管理员账号。
- **测试步骤**:
  1. 调 `DELETE /api/rbac/roles/{platform-admin id}`。
  2. 调 `POST /api/rbac/roles/{platform-admin id}`（或 `PUT`，按实现的更新入口）把 `dataScope` 改为 `SELF`。
  3. 调 `DELETE /api/rbac/roles/{platform-admin id}/permissions/sandbox:scope-item:read`（撤掉一个权限点）。
  4. 调 `DELETE /api/rbac/roles/{platform-user id}`。
  5. 用管理员连接查两张内置角色行。
- **预期结果**:
  1~4. 四次写操作**全部被拒**：HTTP 409 `CONFLICT` 或 403，且错误体 `detail` **明确区分「内置角色受保护」**这一类原因（措辞与普通业务冲突**可区分**，如含 `builtin` / `protected` 字样），不得是笼统的 500。
  5. `platform-admin` 行的 `data_scope` 仍为 `ALL`、`builtin` 仍为 `true`、该角色行**仍存在**；`platform-user` 行**仍存在**且 `data_scope = SELF`；`platform_role_permission` 中 `platform-admin` 仍能查到 `sandbox:scope-item:read`（若实现把内置角色的权限集合动态展开，则该权限点**依然在展开结果里**）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（删除 / 降权 / 撤权 / 批量删）+ 负向用例

### TC-009-013: `platform-admin` 的权限集合 = 注册表当前全部权限点（逐个比对，动态）
- **关联 AC**: AC-S-037
- **前置条件**:
  - 租户 `tenant-a` 已存在内置角色 `platform-admin`（`builtin = true`、`data_scope = ALL`）。
  - 注册表（`platform_permission` / `capabilities/permissions.json`）当前有若干权限点 —— **不写死条数**，一律按集合逐个比对。
  - `platform_role_permission` 中 `platform-admin` **不必**有人工补的授权行（集合动态得出）。
- **测试步骤**:
  1. 调 `GET /api/rbac/roles/{platform-admin id}/permissions`，取展开集合 S。
  2. 读注册表（`GET /api/rbac/permissions`），取出全部权限点编码集合 P。
  3. **逐个**比对 S 与 P（每个 P 中的编码都要在 S 中逐条点名核对；S 中不得有 P 之外的编码）。
  4. 在注册表里**新增**一个权限点（例如给 `modules/sandbox/manifest.json` 加一个测试权限点并同步写 `platform_permission`），刷新注册表后**再次**调 `GET /api/rbac/roles/{platform-admin id}/permissions`，取集合 S2。
  5. 查 `platform_role_permission` 中 `platform-admin` 的行数。
- **预期结果**:
  1. 接口返回 HTTP 200。
  2. P 为注册表当前全部权限点（**动态**读出，不在断言里写死个数）。
  3. S 与 P **逐元素相等**：P 中每个编码都在 S 中；S 中无 P 之外的编码。报告须列出逐条核对结果（缺失项为空时也要显式写「无缺失」）—— **不得**只比数量、不得抽样。
  4. S2 = P ∪ {第 4 步新增的那个权限点} —— 新增权限点后 `platform-admin` 的集合**自动多出**它；**不得**需要人工补 `platform_role_permission` 行。
  5. `platform_role_permission` 中 `platform-admin` 的行数**不随**第 4 步增加（若实现选择显式铺行，则第 4 步后行数 +1 且第 3 步的 S2 仍等于 P ∪ 新点 —— 两种实现都接受，但第 3 步的**集合相等**判定必须成立）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合相等断言（全量逐个比对，**明确禁止**抽样与写死基数）+ 动态性验证（注册表变更后自动跟随）

### TC-009-014: `platform-user` 不可删除且数据范围固定为 SELF
- **关联 AC**: AC-S-037
- **前置条件**:
  - 租户 `tenant-a` 已物化内置角色 `platform-user`（`builtin = true`、`data_scope = SELF`）。
- **测试步骤**:
  1. 调 `DELETE /api/rbac/roles/{platform-user id}`。
  2. 调更新入口把 `platform-user` 的 `dataScope` 改为 `ALL`。
  3. 调 `GET /api/rbac/roles/{platform-user id}`。
  4. 在租户 `tenant-b` 下重复第 1 步（确认保护**逐租户**生效，不是只保护租户 A 那一份）。
- **预期结果**:
  1. 第 1 步返回 HTTP 409/403，错误原因标明「内置角色受保护」。
  2. 第 2 步返回 HTTP 409/403，同一原因类别。
  3. 第 3 步返回 HTTP 200，`dataScope` 仍为 `SELF`（**未被改成 ALL**），`builtin` 为 `true`。
  4. 第 4 步在 `tenant-b` 同样被拒（租户 B 有自己物化的一份内置角色，同样受保护）。
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（第二个内置角色 / 第二个租户）+ 状态不变断言

---
