# REQ-007 追溯性矩阵

- **需求**: REQ-007 认证与账号体系
- **分支**: `feature/REQ-007`
- **基线**: `develop` @ `91d8e09`（REQ-002 工程基线 + REQ-003 扩展点框架 + REQ-004 AI 引导体系 + REQ-005 交付基线 + REQ-006 多租户与隔离已合并）
- **设计依据**: `BA/demands/REQ-007-auth-account/design-summary.md`（D1–D11 已定死，本轮照它做）
- **契约**: api-conventions v1（尤其 C3 / C6 / C8 / C13）、extension-points v1
- **开发者**: myg133-dev-1
- **更新时间**: 2026-11-18
- **本轮改动**: **29 文件 / +7806 -191 行**（提交 `c66605f` 实现 + `6ad3f95` 文档）

图例：**PASS** = 已实现且有自动化或可复现的验证证据；**未验证** = 没有证据，如实标注；
**部分** = 本期只做了一部分（范围外的部分写清楚）。

---

## 1. 自有 AC（AC-S-009 ~ AC-S-020）

| AC | 内容 | 实现位置 | 验证方式与证据 | 结论 |
|----|------|----------|----------------|------|
| AC-S-009 | 密码策略可配置：不满足时拒绝并指出**具体违反哪条规则**（`PASSWORD_POLICY_VIOLATION` + `params`）；与最近 5 次重复被拒（`PASSWORD_REUSED`） | `crates/platform-iam/src/config.rs`（`PasswordPolicySettings` + `check_password_policy`）、`crates/platform-iam/src/password.rs`（`is_common_password` 本地词库）、`crates/platform-iam/src/service.rs`（`apply_new_password` 查历史）、`crates/platform-iam/src/error.rs`（`PolicyViolation`） | ① 单测：`config.rs::common_passwords_are_rejected_by_the_policy`（命中 `common_password` 规则）、`password_policy_violations_name_the_rule`（`params.violations[].rule`）；② **真实库层**（`crates/platform-iam/tests/iam_real_db.rs::changing_the_password_revokes_the_other_families_and_rejects_history`）：弱密码 → **422** + `code=PASSWORD_POLICY_VIOLATION` + `params.violations[0].rule == "min_length"`；改回历史密码 → **422** + `code=PASSWORD_REUSED`；③ 参数可配：`config.rs::parameters_are_configurable`（`BASE_IAM_PASSWORD_MIN_LENGTH` 等） | **PASS** |
| AC-S-010 | 密钥轮换不中断：轮换后新令牌用 K2 签发可验签；宽限期内旧令牌（K1）仍可用；JWKS 同时暴露两个公钥；**私钥绝不出现在 JWKS** | `crates/platform-iam/src/jwt.rs`（`JwtKeySet`：当前 + 上一把、`entry_by_kid`）、`crates/platform-iam/src/config.rs`（`BASE_JWT_PREVIOUS_*`）、`crates/platform-iam/src/http.rs`（`jwks` 端点） | ① 单测 `jwt.rs::rotation_keeps_the_previous_key_valid_and_publishes_both_kids`：旧密钥签发的令牌在宽限期内仍能验签 + JWKS 有 **2 个 kid**；② 单测 `jwt.rs::the_jwks_never_contains_private_key_material`：**逐个 key 成员检查**，不得出现 `d`/`p`/`q`/`dp`/`dq`/`qi`/`k`（EC 私钥的 `d`、RSA 私钥成员）；③ 真实库层 `the_jwks_endpoint_only_exposes_public_keys`：HTTP 端点返回体里无 `PRIVATE`、无私钥成员；④ 私钥**只从环境注入**：`config.rs::placeholder_keys_are_rejected`（`changeme` / `replace_me` / `example` 等占位符 → 启动 fail-fast） | **PASS** |
| AC-S-011 | 密码哈希不可逆且参数可配：存储值不含明文、是 `$argon2id$...` 的 PHC 串（算法自描述）、参数可按目标机器配置 | `crates/platform-iam/src/password.rs`（`hash_password` / `verify_password`）、`crates/platform-iam/src/config.rs`（`Argon2Settings`）、`migrations/20261118000400_iam_auth.up.sql`（`password_hash` 列） | ① 单测 `password.rs::the_stored_value_is_a_phc_string_and_hides_the_plaintext`：以 `$argon2id$` 开头、含 `m=64,t=1,p=1`、**不含明文**；② 单测 `a_hash_made_with_other_parameters_still_verifies`：换参数哈希同一密码仍能验签（**算法自描述** → 换参数不必改表结构）；③ 真实库层：`platform_iam_user.password_hash` 存 PHC 串（表结构见迁移文件）；④ 参数可配：`config.rs::a_complete_environment_produces_the_default_policy`（默认 19456 KiB / t=2 / p=1 = OWASP 下限） | **PASS** |
| AC-S-012 | 登录失败不泄露账号是否存在：两种情况**响应体与错误码完全相同**（`INVALID_CREDENTIALS`）；耗时**接近** | `crates/platform-iam/src/service.rs`（`login`：账号不存在走 `DummyHash::verify` + **同样的数据库工作量**）、`crates/platform-iam/src/password.rs`（`DummyHash`：与真密码**同参数**的假哈希） | 真实库层 `login_failure_does_not_reveal_whether_the_account_exists`：① 两种情况 **401** + `code=INVALID_CREDENTIALS` + `detail` / `type` / `title` / `status` **逐字段相同**；② **耗时对比（多轮取中位数）**：存在账号+错密码 = **248.7ms** / 账号不存在 = **219.2ms** → **比值 1.13**（阈值 1.5）。机制：账号不存在时也跑一次**同参数**的 Argon2id 假哈希校验，并走**同样的两条数据库语句**（一次 UPDATE + 一次 INSERT，同一事务） | **PASS** |
| AC-S-013 | 密码重置不泄露邮箱是否存在：已注册与未注册邮箱**响应完全相同** | `crates/platform-iam/src/service.rs`（`forgot_password` 返回 `Option`；未注册时**什么都不做**）、`crates/platform-iam/src/http.rs`（`forgot_password` 处理器：**两种情况都返回同一个 `{"status":"accepted"}`**，令牌走邮件通道不进响应体） | 真实库层 `password_forgot_does_not_reveal_whether_the_email_is_registered`：两个响应体 **JSON 逐字段相等**（`assert_eq!(body_registered, body_unregistered)`），状态码相同 | **PASS** |
| AC-S-014 | refresh token 轮换与重放检测：旧 refresh **立即失效**；重放 → **整个 family 作废**（`TOKEN_REUSE_DETECTED`）；refresh_2 也已失效 | `crates/platform-iam/src/service.rs`（`refresh`：轮换与作废在**同一个事务**里）、`crates/platform-iam/src/store.rs`（`find_refresh_by_hash_for_update_in` 带 `FOR UPDATE` 行锁、`mark_refresh_used_in`、`revoke_family_in`） | 真实库层 `refresh_rotation_revokes_the_whole_family_on_reuse`：① refresh_1 → refresh_2（**新令牌 ≠ 旧令牌**）；② **再次**用 refresh_1 → **401** + `code=TOKEN_REUSE_DETECTED`；③ refresh_2 → **401** + `code=TOKEN_REVOKED`（**family 确实被作废**）。⚠️ 实现细节：family 作废必须**提交**事务（回滚 = 「检测到泄露却什么都没作废」）—— 该缺陷在开发期被本用例抓到并已修 | **PASS** |
| AC-S-015 | TOTP 防重放与恢复码一次性：同一时间步的验证码**不能用第二次**；恢复码**只能用一次** | `crates/platform-iam/src/totp.rs`（`verify_code` 返回命中时间步并与 `last_used_step` 比对）、`crates/platform-iam/src/store.rs`（`set_mfa_last_step`、`mark_recovery_code_used`）、`crates/platform-iam/src/service.rs`（`verify_mfa_code`） | 真实库层 `totp_codes_cannot_be_replayed_and_recovery_codes_are_single_use`：① 绑定后**同一个验证码再用一次** → **401** + `code=MFA_INVALID`；② 恢复码第一次可用（**204**）、第二次 → **401** + `code=MFA_INVALID`；③ 单测 `totp.rs::the_same_code_cannot_be_used_twice`（纯函数层同样守住） | **PASS** |
| AC-S-016 | 会话带租户维度且不可跨租户复用：跨租户访问被拒（404/403）；**所有会话记录都带 tenant_id** | `crates/platform-iam/src/store.rs`（全部 9 张表都带 `tenant_id`，查询一律 `WHERE tenant_id = {tenant}`）、`crates/platform-iam/src/http.rs`（`Authenticated` 提取器：令牌租户 ≠ 请求头租户 → **404**）、`crates/platform-iam/src/jwt.rs`（`tid` 声明）、`migrations/20261118000400_iam_auth.up.sql`（每张表 RLS + `WITH CHECK`） | 真实库层 `tokens_cannot_cross_tenants_and_sessions_record_the_tenant`：① A 租户的令牌 + B 租户头 → **404**（不泄露资源存在性）；② A 的 refresh token 到 B 租户刷新 → **401**；③ **直查库**：每条 `platform_iam_session` 记录的 `tenant_id` 都非空（断言全部以 `iam-test-` 开头） | **PASS** |
| AC-S-017 | 登录限流与账号锁定：达阈值后锁定并返回 `ACCOUNT_LOCKED`；锁定期内即使密码正确也拒；成功登录后计数清零 | `crates/platform-iam/src/service.rs`（`check_ip_rate_limit` 滑动窗口、`on_password_failure` 累加与锁定、`reset_login_failures`）、`crates/platform-iam/src/store.rs`（`count_login_attempts_since` 按时间倒推 = **滑动窗口**）、`crates/platform-iam/src/config.rs`（`LoginSettings`） | 真实库层 `repeated_failures_lock_the_account`：连续 5 次失败 → 第 6 次用**正确密码** → **403** + `code=ACCOUNT_LOCKED`。⚠️ 阈值与锁定时长可配（`BASE_IAM_LOGIN_ACCOUNT_LIMIT` / `BASE_IAM_ACCOUNT_LOCKOUT_MINUTES`）。**未验证项**：锁定期**过后**自动解锁与「成功登录后计数清零」两条**未写自动化用例**（见 §4） | **部分**（见 §4） |
| AC-S-018 | 设备会话可管理：列表展示设备 / IP / 最近活跃；从 A 下线 B 后 **B 的 refresh token 立即失效**；「登出全端」作废该用户全部 family | `crates/platform-iam/src/service.rs`（`list_sessions` / `revoke_session` / `logout_all`）、`crates/platform-iam/src/store.rs`（`revoke_family` / `revoke_all_sessions` / `revoke_all_refresh_tokens`） | 真实库层 `sessions_can_be_revoked_and_logout_all_revokes_every_family`：① 列表 **2 条**，每条都有 `deviceLabel` / `ip` / `lastUsedAt`；② 逐个踢下线直到 **B 的 refresh 失效**（**立即**失效）；③ 登出全端 → **A 的 refresh 也失效**（全部 family 作废） | **PASS** |
| AC-S-019 | 错误体符合契约 C6：`detail` **必须是英文 ASCII**；中文文案由前端 i18n 按 `code` 渲染 | `crates/platform-iam/src/error.rs`（每个错误的 `detail()` / `code()` / `status()`）、`crates/platform-iam/src/http.rs`（`ProblemDetails`：RFC 9457 形状 + `params`） | ① 单测 `error.rs::every_detail_is_english_ascii`（**全部 17 个错误路径**逐个断言 ASCII）；② 单测 `every_error_code_is_upper_snake_case`；③ 真实库层 `every_error_path_returns_an_english_ascii_detail`：**逐个 HTTP 错误路径**实测并打印 `detail`（`INVALID_CREDENTIALS` → `invalid username or password`；`PASSWORD_POLICY_VIOLATION` → `the password does not satisfy the password policy: min_length`；`UNAUTHENTICATED` → `authentication is required`；`TENANT_CONTEXT_MISSING` → `the tenant context is missing`）；④ `detail` 与设计摘要 §5.1 **逐字一致**（单测 `invalid_credentials_detail_matches_the_design_summary`） | **PASS** |
| AC-S-020 | 依赖预算与编译资源在可控范围：依赖树增长在预算内；**冷编译峰值内存 < 1.5G**；日常增量 `cargo check` < 60 秒 | 根 `Cargo.toml`（依赖声明与注释）、`crates/platform-iam/Cargo.toml` | 见验证报告 §3.1：**依赖树 179 → 240**（**超出预估的 200~210**，原因见 §5 偏离清单）；**冷编译实测**见验证报告 §3.2；`.cargo/config.toml` 的 `jobs` **保持 3**（峰值 < 1.5G 时不下调） | **部分**（依赖树超预估，见 §5） |

