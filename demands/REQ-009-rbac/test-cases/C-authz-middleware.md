# C. 鉴权中间件、错误体与 SQL 层过滤 —— 测试用例（AC-S-044、AC-S-045、AC-S-048）

- **需求**: REQ-009「RBAC 权限与数据范围」
- **来源**: [acceptance.md](../acceptance.md) §AC-S-044 / AC-S-045 / AC-S-048、[design-summary.md](../design-summary.md) §6 ~ §8、[api-conventions v1 C2/C5/C6/C7](../../../contracts/api-conventions/v1.md)
- **用例数**: 12（TC-009-041 ~ TC-009-052）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## C.0 本域约定

- **被保护路由**（夹具模块 `modules/sandbox`，契约 C2 路径形式）：

  | 路由 | 所需权限点 | 说明 |
  |---|---|---|
  | `GET /api/sandbox/scope-item` | `sandbox:scope-item:read` | 列表（叠加数据范围） |
  | `GET /api/sandbox/scope-item/{id}` | `sandbox:scope-item:read` | 详情（对象级越权判 404） |
  | `POST /api/sandbox/scope-item/approve` | `sandbox:scope-item:approve` | 动作（动作类越权判 403） |
  | `POST /api/sandbox/scope-item/export` | `sandbox:scope-item:export` | 动作（数据范围观察） |
  | `GET /api/sandbox/demo` | `sandbox:demo:read` | 沙箱原有示例（`sandbox:demo:write` 对应写动作） |

- **驱动方式**：模块路由**不由 platform-server 挂载**（design §8.1），sandbox 的用例在自己的测试里用 `ServiceExt::oneshot` **进程内驱动** axum Router；平台接口 `/api/rbac/*` 由 `platform-server` 挂载，走真机 HTTP。
- **鉴权入口**（平台唯一入口，design §6.1）：`require_permission("<code>")`，定义在 **`crates/platform-core/src/extension/permission.rs`**（**不在** `platform-rbac` —— 业务模块只能依赖 `platform-core` + `platform-db`），参数必须是**字面量**。
- **拒绝语义**（契约 C5 + design §6.2，**403 与 404 的分工**）：

  | 情形 | 谁判 | HTTP | `code` |
  |---|---|---|---|
  | 无令牌 / 令牌无效 | 认证层 | **401** | `UNAUTHENTICATED` |
  | 令牌有效但**根本没有**该权限点 | `require_permission` 层 | **403** | `INSUFFICIENT_PERMISSION` |
  | **有**权限点，但目标记录**不在数据范围内** | 数据层的范围过滤（查不到 → 当不存在） | **404** | `NOT_FOUND` |
  | 有权限点 + 范围满足 | — | 200 | — |

- **错误体**：RFC 9457 `application/problem+json`，字段 `type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id` / `params`；`detail` 必须是**英文 ASCII**（契约 C6）；`trace_id` 必带。
- **`detail` 的边界**（AC-S-044 新口径）：**可以**含被拒的**权限点编码**（开发者信息），**不得**含被拒**资源**的业务细节（标题 / 金额 / 归属人 / `org_node_id`）。
- **租户上下文**：`X-Tenant-Id`，必须与令牌绑定租户一致，不一致返回 404（契约 C3）。
- **跑法**：模块路由用例为**模块内进程内 HTTP 集成测试**（`modules/sandbox/tests/`，`ServiceExt::oneshot`）；SQL 断言通过平台查询日志或 `pg_stat_statements` 抓取语句文本；数据用真实库 `base_test`。

---

### TC-009-041: 不带令牌 → 401 + `UNAUTHENTICATED`
- **关联 AC**: AC-S-044
- **前置条件**:
  - **执行位置**: 模块内进程内 HTTP（`modules/sandbox/tests/`，`ServiceExt::oneshot`）。
  - `modules/sandbox` 已接入 `require_permission("sandbox:scope-item:read")`。
- **测试步骤**:
  1. 不带任何 `Authorization` 头调 `GET /api/sandbox/scope-item`。
  2. 带一个**无效**令牌（`Authorization: Bearer not-a-jwt`）调同一路由。
  3. 记录两次响应的状态码、`code`、`Content-Type` 与响应体字段集合。
- **预期结果**:
  1. 第 1 步返回 HTTP **401**，`code = UNAUTHENTICATED`，`Content-Type` 为 `application/problem+json`。
  2. 第 2 步返回 HTTP **401**，`code = UNAUTHENTICATED`（**不得** 403，也**不得** 500）。
  3. 两次响应体字段集合相同：`type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id`（`params` 可选）；`detail` 为**英文 ASCII**、非空；`instance` 等于请求路径 `/api/sandbox/scope-item`。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径（未认证）+ 精确状态码/错误码断言

