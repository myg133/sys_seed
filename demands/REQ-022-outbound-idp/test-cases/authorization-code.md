# 授权码 + PKCE（AC-S-137、AC-S-138、AC-S-139）

- **覆盖 AC**: AC-S-137（7 条：TC-022-023 ~ 029）、AC-S-138（5 条：TC-022-030 ~ 034）、
  AC-S-139（4 条：TC-022-035 ~ 038）
- **前置基线**:
  - `client-A` 已注册（`redirect_uris = ['https://client.example/cb']`，
    `allowed_scopes = ['rbac:role:read','rbac:role:write']`，
    `allowed_grant_types` 含 `authorization_code`）
  - `tenant-a` 下用户 `u-perm-read@example.com`（持 `rbac:role:read`）+ 密码 `Tr0ub4dor&3xKq`
  - `base serve` 已起；`apps/shell/dist` 已构建（Playwright 的 `webServer` 会先 build）
- **实现事实（用例按此写）**:
  - **授权页是前端 SPA 路由**（D203）：`/oauth/authorize` → 静态回落 `index.html`
    （`platform-server/src/static_files.rs:36-38` 的 `is_api_path` 只认 `/api` 与 `/api/**`
    → **`/oauth/...` 会回落成 SPA**；D180 的「`/api/...` 必须 404」**不适用**于 `/oauth/*`）
  - 平台会话 = **Bearer + `sessionStorage`**（键 `base.shell.session.v1`，`apps/shell/src/lib/session.ts:25`）
    → 浏览器顶层导航**不带** `Authorization` 头 → 授权请求必须由前端路由承接
  - 平台**没有 CORS 层**（`playwright.config.ts:3-11`）→ E2E 走同源
  - PKCE 校验口径：`base64url(SHA256(code_verifier)) == code_challenge`
    （复用 `crates/platform-iam/src/sso/oidc.rs` 的 `code_challenge()`，D206 §3）

---

## AC-S-137: 授权码 + PKCE 能换到可用令牌（端到端）

### TC-022-023: 授权页能接收 URL 的 query 参数

- **关联 AC**: AC-S-137
- **前置条件**: `base serve` 已起；`apps/shell/dist` 已构建
- **测试步骤**:
  1. `GET /oauth/authorize?response_type=code&client_id=client-A&redirect_uri=https%3A%2F%2Fclient.example%2Fcb&scope=rbac%3Arole%3Aread&state=xyz&code_challenge=abc&code_challenge_method=S256`
  2. 读响应的 `Content-Type` 与 `Content-Length`
  3. 浏览器打开该 URL，读 `window.location.search`
- **预期结果**:
  1. HTTP **200**，`Content-Type` 含 `text/html`（**回落成 SPA**，不是 404）
  2. 响应的 body 是 SPA 的 `index.html`（含 `<div id="app">` 之类的挂载点）
  3. 浏览器里 `new URLSearchParams(location.search).get('state')` **逐字等于** `xyz`，
     `get('client_id')` 逐字等于 `client-A`，`get('code_challenge')` 逐字等于 `abc`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 这一条验的是「SPA 路由能不能拿到 URL query 参数」。
  `router.ts:232-235` 只在 **path 不变**时保留 `search`，所以页面必须自己读 `window.location.search`
  （`sso-api.ts:244-256` 有现成范式）→ **不能**指望路由把 query 传进 `load()`。

### TC-022-024: 未登录 → 跳登录页，且 **query 完整保留**（`return_to`）

- **关联 AC**: AC-S-137
- **前置条件**: 浏览器**无**会话（`sessionStorage` 为空）
- **测试步骤**:
  1. 打开 `/oauth/authorize?client_id=client-A&state=xyz&code_challenge=abc&code_challenge_method=S256&redirect_uri=…&scope=…&response_type=code`
  2. 读浏览器最终地址栏的 query
  3. 在登录页完成登录
  4. 读浏览器最终地址栏
