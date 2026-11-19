# QA Pre-merge 复验：REQ-013（SSO 入站：OIDC + LDAP + 强制 SSO）

- **验证者**：Verifier（独立复验，不采信 Dev 自验报告）
- **被验对象**：`/home/myg133/Projects/system_seed/feature-REQ-013`，分支 `feature/REQ-013`，HEAD `3d02c21`
- **验收依据**：`BA/demands/REQ-013-sso/acceptance.md` **rev 3**（12 条 AC）
- **复验时间**：2026-11-20 01:48 ~ 02:05
- **环境**：Arch Linux / 8 线程 / `CARGO_TARGET_DIR=code/target` / `base_test`（podman `base-postgres`）
- **只读声明**：未修改 feature worktree 任何文件。验证前后 `crates/platform-iam/src/sso/ldap.rs` 的 sha256 均为
  `550d424b0e78e608801f3cb6fe1c41d3049a74101fe2eed09b6a5b0eb482ddaa`；`git status` 全程为空。

---

## 汇总

| 级别 | 数量 |
|---|---|
| 阻塞（AC 不可达 / 安全缺口 / 假绿） | **3** |
| 高 | 3 |
| 中 | 3 |
| 低 | 2 |

**结论**：**FAIL**

> 需要说明的是：**rev 2 的三处致命安全问题（S18 / S19 / S29）全部真修了**，回归数字也**逐 crate 真实**。
> 判 FAIL 不是因为安全事故，而是因为**验收面有三处硬缺口**：一条 AC 预期返回码字面不符、
> 一条 AC 的整步（步骤 5）零测试、一处阳性对照是**恒真断言**（违反出口标准 9.2）。

---

## 回归数字（我实测）

命令（逐 crate 串行，单 `--`，`--all-targets` 在 `-p` 前，`--include-ignored` 在 `--` 后）：

```bash
for c in platform-core platform-config platform-db platform-iam platform-rbac \
         platform-org platform-schema platform-server sandbox; do
  cargo test -p "$c" --all-targets -- --test-threads=1 --include-ignored
done
```

| crate | passed | failed | ignored |
|---|---|---|---|
| platform-core | 127 | 0 | 0 |
| platform-config | 83 | 0 | 0 |
| platform-db | 46 | 0 | 0 |
| platform-iam | 165 | 0 | 0 |
| platform-rbac | 78 | 0 | 0 |
| platform-org | 91 | 0 | 0 |
| platform-schema | 20 | 0 | 0 |
| platform-server | 110 | 0 | 0 |
| sandbox | 33 | 0 | 0 |
| **合计** | **753** | **0** | **0** |

- **与 Dev 自报逐 crate 完全一致**（753）。这是本次复验中 Dev 报告与实测**唯一完全对得上**的一类数字。
- 串行执行，未出现 CPU 竞争 flake。
- 前端实测：`apps/shell` 下 `npx vitest run` → **158 passed / 15 files**（含 `test/sso.test.ts` 30 条），与自报一致。

---

## 12 条 AC 判定

