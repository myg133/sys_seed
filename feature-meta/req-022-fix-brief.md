# REQ-022 修复轮任务卡（独立复核 FAIL 后）

- **触发**: 独立复核（`req-022-independent-review.md`，989 行）判 **FAIL**
- **复核结论**: BA 判 22/22 通过；复核判 **9 通过 / 6 判不了 / 7 不通过**
- **BA 的态度**: **接受 FAIL**。BA 已逐条核实复核的 16 条指控，**全部成立**。
- **根因（认同复核的判断）**: BA 的 22 条判定是**照着 AC 的「预期结果」逐句回填落点**，
  而不是「先打开测试源码看断言、再判 AC」。**形式上的 22 行 ≠ 实质上的 22 次核对。**
- **本轮不改任何 AC 措辞**，也不回滚 D259/D260/D261 —— 只是把 AC 已经写好的判据**真的实现**。

---

## 0. 证据：BA 亲验的 6 条指控（全部成立）

| # | 指控 | BA 的实测命令 | 结果 |
|---|---|---|---|
| 1 | AC-S-145 ES256 验签零覆盖 | `grep -rn "verify\|signature\|p256\|ecdsa" crates/platform-oauth/tests/oauth_http.rs` | 只有一条注释写「**只解，不验签**」→ **0 处验签** |
| 2 | AC-S-151 只调 IAM 函数没打端点 | `grep -n "me/permissions\|audit/events\|verify_access_token"` | 只有 `fixture.iam.verify_access_token()` **2 处**，**0 处**打业务端点 |
| 3 | AC-S-140 路径 5 + D241 三条对照缺失 | `grep -c "不回溯\|rbac:role:\*"` | **1**（只在注释里） |
| 4 | AC-S-154 用内存假 sink | `grep -n "RecordingSink\|PgAuditSink"` | `common/mod.rs:186 pub struct RecordingSink(Arc<Mutex<Vec<AuditEvent>>>)` → **不落库** |
| 5 | AC-S-135 回调白名单拒绝零覆盖 | `grep -rc "OAUTH_INVALID_REDIRECT_URI" crates/platform-oauth/tests/` | **全部 0** |
| 6 | AC-S-149 `logout_token` 内容零解析 | `grep -c "sid\|schemas.openid.net\|nonce"` | **0** |
| 7 | AC-S-134 `{授权页基址}` 无区分力 | `grep -n "OAuthSettings::default()"` | `common/mod.rs:379` → 空串回退 issuer → 两者相等，**判不出** |
| 8 | AC-S-139 TTL 60s 无对照 | `grep -c "expires_at" oauth_http.rs` | **0** |
| 9 | AC-S-137 步骤 3 未登录回跳未测 | `grep -n "login(page" e2e/oauth.spec.ts` | 7 处 `login()` **全部在 `page.goto('/oauth/authorize')` 之前** → 从未走「未登录 → 登录 → 回跳」 |
| 10 | `PreviewResult.scopes` 注释/代码不一致 | 看 `service.rs:160` vs `:903` | 注释「**实际授予**的 scope」，代码传 `requested`（**未取交**）→ **违反 AC-S-146「用户所见 = 实际授予」** |

---

## 1. 你的任务：补齐 12 项判据（按优先级）

> ⚠️ **本轮的验收标准是「每一处新断言都能被变异打红」**。
> 不是「加了个测试就算」，而是「实现被改坏时这个测试会不会红」。
> **每加一处，报告里必须写：它对应哪条 AC、以及你打算用什么变异证明它不是空跑。**

### P0-1（B-01 / AC-S-151）业务端点**默认拒绝**的**端到端**证据

**现在的问题**：`a_third_party_oauth_token_is_rejected_by_the_platform` 只调了
`fixture.iam.verify_access_token(&oauth_token).is_err()` —— 测的是**一个函数**，
不是「平台业务端点拒绝它」。

**要做的**：在 `crates/platform-server/tests/` 下**新建**集成测试（真装配 + 真路由）：

1. 造一枚有效 OAuth access token（走 client_credentials 或 authorization_code）；
2. 用它在**同一进程的真 Router**上：
   - `GET /api/rbac/me/permissions` → **401**，`code == "UNAUTHENTICATED"`，
     且响应体**不含 `error` 键**（D224）；
   - `GET /api/audit/events`（`tenant-a`）→ 同上；
