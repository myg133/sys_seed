# REQ-022 第二轮独立复核（rev 2 的 §9.4 逐行验证）

- **复核者**：Verifier（第二轮独立复核；非 BA、非上一轮复核者）
- **复核对象**：
  - rev 2 报告：`BA/feature-meta/req-022-final-verification.md` §9（§4 已标作废）
  - 修复轮报告：`feature-REQ-022/feature-meta/req-022-fix.md`
  - 上一轮复核（清单）：`BA/feature-meta/req-022-independent-review.md`
  - 修复轮任务卡：`BA/feature-meta/req-022-fix-brief.md`
- **worktree**：`/home/myg133/Projects/system_seed/feature-REQ-022`（分支 `feature/REQ-022`，HEAD `ed9d8d8`）
- **日期**：2026-11-21

---

## 1. 复核范围与方法

### 1.1 实际做了什么

| # | 动作 | 手段 |
|---|---|---|
| 1 | 打开 rev 2 §9.4 的**每一个** `file:line`，确认那里**真的有**所述测试函数 | `read` / `awk` 打印精确行区间 |
| 2 | 在该函数体内找报告所述的**断言**，确认断言存在 | 逐函数通读（22 个函数全部打开） |
| 3 | 判断每条断言**是否有区分力** | ①读断言形态（真值/整字段/含 vs 不含）；②对 4 条做**真实变异**并观察变红点 |
| 4 | 复算 §9.2 的 `grep` 数字 | 自行 `grep -c` |
| 5 | 变异抽查（4 条）+ 环境还原校验 | 备份副本 + `touch` 还原 + `sha256sum -c` |
| 6 | 核实 `logout.rs` 的 BA 亲修 | 读源码 179-210 行 |
| 7 | 核实 `service.rs` 的 preview 改动（方案 A） | 读源码 + 变异证明 |
| 8 | 核对测试数字 | 亲跑 3 个命令 |
| 9 | 越界 / 注释一致性检查 | `git status --porcelain` + 抽查注释 |

### 1.2 亲跑的测试数字

```
cargo test -p platform-oauth --all-targets -- --test-threads=1 --include-ignored
  unittests src/lib.rs          91 passed
  tests/contract_c5.rs           2 passed
  tests/oauth_http.rs           36 passed
  tests/tenant_isolation.rs      2 passed
  → 合计 131 passed / 0 failed / 0 ignored          ✅ 与报告一致

cargo test -p platform-server --all-targets -- --test-threads=1 --include-ignored
  lib 70 + main 8 + audit_chain 5 + audit_default_size 1 + i18n_api 17
  + i18n_context 5 + i18n_no_chinese 6 + i18n_settings 3 + i18n_titles 9
  + oauth_real_stack 2 + sso_me_permissions 1 + sso_real_db 4
  → 合计 131 passed / 0 failed / 0 ignored          ✅ 与报告一致

pnpm --filter @base/shell test
  → 23 files / 279 tests passed                     ✅ 与报告一致
```

E2E 未跑（任务书标为可选；且无 3 分钟预算内的确定性 —— 见 §7 限制说明）。

### 1.3 只读与还原

- 未执行任何 `git commit / checkout / restore / stash / reset`
- 变异全部用「`/tmp/r22mut/` 备份副本 + `cp` 回填 + `touch`」
- 结束态：`git status --porcelain | wc -l` = **44**（与开工时**完全相同**）
- `grep -rn MUTATION crates/ scripts/ e2e/ apps/ migrations/` = **0**
- 5 个被变异的文件 `sha256sum -c` **全部成功**
- 数据库 RLS policy 复查：4 张表各 1 条 `<table>_tenant_isolation`，**未被污染**

---

## 2. 【核心】rev 2 §9.4 那 22 行的逐行核对

判定口径（三问全过才算「成立」）：

- **A 位置存在吗**：那个 `file:line` 处**真的有**报告所述的测试函数？
- **B 断言存在吗**：该函数体内**真的有**报告所述的那条断言？
- **C 有区分力吗**：实现被改坏时这条断言会红？

