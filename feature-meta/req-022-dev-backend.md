# REQ-022 Dev 交付报告 —— 棒 1（后端）

> **需求**：REQ-022 出站 IdP 与单点登出（父需求 REQ-001）
> **棒**：棒 1 —— 后端（新 crate `platform-oauth` + 装配 + 迁移 + 夹具 + E2E）
> **worktree**：`/home/myg133/Projects/system_seed/feature-REQ-022`（分支 `feature/REQ-022`）
> **施工单**：`BA/feature-meta/req-022-dev-backend.md`
> **规格**：`BA/demands/REQ-022-outbound-idp/acceptance.md` rev 2+rev 3（22 条 AC）、
> `decisions.md` D196~D259、`design-summary.md` rev 3
> **用例集**：`BA/demands/REQ-022-outbound-idp/test-cases/`（131 条，TC-022-001~131）
> **本轮不 commit**（按施工单抬头「git：不要提交」，由 BA 统一提交）

---

## 1. 交付清单（文件 → AC）

### 1.1 新 crate `crates/platform-oauth/`（23 个文件）

| 文件 | 行 | 改了什么 | AC / 决策 |
|---|---|---|---|
| `Cargo.toml` | 49 | workspace 成员；依赖 `platform-core` / `platform-db` / `platform-iam` + `axum` / `tokio` / `serde` / `sha2` / `base64` / `rand` / `jsonwebtoken` / `hyper` 等**全部已在树里**；**零新增第三方依赖** | dev-guide §9.10 |
| `manifest.json` | 59 | **6** 个权限点 + **1** 条菜单（`route` 逐字 = `/oauth/grants`）+ `tenantScoped` 段（**4** 张表 / `tenant_id`）+ `config` 段（**5** 个键） | AC-S-153 / AC-S-155 / D214 / D227 |
| `src/lib.rs` | 69 | 模块挂载 + 再导出 + `VERSION` | — |
| `src/error.rs` | 399 | **恰好 9** 个平台错误码 + **D213 的 8 处**登记（常量 / enum 变体 / `code()` / `status()` / `detail()` / `params()` / `ALL_CODES` / 测试的两个数组） | AC-S-152 / D213 / D226 |
| `src/events.rs` | 139 | **8** 个 `oauth.*` 审计事件码常量 + 单测（**不进 `TITLES`**，D223） | AC-S-154 / D215 / D223 |
| `src/model.rs` | 483 | 5 张表的行类型 + 领域类型（`grant_type` / `client_type` / `grant_status` / `token_status` / `response_type`） | — |
| `src/store.rs` | 720 | 数据访问（**事务局部** `set_config` → 查询，D148）；`OAUTH_PLATFORM_TENANT` 伪租户；4 个 **SECURITY DEFINER** 函数（授权码 / 设备码 / user_code 摘要查询 + 设备码批准） | AC-S-139 / 140 / 155 |
| `src/service.rs` | 2313 | 业务编排：授权 / 换令牌 / 内省 / 撤销 / 登出 / client 管理 / scope 判定 | AC-S-137~151 |
| `src/http.rs` | 992 | **平台端点**（Problem Details）+ **协议端点**（协议错误体）+ `router()` + `metadata_router()` | AC-S-134~150 |
| `src/client_auth.rs` | 261 | `client_secret_basic` 认证（**不区分**「client 不存在」与「secret 错」→ 同一 401 `invalid_client`） | D207 / AC-S-135 |
| `src/pkce.rs` | 168 | S256 挑战校验（`plain` 不支持） | AC-S-138 / D206 |
| `src/ticket.rs` | 316 | consent ticket 签发 / 验签（**必须校验 `typ`**，登录令牌**不是**票据） | AC-S-146 / D204 |
| `src/logout.rs` | 254 | 登出编排 + back-channel 投递 + front-channel 收集 | AC-S-149 / 150 |
| `src/outbound.rs` | 195 | 出站 HTTP client（**独立实现**，5 秒超时，**不动** REQ-013 的 OIDC client） | AC-S-149 / D220 |
| `src/sensitive.rs` | 355 | **可单独调用的 `pub fn`** 敏感键扫描器（文本 / 列 / 源码三档）+ 阳性对照 | AC-S-136 / 154 / D243 |
| `src/protocol.rs` | 311 | 协议错误体（`{"error": …}` 逐字）+ 状态码（`invalid_client` → 401 / `server_error` → 500 / 其余 400） | C14 / D244 |
| `src/crypto.rs` | 179 | `digest_base64url`（SHA-256 → base64url 无填充，43 字符）+ 熵源 + `user_code`（base20） | D228 / D246 |
| `src/keys.rs` | 281 | 令牌签发 / 验签（**复用 IAM 同一套 ES256 密钥集**） | AC-S-145 / D198 |
| `src/authz.rs` | 86 | **6** 个 `require_permission("<字面量>")` 调用点（**在本 crate 内**，不写在 `platform-server`） | D216 / C-5 |
| `src/config.rs` | 247 | `OAuthConfigSource` trait + `OAuthSettings`（**每请求读**）+ 授权页基址回退 | D222 |
| `tests/common/mod.rs` | 539 | 真库夹具：管理员连接**清空 5 张 OAuth 表**、应用角色连接（**不绕过 RLS**）、`seed_client` / `seed_grant` / `seed_token` | — |
| `tests/contract_c5.rs` | 125 | **真 C-5 命令**（`every_oauth_permission_has_an_authorization_call_site` + `the_oauth_manifest_is_valid`） | AC-S-153 / D240 |
| `tests/oauth_http.rs` | 2225 | 真 HTTP 集成用例（axum `oneshot`，**22** 个） | AC-S-134~151 / 154 |
| `tests/tenant_isolation.rs` | 265 | 跨租户隔离（读侧 + 写侧 + 平台级表） | AC-S-155 |

> **模块切分超出 design §13 的建议清单**：新增 `protocol.rs / crypto.rs / keys.rs / authz.rs / config.rs / sensitive.rs / logout.rs / http.rs`。
> 理由与逐条说明见 §6.6（**需要 BA 知晓，不需拍板**）。

### 1.2 改动既有文件

| 文件 | 改了什么 | AC / 决策 |
|---|---|---|
| `Cargo.toml` | workspace 成员 + `[workspace.dependencies]` 加 `platform-oauth` | — |
| `Cargo.lock` | 入锁（新 crate + 依赖解析） | 施工单抬头「必须入锁」 |
| `crates/platform-iam/src/jwt.rs` | 加 `pub fn sign_claims<T: Serialize>(&self, claims: &T)`；旧 `sign` **内部改调它，行为不变** | D109 #1 / design §5.1 |
| `crates/platform-iam/src/service.rs` | 加 `pub fn keys(&self) -> &JwtKeySet`（`keys` 字段原为私有） | D109 #1 |
| `crates/platform-server/src/main.rs` | 装配（design §10 的 5 处）：**IAM 没配 → 不提供 `/api/oauth/*`**（与 `/api/org/*` 同款口径） | AC-S-134~151 |
| `crates/platform-server/tests/i18n_titles.rs` | `all_codes()` 加 **一行** `for code in platform_oauth::error::ALL_CODES`；行数断言加 `const REQ_022_NEW_CODES: usize = 9;`（84 → **93**） | AC-S-152 / D213 |
| `crates/platform-core/src/titles.rs` | **+9** 条 ASCII 标题（84 → **93**） | AC-S-152 / D113 |
| `crates/xtask/src/catalog.rs` | 修过期文案「待 REQ-014」→ 让 `cargo xtask capabilities --check` 能过 | TD-044 / D240 |
| `capabilities/index.json`、`capabilities/permissions.json` | `cargo xtask capabilities` 重新生成：`permissions.json` 的 `total` **26 → 32**（+6） | AC-S-153 |

### 1.3 数据库（5 张表 + 2 个种子迁移）

