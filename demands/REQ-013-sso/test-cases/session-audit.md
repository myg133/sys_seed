# SSO 会话同构与审计事件（AC-S-107、AC-S-108）

- **覆盖 AC**: AC-S-107（9 条：TC-013-111 ~ 119）、AC-S-108（9 条：TC-013-120 ~ 128）
- **前置基线**:
  - `tenant-a`：`sso.enforce=false`；本地账号 `grace`、SSO 账号 `bob`（已绑定）
  - 两个账号各自无历史会话（`DELETE FROM platform_iam_session WHERE tenant_id='tenant-a'`）
- **实现事实（用例按此写）**:
  - `SESSION_TABLE` 的读语句逐列列举（`crates/platform-iam/src/store.rs:582-612`），
    `SessionRecord` 由 `tenant_row!` 宏按**列名**映射（`crates/platform-db/src/lib.rs:100-112`）
    → 加了 DB 列但没加进 SELECT，`try_get` 会在运行期报「列不存在」（不是编译错）
  - `SessionView` 当前 6 个字段、**没有** `authProvider`
    （`crates/platform-iam/src/service.rs:147-159`）
  - `revoke_session` 返回 `204 No Content`（`crates/platform-iam/src/http.rs:491`）
  - `platform_core` **没有** `AuditRecorder`；只有 `Auditable` + `AuditEventSpec`
    （`crates/platform-core/src/extension/auditable.rs:8-45`）
  - `platform-org` 的 `RecordingAuditRecorder` 绑定 `OrgAuditEvent`（`crates/platform-org/src/audit.rs:64-118`），
    事件结构只有 `code/action/object_type/object_id/detail` ——**没有结构化 payload**

---

## AC-S-107：SSO 会话与本地会话同构

### TC-013-111: 两种登录的会话都出现在同一套列表接口里

- **关联 AC**: AC-S-107
- **前置条件**: 基线
- **测试步骤**:
  1. `grace` 本地登录，取 `accessToken_local`
  2. `bob` OIDC 登录，取 `accessToken_sso`
  3. 分别 `GET /api/iam/sessions`（各自令牌 + `x-tenant-id: tenant-a`）
- **预期结果**:
  1. 两次列表请求都 `200`
  2. `grace` 的列表长度 `== 1`，`bob` 的列表长度 `== 1`
  3. 两次响应的元素键集合**相等**（同一个 `SessionView` 类型）
  4. 不存在第二个会话列表端点的痕迹（无 `/api/iam/sso/sessions` 路由：
     `GET` 该路径返回 `404`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-112: 会话来源字段存在且可判定（`authProvider`）

- **关联 AC**: AC-S-107
- **前置条件**: 同 TC-013-111
- **测试步骤**:
  1. 取两条会话记录的键集合
  2. 读 `authProvider` 值
- **预期结果**:
  1. `SessionView` 的键集合 `==` `{id, deviceLabel, ip, userAgent, createdAt, lastUsedAt, authProvider}`
     （**7 个键**）
  2. `grace` 的 `authProvider == "local"`；`bob` 的 `authProvider == "oidc"`
  3. 两个值都来自**枚举**，不是自由文本：取值必须 ∈ `{local, oidc, ldap}`
  4. `authProvider` 的值**不等于** `deviceLabel` 的任何子串（不是靠 UA 猜出来的）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: **该字段在现有代码里不存在**：`SessionView`
  （`crates/platform-iam/src/service.rs:147-159`）只有 6 个字段；
  design §2.2 只加了**数据库列**，§8 的落点表也没提 `SessionView` 与 SQL。
  本用例在补齐前必红。〔S22〕

### TC-013-113: `DELETE /sessions/{id}` 返回 204，随后 refresh 得到 `TOKEN_REVOKED`

- **关联 AC**: AC-S-107
- **前置条件**: `bob` 有一个 SSO 会话，记 `session_id` 与 `refreshToken`
- **测试步骤**:
  1. `DELETE /api/iam/sessions/{session_id}`，Bearer + `x-tenant-id`
  2. `POST /api/iam/refresh`，body 用步骤 0 的 `refreshToken`
- **预期结果**:
  1. 步骤 1 状态码 `204`，响应体为空（长度 `0`）
  2. 步骤 2 状态码 `401`，`code == "TOKEN_REVOKED"`
  3. 步骤 2 的 `code != "TOKEN_REUSE_DETECTED"`
  4. `platform_iam_session` 该行 `revoked_at IS NOT NULL`，且**同 `family_id` 的所有行**都非空
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: `TOKEN_REVOKED` 与 `TOKEN_REUSE_DETECTED` 的分支顺序已由既有实现固定
  （`crates/platform-iam/src/service.rs:698-710`：`used_at` 判定在 `revoked_at` 之前）。
  AC 只写「refresh 失败（`TOKEN_REVOKED`）」，本用例钉死状态码与响应体长度。

