# client 管理与 secret 落库（AC-S-135、AC-S-136）

- **覆盖 AC**: AC-S-135（8 条：TC-022-009 ~ 016）、AC-S-136（6 条：TC-022-017 ~ 022）
- **前置基线**:
  - `tenant-a` 下存在 E2E 管理员 `u-perm-oauth@example.com`
    （持 `oauth:client:read` / `oauth:client:write` / `oauth:client:delete`）
    —— **由 `scripts/seed-e2e.sh` 播种**，见 TC-022-009 的备注
  - 夹具 client 五个（**由 `scripts/seed-e2e.sh` 播种**，明文 secret 由脚本写到
    `target/e2e-oauth-clients.json`）：
    | 标识 | type | redirect_uris | allowed_grant_types | allowed_scopes | logout URI |
    |---|---|---|---|---|---|
    | `client-A` | confidential | `https://client.example/cb` | authorization_code / client_credentials / device_code | `rbac:role:read` `rbac:role:write` | backchannel=`http://127.0.0.1:$MOCK_PORT/logout`，frontchannel=`https://client.example/logout` |
    | `client-B` | confidential | `https://client.example/cb2` | authorization_code / client_credentials | `sandbox:demo:read` | 无 |
    | `client-public` | public | `https://client.example/cb4` | authorization_code / device_code | `sandbox:demo:read` | 无 |
    | `client-disabled` | confidential | `https://client.example/cb5` | authorization_code | `rbac:role:read` | 无（`status='disabled'`） |
- **实现事实（用例按此写）**:
  - 平台的 POST 创建端点**返回 201 CREATED**（`platform-iam/src/http.rs:586`、
    `platform-rbac/src/http.rs:590`、`platform-org/src/http.rs:406`）→ 期望值取 **201**
  - 摘要口径 = `digest_base64url`（SHA-256 → **base64url 无填充**，43 字符），
    见 `code/crates/platform-iam/src/crypto.rs:56-60`
  - `platform_oauth_client` 是**平台级表**（无 `tenant_id`、**不启用 RLS**，D219）

---

## AC-S-135: client 可注册、可改、可停用、可删，且回调白名单生效

### TC-022-009: 注册 client 返回 201，含 `clientId` 与**一次性** `clientSecret`

- **关联 AC**: AC-S-135
- **前置条件**: `u-perm-oauth@example.com` 已登录（持 `oauth:client:write`）
- **测试步骤**:
  1. `POST /api/oauth/clients`，body `{name:"QA Client", clientType:"confidential",
     redirectUris:["https://client.example/qa"], allowedGrantTypes:["authorization_code"],
     allowedScopes:["rbac:role:read"]}`（`Content-Type: application/json`）
  2. 读响应体
- **预期结果**:
  1. HTTP **201**（**不是 200** —— 平台惯例是 `CREATED`）
  2. 响应体 `clientId` 是**非空字符串**，且**全库唯一**（再注册一次 → 不同的 `clientId`）
  3. 响应体 `clientSecret` 是**非空字符串**（长度 ≥ 43）
  4. 响应体**不含** `clientSecretHash` 键（**键不存在**，不是值为 null）
  5. 响应体 `redirectUris` **逐字等于**请求里的数组
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 〔S-06〕**前置不可达** —— AC 原文写「管理员账号持有 `oauth:client:write`」，
  但 `scripts/seed-e2e.sh:146-151` 只建 4 个 E2E 账号、授权清单（`:177-187`）里**没有任何 `oauth:*`**
  → 本用例的前置**必须**由 `scripts/seed-e2e.sh` 新增播种 `u-perm-oauth@example.com` 并自证。
  〔S-38〕AC 原文写「**201**（或 **200**）」→ 两个取值都算过 = 不可判定；本用例钉 **201**。

### TC-022-010: `clientSecret` 只出现一次 —— 任何读接口都不回显

- **关联 AC**: AC-S-135
- **前置条件**: TC-022-009 已注册一个 confidential client（记下 `clientId` 与明文 secret）
- **测试步骤**:
  1. `GET /api/oauth/clients`
  2. `GET /api/oauth/clients/{client_id}`
  3. 在两个响应体里查找明文 secret 的**逐字**出现
