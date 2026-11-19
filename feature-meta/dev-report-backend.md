# REQ-011 后端部分 —— Dev 自验报告

- **需求**: REQ-011 前端 SDK 骨架（**后端部分**：AC-S-064 / AC-S-065）
- **分支 / worktree**: `feature/REQ-011` @ `/home/myg133/Projects/system_seed/feature-REQ-011`
- **基线**: `develop` @ `a3fae66`（REQ-010 合并点）
- **实施人**: Dev（后端棒）
- **日期**: 2026-11-19
- **前端部分**: 另一个 Dev 接手，本报告**不覆盖**

---

## 1. 交付概览

| # | 内容 | 落点 |
|---|------|------|
| 1 | `PermissionProvider` trait（`platform-core`） | `crates/platform-core/src/extension/permission.rs:239` |
| 2 | `RbacService` 实现该 trait（**转调**已有 `effective_permissions`） | `crates/platform-rbac/src/service.rs:850` |
| 3 | 新端点 `GET /api/rbac/me/permissions` | `crates/platform-rbac/src/http.rs:492`（路由）/ `:758`（处理器）/ `:437`（响应体） |
| 4 | 登录响应填**真实**权限点 | `crates/platform-iam/src/service.rs:621`（填充）/ `:271`（注入）/ `:283`（降级） |
| 5 | 装配（provider 注入 IAM） | `crates/platform-server/src/main.rs:326` |
| 6 | 集成测试（TC-011-001 ~ 010，10 条） | `crates/platform-rbac/tests/me_permissions.rs` |

---

## 2. AC 实现位置（逐条）

### AC-S-064 —— 后端能返回当前用户的有效权限点集合

| 步骤 / 预期 | 实现位置 | 说明 |
|---|---|---|
| `GET /api/rbac/me/permissions` 路由 | `crates/platform-rbac/src/http.rs:492` | `let me = Router::new().route("/me/permissions", get(my_permissions));` |
| **豁免**权限点要求 | `crates/platform-rbac/src/http.rs:492` | 该子路由**没有** `route_layer(require_permission(...))`（与注册表 `/permissions` 同款） |
| **要求已认证** | `crates/platform-rbac/src/http.rs:186`（`authenticate` 中间件） | 整个 `/api/rbac` 路由都有该中间件 |
| 响应体是**对象**（不是裸数组） | `crates/platform-rbac/src/http.rs:437` | `MyPermissionsResponse { permissions: Vec<String> }` → `{"permissions":[...]}` |
| 权限点**升序且去重** | `crates/platform-rbac/src/service.rs:782` → `:874` | `effective_permissions` 返回 `BTreeSet<String>`（天然升序去重），`into_iter()` 保序 |
| 展开通配（与 `require_permission` 同一套逻辑） | `crates/platform-rbac/src/service.rs:874` | `expand_user_permissions` —— **没有**另写一套展开逻辑 |
| 401 `UNAUTHENTICATED` | `crates/platform-rbac/src/http.rs:196` | `authenticate` 里无 bearer 令牌即 401 |
| `instance` 是**完整路径** | `crates/platform-rbac/src/http.rs:176` | `original_path()` 取 `OriginalUri` 扩展（嵌套路由会剥前缀） |
| 404 `NOT_FOUND`（头租户 ≠ 令牌租户） | `crates/platform-rbac/src/http.rs:214` + `:262` | **新增**：`x-tenant-id` 请求头现在会被读到（见 §4 问题 1） |
| **不引入新权限点** | — | manifest 未改 → 不触发 C-5 扫描与 D27 启动对账 |

### AC-S-065 —— 登录响应带上权限点

| 步骤 / 预期 | 实现位置 | 说明 |
|---|---|---|
| 登录响应的 `permissions` 填**真实**集合 | `crates/platform-iam/src/service.rs:621` | `issue_session` 里 `self.effective_permissions(ctx, &user.id).await` |
| provider 注入（后设接口） | `crates/platform-iam/src/service.rs:271` | `IamService::set_permission_provider(Arc<dyn PermissionProvider>) -> bool` |
| 可选槽位（`OnceLock`） | `crates/platform-iam/src/service.rs:210` | `PermissionProviderSlot` —— 注入**一次**、并发可读 |
| **降级**：未注入 → `[]`，登录**仍成功** | `crates/platform-iam/src/service.rs:283` | provider 缺失 / 读库失败 → 空数组 + `warn` 日志（**不**让登录失败） |
| 两个入口**同一答案** | 两个入口都走 `RbacService::effective_permissions` | `platform-iam` 通过 trait，`platform-rbac` 通过固有方法 —— **同一个实现** |
| 依赖方向（iam **不**依赖 rbac） | `crates/platform-iam/Cargo.toml` | IAM 只依赖 `platform-core` 的 trait；`platform-rbac` → `platform-iam` 单向 |
| 装配 | `crates/platform-server/src/main.rs:326` | `let permission_provider: Arc<dyn PermissionProvider> = rbac_state.service.clone();` → `iam_state.service.set_permission_provider(...)` |

