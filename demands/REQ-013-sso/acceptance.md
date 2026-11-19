# 验收标准：SSO 入站（OIDC + LDAP）与强制 SSO

- **需求编号**: REQ-013
- **父需求**: REQ-001
- **状态**: 已就绪（**rev 3**）
- **更新时间**: 2026-11-20 00:35
- **AC 数量**: **12 条**（AC-S-020 / AC-S-021 重写 + AC-S-100 ~ AC-S-109）
- **rev 3 变更**: QA 挑刺 **51 条全部接受**（阻塞 14 / 高 13 / 中 16 / 低 8）。逐条修订记录见每节末尾的「rev 3 修订」

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。

## 承接的父 AC

**AC-005（SSO 入站三协议）** —— ⚠️ **本轮收窄为两条协议**

| 父 AC-005 要求的协议 | 本轮 | 说明 |
|---|---|---|
| OIDC / OAuth2 | ✅ 交付 | AC-S-100 ~ AC-S-103 |
| LDAP / AD | ✅ 交付 | AC-S-104 / AC-S-105 |
| **SAML 2.0** | ❌ **不交付** | 树里无 XML 库；XMLDSig + C14N 手写验签是 XSW 攻击重灾区。**用户 2026-11-19 裁定** |
| CAS | ❌ **不交付** | CAS 2.0 断言是 XML，同受限制 |

**AC-006（出站 IdP）** → **移到 REQ-022**，本轮不承接。

---

## 全局约定（适用于全部 12 条）

### G1 依赖预算

`ldap3` / `hyper` / `hyper-util` / `jsonwebtoken` / `base64` / `sha2` **均已在树里**（只加 `[dependencies]` 引用，不进锁文件）。

**唯一破例是 TLS（D123，用户 2026-11-19 批准）**：`rustls` + `ring` + `hyper-rustls` 会**新增锁文件条目**
（S42：预计 20~30 个包，含 `ring` 的 C/汇编构建）。
- **包数增量、首次编译耗时、低配机（`jobs = 3`）可编性必须实测报告**（基线 **296**）
- 测试用的服务端 TLS（`tokio-rustls` / `axum-server`）**只进 `[dev-dependencies]`**，不改生产依赖树（D124 决策②）
- **不得引入 `reqwest`**

### G2 新增错误码要改 **8 处**（D125）

不是「两处」。新增一个 `IamError` 变体必须逐项核对：

| # | 位置 | 漏了会怎样 |
|---|---|---|
| 1 | code 常量 | 编译错 |
| 2 | enum 变体 | 编译错 |
| 3 | `code()` 的 match | 编译错（非穷尽） |
| 4 | `detail()` 的 match | 编译错（非穷尽） |
| 5 | `ALL_CODES` 数组 | `ALL_CODES 缺少 … 的码` 失败 |
| 6 | `platform-core/src/titles.rs` 的 `TITLES` | `titles_cover_every_code` 失败 |
| 7 | `error.rs` 测试的 `variants` 数组 | 「清单里有码从未产出」失败 |
| 8 | `error.rs` 测试的 `errors` 数组 | 「detail 非 ASCII」失败 |

**本轮新增 21 个码** → `TITLES.len()` 期望值从 **56** 变为 **77**。

### G3 其它

- 错误 `title` 必须 ASCII（D113）；中文文案只在前端语言包
- 时间一律 UTC 落库；时区只在前端渲染（D114）
- 测试只用 `base_test` 库，应用角色 `base_app`，绝不写 `base_dev`
- **敏感值 / 静态扫描必须带阳性对照**（REQ-021 教训）
- 日志类用例执行前 **`unset RUST_LOG`** —— 生产入口的 `log_filter` 优先读 `RUST_LOG`

---

## 端点与可执行前提（供全部 AC 引用）

### P1 配置读取的真实形状（修 S1/S2/S3）

`GET /api/config/entries` **必须带 `keys`**（逗号分隔）；不给 → `{"entries":[]}`。
且**只要 `keys` 里含任何 `scope: platform` 的键，整请求 403**（`PlatformKeyOnGenericRoute`）。

→ **本需求一律「按精确键名读那 12 个 SSO 键」**。**「全量导出配置」是不存在的能力**。

### P2 阳性对照用哪个键（修 S3）

配置键**强制登记**（D53），未登记键写入 404。
→ 阳性对照用**已登记且无 `format`** 的既有键 `org` 模块的 `ldapBaseDn`（`string`/`platform`/无 format），写完清理。**不得为测试新增 manifest 键**。

### P3 回调的租户来源（修 S12）

浏览器从 IdP 跳回 `/callback` **带不上 `x-tenant-id`**；`/api/iam` 的 `tenant_context` 缺头时把租户归一化为 `None`。

→ **规则**：请求头**有**租户 → 必须与 `state.tenant_id` 相等，不等 → `400 SSO_OIDC_STATE_INVALID`；
请求头**无**租户 → **采纳 `state.tenant_id`** 并据此构造 `RequestContext`；**session 落库必须带该租户**。

### P4 回调的浏览器交接（修 S27，D124 决策①）

回调是**顶层浏览器导航**落点，返回 JSON 会被渲染成文本，SPA 拿不到 `refreshToken`。

```
GET  /api/iam/sso/oidc/callback?code&state
     → 校验 + 建会话 → 生成一次性 loginTicket（TTL 30 秒）
     → 302 到 {前端基址}/sso/callback?ticket=<ticket>

POST /api/iam/sso/exchange   body {ticket}
     → 校验（存在/未用/未过期/租户匹配）→ 消费 → 200 LoginResponse
```

**ticket 一次性、短命、绑租户。不得把 token 放进 URL。**

**失败分支（rev 3 补充，前端 Dev 指出）**：回调**失败**时也必须 302 到前端，并带上错误码：

```
callback 失败 → 302 {前端基址}/sso/callback?error=<ERROR_CODE>
```

否则用户从 IdP 跳回来只看到浏览器里的一个 JSON 错误页，**连登录页都回不去**。
`error` 只放**错误码**（如 `SSO_OIDC_STATE_INVALID`），不放 `detail`、不放任何配置值。
前端据此渲染本地化文案（AC-S-089 的错误码渲染模式）。

