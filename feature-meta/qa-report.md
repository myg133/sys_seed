# 验证报告 —— Pre-merge

- **需求**: REQ-007 认证与账号体系
- **分支**: `feature/REQ-007` @ `e755e5c`（基线 `develop` @ `91d8e09`）
- **改动规模**: 31 文件 / +8118 -785；新增工作区成员 `crates/platform-iam`（共 8 个）
- **验证者**: QA Agent（独立复现；**未采信 Dev 自验报告里的任何数字与结论**，全部自己重跑、自己计时）
- **环境**: rustc/cargo 1.98.1 · podman 6.2.0 + postgres 17-alpine + redis 7-alpine · pnpm 12.19.0 · bun 1.4.4 · node v26.13.1 · 8 核 / 7.7G 内存（实测可用 ~4.8G）
- **日期**: 2026-11-18
- **结论**: **FAIL**

> 核心结论：**除两个 HIGH 级安全缺陷外，其余全部复现为真且质量很高。**
> 轮换/重放、TOTP 防重放、租户隔离、JWKS、错误体、防假绿护栏、回归矩阵 —— 我逐条独立复现为真；
> Dev 自陈的「rollback 致命缺陷」**确实修好了**，而且我用 **DB 级证据**证明它是走 **commit**（不是碰巧）。
> Dev 自陈的 4 个用例缺口，我逐个跑过，**实现全是对的**。
>
> 但有**两个必须合并前处理**的缺陷，都落在同一个根因上：
>
> **#1（HIGH，阻塞）`ACCOUNT_LOCKED` 是一个真实的账号枚举预言机** —— 6 个请求就能区分「账号存在 / 不存在」。
> Dev 在 `error.rs:10-17` 写的论证是**反的**：「未注册账号永远拿不到 ACCOUNT_LOCKED」这件事本身，
> 就是攻击者用来判断账号存不存在的那把钥匙。
>
> **#2（HIGH，阻塞）账号锁可以被无限续期 → 永久登录拒绝服务** —— 攻击者每 15 分钟发一次错密码，
> 就能让受害者**永远**登不进去（`failed_attempts` 只增不减、`locked_until` 每次刷新）。
>
> 两者叠加 = **先枚举、再永久锁死**：只读的信息泄露被升级成拒绝服务。
>
> 我另外发现 **Dev 自己那条「防账号枚举」用例在结构上看不见 #1**（§问题 #4）：把它从 3 轮扩到 8 轮后，
> 第 5 轮起状态码已经 401→403 漂移、耗时从 230ms 塌到 3ms，用例却仍然报 **「比值 1.08 / ok」**。
>
> 依赖预算：实测 **175 → 232 个 crate（+57）**，**超 AC-S-020 的 200~210 约 22 个**；
> 但**冷编译资源全部达标、增量编译没有变慢**，且**没有可行的更省替代**（我独立核实了 jsonwebtoken 的版本约束）。
> 建议**重定基线**而不是降级依赖（见 §依赖预算判定）。

---

## 追溯

| AC | 实现 | 测试 | 结论 |
|----|------|------|------|
| **AC-S-009** 密码策略可配置（具体规则 + 历史不重复） | `crates/platform-iam/src/config.rs` `check_password_policy`；`service.rs:818-874` `apply_new_password` | `error.rs` 单测；`iam_real_db.rs::changing_the_password_revokes_the_other_families_and_rejects_history`；**我自己实测**：`PASSWORD_POLICY_VIOLATION` 422 + `params.violations[0].rule == "min_length"`；重复历史密码 → `PASSWORD_REUSED` 422 | **PASS** |
| **AC-S-010** 密钥轮换不中断 + JWKS 双 kid | `jwt.rs:110-189` `JwtKeySet`（current + previous）、`http.rs:405-413` JWKS 路由 | `jwt.rs::rotation_keeps_the_previous_key_valid_and_publishes_both_kids`；**我自己实测**：JWKS 实响应只有 `use/alg/kid/kty/crv/x/y` | **PASS** |
| **AC-S-011** 密码哈希不可逆 + 参数可配（PHC 自描述） | `password.rs:71-110`（Argon2id，`verify_password` 用**存储串里的参数**） | `password.rs` 单测 4 条；**我自己实测**：登录后直查 `platform_iam_user.password_hash` 为 `$argon2id$v=19$...` | **PASS** |
| **AC-S-012** 登录失败不泄露账号是否存在 | `service.rs:267-354`（假哈希 + 对齐 DB 往返） | `iam_real_db.rs::login_failure_does_not_reveal_whether_the_account_exists`（**只跑 3 轮**，见问题 #4）；**我自己 25 轮实测** | **FAIL** —— 「不存在 vs 密码错」这一对完全达标（见下），但**多出一条 `ACCOUNT_LOCKED` 路径构成枚举面** → 问题 #1/#2/#3 |
| **AC-S-013** 密码重置不泄露邮箱是否存在 | `service.rs:727-754`（返回 `Option<token>`，token 不进响应体） | `iam_real_db.rs::password_forgot_does_not_reveal_whether_the_email_is_registered`（逐字节相等断言） | **PASS** |
| **AC-S-014** refresh 轮换 + 重放检测 → 整个 family 作废 | `service.rs:529-637`（`tx.commit()` 在 **service.rs:566**） | `iam_real_db.rs::refresh_rotation_revokes_the_whole_family_on_reuse`；**我自己 DB 级复验**（见下） | **PASS** —— Dev 自陈的 rollback 缺陷**确实修好**，DB 证据在 §安全断言实测 |
| **AC-S-015** TOTP 防重放 + 恢复码一次性 | `totp.rs:148-190`（`step <= last_used_step` 即拒）、`service.rs:946-991` | `iam_real_db.rs::totp_codes_cannot_be_replayed_and_recovery_codes_are_single_use`；**我自己实测**：同码第二次 → `MFA_INVALID`；恢复码第二次 → `MFA_INVALID` | **PASS** |
| **AC-S-016** 会话带租户维度 + 不可跨租户复用 | 迁移 `20261118000400_iam_auth.up.sql`（每表 `tenant_id` + RLS fail closed）；`http.rs:192-241` `Authenticated`（头与令牌租户不一致 → 404） | `iam_real_db.rs::tokens_cannot_cross_tenants_and_sessions_record_the_tenant`；**我自己实测**（见下） | **PASS** |
| **AC-S-017** 登录限流 + 账号锁定 | `service.rs:411-427` `check_ip_rate_limit`；`service.rs:359-388` `on_password_failure`；`service.rs:312-316` 锁定判定 | `iam_real_db.rs::repeated_failures_lock_the_account`；**我自己补验 4 个缺口** | **PARTIAL** —— 字面四步全过（阈值锁定 / 期内拒绝 / 期后自动解锁 / 成功后清零），**但锁定期可被无限续期 → 永久锁死** → 问题 #2 |
| **AC-S-018** 设备会话可管理 | `service.rs:644-717`（`logout` / `logout_all` / `revoke_session` / `list_sessions`） | `iam_real_db.rs::sessions_can_be_revoked_and_logout_all_revokes_every_family`；**我自己实测** | **PASS**（一处与设计摘要的偏差 → 问题 #5） |
| **AC-S-019** 错误体符合契约 C6（`detail` 英文 ASCII） | `error.rs:189-220` `detail()`；`http.rs:93-123` `problem_details` | `error.rs::every_detail_is_english_ascii`；**我自己抽 8 条实际响应**（见下） | **PASS** |
| **AC-S-020** 依赖预算 + 编译资源 | 根 `Cargo.toml`（安全模块依赖例外） | **我自己实测**依赖树 + 冷编译 | **FAIL（预算）/ PASS（资源）** —— 232 个 crate > 预算上限 210（+22）；但冷编译峰值 < 1.5G、增量 check 0.98s → 问题 #7 |
| **父 AC-007** 令牌轮换与撤销 | 见 AC-S-014 / AC-S-018 | 同上 | **PASS** |
| **父 AC-008** 设备会话可管理 | 见 AC-S-018 | 同上 | **PASS** |
| **父 AC-003** 注册策略按租户隔离 | `platform_iam_user` 唯一约束 `(tenant_id, username)` | `iam_real_db` 跨租户用例 | **PASS** |
| **父 AC-004** 邀请链接一次性且可过期 | —— | 本期未实现（需求文档「明确不做」列表外，design-summary §9 未列入交付边界；Dev 已在追溯矩阵登记） | **未实现（已登记）** |
| **WebAuthn / FIDO2** | `totp.rs:32-70` `MfaMethod::WebAuthn`（只留接口） | `totp.rs::webauthn_is_declared_but_not_implemented` | **PASS（只留接口，符合 design-summary §4）** |