### TC-013-114: `logout-all` 作废该用户的全部 family，含 SSO 会话

- **关联 AC**: AC-S-107
- **前置条件**: `bob` 有两个并行会话（两次 OIDC 登录，两个 `family_id`），
  各自的 `refreshToken` 记为 R1 / R2
- **测试步骤**:
  1. `POST /api/iam/logout-all`（Bearer = 第一个 access token + `x-tenant-id`）
  2. 用 R1、R2 各刷一次
  3. `GET /api/iam/sessions`
- **预期结果**:
  1. 步骤 1 `200` 或 `204`（BA 需钉死；既有实现是 `204`）
  2. 步骤 2 两次都 `401`，`code == "TOKEN_REVOKED"`
  3. 步骤 3 返回空数组 `[]`
  4. 库里该用户所有 `platform_iam_session.revoked_at` 非空
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移

### TC-013-115: 两条会话的表结构同构（列集合差集为空）

- **关联 AC**: AC-S-107
- **前置条件**: 一条本地会话 + 一条 SSO 会话
- **测试步骤**:
  1. 查两条行的全部列名（`SELECT *` 后取列名）
  2. 比较列名集合
- **预期结果**:
  1. 两条行的列名集合**完全相等**（同一个表、同一批列）
  2. 只有 `auth_provider` 的**值**不同（`local` vs `oidc`）
  3. `family_id` / `id` / `created_at` 都非空
  4. `information_schema.columns` 中 `platform_iam_session` 的列包含
     `auth_provider`，其 `data_type='text'`、`is_nullable='NO'`、`column_default='local'`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: AC 步骤 5 写「结构字段同构」并说「不得两套会话表」——这两条要分开验
  （本用例 + TC-013-116）；AC 把它们压在一步里，且「字段集合一致」（步骤 2）与
  「能区分来源」（步骤 2 后半）在**同一字段上互相矛盾**（一致就不可能不同）。〔S25〕

### TC-013-116: 不存在 SSO 专用会话表

- **关联 AC**: AC-S-107
- **前置条件**: 迁移已跑
- **测试步骤**:
  1. `SELECT table_name FROM information_schema.tables WHERE table_schema='public' AND (table_name LIKE '%sso%' OR table_name LIKE '%oidc%')`
  2. 阳性对照：`... LIKE '%iam_session%'`
- **预期结果**:
  1. 步骤 1 结果只有 `platform_iam_oidc_state` 一行（凭证表，不是会话表）
  2. **没有** `*sso*session*` / `*oidc*session*` 表
  3. 步骤 2 至少有 `platform_iam_session` 一行（证明查询有效）
  4. SSO 会话的行都在 `platform_iam_session` 里（`auth_provider='oidc'` 行数 `== 1`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-117: access token 的 claim 结构与本地登录**完全一致**

- **关联 AC**: AC-S-107
- **前置条件**: 两次登录的 `accessToken`
- **测试步骤**:
  1. `decode_header` 取两次的 `alg` 与 `kid`
  2. 验签后取 payload 的键集合
  3. 调 `GET /.well-known/jwks.json`
- **预期结果**:
  1. 两次 `alg == "ES256"`
  2. 两次 `kid` 相等，且都出现在 JWKS 的 `keys[].kid` 中（**同一个 JWKS**）
  3. 两次 payload 键集合 `==` `{sub, tid, sid, iss, aud, iat, nbf, exp, jti}`（9 个）
  4. 两条 token 的 `tid == "tenant-a"`，`iss` / `aud` 相等
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 键集合依据 `AccessClaims`（`crates/platform-iam/src/jwt.rs:32-62`）+ `SIGNING_ALGORITHM`
  （同文件 `:28`）。任何「SSO 专用 token 体系」都会在这里露出来。

### TC-013-118: SSO access token 能被既有校验器接受（同一个 verifier）

- **关联 AC**: AC-S-107
- **前置条件**: `bob` 的 SSO `accessToken`
- **测试步骤**:
  1. `GET /api/rbac/roles`（需要一个 RBAC 端点，且 `bob` 有 `rbac:role:read`）
  2. `GET /api/iam/sessions`
- **预期结果**:
  1. 两次都 `200`
  2. 都**不**返回 `401 UNAUTHENTICATED`
  3. 响应体里没有「token 来源不受支持」类错误
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-119: SSO 会话的「最近活跃」与「踢下线」走同一套逻辑