### P15 authorize 的租户来源（rev 3 补充，前端 Dev 发现的跨端缺口）

`GET /api/iam/sso/oidc/authorize` 是**顶层浏览器导航**的起点 ——
与回调同一个问题：**浏览器无法给整页导航设置请求头**。
而 authorize 此刻**还没有 `state`** 可反查租户（P3 的办法在这里用不了）。

→ **规则**：

| 来源 | 优先级 | 用法 |
|---|---|---|
| 查询参数 `?tenant=<id>` | **1** | **顶层导航唯一可用的通道** —— 前端在跳转前把当前租户拼进 URL |
| 请求头 `x-tenant-id` | 2 | 脚本客户端 / 测试可用 |
| 都没有 | — | `401 TENANT_CONTEXT_MISSING`（复用既有码，见下） |

**为什么不新增错误码**：`TENANT_CONTEXT_MISSING` 的语义就是「租户上下文缺失」，
现有 HTTP 码是 **401**。authorize 缺租户是同一个问题，**不为它专造第 22 个码**
（每加一个码要改 8 处，见 G2）。

**安全说明（与 AC-S-109 步骤 8 一致）**：租户 id 出现在 URL 里是**可接受的** ——
① 登录页本来就要求用户**手输**租户（D98），它不是秘密；
② AC-S-109 已承认「租户/provider 存在性对未认证调用者可见」（登录页需要）；
③ **不得**因此让 URL 或响应携带 `issuer` / `clientId` / LDAP `url` / `baseDn`。

**同样适用于 LDAP**：`POST /api/iam/sso/ldap/login` 是 XHR（不是整页导航），
走 `x-tenant-id` 头即可，**不需要** `?tenant=`。

### P5 算法白名单是硬编码（修 S34）

**删掉「可由配置收窄」**（§7 没有这个键）。白名单**硬编码** `{RS256, ES256}`。

### P6 LDAP 匹配顺序（修 S21）

**先查 `(provider, external_id)` 绑定；命中即用；未命中再按 `mail` 匹配；匹配到他人已绑定账号 → `409`。**

### P7 绑定不可改指（修 S19）

既有 `upsert_external_identity` 是 `ON CONFLICT … DO UPDATE SET user_id`（**改指**）。组织同步合理，**认证路径绝对不行**。
→ 认证路径用 **`ON CONFLICT DO NOTHING` + 读回比对**；不一致 → `409 SSO_IDENTITY_LINK_CONFLICT`。

### P8 JIT 建号规则（修 S33/S20）

- `username` = email 的 `@` 前部分；本租户已存在 → `409 SSO_IDENTITY_LINK_CONFLICT`（**不接管既有账号**）
- `password_hash` = **永不匹配**的固定 PHC 串（占位，满足 NOT NULL）
- **并发幂等**：单事务内 `INSERT … ON CONFLICT (tenant_id, email) DO NOTHING` → 重查 → 绑定 `DO NOTHING` + 读回比对。
  **并发下允许返回 `409`，但 `5xx` 必须为 0**

### P9 TTL 用常量 + 测试改 DB（修 S26）

`state` TTL 与 `ticket` TTL **不新增配置键**。TTL 是**代码常量**（state 300 秒、ticket 30 秒）；
测试通过 **DB 侧 `UPDATE … SET expires_at=`** 制造过期。

---

# AC-S-020: 协议失败可诊断 —— 两类协议的失败给出**可区分**的错误码

- **类型**: 功能 / 可观测性

### 前置条件

- `tenant-a` 已配 OIDC（`issuer` 指向可控 mock IdP）与 LDAP（可控 mock 目录）
- `GET /api/iam/sso/providers` 返回 2 个 provider

### 测试步骤

1. OIDC 的 `issuer` 改成返回 **500** 的地址 → `GET /api/iam/sso/oidc/authorize`
2. OIDC 的 `issuer` 改成返回**非法 JSON** 的地址 → 再发起一次
3. 用伪造 `code` 打 `/callback?code=bogus&state=<有效state>`
4. mock IdP 的 token 端点返回 **503** → 重放一次合法 `code`+`state`
5. LDAP 的 `url` 改成**未监听端口** → `POST /api/iam/sso/ldap/login`
6. 用**错误密码**对存在用户做 LDAP 登录
7. 对**不存在的用户**做 LDAP 登录（密码随便）

### 预期结果

1. 步骤 1 → `503` `SSO_OIDC_DISCOVERY_FAILED`
2. 步骤 2 → `502` `SSO_OIDC_DISCOVERY_INVALID`（与步骤 1 可区分）
3. 步骤 3 → `400` `SSO_OIDC_TOKEN_EXCHANGE_FAILED`
4. 步骤 4 → **`502` `SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE`** —— 与步骤 3 的 `400` **必须不同**
5. 步骤 5 → `503` `SSO_LDAP_UNREACHABLE`
6. 步骤 6 → `401` `SSO_LDAP_BIND_FAILED`；响应体**不含**用户 DN、不含目录错误原文
7. 步骤 7 → 与步骤 6 **完全一致**（同码同 title，防账号枚举）
8. **上述 6 个不同的码**全部出现在 `ALL_CODES` 与 `TITLES` 中，且 `TITLES` 的 `title` 为 ASCII
9. `i18n_titles.rs` 的完备性测试通过

> **rev 3 修订**：
> - **S10**：rev 2 说「6 个」但只列了 5 个。rev 3 因 D124 决策④新增 `TOKEN_ENDPOINT_UNAVAILABLE`，现**真的是 6 个**，逐一点名
> - **S24**：与 AC-S-109 的粒度冲突 → 见下方统一判据

### 与 AC-S-109 的统一灰度判据（修 S24）

| 场景 | 处理 | 理由 |
|---|---|---|
| **远端基础设施故障**（连不上 / 非法响应 / token 端点挂） | **区分** `503`/`502` | 运维需要知道「谁的锅」 |
| **租户 SSO 配置状态**（未配 / 已禁用） | **合并**为 `SSO_PROVIDER_NOT_CONFIGURED` | 对未认证调用者不暴露内部状态 |
| **凭据相关**（密码错 / 用户不存在） | **合并**同一个码 | 防账号枚举 |
| **配置值本身**（issuer / clientId / secret） | **永不**出现在任何响应 | AC-S-106 |

