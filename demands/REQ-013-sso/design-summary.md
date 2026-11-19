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
| LDAP 密码校验原语 | `ldap3::Ldap::simple_bind(bind_dn, bind_pw)`（源码 `ldap.rs:238` 已确认存在） | 直接调用 |
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

> 这是**有意的重复**，理由写在这里，避免下一个人以为是疏漏。

---

## 3. 端点设计

全部挂在 `/api/iam/sso/*`（`platform-iam` 的 `http.rs` 里新增 `sso_router`，
由 `platform-server` 的 `main.rs` 与既有 `/api/iam` 一起 nest）。

| 方法 | 路径 | 认证 | 说明 |
|---|---|---|---|
| `GET` | `/api/iam/sso/providers` | **无**（但要租户头） | 当前租户可用的 provider 列表。登录页要在**未登录**时就知道该显示什么按钮 |
| `GET` | `/api/iam/sso/oidc/authorize` | **无**（但要租户头） | 302 到 IdP；落一条 `oidc_state` |
| `GET` | `/api/iam/sso/oidc/callback` | **无** | 收 `code` + `state`；换 token、验签、绑定、建会话 |
| `POST` | `/api/iam/sso/ldap/login` | **无** | body：`{username, password}`；bind 校验后建会话 |

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

`oidc/callback` 与 `ldap/login` 成功 → **与 `POST /api/iam/login` 完全相同的 `LoginResponse`**。
这是 AC-S-107「同构」的直接要求。

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

   > ⚠️ **这是本轮最大的技术风险点**：`hyper-util` 的 `build_http()` 不带 TLS，
   > 而树里**没有 TLS 客户端实现**（`Cargo.toml` 明确写了「不引 reqwest / 不开 TLS」）。
   >
   > **两种解法，Dev 必须二选一并写进自验报告**：
   > - **(A) 用 `hyper-rustls` 或 `tokio-rustls`** → **新增依赖**，违反依赖预算，需 BA 批准
   > - **(B) 本轮只支持 `http://` IdP**（内网 IdP 常用，且 mock 测试也用 http），
   >   `https` 显式拒绝并给 `SSO_OIDC_TLS_UNSUPPORTED` → **零新依赖，但真实云 IdP 接不了**
   >
   > **BA 倾向 (B) + 登记技术债**：本轮的目标是「链路通 + 安全约束到位」，
   > 真实 https IdP 的接入需要一次**明确的依赖预算决策**（用户此前多轮要求零新增依赖）。
   > **Dev 若认为 (B) 不可接受，必须在开工前提出，不得自行加依赖。**
   >
   > → 已登记为本需求的**开工前必答项**（见 §9）。

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
   ├─ 5. 用用户 DN + 用户密码 simple_bind
   │      成功 → 继续；失败 → 401 SSO_LDAP_BIND_FAILED（与「用户不存在」同一个码）
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

## 6. 错误码清单（13 个新增）

全部加到 `platform-iam/src/error.rs` 的 `IamError` + `ALL_CODES`，
**同时**加到 `platform-core/src/titles.rs` 的 `TITLES`（ASCII title）。
漏一处 → `i18n_titles.rs::titles_cover_every_code` 直接红。

