# REQ-022 终版验收清单（BA 亲跑，§9.14 阶段二）

> **代码冻结后、合并前**跑**一次**。BA 亲自跑，**数字自己 `awk` 算**，不抄 Dev 报告。
> 全量绿 **≠** 需求通过 —— 判据是 **22 条 AC**，不是测试数量。

---

## 0. 三条硬规矩（用户 2026-11-20 定，dev-guide §9.14）

1. **全量只跑一次** —— 在代码冻结后跑。不要「跑一遍看看，再跑一遍确认」。
2. **数字自己算** —— `awk` 聚合 `test result` 行，**不抄** Dev 报告里的数字
   （Dev 报告的数字是**开发期增量**口径，与终版全量口径不同）。
3. **全量绿 ≠ 需求通过** —— 必须逐条对 **22 条 AC** 给出「通过 / 不通过 / 判不了 + 理由」。

---

## 1. 开工前置（**必须先做，否则后面的结果不可复现**）

### 1.1 代码冻结

- [ ] 棒 1 / 棒 2 的**自验报告**都已落盘
- [ ] 两个报告里的「未完成项」都**已消化**（要么补完，要么明确记为技术债并有编号）
- [ ] `git status` 干净（棒 1 / 棒 2 都没提交 → **由 BA 统一提交**，先确认改动集就是预期的那批）
- [ ] **跑一遍静态检查**：两个报告里声明「改动范围外无改动」→ 用 `git status --short` 核对

### 1.2 E2E 干净库（**TD-039**：`scripts/e2e-server.sh` 不能从零建库）

清理 `base_test` 后**必须手工补 3 步**：

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"

# 1) 应用角色（base_platform_app 组角色 + base_app 登录角色 + 授权）
podman exec -i base-postgres psql -U base -d base_test < deploy/seed/02-app-role.sql

