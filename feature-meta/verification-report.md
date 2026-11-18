# REQ-007 验证报告

- **需求**: REQ-007 认证与账号体系
- **分支**: `feature/REQ-007` @ `e755e5c` + 本轮修复提交（实现提交 `c66605f` + 文档提交 `6ad3f95` + 修复轮提交见 §0.1）
- **基线**: `develop` @ `91d8e09`
- **设计依据**: `BA/demands/REQ-007-auth-account/design-summary.md`（D1–D11 照做）
- **契约**: api-conventions v1（C3 / C6 / C8 / C13）、extension-points v1
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: 第二轮（**QA Pre-merge 判定 FAIL → 修复轮**；修复内容见 §0.1）

---

## 0. 一页速览

| 项 | 结果 |
|----|------|
| 分支 / 提交 | `feature/REQ-007` @ `e755e5c`（第一轮）→ **修复轮**（§0.1） |
| 改动 | 第一轮：**29 文件 / +7806 -191 行**；**新建 crate `crates/platform-iam`**。修复轮：**7 文件 / +约 700 -约 100 行**（见 §0.1） |
| 新增第三方依赖 | 第一轮：`argon2` / `jsonwebtoken` / `totp-rs` / `chacha20poly1305` / `p256` / `base64` / `sha2` / `rand`。**修复轮：零新增依赖**（`git diff -- '*Cargo.toml' '*Cargo.lock'` 为空） |
| **ring / aws-lc-rs** | **未引入** —— `cargo tree --workspace --all-features` 里 `ring v…` / `aws-lc-rs v…` **0 命中**（唯一含 “ring” 字样的是 `stringprep`，Unicode 库，非 ring 加密库） |
| 依赖树 | **第三方 232 个 crate**（第一轮实测 240 是总数口径）—— **修复轮未变**（无依赖变更）；QA 建议 AC-S-020 重定基线到 **235**，由 BA 记账（LOW 7 不改代码） |
| Rust 测试 | **225 passed / 0 failed**（非 ignored 口径，与第一轮**持平**）；**带 `--include-ignored`：240 passed / 0 failed**（真实库层 **16/16**，含护栏） |
| 前端测试 | **23/23**（`pnpm -r test`） |
| E2E | **1/1**（`bun run test:e2e`，chromium） |
| lint | `cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** |
| xtask 门禁 | `capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅ |
| ★ 防账号枚举 | ✅ **修复后**：存在账号与不存在账号各 6 次错密码 → **全部 401 `INVALID_CREDENTIALS`，逐次序列完全一致** |
| ★ 锁定期不续期 | ✅ 锁定期内错密码 → `locked_until` **不前移**、计数不增；到期后第一次错密码 → 计数**从头开始**（= 1） |
| ★ 三路径耗时 | ✅ 错密码(未锁) **563.1ms** / 错密码(已锁) **569.0ms** / 账号不存在 **543.2ms** → 最差比值 **1.048**（修复前锁定路径是 8.1ms → **75 倍**） |
| ★ 改密保留当前设备 | ✅ 其它设备 refresh → **401 `TOKEN_REVOKED`**；**当前设备 refresh → 200** |
| ★ IP 限流 | ✅ **跨租户同 IP 也受限**（429 `LOGIN_RATE_LIMITED`）；**成功登录不计入失败窗口** |
| ★ 防枚举用例能抓回归 | ✅ **两次变异验证**：① 锁定判定挪回密码校验前 → 3 条用例变红；② 过度修正（不再返回 `ACCOUNT_LOCKED`）→ 1 条用例变红 |
| **未完成 / 未验证** | 见 §7（如实登记） |

---

## 0.1 ★ 修复轮（QA Pre-merge FAIL → 修复）

QA 报告：`feature-meta/qa-report.md`（判定 **FAIL**：2 个 HIGH + 2 MEDIUM + 3 LOW）。
**核心实现全部复验为真**（轮换/重放、TOTP 防重放、租户隔离、JWKS、错误体）——
问题集中在**账号锁定逻辑的判定顺序**。本轮修 HIGH×2 + MEDIUM×2 + LOW×2；
**LOW 7（依赖预算）不改代码**（由 BA 重定基线）。

### 0.1.1 逐条修复对照

| # | 级别 | 问题 | 修法 | 位置 |
|---|------|------|------|------|
| 1 | **HIGH** | `ACCOUNT_LOCKED` 是账号枚举预言机（6 请求/候选即可枚举） | **锁定判定挪到密码校验之后**：先查账号 → 校验密码 → 密码对了才判锁定。失败分支统一 `INVALID_CREDENTIALS` + 401 | `crates/platform-iam/src/service.rs`（`login`）、`error.rs`（模块注释） |
| 2 | **HIGH** | 账号锁可被无限续期 → 永久登录 DoS | `next_lock_state()`：**已在锁定期 → 不续期、计数不增**；**锁定已到期 → 计数从头开始**（= 1） | `service.rs`（`next_lock_state` / `record_failed_login`） |
| 3 | **MEDIUM** | 锁定路径跳过 Argon2 → 75× 时序差 | 随 #1 一并解决（锁定路径也走 Argon2 校验） | 同上 |
| 4 | **MEDIUM** | 防枚举用例**结构上看不见** #1（状态码只断言第一次 + `median()` 取上中位数丢掉整簇样本） | 用例改为**逐轮断言状态码** + **真中位数**（偶数长度取中间两个的平均值）+ **拆成两条独立用例** | `crates/platform-iam/tests/iam_real_db.rs` |
| 5 | **LOW** | 改密把「当前设备」的 refresh family 也作废了 | `revoke_all_refresh_tokens` → `revoke_other_refresh_tokens`（**除当前 family 外**全作废） | `service.rs`（`apply_new_password`）、`store.rs` |
| 6 | **LOW** | IP 限流按租户计 + 把成功登录也计数 | 限流**按 IP 全局（跨租户）** + **只算失败尝试**（新增全局失败计数表） | `service.rs`（`check_ip_rate_limit`）、`store.rs`、`migrations/20261118000500_iam_login_rate.{up,down}.sql` |

