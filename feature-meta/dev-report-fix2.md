# Dev 自验报告 —— REQ-011 修复轮 2（MED-NEW-1 / MED-NEW-2）

- **日期**: 2026-11-19
- **分支**: `feature/REQ-011`
- **依据**: `BA/demands/REQ-011-frontend-sdk/acceptance.md` rev 4（AC-S-070 补充）、
  `qa-report-recheck.md` §2.1 / §2.2、`D100` / `D101`
- **改动范围**: `packages/sdk`、`packages/auth`（只加认证端点跳过刷新的开关）、
  `apps/shell/test`（回归用例）、`e2e/shell-auth.spec.ts`（真浏览器用例）、
  `scripts/seed-e2e.sh`（新增）、`scripts/e2e-server.sh`（起服务前播种）、
  `capabilities/`（`cargo xtask capabilities` 重生成，公开 API 多了一个导出类型）
- **没改**: 后端 Rust 源码（0 行）、`Login.svelte`（文案分派本来就是对的，错的是送进来的 `code`）

---

## 1. MED-NEW-1：登录失败显示 `TOKEN_REVOKED`

### 1.1 修法

SDK 的 `RequestOptions` 增加 **`skipAuthRetry`**（承担 `D100` 的「认证端点不参与自动刷新」）：

| 行为 | 修复前 | 修复后（`skipAuthRetry: true`） |
|---|---|---|
| 401 后自动刷新 | 会（登录 401 也刷） | **不会** |
| 刷新失败时上报 `onUnauthorized` | 会（清会话） | **不会** |
| 抛出的错误码 | 刷新失败的 `TOKEN_REVOKED` | **后端原始码**（`INVALID_CREDENTIALS`） |

`@base/auth` 的 `AuthStore.login()` 发登录请求时**固定**带上 `{ skipAuthRetry: true }` ——
这条路径是宿主应用唯一的登录入口，不需要调用方「记得」什么。

> 为什么不是「按路径白名单」：`loginPath` 是可配置项，硬编码路径会在自定义路径时静默失效；
> 开关随请求走，语义明确。刷新请求自身的「不递归刷新」仍由 `client.refresh()` 的
> scoped 语义保证（`packages/sdk/src/index.ts:340-370`），两者不互相替代。

### 1.2 实现位置

| 落点 | 内容 |
|---|---|
| `packages/sdk/src/index.ts:83-97` | `RequestOptions.skipAuthRetry` 的声明与理由（含 ①②③） |
| `packages/sdk/src/index.ts:330-337` | `canRefresh` 增加 `requestOptions.skipAuthRetry !== true` |
| `packages/sdk/src/index.ts:353-358` | 认证端点的 401 **不再**触发 `onUnauthorized`（不清已有会话） |
| `packages/auth/src/index.ts:199-207` | `AuthRequestOptions` + `AuthApiClient.post` 的第三个参数（与 SDK 结构兼容） |
| `packages/auth/src/index.ts:287-296` | `login()` 传 `{ skipAuthRetry: true }` |
| `apps/shell/src/pages/Login.svelte:61-82` | 未改：按 `code` 分派文案本来就是对的，现在拿到的是正确的码 |

### 1.3 新增用例

| 类型 | 位置 | 断言 |
|---|---|---|
| SDK 单测 | `packages/sdk/test/sdk.test.ts:360-495`（4 条新用例） | ① 错密码 401：`code === 'INVALID_CREDENTIALS'`、refresh 回调 **0** 次、`onUnauthorized` **0** 次、只发 1 次请求；② 同一路径**不带**开关时仍会刷新（证明判据是开关不是路径）；③ 业务端点 401 仍刷新+重放（AC-S-070 不回退）；④ `client.refresh()` 自身不递归刷新 |
| `@base/auth` 单测 | `packages/auth/test/auth.test.ts:187-196` | `login()` 调 `post` 时第三参**精确等于** `{ skipAuthRetry: true }` |
| shell 单测（真 SDK） | `apps/shell/test/api.test.ts:213-239` | 错密码：`error.code === 'INVALID_CREDENTIALS'`、请求序列**恰好** `[/api/iam/login]`（无 refresh）、已登录用户的旧会话**没被清** |
| 真浏览器 | `e2e/shell-auth.spec.ts:81-114` | 响应码 401 且 `body.code === 'INVALID_CREDENTIALS'`；页面出现「用户名或密码不正确」且 **不含** `TOKEN_REVOKED`；`/api/iam/refresh` 请求数 **0** |

