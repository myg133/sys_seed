# 修复轮 3 自验 —— REQ-011（两个遗留问题：LOW §4.1 / INFO §4.2）

- **需求**: REQ-011「前端 SDK 骨架」
- **基线提交**: `70eccf0`（`feature/REQ-011`）
- **依据**: `feature-meta/qa-report-recheck2.md` §4.1（LOW）/ §4.2（INFO）
- **日期**: 2026-11-19
- **结论**: 两个问题都修好并在**真机**（真 `base serve` + 真 Chromium）验过；
  两处修复的变异都能被新用例抓红；前端 151 passed、后端集成 550 passed、E2E 11 passed。
  **未做任何重构**，改动集中在 3 个实现点 + 4 个用例。

---

## 1. 修 1（LOW）：播种脚本在「同名但不同 id」时 FK 失败

### 1.1 选了哪个方案、为什么

**方案 C：绑定时用查出来的实际 `id`**（`platform_iam_user` 子查询 / `JOIN ... SELECT`），
**不动用户的 `id`，也不删用户行**。

排除另外两个方案的理由（均已核对迁移源码）：

| 方案 | 为什么不用 |
|---|---|
| **A. `ON CONFLICT ... DO UPDATE SET id = EXCLUDED.id`** | `platform_user_role` 的外键只有 `ON DELETE CASCADE`（`migrations/20261118000900_rbac_schema.up.sql:110-112`），**没有 `ON UPDATE CASCADE`**；而 `user_org` / `user_position` / `external_identity` 三张表的外键（`migrations/20261118000800_org_user_fk.up.sql:30-44`）**连级联子句都没有**。改 `id` 时只要这些表里有引用行就会 FK 违约；按现在的语句顺序（用户 upsert 在第 1 步、绑定清理在第 4 步），**A 会在第 1 步就失败**。 |
| **B. 先 `DELETE ... WHERE username = ...` 再插入** | 能过，但会连带删掉该用户的 `user_org` / `user_position` / `external_identity`（级联），把「同名既有账号」的组织关系静默清空；对夹具而言代价不必要。 |

方案 C 的副作用面最小：**不写 `platform_iam_user.id`**，因此没有任何级联问题；
新增的唯一写操作是「按实际 id 先删后写绑定」，与原来的语义（收敛、保证 `u-perm-none` 无角色）一致。

### 1.2 实现位置

- `scripts/seed-e2e.sh:167-185`：绑定段改成
  - `DELETE ... WHERE user_id IN (SELECT id FROM platform_iam_user WHERE tenant_id='tenant-a' AND username IN (...))`（`:170-176`）
  - `INSERT ... SELECT 'tenant-a', u.id, r.role_id FROM platform_iam_user u JOIN (VALUES ...) r ON r.username = u.username`（`:178-185`）
- `scripts/seed-e2e.sh:32-37`、`:123-125`：文件头 / 固定 id 说明补上「同名既有行沿用它的 id」与「为什么不用 `SET id = EXCLUDED.id`」。
- 第 1 步的 upsert **未改**（它本来就只修登录字段），核对段（`:199-206`）本来就按 `u.id` 关联，**无需改**。

### 1.3 真机验证（真实 `base_test`，podman 内 postgres）

**① 造同名不同 id 的行 → 播种 exit 0（修复前是 exit 3）**

```
-- 造数：tenant-a 里 u-perm-read@example.com 的 id = legacy-uuid-abc（email 也是错的）
$ ./scripts/seed-e2e.sh
播种 E2E 夹具（tenant-a，管理员连接）…
         username         |          email           | status | user_rows | role_rows | granted_permissions
--------------------------+--------------------------+--------+-----------+-----------+---------------------
 u-perm-none@example.com  | u-perm-none@example.com  | active |         1 |         0 |                   0
 u-perm-read@example.com  | u-perm-read@example.com  | active |         1 |         1 |                   1
 u-perm-write@example.com | u-perm-write@example.com | active |         1 |         1 |                   2
✓ E2E 夹具就绪
SEED_EXIT=0
```

播种后的实际行（SQL 直查）：

```
user|legacy-uuid-abc|u-perm-read@example.com|active|$argon2id     ← id 保留、密码修好、邮箱单域名
binding|legacy-uuid-abc|e2e-role-read                           ← 绑定用的是实际 id
binding|e2e-user-perm-write|e2e-role-write
counts|3|2
```

**② 播种后该用户能登录（200）**

```
$ curl -X POST .../api/iam/login -H 'x-tenant-id: tenant-a' -d '{u-perm-read...}'
LOGIN=200
{'permissions': ['rbac:role:read'], 'tokenType': 'Bearer', 'expiresIn': 900, ...}
{'id': 'legacy-uuid-abc', 'username': 'u-perm-read@example.com', 'tenantId': 'tenant-a', ...}
```

