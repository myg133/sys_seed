# 状态：REQ-022 出站 IdP 与单点登出

- **状态**: ✅ **已完成并合并** `develop` @ `aa5b305`（2026-11-21）
  → **rev 3 补记**（2026-11-21）：**并行实现前的两个耦合缺口**已补（D257 / D258）
  → **已认领**，**两张 Dev 任务卡已落盘**（棒 1 / 棒 2），**待派单并行开工**
- **更新者**: myg133-ba-1
- **更新时间**: 2026-11-21 01:05

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-19 23:58 | — | 草稿 | myg133-ba-1 | 从 REQ-013 拆出。用户裁定「拆两棒，本轮先做入站」→ 出站部分独立成 REQ-022，承接父 AC-006 |
| 2026-11-20 22:00 | 草稿 | **规格进行中** | myg133-ba-1 | **开工前实测完成**（可行性 / 依赖 / 来源核实）+ **用户裁定范围**（全做，拆小任务，可并行） |
| 2026-11-20 23:58 | 规格进行中 | **规格已就绪** | myg133-ba-1 | **规格四件套定稿**（demand rev2 / acceptance 22 条写实 / decisions D196~D220 / design-summary）+ **契约升 v3** + **REQ-016 按 D109 改写** + 技术债 +2 |
| 2026-11-21 00:10 | 规格已就绪 | **规格已定稿** | myg133-ba-1 | **QA 挑刺 39 条**（阻塞 10 / 高 19 / 中 10）**全部接受** → `acceptance.md` **rev 2** + `decisions.md` **rev 2**（**D221~D256**）+ design-summary rev 2 + 技术债 +2（**TD-043 / TD-044**） |
| 2026-11-21 00:40 | 规格已定稿 | **已认领** | myg133-ba-1 | `aw.py claim take REQ-022` → CLAIMED；worktree `feature-REQ-022` @ `ed9d8d8`（基于 `develop@310e4d3`），工作区干净 |
| 2026-11-21 01:05 | 已认领 | **待派单** | myg133-ba-1 | **rev 3 补记**：写 Dev 任务卡时发现**两个并行耦合缺口** → **D257**（设备码「用户批准」端点补缺）+ **D258**（跨栈 JSON 字段名钉死）；**两张 Dev 任务卡**落盘（`feature-meta/req-022-dev-{backend,frontend}.md`） |

---

## 1. 需求来源核实（D122）

**REQ-022 的需求来源是「用户原话」，不是 BA 自补。** 已从会话快照**逐字核实**：

> 3. sso 多协议适配
> 9. 预留应用接入能力
>
> —— 用户 2026-11-17 的 9 项需求清单（首个会话快照 `g000000000000--ctx_43539355-*.jsonl`）

- **原话 #3「sso 多协议适配」** → 拆成两棒：**入站**（REQ-013，已交付）+
  **出站**（本需求，底座自己当 IdP）。
- **原话 #9「预留应用接入能力」** → REQ-016「应用接入与应用注册表」（依赖本需求）。
- ⚠️ OAuth2 的具体形态（4 种 grant / introspect / 免登 / 单点登出）**全部是 BA 的设计**，
  用户原话里没有 → **范围类决定已回用户**（见 §2）。

**关于「拆两棒」范围裁定的原始回执**：REQ-013 的 `demand.md:42` 与 `status.md:28`
记载了 2026-11-19 的用户裁定，但该次 `ask_user` 的**问卷回执在持久化的会话日志里没有找到**
（`2026/11/19` 全部 messages/snapshots 均无 `questionnaire-response`）
→ 记为「**仓内文档记载，原始回执未找到**」。需求本身（原话 #3/#9）已逐字核实，不受影响。

## 2. 范围裁定（用户，2026-11-20 22:18）

> **「全做，但是拆成小任务，如果可以并行，可以让多个子 agent 一块来做。」**

- 即 草稿 **A + B + C + D 全做**（4 种 grant + introspect + 免登 + 单点登出 + client 模型）。
- **实现策略**：拆小任务 + **并行子 Agent**（文件集不相交）。
- ⚠️ 这与草稿里「REQ-013 已有 12 条 AC，出站再加就翻倍，单棒做不扎实」的拆分理由**相反** ——
  用户已知情并选择「一轮做完、分棒并行」。**不得再自行缩小范围。**
