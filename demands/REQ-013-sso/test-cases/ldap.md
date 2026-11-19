# LDAP bind 认证（AC-S-104、AC-S-105）

- **覆盖 AC**: AC-S-104（14 条：TC-013-039 ~ 052）、AC-S-105（8 条：TC-013-053 ~ 060）
- **前置基线**:
  - 租户 `tenant-a`：`sso.ldap.enabled=true`、`sso.ldap.url=ldap://127.0.0.1:9389`、
    `sso.ldap.baseDn=ou=people,dc=example,dc=com`、
    `sso.ldap.userSearchFilter=(uid={username})`、`sso.ldap.mailAttribute=mail`、
    `sso.enforce=false`、`sso.jitProvisioning=false`
  - **mock 目录服务**：必须提供三个计数器 `__stats = {connect:n, bind:n, search:n}`
    与 `last_filter`
    > ⚠️ **该夹具在树里不存在，必须新建。** `ldap3` 0.11.5 **没有** in-memory 服务端
    > （`src/` 只有 `conn/ldap/protocol/result/search/sync/…`，无 server 实现）；
    > `platform-org` 的 `InMemoryLdapSource` 只实现 `search`，不实现 bind
    > （`crates/platform-org/src/ldap.rs:300-315`）。AC-S-104 前置写的
    > 「`ldap3` 的 in-memory 替身」**是空指**。〔S6〕
    > 可行实现：真实 OpenLDAP 容器 + 一层记录 BIND op 的 TCP 代理，或自写最小 LDAP stub。
  - 目录条目：`uid=erin,ou=people,dc=example,dc=com`（密码 `Er1n-pass`，`mail=erin@example.com`）；
    `uid=frank,ou=people,dc=example,dc=com`（密码 `Fr4nk-ldap`，`mail=frank@example.com`）
  - 本地账号：`frank`（本地密码 `Fr4nk-local`）、`alice`
  - 所有失败用例的请求都带 `x-forwarded-for: 198.51.100.7`（限流按该头计价，
    缺头时 `ip` 为空 → **完全跳过限流**，见 `crates/platform-iam/src/service.rs:544-547`）

---

## AC-S-104：bind 成功 / 失败 / 不可达 / 空密码

### TC-013-039: 正确密码 bind 成功，返回与本地登录同构的 `LoginResponse`

- **关联 AC**: AC-S-104
- **前置条件**: 同基线；计数器归零
- **测试步骤**:
  1. `POST /api/iam/sso/ldap/login`，body `{"username":"erin","password":"Er1n-pass"}`，
     头 `x-tenant-id: tenant-a` + `x-forwarded-for: 198.51.100.7`
  2. 取响应顶层键集合
- **预期结果**:
  1. 状态码 `200`
  2. 键集合 `==` `{accessToken, refreshToken, tokenType, expiresIn, user, permissions}`
  3. `user.username` 有值且 `user.tenantId == "tenant-a"`
  4. `__stats.bind == 1`（先服务账号 bind 1 次 + 用户 bind 1 次时应为 `2`；
     **断言值与实现约定一致并在本用例里写死**：若实现用匿名 search，则 `1`）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: 步骤 4 的歧义来自 design §5 步骤 3「用**服务账号** bind（若要 search）」——
  「若要」让 bind 次数变成 1 或 2，不确定就无法判定。BA 必须钉死。〔S32〕

### TC-013-040: 错误密码 → 401 `SSO_LDAP_BIND_FAILED`，不泄露 DN / 目录原文

- **关联 AC**: AC-S-104
- **前置条件**: `erin` 存在
- **测试步骤**:
  1. `POST /api/iam/sso/ldap/login`，`{"username":"erin","password":"wrong-password"}`
  2. 把响应体转成字符串检查
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_LDAP_BIND_FAILED"`
  2. 响应体**不含** `"ou=people"`、不含 `"dc=example"`、不含 `"invalidCredentials"`
  3. 响应体**不含** `"erin"`（用户名不回显）
  4. `platform_iam_session` 新增 `0` 行
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-041: 不存在的用户与错误密码 —— 响应体**逐字节相同**