### 0.1.2 ★ 枚举面复验（QA HIGH 1）—— 真实 HTTP 服务实测

> 环境：`./target/debug/base serve`（**应用角色 `base_app`**，连 `base_test`），
> 生产默认 Argon2 参数（m=19456 / t=2 / p=1）。

```
注册 victim -> 201
  存在账号 第1次: status=401 code=INVALID_CREDENTIALS (618ms)
  不存在账号 第1次: status=401 code=INVALID_CREDENTIALS (615ms)
  存在账号 第2次: status=401 code=INVALID_CREDENTIALS (603ms)
  不存在账号 第2次: status=401 code=INVALID_CREDENTIALS (696ms)
  存在账号 第3次: status=401 code=INVALID_CREDENTIALS (609ms)
  不存在账号 第3次: status=401 code=INVALID_CREDENTIALS (565ms)
  存在账号 第4次: status=401 code=INVALID_CREDENTIALS (633ms)
  不存在账号 第4次: status=401 code=INVALID_CREDENTIALS (593ms)
  存在账号 第5次: status=401 code=INVALID_CREDENTIALS (620ms)
  不存在账号 第5次: status=401 code=INVALID_CREDENTIALS (599ms)
  存在账号 第6次: status=401 code=INVALID_CREDENTIALS (590ms)
  不存在账号 第6次: status=401 code=INVALID_CREDENTIALS (620ms)

  存在账号序列: [(401, 'INVALID_CREDENTIALS'), (401, 'INVALID_CREDENTIALS'), (401, 'INVALID_CREDENTIALS'),
                (401, 'INVALID_CREDENTIALS'), (401, 'INVALID_CREDENTIALS'), (401, 'INVALID_CREDENTIALS')]
  不存在账号序列: [(401, 'INVALID_CREDENTIALS'), (401, 'INVALID_CREDENTIALS'), (401, 'INVALID_CREDENTIALS'),
                (401, 'INVALID_CREDENTIALS'), (401, 'INVALID_CREDENTIALS'), (401, 'INVALID_CREDENTIALS')]
  状态码集合 存在=[401] 不存在=[401]
  错误码集合 存在=['INVALID_CREDENTIALS'] 不存在=['INVALID_CREDENTIALS']
  >>> 不可区分 = True
```

**逐次输出贴在上面**：第 6 次（越过锁定阈值）**仍然是** `401 INVALID_CREDENTIALS`，
与「账号不存在」**逐次完全一致** → **枚举面消失**。

### 0.1.3 ★ 锁定期内用正确密码 → `ACCOUNT_LOCKED`（两个约束同时成立）

```
  锁定期内 + 正确密码 -> status=403 code=ACCOUNT_LOCKED (605ms) detail=the account is temporarily locked
```

| 约束 | 如何同时满足 |
|------|-------------|
| 攻击者（不知道密码）**永远**只拿到 `INVALID_CREDENTIALS` | 锁定判定排在密码校验**之后** → 攻击者走不到那一支 |
| **知道密码**的被锁用户仍然拿到 `ACCOUNT_LOCKED` | 密码校验通过 → 才判锁定 → 403 + 明确文案 |

### 0.1.4 ★ 三条失败路径耗时对齐（QA MEDIUM 3）

```
  错密码(未锁)   = 563.1 ms
  错密码(已锁)   = 569.0 ms
  账号不存在     = 543.2 ms
  >>> 最差比值 = 1.048（修复前：锁定路径 8.1ms vs 普通失败 606.9ms = 75 倍）
```

### 0.1.5 ★ 锁不续期 + 到期后计数从头开始（QA HIGH 2）

```
  锁定后（5 次错密码）: failed_attempts|locked_until = 5|2026-11-18 08:12:16.802737+00
  锁定期内再错一次:      failed_attempts|locked_until = 5|2026-11-18 08:12:16.802737+00   ← 不前移、不增
  到期后第一次错密码:    failed_attempts|locked_until = 1|NULL                              ← 从头开始
```

### 0.1.6 ★ 改密保留当前设备（QA LOW 5）

```
  改密 -> status=204
  其它设备 refresh -> status=401 code=TOKEN_REVOKED
  当前设备 refresh -> status=200
```

### 0.1.7 ★ IP 限流：跨租户共享预算 + 成功登录不计入（QA LOW 6）

```
  租户 A 前 21 次: [(19, 401, 'INVALID_CREDENTIALS'), (20, 401, 'INVALID_CREDENTIALS'), (21, 429, 'LOGIN_RATE_LIMITED')]
  第 21 次 -> status=429 code=LOGIN_RATE_LIMITED（阈值 20）
  **换租户 B** 同 IP -> status=429 code=LOGIN_RATE_LIMITED
  另一个 IP 连打 20 次**成功**登录 -> 全部 200
  该 IP 随后错一次密码 -> status=401 code=INVALID_CREDENTIALS（成功登录不得计入失败窗口）
```

**实现说明**：限流预算必须**跨租户共享**，而 RLS 下的租户作用域表**只能看见自己租的行** →
新增一张**不带租户维度**的全局计数表 `platform_iam_login_rate`：

| 表 | 内容 | 租户维度 | RLS |
|----|------|---------|-----|
| `platform_iam_login_attempt` | 审计流水（含 `username` —— **租户内**数据） | 有 `tenant_id` | **开** |
| `platform_iam_login_rate`（**新增**） | **失败**计数（只有 **IP 的 SHA-256 摘要**与时间戳） | **没有**（不含任何租户数据） | **不开** |

- 全局可读的表里**不存明文 IP**（存摘要）。
- 只记**失败**尝试 → 成功登录**根本不进窗口**（不是「记了再排除」，是压根不记）。
- `TenantQuery` 强制每条语句带 `{tenant}` 占位符 → 插入用 `INSERT ... SELECT ... WHERE {tenant} IS NOT NULL`、
  计数用 `... AND {tenant} IS NOT NULL`（恒真），**查询本身不受租户维度影响**。

### 0.1.8 ★ 防枚举用例能抓回归（两次变异验证）

