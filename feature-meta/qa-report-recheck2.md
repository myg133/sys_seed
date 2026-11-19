# 修复轮 2 复核报告 —— REQ-011（前端 SDK 骨架）

- **需求**: REQ-011「前端 SDK 骨架」
- **被验提交**: `70eccf0`（`[Dev] 修认证端点错误码透传与 E2E 夹具可复现`），基线 `4292c4e`
- **验收基线**: `BA/demands/REQ-011-frontend-sdk/acceptance.md` **rev 4**（AC-S-070 补充「认证端点不参与自动刷新」）
- **裁定依据**: **D100**（认证端点不刷新）/ **D101**（E2E 夹具独立幂等播种）
- **上一轮报告**: [`qa-report-recheck.md`](./qa-report-recheck.md)（**PASS-WITH-RISK**，新引入 MED-NEW-1 / MED-NEW-2）
- **验证者**: QA Agent —— **未参与前两轮修复**；Dev 自验数字**一律自己复跑**
- **日期**: 2026-11-19
- **结论**: **PASS**

> **一句话**：两个 MEDIUM 在**真机 + 真浏览器**上均真修好；修复没有弄坏 AC-S-067 / 069 / 070 / 071 / D92 / D98 / D99；
> 3 处变异全部被我的用例抓红；两轮全量回归全绿。
> 另有 **1 个 LOW**（新播种脚本的边界：同名不同 id 时 FK 失败）与 **1 个 INFO**（上一轮遗留、非本轮引入），**均不阻塞合入**。

---

## 1. MED-NEW-1（错密码显示 TOKEN_REVOKED）—— **已真修好**

真浏览器（Playwright/Chromium）+ 真服务（`base serve`，8095，`base_test`，播种后的 `tenant-a`）。
探针脚本 `/tmp/r3/browser-probe.mjs`（**我自写，不是仓库 E2E**），**29/29 断言通过**。

| # | 验收判据（rev 4） | 实测 | 结论 |
|---|---|---|---|
| 1 | `code` **精确等于** `INVALID_CREDENTIALS` | 响应 **401**，`body.code === 'INVALID_CREDENTIALS'` | **PASS** |
| 2 | `/api/iam/refresh` 调用次数 **= 0** | `page.on('request')` 全程计数 = **0** | **PASS** |
| 3 | 页面显示「用户名或密码不正确」，**不含** `TOKEN_REVOKED` | `getByText('用户名或密码不正确')` 可见；`body.innerText` 全文不含 `TOKEN_REVOKED`；URL 停在 `/login` | **PASS** |
| 4 | 错密码**不清掉已有会话** | 先登录 → 去 `/login` 用**同一租户** + 错密码：`sessionStorage` 令牌**逐字段未变**（accessToken / refreshToken 相同）、租户仍 `tenant-a`、refresh 计数 **0**；随后敲 `/admin/roles` **仍落 `/403`**（证明旧会话仍然有效，没有被踢出） | **PASS** |

**实现落点核对（逐行读过）**
- `packages/sdk/src/index.ts:97` `RequestOptions.skipAuthRetry` 声明；
- `packages/sdk/src/index.ts:330-337` `canRefresh` 增加 `requestOptions.skipAuthRetry !== true`；
- `packages/sdk/src/index.ts:353-358` 认证端点 401 **不**调 `onUnauthorized`（不清会话）；
- `packages/auth/src/index.ts:291-296` `AuthStore.login()` **固定**传 `{ skipAuthRetry: true }`；
- **全仓只有这一处使用** `skipAuthRetry`（`grep -rn skipAuthRetry apps/ packages/` 命中：sdk 定义/判断 + auth 的 login + 各自测试），**没有误加到业务请求**。

> 第 4 条是**仓库 E2E 没覆盖**的点（`e2e/shell-auth.spec.ts:81` 只覆盖前三条 + 页面文案），
> 由我的探针独立验证。

---

## 2. MED-NEW-2（E2E 夹具不可复现）—— **已真修好**

**严格按复核要求的顺序执行**（这是复现条件）：

```bash
./scripts/test-integration.sh       # ① 先把库搞脏（MED-NEW-2 的复现前提）
./scripts/seed-e2e.sh               # ② 连跑两遍
BASE_E2E_URL=http://127.0.0.1:8095 npx playwright test e2e/   # ③ 必须全绿
```

