# 同意页、授权记录与免登（AC-S-146、AC-S-147、AC-S-148）

- **覆盖 AC**: AC-S-146（7 条：TC-022-077 ~ 083）、AC-S-147（4 条：TC-022-084 ~ 087）、
  AC-S-148（3 条：TC-022-088 ~ 090）
- **前置基线**:
  - `tenant-a` 下 `u-perm-read@example.com`（持 `rbac:role:read`）
  - `client-A`：`redirect_uris = ['https://client.example/cb']`，`allowed_scopes` 含 `rbac:role:read`
  - `tenant-b` 下 `u-perm-other@example.com`（持 `rbac:role:read`）—— **由 `scripts/seed-e2e.sh` 播种**
  - 该用户对 `client-A` **尚无**授权记录
- **实现事实（用例按此写）**:
  - 同意票据 = **ES256 签名 JWT**，claims 见 design §5.2
    （`typ` / `client_id` / `redirect_uri` / `scope` / `state` / `code_challenge` /
    `code_challenge_method` / `sub` / `tid` / `exp`(5 分钟) / `jti`）
  - `confirm` / `deny` **只信 ticket**，不接受原始参数（D204 §2）
  - `POST /api/oauth/authorize/confirm` 与 `/deny` 是**平台端点** → 错误体是 Problem Details

---

## AC-S-146: 同意页可用、同意记录落库，且用户所见 = 实际授予

### TC-022-077: 同意页展示 client 名与**全部**请求权限点

- **关联 AC**: AC-S-146
- **前置条件**: 同本文件头；浏览器已登录 `u-perm-read@example.com`
- **测试步骤**:
  1. 走完授权流程的前半段（打开授权 URL → preview → 渲染同意页）
  2. 读页面文本
- **预期结果**:
  1. 页面文本**逐字出现** client 的 `name`
  2. 页面文本**逐字出现** `rbac:role:read`
  3. 页面**不含** `rbac:role:write`（未请求的权限点不得出现）
  4. 页面有「允许」与「拒绝」两个可点入口
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 步骤 1 的原文（「展示**全部**权限点码」）。

### TC-022-078: 授权后 `platform_oauth_grant` **恰好 1 行**，字段齐全

- **关联 AC**: AC-S-146
- **前置条件**: TC-022-077 已在同意页点过「允许」
- **测试步骤**:
  1. 以 `base` 直连 `SELECT * FROM platform_oauth_grant WHERE tenant_id='tenant-a'`
  2. 以 `base_app` 在事务内 `set_config('app.tenant_id','tenant-a')` 后查同一张表
- **预期结果**:
  1. 两个查询的行数**相等**，且 **== 1**
  2. 该行 `client_id` 逐字等于 `client-A`
  3. 该行 `user_id` 逐字等于 `u-perm-read@example.com` 的用户 id
  4. 该行 `scopes`（数组）**恰好**等于 `["rbac:role:read"]`
  5. 该行 `status` 逐字等于 `active`
  6. 该行 `consent_at` **非 NULL**
  7. 该行 `granted_at` **非 NULL**
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: AC 步骤 2 的原文（「恰好 **1 行**」）。两个连接都查是为了排除 RLS 假绿。

### TC-022-079: **篡改** ticket（改 `scope` 后重新编码）→ 400 + `OAUTH_TICKET_INVALID`，且不签 code

- **关联 AC**: AC-S-146
- **前置条件**: 已从 preview 拿到一个**合法**的 ticket
- **测试步骤**:
  1. 把 ticket 的 payload 第 2 段 base64url **解码** → 解析 JSON → 把 `scope` 改成更大的集合
     （如 `["rbac:role:read","rbac:role:write"]`）→ **重新编码** → 拼回 `header.payload.sig`
  2. `POST /api/oauth/authorize/confirm`，body 带改后的 ticket
  3. 读响应体；查 `platform_oauth_code` 的行数
