# REQ-011「前端 SDK 骨架」—— 测试用例总索引

- **需求编号**: REQ-011「前端 SDK 骨架」
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: AC-016（SDK 按需引入且无全局副作用）、AC-017（权限组件与路由守卫联动）、AC-019（API 类型自动同步）
- **验收基线**: [acceptance.md](../acceptance.md) —— **AC-S-064 ~ AC-S-075（12 条）**，本用例集**只**覆盖这一段
- **设计依据**: [design-summary.md](../design-summary.md)（rev 1，2026-11-19 10:50）
- **契约**: [api-conventions v2](../../../contracts/api-conventions/v2.md)、[extension-points v2](../../../contracts/extension-points/v2.md)（C-5 / §4 补充）
- **代码基线**: `code/` @ `develop` @ `a3fae66`（REQ-010 合并点）—— **本需求尚未实现**
- **状态**: 待实现（用例本身**尚未执行**，不得据此判定需求通过）
- **版本**: 第 1 版（rev 1）—— QA 独立复核 acceptance.md rev 1，逐条核对后端源码

> ⚠️ **本文件同时是「用例集」与「挑刺报告」**。§8 是 QA 对 12 条 AC 的逐条挑刺裁定表 —— **12 条 AC 全部需要修订**，建议按 §8.9 的结论先出 acceptance.md rev 2 再开工。

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **48**（TC-011-001 ~ TC-011-048，编号连续无跳号） |
| 覆盖 AC | **12 / 12**（AC-S-064 ~ AC-S-075，无遗漏） |
| 挑出问题数 | **40**（高 **11** / 中 **21** / 低 **8**） |
| 自动化分布 | 可自动化 = **46**、暂不可自动化 = **2**（TC-011-021 / 022，见 §6） |
| 层分布 | frontend-unit = **35**、rust-integration = **10**、e2e = **2**、rust-unit = **1** |
| 类型分布 | 功能 = **30**、安全 = **13**、性能 = **4**、集成 = **1** |
| 优先级分布 | P0 = **40**、P1 = **8** |
| 用例文件 | 5 个（另本索引 1 个、任务书 1 个） |
| 每条用例字段 | id / ac / title / preconditions / steps / expected / type / layer / automatable / priority / design / refs（**12 项全部齐备**）+ notes（37 / 48 条有，用于标注该用例守的是「现稿」还是「修订稿」） |

> **比照 Sprint 3 复盘的基线**：REQ-009 挑出 22 条、REQ-010 挑出 50 条（含 12 条 BA 自身写错）。本轮挑出 **40 条**，其中 **3 条是 BA 自己写错的**（#4 / #10 / #18），**8 条带「硬伤」标签**（#1、#2、#3、#6、#7、#8 为纯硬伤；#14、#31 兼属越界）。

---

## 2. 文件清单

| 文件 | 能力域 | AC 范围 | 用例数 |
|---|---|---|---|
| [backend-permissions.json](backend-permissions.json) | 后端有效权限点出口与登录响应 | AC-S-064、AC-S-065 | 10（001 ~ 010） |
| [frontend-auth.json](frontend-auth.json) | `can()` / `<Can>` / 路由守卫 | AC-S-066、AC-S-067、AC-S-068 | 12（011 ~ 022） |
| [frontend-sdk.json](frontend-sdk.json) | 错误码 / 刷新重放 / 幂等键 / 上下文头 | AC-S-069 ~ AC-S-072 | 15（023 ~ 037） |
| [frontend-theme.json](frontend-theme.json) | 主题令牌 / 暗色模式 / 品牌主色 | AC-S-073、AC-S-074 | 7（038 ~ 044） |
| [packaging.json](packaging.json) | 五包依赖方向与产物隔离 | AC-S-075 | 4（045 ~ 048） |
| [qa-brief.md](qa-brief.md) | 给 Dev 的任务书摘要 | —— | —— |

---

## 3. 夹具约定

> **原则**：租户与空间在本仓**都没有实体表**（`tenant_id` / `space_id` 都是字符串）。夹具 id 写死为字符串，不造影子表。
> **造数走管理员连接**（`base_test`），**业务访问走 `base_app`**（超管会绕过 RLS）。
> **用例不得依赖执行顺序**。

### 3.1 租户与账号

| ID | 值 | 用途 |
|---|---|---|
| T1 | `tenant-a` | 主测试租户 |
| T2 | `tenant-b` | 跨租对照（TC-011-004） |

| user_id | 租户 | 绑定角色 | 用途 |
|---|---|---|---|
| `u-perm-read@example.com` | T1 | R1 = `role-perm-read`（**只**授予 `rbac:role:read`） | 有效权限点主用例 |
| `u-perm-none@example.com` | T1 | **无任何角色** | 空权限集合 |
| `u-perm-wildcard@example.com` | T1 | R3 = `role-perm-wildcard`（授予 `rbac:role:*`） | 通配展开 |

初始密码统一 `Passw0rd!`；登录一律带 `x-tenant-id: tenant-a`（**后端 `login` 缺租户上下文即 401**，见 `code/crates/platform-iam/src/service.rs:296`）。

### 3.2 角色（`platform_role` + `platform_role_permission`）

