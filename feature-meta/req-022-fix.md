# REQ-022 修复轮报告（独立复核 FAIL 后）

- **需求**：REQ-022 出站 IdP 与单点登出
- **本轮性质**：**只补判据**，不写新功能、不改 AC 措辞、不回滚 D259/D260/D261
- **施工单**：`BA/feature-meta/req-022-fix-brief.md`（12 项）
- **被修复对象**：`BA/feature-meta/req-022-independent-review.md`（16 条）
- **本轮验收标准**：**每一处新断言都必须能被一个变异打红** → 逐条给了变异证明
- **结论**：12 项**全部完成**；16 条指控**全部处理**（15 条修了、1 条不修并说明理由）

---

## 0. 一句话总结

复核说「判据没有区分力」。本轮把 12 处判据**真的实现出来**，并**逐条注入变异**
确认「改坏生产那一行 → 该判据变红」。**23 条变异全部变红、全部逐字节还原。**

---

## 1. 逐条对照：复核的 16 条 → 处置

| # | 复核条目 | 对应 AC | 处置 | 落点 |
|---|---------|---------|------|------|
| 1 | **B-01** 只调 `verify_access_token` 函数，业务端点一次没打 | 151 | ✅ **修了** | 新建 `crates/platform-server/tests/oauth_real_stack.rs` → `an_oauth_token_is_rejected_by_the_business_endpoints` |
| 2 | **B-02** ES256 验签零覆盖 | 145 | ✅ **修了** | `oauth_http.rs` 的 `verify_signature()` + `the_access_token_is_an_es256_jwt_with_the_pinned_claims` + `an_expired_token_verifies_but_is_reported_as_expired` |
| 3 | **B-03** 自签票据越权判据双缺 | 146 | ✅ **修了** | `a_self_signed_ticket_with_a_larger_scope_cannot_obtain_a_token` |
| 4 | **B-04** 路径 5 + D241 三条对照全缺 | 140 | ✅ **修了** | `a_wildcard_scope_is_rejected_even_when_it_is_registered_everywhere`（D241）+ `an_already_issued_token_is_never_widened_retroactively`（路径 5） |
| 5 | **B-05** `logout_token` payload 零断言 | 149 | ✅ **修了** | `the_backchannel_logout_token_carries_the_pinned_claims` + `a_failed_logout_notification_never_fails_the_logout` + `a_failed_logout_delivery_is_logged_with_a_reason` |
| 6 | **B-06** 用内存假 sink，审计表/哈希链全未验 | 154 | ✅ **修了** | `oauth_real_stack.rs` → `the_oauth_actions_land_in_the_audit_table_with_an_intact_chain`（真 `PgAuditSink`） |
| 7 | **B-07** 回调白名单零测试 | 135 | ✅ **修了** | `a_redirect_uri_outside_the_whitelist_is_rejected` |
| 8 | **H-01** `{授权页基址}` 无区分力 | 134/142 | ✅ **修了** | `the_frontend_base_url_is_used_for_the_pages_but_not_for_the_api` + 夹具新增 `setup_with(settings)` |
| 9 | **H-02** TTL 60 秒零测试 | 139 | ✅ **修了** | `an_authorization_code_ttl_is_what_decides_expiry`（含 `consumed_at`/`used_at` 列判据） |
| 10 | **H-03** 设备码 4 个子判据缺失 | 142 | ✅ **修了** | `the_device_code_polling_state_is_written_to_the_database` + `verification_uri_complete` 断言进 H-01 那条 |
| 11 | **H-04** `PreviewResult.scopes` 注释与代码不一致 | 146 | ✅ **修了（选方案 A）** | 改 `service.rs` 的 `preview` + `the_consent_page_shows_exactly_what_will_be_granted` |
| 12 | **M-01** `client_secret_hash` 列 + 两个输出面未验 | 136 | ✅ **修了** | `the_stored_client_secret_hash_is_the_base64url_digest_of_the_plaintext` + `oauth_real_stack.rs` 的审计端点面 |
| 13 | **M-02** `platform_oauth_code` 行数 == 0 未验 | 148 | ✅ **修了** | 加强 `an_existing_grant_allows_silent_renewal` |
| 14 | **M-03** policy 命名 + 恰好 1 条未验 | 155 | ✅ **修了** | 加强 `tenant_isolation.rs` 的 `the_oauth_tables_are_isolated_by_tenant` |
| 15 | **L-01** 过期令牌内省未单独验 | 143 | ✅ **修了** | `an_expired_token_introspects_as_inactive` |
| 16 | **L-02** `userCode` / `user_code` 同名异形（纯可读性） | — | ❌ **不修**（见 §6） | — |

> **不修的那一条（L-02）**：复核自己定性为「纯可读性问题，不是缺陷」。
> 且修它要动 `apps/shell/src/lib/oauth-api.ts`（本轮硬规矩**禁止**碰前端）。
> → 建议作为后续「命名整理」的独立小改动，不阻塞本需求。

---

## 2. 12 项新判据：每项的变异证明

