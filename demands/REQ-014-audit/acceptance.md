# 验收标准：审计日志

- **需求编号**: REQ-014
- **父需求**: REQ-001
- **状态**: **rev 3**（2026-11-20 —— QA 挑刺 36 条后逐条裁决修完）
- **对应验收标准**: 13 条（AC-S-022 + AC-S-122 ~ AC-S-133）
- **决策依据**: [decisions.md](decisions.md) rev 2（D130~D168）
- **用例**: [test-cases/](test-cases/index.md)（QA 产出 120 条，TC-014-001 ~ 120）
- **更新时间**: 2026-11-20 10:20

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## rev 3 变更摘要（**Dev 必读**）

- rev 2 的 **13 条 AC 全部有问题**（QA 挑刺 36 条 / 12 条阻塞）→ 本 rev 全部修完。
- 裁决全部记录在 [decisions.md](decisions.md) rev 2，**每条 AC 的改动都标了决策号**。
- 影响最大的四处：
  1. **AC-S-130 全面改用契约 C7 的分页口径**（游标 + `filter[...]` + `sort`）—— D145
  2. **AC-S-125 的 `reason` 改口**（第 3 条是 `prev_hash_mismatch` 不是 `hash_mismatch`）—— D146
  3. **AC-S-123 增加「重跑部署种子脚本后仍拒」**—— D147（append-only 会被 blanket GRANT 破功）
  4. **AC-S-124 增加并发验收项**—— D161（`FOR UPDATE` 防不住链分叉）

## 承接的父 AC

父 AC：AC-022

具体步骤见父需求 acceptance.md（`AC-022: 审计日志完整且不可篡改`）。

> 本需求的 AC-S-022 是父 AC-022 的**落地判据**；父 AC-022 由
> AC-S-022 / 122 / 123 / 124 / 125 / 126 共同承接。

## 全局约定（所有 AC 默认遵守）

| 项 | 约定 | 依据 |
|---|---|---|
| 应用连接角色 | **`base_app`**（`base_platform_app` 是 NOLOGIN 组角色，**连不上**） | D165 |
| 造数 / 篡改连接 | **`base`**（表 owner + 超管） | — |
| 库 | **`base_test`**（绝不写 `base_dev`） | dev-guide §9.8 |
| `app.tenant_id` | **事务局部**；凡涉及租户上下文的查询**必须在单个事务内** `set_config` → 查询 | D148 |
| 响应体 | 错误一律 `application/problem+json` + `ProblemDetails`（契约 C6） | D153 |
| 时间格式 | UTC、RFC3339、**微秒 6 位** | D142 / D151 |
| 哈希复算 | 测试侧**独立实现**，**直接使用列里存的 `before`/`after` 文本**，**不得**解析回 JSON 再序列化 | D141 |
| 静态扫描 | 必须带**阳性对照**（否则「0 命中」不可信） | dev-guide §9.2 |

---

## 本需求自有 AC

---

### AC-S-022: 哈希链可校验: 校验链完整性，篡改任一条目即告警

> 改动（D164）：告警字段名用 `event_id`；「定期校验」的执行者明确。

- **前置条件**: 测试库 `base_test` 可用；租户 `tenant-a` 下已有 **≥2 条**审计记录
  （通过平台 sink 写入，链正确）。
- **测试步骤**:
  1. 读取该租户的审计记录，按 `created_at ASC, id ASC` 升序排列；
  2. 调用链校验能力（`GET /api/audit/verify` 或 `cargo xtask audit verify`）；
  3. **绕过应用**，以 `base` 直连执行 SQL 把其中一条的 `after` 列改成另一个值；
  4. 再次调用链校验；
  5. 检查步骤 4 期间产生的日志。
- **预期结果**:
  - 步骤 2 返回 `ok = true`、`checked = 2`、`broken = []`；
  - 步骤 4 返回 `ok = false`，`broken` **恰好 1 条**，
    `broken[0].id` == 被篡改那条的 `id`，`broken[0].reason` == `hash_mismatch`；
  - 步骤 5 产生**至少一条 error 级别**日志，且含结构化字段
    **`tenant_id` / `event_id` / `reason`**（**可含更多字段**，不要求「恰好 3 个」）。