## 2. 承接的父 AC（REQ-001）

| 父 AC | 内容 | 实现位置 | 验证方式与证据 | 结论 |
|-------|------|----------|----------------|------|
| AC-003 | 注册策略可配置且**按租户隔离** | `crates/platform-iam/src/service.rs`（`register`）、`crates/platform-iam/src/store.rs`（`insert_user`：`UNIQUE (tenant_id, username)`）、`migrations/20261118000400_iam_auth.up.sql`（RLS） | ① 注册数据**按租户隔离**：用户名在租户内唯一（跨租户同名可共存）；② 密码策略**可配**（平台级，见 AC-S-009）。⚠️ **租户级**的注册开关与域名白名单属 REQ-010（需求文档「不在范围内」明列），本期**未做** | **部分**（租户级开关属 REQ-010） |
| AC-004 | 邀请链接**一次性且可过期** | `crates/platform-iam/src/service.rs`（`reset_password`：一次性 + 过期判定）、`crates/platform-iam/src/store.rs`（`mark_password_reset_used` / `find_password_reset`） | 本期落地的是**密码重置令牌**（一次性 + 可过期，模式与邀请链接一致）。⚠️ **邀请链接本身**属 REQ-010 的注册策略（域名白名单、审批待定态），本期**未做** | **部分**（邀请链接属 REQ-010） |
| AC-007 | 令牌轮换与撤销：旧 refresh 立即不可用；登出后令牌不可用；状态码 401 | `crates/platform-iam/src/service.rs`（`refresh` / `logout` / `logout_all`）、`crates/platform-iam/src/store.rs` | ① 轮换 → 旧令牌立即不可用（AC-S-014 用例）；② 登出 → 该会话的 family 作废（AC-S-018 用例）；③ 状态码 **401**（同上两处实测） | **PASS** |
| AC-008 | 设备会话可见且可远程下线：列表展示两台设备；从 A 端下线 B → **B 端下一次请求被强制登出** | 同 AC-S-018 | 真实库层实测：列表 2 条；从 A 端下线 B → **B 的 refresh 立即失效**（401）。⚠️ 「B 端下一次请求被强制登出」对 **refresh** 路径已验；**access token** 走 15 分钟短 TTL（设计摘要 §2 与 R4 的取舍），本期**不做** access token 的即时撤销检查 | **部分**（access token 即时撤销属设计取舍，见 §4） |