| 角色 code | 授权 | 注意 |
|---|---|---|
| `role-perm-read` | `rbac:role:read` | **必须是自定义角色**：内置 `platform-admin` 的权限集 = 注册表全集（`code/crates/platform-rbac/src/service.rs:857-863`），用它会让「不含 `rbac:role:write`」不成立 |
| `role-perm-wildcard` | `rbac:role:*` | 通配会被展开成具体权限点 |

### 3.3 前端夹具

| 夹具 | 说明 |
|---|---|
| 注入式 fake fetch | `const fakeFetch = async (req) => new Response(body, { status })`（design §4.1，不引 MSW） |
| 受控 pending Promise | 构造并发窗口（TC-011-028） |
| 权限矩阵金标准 | **双方共用**的 JSON 夹具（TC-011-015）：`{ user, permissions[], probe:{ permission, expectAllowed } }`，`permissions[]` 由后端 `effective_permissions` 实测产出 |
| 令牌契约表 | 亮/暗两套 CSS 变量期望值（TC-011-038 ~ 040），**QA 提案，待 BA 固化** |

---

## 4. 用例索引（按 AC）

| AC | 标题 | 用例 | 用例数 | 层 |
|---|---|---|---|---|
| AC-S-064 | 后端能返回当前用户的有效权限点集合 | 001 ~ 005 | 6 | rust-integration |
| AC-S-065 | 登录响应带上权限点 | 006 ~ 010 | 4 | rust-integration / rust-unit |
| AC-S-066 | `can()` 在前端本地判定，且与后端一致 | 011 ~ 015 | 5 | frontend-unit / rust-integration |
| AC-S-067 | `<Can>` 组件按权限点显隐 | 016 ~ 018 | 3 | frontend-unit |
| AC-S-068 | 路由守卫拦截无权访问 | 019 ~ 022 | 4 | frontend-unit / e2e |
| AC-S-069 | SDK 统一解析 RFC 9457 错误体为稳定错误码 | 023 ~ 026 | 4 | frontend-unit |
| AC-S-070 | access token 过期时自动刷新并重放，且并发只刷新一次 | 027 ~ 030 | 4 | frontend-unit |
| AC-S-071 | 写操作自动注入幂等键，读操作不注入 | 031 ~ 034 | 4 | frontend-unit |
| AC-S-072 | SDK 注入租户与空间上下文头 | 035 ~ 037 | 3 | frontend-unit |
| AC-S-073 | 主题令牌以 CSS 变量输出，暗色模式可切换 | 038 ~ 040、043 ~ 044 | 5 | frontend-unit |
| AC-S-074 | 主题主色接品牌接口，不硬编码 | 041 ~ 042 | 2 | frontend-unit |
| AC-S-075 | 五个包无交叉依赖，各自产物独立 | 045 ~ 048 | 4 | frontend-unit（静态检查） |

### 4.1 优先级

| 优先级 | 条数 | 说明 |
|---|---|---|
| P0 | 40 | 必测 —— 覆盖全部安全语义、跨 AC 组合缺陷与假绿高发点 |
| P1 | 8 | 应测 —— 覆盖边界与澄清性行为 |

---

## 5. 判定规则（假绿防护）

本用例集遵守父需求 D83（否定式断言会假绿）、D84（快照优于行数断言）、D89（快照取点纪律）：

| 规则 | 落地方式 |
|---|---|
| **否定式断言 → 精确值断言** | 「不含 X」写成 `getPropertyValue(...) === null`、`innerHTML === ''`、`filter(...).length === 0`；「含 X」写成集合**整体相等**或 `=== 期望值` |
| **集合相等优于包含关系** | TC-011-002 / 006 / 009 用「去重升序后深度相等」，一次断言同时覆盖「含」与「不含」 |
| **扫描类检查必须带非空前置** | TC-011-043 / 044 / 046 / 047 都断言「扫描到的文件总数 >= 1」，防止「扫了 0 个文件 → 空集 → 通过」 |
| **期望值必须来自外部真值** | TC-011-041 用接口返回值；TC-011-015 用后端实测产出的金标准夹具；TC-011-039 / 040 用成对的亮/暗期望值（**不是**「不等于亮色值」） |
| **错误码断言精确到字符串** | 断言 `error.code === 'INSUFFICIENT_PERMISSION'`，不写「错误码正确」 |

---

## 6. 未覆盖 / 阻塞项

| 项 | 状态 | 说明 |
|---|---|---|
| **TC-011-021 / TC-011-022（端到端 403）** | **暂不可自动化** | `apps/shell` 无路由、无登录页、无 403 页 —— AC-S-068 缺实现主体（挑刺 #8）。BA 裁定后本组用例可自动化 |
| **AC-S-068 的「直接输入 URL」端到端** | 包级已覆盖（TC-011-020），端到端待裁定 | 服务端侧深链可达前端已核实（`code/crates/platform-server/src/static_files.rs:32-39` 无扩展名路径回落 `index.html`） |
| **令牌契约表（亮/暗变量期望值）** | **QA 提案，待 BA 固化** | AC-S-073 步骤 2 要求「等于暗色期望值」却未给期望值（挑刺 #5） |
| **权限矩阵金标准夹具** | Dev 侧交付物 | TC-011-015 依赖它；`permissions[]` 必须由后端 `effective_permissions` 实测产出，不得手写 |
| **「过期 access token」的构造** | Dev 侧需确定 | `BASE_IAM_ACCESS_TTL_SECONDS`（`code/crates/platform-iam/src/config.rs:248`）或用测试密钥签一个 `exp` 在过去的 JWT（挑刺 #22） |

