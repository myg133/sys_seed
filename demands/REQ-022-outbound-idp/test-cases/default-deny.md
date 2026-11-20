# 底座业务端点默认拒绝 OAuth 令牌（AC-S-151）

- **覆盖 AC**: AC-S-151（6 条：TC-022-103 ~ 108）
- **前置基线**:
  - `client-A` 持一枚**有效**的 OAuth access token（含 `rbac:role:read` scope）
  - `u-perm-read@example.com` 持有一枚**正常登录令牌**（`platform-iam` 签发的那种）
  - 服务已起；`tenant-a` 有审计数据
- **实现事实（用例按此写）**:
  - 平台的验签**强制校验 `aud`**：`validation.validate_aud = true`，
    `aud = {平台 audience}`（`platform-iam/src/jwt.rs:152-156`）
    → OAuth 令牌的 `aud` 是 `client_id` → **必然验签失败**
  - **第二道防线**：`AccessClaims` 的 `sid` 是**必填**字段（`platform-iam/src/jwt.rs:30-61`），
    而 OAuth 令牌的 claims 清单（D198 §4）**没有 `sid`** → 反序列化也失败
  - 业务端点的 401 = `UNAUTHENTICATED`（`platform-iam/src/error.rs:56`、`:359`）；
    `IamTokenVerifier`（`platform-server/src/audit.rs:106-117`）把任何错误都映射成
    `TokenVerifyError::Invalid` → **不泄露原因**
  - 契约 C14 §14.2 的协议码集合里**没有** `invalid_token`
    （那是 RFC 6750 的资源服务器码，本契约未登记）

---

## AC-S-151: 底座业务端点默认拒绝第三方 OAuth 令牌

### TC-022-103: OAuth 令牌调 `GET /api/rbac/me/permissions` → 401 + `UNAUTHENTICATED`

- **关联 AC**: AC-S-151
- **前置条件**: `client-A` 持有效 OAuth access token（含 `rbac:role:read`）
- **测试步骤**:
  1. `GET /api/rbac/me/permissions`，带 `Authorization: Bearer <OAuth access_token>`
  2. 读响应体
- **预期结果**:
  1. HTTP **401**
  2. 响应体是 Problem Details，`code` **逐字等于** `UNAUTHENTICATED`，`status == 401`
  3. 响应体**不含** `error` 键（业务端点说**平台话**，不是协议话）
  4. 响应体**不含** `permissions` 键（**不泄露**该令牌带的 scope）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: 〔S-05〕〔AC 修正〕 —— AC 原文写「**401**（`UNAUTHENTICATED` 或协议错误 `invalid_token`——
  **二者都接受**）」→ 两个取值都算过 = **判不出红绿**；
  且 `invalid_token` **不在**契约 C14 §14.2 的协议码集合里。
  本用例钉**唯一正确**的形态。

### TC-022-104: OAuth 令牌调 `GET /api/audit/events` → 401 + `UNAUTHENTICATED`

- **关联 AC**: AC-S-151
- **前置条件**: 同 TC-022-103
- **测试步骤**:
  1. `GET /api/audit/events`（带 `x-tenant-id: tenant-a`），带 `Authorization: Bearer <OAuth access_token>`
  2. 读响应体
- **预期结果**:
  1. HTTP **401**
  2. `code` 逐字等于 `UNAUTHENTICATED`
  3. 响应体**不含** `items` 键
  4. 响应体**不含**任何审计记录内容
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: AC 步骤 2 的原文。选审计端点是因为它**带租户上下文** → 顺带排除
  「因为缺租户头才 401」的假绿（本用例**带了** `x-tenant-id`）。

### TC-022-105: **对照** —— 正常登录令牌调同两个端点 → **成功**（阳性对照）

- **关联 AC**: AC-S-151
- **前置条件**: 持有 `u-perm-read@example.com` 的**登录** access token
- **测试步骤**:
  1. `GET /api/rbac/me/permissions`，带登录令牌 → 读响应
  2. `GET /api/audit/events`，带登录令牌 + `x-tenant-id: tenant-a` → 读响应
