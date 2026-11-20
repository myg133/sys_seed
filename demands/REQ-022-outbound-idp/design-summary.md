# 设计摘要：出站 IdP 与单点登出（REQ-022）

- **需求编号**: REQ-022
- **状态**: rev 2（2026-11-20 —— BA 规格阶段；rev 2 同步 QA 挑刺 39 条的裁决 D221~D256）
- **配套**: [demand.md](demand.md) / [acceptance.md](acceptance.md) / [decisions.md](decisions.md)
- **基线代码**: `develop` @ `310e4d3`

> ⚠️ 本文件里的每个 `file:line` 都是**在 `310e4d3` 上实测**的（dev-guide §9.11：
> 「写规格时，凡是引用既有 API，必须打开源码确认」）。Dev 若发现行号漂移，
> **以符号名为准**，并在 PR 里注明。

---

## 1. 架构与依赖

```
                    ┌──────────────────────────────────────────┐
   浏览器 ────────▶ │  apps/shell（前端 SPA）                   │
                    │  /oauth/authorize  /oauth/device         │
                    │  /oauth/grants                           │
                    └───────────────┬──────────────────────────┘
                                    │ Bearer（platform-iam 登录令牌）
                                    ▼
   ┌────────────────────────────────────────────────────────────────┐
   │ platform-server（装配层）                                       │
   │  · nest("/api/oauth", platform_oauth::http::router(state))     │
   │  · merge(platform_oauth::http::metadata_router(state))         │
   │  · 注入 audit_sink / PermissionProvider / JwtKeySet            │
   └───────────────┬──────────────────────────────┬─────────────────┘
                   │                              │
                   ▼                              ▼
   ┌───────────────────────────────┐   ┌──────────────────────────────┐
   │ platform-oauth（**新 crate**） │   │ platform-iam（既有）         │
   │  · 5 张表 + 12 个端点         │   │  · JwtKeySet（签发密钥）     │
   │  · error.rs（10 个平台码）    │   │  · verify_access_token       │
   │  · manifest.json（6 权限点）  │   │  · revoke_family / …        │
   └───────────────┬───────────────┘   └──────────────────────────────┘
                   │
      ┌────────────┼─────────────┬──────────────┐
      ▼            ▼             ▼              ▼
 platform-core  platform-db  platform-iam   hyper/rustls
 （trait/上下文）（租户数据访问）（密钥/会话）  （back-channel 出站）
```

- **依赖方向单向，无环**：`platform-oauth → {platform-core, platform-db, platform-iam}`；
  `platform-oauth` **不依赖** `platform-rbac`（权限点用 `platform-core` 的
  `PermissionProvider` trait，`crates/platform-core/src/extension/permission.rs:239`，
  由装配层注入 —— 与 REQ-011 已有做法一致，
  `crates/platform-server/src/main.rs:395`）。
- **新增第三方依赖：0 个。**

---

## 2. 数据模型（5 张表）

> ⚠️ **rev 2（D227）**：4 张带 `tenant_id` 的表的 **RLS policy 由
> `cargo xtask tenant-policies` 生成**（manifest 的 `tenantScoped` 段是**唯一来源**，
> `crates/platform-db/src/scopes.rs:5`）→ 顺带进平台的 RLS 自检
> （`crates/xtask/src/cli.rs:114-133`）。**不重复 `platform-audit` 的缺口**
> —— 它的 4 张表是**手写** policy，因此**不在**平台自检范围内。

### 2.1 `platform_oauth_client` —— 平台级（**无 `tenant_id`，不启用 RLS**）

| 列 | 类型 | 说明 |
|---|---|---|
| `client_id` | `text` PK | 平台生成，全局唯一 |
| `client_secret_hash` | `text` NULL | SHA-256 base64 摘要；public client 为 NULL |
| `client_type` | `text` | `confidential` / `public` |
| `name` | `text` | 同意页展示名 |
| `redirect_uris` | `text[]` | 回调白名单（**精确匹配**） |
| `allowed_grant_types` | `text[]` | |
| `allowed_scopes` | `text[]` | 权限点码 |
| `backchannel_logout_uri` | `text` NULL | |
| `frontchannel_logout_uri` | `text` NULL | |
| `status` | `text` | `active` / `disabled` |
| `expires_at` | `timestamptz` NULL | NULL = 不过期 |
| `created_at` / `created_by` | `timestamptz` / `text` | |

- 回调白名单**精确匹配**（不做前缀/通配匹配）→ 防开放重定向。
- `backchannel_logout_uri` / `frontchannel_logout_uri` 注册时校验 scheme（`https`；
  本地开发允许 `http`）→ 出站 URL **只**能取自这里 → 无 SSRF 入口（D220）。

### 2.2 `platform_oauth_grant` —— 同意记录 / 撤销依据 / 登出定位（**一表三用**）
| 列 | 说明 |
|---|---|
| `id` / `tenant_id` / `client_id` / `user_id` | |
| `scopes` `text[]` | **实际授予**的 scope |
| `status` | `active` / `revoked` |
| `granted_at` / `consent_at` / `revoked_at` | |