---

# AC-S-021: 强制 SSO 生效 —— 本地密码登录被拒，且**不能用于账号枚举**

- **类型**: 安全

### 前置条件

- `tenant-a` 配好 OIDC + LDAP，`sso.enforce = true`，租户内有本地账号 `alice`
- `tenant-b` 未配 SSO，且 **`tenant-b` 有自己的 `alice`（密码与 a 不同）**
- `tenant-a` 内**不存在** `nobody`

### 测试步骤

1. `tenant-a`：`alice` + **正确密码** → `POST /api/iam/login`
2. `tenant-a`：`alice` + **错误密码** → 同一接口
3. `tenant-a`：**不存在的** `nobody` → 同一接口
4. `tenant-b`：`alice` + 正确答案 → 同一接口
5. 步骤 1~3 的响应体；并统计**本进程内 Argon2id 的调用次数**
6. `tenant-a` 走 OIDC 登录，断言仍成功
7. `sso.enforce` 改回 `false`，再打 `POST /api/iam/login`
8. 把 SSO 配置读取路径打成失败，`sso.enforce = true` 状态下再打 `POST /api/iam/login`

### 预期结果

1. 步骤 1 → `403` `SSO_REQUIRED`，`params.ssoProviders` 是可用 provider 列表（**写死在 `params`**）
2. 步骤 2、3 → 与步骤 1 **响应体完全一致**
3. 步骤 5 → **Argon2id 调用次数 == 0**；**对照组**：`enforce=false` 时同一批请求 **> 0**
   —— **S15**：原判据「耗时同量级 1.5×」**没有区分力**（正确实现下三条都走同一早退路径，即使跑 Argon2id 也同量级）
4. 步骤 4 → `200`（`enforce` 是租户级）
5. 步骤 6 → OIDC 登录成功，响应同构
6. 步骤 7 → `200`，本地登录恢复
7. 步骤 8 → **`403 SSO_REQUIRED`（fail-closed）** —— **S16**：配置读不出来**绝不能**当 `false`，
   否则强制 SSO 静默失效；同时断言有 warn 级以上可观测行
8. **实现位置判据**（替代原「代码位置断言」，S38）：`nobody` 返回 `403 SSO_REQUIRED` 而非 `401 INVALID_CREDENTIALS`
   —— 若先查用户再拒，不存在用户会走「查无此人」分支

---

# AC-S-100: OIDC 授权码流程端到端

- **类型**: 功能

### 前置条件

- mock IdP 就绪，暴露 `/.well-known/openid-configuration`、`/authorize`、`/token`、`/jwks`
- `tenant-a` 配好 `sso.oidc.issuer` / `clientId` / `redirectUri` / `scopes`
- `bob` 在 IdP 侧存在（`sub = "idp-bob-001"`、`email = bob@example.com`），已绑定到本地 user
- 前端基址已配置（供 P4 的 302 目标）

### 测试步骤

1. `GET /api/iam/sso/oidc/authorize?tenant=tenant-a`（**查询参数**，模拟顶层导航 —— 见 P15），**不跟随跳转**
2. 解析 302 的 `Location` query
3. 模拟 IdP 授权完成 → `GET /callback?code=<mock code>&state=<步骤1的state>`
   （**不带** `x-tenant-id` —— 见 P3）
4. 取步骤 3 的 `302 Location` 里的 `ticket`，打 `POST /api/iam/sso/exchange {ticket}`
5. 用 `accessToken` 打 `GET /api/rbac/me/permissions`
6. 用 `refreshToken` 打 `POST /api/iam/refresh`
7. 打 `GET /api/iam/sessions`
8. **再次**用**同一个 ticket** 打 `/sso/exchange`
9. 把 mock IdP 换成 **`https://`（自签证书 + 测试根，见 P10）**，重跑步骤 1~7

### 预期结果

1. 步骤 1 → `302`，`Location` 指向发现文档的 `authorization_endpoint`
2. 步骤 2 → query **必含** `response_type=code`、`client_id`、`redirect_uri`（与配置逐字相同）、
   `scope`（含 `openid`）、`state`、`nonce`、`code_challenge`、`code_challenge_method=S256`。
   `state` 与 `nonce` 满足 **`^[A-Za-z0-9_-]{43,}$`** 且连续 20 次**不重复**（替代「高熵」空话，S38）
3. 步骤 3 → **`302`** 到 `{前端基址}/sso/callback?ticket=…`（**不是 200+JSON**，P4/S27）
4. 步骤 4 → `200`，`LoginResponse` 与本地登录**键集合相同**，取值语义一致：
   `tokenType == "Bearer"`、`expiresIn` 是**正整数**、`permissions` **恒为数组**（S48）
5. 步骤 5 → `200`，返回 `bob` 的真实权限点（非空）
6. 步骤 6 → `200`，返回新 access token
7. 步骤 7 → 列表含本次 SSO 会话（来源字段见 AC-S-107）
8. 步骤 8 → `401 SSO_TICKET_INVALID`（**ticket 一次性**）
9. 步骤 9 → **同样全部成功**（D123 的直接验证点）
10. **不带 `?tenant=` 也不带 `x-tenant-id`** 打 authorize → `401 TENANT_CONTEXT_MISSING`（P15）
11. 回调失败（如把 `state` 改成无效值）→ **`302` 到 `{前端基址}/sso/callback?error=SSO_OIDC_STATE_INVALID`**，
    且 `error` 值**只含错误码**（P4 失败分支）

> **rev 3 修订**：
> - **S12**：回调租户来源矛盾 → P3
> - **S27**：回调返回 JSON 在真实浏览器不可用 → P4
> - **S13**：`with_webpki_roots()` 不信任自签根 → P10
> - **S14**：https mock IdP 需服务端 TLS → 只进 `[dev-dependencies]`
> - **S51**：原预期编号错乱（1,2,3,**8**,4,5,6）且无对应步骤 → 已重排，https 变成真步骤 9

### P10 https 的测试根机制（修 S13/S14）

