# REQ-009 验证报告（自验）

- **需求**: REQ-009「RBAC 权限与数据范围」
- **分支**: `feature/REQ-009`（基线 `develop` @ `400cefa`）
- **验收基线**: `BA/demands/REQ-009-rbac/acceptance.md`（2026-11-18 21:40 修订版，含 BA 第二轮 10 条裁定）
- **契约**: `api-conventions v1`（C1 / C4 / C5 / C6 / C7）、`extension-points v2`（C-1 ~ C-5）
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: 第一轮（实现 + 自验）→ **第二轮（定点修复：多角色 scope 的合并语义改成真正的并集，见 §8）**

---

## 0. 一页速览

| 项 | 结果 |
|----|------|
| 交付形态 | 新建平台 crate **`crates/platform-rbac`**（10 个源文件 + 3 个测试文件）；新增 `platform-core` 的 `extension/permission.rs`（鉴权入口）、`extension/expr.rs`（受限表达式 DSL）、`problem.rs`（RFC 9457 错误体唯一一份）；新增契约检查 **C-5**；`platform-server` 挂载 `/api/rbac/*` 并在**启动时**对账 |
| 改动规模 | **60 文件 / +11653 −675 行**（新建 29 个、修改 31 个；删除的 675 行是**替换掉 REQ-008 遗留的** `feature-meta/traceability.md` 与 `verification-report.md`） |
| 依赖预算 | **第三方包数不变**：锁文件里 **registry 包 284 → 284**（总数 293 → 294，多出的 1 个是**新增的本地工作区成员** `crates/platform-rbac`）。未引策略引擎（Casbin/Opa）、未引 Redis 客户端、未引 nom/pest；表达式解析器自研；`tower` / `http-body-util` 已在锁文件里（只做 **dev-dependency**，**不增长依赖树**） |
| **测试** | `scripts/test-integration.sh`（重置 `base_test` + `--include-ignored`）→ **452 passed / 0 failed**（含真实库层）；`cargo clippy --workspace --all-targets -- -D warnings` → **0 warning** |
| **破坏性验证** | C-5 的 **4 类违规**逐个造出来 → 全部被拦住 → **逐字节还原**（sha256 校验） |
| **变异验证** | 第一轮 **4 个** + 第二轮 **2 个**关键判定故意改错 → 对应测试**变红** → 还原（sha256）→ **变绿**（含 1 次「变异没被抓住 → 补断言 → 再验」） |
| **未验证项** | 见 §6（**如实列出**，未跑的没写成跑了） |

---

## 1. 实现了什么（按 AC）

| AC | 落在哪 | 关键口径 |
|----|--------|----------|
| AC-S-033 | `platform-rbac/src/registry.rs` + `platform-server/src/main.rs::reconcile_permissions` | 对账不一致 → **启动失败**（退出码 1），逐条列出缺失编码与**来源 manifest 路径** |
| AC-S-034 | `platform-rbac/src/{service,http,store}.rs` + 迁移 `20261118000900` | 四张表都带 `tenant_id` + RLS；**★ 自举防护**：管理接口要 `rbac:role:*`，普通用户 403；**豁免** `GET /api/rbac/permissions` |
| AC-S-035 | `service.rs::detect_cycle` / `RoleGraph::ancestors` | 继承**查询时展开**；成环的写入被拒，错误**指出是哪两个角色** |
| AC-S-036 | `entity.rs::PermissionPattern` | 只允许 `module:*` 与 `module:resource:*`；`*:*` 与 `*:demo:read` **写入被拒** |
| AC-S-037 | `entity.rs::BUILTIN_*` + `service.rs` | 内置角色按租户物化；不可删 / 不可降权；`platform-admin` = **注册表全集**（动态） |
| AC-S-038 | `scope.rs::combine_scopes` / `ScopeResolver` | 五种 scope × **五个不同探针用户**，期望集合逐条列出 |
| AC-S-039 | `scope.rs::org_nodes` | 兼岗**默认只按主部门**；并集必须 `use_all_memberships = true` |
| AC-S-040 | `scope.rs::primary_department` + `user_org_one_primary_idx` | 缺失 / 歧义 → **fail closed 到 `SELF`**（**不是** `ALL`）；DB 索引是**预防层**，判定是**防御层** |
| AC-S-041 | `service.rs::contributions_for` + `scope.rs::combine_scopes` | 多角色**取最宽松**；权限点级 scope **覆盖**角色级 |
| AC-S-042 | `platform-core/src/extension/expr.rs` | 白名单 + 参数化 + 禁函数调用 + 禁多语句；SQL 文本里**只有占位符** |
| AC-S-043 | `manifest.rs::PermissionSpec::condition` + `checker.rs` | 条件引用白名单外属性 → **永不匹配**（fail closed），不是「忽略条件放行」 |
| AC-S-044 | `platform-core/src/extension/permission.rs` + `problem.rs` | 401 / 403 / 404 三种语义；错误体**英文 ASCII** + `trace_id` + `instance` |
| AC-S-045 | `sandbox/backend/src/{scope_item,http}.rs` | 无权资源 vs 不存在资源：**状态码 / 字段集合 / 响应头集合全同** |
| AC-S-046 | `platform-contracts/src/authz_scan.rs` + `checks.rs::check_c5` + 宏 | 扫的是**调用**（不是字面量）；入口**全仓库唯一**；参数**必须字面量** |
| AC-S-047 | `checker.rs::ensure_fresh` + `store.rs::bump_epoch` | 写操作同事务 `epoch+1` → 撤销在**下一个请求**即生效（**跨实例**） |
| AC-S-048 | `platform-db/src/query.rs::with_scope` | 条件**插进 WHERE 子句** + 参数绑定；**平台没有「跳过范围」的入口** |

