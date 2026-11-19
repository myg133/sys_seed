# REQ-021 修复轮 1 — Dev 报告

**范围**：只修 BA 核实的 4 个缺口，不重做实现、不重构。
**分支**：`feature/REQ-021`（工作目录 `/home/myg133/Projects/system_seed/feature-REQ-021`）
**状态**：**未提交**（按 BA 要求，`git commit` / `git push` 均未执行）。

---

## 0. 一句话结论

| 缺口 | 结果 | 证据 |
|---|---|---|
| 1. 12 处中文 `title` | ✅ 全部改为走 `TITLES` 表；新增跨行 + 含 `modules/` 的静态扫描测试（带阳性对照） | §1.2 / §1.3 |
| 2. `TITLES` 漏码 | ✅ 补 5 行（含 BA 点的 4 个 + `INVALID_QUERY`，理由见 §2.2）；`platform-db` 导出 `ALL_CODES` 并接入完备性测试 | §2 |
| 3. capabilities 索引过期 | ✅ `cargo xtask capabilities` 重新生成；`--check` 通过 | §3 |
| 4. `extension/schema.rs` 神秘 diff | ✅ 证明是 **rustfmt 产物**（逐字节可复现），保留不动 | §4 |

**测试**：9 个 crate，**630 passed / 0 failed / 0 ignored**（`--include-ignored`，真实库用例全部真跑）。
**CI 门禁**：`cargo clippy --workspace --all-targets -- -D warnings` → **exit 0**。

---

## 1. 缺口 1：12 处中文 `title`

### 1.1 逐处修改对照表

行号 = **改后**的行号（每处新增了 1 行 `use` import，故比 BA 给的行号 +1）。

| # | 文件 | 行号 | 旧值 | 新取值方式 | 依赖方向确认 |
|---|---|---|---|---|---|
| 1 | `crates/platform-schema/src/http.rs` | 188 | `"服务内部错误"` | `title_of_code(platform_core::problem::CODE_INTERNAL_ERROR)` | `platform-schema` 已依赖 `platform-core`（`crates/platform-schema/Cargo.toml:16`） |
| 2 | `crates/platform-schema/src/http.rs` | 246 | `"元数据不存在"` | `title_of_code(code)`（`code` 是入参，运行时为 `CODE_SCHEMA_MODULE_NOT_FOUND` / `CODE_SCHEMA_ENTITY_NOT_FOUND`） | 同上 |
| 3 | `modules/sandbox/backend/src/demo_item_http.rs` | 131 | `"服务内部错误"` | `title_of_code(platform_core::problem::CODE_INTERNAL_ERROR)` | `sandbox` **已依赖** `platform-core`（`modules/sandbox/Cargo.toml:28`） |
| 4 | `modules/sandbox/backend/src/demo_item_http.rs` | 224 | `"服务内部错误"` | 同上 | 同上 |
| 5 | `modules/sandbox/backend/src/demo_note_http.rs` | 101 | `"服务内部错误"` | 同上 | 同上 |
| 6 | `modules/sandbox/backend/src/demo_note_http.rs` | 189 | `"服务内部错误"` | 同上 | 同上 |
| 7 | `modules/sandbox/backend/src/http.rs` | 195 | `"服务内部错误"` | 同上 | 同上 |
| 8 | `modules/sandbox/backend/src/http.rs` | 321 | `"服务内部错误"` | 同上 | 同上 |
| 9 | `modules/sandbox/backend/src/http.rs` | 349 | `"服务内部错误"` | 同上 | 同上 |
| 10 | `modules/sandbox/backend/src/http.rs` | 389 | `"服务内部错误"` | 同上 | 同上 |
| 11 | `modules/sandbox/backend/src/http.rs` | 404 | `"服务内部错误"` | 同上 | 同上 |
| 12 | `modules/sandbox/backend/src/http.rs` | 445 | `"服务内部错误"` | 同上 | 同上 |

**依赖方向**：BA 担心的「业务模块反向依赖平台内部 crate」**不成立** ——
`modules/sandbox/Cargo.toml:28` 已有 `platform-core.workspace = true`
（注释原文：「业务模块只依赖平台核心（扩展点 trait），不依赖平台内部模块」）。
`platform-core` 是平台能力面，`platform-schema` 也已依赖它
（`crates/platform-schema/Cargo.toml:16`）。因此**全部走 `TITLES` 表**，
没有出现「直接写英文字面量」的退路。

**「元数据不存在」对应的 code**：不是单一 code，而是 `schema_not_found(ctx, instance, code, detail)`
的 `code` 入参，两个调用点分别是：
- `crates/platform-schema/src/http.rs:213` → `CODE_SCHEMA_MODULE_NOT_FOUND`
- `crates/platform-schema/src/http.rs:221` → `CODE_SCHEMA_ENTITY_NOT_FOUND`

两个都已在 `TITLES` 表里（`crates/platform-core/src/titles.rs:51-52`），
所以 `title_of_code(code)` 对两者都取到表内值（**不是** ASCII 兜底）。

**没有就地写英文字面量**：12 处全部是 `title_of_code(...)` 调用，
D113 的「唯一来源」没被破坏。

**改动后全仓残留的中文**（`grep` 结果，均**不是** title 位置）：