> **格式**：对应 AC / 测试函数 / **变异点（生产那一行）** / **变红的断言（file:line）** / 观察值 / 还原校验
>
> **还原方法**（全部一致）：备份副本 → 注入 → 跑 → `cp` 回副本 + `touch` → `sha256sum -c`
> → `grep -c MUTATION` == 0。**没有用过 `git checkout` / `restore` / `stash` / `reset`。**

### P0-1 · AC-S-151 业务端点默认拒绝（端到端）

- **测试**：`crates/platform-server/tests/oauth_real_stack.rs:393` `an_oauth_token_is_rejected_by_the_business_endpoints`
- **为什么之前不算**：夹具 router **结构上不含** `/api/rbac/*` 与 `/api/audit/*` → 老用例只能调函数。
  本文件建**真装配**：真 IAM + 真 RBAC（真 `platform_permission` 表）+ **真 `PgAuditSink`** + 4 条真路由。

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M1a** | `platform-core/src/problem.rs:158` `CODE_UNAUTHENTICATED` → `"invalid_token"` | `oauth_real_stack.rs:407` `body["code"] == "UNAUTHENTICATED"` | `left: "invalid_token"` / `right: "UNAUTHENTICATED"` | `sha256sum -c` 成功 / `MUTATION`=0 |
| **M1b** | `platform-core/src/problem.rs:296` `problem_response`（改 `:306` 的 `Json(details)`）往错误体多塞一个 `error` 键 | `oauth_real_stack.rs:409` `body.get("error").is_none()` | 错误体里出现 `"error":"invalid_token"` | `sha256sum -c` 成功 / `MUTATION`=0 |

- **阳性对照（端到端，不是函数级）**：同一个用户的**登录令牌**打同样两个端点 →
  `/api/rbac/me/permissions` **200** 且权限点集合**含** `audit:events:read`；
  `/api/audit/events` **200**。若对照组也 401，本条判据无区分力 → 断言会红。
- **步骤 3.5**：该 OAuth 令牌打 `POST /api/oauth/introspect`（client-A 的 `client_secret_basic`）
  → **200 + `active == true`** → 证明它**只在协议端点上有效**。

### P0-2 · AC-S-145 测试侧独立 ES256 验签

- **测试**：`the_access_token_is_an_es256_jwt_with_the_pinned_claims`（原函数，**加了验签段**）
  + `an_expired_token_verifies_but_is_reported_as_expired`（新）
- **实现**：`oauth_http.rs:220` `verify_signature()` —— 只用 `jsonwebtoken` 的通用 API
  （`JwkSet` / `DecodingKey::from_jwk` / `Validation` / `decode`），
  **不调用 `platform_oauth::keys` 的任何函数**；公钥来自 `GET /.well-known/jwks.json`（真端点）。
- **三档结果**：`Valid` / `ExpiredButSigned` / `BadSignature` —— 「签名无效」与「已过期」是**两个不同的枚举**。

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M2b** | `platform-oauth/src/service.rs:2025` `issue_access_token` 里把令牌**最后一个字符**改掉（签名失效，header/claims 全不变） | `oauth_http.rs:2401` `outcome == Valid` → `BadSignature`；`:2545` `ExpiredButSigned` → `BadSignature` | 两条测试都红（`-----` 0 passed / 2 failed） | `sha256sum -c` 成功 / `MUTATION`=0 |

- **阳性对照（验签本身有区分力）**：`tamper_payload()` 改载荷一个字节（**不重签**）
  → 断言必须 `BadSignature`。若验签恒过，这条会红。
- **过期 vs 无效的分离**：完整校验下过期令牌的 `ErrorKind` 必须是
  `ExpiredSignature`，改过的令牌必须是 `InvalidSignature` —— **两个不同的类型**逐字断言。
- **步骤 5**：`client_credentials` 令牌 `claims.get("sub").is_none()`（真值断言，不是 null 检查）。

> **诚实说明**：第一次注入 `M2`（把 `sign_claims` 换成另一把密钥）时，
> `the_access_token_...` 红在 `oauth_http.rs:1501`「应当有票据」—— 因为登录令牌也走
> `sign_claims`，整个链路先崩了。这种变异**太粗**（同时打挂登录），
> → 改用 M2b（只改 OAuth 令牌的签名）才是**精准**变异。M2b 的红色落在验签断言本身。

### P0-3 · AC-S-146 自签票据越权（D260 的替代判据）

- **测试**：`a_self_signed_ticket_with_a_larger_scope_cannot_obtain_a_token`
- **自签**：`self_signed_ticket()` 用**夹具持有的 IAM 私钥**（同一把、同一 `kid`）
  签 `typ="oauth_consent_ticket"`、`scope="rbac:role:read rbac:role:write"`
  （用户只有 read）→ **平台验签会通过**（这是私钥泄露的前提）。
