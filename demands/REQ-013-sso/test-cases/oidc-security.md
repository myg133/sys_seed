# OIDC state / nonce / PKCE 与 `id_token` 验签（AC-S-101、AC-S-102）

- **覆盖 AC**: AC-S-101（12 条：TC-013-013 ~ 024）、AC-S-102（14 条：TC-013-025 ~ 038）
- **前置基线**: 同 [oidc-flow.md](oidc-flow.md) 的租户与 mock IdP；额外：
  - mock IdP 支持两个测试专用开关：
    `POST /__idp/override`（`{"iss":"…","aud":"…","exp":"past","nonce":"other","alg":"none|HS256|RS512","kid":"…","sign_with":"attacker"}`）
    与 `POST /__idp/reset`
  - mock IdP 的 `/jwks` **自带请求计数器**（`__stats.jwks`）
  - 攻击者密钥对（RSA-2048）与私钥在测试夹具里，只用于构造伪造 token
- **`state` TTL**: 默认 300 秒；过期用例靠 `UPDATE oidc_state SET expires_at = now() - 1 second`
  **构造**，不依赖「等 5 分钟」——`sso.oidc.*` 的 12 个配置键里**没有** TTL 键
  （design §7 表），AC-S-101 步骤 2 的「测试环境可配」没有落地物。〔AC 修正 S26〕

---

## AC-S-101：`state` / `nonce` / PKCE

### TC-013-013: `state` 一次性 —— 同一 `state` 第二次回调必须失败，且库里已标记消费

- **关联 AC**: AC-S-101
- **前置条件**: 按 TC-013-001 取得有效 `state`；mock IdP 允许同一 `code` 换两次 token（用于区分「state 拦下」与「code 被拒」）
- **测试步骤**:
  1. `GET /callback?code=CODE-BOB-1&state=<state>`（第一次）
  2. 立刻用**同一** `state` 再回调一次
  3. 查 `platform_iam_oidc_state` 中该 `state` 行
- **预期结果**:
  1. 步骤 1 状态码 `200`
  2. 步骤 2 状态码 `400`，`code == "SSO_OIDC_STATE_INVALID"`
  3. 步骤 2 的响应体不含任何 token 字段（无 `accessToken` 键）
  4. 步骤 3 该行 `consumed_at IS NOT NULL`
  5. 该 `state` 的行数 `== 1`（不是插入了一条新行）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移

### TC-013-014: `state` 过期与「无效」是可区分的两个码

- **关联 AC**: AC-S-101
- **前置条件**: 按 TC-013-001 取得有效 `state`；然后
  `UPDATE platform_iam_oidc_state SET expires_at = now() - interval '1 second' WHERE state='<state>'`
- **测试步骤**:
  1. 用该 `state` 回调
  2. 再取一个**不存在**的 `state`（`"A"*43`）回调
- **预期结果**:
  1. 步骤 1 状态码 `400`，`code == "SSO_OIDC_STATE_EXPIRED"`
  2. 步骤 2 状态码 `400`，`code == "SSO_OIDC_STATE_INVALID"`
  3. 两次的 `code` **字符串不相等**（可区分）
  4. 步骤 1 之后 mock IdP 的 `token` 计数 `== 0`（过期判定在换 token 之前）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-015: 不存在的 `state` 不产生 500，且不触发任何出站请求

- **关联 AC**: AC-S-101
- **前置条件**: 计数器归零
- **测试步骤**:
  1. `GET /callback?code=x&state=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA`
- **预期结果**:
  1. 状态码 `400`，`code == "SSO_OIDC_STATE_INVALID"`
  2. `__stats.token == 0` 且 `__stats.jwks == 0`（无出站调用）
  3. 状态码 `!= 500`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值

### TC-013-016: 跨租户重放 —— `tenant-b` 的 `state` 到 `tenant-a` 回调被拒

