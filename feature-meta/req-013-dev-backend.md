# REQ-013 后端自验报告（SSO 入站：OIDC + LDAP + 强制 SSO）

- **角色**：后端 Dev
- **worktree**：`feature-REQ-013`（分支 `feature/REQ-013`）
- **规格版本**：`acceptance.md` **rev 3** + **rev 3.1 补充（P15 / design §3.3）**
- **验收依据**：`BA/demands/REQ-013-sso/acceptance.md`（12 条 AC）
- **本轮只改后端**：`crates/**`、`migrations/**`、`scripts/seed-e2e.sh`
- **未提交 git**（按 BA 统一提交的约定）

---

## 1. 交付清单

### 1.1 新建 `crates/platform-iam/src/sso/`

| 文件 | 内容 |
|---|---|
| `mod.rs` | `AuthProvider`（local/oidc/ldap）、`SsoProviderView`、5 个端点路径常量 |
| `config.rs` | 12 个配置键、`SsoConfig::validate()`、`SsoConfigSource` 后设槽、`SsoConfigOutcome`（**读失败 fail-closed**） |
| `oidc.rs` | 发现文档 + JWKS 缓存、`https_or_http()` 出站、PKCE(S256)、算法白名单、claims 校验、code 交换 |
| `ldap.rs` | **`SsoLdapClient` / `SsoLdapConn` trait**（P12）、`ldap3` 生产实现（`.success()` 判 rc）、RFC 4515 转义、空密码判定 |
| `binding.rs` | `DO NOTHING` + 读回比对（P7）、mail 匹配、JIT 建号（P8）、永不匹配的占位 PHC 串 |
| `audit.rs` | **`SsoAuditEvent` + `SsoAuditRecorder`**（P14 自建）、`Auditable` 声明面、`NoopSsoAuditRecorder`（每次都 warn） |
| `ticket.rs` | loginTicket 生成 / 消费（P4），TTL 30 秒、一次性、绑租户、**只存 session_id** |

### 1.2 改动

| 文件 | 改动 |
|---|---|
| `platform-iam/src/error.rs` | 21 个新码的 **8 处全改**（常量 / 变体 / `code()` / `status()` / `detail()` / `params()` / `ALL_CODES` / 两个测试数组） |
| `platform-iam/src/service.rs` | 强制 SSO 判定（**加载用户之前**）、`login_via_ldap`、`complete_oidc_callback`、`exchange_sso_ticket`、`sso_providers`、`SessionView.authProvider`、6 个后设槽、`impl Auditable`、`create_session` / `issue_tokens_for_session` 拆分 |
| `platform-iam/src/store.rs` | `SessionRecord.auth_provider` + **两条 SELECT 补列** + 两处 INSERT 补列；`oidc_state` / `sso_ticket` 的 insert/find/consume；`external_identity` 读与 `DO NOTHING` 写；JIT `ON CONFLICT DO NOTHING` |
| `platform-iam/src/http.rs` | `sso_router`（5 个端点）、`?tenant=` 优先（P15）、回调失败 302（rev 3.1） |
| `platform-iam/src/password.rs` | **Argon2 校验调用计数**（AC-S-021「== 0」的观测面） |
| `platform-core/src/titles.rs` | 21 个 ASCII title（基线实测 **58** 条 + 21 = **79**；规格写的 77 是粗算误差，见 §6 偏离 2） |
| `platform-config/manifest.json` | 12 个键，全部 `scope: tenant` |
| `platform-config/src/registry.rs` | **flag 推导规则修正**（见 §6 偏离 1） |
| `platform-server/src/main.rs` | nest `sso_router`、`ConfigServiceSsoSource`、环境变量注入 |
| `platform-server/tests/i18n_titles.rs` | 钉死 `TITLES` 行数（58+21=79） |
| `platform-org/tests/org_real_db.rs` | `SessionRecord` 初始化补 `auth_provider`（跨 crate 连带） |
| `migrations/20261120000100_iam_sso.{up,down}.sql` | 两张新表（含 GRANT）+ `auth_provider` 列；down 可逆 |
| `scripts/seed-e2e.sh` | 12 个 SSO 键 + epoch bump + 自证断言（rev 3.1 第 3 条） |

### 1.3 测试（新增 10 个文件）

| 文件 | 覆盖 |
|---|---|
| `platform-iam/tests/sso_oidc.rs` | AC-S-100（含 https）、AC-S-101、AC-S-020 步骤 1~4、P15、P4 失败分支 |
| `platform-iam/tests/sso_oidc_security.rs` | AC-S-102（`alg=none` / HS256 混淆 / 篡改 / `jku` 0 次 / `x5c`）、码表完备性 |
| `platform-iam/tests/sso_ldap.rs` | AC-S-104（0 次 connect/bind、注入转义、限流）、AC-S-105 |
| `platform-iam/tests/sso_binding.rs` | AC-S-103（JIT / 禁用 / 并发幂等 / 不可改指）+ P7 静态判据带阳性对照 |
| `platform-iam/tests/sso_ticket.rs` | P4（一次性 / 过期 / 绑租户 / 表里无 token）+ **迁移可逆性** |
| `platform-iam/tests/sso_config_secret.rs` | AC-S-106（三处扫描 + 阳性对照）、AC-S-021（Argon2 计数 / fail-closed）、AC-S-104 步骤 10（日志扫描） |
| `platform-iam/tests/sso_sessions.rs` | AC-S-107（同构）、AC-S-109（多 IdP / 租户隔离） |
| `platform-iam/tests/sso_audit.rs` | AC-S-108（声明 + 发出 + payload 白名单 + 无敏感值） |
| `platform-server/tests/sso_real_db.rs` | 装配层：真实 `ConfigService` 读 12 个键、enforce 经真实配置生效 |
| `platform-iam/tests/sso_support/mod.rs` | 夹具：真实库、**进程内 axum mock IdP（http + https 自签根）**、可计数 LDAP、可记录审计 |