| 变异 | 期望变红的用例 | 实测 |
|------|--------------|------|
| **① 锁定判定挪回密码校验之前** | 「连续 6 次错密码 → 第 6 次仍是 `INVALID_CREDENTIALS`」 | ✅ **3 条变红**：`six_wrong_passwords_never_reveal_that_the_account_exists`、`repeated_failures_lock_the_account`、`the_three_failure_paths_have_similar_latency`（实测报错 `left: 403, right: 401`，`code=ACCOUNT_LOCKED`） |
| **② 过度修正**：锁定期内也不再返回 `ACCOUNT_LOCKED` | 「锁定期内用正确密码 → `ACCOUNT_LOCKED`」 | ✅ **1 条变红**：`a_locked_account_with_the_right_password_is_told_it_is_locked`（实测报错 `left: 401, right: 403`） |

> 两次变异**都已还原**：`service.rs` 的 sha256 与变异前**逐字节一致**
> （`0052eac5c64926ef6664f63f821cbff3458123eb2d613a7894e3ed58868e1031`）。
>
> 说明：两条用例守的是**两个方向**的约束 —— ① 守「攻击者永远拿不到 `ACCOUNT_LOCKED`」，
> ② 守「知道密码的人仍然拿得到 `ACCOUNT_LOCKED`」。修复 #1 只会破坏 ① 方向的约束，
> 所以 ② 需要用「过度修正」这一变异来证明它承重。

### 0.1.9 修复轮新增 / 改写的用例（真实库层）

| 用例 | 守什么 |
|------|--------|
| `six_wrong_passwords_never_reveal_that_the_account_exists` | **新增** —— 6 次错密码逐次断言，两条序列**完全相等** |
| `a_locked_account_with_the_right_password_is_told_it_is_locked` | **新增** —— 锁定期内用正确密码 → `ACCOUNT_LOCKED` |
| `a_lock_is_never_extended_and_the_failure_count_restarts_after_it_expires` | **新增** —— 锁不续期 + 到期后计数从头开始（直查库断言 `failed_attempts` / `locked_until`） |
| `the_three_failure_paths_have_similar_latency` | **新增** —— 三条失败路径耗时对齐（真中位数 + 最差比值） |
| `the_ip_rate_limit_is_shared_across_tenants_and_only_counts_failures` | **新增** —— 限流跨租户共享 + 成功登录不计入 |
| `login_failure_does_not_reveal_whether_the_account_exists` | **改写** —— 逐轮断言状态码 + 真中位数 |
| `repeated_failures_lock_the_account` | **改写** —— 第 6 次错密码**仍**是 `INVALID_CREDENTIALS` |
| `changing_the_password_revokes_the_other_families_and_rejects_history` | **改写** —— 断言当前设备 refresh **仍可用** |

### 0.1.10 修复轮回归

| 项 | 结果 |
|----|------|
| `cargo check --workspace --all-targets` | ✅ 通过 |
| `cargo test --workspace -- --test-threads=2` | ✅ **225 passed / 0 failed**（与第一轮**持平**，只增不减） |
| `cargo test --workspace --tests -- --test-threads=2 --include-ignored` | ✅ **240 passed / 0 failed**（真实库层 **16/16**） |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ **0 告警** |
| `cargo fmt --all --check` | ✅ 通过 |
| `scripts/test-integration.sh` | ✅ **exit 0**，**容器数量 2 → 2** |
| `cargo xtask capabilities --check` / `check-deps` / `check-modules` / `tenant-policies --check` | ✅ 全过 |
| `pnpm -r test` | ✅ **23/23** |
| `bun run test:e2e` | ✅ **1/1** |
| **依赖树** | **232 个第三方 crate（未变）**；QA 建议 AC-S-020 重定基线到 **235**（BA 记账） |

---

## 1. 交付形态：账号体系自持 + 第一个真实 HTTP 业务路由

### 1.1 身份模型（ADR-0001 A3）

底座是身份的唯一权威源，SSO 只是「另一种登录方式」。本期落地**本地账号**：

- `platform_iam_user` 表：租户内唯一的 `username` / `email` + Argon2id 的 PHC 串
- **外部 IdP 的 user id 不做主键**（本期不做 SSO；`external_identity` 属 REQ-013）

### 1.2 混合令牌

| 令牌 | 形态 | TTL | 存储 | 撤销 |
|---|---|---|---|---|
| access | **JWT**（ES256 / P-256，带 `kid`） | **15 分钟** | 不存（自包含） | 靠短 TTL 兜底 |
| refresh | **不透明随机串**（32 字节随机 → base64url） | **30 天** | DB **只存 SHA-256 摘要** | **可立即撤销** |
| family | 一组 refresh 的归属 | — | `platform_iam_refresh_token.family_id` | 检测到重放 → **整个 family 作废** |

**为什么混合**：全 JWT → 无法撤销（「设备会话管理」「登出全端失效」两条需求直接落空）；
全不透明 → 每次请求查库。混合兼顾性能与控制力（设计摘要 §2）。