---

## 3. 实际跑出的数字（真实输出，不是「应该通过」）

### 3.1 依赖预算核对

```
$ grep -c '^\[\[package\]\]' Cargo.lock
295
$ git diff --stat Cargo.lock
（空 —— Cargo.lock 一字未改）
```

**✅ `Cargo.lock` 包数仍为 295，未增长**（本次**没有**新增任何 Rust 依赖：
trait 用 `std::future::Future` / `std::pin::Pin`，`std::sync::OnceLock` 是标准库）。

### 3.2 契约 / 规范检查

```
$ cargo xtask capabilities --check
能力索引校验通过：13 个文件与源码一致

$ cargo xtask check-deps
依赖方向校验通过：平台 crate 未反向依赖业务模块

$ cargo xtask check-modules
模块目录校验通过：1 个模块
  ✓ sandbox

$ cargo fmt --all --check
（无输出 = 通过）
```

### 3.3 Clippy

```
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.12s
```

**✅ 0 警告**。

### 3.4 集成测试（10 条用例逐条）

```
$ DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
  cargo test -p platform-rbac --test me_permissions -- --test-threads=2 --include-ignored

running 10 tests
test tc_011_001_the_response_is_an_object_with_exactly_one_key ... ok
test tc_011_002_the_permission_set_is_exactly_the_granted_one ... ok
test tc_011_003_without_a_token_it_is_401_unauthenticated ... ok
test tc_011_004_another_tenant_in_the_header_is_404_not_found ... ok
test tc_011_005_the_permission_array_is_sorted_and_deduplicated ... ok
test tc_011_006_the_login_response_carries_the_real_permission_set ... ok
test tc_011_007_a_user_without_roles_gets_an_empty_permission_array ... ok
test tc_011_008_both_entry_points_give_the_same_answer ... ok
test tc_011_009_a_wildcard_grant_is_expanded ... ok
test tc_011_010_without_a_provider_login_still_succeeds_with_an_empty_array ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.60s
```

### 3.5 全工作区（`scripts/test-integration.sh`，退出码 0）

```
$ ./scripts/test-integration.sh
...
容器数量比对通过：2 → 2（未新建容器）
（退出码 0）
```

逐二进制 `test result:` 行（`cargo test --workspace --tests -- --test-threads=2 --include-ignored`）：

| 测试二进制 | 结果 |
|---|---|
| `platform_config`（lib） | ok. **30 passed**; 0 failed |
| `branding_static` | ok. **4 passed**; 0 failed |
| `config_real_db` | ok. **25 passed**; 0 failed |
| `contract_c5`（config） | ok. **3 passed**; 0 failed |
| `http_authz`（config） | ok. **10 passed**; 0 failed |
| `platform_contracts` | ok. **19 passed**; 0 failed |
| `platform_core`（lib） | ok. **78 passed**; 0 failed |
| `authz_entry` | ok. **7 passed**; 0 failed |
| `platform_db`（lib） | ok. **35 passed**; 0 failed |
| `quota_real_db` | ok. **2 passed**; 0 failed |
| `platform_iam`（lib） | ok. **41 passed**; 0 failed |
| `iam_real_db` | ok. **16 passed**; 0 failed |
| `platform_org`（lib） | ok. **68 passed**; 0 failed |
| `org_real_db` | ok. **15 passed**; 0 failed |
| `scim_outbound` | ok. **7 passed**; 0 failed |
| `platform_rbac`（lib） | ok. **42 passed**; 0 failed |
| `http_admin` | ok. **3 passed**; 0 failed |
| **`me_permissions`（本次新增）** | ok. **10 passed**; 0 failed |
| `rbac_real_db` | ok. **22 passed**; 0 failed |
| `platform_server`（lib） | ok. **38 passed**; 0 failed |
| `base`（main） | ok. **8 passed**; 0 failed |
| `sandbox`（lib） | ok. **3 passed**; 0 failed |
| `c3_real_db` | ok. **4 passed**; 0 failed |
| `contracts`（sandbox 契约） | ok. **5 passed**; 0 failed |
| `http_authz`（sandbox） | ok. **6 passed**; 0 failed |
| `xtask`（lib） | ok. **49 passed**; 0 failed |
| `platform_contracts_macros` / `xtask`（main） / `platform_core`（bin） | ok. **0 passed**; 0 failed |