- **UNIQUE (tenant_id, client_id, user_id)** → 再次授权 = 更新（幂等）。
- **启用 RLS**。

### 2.3 `platform_oauth_code` —— 授权码

| 列 | 说明 |
|---|---|
| `id` / `tenant_id` / `client_id` / `user_id` | |
| `code_hash` | **SHA-256 摘要**（不是明文） |
| `redirect_uri` / `scope` / `code_challenge` / `code_challenge_method` | 绑定项 |
| `expires_at` | **+60 秒** |

- **交换后即删**（删除即消费 → 天然一次性）。
- **启用 RLS**。

### 2.4 `platform_oauth_device_code` —— 设备码

| 列 | 说明 |
|---|---|
| `id` / `tenant_id` / `client_id` | |
| `device_code_hash` / `user_code_hash` | **都存摘要** |
| `scope` | |
| `status` | `pending` / `approved` / `denied` |
| `user_id` | 批准后写入 |
| `device_code_expires_at` / `user_code_expires_at` | |
| `poll_interval_seconds` / `last_polled_at` | 限流：`slow_down` |

- **启用 RLS**。

### 2.5 `platform_oauth_token` —— 令牌状态（支撑「撤销立即生效」）

| 列 | 说明 |
|---|---|
| `id` / `tenant_id` / `client_id` / `user_id` NULL | |
| `token_id` | `jti`（唯一） |
| `scopes` `text[]` | |
| `grant_id` NULL | 关联 grant（grant 被撤销 → 该 grant 下所有令牌失效） |
| `status` | `active` / `revoked` |
| `issued_at` / `expires_at` / `revoked_at` | |

- **启用 RLS**。
- 内省 = 验签 + 查本表 + 查 grant 状态（D210）。

---

## 3. 端点清单

### 3.1 协议端点（**说 RFC 的话**）

| 方法 | 路径 | 认证 | 作用 |
|---|---|---|---|
| `GET` | `/.well-known/oauth-authorization-server` | 无 | RFC 8414 元数据 |
| `GET` | `/.well-known/jwks.json` | 无 | **已存在**（`platform_iam::http::jwks_router`） |
| `POST` | `/api/oauth/authorize/preview` | 用户会话（Bearer） | 校验参数 → ticket / 免登直发 |
| `POST` | `/api/oauth/authorize/confirm` | 用户会话（Bearer） | 同意 → 签发 code |
| `POST` | `/api/oauth/authorize/deny` | 用户会话（Bearer） | 拒绝 → `error=access_denied` |
| `POST` | `/api/oauth/token` | client 认证（public 可免密） | 4 种 grant 的令牌端点 |
| `POST` | `/api/oauth/device_authorization` | client 认证 | RFC 8628 设备码端点 |
| `POST` | `/api/oauth/introspect` | **confidential** client | RFC 7662 |
| `POST` | `/api/oauth/revoke` | **confidential** client | RFC 7009 |

### 3.2 平台管理端点（**说平台的话** —— Problem Details）

| 方法 | 路径 | 权限点 | 作用 |
|---|---|---|---|
| `GET` | `/api/oauth/clients` | `oauth:client:read` | 列出 client |
| `POST` | `/api/oauth/clients` | `oauth:client:write` | 注册（**secret 只显示一次**） |
| `GET` | `/api/oauth/clients/{client_id}` | `oauth:client:read` | 读单个 |
| `PATCH` | `/api/oauth/clients/{client_id}` | `oauth:client:write` | 改 |
| `DELETE` | `/api/oauth/clients/{client_id}` | `oauth:client:delete` | 删 |
| `GET` | `/api/oauth/grants` | `oauth:grant:read` | **我的**授权记录 |
| `POST` | `/api/oauth/grants/revoke` | `oauth:grant:revoke` | 撤销授权（= 按应用登出） |
| `POST` | `/api/oauth/logout` | 用户会话 | **全局登出**（撤销全部 grant + 全部会话 + 通知） |

> ⚠️ 契约 C2「业务 API 路径形式」针对**业务模块**；平台自身端点沿用既有的
> `/api/<module>/<resource>` 形态（对照 `/api/audit/events`）。

---

## 4. 关键流程时序

### 4.1 授权码 + PKCE（AC-S-137）

