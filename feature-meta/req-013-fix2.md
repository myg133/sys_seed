# REQ-013 修复轮 2 自验报告（Dev）

- **分支**：`feature/REQ-013`，基线 HEAD `a8e5157`
- **依据**：`BA/feature-meta/recheck-req013.md` §F1 / §F2、`decisions.md` **D129**、`dev-guide-low-spec.md` §9.4
- **范围**：只做两件事（任务 1 = LDAP 判定点下沉；任务 2 = Q6 用生产适配器）。**未提交 git**，未改 BA 文档，未碰 `apps/` / `packages/` / `e2e/`。
- **环境**：`CARGO_TARGET_DIR=code/target`、`DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test`、逐 crate 串行、单 `--`、`--all-targets --include-ignored`

---

## 汇总

| 项 | 结果 |
|---|---|
| 全量回归 | **760 passed / 0 failed / 0 ignored**（基线 760，无增减） |
| 变异 A（service 层判定改坏） | **4 条集成用例红**（lib 单测仍 76 绿 —— 正是 D129 指出的盲区） |
| 变异 B（生产适配器丢租户） | **3 条集成用例红**（含租户用例 `left: 200 / right: 403`） |
| 两处变异还原 | sha256 逐字节相同 + `touch` + `git status` 只有本任务的 6 个文件 |

---

## 任务 1：LDAP bind 的决策点从 async 包装层移到 service 层

### 改了什么

文件：`crates/platform-iam/src/sso/ldap.rs`、`crates/platform-iam/tests/sso_support/mod.rs`

**1) trait 不再做判定**（`ldap.rs:110-125`）

```diff
-    /// 返回 `Ok(true)` = **`rc == 0`**（认证成功）；`Ok(false)` = `rc != 0`（凭据错）。
-    /// 实现**必须**显式检查结果码 —— 见模块文档的 S18。
-    fn simple_bind(&mut self, dn: &str, pw: &str)
-    -> SsoBoxFuture<'_, Result<bool, SsoLdapError>>;
+    /// **本方法不做「成功 / 失败」的判定**（D129）—— 外层 `Ok` 只代表
+    /// 「请求发出并收到响应」，`rc != 0`（如 `49`）仍在 `LdapResult` 里（S18）。
+    fn simple_bind(
+        &mut self,
+        dn: &str,
+        pw: &str,
+    ) -> SsoBoxFuture<'_, Result<ldap3::LdapResult, SsoLdapError>>;
```

**2) 生产包装层原样透传**（`ldap.rs:348-364`）

```diff
-            let result = self
-                .ldap
-                .simple_bind(&dn, &pw)
-                .await
-                .map_err(|_| SsoLdapError::Bind)?;
-            Ok(bind_result(&result))
+            // 本层**不做判定**，只把协议结果原样透传（D129）
+            self.ldap
+                .simple_bind(&dn, &pw)
+                .await
+                .map_err(|_| SsoLdapError::Bind)
```

**3) 决策上移到 service 层两处调用点**（`ldap.rs:237` / `ldap.rs:264`）

```diff
-    if !conn.simple_bind(service_dn, service_password).await? {
+    if !bind_result(&conn.simple_bind(service_dn, service_password).await?) {
         return Err(SsoLdapError::Bind);
     }
...
-    if !conn.simple_bind(&entry.dn, password).await? {
+    if !bind_result(&conn.simple_bind(&entry.dn, password).await?) {
         return Err(SsoLdapError::Bind);
     }
```

**4) 测试替身返回真实 `LdapResult`**（`tests/sso_support/mod.rs:1064-1093`）