**合计：550 passed；0 failed；0 ignored。**

### 3.6 ⚠️ 关于「任务书给的命令」

任务书写的：

```bash
cargo test --workspace -- --test-threads=2
```

**在本仓会失败**，但**与本次改动无关**（既有问题）：
`crates/platform-config/tests/config_real_db.rs` 的 24 条用例**没有** `#[ignore]`，
它们要求 `DATABASE_URL_TEST` 环境变量，缺了就 **panic**（「缺少 DATABASE_URL_TEST：真实库用例必须真的连库跑，不能静默跳过」）。

实测输出：

```
---- the_permission_registry_matches_every_manifest stdout ----
thread '...' panicked at crates/platform-config/tests/common/mod.rs:54:13:
缺少 DATABASE_URL_TEST：真实库用例必须真的连库跑，不能静默跳过。
...
test result: FAILED. 0 passed; 24 failed; 1 ignored; 0 measured; 0 filtered out
```

**结论**：本仓的正确跑法是 `scripts/test-integration.sh`
（它设 `DATABASE_URL_TEST` + `--include-ignored` + 重置 `base_test` + 比对容器数量），
本报告 §3.5 用的是它，**退出码 0**。
带环境变量重跑同一条命令也是全绿（§3.5 的表）。

### 3.7 ⚠️ 与前端 Dev 在**同一个 worktree** 并行作业

提交时发现前端 Dev 正在**同一个目录**改 `packages/sdk/src/index.ts`（未提交）。
两个后果与处理：

| 后果 | 处理 |
|---|---|
| `cargo xtask capabilities` 会把**前端未提交的源码**写进 `capabilities/sdk/sdk.json` | 生成能力索引时**临时**把 `packages/sdk/src/index.ts` 换回 HEAD 版本，生成后**原样恢复**（用 sha256 前后比对确认字节一致）。**本次提交不含任何 `packages/` 改动** |
| 工作区里 `cargo xtask capabilities --check` 目前**会红**（`sdk/sdk.json` 与 `index.json` 的指纹过期） | 那是**前端未提交源码**造成的，不是本次改动。**本次提交的状态**（HEAD 前端源码 + 本次后端改动）校验**通过**：「能力索引校验通过：13 个文件与源码一致」。前端 Dev 提交包源码后**需要自己重跑一次** `cargo xtask capabilities`（他们本来就要做） |

### 3.8 真机冒烟（`platform-server` 真实进程）

集成测试是**直接驱动 axum 路由**的（与 `crates/platform-rbac/tests/http_admin.rs` 同一套夹具模式），
它**手动**注入 provider —— 所以**装配那一段**（`platform-server/main.rs`）没有被集成测试覆盖。
为补上这块证据，起了**真实的 `base serve` 进程**（应用角色连接
`postgres://base_app:base@127.0.0.1:5432/base_test`）实测：

```
=== 注册 === status=201
=== 登录（未绑角色）=== status=200   login.permissions = []   has key = True
=== 绑定 rbac:role:read 后重新登录 === status=200   login.permissions = ['rbac:role:read']

=== GET /api/rbac/me/permissions ===
status=200
{"permissions":["rbac:role:read"]}

=== 不带令牌 ===
status=401
{"type":"https://errors.base.example/unauthenticated","title":"未认证","status":401,
 "detail":"authentication is required","instance":"/api/rbac/me/permissions",
 "code":"UNAUTHENTICATED","trace_id":"aee12d07-..."}

=== 头租户 ≠ 令牌租户（x-tenant-id: other-tenant） ===
status=404
{"type":"https://errors.base.example/not-found","title":"资源不存在","status":404,
 "detail":"the resource was not found","instance":"/api/rbac/me/permissions",
 "code":"NOT_FOUND","trace_id":"2795cdd0-...","params":{"resource":"resource"}}
```

→ **登录响应带权限点**（装配生效）、**响应体是对象**、**401/404 的 `instance` 是完整路径**、
**404 体不含 `permissions` 字段**，全部在真实进程上确认。
冒烟数据与临时文件已清理（`smoke-tenant` 的行已删、`/tmp/req011-smoke` 已删、进程已停）。

---

## 4. 自验发现的问题（不只报好消息）

### 问题 1（已修）：`/api/rbac` **从不读** `x-tenant-id` 请求头 → TC-011-004 原本不可达

- **现象**：`platform-rbac/src/http.rs` 的租户**只**来自 `RequestContext` 扩展，
  而 `platform-server` **没有**全局租户中间件（只有 `platform-iam` 有 `tenant_context`），
  所以生产环境里 `/api/rbac/*` 的租户**永远**从令牌推断 —— `x-tenant-id` 被完全忽略。
