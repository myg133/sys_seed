# 决策记录：REQ-022 出站 IdP 与单点登出

- **owner**: myg133-ba-1
- **状态**: rev 1（2026-11-20 —— BA 规格阶段拍板；QA 挑刺后追加 rev 2）
- **关联**: [demand.md](demand.md) / [acceptance.md](acceptance.md) / [design-summary.md](design-summary.md)
- **决策编号段**: **D196 ~ D220**（REQ-014 占 D130~D195）

---

## 0. 需求来源披露（D122 / dev-guide §9.1）

**本需求的需求来源是「用户原话」，不是 BA 自补。** 已从会话快照逐字核实：

> 3. sso 多协议适配
> 9. 预留应用接入能力

- **原话 #3** → 拆两棒：入站（REQ-013，已交付）+ **出站（本需求）**。
- **原话 #9** → REQ-016「应用接入与应用注册表」（依赖本需求）。
- ⚠️ OAuth2 的具体形态（4 种 grant / introspect / 免登 / 单点登出）**全部是 BA 的设计**，
  用户原话里没有 → **范围类决定已回用户**（2026-11-20 22:18）：
  > 「**全做，但是拆成小任务，如果可以并行，可以让多个子 agent 一块来做。**」
- 「拆两棒」那次裁定的**原始问卷回执未找到**（`2026/11/19` 的 messages/snapshots 均无
  `questionnaire-response`）→ 记为「仓内文档记载，原始回执未找到」，不影响本需求的原话来源。

---

## D196: `api-conventions` 升 **v3**，C1 增加 `/api/oauth` 前缀

- **背景**：契约 C1（`BA/contracts/api-conventions/v1.md:22-42`）原文：
  > 「未分配前缀的分组在新增前须先改本契约升版本。」

  C1 的分配表（`:26-40`）**没有** `/api/oauth`。当前版本是 **v2**（`CURRENT` → `v2`），
  且 v2 声明「**C1~C8、C11~C13 一字未改**」。

- **决定**：
  1. 新建 `BA/contracts/api-conventions/v3.md`，**只在 C1 的表里加一行**
     `| /api/oauth | OAuth2 授权服务器（IdP 侧）、令牌内省与撤销 | REQ-022 |`；
     **C2~C13 一字不动** → v3 是**纯新增**，不破坏任何消费方。
  2. v2 标 `deprecated`（保留一个 Sprint），`CURRENT` 指针改 `v3`。
  3. `BA/contracts/INDEX.md` 的版本沿革表加一行。
  4. 端点前缀统一为 **`/api/oauth/**`**（平台统一前缀风格），
     元数据端点按 RFC 8414 固定在 **`/.well-known/oauth-authorization-server`**
     （**不在** `/api` 下 —— 那是 RFC 规定的固定路径）。

- **为什么不用 `/api/iam/oauth`**：C1 把 `/api/iam` 划给 REQ-007（账号、认证、令牌、会话）。
  OAuth 授权服务器是**独立能力域**（有自己的表、client 模型、权限点），
  塞进 `/api/iam` 会让 REQ-007 的前缀语义失真。

- **判定**：AC-S-134 断言元数据里 `issuer` 与各端点路径**逐字等于**契约 v3 登记的值。

---

## D197: 授权服务器放**新 crate `platform-oauth`**，不复用 `platform-iam`

- **决定**：新增 workspace 成员 `crates/platform-oauth`（`Cargo.toml` 的 `members` 加一行 +
  `[workspace.dependencies]` 加 `platform-oauth = { path = "crates/platform-oauth" }`）。

- **依赖方向**（单向，无环）：

  ```
  platform-core  ←  platform-oauth  ←  platform-server（装配层）
        ↑                ↑
   platform-db      platform-iam
  ```

  - `platform-oauth` 依赖：`platform-core`（trait / 错误体 / 上下文）+
    `platform-db`（租户作用域数据访问）+ `platform-iam`（签发密钥 + 用户会话校验）。
  - `platform-oauth` **不依赖** `platform-rbac`：
    「用户有哪些有效权限点」通过 `platform-core` 的 `PermissionProvider` trait
    （`crates/platform-core/src/extension/permission.rs:239`）拿 → **装配层注入**，
    与 REQ-011 已有的做法一致（`crates/platform-server/src/main.rs:395`）。

- **为什么不用 `platform-iam`**：`platform-iam` 已经 8 个模块、21 个错误码；
  OAuth 有 5 张表、20+ 个端点、自己的权限点与 manifest → 塞进去会让 IAM 的
  「账号体系」职责失焦，且 manifest / 权限点 / 启动对账都以 crate 为单位。

- **新增第三方依赖**：**0 个**。用到的 `sha2` `base64` `serde` `serde_json`
  `jsonwebtoken` `time` `hyper` `hyper-util` `hyper-rustls` `rustls` **全部已在 `Cargo.lock`**。

---

## D198: 只发 **JWT access token**（ES256），不做 opaque token

- **决定**：
  1. access token = **JWT**，ES256 签发，`kid` 与登录令牌同一套密钥集
     （`crates/platform-iam/src/jwt.rs:28` `SIGNING_ALGORITHM`）。
  2. `aud` = **`client_id`**（不是平台 audience）→ 第三方应用可用自己的 `aud` 过滤。
  3. `iss` = 平台 issuer（与 `JwtSettings.issuer` 同源）。
  4. claims：`iss` / `aud` / `sub` / `tid` / `client_id` / `scope` / `grant_type` /
     `iat` / `nbf` / `exp` / `jti`。
     - `sub` = 用户 ID（`client_credentials`  grant 时**省略** `sub`）。
     - `tid` = 授权时的租户 ID。
     - `scope` = **空格分隔**的权限点码（RFC 6749 §3.3）。
  5. **opaque token 不做** → 但**内省端点必须做**（它校验的是 JWT：验签 + `exp` +
     查 grant / token 状态）。

- **为什么**：复用现有 JWKS（`crates/platform-iam/src/service.rs:544` `IamService::jwks()`）
  → 第三方零配置即可验签，**零新增状态**（不需要令牌表来「查得到才能验」）。
  代价是撤销不能只靠验签 → 由 D210 的双轨语义兜住。

- **不做的代价**：不透明令牌天然可即时撤销，但需要一张令牌表 + 每次校验一次查库
  （高频接口上不可接受）→ 换成「JWT 验签 + 查 grant 状态」的组合。

---

## D199: OAuth2 client 模型归 **REQ-022**，REQ-016 引用并扩展

- **决定**：REQ-022 建 **`platform_oauth_client`**（协议级）：

  | 列 | 说明 |
  |---|---|
  | `client_id` | 平台生成，全局唯一（不含租户 → 跨租户 client 是**平台级**资产） |
  | `client_secret_hash` | SHA-256 base64 摘要（**可为空** —— public client） |
  | `client_type` | `confidential` / `public` |
  | `name` | 展示名（同意页用） |
  | `redirect_uris` | `text[]`，回调白名单 |
  | `allowed_grant_types` | `text[]` |
  | `allowed_scopes` | `text[]` |
  | `backchannel_logout_uri` | 可空 |
  | `frontchannel_logout_uri` | 可空 |
  | `status` | `active` / `disabled` |
  | `expires_at` | 可空（null = 不过期） |
  | `created_at` / `created_by` | |

- **REQ-016 的「应用注册表」**是**业务级**对象（类型 / 图标 / 描述 / 分类 / 工作台入口）→
  **引用**本表并扩展（`app_id` → `client_id` 外键），**不再自带一份** secret / 回调 / 有效期。

- ⚠️ **REQ-016 的 demand.md 必须按 D109 改写**（草稿里写的「app_id / secret、回调地址、
  令牌有效期」与本表直接重叠）。

- **依赖方向**：REQ-016 → REQ-022（单向，符合 ADR-0001）。

---

## D200: `client_secret` 由平台生成、**只存 SHA-256 摘要**、**只显示一次**

- **决定**：
  1. secret **不允许用户自选** —— 自选 = 低熵 → 需要 Argon2id（慢哈希），
     而 client 认证是高频路径 → 慢哈希不可接受。
  2. 平台生成：`random_base64url(32)`（`crates/platform-iam/src/crypto.rs:39`），
     **32 字节 = 256 位熵** → 与 refresh token 同档（`crypto.rs:26` `REFRESH_TOKEN_BYTES`）→
     **快哈希够用** → 摘要用 **SHA-256**（`crypto.rs:56` `digest_base64url`）。
  3. **只在创建响应里显示一次**，之后任何读接口**不回显**（包括管理列表）。
  4. `public` client（纯 PKCE / device code）**没有** secret，`client_secret_hash` 为空。

- **判定**：AC-S-136 断言库里存的是摘要（等于 `digest_base64url(明文)`）、响应里不出现明文。

---

## D201: `scope` **就是权限点码本身**；授予 = 三重交集

- **决定**：
  1. `scope` 参数的每个 token = **一个权限点码**（形如 `rbac:role:write`），
     空格分隔。**零映射表**。
  2. 实际授予的 scope = **请求 scope ∩ `client.allowed_scopes` ∩ 用户自身的有效权限点**。
     - 交集为空 → `invalid_scope`（400）。
     - **授予的 scope 永远不得超过用户本人**（否则应用拿到比用户更大的权）。
  3. **通配不得隐式生效**：`rbac:role:*` 只有在它**本身是注册过的权限点**时才有效
     （对照契约 C4「允许通配 `order:*`（**角色层面**）」—— 角色层面可以，
     **令牌 scope 层面不行**）。否则 400 `invalid_scope`。
  4. 合法 `scope-token` 字符集按 RFC 6749 §3.3：`%x21 / %x23-5B / %x5D-7E`
     → **`:`（0x3A）允许** → `module:resource:action` 合规。

- **为什么零映射**：授权语义与底座 RBAC **完全同构** → 同意页展示的就是实际权限，
  用户看得懂；映射表是「两套真相」，必然漂移。

- **判定**：AC-S-140 断言三重交集 + 通配 400 + 越权 400。

---

## D202: 同意记录表 **`platform_oauth_grant`**（一表三用）

- **决定**：一张表同时承担：① 同意记录 ② 撤销依据 ③ 登出定位（哪些 client 拿过该用户的令牌）。

  | 列 | 说明 |
  |---|---|
  | `tenant_id` | 租户 |
  | `client_id` | |
  | `user_id` | |
  | `scopes` | `text[]`，**实际授予**的 scope |
  | `status` | `active` / `revoked` |
  | `granted_at` / `revoked_at` | |
  | `consent_at` | 用户实际点击「允许」的时间 |

- **UNIQUE (tenant_id, client_id, user_id)** → 再次授权 = **更新** scope（幂等）。
- 「可审计」由 REQ-014 的审计 sink 满足（本需求埋点，见 D215）→ **不另建同意流水表**。

---

## D203: 授权端点 = **前端路由 + 后端 preview/confirm/deny**