| 文件 | 行 | 改了什么 | AC / 决策 |
|---|---|---|---|
| `migrations/20261120000500_oauth_schema.up.sql` | 369 | **5** 张表（`platform_oauth_client` 平台级 + `platform_oauth_grant` / `_code` / `_device_code` / `_token` 带 `tenant_id`）+ 索引 + **4 个 SECURITY DEFINER 函数**；`REVOKE ALL … FROM PUBLIC` + `pg_roles` 守卫的 `GRANT EXECUTE TO base_platform_app`；**迁移里不手写 RLS policy**（由 `cargo xtask tenant-policies` 生成） | design §2.1~2.5 / D219 / D227 |
| `migrations/20261120000500_oauth_schema.down.sql` | 28 | 可逆：先删函数 → 再删表 | 施工单 §3.3 |
| `migrations/20261120000600_oauth_permission_seed.up.sql` | 26 | **6** 条权限点 INSERT，`source` 逐字 = `'crates/platform-oauth/manifest.json'` | D214 |
| `migrations/20261120000600_oauth_permission_seed.down.sql` | 9 | 按 `source` 逆序删行 | 施工单 §3.3 |
| `migrations/20261120000700_oauth_config_seed.up.sql` | 43 | **5** 个配置键（键名 **lowerCamelCase**）：`oauth.frontendBaseUrl`(`""`) / `oauth.accessTokenTtlSeconds`(3600) / `oauth.authorizationCodeTtlSeconds`(60) / `oauth.deviceCodeTtlSeconds`(600) / `oauth.deviceCodePollIntervalSeconds`(5) | D227 / C9 |
| `migrations/20261120000700_oauth_config_seed.down.sql` | 15 | 按键删行 | 施工单 §3.3 |

### 1.4 脚本与 E2E

| 文件 | 行 | 改了什么 | AC / 决策 |
|---|---|---|---|
| `scripts/seed-e2e.sh` | 594 | **OAuth 夹具**：tenant-a 的 **2** 个 OAuth 账号（`u-perm-oauth` = OAuth 管理员 / `e2e-user-oauth` = 授权流程主体）+ tenant-b 的 **1** 个对照账号（`u-perm-other`）+ **3** 个角色（`e2e-role-oauth` / `e2e-role-oauth-user` / `e2e-role-other`）+ **权限点绑定**（`u-perm-read` 也发 `oauth:grant:read` + `oauth:grant:revoke`，D259 ②）+ **3** 个 client（`client-A` / `client-B` / `public-client`）；`client_secret_hash` 按 **`digest_base64url`** 写；**明文 secret 打印 stdout** + 写 `target/e2e-oauth-clients.json`；**强断言段**自证账号、角色、权限点、client 存在且绑定正确 | D236 / D259 / AC-S-135~155 |
| `scripts/mock-logout-receiver.sh` | 139 | **新增**：node 起**真 HTTP 监听**（`127.0.0.1:$MOCK_PORT`），只记 `/logout`，落 `target/mock-logout-received.jsonl`，写 PID 文件；`MOCK_MODE=ok\|timeout\|500`（默认 `ok`）。**独立进程**（不在 E2E 进程里 mock HTTP） | D237 / dev-guide §9.13 |
| `scripts/e2e-server.sh` | 145 | JWT 私钥生成后写入 `target/e2e-jwt-private.pem`（**0600**、每次覆盖、**不入 Git**）并 `export`；起 mock → 再播种；trap 收尾 | D238 / D236（夹具顺序钉死） |
| `e2e/oauth.spec.ts` | 353 | **8** 个 Playwright 用例：`page.route('https://client.example/**')` 拦截 + `new URL(page.url()).searchParams.get('code')` 取码；真点「允许该应用」/「拒绝该应用」/「允许此设备」/「撤销授权」；登出页 iframe + 读 mock 落盘文件；元数据；client 注册 / 停用 / 删除。**0 skipped** | AC-S-137 / 142 / 147 / 149 / 150 / 134 / 135 |

---

## 2. 逐 crate 测试数字

**统计口径**：该 crate **所有 test target** 的 `passed` 之和（`cargo test -p <crate> --all-targets`）。
**逐 crate 跑**（施工单 §5.2：**禁** `--workspace`），`--test-threads=1` 串行。

### 2.1 常规跑（`--all-targets -- --test-threads=1`）

| crate | 角色 | passed | failed | ignored |
|---|---|---|---|---|
| `platform-oauth` | **本轮新增** | **117** | 0 | 0 |
| `platform-iam` | 被改（`jwt.rs` / `service.rs`） | 159 | 0 | 21 |
| `platform-core` | 被改（`titles.rs`） | 153 | 0 | 0 |
| `platform-db` | 直接依赖方 | 45 | 0 | 1 |
| `platform-server` | 被改（装配 + `i18n_titles.rs`） | 129 | 0 | 0 |
| `xtask` | 被改（`catalog.rs`） | 51 | 0 | 0 |

### 2.2 真实库用例复跑（`--all-targets -- --include-ignored -- --test-threads=1`）

> `--include-ignored` 放在 `--` **之后**（施工单硬规矩）；否则真实库用例只是 skip。

| crate | passed | failed | ignored | 说明 |
|---|---|---|---|---|
| `platform-oauth` | **117** | 0 | 0 | 两种口径**同数** → 本 crate **无** `#[ignore]` 用例 |
| `platform-iam` | 180 | 0 | 0 | 21 个真实库用例**全部真跑**并通过 |
| `platform-core` | 153 | 0 | 0 | 无 `#[ignore]` |
| `platform-db` | 46 | 0 | 0 | 1 个真实库用例真跑并通过 |
| `platform-server` | 129 | 0 | 0 | 无 `#[ignore]` |
| `xtask` | 51 | 0 | 0 | 无 `#[ignore]` |

**合计**：常规跑 **654 passed / 0 failed / 22 ignored**；含 ignored 复跑 **676 passed / 0 failed / 0 ignored**。

### 2.3 两个 xtask 守卫命令（**复验通过**）

```
$ cargo xtask capabilities --check
能力索引校验通过：14 个文件与源码一致          # exit=0；capabilities/ 下**恰好 14** 个文件

$ cargo xtask tenant-policies --check
租户隔离前提校验通过：4 个模块声明的表都开了 RLS 且有策略   # exit=0
```

> ⚠️ `cargo xtask capabilities --check` **基线就是红的**（D240 / TD-044）→ 本轮**修好了**
> （`crates/xtask/src/catalog.rs` 的过期文案），现已绿。

### 2.4 脚本 / E2E / 静态检查

| 项 | 状态 | 说明 |
|---|---|---|
| `e2e/oauth.spec.ts` | **本轮不跑**（施工单 §5「本阶段不跑 E2E」） | 已过 `tsc -p`（exit=0）+ `eslint`（exit=0）+ `prettier --check`（`All matched files use Prettier code style!`）；**8** 个用例、**0 skipped**；终版由 BA 统一跑 |
| `scripts/seed-e2e.sh` | **实跑通过** | 夹具就绪；强断言段自证：`u-perm-oauth` / `e2e-user-oauth`（tenant-a）+ `u-perm-other`（tenant-b）账号、角色、权限点绑定（read=4 / write=5 / none=0 / audit=1 / oauth=5 / oauth-user=1 / other=3）、**3** 个 client（`client-A` / `client-B` / `public-client`，back-channel → `http://127.0.0.1:$MOCK_PORT/logout`）、**明文 secret 打印 stdout**（库里只有摘要） |
| `scripts/mock-logout-receiver.sh` | **冒烟通过** | 起真 HTTP 监听 → `POST /logout`（`logout_token=smoke-token-abc`）→ **200** → 落盘 `target/mock-logout-received.jsonl`（每行一条 JSON，含 `at` / `method` / `url` / `logout_token`）；**只记 `/logout`**（就绪探针打 `/` 不会被记进去）。冒烟产物已清理 |
| `cargo test -p platform-oauth --test contract_c5` | **2 passed**（真 C-5） | D240 |
| `cargo fmt -p platform-oauth -p platform-iam -p platform-core -p xtask` + `rustfmt` 两个 platform-server 文件 | 已执行；`cargo fmt -p platform-oauth -- --check` 无输出 | `cargo fmt --all --check` **基线红 11 个存量文件**（TD-017）→ **只格式化自己改过的文件** |
| 敏感值静态扫描 | **0 命中 + 阳性对照命中** | 见 §3 M7 与 §2.5 |
| C-5 命名禁区自查 | `crates/platform-oauth/src/` 内 8 个禁用标识符**只出现在注释与路由路径字符串里**（如 `/oauth/authorize`、`authorization_endpoint`），**无标识符使用**；真扫描器 `contract_c5` 2 passed | D216 |