| # | code | HTTP | title（ASCII） | 何时 |
|---|---|---|---|---|
| 1 | `SSO_REQUIRED` | 403 | `SSO login is required` | 强制 SSO，本地登录被拒（**在密码校验前**） |
| 2 | `SSO_PROVIDER_NOT_CONFIGURED` | 400 | `SSO provider is not configured` | 租户未配或已禁用该协议 |
| 3 | `SSO_OIDC_DISCOVERY_FAILED` | 503 | `SSO discovery endpoint is unreachable` | 拿不到 `.well-known` |
| 4 | `SSO_OIDC_DISCOVERY_INVALID` | 502 | `SSO discovery document is invalid` | 拿到了但不是合法 JSON / 缺端点 |
| 5 | `SSO_OIDC_STATE_INVALID` | 400 | `SSO state is invalid` | 不存在 / 已被消费 / 租户或 provider 不匹配 |
| 6 | `SSO_OIDC_STATE_EXPIRED` | 400 | `SSO state has expired` | 超过 TTL |
| 7 | `SSO_OIDC_TOKEN_EXCHANGE_FAILED` | 400 | `SSO token exchange failed` | code 换 token 失败（code 伪造/已用） |
| 8 | `SSO_OIDC_TOKEN_INVALID` | 401 | `SSO ID token is invalid` | 签名 / iss / aud / iat 不匹配 |
| 9 | `SSO_OIDC_TOKEN_EXPIRED` | 401 | `SSO ID token has expired` | `exp` 已过 |
| 10 | `SSO_OIDC_ALG_NOT_ALLOWED` | 401 | `SSO ID token algorithm is not allowed` | `none` / `HS*` / 白名单外算法 |
| 11 | `SSO_OIDC_NONCE_MISMATCH` | 401 | `SSO nonce does not match` | `nonce` 与 state 记录不符 |
| 12 | `SSO_OIDC_PKCE_MISMATCH` | 400 | `SSO PKCE verification failed` | `code_verifier` 与 challenge 不符 |
| 13 | `SSO_LDAP_UNREACHABLE` | 503 | `LDAP server is unreachable` | 连不上 |
| 14 | `SSO_LDAP_BIND_FAILED` | 401 | `LDAP authentication failed` | 密码错**或**用户不存在（**同一个码**） |
| 15 | `SSO_LDAP_EMPTY_PASSWORD` | 401 | `LDAP password must not be empty` | **空/纯空白密码**（攻击识别用） |
| 16 | `SSO_LDAP_IDENTITY_CONFLICT` | 409 | `LDAP identity conflicts with an existing account` | 映射字段撞了另一个已绑定账号 |
| 17 | `SSO_ACCOUNT_NOT_LINKED` | 403 | `SSO account is not linked` | 未绑定且 JIT 关闭 |
| 18 | `SSO_OIDC_TLS_UNSUPPORTED` | 501 | `HTTPS IdP endpoints are not supported yet` | 仅当选 §4.2 解法 (B) |

> 表里实际 **18 个**码（acceptance 里说「13 个」是下限估计，**以本表为准**）。
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
| `sso.ldap.url` | string | `""` | `ldap://host:389`（本轮只支持明文，见 §4.2 风险） |
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

### 后端（`crates/platform-iam/`）

| 文件 | 动作 | 内容 |
|---|---|---|
| `src/sso/mod.rs` | 新建 | 模块根 + 公共类型（`SsoProviderView`、`SsoConfig`） |
| `src/sso/oidc.rs` | 新建 | 发现、JWKS、换 token、验签、state 管理 |
| `src/sso/ldap.rs` | 新建 | bind 认证、filter 转义、空密码拦截 |
| `src/sso/binding.rs` | 新建 | `external_identity` upsert（含 JIT） |
| `src/sso/audit.rs` | 新建 | `AuditEventSpec` 声明 + `AuditRecorder` 接缝 |
| `src/sso/config.rs` | 新建 | 从 config 读 SSO 配置 + `validate()` |
| `src/http.rs` | 改 | 加 `sso_router(state)`，4 个端点 |
| `src/error.rs` | 改 | 18 个新变体 + `code()` / `detail()` / `ALL_CODES` |
| `src/service.rs` | 改 | `login()` 开头插强制 SSO 判定；新增 `login_via_sso()` 复用 `issue_session` |
| `src/store.rs` | 改 | `oidc_state` 的 insert / consume；session 插入时带 `auth_provider` |
| `src/lib.rs` | 改 | 导出 `sso` 模块 |

### 数据库（`migrations/`）

| 文件 | 动作 |
|---|---|
| `2026xxxx_iam_sso.up.sql` / `.down.sql` | `platform_iam_oidc_state` 建表 + `platform_iam_session.auth_provider` 加列；down 可逆 |

### 平台侧

| 文件 | 动作 |
|---|---|
| `crates/platform-core/src/titles.rs` | 改：18 个新码的 ASCII title |
| `crates/platform-config/manifest.json` | 改：12 个配置键登记 |
| `crates/platform-server/src/main.rs` | 改：nest `sso_router`（**只在 IAM 就绪时挂**，与既有逻辑一致） |
| `crates/platform-iam/Cargo.toml` | 改：加 `ldap3` / `hyper` / `hyper-util` / `http` / `base64` / `sha2`（**全部已在 `Cargo.lock`**） |

### 前端