- **背景**：平台的登录态是 **Bearer token 存在 `sessionStorage`**
  （`apps/shell/src/lib/session.ts`，键 `base.shell.session.v1`）→ **没有 Cookie**。
  而 OAuth 授权端点按 RFC 6749 §3.1 是**浏览器顶层导航**（GET）→ **导航不会带 Authorization 头**
  → 后端的 authorize 端点拿不到登录态。

- **决定**：**授权请求由前端路由承接，后端提供三个 POST 端点**：

  | 端点 | 作用 |
  |---|---|
  | `POST /api/oauth/authorize/preview` | 校验全部参数 → 返回 `{ticket, application, scopes, needsConsent}`；`needsConsent=false` 时直接返回 `{redirectTo}`（免登） |
  | `POST /api/oauth/authorize/confirm` | 用户点「允许」→ 验签 ticket → 签发 code → `{redirectTo}` |
  | `POST /api/oauth/authorize/deny` | 用户点「拒绝」→ 验签 ticket → `{redirectTo: redirect_uri?error=access_denied&state=…}` |

  前端路由 **`/oauth/authorize`**（`apps/shell/src/lib/routes.ts` 登记）负责：
  未登录 → 引导登录（带 `return_to`）；已登录 → 调 preview → 渲染同意页 → confirm/deny → 跳转。

- **为什么不在后端做 GET authorize**：那需要给平台**新增 Cookie 会话**（跨需求大改），
  或把 access token 放进 URL（进浏览器历史 / Referer / 访问日志 —— 正是 REQ-013
  `crates/platform-iam/src/sso/ticket.rs:1-22` 记的历史教训）。

- **与 REQ-013 的同构性**：REQ-013 是「后端回调 → 302 前端交接页 → 前端 POST 换 token」。
  本需求是「前端承接 → 后端出票据 → 前端 POST 换 code」。**同一个坑不踩第二次**。

---

## D204: 同意票据（consent ticket）用 **ES256 签名**，零新表

- **决定**：
  1. `preview` 通过校验后，签一个 **JWT 票据**（复用平台的 ES256 密钥集）：
     `typ = "oauth_consent_ticket"`、`client_id` / `redirect_uri` / `scope` /
     `code_challenge` / `code_challenge_method` / `state` / `sub` / `tid` /
     `exp`（**5 分钟**）/ `jti`。
  2. `confirm` / `deny` **只信 ticket**（不重新接受原始参数）→ 用户看到的 scope
     与授予的 scope **必然一致**。
  3. 票据**不落库**（无状态）→ **零新表**；一次性由 `jti` + 授权码的一次性语义保证
     （同一请求 confirm 两次 → 第二次授权码已被消费 → 失败）。

- **为什么不用「落库 + 状态机」**：落库要多一张表 + 清理作业；
  而票据的**唯一风险**是「用户看到的 ≠ 授予的」→ 签名票据从根上消除该风险。

- **为什么不用「重新接受原始参数 + 取交集」**：交集只会更小，但**可能与用户所见不一致**
  （用户看到 `{read}`，被改成 `{read,write}` → 交集 = `{read,write}` → **知情同意被破坏**）。

- **判定**：AC-S-146 断言「篡改 ticket 里的 scope → 400」。

---

## D205: 授权码只存 **SHA-256 摘要**、**交换后即删**、TTL 60 秒

- **决定**：
  1. 授权码 = `random_base64url(32)`（高熵 256 位）→ 库里只存 **SHA-256 摘要**。
  2. **TTL 60 秒**（RFC 6749 §4.1.2 建议「尽可能短，建议 10 分钟以内」→ 取 60 秒）。
  3. **交换后即删**（删除即消费 → 天然一次性，且不留残余）。
  4. 授权码**绑定** `client_id` + `redirect_uri` + `code_challenge` + `user_id` + `tenant_id`。

---

## D206: **强制 PKCE S256**（`plain` 不支持）

- **决定**：
  1. `code_challenge` **必填**，`code_challenge_method` 只接受 **`S256`**。
  2. `plain` **不支持**（RFC 7636 §4.2 允许，但 `plain` 与不校验等价 → 降级保护）。
  3. 校验：`base64url(SHA256(code_verifier)) == code_challenge`
     —— **复用既有实现** `crates/platform-iam/src/sso/oidc.rs:404` `code_challenge()`。
  4. verifier 不匹配 → `invalid_grant`（**不是** `invalid_request`）。

- **为什么强制**：底座的 client 可能是 public client（无 secret）→ PKCE 是唯一的
  绑定手段；不强制 = 授权码可被截获后盗用。

---

## D207: client 认证方式 = **`client_secret_basic`**；public client 免密

- **决定**：
  1. 令牌 / 内省 / 撤销端点用 **`client_secret_basic`**
     （`Authorization: Basic base64(urlencode(client_id) + ":" + urlencode(secret))`）。
  2. `public` client 只带 `client_id`（body 里），**不带** secret。
  3. 元数据里声明 `token_endpoint_auth_methods_supported = ["client_secret_basic", "none"]`、
     `introspection_endpoint_auth_methods_supported = ["client_secret_basic"]`、
     `revocation_endpoint_auth_methods_supported = ["client_secret_basic"]`。
  4. **认证失败统一返回 `invalid_client` + 401**（不区分「client 不存在」与「secret 错」）。

- **为什么 `basic` 而不是 `post`**：RFC 6749 §2.3.1 两者都允许；
  `basic` 让 secret 不进请求体 → 不进应用层日志 → 更安全。

---

## D208: 内省与撤销的**授权边界**：只有 confidential client 能调，且**只能操作自己的令牌**

- **决定**：
  1. `/api/oauth/introspect` 与 `/api/oauth/revoke` **必须** client 认证
     （**public client 无权调用**）。
  2. 内省：只返回**本 client 自己**的令牌状态；请求里带的令牌若不属于本 client
     → 返回 **`{"active": false}`**（**不报错**、**不泄露**存在性）。
  3. 撤销：只能撤销**本 client 自己**的令牌；对别人的令牌 → **200**（RFC 7009 §2.2：
     「无效令牌的撤销请求也返回 200」）→ **不泄露**。
  4. 内省响应的 `active=false` 时，**其它字段一律不出现**（RFC 7662 §2.2）。

---

## D209: 底座业务端点**默认拒绝** OAuth 令牌（本轮不做 opt-in 机制）

- **背景**：草稿 §4.3 初裁「OAuth 令牌只能访问显式声明接受 OAuth 令牌的端点」。
  本轮细化时发现：**要支持「业务端点接受 OAuth 令牌」，必须解决数据范围继承** ——
  OAuth 令牌代表「用户 X 授权给 client 的权限子集」，
  而 RBAC 的数据范围（`ScopeFilter`）是**按用户**算的
  （`crates/platform-core/src/extension/permission.rs:178` `effective_scope`）。
  若 OAuth 令牌的 scope checker 返回 `ScopeFilter::All` →
  **持令牌者可读全租户数据** → 越权。

- **决定**：
  1. **本轮底座的业务端点一律不接受第三方 OAuth 令牌** → 拿 OAuth 令牌打业务端点 = **401**。
     （默认拒绝，**零额外机制**，天然 fail closed。）
  2. 第三方应用验证令牌有**两条路**：
     - **(a) 验签 JWKS**：离线、快，但**撤销最长延迟一个 access token TTL**；
     - **(b) 调内省端点**：实时，但多一次网络往返。
  3. 若将来要「OAuth 令牌直连底座业务 API」，必须**另起需求**，同时解决：
     scope checker、**数据范围按授权用户继承**、显式 opt-in 登记机制。
     → 记 **技术债 TD-041**（不在本轮）。

- **为什么不违反用户裁定**：用户裁定的「全做」指草稿的 **A/B/C/D 四组功能**
  （4 种 grant + introspect + 免登 + 单点登出 + client 模型）。
  「底座业务端点接受 OAuth 令牌」**不在草稿里**，是 BA 在细化时自己引入的方向 →
  本轮**不做**不是缩小用户裁定的范围。

- **判定**：AC-S-151 断言 OAuth 令牌打业务端点 = 401。

---

## D210: 撤销语义 = **grant 级**；底座侧立即生效，只验签的第三方最长延迟一个 TTL

- **决定**：
  1. 撤销粒度 = **grant 级**（撤销某 client 对某 user 的全部授权）。
  2. **底座侧立即生效**：
     - 撤销 grant → 该 grant 下**所有**未过期令牌立即 `active=false`（内省查 grant 状态）；
     - 撤销单个令牌 → 令牌状态置 `revoked`。
  3. **只验签 JWKS 的第三方**：**最长延迟一个 access token TTL**（撤销信息不在 JWT 里）。
  4. `access_token` TTL **默认 3600 秒**（1 小时）—— 这个值就是「撤销延迟上界」，
     写进元数据 `access_token_expires_in`，**第三方据此知道自己的撤销延迟窗口**。

- **必须告知 REQ-016**：REQ-016 的 **AC-S-024「用户撤销后该应用的令牌立即失效」**
  在「只验签」场景下**做不到** → 按 D109 改写为
  「撤销后，经**内省**校验的令牌**立即**失效；经**验签**校验的令牌最长延迟
  `access_token_expires_in` 秒失效」。

---

## D211: 单点登出的两种形态 + `logout_token` 的 claims + **失败不影响主流程**

- **决定**：
  1. **`logout_token`** 是平台用 ES256 签的 JWT：
     `iss` / `aud` = client_id / `iat` / `jti` / `sid`（会话 ID）/
     `events` = `{"http://schemas.openid.net/event/backchannel-logout": {}}` /
     `sub`（用户 ID）/ `tid`。
     —— **不含** `nonce`（RFC 避免重放攻击的字段在 back-channel logout 里不需要）。
  2. **Back-Channel**：向 client 的 `backchannel_logout_uri` **POST** `form_encoded`
     （`logout_token=…`），**超时 5 秒**，**失败只记日志、不影响登出主流程**（登出必须成功）。
  3. **Front-Channel**：返回 **HTML**，内含每个 client 一个
     `<iframe src="{frontchannel_logout_uri}?logout_token=…">`。
     - 必须加 `sandbox` 属性与 `X-Frame-Options` 处理（防点击劫持）。
  4. **按应用登出** = 撤销该 client 的 grant + 作废该 client 拿到的令牌 + 通知该 client。
  5. **全局登出** = 撤销该用户**全部** grant + 作废**全部会话**
     （复用 `crates/platform-iam/src/store.rs:754` `revoke_family` /
     `:765` `revoke_all_refresh_tokens`）+ 通知所有已授权 client。

- **失败语义**：通知失败**绝不能**让登出失败（否则一个挂掉的第三方应用能让用户登不出去）。

---

## D212: 免登 = `prompt=none` + **已有 active grant** + scope 满足 → 静默签发

- **决定**：
  1. 前端在拼授权请求时带 `prompt=none`。
  2. 后端 `preview`：已有 **active** grant 且 `grant.scopes ⊇ 请求 scope`
     → `needsConsent=false`，**直接签发 code** → `{redirectTo}`。
  3. 否则按 OIDC 语义返回：
     - 没登录 → `login_required`
     - 登录了但没授权 → `consent_required`
     （**两者都不跳登录页 / 同意页** —— `prompt=none` 的含义就是「别问我」。）