3. **阳性对照（必须有）**：用**正常登录令牌**打同样两个端点 → **成功**
   → 若对照组也 401，说明平台整体坏了，本条判据**无区分力** → 判 fail；
4. **3.5**：用该 OAuth 令牌调 `POST /api/oauth/introspect`（带 client-A 的 `client_secret_basic`）
   → **200 + `active == true`** → 证明令牌「只在 OAuth 协议端点上有效」。

> 参考：`crates/platform-server/tests/` 里既有的真库/真路由测试怎么装配（读几个再动手）。

### P0-2（B-02 / AC-S-145）**测试侧独立 ES256 验签**

**现在的问题**：只断了 `alg == "ES256"`、`kid` 在 JWKS 里、claims 齐全 —— **一个字节都没验签**。

**要做的**（AC-S-145 步骤 3/4/5）：

1. `GET /.well-known/jwks.json` 取公钥；
2. **测试侧独立实现** ES256 验签（**不调用生产代码**）：
   - 可以直接用 `jsonwebtoken`（已在树里）的 `decode` + `Validation`（**独立于平台实现**即可），
     或用 `p256`（也在树里）手工验 —— **不能用 `platform_oauth::keys` 的任何函数**；
3. 断言验签**通过**；
4. **过期令牌**：验签**仍要通过**（签名有效），但 `exp` 已过期 → 结论是「已过期」
   → **「签名无效」与「已过期」必须是两条不同的断言**；
5. `client_credentials` 令牌：`sub` **键不存在**（`get("sub").is_none()`），其余 claims 齐全。

### P0-3（B-03 / AC-S-146）自签票据**越权**判据（D260 的替代判据）

**现在的问题**：D260 把原判据换成「交换令牌必须 `invalid_scope`」，
但**替代判据也没实现** → 私钥泄露这件事**零覆盖**。

**要做的**：

1. 用 `target/e2e-jwt-private.pem`（或测试侧自造的私钥，与平台同 kid）**自签**一个
   `typ="oauth_consent_ticket"`、`scope` **比 preview 返回的更大**的票据；
2. 调 `POST /api/oauth/authorize/confirm`（**允许它 200** —— D260 的实现选择）；
3. **关键断言**：用拿到的 `code` 交换令牌 → **400 + `{"error":"invalid_scope"}`**，
   且 `platform_oauth_token` 里**没有任何行**（按 `jti` 或按 client 查）；
4. 并断言 `platform_oauth_grant` 里**最多**多了一行（TD-046 的已知代价）——
   把这个代价**测出来**，不要让它成为「没人知道的行为」。

> ⚠️ 如果测试里拿不到私钥（E2E 才落盘），可以在集成测试里**自己生成**一对 EC 密钥并
> 用它签票据 + 让平台的 `JwtKeySet` 用同一把私钥验签（看 `test_iam_config()` 怎么给密钥）。

### P0-4（B-06 / AC-S-154）审计**落库 + 哈希链**（不用假 sink）

**现在的问题**：`RecordingSink` 是内存 `Vec` → 测的是「有没有调用 sink」，
不是「审计真的落库、链真的自洽、从表里扫敏感值真的能扫到」。

**要做的**（AC-S-154 步骤 1~4）：

1. 用**真审计 sink**（`PgAuditSink`）装配（参考 `crates/platform-audit/tests/audit_real_db.rs`）；
2. 走一遍：注册 client → 同意 → 换令牌 → 撤销 → 登出；
3. 查 `platform_audit_log`（`tenant-a`）→ 至少 5 个事件码：
   `oauth.client.registered` / `oauth.authorization.approved` / `oauth.token.issued` /
   `oauth.token.revoked` / `oauth.logout.executed`，每条含固定字段集（契约 C12）；
4. **哈希链自洽**：`ChainVerifier::verify(tenant-a)` → `ok == true`；
5. **敏感值扫描**：输入 = 审计行的全部文本列（`action` / `object_type` / `object_id` /
   `before` / `after`），输出 = 命中的敏感键 → 真实数据 **0 命中**；
6. **阳性对照**：管理员直连插一行 `before = '{"client_secret":"POSITIVE-CONTROL"}'`
   → 再扫 → **必须命中**；删掉 → **必须不命中**。

### P0-5（B-05 / AC-S-149）`logout_token` 的**内容** + 失败不影响登出

**现在的问题**：只证明了「有东西被投递到 mock」，`logout_token` 的 payload 一次没解析。

