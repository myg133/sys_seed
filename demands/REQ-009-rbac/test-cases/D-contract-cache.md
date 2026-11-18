# D. 契约检查 C-5 与缓存主动失效 —— 测试用例（AC-S-046、AC-S-047）

- **需求**: REQ-009「RBAC 权限与数据范围」
- **来源**: [acceptance.md](../acceptance.md) §AC-S-046 / AC-S-047、[design-summary.md](../design-summary.md) §6.1 / §6.3 / §7、[extension-points v2 §4](../../../contracts/extension-points/v2.md)
- **用例数**: 9（TC-009-053 ~ TC-009-061）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## D.0 本域约定

- **契约检查执行方式**：模块契约测试由 `#[platform_module_contract]` 宏生成（`modules/<name>/tests/contracts.rs`），跑法 `cargo test -p <module> -- --test-threads=2`；C-5 必须注册进**同一套件**（与 C-1~C-4 并列；宏现在生成 4 个测试，要变 5 个）。
- **C-5 判定口径**（extension-points v2 §4）：

  | 情况 | 判定 |
  |---|---|
  | 已登记，且代码里有对应权限点的鉴权调用点 | ✅ 通过 |
  | 已登记，但代码里没有任何鉴权调用点 | ❌ **失败**（登记了却不执行 = 假权限） |
  | 代码里有鉴权调用点，但 manifest 未登记 | ❌ 失败（C-2 已覆盖） |
  | 鉴权调用点的参数**不是字面量**（无法静态确认） | ❌ **失败**（无法确认 → 不放行，fail closed） |
  | 出现**第二个**鉴权入口 | ❌ **失败**（扫描会被绕过） |

- **唯一鉴权入口**：`require_permission("<code>")`，**定义在 `crates/platform-core/src/extension/permission.rs`**（**不在** `platform-rbac` —— 业务模块只能依赖 `platform-core` + `platform-db`，design §6.1 / R10）。现有 C-2 的扫描实现见 `crates/platform-contracts/src/manifest_scan.rs`（读 `backend/**/*.rs` 的字符串字面量，挑出 `<module>:<resource>:<action>` 形态），C-5 在其之上新增。
- **入口唯一性的判定口径**（AC-S-046 第 5 步）：**全仓搜索鉴权入口的「定义处」**（不是调用处）—— 定义处必须**只有一个**。
- **负向夹具做法**：C-5 的违规用例在 `crates/platform-contracts` 的测试里用**临时目录**构造违规模块（临时目录内放 `manifest.json` + `backend/*.rs`），跑 C-5 检查并断言失败 —— **不改仓库里的模块源码**，避免污染 CI。
- **缓存与 epoch**（design §7 / §7.1）：平台级表 `platform_permission_epoch(tenant_id TEXT PRIMARY KEY, epoch BIGINT NOT NULL, updated_at TIMESTAMPTZ)`；任何角色 / 权限绑定 / 角色继承 / 数据范围的写操作在同一事务里 `epoch = epoch + 1`；缓存 TTL 60s，但**写操作必须主动失效**。
- **跨实例口径**（AC-S-047）：用**两个独立的 checker 实例**（各自持自己的进程内缓存、共享同一个 `base_test` 库）模拟两个进程 —— 不必真起两个服务进程。
- **权限点选取**：「撤销授权」类用例用 `sandbox:demo:read`（AC-S-047 原文所写）+ 路由 `GET /api/sandbox/demo`（`sandbox_tenant_item` 夹具里有 3 条记录）；「数据范围 / 组织成员」类用例用 `sandbox:scope-item:read` + 路由 `GET /api/sandbox/scope-item`（同一套机制、不同权限点）。
- **跑法**：C-5 用例为**单元测试**（`crates/platform-contracts`）；缓存用例为**真实库（`base_test`）+ 模块内进程内 HTTP**（`ServiceExt::oneshot`）。

---