- **关联 AC**: AC-S-104
- **前置条件**: `ghost` 不在目录里
- **测试步骤**:
  1. 请求 A：`{"username":"erin","password":"wrong-password"}`
  2. 请求 B：`{"username":"ghost","password":"whatever-1"}`
  3. 对比两个响应体字节（`instance` 字段若含路径，两次相同）
  4. 对比 `x-request-id` 之外的**全部**响应头
- **预期结果**:
  1. 两次状态码都是 `401`
  2. 两次 `code` 都是 `"SSO_LDAP_BIND_FAILED"`
  3. 两个响应体的 `serde_json` 序列化结果**字节相等**
  4. 响应头集合相等（允许 `date` / `x-request-id` 不同）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-042: 空密码（`""`）+ 存在用户 → 401 `SSO_LDAP_EMPTY_PASSWORD`，**bind 计数 0**

- **关联 AC**: AC-S-104
- **前置条件**: 计数器归零
- **测试步骤**:
  1. `{"username":"erin","password":""}`
  2. 读 `__stats`
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_LDAP_EMPTY_PASSWORD"`
  2. `__stats.bind == 0`
  3. `__stats.search == 0`（连 search 都不该发生：拦截在入口）
  4. `__stats.connect == 0`（**连 socket 都不该建** —— design §5.1 要求「连接之前」）
  5. `code != "SSO_LDAP_BIND_FAILED"`（独立码，便于运维识别攻击）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: 步骤 3/4 比 AC 更严：AC 只要求「0 条 bind」，但 design §5.1 写的是「**连接之前**」。
  只断 bind 会漏掉「先连上再拒」的实现（连接会留下服务端日志/计费）。〔补充〕

### TC-013-043: 空密码 + 不存在的用户 → 与 TC-013-042 **同码同体**

- **关联 AC**: AC-S-104
- **前置条件**: 同上
- **测试步骤**:
  1. `{"username":"ghost","password":""}`
- **预期结果**:
  1. 状态码 `401`，`code == "SSO_LDAP_EMPTY_PASSWORD"`
  2. 响应体与 TC-013-042 的响应体**字节相等**（空密码判定不查用户，不可用于枚举）
  3. `__stats` 三项全 `0`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-044: 纯空白密码（空格 / 制表 / 换行）一律按空处理

- **关联 AC**: AC-S-104
- **前置条件**: 计数器归零
- **测试步骤**:
  1. 依次提交 `password` 为 `" "`、`"   "`、`"\t"`、`"\n"`、`" \t\n "`
  2. 每次都读 `__stats.bind`
- **预期结果**:
  1. 5 次全部 `401`，`code == "SSO_LDAP_EMPTY_PASSWORD"`
  2. 5 次响应体**互相字节相等**
  3. 累计 `__stats.bind == 0`
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: 判据是 `password.is_empty() || password.trim().is_empty()`
  （demand §安全约束 §1），本用例覆盖 `trim()` 的全部空白字符类。

### TC-013-045: 密码为 NUL 字符 / 仅 NUL 不得绕过空密码判定

- **关联 AC**: AC-S-104
- **前置条件**: 计数器归零
- **测试步骤**:
  1. `password = "\u0000"`（JSON `"\u0000"`）
  2. `password = "\u0000\u0000"`
- **预期结果**:
  1. 两次都不返回 `200`
  2. `code` ∈ `{"SSO_LDAP_EMPTY_PASSWORD", "SSO_LDAP_BIND_FAILED"}`（BA 需钉死其一）
  3. `__stats.bind` 与 0 的差 `<= 1`（不得因 NUL 截断导致「空密码当成有密码」反复 bind）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: `〔守卫〕` —— `trim()` 不处理 `\u{0}`，而 LDAP 的 `simple_bind` 会把 NUL 原样发出；
  若实现用 C 风格截断，NUL 会变成空密码。AC 完全没覆盖这一格。

### TC-013-046: 目录不可达 → 503 `SSO_LDAP_UNREACHABLE`（与 OIDC 的失败码不同）

- **关联 AC**: AC-S-104
- **前置条件**: 把 `sso.ldap.url` 指向 `ldap://127.0.0.1:9399`（**未监听**）
- **测试步骤**:
  1. `{"username":"erin","password":"Er1n-pass"}`
