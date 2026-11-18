# 验证报告 —— Pre-merge

- **需求**: REQ-006 多租户与隔离
- **分支**: `feature/REQ-006` @ `668d357ec1fa430e9a22c81c8bd6c07fb54acd38`
- **基线**: `develop` @ `806bb66af904e24f69035ca838ba84a62f0af20f`
- **改动规模**: 45 文件 / +3302 -644 行；新增工作区成员 `crates/platform-db`（共 7 个）
- **验证者**: QA Agent（独立复现；未采信 Dev 自验报告里的任何数字与结论，全部自己重跑）
- **环境**: rustc/cargo 1.98.1 · podman 6.2.0 + postgres 17-alpine · pnpm · bun 1.4.4 · python3 3.14
- **日期**: 2026-11-18
- **结论**: **PASS-WITH-RISK**

> 核心结论：**代码层的实现是对的，测试是有效的**。我逐条自己复现了 Dev 的关键数字 —— RLS fail closed 的判别实验、
> 串租户、公开 API 扫描、C-3 真实库层、依赖门禁破坏性验证、172/172 + 23/23 + 1/1 回归，全部复现为真；
> Dev 的两个关键发现也都**独立复现为真**，而且第二个发现的修法我做了**反向对照实验**证明它是**承重**的
> （去掉事务 → 50001 次读出现 118 次空视图；带事务 → 0 次）。
>
> 但有一个**必须上报的高危缺口**：**部署侧仍用超级用户 `base` 连接，RLS 在部署里等于没开**，
> 租户隔离在生产是 **fail open**。Dev 的 `02-app-role.sql` / `require_isolation` / `tenant-policies --check`
> 三件套都是**检测手段**，**没有任何一处把生产连接切到普通角色**，也没有任何一处把 `require_isolation`
> 接到启动流程。这违反需求文档的完成定义「RLS 策略在生产连接上生效」。
> **合并前必须处理**（最小修复见问题 #1）。

---

## 追溯

