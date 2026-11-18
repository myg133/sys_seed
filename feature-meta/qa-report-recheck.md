# 验证报告 —— 修复复核（Recheck）

- **需求**: REQ-006 多租户与隔离
- **分支**: `feature/REQ-006` @ `c3277f827a4d983cc2ef9d00b3573c1a1b07ad48`
- **首轮报告**: `feature-meta/qa-report.md`（@ `668d357`，结论 PASS-WITH-RISK；1 高 + 2 中）
- **本轮改动**: 4 提交（`1aeec63` → `75355f3` → `90d9195` → `c3277f8`），35 文件 / +1548 -158
- **复核范围**: 首轮 3 个问题（高 1 / 中 1 / 中 2）逐点独立复现 + 一次全量回归 + `base_dev` 缺权限项的严重度判定
- **验证者**: QA Agent（独立复现；未采信 Dev 自验报告里的任何数字与结论，全部自己重跑）
- **环境**: rustc/cargo 1.98.1 · podman 6.2.0 + postgres 17-alpine · podman-compose 1.6.0 · helm v3.16.3（临时下载到 `/tmp` 校验，未装进系统）· pnpm · bun 1.4.4
- **日期**: 2026-11-18
- **结论**: **PASS-WITH-RISK**

> 首轮 3 个问题**全部复验为真（已修复）**：部署侧 7 处逐处独立核对通过，5 项角色/RLS 实测全过，
> C-3 假绿已修且护栏**经变异验证是承重的**，策略已接进 compose 与 Helm。
>
> 但本轮**新发现 2 个中危**，都出在「角色拆分」这条主线上：
> **① 应用容器/Pod 仍然拿到了数据库超管连接串**（角色拆分只做了连接串层面，没做凭据层面）；
> **② RUNBOOK §3.4.3 给出的补建命令救不了「已存在表」的权限缺失**（`ALTER DEFAULT PRIVILEGES` 只管未来对象）。
>
> 另需更正 Dev 自陈的一处**事实误判**：`base_dev` 当前**不缺表权限**（实测 `base_app` 对
> `_sqlx_migrations` / `platform_meta` 均有 SELECT，`pg_default_acl` 里默认权限已落地）；
> Dev 描述的 `permission denied for _sqlx_migrations` **在本机复现不出来**。
> `base_dev` 的 `/health/ready` 503 的**真实原因是迁移未应用**（`pending migrations: [20261118000200]`），
> 属于「跑一次 migrate up」的常规开发态，不是权限问题。

---

## 逐项复核