- **生产**：`with_webpki_roots()`（公网可信根）
- **测试**：`hyper_rustls` 可注入 `rustls::ClientConfig` → 用 `with_custom_root(测试根 CA)` 构造
- **mock IdP 服务端 TLS**：`axum-server` 或 `tokio-rustls` 包一层，**只进 `[dev-dependencies]`**
- **断言**：https 与 http 两种 issuer **都必须能用**（`https_or_http()`），不是二选一

---

# AC-S-101: `state` / `nonce` / PKCE 防 CSRF 与重放

- **类型**: 安全

### 前置条件

同 AC-S-100；`oidc_state` 表可被测试以管理员连接读写

### 测试步骤

1. 发起授权拿 `state` → 回调一次（成功）→ **再用同一个 `state`** 回调
2. 发起授权 → **DB 侧 `UPDATE oidc_state SET expires_at = now() - interval '1 second'`** → 回调
3. `tenant-a` 发起的 `state`，带 `x-tenant-id: tenant-b` 回调
4. **DB 侧 `UPDATE oidc_state SET provider = 'oidc-other'`** → 回调
5. mock 返回的 `id_token.nonce` 改成不匹配值 → 回调
6. **DB 侧 `UPDATE oidc_state SET code_verifier = '<另一个合法 verifier>'`** → 回调
   （mock IdP 的 `/token` 做**真实** PKCE 校验）
7. 回调**不带** `state`
8. 回调**不带** `code`
9. 查询 `oidc_state` 表

### 预期结果

1. 步骤 1 第二次 → `400 SSO_OIDC_STATE_INVALID`
2. 步骤 2 → `400 SSO_OIDC_STATE_EXPIRED`（与「无效」可区分）
3. 步骤 3 → `400 SSO_OIDC_STATE_INVALID`
4. 步骤 4 → `400 SSO_OIDC_STATE_INVALID`（`state` 绑 provider）
5. 步骤 5 → `401 SSO_OIDC_NONCE_MISMATCH`
6. 步骤 6 → `400 SSO_OIDC_PKCE_MISMATCH`
7. 步骤 7 → `400 SSO_OIDC_CALLBACK_PARAM_MISSING`
8. 步骤 8 → `400 SSO_OIDC_CALLBACK_PARAM_MISSING`
9. 步骤 9 → 已用记录 `consumed_at` 非空（或已删除）；**不存在同一 `state` 两条有效记录**

> **rev 3 修订**：
> - **S4**：原步骤 4「用 OIDC 的 `state` 打 **LDAP** 登录接口」**物理无意义**（LDAP body 只有 `{username,password}`）→ 改 DB 侧改 `provider`
> - **S5**：原步骤 6「回调时换 `code_verifier`」**客户端做不到**（verifier 在服务端）→ 改 DB 侧改 + mock 做真实 PKCE 校验
> - **S26**：原「等过 TTL（测试可配）」但**没有 TTL 配置键** → P9
> - **S31**：缺参数没有独立码 → 新增 `SSO_OIDC_CALLBACK_PARAM_MISSING`

---

# AC-S-102: `id_token` 验签 —— 拒绝 `alg: none`、算法混淆、**外部公钥注入**

- **类型**: 安全

### 前置条件

mock IdP 可返回**自定义构造**的 `id_token`（测试专用开关）；测试可起一个**攻击者 JWKS 端点**

### 测试步骤

1. 正常 `RS256` 签名，`iss` 改成别的值
2. 正常签名，`aud` 改成别的 client_id
3. 正常签名，`exp` 设为过去
4. `alg = "none"`、去掉签名
5. `HS256` 签名，密钥取 IdP **公钥内容**
6. `RS512`（白名单只开 `RS256`/`ES256`）
7. 签名正确但 `kid` 是**新的未知** `kid`（mock 轮换密钥）
8. `payload` 改 `email` 但保留原签名
9. **`header.jku` 指向攻击者控制的 JWKS 端点**
10. **`header.x5c` 内嵌攻击者公钥并签名**
11. 读攻击者 JWKS 端点的**被请求次数**

### 预期结果

1. 步骤 1 → `401 SSO_OIDC_TOKEN_INVALID`
2. 步骤 2 → `401 SSO_OIDC_TOKEN_INVALID`
3. 步骤 3 → `401 SSO_OIDC_TOKEN_EXPIRED`
4. 步骤 4 → `401 SSO_OIDC_ALG_NOT_ALLOWED`
5. 步骤 5 → `401 SSO_OIDC_ALG_NOT_ALLOWED`（**算法混淆必须被拒**）
6. 步骤 6 → `401 SSO_OIDC_ALG_NOT_ALLOWED`
7. 步骤 7 → **登录成功**（未知 `kid` 触发 JWKS 刷新）
8. 步骤 8 → `401 SSO_OIDC_TOKEN_INVALID`
9. 步骤 9 → `401`，**且步骤 11 的攻击者端点被请求次数 == 0**（从未去那个地址取公钥）
10. 步骤 10 → `401 SSO_OIDC_TOKEN_INVALID`（不采信 `x5c`）
11. 每个被引用的码都在 `TITLES` 里且 `title` 为 ASCII

> **rev 3 修订**：
> - **S17**：design 明写「不从 `x5c`/`jku` 取公钥」，但原 AC **无一守它** → 新增步骤 9/10/11
> - **S45**：原「全部 5 类失败」与任何分法都对不上 → 删掉计数

---

# AC-S-103: 账号绑定与 JIT 建号 —— 不自动建号、不改指、不绕过禁用、并发幂等

- **类型**: 功能 / 安全

### 前置条件

- `sso.jitProvisioning` 先设 `false`
- IdP 有 `sub = "idp-carol-001"` / `carol@example.com`，底座**无**绑定、**无**该账号
- `sub = "idp-dave-001"` 绑定到本地 `dave`，`dave` 已**禁用**
- 另有一条绑定：`sub = "idp-eve-001"` → 本地 `eve`

### 测试步骤

1. JIT 关闭，`carol` 走 OIDC 登录
2. 打开 JIT，`carol` 再走一次
3. 检查 `carol` 的 user 与 `external_identity`
4. `carol` 再登录一次
5. `dave` 走 OIDC 登录
6. **同一 sub 并发发起 3 次登录**（JIT 开启）
7. **DB 侧把 `eve` 的绑定行 `user_id` 改成另一个用户**，再让该 sub 登录
8. `tenant-b`（同一 IdP）用**同一个** `sub` 登录，未绑定且 **JIT 关闭**
9. `tenant-b` **开启 JIT**，再用同一 `sub` 登录