追溯矩阵见 `feature-meta/traceability.md`（含**权限点 → 鉴权调用点**逐条对照，以及 `platform-org` 9 个权限点的现状）。

---

## 2. 跑过的命令与**实际输出里的 passed 数**

> ⚠️ 每条命令都**只有一个 `--`**（AGENTS.md 硬规则 9）。
> 下面每个数字都来自命令输出里的「N passed」，**不是**估算。

### 2.1 集成测试（重置 `base_test` + 跑真实库层）

```bash
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
  ./scripts/test-integration.sh
```

| 测试二进制 | passed |
|-----------|--------|
| `platform-contracts`（lib） | 15 |
| `platform-core`（lib） | 72 |
| `platform-core`（`tests/authz_entry.rs`） | 7 |
| `platform-db`（lib） | 35 |
| `platform-db`（`tests/quota_real_db.rs`） | 2 |
| `platform-iam`（lib） | 41 |
| `platform-iam`（`tests/iam_real_db.rs`） | 16 |
| `platform-org`（lib） | 68 |
| `platform-org`（`tests/org_real_db.rs`） | 15 |
| `platform-org`（`tests/scim_outbound.rs`） | 7 |
| **`platform-rbac`（lib）** | **39** |
| **`platform-rbac`（`tests/rbac_real_db.rs`，真实库）** | **18** |
| **`platform-rbac`（`tests/http_admin.rs`，真实库）** | **3** |
| `platform-server`（lib） | 38 |
| `base`（bin） | 9 |
| `sandbox`（lib） | 3 |
| `sandbox`（`tests/c3_real_db.rs`，真实库） | 4 |
| `sandbox`（`tests/contracts.rs`，C-1 ~ C-5） | 5 |
| **`sandbox`（`tests/http_authz.rs`，真实库）** | **6** |
| `xtask`（lib） | 49 |
| **合计** | **452 passed / 0 failed** |

> 第二轮（定点修复）的增量：**+13** = 真实库层 **+6**（§8.5）、
> `platform-rbac` 单测 **+6**（§8.5）、`platform-db` 单测 **+1**（§8.4）。

### 2.2 其它门禁

| 命令 | 结果 |
|------|------|
| `cargo clippy --workspace --all-targets -- -D warnings` | **0 warning** |
| `cargo fmt --all --check` | 通过（提交前已 `cargo fmt --all`） |
| `cargo xtask check-modules` | `模块目录校验通过：1 个模块` |
| `cargo xtask check-deps` | `依赖方向校验通过：平台 crate 未反向依赖业务模块` |
| `cargo xtask capabilities` | 生成 13 个产物（`permissions.json` 带 `condition`、`extension-points.json` 的 `contract` = **v2**、`extension_point_count` 仍为 **5**） |
| `scripts/sync-contracts.sh` | `快照一致：docs/contracts/api-conventions/v1.md` + `docs/contracts/extension-points/v2.md`（**exit 0**） |
| `cargo test -p platform-core --test authz_entry` | 7 passed |
| `cargo test -p sandbox --test contracts` | 5 passed（C-1 ~ C-5，含新增 C-5） |
| `cargo test -p xtask` | 49 passed |
| `cargo xtask create-module scaffold-probe` + `cargo test -p scaffold-probe` | **5 passed**（脚手架生成的模块**开箱即过 C-1 ~ C-5**；验证后已删除该探针模块） |

### 2.3 稳定性（**回归不稳定**是本仓的历史问题，本轮专门盯了）