- **预期结果**:
  1. 两个响应体**都不含** `clientSecret` **键**（**键不存在**，不是值为 null）
  2. 两个响应体的**原始文本**都**不含**明文 secret 的任何子串
  3. `GET /api/oauth/clients/{client_id}` 的响应体里**有** `clientId` / `name` / `clientType` /
     `redirectUris` / `allowedGrantTypes` / `allowedScopes` / `status` 键（**字段不被一起删掉**）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 阳性对照
- **备注**: 第 2 条是**阳性对照**（用真值断言，不是「没看到某个键」）——
  这是 AC-S-136 步骤 3 的同款要求，在这里先立住。

### TC-022-011: `PATCH` 改 `status` 为 `disabled` 生效

- **关联 AC**: AC-S-135
- **前置条件**: TC-022-009 注册的 client
- **测试步骤**:
  1. `PATCH /api/oauth/clients/{client_id}`，body `{status:"disabled"}`
  2. `GET /api/oauth/clients/{client_id}`
- **预期结果**:
  1. 步骤 1 返回 **200**（改不是建）
  2. 步骤 2 的 `status` 逐字等于 `disabled`
  3. 步骤 2 的响应体**不含** `clientSecret` 键
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: PATCH 的响应体同样不能回显 secret（AC 只写了「列表里没有」，
  这里把单条也钉上 —— 「任何读接口不回显」是 D200 §3 的原话）。

### TC-022-012: **disabled** 的 client 在 preview 阶段被拒（平台话，403）

- **关联 AC**: AC-S-135
- **前置条件**: 把 `client-disabled` 的 `status` 置为 `disabled`
- **测试步骤**:
  1. `POST /api/oauth/authorize/preview`，body 里 `client_id = client-disabled`
     + 其余合法参数（`redirect_uri` 在白名单内、`code_challenge` + `code_challenge_method=S256`）
  2. 读响应体
- **预期结果**:
  1. HTTP **403**
  2. 响应体是 **RFC 9457 Problem Details**（含 `code` / `status` / `title` / `detail` / `instance` / `trace_id`）
  3. `code` **逐字等于** `OAUTH_CLIENT_DISABLED`
  4. 响应体**不含** `error` 键（不是协议错误体）
  5. 响应体**不含** `ticket` 键（**没有**签出任何票据）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-27〕〔AC 修正〕 —— AC 原文步骤 5 说「用该 client 走一次授权码交换 → **401 + `invalid_client`**」，
  但 disabled 的 client 在 **preview** 就该被拒（design §6.1 给 `OAUTH_CLIENT_DISABLED` 定的状态码是 **403**）
  → 照 AC 字面**走不到令牌交换**。本用例把拒绝点钉在 preview。
  配套的 token 阶段拒绝见 TC-022-013。

### TC-022-013: **disabled** 的 client 在 token 阶段被拒（协议话，401 `invalid_client`）

- **关联 AC**: AC-S-135
- **前置条件**: `client-disabled` 的 `status = 'disabled'`，且已知它的明文 secret
- **测试步骤**:
  1. `POST /api/oauth/token`，form `grant_type=client_credentials` / `scope=rbac:role:read`，
     带 `Authorization: Basic base64(client_id:secret)`
  2. 读响应体
- **预期结果**:
  1. HTTP **401**
  2. 响应体**逐字等于** `{"error":"invalid_client"}`（**只含** `error` 键；
     **不含** `type`/`title`/`status`/`instance`/`code`/`trace_id`）
  3. 响应体**不含**任何 `OAUTH_*` 平台码
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: 〔S-27〕〔AC 修正〕 —— 这一条与 TC-022-012 合起来就把「**两套错误标识在同一端点族里的分工**」钉死了：
  **preview = 平台话（Problem Details + `OAUTH_*`）**，**token = 协议话（`{"error":"invalid_client"}`）**。
  D207 §4：「认证失败统一返回 `invalid_client` + 401（不区分「client 不存在」与「secret 错」）」。

### TC-022-014: 回调白名单生效 —— 不在白名单的 `redirect_uri` 被拒

