# 验收标准：出站 IdP 与单点登出

- **需求编号**: REQ-022
- **父需求**: REQ-001
- **状态**: **rev 1**（2026-11-20 —— BA 规格阶段写实；QA 挑刺后逐条裁决修完）
- **对应验收标准**: **22 条**（AC-S-134 ~ AC-S-155）
- **决策依据**: [decisions.md](decisions.md) rev 1（D196~D220）
- **用例**: 待 QA 产出（`test-cases/index.md`，QA **必须先于 Dev**）
- **更新时间**: 2026-11-20 23:55

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## 承接的父 AC

父 AC：**AC-006**（底座作为 IdP 向第三方应用签发令牌）。

父 AC 原文的验收步骤是「走授权流程拿授权码 → 用 PKCE 换令牌 → 携带令牌调用应用接口」。
本需求把「调用应用接口」这一环**改写**为两种可判定形态（理由见 [decisions.md](decisions.md) D209）：

| 形态 | 判据落在哪条 AC |
|---|---|
| **内省校验**（第三方应用调平台内省端点） | AC-S-143 |
| **验签校验**（第三方应用用平台 JWKS 自行验签） | AC-S-145 |

> 「携带令牌调用**底座**业务接口」**不在本需求范围**（底座业务端点默认拒绝第三方令牌 → AC-S-151）。

---

## 全局约定（所有 AC 默认遵守）

| 项 | 约定 | 依据 |
|---|---|---|
| 应用连接角色 | **`base_app`**（`base_platform_app` 是 NOLOGIN 组角色，连不上） | D165 |
| 造数 / 篡改连接 | **`base`**（表 owner + 超管） | — |
| 库 | **`base_test`**（绝不写 `base_dev`） | dev-guide §9.8 |
| `app.tenant_id` | **事务局部**；凡涉及租户上下文的查询**必须在单个事务内** `set_config` → 查询 | D148 |
| 错误体 | **两套**，不可混用（见下表） | D213 / 契约 C14 |
| 时间格式 | UTC、RFC3339、**微秒 6 位** | D142 / D151 |
| 哈希复算 | 测试侧**独立实现**，直接使用列里存的**文本**，不得解析回 JSON 再序列化 | D141 |
| 静态扫描 | 必须带**阳性对照**（否则「0 命中」不可信） | dev-guide §9.2 |
| 协议端点 | OAuth **协议端点**按 RFC 说 RFC 的话；**平台管理端点**按契约 C6 说平台的话 | 契约 C14 |

### ⚠️ 两套错误标识（本需求最容易搞混的一处）

| 类别 | 形状 | 出现在 | 取值风格 |
|---|---|---|---|
| **协议错误** | `{"error": "invalid_grant", "error_description": "…"}` | `/api/oauth/token`、`/introspect`、`/revoke`、`/device_authorization` | **小写下划线**（RFC 6749 §5.2 / 8628 §3.5） |
| **平台错误** | RFC 9457 Problem Details（契约 C6） | `/api/oauth/authorize/*`、`/api/oauth/clients/*`、`/api/oauth/grants/*` | **大写下划线**（`OAUTH_*`） |

- 协议错误的**取值集合**是 RFC 6749 §4.1.2.1 + RFC 8628 §3.5 + RFC 7662 §2.3 的并集：
  `invalid_request` / `invalid_client` / `invalid_grant` / `unauthorized_client` /
  `unsupported_grant_type` / `unsupported_response_type` / `invalid_scope` /
  `access_denied` / `login_required` / `consent_required` /
  `authorization_pending` / `slow_down` / `expired_token` / `server_error`
- 平台错误码清单（**10 个**，全部要按 D213 的 8 处登记）：
  `OAUTH_CLIENT_NOT_FOUND` / `OAUTH_CLIENT_DISABLED` / `OAUTH_CLIENT_EXPIRED` /
  `OAUTH_CLIENT_SECRET_INVALID` / `OAUTH_INVALID_REQUEST` /
  `OAUTH_INVALID_REDIRECT_URI` / `OAUTH_INVALID_SCOPE` /
  `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` / `OAUTH_TICKET_INVALID` /
  `OAUTH_GRANT_NOT_FOUND`

