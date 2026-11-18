# REQ-006 状态

- **需求**: REQ-006 多租户与隔离
- **分支**: `feature/REQ-006`
- **基线**: `develop` @ `806bb66`（REQ-002 + REQ-003 + REQ-004 + REQ-005 已合并）
- **状态**: **待验证**（QA Pre-merge 复核 **PASS-WITH-RISK** → 第二轮修复完成 → **复核结论：首轮 3 项全部复验为真已修，新发现 2 个中危** → 第三轮修复完成 → 等待复核；本状态由 Dev 按任务要求置位，**最终状态机由 BA 推进**）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`
- **设计依据**: `BA/demands/REQ-006-tenant/design-summary.md`（D1–D8 照做）

## 本轮交付

租户隔离做**双保险**：应用层每次查询都带租户条件，数据库层用 PostgreSQL RLS 再兜一遍。

| 交付项 | 位置 |
|--------|------|
| 新建 `crates/platform-db`：租户作用域入口（事务内 `SET LOCAL app.tenant_id`）+ 配额计数/限额查询 | `crates/platform-db/src/` |
| 平台表与业务表的 RLS 策略（业务表由 manifest 的 `tenantScoped` 段**统一生成**） | `migrations/20261118000200_tenant_isolation.up.sql`、`crates/platform-db/src/rls.rs`、`cargo xtask tenant-policies` |
| manifest 新增 `tenantScoped` 段 + JSON Schema 同步（**`platform-core`，CODEOWNERS 保护目录**） | `crates/platform-core/src/extension/manifest.rs`、`manifest_schema.rs` |
| C-3 契约检查**叠加真实库层**（内存层保留）+ 串租户用例 | `modules/sandbox/tests/c3_real_db.rs` |
| 沙箱模块升级为「业务方照着抄」的标准范例 | `modules/sandbox/backend/src/tenant_repository.rs` |
| 能力索引 / 文档 / 依赖门禁收录 `platform-db` | `crates/xtask/src/{catalog,deps,tenant_policies}.rs`、`AGENTS.md`、`docs/dev-environment.md`、`.github/CODEOWNERS` |

## 自验结论

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **183/183**（第二轮 182，**+1** = 配额防假绿护栏；第一轮 172 个全绿，**只增不减**）
- 真实库用例（默认 `#[ignore]`，由集成脚本 `--include-ignored` 跑到）✅ **6/6**（C-3 真实库层 4 条 + 配额真实库层 2 条）
- `bash scripts/test-integration.sh` ✅ **exit 0**（重置 `base_test`，容器数量 2 → 2；**连跑 2 次全绿**）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** / `cargo fmt --all --check` ✅
- `pnpm -r test` ✅ **23/23** / `bun run test:e2e` ✅ **1/1**
- `cargo xtask capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅
- **依赖方向门禁实测**：临时给 `platform-db` 加反向依赖 → `check-deps` **退出码 1** 并点名 `platform-db → sandbox` → **验证后已还原**
- **租户隔离三层** ✅ 应用层（`{tenant}` 强制）/ **RLS fail closed**（不设 `app.tenant_id` → **0 行**，管理员连接对照 **4 行**）/ **串租户**（同一条物理连接，tenant_a 查完查 tenant_b → 看不到 tenant_a 的行）
- **冷编译实测**：**63.7s / 66.0s**，峰值 **925 MB / 940 MB** → **< 1.5G，`jobs` 保持 3**（无需下调）
- **依赖树**：175 → **179**（工作区成员 6 → 7；**第三方 172，+0**；`sqlx-macros` 不参与编译）
- **新增第三方依赖：无**

## 改动了 `platform-core`（CODEOWNERS 门禁范围）

- `manifest.rs` +138 / `manifest_schema.rs` +76 / `mod.rs` +1 —— **纯新增，0 删除**
- **理由**：需求 G1 要求模块的租户作用域声明写在 manifest 里，平台才有地方读；
  manifest 的 schema 是**对外发布**的，必须与平台实现完全一致
- **门禁影响**：会触发人工评审（预期内）；`platform-contracts` / `platform-contracts-macros` **一行未改**

## 本轮修掉的一个真问题

策略应用原先把 `DROP POLICY` + `CREATE POLICY` 分两条语句执行 →
中间有「表上没有 policy」的窗口（RLS 变成全部拒绝），并发访问时表现为
「表是空的」假象（10 次并发跑 3 次偶发失败）。**已改为在一个事务里应用**，
复测 10/10 全绿。详见验证报告 §4。

## 未验证 / 未完成（如实登记）

1. **AC-S-008（缓存 / 消息链路的租户维度）未实现** —— 本期设计边界不含（`design-summary` §8）
2. **AC-S-012（HTTP 租户上下文全链路）未实现** —— QA 报告问题 #4；**本期三轮均未纳入**
   （需后续需求 REQ-019 补齐）
3. **Helm Chart 未在真集群渲染验证** —— **第三轮已用临时下载的 helm v3.16.3 跑过
   `helm template`**（渲染核对：7 类、Deployment 无 `envFrom`、无 `MIGRATION_DATABASE_URL`、
   三个 hook Job 的权重与 env 键），但**仍未上真集群**：那三个 hook Job 的实际执行顺序、
   Secret 预置、`psql` 镜像拉取**未在真集群验证**（compose 侧的两个新步骤**已**在容器里实测通过）
4. **镜像未构建** —— 本机内存不够；`Dockerfile` 里「摘 manifest 到 `/app/modules`」那步未实跑
5. **覆盖率未测** —— 低配机无覆盖率工具链
6. **`base_dev` 的就绪探针 503** —— **更正（依据 QA 复核报告问题 #3）**：此前记的
   「`base_dev` 缺表权限」**与实测不符** —— `base_dev` 的默认权限**已落地**
   （`pg_default_acl` 里 `base_platform_app=arwd/base`），`base_app` 对
   `_sqlx_migrations` / `platform_meta` **均有 SELECT**；503 的真实原因是
   **迁移未应用**（`pending migrations: [20261118000200]`），跑一次
   `cargo run -p platform-server --bin base -- migrate up` 即可，属常规开发态、
   **不构成缺陷**。**本轮仍未对 `base_dev` 做任何写操作**（只跑 SELECT 与只读探针）
7. **真实负载下的性能未量化** —— 每次访问多一次 `BEGIN`/`COMMIT`（设计取舍）
8. **契约口径张力（QA 报告 #5）** —— 需 BA 裁定（改契约 or 改实现）；**第三轮未处理**
   （只把「`detail` 英文 / 面向用户中文由前端 i18n 渲染」这条口径登记到引用处
   `docs/contracts/README.md`，**ratified 契约快照零改动**）

## 第二轮修复（QA Pre-merge 复核 PASS-WITH-RISK 后）

| 项 | 结果 |
|----|------|
| **高 1 部署侧切普通角色** | ✅ compose（开发 / 私有化）、Helm、`.env.example`、`prod-preflight.sh`、RUNBOOK 全部改成**普通角色**连接；`migrate` / `policies` 保留超级用户；**`require_isolation` 接到应用启动路径**（超级用户 → 拒绝启动，退出码 3；`BASE_ALLOW_SUPERUSER` 仅本地放行） |
| **中 1 C-3 假绿** | ✅ 缺 `DATABASE_URL_TEST` → **3 failed / 退出码 101**（不再是 "3 passed"），并加护栏用例 |
| **中 2 策略进部署流程** | ✅ 新增 `base tenant-policies`；compose `policies` 服务 + Helm `policies` hook Job（**迁移之后**） |
| 回归 | Rust **182/182**（+10）/ 前端 **23/23** / E2E **1/1** / clippy 0 / fmt 通过 / xtask 全过 / 依赖树 **179（+0）** |

详见验证报告 §8。

## 第三轮修复（QA 复核 PASS-WITH-RISK 后的 2 个中危 + 3 个低级项）

| 项 | 结果 |
|----|------|
| **中 1 应用容器不再拿到超管凭据** | ✅ compose 的 `app` 服务**删掉 `env_file: .env`**（逐个列 `environment:`）；Helm 的 Deployment**删掉 `envFrom`**（逐个 `env:` + `valueFrom.secretKeyRef`）；migrate Job 也不再整包引用 Secret。**实测**：compose 渲染的 app 块无 `env_file`、无 `MIGRATION_DATABASE_URL`；`helm template` 的 Deployment 无 `envFrom`、无 `MIGRATION_DATABASE_URL`（只出现在三个 hook Job 里） |
| **中 2 补建命令救得了存量表** | ✅ 三处建角色的地方（`02-app-role.sql` / compose `app-role` / Helm `app-role` Job）都补上 `GRANT … ON ALL TABLES IN SCHEMA public`（**与 `ALTER DEFAULT PRIVILEGES` 并存**：一个管存量、一个管增量）。**实测**：`base_test` 撤销 → `f` + `permission denied` → **旧脚本补建仍 `f`（对照组）** → 新脚本补建 → `t` 且能读 → 复原后快照逐行一致、`base_app` 仍有 SELECT。RUNBOOK §3.4.3 重写（含可直接复制执行的 SQL） |
| **L-a 弱口令标注** | ✅ `02-app-role.sql` / `.env.example` 加「仅开发环境」注释；RUNBOOK §3.4.2 加口令要求表 |
| **L-b 配额自动化用例** | ✅ 新增 `crates/platform-db/tests/quota_real_db.rs`：计数 + 超限判定 + **重启后计数仍在**（换全新连接池）+ 隔离断言 + 防假绿护栏（**经变异验证承重**） |
| **L-c 契约口径** | ✅ 登记在**引用处** `docs/contracts/README.md`（`detail` 英文 / 中文由前端 i18n 渲染）；**ratified 契约快照零改动** |
| 回归 | Rust **183/183**（+1）/ 真实库 **6/6** / 前端 **23/23** / E2E **1/1** / clippy 0 / fmt 通过 / xtask 全过 / 依赖树 **179（+0）** |

详见验证报告 §9。

## 请派 QA 复核

重点看验证报告 §7 的 5 条（第一轮）、**§8**（第二轮修复）与 **§9**（本轮修复）：
逐处对照表、★ 实测输出（0 行 / 4 行 / 退出码 3）、C-3 假绿修复的实际表现、
策略接线的位置与顺序、**本轮的 app 容器环境变量（compose + Helm 渲染输出）**
与**存量表授权的「撤销 → 旧脚本（对照）→ 新脚本 → 复原」四段实测**。
