# REQ-022 独立复核报告（verifier，非 BA 自查）

- **需求**：REQ-022 出站 IdP 与单点登出（父需求 REQ-001）
- **复核对象**：`feature-REQ-022`（分支 `feature/REQ-022`，基线 `develop@310e4d3`）
- **复核者**：独立 verifier 子 Agent（**不是** BA 本人）
- **复核时间**：2026-11-21 03:08 ~ 03:40
- **规格基线**：`acceptance.md` rev 2 + rev 3 + rev 4 补记（22 条 AC）／`decisions.md` D196~D261
- **被复核的报告**：`BA/feature-meta/req-022-final-verification.md`（BA 亲跑，自称 22/22 通过）
- **本报告结论**：**FAIL** —— 22 条 AC 中 **7 条不通过**、**6 条判不了（部分）**、**9 条通过**（见 §6），
  另有 1 处**注释与代码不一致**（§4）、多处「已实现但未被任何测试验到」的判据缺口。
  **BA 判 22/22 全部通过；我判 13 条与 BA 不一致**（见 §6.3 对账表）。

---

## 1. 复核范围与方法

### 1.1 我实际做了什么

**不是读一遍报告就下结论**。我做了四件事：

1. **逐条打开测试源码看断言**（22 条 AC 全部）—— 不信任 BA 报告 §4 的表格。
2. **跑测试**（3 组，共 6 个 crate 口径）：
   - `cargo test -p platform-oauth --all-targets -- --test-threads=1 --include-ignored` → **117 passed**（自查与 BA 一致）
   - `cargo test -p platform-iam / platform-core / platform-server --all-targets -- --test-threads=1 --include-ignored` → **180 / 153 / 129 passed**（与 BA 报告逐 crate 一致）
   - `pnpm --filter @base/shell test` → **279 passed / 23 files**（与 BA 一致）
3. **自己注入 2 条变异并复验**（M4 / M8），核实「变异真的改生产那一行、真的变红、真的还原干净」。
4. **本地实测 PostgreSQL 语义**（核实棒1 §3.1.1 的 M8 说法）。

### 1.2 我跑了多少条命令

约 **58** 条 shell 命令（含 4 次 `cargo test` 调用、5 次 `podman exec psql` 直连查库、约 35 次
`grep`/`read` 源码检索、2 次变异注入与还原、1 次前端测试）。

### 1.3 环境

```
XDG_RUNTIME_DIR=/run/user/$(id -u)
CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test
PostgreSQL 17.12（podman base-postgres，healthy）
```

**关键数字（我自己算的）**：

| 项 | 我的实测 | BA 报告 | 一致？ |
|---|---|---|---|
| platform-oauth | **117 passed / 0 failed / 0 ignored** | 117 | ✅ |
| platform-iam | **180 passed / 0 failed / 0 ignored** | 180 | ✅ |
| platform-core | **153 passed / 0 failed / 0 ignored** | 153 | ✅ |
| platform-server | **129 passed / 0 failed / 0 ignored** | 129 | ✅ |
| @base/shell | **279 passed / 23 files** | 279 | ✅ |
| `capabilities/` 文件数 | **14** | 14 | ✅ |
| `permissions.json` total | **32** | 32 | ✅ |
| `TITLES.len()` | **93**（9 条 `OAUTH_*`） | 93 | ✅ |
| 改动集 | **42 项** | 42 | ✅ |

> ⚠️ `platform-oauth` 的 117 = 91 lib 单测 + 2 contract_c5 + 22 oauth_http + 2 tenant_isolation。

---

## 2. A 判据真实性（22 条 AC 逐条，含我打开源码看到的断言）

> 每条给：**判据在哪** → **我看到的断言** → **有区分力吗** → **结论**。
> 「假绿」= 报告说验了、实际断言无区分力或根本没跑。

### AC-S-134 元数据可发现 — ⚠️ **部分假绿（授权页基址那一维判不了）**

- 判据落点：`tests/oauth_http.rs:315` `the_authorization_server_metadata_is_discoverable`
- 我看到的断言（`:315-361`）：`issuer` / 6 个端点 / `grant_types_supported` 逐元素（顺序钉死）/
  `response_types_supported == ["code"]` / `code_challenge_methods_supported == ["S256"]` /
  两个 `auth_methods` 恰好 `["client_secret_basic"]` / `access_token_expires_in == 3600`。**很扎实**。
- **但是**：`authorization_endpoint` 断言的是 `"base/oauth/authorize"`（`:327`）——
  而**夹具的 `frontendBaseUrl` 是空串**（`tests/common/mod.rs:379` 用 `OAuthSettings::default()`，
  `config.rs:78` `frontend_base_url: String::new()`）→ `frontend_base()` **回退到 issuer `"base"`**。
  → 这条断言**在 `{授权页基址} == {issuer}` 时通过**，**判不出**「代码误用了 `{issuer}`」。
  D222 的整个要点是「E2E 里二者**不相等**」。**全仓没有任何测试把
  `frontend_base_url` 设成与 issuer 不同的值再断言端点**（我 grep 过：
  `frontend_base_url` 只在 `config.rs` 的**单元测试**里出现过，而那里测的是 `frontend_base()` 函数本身，不是元数据端点）。
- E2E（`e2e/oauth.spec.ts:323`）也只断 `response_types_supported` / `grant_types_supported`，**不断 `authorization_endpoint`**。
- **结论：缺陷** —— 如果 `metadata()` 里把 `format!("{base}/oauth/authorize")` 改成
  `format!("{issuer}/oauth/authorize")`（`service.rs:1795`），**当前测试全绿**。这是 D222 的核心禁令，
  判据**没有区分力**。

### AC-S-135 client 增删改停 + 回调白名单 — ⚠️ **回调白名单判据完全缺失**

- 判据落点：`tests/oauth_http.rs:365` `a_client_can_be_registered_disabled_and_deleted`
- 我看到的断言：201 CREATED + `clientId` 非空 + `clientSecret` 有值 + **无** `clientSecretHash` /
  列表无 `clientSecret` / PATCH → 200 `disabled` / preview **403 `OAUTH_CLIENT_DISABLED`** /
  token **401 + 逐字 `{"error":"invalid_client"}`** / DELETE → 204 / 再查 → **404 `OAUTH_CLIENT_NOT_FOUND`**。
- 这些**都对**、**有区分力**。
- **但是**：AC-S-135 的标题是「**且回调白名单生效**」。我 grep 全仓：
  `OAUTH_INVALID_REDIRECT_URI` 在**后端测试里 0 次**（只在前端 `apps/shell/test/oauth-api.test.ts:135`
  与 `oauth-authorize.test.ts:248` 出现，那是前端渲染错误提示的测试，**不是后端拒绝的测试`）。
  生产代码**有**这个校验（`service.rs:834` `client.redirect_uris.iter().any(|uri| uri == redirect_uri)`），
  但**没有任何测试**用一个「不在白名单里的 redirect_uri」去打 preview 并断言 400。
- **后果**：把 `service.rs:834` 的 `!...any(...)` 改成恒 false（放行任何回调）→ 所有测试全绿。
  **开放重定向**这条安全判据**没有测试**。

### AC-S-136 secret 只存摘要、不回显 — ⚠️ **判据只有一半**

- 判据落点：`tests/oauth_http.rs:502` `the_client_secret_is_never_echoed_by_a_read_endpoint` +
  `crypto` 单测 + `sensitive` 扫描。
- 我看到的断言：
  - `GET /clients` 与 `GET /clients/{id}` 都 `get("clientSecret").is_none()` + `!to_string().contains(&secret)` ✅ **有区分力**
  - `crypto::tests` 4 条（43 字符 / base64url 字符集 / 无填充 / **不是 hex** / 确定性 / 与 iam 一致）✅
  - `sensitive::tests::the_crate_source_has_no_sensitive_response_field`：**阳性对照先行**（往临时副本塞
    `clientSecret` → 必须命中），**再**断言真实源码 0 命中 ✅ **这一条是真的**（我读过实现：`sensitive.rs:362-393`）
- **缺口（AC 步骤 1 与步骤 4 的后两个面）**：
  - **步骤 1**（直连查 `platform_oauth_client.client_secret_hash`，与测试侧独立计算的摘要**逐字节比对**）：
    `client_secret_hash` 在 `tests/oauth_http.rs` 里 **0 次出现**（我 grep 过）。测试侧**确实**有独立实现
    `digest_of()`（`:69`，不复用生产代码 ✅），但它**没有**用来比对 DB 里 `client_secret_hash` 那一列——
    只在**授权码**（`platform_oauth_code.code_hash`）上用了。→ **AC-S-136 步骤 1 未验**。
  - **步骤 4** 的四个面里，**`POST /api/oauth/token` 的响应体**与 **`GET /api/audit/events` 的全表文本**
    这两处**没有**断言「不含明文 secret」（我只在 client 读接口上看到）。
- **结论：缺陷** —— AC-S-136 步骤 1 未验 + 步骤 4 的两个输出面未验。base64url 口径本身有人守（授权码那条），
  但 **`client_secret_hash` 列**没有任何断言。

### AC-S-137 授权码 + PKCE 端到端 — ✅ **通过（E2E 兜住了）**

- 判据落点：`tests/oauth_http.rs:568`（后端）+ `e2e/oauth.spec.ts:150`（端到端）
- 后端断言：preview → ticket；confirm → `redirectTo` 以 `https://client.example/cb?code=` 开头 + `state=xyz`；
  交换前**先**用独立摘要查「1 行」+ 明文 0 行（`:631-650`，注释明写「交换后再断会恒真」——**这个自觉很到位**）；
  交换 200 + `token_type: Bearer` + `scope` + `expires_in: 3600`；内省 `active: true` + scope 含 `rbac:role:read`。
- E2E：`page.route('https://client.example/**')` 拦截（D235 的要求，**真的做了**）→
  `new URL(page.url()).searchParams.get('code')` 非空 + `state === 'xyz'` → 用该 code **真的**换到令牌。
- **步骤 3（未登录 → 登录 → 回跳 query 还在）**：E2E **没有**测这条路径（所有 E2E 都是先 `login()` 再 `page.goto(authorizeUrl)`）。
  但**前端单测** `apps/shell/test/oauth-authorize.test.ts:146-171` **逐参数断言**了 `return_to` 保留
  `client_id` / `state` / `code_challenge` / `code_challenge_method` / `redirect_uri` / `scope` —— **这一条是真的、有区分力**。
- **结论：通过**（步骤 3 由前端单测覆盖，不是 E2E，但判据成立）。

### AC-S-138 PKCE 强制 S256 — ✅ **通过**

- 判据落点：`tests/oauth_http.rs:824`
- 三条独立路径全在：缺挑战 → 400 `OAUTH_INVALID_REQUEST` + **无 `ticket` 键**（`:843`）；
  `plain` → 400 `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD`（`:861`）；
  错 verifier → 400 `invalid_grant` + `error_description` **不含 CJK**（逐字符扫 `\u{4e00}..\u{9fff}`，`:929`）
  + **不含** `code_verifier`/`code_challenge`/`client_secret`/`wrong-verifier`（`:936-943`）。
- `pkce::tests` 另有 7 条（含 RFC 向量、`the_challenge_is_recomputed_not_compared_literally`）。
- **结论：通过**，有区分力。

### AC-S-139 授权码一次性 / 只存摘要 / 60 秒 — ⚠️ **一半判据缺失（TTL 那半）**