| AC | 实现 | 测试 | 结论 |
|----|------|------|------|
| **AC-S-007** RLS 兜底生效（应用层漏写过滤 → DB 层不返回他租数据） | `crates/platform-db/src/rls.rs:105-173`（policy 生成）、`crates/platform-db/src/tenant.rs:179-193`（`set_config(..., true)` = `SET LOCAL`）、`migrations/20261118000200_tenant_isolation.up.sql` | **判别实验自测**：应用角色不设 `app.tenant_id` → **0 行**；设 `tenant-a` → 2 行且全是 tenant-a；设 `tenant-b` → 2 行且全是 tenant-b；**超管对照 4 行**（证明「0 行」不是因为表空）；漏写租户列的写入被 `WITH CHECK` 拒绝（`new row violates row-level security policy`） | **PASS**（生产接线问题见 #1） |
| **AC-S-008** 上下文跨消息边界（缓存 key / 消息体带租户维度） | —— | **本期未实现**（Dev 已在追溯矩阵 §5 如实登记，设计摘要 §8 边界外） | **未实现（已登记）** |
| **AC-S-009** 连接池不串租户 | `crates/platform-db/src/tenant.rs:179-193`（每次访问都在**显式事务**内设租户） | **自测 50 轮交替**：`max_connections = 1`（必然复用同一条物理连接）→ tenant_a 查完立刻查 tenant_b → **串租户行数 = 0**；另：C-3 的 `tenant_context_does_not_leak_through_the_connection_pool` 真跑通过 | **PASS** |
| **AC-S-010** 业务模块拿不到裸连接 | `crates/platform-db/src/tenant.rs`（`pool` 字段是 `pub(crate)`，`TenantTx.tx` 私有）、`crates/platform-db/src/lib.rs:206-254`（源码扫描测试） | **自己跑那个扫描测试 + 做了变异验证**：临时注入 `pub fn leak_raw(&self) -> &PgPool` → 测试**真的失败**（`公开方法返回了裸连接类型 PgPool`）→ 证明扫描非空跑；还原后恢复通过 | **PASS** |
| **AC-S-011** 租户作用域表的 policy 由 manifest 统一生成，漏建能报错 | `crates/xtask/src/tenant_policies.rs`（生成/校验）、`crates/platform-db/src/rls.rs:161`（单事务应用）、`modules/sandbox/manifest.json` 的 `tenantScoped` | **破坏性验证**：手动 `DROP POLICY` → policy 数 = 0 → 重跑迁移（**不会**补建）→ `cargo xtask tenant-policies --check` → **报错并 exit=1**（`table sandbox_tenant_item has no isolation policy`）；随后 `tenant-policies` 恢复 | **PASS**（部署接线问题见 #3） |
| **AC-S-012** 租户上下文全链路一致（`X-Tenant-Id` → 不一致返 404） | —— | 全仓库 grep `X-Tenant-Id` **零命中**；`platform-server` 只有 `/health/live`、`/health/ready` 两条路由，无租户中间件 | **未实现（Dev 未登记）→ 问题 #4** |
| **AC-S-013** 配额用量计数可靠（落 DB、重启不丢、超限判定） | `crates/platform-db/src/quota.rs:93-156` | **自测**：`add_quota_usage` 5 → 再 7 → **12**；`set_quota_limit(10)` → `exceeded=true, remaining=-2`；**换新连接（等价重启）再读 → used=12, limit=10**（持久化成立）。但**无自动化用例** | **PASS（手工验证）/ 无自动化测试 → 问题 #6** |
| **AC-S-014** 依赖方向门禁覆盖新 crate | `crates/xtask/src/deps.rs:41-46`（`PLATFORM_CRATES` 含 `platform-db`）、`deps.rs:121-133`（`crates/` 下工作区成员**自动**纳管）、`deps.rs:437-446`（自动纳管断言） | **破坏性验证**：临时给 `platform-db` 加 `sandbox` 依赖 → `cargo xtask check-deps` → **exit=1**，点名 `platform-db → sandbox` 与 `xtask → platform-db → sandbox`；还原后 exit=0 | **PASS** |
| **AC-S-015** 依赖树与编译资源在预算内 | `crates/platform-db/Cargo.toml`（sqlx 不开 `macros`，`tenant_row!` 手写声明宏替代 derive） | **自测**：`cargo tree` 唯一 crate = **179**（7 个工作区成员 → 第三方 **172**，与基线 **+0**：`Cargo.lock` 从 234 → 235 条目，**只多了 `platform-db` 自己**）；`cargo tree -i sqlx-macros` → `nothing to print`（**不参与编译**）；`syn`/`quote`/`proc-macro2` **基线 Cargo.lock 里本来就有**（非本期引入）；**冷编译自测 61.6s / 峰值 968 MB**（< 1.5G ✓）；增量 `cargo check` **0.8s**（< 60s ✓） | **PASS** |

---

## 两个关键发现的独立复核