---

## 2. MED-NEW-2：E2E 夹具不可复现

### 2.1 修法

新增 **`scripts/seed-e2e.sh`**：E2E 需要的三个账号由它**逐条列明**并创建，
不再依赖 `crates/platform-rbac/tests/me_permissions.rs` 的副作用。

| 要求 | 实现 |
|---|---|
| 幂等 | 用户 / 角色 / 授权 / 绑定全部 `ON CONFLICT` 收敛；角色授权与用户绑定**先清后写**（保证 `u-perm-none` 一定没有角色）；连跑两遍用户数不变（实测 3 → 3） |
| 显式 | 脚本头部的表逐条列出「用户 → 权限点 → 用途」，SQL 里也是三条固定 id（`e2e-user-perm-*`） |
| 只写 `base_test` | 白名单闸门：库名**恰好** `base_test` 才继续；`base_dev` 单独给出拒绝文案；其它库名一律拒绝并非 0 退出（`scripts/seed-e2e.sh:40-50`） |
| 能真的登录 | 邮箱**单域名**（`u-perm-read@example.com`，不是 `...@example.com@example.com`）；密码 `Tr0ub4dor&3xKq` 的 Argon2id PHC 串写在脚本里（`scripts/seed-e2e.sh:99-105`），登录实测 **200** |
| 文档 | `e2e/shell-auth.spec.ts:9-20` 写明顺序（集成测试 → 播种 → 起服务 → E2E）；`scripts/e2e-server.sh` 起服务前自动跑播种（webServer 路径也不会忘记） |

另外两处「库脏了也照样能跑」的必要处理（都在脚本里、都只作用 `tenant-a`）：

- 用户已存在时**修好**登录必需字段（邮箱单域名、密码、`status='active'`、
  `failed_attempts=0`、`locked_until=NULL`）—— 只「跳过」的话，旧的坏行仍然登不进去；
- 清掉 `tenant-a` 的 `platform_iam_login_attempt`（登录限流按「租户 + IP」滑动窗口计数，
  上一轮失败的流水会把播种后的登录顶成 401）。

脚本自身带**结果自证**（`scripts/seed-e2e.sh:186-225`）：打印三个用户的
用户行数 / 角色数 / 权限点数，并强断言「合计 3 行、每个用户名恰好 1 行」，不符即非 0 退出。

### 2.2 实现位置

| 落点 | 内容 |
|---|---|
| `scripts/seed-e2e.sh:40-50` | `base_test` 白名单（`base_dev` 单独拒绝） |
| `scripts/seed-e2e.sh:52-77` | SQL 执行器（主机有 `psql` 直连，否则 `podman exec`，与 `test-integration.sh` 同一套约定） |
| `scripts/seed-e2e.sh:79-97` | 没有平台表时先用管理员连接跑 `base migrate up`（全新库也能一次到位） |
| `scripts/seed-e2e.sh:107-184` | 账号 / 角色 / 授权 / 绑定 / epoch / 失败流水（单事务） |
| `scripts/seed-e2e.sh:186-225` | 结果自证 + 强断言 |
| `scripts/e2e-server.sh:46-48` | 起服务前调用播种 |
| `e2e/shell-auth.spec.ts:9-20` | 前置条件与顺序写进 spec 头 |

---

## 3. 真机验证（真实输出）