### 2.5 敏感值扫描（施工单 §5.5：**必须带阳性对照**）

新增单测 `sensitive::tests::the_crate_source_has_no_sensitive_response_field`：

1. **阳性对照先行** —— 把一个 `clientSecret` 响应字段塞进**临时副本** → 扫描器**必须命中**
   （否则下面的「0 命中」就是空跑）；
2. 再扫本 crate `src/**/*.rs` 的**非测试代码** → **0 命中**。

> ⚠️ 扫描面排除 `#[cfg(test)]` 段：测试夹具里**必然**含 `clientSecret`（那正是阳性对照的素材）→ 算进去会自证失败。

---

## 3. 变异验证证据

**方法**（施工单 §5.4）：`cp` 备份（**不用** `cp -p`）→ `sha256sum` 记基线 → 注入**最小变异**（带 `MUTATION` 标记）→
跑**相关**测试 → 确认**变红** → **备份副本**还原（**不用** `git checkout`）→ **必须 `touch`** →
`sha256sum -c` → `grep -c 'MUTATION'` → **0**。

**执行器**：`/tmp/mutation/lib.sh`（`run_mutation`：备份 → sha256 → 注入 → 必须红 → 还原+touch → sha256sum -c → grep 计数）。
**补丁**：9 个 `mN_*.py`；**主脚本**：`/tmp/mutation/run.sh`。
**最终日志**：`/tmp/mutation/run3.log`（`run.sh` 退出码 **0**；`run-1.log` / `run2.log` 是修正 M3 前的两轮）。
**最终状态**：**全部还原**（`git status crates/platform-db/` 为空；全仓 `grep -rn 'MUTATION'` **零残留**）。

| # | 变异点（**生产那一行**） | 注入的文件 | 变红的测试名 | 数字 | 还原校验 |
|---|---|---|---|---|---|
| **M1** | `code_hash` / `client_secret_hash` 口径：base64url → **hex** | `crates/platform-oauth/src/crypto.rs` | `an_authorization_code_with_pkce_exchanges_for_a_usable_token`、`a_code_can_only_be_exchanged_once_under_concurrency`、`a_client_can_only_introspect_its_own_tokens`、`revoking_a_grant_revokes_every_token_it_issued`、`the_access_token_is_an_es256_jwt_with_the_pinned_claims`、`the_device_code_flow_runs_end_to_end` | **6** 个 FAILED | `sha256sum -c` 全过 ✓ / `grep -c MUTATION` = 0 ✓ |
| **M2** | PKCE 校验：比较 `code_challenge` **原文**（空转） | `crates/platform-oauth/src/pkce.rs` | `pkce_is_mandatory_and_s256_only` | 1 FAILED（其余 21 passed） | 全过 ✓ / 0 ✓ |
| **M3** | 授权码交换：把「**删除授权码**」挪到**签发令牌之后** | `crates/platform-oauth/src/service.rs` | `a_code_can_only_be_exchanged_once_under_concurrency`（panic 于 `tests/oauth_http.rs` 的「令牌表里**恰好 1 行**」断言） | **1** FAILED（其余 21 passed）；**检出率实测 10/10** | 全过 ✓ / 0 ✓ |
| **M4** | scope 三重交集：去掉「**用户权限**」那一维 | `crates/platform-oauth/src/service.rs` | `the_scope_is_the_intersection_of_three_sets` | 1 FAILED（其余 21 passed） | 全过 ✓ / 0 ✓ |
| **M5** | 内省：去掉「**属于本 client？**」判定 | `crates/platform-oauth/src/service.rs` | `a_client_can_only_introspect_its_own_tokens` | 1 FAILED（其余 21 passed） | 全过 ✓ / 0 ✓ |
| **M6** | 撤销：**grant 级** → 只撤**其中一枚**令牌 | `crates/platform-oauth/src/store.rs` | `revoking_a_grant_revokes_every_token_it_issued` | 1 FAILED（其余 21 passed） | 全过 ✓ / 0 ✓ |
| **M7** | 敏感键扫描器：**完整键名**匹配 → **子串**匹配 | `crates/platform-oauth/src/sensitive.rs` | `sensitive::tests::a_dotted_event_code_does_not_hit_a_single_word_key`、`sensitive::tests::the_matching_is_on_whole_keys_not_substrings` | 2 FAILED | 全过 ✓ / 0 ✓ |
| **M8** | RLS：写侧放行（`WITH CHECK` 恒真） | `crates/platform-db/src/rls.rs` | `the_oauth_tables_are_isolated_by_tenant` | 1 FAILED（`the_oauth_client_table_is_platform_scoped` 仍 passed → **只破写侧**） | 全过 ✓ / 0 ✓ |
| **M9** | `TITLES` 行数断言：**93 → 92** | `crates/platform-server/tests/i18n_titles.rs` | `the_title_table_has_the_pinned_number_of_rows` | 1 FAILED（其余 8 passed） | 全过 ✓ / 0 ✓ |

**汇总**：**9 / 9** 条变异 → **全部变红** → **全部还原干净**（逐条 `sha256sum -c` 全过 + `grep -c 'MUTATION'` 全 **0**）。

**残留扫描**（全仓）：

```
$ grep -rn 'MUTATION' crates/ scripts/ e2e/ migrations/
（无输出 = 零残留）

$ git status --porcelain crates/platform-db/
（空 = M8 的注入点已逐字节还原）
```

**稳定性实测**（并发类判据，§3.1.4）：

| 状态 | 10 次实测 | 结论 |
|---|---|---|
| 未变异 | **绿 10 / 红 0** | 无假阳性 |
| M3 变异后 | **红 10 / 绿 0** | 检出率 **10/10**，稳定 |

### 3.1 两处**必须记下来**的变异口径修正

#### 3.1.1 M8：BA 建议的字面写法在 PostgreSQL 语义下**无效**（实测）

- BA 建议：「RLS：把 `WITH CHECK` **去掉**」。
- **实测**：字面去掉 `WITH CHECK` 后，`the_oauth_tables_are_isolated_by_tenant` **仍然绿**。
- **原因**：PostgreSQL 里「**没有 `WITH CHECK` 的 policy，`USING` 表达式会被同时用作 INSERT/UPDATE 的检查条件**」
  → 读侧和写侧**都**仍受 `USING` 约束 → 去掉 `WITH CHECK` **不破**写侧隔离。
- **处理**：把 M8 改成**等效且更精确**的「`WITH CHECK (true)`」→ 写侧放行、读侧 `USING` 不动 →
  跨租户 INSERT **真的**穿过去了 → 判据变红（`tenant_isolation.rs:184`）。
- **需要 BA 拍板吗**：**不需要**（判据本身没变，只是把「无效的写法」换成「真正能破写侧的写法」）。

#### 3.1.2 M6：原判据是**空跑**，已补真判据

- 原 `a_user_sees_and_revokes_only_their_own_grants` 造的 grant **一枚令牌都没有** →
  「grant 级撤销」与「撤一枚」**无法区分** → M6 判据空跑。
- **处理**：新增 `revoking_a_grant_revokes_every_token_it_issued`（AC-S-147 步骤 4 的**真正**判据）：
  同一 (租户, client, 用户) 的两次授权码交换**复用同一张 grant 行**（`upsert_grant` 的
  `ON CONFLICT (tenant_id, client_id, user_id)`）→ 一枚 grant + 两枚令牌 → 撤 grant 后**两枚都** `active=false`，
  且 `platform_oauth_token` 里**两行**状态都是 `revoked`。