**要做的**（AC-S-149 步骤 2~5）：

1. 从 `target/mock-logout-received.jsonl`（或集成测试里自己的接收端）读出 `logout_token`，
   **测试侧独立解析 payload**（不验签也行，但要有 decoding）；
2. 断言：`iss == 平台 issuer`、`aud == client-A 的 client_id`、
   `sub` / `sid` / `jti` / `iat` **齐全**、
   `events` **含键** `http://schemas.openid.net/event/backchannel-logout`、
   payload **不含 `nonce`**；
3. **`sid` 逐字等于**触发登出的登录令牌的 `sid`（D230）；
4. **`MOCK_MODE=timeout` 与 `MOCK_MODE=500` 各跑一次** → **登出本身仍 200**，
   grant 已撤销（**通知失败绝不能让登出失败**）；
5. 投递失败时**有日志**（含 `clientId` 与失败原因）。

### P1-6（B-04 / AC-S-140）路径 5（不回溯）+ D241 的三条对照

**现在的问题**：路径 4 掉进了 AC 原文警告的陷阱（`rbac:role:*` 未注册 → 400 的原因
可能只是「没注册」，而不是「通配一律不生效」）；路径 5 完全没有。

**要做的**：

1. **路径 4 的真对照**（D241 第 3 条）：把 `rbac:role:*` **登记进 `platform_permission`**
   （管理员直连 `INSERT`，**仅本用例夹具**，用完 `DELETE`）→ 重跑路径 4 → **仍然** 400 `invalid_scope`；
2. **路径 5（不回溯扩大）**：
   - 请求 `scope=rbac:role:read` → 交换**成功**（令牌 A）；
   - 管理员给该用户**追加** `rbac:role:write`（`platform_role_permission` + **bump `platform_permission_epoch`**）；
   - **重放令牌 A**（不重新交换）→ 内省 → `scope` **仍逐字等于** `rbac:role:read`，**不含** `rbac:role:write`；
   - 另发令牌 B（scope 集 = `rbac:role:read rbac:role:write`）→ 读 B 的 scope **含** `rbac:role:write`。

### P1-7（B-07 / AC-S-135）**回调白名单**拒绝

**要做的**：`preview` 用一个**不在** `redirect_uris` 里的地址 → **400 + `OAUTH_INVALID_REDIRECT_URI`**。
（注意：`OAUTH_INVALID_REDIRECT_URI` 目前在测试里出现 **0 次** —— 它是不是**死码**？如果代码里也没用到它，报告里必须说明。）

### P1-8（H-01 / AC-S-134）`{授权页基址}` 的**区分力**

**现在的问题**：夹具用 `OAuthSettings::default()`（`frontendBaseUrl` 空串 → 回退 issuer）
→ `authorization_endpoint` 与 `issuer` 前缀**相同** → 判不出「用了哪个」。

**要做的**：夹具把 `frontendBaseUrl` 设成**与 issuer 不同**（如 `https://frontend.example`），
然后断言：
- `authorization_endpoint == "https://frontend.example/oauth/authorize"`（**不是** issuer 前缀）；
- `verification_uri == "https://frontend.example/oauth/device"`；
- `verification_uri_complete == "https://frontend.example/oauth/device?user_code=…"`；
- 而 `token_endpoint` / `jwks_uri` 等仍用 **issuer** 前缀。

### P1-9（H-04）`PreviewResult.scopes` 的注释与行为对齐

**现在的问题**：`service.rs:160` 注释写「**实际授予**的 scope」，代码 `:903` 传的是 `requested`
（**未与交集取交**）→ 同意页展示的可能是用户**不该拿到**的 scope
→ **直接违反 AC-S-146「用户所见 = 实际授予」**。

**要做的**（**两种修法选一种，并在报告里说明理由**）：
- **A（推荐）**：`preview` 时真算交集（请求 ∩ client 白名单 ∩ 用户权限点），
  `scopes` 返回**交集后的**；请求里有越界项时**仍 200**（展示能给的），
  但把越界项**排除**（并可回一个 `deniedScopes` 供前端提示）——
  这样同意页展示的就是真会授予的；
- **B**：改注释，明确 `scopes` = 「请求的 scope（**未**取交）」，
  并**同时**改前端同意页文案（不能让人以为那就是会授予的）。

