# 验收标准：出站 IdP 与单点登出

- **需求编号**: REQ-022
- **父需求**: REQ-001
- **状态**: **rev 2**（2026-11-20 —— QA 挑刺 **39 条**后逐条裁决修完）
- **对应验收标准**: **22 条**（AC-S-134 ~ AC-S-155）
- **决策依据**: [decisions.md](decisions.md) rev 2（rev 1 的 D196~D220 + **rev 2 的 D221~D256**）
- **挑刺报告**: [../../feature-meta/qa-req022-spec-review.md](../../feature-meta/qa-req022-spec-review.md)
  （**39 条**：阻塞 10 / 高 19 / 中 10；**22/22 AC 全部有问题**）
- **用例**: [test-cases/index.md](test-cases/index.md)（**131 条**，TC-022-001~131，编号连续）
- **更新时间**: 2026-11-20 23:50

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## rev 2 变更摘要（**Dev 必读**）

> rev 1 的 **22 条 AC 全部有问题**（QA 挑刺 39 条）→ 本 rev 全部修完。
> 裁决全部记录在 [decisions.md](decisions.md) rev 2，**每条 AC 的改动都标了决策号**。

**rev 1 有 6 处会让 Dev「第一次跑就红」或「照字面实现必错」的坑**：

1. **AC-S-137 的第三方回调用 `page.route` 拦截** —— `https://client.example/cb` **不可达**，
   照字面写 `page.goto` 必红（D235）
2. **未登录回跳会丢 query** —— 平台的 `intendedPath` 只有 pathname，
   `write()` 在 path 变化时丢弃 `search` → 授权参数全丢（D221 / **已定方案**）
3. **`TITLES` 行数 = 93**（不是 102）—— 事件码**不进** `TITLES`，死码已删（D223 / D226）
4. **AC-S-151 必须钉 `UNAUTHENTICATED`** —— rev 1 写「二者都接受」= 永远判不出红绿（D224）
5. **`cargo xtask capabilities --check` 基线就是红的** —— 它也**不做** C-5；
   真 C-5 命令是 `cargo test -p platform-oauth --test contract_c5`（D240）
6. **AC-S-139 / AC-S-142 的「SHA-256 摘要」** —— 编码是 **base64url 无填充**，不是 hex（D228）

### rev 3 补记（2026-11-21，**不改任何一条 AC**）

写 Dev 任务卡时发现两个**并行实现的耦合缺口**，已由 BA 补记：

| 决策 | 补的是什么 | 对本文件的影响 |
|---|---|---|
| **D257** | 设备码的「用户批准」端点：`GET /api/oauth/device/verify` + `POST /api/oauth/device/approve`（**AC-S-142 步骤 4 要这个动作，但端点清单里没有**） | **无** —— 只是让步骤 4 可实现；**不新增错误码** → 下面「9 个平台错误码」与「`TITLES` 行数 == 93」**都不动** |
| **D258** | 跨栈 JSON **字段名**：平台端点 camelCase、协议端点用 RFC 的 snake_case；8 个平台端点的请求 / 响应字段**逐字写死** | **无** —— 但 AC-S-147 / AC-S-150 里的 `clientId` / `scopes` / `status` 等字段名**以 D258 为准** |

> 两条的原文见 [decisions.md](decisions.md) 的 rev 3 段。

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
| 哈希复算 | 测试侧**独立实现**，**直接使用列里存的文本**，不得解析回 JSON 再序列化 | D141 |
| 静态扫描 | 必须带**阳性对照**（否则「0 命中」不可信） | dev-guide §9.2 |
| 协议端点 | OAuth **协议端点**按 RFC 说 RFC 的话；**平台管理端点**按契约 C6 说平台的话 | 契约 C14 |

### ⚠️ 两套错误标识（本需求最容易搞混的一处）

| 类别 | 形状 | 出现在 | 取值风格 |
|---|---|---|---|
| **协议错误** | `{"error": "invalid_grant", "error_description": "…"}` | `/api/oauth/token`、`/introspect`、`/revoke`、`/device_authorization` | **小写下划线** |
| **平台错误** | RFC 9457 Problem Details（契约 C6） | `/api/oauth/authorize/*`、`/api/oauth/clients/*`、`/api/oauth/grants/*`、`/api/oauth/logout` | **大写下划线**（`OAUTH_*`） |

- 协议错误的**取值集合**（契约 C14 §14.2）：
  `invalid_request` / `invalid_client` / `invalid_grant` / `unauthorized_client` /
  `unsupported_grant_type` / `unsupported_response_type` / `invalid_scope` /
  `access_denied` / `login_required` / `consent_required` /
  `authorization_pending` / `slow_down` / `expired_token` / `server_error`
  ⚠️ **集合里没有 `invalid_token`**（它来自 RFC 6750，本契约未登记）→ **不得使用**（D224）。
- **平台错误码清单（9 个，D226 删掉死码后）**：
  `OAUTH_CLIENT_NOT_FOUND` / `OAUTH_CLIENT_DISABLED` / `OAUTH_CLIENT_EXPIRED` /
  `OAUTH_INVALID_REQUEST` / `OAUTH_INVALID_REDIRECT_URI` / `OAUTH_INVALID_SCOPE` /
  `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` / `OAUTH_TICKET_INVALID` /
  `OAUTH_GRANT_NOT_FOUND`

