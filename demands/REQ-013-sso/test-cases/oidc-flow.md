# OIDC 授权码流程端到端（AC-S-100）

- **覆盖 AC**: AC-S-100（1 条）
- **用例编号**: TC-013-001 ~ TC-013-012（12 条，连续无跳号）
- **前置基线**: 见 [index.md](index.md) §2 夹具。本节全部用例共用：
  - 租户 `tenant-a`，`sso.oidc.enabled=true`，`issuer=http://127.0.0.1:9410/idp-a`，
    `clientId=base-portal`，`redirectUri=http://127.0.0.1:8080/api/iam/sso/oidc/callback`，
    `scopes="openid email profile"`
  - mock IdP `idp-a`（进程内 axum，端口 9410）：`/.well-known/openid-configuration` /
    `/authorize` / `/token` / `/jwks`，每个端点**自带请求计数器**并在
    `GET /__stats` 返回 `{discovery:n, authorize:n, token:n, jwks:n, last_filter:null}`
  - 用户 `bob`（`platform_iam_user.username=bob`，`email=bob@example.com`）已绑定
    `external_identity(tenant_id='tenant-a', provider='oidc', external_id='idp-bob-001')`
  - 环境变量 `BASE_SSO_OIDC_CLIENT_SECRET=SECRET-CANARY-7f3a9b1e-UNIQUE`

> **本文件的三种标注**（后文沿用）：
> - `〔AC 修正〕`：AC 原文的步骤/期望不可执行或与实测不符，用例按**可执行的等价判据**写，
>   修正点已进挑刺报告（括号里给编号）。
> - `〔守卫〕`：断言的是「AC 没定义、但不定下来就无法判定」的东西。**该用例在 BA 钉死前必红**。
> - `〔补充〕`：AC 没写、但同一条 AC 的语义必然要求，用来堵假绿。

---

### TC-013-001: `authorize` 返回 302 且 `Location` 的 7 个 query 参数逐项正确

- **关联 AC**: AC-S-100
- **前置条件**: 见本节基线；mock IdP 计数器归零（`POST /__stats/reset`）
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`，请求头 `x-tenant-id: tenant-a`，客户端关闭自动跟随跳转
  2. 读响应 `Location`（不跟随），用 `url::Url` 解析 query
  3. `GET http://127.0.0.1:9410/__stats` 读 `discovery` 计数
- **预期结果**:
  1. 状态码 `302`
  2. `Location` 的 scheme+host+path **逐字等于**发现文档里的 `authorization_endpoint`
     （`http://127.0.0.1:9410/idp-a/authorize`）
  3. query 含 `response_type=code`；`client_id=base-portal`；
     `redirect_uri=http://127.0.0.1:8080/api/iam/sso/oidc/callback`（**逐字**，含 scheme、端口、无尾斜杠）；
     `scope` 按空格切分后的集合 **等于** `{openid, email, profile}`；
     `code_challenge_method=S256`；存在非空 `state`；存在非空 `nonce`
  4. `discovery` 计数 `== 1`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-002: `authorize` 落一条 `oidc_state`，字段值与 302 的 query 对应

- **关联 AC**: AC-S-100
- **前置条件**: 同 TC-013-001；先 `DELETE FROM platform_iam_oidc_state WHERE tenant_id='tenant-a'`
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`（`x-tenant-id: tenant-a`），取 302 的 `state` / `nonce` / `code_challenge`
  2. 用管理员连接查 `platform_iam_oidc_state`（`base_test`）
- **预期结果**:
  1. `SELECT count(*) ... WHERE tenant_id='tenant-a'` → `1`
  2. 该行 `provider='oidc'`、`tenant_id='tenant-a'`、`consumed_at IS NULL`
  3. 该行 `nonce` **逐字等于**步骤 1 的 `nonce`
  4. 该行 `code_verifier` 经 `base64url(SHA-256(verifier))` 后的值 **等于**步骤 1 的 `code_challenge`
     （即库里存的是 verifier 明文，能反算 challenge）
  5. `expires_at > created_at`，且 `expires_at - created_at` 等于配置的 TTL（默认 300 秒，容差 ±2 秒）
  6. 该行 `redirect_uri` **逐字等于** `sso.oidc.redirectUri` 的当前值
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-003: 回调成功返回与本地登录**键集合完全相同**的 `LoginResponse`

- **关联 AC**: AC-S-100
- **前置条件**: 已按 TC-013-001 取得 `state`；mock `/authorize` 对任意请求 302 回
  `redirect_uri?code=CODE-BOB-1&state=<state>`
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/callback?code=CODE-BOB-1&state=<state>`
  2. 取响应 JSON 的顶层键集合
  3. 对同一用户走本地登录（`bob` 的本地密码）后取响应 JSON 的顶层键集合
- **预期结果**:
  1. 步骤 1 状态码 `200`
  2. 步骤 2 的键集合 `==` `{accessToken, refreshToken, tokenType, expiresIn, user, permissions}`
     （**6 个键，不多不少**；多一个或少一个都红）
  3. `tokenType == "Bearer"`；`expiresIn` 为整数且 `> 0`
  4. 步骤 3 的键集合与步骤 2 的键集合**相等**
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: 键名依据 `crates/platform-iam/src/service.rs:163-184` 的 `#[serde(rename_all = "camelCase")]`。