- **后果**：AC-S-064 步骤 6 / TC-011-004（「头租户 ≠ 令牌租户 → 404」）**根本不可达**；
  代码注释（`http.rs:186`「请求头租户与令牌租户不一致 → 404」）与实际行为**不一致**。
- **处理**：在 `authenticate` 中间件里读 `x-tenant-id`（契约 C3「请求头优先，缺失则由令牌推断」），
  `crates/platform-rbac/src/http.rs:214`。**这不是改需求** —— 契约 C3、
  AC-S-064 步骤 6、BA 引用的 `http.rs:186-189` 都要求这个行为，代码只是没实现。
- **影响面**：所有 `/api/rbac/*` 路由。**头租户与令牌租户一致时行为不变**；
  不一致时从「静默忽略」变成「404」（契约要求的行为）。
  仓里没有任何 e2e / 前端代码依赖旧的宽松行为（已 grep 确认）。

### 问题 2（已修）：错误体 `instance` 在嵌套路由里**少了前缀**

- **现象**：`Router::nest` 会剥掉前缀（axum 的 `StripPrefix`），
  `request.uri().path()` 在 `/api/rbac` 里是 `/me/permissions`，
  而 TC-011-003 要求 `instance == '/api/rbac/me/permissions'`。
  同一个文件里 `require_permission`（`platform-core`）**已经**用 `OriginalUri` 取完整路径 ——
  即同一个模块里两种错误体的 `instance` 口径不一致（403 是全路径，401/404 是剥过的）。
- **处理**：新增 `original_path()`（`crates/platform-rbac/src/http.rs:176`），
  `authenticate` 与 `Authenticated` 提取器统一用它。现有 rbac 测试**没有**断言 `instance`，无回归。

### 问题 3（已修）：`LoginResponse.permissions` 的注释与代码不符

- 原注释写「本期恒为空 —— RBAC 属 REQ-009」，改完后已同步更新为真实语义
  （登录时填充 / 未注入是空数组 / 刷新响应恒为空）。

### 问题 4（**未做，有意**）：刷新（`refresh`）响应**仍不带**权限点

- **现状**：`platform-iam/src/service.rs:756` 的刷新响应仍是 `permissions: Vec::new()`。
- **原因**：任务书与 AC-S-065 明确「**只改 login**」。
- **风险（请前端 Dev 与 BA 知悉）**：SDK 刷新令牌后如果用刷新响应**覆盖**登录时拿到的权限点，
  用户的权限点会被清空（fail closed → 前端所有按钮消失）。SDK 侧应当
  **保留登录时的那一份**，或改调 `GET /api/rbac/me/permissions` 重新取。
  已在代码注释里写明（`platform-iam/src/service.rs:754`）。

### 问题 5（**未做**）：`platform-org` 也有同一个「不读租户头」的缺口

- `platform-org/src/http.rs` 的 `Authenticated` 同样只看 `RequestContext` 扩展。
  本期**不在 REQ-011 范围内**，**没有**动它（避免扩围）。
  建议后续需求单独处理。

### 问题 6（提示，非缺陷）：`capabilities/extension-points.json` 变了

- 新增 trait 会进能力索引 —— `kind = "platform-capability"`（与 `PermissionChecker` /
  `PermissionRefresher` / `ConfigResolver` 同款）。
- **`extension_point_count` 仍是 5**（已核对：`capabilities/extension-points.json:3`），
  契约 `extension-points v2` **不需要升版本**。
- 已跑 `cargo xtask capabilities` 重新生成（`cargo xtask capabilities --check` 通过）。

---

## 5. 变异验证（注释里凡是「也覆盖 X」的地方都有支撑）

本次改了 4 处关键行为，逐个**故意改坏**再跑用例，确认用例**真的会红**：

| # | 变异 | 期望失败的用例 | 实测 |
|---|---|---|---|
| 1 | `authenticate` 不读 `x-tenant-id` | TC-011-004 | **FAILED**：`跨租户必须是 404：{"permissions":["rbac:role:read"]}` left: 200, right: 404 |
| 2 | `/me/permissions` 返回**裸数组** | TC-011-001 | **FAILED**：`响应体必须是对象（不是裸数组）：["rbac:role:read"]` |
| 3 | 登录响应的权限点**置空**（模拟「另一套展开逻辑」） | TC-011-006 / TC-011-008 | **FAILED**：`权限点集合不得为空` / `两个入口必须给出同一答案 left: [] right: ["rbac:role:read"]` |
| 4 | provider 未注入 → **登录失败**（用例点名要抓的反模式） | TC-011-010 | **FAILED**：`未注入 provider 时登录**仍然成功**: Internal { reason: "the permission provider is not installed" }` |