### ⚠️ 摘要编码口径（rev 2 新增，D228）

本需求所有「只存摘要」的列 —— `client_secret_hash` / `code_hash` /
`device_code_hash` / `user_code_hash` —— 一律用 **`digest_base64url`** 口径：

```
SHA-256 原始 32 字节 → base64url（无填充）→ 43 字符
```

**不是** hex（仓里另有一个 `digest_hex`，本需求**不用**它）。
测试侧独立复算时按此口径。

### ⚠️ `{授权页基址}` 符号（rev 2 新增，D222）

| 符号 | 含义 |
|---|---|
| `{issuer}` | 配置 `BASE_JWT_ISSUER` |
| **`{授权页基址}`** | 配置 **`oauth.frontendBaseUrl`**；**未配置（空串）时回退到 `{issuer}`** |

⚠️ E2E 里二者**不相等**（`https://e2e.base` vs `http://127.0.0.1:8080`）
→ 一切「授权页 / 设备码页」的 URL 都用 `{授权页基址}`，**不得**直接用 `{issuer}`。

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
  - `authorization_endpoint` = **`{授权页基址}` + `/oauth/authorize`**（D222）；
  - `token_endpoint` = `{issuer}/api/oauth/token`；
  - `device_authorization_endpoint` = `{issuer}/api/oauth/device_authorization`；
  - `introspection_endpoint` = `{issuer}/api/oauth/introspect`；
  - `revocation_endpoint` = `{issuer}/api/oauth/revoke`；
  - `jwks_uri` = `{issuer}/.well-known/jwks.json`；
  - `grant_types_supported` **恰好等于**
    `["authorization_code","client_credentials","urn:ietf:params:oauth:grant-type:device_code"]`
    （**逐元素比对，顺序也钉死**；**不含** `refresh_token`，D217）（D250）；
  - `response_types_supported` **恰好** `["code"]`；
  - `code_challenge_methods_supported` **恰好** `["S256"]`（**不含** `plain`，D206）；
  - `token_endpoint_auth_methods_supported` **包含** `client_secret_basic` 与 `none`；
  - `introspection_endpoint_auth_methods_supported` **恰好** `["client_secret_basic"]`（D208）；
  - `revocation_endpoint_auth_methods_supported` **恰好** `["client_secret_basic"]`；
  - `access_token_expires_in` = 平台配置的 `oauth.accessTokenTtlSeconds`（D210）。
- **类型**: 功能

---

### AC-S-135: client 可注册、可改、可停用、可删，且**回调白名单**生效

- **前置条件**:
  - `tenant-a` 下存在 E2E 管理员账号 `u-perm-oauth@example.com`，
    **持有 `oauth:client:read` / `oauth:client:write` / `oauth:client:delete`**
    —— **由 `scripts/seed-e2e.sh` 播种**（D236；脚本的强断言段必须自证这三个权限点已授权）；
  - 测试库 `base_test` 可用。
- **测试步骤**:
  1. `POST /api/oauth/clients`，body 含 `name` / `clientType` / `redirectUris` /
     `allowedGrantTypes` / `allowedScopes`；
  2. 读响应；
  3. `GET /api/oauth/clients` 列出；
  4. `PATCH /api/oauth/clients/{client_id}` 把 `status` 改成 `disabled`；
  5. **`POST /api/oauth/authorize/preview`**（用已 disabled 的 client）；
  6. **`POST /api/oauth/token`**（client 认证用已 disabled 的 client）；
  7. `DELETE /api/oauth/clients/{client_id}` 后再查。
- **预期结果**:
  - 步骤 2：**201 CREATED**（**不是 200** —— 平台创建类端点的统一口径，D234），
    含 `clientId`（**非空、唯一**）与 **`clientSecret`（仅此一次**，D200）；
    响应**不含** `clientSecretHash`；
  - 步骤 3 的列表里**没有** `clientSecret` 字段（D200）；
  - 步骤 5：**403** + Problem Details，`code` **逐字等于** `OAUTH_CLIENT_DISABLED`
    （**平台话** —— preview 是平台端点）；
  - 步骤 6：**401** + 响应体**逐字等于** `{"error":"invalid_client"}`
    （**协议话** —— token 是协议端点）（D249）；
  - 步骤 7 之后 `GET /api/oauth/clients/{client_id}` 返回 **404** + `OAUTH_CLIENT_NOT_FOUND`。
- **类型**: 功能

---

### AC-S-136: `client_secret` 只以 **SHA-256 base64url 摘要**落库，任何读接口不回显

- **前置条件**: 已注册一个 `confidential` client，拿到过一次明文 secret。
- **测试步骤**:
  1. 以 `base` 直连查 `platform_oauth_client` 的 `client_secret_hash` 列；
  2. 测试侧**独立实现** SHA-256 + base64url（**不调用生产代码**），对明文 secret 求摘要；
  3. 调 `GET /api/oauth/clients` 与 `GET /api/oauth/clients/{client_id}`，读全量响应体；
  4. **运行期断言**：拿第 4 步之前拿到的明文 secret 去
     `GET /api/oauth/clients`、`GET /api/oauth/clients/{id}`、
     `POST /api/oauth/token` 的**响应体**、`GET /api/audit/events` 的**全表文本**
     → 逐字**不含**该明文；
  5. **静态扫描（辅判据）**：Rust 侧一个**可单独调用的 `pub fn`**
     扫描 `crates/platform-oauth/src/` 里构造响应体的位置是否出现 `client_secret` 字段名；
     在**临时副本**里塞一个 `client_secret` 字段 → 扫描器**必须命中**（阳性对照，D243）。