- **关联 AC**: AC-S-107
- **前置条件**: `bob` 一个 SSO 会话
- **测试步骤**:
  1. 记 `last_used_at`
  2. `POST /api/iam/refresh` 一次（用该会话的 refresh token）
  3. 再查 `last_used_at`
  4. `DELETE /api/iam/sessions/{id}` 后，用该会话的旧 access token 打
     `GET /api/iam/sessions`
- **预期结果**:
  1. 步骤 3 的 `last_used_at` **严格大于**步骤 1 的值
  2. 步骤 4 状态码 `401`（access token 因 `sid` 已撤销而失效）
  3. 步骤 4 的 `code == "UNAUTHENTICATED"`（既有实现）或 `TOKEN_REVOKED`（BA 需钉死其一）
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: `〔守卫〕` —— AC 说「SSO 会话与本地会话同样可被踢下线」，但没说
  「access token 是否立即失效」。既有实现靠 `sid` + 会话状态判定，
  究竟是 `401 UNAUTHENTICATED` 还是 `TOKEN_REVOKED` 需要钉死。

---

## AC-S-108：审计事件「声明 + 发出」，本轮不落库

> **前置说明（两条硬事实，改写前的 AC 在这上面都落不了地）**：
> 1. `platform_core` **没有** `AuditRecorder` trait，只有 `Auditable` + `AuditEventSpec`
>    （`crates/platform-core/src/extension/auditable.rs:8-45`），
>    而 `AuditEventSpec` 的字段是 `code / action / object_type / description` —— **无 payload**。
> 2. `platform-org` 的 `RecordingAuditRecorder` 绑定 `OrgAuditEvent`
>    （`crates/platform-org/src/audit.rs:64-118`），`platform-iam` **不能**直接用它。
>
> 因此本节用例要求 `platform-iam` 交付 **自己的** `SsoAuditEvent`（含结构化 payload）
> 与 `SsoRecordingAuditRecorder`，并沿用 `Auditable` 声明事件。

### TC-013-120: `platform-iam` 声明 3 个审计事件，每个带 code / action / objectType

- **关联 AC**: AC-S-108
- **前置条件**: 代码已交付
- **测试步骤**:
  1. 取 `platform-iam` 的审计事件声明（`<iam 模块>.audit_events()` 或常量表）
  2. 逐个取 `code` / `action` / `object_type`
- **预期结果**:
  1. 声明数量 `>= 3`
  2. 包含 `iam.sso.login.succeeded`、`iam.sso.login.failed`、`iam.sso.link.created`
  3. 每个事件的三个字段都**非空**
  4. 事件码在声明集合内**唯一**（无重复）
  5. 事件码格式匹配 `^iam\.sso\.[a-z]+\.[a-z]+$`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 「怎么读到声明」在当前设计里**没有落点**：`Auditable` 只有 `platform-org` 在用
  （`crates/platform-org/src/lib.rs:101-105`），平台侧没有聚合器。BA 需指定读取面
  （`IamModule` 结构 / `pub const SSO_AUDIT_EVENTS`）。〔S8 关联〕

### TC-013-121: OIDC 成功登录发出 `iam.sso.login.succeeded`，payload 字段齐全

- **关联 AC**: AC-S-108
- **前置条件**: 注入 `SsoRecordingAuditRecorder`；完整走一次 OIDC 登录（`bob`）
- **测试步骤**:
  1. 登录
  2. 取录到的事件，过滤 `code == "iam.sso.login.succeeded"`
- **预期结果**:
  1. 步骤 1 状态码 `200`
  2. 步骤 2 恰好 `1` 条
  3. 该事件 payload 的键集合 `==` `{provider, externalSubject, userId, ip, userAgent}`
  4. `provider == "oidc"`；`externalSubject == "idp-bob-001"`；`userId == <bob.id>`
  5. `ip` 等于请求的 `x-forwarded-for`；`userAgent` 等于请求的 `User-Agent`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: **payload 这个概念在现有接缝里不存在**：`OrgAuditEvent` 只有一个
  `detail: String`（`platform-org/src/audit.rs:28-41`）。要满足本用例必须新增结构化 payload 类型，
  这不是「照搬 org 模式」能得到的。〔S8〕

### TC-013-122: OIDC 失败登录发出 `iam.sso.login.failed`，且 code 与真实失败原因一致

- **关联 AC**: AC-S-108
- **前置条件**: 注入 recorder；触发 `alg=none`
- **测试步骤**:
  1. 回调（`alg=none`）
  2. 取 `code == "iam.sso.login.failed"` 的事件
- **预期结果**:
  1. 恰好 `1` 条
  2. payload 键集合 `==` `{provider, errorCode, ip}`
  3. `provider == "oidc"`
  4. `errorCode == "SSO_OIDC_ALG_NOT_ALLOWED"`（与 HTTP 响应体的 `code` **逐字相同**）
  5. payload **不含**键名 `token` / `idToken` / `claims` / `password`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-123: LDAP 成功与失败各一条，`provider == "ldap"`