## 3. 交付清单（设计摘要 §9「本期交付边界」）

| 交付项 | 实现位置 | 验证方式与证据 | 结论 |
|--------|----------|----------------|------|
| 本地账号：注册、登录、登出、登出全端 | `service.rs`（`register` / `login` / `logout` / `logout_all`）、`http.rs`（对应路由） | 真实库层：注册 → 201、登录 → 200 + 令牌对、登出 / 登出全端 → 令牌失效 | **PASS** |
| 密码：Argon2id + PHC + 可配策略 + 本地词库 | `password.rs`、`config.rs` | AC-S-009 / AC-S-011 | **PASS** |
| 密码找回与重置（防枚举） | `service.rs`（`forgot_password` / `reset_password`） | AC-S-013 | **PASS** |
| 混合令牌 + 轮换 + 重放检测 + family 作废 | `jwt.rs`、`service.rs`（`refresh`） | AC-S-014 | **PASS** |
| JWT：密钥注入、`kid`、JWKS、轮换宽限期 | `jwt.rs`、`config.rs`、`http.rs`（`jwks`） | AC-S-010 | **PASS** |
| TOTP：绑定、校验、防重放、恢复码 | `totp.rs`、`service.rs` | AC-S-015 | **PASS** |
| **WebAuthn 只留接口** | `crates/platform-iam/src/totp.rs`（`MfaMethod::WebAuthn` → `NOT_IMPLEMENTED`） | 单测 `webauthn_is_declared_but_not_implemented` | **PASS** |
| 登录限流 + 账号锁定 | `service.rs`（`check_ip_rate_limit` / `on_password_failure`）、`store.rs` | AC-S-017（部分，见 §4） | **部分** |
| 设备会话：列表、踢下线、全端失效 | `service.rs`、`store.rs` | AC-S-018 | **PASS** |
| **全部会话与令牌带 tenant_id** | 全部 9 张表 + `AccessClaims.tid` + `Authenticated` 提取器 | AC-S-016 | **PASS** |
| 错误码统一（防账号枚举） | `error.rs` | AC-S-019 | **PASS** |
| **本期新增：第一个真实 HTTP 业务路由 + 租户上下文中间件** | `crates/platform-iam/src/http.rs`（12 个 `/api/iam/*` 路由 + `/.well-known/jwks.json` + `tenant_context` 中间件）、`crates/platform-server/src/main.rs`（挂载） | 真实库层全部走真实 HTTP 路由（`tower::ServiceExt::oneshot` 驱动 axum Router，不连网络）；租户头 → `RequestContext` → `SET LOCAL app.tenant_id` → RLS **闭环**（AC-S-012 的前提） | **PASS** |

