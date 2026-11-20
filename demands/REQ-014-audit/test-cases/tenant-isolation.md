# 跨租户不可见（数据库强制，非应用层过滤）（AC-S-126）

- **覆盖 AC**: AC-S-126（8 条：TC-014-043 ~ 050）
- **前置基线**:
  - 测试库 `base_test`；租户 `t-audit`（T-A）有 1 条记录、`t-other`（T-B）有 1 条记录
  - 管理员连接 = `base`（**超级用户，RLS 对它不生效**）；应用连接 = `base_app`
  - `t-audit` / `t-other` 两条记录的 `id` 已记下
- **实现事实（用例按此写）**:
  - RLS policy：`tenant_id = current_setting('app.tenant_id', true)`（design §3.3）
  - ⚠️ `current_setting(..., true)` 的第二个参数 `true` = 缺失时返回 `NULL`（**不报错**）
    → 未设租户上下文时 policy 求值为 `NULL` → **0 行**（fail closed）
  - ⚠️ **`app.tenant_id` 是事务局部的**：`set_config('app.tenant_id', $1, true)`
    （`crates/platform-db/src/tenant.rs:28`、`:187-190`）——
    `true` = `SET LOCAL`，**事务结束即失效**。裸连接上单独跑一条
    `SELECT set_config(...)` 再单独跑 `SELECT ...`，第二条语句**看不到**这个设置。
  - 超级用户与 `BYPASSRLS` 角色**绕过** RLS
    （`deploy/RUNBOOK.md:299-305`；`crates/platform-db/src/rls.rs:190-193` 自检同一口径）
  - `base_platform_app` 是 NOLOGIN 组角色；能登录的是 `base_app`（成员）

---

### TC-014-043: 以 T-B 的令牌调查询接口，**不含** T-A 的那条记录

- **关联 AC**: AC-S-126
- **前置条件**: T-A、T-B 各有 1 条记录；有一个 T-B 的访问令牌
- **测试步骤**:
  1. 用 T-B 的令牌调 `GET /api/audit/events?page=1&size=50`（带 `x-tenant-id: t-other`）
  2. 读响应 `items`
- **预期结果**:
  1. HTTP `200`
  2. `items` 里**没有任何** `id == <T-A 那条>`
  3. `items` 里含 `id == <T-B 那条>`
  4. `total >= 1`（T-B 自己的记录**可见** —— 不是「全查不到」那种假绿）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径
- **备注**: 第 4 条是**防空绿**断言 —— 只断言「不含 T-A」的话，
  「一条都查不到」也能通过。

### TC-014-044: 应用角色 + **单事务内** `set_config(t-other)` 查表 → 不含 T-A 记录

- **关联 AC**: AC-S-126
- **前置条件**: T-A、T-B 各有 1 条记录；应用连接 = `base_app`
- **测试步骤**:
  1. 以 `base_app` 在**一个事务**里执行：
     `BEGIN; SELECT set_config('app.tenant_id','t-other',true); SELECT * FROM platform_audit_log; COMMIT;`
  2. 读步骤 1 的结果集
- **预期结果**:
  1. 结果集**只含** T-B 那条（`tenant_id == 't-other'`）
  2. 结果集**不含** T-A 那条
  3. 结果集行数 `== 1`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径
- **备注**: ⚠️ **必须是一个事务**。实测（`base_test` 临时表，已清理）：
  - 裸语句（autocommit，每条语句各自一个事务）：`set_config` 之后单独查 → **0 行**
  - 同一事务内：`t-other` → 1 行；`t-audit` → 1 行
  - 完全不设 → 0 行
  AC 原文「把 `app.tenant_id` 设为 `t-other` 后直接查」按裸连接执行 → **步骤 3 的期望不可达**。〔S-04〕

### TC-014-045: 应用角色 + **单事务内** `set_config(t-audit)` 查表 → **含** T-A 记录

- **关联 AC**: AC-S-126
- **前置条件**: 同 TC-014-044
- **测试步骤**:
  1. 以 `base_app` 在**一个事务**里执行：
     `BEGIN; SELECT set_config('app.tenant_id','t-audit',true); SELECT * FROM platform_audit_log; COMMIT;`
  2. 读结果集
- **预期结果**:
  1. 结果集**含** T-A 那条
  2. 结果集**不含** T-B 那条
  3. 结果集行数 `== 1`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径（对照组）
- **备注**: 这条是 TC-014-044 的**对照组** —— 一条证明「看不见别的」，一条证明「看得见自己的」。
  **两条缺一不可**：只有 TC-014-044 时，「RLS 把所有行都藏了」也能通过。

### TC-014-046: 未设 `app.tenant_id` 时查表 → **0 行**（fail closed，不报错）

