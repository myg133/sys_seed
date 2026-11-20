# REQ-022「出站 IdP 与单点登出」—— QA 挑刺报告（开工前）

- **审查对象**: `BA/demands/REQ-022-outbound-idp/` 的四份文档 —— `demand.md`（rev 2）/
  `acceptance.md`（rev 1，**AC-S-134 ~ AC-S-155 共 22 条**）/ `decisions.md`（rev 1，D196~D220）/
  `design-summary.md`（rev 1）；契约 `BA/contracts/api-conventions/v3.md`（本轮新升版本）
- **代码基线**: `code/` @ `develop` @ `310e4d3` —— **只读核实；未改动任何代码文件**
- **环境**: Arch Linux / 8 线程 / 7.7G / podman `base-postgres`（postgres 17-alpine）/
  **只读 + 临时对象实测**（本轮只用了 `SELECT`，**未建临时表、未写任何行**）
- **审查者**: QA（myg133）｜**审查时间**: 2026-11-20
- **方法**: 逐 AC + 逐 design 章节 + 逐契约条款核对；每条结论都给 `file:line` 或
  **可复跑的命令与实测输出**。实测过的 9 组必查项见 §6。
  **未跑 `cargo build` / `cargo test`**（需求尚未实现，跑不了；低配机 8 线程约束）。
- **配套用例集**: [test-cases/index.md](../demands/REQ-022-outbound-idp/test-cases/index.md)
  （**123 条**，22/22 AC 覆盖）
- **分级依据**: dev-guide §9.12 的**五档可判定性**（不是严重程度）；
  级别（阻塞/高/中）另标。**阻塞 = 落在前三档（主体不存在 / 物理不可达 / 不可判定）**，
  或落在「口径不一致 / 不可执行」但会直接造成「按错的规格实现 = 自洽闭环」。

---

## 汇总

| 级别 | 数量 |
|---|---|
| **阻塞**（主体不存在 / 物理不可达 / 不可判定） | **10** |
| **高**（口径不一致 / 安全缺口 / 不可执行） | **19** |
| **中** | **10** |
| **合计** | **39** |

**按可判定性分档（dev-guide §9.12）**：

| 档位（dev-guide §9.12） | 数量 | 编号 |
|---|---|---|
| **主体不存在** | **0** | —（REQ-022 是全新能力，无「引用了不存在的东西」；问题全在判据与口径） |
| **物理不可达** | **6** | S-01、S-02、S-06、S-07、S-08、S-34 |
| **不可判定** | **12** | S-03、S-05、S-09、S-10、S-15、S-17、S-18、S-21、S-24、S-30、S-31、S-35 |
| **口径不一致** | **18** | S-04、S-11、S-12、S-13、S-14、S-16、S-19、S-20、S-22、S-23、S-26、S-27、S-28、S-33、S-36、S-37、S-38、S-39 |
| **表述** | **2** | S-29、S-32 |
| **安全缺口**（本轮增设，归「高」） | **1** | S-25 |
| **合计** | **39** | 唯一条目数 **39**（一条只记一档，归到**最严重**的那一档） |

> **档位 ≠ 级别**：级别（阻塞/高/中）是**独立**的第二维度。
> 「阻塞」= 落在前三档，**或**落在「口径不一致 / 不可执行」但会直接导致
> 「按错的规格实现 + 按错的规格验收」的自洽闭环（沿用 REQ-014 报告的定级惯例）。

**22 条 AC 里，22 条都有问题**（无一条可原样实现并验收）。
按 AC 分布（一条问题可涉及多条 AC，故列和 > 39）：

| AC | 问题编号 | 其中阻塞 | 结论 |
|---|---|---|---|
| AC-S-134 | S-11、S-29 | 0 | `authorization_endpoint` 与 `oauth.frontendBaseUrl` 配置自相矛盾 |
| AC-S-135 | S-06、S-27、S-28、S-38 | **1** | 「持 `oauth:client:write` 的管理员」不存在；disabled 的拒绝点未钉 |
| AC-S-136 | S-06、S-17 | **1** | 「全仓敏感值扫描」没有可实现的宿主与口径 |
| AC-S-137 | S-01、S-02、S-12 | **2** | **第三方回调不可达 + 未登录回跳丢 query（物理不可达）** |
| AC-S-138 | S-02、S-09、S-12、S-30 | **2** | 失败点（preview 还是 confirm）未钉死 |
| AC-S-139 | S-16、S-31、S-32 | 0 | 「SHA-256 摘要」未说编码；TTL 补充判据缺列清单 |
| AC-S-140 | S-14、S-15 | 0 | 「二者必须能区分开」用 AC 给的方法做不到；「不回溯扩大」无法造 |
| AC-S-141 | S-19、S-37 | 0 | 三重交集公式在 client_credentials 下无定义 |
| AC-S-142 | S-16、S-21 | 0 | `verification_uri` 指向未钉；RFC 8628 的 `user_code` / `slow_down` 未覆盖 |
| AC-S-143 | S-18 | 0 | 步骤 5/6 都是 401 → 无区分力；未写错误体形状 |
| AC-S-144 | S-33 | 0 | 「grant 级撤销 → 该 grant 下所有令牌失效」未覆盖 |
| AC-S-145 | S-19 | 0 | 「`sub` 齐全」与 AC-S-141 / D198 冲突 |
| AC-S-146 | S-08 | **1** | **篡改 ticket 需要私钥，测试侧拿不到 → 判据恒真** |
| AC-S-147 | S-33、S-34 | 0 | 「另一个租户的用户」谁建没写 |
| AC-S-148 | S-02、S-12、S-39 | **1** | 未登录回跳丢 query（物理不可达）；`consent_required` 形状未钉 |
| AC-S-149 | S-07、S-24、S-25 | **1** | **mock 接收端在 E2E 起不来**；`sid` 多会话未定义；登出不杀 access token |
| AC-S-150 | S-20、S-25、S-26 | 0 | 「frontChannel 数组」与 design「返回 HTML」矛盾 |
| AC-S-151 | S-05、S-10 | **2** | 「二者都接受」不可判定；步骤 3 测的不是「OAuth 令牌被拒」 |
| AC-S-152 | S-03、S-04 | **2** | **TITLES 行数未给死；D215 的事件码进 TITLES 会让既有测试变红** |
| AC-S-153 | S-13、S-36 | 0 | 判据命令在基线上**已经是红的**，且**不做 C-5**；迁移文件名两处矛盾 |
| AC-S-154 | S-04、S-35 | 0 | 敏感值扫描的宿主与阳性对照注入点未指定 |
| AC-S-155 | S-22、S-23、S-34 | 0 | 4 张表无 manifest `tenantScoped` → 平台 RLS 自检不覆盖；只验读侧 |

---

## 阻塞（10）

### S-01: AC-S-137 步骤 4「读浏览器最终地址栏的 `code`」—— **第三方回调地址物理不可达**

- **档位**: 物理不可达 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-137 步骤 4、预期结果第 1 条（:149、:155）
- **AC 原文**: 「4. 读浏览器最终地址栏的 `code` 与 `state`；」 /
  「步骤 4 的地址栏 **以 `code=` 开头**，`state` **逐字等于** `xyz`」
- **问题**: `redirect_uri` 是 `https://client.example/cb` —— **该主机不存在、不可达**。
  design §4.1（:188）写明「`redirectTo` 由**前端**执行跳转（`location.href`）」→
  浏览器会真的导航到 `https://client.example/cb?code=…&state=…`。
  在 Playwright 里，一个 DNS 解析失败的顶层导航会抛导航错误，**地址栏停留在跳转前**，
  `page.url()` 拿不到 `code` → **步骤 4 不可达**。
  且 `playwright.config.ts:3-11` 写明本仓**没有 CORS 层**、E2E 走**同源**，
  跨源顶层导航在同源策略下也没有任何后端参与。
- **证据**:
  - `code/playwright.config.ts:3-11`（「E2E 走 **同源**…本仓没有 CORS 层；跨源 fetch 会被浏览器拦掉 —— R17」）
  - `code/e2e/sso.spec.ts:43-50`（仓里已有的**跨源拦截范式**：`page.route('**/…', route => route.fulfill(...))`）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:188`（前端执行跳转）
- **建议改法**（可直接落笔）:
  > 步骤 4 改为：**拦截**第三方回调再读地址栏。
  > 1. E2E 先 `page.route('https://client.example/**', route => route.fulfill({ status: 200, contentType: 'text/html', body: '<html>ok</html>' }))`；
  > 2. 走完授权流程后读 `page.url()`；
  > 3. 断言 `new URL(page.url()).searchParams.get('code')` **非空**，
  >    `new URL(page.url()).searchParams.get('state')` **逐字等于** `xyz`。
  >
  > 并在 AC 里**显式写明这一步**（否则 Dev 会写成 `page.goto('https://client.example/cb')` → 必红）。

### S-02: 未登录时 OAuth 查询参数**被前端路由丢弃** → 「登录后回到原 query」的机制**不存在**

- **档位**: 物理不可达 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-137 步骤 1~2（:144-147）、AC-S-138 前置、AC-S-148 步骤 1；
  `decisions.md` D203（:206「未登录 → 引导登录（带 `return_to`）」）
- **AC 原文**: 「1. 浏览器访问 `{平台}/oauth/authorize?response_type=code&client_id=…&redirect_uri=…&scope=…&state=xyz&code_challenge=…`；
  2. 登录（`tenant-a` / 账号 / 密码）；」
- **问题**: 平台前端**没有 `return_to` 机制**，而且**守卫重定向会主动丢掉查询串**：
  - `packages/auth/src/index.ts:118-132` `guard()` 的 `redirectTo` 只有 `'/login'` / `'/403'`，**不带任何 query**；
  - `apps/shell/src/lib/router.ts:229-242` `write()`：`keepsQuery = path === environment.location.pathname`
    —— 从 `/oauth/authorize` 跳到 `/login` 时 **path 变了 → `suffix = ''` → query 被抹掉**；
  - `apps/shell/src/lib/router.ts:261` `const intendedPath = path;` —— **只有 pathname，没有 search**；
  - `apps/shell/src/App.svelte:120-125` `handleLoggedIn()` → `router.navigate(resolution.intendedPath ?? '/')`
    —— 登录后跳回的是 **`/oauth/authorize`（无 query）**。
  → 全仓 `grep -rn "return_to\|returnTo"` 在 `apps/shell` / `packages/auth` / `packages/sdk` 里**只命中
  `router.ts` 的 `intendedPath` 字段**，没有任何一处拼 `return_to`。
  → 结果：**登录完回到 `/oauth/authorize` 时，`client_id` / `state` / `code_challenge` / `redirect_uri` 全部消失**
  → AC-S-137 的步骤 2 之后没法继续，AC-S-148 的「免登」也没法验。
- **证据**:
  - `code/packages/auth/src/index.ts:118-132`
  - `code/apps/shell/src/lib/router.ts:229-242`（`write()`）
  - `code/apps/shell/src/lib/router.ts:252-268`（`apply()` / `intendedPath`）
  - `code/apps/shell/src/App.svelte:120-125`
  - `code/apps/shell/src/lib/routes.ts:68-111`（现有路由表里**没有** `/oauth/authorize`）
- **建议改法**（BA 二选一，然后同步改 D203 + AC）:
  > **选项 A（推荐）**：把 `/oauth/authorize` 登记为**公开路由**（像 `/sso/callback` 那样，
  > `routes.ts:70-76` 有现成范式：公开路由 + 页面自己处理「未登录」分支），
  > 页面在未登录时**自己**渲染登录入口，并把**完整 query** 带在登录链接上
  > （`/login?return_to=<encodeURIComponent(location.pathname + location.search)>`）；
  > `Login.svelte` 登录成功后读 `return_to` 再跳。
  > → 需要改 `apps/shell/src/pages/Login.svelte` + `App.svelte`（**不是** REQ-022 的文件清单里的，
  > **必须补进 design §13 的文件清单**）。
  >
  > **选项 B**：让 `/oauth/authorize` 走 `requiresAuth: true` 守卫，
  > 则**必须同时改** `router.ts:229-242` 的 `write()`，让它在守卫重定向时带上
  > `location.search`（否则 query 必丢）—— 这会动 REQ-011 的既有行为，**需在 D109 里显式声明**。

### S-03: AC-S-152 的 `TITLES` 行数断言只写「本需求新增数」→ **数字未给死**

- **档位**: 不可判定 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-152 预期结果第 1 条末（:461）
- **AC 原文**: 「`TITLES` 的行数断言同步更新（基线 58 + 21 + 5 + **本需求新增数**）」
- **问题**: 「本需求新增数」不是一个数 → **不可判定**（Dev 写 18 还是写 10，AC 都判得过）。
  正确值必须是 **84 + 10 + 8 = 102**：
  - 基线实测 **84**（`58 + 21 + 5`）；
  - 本需求 **10** 个平台错误码（D213 / acceptance.md:56-61）；
  - 本需求 **8** 个审计事件码（D215 / decisions.md:426-435，进 `TITLES` 末尾那一段）。
- **证据**（实测）:
  ```
  $ awk '/pub const TITLES/,/^\];/' crates/platform-core/src/titles.rs | grep -cE '^\s*\('
  84
  ```
  `code/crates/platform-server/tests/i18n_titles.rs:155-165`：
  ```rust
  const BASELINE_ROWS_BEFORE_REQ_013: usize = 58;
  const REQ_013_NEW_CODES: usize = 21;
  const REQ_014_NEW_CODES: usize = 5;
  assert_eq!(TITLES.len(), 58 + 21 + 5);
  ```
  `code/crates/platform-core/src/titles.rs:45`（`TITLES` 定义）
  `BA/demands/REQ-022-outbound-idp/decisions.md:424`（「埋 **8 个事件码**」）
- **建议改法**:
  > - `TITLES` 的行数断言同步更新为 **`58 + 21 + 5 + 10 + 8 = 102`**
  >   （`REQ_022_NEW_ERROR_CODES: usize = 10;` + `REQ_022_NEW_EVENT_CODES: usize = 8;`），
  >   并在 AC 里**逐字写出 102** 这个数字（不许写「本需求新增数」）。

### S-04: D215 的 8 个 `oauth.*` 事件码进 `TITLES` → **会让 `i18n_titles.rs` 的三个既有测试变红**

- **档位**: 口径不一致 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-152（:450-466）、:56-61（平台码清单）；`decisions.md` D215（:437-439）
- **AC 原文**: 「步骤 4：`all_codes()` 里**有** `for code in platform_oauth::error::ALL_CODES` 这一行」；
  「本需求新增了错误码（清单见 §全局约定）」（AC-S-152 前置）
- **问题**: D215 说事件码「走 `crates/platform-core/src/titles.rs` 的 `TITLES` 末尾那一段」，
  且「i18n 侧**不进** `error.*`」。但 `i18n_titles.rs` 的三个既有断言会让这个做法**必红**：
  1. **`codes_follow_their_namespace_convention`**（`i18n_titles.rs:285-300`）：
     键必须满足「`UPPER_SNAKE`」**或**「`org.` 前缀的小写 kebab-case」。
     `oauth.client.registered` **两者都不满足** → 断言失败。
  2. **`every_error_code_is_registered_in_every_locale`**（`:357-405`）：
     `error_codes()`（`:276-282`）只排除来源标签 `platform-org::notify` 的码。
     任何**新增来源**（比如 `platform-oauth::events`）都会被算成「错误码」
     → 要求两个 locale 都有 `error.oauth.client.registered` → **与 D215「不进 `error.*`」直接冲突**。
  3. **`no_orphan_titles`**（`:190-202`）：`TITLES` 的每个键都要被某条 `ALL_CODES` 清单认领
     → 8 个事件码必须有一条新的 `ALL_CODES` → 又触发第 2 条。
  → **照 D215 实现 = 跨栈测试变红；照现有测试实现 = 违反 D215**。
- **证据**:
  - `code/crates/platform-server/tests/i18n_titles.rs:285-300`（命名空间断言）
  - `code/crates/platform-server/tests/i18n_titles.rs:276-282` + `:357-405`（错误码 ↔ locale）
  - `code/crates/platform-server/tests/i18n_titles.rs:190-202`（孤儿标题断言）
  - `code/crates/platform-server/tests/i18n_titles.rs:47`（`ORG_EVENT_SOURCE = "platform-org::notify"`）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:437-439`（D215 的登记口径）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:486`（design §13 的 `platform-oauth/src/` 文件清单里**没有** 事件码模块）
- **建议改法**（BA 二选一）:
  > **选项 A（推荐，改动最小）**：8 个事件码**不进 `TITLES`**，
  > 只在 `platform-oauth` 内部用常量 + 单测覆盖 → 删掉 D215 的「走 TITLES 那一段」，
  > `TITLES` 行数 = **84 + 10 = 94**。
  >
  > **选项 B（保留事件码进 TITLES）**：必须**同步改** `i18n_titles.rs` 三处：
  > ① `codes_follow_their_namespace_convention` 的事件码前缀从 `org.` 放宽为 `org.` / `oauth.`；
  > ② `error_codes()` 的排除条件从「来源 == `platform-org::notify`」改成
  >    「**来源以 `::notify` 或 `::events` 结尾**」或新增 `ORG_LIKE_SOURCES` 白名单；
  > ③ `all_codes()` 加一行 `for code in platform_oauth::events::ALL_CODES`，
  >    并在 `crates/platform-oauth/src/` **新增** `events.rs`（补进 design §13 的文件清单）。
  > 此时 `TITLES` 行数 = **84 + 10 + 8 = 102**。
  > → 无论选哪个，AC-S-152 都必须**逐字写出最终数字**（94 或 102）。

### S-05: AC-S-151「`UNAUTHENTICATED` 或协议错误 `invalid_token`——**二者都接受**」—— **不可判定，且 `invalid_token` 不在契约的协议码集合里**

- **档位**: 不可判定 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-151 预期结果第 1 条（:441-442）
- **AC 原文**: 「步骤 1、2、3：**401**（`UNAUTHENTICATED` 或协议错误 `invalid_token`——
  **二者都接受，但同一端点必须一致**）」
- **问题**: 两个问题：
  1. **不可判定**：AC 说「二者都接受」→ 两个取值都算通过 → **这条判据永远判不出红**（dev-guide §9.12）。
  2. **`invalid_token` 不存在**：契约 v3 C14 §14.2 的协议码**取值集合**里**没有** `invalid_token`
     （集合是 `invalid_request` / `invalid_client` / `invalid_grant` / `unauthorized_client` /
     `unsupported_grant_type` / `unsupported_response_type` / `invalid_scope` / `access_denied` /
     `login_required` / `consent_required` / `authorization_pending` / `slow_down` / `expired_token` /
     `server_error`）；`acceptance.md:51-55` 的同一份清单也没有。
     `invalid_token` 来自 RFC 6750（资源服务器），**本契约没有登记它**。
  3. 而且正确答案其实是**唯一的**：被测的两个端点（`/api/rbac/me/permissions`、`/api/audit/events`）
     是**平台端点** → 按 C6 说平台话 → 只能是 `UNAUTHENTICATED`。
- **证据**:
  - `BA/contracts/api-conventions/v3.md:103-115`（C14 的端点分类表 + 协议码取值集合 + HTTP 状态码口径）
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:51-55`（同一份清单）
  - `code/crates/platform-iam/src/error.rs:56`（`pub const UNAUTHENTICATED: &str = "UNAUTHENTICATED";`）
  - `code/crates/platform-iam/src/error.rs:359`（`Self::Unauthenticated => StatusCode::UNAUTHORIZED`）
  - `code/crates/platform-iam/src/jwt.rs:152-156`（`validation.validate_aud = true; validation.aud = Some({平台 audience})`）
  - `code/crates/platform-iam/src/service.rs:1530-1533`（`verify_access_token` → `keys.verify(token, issuer, audience)`）
