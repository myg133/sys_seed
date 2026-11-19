# REQ-021 修复轮 3 — 复核 F-1 护栏缺口（只补测试，不改实现）

> **本轮的交付物只有一条**：把「DB 里的平台行 → `/api/i18n/context` 返回 `source=platform`」
> 这条路径用**端到端**的方式钉住。**实现代码一行没动**（变异验证期间临时改过 `src/i18n.rs`，
> 已按 sha256 还原，见 §4）。
>
> **分支**：`feature/REQ-021`（**未提交**，未 push，未 checkout/stash/reset/restore）
> **工作区**：`/home/myg133/Projects/system_seed/feature-REQ-021`

---

## 1. 复核的缺口是什么（为什么已有测试不够）

独立复核的 **M-B 变异**（把 `ConfigSource::Platform` 误报成 `Tenant`）暴露出一条**没有护栏**的路径：

| 变异 | 单元 | HTTP（`i18n_api.rs`） | 真库（`i18n_settings_real_db.rs`） | **缺口** |
|---|---|---|---|---|
| M-A（`Platform => None`，= 修复前行为） | 3 红 | 2 红 | 1 红 | 有牙齿 |
| **M-B（`Platform => Some(Tenant)`）** | 3 红 | 2 红 | **0 红** | ★ **真库路径完全没护栏** |

本轮**实测复现**了 M-B 的这条结论（§4.2）：`i18n_settings_real_db.rs` 在 M-B 下
**3 passed / 0 failed** —— 它只断言 `setting.is_explicit()` / `setting.value`，
**只测「配置层」，从不调用协商函数**；而 `i18n_api.rs` 用的是 `FakeSettings`，
伪造的 `Setting.source` 是**手写的**，不是从 DB 解析出来的。

⇒ 「DB 里的平台行 → 真实协商 → 真实 HTTP 响应体报 `source=platform`」这条路径**无护栏**。

---

## 2. 新增文件

**`crates/platform-server/tests/i18n_context_real_db.rs`**（新建，26192 → 22891 字节量级；
与其它改动零冲突，见 §7「为什么单独建文件」）

### 2.1 五个用例

| # | 测试名 | 场景 | 断言的 `source` |
|---|---|---|---|
| 1 | `the_platform_layer_reaches_the_http_context_as_source_platform` | **只有平台层配了**（租户没配） | **`platform`** ★ 核心护栏 |
| 2 | `the_tenant_layer_wins_over_the_platform_layer_end_to_end` | 平台层 + 租户层都配 | `tenant` |
| 3 | `nothing_configured_falls_back_to_the_manifest_default_end_to_end` | 谁都没配 | `default` |
| 4 | `the_accept_language_header_still_wins_over_the_platform_layer_end_to_end` | 平台层配了 + `accept-language: zh-CN` | `accept-language` |
| 5 | `the_user_preference_still_wins_over_the_platform_layer_end_to_end` | 平台层配了 + `?locale=zh-CN&timezone=Europe/London` | `user` |

要求里说「至少覆盖前两条 + 平台层那条，第三、四条若 HTTP 层已有等价覆盖可省」——
**本轮五条全做**，不省。

### 2.2 断言原文

用例 1（核心护栏，`crates/platform-server/tests/i18n_context_real_db.rs:401-441`）：

```rust
let (status, body) = call(fx.state.clone(), "", &[]).await;
// 先清夹具，再断言：断言失败也不会留下残留（平台层的行对所有租户可见）。
clear_i18n(&fx.admin).await;
assert_eq!(status, StatusCode::OK, "body = {body}");

// 语言：平台层的 `en-US` 生效，来源如实报 `platform`。
assert_eq!(body["locale"], "en-US", "平台级的语言必须生效");
assert_eq!(
    body["source"], "platform",
    "来源必须如实报 platform（报成 tenant / default 都是 D119 回归）"
);
assert_eq!(body["sources"]["locale"], "platform");
assert_ne!(body["locale"], "zh-CN", "不能退回 manifest 默认值");

// 时区：平台层的 `America/New_York` 生效。
assert_eq!(body["timezone"], "America/New_York", "平台级的时区必须生效");
assert_eq!(body["sources"]["timezone"], "platform");
assert_ne!(body["timezone"], "Asia/Shanghai", "不能退回 manifest 默认值");

// 币种：平台层的 `USD` 生效。
assert_eq!(body["currency"], "USD", "平台级的币种必须生效");
assert_eq!(body["sources"]["currency"], "platform");
assert_ne!(body["currency"], "CNY", "不能退回 manifest 默认值");
```