### 1.3 HTTP 路由清单（本期新增：平台第一个真实业务路由）

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/api/iam/register` | 注册（策略可配；租户内唯一） |
| POST | `/api/iam/login` | 登录 → `{ accessToken, refreshToken, expiresIn, user, permissions }` |
| POST | `/api/iam/refresh` | 刷新 → **轮换**后的新令牌对 |
| POST | `/api/iam/logout` | 撤销当前会话 |
| POST | `/api/iam/logout-all` | 撤销该用户全部会话 |
| GET | `/api/iam/sessions` | 设备会话列表（设备 / IP / 最近活跃） |
| DELETE | `/api/iam/sessions/{id}` | 踢下线（作废该会话的 family） |
| POST | `/api/iam/password/forgot` | 发起重置（**响应不泄露邮箱是否存在**） |
| POST | `/api/iam/password/reset` | 用重置令牌改密 |
| POST | `/api/iam/password/change` | 已认证用户改密（踢出其它设备） |
| POST | `/api/iam/mfa/enroll` | 绑定 TOTP（返回 secret + otpauth URI + 10 个恢复码） |
| POST | `/api/iam/mfa/verify` | 校验并绑定 |
| GET | `/.well-known/jwks.json` | **只暴露公钥** |

外加**租户上下文中间件**（`X-Tenant-Id` → `RequestContext` → `SET LOCAL app.tenant_id` → RLS）——
这正是 REQ-006 留下的 **AC-S-012** 的闭环前提。

### 1.4 数据访问：全部走 `platform-db` 的租户作用域入口

| 表 | 说明 | 租户维度 |
|---|---|---|
| `platform_iam_user` | 账号（密码哈希是 PHC 串，**不是明文**） | `tenant_id` + RLS |
| `platform_iam_password_history` | 密码历史（最近 5 条，用于 `PASSWORD_REUSED`） | 同上 |
| `platform_iam_session` | 设备会话（**每个会话一个 family**） | 同上 |
| `platform_iam_refresh_token` | refresh token（**只存 hash**） | 同上 |
| `platform_iam_mfa_secret` | TOTP secret（**ChaCha20-Poly1305 密文**）+ `last_used_step` | 同上 |
| `platform_iam_recovery_code` | 一次性恢复码（**只存 hash**） | 同上 |
| `platform_iam_password_reset` | 密码重置令牌（**只存 hash**，一次性 + 可过期） | 同上 |
| `platform_iam_login_attempt` | 登录尝试流水（按 IP 滑动窗口的计数依据） | 同上 |
| `platform_iam_idempotency` | 幂等键（契约 C8，**按租户维度**存储） | 同上 |

**全部 9 张表**都：`tenant_id NOT NULL` + `ENABLE ROW LEVEL SECURITY` +
policy `USING (tenant_id = current_setting('app.tenant_id', true))` + 同式 `WITH CHECK`
（**fail closed** + 写侧兜底）。

---

## 2. ★ 安全断言的实测证据

> 全部来自 **真实库层**（`base_test` 库 + **应用角色** `base_app`，RLS 真正生效），
> 走**真实 HTTP 路由**（`tower::ServiceExt::oneshot` 驱动 axum Router，不连网络）。

### 2.1 防账号枚举（AC-S-012）

> ⚠️ **修复轮重写**：第一轮这条用例**结构上看不见**枚举面（QA MEDIUM 4）：
> 状态码只断言了循环外的第一次，且 `median()` 取上中位数会把双峰分布的一整簇丢掉。
> 修复后的证据见 **§0.1.2**（逐次输出）与 §0.1.8（变异验证）。

```
情况一（账号存在 + 密码错误）：HTTP 401
  {"code":"INVALID_CREDENTIALS","detail":"invalid username or password", …}
情况二（账号不存在）        ：HTTP 401
  {"code":"INVALID_CREDENTIALS","detail":"invalid username or password", …}
