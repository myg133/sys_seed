# 授权服务器元数据（AC-S-134）

- **覆盖 AC**: AC-S-134（8 条：TC-022-001 ~ 008）
- **前置基线**:
  - 测试库 `base_test`；平台已配 IAM（`BASE_JWT_*` 齐全）；`base serve` 已起（同源 `http://127.0.0.1:8080`）
  - **元数据端点不依赖任何 OAuth 夹具** → 本文件的前置只有「服务已起」
- **实现事实（用例按此写）**:
  - 元数据端点是**契约登记的固定路径**：`/.well-known/oauth-authorization-server`
    （`BA/contracts/api-conventions/v3.md:56-60`，**不在** `/api` 前缀下）
  - 平台**没有 CORS 层**（`code/playwright.config.ts:3-11`）→ 元数据用 `request` 直接打，不经浏览器
  - `BASE_JWT_ISSUER` 在 E2E 里默认 `https://e2e.base`（`code/scripts/e2e-server.sh:72`），
    而服务真实地址是 `http://127.0.0.1:8080` → **issuer ≠ 服务基址**

---

## AC-S-134: 授权服务器元数据可被发现

### TC-022-001: 元数据端点返回 200 + `application/json`

- **关联 AC**: AC-S-134
- **前置条件**: `base serve` 已起（`BASE_JWT_ISSUER` 已注入）
- **测试步骤**:
  1. `GET /.well-known/oauth-authorization-server`
  2. 读响应头与响应体
- **预期结果**:
  1. HTTP **200**
  2. `Content-Type` **逐字等于** `application/json`
  3. 响应体是**合法 JSON 对象**（`jq -e 'type == "object"'` 为真）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 契约 C1.1（`api-conventions/v3.md:62-88`）把元数据端点列为**固定路径**，
  与 `/api/*` 分组表并列。TC-022-001 是全部 8 条的**基座**，失败则后续 7 条无意义。

### TC-022-002: `issuer` 逐字等于 `BASE_JWT_ISSUER`

- **关联 AC**: AC-S-134
- **前置条件**: 同 TC-022-001
- **测试步骤**:
  1. 取元数据里的 `issuer`
  2. 取进程环境里的 `BASE_JWT_ISSUER`
- **预期结果**:
  1. 两者**逐字节相等**（不是「包含」，不是「同 host」）
  2. `issuer` **不含**结尾斜杠（末尾 `/` → 判 fail）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: 契约 C1.1 要求 `issuer` 与签发令牌的 `iss` **逐字相同**（`v3.md:70`）→ 必须与 `BASE_JWT_ISSUER` 一致。

### TC-022-003: `authorization_endpoint` 的基址 = 配置 `oauth.frontendBaseUrl`（未配置时 = `{issuer}`）

- **关联 AC**: AC-S-134
- **前置条件**: 同 TC-022-001
- **测试步骤**:
  1. 取元数据里的 `authorization_endpoint`
  2. 取配置项 `oauth.frontendBaseUrl` 的值
- **预期结果**:
  1. `authorization_endpoint` 的 **path** 逐字等于 `/oauth/authorize`
  2. `authorization_endpoint` 的 **origin**：
     - `oauth.frontendBaseUrl` **非空**时 → 逐字等于该值的 origin；
     - `oauth.frontendBaseUrl` **为空**时 → 逐字等于 `BASE_JWT_ISSUER` 的 origin
  3. `authorization_endpoint` **不含** query（无 `?`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（配置两种取值）
- **备注**: 〔S-11〕〔S-12〕〔AC 修正〕 —— AC 原文把 `authorization_endpoint` 钉成 `{issuer}/oauth/authorize`，
  与 design §7 的配置项 `oauth.frontendBaseUrl`（默认 `""`）**自相矛盾**；
  且 E2E 里 issuer（`https://e2e.base`）≠ 服务基址（`http://127.0.0.1:8080`）→
  照 AC 字面实现 → 第三方按 metadata 拼出的授权 URL **不可达**。
  本用例按「未配置时回退到 issuer」写；**BA 拍板后可能要改成只认 `frontendBaseUrl`**。

### TC-022-004: 三个协议端点的 URL 逐字正确

- **关联 AC**: AC-S-134
- **前置条件**: 同 TC-022-001
- **测试步骤**:
  1. 读元数据里的 `token_endpoint` / `device_authorization_endpoint` / `introspection_endpoint` / `revocation_endpoint` / `jwks_uri`