---

## 7. 挑刺口径

| 严重度 | 定义 |
|---|---|
| **高 / 硬伤** | 照这条做会做出错的东西，或这条根本无法成立 |
| **中** | 不可判定、可能假绿、覆盖不足 |
| **低** | 表述问题、边界不清 |

| 类型 | 含义 |
|---|---|
| **硬伤** | AC 描述的行为与后端现状 / 现有构建链冲突，按字面实现必错 |
| **不可判定** | 步骤与预期无法一一对应，或缺可机械断言的判据 |
| **假绿** | 断言方式会让错误实现通过（否定式、开放集合、空集） |
| **矛盾** | 与同需求的其他 AC、或与后端契约互相打架 |
| **越界** | 需要改契约 / 改父 AC / 动其他需求范围才能成立 |

---

## 8. 挑刺裁定表（40 条）

> 证据行号基于 `code/` @ `a3fae66`。`acceptance.md:xx` 指 rev 1 原文行号。
> 「落点」= 本用例集里承载该修订的用例。

### 8.1 高（11 条 —— 硬伤 / 不可判定 / 假绿 / 矛盾）

| # | AC | 问题 | 严重度 | 建议修订 | 类型 |
|---|---|---|---|---|---|
| 1 | AC-S-070 + AC-S-071 | **两条 AC 合起来会让重放拿到 409 而不是 200。** 幂等中间件在 handler **之前** claim 键（`code/crates/platform-iam/src/http.rs:303-320`），而 `complete_idempotency` **只在成功响应时**调用（`http.rs:365-372`）；首次请求因令牌过期返回 401（由 handler 内的 `Authenticated` 提取器产生）→ 键停在 `status=0`（`code/crates/platform-iam/src/store.rs:1042-1059`）。重放若沿用同一键 → 命中 `status=0` 的已声明键 → **返回 409 CONFLICT**（`http.rs:336-356`）。AC-S-070 步骤 1 写「后端 200」，AC-S-071 步骤 1 要求写操作带键，两者叠加必错 | 高 | AC-S-070 步骤 1 补一句：**「刷新后的重放必须使用新的 `idempotency-key`（或重放时不注入该头）」**；AC-S-071 增加一条约束「重放不复用原键」，并把该组合写成显式步骤 | 硬伤 |
| 2 | AC-S-071 | **步骤 4 的预期只在 `/api/iam/*` 成立。** 幂等中间件只装在 IAM 路由上（`code/crates/platform-iam/src/http.rs:399`），`/api/rbac`（`code/crates/platform-rbac/src/http.rs:395-430`）与业务模块路由**没有**幂等支持。但 AC 的前置条件写的是泛指的「对后端发起写请求」，步骤 4 的证据却指向 IAM —— 实现者按 `/api/rbac/roles` 写端到端必然失败，而那是**预期写错了** | 高 | 步骤 4 把路由**钉死**到 `/api/iam/*` 的具体写接口，并加一句「本条仅对 `/api/iam/*` 成立；其他路由当前无幂等支持，前端不得依赖」 | 硬伤 |
| 3 | AC-S-070 | **步骤 2 未指定构造方式，按字面端到端会作废会话。** 后端 refresh 是轮换 + 重放检测：同一个 refresh token 用第二次即 `TokenReuseDetected` 并**作废整个 family**（`code/crates/platform-iam/src/service.rs:606-615`）。若实现有 bug 导致 5 次 refresh 打出去，用户当场被踢下线 —— **测试本身成了破坏源**；且失败后同一测试内的后续断言会因「会话已死」而误判 | 高 | 步骤 2 明确为**前端单测**（注入式 fake fetch + 受控 pending Promise 统计调用次数），**禁止**对真实后端并发打 refresh；端到端版本若保留，须用**独立 family 的独立会话**并声明失败即作废 | 硬伤 |
| 4 | AC-S-070 + AC-S-069 | **两条 AC 的错误码互斥。** AC-S-069 要求 `code` **精确等于**响应体的 `code`；AC-S-070 步骤 3 要求刷新失败抛 `code=UNAUTHENTICATED`。但后端刷新失败返回的是 `TOKEN_REVOKED` / `TOKEN_EXPIRED` / `TOKEN_REUSE_DETECTED`（`code/crates/platform-iam/src/error.rs:44-46,154-156,176-178`）。两条不可同时满足 | 高 | 二选一并写死：① 刷新失败统一合成 `UNAUTHENTICATED`（则在 AC-S-069 写明「**刷新失败路径例外**，合成码不取自响应体」）；② 透传后端码（则改 AC-S-070 步骤 3 的期望） | 矛盾 |
| 5 | AC-S-073 | **步骤 2 不可判定。** 要求「断言**等于**暗色模式下的期望值」，却**没给期望值**；同时用「用 `color-scheme` **或**一组颜色变量体现」给出两个互斥的实现选项 —— 两个选项对应两套不同的期望值。AC 自己引用了 D83（否定式断言会假绿），却没对自己适用 | 高 | 把亮/暗两套令牌写成**具体数值表**（变量名 + 取值），并规定暗色必须改变**哪几个**变量、`color-scheme` 是否必须切到 `dark`；同时明确「模式无关变量」（radius / font-family）不参与「变了」的判定 | 不可判定 |
| 6 | AC-S-073 | **步骤 4 与步骤 1 互斥，且检查范围未定义。** 令牌体系必然需要颜色字面量（亮/暗两套调色板），若按字面禁止 `@base/theme` 源码出现颜色字面量 → 令牌体系无法实现；而现有构建链是 `tsc` only（`code/packages/auth/package.json` → `"build": "tsc -p tsconfig.json"`；`code/packages/auth/tsconfig.json` → `"include": ["src/**/*.ts"]`），把调色板放进 `.css` 又不会被 tsc 编译进产物。同时步骤 4 没定义扫描哪些文件、什么正则、什么排除集 —— 三者缺一即假绿 | 高 | 步骤 4 写明：① `@base/ui` **零字面量**；② `@base/theme` 的字面量**只允许**出现在一个指定的令牌定义文件（路径写死，且只能有一个）；③ 扫描的文件集、正则、排除集（排除 `test/`、`*.test.ts`）全部写死 | 硬伤 |
| 7 | AC-S-067 | **`<Can>` 组件在现有构建链下产不出可发布产物。** 五个包的构建脚本都是 `tsc -p tsconfig.json`（`code/packages/*/package.json`），`code/packages/*/tsconfig.json` 的 `include` 只有 `src/**/*.ts` → **`.svelte` 文件不会被编译**，`dist/` 里不会出现组件；`main` 指向 `./dist/index.js`。dev 下 Vite 别名指向源码能跑（`code/apps/shell/vite.config.ts`），但发布产物是空的 | 高 | 三选一并写进 AC：① `<Can>` 改为**框架无关**的渲染函数（不叫组件，也不需要 svelte 构建）；② 给 `@base/auth` 增加 `svelte-package` 构建（要动构建链 + 依赖，超出本期「五个包」范围）；③ 组件落在 `apps/shell`，AC 的主体改为「应用侧」 | 硬伤 |
| 8 | AC-S-068 | **没有实现主体。** `apps/shell` 无路由（无 `routes/` 目录、`apps/shell/package.json` 依赖只有 `@base/i18n`/`@base/sdk`/`@base/ui`）、无登录页、无 403 页、无受保护页面；`design §3.3` 只给了框架无关的 `guard(permission): RouteGuard`。步骤 3「403 页**不发起**目标接口的请求」是**应用整体行为**，没有任何主体保证它 | 高 | AC 收窄为「`@base/auth` 提供框架无关的 `guard(permission)` 判定（返回放行/拒绝 + 403 落点），**应用侧**把路由元数据接到 `guard` 上」；把「403 页不发起请求」降级为**应用侧 E2E**，并明确该 E2E 需要新增哪些壳能力（登录页 + 路由 + 受保护页 + 403 页）—— 若这些不在 REQ-011 范围，E2E 应移到 REQ-012 / REQ-020 | 硬伤 |
| 9 | AC-S-075 | **步骤 1 全是「不依赖」的否定式断言 → 在当前状态下必然通过（纯假绿）。** 仓库里 **7 个包的 `package.json` 都没有 `dependencies` 字段**，AC 的三条禁止项今天就已经全绿；即便 Dev 把五个包完全做成互不相连的孤岛，AC 照样通过 | 高 | 步骤 1 改成**双向**断言：禁止项不存在 **且** 规定项存在（`@base/auth` → `@base/sdk`、`@base/ui` → `@base/theme`），并要求内部依赖写成 `workspace:*` | 假绿 |
| 10 | AC-S-074 | **步骤 3 与测试夹具自相矛盾。** 「源码里**没有**品牌颜色字面量（`#0055aa` 也不许硬编码 —— 它是接口的运行时值）」：但验收这条 AC **本身**就必须用 `#0055aa` 当期望值 —— 若扫描范围包含测试文件，测试自己就违规 | 高 | 步骤 3 写明扫描范围**排除** `test/` 与 `*.test.ts`，并区分「测试夹具里的期望值」与「产品源码里的字面量」 | 矛盾 |
| 11 | AC-S-066 | **步骤 5 没有可执行的机制。** `require_permission` 是 axum **Layer**（`code/crates/platform-core/src/extension/permission.rs:243-273`），不能被单测直接调用 —— 只能通过「打一条受该权限点保护的路由」来观察后端判定；且「前后端比对」需要**双方共用的**权限矩阵夹具，AC 一个字都没提 | 高 | 步骤 5 改写为：「后端侧用**真实路由探针**（`GET /api/rbac/roles` 验 `rbac:role:read`、`POST /api/rbac/roles` 验 `rbac:role:write`）记录放行/拒绝；前端侧用**同一份金标准夹具**的 `permissions[]` 调 `can()`；逐条比对」 | 不可判定 |

