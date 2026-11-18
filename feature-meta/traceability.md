# REQ-009 追溯矩阵：AC-S-033 ~ AC-S-048

- **需求**: REQ-009「RBAC 权限与数据范围」
- **分支**: `feature/REQ-009`（基线 `develop` @ `400cefa`）
- **验收基线**: `BA/demands/REQ-009-rbac/acceptance.md`（2026-11-18 21:40 修订版）
- **契约**: `api-conventions v1`（C4 / C5 / C6 / C7）、`extension-points v2`（C-1 ~ C-5）
- **开发者**: myg133-dev-1
- **更新时间**: 2026-11-18

> 判据口径：测试名后的括号是「**测了什么**」，不是「跑没跑」。
> 所有真实库用例都在 `base_test` 库上跑（应用连接用**普通角色 `base_app`**）。

## 1. 矩阵

| AC | 实现位置 | 测试位置（用例名 → 断言要点） |
|----|----------|------------------------------|
| **AC-S-033**<br>注册表双向对账，不一致即启动失败 | `crates/platform-rbac/src/registry.rs`<br>（`collect_manifest_permissions` / `reconcile` / `require_consistent`）<br>`crates/platform-server/src/main.rs::reconcile_permissions`（**启动时**对账 → `Failure::Runtime` → 退出码非 0）<br>`migrations/20261118000901_rbac_permission_seed.up.sql`（库侧权限点由**迁移**写入） | `platform-rbac/tests/rbac_real_db.rs::the_registry_matches_the_manifests_and_a_difference_is_reported`（一致 → 通过；多一条 → 逐条报出；少一条 → **带来源 manifest 路径**；`require_consistent` 必须 **Err**）<br>`platform-rbac/src/registry.rs::tests::*`（4 条） |
| **AC-S-034**<br>角色/授权/绑定可用且带租户维度（★ 自举防护） | `crates/platform-rbac/src/service.rs`（角色 CRUD / 授权 / 绑定 / 内置角色）<br>`crates/platform-rbac/src/http.rs`（`/api/rbac/*`，**每个路由绑定管理权限点**）<br>`crates/platform-rbac/src/store.rs`（`{tenant}` 强制）<br>`migrations/20261118000900_rbac_schema.up.sql`（四张表都带 `tenant_id`）<br>`crates/platform-rbac/manifest.json`（4 个管理权限点） | `platform-rbac/tests/http_admin.rs::a_plain_user_cannot_reach_the_management_api`（普通用户写 / 读接口 → **403 `INSUFFICIENT_PERMISSION`** 且**不落库**；注册表查询**豁免** → 200）<br>`::a_manager_can_reach_the_management_api`（对照组 → 201 / 200）<br>`::a_role_of_another_tenant_is_not_found`（跨租户 id → **404**，与不存在**字段集合一致**，别人的数据没被删）<br>`platform-rbac/tests/rbac_real_db.rs::the_role_tables_are_tenant_scoped_and_rls_protected`（三张表都有 `tenant_id`；`pg_policies` **逐张列出**；换租户上下文看不到别租的行） |
| **AC-S-035**<br>角色继承展开且成环被拒 | `crates/platform-rbac/src/service.rs::detect_cycle` / `RoleGraph::ancestors` / `expand_user_permissions`<br>`crates/platform-rbac/src/error.rs::InheritanceCycle` | `platform-rbac/tests/rbac_real_db.rs::inheritance_expands_and_cycles_are_rejected`（传递展开；成环被拒且**指出是哪两个角色**）<br>`platform-rbac/src/service.rs::tests::*`（4 条：传递继承 / 管理员全集 / 成环 / 自继承） |
| **AC-S-036**<br>通配语义正确且禁止全通过 | `crates/platform-rbac/src/entity.rs::PermissionPattern::parse` / `expand`<br>`crates/platform-rbac/src/service.rs::grant_permission`（拒绝时写不进库） | `platform-rbac/tests/rbac_real_db.rs::wildcards_work_and_the_full_wildcard_is_rejected`（`sandbox:*` 覆盖本模块、`org:*` **不覆盖**；`*:*` 与 `*:demo:read` → `WILDCARD_FORBIDDEN`）<br>`platform-rbac/src/entity.rs::tests::*`（6 条） |
| **AC-S-037**<br>内置角色不可删除也不可降权 | `crates/platform-rbac/src/entity.rs::BUILTIN_ADMIN` / `BUILTIN_USER`<br>`crates/platform-rbac/src/service.rs::ensure_builtin_roles`（按租户物化）<br>`::delete_role` / `::update_role_scope` / `::grant_permission`（`builtin` → `BUILTIN_ROLE_PROTECTED`）<br>`::expand_user_permissions`（管理员 = **注册表全集**，动态） | `platform-rbac/tests/rbac_real_db.rs::the_built_in_roles_cannot_be_deleted_or_downgraded`（删 / 降权 / 减权**全被拒**；管理员**逐个**拥有注册表全集；普通用户**一个都没有**） |
| **AC-S-038**<br>数据范围五种语义求值正确 | `crates/platform-rbac/src/scope.rs::combine_scopes` / `ScopeResolver::resolve`<br>`crates/platform-rbac/src/checker.rs::load`（每个权限点的有效范围） | `platform-rbac/tests/rbac_real_db.rs::the_five_data_scopes_resolve_to_the_expected_record_sets`（**五个不同的探针用户**；每种 scope 的**期望记录 id 逐条列出**；五个集合**两两不相等**） |
| **AC-S-039**<br>兼岗默认按主部门，取并集必须显式开启 | `crates/platform-rbac/src/scope.rs::org_nodes`（默认只取 `is_primary` 的节点；`use_all_memberships` 才取全部成员节点）<br>`crates/platform-rbac/src/service.rs::contributions_for`（并集开关取自角色） | `platform-rbac/tests/rbac_real_db.rs::the_part_time_membership_defaults_to_the_primary_department`（默认 OWN_ORG / OWN_ORG_AND_BELOW **不含** dept-b、dept-b-1；显式开启后 = 主部门 ∪ 兼岗部门（含子树）） |
| **AC-S-040**<br>主部门缺失或歧义 → fail closed 到 `SELF` | `crates/platform-rbac/src/scope.rs::primary_department` + `org_nodes`（→ `OrgNodes::DegradeToSelf` → `self_filter`）<br>`migrations/20261118000900_rbac_schema.up.sql`：`user_org_one_primary_idx`（**预防层**） | `platform-rbac/tests/rbac_real_db.rs::a_missing_or_ambiguous_primary_department_falls_back_to_self`（① 第二个主部门被**数据库拒绝**；② 主部门缺失 → 只看到本人创建的记录；③ **绕过索引**造出歧义 → **同样**退化为 `SELF`） |
| **AC-S-041**<br>数据范围优先级与多角色组合 | `crates/platform-rbac/src/service.rs::contributions_for`（**权限点级 scope 覆盖角色级**）<br>`crates/platform-rbac/src/scope.rs::combine_scopes`（`ALL` 最宽松 / `OWN_ORG` ⊂ `OWN_ORG_AND_BELOW` / `SELF` 最严 / `CUSTOM` 取自己那一路） | `platform-rbac/tests/rbac_real_db.rs::multiple_roles_take_the_most_permissive_scope`（普通读 → 最宽松；export → 权限点级 `SELF`；解绑 R3 → 退为 `OWN_ORG`，**与第 1 步不同** → 证明是并集不是交集） |
| **AC-S-042**<br>受限表达式 DSL 拒绝越界写法且不产生拼接 SQL | `crates/platform-core/src/extension/expr.rs`（词法 / 语法 / 白名单 / 编译）<br>`crates/platform-db/src/query.rs::with_condition`（参数绑定） | `platform-core/src/extension/expr.rs::tests::*`（10 条：合法式**只含占位符**、白名单外字段、函数调用、多语句、注入串**当普通值**、类型错配、空数组、裸标识符、未闭合字符串、非法列名）<br>`platform-db/src/query.rs::tests::*`（范围条件插进 WHERE、字面值不出现、占位符数量不符被拒） |
| **AC-S-043**<br>属性级校验（RBAC + ABAC 混合）生效 | `crates/platform-core/src/extension/manifest.rs::PermissionSpec::condition`（**可选**字段）<br>`crates/platform-rbac/src/checker.rs::load`（条件编译；白名单外 → **永不匹配** = fail closed）<br>`modules/sandbox/manifest.json`（`sandbox:scope-item:approve` 携带 `$record.amount <= 10000`）<br>`modules/sandbox/backend/src/http.rs::approve_scope_item`（参考实现） | `platform-core/src/extension/expr.rs::tests::*`（条件解析 / 编译 / 白名单）<br>`modules/sandbox/backend/src/http.rs`（approve 端点：范围外 → 404，条件不满足 → 403）<br>`platform-rbac/src/checker.rs`（条件随注册表装载） |
| **AC-S-044**<br>鉴权中间件的三种拒绝语义与错误体形状 | `crates/platform-core/src/extension/permission.rs::require_permission`（**唯一入口**）<br>`crates/platform-core/src/problem.rs`（RFC 9457 错误体，**唯一一份**） | `platform-core/tests/authz_entry.rs`（7 条：401 `UNAUTHENTICATED` / 403 `INSUFFICIENT_PERMISSION` / 放行 / **判定实现缺失 → fail closed** / `detail` 英文 ASCII / `trace_id` 必带 / `instance` 是请求路径 / 合并子路由与 `with_state` 后鉴权层仍生效）<br>`modules/sandbox/tests/http_authz.rs::a_request_without_a_token_is_unauthenticated` / `::a_user_without_the_permission_is_forbidden` / `::a_record_outside_the_data_scope_is_not_found` |
| **AC-S-045**<br>404 不泄露资源存在性 | `modules/sandbox/backend/src/scope_item.rs`（范围过滤后查不到 → 当不存在）<br>`modules/sandbox/backend/src/http.rs::get_scope_item`（404 `NOT_FOUND`） | `modules/sandbox/tests/http_authz.rs::an_unreadable_record_and_a_missing_record_are_indistinguishable`（无权资源 vs 不存在资源：**状态码相同**、**错误体字段集合相同**、**响应头集合相同**；对照组：范围内的记录 → 200） |
| **AC-S-046**<br>已登记权限点必须有鉴权调用点，且入口唯一 | `crates/platform-contracts/src/authz_scan.rs`（**扫调用**，不是扫字面量；剥注释 + 剥字符串内容）<br>`crates/platform-contracts/src/checks.rs::check_c5`<br>`crates/platform-contracts-macros/src/lib.rs`（生成 **5** 项契约测试） | `modules/sandbox/tests/contracts.rs::contract_c5_declared_permissions_have_authorization_call_sites`<br>`platform-contracts/src/authz_scan.rs::tests::*`（8 条：字面量调用 / 常量调用被拒 / **常量不是调用点** / 自建入口被报 / 注释不是调用点 / 方法调用不算 / 定义检测）<br>**破坏性验证**：4 类违规逐个造出来并确认被拦住（见 `verification-report.md` §4） |
| **AC-S-047**<br>权限撤销在下一个请求即生效，不等 TTL | `crates/platform-rbac/src/checker.rs::ensure_fresh`（epoch 不一致 → 重算）<br>`crates/platform-rbac/src/store.rs::current_epoch` / `bump_epoch`（**写操作同事务** epoch+1）<br>`crates/platform-rbac/src/service.rs`（所有写路径都 bump） | `platform-rbac/tests/rbac_real_db.rs::a_revocation_takes_effect_on_the_next_request`（**两个独立实例**：实例 1 撤销 → 实例 2 的下一个请求即被拒）<br>`::a_revoked_data_scope_takes_effect_on_the_next_request`（**撤销数据范围**同样立即生效） |
| **AC-S-048**<br>数据范围过滤在 SQL 层生效且业务模块无法绕过 | `crates/platform-db/src/query.rs::with_scope` / `with_condition`（条件插进 WHERE 子句 + 参数绑定）<br>`crates/platform-core/src/extension/permission.rs::ScopeFilter`（**没有「跳过范围」的变体**）<br>`modules/sandbox/backend/src/scope_item.rs`（业务模块只提供范围条件） | `platform-db/src/query.rs::tests::*`（渲染出的 SQL **含** `org_node_id = ANY($n)`、**不含**字面值）<br>`modules/sandbox/tests/http_authz.rs::the_data_scope_is_applied_in_the_sql_layer`（SQL 文本含范围条件、值走绑定、结果确实被过滤、**平台构造器上没有跳过范围的入口**）<br>`platform-rbac/tests/rbac_real_db.rs::the_scope_filter_is_rendered_into_the_sql` |

