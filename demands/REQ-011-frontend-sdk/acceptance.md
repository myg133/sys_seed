# 验收标准：前端 SDK 骨架

- **需求编号**: REQ-011
- **父需求**: REQ-001
- **状态**: 草稿（2026-11-19 12:40 经 QA Pre-merge 反馈修订，**rev 3**）
- **设计摘要**: [design-summary.md](design-summary.md)
- **裁定记录**: [acceptance-rev2-ruling.md](acceptance-rev2-ruling.md)
- **用例集**: [test-cases/](test-cases/)（48 条）
- **更新时间**: 2026-11-19 12:40

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## 编号说明与修订记录

- 编号段：**`AC-S-064` ~ `AC-S-075`**（**12 条**，全局唯一段，规则见 D23）
- 原占位 `AC-S-016` / `AC-S-017` 已被本段取代（原占位来自拆分模板，无实际内容）
- **rev 2** 经 QA 用例评审挑出 **40 条**问题（高 **11** / 中 21 / 低 8），
  **全部接受并修订**。逐条裁定见 [acceptance-rev2-ruling.md](acceptance-rev2-ruling.md)，
  新增决策 **D92~D95**。
- **rev 3**（2026-11-19 12:40）：QA **Pre-merge 验证判 FAIL** ——
  11/12 条 AC 通过，但发现 **1 个 HIGH + 1 条 AC 未达成**：
  - **HIGH-1**：壳层**没有任何租户来源** → 登录 100% 失败。裁定：**登录页加租户输入框**（D98）
  - **AC-S-068 步骤 3 未达成**：会话只在内存，整页重载丢令牌 → 敲 URL 落登录页。
    裁定：**`sessionStorage` 持久化**（D99）
  - 同时修订 **用例集过期条目**：`TC-011-029`（原断言 `UNAUTHENTICATED`，与 D94 互斥）
    与 `TC-011-034`（原断言"键不同"，与 D92 互斥）—— **实现是对的，是用例集没跟上 rev 2**
- **AC-S-067 步骤 2** 另经 rev 3 修订为「剔除 Svelte 块锚点后精确等于空串」（D96）

**rev 1 → rev 2 的关键修订**（rev 1 是错在哪）：

| # | rev 1 的问题 | rev 2 怎么改 | 依据 |
|---|---|---|---|
| 1 | AC-S-070 与 AC-S-071 各自看都对，**合起来必错**：401 后重放沿用旧幂等键 → 后端返回 **409** 而非 200 | AC-S-070 步骤 1 补「重放**不带** `idempotency-key`」；AC-S-071 补同一约束 + 组合用例 TC-011-034 | D92 |
| 2 | AC-S-071 步骤 4 的幂等预期**只在 `/api/iam/*` 成立**，但前置写的是泛指 | 步骤 4 路由**钉死**到 `/api/iam/*`，并声明其他路由无幂等支持 | D93 |
| 3 | AC-S-070 步骤 2 照字面端到端会**作废整个会话 family** | 明确为**前端单测**（注入式 fake fetch），禁止对真后端并发 refresh | — |
| 4 | AC-S-069 要求 `code` 精确等于响应体，AC-S-070 要 `UNAUTHENTICATED` —— **两条互斥** | 选**透传后端码**；刷新失败抛后端实际码 | D94 |
| 5 | AC-S-073 要求「等于暗色期望值」却**没给期望值**，还给两个互斥实现选项 | 令牌契约表写死数值；`color-scheme` 必须切 `dark` | — |
| 6 | AC-S-073「无硬编码颜色」与令牌体系**互斥**，检查范围未定义 | `@base/ui` 零字面量；`@base/theme` 单一令牌定义文件例外；扫描参数写死 | D95 |
| 7 | `<Can>` 组件在现有构建链下**产不出产物**（`packages/*` 只有 `tsc`） | 主体改为：包提供判定，**`<Can>` 落在 `apps/shell`** | — |
| 8 | AC-S-068 **没有实现主体**（shell 无路由/登录页/403 页） | 拆两层：包级 `guard()` 判定 + 应用级最小壳（本期做） | — |
| 9 | AC-S-075 全是「不依赖」否定式断言 → **今天就假绿** | 改**双向**：禁止项不存在 **且** 规定项存在 | — |
| 10 | AC-S-074 步骤 3 与测试夹具矛盾（`#0055aa` 既禁又必需） | 扫描范围**排除** `test/` 与 `*.test.ts` | — |
| 11 | AC-S-066 步骤 5 无可执行机制（`require_permission` 是 axum Layer） | 改用**真实路由探针** + 双方共用金标准夹具 | — |

