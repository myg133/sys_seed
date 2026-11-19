# REQ-013 修复轮 1 自验 —— QA Pre-merge 复验的 10 条（证据链补完）

- **需求**: REQ-013「SSO 入站：OIDC + LDAP + 强制 SSO」
- **分支 / 基线**: `feature/REQ-013` @ `3d02c21`
- **依据**: `feature-meta/qa-premereq-req013.md`（10 条）+ `acceptance.md` **rev 3.3**（P16 / P17 + 5 条 AC 改注）
  + `decisions.md` **D128**（4 项裁定）+ `dev-guide-low-spec.md` §9.2（阳性对照）/ §9.4（还原后 `touch`）
- **本轮性质**: **只补验收证据链，不改功能**。压测基线是 **753 passed**。
- **结论**: 10 条全部处理完；**实现逻辑零改动**（只有 `ldap.rs` 把已有的判定抽成一个函数，
  语义完全一致 —— 见 Q4）；9 个 crate 全绿，**760 passed / 0 failed / 0 ignored**（+7）。
  每条新增断言都做了「会红」的变异验证（含 Q1 / Q3 / Q4 / Q5 / Q6 / Q7 / Q9）。

---

## 1. 逐 crate 回归

命令（逐 crate、串行、`--all-targets`、单 `--`、`--include-ignored` 在 `--` 之后）：

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
cd feature-REQ-013
for c in platform-core platform-config platform-db platform-iam platform-rbac \
         platform-org platform-schema platform-server sandbox; do
  cargo test -p "$c" --all-targets -- --test-threads=1 --include-ignored
done
```

| crate | 本轮 | 基线 | 差 | failed | ignored |
|---|---|---|---|---|---|
| platform-core | 127 | 127 | 0 | 0 | 0 |
| platform-config | **85** | 83 | **+2** | 0 | 0 |
| platform-db | 46 | 46 | 0 | 0 | 0 |
| platform-iam | **168** | 165 | **+3** | 0 | 0 |
| platform-rbac | 78 | 78 | 0 | 0 | 0 |
| platform-org | 91 | 91 | 0 | 0 | 0 |
| platform-schema | 20 | 20 | 0 | 0 | 0 |
| platform-server | **112** | 110 | **+2** | 0 | 0 |
| sandbox | 33 | 33 | 0 | 0 | 0 |
| **合计** | **760** | 753 | **+7** | **0** | **0** |

**改动文件**（不含 `feature-meta/` 报告本身）：

| 文件 | 性质 |
|---|---|
| `crates/platform-iam/src/sso/ldap.rs` | **接缝**：把已有的 `rc == 0` 判定抽成 `bind_succeeded` / `bind_result`（P16）；`+3` 单测 |
| `crates/platform-iam/tests/sso_audit.rs` | Q1：阳性对照改成真注入；`+2` 辅助函数（不是测试） |
| `crates/platform-iam/tests/sso_config_secret.rs` | Q5：`ssoProviders` 断言内容；`+1` 辅助函数 |
| `crates/platform-iam/tests/sso_ldap.rs` | Q8：对齐 D128 措辞 + 补「绑定未被改写」；Q10：`permissions` 元素类型 |
| `crates/platform-iam/tests/sso_oidc.rs` | Q10：`permissions` 元素类型（不越位断言预期 5） |
| `crates/platform-iam/tests/sso_oidc_security.rs` | Q9：`jwks_hits >= 2` |
| `crates/platform-iam/tests/sso_sessions.rs` | Q2：删自陈注释、断言 200 + `revokedSessions` 回执 |
| `crates/platform-iam/tests/sso_support/mod.rs` | Q9：mock `/jwks` 计数器 |
| `crates/platform-server/tests/sso_real_db.rs` | Q5 + Q6：`+1` 测试（双租户 enforce）、`ssoProviders` 内容、前置补 LDAP 键 |
| `crates/platform-server/tests/sso_me_permissions_real_db.rs` | **新文件**：Q3（真实 IAM + 真实 RBAC） |
| `crates/platform-server/Cargo.toml` | Q3 的 dev-deps（`jsonwebtoken` / `base64` / `sha2` / `time`，**全是已有 workspace 依赖**） |
| `crates/platform-config/src/registry.rs` | Q7：`+1` 单测（`#[cfg(test)]` 里） |
| `crates/platform-config/tests/config_real_db.rs` | Q7：`+1` 真实库测试（两条写路由） |