**③ 连跑两遍 → 幂等**

```
EXIT1=0  EXIT2=0 ;  users|3  bindings|2   （跑 3 遍后仍 3 行用户）
```

**④ 播完跑 E2E → 全绿**：见 §3.3（严格按「集成 → 播种 ×2 → 起服务 → E2E」顺序，`11 passed`）。

**⑤ 无既有行的正常路径未受影响**：集成测试重置库后（`tenant-a` 夹具 0 行）连播两遍，
用户 id 仍是固定的 `e2e-user-perm-read/write/none`，`exit 0`。

---

## 2. 修 2（INFO）：换租户 + 错密码覆盖已持久化的租户

### 2.1 选了哪个方案、为什么

**方案 A：`setTenant` 只更新内存、不 `persist`**，落盘交给登录成功后的 `setTokens`。
这与 `session.ts` 里**本来就写着**的接口注释一致（`:50` 「设置租户（登录请求要用它；随下一次
`setTokens` 一起持久化）」）—— 实现与文档原本是脱节的，这次把实现改成文档语义。

**额外补了一层（同一处、6 行）：登录失败时回滚内存里的租户。**
只做方案 A 时，`sessionStorage` 确实不会被污染（验收 1/2 过），但**内存里**的
`state.tenantId` 仍停留在「尝试用」的 tenant-b，而旧 `accessToken` 还在手上：
同页如果发生一次 401 → 刷新（`session.setTokens` 会 `persist`），就把 tenant-b 写回磁盘了。
所以失败分支里恢复 `previousTenant`（内存 + SDK context），把这条残余链路也堵住。
`previousTenant === undefined`（本来就没有会话）时不回滚 —— 此时 `persist()` 因缺令牌本来就不会写。

### 2.2 实现位置

- `apps/shell/src/lib/session.ts:174-180`：`setTenant` 去掉 `persist()`（只改内存），注释说明原因。
- `apps/shell/src/lib/api.ts:175-192`：`app.auth.login()`
  - `:178` 记录 `previousTenant`
  - `:183-191` 登录失败时回滚 `session.setTenant(previousTenant)` + `client.setContext({tenantId: previousTenant})` 并原样抛出
  - `:192` 成功路径不变（`current = store.state`，令牌由 auth store 的 `onLogin` → `session.setTokens` 落盘，租户随内存值一起写）

### 2.3 真机验证（`base serve` 8094 + Playwright/Chromium，探针 `/tmp/f5probe/probe.mjs`）

**16/16 断言通过**（探针自写，不走仓库 E2E）：

| # | 判据 | 实测 |
|---|---|---|
| A1 | tenant-a 登录成功、落盘 tenant-a | `{"tenantId":"tenant-a","accessToken":"eyJ0eXAi"}` |
| B1 | tenant-b + 错密码 → 401 `INVALID_CREDENTIALS` | `401` |
| **B2** | **`sessionStorage.tenantId` 仍是 `tenant-a`** | `tenantId=tenant-a` |
| B3 | accessToken 逐字节未变 | `same=true` |
| B4 | refresh 未被调用 | `refresh=0` |
| B5 | 停留在 `/login` | `/login` |
| **C1** | **reload 后恢复会话的权限点请求 = 200**（旧 bug 是跨租户头 404） | `status=200` |
| C2 | reload 后持久化租户仍是 tenant-a | `tenantId=tenant-a` |
| C3 | reload 后的权限点请求带 `x-tenant-id: tenant-a` | `{"tenant":"tenant-a"}` |
| C4 | reload 后敲 `/` 仍是已登录态（不落 /login） | `/` 且出现「已就绪」 |
| **D1** | **tenant-b + 正确密码登录成功** | `/` |
| **D2** | **`sessionStorage.tenantId` 切换为 `tenant-b`** | `tenantId=tenant-b` |
| D3 | 令牌已换新 | `rotated=true` |
| D4 | 登录请求带 `x-tenant-id: tenant-b` | `{"tenant":"tenant-b"}` |
| E1 | D99 回归：只读用户深链 `/admin/roles` → `/403` | `/403` |
| E2 | `/403` 上重载仍 `/403` | `/403` |

> 为验 D1/D2，验证脚本里临时给 `tenant-b` 播了一个只读用户（不写进仓库；集成测试重置库后已消失）。
> 仓库 E2E 不依赖它 —— 新增用例只用 `tenant-a` 夹具。

### 2.4 仓库内新增用例