- **预期结果**:
  1. 步骤 2：HTTP **400**
  2. 响应体是 Problem Details，`code` **逐字等于** `OAUTH_TICKET_INVALID`
  3. 步骤 3：授权码表行数 **== 0**（**不得**签发任何 code）
  4. 响应体**不含** `redirectTo` 键
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 反向变异（篡改输入）
- **备注**: 〔S-08〕**判据强度不足** —— 篡改后签名必失 → 本用例**恒真**：
  它只证明「验签在工作」，**证明不了** D204 想证明的「用户所见 = 实际授予」。
  → 真正能证明的那条是 TC-022-080。

### TC-022-080: 用**自签**的 ticket（`scope` 比所见更大）→ 400 `OAUTH_TICKET_INVALID`（真判据）

- **关联 AC**: AC-S-146
- **前置条件**: 测试侧**持有**平台的 ES256 私钥（见备注）
- **测试步骤**:
  1. 用私钥签一个 `typ="oauth_consent_ticket"` 的 JWT，
     claims 里 `scope = ["rbac:role:read","rbac:role:write"]`
     （**比用户在同意页上看到的更大**），其余字段与 preview 返回的 ticket 一致
  2. `POST /api/oauth/authorize/confirm`，body 带该自签 ticket
  3. 读响应体；查 `platform_oauth_grant` 与 `platform_oauth_code`
- **预期结果**:
  1. 步骤 2：HTTP **400** + `code == "OAUTH_TICKET_INVALID"`
  2. `platform_oauth_grant` 的行数 **== 0**（**没有**新增/扩大授权）
  3. `platform_oauth_code` 的行数 **== 0**（**没有**签发 code）
  4. 用**合法** ticket 走一遍 → 签出的 code 换到的令牌，其 `scope` **恰好**等于
     同意页上显示的 `rbac:role:read`（**不多一个权限**）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 反向变异（构造签名有效但 scope 不同的票据）
- **备注**: 〔S-08〕**前置不可达（当前）** —— E2E 的私钥在 `scripts/e2e-server.sh:65-73` 里
  **现场生成 + `export`、不落盘**，Playwright 是**另一个进程** → 拿不到私钥 → 本用例**跑不了**。
  → **BA 必须先拍板**：把私钥**落盘**到 `target/e2e-jwt-private.pem`（0600、不入库、每次覆盖）
  并补进 design §13 的文件清单；否则「用户所见 = 实际授予」**没有任何 AC 覆盖**。
  第 4 条是这条 AC 的**正向**半边（所见的 == 所授的），**不依赖**私钥 → 建议**优先实现**。

### TC-022-081: `deny` → `redirectTo` 的 query 里 `error=access_denied` 且 `state` 原样

- **关联 AC**: AC-S-146
- **前置条件**: 已从 preview 拿到一个**合法**的 ticket（请求的 `state=xyz`）
- **测试步骤**:
  1. `POST /api/oauth/authorize/deny`，body 带该 ticket
  2. 读响应体的 `redirectTo`
  3. 解析 `redirectTo` 的 query
- **预期结果**:
  1. 步骤 1：HTTP **200**
  2. `redirectTo` 的 origin + path 逐字等于 `https://client.example/cb`
  3. `redirectTo` 的 query 里 `error` **逐字等于** `access_denied`
  4. `redirectTo` 的 query 里 `state` **逐字等于** `xyz`
  5. `redirectTo` 的 query 里**不含** `code` 键
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 步骤 4 的原文。`access_denied` 是 RFC 6749 §4.1.2.1 的**授权错误码**
  （出现在**重定向的 query** 里，不是响应体里）→ 与 §全局约定的「协议码」是两处不同的用法，
  本用例把**位置**钉死。

### TC-022-082: `deny` **不改变**已有授权记录

- **关联 AC**: AC-S-146
- **前置条件**: TC-022-078 已落库 1 行 `active` 的 grant；再拿一个**合法** ticket
- **测试步骤**:
  1. 记录当前 `platform_oauth_grant` 的行数与那一行的 `status`
  2. `POST /api/oauth/authorize/deny`，body 带合法 ticket
  3. 再查 `platform_oauth_grant`