### 8.2 中（21 条）

| # | AC | 问题 | 严重度 | 建议修订 | 类型 |
|---|---|---|---|---|---|
| 12 | AC-S-065 | 步骤 2 要求「与 AC-S-064 步骤 3 **逐项相等**」，但 AC-S-064 步骤 3 是**包含关系**断言 —— 两个包含关系无法「逐项相等」；且两者是**两个不同时刻**的 DB 读（登录时一次、`/me/permissions` 一次），中间发生授权变更就会不等 | 中 | 改成「两次调用在**授权状态不变**的前提下，权限点**集合相等**（去重升序后深度相等）」，并把「两次调用之间不发生授权变更」写进前置条件 | 不可判定 |
| 13 | AC-S-064 | 前置条件只说「R1 授予 `rbac:role:read`」，**未排除通配授权**。后端会展开通配（`code/crates/platform-rbac/src/service.rs:842-867`）→ 若 R1 授予 `rbac:role:*`，`rbac:role:write` 就**在集合里**，AC 的「不包含」不成立 | 中 | 前置条件补「R1 是**自定义**角色、只授予 `rbac:role:read`、**不含任何通配模式**」；并补一条通配展开的派生用例 | 不可判定 |
| 14 | AC-S-064 | 未声明「本接口**不新增**权限点」。若实现者为它加一个 `rbac:me:read` 之类的权限点，就要同步 manifest + `capabilities/permissions.json` + 启动对账（D27：manifest 与库不一致 → **进程起不来**，`code/crates/platform-server/src/main.rs:306-311`），还会被 C-5 扫描（`BA/contracts/extension-points/v2.md` §4） | 中 | 补一句「本接口走**豁免**（与 `/api/rbac/permissions` 注册表同款），**不引入新权限点**；因此不需要 `require_permission` 调用点，也不触发 C-5 / D27 对账」 | 越界 |
| 15 | AC-S-064 | 漏掉真实存在的第三条路径：**请求头租户与令牌租户不一致 → 404**（`code/crates/platform-rbac/src/http.rs:186-189`），既不是 401 也不是 403 | 中 | 补一条跨租户用例（期望 404 / `NOT_FOUND`） | 覆盖不足 |
| 16 | AC-S-065 | `design §2.2` 明确承诺了降级行为（「未注入实现时 permissions 为空数组，登录**仍然成功**」），但 **12 条 AC 没有一条覆盖它** —— 降级路径无守门 | 中 | 补一条降级用例：Provider 未注入时登录成功且 `permissions === []` | 覆盖不足 |
| 17 | AC-S-064 / AC-S-065 | 未规定 `permissions` 的**顺序与去重**规则。后端是 `BTreeSet`（`code/crates/platform-rbac/src/service.rs:780-790`）→ 天然升序去重，但 AC 没写 → 「两个入口逐项相等」无法做确定性断言 | 中 | 写死「升序（字节序）、去重」，并把「集合相等」作为跨入口断言的基准 | 不可判定 |
| 18 | AC-S-069 | **前置条件给的错误体示例是 BA 自己写错的**：`{ title:'未认证', status:403, code:'INSUFFICIENT_PERMISSION' }` —— 三个字段互相矛盾。后端 403 的 title 是「**权限不足**」（`code/crates/platform-core/src/problem.rs:141`），401 的 title 才是「未认证」（`problem.rs:117`）。照示例字面写测试，测的是一个后端**永不返回**的响应体 | 中 | 换成后端真实形状：`{type:'https://errors.base.example/...', title:'权限不足', status:403, detail:'the current role does not grant the permission `rbac:role:write`', instance:'/api/rbac/roles', code:'INSUFFICIENT_PERMISSION', trace_id:'...', params:{required:'rbac:role:write'}}` | 不可判定 |
| 19 | AC-S-069 | `NETWORK_ERROR` 是**客户端合成码**，后端错误码表里没有（`code/crates/platform-core/src/problem.rs:31-38` 只有 4 个；`code/crates/platform-iam/src/error.rs:32-64` 有 12 个）。AC 未说明客户端合成码的归属、命名空间、以及是否会与后端码冲突 | 中 | 在 AC 或契约里写死「客户端合成码表」（`NETWORK_ERROR` / `INTERNAL_ERROR` / …）与命名空间规则 | 越界 |
| 20 | AC-S-069 | 步骤 3 的「**不**把原始响应体塞进错误信息」没有判定方法 —— 不检查哪些字段，SDK 把响应体塞进 `error.response` 之类的新字段照样通过 | 中 | 写明受检字段清单（本用例用 `message` / `detail` / `title` / 整体序列化四个字符串），并要求错误对象有一个**固定的、不含响应体**的文案 | 假绿 |
| 21 | AC-S-069 | 步骤 3 把「**非 JSON 响应体**」与「**网络层失败**」混在一条里；且没定义 2xx + 非 JSON 体、204 空体、截断体分别怎么判 —— 这几种今天都会走同一条 JSON 解析路径 | 中 | 拆成三条：**HTTP 错误状态 + 非 JSON 体** → `INTERNAL_ERROR`；**2xx + 非 JSON 体** → 明确是解析失败还是原样返回；**空体** → 明确返回什么 | 覆盖不足 |
| 22 | AC-S-070 | 「客户端持有**已过期**的 access token」这个前置**无法复现** —— 默认 TTL 是 900 秒（`code/crates/platform-iam/src/config.rs:123`）。AC 没说是把 `BASE_IAM_ACCESS_TTL_SECONDS` 调到 1（要重启服务、影响其他用例），还是用测试密钥签一个 `exp` 在过去的 JWT | 中 | 写死构造方式（建议：用测试签名密钥直接签一个过期 JWT，不动服务配置） | 不可判定 |
| 23 | AC-S-071 | 步骤 3 只要求「两次的幂等键**不同**」—— 用递增计数器也能通过。而后端按 `(tenant_id, idempotency_key)` 去重（`code/migrations/20261118000400_iam_auth.up.sql:194`），键冲突会造成**静默丢数据** | 中 | 追加键格式约束（UUID v4 正则），并说明键必须**每次请求新生成、全局唯一** | 假绿 |
| 24 | AC-S-071 | 后端去重键是 `(tenant_id, idempotency_key)`，**不含 method / path**（`code/migrations/20261118000400_iam_auth.up.sql:194`；`code/crates/platform-iam/src/store.rs:1049-1056`）→ 同一租户内两个**不同路径**的请求若撞键，第二次会返回**第一个请求的响应体**。AC 未说明键必须跨路径唯一 | 中 | 补一句「键必须每请求新生成、不得由路径派生、不得跨路径复用」 | 覆盖不足 |
| 25 | AC-S-072 | 未规定**登录 / 刷新**请求是否也带租户头。而后端 `login` / `refresh` 都要求租户上下文（`code/crates/platform-iam/src/service.rs:296`、`:581`），缺则 401 `TENANT_CONTEXT_MISSING`（`code/crates/platform-iam/src/error.rs:54,160,184`）。SDK 若不在登录请求上带 `x-tenant-id`，`tenantId` 未配置时根本登不进去 | 中 | 写明「`x-tenant-id` 注入于**所有**请求（含 login / refresh），租户由 SDK 配置项提供」 | 覆盖不足 |
| 26 | AC-S-073 | 步骤 1 的「`--base-primary` / `--base-radius` / `--base-font-family` 等变量」是**开放集合**（"等"）→ 不可判定；且 `--base-radius` 的单位（`4` 还是 `4px`）未定 —— 后端 `theme.radius` 是 **number**（`code/crates/platform-config/manifest.json:58-64`）；`--base-font-family` 的名字与后端键 `theme.fontStack`（**array**，`manifest.json:78-84`）**不一致**，映射关系未写 | 中 | 写死变量名清单（精确集合）与取值格式（含单位），并给出后端配置键 → CSS 变量的映射表 | 不可判定 |
| 27 | AC-S-073 / AC-S-074 | **落点未指明** —— REQ-010 已经在 `apps/shell` 实现了同一件事：`:root{--base-primary:currentColor}`（`code/apps/shell/src/app.css:4`）+ `HEX_COLOR` 校验与写属性（`code/apps/shell/src/lib/branding.ts:26,60-64`）。REQ-011 若在 `@base/theme` 再实现一遍 → **两份实现**，AC-S-055 的既有 E2E 会因漂移而红 | 中 | 明确 AC-S-073/074 的落点是 **`@base/theme`**，并要求 `apps/shell` 改为委托 `@base/theme`（单一实现） | 矛盾 |
| 28 | AC-S-074 | 步骤 2 要求「保持中性兜底（`currentColor`）」，但没说**由谁提供**。若主题包只在品牌合法时才写属性，而宿主页面没有 `:root{--base-primary:currentColor}`，读到的是**空串**而不是 `currentColor` → 预期不成立 | 中 | 写明「主题包在无品牌 / 品牌非法时**必须自己**把 `--base-primary` 写成 `currentColor`，不依赖宿主样式表」 | 不可判定 |
| 29 | AC-S-075 | 标题写「**五个**包」，步骤 1/2 写 `packages/*` —— 实际有 **7** 个包（`auth`/`hooks`/`i18n`/`schema`/`sdk`/`theme`/`ui`），通配会把 REQ-021（`i18n`）与 REQ-012（`schema`）一起扫进来 | 中 | 写死本期范围（5 个包），并声明 `i18n` / `schema` 不在本 AC 范围（或显式纳入并给同样的依赖约束） | 不可判定 |
| 30 | AC-S-075 | 步骤 3「不含 UI / 组件相关代码」**没有定义**，且与步骤 1（依赖关系）、步骤 2（dist 导入关系）**重叠** | 中 | 合并进步骤 2，并把判据写成可机械执行的两条：`packages/sdk/dist/**` 里无 `@base/(ui|theme|hooks)` 导入 + 无 DOM API 引用（`document.` / `window.` / `HTMLElement` / `customElements`） | 假绿 |
| 31 | AC-S-075 | `ui → theme` 的单向约束与「表格组件要用 hooks」之间**没有出口** —— `design §3.5` 本期组件含 Table，`design §3.6` 把分页/表格状态放在 `@base/hooks`，而 AC 禁止 `ui → hooks`。实现者只剩三条路：内联一份逻辑（与 hooks 分叉）、违反 AC、或把 Table 做成纯展示组件（正确但没写） | 中 | 补一条组合契约：「`@base/ui` 的 Table **只接受 props**（rows / columns / page / pageSize / total / onPageChange），**不引用** `@base/hooks`；分页与表格状态由消费方用 `@base/hooks` 组合传入」 | 越界 |
| 32 | 全局 | **SDK 的公开 API 形状没钉死** —— `design §3.2` 给了 `createApiClient({...})` 的参数，但没给**请求方法**的签名与返回/异常约定；`design §3.3` 给了 `createAuthStore`，但 `guard()` 的返回类型没定。用例无法在 API 定型前写死 | 中 | 在 design 里钉死最小公开 API：请求方法名 + 参数 + 返回值 + 异常类型（`ApiError{code,status,...}`）；`guard()` 返回 `{allowed, redirectTo}` | 不可判定 |

