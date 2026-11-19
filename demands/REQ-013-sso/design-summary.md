# 设计摘要：SSO 入站（OIDC + LDAP）与强制 SSO

- **需求编号**: REQ-013
- **状态**: rev 2（2026-11-19 范围收窄后）
- **对应验收标准**: [acceptance.md](acceptance.md)（12 条）
- **更新时间**: 2026-11-19 23:50

> 本文只写**实现依据**：现状盘点、数据模型、端点、时序、错误码、代码落点。
> 「要验什么」在 acceptance.md，不在这里重复。

---

## 1. 现状盘点（写规格前实测，不靠推断）

### 1.1 已经有的（可直接复用，不要重造）

| 能力 | 位置 | 复用方式 |
|---|---|---|
| 会话 + refresh family | `platform_iam_session` 表（`family_id` / `revoked_at`） | SSO 登录走**同一个** `issue_session` |
| Access token / JWKS | `platform_iam/src/jwt.rs`（ES256 + `kid` + 双密钥轮换） | 完全复用，不新增 token 体系 |
| 静态加密 | `platform_iam/src/crypto.rs`：`encrypt` / `decrypt`（ChaCha20-Poly1305，密钥来自 `BASE_IAM_SECRET_KEY`） | 租户级 secret 加密列留接口时用 |
| 高熵随机串 | 同上 `random_base64url` | `state` / `nonce` / `code_verifier` |
| LDAP 客户端 | `platform-org/src/ldap.rs` 的 `Ldap3Source`（`ldap3` 0.11.5） | **照搬连接方式**：`LdapConnAsync::new` + `ldap3::drive!` |
| LDAP 密码校验原语 | `ldap3::Ldap::simple_bind(bind_dn, bind_pw)` → `LdapResult`。⚠️ **返回 `Ok` 不代表认证成功**，`rc != 0` 时外层仍是 `Ok`，**必须**再 `.success()` | 包一层 `SsoLdapClient`（见 §5.2），`simple_bind` 后 `.success()` 判 `rc == 0`。正确用法先例：`platform-org/src/ldap.rs:481-495` |
| 出站 HTTP | `platform-org/src/scim.rs` 的 `HttpScimGateway`（`hyper_util::client::legacy` + `HttpConnector`） | **照搬这个模式**，不引 `reqwest` |
| 账号外部身份映射 | `external_identity(tenant_id, provider, external_id, user_id)`，唯一键 `(tenant_id, provider, external_id)` | SSO 绑定写这张表 |
| 登录限流 | `platform_iam_login_rate` / `platform_iam_login_attempt` 表 | LDAP 失败复用同一套限流 |
| 审计接缝 | `platform_core::extension::auditable::{Auditable, AuditEventSpec}` + `platform-org/src/audit.rs` 的 `AuditRecorder` 模式 | **照搬**：声明 + 接缝，不建表 |
| 配置三层 | `platform_config` / `platform_tenant_config` / `platform_space_config` + manifest 登记 + `format` 校验 | SSO 非敏感配置登记进去 |
| 全套错误码规范 | 各 crate 的 `error.rs`：`code()` / `detail()` / `ALL_CODES` | 新增码必须三处齐动 |

### 1.2 **没有的**（本需求要建）

| 缺什么 | 影响 | 本设计怎么补 |
|---|---|---|
| **XML 库** | SAML / CAS 2.0 做不了 | **不做**（用户已裁定）。见 demand §范围收窄 |
| **审计 sink（落库 + 查询）** | 审计只能声明不能查 | 只声明事件 + 走 `AuditRecorder`，落库留 REQ-014 |
| `oidc_state` 类的一次性凭证表 | 无法防 CSRF / 重放 | 新建 `platform_iam_oidc_state` |
| LDAP **bind** 认证（现有只 search 同步） | 无法用 LDAP 登录 | 在 `platform-iam` 新增 `sso/ldap.rs`，照搬 `Ldap3Source` 的连接写法 |
| session 的「来源」维度 | 分不出本地/SSO 会话 | `platform_iam_session` 加 `auth_provider` 列（默认 `local`） |
| SSO 配置项 | 无处配置 | `platform-config/manifest.json` 登记 12 个键 |

---

## 2. 数据模型

### 2.1 新建：`platform_iam_oidc_state`

```sql
CREATE TABLE IF NOT EXISTS platform_iam_oidc_state (
    state          TEXT        PRIMARY KEY,          -- base64url(32B)，高熵
    tenant_id      TEXT        NOT NULL,
    provider       TEXT        NOT NULL,             -- 'oidc'（将来多 provider 时区分）
    nonce          TEXT        NOT NULL,
    code_verifier  TEXT        NOT NULL,             -- PKCE 的明文 verifier（用它算 challenge）
    redirect_uri   TEXT        NOT NULL,             -- 回调用，必须与授权时逐字一致
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ NOT NULL,             -- created_at + TTL（默认 5 分钟）
    consumed_at    TIMESTAMPTZ                       -- 非空 = 已用过（一次性）
);

CREATE INDEX IF NOT EXISTS platform_iam_oidc_state_expiry_idx
    ON platform_iam_oidc_state (expires_at);
CREATE INDEX IF NOT EXISTS platform_iam_oidc_state_tenant_idx
    ON platform_iam_oidc_state (tenant_id, provider);
```

**设计要点**：

- **`state` 作主键** → 天然保证唯一，且回调是一次 `SELECT … FOR UPDATE` + `UPDATE consumed_at` 即可原子消费
  （并发两个回调只有一个能成功）。
