# 验收标准：SSO 入站（OIDC + LDAP）与强制 SSO

- **需求编号**: REQ-013
- **父需求**: REQ-001
- **状态**: 已就绪（**rev 2**）
- **更新时间**: 2026-11-19 23:45
- **AC 数量**: **12 条**（AC-S-020 / AC-S-021 重写 + AC-S-100 ~ AC-S-109 新增）

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## 承接的父 AC

**AC-005（SSO 入站三协议）** —— ⚠️ **本轮收窄为两条协议**

| 父 AC-005 要求的协议 | 本轮 | 说明 |
|---|---|---|
| OIDC / OAuth2 | ✅ 交付 | AC-S-100 ~ AC-S-103 |
| LDAP / AD | ✅ 交付 | AC-S-104 / AC-S-105 |
| **SAML 2.0** | ❌ **不交付** | 树里无 XML 库；XMLDSig + C14N 手写验签是 XSW 攻击重灾区。**用户 2026-11-19 裁定「本轮不做 SAML」** |
| CAS | ❌ **不交付** | CAS 2.0 断言是 XML，同受限制 |

> **父 AC-005 的处理**：与 AC-024（工作流）同样处理 —— **不交付的部分显式标注，不含混通过**。
> 已同步到父 `acceptance.md`。

**AC-006（出站 IdP）** → **移到 REQ-022**，本轮不承接。

## 全局断言约定（适用于本文件全部 12 条）

1. **零新增第三方依赖**：`Cargo.lock` 包数与基线相同（296）。`ldap3` / `hyper` / `jsonwebtoken` 均已在树里。
2. **新增错误码必须进 `TITLES`**：REQ-021 的 `i18n_titles.rs::titles_cover_every_code` 会机械校验 `TITLES ⊇ 各 crate 的 ALL_CODES`。漏一个 → 该测试红。
3. **错误 `title` 必须是 ASCII**（D113）。中文文案只在前端语言包里。
4. **时间一律 UTC 落库**，时区只在前端渲染（D114）。
5. **测试只用 `base_test` 库**，应用角色用 `base_app`，绝不写 `base_dev`。
6. **静态/敏感值扫描必须带阳性对照** —— 注入一次，断言扫描器能命中；否则「0 命中」与「扫描器坏了」不可区分（REQ-021 教训）。

---

### AC-S-020: 协议失败可诊断 —— 两类协议的失败给出**可区分**的错误码与排查提示

- **类型**: 功能 / 可观测性
- **前置条件**:
  - 租户 `tenant-a` 已配置 OIDC provider（`issuer` 指向一个可控的 mock IdP）与 LDAP provider（指向可控 mock 目录）
  - 已由上一轮 `GET /api/iam/sso/providers` 确认两个 provider 均已启用
- **测试步骤**:
  1. 把 OIDC 的 `issuer` 改成一个**返回 500** 的地址 → 发起 `GET /api/iam/sso/oidc/authorize`。
  2. 把 OIDC 的 `issuer` 改成一个**返回非法 JSON** 的地址 → 再发起一次。
  3. 用**伪造的 `code`** 直接打 `/api/iam/sso/oidc/callback?code=bogus&state=<有效state>`。
  4. 把 LDAP 的 `url` 改成一个**未监听的端口** → `POST /api/iam/sso/ldap/login`。
  5. 用**错误密码**对存在用户执行 LDAP 登录。
  6. 对**不存在的用户**执行 LDAP 登录（密码随便）。
- **预期结果**:
  1. 步骤 1 → `503`，`code = "SSO_OIDC_DISCOVERY_FAILED"`，`detail` 说明「无法获取发现文档」并带上 `issuer` 主机名（**不含**完整 URL 的 query）。
  2. 步骤 2 → `502`，`code = "SSO_OIDC_DISCOVERY_INVALID"`（与步骤 1 **可区分**）。
  3. 步骤 3 → `400`，`code = "SSO_OIDC_TOKEN_EXCHANGE_FAILED"`。
  4. 步骤 4 → `503`，`code = "SSO_LDAP_UNREACHABLE"`（与 OIDC 的失败**码不同**、`title` 不同）。
  5. 步骤 5 → `401`，`code = "SSO_LDAP_BIND_FAILED"`；响应体**不含**用户 DN、不含 LDAP 服务器返回的原文。
  6. 步骤 6 → **与步骤 5 完全相同**的 `401` / `SSO_LDAP_BIND_FAILED`（**不可区分** —— 防账号枚举）。
  7. 上述 6 个错误码**全部**出现在 `platform-iam` 的 `ALL_CODES` 与 `platform-core` 的 `TITLES` 中；`TITLES` 里对应 `title` 为 ASCII。
  8. `i18n_titles.rs` 的完备性测试仍通过（`TITLES` 覆盖新增码）。