### 8.3 低（8 条）

| # | AC | 问题 | 严重度 | 建议修订 | 类型 |
|---|---|---|---|---|---|
| 33 | AC-S-066 | 步骤 3（`order:invoice:delete` 未登记）对实现**没有约束力** —— 集合判定的自然结果就是 false；它只守得住「不抛错」这一条 | 低 | 保留但降级为 P1，并在 AC 里把该步骤的真实意图写成「不抛错、返回 false」 | 覆盖不足 |
| 34 | AC-S-067 | 步骤 2「内容**不在 DOM 里**」未区分「文本节点缺失」与「元素缺失」；`<Can>` 渲染一个空壳元素也算满足前者 | 低 | 写明判定为「容器的 `innerHTML` 精确等于空串」 | 不可判定 |
| 35 | AC-S-068 | 缺「**未登录**访问受保护路径 → 登录页（不是 403）」这一条。未认证与无权限混同，会让未登录用户看到 403 而不是登录入口 | 低 | 补一条：未登录 → 登录页，且页面不出现 `403` 字样 | 覆盖不足 |
| 36 | AC-S-070 | 步骤 4「请求体**逐字节相同**」没给取证方式 —— 只断言字符串相等会漏掉编码差异 | 低 | 写明取证方式（`new Uint8Array(await request.clone().arrayBuffer())` 逐字节比对） | 不可判定 |
| 37 | AC-S-072 | 步骤 2「不配置时不带 `x-tenant-id`」没说明**原因**（后端在请求头缺租户时**从令牌推断**租户，`code/crates/platform-rbac/src/http.rs:191-194`）→ 后人可能「顺手修成带空串」 | 低 | 补一句原因注释 | 边界不清 |
| 38 | AC-S-074 | 前置条件给的品牌响应 `{ title:'X', primaryColor:'#0055aa' }` 与后端真实形状不符 —— `GET /api/config/branding` 返回**扁平 map**、**恰好 11 个白名单键**（`code/crates/platform-config/src/http.rs:341-360`；`code/crates/platform-config/src/whitelist.rs:14-27`）。且未说明「非法主色」怎么写进去（后端 `primaryColor` 是 `type:string`、**不校验颜色格式** → `code/crates/platform-config/manifest.json:51-56`，确实能写进去，但需要 `config:entry:write` 权限 + 写接口路径） | 低 | 前置条件改成「品牌接口返回的扁平 map 中 `primaryColor` 字段为 `#0055aa`」；步骤 2 补「先用 `PUT /api/config/entries` 把 `primaryColor` 写成非法值」 | 边界不清 |
| 39 | AC-S-075 | 类型标注为「**性能**」—— 依赖与产物隔离是架构 / 可维护性验收，标成性能会让后续按类型统计失真 | 低 | 改成「功能」或新增「架构」类型 | 边界不清 |
| 40 | AC-S-075 | 步骤 2「构建后检查各包 `dist/`」没说明扫描对象（`.js` 还是含 `.d.ts`），也没提 `dist/` 被 gitignore（`code/.gitignore`）→ 必须先构建 | 低 | 写明「先 `pnpm -r build`；扫描 `dist/**/*.js` 与 `dist/**/*.d.ts`」 | 覆盖不足 |