- **类型**: 安全

> **「定期校验」的执行者**（D164）：本轮**无常驻调度器** → 「定期」由**外部调度器**
> 周期性调用 `GET /api/audit/verify` 或 `cargo xtask audit verify` 实现。
> 本 AC **不要求**平台自带定时器。

---

### AC-S-122: 审计写入完整：一次敏感操作产生一条含全部固定字段的审计记录

> 改动（D163 事件码命名法、D167 字段名与验法）。

- **前置条件**: 测试库可用；已认证用户 `u-1`，租户 `tenant-a`。
- **测试步骤**:
  1. 通过平台审计 sink 记录一条事件（`action = "demo.item.update"`、
     `object_type = "demo_item"`、`object_id = "obj-1"`，
     `before = {"name":"old"}`、`after = {"name":"new"}`）；
  2. 用 `GET /api/audit/events` 查询该租户的审计记录；
  3. 检查步骤 2 返回的那条记录。
- **预期结果**:
  - 步骤 2 的结果里恰好出现 **1 条**匹配该 `object_id` 的新增记录；
  - `actorId` == `u-1`；`action` / `objectType` / `objectId` 与传入**逐字相同**；
  - `before` / `after` 解析后**等于**传入的 JSON（值相等即可，不要求键序）；
  - 响应对象**含 `ip` 与 `userAgent` 两个键**（无值时为空串，**不得缺键**）；
  - `createdAt` 非空；`prevHash` 与 `hash` 均为 **64 位小写十六进制**。
- **类型**: 功能

> 「13 个固定字段」= 契约 C12 的字段集，**不增不减**（D168）。
> 「不得缺列」在**查询层**验（响应里有这两个键），不在表结构层验（D167）。

---

### AC-S-123: 审计表 append-only：应用角色无法 UPDATE / DELETE 审计记录

> 改动（D147 加「重跑种子脚本」步骤、D165 角色改为 `base_app`）。

- **前置条件**: 测试库可用；审计表已存在且至少 1 条记录；
  持有**应用连接角色 `base_app`** 的连接（**不是** `base_platform_app`，它连不上）。
- **测试步骤**:
  1. 以 `base_app` 执行 `UPDATE platform_audit_log SET after = '{}' WHERE id = <某条>`；
  2. 以 `base_app` 执行 `DELETE FROM platform_audit_log WHERE id = <某条>`；
  3. 统计该表的行数；
  4. **重跑部署种子脚本** `deploy/seed/02-app-role.sql`；
  5. **重跑后**再次以 `base_app` 执行步骤 1 与步骤 2，并再次统计行数。
- **预期结果**:
  - 步骤 1、2 **两条语句都失败**（错误含 `permission denied`）；
  - 步骤 3 的行数与执行前**完全相同**；
  - 步骤 5 的两条语句**仍然失败**（`permission denied`），行数**仍然不变**。
- **类型**: 安全

> ⚠️ **步骤 4~5 是本 AC 最关键的部分**：种子脚本的
> `GRANT … ON ALL TABLES IN SCHEMA public`（`deploy/seed/02-app-role.sql:50`）
> 与 `ALTER DEFAULT PRIVILEGES … UPDATE, DELETE`（`:69-70`）
> 会把迁移里的 REVOKE **盖回去**（**已实测**）。种子脚本必须为审计表加例外块（D147）。
> **只测步骤 1~3 会在「补建脚本之后」才暴露问题** → 极易被当成环境问题放过。

---

### AC-S-124: 哈希链按租户分链且可链接，且并发写入不产生分叉

> 改动（D151 复算口径、D161 新增并发验收项）。

