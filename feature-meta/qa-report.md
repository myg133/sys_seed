# 验证报告 —— Pre-merge

- 需求: REQ-005 交付基线与发布
- 分支: `feature/REQ-005` @ `266dfa94ca17df631e654efd303e8dd7aa16cc7e`
- 基线: `develop` @ `37a93c56eb2bbc7729595f292069a41a5282bebc`
- 验证者: QA Agent（独立复现，未采信 Dev 自验报告中的任何数字与结论）
- 工具链: rustc/cargo 1.98.1 / podman + podman-compose 1.6.0 / helm v3.16.3（临时下载到 `/tmp` 校验，未装进系统）/ pnpm 12.19.0 / bun 1.4.4 / jq 1.8.2
- 日期: 2026-11-18
- 结论: **PASS-WITH-RISK**

> 核心结论：12 项核心检查 **11 项 PASS**，1 项 PASS-with-finding（Helm 渲染口径）。
> Dev 的**关键数字与结论全部独立复现为真** —— 健康检查异常路径（ready 503 / live 200）、
> 迁移不随 `up -d` 跑、备份恢复闭环（我自测 311/783/1769ms，与 RUNBOOK §6.2 的 307/704/1509ms 同量级）、
> 迁移可回滚（含无 `down.sql` 被拒绝回滚）、sync-contracts 不再静默跳过、配置 fail-fast、
> sqlx 克制引入、回归 132/132 + 23/23 + 1/1 —— 逐条自己跑通。
> 扣分项不在核心功能：**① 健康检查 2/3 条错误路径把中文句子塞进 `detail`，违反契约 C6**
> （且 Dev 的自验报告与追溯矩阵都把这一项写成「符合 C6 / detail 为英文」，属**不实结论**）；
> ② RUNBOOK 与 Dev 报告里的 Helm 渲染自检口径错误（默认渲染**不含 Ingress**）；
> ③ K8s readinessProbe 超时与连接池超时同值（5s），真实集群里探针会超时而非拿到 503。

---

## 追溯

| AC | 实现 | 测试 | 结论 |
|----|------|------|------|
| **AC-S-005** 配置与代码分离：同一镜像靠环境变量切分 dev/staging/prod，缺必填则 fail-fast | `crates/platform-server/src/config.rs`（`DeployConfig::from_env` / `from_lookup`，三个稳定错误码）、`crates/platform-server/src/main.rs:101-119`（启动前校验 → 退出码 2）、`.env.example`、`deploy/compose.prod.yaml`（env_file 注入）、`deploy/helm/base-platform/templates/configmap.yaml` + `deployment.yaml`（ConfigMap / Secret 引用） | ① 同一二进制 `BASE_ENV=dev/staging/prod` 行为不同；② **实测**缺 `DATABASE_URL` → 打印 `[CONFIG_MISSING_REQUIRED]` + **退出码 2**（未监听端口）；③ 空串 `DATABASE_URL="   "` 同判缺失；④ 4 个非法值（`BASE_ENV=pro` / `SERVER_PORT=not-a-port` / `BASE_AUTH_MODE=ldap` / `BASE_TENANT_MODE=row`）**一次报全**、错误码 `CONFIG_INVALID_VALUE`、退出码 2；⑤ `prod` + `BASE_SEED_ON_START=true` → `CONFIG_FORBIDDEN_COMBINATION`、退出码 2；⑥ 迁移目录不存在 → 监听前退出（码 1） | **PASS**（C6 细节见问题 #1） |
| **AC-S-006** 迁移可回滚：能恢复到迁移前状态，且恢复过程有演练记录 | `crates/platform-server/src/migrate.rs`、`migrations/20261118000100_platform_meta.{up,down}.sql`、`scripts/rollback.sh`、`scripts/backup.sh` / `restore.sh` / `backup-verify.sh`、`deploy/RUNBOOK.md` §6.2 | ① 干净库 `up` → 建出 `platform_meta`；`down --target 0` → **只剩 `_sqlx_migrations`**（业务表清空）→ 再 `up` 可重复；② **无 `down.sql` 的版本被拒绝回滚**（临时目录造 `20261118000200_irreversible_probe.up.sql`，`migrate down` 退出码 1 + 「拒绝回滚」，**表未被动过**）；③ `scripts/rollback.sh 0` 实跑（先自动备份 → 再 down → 打印被撤销版本）；④ **备份恢复闭环**自己跑通并计时（见核心检查项 #3），RUNBOOK §6.2 演练记录为**真实结果** | **PASS** |