- **规模**：**22 条 AC**（本项目最大一轮；REQ-013 是 12 条 / REQ-014 是 13 条）。

## 3. AC 编号段（D23 全局唯一）

- 原预留 **AC-S-110~121**（12 条）**不够用**。
- 全仓已用最大编号 = **133**（REQ-014 占 AC-S-122~133）。
- → **本需求用 `AC-S-134 ~ AC-S-155`**（22 条，**134 起全空**，已核实）。

## 4. 开工前可行性实测（2026-11-20 22:10，零新增依赖）

| 需要的能力 | 树里现成的 | 结论 |
|---|---|---|
| ES256 签发 | `crates/platform-iam/src/jwt.rs:130` `JwtKeySet::sign` | ✅ 需新增 claims 类型 + 泛型签发方法（D109 #1） |
| JWKS 暴露 | `crates/platform-iam/src/service.rs:544` `IamService::jwks()` | ✅ 已有 |
| PKCE S256 | `crates/platform-iam/src/sso/oidc.rs:404` `code_challenge()` | ✅ **已实现，直接复用** |
| 随机串 | `crates/platform-iam/src/sso/oidc.rs:393` `random_token()` / `crypto.rs:39` `random_base64url()` | ✅ |
| 摘要 | `crates/platform-iam/src/crypto.rs:56` `digest_base64url()` | ✅ |
| 出站 HTTP | `crates/platform-iam/src/sso/oidc.rs:88` `OidcHttpClient`（hyper + rustls） | ✅ 但语义不同 → 本轮独立实现（D220 / TD-042） |
| 会话失效 | `crates/platform-iam/src/store.rs:754` `revoke_family` / `:765` `revoke_all_refresh_tokens` | ✅ 复用 |
| 权限点查询 | `crates/platform-core/src/extension/permission.rs:239` `PermissionProvider` | ✅ 用 trait，不引 rbac |
| 鉴权入口 | `crates/platform-core/src/extension/permission.rs:273` `require_permission` | ✅ C-5 唯一入口 |
| **新增第三方依赖** | — | **0 个** |

## 5. 本轮规格阶段的**关键决策**（详见 [decisions.md](decisions.md)）

| 决策 | 一句话 |
|---|---|
| **D196** | 契约升 **v3**：C1 加 `/api/oauth` 前缀；新增 **C14**（OAuth 协议端点用 RFC 错误体，不用 Problem Details） |
| **D197** | 新 crate **`platform-oauth`**；不引 rbac（用 `PermissionProvider` trait） |
| **D198** | 只发 **JWT**（ES256），`aud = client_id`；opaque 不做 |
| **D199** | client 表归 REQ-022；REQ-016 引用并扩展 |
| **D200** | secret 由平台生成、只存 SHA-256 摘要、**只显示一次** |
| **D201** | `scope` = **权限点码本身**；授予 = 请求 ∩ client 白名单 ∩ 用户权限 |
| **D202** | 同意记录用 **`platform_oauth_grant`**（一表三用） |
| **D203** | 授权端点 = **前端路由 + 后端 preview/confirm/deny**（平台会话是 Bearer，导航不带 header） |
| **D204** | 同意票据用 **ES256 签名**（零新表、不可伪造、5 分钟） |
| **D205~D207** | 授权码只存摘要 + 交换即删 + TTL 60s；**强制 PKCE S256**；client 认证 `client_secret_basic` |
| **D208** | 内省/撤销：只有 confidential client，且**只操作自己的令牌** |
| **D209** | ⚠️ **底座业务端点默认拒绝** OAuth 令牌（本轮不做 opt-in → TD-041） |
| **D210** | 撤销语义 = grant 级；底座侧立即生效，只验签的第三方最长延迟一个 TTL |
| **D211** | 单点登出：Back-Channel / Front-Channel；**通知失败不影响登出** |
| **D212~D217** | 免登 = `prompt=none` + 已有 grant；错误码 8 处登记；权限点三处齐；C-5 命名禁区；不做 refresh token |
| **D218~D220** | 前端边界（同意页/设备码页/我的授权归 REQ-022）；4 张表启用 RLS；出站 client 独立实现 |