| 检查 | 实测 | 结论 |
|---|---|---|
| ① 集成跑完后的库状态（**复现前提成立**） | `tenant-a` 下 `u-perm%` 用户 = **0 行**；`u-perm-write@example.com` **全库 0 行**；`u-perm-read@example.com` 全库 6 行但**分散在 `tenant-me-*` 租户**（不是 tenant-a） | 前提成立 |
| ② 播种两遍 | 两遍均 `✓ E2E 夹具就绪`，**exit 0**；第三遍也 exit 0 | **PASS** |
| ② 三个用户各**恰好 1 行** | 独立 SQL：`u-perm-none=1 / u-perm-read=1 / u-perm-write=1`（跑 3 遍后仍合计 **3 行**） | **PASS** |
| ② 邮箱**不是**双域名 | `select count(*) ... email like '%@example.com@example.com'` → **0** | **PASS** |
| ② `base_dev` 拒绝 | `DATABASE_URL_TEST=…/base_dev ./scripts/seed-e2e.sh` → `✗ 拒绝播种 base_dev …`，**exit 2** | **PASS** |
| ② 其它库名也拒绝 | `…/postgres` → `✗ … 不是 base_test，拒绝播种`，**exit 2**（白名单，不是只拦 base_dev） | **PASS** |
| ③ 起服务后 E2E | `10 passed (10.9s)` | **PASS** |
| 权限点数目符合清单 | read=1 / write=2 / none=0（脚本自证表打印，与我独立 SQL 一致） | **PASS** |

**说明（我核对过的两点，避免误判）**
- 上一轮报的「6 个同名」是**跨租户**的（`tenant-me-<uuid>`），因此 `ON CONFLICT (tenant_id, username)` 在 `tenant-a` 上不会撞到它们 —— 脚本的方案在这条复现路径上是成立的（不是碰巧）。
- `E2E 10 passed` 里含新增的 MED-NEW-1 用例（`e2e/shell-auth.spec.ts:81`）。

---

## 3. 是否弄坏了已通过的东西（C 节逐项）

| 项 | 怎么验 | 结果 |
|---|---|---|
| **AC-S-070 业务端点 401 仍刷新 + 重放** | **真机**：登录后把 `sessionStorage.accessToken` 改成垃圾串 → `page.reload()` → 观察 `/api/rbac/me/permissions` 状态序列与 refresh 次数 | **PASS**：refresh **恰好 1 次**；权限点请求 **2 次**，状态序列 **`401,200`**；重放**换了新 Bearer**；刷新后新令牌落盘；页面仍「已就绪」`/`（没被 `skipAuthRetry` 误伤） |
| **AC-S-070 并发只刷新一次** | 我的探针：注入式 fake fetch + 30ms 延迟的控制刷新，**同时**发 5 个业务请求 | **PASS**：5 个全部成功，**刷新接口被调 1 次**（按 AC 要求是前端级，未对真后端并发 refresh） |
| **D92 重放不带幂等键** | 加载**构建产物** `packages/sdk/dist/index.js`，业务 401 → 刷新 → 重放 | **PASS**：首请求键匹配 UUID v4；重放 `headers.get('idempotency-key')` **`=== null`**（不是"键不同"）；重放 body **逐字节相同** |
| **AC-S-069 错误码透传** | 用**从真后端 curl 抓到的真实 403 体**（`code=INSUFFICIENT_PERMISSION`，含 `type/instance/trace_id/params`）驱动 SDK | **PASS**：`code` 精确 `INSUFFICIENT_PERMISSION`；不内嵌完整 body、无 `response/raw/body` 新字段、**未知字段不泄漏**；HTML 500 / 2xx 非 JSON / 错误状态空体 → `client.INTERNAL_ERROR`；204 → 返回空值不抛错；网络失败 → `client.NETWORK_ERROR`（非裸 `TypeError`）；合成码只有 `client.*` |
| **AC-S-071 幂等键** | 同上一行（构建产物） | **PASS**：POST/PUT/DELETE 均带 **UUID v4**；GET **不带**（`=== null`）；连续两次逻辑相同的写 → 键**不同** |
| **D98 租户来源** | 真浏览器抓 `/api/iam/login` 请求头 | **PASS**：`x-tenant-id === 'tenant-a'`（= 表单填的值），登录 200 落 `/` |
| **D99 会话持久化** | 深链 + 重载 | **PASS**：`u-perm-read` 敲 `/admin/roles` → `/403`；`/403` 上再 `reload()` **仍 `/403`**；目标接口 `/api/rbac/roles` 计数 **0** |
| **AC-S-067 `<Can>`** | 真浏览器读 `body.innerHTML` | **PASS**：不含「新建角色」，且不含 `display:none` / `visibility:hidden` |
| 补充：未登录深链 / 新标签页 | 真浏览器 | **PASS**：未登录 → `/login` 且无「403」字样；`context.newPage()` 不继承会话 |