---

## 安全断言实测

### A. 防账号枚举（25 轮，自己计时 —— 生产默认 Argon2 参数 m=19456/t=2/p=1）

| 项 | 期望 | 实测 |
|----|------|------|
| 轮次 | ≥ 20 | **25 轮**（每轮：存在账号+错密码 / 不存在账号，各 1 次） |
| 状态码 | 两者相同 | 两者**全部 401**，`sorted(set)=[401]` |
| 错误码 | 两者相同 | 两者**全部 `INVALID_CREDENTIALS`** |
| 响应体逐字段 | 全部相同 | `code` / `detail` / `instance` / `status` / `title` / `type` **6 个字段全部 same=True**；仅 `trace_id` 不同（长度**均为 36**，字符集**完全相同**，UUIDv4） |
| 响应头 | 不泄露 | 逐头比对：`content-type` / `content-length`(均 233) / `connection` 全部 same=True，仅 `date` 不同；**泄号头扫描 = 空**（无 `X-User-Exists` / `X-Account-*` 之类） |
| 耗时中位数 | 比值 < 1.5 | 存在 **532.6ms** vs 不存在 **542.0ms** → **比值 1.0176**（存在账号反而**更快**） |
| 耗时分位数 | 比值 < 1.5 | p90 0.9199 / p10 0.9570 / max 0.9221 / min 0.9982 / mean 0.9770 —— **全部 < 1.0** |
| 偏差 | —— | 绝对值比 Dev 的 248.7/219.2ms 高，是因为我跑的是**生产默认 Argon2 参数**（Dev 的用例用 m=8192/t=2）；**结论方向一致且比值更稳** |

### B. ★ 账号枚举面（`ACCOUNT_LOCKED`）—— 独立验证结果

**已独立复现为真，Dev 的判断是错的。**

| 场景 | 第 1~5 次 | 第 6 次起 |
|------|----------|-----------|
| **账号存在**（`victim`） | `401` / `INVALID_CREDENTIALS` | **`403` / `ACCOUNT_LOCKED`** |
| **账号不存在**（`ghost`） | `401` / `INVALID_CREDENTIALS` | `401` / `INVALID_CREDENTIALS` |

```
ENUMERATION ORACLE: attacker sends 5 wrong pw, then checks attempt #6
  existing  -> status=403 code=ACCOUNT_LOCKED  title='账号已锁定'
  missing   -> status=401 code=INVALID_CREDENTIALS  title='凭证无效'
  DISTINGUISHABLE = True
```

- **攻击成本**：每个候选名 **6 个请求**（5 次错密码 + 1 次读信号）。
- **速率**：IP 限流 20 次/5 分钟/租户/IP → 单 IP 约 40 个候选/小时；换 IP 可并行 → **实际不可防**。
- **锁定期内用正确密码** → 也是 `403 ACCOUNT_LOCKED`（`service.rs:312` 的判定在密码校验之前）。
- **跨租户不可见**：从别的租户看 `victim` → `401 INVALID_CREDENTIALS`（用户名按租户唯一，枚举面是**租户内**的）。
- **`ACCOUNT_DISABLED` 不是枚举面**（我单独验过）：禁用账号 + **错**密码 → `401 INVALID_CREDENTIALS`；只有**密码正确**才回 `403 ACCOUNT_DISABLED`（因为禁用判定在密码校验**之后**，`service.rs:328`）—— **这一条 Dev 做对了**。

### C. 令牌轮换与重放（Dev 自陈的 rollback 缺陷）