- **预期结果**:
  1. 步骤 1：HTTP **200**，响应体含 `permissions` 键
  2. 步骤 2：HTTP **200**（`audit:events:read` 未授 → 可能是 403，
     但**必须不是 401**；本用例只断言「**不是 401**」）
  3. 两个端点**不是**「整个平台都坏了」
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 阳性对照
- **备注**: AC 步骤 4 的原文（「对照组必须绿 → 否则这条判据没有区分力」）——
  **这是 AC 写得最好的一条**，本用例保留并加强。
  ⚠️ 第 2 条：`u-perm-read@example.com` **没有** `audit:events:read` → 403 才是对的；
  AC 写「**成功**」→ 与夹具不符 → 本用例把判据改成「**不是 401**」。

### TC-022-106: OAuth 令牌调 `POST /api/oauth/introspect`（**不带** client 认证）→ 401

- **关联 AC**: AC-S-151
- **前置条件**: 同 TC-022-103
- **测试步骤**:
  1. `POST /api/oauth/introspect`，带 `Authorization: Bearer <OAuth access_token>`，
     body `token=<任意令牌>`（**不带** client 认证）
  2. 读响应体
- **预期结果**:
  1. HTTP **401**
  2. 响应体**逐字等于** `{"error":"invalid_client"}`（**只含** `error` 键）
  3. 响应体**不含** `active` 键（**不泄露**任何令牌状态）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-10〕**AC 修正（判据与标题对不上）** —— AC 步骤 3 写在「底座业务端点默认拒绝」
  这一节，但 `/api/oauth/introspect` 的认证是 **client 认证**（D208）→
  那个 Bearer **从头到尾没被当成令牌解析** → 本用例测的是「**没带 client 认证**」，
  **不是**「OAuth 令牌被拒」。→ 真正的「OAuth 令牌被拒」由 TC-022-103/104 覆盖。

### TC-022-107: OAuth 令牌调 `POST /api/oauth/introspect`（**带**合法 client 认证）→ 200 `active=true`

- **关联 AC**: AC-S-151
- **前置条件**: `client-A` 的 OAuth access token + `client-A` 的明文 secret
- **测试步骤**:
  1. `POST /api/oauth/introspect`，带 `Authorization: Basic base64(client-A:secret)`，
     body `token=<OAuth access_token>`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. `active` 逐字等于 `true`
  3. `client_id` 逐字等于 `client-A`
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 〔S-10〕〔AC 修正〕 —— 这是「OAuth 令牌**只在 OAuth 协议端点上**有效」的**正向**半边：
  同一个令牌，在协议端点**能用**（靠 client 认证），在业务端点**被拒**（靠 Bearer）→
  合起来才证明 D209 的「默认拒绝」是**按端点**分的，而不是「令牌全废」。

### TC-022-108: OAuth 令牌**不能**被换成登录会话（刷新/交换类端点）

- **关联 AC**: AC-S-151
- **前置条件**: 同 TC-022-103
- **测试步骤**:
  1. `POST /api/iam/refresh`，body `refresh_token=<OAuth access_token>`
  2. `POST /api/iam/login`，body `username=u-perm-read@example.com` / `password=<错密码>`
     + `Authorization: Bearer <OAuth access_token>`
- **预期结果**:
  1. 步骤 1：HTTP **401**（**不是** 200、**不是** 500）
  2. 步骤 1 的响应体**不含** `access_token` / `refresh_token` 键
  3. 步骤 2：HTTP **401**（或 `403 INVALID_CREDENTIALS`）—— **不是** 200
  4. 步骤 2 的响应体**不含**任何令牌键
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: AC 步骤 1、2 只覆盖了两个**读**端点；**写**类端点（刷新 / 登录）
  没被覆盖 → 「默认拒绝」的口子可能开在写路径上。本用例补上。
