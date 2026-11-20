# 审计写入完整性与 append-only（AC-S-122、AC-S-123）

- **覆盖 AC**: AC-S-122（11 条：TC-014-001 ~ 011）、AC-S-123（5 条：TC-014-012 ~ 016）
- **前置基线**:
  - 测试库 `base_test`（`DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test`）
  - 租户 `t-audit`（T-A）、`t-other`（T-B）已存在
  - 管理员连接 = `base`（表 owner + 超级用户）；**应用连接 = `base_app`**（`base_platform_app` 组角色成员）
  - 审计表已由迁移 `migrations/20261120000200_audit.up.sql` 建好
- **实现事实（用例按此写）**:
  - `base_platform_app` 是 **NOLOGIN 组角色**（`deploy/seed/02-app-role.sql:19-23`），
    能登录的是 `base_app`（`:33-37`），它是组角色成员（`:40`）
  - 表 owner 是 `base`（迁移执行者）→ **REVOKE 对 `base_app` 生效**（owner 豁免不适用于 `base_app`）
  - ⚠️ `deploy/seed/02-app-role.sql:50` 有**全表 blanket GRANT**，
    `:69-70` 有 `ALTER DEFAULT PRIVILEGES … GRANT SELECT, INSERT, UPDATE, DELETE`
    → 重跑种子脚本会把 UPDATE/DELETE **重新授给**审计表（实测见 TC-014-016）
  - 契约 C12（`BA/contracts/api-conventions/v1.md:165-171`）固定 **13 个字段名**，不约束类型
  - 既有惯例：租户上下文是**事务局部**的
    （`crates/platform-db/src/tenant.rs:28` `set_config('app.tenant_id', $1, true)`）

---

## AC-S-122：一次敏感操作产生一条含全部固定字段的审计记录

### TC-014-001: 一次敏感操作产生**恰好 1 条**审计记录

- **关联 AC**: AC-S-122
- **前置条件**: T-A 下该 `action` 当前记录数为 0（`SELECT count(*) FROM platform_audit_log WHERE tenant_id='t-audit' AND action='demo.update'`）
- **测试步骤**:
  1. 通过平台审计 sink 记录一条事件：`action="demo.update"`、`object_type="demo_item"`、`object_id="obj-1"`、`before={"name":"old"}`、`after={"name":"new"}`、`actor_id="u-1"`
  2. 用管理员连接查 `SELECT count(*) FROM platform_audit_log WHERE tenant_id='t-audit' AND action='demo.update'`
- **预期结果**:
  1. 步骤 2 的计数 `== 1`（**不是 0，也不是 2**）
  2. 该行的 `tenant_id == 't-audit'`、`actor_id == 'u-1'`
  3. 该行 `id` 非空
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 「恰好 1 条」是 AC 的核心判据 —— 幂等写入（重复落库）只能靠计数暴露。

### TC-014-002: 13 个固定字段**逐字**存在（契约 C12）

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条事件
  2. `SELECT attname FROM pg_attribute WHERE attrelid='public.platform_audit_log'::regclass AND attnum>0 AND NOT attisdropped`
  3. 读该行的列名集合
- **预期结果**:
  1. 步骤 3 的列名集合 `==` `{id, tenant_id, actor_id, action, object_type, object_id, before, after, ip, user_agent, created_at, prev_hash, hash}`
     （**13 个，逐字比对，不多不少**）
  2. 步骤 2 的输出**不包含**任何其它业务列（归档表除外，见 TC-014-093）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（集合精确匹配）
- **备注**: 契约 C12 只固定**字段名**，不固定类型与可空性 → 本用例只锁名字。〔S-28〕

### TC-014-003: `action` / `object_type` / `object_id` 与传入**逐字相同**

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条事件，`action="demo.update"`、`object_type="demo_item"`、`object_id="obj-1"`
  2. 读回三列
- **预期结果**:
  1. `action == "demo.update"`（**逐字节相等**，不改大小写、不加前缀、不替换分隔符）
  2. `object_type == "demo_item"`
  3. `object_id == "obj-1"`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: 设计 §6.1 定的命名法是 `<域>.<对象>.<动作>`；AC 用的是 `demo.update`（两段），
  **AC 与 design 的命名法不一致** → Dev 若按 design 规范化 `demo.update` 会得到 `demo.demo-item.update`，逐字相等即失败。〔S-26〕

### TC-014-004: `before` / `after` 解析后**值相等**（键序无关）

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条事件，`before={"name":"old","n":1}`、`after={"name":"new","n":2}`
  2. 读回两列的**原始文本**
  3. 解析成 JSON 后比较
- **预期结果**:
  1. 两列**都非 NULL**
  2. 解析后 `before == {"name":"old","n":1}`、`after == {"name":"new","n":2}`（**值相等，不要求键序**）
  3. 存储文本本身是**紧凑**的（不含 `, ` / `: ` 形式的空白）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: 「值相等即可，不要求键序」这条 AC 写得对（与 D142 的**存储文本**规范化不冲突：键排序是存储形态要求，比较时是值比较）。