## 6. 跨需求改动（已执行）

| # | 改了什么 | 状态 |
|---|---|---|
| 1 | `crates/platform-iam` 加 `JwtKeySet::sign_claims<T>` + `IamService::keys()` | **待 Dev 实现**（D109 #1，已在 REQ-022 demand 的依赖段声明） |
| 2 | `BA/contracts/api-conventions/` 升 **v3**（C1 + C14） | ✅ **本轮已完成**（`CURRENT` → v3；v2 保留一个 Sprint；INDEX 已更新） |
| 3 | `BA/demands/REQ-016-app-integration/demand.md` 改写（范围重叠拆清） | ✅ **本轮已完成**（rev 2） |
| 4 | `BA/demands/REQ-016-app-integration/acceptance.md` 的 AC-S-024 降级表述 | ✅ **本轮已完成**（rev 2，按 D210） |
| 5 | `BA/plan/roadmap.md` 同步 | ✅ 已更新 |
| 6 | `BA/plan/tech-debt.md` +2（TD-041 / TD-042） | ✅ 已更新（现 **42 条**） |

## 7. 下一步（下一棒照此做）

1. **认领 REQ-022** → 建 `feature-REQ-022` worktree（`aw.py claim take REQ-022`）→
   写**棒 1 / 棒 2 两张 Dev 任务卡**（文件集严格不相交，见 D256）→
   **棒 1 ∥ 棒 2 并行**（子 Agent）：
   - **棒 1（后端）**：`crates/platform-oauth/**` + `crates/platform-iam/src/{jwt,service}.rs`
     + `crates/platform-server/src/{main.rs,tests/i18n_titles.rs}`
     + `crates/platform-core/src/titles.rs` + `crates/xtask/src/catalog.rs`
     + `migrations/**` + `scripts/{seed-e2e.sh,mock-logout-receiver.sh,e2e-server.sh}`
     + `e2e/oauth.spec.ts` + `Cargo.toml` / `Cargo.lock`
   - **棒 2（前端）**：`apps/shell/**`（含 `Login.svelte` / `App.svelte` 的 `return_to` 改动）
     + `packages/i18n/**`
   - ⚠️ **两棒靠约定字符串耦合**，任务卡必须写死三处：
     ① **9 个**错误码的精确清单（码 + ASCII 标题 + 中文文案）；
     ② 前端路由 `/oauth/authorize` / `/oauth/device` / `/oauth/grants` / `/oauth/logout`；
     ③ `manifest.json` 的 `menus[].route` 与前端路由**逐字相同**。
2. **测试节奏**（dev-guide **§9.14**，2026-11-20 用户明确要求）：
   - **开发期**：**增量测试**（只跑被改动的 crate + 直接依赖方，不跑 E2E）；
   - **终版**（代码冻结后、合并前）：**集中全量 + E2E 一次跑完**
     （12 crate 逐个 + 前端三包 + `svelte-check` + `capabilities --check`
     + Playwright **0 skipped**）。
3. **Pre-merge 复验 → 修复轮 → 独立复核 → BA 终验 → 合并 develop + 回收 worktree + 复盘**

## 8. 阻塞项

**无。** REQ-013 / REQ-014 已交付（`develop` @ `310e4d3`），前置全部就绪。

## 9. 交付物