- **预期结果**:
  1. 状态码 `503`，`code == "SSO_LDAP_UNREACHABLE"`
  2. `code != "SSO_OIDC_DISCOVERY_FAILED"`（同为 503 但码不同）
  3. `title_of_code("SSO_LDAP_UNREACHABLE") != title_of_code("SSO_OIDC_DISCOVERY_FAILED")`
  4. 响应体不含 `127.0.0.1:9399` 的完整连接错误原文以外的堆栈（无 `at crates/…` 字样）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-047: 用户名含 `*`（通配注入）—— filter 必须转义，且不得匹配到任意条目

- **关联 AC**: AC-S-104
- **前置条件**: 目录里有 3 个条目 `erin` / `frank` / `grace`，密码各不相同；
  攻击者不知道任何人的密码，用 `password="x"`
- **测试步骤**:
  1. `{"username":"*","password":"x"}`
  2. 读 `__stats.last_filter`
  3. 再试 `{"username":"*","password":"Er1n-pass"}`（通配 + 已知密码）
- **预期结果**:
  1. 步骤 1 状态码 `401`，`code == "SSO_LDAP_BIND_FAILED"`
  2. 步骤 2 的 `last_filter` **不含未转义的 `*`**：必须出现 `\2a`（RFC 4515 转义），
     且形如 `(uid=\2a)`
  3. 步骤 3 **不得** `200`（通配命中多条时不得取第一条去 bind）
  4. `__stats.search` 每次 `== 1`（不重试）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-048: 用户名含 `)(uid=*` 与 `(` / `)` —— 括号必须转义为 `\28` / `\29`

- **关联 AC**: AC-S-104
- **前置条件**: 同 TC-013-047
- **测试步骤**:
  1. `{"username":")(uid=*","password":"x"}`
  2. `{"username":"erin)(|(uid=*","password":"x"}`
  3. 读两次的 `last_filter`
- **预期结果**:
  1. 两次均 `401`、`code == "SSO_LDAP_BIND_FAILED"`
  2. 两次的 `last_filter` 中 `(` 与 `)` 的总出现次数 **等于 2**
     （只有过滤器自己的最外层一对；输入里的括号全部被转义）
  3. `last_filter` 中每个输入的 `(` 都变成 `\28`，`)` 变成 `\29`
  4. 过滤器结构仍是 `(uid=<转义后的用户名>)`，不出现第二个 `|` 或 `&`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-049: RFC 4515 五个特殊字符全覆盖（转义完整性）

- **关联 AC**: AC-S-104
- **前置条件**: 同 TC-013-047
- **测试步骤**:
  1. 提交用户名 `\*()\u0000`（即反斜杠、星号、左右括号、NUL）
  2. 读 `last_filter`
- **预期结果**:
  1. `last_filter` 依次包含 `\5c`（反斜杠）、`\2a`（星号）、`\28`、`\29`、`\00`
  2. 未转义的 `*` 出现次数 `== 0`
  3. 状态码 `401`（不因非法输入变成 `400` 之外的 5xx）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 正交
- **备注**: design §5.1-2 明写「转义 `\ * ( ) NUL` 五个字符」；5 个都得有断言。

### TC-013-050: 用户名大小写 / 空串 —— 空用户名不得导致「匹配到第一条」

- **关联 AC**: AC-S-104
- **前置条件**: 同 TC-013-047
- **测试步骤**:
  1. `{"username":"","password":"Er1n-pass"}`
  2. `{"username":"   ","password":"Er1n-pass"}`
- **预期结果**:
  1. 两次均 `401`，`code == "SSO_LDAP_BIND_FAILED"`
  2. `last_filter` 为 `(uid=)` / `(uid=)`（trim 后为空），且 `__stats.search` 各 `== 1`
  3. 不返回 `200`（空用户名 + 别人的密码不能登录）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值

### TC-013-051: 失败路径同样受 IP 限流保护（复用既有 `LOGIN_RATE_LIMITED`）

- **关联 AC**: AC-S-104
- **前置条件**: 计数器与限流表清空（`DELETE FROM platform_iam_login_rate`）；
  只用一个 IP `198.51.100.7`
