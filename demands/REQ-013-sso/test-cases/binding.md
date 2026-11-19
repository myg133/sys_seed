# 账号绑定与 JIT 建号（AC-S-103）

- **覆盖 AC**: AC-S-103（10 条：TC-013-061 ~ 070）
- **前置基线**: 同 [oidc-flow.md](oidc-flow.md)；额外：
  - 租户 `tenant-a` + 同一 mock IdP 的 `tenant-b`
  - `tenant-a`：`sso.jitProvisioning=false`（TC-013-062 起改为 `true` 时逐个说明）
  - IdP 侧 `sub="idp-carol-001"` / `email="carol@example.com"`；
    `sub="idp-dave-001"` / `email="dave@example.com"`（已绑定本地 `dave`，`dave.status='disabled'`）
  - 底座**无** `carol` 账号、**无** `idp-carol-001` 的绑定
- **关键实现事实（用例按此写）**:
  - 绑定表 `external_identity` 唯一键 `(tenant_id, provider, external_id)`
    （`migrations/20261118000700_org_schema.up.sql:85`）
  - 既有 upsert 是 `ON CONFLICT … DO UPDATE SET user_id = EXCLUDED.user_id`
    （`crates/platform-org/src/store.rs:657-663`）——**语义是「改指」而不是「只增」**
  - `platform_iam_user` 有 `(tenant_id, email)` **部分唯一索引**（`email IS NOT NULL`）
    （`migrations/20261118000400_iam_auth.up.sql:48-50`）

---

### TC-013-061: JIT 关闭 + 未绑定 → 403 `SSO_ACCOUNT_NOT_LINKED`，且**不建任何行**

- **关联 AC**: AC-S-103
- **前置条件**: `sso.jitProvisioning=false`；`carol` 无 user、无绑定
- **测试步骤**:
  1. 完整走一次 OIDC 登录（authorize → 回调，`sub=idp-carol-001`）
  2. 查 `platform_iam_user WHERE tenant_id='tenant-a' AND email='carol@example.com'`
  3. 查 `external_identity WHERE tenant_id='tenant-a' AND external_id='idp-carol-001'`
- **预期结果**:
  1. 状态码 `403`，`code == "SSO_ACCOUNT_NOT_LINKED"`
  2. 步骤 2 行数 `== 0`
  3. 步骤 3 行数 `== 0`
  4. `platform_iam_session` 新增 `0` 行
  5. 响应体不含 `"carol"`（不回显外部身份）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-062: JIT 打开 → 200，建 user + 建绑定，字段逐值正确

- **关联 AC**: AC-S-103
- **前置条件**: `sso.jitProvisioning=true`
- **测试步骤**:
  1. 完整走一次 OIDC 登录（`sub=idp-carol-001`）
  2. 查 user 表与绑定表
- **预期结果**:
  1. 状态码 `200`，返回 `accessToken`
  2. `platform_iam_user` 新增 **1** 行：`tenant_id='tenant-a'`、`email='carol@example.com'`、
     `status='active'`
  3. `external_identity` 新增 **1** 行：`provider='oidc'`、`external_id='idp-carol-001'`、
     `user_id` 指向上一步的 user
  4. `(tenant_id, provider, external_id)` 唯一约束存在：
     `SELECT conname FROM pg_constraint WHERE contype='u'` 含 `external_identity_uq`
  5. 新 user 的 `password_hash` **不是** LDAP/OIDC 密码的摘要：该列以 `$argon2id$v=19$` 开头，
     或为「不可登录」的固定占位（BA 需钉死其一）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: `password_hash` 是 `NOT NULL`（`iam_auth.up.sql:31`），JIT 建号**必须**给一个值。
  AC/design 都没说给什么 —— 「空串」会被 `verify_password` 当成永不匹配（可行），
  但必须钉死，否则无法判定。〔S33〕

### TC-013-063: 已绑定用户第二次登录 —— 复用同一个 user

