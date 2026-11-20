# Device Code（AC-S-142）

- **覆盖 AC**: AC-S-142（10 条：TC-022-053 ~ 062）
- **前置基线**:
  - `client-A`：`allowed_grant_types` 含 `urn:ietf:params:oauth:grant-type:device_code`
  - `tenant-a` 下用户 `u-perm-read@example.com` 已登录浏览器
  - 设备码页是**前端路由** `/oauth/device`（D218）
- **实现事实（用例按此写）**:
  - 协议端点：`POST /api/oauth/device_authorization` 与 `POST /api/oauth/token`
    → 错误体是 **RFC 形状** `{"error": …}`（契约 C14）
  - RFC 8628 §6.1：`user_code` 应为 **8 个字符**，字符集 **base20**
    （`BCDFGHJKLMNPQRSTVWXZ`）→ 本用例按此钉
  - RFC 8628 §3.5：轮询快于 `interval` → 返回 **`slow_down`**，间隔 **+5 秒**
  - design §2.4 的设备码表有 `poll_interval_seconds` / `last_polled_at` 两列（限流依据）

---

## AC-S-142: Device Code 全流程（申请 → 用户输入短码 → 授权 → 轮询换令牌）

### TC-022-053: 申请设备码返回 200 + 六个字段齐全

- **关联 AC**: AC-S-142
- **前置条件**: `client-A`
- **测试步骤**:
  1. `POST /api/oauth/device_authorization`，`Content-Type: application/x-www-form-urlencoded`，
     body `client_id=client-A` / `client_secret=<明文>` / `scope=sandbox:demo:read`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. 响应体含 `device_code` / `user_code` / `verification_uri` /
     `verification_uri_complete` / `expires_in` / `interval` 六个键
  3. `device_code` 与 `user_code` 都是**非空字符串**且**互不相同**
  4. `expires_in` 是**数字**且 **> 0**
  5. `interval` 是**数字**且 **> 0**
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 步骤 1 的原文（「五个字段齐全」实际是**六个** —— AC 自己数错了，
  本用例按 RFC 8628 §3.2 的**六个必填**写：`device_code` / `user_code` /
  `verification_uri` / `verification_uri_complete` / `expires_in` / `interval`）。

### TC-022-054: `expires_in` = 配置 `oauth.deviceCodeTtlSeconds`（默认 600）

- **关联 AC**: AC-S-142
- **前置条件**: 同 TC-022-053；配置 `oauth.deviceCodeTtlSeconds` 为已知值
- **测试步骤**:
  1. 读响应体的 `expires_in`
  2. 读配置 `oauth.deviceCodeTtlSeconds`
- **预期结果**:
  1. `expires_in == ` 配置值
  2. 配成非默认值（如 `300`）时，`expires_in` **跟着变**（**不是**硬编码 600）
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（改配置复验）
- **备注**: AC 原文：「`expires_in` = 平台配置的 `oauth.deviceCodeTtlSeconds`」——
  这条**写得对**，本用例保留（并补「不是硬编码」这一条）。

### TC-022-055: `interval` = 配置 `oauth.deviceCodePollIntervalSeconds`（默认 5）

- **关联 AC**: AC-S-142
- **前置条件**: 同 TC-022-053；配置 `oauth.deviceCodePollIntervalSeconds` 为已知值
- **测试步骤**:
  1. 读响应体的 `interval`
  2. 读配置 `oauth.deviceCodePollIntervalSeconds`
- **预期结果**:
  1. `interval == ` 配置值
  2. 配成非默认值（如 `9`）时，`interval` **跟着变**
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（改配置复验）
- **备注**: 同 TC-022-054。

### TC-022-056: `user_code` 的**格式**：8 字符 + base20 字符集

- **关联 AC**: AC-S-142
- **前置条件**: 同 TC-022-053
- **测试步骤**:
  1. 读响应体的 `user_code`
  2. 断言长度与字符集