### TC-013-004: `LoginResponse.user` 逐字段等于绑定的本地 user

- **关联 AC**: AC-S-100
- **前置条件**: 同 TC-013-003；记录 `bob` 在 `platform_iam_user` 的行
- **测试步骤**:
  1. 完成一次 OIDC 回调
  2. 比对响应 `user` 与数据库行
- **预期结果**:
  1. `user.id == platform_iam_user.id`
  2. `user.username == "bob"`；`user.email == "bob@example.com"`
  3. `user.tenantId == "tenant-a"`
  4. `user` 的键集合 `==` `{id, username, email, tenantId, mfaEnabled, mustChangePassword}`
  5. **不产生**第二条 user 行（`SELECT count(*) FROM platform_iam_user WHERE tenant_id='tenant-a' AND username='bob'` → `1`）
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-005: SSO 签发的 access token 能通过 RBAC 权限校验，权限集与本地登录同源

- **关联 AC**: AC-S-100
- **前置条件**: `bob` 已被授予 `rbac:role:read`；`tenant-a` 有 2 条权限点
- **测试步骤**:
  1. 完成一次 OIDC 回调，取 `accessToken`
  2. `GET /api/rbac/me/permissions`，头 `Authorization: Bearer <accessToken>` + `x-tenant-id: tenant-a`
  3. `bob` 用本地密码登录一次，取 `permissions`
- **预期结果**:
  1. 步骤 2 状态码 `200`
  2. 步骤 2 返回的权限点数组 **非空**（`len >= 2`）
  3. 步骤 2 的集合 **等于** 步骤 3 的集合（排序后逐项相等）
  4. 步骤 2 的集合 **等于** 回调响应里的 `permissions` 数组
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-006: SSO 会话的 refreshToken 能走完整轮换

- **关联 AC**: AC-S-100
- **前置条件**: 同 TC-013-003
- **测试步骤**:
  1. 完成一次 OIDC 回调，记 `refreshToken` 为 R1
  2. `POST /api/iam/refresh`，body `{"refreshToken":"R1"}`，头 `x-tenant-id: tenant-a`
  3. 用步骤 2 返回的新 `refreshToken`（R2）再刷一次
- **预期结果**:
  1. 步骤 2 状态码 `200`，返回新的 `accessToken`（字符串不等于步骤 1 的 access token）
  2. 步骤 2 返回的新 `refreshToken` `!= R1`
  3. 步骤 3 状态码 `200`
  4. 三次响应的 `user.id` **相同**
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: `POST /api/iam/refresh` 必须带 `x-tenant-id` —— `tenant_context` 中间件不从令牌推断租户
  （`crates/platform-iam/src/http.rs:400`），缺头会返回 `401 TENANT_CONTEXT_MISSING`
  （`crates/platform-iam/src/service.rs:670`）。〔AC 修正 S12〕

### TC-013-007: 会话列表含本次 SSO 会话

- **关联 AC**: AC-S-100
- **前置条件**: `bob` 当前无其它会话（`DELETE FROM platform_iam_session WHERE user_id=<bob>`）
- **测试步骤**:
  1. 完成一次 OIDC 回调
  2. `GET /api/iam/sessions`，头 `Authorization: Bearer <accessToken>` + `x-tenant-id: tenant-a`
- **预期结果**:
  1. 步骤 2 状态码 `200`，body 是 JSON 数组
  2. 数组长度 `== 1`
  3. 该元素的 `id` 等于 access token 的 `sid` claim（可用 `/.well-known/jwks.json` 验签后读 claim）
  4. 该元素的 `authProvider == "oidc"`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: `authProvider` 在当前 `SessionView` 中**不存在**（`service.rs:145-159` 只有
  id/deviceLabel/ip/userAgent/createdAt/lastUsedAt），design §2.2 只加了数据库列、没加视图字段。
  该断言在实现补齐前端字段前必红。〔守卫 S22〕

### TC-013-008: 回调缺 `state` 参数 → 400，且不 500

- **关联 AC**: AC-S-100
- **前置条件**: 同基线
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/callback?code=CODE-BOB-1`（无 `state`）
- **预期结果**:
  1. 状态码 `400`
  2. 响应体 `code` 字段 `== "SSO_OIDC_STATE_INVALID"`
  3. 状态码 `!= 500`，`platform_iam_oidc_state` 行数不变（`0` 增量）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: 17 个错误码里没有「缺参数」这一类，本用例按「缺 state = state 无效」写；
  若 Dev 选了别的码，本用例红 → 迫使 BA 先钉死。〔守卫 S31〕

### TC-013-009: 回调缺 `code` 参数 → 400，且不查 `oidc_state` 的凭据以外字段

- **关联 AC**: AC-S-100
- **前置条件**: 按 TC-013-001 取一个有效 `state`
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/callback?state=<有效 state>`（无 `code`）
- **预期结果**:
  1. 状态码 `400`
  2. 响应体 `code` `== "SSO_OIDC_TOKEN_EXCHANGE_FAILED"`
  3. mock IdP 的 `token` 计数 `== 0`（未发出换 token 请求）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: 与 TC-013-008 一样属 `〔守卫〕`：AC 未定义缺 `code` 的码。