- **预期结果**:
  1. 步骤 3：行数 **== 步骤 1 的行数**（**仍然只有那 1 行**）
  2. 那一行的 `status` **仍是** `active`（拒绝**不改**已有授权）
  3. `revoked_at` **仍是 NULL**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: AC 步骤 4 的后半句（「拒绝**不改**已有授权」）——
  这条**语义正确**，本用例保留并补上 `revoked_at` 的否定断言。

### TC-022-083: 同一 (tenant, client, user) 再次授权 = **更新**（幂等，不新增行）

- **关联 AC**: AC-S-146
- **前置条件**: TC-022-078 已落库 1 行 grant
- **测试步骤**:
  1. 再走一遍授权流程并点「允许」
  2. 查 `platform_oauth_grant`
- **预期结果**:
  1. 步骤 2：行数 **== 1**（**不新增**）
  2. 该行的 `scopes` **恰好**等于 `["rbac:role:read"]`（幂等更新，**不累加**）
  3. 该行的 `consent_at` **>=** 上一次的 `consent_at`（被更新了）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 状态迁移（重复执行）
- **备注**: D202 §UNIQUE (tenant_id, client_id, user_id) → 「再次授权 = **更新** scope（幂等）」。
  AC 没覆盖这条 → 补上（否则「重复授权」会变成「授权被叠加扩大」）。

---

## AC-S-147: 用户能查自己的授权并撤销，撤销后令牌立即失效

### TC-022-084: `GET /api/oauth/grants` **恰好 1 条**，字段齐全

- **关联 AC**: AC-S-147
- **前置条件**: `u-perm-read@example.com` 已登录；对 `client-A` 有 1 条 active grant
- **测试步骤**:
  1. `GET /api/oauth/grants`（带用户会话的 Bearer）
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. 返回的数组长度 **== 1**
  3. 该条含 `clientId` / `scopes` / `grantedAt` / `status` 四个键
  4. 该条 `clientId` 逐字等于 `client-A`
  5. 该条 `scopes` **恰好**等于 `["rbac:role:read"]`
  6. 返回里**不含**其它用户或其它 client 的记录（**长度就是 1**，不是「过滤后看着像 1」）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: AC 步骤 1 的原文。

### TC-022-085: 撤销授权 → 200，记录 `status=revoked` 且 `revokedAt` 非空

- **关联 AC**: AC-S-147
- **前置条件**: 同 TC-022-084
- **测试步骤**:
  1. `POST /api/oauth/grants/revoke`，body `{clientId:"client-A"}`（带用户会话）
  2. `GET /api/oauth/grants`
  3. 以 `base` 直连查 `platform_oauth_grant`
- **预期结果**:
  1. 步骤 1：HTTP **200**
  2. 步骤 2：那条的 `status` 逐字等于 `revoked`
  3. 步骤 2：那条的 `revokedAt` **非空**
  4. 步骤 3：库里该行的 `status` 逐字等于 `revoked`，`revoked_at` **非 NULL**
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: AC 步骤 2~3 的原文。

### TC-022-086: 撤销后**该 grant 签发的令牌**立即 `active=false`

- **关联 AC**: AC-S-147
- **前置条件**: `client-A` 对该用户有一枚由该 grant 签发的有效令牌
- **测试步骤**:
  1. 内省该令牌 → 记下 `active`
  2. `POST /api/oauth/grants/revoke`，body `{clientId:"client-A"}`
  3. 再内省**同一个**令牌
- **预期结果**:
  1. 步骤 1：`active` 逐字等于 `true`
  2. 步骤 3：`active` 逐字等于 `false`（**立即**，D210）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: AC 步骤 4 的原文。
  ⚠️ 「所有令牌」的版本见 TC-022-076（挑刺 S-33）—— 本条只钉一枚。

### TC-022-087: 另一个租户的用户查授权 → **空数组**（不泄露存在性）

- **关联 AC**: AC-S-147
- **前置条件**: `tenant-b` 的 `u-perm-other@example.com` 已登录（持 `rbac:role:read`）
- **测试步骤**:
  1. 以 `u-perm-other@example.com` 的会话调 `GET /api/oauth/grants`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. 返回的数组长度 **== 0**（**空数组**，不是 404、不是 403）
  3. 响应体**不含** `tenant-a` 的 `clientId` / 用户 id / `scopes`
  4. **阳性对照**：`tenant-a` 的用户查同一端点 → 数组长度 **== 1**
     （证明第 2 条不是因为「接口返回空」）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 阳性对照
