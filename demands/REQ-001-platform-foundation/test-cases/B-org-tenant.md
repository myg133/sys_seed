# B. 组织与租户 —— 测试用例（AC-009 ~ AC-012）

- **需求**: REQ-001 通用项目底座（Base Platform）
- **来源**: [acceptance.md](../acceptance.md) §B、[demand.md](../demand.md) §B、[ADR-0001](../../decisions/ADR-0001-底座技术架构基线.md) D2（共享表 + `tenant_id` + RLS）
- **用例数**: 8（TC-B-001 ~ TC-B-008）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## B.0 本域约定

- 租户上下文请求头：`X-Tenant-Id`，取值 `T1` / `T2`（沿用 acceptance.md AC-009）。
- 空间上下文请求头：`X-Space-Id`（本用例集约定，实现前需 BA/Dev 确认）。
- 隔离级别：默认 L1 = 共享表 + `tenant_id` + PostgreSQL Row Level Security。
- 业务表统一含 `tenant_id` 列；组织节点统一含 `tenant_id` + `parent_id` + `path`。
- 出站 SCIM 端点：`POST {scim_base}/Users`、`POST {scim_base}/Groups`，请求体为 SCIM 2.0（`schemas: ["urn:ietf:params:scim:schemas:core:2.0:User"]`）。

---

### TC-B-001: X-Tenant-Id 贯穿请求 → DB → 响应
- **关联 AC**: AC-009
- **前置条件**:
  - 租户 T1、T2 各有同名业务数据：订单号 `INV-001` 在 T1 属 `boss@example.com`，在 T2 属 `t2user@example.com`；两者字段与业务含义相同。
  - 账号 `boss@example.com` 在 T1、T2 各有成员关系（跨租授权测试用）。
- **测试步骤**:
  1. 以 `boss@example.com` 登录，请求 `GET /api/order/invoice?order_no=INV-001`，请求头分别带 `X-Tenant-Id: T1` 与 `X-Tenant-Id: T2`。
  2. 记录两次响应的 `tenant_id` 与数据 owner。
  3. 打开 SQL 日志，核对两次请求对应的 SQL。
- **预期结果**:
  1. 两次均 HTTP 200，但返回数据的 `tenant_id` 分别为 `T1` 与 `T2`，owner 分别为 `boss@example.com` 与 `t2user@example.com`。
  2. 两次 SQL 均含租户过滤条件，分别出现 `tenant_id = 'T1'` 与 `tenant_id = 'T2'`，且该条件由中间件注入而非业务代码手写。
  3. 未带 `X-Tenant-Id` 或带非授权租户 ID 时返回 HTTP 400/403，不返回任何业务数据。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径 + 等价类（合法租户 / 缺失 / 越权租户）

### TC-B-002: 异步消息与缓存键携带租户上下文
- **关联 AC**: AC-009
- **前置条件**:
  - T1 触发一条异步任务（如订单导出或通知发送），任务执行器已开启消息体落库/日志打印。
  - 缓存层启用（Redis 或进程内缓存）。
- **测试步骤**:
  1. 以 T1 身份触发 `POST /api/tasks/exports`，携带 `X-Tenant-Id: T1`。
  2. 查看任务队列表中的消息记录体与消费端日志。
  3. 再次以 T1 身份请求同一接口，查看缓存键与缓存命中日志。
- **预期结果**:
  1. 消息记录体含 `tenant_id: "T1"`（或等价的租户上下文字段），消费端日志同值。
  2. 消费端处理出的数据全部来自 T1，无 T2 数据混入。
  3. 缓存键中包含租户维度（如 `cfg:T1:...` / `order:T1:...`），T1 与 T2 的同名 key 不互相命中。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 全链路传播断言

### TC-B-003: 用 T1 令牌访问 T2 资源被拒且不泄露存在性
- **关联 AC**: AC-010
- **前置条件**:
  - 资源 `order-2001` 存在于 T2（属 `t2user@example.com`）；T2 下不存在资源 `order-9999`。
  - `boss@example.com` 是 T1 成员。
- **测试步骤**:
  1. 以 `boss@example.com`（T1）登录，取得 token，请求 `GET /api/order/invoice/order-2001`（携带 `X-Tenant-Id: T1`）。
  2. 请求 `GET /api/order/invoice/order-9999` 作对照。
  3. 比对两次响应的状态码与响应体结构。
- **预期结果**:
  1. 步骤 1 返回 HTTP 404 或 403（按底座统一约定），**绝不返回 200**。
  2. 步骤 1 与步骤 2 的响应体结构、错误 `code`、字段数量一致，不得通过「存在 vs 不存在」区分出资源真实存在（不泄露存在性细节）。
  3. 两次响应均不含 `owner`、金额、`tenant_id` 等任何业务字段。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（IDOR）+ 边界（存在性泄露）

### TC-B-004: RLS 兜底：应用层漏写 tenant_id 过滤仍不返回他租数据
- **关联 AC**: AC-010
- **前置条件**:
  - 数据库已启用 RLS 策略（按 `tenant_id` 过滤）；准备一个**故意缺过滤条件**的只读查询（测试专用 SQL 片段，不落业务代码）。
  - T1、T2 各有数据 `INV-001`。