| # | AC | 报告所指 | A 位置 | B 断言 | C 区分力 | 结论 |
|---|---|---|---|---|---|---|
| 134 | 元数据可发现 | `oauth_http.rs:388` + `:1250` | ✅ `:388` 是 `the_authorization_server_metadata_is_discoverable`；`:1250` 是 `the_frontend_base_url_is_used_for_the_pages_but_not_for_the_api` | ✅ `:400-417` 断 `authorization_endpoint == "base/oauth/authorize"`；`grant_types_supported` 逐字 3 项。`:1266-1280` 断 `issuer=="base"` 而 `authorization_endpoint==https://frontend.example/...`、`token_endpoint`/`jwks_uri` 用 `base` | ✅ 强。夹具 `frontendBaseUrl="https://frontend.example" ≠ "base"`（`:1252`），两值不同 → 「用页面基址」与「误用 issuer」可判 | **成立** |
| 135 | client 管理 + 回调白名单 | `oauth_http.rs:438` + `:1178` | ✅ `:438` `a_client_can_be_registered_disabled_and_deleted`；`:1178` `a_redirect_uri_outside_the_whitelist_is_rejected` | ✅ `:457` 201 / `:525` `OAUTH_CLIENT_DISABLED`（403）/ `:545` `json!({"error":"invalid_client"})`（401）/ `:570` `OAUTH_CLIENT_NOT_FOUND`（404）；`:1205` `OAUTH_INVALID_REDIRECT_URI` + `:1229` 阳性对照 200 | ✅ 强。白名单外 `https://attacker.example/cb` → 400；同一 client 白名单内 → 200（`:1213-1233`）。**旧缺陷（0 覆盖）已真修复** | **成立** |
| 136 | secret 只存摘要、不回显 | `oauth_http.rs:1318` + `crypto::tests` + `sensitive::tests` | ✅ `:1318` `the_client_secret_is_never_echoed_by_a_read_endpoint`；`:1107` 摘要独立算；`crypto.rs:98-145`；`sensitive.rs:356-396` | ✅ `:1142` 直连查 `client_secret_hash` vs **独立** `digest_of`（`:70`，非生产函数）；`:1146` 43 字符；`:1148` 非 hex；4 输出面：列表 `:1354-1361`、单条 `:1372-1379`、token 响应 `:1164`、审计表 `oauth_real_stack.rs:801`；扫描阳性对照 `sensitive.rs:370-377` | ✅ 强。摘要用独立实现比对（改 hex 会红）；扫描**先阳性对照再断 0 命中**（顺序正确，`:370` 在 `:395` 之前） | **成立** |
| 137 | 授权码 + PKCE 端到端 | `oauth_http.rs:1384` + E2E `oauth.spec.ts:166` | ✅ `:1384` `an_authorization_code_with_pkce_exchanges_for_a_usable_token`；E2E `:166` `未登录 → 登录并继续 → …` | ✅ E2E `:172` 计 preview 请求；`:191` `expect(previewRequests)==0`（未登录不发）；`:213-224` **6 个 query 逐个** `toBe(requested.get(key))`；`:227` 反向对照 `previewRequests > 0` | ✅ 强。**注意**：报告把「未登录回跳」判据放在 137，但该断言在 E2E 里 —— 位置正确、断言存在、有反向对照 | **成立** |
| 138 | PKCE 强制 S256 | `oauth_http.rs:1640` | ✅ `pkce_is_mandatory_and_s256_only` | ✅ 三路径：`:1659` 缺挑战 400、`:1679` plain 400、`:1735` 错 verifier 400；`:1743-1748` 无 CJK；`:1749-1759` 无明文回显 | ✅ 强。三路径分别隔离「缺/不支持/不匹配」 | **成立** |
| 139 | 一次性 / 只存摘要 / 60s TTL | `oauth_http.rs`（并发 + 8 处 `expires_at`） | ✅ 并发 = `:1557` `a_code_can_only_be_exchanged_once_under_concurrency`；TTL = `:856` `an_authorization_code_ttl_is_what_decides_expiry`（`expires_at` 实测 **8** 处） | ✅ `:1607-1615` `ok_count == 1`；`:1635` 令牌表 1 行；`:902-909` 90s → 400 `invalid_grant`；`:918-925` 30s → 200（**只差 `expires_at`**，其余 8 列逐字相同 `:880-895`）；`:935-939` 无 `consumed_at`/`used_at` 列 | ✅ 强。TTL 两条断言仅 `expires_at` 不同 → 真判 TTL。**但**：AC 标题写「60 秒」，测试只做 90s/30s 两档（见 §8 中-1，非阻塞） | **成立** |
| 140 | 三重交集 / 通配 / 不回溯 | `oauth_http.rs:1764` + `:686-730` | ✅ `:1764` `the_scope_is_the_intersection_of_three_sets`；`:680` 函数 `a_wildcard_scope_is_rejected_even_when_it_is_registered_everywhere`（报告写 `:686-730` = 该函数内 D241 三段） | ✅ `:1792-1793` 路径 1；`:1799-1800` 路径 2；`:1805-1806` 路径 3；`:1811-1812` 路径 4。D241：`:708` 前置 0 → `:711-717` `INSERT INTO platform_permission` → `:723` 断言 `after == 1` → `:751-756` 仍 400。路径 5 = `:768` `an_already_issued_token_is_never_widened_retroactively`：`:825-828` 旧令牌仍 `read`、`:842-844` 新令牌含 `write` | ✅ 强。D241 **really inserted**（有 `after==1` 自证），否则「没注册所以 400」的空跑会被抓住 | **成立** |
| 141 | Client Credentials | `oauth_http.rs:1817` | ✅ `the_client_credentials_grant_issues_a_token_without_a_user` | ✅ `:1852-1855` `payload.get("sub").is_none()`（真值断言，非 null）；`:1869-1872` 内省无 `username` 键；`:1885-1886` 超白名单 400 | ✅ 强。`get().is_none()` 区分「键不存在」与「值为 null」 | **成立** |
| 142 | Device Code 全流程 | `oauth_http.rs:1891` + `:943-1035` | ✅ `:1891` `the_device_code_flow_runs_end_to_end`；`:951` `the_device_code_polling_state_is_written_to_the_database`（报告写 `:943-1035` = 该函数域） | ✅ `:1026` `slow_down`；`:1032-1037` `interval_2 == interval_1 + 5`；`:1039-1042` `at_2 > at_1`；`:1059-1064` user_code 当 device_code → `invalid_grant`；`:1090-1097` 换令牌后行已删 | ✅ 强。`poll_interval_seconds` 实测 **3** 处；四条判据各有独立断言 | **成立** |
| 143 | 内省只内省自己的 | `oauth_http.rs:2030` | ✅ `a_client_can_only_introspect_its_own_tokens` | ✅ `:2093` `body == json!({"active": false})` **整字段比对**；`:2108`/`:2118`/`:2132` `json!({"error":"invalid_client"})` 逐字 | ✅ 强。整字段比对无法「多泄露一个字段」而通过 | **成立** |
| 144 | 撤销后立即失效 | `oauth_http.rs:2193` | ✅ `a_revoked_token_stops_being_active_immediately` | ✅ `:2232` 撤销后 `active==false`；`:2256` 越权撤销仍 200；`:2265` **别人撤不掉**（`active==true`） | ✅ 强。含「越权撤销不生效」这条高价值判据 | **成立** |
| 145 | ES256 + 自行验签 | `oauth_http.rs:2280` + helper `:188-246` | ✅ `:2280` `the_access_token_is_an_es256_jwt_with_the_pinned_claims`；`:193` `decoding_key_from_jwks`、`:220` `verify_signature`（报告写 `:188-246`） | ✅ `:2397-2402` `outcome == Valid`；`:2415-2418` 篡改载荷 → `BadSignature`；`an_expired_token_...` `:2542-2546` `ExpiredButSigned`；`:2575-2590` `ExpiredSignature` vs `InvalidSignature` **两个不同 ErrorKind** | ✅ 强，**且已亲测**：把 `issue_access_token` 的签名末字符改掉 → `:2398` 变红 `BadSignature/Valid`（见 §4 变异 3）。helper **只用 `jsonwebtoken` 通用 API**，公钥取自真端点 `/.well-known/jwks.json`（`:2378`），不碰 `platform_oauth::keys` | **成立** |
| 146 | 同意页 + 用户所见=实际授予 | `oauth_http.rs:2723` + P0-3 段 + `ticket::tests` | ⚠️ **`:2723` 是 `the_consent_record_is_written_and_denial_does_not_change_it` —— 该函数内 `body["scopes"]` 断言 0 条**（实测 `grep -c scopes` 在该区间仅命中 1 行注释）。`preview.scopes` 的真断言在 **`:630`**（`the_consent_page_shows_exactly_what_will_be_granted`，函数起 `:589`） | ✅ 断言**确实存在**，但在 `:630`：`assert_eq!(body["scopes"], json!(["rbac:role:read"]), "同意页必须只展示实际会授予的 scope")`。P0-3 段 `:2610` 自签票据 → `:2701-2706` 400 `invalid_scope`、`:2715-2718` 令牌表 0 行；`ticket::tests:250` `a_login_token_is_not_a_ticket` | ✅ 强，**且已亲测**：把票据改成装交集 → `:657` 立刻变红（AC-S-140 路径 2 被破坏），证明「票据装 requested」是真的（§4 变异 5） | **部分成立 —— 引用错函数（应指 `:589`/`:630`）** |
| 147 | 我的授权 + 令牌立即失效 | `oauth_http.rs:2812` | ✅ `:2812` `a_user_sees_and_revokes_only_their_own_grants` | ⚠️ 该函数只断**一枚**令牌链路（`:2838` 列表 1 条、`:2854` revoked、`:2867-2871` `revokedAt`）。报告所述「**两枚**令牌都 `active=false` + 库里两行 `revoked`」在 **`:2883` `revoking_a_grant_revokes_every_token_it_issued`**：`:2970-2973` 两枚逐个 `active==false`、`:2986-2989` 两行 `revoked`。「跨租户空数组」在 **`:3051`**：`:3066-3070` `body == json!([])` | ✅ 强（断言全部真实存在且有力），但**三个判据分散在三个函数**，报告只给了 `:2812` | **部分成立 —— 引用不全/错位** |
| 148 | 免登 | `oauth_http.rs:3142` | ✅ `an_existing_grant_allows_silent_renewal` | ✅ `:3165-3172` 覆盖 → 免登 200 + 无票据 + `code=`；`:3191-3196` 不覆盖 → `needsConsent=true`；`:3201-3212` **code 表恰好 1 行**（只算前一步免登那 1 行 → 证明没偷偷发码） | ✅ 强。`:3211` 用「恰好 1 行」而非「0 行」，避免把前序步骤的码误判 | **成立** |
| 149 | Back-Channel Logout | `oauth_http.rs:3626` + `:3478-3560` | ✅ `:3626` `the_logout_revokes_the_grants_and_returns_the_front_channel_list`；`:3490` `the_backchannel_logout_token_carries_the_pinned_claims`（报告写 `:3478-3560` ≈ 该函数域） | ✅ `:3538` `iss=="base"`；`:3539` `aud==CLIENT_A`；`:3540-3542` `sub/sid/jti/iat` 存在；`:3544-3550` `events` **含** `http://schemas.openid.net/event/backchannel-logout`（`schemas.openid.net` 实测 **1** 处）；`:3552-3555` **不含** `nonce`（`nonce` 实测 **4** 处）；`:3557-3560` `sid` **逐字等于**登录令牌 `sid`；`:3569-3621` timeout/500 两模式登出仍 200 + grant 已撤；`:3339-3342` 失败日志含 `client-a` | ✅ 强。**8 项 payload 断言逐条存在**；`sid` 用「与登录令牌逐字相等」而非「非空」 | **成立** |
| 150 | Front-Channel + 按应用登出 | `oauth_http.rs:3626` + `oauth-logout.test.ts` | ✅ `:3626`（同上）；`apps/shell/test/oauth-logout.test.ts` | ✅ 后端 `:3650-3654` `front.len()==1`、`:3655-3657` `clientId`/`logoutUrl`/`logoutToken`、`:3659` 不含 `client-b`；前端 `:78` iframe 数量、`:94-105` `sandbox="allow-scripts"` + **不含** `allow-same-origin` | ✅ 强。`sandbox` 两个方向都断（含 / 不含） | **成立** |
| 151 | 业务端点默认拒绝 | `oauth_real_stack.rs:398-423` | ✅ 文件共 2 个测试：`:393` `an_oauth_token_is_rejected_by_the_business_endpoints`（`:398-423` 是其步骤 1/2） | ✅ `:402-406` 401；`:407` `code=="UNAUTHENTICATED"`；`:408-411` **不含 `error` 键**；`:425-434` 审计端点同上（`:430` 再断 `UNAUTHENTICATED`）；`:448-464` introspect 200 + `active==true`；`:472-476` 阳性对照登录令牌 200 + `:483-486` 含 `audit:events:read` | ✅ 强，**且已亲测**：改 `platform_rbac` 的 `UNAUTHENTICATED` 常量 → `:407` 精确变红 `NOT_AUTHENTICATED/UNAUTHENTICATED`（§4 变异 2b）。`me/permissions` 实测 **7** 处、`UNAUTHENTICATED` **4** 处、`get("error")` **2** 处 | **成立** |
| 152 | 错误码 8 处 +「包含」 | `error::tests` 7 条 + `i18n_titles` 9 个用例 | ✅ `error.rs:272` `mod tests` 含 **7** 个 `#[test]`；`i18n_titles.rs` 含 **9** 个测试 | ✅ `i18n_titles.rs:166-175` `TITLES.len() == 58+21+5+9`（公式常量，非裸 93）；`:369-411` 逐 locale 覆盖；`:365-368` 注释明写「判据是**包含**（`error_codes ⊆ 词库`），不是相等」；`error.rs:303` `ALL_CODES.len()==9`、`:312-336` 变体全覆盖 | ✅ 强。**「包含」不「相等」**在注释与实现都成立（`:391-400` 只查缺失集合） | **成立** |
| 153 | 权限点三处齐 + C-5 | `contract_c5.rs`（2 passed）+ `capabilities --check`（14 文件）+ `tenant-policies --check` | ⚠️ `contract_c5.rs` 确有 **2** 个 test（`:48`、`:97`）；`capabilities/` 实测 **14** 个文件 | ⚠️ **`total == 32` 全仓无任何测试断言**（仅 `capabilities/permissions.json:202` 的产物数值，实测确实 `32`）。`manifest.permission_codes().len()==6` 有断言（`contract_c5.rs:117-118`）。**C-5 步骤 3/4（阳性对照）无测试** —— `contract_c5.rs` 只有 2 个 test，无「删调用点」「放 `authorize` 函数」的变异用例 | ⚠️ **部分**。「6 个权限点恰好」与 `tenantScoped` 4 表有硬断言；`total==32`、14 文件、C-5 阳性对照**都只是人工核对**，不是测试断言 | **部分成立 —— `total==32` 与 C-5 步骤 3/4 无断言** |
| 154 | 审计落库 + 哈希链 | `oauth_real_stack.rs:156, 664, 707` | ✅ `:156` `PgAuditSink::new`（真 sink，非内存）、`:664` 查 `platform_audit_log`、`:707` `ChainVerifier` | ✅ `:666-679` 5 个事件码逐个 `actions.iter().any(...)`；`:690-697` 固定字段集（`prev_hash`/`hash` 64 位）；`:714-719` `report.ok`；`:720-725` `checked >= rows.len()`（防空跑）；`:752-756` 真实数据 0 命中 + `:758-777` **阳性对照**（插假 secret 必须命中）+ `:787-791` 删后不再命中 | ✅ 强，**且已亲测**：审计改空操作 → `:678` 变红「审计表里缺少事件码 …：[]」（§4 变异 4）。`platform_audit_log` 实测 **5** 处 | **成立** |
| 155 | 跨租户不可见 | `tenant_isolation.rs:134` | ✅ `the_oauth_tables_are_isolated_by_tenant` | ✅ `:141-145` 租户 a 各 1 行；`:147-151` 租户 b 各 1 行；`:153-157` 超管 2 行（**证明前两步不是「表本来就空」**）；`:160-164` 无上下文 0 行；`:177-191` policy 名 + **恰好 1 条**；`:198-215` 写侧 `WITH CHECK` 拒跨租户；`:217-232` 自己租户成功 | ✅ 强。`pg_policies` 逐表断「名字 + 恰好 1 条」，能抓「多加一条宽松 policy」 | **成立** |