- **建议改法**:
  > - 步骤 1、2、3：**401** + 错误体 `code` **逐字等于** `UNAUTHENTICATED`，
  >   `Content-Type: application/json`，且**不含** `error` 键（协议错误体形状）。
  >   （OAuth 令牌的 `aud` 是 `client_id`，与平台 audience 不等 →
  >   `validate_aud` 必然失败 → 401；此外 OAuth 令牌**没有** `sid` 键，
  >   `AccessClaims` 反序列化也会失败 → **双重拒绝**。）
  > - 删除「或协议错误 `invalid_token`——二者都接受」。
  > - 若 BA 认为某些端点应说协议话，则该端点**必须**是 C14 的「OAuth 协议端点」，
  >   并在 AC 里**指名**是哪个端点 —— 不许用「二者都接受」兜底。

### S-06: AC-S-135 / AC-S-136 的前置「管理员账号持有 `oauth:client:write`」—— **仓里没有任何账号持有**

- **档位**: 物理不可达 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-135 前置（:101）、AC-S-136 前置（:122）
- **AC 原文**: 「**前置条件**: 管理员账号持有 `oauth:client:write`；测试库 `base_test` 可用。」/
  「**前置条件**: 已注册一个 `confidential` client，拿到过一次明文 secret。」
- **问题**:
  - `scripts/seed-e2e.sh` 只建 **4 个** E2E 账号（`u-perm-read` / `u-perm-write` / `u-perm-none` /
    `u-perm-audit`），授的权限点是 `rbac:role:read|write`、`sandbox:demo:*`、`audit:events:read`
    —— **没有任何 `oauth:*`**；
  - `oauth:client:write` 是**本需求新增**的权限点（D214），种子迁移
    `migrations/…oauth_permission_seed.up.sql` 在本需求落地前**不存在**；
  - → AC 的前置**没有任何一条路径**能让它成立，且 AC **没有写「谁建这个 client」**。
- **证据**:
  - `code/scripts/seed-e2e.sh:146-151`（4 个账号的 INSERT）
  - `code/scripts/seed-e2e.sh:177-187`（授权清单：`rbac:role:*` / `sandbox:demo:*` / `audit:events:read`）
  - `code/scripts/seed-e2e.sh:123-126`（脚本头部的用户清单表 —— 同上）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:397-412`（D214 的 6 个新权限点）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:466-472`（design §12 说「夹具由 `scripts/seed-e2e.sh` 播种」，
    但没说**谁拥有** `oauth:client:write`）
- **建议改法**:
  > - AC-S-135 前置改写为：「
  >   `tenant-a` 下存在 E2E 管理员账号 `u-perm-oauth@example.com`，
  >   **持有 `oauth:client:read` / `oauth:client:write` / `oauth:client:delete`**
  >   （由 `scripts/seed-e2e.sh` 第 N 步播种，**必须自证**这三个权限点已授权 ——
  >   与现有脚本「强断言 ②」的写法同款）；
  >   `client-A` / `client-B` / `public-client` 三个夹具 client **由 `scripts/seed-e2e.sh` 播种**，
  >   其 `client_secret_hash` 由脚本按 `digest_base64url` 口径写入（**明文 secret 由脚本打印到 stdout**，
  >   E2E 从 stdout / 夹具文件读）。」
  > - 把「用户 `e2e-user-oauth@example.com`」与「已注册 client」**都写成脚本的产出物**，
  >   而不是「环境里已有」。

### S-07: AC-S-149 的「**进程内** mock 接收端」—— 平台是**另一个进程**，E2E 里起不来

- **档位**: 物理不可达 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-149 前置（:390-391）、步骤 2/4/5（:395、:397-398）
- **AC 原文**: 「`client-A` 的 `backchannel_logout_uri` 指向一个**进程内**的 mock 接收端
  （HTTP，本地端口；**不得**依赖外部网络）」
- **问题**: 出站 back-channel 是平台**主动发起**的 HTTP POST，
  目标是 **client 的** `backchannel_logout_uri` —— 那个 URI 在**另一个进程**（或至少另一个监听套接字）上。
  平台的 E2E 服务是独立进程（`scripts/e2e-server.sh:88` `exec cargo run … base -- serve`），
  所以「进程内」只能指「**测试进程内**起一个监听端口」—— 但那样：
  - **端口是动态的** → `platform_oauth_client.backchannel_logout_uri` 里登记的端口**必须等 mock 起来之后才知道**
    → **夹具顺序**必须被规定（先起 mock → 再写 client 行），AC 完全没写；
  - 仓里**没有**现成的 mock 接收端范式：`scripts/` 下只有
    `e2e-server.sh` / `seed-e2e.sh` / `test-integration.sh` / `db-up.sh` 等，**没有任何 mock HTTP server**；
  - 「让 mock 超时」（步骤 4）与「让 mock 返回 500」（步骤 5）要求 mock **可切换行为** → 需要一个带开关的 mock。
- **证据**:
  - `code/scripts/e2e-server.sh:88`（`exec cargo run -q -p platform-server --bin base -- serve`）
  - `code/scripts/`（目录清单：无 mock server 脚本）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:472`（「一个 **mock 接收端**（进程内 HTTP server，供 back-channel logout 用）」——
    但没说是哪个进程、怎么起、端口怎么进 client 表）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:347-348`（back-channel POST / 超时 5 秒）
- **建议改法**（给一条可执行路径）:
  > - 把 mock 接收端**做成一个独立的、可开关的进程**：
  >   `scripts/mock-logout-receiver.sh`（或 Rust 集成测试内起的 hyper 服务），
  >   监听 `127.0.0.1:$MOCK_PORT`，把收到的 `logout_token` **写到文件** `target/mock-logout-received.jsonl`，
  >   行为由 `MOCK_MODE=ok|timeout|500` 环境变量控制（默认 `ok`）。
  > - `scripts/seed-e2e.sh` 增加第 N 步：**读 `$MOCK_PORT`**，
  >   把 `backchannel_logout_uri = 'http://127.0.0.1:' || $MOCK_PORT || '/logout'` 写进 client 行。
  > - AC 的步骤 2 改为「读 mock 落盘的文件（`target/mock-logout-received.jsonl` 的最后一行）」
  >   —— **不要**在 E2E 进程里 mock HTTP（那会把「出站真的发出去了」这件事 mock 掉）。
  > - AC 的步骤 4/5 改为「用 `MOCK_MODE=timeout` / `500` 重启 mock 后再触发登出」。

### S-08: AC-S-146 步骤 3「篡改 ticket」—— 要造出「**签名有效但 scope 不同**」的票据，测试侧**拿不到私钥**

- **档位**: 物理不可达 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-146 步骤 3、预期结果第 3 条（:332-333、:340）
- **AC 原文**: 「3. **篡改**同意票据（把 ticket 里的 `scope` 改成更大的集合后重新编码），
  调 `POST /api/oauth/authorize/confirm`；」 → 「步骤 3：**400** + `OAUTH_TICKET_INVALID`」
- **问题**: D204 的安全承诺是「**用户看到的 scope = 实际授予的 scope**」。
  要证明这一点，测试必须造出一个**签名有效、scope 更大**的票据 —— 那需要**测试侧自己持私钥**。
  而 E2E 的私钥**只在 `scripts/e2e-server.sh` 的 shell 变量里**，`export` 之后 `exec` 服务：
  - **不落盘**、**不入库**、**不进 Playwright 进程的环境**（Playwright 由 `webServer.command` 另起进程）；
  - → E2E **读不到私钥**。
  若只做 base64 重编码（AC 字面），签名必失 → `OAUTH_TICKET_INVALID` → **判据恒真**：
  它只证明「验签在工作」，**证明不了**「scope 不能被放大」—— 而这正是 D204 想证明的东西。
- **证据**:
  - `code/scripts/e2e-server.sh:65-73`（私钥现场生成 + `export`，**不落盘**）
  - `code/playwright.config.ts:43-46`（`webServer.command` 是**另一个进程**：`pnpm … build && ./scripts/e2e-server.sh`）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:219-236`（D204：票据是 ES256 签名、`confirm` 只信 ticket）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:305-317`（design §5.2 的票据 claims）