| # | 问题 | Dev 声称 | 复核结果 | 证据 |
|---|------|---------|---------|------|
| **高 1** | 部署侧从未切到普通角色 | 逐处改了 7 个位置 | **PASS（7/7 处逐处核对通过）** | 见下方「部署侧逐处核对」表 |
| **高 1-a** | `compose.yaml` app 服务用普通角色 | `postgres://base_app:base@postgres:5432/base_dev` | **复现为真** | `compose.yaml:131`；渲染实测：`podman-compose -f compose.yaml --profile app --profile migrate --profile policies config` → `app` → `postgres://base_app:base@postgres:5432/base_dev`；`migrate`/`policies` → `postgres://base:base@...`（超管，保留 DDL 权限）✓ |
| **高 1-b** | `.env.example` 拆成两个连接串 + 角色分工注释 | 拆分清晰无混用 | **复现为真** | `.env.example:53-54`（`APP_DATABASE_URL` / `MIGRATION_DATABASE_URL`）；`:9-16` 角色分工注释块；`:36-49` 连接串说明。全文件 grep：**没有任何裸 `DATABASE_URL=` 赋值** |
| **高 1-c** | `deploy/compose.prod.yaml` app → `${APP_DATABASE_URL:?}`；新增 `app-role` 服务 | 生产路径独立、幂等 | **复现为真** | `:172` `DATABASE_URL: ${APP_DATABASE_URL:?…}`；`:130-161` app-role 服务；**prod 的 postgres 服务无 seed 挂载**（渲染后 `volumes` 只有 `pgdata`；全文件 grep `deploy/seed` → **0 命中**）✓；**幂等实测**：把 app-role 的 SQL 原文抠出来连跑 3 次 → 每次 `EXIT=0`，角色不变（`base_app` 仍是 `rolsuper=f / rolbypassrls=f`）✓ |
| **高 1-d** | Helm hook 顺序 app-role(-10) → migrate(-5) → policies(-4) | 先建角色、再建表、最后建 policy | **复现为真** | `helm template` 渲染实测（3.16.3）：`base-platform-app-role` weight=`-10` → `base-platform-migrate` weight=`-5` → `base-platform-policies` weight=`-4`，三者 hook 均为 `pre-upgrade,pre-install` ✓；默认渲染 7 类（`ConfigMap`/`Deployment`/`Job×3`/`Service`/`ServiceAccount`），`--set ingress.enabled=true` → 8 类 ✓（与 RUNBOOK §4 自检口径一致） |
| **高 1-e** | `require_isolation` 有生产调用点 | 接在 `serve()` 启动路径 | **复现为真** | `crates/platform-server/src/main.rs:255-258`（`isolation::verify_startup_isolation(&config.database_url, config.allow_superuser)`）→ 失败映射 `Failure::Isolation` → `main.rs:136-140` **退出码 3**。这是 `serve()` 的必经路径，**不是测试专用** |
| **高 1-f** | `BASE_ALLOW_SUPERUSER` 只本地放行；prod 配它退出码 2 | 两条路径都实测 | **复现为真** | 本地放行：`BASE_ALLOW_SUPERUSER=true` + dev + 超管 → 启动成功并打 WARN「放行了绕过 RLS 的连接角色」✓；**prod 不可绕过**：`BASE_ENV=prod` + `BASE_ALLOW_SUPERUSER=true` → `[CONFIG_FORBIDDEN_COMBINATION] BASE_ALLOW_SUPERUSER` **退出码 2** ✓（`config.rs:273-279`） |
| **高 1-g** | RUNBOOK 新增 §0.1 四步、§3.4 | 内容与实际行为一致 | **复现为真** | `deploy/RUNBOOK.md:20-30`（四步表：应用角色 → 迁移 → 策略 → 应用，标注「用超级用户起 → 直接拒绝启动（退出码 3）」）、`:276-366`（§3.4.1–3.4.4）；`:398-408`（三个 hook Job 权重表）。**四步在一个全新库里逐条跑通**（见「角色与 RLS 实测」第 6 行）✓ |
| **高 1-h** | 部署侧无超级用户残留 | — | **PASS（但见新问题 #1）** | 渲染后的 prod compose：`app` → `postgres://base_app:…`；`migrate`/`policies` → `postgres://base:…`（超管，**这是对的**，需要 DDL / 表属主）；`app-role` 走 `PG*` 而非 `DATABASE_URL`。Helm `deployment.yaml:61-65` 引用 Secret 的 `databaseUrlKey`，`values.yaml:76-78` 写明「DATABASE_URL 必须是普通角色」。**额外硬闸**：`scripts/prod-preflight.sh` 实测 —— `APP_DATABASE_URL` 用超管 → `✗ APP_DATABASE_URL 用的是超级用户（base）…` **exit 1**；用普通角色 → `✓ …应用连接用的是普通角色` **exit 0** ✓ |
| **中 1** | C-3 缺 `DATABASE_URL_TEST` 不再假绿 | 非 0 退出 + 无 "3 passed" | **复现为真** | 不设环境变量跑 `cargo test -p sandbox --test c3_real_db -- --include-ignored` → **`test result: FAILED. 1 passed; 3 failed; 0 ignored; … finished in 0.01s`，`EXIT=101`**；输出中**没有** "3 passed"；panic 文案「缺少 DATABASE_URL_TEST：真实库用例必须真的连库跑，不能静默跳过。」✓（`c3_real_db.rs:66-90`） |
| **中 1-b** | 护栏用例真在断言（变异验证） | 子进程断言非零退出 + 0 passed + 无 "3 passed" | **复现为真（护栏承重）** | 变异验证：临时在 3 条 C-3 用例开头插 `if std::env::var_os("DATABASE_URL_TEST").is_none() { return; }`（**精确复刻首轮的假绿实现**）→ 护栏立刻 **RED**：<br>`缺 DATABASE_URL_TEST 时子进程必须失败（它却成功退出了）：… test result: ok. 3 passed; 0 failed; … finished in 0.00s` —— **输出里是首轮那个一模一样的 "3 passed / 0.00s" 签名**。护栏抓住了它 → 证明不是空跑。已 `git checkout` 还原（`git diff` 为空） |
| **中 1-c** | 设了 `DATABASE_URL_TEST` 三条断言真执行 | 真跑 | **复现为真** | `DATABASE_URL_TEST=…/base_test cargo test -p sandbox --test c3_real_db -- --include-ignored` → **4 passed**（3 条 C-3 + 1 条护栏），`finished in 0.79s`，`EXIT=0`；**变异验证**：临时 `ALTER ROLE base_app BYPASSRLS` → 3 条 C-3 **全部失败**（`RlsBypassed { role: "base_app" }`）→ 证明真的连库、真的断言 → 已 `ALTER ROLE base_app NOBYPASSRLS` 还原 |
| **中 2** | policies 接进部署流程 | compose 两个文件 + Helm job | **复现为真** | `compose.yaml:107-117`（`policies` 服务，`profiles: ['policies']`，超管连接）；`deploy/compose.prod.yaml:108-121`（同名服务 + profile）；**Helm** `policies-job.yaml` weight `-4`（在 migrate `-5` 之后）✓；RUNBOOK §0.1 的顺序与实际服务顺序一致 ✓ |
| **中 2-b** | `base tenant-policies` 与 `--check` 都能跑 | --check 报齐全 | **复现为真** | `base tenant-policies` → `已为 1 个模块生成 RLS 策略：✓ sandbox：sandbox_tenant_item`，exit 0；`base tenant-policies --check`（超管）→ 打印绕过 RLS 告警 + `租户隔离前提校验通过`，exit 0；`--check`（**应用角色** `base_app`）→ 无告警 + 通过，exit 0；`cargo xtask tenant-policies --check` → 通过，exit 0 ✓ |