---

## D213: 错误码归 `platform-oauth/src/error.rs`，按 **8 处**编辑点登记

- **决定**：
  1. 新错误码放在 `crates/platform-oauth/src/error.rs`（**业务 crate 风格**），
     按 REQ-013 实测的 **8 处**编辑点（`crates/platform-iam/src/error.rs` 的形状）：
     常量 / enum 变体 / `code()` / `status()` / `detail()` / `params()` /
     `ALL_CODES` / 测试的两个数组。
  2. **额外**登记到：
     - `crates/platform-core/src/titles.rs:45` `TITLES`（ASCII 标题，D113）
     - `packages/i18n/src/locales/zh-CN.json` 与 `en-US.json` 的 `error.<CODE>`
       （**逐 locale**）
     - `crates/platform-server/tests/i18n_titles.rs` 的 `all_codes()`
       —— 加一行 `for code in platform_oauth::error::ALL_CODES`
     - 该文件里的**行数断言**（`:156-165`）同步加常量。

- **⚠️ 判定写法**：跨栈断言写「**包含**」不写「**相等**」（D194 / dev-guide §9.13）
  → AC-S-152 断言 `TITLES ⊇ oauth 码集合` 且 `每个 locale ⊇ error.<CODE>`，
  **不**断言两侧条数相等。

---

## D214: 权限点登记 = **manifest + 种子迁移 + 启动对账 fail-fast**

- **决定**：新增 `crates/platform-oauth/manifest.json`，登记：

  | 权限点 | 用途 |
  |---|---|
  | `oauth:client:read` | 查看 client |
  | `oauth:client:write` | 注册 / 修改 client |
  | `oauth:client:delete` | 删除 client |
  | `oauth:grant:read` | 查看授权记录 |
  | `oauth:grant:revoke` | 撤销授权 |
  | `oauth:token:introspect` | 令牌内省（**管理视角**；客户端自助内省走 client 认证，不走权限点） |

- **三处齐**（D27）：
  1. `crates/platform-oauth/manifest.json` 的 `permissions[]`
  2. 种子迁移 `migrations/20261120000500_oauth_permission_seed.up.sql`
     —— `source` 必须逐字等于 `'crates/platform-oauth/manifest.json'`
     （`crates/platform-rbac/src/registry.rs` 的 `collect_manifest_permissions`
     用 `format!("{dir_name}/{name}/manifest.json")` 拼）
  3. 启动对账 `crates/platform-rbac/src/registry.rs:198` `require_consistent`
     → **不一致 = 进程起不来**（不是告警）。

- **C-5 合规**：每个登记的权限点都要有**字面量** `require_permission("oauth:…")` 调用点
  （`crates/platform-core/src/extension/permission.rs:273`）。

---

## D215: 审计埋点事件码清单（8 个）

- **决定**：本需求向 REQ-014 的审计 sink 埋 **8 个事件码**：

  | 事件码 | 触发点 |
  |---|---|
  | `oauth.client.registered` | 注册 client |
  | `oauth.client.updated` | 修改 client |
  | `oauth.client.deleted` | 删除 client |
  | `oauth.authorization.approved` | 用户同意 |
  | `oauth.authorization.denied` | 用户拒绝 |
  | `oauth.token.issued` | 签发令牌 |
  | `oauth.token.revoked` | 撤销令牌 / grant |
  | `oauth.logout.executed` | 单点登出 |

- **登记**：事件码是 `org.*` 同款（**不是 HTTP 错误码**）→ 走 `crates/platform-core/src/titles.rs`
  的 `TITLES` 末尾那一段（`org.*` 在 `:164-191`）。
  i18n 侧**不进** `error.*`（事件码不进响应体）→ 与 `org.*` 同款处理。

---

## D216: ⚠️ **C-5 命名禁区**：`platform-oauth/src/` 里不得出现 8 个别名

- **背景**：契约 C-5 要求鉴权入口**唯一**。扫描器
  （`crates/platform-contracts/src/authz_scan.rs:37-63`）把
  `check_permission` / `check_perm` / `has_permission` / `require_perm` /
  `authorize` / `ensure_permission` / `verify_permission` / `permission_check`
  **8 个名字**定义为「即判违例」。

- **决定**：`crates/platform-oauth/src/` 里**不得**出现这 8 个标识符
  （含函数名、变量名、模块名）。
  → 本需求的令牌校验函数**必须**叫别的名字
  （如 `authenticate_bearer` / `authenticate_client` / `verify_client_secret`）。

- **为什么要点名**：OAuth 端点**不用** `require_permission`（它们用 client 认证 /
  用户会话，不是 RBAC 权限点）→ 实现者很容易顺手写个 `authorize_client()` →
  **直接判 C-5 失败**。

- **判定**：AC-S-153 断言 C-5 契约检查在 `platform-oauth` 上**通过**（含阳性对照）。

---

## D217: 本轮**不做 refresh token**

- **决定**：本轮不发 refresh token，元数据里**不声明** `refresh_token` grant type。

- **理由**：
  1. 引入它就要做 **rotation + reuse detection** 一整套（REQ-007 已有 `family_id` 机制，
     但 OAuth 侧要另起一套）；
  2. scope 收窄后重新授权的成本低（用户点一次「允许」）；
  3. 草稿 A 组未列。

- **代价**：第三方应用拿到的 access token 过期后必须重新走授权流程。

---

## D218: 前端交付边界 —— 同意页 / 设备码页 / 「我的授权」页归 **REQ-022**

- **决定**：

  | 页面 | 归属 | 路由 |
  |---|---|---|
  | 授权同意页 | **REQ-022** | `/oauth/authorize` |
  | 设备码授权页 | **REQ-022** | `/oauth/device` |
  | 「我的授权」页（用户视角：看自己的授权 + 撤销） | **REQ-022** | `/oauth/grants` |
  | 应用注册表（管理员视角）+ 工作台 / 应用市场 | **REQ-016** | — |

- **为什么「我的授权」归 REQ-022**：grant 表在 REQ-022，撤销的**主体动作**（授权）也在 REQ-022；
  没有这个页面，用户只能调 API 撤销 → AC-S-147 无法端到端验收。

- ⚠️ 这是对 REQ-016 范围的改动 → 按 D109 在 REQ-016 的 demand.md 里显式声明。

---

## D219: 租户隔离 —— OAuth 的 5 张表全部**启用 RLS**

- **决定**：
  1. `platform_oauth_client` **是平台级资产**（不含 `tenant_id`）→ **不启用 RLS**
     （理由同 `crates/platform-db/src/fx.rs` 的 `platform_fx_rate` 平台级参考数据）。
  2. 其余 4 张表（`grant` / `code` / `device_code` / `token`）**都有 `tenant_id`** → **启用 RLS**，
     policy 形状与 `migrations/20261118000200_tenant_isolation.up.sql:29-34` 同款：
     `USING (tenant_id = current_setting('app.tenant_id', true))` +
     `WITH CHECK (…同…)`。
  3. RLS 验证必须在**单个事务内** `set_config` → 查询（D148）。

- **判定**：AC-S-155 断言跨租户不可见（用 `base` 直连造第二个租户的数据 → 查不到）。

---

## D220: 出站 HTTP client 在 `platform-oauth` 内**独立实现**，不动 REQ-013 的 OIDC client

- **背景**：`crates/platform-iam/src/sso/oidc.rs:88` 的 `OidcHttpClient`
  带 **JWKS 缓存 + discovery**，是**入站**专用（拉 IdP 的 metadata / JWKS）；
  出站 logout 需要的是「POST 一个 form 到任意 URL」→ **语义不同**，复用反而别扭。

- **决定**：`platform-oauth` 内部实现一个**最小**出站 client
  （`hyper` + `hyper-util` + `hyper-rustls`，**全部已在 `Cargo.lock`**，零新增依赖），
  **不改** `platform-iam` 的 OIDC client（不碰已交付的 REQ-013 代码）。

- **代价**：出站 HTTP 逻辑有两份 → 记 **技术债 TD-042**（待 OIDC client 重构时合并）。

- **安全约束**：出站 URL **必须**取自 client 表里**已登记**的
  `backchannel_logout_uri` / `frontchannel_logout_uri` → **不存在 SSRF 入口**
  （客户端注册时校验 scheme 为 `https`，本地开发允许 `http`）。

---

## rev 2（2026-11-20 23:50）：QA 挑刺 39 条的**逐条裁决**

- **挑刺报告**: [../../feature-meta/qa-req022-spec-review.md](../../feature-meta/qa-req022-spec-review.md)
  （**39 条**：阻塞 10 / 高 19 / 中 10；22/22 AC 全部有问题）
- **用例集**: [test-cases/index.md](test-cases/index.md)（**131 条**，TC-022-001~131，编号连续）
- **裁决**: **39 条全部接受**（无一驳回）→ 产出 `acceptance.md` **rev 2** +
  本节 D221~D256（**36 条**新决策）

> ⚠️ **本轮的方法**：QA 的每条结论都带 `file:line` 或可复跑的实测输出，
> BA 逐条**打开源码复核**后接受（不是「照单全收」—— 见 §rev 2 裁决总览的「复核」列）。
> 其中 **S-02** 与 BA 在 QA 运行期间**独立**发现的问题（`status.md` §10.2 BA-01）
> **完全一致** → 两次独立取证同一结论，这条判据的置信度最高。

### rev 2 裁决总览