- 判据落点：`tests/oauth_http.rs:568`（一次性 + 摘要口径）+ `:741`（并发）
- 我看到的断言：独立摘要查「交换前 1 行」→ 明文 0 行 → 交换 200 → 重放 400 `invalid_grant` → 摘要 0 行 ✅。
  并发测试：**4 路** `tokio::join!`，**恰好 1 个 200**、其余 400 `invalid_grant`、**令牌表恰好 1 行** ✅
  （我读过源码，注释解释了为什么 2 路不够、为什么必须断令牌行数 —— **这条判据很硬**）。
- **缺口**：
  - **步骤 5**（`information_schema.columns` 里 `platform_oauth_code` 不含 `consumed_at` / `used_at`）：
    `information_schema` 在 `oauth_http.rs` 里 **0 次**（只出现在 `tenant_isolation.rs:218`，查的是 client 表的 `tenant_id`）。
    → **未验**。
  - **补充判据 TTL（D252）**：AC 明确要求「直连 INSERT 一行 `expires_at = now() - 90秒` → 400；
    同批值只把 `expires_at` 改成 `now() + 30秒` → 200」—— 这条**我在全仓 grep 不到**
    （`interval '90`、`ttl-expired-code`、`authorizationCodeTtlSeconds` 在测试里**全无**）。
    → **TTL 判据完全未验**。「60 秒过期」这个 AC 的核心语义**没有任何测试**。
- **结论：缺陷** —— 一次性/摘要口径通过；**TTL 与「无 consumed_at 列」未验**。

### AC-S-140 scope 三重交集 — ⚠️ **路径 5 与 D241 的三条额外断言全缺**

- 判据落点：`tests/oauth_http.rs:948` `the_scope_is_the_intersection_of_three_sets`
- 我看到的断言（**只有 4 条路径**）：
  - 路径 1：`scope=rbac:role:read` → 200 + `scope == "rbac:role:read"` ✅
  - 路径 2：`rbac:role:read rbac:role:write`（用户无 write）→ 400 `invalid_scope` ✅
  - 路径 3：`sandbox:demo:read`（不在 client 白名单）→ 400 `invalid_scope` ✅
  - 路径 4：`rbac:role:*` → 400 `invalid_scope` ✅
- **路径隔离性核实（你点名要查的）**：夹具用户持 `rbac:role:read` **+** `sandbox:demo:read`（`:956-957`），
  client 白名单 = `["rbac:role:read","rbac:role:write"]`（`:965`）。于是：
  - 路径 2 **只**违反「用户维」（client 有 write、用户没有）→ 隔离用户维 ✅ **成立**
  - 路径 3 **只**违反「client 维」（用户有 sandbox:demo:read、client 无）→ 隔离 client 维 ✅ **成立**
  → **这一段是真的，我核对过两个维度确实各自只动一个变量**。这个设计是对的。
- **缺口 1：路径 5（不回溯，D242）完全缺失。** AC 要求：发令牌 A → 管理员给用户追加 `rbac:role:write`
  + bump `platform_permission_epoch` → 重放 A 内省仍只有 read → 另发 B 含 write。
  我在全仓 grep `permission_epoch` 在 `crates/platform-oauth/` 与 `e2e/` 里 **0 次命中**。
  → **「旧令牌不被回溯扩大」这条判据没有任何测试**。
- **缺口 2：D241 的三条额外断言全缺。** AC 明确要求：
  ① `SELECT count(*) FROM platform_permission WHERE code='rbac:role:*'` == 0；
  ② `GET /api/rbac/me/permissions` 不含 `*`；
  ③ **真正的对照**：把 `rbac:role:*` **登记进 `platform_permission`** → 重跑路径 4 → **仍然** 400。
  我 grep：`rbac:role:*` 在 `crates/platform-oauth/tests/` 里**只出现在 `:993`（路径 4 的请求参数）**，
  **没有** `platform_permission` 的 INSERT、**没有** `me/permissions` 的调用、**没有**「登记后再跑一次」。
  → **缺口 2 最致命**：路径 4 现在只能证明「`rbac:role:*` 没注册/不在白名单 → 400」，
  **证明不了**「通配**一律**不生效」。这正是 AC 原文警告的那个陷阱（「不是因为**它没注册**」），而测试**掉进去了**。
- **结论：缺陷（高）** —— 4 条路径里 3 条成立，但**路径 5 + D241 的三条对照全部缺失**。
  `granted_scopes` 里的 `!scope.contains('*')`（`service.rs:1912`）如果被删掉，**当前测试仍绿**
  （因为白名单里本来就没有含 `*` 的 scope → 路径 4 靠白名单那一维就会 400）。

> **M4 变异复验（我自己做的）**：注入「去掉用户维」→ 路径 2 **确实变红**
> （`oauth_http.rs:983`，实测 `left: 200, right: 400`）。**M4 是真的**。但它只证明路径 2，
> 不覆盖路径 5 / D241。棒1 报告说 M4「证明三重交集成立」——**结论过头了**：它只证明了用户维这一维。

### AC-S-141 Client Credentials 无用户令牌 — ✅ **通过**

- 判据落点：`tests/oauth_http.rs:1001`
- 断言：200 + `scope` / payload **`get("sub").is_none()`**（`:1036`，真值断言不是 null 检查）/
  `client_id` == client / `grant_type` == client_credentials / 内省 `active:true` +
  **`get("username").is_none()`**（`:1053`）/ 超白名单 400 `invalid_scope`。
- **结论：通过**，`skip_serializing_if` 的行为被真正验到了（键不存在 vs 值为 null 区分开了）。

### AC-S-142 Device Code 全流程 — ⚠️ **步骤 3 的数据库侧与补充判据缺失**

- 判据落点：`tests/oauth_http.rs:1075`
- 断言：5 字段齐全 / `user_code` 长度 8 + 字符集 base20 / `verification_uri == "base/oauth/device"` /
  `expires_in == 600` / `interval == 5` / 两摘要口径（交换前查 1 行 + 明文 0 行 ✅ 用了独立 `digest_of`）/
  `authorization_pending` / `slow_down` / verify 200 + clientId + scopes / approve 200 `approved` /
  批准后轮询 200 + scope / 设备码行删除后 0 行。
- **缺口 1（步骤 3 的 DB 侧）**：AC 要求 `poll_interval_seconds` **被更新为 `interval + 5`**。
  我 grep：`poll_interval_seconds` / `last_polled_at` 在 `oauth_http.rs` 里 **0 次**。
  → 「`slow_down` 真的把间隔 +5 写进库」**未验**（只验了返回码 `slow_down`）。
- **缺口 2（步骤 6）**：AC 要求 `last_polled_at` **非空且递增**。同样 **0 次**命中。
- **缺口 3（补充判据）**：AC 要求「用 `user_code` **当成 `device_code`** 去换令牌 → 失败」。
  我在 `oauth_http.rs` 里 grep **没有**这样的调用。→ **未验**（这条证明两个摘要域不串）。
- **缺口 4**：`verification_uri_complete` 在**后端测试里 0 次断言**（实现有：
  `service.rs:1387`；只在 `apps/shell` 的注释里被提到）。AC 要求它**逐字等于**
  `{授权页基址}/oauth/device?user_code=<urlencoded>`。→ **未验**（且同样受 §AC-S-134 的
  「frontend base == issuer」假绿影响）。
- **结论：缺陷（中）** —— 主流程通过，但**4 个判据缺口**（间隔 +5 / last_polled_at / user_code 当 device_code / verification_uri_complete）。

### AC-S-143 内省只内省自己的 — ✅ **通过**

- 判据落点：`tests/oauth_http.rs:1214`
- 断言：自己令牌 → 200 + `active:true` + 8 个字段齐全（`client_id/scope/exp/iat/sub/token_type/iss/aud`）；
  别人的令牌 → **`body == json!({"active": false})`**（`:1264`，**整字段逐字比对**，
  这比「active=false」强得多 —— 它同时证明**没有**多出任何字段）；
  public client / 无认证 / 空 secret → 401 + **逐字** `{"error":"invalid_client"}`（`:1272/1280/1289`）。
- **步骤 3/4（过期 / 已撤销令牌）**：AC 要求这两种也是 200 + 只有 `active`。我**没有**在这个测试里
  看到「过期令牌」与「已撤销令牌」的内省（`:1214-1319` 只测了别人的令牌）。
  不过**撤销那条**在 AC-S-144 的测试里有（`:1354`）。**过期那条确实没有**。
- **结论：通过（主体）** —— 身份隔离与 401 形状都很硬；仅「过期令牌内省」这一子步未单独验。

### AC-S-144 撤销后内省立即失效 — ✅ **通过**

- 判据落点：`tests/oauth_http.rs:1321`
- 5 步全在：撤销 200（RFC 7009）→ 内省 `active:false` → **别的 client 撤销 → 200 但不生效**（`:1380-1391`，
  再内省仍 `active:true` ✅ 这条很有价值）→ 撤销不存在的令牌 → 200。
- **结论：通过**，有区分力。

### AC-S-145 ES256 JWT + 第三方自行验签 — ❌ **判据未验（安全 AC）**

- 判据落点：`tests/oauth_http.rs:1408` `the_access_token_is_an_es256_jwt_with_the_pinned_claims`
- 我看到的断言：`header["alg"] == "ES256"` / `kid` 在 JWKS 里 / payload 的 `iss`/`aud`/9 个 claims 齐全 /
  `scope` / `grant_type`。
- **但是 AC 步骤 3 的核心**是「**测试侧独立实现 ES256 验签**（用 JWKS 里的公钥），**不调用生产代码**」。
  我 grep 全仓：`jsonwebtoken` 在 `crates/platform-oauth/tests/` 里 **0 次**引用；
  没有 `p256` / `ecdsa` / `DecodingKey` 的独立验签代码。
  → **「验签通过」这条断言根本不存在**。
- **步骤 4（过期令牌：验签通过但 exp 过期 → 结论是「已过期」，且必须与「签名无效」分开断言）**：
  同样**完全缺失**（我 grep `expired` 在 `oauth_http.rs` 里只命中 `expires_in` 的数字断言）。
- 后果：把 `keys.rs` 的签名换成**假的**（比如用别的密钥签、或干脆不签）→
  只要 header 里 `alg` 写 `ES256`、claims 齐全，**当前测试全绿**。这是**签名有效性**判据的空缺 ——
  而 AC-S-145 是整个需求「第三方可凭 JWKS 自行验签」的唯一载体。
- **结论：缺陷（阻塞级）** —— 标题说「第三方可凭 JWKS 自行验签」，测试只验了**头字段**与**载荷字段**，
  **没有验一个字节的签名**。

### AC-S-146 同意页 + 同意记录 + 用户所见 = 实际授予 — ⚠️ **步骤 3 的替代判据未实现**

- 判据落点：`tests/oauth_http.rs:1502` + `ticket::tests`（6 条）+ E2E `:150/:193`
- 我看到的断言：
  - confirm → 200；grant 表 `rows.len() == 1` + `status == "active"` + `consent_at.is_some()` +
    `scopes == ["rbac:role:read"]` ✅
  - deny → 200 + `redirectTo.contains("error=access_denied")` + `contains("state=xyz")` +
    grant 表**仍 1 行 + 仍 active** ✅（D202「拒绝不改已有授权」验到了）
  - E2E：同意页 `oauth-scopes` 含 `rbac:role:read`（`:162`）+ 真点「允许」（`:165`）+ 真点「拒绝」（`:200`）