- **预期结果**:
  1. 步骤 2：地址栏的 pathname 是 `/login`（或带 `return_to` 的 `/login`），
     且 **`return_to`（或等价参数）解码后逐字包含** `client_id=client-A`、`state=xyz`、
     `code_challenge=abc`、`redirect_uri=https%3A%2F%2Fclient.example%2Fcb`
  2. 步骤 4：地址栏回到 `/oauth/authorize`，且 **query 与步骤 1 逐字相同**
     （`state` 仍为 `xyz`、`code_challenge` 仍为 `abc`）
  3. **反向对照**：不带 query 打开 `/oauth/authorize` → 也落 `/login`，不报错、不白屏
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移 + 阳性对照
- **备注**: 〔S-02〕**物理不可达** —— 平台**没有** `return_to` 机制：
  `packages/auth/src/index.ts:118-132` 的 `guard()` 只给 `'/login'`；
  `apps/shell/src/lib/router.ts:229-242` 的 `write()` 在 path 变化时**丢弃 query**；
  `router.ts:261` 的 `intendedPath` 只有 pathname；`App.svelte:120-125` 登录后跳的
  `intendedPath` **不含 query** → 照现状，**登录后 OAuth 参数全部消失**，本用例**必红**。
  → **BA 必须先选方案**（公开路由 + 登录页 `return_to` / 改 `router.ts` 的 `write()`），
  否则 AC-S-137 整组都无法执行。

### TC-022-025: 同意页逐字展示 client 名与**全部**请求权限点

- **关联 AC**: AC-S-137
- **前置条件**: 已登录 `u-perm-read@example.com`；浏览器会话已建立
- **测试步骤**:
  1. 打开授权 URL（`scope=rbac:role:read`）
  2. 等同意页渲染完成
  3. 读页面文本
- **预期结果**:
  1. 页面出现 client 的 `name`（逐字）
  2. 页面文本**逐字出现** `rbac:role:read`
  3. 页面**不含** `rbac:role:write`（未请求的权限点不得出现）
  4. 页面有「允许」与「拒绝」两个可点入口
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 这是 D204 的**用户所见**那一半；「实际授予」那一半在 TC-022-076。

### TC-022-026: 点「允许」后跳向 `redirect_uri`，query 里带 `code` 与 `state`

- **关联 AC**: AC-S-137
- **前置条件**: 同 TC-022-025；**先拦截第三方回调**
- **测试步骤**:
  1. `page.route('https://client.example/**', route => route.fulfill({status:200, contentType:'text/html', body:'<html>ok</html>'}))`
  2. 走完授权流程并在同意页点「允许」
  3. 读 `page.url()`
- **预期结果**:
  1. `page.url()` 的 origin 逐字等于 `https://client.example`
  2. `new URL(page.url()).searchParams.get('code')` **非空**（且长度 ≥ 43）
  3. `new URL(page.url()).searchParams.get('state')` **逐字等于** `xyz`
  4. `new URL(page.url()).pathname` 逐字等于 `/cb`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径 + 拦截
- **备注**: 〔S-01〕**物理不可达** —— `https://client.example/cb` **不可达**，
  顶层导航会 DNS 失败 → `page.url()` 停在跳转前 → **AC 步骤 4「读浏览器最终地址栏的 code」按字面不可执行**。
  → **必须**用 `page.route` 拦截（仓里的既有范式：`e2e/sso.spec.ts:43-50`），
  拦截后地址栏**就是**目标 URL，`page.url()` 可读。

### TC-022-027: `code` + `code_verifier` 换到令牌，响应形状与 `scope` 正确

- **关联 AC**: AC-S-137
- **前置条件**: TC-022-026 拿到 `code`；已知 `code_verifier`（测试侧生成，
  `code_challenge = base64url(SHA256(code_verifier))`）
- **测试步骤**:
  1. `POST /api/oauth/token`，`Content-Type: application/x-www-form-urlencoded`，
     body `grant_type=authorization_code` / `code=<code>` / `client_id=client-A` /
     `client_secret=<明文>` / `redirect_uri=https://client.example/cb` / `code_verifier=<verifier>`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. 响应体含 `access_token` / `token_type` / `expires_in` / `scope` 四个键
  3. `token_type` **逐字等于** `"Bearer"`
  4. `scope` **逐字等于** `rbac:role:read`（**恰好 1 个**，不含 `rbac:role:write`）
  5. `access_token` 是 **3 段** base64url（`header.payload.signature`），以 `.` 分隔
  6. 响应体**不含** `refresh_token` 键（D217）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 第 6 条钉住 D217（「本轮不发 refresh token」）——
  AC-S-134 的元数据也声明了不含 `refresh_token`，两处一致。

