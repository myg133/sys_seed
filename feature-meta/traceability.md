# REQ-005 追溯性矩阵

- **需求**: REQ-005 交付基线与发布
- **分支**: `feature/REQ-005`
- **基线**: `develop` @ `37a93c5`（REQ-002 工程基线 + REQ-003 扩展点框架 + REQ-004 AI 引导体系已合并）
- **契约**: api-conventions v1、extension-points v1（已 ratified，已在 `feature-meta/manifest.json` pin）
- **设计依据**: `BA/demands/REQ-005-delivery/design-summary.md`
- **开发者**: myg133-dev-1
- **更新时间**: 2026-11-18（**微修轮**：QA 复核 PASS-WITH-RISK 后，只修复核新发现的 2 个低问题 —— 日志过滤器与 RUNBOOK `env_file` 口径）
- **本轮复核依据**: `feature-meta/qa-report-recheck.md`（QA 复核：首轮 1 中 + 4 低全部复现为已修复，新发现 **2 个低**；结论与证据见验证报告 §4.2）

图例：**PASS** = 已实现且有自动化或可复现的验证证据；**未验证** = 没有证据，如实标注。

---

## 1. 自有 AC

| AC | 内容 | 实现位置 | 验证方式与证据 | 结论 |
|----|------|----------|----------------|------|
| AC-S-005 | 配置与代码分离：同一镜像可通过环境变量切分 dev/staging/prod，无需重新构建；失败路径有明确错误码 | `crates/platform-server/src/config.rs`（`DeployConfig::from_env` / `from_lookup`）、`crates/platform-server/src/main.rs`（启动前校验 + 退出码）、`.env.example`、`deploy/compose.prod.yaml`、`deploy/helm/base-platform/templates/configmap.yaml` + `deployment.yaml` | ① **同一二进制**在 `BASE_ENV=dev` / `staging` / `prod` 三种取值下行为不同（`config::tests::same_image_switches_environment_by_env_var`）；② **fail-fast 实测**：去掉 `DATABASE_URL` 启动 → 打印 `[CONFIG_MISSING_REQUIRED] DATABASE_URL：…` 并以**退出码 2** 退出（不监听端口）；③ 非法取值（`BASE_ENV=pro` / `SERVER_PORT=not-a-port`）→ `CONFIG_INVALID_VALUE`，多个问题**一次报全**（`config::tests::all_problems_are_reported_at_once`）；④ `BASE_ENV=prod` + `BASE_SEED_ON_START=true` → `CONFIG_FORBIDDEN_COMBINATION`；⑤ 镜像里**不含任何环境相关配置**，dev/staging/prod 靠同一镜像 + 环境变量切换（compose 用 `.env`，K8s 用 ConfigMap/Secret）；⑥ 错误码稳定：`CONFIG_MISSING_REQUIRED` / `CONFIG_INVALID_VALUE` / `CONFIG_FORBIDDEN_COMBINATION` | **PASS** |
| AC-S-006 | 迁移可回滚：迁移失败时能恢复到迁移前状态，且恢复过程有演练记录 | `crates/platform-server/src/migrate.rs`、`migrations/20261118000100_platform_meta.{up,down}.sql`、`scripts/rollback.sh`、`deploy/RUNBOOK.md` §6.2 | ① **可逆迁移实测**：干净库上 `migrate up` → 建出 `platform_meta`；`migrate down --target 0` → 该表消失（只剩 sqlx 自身的 `_sqlx_migrations` 记账表）；再 `up` 可重复执行；② **不可逆保护**：`migrate down` 遇到没有 `down.sql` 的版本会返回 `MigrationError::Irreversible` 并拒绝回滚（`migrate::tests::irreversible_error_names_the_blocking_versions`）；③ `scripts/rollback.sh <版本>` 实跑：先自动备份 → 再执行 down → 打印被撤销的版本（实测 `base_migrate_drill` 上回滚到 0 成功）；④ **演练记录真实**：`deploy/RUNBOOK.md` §6.2 记录了 2026-11-18 的实际执行（备份 307ms / 恢复 704ms / 校验演练 1509ms，均为本机实测值），临时库与测试表已清理 | **PASS** |