### TC-009-053: manifest 登记了代码里不存在的权限点 → C-5 失败并点名
- **关联 AC**: AC-S-046
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-contracts`）。
  - 临时目录夹具：`manifest.json` 声明权限点 `neg:thing:read` 与 `neg:thing:ghost`；`backend/src/lib.rs` 中**只有** `require_permission("neg:thing:read")`，**没有** `neg:thing:ghost` 的调用点。
  - C-5 检查已注册进契约套件。
- **测试步骤**:
  1. 对该临时目录夹具跑契约套件。
  2. 读取报告中的违例列表（`SuiteReport.violations`）。
  3. 核对违例的 `check_id` 与 `message` 内容。
- **预期结果**:
  1. 套件**不合规**（`is_compliant() == false`），C-5 对应的失败数为 **1**（只针对 `neg:thing:ghost`）。
  2. 违例的 `check_id` 为 `C-5`。
  3. 违例 `message` 中**同时出现**权限点编码 `neg:thing:ghost` 与「已登记但无鉴权调用点」的含义；**不得**把 `neg:thing:read`（有调用点）也报成违例。**不得**降级为 warning（`ContractWarning` 不算失败）。
- **类型**: 安全 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 负向用例（假权限）+ 精确违例断言（点名 + 不误报）

### TC-009-054: 模块自写鉴权函数绕过平台入口 → C-5 失败
- **关联 AC**: AC-S-046
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-contracts`）。
  - 临时目录夹具：`manifest.json` 声明 `neg:thing:read`；`backend/src/lib.rs` 中写了一个**自定义**鉴权函数 `fn check_perm(user: &User, code: &str) -> bool { ... }`，并在处理函数里调用 `check_perm(user, "neg:thing:read")` —— **没有任何** `require_permission(...)` 调用。
- **测试步骤**:
  1. 对该夹具跑契约套件。
  2. 读取违例列表。
  3. 核对违例内容是否点名 `neg:thing:read`。
- **预期结果**:
  1. 套件**不合规**，C-5 失败数 = 1。
  2. 违例 `check_id` = `C-5`，`message` 中出现 `neg:thing:read`。
  3. 违例原因明确表达「该权限点没有通过平台鉴权入口的调用点」（即扫描不到）—— 不得因为「代码里确实出现了这个编码」就放行。
- **类型**: 安全 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（自建入口绕过扫描）+ 负向用例

### TC-009-055: 非字面量参数调用鉴权入口 → C-5 失败（fail closed）
- **关联 AC**: AC-S-046
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-contracts`）。
  - 临时目录夹具：`manifest.json` 声明 `neg:thing:read`；`backend/src/lib.rs` 中调用 `require_permission(&perm_code)`，其中 `perm_code` 来自运行时变量（**非字面量**）。
- **测试步骤**:
  1. 对该夹具跑契约套件。
  2. 读取违例列表。
  3. 核对违例是否点名 `neg:thing:read` 且原因指向「参数非字面量」。
- **预期结果**:
  1. 套件**不合规**，C-5 失败数 = 1。
  2. 违例 `check_id` = `C-5`，`message` 中出现 `neg:thing:read`。
  3. 违例原因明确表达「无法静态确认（非字面量参数）」；**不得**判为「跳过 / 忽略 / 视为通过」，也**不得**降级为 warning。
- **类型**: 安全 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（静态不可确认）+ fail closed 断言（不得跳过）

### TC-009-056: 出现第二个鉴权入口（**定义处**不唯一）→ C-5 失败
- **关联 AC**: AC-S-046
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-contracts`）。
  - 临时目录夹具：`manifest.json` 声明 `neg:thing:read`；`backend/src/lib.rs` 中既有 `require_permission("neg:thing:read")`，又出现了**第二个**鉴权入口的定义（如自定义的 `authorize(...)` 函数，或平台入口的另一个别名/包装函数）。
- **测试步骤**:
  1. 对该夹具跑契约套件。
  2. 读取违例列表。
  3. **全仓搜索**鉴权入口的**定义处**（`fn require_permission` / `fn authorize` / `fn check_perm` 等定义形态，**不是**调用形态），统计命中处数。
  4. 核对违例是否指向「入口不唯一」并点名第二个定义处。
- **预期结果**:
  1. 套件**不合规**，C-5 失败数 ≥ 1。
  2. 存在一条违例，其原因表达「鉴权入口不唯一 / 出现第二个入口」。
  3. 该违例**点名**第二个入口**定义处**的**文件路径**（或函数名 + 路径）—— 不得只说「存在多个入口」；全仓搜索的命中处数必须与「入口应当只有一个」的口径一致（平台入口 `crates/platform-core/src/extension/permission.rs` 里的那**一处**）。
- **类型**: 安全 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（入口唯一性是扫描可信的前提）+ 负向用例 + 定义处计数