- `apps/shell/test/session.test.ts:119-133`：`setTenant` 只改内存、不落盘。
- `apps/shell/test/api.test.ts:247-279`：换租户 + 错密码 → 持久化会话逐字段不变 + 等价重载仍是已登录 + **同页后续请求仍用 tenant-a 头**。
- `apps/shell/test/api.test.ts:281-311`：失败尝试后触发刷新 → **刷新请求与落盘都用 tenant-a**（这条专门盯内存回滚）。
- `apps/shell/test/api.test.ts:312-336`：换租户 + **正确**密码 → 落盘切换到 tenant-b（正常切租户不被堵死）。
- `e2e/shell-auth.spec.ts:133-165`：真浏览器版「已登录时换租户 + 错密码不覆盖会话」（旧缺陷在此变红，见 §4.4）。

---

## 3. 全量门禁（我自己复跑）

### 3.1 前端

| 命令 | 结果 |
|---|---|
| `pnpm -r check` | `svelte-check found 0 errors and 0 warnings`（exit 0） |
| `pnpm -r test` | **151 passed**（schema 3 · i18n 3 · hooks 17 · sdk 34 · theme 12 · auth 26 · ui 14 · shell **42**）→ 基线 147 + 新增 4 |
| `pnpm -r build` | exit 0；`apps/shell/dist/assets/index-BCMVSps9.js` **64.25 kB** |
| `pnpm exec eslint .` | exit 0（无输出） |
| `npx prettier --check .` | `All matched files use Prettier code style!`（首次跑抓到我新加的两个测试文件格式问题，`prettier --write` 后复查通过） |

### 3.2 后端

| 命令 | 结果 |
|---|---|
| `./scripts/test-integration.sh`（先停 8094 服务，TD-009） | **EXIT=0**，**550 passed / 0 failed**，**29 个套件**，`容器数量比对通过：2 → 2` |

### 3.3 真机端到端（严格按 QA 的复现顺序）

```bash
./scripts/test-integration.sh                       # 库被重置（tenant-a 夹具 0 行）
./scripts/seed-e2e.sh                               # ① exit 0（3 行）
./scripts/seed-e2e.sh                               # ② exit 0（仍 3 行，id 未翻倍）
# 起 base serve（8094 / base_test / apps/shell/dist）
BASE_E2E_URL=http://127.0.0.1:8094 npx playwright test e2e/
```

```
11 passed (13.5s)
  ✓ 登录页填租户 → 登录请求带 x-tenant-id，租户随会话持久化
  ✓ 错误密码 → 错误码透传 INVALID_CREDENTIALS，且不触发 refresh（MED-NEW-1）
  ✓ 租户为空 → 给出提示且不发登录请求
  ✓ 已登录时换租户 + 错密码：不覆盖已持久化会话的租户（recheck2 §4.2）   ← 新增
  ✓ 只读用户直接敲 /admin/roles → 403 页，目标接口不被请求（重载后仍 403）
  ✓ 有写权限的用户直接敲 /admin/roles → 落在该页面并加载数据
  ✓ 无任何权限的用户：登录成功但权限点为空，受保护页仍拦
  ✓ 未登录直接敲受保护 URL → 落登录页，页面不出现 403 字样
  ✓ 新开标签页不继承会话（sessionStorage 语义）
  ✓ shell 从品牌接口读取标题并渲染（同源）
  ✓ shell 渲染并可过滤菜单
```

---

## 4. 变异验证（全部逐字节还原）

还原纪律：`sha256sum` 快照 + `sha256sum -c` + `git status` 复核。

### 4.1 M2：把播种脚本的 FK 修法改回「写死 `e2e-user-perm-*`」

环境保持「同名不同 id」的既有行（`id=legacy-uuid-abc`）：

```
$ ./scripts/seed-e2e.sh
ERROR:  insert or update on table "platform_user_role" violates foreign key constraint "platform_user_role_user_fk"
DETAIL:  Key (tenant_id, user_id)=(tenant-a, e2e-user-perm-read) is not present in table "platform_iam_user".
M2_SEED_EXIT=3
```

→ **验收步骤 1 如期失败**（与 QA §4.1 的现场逐字一致）。还原后 `sha256` 与变异前一致，再跑 `exit 0`。

### 4.2 M1：把修 2 改回去（`setTenant` 改回立即 `persist()`）

```
× 会话持久化（D99） > setTenant 只改内存、不落盘（登录尝试的租户不能覆盖已持久化会话）
Test Files  1 failed | 5 passed (6)
      Tests  1 failed | 40 passed (41)
```

→ **新用例变红**。（`api.test.ts` 的行为用例此时仍绿：失败分支的回滚独立兜底 —— 见 §4.3。）

### 4.3 M1b：拆掉失败分支的内存回滚（`session.setTenant(previousTenant)` 一行）