- **四步断言**：① confirm **200**（D260 的实现选择）→ ② 交换 **400 `invalid_scope`**
  → ③ `platform_oauth_token` **0 行**（查库，不信响应）→ ④ **TD-046 的代价**已测出。

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M3** | `platform-oauth/src/service.rs:1952` `require_scope_granted` 里把 `Err(InvalidScope)` 改成 `Ok(())` | `oauth_http.rs:2704` 「交换令牌必须 400」 | `left: 200` / `right: 400`，且响应体**真的**带 `access_token`（越权令牌被签发出来） | `sha256sum -c` 成功 / `MUTATION`=0 |

- **TD-046 的代价（测出来了，不是「没人知道」）**：
  `platform_oauth_grant` **真的多了一行**，且那一行的 `scopes` 是**交集**（只有 `read`）——
  用户会在「我的授权」里看到一条自己没同意过的记录。断言行：
  `assert_eq!(grant_rows.len(), 1, "TD-046 的已知代价…")`。

### P0-4 · AC-S-154 真审计 sink + 哈希链 + 从表扫

- **测试**：`oauth_real_stack.rs:505` `the_oauth_actions_land_in_the_audit_table_with_an_intact_chain`
- **sink**：`platform_audit::PgAuditSink`（**真库**，不是 `RecordingSink`）。
- **走一遍**：注册 client → 同意 → 换令牌 → 撤销 → 登出（5 个动作，全打真端点）。

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M4a** | `platform-oauth/src/service.rs:2057` `audit_event` 开头 `return`（埋点变空操作） | `oauth_real_stack.rs:680` 「审计表里缺少事件码」 | `[]`（表里 0 行） | `sha256sum -c` 成功 / `MUTATION`=0 |

- **查表（不是查内存）**：`SELECT ... FROM platform_audit_log WHERE tenant_id = $1` →
  5 个事件码逐个断言（`oauth.client.registered` / `oauth.authorization.approved` /
  `oauth.token.issued` / `oauth.token.revoked` / `oauth.logout.executed`）。
- **固定字段集（契约 C12）**：`action` / `object_type` / `created_at` 非空；
  `prev_hash` 与 `hash` **各 64 位十六进制**；`client.registered` 的 `actor_id`
  **逐字等于**发起注册的用户（这条比「非空」强）。
- **哈希链自洽**：`ChainVerifier::verify(tenant-a)` → `report.ok == true`，
  且 `report.checked >= rows.len()`（证明真的检查了那些行，不是空跑）。
- **敏感值扫描（从表里扫）**：输入 = 审计行的 `action` / `object_type` / `object_id` /
  `before` / `after` **五列**；真实数据 **0 命中**。
- **阳性对照（管理员直连插 → 扫到 → 删掉 → 扫不到）**：
  插 `before = '{"client_secret":"POSITIVE-CONTROL"}'` → **必须命中** → `DELETE` → **必须不命中**。

### P0-5 · AC-S-149 `logout_token` 内容 + 失败不影响登出 + 有日志

- **测试**（三条）：
  1. `the_backchannel_logout_token_carries_the_pinned_claims`（payload 内容）
  2. `a_failed_logout_notification_never_fails_the_logout`（`500` 与 `timeout` 两种模式）
  3. `a_failed_logout_delivery_is_logged_with_a_reason`（失败有日志）
- **接收端**：测试侧自己的**真实 HTTP 服务**（`127.0.0.1:0` 随机端口）→ `OutboundClient`
  走**真的** hyper 出站。`ReceiverMode::Fail500` / `Hang`（hang 8 秒 > 出站 5 秒超时）
  分别等价 `MOCK_MODE=500` / `MOCK_MODE=timeout`。

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M5a** | `platform-oauth/src/logout.rs:155` `sid: user.session_id.clone()` → 加 `-MUTATION` 后缀 | `oauth_http.rs:3549` 「sid 必须等于触发登出那次请求的会话 ID」 | `left: "session-user-a-MUTATION"` / `right: "session-user-a"` | `sha256sum -c` 成功 / `MUTATION`=0 |
| **M5b** | `LogoutTokenClaims` 加 `nonce` 字段 + `logout.rs:158` 填它 | `oauth_http.rs:3544` 「logout_token 不得含 nonce」 | payload 里出现 `"nonce":"MUTATION"` | `sha256sum -c` 成功 / `MUTATION`=0 |
| **M5c** | `logout.rs:112` 把 `deliver_backchannel_logouts` 之后**返回 Err**（把通知失败当致命） | `oauth_http.rs:3588` 「登出本身必须仍然 200」 | `left: 500` / `right: 200`（`Fail500` 时） | `sha256sum -c` 成功 / `MUTATION`=0 |
| **M5d** | `logout.rs:199-202` 删掉投递失败的 `tracing::warn!`（日志语句在 `:201`） | `oauth_http.rs:3321` 「投递失败必须留日志」 | `captured` 里**没有**那条日志 | `sha256sum -c` 成功 / `MUTATION`=0 |