## 2. 设计摘要 §7 交付清单

| 交付项 | 实现位置 | 验证方式与证据 | 结论 |
|--------|----------|----------------|------|
| 多阶段 Dockerfile（builder / frontend / runtime），目标镜像 < 100MB | `Dockerfile`（3 个 stage + `cargo-chef` 依赖层缓存）、`.dockerignore` | ① builder 用 `rust:lld`（**不是**官方 `rust` 镜像）、runtime 用 `debian:12-slim`、非 root（uid 10001）；② **本机未构建镜像**（低配机内存不够，见验证报告 §6）—— Dockerfile 只做**语法与阶段逻辑核对**，体积目标由 CI `build-image` job 的 `docker image inspect` 硬校验（>100MB 直接失败）；③ 前端产物路径已核对：`pnpm -r build` 实跑产出 `apps/shell/dist`（56K），与 Dockerfile 的 `COPY --from=frontend` 路径一致 | **PASS（配置层）/ 未验证（镜像实构建）** |
| `compose.yaml`（开发）+ `deploy/compose.prod.yaml`（私有化） | `compose.yaml`（新增 `migrate` 服务，带 `profiles`）、`deploy/compose.prod.yaml`、`scripts/prod-preflight.sh`、`.env.example` | ① 两个文件 `podman-compose config` 均**退出码 0**；② `podman-compose -f deploy/compose.prod.yaml config --services` → `postgres / redis / app`（**无 migrate**）；`--profile migrate` → 多出 `migrate` ⇒ **`up -d` 不会自动跑迁移**；③ 开发 compose 同样验证（默认 `postgres / redis`）；④ **私有化口令无默认值**：`POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:?…}` —— 不设 / 设为空时 compose **解析阶段报错并退出码 1**（本轮实测 `ValueError: required variable POSTGRES_PASSWORD is missing a value: …`），不会静默用弱口令建库；弱口令与库名不一致另由 `scripts/prod-preflight.sh` 拒绝（实测退出码 1）；⑤ 基础镜像默认**官方地址**（`docker.io/library/postgres:17-alpine` / `docker.io/library/redis:7-alpine`），不再是第三方镜像源；⑥ **本轮新增**：服务级 `env_file` 的相对路径按 **compose 文件所在目录**解析（`podman-compose` 1.6.0 **实测** 5 个场景 + 源码佐证；`docker compose` v2 官方文档口径一致）—— RUNBOOK §3.1 此前把 `podman-compose` 的基准**说反了**，本轮已按实测改正并删掉报错的 `ENV_FILE=deploy/.env` 备选写法（验证报告 §3.8） | **PASS** |
| `deploy/helm/base-platform/`：Chart + values.yaml | `deploy/helm/base-platform/`（Chart.yaml / values.yaml / .helmignore / templates × 8） | ① `helm lint` 通过（`1 chart(s) linted, 0 chart(s) failed`）；② `helm template` 默认实渲染出 **ServiceAccount / ConfigMap / Service / Deployment / Job 五类资源**（**不含 Ingress** —— `values.yaml` 里 `ingress.enabled` 默认 `false`；加 `--set ingress.enabled=true` 后才是六类，本轮实测）；③ 迁移 Job 带 `helm.sh/hook: pre-upgrade,pre-install` + `hook-weight: -5` + `hook-delete-policy: before-hook-creation`；④ Deployment 的 liveness→`/health/live`、readiness→`/health/ready`（`timeoutSeconds: 5` > 连接池 `acquire_timeout` 2s）；⑤ Secret **只引用不生成**（`secret.existingSecret`） | **PASS** |
| 配置外置：环境变量清单 + 启动时校验 + `.env.example` | `crates/platform-server/src/config.rs`、`crates/platform-server/src/main.rs`（`log_filter`）、`.env.example`、`deploy/RUNBOOK.md` §1 / §5.3 | 见 §1 的 AC-S-005 行；**本轮新增**：`BASE_LOG_LEVEL` **真的生效** —— 日志过滤器覆盖 **bin target `base`** 与 **lib target `platform_server`**（此前只写 `platform_server=…`，与事件实际所在的 bin target 不匹配 → **默认一行日志都不输出**）；实测 `info` / `warn` / `error` 三档符合预期（验证报告 §3.7），**+4 条单测**守着 | **PASS** |
| 迁移作为独立步骤（compose profile + Helm hook） | `crates/platform-server/src/main.rs`（`migrate` 子命令）、`compose.yaml` 与 `deploy/compose.prod.yaml` 的 `migrate` 服务、Helm 的 `migrate-job.yaml` | ① `base serve` **不跑迁移**（迁移只在 `base migrate up` 时执行）；② compose 侧 `profiles: ["migrate"]` 手动触发；③ K8s 侧 Helm hook Job 自动触发；④ CI 的 `migration-test` job 固化了 up→down→up 流程 | **PASS** |
| `scripts/backup.sh` / `restore.sh` / `rollback.sh` | `scripts/backup.sh`、`scripts/restore.sh`、`scripts/backup-verify.sh`、`scripts/rollback.sh`、`scripts/lib/pg.sh` | ① 备份实测 307ms，产物 `backup/base-<时间戳>.sql.gz`，自动轮转只留 5 份；② 恢复实测 704ms，测试表与数据都在；③ `rollback.sh` 实跑成功（先备份再回滚）；④ 本机无 psql 客户端 → 脚本自动走容器内执行，**口令不落命令行/进程列表** | **PASS** |
| `deploy/RUNBOOK.md` + **恢复演练记录** | `deploy/RUNBOOK.md`（§1 配置 / §2 镜像 / §3 私有化 / §4 K8s / §5 健康检查 + **§5.1 语言边界** + **§5.3 日志级别** / §6 备份恢复 + 演练记录 / §7 故障排查四类） | 演练记录是**真实执行结果**（日期 2026-11-18、执行人 myg133-dev-1、耗时 307/704/1509ms、发现并修掉的 3 个问题），不是模板占位；**本轮新增**：§3.1 的 `env_file` 口径按实测改正、§5.1 补「`detail` 英文 / 日志中文」的语言边界表、§5.3 补 `BASE_LOG_LEVEL` 与 target 匹配规则（验证报告 §3.7 / §3.8） | **PASS** |
| 健康检查端点（liveness / readiness） | `crates/platform-server/src/health.rs`、`crates/platform-server/src/main.rs` | ① 正常路径实测：PG 正常 → `/health/live` **200**、`/health/ready` **200**（响应含 `appliedMigrations`）；② 异常路径**本轮实测**：**停掉 PG 容器** → `/health/ready` **503** + `code=DATABASE_UNAVAILABLE`，`detail` = `database is unreachable: pool timed out while waiting for an open connection`，**耗时 2.002s**（修复前 5.003s；探针超时 5s，有 3s 余量），而 `/health/live` **仍 200**；③ 迁移未应用 → `MIGRATIONS_PENDING`，`detail` = `pending migrations: [20261118000100]`；④ 迁移 dirty → `MIGRATION_DIRTY`，`detail` = `migration 20261118000100 is in a dirty state and needs manual recovery`；⑤ 迁移表读不出来 → `MIGRATIONS_PENDING`，`detail` = `migration state is unreadable, migrations may not have run: …`；⑥ 错误体按 RFC 9457（契约 C6：`type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id`） | **PASS** |
| CI 增加构建镜像与扫描 job | `.github/workflows/ci.yml`（新增 `build-image` / `image-scan` / `migration-test`） | ① 基础镜像一律**官方地址**（`rust:lld` / `node:22-bookworm-slim` / `debian:12-slim` / `postgres:17-alpine`），未加国内镜像前缀；② `build-image` 含体积校验（>100MB 失败）+ 容器内迁移 + 健康检查冒烟；③ `image-scan` 用 Trivy（CRITICAL/HIGH 阻断）；④ `migration-test` 固化迁移可回滚；⑤ **GitHub Actions 本体未实跑**（本仓库无 remote） | **PASS（配置层）/ 未验证（Actions 实跑）** |

