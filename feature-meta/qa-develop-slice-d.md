# develop 切片 D（装配、契约与应用层）独立 QA

基线：`develop` @ `164e32a`；工作目录 `/home/myg133/Projects/system_seed/code`。
范围：`platform-server` / `platform-app` / `platform-config` / `platform-schema` / `platform-contracts` 的 `src/`（约 16600 行，实测 16,669 行）。
独立 QA，未参与实现。

## 结论

**装配没有漏挂**：`main.rs` 的 18 个挂载点逐个核对，路径拼写全部正确（无 `/api/notfy` 类笔误），每个都有对应的 `router()` 实现且 state 已注入。中间件执行顺序与设计一致（实测以 axum `layer` 后挂在外层为前提推导，并读到作者自述两次踩坑记录）。

**但「装配正确性无判据」（TD-076）在本切片范围内实测仍然成立**——这是本次最重的结论：把 `/api/notify` 整个 `.nest()` 删掉后，`platform-server` 全部 **136 个测试 0 失败**。任何 nest 被误删都不会有人发现。这是 MAJOR 而非 BLOCKER，因为当前装配本身是对的（没有漏挂），缺陷在于「没有护栏」而非「护栏内是坏的」。

另有一条 MINOR（配置写入审计 before 值静默丢失）和一条 MINOR（`main.rs:794` 的顺序注释漏了 `deprecation` 一整层）。

> ⚠️ 环境说明：验证期间本工作区有**并行会话**在跑变异实验（`openapi.json` 出现 `/api/fake/mutation-endpoint`、`crates/platform-iam/tests/iam_real_db.rs` 出现 `【QA 临时实验】` 块，均非本会话所为）。本会话的两处临时变异已 100% 还原，`git status` 中属于本切片的文件全干净。

## FINDING 清单

| # | 严重级 | 位置(file:line) | 问题 | 证据 |
|---|---|---|---|---|
| F1 | MAJOR | `crates/platform-server/src/main.rs:675`（删除点）+ `crates/platform-server/tests/` 全体 | 删除 `/api/notify` 整条 nest 后 136 个测试全绿 → 装配完整性无任何判据，TD-076 未修 | 变异实验实测（见详情） |
| F2 | MINOR | `crates/platform-config/src/service.rs:192-194` | `layer_values` 读库失败时 `let Ok(..) else { return out }` 静默返回空 map，不打日志；审计 before 值变成 `{}` | 代码 + 调用点 `service.rs:450`、`service.rs:535` |
| F3 | MINOR | `crates/platform-server/src/main.rs:794` | 中间件顺序注释写「`client_info` → `trace_id` → `rate_limit`」，**漏掉 `deprecation` 一整层**；同仓另两处（`http.rs:443`、`http.rs:492`）都是完整四层 | 三处注释对照 |

## 逐条详情

### F1 装配完整性没有判据（实测确认，非推断）

**做法**：把 `main.rs:675` 的
```rust
let app = app.nest("/api/notify", platform_notify::http::router(notify_state));
```
改成 `let app = app;`（notify 子系统整条路由消失），然后跑
`cargo test -p platform-server --all-targets -- --test-threads=1`。

**结果**：
```text
70 passed; 8 passed; 5 ignored; 5 passed; 1 passed; 17 passed;
5 passed; 6 passed; 3 passed; 9 passed; 2 passed; 1 passed; 4 passed
→ 0 failed
```

**含义**：`platform-server/tests/` 下 12 个测试文件（`app_real_stack.rs`、`audit_chain_real_db.rs`、`i18n_*.rs`、`oauth_real_stack.rs`、`sso_*.rs` …）**没有一个**是对 `main.rs` 装配结果的断言。它们各自构造自己的 router（`app_real_stack.rs` 测的是 `platform_app::http::router`，不是 `main.rs` 里那条 nest）。

**为什么是 MAJOR 不是 BLOCKER**：当前 18 个挂载点实际全部正确（见下方清单），所以线上功能是好的。缺陷是「将来任何人删错一行都不会红」。