### TC-014-005: 无前后值时 `before` / `after` 为 **NULL**（不是空串、不是 `"null"`）

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条**不带** `before` / `after` 的事件（如 `action="demo.touch"`）
  2. 读回两列
- **预期结果**:
  1. 两列 `IS NULL`（**不是** `''`，**不是** `'null'`，**不是** `'{}'`）
  2. 该行仍能通过链校验（`hash` 可被独立复算，见 TC-014-021）
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（空 / 空串 / null / 空对象）
- **备注**: D142 §3 规定哈希里 NULL 用**零字节空串**参与拼接 —— 列值与哈希输入是两件事，本用例同时锁住两者。

### TC-014-006: `ip` / `user_agent` **列存在**，取不到时是**空串**而非缺列

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条**不带** IP / UA 的事件
  2. 读回两列
  3. 查 `information_schema.columns` 里这两列的 `is_nullable` 与 `column_default`
- **预期结果**:
  1. 两列**存在**于表定义中（步骤 3 的结果里有它们）
  2. 步骤 2 读回 `''`（**空串**，不是 `NULL`，不是列缺失）
  3. 两列 `is_nullable == 'NO'` 且 `column_default` 为 `''::text`（对应 design §3.1 的 `TEXT NOT NULL DEFAULT ''`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: AC 说「存在该列（无值时为空串，不得缺列）」—— **在 DB 列层面验**，不在 API 响应层验。〔S-27〕

### TC-014-007: `created_at` 非空且为 UTC `timestamptz`

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条事件
  2. 读回 `created_at`
- **预期结果**:
  1. `created_at IS NOT NULL`
  2. 列类型是 `timestamp with time zone`
  3. 该值以 UTC 呈现（`AT TIME ZONE 'UTC'` 后的小时数落在写入时刻附近，误差 < 60 秒）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 契约 C13 要求「时间一律 UTC 存储（timestamptz）」。

### TC-014-008: `prev_hash` / `hash` 均为 **64 位小写十六进制**

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条事件
  2. 读回两列
- **预期结果**:
  1. 两列都匹配 `^[0-9a-f]{64}$`（**小写**；大写十六进制 **不通过**）
  2. `prev_hash` 是 64 个字符，**不是** 63 或 65
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（大小写 / 长度）
- **备注**: 「64 位小写十六进制」是可机械判定的正则，AC 写对了。

### TC-014-009: 存储的 JSON 文本是**规范化**的（递归键排序 + 紧凑 + 非 ASCII 不转义）

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条事件，`before={"b":1,"a":{"z":1,"y":{"n":2,"m":3}}}`、`after={"名字":"中文"}`
  2. 读回两列的**原始文本**（不解析）
- **预期结果**:
  1. `before` 列的文本 `==` `{"a":{"y":{"m":3,"n":2},"z":1},"b":1}`
     （**递归**按 UTF-8 字节序排序；嵌套对象也排序）
  2. `after` 列的文本 `==` `{"名字":"中文"}`（**非 ASCII 不转义** —— 不是 `{"名字":"\u4e2d\u6587"}`）
  3. 两列文本都不含 `": "` / `", "` 这类空白
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: D142 §2 把字节级格式钉死了。**注意**：本用例锁的是**存储形态**，
  校验方**不依赖**这套规则（读回列里的 TEXT 直接哈希，design §4.2.2）。
  所以「规范文本」与「哈希可复算」是两条独立判据，不要混成一条。〔S-07〕

### TC-014-010: `actor_id` 可空（未认证场景）

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条**不带** `actor_id` 的事件
  2. 读回 `actor_id`
- **预期结果**:
  1. `actor_id IS NULL`（不是 `''`、不是 `"anonymous"`）
  2. 该行仍能被独立复算验证（NULL 在哈希里按**零字节空串**参与，design §4.2 表）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: 登录失败场景必然是无 actor（decisions.md D132）。

### TC-014-011: `actor_id` 非空时**逐字**等于传入值

- **关联 AC**: AC-S-122
- **前置条件**: 同 TC-014-001
- **测试步骤**:
  1. 记录一条事件，`actor_id="u-1"`
  2. 读回 `actor_id`
- **预期结果**:
  1. `actor_id == "u-1"`（逐字）
  2. 该值**不是** 邮箱 / 用户名 / display name（不是从用户表 join 出来的）
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 与 TC-014-003 的「逐字相同」同一判据族，但 actor 单独成条是因为它可空，判据不同。

---

## AC-S-123：应用角色无法 UPDATE / DELETE 审计记录

### TC-014-012: 应用角色 `UPDATE` 审计记录 → `permission denied`

- **关联 AC**: AC-S-123
- **前置条件**: T-A 下至少 1 条审计记录（记其 `id`）；**应用连接 = `base_app`**
- **测试步骤**:
  1. 以 `base_app` 执行 `UPDATE platform_audit_log SET after = '{}' WHERE id = <该 id>`
  2. 记录错误信息与 SQLSTATE