### 8.4 按类型统计

| 类型 | 条数 | 编号 |
|---|---|---|
| 硬伤 | 8 | #1、#2、#3、#6、#7、#8、#14、#31 |
| 不可判定 | 12 | #5、#11、#12、#13、#17、#18、#22、#26、#28、#29、#32、#36 |
| 假绿 | 4 | #9、#20、#23、#30 |
| 矛盾 | 4 | #4、#10、#27、#33※ |
| 越界 | 3 | #19、#14※、#31※ |
| 覆盖不足 | 7 | #15、#16、#21、#24、#25、#35、#40 |
| 边界不清 | 4 | #34、#37、#38、#39 |

> ※ 一条问题可同时命中多个类型（#14 越界 + 硬伤、#31 越界 + 硬伤、#33 覆盖不足 + 矛盾），故分项之和 > 40。

### 8.5 按 AC 统计（哪些 AC 必须改）

| AC | 问题数 | 其中高 | 结论 |
|---|---|---|---|
| AC-S-064 | 4 | 0 | **需改**（#13、#14、#15、#17） |
| AC-S-065 | 3 | 0 | **需改**（#12、#16、#17） |
| AC-S-066 | 2 | 1 | **需改**（#11、#33） |
| AC-S-067 | 2 | 1 | **需改**（#7、#34） |
| AC-S-068 | 3 | 1 | **需改**（#8、#35 + 派生的 #32） |
| AC-S-069 | 4 | 0 | **需改**（#18、#19、#20、#21） |
| AC-S-070 | 5 | 3 | **需改**（#1、#3、#4、#22、#36） |
| AC-S-071 | 4 | 2 | **需改**（#1、#2、#23、#24） |
| AC-S-072 | 2 | 0 | **需改**（#25、#37） |
| AC-S-073 | 4 | 2 | **需改**（#5、#6、#26、#27） |
| AC-S-074 | 4 | 1 | **需改**（#10、#27、#28、#38） |
| AC-S-075 | 6 | 1 | **需改**（#9、#29、#30、#31、#39、#40） |
| 全局 | 1 | 0 | **需改**（#32） |