- **需要 BA 拍板吗**：**不需要**（这是把 AC-S-147 步骤 4 的判据补齐，不是改规格）。

#### 3.1.3 M3：首版补丁**编译不过**（= 「红」但不是判据红），已修正后复验

- **首版**：`m3_delete_after_token.py` 在拼接锚点时把 `.await?;` **重复**了一次 →
  第二次 `.await?` 落在一个返回 `()` 的块之后 → `error[E0277]: () is not a future` → **编译失败**。
- **为什么必须改**：编译失败虽然让 `cargo test` 退出码非 0，但**证明不了判据不是空跑** ——
  它连测试都没跑到。变异必须是「**能编译 + 断言真的红**」。
- **修正**：重写补丁（去掉重复片段）后，**又发现判据本身太弱** → 见 §3.1.4。

#### 3.1.4 并发判据**依赖时序**（flaky）→ 已把并发路数从 2 提到 4，检出率 1/6 → 10/10

- **发现**：M3（把「删除授权码」挪到签发令牌之后）修好补丁后，**有时红有时绿**。
  实测 6 次：**红 5 次 / 绿 1 次** → 「有时绿」= 这条变异**蒙混过关** → 判据不够硬。
- **根因**：M3 要抓的是「**至少两个**请求都读到了码、却都签发了令牌」。
  2 路并发时，只要**其中一路**的读码晚于另一路的删码，它就查不到码 → 400 且**不签发令牌**
  → 令牌表仍是 1 行 → 判据看不出问题。
- **处理**：把 `a_code_can_only_be_exchanged_once_under_concurrency` 的并发路数从 **2 提到 4**
  （并把「被拒的必须是 400 + `invalid_grant`」改成对**全部**被拒响应逐条断言）。
  4 路下只要**任意两路**都读到了码就会暴露 → 判据稳定。
- **复验（实测 10 次）**：
  - **未变异**：**绿 10 / 红 0**（没有假阳性）；
  - **M3 变异后**：**红 10 / 绿 0**（检出率 **10/10**，稳定）。
- **需要 BA 拍板吗**：**不需要**（是把 AC-S-139 的判据改**硬**，不是改规格）。
  ⚠️ 但值得 BA 知晓：**2 路并发的判据对「删除挪后」这类顺序变异不敏感**（§6.9）。

---

## 4. AC → 用例 → 测试函数 对照表（**22 / 22**）

**用例编号区间**取自 `BA/demands/REQ-022-outbound-idp/test-cases/index.md` §3 追溯矩阵。

| AC | 主题 | 用例编号 | 覆盖它的测试函数（**棒 1 自跑**） | 载体 |
|---|---|---|---|---|
| **AC-S-134** | 授权服务器元数据可被发现 | TC-022-001~008 | `the_authorization_server_metadata_is_discoverable` | `tests/oauth_http.rs` |
| **AC-S-135** | client 可注册 / 改 / 停用 / 删 + 回调白名单 | TC-022-009~016 | `a_client_can_be_registered_disabled_and_deleted` | `tests/oauth_http.rs` |
| **AC-S-136** | secret 只存摘要 + 任何读接口不回显 | TC-022-017~022 | `the_client_secret_is_never_echoed_by_a_read_endpoint`<br>`crypto::tests::the_digest_is_base64url_without_padding`<br>`crypto::tests::the_digest_is_not_hex`<br>`crypto::tests::the_digest_matches_the_platform_iam_one`<br>`crypto::tests::the_digest_is_deterministic`<br>`sensitive::tests::the_crate_source_has_no_sensitive_response_field`<br>`sensitive::tests::the_source_scan_hits_a_response_field_name` | `tests/oauth_http.rs` + `src/crypto.rs` + `src/sensitive.rs` |
| **AC-S-137** | 授权码 + PKCE 端到端换到可用令牌 | TC-022-023~029 | `an_authorization_code_with_pkce_exchanges_for_a_usable_token` | `tests/oauth_http.rs` |
| **AC-S-138** | PKCE 强制 S256 | TC-022-030~034 | `pkce_is_mandatory_and_s256_only`<br>`pkce::tests::the_rfc_example_vector_matches` / `the_challenge_is_base64url_without_padding` / `a_matching_verifier_passes` / `a_missing_challenge_is_reported_first` / `plain_and_missing_methods_are_rejected` / `a_wrong_verifier_is_a_mismatch` / `the_challenge_is_recomputed_not_compared_literally` | `tests/oauth_http.rs` + `src/pkce.rs` |
| **AC-S-139** | 授权码一次性 / 只存摘要 / 60 秒过期 | TC-022-035~038 | `an_authorization_code_with_pkce_exchanges_for_a_usable_token`（一次性 + 库里无明文 + 摘要口径）<br>`a_code_can_only_be_exchanged_once_under_concurrency`（**恰好一个 200 + 令牌表恰好 1 行**） | `tests/oauth_http.rs` |
| **AC-S-140** | scope 三重交集 / 通配 / 不越过用户本人 | TC-022-039~046 | `the_scope_is_the_intersection_of_three_sets` | `tests/oauth_http.rs` |
| **AC-S-141** | Client Credentials 换到无用户令牌 | TC-022-047~052 | `the_client_credentials_grant_issues_a_token_without_a_user` | `tests/oauth_http.rs` |
| **AC-S-142** | Device Code 全流程 | TC-022-053~062 | `the_device_code_flow_runs_end_to_end`（含 `slow_down`）<br>`crypto::tests::the_user_code_has_the_rfc_shape` / `the_user_code_avoids_confusable_characters` | `tests/oauth_http.rs` + `src/crypto.rs` |
| **AC-S-143** | 令牌内省（只内省自己的） | TC-022-063~070 | `a_client_can_only_introspect_its_own_tokens` | `tests/oauth_http.rs` |
| **AC-S-144** | 令牌可撤销 + **grant 级**撤销 | TC-022-071~076 | `a_revoked_token_stops_being_active_immediately`（协议端点撤销）<br>`revoking_a_grant_revokes_every_token_it_issued`（**grant 级**，两枚都失效） | `tests/oauth_http.rs` |
| **AC-S-145** | 令牌是 ES256 JWT，第三方可自行验签 | TC-022-125~131 | `the_access_token_is_an_es256_jwt_with_the_pinned_claims` | `tests/oauth_http.rs` |
| **AC-S-146** | 同意页 + 同意记录落库 + 用户所见 = 实际授予 | TC-022-077~083 | `the_consent_record_is_written_and_denial_does_not_change_it`<br>`ticket::tests::a_ticket_round_trips` / `the_ticket_scope_round_trips_as_an_array` / `a_token_with_another_type_is_rejected` / `a_login_token_is_not_a_ticket` / `a_tampered_ticket_is_rejected` / `an_expired_ticket_is_rejected` | `tests/oauth_http.rs` + `src/ticket.rs` |
| **AC-S-147** | 我的授权可查可撤销 + 令牌立即失效 | TC-022-084~087 | `a_user_sees_and_revokes_only_their_own_grants`<br>`revoking_a_grant_revokes_every_token_it_issued`<br>`the_grants_endpoints_are_scoped_to_the_current_user`<br>`another_tenants_user_sees_an_empty_grant_list` | `tests/oauth_http.rs` |
| **AC-S-148** | 已有授权时免登（静默续发） | TC-022-088~090 | `an_existing_grant_allows_silent_renewal` | `tests/oauth_http.rs` |
| **AC-S-149** | Back-Channel Logout 投递 `logout_token` | TC-022-091~097、102 | `the_logout_revokes_the_grants_and_returns_the_front_channel_list`<br>`e2e/oauth.spec.ts`「单点登出：front-channel iframe 出现 + back-channel 真发出去了」 | `tests/oauth_http.rs` + `e2e/oauth.spec.ts` |
| **AC-S-150** | Front-Channel Logout + 按应用登出 | TC-022-098~101 | `the_logout_revokes_the_grants_and_returns_the_front_channel_list` | `tests/oauth_http.rs` |
| **AC-S-151** | 底座业务端点默认拒绝 OAuth 令牌 | TC-022-103~108 | `a_third_party_oauth_token_is_rejected_by_the_platform` | `tests/oauth_http.rs` |
| **AC-S-152** | 错误码 8 处登记 + 跨栈「**包含**」 | TC-022-109~113 | `error::tests::the_code_list_has_exactly_nine_entries` / `all_codes_lists_every_variant` / `every_error_code_is_upper_snake_case` / `every_detail_is_english_ascii` / `no_detail_echoes_a_credential` / `every_code_is_registered_in_titles` / `the_status_codes_are_pinned`<br>`platform-server` 的 `i18n_titles` **9** 个用例（`titles_cover_every_code` / `every_error_code_is_registered_in_every_locale` / `the_title_table_has_the_pinned_number_of_rows` / `titles_are_ascii_and_unique` …） | `src/error.rs` + `crates/platform-server/tests/i18n_titles.rs` |
| **AC-S-153** | 权限点三处齐 + 启动对账 fail-fast + C-5 | TC-022-114~118 | `contract_c5::every_oauth_permission_has_an_authorization_call_site`（**真 C-5**）<br>`contract_c5::the_oauth_manifest_is_valid`<br>`cargo xtask capabilities --check`（14 个文件一致 / `total == 32`）<br>`cargo xtask tenant-policies --check` | `tests/contract_c5.rs` + xtask 命令 |
| **AC-S-154** | 关键动作产生审计记录 | TC-022-119~121 | `the_key_actions_produce_audit_records`<br>`events::tests::the_event_code_list_has_exactly_eight_entries` / `the_event_codes_are_unique` / `every_event_code_follows_the_three_part_naming` / `event_codes_are_not_registered_as_titles` / `event_codes_are_not_platform_error_codes` / `the_five_mandatory_event_codes_are_registered`<br>`sensitive::tests::the_column_scan_reports_the_column_name` | `tests/oauth_http.rs` + `src/events.rs` + `src/sensitive.rs` |
| **AC-S-155** | OAuth 数据跨租户不可见 | TC-022-122~124 | `the_oauth_tables_are_isolated_by_tenant`（读侧跨租户 + 写侧 `WITH CHECK`）<br>`the_oauth_client_table_is_platform_scoped`（平台级表无 `tenant_id` / 无 RLS） | `tests/tenant_isolation.rs` |

