# A. 身份与访问 —— 测试用例（AC-001 ~ AC-008）

- **需求**: REQ-001 通用项目底座（Base Platform）
- **来源**: [acceptance.md](../acceptance.md) §A、[demand.md](../demand.md) §A、[ADR-0001](../../decisions/ADR-0001-底座技术架构基线.md) D3 / D6
- **用例数**: 19（TC-A-001 ~ TC-A-019）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## A.0 本域约定

- 权限点编码：`module:resource:action`，本域使用模块 `order`，资源 `invoice`，动作 `read|write|approve|delete`；通配写作 `order:*`。
- 认证接口：`POST /api/iam/auth/login`、`POST /api/iam/auth/refresh`、`POST /api/iam/auth/logout`、`GET /api/iam/auth/sessions`、`DELETE /api/iam/auth/sessions/{session_id}`。
- 错误格式：RFC 9457 `application/problem+json`，字段 `status` / `title` / `code` / `detail`。
- 前端 SDK：`@base/auth` 导出 `can(perm)`；权限组件 `<Can perm="...">`；路由守卫守卫失败跳 `/403`。
- SSO 入站协议参数：`saml` / `oidc` / `ldap`，入口 `GET /api/iam/auth/sso/{protocol}/login`。
- 令牌有效期：access token 15 分钟（默认），refresh token 7 天（默认），可在测试环境缩短以便验证过期。

---

### TC-A-001: 权限点注册表驱动 can() 与 UI 显隐（正常路径）
- **关联 AC**: AC-001
- **前置条件**:
  - `modules/order/manifest.json` 已声明权限点 `order:invoice:approve`（`type: "button"`）。
  - 租户 T1 存在角色 `finance`，已授予 `order:invoice:approve`；账号 `boss@example.com` 绑定该角色。
  - 页面 `/order/invoice` 的审批按钮由 `<Can perm="order:invoice:approve">` 包裹。
- **测试步骤**:
  1. 以 `boss@example.com` 登录 T1，取得 access token。
  2. 前端控制台执行 `await can('order:invoice:approve')`，并打开 `/order/invoice` 页面。
  3. 观察审批按钮是否渲染；点击该按钮触发 `POST /api/order/invoice/approve`（带 token 与 `X-Tenant-Id: T1`）。
- **预期结果**:
  1. `can('order:invoice:approve')` 返回 `true`。
  2. `/order/invoice` 页面审批按钮可见可点击。
  3. 接口返回 HTTP 200，响应体含审批结果字段（如 `{"approved": true}`）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（有效类）+ 正常路径

### TC-A-002: 无权限角色：can() 为 false、按钮不渲染、直调接口 403
- **关联 AC**: AC-001
- **前置条件**:
  - 账号 `staff@example.com` 绑定角色 `member`，该角色**不含** `order:invoice:approve`。
  - `modules/order/manifest.json` 已声明 `order:invoice:approve`。
- **测试步骤**:
  1. 以 `staff@example.com` 登录 T1。
  2. 前端执行 `await can('order:invoice:approve')`；打开 `/order/invoice` 页面观察审批按钮。
  3. 绕过 UI，用该账号的 token 直接发 `POST /api/order/invoice/approve`（body：`{"order_id":"order-1001"}`）。
- **预期结果**:
  1. `can('order:invoice:approve')` 返回 `false`。
  2. 页面 DOM 中不存在审批按钮节点（查询选择器为空）。
  3. 接口返回 HTTP 403，响应为 `application/problem+json`，`code` 为 `INSUFFICIENT_PERMISSION`（契约 `api-conventions v1` C6 的示例取值，**REQ-009 定案**；不再用 `permission_denied`），且响应体不包含业务数据。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（角色 → 权限 → 判定）+ 攻击面枚举（绕过前端）

### TC-A-003: 权限点通配生效；manifest 未声明的权限点不可用
- **关联 AC**: AC-001
- **前置条件**:
  - 角色 `order-admin` 授予通配权限 `order:*`；账号 `owner@example.com` 绑定该角色。
  - `order:invoice:void` **不在**任何模块 manifest 中声明。
- **测试步骤**:
  1. 以 `owner@example.com` 登录，前端执行 `can('order:invoice:read')` 与 `can('order:invoice:approve')`。
  2. 查询权限点注册表 `GET /api/rbac/permissions`，检索 `order:invoice:void`。
  3. 以同一账号直调 `POST /api/order/invoice/void`。