- **缺口（步骤 3，你最关心的一条之一）**：AC 原文要求「用 `target/e2e-jwt-private.pem` **自己签**一个
  `scope` 比 preview 更大的票据 → confirm → **400 `OAUTH_TICKET_INVALID`**」。
  **D260 把它换成了替代判据**：「即使 confirm 接受自签票据，后续用该 code 交换令牌必须 400 `invalid_scope`
  且令牌表无任何行」。
  **我核实：这个替代判据也没有实现。**
  - 我在全仓 grep `ticket::sign` / `sign_claims` / `TicketClaims` / `e2e-jwt-private` 在
    `crates/platform-oauth/tests/` 里 → **0 次**。**没有任何测试自签过票据**。
  - `invalid_scope` 在 `oauth_http.rs` 里只出现在 `:984/:990/:996/:1070`（AC-S-140 路径 2/3/4 与 AC-S-141）——
    **没有一条**是「用自签票据拿到的 code 去交换」。
  - 「令牌表无任何行」也没有对应断言。
- **结论：缺陷（阻塞级）** —— AC-S-146 步骤 3 **既没有原始判据，也没有 D260 的替代判据**。
  BA 报告 §4 写「步骤 3 见 §5.1」，但 §5.1 只**描述**了 D260 的裁定，**没有指向任何测试**。
  我问的那个问题——「D260 换成 invalid_scope 后，这**还是**在验私钥泄露这件事吗？」
  **答案是：不仅变成 AC-S-140 的重复，而且连这个重复都没写**。私钥泄露这条威胁**当前零覆盖**。

### AC-S-147 我的授权可查可撤销 + 令牌立即失效 — ✅ **通过（两枚令牌是真的）**

- 判据落点：`tests/oauth_http.rs:1591` + `:1662` + `:1773` + `:1830`
- 我核实「两枚令牌」是否真的签了两枚（`revoking_a_grant_revokes_every_token_it_issued`，`:1662`）：
  - 第一枚：走授权码流程换取（`:1670-1680`）
  - 第二枚：`prompt=none` 静默续发 → 拿 code2 → 再交换（`:1682-1720`）
  - **`assert_ne!(token1, token2)`**（`:1720`）← **明确证明是两枚不同的令牌，而不是同一枚用两次** ✅
  - 列表恰好 1 条（共用一张 grant）✅
  - 撤销后**两枚都** `active=false`（`:1738-1749` 的 `for token in [&token1,&token2]`）✅
  - 库里 `rows.len() == 2` + 两行都 `revoked`（`:1752-1768`）✅
  → **你点名要查的「是不是真的签发了**两枚**」—— 答案是「是真的」**。
- 另外 `the_grants_endpoints_are_scoped_to_the_current_user`（`:1773`）验了「持 `oauth:grant:revoke`
  也不能撤别人的（404 `OAUTH_GRANT_NOT_FOUND`）」，`another_tenants_user_sees_an_empty_grant_list`（`:1830`）
  验了跨租户 `body == json!([])`（**空数组不是 403**）。
- **结论：通过**。这条是 22 条里做得最扎实的之一（D254/D259 的最难点都真有判据）。

### AC-S-148 免登（静默续发）— ⚠️ **code 表 0 行未验**

- 判据落点：`tests/oauth_http.rs:1921`
- 断言：`prompt=none` + 覆盖 grant → 200 + `needsConsent == false` + **无 `ticket` 键** + `redirectTo` 含 `code=`；
  超出 grant → 200 + `needsConsent == true`（**不是错误**，D233）；缺省 `prompt` → `needsConsent == true` ✅
- **缺口**：AC 步骤 4 要求「且 `platform_oauth_code` 行数 **== 0**」。我 grep 该测试段（`:1920-2000`）
  **没有**任何 `platform_oauth_code` 的计数断言。→ 「`needsConsent=true` 时**没有**偷偷签发授权码」**未验**。
- 前端侧：`error=consent_required` / `error=login_required` 有单测（`oauth-authorize.test.ts:118/138`）✅
- **结论：缺陷（低-中）** —— 主体通过，缺「code 表 0 行」这一条（这条是「不偷偷发码」的关键）。

### AC-S-149 Back-Channel Logout — ⚠️ **payload 判据与失败模式全缺**

- 判据落点：`tests/oauth_http.rs:2002` + E2E `:280`
- 我看到的断言：登出 200 + `loggedOut.grants == 2` + `frontChannel` **是数组**（`as_array().expect(...)`）+ 恰好 1 条 +
  `clientId`/`logoutUrl` + `logoutToken` 有值 + `!body.to_string().contains("client-b")` ✅
  + 两个 client 的 grant 都 `revoked` ✅（D248 的「配置不影响撤销范围」验到了）
- **缺口 1（payload 判据）**：AC 步骤 3 要求**测试侧独立解析** `logout_token` 的 payload 并断言
  `iss` / `aud` / `sub` / `sid` / `jti` / `iat` 齐全 + `events` 含 backchannel-logout 键 + **不含 `nonce`** +
  **`sid` 逐字等于触发登出的登录令牌的 `sid`**（D230）。
  我 grep：`"sid"` / `schemas.openid.net/event` / `nonce` 在 `crates/platform-oauth/tests/` 与 `e2e/` 里
  **全部 0 次命中**。→ **整个 payload 判据没验**。实现是有的（`logout.rs:145-155` 逐字段写了），
  但**没有测试**。少一个 `sid` 或误加 `nonce` → 全绿。
- **缺口 2（步骤 4/5 的失败模式）**：AC 要求 `MOCK_MODE=timeout` / `MOCK_MODE=500` 重启 mock 后再登出 →
  **登出本身仍成功**（200，授权已撤销）。我 grep `MOCK_MODE` 在 `e2e/` 里 **0 次**。
  → 「通知失败绝不能让登出失败」这条**没有测试**（实现有 `tracing::warn!`，`logout.rs:195+`）。
- **缺口 3（阳性对照式验证）**：AC 要求「投递失败时有日志（含 clientId 与失败原因）」。也没有断言。
- **结论：缺陷（高）** —— back-channel 的**核心**（logout_token 的 claims）**零断言**；
  失败不让登出失败这条**零断言**。E2E 只证明了「mock 收到了**某个** token」，没证明 token 内容对。

### AC-S-150 Front-Channel + 按应用登出 — ✅ **通过**

- 判据落点：`tests/oauth_http.rs:2002`（后端 `frontChannel`）+ `apps/shell/test/oauth-logout.test.ts`（前端 iframe）
- 后端：`frontChannel` **恰好 1 条**只含 client-A + `logoutUrl == "https://client.example/front"` + 不含 client-B ✅
- 前端（我读了 `oauth-logout.test.ts:62-134`）：
  - iframe 数量**恰好** == `frontChannel.length`（`:78` 断言 2 条 → `toHaveLength(2)`）✅
  - `src` **逐字** = `logoutUrl?logout_token=jwt`（`:91` `toContain('src="https://client.example/logout?logout_token=jwt-logout-token"')`）✅
  - `sandbox` 含 `allow-scripts` **不含** `allow-same-origin`（`:104`）✅ — 且我核了生产代码
    `OAuthLogout.svelte:121` 写的是 `sandbox="allow-scripts"`（**不含** allow-same-origin）✅
  - 空数组 → 0 iframe（`:117`）✅
- **步骤 3/4（按应用登出只撤 client-A，client-B 仍 active）**：后端测试验了「全局登出两个都撤」；
  「**按应用**登出（`grants/revoke`）只撤一个、另一个仍 active」我没有在 `:2002` 那段里看到。
  不过 `revoking_a_grant_revokes_every_token_it_issued` 与 `a_user_sees_and_revokes_only_their_own_grants`
  覆盖了 revoke 的作用范围。**这个子步覆盖偏弱但主体成立**。
- **结论：通过**（前端这条做得好，sandbox 安全要求真的有断言）。

### AC-S-151 底座业务端点默认拒绝第三方令牌 — ❌ **判据降级：只测了 IAM 函数，没测端点**

- 判据落点：`tests/oauth_http.rs:2052` `a_third_party_oauth_token_is_rejected_by_the_platform`
- **我看到的断言（全文只有两段）**：
  ```rust
  // OAuth 令牌 → 平台会话校验**失败**。
  assert!(fixture.iam.verify_access_token(&oauth_token).is_err(), ...);
  // **阳性对照**：正常的登录令牌必须通过。
  assert!(fixture.iam.verify_access_token(&login).is_ok(), ...);
  ```
- **这是什么？** 这是**直接调 `IamService::verify_access_token()` 这个 Rust 函数**。
  AC-S-151 的步骤 1/2/3 要求的是「用该令牌调 **`GET /api/rbac/me/permissions`** 与
  **`GET /api/audit/events`** → **401 + `code == "UNAUTHENTICATED"` + 不含 `error` 键**」。
  **这两个 HTTP 端点一次都没被调用**（我 grep：`me/permissions` 与 `audit/events` 在
  `crates/platform-oauth/tests/` 与 `e2e/oauth.spec.ts` 里 **0 次**）。
- **而且夹具的 router 里根本没有这两个路由**：`tests/common/mod.rs:393-395` 只 nest 了
  `/api/oauth` + metadata —— 所以**结构上就打不到** `/api/rbac/me/permissions`。
- **步骤 4（用 OAuth 令牌打 `/api/oauth/introspect` + client-AB 认证 → 200 `active == true`）**：
  这个测试里**没有**（该测试没调 introspect）。不过 AC-S-143 里内省自己的令牌是 200/active，
  算**间接**覆盖，但 AC-S-151 步骤 4 的「同一枚令牌业务端点拒、协议端点收」这个**对照**没有。
- **步骤 5（阳性对照：正常登录令牌打**端点**必须成功）**：AC 明确要求「步骤 5 若也 401 → 整条判据 fail」。
  它的实现却是**函数级**的 `verify_access_token(&login).is_ok()` —— 这证明了**签名/aud 校验会过**，
  **没有**证明「`/api/rbac/me/permissions` 这个端点用登录令牌能拿到 200」。
- **D224 的关键判据（响应体 `code == "UNAUTHENTICATED"` 且**不含** `error` 键）**：
  **零断言**。`UNAUTHENTICATED` 在 `oauth_http.rs` 里 0 次（只在 `src/error.rs` 出现）。
- **结论：缺陷（阻塞级）** —— 这条 AC 是「默认拒绝第三方令牌」的安全承诺，而测试**只验证了
  IAM 密钥校验函数会拒**（`aud` 不匹配 → 拒）。这几乎必然为真（OAuth 令牌 `aud` = client_id，
  登录令牌 `aud` = base-api），**但它绕过了整个「业务端点默认拒绝」的链路**：
  - 若有人给 `/api/rbac/me/permissions` 加一个「接受 OAuth 令牌」的分支 → 测试仍绿。
  - 若业务端点的 401 响应体变成 Problem Details 带 `error` 键 → 测试仍绿（D224 违例）。
  - 最要命的是：**「阳性对照真的跑了吗」的答案是「跑的不是对照组该跑的东西」** ——
    它跑的是函数，不是端点。

### AC-S-152 错误码 8 处登记 — ✅ **通过（这条最扎实）**