- **建议改法**（二选一，都要落进 AC）:
  > **选项 A（推荐）**：把 E2E 的 JWT 私钥**落盘**到 `target/e2e-jwt-private.pem`
  > （`e2e-server.sh:65-70` 改成「生成后写入该文件并 `export`」，
  > 文件权限 0600、**不入 Git**、每次运行覆盖）→ E2E 用它**自己签**一个
  > `typ="oauth_consent_ticket"`、`scope` **比 preview 返回的更大**的票据 →
  > 调 confirm → 必须 **400 + `OAUTH_TICKET_INVALID`**。
  > **这一条才真正证明 D204 的承诺。**
  >
  > **选项 B（不落盘）**：AC 的步骤 3 降级为「**重编码**（不重签）→ 400 + `OAUTH_TICKET_INVALID`」，
  > 并在 AC 里**显式标注**「本条只验验签，不验 scope 不可放大 —— 后者需要私钥，见 D211 补充」。
  > → 此时「用户所见 = 实际授予」**没有任何 AC 覆盖** → 必须在 AC-S-146 补一条
  > 「preview 返回的 `scopes` 集合 **逐字等于** confirm 签出的 code 的 scope」。

### S-09: AC-S-138 路径 1/2 的**失败点**未钉死（preview 就该失败，不是「签发 code 这一步」）

- **档位**: 不可判定 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-138 步骤 1~2、预期结果第 1 条（:167-173）
- **AC 原文**: 「路径 1、2：**签发 code 这一步**就失败 —— 平台错误 `OAUTH_INVALID_REQUEST`
  （缺挑战）或 `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD`（plain），HTTP **400**」
- **问题**: 按 D203（`decisions.md:199-203`），**参数校验在 `POST /api/oauth/authorize/preview`**，
  code 是 `POST /api/oauth/authorize/confirm` 才签的。所以：
  - 「签发 code 这一步就失败」在实现上**不成立** —— 失败发生在 **preview**，
    **根本走不到 confirm**；
  - Dev 若照字面在 confirm 里判，会让 preview 先返回一个「看起来正常」的 ticket
    → 用户在同意页点「允许」才炸 → **把参数错误推迟到了交互之后**；
  - 另一个洞：AC **没说**路径 1/2 的错误体是**平台话**（Problem Details，契约 C14 §14.4
    明确 `/api/oauth/authorize/*` 是平台端点）—— 只写了「平台错误 `OAUTH_…`」，
    没有断言 `code` 字段、`status` 字段、`Content-Type`。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/decisions.md:199-203`（preview = 「校验全部参数」）
  - `BA/contracts/api-conventions/v3.md:127-135`（C14 §14.4：授权请求端点是**平台自有端点**，说平台的话）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:139-141`（§3.1 的端点清单）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:251-256`（D206：PKCE 校验点与错误码）
- **建议改法**:
  > - 路径 1：调 `POST /api/oauth/authorize/preview`（body **不含** `code_challenge`）
  >   → **400** + Problem Details，`code` **逐字等于** `OAUTH_INVALID_REQUEST`，
  >   `status == 400`，`Content-Type: application/json`；响应体**不含** `ticket` 键；
  >   → **不得**调用 `confirm`、**不得**产生任何授权码行。
  > - 路径 2：同 preview，带 `code_challenge_method=plain` →
  >   **400** + `code == "OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD"`。
  > - 路径 3：**先** preview 成功（拿 ticket）**再** confirm 时传错的 `code_verifier`
  >   → **400** + `{"error":"invalid_grant"}`（**协议错误体**，因为 `/api/oauth/token` 是协议端点）。

### S-10: AC-S-151 步骤 3 测的是「**没有 client 认证**」，不是「OAuth 令牌被拒」—— 判据与标题对不上

- **档位**: 不可判定 ｜**级别**: 阻塞
- **位置**: `acceptance.md` AC-S-151 标题 + 步骤 3（:432、:438）、预期结果第 1 条（:441）
- **AC 原文**: 「### AC-S-151: 底座业务端点**默认拒绝**第三方 OAuth 令牌」/
  「3. 用该令牌调 `POST /api/oauth/introspect`（不带 client 认证）；」
- **问题**: `/api/oauth/introspect` 的认证是 **client 认证**（D208：只有 confidential client 能调），
  **不是** Bearer 令牌。所以步骤 3 里那个 OAuth 令牌**从头到尾没被当成令牌解析**——
  它只会被当成「一个 Authorization 头」→ 缺 client 认证 → 401。
  → **这条判据证明的是「没带 client 认证会 401」，与标题「底座业务端点默认拒绝 OAuth 令牌」无关**，
  也**没有测到** D209 的核心（OAuth 令牌在业务端点上被拒）。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/decisions.md:281-288`（D208：内省**必须** client 认证）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:302-304`（D209：业务端点一律不接受 OAuth 令牌 → 401）
  - `BA/contracts/api-conventions/v3.md:97-99`（C14 把 `/introspect` 列为**协议端点**）
- **建议改法**:
  > 步骤 3 改为：「用该 OAuth 令牌调 `GET /api/rbac/me/permissions`（**同一个**端点，步骤 1 已在用）
  > → **401** + `code == "UNAUTHENTICATED`」。
  > 并**新增**步骤 3.5：「用该 OAuth 令牌调 `POST /api/oauth/introspect`（**带** client-A 的合法
  > `client_secret_basic` 认证，但 `token` 参数填该 OAuth 令牌）→ **200** + `active == true`
  > （证明该令牌**只**在 OAuth 协议端点上有效，业务端点拒它）」。

---

## 高（19）

### S-11: AC-S-134 的 `authorization_endpoint = {issuer}/oauth/authorize` 与 design §7 的 `oauth.frontendBaseUrl` **自相矛盾**

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-134 预期结果（:80）；`design-summary.md` §7（:358）
- **AC 原文**: 「`authorization_endpoint` = `{issuer}/oauth/authorize`（**前端路由**，D203）」
- **问题**: design §7 定义了配置项 **`oauth.frontendBaseUrl`**（scope `platform`，默认 `""`，
  说明「授权页 / 同意页的前端基址」）—— **它存在的理由就是「前端基址 ≠ issuer」**。
  但 AC-S-134 把 `authorization_endpoint` 钉成 `{issuer}/oauth/authorize`，
  **没有说 `frontendBaseUrl` 与 issuer 何时相等**。
  后果：第三方按 metadata 拼授权 URL → 跳到 issuer 的主机；
  而平台的 SPA 挂在 `BASE_STATIC_DIR` 服务的根上 → **两者不同时就是死链**。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:356-362`（§7 的 5 个配置键，`oauth.frontendBaseUrl` 默认 `""`）
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:80-85`
  - `code/scripts/e2e-server.sh:72`（E2E 的 `BASE_JWT_ISSUER` 默认 `https://e2e.base`）——
    而 E2E 的真实服务地址是 `http://127.0.0.1:8080`（`playwright.config.ts:12`）→ **两者不等**
- **建议改法**:
  > - `authorization_endpoint` = **`{frontendBaseUrl}` + `/oauth/authorize`**，
  >   其中 `{frontendBaseUrl}` = 配置 `oauth.frontendBaseUrl`；
  >   **未配置时回退到 `{issuer}`**（把这条回退规则写进 AC 与 design §7）。
  > - AC-S-134 的断言改成「`authorization_endpoint` 的 host 部分 == 配置 `oauth.frontendBaseUrl`
  >   的 host（或未配置时 == `BASE_JWT_ISSUER` 的 host）」，**不要**钉死 `{issuer}`。
  > - 顺带把 `oauth.frontendBaseUrl` 的**默认值**从 `""` 改成「空 = 用 issuer」并在 AC 里写明。


### S-12: AC-S-137 / 138 / 148 写「**{平台}**/oauth/authorize」，AC-S-134 写「**{issuer}**/oauth/authorize」—— 同一个 URL 两种基准

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` :144（AC-S-137 步骤 1）、:374（AC-S-148 步骤 1）、:80（AC-S-134）
- **问题**: AC 内部对「授权页的基址」有**两种写法**，且 E2E 里二者**不相等**（见 S-11 的实测）。
  一个照字面实现的 Dev 会用 `{issuer}`（从 metadata 取）→ 在 E2E 里跳到 `https://e2e.base` → **不可达**。
- **证据**: 同 S-11；`code/playwright.config.ts:12`（`BASE_E2E_URL` 默认 `http://127.0.0.1:8080`）
- **建议改法**:
  > 全仓统一成一个符号：**`{授权页基址}`**，并在 §全局约定里定义它 =
  > 「`BASE_E2E_URL`（E2E）/ `BASE_PUBLIC_BASE_URL`（生产）的 origin」，
  > 与 `{issuer}`（= `BASE_JWT_ISSUER`）**明确区分**。


### S-13: AC-S-153 步骤 1 的 `cargo xtask capabilities --check` —— **基线上就已经是红的**，而且**它不做 C-5**

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-153 步骤 1、预期结果第 1 条（:475、:480）
- **AC 原文**: 「1. 跑 `cargo xtask capabilities --check`（或契约检查的等价命令）；」/
  「步骤 1：**通过**（14 个文件与源码一致，OAuth 的 manifest 计入）」
- **问题**: 三件事：
  1. **基线已红**（实测退出码 1）：`crates/xtask/src/catalog.rs:147-149` 仍写着
     「`audit_sink`：审计只到事件声明面，落库与查询**待 REQ-014**」——REQ-014 已合并，
     `capabilities/index.json` 因此过期 → **`--check` 现在就失败**。
     → AC 写「通过」= 要求 Dev 先还一笔**别的需求的债**，否则第一条判据就是红的。
  2. **它不做 C-5**：`capabilities.rs` 只比对**索引产物**与磁盘，不跑任何契约检查。
     用它当「C-5 在本 crate 上通过」的判据 → **判据与目标不匹配**。
  3. **「14 个文件」不会变**：新增 crate **不新增** `capabilities/` 下的文件（实测 14 个），
     变的是 `permissions.json` 的**内容**（`total` 26 → 32）。
     「OAuth 的 manifest 计入」如果被读成「多一个 `capabilities/oauth.json`」就错了。
- **证据**（实测）:
  ```
  $ ./target/debug/xtask capabilities --check --root /home/myg133/Projects/system_seed/code
  能力索引校验失败：错误码 E_CAPABILITIES_OUTDATED
    ✗ 内容不一致：capabilities/index.json
    修复方式：运行 `cargo xtask capabilities` 重新生成
  EXIT=1
  ```
  ```
  $ find capabilities -type f | wc -l
  14
  $ grep '"total"' capabilities/index.json
  "total": 26
  ```
  `code/crates/xtask/src/catalog.rs:147-149`（过期的 `audit_sink` 文案）
  `code/crates/xtask/src/capabilities.rs:226-275`（`permissions_json` 扫 `modules/` + `crates/` 的 manifest）
  `code/crates/xtask/src/capabilities.rs:599-601`（`module_dir_names` → 新 crate 会被收进去）
  `code/crates/platform-contracts/src/checks.rs:505-560`（`check_c5_for` —— 平台 crate 的 C-5 入口，**与 xtask 无关**）
  `code/crates/platform-audit/tests/contract_c5.rs:24-64`（REQ-014 的 C-5 测试范式：**每个 crate 一个 `tests/contract_c5.rs`**）
- **建议改法**:
  > - 步骤 1 拆成两条：
  >   1a. `cargo xtask capabilities` **重新生成**（**先还 REQ-014 的 `catalog.rs` 债**，或在 AC 里
  >       写明「本需求必须先把 `capabilities/` 刷新到与源码一致」）；
  >       断言：`capabilities/` 下**恰好 14 个文件**（**不增不减**），
  >       且 `capabilities/permissions.json` 的 `total == 32`（26 + 6 个 `oauth:*`），
  >       且含 `oauth:client:read` / `oauth:client:write` / `oauth:client:delete` /
  >       `oauth:grant:read` / `oauth:grant:revoke` / `oauth:token:introspect` **逐字 6 条**。
  >   1b. **C-5 判据换成真命令**：`cargo test -p platform-oauth --test contract_c5`
  >       （**该测试文件必须由本需求新增**，补进 design §13 的文件清单）。
  > - 步骤 3 / 4 的「再跑 C-5」也必须**逐字写出这条命令**（现在只写「跑 C-5」→ 不可执行）。


### S-14: AC-S-140 路径 4 的「二者必须能区分开」—— **用 AC 给的方法做不到**

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-140 额外断言第 2 条（:216-218）
- **AC 原文**: 「**额外断言**：`rbac:role:*` 之所以被拒，是因为它**不是** `platform_permission` 表里的
  权限点（测试侧查表确认 `rbac:role:*` **不存在**）—— 不是因为「通配被禁用」，
  二者必须能区分开（用 `rbac:role:read` 走路径 1 证明普通权限点可用）。」
- **问题**: 两个原因**同时成立**，AC 给的区分方法**在两种实现下都通过** → 无区分力：
  - 原因 A「不是注册过的权限点」：`platform_permission` 里确实没有 `rbac:role:*`（**实测**）；
  - 原因 B「通配被禁用」：也成立 —— 因为**用户的有效权限点集合里永远只有展开后的具体码**，
    `PermissionPattern::expand()` 返回的是**注册表里的具体码**，**永不返回通配字面量**。
  → 所以「`rbac:role:*` ∉ 用户有效权限点」在**任何**实现下都成立 →
  **无法用它区分 A 与 B**；而 AC 提的「用 `rbac:role:read` 走路径 1 证明普通权限点可用」
  在 A、B 两种实现下**都成立** → 同样无区分力。