### TC-009-042: 有效令牌但缺权限点 → 403 + `INSUFFICIENT_PERMISSION`
- **关联 AC**: AC-S-044
- **前置条件**:
  - **执行位置**: 模块内进程内 HTTP。
  - 账号 `u-1007`（`noperm@example.com`）绑定角色 `rbac-self`，该角色**只含** `sandbox:scope-item:read`，**不含** `sandbox:scope-item:approve`。
  - 账号 `u-1001` 绑定 `rbac-owner`，**含** `sandbox:scope-item:approve`（对照组）。
- **测试步骤**:
  1. 以 `u-1007` 调 `POST /api/sandbox/scope-item/approve`，body `{"id":"item-s1"}`。
  2. 以 `u-1001` 调同一路由，body `{"id":"item-a1"}`（对照组，记录响应体字段集合）。
  3. 对比两次响应的状态码、`code`、响应体字段集合与 `detail` 内容。
- **预期结果**:
  1. 第 1 步返回 HTTP **403**，`code = INSUFFICIENT_PERMISSION`（**不是** 404、**不是** 401、**不是** 200）—— 缺权限点由 `require_permission` 层判。
  2. 第 2 步返回 HTTP **200**，响应体含审批结果字段。
  3. 第 1 步的响应体字段集合与第 2 步**不同**（成功体不是 problem 体）；第 1 步 `detail` **可以**含权限点编码 `sandbox:scope-item:approve`，但**不得**含被拒记录的标题、金额、归属人、`org_node_id` 等资源细节。
  4. 备注（AC-S-045 明确的口径）：动作类越权返回 403 是**契约 C5 规定的行为**，**不算**存在性泄露 —— 本用例不得把「403 与 404 不同」判为缺陷。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（缺权限点）+ 对照组（有权放行）+ 精确错误码断言

### TC-009-043: 有权限点但目标记录不在数据范围内 → 404 + `NOT_FOUND`
- **关联 AC**: AC-S-044
- **前置条件**:
  - **执行位置**: 模块内进程内 HTTP。
  - 账号 `u-1007`（`rbac-self`，`SELF`）**有** `sandbox:scope-item:read` 权限点，但 `item-a1`（`created_by = u-1001`）**不在**其 `SELF` 范围内。
- **测试步骤**:
  1. 以 `u-1007` 调 `GET /api/sandbox/scope-item/item-a1`。
  2. 以 `u-1007` 调 `GET /api/sandbox/scope-item/item-s1`（自己创建的记录，在范围内，作对照）。
  3. 记录两次响应的状态码、`code`、响应体字段集合。
- **预期结果**:
  1. 第 1 步返回 HTTP **404**，`code = NOT_FOUND`（**不得** 403 —— 资源类越权按契约 C5 是 404；用户**有**权限点，只是记录不在范围内）。
  2. 第 2 步返回 HTTP **200**，响应体含 `item-s1` 的字段。
  3. 第 1 步的错误体字段集合与第 2 步的 200 响应**不同**；第 1 步 `detail` **不出现** `item-a1` 的标题、金额、`org_node_id`、`created_by` 等任何业务细节（不得因 404 而泄露内容）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（对象级越权）+ 对照组（范围内可见）+ 契约 C5 精确判定

### TC-009-044: 三种拒绝的响应体都符合 RFC 9457 且 `detail` 为英文 ASCII
- **关联 AC**: AC-S-044
- **前置条件**:
  - **执行位置**: 模块内进程内 HTTP。
  - 三种拒绝场景各准备一次请求：①无令牌 ②有效令牌无权限点 ③有权限点但范围外（条件同 TC-009-041 ~ TC-009-043）。
- **测试步骤**:
  1. 分别发出三次请求，保存完整响应（状态码 + 头 + 体）。
  2. 逐个校验响应体的字段集合、`status` 与 HTTP 状态码的一致性、`type` 的形态、`detail` 的字符集。
  3. 校验 `instance` 与请求路径、`trace_id` 的存在性与格式。
- **预期结果**:
  1. 三次响应均为 `application/problem+json`。
  2. 三次响应体的字段集合**完全相同**（`type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id`，`params` 可选）；每条的 `status` 字段与 HTTP 状态码一致（401 / 403 / 404）；`type` 形如 `https://errors.base.example/<kebab-case-code>`。
  3. 三次的 `detail` **均为英文 ASCII**（逐字符校验 `is_ascii()` 为真，且不含中文标点/汉字）；`instance` 等于各自请求路径；`trace_id` **非空**且为 32 位十六进制（或与平台既有 `trace_id` 格式一致）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 契约一致性断言（RFC 9457 + 契约 C6）+ 字符集边界校验

