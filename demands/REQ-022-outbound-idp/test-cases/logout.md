# 单点登出（AC-S-149、AC-S-150）

- **覆盖 AC**: AC-S-149（7 条：TC-022-091 ~ 097）、AC-S-150（5 条：TC-022-098 ~ 102）
- **前置基线**:
  - `client-A`：`backchannel_logout_uri = http://127.0.0.1:$MOCK_PORT/logout`，
    `frontchannel_logout_uri = https://client.example/logout`
  - `client-B`：**没有**任何 logout URI
  - `tenant-a` 下 `u-perm-read@example.com` 已对 `client-A` **与** `client-B` 授权
  - **mock 接收端**：`scripts/mock-logout-receiver.sh` 起的独立进程，
    监听 `127.0.0.1:$MOCK_PORT`，把收到的 `logout_token` 追加写到
    `target/mock-logout-received.jsonl`；行为由 `MOCK_MODE=ok|timeout|500` 控制
- **实现事实（用例按此写）**:
  - 平台是**独立进程**（`scripts/e2e-server.sh:88` `exec cargo run … base -- serve`）
    → 出站 back-channel **必须**打到**另一个进程** → mock 端**必须**是另一个进程
  - D211 §2：back-channel POST **form 编码**、**超时 5 秒**、**失败只记日志、不影响登出**
  - D211 §1：`logout_token` = ES256 JWT，claims = `iss` / `aud`(=client_id) / `iat` / `jti` /
    `sid` / `events` / `sub` / `tid`；**不含** `nonce`
  - design §4.5：全局登出 = 撤销全部 grant → 作废全部会话 → 并发投递 back-channel →
    收集 front-channel → 埋审计 `oauth.logout.executed`

---

## AC-S-149: Back-Channel Logout 能向 client 投递 `logout_token`

### TC-022-091: mock 接收端收到 **form 编码**的 `logout_token`

- **关联 AC**: AC-S-149
- **前置条件**: mock 接收端在跑（`MOCK_MODE=ok`）；`client-A` 的 `backchannel_logout_uri` 已指向它；
  该用户已对 `client-A` 授权
- **测试步骤**:
  1. 触发全局登出（`POST /api/oauth/logout`，带用户会话）
  2. 等 mock 把收到的内容落盘
  3. 读 `target/mock-logout-received.jsonl` 的最后一行
- **预期结果**:
  1. mock **收到 1 次**请求
  2. 该请求的 `Content-Type` 逐字等于 `application/x-www-form-urlencoded`
  3. 该请求的 body **含** `logout_token=` 参数，且其值是一个**非空** JWT（3 段 base64url）
  4. 登出响应本身是 **HTTP 200**
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 〔S-07〕**前置不可达（当前）** —— AC 说「**进程内**的 mock 接收端」，
  但平台是**另一个进程** → mock 必须是**独立进程**（仓里**没有**现成范式 → 要新增脚本）。
  本用例的 mock 是**真 HTTP 接收端**（不是 `page.route` 那种浏览器拦截）——
  否则测的是「前端发了个请求」，不是「平台出站真的投递了」。

### TC-022-092: `logout_token` 的 payload：claims 齐全、`aud` = client_id、**无** `nonce`

- **关联 AC**: AC-S-149
- **前置条件**: 同 TC-022-091（已拿到 `logout_token`）
- **测试步骤**:
  1. 测试侧**独立解析** `logout_token` 的 payload（base64url 解第 2 段 → JSON）
  2. 逐个读 claim
- **预期结果**:
  1. 解析成功
  2. `iss` 逐字等于 `BASE_JWT_ISSUER`
  3. `aud` 逐字等于 `client-A` 的 `client_id`
  4. `sub` / `sid` / `jti` / `iat` / `tid` **全部存在**（非空）
  5. `events` 是对象，且**含**键 `http://schemas.openid.net/event/backchannel-logout`
  6. **不含** `nonce` 键（D211）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: AC 步骤 3 的原文。第 6 条是 D211 的**显式**要求 → 必须断言**键不存在**。

### TC-022-093: `logout_token` 的签名**验签通过**（测试侧独立验签）

- **关联 AC**: AC-S-149
- **前置条件**: 同 TC-022-091；平台 JWKS 可取
- **测试步骤**:
  1. `GET /.well-known/jwks.json` → 取公钥
  2. 测试侧**独立实现** ES256 验签（**不调用生产代码**）验 `logout_token`
- **预期结果**:
  1. JWKS 的 `keys[]` 里能按 `kid` 找到该令牌的公钥
  2. 验签**通过**
  3. 令牌的 header `alg` 逐字等于 `ES256`
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 独立复算
- **备注**: AC 只说「测试侧独立解析 payload」，**没要求验签** → 补上。
  第三方是**靠验签**信任这个令牌的（不验签 = 只信了一个自签的 JSON）。

### TC-022-094: mock **超时** → 登出**仍然成功**（HTTP 200），授权已撤销