---

## 2. 逐 crate 测试数字

| crate | passed | failed | ignored |
|---|---|---|---|
| `platform-core` | 127 | 0 | 0 |
| `platform-config` | 83 | 0 | 0 |
| `platform-db` | 46 | 0 | 0 |
| `platform-iam` | **165** | 0 | 0 |
| `platform-rbac` | 78 | 0 | 0 |
| `platform-org` | 91 | 0 | 0 |
| `platform-schema` | 20 | 0 | 0 |
| `platform-server` | 110 | 0 | 0 |
| `sandbox` | 33 | 0 | 0 |
| **合计** | **753** | **0** | **0** |

> **统计口径**：上表的 `passed` 是**每个 crate 所有 test target 的 `passed` 之和**
> （lib 单测 + 每个 `tests/*.rs`）。9 个 crate 全部 `failed = 0`、`ignored = 0`
> （`--include-ignored` 让真实库用例真的跑了，不是跳过）。

跑法（出口标准 §9.8，`--all-targets` 不可省、`--include-ignored` 必须在 `--` 之后、只有一个 `--`）：

```bash
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
unset RUST_LOG
for c in platform-core platform-config platform-db platform-iam platform-rbac \
         platform-org platform-schema platform-server sandbox; do
  cargo test -p "$c" --all-targets -- --test-threads=1 --include-ignored
done
```

**为什么 `--test-threads=1`**：`platform-iam/tests/iam_real_db.rs::the_three_failure_paths_have_similar_latency`
是**耗时对比**用例（三条失败路径的比值 < 1.5）。在 `--test-threads=2` 下并行争抢 CPU 会偶发超阈值
（实测 1 次失败）；串行连跑 3 次全过。这是**既有用例的固有抖动**，不是本轮引入的回归。

### 本轮新增的测试（按文件）

| 文件 | passed |
|---|---|
| `platform-iam/tests/sso_oidc.rs` | 19 |
| `platform-iam/tests/sso_oidc_security.rs` | 12 |
| `platform-iam/tests/sso_ldap.rs` | 11 |
| `platform-iam/tests/sso_binding.rs` | 7 |
| `platform-iam/tests/sso_ticket.rs` | 6 |
| `platform-iam/tests/sso_config_secret.rs` | 5 |
| `platform-iam/tests/sso_sessions.rs` | 8 |
| `platform-iam/tests/sso_audit.rs` | 8 |
| `platform-server/tests/sso_real_db.rs` | 3 |
| `platform-iam` 的 `sso` 模块单测（在 lib 内） | 约 30 |


---

## 3. 12 条 AC 逐条自验结论

> 判据口径：**PASS** = AC 的每一条预期都有可重跑的测试断言且实测通过；
> **PASS（含偏离）** = 实质断言通过，但某条字面数值/顺序按 §6 调整并已说明。

### AC-S-020 协议失败可诊断 —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. `503 SSO_OIDC_DISCOVERY_FAILED` | `ac_s_020_a_500_from_the_issuer_is_discovery_failed` | ✅ |
| 2. `502 SSO_OIDC_DISCOVERY_INVALID`（与 1 可区分） | `ac_s_020_invalid_discovery_json_is_discovery_invalid` | ✅ |
| 3. `400 SSO_OIDC_TOKEN_EXCHANGE_FAILED` | `ac_s_020_a_bogus_code_is_token_exchange_failed` | ✅ |
| 4. `502 SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE`（**与 3 不同**） | `ac_s_020_a_503_from_the_token_endpoint_is_endpoint_unavailable` | ✅ |
| 5. `503 SSO_LDAP_UNREACHABLE` | `ac_s_104_an_unreachable_directory_is_503` | ✅ |
| 6. `401 SSO_LDAP_BIND_FAILED`，**不含 DN / 目录原文** | `ac_s_104_a_wrong_password_and_an_unknown_user_are_indistinguishable`（断言响应体不含 `uid=erin` / `ou=people`） | ✅ |
| 7. 与 6 **完全一致** | 同上：`code`/`title`/`status`/`detail`/`type` 逐字相同 + 键集合相同 | ✅ |
| 8. 六个码都在 `ALL_CODES` / `TITLES`，title 为 ASCII | `ac_s_020_every_referenced_code_has_an_ascii_title` | ✅ |
| 9. `i18n_titles.rs` 完备性通过 | `platform-server --test i18n_titles`（7 passed） | ✅ |

### AC-S-021 强制 SSO 生效且不可枚举 —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. `403 SSO_REQUIRED` + `params.ssoProviders` 是可用列表 | `ac_s_021_enforced_sso_rejects_local_login_without_touching_argon2` | ✅ |
| 2. 密码错 / 不存在 用户与预期 1 **完全一致** | 同上：`code`/`title`/`status`/`detail`/`params`/`type` 逐字段相等 | ✅ |
| 3. **Argon2id 调用次数 == 0**；对照组 > 0 | 同上：`argon2_invocation_count()` 前后相等；`enforce=false` 后增量 > 0 | ✅ |
| 4. 另一租户不受影响 | 同上（tenant-b 的 alice 登录 200） | ✅ |
| 5. OIDC 登录仍成功 | `ac_s_100_*` 全链（enforce 不影响 SSO 路径） | ✅ |
| 6. `enforce=false` 后本地登录恢复 | `ac_s_021_...` 步骤 7 段（200 + Argon2 增量 > 0） | ✅ |
| 7. 配置读失败 → **403 fail-closed** + warn | `ac_s_021_an_unreadable_config_fails_closed` | ✅ |
| 8. 不存在用户返回 `403 SSO_REQUIRED` 而非 `401` | 同预期 2 段（`nobody` 也是 403） | ✅ |

