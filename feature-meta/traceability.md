# REQ-006 追溯性矩阵

- **需求**: REQ-006 多租户与隔离
- **分支**: `feature/REQ-006`
- **基线**: `develop` @ `806bb66`（REQ-002 工程基线 + REQ-003 扩展点框架 + REQ-004 AI 引导体系 + REQ-005 交付基线已合并）
- **设计依据**: `BA/demands/REQ-006-tenant/design-summary.md`（架构决策 D1–D8 已定死，本轮照它做）
- **契约**: api-conventions v1、extension-points v1（已 ratified）
- **开发者**: myg133-dev-1
- **更新时间**: 2026-11-18
- **本轮改动**: 37 文件 / +2860 -20 行（vs BA 分配提交 `31c692b`）
- **第二轮（QA Pre-merge 复核后）改动**: **35 文件 / +1548 -158 行**（vs 第一轮的 `668d357`）；
  **`crates/platform-core/` 一行未改**，`crates/platform-db/src` 只**新增** `scopes.rs`
  与 `IsolationReport::table_problems`（核心查询逻辑一行未改）

图例：**PASS** = 已实现且有自动化或可复现的验证证据；**未验证** = 没有证据，如实标注。

---

## 1. 自有 AC

| AC | 内容 | 实现位置 | 验证方式与证据 | 结论 |
|----|------|----------|----------------|------|
| AC-S-007 | RLS 兜底生效：故意在应用层漏写 `tenant_id` 过滤，数据库仍不返回他租数据 | `crates/platform-db/src/rls.rs`（policy 生成）、`migrations/20261118000200_tenant_isolation.up.sql`（平台表 policy）、`modules/sandbox/tests/c3_real_db.rs`（真实库层）、`crates/platform-db/src/tenant.rs`（租户入口）、**`crates/platform-server/src/isolation.rs`（第二轮：启动自检）** | ① **真实库实测**（`base_test`）：策略 `USING (tenant_id = current_setting('app.tenant_id', true))` + `WITH CHECK` 同式；② **管理员连接（超级用户，绕过 RLS）看到 4 行**，**应用角色不设 `app.tenant_id` 看到 0 行** —— 对照组证明「0 行」不是因为表里没数据，而是 RLS 挡住；③ 应用角色设了 `app.tenant_id='tenant-a'` 只看到 tenant-a 的 2 行；④ 漏写租户条件的写入被 `WITH CHECK` 拒绝（`new row violates row-level security policy`）；⑤ `require_isolation` 自检：角色不绕过 RLS + 每张声明的表都开了 RLS 且有 policy，否则 `DbError::RlsBypassed` / `DbError::MissingPolicy`；⑥ **失败路径有明确错误码**：`RLS_BYPASSED_BY_ROLE` / `RLS_POLICY_MISSING` / `TENANT_CONTEXT_MISSING` / `INVALID_QUERY` / `DATABASE_ERROR`；⑦ **第二轮：`require_isolation` 接到生产启动路径** —— 应用用超级用户连 → **拒绝启动（退出码 3）**，用普通角色 → 启动并就绪（实测见验证报告 §8.2） | **PASS** |
| AC-S-008 | 上下文跨消息边界：异步消息与缓存 key 均带租户维度 | —— | **本期未实现**：设计摘要 §8「本期交付边界」**不含**缓存/消息链路（那是租户上下文中间件，属 REQ-019 API 治理）。本轮交付的是**数据库层**的租户上下文贯穿（`RequestContext` → `SET LOCAL app.tenant_id` → RLS），缓存 key / 消息体的租户维度**未做** | **未实现（如实登记，见 §5）** |

## 2. 交付清单（A–F）