### 部署侧逐处核对（高 1，7/7）

| 位置 | 声称 | 复核 |
|---|---|---|
| `compose.yaml` app | 普通角色；migrate/policies 仍超管 | ✅ 渲染实测确认 |
| `.env.example` | 拆分 + 角色分工注释 | ✅ 无裸 `DATABASE_URL=`，注释块完整 |
| `deploy/compose.prod.yaml` | app → `${APP_DATABASE_URL:?}`；app-role 服务；**不挂 seed**；幂等 | ✅ 四项全过（幂等 = 连跑 3 次 exit 0） |
| Helm | app-role(-10) → migrate(-5) → policies(-4) | ✅ 渲染实测权重 |
| `require_isolation` 调用点 | `isolation.rs` → `serve()` | ✅ `main.rs:255-258`，退出码 3 |
| `BASE_ALLOW_SUPERUSER` | 本地放行 / prod 退出码 2 | ✅ 两条路径都实测 |
| RUNBOOK | §0.1 四步 + §3.4 | ✅ 与实际行为一致，四步在全新库跑通 |

---

## 角色与 RLS 实测

| # | 场景 | 预期 | 实测 | 结论 |
|---|---|---|---|---|
| 1 | 普通角色 `base_app` 不设 `app.tenant_id` | **0 行** | `select count(*) from sandbox_tenant_item` → **0** | ✅ |
| 2 | 超级用户 `base` 对照组 | **4 行**（能看到行 = RLS 被绕过） | → **4** | ✅ |
| 3 | `require_isolation` 对超级用户 | **拒绝启动，退出码 3**，信息清楚 | `✗ 租户隔离前提不成立，进程退出（fail-fast）：连接角色 \`base\` 绕过 RLS（超级用户或 BYPASSRLS）—— 租户隔离会 fail open…` **EXIT=3** | ✅ |
| 4 | **普通角色起应用** | 启动自检通过、`/health/live` 200、`/health/ready` `{"status":"ready"}` | 自检日志「租户隔离前提自检通过：连接角色不绕过 RLS role=base_app」；live **HTTP 200**；ready **HTTP 200** `{"appliedMigrations":[20261118000100,20261118000200],"environment":"dev","status":"ready",…}` | ✅ |
| 5 | `BASE_ALLOW_SUPERUSER=true` + `BASE_ENV=prod` | **必须仍拒绝，退出码 2** | `✗ 配置校验失败… - [CONFIG_FORBIDDEN_COMBINATION] BASE_ALLOW_SUPERUSER：生产环境禁止放行绕过 RLS 的角色` **EXIT=2** | ✅ |
| 6 | **全新库跑完 RUNBOOK §0.1 四步**（自建临时库，非 `base_dev`） | 角色 → 表 → policy → 应用就绪，RLS 生效 | ① `02-app-role.sql` exit 0；② `base migrate up` → `已应用版本 [20261118000100, 20261118000200]`，`base_app` 对 5 张表全有 SELECT；③ `base tenant-policies` → `sandbox_tenant_item` `relrowsecurity=t` / `policies=1`；④ 应用 → live 200 / **ready 200 `{"status":"ready"}`**；⑤ RLS 行为：超管看到 2 行 / `base_app` 不设租户看到 **0 行** | ✅（临时库已删除） |
| 7 | 应用角色的租户上下文（`base_test`） | 设 `tenant-a` / `tenant-b` 各 2 行、不串租户 | `tenant-a` → 2 行 / 1 个租户；`tenant-b` → 2 行 / 1 个租户 | ✅ |