**注**：预期 3 的观测面是本轮**新增**的 `platform_iam::password::argon2_invocation_count()`
（只计**校验**）。没有它就只能比耗时，而「耗时同量级」在 S15 里已被判定没有区分力。

### AC-S-100 OIDC 授权码流程端到端 —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. `302` 到发现文档的 `authorization_endpoint` | `ac_s_100_authorize_redirects_...` | ✅ |
| 2. query 必含 8 个参数（含 `S256`） | 同上 | ✅ |
| 2b. `state`/`nonce` 满足 `^[A-Za-z0-9_-]{43,}$` 且 20 次不重复 | `ac_s_100_state_and_nonce_are_long_random_and_never_repeat` | ✅ |
| 3. 回调 **302** 到 `{前端基址}/sso/callback?ticket=…`（不是 200+JSON） | `ac_s_100_the_callback_hands_off_a_ticket_...` | ✅ |
| 4. `LoginResponse` 键集合与本地登录**相同**，`tokenType`/`expiresIn`/`permissions` 合规 | 同上（与真实本地登录逐键对比） | ✅ |
| 5. `GET /rbac/me/permissions` 返回真实权限点 | 权限点经 `PermissionProvider`（未注入时为空数组，fail closed 的是判定） | ⚠️ 见下 |
| 6. `refresh` 返回新 access token | 同上（`200` + 新 `accessToken`） | ✅ |
| 7. `GET /sessions` 含本次 SSO 会话，来源 `oidc` | 同上（`authProvider == "oidc"`） | ✅ |
| 8. 同一 ticket 再用 → `401 SSO_TICKET_INVALID` | `p4_a_ticket_can_only_be_exchanged_once` | ✅ |
| 9. **https issuer 同样全部成功** | `ac_s_100_an_https_issuer_works_with_an_injected_test_root` | ✅ |
| 10. 无 `?tenant=` 无头 → `401 TENANT_CONTEXT_MISSING` | `ac_s_100_authorize_without_a_tenant_is_401` | ✅ |
| 11. 回调失败 → `302 ...?error=SSO_OIDC_STATE_INVALID`，只含错误码 | `ac_s_100_a_failed_callback_redirects_with_only_the_error_code` | ✅ |

> ⚠️ **预期 5 的说明**：`permissions` 的**非空**取决于装配层是否注入 `PermissionProvider`
> （`platform-server` 在 RBAC 就绪后注入，与 REQ-007 的既有设计一致）。
> 我在 `platform-iam` 的单测里断言的是「键存在且**恒为数组**」，
> 「非空」由 `platform-rbac` 的 `me_permissions.rs`（已合并，本轮全绿）覆盖。
> 这是**分层**而非缺口：`platform-iam` 不能依赖 `platform-rbac`（依赖方向成环）。

### AC-S-101 state / nonce / PKCE —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. 重放同一 state → `400 SSO_OIDC_STATE_INVALID` | `ac_s_101_a_state_can_only_be_used_once` | ✅ |
| 2. DB 侧过期 → `400 SSO_OIDC_STATE_EXPIRED`（**可区分**） | `ac_s_101_an_expired_state_reports_expiry_not_invalidity` | ✅ |
| 3. 跨租户 → `400 SSO_OIDC_STATE_INVALID` | `ac_s_101_a_state_from_another_tenant_is_rejected` | ✅ |
| 4. DB 侧改 `provider` → `400 SSO_OIDC_STATE_INVALID` | `ac_s_101_a_state_bound_to_another_provider_is_rejected` | ✅ |
| 5. `nonce` 不符 → `401 SSO_OIDC_NONCE_MISMATCH` | `ac_s_101_a_mismatched_nonce_is_rejected` | ✅ |
| 6. DB 侧改 `code_verifier` → `400 SSO_OIDC_PKCE_MISMATCH` | `ac_s_101_a_tampered_code_verifier_is_detected_locally` | ✅ |
| 7/8. 缺 `state` / 缺 `code` → `400 SSO_OIDC_CALLBACK_PARAM_MISSING` | `ac_s_101_a_callback_without_state_or_code_is_rejected` | ✅ |
| 9. 已用记录 `consumed_at` 非空，且**无同一 state 两条有效记录** | `ac_s_101_a_consumed_state_is_marked_in_the_database` | ✅ |

**PKCE 篡改为什么能在本地发现**：`oidc_state` 表额外存了 `code_challenge`。
回调时**先本地重算** `S256(verifier)` 比对，不一致就 400。
否则只能等 IdP 回 `invalid_grant`，「verifier 被篡改」与「code 无效」在响应上不可区分。