### 预期结果

1. 步骤 1 → `403 SSO_ACCOUNT_NOT_LINKED`；**数据库里仍未创建** `carol` 的 user 与绑定
2. 步骤 2 → `200`
3. 步骤 3 → 新建 user（`email = carol@example.com`、`username = "carol"`、
   `password_hash` 是**永不匹配的占位 PHC 串**）+ 一条 `external_identity`
4. 步骤 4 → `200`，**复用**同一 user（user 表只有 **1** 条）
5. 步骤 5 → `403 ACCOUNT_DISABLED`（SSO 不绕过禁用）
6. 步骤 6 → 只产生 **1 个** user 与 **1 条**绑定；**`5xx` == 0**（允许个别 `409`）
7. 步骤 7 → `409 SSO_IDENTITY_LINK_CONFLICT`，且**绑定行 `user_id` 未被改动**
   （`DO NOTHING` + 读回比对，**不是** `DO UPDATE`）
8. 步骤 8 → `403 SSO_ACCOUNT_NOT_LINKED`
9. 步骤 9 → 建的是 `tenant-b` 下的**独立** user（不串租户）

> **rev 3 修订**：
> - **S19**（安全缺口）：既有 upsert 是 `DO UPDATE SET user_id` **改指**语义，原 design 让 iam「照抄」
>   = 把「绑定可被静默改到另一个用户」带进认证路径 → P7 + 步骤 7
> - **S20**：原并发幂等**没有任何实现机制** → P8 + 判据「5xx == 0」
> - **S33**：`username` 生成规则与 `password_hash`（NOT NULL）没定 → P8
> - **S39**：原步骤 7 把两个互斥前置塞进一步 → 拆成步骤 8/9

---

# AC-S-104: LDAP bind 认证 —— 成功 / 失败 / 不可达 / 空密码 / filter 注入

- **类型**: 功能 / 安全

### 前置条件

- **mock LDAP 服务**（P12）：记录 `connect` / `search` / `bind` 的次数与内容，可配返回码
- `tenant-a` 配好 `sso.ldap.url` / `baseDn` / `userSearchFilter` / `mailAttribute`
- 目录有 `uid=erin,ou=people,dc=example,dc=com`，密码 `Er1n-pass`

### 测试步骤

1. `username=erin`、`password=Er1n-pass`
2. 同用户名、`password=wrong`
3. 不存在的用户名 `ghost` + 任意密码
4. `password=""`，用户名分别 `erin` 与 `ghost`
5. `password="   "`（纯空格）
6. 停掉 mock LDAP
7. 用**错误密码**连续发满 `ip_limit` 次（读配置取实际值），再发第 `ip_limit + 1` 次
8. **每次请求后读 mock 的 `connect` / `search` / `bind` 计数**
9. 用户名填 **`*`**、**`)(uid=*`**、**`admin*`** 等 LDAP filter 注入载荷
10. 对日志做敏感值扫描（含阳性对照）

### 预期结果

1. 步骤 1 → `200`，`LoginResponse` 同构
2. 步骤 2 → `401 SSO_LDAP_BIND_FAILED`；**不泄露** DN、不泄露目录错误原文
3. 步骤 3 → 与步骤 2 **完全一致**
4. 步骤 4 → **`400` `SSO_LDAP_EMPTY_PASSWORD`**，**且 mock 的 `connect == 0`、`bind == 0`**
   （证明是**发请求之前**拦截）
5. 步骤 5 → 与步骤 4 相同
6. 步骤 6 → `503 SSO_LDAP_UNREACHABLE`
7. 步骤 7 → 第 `ip_limit + 1` 次 `429 LOGIN_RATE_LIMITED`。
   **注意**：IP 限流阈值是 **20 次 / 300 秒**（读配置确认），
   且该限流**只在 `x-forwarded-for` 非空时生效** —— 测试**必须**带该头，否则永不触发
8. 步骤 8 → 正常登录时 `connect=1`、`search=1`、`bind=2`（服务账号 1 + 用户 1，见 P13）；
   步骤 4/5 的三项计数**全为 0**；`bind` 只带用户 DN 与密码
9. 步骤 9 → 三个注入载荷**全部**返回 `401`（或 `401` 且不产生越权结果），
   **不得**因为 `*` 匹配到任意用户而登录成功；载荷**必须被 RFC 4515 转义**
10. 步骤 10 → 密码 / DN 在日志中 0 命中；**阳性对照**能命中

> **rev 3 修订**：
> - **S6**：「`ldap3` 的 in-memory 替身」**不存在** → P12
> - **S7**：「0 条 bind」**没有观测面** → P12 的 trait 必须让测试读计数
> - **S18**（致命陷阱）：`simple_bind` 返回 `Ok(LdapResult)`，**密码错误时外层仍是 `Ok`**，
>   必须再 `.success()`（`rc == 0`）。design 原写「可直接校验密码」→ 照字面实现**任意密码都能登录**
> - **S23**：原「10 次后 429」与既有实现冲突（阈值 20/300s；无 `x-forwarded-for` 时**跳过限流**）
> - **S29**：空密码从 `401` 改 **`400`**（请求不合法 ≠ 凭据错误）
> - 新增**步骤 9 的 LDAP 注入**（RFC 4515 转义）—— rev 2 只在 design 提了，AC 没守

### P12 LDAP 的可观测测试接缝（修 S6/S7）

```rust
pub trait SsoLdapClient: Send + Sync {
    async fn connect(&self) -> Result<Box<dyn SsoLdapConn>, SsoLdapError>;
}
pub trait SsoLdapConn: Send + Sync {
    async fn search(&mut self, base: &str, filter: &str, attrs: &[&str]) -> Result<Vec<LdapEntry>, SsoLdapError>;
    async fn simple_bind(&mut self, dn: &str, pw: &str) -> Result<bool, SsoLdapError>;  // bool = rc == 0
}
```