| 挑刺 | 档位 | 级别 | 裁决 | 落点 |
|---|---|---|---|---|
| S-01 | 物理不可达 | 阻塞 | ✅ 接受 | D235（AC-S-137 步骤 4 改用 `page.route` 拦截） |
| S-02 | 物理不可达 | 阻塞 | ✅ 接受 | **D221**（公开路由 + 登录页 `return_to`） |
| S-03 | 不可判定 | 阻塞 | ✅ 接受 | D223（`TITLES` 行数 = **93**，逐字写出） |
| S-04 | 口径不一致 | 阻塞 | ✅ 接受 | **D223**（事件码**不进** `TITLES`） |
| S-05 | 不可判定 | 阻塞 | ✅ 接受 | D224（钉死 `UNAUTHENTICATED`，删「二者都接受」） |
| S-06 | 物理不可达 | 阻塞 | ✅ 接受 | D236（夹具账号与 client 全部由 `seed-e2e.sh` 播种） |
| S-07 | 物理不可达 | 阻塞 | ✅ 接受 | D237（mock 接收端 = 独立脚本 + 落盘 + `MOCK_MODE`） |
| S-08 | 物理不可达 | 阻塞 | ✅ 接受 | D238（E2E 私钥落盘 `target/e2e-jwt-private.pem`） |
| S-09 | 不可判定 | 阻塞 | ✅ 接受 | D239（PKCE 失败点 = **preview**） |
| S-10 | 不可判定 | 阻塞 | ✅ 接受 | D224（步骤 3 改写 + 补 3.5） |
| S-11 | 口径不一致 | 高 | ✅ 接受 | **D222**（统一 `{授权页基址}` 符号） |
| S-12 | 口径不一致 | 高 | ✅ 接受 | D222 |
| S-13 | 口径不一致 | 高 | ✅ 接受 | D240（判据拆 1a/1b；先还 `catalog.rs` 的债） |
| S-14 | 口径不一致 | 高 | ✅ 接受 | D241（改成三条可判断言） |
| S-15 | 不可判定 | 高 | ✅ 接受 | D242（拆成路径 5） |
| S-16 | 口径不一致 | 高 | ✅ 接受 | **D228**（摘要统一 `digest_base64url`） |
| S-17 | 不可执行 | 高 | ✅ 接受 | D243（运行期真值断言 + 可调用的扫描函数） |
| S-18 | 不可判定 | 高 | ✅ 接受 | D244（401 + `{"error":"invalid_client"}`） |
| S-19 | 口径不一致 | 高 | ✅ 接受 | D245（前置限定 grant 类型 + 补变体） |
| S-20 | 口径不一致 | 高 | ✅ 接受 | **D225**（Front-Channel = **HTML + iframe**） |
| S-21 | 不可判定 | 高 | ✅ 接受 | D246（`verification_uri` / `user_code` / `slow_down` 全补） |
| S-22 | 口径不一致 | 高 | ✅ 接受 | **D227**（manifest 加 `tenantScoped` 段） |
| S-23 | 口径不一致 | 高 | ✅ 接受 | D247（补写侧 `WITH CHECK` 判据） |
| S-24 | 不可判定 | 高 | ✅ 接受 | D230（`sid` = 触发登出的会话 ID） |
| S-25 | 安全缺口 | 高 | ✅ 接受 | **D229**（显式写明现状 + **TD-043**） |
| S-26 | 口径不一致 | 中 | ✅ 接受 | D248（改成有区分力的版本） |
| S-27 | 口径不一致 | 高 | ✅ 接受 | D249（拆 preview / token 两条） |
| S-28 | 口径不一致 | 高 | ✅ 接受 | **D226**（删掉死码 → 平台码 **9** 个） |
| S-29 | 表述 | 中 | ✅ 接受 | D250（「恰好等于」+ 顺序钉死） |
| S-30 | 不可判定 | 中 | ✅ 接受 | D251（两条可判断言） |
| S-31 | 不可判定 | 中 | ✅ 接受 | D252（TTL 判据逐列写死） |
| S-32 | 表述 | 中 | ✅ 接受 | D253（按 `code_hash` 查 + `information_schema`） |
| S-33 | 口径不一致 | 中 | ✅ 接受 | D254（补「两枚令牌」判据） |
| S-34 | 物理不可达 | 中 | ✅ 接受 | D236（`seed-e2e.sh` 加 `tenant-b` 账号） |
| S-35 | 不可判定 | 中 | ✅ 接受 | D243 |
| S-36 | 口径不一致 | 中 | ✅ 接受 | D232（迁移文件名统一） |
| S-37 | 口径不一致 | 高 | ✅ 接受 | D231（`client_credentials` = 两重交集） |
| S-38 | 口径不一致 | 中 | ✅ 接受 | D234（**201**） |
| S-39 | 口径不一致 | 高 | ✅ 接受 | D233（`consent_required` 只在授权页 query） |

---

## D221: 授权页走**公开路由** + 登录页 `return_to`（**不动 `router.ts`**）

- **问题**（S-02 + BA-01，两处独立发现）：平台的守卫重定向**丢掉 query**：
  - `apps/shell/src/lib/router.ts:261` `const intendedPath = path;` —— 只有 pathname；
  - `apps/shell/src/lib/router.ts:232` `const keepsQuery = path === environment.location.pathname;`
    → 跳 `/login` 时 path 变了 → `suffix = ''` → **query 被抹掉**；
  - `apps/shell/src/App.svelte:122` 登录后跳回 `intendedPath` → **原 query 已丢**。
  - 全仓 `grep "return_to\|returnTo"` 在 `apps/shell` / `packages/auth` / `packages/sdk` 里**零命中**。
- **决定**（采纳 QA 的选项 A）：
  1. `/oauth/authorize` 登记为**公开路由**（**不设** `requiresAuth`），
     照 `apps/shell/src/lib/routes.ts:70-76` 的 `SSO_CALLBACK_ROUTE_PATH` 范式；
  2. 页面自己读 `window.location.search` 取全部授权参数；
  3. 未登录 → 渲染「登录并继续」→ 把**完整 URL**（`pathname + search`）编码进
     `?return_to=` → 跳 `/login?return_to=…`；
  4. **改 `apps/shell/src/pages/Login.svelte`**：登录成功后
     `location.replace(return_to)`；**没有** `return_to` 时保持现行为（跳首页）。
  5. ⚠️ `Login.svelte` 与 `App.svelte` **不在** design §13 的原文件清单里 → **补进去**（棒 2 的文件集）。
- **为什么不改 `router.ts`**：`resolve()` 用带 query 的字符串去 `match()` 会匹配失败
  （`router.ts:204-207`）→ 要改就得让 `resolve` 先剥 query →
  牵动 REQ-011 已交付的路由核心，**风险远大于收益**。
- **代价**：改 `Login.svelte` 会影响 REQ-011 的既有前端用例 → 棒 2 必须跑前端全量回归。
- **判定**：AC-S-137 / AC-S-148 必须覆盖「未登录 → 登录 → **参数还在**」这条路径。

## D222: 统一用 **`{授权页基址}`**；`oauth.frontendBaseUrl` 未配置时回退 `{issuer}`

- **问题**（S-11 / S-12）：AC 内部对「授权页基址」有**两种写法**
  （AC-S-134 写 `{issuer}`，AC-S-137/148 写 `{平台}`）；
  而 E2E 里 `BASE_JWT_ISSUER=https://e2e.base`（`scripts/e2e-server.sh:72`）
  与真实服务地址 `http://127.0.0.1:8080`（`playwright.config.ts:12`）**不等**。
- **决定**：
  1. 全仓统一符号 **`{授权页基址}`**，定义 = 「`oauth.frontendBaseUrl` 的值；
     **未配置（非空）时回退到 `{issuer}`**」；
  2. `oauth.frontendBaseUrl` 的**默认值 = `""`**，语义明确为「**空 = 用 issuer**」；
  3. 元数据里的 `authorization_endpoint` = **`{授权页基址}` + `/oauth/authorize`**；
     `verification_uri` = **`{授权页基址}` + `/oauth/device`**；
  4. AC-S-134 的断言改成「`authorization_endpoint` 的 **host+port** == `{授权页基址}` 的 host+port
     （或未配置时 == `BASE_JWT_ISSUER` 的 host+port）」，**不钉死** `{issuer}`。
- **为什么保留 `oauth.frontendBaseUrl` 这个配置**：私有化部署里前端与 issuer 常常不同主机。

## D223: 审计**事件码不进 `TITLES`**；平台错误码 **9** 个；`TITLES` 行数 = **93**

- **问题**（S-04 + S-03）：
  - D215 说 8 个 `oauth.*` 事件码进 `TITLES` → 但 `crates/platform-server/tests/i18n_titles.rs`
    的**三个**既有断言会**必红**：
    ① `:285-300` 命名空间断言（键必须 `UPPER_SNAKE` 或 `org.` 前缀）；
    ② `:276-282` + `:357-405` 错误码 ↔ locale（只排除来源标签 `platform-org::notify`）；
    ③ `:190-202` 孤儿标题（每个 `TITLES` 键都要被某条 `ALL_CODES` 认领）。
  - 且 AC-S-152 只写「本需求新增数」→ **不可判定**。
- **决定**（采纳 QA 的选项 A —— 改动最小）：
  1. **8 个 `oauth.*` 事件码不进 `TITLES`**，也**不进** i18n 的 `error.*`
     （事件码**不进响应体** → 没有「面向用户的标题」这回事）。
     → 删掉 D215 的「走 `TITLES` 末尾那一段」，改为
     「事件码是 `platform-oauth` **内部常量**（`crates/platform-oauth/src/events.rs`），
     由**单测**覆盖其取值与用法」。
  2. **平台错误码 = 9 个**（见 D226，删掉死码 `OAUTH_CLIENT_SECRET_INVALID`）。
  3. **`TITLES` 行数 = 84 + 9 = 93** ——
     `crates/platform-server/tests/i18n_titles.rs` 的断言改成
     `BASELINE_ROWS_BEFORE_REQ_013(58) + REQ_013_NEW_CODES(21) + REQ_014_NEW_CODES(5)
     + REQ_022_NEW_CODES(9)` = **93**，AC 里**逐字写出 93**。
- **为什么不选「事件码进 TITLES」**：那要改 REQ-021 已交付的**跨栈测试契约**
  （三处断言 + 一个新的来源标签白名单）→ 跨需求改动，且**换不来任何用户可见收益**
  （事件码不出现在任何响应体里）。
- **代价**：事件码没有跨栈的「登记完备性」检查 → 由 `platform-oauth` 自己的单测承担。

## D224: AC-S-151 钉死 **`UNAUTHENTICATED`**；步骤 3 改写 + 补 3.5

- **问题**（S-05 + S-10）：
  - 「`UNAUTHENTICATED` 或协议错误 `invalid_token`——**二者都接受**」→ **不可判定**；
  - 且 `invalid_token` **不在**契约 C14 §14.2 的协议码集合里（它来自 RFC 6750，本契约未登记）；
  - AC-S-151 步骤 3 调的是 `/api/oauth/introspect`（**协议端点**，走 **client 认证**）→
    那个 OAuth 令牌**从头到尾没被当成令牌解析** → **判据与标题对不上**。
- **决定**：
  1. AC-S-151 步骤 1、2、3 的期望统一为
     **401 + `code` 逐字等于 `UNAUTHENTICATED`** + `Content-Type: application/json`
     + 响应体**不含** `error` 键（不是协议错误体）；
  2. 步骤 3 改为「用该 OAuth 令牌调 **`GET /api/rbac/me/permissions`**（**同一个**端点，步骤 1 已在用）」；
  3. **新增步骤 3.5**：「用该 OAuth 令牌调 `POST /api/oauth/introspect`，
     但**带** client-A 的合法 `client_secret_basic` 认证、`token` 参数填该 OAuth 令牌
     → **200** + `active == true`」
     → 证明该令牌**只**在 OAuth 协议端点上有效，**业务端点拒它**。
- **依据**（QA 实测）：OAuth 令牌的 `aud` = `client_id` ≠ 平台 audience
  → `validate_aud` 必然失败（`crates/platform-iam/src/jwt.rs:154-159`）；
  **且** `AccessClaims.sid` 是必填字段而 OAuth 令牌没有 `sid` → 反序列化也失败
  → **双重拒绝**，不会误放行。

## D225: Front-Channel Logout = **HTML + iframe**（不是 JSON 数组）

- **问题**（S-20）：AC-S-150 说「响应体里 `frontChannel` **数组**」，
  design §4.5 说「返回 **HTML**，内含 iframe」→ **判据不可能同时成立**。