```
第三方应用            浏览器(平台 SPA)          平台后端                  client 后端
    │                      │                      │                          │
    │─302 到 {issuer}/oauth/authorize?…─────────▶│                          │
    │                      │                      │                          │
    │                      │─未登录→跳 /login────▶│                          │
    │                      │─登录成功→回 /oauth/authorize                │
    │                      │                      │                          │
    │                      │─POST /authorize/preview (Bearer)────────────▶│
    │                      │                      │ 校验 client/redirect/scope │
    │                      │                      │ = 请求 ∩ client ∩ 用户权限 │
    │                      │                      │ 无 grant → 签 consent ticket│
    │                      │◀─{ticket, application, scopes, needsConsent}─│
    │                      │                      │                          │
    │                      │ 渲染同意页（逐字显示权限点码）                  │
    │                      │─POST /authorize/confirm (Bearer)────────────▶│
    │                      │                      │ 验签 ticket → 写 grant    │
    │                      │                      │ 生成 code（只存摘要）       │
    │                      │◀─{redirectTo: client/cb?code=…&state=…}──────│
    │                      │─浏览器跳转──────────────────────────────────▶│
    │                      │                      │      ◀─POST /token──────│
    │                      │                      │         (code + verifier) │
    │                      │                      │      ──▶{access_token}──▶│
```

**关键点**：
- 「授权端点」由**前端路由**承接（D203）—— 因为平台会话是 **Bearer + sessionStorage**，
  浏览器顶层导航**不会**带 Authorization 头。
- `redirectTo` 由**前端**执行跳转（`location.href`），平台**不**返回 302
  → 不涉及「redirect_uri 非法时无法重定向」的特例分支。

### 4.2 令牌交换（AC-S-137 步骤 5~6）

```
POST /api/oauth/token
Content-Type: application/x-www-form-urlencoded
Authorization: Basic base64(urlencode(client_id):urlencode(client_secret))
Body: grant_type=authorization_code&code=…&redirect_uri=…&code_verifier=…
```

处理顺序（**顺序错了就是漏洞**）：

1. 解析并校验 client 认证 → 失败 `401 invalid_client`（**不区分**「不存在」与「secret 错」）；
2. 校验 `grant_type` 允许 → 否则 `400 unsupported_grant_type`；
3. 按 `code_hash` 查授权码 → 查不到 `400 invalid_grant`；
4. 校验 `expires_at` → 过期 `400 invalid_grant`；
5. 校验 `client_id` / `redirect_uri` **与签发时逐字相同** → 否则 `400 invalid_grant`；
6. 校验 PKCE：`base64url(SHA256(code_verifier)) == code_challenge` → 否则 `400 invalid_grant`；
7. **删除授权码**（一次性）；
8. 校验 scope（若 code 里带了 scope 则须仍是有效子集）；
9. 写/更新 `platform_oauth_grant`（同意记录）；
10. 签发 JWT + 写 `platform_oauth_token`；
11. 埋审计 `oauth.token.issued`。

> ⚠️ 第 7 步必须在第 10 步**之前**，否则「签发成功但码没删」= 可重放。
> ⚠️ 第 9 步的 grant 更新必须在第 10 步**成功之后**才记 `consent_at`？——
> **不**：`consent_at` 记的是**用户点同意**的时刻（preview/confirm 时写），
> 不是发令牌的时刻。

### 4.3 免登（AC-S-148）

`preview` 时若 `prompt=none`：
- 有 **active** grant 且 `grant.scopes ⊇ 请求 scope` → `needsConsent = false`，
  **直接签发 code** → 返回 `{redirectTo}`；
- 未登录 → `login_required`；
- 已登录但无 grant / grant 不覆盖 scope → `consent_required`。

### 4.4 内省（AC-S-143）

```
POST /api/oauth/introspect      Authorization: Basic …      Body: token=…
→ 200 {"active": true, "scope": "…", "client_id": "…", "username": "…",
        "token_type": "Bearer", "exp": 1234, "iat": 1200, "sub": "…",
        "iss": "…", "aud": "…"}
→ 200 {"active": false}          ← 只有这一个键
```

判定顺序：验签 → `exp` → 查 `platform_oauth_token` → 查 grant 状态 →
**属于本 client？** → 任一不满足 → `{"active": false}`（**不报错**）。

### 4.5 登出（AC-S-149 / 150）

```
POST /api/oauth/logout   (Bearer)
1. 撤销该用户全部 grant            → platform_oauth_grant.status = revoked
2. 作废全部 refresh token          → IamStore::revoke_all_refresh_tokens
                                    （crates/platform-iam/src/store.rs:765）
   ⚠️ **只作废 refresh token** —— 已签发的**登录 access token** 是自包含 JWT
   （crates/platform-iam/src/jwt.rs:143-166 验签不查库）
   → **最长延迟一个 iam.accessTokenTtl 秒**才失效（D229 / TD-043）
   → 而 **OAuth 令牌**由 platform_oauth_token 表决定 → 撤销 grant 后**立即**失效
3. 收集有 backchannel_logout_uri 的 client → 并发 POST logout_token
   （**失败只记日志**，超时 5 秒，不影响登出结果）
4. 收集有 frontchannel_logout_uri 的 client → 返回 frontChannel 数组
5. 埋审计 oauth.logout.executed
→ 200 { loggedOut: {…}, frontChannel: [{clientId, logoutUrl, logoutToken}] }
```

**rev 2 的两处修正**：