- **前置条件**: 测试库可用；租户 `tenant-a` 与 `tenant-b`。
- **测试步骤**:
  1. 在 `tenant-a` 连续写入 3 条审计记录；
  2. 在 `tenant-b` 写入 1 条；
  3. 按租户分别取出记录，按 `created_at ASC, id ASC` 升序；
  4. **独立**用 `sha256` 复算每条的 `hash`；
  5. **并发**：用**两个独立连接**同时向 `tenant-a` 各写入 1 条；
  6. 对 `tenant-a` 全部记录再跑一次链校验。
- **预期结果**:
  - `tenant-a` 第 1 条的 `prevHash` == **genesis**（64 个字符 `0`）；
  - `tenant-a` 第 2 条的 `prevHash` == 第 1 条的 `hash`；
  - `tenant-a` 第 3 条的 `prevHash` == 第 2 条的 `hash`；
  - `tenant-b` 第 1 条的 `prevHash` == **genesis**（**分链**，不接 `tenant-a` 的尾）；
  - 步骤 4 的独立复算结果与存储的 `hash` **逐条相同**；
  - 步骤 5 的两条**都写入成功**，排序后第 2 条的 `prevHash` == 第 1 条的 `hash`，
    且两条记录的 `prevHash` **互不相同**（**无分叉**）；
  - 步骤 6 的链校验 `ok == true`、`broken == []`。
- **类型**: 安全

> **步骤 4 的复算口径**（D151，**逐字照做**）：
> - 时间戳文本 = `format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z", …)`，
>   且**必须先转 UTC**（`to_offset(time::UtcOffset::UTC)`）；
> - **禁止**复用 `crates/platform-core/src/money.rs` 的 `format_rfc3339_utc`
>   与 `crates/platform-org/src/http.rs` 的 `format_timestamp`
>   —— **都只有秒精度**，复用即每条都对不上；
> - `len(f)` 是**字节长度**；`prev_hash` 以 **64 字节文本**参与拼接；
> - 复算时**直接使用列里存的 `before`/`after` 文本**，
>   **不得**解析回 JSON 再重新序列化（那是「验证生产代码的拷贝」，且会因序列化差异假红）。

---

### AC-S-125: 篡改可检测且沿链连锁

> 改动（D146 —— reason 口径改口，这是 rev 2 的**规格自相矛盾**）。

- **前置条件**: 测试库可用；`tenant-a` 下已有 **3 条**链正确的审计记录。
- **测试步骤**:
  1. **案例 A**：以 `base` 直连把第 2 条的 `after` 内容改成另一个值（不动 `hash`），
     调用链校验；
  2. **案例 B**：恢复原状后，以 `base` 直连把第 2 条**存储的 `hash` 值**改成另一个
     64 位十六进制（其它列不动），调用链校验。
- **预期结果**:
  - 案例 A：`ok = false`，`broken` **恰好 1 条**（第 2 条），`reason` == `hash_mismatch`；
  - 案例 B：`ok = false`，`broken` **恰好 2 条**（第 2 条与**第 3 条**）：
    - 第 2 条 `reason` == **`hash_mismatch`**（它的存储 `hash` 被改，与重算值不符）；
    - 第 3 条 `reason` == **`prev_hash_mismatch`**（它自己的 `prev_hash` 列没被改、
      自身重算值**等于**存储值；断掉的是**接续关系** —— 它指向的上一条 `hash` 已被改）；
    - **第 3 条被牵连**正说明链是**连锁**的，不是逐条独立的校验。
- **类型**: 安全

> **为什么第 3 条不是 `hash_mismatch`**：校验是**逐条独立重算 + 接续校验**两段式（D146）。
> 第 3 条用**自己的 `prev_hash` 列**重算 → 结果**等于**存储值 → 不构成 `hash_mismatch`；
> 但它的 `prev_hash` 与**上一条的存储 `hash`** 不接 → `prev_hash_mismatch`。
> 两个 reason 都能被真实触发，**没有死值**。

---

### AC-S-126: 审计记录跨租户不可见（数据库强制，非应用层过滤）

> 改动（D148 单事务、增「不设上下文」与归档表与写侧三个验收点）。