---

## 本需求自有 AC

---

### AC-S-134: 授权服务器元数据可被发现

- **前置条件**:
  - 平台已配置 IAM（`BASE_JWT_*` 一整套齐全），服务已起；
  - 元数据端点 **`GET /.well-known/oauth-authorization-server`** 已注册。
- **测试步骤**:
  1. `GET /.well-known/oauth-authorization-server`；
  2. 读响应体的每个字段。
- **预期结果**:
  - HTTP **200**，`Content-Type: application/json`；
  - `issuer` **逐字等于** `BASE_JWT_ISSUER` 的值；
  - `authorization_endpoint` = `{issuer}/oauth/authorize`（**前端路由**，D203）；
  - `token_endpoint` = `{issuer}/api/oauth/token`；
  - `device_authorization_endpoint` = `{issuer}/api/oauth/device_authorization`；
  - `introspection_endpoint` = `{issuer}/api/oauth/introspect`；
  - `revocation_endpoint` = `{issuer}/api/oauth/revoke`；
  - `jwks_uri` = `{issuer}/.well-known/jwks.json`；
  - `grant_types_supported` **恰好包含** `authorization_code` / `client_credentials` /
    `urn:ietf:params:oauth:grant-type:device_code`
    （**不含** `refresh_token`，D217）；
  - `response_types_supported` **恰好** `["code"]`；
  - `code_challenge_methods_supported` **恰好** `["S256"]`（**不含** `plain`，D206）；
  - `token_endpoint_auth_methods_supported` **包含** `client_secret_basic` 与 `none`；
  - `introspection_endpoint_auth_methods_supported` **恰好** `["client_secret_basic"]`（D208）；
  - `revocation_endpoint_auth_methods_supported` **恰好** `["client_secret_basic"]`；
  - `access_token_expires_in` = 平台配置的 `oauth.accessTokenTtlSeconds`（D210）。
- **类型**: 功能

---

### AC-S-135: client 可注册、可改、可停用、可删，且**回调白名单**生效

- **前置条件**: 管理员账号持有 `oauth:client:write`；测试库 `base_test` 可用。
- **测试步骤**:
  1. `POST /api/oauth/clients`，body 含 `name` / `clientType` / `redirectUris` /
     `allowedGrantTypes` / `allowedScopes`；
  2. 读响应；
  3. `GET /api/oauth/clients` 列出；
  4. `PATCH /api/oauth/clients/{client_id}` 把 `status` 改成 `disabled`；
  5. 用该 client 走一次授权码交换；
  6. `DELETE /api/oauth/clients/{client_id}` 后再查。
- **预期结果**:
  - 步骤 2 返回 **201**（或 200），含 `clientId`（**非空、唯一**）与
    **`clientSecret`（仅此一次**，D200）；响应**不含** `clientSecretHash`；
  - 步骤 3 的列表里**没有** `clientSecret` 字段（D200）；
  - 步骤 5 返回 **401** + `invalid_client`（`client` 已 disabled → D208 的认证失败口径）；
  - 步骤 6 之后 `GET /api/oauth/clients/{client_id}` 返回 **404** + `OAUTH_CLIENT_NOT_FOUND`。
- **类型**: 功能

---

### AC-S-136: `client_secret` 只以 **SHA-256 摘要**落库，任何读接口不回显

- **前置条件**: 已注册一个 `confidential` client，拿到过一次明文 secret。
- **测试步骤**:
  1. 以 `base` 直连查 `platform_oauth_client` 的 `client_secret_hash` 列；
  2. 测试侧**独立实现** SHA-256 + base64url（**不调用生产代码**），对明文 secret 求摘要；
  3. 调 `GET /api/oauth/clients` 与 `GET /api/oauth/clients/{client_id}`，读全量响应体；
  4. 全仓扫描 `client_secret` 的**明文**是否出现在任何响应构造点。