- **备注**: 〔S-34〕**前置不可达（当前）** —— `scripts/seed-e2e.sh:146-151` **只建 `tenant-a` 的 4 个账号**
  → `tenant-b` 的用户不存在 → 本用例的前置**必须**由播种脚本补上。
  第 4 条是**阳性对照**（没有它，「返回空数组」可能只是「接口坏了」的假绿）。

---

## AC-S-148: 已有授权时免登（静默续发，不重复同意页）

### TC-022-088: `prompt=none` + 已有 grant 覆盖 scope → **不出同意页**，直接签 code

- **关联 AC**: AC-S-148
- **前置条件**: `u-perm-read@example.com` 已对 `client-A` 授权，
  该 grant 的 `scopes ⊇ {"rbac:role:read"}`
- **测试步骤**:
  1. 打开 `/oauth/authorize?…&prompt=none&…`（其余参数同 AC-S-137）
  2. 观察页面；读最终地址栏
- **预期结果**:
  1. 页面**没有**同意页（**没有**出现请求的权限点码文本、没有「允许」按钮）
  2. 地址栏的 query 里 `code` **非空**
  3. 地址栏的 query 里 `state` 逐字等于请求的 `state`
  4. 页面**没有** `error=consent_required` / `error=login_required`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 步骤 2~3 的原文（「没有同意页（页面直接跳转）」）。
  ⚠️ 仍依赖 TC-022-024（`return_to` 机制）—— 若未登录则根本走不到这一步。

### TC-022-089: `prompt=none` 但 grant **不覆盖**请求的 scope → `consent_required`，**不发 code**

- **关联 AC**: AC-S-148
- **前置条件**: `u-perm-read@example.com` 对 `client-A` 的 grant 只有 `rbac:role:read`
- **测试步骤**:
  1. 打开 `/oauth/authorize?…&prompt=none&scope=rbac:role:read%20rbac:role:write&…`
  2. 读页面 / 地址栏
  3. 查 `platform_oauth_code` 的行数
- **预期结果**:
  1. 页面显示**需要同意**（出现请求的权限点码 `rbac:role:write` 的展示 或 明确的同意入口）
  2. 地址栏的 query 里 `error` **逐字等于** `consent_required`
  3. 地址栏的 query 里**不含** `code` 键（**不签发 code**）
  4. 步骤 3：授权码表行数 **== 0**
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-39〕〔AC 修正〕 —— AC 只写「返回 `consent_required`」，**没说是哪个端点、什么形状**。
  D212 §3 说「按 OIDC 语义返回 `login_required` / `consent_required`」，
  而 preview 是**平台端点**（C14 §14.4）→ 究竟是
  `{"error":"consent_required"}`（协议形状）还是 Problem Details 的 `detail` 里含它？
  **必须由 BA 拍板**。本用例按「地址栏 query 的 `error=consent_required`」
  （即前端路由把它透出到 URL）写。

### TC-022-090: **不带** `prompt=none` → **出现**同意页（缺省 = 允许交互）

- **关联 AC**: AC-S-148
- **前置条件**: 同 TC-022-088（已有 grant 且覆盖 scope）
- **测试步骤**:
  1. 打开 `/oauth/authorize?…`（**不带** `prompt`）
  2. 观察页面
- **预期结果**:
  1. 页面**出现**同意页（出现请求的权限点码 + 「允许」按钮）
  2. 地址栏的 query 里**不含** `code`（**还没**签发）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（prompt 缺省 / none）
- **备注**: AC 步骤 5 的原文（「`prompt` 缺省 = 允许交互」）。
  ⚠️ 这条与 TC-022-088 是一对：**只有这两个 prompt 取值能区分「静默续发」与「走同意页」**；
  AC 原文只测了两个极端，缺 `prompt=login` / `prompt=consent` 的语义 → 建议 BA 明确是否支持。