**依赖预算（§9.10）**：`platform-server` 的 4 个 dev-dependency 都是**工作区里已有的**（`Cargo.lock`
只给 `platform-server` 这一节加了 4 行成员标记），**没有新增任何外部包**，生产依赖树不变。
`platform-iam` 最终**没有**加 `platform-rbac` 的 dev-dep（我试过能编译，但 Q3 放在 `platform-server`
更符合 AGENTS.md 的「不存在环」往外读 —— 见 §4.3）。

---

## 2. 10 条逐条处理

### Q1（阻塞）审计的阳性对照是恒真断言 —— **已修**

- **改了什么**：`ac_s_108_no_payload_carries_a_secret` 重写。扫描判据抽成共用的
  `scan_hits(haystack, needle) -> usize`；真实 payload 走 0 命中，阳性对照走
  「**真实的 `SsoAuditEvent` + `user_agent = CANARY`** 序列化后再用同一个 `scan_hits`」。
- **新断言**：
  1. `recorded_payload_text(&recorder)` 非空（先证明**录到了** payload，否则「0 命中」没有意义）；
  2. 6 类敏感串在真实 payload 上 `scan_hits(...) == 0`；
  3. `scan_hits(注入的真实 payload, CANARY) == 1`；
  4. 拼接形态 `scan_hits(surfaces + injected, CANARY) == 1`。
- **什么情况下会红**：
  - 把 `scan_hits` 改成恒 0（扫描器失效）→ 第 3 条红（**已实测**，见 §3.1）；
  - 序列化层把 `user_agent` 从白名单里丢掉 → 第 3 条红；
  - 审计 payload 真的混进 canary / token / 密码 → 第 2 条红（`== 0` 变成 1）；
  - recorder 一条都没录到 → 第 1 条红（防止「扫描空字符串也算 0 命中」）。
- 旧的 `format!("{dump}\n{CANARY}").contains(CANARY)` **已彻底删除**。

### Q2（阻塞）`logout-all` 返回码 —— **按 D128 改规格，实现不动**

- **改了什么**：删掉 `sso_sessions.rs` 里那条「因 REQ-007 既有契约本轮不动」的自陈注释，
  换成「rev 3.3 由 D128 裁定 = 200」的说明；断言保持 `StatusCode::OK`，
  并**新增**对 `{"revokedSessions": n}` 的回执断言（`n >= 1`）。
- **什么情况下会红**：实现改回 204（`assert_eq!(status, OK)` 红）；
  或 `revokedSessions` 字段丢了 / 是 0 / 不是数字（新增那条红）。
- 规格已改，**实现零改动**。

### Q3（阻塞）AC-S-100 步骤 5 零测试 —— **已补（新文件）**

- **改了什么**：新增 `crates/platform-server/tests/sso_me_permissions_real_db.rs`，
  在**装配层**（真实 `IamService` + 真实 `RbacService` + 真实 `/api/rbac/*` 路由 + 进程内 mock IdP）跑：
  `authorize → callback → exchange` 拿到 SSO 的 `accessToken` → 打 `GET /api/rbac/me/permissions`。
- **前置**：给 `bob` 建一个自定义角色并授 `rbac:role:read`（**迁移里已种下的权限点**，不新增注册表条目）。
- **新断言**：
  1. `/me/permissions` → `200`；
  2. `permissions` **`!is_empty()`**（旧用例下 `[]` 也绿 —— 这就是 Q3 的缺口）；
  3. 包含 `rbac:role:read`（bob 角色对应的**具体**权限点）；
  4. 与登录响应里的 `permissions` **逐项相等**（两个入口同一份数据）；
  5. 库里该会话的 `auth_provider == "oidc"`（把这钉成 SSO 会话，不是本地登录）。
