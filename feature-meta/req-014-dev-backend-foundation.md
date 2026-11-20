# REQ-014 后端地基自验报告（棒 1）

- **角色**：后端 Dev（子 Agent，由 BA 派单）
- **worktree**：`feature-REQ-014`（分支 `feature/REQ-014`，基线 `develop@0683115`）
- **规格版本**：`BA/demands/REQ-014-audit/acceptance.md` **rev 3**（13 条 AC）
  + `decisions.md` **rev 2**（D130~D168）+ `design-summary.md` **rev 2**
- **契约**：`api-conventions v1`（C6 / C7 / C12 / C13）
- **本轮范围**：**棒 1 —— 后端地基**（平台核心审计类型 + 新 crate `platform-audit` + 数据库 + 权限）
- **未提交 git**（按 BA 统一提交的约定）

> ⚠️ **本 worktree 里同时有前端棒的产物**（`apps/shell/src/pages/Audit.svelte`、
> `apps/shell/src/lib/audit-api.ts`、`e2e/audit.spec.ts`、`packages/i18n/**` 等）——
> **那些不是本轮 Dev 的改动**，本报告不认领。
> ⚠️ `feature-meta/verification-report.md` 本轮被**覆盖**（上一轮 REQ-010 的同名文件，
> 历史在 git 里）。

---

## 1. 交付清单（文件 → AC）

> **本棒实际改动 = 34 个文件**（另 12 个是前端棒的并行产物，本报告不认领）：
> `Cargo.toml` / `Cargo.lock` / `capabilities/*`（3）= 5；
> `crates/platform-audit/**` = 14（`Cargo.toml`、`manifest.json`、`src/` 8、`tests/` 4）；
> `crates/platform-core/**` = 6；`crates/platform-iam/tests/sso_audit.rs` = 1；
> `crates/platform-server/tests/i18n_titles.rs` = 1；`deploy/seed/02-app-role.sql` = 1；
> `migrations/*audit*` = 4；`feature-meta/*` = 2。
> （超出「预期 20~28」的区间是因为新 crate 本身就有 14 个文件 + 4 个迁移文件；**未超过 40 的红线**。）

### 1.1 平台核心（`platform-core`）

| 文件 | 改动 | 对应 AC / 决策 |
|---|---|---|
| `crates/platform-core/src/extension/audit.rs`（**新建**） | 通用 `AuditEvent`（带 payload）、`AuditSink` trait（`BoxFuture`，**可对象化**）、`AuditError`（5 个变体 + `code()` / `status()` / `detail()`）、**JSON 规范化** `canonical_json`（递归键排序 + 紧凑 + 非 ASCII 不转义）、**敏感键扫描器** `scan_sensitive_keys`（14 键基线 + 完整键名匹配 + 递归）、`normalize_sensitive_key`、`GENESIS_HASH`、`format_timestamp_micros_utc`（**手工拼装**，D144） | AC-S-122 / AC-S-133 / AC-S-124；D131 / D135 / D142 / D144 / D156 |
| `crates/platform-core/src/extension/mod.rs` | 挂载 `pub mod audit;` + 再导出 | 同上 |
| `crates/platform-core/src/lib.rs` | 再导出 `AuditError` / `AuditEvent` / `AuditSink` | 同上 |
| `crates/platform-core/src/context.rs` | `RequestContext` + `client_ip` / `user_agent` 两字段 + `with_client_ip()` / `with_user_agent()` + 两个 getter + 3 条单测 | **D140**（AC-S-127 / AC-S-122 的 IP/UA 来源） |
| `crates/platform-core/src/problem.rs` | **5 个新错误码**的 8 处编辑点（见 §1.5）+ 5 个 `ProblemDetails::audit_*` 构造器 + 3 条单测 | AC-S-130 / AC-S-133 / AC-S-022；D125 / D153 / D157 |
| `crates/platform-core/src/titles.rs` | + 5 条 ASCII 标题（**79 → 84**） | D125（`platform-server/tests/i18n_titles.rs` 的行数断言同步改为 84） |

### 1.2 新 crate `crates/platform-audit/`