### 8.6 已核实为「成立、不需要改」的部分（避免过度挑刺）

| 事项 | 核实结论 | 证据 |
|---|---|---|
| 「新接口豁免权限点但要求已认证」 | **成立** —— `/api/rbac` 整个路由有 `authenticate` 中间件，注册表路由只是没有 `route_layer(require_permission(...))` | `code/crates/platform-rbac/src/http.rs:420`、`:428-430`、`:162-199` |
| 「不带令牌 → 401 `UNAUTHENTICATED`」 | **成立** | `code/crates/platform-rbac/src/http.rs:174-179`；`code/crates/platform-core/src/problem.rs:117` |
| 「登录响应与 `/me/permissions` 用同一套展开逻辑」 | **成立** —— 两者都应走 `effective_permissions` → `expand_user_permissions` | `code/crates/platform-rbac/src/service.rs:780-790,842-867` |
| 「品牌接口能返回非法主色」 | **成立** —— `primaryColor` 是 `type:string`，后端**不校验颜色格式** | `code/crates/platform-config/manifest.json:51-56` |
| 「深链能到达前端」 | **成立** —— 无扩展名路径回落 `index.html` | `code/crates/platform-server/src/static_files.rs:32-39` |
| 「AC 条数 = 12」 | **成立** —— `AC-S-064 ~ AC-S-075` 连续 12 条（REQ-010 曾出现「写 17 实为 15」的计数错误，本段无此问题） | `acceptance.md:14` |
| 「用 `platform-core` 的 trait 注入 PermissionProvider」 | **不违反契约** —— `PermissionChecker` / `TransactionalWrite` 已是同目录的「平台能力声明面」先例，`extension_point_count` 仍是 5 | `BA/contracts/extension-points/v2.md:95-114` |