> 场景 6 用的是我自建的临时库（`qa_fresh_tmp`），验证完已 `DROP DATABASE`；`base_dev` / `base_test` 均未写入业务数据。

---

## 回归

- **Rust**: `cargo test --workspace -- --test-threads=2` → **182 passed / 0 failed / 8 ignored**（8 = 3 条 C-3 `#[ignore]` + 5 条文档 `ignore` 块）—— 与预期的 182/182 一致；首轮的 172 条**全部仍绿**
- **Rust（真实库）**: `cargo test -p sandbox --test c3_real_db -- --include-ignored`（`DATABASE_URL_TEST` 已设）→ **4 passed / 0.79s**
- **集成脚本**: `bash scripts/test-integration.sh` → **exit 0**，容器数量 `2 → 2`（未新建容器），日志中 C-3 `4 passed … in 0.75s`，全脚本 **0 个 FAILED**
- **前端**: `pnpm -r test` → **23/23**（i18n 3 + auth 3 + hooks 3 + schema 3 + theme 2 + sdk 2 + ui 3 + shell 4）
- **E2E**: `bun run test:e2e` → **1/1**
- **Lint/格式**: `cargo clippy --workspace --all-targets -- -D warnings` → **0 告警**；`cargo fmt --all --check` → **通过**
- **门禁**: `cargo xtask capabilities --check` → `能力索引校验通过：13 个文件与源码一致`；`check-deps` → `依赖方向校验通过`；`check-modules` → `模块目录校验通过：1 个模块`；`tenant-policies --check` → `租户隔离前提校验通过`
- **编排文件**: `podman-compose -f compose.yaml config` → **exit 0**；`podman-compose -f deploy/compose.prod.yaml config`（注入必需变量）→ **exit 0**；`helm template` → **exit 0**（7 类 / 加 ingress 8 类）
- **依赖树**: `git diff develop..HEAD -- Cargo.lock` → **只新增 1 条 `name = "platform-db"`（工作区成员自己）** → **第三方 crate +0**；`cargo tree -i sqlx-macros` → `nothing to print`；`cargo tree | grep -c sqlx-macros` → **0**
- **核心逻辑未被改动**: `git diff --numstat 668d357..c3277f8 -- crates/platform-core/ crates/platform-db/src/` → `platform-core/` **无任何条目（零改动）**；`platform-db/src` 只有 `lib.rs +2`（模块导出）、`rls.rs +10`（`IsolationReport::table_problems` 拆分）、`scopes.rs +172`（新增）。**`query.rs` / `tenant.rs` / `quota.rs` 一行未改** ✓
- **脚本**: `prod-preflight.sh` 双路径实测（超管 → exit 1 / 普通角色 → exit 0）；`backup.sh` / `restore.sh` / `rollback.sh` / `backup-verify.sh` 改为 `DATABASE_URL` 缺失时回退 `MIGRATION_DATABASE_URL`（管理操作用超管，方向正确）

---

