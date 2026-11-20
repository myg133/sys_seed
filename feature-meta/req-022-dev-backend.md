# REQ-022 Dev 任务卡 —— 棒 1（后端）

> **本卡是 BA 写给 Dev 子 Agent 的施工单**。你是**棒 1**，与**棒 2（前端）并行**干活。
> 本卡**自包含**：不需要看对话历史就能开工。

---

## 0. 抬头

| 项 | 值 |
|---|---|
| **需求** | **REQ-022 出站 IdP 与单点登出**（父需求 REQ-001） |
| **需求来源** | 父需求 REQ-001 派生，由**平台底座路线图**规划（`BA/plan/roadmap.md` Sprint 5）→ **不是**用户临时的个人需求 |
| **你的棒** | **棒 1 —— 后端**：新 crate `platform-oauth` + 装配 + 迁移 + 夹具 + E2E |
| **worktree** | `/home/myg133/Projects/system_seed/feature-REQ-022`（分支 `feature/REQ-022`，基线 `develop@310e4d3`） |
| **规格版本** | `BA/demands/REQ-022-outbound-idp/acceptance.md` **rev 2 + rev 3 补记**（**22 条 AC**：AC-S-134 ~ AC-S-155）<br>`decisions.md` **rev 2 + rev 3**（D196~D220 + D221~D256 + **D257 / D258**）<br>`design-summary.md` **rev 3** |
| **用例集** | `BA/demands/REQ-022-outbound-idp/test-cases/`（**131 条**，TC-022-001~131） |
| **契约** | `BA/contracts/api-conventions/v3.md`（**C1** 含 `/api/oauth`、**C1.1**、**C14** 协议端点错误体例外、**C13** 字段名 camelCase） |
| **QA 挑刺** | `BA/feature-meta/qa-req022-spec-review.md`（39 条，全部已裁决） |
| **git** | **不要提交**。改动留在工作区，由 **BA 统一提交**（既有约定） |
| **机器** | 低配（8 线程 / 7.7G）。`.cargo/config.toml` 已固定 `jobs = 3`。**禁止 `cargo install`** |

### 0.1 必读（**按顺序**，每份都要**打开源码核对**，不要只读结论）

1. `BA/demands/REQ-022-outbound-idp/acceptance.md` —— **22 条 AC，逐条读**
2. `BA/demands/REQ-022-outbound-idp/decisions.md` —— **D196~D258 全读**（61+2 条决策）
3. `BA/demands/REQ-022-outbound-idp/design-summary.md` —— 表结构 / 端点 / 时序
4. `BA/demands/REQ-022-outbound-idp/test-cases/` —— **131 条用例**（你的判据来源）
5. `BA/plan/dev-guide-low-spec.md` —— **§9.1 ~ §9.14 全部**（尤其 §9.2 / §9.4 / §9.8 / §9.9 / §9.10 / §9.11 / §9.13 / **§9.14 测试节奏**）
6. `BA/contracts/api-conventions/v3.md` —— 至少 C1 / C1.1 / C4 / C6 / C13 / **C14**

> ⚠️ **dev-guide §9.11**：规格里凡是引用既有 API / `file:line`，**都要打开源码确认**。
> 行号漂移**以符号名为准**，并在报告里注明。

---

## 1. 你的文件集（**严格** —— 与棒 2 不相交，D256）

### 1.1 你**拥有**（可以改）

```
Cargo.toml                                   ← workspace 成员 + [workspace.dependencies]
Cargo.lock                                   ← 必须入锁（REQ-021 的教训）
crates/platform-oauth/**                     ← 新 crate，全部
crates/platform-iam/src/jwt.rs               ← 加 sign_claims<T>（D109 #1）
crates/platform-iam/src/service.rs           ← 加 keys() getter（D109 #1）
crates/platform-server/src/main.rs           ← 装配（design §10）
crates/platform-server/tests/i18n_titles.rs  ← all_codes() 加一行 + 行数断言 84 → 93
crates/platform-core/src/titles.rs           ← 加 9 条 ASCII 标题（84 → 93）
crates/xtask/src/catalog.rs                  ← 修过期文案「待 REQ-014」（TD-044）
migrations/20261120000500_oauth_schema.up.sql
migrations/20261120000500_oauth_schema.down.sql
migrations/20261120000600_oauth_permission_seed.up.sql
migrations/20261120000600_oauth_permission_seed.down.sql
migrations/20261120000700_oauth_config_seed.up.sql
migrations/20261120000700_oauth_config_seed.down.sql
scripts/seed-e2e.sh                         ← 加 OAuth 夹具（D236）
scripts/mock-logout-receiver.sh             ← 新增（D237）
scripts/e2e-server.sh                       ← JWT 私钥落盘（D238）
e2e/oauth.spec.ts                            ← 新增
capabilities/**                              ← cargo xtask capabilities 重新生成
```