- **预期结果**:
  1. `user_code` 长度 **== 8**
  2. `user_code` **逐字符** ∈ `BCDFGHJKLMNPQRSTVWXZ`（base20，**不含**易混字符
     `0`/`1`/`I`/`O`/`U`/`V`/`L`… 以 base20 为准）
  3. `user_code` **不含** `-` / `_` / 空格
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（格式）
- **备注**: 〔S-21〕〔AC 修正〕 —— AC **一个字都没测** `user_code` 的格式；
  RFC 8628 §6.1 要求 8 字符 + base20（用户要在电话/CLI 上手输 → 字符集必须无歧义）。
  第 3 条排除掉「把 base64url 字符集直接用上」的省事实现。

### TC-022-057: `verification_uri` / `verification_uri_complete` 的**绝对 URL** 与基址

- **关联 AC**: AC-S-142
- **前置条件**: 同 TC-022-053
- **测试步骤**:
  1. 读 `verification_uri` / `verification_uri_complete`
  2. 解析成 URL
- **预期结果**:
  1. `verification_uri` 是**绝对 URL**（scheme + host 都存在，**不是**相对路径）
  2. `verification_uri` 的 path 逐字等于 `/oauth/device`
  3. `verification_uri` 的 origin 逐字等于**授权页基址**（与 TC-022-003 同一符号：
     配置 `oauth.frontendBaseUrl` 非空时用它，为空时用 `{issuer}` 的 origin）
  4. `verification_uri_complete` **以** `verification_uri` **加** `?user_code=` 开头
  5. `verification_uri_complete` 里 `user_code` 的值（URL 解码后）
     逐字等于响应里的 `user_code`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: 〔S-21〕〔AC 修正〕 —— AC 说「浏览器访问 `verification_uri`」，
  但**没钉它指向哪**（`{issuer}/oauth/device`？`{frontendBaseUrl}/oauth/device`？）→
  design §7 的 5 个配置里也**没有**「设备码页基址」→ **绝对 URL 的基准缺失**。
  第 4/5 条还钉住了 `verification_uri_complete` 的**构造口径**（RFC 8628 §3.2）。

### TC-022-058: 用户未授权时轮询 → 400 `authorization_pending`（且**不发**令牌）

- **关联 AC**: AC-S-142
- **前置条件**: 已申请到 `device_code`
- **测试步骤**:
  1. **立刻**用 `device_code` 轮询 `POST /api/oauth/token`，
     body `grant_type=urn:ietf:params:oauth:grant-type:device_code` /
     `device_code=<device_code>` / `client_id=client-A` / `client_secret=<明文>`
  2. 读响应体
- **预期结果**:
  1. HTTP **400**
  2. 响应体**逐字等于** `{"error":"authorization_pending"}`
  3. 响应体**不含** `access_token` 键
  4. 设备码行**仍在**（`status == 'pending'`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 步骤 2 的原文。`authorization_pending` 在契约 C14 §14.2 的协议码集合里 ✅。

### TC-022-059: 用户在设备码页授权后轮询 → 200 拿到令牌

- **关联 AC**: AC-S-142
- **前置条件**: 已申请到 `device_code` / `user_code`；`u-perm-read@example.com` 已登录浏览器
- **测试步骤**:
  1. 打开 `verification_uri_complete`（或 `verification_uri` + 手输 `user_code`）
  2. 输入 `user_code`，点「允许」
  3. 用**同一个** `device_code` 轮询 `POST /api/oauth/token`
  4. 读响应体
- **预期结果**:
  1. 步骤 3：HTTP **200**
  2. 响应体含 `access_token` / `token_type` / `expires_in` / `scope`
  3. `scope` **逐字等于** `sandbox:demo:read`
  4. `token_type` 逐字等于 `"Bearer"`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（pending → approved）
- **备注**: AC 步骤 3~4 的原文。第 1 步走 `verification_uri_complete` 可以顺带验
  TC-022-057 的 `user_code` 构造口径（少一次手输）。

