# 令牌格式与验签（AC-S-145）

- **覆盖 AC**: AC-S-145（7 条：TC-022-125 ~ 131）
- **前置基线**:
  - `client-A` 持有一枚**由 `authorization_code` grant 签发**的有效令牌
    （**有** `sub`；`client_credentials` 的令牌**没有** `sub`，见 D198 §4）
  - 平台 JWKS 可取：`GET /.well-known/jwks.json`
- **实现事实（用例按此写）**:
  - 签名算法 = **ES256**（`platform-iam/src/jwt.rs:28` 的 `SIGNING_ALGORITHM`）
  - `aud` = **`client_id`**（D198 §2），`iss` = 平台 issuer（D198 §3）
  - claims = `iss` / `aud` / `sub` / `tid` / `client_id` / `scope` / `grant_type` /
    `iat` / `nbf` / `exp` / `jti`（D198 §4）
  - `jti` = `platform_oauth_token.token_id`（design §5.1）
  - 平台验签**强制校验 `aud`** 且 `AccessClaims` 的 `sid` **必填**
    → OAuth 令牌**过不了**平台登录令牌的那道门（= D209 的默认拒绝）

---

## AC-S-145: 令牌是 ES256 签的 JWT，第三方可凭 JWKS 自行验签

### TC-022-125: header 的 `alg` = `ES256`，`kid` 在 JWKS 里

- **关联 AC**: AC-S-145
- **前置条件**: 同本文件头
- **测试步骤**:
  1. `GET /.well-known/jwks.json` → 读 `keys[]`
  2. 测试侧**独立解码** `access_token` 的 header（第 1 段）
  3. 比对
- **预期结果**:
  1. JWKS 的 `keys[]` **非空**
  2. header 的 `alg` **逐字等于** `ES256`（**不是** `none` / `RS256` / `HS256`）
  3. header 的 `kid` **存在**且**逐字等于** JWKS 里某个 `keys[].kid`
  4. JWKS 里**不含**私钥材料（每个 `keys[]` **不含** `d` 字段）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合包含关系
- **备注**: AC 步骤 2 的原文。第 4 条是「JWKS 只暴露公钥」的安全断言
  （`platform-iam/src/jwt.rs` 的 `jwks()`）—— 第三方拿 JWKS 时**不能**拿到私钥。

### TC-022-126: payload 的 `iss` / `aud` 正确，`sub` / `tid` / `client_id` / `scope` 等 **齐全**

- **关联 AC**: AC-S-145
- **前置条件**: 同本文件头（令牌是 `authorization_code` 签发的）
- **测试步骤**:
  1. 测试侧**独立解码** payload（第 2 段）→ 解析 JSON
  2. 逐个读 claim
- **预期结果**:
  1. `iss` 逐字等于 `BASE_JWT_ISSUER`
  2. `aud` 逐字等于 `client-A` 的 `client_id`（**不是**平台 audience）
  3. `sub` / `tid` / `client_id` / `scope` / `grant_type` / `iat` / `nbf` / `exp` / `jti`
     **全部存在**（非空）
  4. `scope` 逐字等于该令牌被授予的 scope（空格分隔）
  5. `grant_type` 逐字等于 `authorization_code`
  6. `exp` / `iat` / `nbf` 都是**数字**（秒），且 `nbf <= iat < exp`
  7. `jti` 逐字等于 `platform_oauth_token.token_id`（那一行）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配
- **备注**: AC 步骤 2 的原文。
  ⚠️ **AC 前置必须写明令牌是哪种 grant** —— `client_credentials` 的令牌**没有** `sub`
  （D198 §4）→ 照 AC 字面会与 AC-S-141 冲突〔S-19〕。本用例固定用 `authorization_code`。
  第 7 条把 `jti` 与库里那行钉上（D210 的「撤销靠查表」依赖它）。

### TC-022-127: 测试侧**独立实现** ES256 验签 → **通过**

- **关联 AC**: AC-S-145
- **前置条件**: 同本文件头
- **测试步骤**:
  1. `GET /.well-known/jwks.json` → 按 `kid` 取公钥
  2. 测试侧**独立实现** ES256 验签（**不调用生产代码**：`platform-iam` / `jsonwebtoken` 都不许 import）
  3. 验签
- **预期结果**:
  1. 验签**通过**
  2. 测试代码里**不含**对 `platform_iam::jwt` 或 `jsonwebtoken` 的引用
     （可用 `grep` 断言测试文件的依赖声明）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 独立复算