## 2. 真实库用例（`base_test`，应用角色 `base_app`）

| 用例文件 | 条数 | 覆盖 AC |
|----------|------|----------|
| `crates/platform-rbac/tests/rbac_real_db.rs` | 12 | AC-S-033 / 034 / 035 / 036 / 037 / 038 / 039 / 040 / 041 / 047 / 048 |
| `crates/platform-rbac/tests/http_admin.rs` | 3 | AC-S-034（★ 自举防护、跨租 404） |
| `modules/sandbox/tests/http_authz.rs` | 6 | AC-S-044 / 045 / 048 |
| `modules/sandbox/tests/c3_real_db.rs`（既有） | 4 | 租户隔离（语义未改动） |

## 3. 纯逻辑用例（不连库）

| 用例文件 | 条数 | 覆盖 AC |
|----------|------|----------|
| `crates/platform-core/src/extension/expr.rs::tests` | 10 | AC-S-042 / 043 |
| `crates/platform-core/src/extension/permission.rs::tests` | 4 | AC-S-044 |
| `crates/platform-core/tests/authz_entry.rs` | 7 | AC-S-044 |
| `crates/platform-db/src/query.rs::tests`（整 crate） | 34 | AC-S-048 |
| `crates/platform-rbac/src/{entity,scope,service,registry}.rs::tests`（整 crate） | 33 | AC-S-035 / 036 / 037 / 038 / 039 / 040 / 041 |
| `crates/platform-contracts/src/authz_scan.rs::tests` | 8 | AC-S-046 |
| `modules/sandbox/tests/contracts.rs`（C-1 ~ C-5） | 5 | AC-S-046 |