### TC-022-028: 换到的令牌能内省，且 `scope` 含请求的权限点

- **关联 AC**: AC-S-137
- **前置条件**: TC-022-027 拿到 `access_token`
- **测试步骤**:
  1. `POST /api/oauth/introspect`，带 `client-A` 的 `client_secret_basic` 认证，body `token=<access_token>`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. `active` 逐字等于 `true`（**布尔**，不是字符串 `"true"`）
  3. `scope` **包含** `rbac:role:read`
  4. `client_id` 逐字等于 `client-A`
  5. `sub` 存在且**等于** `u-perm-read@example.com` 的用户 id
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 步骤 7 的原文。**必须带 client 认证**（D208）→ 否则测的是另一件事。

### TC-022-029: `state` 原样回显（防 CSRF）

- **关联 AC**: AC-S-137
- **前置条件**: 同 TC-022-025
- **测试步骤**:
  1. 授权请求用 `state=xyz`
  2. 读最终地址栏的 `state`
  3. 换一组 `state`（含 URL 保留字符，如 `a b&c=d`）重复步骤 1~2
- **预期结果**:
  1. 步骤 2：回显的 `state` **逐字等于** `xyz`
  2. 步骤 3：回显的 `state` **逐字等于** `a b&c=d`（**URL 编码后取值相等**，不做任何改写）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: AC 只钉了 `xyz` 一个值 → 无区分力（任何实现都原样回显）。
  加一个**含保留字符**的 `state` 才测得到「有没有做改写 / 截断」。

---

## AC-S-138: PKCE 强制 S256，缺挑战或方法不支持一律拒

### TC-022-030: 缺 `code_challenge` → **preview** 就 400 `OAUTH_INVALID_REQUEST`

- **关联 AC**: AC-S-138
- **前置条件**: 已登录 `u-perm-read@example.com`
- **测试步骤**:
  1. `POST /api/oauth/authorize/preview`，body **不含** `code_challenge`
     （其余：`client_id=client-A`、`redirect_uri=https://client.example/cb`、
     `response_type=code`、`scope=rbac:role:read`、`state=xyz`）
  2. 读响应体
  3. 查 `platform_oauth_code` 的行数
- **预期结果**:
  1. HTTP **400**
  2. 响应体是 Problem Details，`code` **逐字等于** `OAUTH_INVALID_REQUEST`，
     `status == 400`，`Content-Type: application/json`
  3. 响应体**不含** `ticket` 键
  4. 步骤 3：授权码表行数 **== 0**（**不得**签出任何 code）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-09〕〔AC 修正〕 —— AC 原文说「**签发 code 这一步**就失败」，
  但按 D203 参数校验在 **preview**；**在 confirm 才判 = 把参数错误推迟到用户点完「允许」之后**。
  本用例把失败点钉在 preview。

### TC-022-031: `code_challenge_method=plain` → **preview** 就 400 `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD`

- **关联 AC**: AC-S-138
- **前置条件**: 同 TC-022-030
- **测试步骤**:
  1. `POST /api/oauth/authorize/preview`，body 带 `code_challenge=<verifier 本身>` +
     `code_challenge_method=plain`
  2. 读响应体
- **预期结果**:
  1. HTTP **400**
  2. `code` **逐字等于** `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD`
  3. 响应体**不含** `ticket` 键
  4. `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` 与 `OAUTH_INVALID_REQUEST` 是**两个不同的码**
     （本用例断言的是前者，不是后者）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-09〕—— AC 把两个错误码都列了，但**没说各自在哪个端点、以什么错误体返回**。
  本用例钉「preview + 平台话 + 各自的码」。

### TC-022-032: `code_challenge_method` 缺失（但有 `code_challenge`）→ 400

- **关联 AC**: AC-S-138
- **前置条件**: 同 TC-022-030
- **测试步骤**:
  1. `POST /api/oauth/authorize/preview`，body 带 `code_challenge=<S256(verifier)>`，
     **不带** `code_challenge_method`
  2. 读响应体
