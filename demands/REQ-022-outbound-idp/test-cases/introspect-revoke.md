# 令牌内省与撤销（AC-S-143、AC-S-144）

- **覆盖 AC**: AC-S-143（8 条：TC-022-063 ~ 070）、AC-S-144（6 条：TC-022-071 ~ 076）
- **前置基线**:
  - `client-A` / `client-B` 均 confidential（`client_secret_basic`）
  - `client-public` 为 public（**无 secret**）
  - 令牌来源：`client-A` 用 **`authorization_code`** grant 换一枚（`token-A`，**有** `sub`）；
    `client-B` 用 `client_credentials` 换一枚（`token-B`，**无** `sub`）
- **实现事实（用例按此写）**:
  - `/api/oauth/introspect` 与 `/api/oauth/revoke` 是**协议端点**（契约 C14 §14.2）
    → 错误体是 **`{"error": …}`**，**不是** Problem Details
  - D208：内省**只能看自己的令牌**；别人的 → **200 + `{"active":false}`**（不报错、不泄露）
  - D208 §4：`active=false` 时**其它字段一律不出现**（RFC 7662 §2.2）
  - D210 §2：撤销粒度 = **grant 级**；撤销 grant → 该 grant 下**所有**未过期令牌立即 `active=false`

---

## AC-S-143: 令牌内省可用，且只能内省自己的令牌

### TC-022-063: 内省自己的令牌 → 200，`active=true`，claims 齐全

- **关联 AC**: AC-S-143
- **前置条件**: `client-A` 持**由 `authorization_code` grant 签发**的 `token-A`（**有** `sub`）
- **测试步骤**:
  1. `POST /api/oauth/introspect`，带 `client-A` 的 `client_secret_basic` 认证，body `token=token-A`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. `active` 逐字等于 `true`（**布尔**）
  3. 响应体的键集合**包含** `client_id` / `scope` / `exp` / `iat` / `sub` /
     `token_type` / `iss` / `aud`
  4. `client_id` 逐字等于 `client-A`
  5. `aud` 逐字等于 `client-A`（= `client_id`，D198 §2）
  6. `iss` 逐字等于 `BASE_JWT_ISSUER`
  7. `exp` 与 `iat` 都是**数字**，且 `exp > iat`
  8. `token_type` 逐字等于 `"Bearer"`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 步骤 1 的原文。⚠️ AC 前置只说「各持一枚有效令牌」，
  **没说哪种 grant** —— 本用例固定用 `client_credentials` 的令牌（它**没有** `sub`），
  所以第 3 条的 `sub` 要求要与 AC-S-141 对齐 → 见挑刺 S-19 与 TC-022-072。

### TC-022-064: 内省**别人的**令牌 → 200 + `{"active":false}`，**只有这一个键**

- **关联 AC**: AC-S-143
- **前置条件**: `client-A` 与 `client-B` 各持一枚有效令牌
- **测试步骤**:
  1. `POST /api/oauth/introspect`，带 `client-A` 的认证，body `token=token-B`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**（**不是** 401 / 403）
  2. 响应体的键集合**恰好**是 `{"active"}`（**只有 1 个键**）
  3. `active` 逐字等于 `false`
  4. 响应体**不含** `client_id` / `scope` / `sub` / `exp` / `iat`（**不泄露**该令牌存在）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 集合精确匹配
- **备注**: AC 步骤 2 的原文（「除 `active` 外没有任何其它字段」）。
  第 2 条用**键集合精确匹配**（不是「抽查几个键」）→ 才抓得住多返回一个 `client_id` 的实现。

### TC-022-065: 内省**已过期**的令牌 → 200 + `{"active":false}`（只有 `active`）

- **关联 AC**: AC-S-143
- **前置条件**: 有一枚 `exp` 已过去的令牌（管理员直连 `platform_oauth_token` 把 `expires_at` 改成过去时间，
  或用 TC-022-037 的 TTL 造法）
- **测试步骤**:
  1. `POST /api/oauth/introspect`，带 `client-A` 的认证，body `token=<过期令牌>`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. 响应体的键集合**恰好**是 `{"active"}`
  3. `active` 逐字等于 `false`
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（过期）
- **备注**: AC 步骤 3 的原文。⚠️ 与 TC-022-076（过期令牌**验签仍通过**）是**两回事**：
  这里判的是**内省**的 `active`，那里判的是**验签**。

### TC-022-066: 内省**已撤销**的令牌 → 200 + `{"active":false}`（只有 `active`）