**Dev 自陈的「检测到泄露却什么都没作废」缺陷确实修好了，而且走的是 commit —— 我用 DB 级证据证明。**

```
--- step 1: rotate refresh_1 -> refresh_2 ---
  rows BEFORE replay (used|revoked):
    25339d9f…|t|f      ← refresh_1：已用过，未作废
    69d566f6…|f|f      ← refresh_2：未用过，未作废
--- step 2: REPLAY refresh_1 (the leak) ---
  replay -> status=401 code=TOKEN_REUSE_DETECTED
  rows AFTER replay (used|revoked):
    25339d9f…|t|t      ← refresh_1：已用过，已作废
    69d566f6…|f|t      ← refresh_2：未用过，已作废  ← family 连带作废
  rows in family still NOT revoked: 0   ← 必须为 0，证明落库（COMMIT）而非 ROLLBACK
  refresh_2 after replay -> status=401 code=TOKEN_REVOKED
  session revoked in DB: t
```

代码侧对应：`crates/platform-iam/src/service.rs:562-568` —— 重放分支里 `revoke_family()` 之后是
`tx.commit().await`（不是 `rollback`）。**结论：真修好了，不是碰巧。**

### D. TOTP

| 项 | 期望 | 实测 |
|----|------|------|
| `mfa/enroll` 返回 secret | 与 `mfa/verify` 能对上 | secret 32 字符 base32；`otpauthUri` 含 secret；`mfa/verify` 用它算出的码 → **204** |
| 同一时间步第二次 | 必须被拒 | 同码再验 → **401 `MFA_INVALID`** |
| 恢复码 | 只能用一次 | 第 1 次 → **204**；第 2 次 → **401 `MFA_INVALID`** |
| 登录时 MFA 强制 | 缺码要 `MFA_REQUIRED` | 无码登录 → **401 `MFA_REQUIRED`**；带**当前**时间步的码 → **200**（带**已用过**的码 → 401 `MFA_INVALID`） |
| DB 里不是明文 | 密文 | `platform_iam_mfa_secret.secret_cipher` = 64 字符 base64url 密文；**明文 base32 不出现在 DB**；不是 base32 字符集 → **ChaCha20-Poly1305 静态加密成立** |

### E. 设备会话与撤销

| 项 | 期望 | 实测 |
|----|------|------|
| 会话列表字段 | deviceLabel / ip / lastUsedAt | 2 条会话，`deviceLabel` / `ip` / `userAgent` / `createdAt` / `lastUsedAt` **齐全** |
| 从 A 下线 B | B 的 refresh **立即失效** | `DELETE /api/iam/sessions/{B}` → 204；B 的 refresh → **401 `TOKEN_REVOKED`**；A 的 refresh → 200 |
| 登出全端 | **全部** family 作废（含 A 自己） | `logout-all` → 200 `{"revokedSessions":2}`；A / B 的 refresh → **401 `TOKEN_REVOKED`** |
| 改密后其它设备 | 撤销 | 改密 → 204；**其它**设备的 refresh → 401 `TOKEN_REVOKED`；**当前**设备的 refresh → 也被作废（→ 问题 #5） |
| access token 不可撤销 | 有意取舍（design-summary R4） | logout-all 后 A 的 access token 仍可访问 `GET /sessions` → 200（**设计取舍，确认成立**） |

### F. 租户隔离

| 项 | 期望 | 实测 |
|----|------|------|
| A 租户 token + B 租户请求头 | 拒绝 | **404 `NOT_FOUND`**（不泄露资源存在性） |
| A 的 refresh 拿到 B 租户用 | 拒绝 | **401 `TOKEN_REVOKED`**（租户作用域查询 → 查不到） |
| 直查库：每条 session 带 tenant_id | 必须 | `platform_iam_session` 里 **tenant_id 为空/NULL 的行数 = 0** |
| 无令牌 | 拒绝 | **401 `UNAUTHENTICATED`** |
| A token + A 租户头 | 正常 | 200 |

### G. JWKS 与私钥

| 项 | 期望 | 实测 |
|----|------|------|
| 只暴露公钥 | 无私钥成员 `d/p/q/dp/dq/qi/k` | 实响应：`{"keys":[{"use":"sig","alg":"ES256","kid":…,"kty":"EC","crv":"P-256","x":…,"y":…}]}` —— **逐成员检查全部不存在** |
| 轮换后新旧 kid 同时在 JWKS | 是 | 单测 `rotation_keeps_the_previous_key_valid_and_publishes_both_kids` 断言 2 个 kid |
| 私钥不在 JWKS / 配置文件 / Git | 绝不 | 全仓 `git grep "BEGIN (EC \|RSA \|)PRIVATE KEY"` → **仅 2 处**，都是 `config.rs` 里的**假夹具**（`MIGHAgEAMBMG` 截断串、`changeme` 占位符，用于测解析失败/占位符拒绝）；**`git log --all -p` 全历史扫描同样只有这 2 处**；**无任何 `.pem/.key/.p12` 被提交**；`.env.example` 的 `BASE_JWT_PRIVATE_KEY` / `BASE_IAM_SECRET_KEY` **留空** |

### H. 错误体符合契约 C6（自己抽 8 条实际响应）

| 路径 | HTTP | `code` | `detail` | ASCII |
|------|------|--------|---------|-------|
| 密码错误 | 401 | `INVALID_CREDENTIALS` | `invalid username or password` | ✅ |
| 密码策略不满足 | 422 | `PASSWORD_POLICY_VIOLATION` | `the password does not satisfy the password policy: min_length` | ✅ |
| 令牌无效 | 401 | `UNAUTHENTICATED` | `authentication is required` | ✅ |
| 缺租户上下文 | 401 | `TENANT_CONTEXT_MISSING` | `the tenant context is missing` | ✅ |
| refresh 令牌不存在 | 401 | `TOKEN_REVOKED` | `the token has been revoked` | ✅ |
| 重置令牌无效 | 401 | `TOKEN_REVOKED` | `the token has been revoked` | ✅ |
| 用户名已存在 | 409 | `CONFLICT` | `the username is already taken` | ✅ |
| 成功登录 | 200 | —— | —— | ✅ |