| AC | 判定 | 证据 |
|---|---|---|
| AC-S-020 | **PASS** | 6 码逐一点名且都真跑：`sso_oidc.rs:614`(503 DISCOVERY_FAILED) / `:624`(502 DISCOVERY_INVALID) / `:640`(400 TOKEN_EXCHANGE_FAILED) / `:655`(502 TOKEN_ENDPOINT_UNAVAILABLE) / `sso_ldap.rs:223`(503 UNREACHABLE) / `sso_ldap.rs:112`(401 BIND_FAILED 且与「用户不存在」逐字段相同) |
| AC-S-021 | **PARTIAL** | Argon2 计数 ==0 且**有对照组**（`sso_config_secret.rs:339`，`assert_eq!(after,before)` + 关掉 enforce 后 `> before`）；`nobody` 403 而非 401（同用例）；fail-closed 403 + warn（`:446`）。**差**：`params.ssoProviders` 只断言 `is_array()`（`:389`），从未断言内容是「可用 provider 列表」；步骤 4「enforce 是租户级」用 `StaticSsoConfigSource`——它**忽略 `ctx`**（`config.rs:318-320` 对 `_ctx`），全测试共用一份全局快照，再靠手动 `set()` 切回 false，**并未验证租户隔离** |
| AC-S-100 | **PARTIAL** | 步骤 1/2→`sso_oidc.rs:152,178`；3→302 到 `{前端}/sso/callback?ticket`（`:204`）；4 键集合与本地登录相同（`:204`）；6 refresh→`200`；7 `authProvider=="oidc"`（`:289`）；8 ticket 一次性（`:414` 系列）；9 **https 真自签根**（`:306`，`MockIdp::start_https` + `with_root_certificate_der`，生产侧 `.https_or_http()` `oidc.rs:112,145`）；10 缺租户 401（`:365`）；11 失败 302 且 `error` 只含码（`:392` + `callback_error` 断言不含 issuer/client_secret/detail/token）。**差**：**步骤 5（`GET /api/rbac/me/permissions` 用 bob 的 accessToken 返回真实非空权限点）在本仓 `crates/**` 中零测试**——全仓 grep `me/permissions` 在 sso 相关测试里 0 命中 |
| AC-S-101 | **PASS** | 9 步全覆盖：`sso_oidc.rs:408`(重放) / `:440`(DB 制造过期→STATE_EXPIRED 与 INVALID 可区分) / `:463`(跨租户) / `:479`(DB 改 provider) / `:502`(nonce) / `:528`(DB 改 verifier→PKCE) / `:559`(缺参) / `:579`(consumed_at) |
| AC-S-102 | **PASS** | `alg=none`/HS256 公钥混淆/RS512 白名单/篡改 payload（`sso_oidc_security.rs:187,206,234,269`）；`jku` **攻击者端点 `attacker_hits()==0`**（`:301`，真实计数端点 `sso_support/mod.rs:531-545`）；`x5c` 不采信（`:338`）；未知 kid 刷新后成功（`:256`）。**注**：步骤 7 仅断言「成功」，未直接证明「刷新过 JWKS」 |
| AC-S-103 | **PASS** | JIT 关闭 403 且未建号（`sso_binding.rs:142`）；JIT 建号一次（`:154`）；disabled 不绕过（`:191`）；并发 3 次 → 无 5xx + 最终 1 user/1 绑定（`:226`）；**DB 改 `user_id` 后登录 → 409 且绑定行未被改写**（`:296`，真读库断言 `bound == other_id`）；跨租户不串（`:347`）；静态扫描 `DO UPDATE` 且**带阳性对照**（`:382`） |
| AC-S-104 | **PASS** | 正常 200 + `connect=1/search=1/bind=2`（`sso_ldap.rs:91`）；密码错 vs 不存在用户逐字段一致（`:112`）；**空密码断言 `connects==0 && binds==0 && searches==0`**（`:140`，可计数替身 `sso_support/mod.rs:895-1070` 真计数）；不可达 503（`:159`）；限流第 21 次 429 且带 `x-forwarded-for`（`:169`）；注入 `*`/`)(uid=*`/`admin*` 全 401 且 filter 里真出现 `\2a`（`:188`） |
| AC-S-105 | **PARTIAL** | 步骤 1/2/3 同一 user、4 本地 PHC 未被覆盖（`sso_ldap.rs:247`）；5 目录改密立即生效（`:291`）；7 DN 不变改 mail 仍落原账号（`:307`）。**差**：**步骤 6 未按 AC 字面执行**——测试作者判定原步骤「物理不可达」并改用「未绑定新用户 + mail 撞已绑他人」的场景（`:326-364`，注释自陈）。规则本身被验了，但 AC 的步骤 6 字面未被满足 |
| AC-S-106 | **PASS** | canary 在 错误 Location/providers/sessions/login 响应/日志/三张配置表 全 0 命中（`sso_config_secret.rs:197`）；**阳性对照真实有效**：写 `ldapBaseDn=CANARY` → `control_hits==1` → 清理后 `==0`（`:292-310`）；`entries.len()==12` 在真实 `ConfigService` 上验（`sso_real_db.rs:242-247`）；12 键全 `scope: tenant` 且 manifest 无 `secret` 键（`:321`） |
| AC-S-107 | **PARTIAL** | 1/2 键集合相同 + `authProvider` 唯一可不同 + 在 **API 响应**里（`sso_sessions.rs:265-280`）；3 DELETE **204 且 body 长度 0**，随后 refresh `TOKEN_REVOKED`（`:310-340`）；5 同表两行无 SSO 专用表；6 `alg`/`kid`/claims 结构一致。**差**：**步骤 4 明写 `logout-all` 返回 `204`，实现与测试都是 `200 {"revokedSessions":n}`**（`sso_sessions.rs:357` 自陈因 REQ-007 既有契约而为 200）→ AC 字面未满足 |
| AC-S-108 | **PARTIAL** | 声明 3 码（`sso_audit.rs:132`）；payload **键集合恰好**白名单（成功 5 键 `:164`、失败 3 键 `:202`）；LDAP 成败各一条（`:233`）；无 recorder 仍登录成功（`:323`）；**断言不存在任何 `*audit*` 表**（`:337`，符合「REQ-014 才落库」）。**差**：步骤 5 的阳性对照**恒真**——见 Q1 |
| AC-S-109 | **PASS** | a=2/b=1/c=0 且 c 为 200 空列表（`sso_sessions.rs:380`）；providers 响应不含 `issuer/clientId/clientSecret/ldapUrl/baseDn/127.0.0.1`（同处）；LDAP+OIDC 两次登录 `family_id` 不等且表里两行（`:424`）；跨租户 state 400（`sso_oidc.rs:463`）；禁用=未配置（`sso_sessions.rs` AC-S-109 组） |