**唯一测得住的装配**是中间件层（`platform-observability/tests/middleware_assembly.rs`），它调 `apply_all_middlewares_with` 这个**与 main 共用的函数**（TD-075 的处置）。nest 层没有等价的「与 main 共用」结构 —— 那正是 TD-076 描述的问题。

**还原**：已 `cp` 回原文件，`grep MUTATION` 无残留，`git status` 该文件干净。

### F2 配置写入审计的 before 值静默丢失

`crates/platform-config/src/service.rs:192`：
```rust
let Ok(layers) = self.store.load(tenant, space).await else {
    return out;   // out 是空 Map，直接返回
};
```
`out` 初始为 `Map::new()`（`service.rs:189`）。库读失败（连接断、超时、RLS 错误）→ 返回空 map，**没有 WARN/ERROR**。

两个调用点都把它直接塞进审计事件：
- `service.rs:450` → `config.entries.write` 的 `.with_before(Value::Object(before))`
- `service.rs:535` → `config.flag.write` 的 `.with_before(Value::Object(before))`

**影响**：审计记录里 before 变成 `{}`。事后审计时无法区分「该键写入前不存在」与「读 before 值时数据库刚好不可用」——前者是正常语义，后者是证据缺失。且因为是 `else` 直接 return，`load` 的错误对象被**完全丢弃**（连 `?` 都不是），排障时日志里没有痕迹。

对照同文件 `record_audit`（`service.rs:167-173`）对 sink 失败会打 `tracing::error!`，说明作者在别处是有「失败要出声」意识的 —— 这里遗漏了。

**不是 BLOCKER 的理由**：写入本身会成功（`write_scoped` 在 `layer_values` 之后，会正常 `?` 传播自己的错误），只是审计 before 值质量下降。

### F3 顺序注释漏了一整层

`crates/platform-server/src/main.rs:794`：
```rust
// 顺序（不可换）：`client_info` → `trace_id` → `rate_limit` → 业务。
```
实际执行链是四层。同仓另外两处写的是完整的：
- `crates/platform-observability/src/http.rs:443`：`// 目标执行：client_info → deprecation → trace_id → rate_limit → 业务`
- `crates/platform-observability/src/http.rs:492`：`// 实际执行顺序：client_info → deprecation → trace_id → rate_limit → 业务`

三处只有 `main.rs:794` 这一处漏了 `deprecation`。代码本身是对的（`http.rs:505` 确实调了 `apply_deprecation_router`），纯注释错误。按项目「注释与代码必须一致」的硬规矩记 MINOR。

## main.rs 装配清单核对

逐行核对 `crates/platform-server/src/main.rs`，18 个挂载点（行号为实际代码行）：