- **证据**（实测 + 源码）:
  ```
  $ podman exec base-postgres psql -U base -d base_test -tAc \
    "SELECT code FROM platform_permission WHERE code LIKE '%*%';"
  （空 —— 注册表里没有任何通配码）
  $ ... "SELECT code FROM platform_permission WHERE code LIKE 'rbac%';"
  rbac:permission:read / rbac:role:read / rbac:role:write / rbac:user-role:write
  ```
  `code/crates/platform-core/src/extension/permission.rs:239-247`（「**已展开通配**，**升序且去重**」）
  `code/crates/platform-rbac/src/service.rs:1013-1038`（`expand_user_permissions` → `pattern.expand(registry)`）
  `code/crates/platform-rbac/src/entity.rs:232-241`（`expand` 返回注册表里的**具体码**）
  `BA/demands/REQ-022-outbound-idp/decisions.md:157-160`（D201 §3：通配只有本身是注册过的权限点才有效）
- **建议改法**:
  > - 删掉「二者必须能区分开」这句**不可判**的断言，改成两条**可判**的：
  >   1. 测试侧查表：`SELECT count(*) FROM platform_permission WHERE code = 'rbac:role:*'` **== 0**；
  >   2. 用户有效权限点集合里**不含**任何含 `*` 的码
  >      （`GET /api/rbac/me/permissions` 的返回集合里 `NOT LIKE '%*%'`）；
  >   3. **再补一条真正能区分的对照**：把 `rbac:role:*` **登记进 `platform_permission`**
  >      （管理员直连 INSERT，仅本用例夹具）→ 重跑路径 4 →
  >      **仍然** 400 `invalid_scope` → 这才证明「令牌 scope 层面通配**一律**不生效」，
  >      而不是因为「它没注册」。
  >      （这条要求 AC 显式写明「测试要往 `platform_permission` 插一行」，否则会被判为越权造数。）


### S-15: AC-S-140「即使用户后来被授予 write，也不得回溯扩大已签发的 scope」—— **这条断言的对象不存在**

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-140 额外断言第 1 条（:214-215）
- **AC 原文**: 「**额外断言**：路径 2 的令牌**不含** `rbac:role:write`
  （即使用户后来被授予 write，也不得回溯扩大已签发的 scope）」
- **问题**: 路径 2 的预期是 **400**（`:213`「路径 2、3、4：**400**，错误为 `invalid_scope`」）
  → **路径 2 根本没有签发任何令牌** → 「已签发的 scope 不被回溯扩大」**没有对象**。
  → 这条断言在任何实现下都「成立」（因为压根没有令牌）→ **假绿**。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:213`（路径 2 → 400）
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:214-215`（额外断言）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:154-156`（D201 §2：交集为空 → `invalid_scope`）
- **建议改法**:
  > 把这条**拆成一条新路径 5**：
  > 1. 用户**只有** `rbac:role:read` → 请求 `scope=rbac:role:read` → 交换**成功** → 令牌 A（`scope == "rbac:role:read"`）；
  > 2. 管理员给该用户**追加** `rbac:role:write`（`platform_role_permission` + bump `platform_permission_epoch`）；
  > 3. **重放**令牌 A（不重新交换）→ 内省 → `scope` **仍然逐字等于** `rbac:role:read`，
  >    **不含** `rbac:role:write`；
  > 4. 另发一枚令牌 B（同一 scope 集 `rbac:role:read rbac:role:write`）→ 令牌 B 的 scope **含** `write`
  >    → 证明「授予**新**令牌会拿到新权限，但**旧**令牌不被回溯扩大」。
  > （第 3 步不需要「重新交换」—— 重新交换会拿到新 scope，**测不到**「不回溯」。）


### S-16: AC-S-139 / AC-S-142 的「SHA-256 摘要」**没说编码形式** —— 与 AC-S-136 的口径不一致

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-136 步骤 2（:125）、AC-S-139 预期结果第 1 条（:190）、
  AC-S-142 补充判据（:261-262）
- **AC 原文**: AC-S-136：「测试侧**独立实现** SHA-256 + base64url（**不调用生产代码**）」；
  AC-S-139：「库里的值 **等于** 测试侧独立计算的 `SHA-256(code)` 摘要」；
  AC-S-142：「`user_code` 与 `device_code` 在库里**都只存摘要**」
- **问题**: 仓里现成**两个**摘要函数，编码不同：
  - `digest_base64url`（SHA-256 → **base64url 无填充**）
  - `digest_hex`（SHA-256 → **小写 hex**）
  AC-S-136 钉了 base64url，AC-S-139 / AC-S-142 只写「SHA-256 摘要」→ **两处口径不一致**。
  Dev 若按「SHA-256 摘要」默认理解成 hex → AC-S-139/142 红、AC-S-136 绿 →
  「按错的规格实现 + 按错的规格验收」的老问题。
- **证据**:
  - `code/crates/platform-iam/src/crypto.rs:56-60`（`digest_base64url`：`URL_SAFE_NO_PAD.encode`）
  - `code/crates/platform-iam/src/crypto.rs:62-68`（`digest_hex`）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:140-143`（D200：摘要用 `digest_base64url`）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:61`（§2.1：client_secret_hash = SHA-256 base64 摘要）
- **建议改法**:
  > 把 §全局约定里加一行：
  > 「**摘要编码**：本需求所有「只存摘要」的列（`client_secret_hash` / `code_hash` /
  > `device_code_hash` / `user_code_hash`）一律用 **`digest_base64url` 口径**
  > —— SHA-256 原始 32 字节 → **base64url 无填充**（43 字符）。
  > **不是** hex。测试侧独立实现时按此口径复算。」
  > 并把 AC-S-139 / AC-S-142 的「SHA-256 摘要」改成「SHA-256 的 base64url 无填充摘要」。


### S-17: AC-S-136 步骤 4「全仓扫描 `client_secret` 的**明文**是否出现在任何响应构造点」—— **没有可实现的宿主与口径**

- **档位**: 不可执行 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-136 步骤 4、预期结果第 4 条（:127、:131）
- **AC 原文**: 「4. 全仓扫描 `client_secret` 的**明文**是否出现在任何响应构造点。」/
  「步骤 4：**0 命中**（且扫描必须带**阳性对照** —— 在临时副本里塞一个明文，扫描器必须命中）。」
- **问题**: 「全仓」指哪些目录？扫什么模式？Rust 源码里根本不会出现明文 secret
  （它是运行时生成的），所以「扫源码」必然 0 命中 —— 这正是 REQ-014 踩过的坑
  （REQ-014 挑刺 S-08：扫构造点字面量必然漏报）。
  「在临时副本里塞一个明文」也**没定塞在哪**、**扫描器怎么实现**（Rust 侧 / 脚本侧）。
  → **判据没有宿主**。
- **证据**:
  - `BA/feature-meta/qa-req014-spec-review.md` S-08（REQ-014 同款问题的结论：扫构造点字面量 → 必然漏报）
  - `code/crates/platform-contracts/src/authz_scan.rs:66-135`（仓里**唯一**成型的扫描器范式：
    扫**函数调用点**，不是扫字面量）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:76-78`（零新增依赖 → 扫描器只能用仓里现成手段）
- **建议改法**:
  > 把步骤 4 换成**可实现**的两条：
  > 1. **运行期断言**（主判据）：用真后端跑一遍
  >    `POST /api/oauth/clients` → 拿到明文 secret →
  >    `GET /api/oauth/clients`、`GET /api/oauth/clients/{id}`、
  >    `POST /api/oauth/token` 的**响应体**、`GET /api/audit/events` 的**全表文本** →
  >    **逐字**不含该明文（**用真值断言**，不是「没看到某个键」）。
  > 2. **静态扫描**（辅判据，必须带阳性对照）：扫 `crates/platform-oauth/src/` 里
  >    **构造响应体的位置**（`Json(...)` / `Serialize` 派生）是否出现 `client_secret` 字段名，
  >    并在**临时副本**里塞一个 `client_secret` 字段断言扫描器命中。
  >    → **扫描器必须可单独调用**（Rust 侧一个 `pub fn`），否则无法做阳性对照。


### S-18: AC-S-143 步骤 5 / 6 的期望都是 **401** → **判据无区分力**，且未写错误体形状

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-143 步骤 5/6、预期结果第 5 条（:275-276、:283）
- **AC 原文**: 「5. `public` client 内省任意令牌；6. 不带任何认证内省。」/
  「步骤 5、6：**401**（D208：public client 无权内省）。」
- **问题**:
  1. **无区分力**：步骤 5（public client 带 `client_id`、不带 secret）与步骤 6（什么都不带）
     在 D207 的口径下**都**是「client 认证失败」→ **都 401** → 两条判据合并后
     **分不出「public client 被单独拒绝」与「认证缺失被拒」**；
  2. **错误体形状缺失**：`/api/oauth/introspect` 是 **C14 的协议端点**
     → 错误体应是 `{"error":"invalid_client"}`，**不是**平台 Problem Details。
     AC 只写「401」，与 §全局约定「协议端点说 RFC 的话」**冲突**（没写形状 = 判不出）。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/decisions.md:263-272`（D207：public client 只带 `client_id`；认证失败统一 `invalid_client` + 401）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:281-283`（D208：public client 无权调用）
  - `BA/contracts/api-conventions/v3.md:97-99`、`:103-106`（`/introspect` 是协议端点 → RFC 错误体）
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:48`（§全局约定的协议端点清单里含 `/introspect`）
- **建议改法**:
  > - 步骤 5 期望：**401** + 响应体**逐字**等于 `{"error":"invalid_client"}`
  >   （**不得**含 `type` / `title` / `status` / `instance` / `code` / `trace_id` ——
  >   契约 C14 §14.2 明文禁止）；
  > - 步骤 6 期望：**401** + 同样形状；
  > - **再补一条能区分的**：步骤 5.5「public client 用 `client_secret_basic` 认证
  >   （secret 为空串）→ 同样 **401 + `invalid_client`**」→ 三条都是 401，
  >   但**必须都是 `invalid_client`**（不是 `OAUTH_*` 平台码）→ 这才是 D208 的可判形态。
  > - 另：内省的**认证失败**与**令牌不属于本 client**要分开：
  >   后者是 **200 + `{"active":false}`**（步骤 2），**不是** 401。


### S-19: AC-S-145 要求 payload 的 `sub` **齐全** —— 与 AC-S-141 / D198「`client_credentials` 省略 `sub`」冲突

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-145 预期结果第 2 条（:317-318）、AC-S-141 预期结果第 3 条（:235）
- **AC 原文**: AC-S-145：「payload 的 `iss` …、`aud` **等于** `client-A` 的 `client_id`，
  `sub` / `tid` / `client_id` / `scope` / `grant_type` / `iat` / `nbf` / `exp` / `jti` **齐全**」；
  AC-S-141：「步骤 3：payload 里**没有** `sub` 键（**不是**空串、不是 null —— 键不存在）」
- **问题**: AC-S-145 的前置只说「`client-A` 持有一枚**有效**令牌」——**没说是哪种 grant**。
  若那枚是 `client_credentials` 的，两条 AC **直接矛盾**（一条要 `sub` 齐全，一条要 `sub` 不存在）。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/decisions.md:89-93`（D198 §4：`sub` = 用户 ID，`client_credentials` grant 时**省略** `sub`）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:285-295`（§5.1 同一张表）
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:235`（AC-S-141）
- **建议改法**:
  > - AC-S-145 前置改成：「`client-A` 持有一枚**由 `authorization_code` grant 签发**的有效令牌」；
  > - 并**新增**一条 AC-S-145 补充判据：把同一套断言对 **`client_credentials`** 令牌再跑一遍，
  >   期望 `sub` **键不存在**、其余 claims 齐全（与 AC-S-141 一致）。


### S-20: AC-S-150 的「响应体里 `frontChannel` **数组**」与 design §4.5「返回 **HTML**，内含 iframe」**矛盾**

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-150 步骤 1、预期结果第 1 条（:418、:423-425）；
  `design-summary.md` §4.5（:349-351）
- **AC 原文**: AC-S-150：「步骤 1：触发全局登出，**读响应体**」/
  「步骤 1：响应体里 `frontChannel` 数组**恰好 1 条**（**只有** client-A），
  含 `clientId` 与 `logoutUrl`」
- **AC 原文（design）**: 「**Front-Channel**：返回 **HTML**，内含每个 client 一个
  `<iframe src="{frontchannel_logout_uri}?logout_token=…">`」