### TC-013-010: 换 token 时用的 `redirect_uri` 取自 `oidc_state`，不是当前配置

- **关联 AC**: AC-S-100
- **前置条件**: 按 TC-013-001 取有效 `state`（落库的 `redirect_uri` 为旧值）
- **测试步骤**:
  1. 把 `sso.oidc.redirectUri` 改成一个**不同**的合法值（`http://127.0.0.1:8080/cb2`）
  2. 用步骤 0 的 `state` 回调
  3. 读 mock IdP 收到的 `/token` 请求表单（`__stats.last_token_form`）
- **预期结果**:
  1. 状态码 `200`（已发出的授权不应因配置变更而失败）
  2. mock 收到的 `redirect_uri` **逐字等于**改配置**之前**的值
     `http://127.0.0.1:8080/api/iam/sso/oidc/callback`
  3. mock 收到的 `grant_type=authorization_code`、`code=CODE-BOB-1`、`client_id=base-portal`
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 依据 design §2.1「`redirect_uri` 回调用，必须与授权时逐字一致」。
  这是「配置热改不能打断在途授权」的可判定形式。

### TC-013-011: 发现文档按 `issuer` 缓存，重复发起不重复拉取

- **关联 AC**: AC-S-100
- **前置条件**: 计数器归零；发现缓存为空（重启服务或调用测试专用 `POST /__cache/flush`）
- **测试步骤**:
  1. 连续 3 次 `GET /api/iam/sso/oidc/authorize`
  2. 读 `__stats.discovery`
- **预期结果**:
  1. 三次均返回 `302`
  2. `discovery == 1`
  3. 三次的 `state` 互不相同（`state` 不复用）
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-012: `scopes` 配置按空格切分透传，且 `openid` 不会被去重丢掉

- **关联 AC**: AC-S-100
- **前置条件**: 把 `sso.oidc.scopes` 改为 `"openid email profile offline_access"`（4 个）
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`
  2. 取 `Location` 的 `scope`，按空格切分
- **预期结果**:
  1. 切分后的数组长度 `== 4`
  2. 集合 `==` `{openid, email, profile, offline_access}`
  3. 顺序与配置字符串一致（`scope` 的首项 `== "openid"`）
- **类型**: 功能
- **优先级**: P2
- **自动化**: 可自动化
- **设计方法**: 正交

### TC-013-129: `https://` IdP 端到端（D123 验证点）—— 证书信任链必须可构造

- **关联 AC**: AC-S-100
- **前置条件**: 
  - mock IdP 以 **https** 起在 `https://127.0.0.1:9413/idp-a`，证书为**测试自签**（SAN 含 `127.0.0.1`），
    自签根证书在夹具目录 `fixtures/tls/test-root.pem`
  - 服务端已按 D123 接入 TLS connector（`hyper-rustls` + `rustls` + `ring`）
- **测试步骤**:
  1. 把 `sso.oidc.issuer` 改为 `https://127.0.0.1:9413/idp-a`
  2. `GET /api/iam/sso/oidc/authorize`（`x-tenant-id: tenant-a`）
  3. 完成回调，取 `accessToken`
  4. 把 `issuer` 改回 `http://127.0.0.1:9410/idp-a`，重跑步骤 2~3
- **预期结果**:
  1. 步骤 2 状态码 `302`，`Location` 指向 `https://…/authorize`
  2. 步骤 3 状态码 `200`
  3. 步骤 4 同样 `302` / `200`（`https_or_http()` 两种都能用，不是二选一）
  4. **实现必须提供让测试根被信任的途径**：或一个 CA 配置键（如 `sso.oidc.caCertPem`），
     或一个仅在测试 feature 下生效的 `with_custom_root(test_root)` 构造。
     若实现只有 `with_webpki_roots()`，步骤 2 会因「自签证书不受信」而变成
     `503 SSO_OIDC_DISCOVERY_FAILED`，本用例红。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 判定表
- **备注**: `〔守卫〕` —— 本条是 AC-S-100 在 rev 2 末新增的期望（原文编号为「预期 8」），
  但 design 给的写法是 `HttpsConnectorBuilder::new().with_webpki_roots()`（`design-summary.md:236`），
  **webpki 根不信任自签证书**，而 §7 的 12 个配置键里没有 CA/根证书项。
  另外「进程内 axum 起 https mock IdP」需要**服务端** TLS（`axum-server` 一类），
  而 D123 批准的是**客户端连接器**（`status.md:72`「破例范围：仅 TLS 连接器」）——
  两件事都不在现有规格里。〔S13 / S14〕
- **编号说明**: 本条是 D123 修订（2026-11-19 23:56）后**追加**的，编号取 129 而不插号为 013，
  以免打乱既有 128 条与 index.md 的追溯矩阵。