- **预期结果**:
  - 步骤 1 的值 **逐字节等于**步骤 2 的计算结果（**base64url 无填充**，D228）；
  - 步骤 3 的两个响应体**都不含** `clientSecret`（**键都不存在**，不是值为 null）；
  - 步骤 4：四处输出**逐字**不含该明文（**用真值断言**，不是「没看到某个键」）；
  - 步骤 5：扫描器在真实代码上 **0 命中**，在临时副本上**命中**。
- **类型**: 安全

---

### AC-S-137: 授权码 + PKCE 能换到可用令牌（端到端）

- **前置条件**:
  - `tenant-a` 下有用户 `e2e-user-oauth@example.com`（持有权限点 `rbac:role:read`）——
    **由 `scripts/seed-e2e.sh` 播种**（D236）；
  - 已注册 `confidential` client，回调白名单含 `https://client.example/cb`，
    `allowedScopes` 含 `rbac:role:read`，`allowedGrantTypes` 含 `authorization_code`；
  - 平台前端已起（`apps/shell/dist`）。
- **测试步骤**:
  1. **先拦截第三方回调**（D235）：
     `page.route('https://client.example/**', route => route.fulfill({
     status: 200, contentType: 'text/html', body: '<html>ok</html>' }))`；
  2. 浏览器访问 `{授权页基址}/oauth/authorize?response_type=code&client_id=…&
     redirect_uri=https://client.example/cb&scope=rbac%3Arole%3Aread&state=xyz&
     code_challenge=<S256(verifier)>&code_challenge_method=S256`；
  3. **未登录时**页面应显示「登录并继续」；点它 → 跳 `/login?return_to=…` → 登录
     （`tenant-a` / 账号 / 密码）→ **自动回到原 URL**（**query 必须还在**，D221）；
  4. 在同意页点「允许」；
  5. 读 `page.url()` 的 **query 参数**（**不是**地址栏裸字符串）；
  6. `POST /api/oauth/token`，form 编码 `grant_type=authorization_code` / `code` /
     `client_id` / `client_secret` / `redirect_uri` / `code_verifier`；
  7. 读响应体；
  8. 用 `access_token` 调 `POST /api/oauth/introspect`（client 认证）。
- **预期结果**:
  - 步骤 3：登录后回到 `/oauth/authorize` 时，**`client_id` / `state` /
    `code_challenge` / `redirect_uri` 全部仍在**（D221 的核心判据）；
  - 步骤 5：`new URL(page.url()).searchParams.get('code')` **非空**，
    `…get('state')` **逐字等于** `xyz`；
  - 步骤 7：**200**，含 `access_token` / `token_type`（值 `"Bearer"`）/
    `expires_in` / `scope`（**等于** `rbac:role:read`）；
  - 步骤 8：`active = true`，且 `scope` **包含** `rbac:role:read`。
- **类型**: 功能

---

### AC-S-138: PKCE **强制 S256**，缺挑战或方法不支持一律拒

- **前置条件**: 同 AC-S-137 的 client（`allowedGrantTypes` 含 `authorization_code`）。
- **测试步骤**（**三条独立路径**）:
  1. 授权请求**不带** `code_challenge` → 调 `POST /api/oauth/authorize/preview`；
  2. 授权请求带 `code_challenge` 但 `code_challenge_method=plain` → 调 preview；
  3. 授权请求带 `code_challenge_method=S256`（challenge 取 `S256("wrong-verifier")`），
     **先 preview 成功**（拿 ticket）**再** confirm 时传错的 `code_verifier` → 交换。
- **预期结果**:
  - 路径 1：preview → **400** + Problem Details，`code` **逐字等于** `OAUTH_INVALID_REQUEST`，
    `status == 400`，`Content-Type: application/json`，
    响应体**不含** `ticket` 键；→ **不得**调用 `confirm`、**不得**产生任何授权码行（D239）；
  - 路径 2：preview → **400** + `code` **逐字等于** `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD`；
  - 路径 3：交换 → **400** + `{"error": "invalid_grant"}`（**协议错误体**），
    且 `error_description`：
    ① **不含** `code_verifier` / `code_challenge` / `client_secret` / `code` 的**明文**
    （逐个子串断言）；② **不含任何 CJK 字符**（D251）。
- **类型**: 安全

---

### AC-S-139: 授权码**一次性**、只存摘要、**60 秒**过期

- **前置条件**: 已通过授权流程拿到一个**未兑换**的授权码 `code`。
- **测试步骤**:
  1. 以 `base` 直连查授权码表里对应行（**用摘要匹配**，不按明文）；
  2. 用 `code` + 正确 `code_verifier` 交换 → 拿令牌；
  3. **立刻**用**同一个** `code` + 同一个 `code_verifier` 再交换一次；
  4. `SELECT count(*) FROM platform_oauth_code WHERE code_hash = <base64url(SHA256(code))>`；
  5. 查 `information_schema.columns` 里 `platform_oauth_code` 的列集合。