### 2.1 逐行结论汇总

| 判定 | 行数 | 是哪几行 |
|---|---|---|
| **完全成立**（位置有函数 + 断言存在 + 有区分力） | **19** | 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 148, 149, 150, 151, 152, 154, 155 |
| **部分成立**（断言真实且有力，但**报告给的落点错/不全**） | **3** | **146**（应指 `:589`/`:630`，报告给了 `:2723` —— 该处无 scopes 断言）<br>**147**（三判据在 `:2883`/`:3051`，报告只给 `:2812`）<br>**153**（`total==32` 与 C-5 步骤 3/4 **无断言**，只有人工核对） |
| **不成立**（指的位置根本没有所述断言） | **0** | — |

> **核心指标**：22 行中，**19 行**「指的位置真的有那个断言」；**3 行**有缺陷（2 行是**引用错函数**，1 行是**报告把人工核对写成了判据**）；**0 行**属于上一轮那种「位置根本没有这条断言」的致命形态。

---

## 3. §9.2 的 `grep` 数字核对（自己跑 vs 报告）

| 报告称 | 我的实测 | 结论 |
|---|---|---|
| `me/permissions` **7 处** | `grep -c` = **7** | ✅ 一致 |
| `UNAUTHENTICATED` **4 处**（oauth_real_stack） | **4** | ✅ 一致 |
| 断「不含 `error` 键」**2 处** | `grep 'get("error")'` = **2** | ✅ 一致 |
| `nonce` **0 → 4** | **4** | ✅ 一致 |
| `expires_at` **0 → 8** | **8** | ✅ 一致 |
| `OAUTH_INVALID_REDIRECT_URI` **0 → 2** | **2** | ✅ 一致 |
| `schemas.openid.net` **1** | **1** | ✅ 一致 |
| `poll_interval_seconds` **3 处** | **3** | ✅ 一致 |
| `platform_audit_log` **5 处** | **5** | ✅ 一致 |
| `sid` | **8**（报告只写「`:3500+`」未给数） | ✅ 不矛盾 |