> AC-S-070 步骤 3（刷新失败）也补验了：抛**后端码** `TOKEN_REVOKED`（不是 `UNAUTHENTICATED`、不是 `client.*`），
> 业务 1 次 / 刷新 1 次（**不无限重试**），`onUnauthorized` 调用 **1 次**。

---

## 4. 本轮新引入 / 新发现的问题

### 4.1 【LOW】播种脚本在「同名但不同 id」的既有行上会 FK 失败（`exit 3`）

- **触发条件**：`tenant-a` 里已存在 `u-perm-read@example.com`（或 write / none），但 `id` **不是**脚本固定的
  `e2e-user-perm-*`（例如该用户名曾由 IAM 注册接口 / 手工 / 旧夹具创建）。
- **实测**：先造 `id='legacy-uuid-abc'` 的同名行再播种 →
  ```
  ERROR:  insert or update on table "platform_user_role" violates foreign key constraint "platform_user_role_user_fk"
  DETAIL:  Key (tenant_id, user_id)=(tenant-a, e2e-user-perm-read) is not present in table "platform_iam_user".
  SEED_EXIT=3
  ```
- **原因**：`scripts/seed-e2e.sh:124-136` 的 `ON CONFLICT (tenant_id, username) DO UPDATE` **不改 `id`**
  （仍更新成旧行的 id），而 `:165-171` 的绑定行固定写 `user_id='e2e-user-perm-*'` → 外键（`migrations/20261118000900_rbac_schema.up.sql:110-112`）拒绝。
- **影响**：脚本**大声失败**并整事务回滚（不会静默产生坏夹具）—— 这比修复前的「6 个同名、登录 401」好得多；
  但脚本头注释承诺「用户已存在时**修好**登录必需字段」，该承诺在「id 不同」时不成立。
- **是否阻塞**：**不阻塞**。D101 的复现路径（跑过集成测试 → 播种）实测成立；此路径要求 `tenant-a` 事先有同名用户，概率低。
- **建议**（1 行）：`ON CONFLICT (tenant_id, username) DO UPDATE SET id = EXCLUDED.id`（或先 `DELETE ... WHERE username = ...` 再插）。
  > 若改成 `id = EXCLUDED.id`，需同时确认旧的 `platform_user_role` 行会随 `ON DELETE CASCADE` 清理 —— 我**没有**验证这条改法的副作用（不是本轮任务）。

### 4.2 【INFO】已登录时用**另一个租户**+错密码 → 持久化的租户被覆盖（**非本轮引入**）

- **现象（真机实测）**：登录 `tenant-a` → 去 `/login` 填 `tenant-b` + 错密码 →
  `sessionStorage.tenantId` 变成 `tenant-b`，accessToken **未变**，refresh **0 次**（MED-NEW-1 的修复仍然生效）；
  `reload()` 后权限点请求得到 **404**（跨租户头）→ UI 落 `/login`，但 `sessionStorage` 里的令牌**仍留着**。
- **原因**：`apps/shell/src/lib/api.ts:176` `session.setTenant()` 在**登录前**就写盘（`session.ts:174-177`），
  而 `api.ts:153` 只在 **401/403** 清会话（404 不清）。
- **归属**：`git blame` 确认这两行都来自 **`4ac5074`（上一轮）**，`70eccf0` **未改** `api.ts` / `session.ts`
  （`git show --stat 70eccf0 -- apps/shell/src/lib/api.ts apps/shell/src/lib/session.ts` 为空）。
- **影响**：无越权（服务端按 tenant-b 拒绝），只是「换租户登录失败 → 下次重载被登出」。
  **不阻塞**，登记备查；与本轮 MED-NEW-1 的判据 4（同一租户下不清会话）**不冲突**，已单列。

### 4.3 未发现的问题（明确写出来，避免"查过"被误解为"没问题"）

- 未发现 `skipAuthRetry` 泄漏到业务路径（全仓 grep + 我的 M3 变异证明该检查有效）。
- 未发现后端 Rust 侧改动（`git show --stat 70eccf0` 无 `crates/`）。
- 未发现 `capabilities/` 与源码不一致（`cargo xtask capabilities --check` 13 文件一致）。

---

## 5. 变异验证表

> 纪律：改 → `pnpm -r build`（E2E 打的是 `dist`）→ 跑用例 → 还原 → `sha256sum -c` → `git diff --exit-code -- apps/ packages/ e2e/ scripts/` → 重建复跑确认绿。
> 三次变异后源码树与 `HEAD` **逐字节一致**（见 §7.4）。

