# REQ-013 修复轮独立复核（QA Pre-merge 10 条的「修复是否真实」）

- **复核者**：Verifier（独立复核，不采信修复轮自验报告）
- **被验对象**：`/home/myg133/Projects/system_seed/feature-REQ-013`，分支 `feature/REQ-013`，HEAD `a8e5157`
- **验收依据**：`BA/feature-meta/qa-premereq-req013.md`（10 条）+ `acceptance.md` **rev 3.3**（P16 / P17）+ `decisions.md` **D128**
- **复核时间**：2026-11-20 03:15 ~ 04:05
- **环境**：Arch Linux / 8 线程 / `CARGO_TARGET_DIR=code/target` / `base_test`（podman `base-postgres`）
- **本轮权限**：允许在 feature worktree 做**临时变异**，但必须还原 + `touch` + `sha256` 逐字节核对 + `git status` 干净。
  **最终状态：`git status` 为空、7 个被动过的文件 sha256 与基线逐字节相同、无 stash、无残留。**

---

## 汇总

| 级别 | 数量 |
|---|---|
| 阻塞 | **0** |
| 高 | **0** |
| 中 | **2** |
| 低 | **2** |

**结论**: **PASS-WITH-RISK**

> 一句话：**10 条问题全部真的处理了，7 条新增测试无一是假绿**。
> 我独立做了 **5 处针对修复轮声称的变异**（Q1/Q3/Q4/Q7/Q9），**全部按声称变红**；
> 另做了 **2 处「生产那一行」的变异**（Q4 调用点 / Q6 生产适配器），**都不红** ——
> 这两处暴露的是**覆盖边界**而非断言失效（见「变异 5」与 §F1/§F2）。
> 判 PASS-WITH-RISK 而不是 PASS，是因为 Q4 与 Q6 各留了一处**生产调用点/生产适配器无测试**的结构性缺口：
> 变异生产那一行，相关测试**仍然全绿**（我实测的 target 见下文，未逐 crate 追跑）。这两处都**不是本轮新增的假绿**（修复轮没有声称覆盖它们），
> 但它们是 QA Q4/Q6 原始担忧中「若被改坏测试仍绿」的那一半。

---

## 10 条问题的处理核实