**§9.2 的 10 项数字全部对得上，无虚报。**

另核：
- `contract_c5.rs` = 2 个 test ✅（报告称「2 passed」）
- `error.rs` = 7 个 `#[test]` ✅
- `i18n_titles.rs` = 9 个 test ✅
- `capabilities/` = **14** 个文件 ✅
- `capabilities/permissions.json` 的 `total` = **32** ✅（数值对，但**无断言**，见 §2 第 153 行）

---

## 4. 变异抽查（4 条 + 1 条澄清）

方法：`cp` 备份到 `/tmp/r22mut/` → 编辑施加变异 → 跑目标测试 → `cp` 回填 + `touch` → `sha256sum -c`。

| # | 变异内容 | 改的生产行 | 结果 | 变红点 | 与报告一致性 |
|---|---|---|---|---|---|
| **1** | `logout.rs` 把 `client_id` 重新装成投递 URI（重现原缺陷） | `logout.rs:101` `backchannel.push((uri.clone(), uri, token))` | **变红** ✅ | `oauth_http.rs:3339` 「日志必须含 clientId」<br>实际日志：`client_id=http://127.0.0.1:44569/logout` | ✅ 与 §9.3 完全一致（报告说 `:3339`，实测就是 `:3339`） |
| **2a** | `problem.rs` 的 `CODE_UNAUTHENTICATED` → `"invalid_token"`（报告 M1a） | `problem.rs:34` | **变红** ✅ | `oauth_real_stack.rs:**430**`（不是报告说的 `:407`） | ⚠️ 变红**位置不同**。原因：`/api/rbac` 用 `platform_rbac::error::code::UNAUTHENTICATED`（`rbac/src/error.rs:26`），而 `/api/audit` 走 `platform_core::problem` → 改后者只影响 `:430`。**这不否定该行，但报告对 `:407` 的因果描述不准** |
| **2b** | `rbac/src/error.rs:26` 的 `UNAUTHENTICATED` → `"NOT_AUTHENTICATED"`（我自加的澄清变异） | `platform-rbac/src/error.rs:26` | **变红** ✅ | `oauth_real_stack.rs:**407**` `left: "NOT_AUTHENTICATED" right: "UNAUTHENTICATED"` | ✅ **证明 `:407` 确有区分力** —— 报告指向的那条断言是真的、硬的 |
| **3** | `service.rs` 的 `issue_access_token` 篡改签名末字符（报告 M2b） | `service.rs:2025-2029` | **变红** ✅ | `oauth_http.rs:2398` `left: BadSignature / right: Valid`，报错文案「AC-S-145 步骤 3：测试侧用 JWKS 公钥独立验签必须**通过**」 | ✅ 与报告 `:2401` 差 3 行（行号漂移），**函数与断言都对** |
| **4** | `service.rs` 的 `audit_event` 改空操作（报告 M4a） | `service.rs:2078-2080` `drop(&event)` | **变红** ✅ | `oauth_real_stack.rs:678`「审计表里缺少事件码 oauth.client.registered：[]」 | ⚠️ 变红点在「事件码缺失」，**不是**报告暗示的 `ChainVerifier → ok`。因果链上游先断（表空 → 链没得验），属**预期内的上游断点**，不否定判据 |
| **5** | `service.rs` 把票据 scope 改成 `granted`（交集）——验证「方案 A」 | `service.rs:934` | **变红** ✅ | `oauth_http.rs:657`「AC-S-140 不能被 P1-9 的修法破坏：请求了用户没有的 scope → 令牌端点必须 400」，实际返回 200 + 完整令牌 | ✅ **精确验证了 BA 的「方案 A」说法**：票据**确实**装 `requested`，一旦改成交集 AC-S-140 路径 2 立刻静默失效 |