**AC 覆盖 = 22 / 22（100%）**。

> **本轮补强的两处判据**（自查变异时发现，已写进 §3.1.4 / §3.1.2）：
> 1. `a_code_can_only_be_exchanged_once_under_concurrency`（AC-S-139）—— 并发路数 **2 → 4**，
>    并对**全部**被拒响应逐条断言 400 + `invalid_grant`；
>    原 2 路对「删除挪后」这类**顺序**变异只有约 **1/6** 检出率 → 现为 **10/10**。
> 2. `revoking_a_grant_revokes_every_token_it_issued`（AC-S-147 步骤 4）—— **新增**：
>    同一 grant 下**两枚**令牌，撤 grant 后**两枚都** `active=false`，且库里**两行**都是 `revoked`。
>    原用例的 grant **一枚令牌都没有** → 「grant 级撤销」判据**空跑**。

> **E2E 覆盖**（`e2e/oauth.spec.ts`，8 个用例，本轮**未跑**）：
> 授权码+PKCE 同意 → 回调带 `code` → 换到令牌（AC-S-137 / 146）·
> 拒绝 → `error=access_denied`（AC-S-146）· 设备码全流程（AC-S-142）·
> 「我的授权」列出 + 撤销（AC-S-147）· 跨租户不可见（AC-S-155）·
> 单点登出 front-channel + back-channel（AC-S-149 / 150）· 元数据（AC-S-134）·
> client 注册 / 停用 / 删除（AC-S-135）。

---

## 5. 开工前必答项（施工单 §4 的 12 条）逐条回答

**1. `platform-oauth` 怎么拿用户会话？**
**能**直接调 `IamService::verify_access_token` —— `platform-oauth` **依赖** `platform-iam`
（`crates/platform-oauth/src/service.rs:517` `authenticate_bearer` → `self.iam.verify_access_token(token)`），
不需要装配层注入状态。
**为什么可行**：平台会话就是 **Bearer + sessionStorage**，浏览器顶层导航（授权页）由**棒 2 的前端路由**
持有会话后调**平台端点**（`/api/oauth/authorize/*`）→ 请求头带 `Authorization: Bearer …` →
装配层只做「把 `IamService` 装进 `OAuthService`」这一件事（`crates/platform-server/src/main.rs`）。
**反过来**，第三方 OAuth 令牌**过不了**这一关（`aud` 是 client_id、没有 `sid`）→ 底座业务端点**默认拒绝**第三方令牌（AC-S-151）。

**2. `PermissionProvider` 怎么注入？**
用 `platform-core` 的 `PermissionProvider` trait（**不依赖** `platform-rbac`）。
6 个权限点的 `require_permission("<字面量>")` 调用点**全部在** `crates/platform-oauth/src/authz.rs:39~67`
（**不在** `platform-server` 里）。
注入点：`OAuthService::new(..., permissions: Arc<dyn PermissionProvider>, ...)`（`src/service.rs:457`），
由 `platform-server` 装配时传入平台既有的权限提供器。

**3. scope 三重交集具体在哪一层算？「用户有效权限点」怎么拿？**
**在 `OAuthService` 里算**（`src/service.rs:1891` `granted_scopes`）：
`请求 scope ∩ client.allowed_scopes ∩ 用户有效权限点`。
「用户有效权限点」走 **`PermissionProvider::effective_permissions(ctx, user_id)`**
（`src/service.rs:1900-1904`）→ **与权限点校验同一条接缝**，不引 `platform-rbac`。
**判定位置**：**令牌端点**（`src/service.rs:1917` 起的「全有或全无」判定）→ 协议错误体 `invalid_scope`。
**依据**：AC-S-140 明确要 `{"error":"invalid_scope"}`（**协议话**）；`preview` 是平台端点，
在它那里判只能得到 `OAUTH_INVALID_SCOPE`（**平台话**）→ 判据形状不符。详见 §6.2。

**4. `client_credentials` 的两重交集（D231）？**
`请求 ∩ client.allowed_scopes`，**无用户维**（`src/service.rs:1149` 起的
`exchange_client_credentials`，scope 判定里**不查**用户权限点）。
**通配一律拒**（D241）。

**5. 令牌签名的 claim 集合？`sub` 省略怎么做到？**
claim 集合：`iss` / `aud`(=client_id) / `sub`(可缺省) / `tid` / `client_id` / `scope` / `grant_type` /
`iat` / `nbf` / `exp` / `jti`（`src/service.rs:398-422` `AccessTokenClaims`）。
`sub` 省略靠 **serde 的 `skip_serializing_if`**：`sub: Option<String>` +
`#[serde(skip_serializing_if = "Option::is_none")]`（`src/service.rs:404-405`）→
`client_credentials` 签发时置 `sub: None`（`src/service.rs:1569`）→ **序列化后该键不存在**
（不是 `null`、不是空串）。

**6. 登出怎么作废全部会话？**
`IamStore::revoke_all_refresh_tokens` **存在**（`crates/platform-iam/src/store.rs:765`）。
**现状（必须明说）**：登出**撤销 OAuth 侧的全部授权与令牌**
（`revoke_grant` → `revoke_tokens_for_grant` → `revoke_tokens_for_user`，`src/service.rs:1744~1770`），
**但已签发的登录 access token 不杀**（D229 / TD-043）—— 登录令牌由平台会话体系掌管，
本轮**有意不做**（施工单 §7 明确列为「本轮不做」）。

**7. back-channel 投递的 URL 从哪来？**
**只**取自 `platform_oauth_client.backchannel_logout_uri`（`src/logout.rs:97-99`，
从 `platform_oauth_grant` → client 行读出），**请求里不接受任何 URL 参数** → **无 SSRF 入口**。
注册时校验 scheme（`src/service.rs:639`）。
出站 client 是**独立实现**（`src/outbound.rs`，只换 connector，**不动** REQ-013 的 OIDC client，D220），
超时 **5 秒**。