`http_admin` 首轮出现过**偶发死锁**（`ALTER TABLE ... ENABLE ROW LEVEL SECURITY` 的
AccessExclusiveLock 与另一个用例的 INSERT 的 RowShareLock 互等）。
**根因已定位并修掉**（夹具只在策略缺失时才执行 DDL），修后连跑 **6 次全绿**，
完整集成套件连跑 **2 次全绿**（439 / 439）。

---

## 3. 破坏性验证（造违规 → 确认被拦住 → 逐字节还原）

> 只写「正常路径通过」的测试**不算数**。每条违规都是**真的造出来**跑一遍的。

### 3.1 契约检查 C-5（`modules/sandbox`，每次都还原 + sha256 校验）

| # | 造出来的违规 | 实际输出 | 还原 |
|---|-------------|---------|------|
| 1 | 把 `export` 路由的 `route_layer(require_permission("sandbox:scope-item:export"))` 去掉（**登记了但没有调用点**） | `the permission point 'sandbox:scope-item:export' is declared in manifest.json but has no authorization call site` → **FAILED** | ✅ sha256 一致 |
| 2 | 在模块里自写一个鉴权函数 `fn check_perm(code: &str) -> bool`（**绕过平台入口**） | `the custom authorization entry point at .../http.rs:10922 bypasses the platform entry point 'require_permission'` → **FAILED** | ✅ sha256 一致 |
| 3 | 把调用点的字面量换成常量 `require_permission(crate::permission::PERMISSION_SCOPE_ITEM_READ)`（**无法静态确认**） | `the authorization call site at .../http.rs:4525 does not use a string literal; it cannot be verified statically` → **FAILED** | ✅ sha256 一致 |
| 4 | 在另一个模块里加 `pub fn require_permission(...)`（**第二个入口定义**） | `the authorization entry point must be defined exactly once in the whole repository, found 2: [.../platform-core/.../permission.rs, .../sandbox/backend/src/rogue.rs]` → **FAILED** | ✅ sha256 一致（rogue.rs 已删除） |

还原后 `cargo test -p sandbox --test contracts` → **5 passed**。

### 3.2 其它「应当被拦住」的机制（断言里就是「被拦住」）

| 机制 | 判据 | 断言位置 |
|------|------|---------|
| `*:*` / `*:demo:read` 通配 | 写入被拒（`WILDCARD_FORBIDDEN`） | `rbac_real_db.rs::wildcards_work_and_the_full_wildcard_is_rejected` |
| 内置角色删除 / 降权 / 减权 | 三个操作全被拒（`BUILTIN_ROLE_PROTECTED`） | `rbac_real_db.rs::the_built_in_roles_cannot_be_deleted_or_downgraded` |
| 角色继承成环 | 被拒 + **指出是哪两个角色** | `rbac_real_db.rs::inheritance_expands_and_cycles_are_rejected` |
| 第二个主部门写入 | **数据库**拒绝（部分唯一索引） | `rbac_real_db.rs::a_missing_or_ambiguous_primary_department_falls_back_to_self` |
| 白名单外字段 / 函数调用 / 多语句 | **解析失败**并给出明确原因 | `expr.rs::tests::*` |
| 注入串 | 当作**普通字符串值**绑定，SQL 文本里不出现 | `expr.rs::tests::an_injection_string_is_compiled_as_a_plain_value` |
| 非法范围列名 | **拒绝**（列名会被拼进 SQL） | `query.rs::tests::an_unsafe_scope_column_is_rejected` |
| 未认证 / 缺权限点 / 判定实现缺失 | 401 / 403 / **fail closed**（不是放行） | `authz_entry.rs`（5 条） |
| 普通用户调 `/api/rbac/*` | 403 且**不落库** | `http_admin.rs::a_plain_user_cannot_reach_the_management_api` |

---

## 4. 变异验证（把关键判定**故意改错** → 测试必须变红）

> 判据：改错 → **对应测试红** → 还原（sha256 校验）→ **对应测试绿**。