- **payload 断言明细**：`iss == "base"`、`aud == "client-a"`、`sub`/`sid`/`jti`/`iat` 齐全、
  `events` **含键** `http://schemas.openid.net/event/backchannel-logout`、
  **不含 `nonce`**、`sid` **逐字等于**登录令牌的 `sid`（D230）。
- **投递是 form 编码**：断言 `content-type` 以 `application/x-www-form-urlencoded` 开头。
- **两种失败模式都跑**：`for mode in [Fail500, Hang]` → 登出 200 / 投递真的发了一次
  （`receiver.requests().len() == 1`，否则测的是「没投递」）/ grant 已 `revoked`。

### P1-6 · AC-S-140 路径 5 + D241 真对照

- **测试**（两条）：`a_wildcard_scope_is_rejected_even_when_it_is_registered_everywhere`
  + `an_already_issued_token_is_never_widened_retroactively`
- **D241 真对照的关键**：把通配同时放进**三处**（让交集**本可以**包含它）：
  ① 管理员直连 `INSERT INTO platform_permission ('rbac:role:*')`
  ② client 的 `allowed_scopes` ③ 用户的有效权限点集合。
  再用完 `DELETE`（D241 第 3 条的字面要求）。

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M6a** | `service.rs:1927` `granted_scopes` 去掉 `!scope.contains('*')` **且** `:1950` `require_scope_granted` 去掉 `scope.contains('*') \|\|` | `oauth_http.rs:754` 「通配登记过…仍必须 400」 | `right: 400` 实际拿到 200（通配被授予） | `sha256sum -c` 成功 / `MUTATION`=0 |
| **M6b** | `service.rs:1652` `introspect` 的 `scope: Some(claims.scope.clone())` 改成并进**当前**权限（回溯扩大） | `oauth_http.rs:827` 「旧令牌不得被回溯扩大」 | `left: "rbac:role:read rbac:role:write"` / `right: "rbac:role:read"` | `sha256sum -c` 成功 / `MUTATION`=0 |

- **路径 5 的两个半边同时断**：令牌 A 重放（**不重新交换**）内省 `scope` 仍逐字 `rbac:role:read`；
  另发令牌 B `scope` **含** `rbac:role:write`。中间真的 bump 了 `platform_permission_epoch`
  （并断言 `epoch >= 1`，证明那个动作不是空跑）。
- **D241 第 1/2 条**：`platform_permission` 里 `rbac:role:*` 计数 == 0（前置）；
  `/api/rbac/me/permissions` 的集合 `NOT LIKE '%*%'`（在 P0-1 的阳性对照里断）。

### P1-7 · AC-S-135 回调白名单

- **测试**：`a_redirect_uri_outside_the_whitelist_is_rejected`
- **`OAUTH_INVALID_REDIRECT_URI` 是不是死码？** —— **不是**。
  它在 `platform-oauth/src/error.rs:42` 定义、`:106` 映射、`:284` 进 `ALL_CODES`、
  `service.rs:834` 真的被 `return Err(OAuthError::InvalidRedirectUri.into())` 使用，
  且是 9 个 `OAUTH_*` 平台错误码之一（`TITLES` 93 行里有一行）。
  **之前是「在用但零测试」，不是死码。**

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M7** | `service.rs:838` `if !client.redirect_uris.iter().any(...)` → `if false && ...`（恒放行） | `oauth_http.rs:1202` 「不在白名单里的回调必须 400」 | `left: 200` / `right: 400`，响应体**真的**带 `ticket`（开放重定向被放出去） | `sha256sum -c` 成功 / `MUTATION`=0 |

- **阳性对照**：同一个 client 用**白名单内**的地址 → 200 且有 `ticket`
  （证明上面的 400 是因为地址，不是别的参数坏了）。

### P1-8 · AC-S-134 `{授权页基址}` 的区分力

- **测试**：`the_frontend_base_url_is_used_for_the_pages_but_not_for_the_api`
- **夹具改动**：新增 `setup_with(settings)`；本条把 `frontendBaseUrl = "https://frontend.example"`
  （**≠** issuer `base`）。

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M8** | `service.rs:1813` `authorization_endpoint` 与 `:1404` `verification_uri` 的 `{base}` → `{issuer}` | `oauth_http.rs:1270` 「authorization_endpoint 必须用授权页基址」 | `right: "https://frontend.example/oauth/authorize"`，实际 `base/oauth/authorize` | `sha256sum -c` 成功 / `MUTATION`=0 |

- **两端分开断**：页面类（`authorization_endpoint` / `verification_uri` /
  `verification_uri_complete`）用 `https://frontend.example`；
  接口类（`token_endpoint` / `jwks_uri` / `device_authorization_endpoint` /
  `introspection_endpoint` / `revocation_endpoint`）用 `base`。
- `verification_uri_complete` **逐字**等于 `https://frontend.example/oauth/device?user_code=<code>`
  —— 顺带补上了 H-03 的第 4 个缺口。

### P1-9 · `PreviewResult.scopes` 注释/代码对齐 —— **选方案 A**