- **关联 AC**: AC-S-101
- **前置条件**: `tenant-b` 配好同一 mock IdP（`sso.oidc.enabled=true`、`clientId=base-portal-b`）；
  `tenant-b` 发一次 authorize 拿到 `state_b`
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/callback?code=CODE-BOB-1&state=<state_b>`，头 `x-tenant-id: tenant-a`
  2. 再打一次，**不带** `x-tenant-id`
- **预期结果**:
  1. 步骤 1 状态码 `400`，`code == "SSO_OIDC_STATE_INVALID"`
  2. 步骤 1 响应体**不含** `"tenant-a"` / `"tenant-b"` 任一字符串
  3. 步骤 2 状态码 `400`，`code == "SSO_OIDC_STATE_INVALID"`
     （`state` 自身携带租户，缺头不能「借用」请求方租户）
  4. 步骤 1/2 均**不返回** `403` / `404`（不泄露租户存在性）
  5. 步骤 2 若成功则红：`state` 携带的租户必须被采用，而不是「无租户 → 走默认租户」
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 步骤 2 的语义是 AC 的空白点：design §2.1 写「`state.tenant_id` 必须等于请求头租户」，
  但浏览器从 IdP 跳回时**不带** `x-tenant-id`（AC-S-100 步骤 3 即如此）。
  「无头 → 采纳 state 的租户」与「无头 → 拒绝」是两种不同实现，AC 必须二选一。
  〔AC 修正 S12〕

### TC-013-017: `state` 绑定 provider（用 DB 篡改 provider 列验证）

- **关联 AC**: AC-S-101
- **前置条件**: 按 TC-013-001 取得有效 `state`；`UPDATE platform_iam_oidc_state SET provider='saml' WHERE state='<state>'`
- **测试步骤**:
  1. 用该 `state` 回调
- **预期结果**:
  1. 状态码 `400`，`code == "SSO_OIDC_STATE_INVALID"`
  2. mock `token` 计数 `== 0`
  3. 该行 `consumed_at` 仍为 `NULL`（**没有**把别人的 state 消费掉）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: AC-S-101 步骤 4「用 OIDC 的 `state` 去打 **LDAP** 的登录接口」**物理不成立** ——
  `POST /api/iam/sso/ldap/login` 的 body 只有 `{username,password}`，不读任何 `state` 参数
  （design §3 端点表）。本用例用 DB 侧篡改 `provider` 列达到同一目的，这是唯一可执行的形式。
  〔AC 修正 S4〕

### TC-013-018: `nonce` 不匹配 → 401，且码与 `TOKEN_INVALID` 不同

- **关联 AC**: AC-S-101
- **前置条件**: 有效 `state`；`POST /__idp/override {"nonce":"attacker-nonce"}`
- **测试步骤**:
  1. 用该 `state` 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_NONCE_MISMATCH"`
  2. 状态码 `!= 400`（这是断言失败而非请求格式错误）
  3. 库中该 `state` 的 `consumed_at` 非空（失败也算消费，不可重放）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-019: `id_token` 缺 `nonce` claim → 401，不得 200

- **关联 AC**: AC-S-101
- **前置条件**: 有效 `state`；`POST /__idp/override {"omit_nonce":true}`
- **测试步骤**:
  1. 用该 `state` 回调
- **预期结果**:
  1. 状态码 `401`
  2. `code` ∈ `{"SSO_OIDC_NONCE_MISMATCH", "SSO_OIDC_TOKEN_INVALID"}` **之一**
     （design §4.2-5 把 `nonce` 列为必校 claim，但没说是哪个码）
  3. 不返回任何 token 字段
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: `〔守卫〕` —— 两个码都算过，是因为 AC/design 未定；一旦 BA 钉死，本用例改成单值。

### TC-013-020: PKCE 校验失败 → 400 `SSO_OIDC_PKCE_MISMATCH`

- **关联 AC**: AC-S-101
- **前置条件**: 有效 `state`；mock 的 `/token` 被配置为「总是返回
  `400 {"error":"invalid_grant","error_description":"code_verifier does not match"}`
  但**仅在** `code_verifier` 与 challenge 不符时」——用真实 PKCE 校验实现，不用硬编码
- **测试步骤**:
  1. `UPDATE platform_iam_oidc_state SET code_verifier='wrong-verifier-wrong-verifier-wrong'`（把存库的 verifier 换掉）
  2. 用该 `state` 回调
- **预期结果**:
  1. 状态码 `400`，`code == "SSO_OIDC_PKCE_MISMATCH"`
  2. `!= 200`，且响应体不含 `accessToken`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: AC 步骤 6 写「回调时把 `code_verifier` 换成随机的另一个值」——**客户端拿不到 verifier**
  （它存服务端 `oidc_state.code_verifier`，design §2.1）。唯一可执行方式是在 DB 侧替换。
  〔AC 修正 S5〕

### TC-013-021: `state` 参数是非 base64url 垃圾串 → 400，不 500、不查库异常

- **关联 AC**: AC-S-101
- **前置条件**: 无
- **测试步骤**:
  1. `GET /callback?code=x&state=` + 2000 个 `'%00'`-类非法字符（`"\u{0}"` 用一个百分号编码串代替）
  2. `GET /callback?code=x&state=` + 10 KB 的 `'A'`