- **前置条件**: 测试库可用；租户 `tenant-a` 有 1 条记录；租户 `tenant-b` 有 1 条记录。
- **测试步骤**:
  1. 以租户 `tenant-b` 的令牌调用审计查询接口；
  2. 以 **`base_app`** 连接，**在单个事务内**执行
     `SELECT set_config('app.tenant_id', 'tenant-b', true);`
     然后 `SELECT * FROM platform_audit_log`，**同一事务内**完成；
  3. 同上，把 `app.tenant_id` 设为 `tenant-a` 再查一次；
  4. 以 `base_app` **不设**任何租户上下文，直接 `SELECT * FROM platform_audit_log`；
  5. 以 `base_app` 在单事务内设 `app.tenant_id = 'tenant-b'`，
     尝试 `INSERT` 一条 `tenant_id = 'tenant-a'` 的行；
  6. 同上，对 `platform_audit_archive` 做步骤 2~4。
- **预期结果**:
  - 步骤 1 的结果**不含** `tenant-a` 的那条记录；
  - 步骤 2 的结果**不含** `tenant-a` 的那条记录（**RLS 生效**），
    且**含** `tenant-b` 自己的那条（**不是 0 行**——0 行会让本 AC 恒真，假绿）；
  - 步骤 3 的结果**含** `tenant-a` 的那条记录；
  - 步骤 4 返回 **0 行且不报错**（fail closed）；
  - 步骤 5 **失败**（`WITH CHECK` 生效，写不进去）；
  - 步骤 6 的归档表**同样**受 RLS 约束，行为与在线表一致。
- **类型**: 安全

> 🔑 **步骤 2/3 必须在同一个事务内**：裸连接单条执行 `set_config` 后事务即结束 →
> 值被清 → 查到 0 行 → 「查不到别的租户」**恒真** → 假绿（**QA 已实测**）。

---

### AC-S-127: 登录审计：成功与失败各一条，含 IP 与设备，且不含凭据

> 改动（D149 显式带头、检索全列、不可枚举）。

- **前置条件**: 测试库可用；本地登录接口可用；一个 SSO provider 已配置。
- **测试步骤**:
  1. 带 `X-Forwarded-For: 203.0.113.9` 与 `User-Agent: audit-test-agent`，
     用正确密码本地登录一次；
  2. **同样带上这两个头**，用错误密码本地登录一次；
  3. 走一次 SSO 登录成功；
  4. 检索该租户**全部审计记录的全部列的文本拼接**（不只是 `before`/`after`）；
  5. 用一个**不存在的账号**登录失败，比对步骤 4 的结果。
- **预期结果**:
  - 步骤 1 产生 **1 条** `iam.login.succeeded`；
  - 步骤 2 产生 **1 条** `iam.login.failed`；
  - 步骤 3 产生 **1 条** `iam.sso.login.succeeded`；
  - 上述三条的 `ip` **逐字等于** `203.0.113.9`，
    `userAgent` **逐字等于** `audit-test-agent`；
  - 步骤 4 的检索结果**不含**测试密码的字面量，也**不含**任何 access/refresh token；
  - 步骤 5 的失败记录**不含**该账号名（失败记录**不可枚举**），
    且失败记录的条数与步骤 2 的**同量级**（不因账号是否存在而变化）。
- **类型**: 安全

> `ip` 取 `x-forwarded-for` 的**第一跳**（既有惯例：
> `crates/platform-iam/src/http.rs:267-269`）；由平台中间件填进
> `RequestContext.client_ip`（D149）。
> ⚠️ `ip` 是**客户端可控的头**，**不是**可信的物理来源 ——
> 它的用途是**关联与排查**，不是**取证**。规格不得把它写成「真实客户端地址」。

---

### AC-S-128: 权限变更审计：授予、撤销、创建、删除各一条，含变更前后值

> 改动（D163 补创建/删除、钉死事件码与形状）。