另有 **21 条中 / 8 条低** 全部接受（去重升序、跨租户 404、降级路径、合成码表、
键格式与唯一性、租户头注入范围、变量清单与映射、落点单一化、扫描参数等）。

---

## 承接的父 AC

父 AC：**AC-016**（SDK 按需引入且无全局副作用）、**AC-017**（权限组件与路由守卫联动）、
**AC-019**（API 类型自动同步）

> ⚠️ **AC-019 在本期收窄**：后端**没有** OpenAPI 产物（无 `utoipa` 依赖，
> 加它会突破依赖预算基线 D22），本期承担到
> **「SDK 的请求/响应类型与后端一致」** —— 手写类型 + 契约测试保证，
> 不引入代码生成器。
>
> ⚠️ **AC-016 在本期收窄**：本期不引入图表/富文本/树/穿梭等重组件，
> 「图表与富文本不进入产物」本期无从验证（它们不存在）。
> 本期承担到 **「五个包之间依赖单向，各包产物只含自己的代码」**（AC-S-075）。
>
> ⚠️ **AC-017 在本期首次真正落地** —— 前两棒只做了服务端。
> 本期做**应用级最小壳**（登录页 + 极简路由 + 受保护页 + 403 页），
> 使「按钮不渲染 / 直接敲 URL 被拦到 403」可端到端验证。

---

## 本需求自有 AC

### AC-S-064: 后端能返回当前用户的有效权限点集合
- **前置条件**:
  - 租户 T1 = `tenant-a` 存在；用户 U1 在 T1 下存在
  - 角色 R1 是**自定义**角色（非内置），**只**授予 `rbac:role:read`，
    **不含任何通配模式**（如 `rbac:role:*`）
  - U1 绑定 R1，且**未**绑定内置 `platform-admin`
    （内置管理员持有注册表全集 → `crates/platform-rbac/src/service.rs:857-863`）
  - **两次调用之间不发生授权变更**
- **测试步骤**:
  1. U1 登录，取 `access_token`
  2. `GET /api/rbac/me/permissions`，头 `Authorization: Bearer <token>`、`x-tenant-id: tenant-a`
  3. 断言 HTTP 状态码与响应体形状
  4. 断言集合内容
  5. 不带令牌请求同一路径
  6. 带 T1 的令牌，但请求头 `x-tenant-id: tenant-b`（T2 存在）请求同一路径
- **预期结果**:
  - 2 → **200**（**豁免**权限点要求 —— 与 `/api/rbac/permissions` 注册表同款；
    但**要求已认证**）
  - 3 → 响应体是**对象**（`!== null` 且 `!Array.isArray`）；
    `Object.keys(响应体).sort()` **深度等于** `['permissions']`（**恰好一个键**）；
    `Array.isArray(响应体.permissions) === true`；
    每一项 `typeof === 'string'`
  - 4 → `permissions` **包含** `rbac:role:read`、**不包含** `rbac:role:write`；
    **升序（字节序）且去重**
  - 5 → **401**，`code === 'UNAUTHENTICATED'`
  - 6 → **404**，`code === 'NOT_FOUND'`；
    响应体**不含** `permissions` 字段
    （请求头租户 ≠ 令牌租户 → 404，不泄露资源存在性 —— `platform-rbac/src/http.rs:186-189`）
- **类型**: 安全
- **附注（越界防护）**: 本接口**走豁免**，**不引入新权限点** →
  不需要 `require_permission` 调用点，**不触发** C-5 扫描与 D27 启动对账

### AC-S-065: 登录响应带上权限点
- **前置条件**: 同 AC-S-064；另加「无任何角色的用户 U2」
- **测试步骤**:
  1. U1 登录，断言响应体 `permissions`
  2. 断言 `permissions` 与 AC-S-064 步骤 4 的集合**相等**
  3. U2 登录，断言 `permissions`
  4. **降级路径**：Provider 未注入时登录，断言结果
