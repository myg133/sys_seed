# D. 契约检查 C-5 与缓存主动失效 —— 测试用例（AC-S-046、AC-S-047）

- **需求**: REQ-009「RBAC 权限与数据范围」
- **来源**: [acceptance.md](../acceptance.md) §AC-S-046 / AC-S-047、[design-summary.md](../design-summary.md) §6.3 / §7、[extension-points v2 §4](../../../contracts/extension-points/v2.md)
- **用例数**: 9（TC-009-051 ~ TC-009-059）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## D.0 本域约定

- **契约检查执行方式**：模块契约测试由 `#[platform_module_contract]` 宏生成（`modules/<name>/tests/contracts.rs`），跑法 `cargo test -p <module> -- --test-threads=2`；C-5 必须注册进**同一套件**（与 C-1~C-4 并列）。
- **C-5 判定口径**（extension-points v2 §4）：

  | 情况 | 判定 |
  |---|---|
  | 已登记，且代码里有对应权限点的鉴权调用点 | ✅ 通过 |
  | 已登记，但代码里没有任何鉴权调用点 | ❌ **失败**（登记了却不执行 = 假权限） |
  | 代码里有鉴权调用点，但 manifest 未登记 | ❌ 失败（C-2 已覆盖） |
  | 鉴权调用点的参数**不是字面量** | ❌ **失败**（无法静态确认 → fail closed） |
  | 出现**第二个**鉴权入口 | ❌ **失败**（扫描会被绕过） |

- **唯一鉴权入口**：平台提供的 `require_permission("<code>")`，参数必须是**字面量**权限点编码。现有 C-2 的扫描实现见 `crates/platform-contracts/src/manifest_scan.rs`（读 `backend/**/*.rs` 的字符串字面量，挑出 `<module>:<resource>:<action>` 形态），C-5 在其之上新增。
- **负向夹具做法**：C-5 的违规用例在 `crates/platform-contracts` 的测试里用**临时目录**构造违规模块（临时目录内放 `manifest.json` + `backend/*.rs`），跑 C-5 检查并断言失败 —— **不改仓库里的模块源码**，避免污染 CI。
- **缓存与 epoch**（design-summary §7）：平台级表 `platform_permission_epoch(tenant_id, epoch, updated_at)`；任何角色 / 权限绑定 / 角色继承 / 数据范围的写操作在同一事务里 `epoch = epoch + 1`；缓存 TTL 60s，但**写操作必须主动失效**。
- **跑法**：C-5 用例为**单元测试**（`crates/platform-contracts`）；缓存用例为**真实库（`base_test`）+ 真机 HTTP**，需要两个进程实例时用同一 `base_test` 库分别起两份 `base serve`。

---

### TC-009-051: manifest 登记了代码里不存在的权限点 → C-5 失败并点名
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

### TC-009-052: 模块自写鉴权函数绕过平台入口 → C-5 失败
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

### TC-009-053: 非字面量参数调用鉴权入口 → C-5 失败（fail closed）
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