### 1.2 你**禁止**改（归棒 2 —— 碰了就是并行冲突）

```
apps/shell/**            ← 全部
packages/i18n/**         ← 全部（含 packages/i18n/src/locales/{zh-CN,en-US}.json）
```

> ⚠️ **`packages/i18n/**` 归棒 2**（D256）：9 个 `error.OAUTH_*` 的**词库登记**
> 由棒 2 做。**你只负责后端侧的 `TITLES`**（`crates/platform-core/src/titles.rs`）。
> 精确清单在 §3.1 —— **棒 2 的清单就是这份清单**，不需要等它。

### 1.3 同一 worktree 的**并行纪律**（两个棒共用 `feature-REQ-022`）

| 规矩 | 原因 |
|---|---|
| **禁止** `git commit` / `git checkout` / `git restore` / `git stash` / `git reset` | 会**清掉棒 2 的改动**。恢复只准用**备份副本 + `touch`** |
| **禁止** `git add -A` / `git add .` | 同上：会把棒 2 的文件也暂存起来 |
| 只改 §1.1 的文件 | D256 |
| `cargo test` 会与棒 2 抢 `CARGO_TARGET_DIR` 的锁 | 正常，cargo 自己排队；**不要**因此去删 `target/` |
| `scripts/seed-e2e.sh` / `scripts/e2e-server.sh` | 你独占。**棒 2 不碰数据库** |

---

## 2. 钉死的耦合契约（**照抄，别自创**）

### 2.1 平台错误码 —— **恰好 9 个**（D226）

> 写在 `crates/platform-oauth/src/error.rs`，按 **D213 的 8 处**登记。
> **第 8 处之外**还要：`crates/platform-core/src/titles.rs` 加 9 条 ASCII 标题、
> `crates/platform-server/tests/i18n_titles.rs` 的 `all_codes()` 加一行 + 行数断言改 **93**。

| # | 码 | HTTP | ASCII 标题（进 `TITLES`） | 中文文案（棒 2 写进 `zh-CN`） | 英文文案（棒 2 写进 `en-US`） |
|---|---|---|---|---|---|
| 1 | `OAUTH_CLIENT_NOT_FOUND` | 404 | `OAuth Client Not Found` | OAuth 客户端不存在 | OAuth client not found |
| 2 | `OAUTH_CLIENT_DISABLED` | 403 | `OAuth Client Disabled` | OAuth 客户端已停用 | OAuth client is disabled |
| 3 | `OAUTH_CLIENT_EXPIRED` | 403 | `OAuth Client Expired` | OAuth 客户端已过期 | OAuth client has expired |
| 4 | `OAUTH_INVALID_REQUEST` | 400 | `Invalid OAuth Request` | OAuth 请求参数非法 | Invalid OAuth request |
| 5 | `OAUTH_INVALID_REDIRECT_URI` | 400 | `Invalid Redirect URI` | 回调地址不在白名单内 | Redirect URI is not registered |
| 6 | `OAUTH_INVALID_SCOPE` | 400 | `Invalid OAuth Scope` | OAuth 权限范围非法 | Invalid OAuth scope |
| 7 | `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` | 400 | `Unsupported Code Challenge Method` | 不支持的 PKCE 挑战方法 | Unsupported code challenge method |
| 8 | `OAUTH_TICKET_INVALID` | 400 | `OAuth Consent Ticket Invalid` | OAuth 同意票据无效 | OAuth consent ticket is invalid |
| 9 | `OAUTH_GRANT_NOT_FOUND` | 404 | `OAuth Grant Not Found` | OAuth 授权记录不存在 | OAuth grant not found |

- `TITLES` 的行数：**84 → 93**（= 58 + 21 + 5 + **9**）。
  断言写成常量：`const REQ_022_NEW_CODES: usize = 9;`（AC-S-152 要求）。