- **预期结果**:
  - 1 → **200**；`permissions` **非空**，**包含** `rbac:role:read`、
    **不包含** `rbac:role:write`；**升序且去重**
  - 2 → 两次调用的权限点**集合相等**（去重升序后**深度相等**）——
    两个入口**不得**给出不同答案（都走 `effective_permissions`）
  - 3 → `permissions` 是**空数组** `[]`（**不是** `null`、**不是省略字段**）
  - 4 → 登录**成功**（200），`permissions === []`，
    `Object.prototype.hasOwnProperty.call(响应体, 'permissions') === true`
    （fail closed 的是「判定」，**不是**「登录」）
- **类型**: 功能

### AC-S-066: `can()` 在前端本地判定，且与后端一致
- **前置条件**: 前端已从 AC-S-064 拿到权限点集合 `["rbac:role:read"]`
- **测试步骤**:
  1. `can("rbac:role:read")`
  2. `can("rbac:role:write")`
  3. `can("order:invoice:delete")`（**未登记**在注册表里的权限点）
  4. 权限点集合为**空**时的 `can("rbac:role:read")`
  5. **一致性比对**：
     - 后端侧：用**真实路由探针** —— `GET /api/rbac/roles`（需 `rbac:role:read`）
       与 `POST /api/rbac/roles`（需 `rbac:role:write`），记录放行/拒绝
     - 前端侧：用**同一份金标准夹具**的 `permissions[]` 调 `can()`
     - 逐条比对两边结果
- **预期结果**:
  - 1 → `true`
  - 2 → `false`
  - 3 → `false`（**不抛错**、返回 `false`；不是 `true`）—— **P1**
  - 4 → `false`（fail closed：没有权限点集合 ≠ 有全部权限）
  - 5 → 前后端判定**逐条相同**
    （`require_permission` 是 axum Layer，**不能**被单测直接调用 ——
    只能「打一条受保护的路由」观察判定）
- **类型**: 安全

### AC-S-067: 无权限时内容不渲染（`@base/auth` 判定 + `apps/shell` 的 `<Can>` 封装）
- **前置条件**: 权限点集合 `["rbac:role:read"]`；
  `apps/shell` 有一个 `<Can>` 封装（**组件落在应用侧** —— 包内构建链为 `tsc` only，
  `.svelte` 文件进不了产物，见 rev 2 裁定 #7）
- **测试步骤**:
  1. 渲染 `<Can permission="rbac:role:read">可见</Can>`
  2. 渲染 `<Can permission="rbac:role:write">不可见</Can>`
  3. 渲染 `<Can permission="rbac:role:write" fallback="无权限">兜底</Can>`
  4. 断言**容器**的内容
- **预期结果**:
  - 1 → 容器渲染出 `可见`
  - 2 → **子内容不在 DOM 里**。判定方式（**rev 3 修订**）：
    - 剔除 Svelte 5 的块锚点标记（`<!--[-->` / `<!--]-->` / `<!--[!-->` / `<!---->`）
      后的容器内容**精确等于空串**；
    - 且**原始** `innerHTML` **不含**子内容文本；
    - 且**不含** `display:none` / `visibility:hidden`
    > ⚠️ **rev 2 的「`innerHTML` 精确等于空串」不可达** —— Svelte 5 的 `{#if}` 块
    > 在 DOM / SSR 输出里会留锚点注释（`<!--[-->`），让 `innerHTML` 永远不等于空串。
    > 真正的可判定不变量是「**子内容不在 DOM 里**」（不是被 CSS 藏起来）。
    > 见 **D96**。
  - 3 → 容器渲染出 `无权限`（**不含** children 的内容）
- **类型**: 功能

### AC-S-068: 路由守卫拦截无权访问
- **前置条件**: 用户 U1 无 `rbac:role:write` 权限；`apps/shell` 有登录页、
  极简路由、**一个**受 `rbac:role:write` 保护的页面、403 页