- **关联 AC**: AC-S-108
- **前置条件**: 注入 recorder；`erin` 正确密码一次、错密码一次
- **测试步骤**:
  1. 两次 LDAP 登录
  2. 取事件
- **预期结果**:
  1. `succeeded` 恰好 `1` 条，`provider == "ldap"`
  2. `failed` 恰好 `1` 条，`provider == "ldap"`，`errorCode == "SSO_LDAP_BIND_FAILED"`
  3. 两条事件的 `externalSubject` 都不含完整 DN（不含 `ou=people`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-124: JIT 建绑定发出 `iam.sso.link.created`

- **关联 AC**: AC-S-108
- **前置条件**: 注入 recorder；`sso.jitProvisioning=true`；`carol` 未绑定
- **测试步骤**:
  1. 走一次 OIDC 登录
  2. 取 `code == "iam.sso.link.created"` 的事件
- **预期结果**:
  1. 恰好 `1` 条
  2. payload 含 `provider`、`externalSubject`、`userId`，且 `userId` 等于新建的 user
  3. 事件条数 `== 1`（不重复发）
  4. 已绑定用户再登录时**不**发 `link.created`（第二条请求后仍为 `1` 条）
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 状态迁移

### TC-013-125: 事件 payload 的敏感值扫描 0 命中（含阳性对照）

- **关联 AC**: AC-S-108
- **前置条件**: `BASE_SSO_OIDC_CLIENT_SECRET=SECRET-CANARY-7f3a9b1e-UNIQUE`；
  `bob` 的 refresh token 与 id_token 原文各留一份
- **测试步骤**:
  1. 注入 recorder，跑成功 + 失败各一次
  2. 把**全部**事件的 JSON 序列化后搜：
     canary、`id_token` 原文、refresh token 原文、本地密码 `Al1ce-correct`、
     目录密码 `Er1n-pass`
  3. 阳性对照：构造一条 payload 故意带 canary 的事件，用同一扫描器扫
- **预期结果**:
  1. 步骤 2 的 5 个串命中数均为 `0`
  2. 步骤 3 命中数 `>= 1`
  3. payload 里**不含** `email` / `name` 等完整 claim（§6 只允许 provider/sub/结果/码/IP/UA）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-126: 默认 `Noop` 实现下登录仍成功，且有 WARN 级可观测提示

- **关联 AC**: AC-S-108
- **前置条件**: 不注入 recorder（走默认实现）；捕获 stdout
- **测试步骤**:
  1. 完整 OIDC 登录一次
  2. 在日志里找审计相关的告警行
- **预期结果**:
  1. 状态码 `200`（审计缺失不阻断登录）
  2. 日志里至少有 `1` 行同时满足：`level=WARN`（或更高）且消息包含 `audit`
  3. 该行**不是** `ERROR`（登录没失败）
  4. 连续两次登录后该告警行数 `>= 1`（不要求每请求一次，但不得为 0）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-127: 本轮**不落库** —— 不存在审计表

- **关联 AC**: AC-S-108
- **前置条件**: 迁移已跑
- **测试步骤**:
  1. `SELECT table_name FROM information_schema.tables WHERE table_schema='public' AND table_name LIKE '%audit%'`
  2. 跑一次成功登录 + 一次失败登录
  3. 再查一遍步骤 1
- **预期结果**:
  1. 步骤 1 结果行数 `== 0`
  2. 步骤 3 与步骤 1 相同（行数仍 `0`）
  3. 事件只在 recorder 的内存里（`recorder.events().len() >= 2`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 这是**反向断言**（断言「没有」）。它是 AC-S-108 第 7 条「明确不验落库」的可执行形式：
  若有人顺手建了审计表并写库，本用例红，提醒把口径同步给 REQ-014。

### TC-013-128: 事件 payload 用**白名单**键集合，多一个键就红

- **关联 AC**: AC-S-108
- **前置条件**: 同 TC-013-121
- **测试步骤**:
  1. 收集全部事件 payload 的键名并集
  2. 与允许集合求差
- **预期结果**:
  1. 键名并集 `==` `{provider, externalSubject, userId, errorCode, ip, userAgent, createdUserId}`
  2. 差集为空（**不允许额外键**，例如 `rawClaims`、`idToken`）
  3. 任何值的长度 `<= 512` 字符（防止有人把整个 claim 集塞进 `detail`）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: AC 只列「payload 含 X」这种**下界**，下界断言对「多塞了敏感字段」完全无感。
  本用例把它补成上界。