- **关联 AC**: AC-S-135
- **前置条件**: `client-A` 已注册（白名单 = `https://client.example/cb`）
- **测试步骤**:
  1. `POST /api/oauth/authorize/preview`，body 的 `redirect_uri` = `https://evil.example/cb`
  2. 再试 `https://client.example/cb/extra`（**前缀相同但不相等**）
  3. 再试 `https://client.example/cb`（**在**白名单里）
- **预期结果**:
  1. 步骤 1：**400** + `code == "OAUTH_INVALID_REDIRECT_URI"`（Problem Details）
  2. 步骤 2：**400** + `code == "OAUTH_INVALID_REDIRECT_URI"`（**精确匹配**，不做前缀匹配）
  3. 步骤 3：**200**（进入正常流程）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（等值 / 前缀 / 精确）
- **备注**: design §2.1 明写「回调白名单**精确匹配**（不做前缀/通配匹配）→ 防开放重定向」。
  第 2 条是最容易被实现成 `starts_with` 的那一种 → 必须单独钉。

### TC-022-015: `DELETE` 后单条查询 404 + `OAUTH_CLIENT_NOT_FOUND`

- **关联 AC**: AC-S-135
- **前置条件**: TC-022-009 注册的临时 client
- **测试步骤**:
  1. `DELETE /api/oauth/clients/{client_id}`
  2. `GET /api/oauth/clients/{client_id}`
  3. `GET /api/oauth/clients`
- **预期结果**:
  1. 步骤 1 返回 **204** 或 **200**（两者都表示删除成功）
  2. 步骤 2：**404** + `code == "OAUTH_CLIENT_NOT_FOUND"`（Problem Details）
  3. 步骤 3 的列表里**不含**该 `clientId`
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: 步骤 1 的 204/200 两可 → 本用例**不判**删除的状态码（只判「删了之后查不到」），
  把判据放在**可判**的那一步。

### TC-022-016: 列表里出现新建的 client，且 `clientSecret` 键**不存在**

- **关联 AC**: AC-S-135
- **前置条件**: TC-022-009 注册的 client 仍在
- **测试步骤**:
  1. `GET /api/oauth/clients`
  2. 在返回的数组里找该 `clientId`
- **预期结果**:
  1. 能找到该 `clientId`
  2. 该项**不含** `clientSecret` 键
  3. 该项**不含** `clientSecretHash` 键
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: AC 步骤 3 的原文；与 TC-022-010 的「原文逐字不含」是**互补**的两条。

---

## AC-S-136: client_secret 只以 SHA-256 摘要落库，任何读接口不回显

### TC-022-017: 库里存的是 `digest_base64url` 摘要（测试侧独立实现）

- **关联 AC**: AC-S-136
- **前置条件**: TC-022-009 注册的 confidential client（已知明文 secret）
- **测试步骤**:
  1. 以 `base` 直连查 `SELECT client_secret_hash FROM platform_oauth_client WHERE client_id = ?`
  2. 测试侧**独立实现** SHA-256 + base64url **无填充**（**不调用生产代码**），对明文 secret 求摘要
- **预期结果**:
  1. 步骤 1 的值**逐字节等于**步骤 2 的计算结果
  2. 步骤 1 的值**不等于**明文 secret
  3. 步骤 1 的值长度 **== 43**（32 字节 → base64url 无填充）
  4. 步骤 1 的值**匹配** `^[A-Za-z0-9_-]{43}$`（**不含** `+` / `/` / `=`）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 独立复算
- **备注**: 〔S-16〕**口径钉死** —— 仓里有两个摘要函数（`digest_base64url` 与 `digest_hex`），
  本需求统一用 **base64url 无填充**（D200 明写 `digest_base64url`）。第 4 条把「base64url」与「hex」彻底分开。

### TC-022-018: 全表扫描 `client_secret_hash` **无明文**

- **关联 AC**: AC-S-136
- **前置条件**: 同 TC-022-017
- **测试步骤**:
  1. 以 `base` 直连 `SELECT client_id, client_secret_hash FROM platform_oauth_client`
  2. 对每一行，断言 `client_secret_hash != 明文 secret`
  3. 断言 `client_secret_hash` **不含**明文 secret 的任何**前缀**（≥ 8 字符）