- **决定**（采纳 QA 的选项 A）：
  1. `POST /api/oauth/logout` 返回 **`Content-Type: text/html`** 的页面，
     内含**恰好 1 个** `<iframe>`（每个有 `frontchannel_logout_uri` 的 client 一个），
     其 `src` **逐字等于** `{client_uri}` + `?logout_token=<JWT>`；
  2. 每个 iframe **必须**带 `sandbox` 属性：**至少** `allow-scripts`，
     且**不含** `allow-same-origin`；
  3. 响应**必须**带 `X-Frame-Options: DENY`（或 CSP `frame-ancestors 'none'`）；
  4. **前端**另提供一个「登出完成」页来渲染这个 HTML？—— **不**：
     后端返回的 HTML 由**浏览器直接导航**到（`POST` 的响应体不能直接导航）→
     → **前端**改为 `GET /api/oauth/logout`？**不**。
     → **裁定**：登出由**前端页面**发起 `POST`，后端返回 JSON
     `{ loggedOut: {...}, frontChannel: [{ clientId, logoutUrl, logoutToken }] }`，
     **前端**负责渲染 iframe 页面（`apps/shell/src/pages/OAuthLogout.svelte`）。
  5. ⚠️ 修正 D225 的第 1 点：**最终形态 = 后端 JSON + 前端渲染 iframe**
     （因为浏览器导航无法发起 POST，而 OAuth 令牌需要 Bearer 头）。
     AC-S-150 的判据落在**前端渲染后的 DOM**（`apps/shell/test/` 或 E2E）：
     - iframe 数量**恰好 1**；
     - `src` **逐字等于** `client-A.frontchannel_logout_uri` + `?logout_token=…`；
     - `sandbox` 属性含 `allow-scripts` 且**不含** `allow-same-origin`；
     - `client-B` **不在**其中。
  6. **后端侧**补一条可判：登出响应体**不含** `client-B` 的 `logoutUrl`。
- **为什么不是纯 HTML**：前端 SPA 的会话是 Bearer → 登出必须带 `Authorization` 头 →
  只能由前端 `fetch` 发起 → 后端回 JSON → 前端渲染 iframe。**这是 D203 的直接推论。**

## D226: 删掉死码 `OAUTH_CLIENT_SECRET_INVALID` → 平台错误码 **9** 个

- **问题**（S-28）：该码在 22 条 AC 里**一次都没出现**，也**没有触发点**
  （D200 决定 secret 不允许自选、只显示一次，而端点清单里**没有**「轮换 secret」端点）
  → 一个**永远不可能出现**的码，却要在 i18n 词库占一条、在 `TITLES` 占一行。
- **决定**：**删掉**。平台错误码清单变成 **9 个**：
  `OAUTH_CLIENT_NOT_FOUND` / `OAUTH_CLIENT_DISABLED` / `OAUTH_CLIENT_EXPIRED` /
  `OAUTH_INVALID_REQUEST` / `OAUTH_INVALID_REDIRECT_URI` / `OAUTH_INVALID_SCOPE` /
  `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` / `OAUTH_TICKET_INVALID` /
  `OAUTH_GRANT_NOT_FOUND`
- **什么时候再加回来**：将来做「轮换 secret」端点时，按 D213 的 8 处登记流程补上。

## D227: manifest 加 **`tenantScoped`** 段 → RLS policy 由 `xtask tenant-policies` 生成

- **问题**（S-22）：平台有**两条**并存的做法，REQ-022 的 design **一条都没选**：
  - `crates/platform-db/src/scopes.rs:5` 明写「RLS 策略的**唯一来源**是 manifest 的 `tenantScoped` 段」
    （`:91-93` 没该段的 manifest **被跳过**）；
  - `cargo xtask tenant-policies --check`（`crates/xtask/src/cli.rs:114-133`）靠这份声明逐表校验；
  - REQ-022 的 manifest 模板**没有** `tenantScoped` → 4 张表**不在**平台自检范围内
    → **漏建 policy 不会被发现**。
- **决定**（采纳 QA 的推荐）：
  1. `crates/platform-oauth/manifest.json` 加：
     ```json
     "tenantScoped": {
       "tables": ["platform_oauth_grant", "platform_oauth_code",
                  "platform_oauth_device_code", "platform_oauth_token"],
       "column": "tenant_id"
     }
     ```
  2. policy 由 `cargo xtask tenant-policies` 生成，命名自动是 `<table>_tenant_isolation`
     （`crates/platform-db/src/rls.rs:36` `POLICY_SUFFIX`）→ 与仓里现有 **31 张**表完全一致（实测）。
  3. `platform_oauth_client` **不**列入（平台级表，无 `tenant_id`，不启用 RLS）。
  4. AC-S-155 补：「`cargo xtask tenant-policies --check` 退出码 **0**，
     且 `pg_policies` 里这 4 张表各有 **1** 条 `<table>_tenant_isolation` policy」。
- **对比先例**：`platform-audit` 走的是**手写**路径（manifest 无 `tenantScoped`，
  policy 手写在 `migrations/20261120000200_audit.up.sql:146-182`）→ 它的表**不在**平台自检里。
  → REQ-022 **不重复这个缺口**。

## D228: 摘要编码统一 **`digest_base64url`**（base64url 无填充）

- **问题**（S-16）：仓里现成**两个**摘要函数：`digest_base64url`（`crypto.rs:56`，
  base64url 无填充）与 `digest_hex`（`crypto.rs:63`，小写 hex）。
  AC-S-136 钉了 base64url，AC-S-139 / AC-S-142 只写「SHA-256 摘要」→ **口径不一致**。
- **决定**：本需求所有「只存摘要」的列一律用 **`digest_base64url`** 口径
  （SHA-256 原始 32 字节 → **base64url 无填充**，43 字符）。
  受影响列：`client_secret_hash` / `code_hash` / `device_code_hash` / `user_code_hash`。
  → 写进 acceptance.md 的 §全局约定，测试侧独立复算时按此口径。

## D229: 全局登出**不**作废已签发的登录 access token —— 显式写明 + **TD-043**

- **问题**（S-25，**安全缺口**）：D211 说全局登出要「作废**全部会话**」，
  但实现引用的是 `revoke_family` / `revoke_all_refresh_tokens`
  （`crates/platform-iam/src/store.rs:754` / `:765`）→ 两者**只** UPDATE
  `platform_iam_refresh_token`。而 access token 是**自包含 JWT**
  （`crates/platform-iam/src/jwt.rs:143-166` 验签**不查库**）
  → **登出后，旧的登录 access token 在 `exp` 之前仍然能调所有业务端点**。
- **决定**（采纳 QA 的选项 B —— **本轮不补实现，但必须明写**）：
  1. D211 的「作废全部会话」**精确化**为「**作废全部 refresh token**（使会话无法续期）」
     —— **不含**「已签发的 access token 立即失效」；
  2. AC-S-149 / AC-S-150 **显式写明**：登出后，已签发的登录 access token
     **最长延迟一个 `iam.accessTokenTtl` 秒**才失效；
  3. **OAuth 令牌**不受影响 —— 它由 `platform_oauth_token` 表的状态决定，
     **撤销 grant 后立即 `active=false`**（这是 OAuth 侧与登录侧的关键差别，
     必须在 AC 里说清楚，否则两边的「撤销」语义会被混为一谈）；
  4. 记 **技术债 TD-043**：「登出不杀已签发的 access token」。
- **为什么本轮不补实现**：让 access token 立即失效需要「验签后查会话状态」——
  给**所有**业务端点的每次请求加一次查库 → **性能与架构改动**，属于 REQ-007 的范畴，
  不是本需求能顺带塞进去的。**显式写明现状**比默认沉默更诚实。

## D230: `logout_token` 的 `sid` = **触发登出的这次请求的会话 ID**

- **问题**（S-24）：D211 写了 `sid`，但没说全局登出（作废全部会话）时 `sid` 取哪个。
  平台是**一表多行**的多会话模型（`platform_iam_session` 实测 **277 行**）。
- **决定**：
  1. `sid` = **触发登出的这次请求所使用的会话 ID**
     （即当前 access token 的 `sid`）—— **一个**登出请求 = **一个** `logout_token`；
  2. AC-S-149 写死：「`logout_token` 的 `sid` **逐字等于** 触发登出的登录令牌的 `sid`」；
  3. 「作废全部会话」是**平台侧**的动作（`revoke_all_refresh_tokens`），
     **不**意味着要发多个 `logout_token`。

## D231: `client_credentials` 的 scope = **两重**交集（无用户维）

- **问题**（S-37）：D201 的「三重交集」在 `client_credentials` grant 下
  「用户自身的有效权限点」这一维**不存在** → 公式**没有定义** → 实现者可任选。
- **决定**：
  - `client_credentials` grant **没有用户** → 第三维**不适用** →
    授予的 scope = **请求 scope ∩ `client.allowed_scopes`**（**两重**）；
  - 交集为空 → `invalid_scope`（400，协议错误）。
  - AC-S-141 补一条判据：请求 `scope=sandbox:demo:write`（**不在** `allowedScopes` 里）
    → **400 + `{"error":"invalid_scope"}`**。

## D232: 迁移文件名统一为 **schema 0500 / permission_seed 0600 / config_seed 0700**

- **问题**（S-36）：design §8 说权限点种子是 `…0500_oauth_permission_seed`，
  §13 说是 `…0600_oauth_permission_seed` 且 `…0500` 已经是 schema → 两处矛盾。
- **决定**（只保留 §13 的一套，三处一致）：
  - `migrations/20261120000500_oauth_schema.up.sql` / `.down.sql`
  - `migrations/20261120000600_oauth_permission_seed.up.sql` / `.down.sql`
  - `migrations/20261120000700_oauth_config_seed.up.sql` / `.down.sql`
- **依据**：`migrations/` 现有最大编号 = `20261120000400_audit_config_seed` → 新迁移从 `…0500` 起。

## D233: `consent_required` / `login_required` 只出现在**授权页的 query**；preview 返 200 + `needsConsent`

- **问题**（S-39）：「返回 `consent_required`」没说端点、没说形状。
  `preview` 按 C14 §14.4 是**平台自有端点** → 错误体应是 Problem Details；
  但 `consent_required` 是**协议码**（小写）→ 两边都有依据 → 契约没覆盖这个组合。
- **决定**：
  1. `preview` 在 `prompt=none` 且 grant 不覆盖 scope 时 → **200 + `needsConsent == true`**
     （**不是错误**）→ 前端渲染同意页；
  2. `login_required` / `consent_required` 两个**协议码**只出现在**授权页的 query**
     （前端把状态透出到 URL），**不是** preview 的错误体；
  3. preview 的**错误**（参数非法、client 不可用）才用 Problem Details + `OAUTH_*`；
  4. AC-S-148 步骤 4 的断言改成「页面显示同意页 + URL query 里 `error=consent_required`
     + `platform_oauth_code` 行数 **== 0**」。