- **预期结果**:
  - 步骤 1：库里的值 **等于** 测试侧独立计算的 **base64url 无填充** SHA-256 摘要
    （**不是**明文、**不是** hex）（D228）；
  - 步骤 2：**200**，拿到令牌；
  - 步骤 3：**400** + `{"error": "invalid_grant"}`；
  - 步骤 4：**恰好 0**（D253）；
  - 步骤 5：列集合里**不含** `consumed_at` / `used_at`（删除即消费，没有标记列）（D253）。
- **补充判据（TTL，D252 —— 逐列写死）**:
  > 以 `base` 直连 `INSERT` 一行 `platform_oauth_code`：
  > `tenant_id='tenant-a'`、`client_id=<client-A>`、`user_id=<u>`、
  > `code_hash=<base64url(SHA256('ttl-expired-code'))>`、
  > `redirect_uri='https://client.example/cb'`、`scope='rbac:role:read'`、
  > `code_challenge=<S256(verifier)>`、`code_challenge_method='S256'`、
  > `expires_at = now() - interval '90 seconds'` →
  > 交换返回 **400 + `{"error":"invalid_grant"}`；
  > 同一批值、**只**把 `expires_at` 改成 `now() + interval '30 seconds'` →
  > 交换**成功**（200）。
  > → 两条断言**只在 `expires_at` 上不同**，其它列逐字相同 → 才证明判的是 TTL。
- **类型**: 安全

---

### AC-S-140: scope 是**三重交集**；通配一律不生效；不得越过用户本人

- **前置条件**:
  - client 的 `allowedScopes` = `["rbac:role:read", "rbac:role:write"]`；
  - 测试用户**只**持有 `rbac:role:read`（**没有** `rbac:role:write`）。
- **测试步骤**（**五条独立路径**）:
  1. 请求 `scope=rbac:role:read` → 交换 → 读令牌响应的 `scope`；
  2. 请求 `scope=rbac:role:read rbac:role:write`（用户**没有** write）→ 交换；
  3. 请求 `scope=sandbox:demo:read`（**不在** client 的 `allowedScopes` 里）→ 交换；
  4. 请求 `scope=rbac:role:*` → 交换；
  5. **不回溯**：请求 `scope=rbac:role:read` → 交换**成功**（令牌 A）→
     管理员给该用户**追加** `rbac:role:write`（`platform_role_permission` +
     bump `platform_permission_epoch`）→ **重放令牌 A**（**不**重新交换）→ 内省；
     另发一枚令牌 B（scope 集 = `rbac:role:read rbac:role:write`）→ 读 B 的 scope。
- **预期结果**:
  - 路径 1：`scope` **等于** `rbac:role:read`；
  - 路径 2、3、4：**400**，错误为 `{"error":"invalid_scope"}`（协议错误，小写）；
  - 路径 5：
    - 令牌 A 的内省 `scope` **仍逐字等于** `rbac:role:read`，**不含** `rbac:role:write`；
    - 令牌 B 的 scope **含** `rbac:role:write`
      → 证明「授予**新**令牌会拿到新权限，但**旧**令牌不被回溯扩大」（D242）。
- **额外断言（D241 —— 改成三条可判断言）**:
  1. 测试侧查表：`SELECT count(*) FROM platform_permission WHERE code = 'rbac:role:*'` **== 0**；
  2. 用户有效权限点集合里**不含**任何含 `*` 的码
     （`GET /api/rbac/me/permissions` 的返回集合 `NOT LIKE '%*%'`）；
  3. **真正的对照**：把 `rbac:role:*` **登记进 `platform_permission`**
     （管理员直连 INSERT，**仅本用例夹具**）→ 重跑路径 4 → **仍然** 400 `invalid_scope`
     → 这才证明「令牌 scope 层面通配**一律**不生效」，而**不是**因为「它没注册」。
- **类型**: 安全

---

### AC-S-141: Client Credentials grant 可换到**无用户**的令牌

- **前置条件**: 已注册 `confidential` client，`allowedGrantTypes` 含 `client_credentials`，
  `allowedScopes` **只含** `sandbox:demo:read`。
- **测试步骤**:
  1. `POST /api/oauth/token`，form `grant_type=client_credentials` / `scope=sandbox:demo:read` /
     `client_id` / `client_secret`；
  2. 读响应体；
  3. 解析 `access_token` 的 payload（**只解 payload，不验签**）；
  4. `POST /api/oauth/introspect` 该令牌；
  5. 再请求一次 `scope=sandbox:demo:write`（**不在** `allowedScopes` 里）。
- **预期结果**:
  - 步骤 2：**200**，含 `access_token` / `token_type` / `expires_in` / `scope`；
  - 步骤 3：payload 里**没有** `sub` 键（**不是**空串、不是 null —— 键不存在）；
    `client_id` **等于**该 client；`grant_type` **等于** `client_credentials`；
  - 步骤 4：`active = true`，且 `username` 键**不存在**（RFC 7662：无用户主体）；
  - 步骤 5：**400** + `{"error":"invalid_scope"}`
    （`client_credentials` 的 scope 是**两重**交集：请求 ∩ `client.allowed_scopes`，
    **无用户维**，D231）。
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
  3. **连续轮询第 2 次**（间隔 < `interval`）；
  4. 浏览器访问 `verification_uri`，输入 `user_code`，点「允许」；
  5. 再用**同一个** `device_code` 轮询；
  6. 查设备码表与 `last_polled_at` / `poll_interval_seconds`。