| # | 故意改错的地方 | 期望变红的测试 | 实测 |
|---|--------------|--------------|------|
| 1 | **fail closed → fail open**：主部门缺失 / 歧义时退化成 `ALL`（`scope.rs` 的 `DegradeToSelf` 分支） | `a_missing_or_ambiguous_primary_department_falls_back_to_self` | ✅ **FAILED**（`主部门缺失必须退化为 SELF（不得看到本部门或全租户数据）`），还原后 12 passed |
| 2 | **兼岗默认从「主部门」改成「并集」**（`scope.rs` 的 `org_nodes`） | `the_part_time_membership_defaults_to_the_primary_department` + `a_missing_or_ambiguous_...` | ✅ **FAILED**（2 条），还原后 12 passed |
| 3 | **鉴权入口静默放行**（`permission.rs` 的 `has` 判断短路成 `false &&`） | `platform-core/tests/authz_entry.rs`（3 条）+ `sandbox/tests/http_authz.rs`（1 条） | ✅ **FAILED**（3 + 1），还原后 7 passed / 6 passed |
| 4 | **对账不一致只告警不失败**（`registry.rs` 的 `require_consistent` 返回 `Ok`） | `the_registry_matches_the_manifests_and_a_difference_is_reported` | ⚠️ **第一次没变红** → 补了断言（`require_consistent` 必须 **Err** + 差异逐条列出）→ **第二次变红** ✅，还原后 12 passed |

> **第 4 条是本轮最有价值的发现**：原用例只测了 `reconcile()`（它本来就报差异），
> **没测启动路径**（`require_consistent()`）—— 也就是说「不一致即启动失败」这条
> **一度没有测试保护**。补断言后，变异能被抓住。这是「变异验证」真正的价值：
> **它发现的是测试的漏洞，不是代码的漏洞。**

---

## 5. 顺带修掉的既有缺陷（BA 点名的「开工前必须先修」）

| 缺陷 | 修法 | 验证 |
|------|------|------|
| `feature-meta/manifest.json` **缺 `contracts` pin** → `scripts/sync-contracts.sh` **恒 exit 2** → 契约门禁在 CI 上**常红** | 补 `"contracts": { "api-conventions": "v1", "extension-points": "v2" }` | `scripts/sync-contracts.sh` → **exit 0**（此前恒 2） |
| `docs/contracts/extension-points/v2.md` 快照缺失 | `scripts/sync-contracts.sh --sync`（**不是手改快照**） | 校验一致 ✅ |
| 脚手架生成的模块**过不了 C-5**（声明了权限点却没有调用点） | 脚手架模板加 `backend/src/http.rs`（三个权限点各一个调用点）+ `condition` 示例 | `cargo xtask create-module scaffold-probe` → **5 passed**（含 C-5） |

---

## 6. 未验证项（**如实列出**）

| 项 | 状态 | 原因 / 补法 |
|----|------|-----------|
| **真机 `base serve` 的启动对账**（AC-S-033 第 1/2 步要求「启动进程 → 记退出码」） | ⚠️ **未跑真机进程** | 已验证的是**对账函数**（`require_consistent` 必须 Err + 逐条列出差异）与**启动路径的接线**（`platform-server/src/main.rs::reconcile_permissions` → `Failure::Runtime` → 退出码 1）。**没有**真的起进程注入差异看退出码 —— 需要一套 IAM 环境变量 + 迁移后的库，本机未搭。**建议 QA 补跑** |
| **多实例真机部署下的 epoch 收敛**（AC-S-047 的跨进程口径） | ⚠️ **用两个独立 checker 实例模拟**，不是两个真机进程 | 两个实例**各自持自己的缓存、共享同一个数据库**（满足「跨进程」的判定要点）；**没有**起两个 `base serve` 进程。BA 的 AC §9 指出单实例部署下无法证伪 —— 真机双实例需部署侧配合 |
| **`platform-org` 的 9 个权限点** | ⚠️ **未修**（按 BA 指示不顺手改） | 现状：**登记了但 `require_permission` 调用点一个都没有**（`crates/platform-org/src/http.rs` 只有令牌校验）。属 REQ-008 范围。已如实登记在 `traceability.md` §4 |
| **C-5 扫平台 crate** | ⚠️ **未做**（BA 说「不要求扩 C-5 的扫描范围」） | C-5 目前只扫 `modules/*/backend/**/*.rs`；`platform-rbac` 的 4 个权限点**逐条列了调用点**（`traceability.md` §4），但**没有机械门禁**。建议后续把 C-5 的扫描范围扩到 `crates/*`（否则平台 crate 的假权限仍无门禁） |
| **CI 真跑**（6 个 job / 镜像体积门禁 / Trivy） | ⚠️ **跑不了** | 本仓**无 remote**。本地已跑：`cargo test --workspace`、`cargo clippy -- -D warnings`、`cargo fmt`、`cargo xtask check-modules`、`cargo xtask check-deps`、`cargo xtask capabilities`、`scripts/sync-contracts.sh`、`scripts/test-integration.sh` |
| **前端 `can()`** | ❌ **不在本期** | 属 REQ-011。本期只保证「注册表有接口可读」+ 能力索引里带 `condition` |
| **覆盖率数字** | ❌ 不设门槛 | 本期不设覆盖率门槛（与既有需求一致） |
| **真实租户的权限点数据** | ⚠️ 只有夹具 | 夹具覆盖了 AC-S-034 的跨租对照（两个租户各自的角色行 + RLS 隔离断言）；**没有**真的建第二个租户并跑完整业务流 |