- **D225（front-channel 的形态）**：后端返回 **JSON 数组**（如上），
  **前端**（`apps/shell/src/pages/OAuthLogout.svelte`）负责渲染 iframe：
  - iframe 数量**恰好** = `frontChannel` 数组长度；
  - `src` = `{logoutUrl}` + `?logout_token={logoutToken}`；
  - `sandbox` **含** `allow-scripts`、**不含** `allow-same-origin`。
  - ⚠️ **不是**后端直接返回 HTML —— 平台的会话是 Bearer，
    登出必须带 `Authorization` 头 → 只能由前端 `fetch` 发起 →
    浏览器**导航**无法发起 `POST`（D203 的直接推论）。
- **D230（`sid` 的取值）**：`sid` = **触发登出的这次请求所使用的会话 ID**
  （即当前 access token 的 `sid`）—— **一个**登出请求 = **一个** `logout_token`；
  「作废全部会话」是**平台侧**的动作，**不**意味着发多个 `logout_token`。

`logout_token` payload：

```json
{
  "iss": "<平台 issuer>",
  "aud": "<client_id>",
  "sub": "<用户 id>",
  "sid": "<触发登出的会话 id>",
  "events": { "http://schemas.openid.net/event/backchannel-logout": {} },
  "iat": 1700000000,
  "jti": "…"
}
```

（**不含** `nonce` —— D211）

---

## 5. 令牌与票据格式

### 5.1 access token（ES256 JWT）

| claim | 值 |
|---|---|
| `iss` | 平台 issuer |
| `aud` | **client_id** |
| `sub` | 用户 ID（`client_credentials` 时**省略该键**） |
| `tid` | 授权时的租户 ID |
| `client_id` | |
| `scope` | 空格分隔的权限点码 |
| `grant_type` | `authorization_code` / `client_credentials` / `urn:…:device_code` |
| `iat` / `nbf` / `exp` | 秒 |
| `jti` | 令牌 ID（`platform_oauth_token.token_id`） |

- **签名复用** `platform_iam::jwt::JwtKeySet`（`crates/platform-iam/src/jwt.rs:105`），
  与登录令牌**同一套密钥** → 同一个 JWKS 覆盖两种令牌。
- ⚠️ **需要改动 `platform-iam`**（D109 已声明）：
  `JwtKeySet::sign`（`crates/platform-iam/src/jwt.rs:130`）**特化到 `AccessClaims`**
  → 需新增泛型方法 `sign_claims<T: Serialize>`（旧 `sign` 内部改调它，**行为不变**）；
  `IamService`（`crates/platform-iam/src/service.rs:331`）的 `keys` 字段私有
  → 需新增 `pub fn keys(&self) -> &JwtKeySet` getter。

### 5.2 consent ticket（ES256 JWT，零新表）

| claim | 值 |
|---|---|
| `typ` | `"oauth_consent_ticket"`（**必须**校验，防令牌混用） |
| `client_id` / `redirect_uri` / `scope` / `state` | 原样回显 |
| `code_challenge` / `code_challenge_method` | 原样回显 |
| `sub` / `tid` | 用户与租户 |
| `exp` | **+5 分钟** |
| `jti` | |

- `confirm` / `deny` **只信 ticket**（不接受原始参数）→ 用户所见 = 实际授予（D204）。
- ⚠️ `typ` 必须校验：否则一个**登录令牌**（同一个密钥签的）可能被当成 ticket 用。

---

## 6. 错误码

### 6.1 平台码（**9 个**，`crates/platform-oauth/src/error.rs`）

| 码 | HTTP | 何时 |
|---|---|---|
| `OAUTH_CLIENT_NOT_FOUND` | 404 | 管理端点查不到 client |
| `OAUTH_CLIENT_DISABLED` | 403 | client 已停用（preview 阶段，D249） |
| `OAUTH_CLIENT_EXPIRED` | 403 | client 已过期 |
| `OAUTH_INVALID_REQUEST` | 400 | 参数缺失 / 非法（含缺 `code_challenge`） |
| `OAUTH_INVALID_REDIRECT_URI` | 400 | 回调地址不在白名单 |
| `OAUTH_INVALID_SCOPE` | 400 | scope 为空 / 格式非法 / 通配未注册 |
| `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` | 400 | `code_challenge_method` 不是 `S256` |
| `OAUTH_TICKET_INVALID` | 400 | consent ticket 验签失败 / 过期 / 被篡改 |
| `OAUTH_GRANT_NOT_FOUND` | 404 | 授权记录不存在 |

> ⚠️ **rev 2（D226）**：原 10 个码里的 **`OAUTH_CLIENT_SECRET_INVALID` 已删** ——
> 它在 22 条 AC 里一次都没出现，也**没有触发点**
> （D200 决定 secret 不允许自选、只显示一次，而端点清单里**没有**「轮换 secret」端点）
> → 一个**永远不可能出现**的码却要在 i18n 词库占一条、在 `TITLES` 占一行 = **死登记**。
> 将来做「轮换 secret」端点时再按 D213 的 8 处补上。

