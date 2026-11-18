# REQ-005 状态

- **需求**: REQ-005 交付基线与发布
- **分支**: `feature/REQ-005`
- **基线**: `develop` @ `37a93c5`（REQ-002 工程基线 + REQ-003 扩展点框架 + REQ-004 AI 引导体系已合并）
- **状态**: **待验证**（微修轮完成：QA 复核 PASS-WITH-RISK 后，只修复核新发现的 2 个低问题 → 实测 → 提交，等待 QA 复核）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`
- **本轮依据**: `feature-meta/qa-report-recheck.md`（QA 复核结论 PASS-WITH-RISK：首轮 1 中 + 4 低**全部复现为已修复**，新发现 **2 个低**：`BASE_LOG_LEVEL` 完全无效 / RUNBOOK §3.1 把 `podman-compose` 的 `env_file` 基准说反了）

## 本轮交付

底座**同一套镜像与配置**，三处复用：开发（本机 compose）/ 私有化部署（企业内网 compose）/ 云上（K8s Helm）。

| 交付项 | 位置 |
|--------|------|
| 多阶段 Dockerfile（builder / frontend / runtime，cargo-chef 分层缓存） | `Dockerfile`、`.dockerignore` |
| 开发 compose（新增 `migrate` 服务，带 profile） | `compose.yaml` |
| 私有化部署 compose（迁移独立服务，`up -d` 不自动跑） | `deploy/compose.prod.yaml` |
| Helm Chart（Deployment / Service / ConfigMap / Secret 引用 / 迁移 hook Job；Ingress 由 `ingress.enabled` 控制，默认关） | `deploy/helm/base-platform/` |
| 配置外置 + 启动 fail-fast 校验 | `crates/platform-server/src/config.rs`、`.env.example` |
| 健康检查（liveness / readiness 含数据库与迁移检查） | `crates/platform-server/src/health.rs`、`src/main.rs` |
| 迁移独立化（`base migrate up/down/status` + 可逆迁移文件） | `crates/platform-server/src/migrate.rs`、`migrations/` |
| 备份恢复与回滚脚本 | `scripts/backup.sh`、`restore.sh`、`backup-verify.sh`、`rollback.sh`、`scripts/lib/pg.sh` |
| 运维手册（含**真实**演练记录与故障排查） | `deploy/RUNBOOK.md` |
| CI 增补（构建镜像 + 体积校验 + 漏洞扫描 + 迁移可回滚） | `.github/workflows/ci.yml` |

## 自验结论

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **142/142**（上一轮 138 个全绿，本轮微修轮 **+4** 只增不减）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ 0 告警
- `cargo fmt --all --check` ✅
- `pnpm -r test` ✅ 23/23 / `bun run test:e2e` ✅ 1/1 / `pnpm exec eslint .` ✅ / `pnpm exec prettier --check .` ✅
- `cargo xtask capabilities --check` ✅ 13 个文件一致 / `check-deps` ✅ / `check-modules` ✅
- **`podman-compose -f compose.yaml config`** ✅ / **`podman-compose -f deploy/compose.prod.yaml config`** ✅
- **`up -d` 不自动跑迁移** ✅（`config --services` 默认无 `migrate`；`--profile migrate` 才有）
- **配置 fail-fast** ✅ 缺 `DATABASE_URL` → `CONFIG_MISSING_REQUIRED` + **退出码 2**（不监听端口）
- **健康检查** ✅ PG 正常：live 200 / ready 200；**停库后**：ready **503**（`DATABASE_UNAVAILABLE`）/ live 仍 **200**（已把 PG 容器起回来）；**修复轮复测**：ready 返回 503 耗时 **2.002s**（修复前 5.003s，探针超时 5s 有余量）
- **契约 C6** ✅ 三个 readiness 错误路径的 `detail` **全部为英文**、不含任何中文字符（`database is unreachable: …` / `pending migrations: […]` / `migration … is in a dirty state …` / `migration state is unreadable, …`），5 条单测守着
- **私有化口令缺口令** ✅ 不设 `POSTGRES_PASSWORD` → compose 解析阶段报错 + **退出码 1**（不静默用弱口令）；弱口令与库名不一致由 `scripts/prod-preflight.sh` 拒绝；基础镜像默认官方地址
- **备份恢复闭环** ✅ 备份 **307ms** / 恢复 **704ms**（测试表与数据都在）/ 校验演练 **1509ms**（临时库已清理）
- **迁移可回滚** ✅ `up` → `down --target 0`（业务表清空）→ 再 `up`；`scripts/rollback.sh 0` 实跑通过
- **`scripts/sync-contracts.sh` 失败路径** ✅ 拿不到 `demand` → 明确报错 + **退出码 2**（不再静默跳过）
- **`scripts/core-change-detect.sh --self-test`** ✅ merge-base 口径不误算 base 侧改动，PR 侧核心改动能检出，**未登记平台 crate 能检出**（本轮修复轮把 crate 登记检查并进了自检入口）
- **`helm lint`** ✅ / **`helm template`** ✅ 默认渲染 **5 类**（ServiceAccount / ConfigMap / Service / Deployment / Job，**不含 Ingress**）；`--set ingress.enabled=true` 后才是 6 类（含 Ingress）—— 上一轮「六类资源」的说法是**不实结论**，本轮已按实测更正
- **前端静态托管** ✅ `/` 200、SPA 回落 200、路径逃逸 404、缺文件 404

## 本轮修复轮（QA 退回的 5 项）

| # | QA 登记的问题 | 复核结论 | 落地位置 |
|---|--------------|---------|---------|
| 中 1 | 健康检查 `detail` 塞中文，违反契约 C6；上一轮「符合 C6」是**不实结论** | ✅ **已修完并复验**：三条路径 `detail` 全英文（实测响应体见验证报告 §3.2），5 条单测守着；上一轮的不实表述已在验证报告与追溯矩阵里更正 | `crates/platform-server/src/health.rs` |
| 低 1 | RUNBOOK 称 Helm 默认渲染含 Ingress | ✅ **已修完并复验**：RUNBOOK §4 改正（默认 5 类不含 Ingress），本轮 `helm template` 实测确认；上一轮报告与追溯矩阵的「六类资源」表述一并更正 | `deploy/RUNBOOK.md` §4 |
| 低 2 | readinessProbe 5s 与连接池 acquire_timeout 5s 相撞 | ✅ **已修完并复验**：`acquire_timeout` → **2s**，ready 503 实测 **2.002s**（修复前 5.003s），1 条单测守着这个关系 | `crates/platform-server/src/db.rs` |
| 低 3 | `core-change-detect.sh --self-test` 未覆盖「平台 crate 必须登记」 | ✅ **已修完并复验**：检查并入 `self_test()`，本轮实跑 exit 0 | `scripts/core-change-detect.sh` |
| 低 4 | prod compose 默认弱口令 + 第三方镜像源 | ✅ **已修完并复验**：`${POSTGRES_PASSWORD:?…}` 缺失/空值 → 报错退出 1；镜像默认值改官方地址；另补 `scripts/prod-preflight.sh` 拦弱口令与库名不一致 | `deploy/compose.prod.yaml`、`.env.example`、`scripts/prod-preflight.sh` |

本轮补的漏：RUNBOOK §3.1 与 compose 头部的 `./scripts/prod-preflight.sh` **路径不存在**（该段已 `cd deploy`）→ 改 `../scripts/prod-preflight.sh`，compose 头部用法统一成「在 `deploy/` 目录里执行」。

## 本轮微修轮（QA 复核新发现的 2 个低问题）

QA 复核结论 **PASS-WITH-RISK**：首轮登记的 **1 中 + 4 低全部复现为已修复**，新发现 **2 个低**。本轮只修这 2 项。

| # | QA 登记的问题 | 复核结论 | 落地位置 |
|---|--------------|---------|---------|
| 低 1 | `BASE_LOG_LEVEL` **完全无效**：过滤器只匹配 lib crate 的 target `platform_server`，而日志事件实际来自 **bin target `base`** → 默认配置下**一行日志都不输出**，RUNBOOK §5.1「运维中文说明在日志里」这条兜底通道是**死的** | ✅ **已修完并复验**：过滤器同时覆盖 `base` 与 `platform_server`（`log_filter()`）；**实测 `BASE_LOG_LEVEL=info` 真的有日志输出**（`INFO base: 底座服务已启动 …`），`warn` / `error` 两档也符合预期；**+4 条单测**守着；RUNBOOK §5.1 补**语言边界表**（响应体 `detail` 永远英文 / 运维中文说明永远在日志里），§5.3 补日志级别与 target 说明 | `crates/platform-server/src/main.rs`、`deploy/RUNBOOK.md` §5.1 / §5.3 |
| 低 2 | RUNBOOK §3.1 把 `podman-compose` 的 `env_file` 解析基准**说反了**，且备选写法 `ENV_FILE=deploy/.env` 实测报 `deploy/deploy/.env does not exist` | ✅ **已修完并复验**：按**实测**改正（两种实现都按 **compose 文件所在目录**解析，附 `docker compose` 官方文档口径与 `podman-compose` 源码佐证）；**删掉坏的备选写法**；推荐用法统一为**方案 A：在 `deploy/` 目录里执行**并**实跑通过**；compose 头部同步加了 `env_file` 基准说明 | `deploy/RUNBOOK.md` §3.1、`deploy/compose.prod.yaml` 头部 |

⚠️ **契约 C6 未动**：本轮**没有**把 `detail` 改回中文 —— 响应体 `detail` 仍是纯 ASCII 英文，中文只在日志里。
⚠️ **`docker compose` v2 未在本机实测**：本机无 compose 插件（只有 `podman-compose` 1.6.0），该实现口径来自官方文档（见验证报告 §3.8）。

## 顺带修的 4 项遗留技术债

| # | 债务 | 结果 |
|---|------|------|
| 1 | `sync-contracts.sh` 静默跳过 | ✅ 改为明确失败（exit 2）；CI 显式 fetch `demand` 后再校验 |
| 2 | `core-change-review` 用 `base.sha` 偏严 | ✅ 改为 **merge-base** 口径，判定抽到共享脚本并带自检 |
| 3 | 核心 diff 只看路径 | ✅ 确认覆盖三个核心 crate；「新增平台 crate 必须登记」的检查**已并进 `--self-test` 入口**（本轮修复轮补齐） |
| 4 | `migrations/` 占位即可过校验 | ✅ 规则不改，提示写进 README 与 `check-modules` 错误提示（DB 层落地后升级为至少一个 `*.sql`） |

## 新增依赖

- 第三方：**`sqlx` 0.8**（`runtime-tokio` + `postgres` + `migrate`；关闭 `macros` / `json` / TLS 特性，`cargo tree` 实测 `sqlx-macros` **不参与编译**）
  —— 只用于部署期健康检查与迁移，**不引入业务数据访问层**（AGENTS.md 的 `db_access_layer` 仍标为「尚未提供」）
- workspace crate：新增 1 个（`crates/platform-server`）
- 未改动 `crates/platform-core/`、`crates/platform-contracts/`（`git diff` 为空）

## 未验证（请 QA 重点看）

1. **镜像实构建**（体积 <100MB、容器内启动与探针）—— 本机内存不够，交给 CI `build-image` job；GitHub Actions 因仓库无 remote **未实跑**
2. **真实 K8s 集群部署** —— Chart 经 `helm lint` + `helm template` 校验，未上集群
3. **Trivy 扫描结果** —— 需联网拉漏洞库
4. **数据库 TLS** —— `sqlx` 未开 TLS 特性（`tls-none`），随 REQ-006/007 的 DB 层处理

## 请派 QA 复验

本轮是**微修轮**：只修复核新发现的 2 个低问题（结论见上表与 `feature-meta/verification-report.md` §4.2），
请 QA 按 `feature-meta/qa-report-recheck.md` 的 2 项逐条复验，重点看：
① `BASE_LOG_LEVEL=info` 时后端**是否真的有日志输出**（且 `warn` / `error` 两档是否符合预期）；
② RUNBOOK §3.1 的 `env_file` 口径是否与实测一致、**推荐用法（在 `deploy/` 里执行）是否真的能跑通**；
③ 顺带确认：响应体 `detail` **仍是纯 ASCII 英文**（C6 未动），中文只在日志里。