| # | 行 | 方法 | 路径 / 路由 | 路由实现存在 | state 注入 | 结论 |
|---|---|---|---|---|---|---|
| 1 | 354 | `.route` | `/health/live` | 本文件 `live()` L2027 | `AppState` | ✅ |
| 2 | 355 | `.route` | `/health/ready` | 本文件 `ready()` L2037 | `AppState` | ✅ |
| 3 | 412 | `.nest` | `/api/iam` | `platform_iam::http::router`（`http.rs:367`） | `IamState` | ✅ |
| 4 | 413 | `.merge` | `/.well-known/jwks.json`（绝对路径，故用 merge 正确） | `jwks_router`（`http.rs:392`） | 同上 | ✅ |
| 5 | 415-418 | `.nest` | `/api/iam`（第二次，挂 `/sso/*`） | `sso_router`（`http.rs:413`） | 同上 | ✅ 子路径 `/sso/*` 与 #3 的 `/login` 等**不重叠**，axum 0.8 合并为同一前缀两组路由，非覆盖 |
| 6 | 432 | `.nest` | `/api/org` | `platform_org::http::router`（`http.rs:281`） | `OrgState` | ✅ 在 IAM 分支内（设计如此，fail closed） |
| 7 | 441 | `.nest` | `/api/rbac` | `platform_rbac::http::router`（`http.rs:445`） | `RbacState` | ✅ |
| 8 | 494 | `.nest` | `/api/audit` | `platform_server::audit::router`（`audit.rs:260`） | `AuditState` | ✅ |
| 9 | 540 | `.nest` | `/api/schema` | `platform_schema::http::router`（`http.rs:108`） | `SchemaState` | ✅ |
| 10 | 572 | `.nest` | `/api/sandbox` | `sandbox::mount(...)` | 模块内 + `ModuleAuthState` 层 | ✅ `sandbox` 确在 `modules/sandbox/backend/src/mod.rs:100` 注册 |
| 11 | 591 | `.fallback` | `static_fallback` | L2126 | +`.with_state(state)` | ✅ |
| 12 | 607 | `.merge` | `/api/config/*` + `/api/feature-flags/*`（常量均为绝对路径 `http.rs:51-57`，故 merge 正确） | `platform_config::http::router` | `ConfigState` | ✅ |
| 13 | 629-632 | `.nest` | `/api/oauth` | `platform_oauth::http::router`（`http.rs:224`） | `OAuthState` | ✅ |
| 14 | 633 | `.merge` | `/.well-known/oauth-authorization-server` | `metadata_router`（`http.rs:314`） | 同上 | ✅ |
| 15 | 657 | `.nest` | `/api/apps` | `platform_app::http::router`（`http.rs:145`） | `AppState` | ✅ |
| 16 | 675 | `.nest` | `/api/notify` | `platform_notify::http::router`（`http.rs:159`） | `NotifyState` | ✅ |
| 17 | 702-705 | `.nest` | `/api/storage` | `platform_storage::http::router`（`http.rs:145`） | `StorageState` | ✅ |
| 18 | 788 | `.merge` | `/api/i18n/context` + `/api/i18n/fx/convert`（绝对路径常量 `i18n.rs:96,98`） | `i18n::router`（`i18n.rs:745`） | `I18nState` | ✅ |
| 19 | 818-821 | `.nest` | `/api/observability` | `observability::router`（重导出 `observability.rs:9` → `platform_observability::http::router`） | `StatusState` | ✅ |

**nest / merge 的用法是否正确（关键区分）**：
- 用 `.nest` 的 11 处，其内部路由全是**相对路径**（如 `platform_app` 的 `/workspace`、`platform_schema` 的 `/{module_id}/{entity_key}`）→ 正确。
- 用 `.merge` 的 5 处，其内部全是**绝对路径**（`/api/config/entries`、`/.well-known/jwks.json`、`/api/i18n/context`）→ 正确。若把绝对路径的 router 用 `.nest` 挂，会变成 `/api/config/api/config/entries` —— 没有踩这个坑。

