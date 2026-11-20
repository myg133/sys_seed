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

## rev 2 待填

QA 挑刺后逐条裁决追加。**本节的每一条都必须写明：挑刺编号 → 裁决 → 改了哪个文件。**