- 判据落点：`src/error.rs` 7 条单测 + `crates/platform-server/tests/i18n_titles.rs` 9 条
- 我读到的关键断言：
  - `the_title_table_has_the_pinned_number_of_rows`：`TITLES.len() == 58 + 21 + 5 + REQ_022_NEW_CODES`，
    `const REQ_022_NEW_CODES: usize = 9` → **93**（`i18n_titles.rs:170-173`）✅ 且**逐字**写的 93 是公式算出来的，
    我实测 `TITLES.len()` 里的 `OAUTH_` 行 = **9** ✅
  - `for code in platform_oauth::error::ALL_CODES`（`:88`）—— **AC 步骤 4 要求的「包含」那一行真的在** ✅
  - `every_error_code_is_registered_in_every_locale` 逐 locale 跑、缺的列出 ✅
  - `no_orphan_titles`（反向：TITLES 的每个键都要被某 crate 认领）✅ —— 这条防「拼错键」很到位
  - 我实测：`zh-CN.json` / `en-US.json` 各有 **9** 个 `error.OAUTH_*` ✅
- **结论：通过**。「93」是真断言、不是巧合；「包含」不「相等」也遵守了（D194 的警告）。

### AC-S-153 权限点三处齐 + fail-fast + C-5 — ✅ **通过（C-5 是真的）**

- 判据落点：`tests/contract_c5.rs`（2 条）+ `cargo xtask capabilities --check` + `tenant-policies --check`
- `every_oauth_permission_has_an_authorization_call_site`：
  - `check_c5_for(&manifest, &src)` 返回 violations 为空 ✅
  - `scan_auth_calls` 后**逐个** 6 个权限点断言有调用点 ✅
  - `scan.codes.len() == 6`（**恰好** 6 个）✅
  - `non_literal_calls.is_empty()`（参数必须是字面量）✅
  - `custom_entries.is_empty()`（不得自建鉴权入口）✅
- 我实测 `cargo test -p platform-oauth --test contract_c5` → **2 passed**（在 117 里）。
- 我实测 `capabilities/` 下 **14 个文件**、`permissions.json` total **32**、`oauth:` 出现 **6** 次 ✅
- **AC 步骤 3/4（阳性对照：删掉调用点 → C-5 失败；放个 `authorize` 函数 → C-5 失败）**：
  这是**变异式**判据，交付里没有自动化；但 C-5 扫描器本身是**既有平台能力**
  （`platform-contracts`），且 `custom_entries`/`non_literal_calls` 断言让「自建入口」真的会被抓。
  我**没有**亲自跑这两个变异（时间/改动约束），记为**未亲验但机制可信**。
- **结论：通过**（步骤 3/4 的阳性对照我未亲跑，属**残留未验项**）。

### AC-S-154 关键动作产生审计记录 — ❌ **判据降级：用的是内存假 sink，不是真库**

- 判据落点：`tests/oauth_http.rs:2099`
- 我看到的断言：
  - 5 个事件码在 `fixture.sink.events()` 里 ✅
  - 每个 event 的 `tenant_id` / `action` / `object_type` **非空**（`:2215-2219`）
  - 敏感扫描 `scan_sensitive_fields` 对 recorded events **0 命中** + 阳性对照命中 ✅
- **问题 1：`fixture.sink` 是 `RecordingSink`（内存 `Arc<Mutex<Vec<AuditEvent>>>`，
  `tests/common/mod.rs:186`，注释自己写「**不落库** —— 本用例只断言**埋点契约**」）。**
  于是：
  - AC 步骤 2「查**审计表**（`tenant-a`）」→ **没有查库**（我 grep `platform_audit` 在
    oauth 测试里 **0 次**）。→ 「审计**真的落库**」未验。
  - AC 步骤 3「扫描面 = **审计表的全部文本列**」→ 扫的是内存对象的字段，**不是表的列**。
    （`sensitive::scan_sensitive_fields` 本身没问题，问题是**输入源**不是表。）
  - AC 步骤 4「**管理员直连**往审计表插一条含假 secret 的行 → 再扫 → 必须命中；删掉后不命中」
    → **没有**。测试是在**内存数组**里塞了一个字符串（`:2252`）→ 那证明扫描器会命中，
    **不证明**「从表里读出来再扫」这条链路。
- **问题 2：AC 要求的固定字段集**（`actor_id` / `action` / `object_type` / `object_id` / `ip` /
  `user_agent` / `created_at` / `prev_hash` / `hash` —— 契约 C12）：测试**只断了 3 个**
  （`tenant_id` / `action` / `object_type`）。`ip` / `user_agent` / `prev_hash` / `hash` / `created_at` /
  `actor_id` **都没断**。
- **问题 3：AC 要求「哈希链自洽：`GET /api/audit/verify` 返回 `ok = true`」**：
  `audit/verify` / `prev_hash` 在测试里 **0 次**。→ **完全未验**。
- **结论：缺陷（阻塞级）** —— 这条 AC 的整个「**落库 + 哈希链 + 从表里扫**」三层都没验。
  「事件码被 emit 了」验到了，但那只是 AC 的第一句话。

### AC-S-155 OAuth 数据跨租户不可见 — ⚠️ **主判据通过，policy 命名判据缺失**

- 判据落点：`tests/tenant_isolation.rs`
- 我看到的断言（**读侧 4 张表 × 2 租户 + 写侧**）：
  - 步骤 1/2：`counts_in_tenant(app, Some(tenant))` → `vec![1,1,1,1]` 对两个租户 ✅
  - 步骤 3：**超管**看到 2 行（证明前面不是「表本来空」）✅ **这个对照很关键，做了**
  - 步骤 4：不 `set_config` → `vec![0,0,0,0]`（fail closed）✅
  - 步骤 5：单事务 `set_config('tenant-a')` → INSERT `tenant-b` **失败**（`:184`）+
    两个 INSERT **用两个事务**（注释解释了 aborted 事务的坑 —— **这个细节很专业**）✅
  - `platform_oauth_client` 平台级：无 `tenant_id` 列 + `relrowsecurity = false` + 两租户都看得见 ✅
- **我独立实测 DB**：`platform_oauth_grant` 的 policy 是
  `USING (tenant_id = current_setting('app.tenant_id'::text, true))` +
  `WITH CHECK (同上)` ✅ —— 与报告说的一致（我直接查了 `pg_policy`）。
- **缺口（步骤 6）**：AC 要求「`pg_policies` 里这 4 张表**各有 1 条** `<table>_tenant_isolation` policy
  （**命名**也要对）」。测试只查了 `relrowsecurity`，**没有断言 policy 的**名字**与**条数**。
  → 「有人把 policy 改名 / 加第二条宽松 policy」当前测不出。
- **结论：缺陷（低）** —— 读写隔离都很硬（含「超管对照」与「两事务」两个专业细节），
  仅「policy 命名 + 恰好 1 条」未验。

---

## 3. B 变异验证抽查（我抽了 2 条，并核实 M8 的 PostgreSQL 说法）

### 3.1 抽查清单

| 抽哪条 | 为什么抽它 |
|---|---|
| **M4**（scope 去掉用户维） | AC-S-140 的核心，且是最容易被写成「测了但其实没测」的一条 |
| **M8**（RLS `WITH CHECK` 恒真） | 你点名要我核实 §3.1.1 的 PostgreSQL 语义说法 |

### 3.2 M4 复验（我自己注入的）

```bash
cp crates/platform-oauth/src/service.rs /tmp/verif-service.bak
sha256sum crates/platform-oauth/src/service.rs > /tmp/verif-sha.txt
# → c9a1c27ab5085c9a...c26f2c
python3 /tmp/mutation/m4_scope_drop_user.py crates/platform-oauth/src/service.rs
# 改的是 service.rs:1909 —— granted_scopes 的 filter 子句（生产那一行）✅
cargo test -p platform-oauth --test oauth_http -- --test-threads=1 the_scope_is_the_intersection_of_three_sets
```

**结果**：
```
test the_scope_is_the_intersection_of_three_sets ... FAILED
panicked at crates/platform-oauth/tests/oauth_http.rs:983:5:
assertion `left == right` failed: 路径 2 应当 400：{"...token..."}
  left: 200   right: 400
```

- ✅ 改的是**生产那一行**（`service.rs:1909` 的 `&& user_scopes.contains(...)`）
- ✅ **真的变红**，且**红在正确的断言**（路径 2，`oauth_http.rs:983`）
- ✅ 还原：`sha256sum -c` **成功**、`grep -c MUTATION` = **0**、`git status crates/platform-oauth/` 无 service.rs
- ⚠️ **但**：只有**路径 2** 变红（日志显示 `0 passed; 1 failed; 21 filtered out`，
  因为只跑了这一个函数）。M4 **不能**证明路径 5 / D241 也成立 —— 见 §2 的 AC-S-140。

### 3.3 M8 复验 + PostgreSQL 语义核实（你点名要查的）

**先核实「BA 建议的『去掉 `WITH CHECK`』在 PostgreSQL 语义下无效」这个说法。**

我的本地实测（PostgreSQL 17.12，`base_test`）：

```sql
CREATE TABLE rls_probe (tenant_id text, note text);
ALTER TABLE rls_probe ENABLE ROW LEVEL SECURITY;
CREATE POLICY probe_tenant_isolation ON rls_probe
  USING (tenant_id = current_setting('app.tenant_id', true));   -- 只有 USING，没有 WITH CHECK
SET ROLE base_app;
SELECT set_config('app.tenant_id','tenant-a',false);
INSERT INTO rls_probe VALUES ('tenant-a','same');    -- → INSERT 0 1  （成功）
INSERT INTO rls_probe VALUES ('tenant-b','cross');   -- → ERROR: new row violates row-level security policy
```

→ **棒1 §3.1.1 的说法完全正确**：policy 只有 `USING` 时，PostgreSQL 会**把 `USING` 同时当作
INSERT/UPDATE 的检查条件**（PostgreSQL 文档 CREATE POLICY 明文：*"If no WITH CHECK expression is
defined, then the USING expression will be used both to determine which rows are visible and which
new rows will be allowed to be added"*）。**去掉 `WITH CHECK` 不破写侧隔离**。
→ 所以 BA 施工单 §5.4 里「RLS：把 `WITH CHECK` 去掉」这条**变异建议本身就是错的**；
棒1 改用 `WITH CHECK (true)` 是**正确且必要的修正**。**这一点要给棒1 记功。**

**再核实 M8 变异本身。**

```bash
python3 /tmp/mutation/m8_rls_no_with_check.py crates/platform-db/src/rls.rs   # WITH CHECK → (true)
cargo test -p platform-oauth --test tenant_isolation -- --test-threads=1
```

**结果**：
```
test the_oauth_client_table_is_platform_scoped ... ok
test the_oauth_tables_are_isolated_by_tenant ... FAILED
panicked at crates/platform-oauth/tests/tenant_isolation.rs:184:5:
往别的租户插行必须被 RLS 的 WITH CHECK 拒掉
```

- ✅ 改的是**生产那一行**（`crates/platform-db/src/rls.rs:142` 的 policy 生成 SQL）
- ✅ **真的变红**，红在 `tenant_isolation.rs:184`，与棒1 报告**逐字一致**
- ✅ **「只破写侧」也核实了**：`the_oauth_client_table_is_platform_scoped` 仍 **ok**（读侧不受影响）——
  这证明该变异是**精准**的，不是「把整个 RLS 打挂」
- ✅ 这个变异是**真的**：夹具在 `tests/common/mod.rs:329-331` **从源码重新生成 policy**
  （`apply_policies(&admin_db, &scope.spec)`）→ 改源码真的会影响 DB 里的 policy