- **测试步骤**:
  1. **包级**：调 `@base/auth` 的 `guard("rbac:role:write")`，断言返回值
  2. **应用级**：U1 登录后，前端路由跳转到受保护路径
  3. **应用级**：**直接输入该路径 URL**（绕过前端路由跳转，**整页重载**）
  4. 断言最终落点与网络请求
  5. **未登录**访问受保护路径
- **预期结果**:
  - 1 → `{ allowed: false, redirectTo: <403 落点> }`（**框架无关的纯判定**）
  - 2 → 落在 **403 页**
  - 3 → **同样**落在 403 页（守卫不能只在「程序化跳转」时生效 ——
    直接敲 URL 是最常见的绕过方式）。
    **前置**：会话必须**跨整页重载存活** —— 见下「租户与会话的来源」
  - 4 → 落在 403 页，**不是**目标页面；
    目标接口**未被请求**（断言该接口的请求计数为 **0**）
  - 5 → 落在**登录页**，页面**不出现** `403` 字样
    （未认证与无权限混同 → 用户会看到 403 而不是登录入口）
- **类型**: 安全

#### AC-S-068 补充：租户与会话的来源（rev 3，D98 / D99）

QA 在 Pre-merge 验证中发现壳层**没有任何租户来源**，
导致登录 100% 失败（后端要求 `x-tenant-id`，缺失 → **401 `TENANT_CONTEXT_MISSING`**）：

```
App.svelte   createAppApi()          // 不传 tenantId
  → api.ts   tenantId: undefined
  → SDK      按 AC-S-072 **正确地**不带头
  → 后端     service.rs → 401 TENANT_CONTEXT_MISSING
```

**SDK 的行为是对的**（AC-S-072 明确要求「未配置就不带头」，空串更糟）。
错的是**壳没给它值**。裁定如下：

| 项 | 裁定 | 决策 |
|---|---|---|
| **租户来源** | **登录页加租户输入框**：用户填租户标识 → 登录请求带上 `x-tenant-id` → 登录后整个会话沿用 | **D98** |
| **会话持久化** | **`sessionStorage`**：access / refresh token 与租户存 `sessionStorage`，整页重载后恢复；**关标签页即失效** | **D99** |

**为什么不用 `localStorage`**：refresh token 长期驻留浏览器是供应链/XSS 的放大器；
`sessionStorage` 的作用域是「单标签页 + 单次会话」，泄露窗口小得多。

**验收要求**：

- 登录页有租户输入（不能硬编码、不能从环境变量读死）
- 登录请求**带** `x-tenant-id`（断言头存在且等于用户填的值）
- 登录成功后，后续请求沿用该租户
- 整页重载后（`page.reload()`）会话仍在 → AC-S-068 步骤 3 可达
- 新开标签页**不**继承会话（`sessionStorage` 语义）

### AC-S-069: SDK 统一解析 RFC 9457 错误体为稳定错误码
- **前置条件**: 后端返回 **403**，响应体为后端**真实形状**：
  ```json
  {
    "type": "https://errors.base.example/insufficient-permission",
    "title": "权限不足",
    "status": 403,
    "detail": "the current role does not grant the permission `rbac:role:write`",
    "instance": "/api/rbac/roles",
    "code": "INSUFFICIENT_PERMISSION",
    "trace_id": "…",
    "params": { "required": "rbac:role:write" }
  }
  ```
  > ⚠️ rev 1 把 `title` 写成「未认证」—— 那是 **401** 的 title，
  > 403 的 title 是「**权限不足**」（`crates/platform-core/src/problem.rs:141`）。
  > 照 rev 1 字面写测试，测的是后端**永不返回**的响应体。
- **测试步骤**:
  1. SDK 发起会得到该响应的请求，断言抛出的错误对象
  2. 断言受检字段：`message` / `detail` / `title` / **整体序列化**四个字符串
  3. **HTTP 错误状态 + 非 JSON 体**（如 HTML 500 页）
  4. **2xx + 非 JSON 体**
  5. **空体**（204 / 空响应）
  6. **网络层失败**（fetch 抛错）