- **预期结果**:
  - 步骤 1：**200**，五个字段齐全；`expires_in` = 平台配置的 `oauth.deviceCodeTtlSeconds`；
    `interval` = 平台配置的 `oauth.deviceCodePollIntervalSeconds`；
    `verification_uri` **逐字等于** `{授权页基址}/oauth/device`（D246 / D222）；
    `verification_uri_complete` **逐字等于**
    `{授权页基址}/oauth/device?user_code=<urlencoded user_code>`；
    `user_code` 长度 **== 8**，且**逐字符** ∈ `BCDFGHJKLMNPQRSTVWXZ`（base20，RFC 8628 §6.1）；
  - 步骤 2：**400** + `{"error": "authorization_pending"}`；
  - 步骤 3：**400** + `{"error": "slow_down"}`，
    且设备码行的 `poll_interval_seconds` **被更新为** `interval + 5`（RFC 8628 §3.5）；
  - 步骤 5：**200**，含 `access_token`（`scope` = `sandbox:demo:read`）；
  - 步骤 6：设备码行**已被删除**（按 `device_code_hash` 查 **恰好 0**）；
    `last_polled_at` **非空**且**递增**；
  - **补充判据**：`user_code` 与 `device_code` 在库里**都只存 base64url 摘要**（D228）；
    用 `user_code` **直接换令牌**（当成 `device_code` 用）→ **失败**。
- **类型**: 功能

---

### AC-S-143: 令牌内省可用，且**只能内省自己的令牌**

- **前置条件**: `client-A`（confidential）与 `client-B`（confidential）各持一枚有效令牌；
  另有一个 `public` client（无 secret）。
- **测试步骤**:
  1. `client-A` 内省 `client-A` 的令牌；
  2. `client-A` 内省 `client-B` 的令牌；
  3. `client-A` 内省一枚**已过期**的令牌；
  4. `client-A` 内省一枚**已撤销**的令牌；
  5. `public` client 内省任意令牌（只带 `client_id`，不带 secret）；
  6. 不带任何认证内省；
  7. `public` client 用 `client_secret_basic` 认证（secret 为**空串**）内省。
- **预期结果**:
  - 步骤 1：**200**，`active = true`，`client_id` / `scope` / `exp` / `iat` / `sub` /
    `token_type` / `iss` / `aud` **齐全**；
  - 步骤 2：**200**，`active = false`，且**除 `active` 外没有任何其它字段**
    （**不泄露**该令牌存在、更不泄露其 `client_id`）；
  - 步骤 3、4：**200**，`active = false`，同样**只有 `active` 字段**；
  - 步骤 5、6、7：**401** + 响应体**逐字等于** `{"error":"invalid_client"}`
    （**协议错误体**，**不得**含 `type` / `title` / `status` / `instance` / `code` /
    `trace_id` —— 契约 C14 §14.2）（D244）；
  - ⚠️ **认证失败（401 + `invalid_client`）与「令牌不属于本 client」（200 + `active=false`）
    是两件事**，不得混判（D244）。
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

- **前置条件**: `client-A` 持有一枚**由 `authorization_code` grant 签发**的有效令牌
  （D245）；平台 JWKS 可取。
- **测试步骤**:
  1. 取 `GET /.well-known/jwks.json`；
  2. 解析 `access_token` 的 header 与 payload（**测试侧独立解码**）；
  3. 测试侧**独立实现** ES256 验签（用 JWKS 里的公钥），**不调用生产代码**；
  4. 用**过期**令牌重复步骤 3；
  5. 对 **`client_credentials`** 令牌重复步骤 2。
- **预期结果**:
  - 步骤 2：header 的 `alg` **等于** `ES256`，`kid` **在** JWKS 的 `keys[].kid` 之中；
    payload 的 `iss` **等于** `BASE_JWT_ISSUER`，`aud` **等于** `client-A` 的 `client_id`，
    `sub` / `tid` / `client_id` / `scope` / `grant_type` / `iat` / `nbf` / `exp` / `jti` **齐全**；
  - 步骤 3：验签**通过**；
  - 步骤 4：验签**通过**（签名有效）但 `exp` **已过期** → 结论是「已过期」，
    **不得**把「签名无效」与「已过期」混为一谈（两者必须是不同的断言）；
  - 步骤 5：`sub` **键不存在**，其余 claims 齐全（与 AC-S-141 一致，D245）。
- **类型**: 安全

---

### AC-S-146: 同意页可用、同意记录落库，且**用户所见 = 实际授予**

- **前置条件**: 同 AC-S-137；`tenant-a` 下该用户**尚无**该 client 的授权记录；
  **E2E 的 JWT 私钥落盘在 `target/e2e-jwt-private.pem`（0600，不入 Git，每次运行覆盖）**
  —— 由 `scripts/e2e-server.sh` 生成并写入（D238）。
- **测试步骤**:
  1. 走完授权流程（AC-S-137 步骤 2~4），在同意页**允许**；
  2. 以 `base` 直连查 `platform_oauth_grant`；
  3. 用 `target/e2e-jwt-private.pem` **自己签**一个 `typ="oauth_consent_ticket"`、
     `scope` **比 preview 返回的更大**的票据 → 调 `POST /api/oauth/authorize/confirm`；
  4. 调 `POST /api/oauth/authorize/deny`（用步骤 1 的合法 ticket）。