### TC-022-060: 授权后设备码行被**删除**（行数 = 0）

- **关联 AC**: AC-S-142
- **前置条件**: TC-022-059 已完成
- **测试步骤**:
  1. 以 `base` 直连 `SELECT count(*) FROM platform_oauth_device_code`
  2. 以 `base_app` 在事务内 `set_config('app.tenant_id','tenant-a')` 后查同一张表
- **预期结果**:
  1. 两个计数**相等**
  2. 该计数 **== 0**（设备码行**已被删除**）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: AC 步骤 5 的原文（「行数 = 0」）。两个连接都查是为了排除「RLS 把它藏了」
  （REQ-014 的「假绿」教训）—— 超管看到 0 才是真的 0。

### TC-022-061: `user_code` 与 `device_code` 在库里**都只存摘要**；拿 `user_code` 当 `device_code` 用 → 失败

- **关联 AC**: AC-S-142
- **前置条件**: 已申请到 `device_code` / `user_code`（**未**授权）
- **测试步骤**:
  1. 以 `base` 直连查设备码行的 `device_code_hash` / `user_code_hash`
  2. 测试侧**独立实现** SHA-256 → base64url 无填充，对 `device_code` / `user_code` 各算一次
  3. 用 `user_code` **当作 `device_code`** 轮询 `POST /api/oauth/token`
- **预期结果**:
  1. 步骤 1：两个列都**非空**，且**互不相同**
  2. 步骤 2：`device_code_hash` 逐字节等于对 `device_code` 的计算结果；
     `user_code_hash` 逐字节等于对 `user_code` 的计算结果
  3. 全表 `SELECT count(*) FROM platform_oauth_device_code WHERE device_code_hash = '<明文 device_code>'`
     **== 0**（**没有明文**）
  4. 步骤 3：HTTP **400** + `{"error":"invalid_grant"}`
     （**不是** `authorization_pending` —— 用错码 → 查不到行 → 走 `invalid_grant` 分支）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 独立复算 + 异常路径
- **备注**: 〔S-16〕**口径钉死** —— AC 原文只说「都只存摘要」，**没说编码**；
  本用例按 **base64url 无填充**（与 TC-022-017/035 一致）。第 4 条钉住「用错码的失败形态」
  —— 实现成 `authorization_pending` 会让用户无限等待。

### TC-022-062: 轮询快于 `interval` → 400 `slow_down`，且间隔被 +5 秒

- **关联 AC**: AC-S-142
- **前置条件**: 已申请到 `device_code`（`interval` 已知，默认 5）；`u-perm-read@example.com` 已登录但**未**授权
- **测试步骤**:
  1. **立刻**轮询 `POST /api/oauth/token`（`grant_type=urn:ietf:params:oauth:grant-type:device_code`）
  2. **不等 `interval`**，**立刻**再轮询一次（第二次）
  3. 以 `base` 直连查该行的 `poll_interval_seconds` / `last_polled_at`
- **预期结果**:
  1. 步骤 1：HTTP **400** + `{"error":"authorization_pending"}`
  2. 步骤 2：HTTP **400** + `{"error":"slow_down"}`（**不是** `authorization_pending`）
  3. 步骤 3：该行的 `poll_interval_seconds` **== 初始 `interval` + 5**（RFC 8628 §3.5）
  4. 步骤 3：该行的 `last_polled_at` **非 NULL**，且 **>=** 步骤 1 的轮询时刻
  5. 步骤 3：设备码行的 `status` 仍为 `pending`（**慢下来不等于失败**）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（时间间隔）
- **备注**: 〔S-21〕〔AC 修正〕 —— AC **完全没测** RFC 8628 §3.5 的 `slow_down`，
  design §2.4 的 `poll_interval_seconds` / `last_polled_at` 两列也**没有任何 AC 覆盖**
  → 不覆盖 = 限流没被验收。第 5 条钉住「慢下来不是失败」——
  实现成「直接作废设备码」会让用户在正常轮询节奏下被踢。