```diff
-    ) -> SsoBoxFuture<'_, Result<bool, SsoLdapError>> {
+    ) -> SsoBoxFuture<'_, Result<ldap3::LdapResult, SsoLdapError>> {
         ...
         if dn.is_empty() {
-            return Ok(guard.service_bind_ok);
+            return Ok(ldap_result(if guard.service_bind_ok { 0 } else { 49 }));
         }
         let matched = ...;
-        Ok(matched)
+        // rc=0 = success；rc=49 = invalidCredentials。
+        // 这里**不返回 bool**：判定在生产 service 层（D129）。
+        Ok(ldap_result(if matched { 0 } else { 49 }))
     }
 }

+fn ldap_result(rc: u32) -> ldap3::LdapResult {
+    ldap3::LdapResult { rc, matched: String::new(), text: String::new(), refs: Vec::new(), ctrls: Vec::new() }
+}
```

同步改了模块文档、`bind_succeeded` / `bind_result` / 单测的注释里「包装层写 `Ok(bind_result(&result))`」这类**已经不成立**的说法（避免注释与代码不一致）。

**行为等价性**：`rc → bool` 仍是 `bind_succeeded(rc) == (rc == 0)`，只是判定位置从包装层搬到 service 层，**没有任何语义变化**；`bind_succeeded` / `bind_result` 及 3 条单测原样保留。

### 变异 A：把 service 层判定改成「外层 `Ok` 即成功」

- 文件：`crates/platform-iam/src/sso/ldap.rs`
- **sha256 前**：`fe3fa85426fb2fd01da59c644bbb0dc3d69751c9bdd9d1aca71534e8c6a06841`
- **sha256 后（还原）**：`fe3fa85426fb2fd01da59c644bbb0dc3d69751c9bdd9d1aca71534e8c6a06841`（**相同**）

```diff
-    if !bind_result(&conn.simple_bind(&entry.dn, password).await?) {
+    // MUTATION A: 外层 Ok 即成功（= 任意密码都能登录）
+    if conn.simple_bind(&entry.dn, password).await.is_err() {
         return Err(SsoLdapError::Bind);
     }
```

**实测（先单测、再集成）**：

```
cargo test -p platform-iam --lib            → test result: ok. 76 passed; 0 failed
cargo test -p platform-iam --test sso_ldap  → test result: FAILED. 7 passed; 4 failed
```

红的 4 条（**全是集成用例**）：

| 用例 | panic |
|---|---|
| `ac_s_104_a_wrong_password_and_an_unknown_user_are_indistinguishable` | `sso_ldap.rs:124: left: 200  right: 401` |
| `ac_s_020_ldap_unreachable_is_503_while_bad_credentials_are_401` | `sso_ldap.rs:243: left: 200  right: 401` |
| `ac_s_104_the_ip_rate_limit_applies_to_ldap_failures` | `sso_ldap.rs:183: left: 200  right: 401` |
| `ac_s_105_changing_the_directory_password_takes_effect_immediately` | `sso_ldap.rs:306: 旧密码必须立即失效 left: 200  right: 401` |

→ 这就是 D129 要求的闭合：**判据的函数体**由 `--lib` 守（76 绿说明单测确实抓不到这个变异），**「有没有调用判据」**由集成测试守。上一轮 `Ok(bind_result(&result)) → Ok(true)` 全绿的情形，**现在会红**。

还原后：`touch` + sha256 相同；`--test sso_ldap` 复跑 **11 passed**；最终全量回归中该 target 也是 11 passed。

---

## 任务 2：Q6 用生产适配器，删掉测试里的副本

### 改了什么

**1) 新建 lib 模块**：`crates/platform-server/src/sso_source.rs`（新文件，`pub struct ConfigServiceSsoSource` + `pub fn sso_config_from_entries`）

内容与原先 `main.rs` 里的定义**逐字一致**（含 `read_entries(&ctx, &SSO_CONFIG_KEYS)` 与 `ctx` 透传），只把 `struct`/字段改成 `pub fn new(config)` 构造：