## D234: `POST /api/oauth/clients` 返回 **201**（不是 200）

- **问题**（S-38）：「**201**（或 200）」→ 两个取值都算过 → 不可判定。
- **决定**：**201 CREATED**。依据：平台的创建类端点**一律 201**
  （`crates/platform-iam/src/http.rs:586`、`crates/platform-rbac/src/http.rs:590`、
  `crates/platform-org/src/http.rs:406` 与 `:624`）。

## D235: 第三方回调用 **`page.route` 拦截**后读 `page.url()` 的 query

- **问题**（S-01）：`redirect_uri` = `https://client.example/cb` —— **该主机不存在**。
  design §4.1 写明「`redirectTo` 由**前端**执行跳转（`location.href`）」→
  浏览器真的导航过去 → Playwright 里 DNS 失败会抛导航错误，**地址栏停留在跳转前**
  → 「读地址栏的 `code=`」**不可达**。
- **决定**：
  1. E2E 先 `page.route('https://client.example/**', route =>
  route.fulfill({ status: 200, contentType: 'text/html', body: '<html>ok</html>' }))`；
  2. 走完授权流程后读 `page.url()`；
  3. 断言 `new URL(page.url()).searchParams.get('code')` **非空**，
     `…get('state')` **逐字等于** `xyz`。
  4. AC 里**显式写明这一步**（否则 Dev 会写成 `page.goto('https://client.example/cb')` → 必红）。
- **先例**：`code/e2e/sso.spec.ts:43-50` 已有**跨源拦截范式**。

## D236: 夹具账号与 client **全部**由 `scripts/seed-e2e.sh` 播种

- **问题**（S-06 + S-34）：
  - `scripts/seed-e2e.sh` 只建 `tenant-a` 的 **4** 个账号，**没有任何 `oauth:*`** 权限点
    → AC-S-135/136 的前置「管理员账号持有 `oauth:client:write`」**不可达**；
  - `tenant-b` 的用户**根本不存在** → AC-S-147 步骤 5 与 AC-S-155 的前置**不可达**。
- **决定**：
  1. `scripts/seed-e2e.sh` 新增：
     - `tenant-a` 的 **OAuth 管理员账号** `u-perm-oauth@example.com`，
       持 `oauth:client:read` / `oauth:client:write` / `oauth:client:delete`
       / `oauth:grant:read` / `oauth:grant:revoke`；
     - `tenant-a` 的**普通用户** `u-perm-oauth@example.com`（持 `rbac:role:read`）；
     - **`tenant-b`** 的账号 `u-perm-other@example.com`（持 `rbac:role:read`）；
     - **三个夹具 client**：`client-A`（confidential，`allowedScopes=['rbac:role:read']`）、
       `client-B`（confidential）、`public-client`（public）；
  2. client 的 `client_secret_hash` 由脚本按 **`digest_base64url`** 口径写入，
     **明文 secret 由脚本打印到 stdout** → E2E 从 stdout / 夹具文件读；
  3. 脚本的「强断言」段必须**自证**这些账号与 client **存在且绑定正确**
     （与现有脚本「强断言 ②」的写法同款）；
  4. AC 的前置**一律写成「由 `scripts/seed-e2e.sh` 播种」**，不再写「环境里已有」。

## D237: back-channel mock 接收端 = **独立脚本 + 文件落盘 + `MOCK_MODE` 开关**

- **问题**（S-07）：「**进程内** mock 接收端」—— 平台的 E2E 服务是**独立进程**
  （`scripts/e2e-server.sh:88` `exec cargo run … base -- serve`）；
  「进程内」只能指「**测试进程内**起一个监听端口」→ **端口是动态的** →
  client 表里的 `backchannel_logout_uri` 必须等 mock 起来之后才知道 → **夹具顺序**未规定；
  且仓里**没有**任何 mock HTTP server 的现成范式。
- **决定**：
  1. 新增 `scripts/mock-logout-receiver.sh`：监听 `127.0.0.1:$MOCK_PORT`，
     把收到的 `logout_token` **写到文件** `target/mock-logout-received.jsonl`，
     行为由 `MOCK_MODE=ok|timeout|500` 环境变量控制（默认 `ok`）；
  2. `scripts/seed-e2e.sh` 读 `$MOCK_PORT`，把
     `backchannel_logout_uri = 'http://127.0.0.1:' || $MOCK_PORT || '/logout'`
     写进 client 行 → **夹具顺序钉死**：先起 mock → 再播种；
  3. AC 的步骤 2 改为「读 mock 落盘的文件（`target/mock-logout-received.jsonl` 的最后一行）」
     —— **不要**在 E2E 进程里 mock HTTP（那会把「出站真的发出去了」这件事 mock 掉）；
  4. AC 的步骤 4/5 改为「用 `MOCK_MODE=timeout` / `500` **重启** mock 后再触发登出」。
- **为什么必须独立进程**：出站 HTTP 的**真实性**是本条 AC 的全部价值；
  在 E2E 进程内 mock HTTP = 把被测逻辑本身 mock 掉（dev-guide §9.13）。

## D238: E2E 的 JWT 私钥**落盘**到 `target/e2e-jwt-private.pem`

- **问题**（S-08）：要证明 D204 的「用户所见 = 实际授予」，
  测试必须造出「**签名有效、scope 更大**」的票据 → 需要**测试侧自己持私钥**。
  而 `scripts/e2e-server.sh:65-73` 的私钥是现场生成 + `export`，**不落盘**，
  且 Playwright 是**另一个进程**（`playwright.config.ts:43-46`）→ E2E 读不到。
  → 只做 base64 重编码的话，签名必失 → `OAUTH_TICKET_INVALID` → **判据恒真**
  （只证明「验签在工作」，证明不了「scope 不能被放大」）。
- **决定**（采纳 QA 的选项 A）：
  1. `scripts/e2e-server.sh:65-70` 改成「生成后**写入** `target/e2e-jwt-private.pem`
     并 `export`」；
  2. 文件权限 **0600**、**不入 Git**（`target/` 已被 `.gitignore` 忽略）、**每次运行覆盖**；
  3. E2E 用它**自己签**一个 `typ="oauth_consent_ticket"`、`scope` **比 preview 返回的更大**的票据
     → 调 confirm → 必须 **400 + `OAUTH_TICKET_INVALID`**。
- **安全说明**：这是**测试环境的临时密钥**，只落在 `target/` 下；
  生产密钥仍**只从环境注入**（`crates/platform-iam/src/jwt.rs:4-7` 的约定不变）。

## D239: PKCE 的失败点在 **`preview`**（不是 confirm）

- **问题**（S-09）：按 D203，参数校验在 `preview`，code 在 `confirm` 才签
  → AC 的「**签发 code 这一步**就失败」在实现上**不成立**；
  Dev 若照字面在 confirm 里判 → 用户在同意页点「允许」才炸 → **把参数错误推迟到了交互之后**。
- **决定**：
  - 路径 1（缺 `code_challenge`）：调 `preview` → **400** + Problem Details，
    `code == "OAUTH_INVALID_REQUEST"`，`status == 400`，`Content-Type: application/json`，
    响应体**不含** `ticket` 键；→ **不得**调用 `confirm`、**不得**产生任何授权码行；
  - 路径 2（`plain`）：同 preview → **400** + `code == "OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD"`；
  - 路径 3（verifier 不匹配）：**先** preview 成功（拿 ticket）**再** confirm 时传错的
    `code_verifier` → **400** + `{"error":"invalid_grant"}`（**协议错误体**，
    因为 `/api/oauth/token` 是协议端点）。

## D240: 契约检查判据拆 **1a / 1b**；先还 `xtask catalog.rs` 的过期文案

- **问题**（S-13）：
  1. `cargo xtask capabilities --check` **在基线上就已经是红的**
     （实测退出码 1，`E_CAPABILITIES_OUTDATED`）——
     根因：`crates/xtask/src/catalog.rs:147-149` 仍写
     「`audit_sink`：审计只到事件声明面，落库与查询**待 REQ-014**」（REQ-014 已合并）；
  2. 它**不做 C-5**（`crates/xtask/src/capabilities.rs` 只比对索引产物与磁盘）；
  3. 「14 个文件」**不会变**（新增 crate **不新增** `capabilities/` 下的文件），
     变的是 `permissions.json` 的 `total`（26 → 32）。
- **决定**：
  1. AC-S-153 步骤 1 拆成两条：
     - **1a**：`cargo xtask capabilities` **重新生成** → 断言 `capabilities/` 下**恰好 14 个文件**
       （**不增不减**），`capabilities/permissions.json` 的 `total == 32`
       （26 + 6 个 `oauth:*`），且含 6 个 `oauth:*` 权限点**逐字**；
     - **1b**：**C-5 判据换成真命令** `cargo test -p platform-oauth --test contract_c5`
       （**该测试文件必须由本需求新增**，补进 design §13 的文件清单；
       范式见 `crates/platform-audit/tests/contract_c5.rs:24-64`）；
  2. 步骤 3/4 的「再跑 C-5」也**逐字写出这条命令**；
  3. **本需求必须先还 `catalog.rs` 的过期文案**（否则 1a 第一步就红）→
     记 **技术债 TD-044**（REQ-014 的遗留，本轮顺带还）。

## D241: 通配不可生效的判据改成**三条可判断言**

- **问题**（S-14）：AC-S-140 的「二者必须能区分开」**做不到** ——
  原因 A「不是注册过的权限点」与原因 B「通配被禁用」**同时成立**
  （`PermissionProvider::effective_permissions` 返回的是**已展开通配**的集合，
  `crates/platform-rbac/src/entity.rs:232` 的 `expand()` 返回注册表里的**具体码**
  → 用户的有效权限点集合里**永远**没有 `rbac:role:*`）
  → AC 给的区分方法在**两种实现下都通过** → **无区分力**。
- **决定**：删掉那句不可判的断言，改成三条**可判**的：
  1. 测试侧查表：`SELECT count(*) FROM platform_permission WHERE code = 'rbac:role:*'` **== 0**；
  2. 用户有效权限点集合里**不含**任何含 `*` 的码
     （`GET /api/rbac/me/permissions` 的返回集合 `NOT LIKE '%*%'`）；
  3. **真正的对照**：把 `rbac:role:*` **登记进 `platform_permission`**
     （管理员直连 INSERT，仅本用例夹具）→ 重跑路径 4 → **仍然** 400 `invalid_scope`
     → 这才证明「令牌 scope 层面通配**一律**不生效」，而**不是**因为「它没注册」。
  4. AC 里必须**显式写明**「测试要往 `platform_permission` 插一行」，
     否则会被判为越权造数。

## D242: 「不回溯扩大」拆成**路径 5**

- **问题**（S-15）：AC-S-140 的额外断言「路径 2 的令牌不含 `rbac:role:write`」
  —— 路径 2 的预期是 **400**，**根本没有签发任何令牌** → **断言没有对象** → 假绿。