```

- `code` / `detail` / `type` / `title` / `status` **逐字段相同**（断言 `assert_eq!`）
- `detail` 与设计摘要 §5.1 **逐字一致**
- **超过锁定阈值之后仍然是 `INVALID_CREDENTIALS`**（连发 6 次错密码逐次断言）——
  锁定判定排在密码校验**之后**（§0.1.1 HIGH 1）。
- **耗时对比（多轮取真中位数）**：

| 情况 | 耗时（中位数） |
|---|---|
| 账号存在 + 密码错误（未锁） | **563.1 ms** |
| 账号存在 + 密码错误（已锁） | **569.0 ms** |
| 账号不存在 | **543.2 ms** |
| **最差比值** | **1.048**（断言阈值 1.5） |

**机制（三道）**：
1. **假哈希校验**：账号不存在时也跑一次 Argon2id 校验，**用与真密码完全相同的参数**
   （`DummyHash` 在服务构造时按配置参数生成一次，之后复用）；
2. **同样的数据库工作量**：三条失败路径走**同样的三条语句**（一次 UPDATE + 一次 INSERT 流水
   + 一次 INSERT 失败计数，同一个事务）—— 账号不存在时 UPDATE 命中 0 行；
3. **锁定路径同样跑 Argon2**（修复前锁定路径 8.1ms vs 普通失败 606.9ms = **75 倍**时序差）。

> ⚠️ 开发过程中曾出现三个**真实缺陷**，都是被上面的用例抓出来的：
> ① INSERT 的列序与 `{tenant}` 占位符位置不匹配（`tenant_id` 被写成了 id 值）→ RLS `WITH CHECK` 拒绝；
> ② 重放检测后 family 作废走了 **rollback** → 「检测到泄露却什么都没作废」；
> ③ **锁定判定排在密码校验之前** → `ACCOUNT_LOCKED` 变成账号枚举预言机（QA HIGH 1，修复轮修掉）。

### 2.2 防密码重置枚举（AC-S-013）

```
已注册邮箱  → HTTP 200  {"status":"accepted"}
未注册邮箱  → HTTP 200  {"status":"accepted"}
```

两个响应体 **JSON 逐字段相等**（`assert_eq!(body_registered, body_unregistered)`）。
重置令牌**不进响应体**（真实系统走邮件通道）。

### 2.3 refresh 轮换与重放检测（AC-S-014）

| 步骤 | 结果 |
|---|---|
| ① 用 `refresh_1` 刷新 | **200** → 拿到 `refresh_2`（新令牌 ≠ 旧令牌） |
| ② **再次**用 `refresh_1` | **401** + `code=TOKEN_REUSE_DETECTED` → **整个 family 作废** |
| ③ 用 `refresh_2` 刷新 | **401** + `code=TOKEN_REVOKED` → **family 确实被作废** |

**实现要点**：轮换与作废在**同一个事务**里（`SELECT … FOR UPDATE` 行锁 → 作废旧的 → 签发新的），
且 family 作废**必须提交**（回滚 = 作废丢失）。

### 2.4 TOTP 防重放与恢复码一次性（AC-S-015）

| 步骤 | 结果 |
|---|---|
| 绑定 TOTP（返回 secret + otpauth URI + **10 个**恢复码） | **200** |
| 用当前时间步的验证码绑定 | **204** |
| **同一个验证码再用一次** | **401** + `code=MFA_INVALID` |
| 恢复码第一次 | **204** |
| **恢复码第二次** | **401** + `code=MFA_INVALID` |

### 2.5 设备会话（AC-S-018）

- 列表：**2 条**，每条都有 `deviceLabel` / `ip` / `lastUsedAt`
- 从 A 端下线 B → **B 的 refresh 立即失效**（401）
- 登出全端 → **A 的 refresh 也失效**（全部 family 作废）

### 2.6 租户隔离（AC-S-016）

| 场景 | 结果 |
|---|---|
| A 租户的 access token + B 租户的 `X-Tenant-Id` | **404**（不泄露资源存在性，契约 C3 / C5） |
| A 租户的 refresh token 到 B 租户刷新 | **401** |
| 直查 `platform_iam_session` | 每条记录的 `tenant_id` **都非空** |

### 2.7 JWKS（AC-S-010）

```json
{"keys":[{"use":"sig","alg":"ES256","kid":"kid-test","kty":"EC","crv":"P-256","x":"…","y":"…"}]}
```

- **逐个 key 成员检查**：不得出现 `d` / `p` / `q` / `dp` / `dq` / `qi` / `k`
  （EC 私钥的 `d`、RSA 私钥成员）—— 断言在**单测**与**真实库层**两处
- 轮换宽限期：JWKS 同时暴露 **2 个 kid**（单测 `rotation_keeps_the_previous_key_valid_and_publishes_both_kids`）
- 私钥**只从环境注入**：`changeme` / `replace_me` / `example` 等占位符 → **启动 fail-fast**

### 2.8 错误体合规（AC-S-019，契约 C6）

逐个错误路径实测并打印 `detail`（真实库层 `every_error_path_returns_an_english_ascii_detail`）：

| 错误码 | `detail`（实测输出） |
|---|---|
| `INVALID_CREDENTIALS` | `invalid username or password` |
| `PASSWORD_POLICY_VIOLATION` | `the password does not satisfy the password policy: min_length` |
| `UNAUTHENTICATED` | `authentication is required` |
| `TENANT_CONTEXT_MISSING` | `the tenant context is missing` |

另有单测覆盖**全部 17 个错误路径**（`every_detail_is_english_ascii` / `every_error_code_is_upper_snake_case`）。

**形状**：RFC 9457 —— `type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id` / `params`。
JSON 字段一律 **camelCase**（契约 C13）。

---

## 3. 依赖与编译资源

### 3.1 依赖树

```
cargo tree --workspace --prefix none | awk '{print $1}' | sort -u | wc -l
→ 232（第三方口径；QA 实测同口径 232，与本轮一致）
```

> ⚠️ **修复轮未改任何依赖**（`git diff -- '*Cargo.toml' '*Cargo.lock'` **为空**）——
> 新增的全局限流计数表只用了平台已有的 `platform-db` 入口，**零新增 crate**。

| 项 | 数量 |
|---|---|
| 工作区成员 | 7 → **8**（新增 `platform-iam`） |
| 第三方 crate | 172 → **232**（**+60**） |
| **修复轮增量** | **0**（232 → **232**） |

**新增的第三方 crate 与理由**：

| crate | 版本 | 用途 | 为什么必须 |
|---|---|---|---|
| `argon2` | 0.5.3 | 密码哈希（Argon2id） | 设计摘要 §3：「安全模块不省依赖 —— 自制密码学绝不可接受」 |
| `jsonwebtoken` | 10.4.0 | JWT 签发 / 验签 / JWKS | 设计摘要 §8 指定的 crate；**`default-features = false` + `rust_crypto`**（ring / aws-lc-rs 要编 C/汇编，低配机上又慢又吃内存） |
| `totp-rs` | 5.7.2 | TOTP（HMAC-SHA1 + base32） | 设计摘要 §4 指定的 crate |
| `chacha20poly1305` | 0.11.0 | TOTP secret 的**静态加密** | TOTP secret 必须**可逆**（要还原才能算验证码）→ 不能只哈希；用成熟 AEAD 而不是自制（设计摘要 §3 的同一条原则） |
| `p256` | 0.13.2 | JWT 密钥生成 + 公钥坐标 | 与 `jsonwebtoken` 的 `rust_crypto` **同版本**（不新增 crate，只统一 feature） |
| `base64` | 0.22.1 | refresh token / JWKS 坐标的 base64url | 树里已有（sqlx 带入），**不新增** |
| `sha2` | 0.10.9 | refresh token / 重置令牌 / 恢复码的**摘要** | 高熵值 → 快哈希就够；树里已有，**不新增** |
| `rand` | 0.8.8 | 随机字节（refresh token / 恢复码 / TOTP secret） | 与 `jsonwebtoken` 的 `rust_crypto` 同版本（rand 0.8），**不新增** |

**ring / aws-lc-rs 的实测**：

```bash
cargo tree --workspace --all-features | grep -oE "\b(ring|aws-lc-rs|aws-lc-sys) v[0-9][^ ]*" | sort -u
→ （空）
```

### 3.2 冷编译实测（依赖树变了，REQ-006 的 63.7s / 925MB 作废）

```bash
./scripts/measure-cold-build.sh     # cargo clean → 逐项实测（VmHWM 采样求和）
```

| 项 | 耗时 | 峰值内存 |
|---|---|---|
| `cargo check --workspace --all-targets` | **55.7s** | **937 MB** |
| `cargo test --workspace --no-run`（**含链接**） | **59.1s** | **1250 MB** |

- **峰值 937–1250 MB < 1.5 GB** → `.cargo/config.toml` 的 `jobs` **保持 3**（**无需**下调到 2）
- 口径与 REQ-006 一致：本机**没有** `/usr/bin/time` → 用 **`VmHWM` 采样**（cargo + 其 rustc 子进程的高水位求和，偏保守的上界）
- 链接期（1250 MB）才是真峰值 —— 比 `cargo check` 高 313 MB

---

## 4. 回归

> 修复轮的数字（2026-11-18）：**225 passed / 0 failed**（非 ignored 口径，与第一轮**持平**）；
> 带 `--include-ignored` **240 passed / 0 failed**（真实库层 **16/16**）。

| 项 | 结果 |
|---|---|
| `cargo check --workspace --all-targets` | ✅ |
| `cargo test --workspace -- --test-threads=2` | ✅ **225 passed / 0 failed / 19 ignored**（REQ-006 基线 **183 个全绿**，**只增不减**） |
| `cargo test --workspace --tests -- --test-threads=2 --include-ignored` | ✅ **240 passed / 0 failed**（真实库层 **16/16**） |
| `bash scripts/test-integration.sh`（`--include-ignored`，重置 `base_test`） | ✅ **exit 0**；**真实库用例 16/16**；容器数量 **2 → 2**（未新建容器） |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ **0 告警** |
| `cargo fmt --all --check` | ✅ |
| `pnpm -r test` | ✅ **23/23** |
| `bun run test:e2e` | ✅ **1/1**（chromium，5.2s） |
| `cargo xtask capabilities --check` | ✅ 13 个文件与源码一致 |
| `cargo xtask check-deps` | ✅ 平台 crate 未反向依赖业务模块（`platform-iam` 自动纳管） |
| `cargo xtask check-modules` | ✅ 1 个模块（sandbox） |
| `cargo xtask tenant-policies --check` | ✅ 模块声明的表都开了 RLS 且有策略（**须在 `scripts/test-integration.sh` 之后跑**，见 §5.2 第 10 条） |

**用例总数**：41 条单测（`platform-iam`）+ **16 条真实库层用例**（含 1 条防假绿护栏；第一轮 11 条 → 修复轮 **+5** 条）。

---

## 5. 偏离清单（如实登记）

### 5.1 第一轮的偏离

| # | 偏离 | 原计划 | 实际 | 原因与影响 |
|---|------|--------|------|-----------|
| 1 | **依赖树超出预算** | 179 → ≈215（AC-S-020 写 200~210） | **179 → 240**（第三方 +60） | ① `jsonwebtoken` 10 的 `rust_crypto` feature 是**全量**的（`ed25519-dalek` + `p256` + `p384` + `rsa` + `hmac` + `sha2`），**不能只挑 P-256**；jsonwebtoken 9.x 的默认后端是 **ring**（要编 C/汇编，任务明确禁止）→ 只能整包接受；② `chacha20poly1305` + 它的 AEAD 依赖（aead / cipher / poly1305 / universal-hash …）**+14**；③ `sqlx` 打开 `time` feature → `time-macros`（proc-macro，+1）。**影响**：冷编译峰值仍 **< 1.5G**（§3.2），但增量编译时间变长。**QA 判定**：资源侧全部达标、且**无可行的更省替代** → **不改代码**，由 BA 把 AC-S-020 重定基线到 **235** 并登记偏差（LOW 7） |
| 2 | **按 IP 的限流用 DB 计数而不是 Redis** | 设计摘要 §6：Redis 滑动窗口 + Redis 锁定快计数 | DB 滑动窗口（`occurred_at >= now() - window`） | 任务书明确允许「登录限流/配额若需 DB 计数则复用 `platform-db` 的配额能力」；`redis` 客户端会再加一批依赖（与依赖预算冲突）。**语义等价**（都是滑动窗口），**存储位置不同**。⚠️ **QA 附：观察已登记**（见 §5.2 第 1 条）：`platform-iam` 里 `grep -i redis` **0 命中**，`base-redis` 容器在跑但 IAM 不用 —— **高并发下 DB 计数会成为瓶颈 → 建议后续需求补** |
| 3 | **登录失败计数写在用户表** | 设计摘要 §6：Redis 快计数 + DB 持久 | 用户表的 `failed_attempts` + `locked_until` | 同上；账号锁定必须**持久**（重启不能丢），DB 是唯一可靠位置；快计数层（Redis）留待后续 |
| 4 | **恢复码用 SHA-256 而不是慢哈希** | 设计摘要 §4：只说「存 hash」 | SHA-256（base64url） | 恢复码是 **80 位熵**的高熵值，快哈希就够（慢哈希只对低熵的密码必要）。已在代码注释里写明口径 |
| 5 | **`title` 用中文、`detail` 用英文** | 契约 C6 | `title` = 中文类别名，`detail` = **英文 ASCII** | 契约 C6 的**示例**里 `title` 就是中文（「权限不足」）；仓库既有实现（REQ-005 的 `ProblemDetails`）也是中文 `title` + 英文 `detail`。**约束对象是 `detail`**（AC-S-019 明写） |
| 6 | **扩展了 `platform-db`** | REQ-006 的数据访问层 | `BindValue` 新增 `Timestamp` / `NullableText` / `NullableTimestamp`；`sqlx` 打开 `time` feature | 令牌过期、重置令牌过期、账号锁定到期都要写时间戳。`platform-core` / `platform-contracts`（CODEOWNERS 保护目录）**一行未改** |
| 7 | **平台表不走 manifest 的 `tenantScoped`** | REQ-006 的统一生成机制 | IAM 表在**平台迁移**里直接建 RLS policy | 账号体系是底座自身的身份权威源（ADR-0001 A3），属**平台表**（与 `platform_tenant_config` / `platform_tenant_quota` 同类），不是业务模块表 |

### 5.2 修复轮新增的偏离与 QA「附：观察」登记

| # | 偏离 / 观察 | 原计划 | 实际 | 原因与影响 |
|---|------------|--------|------|-----------|
| 8 | **QA 附：观察 ① —— 「Redis 滑动窗口 + Redis 锁定快计数」未实现** | 设计摘要 §6 | 实现是**纯 DB 计数**（`platform_iam_login_attempt` 审计流水 + `platform_iam_login_rate` 全局失败计数） | `platform-iam` 里 `grep -i redis` **0 命中**；`base-redis` 容器在跑但 **IAM 不用**。**功能等价**（429 分支已实测通过），但**高并发下 DB 计数会成为瓶颈** → **建议后续需求补**（本期不改：引入 redis 客户端会再加一批依赖，与依赖预算冲突） |
| 9 | **QA 附：观察 ② —— 前端没有 IAM 接入面** | —— | `apps/shell/src/lib/nav.ts` 的 IAM 入口是**占位数据**（点击落到 404） | **本期不含前端登录页**（设计摘要 §9 未列入）→ **不算缺陷**。但**本期没有 UI 验收面** —— IAM 的验收全部在 HTTP / 数据库层 |
| 10 | **QA 附：观察 ③ —— `tenant-policies --check` 对库状态敏感** | —— | 在**未经 `scripts/test-integration.sh` 重置**的旧库上会失败（报 `sandbox_tenant_item` 未开 RLS） | 那是**模块表**、与 REQ-007 无关（模块表不是 IAM 表）。→ **已写进 `docs/dev-environment.md` §4.2**：「**先跑 `scripts/test-integration.sh` 再跑 `tenant-policies --check`**」 |
| 11 | **修复轮：新增一张**不带租户维度**的全局表** | 设计摘要 §6：限流按租户 | `platform_iam_login_rate`（**只有 IP 的 SHA-256 摘要 + 时间戳**，**不含任何租户数据**、**不开 RLS**） | 限流预算必须**跨租户共享**（QA LOW 6：按租户计 → 预算被放大成「租户数倍」），而 RLS 下的租户作用域表**只能看见自己租的行**。**影响**：全局表里**不存明文 IP**；只记**失败**尝试（成功登录压根不记） |
| 12 | **修复轮：`ACCOUNT_LOCKED` 的判定位置** | 第一轮：锁定判定在密码校验**之前** | 锁定判定挪到密码校验**之后** | 修复前的 `ACCOUNT_LOCKED` 是**账号枚举预言机**（QA HIGH 1）。**契约不变**：`ACCOUNT_LOCKED` 仍是稳定 `code`，前端 i18n 按 `code` 渲染；只是**拿到它的人**变了（必须是**知道密码**的人） |

---

## 6. ★ 防假绿护栏（缺环境变量 → 必须非绿）

### 6.1 机制

`crates/platform-iam/tests/iam_real_db.rs`：

- 真实库用例的夹具 `required_test_database_url()`：**缺 `DATABASE_URL_TEST` 直接 panic**（不是「安静地返回」）；
- 数据库名**不是 `base_test` 直接失败**（`base_dev` 是开发库，测试绝不能碰）；
- **护栏用例** `missing_database_url_test_does_not_report_success`：把**自己这个测试二进制**再拉起一个子进程，
  **去掉** `DATABASE_URL_TEST`，只跑真实库用例（`--skip` 掉本护栏避免递归），断言：
  1. 子进程**退出码非 0**（承重的一条）；
  2. 子进程输出含 **`0 passed`**（一个用例都没报通过）。

### 6.2 实测

```bash
$ cargo test -p platform-iam --test iam_real_db -- --test-threads=1
test result: ok. 1 passed; 0 failed; 10 ignored; …        # 护栏本身绿