### TC-009-054: 出现第二个鉴权入口 → C-5 失败
- **关联 AC**: AC-S-046
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-contracts`）。
  - 临时目录夹具：`manifest.json` 声明 `neg:thing:read`；`backend/src/lib.rs` 中既有 `require_permission("neg:thing:read")`，又出现了**第二个**鉴权入口（如自定义的 `authorize(...)` 函数，或平台入口的另一个别名/包装函数）。
- **测试步骤**:
  1. 对该夹具跑契约套件。
  2. 读取违例列表。
  3. 核对违例是否指向「入口不唯一」并点名第二个入口的位置。
- **预期结果**:
  1. 套件**不合规**，C-5 失败数 ≥ 1。
  2. 存在一条违例，其原因表达「鉴权入口不唯一 / 出现第二个入口」。
  3. 该违例**点名**第二个入口的**文件路径**（或函数名）—— 不得只说「存在多个入口」。
- **类型**: 安全 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（入口唯一性是扫描可信的前提）+ 负向用例

### TC-009-055: 正常模块（既登记又有调用点）→ 通过；`sandbox` 接上真实鉴权后 C-5 通过
- **关联 AC**: AC-S-046
- **前置条件**:
  - **执行位置**: 单元测试（`crates/platform-contracts`）+ 模块契约测试（`cargo test -p sandbox -- --test-threads=2`）。
  - `modules/sandbox/manifest.json` 声明 `sandbox:demo:read` / `sandbox:demo:write`；**当前** `modules/sandbox/backend/src/permission.rs` 只定义常量、**从不鉴权**（v1 下 C-2 全绿）。
  - REQ-009 已为 `sandbox` 接入 `require_permission("sandbox:demo:read")` 与 `require_permission("sandbox:demo:write")` 的**字面量**调用点。
- **测试步骤**:
  1. 跑 `cargo test -p sandbox -- --test-threads=2`，记录契约套件的 C-5 结果。
  2. 核对 `sandbox:demo:read` / `sandbox:demo:write` 在代码中各有**字面量**调用点。
  3. 跑 `cargo test -p demo -- --test-threads=2`（夹具模块 `modules/demo`），记录 C-5 结果。
- **预期结果**:
  1. `sandbox` 的契约套件**通过**（C-5 失败数 = 0）；C-1 ~ C-4 的判定口径**不变**（C-2 仍对「manifest 声明了代码没用到的权限点」给 warning）。
  2. 两个权限点各有一个**字面量**调用点；扫描结果与 manifest 声明**逐条相等**。
  3. `modules/demo` 的 C-5 **通过**（`demo:invoice:read` / `demo:invoice:approve` / `demo:invoice:export` 都有字面量调用点）。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径（负向夹具的镜像）+ 回归断言（C-1~C-4 口径不变）

### TC-009-056: 撤销角色授权后，下一个请求立即被拒（不等 60s TTL）
- **关联 AC**: AC-S-047
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP。
  - 账号 `u-1001`（`owner@example.com`）绑定角色 `rbac-owner`，该角色含 `demo:invoice:approve`。
  - `item-a1`（`amount = 5000`、`dept_id = org-dept-a`）在 `u-1001` 的 `OWN_ORG` 范围内。
  - 平台进程内缓存已存在 `u-1001` 的权限集合（先调一次即可生成）。
- **测试步骤**:
  1. 调 `POST /api/demo/invoice/approve`（body `{"id":"item-a1"}`），记录状态码与**时刻 T1**。
  2. 调 `DELETE /api/rbac/roles/{rbac-owner id}/permissions/demo:invoice:approve` 撤销授权。
  3. **立刻**（不等待、不重启、不清缓存）再调同一接口，记录状态码与**时刻 T2**。
  4. 用管理员连接查 `platform_permission_epoch` 中 `tenant-a` 的 `epoch`，与步骤 2 之前的值比较。
- **预期结果**:
  1. 第 1 步返回 HTTP **200**（撤销前可用）。
  2. 第 2 步返回 HTTP 204/200。
  3. 第 3 步返回 HTTP **403** + `code = INSUFFICIENT_PERMISSION`（**不得** 200、**不得** 404 —— 记录在范围内，拒绝原因是缺权限点）；且 `T2 - T1 < 2 秒`（证明没有等 60s TTL）。
  4. `platform_permission_epoch` 中 `tenant-a` 的 `epoch` 比步骤 2 之前**恰好 +1**（写操作与 epoch 递增在同一事务内完成）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（撤销后残留可用）+ 时序断言（不是等 TTL）+ epoch 旁证

### TC-009-057: 收紧角色的数据范围后立即生效
- **关联 AC**: AC-S-047
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP。
  - 账号 `u-1001` 绑定角色 `rbac-owner`（`data_scope = OWN_ORG`）；`item-a1`（`dept_id = org-dept-a`）与 `item-big`（`dept_id = org-dept-a`、`amount = 20000`）都在其范围内。
- **测试步骤**:
  1. 调 `GET /api/demo/invoice`（`limit=200`），记录条数与集合 S0（应为 2 条：`item-a1`、`item-big`）。
  2. 把角色 `rbac-owner` 的 `dataScope` 改为 `SELF`（`POST /api/rbac/roles/{id}`，body `{"dataScope":"SELF"}`）。
  3. **立刻**再调同一接口，记录条数与集合 S1。
  4. 调 `GET /api/demo/invoice/item-big` 记录状态码。
- **预期结果**:
  1. 第 1 步返回 HTTP 200，S0 = {`item-a1`, `item-big`}（条数 2）。
  2. 第 2 步返回 HTTP 204/200；`platform_permission_epoch` 中 `tenant-a` 的 `epoch` +1。
  3. 第 3 步返回 HTTP 200，S1 = {`item-a1`}（条数 **1**，`item-big` **不可见**）—— 范围收紧**立即**生效，未等 TTL。
  4. 第 4 步返回 HTTP **404** + `code = NOT_FOUND`（记录已不在范围内）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（范围变更 → 可见集变化）+ 集合精确断言

### TC-009-058: 改组织成员关系（主部门变更）后立即生效
- **关联 AC**: AC-S-047
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP。
  - `user_org` 中 `u-1001` 当前主部门为 `org-dept-a`（`is_primary = TRUE`）；角色 `rbac-owner` 的 `data_scope = OWN_ORG`。
  - `item-a1`（`dept_id = org-dept-a`）、`item-c1`（`dept_id = org-dept-c`）。
- **测试步骤**:
  1. 调 `GET /api/demo/invoice`（`limit=200`），记录集合 S0（应为 {`item-a1`, `item-big`}）。
  2. 调 `POST /api/org/user-org`，body `{"userId":"u-1001","orgNodeId":"org-dept-c","isPrimary":true}`（REQ-008 的加成员逻辑会清掉旧主部门）。
  3. **立刻**再调 `GET /api/demo/invoice`，记录集合 S1。
  4. 用管理员连接查 `user_org` 中 `u-1001` 的 `is_primary = TRUE` 行数与 `org_node_id`。
- **预期结果**:
  1. 第 1 步返回 HTTP 200，S0 = {`item-a1`, `item-big`}（条数 2）。
  2. 第 2 步返回 HTTP 201；`platform_permission_epoch` 中 `tenant-a` 的 `epoch` +1（组织变更也触发范围缓存失效）。
  3. 第 3 步返回 HTTP 200，S1 = {`item-c1`}（条数 **1**）—— 主部门已切到 `org-dept-c`，`item-a1`/`item-big` **立即不可见**。
  4. `user_org` 中 `u-1001` 的 `is_primary = TRUE` 行**恰好 1 条**且 `org_node_id = org-dept-c`。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（主部门变更 → 范围变更）+ 集合精确断言

### TC-009-059: 撤销结论跨进程成立（第二个实例立即看到失效）
- **关联 AC**: AC-S-047
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 两个真机 HTTP 实例。
  - 实例 **P1** 与实例 **P2** 连的是**同一个** `base_test` 库（不同端口），二者都已为 `u-1001` 生成过权限缓存（先各调一次 approve 接口）。
  - 账号 `u-1001` 绑定角色 `rbac-owner`（含 `demo:invoice:approve`）。
- **测试步骤**:
  1. 在 P1 上调 `POST /api/demo/invoice/approve`（body `{"id":"item-a1"}`）→ 期望 200；在 P2 上调同一接口 → 期望 200。
  2. **只在 P1 上**调 `DELETE /api/rbac/roles/{rbac-owner id}/permissions/demo:invoice:approve`。
  3. **不重启 P2**，立刻在 P2 上调同一接口，记录状态码与时刻。
  4. 用管理员连接查 `platform_permission_epoch` 中 `tenant-a` 的 `epoch`。
- **预期结果**:
  1. 两次均返回 HTTP 200（P2 的缓存此时仍是「有权」）。
  2. P1 的撤销返回 HTTP 204/200。
  3. P2 上的请求返回 HTTP **403** + `code = INSUFFICIENT_PERMISSION`（**不得** 200、**不得** 等到 60s TTL）；P2 的拒绝在撤销后 **2 秒内**发生 —— 证明 P2 通过读 DB epoch 发现缓存过期并重算（**不是**进程内广播）。
  4. `platform_permission_epoch` 中 `tenant-a` 的 `epoch` 比撤销前 +1，且 P1 与 P2 读到的 epoch **相同**（共享 DB 为真相）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 跨进程一致性（epoch 收敛）+ 时序断言（不等 TTL）

---
