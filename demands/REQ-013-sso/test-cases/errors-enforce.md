# 失败可诊断与强制 SSO（AC-S-020、AC-S-021）

- **覆盖 AC**: AC-S-020（11 条：TC-013-090 ~ 100）、AC-S-021（10 条：TC-013-101 ~ 110）
- **前置基线**:
  - `tenant-a` 配好 OIDC（`idp-a`）+ LDAP（mock 目录）；`tenant-b` 未配 SSO、`sso.enforce=false`
  - `tenant-a`：`sso.enforce=true`（TC-013-101 起）
  - 本地账号：`tenant-a` 的 `alice`（密码 `Al1ce-correct`）、
    `tenant-b` 的 `alice`（**同名、不同人、密码 `Al1ce-tb`**）
  - mock 端点可切换：`:9420` 返回 500；`:9421` 返回非法 JSON；
    `:9422` 返回合法 JSON 但缺 `token_endpoint`

---

## AC-S-020：两类协议的失败给出可区分的错误码

### TC-013-090: 发现文档返回 500 → 503 `SSO_OIDC_DISCOVERY_FAILED`

- **关联 AC**: AC-S-020
- **前置条件**: `sso.oidc.issuer=http://127.0.0.1:9420/idp-a`
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`，`x-tenant-id: tenant-a`
- **预期结果**:
  1. 状态码 `503`，`code == "SSO_OIDC_DISCOVERY_FAILED"`
  2. `detail` 含 `127.0.0.1:9420`（issuer 主机）
  3. `platform_iam_oidc_state` 新增 `0` 行（发现失败不得留下 state）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-091: 发现文档是非法 JSON → 502 `SSO_OIDC_DISCOVERY_INVALID`（与 090 可区分）

- **关联 AC**: AC-S-020
- **前置条件**: `issuer=http://127.0.0.1:9421/idp-a`（返回 `{"broken":`）
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`
- **预期结果**:
  1. 状态码 `502`，`code == "SSO_OIDC_DISCOVERY_INVALID"`
  2. 状态码 `!= 503` 且 `code !=` TC-013-090 的 `code`
  3. `title_of_code` 两个值不相等
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-092: 发现文档合法但缺 `token_endpoint` → 502 `SSO_OIDC_DISCOVERY_INVALID`

- **关联 AC**: AC-S-020
- **前置条件**: `issuer=http://127.0.0.1:9422/idp-a`（JSON 合法，只给 `authorization_endpoint`）
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`
- **预期结果**:
  1. 状态码 `502`，`code == "SSO_OIDC_DISCOVERY_INVALID"`
  2. `!= 200`（不得给出指向 `null` 的 `Location`）
  3. 无 `Location` 响应头
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值

### TC-013-093: 伪造 `code` → 400 `SSO_OIDC_TOKEN_EXCHANGE_FAILED`

- **关联 AC**: AC-S-020
- **前置条件**: 有效 `state`；mock `/token` 对未知 `code` 返回 `400 {"error":"invalid_grant"}`
- **测试步骤**:
  1. `GET /callback?code=bogus&state=<有效 state>`
- **预期结果**:
  1. 状态码 `400`，`code == "SSO_OIDC_TOKEN_EXCHANGE_FAILED"`
  2. 响应体不含 `id_token` / `accessToken`
  3. 该 `state` 行 `consumed_at` 非空（一次性，失败不退还）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-094: LDAP 指向未监听端口 → 503 `SSO_LDAP_UNREACHABLE`（码与 title 都不同）

- **关联 AC**: AC-S-020
- **前置条件**: `sso.ldap.url=ldap://127.0.0.1:9399`
- **测试步骤**:
  1. `POST /api/iam/sso/ldap/login`，`{"username":"erin","password":"x"}`
- **预期结果**:
  1. 状态码 `503`，`code == "SSO_LDAP_UNREACHABLE"`
  2. `code != "SSO_OIDC_DISCOVERY_FAILED"`
  3. `title_of_code("SSO_LDAP_UNREACHABLE") != title_of_code("SSO_OIDC_DISCOVERY_FAILED")`
  4. 响应体不含 `at crates/` / `RUST_BACKTRACE` 字样
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-095: LDAP 密码错 → 401 `SSO_LDAP_BIND_FAILED`，不泄露 DN / 目录原文

