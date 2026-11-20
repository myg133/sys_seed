# REQ-014 后端接入自验报告（棒 2）

- **角色**：后端 Dev（子 Agent，由 BA 派单）
- **worktree**：`feature-REQ-014`（分支 `feature/REQ-014`，基线 `develop@0683115`）
- **规格版本**：`BA/demands/REQ-014-audit/acceptance.md` **rev 3**（13 条 AC）
  + `decisions.md` **rev 2**（D130~D168，含 BA 后补的 **D171~D180**）
  + `design-summary.md` **rev 2** + 契约 `api-conventions v1`（C6 / C7 / C9 / C12 / C13）
- **本轮范围**：**棒 2 —— 接入与接口（后端）**：D171 链尖、D172 驼峰拆词、业务模块埋点、
  sink 装配（D150）、`/api/audit` 路由、全局中间件（D149）、xtask、D178 配置登记，
  外加 BA 后补的 **D180**（静态回落的 API 前缀）。
- **未提交 git**（按 BA 统一提交的约定）

> ⚠️ **本 worktree 里同时有前端棒的产物**（`apps/shell/**`、`packages/i18n/**`、
> `scripts/seed-e2e.sh`、`e2e/audit.spec.ts`、`capabilities/**` 等）——
> **那些不是本轮 Dev 的改动**，本报告不认领。
> ⚠️ 本轮**只**改 `crates/**` 与 `migrations/**`（+ 本报告与验证报告的追加）。

---

## 1. 交付清单（文件 → AC）

> **本棒实际改动 = 37 个文件**（另有前端棒的并行产物，本报告不认领）：
> `crates/platform-core/src/extension/audit.rs` = 1；
> `crates/platform-iam/src/{audit.rs,error.rs,service.rs,lib.rs}` + `src/sso/{audit.rs,bridge.rs,mod.rs}`
> + `tests/audit_events.rs` = 8；
> `crates/platform-rbac/src/service.rs` + `tests/audit_events.rs` = 2；
> `crates/platform-config/{manifest.json,src/service.rs}` + `tests/audit_events.rs` = 3；
> `crates/platform-org/src/audit.rs` = 1；
> `crates/platform-server/{Cargo.toml,src/audit.rs,src/client_info.rs,src/lib.rs,src/main.rs,src/static_files.rs}` = 6；
> `crates/platform-audit/src/{archive.rs,lib.rs,query.rs,verify.rs}` + `tests/audit_real_db.rs` = 5；
> `crates/xtask/{Cargo.toml,src/audit.rs,src/cli.rs,src/lib.rs,src/main.rs}` = 5；
> `migrations/20261120000200_audit.{up,down}.sql` + `migrations/20261120000400_audit_config_seed.{up,down}.sql` = 4；
> `feature-meta/{req-014-dev-backend-integration.md,verification-report.md}` = 2。
> **未超过 45 的红线**（预期 22~32 → 实到 **37**；超出是因为本棒补了
> **3 个新的真库测试文件**（`platform-iam` / `platform-rbac` / `platform-config` 的 `tests/audit_events.rs`）
> 与 **xtask 的 5 个文件**）。

### 1.1 D171 —— 链尖（`tip_hash`）+ 末尾截断检出

| 文件 | 改动 |
|---|---|
| `migrations/20261120000200_audit.up.sql` | `platform_audit_chain_state` 加 `tip_hash TEXT NOT NULL DEFAULT repeat('0',64)`；`platform_audit_append` 写入时更新 `tip_hash`；`platform_audit_archive_move` 归档时**重算** `tip_hash`（在线表被搬空时 = `v_tail_hash`）；新增 `platform_audit_tip_hash(TEXT)`（`SECURITY DEFINER` + 租户守卫，**无链状态记录时返回 `NULL`**） |
| `migrations/20261120000200_audit.down.sql` | 重写：补 `DROP FUNCTION platform_audit_tip_hash` 与 `platform_audit_normalize_key` |
| `crates/platform-audit/src/verify.rs` | 新增 `TipHash { value: Option<String> }`；末尾截断检测：在线表最后一条的 `hash` 必须 == `tip_hash`，否则记一条 `hash_mismatch`；在线表为空时用 `head_hash` 兜底；`tip` 为 `None` 时**跳过**判定 |
| `crates/platform-audit/tests/audit_real_db.rs` | 新增 `a_truncated_online_tail_is_detected_after_the_archive` + 辅助 `chain_state_tip()` |

> ⚠️ **迁移顺序坑（已踩并修）**：`ALTER TABLE … ADD COLUMN IF NOT EXISTS` 必须排在
> `COMMENT ON COLUMN … tip_hash` **之前** —— 否则表已存在时整条迁移失败
> （`CREATE TABLE IF NOT EXISTS` 是空操作 → 列不存在）。
> 迁移文件改动后必须让 sqlx migrator 重跑：
> `DELETE FROM _sqlx_migrations WHERE version IN (20261120000200, 20261120000300);`

### 1.2 D172 —— 敏感键归一化追加「驼峰拆词」

| 文件 | 改动 |
|---|---|
| `crates/platform-core/src/extension/audit.rs` | `normalize_sensitive_key` 改为 4 步（缩写边界 `XYz`→`X_Yz` → 小写/数字与随后大写插 `_` → 全小写 → `-`→`_`），**只认 ASCII**；新增 `AuditSinkSlot`（OnceLock + Clone + 自定义 Debug）；3 个新单测 |
| `migrations/20261120000200_audit.up.sql` | 新增 `platform_audit_normalize_key(TEXT)`（plpgsql，用 `ascii()` 区间判断避免排序规则影响）；`platform_audit_sensitive_keys` 改用该函数 |
| `crates/platform-audit/tests/audit_real_db.rs` | 新增 `the_db_and_rust_normalization_rules_are_identical`（18 个样例键逐字比对 + 阳性/阴性对照） |