- **预期结果**:
  - 步骤 1：同意页**展示**了 client 的 `name` 与请求的**全部**权限点码
    （页面文本里逐字出现 `rbac:role:read`）；
  - 步骤 2：恰好 **1 行**，`client_id` / `user_id` / `scopes` / `status = active` /
    `consent_at` **非空**；
  - 步骤 3：**400** + `OAUTH_TICKET_INVALID`（**不得**签发任何 code）
    → **这一条才真正证明 D204 的承诺**（签名有效、scope 更大 → 仍被拒）；
  - 步骤 4：返回的 `redirectTo` 的 query 里 `error` **等于** `access_denied`，
    且 `state` **逐字等于**原请求的 `state`；
    → 步骤 4 之后查 grant 表：**仍然只有步骤 2 那 1 行**，且 `status` 仍是 `active`
    （拒绝**不改**已有授权）。
- **类型**: 安全

---

### AC-S-147: 用户能查自己的授权并撤销，撤销后令牌**立即失效**

- **前置条件**:
  - `tenant-a` 下用户已对 `client-A` 授权（AC-S-146 步骤 1 已落库）；
  - `client-A` 持有**两枚**由该 grant 签发的有效令牌（同一 grant，D254）；
  - `tenant-b` 下的用户 `u-perm-other@example.com`（**由 `scripts/seed-e2e.sh` 播种**，D236）。
- **测试步骤**:
  1. `GET /api/oauth/grants`（用户会话）；
  2. `POST /api/oauth/grants/revoke`，body `{ clientId: "client-A" }`；
  3. 再查授权记录；
  4. 内省**两枚**令牌；
  5. 用 `tenant-b` 的用户调 `GET /api/oauth/grants`。
- **预期结果**:
  - 步骤 1：返回**恰好 1 条**，含 `clientId` / `scopes` / `grantedAt` / `status`；
    **不含**任何其它用户或其它 client 的记录；
  - 步骤 2：**200**；
  - 步骤 3：该条 `status` **等于** `revoked`，`revokedAt` **非空**；
  - 步骤 4：**两枚**都 `active = false`（**grant 级撤销**，D254），
    且 `platform_oauth_token` 里这两行状态**都是** `revoked`；
  - 步骤 5：返回**空数组**（**不泄露**其它租户的授权存在性）。
- **类型**: 安全

---

### AC-S-148: 已有授权时**免登**（静默续发，不重复同意页）

- **前置条件**: `tenant-a` 下用户已对 `client-A` 授权，且该 grant 覆盖请求的 scope。
- **测试步骤**:
  1. 浏览器访问 `{授权页基址}/oauth/authorize?…&prompt=none&…`（参数同 AC-S-137）；
  2. 观察**是否出现同意页**；
  3. 读最终地址栏的 `code`；
  4. 再访问一次，**但**把 `scope` 改成**更大**的集合（超出已有 grant）；
  5. 访问 `{授权页基址}/oauth/authorize?…` **不带** `prompt=none`。
- **预期结果**:
  - 步骤 2：**没有**同意页（页面直接跳转）；步骤 3：地址栏含 `code`；
  - 步骤 4：`preview` 返回 **200 + `needsConsent == true`**（**不是错误**，D233）
    → 页面显示同意页，且**授权页的 query** 里 `error=consent_required`；
    且 `platform_oauth_code` 行数 **== 0**；
  - 步骤 5：**出现**同意页（`prompt` 缺省 = 允许交互）。
  - ⚠️ `login_required` / `consent_required` 两个**协议码**只出现在**授权页的 query**，
    **不是** preview 的错误体（D233）。
- **类型**: 功能

---

### AC-S-149: Back-Channel Logout 能向 client 投递 `logout_token`

- **前置条件**:
  - **mock 接收端已按 `scripts/mock-logout-receiver.sh` 起好**
    （监听 `127.0.0.1:$MOCK_PORT`，收到的 `logout_token` 写到
    `target/mock-logout-received.jsonl`，行为由 `MOCK_MODE=ok|timeout|500` 控制）（D237）；
  - `tenant-a` 下用户已对 `client-A` 授权（其 `backchannel_logout_uri` 由
    `scripts/seed-e2e.sh` 按 `$MOCK_PORT` 写入）。
- **测试步骤**:
  1. 触发全局登出；
  2. 读 mock 落盘的文件（`target/mock-logout-received.jsonl` 的最后一行）
     —— **不要**在 E2E 进程里 mock HTTP；
  3. **测试侧独立**解析 `logout_token` 的 payload；
  4. 用 `MOCK_MODE=timeout` **重启** mock 后，再触发一次登出；
  5. 用 `MOCK_MODE=500` **重启** mock 后，再触发一次登出。
- **预期结果**:
  - 步骤 2：收到的是 **form 编码**（`Content-Type: application/x-www-form-urlencoded`），
    含 `logout_token=…`；
  - 步骤 3：payload 的 `iss` **等于**平台 issuer、`aud` **等于** `client-A` 的 `client_id`、
    `sub` / `sid` / `jti` / `iat` **齐全**、
    `events` **含**键 `http://schemas.openid.net/event/backchannel-logout`；
    payload **不含** `nonce`（D211）；
    `sid` **逐字等于** 触发登出的登录令牌的 `sid`（D230）；
  - 步骤 4、5：**登出本身仍然成功**（HTTP 200，授权记录已撤销）——
    **通知失败绝不能让登出失败**（D211）；
  - **补充判据**：投递失败时**有日志**（含 `clientId` 与失败原因）；
  - **补充判据（D229 —— 显式写明现状）**：
    登出**只**撤销 OAuth 授权与 refresh token；
    **已签发的登录 access token 最长延迟一个 `iam.accessTokenTtl` 秒**才失效
    （平台验签不查库）→ **本轮不修**，记 **TD-043**；
    而 **OAuth 令牌**由 `platform_oauth_token` 表的状态决定 → 撤销 grant 后**立即**失效。