- **关联 AC**: AC-S-020
- **前置条件**: `erin` 存在
- **测试步骤**:
  1. `{"username":"erin","password":"nope"}`
  2. 在响应体里搜 DN 片段与目录错误码
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_LDAP_BIND_FAILED"`
  2. 不含 `ou=people`、不含 `dc=example`、不含 `invalidCredentials`、不含 `data 52e`
  3. 不含完整用户名 `erin`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-096: LDAP 不存在的用户 → 与 TC-013-095 **完全一致**

- **关联 AC**: AC-S-020
- **前置条件**: `ghost` 不在目录
- **测试步骤**:
  1. `{"username":"ghost","password":"nope"}`
  2. 与 TC-013-095 的响应体做字节比较
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_LDAP_BIND_FAILED"`
  2. 两个响应体字节相等
  3. 响应头集合相等（除 `date` / `x-request-id`）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-097: 本 AC 涉及的**5 个**码全部进 `ALL_CODES` 与 `TITLES`，`title` 为 ASCII

- **关联 AC**: AC-S-020
- **前置条件**: 代码基线
- **测试步骤**:
  1. 取集合 `{SSO_OIDC_DISCOVERY_FAILED, SSO_OIDC_DISCOVERY_INVALID,
     SSO_OIDC_TOKEN_EXCHANGE_FAILED, SSO_LDAP_UNREACHABLE, SSO_LDAP_BIND_FAILED}`
  2. 逐个断言在 `platform_iam::error::ALL_CODES` 中
  3. 逐个断言 `platform_core::titles::is_registered(code) == true`
  4. 逐个断言 `title_of_code(code).is_ascii()`
- **预期结果**:
  1. 集合大小 `== 5`
  2. 步骤 2/3/4 全部为真
  3. 任一项为假 → 该用例红
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: **AC 步骤 7 写「上述 6 个错误码」，但步骤 1~6 只产生 5 个不同的码**
  （步骤 5、6 共用 `SSO_LDAP_BIND_FAILED`）。数字自相矛盾。〔S10〕

### TC-013-098: `i18n_titles` 两条完备性测试通过（覆盖 + 无孤儿）

- **关联 AC**: AC-S-020
- **前置条件**: 17 个新码已加入 `ALL_CODES` 与 `TITLES`（D123 已删掉第 18 个 `SSO_OIDC_TLS_UNSUPPORTED`）
- **测试步骤**:
  1. `cargo test -p platform-server --test i18n_titles`
  2. 记录 `TITLES.len()`
- **预期结果**:
  1. `titles_cover_every_code` 通过
  2. `no_orphan_titles` 通过（不存在「`TITLES` 有、`ALL_CODES` 没有」的码）
  3. `every_title_is_ascii` / `keys_are_unique` 通过
  4. `TITLES.len() == 56 + 17 == 73`（当前 56 行，`crates/platform-core/src/titles.rs:45`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 第 4 条的 **73** 是「17 个码」的唯一自洽值。当前文档里仍有 4 处写 18
  （`design-summary.md:368`「18 个新变体」、`:383`「18 个新码」、`status.md:57`、`status.md:94`），
  而 §6 标题又写「13 个新增」、表下注写「17 个」—— 四处不一致。〔S11〕

### TC-013-099: `detail` 不含完整 URL 的 query（不把 issuer 的参数带出去）