- **预期结果**:
  - 步骤 1 的值 **逐字节等于**步骤 2 的计算结果；
  - 步骤 3 的两个响应体**都不含** `clientSecret`（**键都不存在**，不是值为 null）；
  - 步骤 4：**0 命中**（且扫描必须带**阳性对照** —— 在临时副本里塞一个明文，扫描器必须命中）。
- **类型**: 安全

---

### AC-S-137: 授权码 + PKCE 能换到可用令牌（端到端）

- **前置条件**:
  - `tenant-a` 下有用户 `e2e-user-oauth@example.com`（持有权限点 `rbac:role:read`）；
  - 已注册 `confidential` client，回调白名单含 `https://client.example/cb`，
    `allowedScopes` 含 `rbac:role:read`，`allowedGrantTypes` 含 `authorization_code`；
  - 平台前端已起（`apps/shell/dist`）。
- **测试步骤**:
  1. 浏览器访问 `{平台}/oauth/authorize?response_type=code&client_id=…&
     redirect_uri=https://client.example/cb&scope=rbac%3Arole%3Aread&state=xyz&
     code_challenge=<S256(verifier)>&code_challenge_method=S256`；
  2. 登录（`tenant-a` / 账号 / 密码）；
  3. 在同意页点「允许」；
  4. 读浏览器最终地址栏的 `code` 与 `state`；
  5. `POST /api/oauth/token`，form 编码 `grant_type=authorization_code` / `code` /
     `client_id` / `client_secret` / `redirect_uri` / `code_verifier`；
  6. 读响应体；
  7. 用 `access_token` 调 `POST /api/oauth/introspect`（client 认证）。
- **预期结果**:
  - 步骤 4 的地址栏 **以 `code=` 开头**，`state` **逐字等于** `xyz`；
  - 步骤 6 返回 **200**，含 `access_token` / `token_type`（值 `"Bearer"`）/
    `expires_in` / `scope`（**等于** `rbac:role:read`）；
  - 步骤 7 返回 `active = true`，且 `scope` **包含** `rbac:role:read`。
- **类型**: 功能

---

### AC-S-138: PKCE **强制 S256**，缺挑战或方法不支持一律拒

- **前置条件**: 同 AC-S-137 的 client（`allowedGrantTypes` 含 `authorization_code`）。
- **测试步骤**（**三条独立路径**）:
  1. 授权请求**不带** `code_challenge` → 走到令牌交换；
  2. 授权请求带 `code_challenge` 但 `code_challenge_method=plain`；
  3. 授权请求带 `code_challenge_method=S256`（challenge 取 `S256("wrong-verifier")`），
     交换时传 `code_verifier="right-verifier"`。
- **预期结果**:
  - 路径 1、2：**签发 code 这一步**就失败 —— 平台错误 `OAUTH_INVALID_REQUEST`
    （缺挑战）或 `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD`（plain），HTTP **400**；
    **不得**签出任何 code；
  - 路径 3：交换返回 **400** + `{"error": "invalid_grant"}`，
    且 `error_description` **不含**「哪一步错了」的细节以外的信息。
- **类型**: 安全

---

### AC-S-139: 授权码**一次性**、只存摘要、**60 秒**过期

- **前置条件**: 已通过授权流程拿到一个**未兑换**的授权码 `code`。
- **测试步骤**:
  1. 以 `base` 直连查授权码表里对应行（**用摘要匹配**，不按明文）；
  2. 用 `code` + 正确 `code_verifier` 交换 → 拿令牌；
  3. **立刻**用**同一个** `code` + 同一个 `code_verifier` 再交换一次；
  4. 查授权码表的行数。
- **预期结果**:
  - 步骤 1：库里的值 **等于** 测试侧独立计算的 `SHA-256(code)` 摘要
    （**不是**明文；**库里不得出现明文 code**）；
  - 步骤 2：**200**，拿到令牌；
  - 步骤 3：**400** + `{"error": "invalid_grant"}`；
  - 步骤 4：授权码**已被删除**（行数 = 0 行，**不是**置一个 `consumed_at` 标记）。
- **补充判据（TTL）**: 造一个 `expires_at` 在 **90 秒前**的授权码 → 交换返回
  `{"error": "invalid_grant"}`；造一个 `expires_at` 在 **30 秒后**的 → 交换**成功**。