```
crates/platform-core/src/problem.rs:39:/// 稳定的机器可读错误码：服务内部错误。     ← 注释
crates/platform-iam/src/error.rs:63,162:/// 服务内部错误。                          ← 注释
crates/platform-rbac/src/error.rs:43,112:/// 服务内部错误。                         ← 注释
crates/platform-org/src/error.rs:50,144:/// 服务内部错误。                          ← 注释
crates/platform-config/src/http.rs:407:tracing::error!(..., "配置服务内部错误");     ← 日志
modules/sandbox/backend/src/demo_note_http.rs:184:/// 服务内部错误（…）。              ← 注释
modules/sandbox/backend/src/demo_item_http.rs:219:/// 服务内部错误（…）。              ← 注释
```

注释与 `tracing` 日志按 `crates/platform-core/src/titles.rs:27-29` 的约定
（「`tracing` 日志、`params` 里的用户数据、代码注释 —— 这些都不是响应体的文案」）**不整改**。

### 1.2 新增的静态扫描测试

**文件**：`crates/platform-server/tests/i18n_no_chinese_titles.rs`（新增，6 个测试）

| 项 | 实现 |
|---|---|
| **扫描范围** | 仓库根下的 `crates/` 与 `modules/`，递归全部 `.rs`（跳过 `target/` / `node_modules/` / `.git/`）。**`modules/` 必含** —— 第一轮正是在这里漏了 10 处。 |
| **剥注释** | 自写词法分析器，正确处理 `//` 行注释、`/* */` 嵌套块注释；**字符串字面量里的 `//` 不会被当成注释**（正则做不到这点）。 |
| **跨行怎么处理** | 不按行匹配。先把源码切成 **token 流**（空白与换行被丢弃，字符串字面量保留内容），再在 token 流上定位位置：① `from_context(` / `ProblemDetails::new(` 的**第 2 个实参**；② `ProblemDetails { ... }` 结构体字面量里的 `title:` 字段。**跨不跨行结果完全一样**，因为换行只是被丢掉的空白。 |
| **命中条件** | 该位置上是**字符串字面量**且**非 ASCII**。 |
| **阳性对照** | `the_scanner_detects_an_injected_non_ascii_title`：喂入 3 段**故意注入**的源码（同行 / **跨行** / `title:` 字段），断言扫描器**必须命中**。跨行那例是第一轮 12 处漏网的**原样形状**。 |
| **阴性对照** | `the_scanner_ignores_non_title_positions`（日志 / 参数 / 注释里的中文）、`the_scanner_ignores_business_data_titles`（业务实体 `TenantItem { title: "同名记录" }` —— **用户数据**，本来就该本地化）、`the_scanner_accepts_ascii_title_literals`。 |
| **防空绿** | 扫描范围非空被断言（`files >= 40`）；`modules/` 确实被扫到被断言（`files_in_modules >= 5`）。扫描范围被悄悄改小 → 断言红。 |

**为什么 `title:` 位置只认 `ProblemDetails`**：`title` 是极常见的字段名 ——
`modules/sandbox/tests/c3_real_db.rs:236` 的 `TenantItem { title: "同名记录" }` 是
**业务数据**（用户自己的标题），按契约 C6 应当本地化，不该被整改。
扫描器因此按花括号上下文判定「这个 `title` 是不是**错误体**的 title」。
（这条是在**第一版扫描器报出这个假阳性之后**收紧的，收紧过程记录在 §8.1。）

### 1.3 实际输出

**阳性对照**（`cargo test -p platform-server --all-targets --test i18n_no_chinese_titles -- --nocapture`）：

```
阳性对照（同行）：注入 1 处非 ASCII title → 扫描器命中 1 处：[(3, "from_context", "服务内部错误")]
阳性对照（跨行）：注入 1 处非 ASCII title → 扫描器命中 1 处：[(5, "from_context", "服务内部错误")]
阳性对照（title: 字段）：注入 1 处非 ASCII title → 扫描器命中 1 处：[(5, "title:", "服务内部错误")]
```

**全仓扫描**（同一命令）：

```
扫描范围：crates/ + modules/ 共 145 个 .rs 文件（其中 modules/ 下 16 个）；剥注释后 title 位置的中文字面量命中数 = 0
```

**测试结果**：

```
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
```

---

## 2. 缺口 2：`TITLES` 完备性

### 2.1 `TITLES` 补的行（5 行，不是 4 行 —— 见 §2.2）

`crates/platform-core/src/titles.rs`，新增段落在「platform-server 健康检查」之后、
「platform-core::money」之前：

```rust
    // -- platform-db（数据访问层）-------------------------------------------
    // 这些码经 `ConfigError::Database(code)` 进**响应体的 `detail` 字段**
    // （`crates/platform-config/src/error.rs`：`the config store is unavailable ({code})`），
    // 且 `DbError::code()` 是稳定机器可读码 → 前端可能按它分支 → 必须有标题。
    ("DATABASE_ERROR", "Database Error"),
    ("INVALID_QUERY", "Invalid Query"),
    ("RECORD_NOT_FOUND", "Record Not Found"),
    ("RLS_BYPASSED_BY_ROLE", "RLS Bypassed By Role"),
    ("RLS_POLICY_MISSING", "RLS Policy Missing"),
```

### 2.2 为什么补了 5 行而不是 BA 点的 4 行

BA 点的 4 个：`DATABASE_ERROR` / `RECORD_NOT_FOUND` / `RLS_BYPASSED_BY_ROLE` / `RLS_POLICY_MISSING`。
**第 5 个是 `INVALID_QUERY`**，理由是**一致性**，不是发散：

`DbError::code()` 的 6 个码走的是**完全同一条**通道进响应体
（`crates/platform-db/src/error.rs:52-61`）：