- **决定**：拆成路径 5：
  1. 用户**只有** `rbac:role:read` → 请求 `scope=rbac:role:read` → 交换**成功** → 令牌 A
     （`scope == "rbac:role:read"`）；
  2. 管理员给该用户**追加** `rbac:role:write`
     （`platform_role_permission` + bump `platform_permission_epoch`）；
  3. **重放**令牌 A（**不**重新交换）→ 内省 → `scope` **仍然逐字等于** `rbac:role:read`，
     **不含** `rbac:role:write`；
  4. 另发一枚令牌 B（同一 scope 集 `rbac:role:read rbac:role:write`）
     → 令牌 B 的 scope **含** `write`
     → 证明「授予**新**令牌会拿到新权限，但**旧**令牌不被回溯扩大」。
- **关键**：第 3 步**不**能重新交换（重新交换会拿到新 scope → **测不到**「不回溯」）。

## D243: 敏感值扫描 = **Rust 侧可单独调用的 `pub fn`** + 运行期真值断言 + 阳性对照

- **问题**（S-17 + S-35）：「全仓扫描明文」**没有可实现的宿主与口径**
  （Rust 源码里根本不会出现运行时生成的明文 secret → 「扫源码」必然 0 命中
  —— 这正是 REQ-014 挑刺 S-08 踩过的坑）；
  「在临时副本里塞一个明文」也没定塞在哪、扫描器怎么实现。
- **决定**：
  1. **运行期断言（主判据）**：用真后端跑一遍
     `POST /api/oauth/clients` → 拿到明文 secret →
     `GET /api/oauth/clients`、`GET /api/oauth/clients/{id}`、
     `POST /api/oauth/token` 的**响应体**、`GET /api/audit/events` 的**全表文本**
     → **逐字**不含该明文（**用真值断言**，不是「没看到某个键」）；
  2. **静态扫描（辅判据，必须带阳性对照）**：
     - **宿主** = Rust 侧一个**可单独调用**的 `pub fn`
       （放 `crates/platform-oauth/src/`），输入 = 审计行的
       `action` / `object_type` / `object_id` / `before` / `after` 文本，
       输出 = 命中的敏感键列表；
     - **扫描面** = 审计表的**全部文本列**（不只 `before`/`after`）；
     - **阳性对照** = 管理员直连往审计表插一条**含假 secret** 的行
       （如 `before = '{"client_secret":"POSITIVE-CONTROL"}'`）→ 扫描器**必须命中**该行；
       删掉该行后 → 扫描器**必须不命中**；
     - **敏感键集合**列全（至少）：`client_secret` / `code` / `code_verifier` /
       `device_code` / `user_code` / `access_token` / `refresh_token` /
       `logout_token` / `authorization` / `password`。
- **为什么静态扫描只能是辅判据**：它证明「代码里没有把 secret 塞进响应的写法」，
  **证明不了**「运行时没泄漏」→ 主判据必须是**运行期的真值断言**。

## D244: 内省的认证失败 = **401 + `{"error":"invalid_client"}`**；令牌不属于本 client = **200 + `{"active":false}`**

- **问题**（S-18）：AC-S-143 步骤 5（public client）与步骤 6（无认证）
  在 D207 的口径下**都**是「client 认证失败」→ **都 401** → **无区分力**；
  且 AC 只写「401」，没写**错误体形状** —— `/introspect` 是 C14 的**协议端点**
  → 错误体应是 `{"error":"invalid_client"}`，**不是** Problem Details。
- **决定**：
  1. 步骤 5、6 的期望：**401** + 响应体**逐字**等于 `{"error":"invalid_client"}`
     （**不得**含 `type` / `title` / `status` / `instance` / `code` / `trace_id`）；
  2. **新增步骤 5.5**：「public client 用 `client_secret_basic` 认证（secret 为**空串**）
     → 同样 **401 + `invalid_client`**」→ 三条都是 401，
     但**必须都是 `invalid_client`**（不是 `OAUTH_*` 平台码）→ 这才是 D208 的可判形态；
  3. 内省的**认证失败**与**令牌不属于本 client**要分开：
     后者是 **200 + `{"active":false}`**（步骤 2），**不是** 401。

## D245: AC-S-145 前置限定 grant 类型 + 补 `client_credentials` 变体

- **问题**（S-19）：AC-S-145 要求 payload 的 `sub` **齐全**，
  AC-S-141 要求 `client_credentials` 令牌的 `sub` **键不存在**
  → AC-S-145 的前置没说是哪种 grant → 两条 AC 可能直接矛盾。
- **决定**：
  1. AC-S-145 前置改成：「`client-A` 持有一枚**由 `authorization_code` grant 签发**的有效令牌」；
  2. **新增**一条补充判据：把同一套断言对 **`client_credentials`** 令牌再跑一遍，
     期望 `sub` **键不存在**、其余 claims 齐全（与 AC-S-141 一致）。

## D246: 设备码补 **`verification_uri` / `user_code` 格式 / `slow_down` / 列覆盖**

- **问题**（S-21）：`verification_uri` **指向哪没定**；
  RFC 8628 §6.1 的 `user_code`（**8 个字符**、base20 字符集 `BCDFGHJKLMNPQRSTVWXZ`）
  与 §3.5 的 **`slow_down`**（轮询快于 `interval` → 返回 `slow_down` 并把间隔 **+5 秒**）
  **完全没覆盖**；design §2.4 的 `poll_interval_seconds` / `last_polled_at` 两列也**没有任何 AC 覆盖**。
- **决定**：
  1. `verification_uri` **逐字等于** `{授权页基址}/oauth/device`（与 D222 的符号统一）；
     `verification_uri_complete` **逐字等于**
     `{授权页基址}/oauth/device?user_code=<urlencoded user_code>`；
  2. `user_code` 长度 **== 8**，且**逐字符** ∈ `BCDFGHJKLMNPQRSTVWXZ`（base20）；
  3. **限流**：拿到令牌前**连续轮询 2 次**（间隔 < `interval`）→
     第二次返回 **400 + `{"error":"slow_down"}`**；
     且设备码行的 `poll_interval_seconds` **被更新为** `interval + 5`；
  4. **列覆盖**：`last_polled_at` 在轮询后**非空**且**递增**。

## D247: RLS 补**写侧 `WITH CHECK`** 判据

- **问题**（S-23）：AC-S-155 的 RLS 判据**只覆盖读侧** →
  **写侧漏写**（`WITH CHECK` 缺失 → 应用层能往表里写别的租户的行：
  写得进去、读不出来 → **静默的写侧泄漏**）**没有任何 AC 覆盖**。
- **决定**：AC-S-155 补一条：
  > 以 `base_app` 角色，在**单个事务内** `set_config('app.tenant_id','tenant-a')` 后
  > `INSERT` 一行 `tenant_id = 'tenant-b'` → **失败**（RLS 的 `WITH CHECK` 拒绝）；
  > 同一事务内 `INSERT` 一行 `tenant_id = 'tenant-a'` → **成功**。

- 并在 AC 里写明「`platform_oauth_client` **不参与**本条
  （平台级表，无 `tenant_id`、不启用 RLS）」。
- **依据**：`crates/platform-db/src/rls.rs:138-146`（既有 policy 全是
  `USING … WITH CHECK …` 成对）+ `migrations/20261118000200_tenant_isolation.up.sql:11-13`
  （「没有它，应用层可以往表里写入别的租户的行」）。

## D248: AC-S-150 步骤 2 改成**有区分力**的版本

- **问题**（S-26）：步骤 1 是**全局**登出 → 步骤 2 的「两个 client 的授权都被撤销」
  是步骤 1 的**定义**，**不是独立判据** → **没有区分力**（不可能不成立）。
- **决定**：改成：
  > 「步骤 2：**`client-B`（未配 front-channel URI）的 grant 也**被撤销
  > → 这证明 front-channel 配置**只影响通知方式**，**不影响撤销范围**；
  > 且 `client-B` **不在** `frontChannel` 数组里
  > → 证明收集逻辑按 `frontchannel_logout_uri` **非空**过滤。」

## D249: disabled client 的拒绝点**拆两条**（preview 403 平台码 / token 401 协议码）

- **问题**（S-27）：AC-S-135 步骤 5 说「用该 client 走一次授权码交换 → 401 + `invalid_client`」，
  但按 design `OAUTH_CLIENT_DISABLED` = **403**，
  且 disabled 的 client 在 **preview** 阶段就该被拒 → **根本走不到令牌交换**；
  两套错误标识在同一个端点族里混用。
- **决定**：步骤 5 拆成两条，各钉死端点与错误体：
  1. **preview 阶段**：`POST /api/oauth/authorize/preview`（用已 disabled 的 client）→
     **403** + Problem Details，`code == "OAUTH_CLIENT_DISABLED"`（**平台话**，preview 是平台端点）；
  2. **token 阶段**：`POST /api/oauth/token`（client 认证用 disabled 的 client）→
     **401** + `{"error":"invalid_client"}`（**协议话**，token 是协议端点）。
- **顺带钉死**：两套错误标识在同一端点族里的分工就此明确，与 C14 §14.2 一致。

## D250: 「恰好包含」→「恰好等于」+ **顺序钉死**

- **问题**（S-29）：AC-S-134 同一条里别的字段用「**恰好** `[...]`」，
  唯独 `grant_types_supported` 用「**恰好包含**」→ 自造词，语义歧义。
- **决定**：「`grant_types_supported` **恰好**等于
  `["authorization_code","client_credentials","urn:ietf:params:oauth:grant-type:device_code"]`
  （**逐元素比对，顺序也钉死**；**不含** `refresh_token`，D217）」。

## D251: `error_description` 判据改成**两条可判断言**

- **问题**（S-30）：「不含『哪一步错了』的细节以外的信息」→ 句义歧义
  且「哪一步错了」**没有定义** → 不可机械判定。
- **决定**：改成两条：
  1. `error_description` **不含** `code_verifier` / `code_challenge` / `client_secret` / `code`
     的**明文**（逐个子串断言 `NOT contains`）；
  2. `error_description` **不含任何 CJK 字符**（契约 C14 §14.2）。

## D252: TTL 补充判据**逐列写死**

- **问题**（S-31）：授权码行有 **9 个绑定列** → 直插一行时每个值都得对，
  否则交换会**先卡在绑定校验上**而不是卡在 TTL 上 → **判据失真**。
- **决定**：补充判据逐列写死：
  > 以 `base` 直连 `INSERT` 一行 `platform_oauth_code`：
  > `tenant_id='tenant-a'`、`client_id=<client-A>`、`user_id=<u>`、
  > `code_hash=<base64url(SHA256('ttl-expired-code'))>`、
  > `redirect_uri='https://client.example/cb'`、`scope='rbac:role:read'`、
  > `code_challenge=<S256(verifier)>`、`code_challenge_method='S256'`、
  > `expires_at = now() - interval '90 seconds'` →
  > 交换返回 **400 + `{"error":"invalid_grant"}`；
  > 同一批值、只把 `expires_at` 改成 `now() + interval '30 seconds'` → 交换**成功**（200）。
  > → 两条断言**只在 `expires_at` 上不同**，其它列逐字相同 → 才证明判的是 TTL。