- **预期结果**:
  1. `token_endpoint` 的 path 逐字等于 `/api/oauth/token`
  2. `device_authorization_endpoint` 的 path 逐字等于 `/api/oauth/device_authorization`
  3. `introspection_endpoint` 的 path 逐字等于 `/api/oauth/introspect`
  4. `revocation_endpoint` 的 path 逐字等于 `/api/oauth/revoke`
  5. `jwks_uri` 的 path 逐字等于 `/.well-known/jwks.json`
  6. 五个端点的 **origin** 全部**逐字等于** `issuer` 的 origin
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: 这 5 个是**后端**端点 → origin 必须等于 issuer（只有 `authorization_endpoint` 是前端路由，
  见 TC-022-003）。AC 原文写的是 `{issuer}/api/oauth/token` 等，本用例只钉 path + origin 关系，
  避免与 TC-022-003 的「前端基址」口径打架。

### TC-022-005: `grant_types_supported` **恰好**等于三个（不含 `refresh_token`）

- **关联 AC**: AC-S-134
- **前置条件**: 同 TC-022-001
- **测试步骤**:
  1. 读元数据里的 `grant_types_supported`
  2. 与期望集合逐元素比对
- **预期结果**:
  1. 集合 `==` `{"authorization_code","client_credentials","urn:ietf:params:oauth:grant-type:device_code"}`
     （**恰好 3 个**，不多不少）
  2. **不含** `refresh_token`（D217）
  3. **不含** `implicit` / `password`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: 〔S-29〕〔AC 修正〕 —— AC 原文写「**恰好包含**」，是自造词（与同一条 AC 里
  `response_types_supported` 的「**恰好** `["code"]`」不一致）。本用例按「集合恰好等于」写。

### TC-022-006: `response_types_supported` / `code_challenge_methods_supported` 恰好各一个

- **关联 AC**: AC-S-134
- **前置条件**: 同 TC-022-001
- **测试步骤**:
  1. 读元数据里的 `response_types_supported` / `code_challenge_methods_supported`
- **预期结果**:
  1. `response_types_supported` **恰好**等于 `["code"]`（**只有 1 个元素**）
  2. `code_challenge_methods_supported` **恰好**等于 `["S256"]`（**只有 1 个元素**）
  3. `code_challenge_methods_supported` **不含** `plain`（D206）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: `plain` 不出现在元数据里，是第三方**发现**「本平台强制 S256」的唯一途径
  → 这条与 TC-022-031（preview 拒 `plain`）是一对。

### TC-022-007: 三个 `*_auth_methods_supported` 的取值

- **关联 AC**: AC-S-134
- **前置条件**: 同 TC-022-001
- **测试步骤**:
  1. 读元数据里的 `token_endpoint_auth_methods_supported` /
     `introspection_endpoint_auth_methods_supported` / `revocation_endpoint_auth_methods_supported`
- **预期结果**:
  1. `token_endpoint_auth_methods_supported` **包含** `client_secret_basic` **且** 包含 `none`
     （`none` 是给 public client 的，D207）
  2. `introspection_endpoint_auth_methods_supported` **恰好**等于 `["client_secret_basic"]`
     （**不含** `none` —— D208：public client 无权内省）
  3. `revocation_endpoint_auth_methods_supported` **恰好**等于 `["client_secret_basic"]`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: ① 与 ② 是**不同的严格度**（token 端点要 `none`，内省/撤销不要）→ 别抄成一样。
  ② 是 D208 在**元数据层**的可判形态（与 TC-022-068 的运行期 401 配对）。

### TC-022-008: `access_token_expires_in` = 配置 `oauth.accessTokenTtlSeconds`（默认 3600）

- **关联 AC**: AC-S-134
- **前置条件**: 同 TC-022-001；配置 `oauth.accessTokenTtlSeconds` 为已知值
- **测试步骤**:
  1. 读元数据里的 `access_token_expires_in`
  2. 读配置 `oauth.accessTokenTtlSeconds`
- **预期结果**:
  1. `access_token_expires_in` 是**数字**（JSON number，不是字符串）
  2. `access_token_expires_in == ` 配置值
  3. 配成非默认值（如 `120`）时，元数据里的值**跟着变**（不是硬编码 3600）
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（改配置复验）
- **备注**: 这个值是**第三方唯一的撤销延迟上界**（D210 §4）→ 必须真读配置，不能硬编码。
  第 3 条需要「改配置 → 重启 → 复验」，属于**半自动**的一部分（配置改动走运维脚本），
  但断言本身可自动化。
