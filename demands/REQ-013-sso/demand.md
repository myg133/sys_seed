# 需求：SSO 入站（OIDC + LDAP）与强制 SSO

- **需求编号**: REQ-013
- **父需求**: [REQ-001 通用项目底座](../REQ-001-platform-foundation/demand.md)
- **优先级**: P1
- **状态**: 已就绪（rev 2，2026-11-19 范围收窄）
- **所属迭代**: Sprint 5
- **依赖**: REQ-007（认证与账号）、REQ-009（RBAC）、REQ-010（配置三层）
- **更新时间**: 2026-11-19 23:40

## 需求描述

让租户能接入**自己的企业身份源**登录底座：用户不必在底座里再记一套密码。

本轮只做**入站**（底座作 SP / 认证消费方）。出站（底座作 IdP 给第三方发令牌）、
免登跳转、单点登出**移到 REQ-022**（见「范围收窄」）。

## 范围收窄（2026-11-19 用户裁定）

原始草稿写了「四种入站协议 + 出站三种流程」，本轮按**技术可行性**和**交付节奏**收窄。

### 已确认可做（本轮）

| 能力 | 可行性依据 |
|---|---|
| OIDC / OAuth2 入站（授权码流程） | 树里已有 `hyper`（出站 HTTP，SCIM 已在用）、`jsonwebtoken`（RS256/ES256 验签，rust_crypto 后端）、`serde_json`。**但 TLS 需新增依赖** —— 用户 2026-11-19 批准（**D123**，本项目唯一一次破例） |
| LDAP / AD 入站认证 | 树里已有 `ldap3` 0.11.5（REQ-008 引入，现用于组织同步的 search）；`Ldap::simple_bind(bind_dn, bind_pw)` 可用，但 ⚠️ **返回 `Ok` 不代表认证成功**（`rc != 0` 时外层仍是 `Ok`），**必须**再 `.success()` 判 `rc == 0` → **零新增依赖** |
| 强制 SSO 策略 | 走 REQ-010 已交付的 config 三层覆盖 |
| SSO 会话 | 复用 REQ-007 已交付的 session / refresh family / access token / JWKS |

### 明确不做（本轮）

| 能力 | 为什么不做 | 去哪了 |
|---|---|---|
| **SAML 2.0 入站** | **树里没有任何 XML 库**。SAML 断言是 XML + XMLDSig 签名，验签要求 **C14N 规范化 + 签名引用解析**。手写这套是 **签名包装攻击（XSW）的重灾区** —— 历史上 ADFS/Okta 集成出过的严重漏洞几乎都在这。**不做比做错好** | **不排期**。若将来必须接 SAML，需先单独评估：引成熟 XML 库 + 严格防 XSW，或引 `samael`。这是**新增依赖**决策，要单独拍板 |
| **CAS 入站** | CAS 2.0 返回 **XML 断言**（同样受 XML 库缺失限制）；CAS 1.0 虽是纯文本但仍需兼容 CAS 服务器的 XML 路径 | 不排期 |
| **出站 IdP**（授权码 + PKCE / Client Credentials / Device Code / introspect / 免登跳转） | 技术完全可行（纯 JSON + 已有 JWKS），但**与本轮加起来 AC 会翻倍**，且它服务的是 REQ-016 应用接入 | **REQ-022**（已建骨架） |
| **单点登出**（Back-Channel / Front-Channel Logout） | 依赖出站 IdP 与多应用形态 | **REQ-022** |

> **用户裁定依据（2026-11-19）**：
> - 问题「树里没有 XML 库，SAML 2.0 的签名验证做不了。怎么定？」→ **「本轮不做 SAML，只做 OIDC + LDAP」**
> - 问题「SSO 范围很大…要拆吗？」→ **「拆两棒，本轮先做入站」**
>
> OIDC 已覆盖 Azure AD / Okta / Keycloak / Auth0 / Google Workspace；
> LDAP 覆盖 AD / OpenLDAP。**这两条覆盖了绝大多数企业身份源**。