- **类型**: 安全

---

### AC-S-140: scope 是**三重交集**；通配不隐式生效；不得越过用户本人

- **前置条件**:
  - client 的 `allowedScopes` = `["rbac:role:read", "rbac:role:write"]`；
  - 测试用户**只**持有 `rbac:role:read`（**没有** `rbac:role:write`）。
- **测试步骤**（**四条独立路径**）:
  1. 请求 `scope=rbac:role:read` → 交换 → 读令牌响应的 `scope`；
  2. 请求 `scope=rbac:role:read rbac:role:write`（用户**没有** write）→ 交换；
  3. 请求 `scope=sandbox:demo:read`（**不在** client 的 `allowedScopes` 里）→ 交换；
  4. 请求 `scope=rbac:role:*`（**不是**注册过的权限点）→ 交换。
- **预期结果**:
  - 路径 1：`scope` **等于** `rbac:role:read`；
  - 路径 2、3、4：**400**，错误为 `invalid_scope`（协议错误，小写）；
  - **额外断言**：路径 2 的令牌**不含** `rbac:role:write`
    （即使用户后来被授予 write，也不得回溯扩大已签发的 scope）；
  - **额外断言**：`rbac:role:*` 之所以被拒，是因为它**不是** `platform_permission` 表里的
    权限点（测试侧查表确认 `rbac:role:*` **不存在**）—— 不是因为「通配被禁用」，
    二者必须能区分开（用 `rbac:role:read` 走路径 1 证明普通权限点可用）。
- **类型**: 安全

---

### AC-S-141: Client Credentials grant 可换到**无用户**的令牌

- **前置条件**: 已注册 `confidential` client，`allowedGrantTypes` 含 `client_credentials`，
  `allowedScopes` 含 `sandbox:demo:read`。
- **测试步骤**:
  1. `POST /api/oauth/token`，form `grant_type=client_credentials` / `scope=sandbox:demo:read` /
     `client_id` / `client_secret`；
  2. 读响应体；
  3. 解析 `access_token` 的 payload（**只解 payload，不验签**）；
  4. `POST /api/oauth/introspect` 该令牌。
- **预期结果**:
  - 步骤 2：**200**，含 `access_token` / `token_type` / `expires_in` / `scope`；
  - 步骤 3：payload 里**没有** `sub` 键（**不是**空串、不是 null —— 键不存在）；
    `client_id` **等于**该 client；`grant_type` **等于** `client_credentials`；
  - 步骤 4：`active = true`，且 `username` 键**不存在**（RFC 7662：无用户主体）。
- **类型**: 功能

---

### AC-S-142: Device Code 全流程（申请 → 用户输入短码 → 授权 → 轮询换令牌）

- **前置条件**: 已注册 `confidential` client，`allowedGrantTypes` 含
  `urn:ietf:params:oauth:grant-type:device_code`；用户 `tenant-a` 已登录浏览器。
- **测试步骤**:
  1. `POST /api/oauth/device_authorization`，form `client_id` / `client_secret` /
     `scope=sandbox:demo:read` → 读 `device_code` / `user_code` / `verification_uri` /
     `verification_uri_complete` / `expires_in` / `interval`；
  2. **立刻**用 `device_code` 轮询 `POST /api/oauth/token`
     （`grant_type=urn:ietf:params:oauth:grant-type:device_code`）；
  3. 浏览器访问 `verification_uri`，输入 `user_code`，点「允许」；
  4. 再用**同一个** `device_code` 轮询；
  5. 查设备码表。
- **预期结果**:
  - 步骤 1：**200**，五个字段齐全；`expires_in` = 平台配置的 `oauth.deviceCodeTtlSeconds`；
    `interval` = 平台配置的 `oauth.deviceCodePollIntervalSeconds`；
  - 步骤 2：**400** + `{"error": "authorization_pending"}`；
  - 步骤 4：**200**，含 `access_token`（`scope` = `sandbox:demo:read`）；
  - 步骤 5：设备码行**已被删除**（行数 = 0）；
  - **补充判据**：`user_code` 与 `device_code` 在库里**都只存摘要**；
    用 `user_code` **直接换令牌**（当成 `device_code` 用）→ **失败**。