**8. `{授权页基址}` 怎么读？配置读不到怎么办？**
读配置键 **`oauth.frontendBaseUrl`**（`src/config.rs:63`），**空串时回退 `{issuer}`**
（`src/config.rs:93` `frontend_base()` → `settings.frontend_base(self.issuer())`，D222）。
**读不到**（配置源报错 / 键未登记）→ `ServiceError::Internal`（`src/service.rs:498-505`）→
**500**，**不**静默回退（避免「配置坏了就用 issuer 顶上」这种静默降级）。

**9. 5 个配置项的运行时读取路径？**
`platform_config_upsert` 写进 `platform_config` → 装配层实现 `OAuthConfigSource`（`src/config.rs:110`）→
**每请求读一次**（`src/service.rs:498` `settings(ctx)` → `self.config.read(ctx).await`）。
**语义**：「配置改了要生效」= **下一个请求就生效**（不缓存）。
**代价**：每个平台端点多一次配置读 —— 这是**为了语义正确**付的代价，本轮接受。

**10. 怎么保证没有一处用了 `{issuer}` 而不是 `{授权页基址}`？**
所有**授权页 / 设备码页 / 同意页** URL 都从 `frontend_base(ctx)` 派生
（`src/service.rs:508`），只有**协议端点**（`token` / `introspect` / `revoke` / `device_authorization`）
和 `jwks_uri` 用 `{issuer}`。
`the_authorization_server_metadata_is_discoverable` 逐条断言了元数据里的
`authorization_endpoint` 用**授权页基址**、其余 4 个用 `{issuer}`。
E2E 里 `oauth.frontendBaseUrl = https://e2e.base` ≠ `issuer = http://127.0.0.1:8080` → 两者**不等**时也能判对。

**11. `/api/oauth/device/verify` 与 `/api/oauth/device/approve` 怎么保证 grant 与授权码路径同构？**
两者**走同一个** `store.upsert_grant(ctx, client_id, user_id, scopes, now)`
（`src/service.rs:962` 是设备码批准路径，`src/service.rs:1109` 是授权码交换路径）
→ **同表、同列、同唯一键** `(tenant_id, client_id, user_id)`（D202「一表三用」）。
`approve_device_code`（`src/service.rs:1442`）先经 `decide_device_code`（SECURITY DEFINER）把设备码行
**迁到用户租户**，再写 grant → 与授权码路径产出的 grant 行**逐列同构**。

**12. 启动对账 fail-fast：manifest + 种子迁移是否一致？**
确认一致：manifest 的 **6** 个权限点（`crates/platform-oauth/manifest.json`）与种子迁移
`migrations/20261120000600_oauth_permission_seed.up.sql` 的 **6** 条 INSERT **逐条对应**，
且 `source` 逐字 = `'crates/platform-oauth/manifest.json'`。
平台既有能力 `crates/platform-rbac/src/registry.rs:198` `require_consistent` 负责对账。
**验证**：`cargo xtask capabilities --check` 通过（14 个文件与源码一致）+ `permissions.json` 的 `total == 32`
→ manifest 与种子迁移**没有**漂移。

---

## 6. 与规格不一致的发现

> 每条写：**现状 / 证据（file:line）/ 怎么处理 / 需要 BA 拍板吗**。
> **不自己改规格**，也**不把偏离悄悄咽下去**。

### 6.1 协议端点的**租户来源**没有钉死 —— ⚠️ **需要 BA 拍板**

- **现状**：协议端点（`token` / `introspect` / `revoke` / `device_authorization`）**没有用户会话**
  → 没有租户上下文。本实现的口径是**优先用授权凭据推导租户**：
  1. 授权码交换 → 用**授权码行**的 `tenant_id` **覆盖**租户；
  2. 设备码交换 → 用**设备码行**的 `tenant_id` **覆盖**租户；
  3. 内省 / 撤销 → 用令牌 JWT 的 `tid` 声明**覆盖**租户；
  4. 兜底（`client_credentials`、**未批准**的设备码）→ 落 OAuth 自有保留伪租户
     **`"oauth-platform"`**。
- **证据**：
  - `crates/platform-oauth/src/store.rs` 的 `OAUTH_PLATFORM_TENANT`（常量 `"oauth-platform"`）；
  - `crates/platform-oauth/src/service.rs` 里「用行里的 `tenant_id` 覆盖租户」的分支；
  - `crates/platform-oauth/src/http.rs:204` `protocol_ctx()` 统一给协议端点兜底租户。
- **为什么用 `"oauth-platform"` 而不是 `"platform"`**：`platform-config` 的 `read_platform_entries`
  **拒绝** ctx.tenant == `"platform"`（D72 保留字）→ 用 `"platform"` 会被直接拒掉。
- **附带后果（需要一并拍板）**：设备码在**批准前**落伪租户，**批准时**要跨租户**迁移**那一行
  → 普通 `UPDATE` 做不到（RLS 的 `USING` 看**旧行**）→ 迁移里加了
  **4 个 `SECURITY DEFINER` 函数**（`platform_oauth_code_by_hash` / `platform_oauth_device_code_by_hash` /
  `platform_oauth_device_code_by_user_code_hash` / `platform_oauth_device_code_decide`），
  并 `REVOKE ALL … FROM PUBLIC` + 用 `pg_roles` 守卫 `GRANT EXECUTE TO base_platform_app`。
- **需要 BA 拍板吗**：**需要**。规格没写协议端点的租户来源；本口径是**实现选择**，
  可能与 BA 设想的（例如「一律走 `{issuer}` 所属平台租户」）不同。**建议 BA 明确写进 decisions.md。**

### 6.2 AC-S-140 与 AC-S-146 步骤 3 / TC-022-080 **互斥** —— ⚠️ **需要 BA 拍板**

- **冲突**：
  - **AC-S-140**：scope 越界必须在**令牌端点**回 `{"error":"invalid_scope"}`（TC-022-040~043）；
  - **AC-S-146 步骤 3 / TC-022-080**：**自签**（scope 更大）的票据必须在 **confirm** 回 `OAUTH_TICKET_INVALID`。
- **为什么互斥**：consent ticket 是**无状态**的（`src/ticket.rs` 签发 / 验签，不落库）。
  confirm 时**无法区分**「用户请求了更大的 scope」与「用户看到的是更大的 scope」——
  两者在票据里长得**一模一样**。若 confirm 一律校验 scope → AC-S-140 的令牌端点判据**永远走不到**
  （scope 越界在 confirm 就被拦下，`invalid_scope` 永不出现）→ **AC-S-140 没了**；
  若 confirm 不校验 → 自签票据**过得了 confirm** → TC-022-080 判不到。
- **本轮的选择（选了 AC-S-140 一侧）**：
  - `preview` **只判语法**，展示**请求的** scope（`src/service.rs:863` 注释明写「不判交集」）；
  - `confirm` **只信 ticket**（D204），**不**校验 scope（`src/service.rs:1864` 注释明写理由）；
  - **令牌端点**做三重交集 + 通配一律拒（D241）→ 协议错误体 `invalid_scope`。
- **代价（必须明说）**：confirm **不**校验 scope → **伪造的**票据能写出 grant 行；
  但令牌端点仍会拒（交集判不过 → `invalid_scope`）→ **不会越权拿到令牌**，
  只是**多写了一行** grant 记录（用户列表里会看到一条自己没同意过的授权 → 可在「我的授权」里撤销）。
- **需要 BA 拍板吗**：**需要**。两条 AC 不能同时满足，BA 需要二选一
  （或给出「票据里带用户所见 scope 的签名副本」这类第三方案）。

### 6.3 敏感扫描器扫 **JSON 名** 而不是 Rust 名 —— 需要 BA 知晓，**不需拍板**

- **现状**：静态扫描扫的是**响应里的 JSON 字段名**（`clientSecret` 等），
  **不**扫 Rust 字段名（`client_secret`）。