- **关联 AC**: AC-S-143
- **前置条件**: `token-A` 已被 `POST /api/oauth/revoke` 撤销
- **测试步骤**:
  1. `POST /api/oauth/introspect`，带 `client-A` 的认证，body `token=token-A`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. 响应体的键集合**恰好**是 `{"active"}`
  3. `active` 逐字等于 `false`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（active → revoked）
- **备注**: AC 步骤 4 的原文。

### TC-022-067: `public` client 内省 → 401 + `{"error":"invalid_client"}`（协议话）

- **关联 AC**: AC-S-143
- **前置条件**: `client-public`（public，无 secret）
- **测试步骤**:
  1. `POST /api/oauth/introspect`，body `client_id=client-public` / `token=token-A`
     （**不带** `Authorization` 头）
  2. 读响应体
- **预期结果**:
  1. HTTP **401**
  2. 响应体**逐字等于** `{"error":"invalid_client"}`（**只含** `error` 键；
     **不含** `type`/`title`/`status`/`instance`/`code`/`trace_id`）
  3. 响应体**不含**任何 `OAUTH_*` 平台码
  4. 响应体**不含** `active` 键（**不泄露**令牌状态）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-18〕〔AC 修正〕 —— AC 步骤 5 只写「**401**」，**没写错误体形状**；
  `/introspect` 是**协议端点** → 必须是 RFC 形状，**不是** Problem Details。
  第 4 条钉住「认证失败不泄露令牌状态」。

### TC-022-068: 完全**不带认证**内省 → 401 + `{"error":"invalid_client"}`

- **关联 AC**: AC-S-143
- **前置条件**: 无
- **测试步骤**:
  1. `POST /api/oauth/introspect`，body 只有 `token=token-A`（**无** `Authorization` 头、**无** `client_id`）
  2. 读响应体
- **预期结果**:
  1. HTTP **401**
  2. 响应体**逐字等于** `{"error":"invalid_client"}`
  3. 与 TC-022-067 的响应体**完全相同**（**同一个码**）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-18〕〔AC 修正〕 —— AC 步骤 5 与 6 的期望**都是 401** →
  **无区分力**（分不出「public 被单独拒绝」与「没带认证」）。
  本用例把两条**都**钉成 `invalid_client`，并新增 TC-022-069 作为**真正能区分**的那条。

### TC-022-069: public client 带 `client_secret_basic`（secret 空串）→ 仍 401 `invalid_client`

- **关联 AC**: AC-S-143
- **前置条件**: `client-public`
- **测试步骤**:
  1. `POST /api/oauth/introspect`，带 `Authorization: Basic base64("client-public:")`，body `token=token-A`
  2. 读响应体
- **预期结果**:
  1. HTTP **401**
  2. 响应体**逐字等于** `{"error":"invalid_client"}`
  3. 响应体**不含** `OAUTH_CLIENT_SECRET_INVALID` 或任何 `OAUTH_*` 码
     （public client **没有** secret → 它不走「secret 错」这条路）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-18〕〔AC 修正〕 —— 这是**唯一**能把「public client 无权内省」（D208）
  与「认证缺失」区分开的形态：**两者都 401，但码必须都是协议码 `invalid_client`，
  而不是平台码 `OAUTH_*`**。配套 TC-022-067/068。

### TC-022-070: 内省响应**不含** `refresh_token` / `password` 等敏感键

- **关联 AC**: AC-S-143
- **前置条件**: 同 TC-022-063
- **测试步骤**:
  1. 读 TC-022-063 的内省响应体的**键集合**
- **预期结果**:
  1. 键集合里**不含** `refresh_token` / `password` / `client_secret` / `code`
  2. 键集合里**不含** `nonce`（D211 的 `nonce` 规则只针对 `logout_token`，但内省也不该有）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: RFC 7662 §2.2 的响应成员是**固定集合**；多返回一个 `client_secret` 就是泄漏。

---

## AC-S-144: 令牌可撤销，撤销后内省立即失效

### TC-022-071: 撤销自己的令牌 → 200，内省立即 `active=false`

- **关联 AC**: AC-S-144
- **前置条件**: `client-A` 持 `token-A`（内省为 `active=true`）
- **测试步骤**:
  1. `POST /api/oauth/revoke`，带 `client-A` 的 `client_secret_basic` 认证，
     `Content-Type: application/x-www-form-urlencoded`，body `token=token-A`
  2. 再内省 `token-A`
  3. 以 `base` 直连查 `platform_oauth_token` 里该 `jti` 的行
- **预期结果**:
  1. 步骤 1：HTTP **200**（RFC 7009 §2.2：撤销成功也是 200）
  2. 步骤 2：`active` 逐字等于 `false`（**立即**，D210）
  3. 步骤 3：该行的 `status` 逐字等于 `revoked`，`revoked_at` **非空**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: AC 步骤 1~3 的原文。