### AC-S-102 id_token 验签 —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. `iss` 错 → `401 SSO_OIDC_TOKEN_INVALID` | `ac_s_102_a_wrong_issuer_is_rejected` | ✅ |
| 2. `aud` 错 → `401 SSO_OIDC_TOKEN_INVALID` | `ac_s_102_a_wrong_audience_is_rejected` | ✅ |
| 3. `exp` 过去 → `401 SSO_OIDC_TOKEN_EXPIRED` | `ac_s_102_an_expired_token_reports_expiry` | ✅ |
| 4. `alg=none` → `401 SSO_OIDC_ALG_NOT_ALLOWED` | `ac_s_102_alg_none_is_rejected` | ✅ |
| 5. `HS256` + 公钥 → `401 SSO_OIDC_ALG_NOT_ALLOWED` | `ac_s_102_hs256_signed_with_the_public_key_is_rejected` | ✅ |
| 6. `RS512` → `401 SSO_OIDC_ALG_NOT_ALLOWED` | `ac_s_102_rs512_outside_the_whitelist_is_rejected` | ✅ |
| 7. 未知 `kid` → **登录成功**（JWKS 刷新） | `ac_s_102_an_unknown_kid_triggers_a_jwks_refresh_and_succeeds` | ✅ |
| 8. 篡改 payload → `401 SSO_OIDC_TOKEN_INVALID` | `ac_s_102_a_tampered_payload_breaks_the_signature` | ✅ |
| 9. `jku` → `401`，**且攻击者端点被请求 0 次** | `ac_s_102_a_jku_pointer_is_never_fetched`（`attacker_hits() == 0`） | ✅ |
| 10. `x5c` 内嵌攻击者公钥 → `401 SSO_OIDC_TOKEN_INVALID` | `ac_s_102_an_x5c_embedded_key_is_not_trusted` | ✅ |
| 11. 每个码都在 `TITLES` 且 ASCII | `ac_s_020_every_referenced_code_has_an_ascii_title` + `ac_s_102_the_algorithm_whitelist_is_hardcoded` | ✅ |

### AC-S-103 绑定与 JIT —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. JIT 关 → `403 SSO_ACCOUNT_NOT_LINKED`，**库里无 user / 无绑定** | `ac_s_103_with_jit_off_...`（计数 `(0,0)`） | ✅ |
| 2. JIT 开 → `200` | `ac_s_103_with_jit_on_a_new_account_is_provisioned_once` | ✅ |
| 3. `email`/`username = "carol"`/占位 PHC 串 + 一条绑定 | 同上（并断言占位串对 ``/`carol`/`PASSWORD` 都不匹配） | ✅ |
| 4. 再登录复用同一 user（表里只有 1 条） | 同上（计数仍 `(1,1)`） | ✅ |
| 5. 禁用账号 → `403 ACCOUNT_DISABLED` | `ac_s_103_a_disabled_account_is_not_bypassed_by_sso` | ✅ |
| 6. 并发 3 次 → 1 user + 1 绑定，**5xx == 0** | `ac_s_103_concurrent_jit_logins_are_idempotent_and_never_5xx` | ✅ |
| 7. 改指后 → `409`，**绑定行未被改动** | `ac_s_103_a_repointed_binding_is_a_conflict_and_is_not_rewritten`（读回 `user_id` 仍是改后的值 = 我们没覆盖） | ✅ |
| 8. 另一租户 JIT 关 → `403` | `ac_s_103_the_same_subject_in_another_tenant_is_independent` | ✅ |
| 9. 另一租户 JIT 开 → 独立 user（不串租户） | 同上（`a_id != b_id`，且 `tenant_id` 正确） | ✅ |

另加一条**静态判据**（带阳性对照）：`ac_s_103_the_authentication_path_has_no_repointing_upsert`
扫 `sso/*.rs`（剥注释）断言**没有** `DO UPDATE`；阳性对照断言同一判据能命中注入的样例，
且 `DO NOTHING` 不被误报。

### AC-S-104 LDAP bind 认证 —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. 正常 → `200` + `LoginResponse` 同构 | `ac_s_104_a_valid_bind_returns_a_login_response` | ✅ |
| 2. 密码错 → `401 SSO_LDAP_BIND_FAILED`，**不泄露 DN / 目录原文** | `ac_s_104_a_wrong_password_...` | ✅ |
| 3. 用户不存在 → **完全一致** | 同上 | ✅ |
| 4. 空密码 → **`400 SSO_LDAP_EMPTY_PASSWORD`**，`connect == 0`、`bind == 0` | `ac_s_104_an_empty_password_never_touches_the_directory`（还断言 `search == 0`） | ✅ |
| 5. 纯空白 → 同 4 | 同上（`"   "`） | ✅ |
| 6. 停掉 mock → `503 SSO_LDAP_UNREACHABLE` | `ac_s_104_an_unreachable_directory_is_503` | ✅ |
| 7. 第 `ip_limit + 1` 次 → `429 LOGIN_RATE_LIMITED` | `ac_s_104_the_ip_rate_limit_applies_to_ldap_failures`（带 `x-forwarded-for`） | ✅ |
| 8. 正常 `connect=1`/`search=1`/`bind=2`；空密码全 0 | `ac_s_104_a_valid_bind_...`（逐项断言）+ 预期 4 | ✅ |
| 9. 三个注入载荷都 `401`，filter 被 RFC 4515 转义 | `ac_s_104_filter_injection_payloads_are_escaped`（还断言发给目录的 filter 里无裸 `*`、无多余括号、含 `\2a`） | ✅ |
| 10. 密码 / DN 在日志 0 命中，**阳性对照能命中** | `ac_s_104_ldap_passwords_and_dns_never_reach_the_logs` | ✅ |

**`simple_bind` 的 `.success()`**：生产实现 `Ldap3Conn::simple_bind` 返回 `result.success().is_ok()`，
即显式判 `rc == 0`。这是 S18 的致命陷阱，**没有**照字面 `simple_bind(..).await?` 当成功。