## 功能要求

### A. OIDC / OAuth2 入站（授权码流程）

- 发起：`GET /api/iam/sso/oidc/authorize` → 302 到 IdP 授权端点
  - 必带 `client_id` / `redirect_uri` / `response_type=code` / `scope=openid ...`
  - 必带 `state`（CSRF，一次性）、`nonce`（重放防护）、`code_challenge` + `code_challenge_method=S256`（PKCE）
- 回调：`GET /api/iam/sso/oidc/callback?code=…&state=…`
  - 校验 `state`（存在、未用、未过期、属于同一租户 + 同一 provider）
  - 用 `code` + `code_verifier` 换 `id_token` / `access_token`（向 IdP 的 token 端点）
  - 验 `id_token` 签名与声明
  - 解析 `sub` / `email` / `name` → 走账号绑定
  - 建与本地登录**同构**的会话
- IdP 元数据：从 `{issuer}/.well-known/openid-configuration` 发现授权/token/JWKS 端点
- **JWKS 缓存**：按 `kid` 取公钥；遇到**未知 `kid`** 触发一次刷新（IdP 轮换密钥时不中断登录）

### B. LDAP / AD 入站认证

- 认证即 **bind**：用用户 DN + 密码 `simple_bind` 到目录服务
- **空密码必须显式拒绝** —— 见「安全约束」§1
- 用户 DN 解析：按配置的 `userSearchFilter` 先 search 出 DN，再 bind（避免要求用户知道自己的 DN）
- bind 成功后按 `mail` / `sAMAccountName` 等映射字段匹配本地用户
- **密码不落库**：LDAP 用户的密码永远只在目录服务里

### C. 强制 SSO 策略

- 租户级配置 `sso.enforce`（boolean，默认 `false`）
- 开启后：
  - `POST /api/iam/login` 一律拒绝，返回 **403 `SSO_REQUIRED`**
  - **在密码校验之前拒绝**（见「安全约束」§2）
  - 响应带可发现 SSO 入口的信息（`ssoProviders`），让前端能引导
- 关闭时：本地密码登录与 SSO 登录**并存**

### D. 账号绑定与 JIT 建号

- 绑定表复用 REQ-008 已交付的 `external_identity(tenant_id, provider, external_id, user_id)`，
  唯一键 `(tenant_id, provider, external_id)` 已在
- 已绑定 → 直接登录到该 user
- 未绑定：
  - `sso.jitProvisioning = true`（默认 `false`）→ 建 user + 建绑定
  - 否则 → **403 `SSO_ACCOUNT_NOT_LINKED`**，不自动建号
- 绑定到已禁用的 user → 拒绝（`ACCOUNT_DISABLED`），不因 SSO 绕过禁用

### E. SSO 配置与密钥

- **非敏感配置**走 REQ-010 的 config（`scope: tenant`）：
  `sso.oidc.enabled` / `sso.oidc.issuer` / `sso.oidc.clientId` / `sso.oidc.scopes` /
  `sso.oidc.redirectUri` / `sso.enforce` / `sso.jitProvisioning` /
  `sso.ldap.enabled` / `sso.ldap.url` / `sso.ldap.baseDn` / `sso.ldap.userSearchFilter` / `sso.ldap.mailAttribute`
- **`client_secret` 绝不进 config** —— 见「安全约束」§3。
  来源二选一：
  - 环境变量 `BASE_SSO_OIDC_CLIENT_SECRET`（平台级，单 IdP 部署）
  - 租户级加密列（用 REQ-007 已交付的 `crypto::encrypt`，ChaCha20-Poly1305，
    密钥来自 `BASE_IAM_SECRET_KEY`）—— **本轮只做环境变量**，加密列留接口

## 安全约束（每条都必须在 AC 里有对应断言）

### §1 LDAP 空密码必须显式拒绝