| # | Dev 声称 | 复核结果 | 证据 |
|---|----------|---------|------|
| **1** | **超级用户绕过 RLS，租户隔离 fail open**。开发库 `base` 是超级用户 → 绕过 RLS →「管理员连接看到 4 行，应用角色不设 `app.tenant_id` 看到 0 行」。已加 `02-app-role.sql`（组角色+登录角色）、`require_isolation` 的 `RLS_BYPASSED_BY_ROLE` 拒绝、`tenant-policies --check` 告警 | **现象复现为真**（判别实验完全一致）；**三个补救手段都真实存在且有效**；**但「部署侧是否切到普通角色」= 没有切** → 详见问题 #1 | **现象复现**（`base_test`，我自己的命令）：<br>`base_app` 不设租户 → `visible = 0`；`base_app` 设 `tenant-a` → 2 行（`c3-a`,`leak-a`）；设 `tenant-b` → 2 行（`c3-b`,`leak-b`）；**超管 `base` → 4 行**；`base_app` 写他租行 → `ERROR: new row violates row-level security policy`；写 `NULL` 租户 → 同错<br>**角色属性**：`base\|t\|t`（rolsuper=t, rolbypassrls=t）、`base_app\|f\|f`、`base_platform_app\|f\|f` → **普通角色确实非超管、无 BYPASSRLS** ✓<br>**`require_isolation` 实测**：超管 → `拒绝 code=RLS_BYPASSED_BY_ROLE`；`base_app` → 通过 ✓<br>**`tenant-policies --check`**：超管 → 打告警但 **exit=0**；`base_app` → 无告警 exit=0 ✓<br>**部署侧（未切）**：`compose.yaml:13 POSTGRES_USER: base`、`compose.yaml:85 DATABASE_URL: postgres://base:base@postgres:5432/base_dev`、`.env.example:21 DATABASE_URL=postgres://base:base@postgres:5432/base_dev`（`deploy/compose.prod.yaml` 的 `app` 服务通过 `env_file` 注入 → 同样是 `base`）、`deploy/compose.prod.yaml:40 POSTGRES_USER: ${POSTGRES_USER:-base}`；`deploy/compose.prod.yaml` **无 seed 挂载** → 生产库**连 `base_platform_app` 角色都不存在**；Helm 侧 `DATABASE_URL` 来自 Secret（`values.yaml:77`），**无角色指引、无建角色步骤**<br>**`require_isolation` 的调用方**：全仓 grep 只有 `crates/platform-db/src/{rls,lib}.rs` 与 `modules/sandbox/tests/c3_real_db.rs` → **没有任何生产路径调用它** |
| **2** | **策略应用曾有「全部拒绝」窗口**：`DROP POLICY` + `CREATE POLICY` 分两条语句 → 中间「表上没有 policy」→ 并发读写表现为「表是空的」假象（10 次并发跑 3 次偶发失败）。已改为**在一个事务里应用**，复测 10/10 全绿 | **现象复现为真**；**修法正确且承重**（我做了 Dev 没做的**反向对照实验**） | **代码**：`crates/platform-db/src/rls.rs:161-173` —— `db.pool.begin()` → 逐条 `execute(&mut *tx)` → `tx.commit()`，**整段在一个事务** ✓<br>**我的实验**（高速读者：单 psql 会话内 50000 次独立事务读 + 并发反复 `cargo xtask tenant-policies`）：<br>① **原始代码（带事务）** → 50001 次读，**空视图 0 次**，全部返回 2 行<br>② **反向对照**：临时把 `apply_policies` 的事务去掉（改 `execute(&db.pool)`）→ 50001 次读，**空视图 118 次** ← **证明窗口真实存在，且我的测量足够灵敏**<br>③ 还原代码（`git diff` 为空）→ 回到 ① 的结果<br>→ **事务就是关掉这个窗口的那一步，不是无关紧要的改动**<br>**生产是否仍有抖动**：① `cargo xtask tenant-policies` 路径 → 无空表假象；DDL 持 ACCESS EXCLUSIVE 锁，并发读**短暂阻塞**而非读到空表（我的 50001 次读里 **0 次错误、0 次空视图**）✓ ② 平台迁移 `20261118000200_tenant_isolation.up.sql` 里的 `DROP POLICY`/`CREATE POLICY` 仍是两条语句，但 **sqlx 默认在事务内执行迁移**（`sqlx-core-0.8.6/src/migrate/migrator.rs:52 no_tx: false`；`source.rs:127` 仅当文件首行是 `-- no-transaction` 才关闭事务 —— **我们的迁移文件没有该标记**）→ 原子 ✓ |

---

## 核心检查项结果