### AC-S-105 LDAP 与本地账号共存 —— **PASS（含 1 处步骤重排）**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. 本地密码 → `200` | `ac_s_105_ldap_and_local_login_reach_the_same_account` | ✅ |
| 2. LDAP 密码 → `200` | 同上 | ✅ |
| 3. 同一 `user_id`（无影子账号） | 同上 | ✅ |
| 4. `password_hash` 仍是本地 Argon2id PHC 串，未被覆盖 | 同上（断言 `$argon2id$` 前缀） | ✅ |
| 5. 目录改密 → 旧 `401`、新 `200` | `ac_s_105_changing_the_directory_password_takes_effect_immediately` | ✅ |
| 6. `mail` 撞他人绑定 → `409 SSO_IDENTITY_LINK_CONFLICT` | `ac_s_105_a_mail_colliding_with_another_bound_account_is_409` | ✅ 见 §6 偏离 7 |
| 7. 改 `mail` 但 DN 不变 → 仍落到 `frank`（P6 优先 `external_id`） | `ac_s_105_a_changed_mail_attribute_does_not_change_the_account` | ✅ |

### AC-S-106 配置与密钥保护 —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. `entries` 长度 **== 12**（按精确键名） | `platform-server::ac_s_106_the_twelve_keys_come_back_from_the_real_config_service`（真实 `ConfigService`） | ✅ |
| 2. `source ∈ {platform, tenant, default}`，**不得**为 `space` | 同上（逐条断言） | ✅ |
| 3. canary 在 6 处全 0 命中 | `ac_s_106_the_client_secret_never_appears_anywhere_readable`（providers / sessions / 登录响应 / 错误交接 URL / 三张配置表 / 日志） | ✅ |
| 4. **阳性对照**能命中（用 `org` 的 `ldapBaseDn`） | 同上（`control_hits == 1`，清理后回到 0） | ✅ |
| 5. `clientId` 可出现；secret 片段 0 命中 | 同上 | ✅ |
| 6. 12 个键全部登记 | `ac_s_106_the_twelve_keys_are_registered_as_tenant_scoped`（读 `manifest.json` 断言 `scope == "tenant"`） | ✅ |
| 7. 日志里至少有 SSO 的 tracing 行 | 同上（`log_text.contains("sso")`） | ✅ |

### AC-S-107 会话同构 —— **PASS（1 处状态码偏离）**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. 两条会话在同一列表，**键集合完全相同** | `ac_s_107_sso_and_local_sessions_share_one_shape` | ✅ |
| 2. `authProvider` 存在且取值合法，是**唯一允许不同**的字段 | 同上（逐键断言不含 null） | ✅ |
| 3. `DELETE` → **`204` 且响应体长度 0**；refresh 失败 `TOKEN_REVOKED` | `ac_s_107_deleting_a_session_revokes_the_family_...` | ✅ |
| 4. `logout-all` → 全部 family 作废（含 SSO） | 同上 | ✅ 状态码见 §6 偏离 3 |
| 5. 两条会话结构同构；**无 SSO 专用会话表** | 同上（两张会话都在 `platform_iam_session`） | ✅ |
| 6. access token 的 `alg`/`kid`/claim 结构完全一致 | 同上（解 JWT 头与 claims 键集合对比） | ✅ |

**`SessionView.authProvider` 与两条 SELECT**：加列 + 改 `SessionRecord` + 改
`list_sessions` / `find_session` 两条 SELECT + 两处 INSERT。
S22 的陷阱（`tenant_row!` 按列名 `try_get`，漏列是**运行期** 500）已被
`ac_s_107_...`（真实查 `GET /sessions` 返回 200）覆盖。

### AC-S-108 审计事件 —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. 至少声明 3 个码，各有 `code`/`action`/`objectType` | `ac_s_108_the_declared_events_cover_the_three_required_codes` | ✅ |
| 2. `login.succeeded` payload 键集合**恰好** 5 个 | `ac_s_108_a_successful_login_emits_the_exact_succeeded_payload` | ✅ |
| 3. `login.failed` 键集合**恰好** `{provider, code, ip}` | `ac_s_108_a_failed_login_emits_only_provider_code_and_ip` | ✅ |
| 4. LDAP 成败各一条，`provider == "ldap"` | `ac_s_108_ldap_success_and_failure_are_both_recorded_with_provider_ldap` | ✅ |
| 5. canary 0 命中，**阳性对照**能命中 | `ac_s_108_no_payload_carries_a_secret` | ✅ |
| 6. 不注入 recorder 仍登录成功；有 warn | `ac_s_108_login_still_works_without_a_recorder` + `NoopSsoAuditRecorder` 每次 `tracing::warn!` | ✅ |
| 7. **不验落库**（REQ-014） | `ac_s_108_no_sso_audit_table_exists_yet`（断言库里不存在 `*audit*` 表） | ✅ |

**payload 白名单是结构性的**：`SsoAuditEvent` 的 `Serialize` 是**手写**的，
按 `code` 查 `allowed_keys()` 只发白名单里的键 —— 不是「记得别塞」。

### AC-S-109 多 IdP 与租户隔离 —— **PASS**

| 预期 | 断言 | 结果 |
|---|---|---|
| 1. a=2 / b=1 / c=**空列表**（不是 404/500） | `ac_s_109_providers_are_per_tenant_and_never_leak_configuration` | ✅ |
| 2. 两次登录都成功，`family_id` 不相等，表中新增两行 | `ac_s_109_ldap_and_oidc_sessions_have_different_families` | ✅ |
| 3. 跨租户 state → `400 SSO_OIDC_STATE_INVALID` | `ac_s_109_a_state_from_another_tenant_cannot_be_used` | ✅ |
| 4. 未配置 → `400 SSO_PROVIDER_NOT_CONFIGURED` | `ac_s_109_an_unconfigured_tenant_gets_the_same_code_for_both_protocols` | ✅ |
| 5. LDAP 与预期 4 **同一个码** | 同上（逐字比较两个 `code`） | ✅ |
| 6. 禁用 = 未配置 → `400 SSO_PROVIDER_NOT_CONFIGURED` | `ac_s_109_disabling_a_provider_looks_like_not_configured` | ✅ |
| 7. 改 b 的 issuer，a 不受影响 | `ac_s_109_changing_one_tenant_issuer_does_not_affect_another` | ✅ |
| 8. providers 里**无配置值** | `ac_s_109_providers_...`（断言不含 `issuer`/`clientId`/`clientSecret`/`ldapUrl`/`baseDn`/`127.0.0.1`） | ✅ |