- **类型**: 功能

---

### AC-S-143: 令牌内省可用，且**只能内省自己的令牌**

- **前置条件**: `client-A`（confidential）与 `client-B`（confidential）各持一枚有效令牌。
- **测试步骤**:
  1. `client-A` 内省 `client-A` 的令牌；
  2. `client-A` 内省 `client-B` 的令牌；
  3. `client-A` 内省一枚**已过期**的令牌；
  4. `client-A` 内省一枚**已撤销**的令牌；
  5. `public` client 内省任意令牌；
  6. 不带任何认证内省。
- **预期结果**:
  - 步骤 1：**200**，`active = true`，`client_id` / `scope` / `exp` / `iat` / `sub` /
    `token_type` / `iss` / `aud` **齐全**；
  - 步骤 2：**200**，`active = false`，且**除 `active` 外没有任何其它字段**
    （**不泄露**该令牌存在、更不泄露其 `client_id`）；
  - 步骤 3、4：**200**，`active = false`，同样**只有 `active` 字段**；
  - 步骤 5、6：**401**（D208：public client 无权内省）。
- **类型**: 安全

---

### AC-S-144: 令牌可撤销，撤销后**内省立即失效**

- **前置条件**: `client-A` 持一枚有效令牌。
- **测试步骤**:
  1. 内省该令牌 → `active = true`；
  2. `POST /api/oauth/revoke`（client 认证，form `token=…`）；
  3. 再内省该令牌；
  4. 用 `client-B` 的认证去撤销 `client-A` 的令牌；
  5. 撤销一枚**根本不存在**的令牌。
- **预期结果**:
  - 步骤 2：**200**（RFC 7009 §2.2：撤销成功也是 200）；
  - 步骤 3：`active = false`（**立即**，D210）；
  - 步骤 4：**200**（不报错、不泄露 —— 但该令牌**不得**被真的撤销掉）；
    → 步骤 4 之后用 `client-A` 内省该令牌仍是 `active = true`；
  - 步骤 5：**200**（无效令牌也返回 200）。
- **类型**: 安全

---

### AC-S-145: 令牌是 **ES256 签的 JWT**，第三方可凭 JWKS 自行验签

- **前置条件**: `client-A` 持一枚有效令牌；平台 JWKS 可取。
- **测试步骤**:
  1. 取 `GET /.well-known/jwks.json`；
  2. 解析 `access_token` 的 header 与 payload（**测试侧独立解码**）；
  3. 测试侧**独立实现** ES256 验签（用 JWKS 里的公钥），**不调用生产代码**；
  4. 用**过期**令牌重复步骤 3。
- **预期结果**:
  - 步骤 2：header 的 `alg` **等于** `ES256`，`kid` **在** JWKS 的 `keys[].kid` 之中；
    payload 的 `iss` **等于** `BASE_JWT_ISSUER`，`aud` **等于** `client-A` 的 `client_id`，
    `sub` / `tid` / `client_id` / `scope` / `grant_type` / `iat` / `nbf` / `exp` / `jti` **齐全**；
  - 步骤 3：验签**通过**；
  - 步骤 4：验签**通过**（签名有效）但 `exp` **已过期** → 结论是「已过期」，
    **不得**把「签名无效」与「已过期」混为一谈（两者必须是不同的断言）。
- **类型**: 安全

---

### AC-S-146: 同意页可用、同意记录落库，且**用户所见 = 实际授予**

- **前置条件**: 同 AC-S-137；`tenant-a` 下该用户**尚无**该 client 的授权记录。
- **测试步骤**:
  1. 走完授权流程（AC-S-137 步骤 1~3），在同意页**允许**；
  2. 以 `base` 直连查 `platform_oauth_grant`；
  3. **篡改**同意票据（把 ticket 里的 `scope` 改成更大的集合后重新编码），
     调 `POST /api/oauth/authorize/confirm`；
  4. 调 `POST /api/oauth/authorize/deny`（用步骤 1 的合法 ticket）。
