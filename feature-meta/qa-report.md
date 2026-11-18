# 验证报告 —— Pre-merge

- **需求**: REQ-009「RBAC 权限与数据范围」
- **被验分支**: `feature/REQ-009` @ `05bc6bd`（含 `628ac19` 实现 + `05bc6bd` CUSTOM 并集修正）
- **基线**: `develop` @ `400cefa`
- **验证者**: QA Agent（**独立复现；Dev 自验报告里的每个数字都重跑过，未采信**）
- **环境**: rustc/cargo 1.98.1 · podman + postgres 17-alpine（`base-postgres` / `base-redis` healthy）· pnpm 12.19.0 · 7 核 / 7.7G 内存（可用 ~4.8G）· 磁盘剩 110G · `CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target`
- **日期**: 2026-11-18
- **结论**: **PASS-WITH-RISK** —— 无 HIGH；**2 项 MEDIUM** 需 BA 拍板（1 项是规格与实现的契约冲突，1 项是测试夹具污染）

> **核心结论**：16 条 AC（AC-S-033 ~ AC-S-048）**逐条实测通过**；完整集成套件 **4 轮全绿，452/452**；
> **3 处关键变异全部被测试抓红**（fail closed→fail open、并集→交集、404→403），还原后逐字节一致、重新变绿。
> **C-5 我用自造夹具 + 真实 `check_c5` 独立跑过 4 负向 + 2 正向**，确认它扫的是**调用点**而不是字面量。
>
> **没有 HIGH、没有阻塞合并的缺陷。** 两项 MEDIUM 必须如实登记（均不改变当前实现的正确性，但一项是已裁定决策与 trait 文档的冲突，另一项是测试夹具的全局副作用 —— 后者对 Dev 声称的「platform-org 偶发失败与本次改动无关」构成反证）。

---

## 1. 追溯矩阵