## 仍存在的问题

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **中** | **应用容器 / Pod 仍然拿到了数据库超管连接串** —— 角色拆分只做到了「连接串层面」，没做到「凭据层面」。<br>① `deploy/compose.prod.yaml:167-168`：`app` 服务挂 `env_file: ${ENV_FILE:-.env}`，而 `.env` 里含 `MIGRATION_DATABASE_URL=postgres://base:<超管口令>@…` 与 `APP_ROLE_PASSWORD` → **这些都进了 app 容器的环境变量**（`environment:` 只覆盖了 `DATABASE_URL` 一项）。<br>② `deploy/helm/base-platform/templates/deployment.yaml:53-54`：`envFrom: secretRef: base-platform-secret` → **整个 Secret 被注入 app Pod**，而该 Secret 按 RUNBOOK §1.3 含 `MIGRATION_DATABASE_URL`（超管）+ `APP_ROLE_PASSWORD`。<br>**影响**：应用连接本身已是普通角色、RLS 真的开着（这点已复验为真），但应用进程环境里躺着超管口令 —— 应用一旦被拿下（任意 RCE / 环境变量泄露 / 崩溃转储），拿到的是**超级用户** → RLS 直接被绕过 → 跨租户全量可读。**这把本轮刚建立的角色边界从容器外又漏回容器内**。 | 渲染实测（prod compose `app` 服务块）：`env_file: [ .env ]` + `environment: { DATABASE_URL: postgres://base_app:… }`；`.env.example:54` `MIGRATION_DATABASE_URL=postgres://base:…`；`deployment.yaml:53-54` `envFrom: secretRef: base-platform-secret`；`RUNBOOK.md:76-81` Secret 建法含 `MIGRATION_DATABASE_URL` 与 `APP_ROLE_PASSWORD` | **本轮建议补**：① compose：给 `app` 单独一个 `app.env`（只含 `APP_DATABASE_URL` 与非敏感项），别复用管理侧 `.env`；② Helm：把 Secret 拆成 `base-platform-app`（只有 `DATABASE_URL`）与 `base-platform-admin`（迁移用），`deployment.yaml` 只 `envFrom` 前者；③ 迁移 / 策略 Job 改 `env` 显式点名，不要整包 `envFrom`。<br>（不阻塞合并：应用**用的**连接是普通角色，RLS 已真实生效；这是纵深防御问题） |
| **2** | **中** | **RUNBOOK §3.4.3 给出的补建命令救不了「已存在表」的权限缺失**（`ALTER DEFAULT PRIVILEGES` 只对**之后**新建的对象生效，对**已存在**的表无效）。<br>**实测（临时库）**：建表 → 跑 app-role 的 SQL → `has_table_privilege('base_app','old_table','SELECT')` = **f**；再建一张新表 → **t**。<br>**影响**：RUNBOOK §3.4.3 的症状对照表写着「应用连得上，但 `permission denied for table <表>` → 同上：重跑角色脚本 / app-role 步骤」—— **这条处置对已存在的表无效**。存量环境（表由更早的迁移建好）跑完这条命令后**仍然 503**，运维会被这条 RUNBOOK 误导（重跑一遍没用 → 以为是别的问题）。 | 临时库 `qa_recheck_tmp` 实测：`CREATE TABLE old_tenant_item` → 跑 `deploy/seed/02-app-role.sql` → `old_table_select = f`；`CREATE TABLE new_tenant_item` → `new_table_select = t`（临时库已删）。`deploy/seed/02-app-role.sql:38-39` 与 `deploy/compose.prod.yaml:155-156`、`deploy/helm/base-platform/templates/app-role-job.yaml:78-79` 三处都**只有** `ALTER DEFAULT PRIVILEGES`，**没有** `GRANT … ON ALL TABLES IN SCHEMA public` | **本轮建议补（一行）**：在 `02-app-role.sql`、compose 的 `app-role` 服务、Helm 的 `app-role-job` 三处各加一句<br>`GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO base_platform_app;`<br>（新建对象靠默认权限、存量对象靠这句，一起覆盖）；并把 RUNBOOK §3.4.3 的处置说明改成「默认权限只管新建表，存量表要用这句 GRANT」 |
| **3** | **低** | **Dev 自陈的「`base_dev` 缺表权限」在本机复现不出来 —— 事实误判。** 实测 `base_dev`：`pg_default_acl` 里 `base` 的默认权限**已落地**（`base_platform_app=arwd/base`），`base_app` 对 `_sqlx_migrations` / `platform_meta` **均有 SELECT**。Dev 描述的 `permission denied for _sqlx_migrations` **没有出现**；`base_dev` 的 `/health/ready` 503 的**真实 detail 是 `pending migrations: [20261118000200]`** —— 即「迁移没跑」（迁移 `20261118000200` 早于本次改动就已存在，与本轮无关）。 | `podman exec base-postgres psql -U base -d base_dev -c "select … from pg_default_acl"` → `base\|public\|r\|{base_platform_app=arwd/base}`；`has_table_privilege('base_app','public._sqlx_migrations','SELECT')` = **t**；应用实测：`live HTTP 200`、`ready HTTP 503` 且 `detail="pending migrations: [20261118000200]"`；`base_dev` 的 `_sqlx_migrations` 只有 `20261118000100` | ① 更正 Dev 报告与 `feature-meta/traceability.md:86` 的表述（那里写的是「存量开发卷早于 `02-app-role.sql` 创建 → `base_app` 在 `base_dev` 上缺表权限」）—— 与实测不符；② `base_dev` 的 503 只需在开发机上跑一次 `cargo run -p platform-server --bin base -- migrate up`（超管连接）即可，属常规开发态，**不构成缺陷**；③ `base_dev` 是只读纪律，本轮**未对它做任何写操作**（只跑 SELECT 与只读的就绪探针） |
| **4** | 低 | **首轮 #8 的弱口令仍在**：`deploy/seed/02-app-role.sql:29` 硬编码 `CREATE ROLE base_app LOGIN PASSWORD 'base'`。已**降级为开发专用**（生产由 `app-role` 步骤用 `APP_ROLE_PASSWORD` 建角色），但开发环境口令仍是弱口令，且 `.env.example:53` 的 `APP_DATABASE_URL=postgres://base_app:base@…` 同样把弱口令写死。 | `deploy/seed/02-app-role.sql:29`；`.env.example:53` | 生产路径已不受影响；开发环境保留弱口令是可接受的取舍，建议在 RUNBOOK §3.4.2 明确标注「仅开发库」。**不阻塞合并** |
| **5** | 低 | **首轮 #6（配额无自动化用例）未处理**：`quota.rs` 仍只有纯结构体单测，「计数落 DB / 重启不丢 / 超限判定 / 跨租户写入被拒」无自动化用例。 | `git diff 668d357..c3277f8` 未触及 `crates/platform-db/src/quota.rs` | 记入 Sprint 2 收尾 |
| **6** | 低 | **首轮 #5（契约口径张力）未处理，但已如实登记**：`api-conventions v1` C3/C7 写「业务模块不得手工加 `tenant_id` 过滤条件 / 平台自动叠加」，本期实现是「模块在自己写的 SQL 里写 `tenant_id = {tenant}`，平台强制占位符」。`feature-meta/traceability.md:91` 已登记为「需 BA 裁定」✓ | `modules/sandbox/backend/src/tenant_repository.rs` 全部显式带 `{tenant}`；`crates/platform-db/src/query.rs` 强制占位符；`traceability.md:91` | 需 BA 裁定（改契约 or 改实现）。**本轮 RLS 已真实生效，风险已从「会泄漏」降级为「文档口径不一致」** |
| **7** | 低 | **首轮 #4（AC-S-012 未登记）已解决**：`traceability.md:90` 现已如实登记 AC-S-012 为「本期未实现」✓（AC-S-008 同样已登记）。**该 AC 本身仍未实现**（`X-Tenant-Id` 全仓仍零命中） | `grep -rn "X-Tenant-Id"` → 0 命中；`traceability.md:90` | 记入后续需求（REQ-019 API 治理）。**不阻塞本轮** |