- **预期结果**:
  1. 两次均 `400`，`code == "SSO_OIDC_STATE_INVALID"`
  2. 两次均 `!= 500`
  3. 响应体长度 `< 4096` 字节（不回显超长输入）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值

### TC-013-022: 并发双回调同一 `state` → 恰好 1 次成功

- **关联 AC**: AC-S-101
- **前置条件**: 有效 `state`；mock `/token` 对同一 `code` 每次返回合法 `id_token`（不做一次性）
- **测试步骤**:
  1. 用 `tokio::join!` **同时**发出两个 `GET /callback?code=CODE-BOB-1&state=<state>`
  2. 统计状态码
- **预期结果**:
  1. `200` 的数量 `== 1`
  2. `400`（`SSO_OIDC_STATE_INVALID`）的数量 `== 1`
  3. `platform_iam_session` 新增行数 `== 1`
  4. 该 `state` 行数 `== 1`，`consumed_at` 非空
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: 依赖 design §2.1 的 `SELECT … FOR UPDATE` + `UPDATE consumed_at`；
  没有行锁时本用例会稳定红（两个都 200 或两个都 400）。

### TC-013-023: 过期记录有清理路径，且不存在「同一 `state` 两条有效记录」

- **关联 AC**: AC-S-101
- **前置条件**: 插入 10 行 `expires_at = now() - interval '10 minutes'`、`consumed_at IS NULL` 的假记录
- **测试步骤**:
  1. 触发清理（测试专用 `POST /__maintenance/purge_oidc_state`，或等待后台任务一个周期）
  2. 查表
- **预期结果**:
  1. 步骤 1 后 `SELECT count(*) FROM platform_iam_oidc_state WHERE expires_at < now()` → `0`
  2. 全表 `SELECT state, count(*) … GROUP BY state HAVING count(*) > 1` → `0` 行
  3. 清理后仍可用新 `state` 正常登录（`200`）——清理不得误删未过期行
- **类型**: 功能
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 状态迁移
- **备注**: AC 只写「有清理路径（不无限增长）」，没给触发方式；本用例要求实现暴露一个可调用入口
  （后台任务在测试里不可控）。若实现选择「只在读路径懒清理」，应把步骤 1 改成
  「发一次 authorize 后再查」。〔AC 修正 S26 关联〕

### TC-013-024: `state` 的熵与字符集可机械判定

- **关联 AC**: AC-S-101
- **前置条件**: 连续发起 20 次 authorize
- **测试步骤**:
  1. 收集 20 个 `state`
  2. 用正则 `^[A-Za-z0-9_-]{43,}$` 校验
  3. 去重后计数
- **预期结果**:
  1. 20 个值全部匹配该正则（32 字节 base64url 最短 43 字符）
  2. 去重后 `== 20`（无重复）
  3. `nonce` 同样满足上述两条
- **类型**: 安全
- **优先级**: P2
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: AC 步骤 2 只写「`state` 非空且高熵」，**不可判定**；本用例把「高熵」落成
  「长度 + 字符集 + 20 次不重复」。〔AC 修正 S38〕

---

## AC-S-102：`id_token` 验签与算法白名单

> 本节全部用例走**同一条**入口：先拿有效 `state`，再让 mock 返回被篡改的 `id_token`。
> 判定基线：`jsonwebtoken` 10.4.0（`rust_crypto` 后端）在 `Validation.algorithms={RS256,ES256}` 下
> 会先拒 `alg`、后验签、再校 claim —— 因此「`alg` 类失败」与「claim 类失败」的**码必须分开**，
> 混成一个码就是不可区分（`crates/platform-iam/src/jwt.rs:141-171` 是本地 token 的对照实现）。