| 文件 | 动作 |
|---|---|
| `apps/shell/src/pages/Login.svelte` | 改：增加「用企业账号登录」入口（按 `/api/iam/sso/providers` 渲染） |
| `apps/shell/src/components/SsoButtons.svelte` | 新建 |
| `apps/shell/src/lib/sso-api.ts` | 新建 |
| `packages/i18n/src/locales/{zh-CN,en-US}.json` | 改：加 SSO 相关键（**中英键集合必须相等**） |

### 测试

| 文件 | 内容 |
|---|---|
| `crates/platform-iam/tests/sso_oidc.rs` | OIDC 全链路（mock IdP，用 axum 起本地假 IdP） |
| `crates/platform-iam/tests/sso_oidc_security.rs` | AC-S-102 的攻击样例（alg=none / HS 混淆 / 篡改 payload） |
| `crates/platform-iam/tests/sso_ldap.rs` | AC-S-104（含空密码 0 请求断言） |
| `crates/platform-iam/tests/sso_binding.rs` | AC-S-103（JIT / 禁用 / 并发幂等） |
| `crates/platform-iam/tests/sso_config_secret.rs` | AC-S-106（三处扫描 + 阳性对照） |
| `crates/platform-server/tests/sso_real_db.rs` | 真实库端到端（AC-S-100 / 107 / 109） |
| `apps/shell/test/sso.test.ts` | 前端 providers 渲染 |
| `e2e/sso.spec.ts` | 登录页出现企业登录入口（mock IdP 在 E2E 里起） |

---

## 9. 开工前必答项（Dev 不得自行决定）

| # | 问题 | 选项 | BA 倾向 |
|---|---|---|---|
| 1 | **IdP 的 https 支持** | (A) 加 TLS 依赖违反预算 / (B) 只支持 http + `SSO_OIDC_TLS_UNSUPPORTED` + 技术债 | **(B)**。真实 https IdP 需要一次明确的依赖预算决策，本轮先保证链路与安全约束到位 |
| 2 | **mock IdP 怎么起** | 用 axum 在测试进程内起 / 用外部容器 | **进程内 axum**（零外部依赖、更快、可在 CI 无网络环境跑） |
| 3 | **LDAP 测试替身** | 真实 OpenLDAP 容器 / `Ldap3Source` 的内存替身 | **内存替身**（`platform-org` 已有 `InMemoryLdapSource` 先例）；但**空密码「0 请求」断言需要能观察请求**，替身要记录 bind 调用 |

---

## 10. 低配机注意事项

本机是 8 线程 i7-8550U / 7.7G，已有多条硬约束：

- `.cargo/config.toml` 固定 `jobs = 3`
- `cargo test` **逐 crate** 跑（`--workspace` 必失败）
- `cargo test -p <crate>` 必须带 `--all-targets`
- 后台服务用 `setsid … & disown`，停止用 `ss -tlnp` 取 pid（**不要** `pkill -f`）

本需求新增的编译成本主要来自给 `platform-iam` 加 `ldap3` / `hyper` 依赖 ——
全是 `Cargo.lock` 里已有的包，**不会引入新的编译单元**，但会重编 `platform-iam`。
预计首次 `cargo test -p platform-iam` 需要几分钟（可接受）。

---

## 11. 与 AC 的对应

| AC | 主要落地文件 |
|---|---|
| AC-S-020 协议失败可诊断 | `sso/oidc.rs` / `sso/ldap.rs` / `error.rs` |
| AC-S-021 强制 SSO | `service.rs`（`login` 开头）/ `sso/config.rs` |
| AC-S-100 OIDC 端到端 | `sso/oidc.rs` / `http.rs` |
| AC-S-101 state/nonce/PKCE | `sso/oidc.rs` / `store.rs` / 迁移 |
| AC-S-102 验签与算法白名单 | `sso/oidc.rs` |
| AC-S-103 绑定与 JIT | `sso/binding.rs` |
| AC-S-104 LDAP bind | `sso/ldap.rs` |
| AC-S-105 LDAP 与本地共存 | `sso/ldap.rs` / `sso/binding.rs` |
| AC-S-106 密钥保护 | `sso/config.rs` / `error.rs`（不泄露）/ 全链日志 |
| AC-S-107 会话同构 | `service.rs`（`issue_session`）/ 迁移（`auth_provider`）|
| AC-S-108 审计事件 | `sso/audit.rs` |
| AC-S-109 多 IdP 与租户隔离 | `sso/config.rs` / `sso/oidc.rs`（state 绑租户）|