### 3.1 复现路径（严格按复核 QA 的顺序）

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-011
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"

# 1) 先跑集成测试（会把库搞脏 —— 正是 MED-NEW-2 的复现条件）
./scripts/test-integration.sh      → EXIT=0，550 passed / 0 failed，容器 2 → 2

# 2) 播种（刚被重置/污染过的库）
./scripts/seed-e2e.sh
   u-perm-none@example.com  | … | active | 1 | 0 | 0
   u-perm-read@example.com  | … | active | 1 | 1 | 1
   u-perm-write@example.com | … | active | 1 | 1 | 2
   ✓ E2E 夹具就绪

# 3) 起服务（8096，PKCS#8 EC 私钥 + base64url secret，DATABASE_URL=base_app@base_test）
#    /health/live → 200

# 4) E2E（BASE_E2E_URL=http://127.0.0.1:8096 npx playwright test e2e/）
   10 passed (11.7s)     ← 含新增的 MED-NEW-1 用例；MED-NEW-2 的验收
```

> 这 10 条是**在刚跑过集成测试、再播种**的库上取得的 —— 旧版本在同一位置实测 5 failed。

### 3.2 MED-NEW-1 的三条判据（真浏览器，`e2e/shell-auth.spec.ts:81`）

| 判据 | 实测 |
|---|---|
| `error.code === 'INVALID_CREDENTIALS'`（精确） | 通过：响应 401，`body.code === 'INVALID_CREDENTIALS'` |
| `/api/iam/refresh` 调用次数 `=== 0` | 通过：`page.on('request')` 全程拦截，计数 **0**（即抓包层面的一次都没有） |
| 页面文案「用户名或密码不正确」，不含 `TOKEN_REVOKED` | 通过：`getByText('用户名或密码不正确')` 可见；`body` 全文 **不含** `TOKEN_REVOKED` |

服务端侧证（curl，同一台机器，播种后）：

```
POST /api/iam/login {u-perm-read@example.com, 正确密码}
  → 200 {"permissions":["rbac:role:read"], …}

POST /api/iam/login {u-perm-read@example.com, wrong-password-xyz}
  → 401 {"title":"凭证无效","code":"INVALID_CREDENTIALS", …}