- ✅ 还原：`sha256sum -c` 成功、`grep -c MUTATION` = 0、`git status crates/platform-db/` **空**
- ✅ **环境副作用已清理**：因为 M8 把 policy 写进了真库，我**重跑**了 `tenant_isolation`
  让夹具用**正确**的 policy 覆盖回去，并直接查库确认：
  `USING (tenant_id = current_setting('app.tenant_id'::text, true))` +
  `WITH CHECK (tenant_id = current_setting('app.tenant_id'::text, true))` ✅

### 3.4 变异验证的可信度结论

- 我抽的 **2/2 条**（M4、M8）都**成立**：改的是生产行、真的变红、还原干净。
- M8 的 PostgreSQL 语义说法**经本地实测确认正确**。
- **MUTATION 残留全仓扫描**：`grep -rn 'MUTATION' crates/ scripts/ e2e/ migrations/` → **无输出** ✅
- **但是**：变异**覆盖的是判据**，不是**AC**。
  M4 证明了「路径 2 不是空跑」，**不能**推出「AC-S-140 的四条路径 + 路径 5 + D241 都成立」。
  棒1 报告 §3 的表格总体可信，但 **§4 的「AC 覆盖 22/22」与 §3 的变异条数没有对应关系** ——
  9 条变异覆盖不到 22 条 AC 的 40 多个子判据。

---

## 4. C 注释与代码一致性（抽查 12 处，发现 1 处不一致）

| # | 位置 | 注释说什么 | 代码在做什么 | 一致？ |
|---|---|---|---|---|
| 1 | `src/sensitive.rs:33-40` | 「静态扫描只扫 **JSON 名**（`clientSecret`）而不是 Rust 名（`client_secret`）」，理由：Rust 名合法存在一次，扫它会恒红 | `scan_response_source` 实现（`:160`）确实只用 `is_json_name(key)`（含 `_` 或大写）+ `contains_whole_key` | ✅ 一致 |
| 2 | `src/sensitive.rs:196-199` | 「`code` / `authorization` 这类**单词**不进静态扫描面」 | `is_json_name` = `key.contains('_') \|\| any(uppercase)` → `code`（无 `_` 无大写）**被排除** ✓ | ✅ 一致 |
| 3 | `tests/common/mod.rs:167-171` | 「`AlwaysFresh` 的 `ensure_fresh` 是 **no-op**；⚠️ **生产实现不是 no-op**」 | `impl PermissionRefresher for AlwaysFresh` 返回 `async { Ok(()) }`；生产侧 `http.rs:362` **真的**调 `state.refresher.ensure_fresh(&ctx).await`，装配层传 `RbacChecker` | ✅ 一致（这条注释**准确且有价值**，解释了 TD-047） |
| 4 | `src/http.rs:225-233` | 「**不能**在 `Router` 上叠多个 `route_layer` —— 是**叠加**语义 → 改成**逐 method**挂层」 | 代码确实是 `.route("/clients", get(list_clients).route_layer(...))` + 另一个 `.route("/clients", post(...).route_layer(...))`（`:243-264`），**每个 method 一个层** ✓ | ✅ 一致，且有测试守着：`the_permission_points_are_per_endpoint_not_stacked`（`oauth_http.rs:1854`）验了「只持 read 也能读、缺 write 就 403」 |
| 5 | `src/service.rs:1864-1868` | 「`preview` 只挡**参数非法**，其余留给令牌端点（否则 AC-S-140 判据永远走不到）」 | `parse_requested_scope` 只判非空 + `is_scope_token` 字符集 ✓；`preview` 里**不**调 `granted_scopes` ✓ | ✅ 一致 |
| 6 | `src/service.rs:951-955` | 「同意记录里存的是**实际能授予**的 scope（D202）」 | `confirm` 里 `granted = self.granted_scopes(...)` 后 `upsert_grant(..., &granted, ...)` ✓ | ✅ 一致 |
| 7 | **`src/service.rs:160-161`** | **`/// **实际授予**的 scope（数组）。`** （`PreviewResult.scopes` 的文档） | **`preview` 里两处都写 `scopes: requested`（`:903`、`:929`）—— 是**请求的** scope，**交集前**的值，不是「实际授予」** | ❌ **不一致** |
| 8 | `src/service.rs:1957-1962` | 「PKCE 挑战**原样落库**」；`confirm` 传**票据**里的值（用户所见 = 实际授予） | `NewCode.code_challenge: code_challenge.to_string()` ✓ | ✅ 一致 |
| 9 | `src/http.rs:200-210` | 协议端点租户「一律是 OAuth 保留伪租户，服务层会**覆盖**」；`/introspect` `/revoke` 用 JWT 的 `tid` | `protocol_ctx(ctx)` 返回 `with_tenant(OAUTH_PLATFORM_TENANT)` ✓；服务层确有覆盖分支 | ✅ 一致 |
| 10 | `src/logout.rs:143-155` | 「`sid` = **触发登出的这次请求**的会话 ID（D230）」；「**不含 `nonce`**（D211）」 | `LogoutTokenClaims { sid: user.session_id.clone(), ... }`，结构体**无** `nonce` 字段 ✓ | ✅ 一致（**但见 §2 AC-S-149：没有测试验它**） |
| 11 | `apps/shell/src/pages/OAuthDevice.svelte:90-107` | 「**必须把 URL 里的 `user_code` 同步进 `userCode` 状态**，否则空串 → handleDecision 直接 return」（F1 修复的注释） | `initialOutcome()` 里 `const fromQuery = readUserCode(env.search) ?? ''; ... userCode = fromQuery;`（`:100-107`）✓ | ✅ 一致（**注释准确描述了那个真 bug 的成因**） |
| 12 | `src/crypto.rs:3-12` | 「摘要口径 `digest_base64url`（**不是** hex）；仓里另有 `digest_hex`，本需求**不用**」 | `digest_base64url` 用 `URL_SAFE_NO_PAD`；`the_digest_is_not_hex` 断言 `!all(is_ascii_hexdigit)` ✓ | ✅ 一致 |

### 4.1 发现的不一致（1 处，必须打回）

**`crates/platform-oauth/src/service.rs:160`**：

```rust
/// **实际授予**的 scope（数组）。
pub scopes: Vec<String>,
```

但 `:903` 与 `:929` 都写 `scopes: requested` —— `requested` 是 `parse_requested_scope` 之后、
**三重交集之前**的**请求值**。`granted_scopes()`（`service.rs:1907`）算出的交集**没有**回填到 `PreviewResult.scopes`。

**为什么这是真问题（不只是注释笔误）**：

- 前端同意页（`OAuthAuthorize.svelte:190` `{#each outcome.preview.scopes as scope}`）**直接渲染这个字段**。
  → 用户可能看到「请求了 `rbac:role:write`」并点「允许」，但 `confirm` 写进 grant 的 `scopes`
  是**交集**（`granted`）—— **用户看到的 ≠ 实际授予的**。
- 这**正好是 AC-S-146 的标题**「**用户所见 = 实际授予**」。
  AC 步骤 1 只要求「页面文本里逐字出现 `rbac:role:read`」，E2E 也只断了这一个码 ——
  **没有测「请求了用户没有的 scope 时，同意页显示什么」**。
- 结合 D260（`confirm` 不校验 scope），这条链路的实际后果是：用户看到 `read+write`、
  点允许、grant 里只写 `read`、令牌端点再拒 `write`。**不越权，但「所见非所得」**。
- 注释声称「实际授予」，代码给的是「请求的」→ **注释比代码旧，按 dev-guide 是必须打回的项**。

**修法建议（给 BA/Dev，不是我来改）**：要么把 `scopes` 改成交集结果（`granted`），
要么把注释改成「请求的 scope」并在 AC 里明确「同意页展示请求值」。**两者要选一个并加测试**。

---

## 5. D 跨栈契约对齐（D258 逐字段核对）

### 5.1 核对表

| 端点 | 后端（`http.rs` / `service.rs`） | 前端（`oauth-api.ts` / `apps/shell`） | 一致？ |
|---|---|---|---|
| `POST /api/oauth/authorize/preview` 请求 | `PreviewBody`（`http.rs:423`）`#[serde(rename_all="camelCase")]` → `responseType`/`clientId`/`redirectUri`/`scope`/`state`/`codeChallenge`/`codeChallengeMethod`/`prompt`，**全部 `Option` + `#[serde(default)]`** | `OAuthAuthorizeParams` 用 `OAUTH_AUTHORIZE_PARAM_NAMES` **把 camelCase 映射成 RFC snake_case** 发 query（`oauth-api.ts:328-333`），但 `preview` 走 JSON body：`clientId`/`redirectUri`/`codeChallenge`/`codeChallengeMethod`/`responseType`/`scope`/`state`/`prompt`（`:357-362`） | ✅ 一致（**且前端做了正确的两套映射：授权页 URL 用 snake_case，平台端点用 camelCase** —— 这正是 D258 的要求） |
| preview 响应 | `PreviewResult`（`service.rs:152`）`needsConsent`/`ticket?`/`application{clientId,name}`/`scopes[]`/`redirectTo?`（`skip_serializing_if`） | `parsePreviewResponse`（`oauth-api.ts:474-489`）读 `needsConsent`/`ticket`/`application.clientId`/`application.name`/`scopes`/`redirectTo` | ✅ 逐字段一致 |
| `GET /api/oauth/device/verify` 查询参数 | `DeviceVerifyQuery`（`http.rs:702`）`#[serde(rename_all="camelCase")]` → **`userCode`** | `oauth-api.ts:727` `query: { userCode }` | ✅ 一致（**是 `userCode` 不是 `user_code`** ✓） |
| `POST /api/oauth/device/approve` | `http.rs:726` `{userCode, action}` → `DeviceApproveResult{status}` | `oauth-api.ts:738` `client.post(path, { userCode, action })` | ✅ 一致 |
| `POST /api/oauth/logout` | `LogoutResult`（`service.rs:226`）`loggedOut{grants,sessions}` + **`front_channel: Vec<...>`**（**永不为 null**） | `oauth-api.ts` 读 `frontChannel` 数组 | ✅ 一致（后端测试 `:2017` 用 `as_array().expect("frontChannel 必须是数组")` **真的断言了不是 null**） |
| `GET /api/oauth/grants` | `GrantView`（`service.rs:178`）`clientId`/`clientName?`/`scopes[]`/`grantedAt`/`status`/`revokedAt?` | `oauth-api.ts:495-514` 读 `clientId`/`scopes`/... | ✅ 一致 |
| `POST /api/oauth/grants/revoke` | `RevokeGrantBody` → `RevokeGrantResult{status}` | `oauth-api.ts` 发 `{clientId}` | ✅ 一致 |
| 协议端点 | `token`/`introspect`/`revoke`/`device_authorization` 用 **form 编码 + snake_case**（`call_form` 测试证实：`grant_type`/`code_verifier`/`device_code`/`redirect_uri`） | `oauth.spec.ts:176-186` 用 `form: { grant_type, code, redirect_uri, code_verifier }` | ✅ 一致 |
| **两套不混** | 平台端点全部 `#[serde(rename_all="camelCase")]`（7 处：`http.rs:420/474/521/554/659/702/726`）；协议端点走 form/`ProtocolError` 不走 serde | 前端 `OAUTH_AUTHORIZE_PARAM_NAMES` **专门**把 camelCase 翻成 snake_case 用于**授权页 URL**（不属于后端契约，是 RFC 6749 的 query） | ✅ **两套没有混** |