- **关联 AC**: AC-S-149
- **前置条件**: mock 以 `MOCK_MODE=timeout` 运行（**不响应**）；重新授权一次
- **测试步骤**:
  1. 触发全局登出 → 读响应
  2. 查 `platform_oauth_grant`（该用户的全部行）
  3. 查 mock 的落盘文件
- **预期结果**:
  1. 步骤 1：HTTP **200**（**登出本身成功**）
  2. 步骤 2：该用户的**全部** grant 行 `status` 逐字等于 `revoked`
  3. 步骤 3：mock **没有**收到新的 `logout_token`（超时 → 连接被放弃）
  4. 登出响应**不含**任何 `OAUTH_*` 错误码
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: D211 §2：「**超时 5 秒**，**失败只记日志、不影响登出主流程**（登出必须成功）」。
  第 1 条的**耗时**也要断言：整个登出请求应在 **< 15 秒**内返回（5 秒超时 + 余量）
  → 否则「超时」会变成「挂死」。

### TC-022-095: mock 返回 **500** → 登出**仍然成功**，且**有日志**

- **关联 AC**: AC-S-149
- **前置条件**: mock 以 `MOCK_MODE=500` 运行；重新授权一次
- **测试步骤**:
  1. 触发全局登出 → 读响应
  2. 查 `platform_oauth_grant`
  3. 读平台日志（`BASE_LOG_LEVEL` 至少 `info`）
- **预期结果**:
  1. 步骤 1：HTTP **200**
  2. 步骤 2：全部 grant `status == 'revoked'`
  3. 步骤 3：日志里**含**该 `clientId`，且**含**失败原因（如 `500` / `status`）
  4. 步骤 3：日志里**不含** `logout_token` 的**明文**（不泄漏一次性凭据）
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: AC 的「补充判据」：「投递失败时**有日志**（含 `clientId` 与失败原因）」。
  第 4 条是 REQ-014 式的敏感值纪律（REQ-014 AC-S-133 的同款要求）。

### TC-022-096: 只有**配了** `backchannel_logout_uri` 的 client 收到通知

- **关联 AC**: AC-S-149
- **前置条件**: 该用户已对 `client-A`（**有** backchannel URI）**与** `client-B`（**没有**）授权
- **测试步骤**:
  1. 触发全局登出
  2. 读 mock 的落盘文件 → 取出全部 `logout_token`
  3. 逐个解 `aud`
- **预期结果**:
  1. mock 收到的请求数 **== 1**（**只有** `client-A`）
  2. 该令牌的 `aud` 逐字等于 `client-A`
  3. **没有任何**令牌的 `aud` 逐字等于 `client-B`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 阳性对照
- **备注**: AC 没写这条 → 补上（**收集逻辑**必须按 `backchannel_logout_uri` 非空过滤，
  否则会向没配 URI 的 client 发空请求）。

### TC-022-097: `logout_token` 的 `sid` 逐字等于**触发登出的会话** id

- **关联 AC**: AC-S-149
- **前置条件**: `u-perm-read@example.com` 已登录（当前 access token 的 `sid` 已知）
- **测试步骤**:
  1. 触发全局登出
  2. 读 mock 收到的 `logout_token` → 解 `sid`
- **预期结果**:
  1. `sid` **非空**
  2. `sid` **逐字等于**触发登出的那次登录 access token 的 `sid` claim
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: 〔S-24〕〔AC 修正〕 —— D211 写了 `sid` 但**没定多会话时取哪个**；
  平台的用户会话是**一表多行**（`platform_iam_session` 带 `family_id` / `revoked_at`，
  **实测 277 行**）→ 多会话是常态。本用例钉「取**触发登出的这次**会话」这一口径。
  若 BA 决定「按会话逐个发」，本用例要改成「`sid` 集合 == 该用户的全部会话 id 集合」。

---

## AC-S-150: Front-Channel Logout 与按应用登出可用

### TC-022-098: 全局登出后 `client-A` 的授权被撤销、`client-B` 的**也**被撤销

- **关联 AC**: AC-S-150
- **前置条件**: 该用户已对 `client-A` 与 `client-B` 授权
- **测试步骤**:
  1. 触发全局登出
  2. 查 `platform_oauth_grant` 里该用户的**全部**行
- **预期结果**:
  1. 该用户的**全部** grant 行 `status` 逐字等于 `revoked`
  2. `client-A` 与 `client-B` 的行**都在**其中
  3. **没有**任何一行仍是 `active`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 〔S-26〕〔AC 修正〕 —— AC 步骤 2 说「两个 client 的授权**都**被撤销」，
  但步骤 1 已经是**全局**登出 → 这条是**必然后果**，**无区分力**。
  本用例补上第 3 条（没有残留 `active`）与 TC-022-099（通知只给配了 URI 的那个）。

### TC-022-099: front-channel 通知**只**包含 `client-A`