- **预期结果**:
  - 1 → 错误对象上 `code` **精确等于** `INSUFFICIENT_PERMISSION`
    （**不是** `title` 的「权限不足」，也不是 `detail` 的英文原文 ——
    前端只认稳定错误码，文案由 i18n 渲染）
  - 2 → 四个字符串**均不含**响应体的原始内容
    （SDK 把响应体塞进 `error.response` 之类的新字段也**不算通过**）
  - 3 → `code === 'client.INTERNAL_ERROR'`
  - 4 → `code === 'client.INTERNAL_ERROR'`（**解析失败**，不是原样返回）
  - 5 → 2xx + 空体 → 返回**空值**（不抛错）；
    错误状态 + 空体 → `code === 'client.INTERNAL_ERROR'`
  - 6 → `code === 'client.NETWORK_ERROR'`，
    **不**抛裸 `TypeError`，**不**把原始响应体塞进错误信息
- **类型**: 功能

### AC-S-070: access token 过期时自动刷新并重放，且并发只刷新一次
- **前置条件**:
  - 客户端持有**已过期**的 access token 与有效的 refresh token
  - **过期 token 的构造方式（写死）**：用测试签名密钥直接签一个 `exp` 在过去的 JWT，
    **不动服务配置**（默认 TTL 是 900 秒 ——
    `crates/platform-iam/src/config.rs:123`；调 `BASE_IAM_ACCESS_TTL_SECONDS` 要重启服务、
    会影响其他用例）
- **测试步骤**:
  1. 用过期 token 发起请求 → 后端 401 → SDK 自动刷新 → 重放原请求 → 后端 200
  2. **同时**发起 5 个请求（token 已过期），统计刷新接口被调用次数
  3. 刷新**失败**（refresh token 失效）时，原请求的最终结果
  4. 重放时**原请求体**是否被正确再次发送
  5. **组合**：过期 token + 写请求（带幂等键）→ 重放
- **预期结果**:
  - 1 → 调用方拿到 **200 的业务结果**，感知不到中间的 401；
    **重放请求不带 `idempotency-key`**（D92 —— 401 意味着「请求没执行」，
    没执行的请求不该留幂等声明；沿用旧键会被后端判为「还在处理」→ **409**）
  - 2 → 刷新接口**只被调用 1 次**（不是 5 次 —— 5 次会触发后端的
    **轮换 + 重放检测**（`crates/platform-iam/src/service.rs:606-615`），
    **整个会话 family 作废**，用户被踢下线）。
    ⚠️ **本步骤是前端单测**（注入式 fake fetch + 受控 pending Promise 统计调用次数），
    **禁止**对真实后端并发打 refresh —— 那会让**测试本身成为破坏源**，
    且失败后同用例内的后续断言会因「会话已死」而误判
  - 3 → 抛出**后端返回的**错误码
    （`TOKEN_EXPIRED` / `TOKEN_REVOKED` / `TOKEN_REUSE_DETECTED` 之一），
    **不**无限重试
    （D94：**透传**后端码，不合成 `UNAUTHENTICATED` ——
    合成会丢掉「为什么被踢下线」的直接线索）
  - 4 → 重放请求的 body 与原请求**逐字节相同**
    （取证：`new Uint8Array(await request.clone().arrayBuffer())` 比对；
    流式 body 不可复用 → 重放会发空 body，表现为「第一次失败、第二次成功」的诡异 bug）
  - 5 → 组合用例（TC-011-034）：重放**不带**幂等键 → 后端返回 **200**，
    **不是 409**
- **类型**: 功能

### AC-S-071: 写操作自动注入幂等键，读操作不注入
- **前置条件**: 客户端对后端发起写请求
- **测试步骤**:
  1. SDK 发起 `POST` / `PUT` / `DELETE` 请求，断言请求头
  2. SDK 发起 `GET` 请求，断言请求头
  3. 同一个 SDK 实例连续发起两次**相同逻辑**的写请求，断言两次的幂等键
  4. 端到端：带**同一**幂等键的**同一**写请求发两次，断言后端响应
  5. 断言键的格式与唯一性范围