- **预期结果**:
  1. 每行的 `client_secret_hash` 都**不等于**明文 secret
  2. 每行的 `client_secret_hash` 都**不含**明文 secret 的前 8 个字符
  3. 表里**没有**任何一列的值逐字等于明文 secret
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 阳性对照
- **备注**: 阳性对照 = **第 3 条反向验证**：把明文 secret 当作查询值去查，
  `SELECT count(*) … WHERE client_secret_hash = '<明文>'` 必须 **== 0**
  → 这证明扫描真的在扫，而不是「什么也没查到」。

### TC-022-019: 两个读接口的响应体**键不存在** `clientSecret`

- **关联 AC**: AC-S-136
- **前置条件**: 同 TC-022-017
- **测试步骤**:
  1. `GET /api/oauth/clients` → 读**全量**响应体
  2. `GET /api/oauth/clients/{client_id}` → 读**全量**响应体
- **预期结果**:
  1. 两个响应体的**键集合**里都**不含** `clientSecret`（**不是**值为 null）
  2. 两个响应体的**原始文本**都**不含**明文 secret 的任何子串
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: AC 步骤 3 的原文（「键都不存在，不是值为 null」）—— 这条措辞是对的，本用例保留。

### TC-022-020: `public` client **没有** secret 键与摘要列

- **关联 AC**: AC-S-136
- **前置条件**: `client-public` 已存在（type = public）
- **测试步骤**:
  1. 以 `base` 直连查 `SELECT client_type, client_secret_hash FROM platform_oauth_client WHERE client_id = 'client-public'`
  2. `GET /api/oauth/clients/client-public`
- **预期结果**:
  1. `client_type` 逐字等于 `public`
  2. `client_secret_hash` **IS NULL**（**不是**空串、不是 `"null"`）
  3. 步骤 2 的响应体**不含** `clientSecret` 键
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（空 / null）
- **备注**: D200 §4：「`public` client **没有** secret，`client_secret_hash` 为空」。
  AC 没提 public 的形态 → 本用例补上（**null 而不是空串**是仓里其它可空列的惯例）。

### TC-022-021: 敏感值扫描器**能命中**（阳性对照）

- **关联 AC**: AC-S-136
- **前置条件**: 扫描器已实现且**可单独调用**（Rust 侧一个 `pub fn`）
- **测试步骤**:
  1. 造一份**含明文 secret** 的样本（临时副本 / 内存里的样本串）
  2. 调扫描器
- **预期结果**:
  1. 扫描器**命中**该样本（返回非空命中列表）
  2. 对**干净**样本（不含明文），扫描器命中数 **== 0**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 阳性对照
- **备注**: 〔S-17〕〔AC 修正〕 —— AC 步骤 4 说「全仓扫描 `client_secret` 的明文是否出现在任何响应构造点」，
  但**没定扫描器宿主、扫描面、阳性对照的注入点**。dev-guide §9.2：「扫『0 命中』之前，
  先证明扫描器会命中」→ 本用例就是那道闸。

### TC-022-022: 响应构造点里**没有** `clientSecret` 字段（静态扫描 + 阳性对照）

- **关联 AC**: AC-S-136
- **前置条件**: 同 TC-022-021
- **测试步骤**:
  1. 静态扫描 `crates/platform-oauth/src/` 里**构造响应体的位置**
     （`Json(...)` / `Serialize` 派生 / `#[serde(rename)]`）是否出现 `clientSecret` 字段名
  2. 在**临时副本**里塞一个 `clientSecret` 字段 → 重跑扫描器
- **预期结果**:
  1. 步骤 1：命中数 **== 0**
  2. 步骤 2：命中数 **>= 1**，且命中位置包含被塞入的那个文件
  3. 临时副本在用例结束时被清理（不留残留在 `code/` 下）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 阳性对照
- **备注**: 〔S-17〕〔AC 修正〕 —— 「扫响应构造点」必须**能命中**才算数。
  ⚠️ 变异性验证纪律（dev-guide §9.4）：临时副本**不要**用 `cp -p`（保留旧 mtime → 复用陈旧二进制）；
  新建文件做变异性验证前**必须备份**并给出 `sha256` 基线。