- **8 处登记点**（D213）：常量 / enum 变体 / `code()` / `status()` / `detail()` /
  `params()` / `ALL_CODES` / 测试的两个数组；
- **外加** `crates/platform-core/src/titles.rs:45` `TITLES`、
  `packages/i18n/src/locales/{zh-CN,en-US}.json` 的 `error.<CODE>`、
  `crates/platform-server/tests/i18n_titles.rs:59` 的 `all_codes()` **加一行**、
  同文件 `:156-165` 的**行数断言**同步加常量。
- ⚠️ **`TITLES` 行数 = 93**（58 + 21 + 5 + **9**）——
  **8 个 `oauth.*` 审计事件码不进 `TITLES`**（D223：进了会让 `i18n_titles.rs` 的
  三个既有断言必红，且事件码不进响应体 → 没有「面向用户的标题」这回事）。
  事件码改为 `crates/platform-oauth/src/events.rs` 里的**内部常量** + 单测覆盖。

### 6.2 协议码（小写，见 acceptance.md §全局约定）

RFC 6749 §4.1.2.1 + RFC 8628 §3.5 + RFC 7662 §2.3 的并集。

> ⚠️ **协议端点的错误体不是 Problem Details** → 需要在 api-conventions v3 里
> 新增 **C14** 明说这个例外（D196）。

---

## 7. 配置项（5 个，登记在 `crates/platform-oauth/manifest.json` 的 `config{}` 段）

| 键 | 类型 | scope | 默认 | 说明 |
|---|---|---|---|---|
| `oauth.frontendBaseUrl` | `string` | `platform` | `""` | 授权页 / 同意页的前端基址；**空 = 用 `{issuer}`**（D222） |
| `oauth.accessTokenTtlSeconds` | `number` | `platform` | `3600` | **撤销延迟上界**（D210） |
| `oauth.authorizationCodeTtlSeconds` | `number` | `platform` | `60` | |
| `oauth.deviceCodeTtlSeconds` | `number` | `platform` | `600` | |
| `oauth.deviceCodePollIntervalSeconds` | `number` | `platform` | `5` | |

- 键名 **camelCase**（契约 C9）→ `oauth.frontendBaseUrl` ✓
- **两处登记**：manifest 的 `config{}` 段 + 种子迁移里的
  `platform_config_upsert('…', …::jsonb)`
  （范式见 `migrations/20261120000400_audit_config_seed.up.sql`）。
- ⚠️ `ConfigKey::parse` 要求每段 **lowerCamelCase**
  （`crates/platform-core/src/config.rs:118-128`）→ `frontendBaseUrl` ✓
- ⚠️ **rev 2（D222）**：`oauth.frontendBaseUrl` 与 `{issuer}` **在 E2E 里不相等**
  （`https://e2e.base` vs `http://127.0.0.1:8080`）→
  一切「授权页 / 设备码页」的 URL 都用符号 **`{授权页基址}`**
  （= `oauth.frontendBaseUrl`，**未配置时回退 `{issuer}`**），**不得**直接用 `{issuer}`。

---

## 8. 权限点与 manifest

`crates/platform-oauth/manifest.json`（结构照抄 `crates/platform-audit/manifest.json`）：

```json
{
  "id": "oauth",
  "name": "出站 IdP",
  "version": "0.1.0",
  "enabled": true,
  "permissions": [
    { "code": "oauth:client:read",   "name": "查看 OAuth 客户端", "type": "menu" },
    { "code": "oauth:client:write",  "name": "管理 OAuth 客户端", "type": "button" },
    { "code": "oauth:client:delete", "name": "删除 OAuth 客户端", "type": "button" },
    { "code": "oauth:grant:read",    "name": "查看授权记录", "type": "menu" },
    { "code": "oauth:grant:revoke",  "name": "撤销授权", "type": "button" },
    { "code": "oauth:token:introspect", "name": "内省令牌", "type": "button" }
  ],
  "menus": [
    { "label": "授权管理", "icon": "key", "route": "/oauth/grants", "perm": "oauth:grant:read" }
  ],
  "tenantScoped": {
    "tables": ["platform_oauth_grant", "platform_oauth_code",
               "platform_oauth_device_code", "platform_oauth_token"],
    "column": "tenant_id"
  },
  "dependencies": [],
  "config": { … §7 的 5 个键 … }
}
```

- ⚠️ **rev 2（D227）**：加了 **`tenantScoped`** 段 →
  4 张表的 RLS policy 由 `cargo xtask tenant-policies` **生成**
  （命名自动是 `<table>_tenant_isolation`，`crates/platform-db/src/rls.rs:36`）
  → 顺带进平台的 RLS 自检（`crates/xtask/src/cli.rs:114-133`）。
  `platform_oauth_client` **不**列入（平台级表，无 `tenant_id`、不启用 RLS）。