```rust
Self::Database(_)      => "DATABASE_ERROR",       // error.rs:54
Self::MissingTenant    => "TENANT_CONTEXT_MISSING", // error.rs:55  ← 表里已有（iam 段）
Self::InvalidQuery{..} => "INVALID_QUERY",        // error.rs:56
Self::RowNotFound      => "RECORD_NOT_FOUND",      // error.rs:57
Self::MissingPolicy{..}=> "RLS_POLICY_MISSING",   // error.rs:58
Self::RlsBypassed{..}  => "RLS_BYPASSED_BY_ROLE", // error.rs:59
```

只要把 `platform-db` 的 `ALL_CODES` 接进完备性测试（BA 动作 2 的明文要求），
`INVALID_QUERY` 就**必然**出现在清单里 —— 若不给它 `TITLES` 条目，
`titles_cover_every_code` 会直接红。所以只有两个选择：
(a) 给它 `TITLES` 条目；(b) 把它排除出 `ALL_CODES`（= 让 `ALL_CODES` 名不副实）。
选 (a)。

**对 BA 判断的一处修正**：BA 说这 4 个码「进 HTTP 响应体」——**成立**，
但精确路径是 **`detail` 字段**而不是 `code` 字段：

- `crates/platform-config/src/error.rs:360-364`：`From<DbError> for ConfigError` → `ConfigError::Database(error.code().to_string())`
- `crates/platform-config/src/error.rs:269`：`Self::Database(code) => format!("the config store is unavailable ({code})")`
  → db 码**原文**出现在响应体的 `detail` 里
- `crates/platform-config/src/error.rs:163`：`Self::Database(_) | Self::Internal { .. } => CODE_INTERNAL_ERROR`
  → 响应体的 `code` 字段是 `INTERNAL_ERROR`，**不是** db 码

即：db 码是**稳定机器可读码**且**出现在响应体里** → 前端可能按它分支 → 需要标题。
本轮**不改**这个映射（改 `code` 字段的语义超出本轮范围，且会动第一轮已验收的行为）。

### 2.3 `platform-db` 导出的 `ALL_CODES`

`crates/platform-db/src/error.rs:92-99`（新增）：

```rust
pub const ALL_CODES: &[&str] = &[
    "DATABASE_ERROR",
    "TENANT_CONTEXT_MISSING",
    "INVALID_QUERY",
    "RECORD_NOT_FOUND",
    "RLS_POLICY_MISSING",
    "RLS_BYPASSED_BY_ROLE",
];
```

配套两处：

- `crates/platform-db/src/lib.rs:56`：`mod error;` → **`pub mod error;`**
  （与 `platform-config` / `platform-iam` / `platform-org` / `platform-rbac` / `platform-core`
  的 `pub mod error;` 写法对齐 —— `platform-db` 原先是唯一私有的）
- `crates/platform-db/src/error.rs:147-166`：新增测试 `all_codes_matches_the_variants`
  —— 双向校验清单与 `code()` 的 `match` **逐项一致**
  （清单多 = 有码永远产不出；清单少 = 有码永远没标题）

### 2.4 完备性测试接入

`crates/platform-server/tests/i18n_titles.rs:50-54`：

```rust
    // 数据访问层的码（REQ-021 补齐）：`DbError::code()` 是稳定机器可读码，
    // 经 `ConfigError::Database(code)` 进响应体的 `detail` 字段
    // （`the config store is unavailable ({code})`）→ 必须有标题。
    for code in platform_db::error::ALL_CODES {
        codes.push(("platform-db", code));
    }
```

`platform-server` 已依赖 `platform-db`（`crates/platform-server/Cargo.toml:23`），无需改依赖。

### 2.5 完备性测试的实际输出

