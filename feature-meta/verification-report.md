# REQ-007 验证报告

- **需求**: REQ-007 认证与账号体系
- **分支**: `feature/REQ-007` @ `6ad3f95`（实现提交 `c66605f` + 文档提交 `6ad3f95`）
- **基线**: `develop` @ `91d8e09`
- **设计依据**: `BA/demands/REQ-007-auth-account/design-summary.md`（D1–D11 照做）
- **契约**: api-conventions v1（C3 / C6 / C8 / C13）、extension-points v1
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: 第一轮（开发完成 → 待 QA 验证）

---

## 0. 一页速览

| 项 | 结果 |
|----|------|
| 分支 / 提交 | `feature/REQ-007` @ `6ad3f95` |
| 改动 | **29 文件 / +7806 -191 行**；**新建 crate `crates/platform-iam`**（9 个源文件 + 1 个真实库测试文件） |
| 新增第三方依赖 | `argon2` 0.5.3（Argon2id）、`jsonwebtoken` 10.4.0（**`rust_crypto` 后端**）、`totp-rs` 5.7.2、`chacha20poly1305` 0.11.0、`p256` 0.13.2、`base64` 0.22.1、`sha2` 0.10.9、`rand` 0.8.8 |
| **ring / aws-lc-rs** | **未引入** —— `cargo tree --workspace --all-features` 里 `ring v…` / `aws-lc-rs v…` **0 命中**（唯一含 “ring” 字样的是 `stringprep`，Unicode 库，非 ring 加密库） |
| 依赖树 | **179 → 240 个 crate**（工作区成员 7 → 8；**第三方 172 → 232，+60**）—— **超出预算**（预计 ≈215 / AC-S-020 写 200~210），原因与影响见 §5 |
| 冷编译实测 | `cargo check --workspace --all-targets` **55.7s / 峰值 937 MB**；`cargo test --workspace --no-run`（含链接）**59.1s / 峰值 1250 MB** → **峰值 < 1.5G，`.cargo/config.toml` 的 `jobs` 保持 3** |
| Rust 测试 | **225 passed / 0 failed**（REQ-006 基线 **183 个全绿**，**+42 只增不减**）；**19 ignored**（9 个原有 + **10 个本期真实库用例**，由集成脚本 `--include-ignored` 跑到，实测 **11/11** 含护栏） |
| 前端测试 | **23/23**（`pnpm -r test`） |
| E2E | **1/1**（`bun run test:e2e`，chromium） |
| lint | `cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** |
| xtask 门禁 | `capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅ |
| ★ 防账号枚举 | ✅ 错误码与响应体**逐字段相同**；耗时 **248.7ms vs 219.2ms（比值 1.13）** |
| ★ 防重置枚举 | ✅ 两个响应体 **JSON 逐字段相等** |
| ★ 轮换与重放 | ✅ 旧 refresh 立即失效；重放 → `TOKEN_REUSE_DETECTED` → **整个 family 作废** |
| ★ TOTP 防重放 | ✅ 同一时间步第二次 → `MFA_INVALID`；恢复码第二次 → `MFA_INVALID` |
| ★ 设备会话 | ✅ 从 A 下线 B → B 的 refresh **立即失效**；登出全端 → 全部 family 作废 |
| ★ 租户隔离 | ✅ A 的令牌 + B 的租户头 → **404**；会话记录**全部带 `tenant_id`** |
| ★ JWKS | ✅ **只暴露公钥**（逐成员检查）；轮换后新旧 `kid` 都在 |
| ★ 错误体合规 | ✅ 逐个错误路径的 `detail` 都是**英文 ASCII** |
| ★ 防假绿护栏 | ✅ 缺 `DATABASE_URL_TEST` → 子进程**非零退出** + 输出含 `0 passed`；**护栏经变异验证承重**（§6） |
| **未完成 / 未验证** | 见 §7（如实登记） |

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