- **种子迁移**：`migrations/20261120000600_oauth_permission_seed.up.sql`
  —— `source` 必须逐字等于 `'crates/platform-oauth/manifest.json'`
  （`crates/platform-rbac/src/registry.rs` 的 `collect_manifest_permissions` 拼法）。
- **启动对账**：`crates/platform-rbac/src/registry.rs:198` `require_consistent`
  → 不一致 = **进程起不来**（`crates/platform-server/src/main.rs:954` 调用）。
- **C-5 合规**：每个权限点都要有**字面量** `require_permission("oauth:…")` 调用点
  （`crates/platform-core/src/extension/permission.rs:273`）。
- **真 C-5 命令**（D240）：`cargo test -p platform-oauth --test contract_c5`
  —— **该测试文件必须由本需求新增**（范式见 `crates/platform-audit/tests/contract_c5.rs:24-64`）。
  ⚠️ `cargo xtask capabilities --check` **不做** C-5，且**基线上就是红的**
  （`crates/xtask/src/catalog.rs:147-149` 仍写「待 REQ-014」→ **TD-044**，本轮顺带还）。

---

## 9. 前端交付

| 文件 | 范式参考 |
|---|---|
| `apps/shell/src/pages/OAuthAuthorize.svelte` | `apps/shell/src/pages/Audit.svelte`（`$props` / `$state` / `$derived` / `$effect`） |
| `apps/shell/src/pages/OAuthDevice.svelte` | 同上 |
| `apps/shell/src/pages/OAuthGrants.svelte` | 同上（列表 + 撤销按钮） |
| `apps/shell/src/pages/OAuthLogout.svelte` | 同上（**渲染 front-channel iframe**，D225） |
| `apps/shell/src/lib/oauth-api.ts` | `apps/shell/src/lib/audit-api.ts`（纯函数 + 常量 + 白名单） |
| `apps/shell/src/lib/routes.ts` | 加 4 条路由（`createRoutes`，`:68-111`） |
| ⚠️ **`apps/shell/src/pages/Login.svelte`** | **改动**：支持 `?return_to=` 登录后回跳（D221） |
| ⚠️ **`apps/shell/src/App.svelte`** | **改动**：登录后走 `return_to`（D221） |

- ⚠️ **rev 2（D221）—— 未登录回跳**：
  - `/oauth/authorize` 登记为**公开路由**（**不设** `requiresAuth`），
    照 `apps/shell/src/lib/routes.ts:70-76` 的 `SSO_CALLBACK_ROUTE_PATH` 范式；
  - 页面自己读 `window.location.search` 取全部授权参数；
  - 未登录 → 渲染「登录并继续」→ 把**完整 URL**（`pathname + search`）编码进
    `?return_to=` → 跳 `/login?return_to=…`；
  - `Login.svelte` 登录成功后 `location.replace(return_to)`；
    **没有** `return_to` 时保持现行为（跳首页）。
  - **不动** `apps/shell/src/lib/router.ts` —— 改它要动 REQ-011 已交付的路由核心
    （`resolve()` 用带 query 的字符串 `match()` 会失败，`router.ts:204-207`）。
  - ⚠️ 改 `Login.svelte` / `App.svelte` 会影响 REQ-011 的既有前端用例 → **棒 2 必须跑前端全量回归**。
- i18n 键：`packages/i18n/src/locales/{zh-CN,en-US}.json` 新增
  `oauth.*` 命名空间 + **9 个** `error.OAUTH_*`（D226）。
  ⚠️ **整个 `packages/i18n/**` 归棒 2**（D256 —— 两个棒都改同一文件 = 并行冲突）。
- 组件用**本地** `apps/shell/src/components/`（`Button` / `Input` / `Table` / `Card` / `Dialog`）。

---

## 10. 装配路径（`crates/platform-server/src/main.rs` 改哪几处）

| # | 位置 | 改什么 |
|---|---|---|
| 1 | `:289` `Router::new()` 后 | 新增 `let oauth_state = build_oauth_state(...).await?;` |
| 2 | `:330` 附近 | `app = app.nest("/api/oauth", platform_oauth::http::router(oauth_state.clone()))` |
| 3 | `:331` 附近 | `app = app.merge(platform_oauth::http::metadata_router(oauth_state.clone()))` |
| 4 | IAM 就绪后（`:337` 之后） | 注入 `PermissionProvider`（RBAC 的 `RbacService`）+ `audit_sink` |
| 5 | 新函数 `build_oauth_state` | 参照 `build_iam_state`（`:596`）与 `build_org_state`（`:630`） |

- **装配顺序**：`platform-oauth` 依赖 IAM 的密钥 → 必须在 `IamConfig::is_configured`
  分支内（`crates/platform-server/src/main.rs:327`）。
- IAM 没配 → **不提供** `/api/oauth/*`（与 `/api/org/*` 同款口径，`:346-351`）。

---