## 4. 如实登记：未验证 / 未做 / 有取舍的部分

| 项 | 状态 | 说明 |
|----|------|------|
| 锁定期**过后**自动解锁 | **未写自动化用例** | 实现上 `locked_until` 是时间戳，登录判定用 `locked_until > now()`（`service.rs::is_locked`），到期后自然不再锁定；但**没有**用例真的等过锁定期（本机不划算）。属**用例缺口**，不是实现缺口 |
| 「成功登录后失败计数清零」 | **未写自动化用例** | 实现上登录成功调 `reset_login_failures`（清零 + 解除锁定）；**没有**用例断言「先失败 4 次 → 成功登录 → 再失败 1 次不锁定」。属**用例缺口** |
| 按 IP 的滑动窗口限流 | **未写自动化用例** | 实现上按 `occurred_at >= now() - window` 统计（`count_login_attempts_since`，真滑动窗口）；`LOGIN_RATE_LIMITED` 的 429 分支**没有**端到端用例。属**用例缺口** |
| 幂等键（C8）的端到端行为 | **未写自动化用例** | 中间件已实现（认领 → 重复请求返回首次结果 / 处理中返回 409）；**没有**用例断言「同一 key 两次 POST 返回同一响应体」。属**用例缺口** |
| access token 的**即时**撤销 | **本期不做**（设计取舍） | 设计摘要 R4：access token 是 15 分钟短 TTL 的自包含 JWT，撤销能力由 refresh token 与会话承担；「改密码后旧 access token 仍可用 ≤ 15 分钟」是**已知取舍**，本期不引入逐请求查库 |
| `ACCOUNT_LOCKED` / `ACCOUNT_DISABLED` 的枚举面 | **已知取舍** | 需求与设计摘要要求锁定账号返回 `ACCOUNT_LOCKED`（AC-S-017），这与「账号不存在返回 `INVALID_CREDENTIALS`」在**错误码**上可区分。缓解：达到锁定阈值需要 **5 次失败**且同 IP 有滑动窗口限流；`ACCOUNT_DISABLED` 只在**密码正确之后**才提示 |
| 租户级注册策略（开关 / 域名白名单 / 审批态） | **不做** | 需求文档「不在范围内」明列（REQ-010） |
| 邀请链接 | **不做** | 同上（REQ-010） |
| WebAuthn / FIDO2 实现 | **不做** | 设计摘要 §9：只留接口（`MfaMethod::WebAuthn` → `NOT_IMPLEMENTED`） |
| 短信验证码、图形验证码 | **不做** | 设计摘要 §9；限流与锁定已覆盖「登录失败限流」这条需求 |
| 完整 HIBP k-anonymity 泄露库接入 | **不做** | 设计摘要 §9：本期只做本地小词库 + 长度策略 |
| SSO 入站、企业目录同步 | **不做** | 属 REQ-013 / REQ-008 |