**分布：PASS 7 / PARTIAL 5 / FAIL 0**（未达字面要求的 5 条均为 PARTIAL，无一条是「功能完全没做」）

---

## 8 条安全约束

| 约束 | 有测试吗 | 验的是攻击样例吗 | 阳性对照 | 判定 |
|---|---|---|---|---|
| §1 LDAP 空密码 | 有 | **是**：`""` 与 `"   "` × `erin`/`ghost`，断言 `connects==0 && binds==0 && searches==0` | 不适用 | **PASS** |
| §2 强制 SSO 在密码校验前 + fail-closed | 有 | **是**：Argon2 计数 `==0`，**对照组 `>0`**；读失败仍 403 + warn | 不适用（对照组即阳性对照） | **PASS** |
| §3 `client_secret` 不泄露 | 有 | **是**：三处扫描 + 错误 Location + 日志 + 三表 | **有效**（写入→命中 1→清理→0） | **PASS** |
| §4 拒 `none`/混淆/不采信 `x5c`/`jku` | 有 | **是**：`jku` 断言**攻击者端点被请求 0 次**；`x5c` 内嵌公钥仍 401 | 不适用（计数即证据） | **PASS** |
| §5 不绕过禁用 + 绑定不可改指 | 有 | **是**：disabled→403；DB 改指后 409 且断言未被改写 | 不适用 | **PASS** |
| §6 审计不含敏感值 | 有 | 是（键集合白名单比扫描更强） | **❌ 恒真断言，等同于没有** | **PARTIAL**（Q1） |
| §7 LDAP filter RFC 4515 转义 | 有 | **是**：三载荷 + filter 内确有 `\2a`、括号计数==1 | 不适用 | **PASS** |
| §8 token 不进 URL | 有 | **是**：成功交接 query 只有 `ticket`，显式断言无 `accessToken`/`refreshToken` | 不适用 | **PASS** |

---

## 逐条问题

### Q1: AC-S-108 步骤 5 的「阳性对照」是恒真断言，等于没有阳性对照（违反出口标准 9.2）

- **级别**：**阻塞**（假绿）
- **位置**：`crates/platform-iam/tests/sso_audit.rs:309-315`
- **问题**：用例先对 `dump`（全部 payload 拼接）断言不含 6 类敏感串（这部分有效），随后「阳性对照」写成把 canary **拼接到 dump 后面**再断言 `contains(CANARY)`。该断言对任何输入都成立，**无法证明扫描判据本身有区分力**——即便 payload 里真的混入了 canary 的**变体**（如 base64、子串），这段代码也照样绿。出口标准 9.2 与本任务书均要求「扫 0 命中之前先证明扫描器会命中」。