## 11. ⚠️ 命名禁区（C-5，D216）

`crates/platform-oauth/src/` 里**不得**出现这 8 个标识符
（`crates/platform-contracts/src/authz_scan.rs:43` `AUTH_ENTRY_ALIASES`）：

```
check_permission   check_perm     has_permission   require_perm
authorize          ensure_permission  verify_permission  permission_check
```

- 本需求的令牌校验函数**必须**用别的名字：
  `authenticate_bearer` / `authenticate_client` / `verify_client_secret`。
- ⚠️ `authorize` 是最容易踩的一个（OAuth 语境下太自然）。

---

## 12. 测试落点

| 层 | 位置 | 备注 |
|---|---|---|
| 单元 | `crates/platform-oauth/src/**` 的 `mod tests` | scope 交集、PKCE 校验、client 认证、事件码常量 |
| 集成 | `crates/platform-oauth/tests/*.rs` | 真库（`DATABASE_URL_TEST`）+ 真 HTTP（axum `oneshot`） |
| 契约 | `crates/platform-oauth/tests/contract_c5.rs` | **真 C-5 命令**（D240）；范式见 `crates/platform-audit/tests/contract_c5.rs:24-64` |
| 跨栈 | `crates/platform-server/tests/i18n_titles.rs` | 错误码 / TITLES（**行数 = 93**）/ i18n |
| 能力索引 | `cargo xtask capabilities`（**重新生成**，不是 `--check`） | `capabilities/` 仍 **14 个文件**；`permissions.json` 的 `total == 32`（D240） |
| RLS | `cargo xtask tenant-policies --check` | 4 张表的 policy 由 manifest 的 `tenantScoped` 生成（D227） |
| 前端单测 | `apps/shell/test/oauth*.test.ts` | `svelte/server` 的 `render` |
| E2E | `e2e/oauth.spec.ts` | Playwright；夹具由 `scripts/seed-e2e.sh` 播种 |

**夹具**（`scripts/seed-e2e.sh` 要加，D236 —— **全部**是脚本的产出物，AC 前置必须写明）：

| 夹具 | 说明 |
|---|---|
| `u-perm-oauth@example.com`（`tenant-a`） | OAuth 管理员：持 `oauth:client:read/write/delete` + `oauth:grant:read/revoke` |
| `e2e-user-oauth@example.com`（`tenant-a`） | 普通用户：持 `rbac:role:read` |
| `u-perm-other@example.com`（**`tenant-b`**） | 跨租户用例（D236 / S-34） |
| `client-A`（confidential） | `allowedScopes = ["rbac:role:read"]`；配 `backchannel_logout_uri` + `frontchannel_logout_uri` |
| `client-B`（confidential） | **不**配 front-channel URI（验收集逻辑按非空过滤） |
| `public-client`（public） | 无 secret |

- client 的 `client_secret_hash` 由脚本按 **`digest_base64url`** 口径写入，
  **明文 secret 由脚本打印到 stdout** → E2E 从 stdout / 夹具文件读；
- 脚本的「强断言」段必须**自证**这些账号与 client **存在且绑定正确**。

**mock 接收端**（D237 —— **独立进程**，不是「进程内」）：

- 新增 `scripts/mock-logout-receiver.sh`：监听 `127.0.0.1:$MOCK_PORT`，
  把收到的 `logout_token` **写到文件** `target/mock-logout-received.jsonl`，
  行为由 `MOCK_MODE=ok|timeout|500` 环境变量控制（默认 `ok`）；
- `scripts/seed-e2e.sh` 读 `$MOCK_PORT`，把
  `backchannel_logout_uri = 'http://127.0.0.1:' || $MOCK_PORT || '/logout'` 写进 client 行
  → **夹具顺序钉死**：先起 mock → 再播种；
- ⚠️ **不要**在 E2E 进程里 mock HTTP —— 那会把「出站真的发出去了」这件事 mock 掉
  （dev-guide §9.13）。

**E2E 私钥落盘**（D238）：

- `scripts/e2e-server.sh:65-70` 改成「生成后**写入** `target/e2e-jwt-private.pem` 并 `export`」；
- 权限 **0600**、**不入 Git**、**每次运行覆盖**；
- E2E 用它**自己签**一个 `scope` 更大的 consent ticket → 确认被拒（D238 的核心判据）。

> ⚠️ **E2E 必须从干净库跑**（REQ-014 F11 教训）：`scripts/e2e-server.sh` **不能从零建库**
> （TD-039）→ 清理后必须手工补 3 步（`deploy/seed/02-app-role.sql` +
> `modules/sandbox/migrations/` + `xtask tenant-policies`）。

---

## 13. 迁移与文件清单