`title` 为中文（「凭证无效」等）—— **契约 C6 允许**（面向用户的中文由前端 i18n 按 `code` 渲染）。

### I. ★ 防假绿护栏（承重性验证）

| 项 | 期望 | 实测 |
|----|------|------|
| 不设 `DATABASE_URL_TEST` 跑真实库用例 | 非 0 退出 | **EXIT 101** |
| 输出不含「N passed」 | 是 | `test result: FAILED. 1 passed; 10 failed` —— **10 个真实库用例全部 FAILED**（唯一那个 `1 passed` 是**护栏用例自己**，它正是「护栏生效」的证据） |
| **自己做一次变异验证** | 护栏应红 | 把 10 个用例改成「缺环境变量就早退」（= 最经典的假绿形态）→ **护栏立刻变红**：`缺 DATABASE_URL_TEST 时子进程必须失败（它却成功退出了）：… test result: ok. 10 passed; 0 failed` → **护栏是承重的，不是装饰** |
| 还原 | 必须还原 | `sha256` 与原文件**逐字节一致**（`151cb0ee0122601a06d77ee0b544c141f5e87c658a61b6c65b9e74b04fa15963`）；`git status` 干净 |

### J. ★ 防假绿护栏的**反面** —— Dev 的「防账号枚举」用例结构上看不见枚举面（问题 #4）

把 `iam_real_db.rs` 的耗时循环从 3 轮扩到 **8 轮**（只改测试，不改实现，在**交付物之外**的副本上做）：

```
TRACE existing=401 235ms | missing=401 272ms
TRACE existing=401 232ms | missing=401 220ms
TRACE existing=401 227ms | missing=401 264ms
TRACE existing=401 252ms | missing=401 213ms
TRACE existing=403 6ms   | missing=401 250ms   ← 状态码开始漂移
TRACE existing=403 3ms   | missing=401 245ms
TRACE existing=403 3ms   | missing=401 223ms
TRACE existing=403 3ms   | missing=401 225ms
防枚举耗时：存在账号+错密码 = 227.5ms；账号不存在 = 245.6ms；比值 = 1.08
test result: ok. 1 passed          ← 用例仍然报「通过」
```

**两条独立原因**让它看不见：

1. **状态码断言只覆盖第一次**：`iam_real_db.rs:361` 的 `assert_eq!(status_existing, status_missing)`
   用的是循环**外**的第 1 次结果；循环里第 5 轮起已经是 401 vs 403，**从未被断言**。
2. **「中位数」取法把双峰分布掩盖了**：`iam_real_db.rs:686-689` 的 `median()` 是 `values[values.len()/2]`，
   偶数长度时取**上中位数**。8 个样本排序后 = `[3,3,3,6,227,232,235,252]`，下标 4 = **227ms** ——
   恰好落在「未锁定」那一簇，**4 个 3~6ms 的锁定样本全被丢掉**，比值算出来是 1.08。

→ 护栏没坏，但**它守的方向和真正的洞不是同一个**。

---

## 账号枚举面判定

### 严重度：**HIGH**（CWE-204 可观察响应差异 / 账号枚举）+ **叠加拒绝服务**

### 是否阻塞合并：**是（阻塞）**

理由：

1. 与 **AC-S-012** 的意图（「登录失败不泄露账号是否存在」）**直接冲突** —— AC-S-012 字面只覆盖「不存在 vs 密码错」这一对，
   但这一对做对了，**另一对（存在+已锁 vs 不存在）就把洞补上了**。
2. 底座的**第一个安全敏感模块**；账号枚举是后续 RBAC / 配额 / 数据范围 / 审批流全部策略的前置攻击面。
3. 攻击成本极低（**6 请求/候选**），且可跨 IP 并行；IP 限流是**按租户**计的，挡不住。
4. 与问题 **#2** 叠加后，攻击者可以**先枚举、再永久锁死**，把只读信息泄露升级成拒绝服务。

### Dev 的论证为什么不成立

`crates/platform-iam/src/error.rs:10-17` 原文：

> 「区分『锁定的账号』与『密码错误』是**允许**的 —— 账号已存在且被锁定这件事本身不是秘密……
> 锁定判定发生在**账号存在**的前提下，所以它与『密码错误』的错误码不同，但**不构成账号枚举**——未注册账号永远不会拿到 `ACCOUNT_LOCKED`。」

这段话**自相矛盾**：正因为「未注册账号**永远不会**拿到 `ACCOUNT_LOCKED`」，而「已注册账号第 6 次**就会**拿到」，
**这个差异本身就是一个判别器**。攻击者不需要知道密码，只需要**数到第 6 次**。

### 建议修法（两个约束同时满足：不泄露账号存在性 + 被锁用户知道被锁）

**核心改动：把「锁定判定」挪到「密码校验之后」。**（`crates/platform-iam/src/service.rs:312-330`）

```rust
// 4) 先校验密码。
//    ACCOUNT_LOCKED 只能在「调用方已经证明知道密码」之后出现 ——
//    否则它就是一个账号枚举预言机（CWE-204）：
//    不存在的账号永远拿不到它，已注册的账号第 6 次就拿到 → 6 个请求即可枚举。
if !password::verify_password(&req.password, &user.password_hash) {
    self.on_password_failure(ctx, &user, meta, &req.username).await?;
    return Err(IamError::InvalidCredentials);   // 锁定期内也一律这个码
}

// 5) 密码正确 → 这时才可以说「你的账号被锁了」
if is_locked(&user) {
    self.record_attempt(ctx, meta, &req.username, false).await?;
    return Err(IamError::AccountLocked);
}

// 6) 账号已禁用（密码正确之后才提示）
if user.status != STATUS_ACTIVE {
    return Err(IamError::AccountDisabled);
}
```

**为什么这样两个约束都满足**：