$ DATABASE_URL_TEST=… cargo test -p platform-iam --test iam_real_db -- --include-ignored
test result: ok. 11 passed; 0 failed; …                    # 真实库用例真跑（11/11）
```

### 6.3 变异验证（证明护栏**承重**，不是摆设）

把 10 个真实库用例的函数体改成「缺 `DATABASE_URL_TEST` 就**早退**」（制造假绿）：

```
running 10 tests
test login_failure_does_not_reveal_whether_the_account_exists ... ok
…（10 个全部 ok）
test result: ok. 10 passed; 0 failed; …

---- missing_database_url_test_does_not_report_success stdout ----
panicked at crates/platform-iam/tests/iam_real_db.rs:1275:5:
缺 DATABASE_URL_TEST 时子进程必须失败（它却成功退出了）：
running 10 tests
… test result: ok. 10 passed; 0 failed …

test result: FAILED. 0 passed; 1 failed; 10 ignored
```

→ **护栏立刻变红**（子进程退出码 0 + 「10 passed」两个断言同时挂掉）。
**已还原**（还原后护栏重新变绿，上面的输出就是还原后的状态）。

> 顺带记录一个**更弱的变异**（把夹具的 panic 改成「安静地返回空串」）：护栏**没有**变红 ——
> 因为后面的连接失败仍然 panic，子进程照样非零退出。**这说明只有「测试体早退」才是真正的假绿形状**，
> 护栏守的正是这一种。

---

## 7. 未完成 / 未验证（如实登记）

### 7.1 修复轮**已补上**的用例缺口

| 项 | 状态 | 说明 |
|---|---|---|
| 锁定期**过后**自动解锁 | ✅ **已补用例** | `a_lock_is_never_extended_and_the_failure_count_restarts_after_it_expires`：用管理员连接把 `locked_until` 挪到过去（模拟到期），断言到期后第一次错密码 → 失败计数**从头开始**（= 1）、锁定解除 |
| 按 IP 滑动窗口限流的 **429 分支** | ✅ **已补用例** | `the_ip_rate_limit_is_shared_across_tenants_and_only_counts_failures`：第 21 次 → **429 `LOGIN_RATE_LIMITED`**；**换租户同 IP 同样 429**；20 次**成功**登录**不计入**窗口 |

### 7.2 仍然未完成 / 未验证

| 项 | 状态 | 说明 |
|---|---|---|
| 「成功登录后失败计数清零」 | **未写自动化用例** | 实现上登录成功调 `reset_login_failures`（清零 + 解锁）；**没有**端到端断言。⚠️ 但 QA 已**独立实测**过这条（Dev 自陈缺口，QA 复验为真） |
| 幂等键（C8）的端到端行为 | **未写自动化用例** | 中间件已实现（认领 → 重复请求返回首次结果 / 处理中 409）；**没有**断言「同一 key 两次 POST 返回同一响应体」。⚠️ QA 已**独立实测**过这条（复验为真） |
| access token 的**即时**撤销 | **本期不做**（设计取舍） | 15 分钟短 TTL 的自包含 JWT；撤销能力由 refresh token + 会话承担。改密码后旧 access token 仍可用 ≤ 15 分钟 —— **已知取舍**（设计摘要 R4） |
| ~~`ACCOUNT_LOCKED` 的枚举面~~ | ✅ **修复轮已消除** | 锁定判定挪到密码校验**之后** → 攻击者**永远**只拿到 `INVALID_CREDENTIALS`；`ACCOUNT_LOCKED` 只有**知道密码**的人拿得到（§0.1.2 / §0.1.3）。`ACCOUNT_DISABLED` 一直在密码正确之后才提示，无枚举面 |
| `platform_iam_login_rate` 的**清理/归档** | **未做**（与既有流水表同状况） | 全局失败计数表按行累积（与 `platform_iam_login_attempt` 一样）。已有 `purge_login_attempts_before` 但**没有被任何定时任务调用** —— 属**既有**状况，本期未引入定时任务。索引 `(ip_hash, occurred_at DESC)` 保证计数查询与表大小无关 |
| 限流的**多副本一致性** | **未实测** | 限流是 DB 计数（不是 Redis）→ 多实例共享 DB 时行为**理论上**一致，但本期**单机单实例**实测（与 QA 报告的「未覆盖」一致） |
| 租户级注册策略（开关 / 域名白名单 / 审批态）、邀请链接 | **不做** | 需求文档「不在范围内」明列（REQ-010） |
| WebAuthn / FIDO2 实现、短信验证码、图形验证码、完整 HIBP 泄露库 | **不做** | 设计摘要 §9「不做」 |
| SSO 入站、企业目录同步 | **不做** | 属 REQ-013 / REQ-008 |
| 覆盖率数字 | **未测** | 本机没有 `cargo-llvm-cov`（不装新工具，见低配环境约束） |
| 真实负载性能 | **未测** | 属压测范畴（REQ 之外） |
| Helm / compose 侧的 IAM 环境变量透传 | **未做** | 本期只补了 `.env.example`；**部署编排**（compose / Helm）注入 IAM 密钥需在交付流程里单独处理（密钥走 Secret，不进仓库） |

---

## 8. 真机冒烟（**跑的是真的 `base` 二进制**，不是测试夹具）

> 上面 §2 的证据来自**真实库层**（`tower::ServiceExt::oneshot` 驱动 axum Router）。
> 这一节是**端到端**的：真的起进程、真的发 HTTP 请求、真的连 `base_test` 库
> （**应用角色** `base_app` → RLS 真正生效）。

```bash
export DATABASE_URL=postgres://base_app:base@127.0.0.1:5432/base_test
export BASE_JWT_PRIVATE_KEY="$(cat jwt.pem)" BASE_JWT_KID=kid-smoke
export BASE_IAM_SECRET_KEY="$(cat secret.txt)"
./target/debug/base serve &        # SERVER_PORT=18080
```

| 步骤 | 实测输出 |
|---|---|
| `GET /health/live` | `{"status":"alive","traceId":"…","version":"0.1.0"}` |
| `GET /health/ready` | `{"appliedMigrations":[20261118000100,20261118000200,20261118000400],"status":"ready"}` ← **IAM 迁移已应用** |
| `GET /.well-known/jwks.json` | `{"keys":[{"use":"sig","alg":"ES256","kid":"kid-smoke","kty":"EC","crv":"P-256","x":"…","y":"…"}]}` ← **只有公钥** |
| `POST /api/iam/login`（**无租户头**） | **401**（`TENANT_CONTEXT_MISSING`） |
| `POST /api/iam/register`（`X-Tenant-Id`） | **201** + 账号视图（`tenantId` 回显） |
| `POST /api/iam/login` | **200** + `accessToken` / `refreshToken` / `user` |
| `GET /api/iam/sessions` | `[{"id":"…","deviceLabel":"unknown","ip":"","userAgent":"curl/8.23.0","createdAt":"…","lastUsedAt":"…"}]` |
| `POST /api/iam/refresh` | **200** + **新**的令牌对（轮换） |
| **重放**旧 refresh | **401** `{"code":"TOKEN_REUSE_DETECTED","detail":"refresh token reuse was detected and the token family has been revoked",…}` ← `detail` **英文 ASCII** |
| 跨租户（令牌 A + 租户头 B） | **404** |

**占位符密钥 fail-fast**（R3）：

```bash
BASE_JWT_PRIVATE_KEY="changeme-private-key" … ./target/debug/base serve
→ EXIT=2
✗ 配置校验失败，进程退出（fail-fast）：
  - [IAM_CONFIG_PLACEHOLDER_SECRET] BASE_JWT_PRIVATE_KEY：密钥看起来是占位符；生产密钥必须是真实生成的（P-256 / PKCS#8）