# 2) sandbox 模块自己的迁移（不在平台 migrations/ 里）
for f in modules/sandbox/migrations/*.up.sql; do
  podman exec -i base-postgres psql -U base -d base_test < "$f"
done

# 3) 租户隔离策略（manifest 的 tenantScoped 段生成 —— REQ-022 的 4 张表靠它）
cargo xtask tenant-policies
```

- [ ] 三步都跑完且**无报错**
- [ ] `platform_oauth_*` 的 4 张表**各有 1** 条 `<table>_tenant_isolation` policy
      （AC-S-155 步骤 6）
- [ ] `platform_oauth_client` **没有** policy（平台级表，D219）

### 1.3 密钥与私有密钥落盘

- [ ] `scripts/e2e-server.sh` 把 JWT 私钥写到了 `target/e2e-jwt-private.pem`
- [ ] 该文件权限 **0600**、**不在 git 里**（`git check-ignore` 或 `git status` 里看不到）
- [ ] `BASE_JWT_ISSUER` = `https://e2e.base`；`oauth.frontendBaseUrl` = `http://127.0.0.1:8080`
      → **二者不相等**（D222 的前提）

---

## 2. Rust 全量（**逐 crate，串行**）

### 2.1 成员清单 —— **现场数，不许手抄**

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-022
python3 -c "
import re
s=open('Cargo.toml').read()
m=re.search(r'members\s*=\s*\[(.*?)\]', s, re.S)
print('\n'.join(x.strip().strip('\"') for x in m.group(1).split(',') if x.strip()))
"
```

⚠️ **包名 ≠ 目录名**：`modules/sandbox` 的包名是 **`sandbox`**。
⚠️ `platform-contracts-macros` 是 **proc-macro** crate，`--all-targets` 行为与别的不同
（**不要**因为它特殊就跳过 —— 跳过就是「少跑了一个 crate」）。

### 2.2 跑法

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"

for c in <上一步数出来的包名，逐个>; do
  echo "===== $c ====="
  cargo test -p "$c" --all-targets -- --test-threads=1 --include-ignored
done 2>&1 | tee /tmp/req022-full.log
```

**硬规矩**：
- `--all-targets` **不可省**（否则 lib 单测不跑）
- **只能有一个 `--`**（`cargo test -- --x -- --y` → **0 passed 假绿**）
- `--include-ignored` 放 `--` **之后**（真实库用例靠它才真跑，否则是 skip）
- **串行** `-- --test-threads=1`（低配机 + 真实库并发会 flake）
- **不要** `cargo test --workspace`（workspace 级跑必失败）

### 2.3 数字自己算

```bash
grep -E '^test result' /tmp/req022-full.log \
  | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
```

- [ ] `failed == 0`
- [ ] **`ignored == 0`**（`--include-ignored` 之后还有 ignored，说明有 `#[ignore]` 没被解开 → 查）
- [ ] 把这个 `passed` 数字**记下来**，与 Dev 报告里的数字**对比**：
      不一致 → **要解释**（口径差异可以接受，**没解释不行**）

### 2.4 额外命令

```bash
cargo test -p platform-oauth --test contract_c5     # 真 C-5（D240）
cargo xtask capabilities --check                     # TD-044 本轮已还 → 必须绿
cargo xtask tenant-policies --check                  # 4 张表的 policy 必须齐
```

- [ ] 三条**都**绿
- [ ] `cargo xtask capabilities`（**重新生成**，不是 `--check`）之后，
      `capabilities/` 下**恰好 14 个文件**（AC-S-153 步骤 1a）
- [ ] `capabilities/permissions.json` 的 `total == 32`，且含 6 个 `oauth:*` 逐字

---

## 3. 前端全量

```bash
pnpm --filter @base/shell test
pnpm --filter @base/i18n test
pnpm --filter @base/sdk test
pnpm --filter @base/shell check      # svelte-check → 必须 0 error
pnpm --filter @base/i18n check       # tsc --noEmit
```

- [ ] 三个包**都**绿
- [ ] `svelte-check` **0 error**
- [ ] ⚠️ **REQ-011 的既有用例全过**（`router` / `session` / `sso` / `zero-code` 等）
      —— 改 `Login.svelte` / `App.svelte` 的回归（design R10）

---

## 4. E2E（Playwright）

```bash
npx playwright test e2e/
```

- [ ] **0 skipped** —— ⚠️ **skip 只允许「被测系统未就绪」**（D108）。
      **夹具缺失必须 fail**，不许 skip。
- [ ] `e2e/oauth.spec.ts` 全过
- [ ] 既有 `e2e/{audit,i18n,schema-driven,shell-auth,shell,sso}.spec.ts` **没被弄坏**
- [ ] **干净起点已验证**（§1.2 的 3 步做过，且 E2E 是从**空表**跑起来的）
- [ ] **负向对照**：D238 的「自己签一个 scope 更大的 consent ticket → 确认被拒」有断言

### 4.1 ⚠️「模拟点击」这一层由 E2E 兜底（BA 裁定：2026-11-21）

**背景**：棒 2 报告 §10.4 —— 本机**没有 DOM 测试环境**
（`jsdom` / `happy-dom` 只是可选 peer dep，**不在 pnpm store 里**，离线装不了）
→ 没法写「模拟点击按钮」的组件测试。
棒 2 的缓解：把点击决策抽成 `apps/shell/src/lib/oauth-api.ts` 的**纯函数**
（`isSafeReturnTo` / `readReturnTo` / `buildLoginUrl` / 授权页初始状态判定）并在那里断言，
组件层只断言**渲染**。

**BA 裁定：接受当前形态，不授权新增 DOM 依赖。**
理由：① 约束是**零新增第三方依赖** + 离线可复现；② 缺的「点击」这一层由**终版 E2E**
在真实浏览器里补上。

→ **代价是这个缺口必须由 E2E 兜住，所以下面这条是必查项**：

- [ ] **E2E 真的点到了那些按钮**（不是只断言 URL）：
      同意页「允许」/「拒绝」、设备码页「允许」、授权列表「撤销」、登出页的 iframe
      —— **每一条都要有一个「点了之后产生了可观测后果」的 E2E 断言**。
      若 E2E 只测了后端接口、没点按钮 → **这条判 fail**（前端点击链没人验过）。

### 4.2 棒 2 → 棒 1 的 4 条对齐（BA 已裁定，**棒 1 必须照做**）

| # | 裁定 |
|---|---|
| 1 | `GET /api/oauth/device/verify` 的查询参数名 = **`userCode`**（camelCase，**不是** `user_code`）—— D258 |
| 2 | `preview` 请求体**缺字段时不发那个键**（不发空串 / null / undefined）→ 后端按「**键不存在 = 没传**」处理 —— D258 |
| 3 | `preview` 返 400 的 `code` **是 9 个 `OAUTH_*` 之一**（不发明第 10 个码）—— D213 / D226 |
| 4 | `POST /api/oauth/logout` 的 `frontChannel` **永远是数组**（没有就 `[]`，**不是** `null`、不省略该键）—— D258 / D248 |

- [ ] **终版核对**：棒 1 的 `http.rs` 响应体**逐字**符合上表
      （尤其 `frontChannel` 永远是数组、`preview` 的可选键缺失时响应里也不出现）

---

## 5. 22 条 AC 的逐条判定（**判据，不是测试数量**）

> 每条给：**通过 / 不通过 / 判不了（+ 理由）**。
> 「判不了」必须写清**缺什么**。

| # | AC | 判据落点 | 结论 | 证据 |
|---|---|---|---|---|
| 134 | 元数据可被发现 | `GET /.well-known/oauth-authorization-server` | | |
| 135 | client 注册/改/停用/删 + 回调白名单 | `POST/GET/PATCH/DELETE /api/oauth/clients*` | | |
| 136 | secret 只存 SHA-256 base64url 摘要，不回显 | 真库查列 + 四处输出真值断言 + 静态扫描（**带阳性对照**） | | |
| 137 | 授权码 + PKCE 端到端 | E2E（`page.route` 拦截 + **query 还在**） | | |
| 138 | 强制 S256 | 三条独立路径（缺挑战 / plain / 错 verifier） | | |
| 139 | 授权码一次性 + 只存摘要 + 60s | 真库查列 + TTL 判据（**只差 `expires_at`**） | | |
| 140 | scope 三重交集 + 通配不生效 + 不回溯 | 五条路径 + **真对照**（把 `rbac:role:*` 登记进表仍 400） | | |
| 141 | Client Credentials | payload **无 `sub` 键** + 内省**无 `username` 键** | | |
| 142 | Device Code 全流程 | 申请 → 轮询 → `slow_down` → 批准 → 换令牌 | | |
| 143 | 内省 + 只能内省自己的 | 七步（含 401 `invalid_client` 与 200 `active=false` **不混判**） | | |
| 144 | 撤销后内省立即失效 | 含「撤销别人的令牌不生效」 | | |
| 145 | ES256 + JWKS 自行验签 | **测试侧独立实现**验签（不调生产代码） | | |
| 146 | 同意页 + 用户所见 = 实际授予 | 含**自己签更大 scope 的 ticket → 被拒** | | |
| 147 | 查自己的授权 + 撤销 + 令牌立即失效 | **两枚**令牌 + 跨租户空数组 | | |
| 148 | 免登 | 无同意页 / `consent_required` 透出到 query / `prompt` 缺省仍要同意 | | |
| 149 | Back-Channel Logout | mock 接收端**独立进程** + 超时/500 **不影响登出** | | |
| 150 | Front-Channel + 按应用登出 | iframe 数量/`src`/`sandbox` **逐字** | | |
| 151 | 底座业务端点默认拒绝第三方令牌 | 401 `UNAUTHENTICATED` + **阳性对照**（正常登录令牌必须成功） | | |
| 152 | 错误码 8 处登记 + 跨栈「包含」 | `TITLES.len() == 93` + 逐 locale 覆盖 | | |
| 153 | 权限点三处齐 + 启动 fail-fast + C-5 | `cargo test -p platform-oauth --test contract_c5` | | |
| 154 | 关键动作产生审计记录 | 5 个事件码 + 敏感值扫描**带阳性对照** | | |
| 155 | OAuth 数据跨租户不可见 | 4 张表 RLS + 写侧 `WITH CHECK` + client 表**不**参与 | | |

---

## 6. 交付物清单

- [ ] `feature-meta/req-022-dev-backend.md`（棒 1 自验报告）
- [ ] `feature-meta/req-022-dev-frontend.md`（棒 2 自验报告）
- [ ] `feature-meta/req-022-final-verification.md`（**本轮** BA 终版报告）
- [ ] `feature-meta/req-022-premerge-review.md`（Pre-merge 复验报告）
- [ ] `feature-meta/req-022-independent-review.md`（独立复核报告）
- [ ] 复盘：`BA/sprint/req-022-retro.md`

---

## 7. 合并与收尾

- [ ] Pre-merge 复验（BA 亲跑，重点：**注释与代码是否一致**、**越界改动**、**用例是否验证了生产代码的拷贝**）
- [ ] 修复轮（若有）
- [ ] **独立复核**（**不是** BA 自己复核 —— dev-guide 强制项：修复轮后必须有独立复核）
- [ ] BA 终验（22 条 AC 全过或明确记为技术债）
- [ ] 合并 `develop` + **回收 worktree**（`aw.py claim release REQ-022 --remove-worktree`）
- [ ] `BA/plan/roadmap.md` / `tech-debt.md` / `sprint/sprint-current.md` 同步
- [ ] 复盘落盘