### 设计摘要 §7 交付清单追溯（摘要）

| 交付项 | 复核结论 |
|--------|----------|
| 多阶段 Dockerfile（<100MB） | PASS（配置层）；镜像实构建**未验证**（见「未覆盖」） |
| `compose.yaml` + `deploy/compose.prod.yaml` | PASS |
| `deploy/helm/base-platform/` | PASS-with-finding（见问题 #2） |
| 配置外置 + 启动校验 + `.env.example` | PASS |
| 迁移作为独立步骤（compose profile + Helm hook） | PASS |
| `scripts/backup.sh` / `restore.sh` / `rollback.sh` | PASS |
| `deploy/RUNBOOK.md` + 恢复演练记录 | PASS |
| 健康检查端点 | PASS（异常路径全绿；C6 细节见问题 #1） |
| CI 构建镜像与扫描 job | PASS（配置层）；Actions **未实跑**（见「未覆盖」） |

---

## 核心检查项结果

| # | 检查项 | Dev 声称 | 复核结果 | 证据 |
|---|--------|---------|---------|------|
| **1** ★ | **健康检查异常路径** | PG 正常 → live 200 / ready 200；停库 → ready **503**（`DATABASE_UNAVAILABLE`）/ live 仍 **200** | ✅ **复现为真（自行起服务 + 停/起 PG 容器实测）** | PG 停：`/health/live` → **HTTP 200** `{"status":"alive",...}`；`/health/ready` → **HTTP 503** `{"code":"DATABASE_UNAVAILABLE","detail":"database is unreachable: pool timed out while waiting for an open connection","status":503,...}`；PG 起回：ready → **HTTP 200** `{"appliedMigrations":[20261118000100],"status":"ready",...}`；`_sqlx_migrations` 数据完好。**额外自测**另外两个错误码：`MIGRATIONS_PENDING`（连上但迁移表无记录）→ 503；`MIGRATION_DIRTY`（`UPDATE _sqlx_migrations SET success=false`）→ 503 `code=MIGRATION_DIRTY` |
| **2** ★ | **迁移不随 `up -d` 自动跑** | 默认服务列表不含 migrate；profile 下含；迁移不在 app entrypoint | ✅ **复现为真** | `podman-compose -f deploy/compose.prod.yaml config --services` → `postgres / redis / app`（**无 migrate**）；`--profile migrate config --services` → 多出 `migrate`；开发 compose 同理（默认 `postgres / redis`）。**另用活证据证明不在 entrypoint**：对着一个**零表**的新库起 `base serve`，服务起来后该库**仍然零表**（`base serve` 不跑迁移），ready 返回 `MIGRATIONS_PENDING` |
| **3** ★ | **备份恢复闭环 + 自己计时** | 备份 307ms / 恢复 704ms / 校验演练 1509ms | ✅ **复现为真，数字同量级**（RUNBOOK §6.2 未虚报） | 自测：**备份 311ms** / **恢复 783ms** / **校验演练 1769ms**。流程：造 `qa_probe_drill` 表 + 1 行 → `backup.sh` → `restore.sh <file> base_qa_restore_drill` → 临时库**表与数据都在**（`1|qa-drill-row`，4 张表）→ `backup-verify.sh` 输出「✓ 备份已导入临时库」且报 **4 张表**（= 源库张数，证明**确实导入**了，不是空库冒烟）→ 临时库 `base_verify_*` **自动清理**（前后 `pg_database` 对比无残留）。**反向测试**：损坏备份 → 退出码 3；合法 gzip 但空内容 → 冒烟 2 失败退出码 1；两种失败路径临时库均被清干净；`restore.sh` 不带 `--yes` 拒绝（`base_dev` 未被动）、未知参数退出码 2、`BACKUP_KEEP=abc` 退出码 1、缺 `DATABASE_URL` 退出码 1 |
| **4** ★ | **迁移可回滚 + 不可逆拒绝** | up → down --target 0 → 业务表清空 → 再 up；`rollback.sh 0` 实跑 | ✅ **复现为真** | `migrate up` → `platform_meta` 建出；`migrate down --target 0` → 只剩 `_sqlx_migrations`（记账表 0 行）；再 `up` 成功。**不可逆拒绝**：在 `/tmp` 造 `20261118000200_irreversible_probe.up.sql`（**故意无 down**）→ `migrate status` 列出「不可逆（无配套 down）：[20261118000200]」；`migrate down --target 0` → **退出码 1**「目标版本 0 之上存在不可逆迁移 [20261118000200]（没有配套 down），拒绝回滚」，**三张表原样保留**；`scripts/rollback.sh 0` 同样退出码 1 并拒绝。`rollback.sh 0` 正常路径实跑：自动备份 → down → 提示用备份回退 |
| **5** ★ | **sync-contracts 不再静默跳过** | 拿不到 demand → exit 2 + 清晰提示；正常路径仍一致 | ✅ **复现为真** | 临时仓库（**无 demand 引用**）→ 打印「✗ 找不到 BA 正本分支（demand / origin/demand）…本脚本不会静默跳过」→ **退出码 2**；`manifest.json` 缺 `contracts` pin → **退出码 2**；未知参数 → 退出码 2；本仓库正常路径 → 两份快照一致、**退出码 0** |
| **6** | **配置 fail-fast** | 缺 `DATABASE_URL` → `CONFIG_MISSING_REQUIRED` + 退出码 2 | ✅ **复现为真** | 缺 `DATABASE_URL` / 空串 / 4 个非法值 / prod+seed 四组场景全部**退出码 2** 且给出稳定错误码与修复提示；迁移目录缺失 → 监听前退出码 1。**未出现「静默用默认值启动」** |
| **7** | **Dockerfile 与镜像预算** | 三阶段、`rust:lld`、runtime 非 root、官方镜像地址、CI 有 build-image + 体积校验 | ✅ **复现为真（配置层）** | `Dockerfile`：`FROM rust:lld AS builder` / `FROM node:22-bookworm-slim AS frontend` / `FROM debian:12-slim AS runtime`；`RUN useradd --system --uid 10001` + `USER base`；**三个基础镜像全是官方地址，无国内镜像前缀**。CI：`build-image` job 含 `docker image inspect ... --format '{{.Size}}'` 且 `>100MB` 即 `exit 1`；`ci.yml` YAML 可解析，**12 个 job**。本机不构建镜像合理，且 CI 有 job 兜住（构建 + 体积 + 容器内迁移 + 探针冒烟） |
| **8** | **Helm Chart** | `helm lint` / `helm template` 通过；渲染出 Deployment / Service / ConfigMap / 迁移 Job；迁移是 `pre-upgrade,pre-install` hook | ⚠️ **基本成立，但 Dev 与 RUNBOOK 的渲染口径不实** | `helm lint` → `1 chart(s) linted, 0 chart(s) failed`（exit 0）；默认渲染 = **ServiceAccount / ConfigMap / Service / Deployment / Job**（**无 Ingress**）；`--set ingress.enabled=true` 才多出 Ingress。迁移 Job：`"helm.sh/hook": pre-upgrade,pre-install`、`hook-weight: "-5"`、`hook-delete-policy: before-hook-creation` ✓；Secret **只引用不生成**（`secret.existingSecret`）✓；探针 / 资源配额 / 非 root / `readOnlyRootFilesystem` ✓。→ **问题 #2** |
| **9** | **core-change-detect.sh 与 CI 门禁** | 自检通过；覆盖三个 crate；未登记平台 crate 自检能发现；`pull_request_review` 仍在；merge-base 口径语义正确 | ✅ **复现为真**（合并语义用**真实 diff** 验证） | `./scripts/core-change-detect.sh --self-test` → `✓ 自检通过`（exit 0）。**merge-base 语义（真实 diff 验证）**：构造「base 侧合入核心改动 + PR 侧只改非核心」→ **旧口径（diff base.sha）会误报** `crates/platform-core/a.txt`，**新口径输出为空**（exit 0）；再让 PR 侧改核心 → 正确检出 `crates/platform-core/a.txt`。**未登记平台 crate**：临时仓库加 `crates/qa-unregistered-probe/` → **退出码 1** 且点名该 crate。CI：`pull_request_review: types: [submitted, edited, dismissed]` **仍在**（REQ-004 修复未被改坏），`pull_request.types` 含 `review_requested` → **问题 #3**（自检入口未覆盖登记检查） |
| **10** | **sqlx 引入克制** | 只用于部署期健康检查与迁移；无业务数据访问层；`sqlx-macros` 不参与编译；两个核心 crate 零改动 | ✅ **复现为真** | `cargo tree -i sqlx` → **只有** `platform-server`；`cargo tree --workspace \| grep -c sqlx-macros` → **0**（macros 特性已关，不参与编译）；依赖树中只有 `sqlx` / `sqlx-core` / `sqlx-postgres`；全仓 `Cargo.toml` 中除工作区根的 sqlx 外**无第二个 crate 依赖 sqlx**；`git diff develop..HEAD -- crates/platform-core crates/platform-contracts crates/platform-contracts-macros` → **输出为空**；`AGENTS.md:132` 与 `capabilities/index.json:67` 仍如实标注 `db_access_layer` = 「尚未提供」 |
| **11** | **回归** | Rust 132/132（原有 99 全绿）、clippy 0 告警、fmt 通过、前端 23/23、E2E 1/1、三个 xtask 命令全过 | ✅ **全部复现为真** | Rust `cargo test --workspace -- --test-threads=2` → **132 passed / 0 failed**（platform-core 41 + platform-contracts 8 + platform-server 26+5 + sandbox 2+4 + xtask 46；3 个 doctest ignored）。「原有 99」核算成立：44(xtask 基线) + 41 + 8 + 2 + 4 = **99**，本轮新增 33（platform-server 31 + xtask 2，`modules.rs` 的 `#[test]` 由 12 → 14），且**无任何既有测试文件被改动**。`cargo clippy --workspace --all-targets -- -D warnings` → **0 告警**；`cargo fmt --all --check` → 通过；`pnpm -r test` → **23/23**；`bun run test:e2e` → **1/1**；`cargo xtask capabilities --check`（13 文件一致）/ `check-deps` / `check-modules` → 全过 |
| **12** | **通用清单** | — | ⚠️ **基本通过，问题 #4 / #5 / #6** | **Secret**：`git ls-files` 只有 `.env.example`，**无 `.env`**（磁盘上也没有）；`.dockerignore` 排除 `.env*`（`!.env.example`）；全仓**无硬编码密钥**（只有 `PGPASSWORD` 变量传递）。**TODO/FIXME/XXX/HACK**：本轮改动的 `.rs/.ts/.tsx/.sh/.yaml/.sql` 文件中 **0 处**。**调试输出**：无 `dbg!`；`println!` 仅在 CLI 输出路径。**迁移位置**：放仓库根 `migrations/` **与 `check-modules` 的模块级约定不冲突** —— `modules/sandbox/migrations/` 仍是 README 占位（新规则已写进该 README 与 xtask 提示语），`check-modules` 通过。**RUNBOOK 演练记录**：与我自己的复现数字吻合（见 #3），**不是模板占位**。→ **问题 #4 / #5 / #6** |