- ⚠️ **8 个 `oauth.*` 审计事件码**（D215）**不进 `TITLES`**（D223）→ 行数**不是 102**。
- ⚠️ `TITLES` 的 `title` 必须是 **ASCII**、**不得等于** code、键唯一（既有测试会查）。
- ⚠️ **没有第 10 个码**。设备码查不到 / 过期 → `OAUTH_INVALID_REQUEST`（**D257**）。

### 2.2 协议错误码（**小写**，契约 C14 §14.2）

```
invalid_request / invalid_client / invalid_grant / unauthorized_client /
unsupported_grant_type / unsupported_response_type / invalid_scope /
access_denied / login_required / consent_required /
authorization_pending / slow_down / expired_token / server_error
```

- ⚠️ **集合里没有 `invalid_token`** → **不得使用**（D224）。
- 协议端点的错误体**逐字**是 `{"error": "<code>"}`（内省/撤销认证失败时）
  或 `{"error": "<code>", "error_description": "..."}`。
  **不得**含 `type` / `title` / `status` / `instance` / `code` / `trace_id`（D244）。
- `error_description` **不得**含任何 **CJK 字符**，也不得回显
  `code_verifier` / `code_challenge` / `client_secret` / `code` 的**明文**（D251）。

### 2.3 两套错误标识 —— **最容易搞混的一处**

| 类别 | 端点 | 错误体 |
|---|---|---|
| **协议错误** | `/api/oauth/token` / `introspect` / `revoke` / `device_authorization` | `{"error":"invalid_grant", …}` 小写 |
| **平台错误** | `/api/oauth/authorize/*` / `clients/*` / `grants/*` / `logout` / `device/*` | RFC 9457 Problem Details + `OAUTH_*` 大写 |

### 2.4 摘要口径（**不是 hex**）

`client_secret_hash` / `code_hash` / `device_code_hash` / `user_code_hash`
一律 **`digest_base64url`**：SHA-256 原始 32 字节 → base64url（**无填充**）→ **43 字符**。
**不得**用仓里另一个 `digest_hex`（D228）。

### 2.5 跨栈 JSON 字段名（**D258** —— 棒 2 照抄这份）

- **平台端点 = camelCase**（契约 C13）。**协议端点 = RFC 的 snake_case**。**两套不混**。
- 逐字的请求 / 响应字段见 `decisions.md` **D258** 的表格 —— **以那份为准**，
  本卡只强调最容易错的三条：
  - `preview` 的请求体是 **`clientId` / `redirectUri` / `codeChallenge` / `codeChallengeMethod` / `responseType`**
    （**不是** `client_id` / `redirect_uri` …）；
  - `preview` 响应体是 **`needsConsent` / `ticket` / `application` / `scopes` / `redirectTo`**；
  - `grants` 列表的字段是 **`clientId` / `clientName` / `scopes` / `grantedAt` / `status` / `revokedAt`**。
- `scope` 在**请求**里是**空格分隔的字符串**；在**响应**里是**数组**。
- `frontChannel` **可能为空数组**（不是 `null`）。

### 2.6 前端路由 + manifest（**D256 ③**）

- 前端路由（棒 2 建）：`/oauth/authorize` / `/oauth/device` / `/oauth/grants` / `/oauth/logout`
- `crates/platform-oauth/manifest.json` 的 `menus[].route` **逐字** = **`/oauth/grants`**（棒 2 的路由表里也必须是这个字符串）

### 2.7 ⚠️ C-5 命名禁区（D216）

`crates/platform-oauth/src/` 里**不得**出现这 8 个标识符：

```
check_permission   check_perm     has_permission   require_perm
authorize          ensure_permission  verify_permission  permission_check
```

- `authorize` 是 OAuth 语境下最自然的名字 → **最容易踩**。
- 令牌校验函数用：`authenticate_bearer` / `authenticate_client` / `verify_client_secret`。
- 真 C-5 命令：**`cargo test -p platform-oauth --test contract_c5`**（你要**新建**这个测试文件）。
  ⚠️ `cargo xtask capabilities --check` **不做** C-5，且**基线就是红的**（D240 / TD-044）。

### 2.8 其它钉死的数