- **证据**：`crates/platform-oauth/src/sensitive.rs:142-158`（模块文档「为什么只扫 JSON 名」）+
  `scan_response_source` 的实现（`src/sensitive.rs:160`）。
- **理由**：Rust 字段名 `client_secret` 在**一次性交付**的响应里**合法存在一次**
  （D200 的 `clientSecret` 只此一次）→ 扫它会让判据**恒红**；
  本 crate 的响应体一律由 serde 派生构造 → **字面量** `clientSecret` 只可能来自手写 JSON → 那正是要防的泄漏面。
- **需要 BA 拍板吗**：**不需要**（口径写在代码注释里，行为符合 AC-S-136 步骤 5 的意图）。**但请 BA 知晓**：
  「敏感键扫描」= **JSON 名口径**，不是 Rust 名口径。

### 6.4 scope 判定**放在令牌端点**（不是 preview / confirm）—— 需要 BA 知晓，**不需拍板**

- **现状**：见 §6.2。`preview` 只判语法、`confirm` 只信 ticket、**令牌端点**判交集。
- **证据**：`src/service.rs:863`（preview 不判交集）、`src/service.rs:1864`（confirm 只信 ticket）、
  `src/service.rs:1917`（令牌端点「全有或全无」）。
- **需要 BA 拍板吗**：**不需要**（这是 AC-S-140 的一侧）；但 §6.2 的冲突**需要**拍板。

### 6.5 BA 建议的 M8 变异点**在 PostgreSQL 语义下无效** —— 需要 BA 知晓，**不需拍板**

- 见 §3.1.1：字面去掉 `WITH CHECK` **不破**写侧隔离（`USING` 会兜底）。
- 已改用等效的 `WITH CHECK (true)` → 判据变红。

### 6.6 模块切分**超出** design §13 的建议清单 —— 需要 BA 知晓，**不需拍板**

- **超出部分**：`protocol.rs` / `crypto.rs` / `keys.rs` / `authz.rs` / `config.rs` / `sensitive.rs` /
  `logout.rs` / `http.rs`（design §13 列的是 `error/events/model/store/service/http/client_auth/pkce/ticket/logout/outbound/sensitive`）。
- **理由**（逐条）：
  - `protocol.rs` —— 协议错误体 + 状态码**单独成模块**：它被 `service` / `http` **两边**用
    （且 D244 要求**逐字**形状）→ 内联在 `http.rs` 会形成反向依赖；
  - `crypto.rs` —— `digest_base64url` 被 `store` / `client_auth` / `service` / 测试共用
    （D228 的口径是**跨栈**统一项）→ 单点可钉死；
  - `keys.rs` —— 令牌签发 / 验签（ES256）单独成模块，避免与 `service` 的业务编排混在一起；
  - `authz.rs` —— 6 个 `require_permission` 调用点**必须**集中在一处（C-5 只扫本 crate 的 `src/`）；
  - `config.rs` —— `OAuthConfigSource` + `OAuthSettings` 是**接缝**（装配层实现）→ 独立便于替换；
  - `http.rs` —— 平台端点 + 协议端点 + `router()` + `metadata_router()`（design §13 有列，位置调整）。
- **需要 BA 拍板吗**：**不需要**（施工单 §3.1 明写「建议模块切分（design §13 已给，**可微调**，但文件清单要在报告里说清）」）。
  本节即「说清」。

### 6.7 `oauth-platform` 伪租户 —— 需要 BA 知晓，**不需拍板**

- 见 §6.1 第 4 点与「为什么用 `"oauth-platform"`」。这是**实现细节**，但它会出现在
  `platform_oauth_code` / `platform_oauth_device_code` / `platform_oauth_token` 的 `tenant_id` 列里
  → QA / 棒 2 排数据时可能撞见 → 提前说明。

### 6.8 `AGENTS.md` 因 `cargo xtask capabilities` 重新生成而**被动同步**（不在 §1.1 文件集内）

- 见 §9。

### 6.9 「2 路并发」的授权码一次性判据**对顺序变异不敏感** —— 需要 BA 知晓，**不需拍板**

- **现状**：本轮自查时发现，原 `a_code_can_only_be_exchanged_once_under_concurrency` 用 **2 路**并发，
  对「把删除挪到签发之后」这类**顺序**变异只有约 **1/6** 的检出率 → 判据偏软。
- **处理**：已把并发路数提到 **4**，实测检出率 **10/10**（未变异 **10/10 绿**，无假阳性）。
  见 §3.1.4。
- **需要 BA 拍板吗**：**不需要**（判据改硬，规格未动）。**请 BA 知晓**：
  BA 用例集里的并发类用例若沿用 2 路，在 QA 复核「一次性」时可能得到**假绿**。

---

## 7. 未完成的部分 + 原因

| 项 | 状态 | 原因 |
|---|---|---|
| **E2E 实跑**（`e2e/oauth.spec.ts` 的 8 个用例） | **未跑** | 施工单 §5「⚠️ **本阶段不跑 E2E**（代码未冻结，棒 2 还没落地）；终版（代码冻结后、合并前）由 BA **集中跑一次**全量 + E2E」。文件已写好，已过 `tsc` + `eslint` + `prettier --check`，**0 skipped** |
| `scripts/e2e-server.sh` **不能从零建库** | **未修** | TD-039 明确「**记录**即可」；清理后必须手工补 3 步：`deploy/seed/02-app-role.sql` + `modules/sandbox/migrations/` + `cargo xtask tenant-policies` |
| `cargo fmt --all --check` 的 **11 个存量文件** | **未修** | TD-017：基线就是红的 → 只格式化**自己改过的文件**（`cargo fmt -p platform-oauth -p platform-iam -p platform-core -p xtask` + `rustfmt` 两个 platform-server 文件） |
| **refresh token** | **有意不做** | D217 |
| 底座业务端点接受第三方 OAuth 令牌（opt-in） | **有意不做** | D209 / TD-041 |
| 登出杀掉已签发的**登录** access token | **有意不做** | D229 / TD-043 —— 现状已在 §5 第 6 条**显式写明** |
| `client_secret` 轮换端点 | **有意不做** | D226 |
| 多租户各自的 client（client 是**平台级**） | **有意不做** | D219（`platform_oauth_client` 无 `tenant_id`、不启用 RLS） |
| **提交 commit** | **未做** | 施工单抬头「**不要提交**。改动留在工作区，由 **BA 统一提交**」 |
| 131 条用例的**逐条**用例级实现 | **未做（有意）** | 施工单 §5.3 的要求是「逐条映射到你的测试函数名，并在报告里给出对照表」→ 已给（§4）；131 条**用例文件**本身是**测试设计**产物（BA / QA 侧），棒 1 只提供**判据实现** |
| AC-S-140 与 AC-S-146 的**冲突** | **未消解** | 规格内在冲突（§6.2）→ **等 BA 拍板**，本轮选了 AC-S-140 一侧 |

---

## 8. 棒 2 接入时必须遵守的形状

### 8.1 前端路由（棒 2 建）

```
/oauth/authorize   /oauth/device   /oauth/grants   /oauth/logout
```

### 8.2 后端端点（棒 2 调的）

**平台端点**（**用户会话** `Authorization: Bearer <登录令牌>`；错误 = RFC 9457 Problem Details + `OAUTH_*` 大写码）：