用例 2（反向断言，`…:444-486`）：

```rust
assert_eq!(body["locale"], "en-US", "租户层的语言必须压过平台层");
assert_eq!(body["source"], "tenant", "如实报出是租户层生效");
assert_eq!(body["sources"]["locale"], "tenant");
assert_ne!(body["locale"], "zh-CN", "不能取平台层的值");

assert_eq!(body["timezone"], "Europe/London", "租户层的时区必须压过平台层");
assert_eq!(body["sources"]["timezone"], "tenant");
assert_ne!(body["timezone"], "America/New_York", "不能取平台层的值");

assert_eq!(body["currency"], "JPY", "租户层的币种必须压过平台层");
assert_eq!(body["sources"]["currency"], "tenant");
assert_ne!(body["currency"], "USD", "不能取平台层的值");
```

用例 3（反向断言，`…:489-518`）：

```rust
assert_eq!(body["source"], "default", "谁都没配时来源必须是 default");
assert_eq!(body["sources"]["locale"], "default");
assert_eq!(body["sources"]["timezone"], "default");
assert_eq!(body["sources"]["currency"], "default");

// 平台默认值来自 manifest（裁定 #17），不是代码常量。
for (field, key) in [("locale", "locale"), ("timezone", "timezone"), ("currency", "currency")] {
    assert_eq!(body[field], fx.manifest_default(key), "{field} 的平台默认值必须来自 manifest");
}
```

用例 4（反向断言，`…:520-548`）：

```rust
assert_eq!(body["locale"], "zh-CN", "请求头仍压过平台层");
assert_eq!(body["source"], "accept-language");
assert_eq!(body["sources"]["locale"], "accept-language");

// 时区不看请求头 → 仍是平台层。
assert_eq!(body["sources"]["timezone"], "platform");
assert_eq!(body["sources"]["currency"], "platform");
```

用例 5（反向断言，`…:551-586`）：

```rust
assert_eq!(body["locale"], "zh-CN", "用户偏好必须压过平台层");
assert_eq!(body["source"], "user");
assert_eq!(body["sources"]["locale"], "user");
assert_eq!(body["timezone"], "Europe/London");
assert_eq!(body["sources"]["timezone"], "user");

// 币种本轮没有用户级覆盖 → 仍是平台层。
assert_eq!(body["sources"]["currency"], "platform");
```

---

## 3. 它**怎么**走真实配置解析器（为什么不是 FakeSettings）

### 3.1 链路

```text
真实 DB（base_test，管理员建夹具 / 应用角色 base_app 跑业务读）
  → platform_config::ConfigService          （真实）
      └─ ConfigService::read_entries()
           └─ DbConfigResolver::refresh() + resolve()   （真实 resolver.rs）
                ├─ SELECT … FROM platform_config          （平台层）
                ├─ SELECT … FROM platform_tenant_config   （租户层）
                └─ 成对 epoch 比对（D55，不取 max）
      → ConfigSource::{Platform,Tenant,Default}          （**从 DB 解析出来的**）
  → platform_server::i18n::negotiate_locale / negotiate_timezone / resolve_currency（真实纯函数）
  → platform_server::i18n::router + axum::middleware       （真实 HTTP）
  → 响应体 {"locale","timezone","currency","source","sources"}
```

### 3.2 关键代码片段

`crates/platform-server/tests/i18n_context_real_db.rs:186-209`：