| AC | 标题 | 我实际执行的检查 | 结论 |
|---|---|---|---|
| AC-S-033 | 注册表双向对账，不一致即启动失败 | 真启进程：manifest 多一个权限点 → **exit 1**；库多一个权限点 → **exit 1**；两边一致 → **正常启动**（日志「角色与权限已启用：/api/rbac/*」） | **PASS** |
| AC-S-034 | 角色/授权/绑定 + 租户维度 + 自举防护 | `base_app` 实测 RLS（tenant-a 只看到 A、tenant-b 只看到 B、无 ctx 看到 0）；`pg_policies` 逐张列出 5 张表 policy；普通用户调写/读接口 → 403；注册表查询豁免（路由无 `route_layer`） | **PASS** |
| AC-S-035 | 继承展开 + 成环被拒 | `detect_cycle` 错误带**两个角色 code**（`service.rs:525-534`）；`set_inherit` 走 `require_role` → 跨租 id = 404 | **PASS** |
| AC-S-036 | 通配语义 + 禁全通过 | `PermissionPattern::parse` 拒绝 `*:*` 与跨模块通配 → `WildcardForbidden`（`service.rs:405-411`）；精确权限点必须命中注册表，否则 `InvalidArgument`（防「假权限」） | **PASS** |
| AC-S-037 | 内置角色不可删降权；admin = 注册表全集 | 删/改 scope/撤权 → 均 `BUILTIN_ROLE_PROTECTED`；admin 权限集合**逐个比对注册表全集**（测试遍历 `registry` 每一项，未写死数字、未抽样） | **PASS** |
| AC-S-038 | 五种 scope 求值 | 五个探针用户 × 五种 scope，逐条列出期望 id 集合；五个集合两两不相等 | **PASS** |
| AC-S-039 | 兼岗默认主部门；取并集须显式开启 | 默认 OWN_ORG → 只含 dept-a；默认 BELOW → 只含主部门子树；开 `use_all_memberships` → 变宽（含 dept-b/dept-b-1），且 `assert_ne!` 断言与默认不同 | **PASS** |
| AC-S-040 | 主部门缺失/歧义 → fail closed 到 SELF | **预防层**：DB 拒第二个主部门（部分唯一索引）；**防御层**：`DROP INDEX` 造歧义 → 仍退化为 SELF。**变异验证**：改成 fail open 后该用例立刻变红 | **PASS** |
| AC-S-041 | scope 优先级 + 多角色并集 | D37 四象限逐条断言（含核心判据「本部门内但不满足 CUSTOM → **仍可见**」）；多个 CUSTOM 取 OR；权限点级 scope 覆盖角色级 | **PASS** |
| AC-S-042 | 受限 DSL 拒绝越界写法 | **独立探针**跑真实编译器：5 条表达式逐条实测 + **DB 级**注入三判据（SQL 文本无注入串 / 无全表命中 / 只返字面量等值行） | **PASS** |
| AC-S-043 | 属性级校验（RBAC+ABAC） | 白名单外属性 → `never_match()`（`1 = 0`）→ 判不通过；属性条件不满足 → 403 | **PASS** |
| AC-S-044 | 401/403/404 三语义 + 错误体 | 三种拒绝实测；`detail` 英文 ASCII；不含被拒资源业务字段 | **PASS** |
| AC-S-045 | 404 不泄露存在性 | 越权资源 vs 不存在资源：**状态码 / 错误体字段集合 / 响应头集合**逐项对比全同；范围内记录 → 200 | **PASS** |
| AC-S-046 | 已登记权限点必须有鉴权调用点；入口唯一 | **自造 6 个夹具**用真实 `check_c5` 跑：4 负向全红、2 正向全绿；全仓入口定义处 = **恰好 1 处**（真实扫描器实测） | **PASS** |
| AC-S-047 | 撤销下一请求即生效 + 跨进程 | **两个独立 `RbacChecker` 实例**（各自缓存、共享 DB）→ 实例 2 撤销后立刻被拒；撤销**数据范围**同样立即生效 | **PASS** |
| AC-S-048 | 数据范围在 SQL 层生效、无法绕过 | 渲染出的 SQL 含 `org_node_id = ANY($n)`，值全走绑定；`TenantQuery` 公开 API 无「跳过范围」入口 | **PASS** |

---

## 2. 安全断言实测

### A. 数据范围（算错 = 泄漏）

| 判据 | 怎么造的违规 | 实际返回 | 结论 |
|---|---|---|---|
| A-1 兼岗默认主部门 | 用户兼岗 `dept-a`(primary) + `dept-b`(非 primary)，两部门各有记录 | 默认 OWN_ORG = `{item-a1,item-big}`（**不含 dept-b 任何一条**）；默认 BELOW = 主部门子树；开 `use_all_memberships` 后 = 7 条（含 `item-b1/item-b2/item-self2`） | PASS |
| A-2 预防层 | 已有主部门的用户再插 `is_primary=TRUE` | DB **拒绝**（部分唯一索引 `user_org_one_primary_idx`） | PASS |
| A-2 防御层 | `DROP INDEX` 后造两个主部门 | 判定退化为 `SELF` = **只有本人创建的 1 条**（`item-dualprimary`），**看不到 `item-a1`** | PASS |
| A-3 多角色最宽松 | R1(OWN_ORG)+R2(export 收紧 SELF)+R3(BELOW)；第 3 步**解绑 R3** | 第 1 步=BELOW；解绑后**退为 OWN_ORG**（与第 1 步**不同**）→ 证明是并集不是交集 | PASS |
| A-3 权限点级覆盖 | R2 的 `export` 收紧为 SELF | export 有效范围 = `SELF`（覆盖角色级 OWN_ORG） | PASS |
| A-4 SQL 层过滤 | 业务侧不写任何范围条件，只交 `ScopeFilter` | 渲染 SQL = `... AND (org_node_id = ANY($2))`，**SQL 文本里无 `dept-a`**；值走 `BindValue::TextArray` | PASS |
| A-4 无绕过开关 | 翻 `TenantQuery` / `ScopeFilter` 公开 API | 无任何「跳过范围」方法或参数；`ScopeFilter` 枚举里也没有该选项 | PASS |

**D37 核心判据（四象限，逐条断言）** —— 用户 Y 同时有 P1(OWN_ORG) 与 P2(CUSTOM `amount<=10000`)：

| 象限 | 记录 | 期望 | 实测 |
|---|---|---|---|
| P1 内 ∧ 满足 CUSTOM | `item-a1` | 可见 | **可见** ✓ |
| **P1 内 ∧ 不满足 CUSTOM** | `item-big` | **可见** | **可见** ✓ ← 旧实现的 bug 点 |
| P1 外 ∧ 满足 CUSTOM | `item-b1` | 可见 | **可见** ✓ |
| P1 外 ∧ 不满足 CUSTOM | `item-out-big` | 不可见 | **不可见** ✓ |

过滤条件形态实测为 `ScopeFilter::CustomOr { conditions: 1, scope: OrgNodes }`；渲染 SQL 含 ` OR `、**不含** `dept-a` / `10000` 字面值，绑定值 3 个（租户 + 金额 + 节点数组）。

### B. 鉴权三语义与存在性泄露

| 情形 | 期望 | 实测 |
|---|---|---|
| 无 / 无效 token | 401 `UNAUTHENTICATED` | ✓ |
| 有 token、**无权限点** | 403 `INSUFFICIENT_PERMISSION`（**不是** `PERMISSION_DENIED`） | ✓ |
| 有权限点、记录超范围 | 404 `NOT_FOUND` | ✓ |

**B-4 自举防护**：普通用户（只绑内置 `platform-user`）调 `POST /api/rbac/roles` → **403 `INSUFFICIENT_PERMISSION`**，且**库里查不到那条角色**（被拒的写入不得部分生效）；读 `GET /api/rbac/roles` → 同样 403；`GET /api/rbac/permissions` → **豁免**（该路由无 `route_layer`，`http.rs:420`）。

### C. 注册表双向对账（判据是「进程起不来」，不是「日志里一行警告」）

真启 `base serve`（`DATABASE_URL` 用**普通角色** `base_app`，超管会被 `require_isolation` 拒绝）：

- **manifest 多一个权限点** → `EXIT_CODE=1`，报错逐条列出且**带来源 manifest 路径**：
  ```
  ✗ 权限点注册表与数据库不一致，进程退出（fail-fast）：
  [REGISTRY_MISMATCH] ... the permission `sandbox:ghost:read` is declared in modules/sandbox/manifest.json but missing from the database
  ```
- **库里多一个权限点** → `EXIT_CODE=1`，列出 `the permission `ghost:perm:read` exists in the database but is not declared in any manifest`。
- **两边一致** → 服务正常起（`角色与权限已启用：/api/rbac/*` + `底座服务已启动`），超时杀进程 → exit 124（= 保持运行）。

### D. C-5 契约检查（**全部自造夹具**，用真实 `check_c5` 跑）

探针：独立 crate（`/tmp/qa-req009/c5probe`）链接仓库里的 `platform-contracts`，构造假仓库根（含 `crates/platform-core/src/extension/permission.rs` 以便 `find_repo_root` 定位），每个夹具一个模块目录。

| 夹具 | 造法 | 期望 | 实测 `VIOLATION_COUNT` | 关键信息 |
|---|---|---|---|---|
| v1_literal_only | manifest 登记 `probe:thing:read`，后端**只有 const 字面量、无调用点** | 失败 | **1** | `the permission point 'probe:thing:read' is declared in manifest.json but has no authorization call site` |
| v2_custom_entry | 模块自己写 `fn check_perm()` 并调用 | 失败 | **3** | `the custom authorization entry point ... bypasses the platform entry point` |
| v3_non_literal | `require_permission(PERM)`（常量参数） | 失败 | **2** | `does not use a string literal; it cannot be verified statically` |
| v4_ok | 正常模块（登记 + 字面量调用点） | 通过 | **0** | —— |
| v5_second_entry | 另一个 crate 里定义 `fn require_permission` | 失败 | **1** | `must be defined exactly once in the whole repository, found 2` |
| v6_undeclared | 调用了未登记的权限点 | 失败 | **2** | `uses the permission point 'probe:other:read', which is not declared` |

★ **v1 是「防假绿」的决定性证据**：C-2 的字面量扫描会把 `const PERMISSION_THING_READ: &str = "probe:thing:read"` 当成「用到了」；C-5 **没有** —— 它认的是**调用点**。**C-5 没有沿用 C-2 的字面量逻辑。**

**入口唯一性（真实仓库实测）**：
```
$ c5probe --entries /home/myg133/Projects/system_seed/feature-REQ-009
ENTRY_DEFINITION_COUNT=1
ENTRY_DEFINITION=.../crates/platform-core/src/extension/permission.rs
```
（`authz_scan.rs` 里另有 4 处 `fn require_permission` 字样，均为**文档注释或测试里的字符串字面量** —— 扫描器用 `strip_comments` + `blank_strings` 正确排除，实测计数为 1。）

### E. 撤销立即生效 + 跨进程

两个**独立** `RbacChecker` 实例（各自持自己的 `Mutex<BTreeMap>` 缓存、共享同一个 DB）：实例 1 撤销 → **实例 2** 的下一个 `ensure_fresh` + `has` 立刻为 false；实例 1 亦然。撤销**数据范围**（改 scope 而非权限点）同样立即生效。失效信号来自 `platform_permission_epoch`（DB 共享真相），非进程内广播。

### F. 表达式 DSL 逃逸（**独立探针**，真实编译器 + DB）

| # | 表达式 | 期望 | 实测 |
|---|---|---|---|
| 1 | `$record.amount <= 10000` | 成功 + 参数绑定 | `sql="amount <= ?"` `values=[Int(10000)]`，**SQL 文本无 `10000`** ✓ |
| 2 | `$record.secret == "x"` | 拒绝 | `the attribute 'record.secret' is not in the whitelist (allowed: ctx.tenant_id, record.amount, record.dept_id, user.id)` ✓ |
| 3 | `now() < $record.created_at` | 拒绝 | `function calls are not allowed in a condition expression ('now')` ✓ |
| 4 | `$record.org_node_id == "a' OR '1'='1"` | 当作普通字符串 | 渲染 SQL = `... AND (org_node_id = $2)`，**SQL 文本无注入串、无 ` OR `**；DB 实跑：总 3 行 / **命中 1 行**（恰好字面量等值那行）✓ |
| 5 | `$record.amount <= 10000; DROP TABLE t` | 拒绝 | `multiple statements are not allowed in a condition expression` ✓ |

---

## 3. 存在性泄露实测（AC-S-045）

对照组 = **「无权访问的资源」 vs 「不存在的资源」**（不是「有权 vs 无权」）：

| 比对项 | 越权资源 `item-a2` | 不存在 `item-not-exist-9999` | 差异 |
|---|---|---|---|
| HTTP 状态码 | 404 | 404 | **无** |
| 错误码 | `NOT_FOUND` | `NOT_FOUND` | **无** |
| 错误体字段集合 | `type,title,status,detail,instance,code,trace_id` | 同 | **无** |
| 响应头集合 | 逐项相同 | 逐项相同 | **无** |
| 对照组：范围内记录 | 200 + 返回该记录 | — | 接口非恒 404 ✓ |

`detail` 为英文 ASCII，且**不含**被拒资源的业务字段（`item-b1` / `dept-b` / `5000` / 归属人 均断言不出现）。

★ **变异验证**：把 404 改成 403 → **两个用例同时变红**（`a_record_outside_the_data_scope_is_not_found` + `an_unreadable_record_and_a_missing_record_are_indistinguishable`）→ 证明这两条断言不是摆设。

---

## 4. 假绿护栏结果

### 4.1 实际 passed 数

| 跑法 | 结果 |
|---|---|
| `bash scripts/test-integration.sh`（含 `--include-ignored`，23 个测试二进制） | **452 passed / 0 failed / 0 ignored** |
| `cargo test --workspace -- --test-threads=2`（默认，真实库用例被 `#[ignore]` 跳过） | **393 passed / 0 failed / 65 ignored** |
| 前端 `pnpm -r test` | **23/23 passed**（与基线 23/23 一致） |

`cargo test` 全程**只有一个 `--`**。基线参考：REQ-008 合并时 303/303（含 ignored 341/341）→ 本需求后为 393/393（含 ignored 458/458）。

### 4.2 连跑轮次（≥2 轮）

| 轮次 | 范围 | 结果 |
|---|---|---|
| 完整集成套件 × **4 轮** | `scripts/test-integration.sh`（每轮重置 `base_test`） | **4/4 全绿，452/452** |
| `org_real_db` 单二进制 × **12 轮** | `--test-threads=2 --include-ignored` | 12/12 绿 |
| `rbac_real_db` 单二进制 × **12 轮** | 同上 | 12/12 绿 |

合计 **24 次真实库二进制重复跑 + 4 次完整套件**，**零偶发绿、零偶发红**。

### 4.3 ★ `platform-org` 偶发失败的专门排查（Dev 声称「与本次改动无关」）

**结论：未能复现（15 次 `org_real_db` + 4 轮完整套件全绿），但 Dev 的「无关」归因不成立** —— REQ-009 这次**同时**（a）给 REQ-008 的 `user_org` 加了全局部分唯一索引，（b）新增了一个**在测试运行期间把这个索引摘掉再装回去**的用例（见问题 #2）。

已实测证明该窗口的危害（直接在 `base_test` 上做）：

```
-- 索引在位时
DROP INDEX IF EXISTS user_org_one_primary_idx;
INSERT ... is_primary=TRUE;   -- 第 1 条
INSERT ... is_primary=TRUE;   -- 第 2 条  → 成功（预防层暂时失效）
SELECT 'duplicate_primary_rows=' || count(*) ...  → 2
-- 重新装回索引（rbac 用例的最后一步）
CREATE UNIQUE INDEX user_org_one_primary_idx ON user_org (tenant_id,user_id) WHERE is_primary;
→ ERROR: could not create unique index "user_org_one_primary_idx"
          DETAIL: Key (tenant_id, user_id)=(...) is duplicated.
```

即：**索引被摘掉的窗口内，第二个主部门写得进去；窗口结束后索引装不回来**。若 `rbac_real_db` 在 `DROP` 与 `CREATE` 之间 panic，预防层就**留在关闭状态**直到本轮结束，下一轮的 `CREATE UNIQUE INDEX`（`:840`，同样 `.expect(...)`）也会失败 → 级联变红，且此时 `platform-org` 的用例已失去 DB 层保护。

### 4.4 变异验证（3 处，逐字节还原）

| # | 变异 | 改动位置 | 结果 |
|---|---|---|---|
| 1 | **fail closed → fail open**（`DegradeToSelf` 改成 `ScopeFilter::All`） | `crates/platform-rbac/src/scope.rs`（2 处） | **红**：`a_missing_or_ambiguous_primary_department_falls_back_to_self` 失败（17 passed / 1 failed） |
| 2 | **并集 → 交集**（CUSTOM 只取 CUSTOM 那一路，即旧 bug 语义） | `crates/platform-rbac/src/scope.rs`（`combine_scopes`） | **红**：`a_custom_scope_is_unioned_with_the_other_roles_scope` 失败（17 passed / 1 failed） |
| 3 | **404 → 403** | `modules/sandbox/backend/src/http.rs` | **红**：`a_record_outside_the_data_scope_is_not_found` + `an_unreadable_record_and_a_missing_record_are_indistinguishable` 同时失败（4 passed / 2 failed） |

**还原核对**：`git checkout --` 还原后 `git diff --stat` 为空、`git status --short` 为空，sha256 与变异前**逐字节一致**：

```
f2b627285b33f9375f8193239737169a74f35c79f0bcaf22ebf5a701dc8d9bb0  crates/platform-rbac/src/scope.rs
8f283c2812fcbd36955a18b77af339f80e51d6991ade97921ee38dc532308e97  modules/sandbox/backend/src/http.rs
```

还原后重跑：**rbac_real_db 18 passed / 0 failed**、**http_authz 6 passed / 0 failed** → **绿**。

---

## 5. 契约与基建

| 检查 | 命令 | 结果 |
|---|---|---|
| 契约门禁 | `bash scripts/sync-contracts.sh` | **exit 0** —— `✓ 快照一致：docs/contracts/api-conventions/v1.md` + `✓ 快照一致：docs/contracts/extension-points/v2.md`（此前恒 exit 2 的缺陷**已修掉**） |
| 契约 pin | `feature-meta/manifest.json` | `"contracts": {"api-conventions":"v1","extension-points":"v2"}` ✓ |
| 快照与正本一致 | 由上条脚本验证（正本 = `demand` 分支） | ✓（快照非手改） |
| 能力索引 | `cargo xtask capabilities --check` | **通过**：13 个文件与源码一致 |
| 依赖方向 | `cargo xtask check-deps` | **通过**：平台 crate 未反向依赖业务模块（`platform-rbac` 未反向依赖业务模块 ✓） |
| 模块目录 | `cargo xtask check-modules` | 通过：1 个模块（sandbox） |
| 格式 | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | **0 告警**（exit 0） |
| `AGENTS.md` 一致性 | 人工核对 diff | ✓ 新增 `crates/platform-rbac` 目录；✓ C-1~C-5（`contract_suite` 行改为「五项」）；✓ 硬规则 1 的**例外说明**（平台自身在已评审需求下的演进）；✓ 新增「鉴权怎么写」段 + 数据范围由平台叠加 + C-5 常量即假权限 |

---

## 6. 依赖预算

| 项 | 基线 | 实测 | 差 |
|---|---|---|---|
| `Cargo.lock` 包数 | 293 | **294** | **+1** |
| 第三方 registry 包数 | 284 | **284** | **0** |
| 本地工作区成员 | 9 | 10 | +1（`platform-rbac`） |

**核的是 `Cargo.lock` 的 diff，不是 `Cargo.toml`**：`git diff 400cefa..HEAD -- Cargo.lock` 显示**只有** `platform-rbac` 这个新本地成员的条目，以及 `platform-core`/`platform-server`/`sandbox` 因引用它而多出的依赖名（`axum` / `http-body-util` / `tower` —— 全部是**已在锁文件里**的第三方包，未新增 registry 包）。**未引入策略引擎、未引入 Redis 客户端、未用 nom/pest**。✓

---

## 7. 问题（按严重度）

### HIGH：0

### MEDIUM：2

#### #1（中）`PermissionChecker::condition()` 的 trait 文档与已裁定决策 D40 相互矛盾 —— 给后续实现者留了 fail-open 陷阱

- **触发条件**：任何第三方/后续按 **trait 文档**实现 `PermissionChecker`（或照文档理解调用方契约）。
- **期望**（D40，`BA/demands/REQ-001-platform-foundation/decisions.md:46`，已 ratified、标注「否（安全语义）」）：`None` = **无条件** → 直接放行；`Some(永不匹配)` = **条件非法** → 判不通过。
- **实际**：
  - `crates/platform-core/src/extension/permission.rs:180-184` 的文档写的是「**条件引用白名单外的属性时返回 `None`**」—— **既与 D40 相反，也与本需求的实现相反**。
  - **实现是对的**：`crates/platform-rbac/src/checker.rs:42-55` 对非法条件返回 `Some(never_match())`（`1 = 0`）；`:361-366` 的 `condition()` 取出的是已编译的永不匹配条件。
  - **调用方会把 `None` 当放行**：`modules/sandbox/backend/src/http.rs:273-313` —— `if let Some(condition) = ... { 应用条件 }`，`None` 直接落到 200。
  - ⇒ 照 trait 文档实现的 `PermissionChecker` 会让**非法属性条件静默放行** —— 正是 D40 要防的「静默放行」。
- **影响**：**当前实现无漏洞**（行为正确、且已被测试锁住）；风险在**平台 API 的契约文字**，是给下一个实现者的 fail-open 陷阱。
- **证据**：`crates/platform-core/src/extension/permission.rs:180-184`（文档）vs `crates/platform-rbac/src/checker.rs:42-55`（实现）vs `modules/sandbox/backend/src/http.rs:273-313`（调用方）。
- **Dev 已自报**：verification-report §8.8 ② 建议「BA 修订 trait 文档（而不是改实现）」，**本轮未修**。
- **建议**：BA 拍板后由 Dev 只改文档措辞（改成「返回**永不匹配**的条件」），**不改实现**。

#### #2（中）`rbac_real_db` 的 fail-closed 用例在**全局**摘掉唯一索引 + 跨租通配 DELETE —— 测试夹具污染，且构成对 Dev「platform-org 偶发失败与本次改动无关」归因的反证

- **触发条件**：同一测试库里并行跑用例（`--test-threads=2`），或同一轮里 `rbac_real_db` 之后还有别的用例。
- **期望**：用例只影响**自己**的夹具；任何时刻都不应把**全局**约束（DB 预防层）摘掉。
- **实际**（`crates/platform-rbac/tests/rbac_real_db.rs`）：
  - `:835-838` `DELETE FROM user_org WHERE user_id LIKE 'u-dual-primary-%'` —— **没有租户条件**，删的是**所有租户**下匹配前缀的成员行；
  - `:876` `DROP INDEX IF EXISTS user_org_one_primary_idx` —— **全局 schema 变更**（影响所有租户、所有并发用例）；
  - `:927-933` `CREATE UNIQUE INDEX ... .expect("重建索引应当能成功")` —— 若此刻库里有任何一对重复主部门，**panic**；
  - `:840` 同名 `CREATE UNIQUE INDEX IF NOT EXISTS ... .expect("部分唯一索引应当能建立（预防层）")` —— 上一轮若遗留重复，**同样 panic**。
- **实测证据**（§4.3）：索引摘掉期间**第二个主部门写得进去**，`CREATE UNIQUE INDEX` 随即失败（`could not create unique index ... is duplicated`）。
- **影响**：① 若该用例在 `DROP` 与 `CREATE` 之间失败，**预防层会留在关闭状态**直到本轮结束 → 后续 `platform-org`/`platform-rbac` 用例失去 DB 层保护，可能出现**间歇性失败**（与 Dev 报告里那条 `platform-org` 偶发失败现象一致）；② 跨租通配 DELETE 可能清掉并发用例的夹具。**未复现**（15 次 org + 12 次 rbac + 4 轮完整套件全绿），故列为**存疑**而非确认失败。
- **对 Dev 归因的反驳**：Dev 称该偶发失败「与本次改动无关（platform-org 不碰 `ScopeFilter`），属本仓既有的回归不稳定」。但 REQ-009 本次**恰恰**给 `user_org` 加了这条全局唯一索引，并新增了**运行期摘掉它**的用例 —— 与 `ScopeFilter` 无关不等于与本次改动无关。**「无关」的归因没有证据支撑。**
- **建议**：把该用例的索引操作**限定到本用例自己的租户**（或用独立 schema / 独立测试库），并把 `DELETE` 加上 `tenant_id` 条件；至少不要让 panic 发生在 `DROP` 与 `CREATE` 之间（用 guard 保证异常路径下也装回索引）。

### LOW：2

#### #3（低）C-5 违例信息里的位置是**字符偏移**，但文档说是「文件:行」

- `crates/platform-contracts/src/authz_scan.rs:62` 文档：「绕过平台入口的自定义鉴权入口（**`文件:行`**）」；而 `:117` / `:127` / `:135` 用的是 `format!("{}:{}", file.display(), start)`，`start` 是源码的**字符下标**，不是行号。
- **实测**：我的 v2 夹具报 `.../v2_custom_entry/backend/src/lib.rs:7` 与 `.../lib.rs:96` —— 该文件只有 3 行，`:96` 明显是字符偏移。
- **影响**：报错指向的位置不对，定位契约违规会多花时间。不影响判定结果。

#### #4（低）「通配授权 vs 精确授权」同时命中同一权限点时，取哪一条的 scope 由**排序**决定，规格未定义

- `crates/platform-rbac/src/service.rs:883-891`：`matched` 保留**最后一个**匹配的 grant；遍历顺序 = `ancestors()`（按**角色 ID** 排序，`service.rs:88`）+ `ORDER BY role_id, permission_code`（`store.rs:381,395`）。
- 直接授权时，因 `'*' < 小写字母`，精确授权排在通配之后 → 精确的 scope 生效（结果符合直觉，但**是排序的副产物，不是显式规则**）；经继承时，胜者由**角色 ID 的字典序**决定（角色 ID 是 UUID → 实际等同随机）。
- **AC-S-041 未定义**该情形。**属规格缺口，不是已确认缺陷**（无测试覆盖此组合）。

### INFO：3

- **#5** Dev 自验报告写的完整套件数字 **「439 / 439」已过时**：`05bc6bd`（CUSTOM 并集修正）新增了 **13** 个测试属性（`git diff 628ac19..05bc6bd | grep -c '^\+\s*#\[(tokio::)?test\]'` = 13），HEAD 实测 **452/452**。报告未在最后一次提交后重测。
- **#6** `use_all_memberships` 取自**所有**贡献角色（任一角色开即整体开，`scope.rs:274`）。已核实**不会跨权限点泄漏**：只有**真正授予该权限点**的角色才进入贡献（`service.rs:892-900`）。与 D37「取最宽松」一致，但规格未明写此组合。
- **#7** `platform-core` 的改动属已评审的平台演进（D36 允许），`AGENTS.md` 已同步例外说明 —— **与规格一致，非偏离**，登记备查。

---

## 8. 未覆盖 / 无法验证（如实列）

| 项 | 状态 | 原因 |
|---|---|---|
| **真实多实例部署下的 epoch 收敛** | **未覆盖** | 本机单实例；AC-S-047 用**两个独立 checker 实例**模拟跨进程（已验），**真正的两个 OS 进程 + 真实网络 + 真实数据库连接池**未跑。`acceptance.md` 自身把此项列为「已知不验收项（部署侧）」 |
| **Redis 部署形态下的缓存一致性** | **不适用** | 本期缓存是「进程内 + DB epoch」，本期明确不验收（`acceptance.md` 已知不验收项） |
| **CI 真跑**（6 个 job / 镜像体积门禁 / Trivy） | **无法验证** | 本仓**无 remote**，GitHub Actions 跑不起来（验收标准已列为已知不验收项） |
| **分支保护 / Code Owners** | **无法验证** | 需人工在远端配置 |
| **前端权限组件与 `can()`** | **不适用** | 属 REQ-011，本期只保证「注册表有接口可读」（已验：豁免路由存在） |
| **审计落库** | **不适用** | 属 REQ-014；本期只保证「权限变更触发了可审计事件」 |
| **跨租授权（平台管理员代客操作）** | **不适用** | 明确不在本期范围 |
| **`base_dev` 库** | **未触碰** | 全程只读写 `base_test`；探针、变异的夹具全部落在 `base_test` |
| **E2E（Playwright）** | **未跑** | 本需求不触碰前端路由/组件（前端 23/23 已跑）；判断为「不受影响」，但**未实测** |
| **`cargo xtask create-module` 脚手架的开箱 C-5** | **未跑** | Dev 自报做过（`scaffold-probe`，验证后已删）；我**未独立复跑**（会往 `modules/` 写文件，超出「不改项目源」边界）。C-5 本身我用**自造夹具**独立验过（§2-D） |
| **`platform-org` 偶发失败的确切根因** | **未能复现 / 未定论** | 15 次 org + 4 轮完整套件全绿；已定位**一条可疑机制**（问题 #2）但未证实因果 |
| **AC-S-036 通配的「跨模块通配被拒」端到端 HTTP 路径** | **部分** | 服务层 `WildcardForbidden` 已核实（`service.rs:405-411`）；我只做了代码级核实，**未自造 HTTP 请求**打这条路径（Dev 用例覆盖） |
| **覆盖率数字** | **不适用** | 本期不设覆盖率门槛 |

---

## 9. 结论

- **VERDICT: PASS-WITH-RISK**（角色口径：**PASS**，带 2 项需 BA 拍板的 MEDIUM）
- **问题计数**：**HIGH 0 / MEDIUM 2 / LOW 2 / INFO 3**
- **实际 passed**：完整集成套件 **452/452**（4 轮全绿，23 个二进制）；默认套件 **393/393**（65 ignored）；前端 **23/23**
- **最需要 BA 拍板的一条**：**问题 #1 —— `PermissionChecker::condition()` 的 trait 文档与 D40 矛盾**。请 BA 裁定：按 Dev 建议**只改文档措辞**（「非法条件返回**永不匹配**的条件」，而不是 `None`），实现不动；否则平台 API 的契约文字会给下一个实现者留一个 fail-open 陷阱。
- **次需 BA 关注**：**问题 #2** —— 请 BA 裁定是否要求 Dev 把 `rbac_real_db` 的全局索引操作限定在本用例租户内；并**复议 Dev 对 `platform-org` 偶发失败「与本次改动无关」的归因**（该归因缺少证据支撑）。
- **未改实现、未改规格、未 commit**：本轮只做了 §4.4 的 3 处临时变异（已逐字节还原，`git diff` 与 `git status` 均为空），以及 `/tmp` 下的探针与夹具。