- **预期结果**:
  1. HTTP **400**
  2. `code` **逐字等于** `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD`（**不是** `OAUTH_INVALID_REQUEST`）
     —— 缺 method ≠ 缺 challenge，两者**必须可区分**
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（缺省）
- **备注**: RFC 7636 说缺 method 时**默认 `plain`**；本平台只接受 `S256` →
  缺 method **必须**被拒（等价于 `plain`）。AC 只测了「显式 plain」，
  **没测「缺 method」** → 这是最容易漏的一条（D206 §1：「`code_challenge_method` 只接受 `S256`」）。

### TC-022-033: `code_verifier` 不匹配 → **token** 端点 400 `invalid_grant`（协议话）

- **关联 AC**: AC-S-138
- **前置条件**: 已用 `code_challenge = S256("wrong-verifier")` 走完 preview + confirm，拿到 `code`
- **测试步骤**:
  1. `POST /api/oauth/token`，body `grant_type=authorization_code` / `code=<code>` /
     `client_id=client-A` / `client_secret=<明文>` / `redirect_uri=https://client.example/cb` /
     `code_verifier=right-verifier`
  2. 读响应体
- **预期结果**:
  1. HTTP **400**
  2. 响应体**逐字等于** `{"error":"invalid_grant"}`（**只含** `error` 键；**不含**
     `type`/`title`/`status`/`instance`/`code`/`trace_id`）
  3. 响应体**不含**任何 `OAUTH_*` 平台码
  4. `error_description`（若存在）**不含** `code_verifier` / `code_challenge` / `client_secret`
     的明文，且**不含任何 CJK 字符**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-30〕〔AC 修正〕 —— AC 原文「`error_description` **不含**「哪一步错了」的细节以外的信息」
  句义歧义且不可机械判定 → 本用例改成两条可判断言（子串断言 + CJK 断言，契约 C14 §14.2）。
  第 2 条还钉住了「协议端点说协议话」。

### TC-022-034: 失败的授权**不产生任何** code 行

- **关联 AC**: AC-S-138
- **前置条件**: TC-022-030 ~ 032 各失败一次
- **测试步骤**:
  1. 以 `base` 直连 `SELECT count(*) FROM platform_oauth_code`
  2. 再以 `base_app` 在事务内 `set_config('app.tenant_id','tenant-a')` 后查同一张表
- **预期结果**:
  1. 步骤 1 的计数 == 步骤 2 的计数
  2. 该计数 == **0**（这三条路径一条 code 都不该留下）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: 「**不得**签出任何 code」是 AC 的原文，但 AC **没说在哪查**。
  这里同时用 `base`（超管）与 `base_app`（受 RLS）查 —— 两者必须一致，
  否则「0 行」可能只是 RLS 把它藏起来了（假绿）。

---

## AC-S-139: 授权码一次性、只存摘要、60 秒过期

### TC-022-035: 库里存的是 `code` 的摘要，**没有明文**

- **关联 AC**: AC-S-139
- **前置条件**: 已通过授权流程拿到**未兑换**的 `code`
- **测试步骤**:
  1. 以 `base` 直连查授权码表（用**摘要匹配**）
  2. 测试侧**独立实现** SHA-256 → **base64url 无填充**，对明文 `code` 求摘要
- **预期结果**:
  1. 库里存在一行，其 `code_hash` **逐字节等于**步骤 2 的计算结果
  2. 库里**任何一列**都**不等于**明文 `code`（`code_hash` 列除外）
  3. 全表 `SELECT count(*) FROM platform_oauth_code WHERE code_hash = '<明文 code>'` **== 0**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 独立复算 + 否定断言
- **备注**: 〔S-16〕**口径钉死** —— AC 原文只写「`SHA-256(code)` 摘要」，
  仓里有 `digest_base64url`（base64url）与 `digest_hex`（hex）两个口径 → 本用例按 **base64url 无填充**。
  第 3 条是阳性对照（证明扫的是真东西）。

### TC-022-036: 同一个 `code` 第二次兑换 → 400 `invalid_grant`，且行被**删除**