- **什么情况下会红**：
  - SSO 会话没拿到权限点（`permissions == []`）→ 第 2 条红，**已实测**（§3.2，报错正文就是 `{"permissions":[]}`）；
  - 登录响应与 `/me/permissions` 分叉 → 第 4 条红，**已实测**（§3.2 第一个变异）；
  - SSO 会话的 `user_id` / `tenant_id` 没进令牌 → 第 1 条 401/404；
  - 角色绑到了别的租户 → 第 3 条红。

### Q4（高）`Ldap3Conn::simple_bind` 零覆盖 —— **按 P16 选项 (A)+(B) 已补**

- **改了什么**（`crates/platform-iam/src/sso/ldap.rs`）：
  - 新增 `pub fn bind_succeeded(rc: u32) -> bool { rc == 0 }`（P16-A 纯函数）；
  - 新增 `pub fn bind_result(result: &ldap3::LdapResult) -> bool`（P16-B：真实 `LdapResult` 可被直接驱动，
    且签名**不接受 `Result`** —— 把「外层 `Ok` ≠ 认证成功」做进类型）；
  - 生产包装层那一行改成 `Ok(bind_result(&result))`。
    **语义与改前的 `.success().is_ok()` 逐值一致**（见下条测试），不是功能改动。
- **新断言（3 条单测）**：
  1. `bind_succeeded_only_accepts_rc_zero`：`0→true` / `49→false` / `32→false` / `1→false` / `u32::MAX→false`；
  2. `bind_succeeded_agrees_with_ldap3_success_semantics`：对 `{0,1,32,48,49,50,51,53}` 逐值断言
     `bind_succeeded(rc) == LdapResult::success().is_ok()`，且 `0..=60` 里**恰好只有 rc=0 成功**；
  3. `a_real_ldap_result_with_rc_49_is_a_failed_bind`：构造 `ldap3::LdapResult { rc: 49, .. }`
     → `bind_result == false`；`rc: 0` → `true`。
- **什么情况下会红**：`bind_succeeded` 改成恒 `true`（= 「任意密码都能登录」）→ 三条全红（**已实测**，§3.3）；
  「偶发成功」这种 rc 抽样也会被第 2 条的 `0..=60` 全枚举抓到。
- **变异验证证据**见 §3.3（会红 + 还原后绿 + sha256 前后一致 + `touch`）。

### Q5（高）`params.ssoProviders` 只断言 `is_array()` —— **已修（两处）**

- **改了什么**：
  - `sso_config_secret.rs`：新增 `provider_kinds(&body)` 辅助，断言
    `["oidc", "ldap"]`（逐元素取 `type`）；
  - `sso_real_db.rs`：同样断言 `vec!["oidc", "ldap"]`；并把前置补齐 ——
    原来只写 OIDC 的键，现在**同时写 LDAP 的 5 个键**（否则按 AC 前置根本不该出现 ldap）。
- **什么情况下会红**：`providers()` 退化成空列表（`enforce_sso` 读失败分支正是 `Vec::new()`）；
  少一个 provider；顺序/内容变了。**已实测**两处都会红（§3.4）。

### Q6（中）AC-S-021 步骤 4 未真正验「enforce 是租户级」 —— **已补**

- **改了什么**：新增 `ac_s_021_enforcement_is_tenant_scoped_through_the_real_config_service`
  （`platform-server/tests/sso_real_db.rs`），用**真实 `ConfigService`**：
  `tenant-a` 写 `sso.enforce=true` + OIDC/LDAP 全套；`tenant-b` **一个键都不写**。
- **新断言**：
  1. a → `403 SSO_REQUIRED`，且 **Argon2 计数差 == 0**（密码校验之前返回）；
  2. b → `200`，且 **Argon2 计数 > 0**（对照组，让「== 0」有区分力）；
  3. 再打一次 a 仍 `403`（两套策略**并存**，不是「切回」）。
- **什么情况下会红**：把 `enforcement()` 改成「读到就 true」（= 全局开关）→ b 也 403，
  第 2 条红，**已实测**（§3.5，报错正文就是 b 的 403 体）；
  a 侧判定跑到 Argon2 之后 → 第 1 条红；读配置时忽略 `ctx` → 两侧对调。