- **预期结果**:
  1. 两个 `can()` 均返回 `true`（通配命中）。
  2. 注册表中不存在 `order:invoice:void`；所有权限点的 `code` 均能在某个模块 manifest 中找到来源。
  3. 接口返回 HTTP 403（未注册的权限点不构成授权依据），不返回 500。
- **类型**: 功能 / 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值 + 等价类（已声明 / 未声明）

### TC-A-004: 数据范围「本组织及下级」可见全量下级数据
- **关联 AC**: AC-002
- **前置条件**:
  - 租户 T1 组织树：`总部(org-hq)` → `销售部(org-sales)` → `销售一组(org-sales-1)`。
  - 角色 `manager` 的数据范围 = `本组织及下级`；账号 `boss@example.com` 绑定该角色且所属 `org-sales`。
  - 数据：订单 `order-1001`（owner=boss，org=org-sales）、`order-1002`（owner=staff，org=org-sales-1）、`order-1003`（owner=other，org=org-fin）。
- **测试步骤**:
  1. 以 `boss@example.com` 登录 T1，请求 `GET /api/order/invoice?page=1&page_size=50`。
  2. 记录返回的订单 ID 集合与每条的 `owner`、`org_id`。
  3. 打开 DB 查询日志，核对本次请求对应的 SQL 片段。
- **预期结果**:
  1. HTTP 200，返回集合包含 `order-1001` 与 `order-1002`（下级组织数据），**不包含** `order-1003`（平级/他部门数据）。
  2. 返回条目的 `org_id` 均属于 `org-sales` 或其下级 `org-sales-1`。
  3. SQL 片段同时含租户条件 `tenant_id = 'T1'` 与数据范围条件（如 `org_id IN ('org-sales','org-sales-1')` 或等价子查询）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 等价类 + 数据隔离断言

### TC-A-005: 数据范围「仅本人」仅返回本人数据
- **关联 AC**: AC-002
- **前置条件**:
  - 角色 `member` 的数据范围 = `仅本人`；账号 `staff@example.com` 绑定该角色。
  - 数据同 TC-A-004（`order-1001`/`order-1002`/`order-1003`）。
- **测试步骤**:
  1. 以 `staff@example.com` 登录 T1，请求 `GET /api/order/invoice?page=1&page_size=50`。
  2. 对比返回集合与自己名下的订单。
  3. 查看 SQL 日志中的过滤条件。
- **预期结果**:
  1. HTTP 200，返回集合仅含 `order-1002`；不含 `order-1001`、`order-1003`。
  2. 返回条目的 `owner` 字段恒等于当前登录用户 ID。
  3. SQL 含 `tenant_id = 'T1'` 与按 owner/创建人限定的过滤条件（如 `created_by = '<staff_user_id>'`），不含仅按组织的宽条件。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 等价类 + 边界（空结果时 `total=0` 而非报错）

### TC-A-006: 直接按 ID 取他人数据被拒（越权路径）
- **关联 AC**: AC-002
- **前置条件**:
  - 同 TC-A-005；`order-1001` 属于 `boss@example.com`。
- **测试步骤**:
  1. 以 `staff@example.com` 登录，取得 access token。
  2. 直接请求 `GET /api/order/invoice/order-1001`（不带任何过滤参数）。
  3. 同时以 `boss@example.com` 请求同一路径作对照。
- **预期结果**:
  1. `staff` 请求返回 HTTP 403 或 404（按底座统一错误约定），响应体不含 `owner`、金额等任何业务字段。
  2. 错误体 `code` 明确表达权限/范围拒绝，不得返回 200。
  3. `boss` 的同一请求返回 HTTP 200 且含 `order-1001` 数据（证明不是数据本身缺失，而是范围隔离生效）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（对象级越权）+ 边界

### TC-A-007: 白名单域名注册成功并进入审批待定态
- **关联 AC**: AC-003
- **前置条件**:
  - 租户 T1 配置：注册开启、邮箱域名白名单 = `example.com`、审批制注册开启。
  - 租户 T2 配置：注册关闭。
  - 账号 `a@example.com` 在 T1 尚不存在。