- **预期结果**:
  - 步骤 1：同意页**展示**了 client 的 `name` 与请求的**全部**权限点码
    （页面文本里逐字出现 `rbac:role:read`）；
  - 步骤 2：恰好 **1 行**，`client_id` / `user_id` / `scopes` / `status = active` /
    `consent_at` **非空**；
  - 步骤 3：**400** + `OAUTH_TICKET_INVALID`（**不得**签发任何 code）；
  - 步骤 4：返回的 `redirectTo` 的 query 里 `error` **等于** `access_denied`，
    且 `state` **逐字等于**原请求的 `state`；
    → 步骤 4 之后查 grant 表：**仍然只有步骤 2 那 1 行**，且 `status` 仍是 `active`
    （拒绝**不改**已有授权）。
- **类型**: 安全

---

### AC-S-147: 用户能查自己的授权并撤销，撤销后令牌**立即失效**

- **前置条件**: `tenant-a` 下用户已对 `client-A` 授权（AC-S-146 步骤 1 已落库），
  `client-A` 持有一枚由该 grant 签发的有效令牌。
- **测试步骤**:
  1. `GET /api/oauth/grants`（用户会话）；
  2. `POST /api/oauth/grants/revoke`，body `{ clientId: "client-A" }`；
  3. 再查授权记录；
  4. 内省步骤 1 之前那枚令牌；
  5. 用**另一个租户**的用户调 `GET /api/oauth/grants`。
- **预期结果**:
  - 步骤 1：返回**恰好 1 条**，含 `clientId` / `scopes` / `grantedAt` / `status`；
    **不含**任何其它用户或其它 client 的记录；
  - 步骤 2：**200**；
  - 步骤 3：该条 `status` **等于** `revoked`，`revokedAt` **非空**；
  - 步骤 4：`active = false`（**立即**，D210）；
  - 步骤 5：返回**空数组**（**不泄露**其它租户的授权存在性）。
- **类型**: 安全

---

### AC-S-148: 已有授权时**免登**（静默续发，不重复同意页）

- **前置条件**: `tenant-a` 下用户已对 `client-A` 授权，且该 grant 覆盖请求的 scope。
- **测试步骤**:
  1. 浏览器访问 `{平台}/oauth/authorize?…&prompt=none&…`（参数同 AC-S-137）；
  2. 观察**是否出现同意页**；
  3. 读最终地址栏的 `code`；
  4. 再访问一次，**但**把 `scope` 改成**更大**的集合（超出已有 grant）；
  5. 访问 `{平台}/oauth/authorize?…` **不带** `prompt=none`。
- **预期结果**:
  - 步骤 2：**没有**同意页（页面直接跳转）；步骤 3：地址栏含 `code`；
  - 步骤 4：返回 `consent_required`（**不**签发 code）→ 前端显示同意页；
  - 步骤 5：**出现**同意页（`prompt` 缺省 = 允许交互）。
- **类型**: 功能

---

### AC-S-149: Back-Channel Logout 能向 client 投递 `logout_token`

- **前置条件**:
  - `client-A` 的 `backchannel_logout_uri` 指向一个**进程内**的 mock 接收端
    （HTTP，本地端口；**不得**依赖外部网络）；
  - `tenant-a` 下用户已对 `client-A` 授权。
- **测试步骤**:
  1. 触发全局登出（AC-S-150 的步骤 1）；
  2. 读 mock 接收端收到的**请求体**；
  3. **测试侧独立**解析 `logout_token` 的 payload；
  4. 让 mock 接收端**超时**（不响应）后，再触发一次登出；
  5. 再让 mock 返回 **500**。
- **预期结果**:
  - 步骤 2：收到的是 **form 编码**（`Content-Type: application/x-www-form-urlencoded`），
    含 `logout_token=…`；
  - 步骤 3：payload 的 `iss` **等于**平台 issuer、`aud` **等于** `client-A` 的 `client_id`、
    `sub` / `sid` / `jti` / `iat` **齐全**、
    `events` **含**键 `http://schemas.openid.net/event/backchannel-logout`；
    payload **不含** `nonce`（D211）；
  - 步骤 4、5：**登出本身仍然成功**（HTTP 200，授权记录已撤销）——
    **通知失败绝不能让登出失败**（D211）；
  - **补充判据**：投递失败时**有日志**（含 `clientId` 与失败原因）。