```rust
pub struct ConfigServiceSsoSource { config: Arc<platform_config::ConfigService> }

impl ConfigServiceSsoSource { pub fn new(config: Arc<platform_config::ConfigService>) -> Self { ... } }

impl SsoConfigSource for ConfigServiceSsoSource {
    fn read(&self, ctx: &RequestContext) -> ... {
        let config = self.config.clone();
        // 租户上下文必须跟着走：这一行丢了 = 所有租户的 enforce 一起失效。
        let ctx = ctx.clone();
        Box::pin(async move { ... })
    }
}
```

**2) `platform-server/src/lib.rs`** 注册模块（`main.rs` 是 bin，测试够不到私有类型，必须先挪到 lib 侧）：

```diff
+pub mod sso_source;
 pub mod static_files;
```

**3) `main.rs` 删除本地副本、改用 lib 侧同一个类型**（-88 行）

```diff
-use platform_server::{db, health, isolation, migrate, policies, probe, static_files};
+use platform_server::sso_source::ConfigServiceSsoSource;
+use platform_server::{db, health, isolation, migrate, policies, probe, static_files};
...
-        let source = Arc::new(ConfigServiceSsoSource {
-            config: config_service.clone(),
-        });
+        let source = Arc::new(ConfigServiceSsoSource::new(config_service.clone()));
```

**4) `tests/sso_real_db.rs` 删除 `RealConfigSource` + `map_entries` 副本**（-84 行），改用生产类型：

```diff
-/// 与 `platform-server/src/main.rs` 的 `ConfigServiceSsoSource` 等价的实现。
-struct RealConfigSource { ... }
-impl SsoConfigSource for RealConfigSource { ... }
-fn map_entries(...) -> SsoConfig { ... }
+use platform_server::sso_source::ConfigServiceSsoSource;
...
-        service.set_sso_config_source(Arc::new(RealConfigSource {
-            config: config_service.clone(),
-        })),
+        // **生产装配层的同一个类型**（`platform-server/src/sso_source.rs`）——
+        // 不是测试里的等价拷贝。它丢了 `ctx` 时，下面 `ac_s_021` 的租户用例会红（D129）。
+        service.set_sso_config_source(Arc::new(ConfigServiceSsoSource::new(
+            config_service.clone()
+        ))),
```

现在「测试内 `map_entries` 与生产 `sso_config_from_entries` 各自漂移」这条风险也不存在了（只剩一份）。

### 变异 B：让生产适配器读配置时丢弃租户

- 文件：`crates/platform-server/src/sso_source.rs`
- **sha256 前**：`3951fff44b97b3b3b9a454bc03b0bf72840ddfa211c2f8ded308286c1deb7de1`
- **sha256 后（还原）**：`3951fff44b97b3b3b9a454bc03b0bf72840ddfa211c2f8ded308286c1deb7de1`（**相同**）

```diff
         let config = self.config.clone();
-        // 租户上下文必须跟着走：这一行丢了 = 所有租户的 enforce 一起失效。
-        let ctx = ctx.clone();
+        // MUTATION B: 读配置时丢弃租户上下文
+        let _ = ctx;
+        let ctx = platform_core::RequestContext::new();
```

**实测**：`cargo test -p platform-server --test sso_real_db` → `FAILED. 1 passed; 3 failed`

| 用例 | panic |
|---|---|
| `ac_s_021_enforcement_is_tenant_scoped_through_the_real_config_service` | `sso_real_db.rs:416: left: 200  right: 403`（tenant-a 本该被强制 SSO 拦住） |
| `ac_s_021_enforcement_works_through_the_real_config_service` | `sso_real_db.rs:287: left: 200  right: 403` |
| `ac_s_106_the_twelve_keys_come_back_from_the_real_config_service` | `sso_real_db.rs:192: OIDC + LDAP 都应可用 left: 0  right: 2` |

→ 复核者 §F2 的变异（打在生产 `main.rs` 上全绿）**现在打在生产适配器上会红**，因为用例驱动的是同一份代码。

还原后：`touch` + sha256 相同；最终全量回归中 `sso_real_db` **4 passed**。