```

## 8. 关键实现取舍（供 QA 复核）

| 取舍 | 选择 | 理由 |
|---|---|---|
| 令牌模型 | access = JWT / refresh = 不透明串 | 设计摘要 D1；全 JWT 无法撤销（全不透明则每次查库） |
| 轮换的原子性 | **单事务** + `FOR UPDATE` 行锁 | 「读旧 → 作废旧 → 签发新」要么全成要么全不成 |
| 重放判定顺序 | `used_at`（重放）**先于** `revoked_at`（已撤销） | family 被作废时旧令牌也会带上 `revoked_at`；但「用过又再用」是更严重的信号 |
| 防枚举的假哈希 | **同参数**生成一次，**复用** | 参数不同 → 耗时不同 → 枚举信号；生成一次避免每次登录多一次哈希 |
| 假哈希的时机 | 在**返回错误之前**执行 | 先返回 = 耗时立刻暴露「账号不存在」 |
| 数据库工作量对齐 | 两条失败路径走**同样的两条语句** | 只对齐哈希耗时还不够：一次 UPDATE 的往返也会成为信号 |
| JSON 字段命名 | **camelCase** | 契约 C13 |
| `title` 的语言 | 中文类别名 | 契约 C6 的示例 + 仓库既有实现；**约束对象是 `detail`** |
| 权限点 | 登录响应的 `permissions` 恒为空数组 | RBAC 属 REQ-009（契约 C4） |
| IAM 的启用方式 | 环境变量**一个都没配** → 不挂载 `/api/iam/*`；**配了一部分** → fail-fast | 「没配」是合法状态；「配错」绝不能半配置上线 |