| 文件 | 内容 | 对应 AC / 决策 |
|---|---|---|
| `Cargo.toml` | workspace 成员；依赖 `platform-core` / `platform-db` / `sha2` / `time` / `base64` / `serde_json` / `tracing`（**全部已在树里，零新增第三方依赖**） | dev-guide §9.10 |
| `manifest.json` | 3 个权限点（`audit:events:read` / `audit:verify:run` / `audit:archive:run`）+ 1 条菜单 + `config: {}`；**不写 `tenantScoped`**（写了空数组会过不了 `Manifest::validate`） | AC-S-129；D138 / D160 / D166 |
| `src/lib.rs` | 模块挂载 + 再导出 + `VERSION` | — |
| `src/sink.rs` | `PgAuditSink`（`impl AuditSink`）：租户一致性校验 → **Rust 侧敏感键提前拦截** → JSON 规范化 → 调 `platform_audit_append` | AC-S-122 / AC-S-133；D134 / D141 / D150 |
| `src/query.rs` | `AuditQueryParams::parse`（**自解析**，不用 axum `Query`）+ `AuditQueryService::query`（C7 游标分页 + 过滤 + 排序）+ `encode_cursor` / `decode_cursor` | AC-S-130 的**服务层**（HTTP 在棒 2）；D145 / D153 / D155 / D162 |
| `src/verify.rs` | `ChainVerifier::verify`：**两段式**校验（逐条独立重算 + 接续校验）+ `BrokenReason`（**恰好两个**）+ 断裂时 `tracing::error!`（含 `tenant_id` / `event_id` / `reason`）+ `compute_hash`（长度前缀拼接） | AC-S-022 / AC-S-124 / AC-S-125；D141 / D146 / D151 / D164 |
| `src/archive.rs` | `ArchiveService::archive_before` → `ArchiveResult { moved, archivedAt, nextRecommendedAt }` | AC-S-131；D137 / D158 / D159 |
| `src/authz.rs` | **3 个薄封装**：`require_events_read()` / `require_verify_run()` / `require_archive_run()`（内部是 `require_permission("字面量")`） | **D160**（C-5 只扫 manifest 所属 crate 的 `src/`） |
| `src/sensitive.rs` | 再导出 Rust 侧扫描器 + **从迁移 SQL 抽出 DB 侧键数组**（`db_sensitive_keys`） | **D156** 的「一致性测试」的原料 |
| `src/error.rs` | 再导出 5 个错误码 + `ALL_CODES` + `problem_details()` | D113 / D125 |
| `tests/common/mod.rs` | 真实库夹具（只连 `base_test`、管理员连接 + 应用角色连接、部署锁、夹具复位） | dev-guide §9.9 |
| `tests/audit_real_db.rs` | 11 条真实库用例（写入完整性 / append-only / **重跑种子脚本后仍拒** / RLS 隔离 / 敏感载荷 / 扫描口径 / 归档） | AC-S-122 / 123 / 126 / 129 / 131 / 133 |
| `tests/audit_chain.rs` | 7 条链用例（分链 / **独立复算** / 时间戳严格递增 / **并发不分叉** / 两种篡改 / 时间戳逐字节一致） | AC-S-022 / 124 / 125 |
| `tests/contract_c5.rs` | 2 条（C-5：3 个权限点各有字面量调用点；manifest 合法） | D160 |

### 1.3 数据库

| 文件 | 内容 | 对应 AC / 决策 |
|---|---|---|
| `migrations/20261120000200_audit.up.sql` | **3 张表**（`platform_audit_log` 13 个固定字段 / `platform_audit_archive` 同 13 个 + `archived_at` / `platform_audit_chain_state`）+ 4 个索引 + **3 张表的 RLS（手写）** + `GRANT SELECT, INSERT` + `REVOKE UPDATE, DELETE`（append-only）+ **3 个 `SECURITY DEFINER` 函数**（`platform_audit_append` / `platform_audit_head_hash` / `platform_audit_archive_move`）+ 2 个普通函数（`platform_audit_canonical_hash` / `platform_audit_sensitive_keys`）+ 函数授权 | AC-S-122 / 123 / 124 / 125 / 126 / 131 / 133；D130 / D132 / D133 / D141 / D143 / D156 / D159 / D161 / D166 |
| `migrations/20261120000200_audit.down.sql` | 先删 5 个函数，再删 3 张表 | 可逆 |
| `migrations/20261120000300_audit_permission_seed.up.sql` | **3 条权限点 INSERT**，`source = 'crates/platform-audit/manifest.json'` | D27 / D160 |
| `migrations/20261120000300_audit_permission_seed.down.sql` | 只删这 3 行（且校验 `source`） | 可逆 |
| `deploy/seed/02-app-role.sql` | 🔑 **审计表例外块**（`platform_audit_log` / `platform_audit_archive` / `platform_audit_chain_state`） | **D147**（append-only 会被 blanket GRANT 破功） |

### 1.4 工作区与生成物