```rust
let app_url = app_role_url(&url);
let app_db = TenantDb::connect(&app_url, DbOptions::default())
    .await
    .expect("应用角色连接串应可解析");
let fx_db = TenantDb::connect(&app_url, DbOptions::default())
    .await
    .expect("应用角色连接串应可解析");
let registry = Arc::new(
    ConfigRegistry::from_manifest_root(&root).expect("配置注册表应能从 manifest 建立"),
);
let service = ConfigService::new(app_db, registry.clone());

let state = Arc::new(I18nState {
    // ★ 真实配置服务 —— `platform_server::i18n` 里为 `ConfigService` 实现的
    //   `I18nSettingsSource` 适配器：它调用**真实**的 `read_entries`
    //   （→ 真实 `DbConfigResolver` → 真实 `platform_config` /
    //   `platform_tenant_config` 解析）。**不是** `FakeSettings`。
    settings: Arc::new(service.clone()),
    // 汇率接缝同样用**真实**实现（本用例不碰它，但不留假实现）。
    rates: Arc::new(platform_db::fx::FxRateStore::new(fx_db)),
    tokens: Arc::new(StubTokens),
    // 「已注册语言」的唯一来源（裁定 #16）：翻译包目录的文件名。
    locales: Arc::new(registry.locales().clone()),
});
```

**为什么这不是 FakeSettings**：

- `settings` 的类型是 `Arc<dyn I18nSettingsSource>`；这里塞进去的是
  `Arc<ConfigService>` —— `crates/platform-server/src/i18n.rs:219-270` 里
  **生产用的那个 `impl I18nSettingsSource for platform_config::ConfigService`**。
  它内部调 `self.read_entries(ctx, &keys)`（真实 SQL），
  `source` 由 `DbConfigResolver::resolve` 按**实际命中的层**产出
  （`crates/platform-config/src/resolver.rs:256-274`）。
- `i18n_api.rs` 的 `FakeSettings` 则是 `impl I18nSettingsSource for FakeSettings`，
  `Setting.source` 是**手写的常量**（`ConfigSource::Platform` / `Tenant` …）。
  本文件**没有任何一处手写 `ConfigSource`** —— 全是 DB 解析出来的。
- 写夹具也走**真实写通道**：平台层用 `service.write_entries(…, WriteRoute::Platform)`
  → `platform_config_upsert`（SECURITY DEFINER）+ 同事务自增平台 epoch；
  租户层用 `WriteRoute::Scoped` → `platform_tenant_config` + 自增租户 epoch。
  **不是**手工 INSERT 绕过校验。

### 3.3 唯一的替身：`StubTokens`（令牌校验）

```rust
struct StubTokens;

impl TokenVerifier for StubTokens {
    fn verify(&self, token: &str) -> Result<VerifiedToken, ConfigError> {
        if token != TOKEN {
            return Err(ConfigError::Unauthenticated);
        }
        Ok(VerifiedToken { user_id: USER.to_string(), tenant_id: TENANT.to_string() })
    }
}
```

**如实说明**：令牌校验是**唯一**的替身。它只回答「你是谁、属于哪个租户」，
与本条用例要守的「配置层 → 协商 → 响应体」**正交**。
生产用的是 IAM 的 JWT 验签（`main.rs` 里的 `ConfigIamTokenVerifier`），
在本机真库夹具里铸造真 JWT 需要一整套 IAM 环境变量 + 密钥，超出本轮范围。
**风险**：如果将来 `authenticate` 中间件本身坏了，本用例**测不到**（它只保证
认证通过后的链路正确）。这是**已知且可接受**的边界 —— 认证行为由
`i18n_api.rs::context_requires_authentication` 覆盖。

---

## 4. 变异验证（必须做）

### 4.1 操作与还原

```bash
# 1) 备份（该文件是 untracked 新文件，git checkout 还原不了）
mkdir -p /tmp/req021-fix3
cp crates/platform-server/src/i18n.rs /tmp/req021-fix3/i18n.rs.orig
sha256sum crates/platform-server/src/i18n.rs /tmp/req021-fix3/i18n.rs.orig
# → b25eb0a576ce206aba6f68b268e4f1d52f55d2e6534e650b2aceb137624f54c2  （两者一致）

# 2) 变异 M-B
#    crates/platform-server/src/i18n.rs:143
#    -  ConfigSource::Platform => Some(LocaleSource::Platform),
#    +  ConfigSource::Platform => Some(LocaleSource::Tenant),

# 3) 跑新用例 → 必须变红
cargo test -p platform-server --test i18n_context_real_db -- --test-threads=1

# 4) 还原 + ⚠️ touch（cp -p 会保留旧 mtime → cargo 复用含变异的陈旧二进制）
cp /tmp/req021-fix3/i18n.rs.orig crates/platform-server/src/i18n.rs
touch crates/platform-server/src/i18n.rs      # ← 强制重编
```