> **键集合仍是 14 个**（D156 基线不变）—— 变的是**归一化规则**，不是键的数量。

### 1.3 业务模块埋点（事件码与形状**照抄**棒 1 报告 §7 / D163）

| 文件 | 改动 |
|---|---|
| `crates/platform-iam/src/audit.rs`（新） | `login_event()` 构造器（**结构上无法**产出含账号名的失败事件）+ 4 个事件码常量 + 3 个单测 |
| `crates/platform-iam/src/sso/bridge.rs`（新） | `PlatformSinkSsoRecorder` 实现既有 `SsoAuditRecorder`（**签名不变**）；**只**桥接 `iam.sso.link.created`；`translate()` 返回 `Option`（缺租户 → 跳过 + warn）；2 个单测 |
| `crates/platform-iam/src/sso/audit.rs` | `SsoAuditPayload` 加 `pub tenant_id: Option<String>` + `with_tenant()`（**不序列化** → 白名单不变） |
| `crates/platform-iam/src/service.rs` | `IamService` 加 `audit: AuditSinkSlot` + `with_audit_sink()` + `record_audit()`；`login` 三条失败路径各写 1 条 `iam.login.failed`、成功写 `iam.login.succeeded`；`login_via_ldap` 写 `iam.login.*`；`complete_oidc_callback` 写 `iam.sso.login.*` |
| `crates/platform-iam/src/error.rs` | 新增 `code::AUDIT_SINK_UNAVAILABLE` + `IamError::AuditUnavailable { reason }`（**503**） |
| `crates/platform-rbac/src/service.rs` | `RbacService` 加 `audit: AuditSinkSlot` + `with_audit_sink()` + `record_audit()` + `role_permission_codes()`（**排序后**的直接授权）；create / delete / grant / revoke 四处埋点 |
| `crates/platform-config/src/service.rs` | `ConfigService` 加 `audit: AuditSinkSlot` + `with_audit_sink()` + `record_audit()` + `layer_values()`（读**被写入那一层**的当前值）；`write_entries` → `config.entries.write`；`write_flag` → `config.flag.write` |
| `crates/platform-org/src/audit.rs` | 新增 `PlatformSinkAuditRecorder`（实现既有 `AuditRecorder`，**签名不变**）+ `translate()` |

### 1.4 sink 装配（D150）+ `/api/audit` 路由 + 全局中间件（D149）

| 文件 | 改动 |
|---|---|
| `crates/platform-server/src/audit.rs`（新） | `AuditState`、本地 `TokenVerifier` / `IamTokenVerifier`、`ReqCtx` 提取器、`authenticate` 中间件（验令牌 → 建 ctx → 刷新判定缓存 → 注入 checker）、4 个处理器、camelCase 响应视图、错误响应（`AUDIT_INVALID_FILTER` / `AUDIT_EVENT_NOT_FOUND` 等） |
| `crates/platform-server/src/client_info.rs`（新） | 全局中间件 `client_info()`：读 `x-forwarded-for` **第一跳** + `user-agent` 填进 `RequestContext`；3 个单测 |
| `crates/platform-server/src/main.rs` | 构造 `audit_db` + `audit_sink`；注入 iam / rbac / config / org / sso-recorder；`nest("/api/audit", …)`；顶层 `.layer(from_fn(client_info))` |
| `crates/platform-server/src/lib.rs` | 新增 `pub mod audit; pub mod client_info;` |
| `crates/platform-server/Cargo.toml` | 加 `platform-audit.workspace = true` + `time.workspace = true`（**零新增第三方依赖**） |

### 1.5 xtask（D158 / D174）

| 文件 | 改动 |
|---|---|
| `crates/xtask/src/audit.rs`（新） | `verify_tenant()` / `list_tenants()` / `archive_tenant()` / `summarize()` / `has_failures()` / `require_tenant()` / `require_positive_days()` |
| `crates/xtask/src/{cli.rs,main.rs,lib.rs}` | `Command::Audit` + `AuditAction::{Verify,Archive}` + `run_audit()` + `database_url_of()` |
| `crates/xtask/Cargo.toml` | 加 `platform-audit.workspace = true` + `sqlx.workspace = true`（**都在树内**，不增长依赖树） |

### 1.6 D178 —— 4 个配置项登记

| 文件 | 改动 |
|---|---|
| `crates/platform-config/manifest.json` | 加 4 个配置项（**camelCase**）：`audit.retention.onlineDays` 90 / `audit.retention.archiveDays` 365 / `audit.page.defaultSize` 50 / `audit.page.maxSize` 200（`scope = platform`） |
| `migrations/20261120000400_audit_config_seed.up.sql`（新） | 4 次 `platform_config_upsert(...)`；注释里有 snake_case ↔ camelCase 对照表与冲突说明 |
| `migrations/20261120000400_audit_config_seed.down.sql`（新） | `DELETE` 那 4 行 |

### 1.7 D180（BA 后补）—— 静态回落跳过 `/api/` 前缀

| 文件 | 改动 |
|---|---|
| `crates/platform-server/src/static_files.rs` | 新增 `is_api_path()`（`/api` 或 `/api/**`）+ `api_not_found()`（ProblemDetails 404，走平台统一 `problem_response`）；**只改 API 前缀的回落分支**，静态资源回落一字未动；2 个新测试 |