---

## 测试

- **Rust 132/132**（`cargo test --workspace -- --test-threads=2`，0 失败 / 3 doctest ignored；原有 99 个用例全绿且无既有测试文件被改动）
- **前端 23/23**（`pnpm -r test`：i18n 3 + auth 3 + schema 3 + hooks 3 + sdk 2 + theme 2 + ui 3 + shell 4）
- **E2E 1/1**（`bun run test:e2e`，chromium，`底座壳渲染并可过滤菜单`）
- **Lint**：`cargo clippy --workspace --all-targets -- -D warnings` → 0 告警；`cargo fmt --all --check` → 通过
- **静态门禁**：`cargo xtask capabilities --check` / `check-deps` / `check-modules` → 全过；`./scripts/sync-contracts.sh` → 两份快照一致（exit 0）
- **Helm**：`helm lint` → 0 failed；`helm template` → 渲染通过（默认 5 类资源）
- **运维脚本**：备份 311ms / 恢复 783ms / 校验演练 1769ms；迁移 up→down→up；`rollback.sh 0`；`core-change-detect.sh --self-test`；6 组失败路径反向测试全部按预期非 0 退出

---

## 问题（按严重度）

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **中** | **契约 C6 违规：健康检查 3 条错误路径中有 2 条把中文句子写进 `detail`。** 契约明文要求「后端**不得**在 `detail` 里拼中文句子」（面向用户的中文文案由前端 i18n 渲染，见 REQ-021 AC-S-029）。`MIGRATION_DIRTY` 与 `MIGRATIONS_PENDING` 的 `detail` 均为中文句子，`DATABASE_UNAVAILABLE` 才是英文。**更严重的是 Dev 的自验产物把这一项写成「符合 C6 / detail 为英文」，属不实结论**（验证只覆盖了数据库不可达那一条路径） | `crates/platform-server/src/health.rs:80` → `Some(format!("迁移 {version} 处于 dirty 状态，需人工处理"))`；`crates/platform-server/src/health.rs:85` → `Some(format!("待应用迁移：{pending:?}"))`；实测响应：`{"...","detail":"待应用迁移：[20261118000100]","code":"MIGRATIONS_PENDING",...}`、`{"...","detail":"迁移 20261118000100 处于 dirty 状态，需人工处理","code":"MIGRATION_DIRTY",...}`。契约依据：`BA/contracts/api-conventions/v1.md:110-111`「`detail` 是面向开发者的英文描述…后端**不得**在 `detail` 里拼中文句子（见 REQ-021 AC-S-029）」。**Dev 不实结论**：`feature-meta/verification-report.md:106`「错误体符合契约 C6…`detail` 为英文」；`feature-meta/traceability.md:49`「`detail`（**英文**，符合 C6「不得在 detail 里拼中文句子」）」。影响：REQ-021 要在错误体上做 i18n，后端塞中文句子会让非中文租户看到中文，且违反平台硬规则 5 | ① 把两条 `detail` 改成**英文**（如 `migration 20261118000100 is in a dirty state and needs manual recovery` / `pending migrations: [...]`），面向运维的中文说明**移到日志**（`main.rs:295-300` 已有 `tracing::warn!` 记录 code+detail，中文放日志是允许的）；② 补一条单测断言 `detail` 为纯 ASCII，防止回潮；③ 更正 `verification-report.md:106` 与 `traceability.md:49` 的「符合 C6」表述 |
| **2** | **低** | **RUNBOOK §4 的 Helm 渲染自检口径错误，且 Dev 的报告/追溯矩阵重复了同一错误结论。** RUNBOOK 写「应含：ServiceAccount / ConfigMap / Service / Deployment / Ingress / Job」，但 `values.yaml` 默认 `ingress.enabled: false`，**默认渲染不含 Ingress**；运维按 RUNBOOK 逐字执行会以为 Chart 少了东西 | `deploy/RUNBOOK.md:191-192`（文档里的命令 + 注释）；实测默认渲染 = ServiceAccount / ConfigMap / Service / Deployment / Job；加 `--set ingress.enabled=true` 才出现 Ingress。**Dev 不实结论**：`feature-meta/verification-report.md:173`「`helm template ... \| grep -E '^kind:'` → kind: ServiceAccount / ConfigMap / Service / Deployment / **Ingress** / Job」；`feature-meta/traceability.md:28`「实渲染出 **ServiceAccount / ConfigMap / Service / Deployment / Ingress / Job** 六类资源」 | ① RUNBOOK 的自检命令改为 `helm template base-platform deploy/helm/base-platform --set ingress.enabled=true \| grep -E '^kind:'`，或把注释改成「默认 5 类；加 `--set ingress.enabled=true` 后含 Ingress」；② 更正 `verification-report.md:173` 与 `traceability.md:28` 的渲染清单 |
| **3** | **低** | **K8s readinessProbe 超时与连接池获取超时同值（均 5s），真实集群里探针会「超时」而不是拿到设计好的 503/`DATABASE_UNAVAILABLE`。** 数据库挂掉时 ready 实测 **5.003s** 才返回 503，而 `readinessProbe.timeoutSeconds: 5` —— 探针会在 5s 处被 kubelet 掐断。功能上仍会「不接流量」（AC-S-006 的意图成立），但**设计好的错误码永远进不了 K8s 事件**，且 3ms 的余量是竞态 | `crates/platform-server/src/db.rs:24` `.acquire_timeout(Duration::from_secs(5))`；实测 `curl /health/ready`（PG 停）→ `time=5.003031s` → 503；`deploy/helm/base-platform/templates/deployment.yaml:77-84` readinessProbe `timeoutSeconds: 5`；镜像内 `HEALTHCHECK --timeout=5s`（`Dockerfile:89`）同样是 5s | ① readinessProbe `timeoutSeconds` 调到 **8**（或把池的 `acquire_timeout` 降到 2s）；② 镜像 `HEALTHCHECK --timeout` 同样放宽到 8s；③ 在 RUNBOOK §7.3 补一句「DB 不可达时 ready 需要约 5s 才返回 503，探针超时需大于该值」 |
| **4** | **低** | **`core-change-detect.sh --self-test` 这个「自检入口」没有覆盖「平台 crate 必须登记」这条检查。** `check_core_coverage()` 只在主流程调用；`self_test()` 只测 merge-base 语义。检查本身有效（我实测能发现未登记 crate），但 Dev 的报告把它描述成「加自检」，且命名上的自检入口覆盖不到 | `scripts/core-change-detect.sh:47-68`（`check_core_coverage`）、`:71-115`（`self_test`，**不含** coverage 检查）、`:135`（coverage 仅在主流程调用）；`feature-meta/verification-report.md:197`「**加自检**：`crates/` 下每个平台 crate 都必须在清单里，忘了登记直接失败」 | 把 `check_core_coverage` 的调用并入 `self_test()`（在临时仓库里建一个未登记 crate，断言其失败），让 `--self-test` 成为名副其实的入口；否则把报告里的「加自检」措辞改为「主流程强制校验」 |
| **5** | **低** | **私有化交付的 compose 默认口令是 `base`，且默认基础镜像指向第三方镜像源。** `POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:-base}` 在客户没设该变量时**静默**用弱口令建库（设计摘要 §3.3 要求生产用 Docker secrets / K8s Secret 注入）；`POSTGRES_IMAGE` / `REDIS_IMAGE` 默认值是 `docker.m.daocloud.io/...`（客户交付物默认从非官方镜像源拉基础镜像）。另：`.env.example` 的 `DATABASE_URL` 指向 `base_dev`，而 prod compose 默认 `POSTGRES_DB=base` —— 照 RUNBOOK §3.1 逐字执行（只 `cp` 不改）会连错库名 | `deploy/compose.prod.yaml:23`（`POSTGRES_IMAGE` 默认第三方源）、`:28`（`POSTGRES_PASSWORD` 默认 `base`）、`:29`（`POSTGRES_DB` 默认 `base`）；`:43`（`REDIS_IMAGE` 默认第三方源）；`.env.example` 的 `DATABASE_URL=postgres://base:base@postgres:5432/base_dev`；`deploy/RUNBOOK.md:113-115` 的 §3.1 步骤 | ① prod compose 的口令默认值改为**空**并在缺失时直接报错（如 `POSTGRES_PASSWORD:?必须设置`，或启动前校验），避免弱口令静默生效；② 基础镜像默认值改为官方地址，国内/内网场景走 `${POSTGRES_IMAGE:-postgres:17-alpine}` + 文档说明内网覆盖方式；③ `.env.example` 与 prod compose 的库名保持一致（或在 RUNBOOK §3.1 加一句「库名要改成 `base`」） |
| **6** | **信息** | **本轮把上一轮需求的 `feature-meta/status.md` 与 `feature-meta/qa-report.md` 原地覆盖了。** `develop` 上的 `feature-meta/status.md` 是 **REQ-004** 的状态记录，被本轮替换为 REQ-005；本报告也覆盖了 REQ-004 的 QA 报告。内容仍在 git 历史里（`beee35f` 归档了 REQ-004 的两份报告），但**分支工作区里看不到了**；BA 的正本在 `BA/demands/REQ-005-delivery/`，不受影响 | `git diff develop..HEAD -- feature-meta/status.md`（整文件替换）；`git log --oneline -- feature-meta/qa-report.md` → `beee35f [BA] 归档 REQ-004 的 QA 首轮报告与修复复核报告`；本文件写入后即覆盖 REQ-004 版本 | 沿用现有仓库约定即可（BA 归档进 git 历史后再被下一轮覆盖），但建议 BA 在归档时把上一轮文件**改名保留**（如 `qa-report-REQ-004.md`），避免历史报告在工作区里被无声覆盖。另按 QA 角色规范「不改 `status.md`（那是 BA 的）」，本轮 `status.md` 由 Dev 改写，若状态机由 BA 维护需 BA 复核 |