| 文件 | 状态 |
|---|---|
| [demand.md](demand.md) | ✅ **rev 2**（范围按用户裁定全做 + D109 跨需求改动声明） |
| [acceptance.md](acceptance.md) | ✅ **rev 2**（22 条写实，AC-S-134~155；**QA 39 条挑刺已逐条裁决修完**） |
| [decisions.md](decisions.md) | ✅ **rev 2**（D196~D220 + **rev 2 的 D221~D256**，共 **61 条**） |
| [design-summary.md](design-summary.md) | ✅ **rev 2**（5 张表 / 端点清单 / 5 条时序 / file:line 落点 / **9** 个平台错误码 / 5 个配置项 / **12** 条风险） |
| [status.md](status.md) | ✅ 本文件 |
| [test-cases/](test-cases/index.md) | ✅ **QA 产出 131 条**（TC-022-001~131，编号连续；22/22 AC 覆盖） |
| [../../feature-meta/qa-req022-spec-review.md](../../feature-meta/qa-req022-spec-review.md) | ✅ **QA 挑刺 39 条**（阻塞 10 / 高 19 / 中 10） |
| `BA/contracts/api-conventions/v3.md` | ✅ 本轮已建（C1 + C14） |
| `BA/plan/dev-guide-low-spec.md` §9.14 | ✅ **测试节奏**：增量 → 终版集中全量 + E2E（2026-11-20 用户要求） |

---

## 10. BA 自查发现（QA 挑刺期间做的独立核实）

> 这一节是 BA 在 QA 跑挑刺期间**自己打开源码核实**的结果（dev-guide §9.11）。
> QA 的报告回来后，这些要么并入 `decisions.md` rev 2，要么被 QA 的同款发现覆盖。

### 10.1 ✅ D203 的前提**成立**（三处核实）

| 核实点 | 结论 | 证据 |
|---|---|---|
| `/oauth/*` 会不会被回落到 SPA | ✅ **会**（且**应该**） | `crates/platform-server/src/static_files.rs:36` `is_api_path` 只覆盖 `/api` 与 `/api/**`；`:61-64` API 路径 404，其余无扩展名路径回落 `index.html` |
| SPA 能不能读 URL 的 query | ✅ 能 | `apps/shell/src/lib/router.ts:229-242` `write()` 保留 `location.search`；`apps/shell/src/pages/SsoCallback.svelte:55` 读 `window.location.search` 取 ticket（**同类先例已跑通**） |
| 登录后能不能回到原页面 | ⚠️ **有条件**（见 10.2） | `apps/shell/src/lib/router.ts:217-225` + `:261` `intendedPath`；`apps/shell/src/App.svelte:122` `router.navigate(resolution.intendedPath ?? '/')` |

### 10.2 ⚠️ **BA-01（阻塞级候选）**：`intendedPath` 回跳**丢掉 query** → 授权参数全丢

- **机制**（`apps/shell/src/lib/router.ts`）：
  - `start()`（`:298`）调 `apply(environment.location.pathname, true)` —— **只传 pathname**；
  - `apply()`（`:252-268`）在守卫拒绝时 `const intendedPath = path;`（`:261`）—— **仍然只有 pathname**；
  - `write()`（`:232`）`const keepsQuery = path === environment.location.pathname;`
    → 跳 `/login` 时 `keepsQuery` 为 **false** → **query 被抹掉**；
  - 登录后 `App.svelte:122` 跳 `intendedPath` —— **原 query 已丢**。
- **后果**：用户被导航到
  `/oauth/authorize?response_type=code&client_id=…&redirect_uri=…&scope=…&state=…&code_challenge=…`
  后若未登录 → 登录 → 回到 `/oauth/authorize`（**参数全无**）→
  授权流程**断裂**（`client_id` 缺失 → 400）。
- **为什么现有 SSO 回调没踩到**：`/sso/callback` 是**公开路由**
  （`apps/shell/src/lib/routes.ts:71-76`，无 `requiresAuth`）→ 不触发 redirect 分支 →
  `write()` 的 `keepsQuery` 为 true → ticket 保住了。
  → **「公开路由」才是这个坑的安全区**。
- **建议修法（棒 2 范围内，不动 `router.ts`）**：
  1. `/oauth/authorize` 注册为**公开路由**（**不设** `requiresAuth`）；
  2. 页面自己读 `window.location.search` 取全部授权参数；
  3. 未登录 → 渲染「登录并继续」按钮 → 把**完整 URL** 编码进 `?return_to=` → 跳 `/login?return_to=…`；
  4. **改 `Login.svelte`**：登录成功后 `location.replace(return_to)`（回跳），
     **没有** `return_to` 时保持现行为（跳首页）——
     `apps/shell/src/pages/Login.svelte` 与 `apps/shell/src/App.svelte`
     **都在棒 2 的文件集内**，**不动** `router.ts`（REQ-011 的核心）。
  5. ⚠️ 改动 `Login.svelte` / `App.svelte` 会影响 REQ-011 的既有前端用例 → 棒 2 必须跑前端回归。