---

### 汇总

| AC | 结论 |
|---|---|
| AC-S-020 | **PASS** |
| AC-S-021 | **PASS** |
| AC-S-100 | **PASS**（预期 5 分层实现，见说明） |
| AC-S-101 | **PASS** |
| AC-S-102 | **PASS** |
| AC-S-103 | **PASS** |
| AC-S-104 | **PASS** |
| AC-S-105 | **PASS**（1 处步骤按 P6 重排，§6 偏离 7） |
| AC-S-106 | **PASS** |
| AC-S-107 | **PASS**（`logout-all` 保 200，§6 偏离 3） |
| AC-S-108 | **PASS** |
| AC-S-109 | **PASS** |


---

## 4. 依赖增量实测

| 指标 | 数值 |
|---|---|
| `Cargo.lock` 包数（基线） | **296** |
| `Cargo.lock` 包数（本轮后） | **319** |
| **增量** | **+23**（其中 12 个是 `windows-*` 平台专属条目，Linux 不编译） |
| 真正新增编译单元（Linux） | **13**：`rustls` / `ring` / `hyper-rustls` / `rustls-pki-types` / `rustls-webpki` / `webpki-roots` / `untrusted` / `cc` / `shlex` / `find-msvc-tools` / `rcgen` / `tokio-rustls` / `yasna` |
| TLS 子树冷编译（`jobs=3`，独立临时 crate） | **20 秒**（19.64s），`ring` 的 C/汇编编译成功 |
| 低配机（`jobs=3`）可编性 | **通过**——`.cargo/config.toml` 已固定 `jobs = 3`，本轮全部构建/测试均在该配置下完成 |

**生产依赖树 vs 测试依赖的边界**（D124 决策 ②）：

| crate | 位置 | 说明 |
|---|---|---|
| `rustls` / `hyper-rustls` | `[dependencies]` | 生产 https |
| `tokio-rustls` / `rcgen` / `tracing-subscriber` | `[dev-dependencies]` | mock IdP 服务端 TLS / 自签根 / 日志捕获 |
| `ldap3` / `hyper` / `hyper-util` / `http` / `bytes` / `http-body-util` | `[dependencies]`（锁文件已有） | 包数不变 |

**未引入 `reqwest`**：`Cargo.lock` 里 `reqwest` 仍为 0 命中，出站仍是 `hyper` + `hyper-util` + 换 connector。

---

## 5. rev 3.1 三条补充的落实

### 5.1 `authorize` 接受 `?tenant=`（P15）

`crates/platform-iam/src/http.rs` 的 `sso_authorize`：

```
租户 = ?tenant= 查询参数  >  x-tenant-id 请求头  >  401 TENANT_CONTEXT_MISSING
```

- **复用既有 `TENANT_CONTEXT_MISSING`（401）**，**没有**新增第 22 个错误码。
- handler 里显式处理「查询参数优先于 ctx」：`tenant_context` 中间件在缺头时给的是空串（归一化为 `None`），
  所以 `query_tenant.or_else(|| ctx.tenant_id())` 的顺序即 P15 的优先级。
- LDAP 端点**不需要**（XHR 正常带头）；回调靠 `state.tenant_id`（P3）。
- URL / 响应**不含** `issuer` / `clientId` / LDAP `url` / `baseDn`（AC-S-109 步骤 8 的断言守着）。

测试：`sso_oidc.rs::ac_s_100_authorize_without_a_tenant_is_401`（无参数无头 → 401）
与 `ac_s_100_the_tenant_query_parameter_wins_over_the_header`（头给别的租户，query 赢）。

### 5.2 回调失败也 302 带错误码

`callback_error_redirect`：任意失败 → `302 {前端基址}/sso/callback?error=<CODE>`。

- `error` **只放错误码**（`percent_encode` 后），不放 `detail`、不放配置值、不放 token。
- 成功路径仍是 `302 ...?ticket=<ticket>`。

测试覆盖（每个失败分支都断言 302 + 精确码，并断言 query 里没有 `detail`/`issuer`/`clientId`/`token`）：

| 场景 | 期望 |
|---|---|
| 非法 `state` | `SSO_OIDC_STATE_INVALID` |
| 缺 `code` / 缺 `state` | `SSO_OIDC_CALLBACK_PARAM_MISSING` |
| `state` 过期 | `SSO_OIDC_STATE_EXPIRED` |
| `nonce` 不符 | `SSO_OIDC_NONCE_MISMATCH` |
| PKCE 被篡改 | `SSO_OIDC_PKCE_MISMATCH` |
| code 伪造 | `SSO_OIDC_TOKEN_EXCHANGE_FAILED` |
| token 端点 503 | `SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE` |
| 未绑定 / JIT 关闭 | `SSO_ACCOUNT_NOT_LINKED` |
| 绑定冲突 | `SSO_IDENTITY_LINK_CONFLICT` |
| 账号禁用 | `ACCOUNT_DISABLED` |

### 5.3 E2E 播种脚本补 12 个键

`scripts/seed-e2e.sh` 新增第 7 段：12 个 `platform_tenant_config` 行 + `platform_config_epoch` 的
`platform` 与 `tenant:tenant-a` 双 bump（resolver 成对比较，只写表不 bump 会让已启动实例继续用旧快照）。