- **关联 AC**: AC-S-150
- **前置条件**: 同 TC-022-098
- **测试步骤**:
  1. 触发全局登出 → 读响应
  2. 按 BA 拍板的形态读 front-channel 载荷
     （**选项 A**：HTML 里的 `<iframe>`；**选项 B**：JSON 的 `frontChannel` 数组）
- **预期结果**:
  1. 载荷里**恰好 1 条**（**只有** `client-A`）
  2. 该条的 `clientId` 逐字等于 `client-A`
  3. 该条的 `logoutUrl` / `iframe src` 的 **origin + path** 逐字等于
     `client-A.frontchannel_logout_uri` 的 origin + path
     （**允许**附加 `?logout_token=…`）
  4. **不含** `client-B` 的 `clientId` 或 URL（**逐字不含** `client-B` 的 host）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: 〔S-20〕〔AC 修正〕 —— AC 写「响应体里 `frontChannel` **数组**」，
  而 design §4.5 写「返回 **HTML**，内含 `<iframe>`」→ **两者矛盾** → 本用例按
  「BA 拍板的形态」写（两种形态的判据已并列）。第 4 条钉住「**只**给配了 URI 的 client」。

### TC-022-100: front-channel iframe 带 `sandbox`，响应带 `X-Frame-Options`

- **关联 AC**: AC-S-150
- **前置条件**: 同 TC-022-098；**BA 已选定「HTML + iframe」形态**
- **测试步骤**:
  1. 触发全局登出 → 读响应头与响应体
  2. 解析 HTML 里的 `<iframe>`
- **预期结果**:
  1. 响应头 `X-Frame-Options` 逐字等于 `DENY`
     （**或**响应头 `Content-Security-Policy` 含 `frame-ancestors 'none'` —— 二者其一，**不是**都没有）
  2. 每个 `<iframe>` **必须**带 `sandbox` 属性
  3. `sandbox` 的值**不含** `allow-same-origin`（否则等于没隔离）
  4. `<iframe>` 的 `src` 以 `https://client.example/logout` 开头
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: 〔S-20〕〔AC 修正〕 —— design §4.5 提了「必须加 `sandbox` 属性与 `X-Frame-Options` 处理」，
  **AC 一个字都没提** → 安全项**无判据**。第 3 条钉住「`allow-same-origin` 会让 sandbox 失效」。
  ⚠️ 若 BA 选「JSON 数组」形态，本用例要改成**前端**用例（渲染后的 DOM）。

### TC-022-101: **按应用登出**只撤销 `client-A`，`client-B` **仍**是 `active`

- **关联 AC**: AC-S-150
- **前置条件**: 该用户已对 `client-A` 与 `client-B` 授权；**重新授权**过（AC 步骤 3 的前置）
- **测试步骤**:
  1. `POST /api/oauth/grants/revoke`，body `{clientId:"client-A"}`（带用户会话）
  2. 查 `platform_oauth_grant` 里该用户的**全部**行
- **预期结果**:
  1. 步骤 1：HTTP **200**
  2. `client-A` 的行 `status` 逐字等于 `revoked`
  3. `client-B` 的行 `status` **仍是** `active`
  4. `client-B` 的 `revoked_at` **仍是 NULL**
  5. `client-B` 的令牌仍可内省为 `active=true`（**按应用登出不影响别的 client**）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 阳性对照
- **备注**: AC 步骤 3~4 的原文（第 5 条是补的**阳性对照**——否则「client-B 还是 active」
  可能只是「它本来就没授权过」）。

### TC-022-102: 全局登出后**已签发的登录 access token** 仍然有效（现状钉死）**或**立即失效（BA 选 B）

- **关联 AC**: AC-S-149、AC-S-150
- **前置条件**: 登出前拿到一枚**登录** access token（`platform-iam` 的那种，**不是** OAuth 令牌）
- **测试步骤**:
  1. 触发全局登出
  2. 用**登出前**的登录 access token 调 `GET /api/rbac/me/permissions`
- **预期结果**:
  1. **选项 A（BA 选了「立即失效」）**：HTTP **401**
  2. **选项 B（BA 承认现状）**：HTTP **200**，且用例备注写明
     「登出**只**撤销 OAuth 授权与 refresh token；已签发的登录 access token
     最长延迟一个 `iam.accessTokenTtl` 秒失效」——**且该事实必须同时写进 D211 与元数据**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 决策表（BA 拍板二选一）
- **备注**: 〔S-25〕**安全缺口** —— D211 说全局登出要「作废**全部会话**」，
  但实现上引用的 `revoke_family` / `revoke_all_refresh_tokens`
  （`platform-iam/src/store.rs:754-778`）**只** UPDATE `platform_iam_refresh_token`；
  而 access token 是**自包含 JWT**（验签不查库）→ **登出后旧 token 仍可用到过期**。
  AC 没断言 → **既没实现、也没验收**。**BA 必须明写选 A 还是 B**，不许默认。