- **生产实现**：包 `ldap3`（`simple_bind` 后**必须** `.success()`）
- **测试实现**：记录 `connect`/`search`/`bind` 的**次数与参数**
- **`ldap3` 没有服务端/内存实现**；`platform-org` 的 `InMemoryLdapSource` 只实现 `search`，**不能**测 bind

---

# AC-S-105: LDAP 与本地账号共存 —— 密码不落库、映射不串、匹配顺序确定

- **类型**: 功能

### 前置条件

- `sso.enforce = false`；本地 `frank` 有本地密码；LDAP 也有 `mail = frank@example.com`
- `sso.ldap.mailAttribute = "mail"`；`external_id` = 用户 DN（P13）

### 测试步骤

1. `frank` 用**本地密码** → `POST /api/iam/login`
2. `frank` 用**LDAP 密码** → `POST /api/iam/sso/ldap/login`
3. 检查两条路径落到的是否同一 user
4. 检查 `platform_iam_user` 里 `frank` 的 `password_hash`
5. LDAP 侧改密码 → 旧密码登录 / 新密码登录
6. LDAP 侧把 `frank` 的 `mail` 改成**已被本地账号 `gina` 绑定**的邮箱 → 登录
7. 改掉 `frank` 的 `mail` 但 `external_id`（DN）不变 → 登录

### 预期结果

1. 步骤 1 → `200`；2. 步骤 2 → `200`
3. 步骤 3 → **同一** `user_id`（不产生影子账号）
4. 步骤 4 → 仍是**本地密码的 Argon2id PHC 串**，未被覆盖，
   也**未**新增「LDAP 密码摘要」类字段
5. 步骤 5 → 旧密码 `401`，新密码 `200`
6. 步骤 6 → `409 SSO_IDENTITY_LINK_CONFLICT`（**不**静默绑定到别人账号）
7. 步骤 7 → `200`，**仍然**落到 `frank`（P6 优先 `external_id`）

> **rev 3 修订**：**S21** —— `409` 在「已绑定」路径上**不可达**（design 没说 `external_id` 与 `mail` 谁优先）→ P6 + 步骤 7

### P13 LDAP 的外部标识取值（修 S32）

| 列 | 取值 | 理由 |
|---|---|---|
| `provider` | 固定 **`"ldap"`** | 单一 LDAP 源 |
| `external_id` | 用户的 **DN** | DN 是稳定唯一标识；`uid`/`mail` 都可能变 |
| 是否用服务账号 bind | **是**：`search` 用服务账号，`bind` 用用户凭据 → **每次登录 2 次 bind** | 否则无法按 filter 找 DN |

**bind 次数期望**（AC-S-104 步骤 8 依赖）：正常登录 `connect=1`/`search=1`/`bind=2`；空密码全 `0`

---

# AC-S-106: SSO 配置与密钥保护 —— `client_secret` 三处扫描 0 命中（带阳性对照）

- **类型**: 安全

### 前置条件

- `BASE_SSO_OIDC_CLIENT_SECRET` 注入唯一可识别串，如 `SECRET-CANARY-7f3a9b1e-UNIQUE`
- `tenant-a` 的 12 个键都写
- **执行前 `unset RUST_LOG`**（否则 `BASE_LOG_LEVEL=trace` 被覆盖）

### 测试步骤

1. `GET /api/config/entries?keys=<12 个 SSO 键>`（平台管理员令牌）
2. 同 12 个键（租户管理员令牌）
3. 触发一次 OIDC 失败（`alg=none`），取**完整错误响应体**
4. `BASE_LOG_LEVEL=trace` 起服务，重放一次成功 + 一次失败登录，捕获 **stdout 全量日志**
5. `GET /api/iam/sso/providers` 与 `GET /api/iam/sessions` 的完整响应体
6. 对 `platform_config` / `platform_tenant_config` / `platform_space_config` 三表做全表文本扫描
7. **阳性对照**：用**已登记且无 `format`** 的 `org` 模块 `ldapBaseDn` 写入同一 canary →
   重跑步骤 1 与步骤 3 → 断言扫描器**能命中** → 清理

### 预期结果

1. 步骤 1 → `200`，`entries` 长度 **== 12**（按精确键名读；**不是**「全量导出」）
2. 步骤 2 → `200`，`source ∈ {platform, tenant, default}`，**不得**为 `space`
   （D53：`scope: tenant` 意味着平台可写、space 不可写）
3. 步骤 1~6 → canary 在**全部位置** 0 命中
4. 步骤 7 → 扫描器**命中** canary（证明 0 命中是真的）
5. `sso.oidc.clientId` **可以**出现；`client_secret` 的任何片段 0 命中
6. 12 个键**全部**在 `platform-config/manifest.json` 登记
7. 步骤 4 的日志里**至少有** SSO 模块的 `tracing` 行（证明 trace 真的生效）

> **rev 3 修订**（本节是 rev 2 的头号缺陷，三步全跑不起来）：
> - **S1**：`GET /api/config/entries` **必须带 `keys`**，不给返回空；含 platform scope 键 → 403。原「取全量」是**不存在的能力**
> - **S2**：`?scope=tenant` 参数**不存在** → 改「同一组键 + 租户令牌」
> - **S3**：原「往测试用键写入」做不到（未登记键 404）→ 改用已登记的 `ldapBaseDn`
> - **S40**：`source` 是四值 → 写死 `{platform, tenant, default}`，禁 `space`
> - **S47**：`BASE_LOG_LEVEL` 会被 `RUST_LOG` 覆盖 → 补 `unset`
> - **S41**：承认「租户/provider 存在性可见」（登录页需要），但**不得**泄露 issuer/clientId

---

# AC-S-107: SSO 会话与本地会话**同构**（键集合相同，来源可辨）

- **类型**: 功能 / 安全

### 前置条件

`tenant-a` 同时有本地账号 `grace` 与 SSO 账号 `bob`（绑定完成）

### 测试步骤

1. `grace` 本地登录；`bob` OIDC 登录
2. 对两者分别 `GET /api/iam/sessions`
3. 对 `bob` 的会话 `DELETE /api/iam/sessions/{id}` → 再用他的 `refreshToken` 刷新
4. 对 `bob` `POST /api/iam/logout-all`
5. 检查 `platform_iam_session` 表里两条会话
6. 比较两人 access token 的 header（`alg`/`kid`）与 claim 结构