### TC-009-045: 被拒资源的细节不得出现在错误体中（权限点编码允许）
- **关联 AC**: AC-S-044
- **前置条件**:
  - **执行位置**: 模块内进程内 HTTP。
  - 目标记录 `item-big` 的已知敏感值：`name = "大额单"`、`amount = 20000`、`created_by = u-1007`、`org_node_id = dept-a`。
  - 账号 `u-1007` 缺 `sandbox:scope-item:approve`（`item-big` 由 `u-1007` 创建，**在其 SELF 范围内**）。
- **测试步骤**:
  1. 以 `u-1007` 调 `POST /api/sandbox/scope-item/approve`，body `{"id":"item-big"}`。
  2. 把响应体**全文**（含 `detail`、`params`、`title`、`type`）取出，逐个敏感值做子串搜索。
  3. 检查响应头（除 `trace_id` 相关头外）是否含业务值。
  4. 单独检查 `detail` 是否含权限点编码 `sandbox:scope-item:approve`。
- **预期结果**:
  1. 返回 HTTP 403 + `code = INSUFFICIENT_PERMISSION`。
  2. 响应体全文**不包含**：`大额单`、`20000`、`u-1007`、`dept-a`（`org_node_id` 的值）；也**不包含**中文句子（`detail` 必须是英文 ASCII）。
  3. 响应头中**不含**任何业务值（不得用自定义头回传资源信息）；`trace_id` 存在且非空。
  4. `detail` 中**允许**出现权限点编码 `sandbox:scope-item:approve` —— 若出现，**不判为失败**（AC-S-044 明确：权限点编码是开发者信息）；若不出现，同样通过。本步只断言「不得出现资源业务字段」。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 负向断言（敏感值全量子串搜索）+ 允许项显式豁免（权限点编码）+ 边界（标题为中文时同样不得回显）

### TC-009-046: 绕过前端直调接口访问范围外资源 → 被拒
- **关联 AC**: AC-S-045
- **前置条件**:
  - **执行位置**: 模块内进程内 HTTP。
  - 账号 `u-1012`（`scope-owner@example.com`）绑定 `rbac-owner`（`OWN_ORG`），**拥有**权限点 `sandbox:scope-item:read`，其主部门为 `dept-a-1`。
  - 按夹具：`item-a1`（`org_node_id = dept-a`）落在 `u-1012` 的数据范围**之外**；`item-a2`（`org_node_id = dept-a-1`）落在其范围**之内**（见 [index.md](index.md) §3.5 的期望集合表）。
  - 请求**不经过任何 UI 显隐逻辑**（直接用 HTTP 客户端 / `oneshot` 发请求）。
- **测试步骤**:
  1. 直调 `GET /api/sandbox/scope-item/item-a1`（带 `u-1012` 的令牌）。
  2. 记录状态码、`code`、响应体全文与响应头集合。
  3. 核对响应体是否含 `item-a1` 的任何业务字段。
- **预期结果**:
  1. 返回 HTTP **404** + `code = NOT_FOUND`（资源类越权按 C5 判 404）；**不得**返回 200、**不得**返回 403。
  2. 响应体为 RFC 9457 problem 体（字段集合同 TC-009-044）。
  3. 响应体**不包含** `item-a1` 的名称、金额、`org_node_id`、`created_by`；**不得**因为「前端已隐藏按钮」而放行。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（绕过前端）+ 精确状态码断言

### TC-009-047: 对照组：范围内的资源返回 200（证明接口可用，不是恒 404）
- **关联 AC**: AC-S-045
- **前置条件**:
  - **执行位置**: 模块内进程内 HTTP。
  - 同 TC-009-046 的账号与权限；`item-a2`（`org_node_id = dept-a-1`）落在 `u-1012` 的数据范围**之内**。
- **测试步骤**:
  1. 直调 `GET /api/sandbox/scope-item/item-a2`，记录状态码、响应头集合与响应体字段集合。
  2. 与 TC-009-046 的被拒响应做比对：①状态码 ②错误体字段集合 ③响应头集合。
  3. 核对成功响应体里的业务字段是否**只**来自 `item-a2` 本身。