- **关联 AC**: AC-S-139
- **前置条件**: 已拿到**未兑换**的 `code` 与正确的 `code_verifier`
- **测试步骤**:
  1. 第一次 `POST /api/oauth/token`（`grant_type=authorization_code` + 正确 verifier）
  2. **立刻**用**同一个** `code` + **同一个** `code_verifier` 再兑换一次
  3. 以 `base` 直连 `SELECT count(*) FROM platform_oauth_code WHERE code_hash = <摘要>`
- **预期结果**:
  1. 步骤 1：HTTP **200**，拿到令牌
  2. 步骤 2：HTTP **400**，响应体**逐字等于** `{"error":"invalid_grant"}`
  3. 步骤 3：计数 **== 0**（**行已被删除**，不是置一个 `consumed_at` 标记）
  4. 该表的列集合里**不含** `consumed_at` / `used_at`（查 `information_schema.columns`）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（可用 → 已消费）
- **备注**: 〔S-32〕〔AC 修正〕 —— AC 步骤 4 写「查授权码表的**行数**」（整表？该行？）→
  本用例钉「**按摘要查该 code 的行数 == 0**」。第 4 条补上「不是标记位」这条否定断言的**对象**
  （design §2.3 的列集合里本来就没有 `consumed_at`）。

### TC-022-037: TTL —— 过期 90 秒的码换不到令牌，未过期的换得到

- **关联 AC**: AC-S-139
- **前置条件**: 已知 `client-A` / `u-perm-read@example.com` / `code_verifier` 的对应关系
- **测试步骤**:
  1. 以 `base` 直连 `INSERT` 一行 `platform_oauth_code`：
     `tenant_id='tenant-a'`、`client_id='client-A'`、`user_id=<u-perm-read 的 id>`、
     `code_hash=<base64url(SHA256('ttl-expired-code'))>`、
     `redirect_uri='https://client.example/cb'`、`scope='rbac:role:read'`、
     `code_challenge=<S256(verifier)>`、`code_challenge_method='S256'`、
     `expires_at = now() - interval '90 seconds'`
  2. 用 `code='ttl-expired-code'` + 正确的 `code_verifier` 兑换 → 读响应
  3. **只改 `expires_at`** 为 `now() + interval '30 seconds'`（其余列逐字相同），再兑换一次 → 读响应
- **预期结果**:
  1. 步骤 2：HTTP **400**，响应体**逐字等于** `{"error":"invalid_grant"}`
  2. 步骤 3：HTTP **200**，拿到令牌（`scope` 逐字等于 `rbac:role:read`）
  3. 两行的**其余列逐字相同**（用 `SELECT` 回读比对）→ 证明判的是 TTL，不是别的
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（过期 / 未过期）
- **备注**: 〔S-31〕〔AC 修正〕 —— AC 原文「造一个 `expires_at` 在 90 秒前的授权码」**没写要插哪些列**；
  少插绑定列 → 交换会先卡在绑定校验上 → **判据失真**（测到的不是 TTL）。
  本用例把**全部绑定列**逐字写死，并要求「两次只差 `expires_at`」。

### TC-022-038: 授权码的 TTL 配置是 60 秒（默认）

- **关联 AC**: AC-S-139
- **前置条件**: 配置 `oauth.authorizationCodeTtlSeconds` 为默认或已知值
- **测试步骤**:
  1. 走完授权流程拿到 `code`
  2. 以 `base` 直连查该行的 `expires_at`
  3. 与 `issued` 侧的时间基准比对（`platform_oauth_code` 无 `created_at` 时用**本次请求时间**）
- **预期结果**:
  1. `expires_at` **非空**
  2. `expires_at` 与签发时刻的差 **落在 [55, 90] 秒**区间（默认 60 秒，允许调度抖动）
  3. 把配置改成 `120` 后重跑 → 差值落在 **[115, 150] 秒**（**不是**硬编码 60）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 边界值
- **备注**: D205 §2：「TTL **60 秒**（RFC 6749 §4.1.2 建议「尽可能短，建议 10 分钟以内」→ 取 60 秒）」。
  第 3 条要改配置 + 重启 → **半自动**。区间取 [55, 90] 是为了容忍时钟/调度抖动，
  但**不能**宽到 3600（否则判不出）。