---

## 7. 对规格的疑问（**按现有规格实现**；有分歧的地方已说明取舍）

| # | 疑问 | 本期实现 | 影响 |
|---|------|---------|------|
| 1 | **design-summary §4.4**：「`CUSTOM` 与其它 scope 同时出现时**取 `CUSTOM` 的那一路**」—— 多个角色的 scope 是**并集**语义；按并集，`CUSTOM` 与 `OWN_ORG` 应当是「**或**」（更宽），而「取 CUSTOM 那一路」比并集**窄**、比「两者都满足」**宽** | **照规格实现**（`combine_scopes`：`CUSTOM` 存在时只走 CUSTOM；多个 CUSTOM 取**交集**） | ⚠️ **安全方向的疑问**：若某用户既有 `OWN_ORG`（本部门）又有 `CUSTOM`（如 `$record.amount <= 10000`）两个角色，按「取 CUSTOM 那一路」，他会看到**本部门之外**满足金额条件的记录 —— 比「本部门」**宽**。这不是越权（两个角色都是管理员授的），但**比字面的「本部门」宽**。若 BA 认为应当更严（`AND` 语义），改 `scope.rs::combine_scopes` 一处即可（**本次未擅自改设计**） |
| 2 | **`PermissionChecker` 的 trait 形状**：design §1 给的是**两个方法**（`has` + `scope`），但 §5.3 的属性级校验与 §4.3 的权限点级覆盖需要更多通道 | **按设计的两个方法实现**，另加 `effective_scope` 与 `condition` 两个方法（共 4 个，仍**全同步** → dyn 兼容） | 加的是**能力面**，没有削弱任何安全属性；`PermissionChecker` **不属于** `extension-points v2` 冻结的 5 个 trait，改它**不违反**契约 |
| 3 | **`platform_permission` 是平台级表**（无 `tenant_id`），但 `TenantQuery` 强制每条语句都要有 `{tenant}` | 语句写成 `WHERE {tenant} IS NOT NULL`（恒真，租户照样绑定） | 保持了「`{tenant}` 是**所有**语句的硬约束」这条不变量；没有为平台级表开特例 |
| 4 | **`rbac:permission:read` 登记了却没有调用点** | 按 BA 裁定**豁免** `GET /api/rbac/permissions` 的管理权限点 | 它是**唯一**「登记了但无调用点」的权限点；不是假权限（该接口对任何已认证用户开放） |
| 5 | **AC-S-045 前置条件的自相矛盾**（BA 已在 §9 提过） | 按 BA 第二轮裁定实现：**无权资源 vs 不存在资源**必须无法区分；**有权资源 → 200** | 夹具里可见记录（`dept-a-2`）与不可见记录（`dept-b`）落在**不同部门分支** |

> **第 1 条已由 BA 裁定并在本轮修复** → 见 §8（多角色 scope 的合并语义改成**真正的并集**）。
> 下面第 1 行保留第一轮的原样，作为「当时按规格实现」的记录。

---

## 8. 第二轮：定点修复 —— 多角色 scope 的合并语义改成**真正的并集**

> 触发：BA 裁定了 §7 的第 1 条疑问。**实现是忠实的，错的是规格**（design-summary §4.4 的笔误）。
> 本轮只做这一处定点修复 + 两个「顺带确认」，不夹带别的需求。

### 8.1 错在哪：旧语义既不是并集也不是交集

design-summary §4.4 原文：

> `CUSTOM` 不参与宽松比较：与其它 scope 同时出现时，**取 `CUSTOM` 的那一路**

按这条实现出来的效果是：**多角色里有 `CUSTOM` 时，其它角色（`OWN_ORG` / `OWN_ORG_AND_BELOW`）
的贡献被整个丢掉**。后果：

- 用户同时有「本部门」角色和一个「CUSTOM 条件」角色时，
  **本部门里不满足 CUSTOM 条件的记录反而看不见了** —— **加一个角色缩小了可见范围**；
- 这与同节 §4.3「多个角色的 scope **取最宽松的**（并集语义）—— 与权限点『任一满足即通过』一致」
  直接矛盾；
- 多个 `CUSTOM` 之间取**交集**（原 §4.4 的另一条）同样与并集原则矛盾 → **一并作废**。

### 8.2 BA 裁定后的新规则（实现按此）