- **预期结果**:
  1. 返回 HTTP **200**；响应体含 `item-a2` 的字段（`id` / `name` / `amount` / `org_node_id` / `created_by`）。
  2. 被拒响应（404）与成功响应（200）的**状态码不同**、响应体**字段集合不同**（problem 体 vs 业务体）、响应头集合中仅 `content-type` / `content-length` / `trace` 类头可能不同 —— 差异只来自「资源可见 / 不可见」这**一个事实**。
  3. 成功响应体中**不含**除 `item-a2` 自身以外的任何记录数据（不得顺带带出范围外的记录）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 破坏性对照（范围内组 vs 范围外组必须可区分且可解释）+ 字段集合比对

### TC-009-048: 范围外资源 vs **不存在**资源 → 响应完全一致（404 不泄露存在性）
- **关联 AC**: AC-S-045
- **前置条件**:
  - **执行位置**: 模块内进程内 HTTP。
  - `item-a1` 真实存在但 `u-1012` 的范围**看不到**；`item-not-exist-9999` 在库中**不存在**。
- **测试步骤**:
  1. 直调 `GET /api/sandbox/scope-item/item-a1`（存在但范围外）。
  2. 直调 `GET /api/sandbox/scope-item/item-not-exist-9999`（不存在）。
  3. 逐字段比对两次响应：状态码、`code`、`type`、`title`、响应体字段集合、响应头集合；对 `detail` 做文本比对（剔除 `instance` / `trace_id` 两个必然不同的字段）。
- **预期结果**:
  1. 两次均返回 HTTP **404** + `code = NOT_FOUND`。
  2. 两次的 `type`、`title`、`code` **逐字相同**；响应体字段集合**完全相同**；响应头集合**完全相同**（含 `content-type`）。
  3. 两次的 `detail` 在剔除 `instance` 与 `trace_id` 后**文本相同**（或至少不含任何可区分信息）；两次响应**不得**出现「资源存在但范围外」与「资源不存在」的可区分措辞（如 `out of scope` vs `not found`）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（存在性探测）+ 逐字段一致性断言（可区分性检测）

### TC-009-049: 跨租户 id 与同租户范围外 id 返回同样的 404
- **关联 AC**: AC-S-045
- **前置条件**:
  - **执行位置**: 模块内进程内 HTTP。
  - `tenant-b` 存在记录 `item-t2-1`；`u-1001` 的令牌绑定 `tenant-a`。
- **测试步骤**:
  1. 以 `u-1001` 的令牌 + `X-Tenant-Id: tenant-a` 调 `GET /api/sandbox/scope-item/item-t2-1`。
  2. 以 `u-1001` 的令牌 + `X-Tenant-Id: tenant-b` 调同一路由（头与令牌租户**不一致**）。
  3. 以 `tenant-b` 的账号 `u-2001` 调 `GET /api/sandbox/scope-item/item-t2-1`（对照：本人有权的跨租记录）。
- **预期结果**:
  1. 第 1 步返回 HTTP **404** + `code = NOT_FOUND`，响应体**不含** `item-t2-1` 的任何字段（跨租 id 属资源类越权，判定口径同本 AC）。
  2. 第 2 步返回 HTTP **404** + `code = NOT_FOUND`（契约 C3：请求头租户与令牌租户不一致 → 404）；与第 1 步的响应体字段集合、`type`、`title`、`code` **一致**。
  3. 第 3 步返回 HTTP **200** 且含 `item-t2-1` 的字段 —— 证明第 1~2 步的 404 是**隔离/范围**结果，不是「接口坏了」。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（跨租户 id + 伪造租户头）+ 对照组（本人有权）

### TC-009-050: 业务模块不写范围条件时，平台自动叠加数据范围过滤（SQL 层）
- **关联 AC**: AC-S-048
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 模块内进程内 HTTP。
  - `modules/sandbox` 的列表查询**故意不写**任何 `org_node_id` / `created_by` 条件，只用 `TenantQuery`（`{tenant}` + 业务参数）。
  - 账号 `u-1001`（`OWN_ORG`）；`tenant-a` 共 14 条记录，其中 2 条在 `dept-a`。
- **测试步骤**:
  1. 以 `u-1001` 调 `GET /api/sandbox/scope-item`（`limit=200`），记录返回条数与 id 集合。
  2. 抓取该次请求对应的 SQL 语句文本（平台查询日志 / `pg_stat_statements`）。
  3. 在 SQL 文本中查找**平台追加的**范围条件片段。
  4. 用管理员连接直接执行「不带任何范围条件」的等价 SQL，统计命中行数。