- **类型**: 功能

---

### AC-S-150: Front-Channel Logout 与按应用登出可用

- **前置条件**: `tenant-a` 下用户已对 `client-A`（配了 `frontchannel_logout_uri`）
  与 `client-B`（**未**配）授权。
- **测试步骤**:
  1. 触发全局登出，读响应体；
  2. 读「我的授权」页数据；
  3. 重新授权后，触发**按应用登出**（`POST /api/oauth/grants/revoke`，body `{ clientId }`）；
  4. 查 `client-B` 的授权记录。
- **预期结果**:
  - 步骤 1：响应体里 `frontChannel` 数组**恰好 1 条**（**只有** client-A），
    含 `clientId` 与 `logoutUrl`（该 URL **逐字等于** client 表里登记的值）；
    `client-B` **不在**其中（它没配 front-channel URI）；
  - 步骤 2：两个 client 的授权**都**被撤销（`status = revoked`）；
  - 步骤 3：只撤销 `client-A`；步骤 4：`client-B` 的记录 `status` **仍是** `active`。
- **类型**: 功能

---

### AC-S-151: 底座业务端点**默认拒绝**第三方 OAuth 令牌

- **前置条件**: `client-A` 持有一枚**有效**的 OAuth access token（含 `rbac:role:read` scope）。
- **测试步骤**:
  1. 用该令牌调 `GET /api/rbac/me/permissions`；
  2. 用该令牌调 `GET /api/audit/events`（租户 `tenant-a`）；
  3. 用该令牌调 `POST /api/oauth/introspect`（不带 client 认证）；
  4. **对照**：用**正常的登录令牌**调同两个端点。
- **预期结果**:
  - 步骤 1、2、3：**401**（`UNAUTHENTICATED` 或协议错误 `invalid_token`——
    **二者都接受，但同一端点必须一致**）；
  - 步骤 4：**成功**（对照组必须绿 → 否则这条判据没有区分力）；
  - **必须带阳性对照**：步骤 4 若也 401 → 说明「整个平台都坏了」，
    本条判据**无区分力** → 判 fail。
- **类型**: 安全

---

### AC-S-152: 新增错误码按 **8 处**登记，且跨栈断言是「包含」不是「相等」

- **前置条件**: 本需求新增了错误码（清单见 §全局约定）。
- **测试步骤**:
  1. 跑 `crates/platform-server/tests/i18n_titles.rs`；
  2. 跑 `crates/platform-oauth` 自己的 `error.rs` 单测；
  3. 跑 `packages/i18n` 的测试；
  4. **人工核对** `i18n_titles.rs` 的 `all_codes()` 里**有** `platform_oauth` 那一行。
- **预期结果**:
  - 步骤 1：`TITLES` **包含**全部 `platform_oauth::error::ALL_CODES`；
    每个 locale（`en-US` 与 `zh-CN` **逐个**）**包含** `error.<CODE>`；
    `TITLES` 的行数断言同步更新（基线 58 + 21 + 5 + **本需求新增数**）；
  - 步骤 2：`ALL_CODES` 与 enum 变体**一一对应**（漏一个变体 → 变红）；
  - 步骤 3：两个 locale 的 `error.*` 键集合**各自**覆盖本需求的码；
  - 步骤 4：`all_codes()` 里**有** `for code in platform_oauth::error::ALL_CODES` 这一行；
  - ⚠️ **不得**断言 `TITLES.len() == i18n 条数`（D194：两侧数字相等是**巧合**，
    集合差 6↔6）→ 判据只钉「**包含**」。
- **类型**: 可维护性

---

### AC-S-153: 权限点三处齐、**启动对账 fail-fast**、C-5 在本 crate 上通过