---

## 2. 逐 crate 测试数字（真跑出来的）

跑法（dev-guide §9.8：`--all-targets` 不可省、只有一个 `--`、`--include-ignored` 在 `--` 之后）：

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
cargo test -p <crate> --all-targets --no-fail-fast -- --test-threads=1 --include-ignored
```

数字统计口径：`grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8}'`（该 crate **所有** test target 求和）。

| crate | passed | failed | ignored | 棒 1 基线 | 变化 | 备注 |
|---|---|---|---|---|---|---|
| `platform-core` | **153** | 0 | 0 | 150 | **+3** | `extension/audit.rs` 新增 3 个单测（D172 的驼峰拆词） |
| `platform-audit` | **46** | 0 | 0 | 44 | **+2** | `audit_real_db` 13（+2：D171 截断、D172 两侧一致） |
| `platform-iam` | **178** | 0 | 0 | 168 | **+10** | lib **+6**（`audit.rs` 3 + `sso/bridge.rs` 3）；**新增** `tests/audit_events.rs` **+4** |
| `platform-rbac` | **83** | 0 | 0 | 78 | **+5** | **新增** `tests/audit_events.rs` 5 条（create / grant / revoke / delete / 排序） |
| `platform-config` | **88** | 0 | 0 | 85 | **+3** | **新增** `tests/audit_events.rs` 3 条 |
| `platform-org` | **93** | 0 | 0 | 91 | **+2** | `audit.rs` 模块内 +2（桥接转发 / 失败报错） |
| `platform-server` | **121** | 0 | 0 | 112 | **+9** | `audit.rs` **4**（2 视图/令牌 + **2 端到端接线**）+ `client_info.rs` 3 + `static_files.rs` 2 |
| `platform-db` | 46 | 0 | 0 | 46 | 0 | 未改代码，回归 |
| `platform-schema` | 20 | 0 | 0 | 20 | 0 | 未改代码，回归 |
| `xtask` | **51** | 0 | 0 | （棒 1 未纳入统计） | — | 棒 1 的合计里**没有** xtask |
| `sandbox` | 33 | 0 | 0 | 33 | 0 | 未改代码，回归 |
| **合计** | **912** | **0** | **0** | 827（10 crate） | **+85** | 剔除棒 1 没统计的 xtask（51）后：**861**，即 **+34** |

> 全部 `failed = 0`、`ignored = 0` —— `--include-ignored` 让真实库用例**真的跑了**，不是跳过。

### 2.1 真实库验证（`base_test`）

- **3 张表**：`platform_audit_log` / `platform_audit_archive` / `platform_audit_chain_state`（`\d` 确认列集合）。
- **7 个函数**：`platform_audit_append` / `platform_audit_archive_move` / `platform_audit_canonical_hash` /
  `platform_audit_head_hash` / `platform_audit_tip_hash` / `platform_audit_normalize_key` /
  `platform_audit_sensitive_keys`（`pg_proc` 确认全部存在，`SECURITY DEFINER` 标记正确）。
- **`tip_hash` 列**：`information_schema.columns` 确认 `NOT NULL DEFAULT repeat('0'::text, 64)`。
- **迁移 `20261120000400` 已在 `base_test` 应用**：`_sqlx_migrations` 里 `20261120000400 | audit config seed`；
  `platform_config` 里 4 行 `audit.*` 全部就位：
  `audit.retention.onlineDays=90` / `audit.retention.archiveDays=365` /
  `audit.page.defaultSize=50` / `audit.page.maxSize=200`。
  ⚠️ **一处如实记录的交互**：`platform-config` 的**测试夹具** `reset_fixture()` 会
  `DELETE FROM platform_config`（**整表**）再回填自己的夹具 → 跑完 config 真实库用例后，
  这 4 行**在测试库里**会被清掉（**生产不会**：生产只经 `platform_config_upsert` 改值，从不删行；
  且 manifest 的默认值仍会兜底解析）。**迁移是幂等的**（`platform_config_upsert`）→
  删掉 `_sqlx_migrations` 里 `20261120000400` 的记录重跑即可复原（**已实测复原**）。
- `platform-audit` 的 **13 条真实库用例全绿**（含 D171 截断、D172 两侧一致、append-only、跨租不可见）。

### 2.2 xtask 实测（真实库 `base_test`）

```text
$ DATABASE_URL=postgres://base:base@127.0.0.1:5432/base_test cargo xtask audit verify --tenant tenant-a
  tenant-a：通过（检查 2 条，断裂 0 条）
审计链校验通过：1 个租户，共 2 条记录                                   # exit=0

$ cargo xtask audit verify                       # 不带 --tenant → 列出全部租户
  tenant-a：通过（检查 2 条，断裂 0 条）
  tenant-b：通过（检查 1 条，断裂 0 条）
审计链校验通过：2 个租户，共 3 条记录                                   # exit=0

$ # 篡改 tenant-a 的一条记录后重跑
  tenant-a：失败（检查 2 条，断裂 1 条）
审计链校验失败：tenant-a（1 条断裂）                                     # exit=1  ← 篡改被检出

$ cargo xtask audit archive --tenant tenant-b --days 90
已归档租户 tenant-b 的超期记录：1 条（在线保留 90 天）                    # exit=0
  → 在线表 tenant-b 剩 1 条（新记录），归档表 1 条（100 天前那条）✓

$ cargo xtask audit archive --tenant "" --days 90
错误：缺少 --tenant：审计操作是**单租户**的（D158）                        # exit=1
$ cargo xtask audit archive --tenant tenant-b --days 0
错误：--days 必须是正整数（当前：0）                                      # exit=1
```