| # | 变异点 | 预期红的用例 | **实际** | 结论 |
|---|---|---|---|---|
| **M1** | 去掉认证端点的开关：`packages/auth/src/index.ts:294` 删除 `{ skipAuthRetry: true }` | 我的浏览器探针（错密码文案/code/refresh 计数）+ 仓库 E2E 的 MED-NEW-1 用例 | **变红 ✓**。抓到的实际现象：`refresh 请求次数 = 1`、页面文案 **`登录失败（TOKEN_REVOKED）`**（与 MED-NEW-1 原缺陷一致）；仓库 E2E **1 failed / 9 passed**（红的是 `shell-auth.spec.ts:81`） | 抓到 |
| **M2** | `scripts/seed-e2e.sh` **成功**但不建 `u-perm-write`（同步去掉其账号行、绑定行，并把自证期望改成 `2 1 1`，**保证脚本自己 exit 0**） | 使用写权限用户的 E2E | **变红 ✓**：E2E **2 failed / 8 passed**，红的是「有写权限的用户直接敲 /admin/roles」与「新开标签页不继承会话」——**证明 E2E 真依赖播种出来的那个用户** | 抓到 |
| **M3**（我自选） | 把 `skipAuthRetry` **误加到业务请求**：`apps/shell/src/lib/api.ts:141` 的 `client.get(PERMISSIONS_PATH, { skipAuthRetry: true })` | 我的 AC-S-070 真机检查（业务 401 必须刷新+重放） | **变红 ✓**：`刷新接口恰好 1 次 :: 0`、`权限点请求共 2 次 :: 1`、`状态序列 :: "401"`、会话被清（`sessionStorage` 为 `null`） | 抓到（证明**这条「误伤」检查本身是有效的**） |

**对「变异没变红」的自审**：本轮三次变异**全部按预期变红**，无需双点。
M2 第一次我漏删一个逗号导致脚本自身 psql 语法错（exit 3）—— 那是**变异写错**，不是用例问题；
修正为「脚本成功但少一个用户」后才作为有效变异记录（否则只能证明脚本红，证明不了 E2E 依赖它）。

---

## 6. 没验到的（**不要**当成通过）

| 项 | 状态 | 原因 |
|---|---|---|
| **AC-S-071 步骤 4 端到端幂等（同键写两次只落库一次）** | **本轮未跑** | 属后端幂等中间件行为，本轮 `70eccf0` **未改后端**（`git show --stat` 无 `crates/`）；首轮 QA 已真机验过（第二次返回首次结果、只落库 1 条）。我**没有**复跑。 |
| **播种脚本的「主机直连 psql」分支** | **未跑** | 本机只有 `podman`、无 `psql`（`command -v psql` = 1），只跑到 `podman exec base-postgres psql` 分支。Dev 也如是说。 |
| **`429 / 账号锁定 / 账号停用` 的登录文案** | **未跑** | 需要构造限流/锁定状态（脚本开头还会清 `tenant-a` 的失败流水）。从代码看非 401 不会被 refresh 覆盖，但**未实测**。 |
| **`scripts/e2e-server.sh` 端到端（含自动播种 + 迁移）** | **部分** | 我按任务给的命令**手工**播种 + 手工起服务，**没有**让 Playwright 的 `webServer` 去跑 `e2e-server.sh`（`reuseExistingServer: true` 会直接复用我的服务）。脚本本身我逐行读过（`seed-e2e.sh` 调用在第 46-48 行），但**未实测**该脚本串起来的路径。 |
| **跨浏览器 / 暗色主题** | **未跑** | 与上一轮范围一致；本轮改动不涉及（只 Chromium）。 |
| **`Capabilities` 新增导出在消费侧的实际可用性** | **未验** | `capabilities/sdk/auth.json` 的 `exports` 增加 `AuthRequestOptions`；`--check` 通过即可，我**没**写外部消费方代码去 import 它。 |

---

## 7. 实跑数字（全部我自己跑）

### 7.1 环境

```
真实服务：cargo run -p platform-server --bin base -- serve
  SERVER_PORT=8095 · DATABASE_URL=postgres://base_app:base@127.0.0.1:5432/base_test
  BASE_STATIC_DIR=apps/shell/dist · BASE_MANIFEST_ROOT=. · BASE_LOG_LEVEL=warn
  PKCS#8 EC 私钥 /tmp/jwt-r3.pem · base64url secret /tmp/secret-r3.txt（kid=r3-kid）
  /health/live → 200 · / → 200
浏览器：Playwright + Chromium（node v26.13.1 / pnpm 12.19.0）
数据库：podman 容器 base-postgres（主机无 psql → 走 podman 分支）
```