- **备注**: AC 步骤 3 的原文（「**不调用生产代码**」是这条 AC 的**核心价值**——
  用生产代码验签 = 自己证明自己）。

### TC-022-128: **篡改** payload → 验签**失败**（且不得被当成「过期」）

- **关联 AC**: AC-S-145
- **前置条件**: 同本文件头
- **测试步骤**:
  1. 把 payload 第 2 段解码 → 把 `scope` 改大 → 重新编码 → 拼回 `header.payload.sig`
  2. 用 TC-022-127 的独立验签器验它
  3. 同时读该令牌的 `exp`
- **预期结果**:
  1. 验签**失败**
  2. 失败原因**不是**「令牌已过期」（`exp` 仍在未来）→ **「签名无效」与「已过期」是两个不同的断言**
  3. 篡改后的令牌**不能**通过 `POST /api/oauth/introspect` 得到 `active=true`
     （用 `client-A` 的 client 认证内省它 → `active=false`）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 反向变异
- **备注**: AC 步骤 4 的后半句（「**不得**把『签名无效』与『已过期』混为一谈」）。
  第 3 条补上「篡改的令牌**在内省端点也不认**」—— 这是「验签」在平台侧的**第二道**关。

### TC-022-129: **过期**令牌验签**通过**，但 `exp` 已过 → 结论是「已过期」

- **关联 AC**: AC-S-145
- **前置条件**: 有一枚 `exp` 已过去的令牌（用 TC-022-037 的 TTL 造法）
- **测试步骤**:
  1. 用 TC-022-127 的独立验签器验它
  2. 读它的 `exp` 与当前时间
- **预期结果**:
  1. 验签**通过**（**签名本身有效**）
  2. `exp` **小于**当前时间（已过期）
  3. 结论判据**分开**：断言「签名有效 == true」**且**「已过期 == true」→ **两条独立的断言**
  4. 该令牌在内省端点 → `active=false`
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（过期）
- **备注**: AC 步骤 4 的原文（「验签**通过**（签名有效）但 `exp` **已过期** → 结论是「已过期」」）。
  这与 TC-022-128 是一对：**两个方向**都要能判。

### TC-022-130: `alg=none` / 算法混淆的令牌被拒

- **关联 AC**: AC-S-145
- **前置条件**: 同本文件头
- **测试步骤**:
  1. 构造一个 `alg=none`、payload 为原 payload 的 JWT（签名段为空）
  2. 构造一个 `alg=HS256`、用公开 JWKS 里的坐标当 HMAC 密钥签的 JWT
  3. 分别调 `POST /api/oauth/introspect`（带 `client-A` 的 client 认证）
- **预期结果**:
  1. 两个令牌的 `active` **都**逐字等于 `false`
  2. 平台**不**因为 `alg` 是 `none` 就接受它
  3. 平台**不**因为 `kid` 对得上就用别的算法验
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: AC 没覆盖 → 补上。
  平台的验签用 `Validation::new(SIGNING_ALGORITHM)` **钉死 ES256**
  （`platform-iam/src/jwt.rs:150`）→ 这条是它的**行为**判据（dev-guide §9.13：
  判据要打在**真界面**上）。

### TC-022-131: OAuth 令牌**过不了**平台登录令牌的那道门（`aud` + `sid` 双重）

- **关联 AC**: AC-S-145
- **前置条件**: 同本文件头（有效 OAuth 令牌）
- **测试步骤**:
  1. 把 OAuth 令牌当**登录令牌**用 → `GET /api/rbac/me/permissions`
  2. 用测试侧**独立实现**的「平台登录令牌验签器」（`aud` = 平台 audience + 必填 `sid`）
     去解这枚 OAuth 令牌
- **预期结果**:
  1. 步骤 1：HTTP **401**（见 TC-022-103）
  2. 步骤 2：**验签失败**（`aud` 不匹配）
  3. 步骤 2：payload 里**没有** `sid` 键（**即使**验签过了也解不出 `AccessClaims`）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 否定断言
- **备注**: 〔S-05〕〔AC 修正〕 —— AC-S-151 说「底座业务端点默认拒绝 OAuth 令牌」，
  但**没给出「为什么必然拒」的机制判据**。本用例把**两道防线**都钉成可判的断言：
  ① `aud` 不匹配 → 验签失败；② `sid` 缺失 → `AccessClaims` 反序列化失败。
  → 这让「默认拒绝」不依赖「实现者恰好没做别的事」。