## 3. 顺带修的 4 项遗留技术债（QA 前几轮登记，归属本需求）

| # | 债务 | 处置 | 验证方式与证据 | 结论 |
|---|------|------|----------------|------|
| 1 | `scripts/sync-contracts.sh` 拿不到 `demand` 分支时**静默跳过 exit 0**，CI 门禁可能空转 | 改为**明确失败**：找不到正本分支 → 打印原因与修复命令 → **退出码 2**；另加 `--fetch` 参数（显式拉取后再校验）与「`manifest.json` 缺 `contracts` pin」的明确失败；CI 的 `guardrails` job 增加 `git fetch --no-tags origin demand` | ① **失败路径实测**：在无 `demand` 分支的临时仓库里跑 → 打印「✗ 找不到 BA 正本分支…本脚本不会静默跳过」并 **exit 2**；② **正常路径实测**：本仓库跑 → 两份快照一致、**exit 0**；③ CI 侧显式 fetch 后再校验 | **PASS** |
| 2 | `core-change-review` 用 `pull_request.base.sha` 做 diff，该 SHA 不随 base 前进 → 偏严 | **已修正**：改用 **merge-base(base 分支最新提交, PR head)** 作为 diff 起点，判定逻辑抽到共享脚本 `scripts/core-change-detect.sh`，两个 job（打标 / 强制评审）同源 | `scripts/core-change-detect.sh --self-test` 实跑通过：① base 侧合入核心改动、PR 只改非核心路径 → **不误报**（修复前会误报）；② PR 侧改核心路径 → **能检出**。CI 侧 `BASE_REF=origin/${{ github.base_ref }}` + `HEAD_SHA` | **PASS（已修正）** |
| 3 | `core-change-review` 的 diff 判定只看文件路径 | **确认覆盖**：`crates/platform-core` / `crates/platform-contracts` / `crates/platform-contracts-macros`（REQ-003 新增的 macros 已算核心）三条都在清单里；脚本会比对 `crates/` 下每个平台 crate 是否都在清单里，新增平台 crate 忘了登记会**直接失败**（`crates/platform-server` 与 `crates/xtask` 显式豁免并写明理由）。**本轮把该检查并进 `--self-test` 入口**（上一轮只在主流程校验，自检入口覆盖不到） | ① `scripts/core-change-detect.sh` 的 `check_core_coverage` 实跑通过（exit 0）；② `scripts/core-change-detect.sh --self-test` → `✓ 自检通过：merge-base 口径不误算 base 侧改动，PR 侧核心改动能检出，未登记平台 crate 能检出`（**exit 0**）—— 自检里额外构造「临时仓库建未登记的 `crates/other` → 必须检出」与「全部 crate 已登记 → 必须放行」两个场景；③ 豁免名单带理由注释 | **PASS** |
| 4 | `migrations/` 只有 README 占位就能过 `check-modules` 的「非空」校验 | **规则暂不改**（本期无业务 DB 层），但**提示写进两处**：`modules/sandbox/migrations/README.md` 新增「规则何时升级」段；`crates/xtask/src/modules.rs` 的空目录提示单列一条，明确「DB 层落地后规则将升级为至少一个 `*.sql`」 | ① `cargo xtask check-modules` 仍通过（规则未改）；② `xtask::tests::empty_migrations_dir_explains_the_future_rule` 断言提示语含 `*.sql` 与「DB 层落地后」；③ README 与提示语口径一致 | **PASS** |