- **测试步骤**:
  1. 连续发 20 次 `{"username":"erin","password":"wrong-<i>"}`
  2. 发第 21 次
  3. 再发第 22 次
- **预期结果**:
  1. 第 1~20 次均为 `401`（`SSO_LDAP_BIND_FAILED` **或** `ACCOUNT_LOCKED` —— 见下）
  2. 第 21 次起状态码 `429`，`code == "LOGIN_RATE_LIMITED"`
  3. `429` 响应带 `retryAfterSeconds == 300`
  4. `platform_iam_login_rate` 中该 IP 摘要的行数 `>= 20`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值
- **备注**: **AC 写「连续 10 次错误密码，再打第 11 次 → 429」与实测不符**：
  `ip_limit` 默认 **20**、窗口 300 秒（`crates/platform-iam/src/config.rs:167-168`），
  第 11 次只会 `401`。另外 `account_failure_limit` 默认 **5**（同文件 `:169`），
  若 LDAP 失败也计入账号锁定，第 6 次就变 `ACCOUNT_LOCKED`。
  两条阈值谁生效必须在 BA 侧钉死，否则本用例无法写死期望值。〔AC 修正 S23〕

### TC-013-052: 密码不出现在日志里（含阳性对照）

- **关联 AC**: AC-S-104
- **前置条件**: `BASE_LOG_LEVEL=trace`（且**不设** `RUST_LOG` —— 后者优先，
  见 `crates/platform-server/src/main.rs:1062`）；捕获 stdout 到文件
- **测试步骤**:
  1. 起服务，跑一次成功 bind（`Er1n-pass`）+ 一次失败 bind（`WrongCanary-9963`）
  2. 在日志文件里搜 `Er1n-pass`、`WrongCanary-9963`
  3. **阳性对照**：在测试专用 handler 里 `tracing::warn!("canary={}", "WrongCanary-9963")`，
     再搜一次
- **预期结果**:
  1. 步骤 2：两个串的命中数 `== 0`
  2. 步骤 3：阳性对照的命中数 `>= 1`（证明扫描器有效）
  3. 日志里**可以**出现用户名 `erin`（它不是密钥），但**不得**出现 `uid=erin,ou=people,…` 的完整 DN
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

---

## AC-S-105：LDAP 与本地账号共存

### TC-013-053: `frank` 用本地密码登录 → 200

- **关联 AC**: AC-S-105
- **前置条件**: 本地 `frank` 有密码 `Fr4nk-local`；`sso.enforce=false`
- **测试步骤**:
  1. `POST /api/iam/login`，`{"username":"frank","password":"Fr4nk-local"}`
- **预期结果**:
  1. 状态码 `200`
  2. `user.id` 记为 `U_local`
  3. `platform_iam_session` 该会话 `auth_provider == "local"`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类

### TC-013-054: `frank` 用 LDAP 密码登录 → 200，且解析到**同一个** user

- **关联 AC**: AC-S-105
- **前置条件**: 同 TC-013-053；`sso.ldap.mailAttribute=mail`，目录里 `frank` 的 `mail=frank@example.com`，
  与本地 `frank.email` 相同
- **测试步骤**:
  1. `POST /api/iam/sso/ldap/login`，`{"username":"frank","password":"Fr4nk-ldap"}`
  2. 比对 `user.id` 与 `U_local`
  3. 查 `platform_iam_user` 中 `username='frank'` 的行数
- **预期结果**:
  1. 状态码 `200`
  2. `user.id == U_local`
  3. 行数 `== 1`（**不产生影子账号**）
  4. `external_identity` 新增 1 行：`provider='ldap'`、`user_id == U_local`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: `provider` 与 `external_id` 的取值 AC/design 都没定（是 `'ldap'` 还是 `'ldap-<url>'`？
  `external_id` 是 DN 还是 `uid`？）—— 这两列进唯一键，定不下来本用例不可判定。〔S32〕

### TC-013-055: 本地密码哈希**未被** LDAP 密码覆盖

- **关联 AC**: AC-S-105
- **前置条件**: 记录 `SELECT password_hash FROM platform_iam_user WHERE id=<U_local>`
- **测试步骤**:
  1. 走 TC-013-054 的 LDAP 登录
  2. 再查一次 `password_hash`
  3. 查 `platform_iam_password_history` 中该用户的行数