| # | 声称 | 实测（证据） | 判定 |
|---|---|---|---|
| Q1（阻塞） | 阳性对照改为真注入真实 `SsoAuditPayload.user_agent=CANARY`，共用 `scan_hits` | `sso_audit.rs:317-346`；`scan_hits` 在 `:360-363`，用例与对照**同一函数**；旧 `format!("{dump}\n{CANARY}").contains(...)` 已删（diff 中 -4 行）；**我把它改成恒 0 → 用例红**（见变异 2） | **已修** |
| Q2（阻塞） | 按 D128 改规格：AC-S-107 步骤 4 200 + `revokedSessions` 回执，实现不动 | AC 现文 `acceptance.md:648`「步骤 4 → `200 {"revokedSessions": n}`（rev 3.3 由 D128 裁定）」；测试 `sso_sessions.rs:360-369` 断言 `OK` + `revokedSessions >= 1`；实现 `http.rs:642` 返回真实计数（`service.rs:1557-1568` 从 store 读） | **已修**（规格已改，AC 与实现/测试一致） |
| Q3（阻塞） | 新增 `sso_me_permissions_real_db.rs`：真实 IAM + 真实 RBAC + `/api/rbac/me/permissions` | 新文件 648 行；`world()` 注入 `iam.set_permission_provider(rbac.clone())`（`:359-362`，与生产 `main.rs:353` 同构）；真实 `platform_rbac::http::router`（`:440-443`）；断言 `!is_empty()`（`:615`）+ 含 `rbac:role:read`（`:621`）+ 与登录响应相等（`:632`）；**我把生产 handler 改成返回 `[]` → 用例红**（变异 3） | **已修** |
| Q4（高） | P16 (A)+(B)：抽 `bind_succeeded` / `bind_result` + 3 条单测；生产改调 `bind_result` | `ldap.rs:140-142`（`bind_succeeded`）/ `:152-154`（`bind_result`）；生产 `:348` `Ok(bind_result(&result))`；3 条单测 `:416-500`；`LdapResult::success()` 语义我另核 `ldap3-0.11.*/src/result.rs:279-287`（`rc==0` 才 `Ok`），与旧 `.success().is_ok()` 逐值等价；**`bind_succeeded` 改恒 true → 3 条全红**（变异 1）。**但调用点无覆盖**（变异 5，见 §F1） | **已修（P16 字面达成）**，残留缺口见 §F1 |
| Q5（高） | 两处 `ssoProviders` 断言 `["oidc","ldap"]` | `sso_config_secret.rs:343-355`（`provider_kinds`）+ `:412-415`；`sso_real_db.rs:366-376`，并补齐 LDAP 5 键前置（`:332-339`）；实现 `config.rs:226-238` 顺序固定（oidc→ldap），顺序断言不 flaky | **已修** |
| Q6（中） | 新增双租户用例（真实 `ConfigService`） | `sso_real_db.rs:384-516`：a 写 enforce → 403 + Argon2 差 0；b 不写 → 200 + Argon2 > 0；再打 a 仍 403。**我让测试内适配器丢租户 → 用例红**（变异 6）。**但生产适配器 `main.rs:606-636` 是另一份拷贝，未被该用例覆盖**（变异 5，见 §F2） | **部分修**（断言有效；生产适配器无覆盖） |
| Q7（中） | 单测 `!is_flag("sso.enforce")` + 两条写路由集成用例 | `registry.rs:583-621`（`#[cfg(test)] mod tests` 内，`cfg(test)` 在 `:485`）；`config_real_db.rs:1713-1801` 真跑 `write_tenant`（并断言落库值与 `source == Tenant`）与 `write_flag`（400 `FLAG_NAME_INVALID`）；**删回 `is_valid_flag_name` 约束 → 两处都红**（变异 4），报错正是声称的 `FlagEndpointRequired`/`CONFIG_FLAG_USE_FLAG_ENDPOINT` | **已修** |
| Q8（中） | 按 D128 改写步骤 6（`henry` + mail 撞 `gina`），并断言副作用为零 | AC 现文 `acceptance.md:550-553` 与测试 `sso_ldap.rs:336-395` **逐字一致**（`henry`、`mail=gina@example.com`、409）；新增读库断言 `external_id` 仍是 `uid=gina,...`（`:379-394`） | **已修**（AC 与测试不再并存两种说法） |
| Q9（低） | mock `/jwks` 加真实计数，断言 `>= hits_before + 1` | `sso_support/mod.rs:696-704` 真 `+= 1`；`:528-531` `jwks_hits()`；用例 `sso_oidc_security.rs:262-289`；**我把计数改成不递增 → 用例红**（变异 7） | **已修** |
| Q10（低） | 三处 `is_array()` 转强 | `sso_ldap.rs:96-107`、`sso_oidc.rs:249-259` 改为「字符串数组」；`sso_config_secret.rs` 由 Q5 覆盖；`sso_oidc.rs` 注释明确「非空属预期 5，由 Q3 负责」——**没有越位断言，也没有放宽** | **已修** |

**结论：10/10 都处理了。**其中 Q6 我判「部分修」是就**覆盖边界**而言（用例对 `ConfigService` 的租户行为是真的，但对生产装配层的拷贝无覆盖），不是断言失效。

---

## 我独立复现的变异验证

**统一前置**：每个变异前记录 `sha256` + `git status`；变异后 `git checkout -- <file>` 还原 → **`touch`**（出口标准 9.4）→ 重新 `sha256sum` 与基线**逐字节比对** → `git status` 核对。

命令模板（逐 crate、单 `--`、串行）：

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
unset RUST_LOG
cd /home/myg133/Projects/system_seed/feature-REQ-013
cargo test -p <crate> ... -- --test-threads=1 --include-ignored
```

### 变异 1: Q4 —— `bind_succeeded` 改成恒 `true`（= 任意密码都能登录）

- 文件：`crates/platform-iam/src/sso/ldap.rs`
- **sha256 前**：`8de2659e4c74b7589e2d0779ed5b2c29acd56d9a7a4ff105d27dfb4ddc54081b`
- **sha256 后（还原）**：`8de2659e4c74b7589e2d0779ed5b2c29acd56d9a7a4ff105d27dfb4ddc54081b`（**相同**）

```diff
 pub fn bind_succeeded(rc: u32) -> bool {
-    rc == 0
+    let _ = rc;
+    true
 }