- **测试步骤**:
  1. 以 `boss@example.com`（T1）建立数据库会话，在测试钩子中执行不带 `WHERE tenant_id` 的 `SELECT * FROM order_invoice`。
  2. 观察返回行数与 `tenant_id` 分布。
  3. 再执行带 `WHERE tenant_id = 'T1'` 的同查询作对照。
- **预期结果**:
  1. 第 1 步返回的行**全部** `tenant_id = 'T1'`，不含 T2 行（RLS 生效）。
  2. 第 1 步行数 ≤ 第 3 步行数，且第 1 步不含 T2 的 `INV-001`。
  3. RLS 策略存在且生效（可从 `pg_policies` 查到对应策略）；说明 ADR-0001 D2 的「应用层漏写过滤是最高频越权来源」已被兜底。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 故障注入（故意错误实现）+ 边界

### TC-B-005: 出站 SCIM 同步在目标系统创建用户与部门
- **关联 AC**: AC-011
- **前置条件**:
  - 目标系统实现 SCIM 2.0 `Users` / `Groups` 端点（可用 mock SCIM server），已配置 base URL 与鉴权。
  - T1 中新建用户 `newbie@example.com`（属于 `org-sales-1`），并新建部门 `org-legal`（父节点 `org-sales`）。
- **测试步骤**:
  1. 在底座新增上述用户与部门。
  2. 触发 SCIM 同步（`POST /api/org/scim/sync`，`scope: "incremental"`）。
  3. 查询目标系统的 `Users` 与 `Groups` 列表。
- **预期结果**:
  1. 同步任务返回 HTTP 200 且 `status = "succeeded"`，记录同步条数。
  2. 目标系统 `Users` 中出现 `newbie@example.com`，`userName` 为邮箱，`active = true`；`Groups` 中出现 `org-legal` 对应条目，且 `members` 含该用户。
  3. 部门与用户的对应关系（`org-legal` / `org-sales-1`）正确传递。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 端到端链路 + 正常路径

### TC-B-006: 重复同步幂等，不产生重复条目
- **关联 AC**: AC-011
- **前置条件**:
  - 同 TC-B-005，已完成一次同步；目标系统已有对应 Users / Groups 条目。
- **测试步骤**:
  1. 不做任何数据变更，再次触发同一 scope 的同步。
  2. 查询目标系统 `Users` / `Groups` 中对应条目的数量与 `id`。
  3. 对比两次同步任务的 `items_synced` 与去重结果。
- **预期结果**:
  1. 同步返回 HTTP 200，`status = "succeeded"`。
  2. 目标系统中 `newbie@example.com` 与 `org-legal` 的条目数各为 **1**（不新增重复），条目 `id` 与首次一致。
  3. 同步任务记录 `created` / `updated` / `skipped` 计数，第二次 `created = 0`；不存在同一 `externalId` 的多条记录。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 幂等性 + 边界（重复执行）

### TC-B-007: 冲突策略为「人工待处理」时进队列并可人工裁决
- **关联 AC**: AC-012
- **前置条件**:
  - 同一用户 `newbie@example.com`：底座部门 = `org-sales-1`，外部系统（SCIM 目标或入站 LDAP）部门 = `org-finance`。
  - 租户 T1 的同步冲突策略配置为 `manual`（人工待处理）。
- **测试步骤**:
  1. 执行一次同步。
  2. 查询冲突待处理队列 `GET /api/org/sync-conflicts?status=pending`。
  3. 以管理员 `owner@example.com` 在冲突条目上做人工裁决（选择「以底座为准」）。
  4. 再次同步并查看目标系统与队列状态。
- **预期结果**:
  1. 同步任务返回 HTTP 200，`status = "partial"` 或 `completed_with_conflicts`，`conflicts = 1`。
  2. 队列中存在 1 条冲突记录，含 `user_id`、底座部门、外部部门、两侧取值、发现时间。
  3. 裁决接口返回 HTTP 200，冲突状态变为 `resolved`，记录 `resolved_by` 与 `resolution = "platform_wins"`。
  4. 再次同步后目标系统该用户部门被更新为 `org-sales-1`，队列中该条不再是 pending，且系统**不会**对同一冲突反复产生新待处理记录。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 判定表（冲突策略分支）+ 状态迁移

### TC-B-008: 冲突策略为「底座为准 / 外部为准」时自动处理
- **关联 AC**: AC-012
- **前置条件**:
  - 同 TC-B-007 的冲突场景；分别把策略切为 `platform_wins` 与 `external_wins`，其余夹具不变。
- **测试步骤**:
  1. 策略 = `platform_wins`，执行同步，记录目标系统该用户的部门与冲突队列数量。
  2. 策略 = `external_wins`，执行同步，记录目标系统该用户的部门与冲突队列数量。
- **预期结果**:
  1. `platform_wins`：目标系统部门被更新为底座值 `org-sales-1`；冲突队列中**不新增**待处理记录（自动解决）。
  2. `external_wins`：目标系统保持外部值 `org-finance`；底座侧记录外部覆盖结果（底座部门被更新为 `org-finance` 或标记为外部权威覆盖，须二选一且在实现中固定一致）。
  3. 两次执行均留下审计记录（含策略名、冲突两侧值、处理结果）。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 判定表（三种策略分支对比）