- **类型**: 功能

---

### AC-S-150: Front-Channel Logout 与按应用登出可用

- **前置条件**: `tenant-a` 下用户已对 `client-A`（配了 `frontchannel_logout_uri`）
  与 `client-B`（**未**配）授权。
- **测试步骤**:
  1. 触发全局登出，读**后端响应体**（JSON）；
  2. 读**前端渲染后的 DOM**（`apps/shell/test/` 的组件测试或 E2E）；
  3. 重新授权后，触发**按应用登出**（`POST /api/oauth/grants/revoke`，body `{ clientId }`）；
  4. 查 `client-B` 的授权记录。
- **预期结果**:
  - 步骤 1：后端响应体里 `frontChannel` 数组**恰好 1 条**（**只有** client-A），
    含 `clientId` 与 `logoutUrl`（该 URL **逐字等于** client 表里登记的值）；
    **不含** `client-B` 的 `logoutUrl`（D248）；
  - 步骤 2：**前端渲染**的 iframe：
    - 数量**恰好 1**；
    - `src` **逐字等于** `client-A.frontchannel_logout_uri` + `?logout_token=<JWT>`；
    - `sandbox` 属性**含** `allow-scripts` 且**不含** `allow-same-origin`；
    - `client-B` **不在**其中（D225）；
  - 步骤 3：**只**撤销 `client-A`；步骤 4：`client-B` 的记录 `status` **仍是** `active`；
  - **补充判据（D248 —— 有区分力的版本）**：
    全局登出后 **`client-B`（未配 front-channel URI）的 grant 也**被撤销
    → 这证明 front-channel 配置**只影响通知方式**，**不影响撤销范围**。
- **类型**: 功能

---

### AC-S-151: 底座业务端点**默认拒绝**第三方 OAuth 令牌

- **前置条件**: `client-A` 持有一枚**有效**的 OAuth access token（含 `rbac:role:read` scope）。
- **测试步骤**:
  1. 用该令牌调 `GET /api/rbac/me/permissions`；
  2. 用该令牌调 `GET /api/audit/events`（租户 `tenant-a`）；
  3. 用该令牌调 **`GET /api/rbac/me/permissions`**（**同一个**端点，步骤 1 已在用）；
  4. **3.5**：用该令牌调 `POST /api/oauth/introspect`，但**带** client-A 的合法
     `client_secret_basic` 认证、`token` 参数填该 OAuth 令牌；
  5. **对照**：用**正常的登录令牌**调同两个端点。
- **预期结果**:
  - 步骤 1、2、3：**401** + 错误体 `code` **逐字等于** `UNAUTHENTICATED`
    + `Content-Type: application/json`，且**不含** `error` 键（D224 —— **不得**用
    `invalid_token`，它不在契约 C14 的协议码集合里）；
  - 步骤 4：**200** + `active == true`
    → 证明该令牌**只**在 OAuth 协议端点上有效，**业务端点拒它**；
  - 步骤 5：**成功**（对照组必须绿 → 否则这条判据没有区分力）；
  - **必须带阳性对照**：步骤 5 若也 401 → 说明「整个平台都坏了」，
    本条判据**无区分力** → 判 fail。
- **类型**: 安全

---

### AC-S-152: 新增错误码按 **8 处**登记，且跨栈断言是「包含」不是「相等」

- **前置条件**: 本需求新增了 **9 个**平台错误码（D226）。
- **测试步骤**:
  1. 跑 `crates/platform-server/tests/i18n_titles.rs`；
  2. 跑 `crates/platform-oauth` 自己的 `error.rs` 单测；
  3. 跑 `packages/i18n` 的测试；
  4. **人工核对** `i18n_titles.rs` 的 `all_codes()` 里**有** `platform_oauth` 那一行。
- **预期结果**:
  - 步骤 1：`TITLES` **包含**全部 `platform_oauth::error::ALL_CODES`；
    每个 locale（`en-US` 与 `zh-CN` **逐个**）**包含** `error.<CODE>`；
    **`TITLES` 的行数 == 93**（= 58 + 21 + 5 + **9**）——
    断言常量为 `REQ_022_NEW_CODES: usize = 9`；**逐字写出 93**（D223）；
    ⚠️ **8 个 `oauth.*` 审计事件码不进 `TITLES`**（D223）→ 行数**不是** 102；
  - 步骤 2：`ALL_CODES` 与 enum 变体**一一对应**（漏一个变体 → 变红）；
  - 步骤 3：两个 locale 的 `error.*` 键集合**各自**覆盖本需求的 9 个码；
  - 步骤 4：`all_codes()` 里**有** `for code in platform_oauth::error::ALL_CODES` 这一行；
  - ⚠️ **不得**断言 `TITLES.len() == i18n 条数`（D194：两侧数字相等是**巧合**，
    集合差 6↔6）→ 判据只钉「**包含**」。