### 5.2 结论

**D258 的跨栈契约对齐是成立的**，且是整个交付里**做得最好的一环**：
- 后端 7 个 DTO 全部显式 `rename_all = "camelCase"`；
- 前端明确区分「授权页 URL 用 RFC snake_case」与「平台端点用 camelCase」；
- `frontChannel` 空数组不是 null 这条**有断言守着**。

**唯一瑕疵**：`GET /api/oauth/device/verify` 的参数名在后端是 `userCode`，
而**设备码页的路由 query** 是 `user_code`（`oauth-api.ts:97` `OAUTH_USER_CODE_PARAM = 'user_code'`）——
这两个**不同是对的**（前者是平台 API，后者是 `verification_uri_complete` 的 RFC 形状），
但**同一个文件里两个名字**容易误读。这是**可读性**问题，不是缺陷。

---

## 6. E 22 条 AC 独立判定 + 与 BA 对账

### 6.1 我的独立判定（不看 BA 结论）

| AC | 我的判定 | 关键理由（证据见 §2） |
|---|---|---|
| **134** | ⚠️ **判不了（部分）** | 字段都很硬，但 `authorization_endpoint` 用 `{授权页基址}` 这条**没有区分力**（夹具 frontendBaseUrl 空 → 回退 issuer；无任何测试设成不同值） |
| **135** | ❌ **不通过** | 「回调白名单生效」**零测试**（`OAUTH_INVALID_REDIRECT_URI` 后端 0 次） |
| **136** | ⚠️ **判不了（部分）** | 回显/摘要口径/扫描阳性对照成立；但**步骤 1**（DB 列比对）与**步骤 4 的两个输出面**（token 响应、审计表）未验 |
| **137** | ✅ **通过** | 后端 + E2E + 前端逐参数断言，D235/D221 都真的做了 |
| **138** | ✅ **通过** | 三路径 + CJK/明文逐子串断言 |
| **139** | ⚠️ **判不了（部分）** | 一次性/并发/摘要口径成立；**TTL（D252）与「无 consumed_at 列」零测试** |
| **140** | ❌ **不通过** | 路径 1~4 成立且隔离性真的做对了；但**路径 5（不回溯）与 D241 三条对照全缺** → 「通配一律不生效」证明不了 |
| **141** | ✅ **通过** | `sub`/`username` **键不存在**的真值断言 |
| **142** | ⚠️ **判不了（部分）** | 主流程成立；`poll_interval_seconds` +5 / `last_polled_at` / user_code 当 device_code / `verification_uri_complete` **四项未验** |
| **143** | ✅ **通过** | 「别人的令牌 → 整字段逐字 `{"active":false}`」很有力；仅「过期令牌内省」未单独验 |
| **144** | ✅ **通过** | 含「别人撤不掉」这条高价值判据 |
| **145** | ❌ **不通过** | **「验签通过」这条断言根本不存在**（测试里 0 处 ES256 验签）；过期令牌判定缺失 |
| **146** | ❌ **不通过** | 同意记录/拒绝不改均成立；但**步骤 3 既无原判据、也无 D260 替代判据** → 私钥泄露零覆盖 |
| **147** | ✅ **通过** | **两枚令牌是真的**（`assert_ne!(token1,token2)` + 两行 revoked）；跨租户空数组 + 数据范围都有 |
| **148** | ⚠️ **判不了（部分）** | 三档 prompt 判定成立；**「code 表 0 行」未验** |
| **149** | ❌ **不通过** | **logout_token 的 payload（sid/events/无 nonce）零断言**；timeout/500 失败模式零测试 |
| **150** | ✅ **通过** | 后端 `frontChannel` 恰好 1 条 + 前端 iframe 数量/src 逐字/sandbox 属性（含 **不含 allow-same-origin**）都真有断言 |
| **151** | ❌ **不通过** | 只调了 `IamService::verify_access_token()` **函数**，**两个业务端点一次都没打**；`UNAUTHENTICATED` 响应体零断言 |
| **152** | ✅ **通过** | 93 是真公式、`ALL_CODES` 那行真的在、逐 locale 覆盖、孤儿反查 |
| **153** | ✅ **通过** | C-5 真的跑（2 passed）、6 个权限点恰好、capabilities 14/32 实测一致；步骤 3/4 阳性对照未亲跑 |
| **154** | ❌ **不通过** | 用的是**内存假 sink**（不落库）；审计表 / 哈希链 `ok=true` / 从表里扫 **三层全缺**；固定字段只断 3/9 |
| **155** | ⚠️ **判不了（部分）** | 读写隔离很硬（含超管对照 + 两事务）；**policy 命名 + 恰好 1 条未验** |

### 6.2 汇总

| 判定 | 条数 | AC |
|---|---|---|
| ✅ **通过** | **9** | 137 / 138 / 141 / 143 / 144 / 147 / 150 / 152 / 153 |
| ⚠️ **判不了（部分）** | **6** | 134 / 136 / 139 / 142 / 148 / 155 |
| ❌ **不通过** | **7** | 135 / 140 / 145 / 146 / 149 / 151 / 154 |

> 合计 9 + 6 + 7 = **22** ✅

### 6.3 与 BA 判定对账（**这是本报告最重要的表**）

BA 报告 §4 判 **22/22 全部通过**。我判 **9 通过 / 6 判不了 / 7 不通过**。
**不一致 13 条**：

| AC | BA 判 | 我判 | 分歧的**确切原因** |
|---|---|---|---|
| **134** | ✅ | ⚠️ | BA 说「字段逐字」——对，但 `authorization_endpoint` 的 `{授权页基址}` 维度**判不了**（夹具的 base == issuer）。BA 报告 §3.3 还引用 D222 说「E2E 里二者不相等」——**但 E2E 根本没断 `authorization_endpoint`**（`oauth.spec.ts:323-338` 只断 response_types / grant_types / code_challenge_methods）。 |
| **135** | ✅ | ❌ | BA 把「回调白名单」写进了标题认可，但**没有任何后端点断言** `OAUTH_INVALID_REDIRECT_URI`。BA 报告 §4 的第 135 行只列了 201/403/401/404 —— **它自己也漏了白名单**。 |
| **136** | ✅ | ⚠️ | BA 说「`sensitive` 扫描带阳性对照」——**对**；但 AC 步骤 1（`client_secret_hash` 列比对）与步骤 4 的 token/审计两个面，BA **没有证据**（`client_secret_hash` 在 `oauth_http.rs` 里 0 次）。 |
| **139** | ✅ | ⚠️ | BA 的判据落点写的是「一次性 + 库里无明文 + 摘要口径」+ 并发 —— **全是「一次性」那一半**。**AC 标题里的「60 秒过期」BA 一个证据都没给**，`expires_at` 的 TTL 对照 INSERT 零存在。这是 BA 报告**最明显的漏判**（标题的一半没验却说通过）。 |
| **140** | ✅ | ❌ | BA 说「三条路径**分别**隔离用户维/client 维/交集」—— 前两条**对**；但 AC 写的是**五**条路径，**路径 5（不回溯）BA 完全没提**，D241 的三条额外断言（含「把通配登记进 platform_permission 再试」这个**真正的对照**）也**完全没提**。 |
| **142** | ✅ | ⚠️ | BA 说「含 authorization_pending / slow_down +5」——**「+5」是错的**：它验到了 `slow_down` 返回码，**没有验** `poll_interval_seconds` 被更新为 `interval+5`（DB 侧 0 断言）。另外 `verification_uri_complete` / `last_polled_at` / user_code 当 device_code 三项 BA 也没提。 |
| **145** | ✅ | ❌ | BA 说「`alg` ES256、`kid` 在 JWKS 内、claims 齐全」——**全对，但全是**结构**断言**。AC 步骤 3 要求「**测试侧独立实现 ES256 验签**」，BA 把它当成「有 jsonwebtoken 依赖」就算过了。**签名一个字节都没验。** 这是**最危险的假绿**。 |
| **146** | ✅ | ❌ | BA 报告 §4 写「步骤 3 见 §5.1」。我读了 §5.1：**它只描述 D260 的裁定理由，没有指向任何测试**。我 grep 确认：**没有测试自签过票据**，`invalid_scope` 也没有在「自签 ticket → code → 交换」这条链上出现。**BA 用一段「决策说明」替代了「判据」。** |
| **148** | ✅ | ⚠️ | BA 的落点描述里有「不覆盖 → 200 + needsConsent=true 且 **code 表 0 行**」——**这句话在报告里，但测试里没有**。我 grep 该测试函数段（`:1920-2000`）无 `platform_oauth_code`。BA **把自己想要的行为写进了报告，却不是测试事实**。 |
| **149** | ✅ | ❌ | BA 说 E2E 验了「`logout_token` payload `iss`/`aud`/`sub`/`sid`/`jti`/`iat` 齐全、`events` 含 backchannel 键、**无 `nonce`**」——**这是凭空写的**。E2E 只做了 `expect(src).toContain('...?logout_token=')` 与「mock 文件行数 > 0」。**payload 从未被解析**（`"sid"` 在 e2e 里 0 次）。BA 报告 §3.3 的表格也写「mock 落盘文件有记录」——那是对的，但「记录内容对不对」没验。 |
| **151** | ✅ | ❌ | BA 说「401 `UNAUTHENTICATED` + 不含 `error` 键 + 阳性对照」——**测试里没有 401、没有 UNAUTHENTICATED、没有端点**。它只有两行 `verify_access_token(...).is_err()/is_ok()`。BA 报告 §4 第 151 行**描述了 AC 的理想结果，而不是测试实际断言**。这是最严重的「报告与代码脱节」。 |
| **154** | ✅ | ❌ | BA 说「5 个事件码 + 固定字段集 + 敏感扫描带阳性对照」——事件码对、扫描对；但「固定字段集」测试只断 3 个（BA 说「契约 C12 的固定字段集」，暗示 9 个全断）。**审计表 / 哈希链 `ok=true` BA 一个字没提**，而 AC 明确要求。且 sink 是 `RecordingSink`（不落库）——BA 报告也没说。 |
| **155** | ✅ | ⚠️ | BA 在 §2.4 说「我直接查 `pg_policies`」——**那是对的环境核查，不是测试断言**。AC 步骤 6 要求测试里断言 policy **名字**与**恰好 1 条**；测试只查 `relrowsecurity`。BA 把「我手工查过」当成了「判据成立」。 |

### 6.4 一致的部分（9 条）

137 / 138 / 141 / 143 / 144 / 147 / 150 / 152 / 153 —— 这 9 条我和 BA 都判通过，
且我**逐条打开源码确认过断言有区分力**（尤其 147 的两枚令牌、150 的 sandbox 属性、152 的 93 是真公式）。

---

## 7. F 越界与回归

### 7.1 改动集

| 检查 | 结果 |
|---|---|
| `git status --porcelain \| wc -l` | **42** ✅ 与 BA 一致 |
| 明细是否只有预期的 42 项 | ✅ 我逐个看过：新增 1 crate + 6 迁移 + 1 mock 脚本 + 1 E2E + 2 报告 + 11 前端 + 20 已跟踪修改 |
| **`apps/shell/src/lib/router.ts` 没被改吗？（D221 明令禁止）** | ✅ **`git status` 对它是空的** —— 未被改 |
| **`packages/i18n/**` 只有棒2 的改动吗？** | ✅ 3 个文件（zh-CN / en-US / i18n.test.ts），diff 内容是 9 个 `error.OAUTH_*` + OAuth 导航词 + 测试，**无后端痕迹** |
| 有无 `crates/platform-db/` 残留（M8 注入点） | ✅ `git status crates/platform-db/` **空** |