- **问题**: 一个是 **HTML 页面**，一个是 **JSON 数组** → 判据不可能同时成立。
  而且 AC 的措辞「读响应体」**没有区分**「HTTP 响应体」与「渲染后的 DOM」——
  若实现返回 HTML，JSON 数组判据不可判；若实现返回 JSON，「前端渲染 iframe」这件事
  **根本没有 AC 覆盖**（design §4.5 说「前端渲染 iframe」，AC 却查后端数组）。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:349-351`
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:258-261`（§4.5 时序第 4 步：「收集有 frontchannel_logout_uri 的 client → **返回 frontChannel 数组**」）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:351`（「必须加 `sandbox` 属性与 `X-Frame-Options` 处理」）
- **建议改法**:
  > BA 二选一并把 design §4.5 与 AC 同步：
  > **选项 A（推荐，保留 HTML）**：
  > - `POST /api/oauth/logout` 返回 **`Content-Type: text/html`** 的页面，
  >   内含**恰好 1 个** `<iframe>`，其 `src` **逐字等于**
  >   `client-A.frontchannel_logout_uri` + `?logout_token=<JWT>`；
  >   `client-B` **不在**其中；
  > - 每个 iframe **必须**带 `sandbox` 属性（**至少** `allow-scripts`，
  >   且**不含** `allow-same-origin`）；
  > - 响应**必须**带 `X-Frame-Options: DENY`（或 CSP `frame-ancestors 'none'`）；
  > - 另加一条**后端可判**的辅助判据：登出响应里**不含** `client-B` 的 `logoutUrl`。
  > **选项 B（保留 JSON）**：登出返回 JSON `{loggedOut, frontChannel:[{clientId, logoutUrl}]}`，
  > 前端自己渲染 iframe → AC 加一条**前端**用例（`apps/shell/test/` 或 E2E）断言 iframe 的
  > `src` / `sandbox` / `X-Frame-Options`。
  > → 无论选哪个，「`sandbox` 与 `X-Frame-Options`」必须**进 AC**（design 提了、AC 没提 = 无判据）。


### S-21: AC-S-142 的 `verification_uri` **指向哪没定**；RFC 8628 的 `user_code` 格式与 `slow_down` **完全没覆盖**

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-142 步骤 1/3、预期结果（:247-249、:252、:256-262）
- **AC 原文**: 「1. `POST /api/oauth/device_authorization` … → 读 `device_code` / `user_code` /
  `verification_uri` / `verification_uri_complete` / `expires_in` / `interval`；」/
  「3. 浏览器访问 `verification_uri`，输入 `user_code`，点「允许」；」
- **问题**:
  1. **`verification_uri` 指向哪没定**：`{issuer}/oauth/device`？`{frontendBaseUrl}/oauth/device`？
     AC 没写，design §7 的 5 个配置里**也没有**「设备码页基址」，
     D218 只说路由是 `/oauth/device`（前端路由）→ **绝对 URL 的基准缺失**；
  2. **RFC 8628 §6.1** 要求 `user_code` **8 个字符**、只能用 **base20 字符集**
     （`BCDFGHJKLMNPQRSTVWXZ`）→ AC **一个字都没测**；
  3. **RFC 8628 §3.5** 的 **`slow_down`**（轮询快于 `interval` → 返回 `slow_down` 并把间隔加 5 秒）
     → AC **没测**；
  4. design §2.4 的 **`poll_interval_seconds` / `last_polled_at`** 两列是限流的依据
     → **没有任何 AC 覆盖**（不覆盖 = 限流没被验收）。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:101-113`（§2.4 的两张列）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:480-487`（D218：设备码页路由 `/oauth/device`）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:356-362`（§7 的 5 个配置里**没有**设备码页基址）
  - `BA/contracts/api-conventions/v3.md:108-111`（协议码集合里有 `authorization_pending` / `slow_down`，
    但 AC-S-142 只用了 `authorization_pending`）
- **建议改法**:
  > - AC 补：`verification_uri` **逐字等于** `{授权页基址}/oauth/device`（与 S-11 的符号统一），
  >   `verification_uri_complete` **逐字等于** `{授权页基址}/oauth/device?user_code=<urlencoded user_code>`；
  > - AC 补：`user_code` 长度 **== 8**，且**逐字符** ∈ `BCDFGHJKLMNPQRSTVWXZ`（base20）；
  > - AC 补（限流）：拿到令牌前**连续轮询 2 次**（间隔 < `interval`）→
  >   第二次返回 **400 + `{"error":"slow_down"}`**；且设备码行的 `poll_interval_seconds`
  >   **被更新为** `interval + 5`；
  > - AC 补（列覆盖）：`last_polled_at` 在轮询后**非空**且**递增**。


### S-22: AC-S-155 的 4 张表没有 manifest `tenantScoped` 声明 → **平台的 RLS 自检不覆盖它们**

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-155（:510-526）；`design-summary.md` §8 的 manifest 模板（:377-397）
- **AC 原文**: 「1. 以 `tenant-a` 的上下文查 `platform_oauth_grant` / `platform_oauth_token` /
  `platform_oauth_code` / `platform_oauth_device_code`；」
- **问题**: 平台有**两条**并存的做法，而 REQ-022 的 design **一条都没选**：
  - `crates/platform-db/src/scopes.rs:5` 明写「RLS 策略的**唯一来源**是业务模块 manifest 的
    `tenantScoped` 段」；`:91-93` 没有该段的 manifest **被跳过**；
  - `cargo xtask tenant-policies --check`（`xtask/src/cli.rs:114-133`）就是靠这份声明
    **逐表校验 policy 存在**；
  - REQ-022 的 `crates/platform-oauth/manifest.json` 模板（design §8）**没有 `tenantScoped` 段**
    → 这 4 张表**不在** `xtask tenant-policies --check` 的覆盖范围内
    → **平台自己的 RLS 自检查不到它们**（漏建 policy 不会被 CI 发现）；
  - 既有先例：`platform-audit/manifest.json` **也没有** `tenantScoped`（策略手写在迁移里
    `migrations/20261120000200_audit.up.sql:146-182`）→ 所以「手写」这条路也走得通，
    **但那 4 张表就永远不在平台检查里**。
  → AC 必须把「谁来建 policy、谁来验 policy」钉死。
- **证据**:
  - `code/crates/platform-db/src/scopes.rs:5`、`:56-61`、`:91-93`
  - `code/crates/xtask/src/cli.rs:114-133`（`tenant-policies --check`：「有表漏建 policy 时以非零码退出（CI 用）」）
  - `code/modules/sandbox/manifest.json:21-29`（有 `tenantScoped` → 策略由 xtask 生成）
  - `code/modules/sandbox/migrations/*.sql`（**零条** `POLICY` 语句 → 证明 sandbox 走的是生成路径）
  - `code/crates/platform-audit/manifest.json`（无 `tenantScoped`）+ `code/migrations/20261120000200_audit.up.sql:146-182`（手写路径）
  - `code/crates/platform-db/src/rls.rs:36`（`POLICY_SUFFIX = "_tenant_isolation"`）
- **建议改法**:
  > **推荐**：在 `crates/platform-oauth/manifest.json` 里**加** `tenantScoped` 段：
  > ```json
  > "tenantScoped": {
  >   "tables": ["platform_oauth_grant", "platform_oauth_code",
  >              "platform_oauth_device_code", "platform_oauth_token"],
  >   "column": "tenant_id"
  > }
  > ```
  > → 策略由 `cargo xtask tenant-policies` 生成，命名自动是 `<table>_tenant_isolation`
  > （`rls.rs:36`），与仓里**现有 31 张表**的命名完全一致（实测）。
  > → AC-S-155 补一条：「`cargo xtask tenant-policies --check` 退出码 **0**，
  > 且 `pg_policies` 里这 4 张表各有 1 条 `<table>_tenant_isolation` policy」。
  > （若 BA 坚持手写，则 AC 必须写明「这 4 张表**不在** `xtask tenant-policies` 的检查范围内」，
  > 并补一条**直接查 `pg_policies`** 的判据。）


### S-23: AC-S-155 的 RLS 判据**只覆盖读侧**，没覆盖写侧（`WITH CHECK`）

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-155 预期结果第 4 条（:522-523）
- **AC 原文**: 「**RLS 真在生效的证据**：以 `base_app` 角色（**不**手动 `set_config`）查 →
  返回 **0 行**（RLS 默认隔离）→ 再在**单个事务内** `set_config` → 查 → 有行；」
- **问题**: 读侧判据**是对的**（我实测复现了，见 §6.4），但只验读侧 →
  **写侧漏写**（`WITH CHECK` 缺失 → 应用层能往表里写别的租户的行：写得进去、读不出来）
  **没有任何 AC 覆盖**。仓里所有既有 policy 都是 `USING ... WITH CHECK ...` 成对出现
  （`rls.rs:141-146`；实测 31 张表的 policy 都是这个形状）。
- **证据**:
  - `code/crates/platform-db/src/rls.rs:138-146`（`CREATE POLICY … USING (…) WITH CHECK (…)`）
  - `code/migrations/20261118000200_tenant_isolation.up.sql:11-13`（「为什么还要 `WITH CHECK`：
    没有它，应用层可以往表里写入别的租户的行（写得进去、读不出来 —— 静默的写侧泄漏）」）
- **建议改法**:
  > AC-S-155 补一条判据：
  > 「以 `base_app` 角色，在**单个事务内** `set_config('app.tenant_id','tenant-a')` 后
  > `INSERT` 一行 `tenant_id = 'tenant-b'` → **失败**（RLS 的 `WITH CHECK` 拒绝）；
  > 同一事务内 `INSERT` 一行 `tenant_id = 'tenant-a'` → **成功**。」
  > 并在 AC 里写明「`platform_oauth_client` **不参与**本条（平台级表，无 `tenant_id`、不启用 RLS）」。


### S-24: AC-S-149 的 `logout_token` 的 `sid` —— **多会话时取哪个，没定义**

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-149 预期结果第 3 条（:402-404）；`decisions.md` D211（:342-346）
- **AC 原文**: 「payload 的 `iss` …、`sub` / `sid` / `jti` / `iat` **齐全**」
- **问题**: D211 写了 `sid`（会话 ID）但**没说全局登出（作废全部会话）时 `sid` 取哪个**。
  平台的用户会话是**一表多行**（`platform_iam_session` 带 `family_id` / `revoked_at` / `last_used_at`，
  **实测 277 行**）→ 一个用户同时有多个会话是**常态**。
  一次全局登出作废全部会话，却只发**一个** `logout_token`（一个 `aud`、一个 `sid`）
  → 语义上应当**每个会话一个 `logout_token`**，D211 没定。
  AC 只断言「`sid` 齐全」→ 两种实现（取当前会话 / 取任意一个 / 按会话逐个发）**都判得过**。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/decisions.md:353-355`（D211 §5：全局登出 = 作废**全部会话**）
  - `code/crates/platform-iam/src/store.rs:764-778`（`revoke_all_refresh_tokens` —— 只按 `user_id` 作废 refresh token）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:263-275`（§4.5 的 `logout_token` 载荷只列**一个** `sid`）
  - 实测：`platform_iam_session` 有 `family_id` / `revoked_at` / `last_used_at` 三列，277 行
- **建议改法**:
  > - 明确 `sid` = **触发登出的这次请求的会话 ID**（即当前 access token 的 `sid`），
  >   并在 AC 里写死「`logout_token` 的 `sid` **逐字等于** 触发登出的登录令牌的 `sid`」；
  > - 若 BA 认为要按会话逐个发，则 AC 必须写成「**每个**会话各发一个 `logout_token`，
  >   `sid` 集合 == 该用户的全部会话 id 集合」→ 并把 mock 的接收条数也钉死。


### S-25: 「全局登出」只作废 refresh token —— **已签发的 access token 仍然有效到过期**（安全缺口）

- **档位**: 安全缺口 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-149 预期结果第 4 条（:406-407）、AC-S-150（:426）；
  `decisions.md` D211（:353-355）
- **AC 原文**: 「步骤 4、5：**登出本身仍然成功**（HTTP 200，授权记录已撤销）」
- **问题**: D211 说全局登出要「作废**全部会话**」，实现上引用的是
  `revoke_family` / `revoke_all_refresh_tokens` —— 两者**只** UPDATE `platform_iam_refresh_token`。
  而 access token 是**自包含的 JWT**（`jwt.rs:30-61` 的 `AccessClaims`，`platform-iam/src/jwt.rs:143-166` 验签不查库）
  → **登出后，旧的登录 access token 在 `exp` 之前仍然能调所有业务端点**。
  → AC-S-149 / AC-S-150 **都没有断言**「登出后旧 access token 立即失效」
  → **这是一个既没有被实现、也没有被验收的安全缺口**（用户点「退出登录」后，别人拿他的 token 还能用一小时）。
- **证据**:
  - `code/crates/platform-iam/src/store.rs:754-762`（`revoke_family` —— 只动 refresh token 表）
  - `code/crates/platform-iam/src/store.rs:764-778`（`revoke_all_refresh_tokens` —— 同上）
  - `code/crates/platform-iam/src/jwt.rs:143-166`（`verify` 只验签 + `exp`，**不查库**）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:355`（D211 §5 的「作废全部会话」）
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:426`（AC-S-150 步骤 2 只断言 grant 被撤销）
- **建议改法**:
  > BA 二选一并写进 AC：
  > **选项 A（推荐，补实现）**：全局登出额外作废**登录会话**（`platform_iam_session.revoked_at`）
  >   + 平台在验签后**查会话状态**（或引入 `sid` 黑名单）→ AC 补一条：
  >   「登出后，用**登出前**的登录 access token 调 `GET /api/rbac/me/permissions` → **401**」。
  > **选项 B（承认现状）**：在 AC-S-149 / AC-S-150 里**显式写明**
  >   「登出**只**撤销 OAuth 授权与 refresh token；**已签发的登录 access token 最长延迟一个
  >   `iam.accessTokenTtl` 秒失效**」→ 并把这条写进 D211 + 元数据（第三方据此知道窗口）。
  >   ⚠️ 选 B 等于接受「登出后 token 仍可用」——**必须由 BA 明写，不能默认**。


### S-27: AC-S-135 步骤 5「用该 client 走一次授权码交换 → 401 + `invalid_client`」—— **拒绝点与错误码没钉死**

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-135 步骤 5、预期结果第 3 条（:108、:114）
- **AC 原文**: 「步骤 5 返回 **401** + `invalid_client`（`client` 已 disabled → D208 的认证失败口径）」
- **问题**: 两处对不上：
  1. **拒绝点**：disabled 的 client 在 **preview**（参数校验）阶段就该被拒
     （design §6.1 给 `OAUTH_CLIENT_DISABLED` 定的状态码是 **403**）→
     **根本走不到令牌交换**。AC 说「走一次授权码交换」，但按 design 那一步到不了；
  2. **码与状态码**：AC 说 **401 + `invalid_client`**（协议码）；
     design 说 `OAUTH_CLIENT_DISABLED` = **403**（平台码）→
     **两套错误标识在同一个端点上混用**，且 AC 没说明走的是哪一步。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:327-336`（§6.1：`OAUTH_CLIENT_DISABLED` → 403）
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:114`
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:46-49`（§全局约定的两套错误标识表）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:272`（D207 §4：认证失败统一 `invalid_client` + 401）
- **建议改法**:
  > 把步骤 5 拆成**两条**，并各自钉死端点与错误体：
  > 1. **preview 阶段**：`POST /api/oauth/authorize/preview`（用已 disabled 的 client）→
  >    **403** + Problem Details，`code == "OAUTH_CLIENT_DISABLED"`（**平台话**，因为 preview 是平台端点）；
  > 2. **token 阶段**：`POST /api/oauth/token`（client 认证用 disabled 的 client）→
  >    **401** + `{"error":"invalid_client"}`（**协议话**，因为 token 是协议端点）。
  > → **两套错误标识在同一端点族里的分工就此钉死**，与 C14 §14.2 一致。