**选了 A（preview 真算交集）**，理由见 §3。

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M9** | `service.rs:947` `scopes: granted` → `scopes: requested`（回到旧行为） | `oauth_http.rs:632` 「同意页必须只展示实际会授予的 scope」 | 返回 `["rbac:role:read","rbac:role:write"]`（应只有 read） | `sha256sum -c` 成功 / `MUTATION`=0 |
| **M9b** | `service.rs:934` 签票据时 `&requested.join(" ")` → `&granted.join(" ")` | `oauth_http.rs:660`（本条）+ `:1799`（**AC-S-140 路径 2**）**同时红** | 两条都红：路径 2 拿到 200 + 令牌 | `sha256sum -c` 成功 / `MUTATION`=0 |

> **M9b 是关键证明**：它证明「选 A 时如果把票据也改成交集，AC-S-140 会静默失效」——
> 所以本用例把「preview 只展示交集」与「交换仍必须 400」写在**同一个测试**里。

### P2-10 · AC-S-139 TTL 90s / 30s 对照

- **测试**：`an_authorization_code_ttl_is_what_decides_expiry`
- **按 D252 逐列写死直插**：`tenant_id` / `client_id` / `user_id` / `code_hash` /
  `redirect_uri` / `scope` / `code_challenge` / `code_challenge_method` 全部绑定，
  **只有 `expires_at`** 在两次插入之间变（`now() - 90s` vs `now() + 30s`）。

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M10** | `service.rs:1081` `if row.expires_at <= now()`（授权码交换） → `if false && ...`（不判 TTL） | `oauth_http.rs:907` 「过期 90 秒的授权码必须 400」 | `left: 200` / `right: 400` | `sha256sum -c` 成功 / `MUTATION`=0 |

- **另含步骤 5**：`information_schema.columns` 里 `platform_oauth_code`
  **不含** `consumed_at` / `used_at`（D253）。

### P2-11 · AC-S-142 设备码库侧断言

- **测试**：`the_device_code_polling_state_is_written_to_the_database`

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M11** | `service.rs:1272` `let next_interval = interval + 5` → `= interval` | `oauth_http.rs:1035` 「slow_down 后 poll_interval_seconds 必须等于 interval + 5」 | `right: 10`，实际 5 | `sha256sum -c` 成功 / `MUTATION`=0 |

- **四条库侧断言**：
  ① 初始 `poll_interval_seconds == 5`；
  ② `slow_down` 后 **`== interval + 5`（10）**；
  ③ `last_polled_at` **非空且严格递增**（读 `extract(epoch FROM …)` 比大小）；
  ④ 拿 `user_code` 当 `device_code` → **400 `invalid_grant`**（证明两个摘要域不串）；
  ⑤ 换令牌后按 `device_code_hash` 查 **== 0**（行已删除）。

### P2-12 · AC-S-136 补两个面

- **测试**：`the_stored_client_secret_hash_is_the_base64url_digest_of_the_plaintext`

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **M12** | `platform-oauth/src/crypto.rs:37` `URL_SAFE_NO_PAD.encode(...)` → `format!("{:x}", ...)`（hex 口径，D228 明令禁止） | `oauth_http.rs:1144` 「client_secret_hash 必须是 base64url」 | `left: "1fd35016…"`（hex 64 位）/ `right: "H9NQFvbf…"`（base64url 43 位） | `sha256sum -c` 成功 / `MUTATION`=0 |

- **步骤 1**：管理员直连查 `client_secret_hash` 列，与**测试侧独立算**的
  `digest_of()`（`oauth_http.rs:70`，另写一份）逐字节比对；附断 43 字符 + **不是 hex**。
- **步骤 4 的四处输出**：`GET /clients`、`GET /clients/{id}`（原有）+
  **`POST /api/oauth/token` 的响应体**（新）+ **`GET /api/audit/events` 的全表文本**
  （新，在 `oauth_real_stack.rs` 里打真端点）。

### P1-13 · E2E：未登录 → 登录 → 回跳后 query 还在

- **测试**：`e2e/oauth.spec.ts:166` 「未登录 → 登录并继续 → 回到授权页且 6 个 query 一个不丢」

| 变异 | 改的生产行 | 变红的断言 | 观察值 | 还原 |
|---|---|---|---|---|
| **ME2E** | `apps/shell/src/lib/oauth-api.ts:275` `currentTarget()` 只返回 `pathname`（丢 query —— D221 之前的缺陷形态） | `oauth.spec.ts:200` `toHaveURL(/\/oauth\/authorize\?/)` | 回跳落到 `/oauth/authorize`（无 query）→ 超时失败 | `sha256sum -c` 成功 / `MUTATION`=0 |

- **五步都在**：① 不先登录直接 `goto(authorizeUrl(...))`；② 断「登录并继续」可见
  **且 `previewRequests == 0`**（用 `page.on('request')` 计数）；
  ③ 点它 → `/login?return_to=…`；④ 填表登录；
  ⑤ 回跳后 `client_id` / `state` / `code_challenge` / `redirect_uri` / `scope` / `response_type`
  **逐个**与请求时相等。