- **为什么不改 `router.ts`**：`resolve()` 用带 query 的字符串去 `match()` 会匹配失败
  （`:204-207`）→ 要改就得让 `resolve` 先剥 query → 牵动 REQ-011 已交付的路由核心，
  风险远大于收益。

### 10.3 ✅ 编号段核实：`AC-S-134 ~ AC-S-155` **无真冲突**

- 用 `git grep` 核 **改动前的基线 `b65a73d`**（排除 REQ-022 自己的目录）：
  基线最大真编号 = **133**；`134` 与 `155` 在基线里**未被占用**。
- 唯一的 `AC-S-136` 出现在 `BA/feature-meta/qa-recheck-req014.md:638`
  —— 那是 **REQ-014 复核报告里的一处笔误**（REQ-014 的 AC 是 `AC-S-022` + `AC-S-122~133`，
  没有 136）。→ 记为 **BA-02（低）**，在裁决轮一并处理。

### 10.4 ✅ 零新增依赖核实（锁文件级）

`Cargo.lock` 里 **21 个**必需包**全部已在**：
`sha2 0.10.9` / `base64 0.22.1` / `jsonwebtoken 10.4.0` / `hyper 1.12.0` /
`hyper-util 0.1.21` / `hyper-rustls 0.27.10` / `rustls 0.23.46` / `uuid 1.29.3` /
`time 0.3.55` / `serde 1.0.229` / `serde_json 1.0.154` / `tokio 1.54.1` /
`rand 0.8.8` / `p256 0.13.2` / `argon2 0.5.3` / `inventory 0.3.25` / `sqlx 0.8.6` /
`tower 0.5.3` / `http-body-util 0.1.5` / `bytes 1.12.1` / `http 1.5.0`
→ D197 的「零新增第三方依赖」成立。

### 10.5 ⚠️ **BA-03（高）**：棒 1 ∥ 棒 2 的文件集必须**严格不相交**

- `packages/i18n/src/locales/*.json` **两个棒都要改**
  （棒 1 要加 10 个 `error.OAUTH_*`；棒 2 要加 `oauth.*` 的界面文案）→ **同文件 = 冲突**。
- **裁定**：**整个 `packages/i18n/**` 归棒 2**；
  棒 1 把 10 个错误码的**精确清单**（码 + ASCII 标题 + 中文文案）写进任务卡，
  由棒 2 一并写进词库。
  → `crates/platform-core/src/titles.rs` 与 `crates/platform-server/tests/i18n_titles.rs`
  仍归棒 1（后端侧）。
- 其余分棒边界（初步）：

  | 棒 | 文件集 |
  |---|---|
  | **棒 1**（后端） | `crates/platform-oauth/**`、`crates/platform-iam/src/{jwt,service}.rs`、`crates/platform-server/src/main.rs`、`crates/platform-server/tests/i18n_titles.rs`、`crates/platform-core/src/titles.rs`、`migrations/**`、`Cargo.toml`、`Cargo.lock` |
  | **棒 2**（前端） | `apps/shell/**`、`packages/i18n/**` |

  → 两侧**零重叠** ✓（`Cargo.toml` / `Cargo.lock` 只归棒 1）。
  ⚠️ 两棒之间靠**约定字符串**耦合，任务卡必须写死：
  ① 10 个错误码的**精确清单**；② 前端路由路径 `/oauth/authorize` / `/oauth/device` / `/oauth/grants`；
  ③ `manifest.json` 的 `menus[].route` 与前端路由**逐字相同**。

---

## 11. rev 3 补记（2026-11-21 01:05）：**并行实现前的两个耦合缺口**

> **背景**：BA 在写两张 Dev 任务卡时发现 —— 这两个缺口**不是**「QA 挑刺能挑出来」的问题，
> 而是**只有把规格落成「两个互不相交的文件集」时才会暴露**的缺口。
> 若不补，棒 1 与棒 2 并行时**各造各的**，代价是终版 E2E 全红 + 返工一轮。