- **`code_verifier` 明文存**（不是 hash）：PKCE 的 verifier 要在换 token 时**原样发出**，
  无法只存摘要。它的价值不在于「不可还原」，而在于**只对这一次授权有效且 5 分钟过期**。
  存库而非内存，是因为**多实例部署下内存不共享**（本机单实例也必须按多实例写，否则将来必挂）。
- **`expires_at` + 清理**：读路径**不**依赖「有没有被清理过」—— 判定先看 `expires_at`，
  清理是独立的后台动作（避免清理失败就等于永久有效）。
- **RLS**：本表**不启用 RLS**。理由：回调时请求还没有认证上下文，拿不到 `tenant_id` 的权威来源，
  只能靠 `state` 反查。**安全由「`state` 是不可猜的高熵串」保证**，不是由 RLS 保证。
  租户校验在应用层做（`state.tenant_id` 必须等于请求头租户）。
- 表名用 `platform_iam_` 前缀，与既有 IAM 表一致。
- ⚠️ **必须 `GRANT`（rev 3 新增，QA 的 S37）**：本仓 IAM 表在迁移里显式授权给应用角色；
  新表不授权 → 应用角色 `INSERT` 会 `permission denied for table`。
  **本表明确不启用 RLS，就更需要 GRANT**。写法照 `20261118000500_iam_login_rate.up.sql:38-44`
  （带 `IF EXISTS (SELECT 1 FROM pg_roles …)` 守卫，兼容没有该角色的环境）。

### 2.1.1 新建：`platform_iam_sso_ticket`（rev 3 新增，P4 / D124 决策①）

回调不能直接返回 JSON（浏览器会渲染成文本），所以回调 302 到前端并携带**一次性 ticket**。

```sql
CREATE TABLE IF NOT EXISTS platform_iam_sso_ticket (
    ticket       TEXT        PRIMARY KEY,          -- base64url(32B)
    tenant_id    TEXT        NOT NULL,
    user_id      TEXT        NOT NULL,
    session_id   TEXT        NOT NULL,             -- 已建好的会话
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL,             -- created_at + 30 秒
    consumed_at  TIMESTAMPTZ                       -- 非空 = 已换过
);

CREATE INDEX IF NOT EXISTS platform_iam_sso_ticket_expiry_idx
    ON platform_iam_sso_ticket (expires_at);
```

**要点**：

- **TTL 只有 30 秒** —— 它只在「回调 302 → 前端加载完成 → 调 exchange」之间存活
- **一次性**：`consumed_at` 非空即拒（`SSO_TICKET_INVALID`）
- **不存 token**：只存 `session_id`，exchange 时复用 `issue_session` 的同一套逻辑 ——
  **token 绝不进 URL、绝不进数据库**
- 同样**不启用 RLS**（exchange 时前端还没令牌，拿不到权威租户来源）+ **必须 GRANT**

### 2.2 改：`platform_iam_session` 加来源列

```sql
ALTER TABLE platform_iam_session
    ADD COLUMN IF NOT EXISTS auth_provider TEXT NOT NULL DEFAULT 'local';
-- 取值：'local' | 'oidc' | 'ldap'
-- 不建外键：provider 不是实体表，是一组配置
```

**为什么加列而不是复用 `device_label`**：AC-S-107 要求「能区分来源」，
用自由文本字段承载结构化语义，等于让**展示字符串**当**判定依据** —— 那是必然出错的。
一个 `TEXT` 列 + 三个已知取值，成本极低。

> **`down` 迁移**必须 `DROP COLUMN`，且**不删表**（与 REQ-010 对 `platform_tenant_config.value`
> 的 down 策略一致：降级不应丢数据）。

### 2.3 复用：`external_identity`

不需要改表。`upsert_external_identity` 已在 `platform-org/src/store.rs:635`。

**依赖方向问题**：`platform-iam` **不能**依赖 `platform-org`（会成环：
`platform-server` 同时挂两者，org 已依赖 iam 的令牌校验）。

**解决**：`platform-iam` 自己写一个最小 upsert（同一张表，列就那几个），
或用 `platform-core` 的 trait 由装配层注入。**选前者** —— 一个 6 列的 upsert
不值得为它立一个 trait 再加一层注入；两处写同一张表的风险由**唯一约束**兜住
（`(tenant_id, provider, external_id)` 冲突时 `ON CONFLICT DO UPDATE`，不会产生重复）。

> ⚠️ **rev 3 修正（QA 的 S19）：绝对不要照抄 org 的 upsert。**
> 既有实现是 `ON CONFLICT (tenant_id, provider, external_id) DO UPDATE SET user_id = EXCLUDED.user_id`
> （`platform-org/src/store.rs:655-656`）—— 这是**改指**语义：同一个外部身份再写一次，
> 会把绑定**静默改到另一个本地用户**。在组织同步场景合理（LDAP 管理员改了映射），
> **但在认证路径上是账号接管**。
>
> **认证路径必须用 `ON CONFLICT DO NOTHING` + 读回比对**：读回的 `user_id` 与期望不一致
> → `409 SSO_IDENTITY_LINK_CONFLICT`，**不静默改指**。AC-S-103 步骤 7 专门守这一条。

> 这是**有意的重复**，理由写在这里，避免下一个人以为是疏漏。

---

## 3. 端点设计