### 2.3 格式化

- 仓库**基线本身不是 `cargo fmt --all --check` 干净的**（`platform-contracts` / `platform-schema` /
  `platform-server/src/i18n.rs` / `modules/sandbox/**` 等**本轮没碰**的文件也不干净）
  → **没有**跑 `cargo fmt --all`（会污染别人与基线的文件）。
- 只对本轮**自己改过**的文件跑了 `rustfmt --edition 2024`：
  `platform-audit/{archive,query,verify}.rs` + `tests/audit_real_db.rs`、
  `platform-config/{src/service.rs,tests/audit_events.rs}`、
  `platform-iam/{src/audit.rs,src/service.rs,src/sso/bridge.rs,tests/audit_events.rs}`、
  `platform-org/src/audit.rs`、`platform-rbac/{src/service.rs,tests/audit_events.rs}`、
  `platform-server/src/{audit,client_info,static_files}.rs`、`xtask/src/{audit,main}.rs`。
- 复核：`cargo fmt --all -- --check` 的输出里**不再有**本轮改过的任何文件。

---

## 3. 变异验证（注入最小变异 → 确认测试变红 → 还原 → 逐字节相同）

> 方法：先记基线 `sha256` → 注入**生产那一行**的变异 → 跑**相关**测试确认变红
> → 用备份副本还原（**不用** `git checkout`，未跟踪文件还原不了）→ **`touch`**（dev-guide §9.4：
> `cp -p` 保留旧 mtime 会让 cargo **复用陈旧二进制** → 变异还原了但测试还在跑变异后的代码）
> → `sha256sum -c` 确认逐字节相同 → `git diff` 与变异前**逐字一致**。
>
> 脚本：`/tmp/req014-mut/{apply,restore}.py`（**临时脚本，不进仓库**）；
> 备份目录 `/tmp/req014-mut/`（收尾已删）。

### 3.1 逐条证据

| # | 变异点（改的是**生产那一行**） | 变红的测试 | 数字 | 还原校验 |
|---|---|---|---|---|
| **M1** | `crates/platform-audit/src/verify.rs` —— 末尾截断判定 `if actual_tail != expected_tip {` 改成 `if false {`（**D171 的链尖比对被短路**） | `a_truncated_online_tail_is_detected_after_the_archive` | `0 passed; **1 failed**`（`--test audit_real_db`） | `sha256sum -c` → **成功**；`git diff` 与变异前**逐字一致** |
| **M2** | `crates/platform-core/src/extension/audit.rs` —— 驼峰边界的 `&& (is_ascii_lower(prev) \|\| prev.is_ascii_digit())` 改成 `&& false`（**D172 的拆词规则被摘掉**） | `the_db_and_rust_normalization_rules_are_identical` + `the_normalization_follows_the_pinned_rules` + `the_scanner_hits_camel_case_keys` | platform-audit `0 passed; **1 failed**`；platform-core lib `21 passed; **2 failed**` | `sha256sum -c` → **成功**；`git diff` **逐字一致** |
| **M3** | `crates/platform-server/src/static_files.rs` —— `is_api_path()` 的 `path == API_PREFIX \|\| path.starts_with("/api/")` 改成 `false`（**D180 的 API 前缀判定失效** → 未知 API 路径回落成 SPA） | `api_paths_are_recognized` + `an_unknown_api_path_returns_404_instead_of_the_spa` | `4 passed; **2 failed**`（platform-server lib） | `sha256sum -c` → **成功**；`git diff` **逐字一致** |
| **M4** | `crates/platform-rbac/src/service.rs` —— 授权的 `after.sort()` 改成 `after.sort_by(\|l, r\| r.cmp(l))`（**`after` 侧不再是排序后的数组**） | `the_permission_arrays_are_sorted` | `4 passed; **1 failed**`（`--test audit_events`） | `sha256sum -c` → **成功**；`git diff` **逐字一致** |
| **M5** | 同文件 —— `role_permission_codes()` 的 `codes.sort()` 改成 `codes.sort_by(\|l, r\| r.cmp(l))`（**`before` 侧不再是排序后的数组**） | `the_permission_arrays_are_sorted` | `4 passed; **1 failed**`（`--test audit_events`） | `sha256sum -c` → **成功**；`git diff` **逐字一致** |
| **M6** | `crates/platform-server/src/audit.rs` —— 中间件 `authenticate` 的 `State(state): State<Arc<AuditState>>` 改回 `Extension(state): Extension<Arc<AuditState>>`（**B2 的接线 bug 复发**） | `the_events_route_reaches_the_handler_instead_of_failing_to_extract` + `a_request_without_a_token_is_unauthenticated` | `2 passed; **2 failed**`（platform-server lib） | `sha256sum -c` → **成功**；`git diff` **逐字一致**；还原后重跑 **4 passed** |

### 3.2 变异覆盖到的「不变量」

| 变异 | 它证明哪条断言**不是空跑** |
|---|---|
| M1 | **末尾截断真的能被检出**（不是「有链尖但从来没人比过」）—— D171 / AC-S-025 |
| M2 | 驼峰拆词**两侧都真的在跑**（Rust 侧与 DB 侧逐字一致）—— D172 / AC-S-133 |
| M3 | 未知 `/api/**` 路径**真的不回落成 SPA** —— D180 / 契约 C6 |
| M4 / M5 | `before` / `after` 的权限集合**确实是排序后的数组**（两侧分别验）—— AC-S-128 |
| M6 | 路由的**提取器接线**真的被打过（不是「看着像对的」）—— 见 §3.6 |