| 文件 | 棒 | 动作 |
|---|---|---|
| `Cargo.toml` | 1 | `members` 加 `crates/platform-oauth`；`[workspace.dependencies]` 加一行 |
| `Cargo.lock` | 1 | 同步（REQ-021 的教训：**必须**入锁） |
| `crates/platform-oauth/` | 1 | 新 crate：`Cargo.toml` / `manifest.json` / `src/{lib,error,events,model,store,service,http,client_auth,pkce,ticket,logout,outbound,sensitive}.rs` |
| `crates/platform-oauth/tests/contract_c5.rs` | 1 | **C-5 真命令**（D240） |
| `crates/platform-iam/src/jwt.rs` | 1 | 加 `sign_claims<T>`（D109 #1） |
| `crates/platform-iam/src/service.rs` | 1 | 加 `keys()` getter（D109 #1） |
| `crates/platform-server/src/main.rs` | 1 | 装配（§10） |
| `crates/platform-core/src/titles.rs` | 1 | 加 **9** 条 ASCII 标题 |
| `crates/platform-server/tests/i18n_titles.rs` | 1 | `all_codes()` 加一行 + 行数断言改 **93** |
| `crates/xtask/src/catalog.rs` | 1 | 修过期文案「待 REQ-014」（**TD-044**，D240） |
| `packages/i18n/src/locales/{zh-CN,en-US}.json` | **2** | 加 `error.OAUTH_*`（**9** 个）+ `oauth.*`（D256） |
| `packages/i18n/test/i18n.test.ts` | **2** | 加本需求的 9 个码的登记断言 |
| `apps/shell/src/pages/OAuth{Authorize,Device,Grants,Logout}.svelte` | **2** | 4 个页面 |
| `apps/shell/src/pages/Login.svelte` | **2** | **改**：支持 `?return_to=`（D221） |
| `apps/shell/src/App.svelte` | **2** | **改**：登录后走 `return_to`（D221） |
| `apps/shell/src/lib/oauth-api.ts` | **2** | API 层 |
| `apps/shell/src/lib/routes.ts` | **2** | 加 4 条路由 |
| `migrations/20261120000500_oauth_schema.up/down.sql` | 1 | 5 张表 + 索引（**RLS 由 xtask 生成**，D227） |
| `migrations/20261120000600_oauth_permission_seed.up/down.sql` | 1 | 6 个权限点 |
| `migrations/20261120000700_oauth_config_seed.up/down.sql` | 1 | 5 个配置键 |
| `scripts/seed-e2e.sh` | 1 | 加 OAuth 夹具（D236） |
| `scripts/mock-logout-receiver.sh` | 1 | **新增**（D237） |
| `scripts/e2e-server.sh` | 1 | 私钥落盘（D238） |
| `e2e/oauth.spec.ts` | 1 | E2E |

> ⚠️ **棒 1 ∥ 棒 2 的文件集严格不相交**（D256）→
> `packages/i18n/**` 与 `apps/shell/**` 归**棒 2**；其余归**棒 1**；
> `Cargo.toml` / `Cargo.lock` 只归棒 1。

---

## 14. ⚠️ 已识别的风险

| # | 风险 | 处置 |
|---|---|---|
| R1 | **JWT 的撤销延迟** —— 只验签的第三方最长延迟 3600 秒 | 写进元数据 + REQ-016 的 AC-S-024 改写（D210） |
| R2 | **底座业务端点不接受 OAuth 令牌** —— 第三方想「一个令牌走天下」做不到 | D209 + TD-041；本轮**有意**不做 |
| R3 | `platform_oauth_client` 是**平台级**（无 `tenant_id`）→ 所有租户共享同一批 client | D219；这是 OAuth 的常态（client 是「应用」不是「租户资产」） |
| R4 | consent ticket 无状态 → 5 分钟内可**重复** confirm（第二次因码已消费而失败） | 可接受；AC-S-146 断言 |
| R5 | 新增 crate 增加编译时间（低配机敏感） | `jobs = 3` 不变；依赖 0 新增 |
| R6 | back-channel 出站 HTTP **超时**拖慢登出 | 超时 5 秒 + 并发投递 + **失败不影响登出** |
| R7 | 改 `platform-iam` 会碰到 REQ-013 的回归 | 改动**只有 2 个新增方法**，不改 `AccessClaims`；全量回归把关 |
| R8 | 契约 v3 升版要同步 `docs/contracts/` 快照 | `scripts/sync-contracts.sh --sync`（既有脚本） |
| **R9** | **未登录回跳丢 query** —— 平台的 `intendedPath` 只有 pathname | **D221**：公开路由 + 登录页 `return_to`；**不动** `router.ts` |
| **R10** | **改 `Login.svelte` / `App.svelte` 会影响 REQ-011 的既有前端用例** | 棒 2 必须跑**前端全量回归**；QA 用例已覆盖回跳路径 |
| **R11** | **登出不杀已签发的登录 access token** | **D229** 显式写明现状 + **TD-043**；本轮**不**补实现 |
| **R12** | **`xtask capabilities --check` 基线就是红的** | **D240** + **TD-044**；本轮顺带还 `catalog.rs` 的过期文案 |