```rust
// sso_audit.rs:309-315 —— 这不是阳性对照，是同义反复
let injected = format!("{dump}\n{CANARY}");
assert!(injected.contains(CANARY), "扫描器必须能命中 canary");
```

- **证据**：实跑 `cargo test -p platform-iam --all-targets -- --test-threads=1 --include-ignored` 中 `ac_s_108_no_payload_carries_a_secret ... ok`（passed）。对照 `sso_config_secret.rs:292-310` 的**真**阳性对照（写库→命中→清理→归零），ethodology 明显不同。
- **建议**：改为把 canary 注入一个真实的 `SsoAuditPayload`（例如 `user_agent = CANARY`）再跑同一判据断言命中；或在 `RecordingAudit` 里追加一条合成事件。当前状态下 AC-S-108 预期 5 不可信。

### Q2: AC-S-107 步骤 4 —— `logout-all` 返回 200，AC 明写 204

- **级别**：**阻塞**（AC 字面不符）
- **位置**：`crates/platform-iam/tests/sso_sessions.rs:360-363`；`crates/platform-iam/src/http.rs:635-645`
- **问题**：AC 原文「步骤 4 → `204`；该用户**全部** family 作废，含 SSO 会话」。实测 handler 返回 `Json({"revokedSessions": n})`（200），测试也注释自陈「既有 `logout-all` 契约是 200，改它属跨需求契约变更（D109），本轮不动」。功能面（全 family 作废、SSO 会话被覆盖）已验，但返回码字面未满足，且**这是一处需要 BA 裁定的规格冲突，Dev 自行选择了不改**。
- **证据**：`assert_eq!(status, StatusCode::OK, ...)`；既有 `iam_real_db.rs:1132` 亦断言 `StatusCode::OK`。
- **建议**：要么 BA 把 AC-S-107 步骤 4 改为 200 并登记决策，要么本轮改 handler 并同步 REQ-007 的既有断言。二者之一**必须在合入前定稿**，不能留「AC 写 204、实现 200」并存。

### Q3: AC-S-100 步骤 5 零测试 —— bob 的真实权限点从未被取过一次

- **级别**：**阻塞**（AC 步骤无验证）
- **位置**：AC-S-100 步骤 5「用 `accessToken` 打 `GET /api/rbac/me/permissions`」；预期 5「返回 bob 的**真实**权限点（非空）」
- **问题**：全仓对 `me/permissions` 的 SSO 侧引用为 **0**。`ac_s_100_the_callback_hands_off_a_ticket_and_exchange_returns_a_login_response` 只断言 `login["permissions"].is_array()`（`sso_oidc.rs:245`），**只验类型不验内容**。而 `permissions` 恒为数组是 AC 预期 4 的要求，不是预期 5 的要求——SSO 会话若拿到**空权限集**，本条 AC 依然全绿。
- **证据**：`grep -rn "me/permissions" crates/platform-iam/tests/` → 无命中；`grep -rn "permissions\].*len\|permissions.*is_empty"` → 无命中。
- **建议**：补一条用例：SSO 登录后带 accessToken 打 `/api/rbac/me/permissions`，断言 `len() > 0` 且包含 bob 角色对应的具体权限点。

### Q4: 生产 LDAP 包装层 `Ldap3Conn` 无任何测试覆盖（S18 的核心映射未被验证）

- **级别**：高
- **位置**：`crates/platform-iam/src/sso/ldap.rs:300-318`
- **问题**：rev 2 的头号致命错误是「`simple_bind` 外层 `Ok` ≠ 认证成功」。生产实现确实改对了（`.success().is_ok()`，且我核对了 `ldap3-0.11.5/src/result.rs:281-287` 的 `success()` 只在 `rc==0` 返回 `Ok`）。**但所有测试都用 `RecordingLdap` 直接返回 `Ok(false)`/`Ok(true)`，绕过了「`LdapResult{rc!=0}` → `Ok(false)`」这一行映射本身**。即：真正会「任意密码都能登录」的那一行代码**没有被任何测试执行过**。若将来有人把 `.success()` 删掉，753 条测试**仍然全绿**。
- **证据**：`grep -rn "Ldap3\|ldap3::" crates/platform-iam/tests/` → 0 命中。
- **建议**：至少加一条单测，用构造的 `LdapResult { rc: 49, .. }`（或对 `simple_bind` 的判定抽成纯函数 `fn bind_succeeded(rc: u32) -> bool` 并直接测）证明 `rc=49 → false`、`rc=0 → true`。这样该行来源可被机械验证，不依赖网络。