- **预期结果**:
  1. HTTP 200，返回**恰好 2 条**（`item-a1`、`item-big`）—— **判据一：结果已被过滤**，平台自动叠加的范围条件生效。
  2. SQL 文本中**同时**含租户条件与**平台追加的**范围条件片段（`org_node_id` / `created_by` / 子树 CTE / `= ANY($n)` 之一），且该条件是**平台注入**的（业务模块源码里没有这段）。
  3. **判据二**：渲染出的 SQL **含平台追加的范围条件片段**，且片段里的值**走参数绑定**（SQL 文本里不出现拼进去的字面值）—— 与 TC-009-052 的判据一致。
  4. 不带范围条件的等价 SQL 命中 **13** 条（与接口返回的 2 条**不同**）—— 证明过滤发生在**数据库层**（查出来再在内存筛也会得到 2 条，但第 2、3、4 步与本步共同排除「内存筛」）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（业务模块漏写范围条件）+ SQL 层断言（过滤必须在 DB 层）

### TC-009-051: 不存在「跳过数据范围」的开关 / 参数
- **关联 AC**: AC-S-048
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 模块内进程内 HTTP。
  - 账号 `u-1001`（`OWN_ORG`）；接口 `GET /api/sandbox/scope-item`。
- **测试步骤**:
  1. 在 `GET /api/sandbox/scope-item` 上依次加查询参数 `skipScope=true`、`scope=ALL`、`ignoreDataScope=true`、`dataScope=ALL`、`bypass_scope=1`。
  2. 每次记录返回条数与 id 集合。
  3. 静态检查 `platform-db` 的 `TenantQuery` 公开 API（含计划新增的 `with_scope()`）与 `platform-core` 的扩展点 trait：搜索是否存在「关闭范围过滤」的构造参数或方法。
  4. 用**直接 SQL**（管理员连接，不经平台）执行同样的越权查询，记录命中行数作为「越权可达」的参照。
  5. 静态检查 `modules/sandbox` 的源码：是否存在绕过 `TenantQuery` 直接拼 SQL 的列表查询。
- **预期结果**:
  1. 五次请求**全部**返回 HTTP 200 且结果集合**完全相同**（均为 `item-a1` + `item-big` 两条）；**不得**出现 14 条（越权）或 0 条。
  2. 每次返回条数 = 2，与不带任何参数时一致（参数被忽略，而不是被当成越权开关）。
  3. **判据三**：平台**不提供**任何关闭/放宽数据范围的公开 API（`TenantQuery` 只有 `bind` / `render` / `statement` / `arg_count` 与 `with_scope`；扩展点 trait 无 `skip_scope` 之类开关）；业务模块也构造不出来。
  4. 直接 SQL 可得 14 条 —— 说明**只有**平台的构造器能施加范围条件，业务模块拿不到裸连接（`platform-db` 公开 API 不暴露裸连接）。
  5. `modules/sandbox` 的列表查询**没有**绕过 `TenantQuery` 直接拼 SQL（若有，**由 `check-deps` / 约定挡住并在评审中判失败** —— 本步记录事实并按门禁结论判定）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（越权开关探测）+ 负向断言（参数无效）+ 门禁视角

### TC-009-052: 数据范围条件与租户条件一起以参数化方式渲染
- **关联 AC**: AC-S-048
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）。
  - 账号 `u-1001`（`OWN_ORG`），主部门 `dept-a`。
  - 可读取平台查询日志 / `pg_stat_statements` 的 SQL 文本与参数。
- **测试步骤**:
  1. 以 `u-1001` 调 `GET /api/sandbox/scope-item`，抓取 SQL 文本与绑定参数。
  2. 在 SQL 文本中搜索 `tenant-a`、`dept-a`、`u-1001` 等字面值。
  3. 在绑定参数列表中搜索上述值。
  4. 调 `GET /api/sandbox/scope-item?filter[name][eq]=a' OR '1'='1`（注入串作为业务过滤值），抓取 SQL 与参数。
- **预期结果**:
  1. 一次请求对应一条带租户条件与范围条件的语句，条件以 `$n` 占位。
  2. SQL **文本**中**不出现** `tenant-a`、`dept-a`、`u-1001` 三个字面值（一个都不出现）。
  3. 三个值**全部**出现在绑定参数列表中（`tenant-a` 是 `$1`，其余为后续 `$n`）。
  4. 注入串作为**参数值**原样绑定（出现在参数列表里），**不出现在 SQL 文本中**；返回结果条数与用普通值时一致（0 条，因无 `name` 等于该串的记录）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 参数化断言（文本里搜不到字面值）+ 注入串边界

---