### TC-013-025: `iss` 不匹配 → 401 `SSO_OIDC_TOKEN_INVALID`

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；`POST /__idp/override {"iss":"http://evil.example"}`（用合法私钥签名）
- **测试步骤**:
  1. 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_TOKEN_INVALID"`
  2. 响应体不含 `accessToken`
  3. `platform_iam_session` 新增 `0` 行
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-026: `aud` 不匹配 → 401 `SSO_OIDC_TOKEN_INVALID`

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；`{"aud":"another-client"}`
- **测试步骤**:
  1. 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_TOKEN_INVALID"`
  2. 与 TC-013-025 的 `code` 相同（同一类失败同一码）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-027: `exp` 已过 → 401 `SSO_OIDC_TOKEN_EXPIRED`（与「无效」可区分）

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；`{"exp":"now-3600s"}`
- **测试步骤**:
  1. 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_TOKEN_EXPIRED"`
  2. 该 `code` `!=` TC-013-025 的 `code`
  3. 状态码同为 `401`（码不同、HTTP 相同）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-028: `alg: none` 的无签名 token 被拒

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；mock 返回手工拼的 `base64url(header{"alg":"none"}) + "." + base64url(payload) + "."`（空签名段）
- **测试步骤**:
  1. 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_ALG_NOT_ALLOWED"`
  2. `!= 200`；`platform_iam_session` 新增 `0` 行
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: `jsonwebtoken` 10 的 `Algorithm` 无 `None` 变体，`alg=none` 会在 header 解码阶段失败
  → 实现**必须显式把该错误映射**到 `SSO_OIDC_ALG_NOT_ALLOWED`，否则会落到 `TOKEN_INVALID`。

### TC-013-029: RS→HS 混淆（`HS256` + 公钥 PEM 内容作 HMAC 密钥）被拒

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；从 mock `/jwks` 取 RSA 公钥，转成 PEM，
  用该 **PEM 字节** 作 HMAC-SHA256 密钥签一个 `alg=HS256` 的 `id_token`（`kid` 与真实公钥相同）
- **测试步骤**:
  1. 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_ALG_NOT_ALLOWED"`
  2. `!= 200`；不产生会话
  3. 该请求**未**触发 `jwks` 刷新（`__stats.jwks` 增量 `0`）——`alg` 判定早于取密钥
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 这是「用公钥当共享密钥」的经典混淆。若实现按 token 头里的 `alg` 选 `DecodingKey::from_secret`
  （而不是固定 `from_rsa_components`），本用例必红。

### TC-013-030: 白名单外算法（`RS512`）被拒

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；用真实私钥签一个 `alg=RS512` 的 `id_token`（签名本身正确）
- **测试步骤**:
  1. 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_ALG_NOT_ALLOWED"`
  2. 与 TC-013-028 / 029 同码（同一类失败）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-031: 篡改 payload 保留原签名 → 401 `SSO_OIDC_TOKEN_INVALID`

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；取一个合法 token，把 payload 的 `email` 改成 `attacker@evil.example`，
  **保留原签名段**
- **测试步骤**:
  1. 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_TOKEN_INVALID"`
  2. `platform_iam_user` 中 `email='attacker@evil.example'` 的行数 `== 0`
     （签名先于业务字段，篡改的 `email` 不得进入任何匹配逻辑）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-032: `jku` 头指向攻击者 JWKS —— 服务端不得去取

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；攻击者 JWKS 起在 `http://127.0.0.1:9499/jwks`（带请求计数器）；
  token 头含 `{"alg":"RS256","kid":"k1","jku":"http://127.0.0.1:9499/jwks"}`，
  用攻击者私钥签名（`kid` 在 `jku` 里存在，在真实 JWKS 里不存在）
- **测试步骤**:
  1. 回调
  2. 读攻击者 JWKS 的计数器
- **预期结果**:
  1. 状态码 `401`（`SSO_OIDC_TOKEN_INVALID`：未知 `kid` 刷新后仍找不到）
  2. 攻击者 JWKS 计数器 `== 0`
  3. 真实 mock `jwks` 计数增量 `== 1`（只对**配置的** `jwks_uri` 刷新一次）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: design §4.2-2 明写「只从 `jwks_uri` 取公钥，不从 token 自带的 `x5c`/`jku` 取」，
  **但 12 条 AC 里没有任何一条守这条规则**。本用例是补的缺口。〔S17〕

### TC-013-033: `x5c` 头携带攻击者公钥 —— 不得用来自验证

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；token 头含 `{"alg":"RS256","kid":"<真实 kid>","x5c":[<攻击者公钥 DER base64>]}`，
  用**攻击者私钥**签名
- **测试步骤**:
  1. 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_TOKEN_INVALID"`（签名与 `jwks_uri` 里的公钥不符）
  2. `!= 200`
  3. 攻击者 JWKS 计数器 `== 0`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-034: 未知 `kid`（IdP 轮换）→ 刷新一次后登录成功

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；mock `/jwks` 已切到新密钥对（`kid=k2`），服务端缓存里只有 `k1`；
  `__stats.jwks` 归零
- **测试步骤**:
  1. 回调
  2. 读 `__stats.jwks`