- **反向对照**：登录**之后** `previewRequests > 0`（`expect.poll`）
  → 否则第 ② 步的 `0` 可能是空跑。

### 附带修复（复核的 L-01 / M-02 / M-03）

| 项 | 测试 | 变异 | 结果 |
|---|---|---|---|
| **L-01** 过期令牌内省 | `an_expired_token_introspects_as_inactive`（TTL=-60 由**平台自己签**） | **M13**：三层同时关掉（`service.rs:1602` `claims.exp <= unix_now()`、`:1629` `row.expires_at <= now()`、`keys.rs:102` `validate_exp`） | 单开任一层仍绿（三层冗余）→ **三层全关**才红。**这是如实记录的一个观察**：内省有 3 道过期闸 |
| **AC-S-148 免登覆盖判定** | 加强 `an_existing_grant_allows_silent_renewal`（旧行为部分） | **M2b_preview**：`service.rs:898` 的 `&& scopes_cover(&grant.scopes, &requested)` → `&& true`（忘判覆盖） | 红：`needsConsent` 期望 `true` 实际 `false` |
| **M-02** code 表 0 行 | 加强 `an_existing_grant_allows_silent_renewal` | **M2c**：`preview` 在 `needsConsent=true` 时**偷偷**也调 `issue_authorization_code` | 红：`right: 1`，实际 2 行 |
| **M-03** policy 名字 + 恰好 1 条 | 加强 `the_oauth_tables_are_isolated_by_tenant` | **M3rls**：`platform-db/src/rls.rs:36` `POLICY_SUFFIX` 改 `_isolation` | 红：`["platform_oauth_grant_isolation", "platform_oauth_grant_tenant_isolation"]` |

> ⚠️ **M3rls 的环境副作用已清理**：该变异把 policy **写进了真库**。
> 还原源码后重跑用例仍红（库里留着两条 policy）→ 我按 policy 名手工 `DROP POLICY` 清掉
> 4 条残留 → 重跑 **2 passed** → 再查 `pg_policies` 确认每表**恰好 1 条**
> `platform_oauth_XXXX_tenant_isolation`。

---

## 3. P1-9 为什么选方案 A

**选 A（preview 真算交集），不选 B（改注释 + 改前端文案）**。三条理由：

1. **AC-S-146 的标题就是「用户所见 = 实际授予」**。选 B 等于承认「所见 ≠ 所得」
   并把规格降级 —— 那要改 AC 措辞，而施工单明确「本轮不改任何 AC 措辞」。
   选 A 是**让代码追上规格**。
2. **选 B 要改前端文案**（`apps/shell/**` + `packages/i18n/**`）——
   本轮硬规矩**明令禁止**，且施工单要求「先问 BA」。选 A 只动
   `crates/platform-oauth/src/service.rs`（在允许范围内）。
3. **交集已经在算了**：`confirm` 本来就要调 `granted_scopes()` 写 grant
   （D202）→ preview 复用它**不增加任何新逻辑**，只是把结果多回传一次。

**关键约束（已满足并逐条断言）**：`preview.scopes` = 交集，
但**票据仍装 `requested`** → 令牌端点仍按**请求的** scope 判 `invalid_scope`（AC-S-140 不变）。
`the_consent_page_shows_exactly_what_will_be_granted` 把这两件事写在**同一个测试**里，
`M9b` 变异证明「若两者一起改，AC-S-140 的路径 2 会静默失效」。

**顺带修正的文档**：`PreviewResult.scopes` 的 doc comment 从「**实际授予**的 scope（数组）」
改成明确写「已取交集」「与票据里装的 scope **不同**」——注释与代码现在一致。

---

## 4. 修复后的测试数字（我自己跑的，不是抄的）

命令（与施工单 §9.14 阶段一一致）：

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
cargo test -p platform-oauth -p platform-server --all-targets -- --test-threads=1 --include-ignored
npx playwright test e2e/oauth.spec.ts
```

| crate / 套件 | 本轮实测 | 改前 | 增量 |
|---|---|---|---|
| **platform-oauth** | **131 passed / 0 failed / 0 ignored** | 117 | **+14** |
| **platform-server** | **131 passed / 0 failed / 0 ignored** | 129 | **+2** |
| **E2E（`e2e/oauth.spec.ts`）** | **9 passed** | 8 | **+1** |
| 两 crate 合计 | **262 passed / 0 failed / 0 ignored**（16 条 `test result` 行） | 246 | +16 |

`platform-oauth` 的 131 拆解：**91** lib 单测 + **2** `contract_c5` + **36** `oauth_http`
+ **2** `tenant_isolation`。
`platform-server` 的 131 拆解：70 lib + 8 bin + 5 `audit_chain` + 1 `audit_default_size`
+ 17 `i18n_api` + 5 `i18n_context` + 6 `i18n_no_chinese` + 3 `i18n_settings` + 9 `i18n_titles`
+ **2 `oauth_real_stack`（新）** + 1 `sso_me_permissions` + 4 `sso_real_db`。

> **没有跑 14 个 crate 的全量**（施工单明令只要增量）。
> **没有跑 `apps/shell` 的前端单测** —— 本轮**没改任何前端源码**
> （`apps/shell/src/lib/oauth-api.ts` 只在变异里被临时改过并已逐字节还原），
> 前端行为不受影响；E2E 9 条已覆盖前端渲染与点击。

---

## 5. 新发现的与规格不一致（如实列出）

### 5.1 `logout.rs` 的 `client_id` 字段实际装的是**投递 URI**（AC-S-149 的「含 clientId」未满足）

- **位置**：`crates/platform-oauth/src/logout.rs:184` `let client_id = uri.clone();`
- **现象**：投递失败的日志字段名叫 `client_id`，值却是
  `http://127.0.0.1:PORT/logout`（URI，不是 `client-a`）。