---

## 8.7 总体结论

1. **12 条 AC 全部需要修订**（§8.5）。其中 **11 条问题为高severity**，涉及 **8 条硬伤**：AC-S-064/065 的前置与形状未钉死、AC-S-066 步骤 5 无执行机制、AC-S-067 的组件在现有构建链下产不出来、AC-S-068 没有实现主体、AC-S-069/070 的错误码互斥、AC-S-070 与 AC-S-071 合起来会让重放拿到 409、AC-S-071 的幂等预期只对 IAM 路由成立、AC-S-073 的令牌期望值缺失且检查规则自相矛盾、AC-S-075 的依赖检查今天就假绿。

2. **最危险的一条是 #1** —— 它不是「写得不清楚」，而是**照两条 AC 的字面实现就会做错**：access token 过期 → 刷新 → 重放，若重放沿用同一个 `idempotency-key`，后端返回 **409 CONFLICT**，用户点保存没反应。这类缺陷在两条 AC 各自的用例里都测不出来（AC-S-070 的用例会看到 409 而不是 200，AC-S-071 的用例测不到重放路径），必须有一条**组合用例**（本用例集的 TC-011-034）。

3. **AC-S-068 与 AC-S-067 是「主体不存在」类问题** —— 前者缺路由 / 登录页 / 403 页，后者缺组件构建链。它们不是「用例写得不好」，是**需求范围与现有底座之间有缺口**。这两条要么扩范围（把壳能力纳入 REQ-011），要么改写 AC 的主体（收窄到包级判定）。

4. **能否开工：按现稿不能开工。** 建议顺序：
   - **第 1 步**：BA 出 `acceptance.md` rev 2，把 §8.1 的 11 条高severity问题逐条落进 AC（其中 #1、#2、#4、#5、#6、#8、#9、#11 直接改写步骤与预期）；
   - **第 2 步**：BA 固化 §5 的**令牌契约表**（亮/暗具体数值）与 §8.1 #6 的**令牌定义文件路径**、§8.1 #11 的**权限矩阵夹具**格式；
   - **第 3 步**：Dev 按 §8.1 #7、#8 裁定 `<Can>` 与路由守卫的落点（框架无关函数 / 扩构建链 / 移到应用侧），并按 §8.2 #32 钉死 SDK 公开 API；
   - **第 4 步**：本用例集的 46 条可自动化用例即可落地执行（2 条 e2e 待裁定后解锁）。

   **本用例集本身可立即用于开发**：48 条用例已按「当前 AC + §8 的建议修订」双向写好 —— 每条用例的 `notes` 都标明了它是在守**现稿**还是在守**修订稿**；AC 修订后，只需按 `notes` 的指引回填期望值，不需要重写用例。