---

### AC-S-021: 强制 SSO 生效 —— 租户开启后本地密码登录被拒，且**不能用于账号枚举**

- **类型**: 安全
- **前置条件**:
  - `tenant-a` 配好 OIDC 与 LDAP，`sso.enforce = true`
  - `tenant-b` 未配 SSO，`sso.enforce = false`
  - `tenant-a` 存在本地账号 `alice`（有正确密码）与不存在的账号 `nobody`
- **测试步骤**:
  1. 对 `tenant-a` 用 `alice` + **正确密码**打 `POST /api/iam/login`。
  2. 对 `tenant-a` 用 `alice` + **错误密码**打同一接口。
  3. 对 `tenant-a` 用 **不存在的账号** `nobody` 打同一接口。
  4. 对 `tenant-b` 用 `alice` + 正确密码打同一接口。
  5. 观察步骤 1~3 的**响应耗时**与**响应体**。
  6. 对 `tenant-a` 走 OIDC 登录（mock IdP），断言仍能成功。
  7. 把 `tenant-a` 的 `sso.enforce` 改回 `false`，再打 `POST /api/iam/login`。
- **预期结果**:
  1. 步骤 1 → **403**，`code = "SSO_REQUIRED"`，响应体带 `ssoProviders`（可用 provider 列表），**不含**任何账号存在性信息。
  2. 步骤 2、3 → 与步骤 1 **响应体完全一致**（同样的 403 / 同样的 code / 同样的字段集合）。
  3. 步骤 1 / 2 / 3 的耗时**同量级**（三者比值在 1.5× 内）—— 证明**没有**在拒绝前跑 Argon2id。
  4. 步骤 4 → `200`，本地登录正常（`sso.enforce` 是**租户级**，不串）。
  5. 步骤 6 → OIDC 登录成功，返回与本地登录**同构**的 `LoginResponse`。
  6. 步骤 7 → `200`，本地登录恢复。
  7. 断言实现层面：`sso.enforce` 的判定发生在**加载用户之前**（可用「数据库无该用户时仍返回 SSO_REQUIRED 而非 INVALID_CREDENTIALS」加一句代码位置断言）。

---

### AC-S-100: OIDC 授权码流程端到端 —— 从发起授权到拿到可用会话