- **测试步骤**:
  1. 在 T1 登录页提交注册 `a@example.com` + 密码 `Passw0rd!`。
  2. 查询该用户记录与审批队列。
  3. 以 `a@example.com` 尝试登录。
- **预期结果**:
  1. 注册接口返回 HTTP 201，响应体 `status` 为 `pending_approval`（或等价的待审批状态），不返回可登录态 token。
  2. 审批队列中存在该用户记录，租户为 T1。
  3. 登录被拒绝，错误 `code` 表明账号待审批，不得返回 200。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（未注册 → 待审批 → 激活）

### TC-A-008: 非白名单域名与关闭注册的租户一律拒绝
- **关联 AC**: AC-003
- **前置条件**:
  - T1 注册开启但白名单仅 `example.com`；T2 注册关闭。
  - `b@other.com` 在两个租户均不存在。
- **测试步骤**:
  1. 在 T1 提交注册 `b@other.com`。
  2. 在 T2 提交注册 `b@other.com`。
  3. 分别记录两次响应。
- **预期结果**:
  1. T1 返回 HTTP 403（`code` 如 `email_domain_not_allowed`），不创建任何用户记录。
  2. T2 返回 HTTP 403（`code` 如 `registration_disabled`），不创建任何用户记录。
  3. 两个错误码可区分，且响应体不泄露「该邮箱是否已注册」等存在性信息。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（租户开关 × 域名白名单）

### TC-A-009: 邀请链接一次性：首次使用成功，复用被拒
- **关联 AC**: AC-004
- **前置条件**:
  - 生成邀请链接：有效期 1 小时、使用次数上限 1 次，指向 `https://<host>/invite/<token>`。
  - 受邀邮箱 `invitee@example.com` 尚无账号。
- **测试步骤**:
  1. 用该链接完成注册（填写邮箱与密码 `Passw0rd!`）。
  2. 不刷新 token，用同一链接（同一浏览器新标签）再次访问并尝试注册 `invitee2@example.com`。
  3. 查询邀请记录的 `used_count` 与 `status`。
- **预期结果**:
  1. 首次注册返回 HTTP 201，用户创建成功并进入正常登录态流程。
  2. 第二次使用返回 HTTP 410 或 403（错误 `code` 如 `invitation_used`），不得创建 `invitee2@example.com`。
  3. 邀请记录 `used_count = 1`，`status` 为 `used`。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（有效 → 已用）+ 边界（次数上限）

### TC-A-010: 邀请链接过期被拒
- **关联 AC**: AC-004
- **前置条件**:
  - 生成有效期 1 分钟的邀请链接 `expired-token`；测试环境通过可控时钟或调整 `expires_at` 使其已过期。
  - 受邀邮箱 `expiree@example.com` 尚无账号。
- **测试步骤**:
  1. 用 `expired-token` 访问邀请页。
  2. 提交注册表单。
  3. 查询用户与邀请记录。
- **预期结果**:
  1. 邀请页返回明确提示「链接已过期」，且不进入注册表单填写态。
  2. 提交注册返回 HTTP 410（`code` 如 `invitation_expired`）。
  3. 不创建 `expiree@example.com`；邀请记录 `status` 保持 `expired`。
- **类型**: 功能 / 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（有效期临界：过期前 1 秒成功 / 过期后 1 秒失败）

### TC-A-011: SAML / OIDC / LDAP 三种入站 SSO 分别登录成功
- **关联 AC**: AC-005
- **前置条件**:
  - 测试环境各备一套：SAML IdP（提供 `metadata.xml` 与测试用户 `saml-user`）、OIDC IdP（提供 `/.well-known/openid-configuration` 与测试用户 `oidc-user`）、LDAP 服务（提供目录条目 `uid=ldap-user`）。
  - 底座已分别配置三套连接器；三个外部用户均未在底座建号。
- **测试步骤**:
  1. 分别访问 `/api/iam/auth/sso/saml/login`、`/api/iam/auth/sso/oidc/login`、`/api/iam/auth/sso/ldap/login`，完成外部认证回调。
  2. 每次回调后查询底座用户表与角色绑定。
  3. 检查会话 cookie / token 是否建立。