| # | 缺口 | 现象 | 补法 |
|---|---|---|---|
| **BA-04** | **设备码的「用户批准」没有后端端点** | AC-S-142 步骤 4 要求「输入 `user_code` → 点「允许」」，但 design §3 的端点清单里**没有**对应端点（`device_authorization` 只**申请**，`token` 只**轮询**）→ 前端设备码页**没有可调的接口** | **D257**：补 2 个平台端点 —— `GET /api/oauth/device/verify?userCode=`（批准前展示）+ `POST /api/oauth/device/approve`（`{ userCode, action }`）。查不到/过期 → **400 + `OAUTH_INVALID_REQUEST`** → **不新增第 10 个错误码** → **`TITLES` 行数仍是 93**（AC-S-152 不动） |
| **BA-05** | **跨栈 JSON 字段名没钉死** | 棒 2 的 4 个页面要调棒 1 的 8 个平台端点，但请求/响应字段名规格里**从没写死** → 棒 2 只能猜 `client_id` 还是 `clientId` | **D258**：按契约 **C13** 钉死 —— **平台端点 camelCase / 协议端点用 RFC 的 snake_case**（**两套不混**）；8 个平台端点的请求体与响应体字段**逐字写死** |

**两条都**不改任何一条 AC**（22 条 AC 与 131 条用例**不受影响**）：
- D257 只是让 AC-S-142 步骤 4 **可实现**；
- D258 只是让 AC-S-147 / AC-S-150 里的字段名**有据可依**。

**同步落盘**：
- `decisions.md` → rev 3 段（D257 / D258）
- `design-summary.md` → **rev 3**（§3.2 端点表补 2 行 + 状态行）
- `acceptance.md` → 头部加「rev 3 补记」小节（**明写「不改任何一条 AC」**）

### 11.1 两张 Dev 任务卡（已落盘）

| 卡 | 路径 | 覆盖 |
|---|---|---|
| **棒 1（后端）** | `BA/feature-meta/req-022-dev-backend.md` | 新 crate `platform-oauth` / 装配 / 迁移 / 夹具 / mock 接收端 / E2E |
| **棒 2（前端）** | `BA/feature-meta/req-022-dev-frontend.md` | 4 个页面 / API 层 / 4 条路由 / 登录回跳 / i18n 词库 |

**两卡都写死了三处约定字符串**（D256 ④）：

1. **9 个**平台错误码的精确清单（码 + ASCII 标题 + 中英文案）——
   棒 1 写 `TITLES`，棒 2 写词库 `error.<CODE>`；
2. 前端路由 `/oauth/authorize` / `/oauth/device` / `/oauth/grants` / `/oauth/logout`；
3. `manifest.json` 的 `menus[].route` **逐字** = `/oauth/grants`。

**两卡都写了**：文件集严格不相交、同一 worktree 的并行纪律
（禁 `git commit` / `checkout` / `stash` / `add -A`）、
§9.14 阶段一的增量测试口径、变异验证方法、报告格式。

### 11.2 下一步

**并行派两个子 Agent**（棒 1 / 棒 2）→ 各自按卡施工 →
**终版**（代码冻结后）BA **集中跑一次**全量 + E2E（§9.14 阶段二）。
| 2026-11-21 01:40 | 待派单 | **棒 2 已交付** | myg133-ba-1 | 前端 18 个文件；279 shell 测试 / 45 i18n 测试 / svelte-check 0 error / 8 条变异；**零越界改动** |
| 2026-11-21 01:50 | 棒 2 已交付 | **棒 1 施工中** | myg133-ba-1 | 跨栈线协议对账发现**两处必须修**（`route_layer` 叠加 / 普通用户调不了「我的授权」）→ **D259** + **TD-045**；已 steer 给棒 1 |
| 2026-11-21 05:50 | 棒 1 施工中 | **已合并** | myg133-ba-1 | 两轮独立复核（FAIL → 修复轮 → PASS-WITH-RISK）+ BA 终验 → 合并 `develop` @ `aa5b305` + worktree 回收 + 复盘落盘 |