| 项 | 值 | 依据 |
|---|---|---|
| 授权码 TTL | **60 秒** | D205 / `oauth.authorizationCodeTtlSeconds` |
| 设备码 TTL | **600 秒** | design §7 |
| 设备码轮询间隔 | **5 秒**（`slow_down` 时 **+5**） | RFC 8628 §3.5 / D246 |
| access token TTL | **3600 秒** | `oauth.accessTokenTtlSeconds` |
| consent ticket TTL | **+5 分钟** | design §5.2 |
| `user_code` | **8 字符**，字符集 **`BCDFGHJKLMNPQRSTVWXZ`**（base20，无易混字符） | RFC 8628 §6.1 / D246 |
| PKCE | **强制 S256**，`plain` **不支持** | D206 |
| `grant_types_supported` | **恰好** `["authorization_code","client_credentials","urn:ietf:params:oauth:grant-type:device_code"]`（**顺序钉死**，**不含** `refresh_token`） | D217 / D250 |
| `response_types_supported` | **恰好** `["code"]` | AC-S-134 |
| `code_challenge_methods_supported` | **恰好** `["S256"]` | D206 |
| `introspection_endpoint_auth_methods_supported` / `revocation_endpoint_auth_methods_supported` | **恰好** `["client_secret_basic"]` | D208 |
| `token_endpoint_auth_methods_supported` | **包含** `client_secret_basic` 与 `none` | AC-S-134 |
| 权限点 | **恰好 6 个**：`oauth:client:read` / `oauth:client:write` / `oauth:client:delete` / `oauth:grant:read` / `oauth:grant:revoke` / `oauth:token:introspect` | D214 |
| `capabilities/permissions.json` 的 `total` | **32**（26 + 6） | AC-S-153 |
| `capabilities/` 下文件数 | **恰好 14 个**（新 crate **不**新增文件） | AC-S-153 |

---

## 3. 交付清单（文件 → AC）

### 3.1 新 crate `crates/platform-oauth/`

建议模块切分（design §13 已给，可微调，但**文件清单要在报告里说清**）：

| 文件 | 内容 | 对应 AC |
|---|---|---|
| `Cargo.toml` | workspace 成员；依赖 **platform-core / platform-db / platform-iam / sha2 / base64 / serde_json / time / tracing / axum / hyper**（**全部已在树里**）。**零新增第三方依赖** | dev-guide §9.10 |
| `manifest.json` | 6 个权限点 + 1 条菜单（`route` = `/oauth/grants`）+ **`tenantScoped`** 段（4 张表）+ `config` 段（5 个键） | AC-S-153 / AC-S-155 / D227 |
| `src/lib.rs` | 模块挂载 + 再导出 + `VERSION` | — |
| `src/error.rs` | **9 个**平台错误码 + **D213 的 8 处**登记 | AC-S-152 / D213 / D226 |
| `src/events.rs` | **8 个** `oauth.*` 审计事件码常量 + 单测（**不进 `TITLES`**，D223） | AC-S-154 / D215 / D223 |
| `src/model.rs` | 5 张表的行类型 + 领域类型 | — |
| `src/store.rs` | 数据访问（**事务内 `set_config`** → 查询，D148） | AC-S-139 / 140 / 155 |
| `src/service.rs` | 业务编排（授权 / 换令牌 / 内省 / 撤销 / 登出） | AC-S-137~151 |
| `src/http.rs` | **平台端点**（Problem Details）+ **协议端点**（协议错误体）+ `router()` + `metadata_router()` | AC-S-134~150 |
| `src/client_auth.rs` | `client_secret_basic` 认证（**不区分**「client 不存在」与「secret 错」） | D207 / AC-S-135 |
| `src/pkce.rs` | S256 挑战校验 | AC-S-138 |
| `src/ticket.rs` | consent ticket 签发 / 验签（**必须校验 `typ`**，D204） | AC-S-146 |
| `src/logout.rs` | 登出编排 + back-channel 投递 + front-channel 收集 | AC-S-149 / 150 |
| `src/outbound.rs` | 出站 HTTP client（**独立实现**，超时 5 秒，**不动** REQ-013 的 OIDC client，D220） | AC-S-149 |
| `src/sensitive.rs` | **可单独调用的 `pub fn`** 敏感键扫描器（D243） | AC-S-136 / AC-S-154 |
| `tests/contract_c5.rs` | **真 C-5 命令**（D240） | AC-S-153 |
| `tests/*.rs`（其余） | 真库 + 真 HTTP（axum `oneshot`）集成用例 | 其余 AC |