### 4.2 M-B 变异下的结果（本轮实测）

```
running 5 tests
test nothing_configured_falls_back_to_the_manifest_default_end_to_end ... ok
test the_accept_language_header_still_wins_over_the_platform_layer_end_to_end ... FAILED
test the_platform_layer_reaches_the_http_context_as_source_platform ... FAILED
test the_tenant_layer_wins_over_the_platform_layer_end_to_end ... ok
test the_user_preference_still_wins_over_the_platform_layer_end_to_end ... FAILED

test result: FAILED. 2 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.83s
```

**变红的用例（3 条）**：

1. `the_platform_layer_reaches_the_http_context_as_source_platform` ★
   ```
   platform-only → {"currency":"USD","locale":"en-US","source":"tenant",
                    "sources":{"currency":"tenant","locale":"tenant","timezone":"tenant"},
                    "timezone":"America/New_York"}
   assertion `left == right` failed: 来源必须如实报 platform（报成 tenant / default 都是 D119 回归）
     left: String("tenant")
    right: "platform"
   ```
2. `the_accept_language_header_still_wins_over_the_platform_layer_end_to_end`
   ```
   assertion `left == right` failed
     left: String("tenant")     ← sources.timezone 本应是 platform
    right: "platform"
   ```
3. `the_user_preference_still_wins_over_the_platform_layer_end_to_end`
   ```
   assertion `left == right` failed
     left: String("tenant")     ← sources.currency 本应是 platform
    right: "platform"
   ```

**仍然绿的 2 条**（**符合预期**，与平台层无关）：`nothing_configured_…`（default 层）、
`the_tenant_layer_wins_…`（租户层）。

**同一变异下，既有测试的表现（复核结论复现）**：

| 既有测试 | M-B 下结果 | 说明 |
|---|---|---|
| `i18n_settings_real_db.rs`（真库，3 条） | **3 passed / 0 failed** | ★ **缺口复现**：真库路径本来就没护栏 |
| `i18n_api.rs`（HTTP，17 条） | 15 passed / **2 failed** | `FakeSettings` 手写的 source 碰巧能抓住 |
| `src/i18n.rs` 单元测试 | 58 passed / **3 failed** | `d119_each_config_layer_maps_to_its_own_source` 等 |

⇒ **M-B 总计红 8 条**（单元 3 + HTTP 2 + **新真库 3**），其中**真库路径从 0 红变成 3 红**
—— 这就是本轮补上的牙齿。

### 4.3 还原后（+ `touch`）全绿

```
$ cargo test -p platform-server --test i18n_context_real_db -- --include-ignored
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.09s
```

### 4.4 sha256 对比（还原后 vs 进场）

```
$ sha256sum crates/platform-server/src/i18n.rs /tmp/req021-fix3/i18n.rs.orig
b25eb0a576ce206aba6f68b268e4f1d52f55d2e6534e650b2aceb137624f54c2  crates/platform-server/src/i18n.rs
b25eb0a576ce206aba6f68b268e4f1d52f55d2e6534e650b2aceb137624f54c2  /tmp/req021-fix3/i18n.rs.orig

$ diff -q crates/platform-server/src/i18n.rs /tmp/req021-fix3/i18n.rs.orig
（无输出 = IDENTICAL）
```

进场时复核者的备份 sha256 是 `b25eb0a5…` —— **与本轮进场一致** ⇒ 进场到离场该文件**零改动**。

---

## 5. 夹具清理验证（不留残留）

### 5.1 清理策略