### S-28: `OAUTH_CLIENT_SECRET_INVALID` 是**死码** —— 22 条 AC 里**一次都没出现**，且没有触发点

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` :58（10 个平台码清单）、:56（「全部要按 D213 的 8 处登记」）；
  `design-summary.md` §6.1（:323-336）
- **问题**: 这个码被写进「10 个码」的清单，并要求「按 8 处登记」
  （→ 进 `TITLES`、进两个 locale 的 `error.*`、进 `ALL_CODES`…），
  但：
  - **22 条 AC 里没有任何一条断言它**（逐条核对：0 次出现）；
  - **它没有触发点**：design §6.1 说它用于「管理端点**改 secret** 时旧 secret 错」，
    但 D200 决定 secret **不允许自选**、**只显示一次**，而 design §3.2 的端点清单里
    **没有「轮换 secret」端点**（只有 GET/POST/PATCH/DELETE clients）→
    → **这个码永远不可能出现在响应体里**；
  - → 一个「永远不出现」的码却要在 i18n 词库里占一条、在 `TITLES` 里占一行 → **死登记**。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:56-61`（10 个码清单）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:323-336`（§6.1 的「何时」列）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:147-158`（§3.2 端点清单：**没有**轮换 secret）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:135-147`（D200：不允许自选、只显示一次）
- **建议改法**:
  > 二选一：
  > **选项 A（推荐）**：**删掉** `OAUTH_CLIENT_SECRET_INVALID` → 平台码变成 **9 个**，
  >   `TITLES` 行数 = 84 + 9 (+ 事件码) → AC-S-152 的数字同步改。
  > **选项 B**：**补一个「轮换 secret」端点**（`POST /api/oauth/clients/{id}/secret`），
  >   并在 AC-S-135 补一条断言：带**错误的旧 secret** 调用 → **401 + `OAUTH_CLIENT_SECRET_INVALID`**。
  > → 无论选哪个，**AC-S-152 的「10 个码」与 `TITLES` 行数必须一致**（现在是 10 但只有 9 个有触发点）。


### S-37: D201 的「三重交集」公式在 `client_credentials` grant 下**没有定义**

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-141（:223-238）；`decisions.md` D201 §2（:154-157）
- **AC 原文（AC-S-141）**: 「已注册 `confidential` client，`allowedGrantTypes` 含 `client_credentials`，
  `allowedScopes` 含 `sandbox:demo:read`。」/ 「步骤 2：**200**，含 `access_token` / `token_type` /
  `expires_in` / `scope`」
- **AC 原文（D201）**: 「实际授予的 scope = **请求 scope ∩ `client.allowed_scopes` ∩ 用户自身的有效权限点**」
- **问题**: `client_credentials` grant **没有用户** → 「用户自身的有效权限点」这一维
  **不存在** → 三重交集退化成两重。D201 没写「无用户时这一维怎么处理」，
  AC-S-141 也没写 → 实现者可以任选（跳过 / 用 client 自身的权限 / 报错），
  **AC 判不出**。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/decisions.md:154-157`（D201 §2）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:91`（D198 §4：`client_credentials` 时**省略** `sub`）
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:223-238`
- **建议改法**:
  > - D201 补一句：「`client_credentials` grant **没有用户** → 第三维**不适用**，
  >   授予的 scope = **请求 scope ∩ `client.allowed_scopes`**（两重）；
  >   交集为空 → `invalid_scope`（400）。」
  > - AC-S-141 补一条判据：请求 `scope=sandbox:demo:write`（**不在** `allowedScopes` 里）→
  >   **400 + `{"error":"invalid_scope"}`**。

### S-39: AC-S-148 的 `consent_required` —— **端点与错误体形状都没钉**，与 C14 §14.4 冲突

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: `acceptance.md` AC-S-148 步骤 4、预期结果第 2 条（:377、:381）
- **AC 原文**: 「4. 再访问一次，**但**把 `scope` 改成**更大**的集合（超出已有 grant）；」/
  「步骤 4：返回 `consent_required`（**不**签发 code）→ 前端显示同意页；」
- **问题**: 「返回 `consent_required`」没有说：
  1. **哪个端点**返回它 —— preview（后端）还是前端路由？
  2. **什么形状** —— 按 D212 §3 的 OIDC 语义，它是**协议码**（小写）；
     但 `preview` 按契约 C14 §14.4 是**平台自有端点** → 错误体应是 **Problem Details**；
     → 到底是 `{"error":"consent_required"}`（协议形状）还是
     `{"code":"OAUTH_…","detail":"…"}`（平台形状）？**契约没有覆盖这个组合**，
     两种都「有依据」。
  3. AC 同时说「**不**签发 code」→ 那「返回 `consent_required`」到底指什么？
     若 preview 返 200 + `needsConsent=true`，就没有任何地方「返回 `consent_required`」了。
- **证据**:
  - `BA/contracts/api-conventions/v3.md:127-135`（C14 §14.4：授权请求端点是**平台自有端点**，说平台的话）
  - `BA/contracts/api-conventions/v3.md:108-111`（协议码集合里有 `consent_required`）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:366-370`（D212 §3 的两条 OIDC 语义）
  - `BA/demands/REQ-022-outbound-idp/decisions.md:201`（D203：`preview` 返回 `{ticket, application, scopes, needsConsent}`）
- **建议改法**:
  > - 明确 `prompt=none` 且 grant 不覆盖时，**preview 返回 200 + `needsConsent == true`**
  >   （**不是**错误）→ 前端渲染同意页；
  > - `login_required` / `consent_required` 两个**协议码**只出现在
  >   **授权页的 query**（前端把状态透出到 URL）里，**不是** preview 的错误体；
  > - preview 的**错误**（参数非法、client 不可用）才用 Problem Details + `OAUTH_*`；
  > - AC 的步骤 4 断言改成「页面显示同意页 + URL query 里 `error=consent_required`
  >   + `platform_oauth_code` 行数 **== 0**」。


## 中（10）

### S-26: AC-S-150 步骤 2「两个 client 的授权**都**被撤销」—— 是步骤 1（全局登出）的**必然后果**，不是独立判据

- **档位**: 口径不一致 ｜**级别**: 中
- **位置**: `acceptance.md` AC-S-150 步骤 2、预期结果第 2 条（:419、:426）
- **问题**: 步骤 1 是**全局**登出 → 按 D211 全局登出撤销该用户**全部** grant →
  步骤 2 的「client-A 与 client-B 的授权都被撤销」是**步骤 1 的定义**，**不是独立判据**。
  → 这条断言**没有区分力**（它不可能不成立）。
- **证据**: `BA/demands/REQ-022-outbound-idp/decisions.md:353-355`（D211 §5）
- **建议改法**:
  > 把步骤 2 的判据改成**有区分力的**版本：
  > 「步骤 2：**`client-B`（未配 front-channel URI）的 grant 也**被撤销
  > → 这证明 front-channel 配置**只影响通知方式**，**不影响撤销范围**；
  > 且 `client-B` **不在** `frontChannel` 数组里 → 证明收集逻辑按 `frontchannel_logout_uri` **非空**过滤。」


### S-29: AC-S-134 的「**恰好包含**」—— 自造词，与同一条 AC 里其它字段的「**恰好**」不一致

- **档位**: 表述 ｜**级别**: 中
- **位置**: `acceptance.md` AC-S-134 预期结果（:86-93）
- **AC 原文**: 「`grant_types_supported` **恰好包含** `authorization_code` / `client_credentials` /
  `urn:ietf:params:oauth:grant-type:device_code`（**不含** `refresh_token`，D217）」
- **问题**: 同一条 AC 里，`response_types_supported` 用「**恰好** `["code"]`」、
  `code_challenge_methods_supported` 用「**恰好** `["S256"]`」、
  `introspection_endpoint_auth_methods_supported` 用「**恰好** `[...]`」——
  唯独 `grant_types_supported` 用「**恰好包含**」→ 是「恰好等于这三个」还是「至少包含这三个」？
  按字面「恰好包含」= **恰好等于**（因为后半句「不含 refresh_token」是同义反复）。
- **证据**: `BA/demands/REQ-022-outbound-idp/acceptance.md:86-93`
- **建议改法**:
  > 改成：「`grant_types_supported` **恰好**等于
  > `["authorization_code","client_credentials","urn:ietf:params:oauth:grant-type:device_code"]`
  > （**逐元素比对，顺序也钉死**；**不含** `refresh_token`，D217）」


### S-30: AC-S-138 路径 3 的 `error_description` 判据 —— **句义自相矛盾且不可机械判定**

- **档位**: 不可判定 ｜**级别**: 中
- **位置**: `acceptance.md` AC-S-138 预期结果第 2 条（:175-176）
- **AC 原文**: 「路径 3：交换返回 **400** + `{"error": "invalid_grant"}`，
  且 `error_description` **不含**「哪一步错了」的细节以外的信息。」
- **问题**: 「不含 X 以外的信息」= 只含 X？句义歧义；且「哪一步错了」**没有定义** → 无法机械判定。
- **证据**: `BA/demands/REQ-022-outbound-idp/acceptance.md:175-176`；
  `BA/contracts/api-conventions/v3.md:113`（协议错误不得在 `error_description` 里放中文）
- **建议改法**:
  > 改成两条**可判**的：
  > 1. `error_description` **不含** `code_verifier` / `code_challenge` / `client_secret` / `code` 的**明文**
  >    （逐个子串断言 `NOT contains`）；
  > 2. `error_description` **不含任何 CJK 字符**（契约 C14 §14.2）。


### S-31: AC-S-139 的「补充判据（TTL）」—— 没写「要插哪些列、什么值」

- **档位**: 不可判定 ｜**级别**: 中
- **位置**: `acceptance.md` AC-S-139 补充判据（:195-196）
- **AC 原文**: 「**补充判据（TTL）**: 造一个 `expires_at` 在 **90 秒前**的授权码 → 交换返回
  `{"error": "invalid_grant"}`；造一个 `expires_at` 在 **30 秒后**的 → 交换**成功**。」
- **问题**: 授权码的行按 design §2.3 有 **8 个绑定列**
  （`tenant_id` / `client_id` / `user_id` / `code_hash` / `redirect_uri` / `scope` /
  `code_challenge` / `code_challenge_method` / `expires_at`）→ **直插一行时每个值都得对**，
  否则交换会**先卡在绑定校验上**（`invalid_grant`）而不是卡在 TTL 上 → **判据失真**。
  AC 没写列清单 → 不可执行。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:89-99`（§2.3 的列集合）
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:213-216`（design §4.2 的处理顺序：③ 查码 → ④ 查 `expires_at` → ⑤ 校验绑定）
- **建议改法**:
  > 补充判据改写为（**逐列写死**）：
  > 「以 `base` 直连 `INSERT` 一行 `platform_oauth_code`：
  > `tenant_id='tenant-a'`、`client_id=<client-A>`、`user_id=<u>`、
  > `code_hash=<base64url(SHA256('ttl-expired-code'))>`、
  > `redirect_uri='https://client.example/cb'`、`scope='rbac:role:read'`、
  > `code_challenge=<S256(verifier)>`、`code_challenge_method='S256'`、
  > `expires_at = now() - interval '90 seconds'` →
  > 交换返回 **400 + `{"error":"invalid_grant"}`；
  > 同一批值、只把 `expires_at` 改成 `now() + interval '30 seconds'` → 交换**成功**（200）。
  > → 两条断言**只在 `expires_at` 上不同**，其它列逐字相同 → 才证明判的是 TTL。」


### S-32: AC-S-139 步骤 4「查授权码表的**行数**」—— 口径不清（整表还是该 code）

- **档位**: 表述 ｜**级别**: 中
- **位置**: `acceptance.md` AC-S-139 步骤 4、预期结果第 4 条（:188、:194）
- **AC 原文**: 「4. 查授权码表的行数。」/ 「步骤 4：授权码**已被删除**（行数 = 0 行，**不是**置一个 `consumed_at` 标记）。」
- **问题**: 「行数」是**整表**行数还是**该 code 对应的行数**？若是整表，
  其它用例 / 其它租户的行会污染判据（REQ-014 的「干净起点」教训）。
  另外 design §2.3 的列清单里**没有** `consumed_at` 列 → 「不是置一个 `consumed_at` 标记」
  这个否定断言**没有对象**。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:188`、`:194`
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:89-99`（§2.3 的列集合：**无** `consumed_at`）
  - `BA/plan/dev-guide-low-spec.md:619-623`（「E2E 必须在干净起点上跑」）
- **建议改法**:
  > 步骤 4 改为：「`SELECT count(*) FROM platform_oauth_code WHERE code_hash = <base64url(SHA256(code))>`
  > → **恰好 0**」；并补一条「该表的列集合里**不含** `consumed_at` / `used_at`」
  > （查 `information_schema.columns`）。


### S-33: 「grant 级撤销 → 该 grant 下**所有**令牌立即失效」—— 没有任何 AC 覆盖

- **档位**: 口径不一致 ｜**级别**: 中
- **位置**: `acceptance.md` AC-S-144 步骤 1~3、AC-S-147 步骤 4
- **AC 原文**: D210 §2：「撤销 grant → 该 grant 下**所有**未过期令牌立即 `active=false`」；
  AC-S-144 只验**一枚**令牌；AC-S-147 步骤 4 也只验「步骤 1 之前那枚令牌」
- **问题**: D210 的核心语义是「**grant 级**撤销」，但两条 AC 都只验**一枚**令牌 →
  实现成「只撤销那一枚令牌」也能过 → **grant 级语义没被验收**。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/decisions.md:323-327`（D210 §2）
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:288-302`、`:349-365`
- **建议改法**:
  > AC-S-147 补一条：**先签发 2 枚**令牌（同一 grant）→ 撤销 grant → **两枚**都 `active == false`
  > （并断言 `platform_oauth_token` 里这两行状态都是 `revoked`）。


### S-34: 「**另一个租户**的用户」—— 谁建没写（AC-S-147 步骤 5 / AC-S-155 前置）

- **档位**: 物理不可达 ｜**级别**: 中
- **位置**: `acceptance.md` AC-S-147 步骤 5（:358）、AC-S-155 前置（:512）
- **AC 原文**: 「5. 用**另一个租户**的用户调 `GET /api/oauth/grants`。」/
  「**前置条件**: `tenant-a` 与 `tenant-b` 各有用户与授权记录。」
- **问题**: `scripts/seed-e2e.sh` **只建 `tenant-a` 的 4 个账号**（`:146-151`）→
  `tenant-b` 的用户**不存在** → 步骤 5 与 AC-S-155 的前置**不可达**。
- **证据**:
  - `code/scripts/seed-e2e.sh:146-151`（只 INSERT `tenant_id='tenant-a'` 的 4 行）
  - `code/scripts/seed-e2e.sh:347-349`（自证查询也只查 `tenant-a`）
- **建议改法**:
  > 在 `scripts/seed-e2e.sh` 增加 `tenant-b` 的夹具账号 `u-perm-other@example.com`
  > （持 `rbac:role:read`），并在脚本的「强断言」里核对它**存在且绑定正确**；
  > AC-S-147 步骤 5 与 AC-S-155 的前置写明「`tenant-b` 的用户由 `scripts/seed-e2e.sh` 播种」。


### S-35: AC-S-154 的「敏感值扫描」—— **宿主与阳性对照注入点**未指定

- **档位**: 不可判定 ｜**级别**: 中
- **位置**: `acceptance.md` AC-S-154 预期结果第 3 条（:502-503）
- **AC 原文**: 「**不含**任何 secret / token / code 的**明文**
  （对全表做敏感值扫描，**带阳性对照**）；」
- **问题**: 与 S-17 同源：「全表」指哪张表？扫描器在哪（Rust 侧 / SQL 侧 / 脚本侧）？
  阳性对照怎么注入（往审计表插一条含假 secret 的记录 → 扫描器必须命中）？
  → 「带阳性对照」这四个字**没有可执行的形态**。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/acceptance.md:502-503`
  - `BA/feature-meta/qa-req014-spec-review.md` S-19（REQ-014 同款：「敏感键扫描器**没有可实现的宿主**」）
  - `code/crates/platform-audit/src/audit.rs`（REQ-014 的敏感键硬拒绝在 **Rust 侧** → 本轮可沿用）