### 4.1 还原校验（全部做完后才继续）

```
sha256sum -c /tmp/r22mut/baseline.sha256
  logout.rs   : 成功
  service.rs  : 成功
  problem.rs  : 成功
  jwt.rs      : 成功
  keys.rs     : 成功

git diff --stat crates/platform-rbac/src/error.rs   → 无输出（已还原）
grep -rn MUTATION crates/ scripts/ e2e/ apps/ migrations/  → 0
git status --porcelain | wc -l  → 44（与开工时相同）

psql 复查 RLS：
  platform_oauth_code|platform_oauth_code_tenant_isolation
  platform_oauth_device_code|platform_oauth_device_code_tenant_isolation
  platform_oauth_grant|platform_oauth_grant_tenant_isolation
  platform_oauth_token|platform_oauth_token_tenant_isolation
  → 4 张表各 1 条，未被污染
```

还原后重跑 `platform-oauth` 全套：**131 passed / 0 failed** → 变异未留副作用。

---

## 5. `logout.rs` 的修复核实（§9.3）

**报告声称**：BA 把 `let client_id = uri.clone();` 修成三元组 `(client_id, uri, token)`，日志里现在装的是 clientId。

**实测（`crates/platform-oauth/src/logout.rs`）**：

```rust
// :97-101  收集阶段 —— client_id 与 uri 分开传
if let Some(uri) = client.backchannel_logout_uri.clone() {
    if let Some(token) = self.sign_logout_token(user, &client.client_id)? {
        // ⚠️ `client_id` **必须**一起带下去：日志要记的是 **clientId**
        backchannel.push((client.client_id.clone(), uri, token));
    }
}

// :179-184  投递阶段 —— 三元组解构
async fn deliver_backchannel_logouts(&self, deliveries: Vec<(String, String, String)>) {
    for (client_id, uri, token) in deliveries {
        let result = outbound.post_form(&uri, &body).await;   // ← 投递用 uri
        (client_id, result)                                     // ← 日志用 client_id
    }
}

// :197-203  失败日志
tracing::warn!(client_id, reason = error.reason(),
               "the logout token could not be delivered");
```