### 3.3 一处**如实记录**的现象（也是本轮最大的坑）

- **第一次**做 M4 时，变异 `codes.sort()` → 反序后**测试仍然绿** —— 因为
  授权事件的 `after` 是 `before.clone()` + `push` + **另一处** `after.sort()` 排的序，
  `codes.sort()` 只影响 `before`；而当时的用例**只断言了 `after`**。
- 处置：① 把用例扩成**同时**断言 `before` 与 `after`（各带期望条数）；
  ② 补做 **M5**（直接变异 `codes.sort()`）→ 变红 ✓。
- **另一个坑**：`shutil.copy2` **保留旧 mtime** → cargo 认为源文件「没变」→ **复用变异后的二进制**
  → 还原后测试**仍然是红的**（第一次跑 M4 还原后就是这个现象）。
  处置：还原脚本加 `os.utime(...)`（= `touch`），并在还原后**重跑**确认回到绿。
  → 这条已写进还原脚本，**M1~M5 五处都是带 `touch` 重跑过的**。

### 3.4 还原后状态

- 每处变异还原后 `sha256sum -c` 都打印 **成功** → **逐字节相同**。
- 还原后 `git diff -- <file>` 与**变异前**的 `git diff` **逐字一致**（不是「大概一样」）。
- 六处全部还原后**重跑全量**：见 §2（**912 passed / 0 failed / 0 ignored**）。
- 收尾：变异脚本与备份目录 `/tmp/req014-mut/` **已删除**（不入仓库、不留在 `/tmp`）。

### 3.5 阴性对照（避免「测试根本跑不到」）

- M1~M6 的变异点全部在 `#[cfg(test)]` **之外**的**生产代码**里
  → 变异后对应 crate **重新编译**（`Finished` 行后立刻跑测试）。
- M1/M2/M3 用的是**真实库用例**（`--include-ignored` 真的跑了），不是跳过。
- M3 的失败信息指向 `crates/platform-server/src/static_files.rs` 里的断言，
  M4/M5 指向 `crates/platform-rbac/tests/audit_events.rs` 里的断言，
  M6 指向 `crates/platform-server/src/audit.rs` 里的断言 —— 不是别的 crate 的假阳性。

### 3.6 本棒**自己写出来、又在交付前抓到**的两个接线 bug（如实记录）

写完路由后单测与编译**全绿**，但**没有一个用例真的打过一次 `/api/audit/…`** →
两个「提取器接线」bug 就这样溜过去了。补了端到端用例后当场变红：

| # | bug | 症状 | 根因 | 修法 |
|---|---|---|---|---|
| **B1** | 处理器用 `Extension<ReqCtx>` 提取上下文 | 过鉴权的请求 → **500** | 扩展里插的是 `RequestContext`，**没人插 `ReqCtx`** → axum 的「扩展缺失」→ 500 | 处理器改用 `ReqCtx`（`FromRequestParts`）——**与 `platform-config` / `platform-rbac` 的既有写法一致** |
| **B2** | 中间件用 `Extension<Arc<AuditState>>` 取状态 | **所有**请求（含没带令牌的）→ **500** | 中间件状态由 `from_fn_with_state(state, authenticate)` 提供，**请求扩展**里此刻还没有它 | 中间件改用 `State(state): State<Arc<AuditState>>`（中间件自己的状态）；处理器那边继续用 `Extension`（中间件会把状态**插进请求扩展**） |

补的兜底用例（`crates/platform-server/src/audit.rs` 的 `mod tests`）：

- `the_events_route_reaches_the_handler_instead_of_failing_to_extract` ——
  真打一次 `GET /api/audit/events`（带令牌）→ 断言**处理器被真正调用**
  （库不可用 → **503** `AUDIT_SINK_UNAVAILABLE`），**不是**提取器失败（**500**）。
  → B1 / B2 任一复发，这条立刻变红。
- `a_request_without_a_token_is_unauthenticated` —— 没令牌 → **401**（不是 403、不是 500）。

> **教训（如实记录）**：「处理器 + 中间件」这一层，**光看单测绿是不够的** ——
> 提取器接线错了，编译期**一声不吭**。必须**真的打一次请求**。
> 本棒的两处 bug 都是**自己写的**，靠这条兜底用例抓出来 —— 如果不补，交付的就是
> 「一整套 `/api/audit/*` 全返回 500」的接口。

---

## 4. 「开工前必答项」—— 棒 1 §4 的 9 条 + 本棒**新增**的 8 条

### 4.1 棒 1 的 9 条（本棒**全部沿用**，没有推翻）