- **建议改法**:
  > - 扫描**宿主**：Rust 侧一个可单独调用的 `pub fn`（放在 `crates/platform-oauth/src/` 或
  >   `crates/platform-server/src/`），输入 = 审计行的 `action` / `object_type` / `object_id` /
  >   `before` / `after` 文本，输出 = 命中的敏感键列表；
  > - **扫描面**：审计表的**全部文本列**（不只 `before`/`after`）；
  > - **阳性对照**：用管理员直连往审计表插一条**含假 secret** 的行
  >   （如 `before = '{"client_secret":"POSITIVE-CONTROL"}'`）→ 扫描器**必须命中**该行；
  >   删掉该行后 → 扫描器**必须不命中**。
  > - **敏感键集合**要**列全**（至少：`client_secret` / `code` / `code_verifier` /
  >   `device_code` / `user_code` / `access_token` / `refresh_token` / `logout_token` /
  >   `authorization` / `password`）。


### S-36: design §8 与 §13 的**迁移文件名自相矛盾**（权限点种子是 `…0500_` 还是 `…0600_`）

- **档位**: 口径不一致 ｜**级别**: 中
- **位置**: `design-summary.md` §8（:399-401）与 §13（:494-496）；`decisions.md` D214（:410-413）
- **AC 原文（design §8）**: 「**种子迁移**：`migrations/20261120000500_oauth_permission_seed.up.sql`」
- **AC 原文（design §13）**: 「`migrations/20261120000500_oauth_schema.up/down.sql`」+
  「`migrations/20261120000600_oauth_permission_seed.up/down.sql`」
