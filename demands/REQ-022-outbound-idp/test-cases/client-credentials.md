# Client Credentials grant（AC-S-141）

- **覆盖 AC**: AC-S-141（6 条：TC-022-047 ~ 052）
- **前置基线**:
  - `client-A`：`allowed_grant_types` 含 `client_credentials`，`allowed_scopes`
    含 `sandbox:demo:read`（**测试时用 `client-B`**，它的白名单正好是 `['sandbox:demo:read']`）
  - 无需用户会话（本 grant 无用户参与）
- **实现事实（用例按此写）**:
  - `client_credentials` 的令牌**省略 `sub`**（D198 §4）→ payload 里**没有** `sub` 键
  - `client_credentials` 的 scope 是**两重**交集（请求 ∩ `client.allowed_scopes`），
    **没有**「用户有效权限点」这一维（D201 §2 的第三维在此 grant 下不存在）→ 见挑刺 S-37

---

## AC-S-141: Client Credentials grant 可换到无用户的令牌

### TC-022-047: 换到令牌，响应形状完整

- **关联 AC**: AC-S-141
- **前置条件**: `client-B`（confidential，`allowed_grant_types` 含 `client_credentials`，
  `allowed_scopes = ['sandbox:demo:read']`）
- **测试步骤**:
  1. `POST /api/oauth/token`，`Content-Type: application/x-www-form-urlencoded`，
     body `grant_type=client_credentials` / `scope=sandbox:demo:read` /
     `client_id=client-B` / `client_secret=<明文>`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. 响应体含 `access_token` / `token_type` / `expires_in` / `scope` 四个键
  3. `token_type` 逐字等于 `"Bearer"`
  4. `expires_in` 是**数字**，且 **> 0**
  5. `scope` **逐字等于** `sandbox:demo:read`
  6. 响应体**不含** `refresh_token` 键（D217）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 步骤 1~2 的原文。

### TC-022-048: payload 里**没有** `sub` 键（不是空串、不是 null）

- **关联 AC**: AC-S-141
- **前置条件**: TC-022-047 拿到 `access_token`
- **测试步骤**:
  1. 测试侧把 `access_token` 按 `.` 分成 3 段
  2. base64url 解第 2 段 → 解析成 JSON
  3. 读 `sub` 与 `client_id` / `grant_type`
- **预期结果**:
  1. 步骤 1：恰好 **3 段**，第 3 段**非空**
  2. 步骤 2：解析成功，是 JSON 对象
  3. 步骤 3：**`sub` 键不存在**（`!("sub" in payload)` 为真；**不是** `null`、**不是** `""`）
  4. `client_id` 逐字等于 `client-B`
  5. `grant_type` 逐字等于 `client_credentials`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言（键不存在）
- **备注**: AC 步骤 3 的原文，措辞（「键不存在」）是**对的** —— 本用例保留。
  解 payload **不验签**是允许的（AC 明说），但**不等于**可以忽略验签 → 见 TC-022-089。

### TC-022-049: 内省该令牌 → `active = true` 且**没有** `username` 键

- **关联 AC**: AC-S-141
- **前置条件**: TC-022-047 拿到 `access_token`
- **测试步骤**:
  1. `POST /api/oauth/introspect`，带 `client-B` 的 `client_secret_basic` 认证，body `token=<access_token>`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. `active` 逐字等于 `true`
  3. **`username` 键不存在**（RFC 7662 §2.2：无用户主体）
  4. **`sub` 键也不存在**（与 TC-022-048 的 payload 一致）
  5. `client_id` 逐字等于 `client-B`
  6. `scope` 逐字等于 `sandbox:demo:read`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: AC 步骤 4 的原文。RFC 7662 §2.2 的响应里 `username` 是**可选**的；
  无用户主体时**必须不出现**（不是空串）→ 与 TC-022-048 同一口径。

### TC-022-050: 请求的 scope **超出** client 白名单 → 400 `invalid_scope`

- **关联 AC**: AC-S-141
- **前置条件**: `client-B` 的白名单 = `['sandbox:demo:read']`（**不含** `rbac:role:read`）
- **测试步骤**:
  1. `POST /api/oauth/token`，body `grant_type=client_credentials` /
     `scope=rbac:role:read` / `client_id=client-B` / `client_secret=<明文>`
  2. 读响应体
- **预期结果**:
  1. HTTP **400**
  2. 响应体**逐字等于** `{"error":"invalid_scope"}`
  3. **没有任何令牌被签发**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-37〕〔AC 修正〕 —— D201 的公式是「请求 ∩ client.allowed_scopes ∩ **用户有效权限点**」，
  但 `client_credentials` **没有用户** → 第三维**不适用** → D201 没写怎么处理。
  本用例把「两重交集」钉死（否则实现者可以任选：跳过 / 报错 / 用 client 自己的权限）。

### TC-022-051: `grant_type` 不被允许 → 400 `unauthorized_client`

- **关联 AC**: AC-S-141
- **前置条件**: `client-public` 的 `allowed_grant_types = ['authorization_code','urn:ietf:params:oauth:grant-type:device_code']`
  （**不含** `client_credentials`）
- **测试步骤**:
  1. `POST /api/oauth/token`，body `grant_type=client_credentials` /
     `client_id=client-public`（public → 不带 secret）
  2. 读响应体
- **预期结果**:
  1. HTTP **400**
  2. 响应体**逐字等于** `{"error":"unauthorized_client"}`
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: D207 §2 说 public client「只带 `client_id`（body 里），**不带** secret」→
  本用例顺带验了 public client 的免密形态；错误码用 RFC 6749 §5.2 的 `unauthorized_client`。

### TC-022-052: `grant_type` 不认识 → 400 `unsupported_grant_type`

- **关联 AC**: AC-S-141
- **前置条件**: `client-B`（confidential）
- **测试步骤**:
  1. `POST /api/oauth/token`，body `grant_type=urn:ietf:params:oauth:grant-type:token-exchange` +
     `client_id=client-B` / `client_secret=<明文>`
  2. 读响应体
- **预期结果**:
  1. HTTP **400**
  2. 响应体**逐字等于** `{"error":"unsupported_grant_type"}`
  3. 响应体**不含** `refresh_token` 相关提示（D217）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 契约 C14 §14.2 的协议码集合里**有** `unsupported_grant_type` → 属于协议端点该说的话。
  与 TC-022-051 的区别：`unauthorized_client` = 「这个 client 不许用这个 grant」；
  `unsupported_grant_type` = 「这个 grant 本平台根本不支持」→ **必须可区分**。