- 原 `StaticSsoConfigSource` 那条用例**保留**（它覆盖步骤 7「改回 false 后恢复」，那是另一个要求）。

### Q7（中）D127 的 config 推导改动无专测 —— **已补两条**

- **改了什么**：
  - `registry.rs`（`#[cfg(test)]`）：`a_dotted_boolean_key_is_plain_config_and_not_a_flag`
    —— 4 个 SSO 布尔键 `!is_flag` / 类型是 `Boolean` / 无 `flags.<key>.rolloutPercent` 派生键 /
    `!is_flag_namespace`；并**固化既有行为**：`is_flag("beta")` / `is_flag("newInvoice")` 仍成立、
    `flags.beta.rolloutPercent` 仍是 `Number`；
  - `config_real_db.rs`：`a_dotted_boolean_key_is_writable_as_config_and_rejected_as_a_flag`
    —— 带点号布尔键走 **Scoped 路由成功**（并断言值真的落到 `(TENANT_A, key)` 那一行、读回 `source == Tenant`）、
    走 **flag 路由被拒**（400 `FLAG_NAME_INVALID`）；再加既有契约：`beta` 走 Scoped 仍 `CONFIG_FLAG_USE_FLAG_ENDPOINT`、
    走 flag 接口可改。
- **什么情况下会红**：删回 `kind == Boolean` 一行约束 →
  单测第 1 条红、集成测试在「Scoped 写应成功」处红（报 `FlagEndpointRequired`）。**两处都已实测**（§3.6）。
- 注意：断言读库时按 `(tenant_id, key)` 取 —— 裸 `WHERE key = $1` 会读到别的测试二进制在同一张
  `base_test` 里留下的行（我第一版就踩了这个，已修）。

### Q8（中）AC-S-105 步骤 6 —— **按 D128 对齐措辞**

- **改了什么**：`sso_ldap.rs` 的 `ac_s_105_a_mail_colliding_with_another_bound_account_is_409`
  **内容对齐 rev 3.3**：身份从 `frank` 改为 `henry`（AC 现在的措辞），DN 未绑定 + `mail` 撞 `gina`
  （测试函数名不变，只改 fixture 与注释）；把「原步骤不可达」的自陈注释换成「见 D128 裁定」。
  **新增一条断言**：冲突后 `gina` 的绑定行仍是 `uid=gina,...`（**没有被静默改写**）——
  这样「409」不只是状态码，还证明了副作用为零。
- **什么情况下会红**：实现改成「静默接管」（返回 200）→ 状态码红；
  实现「返回 409 但把绑定行改指了」→ 新增的快照断言红。

### Q9（低）AC-S-102 步骤 7 只断言「成功」 —— **已补**

- **改了什么**：给 mock IdP 的 `/jwks` 加**真实计数器**（`sso_support::MockIdp::jwks_hits()`），
  用例在轮换前后各取一次。
- **新断言**：首次登录后 `hits_before >= 1`；轮换 + 未知 kid 登录后 `hits_after >= hits_before + 1`；
  且 `current_kid() != "kid-1"`（前置成立）。
- **什么情况下会红**：
  - 未知 kid 不刷新 JWKS → 计数不增（**已实测**：`signing_keys` 忽略 `force` 时会红）；
  - 计数器本身失效（我把它改成不计数）→ `hits_before >= 1` 立刻红（**已实测**，说明这条断言依赖真实计数）。

### Q10（低）三处 `is_array()` —— **两处已随 Q3/Q5 转强，第三处收紧**

- Q3（`sso_oidc.rs` 的 `permissions`）+ Q5（两处 `ssoProviders`）已由 Q3/Q5 覆盖。
- **第三处**是 `sso_ldap.rs:97` 的 `body["permissions"].is_array()`：改成
  「是数组**且每个元素都是字符串**」（`is_array()` 连 `[null]` / `[1,2]` 也放行）。
  `sso_oidc.rs` 那处也顺手做同样收紧，并在注释里**明确不越位**：
  「非空」是预期 5 的要求，由 Q3 的装配层用例负责 —— 这里没有注入权限点提供者，断言非空会是错的。