- **问题**: 同一个迁移文件在两处有**两个编号**（`…0500_` 与 `…0600_`），
  而 `…0500_` 在 §13 里已经是 **schema** 迁移 → Dev 会建错文件名 / 建两个文件。
  另：§7 说配置种子的范式是 `migrations/20261120000400_audit_config_seed.up.sql`，
  §13 却写 `…0700_oauth_config_seed` —— 编号也对不上（不影响正确性，但要一次定死）。
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/design-summary.md:399-401`、`:494-496`
  - `BA/demands/REQ-022-outbound-idp/decisions.md:410-413`
  - `code/migrations/`（现有最大编号 = `20261120000400_audit_config_seed.up.sql` → 新迁移应从
    `20261120000500` 起，**但 req-013/014 的 `2026112000` 段已被占用 4 个号**）
- **建议改法**:
  > 统一为：`20261120000500_oauth_schema` / `20261120000600_oauth_permission_seed` /
  > `20261120000700_oauth_config_seed`（**只保留 design §13 的一套**），
  > 并在 §8 里改成同一组文件名（**三处一致**）。

### S-38: AC-S-135 的「**201**（或 **200**）」—— 两个取值都算过 → 不可判定

- **档位**: 口径不一致 ｜**级别**: 中
- **位置**: `acceptance.md` AC-S-135 预期结果第 1 条（:111）
- **AC 原文**: 「步骤 2 返回 **201**（或 200），含 `clientId`（**非空、唯一**）与
  **`clientSecret`（仅此一次**，D200）；响应**不含** `clientSecretHash`；」
- **问题**: 「201（或 200）」= 两个取值都算通过 → **判不出红绿**。
  而平台的既有惯例是**明确的**：创建类端点一律 **201 CREATED**
  （`platform-iam/src/http.rs:586`、`platform-rbac/src/http.rs:590`、
  `platform-org/src/http.rs:406` 与 `:624`）。
  → BA 应把 AC 改成单一值 **201**，否则实现者自选 200 也能过验收。
- **证据**:
  - `code/crates/platform-iam/src/http.rs:586`（`Ok(user) => (StatusCode::CREATED, Json(user)).into_response()`）
  - `code/crates/platform-rbac/src/http.rs:590`（`Ok(created) => (StatusCode::CREATED, …)`）
  - `code/crates/platform-org/src/http.rs:406`、`:624`
- **建议改法**:
  > 步骤 2 返回 **201**（**不是 200** —— 平台创建类端点的统一口径是 `CREATED`）。



---

## 6. 我**实际验证过**的检查点（9 组必查 + 若干附加）

> 这一节是「不采信 BA 自查」的证据。**所有结论都可复跑。**

### 6.1 授权页的 query 参数**能不能活过登录**（必查 #2 的第 1 问）—— **不能**

| 检查 | 结果 |
|---|---|
| SPA 路由表里有没有 `/oauth/authorize`？ | ❌ **没有**（`code/apps/shell/src/lib/routes.ts:68-111` 只有 `/`、`/login`、`/sso/callback`、`/403`、`/schema/:moduleId/:entityKey`、`/admin/roles`、`/admin/audit`）→ 这是 Dev 的活，不是缺陷 |
| SPA 能不能拿到 URL 的 query？ | ✅ **能**（`router.ts:232-235` 在 path 不变时保留 `search`；页面自己读 `window.location.search`，`sso-api.ts:244-256` 有现成范式） |
| 静态文件回落：`/oauth/authorize` 会不会被回落到 `index.html`？ | ✅ **会**（且这是**对的**）：`static_files.rs:36-38` 的 `is_api_path` 只认 `/api` 与 `/api/**` → **`/oauth/...` 不是 API 路径** → `:64-70`「路径无扩展名 → 回落 index.html」→ SPA 能接到 |
| D180「未注册的 `/api/...` 必须 404」会不会误伤 `/oauth/...`？ | ✅ **不会**（`static_files.rs:36-38` + `:202-214` 的既有用例明确断言 `!is_api_path("/admin/audit")`）→ **D180 不适用于 `/oauth/*`** |
| 未登录时 query 会不会被丢掉？ | ❌ **会丢** → **S-02**（`router.ts:229-242` + `:261` + `App.svelte:120-125` + `packages/auth/src/index.ts:118-132`） |
| 全仓有没有 `return_to`？ | ❌ **没有**（`grep -rn "return_to\|returnTo" apps/shell/src packages/auth/src packages/sdk/src` 只命中 `router.ts` 的 `intendedPath` 字段声明） |

### 6.2 `scope` 三重交集的取数口径（必查 #3）—— **展开后永不含通配字面量**

```
$ grep -n "effective_permissions" -A 12 code/crates/platform-core/src/extension/permission.rs
  /// 当前用户的**有效**权限点集合（**已展开通配**，**升序且去重**）。

$ grep -n "fn expand" -A 10 code/crates/platform-rbac/src/entity.rs
  pub fn expand(&self, registry: &[String]) -> Vec<String> {
      let mut matched: Vec<String> = registry.iter().filter(|code| self.matches(code))...
```
→ `expand` 返回的是**注册表里的具体码** → 用户有效权限点集合里**永远没有** `rbac:role:*`
→ **S-14**（AC 的「二者必须能区分开」做不到）。

```
$ podman exec base-postgres psql -U base -d base_test -tAc "SELECT code FROM platform_permission WHERE code LIKE '%*%';"
（空）
$ podman exec base-postgres psql -U base -d base_test -tAc "SELECT code FROM platform_permission WHERE code LIKE 'rbac%';"
rbac:permission:read
rbac:role:read
rbac:role:write
rbac:user-role:write
```
→ AC-S-140 路径 4 的**前提成立**（`rbac:role:*` 确实不是注册过的权限点），
但**推不出** AC 想要的「区分两个原因」→ **S-14**。

### 6.3 OAuth 令牌打到业务端点会怎样（必查 #8）—— **必然 401，且是双重拒绝**

```
$ sed -n '143,166p' code/crates/platform-iam/src/jwt.rs
      validation.set_required_spec_claims(&["exp", "sub", "iss", "aud"]);
      validation.validate_exp = true;
      validation.validate_nbf = true;
      validation.validate_aud = true;
      validation.aud = Some(HashSet::from([audience.to_string()]));
      validation.iss = Some(HashSet::from([issuer.to_string()]));
```
| 检查 | 结果 |
|---|---|
| 验签会不会放过 `aud` 不匹配的令牌？ | ❌ **不会**（`validate_aud = true` 且 `aud = {平台 audience}`）→ OAuth 令牌 `aud = client_id` → **必然验签失败** → `IamError::Unauthenticated` → **401** |
| 验签会不会把 OAuth 令牌当成登录令牌**放行**？ | ❌ **不会**，且有**第二道**防线：`AccessClaims` 的 `sid` 是**必填**字段（`jwt.rs:30-61`），而 OAuth 令牌的 claims 清单（D198 §4）**没有 `sid`** → 反序列化也失败 |
| 业务端点的 401 是什么码？ | `UNAUTHENTICATED` + **401**（`platform-iam/src/error.rs:56`、`:359`）；`IamTokenVerifier`（`platform-server/src/audit.rs:106-117`）把任何错误都映射成 `TokenVerifyError::Invalid` → **不泄露原因** ✅ |
| 「`UNAUTHENTICATED` 或 `invalid_token`」哪个对？ | **只有 `UNAUTHENTICATED`** —— `invalid_token` **不在**契约 C14 §14.2 的协议码集合里 → **S-05** |
| 有没有端点绕过 `aud` 校验？ | ❌ **没有**：全仓 `grep "Validation::new\|decode_header"` 只有 `platform-iam/src/jwt.rs`（平台验签）与 `platform-iam/src/sso/oidc.rs:564`（**入站** IdP 的验签，与本需求无关） |

### 6.4 `TITLES` / i18n / 权限点的真实数字（必查 #9）—— **84 / 84 / 26**

```
$ awk '/pub const TITLES/,/^\];/' code/crates/platform-core/src/titles.rs | grep -cE '^\s*\('
84
$ grep -c '"error\.' code/packages/i18n/src/locales/en-US.json   → 84
$ grep -c '"error\.' code/packages/i18n/src/locales/zh-CN.json   → 84
$ podman exec base-postgres psql -U base -d base_test -tAc "SELECT count(*) FROM platform_permission;"   → 26
```
| 检查 | 结果 |
|---|---|
| AC-S-152 的「基线 58 + 21 + 5」对不对？ | ✅ **对**（= 84）；但「**本需求新增数**」没给死 → **S-03**（应为 10 + 8 = 18 → **102**） |
| D215 的 8 个 `oauth.*` 事件码进 `TITLES` 会怎样？ | ❌ **让 3 个既有测试变红**（命名空间 / 错误码↔locale / 孤儿标题）→ **S-04** |
| `platform_oauth::error::ALL_CODES` 加进 `all_codes()` 后，事件码算不算「错误码」？ | ❌ **算**（`error_codes()` 只排除来源标签 `platform-org::notify`）→ 与 D215「不进 i18n `error.*`」冲突 → **S-04** |
| 现有 `platform_permission` 的 `source` 长什么样？ | `crates/<crate>/manifest.json` 或 `modules/<module>/manifest.json`（实测 5 个来源）→ D214 要求 `source = 'crates/platform-oauth/manifest.json'` ✅ 与 `registry.rs:140` 的拼法一致 |

### 6.5 C-5 的扫描范围与「14 个文件」（必查 #10）—— **范围对，命令错**

| 检查 | 结果 |
|---|---|
| C-5 扫的范围 | ✅ **就是「该 manifest 所属 crate 自己的 `src/`」**（`platform-contracts/src/checks.rs:505-560` 的 `check_c5_for`），且 `platform-oauth/src/http.rs` 在范围内 ✅（design §3.2 把 HTTP 放在 `platform-oauth` 里，**不是** REQ-014 那种「manifest 在 A、HTTP 在 B」的错位） |
| 8 个别名（含 `authorize`）会被判违例吗？ | ✅ **会**（`authz_scan.rs:43-52` + `:131-133`：自由出现即记 `custom_entries`）→ D216 的命名禁区**有效** ✅ |
| 平台 crate 的 C-5 用什么命令跑？ | `cargo test -p <crate> --test contract_c5`（范式见 `crates/platform-audit/tests/contract_c5.rs:24-64`、`crates/platform-config/tests/contract_c5.rs`）→ **该测试文件对 `platform-oauth` 尚不存在**，必须由本需求新增 → **S-13** |
| `cargo xtask capabilities --check` 会跑 C-5 吗？ | ❌ **不会**（`xtask/src/capabilities.rs` 只比对索引产物）→ **S-13** |
| `capabilities/` 现在的文件数 | **14**（`find capabilities -type f | wc -l`）→ 加一个 crate **不新增文件**，变的是 `permissions.json` 的 `total`（26 → 32）→ **S-13** |
| 这个命令现在绿吗？ | ❌ **红**（实测退出码 1，`E_CAPABILITIES_OUTDATED`）→ **S-13** |

```
$ ./target/debug/xtask capabilities --check --root /home/myg133/Projects/system_seed/code
能力索引校验失败：错误码 E_CAPABILITIES_OUTDATED
  ✗ 内容不一致：capabilities/index.json
  修复方式：运行 `cargo xtask capabilities` 重新生成
EXIT=1
$ grep -n "audit_sink" -A 2 code/crates/xtask/src/catalog.rs
    key: "audit_sink",
    summary: "审计只到事件声明面，落库与查询待 REQ-014",      ← REQ-014 已合并，这句是旧的
```

### 6.6 RLS：4 张新表的 policy 谁来生成（必查 #11）—— **两种先例都存在，REQ-022 一条都没选**

| 检查 | 结果 |
|---|---|
| 平台现状：多少张表启用 RLS？ | **31 张**（`SELECT tablename FROM pg_tables WHERE rowsecurity`） |
| 它们的 policy 命名 | 全部是 **`<table>_tenant_isolation`**（与 `rls.rs:36` 的 `POLICY_SUFFIX` 一致） |
| 平台的「唯一来源」说法 | ✅ `platform-db/src/scopes.rs:5`：「RLS 策略的**唯一来源**是业务模块 manifest 的 `tenantScoped` 段」；`:91-93` 没有该段就**跳过** |
| sandbox 走哪条路？ | **生成**（`modules/sandbox/manifest.json:21-29` 有 `tenantScoped`；`modules/sandbox/migrations/*.sql` 里 **零条** `POLICY` 语句） |
| audit 走哪条路？ | **手写**（`crates/platform-audit/manifest.json` **没有** `tenantScoped`；`migrations/20261120000200_audit.up.sql:146-182` 手写 policy + GRANT） |
| REQ-022 的 manifest 有 `tenantScoped` 吗？ | ❌ **没有**（design §8 的模板里没有）→ 4 张表**不在** `xtask tenant-policies --check` 的覆盖范围 → **S-22** |
| `platform_oauth_client` 无 `tenant_id` | ✅ 与 D219 一致（平台级表不启用 RLS）；`scopes.rs` 天然跳过它 |
| 新表能不能拿到应用角色的 SELECT 权限？ | ✅ **能**（`deploy/seed/02-app-role.sql:99-100` 的 `ALTER DEFAULT PRIVILEGES … GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO base_platform_app` 对**之后创建的表**生效；sandbox 就是靠这个拿到权限的，实测 `has_table_privilege('base_app','sandbox_demo_item','SELECT') = t`） |

### 6.7 AC-S-155 的 RLS 判据形状（**实测复现，成立**）—— 但只覆盖读侧

```
$ podman exec base-postgres psql -U base_app -d base_test -tAc "SELECT count(*) FROM platform_audit_log;"
0                                   ← 不 set_config → 0 行 ✅（AC 的判据成立）
$ podman exec base-postgres psql -U base_app -d base_test -tAc \
    "BEGIN; SELECT set_config('app.tenant_id','tenant-a',true); SELECT count(*) FROM platform_audit_log; COMMIT;"
BEGIN
tenant-a
5                                   ← 单事务内 set_config → 有行 ✅
COMMIT
$ podman exec base-postgres psql -U base_app -d base_test -tAc \
    "BEGIN; SELECT set_config('app.tenant_id','tenant-zzz',true); SELECT count(*) FROM platform_audit_log; COMMIT;"
0                                   ← 别的租户 → 0 行 ✅
$ podman exec base-postgres psql -U base -d base_test -tAc \
    "SELECT has_table_privilege('base_app','platform_audit_log','SELECT'),
            (SELECT rolbypassrls FROM pg_roles WHERE rolname='base_app'),
            (SELECT rolsuper FROM pg_roles WHERE rolname='base_app');"
t|f|f                             ← 有 SELECT、不是超管、不带 BYPASSRLS ✅
```
→ **AC-S-155 的 RLS 判据形状是对的**（`base_app` 不 `set_config` → 0 行），
但**只覆盖读侧** → **S-23**（要补 `WITH CHECK` 的写侧判据）。

### 6.8 客户端库接不接得上（C14 的射程）—— **接不上，但有解**

| 检查 | 结果 |
|---|---|
| 现成的 OAuth 客户端库能接 `/api/oauth/token` 吗？ | ✅ **能**（协议端点 → RFC 错误体 → 库解析得了） |
| 能接 `/api/oauth/authorize` 吗？ | ❌ **不能** —— 它**不是** RFC 6749 §3.1 那个「返回 302 的授权端点」，而是前端路由 + 三个 POST 端点。契约 C14 §14.4 **已经显式声明**了这个例外（:127-135），所以**不算违反契约**；但对「现成 OAuth 库」来说，**授权环节必须手写**（库只会做「302 → 解析 code」） |
| 这会不会被 AC 覆盖？ | ❌ **没有任何 AC 断言「第三方按 metadata 拼出的授权 URL 可用」** → 这正是 S-11/S-12 要钉的 |

### 6.9 其它顺手确认的

| 检查 | 结果 |
|---|---|
| `platform_oauth` 相关代码现在存在吗？ | ❌ **不存在**（`grep -rn "api/oauth" crates/` → 0 命中；`grep -rn "frontendBaseUrl" .` → 0 命中）→ 纯新增，**零既有行为受影响** ✅ |
| 平台 POST 创建端点的状态码惯例 | **201 CREATED**（`platform-iam/src/http.rs:586`、`platform-rbac/src/http.rs:590`、`platform-org/src/http.rs:406`、`:624`）→ AC-S-135 的「**201（或 200）**」→ **不可判定**（见用例 TC-022-009 的备注） |
| `JWT 私钥` 在 E2E 里能拿到吗？ | ❌ **拿不到**（`scripts/e2e-server.sh:65-73` 现场生成 + `export`，**不落盘**；Playwright 是另一个进程）→ **S-08** |
| `digest_base64url` 与 `digest_hex` 两个口径 | **两个都在**（`platform-iam/src/crypto.rs:56-60` / `:62-68`）→ **S-16** |
| `platform_iam_session` 是多会话模型吗？ | ✅ **是**（列：`family_id` / `revoked_at` / `last_used_at`；实测 277 行）→ **S-24** |
| workspace 成员数 | **13**（含 `platform-contracts-macros`，无测试）→ 本需求 +1 = **14**；`xtask capabilities` 的「14 个文件」是**另一回事**（见 S-13） |
| `scripts/` 里有 mock HTTP server 吗？ | ❌ **没有**（`e2e-server.sh` / `seed-e2e.sh` / `test-integration.sh` / `db-up.sh` / `seed.sh` / `prod-preflight.sh` …）→ **S-07** |

---

## 7. 需要 BA 拍板、**用例解决不了**的问题（按优先级，**14 项**）

| # | 待拍板项 | 阻塞在 | 影响用例 | 我的建议（可直接落笔） |
|---|---|---|---|---|
| 1 | **未登录回跳怎么保住 query**（`/oauth/authorize` 走公开路由 + 登录页带 `return_to`，还是改 `router.ts` 的 `write()`） | S-02 | 授权码全组（≈ 28 条） | 选**公开路由 + `return_to`**（不动 REQ-011 的既有行为），并把 `Login.svelte` 补进 design §13 的文件清单 |
| 2 | **授权 URL 的基址符号**（`{issuer}` 还是 `{frontendBaseUrl}`；E2E 里二者不等） | S-11、S-12 | 全部授权相关 | 用 `{授权页基址}`，并规定「未配置 `oauth.frontendBaseUrl` 时回退到 `{issuer}`」 |
| 3 | **D215 的 8 个事件码进不进 `TITLES`** | S-04、S-03 | 登记组（≈ 16 条） | **不进**（删掉 D215 的 TITLES 那一段）→ `TITLES` = 94；或**进**并同步改 `i18n_titles.rs` 三处 → 102。**二选一，数字写死** |
| 4 | **AC-S-151 的 401 口径**（删掉「二者都接受」，钉死 `UNAUTHENTICATED`） | S-05、S-10 | 默认拒绝组 | 钉 `UNAUTHENTICATED`；步骤 3 改成「同端点再打一次」 |
| 5 | **front-channel 到底是 HTML 还是 JSON** | S-20 | 登出组 | 选 **HTML + iframe**（design §4.5 的写法），并把 `sandbox` / `X-Frame-Options` 补进 AC |
| 6 | **`OAUTH_CLIENT_SECRET_INVALID` 删还是补触发点** | S-28 | 登记组 | **删**（→ 9 个平台码）；若要留则补「轮换 secret」端点 |
| 7 | **4 张表的 RLS policy 谁生成** | S-22 | 租户隔离组 | **加 manifest `tenantScoped` 段** → 由 `xtask tenant-policies` 生成 → 顺带进平台自检 |
| 8 | **摘要的编码口径**（base64url vs hex） | S-16 | 摘要相关 | 全用 **`digest_base64url`**（base64url 无填充），在 §全局约定里写一行 |
| 9 | **全局登出要不要作废已签发的登录 access token** | S-25 | 登出组 | 至少在 AC 里**显式写明现状**（选项 B）；要「立即失效」就得另起实现（选项 A） |
| 10 | **`sid` 在多会话下的取值** | S-24 | 登出组 | 取**触发登出的这次请求的会话 ID**，并在 AC 里写死 |
| 11 | **client_credentials 的 scope 是几重交集** | S-37 | client_credentials 组 | **两重**（请求 ∩ client.allowed_scopes），无用户维 |
| 12 | **迁移文件名**（`…0500_` vs `…0600_`） | S-36 | 迁移 / 对账 | 统一成 §13 的一套（schema 0500 / seed 0600 / config 0700） |
| 13 | **`consent_required` 的端点与错误体形状** | S-39 | 免登组（≈ 3 条） | `preview` 返回 **200 + `needsConsent=true`**；两个 OIDC 协议码只出现在**授权页的 query** 里 |
| 14 | **`POST /api/oauth/clients` 返回 201 还是 200** | S-38 | client 管理组 | **201**（平台创建类端点的统一口径：`platform-iam/http.rs:586`、`platform-rbac/http.rs:590`、`platform-org/http.rs:406`） |

---

## 8. 本轮**没有**做的事（避免「看起来查过了」）

| 项 | 说明 |
|---|---|
| **没有编译 / 跑任何测试** | 需求尚未实现，无从编译。**未跑** `cargo build` / `cargo test` / `cargo xtask capabilities`（只跑了**已构建好的** `target/debug/xtask` 二进制做只读 `--check`） |
| **没有改 `code/` 下任何文件** | 只读；`git status` 全程为空 |
| **没有执行任何用例** | 用例是**给 Dev 的输入**，不是验收结论 |
| **没有建临时表 / 写任何行** | §6 的数据库实测**全是 `SELECT`**（`pg_tables` / `pg_policies` / `information_schema` / `platform_permission` / `platform_audit_log` 计数 / `has_table_privilege`） |
| **没有起 mock 接收端 / 没有跑 E2E** | 本轮只核实「怎么起」是否可行（S-07） |
| **没有改 `status.md` / 契约 / plan** | 那是 BA 的；契约由 BA 改 |
| **没有 commit** | 按派单要求，BA 统一提交 |
| **没有审 D196~D220 的「值不值得」** | 只审**可判定性**；决策层取舍只在与验收判据冲突时提（S-24、S-25、S-37） |

---

## 9. 复核建议（给 BA 的下一步）

1. **先拍板 §7 的前 5 条** —— 第 1、2、3 条不拍板，Dev 一动手就会写出**自相矛盾**的实现
   （授权页 query 丢一半、元数据里的授权 URL 指向错误的主机、跨栈测试必红）。
2. **S-13 是「Dev 第一次跑就红」的坑**（`xtask capabilities --check` 基线已红，且它根本不做 C-5）——
   很容易被当成「环境问题」放过。修法已给（拆成 1a 刷索引 + 1b 真 C-5 命令）。
3. **S-04 要 BA 与「跨栈测试的 owner」一起定** —— 它不是 QA 单方面能判的：
   要么事件码不进 `TITLES`（改 D215），要么改 `i18n_titles.rs` 三处（改契约外的既有测试）。
4. 用例集已按「可机械判定」写好（**123 条**，22/22 AC 覆盖）；
   标注了 `〔S-xx〕` 的用例里，**大部分在 BA 拍板前判不出红绿**；
   但**不依赖任何待定口径**的那部分（字段完整性、摘要落库、权限拒绝、RLS 读侧、
   错误体形状、命名禁区）Dev 可以先写 —— 详见
   [test-cases/index.md](../demands/REQ-022-outbound-idp/test-cases/index.md) §1.2 与 §6。