`ldap3` 的 `simple_bind(dn, "")` 在多数目录服务上等价于 **anonymous bind** →
**返回成功**。若不显式拦截，攻击者用「任意用户名 + 空密码」即可登录。
这是 LDAP 集成最经典的漏洞。判据：`password.is_empty() || password.trim().is_empty()` → 直接拒绝，
**不发出 bind 请求**（断言 IdP 侧无请求）。

### §2 强制 SSO 必须在密码校验之前拒绝

若先校验密码再拒，则：
- 攻击者能借「403 vs 401」的差异做**账号枚举**
- 每轮 SSO 强制期都白跑一次 Argon2id（低配机上这是真实的 CPU 成本）

判据：`sso.enforce=true` 时，`login` 分支在**加载用户之前**就返回 403。

### §3 `client_secret` 不得出现在任何可读位置

`platform_config.value` 是**明文 JSONB**，且 `GET /api/config/entries` 会把它读出来。
因此 `client_secret`：不进 config 表、不进任何响应体、不进日志、不进错误详情。
判据：AC-S-106 用「配置全量导出 + 错误响应体 + 日志捕获」三处扫描，断言 0 命中，
并带**阳性对照**（故意注入一次，断言扫描器能命中）。

### §4 `id_token` 验签不得接受 `alg: none`，不得无条件接受对称算法

- `alg` 走**白名单**（默认只允许 `RS256` / `ES256`）
- 禁止 `none`
- 禁止把 IdP 的公钥当 HMAC 密钥做「RS→HS 混淆」（经典混淆攻击）
判据：AC-S-102 用「`alg=none` 的伪造 token」「HS256 + 公钥内容签名」两个攻击样例，断言均被拒。

### §5 SSO 不得绕过账号禁用与租户隔离

- 绑定的 user 被禁用 → 拒绝
- `state` 绑定租户与 provider，跨租户重放 → 拒绝
判据：AC-S-103、AC-S-109。

### §6 审计事件不含敏感值

SSO 审计事件只记：provider、外部 subject、结果、错误码、IP、UA。
**不含** token、密码、client_secret、完整 claim 集。
判据：AC-S-108 对事件 payload 做同样的敏感值扫描。

### §7 LDAP filter 必须按 RFC 4515 转义（rev 3 新增）