| 情况 | 结果 |
|------|------|
| 任一角色是 `ALL` | `ALL`（最宽松，其他都不必看） |
| 否则有 `CUSTOM` 角色 | **CUSTOM 的条件 OR 其它（非 CUSTOM）角色的最宽松范围** |
| 否则有 `OWN_ORG_AND_BELOW` | `OWN_ORG_AND_BELOW`（最宽松） |
| 否则有 `OWN_ORG` | `OWN_ORG` |
| 否则 | `SELF` |

多个 `CUSTOM` 角色：条件之间取 **OR**（并集），**不是 AND**。

### 8.3 改动落点

| 文件 | 改动 |
|------|------|
| `crates/platform-rbac/src/scope.rs` | `CombinedScope::Custom` 增加 `plus: Option<ContainmentScope>`；`combine_scopes` 改成并集；`ScopeResolver` 把包含型范围与自定义条件合成**一个** OR 组 |
| `crates/platform-rbac/src/checker.rs` | 抽出 `compile_condition`（编译失败 → 永不匹配）+ 3 条单测 |
| `crates/platform-core/src/extension/permission.rs` | **平台契约的最小演进**：`ScopeFilter` 增加 `CustomOr { conditions, scope }`（理由见 §8.4） |
| `crates/platform-db/src/query.rs` | 渲染 `CustomOr`：**一个**括号包起来的 OR 表达式；`Custom` 的多个条件由 AND 改为 OR |
| `crates/platform-rbac/tests/rbac_real_db.rs` | 新增 **6** 条真实库用例 + 1 个新夹具（`visible_ids_with_condition`） |

**没有**改 BA 的任何文件；**没有**改 `platform-core` 以外的平台契约面（`PermissionChecker` 的
四个方法签名、`DataScope` 五种语义、`require_permission` 入口都**原样**）。

### 8.4 为什么必须动 `platform-core`（**不是**顺手扩平台，是契约表达能力的缺口）

BA 的渲染要求是「整体必须是**一个**括号包起来的 OR 表达式，且**所有值走参数绑定**」，
同时「`OWN_ORG_AND_BELOW` 的子树 id 集合是**数组绑定**，与 OR 组合时**不要**把数组展开成字符串拼接」。
这两条在旧契约下**无法同时满足**：

| 障碍 | 事实 |
|------|------|
| `ScopeFilter` 没有能表达「OR」的变体 | 四个变体（`All` / `OrgNodes` / `Owner` / `Custom`）各自渲染成**独立**的 AND 条件；`Custom` 的多个条件之间也是 AND |
| 自定义条件的载体装不下数组 | `CompiledCondition.values` 的类型是 `ConditionValue`（`Text` / `Int` / `Bool` / `Float`），**没有数组** → `org_node_id = ANY($n)` 根本无法放进 `CompiledCondition` → 也就无法与自定义条件拼成同一个 OR 表达式 |
| 展开数组被明确禁止 | 把节点集合拼成 `IN ('dept-a', 'dept-b')` 违反「值走参数绑定」，也不许做 |

所以契约必须**恰好长一个变体**：

```rust
/// 自定义条件 **OR** 一个包含型范围（**并集**语义）。
CustomOr {
    conditions: Vec<CompiledCondition>,   // 彼此 OR
    scope: Box<ScopeFilter>,              // 另一侧：OrgNodes / Owner
}
```

- 落在 `platform-core`（`ScopeFilter` 的定义处）→ 属 AGENTS.md 硬规则 1 的**例外条款**
  （「平台 crate 可以在**已评审的需求**里演进」，REQ-009 已在该文件新增过 `extension/permission.rs`）；
- 改动是**纯增量**：既有四个变体的渲染行为不变；只有 `Custom` 的「多条件之间」由 AND 改为 OR ——
  那是**本轮语义修正的一部分**（且只有 `platform-rbac` 生产 `ScopeFilter`，业务模块只消费）；
- `AGENTS.md` **不需要**改：没有新增 crate / 文件 / 能力项，它对 `platform-core` 的描述仍然成立；
- 能力索引 `cargo xtask capabilities --check` → **13 个文件与源码一致**（无需重新生成）。

### 8.5 渲染（一个括号包起来的 OR 表达式 + 值全部走绑定）

```sql
-- 自定义条件 OR 本部门（OWN_ORG）
... AND ((amount <= $2) OR (org_node_id = ANY($3)))
```

| 要求 | 落实 |
|------|------|
| 一个括号包起来的 OR 表达式 | 整个 OR 组只加**一对**括号 → 不会被外层的 AND 拆开 |
| 自定义条件的值走绑定 | `$2`（`ConditionValue` → `BindValue`） |
| 子树 id 集合走**数组绑定** | `$3` = `BindValue::TextArray` —— **没有**展开成字符串拼接 |
| 多个 CUSTOM 取 OR | `((c1) OR (c2))` |