| # | 检查项 | Dev 声称 | 复核结果 | 证据 |
|---|--------|---------|---------|------|
| A1 | 应用层强制 `{tenant}` 占位符，缺则 `INVALID_QUERY` | 缺则报错 | **复现为真** | 自建探针（/tmp 独立 crate，path 依赖 `platform-db`）：<br>缺占位符 → `INVALID_QUERY`（`the statement must contain exactly one {tenant} placeholder, found 0`）<br>**两个**占位符 → `INVALID_QUERY`<br>无租户上下文 → `TENANT_CONTEXT_MISSING` |
| A2 | RLS fail closed（不设租户 → 0 行，不是报错也不是全表） | 0 行 | **复现为真** | 见上表 #1 |
| A3 | 漏写租户列的写入被 `WITH CHECK` 拒绝 | 被拒 | **复现为真** | `base_app` 写 `tenant_id='tenant-b'` → `ERROR: new row violates row-level security policy`；写 `tenant_id` 为 NULL → 同样被拒 |
| A4 | 串租户（连接池只开 1 条连接） | 无串租户 | **复现为真** | 50 轮交替（`max_connections=1`）→ **串租户行数 = 0** |
| A5 | `SET` vs `SET LOCAL` 对照证据 | 会串 | **机制确认** | `set_config(..., false)` 提交后连接仍带旧租户；`set_config(..., true)` 提交后归 0；实现用的是 `set_config('app.tenant_id', $1, true)`（`is_local=true` ≡ `SET LOCAL`），且 `SET` 不接受绑定参数 —— 用 `set_config` 是对的 |
| B | C-3 真实库层跑在 `base_test`，三条断言真被执行 | 3/3 通过 | **断言真执行**（做了变异验证）；**但存在假绿路径** → 问题 #2 | **变异验证**：把 `base_app` 改成 `ALTER ROLE base_app BYPASSRLS` → 三条用例**全部失败**（`RlsBypassed { role: "base_app" }`）→ 证明它们真的连库、真的断言；随后 `ALTER ROLE base_app NOBYPASSRLS` 还原<br>**真跑**：`DATABASE_URL_TEST=…/base_test cargo test -p sandbox --test c3_real_db -- --include-ignored` → `3 passed … finished in 0.75s`<br>**集成脚本**：`bash scripts/test-integration.sh` → **exit=0**，C-3 `3 passed … in 0.94s`，容器数量 `2 → 2` ✓ |
| C | 公开 API 无裸连接 + 机械保障测试真在扫 | 有机械保障 | **复现为真** | 公开面只有 `TenantDb::connect/from_pool/fetch_*/execute/begin`、`TenantTx::fetch_*/execute/commit/rollback/tenant_id`；`pool` 是 `pub(crate)`、`TenantTx.tx` 私有<br>**变异验证**：注入 `pub fn leak_raw(&self) -> &PgPool` → `no_public_api_returns_a_raw_connection` **FAILED**（`公开方法返回了裸连接类型 PgPool`）→ 还原后通过 |
| D | manifest `tenantScoped` → 平台统一生成 policy | 全部表都有 policy + `relrowsecurity=true` | **复现为真** | `pg_class` 实测：`platform_tenant_config\|t\|1`、`platform_tenant_quota\|t\|1`、`sandbox_tenant_item\|t\|1`（relrowsecurity / policy 数）<br>**破坏性验证**：删 policy → `--check` **exit=1** 报错 → 恢复 |
| E | 依赖方向门禁自动纳管新 crate | 必须检出 | **复现为真** | 破坏性验证 exit=1，还原后 exit=0（见追溯 #AC-S-014） |
| F | `platform-core` 改动纯新增 | +215 / 0 删除 | **复现为真** | `git diff --numstat`：`manifest.rs 138/0`、`manifest_schema.rs 76/0`、`mod.rs 1/0` → **215 增 0 删**；`platform-contracts` / `platform-contracts-macros` **一行未改**（numstat 为空） |
| F2 | manifest schema 与实现判定完全一致 | 一致 | **独立求值器对拍通过**（自写，未用 Dev 的 `pattern` 交叉测） | 自写 Python 正则求值器 + **独立复刻** Rust `is_sql_identifier` 语义，与 schema 的 `^[a-z][a-z0-9_]*$` 对拍 **20184 条语料**（含全量 1~2 字符穷举 + 2 万条随机 fuzz + 换行/制表/中文/注入串）：**表名不一致 0 条、列名不一致 0 条**<br>另：内置常量 `MANIFEST_JSON_SCHEMA` 与已发布的 `capabilities/schema/manifest.schema.json` **语义相等**（`json.loads` 后 `==` 为 True） |
| F3 | `tenantScoped` 与两份契约无冲突 | 无冲突 | **字段本身无冲突**；**但有 2 处口径张力** → 问题 #5、#7 | `extension-points v1` §6 破坏性清单里的「manifest 的必填字段或字段约束」**未触发**：顶层 `required` 仍是 `['id','name','version','enabled','permissions','menus','dependencies']`，`tenantScoped` 是**可选**字段；顶层 `additionalProperties:false` 下新增字段属**放宽**（原先带 `tenantScoped` 的 manifest 会被拒，现在接受）→ 非破坏性<br>错误码符合 `api-conventions` C6「大写 + 下划线」：`INVALID_QUERY` / `RLS_BYPASSED_BY_ROLE` / `RLS_POLICY_MISSING` / `TENANT_CONTEXT_MISSING` / `DATABASE_ERROR` / `RECORD_NOT_FOUND` ✓；列名 `tenant_id` 符合 C13「数据库列 snake_case」✓ |
| G | 回归 | 172/172、23/23、1/1、clippy 0、fmt 通过、xtask 全过、依赖树 +0、无 sqlx-macros | **全部复现为真** | Rust `cargo test --workspace -- --test-threads=2` → **172 passed / 0 failed / 8 ignored**（8 = 3 个 C-3 + 5 个文档 `ignore` 块）<br>前端 `pnpm -r test` → **23/23**（i18n 3 + auth 3 + hooks 3 + schema 3 + theme 2 + sdk 2 + ui 3 + shell 4）<br>E2E `bun run test:e2e` → **1/1**<br>`cargo clippy --workspace --all-targets -- -D warnings` → **0 告警**<br>`cargo fmt --all --check` → 通过<br>`cargo xtask capabilities --check` → 13 个文件一致；`check-deps` → 通过；`check-modules` → 通过；`tenant-policies --check` → 通过<br>依赖树：唯一 crate **179**（第三方 172，**+0**）；`cargo tree -i sqlx-macros` → `nothing to print` |