`username` 拼进 `(uid={username})` 前必须转义五个字符：`\`、`*`、`(`、`)`、`NUL`。
不转义 = **LDAP 注入**：用户名填 `*` 或 `)(uid=*` 可绕过匹配。
这是 LDAP 集成的第二经典漏洞（第一是 §1 的空密码）。
判据：AC-S-104 步骤 9 用三个注入载荷，断言均不产生越权结果。

### §8 token 不得进 URL（rev 3 新增）

回调是顶层浏览器导航，**不得**把 `accessToken` / `refreshToken` 放进 URL
（会进浏览器历史、`Referer` 头、以及服务端访问日志）。
判据：AC-S-100 预期 3 —— 回调只 302 携带**一次性 ticket**，token 只经
`POST /api/iam/sso/exchange` 的响应体返回。

## 与既有交付物的关系（跨需求改动必须声明，D109）

| 改什么 | 为什么 | 是否破坏既有契约 |
|---|---|---|
| `platform-iam` 新增错误码（**21 个**，rev 3 定稿） | AC-S-020 要求可区分 | **否**，但**必须改 8 处**（D125）：code 常量 / enum 变体 / `code()` / `detail()` / `ALL_CODES` / `TITLES` / 测试的两个数组。漏一处就红在 `i18n_titles.rs` 或 `error.rs` 的测试里。`TITLES.len()` 期望 **79** |
| `platform-iam` 新增 `sso` 模块 + 迁移（2 张表） | `oidc_state`（state/nonce/verifier 一次性凭证）、`sso_identity_link` 审计辅助（可选，若复用 `external_identity` 则只要 `oidc_state`） | 否（纯新增） |
| `platform-iam` 依赖 `ldap3` | LDAP bind 认证 | 否（`ldap3` 已在 `Cargo.lock`，由 `platform-org` 引入） |
| **新增 TLS 依赖（`rustls` + `ring` + `hyper-rustls`）** | **真实 IdP 全是 https，没有 TLS 的 OIDC 在真实环境不可用** | **依赖预算破例** —— 用户 2026-11-19 明确批准（**D123**）。`Cargo.lock` 包数会从 **296** 增加，**具体增量必须实测报告**。只换 connector，**不引 `reqwest`** |
| `platform-config/manifest.json` 新增约 12 个键 | 非敏感 SSO 配置 | 否（纯新增登记） |
| `packages/i18n` 新增 SSO 相关错误文案键 | AC-S-089 要求前端能渲染完整文案 | 否（纯新增键，中英键集合仍须相等） |
| `apps/shell` 登录页增加「用企业账号登录」入口 + **新增 `SsoCallback.svelte` 与 `/sso/callback` 路由** | 回调需要 ticket 交接（D124 决策①） | 否（纯新增） |
| `IamService` 加 `with_sso_audit_recorder()` **后设槽** | 审计事件注入 | 否 —— **用后设槽不破 `new()` 签名**，因此不破坏 4 处既有调用点（含 `platform-rbac` 的 2 处测试） |
| `SessionView` 加 `authProvider` + `store.rs` **两条 SELECT 补该列** | AC-S-107 要求「会话能看出 SSO 来源」 | 否，但**陷阱**：`tenant_row!` 按列名 `try_get`，加 struct 字段却不加进 SELECT 会在**运行期**报列不存在（不是编译错） |

## 不在范围内

- SAML 2.0、CAS（技术上不可行，见「范围收窄」）
- 出站 IdP、免登跳转、单点登出（REQ-022）
- SSO 登录页品牌定制（REQ-010 已提供品牌配置，本轮只消费不扩展）
- 权限申请审批流（审批引擎已取消，D121）
- 审计**落库与查询**（REQ-014；本轮只声明事件 + 走 `AuditRecorder` 接缝）
- 多 IdP 并存的**管理界面**（本轮只支持配置层声明 + 至少 OIDC/LDAP 各一个）

## 关联验收标准

父需求 AC：**AC-005**（SSO 入站三协议 → 本轮收窄为 OIDC + LDAP，见下）

> ⚠️ **AC-005 的「三协议」本轮只交付两条**（OIDC + LDAP）。
> SAML 不予交付，原因见「范围收窄」。父 AC 将同步标注，
> 这与 AC-024 的处理方式一致 —— **不交付的部分必须显式说明，不能含混通过**。

本需求自有 AC：**AC-S-020 / AC-S-021（重写）+ AC-S-100 ~ AC-S-109（新增 10 条）= 12 条**

详见 [acceptance.md](acceptance.md)。

## 完成定义

- 租户能配 OIDC 或 LDAP，用户能真实登录并拿到与本地登录同构的会话
- 两种协议的失败**可区分、可排查**，且不泄露敏感值
- 强制 SSO 开关生效，且不能被账号枚举利用
- LDAP 空密码攻击被拒
- `id_token` 的 `alg=none` 与算法混淆攻击被拒
- `client_secret` 在三处扫描（配置导出 / 错误体 / 日志）中 0 命中
- 新增 **21 个**错误码全部按 **8 个编辑点**同步（D125），REQ-021 的完备性测试仍绿（`TITLES.len()` = 79）
- **不需要** `SSO_OIDC_TLS_UNSUPPORTED`（D123 后已删除）
- **依赖预算（D123 破例）**：新增 TLS 依赖是被用户批准的**唯一破例**；
  `Cargo.lock` 包数增量必须实测报告（基线 **296**），且**不得引入 `reqwest`**