全部挂在 `/api/iam/sso/*`（`platform-iam` 的 `http.rs` 里新增 `sso_router`，
由 `platform-server` 的 `main.rs` 与既有 `/api/iam` 一起 nest）。

| 方法 | 路径 | 认证 | 说明 |
|---|---|---|---|
| `GET` | `/api/iam/sso/providers` | **无**（但要租户头） | 当前租户可用的 provider 列表。登录页要在**未登录**时就知道该显示什么按钮 |
| `GET` | `/api/iam/sso/oidc/authorize` | **无** | 302 到 IdP；落一条 `oidc_state`。**租户来源：`?tenant=` 查询参数优先，其次 `x-tenant-id` 头** —— 见 §3.3 |
| `GET` | `/api/iam/sso/oidc/callback` | **无** | 收 `code` + `state`；换 token、验签、绑定、建会话 |
| `POST` | `/api/iam/sso/ldap/login` | **无** | body：`{username, password}`；bind 校验后建会话 |
| `POST` | `/api/iam/sso/exchange` | **无** | body：`{ticket}`；**rev 3 新增**（见 §3.2）：把一次性 ticket 换成 `LoginResponse` |

**为什么这些端点都不需要认证**：它们是**登录入口**，用户此刻还没令牌。

**为什么仍要租户头**：SSO 配置是租户级的；没有租户就无法决定用哪个 IdP。
与既有 `/api/iam/login` 的约定一致（`x-tenant-id`）。

**为什么不新增权限点**：这些是公开登录端点。
SSO **配置的写入**走 REQ-010 的通用配置接口（已有 `config:entry:write` 权限点），
**不新造一套 SSO 配置管理接口** —— 那是重复建设，且要连带权限点三件套（D27 启动对账）。

### 3.1 响应形状

`GET /api/iam/sso/providers` →

```json
{
  "providers": [
    { "type": "oidc", "id": "oidc", "label": "企业单点登录", "authorizeUrl": "/api/iam/sso/oidc/authorize" },
    { "type": "ldap", "id": "ldap", "label": "LDAP / AD", "loginUrl": "/api/iam/sso/ldap/login" }
  ]
}
```

未配置任何 provider → `{"providers": []}` + **200**（不是 404：这是「查了但没有」，不是「没有这个接口」）。

`ldap/login` 与 `POST /api/iam/sso/exchange` 成功 → **与 `POST /api/iam/login` 完全相同的 `LoginResponse`**。
这是 AC-S-107「同构」的直接要求。

> **rev 3 修正（QA 的 S27）**：`oidc/callback` **不返回** `LoginResponse`。
> 它是**顶层浏览器导航**的落点 —— 返回 `200 + JSON` 会被浏览器渲染成文本，
> SPA 拿不到 `refreshToken`。rev 2 让 callback 直接返 JSON，靠「AC 用脚本客户端测」**恰好掩盖**了这条。
>
> **rev 3 改为 ticket 交接（§2.1.1）**：
> ```
> callback → 302 {前端基址}/sso/callback?ticket=<ticket>
> 前端 SsoCallback 页 → POST /api/iam/sso/exchange {ticket} → LoginResponse
> ```
> 前端因此需要新增 `SsoCallback.svelte` + `/sso/callback` 路由（见 §8）。
>
> **失败时也必须 302**（rev 3 补充）：`302 {前端基址}/sso/callback?error=<ERROR_CODE>` ——
> 否则用户从 IdP 跳回来只看到一个 JSON 错误页，连登录页都回不去。
> `error` **只放错误码**，不放 detail、不放配置值。
> **不选 cookie 方案**：那会破坏 D99（会话存 `sessionStorage`），引入第二套会话机制。

---

### 3.3 authorize 的租户来源（rev 3 补充，前端 Dev 发现的跨端缺口）

**问题**：`/authorize` 是**顶层浏览器导航**的起点，浏览器**无法给整页导航设置请求头**。
而此刻**还没有 `state`** 可反查租户（§2.1 的办法在这里用不了）。
rev 2 写「要租户头」→ **真实浏览器里 OIDC 入口物理不可用**，而 AC 用脚本客户端测**又恰好看不出来**。

**规则**：

```
租户 = ?tenant= 查询参数  >  x-tenant-id 请求头  >  401 TENANT_CONTEXT_MISSING
```

- 前端在跳转前把当前租户拼进 URL（登录页本来就要求用户手输租户，D98，所以这个值现成）
- **LDAP 不需要**：`POST /sso/ldap/login` 是 XHR，正常带头即可
- **回调不需要**：它靠 `state.tenant_id` 反查（§2.1 / acceptance P3）
- **不新增错误码**：复用 `TENANT_CONTEXT_MISSING`（401），避免为它改 8 处（G2）

**租户 id 进 URL 是可接受的**：登录页本来就要求手输租户，它不是秘密；
AC-S-109 步骤 8 已承认「存在性可枚举」。但**不得**因此在 URL 里带 `issuer` / `clientId` / LDAP 连接串。

---

## 4. OIDC 授权码流程

### 4.1 时序