证据：`platform-db` 的 2 条单测（`a_custom_scope_renders_every_condition_as_one_or_group`、
`a_custom_or_scope_renders_one_or_group_with_an_array_bound`）+ 真实库用例里的 SQL 断言
（`sql.contains(" OR ")`、SQL 文本里**不含** `dept-a` / `10000`、绑定值 3 个）。

### 8.6 新增用例（真实库层，**6** 条）

| 用例 | 覆盖 |
|------|------|
| `a_custom_scope_is_unioned_with_the_other_roles_scope` | **四个象限**（主部门内/外 × 满足/不满足 CUSTOM）；★「主部门内但不满足 CUSTOM → **可见**」是本次修复的核心判据；另断言过滤条件形态是 `CustomOr`、SQL 是 OR 组、值全走绑定 |
| `multiple_custom_scopes_are_unioned` | 两个 CUSTOM 条件不同 → **满足任一**即可见（OR，不是 AND） |
| `all_wins_over_a_custom_scope` | `ALL` + `CUSTOM` → `ALL`（全部可见，含金额超限的记录） |
| `the_most_permissive_containment_scope_still_wins` | **回归**：`OWN_ORG` + `OWN_ORG_AND_BELOW` → `OWN_ORG_AND_BELOW`（行为不变） |
| `the_permission_checker_is_usable_as_a_trait_object` | 顺带确认 1（dyn 兼容） |
| `a_condition_outside_the_whitelist_never_grants_access` | 顺带确认 2（fail closed） |

另有 **7** 条单测（`platform-rbac` 的 `scope::tests` **+3**、`checker::tests` **+3**；
`platform-db` 的 `query::tests` **+1**）。

### 8.7 变异验证（第二轮 **2** 条）

判据：改错 → **对应测试红** → 还原（**sha256 校验**）→ **对应测试绿**。

| # | 故意改错的地方 | 期望变红的测试 | 实测 | 还原 |
|---|--------------|--------------|------|------|
| 1 | **退回旧行为**：「有 CUSTOM 就只返回 CUSTOM」（`scope.rs::combine_scopes` 把 `plus` 丢成 `None`） | `rbac_real_db::a_custom_scope_is_unioned_with_the_other_roles_scope` + `scope::tests::a_custom_scope_is_unioned_with_the_other_roles_scope` | ✅ **FAILED（2 条）**：真实库报「主部门内不满足 CUSTOM 的记录必须仍然可见」且可见集合少了 `item-big`；单测报 `left: None / right: Some(OwnOrg { .. })` | ✅ sha256 一致（`f2b62728…d9bb0`）→ 还原后 1 passed / 2 passed |
| 2 | **多个 CUSTOM 由 OR 改成 AND**（`platform-db::render_condition_group` 的 `join(" OR ")` → `join(" AND ")`） | `rbac_real_db::multiple_custom_scopes_are_unioned` + `platform-db::query::tests::a_custom_scope_renders_every_condition_as_one_or_group` | ✅ **FAILED（2 条）**：真实库只剩交集 `{item-b1, item-self2}`；单测渲染出 `((amount <= $2) AND (dept_id = $3))` | ✅ sha256 一致（`655df077…51f94`）→ 还原后 2 passed / 1 passed |

> 两次变异都**没有**靠「整条 SQL 文本比对」蒙对：第 1 条红在**记录集合**上，
> 第 2 条红在**记录集合**与**渲染结果**上 —— 也就是说，修复的语义本身被用例**真的**盯着。

### 8.8 两个「顺带确认」的结论

#### ① 四个方法全同步 → trait 仍 dyn 兼容 ✅

`PermissionChecker` 的四个方法（`has` / `scope` / `effective_scope` / `condition`）
**全部是同步方法**：没有 `async fn`、没有 RPITIT（返回具名类型）→ trait **对象安全**。

证据（`the_permission_checker_is_usable_as_a_trait_object`，真实库层）：

- `let dyn_checker: Arc<dyn PermissionChecker> = platform_rbac::as_dyn_checker(checker.clone());`
  —— **这一行能编译过就是证明**：任何一个方法改成 `async fn` 或返回 `impl Trait`，
  这行都编译不过（`Arc<dyn PermissionChecker>` 不成立）；
- 四个方法**逐个经由 trait 对象**调用并断言结果（`has` / `scope` / `effective_scope` / `condition`）；
- 平台侧本来就靠它注入请求扩展（`platform-core::permission::authorize` 从扩展里取
  `Arc<dyn PermissionChecker>`），`platform-core/tests/authz_entry.rs` 另有 7 条用例覆盖。