- **顺序正确**：12 个键已先登记进 `crates/platform-config/manifest.json`（未登记写入 404 / D53）。
- `sso.enforce` 播种为 **`false`** —— 为 `true` 会让 `shell-auth.spec.ts` 的三条本地登录一起 403。
- `issuer` / `url` 是**不会被访问**的占位地址（只做 URL 格式校验）。
- 新增自证断言：12 个键计数必须 `== 12`，否则脚本非 0 退出
  （REQ-021 教训：只自证「脚本成功」不等于「夹具可用」）。

### 5.4 关于 `params.ssoProviders` 的形状

按要求放**数组**（`serde_json::Value::Array`），由 `SsoProviderView` 的 `Vec` 序列化而来。
写死在 `params` 里（`IamError::SsoRequired { providers }` 的 `params()`），不是响应体顶层字段。

---

## 6. 偏离规格之处（逐条说明理由）

### 偏离 1（**平台侧必要修复**）：`boolean ⇒ flag` 的推导规则

**问题**：`platform-config` 把所有 `type: boolean` 的配置键都视为 flag
（`registry.rs` 原先的 `filter(kind == Boolean)`），而 flag 名只允许 `[a-z][a-z0-9]{0,63}`（**不许点号**）。

我实测确认（`is_flag("sso.enforce") = true` / `is_valid_flag_name("sso.enforce") = false`）：

| 路径 | 结果 |
|---|---|
| `PUT /api/config/entries` | `400 FLAG_USE_FLAG_ENDPOINT`（`FlagEndpointRequired`） |
| `PUT /api/feature-flags/sso.enforce` | `400 FLAG_NAME_INVALID`（`FlagNameInvalid`） |

→ **`sso.enforce` / `sso.jitProvisioning` / `sso.oidc.enabled` / `sso.ldap.enabled`
四个布尔键两条路都写不进去**，配置永远停在 manifest 默认值。
这会让 AC-S-021（强制 SSO）与 AC-S-106（12 个键可写）**物理不可达**。

**修法**（1 处，最小改动）：flag 推导加名字约束 —— 只有**合法 flag 名**的布尔键才是 flag。

```rust
.filter(|registered| {
    registered.kind == ConfigType::Boolean
        && crate::flag::is_valid_flag_name(&registered.key)
})
```

**为什么这符合原意而非改规格**：`boolean ⇒ flag` 本来就是启发式（模块文档第 22~28 行明写
「规格没有给 flag 单独的声明段，`boolean` 是唯一可推导的判据」）。加上名字约束后它才自洽：
**无点号的布尔键是开关，带点号的布尔键只是布尔类型的配置**。
既有 flag（`beta` / `newInvoice`）行为**不变** —— `flags_are_tenant_isolated_and_close_returns_200_semantics`、
`flags_cannot_be_written_through_the_generic_config_route`、`unknown_and_invalid_flag_names_follow_the_contract`
等 25 条 config 用例全绿。

**副作用**：不再给 `sso.*` 派生无意义的 `flags.sso.enforce.rolloutPercent`。

> ⚠️ 这一条**动了已合并需求（REQ-010）的代码**，按 D109 必须声明。请 BA 复核。

### 偏离 2（**规格数字错误**）：`TITLES.len()` 期望值

acceptance 写「56（基线）+ 21 = **77**」。**基线实测是 58 条**。

计数方式（括号深度解析，只数 `TITLES` 数组里的顶层元组，剥掉行注释）：

```
HEAD（git show HEAD:crates/platform-core/src/titles.rs） = 58
WORK（含本轮 21 个 SSO 码）                              = 79
delta                                                     = 21   ← 与规格一致
```

> 注意：`grep -cE '^\s+\("'` 这类按行匹配的粗算会得 56 —— 它**漏掉**
> `TITLES` 里两个被 `rustfmt` 折成多行的条目（形如 `(
  "SYNC_CONFLICT_STRATEGY",
  "..."
),`）。
> 规格里的 56 极可能就是同一种粗算造成的。

→ 实测值 **79**。我没有为了凑 77 而删标题，而是钉死 `58 + 21 = 79`（新增
`the_title_table_has_the_pinned_number_of_rows`，断言 79 == `58 + 21`）。
**断言的目的**（「加码必须同时加标题」）完整保留：任何人加第 22 个码而不加标题，这条会红。

### 偏离 3：`logout-all` 的状态码

AC-S-107 预期 4 写「`logout-all` → **`204`**」。

**既有契约（REQ-007 已合并）返回 `200 {"revokedSessions": n}`**，`iam_real_db.rs` 也断言 `200`。
改成 204 会破既有契约且要连带改前端（前端 Dev 已完成，按 200 解析）。

→ 本轮**保持 200**，在测试里显式标注这一点。AC-S-107 的**实质断言**（「全部 family 作废，含 SSO 会话」）
已完整验证：`logout-all` 之后 refresh 必须 `TOKEN_REVOKED`。

### 偏离 4：`DELETE /sessions/{id}` 的 204 —— **已按规格做到**

这条不是偏离，只是记录：`DELETE` 返回 **204 且响应体长度为 0**，
随后 refresh 失败 `TOKEN_REVOKED`。与 AC 一致。

### 偏离 5：LDAP 生产实现的「服务账号 bind」

P13 说「用服务账号 bind」，但**没有规定凭据从哪来**（config 里没有这两个键，design §7 也没有）。

→ 我用了环境变量 `BASE_SSO_LDAP_BIND_DN` / `BASE_SSO_LDAP_BIND_PASSWORD`（可选）。
**不配置时走匿名 bind** —— 那**仍然是一次 `simple_bind` 调用**，所以 P13 的
「`bind = 2`」计数期望不受影响。

### 偏离 6：`sso.ldap.baseDn` 的阳性对照写入方式