- **关联 AC**: AC-S-126
- **前置条件**: T-A、T-B 各有 1 条记录；应用连接 = `base_app`
- **测试步骤**:
  1. 以 `base_app`（**不设** `app.tenant_id`）执行 `SELECT count(*) FROM platform_audit_log`
  2. 读结果
- **预期结果**:
  1. 语句**成功**（**不报错**）
  2. 结果 `== 0`（**0 行，不是全表**）
  3. 结果**不是** `null`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（缺失上下文）
- **备注**: 这是 RLS **fail closed** 的判据（`crates/platform-db/src/rls.rs:24` 注释同款语义）。
  `current_setting('app.tenant_id', true)` 的 `true` 参数让「缺失」变成 `NULL` 而不是报错。

### TC-014-047: 归档表**同样**受 RLS 约束

- **关联 AC**: AC-S-126
- **前置条件**: 归档表已存在且 T-A、T-B 各有 1 条（需先触发一次归档，见 TC-014-091 的夹具做法）
- **测试步骤**:
  1. 以 `base_app` 在一个事务里 `set_config('app.tenant_id','t-other',true)` 后查 `platform_audit_archive`
  2. 再以 `set_config('app.tenant_id','t-audit',true)` 查一次
- **预期结果**:
  1. 步骤 1 只含 T-B 那条，不含 T-A 那条
  2. 步骤 2 只含 T-A 那条，不含 T-B 那条
  3. 两张表都 `relrowsecurity == true`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径
- **备注**: design §3.3 明确**两张表**都开 RLS。AC-S-126 只说「审计记录」，
  未提归档表 —— 本用例把「两张表」锁死，避免归档表成为绕过隔离的后门。

### TC-014-048: 应用角色**写**别的租户的行 → 被 `WITH CHECK` 拒

- **关联 AC**: AC-S-126
- **前置条件**: 应用连接 = `base_app`；T-A、T-B 各有 1 条记录
- **测试步骤**:
  1. 以 `base_app` 在一个事务里 `set_config('app.tenant_id','t-other',true)`，
     然后执行 `INSERT INTO platform_audit_log (tenant_id, action, object_type, prev_hash, hash, ip, user_agent, created_at) VALUES ('t-audit', 'x.y', 'z', '…', '…', '', '', now())`
  2. 读错误
- **预期结果**:
  1. 语句**失败**
  2. 错误信息含 `row-level security`（**不是** `permission denied` —— `base_app` 有 INSERT 权限）
  3. T-A 的记录数**未变**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径（写侧泄漏）
- **备注**: `crates/platform-db/src/rls.rs:26-28` 明确 `WITH CHECK` 的意义：
  没有它，「写得进去、读不出来」= **静默的写侧泄漏**。AC-S-126 只验了读侧，本条补写侧。

### TC-014-049: 应用连接角色**不绕过** RLS（自检口径与平台一致）

- **关联 AC**: AC-S-126
- **前置条件**: 应用连接 = `base_app`
- **测试步骤**:
  1. 以 `base_app` 执行 `SELECT current_user, (rolsuper OR rolbypassrls) FROM pg_roles WHERE rolname = current_user`
  2. 以 `base`（超级用户）执行同一查询作对照
- **预期结果**:
  1. 步骤 1：`current_user == 'base_app'`，`rolsuper OR rolbypassrls == false`
  2. 步骤 2：`current_user == 'base'`，`rolsuper OR rolbypassrls == true`（**对照成立**）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 这是 AC-S-126「**数据库强制**」这半句的前提。
  若应用连的是超级用户，RLS **完全失效**，而所有「查不到别的租户」的断言都会**假绿**。
  platform 的 `require_isolation`（`crates/platform-db/src/rls.rs:237-246`）做同一件事。

### TC-014-050: 跨租户按 **ID 猜**也拿不到（`GET /api/audit/events/{id}`）

- **关联 AC**: AC-S-126
- **前置条件**: T-A、T-B 各有 1 条记录；有一个 T-B 的令牌
- **测试步骤**:
  1. 用 T-B 的令牌调 `GET /api/audit/events/<T-A 那条的 id>`
  2. 读状态码与响应体
- **预期结果**:
  1. HTTP `404`（契约 C5：资源类越权用 **404**，不泄露存在性）
  2. 响应体是 RFC 9457 ProblemDetails，`code == "NOT_FOUND"`
  3. 响应体**不含** T-A 那条的任何字段值（`action` / `before` / `after` / `ip` 等）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 攻击路径
- **备注**: AC-S-126 只说「审计记录跨租户不可见」，没覆盖**按 ID 直取**这条路径。
  契约 C5 规定资源类越权走 404 —— 这里补上，避免出现「403 + 明示资源存在」的侧信道。

---