### 7.2 回归（REQ-011/013/014 既有测试）

我**自己**跑了 3 个 crate（BA 说 1080 passed，我核实其中 3 个的数字）：

| crate | 我的实测 | BA 报告 | 一致 |
|---|---|---|---|
| platform-iam | **180 passed / 0 failed / 0 ignored** | 180 | ✅ |
| platform-core | **153 passed / 0 failed / 0 ignored** | 153 | ✅ |
| platform-server | **129 passed / 0 failed / 0 ignored** | 129 | ✅ |
| **小计（我跑的 3 个）** | **462** | 462 | ✅ |

→ **回归是真的**，既有测试全过。BA 的 1080 我用 `/tmp/req022-full.log` 复算过：
`grep -E '^test result' | awk '{p+=$4;f+=$6;i+=$8}'` → **passed=1080 failed=0 ignored=0**，
**66 条 `test result` 行** —— **这个数字不是抄的，是从原始 log 聚合出来的** ✅

### 7.3 一处需要说明的越界（BA 已披露，我核实合理）

- `crates/platform-server/Cargo.toml`（+2 行依赖）+ `AGENTS.md`（3 行 `audit_sink` 迁移）——
  BA 报告 §1 与棒1 报告 §9 都**主动披露**了，理由是「装配必然连带」与「`xtask capabilities`
  重新生成的必然连带」。我核实 `crates/xtask/src/catalog.rs` 的 `agents_doc_matches_catalog`
  测试**确实**强制 `AGENTS.md` 与能力目录一致 → **理由成立，不算偷偷越界**。

---

## 8. G 三条硬规矩

| 规矩 | 核实 |
|---|---|
| **全量只跑一次（代码冻结后）** | ⚠️ **无法完全证实**。我看到的证据：`/tmp/req022-full.log`（02:41，14950 字节，66 条 `test result`）；`target/e2e-*.pem` 等是 03:01。BA 报告说执行时间 02:36~03:05。**时间线自洽**，但「只跑一次」这件事本身不可从产物证明（我不做取证式的时间戳审计）。**判：未发现违反，但未能证明**。 |
| **数字自己算的**（不是抄 Dev） | ✅ **成立**。我用同一命令从同一 log 复算出 `passed=1080`，与 BA 一致；我另跑 3 个 crate 与 1 个前端包，**逐 crate 数字全对**。BA 报告 §2.1 的聚合命令是可复现的。 |
| **22 条 AC 逐条判定（不是「全绿就通过」**） | ❌ **形式上做了，实质上不合格**。BA 报告 §4 确实有 22 行表格，**但其中 13 行的「判据落点」描述与实际测试断言不符**（见 §6.3）。尤其 **145 / 149 / 151 / 154** 四行描述的能力**在测试里不存在**。这不是「逐条判定」，是「按 AC 的预期结果回填了一张表」。 |

---

## 9. 发现的问题（按严重程度分级，每条带 `file:line`）

### 🔴 阻塞（7 条）

**B-01｜AC-S-151 的判据降级为「调一个 Rust 函数」，业务端点一次都没打**

- 证据：`crates/platform-oauth/tests/oauth_http.rs:2052-2096` 全文只有
  `fixture.iam.verify_access_token(&oauth_token).is_err()` 与 `...(&login).is_ok()`。
- 夹具 router **结构上就不含** `/api/rbac/*` 与 `/api/audit/*`
  （`crates/platform-oauth/tests/common/mod.rs:393-395` 只 nest `/api/oauth` + metadata）。
- `UNAUTHENTICATED` 在 `oauth_http.rs` 里 **0 次**；`me/permissions`、`audit/events` 在
  `crates/platform-oauth/tests/` 与 `e2e/oauth.spec.ts` 里 **0 次**。
- **影响**：AC 承诺的「底座业务端点默认拒绝第三方令牌」**没有任何端到端证据**。
  只要 `verify_access_token` 的 `aud` 校验还在（它必然在，因为 `aud` 不同），这条测试**恒绿**，
  而业务端点是否真的加了 OAuth 分支、401 响应体是否真的符合 D224 —— **全都没验**。
- **修法**：在 `platform-server` 的集成测试里（真装配）用 OAuth 令牌打
  `GET /api/rbac/me/permissions` 与 `GET /api/audit/events`，断 401 + `code == "UNAUTHENTICATED"`
  + **无 `error` 键**；再加登录令牌的**端点级**阳性对照。

**B-02｜AC-S-145 的「ES256 验签」一个字节都没验**

- 证据：`crates/platform-oauth/tests/oauth_http.rs:1408-1499` 只断言 `header["alg"]=="ES256"`、
  `kid` 在 JWKS、payload 的 claims 齐全。**没有**任何独立验签代码
  （`jsonwebtoken` / `p256` / `DecodingKey` 在 `tests/` 下 0 次引用）。
- AC 步骤 3/4 要求的「独立实现 ES256 验签」+「过期令牌：验签**通过**但 exp 过期」**均不存在**。
- **影响**：把 `keys.rs` 的签名换成任意值（或换密钥），本测试仍绿。
  「第三方可凭 JWKS 自行验签」这个 AC 的**唯一实质承诺**未验。

**B-03｜AC-S-146 步骤 3：D260 的替代判据也没实现 —— 私钥泄露零覆盖**

- 证据：全仓 grep `ticket::sign` / `sign_claims` / `TicketClaims` / `e2e-jwt-private` 在
  `crates/platform-oauth/tests/` 下 **0 次**。**没有任何测试自签过票据**。
- D260 声称的替代判据（「用该 code 交换必须 400 `invalid_scope` + 令牌表无行」）在
  `oauth_http.rs` 里找不到对应调用；`invalid_scope` 的 4 处出现（`:984/:990/:996/:1070`）
  全部属于 AC-S-140/141。
- BA 报告 §4 AC-146 行写「步骤 3 见 §5.1」，而 §5.1 **只是一段决策说明，不含任何测试引用**。
- **影响**：AC 原文「这一条才真正证明 D204 的承诺（签名有效、scope 更大 → 仍被拒）」**未验**；
  换成的替代判据「私钥泄露也拿不到越权令牌」**也未验**。这条威胁**当前零覆盖**，
  而 BA 报告却把它标为「通过」。

**B-04｜AC-S-140 的「路径 5 不回溯」+ D241 三条对照全部缺失**

- 证据：`crates/platform-oauth/tests/oauth_http.rs:948-997` 只有 4 条路径；
  `permission_epoch` 在 `crates/platform-oauth/` 与 `e2e/` 下 **0 次**；
  `platform_permission` 的 INSERT / `me/permissions` 调用 / 「登记通配后再跑」**均不存在**。
- **影响 1**：D242「旧令牌不被回溯扩大」零测试。
- **影响 2（更严重）**：路径 4（`rbac:role:*` → 400）**证明不了「通配一律不生效」**——
  因为 client 白名单里本来就没有含 `*` 的 scope，靠白名单那一维就会 400。
  AC 原文**明确警告**了这个陷阱（「不是因为「它没注册」」），而测试**正好掉进去了**。
  删掉 `service.rs:1912` 的 `!scope.contains('*')`，**当前测试仍绿**。

**B-05｜AC-S-149 的 `logout_token` payload 零断言**

- 证据：`"sid"` / `schemas.openid.net/event` / `nonce` 在 `crates/platform-oauth/tests/` 与
  `e2e/` 里 **0 次**。后端测试只断 `front[0]["logoutToken"].as_str().is_some()`
  （`oauth_http.rs:2033`）；E2E 只断 `src` 含 `?logout_token=` 与 mock 文件行数 > 0。
- AC 步骤 3 要求独立解析 payload 并断 `iss`/`aud`/`sub`/`sid`/`jti`/`iat` + `events` 键 +
  **无 `nonce`** + **`sid` 逐字等于登录令牌的 `sid`**（D230）—— **全部未验**。
- 实现是对的（`logout.rs:145-155`），但**没有测试守着**。
- 另：`MOCK_MODE=timeout` / `500` 的失败模式（「通知失败绝不能让登出失败」）**零测试**。

**B-06｜AC-S-154 用内存假 sink，审计表 / 哈希链全未验**

- 证据：`crates/platform-oauth/tests/common/mod.rs:184-195` `RecordingSink` 注释自承
  「**不落库** —— 本用例只断言**埋点契约**」。`platform_audit` 在 oauth 测试里 **0 次**；
  `audit/verify` / `prev_hash` **0 次**。
- AC 步骤 2（查审计表）/ 步骤 3（扫**表的全部文本列**）/ 步骤 4（**管理员直连**插假 secret 行 → 命中）
  / 「哈希链 `ok = true`」—— **四层全缺**。
- 固定字段集只断 3/9（`tenant_id`/`action`/`object_type`），`actor_id`/`ip`/`user_agent`/
  `created_at`/`prev_hash`/`hash` 全部未断。

**B-07｜AC-S-135 的「回调白名单生效」零测试（开放重定向）**

- 证据：生产代码有校验（`crates/platform-oauth/src/service.rs:834`
  `if !client.redirect_uris.iter().any(|uri| uri == redirect_uri)`）；
  `OAUTH_INVALID_REDIRECT_URI` 在**后端测试里 0 次**（只在前端渲染测试里出现）。
- **影响**：把该判断改成恒放行 → 所有测试绿。AC 标题的一半（「**且回调白名单生效**」）未验。

### 🟠 高（4 条）

**H-01｜AC-S-134/142 的 `{授权页基址}` 维度无区分力（D222 的核心禁令判不了）**

- 证据：夹具用 `OAuthSettings::default()`（`tests/common/mod.rs:379`），
  `frontend_base_url` 为**空串**（`src/config.rs:78`）→ `frontend_base()` 回退到 issuer `"base"`。
  于是 `authorization_endpoint == "base/oauth/authorize"`（`oauth_http.rs:327`）
  在「误用 `{issuer}`」时**也成立**。
- 全仓**没有**任何测试把 `frontend_base_url` 设成 ≠ issuer 再断端点
  （`frontend_base_url` 只在 `config.rs` 的单元测试里被设成 `https://e2e.base`，
  那里测的是 `frontend_base()` 函数本身，不是 HTTP 端点）。
- E2E（`e2e/oauth.spec.ts:323-338`）**也不断 `authorization_endpoint`**。
- 同源问题影响 `verification_uri`（`oauth_http.rs:1101` 断 `"base/oauth/device"`）
  与 `verification_uri_complete`（**根本没断**）。

**H-02｜AC-S-139 的「60 秒 TTL」零测试**

- 证据：AC 的「补充判据（TTL，D252）」要求直连 INSERT 两行（`expires_at` 只差 90s 过期 vs 30s 有效）
  → 一个 400 一个 200。全仓 grep `interval '90`、`ttl-expired-code`、
  `authorizationCodeTtlSeconds` 在测试里 **0 次**。
- 另：步骤 5「`information_schema.columns` 里无 `consumed_at`/`used_at`」也未验
  （`information_schema` 在 `oauth_http.rs` 里 0 次）。
- **影响**：AC-S-139 标题的一半（「**60 秒过期**」）没有判据。