- **AC-S-149 原文**：「投递失败时**有日志**（含 `clientId` 与失败原因）」——
  **失败原因有了，`clientId` 没有**。
- **本轮处置**：**不修**（本轮只补判据；修它要改 `deliver_backchannel_logouts`
  的签名从 `Vec<(String, String)>` 变成带 `client_id` 的三元组，属功能改动）。
  测试里**如实断言当前行为**：
  - 断言日志**含 URI**（「能定位到投给谁」）；
  - **显式断言日志不含 `client-a`** 并注明「这是与 AC-S-149 的已知偏差，
    若哪天修了这条会红并提示同步更新报告」。
  → 这样既不让判据空跑，也不假装 `clientId` 已经记了。
- **建议**：记一条 TD，或在下一轮把 `deliver_backchannel_logouts` 的载荷改成
  `Vec<(String /*client_id*/, String /*uri*/, String /*token*/)>`。

### 5.2 内省的「过期」有**三层冗余闸**（不是缺陷，但影响变异设计）

- `keys.rs` 的 `validate_exp` → `service.rs` 的 `claims.exp <= now()` →
  `service.rs` 的 `row.expires_at <= now()`。
- 关掉任一层，`an_expired_token_introspects_as_inactive` **仍然绿** → 必须三层全关才红。
- **这是纵深防御，不是缺陷**。但记在这里，因为「只关一层仍然绿」容易被误读成「判据没区分力」。

### 5.3 `PreviewResult.scopes` 语义变了 —— 前端**没有**回归，但值得知道

- 现在 `scopes` 是交集。前端 `OAuthAuthorize.svelte` 直接渲染它 →
  用户只会看到**确实会授予**的权限点（这正是 AC-S-146 要的）。
- **E2E 仍绿**（`oauth-scopes` 含 `rbac:role:read`）：因为用例请求的 scope
  用户确实有 → 交集 == 请求值，看不出差别。**这不是回归**。
- ⚠️ 但若将来有「请求了用户没有的 scope」的 **E2E**，页面文本会**少**那个码 ——
  那是**正确行为**（用户所见 = 实际授予），不是缺陷。

---

## 6. 未完成项与理由（如实）

| 项 | 状态 | 理由 |
|---|---|---|
| **L-02**（`userCode` vs `user_code` 同名异形） | **未修** | 复核自己定性为「纯可读性，不是缺陷」；修它要动 `apps/shell/src/lib/oauth-api.ts`（本轮硬规矩**禁止**碰前端）。建议作为独立命名整理 |
| **AC-S-153 步骤 3/4 的两个变异**（删 C-5 调用点 → C-5 失败；放 `authorize` 函数 → C-5 失败） | **未做** | 复核自己标为「未亲验但机制可信」。本轮**未新增** C-5 判据（`contract_c5.rs` 未改），所以不在「每一处新断言」的范围内。若 BA 要求，需要单独一轮 |
| **`logout_token` 的**签名**验证** | **未做** | AC-S-149 只要求「测试侧**独立解析** payload」。签名的有效性由 AC-S-145 的 JWKS 用例守（同一套密钥、同一个 `kid`）。若要在 AC-S-149 里也验签，需要把 `verify_signature()` 搬进那条用例（可行，但超出 AC 字面要求） |
| **`platform-oauth` 之外 crate 的全量回归** | **未跑** | 施工单明令「只跑增量，不要跑 14 个 crate 的全量」。本轮**没改** `platform-iam` / `platform-core` / `platform-db` 的生产代码（只在变异里临时改过并逐字节还原）。⚠️ `platform-core/src/problem.rs` 与 `platform-db/src/rls.rs` 的变异确实触及过，但已 `sha256sum -c` 确认还原；`problem.rs` 与 `rls.rs` 的**既有**测试在 `platform-server` / `platform-oauth` 的增量里间接覆盖 |
| **`M2`（把 `sign_claims` 换密钥）这个变异** | **作废，改用 M2b** | 见 P0-2 的诚实说明：它同时打挂登录令牌，红在「应当有票据」而不是验签断言 → 不够精准，不算数 |

---

## 7. 硬规矩遵守情况