```text
浏览器            platform-server                  IdP
   │                     │                          │
   ├─ GET authorize ────>│                          │
   │                     ├─ 读租户配置（issuer…）      │
   │                     ├─ 拉 .well-known（带缓存）   │
   │                     ├─ 生成 state/nonce/verifier │
   │                     ├─ INSERT oidc_state        │
   │<─ 302 + state… ─────┤                          │
   ├──────────────────────── 用户认证 ─────────────>│
   │<────────────────────── 302 callback?code&state ┤
   ├─ GET callback ─────>│                          │
   │                     ├─ SELECT … FOR UPDATE state│
   │                     ├─ 校验租户/过期/未消费       │
   │                     ├─ POST token（code+verifier）──>│
   │                     │<──────── id_token ────────┤
   │                     ├─ JWKS 取公钥（按 kid）      │
   │                     ├─ 验签 + 校验 iss/aud/exp/nonce
   │                     ├─ 绑定/建号（external_identity）
   │                     ├─ issue_session（复用）     │
   │<─ LoginResponse ────┤                          │
```

### 4.2 实现要点

1. **发现文档缓存**：按 `issuer` 缓存 `authorization_endpoint` / `token_endpoint` / `jwks_uri`，
   TTL 取小时级（这些端点极少变）。缓存是「性能优化」，**不是正确性依赖**：
   拿不到缓存就现拉，拉不到就 `503 SSO_OIDC_DISCOVERY_FAILED`。

2. **JWKS 缓存与刷新**：
   - 按 `kid` 索引公钥，TTL 小时级。
   - **未知 `kid` → 强制刷新一次再试**（AC-S-102 步骤 7）。
   - 刷新后仍找不到 → `401 SSO_OIDC_TOKEN_INVALID`。
   - **只从 `jwks_uri` 取公钥**，不从 token 里自带的 `x5c` / `jku` 取 ——
     **从 token 自身取公钥等于不验签**（这是 JWT 验签最常见的实现错误）。

3. **算法白名单**：
   ```
   允许集合 = {RS256, ES256}（可由配置收窄，不可放宽到 none / HS*）
   ```
   用的是 `jsonwebtoken::Validation` 的 `algorithms` 字段 —— 它**默认就拒绝 `none`**，
   但仍要显式声明，防止将来有人把它改成 `Algorithm::from_str(header.alg)`。
   **绝不允许**「按 token header 里的 `alg` 决定验签方式」。

4. **RS→HS 混淆防御**：因为白名单不含 `HS*`，用公钥内容做 HMAC 密钥的伪造 token 会被
   `algorithms` 校验直接拒（AC-S-102 步骤 5）。

5. **claims 校验清单**：
   | claim | 校验 |
   |---|---|
   | `iss` | 与配置的 issuer **逐字相等** |
   | `aud` | 包含配置的 `clientId` |
   | `exp` | 未过期（**允许小 skew**，如 60s —— 但 skew 要显式配置，不能隐式给大值） |
   | `iat` | 不在未来（同样带 skew） |
   | `nonce` | 与 `oidc_state.nonce` **逐字相等** |

6. **换 token 的请求**：`application/x-www-form-urlencoded`，字段
   `grant_type=authorization_code` / `code` / `redirect_uri` / `client_id` / `client_secret` /
   `code_verifier`。**`client_secret` 只在这里出现**（内存里从环境变量取），
   绝不进日志、绝不进错误详情（AC-S-106）。

7. **出站 HTTP**：照搬 `HttpScimGateway` 的 `hyper_util::client::legacy::Client::builder(...).build_http()`。
   **只支持 `http://` / `https://`**，且 `https` 需要 TLS —— **注意**：现有 SCIM 网关是 `build_http()`
   （无 TLS）。本需求对 IdP 的调用**必须支持 https**（真实 IdP 都是 https）。

   > ✅ **TLS 决策已定（D123，用户 2026-11-19 批准）**：**引入 TLS 依赖，支持 `https`。**
   >
   > 实测背景：`Cargo.lock` 里 `rustls` / `native-tls` / `openssl` / `ring` **全 0 命中**，
   > `hyper-util` 的 `build_http()` 不带 TLS。真实企业 IdP 全是 `https://`
   > → 只支持 http 的 OIDC **在真实环境基本不可用**。已上报用户并获批。
   >
   > **Dev 的实施约束（必须逐条做到并写进自验报告）**：
   > 1. **只换 connector，不换 HTTP 栈**：继续用 `hyper` + `hyper-util`，
   >    把 `Client::builder(..).build_http()` 换成 `.build(带 TLS 的 connector)`。
   >    **不得引入 `reqwest`**。
   > 2. **优先 `rustls` + `ring`**：自包含、不依赖系统 OpenSSL
   >    （与项目「私有化机常常没有 OpenSSL 开发包」的既有判断一致）。
   >    标准写法参考 `hyper_rustls::HttpsConnectorBuilder::new().with_webpki_roots().https_or_http().enable_http1().build()`。
   >    **`https_or_http()` 是刻意的**：本轮的 mock IdP 用 `http://`，真实 IdP 用 `https://`，
   >    两者都要能用（不是二选一）。
   > 3. **必须实测并报告三个数**：① `Cargo.lock` 包数增量（基线 **296**）；
   >    ② 首次 `cargo build` 耗时；③ 低配机（`jobs = 3`）能否编过。
   > 4. **若 `ring` 的汇编编译不可接受**：备选 `native-tls`（系统 OpenSSL），
   >    但要在报告里写明对私有化部署的影响，**并回报 BA**。
   > 5. `SSO_OIDC_TLS_UNSUPPORTED` 这个错误码**不再需要** ——
   >    但 `SSO_PROVIDER_NOT_CONFIGURED` 仍要覆盖「issuer 是 https 但本机编译时没启用 TLS」的极端情形
   >    （理论上不会发生，但不要假设）。
   >
   > **这与既有的「避开 ring / aws-lc-rs」判断（`jsonwebtoken` 选 `rust_crypto`）不矛盾**：
   > 那条是为 JWT 验签性能；TLS 没有成熟纯 Rust 等价物，`ring` 是当前最省的选择。
   > **D123 是唯一破例点**，别处仍按零新增执行。