| 文件 | 改动 |
|---|---|
| `Cargo.toml` | + workspace 成员 `crates/platform-audit` + workspace 依赖项 |
| `Cargo.lock` | + `platform-audit`（**第三方包数不变** —— `sha2` / `base64` / `sqlx` / `time` / `uuid` / `tracing` 全在锁文件里） |
| `capabilities/permissions.json` / `index.json` / `extension-points.json` | `cargo xtask capabilities` **重新生成**（新增 manifest + 新增 `AuditSink` trait）→ `cargo xtask capabilities --check` 通过 |

### 1.5 「新增错误码 = 8 处编辑点」在 `platform-core` 的落法

core 的 `problem.rs` 没有「enum 变体 / `code()` / `detail()`」那套结构（那是各业务 crate 的
`error.rs` 风格），所以 8 处按 core 的真实结构落：

| # | 编辑点 | 位置 |
|---|---|---|
| 1 | 5 个 `CODE_*` 常量 | `problem.rs` 顶部 |
| 2 | `ALL_CODES` + 5 项 | `problem.rs` |
| 3 | 5 个 `ProblemDetails::audit_*` 构造器 | `problem.rs` |
| 4 | 测试：5 个码的 `code` / `status` / `detail` / `title` 逐个断言 | `problem.rs` 的 `mod tests` |
| 5 | 测试：错误体**不回显**敏感值 | `problem.rs` 的 `mod tests` |
| 6 | 测试：5 个码都在 `ALL_CODES` | `problem.rs` 的 `mod tests` |
| 7 | `TITLES` + 5 条标题（79 → 84） | `titles.rs` |
| 8 | 行数断言 79 → 84 | `crates/platform-server/tests/i18n_titles.rs` |

外加 `crates/platform-audit/src/error.rs` 的 `ALL_CODES`（完备性清单）。

---

## 2. 逐 crate 测试数字（真跑出来的）