| 约束 | 如何满足 |
|------|----------|
| **不泄露账号存在性** | 攻击者（不知道密码）**永远**只能拿到 `INVALID_CREDENTIALS` / 401 —— 与不存在的账号**完全一致**。枚举面**消失**。 |
| **被锁用户知道被锁** | 真正被锁的用户**知道密码** → 密码校验通过 → 拿到 `ACCOUNT_LOCKED` / 403 + 明确文案。**「被锁用户知道被锁」照样满足**。 |
| **契约不变** | `ACCOUNT_LOCKED` 仍是**稳定 code**，前端 i18n 按 `code` 渲染中文文案，**契约 C6 与前端契约都不动**。 |
| **顺带修掉时序洞（问题 #3）** | 锁定路径也走 Argon2 校验 → 耗时与其它路径对齐 → **75× 的时序差一并消失**（一次改动解决两个问题）。 |

**配套改动 1 —— 防「永久锁死」（问题 #2）**，在 `service.rs:359-388` `on_password_failure`：

```rust
let now = OffsetDateTime::now_utc();
// 已经在锁定期 → **不续期**（否则攻击者每 15 分钟一次错密码就能把锁无限延长 → 受害者永远登不进去）
if user.locked_until.is_some_and(|until| until > now) {
    return Ok(());   // 只记流水，不动 failed_attempts / locked_until
}
// 上一轮锁定已到期 → 失败计数从头开始
let attempts = if user.locked_until.is_some() { 1 } else { user.failed_attempts + 1 };
let locked_until = if attempts >= self.config.login.account_failure_limit as i32 {
    Some(now + self.config.login.lockout_duration)
} else {
    None
};
```

**配套改动 2 —— 补上会再次漏掉这个洞的用例**（问题 #4）：

- 「同一账号连续 6 次错密码」→ 第 6 次的 `code` **必须仍是 `INVALID_CREDENTIALS`**；
- 「账号被锁 + **正确**密码」→ `ACCOUNT_LOCKED`（证明「被锁用户知道被锁」没被改坏）；
- 防枚举耗时用例要**逐轮断言状态码**（不能只断言第一轮），且耗时比值要对**全部样本**用**真正的中位数**
  （现在的 `values[len/2]` 在偶数长度时取上中位数，恰好落在未锁定那一簇，把双峰分布掩盖了）。

---

## 依赖预算判定

### 实测数字（我自己数的，不采信 Dev 的）

| 项 | Dev 的说法 | **我的实测** | 差异 |
|----|-----------|-------------|------|
| 基线 crate 数 | 179 | **175** | Dev 偏高 4 |
| 本期 crate 数 | 240 | **232** | 绝对值与 Dev 的「第三方 232」一致（Dev 的 240 是总数口径，偏高 8） |
| 增量 | +60 | **+57** | — |
| 超预算（AC-S-020：175 → 200~210） | — | **232 > 210，超 22 个（+10.5%）** | 预算**确实被突破** |

- 口径：`cargo tree --workspace`（与 AC-S-020 字面一致）与 `cargo tree --workspace --all-features` 结果**相同**（都是 232）。
- `comm` 比对：**新增 57 个，移除 0 个**。

### 独立验证 Dev 的「没有 ring」主张 —— **成立**

```
cargo tree --workspace --all-features | grep -oE "\b(ring|aws-lc-rs|aws-lc-sys) v[0-9][^ ]*"
  -> 0 命中
```
唯一名字里含 "ring" 的 crate 是 **`stringprep v0.1.5`**，来源是 **`sqlx-postgres` → `sqlx`**（RFC 3454 Unicode Stringprep，
用于 SCRAM 的 SASLprep，**不是加密库**）。Dev 的说法**属实**。

### 冷编译实测（用 Dev 自己的 `scripts/measure-cold-build.sh`，同一套 VmHWM 采样口径）

| 阶段 | **REQ-007 实测** | develop 基线实测（同一脚本、同一台机） | 增量 | AC-S-020 上限 |
|------|------------------|--------------------------------------|------|--------------|
| `cargo check --workspace --all-targets` | **46.7s / 1074 MB** | 40.2s / 1028 MB | **+16%** | 峰值 < 1.5G ✅ |
| `cargo test --workspace --no-run` | **63.1s / 1414 MB** | 47.8s / 1118 MB | **+32%** | 峰值 < 1.5G ✅（占 94%） |
| 增量 `cargo check`（改叶子 crate 后） | **0.98s** | —— | 无明显变慢 | 日常增量 < 60s ✅ |

Dev 声称：check 55.7s/937MB、test+link 59.1s/1250MB → **耗时口径一致（我的更快/更慢都在噪声内），
峰值内存我测到的更高**（VmHWM 是求和采样，随调度波动大）—— **两边都 < 1.5G**。

### 「有没有更省的替代？」—— 我逐个查了，结论是**没有可行的**

每个依赖的**边际 crate 数**（在**交付物之外**的副本里逐个删掉再数 `cargo tree`）：

| 依赖 | 边际 crate | 能否更省 |
|------|-----------|----------|
| `jsonwebtoken` | **19** | **不能** —— 见下 |
| `chacha20poly1305` | **10** | **不能** —— 见下 |
| `totp-rs` | 5 | 不能（8 个 crate 的 TOTP 是成熟实现，不值得自制） |
| `argon2` | 3 | 不能（安全模块不省依赖是设计决策） |
| `p256` / `sha2` / `base64` / `rand` | **0** | 已经在了，不增加任何 crate |
| 8 个新直接依赖**全删** | 232 → **177** | 即 REQ-007 的依赖全部 attributable ≈ 55 个 |

**为什么 `jsonwebtoken` 不能再省**（我独立核实，不是采信 Dev）：

1. **jsonwebtoken 9.x 不行**。crates.io API 实测 `jsonwebtoken 9.3.1` 的依赖表：
   `ring ^0.17.4` 是 **`optional: false`**（非可选），对 `cfg(not(target_arch = "wasm32"))` **强制生效**；
   9.x **没有 `rust_crypto` feature**。→ 用 9.x 就等于**违反「禁 ring」约束**。