- **预期结果**:
  1. 状态码 `200`，返回 `accessToken`
  2. `__stats.jwks == 1`（**恰好一次**强制刷新，不是 2 次）
  3. 再走一次完整登录（新的 `state` + 新 token，`kid=k2` 已缓存）→ `200` 且 `__stats.jwks` 仍 `== 1`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移
- **备注**: 「刷新几次」是 AC 没写的判据；只断言 200 无法区分「缓存生效」与「每请求都刷」。
  〔S17 关联〕

### TC-013-035: 未知 `kid` 刷新后仍找不到 → 401 `SSO_OIDC_TOKEN_INVALID`

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；token `kid=k-never-exists`，用任意密钥签名
- **测试步骤**:
  1. 回调
  2. 读 `__stats.jwks`
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_TOKEN_INVALID"`
  2. `__stats.jwks` 增量 `== 1`（刷新过且只一次）
  3. `!= 500`
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: AC 未写此分支，design §4.2-2 写了（「刷新后仍找不到 → 401」）。〔守卫〕

### TC-013-036: `iss` 要求**逐字相等** —— 尾部斜杠差异必须被拒

- **关联 AC**: AC-S-102
- **前置条件**: 配置 `issuer=http://127.0.0.1:9410/idp-a`（无尾斜杠）；
  mock 返回 `iss=http://127.0.0.1:9410/idp-a/`（有尾斜杠），签名合法
- **测试步骤**:
  1. 回调
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_OIDC_TOKEN_INVALID"`
  2. 反向对照：`iss` 与配置**逐字相同**时同一套夹具返回 `200`
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: design §4.2-5 写「逐字相等」；`jsonwebtoken` 的 `validation.iss` 是集合精确匹配，
  本用例守住「不要用 `starts_with`」。

### TC-013-037: `iat` 在未来 —— 小 skew 内接受、超 skew 拒绝（边界）

- **关联 AC**: AC-S-102
- **前置条件**: 有效 `state`；`skew` 取 design §4.2-5 的 60 秒
- **测试步骤**:
  1. `{"iat":"now+30s"}` 回调
  2. 新的 `state` + `{"iat":"now+120s"}` 回调
- **预期结果**:
  1. 步骤 1 状态码 `200`
  2. 步骤 2 状态码 `401`，`code == "SSO_OIDC_TOKEN_INVALID"`
  3. 步骤 2 的 `code != "SSO_OIDC_TOKEN_EXPIRED"`（方向不同，码不得混）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值

### TC-013-038: 五类失败码全部进 `ALL_CODES` 与 `TITLES`，且 `title` 为 ASCII

- **关联 AC**: AC-S-102
- **前置条件**: 代码基线；`platform-iam` 已加 17 个新变体（D123 后删去 `SSO_OIDC_TLS_UNSUPPORTED`）
- **测试步骤**:
  1. 收集本 AC 涉及的 5 个码：`SSO_OIDC_TOKEN_INVALID` / `SSO_OIDC_TOKEN_EXPIRED` /
     `SSO_OIDC_ALG_NOT_ALLOWED` / `SSO_OIDC_NONCE_MISMATCH` / `SSO_OIDC_PKCE_MISMATCH`
  2. 断言每个码在 `platform_iam::error::ALL_CODES` 中
  3. 断言每个码在 `platform_core::titles::TITLES` 中，且 `is_registered(code) == true`
  4. 断言每个 `title_of_code(code)` 满足 `is_ascii()` 且 `!= FALLBACK_TITLE` 且 `!= code`
  5. 跑 `cargo test -p platform-server --test i18n_titles`
- **预期结果**:
  1. 步骤 2 缺任一码 → 红
  2. 步骤 3 缺任一码 → 红；取到 `FALLBACK_TITLE` → 红
  3. 步骤 4 任一 `title` 含非 ASCII 字符 → 红
  4. 步骤 5 的 `titles_cover_every_code` 与 `no_orphan_titles` **两条都通过**
  5. `TITLES` 行数 `== 56 + 新增码数`（当前 56 行，见 `crates/platform-core/src/titles.rs:45`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: `no_orphan_titles`（`crates/platform-server/tests/i18n_titles.rs:135`）会拒绝
  「`TITLES` 里有、`ALL_CODES` 里没有」的码。D123 后 `SSO_OIDC_TLS_UNSUPPORTED` 已被删除，
  **不得**再登记进 `TITLES`（否则本用例红）。当前文档仍有 4 处写「18 个」——见挑刺 S11。