- **预期结果**:
  1. 两次 `password_hash` **逐字相等**
  2. 该值以 `$argon2id$v=19$` 开头
  3. `password_history` 行数**未增加**（LDAP 登录不是改密，不该写历史）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-056: 不存在任何「LDAP 密码摘要」类列

- **关联 AC**: AC-S-105
- **前置条件**: 迁移已跑
- **测试步骤**:
  1. 查 `information_schema.columns`
     `WHERE table_schema='public' AND table_name IN ('platform_iam_user','platform_iam_password_history')`
  2. 过滤列名匹配 `ldap|external|sso`
- **预期结果**:
  1. 步骤 2 的结果行数 `== 0`
  2. 阳性对照：同一查询过滤 `password_hash` → 行数 `== 1`（证明查询本身有效）
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-057: 改目录里的密码后，旧密码立即失效、新密码立即生效（底座不缓存）

- **关联 AC**: AC-S-105
- **前置条件**: TC-013-054 通过
- **测试步骤**:
  1. 在 mock 目录里把 `frank` 的密码从 `Fr4nk-ldap` 改为 `Fr4nk-ldap-2`
  2. 用旧密码登录
  3. 用新密码登录
- **预期结果**:
  1. 步骤 2 状态码 `401`，`code == "SSO_LDAP_BIND_FAILED"`
  2. 步骤 3 状态码 `200`
  3. 步骤 3 的 `user.id == U_local`（改密不改绑定）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移

### TC-013-058: 目录 `mail` 改成已被**另一个**本地账号占用的邮箱 → 409 且不改绑

- **关联 AC**: AC-S-105
- **前置条件**: 本地已有 `grace`（`email=grace@example.com`）；把目录里 `frank` 的 `mail`
  改为 `grace@example.com`；`frank` 已有 `external_identity(provider='ldap', external_id=<frank 的 id>)`
- **测试步骤**:
  1. `frank` 用 LDAP 密码登录
  2. 查 `external_identity` 中 `user_id=<grace>` 的行
  3. 再查 `(provider, external_id)` 指向 `U_local` 的那行
- **预期结果**:
  1. 状态码 `409`，`code == "SSO_LDAP_IDENTITY_CONFLICT"`
  2. 步骤 2 行数 `== 0`（**不得**静默绑到 `grace`）
  3. 步骤 3 的行仍指向 `U_local`，且该行 `last_synced_at` **未变**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: `〔守卫〕` —— 若实现「先按 `external_identity` 命中就直接放行、不再看 `mail`」，
  这条会返回 `200`；那是设计允许还是缺陷，BA 必须先定匹配顺序（`external_id` 优先还是
  `mail` 优先）。design §5 步骤 6 只写「从条目取映射字段 → 匹配/建本地 user」，**没说顺序**。〔S21〕

### TC-013-059: 同一 `mail` 的两个不同 `uid` 条目 —— 第二次 409，不静默改绑

- **关联 AC**: AC-S-105
- **前置条件**: 目录里再加 `uid=frank2`，`mail=frank@example.com`（与 `frank` 同 mail）
- **测试步骤**:
  1. `frank` 登录（`200`，建立绑定）
  2. `frank2` 登录
- **预期结果**:
  1. 步骤 1 后 `external_identity(provider='ldap', external_id=<frank>)` → `U_local`
  2. 步骤 2 状态码 `409`（或 `403`，BA 需钉死），`!= 200`
  3. `frank2` 没有新建 user（`platform_iam_user` 行数不变）
  4. `external_identity` 总行数不变
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-060: `sso.enforce=false` 时两条路径共存，各自独立 family

- **关联 AC**: AC-S-105
- **前置条件**: `sso.enforce=false`
- **测试步骤**:
  1. `frank` 本地登录一次
  2. `frank` LDAP 登录一次
  3. `GET /api/iam/sessions`（用任一 access token + `x-tenant-id: tenant-a`）
- **预期结果**:
  1. 两次都 `200`
  2. 两条会话的 `family_id` **不相等**（各自独立 family）
  3. 列表长度 `== 2`
  4. 两条的 `authProvider` 分别为 `"local"` 与 `"ldap"`
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 正交