#### ② `condition()` 引用白名单外属性 → fail closed ✅（但**返回的不是 `None`**）

**实测行为**：返回 **`Some(1 = 0)`**（永不匹配的条件），**不是** `None`。
（`platform-core` 的 trait 文档写的是「条件引用白名单外的属性时返回 `None`」——
**文档与实现不一致**，见下面的说明。）

证据（`a_condition_outside_the_whitelist_never_grants_access`，真实库层）：

1. 做法：用一个**不含 `record.amount`** 的白名单重建判定器 → 注册表里
   `sandbox:scope-item:approve` 的条件 `$record.amount <= 10000` 就成了**白名单外**的写法；
2. `checker.condition(ctx, PERM_APPROVE)` → `sql == "1 = 0"`、`values` 为空
   → **永不匹配**；
3. 调用方把它交给 `TenantQuery::with_condition` → **一行都看不到**（数据范围是 `ALL` 也看不到）
   → 调用方据此**判定不通过**（fail closed），**不是**「忽略条件放行」；
4. 对照组：白名单内的同一条件 → 编译成**真实**条件 → 金额超限的记录被挡住、满足条件的可见
   → 证明条件**确实被生效**，不是被忽略；
5. 另有 3 条单测（`checker::tests`）钉住同一条策略：白名单外属性 / 函数调用 / 多语句
   → 一律落到 `1 = 0`。

**为什么实现不改成「返回 `None`」**（这点需要 BA 复核）：

| 方案 | 后果 |
|------|------|
| 返回 `None`（照 trait 文档改） | `None` 同时表示「该权限点**没有**条件」与「条件**非法**」→ 调用方无法区分 → 按文档「`None` 即判定不通过」的话，**所有没有条件的权限点都会被拒**（把「无条件」误当成「条件非法」）；若调用方选择「`None` 就放行」，则白名单外的条件会被**静默忽略 = 放行** |
| 保持 `Some(1 = 0)`（现实现） | 「没有条件」= `None`（调用方不加条件）；「条件非法」= 永不匹配（调用方**必然**判不通过）→ **两个语义不混淆，且都是 fail closed** |

→ **建议 BA 修订 trait 文档**（把「引用白名单外的属性时返回 `None`」改成
「返回**永不匹配**的条件」），而不是改实现。**本轮按「不改，只确认」的要求没有动它。**

### 8.9 本轮的已知取舍与剩余风险（如实列出）

| 项 | 说明 |
|----|------|
| **改了 `platform-core`** | 契约表达能力缺口所迫（§8.4），**需要 BA 复核这处演进**；改动是纯增量 |
| **trait 文档与实现不一致** | 见 §8.8 ② —— 建议改文档，本轮未改 |
| `use_all_memberships` 开关口径 | 「取所有成员节点的并集」的开关取自**所有**贡献（任一角色要求并集 → 整体并集）—— 与第一轮一致，**行为未变** |
| 自定义 + 包含型范围时的查询次数 | 组合出 `CustomOr` 时，每个权限点会**多查一次**组织节点（第一轮只查纯 scope）。只在「有 CUSTOM 角色」的用户上发生，量级是「权限点数 × 1 次查询 / 缓存刷新」—— **未做优化**（正确性优先），留作后续 |
| `platform-org` 的偶发失败 | 一次完整套件跑里 `platform-org` 的 `org_real_db` 有 **1 条**偶发失败（单独重跑 **15 passed**，另两次完整套件全绿）—— 与本次改动**无关**（platform-org 不碰 `ScopeFilter`），属本仓既有的回归不稳定（见 §2.3） |
| 依赖预算 | **不变**：锁文件未改动，仍是 **293 → 294**（+1 是本地工作区成员 `platform-rbac`）；本轮**未引入任何新依赖** |

---

## 9. 提交与分支

| 项 | 值 |
|----|---|
| 分支 | `feature/REQ-009` |
| 基线 | `develop` @ `400cefa` |
| 提交 | ① `[Dev] 实现 RBAC 权限与数据范围（角色模型 / 数据范围 / 鉴权中间件 / 契约 C-5） (关联: REQ-009)`<br>② `[Dev] 修正 CUSTOM scope 的多角色合并语义为真正的并集 (关联: REQ-009)` |
| 契约 pin | `feature-meta/manifest.json` → `{"api-conventions": "v1", "extension-points": "v2"}` |
| 追溯矩阵 | `feature-meta/traceability.md` |
| 变更日志 | `CHANGELOG.md` → `### REQ-009 RBAC 权限与数据范围` |