**H-03｜AC-S-142 的四个子判据缺失**

- 证据（均 0 次命中）：`poll_interval_seconds`（`slow_down` 后要变成 `interval+5`）、
  `last_polled_at`（非空且递增）、用 `user_code` 当 `device_code` 换令牌（必须失败）、
  `verification_uri_complete` 的逐字断言。
- BA 报告 §4 第 142 行写「含 `authorization_pending` / `slow_down` **+5**」——
  **「+5」是错的**：测试只断了返回码 `slow_down`，没断 DB 里的 `poll_interval_seconds`。

**H-04｜注释与代码不一致：`PreviewResult.scopes` 文档说「实际授予」，代码给「请求值」**

- 证据：`crates/platform-oauth/src/service.rs:160` 文档 `/// **实际授予**的 scope（数组）。`；
  但 `:903` 与 `:929` 都是 `scopes: requested`（三重交集**之前**的值）。
- 前端 `apps/shell/src/pages/OAuthAuthorize.svelte:190` **直接渲染**这个字段 →
  用户可能看到「请求了 `rbac:role:write`」而 grant 里只写了交集（`read`）。
- 这**正是 AC-S-146 的标题**「用户所见 = 实际授予」，而 AC 步骤 1 与 E2E 只断了
  「`rbac:role:read` 逐字出现」（那是用户**有**的权限，看不出差异）。
- 按 dev-guide「注释与代码必须一致」，这是**必须打回**的项。

### 🟡 中（3 条）

**M-01｜AC-S-136 步骤 1（`client_secret_hash` 列比对）与步骤 4 的两个输出面未验**

- 证据：`client_secret_hash` 在 `crates/platform-oauth/tests/oauth_http.rs` **0 次**；
  测试侧独立实现 `digest_of()`（`:69`）**只用于授权码**，没用于比对 client secret 列。
- 步骤 4 要求的「`POST /api/oauth/token` 响应体」「`GET /api/audit/events` 全表文本」不含明文
  —— 两处均无断言。

**M-02｜AC-S-148 的「`platform_oauth_code` 行数 == 0」未验**

- 证据：`oauth_http.rs:1920-2000` 无 `platform_oauth_code` 计数断言。
- **影响**：`needsConsent=true` 时若偷偷签发授权码，测不出。
  （BA 报告 §4 第 148 行**写了**这条判据，但测试里没有 —— 报告与代码脱节。）

**M-03｜AC-S-155 步骤 6 的 policy 命名 + 「恰好 1 条」未验**

- 证据：`crates/platform-oauth/tests/tenant_isolation.rs` 只查 `relrowsecurity`（`:236`），
  **没有**查 `pg_policies` 的 `polname` 或 policy 数。
- **影响**：policy 被改名 / 多加一条宽松 policy 测不出。
  （我手工查库确认**当前** 4 张表各 1 条且命名正确 —— 但那是环境事实，不是判据。）

### 🟢 低（2 条）

**L-01｜AC-S-143 的「过期令牌内省」子步未单独验**

- 该测试（`:1214-1319`）覆盖了「别人的令牌」，但「过期令牌 → 200 + 只有 active」没有单独断言。
  撤销那条在 AC-S-144 里覆盖了。

**L-02｜`oauth-api.ts` 同一文件里 `userCode`（平台 API）与 `user_code`（路由 query）并存**

- `:97` `OAUTH_USER_CODE_PARAM = 'user_code'`（RFC 8628 的 `verification_uri_complete`）与
  `:727` `query: { userCode }`（平台 API）。**两者不同是对的**，但同名异形容易误读。
  纯可读性问题。

---

## 10. 总结论

# **FAIL**

### 10.1 为什么不是 PASS / PASS-WITH-RISK

**7 条阻塞项**里，有 **4 条是「AC 的核心承诺完全没有测试」**（不是「测了但弱」）：

| 阻塞项 | 缺失的是什么 |
|---|---|
| B-01 ｜AC-S-151 | 业务端点**默认拒绝**的端到端证据（只测了 IAM 函数） |
| B-02 ｜AC-S-145 | **ES256 签名有效性**（只测了 header/payload 字段） |
| B-03 ｜AC-S-146 | 自签票据**越权**判据（原判据与 D260 替代判据**双缺**） |
| B-06 ｜AC-S-154 | 审计**落库 + 哈希链**（用的是内存假 sink） |
| B-05 ｜AC-S-149 | `logout_token` 的**内容**（只证明了「有东西被投递」） |

这五条的共同特征：**实现看起来是对的，但没有一个测试能在实现被改坏时变红**。
BA 报告把它们全部标成「✅ 通过」，且**描述的是 AC 的理想结果，不是测试的实际断言**
（§6.3 有逐条对照）。这是「假绿」的定义。

此外 **B-04（AC-S-140 路径 5 + D241）** 是「判据看起来有、但没有区分力」的典型
—— AC 原文专门警告的陷阱，测试掉进去了。

### 10.2 我验证了哪些点才敢说「通过」的 9 条

为避免「一律打回」的偏见，以下 9 条我**逐条打开源码确认断言有区分力**，才判通过：

1. **AC-S-137**：E2E 真的用 `page.route` 拦截 + `new URL(...).searchParams.get('code')`；
   前端单测**逐参数**断言 `return_to` 保留 6 个 query 参数。
2. **AC-S-138**：三路径 + `error_description` 的 CJK 逐字符扫描 + 4 个明文字串逐个断言。
3. **AC-S-141**：`get("sub").is_none()` 是**真值断言**（区分「键不存在」与「值为 null」）。
4. **AC-S-143**：别人令牌断的是 **`body == json!({"active": false})`** 整字段比对。
5. **AC-S-144**：含「别的 client 撤销 → 200 但令牌仍 active」这条高价值判据。
6. **AC-S-147**：**`assert_ne!(token1, token2)` + 两行 revoked** —— 「两枚令牌」是真的。
7. **AC-S-150**：前端 iframe 数量/src 逐字/**sandbox 不含 allow-same-origin** 全部有断言。
8. **AC-S-152**：`TITLES.len()` 的 93 是**公式算出来的常量**，`ALL_CODES` 那行真的在，
   且我实测 `TITLES` 里 `OAUTH_` 行 = 9。
9. **AC-S-153**：C-5 真的跑（2 passed）、6 个权限点**恰好**、我实测 capabilities 14 文件 / total 32。

### 10.3 最值得警惕的一点

> **BA 报告的 §4 表格，有 13 行的「判据落点」描述的是「AC 的预期结果」，
> 而不是「测试里的实际断言」。**

具体地，**AC-S-151 / 145 / 149 / 154** 四行的描述——「401 `UNAUTHENTICATED` + 不含 `error` 键」、
「`sid` 齐全、`events` 含 backchannel 键、无 `nonce`」、「独立 ES256 验签」、「哈希链 `ok=true`」——
**这些字符串在测试源码里根本不存在**。这不是「判断失误」，是**报告与代码脱节**。

**为什么这比单个缺陷更危险**：它让复核者失去「用报告定位判据」的能力。
我这次是靠**逐条打开测试源码**才发现的；任何人只要信了 §4 的表格，就会得出 22/22 的结论。

**根因猜测（可验证）**：BA 的 22 条判定是**照着 AC 的「预期结果」逐句回填落点**，
而不是「先看测试断言、再判 AC」。BA 报告 §0 第 3 条说「判据是 22 条逐条判定」，
但**形式上的 22 行不等于实质上的 22 次核对**。

### 10.4 给 BA 的最小修复集（我不改代码，只给建议）

| 优先级 | 要补的测试 | 对应阻塞项 |
|---|---|---|
| P0 | `platform-server` 集成测试：OAuth 令牌打 `/api/rbac/me/permissions` + `/api/audit/events` → 401 `UNAUTHENTICATED`（无 `error` 键）+ 登录令牌端点级阳性对照 | B-01 |
| P0 | 测试侧独立 ES256 验签（用 JWKS 公钥）+ 过期令牌「签名有效但 exp 过期」分开断言 | B-02 |
| P0 | 自签 ticket（scope 更大）→ 交换 400 `invalid_scope` + 令牌表 0 行 | B-03 |
| P0 | 真装配 + 真审计 sink：查审计表 + `audit/verify` 的 `ok=true` + 管理员直连插假 secret 再从表扫 | B-06 |
| P0 | 解析 `logout_token` payload：`sid`/`events`/无 `nonce`/`sid == 登录令牌 sid`；`MOCK_MODE=timeout` 与 `500` 后登出仍 200 | B-05 |
| P1 | 把 `rbac:role:*` 登记进 `platform_permission` 后再跑路径 4；补路径 5（epoch bump 后旧令牌不扩大） | B-04 |
| P1 | preview 用不在白名单的 `redirect_uri` → 400 `OAUTH_INVALID_REDIRECT_URI` | B-07 |
| P1 | 夹具把 `frontendBaseUrl` 设成 ≠ issuer（如 `https://e2e.base`），断 `authorization_endpoint` / `verification_uri` / `verification_uri_complete` | H-01 |
| P2 | 授权码 TTL 的 90s/30s 对照 INSERT | H-02 |
| P2 | `poll_interval_seconds == interval+5` / `last_polled_at` 递增 / user_code 当 device_code 失败 | H-03 |
| P2 | `PreviewResult.scopes` 的注释与行为对齐（改注释或改值，**并加一条测试**） | H-04 |

> **注**：以上修复**不改变任何 AC 措辞**，只是把 AC 已经写好的判据**真的实现**。
> 因此**不需要**回滚 D260/D259/D261，也不需要动 `acceptance.md`。

---

## 11. 复核的局限（诚实声明）

1. **E2E 我没有亲自跑**（`npx playwright test e2e/` 需要起完整服务 + 清库 + 手工 3 步建库，
   约 10+ 分钟，且会改动 `base_test` 的大量状态）。我核了 E2E **源码**与 BA 的产物
   （`target/mock-logout-received.jsonl` 578 字节、`target/e2e-oauth-clients.json`、
   `target/e2e-jwt-private.pem` 0600，时间戳 03:01，与 BA 报告时间线自洽）。
   **E2E 的 43 passed 我信它跑过，但没复跑。**
2. **AC-S-153 步骤 3/4 的两个变异**（删调用点 → C-5 失败；放 `authorize` 函数 → C-5 失败）
   **我没有亲跑**（它们需要临时改 `src/` 与 manifest，超出「只读复核」的边界）。
   机制上可信（`scan_auth_calls` 的 `custom_entries`/`non_literal_calls` 断言存在），
   但记为**未亲验**。
3. **「全量只跑一次」** 无法从产物证明。我看到的是一条 02:41 的完整 log（66 条 `test result`）
   与 03:01 的 E2E 产物，时间线自洽，**未发现违反**，但**不能证明**没有跑过第二次。
4. 我只抽查了 **2/9 条变异**（M4/M8）。其余 7 条（M1/M2/M3/M5/M6/M7/M9）我读了棒1 报告
   的描述与测试名，**看起来**对得上，但**没有逐条复跑**。
5. 我的 22 条判定是**基于测试源码的静态阅读 + 我的 4 次 test 运行**。
   若某条判据藏在**我没读到的文件**里（例如 `platform-server` 的其他测试），我可能漏判。
   我对「未找到」的每一条都做了**跨目录 grep**（`crates/`、`e2e/`、`apps/shell/test/`），
   但我不能保证 100% 穷尽。

---

**报告结束。**