---

## 5. LDAP bind 认证流程

```text
POST /api/iam/sso/ldap/login {username, password}
   │
   ├─ 1. 读租户 LDAP 配置（enabled / url / baseDn / filter / mailAttribute）
   ├─ 2. **空密码直接拒**（trim 后为空）→ 401 SSO_LDAP_EMPTY_PASSWORD
   │      ⚠️ 必须在发请求之前，且不得连接目录服务
   ├─ 3. 连接（LdapConnAsync + drive!），用**服务账号** bind（若要 search）
   ├─ 4. search 出用户 DN（filter 代入 username，**必须做转义**）
   ├─ 5. 用用户 DN + 用户密码 simple_bind → **LdapResult** → **必须** `.success()`
   │      ⚠️ 外层 `Ok` 只代表「请求发出并收到响应」；`rc != 0`（如 rc=49）才是密码错
   │      `.success()` 成功 → 继续；失败 → 401 SSO_LDAP_BIND_FAILED（与「用户不存在」同一个码）
   ├─ 6. 从条目取映射字段（如 mail）→ 匹配/建本地 user
   ├─ 7. issue_session（auth_provider='ldap'）
   └─ 8. 发审计事件
```

### 5.1 三个必须写死的细节

1. **空密码拦截位置**：在 `service` 层入口、**连接之前**。
   AC-S-104 步骤 8 会用「mock LDAP 收到 0 条 bind」来验证这一点 —— 若拦截写在 `ldap.rs` 里
   而连接已经建立，断言会失败（连接本身也可能触发日志/计费）。

2. **LDAP filter 必须转义**：`username` 拼进 `(uid={username})` 前要转义
   `\ * ( ) NUL` 五个字符（RFC 4515）。不转义 = **LDAP 注入**，
   可构造 `*` 或 `)(uid=*` 绕过匹配。这是 LDAP 集成第二经典的漏洞（第一是空密码）。

3. **`simple_bind` 的两种失败要归一**：
   - 用户不存在（search 无结果）
   - 密码错（bind 返回 `invalidCredentials`）
   两者**必须**返回同一个 `401 SSO_LDAP_BIND_FAILED`（AC-S-104 步骤 2/3）。

### 5.2 连接策略

**每次登录新建连接**，不做连接池。理由：
- `ldap3` 的 `LdapConnAsync` + `drive!` 起一个后台任务，用完即弃，实现简单；
- 连接池要处理超时、重连、并发上限 —— 是一块独立能力，本轮无需求；
- 登录是低频操作（不是每秒千次），连接开销可接受。
- **登记技术债**：高频登录场景需要连接池。

---

## 6. 错误码清单（**21 个**新增，rev 3 定稿）

全部加到 `platform-iam/src/error.rs` 的 `IamError`。

> ⚠️ **rev 3 修正（D125 / QA 的 S36）**：原文档说「加 `ALL_CODES` + `TITLES` 两处」——
> **这个说法是错的，会让人以为改完就对了**。实测新增一个变体要动 **8 个地方**：
> ① code 常量 ② enum 变体 ③ `code()` match ④ `detail()` match ⑤ `ALL_CODES` 数组
> ⑥ `platform-core/src/titles.rs` 的 `TITLES` ⑦ `error.rs` 测试的 `variants` 数组
> ⑧ `error.rs` 测试的 `errors` 数组。
> 漏 ⑦ 红在「清单里有码从未产出」，漏 ⑧ 红在「detail 非 ASCII」。

**本轮 21 个码 → `TITLES.len()` 期望值 = 58（基线）+ 21 = 79。**