2. **jsonwebtoken 10.x 的 `rust_crypto` 是全量 feature，没有子集**。`jsonwebtoken-10.4.0/Cargo.toml` 实测：

   ```toml
   rust_crypto = ["ed25519-dalek", "hmac", "p256", "p384", "rand", "rsa", "sha2"]
   ```

   代码只用 **ES256**，但 `ed25519-dalek` / `rsa` / `p384` / `hmac` **无法关闭** —— **没有 ECDSA-only 子 feature**。
   → **Dev 的「只能整包接受」属实**。

**为什么 `chacha20poly1305` 必要**：

- TOTP secret 必须**可还原**才能验码 —— **哈希不可行**（哈希后无法比对验证码）；
  design-summary §4 里的「或至少哈希」对 TOTP secret 而言**技术上不成立**。
- 无 ring 的替代只有 `aes-gcm`（RustCrypto），子树相当或更大；`chacha20poly1305` 是 **10 个**边际 crate、纯 Rust、审计良好 —— **价格合理**。

**Argon2 参数可调**：已经是 `BASE_IAM_ARGON2_MEMORY_KIB / _ITERATIONS / _PARALLELISM`，
默认 m=19456/t=2/p=1 = **OWASP 推荐下限**；低配机可调小，**参数写在 PHC 串里，改参数不用改表结构**（AC-S-011 达标）。

### 结论

> **232 在这台机器上可以接受；但它超了 AC-S-007 写的预算（+22 个 crate），必须由 BA 显式重定基线 —— 建议改预算，不要降级依赖。**

- **可接受**：冷编译 +16%/+32%，峰值全部 < 1.5G，**增量编译 0.98s 完全没有变慢**；这 57 个 crate 里 19 个来自
  `jsonwebtoken` 的**无法关闭**的全量 crypto 后端，不是选型失误。
- **必须记账**：AC-S-020 的「预计 175 → 200~210」这个数字是在 `jsonwebtoken 10` 的 `rust_crypto`
  feature 全貌被知道**之前**估的；而 design-summary §8 明确写了「这是 Sprint 2 唯一允许引入重量级依赖的地方……
  正确做法是引成熟库 + **记入依赖预算**」。→ **把 AC-S-020 的预算重定基线到 235，并在追溯矩阵登记偏差**。
- **不建议的做法**：为了压数字换 `jsonwebtoken 9.x`（= 引入 ring，违反硬约束）、或绕开 `jsonwebtoken`
  直接用已有的 `p256`+`sha2` 手写 JWS 信封（能省约 19 个 crate，但要在**第一个安全模块**里手写 JWT 编解码 ——
  那是拿正确性换数字，**不推荐**）。

---

## 用例缺口补验

Dev 自陈「实现有、但无端到端用例」的 4 个缺口 —— **我逐个自己跑了一遍，实现全部正确**：

| 缺口 | 实现是否正确 | 证据 |
|------|------------|------|
| **1. 锁定期过后自动解锁** | ✅ **正确** | 锁定后（`failed_attempts=5`）由管理员把 `locked_until` 置为 `now()-1min`（模拟到期）→ 用**正确**密码登录 → **200**，登录成功。**但见问题 #2**：只要期间还有一次错密码，锁就被续期，永不到期。 |
| **2. 成功登录后失败计数清零** | ✅ **正确** | 3 次错密码后 `failed_attempts=3` → 正确密码登录 → DB 变 **0**；再连打 4 次错密码 → **仍 401 `INVALID_CREDENTIALS`（未锁定）**；再用正确密码 → **200**。计数确实清零。 |
| **3. IP 限流的 429 分支** | ✅ **正确** | 同一 IP 同一租户连打：第 20 次 → 401；**第 21 次 → 429 `LOGIN_RATE_LIMITED`**，`params.retryAfterSeconds=300`，`detail='too many login attempts; retry after 300 seconds'`。 |
| **4. 幂等键端到端重复请求** | ✅ **正确** | 同一 `Idempotency-Key` 连发两次 `register` → **两次都是 201 且响应体逐字节相同**（同一个 user id）；DB 里该用户名**只有 1 行**；**同一 key 换租户 → 201**（幂等键按租户维度，不跨租户共享，符合契约 C8）。 |

**结论：4 个缺口是「用例没写」，不是「实现没做」** —— 实现全部可用。这 4 条建议补成端到端用例（工作量很小）。

---

## 回归

- **Rust**：`cargo test --workspace -- --test-threads=2` → **225 passed / 0 failed / 0 ignored** ✅（基线 183 → 225）
  - 逐二进制：platform-contracts 8 · platform-core 48 · platform-db 22 · platform-iam 41 · platform-server 38+9 · sandbox 3 · contracts 4 · xtask 49
  - `cargo test --workspace --tests -- --include-ignored`（带真实库）→ **239 passed / 0 failed**（iam_real_db **11 passed**、quota_real_db 2、c3_real_db 4）
- **Lint / 格式**：`cargo clippy --workspace --all-targets -- -D warnings` → **exit 0，0 告警** ✅；`cargo fmt --all --check` → **exit 0** ✅
- **工程门禁**：`cargo xtask capabilities --check` ✅（13 个文件与源码一致）· `check-deps` ✅ · `check-modules` ✅ · `tenant-policies --check` ✅（**注意：必须在 `scripts/test-integration.sh` 跑过之后**；在陈旧 DB 上会因 `sandbox_tenant_item` 未开 RLS 而失败 —— 那是**模块表**、与 REQ-007 无关，重置库后即通过）
- **集成脚本**：`scripts/test-integration.sh` → **exit 0**，**容器数量比对通过：2 → 2** ✅（真实库用例真跑，无假绿）
- **前端单测**：`pnpm -r test` → **23 passed / 23**（8 个包：auth 3 · schema 3 · i18n 3 · hooks 3 · theme 2 · sdk 2 · ui 3 · shell 4）✅
- **前端 E2E**：`bun run test:e2e` → **1 passed / 1** ✅（chromium）
- **CODEOWNERS 保护目录**：`crates/platform-core/`、`crates/platform-contracts/`、`crates/platform-contracts-macros/` → **零改动** ✅；`crates/platform-db/` → 只加 `time` 依赖 + `BindValue` 3 个变体（`NullableText` / `Timestamp` / `NullableTimestamp`）+ 对应 `From` 实现与 bind 分支（**纯增量、无既有行为改动**）；`modules/` → **零改动** ✅