| 方法 + 路径 | 请求 | 响应 |
|---|---|---|
| `POST /api/oauth/authorize/preview` | `clientId` / `redirectUri` / `responseType` / `scope` / `state` / `codeChallenge` / `codeChallengeMethod` / `prompt` | `needsConsent` / `ticket` / `application` / `scopes` / `redirectTo` |
| `POST /api/oauth/authorize/confirm` | `ticket` | `redirectTo` |
| `POST /api/oauth/authorize/deny` | `ticket` | `redirectTo`（带 `error=access_denied`） |
| `GET /api/oauth/device/verify` | query `user_code` | 设备码页信息 |
| `POST /api/oauth/device/approve` | `user_code` | 批准结果 |
| `GET /api/oauth/clients` | — | `clientId` / `clientName` / `status` / `allowedScopes` / `redirectUris` / `allowedGrantTypes` / `backchannelLogoutUri` / `frontchannelLogoutUri`（**无** `clientSecret`） |
| `POST /api/oauth/clients` | `name` / `clientType` / `redirectUris` / `allowedGrantTypes` / `allowedScopes` / `backchannelLogoutUri` / `frontchannelLogoutUri` | **201** + `clientId` + **`clientSecret`（仅此一次）** |
| `GET /api/oauth/clients/{client_id}` | — | 同列表项（**无** `clientSecret`） |
| `PATCH /api/oauth/clients/{client_id}` | `status`（本轮**只支持改 `status`**） | 同上 |
| `DELETE /api/oauth/clients/{client_id}` | — | 204 |
| `GET /api/oauth/grants` | — | **数组**：`clientId` / `clientName` / `scopes` / `grantedAt` / `status` / `revokedAt` |
| `POST /api/oauth/grants/revoke` | `clientId` | `status`（`revoked`） |
| `POST /api/oauth/logout` | — | `frontChannel`（**可能是空数组**，不是 `null`） |

**协议端点**（**client 认证** `Authorization: Basic base64(client_id:client_secret)`；
错误 = `{"error":"<小写码>"}` / `{"error":"…","error_description":"…"}`）：

| 方法 + 路径 | 备注 |
|---|---|
| `POST /api/oauth/token` | `grant_type` ∈ `authorization_code` / `client_credentials` / `urn:ietf:params:oauth:grant-type:device_code`；字段 **snake_case** |
| `POST /api/oauth/device_authorization` | RFC 8628 |
| `POST /api/oauth/introspect` | 只能内省**自己的**令牌 |
| `POST /api/oauth/revoke` | RFC 7009 |
| `GET /.well-known/oauth-authorization-server` | 元数据 |

### 8.3 字段名口径（D258）

- **平台端点 = camelCase**；**协议端点 = RFC 的 snake_case**；**两套不混**。
- `scope` 在**请求**里是**空格分隔的字符串**；在**响应**里是**数组**。
- `frontChannel` **可能为空数组**（不是 `null`）。

### 8.4 错误码（棒 2 的词库登记用）

**平台错误码恰好 9 个**（`OAUTH_CLIENT_NOT_FOUND` / `OAUTH_CLIENT_DISABLED` / `OAUTH_CLIENT_EXPIRED` /
`OAUTH_INVALID_REQUEST` / `OAUTH_INVALID_REDIRECT_URI` / `OAUTH_INVALID_SCOPE` /
`OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` / `OAUTH_TICKET_INVALID` / `OAUTH_GRANT_NOT_FOUND`）。

**协议错误码 14 个**（小写）：`invalid_request` / `invalid_client` / `invalid_grant` /
`unauthorized_client` / `unsupported_grant_type` / `unsupported_response_type` / `invalid_scope` /
`access_denied` / `login_required` / `consent_required` / `authorization_pending` / `slow_down` /
`expired_token` / `server_error`。
**集合里没有 `invalid_token`** → **不得使用**（D224）。

### 8.5 权限点（棒 2 的菜单 / 按钮用）

```
oauth:client:read   oauth:client:write   oauth:client:delete
oauth:grant:read    oauth:grant:revoke   oauth:token:introspect
```

`crates/platform-oauth/manifest.json` 的 `menus[].route` **逐字** = **`/oauth/grants`**
（棒 2 的路由表里也**必须**是这个字符串）。

### 8.6 审计事件码（**8** 个，**不进 `TITLES`**）

```
oauth.client.registered   oauth.client.updated      oauth.client.deleted
oauth.authorization.approved  oauth.authorization.denied
oauth.token.issued        oauth.token.revoked        oauth.logout.executed
```

### 8.7 E2E 侧

- 第三方回调用 **`page.route('https://client.example/**')`** 拦截（D235）。
- 读 code 用 **`new URL(page.url()).searchParams.get('code')`**（**不是**地址栏裸字符串）。
- 登录令牌从 **`target/e2e-oauth-clients.json`** 读**明文 secret**（`target/` 不入库）。
- 夹具顺序：**先起 mock → 再播种**（client 的 backchannel URI 含 `$MOCK_PORT`）。

---

## 9. 对棒 1 文件集（施工单 §1.1）之外的任何改动

> 施工单 §6 第 9 条：「一律列出来（**应该是空的**）」。
> 本轮**不是空的** → 逐条列明，并说明原因。

| 文件 | 改动 | 为什么在文件集外 | 可否回退 |
|---|---|---|---|
| `crates/platform-server/Cargo.toml` | **+2** 行：`platform-oauth.workspace = true` + 一行注释 | §1.1 只列了 `crates/platform-server/src/main.rs`，但 §3.2 要求在 `main.rs` **装配** `/api/oauth/*` → 装配**必须**依赖该 crate。**这是 §3.2 的必然连带** | **不可**（去掉就编不过） |
| `AGENTS.md` | **3** 行：`audit_sink` 从 `not_yet_provided` 移到 `already_provided`（+ 表里加一行）+ 两处计数（27 → 28 / 5 → 4） | §1.1 有 `capabilities/**`（要 `cargo xtask capabilities` 重新生成）。重新生成后 `capabilities/index.json` 把 REQ-014 已落地的 `audit_sink` 从「尚未提供」挪到「已有能力」→ `crates/xtask/src/catalog.rs` 的 `agents_doc_matches_catalog` **强制** `AGENTS.md` 的表与能力目录**逐条一致** → 不改就**红** | **不可**（不改 `cargo test -p xtask` 红） |

**说明**：这两处改动**都不涉及** REQ-022 的业务语义 —— `AGENTS.md` 的 3 行是 **REQ-014** 的 `audit_sink` 迁移
（`capabilities/index.json` 的重新生成把它带了出来），`platform-server/Cargo.toml` 是装配的依赖声明。

**棒 2 的文件集（`apps/shell/**`、`packages/i18n/**`、`feature-meta/req-022-dev-frontend.md`）**
本轮**一行未碰**（工作区里那些改动是**棒 2 自己的**，见 `git status`）。

**其它核对**：

- `crates/platform-db/src/rls.rs` —— **最终**未改动（`git status crates/platform-db/` 为空）。
  ⚠️ 但它是 **M8 变异**的注入点（临时改、逐字节还原 + `sha256sum -c` 全过 + `grep -c MUTATION` = 0）；
  见 §3.1.1 的口径修正说明。
- `crates/platform-db/src/**` 其余文件、`crates/platform-rbac/**`、`crates/platform-config/**`、
  `crates/platform-contracts/**` —— **一行未碰**。
- 全仓 `grep -rn 'MUTATION' crates/ scripts/ e2e/ migrations/` → **零残留**。

---

## 10. 完工信号

| 项 | 值 |
|---|---|
| 报告绝对路径 | `/home/myg133/Projects/system_seed/feature-REQ-022/feature-meta/req-022-dev-backend.md` |
| 逐 crate passed / failed / ignored | 见 §2.1 / §2.2（常规跑合计 **654 / 0 / 22**；含 ignored 复跑 **676 / 0 / 0**） |
| 变异验证条数 | **9 / 9** 全部**变红**；全部**还原干净**（`sha256sum -c` 全过 + `grep -c 'MUTATION'` 全 **0** + 全仓零残留）；并发类判据**检出率实测 10/10** |
| AC 覆盖 | **22 / 22** |
| 未完成项 | 见 §7（E2E 实跑按施工单**本阶段不跑**；其余为「本轮不做」的既定范围） |
| 需要 BA 拍板 | **2 条**：§6.1（协议端点租户来源）、§6.2（AC-S-140 与 AC-S-146 / TC-022-080 互斥） |
| 文件集外改动 | **2 处**（均已在 §9 列明并说明原因）：`crates/platform-server/Cargo.toml`（装配依赖，§3.2 必然连带）、`AGENTS.md`（`cargo xtask capabilities` 重新生成的必然连带） |
| commit | **未提交**（按施工单抬头） |