### Q5: AC-S-021 预期 1 —— `params.ssoProviders` 只断言是数组，未验「可用列表」

- **级别**：高
- **位置**：`crates/platform-iam/tests/sso_config_secret.rs:389`；`crates/platform-server/tests/sso_real_db.rs:359`
- **问题**：AC 写「`params.ssoProviders` 是**可用 provider 列表**」。两处断言都是 `is_array()`。实现 `error.rs:498-504` 确实序列化了 `Vec<SsoProviderView>`，所以大概率正确，但**测试无法区分「正确列表」与「空数组」**——`enforce_sso` 在读失败分支就传 `Vec::new()`（`service.rs:796`），若正常分支哪天退化，测试不红。
- **证据**：上述两处均无 `len()` 断言；`ssoProviders` 全仓仅这两处出现。
- **建议**：前置配好 OIDC+LDAP 后断言 `len()==2` 且 `type` 集合为 `{oidc, ldap}`。

### Q6: AC-S-021 步骤 4 未真正验证「enforce 是租户级」

- **级别**：中
- **位置**：`crates/platform-iam/tests/sso_config_secret.rs:339-441`
- **问题**：`StaticSsoConfigSource::read(&self, _ctx)` **忽略租户上下文**（`sso/config.rs:318-320`），全测试只有一份全局配置。用例为验「tenant-b 不受影响」，只能先 `config.enforce=false` 再打 tenant-b——这验的是「关掉后能登录」，**不是「tenant-a 开、tenant-b 关」并存**。真实 `ConfigService` 路径下有 enforce 生效测试（`sso_real_db.rs:292`），但同样只有一个租户。
- **证据**：`grep -rn "tenant_b\|tenant-b" crates/platform-server/tests/sso_real_db.rs` → 0 命中。
- **建议**：在 `sso_real_db.rs` 里加双租户：a 写 `sso.enforce=true`、b 不写，断言 a→403、b→200，且 **xa与b 的 Argon2 计数分别为 0 / >0**。

### Q7: D127 的 config 推导改动没有新增任何测试

- **级别**：中
- **位置**：`crates/platform-config/src/registry.rs:311-330`
- **问题**：`boolean ⇒ flag` 增加 `&& is_valid_flag_name(...)` 是**跨需求改动**（REQ-010 产物）。既有契约我有实测背书：`is_flag("beta")`/`is_flag("newInvoice")` 仍成立、`flags.beta.rolloutPercent` 仍存在（`registry.rs:562-570`，实跑通过）；SSO 的 4 个布尔键确实能经真实 `ConfigService::write_entries` 写入（`sso_real_db.rs:218-238`）。**没有破坏既有契约**。但「带点号的布尔键不再被拦成 `FlagEndpointRequired`」这一**新行为本身**无专测——它只在「12 键写入恰好没报错」里被间接覆盖。
- **证据**：`git diff 59486d0..HEAD -- crates/platform-config/` → 仅 `registry.rs` 19 行 + `manifest.json`，**无测试文件改动**。
- **建议**：加一条 `assert!(!registry.is_flag("sso.enforce"))` + 一条「带点号布尔键走 Scoped 写路由成功、走 Flag 路由被拒」的用例。

### Q8: AC-S-105 步骤 6 未按字面执行（测试作者自判不可达）