### 预期结果

1. 步骤 2 → 两条会话**都在**同一列表里，**键集合完全相同**（同一视图类型）
2. `authProvider` 字段存在，值是 **`"local"` / `"oidc"` / `"ldap"`** 之一；
   **它是唯一允许两条会话取值不同的字段**（其余字段只是值不同，键必须相同）
3. 步骤 3 → `DELETE` 返回 **`204`** 且响应体长度为 **0**；随后 refresh 失败 `TOKEN_REVOKED`
4. 步骤 4 → `204`；该用户**全部** family 作废，含 SSO 会话
5. 步骤 5 → 两条会话结构**同构**；**无「SSO 专用会话表」**
6. 步骤 6 → access token 的 `alg`/`kid`/claim 结构**完全一致**；无第二套 token 体系

> **rev 3 修订**：
> - **S22**：原 AC 要求「会话里能看出 SSO 来源」，但 design 只加**数据库列**，`SessionView` 没有该字段
>   → 必须同时改 `SessionView` + 两条 SELECT。**陷阱**：`SessionRecord` 由 `tenant_row!` 按列名 `try_get`，
>   加 struct 字段却不加进 SELECT → **运行期** 500（不是编译错）
> - **S25**：「字段集合一致」与「能区分来源」矛盾 → 拆成「键集合相同 + `authProvider` 是唯一允许不同值的字段」
> - **S46**：`DELETE`/`logout-all` 返回码没钉死 → 写死 `204`
> - **S50**：「不同会话」没给判定字段 → 写死 `family_id` 不等 + 表中两行

---

# AC-S-108: SSO 审计事件 —— **声明 + 发出**，本轮**不落库**

- **类型**: 功能 / 合规

### 前置条件

- `platform-iam` **自建** `SsoAuditEvent` + `SsoAuditRecorder`（P14）
- 测试注入 `SsoRecordingAuditRecorder`

### 测试步骤

1. 检查声明的 `AuditEventSpec` 清单
2. 注入 recorder，跑一次 OIDC 成功登录
3. 跑一次 OIDC 失败登录（`alg=none`）
4. 跑一次 LDAP 成功 + 一次失败
5. 对录制到的**全部 payload** 做敏感值扫描（canary + 阳性对照）
6. **不注入** recorder（默认实现），跑一次成功登录
7. 检查事件是否落入任何表

### 预期结果

1. 步骤 1 → 至少声明 `iam.sso.login.succeeded` / `iam.sso.login.failed` / `iam.sso.link.created`，
   每个有 `code`/`action`/`objectType`
2. 步骤 2 → 发出 `iam.sso.login.succeeded`，payload 的**键集合恰好是**
   `{provider, external_id, user_id, ip, user_agent}`（白名单，多余键即失败）
3. 步骤 3 → 发出 `iam.sso.login.failed`，payload 键集合 `{provider, code, ip}`
   —— **不含** token、**不含** `id_token` 原文、**不含**密码
4. 步骤 4 → LDAP 成败各一条，`provider == "ldap"`
5. 步骤 5 → canary 0 命中；**阳性对照**能命中
6. 步骤 6 → 默认 `NoopSsoAuditRecorder`，**登录仍成功**；且有 **warn 级**提示（「审计未接 sink」）
7. 步骤 7 → **不验落库** —— REQ-014 才提供 sink。此条**不得**写成「审计记录已可查询」

> **rev 3 修订**（修 S8，主体不存在）：
> - `platform_core` **没有** `AuditRecorder` trait（只有 `Auditable` + `AuditEventSpec`）
> - `AuditEventSpec` 字段是 `code/action/object_type/description`，**没有 payload**
> - `platform-org` 的 `RecordingAuditRecorder` 绑死 `OrgAuditEvent`（只有 `detail`，无 payload），且 `iam → org` 会成环
> - → **`platform-iam` 必须自建**（P14）
> - **S35**：注入用**后设槽**（照 `PermissionProviderSlot`），**不改 `IamService::new` 签名**
>   —— 否则会破 4 处调用点（含 `platform-rbac` 的 2 处测试，属已合并需求产物，按 D109 须声明）

### P14 审计接缝（修 S8/S35）

```rust
// platform-iam 自建，不依赖 platform-org
pub struct SsoAuditEvent { pub code: &'static str, pub payload: SsoAuditPayload }
pub struct SsoAuditPayload {  // 键集合是白名单，见预期 2/3
    pub provider: String, pub external_id: Option<String>, pub user_id: Option<String>,
    pub ip: String, pub user_agent: String, pub code: Option<&'static str>,
}
pub trait SsoAuditRecorder: Send + Sync { fn record(&self, ev: &SsoAuditEvent); }

impl IamService {
    pub fn with_sso_audit_recorder(&self, r: Arc<dyn SsoAuditRecorder>) -> bool { /* 同 PermissionProviderSlot 模式 */ }
}
```

同时仍要实现 `Auditable`（`audit_events()` 返回 `AuditEventSpec` 列表）——
那是**平台契约**要求的声明面，与上面的 payload 接缝是**两件不同的事**。

---

# AC-S-109: 多 IdP 并存与租户隔离

- **类型**: 功能 / 安全

### 前置条件

`tenant-a` 配 OIDC + LDAP；`tenant-b` 只配 OIDC（**另一个** mock IdP）；`tenant-c` 什么都没配

### 测试步骤

1. 三个租户分别 `GET /api/iam/sso/providers`
2. `tenant-a` 走 LDAP 登录；再走 OIDC 登录
3. 用 `tenant-b` 的 OIDC `state` 到 `tenant-a` 回调
4. `tenant-c` 打 `authorize`
5. `tenant-c` 打 `POST /api/iam/sso/ldap/login`
6. `tenant-a` 关闭 OIDC 后打 `authorize`
7. 修改 `tenant-b` 的 issuer，检查 `tenant-a` 的登录是否受影响
8. **用未认证请求读 `providers`，检查响应里有没有配置值**

### 预期结果