- **类型**: 可维护性

---

### AC-S-153: 权限点三处齐、**启动对账 fail-fast**、C-5 在本 crate 上通过

- **前置条件**: 本需求新增了 `crates/platform-oauth/manifest.json` 与权限点种子迁移。
- **测试步骤**:
  1a. `cargo xtask capabilities` **重新生成**；
  1b. `cargo test -p platform-oauth --test contract_c5`（**该测试文件由本需求新增**）；
  2. **只改 manifest、不加种子 INSERT**，起服务；
  3. 把 manifest 里的某个权限点从代码的 `require_permission` 调用点**删掉**，再跑 1b 的命令；
  4. 在 `crates/platform-oauth/src/` 里放一个叫 `authorize` 的函数，再跑 1b 的命令。
- **预期结果**:
  - 步骤 1a：`capabilities/` 下**恰好 14 个文件**（**不增不减** —— 新增 crate **不新增**文件），
    且 `capabilities/permissions.json` 的 `total == 32`（26 + 6 个 `oauth:*`），
    且含 `oauth:client:read` / `oauth:client:write` / `oauth:client:delete` /
    `oauth:grant:read` / `oauth:grant:revoke` / `oauth:token:introspect` **逐字 6 条**；
    ⚠️ **本需求必须先还 `xtask catalog.rs` 的过期文案**
    （`crates/xtask/src/catalog.rs:147-149` 仍写「待 REQ-014」）→ **TD-044**（D240）；
  - 步骤 1b：**通过**（真 C-5 命令，**不是** `xtask capabilities` —— 它不做 C-5）；
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
  2. 查审计表（`tenant-a`）；
  3. **敏感值扫描**（Rust 侧一个**可单独调用的 `pub fn`**，D243）：
     输入 = 审计行的 `action` / `object_type` / `object_id` / `before` / `after` 文本，
     输出 = 命中的敏感键列表；扫描面 = 审计表的**全部文本列**；
  4. **阳性对照**：管理员直连往审计表插一条**含假 secret** 的行
     （如 `before = '{"client_secret":"POSITIVE-CONTROL"}'`）→ 再扫 → **必须命中**；
     删掉该行后 → **必须不命中**。
- **预期结果**:
  - 至少产生这 5 条事件码（D215）：
    `oauth.client.registered` / `oauth.authorization.approved` /
    `oauth.token.issued` / `oauth.token.revoked` / `oauth.logout.executed`；
  - 每条含 `actor_id` / `action` / `object_type` / `object_id` / `ip` / `user_agent` /
    `created_at` / `prev_hash` / `hash`（契约 C12 的固定字段集）；
  - 步骤 3：对真实数据**0 命中**；步骤 4：阳性对照**命中**、删掉后**不命中**；
  - **敏感键集合**至少含：`client_secret` / `code` / `code_verifier` /
    `device_code` / `user_code` / `access_token` / `refresh_token` /
    `logout_token` / `authorization` / `password`；
  - **哈希链自洽**：`GET /api/audit/verify`（或 `cargo xtask audit verify`）对该租户
    返回 `ok = true`。
- **类型**: 安全

---

### AC-S-155: OAuth 数据**跨租户不可见**

- **前置条件**:
  - `tenant-a` 与 `tenant-b` 各有用户与授权记录
    （**由 `scripts/seed-e2e.sh` 播种**，D236）；
  - `crates/platform-oauth/manifest.json` 含 `tenantScoped` 段（D227）。
- **测试步骤**:
  1. 以 `tenant-a` 的上下文查 `platform_oauth_grant` / `platform_oauth_token` /
     `platform_oauth_code` / `platform_oauth_device_code`；
  2. 以 `tenant-b` 的上下文查同样的表；
  3. 直接以 `base`（超管）查全部行；
  4. 以 `base_app` 角色（**不**手动 `set_config`）查 → 再在**单个事务内**
     `set_config` → 查；
  5. **写侧**：以 `base_app` 角色，在单个事务内 `set_config('app.tenant_id','tenant-a')`
     后 `INSERT` 一行 `tenant_id='tenant-b'` → 再 `INSERT` 一行 `tenant_id='tenant-a'`；
  6. `cargo xtask tenant-policies --check`；查 `pg_policies`。
- **预期结果**:
  - 步骤 1：只看到 `tenant-a` 的行；
  - 步骤 2：只看到 `tenant-b` 的行；
  - 步骤 3：能看到**两租户**的全部行（证明步骤 1、2 不是「表本来就空」）；
  - 步骤 4：不 `set_config` → **0 行**；单个事务内 `set_config('tenant-a')` → 有行；
  - 步骤 5：插入 `tenant-b` 的行 → **失败**（RLS 的 `WITH CHECK` 拒绝）；
    插入 `tenant-a` 的行 → **成功**（D247）；
  - 步骤 6：退出码 **0**，且 `pg_policies` 里这 4 张表**各有 1** 条
    `<table>_tenant_isolation` policy（命名与 `crates/platform-db/src/rls.rs:36` 一致）（D227）；
  - **补充**：`platform_oauth_client` 是**平台级**表（无 `tenant_id`、**不启用** RLS）
    → 步骤 1、2 都**能看到**全部 client，且**不参与**步骤 5 的写侧判据（D219 / D227）。
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