- **级别**：中
- **位置**：`crates/platform-iam/tests/sso_ldap.rs:326-334`
- **问题**：AC 步骤 6 是「LDAP 侧把 `frank` 的 mail 改成已被 `gina` 绑定的邮箱 → 登录 → 409」。测试注释称此时 P6 第一步（按 `external_id` 命中）已先返回，故 409 不可达，改用「新用户 frank + mail 撞 gina」的场景。**判断本身是对的**（P6 顺序确实使原步骤不可达），但这是**规格缺陷而非实现缺陷**，应回报 BA 修 AC，而不是让 AC 与测试长期不一致。
- **证据**：测试内注释「AC-S-105 步骤 6 的原始步骤顺序……实际不可达 409」。
- **建议**：BA 把步骤 6 改为「未绑定身份 + mail 撞他人绑定」的措辞。

### Q9: AC-S-102 步骤 7 只断言「成功」，未证明「刷新过 JWKS」

- **级别**：低
- **位置**：`crates/platform-iam/tests/sso_oidc_security.rs:255-266`
- **问题**：`callback_ok` 只断言 302 + 有 ticket。若实现改成「未知 kid 直接失败但碰巧 mock 又能签发」，该测试也可能绿。缺少「JWKS 被再次请求」的计数断言（mock 已有 `jwks` 端点，可加计数）。
- **建议**：给 mock 的 `/jwks` 加 counter，断言轮换后 `jwks_hits >= 2`。

### Q10: three 处 `is_array()` 类型断言替代内容断言

- **级别**：低
- **位置**：`sso_oidc.rs:245`（permissions）、`sso_config_secret.rs:389` / `sso_real_db.rs:359`（ssoProviders）
- **问题**：属于「弱断言」，但在 AC 预期 4（permissions 恒为数组）语境下**部分合规**。仅 AC-S-021 预期 1、AC-S-100 预期 5 处构成实质缺口（已单列 Q3/Q5）。

---

## 假绿排查（7 项逐项）

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 1 | `#[ignore]` 无原因 | ✅ 干净 | 全仓 `#[ignore]` 均带 `= "需要 base_test 库：…"`，无裸 ignore |
| 2 | E2E `test.skip` 用于「夹具不对」 | ✅ 合规 | `e2e/sso.spec.ts:245` 仅在 404/405（被测系统未就绪）时 skip；200+空列表走 `expect(len).toBeGreaterThan(0)` → **fail**，符合 D108 |
| 3 | `is_ok()`/`is_some()` 弱断言代替具体值 | ⚠️ 轻微 | 仅 `sso_oidc.rs:173` 一处 `code_challenge.is_some()`，紧邻有 `code_challenge_method=="S256"` 具体值断言，可接受；sso 测试**零** `is_ok()` |
| 4 | 「断言了但不可能失败」 | ❌ **存在** | `sso_audit.rs:309-315` 阳性对照恒真（Q1）。其余无 `assert!(true)` / `assert_eq!(1,1)` / `>= 0` |
| 5 | 测试在验 mock 而非验实现 | ❌ **部分存在** | 生产 `Ldap3Conn::simple_bind`（`ldap.rs:300-318`）零覆盖，S18 映射靠人眼（Q4）。其余（config 源、OIDC 验签、PKCE、审计接缝）均有真实被测路径 |
| 6 | 迁移 down 可真逆 | ✅ 真跑 | `sso_ticket.rs:235` 在**独立库** `CREATE DATABASE` 后跑 up→down→up，断言 `auth_provider` 列存在/默认 `local`/down 后两表消失，且**不删既有 `platform_iam_session`** |
| 7 | 并发/幂等的弱判据 | ✅ 干净 | `sso_binding.rs:226` 真起 3 个 tokio 任务，断言 `!is_server_error()` 且 `contains(OK)` 且最终 `(1,1)`；无 `>= 0` 类断言 |

---

## 「Dev 说做了但实际没做 / 说少了」的地方