| 核对项 | 结果 |
|---|---|
| 代码真的改了吗（字段现在装的是 `client_id` 不是 URI） | ✅ **是**。`:101` 传 `client.client_id.clone()`；`:184` 解构为 `(client_id, uri, token)`，`:188` 投递用 `uri`、`:200` 日志用 `client_id` |
| 注释与代码一致吗 | ✅ **一致**。`:178` 文档注释「失败只记日志（含 `clientId` 与失败原因）」↔ `:200` 日志字段 `client_id` + `:201` `reason = error.reason()`。`:99` 的注释「`client_id` 必须一起带下去」↔ `:101` 实际传 `client.client_id` |
| `:3339` 附近断的是「**必须含** clientId」吗 | ✅ **是**。`oauth_http.rs:3339-3342` `assert!(delivery_log.contains("client-a"), "日志必须含 clientId（AC-S-149 的补充判据）")` —— 正向「必须含」，不是「不得含」 |
| 反向断言是否合理 | ✅ `:3346-3349` 同时断「**不应**回显投递 URI」，理由（泄漏内部拓扑）与代码现状一致 |
| 变异证明（§9.3 声称） | ✅ **亲测复现**（§4 变异 1）：重现原缺陷 → `:3339` 精确变红，日志为 `client_id=http://127.0.0.1:44569/logout`，与报告描述逐字一致 |

**结论：`logout.rs` 这条修得干净，注释与代码一致，守门断言方向正确。**

---

## 6. 新问题检查

### 6.1 `service.rs` 的 preview 改动会不会破坏 AC-S-140？（报告称「方案 A」）

**报告声称**：`preview.scopes` 真算交集，但**票据仍装 `requested`** → AC-S-140 的 400 不变。

**实测（`crates/platform-oauth/src/service.rs`）**：

```rust
// :879-882
let requested = Self::parse_requested_scope(...)?;
let granted = self.granted_scopes(ctx, &user.user_id, &client, &requested).await?;

// :917 / :947  响应体里的 scopes = 交集
PreviewResult { needs_consent: true, ticket: Some(ticket), application, scopes: granted, ... }

// :929-941  票据仍装 requested
let ticket = ticket::sign(..., &requested.join(" "), ...)?;
```

- `PreviewResult.scopes` 的文档注释（`:158-166`）现已明写「= `requested ∩ client 白名单 ∩ 用户权限点`」且「⚠️ 与**票据**里装的 scope **不同**：票据装的是**请求的** scope」→ **注释与代码一致**（这正是修复轮 P1-9 要修的那处不一致）。
- **不会破坏 AC-S-140**：我把票据改成 `granted` 后（§4 变异 5），`oauth_http.rs:657`（AC-S-140 路径 2 的 400 断言）**立刻变红**，返回 200 完整令牌 → 反证现状的票据**确实**装 `requested`。另 `:868-878` 的注释明确「这里**不判交集** —— 交集由令牌端点判」。
- AC-S-140 全套实测通过（`the_scope_is_the_intersection_of_three_sets` + `a_wildcard_scope_...` + `an_already_issued_token_...` 均 ok）。

**结论：方案 A 属实，preview 改动未破坏 AC-S-140。**

### 6.2 注释与代码不一致的新地方（抽查 8+ 处）

| # | 位置 | 注释 | 代码 | 一致？ |
|---|---|---|---|---|
| 1 | `service.rs:158-166` | `scopes` = 交集；票据装 requested | `:947` 传 `granted`；`:934` 传 `requested` | ✅ |
| 2 | `service.rs:866-878` | preview 只判语法、不判交集 | `:879-882` 只调 `granted_scopes` 供展示，无 `Err(InvalidScope)` | ✅ |
| 3 | `service.rs:924-928` | 票据装 `requested`，否则 AC-S-140 失效 | `:934` `&requested.join(" ")` | ✅ |
| 4 | `logout.rs:178` | 失败日志含 `clientId` 与原因 | `:200-202` `client_id` + `reason` | ✅ |
| 5 | `logout.rs:99` | `client_id` 必须一起带下去 | `:101` 传 `client.client_id` | ✅ |
| 6 | `sensitive.rs:356-360` | **先**阳性对照、**再**断真实 0 命中 | `:370-377` 阳性在前，`:395` 真实断在后 | ✅ |
| 7 | `oauth_http.rs:3277-3282` | 失败必须留日志，否则线上无人知道 | `:3318-3326` 找 `"could not be delivered"` + 断 `reason=` | ✅ |
| 8 | `oauth_http.rs:3327-3342` | 断言**已反过来**：必须含 clientId（原先是不得含） | `:3339-3342` `contains("client-a")` | ✅ |
| 9 | `ticket.rs:5-7` | 「用户看到的 scope」与「实际授予」必然一致 | `:117` `verify` 只信票据；`service.rs:991` confirm 取交集 | ✅ |
| 10 | `service.rs:198-199` 的 `PreviewResult` vs `ticket::scopes()` | 票据 scope 是 requested | `ticket.rs:78-80` `scopes()` 原样 split | ✅ |

**未发现新的注释/代码不一致。**

### 6.3 越界改动

`git status --porcelain`（44 项）逐项核对：