---

## 逐 crate 回归（还原后，串行、单 `--`）

| crate | 基线 | 本轮 | failed | ignored |
|---|---|---|---|---|
| platform-core | 127 | **127** | 0 | 0 |
| platform-config | 85 | **85** | 0 | 0 |
| platform-db | 46 | **46** | 0 | 0 |
| platform-iam | 168 | **168** | 0 | 0 |
| platform-rbac | 78 | **78** | 0 | 0 |
| platform-org | 91 | **91** | 0 | 0 |
| platform-schema | 20 | **20** | 0 | 0 |
| platform-server | 112 | **112** | 0 | 0 |
| sandbox | 33 | **33** | 0 | 0 |
| **合计** | **760** | **760** | **0** | **0** |

- 本任务**没有新增/删除任何测试**（+0，因为没有加新用例也能闭合：改的是结构和测试替身），所以总数仍 760。
- `platform-iam` 明细：76(lib)+16+8+7+5+11+19+12+8+6 = 168。
- `platform-server` 明细：61(lib)+8(bin)+17+5+6+3+7+1+4 = 112。

---

## 最终状态声明

| 文件 | 性质 | 结束 sha256 |
|---|---|---|
| `crates/platform-iam/src/sso/ldap.rs` | 改（任务 1） | `fe3fa854…a06841` |
| `crates/platform-iam/tests/sso_support/mod.rs` | 改（任务 1 替身） | `650c2a77…ecaf6b` |
| `crates/platform-server/src/sso_source.rs` | **新增**（任务 2） | `3951fff4…eb7de1` |
| `crates/platform-server/src/lib.rs` | 改（注册模块） | — |
| `crates/platform-server/src/main.rs` | 改（删副本） | — |
| `crates/platform-server/tests/sso_real_db.rs` | 改（删副本） | `8cfb2851…9f6118` |

- `git status --porcelain`：仅上面 5 个 `M` + 1 个 `??`（`sso_source.rs`），**无其它残留**
- `git stash list`：空
- `grep -rn MUTATION`（两个被变异文件）：0 命中
- **未提交 git**；未改 BA 文档；未碰 `apps/` `packages/` `e2e/`

---

## 我认为还不足的地方

1. **测试替身的 `rc=49` 是写死的单一失败码**：`RecordingLdap` 只用 `0` / `49`。真实目录还会返回 `32`（noSuchObject）、`53`（unwillingToPerform）等 —— 判据本身由 `bind_succeeded` 的单测覆盖（0/1/32/48/49/50/51/53 逐值对齐 `ldap3::success()`），但**端到端只验了 49**。若要在集成层区分「凭据错」与「服务端拒绝」，需要在替身上加可配置的 rc；本轮不改，因为任务只要求闭合判定点。
2. **`Ldap3Conn::simple_bind` 的网络层仍无测试**（客观边界：`ldap3` 没有服务端/内存实现）。本轮闭合的是「判定有没有被调用」，不是「`ldap3` 的 `rc` 读得对不对」——后者仍只有 `bind_result` 的构造用例把关。
3. **`SsoLdapConn` 的公开签名现在暴露 `ldap3::LdapResult`**：好处是替身能构造真实结果；代价是 `platform-iam` 的公共 API 与 `ldap3` 的版本绑定更紧（未来升级 `ldap3` 会波及 trait 的消费方）。当前全仓只有 `sso_support` 一个消费方，可接受。
4. **未跑 clippy / fmt 门禁**：本轮只跑了 `cargo test --all-targets`。若要合入前跑 `cargo clippy -- -D warnings`，需要单独一轮（我未跑，不宣称通过）。
5. **`platform-server/tests/sso_real_db.rs` 里删掉了原有的「装配：真实 `ConfigService` 作为 SSO 配置源」小节注释**，现在文件的这一层说明回到顶部模块文档；可读性略降，但去掉了指向「等价实现」的误导性文字。