| # | 问题 | 本棒的回答 |
|---|---|---|
| 1 | 中间件挂哪一层？ | **顶层 Router**（`.layer(from_fn(client_info))`）—— 各子系统**各自**建上下文（`platform-config/src/http.rs:179`、`platform-rbac/src/http.rs:215`、`platform-server/src/i18n.rs:794`、`main.rs:656` 的 `authenticate_module` 只挂在 `/api/sandbox`）→ 只有顶层能对**所有**路由（含 `/api/iam`）生效。中间件把 `RequestContext` 放进扩展，各子系统随后 `extensions().get::<RequestContext>().cloned()` **继承**它；它们会用**令牌**覆盖租户 / 用户，但**不会**覆盖 IP / UA → 继承成立。 |
| 2 | 业务模块怎么拿到 sink？ | `platform_core::extension::audit::AuditSinkSlot`（OnceLock + Clone）→ **后设槽**：不改 `new` 签名（既有调用点全是已合并需求的产物）→「未装配」成为**可表达状态**。 |
| 3 | 未装配 sink 怎么办？ | **跳过埋点 + 记一条 warn**（「以为在审计其实没有」比没有审计更危险）→ 登录**照常**（单测与裸用服务不装 sink）。**装配了但写失败** → 登录 **503** `AUDIT_SINK_UNAVAILABLE`（D157）。 |
| 4 | 登录失败事件怎么保证不含账号名？ | **结构性的**：`login_event()` 的签名里**没有账号名参数** → 构造器**无法**产出含账号名的失败事件；`actor_id` / `object_id` / `before` / `after` 全空。 |
| 5 | SSO 的两个登录事件码谁写？ | **服务自己写**（`IamService` await 平台 sink）；既有 `SsoAuditRecorder::record` 是**同步**的，**表达不了** D157 的「写失败要拒绝登录」。桥接器**只**搬 `iam.sso.link.created` —— 若两者都写，一次登录产生**两条**记录 → AC-S-127 的「产生 1 条」直接失败。 |
| 6 | 既有 recorder trait 签名能改吗？ | **一律不改**（AC-S-129 / D135）：`SsoAuditRecorder` / `AuditRecorder`（org）都实现**同一个 trait**；唯一的新增是 `SsoAuditPayload.tenant_id`（**不序列化** → 白名单不变，既有 REQ-013 测试不受影响）。 |
| 7 | `ip` 能信吗？ | **不能**：`x-forwarded-for` 是**客户端可控的头**，取**第一跳**（既有惯例 `platform-iam/src/http.rs:264-274`）。用途是**关联与排查**，**不是取证**；代码注释与本报告都**不**把它写成「真实客户端地址」（D149）。取不到 → 落**空串**（AC-S-122 要求不得缺键）。 |
| 8 | 归档阈值从哪来？ | `ArchiveService::DEFAULT_ONLINE_DAYS = 90`（与登记的 `audit.retention.onlineDays` 默认值一致）。**本轮只登记不执行**配置读取；xtask 与 HTTP 归档**共用** `archive_older_than()` 路径 → 语义一致。 |
| 9 | 扫描「Rust 侧 0 命中」的口径？ | 不变：写入走 `SECURITY DEFINER` 函数 → `INSERT` 在 **plpgsql 函数体内**，Rust 源码里根本不出现（D152）。棒 1 的扫描用例仍绿（`no_rust_source_writes_the_audit_table_directly`）。 |

### 4.2 本棒**新增**的 8 条（BA 派单时提出、棒 1 未覆盖）

| # | 问题 | 本棒的回答 |
|---|---|---|
| **10** | 中间件挂哪层（**棒 1 §4 第 9 条的延续**）？ | 见上表第 1 条。**顶层** `.layer(from_fn(client_info))`，在**所有** nest 之后、监听之前挂。 |
| **11** | xtask 怎么连库？ | `--database-url` 显式参数 **优先**，其次环境变量 `DATABASE_URL`（都没有 → 报错并给出提示）。**归档必须用应用角色连接**（审计表 append-only → 应用角色**没有** DELETE 权限 → 归档只能走 `SECURITY DEFINER` 的 `platform_audit_archive_move`，而它**绕过 RLS** → 必须由**调用方**把作用域限定住（D174））→ **逐租户**调用（D158）。 |
| **12** | xtask 的「校验全部租户」怎么实现？ | 直接建 `sqlx::PgPool` 查 `platform_audit_chain_state`（**不能**用 `TenantDb`——它的读接口**强制**带租户作用域 → RLS 下恒返回 0 行）。⚠️ 用**应用角色**跑会看到 **0 个租户**（**不是**「没有审计数据」）→ CLI 有明确提示。 |
| **13** | 桥接与直写怎么分工？ | 见上表第 5 条：**服务直写**（两个登录事件码，因为要 await + 要能拒绝）＋ **桥接**（`link.created`、org 的既有事件，因为它们的 trait 签名不可改且是 async 的）。 |
| **14** | 权限变更的审计失败要不要让操作失败？ | **不要**（与登录**刻意不同**）：变更**已经提交** → 这时再返回错误是**撒谎**（客户端以为没改，重试就变重复应用）→ 记 **error** 日志（可观测），操作**照常返回成功**。登录则是「还没生效」→ 审计失败 → **503 拒绝**（D157）。 |
| **15** | `before` / `after` 的权限集合是什么形状？ | **排序后**的字符串数组（确定性）。`before` 取 `role_permission_codes()`（**直接**授权，不含继承展开）；`after` = `before` + 新增的那一个。 |
| **16** | 配置写入的 `before` 取哪一层的值？ | 取「**被写入那一层**」的当前值（不是生效值）—— 审计要记的是「**这次写入**把哪一层的什么值换成了什么」；生效值会混进「平台默认 + 租户覆盖 + 空间覆盖」的解析结果。 |
| **17** | 审计路由的鉴权怎么分层？ | `authenticate` 中间件**必须在 `require_permission` 的外层**（否则鉴权层拿不到用户 → 一律 401，403/401 分不清）。权限点走 `platform-audit` 的**鉴权薄封装**（D160）—— 本文件**不**直接写 `require_permission("audit:…")` 字面量（C-5 只扫「manifest 所属 crate 的 `src/`」→ 字面量必须与权限点登记**同处一地**）。 |
| **18** | 数值查询参数怎么解析？ | **自解析**（D153）：axum 0.8 的 `QueryRejection` 是**纯文本** 400 → 违反契约 C6 → 参数由 `platform-audit` 的 `AuditQueryParams::parse` 解析，错误由路由转成 `ProblemDetails`（`AUDIT_INVALID_FILTER`，400）。 |