| # | code | HTTP | title（ASCII） | 何时 |
|---|---|---|---|---|
| 1 | `SSO_REQUIRED` | 403 | `SSO login is required` | 强制 SSO，本地登录被拒（**在密码校验前**） |
| 2 | `SSO_PROVIDER_NOT_CONFIGURED` | 400 | `SSO provider is not configured` | 租户未配**或已禁用**（合并，不暴露内部状态） |
| 3 | `SSO_CONFIG_INVALID` | 400 | `SSO configuration is invalid` | **rev 3 新增**：配置值非法（`ldaps://` / 相对 redirectUri / filter 缺占位符 / 空 issuer） |
| 4 | `SSO_OIDC_DISCOVERY_FAILED` | 503 | `SSO discovery endpoint is unreachable` | 拿不到 `.well-known` |
| 5 | `SSO_OIDC_DISCOVERY_INVALID` | 502 | `SSO discovery document is invalid` | 拿到了但不合法 / 缺端点 |
| 6 | `SSO_OIDC_STATE_INVALID` | 400 | `SSO state is invalid` | 不存在 / 已消费 / 租户或 provider 不匹配 |
| 7 | `SSO_OIDC_STATE_EXPIRED` | 400 | `SSO state has expired` | 超过 TTL |
| 8 | `SSO_OIDC_CALLBACK_PARAM_MISSING` | 400 | `SSO callback parameter is missing` | **rev 3 新增**：缺 `code` 或 `state` |
| 9 | `SSO_OIDC_TOKEN_EXCHANGE_FAILED` | 400 | `SSO token exchange failed` | **我们发错了**：code 伪造 / 已用 / PKCE 不符 |
| 10 | `SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE` | 502 | `SSO token endpoint is unavailable` | **rev 3 新增**（D124 决策④）：**IdP 挂了 / 超时** |
| 11 | `SSO_OIDC_TOKEN_INVALID` | 401 | `SSO ID token is invalid` | 签名 / iss / aud / iat 不匹配 |
| 12 | `SSO_OIDC_TOKEN_EXPIRED` | 401 | `SSO ID token has expired` | `exp` 已过 |
| 13 | `SSO_OIDC_ALG_NOT_ALLOWED` | 401 | `SSO ID token algorithm is not allowed` | `none` / `HS*` / 白名单外 |
| 14 | `SSO_OIDC_NONCE_MISMATCH` | 401 | `SSO nonce does not match` | `nonce` 不符 |
| 15 | `SSO_OIDC_PKCE_MISMATCH` | 400 | `SSO PKCE verification failed` | `code_verifier` 与 challenge 不符 |
| 16 | `SSO_LDAP_UNREACHABLE` | 503 | `LDAP server is unreachable` | 连不上 |
| 17 | `SSO_LDAP_BIND_FAILED` | 401 | `LDAP authentication failed` | 密码错**或**用户不存在（**同一个码**） |
| 18 | `SSO_LDAP_EMPTY_PASSWORD` | **400** | `LDAP password must not be empty` | 空 / 纯空白密码。**rev 3 从 401 改 400**（D124 决策③） |
| 19 | `SSO_IDENTITY_LINK_CONFLICT` | 409 | `SSO identity conflicts with an existing account` | **rev 3 合并 + 扩充**：绑定改指 / `mail` 撞他人绑定 / JIT 撞既有 username（原 `SSO_LDAP_IDENTITY_CONFLICT` 并入） |
| 20 | `SSO_ACCOUNT_NOT_LINKED` | 403 | `SSO account is not linked` | 未绑定且 JIT 关闭 |
| 21 | `SSO_TICKET_INVALID` | 401 | `SSO login ticket is invalid` | **rev 3 新增**（P4）：ticket 不存在 / 已用 / 过期 |

> **`SSO_OIDC_TLS_UNSUPPORTED` 已删除**（D123 决定支持 https）。
> 若 Dev 认为某码可以合并，**必须在自验报告里说明并更新 acceptance**，不得静默减少。

---

## 7. 配置项（登记进 `platform-config/manifest.json`）

全部 `scope: tenant`（SSO 是租户级能力），`type: string` 或 `boolean`。

| key | 类型 | 默认 | 说明 |
|---|---|---|---|
| `sso.enforce` | boolean | `false` | 强制 SSO：禁用本地密码登录 |
| `sso.jitProvisioning` | boolean | `false` | 未绑定时是否自动建号 |
| `sso.oidc.enabled` | boolean | `false` | OIDC 开关 |
| `sso.oidc.issuer` | string | `""` | IdP issuer（用于发现） |
| `sso.oidc.clientId` | string | `""` | 非敏感，可读 |
| `sso.oidc.scopes` | string | `"openid email profile"` | 空格分隔 |
| `sso.oidc.redirectUri` | string | `""` | 必须是**绝对 URL**，且与 IdP 侧登记逐字一致 |
| `sso.ldap.enabled` | boolean | `false` | LDAP 开关 |
| `sso.ldap.url` | string | `""` | `ldap://host:389`。**rev 3 说明（S43）**：`ldaps://` 本轮**仍不支持**，理由**不是**「树里没有 TLS」（D123 已引入 TLS 栈），而是：`ldap3` 的 `tls-rustls` feature 需要单独启用并配置客户端证书校验，**属于 LDAP 侧的独立工作量**，本轮不扩范围。配置成 `ldaps://` → `400 SSO_CONFIG_INVALID` |
| `sso.ldap.baseDn` | string | `""` | search 起点 |
| `sso.ldap.userSearchFilter` | string | `"(uid={username})"` | **必须含 `{username}` 占位符**，否则匹配不到 |
| `sso.ldap.mailAttribute` | string | `"mail"` | 映射到本地用户的属性名 |

**`client_secret` 不在本表**（AC-S-106）。来源：环境变量 `BASE_SSO_OIDC_CLIENT_SECRET`。

**`format` 校验**：本轮**不加**新 `format` 枚举 —— `issuer` / `redirectUri` / `ldap.url` 的格式校验
放在 `service` 层的 `validate()` 里（与 `platform-org` 的 `validate_ldap_url` 同风格）。
理由：`format` 是 REQ-021 为「固定枚举」建的机制（locale / iana-timezone / iso4217），
URL 校验不是枚举，塞进去会污染那个机制的语义。

---

## 8. 代码落点（Dev 按此切文件，避免冲突）

> **rev 3 修正（QA 的 S22 / S35 / S44 / S49）**：本表补了 **3 处 rev 2 漏掉的落点**，
> 每一处漏掉都会导致「AC 写了但实现没做，或做在运行期炸」：
> ① `SessionView` + 两条 SELECT（漏了 → `GET /api/iam/sessions` **运行期 500**，不是编译错）
> ② `SsoLdapClient` trait（漏了 → 「0 条 bind」断言**没有观测面**）
> ③ `SsoAuditRecorder` 自建（漏了 → 照搬 org 的 recorder 会**成环**且 payload 不存在）