```

- **预期**：红。**实测：3 条全红**（`cargo test -p platform-iam --lib`）：

```
test result: FAILED. 73 passed; 3 failed; 0 ignored; ...
sso::ldap::tests::a_real_ldap_result_with_rc_49_is_a_failed_bind
  panicked at ldap.rs:485: rc=49（invalidCredentials）必须判定为认证失败 —— 否则任意密码都能登录
sso::ldap::tests::bind_succeeded_agrees_with_ldap3_success_semantics
  left: true  right: false  (rc=1：bind_succeeded 与 ldap3::LdapResult::success() 不一致)
sso::ldap::tests::bind_succeeded_only_accepts_rc_zero
  panicked at ldap.rs:426: rc=49（invalidCredentials）必须是失败
```

- 还原后：sha256 相同、已 `touch`（mtime 更新）、`git status` 空；**还原后该 target 复跑 76 passed / 0 failed**。

### 变异 2: Q1 —— 把共用判据 `scan_hits` 改成恒 `0`

- 文件：`crates/platform-iam/tests/sso_audit.rs`
- **sha256 前/后**：`eead6bd7b055e00a6f3544194c4dece1788dd4d29be60423bcad788c6beab084`（**相同**）

```diff
 fn scan_hits(haystack: &str, needle: &str) -> usize {
-    haystack.matches(needle).count()
+    let _ = (haystack, needle);
+    0
 }
```

- **预期**：阳性对照红（而真实 payload 的「0 命中」会**假绿**，正是要证明它不同）。**实测**：

```
test ac_s_108_no_payload_carries_a_secret ... FAILED
panicked at sso_audit.rs:335: assertion `left == right` failed:
  扫描判据必须能命中注入到 user_agent 的 canary（否则上面的 0 命中不可信）
  left: 0  right: 1