---

## 5. 与规格不一致的发现（**需要 BA 拍板 / 已知情**）

### 偏离 1（🔴 **D178 的键名与契约 C9 冲突** —— 需 BA 裁定）

- **D178 正文**写的是 snake_case：`audit.retention.online_days` / `audit.retention.archive_days` /
  `audit.page.default_size` / `audit.page.max_size`。
- **契约 C9** 要求「键名 **camelCase**，层级用 `.` 分隔」。
- **实测**：`platform_config::ConfigRegistry::from_manifest_root` 在**启动时**用 `ConfigKey::parse`
  校验每一个键，而它要求每一段「**小写字母开头 + 只含字母数字**」
  → **带 `_` 的键名直接让服务起不来**（`ConfigError::KeyInvalid`）；仓内现无任何 snake_case 配置键。
- **本棒处置**：按**可用的** camelCase 登记，映射表写在
  `migrations/20261120000400_audit_config_seed.up.sql` 的注释里：

  | D178 正文（snake_case） | 本次登记（camelCase） |
  |------------------------|----------------------|
  | `audit.retention.online_days` | `audit.retention.onlineDays` |
  | `audit.retention.archive_days` | `audit.retention.archiveDays` |
  | `audit.page.default_size` | `audit.page.defaultSize` |
  | `audit.page.max_size` | `audit.page.maxSize` |

- **请 BA 裁定**：改 D178 正文，还是改契约 C9？（本仓**代码**站在 C9 一边。）

### 偏离 2（🟡 `audit.page.defaultSize` = 50 与契约 C7 的默认 limit = 20 不一致）

- D178 登记的值是 50；契约 C7 的**默认 limit 是 20**（上限 200）。
- **本棒处置**：**只登记不执行** —— 查询行为仍按 C7（默认 20 / 上限 200）。
  `manifest.json` 的 label 里已注明「登记值；契约 C7 的默认口径仍是 20」。
- **请 BA 裁定**：把登记值改成 20，还是把 C7 改成 50？（本仓**行为**站在 C7 一边。）

### 偏离 3（🟡 契约 C6 的 Content-Type 口径：全平台是 `application/json`，不是 `application/problem+json`）

- 棒 1 的 D180 说明（及 AC-S-130 步骤 5/6）称「契约 C6 要求错误响应是 `application/problem+json`」。
- **实测**：契约 C6 实际只钉**错误体形状**（`code` / `status` / `detail` / `instance` / `trace_id`）；
  平台全站的 `problem_response()` 统一发 **`application/json`**。
- **本棒处置**：**与平台保持一致**（不另立一套 Content-Type）→ 只改**形状**，不改全站的响应头。
  理由：改全局 `problem_response` 会**超出本棒范围**且有全站回归风险。
- **请 BA 裁定**：C6 是否强制 `application/problem+json`？若是 → 需要一个**跨全站**的独立需求
  （改 `problem_response` 会影响所有子系统的错误响应）。

### 偏离 4（🟡 D179：游标 + 排序 → 400 是**本仓的收紧**，契约 C7 没写）

- **C7 只钉了**「`cursor` 与 `page` 互斥」，**没有**钉「游标 + 任何排序」。
- 本仓的收紧：带了游标又带 `sort` → **400** `AUDIT_INVALID_FILTER`。
  理由：游标里编码的是 `(created_at, id)` 这个**排序键**，翻页条件
  `(created_at, id) < (游标的 created_at, 游标的 id)` 也只在这个排序方向下成立 →
  换排序后游标与新排序对不上 → 翻页会**漏行或重复行** → **宁可 400 也不给出一个错的页**。
- **已落到代码注释**（`crates/platform-audit/src/query.rs` 的模块文档 + 判定处），
  并在本报告复述 → **D179 已裁决保留**。

### 偏离 5（🟡 D180 是「修既有缺陷」，不在棒 2 的原任务卡里）

- D180（BA 后补）要修的是：`/api/audit/**` 这类**未实现的端点**会被**静态回落**成 SPA
  → 返回 **200 + `text/html`**（客户端按 JSON 解析会炸；错误码语义完全错；就绪探测只能靠
  「响应是不是 JSON」这种间接判据）。
- **本棒处置**：**只改 API 前缀的回落分支** → 未知 `/api/**` 返回 **404**（带 `ProblemDetails`）；
  `/assets/**`、`/`、`index.html`、`/apidocs` 等既有回落行为**一字未改**。
- **副作用**：前端棒的「就绪探测」口径可以改成「看状态码」而不是「看内容类型」——
  这也正是前端报告 §5.3 里那条「未注册的 `/api/...` 回落成 SPA」的根因被拔掉。

### 偏离 6（🟢 IAM 审计的分工：两个登录事件码由**服务**写，不走桥接）

- 既有 `SsoAuditRecorder::record` 是**同步**的 → **表达不了** D157 的「写失败要拒绝登录」。
- 若桥接器也写那两个事件码 → 同一次登录产生**两条**记录 → **AC-S-127 的「产生 1 条」直接失败**。
- → 桥接器**只**搬 `iam.sso.link.created`；两个登录事件码由 `IamService` **自己 await** 平台 sink 写。
  三个 SSO 事件码最终**都**进平台 sink ✓。