⚠️ 若选 A，要**加一条测试**：请求 `{read, write}`（用户无 write）→ preview 的 `scopes` **只含 read**，
且**后续交换仍必须 400 `invalid_scope`**（AC-S-140 的判据不能因此失效 —— 这条要**同时**成立）。

### P2-10（H-02 / AC-S-139）TTL 90s / 30s 对照

按 **D252** 逐列写死直插 `platform_oauth_code`：
- `expires_at = now() - interval '90 seconds'` → 交换 **400 `invalid_grant`**；
- **同一批值只改** `expires_at = now() + interval '30 seconds'` → 交换**成功 200**；
- 两条断言**只在 `expires_at` 上不同**，其它列逐字相同。

### P2-11（H-03 / AC-S-142）设备码的库侧断言

- `slow_down` 后 `platform_oauth_device_code.poll_interval_seconds` **== interval + 5**；
- `last_polled_at` **非空且递增**；
- 拿 `user_code` **当 `device_code`** 用 → **失败**；
- 换令牌后设备码行**已被删除**（按 `device_code_hash` 查 == 0）。

### P2-12（AC-S-136 补 2 个面）步骤 1 + 步骤 4 的输出面

- 步骤 1：以 `base` 直连查 `client_secret_hash` 列，与测试侧**独立算**的
  `base64url(SHA256(明文))` **逐字节**比对（**不是** hex）；
- 步骤 4 的**四处输出**都要逐一断言不含明文：`GET /clients`、`GET /clients/{id}`、
  `POST /token` 的**响应体**、`GET /api/audit/events` 的**全表文本**。

### P1-13（E2E）未登录 → 登录 → **回跳后 query 还在**

**现在的问题**：E2E 的 7 处 `login()` **全在**访问授权页**之前** →
「未登录进去 → 点登录并继续 → 登录 → 回到原 URL 且 query 一个不丢」**从未被测**。
**那正是 D221 的核心判据。**

**要做的**（`e2e/oauth.spec.ts` 加一个用例）：

1. **不先登录**，直接 `page.goto('/oauth/authorize?…')`（带全部 6 个参数）；
2. 断言页面渲染「**登录并继续**」，且**没有**发 preview 请求（可以用 `page.on('request')` 计数）；
3. 点「登录并继续」→ 地址变成 `/login?return_to=…`；
4. 填表登录；
5. **关键断言**：回到 `/oauth/authorize` 后，
   `client_id` / `state` / `code_challenge` / `redirect_uri` / `scope` / `response_type`
   **全部仍在**（`new URL(page.url()).searchParams` 逐个比）。

---

## 2. 硬规矩（与之前一致）

- **只改** `crates/platform-oauth/tests/**`、`crates/platform-server/tests/**`、
  `e2e/oauth.spec.ts`、`crates/platform-oauth/src/**`（仅 H-04 需要的改动）
- ⚠️ **不要碰** `apps/shell/**` 与 `packages/i18n/**`（除非 H-04 选方案 B 要改前端文案 —— 那要先问 BA）
- **不要** `git commit` / `checkout` / `stash` / `reset` / `add -A`
- **零新增第三方依赖**（`jsonwebtoken` / `p256` / `sha2` / `base64` 都已在树里）
- **只跑增量**：`cargo test -p platform-oauth -p platform-server --all-targets -- --test-threads=1 --include-ignored`
  + `npx playwright test e2e/oauth.spec.ts`
  ⚠️ **不要**跑 14 个 crate 的全量（那已经跑过；本轮只重跑受影响的）
- **变异验证**：本轮**每一处新增判据**都要有一条对应的变异（改坏生产代码 → 该判据变红）
  → 报告里逐条给「变异点 / 变红的测试 / 数字 / 还原校验」

## 3. 报告

写进 `/home/myg133/Projects/system_seed/feature-REQ-022/feature-meta/req-022-fix.md`，必含：

1. **逐条对照**：复核的 16 条 → 你的处置（修了 / 为什么不修）
2. **12 项新判据**：每项写「对应 AC / 测试函数名 / **它的变异证明**」
3. **H-04 你选了 A 还是 B，为什么**
4. **修复后的测试数字**（自己跑的）
5. **新发现的与规格不一致**（若有）
6. **未完成项 + 原因**

## 4. 完工信号

- 报告绝对路径
- **新增/修改的测试函数个数**
- **变异验证条数**（每处新判据都有一条）
- **修复后的数字**：`platform-oauth` / `platform-server` / E2E
- **12 项里哪些做完了、哪些没做**（如实）