- **预期结果**:
  1. 语句**失败**（非 0 退出 / `Err`）
  2. 错误信息含 `permission denied`（**不是** `row-level security`、不是 `not found`）
  3. SQLSTATE `== 42501`（`insufficient_privilege`）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径
- **备注**: **必须区分「权限不足」与「RLS 过滤」** —— RLS 只挡读不挡写，写失败只可能来自 REVOKE。
  若实现改成「靠 RLS」，本用例会红（错误信息不含 `permission denied`）。

### TC-014-013: 应用角色 `DELETE` 审计记录 → `permission denied`

- **关联 AC**: AC-S-123
- **前置条件**: 同 TC-014-012
- **测试步骤**:
  1. 以 `base_app` 执行 `DELETE FROM platform_audit_log WHERE id = <该 id>`
  2. 记录错误信息与 SQLSTATE
- **预期结果**:
  1. 语句**失败**
  2. 错误信息含 `permission denied`
  3. SQLSTATE `== 42501`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径
- **备注**: 同 TC-014-012。UPDATE 与 DELETE 分开成条：两者是**独立的** REVOKE，
  实现可能只落一个（只 REVOKE UPDATE 不 REVOKE DELETE 是常见疏漏）。

### TC-014-014: 两条失败语句之后，行数与内容**逐字段不变**

- **关联 AC**: AC-S-123
- **前置条件**: 同 TC-014-012；先记录该行的**全列快照**
- **测试步骤**:
  1. 执行 TC-014-012 与 TC-014-013 的两条语句
  2. 统计 `platform_audit_log` 的总行数
  3. 重新读回该行，逐列与步骤 0 的快照比对
- **预期结果**:
  1. 步骤 2 的行数与执行前**完全相同**
  2. 步骤 3 的 13 个列值**逐个相同**（含 `hash` / `prev_hash` / `after`）
  3. 该行的 `ctid` 未变（**证明不是「删了又插回来」**）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径 + 状态快照
- **备注**: `ctid` 这一条能挡住「UPDATE 被拒但触发了某种重建」的伪实现。

### TC-014-015: 用 `base_platform_app` **直接连接**必然失败（说明「应用角色」指谁）

- **关联 AC**: AC-S-123
- **前置条件**: `base_platform_app` 角色已存在（`deploy/seed/02-app-role.sql:19-23`）
- **测试步骤**:
  1. 尝试以 `base_platform_app` 建立数据库连接
  2. 以 `base_app` 建立连接并执行 `SELECT current_user, session_user`
- **预期结果**:
  1. 步骤 1 **失败**，错误含 `role "base_platform_app" cannot be logged in`（它是 **NOLOGIN** 组角色）
  2. 步骤 2 返回 `current_user == 'base_app'`
  3. `base_app` 在 `pg_auth_members` 里是 `base_platform_app` 的成员（`SELECT 1 FROM pg_auth_members …` 命中）
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC-S-123 前置写「持有**应用连接角色**（`base_platform_app`）的连接」—— 字面上不可执行
  （该角色不能登录）。用例按**能连的那个**写：`base_app`。〔S-14〕

### TC-014-016: 种子脚本重跑后，append-only **仍然成立**（回归护栏）

- **关联 AC**: AC-S-123
- **前置条件**: 审计表已建好且有记录；`deploy/seed/02-app-role.sql` 可重复执行
- **测试步骤**:
  1. 记录审计表当前 ACL：`SELECT relacl::text FROM pg_class WHERE relname='platform_audit_log'`
  2. 以管理员连接重跑 `deploy/seed/02-app-role.sql` 的**存量授权**语句
     （`GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO base_platform_app;`）
  3. 再次记录 ACL
  4. 以 `base_app` 执行 `UPDATE platform_audit_log SET after='{}' WHERE id=<某条>`
- **预期结果**:
  1. 步骤 3 的 ACL **仍然不含** `w`（UPDATE）与 `d`（DELETE）—— 即 `base_platform_app=ar` 或更严
  2. 步骤 4 **失败**，错误含 `permission denied`
  3. 审计表行数不变
- **类型**: 安全
- **优先级**: P0
- **自动化**: 半自动
- **自动化理由**: 判据本身可机械断言，但**前置动作**是执行部署脚本（DB 运维动作），
  不适合放进常规测试套件 —— 放在「部署/种子幂等」检查里，或由 QA 在验收前手工跑一次。
- **设计方法**: 回归护栏
- **备注**: **本条是本需求最容易踩的坑**。实测（`base_test`，临时表已清理）：
  新表建出来 → `base_platform_app=arwd`（`ALTER DEFAULT PRIVILEGES` 自动授的）→
  迁移里 `REVOKE UPDATE, DELETE` → `base_platform_app=ar`（append-only 成立）→
  **重跑种子的 blanket GRANT** → `base_platform_app=arwd` →
  `base_app` 的 `UPDATE` **成功执行**（无 permission denied）。
  `deploy/seed/02-app-role.sql:53-66` 已经为 `platform_config` 做了例外块，
  **审计表没有对应例外块** → 必须在种子脚本里补同样的例外，或在迁移里追加一条「收权」语句。〔S-03〕

---