```
$ cargo test -p platform-server --all-targets --test i18n_titles -- --nocapture

running 6 tests
test codes_follow_their_namespace_convention ... ok
test no_orphan_titles ... ok
test titles_are_ascii_and_unique ... ok
test unknown_codes_fall_back_to_an_ascii_placeholder ... ok
TITLES = 58 行；各 crate 的 code 合计 75 条（去重后 58 条）
test titles_cover_every_code ... ok
test every_registered_code_produces_a_cjk_free_problem_body ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**原文那行**：`TITLES = 58 行；各 crate 的 code 合计 75 条（去重后 58 条）`

补 5 行前是 `53 行 / 70 条（去重后 53 条）`；补 5 行后 58 / 75 / 58。
`no_orphan_titles` 通过 = **58 个键全部被某个 crate 的 `ALL_CODES` 认领**
（无孤儿键）；`titles_cover_every_code` 通过 = **75 条 code 全部有标题**。

### 2.6 逐个码的复核判定表

复核方法：找到每个码的**产生点**，看它最终走的是「HTTP 响应体」还是「启动失败 / 日志 / 内部」。

| 码 | 产生点 | 是否进 HTTP 响应体 | 证据（文件:行号） | 结论 |
|---|---|---|---|---|
| `INVALID_CONFIG_KEY` | `platform-core/src/error.rs:31`（`CoreError::InvalidConfigKey`） | **否** | 唯一产生点 `crates/platform-core/src/config.rs:124`（`ConfigKey::parse`）；HTTP 路径的 4 个调用点 `crates/platform-config/src/service.rs:163, 208, 290, 545` 全部 `.map_err(|_| ConfigError::KeyInvalid { key })` —— **`CoreError` 被丢弃**，响应体 `code` 变成 `CONFIG_KEY_INVALID` | **不进表** ✓ BA 判断成立 |
| `INVALID_IDENTIFIER` | `platform-core/src/error.rs:32` | **否** | `grep -rn "CoreError::InvalidIdentifier" crates/ modules/` → **除 error.rs 自身的单元测试外无任何产生点** | **不进表** ✓ |
| `IO_ERROR` | `platform-core/src/error.rs:33` | **否** | `grep -rn "CoreError::Io" crates/ modules/` → **除 error.rs 自身的单元测试外无任何产生点** | **不进表** ✓ |
| `CONFIG_INVALID_VALUE` | `platform-server/src/config.rs:18` | **否** | `DeployConfig::from_env()`（`config.rs:183`）→ `crates/platform-server/src/main.rs:214` `DeployConfig::from_env().map_err(Failure::from)` → `main.rs:136-143` `eprintln!` + `ExitCode::from(EXIT_CONFIG)`，`EXIT_CONFIG = 2`（`main.rs:45`） | **不进表** ✓（启动期 fail-fast，退出码 2，走 stderr） |
| `CONFIG_MISSING_REQUIRED` | `platform-server/src/config.rs:16` | **否** | 同上（`config.rs:326` 产生，`config.rs:183` `from_env`） | **不进表** ✓ |
| `CONFIG_FORBIDDEN_COMBINATION` | `platform-server/src/config.rs:20` | **否** | 同上（`config.rs:267, 275` 产生） | **不进表** ✓ |
| `MANIFEST_JSON_INVALID` | `platform-core/src/extension/manifest.rs:52`（`ManifestError::code()`） | **否** | 唯一消费者 `crates/platform-schema/src/registry.rs:45` `ManifestIssue::from_error` —— `ManifestIssue` 结构体（`registry.rs:34-40`）**只有 `source` / `field` / `reason` 三个字段，根本没有 `code` 字段**；且 manifest 加载在**启动期**（`crates/platform-server/src/main.rs:392-396`「沙箱模块装配失败」→ `Failure::Runtime` → 退出码 1） | **不进表** ✓ BA 判断成立 |
| `MANIFEST_FIELD_INVALID` | `platform-core/src/extension/manifest.rs:53` | **否** | 同上 | **不进表** ✓ |
| `IAM_CONFIG_MISSING_REQUIRED` | `platform-iam/src/config.rs:29` | **否** | `IamConfig::from_env()`（`config.rs:273`）→ `crates/platform-server/src/main.rs:489` → `Failure::Config` → `main.rs:136-143` → 退出码 2 | **不进表** ✓ |
| `IAM_CONFIG_INVALID_VALUE` | `platform-iam/src/config.rs:31` | **否** | 同上（`config.rs:707` 产生） | **不进表** ✓ |
| `IAM_CONFIG_PLACEHOLDER_SECRET` | `platform-iam/src/config.rs:33` | **否** | 同上（`config.rs:716` 产生） | **不进表** ✓ |
| `DATABASE_ERROR` | `platform-db/src/error.rs:54` | **是**（`detail` 字段） | `platform-config/src/error.rs:360-364` → `error.rs:269` `the config store is unavailable (DATABASE_ERROR)` | **进表** ✓（BA 指定） |
| `RECORD_NOT_FOUND` | `platform-db/src/error.rs:57` | **是**（`detail` 字段） | 同上 | **进表** ✓（BA 指定） |
| `RLS_BYPASSED_BY_ROLE` | `platform-db/src/error.rs:59` | **是**（`detail` 字段） | 同上 | **进表** ✓（BA 指定） |
| `RLS_POLICY_MISSING` | `platform-db/src/error.rs:58` | **是**（`detail` 字段） | 同上 | **进表** ✓（BA 指定） |
| `INVALID_QUERY` | `platform-db/src/error.rs:56` | **是**（`detail` 字段） | 同上（产生点：`platform-db/src/fx.rs:93, 102`、`platform-db/src/query.rs` 多处） | **进表**（本轮补的第 5 行，理由见 §2.2） |
| `TENANT_CONTEXT_MISSING` | `platform-db/src/error.rs:55` | **是** | 同上 | **表里已有**（iam 段，`titles.rs:65`），本轮不重复登记 |

**小结**：BA 复核的 11 个码（3+3+2+3）的判断**全部成立**，逐条证据如上。
BA 点的 4 个 db 码**全部进表**，另加 `INVALID_QUERY`（同通道，理由 §2.2）。

> 注：任务书写「缺口 2 里那 12 个码」，实际清单是 **11 个复核码**（3+3+2+3）+ **4 个待补码** = 15 个。
> 本表把 15 个**全部**列出（16 行含 `TENANT_CONTEXT_MISSING`）。

---

## 3. 缺口 3：capabilities 索引过期

**修法**（按 BA 要求用生成器，不手改生成物）：

```
$ cargo xtask capabilities
能力索引已生成：14 个文件
  ✓ capabilities/api.json
  ✓ capabilities/extension-points.json
  ✓ capabilities/index.json
  ✓ capabilities/permissions.json
  ✓ capabilities/schema/entity.schema.json
  ✓ capabilities/schema/manifest.schema.json
  ✓ capabilities/schema/metadata.schema.json
  ✓ capabilities/sdk/auth.json
  ✓ capabilities/sdk/hooks.json
  ✓ capabilities/sdk/i18n.json
  ✓ capabilities/sdk/schema.json
  ✓ capabilities/sdk/sdk.json
  ✓ capabilities/sdk/theme.json
  ✓ capabilities/sdk/ui.json