```
× 登录尝试不污染已持久化会话的租户（recheck2 §4.2） > 失败尝试后触发刷新：刷新请求与落盘仍用已建立会话的租户（tenant-a）
+   "tenantId": "tenant-b",          ← 实际落盘成了 tenant-b
      Tests  1 failed | 41 passed (42)
```

→ 专门盯内存回滚的那条用例变红。**这条变异证明：只把 `setTenant` 改成不 persist 还不够**
（登录失败后同页若发生一次刷新，仍会把「尝试租户」写回磁盘）。

### 4.4 M1c（真机端到端）：把修 2 整体还原到 `HEAD`（`session.ts` + `api.ts`）

```
$ pnpm --filter @base/shell build      # E2E 打的是 dist
$ BASE_E2E_URL=http://127.0.0.1:8094 npx playwright test e2e/shell-auth.spec.ts -g "换租户"
  ✘  1 已登录时换租户 + 错密码：不覆盖已持久化会话的租户（recheck2 §4.2）
  1 failed
```

→ 新增的**真浏览器**用例在原始缺陷下变红；还原 + 重建后全量 `11 passed (14.0s)`。

### 4.5 还原确认

变异当时（§2.4 的最后一条用例加入**之前**）保存的快照校验：

```
$ sha256sum -c /tmp/f5/snap/before.sha256
scripts/seed-e2e.sh: 成功
apps/shell/src/lib/session.ts: 成功
apps/shell/src/lib/api.ts: 成功
apps/shell/test/session.test.ts: 成功
apps/shell/test/api.test.ts: 成功
```

此后 `api.test.ts` 又加了一条用例（§2.4 的「失败尝试后触发刷新」），所以**它现在的哈希不再等于快照**；
实现文件未再改动：

```
$ sha256sum scripts/seed-e2e.sh apps/shell/src/lib/session.ts apps/shell/src/lib/api.ts
57298b6c10cffae77785019e9882392e149aa303dcadc92209a7e89fdc8dcdb3  scripts/seed-e2e.sh
0dbe3fc1af3c8c7a310d787744185ba9407c0b5a804bedad34707af57d037920  apps/shell/src/lib/session.ts
7dad5f1ccb2001e9bca3d03a72a1e1666cad3d4a0db21899e7b559844f9f9210  apps/shell/src/lib/api.ts
```

`git diff` 里这三处只有 §1.2 / §2.2 列出的改动（无顺手重构）。

---

## 5. 副作用与遗留（明确写出来）

1. **修 1 没有改动用户 `id`**：同名既有账号的 `id` 会被**保留**（例如 `legacy-uuid-abc`），
   脚本不再保证夹具用户 id 是固定的 `e2e-user-perm-*`。所有下游（绑定、核对）已改为按实际 id 查，
   但若将来有人写死 `e2e-user-perm-*` 去查库，仍可能踩空 —— 文件头已写明。
2. **修 1 未覆盖的极端情形**：若 `platform_iam_user` 里已存在 `id='e2e-user-perm-read'` 但
   `username` 是**别的**值（即固定 id 被别的行占用），`INSERT` 会撞主键，而 `ON CONFLICT` 只声明了
   `(tenant_id, username)` → 脚本仍会失败。这条与 QA 报的场景无关（需要有人手动占用固定 id），
   未改；如需兜底可改成「按 `(tenant_id, username)` 先查后插入」。
3. **修 2 的语义变化**：`setTenant` 不再落盘，属于**接口契约**的收紧（注释本来就这么写）。
   全仓生产代码只有 `api.ts:179` 一处调用（`grep -rn setTenant`），且登录成功路径仍会由
   `setTokens` 落盘，因此「正常切换租户」不受影响（真机 D1/D2 已验）。
4. **会话首次建立时的持久化时机**：现在「先 `setTenant` → 再 `setTokens`」，若登录成功后 auth store
   因某种原因**没有**调 `onLogin`，则租户不会落盘。`createAuthStore.login()` 必然走 `onLogin`
   （`packages/auth/src/index.ts:287-296`），单测与真机都已覆盖；但这算一处**新的隐性依赖**，记录备查。
5. **未做**：`scripts/e2e-server.sh` 串起来的 `webServer` 路径仍未实测（与 QA §6 一致）；
   主机直连 psql 分支仍未跑（本机无 `psql`，走 `podman exec`）。

---

## 6. 涉及文件

**实现**
- `scripts/seed-e2e.sh`
- `apps/shell/src/lib/session.ts`
- `apps/shell/src/lib/api.ts`

**用例**
- `apps/shell/test/session.test.ts`
- `apps/shell/test/api.test.ts`
- `e2e/shell-auth.spec.ts`