---

## Dev 自陈未完成项的严重度判定（`base_dev` 缺权限）

| 问 | 答 | 依据 |
|---|---|---|
| **1. 开发环境当前是坏的吗？** | **不是。** `base_dev` 权限完整（`base_app` 对 `_sqlx_migrations` / `platform_meta` 均有 SELECT，默认权限已落地）。应用能起、自检通过、`/health/live` **200**。`/health/ready` **503** 的真实 detail 是 `pending migrations: [20261118000200]` —— 是「迁移没跑」，不是权限问题。跑一次 `migrate up` 即恢复，**属常规开发态**。 | 实测输出见问题 #3 |
| **2. 补建命令正确可用吗？** | **语法可用，但不足以解决它声称要解决的症状。** 命令本身能跑通（原文 SQL 连跑 3 次 exit 0，幂等 ✓）；但它**只设默认权限**，对**已存在**的表无效（实测 `old_table_select = f`）。RUNBOOK §3.4.3 症状表里「`permission denied for table <表>` → 重跑角色脚本 / app-role 步骤」这条**处置是错的**（重跑没用）。 | 临时库实测；`ALTER DEFAULT PRIVILEGES` 语义 |
| **3. 新建卷无此问题吗？** | **是。** 全新路径已端到端跑通：app-role → migrate → policies → 应用，`base_app` 对所有表有权限、RLS 生效、ready 200、RLS fail-closed（超管 2 行 / 普通角色 0 行）。新建卷在 initdb 阶段就跑 `02-app-role.sql`（含 `ALTER DEFAULT PRIVILEGES`，`:38-39` ✓），之后的迁移建表自动继承。 | 临时库 `qa_fresh_tmp` 四步实测 |
| **4. 严重度与建议** | **中 —— 本轮必须补，但补的不是「开发环境」，而是「补建手段本身」。**<br>① **必须补**：在 app-role 三处（`02-app-role.sql` / compose `app-role` 服务 / Helm `app-role-job`）补 `GRANT … ON ALL TABLES IN SCHEMA public TO base_platform_app`，让 RUNBOOK §3.4.3 的处置对存量表真正有效（问题 #2）。<br>② **不必补**：`base_dev` 本身没坏，跑一次 `migrate up` 即可；**Dev 的诊断（缺表权限）与实测不符，应更正**（问题 #3）。<br>③ 若 BA 认为「存量私有化环境升级」不在本期范围，则可把问题 #2 **降级为 Sprint 2 收尾**，但 RUNBOOK 的处置文字必须同步改成「默认权限只管新建表」，否则会误导运维。 | 综合上述 |