### 3.2 改动既有文件

| 文件 | 改什么 |
|---|---|
| `crates/platform-iam/src/jwt.rs` | 加 `pub fn sign_claims<T: Serialize>(&self, claims: &T) -> Result<String, IamError>`；旧 `sign` **内部改调它，行为不变**（D109 #1 / design §5.1） |
| `crates/platform-iam/src/service.rs` | 加 `pub fn keys(&self) -> &JwtKeySet`（`keys` 字段现在是私有的，`service.rs:334`） |
| `crates/platform-server/src/main.rs` | 装配（design §10 的 5 处）；**IAM 没配 → 不提供 `/api/oauth/*`**（与 `/api/org/*` 同款口径） |
| `crates/platform-core/src/titles.rs` | + **9** 条 ASCII 标题（84 → 93） |
| `crates/platform-server/tests/i18n_titles.rs` | `all_codes()` 加 `for code in platform_oauth::error::ALL_CODES { … }` **一行**；行数断言加 `const REQ_022_NEW_CODES: usize = 9;`（→ 93） |
| `crates/xtask/src/catalog.rs` | 修 `:147-149` 的过期文案「待 REQ-014」（**TD-044**）—— 让 `cargo xtask capabilities --check` 能过 |

### 3.3 数据库（5 张表 + 2 个种子迁移）

- `20261120000500_oauth_schema.up.sql`：5 张表（design §2.1~2.5 的列**逐字**照抄）+ 索引。
  - ⚠️ **RLS policy 由 `cargo xtask tenant-policies` 生成**（manifest 的 `tenantScoped` 段是唯一来源，D227）
    → **迁移里不要手写 policy**。
  - `platform_oauth_client` 是**平台级**（**无 `tenant_id`、不启用 RLS**，D219）。
  - 4 张带 `tenant_id` 的表**启用 RLS**，policy 名 `<table>_tenant_isolation`（`crates/platform-db/src/rls.rs:36`）。
  - 写侧要 `WITH CHECK`（D247）。
- `20261120000600_oauth_permission_seed.up.sql`：6 条权限点 INSERT，
  `source` **逐字** = `'crates/platform-oauth/manifest.json'`（`crates/platform-rbac/src/registry.rs` 的拼法）。
- `20261120000700_oauth_config_seed.up.sql`：5 个配置键 INSERT（范式见
  `migrations/20261120000400_audit_config_seed.up.sql`）：
  `oauth.frontendBaseUrl`（`""`）/ `oauth.accessTokenTtlSeconds`（3600）/
  `oauth.authorizationCodeTtlSeconds`（60）/ `oauth.deviceCodeTtlSeconds`（600）/
  `oauth.deviceCodePollIntervalSeconds`（5）。
  ⚠️ 键名 **lowerCamelCase**（契约 C9；`crates/platform-core/src/config.rs:118-128` 会拒非 camelCase）。
- **每个 `.down.sql` 必须可逆**（先删函数 / 行，再删表）。

### 3.4 脚本与 E2E

| 文件 | 改什么 |
|---|---|
| `scripts/seed-e2e.sh` | 加 OAuth 夹具（D236）：`u-perm-oauth@example.com`(tenant-a, 持 `oauth:client:read/write/delete` + `oauth:grant:read/revoke`)、`e2e-user-oauth@example.com`(tenant-a, 持 `rbac:role:read`)、`u-perm-other@example.com`(**tenant-b**)、`client-A`(confidential, `allowedScopes=["rbac:role:read"]`, 配 back+front channel URI)、`client-B`(confidential, **不**配 front-channel URI)、`public-client`(public, 无 secret)。<br>client 的 `client_secret_hash` 按 **`digest_base64url`** 写；**明文 secret 打印到 stdout**。脚本的**强断言段**必须**自证**这些账号与 client 存在且绑定正确。<br>⚠️ **夹具顺序钉死**：**先起 mock → 再播种**（client 的 backchannel URI 含 `$MOCK_PORT`） |
| `scripts/mock-logout-receiver.sh` | **新增**（D237）：监听 `127.0.0.1:$MOCK_PORT`，把收到的 `logout_token` **写到文件** `target/mock-logout-received.jsonl`；行为由 `MOCK_MODE=ok\|timeout\|500` 控制（默认 `ok`）。<br>⚠️ **不要**在 E2E 进程里 mock HTTP —— 那会把「出站真的发出去了」mock 掉（dev-guide §9.13） |
| `scripts/e2e-server.sh` | `:65-70` 的 JWT 私钥生成**改成「生成后写入 `target/e2e-jwt-private.pem` 并 `export`」**；权限 **0600**、**不入 Git**、**每次运行覆盖**（D238） |
| `e2e/oauth.spec.ts` | **新增**。第三方回调用 **`page.route` 拦截**（D235）：`page.route('https://client.example/**', route => route.fulfill({ status: 200, contentType: 'text/html', body: '<html>ok</html>' }))`。读 `new URL(page.url()).searchParams.get('code')`（**不是**地址栏裸字符串）。<br>⚠️ **E2E 必须从干净库跑**；`scripts/e2e-server.sh` **不能从零建库**（TD-039）→ 清理后必须手工补 3 步：`deploy/seed/02-app-role.sql` + `modules/sandbox/migrations/` + `cargo xtask tenant-policies` |