### 后端（`crates/platform-iam/`）

| 文件 | 动作 | 内容 |
|---|---|---|
| `src/sso/mod.rs` | 新建 | 模块根 + 公共类型（`SsoProviderView`、`SsoConfig`） |
| `src/sso/oidc.rs` | 新建 | 发现、JWKS（含未知 `kid` 刷新）、换 token、验签、state 管理 |
| `src/sso/ldap.rs` | 新建 | **`SsoLdapClient` / `SsoLdapConn` trait**（§5.2）+ `ldap3` 实现 + filter 转义 + 空密码拦截 |
| `src/sso/binding.rs` | 新建 | `external_identity` **`DO NOTHING` + 读回比对**（§2.3，**不是** `DO UPDATE`）+ JIT |
| `src/sso/audit.rs` | 新建 | **`SsoAuditEvent` + `SsoAuditRecorder`（自建，不依赖 org）** + `Auditable` 声明 |
| `src/sso/config.rs` | 新建 | 从 config 读 SSO 配置 + `validate()` + **读取失败 fail-closed** |
| `src/sso/ticket.rs` | 新建 | **rev 3 新增**：loginTicket 的生成 / 消费（P4） |
| `src/http.rs` | 改 | 加 `sso_router(state)`，**5 个端点**（providers / authorize / callback / ldap.login / **exchange**） |
| `src/error.rs` | 改 | **21 个**新变体 + `code()` / `detail()` / `ALL_CODES` + 两个测试数组（共 8 处，见 §6） |
| `src/service.rs` | 改 | `login()` 开头插强制 SSO 判定（**加载用户之前**）；新增 `login_via_sso()` 复用 `issue_session`；**`SessionView` 加 `authProvider` 字段**；`with_sso_audit_recorder()` 后设槽 |
| `src/store.rs` | 改 | `oidc_state` / `sso_ticket` 的 insert / consume；**session 插入带 `auth_provider`**；**两条会话 SELECT 必须补该列**（否则 `tenant_row!` 运行期报列不存在） |
| `src/lib.rs` | 改 | 导出 `sso` 模块 |

### 数据库（`migrations/`）

| 文件 | 动作 |
|---|---|
| `2026xxxx_iam_sso.up.sql` / `.down.sql` | `platform_iam_oidc_state` + `platform_iam_sso_ticket` 建表；`platform_iam_session.auth_provider` 加列；**两张新表都要 `GRANT` 给应用角色**（S37，否则 `permission denied`）；down 可逆（`DROP COLUMN` 但**不删表**） |

### 平台侧

| 文件 | 动作 |
|---|---|
| `crates/platform-core/src/titles.rs` | 改：**21 个**新码的 ASCII title（`TITLES.len()` 58 → **79**） |
| `crates/platform-config/manifest.json` | 改：12 个配置键登记（`scope: tenant`） |
| `crates/platform-server/src/main.rs` | 改：nest `sso_router`（**只在 IAM 就绪时挂**） |
| `crates/platform-iam/Cargo.toml` | 改：见下方「依赖变更」 |

### 依赖变更（rev 3 拆成两类，修 S44 / S49）

**A. 已在 `Cargo.lock` 里 —— 只加 `[dependencies]` 引用，包数不变：**

| crate | 备注 |
|---|---|
| `ldap3` | 由 `platform-org` 引入，锁文件已有 0.11.5 |
| `hyper` / `hyper-util` / `http` | SCIM 出站已在用 |
| `base64` / `sha2` | **`platform-iam` 已经依赖了**，无需再加（S44） |

**B. D123 破例新增 —— 会进锁文件，增量必须实测报告：**

| crate | 用途 |
|---|---|
| `rustls` + `ring` | TLS 实现（生产） |
| `hyper-rustls` | hyper 的 rustls connector |

**C. 只进 `[dev-dependencies]`（不进生产依赖树，D124 决策②）：**

| crate | 用途 |
|---|---|
| `tokio-rustls` / `axum-server` | mock IdP 的**服务端** TLS |
| `rcgen` | 测试根证书生成 |

### 前端

| 文件 | 动作 |
|---|---|
| `apps/shell/src/pages/Login.svelte` | 改：增加「用企业账号登录」入口 |
| `apps/shell/src/pages/SsoCallback.svelte` | **rev 3 新增**：接 `?ticket=`，调 `/sso/exchange` 换会话（P4） |
| `apps/shell/src/components/SsoButtons.svelte` | 新建 |
| `apps/shell/src/lib/sso-api.ts` | 新建 |
| `apps/shell/src/lib/routes.ts` | 改：加 `/sso/callback` 路由 |
| `packages/i18n/src/locales/{zh-CN,en-US}.json` | 改：加 SSO 相关键（**中英键集合必须相等**） |

### 测试

| 文件 | 内容 |
|---|---|
| `crates/platform-iam/tests/sso_oidc.rs` | OIDC 全链路（进程内 axum mock IdP） |
| `crates/platform-iam/tests/sso_oidc_security.rs` | AC-S-102：`alg=none` / HS 混淆 / 篡改 payload / **`jku` / `x5c`** |
| `crates/platform-iam/tests/sso_ldap.rs` | AC-S-104：**用 `SsoLdapClient` 的计数实现**断言 bind 次数 / 空密码 0 请求 / filter 注入 |
| `crates/platform-iam/tests/sso_binding.rs` | AC-S-103：JIT / 禁用 / 并发幂等 / **不可改指** |
| `crates/platform-iam/tests/sso_config_secret.rs` | AC-S-106：三处扫描 + 阳性对照 |
| `crates/platform-iam/tests/sso_ticket.rs` | **rev 3 新增**：ticket 一次性 / 过期 |
| `crates/platform-server/tests/sso_real_db.rs` | 真实库端到端（AC-S-100 / 107 / 109） |
| `apps/shell/test/sso.test.ts` | 前端 providers 渲染 + callback 页 |
| `e2e/sso.spec.ts` | 登录页出现企业登录入口 |