---

## 未验证

| 项 | 说明 |
|---|---|
| **真实 K8s 集群里的 Helm Job 执行** | 本机无集群，只做了 `helm template` **渲染**（权重、kind 数、Secret 引用均正确）。三个 hook Job 在真集群上的**实际执行顺序**、Secret 预置、`psql` 镜像拉取**未在真集群验证** |
| **镜像实构建 + 容器里跑 compose 的 `app-role` / `policies` 服务** | 本机内存不足未构建镜像；`app-role` 的 SQL 我用**原文**在容器里连跑 3 次验证幂等，`policies` 用交付二进制验证；**但没有真的用 `podman-compose --profile app-role run --rm app-role` 起这个服务**（需要与 `base-postgres` 同网络且口令不同，会动到运行环境） |
| **私有化生产库的真实部署闭环** | prod compose 只做了 `config` 渲染校验 + `prod-preflight.sh` 双路径实测；未真正 `up -d` 一套私有化环境 |
| **`base_dev` 上的写操作** | 按纪律全程只读（仅 SELECT 与只读就绪探针）。`base_dev` 上「跑完 migrate up 后的策略与权限状态」未实测 |
| **覆盖率数字** | 低配机无覆盖率工具链，未测 |
| **真实负载性能** | 未压测（「每次访问多一次 `BEGIN`/`COMMIT`」的开销未量化） |
| **AC-S-008 / AC-S-012** | 本期未实现（已如实登记），不计入通过 |

---

## 本轮产生的临时物（均已清理）

| 物 | 处置 |
|---|---|
| 临时库 `qa_recheck_tmp`（权限实验） | 已 `DROP DATABASE` ✓ |
| 临时库 `qa_fresh_tmp`（四步全流程） | 已 `DROP DATABASE` ✓ |
| `ALTER ROLE base_app BYPASSRLS`（变异验证） | 已 `ALTER ROLE base_app NOBYPASSRLS` 还原 → 实测 `base_app \| rolsuper=f \| rolbypassrls=f` ✓ |
| `modules/sandbox/tests/c3_real_db.rs` 变异（3 行早退） | 已 `git checkout` 还原，`git diff` 为空 ✓ |
| `/tmp` 下的日志、渲染结果、helm 二进制 | 已删除 ✓ |
| 容器 | `base-postgres` / `base-redis` 仍为 2 个且 healthy（未新建） ✓ |

**结束状态**：`git status` 仅 `M feature-meta/qa-report.md`（首轮报告，本轮未改动）与 `M feature-meta/qa-report-recheck.md`（本报告）；其余干净。

> 注：本文件在本次复核前存放的是 **REQ-005** 的复核报告（BA 已归档于 `a56869f`，内容在 git 历史里可查）。
> 与首轮报告 `feature-meta/qa-report.md` 的处理方式一致（同一路径复用，内容按当前需求覆盖）。