**模块装配点**（`main.rs:549-589`）：
- `discover_enabled()` 走 `inventory` 链接期注册；全仓仅 `platform-org`（`lib.rs:111`）与 `sandbox`（`backend/src/mod.rs:100`）注册，二者 `enabled()` 均返回 `true`。
- `"sandbox"` → L572 挂载 ✅
- `"org"` → L581 显式跳过（由 L432 单独装配）✅；IAM 未配置时 L525-527 的 warn 已明确覆盖「不提供 /api/org/*」，**不构成静默**
- `other` → L582-587 出声告警 ✅

**路径拼写**：逐个核对，无 `/api/notfy`、无 `/api/aps` 之类笔误。

**中间件顺序**（`main.rs:825` → `platform_observability::http::apply_all_middlewares_with`）：
- 前提核实：axum `layer` 后挂在外层 —— 作者在 `http.rs:490` 与 `lib.rs:162` 两处都写明了这一点，且 `lib.rs` 的注释记录了「反着写会让限流先跑、读不到租户」的踩坑。我按此前提逐行推导挂载序列：`apply_observability`（内挂 `rate_limit` 再挂 `trace_id`）→ `apply_deprecation_router` → 最后 `.layer(client_info)`。
- 推导结果：执行序 `client_info`（最外）→ `deprecation` → `trace_id` → `rate_limit`（最内）→ 业务，**与设计一致**。
- 注意：**这条顺序没有任何测试判据**（见「未验证项」）。

## 已查确认无问题的项

- ✅ **已查：`/health/ready` 的 503 语义，无问题。** `main.rs:2064` 判定 `readiness.is_ready() && rate_limit_outcome.is_ok()`；不成立则走两段 503 返回（限流不可达 L2098、DB/迁移问题 L2122）。`health::check_readiness`（`health.rs:47`）在 `SELECT 1` 失败时返回 `CODE_DB_UNAVAILABLE`，在 `_sqlx_migrations` 读不出时返回 `MIGRATIONS_PENDING`，dirty 时返回 `MIGRATION_DIRTY` —— 三个错误路径都 `is_ready()==false`。
- ✅ **已查：「关闭限流不该因限流表不就绪」，语义成立。** `rate_limit_readiness`（`http.rs:255`）先 `state.as_ref()?`（未装配→`None`），再读 `settings.enabled`，为 `false` 时返回 `None`（`http.rs:262-264`）；`readiness_rate_limit_outcome(None)` → `Ok(None)`（`http.rs:306`）→ 不参与判定。**不会**因为限流表问题 503。
- ✅ **已查：中间件顺序前提（axum 后挂在外层），成立。** 见上方推导；且 `middleware_assembly` 的 `the_client_info_runs_outermost_so_downstream_sees_the_ip` 用例实测通过，证明 client_info 确在最外。
- ✅ **已查：配置三层覆盖优先级，成立。** `resolver.rs:236-275` 的 `resolve`：空间级（且仅限 `ConfigScope::Space` + 品牌白名单）→ 租户 → 平台 → 默认值，逐层 `if let Some`。`resolve_platform`（`:155`）只吃 platform 层。`flag()`（`:277-347`）同为租户 → 平台 → 默认。
- ✅ **已查：缓存失效不是 `max(epoch)`，无问题。** `resolver.rs:117-135` 用**逐一相等**比对 `platform_epoch` 与 `tenant_epoch`，且 epoch 不等时**强制重算**（`:137`）；TTL 只是兜底上限，不参与正确性。
- ✅ **已查：「读不到配置就静默用默认值」在 **读路径** 不存在。** 各读入口（`read_entries:249`、`read_platform_entries:294`、`read_flag:329`、`read_flags:336`、`branding`）都 `refresh(ctx).await?` —— 库错**向上传播**，不吞。未登记键返回 404（`:228-231`），平台级键走错路由返回 403（`:243-247`）。**唯一例外**是 F2 的审计 before 路径。
- ✅ **已查：启动失败路径不泄露数据库连接串/密码（实测，非推断）。** `main.rs` 多处 `format!("数据库连接串非法：{err}")`。我实测 sqlx 0.8：
  - `PgConnectOptions::from_str("postgres://base:sup3rs3cr3t@127.0.0.1:notaport/base")` → `Err("error with configuration: invalid port number")`
  - `("postgres://base:sup3rs3cr3t@/base")` → `Err("... empty host")`
  - 真实连接失败 → `"pool timed out while waiting for an open connection"`
  **三种情况都不回显密码或连接串原文。** `isolation.rs:83/95`、`load_migrator`（`main.rs:292`）同理。
- ✅ **已查：启动失败不留半初始化状态。** 所有 `?` 都在 `axum::serve`（`main.rs:842`）之前；`listener` 绑定（`:832`）也在最后。任一失败直接 `ExitCode` 退出，不会出现「已监听但子系统缺一半」。
- ✅ **已查：`platform-contracts` 无死类型。** `ContractCheck` / `ContractSuite` / `SuiteReport` / `ContractWarning` 虽外部零引用，但 crate 内构成完整套件机制（`suite.rs:52/67/73`，含单元测试），是给下游模块的扩展点，不是死代码。`CheckOutcome` 是全部 `check_cN` 的返回类型。`check_c6_for` 外部零引用属预留导出口，非 `pub struct` 级死类型。
- ✅ **已查：序列化命名约定一致（camelCase）。** 扫描 5 个 crate 所有含 `Serialize` 的 `pub struct`，无「有 serde 但缺 `rename_all`」的情形。唯一命中 `platform-schema/src/http.rs:93` 的 `EntityMetadata`，其字段 `module`/`entity`/`label`/`api`/`fields` **全是单词**，camelCase 与 snake_case 无差异，不构成违规。其嵌套类型 `FieldSchema`（`core/extension/schema.rs:154`）与 `EntityApi`（`:118`）均正确标注 `rename_all = "camelCase"` 并对多词字段用 `skip_serializing_if`。
- ✅ **已查：`/api/app` 逐端点鉴权挂载正确。** `platform_app/src/http.rs:148-177` 每个 CRUD 路由用 `.route_layer(authz::require_*())` **绑在该路由上**，不是整表 layer；`/workspace`、`/recent`、`/navigation` 不加权限点是 D271 的明确设计（登录即可）。
- ✅ **已查：TD-077（限流恒不生效）已真修。** `build_rate_limit_state`（`main.rs:1279-1296`）把 `IamService` 包成 `TokenTenantResolverImpl` 注入 `RateLimitState.token_resolver`；中间件 `middleware.rs:164-187` 在 `ctx.tenant_id()` 为空时回退用令牌解析租户。未注入时 `main.rs:1283-1288` 会 warn。
- ✅ **已查：`rate_limit` 的 `OnceLock` 注入时序正确。** `main.rs:809` `rate_limit_slot.set(...)` 早于 `:832` 绑定 listener，任何请求都晚于注入，`/health/ready` 不会读到未初始化槽。

## 未验证项

1. **中间件「相对顺序」没有判据（本次第二重要的观察）。** 我实测变异：把 `lib.rs:177-187` 里 `rate_limit` 与 `trace_id` 的挂载顺序**对调**，`cargo test -p platform-observability --test middleware_assembly` → **11 个测试全绿**。原因：现有用例断言的是「装上了没有」（`the_trace_middleware_is_installed` 查响应头、`the_rate_limit_middleware_is_installed` 查 429）与「client_info 在最外」，**没有一条断言 `trace_id` 在 `rate_limit` 之前**。文件头 `middleware_assembly.rs:19-21` 声称「第 4 条最重要：顺序错了…」但实际只覆盖了 client_info 这一对。
   - 已还原（`git status` 干净）。
   - 这解释了为什么 §「中间件顺序一致」的结论只能基于**代码推导**（含作者自述的踩坑记录），而非测试判据。
   - 归为「未验证项」而非独立 FINDING：代码当前是对的，且与 F1 同源（都是「装配无判据」），我不重复计数。
2. **`apply_deprecation_router` 与 `apply_observability` 的交换无影响** —— 我推断 `deprecation` 只读 URI 贴响应头、不依赖 trace 上下文，但**未实测**该变异的测试反应（因 F1 已足够说明判据缺口，未再消耗编译时间）。
3. **IamConfig 未配置时 `/api/org` 之外**，`platform-org` 模块 enabled=true 但仍会被 `discover_enabled()` 枚举后静默跳过 —— 我已确认 L525 的 warn 覆盖此语义，但**未在真实无-IAM 部署下起服务验证日志文本**。
4. **未跑 `platform-app` / `platform-schema` / `platform-contracts` 的独立测试套件**（时间/线程预算优先给了 `platform-server` 与 `platform-config`，这两个覆盖了装配与配置主路径）。这三个 crate 的结论来自代码阅读。
5. **`openapi.json` / `iam_real_db.rs` 的幽灵改动**归属于并行会话，我只做了归因，**未深究其变异内容**（不在切片 D 范围）。

## 测试执行记录

| 命令 | 结果 |
|---|---|
| `cargo test -p platform-server --all-targets -- --test-threads=1` | **全通过**（136 用例，0 失败，5 ignored） |
| 同上，注入 F1 变异（删 notify nest） | **仍全通过**（0 失败）← 核心证据 |
| `cargo test -p platform-config --all-targets -- --test-threads=1` | **全通过**（88 用例，0 失败，1 ignored*） |
| `cargo test -p platform-observability --test middleware_assembly`（顺序对调变异） | **仍全通过**（11 用例）← 顺序判据缺口证据 |

\* 那个 ignored 是 `config_real_db.rs:1043` 的 `rollout_child_process_helper`，由父测试 `rollout_across_two_processes_is_identical` 通过子进程显式调用，不是漏测。

**变异还原确认**：两处临时变异均已还原，`grep -rn MUTATION crates/` 无结果；`git status` 中 `platform-server/src/main.rs`、`platform-observability/src/{http,lib}.rs` 均干净。