### TC-009-057: 正常模块（既登记又有调用点）→ 通过；`sandbox` 接上真实鉴权后 C-5 通过
- **关联 AC**: AC-S-046
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-contracts`）+ 模块契约测试（`cargo test -p sandbox -- --test-threads=2`）。
  - `modules/sandbox/manifest.json` 声明 `sandbox:demo:read` / `sandbox:demo:write` / `sandbox:scope-item:read` / `sandbox:scope-item:approve` / `sandbox:scope-item:export`；**当前** `modules/sandbox/backend/src/permission.rs` 只定义常量、**从不鉴权**（v1 下 C-2 全绿）。
  - REQ-009 已为 `sandbox` 接入 `require_permission("sandbox:demo:read")` 等**字面量**调用点（路由在 `modules/sandbox/backend/src/http.rs`，由 sandbox 自己的测试用 `ServiceExt::oneshot` 驱动）。
- **测试步骤**:
  1. 跑 `cargo test -p sandbox -- --test-threads=2`，记录契约套件的 C-5 结果。
  2. 核对 manifest 里的每个权限点在代码中各有**字面量**调用点。
  3. 核对 C-1 ~ C-4 的判定口径**未变**。
- **预期结果**:
  1. `sandbox` 的契约套件**通过**（C-5 失败数 = 0）。
  2. 每个已登记权限点各有一个**字面量**调用点；扫描结果与 manifest 声明**逐条相等**。
  3. C-2 仍只对「manifest 声明了代码没用到的权限点」给 **warning**（不失败）；C-1 / C-3 / C-4 的判定口径与 v1 一致。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径（负向夹具的镜像）+ 回归断言（C-1~C-4 口径不变）

### TC-009-058: 撤销角色授权后，下一个请求立即被拒（不等 60s TTL）
- **关联 AC**: AC-S-047
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 模块内进程内 HTTP。
  - 账号 `u-1001`（`owner@example.com`）绑定角色 `rbac-owner`，该角色含 `sandbox:demo:read`。
  - `sandbox_tenant_item` 夹具里有 3 条记录，`GET /api/sandbox/demo` 正常返回 3 条。
  - 平台进程内缓存已存在 `u-1001` 的权限集合（先调一次即可生成）。
- **测试步骤**:
  1. 调 `GET /api/sandbox/demo`，记录状态码与**时刻 T1**。
  2. 调 `DELETE /api/rbac/roles/{rbac-owner id}/permissions/sandbox:demo:read` 撤销授权。
  3. **立刻**（不等待、不重启、不清缓存）再调同一接口，记录状态码与**时刻 T2**。
  4. 用管理员连接查 `platform_permission_epoch` 中 `tenant-a` 的 `epoch`，与步骤 2 之前的值比较。
- **预期结果**:
  1. 第 1 步返回 HTTP **200**（撤销前可用，响应体含 3 条记录）。
  2. 第 2 步返回 HTTP 204/200。
  3. 第 3 步返回 HTTP **403** + `code = INSUFFICIENT_PERMISSION`（**不得** 200、**不得** 404 —— 记录存在且在范围内，拒绝原因是**缺权限点**）；且 `T2 - T1 < 2 秒`（证明没有等 60s TTL）。
  4. `platform_permission_epoch` 中 `tenant-a` 的 `epoch` 比步骤 2 之前**恰好 +1**（写操作与 epoch 递增在同一事务内完成）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（撤销后残留可用）+ 时序断言（不是等 TTL）+ epoch 旁证

### TC-009-059: 收紧角色的数据范围后立即生效
- **关联 AC**: AC-S-047
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 模块内进程内 HTTP。
  - 账号 `u-1001` 绑定角色 `rbac-owner`（`data_scope = OWN_ORG`）；`item-a1`（`org_node_id = dept-a`）与 `item-big`（`org_node_id = dept-a`）都在其范围内。
- **测试步骤**:
  1. 调 `GET /api/sandbox/scope-item`（`limit=200`），记录条数与集合 S0（应为 2 条：`item-a1`、`item-big`）。
  2. 把角色 `rbac-owner` 的 `dataScope` 改为 `SELF`（`POST /api/rbac/roles/{id}`，body `{"dataScope":"SELF"}`）。
  3. **立刻**再调同一接口，记录条数与集合 S1。
  4. 调 `GET /api/sandbox/scope-item/item-big` 记录状态码。
- **预期结果**:
  1. 第 1 步返回 HTTP 200，S0 **逐条列出** = `item-a1`、`item-big`。
  2. 第 2 步返回 HTTP 204/200；`platform_permission_epoch` 中 `tenant-a` 的 `epoch` +1。
  3. 第 3 步返回 HTTP 200，S1 **逐条列出** = `item-a1`（`item-big` **不可见** —— 撤销**数据范围**同样立即生效）。
  4. 第 4 步返回 HTTP **404** + `code = NOT_FOUND`（记录已不在范围内）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（范围变更 → 可见集变化）+ 集合精确断言

### TC-009-060: 改组织成员关系（主部门变更）后立即生效
- **关联 AC**: AC-S-047
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 模块内进程内 HTTP。
  - `user_org` 中 `u-1001` 当前主部门为 `dept-a`（`is_primary = TRUE`）；角色 `rbac-owner` 的 `data_scope = OWN_ORG`。
  - `item-a1`（`org_node_id = dept-a`）、`item-c1`（`org_node_id = dept-c`）。
- **测试步骤**:
  1. 调 `GET /api/sandbox/scope-item`（`limit=200`），记录集合 S0。
  2. 调 `POST /api/org/user-org`，body `{"userId":"u-1001","orgNodeId":"dept-c","isPrimary":true}`（REQ-008 的加成员逻辑会清掉旧主部门）。
  3. **立刻**再调 `GET /api/sandbox/scope-item`，记录集合 S1。
  4. 用管理员连接查 `user_org` 中 `u-1001` 的 `is_primary = TRUE` 行数与 `org_node_id`。
- **预期结果**:
  1. 第 1 步返回 HTTP 200，S0 **逐条列出** = `item-a1`、`item-big`。
  2. 第 2 步返回 HTTP 201；`platform_permission_epoch` 中 `tenant-a` 的 `epoch` +1（组织变更也触发范围缓存失效）。
  3. 第 3 步返回 HTTP 200，S1 **逐条列出** = `item-c1` —— 主部门已切到 `dept-c`，`item-a1`/`item-big` **立即不可见**。
  4. `user_org` 中 `u-1001` 的 `is_primary = TRUE` 行**恰好 1 条**且 `org_node_id = dept-c`。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（主部门变更 → 范围变更）+ 集合精确断言

### TC-009-061: 两个独立 checker 实例：实例 1 撤销 → 实例 2 立刻调也被拒（跨进程）
- **关联 AC**: AC-S-047
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 两个独立 checker 实例。
  - **实例 1** 与**实例 2** 是两个各自持**自己的进程内缓存**的 `PermissionChecker` 实现，**共享同一个 `base_test` 库**（用来模拟两个进程；不必真起两个服务进程）。
  - 账号 `u-1001` 绑定角色 `rbac-owner`（含 `sandbox:demo:read`）；`GET /api/sandbox/demo` 正常返回 3 条记录。
  - 两个实例此前**各自**都调过一次该接口（各自缓存里都是「有权」）。
- **测试步骤**:
  1. 用**实例 1** 调 `GET /api/sandbox/demo`，记录状态码与**时刻 T1**。
  2. 用**实例 1** 撤销 `u-1001` 的角色授权（`DELETE /api/rbac/roles/{rbac-owner id}/permissions/sandbox:demo:read`）。
  3. **立刻**用**实例 2**（其缓存里**仍是旧值**）调同一接口，记录状态码与**时刻 T2**。
  4. 再用**实例 1** 调同一接口，记录状态码。
  5. 用管理员连接查 `platform_permission_epoch` 中 `tenant-a` 的 `epoch`，并确认两个实例读到的 epoch **相同**。
- **预期结果**:
  1. 第 1 步返回 HTTP **200**。
  2. 第 2 步返回 HTTP 204/200。
  3. 第 3 步返回 HTTP **403**（或 404，按 C5 口径），**不得** 200；且 `T2 - T1 < 2 秒` —— 证明**实例 2 的缓存不是失效原因**：失效信号来自**共享的数据库 epoch**，否则多实例部署下撤销不生效。
  4. 第 4 步同样被拒（403 / 404），与第 3 步一致。
  5. `platform_permission_epoch` 中 `tenant-a` 的 `epoch` 比撤销前 **+1**，且两个实例读到的 epoch **相同**（DB 是共享真相）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 跨进程一致性（epoch 收敛）+ 时序断言（不等 TTL）+ 缓存归属判别（实例 2 未失效却仍被拒）

---