4 次变异全部被对应的用例抓住，随后已全部还原（`git diff` 复核过）。

---

## 6. 没做的部分 + 原因

| 没做 | 原因 |
|---|---|
| **前端包**（`@base/sdk` / `@base/auth` / `<Can>` / 路由守卫 / 主题令牌 / 品牌） | 本棒只做**后端部分**；前端由另一个 Dev 接手 |
| **`refresh` 响应带权限点** | 任务书与 AC-S-065 明确「只改 login」；见问题 4（附风险提示） |
| **`platform-org` 的租户头缺口** | 不在 REQ-011 范围（见问题 5），避免扩围 |
| **`AGENTS.md` 的「已有能力 / 尚未提供」两张表** | `permission_can` 那一行属**前端** Dev 的活；`cargo test -p xtask` 的 `agents_doc_matches_catalog` 要求表与 `crates/xtask/src/catalog.rs` **逐字一致**，两边同时改才有意义 → 本次只加了「鉴权怎么写」小节（规则 1 要求引导文件与实际一致），**没动**那两张表与 catalog。**请前端 Dev 改 `permission_can` 时，`crates/xtask/src/catalog.rs` 与 `AGENTS.md` 必须一起改**，否则 `agents_doc_matches_catalog` 会红 |
| **端点写进 `docs/contracts/api-conventions`** | 本期**不引入**新权限点 → 契约正文无变化；契约正本在 BA 分支，由 BA 处理 |
| **`platform-server` 的装配做端到端自动化测试** | 装配在 `main.rs` 的 `serve()` 里，起真实进程做集成测试成本高。改为**真机冒烟**（§3.8）取证 |

---

## 7. 约束核对表

| # | 约束 | 结果 |
|---|------|------|
| 1 | `Cargo.lock` 包数不得增长（295） | ✅ 仍为 **295**，`git diff Cargo.lock` 为空 |
| 2 | `platform-iam` 不得依赖 `platform-rbac` | ✅ IAM 只用 `platform-core` 的 trait；`cargo xtask check-deps` 通过 |
| 3 | 业务模块不得反向依赖平台模块 | ✅ `check-deps` 通过；本次**未动** `modules/` |
| 4 | `cargo test` 只能有一个 `--` | ✅ 本报告所有命令都是**一个** `--` |
| 5 | 复用 target | ✅ `CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target` |
| 6 | 应用连接用普通角色 `base_app` | ✅ 冒烟用 `postgres://base_app:base@127.0.0.1:5432/base_test`；集成测试的夹具也把管理员连接换成应用角色 |
| 7 | 本机无 psql → `podman exec base-postgres psql` | ✅ 全程用 `podman exec`（**注意**：喂 heredoc 必须加 `-i`，否则容器收不到 stdin —— 我踩过一次，冒烟第一遍写库是空操作） |
| 8 | 容器按需起 | ✅ `base-postgres` / `base-redis` 本来就在跑，**未新建容器**（脚本的容器数量比对 `2 → 2` 通过） |
| 9 | 注册密码过策略 | ✅ `Tr0ub4dor&3xKq`（15 字符，不在常见口令词库） |
| 10 | `cargo clippy --workspace --all-targets -- -D warnings` 0 警告 | ✅ 通过 |
| 11 | 注释里的「也覆盖 X」要有变异验证 | ✅ 4 处关键行为做了变异验证（§5），并**修掉了**两处注释与代码不符（问题 2、3） |

---

## 8. 给下一棒 / BA 的交接

1. **前端 Dev**：`GET /api/rbac/me/permissions` → `{"permissions":[...]}`（**对象**，不是裸数组）；
   登录响应里也有同一份 `permissions`（**未注入时是 `[]`**）。
   **刷新响应不带权限点** —— 别用刷新响应覆盖登录时的那一份（问题 4）。
2. **BA**：`/api/rbac` 现在会读 `x-tenant-id`（契约 C3）。头租户与令牌租户不一致 → **404**。
   若前端某处会带**错误的**租户头，会从「静默忽略」变成 404 —— 这是契约要求，但属于行为变化，值得在验收里留个心。
3. **BA**：`platform-org` 的同一个缺口没动（问题 5）。
4. **AGENTS.md / `crates/xtask/src/catalog.rs`**：前端 Dev 改 `permission_can` 时**必须两边一起改**
   （`agents_doc_matches_catalog` 会逐字比对）。