---

## 问题（按严重度）

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **HIGH** | **`ACCOUNT_LOCKED` 是账号枚举预言机**：对同一租户内任意候选名发 5 次错密码，第 6 次的响应（403 / `ACCOUNT_LOCKED`）与「账号不存在」的响应（401 / `INVALID_CREDENTIALS`）**可区分** → **6 请求/候选**即可枚举账号存在性。与 AC-S-012 的意图冲突。根因：`service.rs:312-316` 的锁定判定在密码校验**之前**，且用了不同的错误码与 HTTP 状态。`error.rs:10-17` 的「未注册账号永远拿不到 ACCOUNT_LOCKED ⇒ 不构成枚举」论证**方向反了**。 | `probe_b.py`：existing 第 6 次 → `403 ACCOUNT_LOCKED`；missing 第 6 次 → `401 INVALID_CREDENTIALS`；`DISTINGUISHABLE = True`。扩展后的 Dev 用例 TRACE：第 5 轮起 `existing=403` / `missing=401` | **阻塞合并。** 把锁定判定挪到**密码校验之后**（见 §账号枚举面判定 的完整代码）；锁定分支只记流水、不动失败计数 |
| **2** | **HIGH** | **账号锁可被无限续期 → 永久登录拒绝服务**：锁定期内每来一次错密码，`on_password_failure` 就把 `locked_until` 重算成 `now + 15min`，且 `failed_attempts` 只增不减。攻击者**每 15 分钟一次错密码**即可让受害者**永远**登不进去（连正确密码都被拒）。同时也是**未认证的账号锁死 DoS**（对任意已知用户名 5 次错密码即可触发）。 | `probe_k2.py`：到期后 1 次错密码 → `failed_attempts=6`、`locked_until` 刷新为 `now+15min` → 正确密码 → `403 ACCOUNT_LOCKED`；连续 3 轮 `locked_until` 每次前移 15 分钟 | **阻塞合并。** `on_password_failure` 里：已在锁定期 → **不续期**；锁定已到期 → 失败计数**从头开始**（见 §账号枚举面判定） |
| **3** | **MEDIUM** | **锁定路径跳过 Argon2 → 75× 时序差**：被锁账号的登录请求直接返回，**不做密码哈希**，耗时 **8.1ms**；普通失败 **606.9ms**、账号不存在 **599.3ms**。这既让「账号已锁」变成**单请求可探测**的信号（比 #1 的 6 请求更便宜），也削弱了「两条失败路径耗时对齐」的设计前提。根因同 #1（锁定判定在密码校验之前）。 | `probe_e.py`：wrong pw (unlocked) 606.9ms / wrong pw (LOCKED) **8.1ms** / wrong pw (no account) 599.3ms → **75.3×** | 随 **#1** 一并修复（密码校验挪到锁定判定之前，锁定路径也就带上 Argon2 → 耗时自然对齐） |
| **4** | **MEDIUM** | **Dev 的「防账号枚举」用例结构上看不见 #1**：① 状态码相等断言（`iam_real_db.rs:361`）只覆盖**第一次**请求，循环里第 5 轮起的 401→403 漂移**从未被断言**；② `median()`（`iam_real_db.rs:686-689`）用 `values[len/2]`，偶数长度取**上中位数**，8 个样本 `[3,3,3,6,227,232,235,252]` 的下标 4 = **227ms**，**恰好落在未锁定那一簇**，把 4 个 3~6ms 的锁定样本全丢掉 → 比值算成 1.08。⇒ **把循环从 3 轮扩到 8 轮，用例仍然报「通过」。** 这条护栏与真正的洞不是同一个方向。 | 扩展 8 轮后的 TRACE（见 §安全断言实测 I）：`existing=403 3ms \| missing=401 250ms`，用例仍 `test result: ok`、比值 1.08 | 用例必须**逐轮断言状态码** + 对**全部样本**用**真中位数**（或直接比较分布）。建议把「连续 6 次错密码 → 第 6 次仍是 `INVALID_CREDENTIALS`」与「锁定期内正确密码 → `ACCOUNT_LOCKED`」各写成一条独立用例 |
| **5** | **LOW** | **改密把「当前设备」的 refresh family 也作废了** —— 偏离 design-summary §7（「改密后踢出其他设备」= 作废**除当前以外**的全部 family）。`apply_new_password` 无条件 `revoke_all_refresh_tokens`，随后才 `revoke_other_sessions`。结果：用户在自己设备上改完密，**当前设备的 refresh 也立刻失效**，15 分钟后 access token 过期就得重新登录。方向是**偏严**（不是安全洞），属实现与设计摘要不一致。 | `probe_f.py`：改密 204 → 其它设备 refresh `401 TOKEN_REVOKED`、**当前设备 refresh 也 `401 TOKEN_REVOKED`** | 保留当前 session 的 family：`revoke_all_refresh_tokens` 改为「除当前 family 外全作废」；或至少在追溯矩阵登记偏差 |
| **6** | **LOW** | **IP 限流是「按租户」计的，且把成功登录也计数**：① 同一 IP 换租户**不受限** → 限流预算可被租户数放大；② `count_login_attempts_since` 统计的是**全部**尝试（含成功）→ 同一租户内共用一个 NAT 出口的办公网 **5 分钟内 20 次登录**就会被 429 误伤正常用户。 | `probe` 实测：租户 A 第 21 次 → `429 LOGIN_RATE_LIMITED`；**同 IP 换租户 B → 401 不受限**；租户 A 继续 → 仍 429 | 限流按 **IP 全局**（跨租户）计；计数只算**失败**尝试；或按「IP + 租户」但成功登录不计入窗口 |
| **7** | **LOW** | **依赖树超 AC-S-020 预算 22 个 crate**（实测 175 → 232，预算 200~210）。资源侧全部达标（冷 check 46.7s/1074MB、冷 test+link 63.1s/1414MB < 1.5G、增量 check 0.98s），且**无 ring**（0 命中）、**无可行的更省替代**（jsonwebtoken 9.x 强制依赖 ring；10.x 的 `rust_crypto` 是全量 feature 无子集；chacha20poly1305 对 TOTP secret 必需）。 | `cargo tree` 实测 175 → 232（+57，删 8 个新依赖 → 177）；`grep -oE "\b(ring\|aws-lc-rs\|aws-lc-sys) v"` → **0 命中**；crates.io `jsonwebtoken 9.3.1` 依赖表 `ring` 非可选；`jsonwebtoken-10.4.0/Cargo.toml` 的 `rust_crypto = [ed25519-dalek, hmac, p256, p384, rand, rsa, sha2]` | **不改代码。** 由 BA 把 AC-S-020 的预算**重定基线到 235**，并在追溯矩阵登记偏差（design-summary §8 已授权安全模块的依赖例外，且明确「引成熟库 + 记入依赖预算」）。**不要**为了压数字换 jsonwebtoken 9.x（引入 ring）或手写 JWS |