### 7.2 验收侧

| 命令 / 脚本 | 结果 |
|---|---|
| `./scripts/test-integration.sh`（第 1 轮，**先跑以制造复现条件**） | **EXIT=0**，**550 passed / 0 failed**，29 个套件，容器 **2 → 2** |
| `./scripts/seed-e2e.sh` ×3（中间夹 E2E） | 每次 **exit 0**；第 3 遍后 `tenant-a` 仍 **3 行** |
| `DATABASE_URL_TEST=…/base_dev ./scripts/seed-e2e.sh` | **exit 2**（拒绝文案正确） |
| `DATABASE_URL_TEST=…/postgres ./scripts/seed-e2e.sh` | **exit 2** |
| `BASE_E2E_URL=http://127.0.0.1:8095 npx playwright test e2e/` | **10 passed (10.9s)**；变异/还原后复跑 **10 passed (10.6s)** |
| 我的浏览器探针 `/tmp/r3/browser-probe.mjs` | **29/29 passed** |
| 我的 SDK 探针 `/tmp/r3/sdk-probe.mjs`（打 `packages/sdk/dist/index.js`） | **32/32 passed** |

### 7.3 全量回归

| 命令 | 结果 |
|---|---|
| `./scripts/test-integration.sh`（第 2 轮） | **EXIT=0**，**550 passed / 0 failed**，29 套件，容器 **2 → 2** |
| `pnpm -r test` | **147 passed**（schema 3 · i18n 3 · hooks 17 · sdk 34 · theme 12 · auth 26 · ui 14 · shell 38） |
| `pnpm -r check` | `svelte-check found 0 errors and 0 warnings`（exit 0） |
| `pnpm -r build` | exit 0；`apps/shell/dist/assets/index-Cg7s-fXa.js` **64.15 kB** |
| `pnpm exec eslint .` | exit 0（无输出） |
| `npx prettier --check .` | `All matched files use Prettier code style!`（exit 0） |
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0**，0 告警 |
| `cargo xtask capabilities --check` | **13 个文件与源码一致**（exit 0） |
| `cargo test -p xtask --all-targets` | **49 passed / 0 failed** |

> 后端回归在**停掉 8095 服务之后**跑（TD-009）。第 2 轮集成把 `base_test` 重置，夹具再次消失 —— 这正是 D101 描述的
> 既有事实（再跑 E2E 前需重播种），**不是**回归。

### 7.4 纪律确认

```
$ sha256sum -c /tmp/r3/before.sha256
packages/auth/src/index.ts: 成功
apps/shell/src/lib/api.ts: 成功
scripts/seed-e2e.sh: 成功

$ git diff --exit-code -- apps/ packages/ e2e/ scripts/    → 空（源码树与 HEAD 逐字节一致）
$ git status --short
 M feature-meta/qa-report-recheck.md     ← 上一轮 QA 的未提交改动（我进会话前就在）
 M feature-meta/qa-report.md             ← 同上
?? feature-meta/qa-report-recheck2.md    ← 本报告
```

- 本轮**未 commit**、**未改实现**（除 3 处已逐字节还原的变异）。
- 我为验证插入/删除的数据库行（`legacy-uuid-abc` 等）已全部清理，`tenant-a` 夹具回到播种后的 3 行。

---

## 8. 结论

**VERDICT: PASS**

1. **MED-NEW-1 已真修好**（真浏览器 4 条判据全过，含仓库 E2E 未覆盖的「不清已有会话」）。
   `skipAuthRetry` 的引入面**只有** `AuthStore.login()`，业务路径未被误伤（真机 refresh+replay 实测 + M3 变异证明检查有效）。
2. **MED-NEW-2 已真修好**（严格按「集成 → 播种 ×2 → 起服务 → E2E」顺序：夹具 1 行/人、单域名邮箱、`base_dev` exit 2、E2E 10 passed）。
3. **无回归**：AC-S-067 / 069 / 070 / 071、D92 / D98 / D99 逐项复验通过；两轮全量门禁全绿。
4. **3 处变异全部抓红**，源码逐字节还原。

**合入后可跟进（不阻塞）**：
- **LOW（§4.1）**：`scripts/seed-e2e.sh` 对「同名不同 id」的既有行会 FK 失败 —— 建议 `DO UPDATE SET id = EXCLUDED.id`（需连带确认绑定行的级联清理）。
- **INFO（§4.2）**：上一轮遗留的「换租户失败登录覆盖持久化租户」，可另开小单。