- **预期结果**:
  - 1 → 三个写方法都带 `idempotency-key` 头
  - 2 → `GET` **不带**该头
  - 3 → 两次的幂等键**不同**（每次调用自动生成新键 ——
    复用同一个键会让第二次调用被当成重放，返回首次的结果，**写入被静默丢弃**）
  - 4 → 第二次返回**首次的结果**（后端幂等中间件 ——
    `crates/platform-iam/src/http.rs:295`），且**只落库一次**。
    ⚠️ **本条仅对 `/api/iam/*` 成立** —— 幂等中间件**只装在 IAM 路由**上；
    `/api/rbac/*` 与业务模块路由**当前没有**幂等支持（D93）。
    前端**不得**依赖其他路由的幂等行为
  - 5 → 键匹配 **UUID v4 正则**；键**每请求新生成**、
    **不得由路径派生**、**不得跨路径复用**
    （后端去重键是 `(tenant_id, idempotency_key)`，**不含** method/path ——
    `migrations/20261118000400_iam_auth.up.sql:194`；
    同租户内不同路径撞键 → 第二次会返回**第一个请求的响应体**）
- **类型**: 功能

### AC-S-072: SDK 注入租户与空间上下文头
- **前置条件**: SDK 实例配置了 `tenantId`
- **测试步骤**:
  1. 发起请求，断言请求头
  2. **不配置** `tenantId` 时发起请求，断言请求头
  3. 配置了 `spaceId` 时发起请求，断言请求头
  4. 断言**登录 / 刷新**请求的租户头
- **预期结果**:
  - 1 → 带 `x-tenant-id` 头，值**等于**配置的租户
  - 2 → **不带** `x-tenant-id` 头（不是带空串 —— 空串会被后端当成
    「有一个空租户」）。
    原因：后端在请求头缺租户时**从令牌推断**租户
    （`crates/platform-rbac/src/http.rs:191-194`）
  - 3 → 带 `x-space-id` 头
  - 4 → 租户头注入于**所有**请求（**含 login / refresh**）——
    后端 login / refresh 都要求租户上下文，缺则 401 `TENANT_CONTEXT_MISSING`
    （`crates/platform-iam/src/error.rs:54`）。
    若 SDK 不在登录请求上带租户头，`tenantId` 未配置时**根本登不进去**
- **类型**: 功能

### AC-S-073: 主题令牌以 CSS 变量输出，暗色模式可切换
- **前置条件**: 无
- **测试步骤**:
  1. 应用主题令牌，断言文档根元素上的 CSS 变量
  2. 切换到暗色模式，断言变量值
  3. 切回亮色，断言变量值
  4. 检查 `@base/ui` 与 `@base/theme` 的源码（**扫描规则见下**）
- **预期结果**:
  - 1 → 根元素上有**精确的变量名清单**（见 design §3.4 的令牌契约表），
    取值格式符合契约（含单位）
  - 2 → 变量值**等于** design §3.4 令牌契约表中的**暗色**取值；
    且 `color-scheme` **等于** `dark`；
    至少一个**模式相关**变量（颜色类）**等于**暗色期望值
    （**不是**「不等于亮色值」—— D83 的假绿经典）；
    **模式无关**变量（`radius` / `font-family`）**不参与**「变了」的判定
  - 3 → 变量值**等于**亮色期望值（**可逆**）；`color-scheme` **等于** `light`
  - 4 → **颜色字面量扫描**通过，扫描参数**写死**：
    - 文件集：本期五个包的 `src/**/*.{ts,svelte.ts}`
    - 正则：`#[0-9a-fA-F]{3,8}\b` + 常见 CSS 颜色函数（`rgb(` / `hsl(` / `oklch(`）
    - 排除集：`test/`、`*.test.ts`、**令牌定义文件**（唯一允许字面量的位置）
    - 允许范围（D95）：**`@base/ui` 零字面量**；
      `@base/theme` 的**单一令牌定义文件**允许；**其余位置一律禁止**
- **类型**: 功能

### AC-S-074: 主题主色接品牌接口，不硬编码
- **前置条件**: 品牌接口 `GET /api/config/branding` 返回**扁平 map**
  （**恰好 11 个白名单键** —— `crates/platform-config/src/whitelist.rs:14-27`），
  其中 `primaryColor` 字段为 `#0055aa`