跑法（dev-guide §9.8：`--all-targets` 不可省、只有一个 `--`、`--include-ignored` 在 `--` 之后）：

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored
```

| crate | passed | failed | ignored | 备注 |
|---|---|---|---|---|
| `platform-audit`（**本轮新增**） | **44** | 0 | 0 | lib 24 + `audit_chain` 7 + `audit_real_db` 11 + `contract_c5` 2 |
| `platform-core` | **150** | 0 | 0 | lib 143 + `tests/authz_entry.rs` 7；本轮 lib **+23**（`extension/audit.rs` 18 + `context.rs` 2 + `problem.rs` 3；HEAD 基线 lib = 120） |
| `platform-db` | 46 | 0 | 0 | 未改代码，回归 |
| `platform-iam` | **168** | 0 | 0 | 未改代码，回归（改了 1 条既有断言，见 §5 偏离 7） |
| `platform-org` | 91 | 0 | 0 | 未改代码，回归 |
| `platform-rbac` | 78 | 0 | 0 | 未改代码，回归 |
| `platform-config` | 85 | 0 | 0 | 未改代码，回归 |
| `platform-schema` | 20 | 0 | 0 | 未改代码，回归 |
| `platform-server` | 112 | 0 | 0 | 未改代码，回归（改了 1 条既有断言：TITLES 行数） |
| `sandbox` | 33 | 0 | 0 | 未改代码，回归 |
| **合计** | **827** | **0** | **0** | |

> **统计口径**：`passed` = 该 crate **所有 test target 的 `passed` 之和**（lib 单测 + 每个 `tests/*.rs`）。
> 全部 `failed = 0`、`ignored = 0`（`--include-ignored` 让真实库用例真的跑了，不是跳过）。

### 本轮新增的测试（按文件）

| 文件 | passed | 说明 |
|---|---|---|
| `crates/platform-core/src/extension/audit.rs`（**新建**，core lib 内） | **18** | `extension::audit` 模块实测 20 条 = 本文件 18 + 既有 `extension::auditable` 2 |
| `crates/platform-core/src/context.rs` | **2** | 模块内 6 → 8 |
| `crates/platform-core/src/problem.rs` | **3** | 模块内 5 → 8 |
| `crates/platform-audit/src/*`（**新建** crate lib 内） | **24** | `query` 13 + `verify` 3 + `error` 2 + `sensitive` 2 + `archive` 1 + `authz` 1 + `lib` 2 |
| `crates/platform-audit/tests/audit_real_db.rs`（**新建**） | **11** | |
| `crates/platform-audit/tests/audit_chain.rs`（**新建**） | **7** | |
| `crates/platform-audit/tests/contract_c5.rs`（**新建**） | **2** | |
| **本轮新增合计** | **67** | 23（core lib）+ 44（platform-audit 全 crate） |

> `crates/platform-audit/tests/common/mod.rs` 是夹具模块，不含 `#[test]`。

### 格式化（`cargo fmt`）

- 本轮改动文件已全部 `rustfmt` 化：`cargo fmt -p platform-audit -p platform-core`（**只**命中本轮 13 个文件，
  未污染其它 crate）。
- `cargo fmt --all --check` 仍报 **11 个文件**有差异 —— **全部是 HEAD 上就存在的既有问题**，
  且 **本轮一行都没碰过**（`git diff --name-only` 与该列表交集为空）：
  `platform-contracts/{src/checks.rs, src/schema_scan.rs, tests/c6_schema_projection.rs}`、
  `platform-schema/{src/registry.rs, tests/metadata_api.rs}`、
  `platform-server/{src/i18n.rs, tests/i18n_api.rs, tests/i18n_context_real_db.rs}`、
  `modules/sandbox/{backend/src/demo_item_http.rs, backend/src/demo_item.rs, tests/demo_items.rs}`。
- → **按「不修无关失败」原则，这 11 个不动的**（修它们会把本轮 diff 撑大、且不属于 REQ-014 范围）。

---

## 3. 变异验证（注入最小变异 → 确认测试变红 → 还原 → 逐字节相同）

> 方法：先记基线 `sha256` → 注入**生产那一行**的变异 → 跑**相关**测试确认变红
> → 用备份副本还原（**不用** `git checkout`，未跟踪文件还原不了）→ `touch`（dev-guide §9.4：
> `cp -p` 保留旧 mtime 会让 cargo 复用陈旧二进制）→ `sha256sum -c` 确认逐字节相同
> → `git status` 干净。
>
> 变异脚本：`/tmp/req014-mutate.sh`（**临时脚本，不进仓库**）；备份目录 `/tmp/req014-mut/`（收尾已删）。
> 下面 5 条证据是**最终态（rustfmt 之后）**重跑出来的，下面的表格就是那次运行的原始输出摘录
> （`test result: FAILED. N passed; M failed` 行是原样贴过来的）。

### 3.1 逐条证据

| # | 变异点（改的是**生产那一行**） | 变红的测试 | 数字 | 还原校验 |
|---|---|---|---|---|
| **M1** | `crates/platform-core/src/extension/audit.rs` —— `canonical_json` 的 `keys.sort_by` 由**升序**改成**降序**（D142 要求的 UTF-8 字节序升序） | `extension::audit::tests::canonical_json_sorts_keys_recursively` | `142 passed; **1 failed**`（platform-core lib） | `sha256sum -c` → **成功** |
| **M2** | 同文件 —— `scan_sensitive_keys` 的 `SENSITIVE_KEYS.contains(&normalized)` 改成 `iter().any(|key| normalized.contains(key))`（**完整键名匹配 → 子串匹配**） | `extension::audit::tests::the_scanner_does_not_match_substrings` | `142 passed; **1 failed**`（platform-core lib） | `sha256sum -c` → **成功** |
| **M3** | `deploy/seed/02-app-role.sql` —— 审计表例外块里的 `REVOKE UPDATE, DELETE ON platform_audit_log FROM base_platform_app` 改成 `GRANT ... TO base_platform_app`（**D147 的破功路径**：blanket GRANT 覆盖不到收回） | `the_app_role_cannot_update_or_delete_audit_rows` + `the_append_only_guarantee_survives_a_seed_script_rerun` | `9 passed; **2 failed**`（`--test audit_real_db`） | `sha256sum -c` → **成功** |
| **M4** | `crates/platform-audit/src/verify.rs` —— `compute_hash` 的字段表把 `input.before, input.after` **对调**（规范哈希的字段顺序） | `concurrent_writes_to_one_tenant_do_not_fork_the_chain` + `tampering_with_the_after_column_is_detected` + `tampering_with_the_stored_hash_breaks_the_next_link_too` | `4 passed; **3 failed**`（`--test audit_chain`） | `sha256sum -c` → **成功** |
| **M5** | `migrations/20261120000200_audit.up.sql` —— `platform_audit_canonical_hash` 的长度前缀由 `octet_length(convert_to(f,'UTF8'))`（**字节长度**）改成 `length(f)`（**字符长度**，D131 的长度前缀语义被破坏） | `concurrent_writes_to_one_tenant_do_not_fork_the_chain` + `tampering_with_the_after_column_is_detected` + `tampering_with_the_stored_hash_breaks_the_next_link_too` + `the_stored_hash_can_be_recomputed_independently` | **4 failed**（`--test audit_chain`） | `sha256sum -c` → **成功** |

### 3.2 变异覆盖到的「不变量」

| 变异 | 它证明哪条断言**不是空跑** |
|---|---|
| M1 | 键序**规范化**真的被断言（`canonical_json` 不是随便拼字符串） |
| M2 | 敏感键是**完整键名匹配**（`api_token` 不会误伤 `api_tokenizer`）—— D156 |
| M3 | **部署种子脚本重跑后 append-only 仍然成立**（不是只靠迁移里那一句）—— AC-S-123 / D147 |
| M4 | 规范哈希**覆盖 before/after 两个字段且顺序固定** —— AC-S-125 / D141 |
| M5 | 长度前缀用**字节长度**（多字节字符会算错）—— D131；且**独立复算**用例能抓住（`the_stored_hash_can_be_recomputed_independently`） |

### 3.3 还原后状态

- 每处变异还原后 `sha256sum -c` 都打印 **成功** → **逐字节相同**。
- M5 改的是**迁移文件** → 还原后 `DELETE FROM _sqlx_migrations WHERE version = 20261120000200;`
  让 sqlx migrator 重跑（迁移是幂等的：`CREATE TABLE IF NOT EXISTS` / `CREATE OR REPLACE FUNCTION`）。
- 全部 5 处还原后复跑：`platform-audit` **44 passed / 0 failed / 0 ignored**、
  `platform-core` **150 passed / 0 failed / 0 ignored**。

### 3.4 阴性对照（避免「测试根本跑不到」）

- M3/M5 用的都是**真实库**用例（`--include-ignored` 真的跑了），不是跳过。
- M1/M2 的变异点是 `#[cfg(test)]` 之外的**生产代码** → 变异后 lib 目标**重新编译**，
  失败信息指向 `crates/platform-core/src/extension/audit.rs:527` / `:488`（= 对应用例里那条
  `assert!` 的行号，两处都在**被测生产模块**所在的文件里，不是别的 crate 的假阳性）。

### 3.5 一处**如实记录**的现象

- M1/M2 的 `git status --short` 对 `crates/platform-core/src/extension/audit.rs` 显示 `??`
  —— 因为该文件是**本轮新建的未跟踪文件**（`git checkout` 还原不了，故用备份副本还原），
  **不是**「还原后仍有残留改动」。字节级一致性由 `sha256sum -c` 保证。
- M3 的 `deploy/seed/02-app-role.sql` 显示 ` M` —— 这是它**本来就有的本轮合法改动**，
  还原后 sha256 与变异前一致。

### 3.6 收尾

- 变异脚本 `/tmp/req014-mutate.sh`、备份目录 `/tmp/req014-mut/` 与本次的其它临时产物
  **已删除**（不入仓库、不留在 `/tmp`）。

---

## 4. 「开工前必答项」（design §12 的 9 条）逐条回答

| # | 问题 | 本轮的回答 |
|---|---|---|
| 1 | 并发安全方案 | ✅ **照 D161 做**：`pg_advisory_xact_lock(hashtext('audit:' \|\| p_tenant_id))` + **拿到锁后用新语句重读**最后一条。**没有**用 `SELECT … FOR UPDATE`（READ COMMITTED 下防不住分叉 —— 迁移里写了这个警告）。并发验收项有测试（`concurrent_writes_to_one_tenant_do_not_fork_the_chain`） |
| 2 | JSON 规范化放 core 还是 platform-audit | **放 core**（`platform_core::extension::audit::canonical_json`），**与 design 建议一致**。理由：业务方自己落审计时也需要同一套形态；core 是最底层，各模块都依赖它；放 `platform-audit` 会让 iam/org 想用就得依赖它（`platform-audit → platform-iam` 成环风险） |
| 3 | `RequestContext` 加字段后有没有被序列化 / 跨进程传递 | **没有**。实测：`RequestContext` 只 derive `Debug, Clone, PartialEq, Eq`（**不 derive Serialize**），全仓它只以 `&RequestContext` **引用**出现（`platform-iam/src/store.rs` 等），没有任何结构体把它**按值**持有并序列化 → 加两个字段**零连带影响** |
| 4 | 查询接口放 server 还是 platform-audit | **HTTP 放 `platform-server`（棒 2），查询**服务**放 `platform-audit`**。依赖图证明：`platform-audit` 只依赖 `platform-core` + `platform-db`；**不依赖 `platform-rbac`**（鉴权入口在 core 的 `extension::permission`，薄封装只是调用它）→ `platform-audit → platform-rbac → platform-iam → platform-audit` 的环**不存在** |
| 5 | `audit.retention.archive_days` 物理删除 | ✅ **同意「本轮只登记不执行」**。⚠️ 但本轮**连「登记」都没做** —— 登记要改 `crates/platform-config/manifest.json` + 加迁移 INSERT（漏 INSERT → 启动失败），不在棒 1 的文件清单里；`crates/platform-audit/manifest.json` 的 `config` 段**留空**（不引入未登记的配置项）。归档节奏当前是常量 `RECOMMENDED_INTERVAL_DAYS = 1` |
| 6 | `pg_advisory_xact_lock` 的键 | **用 `hashtext`（32 位）**，与 design 建议一致。理由：`hashtext` 是**碰撞只影响并发度**（不同租户撞同一锁 = 串行化过度，**不影响正确性**）；换 64 位的 `hashtextextended` 能减少碰撞，但**收益极小**（租户数是几百量级），而 `hashtext` 是 Postgres 内置、行为稳定、迁移里可读 |
| 7 | 游标的编码方式 | **base64url(`"{createdAt}\|{id}"`)**，`createdAt` 是规范时间戳文本（UTC、微秒 6 位）。**与排序的对应关系**：默认排序是 `ORDER BY created_at DESC, id DESC`，游标翻页条件是**元组比较** `(created_at, id) < (游标的 created_at, 游标的 id)` —— **方向与排序一致**（DESC）。⚠️ 因此**游标只对默认排序有效**：指定了别的 `sort` 又带 `cursor` → **400 `AUDIT_INVALID_FILTER`**（游标与排序对不上时悄悄给一页错的更糟）。`createdAt` 以**文本**回传并交给 SQL `?::timestamptz` 转型 —— `time` crate **没开** `parsing` feature，Rust 侧**不能**解析 RFC3339 |
| 8 | 敏感键扫描器放 DB 还是只在 Rust | **两侧都做**（D156）：DB 侧 `platform_audit_sensitive_keys`（**权威**，写在 `SECURITY DEFINER` 函数内 → 绕过应用直接调函数也会被拒）+ Rust 侧 `scan_sensitive_keys`（**可单独调用**，供测试 / 工具 / 提前拦截）。**一致性测试**：`tests/audit_real_db.rs::the_db_and_rust_sensitive_keys_are_identical` 从**迁移 SQL 里**抽出 DB 侧的 14 键数组，与 Rust 常量**逐项比对** |
| 9 | 中间件在哪一层填 `RequestContext.client_ip` | **棒 2 的活**（本轮只加了字段与构造器）。实测现状：`platform-server/src/main.rs` 的 `authenticate_module`（`:656` 起的 axum 中间件）只建**租户 / 用户**上下文，**且它只挂在「业务模块装配点」**（`:410` 的 `router.layer(from_fn_with_state(ModuleAuthState{…}, authenticate_module))`，目前只有 `/api/sandbox`）；平台子系统（`/api/iam` / `/api/org` / `/api/rbac` / `/api/config`）**各自**在自己的中间件里建上下文（`platform-config/src/http.rs:179`、`platform-rbac/src/http.rs:215`、`platform-server/src/i18n.rs:794`）。→ **要「对所有路由（含 `/api/iam`）生效」，棒 2 必须在 `platform-server` 的顶层 `Router` 上加一个**全局**中间件**：读 `x-forwarded-for`（**第一跳**，与 `crates/platform-iam/src/http.rs:264-274` 的 `request_meta` 既有惯例一致）与 `user-agent`，把 `RequestContext` 塞进请求扩展；各子系统的中间件随后 `extensions().get::<RequestContext>().cloned()` 就会**继承**它。⚠️ `ip` 是**客户端可控的头**，**不是**可信的物理来源 —— 用途是关联与排查，不是取证（D149） |

---

## 5. 与规格不一致的发现（**需要 BA 拍板 / 已知情**）

### 偏离 1（🔴 **D159 自相矛盾**，我按「列的定义 + 接续校验规则」实现）

D159 同时说了三件事：

1. `head_hash` 的定义 = 「该租户链的**当前基准**（genesis，或上次归档的尾 `hash`）」；
2. 「**写入时**更新 `head_hash` = 新记录的 `hash`」；
3. 「**校验时**：在线表**第一条**的 `prev_hash` 必须等于 `head_hash`」。

**2 与 3 互相矛盾**：若把新记录的 hash 写进 `head_hash`，接续校验就会拿「在线段第一条的 `prev_hash`」（= genesis 或归档尾）与「链尖」（= 最新一条的 hash）比 → **每写一条都误报断裂**。

- **实测证据**：第一版按 2 实现，`the_chain_is_linked_per_tenant` / `concurrent_writes_...` / 两个篡改用例**全红**（`PrevHashMismatch`，expected = 链尖、actual = genesis）。
- **本轮按 1 + 3 实现**：`head_hash` = 「当前**在线段**的基准」= genesis 或上次归档那段的尾 hash；**写入只维护 `updated_at`，归档才更新 `head_hash`**。
- **为什么选它**：1 与 3 自洽，且 3 是**校验规则**（AC-S-131 步骤 6「归档后链校验 `ok == true`」直接依赖它）；2 单独看没有可验证的判据。
- **代价**：链的**尖端**不再落库 → 「末尾被截断」检不出来（链内**相邻**断裂仍能检出）。若 BA 要「截断可检出」，需要给 `platform_audit_chain_state` **再加一列**（`head_hash` 存基准、`tip_hash` 存尖端）—— 这会改 D159 的表结构，故**不在本轮自作主张**。

### 偏离 2（🟡 design §15.2 的薄封装签名写不成）

design §15.2 的示例是
`pub fn require_events_read(ctx: &RequestContext) -> anyhow::Result<()> { require_permission(ctx, "audit:events:read") }`，
但 core 的鉴权入口 `require_permission` 是**路由层构造器**（返回 `PermissionLayer`），
**没有**「传 ctx 直接判定」的形态 → 示例**编译不过**。

- **本轮实现**：`pub fn require_events_read() -> PermissionLayer { require_permission("audit:events:read") }`
  —— 与既有惯例一致（`crates/platform-rbac/src/http.rs` 的 `.route_layer(require_permission("rbac:role:read"))`），
  且 C-5 照样扫得到字面量 ✓。
- **不影响任何 AC**（C-5 只要求「该 crate 的 `src/` 里有 `require_permission("字面量")` 调用点」）。

### 偏离 3（🟡 归档函数的签名多了租户参数）

design §3.6 的 `platform_audit_archive_move(p_before TIMESTAMPTZ)` **没有**租户参数，
但 D158 要求归档**单租户**（受 RLS 约束）。⚠️ `SECURITY DEFINER` 会**绕过 RLS** →
按 design 的签名实现就是「全平台归档」，与 D158 矛盾，且是**跨租户数据搬运**。

- **本轮实现**：`platform_audit_archive_move(p_tenant_id TEXT, p_before TIMESTAMPTZ)`，
  函数内**显式守卫**：`p_tenant_id` 必须与 `current_setting('app.tenant_id', true)` 一致，否则
  `RAISE EXCEPTION … ERRCODE = 'insufficient_privilege'`。
- 同理，`platform_audit_head_hash(p_tenant_id)` 也带显式守卫。

### 偏离 4（🟡 `base_platform_app` 连不上时的错误措辞）

TC-014-015 期望错误含 `role "base_platform_app" cannot be logged in`，
但**口令不匹配**时 Postgres 先报 `password authentication failed`（组角色没有口令）。
→ 本轮测试断言**事实**（连接失败 + `rolcanlogin = false`），**不断言措辞**。

### 偏离 5（🟡 TC-014-107 的 `apiKey` 期望与 D156 冲突）

TC-014-107 期望 `{"apiKey":"x"}` **命中**；D156 钉死的归一化是「转小写 + `-` 视为 `_`」
→ `apiKey` → `apikey` ≠ `api_key` → **不命中**。

- **本轮按 D156 + acceptance rev 3 实现**（acceptance rev 3 只要求 `api_key` 命中、
  `secretary` / `tokenizer` **不**命中）。
- ⚠️ 后果：`apiKey` 这类**驼峰**写法**不被拦** → 若 BA 认为该拦，需要 D156 追加「驼峰也归一化」的规则
  （那是**规范变更**，不是实现自由）。

### 偏离 6（🟡 排序字段名同时接受 camelCase 与 snake_case）

acceptance rev 3 / design §5.1 用 camelCase（`?sort=-createdAt`），D145 正文与契约 C7 的示例用
snake_case（`?sort=-created_at`）。→ 本轮**两种写法都接受**（指向同一列），
**未声明**的字段一律 **400 `AUDIT_INVALID_FILTER`**。

### 偏离 7（🟡 改了 2 处既有测试的断言）

| 文件 | 改动 | 为什么必须改 |
|---|---|---|
| `crates/platform-server/tests/i18n_titles.rs` | `TITLES` 行数断言 `58 + 21` → `58 + 21 + 5`（79 → 84） | D125：新增错误码必须同步登记标题 → 行数钉死值必须跟着动 |
| `crates/platform-iam/tests/sso_audit.rs` | `ac_s_108_no_sso_audit_table_exists_yet` 的断言从「库里**没有任何** `*audit*` 表」改成「没有任何 **SSO 专用**审计表」（排除 `platform_audit_*`），并**加一条对照**：平台级通用审计表**应当存在** | REQ-013 的那条断言编码的是「落库是 REQ-014」→ REQ-014 落地后它**必然**红。它的**意图**（不要偷偷建 SSO 专用表）被保留 |

### 偏离 8（🟢 归一化由 **Rust 侧**做，DB 侧只做敏感键检查）

design §3.5 一处说「函数内部做 JSON 规范化」，另一处说「函数内部不做 JSON 处理」。
- **本轮**：**Rust 侧**（`canonical_json`）产出规范化文本 → 传给 DB 函数的**就是**规范化文本
  → DB **原样存储**（哈希覆盖的就是存进列里的那段文本，D141）。
- **理由**：D142 要求的键序是「UTF-8 **字节序**升序」，而 Postgres 的 `jsonb` 是按「长度 + 字节」排的
  → DB 侧用 `jsonb` 做不出这个序；要用 plpgsql 递归实现 JSON 规范化，复杂且易错。
- **代价**：绕过 Rust 直接调 `platform_audit_append` 时，存进去的 JSON **可能不是规范形态**
  （但**哈希仍然自洽** —— 哈希覆盖的就是存进去的那段文本）。

### 偏离 9（🟢 `tenant_id` 查询参数：忽略 + warn（D162）已在本轮落地）

`AuditQueryParams::parse` 遇到 `tenant_id` → **忽略** + `tracing::warn!`（不 400）→ 已实现并有单测。

---

## 6. 未完成的部分 + 原因

| 项 | 原因 |
|---|---|
| **业务模块埋点**（iam / rbac / config / org） | 棒 2 的范围。本轮已备好通用 `AuditEvent` / `AuditSink` 与适配器所需的形状 |
| **`/api/audit` 路由 + sink 装配 + 中间件填 IP/UA** | 棒 2 的范围。本轮已备好 `PgAuditSink` / `AuditQueryService` / `ChainVerifier` / `ArchiveService` / 三个鉴权薄封装 |
| **`xtask audit verify` / `xtask audit archive`** | 棒 2 的范围 |
| **前端审计页** | 棒 2 的范围（本 worktree 里的前端产物由前端棒负责） |
| **`audit.retention.*` / `audit.page.*` 四个配置项登记** | 见 §4 第 5 条与偏离 —— 需要改 `platform-config/manifest.json` + 迁移 INSERT，不在本轮文件清单内；**不引入未登记的配置项**（漏 INSERT → 启动失败） |
| **AC-S-130 的 HTTP 侧**（400 的 ProblemDetails 响应体、403 `INSUFFICIENT_PERMISSION`、响应含 `ip` / `userAgent` 两键） | 棒 2 的范围。参数自解析与错误码**已**在 `query.rs` 落地并有单测 |
| **AC-S-127 / AC-S-128 的埋点** | 棒 2 的范围。事件码与 `before`/`after` 形状已按 D163 写进 §7 |

---

## 7. 棒 2 接入时必须遵守的事件码 / 形状（D163，**照抄**）

| 事件 | 事件码 | `object_type` | `before` | `after` |
|---|---|---|---|---|
| 本地 / LDAP 登录成功 | `iam.login.succeeded` | `iam_session` | `NULL` | `NULL` |
| 本地 / LDAP 登录失败 | `iam.login.failed` | `iam_session` | `NULL` | `NULL` |
| SSO 登录成功 | `iam.sso.login.succeeded` | `iam_sso_session` | `NULL` | `NULL` |
| SSO 登录失败 | `iam.sso.login.failed` | `iam_sso_session` | `NULL` | `NULL` |
| 角色创建 | `rbac.role.create` | `role` | `NULL` | `{"permissions":[]}` |
| 角色删除 | `rbac.role.delete` | `role` | `{"permissions":[…]}` | `NULL` |
| 授予权限 | `rbac.role.grant` | `role` | `{"permissions":[…]}` | `{"permissions":[…+p]}` |
| 撤销权限 | `rbac.role.revoke` | `role` | `{"permissions":[…+p]}` | `{"permissions":[…]}` |
| 配置条目写入 | `config.entries.write` | `config_entry` | 变更前 | 变更后 |
| 配置 flag 写入 | `config.flag.write` | `config_flag` | 变更前 | 变更后 |

- 权限集合是**排序后**的字符串数组（确定性）。
- ⚠️ **登录失败的事件不得含账号名**（不可枚举，AC-S-127 步骤 5）。
- ⚠️ **审计写入失败 → 拒绝登录并返回 503 `AUDIT_SINK_UNAVAILABLE`**（D157）。