| 规矩 | 核实 |
|---|---|
| **只改** `crates/platform-oauth/tests/**`、`crates/platform-server/tests/**`、`e2e/oauth.spec.ts`、`crates/platform-oauth/src/**` | ✅ 新增/修改的文件：`crates/platform-oauth/tests/{oauth_http.rs,tenant_isolation.rs,common/mod.rs}`、`crates/platform-server/tests/oauth_real_stack.rs`（新）、`e2e/oauth.spec.ts`、`crates/platform-oauth/src/service.rs`（仅 P1-9） |
| **不要碰** `apps/shell/**` 与 `packages/i18n/**` | ✅ 前端源码**零改动**。`apps/shell/src/lib/oauth-api.ts` 只在 P1-13 的**变异**里被临时改过，已 `sha256sum -c` 逐字节还原（`grep MUTATION` == 0） |
| **不要** `git commit` / `checkout` / `restore` / `stash` / `reset` / `add -A` | ✅ 一次都没用。还原全部走「备份副本 + `cp` + `touch` + `sha256sum -c`」 |
| **零新增第三方依赖** | ✅ `Cargo.toml` 一个字节没改。用到的 `jsonwebtoken` / `p256` / `sha2` / `base64` / `time` / `axum` / `tokio` 都已在树里（`p256` 未直接用到 —— 验签走 `jsonwebtoken`，它内部用 `p256`） |
| **变异验证**：每处新判据都要有一条 | ✅ **23 条变异**，见 §2 各表 |
| **还原校验** | ✅ **8 个被改的生产文件全部 `sha256sum -c` 成功**；`grep -rn MUTATION crates/ e2e/ apps/shell/src apps/shell/test` → **无输出** |
| **如实报告** | ✅ 见 §5 / §6：点名了 1 处已知偏差（`clientId`）、1 个作废变异（M2）、4 项未做 |

### 变异清单（23 条，按文件归组）

| 文件 | 变异 | 结论 |
|---|---|---|
| `platform-core/src/problem.rs` | M1a 错误码 / M1b 多余 `error` 键 | 2 条红 ✅ |
| `platform-oauth/src/service.rs` | M2b 签名篡改 / M3 不拒超范围 / M4a 审计空操作 / M6a 去通配过滤 / M6b 内省回溯 / M9 preview 展示请求值 / M9b 票据装交集 / M10 不判 TTL / M11 忘 +5 / M2c 偷偷发码 / M2b_preview 忘判覆盖 / M13a+M13b 内省不判过期 | 12 条红 ✅ |
| `platform-oauth/src/logout.rs` | M5a `sid` / M5b 加 `nonce` / M5c 通知失败当致命 / M5d 删失败日志 | 4 条红 ✅ |
| `platform-oauth/src/keys.rs` | M13c 不验 `exp` | 1 条（需与 M13a/b 同时）✅ |
| `platform-oauth/src/crypto.rs` | M12 摘要口径改 hex | 1 条红 ✅ |
| `platform-db/src/rls.rs` | M3rls policy 改名 | 1 条红 ✅ |
| `apps/shell/src/lib/oauth-api.ts` | ME2E 丢 query | 1 条红 ✅ |

---

## 7.1 关于本报告里的行号（**核对前请读这一段**）

- 文中所有 `file:line` 都是**本轮结束时（代码冻结后）**的真实行号 ——
  我用 `grep -n` 逐个查出后回填的，**不是**写报告时凭记忆填的。
- 如果你按行号打开对不上（源码后来又动了），**以函数名为准**：
  每条判据都写了完整的**测试函数名**，`grep -n "async fn <名字>"` 一定找得到。
- 我**没有**为「行号好看」去重排源码。

## 8. 完工信号

- **报告绝对路径**：`/home/myg133/Projects/system_seed/feature-REQ-022/feature-meta/req-022-fix.md`
- **新增/修改的测试函数**：**新增 17 个**（`oauth_http.rs` 14 + `oauth_real_stack.rs` 2 + E2E 1）
  + **加强 3 个**（AC-S-145 / AC-S-148 / AC-S-155 的既有用例）= **20 处**
- **变异验证条数**：**23 条**，全部变红、**8 个生产文件全部逐字节还原**
- **修复后的数字**：`platform-oauth` **131 passed** / `platform-server` **131 passed** /
  E2E **9 passed**（全部 0 failed / 0 ignored）
- **12 项完成情况**：**P0-1 ~ P2-12 全部完成**（12/12），另附带修了复核的 L-01 / M-02 / M-03
- **需要 BA 拍板的问题**：
  1. **`logout.rs` 的 `client_id` 装的是 URI**（§5.1）—— 是修（改投递函数签名）
     还是记 TD？本轮按「只补判据」如实断言了现状。
  2. **L-02（`userCode` / `user_code` 命名）** 要不要单独一轮收拾？
  3. **AC-S-153 步骤 3/4 的两个 C-5 变异**要不要补？（本轮未新增 C-5 判据，故不在范围）