- **前置条件**: 本需求新增了 `crates/platform-oauth/manifest.json` 与权限点种子迁移。
- **测试步骤**:
  1. 跑 `cargo xtask capabilities --check`（或契约检查的等价命令）；
  2. **只改 manifest、不加种子 INSERT**，起服务；
  3. 把 manifest 里的某个权限点从代码的 `require_permission` 调用点**删掉**，再跑 C-5；
  4. 在 `crates/platform-oauth/src/` 里放一个叫 `authorize` 的函数，再跑 C-5。
- **预期结果**:
  - 步骤 1：**通过**（14 个文件与源码一致，OAuth 的 manifest 计入）；
  - 步骤 2：服务**起不来**（退出码非 0），日志**逐条**列出缺失的权限点编码与
    来源 manifest 路径 `crates/platform-oauth/manifest.json`；
  - 步骤 3：C-5 **失败**（已登记但无鉴权调用点）→ 阳性对照，证明 C-5 在本 crate 上**真的在跑**；
  - 步骤 4：C-5 **失败**（出现第二个鉴权入口）→ 证明 D216 的命名禁区是**有效**的。
- **类型**: 可维护性

---

### AC-S-154: 关键动作产生**审计记录**

- **前置条件**: `tenant-a` 下有用户与 client；审计 sink 已接真实库
  （`crates/platform-server/src/main.rs` 的装配路径，REQ-014 D150）。
- **测试步骤**:
  1. 走一遍：注册 client → 同意授权 → 换令牌 → 撤销 → 登出；
  2. 查审计表（`tenant-a`）。
- **预期结果**:
  - 至少产生这 5 条事件码（D215）：
    `oauth.client.registered` / `oauth.authorization.approved` /
    `oauth.token.issued` / `oauth.token.revoked` / `oauth.logout.executed`；
  - 每条含 `actor_id` / `action` / `object_type` / `object_id` / `ip` / `user_agent` /
    `created_at` / `prev_hash` / `hash`（契约 C12 的固定字段集）；
  - **不含**任何 secret / token / code 的**明文**
    （对全表做敏感值扫描，**带阳性对照**）；
  - **哈希链自洽**：`GET /api/audit/verify`（或 `cargo xtask audit verify`）对该租户
    返回 `ok = true`。
- **类型**: 安全

---

### AC-S-155: OAuth 数据**跨租户不可见**

- **前置条件**: `tenant-a` 与 `tenant-b` 各有用户与授权记录。
- **测试步骤**:
  1. 以 `tenant-a` 的上下文查 `platform_oauth_grant` / `platform_oauth_token` /
     `platform_oauth_code` / `platform_oauth_device_code`；
  2. 以 `tenant-b` 的上下文查同样的表；
  3. 直接以 `base`（超管）查全部行。
- **预期结果**:
  - 步骤 1：只看到 `tenant-a` 的行；
  - 步骤 2：只看到 `tenant-b` 的行；
  - 步骤 3：能看到**两租户**的全部行（证明步骤 1、2 不是「表本来就空」）；
  - **RLS 真在生效的证据**：以 `base_app` 角色（**不**手动 `set_config`）查 →
    返回 **0 行**（RLS 默认隔离）→ 再在**单个事务内** `set_config` → 查 → 有行；
- **补充**：`platform_oauth_client` 是**平台级**表（无 `tenant_id`）→ 步骤 1、2 都**能看到**
  全部 client（这是**有意的**，D219）。
- **类型**: 安全

---

## 完成定义对照

| 能力 | AC |
|---|---|
| 元数据可发现 | 134 |
| client 管理 | 135 / 136 |
| 授权码 + PKCE | 137 / 138 / 139 |
| scope 语义 | 140 |
| Client Credentials | 141 |
| Device Code | 142 |
| 内省 / 撤销 | 143 / 144 |
| 令牌格式 | 145 |
| 同意与授权管理 | 146 / 147 |
| 免登 | 148 |
| 单点登出 | 149 / 150 |
| 默认拒绝 | 151 |
| 错误码登记 | 152 |
| 权限点登记 | 153 |
| 审计埋点 | 154 |
| 租户隔离 | 155 |