---

## 测试

- **Rust**: `cargo test --workspace -- --test-threads=2` → **172/172 通过**，0 失败，8 ignored
- **Rust（真实库）**: `cargo test -p sandbox --test c3_real_db -- --include-ignored` → **3/3 通过**（0.75s）
- **集成脚本**: `bash scripts/test-integration.sh` → **exit 0**，容器数量 2 → 2
- **前端**: `pnpm -r test` → **23/23**
- **E2E**: `bun run test:e2e` → **1/1**
- **Lint/格式**: clippy **0 告警**；`cargo fmt --all --check` 通过
- **门禁**: `capabilities --check` / `check-deps` / `check-modules` / `tenant-policies --check` 全过
- **冷编译自测**: **61.6 s / 峰值 968 MB**（预算 1.5G）；增量 `cargo check` **0.8 s**

---

## 问题（按严重度）

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **高** | **部署侧仍以超级用户 `base` 连接数据库 → RLS 在部署里等于没开，租户隔离 fail open。** Dev 提供的三件套（`02-app-role.sql` / `require_isolation` / `tenant-policies --check`）全是**检测手段**，**没有一处把生产连接切到普通角色**，`require_isolation` 也没有接到任何启动/部署路径。需求文档的完成定义明确写「**RLS 策略在生产连接上生效**」—— 当前不成立。 | `compose.yaml:13 POSTGRES_USER: base`；`compose.yaml:85 DATABASE_URL: postgres://base:base@postgres:5432/base_dev`；`.env.example:21` 同上（`deploy/compose.prod.yaml` 的 `app` 服务走 `env_file` → 同一个 `base`）；`deploy/compose.prod.yaml:40 POSTGRES_USER: ${POSTGRES_USER:-base}`；`deploy/compose.prod.yaml` **无 seed 挂载** → 生产库连 `base_platform_app` 角色都不存在；Helm `values.yaml:77` 的 `DATABASE_URL` 来自 Secret，**无角色指引、无建角色步骤**；`require_isolation` 的调用方 grep 结果只有 `crates/platform-db/src/{rls,lib}.rs` + `modules/sandbox/tests/c3_real_db.rs`（**无生产调用点**）<br>对照实测（`base_test`）：超管 `base` 看到 4 行、普通角色 `base_app` 不设租户看到 0 行 → **超管连接下 DB 层兜底确实完全失效** | **最小修复（不改镜像、不改架构）**：<br>① `POSTGRES_USER` **保持**超级用户，仅用于建库/迁移；<br>② **应用连接串改用普通角色** —— 在 `deploy/seed/02-app-role.sql` 基础上补一条生产用登录角色（口令由 `.env`/Secret 注入），把 `.env.example` 的 `DATABASE_URL` 改成该角色；<br>③ `deploy/compose.prod.yaml` 补 `deploy/seed` 的挂载或独立 init 步骤，否则角色不存在；<br>④ **把 `require_isolation` 接到应用启动流程**（超管即 fail-fast 退出），或至少接到部署后的健康检查；<br>⑤ Helm 侧同步（Secret 里给普通角色的连接串 + 建角色 Job） |
| **2** | 中 | **C-3 真实库用例在缺 `DATABASE_URL_TEST` 时是「假绿」**：测试体 `let Some(fixture) = setup().await else { return; };` → 未设环境变量时**提前 return**，断言一条都没跑，但结果报 **`ok`（3 passed）而不是 `ignored`**。任何形如 `cargo test --workspace -- --include-ignored` 的 CI 都会把「真实库层」当成通过。 | `modules/sandbox/tests/c3_real_db.rs:96-102`（`setup()` 返回 `None`）+ `:230-232`（`else { return; }`）<br>**实测**（不设 `DATABASE_URL_TEST`）：`cargo test -p sandbox --test c3_real_db -- --include-ignored` → `test result: ok. 3 passed; 0 failed; 0 ignored … finished in 0.00s`（**0.00s = 没做任何事**）；设上后 → `3 passed … finished in 0.75s` | 让「跳过」表现为**跳过而不是通过**：无 `DATABASE_URL_TEST` 时改用 `#[ignore]`-style 早退 + 明确的失败/跳过语义（例如用一个 `skip` 计数器或让测试在缺环境变量时 `panic!` 说明必须经 `scripts/test-integration.sh` 跑）；至少**不要报 `ok`** |
| **3** | 中 | **部署流程不调用 `cargo xtask tenant-policies`** → 业务模块表的 policy 在部署后**根本不存在**（直到有人手工跑一次）。Dev 已在验证报告 R2 与追溯矩阵 §5 登记为风险，但**没有解决**。与问题 #1 叠加：即使切了普通角色，没有 policy 的表在 RLS 打开状态下是「全部拒绝」，但若表没开 RLS 则是「完全无隔离」—— 两者都不可接受。 | 破坏性实测：删掉 `sandbox_tenant_item` 的 policy → 重跑迁移（`迁移：没有待应用的迁移`）→ **policy 数仍为 0**（迁移不会补建）→ 只有 `cargo xtask tenant-policies` 能补；`deploy/compose.prod.yaml` / `deploy/helm/base-platform/templates/migrate-job.yaml` **均无该步骤**（grep 无命中） | 把 `tenant-policies` 挂到迁移 Job 之后、app 启动之前（compose profile + Helm hook），并在 Helm 里做成 initContainer 或 post-upgrade hook；`--check` 失败应让部署**失败**而不是只打告警 |
| **4** | 中 | **AC-S-012（租户上下文全链路一致）完全未实现，且 Dev 的追溯矩阵没有把它登记为「未实现」**（矩阵 §1 只列了 AC-S-007 / AC-S-008）。该 AC 要求：`X-Tenant-Id` 请求头解析 → 数据仅来自该租户 → 请求头与令牌绑定租户不一致时返回 **404**。 | 全仓 grep `X-Tenant-Id` / `x-tenant-id` → **零命中**；`crates/platform-server/src/main.rs:231-232` 只有 `/health/live`、`/health/ready` 两条路由，无租户中间件、无 `/api/tenant/*`（`api-conventions` C1 把租户域划在 `/api/tenant`）<br>追溯矩阵 `feature-meta/traceability.md` §1 与 §5 均未出现 AC-S-012 | 要么本期补一个最小租户上下文中间件（解析头 → 注入 `RequestContext` → 令牌不一致返 404），要么在追溯矩阵里**如实登记为「未实现 / 移入下一期」**，不要留白（自验产物的诚实性是可审计性的前提） |
| **5** | 低 | **与 `api-conventions` v1 C3 的口径张力**：C3 写「业务模块**不得手工加 tenant_id 过滤条件**」、C7 写「数据范围过滤由平台**自动叠加**」；而本期的实现是**业务模块在自己写的 SQL 里写 `tenant_id = {tenant}`**（`modules/sandbox/backend/src/tenant_repository.rs:75-183`），平台只强制「占位符必须出现且租户值只能来自 ctx」。安全属性（模块无法伪造租户值）成立，但**契约文字与实现机制不一致**，第三方按 C3 理解会写出「不写过滤条件」的代码 —— 那在**平台未开 RLS**（问题 #1）时会直接跨租户泄漏。 | `api-conventions/v1.md` C3 第 4 条 + C7「数据范围过滤」行；`modules/sandbox/backend/src/tenant_repository.rs:76-78,89,137,153,174` 全部显式带 `{tenant}`；`crates/platform-db/src/query.rs:134-143`（缺占位符 → `INVALID_QUERY`） | 二选一并落文：① **修订 api-conventions C3/C7**，把「平台强制占位符」写成合规形式（租户值由平台绑定、模块不得自造租户值）；② 或保留契约原意，在平台层**自动注入** WHERE 条件。**在问题 #1 未修前，现状有真实泄漏风险** |
| **6** | 低 | **配额（AC-S-013）无自动化用例**：`quota.rs` 只有 1 条纯结构体单测，「计数落库 / 重启不丢 / 超限判定 / 跨租户写入被拒」全靠手工验证。 | `crates/platform-db/src/quota.rs:159-182`（仅 `quota_usage_reports_exceeded_and_remaining` 一条纯函数单测）；`modules/sandbox/tests/c3_real_db.rs` 无配额用例 | 补一条真实库用例：加用量 → 换连接再读（等价重启）→ 断言 `used` 不变；再用 `tenant-b` 的 ctx 写 `tenant-a` 的账，断言被 `WITH CHECK` 拒 |
| **7** | 低 | `extension-points v1` §1 枚举的平台 crate 仍是 3 个（`platform-core` / `platform-contracts` / `platform-contracts-macros`），未更新为含 `platform-db`；实现在 `crates/xtask/src/deps.rs:41-46` 已加。属于**收紧**（契约 §1 的原话就是「检查覆盖全部平台 crate」），**不构成冲突**，只是契约文本滞后。 | `BA/contracts/extension-points/v1.md` §1；`crates/xtask/src/deps.rs:41-46`（`PLATFORM_CRATES` 已含 `platform-db`）+ `deps.rs:121-133`（`crates/` 自动纳管） | 契约升版时把 `platform-db` 补进 §1 的枚举 |
| **8** | 低 | `deploy/seed/02-app-role.sql` 里 `CREATE ROLE base_app LOGIN PASSWORD 'base'` **硬编码弱口令**；且该脚本挂在 `docker-entrypoint-initdb.d`，**只在数据卷首次初始化时执行** —— 已有卷的环境升级后不会补建该角色。 | `deploy/seed/02-app-role.sql:29`；`compose.yaml:42`（`./deploy/seed:/docker-entrypoint-initdb.d:ro`）；`deploy/compose.prod.yaml` 无 seed 挂载 | 口令改由 `.env`/Secret 注入；建角色做成**幂等的部署步骤**（不依赖 initdb），与问题 #1 的修复一并处理 |