| 交付项 | 实现位置 | 验证方式与证据 | 结论 |
|--------|----------|----------------|------|
| **A** `crates/platform-db`：租户感知的数据访问 | `crates/platform-db/src/{lib,tenant,query,error,quota,rls}.rs` | ① 每次访问在**显式事务**内设租户：`TenantDb::begin` → `set_config('app.tenant_id', $1, true)`（= `SET LOCAL` 语义）；② 单次操作（`fetch_all`/`fetch_optional`/`fetch_one`/`execute`）与多操作（`begin` → `TenantTx` → `commit`/`rollback`）两种形态；③ 公开 API **拿不到裸连接**：`no_public_api_returns_a_raw_connection` 源码扫描测试（本 crate 全部 `.rs` 的公开签名与公开字段里不出现 `PgPool`/`PgConnection`/`Transaction<`/`&mut PgConnection`），另有 `no_public_api_is_named_after_a_raw_connection`；④ 错误类型沿用 `CoreError` 风格：`DbError` + `code()` 稳定错误码；⑤ crate 级文档写清「为什么是 `SET LOCAL` 而不是 `SET`」（连接池串租户风险 + `SET` 不接受绑定参数）；⑥ 配额：`add_quota_usage`（计数**写 DB**，`INSERT … ON CONFLICT DO UPDATE`）/ `quota_usage`（用量 + 限额）/ `set_quota_limit` | **PASS** |
| **B** 迁移与 RLS | `migrations/20261118000200_tenant_isolation.{up,down}.sql`、`crates/platform-db/src/rls.rs`、`crates/xtask/src/tenant_policies.rs`、`modules/sandbox/migrations/20261118000300_sandbox_tenant_item.{up,down}.sql` | ① 平台表建 `ENABLE ROW LEVEL SECURITY` + policy（实测 `relrowsecurity = t`、policy 数 = 1）；② 业务表的 policy **由 manifest 的 `tenantScoped` 统一生成**（模块的迁移文件里**只有建表**，没有 policy）；③ **漏建 = 完全无隔离的检查**：`cargo xtask tenant-policies --check` → 逐表核对 `relrowsecurity` 与 policy 数，缺一即失败；④ schema 同步：`manifest_schema.rs` 的内置常量与 `capabilities/schema/manifest.schema.json` 逐字节一致（平台核心测试守着） | **PASS** |
| **C** C-3 契约检查叠加真实库层 | `modules/sandbox/tests/c3_real_db.rs`（真实库层）、`modules/sandbox/tests/contracts.rs`（内存层，**保留未改**） | 真实库层三条断言全通过：① tenant_a 的 `list()` 只返 tenant_a 的行；② tenant_a 的 `find(tenant_b 的 id)` 返回 `None`；③ **绕过应用层**（平台内部连接、不设 `app.tenant_id`）→ **0 行**（对照：管理员连接 4 行）；另有 **串租户用例**（tenant_a 查完立刻用**同一个连接池**查 tenant_b，连接池只开 1 条连接 → 必然复用同一条物理连接）；**第二轮：缺 `DATABASE_URL_TEST` 时判为失败（不再假绿）** —— 实测 `3 failed` + 退出码 101（不是 "3 passed"），并有护栏用例守着 | **PASS** |
| **D** 示例模块 | `modules/sandbox/backend/src/tenant_repository.rs`、`modules/sandbox/manifest.json`、`modules/sandbox/migrations/*`、`modules/sandbox/Cargo.toml` | ① 沙箱新增 `sandbox_tenant_item` 表（`tenant_id` 作用域，`tenantScoped` 声明）；② 仓储实现走 `platform-db` 的 `TenantDb` 租户入口（语句里带 `{tenant}`）；③ 沙箱是「业务方照着抄」的标准范例：注释写明三条规矩（租户只从 `ctx` 取 / 每条语句带 `{tenant}` / 拿不到裸连接） | **PASS** |
| **E** 能力索引与文档 | `crates/xtask/src/catalog.rs`、`AGENTS.md`、`capabilities/index.json`、`docs/dev-environment.md`、`CHANGELOG.md` | ① 能力目录新增 `tenant_scoped_db`（`already_provided`），`db_access_layer` 从 `not_yet_provided` 换成 `crud_scaffolding`（口径如实）；② `cargo xtask capabilities --check` → **13 个文件与源码一致**；③ `AGENTS.md` 的「已有能力 / 尚未提供」两段与能力目录**逐行一致**（`agents_doc_matches_catalog` 测试守着）；④ **硬规则新增第 4 条**：数据访问必须走 `platform-db` 的租户作用域入口，不得直连池；⑤ `docs/dev-environment.md` 补依赖树变化（91 → 175 → 179，第三方 **172 不变**）与冷编译实测 | **PASS** |
| **F** 依赖方向门禁 | `crates/xtask/src/deps.rs`（`PLATFORM_CRATES` + 自动纳管测试）、`.github/CODEOWNERS`、`scripts/core-change-detect.sh` | ① `platform_db_is_covered_by_the_platform_crate_rules` 断言 `platform-db` 在平台 crate 集合里（自动纳管生效）；② **实测造违规**：临时给 `platform-db` 加一条指向 `modules/sandbox` 的依赖 → `cargo xtask check-deps` **退出码 1** 并点名「平台 crate `platform-db` 反向依赖了业务模块 `sandbox`（依赖链：platform-db → sandbox）」→ 验证后**已还原**（还原后 `check-deps` 退出码 0）；③ `CODEOWNERS` 与核心路径清单收录 `platform-db`（业务模块的数据访问都走它 → 改动影响所有业务模块 → 属核心） | **PASS** |