### TC-022-072: 用**别人的**认证撤销 → 200，且**目标令牌不被撤销**

- **关联 AC**: AC-S-144
- **前置条件**: `client-A` 持 `token-A`；`client-B` 持 `token-B`
- **测试步骤**:
  1. `POST /api/oauth/revoke`，带 `client-B` 的认证，body `token=token-A`
  2. 内省 `token-A`（用 `client-A` 的认证）
  3. 内省 `token-B`（用 `client-B` 的认证）
- **预期结果**:
  1. 步骤 1：HTTP **200**（**不报错、不泄露**）
  2. 步骤 2：`active` 逐字等于 `true`（`token-A` **未被撤销**）
  3. 步骤 3：`active` 逐字等于 `true`（`token-B` 未受影响）
  4. 步骤 1 的响应体**不含** `active` / `error` / `OAUTH_*` 任何码
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径 + 否定断言
- **备注**: AC 步骤 4 的原文（「但该令牌**不得**被真的撤销掉」）。
  RFC 7009 §2.2：「无效令牌的撤销请求也返回 200」。第 4 条钉住「200 的响应体里不带任何状态信息」。

### TC-022-073: 撤销**根本不存在**的令牌 → 200

- **关联 AC**: AC-S-144
- **前置条件**: 无
- **测试步骤**:
  1. `POST /api/oauth/revoke`，带 `client-A` 的认证，body `token=definitely-not-a-real-token`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. 响应体**不含** `error` 键（**不是** `invalid_grant`）
  3. 响应体**不含** `OAUTH_*` 码
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: AC 步骤 5 的原文。RFC 7009 §2.2 明文：「无效令牌的撤销请求也返回 200」。
  第 3 条钉住「**不区分** client 不存在与令牌不存在」→ 不做枚举。

### TC-022-074: 撤销**无认证** → 401 + `{"error":"invalid_client"}`（协议话）

- **关联 AC**: AC-S-144
- **前置条件**: 无
- **测试步骤**:
  1. `POST /api/oauth/revoke`，body 只有 `token=token-A`（**无** `Authorization` 头）
  2. 读响应体
- **预期结果**:
  1. HTTP **401**
  2. 响应体**逐字等于** `{"error":"invalid_client"}`（**只含** `error` 键）
  3. 响应体**不含** `OAUTH_*` 码
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-18〕同款 —— `/revoke` 是**协议端点** → 认证失败也必须是 RFC 形状。
  AC 完全没写这条。

### TC-022-075: 撤销**过期**令牌 → 200（不报错）

- **关联 AC**: AC-S-144
- **前置条件**: 有一枚已过期的令牌
- **测试步骤**:
  1. `POST /api/oauth/revoke`，带 `client-A` 的认证，body `token=<过期令牌>`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. 响应体**不含** `error` 键
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（过期）
- **备注**: 与 TC-022-073 同款语义（无效令牌也 200），但**状态不同**（过期 vs 不存在）
  → 单独列一条，防止实现成「过期 → 400 `invalid_grant`」。

### TC-022-076: 撤销 **grant** → 该 grant 下**所有**令牌立即失效

- **关联 AC**: AC-S-144
- **前置条件**: `client-A` 对 `u-perm-read@example.com` 有 **active grant**；
  该 grant 下签发了 **2 枚**令牌（`token-A1` / `token-A2`）
- **测试步骤**:
  1. 内省 `token-A1` 与 `token-A2` → 都是 `active=true`
  2. `POST /api/oauth/grants/revoke`（用户会话），body `{clientId:"client-A"}`
  3. 再内省 `token-A1` 与 `token-A2`
  4. 以 `base` 直连查 `platform_oauth_token` 里这两行
- **预期结果**:
  1. 步骤 1：两枚都是 `active=true`
  2. 步骤 3：两枚**都**是 `active=false`（**所有**令牌，不是只有一枚）
  3. 步骤 4：两行的 `status` **都**逐字等于 `revoked`
  4. `platform_oauth_grant` 里该行的 `status` 逐字等于 `revoked`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（批量）
- **备注**: 〔S-33〕〔AC 修正〕 —— D210 §2：「撤销 grant → 该 grant 下**所有**未过期令牌立即 `active=false`」，
  但 AC-S-144 与 AC-S-147 **都只验一枚令牌** → 实现成「只撤销那一枚」也能过 → **grant 级语义没被验收**。
  本用例签发**两枚**再撤，钉住「所有」。