## 4. 契约与既有规则的符合性

| 约定 | 实现位置 | 验证方式与证据 | 结论 |
|------|----------|----------------|------|
| C6 统一错误响应体（RFC 9457 + 稳定 `code` + `trace_id`） | `crates/platform-server/src/main.rs`（`ProblemDetails`）、`crates/platform-server/src/health.rs`（`dev_detail` / `detail_*`） | 未就绪响应实测含 `type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id`；**`detail` 在三个 readiness 错误路径上全部为英文**（`database is unreachable: …` / `pending migrations: […]` / `migration {version} is in a dirty state and needs manual recovery` / `migration state is unreadable, …`），**不含任何中文字符** —— 符合 C6「后端不得在 `detail` 里拼中文句子」；运维视角的中文说明只进日志（`tracing::warn!` 记 `code` + `detail` + `pending` + `dirty`）。5 条单测守着（`health::tests::readiness_detail_is_english_on_every_error_path` 等）。**上一轮此处写的「`detail`（英文），符合 C6」是不实结论**（当时只验了「数据库不可达」那一条路径），本轮已按三条路径的实测响应体更正；**本轮（微修轮）复核**：C6 未动、`detail` 仍为纯 ASCII 英文，运维中文说明只在日志里（实测 `WARN base: readiness 未通过：…`，验证报告 §3.7），两条通道的边界已写进 RUNBOOK §5.1 的语言边界表 | **PASS** |
| C13 命名与编码（JSON camelCase、DB snake_case、UTC） | 健康检查成功响应、迁移表列名 | 成功响应为 `appliedMigrations` / `traceId`（camelCase）；迁移表列名 `version` / `success` / `checksum`（snake_case）；容器时区固定 UTC | **PASS** |
| 硬规则 1：不得修改 `crates/platform-core/` 与 `crates/platform-contracts/` | `git diff 31ca87a..HEAD -- crates/platform-core crates/platform-contracts crates/platform-contracts-macros` | **输出为空** —— 本需求一行未改这两个 crate（部署侧能力放在新 crate `crates/platform-server`） | **PASS** |
| 硬规则 2：业务模块只依赖扩展点 trait | `cargo xtask check-deps` | 实跑通过：平台 crate 未反向依赖业务模块 | **PASS** |
| AGENTS.md「尚未提供」段不被夸大 | `AGENTS.md` / `crates/xtask/src/catalog.rs` / `capabilities/index.json` | **未改动** `AGENTS.md` 与能力目录 ⇒ `db_access_layer` 仍如实标为「尚未提供」（业务数据访问层仍待 REQ-006/007）；`cargo xtask capabilities --check` 通过（13 个文件与源码一致） | **PASS** |
| 目录规范（`cargo xtask check-modules`） | `modules/sandbox/` | 实跑通过 | **PASS** |

## 5. 未验证 / 不成立的部分（如实登记）

| 项 | 原因 | 影响 |
|----|------|------|
| **镜像实构建**（`docker build`） | 本机 4.2G 可用内存，`cargo build --release` + LTO 的链接峰值远超余量（设计摘要 §8 与任务约束明确要求本机不构建） | Dockerfile 只做了阶段/路径核对；体积、启动、探针在容器里的真实行为由 CI `build-image` job 验证（**Actions 未实跑**，本仓库无 remote） |
| **GitHub Actions 实跑** | 本仓库没有 remote（`git remote -v` 为空），云端 CI 跑不起来 | CI 配置只做了 YAML 解析校验（`yaml.safe_load` 通过，12 个 job）+ 命令与仓库现状逐条核对 |
| **Helm 在真实集群部署** | 本机无 K8s 集群 | Chart 经 `helm lint` + `helm template` 渲染校验（helm 二进制临时下载到 `/tmp` 校验，未安装到系统） |
| **数据库 TLS** | `sqlx` 未开 TLS 特性（`tls-none`） | `DATABASE_URL` 带 `sslmode=require` 会连接失败；私有化部署的库走内网/本地 socket，TLS 需求随 REQ-006/007 的 DB 层一并处理 |