```

**校验（CI 门禁）**：

```
$ cargo xtask capabilities --check
能力索引校验通过：14 个文件与源码一致
```

退出码 **0**。（修前的基线是 `E_CAPABILITIES_OUTDATED` +
「内容不一致：`capabilities/index.json`」「内容不一致：`capabilities/sdk/i18n.json`」。）

**改动的生成物**：`capabilities/index.json`（+2/-2）、`capabilities/sdk/i18n.json`（+78/-4）。

### 3.1 前端 `@base/i18n` 的能力是否被体现 —— **是**

生成器**覆盖**前端包（`crates/xtask/src/capabilities.rs:23` 的表：
`sdk/<package>.json` ← 各 `packages/*/package.json` + `src/index.ts` 的导出清单），
`packages/i18n` → `capabilities/sdk/i18n.json`。重新生成后该文件新增了
**13 个导出条目**：`BASE_LOCALE`、`LOCALE_OPTIONS`、`LocaleOption`、
`registeredLocales`、`isRegisteredLocale`、`I18nOptions`、**`I18n`**、
`missingKeys`、`resetMissingKeys`、`DEFAULT_CURRENCY_EXPOSITION`→`DEFAULT_CURRENCY_EXPONENT`、
`currencyExponent`、`formatIsoDateTime`、**`createI18n`**。

**一处如实说明（不硬编）**：任务书列的前端能力里，
`t` / `setLocale` / `formatDate` / `formatMoney` / `subscribe` 是
**`I18n` 接口的成员**，不是顶层 `export` 声明
（`packages/i18n/src/index.ts:94-113`）。生成器的抽取规则是
`crates/xtask/src/capabilities.rs:731` 的 `line.strip_prefix("export ")` ——
**只抽顶层 `export`**，因此这 5 个方法**不会**作为独立条目出现，
只体现为 `I18n` 接口这一条（`capabilities/sdk/i18n.json:46-51`）。
要让它们逐条出现，得改生成器去解析 `interface` 的成员 —— 超出本轮范围，未做。

**另一处观察（未改）**：`capabilities/sdk/i18n.json:90` 里 `translate` 的 `description`
变成了 `"--------------------------------------------------------------------------"`。
这是生成器把源码 doc 注释里的分隔线当成了描述（`packages/i18n/src/index.ts` 里
`translate` 上方有一条 `---...---` 分隔线）。`--check` 通过（文件与生成器输出一致），
但这个描述本身没意义。**属前端源码的注释排版问题**，不在本轮范围，未改。

---

## 4. 缺口 4：`extension/schema.rs` 的神秘 diff

`git diff crates/platform-core/src/extension/schema.rs` 全文（**19 增 19 删**）：

```diff
diff --git a/crates/platform-core/src/extension/schema.rs b/crates/platform-core/src/extension/schema.rs
index 722dffb..850efea 100644
--- a/crates/platform-core/src/extension/schema.rs
+++ b/crates/platform-core/src/extension/schema.rs
@@ -237,10 +237,7 @@ impl EntitySchema {
             ));
         }
         if self.label.trim().is_empty() {
-            issues.push(field_error(
-                &format!("{path}.label"),
-                "must not be empty",
-            ));
+            issues.push(field_error(&format!("{path}.label"), "must not be empty"));
         }
         if self.fields.is_empty() {
             issues.push(field_error(
@@ -306,7 +303,9 @@ impl EntitySchema {
             if !value.starts_with("/api/") {
                 issues.push(field_error(
                     &format!("{path}.api.{key}"),
-                    &format!("must start with /api/ (declared external URLs are rejected), got `{value}`"),
+                    &format!(
+                        "must start with /api/ (declared external URLs are rejected), got `{value}`"
+                    ),
                 ));
             }
         }
@@ -428,9 +427,7 @@ impl SchemaProjector {
 pub fn is_entity_key(value: &str) -> bool {
     let mut chars = value.chars();
     match chars.next() {
-        Some(first) if first.is_ascii_lowercase() => {
-            chars.all(|ch| ch.is_ascii_alphanumeric())
-        }
+        Some(first) if first.is_ascii_lowercase() => chars.all(|ch| ch.is_ascii_alphanumeric()),
         _ => false,
     }
 }
@@ -517,15 +514,15 @@ mod tests {
         assert_eq!(entity.fields[1].max_length, Some(200));
         assert_eq!(entity.fields[1].required, Some(true));
         assert_eq!(entity.fields[0].required, None);
-        assert_eq!(
-            entity.fields[2].permission(),
-            Some("sandbox:demo:secret")
-        );
+        assert_eq!(entity.fields[2].permission(), Some("sandbox:demo:secret"));
     }
 
     #[test]
     fn an_unknown_type_is_reported_with_the_field_path_and_allowed_set() {
-        let raw = entity_json().replace("\"type\": \"text\", \"required\"", "\"type\": \"currency\", \"required\"");
+        let raw = entity_json().replace(
+            "\"type\": \"text\", \"required\"",
+            "\"type\": \"currency\", \"required\"",
+        );
         let issues = parse_entity(&raw).validate("demoItem");
         assert_eq!(issues.len(), 1, "issues = {issues:?}");
         assert_eq!(issues[0].field(), "schema.demoItem.fields[1].type");
@@ -563,8 +560,14 @@ mod tests {
         let issues = parse_entity(&raw).validate("demoItem");
         assert_eq!(issues.len(), 2, "issues = {issues:?}");
         let fields: Vec<&str> = issues.iter().map(|issue| issue.field()).collect();
-        assert!(fields.contains(&"schema.demoItem.fields[2].type"), "{fields:?}");
-        assert!(fields.contains(&"schema.demoItem.fields[3].key"), "{fields:?}");
+        assert!(
+            fields.contains(&"schema.demoItem.fields[2].type"),
+            "{fields:?}"
+        );
+        assert!(
+            fields.contains(&"schema.demoItem.fields[3].key"),
+            "{fields:?}"
+        );
     }
 
     #[test]
@@ -607,10 +610,7 @@ mod tests {
 
     #[test]
     fn a_missing_required_api_endpoint_is_reported() {
-        let raw = entity_json().replace(
-            "\"create\": \"/api/sandbox/demo-items\",",
-            "",
-        );
+        let raw = entity_json().replace("\"create\": \"/api/sandbox/demo-items\",", "");
         let issues = parse_entity(&raw).validate("demoItem");
         assert_eq!(issues.len(), 1, "issues = {issues:?}");
         assert_eq!(issues[0].field(), "schema.demoItem.api.create");
```

### 4.1 结论：**纯格式化**，来源不明（但是 rustfmt）

**处置：保留不动**（未回退）。

**证明**（逐字节可复现，不是靠眼看）：

```
$ git show HEAD:crates/platform-core/src/extension/schema.rs > /tmp/schema_head.rs
$ cp /tmp/schema_head.rs /tmp/schema_fmt.rs
$ rustfmt --edition 2024 /tmp/schema_fmt.rs
$ diff -u /tmp/schema_fmt.rs crates/platform-core/src/extension/schema.rs
（无输出）
✅ 完全一致：diff 就是 rustfmt 产物
```

即：把 HEAD 版跑一遍 rustfmt，产物与工作区**逐字节相同**。

补充证据（逐项语义比对）：
- 7 处 hunk 全部是**换行位置的挪动**（1 行拆成多行 / 多行并成 1 行）；
- 3 处删掉了**尾随逗号**（`field_error(&format!(...), "must not be empty",)` → 去掉末尾 `,`）
  —— Rust 里函数调用尾随逗号**可省**，语义不变；
- 1 处把 match 分支体从**块** `{ chars.all(..) }` 改成**表达式** `chars.all(..)`
  —— 单表达式块的尾表达式值就是该表达式的值，**语义不变**。

**来源**：不明（不是后端 Dev 写的 —— 他的报告也这么说）。
**推断**：工作区里另一个会话跑了 `cargo fmt`（同一批次还波及了另外 7 个文件，见 §8.2）。
`crates/platform-core/src/extension/schema.rs` **不在**本轮 4 个缺口的范围内，
按 BA 指示「不要回退」，故保留。

---

## 5. 测试

### 5.1 环境

```bash
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
export DATABASE_URL="$DATABASE_URL_TEST"
export SQLX_OFFLINE=true
```

数据库：`podman ps` → `base-postgres Up 10 hours (healthy) 127.0.0.1:5432->5432/tcp`；
`base_test` 库 `public` schema 下 35 张表。测试**只**连 `base_test`，**未**写 `base_dev`。

### 5.2 命令与结果（**原文照抄**）

逐 crate 跑（`cargo test --workspace` 是已知技术债，不跑）。
命令形式：`cargo test -p <crate> --all-targets -- --test-threads=2 --include-ignored`
（`--include-ignored` 与本仓库 `scripts/test-integration.sh:149` 一致，
目的是让真实库用例**真跑**，而不是拿 `skip` 当通过）。

| crate | 命令 | 结果（原文） | passed 合计 |
|---|---|---|---|
| platform-core | `cargo test -p platform-core --all-targets -- --test-threads=2 --include-ignored` | `ok. 120 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s`<br>`ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`<br>`ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s` | **127** |
| platform-config | `cargo test -p platform-config --all-targets -- --test-threads=2 --include-ignored` | `ok. 38 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s`<br>`ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`<br>`ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.95s`<br>`ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s`<br>`ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.88s`<br>`ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.66s` | **83** |
| platform-iam | `cargo test -p platform-iam --all-targets -- --test-threads=2 --include-ignored` | `ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s`<br>`ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 24.86s` | **58** |
| platform-rbac | `cargo test -p platform-rbac --all-targets -- --test-threads=2 --include-ignored` | `ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`<br>`ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.19s`<br>`ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.32s`<br>`ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.47s` | **78** |
| platform-org | `cargo test -p platform-org --all-targets -- --test-threads=2 --include-ignored` | `ok. 69 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`<br>`ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.18s`<br>`ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.81s` | **91** |
| platform-db | `cargo test -p platform-db --all-targets -- --test-threads=2 --include-ignored` | `ok. 38 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`<br>`ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.41s`<br>`ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.01s`<br>`ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.64s` | **46** |
| platform-schema | `cargo test -p platform-schema --all-targets -- --test-threads=2 --include-ignored` | `ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`<br>`ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s` | **20** |
| platform-server | `cargo test -p platform-server --all-targets -- --test-threads=2 --include-ignored` | `ok. 58 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`<br>`ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`<br>`ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`<br>`ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.18s`（`i18n_no_chinese_titles`，本轮新增）<br>`ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s`（`i18n_settings_real_db`）<br>`ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`（`i18n_titles`） | **94** |
| sandbox | `cargo test -p sandbox --all-targets -- --test-threads=2 --include-ignored` | `ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`<br>`ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.63s`<br>`ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s`<br>`ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.56s`<br>`ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.12s` | **33** |
| **合计** | | **0 failed / 0 ignored** | **630** |

**`sandbox` 是本轮加跑的**（任务书的循环里没列，但它在 workspace 里且本轮改了它的 3 个文件）。

**关于 `ignored`**：不带 `--include-ignored` 时，真实库用例默认是 `#[ignore]` 的
（全仓 81 处 `#[ignore]`，属性理由写明「需要 base_test 库：scripts/test-integration.sh 会用
`--include-ignored` 跑到」）。本轮**全部用 `--include-ignored` 跑**，所以
**没有任何一个用例是被 skip 掉的**（上表 `ignored` 全为 0）。

### 5.3 CI 门禁

| 门禁 | 命令 | 结果 |
|---|---|---|
| lint | `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0**（`Finished \`dev\` profile ... in 12.97s`） |
| format | `cargo fmt --all --check` | **exit 1** —— 但**失败的全部是 HEAD 原样就有的文件**（见 §8.2），**我改的 7 个文件全部干净** |
| capabilities | `cargo xtask capabilities --check` | **exit 0**（§3） |

---

## 6. 本轮改动的文件（未提交）

### 6.1 改了（Rust 源码 / 测试）

| 文件 | 改动 |
|---|---|
| `crates/platform-schema/src/http.rs` | 缺口 1 的 2 处 title（+1 行 import）；`schema_not_found` 按 rustfmt 排版 |
| `modules/sandbox/backend/src/demo_item_http.rs` | 缺口 1 的 2 处 title（+1 行 import） |
| `modules/sandbox/backend/src/demo_note_http.rs` | 缺口 1 的 2 处 title（+1 行 import） |
| `modules/sandbox/backend/src/http.rs` | 缺口 1 的 6 处 title（+1 行 import） |
| `crates/platform-core/src/titles.rs` | `TITLES` 补 5 行 + 注释 |
| `crates/platform-db/src/error.rs` | 新增 `ALL_CODES` + `all_codes_matches_the_variants` 测试 |
| `crates/platform-db/src/lib.rs` | `mod error;` → `pub mod error;` |
| `crates/platform-server/tests/i18n_titles.rs` | `all_codes()` 接入 `platform_db::error::ALL_CODES` |
| `crates/platform-server/tests/i18n_no_chinese_titles.rs` | **新增**：静态扫描测试（6 个测试） |

### 6.2 改了（生成物，**由生成器产出，未手改**）

| 文件 | 来源 |
|---|---|
| `capabilities/index.json` | `cargo xtask capabilities` |
| `capabilities/sdk/i18n.json` | 同上 |

### 6.3 没动

- `packages/**`、`apps/**`、`e2e/**`、`playwright.config.ts` —— 前端已验收
- `AGENTS.md`
- **已应用的迁移文件**（`2026111800*.sql`、`20261119000100_*`）—— checksum 未变。
  `migrations/20261119000200_i18n_fx_rate.*`（第一轮新建的）本轮**也没碰**
  —— 两者的 mtime 分别是 18:48 / 19:59，均**早于**本轮会话开始（本轮 20:28 起），
  `git status` 显示它们仍是第一轮留下的 untracked 状态。
- 第一轮后端交付的其它文件（`crates/platform-core/src/money.rs`、`crates/platform-db/src/fx.rs`、
  `crates/platform-server/src/i18n.rs`）—— 本轮未改

---

## 7. 未做 / 做不到 / 有疑虑（逐条）

### 7.1 明确没做（超出本轮 4 个缺口）

1. **`@base/i18n` 接口成员未逐条进 `sdk/i18n.json`**。`t` / `setLocale` / `formatDate` /
   `formatMoney` / `subscribe` 是 `I18n` 接口的成员（`packages/i18n/src/index.ts:94-113`），
   生成器只抽顶层 `export`（`crates/xtask/src/capabilities.rs:731`）→ 这 5 个方法
   **不会**成为独立条目，只体现为 `I18n` 这一条。要逐条出现需改生成器去解析 interface 成员，
   超出本轮范围。**已按 BA 要求在 §3.1 说明，未硬编。**
2. **`capabilities/sdk/i18n.json:90` 里 `translate` 的 description 是分隔线**
   （`"--------------------------------------------------------------------------"`）。
   根因在前端源码的 doc 注释排版，属前端范围，本轮未改。
3. **响应体 `code` 字段的 db 码语义未改**。当前 `ConfigError::code()` 把
   `Database(_)` 折叠成 `INTERNAL_ERROR`（`crates/platform-config/src/error.rs:163`），
   db 码只出现在 `detail` 字段（`error.rs:269`）。把 db 码提到 `code` 字段会改变
   第一轮已验收的行为，超出本轮范围。
4. **`crates/platform-contracts/*`、`crates/platform-schema/src/registry.rs`、
   `modules/sandbox/backend/src/demo_item.rs` 等 8 个文件的 rustfmt 不合规未修**
   —— 详见 §8.2。
5. **未跑前端测试**（`pnpm` / `bun` / `npx`）—— 按 BA 指示，本轮只动 Rust 侧。

### 7.2 做不到

- **`cargo test --workspace`**：本仓库已知技术债，本轮按 BA 指示**逐 crate 跑**
  （`cargo test --workspace` 会失败）。
- **`platform-db` 的 `ALL_CODES` 不能「只导出 4 个」**：见 §2.2 ——
  要么 `INVALID_QUERY` 进表，要么 `ALL_CODES` 名不副实。本轮选了前者。

### 7.3 有疑虑（留给 BA 判断，本轮未自行决定）

1. **§2.1 补了 5 行而不是 BA 点的 4 行**（多 `INVALID_QUERY`）。
   理由是同通道 + 完备性测试的硬约束（§2.2）。若 BA 坚持只补 4 行，
   则 `platform_db::error::ALL_CODES` 必须排除 `INVALID_QUERY`，
   完备性测试就抓不到它 —— 请 BA 定夺。
2. **§4 的 rustfmt diff 保留未回退**。已证明是 rustfmt 产物（逐字节可复现），
   但**来源会话不明**。若那是别的需求的在制品，本轮动了它的 7 个文件里的 1 个
   （`extension/schema.rs`）—— 本轮**没有**改它，只是保留。
3. **§8.2 的 8 个文件 fmt 不合规**：`cargo fmt --all --check` 仍然 exit 1，
   **CI 的 lint job 仍会红**。这 8 个文件在 HEAD 上就不合规（不是本轮引入），
   但 CI 看到的是同一个 exit 1。**要不要在本需求内顺手修掉，请 BA 定。**
   （本轮只修了「我改过的那一行」：`crates/platform-schema/src/http.rs` 的
   `schema_not_found`。）
4. **扫描器的判据是「非 ASCII」而不是「中文」**：比 BA 的要求**更严**
   （任何非 ASCII 字面量都算命中）。本轮结果一样是 0 命中，不影响结论。

---

## 8. 附：本轮过程中的两处发现

### 8.1 扫描器第一版报出的假阳性（已收紧，但值得记录）

第一版扫描器对**所有** `title:` 位置判定，报出：

```
命中：modules/sandbox/tests/c3_real_db.rs:236 [title:] "同名记录"
```

查证：`modules/sandbox/tests/c3_real_db.rs:231-239` 是
`TenantItem { title: "同名记录" }` —— **业务实体的用户数据**，按契约 C6 应当本地化，
**不该整改**。因此把 `title:` 的判定收紧为「只在 `ProblemDetails` 的结构体字面量里」
（花括号上下文跟踪，兼容 `impl ProblemDetails` 里的 `Self { ... }`）。
收紧后新增了阴性对照 `the_scanner_ignores_business_data_titles` 守住这条边界。

### 8.2 HEAD 上就有 8 个文件 rustfmt 不合规（非本轮引入）

```
$ cargo fmt --all --check        → exit 1
受影响文件：
  crates/platform-contracts/src/checks.rs
  crates/platform-contracts/src/schema_scan.rs
  crates/platform-contracts/tests/c6_schema_projection.rs
  crates/platform-schema/src/registry.rs
  crates/platform-schema/tests/metadata_api.rs
  modules/sandbox/backend/src/demo_item_http.rs
  modules/sandbox/backend/src/demo_item.rs
  modules/sandbox/tests/demo_items.rs
```

**其中 7 个在本轮完全未改**（`git diff --quiet` → HEAD 原样），
即**在 HEAD 上就不合规** —— 与 REQ-021 无关。

第 8 个 `modules/sandbox/backend/src/demo_item_http.rs` 本轮**改过**，
但已逐块比对确认**本轮引入 0 条新违规**：HEAD 版与工作区版的 rustfmt 违规块
**数量相同（各 4 块）且内容逐字节相同**：

```
HEAD 的 fmt 违规块数: 4
现在 的 fmt 违规块数: 4
两者完全相同（我未引入新违规）: True
```

（这 4 块是：`use axum::Json;` 的 import 顺序、`match repository.list(...)` 的排版、
`Ok(None) => return ...` 的块化 —— 都与本轮改的 title 无关。）

**我改过的 7 个文件的 fmt 状态**：

```
✓ crates/platform-schema/src/http.rs（fmt 干净）
✓ crates/platform-server/tests/i18n_no_chinese_titles.rs（fmt 干净）
✓ crates/platform-db/src/error.rs（fmt 干净）
✓ crates/platform-db/src/lib.rs（fmt 干净）
✓ crates/platform-server/tests/i18n_titles.rs（fmt 干净）
✓ modules/sandbox/backend/src/http.rs（fmt 干净）
✓ modules/sandbox/backend/src/demo_note_http.rs（fmt 干净）
```

---

## 9. 复现命令速查

```bash
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
export DATABASE_URL="$DATABASE_URL_TEST"
export SQLX_OFFLINE=true
cd /home/myg133/Projects/system_seed/feature-REQ-021

# 缺口 1：静态扫描（含阳性对照的实际输出）
cargo test -p platform-server --all-targets --test i18n_no_chinese_titles -- --nocapture

# 缺口 2：完备性（含「TITLES = N 行；…」那行原文）
cargo test -p platform-server --all-targets --test i18n_titles -- --nocapture

# 缺口 3：capabilities
cargo xtask capabilities
cargo xtask capabilities --check

# 缺口 4：证明 diff 是 rustfmt 产物
git show HEAD:crates/platform-core/src/extension/schema.rs > /tmp/s.rs
cp /tmp/s.rs /tmp/s_fmt.rs && rustfmt --edition 2024 /tmp/s_fmt.rs
diff -u /tmp/s_fmt.rs crates/platform-core/src/extension/schema.rs   # 无输出

# 全部 Rust 测试
for c in platform-core platform-config platform-iam platform-rbac platform-org \
         platform-db platform-schema platform-server sandbox; do
  cargo test -p $c --all-targets -- --test-threads=2 --include-ignored
done

# CI 门禁
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```