| 位置 | 动作 | 用什么连接 |
|---|---|---|
| `platform_config`（平台层三键） | `DELETE … WHERE key = ANY($1)` | **管理员**（`base`）—— 应用角色对它只有 SELECT，直接 DML 是 42501 |
| `platform_tenant_config`（本夹具租户三键） | `DELETE … WHERE tenant_id = $1 AND key = ANY($2)` | 管理员 |
| `platform_config_epoch`（`platform` 与 `tenant:<T>`） | upsert `epoch = epoch + 1` | 管理员 |

- **开始前**：`setup()` 里先 `clear_i18n(&admin)`（每条用例都跑）。
- **结束前**：① 用例在**断言之前**就 `clear_i18n(&fx.admin)` ——
  断言失败也不会留残留；② `Fixture` 里挂一个 `Cleanup` **Drop 守卫**兜底
  （断言 panic 时展开也会跑 Drop），在**独立线程 + 独立 runtime** 里
  **另建连接池**同步清完再 `join()`。
  ⚠️ 踩过的坑：不能复用当前 runtime 建出来的 sqlx 连接池（跨 runtime → `PoolTimedOut`，
  实测整轮测试从 1.8s 变成 151s）。
- 本夹具**独占租户** `tenant-i18n-context-e2e` → 与其它用例互不干扰。

### 5.2 用例跑完后查库（命令与输出）

```bash
podman exec base-postgres psql -U base -d base_test -tAc \
"SELECT 'platform_config|'||key||'='||value::text FROM platform_config WHERE key IN ('locale','timezone','currency')
 UNION ALL SELECT 'platform_tenant_config|'||tenant_id||'/'||key||'='||value::text FROM platform_tenant_config WHERE key IN ('locale','timezone','currency') ORDER BY 1;"
```

输出（**紧跟** `cargo test -p platform-server --test i18n_context_real_db` 之后）：

```
platform_tenant_config|tenant-a/currency="USD"
platform_tenant_config|tenant-a/locale="en-US"
platform_tenant_config|tenant-a/timezone="America/New_York"
platform_tenant_config|tenant-i18n-settings/currency="JPY"
platform_tenant_config|tenant-i18n-settings/locale="zh-CN"
platform_tenant_config|tenant-i18n-settings/timezone="Europe/London"
```

```bash
podman exec base-postgres psql -U base -d base_test -tAc \
"SELECT count(*) FROM platform_tenant_config WHERE tenant_id='tenant-i18n-context-e2e';"
→ 0
```

**结论**：

- `platform_config` 的三个 i18n 键 —— **0 行**（平台层干净）；
- `tenant-i18n-context-e2e` —— **0 行**（本夹具干净）；
- 剩下的 6 行**全部是进场就有的、与本轮无关的**残留：
  - `tenant-a/*`：`crates/platform-config/tests/i18n_config_real_db.rs` 留下的
    （该文件同样不清理租户级行）；
  - `tenant-i18n-settings/*`：**复核者报的那 3 行**残留，来自
    `crates/platform-server/tests/i18n_settings_real_db.rs`。
    本轮**没有动**它（不在本轮范围内），如实上报。

**失败路径也干净**：M-B 变异下 3 条用例 panic 后立即查库，输出同上
（平台层 0 行、本夹具租户 0 行）—— `Cleanup` Drop 守卫生效。

### 5.3 进/离场 DB 状态一致性

跑完全部 crate 的测试后，`platform-config` 那个 crate 的真库用例把 `tenant-a` 的
三行改成了 `locale="zh-CN"`（只留 1 行）。本轮**已把它还原成进场值**
（`locale="en-US"` / `timezone="America/New_York"` / `currency="USD"`），
所以现在库里的状态与进场**逐行一致**。

---

## 6. 各 crate 的测试命令与结果（逐 crate，`--all-targets -- --include-ignored`）

环境：

```bash
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
cd /home/myg133/Projects/system_seed/feature-REQ-021
```