| 检查项 | 结果 |
|---|---|
| `router.ts` 是否被改 | ✅ **未改**（`git status` 里无 `router.ts`） |
| `apps/shell/**` 是否只有修复轮允许的改动 | ✅ 只有 `App.svelte` / `lib/routes.ts` / `pages/Login.svelte` 三个 M（均为 REQ-022 前端接线必需），其余全是本需求新增的 `oauth-*` 文件 |
| 是否有 `feature-meta` 之外的报告文件 | 只有 `feature-meta/req-022-{dev-backend,dev-frontend,fix}.md`（属本需求过程产物） |
| 是否动了 14 个 crate 之外的 | 改动集中在 `platform-oauth`（新建）、`platform-server`、`platform-core`、`platform-iam`、`platform-rbac`（仅我变异时临时触碰，已还原） |
| 依赖是否新增 | `Cargo.toml` 有 M，但修复轮声称「零新增第三方依赖」—— 抽查 `jsonwebtoken`/`p256`/`sha2`/`base64`/`time` 均本来在树里 |

**未发现越界改动。**

---

## 7. 测试数字核对

| 命令 | 报告 | 我实测 | 结论 |
|---|---|---|---|
| `platform-oauth`（`--all-targets --include-ignored`） | 131 | **131**（91+2+36+2） | ✅ |
| `platform-server`（同上） | 131 | **131**（70+8+5+1+17+5+6+3+9+2+1+4） | ✅ |
| `pnpm --filter @base/shell test` | 279 | **279**（23 files） | ✅ |
| `failed` / `ignored` | 0 / 0 | **0 / 0** | ✅ |
| E2E 全量 | 44 | **未跑**（任务书标为可选） | ⚠️ 未验证 |

`cargo test -p platform-oauth --test contract_c5` = 2 passed ✅（报告称 2）。
`capabilities/` = 14 文件 ✅。`permissions.json` 的 `total` = 32 ✅（数值对；但**无断言**）。

**环境、访问边界**：E2E 需 playwright + 真实浏览器 + 起服务，约 3 分钟且需手工 TRUNCATE。本轮以「22 行逐行核对」为唯一核心任务（任务书优先级 ①），E2E 的部分判据已在 `137`/`150` 行通过**读源码**核对（`oauth.spec.ts:166-227`、`oauth-logout.test.ts:78-117` 的断言真实存在且有力）。**E2E 的「44 passed」这一具体执行结果未独立复现。**

---

## 8. 发现的问题（按严重度）

### 阻塞（0 条）

无。

### 高（1 条）

**H-1 · §9.4 第 146 行的 `file:line` 指向错误的测试函数**

- **触发条件**：按报告给的 `oauth_http.rs:2723` 打开。
- **实测**：`:2723` 是 `the_consent_record_is_written_and_denial_does_not_change_it`，**该函数体内没有任何 `body["scopes"]` 断言**（`grep -c scopes` 在 `:2723-2808` 只命中 1 行注释）。
- **正确位置**：`preview.scopes = 交集` 的断言在 **`oauth_http.rs:630`**（`assert_eq!(body["scopes"], json!(["rbac:role:read"]), "同意页必须只展示实际会授予的 scope")`），所属函数起于 **`:589`** `the_consent_page_shows_exactly_what_will_be_granted`。
- **影响**：断言**真实存在且极有区分力**（我亲测：票据改交集 → `:657` 变红，立刻暴露 AC-S-140 失效），所以 **AC-S-146 的实质判定成立**；但报告的「每条都指到 `file:line`」这一承诺在该行**未兑现** —— 这正是上一轮 FAIL 的同一形态（指向的位置没有那条断言），只是这次错的是函数、不是虚构。
- **建议**：改为 `oauth_http.rs:589 + :630`。

### 中（3 条）

**M-1 · §9.4 第 147 行只给了一个函数，三个判据分散在三处**

- 报告指 `:2812`，但该函数只覆盖「看到自己的 1 条 + 撤销后 revoked + `revokedAt`」。
- 报告所述的「**两枚**令牌都 `active=false` + 库里两行 `revoked`」在 **`:2883`**（`:2970-2973`、`:2986-2989`）；「跨租户空数组」在 **`:3051`**（`:3066-3070`）。
- 断言**全部真实存在且有力**，AC-S-147 实质成立；只是「一条 AC 一个落点」的表述不成立。

**M-2 · §9.4 第 153 行的 `total == 32` 与 C-5 步骤 3/4 没有测试断言**

- `grep -rn "total" crates/` 全部命中都是分页总数（`query_real_db.rs` 等），**没有任何测试断言 `capabilities/permissions.json` 的 `total == 32`**；该值只存在于产物文件（`:202`）里，靠人工核对。
- AC-S-153 步骤 3（删 `require_permission` 调用点 → C-5 失败）与步骤 4（放 `authorize` 函数 → C-5 失败）**没有任何测试**：`contract_c5.rs` 只有 2 个 test（正向扫描 + manifest 合法性），无变异用例。
- 「14 个文件」同样无断言（`on_disk_files` 只用于 `--check` 比对，不钉数量）。
- 影响：报告把「人工核对的值」写进了「核心断言」列，措辞是「6 个权限点恰好；`total == 32`」—— 前者真有断言（`contract_c5.rs:117-118`），后者没有。

**M-3 · §9.2 的 M1a 变异描述与实测变红点不符**

- 报告称 M1a（改 `problem.rs` 的 `CODE_UNAUTHENTICATED`）变红在 `oauth_real_stack.rs:407`。
- 实测：变红在 **`:430`**。原因：`/api/rbac/me/permissions` 的 401 用 `platform_rbac::error::code::UNAUTHENTICATED`（`rbac/src/error.rs:26`），`/api/audit/events` 走 `platform_core::problem`。改 core 的常量**只影响审计端点**。
- 该行的**判据本身是硬的**（我用 2b 变异证明 `:407` 会精确变红）—— 问题在于报告对**因果关系**的描述不准。