## 4. 权限点 → 鉴权调用点对照（BA 要求逐条列出）

| 权限点 | 鉴权调用点 | 覆盖的路由 |
|--------|-----------|-----------|
| `rbac:role:read` | `crates/platform-rbac/src/http.rs`（读路由组） | `GET /api/rbac/roles`、`GET /api/rbac/roles/{id}`、`GET /api/rbac/roles/{id}/permissions` |
| `rbac:role:write` | `crates/platform-rbac/src/http.rs`（写路由组） | `POST/DELETE /api/rbac/roles`、`POST /api/rbac/roles/{id}/permissions`、`DELETE /api/rbac/roles/{id}/permissions/{code}`、`POST/DELETE /api/rbac/roles/{id}/inherit` |
| `rbac:user-role:write` | `crates/platform-rbac/src/http.rs`（绑定路由组） | `POST /api/rbac/users/{user_id}/roles`、`DELETE /api/rbac/users/{user_id}/roles/{role_id}` |
| `rbac:permission:read` | **无调用点** —— 对应 `GET /api/rbac/permissions`，按 BA 裁定**豁免**管理权限点 | `GET /api/rbac/permissions`（任何已认证用户可读） |
| `sandbox:demo:read` | `modules/sandbox/backend/src/http.rs` | `GET /api/sandbox/demo` |
| `sandbox:demo:write` | `modules/sandbox/backend/src/http.rs` | `POST /api/sandbox/demo` |
| `sandbox:scope-item:read` | `modules/sandbox/backend/src/http.rs` | `GET /api/sandbox/scope-item`、`GET /api/sandbox/scope-item/{id}` |
| `sandbox:scope-item:approve` | `modules/sandbox/backend/src/http.rs` | `POST /api/sandbox/scope-item/approve` |
| `sandbox:scope-item:export` | `modules/sandbox/backend/src/http.rs` | `POST /api/sandbox/scope-item/export` |