- **关联 AC**: AC-S-103
- **前置条件**: TC-013-062 已建号
- **测试步骤**:
  1. 再走一次完整 OIDC 登录（新 `state`、新 `code`）
  2. 查 `platform_iam_user WHERE email='carol@example.com'` 行数
  3. 查 `external_identity` 行数（该 `external_id`）
- **预期结果**:
  1. 状态码 `200`
  2. 步骤 2 行数 `== 1`
  3. 步骤 3 行数 `== 1`
  4. 两次响应的 `user.id` 相同
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移

### TC-013-064: 绑定到已禁用用户 → 403 `ACCOUNT_DISABLED`，SSO 不绕过禁用

- **关联 AC**: AC-S-103
- **前置条件**: `dave.status='disabled'`；`external_identity(sub=idp-dave-001) → dave`
- **测试步骤**:
  1. 走 OIDC 登录（`sub=idp-dave-001`）
  2. 查 `platform_iam_session` 中 `user_id=<dave>`
- **预期结果**:
  1. 状态码 `403`，`code == "ACCOUNT_DISABLED"`
  2. 步骤 2 新增 `0` 行
  3. 响应体不含 `accessToken`
  4. `code != "SSO_ACCOUNT_NOT_LINKED"`（已绑定但禁用，与未绑定可区分）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-065: 同一 `sub` 并发 3 次登录 —— 只产生 1 个 user、1 条绑定

- **关联 AC**: AC-S-103
- **前置条件**: `sso.jitProvisioning=true`；`carol` 既无 user 也无绑定；
  准备 3 个独立有效的 `state`（各自 authorize）
- **测试步骤**:
  1. `tokio::join!` 同时发起 3 个回调（3 个不同 `state`，同一 `sub`）
  2. 统计状态码
  3. 查 user 表与绑定表
- **预期结果**:
  1. `200` 的数量 `>= 1`
  2. **`5xx` 的数量 `== 0`**
  3. `platform_iam_user WHERE email='carol@example.com'` 行数 `== 1`
  4. `external_identity WHERE external_id='idp-carol-001'` 行数 `== 1`
  5. `platform_iam_session` 的行数 `== 步骤 1 的 200 数量`（每个成功登录一个会话）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: **AC 要求「只产生 1 个 user」，但 design 没有给出支撑它的机制**：
  没有 `ON CONFLICT` 的 user 插入、没有 advisory lock、没有唯一键兜 `(tenant_id, external_id)`→user。
  能兜住的只有 `(tenant_id, email)` 唯一索引 —— 而它只在「JIT 按 email 建号」时生效，
  且并发下第二个请求会拿到唯一冲突（必须显式映射成「重查并复用」而不是 500）。
  本用例第 2 条（无 5xx）就是把这个隐性要求写成判据。〔S20〕

### TC-013-066: 同一 `sub` 在 `tenant-b`（未开 JIT）→ 403，且 `tenant-b` 无绑定

- **关联 AC**: AC-S-103
- **前置条件**: `tenant-b` 已配同一 mock IdP，`sso.jitProvisioning=false`；
  `tenant-b` **无** `idp-carol-001` 绑定
- **测试步骤**:
  1. `tenant-b` 走 OIDC 登录（`sub=idp-carol-001`）
  2. 查 `external_identity WHERE tenant_id='tenant-b'`
- **预期结果**:
  1. 状态码 `403`，`code == "SSO_ACCOUNT_NOT_LINKED"`
  2. 步骤 2 行数 `== 0`
  3. `tenant-a` 的 `carol` 仍只有 1 行（未被本次请求影响）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-067: 同一 `sub` 在 `tenant-b`（开 JIT）→ 建**独立** user，不串租户

- **关联 AC**: AC-S-103
- **前置条件**: `tenant-b` 的 `sso.jitProvisioning=true`
- **测试步骤**:
  1. `tenant-b` 走 OIDC 登录（`sub=idp-carol-001`）
  2. 查两租户的 user 与绑定
- **预期结果**:
  1. 状态码 `200`
  2. `platform_iam_user WHERE tenant_id='tenant-b'` 中 `carol@example.com` 行数 `== 1`
  3. 该行 `id !=` `tenant-a` 的 carol 的 `id`
  4. `external_identity` 在两个租户各 1 行，`user_id` 不同
  5. 返回的 `user.tenantId == "tenant-b"`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正交