| Dev 报告的说法 | 实测 | 性质 |
|---|---|---|
| 回归 753（core 127/config 83/db 46/iam 165/rbac 78/org 91/schema 20/server 110/sandbox 33） | 逐 crate **完全一致** | ✅ 诚实 |
| `TITLES.len()` = 58+21 = **79**，规格写 77 是 BA 数错 | Python 解析数组实测 **79 条（SSO 21 条，全 ASCII，键唯一）** | ✅ 诚实，且 Dev 对 BA 的纠错是**正确**的 |
| 8 处编辑点全改 | code 常量 21 / enum 21 / `code()` 21 / `status()` 合并覆盖 21 / `detail()` 21 / `ALL_CODES` 21 / `variants` 21 / `errors` 21 —— **全齐** | ✅ 诚实 |
| 「AC-S-100（含 https）」 | 含 https 属实（真自签根），**但步骤 5 未实现验证** | ⚠️ 覆盖描述**夸大**（Q3） |
| 「§6 审计 payload 无敏感值（带阳性对照）」 | 阳性对照是恒真断言 | ⚠️ 自评**过实**（Q1） |
| `logout-all` 200 偏离、LDAP `limit=20` 硬编码、`client_secret` 单环境变量、无连接池、无 `ldaps://` | **全部已在自验报告 §7/§8 主动声明** | ✅ 诚实（尤其是 `client_secret` 多租户共用这个安全债，主动写出，值得肯定） |

**未被 Dev 声明的缺口**：Q3（步骤 5 无测试）、Q4（生产包装零覆盖）、Q5（`ssoProviders` 只验类型）、Q6（租户级 enforce 未真验）、Q7（D127 无新测试）。

---

## 复核过的决定性证据（供反驳用）

1. **S18 实现正确性**：`ldap.rs:313-316` `Ok(result.success().is_ok())`；`ldap3-0.11.5/src/result.rs:281-287` `success()` 在 `rc!=0` 时返回 `Err`。语义无误。
2. **S19 绑定不可改指**：`store.rs:1426-1439` `ON CONFLICT DO NOTHING`（无 `DO UPDATE`）；`binding.rs:266-291` 读回比对，不一致 → `SsoIdentityLinkConflict`；`binding.rs:113-137` 另有「目录 mail 与库内 owner 不一致」二次校验。
3. **S29 空密码前置**：`service.rs:1116` 位于 `ldap_client()`（`:1120`）/`authenticate()` 之前；`ldap.rs:124-126` `trim().is_empty()` 覆盖纯空白。
4. **fail-closed**：`config.rs:168-174` `Unreadable => true`；`service.rs:798-804` 打 warn。
5. **D126**：`http.rs:461-472` `?tenant=` 优先于 header，缺则 `TENANT_CONTEXT_MISSING`；`http.rs:519-525` 失败 302 只带 `error.code()`。
6. **D127**：`registry.rs:315-322` 增 `is_valid_flag_name`；既有 `beta`/`newInvoice` 断言仍在并通过；SSO 4 布尔键经真实写路由成功。

---

## 未验（诚实标注）

| 项 | 原因 |
|---|---|
| E2E `真后端：providers 非空…`（`e2e/sso.spec.ts:239`） | 需在 8080 起 `platform-server` + `scripts/e2e-server.sh`；本机为低配机且任务要求串行跑 cargo，未为此启动长驻服务。其余 8 条桩用例由 `page.route` 拦截，只验前端渲染，不构成后端证据 |
| `pnpm -r build` / svelte-check / eslint | 超出「后端 AC 验收」射程，且非本次复验必需；shell 单测 158 已独立跑通 |
| `seed-e2e.sh` 的 12 键播种是否幂等 | 依赖上述未启动的真后端栈 |
| 前端 `packages/*` 其它包的测试 | 未逐包跑（Dev 自报 hooks 17 / schema 38 / sdk 38 等），未复验 |

---

## 建议的合入前动作（按优先级）

1. **修 Q1**：审计阳性对照改成真注入（否则 AC-S-108 预期 5 不算通过）。
2. **裁 Q2**：`logout-all` 204 还是 200 —— BA 一句话定稿，同步修 AC 或修实现。
3. **补 Q3**：`GET /api/rbac/me/permissions` 非空权限点用例。
4. **补 Q4**：把 `rc != 0 → false` 变成可机械验证的纯函数并单测（防 S18 回归）。
5. Q5/Q6/Q7 建议同批补，成本低。

> 安全面（§1/§2/§3/§4/§5/§7/§8）我判**可合入**；阻塞项集中在**验收证据链**而非可利用漏洞。