- **测试步骤**:
  1. 前端应用主题，断言 `--base-primary`
  2. 品牌接口的 `primaryColor` 为**非法值**时，断言 `--base-primary`
     （用 `PUT /api/config/entries` 写入 —— 后端 `primaryColor` 是 `type:string`，
     **不校验颜色格式**，`crates/platform-config/manifest.json:51-56`）
  3. 检查前端源码（扫描范围**排除** `test/` 与 `*.test.ts`）
  4. 断言 `apps/shell` 的品牌实现**委托** `@base/theme`
- **预期结果**:
  - 1 → `--base-primary` **等于** `#0055aa`（来自接口）
  - 2 → **等于** `currentColor`（中性兜底），**不是** `#zzz`、不是 `red`。
    ⚠️ 主题包在无品牌 / 品牌非法时**必须自己**把 `--base-primary` 写成 `currentColor`，
    **不依赖宿主样式表**（否则宿主没有 `:root{--base-primary:currentColor}` 时
    读到的是**空串**而不是 `currentColor` → 预期不成立）
  - 3 → 产品源码里**没有**品牌颜色字面量；
    **测试夹具里的期望值不算违规**（扫描已排除 `test/`）
  - 4 → `apps/shell` **不再有独立的品牌实现**（REQ-010 的
    `apps/shell/src/lib/branding.ts` 改为委托 `@base/theme`）——
    两份实现会让 REQ-010 的既有 E2E（AC-S-055）因漂移而红
- **类型**: 功能

### AC-S-075: 五个包无交叉依赖，各自产物独立
- **前置条件**: 先 `pnpm -r build`（`dist/` 被 gitignore，必须先构建）
- **测试步骤**:
  1. 检查 `packages/*/package.json` 的 `dependencies`（**双向断言**）
  2. 扫描 `dist/**/*.js` 与 `dist/**/*.d.ts` 的导入关系与 DOM API 引用
  3. 检查 `packages/sdk` 的产物是否含 UI / 组件相关代码
- **预期结果**:
  - 1 → **双向**断言：
    - **禁止**：`@base/sdk` → `@base/ui` / `@base/theme` / `@base/hooks`；
      `@base/auth` → `@base/ui`；`@base/ui` → `@base/hooks`
      （表格要用分页 hooks 时 → `@base/ui` 的 Table **只接受 props**
      （rows / columns / page / pageSize / total / onPageChange），
      **不引用** `@base/hooks`；分页与表格状态由消费方用 `@base/hooks` 组合传入）
    - **规定**：`@base/auth` → `@base/sdk` 存在；`@base/ui` → `@base/theme` 存在
    - 所有内部依赖写成 `workspace:*`
    > ⚠️ rev 1 全是「不依赖」否定式断言 → **今天就假绿**
    > （七个包的 `package.json` **都没有** `dependencies` 字段）
  - 2 → 每个包的 `dist` 只 import 自己的依赖；
    `packages/sdk/dist/**` 里**无** `@base/(ui|theme|hooks)` 导入，
    且**无** DOM API 引用（`document.` / `window.` / `HTMLElement` / `customElements`）
  - 3 → **不含**组件代码
- **类型**: 功能 / 架构
- **范围声明**: 本 AC 只覆盖本期 **5 个包**
  （`auth` / `hooks` / `sdk` / `theme` / `ui`）；
  `i18n`（REQ-021）与 `schema`（REQ-012）**不在本 AC 范围**

---

## 本期不验收（明确排除）

| 项 | 原因 |
|---|---|
| OpenAPI → 类型**自动生成** | 后端无 OpenAPI 产物；本期用手写类型 + 契约测试代替（父 AC-019 收窄） |
| Storybook | 依赖树重，磁盘预算 20G（已用 ~4G） |
| 图表 / 富文本 / 树 / 穿梭 / 上传 / 编辑器组件 | 本期只做基础组件 |
| MSW mock 基线 | 前端单测用注入式 `fetch` 假件（不引依赖） |
| 完整 i18n | 属 REQ-021 |
| 元数据驱动渲染器 | 属 REQ-012 |
| 完整布局框架（侧边栏 / 面包屑 / 多标签 / 租户空间切换器） | 本期只做**最小壳**（登录页 + 路由 + 受保护页 + 403 页） |
| 全站幂等支持 | 本期只覆盖 `/api/iam/*`（D93） |