```
情况一（账号存在 + 密码错误）：HTTP 401
  {"code":"INVALID_CREDENTIALS","detail":"invalid username or password", …}
情况二（账号不存在）        ：HTTP 401
  {"code":"INVALID_CREDENTIALS","detail":"invalid username or password", …}
```

- `code` / `detail` / `type` / `title` / `status` **逐字段相同**（断言 `assert_eq!`）
- `detail` 与设计摘要 §5.1 **逐字一致**
- **耗时对比（多轮取中位数）**：

| 情况 | 耗时（中位数） |
|---|---|
| 账号存在 + 密码错误 | **248.7 ms** |
| 账号不存在 | **219.2 ms** |
| **比值** | **1.13**（断言阈值 1.5） |

**机制（两道）**：
1. **假哈希校验**：账号不存在时也跑一次 Argon2id 校验，**用与真密码完全相同的参数**
   （`DummyHash` 在服务构造时按配置参数生成一次，之后复用）；
2. **同样的数据库工作量**：两条失败路径走**同样的两条语句**（一次 UPDATE + 一次 INSERT，
   同一个事务）—— 账号不存在时 UPDATE 命中 0 行。

> ⚠️ 开发过程中曾出现两个**真实缺陷**，都是被上面的用例抓出来的：
> ① INSERT 的列序与 `{tenant}` 占位符位置不匹配（`tenant_id` 被写成了 id 值）→ RLS `WITH CHECK` 拒绝；
> ② 重放检测后 family 作废走了 **rollback** → 「检测到泄露却什么都没作废」。

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
cargo tree --workspace --prefix none --all-features | sed 's/ (.*//' | sort -u | wc -l
→ 240（基线 179）
```

| 项 | 数量 |
|---|---|
| 工作区成员 | 7 → **8**（新增 `platform-iam`） |
| 第三方 crate | 172 → **232**（**+60**） |

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

| 项 | 结果 |
|---|---|
| `cargo check --workspace --all-targets` | ✅ |
| `cargo test --workspace -- --test-threads=2` | ✅ **225 passed / 0 failed / 19 ignored**（REQ-006 基线 **183 个全绿**，**+42 只增不减**） |
| `bash scripts/test-integration.sh`（`--include-ignored`，重置 `base_test`） | ✅ **exit 0**；**真实库用例 11/11**；容器数量 **2 → 2**（未新建容器） |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ **0 告警** |
| `cargo fmt --all --check` | ✅ |
| `pnpm -r test` | ✅ **23/23** |
| `bun run test:e2e` | ✅ **1/1**（chromium，5.2s） |
| `cargo xtask capabilities --check` | ✅ 13 个文件与源码一致 |
| `cargo xtask check-deps` | ✅ 平台 crate 未反向依赖业务模块（`platform-iam` 自动纳管） |
| `cargo xtask check-modules` | ✅ 1 个模块（sandbox） |
| `cargo xtask tenant-policies --check` | ✅ 模块声明的表都开了 RLS 且有策略 |

**新增用例**：41 条单测（`platform-iam`）+ 11 条真实库层用例（含 1 条防假绿护栏）。

---

## 5. 偏离清单（如实登记）

| # | 偏离 | 原计划 | 实际 | 原因与影响 |
|---|------|--------|------|-----------|
| 1 | **依赖树超出预算** | 179 → ≈215（AC-S-020 写 200~210） | **179 → 240**（第三方 +60） | ① `jsonwebtoken` 10 的 `rust_crypto` feature 是**全量**的（`ed25519-dalek` + `p256` + `p384` + `rsa` + `hmac` + `sha2`），**不能只挑 P-256**；jsonwebtoken 9.x 的默认后端是 **ring**（要编 C/汇编，任务明确禁止）→ 只能整包接受；② `chacha20poly1305` + 它的 AEAD 依赖（aead / cipher / poly1305 / universal-hash …）**+14**；③ `sqlx` 打开 `time` feature → `time-macros`（proc-macro，+1）。**影响**：冷编译峰值仍 **< 1.5G**（§3.2），但增量编译时间变长 |
| 2 | **按 IP 的限流用 DB 计数而不是 Redis** | 设计摘要 §6：Redis 滑动窗口 | DB 滑动窗口（`occurred_at >= now() - window`） | 任务书明确允许「登录限流/配额若需 DB 计数则复用 `platform-db` 的配额能力」；`redis` 客户端会再加一批依赖（与依赖预算冲突）。**语义等价**（都是滑动窗口），**存储位置不同** |
| 3 | **登录失败计数写在用户表** | 设计摘要 §6：Redis 快计数 + DB 持久 | 用户表的 `failed_attempts` + `locked_until` | 同上；账号锁定必须**持久**（重启不能丢），DB 是唯一可靠位置；快计数层（Redis）留待后续 |
| 4 | **恢复码用 SHA-256 而不是慢哈希** | 设计摘要 §4：只说「存 hash」 | SHA-256（base64url） | 恢复码是 **80 位熵**的高熵值，快哈希就够（慢哈希只对低熵的密码必要）。已在代码注释里写明口径 |
| 5 | **`title` 用中文、`detail` 用英文** | 契约 C6 | `title` = 中文类别名，`detail` = **英文 ASCII** | 契约 C6 的**示例**里 `title` 就是中文（「权限不足」）；仓库既有实现（REQ-005 的 `ProblemDetails`）也是中文 `title` + 英文 `detail`。**约束对象是 `detail`**（AC-S-019 明写） |
| 6 | **扩展了 `platform-db`** | REQ-006 的数据访问层 | `BindValue` 新增 `Timestamp` / `NullableText` / `NullableTimestamp`；`sqlx` 打开 `time` feature | 令牌过期、重置令牌过期、账号锁定到期都要写时间戳。`platform-core` / `platform-contracts`（CODEOWNERS 保护目录）**一行未改** |
| 7 | **平台表不走 manifest 的 `tenantScoped`** | REQ-006 的统一生成机制 | IAM 表在**平台迁移**里直接建 RLS policy | 账号体系是底座自身的身份权威源（ADR-0001 A3），属**平台表**（与 `platform_tenant_config` / `platform_tenant_quota` 同类），不是业务模块表 |

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

| 项 | 状态 | 说明 |
|---|---|---|
| 锁定期**过后**自动解锁 | **未写自动化用例** | 实现上 `locked_until` 是时间戳，判定用 `locked_until > now()`，到期自然解锁；**没有**用例真的等过锁定期。属**用例缺口** |
| 「成功登录后失败计数清零」 | **未写自动化用例** | 实现上登录成功调 `reset_login_failures`（清零 + 解锁）；**没有**端到端断言 |
| 按 IP 滑动窗口限流的 **429 分支** | **未写自动化用例** | 实现上按 `occurred_at >= now() - window` 统计（真滑动窗口）；`LOGIN_RATE_LIMITED` 的 429 **没有**端到端用例 |
| 幂等键（C8）的端到端行为 | **未写自动化用例** | 中间件已实现（认领 → 重复请求返回首次结果 / 处理中 409）；**没有**断言「同一 key 两次 POST 返回同一响应体」 |
| access token 的**即时**撤销 | **本期不做**（设计取舍） | 15 分钟短 TTL 的自包含 JWT；撤销能力由 refresh token + 会话承担。改密码后旧 access token 仍可用 ≤ 15 分钟 —— **已知取舍**（设计摘要 R4） |
| `ACCOUNT_LOCKED` / `ACCOUNT_DISABLED` 的枚举面 | **已知取舍** | AC-S-017 要求锁定账号返回 `ACCOUNT_LOCKED`，与「不存在 → `INVALID_CREDENTIALS`」在错误码上可区分。缓解：锁定需 **5 次失败** + 同 IP 滑动窗口限流；`ACCOUNT_DISABLED` 只在**密码正确之后**才提示 |
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