## 2.1 第二轮：QA Pre-merge 复核（PASS-WITH-RISK）后的修复

> QA 报告：`feature-meta/qa-report.md`。本轮**只修 3 项**，不扩大 scope。
> `platform-core` 一行未改；`platform-db/src` 的**核心查询逻辑**（租户入口 / RLS 兜底）一行未改。

| 项 | 实现位置 | 验证方式与证据 | 结论 |
|----|----------|----------------|------|
| **高 1 部署侧切普通角色** | `compose.yaml`（`app` / `migrate` / `policies` 三个服务）、`deploy/compose.prod.yaml`（`app` / `migrate` / `policies` / `app-role` 四个服务）、`.env.example`（`APP_DATABASE_URL` + `MIGRATION_DATABASE_URL`）、`deploy/helm/base-platform/{values.yaml,templates/app-role-job.yaml,templates/policies-job.yaml,templates/migrate-job.yaml,templates/deployment.yaml}`、`scripts/prod-preflight.sh`、`deploy/RUNBOOK.md` §0.1 / §3.4 | ① **普通角色不设 `app.tenant_id` → 0 行**、**超级用户对照组 → 4 行**（`base_test` 实测）；② **应用启动自检**：超级用户连接 → **拒绝启动（退出码 3）**；普通角色 → 启动且 `/health/ready` = ready；③ `BASE_ALLOW_SUPERUSER=true` 只在本地放行，`BASE_ENV=prod` 配它 → 启动即失败（退出码 2）；④ `prod-preflight.sh` 硬闸：应用连接串用户是超级用户 → 拒绝部署；⑤ compose 两个新服务（`app-role` 建角色、`policies` 建策略）**在容器里实测通过** | **PASS**（Helm 侧未在真集群渲染验证，见 §5） |
| **中 1 C-3 假绿** | `modules/sandbox/tests/c3_real_db.rs`（夹具改「缺环境变量 = 失败」+ 新增护栏用例） | ① 不设 `DATABASE_URL_TEST` 跑 `--include-ignored` → **3 failed / 退出码 101**（**不是 "3 passed"**）；② 护栏用例每次 `cargo test --workspace` 都跑（子进程必须非零退出 + `0 passed` + 无 "3 passed"）；③ 设上环境变量 → 真跑通过（0.72s，集成脚本 exit 0） | **PASS** |
| **中 2 策略进部署流程** | `crates/platform-server/src/policies.rs`（`base tenant-policies [--check]`）、`compose.yaml` / `deploy/compose.prod.yaml` 的 `policies` 服务、`deploy/helm/base-platform/templates/policies-job.yaml`、`Dockerfile`（镜像带 manifest）、`deploy/RUNBOOK.md` §0.1 / §3.2 | ① `--check` 通过时 exit 0；**破坏性验证**：删掉一条 policy → `--check` **exit 1** 并点名表；② `base tenant-policies` 能补建（幂等）；③ 顺序：迁移（`-5` / `migrate` 服务）→ 策略（`-4` / `policies` 服务）→ 应用 | **PASS** |