## 5. 偏离清单

| 偏离 | 原计划 | 实际 | 原因与影响 |
|------|--------|------|-----------|
| **依赖树超出预算** | 179 → 约 215（AC-S-020 写 200~210） | **179 → 240**（+61） | `jsonwebtoken` 10 的 `rust_crypto` feature 是**全量**的（`ed25519-dalek` + `p256` + `p384` + `rsa` + `hmac` + `sha2`），不能只挑 P-256；jsonwebtoken 9.x 的默认后端是 **ring**（要编 C/汇编，任务明确禁止）。另加 `chacha20poly1305`（TOTP secret 静态加密，设计摘要 §3「安全模块不省依赖，自制密码学绝不可接受」）。**影响**：冷编译峰值仍在 1.5G 以内（见验证报告 §3.2），但增量编译时间变长 |
| **按 IP 的限流用 DB 计数而不是 Redis** | 设计摘要 §6：Redis 滑动窗口 | DB 滑动窗口（`occurred_at >= now() - window`） | 任务书明确允许「登录限流/配额若需 DB 计数则复用 `platform-db` 的配额能力」；引入 `redis` 客户端会再加一批依赖（与依赖预算冲突）。**语义等价**（都是滑动窗口），**存储位置不同**（DB vs Redis） |
| **登录失败计数写在用户表而不是独立表** | 设计摘要 §6：Redis 快计数 + DB 持久 | 用户表的 `failed_attempts` + `locked_until` | 同上；账号锁定必须**持久**（重启不能丢），DB 是唯一可靠位置；快计数层（Redis）留待后续 |
| **恢复码用 SHA-256 而不是慢哈希** | 设计摘要 §4：只说「存 hash」 | SHA-256（base64url） | 恢复码是 **80 位熵**（16 字符 × 5 bit）的高熵值，快哈希就够（慢哈希只对低熵的密码必要）。已在代码注释里写明口径 |
| **`title` 用中文、`detail` 用英文** | 契约 C6：`detail` 是英文描述 | `title` = 中文类别名（如「凭证无效」），`detail` = **英文 ASCII** | 契约 C6 的**示例**里 `title` 就是中文（「权限不足」）；仓库既有实现（REQ-005 的 `ProblemDetails`）也是中文 `title` + 英文 `detail`。**约束对象是 `detail`**（AC-S-019 明写「`detail` 必须是英文 ASCII」） |
| **`platform-db` 被扩展** | REQ-006 的数据访问层 | `BindValue` 新增 `Timestamp` / `NullableText` / `NullableTimestamp`；`sqlx` 打开 `time` feature | 令牌过期、重置令牌过期、账号锁定到期都要写时间戳；`platform-db` 的 `BindValue` 原本只有标量。`platform-core` / `platform-contracts` **一行未改**（CODEOWNERS 保护目录） |

## 6. 改动统计与产物

- **新增 crate**：`crates/platform-iam`（平台 crate）
- **新增平台表**：9 张（`platform_iam_user` / `_password_history` / `_session` / `_refresh_token` / `_mfa_secret` / `_recovery_code` / `_password_reset` / `_login_attempt` / `_idempotency`），全部带 `tenant_id` + RLS
- **新增路由**：12 个 `/api/iam/*` + 1 个 `/.well-known/jwks.json`
- **新增用例**：41 条单测 + 11 条真实库层用例（含 1 条防假绿护栏）
- **产物**：`feature-meta/traceability.md`（本文件）、`feature-meta/verification-report.md`、`feature-meta/status.md`