AC-S-106 步骤 7 说「用 `org` 模块的 `ldapBaseDn` 写入同一 canary」。该键 `scope: platform`，
走平台级写通道（`SECURITY DEFINER` 函数 / `platform_config` 表）。

→ 测试里用管理员连接直接写 `platform_config` 表（表级 DML 对应用角色已 REVOKE，
所以这是**测试夹具**的行为，不是产品路径）。写入后立刻清理，并断言清理后回到 0 命中。

**已确认扫描器能命中**（这是关键：`control_hits == 1`）。

### 偏离 7：AC-S-105 步骤 6 的原始步骤顺序

AC-S-105 步骤 6 写「LDAP 侧把 `frank` 的 `mail` 改成**已被本地账号 `gina` 绑定**的邮箱 → 登录 →
`409 SSO_IDENTITY_LINK_CONFLICT`」。

**按 P6 的匹配顺序，这个步骤在它的前后文里不可达**：步骤 2 已经把 `frank` 的 DN 绑上了，
之后改 mail 时 P6 第一步（按 `external_id` 命中）直接返回，**根本走不到 mail 匹配**。

→ 我按**规则本身**验证（这也正是 QA 的 S21 的原意）：
用**尚未绑定**的身份 `frank`（DN 未绑）+ `gina` 已占的 mail → `409`。
用例名 `ac_s_105_a_mail_colliding_with_another_bound_account_is_409`，并在注释里写清了原因。

### 偏离 8：绑定「改指」的检测方式

AC-S-103 步骤 7 只要求「DB 侧改 `user_id` 后登录必须 409」。

**纯靠 `(provider, external_id)` 命中是不够的** —— 那一行存在且指向别人，按 P6 第一步就该「命中即用」。
所以我在命中路径上加了**一致性校验**：若目录给的 email 匹配到**另一个**本地账号 → 409，且不改库。

这个校验在正常场景下不触发（改 mail 后匹配不到任何账号，或仍匹配同一账号）。
它把「绑定被改指」这件事变成**可检测**的，而不只是「不主动改指」。

---

## 7. 我认为还没做好的地方（诚实清单）

1. **`sso.enforce` / `jitProvisioning` 是「租户级」还是「全局」**
   我按 design §7 做成了租户级（`scope: tenant`）。但 `client_secret` 只有**一个环境变量**
   （`BASE_SSO_OIDC_CLIENT_SECRET`）—— 即**多租户共用同一个 IdP 凭据**。
   多 IdP 场景下这就错了（tenant-b 的 token 会用 tenant-a 的 secret 去换）。
   **AC 里没有一条覆盖这一点**（AC-S-109 只要求两个 mock IdP 各自可用 —— 我做到了，
   因为 mock 不校验 secret）。生产前必须解决：`client_secret` 需要租户级加密列（demand §E 已注明「留接口」）。

2. **LDAP 连接无池化**（design §5.2 已登记为技术债，我照做了）。
   高频登录场景每次登录建一次连接 + 2 次 bind，延迟不可接受。本轮无需求。

3. **`ldaps://` 不支持**（design §7 / S43 决定）。真实 AD 大多强制 LDAPS 或 StartTLS ——
   生产可用性上这是个真缺口，本轮按规格排除。

4. **发现文档 / JWKS 缓存无租户维度淘汰**
   缓存键是 `issuer`，`Mutex<HashMap>` 无上限。多租户 + 多 issuer 时是**无界内存增长**。
   本轮按「性能优化，不是正确性依赖」实现（design §4.2 允许），但**没有任何淘汰机制**。
   属于设计缺口，建议下一轮补 TTL 清理。

5. **`SsoConfigSource` 每次请求都 `read_entries`**
   没有请求级缓存 —— 每个 SSO 相关请求都走一次 `resolver.refresh` + 12 次 `resolve`。
   功能正确（这恰恰保证了 AC-S-021 步骤 7「改回 `false` 立即生效」），但登录路径上是额外开销。

6. **AC-S-108 步骤 6 的「warn 级提示」只在默认 recorder 被调用时出现**
   即「发生了 SSO 登录」才有 warn。如果运维想「装配期就被告知没有 sink」，
   目前没有启动期的一次性提示。这是 AC 的字面要求（「跑一次成功登录 + 有 warn」）已满足，
   但装配期静默可能是更早发现问题的更好位置。

7. **`platform-org/tests/org_real_db.rs` 被我改动**（补 `auth_provider` 字段）
   这是 `SessionRecord` 加字段的必要连带，属于已合并需求（REQ-008）的测试文件。
   按 D109 声明。改动只是补一个字段，不动断言。

8. **`ac_s_104_the_ip_rate_limit_applies_to_ldap_failures` 里的 `limit = 20` 是硬编码**
   取的是测试配置 `IamConfig::login.ip_limit`。AC 要求「读配置取实际值」，
   但测试配置本身就是代码常量，读它等于读同一个常量。生产配置从环境变量来（`admin_lim` 那类），
   测试里不可达，所以写死并在注释里标明来源。

---

## 8. 明确**未**验的东西

| 项 | 为什么 |
|---|---|
| 审计**落库 / 查询** | sink 在 REQ-014（AC-S-108 步骤 7 显式排除）。有一条结构性断言守着「本轮不应建审计表」 |
| SAML / CAS | 本轮不交付（树里无 XML 库，XMLDSig 手写验签是 XSW 重灾区） |
| 出站 IdP / 单点登出 | REQ-022 |
| `ldaps://` / StartTLS | design §7 / S43 |
| 多 IdP 的管理界面 | 本轮只做配置层声明 |
| 前端（`apps/` / `packages/` / `e2e/`） | 由前端 Dev 负责，我未改动 |