- **什么情况下会红**：`permissions` 从字符串数组退化成含 `null`/数字，或退化成非数组。

---

## 3. 变异性验证证据（「会红」逐条实测）

### 3.1 Q1：扫描判据失效

变异：`scan_hits` 改成恒 `0`。

```
test ac_s_108_no_payload_carries_a_secret ... FAILED
panicked at sso_audit.rs:335:
assertion `left == right` failed: 扫描判据必须能命中注入到 user_agent 的 canary（否则上面的 0 命中不可信）
```

还原后：`1 passed`。

### 3.2 Q3：SSO 会话拿不到权限点

变异 A（`effective_permissions` 恒返回空）→ 第 4 条（两入口一致）红：
```
assertion `left == right` failed: 登录响应与 /me/permissions 必须是同一份数据
```

变异 B（RBAC `effective_permissions` 恒返回空集，模拟「整条链路无权限点」）→ **第 2 条（非空）红**：
```
SSO 会话必须拿到真实权限点（空集正是本用例要防的假绿）：{"permissions":[]}
```
**这正是旧用例的假绿形态**：`{"permissions":[]}` 下 `is_array()` 仍然绿。

两次还原后：`1 passed`。`service.rs` / `platform-rbac/src/service.rs` 的 `git diff` 均为空。

### 3.3 Q4：`bind_succeeded` 改成恒 `true`（= 「任意密码都能登录」）

变异前 sha256：`8de2659e4c74b7589e2d0779ed5b2c29acd56d9a7a4ff105d27dfb4ddc54081b`
变异后 sha256：`c2242d6d3809bca5658038d8f6f49213b713286babfaf65d74be69fbdb902af3`

```
test sso::ldap::tests::a_real_ldap_result_with_rc_49_is_a_failed_bind ... FAILED
test sso::ldap::tests::bind_succeeded_agrees_with_ldap3_success_semantics ... FAILED
  assertion `left == right` failed: rc=1：bind_succeeded 与 ldap3::LdapResult::success() 不一致
test sso::ldap::tests::bind_succeeded_only_accepts_rc_zero ... FAILED
test result: FAILED. 4 passed; 3 failed
```

还原（`cp` 回原文件 + **`touch`**，出口标准 9.4）后：
- sha256：`8de2659e4c74b7589e2d0779ed5b2c29acd56d9a7a4ff105d27dfb4ddc54081b` —— **与变异前逐字节相同**；
- mtime 已刷新到还原时刻（`2026-11-20 02:55:34`），cargo 不会复用陈旧二进制；
- `sso::ldap` 7 passed / 0 failed。

### 3.4 Q5：`providers()` 恒返回空列表

```
sso_config_secret: ac_s_021_enforced_sso_rejects_local_login_without_touching_argon2 ... FAILED
sso_real_db:      ac_s_021_enforcement_works_through_the_real_config_service ... FAILED
  assertion `left == right` failed: params.ssoProviders 必须列出两个可用入口：... "ssoProviders":[]
```
还原后两处皆绿。

### 3.5 Q6：`enforcement()` 改成「读到就 true」（全局开关）

```
ac_s_021_enforcement_is_tenant_scoped_through_the_real_config_service ... FAILED
assertion `left == right` failed: tenant-b 没开 enforce，本地登录必须仍然可用：
{"code":"SSO_REQUIRED", ... "params":{"ssoProviders":[]}, "status":403, ...}
```
还原后 `sso_real_db` 4 passed。

### 3.6 Q7：删回 `kind == Boolean` 一行约束

```
registry::tests::a_dotted_boolean_key_is_plain_config_and_not_a_flag ... FAILED
config_real_db: a_dotted_boolean_key_is_writable_as_config_and_rejected_as_a_flag ... FAILED
  sso.enforce 必须能经通用写路由写入（D127 约束前的症状是 CONFIG_FLAG_USE_FLAG_ENDPOINT）：
  FlagEndpointRequired { key: "sso.enforce" }
```
还原后两处皆绿。