- **关联 AC**: AC-S-020
- **前置条件**: `issuer=http://127.0.0.1:9420/idp-a?tenant=secret-tenant&sig=abc123`
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`
  2. 在响应体里搜 `secret-tenant`、`sig=abc123`、`tenant=secret-tenant`
- **预期结果**:
  1. 状态码 `503`，`code == "SSO_OIDC_DISCOVERY_FAILED"`
  2. 三个子串命中数均为 `0`
  3. `detail` 含 `127.0.0.1:9420`（主机可留，用于排查）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值

### TC-013-100: 六个失败响应的 RFC 9457 形状一致

- **关联 AC**: AC-S-020
- **前置条件**: TC-013-090 ~ 096 的六个响应
- **测试步骤**:
  1. 对每个响应取顶层键集合
  2. 比较 `status` 字段与 HTTP 状态行
- **预期结果**:
  1. 每个响应的键集合 `⊇` `{type, title, status, detail, instance}`
  2. 每个 `status` 是整数且**等于** HTTP 状态行（`503/502/400/401`）
  3. 每个 `title` 是 ASCII 且 `!=` `code`
  4. 每个 `instance` 等于请求路径
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 判定表

---

## AC-S-021：强制 SSO 生效且不能用于账号枚举

### TC-013-101: `sso.enforce=true` 时正确密码也返回 403 `SSO_REQUIRED`

- **关联 AC**: AC-S-021
- **前置条件**: `tenant-a` 的 `sso.enforce=true`；`alice` 密码正确 `Al1ce-correct`
- **测试步骤**:
  1. `POST /api/iam/login`，`{"username":"alice","password":"Al1ce-correct"}`，`x-tenant-id: tenant-a`
- **预期结果**:
  1. 状态码 `403`，`code == "SSO_REQUIRED"`
  2. 响应体含 `ssoProviders`，其长度 `== 2`（oidc + ldap）
  3. `platform_iam_session` 新增 `0` 行
  4. 响应体不含 `accessToken`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: `ssoProviders` 是 `ProblemDetails` 的扩展字段（`params` 或顶层），
  AC 说「响应体带 `ssoProviders`」但没说是哪个容器；本用例只断言「响应体能取到该数组」，
  BA 需钉死字段位置。〔S38 关联〕

### TC-013-102: 错误密码与正确密码 —— 响应体完全相同

- **关联 AC**: AC-S-021
- **前置条件**: 同上
- **测试步骤**:
  1. `{"username":"alice","password":"wrong"}`
  2. 与 TC-013-101 的响应体做字节比较
- **预期结果**:
  1. 状态码 `403`，`code == "SSO_REQUIRED"`
  2. 两个响应体字节相等
  3. `ssoProviders` 数组内容完全相同
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-103: 不存在的账号 —— 响应体与 TC-013-101 完全相同

- **关联 AC**: AC-S-021
- **前置条件**: `nobody` 不存在
- **测试步骤**:
  1. `{"username":"nobody","password":"whatever"}`
  2. 与 TC-013-101 比对
- **预期结果**:
  1. 状态码 `403`，`code == "SSO_REQUIRED"`
  2. 响应体字节相等
  3. `code != "INVALID_CREDENTIALS"`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-104: 响应体不含任何账号存在性信息

- **关联 AC**: AC-S-021
- **前置条件**: 同 TC-013-101/102/103 三个响应
- **测试步骤**:
  1. 检查三个响应体是否含 `alice`、`nobody`、`username` 键、`accountExists` 类键
  2. 检查 `params` 容器
- **预期结果**:
  1. 三个响应体均不含 `alice` 与 `nobody`
  2. 三个响应体均不含键名 `username` / `accountStatus` / `locked`
  3. `params` 容器（若有）只含 `ssoProviders`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-105: Argon2id **调用次数为 0**（取代 AC 的耗时判据）

- **关联 AC**: AC-S-021
- **前置条件**: 测试构建注入一个 `CountingPasswordVerifier`（包装 `password::verify_password`，
  原子计数）；`sso.enforce=true`
- **测试步骤**:
  1. 计数归零
  2. 连发 3 次 `POST /api/iam/login`（正确密码 / 错误密码 / 不存在账号）
  3. 读计数
- **预期结果**:
  1. 三次均 `403 SSO_REQUIRED`
  2. **计数 `== 0`**
  3. 对照组：把 `sso.enforce` 改 `false`，同一组三次请求 → 计数 `== 3`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 判定表
- **备注**: **AC 步骤 3 的「耗时同量级（1.5× 内）」是不可判定/假绿的**：三次请求走的是
  **同一条早退路径**，即使实现每次都跑一遍 Argon2id，三个耗时也同量级、仍然通过。
  耗时要能判伪，必须有「跑了 Argon2id 的基线」——而那正好是第二个对照组。
  本用例把判据换成计数（可精确判定）。〔S15〕

### TC-013-106: `sso.enforce` 是租户级 —— `tenant-b` 的 `alice` 仍能本地登录

- **关联 AC**: AC-S-021
- **前置条件**: `tenant-b` 有**自己**的 `alice`（密码 `Al1ce-tb`），`sso.enforce=false`
- **测试步骤**:
  1. `POST /api/iam/login`，`{"username":"alice","password":"Al1ce-tb"}`，`x-tenant-id: tenant-b`
- **预期结果**:
  1. 状态码 `200`
  2. `user.id !=` `tenant-a` 的 alice 的 id
  3. `user.tenantId == "tenant-b"`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正交
- **备注**: **AC 步骤 4 的前置不成立**：AC 只写「`tenant-b` 未配 SSO」，
  但 §前置条件里 `alice` 是 `tenant-a` 的账号。用户名按租户隔离
  （`find_user_by_username` 用 `ctx.tenant_id()`，`crates/platform-iam/src/service.rs:388-392`），
  `tenant-b` 没有同名账号时只会得到 `401 INVALID_CREDENTIALS`。
  本用例补了 `tenant-b` 的 `alice` 前置。〔S9〕

### TC-013-107: 强制 SSO 期间 OIDC 登录仍可用

- **关联 AC**: AC-S-021
- **前置条件**: `sso.enforce=true`；`bob` 已绑定
- **测试步骤**:
  1. 完整走一次 OIDC 登录
  2. 与本地登录响应比较键集合
- **预期结果**:
  1. 状态码 `200`
  2. 键集合 `== {accessToken, refreshToken, tokenType, expiresIn, user, permissions}`
  3. `user.id` 是 `bob`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-108: 关掉 `sso.enforce` 后本地登录恢复

- **关联 AC**: AC-S-021
- **前置条件**: 把 `tenant-a` 的 `sso.enforce` 改回 `false`
- **测试步骤**:
  1. `POST /api/iam/login`，`alice` + `Al1ce-correct`
  2. 再试一次错误密码
- **预期结果**:
  1. 步骤 1 `200`，返回 `accessToken`
  2. 步骤 2 `401`，`code == "INVALID_CREDENTIALS"`
  3. 步骤 2 的 `code != "SSO_REQUIRED"`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移

### TC-013-109: 顺序判别 —— 无该用户的租户开启 enforce 时返回 403 而不是 401

- **关联 AC**: AC-S-021
- **前置条件**: `tenant-a` 的 user 表**清空**（`DELETE FROM platform_iam_user WHERE tenant_id='tenant-a'`）；
  `sso.enforce=true`
- **测试步骤**:
  1. `POST /api/iam/login`，`{"username":"anyone","password":"x"}`
- **预期结果**:
  1. 状态码 `403`，`code == "SSO_REQUIRED"`
  2. **不是** `401 INVALID_CREDENTIALS`
  3. 该请求未产生 `platform_iam_login_attempt` 行（早于「查账号」）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 这是 AC「在加载用户之前拒绝」的**可机械判据**形式；
  AC 原文的「加一句代码位置断言」不是测试能判定的事。〔S38〕

### TC-013-110: `sso.enforce` 的读取失败必须 fail-closed

- **关联 AC**: AC-S-021
- **前置条件**: 使配置解析器返回错误（例：`DROP` 掉 `platform_config` 的读权限，或注入一个
  总是返回 `Err` 的 `ConfigResolver`）
- **测试步骤**:
  1. `POST /api/iam/login`（正确密码）
  2. 再用错误密码试一次
- **预期结果**:
  1. 两次都**不得** `200`
  2. 两次状态码相同（`503` 或 `500`，BA 需钉死）
  3. 响应 `code ∈ {INTERNAL_ERROR}` 或专用的「配置不可用」码
  4. `platform_iam_session` 新增 `0` 行
- **类型**: 安全
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 判定表
- **备注**: `〔守卫〕` —— 六条安全约束**没有一条**覆盖「enforce 读不出来时怎么办」。
  若实现把配置读错误当成「enforce=false」，攻击者只需让配置读失败（或在一个配置尚未同步的
  新实例上打）就能绕过强制 SSO。AC 必须写明 fail-closed。〔S16〕