---

## 未覆盖 / 无法验证

| 项 | 原因 | 影响 / 由谁补 |
|----|------|--------------|
| **镜像实构建**（体积 <100MB、容器内启动、探针在容器内的真实行为、非 root 校验） | 本机内存不足，`cargo build --release` + LTO 链接峰值远超余量（设计摘要 §8 与任务约束明确要求本机不构建）—— **本次未构建任何镜像** | Dockerfile 只做了阶段/路径/权限静态核对（3 阶段、`rust:lld`、官方地址、`USER base` uid 10001）。体积与容器行为由 CI `build-image` job 兜住（含 `docker image inspect` >100MB 硬失败 + 容器内 `migrate up` + 探针冒烟），但 **Actions 未实跑**（本仓库 `git remote -v` 为空） |
| **GitHub Actions 实跑** | 同上，仓库无 remote | CI 配置只做了 YAML 解析（12 个 job 全可枚举）与命令/仓库现状逐条核对；`pull_request_review` 触发器、build-image 体积校验均已静态确认，但**未在真实 GitHub 上跑过** |
| **真实 K8s 集群部署**（Helm install / 探针真实行为 / 蓝绿切流） | 本机无 K8s 集群（`helm` 是临时下载到 `/tmp` 校验的，未装进系统） | Chart 经 `helm lint` + `helm template` 渲染校验；探针在集群里的超时行为见问题 #3（静态推断，未实跑） |
| **Trivy 镜像漏洞扫描结果** | 需联网拉扫描器与漏洞库 | 由 CI `image-scan` job（CRITICAL/HIGH 阻断）验证 |
| **数据库 TLS** | `sqlx` 未开 TLS 特性（`tls-none`），带 `sslmode=require` 的连接串会失败 | 已知偏离（Dev 报告 §6.1 已如实登记），随 REQ-006/007 的 DB 层一并处理；本期私有化部署走内网/本地 socket |
| **`deploy/seed/` 种子脚本的实际效果** | 种子由 postgres 镜像的 initdb 钩子执行，本次未重建数据卷 | `base_dev` 中 `platform_seed_log` 表存在；种子内容未逐条核对（不属本需求 AC） |
| **`capabilities/index.json` 13 个文件与源码的逐条语义比对** | 仓库已有机器守卫 `cargo xtask capabilities --check`（本次通过，13 文件与源码一致），逐条人工语义比对未做 | 与 REQ-004 已归档结论一致，本轮未改动 `AGENTS.md` 与能力目录 |

---

## 清理确认

- 本轮自建的临时库 `base_qa_restore_drill` / `base_qa_migrate_drill` / `base_qa_irrev_drill` / `base_qa_nomig` **已全部删除**
- 测试表 `qa_probe_drill`（base_dev）**已删除**；`base_dev` 表清单回到 `_sqlx_migrations, platform_meta, platform_seed_log`（与开工前一致），迁移账本 `20261118000100|true` 完好
- 本轮产生的 3 个备份文件（`base-20261118-094753/094832/094839.sql.gz`）**已删除**；Dev 遗留的 `base-20261118-093935.sql.gz` 保留原状
- `/tmp` 下的临时迁移目录、静态目录、假仓库、helm 二进制与缓存、临时脚本**已删除**
- 无残留 `base serve` 进程；`base-postgres` / `base-redis` 均 healthy
- `git status --short` 除本报告文件外**为空**

**VERDICT: PASS-WITH-RISK**