```

### 3.3 MED-NEW-2 的判据

| 判据 | 实测 |
|---|---|
| 跑过 `test-integration.sh` 的库上播种后 E2E 全绿 | **10 passed / 0 failed**（11.7s） |
| 连跑两遍播种不报错、用户数不翻倍 | 两遍均 `✓`；`tenant-a` 下三个用户名各 **恰好 1 行**（第三遍后仍为 3 行） |
| `base_dev` 保护 | `DATABASE_URL_TEST=…/base_dev ./scripts/seed-e2e.sh` → `✗ 拒绝播种 base_dev …`，exit **2** |

---

## 4. 变异验证（两条，均逐字节还原）

### 4.1 M1：去掉「认证端点跳过刷新」

- 变异点：`packages/auth/src/index.ts` 的 `login()` 去掉第三参 `{ skipAuthRetry: true }`
  → `pnpm --filter @base/auth build && pnpm --filter @base/shell build`（E2E 打的是 dist）。
- **E2E：`1 failed / 9 passed`** —— 红的是
  `e2e/shell-auth.spec.ts:81 错误密码 → 错误码透传…`，
  失败在 `getByText('用户名或密码不正确')`（页面显示的是 `TOKEN_REVOKED` 分支）。
- **单测也红**：
  - `packages/auth`：`FAIL … 登录请求带 skipAuthRetry`（1 failed / 25 passed）
  - `apps/shell`：`FAIL … 错密码登录（MED-NEW-1 / D100）`（1 failed / 37 passed）
- 还原：`cp` 回备份 → `sha256sum -c` 通过 → 重建 → E2E **10 passed**。

### 4.2 M2：删掉播种脚本里的一个用户（`u-perm-write`）

- 变异点：`scripts/seed-e2e.sh` 删掉 `u-perm-write@example.com` 的账号行与绑定行
  （同步把自证断言改成 2 个用户，让脚本本身仍能成功退出 —— 否则只是脚本红，证明不了 E2E 依赖它）。
- 为模拟「刚重置的库」，先 `DELETE` 掉三个夹具用户（模拟干净状态），再跑变异脚本。
- **E2E：`2 failed / 8 passed`** —— 红的是两条真正使用写权限用户的用例：
  - `有写权限的用户直接敲 /admin/roles → 落在该页面并加载数据`
  - `新开标签页不继承会话（sessionStorage 语义）`
- 还原：`cp` 回备份 → **`sha256sum -c` 通过（逐字节一致）** → 再清夹具 → 重跑脚本 → E2E **10 passed**。

> 说明：M2 的第一次演示用的是加「显式无 psql/podman 守卫」之前的版本；为让「验证过的脚本 == 提交的脚本」，
> 我用**最终版本**重做了 M2（结果同上：2 failed → 还原 → 10 passed）。

---

## 5. 回归（全部实跑）

| 命令 | 结果 |
|---|---|
| `pnpm -r check` | 0 errors / 0 warnings（`svelte-check found 0 errors and 0 warnings`） |
| `pnpm -r test` | **147 passed**（schema 3 · i18n 3 · hooks 17 · sdk 34 · theme 12 · auth 26 · ui 14 · shell 38） |
| `pnpm -r build` | exit 0（shell 132 modules → `dist/assets/index-Cg7s-fXa.js` 64.15 kB） |
| `pnpm exec eslint .` | exit 0（无输出） |
| `npx prettier --check .` | `All matched files use Prettier code style!` |
| `./scripts/test-integration.sh` | **EXIT=0**，**550 passed / 0 failed**，容器 2 → 2 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0，0 告警 |
| `cargo xtask capabilities --check` | 通过：**13 个文件与源码一致**（生成后复检） |
| `cargo test -p xtask --all-targets` | **49 passed** |

> 后端回归在**停掉 8096 服务之后**跑（TD-009：`iam_real_db.rs` 的 Argon2 时序用例对 CPU 负载敏感）。

> `capabilities/` 有改动是**预期内**的：`@base/auth` 多导出了一个 `AuthRequestOptions` 类型
> （`capabilities/sdk/auth.json` 的 `exports` 17 → 18），由 `cargo xtask capabilities` 重新生成。

---

## 6. 遇到的问题 / 局限

1. **本机没有 `psql`**：`scripts/seed-e2e.sh` 与 `test-integration.sh` 一样走
   `podman exec base-postgres psql` 分支（该分支实测通过）；**主机直连 `psql` 那条分支在本机没被跑到**
   —— 代码路径与 `test-integration.sh` 完全一致，但未实测。
2. **`psql -v phc=…` 的变量插值**只在 podman 分支验证过；直连分支用的是同一个 `psql` 命令形态，
   风险低但未实测。
3. **`tenant-a` 的失败流水会被播种脚本清空** —— 这是有意的（否则限流计数会让播种后的登录仍 401），
   但它意味着播种脚本会删测试库里的登录审计流水（只限 `tenant-a`，只限 `base_test`）。
   若后续有「登录审计」E2E 依赖这些流水，需要调整。
4. **夹具 id 是固定值**（`e2e-user-perm-*` / `e2e-role-*`）：若别的测试也用同样的 id 会撞；
   目前 `grep` 无命中，且 id 带 `e2e-` 前缀可辨识。
5. **`feature-meta/qa-report.md` / `qa-report-recheck.md` 的改动是 QA 上一轮留下的**（我进会话前就在工作区），
   本次提交**不包含**这两个文件。
6. 未做：暗色主题、跨浏览器（与上一轮范围一致，本次改动不涉及）。