- **预期结果**:
  1. 三次流程均最终跳转到底座应用首页并建立会话（HTTP 302 到应用路由，非登录页）。
  2. 底座分别创建或关联 `saml-user`、`oidc-user`、`ldap-user` 三个本地账号，字段 `sso_source` 分别为 `saml` / `oidc` / `ldap`；外部唯一标识正确写入。
  3. 每个账号权限仍由本地角色体系决定（外部 IdP 不直接授予权限点）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 等价类（按协议分）+ 场景法

### TC-A-012: SSO 协议失败回落登录页且错误可区分
- **关联 AC**: AC-005
- **前置条件**:
  - 配置三套 IdP 但分别置于故障态：SAML IdP 断言签名无效；OIDC IdP `token_endpoint` 返回 500；LDAP 服务端口不可达。
- **测试步骤**:
  1. 分别触发三条 SSO 登录，记录最终 URL 与错误提示。
  2. 观察日志中的错误分类。
- **预期结果**:
  1. 三次均回落到登录页 `/login`，页面显示可区分的错误提示（如 `SAML 响应校验失败` / `OIDC 服务异常` / `LDAP 连接失败`），不得统一为「未知错误」。
  2. 每次提示中可读出协议名（`SAML` / `OIDC` / `LDAP`）。
  3. 不创建任何本地账号，不建立会话；日志记录 `protocol` 与失败原因分类字段。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 异常路径 + 等价类（失败原因分类）

### TC-A-013: 底座作为 IdP：授权码 + PKCE 换令牌并调用应用接口成功
- **关联 AC**: AC-006
- **前置条件**:
  - 已注册第三方应用 `app_demo`，`client_id=app_demo`、`redirect_uri=https://app.example.com/cb`、授权模式 `authorization_code`、开启 PKCE（S256）。
  - 账号 `owner@example.com` 已登录 T1，并在授权页授予应用 `order:invoice:read` 范围。
- **测试步骤**:
  1. 访问 `GET /api/app/oauth/authorize?response_type=code&client_id=app_demo&redirect_uri=https://app.example.com/cb&scope=order:invoice:read&code_challenge=<S256(code_verifier)>&code_challenge_method=S256`，在授权页点击「同意」并跳转回调地址。
  2. 从回调地址取出 `code`。
  3. 调用 `POST /api/app/oauth/token`，body：`grant_type=authorization_code&code=<code>&client_id=app_demo&code_verifier=<code_verifier>`。
  4. 用返回的 access token 调用应用接口 `GET /api/app/demo/order/invoice`（携带 `Authorization: Bearer <token>`）。
- **预期结果**:
  1. 回调地址带回 `code`（且 `state` 原样回传）。
  2. 令牌接口返回 HTTP 200，含 `access_token`、`token_type=Bearer`、`expires_in`、`refresh_token`、`scope`；响应不含 `secret` 等敏感配置。
  3. 应用接口返回 HTTP 200，且数据范围受令牌 scope 与用户数据范围双重约束（看不到 T2 数据）。
  4. 应用接口可选用 JWKS 或 introspect 校验令牌：调用 `POST /api/app/oauth/introspect` 返回 `{"active": true, "client_id": "app_demo"}`；`GET /.well-known/jwks.json` 返回该令牌 `kid` 对应的公钥。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径 + 端到端链路

### TC-A-014: PKCE 校验失败与令牌过期被拒
- **关联 AC**: AC-006
- **前置条件**:
  - 同 TC-A-013；测试环境 access token 有效期缩短为 60 秒。
- **测试步骤**:
  1. 用错误的 `code_verifier`（与 `code_challenge` 不匹配）调用 `POST /api/app/oauth/token`。
  2. 重新走一次授权流程取得有效 token，等待其过期后再调用应用接口。
  3. 记录两次响应。
- **预期结果**:
  1. 第一次返回 HTTP 400，`code` 为 `invalid_grant`（或 `invalid_request`），响应体不含任何 token 字段；授权码被标记为已消费/失效，不可再用正确 verifier 重试。
  2. 第二次应用接口返回 HTTP 401，`WWW-Authenticate` 头存在，响应体为 `application/problem+json`。
  3. 错误体不泄露「授权码正确但 PKCE 失败」以外的内部细节。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径 + 边界值（有效期临界）

### TC-A-015: refresh token 轮换，旧 refresh 立即失效
- **关联 AC**: AC-007
- **前置条件**:
  - 账号 `boss@example.com` 已登录，持有 `refresh_token` R1 与 access token A1。