- **前置条件**: 测试库可用；已有一个角色 `r-1`；操作者具备角色管理权限。
- **测试步骤**:
  1. 给 `r-1` 授予权限 `p-1`，检索审计记录；
  2. 从 `r-1` 撤销权限 `p-1`，检索审计记录；
  3. 创建一个角色，检索审计记录；
  4. 删除步骤 3 创建的角色，检索审计记录。
- **预期结果**:
  - 步骤 1 产生 **1 条**记录，事件码 == `rbac.role.grant`，
    `objectType` == `role`，`objectId` == `r-1`；
    `before` 解析后 == `{"permissions":[]}`，
    `after` 解析后 == `{"permissions":["p-1"]}`；
  - 步骤 2 再产生 **1 条**记录，事件码 == `rbac.role.revoke`，
    `before` == `{"permissions":["p-1"]}`、`after` == `{"permissions":[]}`
    （**方向相反**）；
  - 步骤 3 产生 **1 条**记录，事件码 == `rbac.role.create`，
    `objectType` == `role`，`before` 为 `null`、`after` == `{"permissions":[]}`；
  - 步骤 4 产生 **1 条**记录，事件码 == `rbac.role.delete`，
    `before` == `{"permissions":[]}`、`after` 为 `null`；
  - 四条记录的 `actorId` == 操作者 ID。
- **类型**: 安全

> `before`/`after` 里的权限集合是**排序后**的字符串数组（确定性）。

---

### AC-S-129: 统一埋点入口：业务模块不需自建审计表

> 改动（D150 补装配路径、D152 改扫描口径）。

- **前置条件**: 测试库可用；**平台 sink 已被注入**到相关 service
  （`platform-server` 构造 `PgAuditSink` 后向下注入）。
- **测试步骤**:
  1. 用平台提供的**通用审计事件类型 + sink trait** 记录一条事件；
  2. 把 `platform-org` 已声明的事件（`org.node.create`）送进平台 sink；
  3. 把 `platform-iam` 的 SSO 事件送进平台 sink；
  4. 扫 `crates/**/*.rs`，查找 `INSERT INTO platform_audit_log`；
  5. 扫 `migrations/**/*.sql`，查找 `INSERT INTO platform_audit_log`；
  6. 运行 `platform-org` 与 `platform-iam` 的既有测试。
- **预期结果**:
  - 步骤 1~3 的事件**都**出现在审计表里；
  - 步骤 4 的扫描结果 **0 命中**（业务模块与平台 crate 的 Rust 源码里**没有**裸 INSERT）；
  - 步骤 5 的扫描结果**恰好 1 处**（平台自己的迁移里）；
  - 步骤 4/5 的扫描器有**阳性对照** —— 用同一扫描器扫一段确实含该 SQL 的样例，
    **必然命中**（否则「0 命中」不可信，见 dev-guide §9.2）；
  - 步骤 6 全绿（`platform-org` / `platform-iam` 的既有 recorder trait
    **签名未被破坏**）。
- **类型**: 功能

> **为什么 Rust 侧是 0 命中**：写入走 `SECURITY DEFINER` 函数 → `INSERT` 在
> **plpgsql 函数体内**，Rust 源码里根本不出现（D152）。

---

### AC-S-130: 审计查询接口：分页、过滤、排序、权限控制（**契约 C7 口径**）

> 改动（D145 —— 全面改用契约 C7；D153 非法值 vs 超上限；D155 排序）。

- **前置条件**: 测试库可用；租户 `tenant-a` 下写入 **5 条**记录，
  它们在 `actorId` / `action` / `objectType` / `createdAt` 上**互不相同**。
- **测试步骤**:
  1. 调用 `GET /api/audit/events?limit=2`；
  2. 用 `?cursor=<步骤1返回的 next_cursor>&limit=2` 翻页；
  3. 分别用 `?filter[actorId][eq]=…`、`?filter[action][eq]=…`、
     `?filter[objectType][eq]=…`、`?filter[createdAt][range]=…` 过滤；
  4. 用 `?sort=-createdAt` 与 `?sort=createdAt` 排序；
  5. 同时给 `?cursor=…&page=1`；
  6. 给 `?limit=0`、`?limit=abc`、`?limit=1000`、`?sort=unknownField`；
  7. 用**没有** `audit:events:read` 权限的令牌调用同一接口；
  8. 给 `?tenant_id=tenant-b`。