## D253: 授权码行数判据按 **`code_hash`** 查 + `information_schema`

- **问题**（S-32）：「查授权码表的行数」是整表还是该 code？
  若是整表，其它用例 / 其它租户的行会污染判据（REQ-014 的「干净起点」教训）；
  且 design §2.3 的列清单里**没有** `consumed_at` → 「不是置一个 `consumed_at` 标记」
  这个否定断言**没有对象**。
- **决定**：
  - 步骤 4 改为「`SELECT count(*) FROM platform_oauth_code
    WHERE code_hash = <base64url(SHA256(code))>` → **恰好 0**」；
  - 补一条「该表的列集合里**不含** `consumed_at` / `used_at`」
    （查 `information_schema.columns`）。

## D254: grant 级撤销补「**两枚令牌**」判据

- **问题**（S-33）：D210 的核心语义是「**grant 级**撤销」，
  但 AC-S-144 只验**一枚**令牌、AC-S-147 也只验一枚
  → 实现成「只撤销那一枚令牌」也能过 → **grant 级语义没被验收**。
- **决定**：AC-S-147 补一条：
  > **先签发 2 枚**令牌（同一 grant）→ 撤销 grant → **两枚**都 `active == false`
  > （并断言 `platform_oauth_token` 里这两行状态都是 `revoked`）。

## D255: 修 `qa-recheck-req014.md:638` 引用了**不存在的** `AC-S-136`（笔误）

- **发现**：BA 自查（`status.md` §10.3）—— `BA/feature-meta/qa-recheck-req014.md:638`
  写着「`config.flag.write` 的事件码与 `before`/`after` 形状（**AC-S-136 表内一行**）」，
  但 REQ-014 的 AC 是 `AC-S-022` + `AC-S-122~133`，**没有 136**。
- **决定**：改为引用**真实存在**的 AC（该行对应的是 REQ-014 的**审计埋点**那一条），
  并在 REQ-022 的这次提交里一并修掉 —— **一处指向不存在 AC 的引用会误导后来人**。
  （本条是 BA 自查发现的，不在 QA 的 39 条里。）

## D256: 棒 1 ∥ 棒 2 的文件集划分（`packages/i18n/**` 归棒 2）

- **发现**：BA 自查（`status.md` §10.5）—— `packages/i18n/src/locales/*.json`
  **两个棒都要改**（棒 1 要加 9 个 `error.OAUTH_*`；棒 2 要加 `oauth.*` 的界面文案）
  → **同文件 = 并行冲突**。
- **决定**：
  1. **整个 `packages/i18n/**` 归棒 2**；
  2. 棒 1 把 **9 个错误码的精确清单**（码 + ASCII 标题 + 中文文案）写进**任务卡**，
     由棒 2 一并写进词库（**棒 2 必须在棒 1 的错误码清单定稿后写**，
     清单本身在任务卡里就是定稿的 → 无需等待）；
  3. `crates/platform-core/src/titles.rs` 与
     `crates/platform-server/tests/i18n_titles.rs` 仍归**棒 1**（后端侧）；
  4. 两侧靠**约定字符串**耦合，任务卡必须写死三处：
     ① 9 个错误码的精确清单；② 前端路由路径
     `/oauth/authorize` / `/oauth/device` / `/oauth/grants`；
     ③ `manifest.json` 的 `menus[].route` 与前端路由**逐字相同**。

---

## rev 3（2026-11-21）：**并行实现前**发现的耦合缺口补记（2 条）

> **为什么有 rev 3**：D221~D256 是 QA 挑刺后的裁决轮；本 rev 是 BA 在**写 Dev 任务卡**
> 时发现的**两个规格缺口** —— 它们都不是「挑刺能挑出来」的问题，而是
> **只有把规格落成「两个互不相交的文件集」时才会暴露**的缺口：
> - **D257**：设备码流程里「用户点允许」**没有对应的后端端点** →
>   AC-S-142 步骤 4 要求这个动作存在，但 design §3 的端点清单里**没有它** →
>   棒 1 与棒 2 并行时，**两边会各造各的**。
> - **D258**：平台端点的**请求 / 响应字段名**没钉死 → 棒 2 只能**猜** →
>   猜错就是终版 E2E 期的返工。
>
> ⚠️ **本 rev 不改任何一条 AC**（22 条 AC 与 131 条用例**不受影响**），
> 只补「让 AC-S-142 步骤 4 **可实现**」的端点与「让两棒**不猜**」的字段名。

## D257: 设备码的「用户批准」端点 —— **补规格缺口**

- **问题**：AC-S-142 步骤 4 要求「浏览器访问 `verification_uri`，输入 `user_code`，
  点「允许」」→ 这个「允许」必须**调一个后端端点**。
  但 design §3.1 / §3.2 的端点清单里**没有任何「批准设备码」的端点**
  （`/api/oauth/device_authorization` 只是**申请**设备码，
  `POST /api/oauth/token` 是**轮询**换令牌，两头都不是「批准」）。
  → 前端设备码页**没有可调的接口**；棒 2 只能自己编一个路径 → 棒 1 不会实现它。
- **决定**：补 **2 个**平台端点（说平台的话 → Problem Details + `OAUTH_*`）：

  | 方法 | 路径 | 认证 | 作用 |
  |---|---|---|---|
  | `GET` | `/api/oauth/device/verify?userCode=<code>` | 用户会话（Bearer） | **批准前**展示 client 名与请求的 scope |
  | `POST` | `/api/oauth/device/approve` | 用户会话（Bearer） | **批准 / 拒绝** |

  1. **`verify` 的响应体（200）**：
     `{ "clientId": "<id>", "clientName": "<名>", "scopes": ["<权限点码>", …] }`
  2. **`approve` 的请求体**：`{ "userCode": "<8 位 base20>", "action": "approve" | "deny" }`
  3. **`approve` 的响应体（200）**：`{ "status": "approved" | "denied" }`
  4. **查不到 / 已过期 / 已使用的 `user_code`** → 两个端点**都**返回
     **400 + Problem Details + `code` 逐字等于 `OAUTH_INVALID_REQUEST`**。
     → **不新增第 10 个错误码** → **`TITLES` 行数仍是 93**（D223 / AC-S-152 不动）。
  5. **批准**的动作：把设备码行 `status` 置 `approved`、写入 `user_id`、
     并**写 / 更新 `platform_oauth_grant`**（`tenant_id` / `client_id` / `user_id` /
     `scopes` / `status=active` / `consent_at`）→ 令牌才能被 grant 级撤销（D210 / D202）。
  6. **拒绝**的动作：设备码行 `status` 置 `denied` → 轮询 `POST /api/oauth/token`
     返回 **400 + `{"error":"access_denied"}`**（协议码，`access_denied` 在契约 C14 的集合内）。
  7. **查询一律按摘要**：`user_code_hash`（`digest_base64url`，D228）→ 库里不存明文。
  8. ⚠️ 这两个端点**不**在 design §3.2 的原表里 → **本条是新增**，
     已在 design-summary.md §3.2 同步补上。

## D258: 跨栈 JSON 契约钉死（**字段名 = camelCase**）

- **问题**：棒 2 的 4 个页面要调棒 1 的 8 个平台端点；
  但「请求体 / 响应体的字段名」在规格里**从没钉死**
  → 棒 2 只能猜 `client_id` 还是 `clientId` → 猜错 = 终版 E2E 全红 + 返工一轮。
- **依据**：契约 **C13**（D167）—— API 的 JSON 字段名一律 **camelCase**
  （既有事实：`crates/platform-audit` 的 Rust 侧是 `next_cursor` /
  `created_at`，接口给的是 `nextCursor` / `createdAt`；
  `apps/shell/src/lib/audit-api.ts` 的 `AUDIT_SORT_FIELDS` 也是 camelCase）。
  协议端点（`/token` / `/introspect` / `/revoke` / `/device_authorization`）
  的字段名**由 RFC 定死**（`access_token` / `token_type` / `expires_in` /
  `scope` / `device_code` / `user_code` / `verification_uri` /
  `verification_uri_complete` / `interval` / `active` / `client_id` …）
  → **协议端点用 RFC 的 snake_case，平台端点用 camelCase**，**两套不混**。
- **决定**：把两套字段名**逐字写死**（棒 1 实现、棒 2 调用，**照抄**）：

  ### 平台端点（camelCase）

  | 端点 | 请求体 | 响应体（200） |
  |---|---|---|
  | `POST /api/oauth/authorize/preview` | `{ "responseType": "code", "clientId": "…", "redirectUri": "…", "scope": "…", "state": "…", "codeChallenge": "…", "codeChallengeMethod": "S256", "prompt": "none" }`（`prompt` 可选；`state` 可选） | `{ "needsConsent": true, "ticket": "<JWT>", "application": { "clientId": "…", "name": "…" }, "scopes": ["…"], "redirectTo": "…" }`（`ticket` 仅 `needsConsent == true` 时有；`redirectTo` 仅 `needsConsent == false`（免登直发）时有） |
  | `POST /api/oauth/authorize/confirm` | `{ "ticket": "<JWT>" }` | `{ "redirectTo": "https://client.example/cb?code=…&state=…" }` |
  | `POST /api/oauth/authorize/deny` | `{ "ticket": "<JWT>" }` | `{ "redirectTo": "https://client.example/cb?error=access_denied&state=…" }` |
  | `GET /api/oauth/grants` | — | `[{ "clientId": "…", "clientName": "…", "scopes": ["…"], "grantedAt": "<RFC3339>", "status": "active", "revokedAt": null }]` |
  | `POST /api/oauth/grants/revoke` | `{ "clientId": "…" }` | `{ "status": "revoked" }` |
  | `POST /api/oauth/logout` | — | `{ "loggedOut": { "grants": 2, "sessions": 3 }, "frontChannel": [{ "clientId": "…", "logoutUrl": "…", "logoutToken": "<JWT>" }] }` |
  | `GET /api/oauth/device/verify` | 查询参数 `userCode` | `{ "clientId": "…", "clientName": "…", "scopes": ["…"] }`（D257） |
  | `POST /api/oauth/device/approve` | `{ "userCode": "…", "action": "approve" \| "deny" }` | `{ "status": "approved" \| "denied" }`（D257） |

  - `grants` 的**字段是「包含」关系**（AC-S-147 用「含」）→ 棒 1 **可以**多给字段，
    但上面列出的字段名与**类型**不得改。
  - `frontChannel` 数组**可能为空**（没有 client 配 front-channel URI 时）→ 空数组，不是 `null`。
  - `POST /api/oauth/authorize/preview` 的 **`scope` 是空格分隔的字符串**
    （RFC 的写法）；响应里的 `scopes` 是**数组**（前端好渲染）。
  - ⚠️ **`prompt` 的缺省值 = 允许交互**（AC-S-148 步骤 5）；
    `prompt=none` 时**不得**跳登录页（D233）。
  - ⚠️ 前端把 `login_required` / `consent_required` 透出到**授权页的 URL query**
    （D233）→ 棒 2 用 `history.replaceState`，**不重新导航**。