### 附：观察（不算缺陷，仅备案）

- **限流/锁定的存储与设计摘要不符**：design-summary §6 写的是「Redis 滑动窗口」+「锁定存储 Redis（快）+ DB（持久）」，
  实现是**纯 DB 计数**（`crates/platform-iam/src/store.rs:863` `count_login_attempts_since`），
  **`platform-iam` 里没有任何 Redis 代码**（`grep -i redis` → 0 命中；`base-redis` 容器在跑但 IAM 不用）。
  功能上等价且 429 分支验证通过，但**「Redis 滑动窗口」这一条未实现**；DB 计数在高并发下会成为瓶颈。
  → 建议在追溯矩阵登记偏差，或后续需求补。
- **前端没有 IAM 接入面**：`apps/shell/src/lib/nav.ts:14` 的 `{key:'iam', label:'账号与认证', path:'/api/iam'}`
  是**占位数据**（注释写明「本期为占位数据，后续由应用注册表下发」），与 `/api/rbac`、`/api/tenant` 同一形态，
  点击会落到 404。**REQ-007 本期不含前端登录页**（design-summary §9 未列入），**不计入缺陷**，但本期**没有 UI 验收面**。
- **`tenant-policies --check` 对库状态敏感**：在**未经 `scripts/test-integration.sh` 重置**的旧库上会失败
  （报 `sandbox_tenant_item` 未开 RLS）——那是**模块表**、与 REQ-007 无关，重置后即通过。
  → 建议在 CI 文档里写明「先跑 `test-integration.sh` 再跑 `tenant-policies --check`」。

---

## 未覆盖 / 无法验证

| 项 | 原因 | 说明 |
|----|------|------|
| 生产部署链路（Post-merge） | 本阶段是 Pre-merge | 镜像构建、K8s Secret 注入、`BASE_JWT_PRIVATE_KEY` 在真实 Secret 里的轮换流程未验证 |
| 前端登录/绑定 MFA 的 UI | 本期无前端接入面 | 前端只有占位导航项，**没有登录页** → 前端无法做 IAM 端到端验收 |
| 多实例/多副本下的限流与锁定一致性 | 单机单实例 | 限流是 DB 计数（见上），多实例共享 DB 时行为未实测 |
| 密钥轮换的跨进程行为 | 只在同进程内验过 | `JwtKeySet` 的 current/previous 双密钥在**单进程**验过（单测 + JWKS 实响应）；跨实例滚动重启时旧 kid 的宽限期未实测 |
| 密码有效期（180 天）与「首次登录强制改密」 | 需要造时间/状态 | `must_change_password` 字段已建模且 `register` 里默认 `false`；AC-S-009 只列了「最小长度 / 历史不重复」，「有效期」「强制改密」的**实际强制逻辑未实现/未验**（`password_changed_at` 只写不读）→ **记为未实现（已登记），不计入缺陷** |
| `BASE_IAM_SECRET_KEY` 轮换 | —— | 静态加密密钥轮换后，旧密文的 TOTP secret 不可解 —— 本期无轮换机制，**未验证** |
| 泄露密码库（HIBP） | 明确不做 | 本期只做本地 40 条小词库（`password.rs:27-68`），符合 acceptance「明确不做的」清单 |

---

## 附：本次验证的自证材料

| 材料 | 位置 | 说明 |
|------|------|------|
| 独立探针脚本 | `/tmp/qa-req007/probe_{a,b,d,e,f,k,k2}.py` | 全部基于**真实 HTTP 服务器**（`./target/debug/base serve`，连 `base_test` + `base_app` 应用角色），不是单元测试替身 |
| 冷编译实测 | `/tmp/qa-req007/coldbuild.log` | 用 Dev 自己的 `scripts/measure-cold-build.sh`（同一 VmHWM 采样口径） |
| 变异验证 | 已**还原** | `iam_real_db.rs` 的 sha256 与原始文件**逐字节一致**（`151cb0ee…59d3`），`git status` 干净 |
| 工作树状态 | `feature/REQ-007` @ `e755e5c` | `git status --porcelain` **为空**（除本报告文件）；`develop` worktree `91d8e09` 亦为空 |
| `base_test` | 已清理 | 我自己造的 `qa-%` 租户数据**全部删除**；`base_dev` **零 IAM 表、零写入**（`platform_iam_user` 表不存在）；容器 **2 个 healthy**，未新建容器 |
| 交付物 | `feature-meta/qa-report.md`（本文件） | 覆盖了上一轮归档在此路径的 **REQ-006** QA 报告（该内容仍可从 git commit `5487658` 取回；本仓库每轮 REQ 都在同一路径覆盖，属既有约定） |

**声明**：本报告中的所有数字（耗时、峰值内存、crate 数、状态码、耗时比值、容器数、测试计数）**全部由 QA Agent 自己重跑、自己计时**，
**未采信 Dev 自验报告（`feature-meta/verification-report.md`）里的任何数字**。
其中与 Dev 的说法**不一致**的两处已在正文标注：依赖树口径（实测 175 → 232，Dev 说 179 → 240）、冷编译峰值内存（实测 1074/1414MB，Dev 说 937/1250MB）。