- **预期结果**:
  - 步骤 1 返回该租户的 **2 条**记录 + **总数 5** + `nextCursor`（非 `null`）；
  - 步骤 2 用步骤 1 的 `nextCursor` 翻页，返回**与步骤 1 不重复**的记录，
    且继续推进；
  - 步骤 3 每种过滤的结果与写入的数据**一致**（不多不少）；
  - 步骤 4 排序结果与 `createdAt` 的升/降序**一致**；
  - 步骤 5 返回 **400**（`cursor` 与 `page` **互斥**，契约 C7），错误码明确，
    响应体是 `application/problem+json`；
  - 步骤 6 的四个子项**分别**：
    - `limit=0` → **400**，错误码 `AUDIT_INVALID_FILTER`；
    - `limit=abc` → **400**，错误码 `AUDIT_INVALID_FILTER`；
    - `limit=1000` → **不报错**，实际按 **200** 上限执行（**夹到上限**）；
    - `sort=unknownField` → **400**（未声明字段，C7），错误码 `AUDIT_INVALID_FILTER`；
    - 所有 400 的响应体均为 `application/problem+json` + `ProblemDetails`；
  - 步骤 7 返回 **403**，错误码 == `INSUFFICIENT_PERMISSION`（**不是** 500）；
  - 步骤 8 的 `tenant_id` 参数被**忽略**（结果仍是 `tenant-a` 的记录），
    且产生一条 **warn** 级日志。
- **类型**: 功能

> **分页/过滤/排序口径完全按契约 C7**（D145）：
> 游标分页 `?cursor=&limit=`（默认 `limit=20`，上限 200，响应含 `nextCursor`）；
> 兼容页码 `?page=N&page_size=M`（与 cursor 互斥）；
> 排序 `?sort=-createdAt,name`（白名单：`createdAt` / `actorId` / `action` /
> `objectType` / `objectId`）；过滤 `?filter[field][op]=value`
> （op 白名单 `eq` / `ne` / `in` / `contains` / `range`）。
> **数据范围过滤由平台自动叠加**，审计侧不得自行实现绕过开关。

---

### AC-S-131: 保留策略与归档：超期记录搬入归档表，归档表同样 append-only

> 改动（D154 前置改写、D159 链状态表、D166 手写 policy）。

- **前置条件**: 测试库可用；审计表有 1 条**足够旧**的记录与 1 条**很新**的记录；
  `base_app` 连接可用。
- **测试步骤**:
  1. 执行归档动作（`POST /api/audit/archive` 或 `cargo xtask audit archive --tenant …`）；
  2. 查询在线审计表与归档表；
  3. 再执行一次归档动作；
  4. 以 `base_app` 对**归档表**执行 `UPDATE` 与 `DELETE`；
  5. 统计归档表行数；
  6. 对归档后的在线表跑一次链校验。
- **预期结果**:
  - 步骤 2：旧记录**不在**在线表、**在**归档表；新记录**仍在**在线表；
  - 步骤 3：**幂等** —— 不报错，且归档表行数与步骤 2 相同；
  - 步骤 4：两条语句**都失败**（`permission denied`），归档表行数不变；
  - 步骤 5：归档表的**列集合** == 在线表的列集合 **∪** `{archived_at}`；
    被搬走记录的**其余每个字段**与源行**逐字段相同**（含 `hash` / `prevHash`）；
  - 步骤 6：链校验 `ok == true`（归档**不**破坏链 —— 见下）。
- **类型**: 安全