| crate | 命令 | 结果（原文照抄） |
|---|---|---|
| platform-core | `cargo test -p platform-core --all-targets -- --include-ignored` | `test result: ok. 120 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`<br>`test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`<br>`test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s` |
| platform-contracts | 同上 | `test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`<br>`test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s` |
| platform-contracts-macros | 同上 | `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s` |
| platform-config | 同上 | `38 passed; 0 failed`（0.32s）／`4 passed; 0 failed`（0.00s）／`25 passed; 0 failed`（11.50s）／`3 passed; 0 failed`（0.04s）／`10 passed; 0 failed`（4.08s）／`3 passed; 0 failed`（1.31s） |
| platform-db | 同上 | `38 passed; 0 failed`（0.01s）／`1 passed; 0 failed`（0.37s）／`5 passed; 0 failed`（0.44s）／`2 passed; 0 failed`（0.36s） |
| platform-iam | 同上 | `42 passed; 0 failed`（0.08s）＋ `iam_real_db`：`15 passed; 1 failed`（14.46s）★ **预存 flake，见 §8** |
| platform-org | 同上 | `69 passed; 0 failed`（0.00s）／`15 passed; 0 failed`（1.85s）／`7 passed; 0 failed`（0.80s） |
| platform-rbac | 同上 | `43 passed; 0 failed`（0.00s）／`3 passed; 0 failed`（0.81s）／`10 passed; 0 failed`（3.82s）／`22 passed; 0 failed`（6.52s） |
| platform-schema | 同上 | `11 passed; 0 failed`（0.00s）／`9 passed; 0 failed`（0.00s） |
| **platform-server**（本轮改动所在 crate） | 同上 | `61 passed; 0 failed`（lib）／`8 passed; 0 failed`（bin）／`17 passed; 0 failed`（i18n_api）／**`5 passed; 0 failed`（i18n_context_real_db ← 本轮新增）**／`6 passed; 0 failed`（i18n_no_chinese_titles）／`3 passed; 0 failed`（i18n_settings_real_db）／`6 passed; 0 failed`（i18n_titles） |
| sandbox | 同上 | `10 passed; 0 failed`／`4 passed; 0 failed`／`6 passed; 0 failed`／`7 passed; 0 failed`／`6 passed; 0 failed` |
| xtask | 同上 | `51 passed; 0 failed`（0.09s）／`0 passed; 0 failed`（0.00s） |

**合计**：除 platform-iam 的 1 条预存 flake 外，**全部通过**。
`platform-server` 合计 **106 passed / 0 failed**（本轮新增 5 条）。

补充：

```bash
$ cargo clippy -p platform-server --tests
（无 warning / 无 error —— 新文件零告警）

$ cargo test -p platform-server --test i18n_context_real_db -- --include-ignored
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.09s
```

---

## 7. 为什么单独建文件（而不是加进 `i18n_settings_real_db.rs`）

选**新建独立文件** `crates/platform-server/tests/i18n_context_real_db.rs`：

1. **零冲突**：本轮工作区里 `crates/platform-server/tests/` 整个目录是 untracked 的
   新增目录，别的 Dev 很可能同时在改同目录下的文件。独立文件 → 不会撞车。
2. **职责不同**：既有文件的用例断言的是「**配置层**解析出来的 `Setting`」
   （`source` / `value` / `is_explicit`）；本文件断言的是「**HTTP 响应体**」。
   混在一起会让「这一条到底在测哪一层」变模糊。
3. **夹具不同**：既有文件用 `tenant-i18n-settings` 夹具租户；本文件用
   `tenant-i18n-context-e2e`（独占）→ 不与既有文件抢同一行。
4. **可复用**：`clear_platform_i18n()` 的思路（管理员连接 + 手动补 epoch）被**照搬**到
   本文件的 `clear_i18n()` 里（复核者提醒的「应用角色对 `platform_config` 只有 SELECT，
   实测 42501」那个坑没有重踩）。

---

## 8. 未做 / 做不到 / 有疑虑的项

### 8.1 预存的 platform-iam flake（**与本轮无关**，但如实上报）

```
crates/platform-iam/tests/iam_real_db.rs:706
  the_three_failure_paths_have_similar_latency
  assert!(ratio < 1.5, "三条失败路径的耗时必须接近（ratio = {ratio:.2} …）")
```

诊断（本轮实测）：