---

## 4. 开工前必答项（**报告里逐条回答**，答不了就写「为什么答不了」）

1. **`platform-oauth` 怎么拿用户会话？** 平台会话是 **Bearer + sessionStorage**。
   `platform-oauth` **依赖** `platform-iam` → 能不能直接调 `IamService::verify_access_token`？
   如果不能，**装配层**（`platform-server`）解析后**注入**状态？
   （D203 的直接推论：浏览器顶层导航不会带 `Authorization` 头 → 授权端点只能是前端路由。）
2. **`PermissionProvider` 怎么注入？** `platform-oauth` **不依赖** `platform-rbac`
   （用 `platform-core` 的 `PermissionProvider` trait，`crates/platform-core/src/extension/permission.rs:239`）。
   6 个权限点各有 `require_permission("<字面量>")` 调用点 → **放在 `crates/platform-oauth/src/` 里**
   （C-5 只扫该 crate 自己的 `src/`，D240）。**不许**写在 `platform-server` 里。
3. **scope 三重交集**具体在哪一层算？`请求 ∩ client.allowed_scopes ∩ 用户有效权限点`。
   「用户有效权限点」怎么拿（`platform-oauth` 不依赖 `platform-rbac`）？
   → 走 `PermissionProvider` 还是别的接缝？**回答并给出依据**。
4. **`client_credentials` 的两重交集**（D231）：`请求 ∩ client.allowed_scopes`，**无用户维**。
5. **令牌签名的 claim 集合**（design §5.1）：`iss` / `aud`(=client_id) / `sub`(client_credentials **省略该键**) /
   `tid` / `client_id` / `scope` / `grant_type` / `iat` / `nbf` / `exp` / `jti`。
   `sub` **省略该键**（不是 null、不是空串）怎么做到？
6. **登出怎么作废全部会话？** design §4.5 提到 `IamStore::revoke_all_refresh_tokens`
   （`crates/platform-iam/src/store.rs:765`）—— 确认这个符号**存在**；
   已有签发的**登录 access token 不杀**（D229 / TD-043）→ 在报告里明说。
7. **back-channel 投递的 URL 从哪来？** **只**能取自 `platform_oauth_client.backchannel_logout_uri`（注册时校验 scheme）→ **无 SSRF 入口**（D220）。你怎么保证？
8. **`{授权页基址}` 怎么读？** 配置 `oauth.frontendBaseUrl`，**空串时回退 `{issuer}`**（D222）。
   配置读不到 / 未登记时怎么处理？
9. **5 个配置项的运行时读取路径**：`platform_config_upsert` 写进 `platform_config` → 运行时怎么读？
   装配时读一次，还是每请求读？（**影响「配置改了要生效」的语义** → 说清。）
10. **`oauth.frontendBaseUrl` 与 `{issuer}` 在 E2E 里不相等**（`https://e2e.base` vs `http://127.0.0.1:8080`）
    → **一切授权页 / 设备码页 URL 都用 `{授权页基址}`**。你怎么保证没有一处用了 `{issuer}`？
11. **`/api/oauth/device/verify` 与 `/api/oauth/device/approve`**（**D257 补的**）：
    批准时**写 grant 行**（`tenant_id` / `client_id` / `user_id` / `scopes` / `status=active` / `consent_at`）——
    怎么保证与 `authorization_code` 路径写出来的 grant **同构**（D202 一表三用）？