## 3. 设计决策 D1–D8 的符合性

| 决策 | 要求 | 实现与证据 | 结论 |
|------|------|------------|------|
| D1 | 新建 `crates/platform-db`，**不改 `platform-core`**；依赖方向 `platform-core ← platform-db ← 业务模块` | `platform-db` 依赖 `platform-core` ✓；`sandbox` 依赖 `platform-core` + `platform-db` ✓；`check-deps` 实测无反向依赖 ✓。**唯一例外**：`platform-core` 的 **manifest schema 扩展**（G1 的必要条件，见 §4） | **PASS（含 1 处必要例外，已说明）** |
| D2 | sqlx 的 `macros` feature 关闭，用 `query_as` + `FromRow`（运行时检查）；不引入 `syn`/`quote`/`proc-macro2` | `cargo tree` 实测 **`sqlx-macros` 不参与编译** ✓；`FromRow` 用 `platform_db::tenant_row!` 声明宏手写实现（无 proc-macro）✓；`syn`/`quote`/`proc-macro2` 是 REQ-002 起就有的（serde_derive / thiserror-impl / clap_derive），**不是本期引入** ✓ | **PASS** |
| D3 | 必须用 `SET LOCAL app.tenant_id`（事务局部），绝不用普通 `SET` | 实现用 `set_config('app.tenant_id', $1, true)`（`is_local = true` **等价于 `SET LOCAL`**，且能走绑定参数 —— `SET` 命令不接受 `$1`）✓；crate 文档与 `AGENTS.md` 写明原因 ✓；**串租户用例**实测：连接池只开 1 条连接 → tenant_a 查完立刻查 tenant_b，**不返回 tenant_a 的数据** ✓ | **PASS** |
| D4 | 不向业务模块暴露「未加租户条件的连接」 | 公开 API 全部带 `&RequestContext` ✓；裸连接类型不出现在任何公开签名/字段（源码扫描测试）✓；`TenantDb` 的 `pool` 字段是 `pub(crate)` ✓ | **PASS** |
| D5 | RLS policy 用 `current_setting('app.tenant_id', true)`（fail closed） | 平台表与业务表的 policy 实测：`USING ((tenant_id = current_setting('app.tenant_id'::text, true)))` ✓；漏设时**查不到任何行**（0 行，不是报错）✓ | **PASS** |
| D6 | 业务模块的表通过 manifest 的 `tenantScoped` 段声明，平台统一生成 policy | `manifest.json` 新增 `tenantScoped` ✓；模块迁移文件里**没有** policy ✓；`cargo xtask tenant-policies` 统一生成 ✓ | **PASS** |
| D7 | L2/L3 只留接口不实现；**不把 L1 的假设写死**（schema 名从配置来） | `DbOptions.schema`（默认 `public`，可配）✓；`policy_statements(spec, schema)` 的 schema 是**参数** ✓；`TenantDb::table()` 按配置的 schema 拼表名 ✓。**L2/L3 本期不实现**（设计如此） | **PASS** |
| D8 | C-3 契约检查**叠加**真实库层，内存层保留 | 内存层（`#[platform_module_contract]`）**未改动** ✓；新增真实库层（`c3_real_db.rs`）三条断言 ✓ | **PASS** |

## 4. 对 `platform-core` 的改动（CODEOWNERS 门禁范围）