### 偏离 7（🟢 权限变更 / 配置写入的审计**失败不**让操作失败（与登录刻意不同））

- 变更**已经提交** → 再返回错误是**撒谎**（客户端重试会重复应用）→ 记 **error** 日志，操作照常成功。
- 登录是「**还没**生效」→ 审计失败 → **503 拒绝**（D157）。

### 偏离 8（🟢 「未装配 sink」≠「写入失败」）

- 未注入 sink → **跳过埋点 + warn**（保持既有单测绿、裸用服务可用）；
- 注入了但写失败 → 登录 **503**。

### 偏离 9（🟢 SSO 租户的来源：给 `SsoAuditPayload` 加了**不序列化**的 `tenant_id`）

- 既有 `SsoAuditRecorder::record` 的签名里**没有 ctx**（D135 钉死不可改）→ 租户只能由
  **知道租户的服务**填进载荷。
- 加的是 `pub tenant_id: Option<String>` + `with_tenant()`，**不参与序列化**
  → 载荷**白名单不变** → 既有 REQ-013 的 SSO 审计测试**不受影响**（全绿）。
- 拿不到租户 → **跳过 + warn**（审计记录**必须**属于某个租户）。

---

## 6. 未完成的部分 + 原因

| 项 | 状态 | 原因 |
|---|---|---|
| `/api/audit/**` 的 **HTTP 层真实库用例** | **本棒未做** | 本棒补了**不带真实库**的端到端用例（**真的打一次** `GET /api/audit/events` —— 401 / 处理器被真正调用，见 §3.6），但**没有**「起真实服务 + 真的带令牌 + 真的有数据」的用例 —— 那需要**已配置的 IAM 实例**（密钥 / 租户 / 角色 / 权限点齐全）的起服环境，本棒**没有**这个环境。**QA 的 120 条用例**里数据类用例应覆盖此处。 |
| `audit.retention.*` / `audit.page.*` 的**执行** | **只登记不执行** | D178 的原话是「登记」；且 `audit.page.defaultSize` 与 C7 的 20 冲突（§5 偏离 2）→ 执行前必须先有裁定。 |
| 归档记录的**物理删除**（`archiveDays`） | **不做** | 涉及不可逆操作；归档策略是**手写**的（D166）；本棒只登记阈值。 |
| **审计的分库 / 分区** | **不做** | 父 AC 之外的容量议题，本棒无判据。 |
| **SSO 登录事件码的落库用例** | **只做了单测 + 真实库夹具** | OIDC 回调的端到端需要**真的 OIDC provider**（`sso_oidc.rs` 那套 mock 已存在但没接平台 sink）；本棒在 `sso/bridge.rs` 里验了桥接与「两个登录码由服务写」的分工。 |
| **`cargo xtask` 装成全局命令** | **没做** | 仓内是 `cargo xtask`（`.cargo/config.toml` 的 alias），不装全局二进制（超出本棒范围）。 |

---

## 7. 端点与权限点（落地口径）

| 方法 | 路径 | 权限点 | 处理器 |
|---|---|---|---|
| GET | `/api/audit/events` | `audit:events:read` | `list_events` |
| GET | `/api/audit/events/{id}` | `audit:events:read` | `get_event` |
| GET | `/api/audit/verify` | `audit:verify:run` | `verify_chain` |
| POST | `/api/audit/archive` | `audit:archive:run` | `run_archive` |

响应字段名 **camelCase**（D167 / 契约 C13）：
`id` / `actorId` / `action` / `objectType` / `objectId` / `before` / `after` /
`ip` / `userAgent` / `createdAt` / `prevHash` / `hash` + `total` + `nextCursor`。

---

## 8. 事件码与形状（D163，**照抄**棒 1 报告 §7）

| 事件 | 事件码 | object_type | before | after |
|---|---|---|---|---|
| 本地 / LDAP 登录成功 | `iam.login.succeeded` | `iam_session` | NULL | NULL |
| 本地 / LDAP 登录失败 | `iam.login.failed` | `iam_session` | NULL | NULL |
| SSO 登录成功 | `iam.sso.login.succeeded` | `iam_sso_session` | NULL | NULL |
| SSO 登录失败 | `iam.sso.login.failed` | `iam_sso_session` | NULL | NULL |
| 角色创建 | `rbac.role.create` | `role` | NULL | `{"permissions":[]}` |
| 角色删除 | `rbac.role.delete` | `role` | `{"permissions":[…]}` | NULL |
| 授予权限 | `rbac.role.grant` | `role` | `{"permissions":[…]}` | `{"permissions":[…+p]}` |
| 撤销权限 | `rbac.role.revoke` | `role` | `{"permissions":[…+p]}` | `{"permissions":[…]}` |
| 配置条目写入 | `config.entries.write` | `config_entry` | 变更前 | 变更后 |
| 配置 flag 写入 | `config.flag.write` | `config_flag` | 变更前 | 变更后 |

补充（**不在** D163 表里、但本棒落地的）：

| 事件 | 事件码 | object_type | before | after |
|---|---|---|---|---|
| SSO 身份关联 | `iam.sso.link.created` | `external_identity` | 载荷白名单 | NULL |
| 组织架构事件 | `org.<对象>.<动作>`（既有 `AUDIT_EVENTS` 声明面） | 既有 | `{"detail": …}` | NULL |

> `before` / `after` 的权限集合是**排序后**的字符串数组；
> 登录事件的 `before` / `after` **恒为 NULL**（结构上无法产出账号名）。