> `rbac:permission:read` 是**唯一**「登记了但没有调用点」的权限点 ——
> 原因是 BA 明确裁定「注册表查询豁免管理权限点」（否则前端 `can()` 拿不到注册表）。
> 它**不是**假权限：该接口对任何已认证用户开放。

### `platform-org` 的 9 个权限点（**如实列出，本期未改**）

| 权限点 | 有无 `require_permission` 调用点 | 说明 |
|--------|----------------------------------|------|
| `org:org-node:read` / `org:org-node:write` | **无** | `crates/platform-org/src/http.rs` 只有**令牌校验**（`Authenticated` 提取器），**没有权限点判定** |
| `org:user-org:read` / `org:user-org:write` | **无** | 同上 |
| `org:user-position:read` / `org:user-position:write` | **无** | 同上 |
| `org:sync-conflict:read` / `org:sync-conflict:resolve` | **无** | 同上 |
| `org:scim-sync:run` | **无** | 同上 |

> ⚠️ **这 9 个权限点属于「登记了但从不鉴权」** —— 正是 `extension-points v1` §4 要关掉的那类
> **假权限**。本期（REQ-009）**不修**（那是 REQ-008 的范围），在此如实登记，
> 由 BA 决定处理方式（要么补鉴权，要么从 manifest 撤下）。
> 本期新增的 **C-5 只扫业务模块**（`modules/*/backend/**/*.rs`），
> 因此**不会**让 `platform-org` 的契约测试变红 —— 但这也意味着
> **平台 crate 的权限点目前没有机械门禁**（见 `verification-report.md` §7 的建议）。