- **备注**: AC 步骤 7 把「未开 JIT → 403」和「开启 → 建独立 user」塞进**同一个步骤**、
  给了两个互斥前置。本用例只取后者；前者是 TC-013-066。〔AC 修正 S39〕

### TC-013-068: `id_token` 的 `sub` 为空 —— 不得建号、不得绑到任意用户

- **关联 AC**: AC-S-103
- **前置条件**: `sso.jitProvisioning=true`；mock 返回 `sub=""`
- **测试步骤**:
  1. 走 OIDC 登录
  2. 查新增行
- **预期结果**:
  1. 状态码 `400` 或 `401`（BA 需钉死其一），`!= 200`
  2. `platform_iam_user` 新增 `0` 行
  3. `external_identity WHERE external_id=''` 行数 `== 0`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: `〔守卫〕` — `sub` 是绑定的唯一外部键；空 `sub` 若被接受，**所有 IdP 侧无 `sub` 的用户
  会共用一个绑定行**（该行 `DO UPDATE SET user_id` 会把后来者全部改指到一个人）。AC 无此格。

### TC-013-069: JIT 建号时 email 已被**另一个本地账号**占用 —— 不得静默绑定

- **关联 AC**: AC-S-103
- **前置条件**: `sso.jitProvisioning=true`；本地已存在 `username='carol'`、
  `email='carol@example.com'` 的账号 `C_local`（手工建），**但没有任何绑定**
- **测试步骤**:
  1. IdP 侧 `sub="idp-carol-001"` 走 OIDC 登录
  2. 查绑定与 user
- **预期结果**:
  1. **不得** `200`
  2. `external_identity` 新增行数 `== 0`，或新增行的 `user_id == C_local` **且**
     响应 `200` —— 二者必须**择一**并在 AC 里写明
  3. `platform_iam_user` 行数不变（不出现第二个 `carol@example.com`）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: `〔守卫〕` —— 「JIT 遇到同 email 的既有账号」是最常见的接管路径（攻击者控制 IdP
  侧一个 `carol@example.com` 即可接管本地同名账号）。design §D「未绑定 → 建 user + 建绑定」
  与 AC-S-105 的 `SSO_LDAP_IDENTITY_CONFLICT` 对 OIDC 侧**没有对应规则**。必须钉死。〔S33 关联〕

### TC-013-070: 绑定不会被「改指」—— 用同一 `sub` 但不同 `user_id` 的 upsert 必须失败

- **关联 AC**: AC-S-103
- **前置条件**: `carol` 已绑定 `U_carol`（TC-013-062）；手工建 `U_other`（`username='carol2'`）
- **测试步骤**:
  1. 用管理员连接执行一次「按 design §2.3 语义」的 upsert：
     `INSERT INTO external_identity(tenant_id,id,provider,external_id,user_id) VALUES('tenant-a','x','oidc','idp-carol-001','<U_other>') ON CONFLICT (tenant_id,provider,external_id) DO UPDATE SET user_id=EXCLUDED.user_id`
  2. 查该绑定行的 `user_id`
  3. 再走一次正常 SSO 登录，看落到谁
- **预期结果**:
  1. 步骤 2 的 `user_id` 变成 `U_other`（**证实现有 upsert 会静默改指**）——
     这是**缺陷证据**，不是期望行为
  2. 因此在 `platform-iam` 侧**不得**复用这个 `DO UPDATE` 语义：
     正常 SSO 登录（步骤 3）必须**复用** `U_carol`，不得因为库里被改指而落到 `U_other`
  3. 会话的 `user.id` == `U_carol`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 判定表
- **备注**: 本条同时是**证据用例**：`crates/platform-org/src/store.rs:657-663` 的
  `DO UPDATE SET user_id = EXCLUDED.user_id` 表明「写入即改指」。design §2.3 说「照抄最小 upsert」，
  等于把改指语义带进认证路径。安全约束 §5（不越权）没有守这一条。〔S19〕