```

- 还原后：sha256 相同、已 `touch`、`git status` 空；复跑 `sso_audit` 8 passed。

### 变异 3: Q3 —— 生产 `/api/rbac/me/permissions` 返回空权限集

（这是「SSO 会话拿到空权限集」的退化形态，改**生产**而非测试，用来证明用例真的打到 RBAC）

- 文件：`crates/platform-rbac/src/http.rs`
- **sha256 前/后**：`4bb65eecf63f83db647680cdb9330967e1d405ecdb8ecff525d971eae639fa71`（**相同**）

```diff
         Ok(permissions) => {
+            let _ = permissions;
             let body = MyPermissionsResponse {
-                permissions: permissions.into_iter().collect(),
+                permissions: Vec::new(),
             };
```

- **预期**：红。**实测**：

```
test ac_s_100_step_5_an_sso_session_reaches_its_real_permission_points ... FAILED
panicked at sso_me_permissions_real_db.rs:615:
  SSO 会话必须拿到真实权限点（空集正是本用例要防的假绿）：{"permissions":[]}
```

- 还原后：sha256 相同、已 `touch`、`git status` 空；复跑 1 passed。

### 变异 4: Q7 —— 删回 `boolean ⇒ flag` 的 `is_valid_flag_name` 约束

- 文件：`crates/platform-config/src/registry.rs`
- **sha256 前/后**：`f72b06116f0a76e7f4d5a83f50790291b952198e8033ad0553d48663d4d80a7a`（**相同**）

```diff
             .filter(|registered| {
                 registered.kind == ConfigType::Boolean
-                    && crate::flag::is_valid_flag_name(&registered.key)
             })
```

- **预期**：单测 + 集成用例都红。**实测（两处都红）**：

```
cargo test -p platform-config --lib
  test result: FAILED. 38 passed; 1 failed
  registry::tests::a_dotted_boolean_key_is_plain_config_and_not_a_flag
    panicked at registry.rs:599: sso.enforce 带点号，不是 flag

cargo test -p platform-config --test config_real_db -- a_dotted_boolean_key_is_writable
  panicked at config_real_db.rs:1723:
    sso.enforce 必须能经通用写路由写入（D127 约束前的症状是 CONFIG_FLAG_USE_FLAG_ENDPOINT）：
    FlagEndpointRequired { key: "sso.enforce" }
```

- 还原后：sha256 相同、已 `touch`、`git status` 空；复跑 39 + 26 passed。

### 变异 5: **两条「生产那一行」的变异 —— 都不红（这是本次最重要的发现）**

#### 5a Q4 生产调用点：`Ok(bind_result(&result))` → `Ok(true)`

- 文件：`crates/platform-iam/src/sso/ldap.rs`（同一文件，基线 sha 同上）
- 还原后 sha256：`8de2659e4c74b7589e2d0779ed5b2c29acd56d9a7a4ff105d27dfb4ddc54081b`（**相同**）

```diff
-            Ok(bind_result(&result))
+            let _ = &result;
+            Ok(true)
```

- **预期红，实测全绿**：

```
cargo test -p platform-iam --lib            → 76 passed; 0 failed
cargo test -p platform-iam --test sso_ldap  → 11 passed; 0 failed
```

→ **含义**：`rc → bool` 的**函数体**现在被机械验证了（变异 1 红），但**生产是否调用它**仍无测试。
即 QA 原话「若将来有人把 `.success()` 删掉，753 条测试仍然全绿」的**同型回归依然成立**（本 crate 的 `--lib` 76 条 + `sso_ldap` 11 条我实测全绿；其余 crate 未逐条追跑，因为它们不驱动 `Ldap3Conn`）。

#### 5b Q6 生产适配器 `ConfigServiceSsoSource`：读配置时丢弃租户

- 文件：`crates/platform-server/src/main.rs`
- **sha256 前/后**：`091ee846c5c878d10bb5e45eba72548c2583eca16d9dd4521b8213c422a74818`（**相同**）

```diff
         let config = self.config.clone();
-        let ctx = ctx.clone();
+        let _ = ctx;
+        let ctx = platform_core::RequestContext::new();
```

- **预期红，实测绿**：

```
cargo test -p platform-server --test sso_real_db -- ac_s_021_enforcement_is_tenant_scoped → ok (1 passed)
```

- 原因已定位：`sso_real_db.rs:51-73` 的用例用的是**测试内复制的** `RealConfigSource`（注释自陈「与 `main.rs` 的 `ConfigServiceSsoSource` 等价的实现」），
  而 `main.rs:606-636` 的生产副本**没有**任何用例直接驱动（那是 bin crate，集成测试够不到）。
- **对照实验（证明用例本身有区分力）**：把**测试内** `RealConfigSource` 改成同样丢租户 →

```
test ac_s_021_enforcement_is_tenant_scoped_through_the_real_config_service ... FAILED
  panicked at sso_real_db.rs:489: left: 200  right: 403   （tenant-a 本该 403，读到了默认 enforce=false）
```

→ 用例对「租户盲读」**是敏感的**，只是被测对象是拷贝而不是生产装配层。还原后 sha256 相同、已 `touch`、`git status` 空。

### 变异 6: Q9 —— 让 mock `/jwks` 计数器不递增（检验「计数器真的在计数」）

- 文件：`crates/platform-iam/tests/sso_support/mod.rs`
- **sha256 前/后**：`4adc75a94e458ac81ec23387752216433ff03d513753fe3db35367c87276165d`（**相同**）

```diff
     {
         let hits = state.jwks_hits.clone();
-        *hits.lock().expect("锁") += 1;
+        let _ = hits;
     }
```

- **预期**：红。**实测**：

```
test ac_s_102_an_unknown_kid_triggers_a_jwks_refresh_and_succeeds ... FAILED
panicked at sso_oidc_security.rs:266:
  首次登录必须先取一次 JWKS（否则「再取一次」无法区分）：0
```

- 还原后：sha256 相同、已 `touch`、`git status` 空；复跑 `sso_oidc_security` 12 passed。

### 变异小结

| # | 变异对象 | 声称 | 我实测 | 结论 |
|---|---|---|---|---|
| 1 | `bind_succeeded` 函数体恒 true | 红 | **3 条红** | 属实 |
| 2 | `scan_hits` 恒 0 | 红 | **1 条红（阳性对照）** | 属实 |
| 3 | 生产 `/me/permissions` 空集 | 红 | **1 条红** | 属实 |
| 4 | 删 `is_valid_flag_name` 约束 | 两处红 | **单测 + 集成都红** | 属实 |
| 5a | **生产调用点 `Ok(true)`** | （未声称） | **全绿** | 残留缺口 §F1 |
| 5b | **生产适配器丢租户** | （未声称） | **绿** | 残留缺口 §F2 |
| 6 | `/jwks` 计数器不递增 | 红 | **1 条红** | 属实 |

**五处修复轮声称的变异（Q1/Q3/Q4/Q7/Q9）我独立复现，全部按声称变红。**修复轮「测试会红」的声称是**真的**。

---

## 回归数字（我实测）

逐 crate、串行、单 `--`、`--all-targets`、`--include-ignored`：

| crate | 修复前（QA 基线） | 修复轮自报 | **我实测** | failed | ignored |
|---|---|---|---|---|---|
| platform-core | 127 | 127 | **127** | 0 | 0 |
| platform-config | 83 | 85 | **85** | 0 | 0 |
| platform-db | 46 | 46 | **46** | 0 | 0 |
| platform-iam | 165 | 168 | **168** | 0 | 0 |
| platform-rbac | 78 | 78 | **78** | 0 | 0 |
| platform-org | 91 | 91 | **91** | 0 | 0 |
| platform-schema | 20 | 20 | **20** | 0 | 0 |
| platform-server | 110 | 112 | **112** | 0 | 0 |
| sandbox | 33 | 33 | **33** | 0 | 0 |
| **合计** | 753 | **760** | **760** | **0** | **0** |

- **+7 的来源逐项核实**：iam +3 = `ldap.rs` 三条 P16 单测；config +2 = `registry.rs` 单测 + `config_real_db.rs` 集成；server +2 = 新文件 1 条 + `sso_real_db.rs` 双租户 1 条。**数字完全对得上，无虚报。**
- 变异全部还原后**又完整跑了一遍 9 个 crate**：仍 **760 passed / 0 failed / 0 ignored**（证明没有陈旧二进制残留）。
- `#[ignore]` 属性数：`3d02c21` = **66**，`HEAD` = **66**（**没有新增任何 ignore**；且全仓 `#[ignore]` 无一裸写，0 条缺原因）。
- **未验**：前端 `apps/shell` vitest 本轮**未复跑**（`git diff 3d02c21..HEAD` 不含 `apps/`，前端零改动；前一轮 QA 的 158 条结论沿用）。

---

## 新增假绿排查（逐项）

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 新断言是否恒真（`assert!(true)` / `x.contains(x)` / `>= 0`） | ✅ **无** | 全仓 `assert!(true)`/`assert_eq!(1,1)`/`>= 0` 0 命中；新增 62 条断言逐条看过，均为具体值或集合比较 |
| 2 | Q1 阳性对照是否**同一判据** | ✅ 是 | 用例与对照都走 `scan_hits`（`sso_audit.rs:360`）；对照的 haystack 是 `serde_json::to_value(SsoAuditEvent)`，与 `RecordingAudit::payloads()`（`sso_support/mod.rs:1113-1127`）**同一序列化形态**；变异 2 已证会红 |
| 3 | Q3 是否**真接 RBAC**（而非硬编码响应） | ✅ 是 | 真实 `RbacService`（`sso_me_permissions_real_db.rs:352-362`）+ 真实路由（`:440-443`）+ 真实授权（`:404-433`）；变异 3 把生产 handler 打空即红 |
| 4 | Q4 `bind_result` 是否**真被生产调用** | ⚠️ **调用了，但调用点无测试** | 生产 `ldap.rs:348` 确实调用；但变异 5a 证明该行被改成 `Ok(true)` 后 `platform-iam` 的 lib/sso_ldap target 全绿 → §F1 |
| 5 | Q7 是否**真跑写路由**（而非只测 `is_flag()`） | ✅ 是 | 集成用例经 `service.write_tenant`/`write_flag` 并读库断言 `(TENANT_A, key)` 行值 + `source == Tenant`（`config_real_db.rs:1718-1744`）；变异 4 在「Scoped 写应成功」处红 |
| 6 | Q9 计数器是否**真在计数** | ✅ 是 | `sso_support/mod.rs:696-703` 真 `+= 1`；变异 6 打掉递增即红 |
| 7 | 新增 `#[ignore]` / `test.skip` 掩盖 | ✅ 无 | ignore 66→66；`test.skip` 仍只有 `e2e/sso.spec.ts:245`（仅 404/405 未就绪）与 `e2e/schema-driven.spec.ts:137`（既有，非本轮） |
| 8 | 是否放宽既有断言 | ✅ 无 | `git diff` 中测试断言**只增不减**；唯一被删的是 Q1 那条恒真假对照与 Q2 的自陈注释 |
| 9 | 是否测试 mock 而非实现 | ⚠️ 部分 | OIDC/LDAP/RBAC/配置主路径均打到真实实现；两个边界：Q4 调用点（§F1）、Q6 生产适配器（§F2） |
| 10 | 依赖预算是否被偷用 | ✅ 合规 | `platform-server/Cargo.toml` 只加 `jsonwebtoken`/`base64`/`sha2`/`time` 4 个 **workspace** dev-dep；`Cargo.lock` 仅该 crate 依赖列表 +4 行，**无新增外部包** |

---

## 生产逻辑改动核实

`git diff 3d02c21..HEAD -- crates/*/src` 的实际内容（**只有两个文件**）：

**1. `crates/platform-iam/src/sso/ldap.rs`（+120/-4）—— 接缝抽取，语义等价**

```diff
-            Ok(result.success().is_ok())
+            Ok(bind_result(&result))
```
外加 `bind_succeeded(rc: u32)` / `bind_result(&LdapResult)` 两个 `pub fn` 与 3 条单测。
**语义等价性我独立核对**：`ldap3-0.11.*/src/result.rs:279-287` 的 `success()` 仅在 `rc == 0` 返回 `Ok`，与 `rc == 0` 逐值相同。
→ **不是功能改动**，与修复轮「生产逻辑唯一改动是 Q4 接缝抽取」的自陈**一致**。

**2. `crates/platform-config/src/registry.rs`（+49）—— 全在 `#[cfg(test)]` 之后**

`#[cfg(test)] mod tests` 起于 `:485`；新增测试函数起于 `:574`。
→ **本例文区域的 `is_valid_flag_name` 约束（`:328-333`）在本轮之前就存在**，本轮**未动**实现。
修复轮「Q7 只加测试」的声称**属实**。

**3. 没有其它生产改动。** `platform-server/src/main.rs`、`platform-rbac/src/http.rs` 在 `3d02c21..HEAD` 区间**零改动**（我的变异就是打在这两处、并已逐字节还原）。

**4. 没有借修复轮偷改实现**：`git diff --stat` 的 15 个文件里，除上述两个 src 文件与 1 个 `Cargo.toml`/`Cargo.lock` 外，其余全是 `tests/` 与 `feature-meta/*.md`。

---

## 逐条问题

### §F1（中）Q4 的「唯一防线」只被验证到函数体，**生产调用点仍无测试**

- **触发条件**：任何人把 `crates/platform-iam/src/sso/ldap.rs:348` 的 `Ok(bind_result(&result))` 改成 `Ok(true)`（或退回「外层 `Ok` 即成功」）。
- **观测影响**：**任意密码都能登录**，而 `platform-iam` 的 `--lib`（76 条）与 `--test sso_ldap`（11 条）**全绿**（我实测）。该行不涉及其它 crate 的行为，故未再逐 crate 追跑。
- **证据**：变异 5a；`bind_result` 的实际调用点只有 `ldap.rs:348`，无任何测试驱动 `Ldap3Conn::simple_bind`（`grep -rn "Ldap3" crates/platform-iam/tests/` → 0 命中）。
- **性质**：**不是新引入的假绿**，是「无服务端 LDAP 就无法驱动」的客观边界；但修复轮注释里「包装层现在只写 `Ok(bind_result(&result))`，所以这里断言的行为就是生产的判定行为」这句**容易被读成「生产已被覆盖」**，建议在注释与本报告层面都点明「调用点仍靠人眼」。
- **建议（不阻塞合入）**：加一条静态断言（读 `src/sso/ldap.rs` 源码断言存在 `Ok(bind_result(&result))`），把这一行也纳入机械验证；或把 `Ldap3Conn` 的构造抽成可注入 `simple_bind` 的 trait，用替身驱动包装层。

### §F2（中）Q6 验证的是**测试内复制的**配置适配器，生产 `ConfigServiceSsoSource` 无覆盖

- **触发条件**：`crates/platform-server/src/main.rs:610-636` 的 `read()` 丢失 `ctx`（例如被改成 `RequestContext::new()`）。
- **观测影响**：强制 SSO 变成「读平台默认值」，**所有租户的 enforce 一起失效**；测试仍全绿（我实测 `ac_s_021_enforcement_is_tenant_scoped...` ok）。
- **证据**：变异 5b（main.rs 变红→实测绿）；对照实验：改测试内 `RealConfigSource`（`sso_real_db.rs:51-73`）同样的变异 → 用例红（`left: 200 right: 403`）。该测试适配器注释自陈「与 `main.rs` 的实现等价」，但它是**拷贝**，不随生产改动一起红。
- **性质**：Q6 的**断言有效**（对租户盲读敏感），缺口在「被测对象是拷贝」。与 §F1 同源：**生产副本无测试**。

### §F3（低）Q1 注释里的「第 3 步」不存在（注释与代码编号不一致）

- **位置**：`crates/platform-iam/tests/sso_audit.rs:317-324`
- **问题**：函数内只标了 `// 1)`（真实 payload 0 命中）与 `// 2)`（阳性对照）两个编号步骤，注释却两次写「第 3 步会红」；实际指的是步骤 2 里的第 3 条断言（`:330-338`）。本项目明令「注释与代码不一致比没有注释更坏」。
- **影响**：纯可读性，不影响判定。建议改成「本条的第 2 个断言」。

### §F4（低）用例文档仍留 `204` 的旧说法（AC 已改，`test-cases/` 未同步）

- **位置**：`BA/demands/REQ-013-sso/test-cases/session-audit.md:94`
- **原文**：「步骤 1 `200` 或 `204`（BA 需钉死；**既有实现是 `204`**）」
- **实测**：AC 已被 D128 钉死为 `200 {"revokedSessions": n}`（`acceptance.md:648`），实现 `http.rs:642` 也确实是 200 —— 所以该句「既有实现是 204」是**事实错误**，且「需要钉死」已被 D128 钉死。
- **影响**：不影响代码与测试；但留下新的「AC 说 200 / 用例文档说 200 或 204」文档漂移。建议 BA 顺手同步该行。

### §F5（观察，非缺陷）修复轮自验报告的若干表述

修复轮自验报告（`feature-meta/req-013-fix1.md`）在**数字、改动清单、变异声明**上与我实测**完全一致**（760、+7 分布、6 个文件性质、5 处变异）。
唯一需要补注的是两处它**没有声称**但读者可能误读为已覆盖的地方：§F1（Q4 调用点）与 §F2（Q6 生产适配器）。
另：Q5 断言用了顺序敏感比较 `vec!["oidc","ldap"]`，我核了 `config.rs:226-238` 的顺序是代码里写死的（oidc 先、ldap 后），不构成 flake。

---

## 未验 / 边界说明

1. **前端 158 条 vitest 未复跑**：本轮 `git diff 3d02c21..HEAD` 不含 `apps/`，前端零改动，沿用上一轮 QA 结论。
2. **真实 LDAP 服务器路径未验**：本机无 LDAP 服务端，`Ldap3Conn::simple_bind` 的网络层只能靠 `bind_result` 纯函数 + `LdapResult` 构造驱动（这也是 §F1 存在的根因）。
3. **OIDC https（自签根）用例未单独重跑**：它在 `sso_oidc.rs` 内，随 `platform-iam --all-targets` 一并通过（168/168），但未逐条拆看。

---

## 最终状态声明

| 我改过的文件 | 基线 sha256 | 还原后 sha256 | 一致 | touch | 还原后复跑 |
|---|---|---|---|---|---|
| `crates/platform-iam/src/sso/ldap.rs` | `8de2659e…54081b` | `8de2659e…54081b` | ✅ | ✅ | 76 lib passed |
| `crates/platform-iam/tests/sso_audit.rs` | `eead6bd7…eab084` | `eead6bd7…eab084` | ✅ | ✅ | 8 passed |
| `crates/platform-iam/tests/sso_support/mod.rs` | `4adc75a9…76165d` | `4adc75a9…76165d` | ✅ | ✅ | 12 passed |
| `crates/platform-rbac/src/http.rs` | `4bb65eec…39fa71` | `4bb65eec…39fa71` | ✅ | ✅ | 1 passed |
| `crates/platform-server/src/main.rs` | `091ee846…a74818` | `091ee846…a74818` | ✅ | ✅ | （5b 对照组见 §F2） |
| `crates/platform-config/src/registry.rs` | `f72b0611…d80a7a` | `f72b0611…d80a7a` | ✅ | ✅ | 39 + 26 passed |
| `crates/platform-server/tests/sso_real_db.rs` | `d371f746…f37578` | `d371f746…f37578` | ✅ | ✅ | 4 passed |

- `git status --porcelain` → **空**（复核结束时与开始时一致）
- `git stash list` → **空**
- 最终全量回归（还原后）：**760 passed / 0 failed / 0 ignored**
- **未提交任何 git commit；未修改 BA 文档**（本报告是唯一的写入，且写在 `BA/feature-meta/`）。