## 9. 开工前必答项（Dev 不得自行决定）

| # | 问题 | 选项 | BA 倾向 |
|---|---|---|---|
| 1 | ~~**IdP 的 https 支持**~~ | ~~(A) 加 TLS 依赖 / (B) 只支持 http~~ | ✅ **已定（D123，用户批准）**：**加 TLS 依赖支持 https**。只换 connector 不引 `reqwest`；`https_or_http()` 两者都能用；包数增量 / 编译耗时 / 低配机可编性必须实测报告 |
| 2 | **mock IdP 怎么起** | 用 axum 在测试进程内起 / 用外部容器 | **进程内 axum**（零外部依赖、更快、可在 CI 无网络环境跑） |
| 3 | ~~**LDAP 测试替身**~~ | ~~内存替身~~ | ✅ **rev 3 已定（QA 的 S6/S7）**：`ldap3` **没有**服务端/内存实现，`platform-org` 的 `InMemoryLdapSource` **只实现 `search`、不能测 bind** —— 原「内存替身」是**错误假设**。改为 **`SsoLdapClient` trait + 可计数的测试实现**（§5.2） |
| 4 | **LDAP 是否用服务账号 search** | 用 / 不用 | **用**（P13）：不改的话无法按 filter 从用户名找 DN。代价是**每次登录 2 次 bind**，AC-S-104 步骤 8 的计数期望依赖它 |
| 5 | **mock IdP 的 https 怎么起** | 加 `[dev-dependencies]` 做服务端 TLS / 外部反代终止 TLS | **加 dev-dependencies**（D124 决策②）：用 `tokio-rustls`/`axum-server`，**不进生产依赖树**，与 REQ-021 加 `tower`/`http-body-util` 同做法。外部反代要额外起进程，低配机不划算 |

---

## 10. 低配机注意事项

本机是 8 线程 i7-8550U / 7.7G，已有多条硬约束：

- `.cargo/config.toml` 固定 `jobs = 3`
- `cargo test` **逐 crate** 跑（`--workspace` 必失败）
- `cargo test -p <crate>` 必须带 `--all-targets`
- 后台服务用 `setsid … & disown`，停止用 `ss -tlnp` 取 pid（**不要** `pkill -f`）

### ⚠️ rev 3 修正（QA 的 S42）：编译成本**不能沿用旧估计**

rev 2 这里写「全是 `Cargo.lock` 里已有的包，**不会引入新的编译单元**」—— **在 D123 之后这是错的**：

| 依赖 | 是否新编译单元 | 说明 |
|---|---|---|
| `ldap3` / `hyper` / `hyper-util` | 否 | 锁文件里已有 |
| **`rustls` / `ring` / `hyper-rustls`** | **是** | **约 20~30 个新包**；`ring` 还要**编 C/汇编** |
| dev-only（`tokio-rustls` / `axum-server` / `rcgen`） | **是** | 只影响测试构建，但**也进锁文件** |

**所以**：
- 锁文件增量与「首次 `cargo build` 耗时」**必须实测后填在这里**，不得沿用 rev 2 的乐观估计
- `ring` 的汇编编译在 8 线程低配机上可能明显拉长首次构建 —— 若超过可接受范围，
  按 D123 的备选方案改用 `native-tls`（系统 OpenSSL）并回报 BA
- 之后增量编译只重编 `platform-iam`，不受影响

---

## 11. 与 AC 的对应

| AC | 主要落地文件 |
|---|---|
| AC-S-020 协议失败可诊断 | `sso/oidc.rs` / `sso/ldap.rs` / `error.rs` |
| AC-S-021 强制 SSO | `service.rs`（`login` 开头）/ `sso/config.rs` |
| AC-S-100 OIDC 端到端 | `sso/oidc.rs` / `http.rs` / **`sso/ticket.rs`** / **前端 `SsoCallback.svelte`** |
| AC-S-101 state/nonce/PKCE | `sso/oidc.rs` / `store.rs` / 迁移 |
| AC-S-102 验签与算法白名单 | `sso/oidc.rs` |
| AC-S-103 绑定与 JIT | `sso/binding.rs` |
| AC-S-104 LDAP bind | `sso/ldap.rs`（含 **`SsoLdapClient` trait**） |
| AC-S-105 LDAP 与本地共存 | `sso/ldap.rs` / `sso/binding.rs` |
| AC-S-106 密钥保护 | `sso/config.rs` / `error.rs`（不泄露）/ 全链日志 |
| AC-S-107 会话同构 | `service.rs`（`issue_session` + **`SessionView`**）/ `store.rs`（**两条 SELECT**）/ 迁移 |
| AC-S-108 审计事件 | `sso/audit.rs`（**自建 `SsoAuditRecorder`** + `Auditable` 声明） |
| AC-S-109 多 IdP 与租户隔离 | `sso/config.rs` / `sso/oidc.rs`（state 绑租户）|