| 文件 | 改动 | 为什么必须改 |
|------|------|--------------|
| `crates/platform-core/src/extension/manifest.rs` | +138 行（**纯新增**）：新增 `TenantScopedSpec` 结构、`Manifest.tenant_scoped` 字段、字段校验（表名/列名必须匹配 `^[a-z][a-z0-9_]*$`）、6 条新单测 | 需求 G1「业务模块的表通过 manifest 的 `tenantScoped` 段声明，平台统一生成 policy」——**声明段必须是 manifest 的一部分**，否则平台无处可读模块的租户作用域声明 |
| `crates/platform-core/src/extension/manifest_schema.rs` | +76 行（**纯新增**）：schema 增加 `tenantScoped` 段 + 2 条新单测 | manifest 的 schema 是**对外发布**的（第三方拿它做校验）—— schema 与平台实现必须完全一致，否则第三方按 schema 校验的结果与平台判定不同（`extension-points v1` §5 的硬要求） |
| `crates/platform-core/src/extension/mod.rs` | +1 行：导出 `TenantScopedSpec` | 供 `platform-db` / `xtask` 复用同一套类型 |

**影响说明**：改动是**纯增量**的 —— 既有字段、既有校验、既有 schema 约束**一行未删**；
`deny_unknown_fields` 仍生效（新字段是已知的）；既有 manifest 全部照常解析。
破坏性变更清单（`extension-points v1` §6）里**没有**这一项
（5 个 trait 的签名、`discover()` 签名、manifest 的**必填字段**都没动 ——
`tenantScoped` 是**可选**字段）。

## 4.1 顺带落地：REQ-005 遗留技术债 #4（scope 之外的一小步）

`crates/xtask/src/modules.rs`：`migrations/` 下没有 `*.sql` → `check-modules` 判失败
（REQ-005 登记的债务写明「DB 层落地后升级」→ 本轮兑现）。
**不在任务交付清单 A–F 内** —— 属主动扩大的一小步 scope，已在验证报告 §5.1 / 风险 R7 登记。

## 5. 未验证 / 不成立的部分（如实登记）

| 项 | 原因 | 影响 |
|----|------|------|
| **AC-S-008（上下文跨消息边界）** | 本期设计边界不含缓存/消息链路（`design-summary` §8）；本轮只做了**数据库层**的租户上下文贯穿 | 该 AC **未实现**，需要后续需求（租户上下文中间件 / REQ-019）补齐 |
| **`base_dev`（开发库）上的真实验证** | 本轮的数据库验证全部跑在 `base_test`（`scripts/test-integration.sh` 的白名单闸门只允许重置 `base_test`）；**开发库未被写入**（纪律要求只读） | 开发库上还没有应用角色的策略（需手工跑一次 `base tenant-policies`）；**且存量开发卷早于 `02-app-role.sql` 创建 → `base_app` 在 `base_dev` 上缺表权限**（补建命令见 RUNBOOK §3.4.3） |
| **K8s / 私有化部署里的策略生成** | ✅ **第二轮已接上**：`base tenant-policies` 子命令 + compose `policies` 服务 + Helm `policies` hook Job（迁移之后）；**Helm 侧未在真集群渲染验证**（本机无 helm 二进制） | compose 侧已实测通过；Helm 侧只做了模板静态审阅 |
| **覆盖率数字** | 本轮未跑覆盖率工具（低配机 + 无覆盖率工具链） | 角色规范里的「覆盖率 ≥ 80%」这条**未验证**（新增代码的每个分支都有对应用例，但无工具数字） |
| **真实负载下的性能** | 未做压测；每次访问多一次 `BEGIN`/`COMMIT` 往返 | 性能影响未量化（设计上的刻意取舍，见 `platform-db` 的 crate 文档） |
| **AC-S-012（HTTP 租户上下文全链路）** | 本期未实现（QA 报告问题 #4：`X-Tenant-Id` 全仓零命中，平台服务只有两条健康路由） | 该 AC **未实现** —— 本轮只修「部署侧切普通角色 / C-3 假绿 / 策略接线」3 项，未纳入；需后续需求补齐 |
| **契约口径张力（QA 报告 #5）** | `api-conventions v1` C3/C7 写「业务模块不得手工加 tenant_id 过滤条件 / 平台自动叠加」，本期实现是「模块在自己写的 SQL 里写 `tenant_id = {tenant}`，平台强制占位符」 | 契约文字与实现机制不一致，需 BA 裁定（改契约 or 改实现）；**本轮未处理** |