| 跑法 | 结果 |
|---|---|
| 单跑这一条 | **ok**（ratio = 1.01 / 1.19） |
| 整个 `iam_real_db` 二进制，`--test-threads=1` | **ok**（16 passed） |
| 整个 `iam_real_db` 二进制，默认并发 | **间歇性 FAILED**（15 passed / 1 failed） |

⇒ 是**耗时比值断言**在同二进制内 16 条用例抢 CPU 时不稳定（环境相关）。
**与本轮改动无关**：`platform-iam` 既不依赖 `platform-server`，也不编译本轮新增的
测试文件（`crates/platform-server/tests/` 只进 `platform-server` 的测试二进制）。
本轮**没有**去改它（超出「只补这一条测试」的范围）。
**建议**：交给 QA/其它轮次处理（例如放宽比值阈值，或给这条测试单独串行）。

### 8.2 令牌校验仍是替身

见 §3.3。`StubTokens` 不验签，只回固定租户。
**影响**：认证中间件本身的正确性不在本文件覆盖范围内（由 `i18n_api.rs` 覆盖）。
**要真覆盖**需要在本机起 IAM + 铸真 JWT（`BASE_JWT_*` / `BASE_IAM_*` 环境变量），
超出本轮范围。

### 8.3 断言依赖「已注册语言」= `zh-CN` / `en-US`

`negotiate_locale` 只接受**已注册**的语言（裁定 #16：已注册语言 = `packages/i18n/src/locales/*.json`
的文件名）。当前该目录只有 `zh-CN.json` / `en-US.json`，
所以平台层只能用 `en-US` 与 manifest 默认的 `zh-CN` 区分。
若将来前端只生成 `zh-CN.json`，用例 1 会因为「平台层的 `en-US` 未注册 → 回落默认」而红
—— 那是**正确的红**（说明平台层没生效），但需要有人知道「这条用例隐含依赖 `en-US` 已注册」。
（`crates/platform-config/tests/i18n_config_real_db.rs` 用了「取注册表里第一个非 `zh-CN` 的语言」
的写法来规避；本文件因为要**断言具体字面值**，没法那么写。）

### 8.4 清理守卫的线程模型

`Cleanup` 的 Drop 会起一个线程 + 独立 runtime + **新连接池**。
若 `thread::spawn` 失败或新池连不上 → **静默不清理**（`let _ =` 吞掉了错误）。
此时残留最多留到**下一条用例的 `setup()`**（每条用例开始前都 `clear_i18n`），
不会跨用例污染。正常路径（连接正常）实测可靠。

### 8.5 没有做的事（明确不做）

- **没有改实现**（`crates/platform-server/src/i18n.rs` 离场与进场 sha256 相同）。
- **没有改** `AGENTS.md`、`packages/**`、`apps/**`、`e2e/**`、任何迁移文件。
- **没有跑前端测试**（本轮不动前端）。
- **没有** `git commit` / `push` / `checkout` / `stash` / `reset` / `restore`。
- **没有**去清理 `tenant-i18n-settings` 的 3 行残留（属于 `i18n_settings_real_db.rs`
  的夹具问题，不在本轮范围；已在 §5.2 如实列出）。

---

## 9. 离场状态核对

```bash
$ git status --short
…
 M crates/platform-server/src/i18n.rs        ← ❌ 不存在（见下）
?? crates/platform-server/src/i18n.rs        ← ✅ untracked，与进场一致
?? crates/platform-server/tests/              ← ✅ untracked 目录，与进场一致
                                         （新增文件 crates/platform-server/tests/i18n_context_real_db.rs 在其中）
```

- `git status --short` 的**每一行**与进场**完全一致**（新增的文件落在一个**进场就存在**的
  untracked 目录 `crates/platform-server/tests/` 下，所以目录级那一行没变）。
- `crates/platform-server/src/i18n.rs`：sha256 `b25eb0a5…`，`diff` 与备份 **IDENTICAL**。
- 唯一新增的文件：`crates/platform-server/tests/i18n_context_real_db.rs`。
- 唯一新增的文档：`feature-meta/req-021-fix3.md`（本文件）。