### 3.7 Q9：未知 kid 不强制刷新

两个变异都实测会红：
- `signing_keys(..., force: true)` → `false`：回调失败，用例红；
- `signing_keys` 忽略 `force`（永远走缓存）：用例红；
- 把 mock 的 `/jwks` 计数器改成不计数：`首次登录必须先取一次 JWKS ...：0` 红。

还原后 `sso_oidc_security` 12 passed。

> **变异残留核查**：所有变异文件都用 `git diff --quiet` 复查过 ——
> `platform-iam/src/service.rs`、`platform-rbac/src/service.rs`、`platform-iam/src/sso/config.rs`、
> `platform-iam/src/sso/oidc.rs` **全部 UNCHANGED**。

---

## 4. 我认为需要说清楚的三点

### 4.1 唯一的实现侧改动是「抽接缝」，不是「改行为」

`ldap.rs` 的 `Ok(result.success().is_ok())` → `Ok(bind_result(&result))`，
而 `bind_result(r) = bind_succeeded(r.rc) = (r.rc == 0)`。
有一条测试**逐值证明**它与 `ldap3::LdapResult::success()` 在 `rc ∈ {0,1,32,48,49,50,51,53}` 上一致，
并在 `0..=60` 上枚举证明「只有 0 成功」。这是 P16 明确要求的做法（选项 A + B 都做了）。

### 4.2 Q3 放在 `platform-server` 而不是 `platform-iam`

我实测过 `platform-iam` 加 `[dev-dependencies] platform-rbac` 能编译（Cargo 允许测试用的环）——
但 Q3 要验的是**装配**行为（权限点提供者与 `/api/rbac/*` 都是装配层接的），
放在 `platform-server` 才验的是「生产装配顺序」，也避免在 `platform-iam` 引入一条
AGENTS.md 明确写着「不能依赖」的反向边（哪怕只在 dev-deps）。最终**没加**这条 dev-dep。

### 4.3 我判断 QA 可能有一处表述不够精确（但结论成立）

Q10 说「三处 `is_array()`」并点名 `sso_oidc.rs:245` / `sso_config_secret.rs:389` / `sso_real_db.rs:359`。
实际上 `sso_ldap.rs:97` 也有一处（`body["permissions"].is_array()`）——
所以是**四处**而不是三处。QA 自己也说「仅两处构成实质缺口」，结论没错，只是清单少列了一处。
本轮把这一处也收紧了（`is_array` + 元素类型）。

---

## 5. 仍不足的地方（诚实标注）

1. **`Ldap3Conn::simple_bind` 的 async 外壳仍然没有被执行过**。
   P16 允许二选一，我做的是「判定函数可机械验证」（A+B）。但「wrapper 里 `await` 之后
   忘了调用判定函数」这种回归，目前的单测抓不到 —— 那需要一条真实的 LDAP 服务端
   （`ldap3` 没有内存实现，需要自建 mock LDAP server，属新依赖范畴）。
   现状是：`bind_result` 被生产代码**唯一**调用点使用（`grep` 可核），
   且该函数自身被 3 条测试覆盖。
2. **Q9 的 `jwks_hits >= 2` 只覆盖 http mock**。https 那条路径（P10 自签根）没有同样的计数断言，
   不过它对 JWKS 刷新的要求与 http 相同，且 https 用例已断言登录成功。
3. **`ssoProviders` 的内容断言依赖「配置校验通过」**：`providers()` 在 `validate()` 失败时返回空。
   所以断言 `["oidc","ldap"]` 同时也在断言两份配置都合法 —— 这是 AC 前置的要求，不是多余耦合。
4. **未跑前端 / E2E**：本轮范围是后端证据链；`apps/` 与 `e2e/` 按任务书不动。
5. **`config_real_db.rs` 的读库断言必须按 `(tenant_id, key)` 取**。同一张 `base_test` 表会被
   多个测试二进制共用，裸 `WHERE key = $1` 会读到别人的行（我第一版踩过，已修并加了注释）。
   以后在这个文件里加读库断言要注意同一点。