12. **启动对账 fail-fast**：manifest 改了但种子迁移没加 → 服务**起不来**（AC-S-153 步骤 2）。
    这条是**平台既有能力**（`crates/platform-rbac/src/registry.rs:198` `require_consistent`）
    → 确认你的 manifest + 种子迁移**一致**，否则**故意不改**服务也起不来。

---

## 5. 测试与自验（**§9.14 阶段一：增量测试**）

> ⚠️ **本阶段不跑 E2E**（代码未冻结，棒 2 还没落地）。
> 终版（代码冻结后、合并前）由 BA **集中跑一次**全量 + E2E。

### 5.1 环境

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
```

- ⚠️ **应用连接用 `base_app`**（`base_platform_app` 是 NOLOGIN 组角色，连不上）；
  **造数 / 篡改 / 直连查**用 `base`。
- ⚠️ **本机没有 `psql` 客户端** → 用
  `podman exec base-postgres psql -U base -d base_test -tAc "…"`。
- ⚠️ 绝不写 `base_dev`。

### 5.2 增量测试范围（**只跑被改动的 crate + 直接依赖方**）

```bash
# 本轮新增
cargo test -p platform-oauth --all-targets -- --test-threads=1
# 直接依赖方（platform-oauth 改了 → iam/core/db 被依赖；iam 改了 → server）
cargo test -p platform-iam --all-targets -- --test-threads=1
cargo test -p platform-core --all-targets -- --test-threads=1
cargo test -p platform-db --all-targets -- --test-threads=1
cargo test -p platform-server --all-targets -- --test-threads=1
# 真 C-5
cargo test -p platform-oauth --test contract_c5
```

**硬规矩**（踩过的坑）：

| 规矩 | 原因 |
|---|---|
| `--all-targets` **不可省** | 否则 lib 单测不跑 |
| **只能有一个 `--`** | `cargo test -- --x -- --y` → **0 passed 假绿** |
| `--include-ignored` 放 `--` **之后** | 真实库用例靠它才真跑（否则是 skip） |
| **串行**：`-- --test-threads=1` | 低配机 + 真实库并发会 flake |
| **不要** `cargo test --workspace` | workspace 有 **13 个成员**，其中 **12 个有测试**；workspace 级跑必失败 → **逐 crate 跑** |
| `CARGO_TARGET_DIR` 指向 `code/target` | 复用编译产物，否则低配机重编到哭 |

### 5.3 判据来源：**131 条用例**

`BA/demands/REQ-022-outbound-idp/test-cases/` 的 12 个文件（index + 11 个主题）。
**逐条映射**到你的测试函数名，并在报告里给出「AC → 用例 → 测试函数」的对照表。
**22 条 AC 必须 100% 有覆盖**。

### 5.4 变异验证（**必须做**，方法照抄 REQ-014 的做法）

每条变异必须：
1. 改**生产那一行**（不是测试里的）；
2. 跑**相关**测试 → 确认**变红**；
3. 用**备份副本**还原（**不用** `git checkout` —— 未跟踪文件还原不了）；
4. **必须 `touch`**（`cp -p` 保留旧 mtime → cargo 复用陈旧二进制 → 假绿）；
5. `sha256sum -c` 确认**逐字节相同**；
6. `grep -c 'MUTATION' <file>` → **0**。

```bash
cp <file> /tmp/x.bak            # 备份（用 cp 不用 cp -p）
sha256sum <file> > /tmp/sha-before.txt
# 注入最小变异 → 跑测试 → 确认红
cp /tmp/x.bak <file> && touch <file>
sha256sum -c /tmp/sha-before.txt
grep -c 'MUTATION' <file>        # 必须 0
git status --short
```

**建议至少覆盖这些「生产那一行」**（不够就补，报告里逐条给数字）：

| 变异点 | 它该证明哪条断言不是空跑 |
|---|---|
| `platform_oauth_code` 的 `code_hash` 口径：base64url → hex | D228 的摘要口径真的被断言 |
| `PKCE` 校验：改成比较 `code_challenge` 原文 | S256 真的被算过（AC-S-138） |
| 授权码交换：把「删除授权码」挪到签发令牌**之后** | 一次性真的成立（AC-S-139） |
| scope 交集：去掉「用户权限」那一维 | 三重交集真的成立（AC-S-140） |
| 内省：去掉「属于本 client？」判定 | 越权内省被挡住（AC-S-143 步骤 2） |
| 撤销：grant 级 → 只撤那一枚令牌 | grant 级语义（AC-S-147 步骤 4） |
| 敏感键扫描器：完整键名匹配 → 子串匹配 | 扫描器不是摆设（D243） |
| RLS：把 `WITH CHECK` 去掉 | 写侧隔离（AC-S-155 步骤 5） |
| `TITLES` 行数断言：93 → 92 | 登记完备性（AC-S-152） |

### 5.5 静态扫描 / 敏感值扫描 —— **必须带阳性对照**（出口标准 9.2）

- 敏感键扫描（AC-S-136 步骤 5 / AC-S-154 步骤 4）：
  → 在**临时副本**里塞一个 `client_secret` 字段 → 扫描器**必须命中**（**阳性对照**）。
  → 真实代码上 **0 命中**。
- 「0 命中」**先证明扫描器会命中**，再说它没命中。
- ⚠️ **不得**用 `RandomState`（哈希种子不确定 → 断言不可复现）。用树里已有的 `sha2`。

### 5.6 格式化

```bash
cargo fmt -p platform-oauth -p platform-iam -p platform-core -p platform-server -p platform-xtask
```

⚠️ `cargo fmt --all --check` **基线就是红的**（**11 个存量文件**，TD-017）→
**只格式化你改过的文件**，不要顺手修存量（会把 diff 撑大）。

---

## 6. 报告（**写进 `feature-meta/req-022-dev-backend.md`**）

> 路径：`/home/myg133/Projects/system_seed/feature-REQ-022/feature-meta/req-022-dev-backend.md`
> （结构照抄 `BA/feature-meta/req-014-dev-backend-foundation.md`，在 `feature/REQ-014` 分支上）

必含章节：

1. **交付清单（文件 → AC）** —— 逐文件一行，写清改了什么、对应哪条 AC / 哪条决策
2. **逐 crate 测试数字** —— **自己跑出来的**：`passed` / `failed` / `ignored`，
   并说清统计口径（= 该 crate 所有 test target 的 `passed` 之和）
3. **变异验证证据** —— 逐条：变异点（**生产那一行**）→ 变红的测试名 → 数字 → 还原校验结果
4. **AC → 用例 → 测试函数 的完整对照表** —— **22/22**
5. **开工前必答项（§4 的 12 条）逐条回答**
6. **与规格不一致的发现** —— 每条写：现状 / 证据（`file:line`）/ 你怎么处理的 / **需要 BA 拍板吗**
   （**不要**自己改规格；**也不要把偏离悄悄咽下去**）
7. **未完成的部分 + 原因** —— 逐条写清
8. **棒 2 接入时必须遵守的形状** —— API 字段名 / 错误码 / 路由 / 事件码
9. **对棒 1 文件集之外的任何改动** —— 一律列出来（应该是空的）

> ⚠️ **注释与代码必须一致**（dev-guide 强制项）。注释比代码旧**比没有注释更坏**。

---

## 7. 明确的「本轮不做」（**不要顺手做**）

| 项 | 依据 |
|---|---|
| refresh token | D217 |
| 底座业务端点接受第三方 OAuth 令牌（opt-in） | D209 / TD-041 —— **本轮有意不做** |
| 登出杀掉已签发的**登录** access token | D229 / TD-043 —— 显式写明现状 |
| `client_secret` 轮换端点 | D226 |
| 多租户各自的 client（client 是**平台级**） | D219 |
| 修 `cargo fmt --all --check` 的 11 个存量文件 | TD-017 |
| 修 `scripts/e2e-server.sh` 不能从零建库 | TD-039 —— **记录**即可 |

---

## 8. 完工信号

报告写完后，在**你的最终回复**里给出：

- 报告的**绝对路径**
- **逐 crate 的 `passed` / `failed` / `ignored` 数字**（自己跑的）
- **变异验证条数** + 是否全部还原干净（`sha256sum -c` 全过 + `grep -c MUTATION` 全 0）
- **AC 覆盖 = 22/22**
- **未完成项清单**（若有）
- **需要 BA 拍板的问题**（若有）