### 低（2 条）

**L-1 · AC-S-139 标题「60 秒」无对应断言**

- AC 标题写「60 秒过期」，`config.rs:80` 默认确实 `60`，`config.rs:195` 有 `assert_eq!(settings.authorization_code_ttl_seconds, 60)`。
- 但交换语义的 TTL 判据只做了 **90s → 400 / 30s → 200**（AC 补充判据 D252 字面要求就是这两档）。
- 影响：`60` 这个**具体默认值**由配置单测守（`config.rs:195`），TTL**机制**由交换用例守（`:902-925`）—— 两条合起来覆盖，但报告 §9.4 第 139 行写「60s TTL」容易让人以为有一条直接断 60 秒的交换用例。属措辞问题。

**L-2 · §9.2 的变异条数内部不一致**

- rev 2 §9 开头说「**24** 条变异」；修复轮报告 §0/§7 说「**23** 条」，§7 的清单按文件归组正好 23 条（`2+12+4+1+1+1+1 = 22`… 含 M2 作废后为 23）。两处数字不同，但都不是判据本身的问题。

---

## 9. 总结论

### **PASS-WITH-RISK**

**核心理由**：

1. **22 行里 19 行完全成立** —— 我逐个打开源码确认：**位置有那个函数、函数里有那条断言、断言有区分力**。其中 4 条（145 / 151 / 154 / 146-实质）我用**真实变异**验证过会变红。这与上一轮「13 行指向不存在的断言」是**质的不同**：这次所有断言都真实存在于测试源码里。
2. **§9.2 的 10 项 `grep` 数字全部对得上，无虚报。**
3. **3 个测试套件的数字全部吻合**（131 / 131 / 279，0 failed / 0 ignored）。
4. **`logout.rs` 修得干净**，注释与代码一致，守门断言方向正确，且我独立复现了它的变异。
5. **「方案 A」属实**：preview 真算交集、票据仍装 `requested`，AC-S-140 未被破坏 —— 我用变异从反面证明（票据改交集 → AC-S-140 路径 2 立刻变红）。

**为什么不是 PASS**：

- **H-1**：第 146 行的 `file:line` 指到了**没有该断言的函数**（`:2723` vs 真实 `:630`）。虽不是虚构，但与「每条都指到 `file:line`」的承诺不符，且形态与上一轮同类。
- **M-2**：第 153 行把 `total == 32`（**无断言**、仅人工核对）写进了「核心断言」列。
- **M-1**：第 147 行只给一个函数，另两个判据的落点未给。

即：**22 条 AC 的实质判定我认可（没有一条是「假绿」）**，但 rev 2 报告的**证据颗粒度仍有 3 行不达标** —— 达不到 BA 自己立的「每条都指到 `file:line` 的 `assert!`」标准。

**为什么不是 FAIL**：这三处不构成「AC 没被验证」的实质缺陷。断言都在，只是报告的指针错了/不全；AC-S-153 的 `total==32` 属于「人工核对也能确认」（我已实跑确认数值为 32），不是被测代码坏了。

**仍然未独立验证的部分**（诚实列出）：

- **E2E 全量 `44 passed / 0 skipped` 未跑**。`137` / `150` 两行涉及 E2E 的判据，我通过**读源码**确认断言真实且有力（`oauth.spec.ts:166-227` 的 6 个 query 逐个断言 + 反向对照；`oauth-logout.test.ts:78-117` 的 sandbox 双向断言），但**没有观察它们实际跑绿**。
- **AC-S-153 步骤 2（启动对账 fail-fast）** 我只确认了实现存在（`platform-server/src/main.rs:364` + `:1133 reconcile_permissions`），**没有真的「只改 manifest 不起种子」去跑起来看退出码**。
- **AC-S-153 步骤 3/4 的 C-5 阳性对照** 未跑（`contract_c5.rs` 无对应用例，见 M-2）。

---

## 附：本轮复核用到的命令（可复现）

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-022
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"

cargo test -p platform-oauth  --all-targets -- --test-threads=1 --include-ignored
cargo test -p platform-server --all-targets -- --test-threads=1 --include-ignored
pnpm --filter @base/shell test

# grep 数字核对
grep -c "me/permissions"         crates/platform-server/tests/oauth_real_stack.rs   # 7
grep -c "UNAUTHENTICATED"        crates/platform-server/tests/oauth_real_stack.rs   # 4
grep -c "nonce"                  crates/platform-oauth/tests/oauth_http.rs          # 4
grep -c "expires_at"             crates/platform-oauth/tests/oauth_http.rs          # 8
grep -c "OAUTH_INVALID_REDIRECT_URI" crates/platform-oauth/tests/oauth_http.rs      # 2
grep -c "poll_interval_seconds"  crates/platform-oauth/tests/oauth_http.rs          # 3

# 变异（任一条）：备份 → 改 → 跑 → 回填 → 校验
cp crates/platform-oauth/src/service.rs /tmp/r22mut/crates_platform-oauth_src_service.rs
# … 编辑 …
cargo test -p platform-oauth --test oauth_http <目标函数> -- --test-threads=1
cp /tmp/r22mut/crates_platform-oauth_src_service.rs crates/platform-oauth/src/service.rs
touch crates/platform-oauth/src/service.rs
sha256sum -c /tmp/r22mut/baseline.sha256

podman exec base-postgres psql -U base -d base_test -tAc \
  "SELECT tablename, policyname FROM pg_policies WHERE tablename LIKE 'platform_oauth%';"
```