1. 步骤 1 → `tenant-a` **2 个**、`tenant-b` **1 个**、`tenant-c` **空列表**（不是 404/500）
2. 步骤 2 → 两条都成功，且**两次登录的 `family_id` 不相等**，`platform_iam_session` 新增**两行**
3. 步骤 3 → `400 SSO_OIDC_STATE_INVALID`
4. 步骤 4 → `400 SSO_PROVIDER_NOT_CONFIGURED`
5. 步骤 5 → 与步骤 4 **同一个码**
6. 步骤 6 → `400 SSO_PROVIDER_NOT_CONFIGURED`（**禁用 = 未配置**）
7. 步骤 7 → `tenant-a` 的登录结果**不变**
8. 步骤 8 → 响应里**只有** `type`/`id`/`label`/端点路径，
   **不含** `issuer`/`clientId`/`clientSecret`/LDAP `url`/`baseDn`（S41）

> **rev 3 修订**：
> - **S24**：与 AC-S-020 粒度冲突 → 见 AC-S-020 的统一判据
> - **S40**：`source` 取值与 `scope: tenant` 语义 → 见 AC-S-106 预期 2
> - **S41**：承认「存在性可枚举」，但用步骤 8 守住「不泄露配置值」
> - **S50**：用 `family_id` 不相等 + 表中两行判定

---

# 错误码总表（**21 个**，D124）

| # | code | HTTP | 何时 |
|---|---|---|---|
| 1 | `SSO_REQUIRED` | 403 | 强制 SSO，本地登录被拒（在密码校验前） |
| 2 | `SSO_PROVIDER_NOT_CONFIGURED` | 400 | 租户未配**或已禁用**该协议（合并） |
| 3 | `SSO_CONFIG_INVALID` | 400 | 配置值非法（`ldaps://`、相对 redirectUri、filter 缺占位符、空 issuer） |
| 4 | `SSO_OIDC_DISCOVERY_FAILED` | 503 | 拿不到 `.well-known` |
| 5 | `SSO_OIDC_DISCOVERY_INVALID` | 502 | 拿到了但非法 / 缺端点 |
| 6 | `SSO_OIDC_STATE_INVALID` | 400 | 不存在 / 已消费 / 租户或 provider 不匹配 |
| 7 | `SSO_OIDC_STATE_EXPIRED` | 400 | 超过 TTL |
| 8 | `SSO_OIDC_CALLBACK_PARAM_MISSING` | 400 | 回调缺 `code` 或 `state` |
| 9 | `SSO_OIDC_TOKEN_EXCHANGE_FAILED` | 400 | code 伪造 / 已用 / PKCE 不符 |
| 10 | `SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE` | 502 | IdP 的 token 端点挂 / 超时 |
| 11 | `SSO_OIDC_TOKEN_INVALID` | 401 | 签名 / iss / aud / iat 不匹配 |
| 12 | `SSO_OIDC_TOKEN_EXPIRED` | 401 | `exp` 已过 |
| 13 | `SSO_OIDC_ALG_NOT_ALLOWED` | 401 | `none` / `HS*` / 白名单外 |
| 14 | `SSO_OIDC_NONCE_MISMATCH` | 401 | `nonce` 不符 |
| 15 | `SSO_OIDC_PKCE_MISMATCH` | 400 | `code_verifier` 与 challenge 不符 |
| 16 | `SSO_LDAP_UNREACHABLE` | 503 | 连不上 |
| 17 | `SSO_LDAP_BIND_FAILED` | 401 | 密码错**或**用户不存在（同码） |
| 18 | `SSO_LDAP_EMPTY_PASSWORD` | **400** | 空 / 纯空白密码 |
| 19 | `SSO_IDENTITY_LINK_CONFLICT` | 409 | 绑定改指 / mail 撞他人绑定 / JIT 撞既有 username |
| 20 | `SSO_ACCOUNT_NOT_LINKED` | 403 | 未绑定且 JIT 关闭 |
| 21 | `SSO_TICKET_INVALID` | 401 | ticket 不存在 / 已用 / 过期 |

**`TITLES.len()` 期望值 = 56（基线）+ 21 = 77。**

---

## AC 与安全约束的对应关系

| 约束 | 由哪条 AC 守 |
|---|---|
| §1 LDAP 空密码必须显式拒绝 | AC-S-104 步骤 4/5/8（含 0 次请求） |
| §2 强制 SSO 在密码校验前 + **读取失败 fail-closed** | AC-S-021 步骤 1~3/5/7/8 |
| §3 `client_secret` 不得出现在任何可读位置 | AC-S-106（含阳性对照）、AC-S-108 步骤 5、AC-S-109 步骤 8 |
| §4 拒绝 `none`/混淆、**不采信 `x5c`/`jku`** | AC-S-102 步骤 4/5/6/9/10/11 |
| §5 不绕过禁用与租户隔离 + **绑定不可改指** | AC-S-103 步骤 5/7、AC-S-109 步骤 3、AC-S-105 步骤 6 |
| §6 审计事件不含敏感值 | AC-S-108 步骤 5 |
| **§7（新增）LDAP filter 必须转义** | AC-S-104 步骤 9 |
| **§8（新增）token 不得进 URL** | AC-S-100 预期 3 |

## 覆盖度自检（rev 3）

| 问题 | 答案 |
|---|---|
| 每条 AC 的主体都存在吗？ | 已逐项核实。**rev 3 新增 3 个必须新建的接缝**：`SsoLdapClient`（P12）、`SsoAuditRecorder`（P14）、`SessionView.authProvider`（AC-S-107）。**审计 sink 仍不存在**（REQ-014）→ AC-S-108 显式排除落库 |
| 有没有物理不可达的前置？ | rev 3 已消除 4 处：LDAP 替身（S6）、`code_verifier` 客户端篡改（S5）、用 OIDC state 打 LDAP（S4）、`?scope=tenant`（S2）。https 用测试根（P10）后可达 |
| 失败路径都有明确错误码吗？ | 21 个码，每条 AC 的失败分支都点名 |
| 数字是否自洽？ | 错误码 **21**（全文统一）；AC **12** 条；`TITLES.len()` 期望 **77** |
| 有没有依赖未交付的需求？ | 依赖 REQ-007/009/010，均已合并。**不依赖** REQ-014 / REQ-022 |