---

## 未覆盖 / 无法验证

| 项 | 说明 |
|----|------|
| **AC-S-008（缓存 / 消息链路的租户维度）** | 本期未实现（设计摘要 §8 边界外，Dev 已登记）。**未验证**，不计入通过 |
| **AC-S-012（HTTP 租户上下文全链路）** | 未实现（问题 #4）。**未验证** |
| **覆盖率数字** | 低配机无覆盖率工具链，**未测**。新增代码的分支覆盖只能靠逐条用例推断，无工具数字 |
| **真实负载性能** | 未压测。「每次访问多一次 `BEGIN`/`COMMIT`」的实际开销**未量化**（设计上的刻意取舍） |
| **镜像实构建 / 真实 K8s 集群** | 本机内存不足，未构建镜像、未在真集群跑 Helm 渲染与迁移 Job；问题 #1 / #3 的修复**必须在真实部署环境复验** |
| **`base_dev`（开发库）** | 按纪律未做任何写操作（仅一次只读元数据查询用于自证未触碰）。开发库上的策略/角色状态**未验证** |
| **`cargo xtask tenant-policies` 的并发重放（生产节奏）** | 我验证了「策略应用不产生空表窗口」与「并发读不报错」；**生产真实重放频率下的锁等待时长**未测 |
| **多节点 / 跨副本的租户上下文** | 单进程单库场景；连接池跨副本行为**未验证** |

---

## QA 处置建议

1. **合并前必须处理问题 #1**（部署连接串切普通角色 + `require_isolation` 接启动流程）—— 否则租户隔离在生产是 fail open，需求完成定义不成立。
2. 问题 #3（部署跑 `tenant-policies`）与 #1 同批处理，否则改完角色仍然没有 policy。
3. 问题 #2（假绿）与 #4（AC-S-012 未登记）建议在本轮内修掉 —— 前者会让后续回归失去 C-3 这道护栏，后者是自验产物的诚实性问题。
4. 问题 #5 的契约口径请 BA 裁定（改契约 or 改实现），不要留给下游模块自行解读。