- **类型**: 功能
- **前置条件**:
  - mock IdP 已就绪，暴露 `/.well-known/openid-configuration`、`/authorize`、`/token`、`/jwks`
  - `tenant-a` 配好 `sso.oidc.issuer` / `clientId` / `redirectUri` / `scopes`
  - 用户 `bob` 在 IdP 侧存在（`sub = "idp-bob-001"`，`email = bob@example.com`），且已在底座绑定到本地 user
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`（带 `x-tenant-id: tenant-a`），不自动跟随跳转。
  2. 解析 302 的 `Location`，断言其 query 参数。
  3. 模拟 IdP 授权完成，回调 `GET /api/iam/sso/oidc/callback?code=<mock code>&state=<步骤1的 state>`。
  4. 用响应里的 `accessToken` 打 `GET /api/rbac/me/permissions`。
  5. 用响应里的 `refreshToken` 打 `POST /api/iam/refresh`。
  6. 打 `GET /api/iam/sessions`。
- **预期结果**:
  1. 步骤 1 → `302`，`Location` 指向发现文档里的 `authorization_endpoint`。
  2. 步骤 2 → query **必含** `response_type=code`、`client_id`、`redirect_uri`（与配置逐字相同）、`scope`（含 `openid`）、`state`（非空且高熵）、`nonce`（非空）、`code_challenge` + `code_challenge_method=S256`。
  3. 步骤 3 → `200`，响应体**与本地登录的 `LoginResponse` 字段集合完全一致**（`user` / `accessToken` / `refreshToken` / `permissions` / …）。
  4. 步骤 4 → `200`，返回的是 `bob` 的真实权限点（与本地登录同一套展开逻辑，非空）。
  5. 步骤 5 → `200`，返回新 access token（refresh 链路可用）。
  6. 步骤 6 → 列表含本次 SSO 会话，且**能看出它来自 SSO**（见 AC-S-107 的字段要求）。

---

### AC-S-101: `state` / `nonce` / PKCE 防 CSRF 与重放

- **类型**: 安全
- **前置条件**: 同 AC-S-100
- **测试步骤**:
  1. 发起一次授权，拿到 `state`；回调一次（成功）；**再用同一个 `state`** 回调第二次。
  2. 发起授权后**等过 `state` 的 TTL**（测试环境可配，如 5 秒）再回调。
  3. 用 `tenant-a` 发起的 `state`，带 `x-tenant-id: tenant-b` 回调。
  4. 用 OIDC provider 的 `state` 去打 **LDAP** 的登录接口。
  5. 回调时把 `nonce` 对应的 claim 改成不匹配的值（mock IdP 侧改 `id_token.nonce`）。
  6. 回调时把 `code_verifier` 换成随机的另一个值。
  7. 完成后查询 `oidc_state` 表。
- **预期结果**:
  1. 步骤 1 第二次 → `400`，`code = "SSO_OIDC_STATE_INVALID"`（**一次性**）。
  2. 步骤 2 → `400`，`code = "SSO_OIDC_STATE_EXPIRED"`（与「无效」**可区分**）。
  3. 步骤 3 → `400`，`SSO_OIDC_STATE_INVALID`（`state` 绑定租户）。
  4. 步骤 4 → `400`，`SSO_OIDC_STATE_INVALID`（`state` 绑定 provider）。
  5. 步骤 5 → `401`，`code = "SSO_OIDC_NONCE_MISMATCH"`。
  6. 步骤 6 → `400`，`code = "SSO_OIDC_PKCE_MISMATCH"`。
  7. 步骤 7 → `oidc_state` 表中已用的记录**标记为已消费**（`consumed_at` 非空）或**已删除**，
     且**不存在**「同一 `state` 两条有效记录」；过期记录有清理路径（不无限增长）。

---

### AC-S-102: `id_token` 验签 —— 拒绝 `alg: none` 与算法混淆

- **类型**: 安全
- **前置条件**: mock IdP 可返回**自定义构造**的 `id_token`（测试专用开关，仅 mock 侧）
- **测试步骤**:
  1. 正常签名（`RS256`）的 `id_token`，但把 `iss` 改成别的值 → 回调。
  2. 正常签名但 `aud` 改成别的 client_id → 回调。
  3. 正常签名但 `exp` 设为过去 → 回调。
  4. `alg = "none"`、去掉签名的 `id_token` → 回调。
  5. 用 **`HS256`** 签名、密钥取 IdP **公钥内容**的 `id_token` → 回调。
  6. 用**未在配置白名单里**的算法（如 `RS512`，白名单只开 `RS256`/`ES256`）→ 回调。
  7. 签名正确但 `kid` 是一个**新的、未知的** `kid`（mock IdP 轮换密钥）→ 回调。
  8. 把 `payload` 里 `email` 改掉但保留原签名 → 回调。
- **预期结果**:
  1. 步骤 1 → `401`，`SSO_OIDC_TOKEN_INVALID`（`iss` 不匹配）。
  2. 步骤 2 → `401`，`SSO_OIDC_TOKEN_INVALID`（`aud` 不匹配）。
  3. 步骤 3 → `401`，`SSO_OIDC_TOKEN_EXPIRED`（与「无效」可区分）。
  4. 步骤 4 → `401`，`SSO_OIDC_ALG_NOT_ALLOWED`。
  5. 步骤 5 → `401`，`SSO_OIDC_ALG_NOT_ALLOWED`（**算法混淆必须被拒**）。
  6. 步骤 6 → `401`，`SSO_OIDC_ALG_NOT_ALLOWED`。
  7. 步骤 7 → **登录成功**（未知 `kid` 触发 JWKS 刷新，IdP 轮换不中断登录）。
  8. 步骤 8 → `401`，`SSO_OIDC_TOKEN_INVALID`（签名不匹配）。
  9. 全部 5 类失败均已在 `TITLES` 登记，`title` 为 ASCII。

---

### AC-S-103: 账号绑定与 JIT 建号 —— 不自动建号、不绕过禁用

- **类型**: 功能 / 安全
- **前置条件**:
  - `tenant-a` 的 `sso.jitProvisioning` 先设为 `false`
  - IdP 侧有 `sub = "idp-carol-001"` / `carol@example.com`，底座**无**绑定、**无**该账号
  - 另有 `sub = "idp-dave-001"` 绑定到本地 user `dave`，且 `dave` 已被禁用
- **测试步骤**:
  1. JIT 关闭时，`carol` 走 OIDC 登录。
  2. 打开 `sso.jitProvisioning = true`，`carol` 再走一次。
  3. 检查 `carol` 的 user 记录与 `external_identity` 绑定。
  4. `carol` 再登录一次（此时已绑定）。
  5. `dave` 走 OIDC 登录。
  6. 用**同一个** `sub` 在**同一租户**并发发起 3 次登录。
  7. 用**同一个** `sub` 在 `tenant-b`（同一 IdP）登录，`tenant-b` 未绑定该 sub。
- **预期结果**:
  1. 步骤 1 → `403`，`code = "SSO_ACCOUNT_NOT_LINKED"`；**断言数据库里仍未创建** `carol` 的 user 与绑定。
  2. 步骤 2 → `200` 登录成功。
  3. 步骤 3 → 新建了 user（`email = carol@example.com`）与一条 `external_identity`
     （`provider = "oidc"`、`external_id = "idp-carol-001"`、`user_id` 指向新 user），
     且 `(tenant_id, provider, external_id)` 唯一约束成立。
  4. 步骤 4 → `200`，**复用**步骤 2 的 user（user 表里 `carol` 只有 1 条）。
  5. 步骤 5 → `403`，`code = "ACCOUNT_DISABLED"`（**SSO 不绕过禁用**）。
  6. 步骤 6 → 只产生 **1 个** user 与 **1 条**绑定（并发幂等；不得因竞态建出多个）。
  7. 步骤 7 → `tenant-b` 未开启 JIT 时 `403 SSO_ACCOUNT_NOT_LINKED`；开启时建的是
     `tenant-b` 下的**独立** user（`external_identity` 的租户维度生效，不串租户）。

---

### AC-S-104: LDAP bind 认证 —— 成功/失败/不可达/空密码

- **类型**: 功能 / 安全
- **前置条件**:
  - mock LDAP 目录已就绪（可用 `ldap3` 的 in-memory 替身或真实 OpenLDAP 容器）
  - `tenant-a` 配好 `sso.ldap.url` / `baseDn` / `userSearchFilter` / `mailAttribute`
  - 目录里有 `uid=erin,ou=people,dc=example,dc=com`，密码 `Er1n-pass`
- **测试步骤**:
  1. `POST /api/iam/sso/ldap/login`，`username=erin`、`password=Er1n-pass`。
  2. 同样用户名、`password=wrong`。
  3. 不存在的用户名 `ghost` + 任意密码。
  4. **`password=""`（空串）**，用户名分别用 `erin` 与 `ghost`。
  5. **`password="   "`（纯空格）**。
  6. 停掉 mock LDAP，再打一次。
  7. 连续 10 次错误密码，再打第 11 次。
  8. 观察 mock LDAP 侧收到的 bind 请求记录。
- **预期结果**:
  1. 步骤 1 → `200`，返回与本地登录同构的 `LoginResponse`。
  2. 步骤 2 → `401`，`SSO_LDAP_BIND_FAILED`；**不泄露** DN、不泄露目录错误原文。
  3. 步骤 3 → `401`，与步骤 2 **完全一致**（防枚举）。
  4. 步骤 4 → `401`，`code = "SSO_LDAP_EMPTY_PASSWORD"`（**独立码，便于运维识别攻击**）；
     **且 mock LDAP 侧 0 条 bind 请求** —— 证明是**发出前**拦截，不是靠目录服务返回失败。
  5. 步骤 5 → 同步骤 4（纯空白等同空）。
  6. 步骤 6 → `503`，`SSO_LDAP_UNREACHABLE`。
  7. 步骤 7 → `429`，`code = "LOGIN_RATE_LIMITED"`（复用 REQ-007 已交付的限流表，
     不新造一套；限流维度含 IP）。
  8. 步骤 8 → 步骤 1~3 的 bind 请求**只带用户 DN 与密码**，不带任何额外敏感属性；
     **密码不出现在任何日志**里（对日志做扫描 + 阳性对照）。

---

### AC-S-105: LDAP 与本地账号共存 —— 密码不落库、映射不串

- **类型**: 功能
- **前置条件**:
  - `tenant-a` 的 `sso.enforce = false`（允许并存）
  - 本地账号 `frank` 有本地密码；LDAP 目录里**也**有 `mail = frank@example.com` 的条目
  - `sso.ldap.mailAttribute = "mail"`
- **测试步骤**:
  1. `frank` 用**本地密码**打 `POST /api/iam/login`。
  2. `frank` 用**LDAP 密码**打 `POST /api/iam/sso/ldap/login`。
  3. 检查两条路径落到的是否同一个 user。
  4. 检查 `platform_iam_user` 里 `frank` 的密码哈希。
  5. 在 LDAP 侧改 `frank` 的密码，再用旧 LDAP 密码登录、用新 LDAP 密码登录。
  6. 在 LDAP 侧把 `frank` 的 `mail` 改成一个**已被另一个本地账号占用**的邮箱，再登录。
- **预期结果**:
  1. 步骤 1 → `200`。
  2. 步骤 2 → `200`。
  3. 步骤 3 → **同一个** `user_id`（两条路径都解析到同一 user，不产生影子账号）。
  4. 步骤 4 → 该 user 的密码哈希**仍是本地密码的 Argon2id PHC 串**，
     **未**被 LDAP 密码覆盖，也**未**新增「LDAP 密码摘要」类字段。
  5. 步骤 5 → 旧 LDAP 密码 `401`，新 LDAP 密码 `200`（**密码只在目录服务里**，底座不缓存）。
  6. 步骤 6 → `409`，`code = "SSO_LDAP_IDENTITY_CONFLICT"`（**不**静默绑定到别人账号）。

---

### AC-S-106: SSO 配置与密钥保护 —— `client_secret` 三处扫描 0 命中

- **类型**: 安全
- **前置条件**:
  - `BASE_SSO_OIDC_CLIENT_SECRET` 注入一个**独一无二的可识别串**，
    例如 `SECRET-CANARY-7f3a9b1e-UNIQUE`
  - `tenant-a` 的 OIDC 配置完整
- **测试步骤**:
  1. `GET /api/config/entries`（平台管理员令牌）→ 取全量配置。
  2. `GET /api/config/entries?scope=tenant`（租户管理员令牌）。
  3. 触发一次 OIDC 失败（AC-S-102 的 `alg=none` 用例），把**完整错误响应体**取出。
  4. 以 `BASE_LOG_LEVEL=trace` 起服务，重放一次成功登录 + 一次失败登录，捕获 **stdout 全量日志**。
  5. `GET /api/iam/sso/providers` 与 `GET /api/iam/sessions` 的完整响应体。
  6. 对 `platform_config` / `platform_tenant_config` / `platform_space_config` 三张表
     做 `SELECT` 全表文本扫描（管理员连接）。
  7. **阳性对照**：往一个**测试用**配置键里写入同一串 canary，重跑步骤 1 与步骤 3，断言扫描器**能命中**。
- **预期结果**:
  1. 步骤 1~6 → canary 串在**全部位置** 0 命中：
     配置导出、错误体、日志、providers、sessions、三张配置表。
  2. 步骤 7 → 扫描器**命中**该 canary（证明步骤 1~6 的「0 命中」是真的，不是扫描器坏了）。
  3. `sso.oidc.clientId` **可以**出现（它不是密钥）；`client_secret` 的任何前缀/后缀片段也 0 命中。
  4. 非敏感 SSO 配置项**全部**在 `platform-config/manifest.json` 登记，
     且 `GET /api/config/entries` 的 `source` 字段能正确反映三层覆盖（平台 / 租户）。

---

### AC-S-107: SSO 会话与本地会话**同构**

- **类型**: 功能 / 安全
- **前置条件**: `tenant-a` 同时有本地账号 `grace` 与 SSO 账号 `bob`（绑定完成）
- **测试步骤**:
  1. `grace` 本地登录；`bob` OIDC 登录。
  2. 对两者分别打 `GET /api/iam/sessions`。
  3. 对 `bob` 的会话打 `DELETE /api/iam/sessions/{id}`，再用他的 `refreshToken` 刷新。
  4. 对 `bob` 打 `POST /api/iam/logout-all`。
  5. 检查 `platform_iam_session` 表里两条会话的字段差异。
- **预期结果**:
  1. 步骤 2 → 两条会话**都在**同一列表里，字段集合一致。
  2. 能区分来源：会话记录里能看出 `bob` 这条来自 **SSO**（具体字段由设计定，
     但**必须存在**且**不得**靠 `device_label` 之类的自由文本猜）。
  3. 步骤 3 → `DELETE` 返回成功；随后 refresh **失败**（`TOKEN_REVOKED`）——
     SSO 会话与本地会话**同样可被踢下线**，走同一套 `family_id` 作废逻辑。
  4. 步骤 4 → `logout-all` 作废该用户**全部** family，包含 SSO 会话。
  5. 步骤 5 → 两条会话的 `family_id` / `revoked_at` / `created_at` 等结构字段**同构**；
     无「SSO 专用会话表」（不得两套）。
  6. SSO 登录**不产生**第二套 token 体系：access token 的 claim 结构、签名算法、
     `kid` 与本地登录**完全一致**（同一 `jwks`）。

---

### AC-S-108: SSO 审计事件 —— **声明 + 发出**，但本轮**不落库**

- **类型**: 功能 / 合规
- **前置条件**: `platform-iam` 实现中已注册审计事件声明；测试用 `RecordingAuditRecorder`
- **测试步骤**:
  1. 检查 `platform-iam` 是否声明了 `AuditEventSpec` 清单。
  2. 用 `RecordingAuditRecorder` 注入，跑一次 OIDC 成功登录。
  3. 跑一次 OIDC 失败登录（`alg=none`）。
  4. 跑一次 LDAP 成功 + 一次 LDAP 失败。
  5. 对录制到的**全部事件 payload** 做敏感值扫描（同 AC-S-106 的 canary）。
  6. 检查默认（未注入 recorder 时）行为。
- **预期结果**:
  1. 步骤 1 → 至少声明：`iam.sso.login.succeeded`、`iam.sso.login.failed`、
     `iam.sso.link.created`（JIT 建绑定时）三个事件，每个带 `code` / `action` / `objectType`。
  2. 步骤 2 → 发出 `iam.sso.login.succeeded`，payload 含 `provider` / 外部 `sub` / `userId` / `ip` / `userAgent`。
  3. 步骤 3 → 发出 `iam.sso.login.failed`，payload 含 `provider` / 错误码 / `ip`；
     **不含** token、**不含** `id_token` 原文。
  4. 步骤 4 → LDAP 的成败同样各发一条，`provider = "ldap"` 可区分。
  5. 步骤 5 → canary 0 命中；密码 / token / claim 全集均不出现。
  6. 步骤 6 → 默认 `NoopAuditRecorder`，**登录仍正常成功**（审计缺失不得阻断登录，
     但**必须**有 `tracing::warn` 级别之上的可观测提示，避免「以为在审计其实没有」）。
  7. **明确不验**：事件是否落入审计表 —— 那是 REQ-014 的交付物，本轮**没有 sink**。
     此条**不得**写成「审计记录已可查询」。

---

### AC-S-109: 多 IdP 并存与租户隔离

- **类型**: 功能 / 安全
- **前置条件**:
  - `tenant-a` 同时配好 OIDC 与 LDAP（都对 mock 服务）
  - `tenant-b` 只配 OIDC，且指向**另一个** mock IdP（issuer 不同）
  - `tenant-c` 未配任何 SSO
- **测试步骤**:
  1. 对三个租户分别打 `GET /api/iam/sso/providers`。
  2. `tenant-a` 走 LDAP 登录；`tenant-a` 走 OIDC 登录。
  3. 用 `tenant-b` 的 OIDC `state` 到 `tenant-a` 回调。
  4. `tenant-c` 打 `GET /api/iam/sso/oidc/authorize`。
  5. `tenant-c` 打 `POST /api/iam/sso/ldap/login`。
  6. 把 `tenant-a` 的 OIDC 关掉（`sso.oidc.enabled=false`），再打 `authorize`。
  7. 检查 `tenant-a` 与 `tenant-b` 的配置读取。
- **预期结果**:
  1. 步骤 1 → `tenant-a` 返回 **2 个** provider（oidc + ldap），`tenant-b` 返回 **1 个**（oidc），
     `tenant-c` 返回**空列表**（不是 404、不是 500）。
  2. 步骤 2 → 两条路径都能登录成功，且产生的是**不同**会话（各自独立 family）。
  3. 步骤 3 → `400`，`SSO_OIDC_STATE_INVALID`（`state` 绑租户，跨租户不可用）。
  4. 步骤 4 → `400`，`code = "SSO_PROVIDER_NOT_CONFIGURED"`。
  5. 步骤 5 → 与步骤 4 同样的 `SSO_PROVIDER_NOT_CONFIGURED`（不要因为协议不同给不同码，
     让前端能统一提示「该租户未配置企业登录」）。
  6. 步骤 6 → `400`，`SSO_PROVIDER_NOT_CONFIGURED`（**禁用即等于未配置**，
     不返回「已禁用」这种暴露内部状态的码）。
  7. 步骤 7 → `tenant-a` 的 issuer / LDAP url 与 `tenant-b` 的值**互不影响**；
     修改 `tenant-b` 的配置不会改变 `tenant-a` 的登录结果。

---

## AC 与安全约束的对应关系

| 安全约束（demand §安全约束） | 由哪条 AC 守 |
|---|---|
| §1 LDAP 空密码必须显式拒绝 | **AC-S-104** 步骤 4/5/8 |
| §2 强制 SSO 必须在密码校验之前拒绝 | **AC-S-021** 步骤 1~3、7 |
| §3 `client_secret` 不得出现在任何可读位置 | **AC-S-106**（含阳性对照）、AC-S-108 步骤 5 |
| §4 不得接受 `alg: none`，不得算法混淆 | **AC-S-102** 步骤 4/5/6 |
| §5 SSO 不得绕过账号禁用与租户隔离 | **AC-S-103** 步骤 5、**AC-S-109** 步骤 3 |
| §6 审计事件不含敏感值 | **AC-S-108** 步骤 5 |

## 覆盖度自检

| 问题 | 答案 |
|---|---|
| 每条 AC 的「主体」都存在吗？ | 已逐项核实：`ldap3::simple_bind` 在 0.11.5 存在；`external_identity` 表与 `upsert_external_identity` 已交付；`crypto::encrypt/decrypt` 已交付；`login_rate` 限流表已交付；`AuditRecorder` 接缝已交付（org 模块已在用）。**唯一不存在的是审计 sink** → 已在 AC-S-108 显式排除落库断言 |
| 有没有「物理不可达」的前置？ | 「LDAP 空密码」是可达的（空串可提交）；「alg=none」需要 mock IdP 支持构造（已在前置声明）；「等过 TTL」用测试可配值，不等真实 5 分钟 |
| 失败路径都有明确错误码吗？ | 每条 AC 的失败分支都点名了 `code`，共 **13 个新错误码** |
| 有没有依赖尚未交付的需求？ | 依赖 REQ-007 / 009 / 010，均已合并。**不依赖** REQ-014（审计 sink）与 REQ-022（出站） |