> **「足够旧的记录」的构造**（D154）：写入函数固定用 `clock_timestamp()`，
> **没有**指定 `created_at` 的入口 → 旧记录**只能由 `base` 直连 INSERT 构造**，
> 且**不参与**链校验断言（它的 `hash`/`prev_hash` 由测试侧自算或用自洽值）。
>
> **归档后链校验为何仍 `ok == true`**（D159）：新增表 `platform_audit_chain_state`
> 记录每个租户链的**当前基准** `head_hash`（genesis 或上次归档的尾 `hash`）。
> 归档时更新它；校验时在线表**第一条**的 `prevHash` 与 `head_hash` 比对。
> 没有这个基准，归档一次就会让接续校验**必然误报断裂**。

---

### AC-S-132: 审计查询界面：列表、分页、过滤、字段展示

> 改动（D145 服务端分页、D155 排序、D167 403 判据）。

- **前置条件**: shell 已启动；测试库可用且有审计数据；
  有一个具备 `audit:events:read` 权限的账号。
- **测试步骤**:
  1. 进入审计查询页面；
  2. 翻页；
  3. 按操作人与动作过滤；
  4. 用**无权限**账号访问该页面；
  5. 检查步骤 4 期间发出的**目标接口请求数**。
- **预期结果**:
  - 步骤 1 列出审计记录，且**同时显示**操作人、时间、动作、对象、来源 IP；
  - 步骤 2 分页可用，翻页后记录**不重复**（排序为 `createdAt DESC, id DESC`）；
  - 步骤 3 过滤生效，结果与后端接口一致；
  - 步骤 4 落 **403 页**，且 403 页**不是**空白页；
  - 步骤 5 **目标接口请求数 == 0**（无权限时**不发起**目标数据请求）。
- **类型**: 功能

> **必须是服务端分页**（D145 / S-35）：审计数据量大且持续追加，
> 不得照抄既有受保护页的**客户端**分页。
> 步骤 5 是既有守卫机制的硬要求（与 `App.svelte` 的既有行为一致）。

---

### AC-S-133: 敏感值防护：`before` / `after` 含敏感键时写入被拒绝

> 改动（D156 —— 14 键基线、完整键名匹配、递归、两侧都做 + 一致性测试）。

- **前置条件**: 测试库可用。
- **测试步骤**:
  1. 尝试记录一条 `before` 为 `{"password":"p@ssw0rd"}` 的事件；
  2. 记录一条 `before` 为 `{"name":"ok"}` 的正常事件；
  3. 检索该租户**全部审计记录的全部列的文本拼接**；
  4. 用一个**可单独调用**的敏感键扫描器扫一段**确实含**敏感键的样例串
     （如 `{"a":{"b":{"api_key":"x"}}}`）；
  5. 用同一个扫描器扫一段**不含**敏感键的样例串（如 `{"secretary":"x","tokenizer":"y"}`）。
- **预期结果**:
  - 步骤 1 **被拒绝**，返回**明确错误码** `AUDIT_SENSITIVE_PAYLOAD_REJECTED`
    （不是 500），且该事件**未落库**；
  - 步骤 2 正常落库；
  - 步骤 3 的检索结果**不含** `p@ssw0rd`；
  - 步骤 4 的扫描器**命中** `api_key`（**含嵌套层**）—— 阳性对照，
    否则「0 命中」不可信（dev-guide §9.2）；
  - 步骤 5 的扫描器**不命中**（`secretary` / `tokenizer` **不是**敏感键 ——
    匹配按**完整键名**，**不是子串**）。
- **类型**: 安全

> **基线键（恰好 14 个）**（D156）：
> `password` / `passwd` / `pwd` / `secret` / `client_secret` / `token` /
> `access_token` / `refresh_token` / `id_token` / `authorization` /
> `credential` / `api_key` / `private_key` / `cookie`
>
> **归一化**：键名转小写，`-` 视为 `_`。**匹配**：归一化后与基线键**精确相等**。
> **递归**：检查**每一层**。
> **两侧都做**：DB 函数内（权威）+ Rust 侧 `scan_sensitive_keys()`
> （供测试/工具/提前拦截）→ 两侧键集合**必须逐项相等**（加一条**一致性测试**）。