- **测试步骤**:
  1. 用 R1 调用 `POST /api/iam/auth/refresh`。
  2. 记录返回的新 access token A2 与新 refresh token R2。
  3. 再次用 R1 调用同一接口。
  4. 用 A1 调用 `GET /api/iam/auth/sessions`。
- **预期结果**:
  1. 第一次返回 HTTP 200，含 A2 与 R2。
  2. 第二次用 R1 返回 HTTP 401（`code` 如 `refresh_token_invalid`），不得签发新令牌。
  3. R1 在数据库中的状态为 `rotated`（而非仍可用）。
  4. A1 若已超过有效期返回 401；若未过期，仍可继续使用（access token 不因轮换立即失效），该行为须在实现文档中明确且一致。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（有效 → 已轮换）+ 攻击面枚举（重放）

### TC-A-016: 登出后令牌立即不可用
- **关联 AC**: AC-007
- **前置条件**:
  - 账号 `staff@example.com` 已登录，持有有效 access token A 与 refresh token R。
- **测试步骤**:
  1. 调用 `POST /api/iam/auth/logout`。
  2. 用 A 调用 `GET /api/iam/auth/sessions`。
  3. 用 R 调用 `POST /api/iam/auth/refresh`。
- **预期结果**:
  1. 登出返回 HTTP 204。
  2. A 调用返回 HTTP 401。
  3. R 调用返回 HTTP 401，且不签发任何新令牌。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移 + 攻击面枚举

### TC-A-017: 令牌撤销后不可用，撤销记录可查
- **关联 AC**: AC-007
- **前置条件**:
  - 账号 `owner@example.com` 登录后在两个浏览器各有一个会话（token A1 / A2）；底座提供管理员撤销接口 `POST /api/iam/auth/revocations`。
- **测试步骤**:
  1. 管理员撤销 A1 对应的会话/令牌。
  2. 用 A1 调用任一业务接口（如 `GET /api/order/invoice`）。
  3. 查询撤销记录与令牌状态（`POST /api/iam/auth/revocations` 列表或令牌 introspect）。
  4. 用 A2 重复步骤 2。
- **预期结果**:
  1. 撤销操作返回 HTTP 200/204，并写入审计日志（含操作人、对象、原因）。
  2. A1 调用返回 HTTP 401。
  3. 撤销记录中可查到 A1 的指纹（`jti`）与撤销时间、撤销人；introspect 返回 `{"active": false}`。
  4. A2 仍可用（仅撤销指定会话，不误伤其他会话）。
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 等价类 + 边界（精确撤销 vs 全量撤销）

### TC-A-018: 设备会话列表与实际登录一致
- **关联 AC**: AC-008
- **前置条件**:
  - 账号 `boss@example.com`；准备两个独立浏览器 profile（Chrome、Firefox），分别登录同一账号（会话 S1、S2）。
- **测试步骤**:
  1. 以 `boss@example.com` 登录，调用 `GET /api/iam/auth/sessions`。
  2. 对比返回条目与实际两个浏览器会话。
- **预期结果**:
  1. 返回 HTTP 200，条目数 = 2。
  2. 每条含 `session_id`、`device`（浏览器/系统）、`ip`、`created_at`、`last_active_at`、`current`（是否当前会话标记）。
  3. 两条的 `device` / `ip` 与两个浏览器实际一致，且当前浏览器那条 `current = true`。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 正常路径 + 数据一致性

### TC-A-019: 从 A 端远程下线 B，B 下一次请求被强制登出
- **关联 AC**: AC-008
- **前置条件**:
  - 同 TC-A-018（S1 = Chrome，S2 = Firefox）。
- **测试步骤**:
  1. 在 S1（Chrome）的「设备会话」页面对 S2 执行「下线此设备」，即 `DELETE /api/iam/auth/sessions/{S2.session_id}`。
  2. 在 S2（Firefox）刷新页面或调用任一业务接口。
  3. 回到 S1 刷新会话列表。
- **预期结果**:
  1. 下线操作返回 HTTP 200/204。
  2. S2 的下一次请求返回 HTTP 401，前端跳转登录页并提示「会话已被下线」。
  3. S1 刷新后会话列表仅剩 S1，条目数 = 1，且不残留已下线会话。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 状态迁移（会话在线 → 已下线）
