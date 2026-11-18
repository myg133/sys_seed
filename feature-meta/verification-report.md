# REQ-005 验证报告

- **需求**: REQ-005 交付基线与发布
- **分支**: `feature/REQ-005`
- **基线**: `develop` @ `37a93c5`（REQ-002 工程基线 + REQ-003 扩展点框架 + REQ-004 AI 引导体系已合并）
- **设计依据**: `BA/demands/REQ-005-delivery/design-summary.md`
- **契约**: api-conventions v1、extension-points v1（已 ratified，已在 `feature-meta/manifest.json` pin）
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: 第二轮（QA 退回后的**修复轮** —— 接管上一轮未提交的改动，复核 + 补漏 + 实测 + 提交）
- **本轮修复依据**: `feature-meta/qa-report.md`（QA Pre-merge 结论 PASS-WITH-RISK，登记 5 项问题）

---

## 0. 摘要

| 项 | 结果 |
|----|------|
| 改动 | **48 文件 / +5126 -575 行**（基线 `31ca87a` → 本轮；删除行主要来自 REQ-004 的 `feature-meta/*.md` 被本轮文档替换） |
| 新增第三方依赖 | **`sqlx` 0.8**（`runtime-tokio` + `postgres` + `migrate`，**关闭** `macros` / `json` / TLS 特性）；锁文件 107 → 234 个 crate，其中 `sqlx-macros` / `sqlx-mysql` / `sqlx-sqlite` 只进锁文件**不参与编译**（`cargo tree` 实测 0 处引用） |
| 新增 crate | `crates/platform-server`（交付基线服务：`base` 二进制）——**未改动** `platform-core` / `platform-contracts` |
| Rust 测试 | **138/138 通过**（上一轮 132 个全绿，**只增不减**；本轮修复轮 **+6**：`health.rs` 5 个 + `db.rs` 1 个） |
| 前端测试 | **23/23**（`pnpm -r test`） |
| E2E | **1/1**（Playwright chromium） |
| lint | `cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` ✅ 0 告警 / `pnpm exec eslint .` ✅ / `pnpm exec prettier --check .` ✅ |
| compose 校验 | `podman-compose -f compose.yaml config` ✅ / `podman-compose -f deploy/compose.prod.yaml config` ✅；**`up -d` 不会自动跑迁移**（profile 验证通过）；**缺 `POSTGRES_PASSWORD` → 报错 + 退出码 1**（本轮实测，见 §3.5） |
| Helm | `helm lint` ✅（0 failed）/ `helm template` ✅ 默认渲染 **5 类资源**（ServiceAccount / ConfigMap / Service / Deployment / Job，**不含 Ingress**）；`--set ingress.enabled=true` 才渲染出 Ingress（共 6 类） |
| 健康检查 | live **200** / ready **200**（正常）；停库后 ready **503**（`DATABASE_UNAVAILABLE`，**实测 2.002s**）/ live 仍 **200**；三个错误路径的 `detail` **全部为英文**（本轮实测，见 §3.2） |
| 配置 fail-fast | 缺 `DATABASE_URL` → 打印 `CONFIG_MISSING_REQUIRED` 并 **退出码 2** |
| 备份恢复闭环 | 备份 **307ms** / 恢复 **704ms** / 校验演练 **1509ms**，全部实测通过（临时库与测试表已清理） |
| 迁移可回滚 | `up` → `down --target 0` → 业务表清空 → 再 `up`，实测通过 |
| 4 项技术债 | 逐条已修（§4），每条都有可复现的验证命令 |
| ★ 未验证 | **镜像实构建**（本机内存不够）、**GitHub Actions 实跑**（仓库无 remote）、**真实 K8s 集群部署**（§6 如实登记） |

---

## 1. 交付形态：同一套镜像 + 同一套配置，三处复用

| 形态 | 编排 | 迁移触发方式 | 入口 |
|---|---|---|---|
| 开发（本机） | `compose.yaml`（rootless podman 6.2.0 / podman-compose 1.6.0） | `cargo run -p platform-server --bin base -- migrate up` | `docs/dev-environment.md` |
| 私有化部署（企业内网单机） | `deploy/compose.prod.yaml` | `docker compose --profile migrate run --rm migrate`（**手动**） | `deploy/RUNBOOK.md` §3 |
| 云上（K8s） | `deploy/helm/base-platform` | Helm hook Job（`pre-install,pre-upgrade`，**自动**） | `deploy/RUNBOOK.md` §4 |

镜像里**不含任何环境相关配置**：dev/staging/prod 的差异全部来自环境变量
（`BASE_ENV` / `BASE_TENANT_MODE` / `BASE_AUTH_MODE` / `BASE_SEED_ON_START` / 连接串）。

---

## 2. 镜像（Dockerfile）

```text
stage builder   rust:lld          → /app/target/release/base   （cargo-chef 分层缓存依赖）
stage frontend  node:22-bookworm-slim → apps/shell/dist        （pnpm -r build）
stage runtime   debian:12-slim    → base + /app/static + /app/migrations
```

- **为什么不用官方 `rust` 镜像**：其 GNU ld 链接内存是本机问题的放大版（设计摘要 §2.1）。
- **依赖层缓存**：`cargo chef prepare` → `cargo chef cook --release --locked` → 才编业务代码；
  源码改动不重编依赖。
- **体积**：目标 < 100MB。runtime 只装 `ca-certificates`、非 root（uid 10001）、
  `readOnlyRootFilesystem`（K8s 侧）。CI `build-image` job 用 `docker image inspect`
  **硬校验** >100MB 即失败。
- **未在本机构建**（见 §6）。

---

## 3. 实测记录（本机，2026-11-18）

### 3.1 配置 fail-fast

```console
$ unset DATABASE_URL && base serve
✗ 配置校验失败，进程退出（fail-fast）：
配置校验失败（1 项）
  - [CONFIG_MISSING_REQUIRED] DATABASE_URL：必填环境变量缺失或为空；数据库连接串必须显式提供，不使用默认值；示例：postgres://user:pass@host:5432/base
提示：必填项见 deploy/RUNBOOK.md 与 .env.example；生产用 K8s Secret / compose .env 注入。
$ echo $?
2
```

非法取值（`BASE_ENV=pro`、`SERVER_PORT=not-a-port`、`BASE_AUTH_MODE=ldap`、
`BASE_TENANT_MODE=row`）→ 4 条问题**一次报全**，错误码 `CONFIG_INVALID_VALUE`；
`BASE_ENV=prod` + `BASE_SEED_ON_START=true` → `CONFIG_FORBIDDEN_COMBINATION`。

### 3.2 健康检查（本轮**实测**，2026-11-18 10:05-10:08）

**正常路径**（PG 正常，`base_dev` 已应用迁移 `20261118000100`）：

```console
$ curl -s -w '\nHTTP %{http_code}  time=%{time_total}s\n' http://127.0.0.1:18080/health/live
{"status":"alive","traceId":"2c435925-eff3-4ae4-a493-4ea605448fd6","version":"0.1.0"}
HTTP 200  time=0.001646s
$ curl -s -w '\nHTTP %{http_code}  time=%{time_total}s\n' http://127.0.0.1:18080/health/ready
{"appliedMigrations":[20261118000100],"environment":"dev","status":"ready","traceId":"5b2f0d5d-49a4-4556-9503-f2d66cd23381","version":"0.1.0"}
HTTP 200  time=0.170653s
```

**路径 1 —— `DATABASE_UNAVAILABLE`**（`podman stop base-postgres`）：

```console
$ podman stop base-postgres
$ curl -s -w '\nHTTP %{http_code}  time=%{time_total}s\n' http://127.0.0.1:18080/health/live
{"status":"alive","traceId":"bc9dc3d5-66ce-487e-9868-704a3c85c926","version":"0.1.0"}
HTTP 200  time=0.001938s                          ← 进程还活着
$ curl -s -w '\nHTTP %{http_code}  time=%{time_total}s\n' http://127.0.0.1:18080/health/ready
{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,"detail":"database is unreachable: pool timed out while waiting for an open connection","instance":"/health/ready","code":"DATABASE_UNAVAILABLE","trace_id":"2c8ae27f-c030-4357-a2ab-84d239f3c849"}
HTTP 503  time=2.002441s                         ← 2.0s ≪ 探针超时 5s
$ curl -s -w '\nHTTP %{http_code}  time=%{time_total}s\n' http://127.0.0.1:18080/health/ready   # 复测
HTTP 503  time=2.002119s
```

**恢复**（`podman start base-postgres`）：

```console
$ podman start base-postgres && sleep 6
$ curl -s -w '\nHTTP %{http_code}  time=%{time_total}s\n' http://127.0.0.1:18080/health/ready
{"appliedMigrations":[20261118000100],"environment":"dev","status":"ready","traceId":"18920fe6-0620-4498-9b18-535c0d193a10","version":"0.1.0"}
HTTP 200  time=0.176866s
```

**路径 2 —— `MIGRATIONS_PENDING`**（连上了但迁移没跑：临时库 `base_dev005`，
`base migrate up` 后 `base migrate down --target 0`，`_sqlx_migrations` 表在、账本空）：

```console
$ curl -s -w '\nHTTP %{http_code}  time=%{time_total}s\n' http://127.0.0.1:18080/health/ready
{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,"detail":"pending migrations: [20261118000100]","instance":"/health/ready","code":"MIGRATIONS_PENDING","trace_id":"00d87c93-e2f9-41e3-bd93-77cd6ea8ba03"}
HTTP 503  time=0.141962s
```

**路径 3 —— `MIGRATION_DIRTY`**（同一临时库，`UPDATE _sqlx_migrations SET success=false`）：

```console
$ curl -s -w '\nHTTP %{http_code}  time=%{time_total}s\n' http://127.0.0.1:18080/health/ready
{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,"detail":"migration 20261118000100 is in a dirty state and needs manual recovery","instance":"/health/ready","code":"MIGRATION_DIRTY","trace_id":"4df54f33-4778-40fb-807e-79ed742b687b"}
HTTP 503  time=0.002321s
```

**路径 2 的另一个分支**（迁移表整个读不出来 —— `DROP TABLE _sqlx_migrations`）：

```console
$ curl -s -w '\nHTTP %{http_code}  time=%{time_total}s\n' http://127.0.0.1:18080/health/ready
{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,"detail":"migration state is unreadable, migrations may not have run: error returned from database: relation \"_sqlx_migrations\" does not exist","instance":"/health/ready","code":"MIGRATIONS_PENDING","trace_id":"b87f1317-d2f2-41a9-8066-fc9bd078a5a5"}
HTTP 503  time=0.002425s
```

> **契约 C6 的实际结论（本轮更正）**：
> 错误体符合 RFC 9457（`type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id`），
> `code` 稳定、`trace_id` 必带；
> **`detail` 在全部三个 readiness 错误路径上都是英文**（`database is unreachable: ...` /
> `pending migrations: [...]` / `migration {version} is in a dirty state and needs manual recovery` /
> `migration state is unreadable, ...`），**不含任何中文字符**。
>
> ⚠️ 更正说明：**上一轮报告在这里写的「错误体符合契约 C6（…，`detail` 为英文）」是不实结论** ——
> 上一轮的自验只覆盖了「数据库不可达」那一条路径（该路径本来就是英文），
> 另外两条路径（`MIGRATIONS_PENDING` / `MIGRATION_DIRTY`）当时塞的是中文句子
> （`待应用迁移：[...]` / `迁移 … 处于 dirty 状态，需人工处理`），QA 已独立复现。
> 本轮修复后才有资格下「`detail` 为英文」这个结论，结论依据是上面的**实测响应体**，
> 另有 5 条单测守着（`health::tests::readiness_detail_is_english_on_every_error_path` 等）。
>
> `title` 保持中文短句（`服务未就绪`）是**符合契约**的：C6 只约束 `detail`，
> 契约自己的示例里 `title` 就是中文（`"title": "权限不足"`），
> 面向用户的中文文案由前端按 `code` 做 i18n 渲染（见 REQ-021 AC-S-029）。

### 3.3 迁移可回滚

```console
# 干净库 base_migrate_drill
$ base migrate up
迁移：已应用版本 [20261118000100]
$ psql -Atc "SELECT table_name FROM information_schema.tables WHERE table_schema='public'"
_sqlx_migrations
platform_meta

$ base migrate down --target 0
迁移：已回滚版本 [20261118000100] → 目标版本 0
$ psql -Atc "SELECT table_name FROM information_schema.tables WHERE table_schema='public'"
_sqlx_migrations          ← 业务表已清空（只剩 sqlx 自己的记账表）

$ base migrate up
迁移：已应用版本 [20261118000100]      ← 可重复执行
```

`scripts/rollback.sh 0` 实跑：先自动备份（打印备份路径）→ 执行 down → 打印被撤销版本。

### 3.4 备份 / 恢复闭环

```console
$ ./scripts/backup.sh
▸ 备份数据库 base_dev（模式：container）→ backup/base-20261118-093935.sql.gz
✓ 备份完成：backup/base-20261118-093935.sql.gz（4.0K）      # 耗时 307 ms

$ ./scripts/restore.sh backup/base-20261118-093935.sql.gz base_runbook_drill
▸ 目标数据库：base_runbook_drill（删除后重建 + 导入）
✓ 恢复完成：base_runbook_drill ← backup/base-20261118-093935.sql.gz   # 耗时 704 ms

$ psql -d base_runbook_drill -Atc "SELECT id||'|'||note FROM runbook_probe"
1|runbook-drill                                              # 测试表与数据都在

$ ./scripts/backup-verify.sh
▸ 校验备份：backup/base-20261118-093935.sql.gz
  ✓ 备份已导入临时库
  ✓ 冒烟 1：连接与查询正常
  ✓ 冒烟 2：迁移表存在
  ✓ 冒烟 3：platform_meta 可查询
✓ 备份校验通过：...（4 张表，耗时 1s，临时库已清理）           # 耗时 1509 ms
```

演练产物（`base_runbook_drill` / `base_verify_*` / `base_migrate_drill` 临时库与
`runbook_probe` / `restore_drill_probe` 测试表）**已全部清理**。

### 3.5 compose 与 Helm（本轮**实测**）

```console
$ podman-compose -f compose.yaml config                       # exit 0
$ podman-compose -f deploy/compose.prod.yaml config           # exit 0（带口令时）
$ podman-compose -f deploy/compose.prod.yaml config --services
postgres
redis
app                                                      ← 没有 migrate：up -d 不会自动跑迁移
$ podman-compose --profile migrate -f deploy/compose.prod.yaml config --services
postgres
redis
migrate
app

$ helm lint deploy/helm/base-platform
1 chart(s) linted, 0 chart(s) failed
$ helm template base-platform deploy/helm/base-platform | grep -E '^kind:'
kind: ServiceAccount
kind: ConfigMap
kind: Service
kind: Deployment
kind: Job                                      ← 默认 5 类，**不含 Ingress**
$ helm template base-platform deploy/helm/base-platform --set ingress.enabled=true | grep -E '^kind:'
kind: ServiceAccount
kind: ConfigMap
kind: Service
kind: Deployment
kind: Ingress                                 ← 加 --set ingress.enabled=true 后才有 6 类
kind: Job
```

**私有化 compose 的口令缺口令（QA 问题 #4 的验收点）** ——
不设 `POSTGRES_PASSWORD` 时，compose **解析阶段**就报错退出（非 0），不会静默用弱口令建库：

```console
$ env -u POSTGRES_PASSWORD podman-compose -f deploy/compose.prod.yaml config
ValueError: required variable POSTGRES_PASSWORD is missing a value: POSTGRES_PASSWORD is required - inject it via .env or a Docker secret (see .env.example / deploy/RUNBOOK.md 3.1)
$ echo $?
1
```

`deploy/.env` 里 `POSTGRES_PASSWORD=`（**空值**）同样报错退出 1；
给了有效口令则 `config` **exit 0**，且渲染出的口令与镜像地址正确：

```console
$ POSTGRES_PASSWORD='S3cure-Passw0rd-Prod' podman-compose -f deploy/compose.prod.yaml config
      POSTGRES_DB: base
      POSTGRES_PASSWORD: S3cure-Passw0rd-Prod
    image: docker.io/library/postgres:17-alpine     ← 官方地址（不再是第三方镜像源）
    image: docker.io/library/redis:7-alpine
$ echo $?
0
```

弱口令 / 库名不一致由 `scripts/prod-preflight.sh` 拦（compose 只管「有没有」，不管「强不强」）：

```console
$ ENV_FILE=weak.env ./scripts/prod-preflight.sh
✗ 缺少 POSTGRES_PASSWORD：生产口令必须由 .env / Docker secret 显式注入（compose 里没有默认值）。
✗ POSTGRES_PASSWORD 是弱口令：生产库不接受默认/示例口令。
✗ DATABASE_URL 里的口令是弱口令（生产库不接受默认/示例口令）。
✗ 生产部署前置校验不通过（2 项）—— 不要继续 up -d
$ echo $?
1
$ ENV_FILE=ok.env ./scripts/prod-preflight.sh
✓ 生产部署前置校验通过：口令已显式注入且非弱口令，库名 'base' 与 DATABASE_URL 一致
$ echo $?
0
```

> ⚠️ **更正说明**：上一轮报告与追溯矩阵里写的
> 「`helm template` 实渲染出 ServiceAccount / ConfigMap / Service / Deployment / **Ingress** / Job 六类资源」
> 是**不实结论** —— `values.yaml` 里 `ingress.enabled` 默认 `false`，默认渲染**不含 Ingress**。
> RUNBOOK §4 的自检命令与注释本轮已按实测改正。

### 3.6 前端静态托管（Rust 侧，少一个进程）

```console
$ curl -s -o /dev/null -w '%{http_code} %{content_type}\n' http://127.0.0.1:18081/
200 text/html; charset=utf-8
$ curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:18081/some/deep/route
200                                    ← SPA 路由回落 index.html
$ curl -s -o /dev/null -w '%{http_code}\n' --path-as-is http://127.0.0.1:18081/../../../etc/passwd
404                                    ← 路径逃逸被拒
$ curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:18081/assets/nope.js
404
```

---

## 4. 4 项遗留技术债（QA 前几轮登记，归属本需求）

| # | 债务 | 处置 | 验证命令与结果 |
|---|------|------|----------------|
| 1 | `sync-contracts.sh` 拿不到 `demand` 时静默跳过 exit 0 | 改为**明确失败**（exit 2 + 原因 + 修复命令）；新增 `--fetch`；CI 里显式 `git fetch --no-tags origin demand` 后再校验；另加「`manifest.json` 缺 `contracts` pin」也明确失败 | 失败路径：在无 `demand` 的临时仓库跑 → `✗ 找不到 BA 正本分支（demand / origin/demand）…本脚本不会静默跳过` **exit 2**；正常路径：本仓库 → 两份快照一致 **exit 0** |
| 2 | `core-change-review` 用 `pull_request.base.sha`（不随 base 前进，偏严） | **已修正**为 merge-base 口径；判定抽到共享脚本 `scripts/core-change-detect.sh`，打标与强制评审两个 job 同源 | `./scripts/core-change-detect.sh --self-test` → `✓ 自检通过：merge-base 口径不误算 base 侧改动，PR 侧核心改动能检出，未登记平台 crate 能检出` **exit 0**（自检里构造了「base 侧改核心 + PR 侧只改非核心」与「PR 侧改核心」两个场景） |
| 3 | 核心 diff 判定只看路径 | 清单确认覆盖 `platform-core` / `platform-contracts` / `platform-contracts-macros`；`crates/` 下每个平台 crate 都必须在清单里，忘了登记直接失败（`platform-server` / `xtask` 显式豁免并写明理由）。**本轮把这条检查并进了 `--self-test` 入口**（上一轮只在主流程校验，自检入口覆盖不到） | `BASE_REF=develop HEAD_SHA=HEAD ./scripts/core-change-detect.sh` → exit 0（覆盖检查通过）；`./scripts/core-change-detect.sh --self-test` → `✓ 自检通过：merge-base 口径不误算 base 侧改动，PR 侧核心改动能检出，未登记平台 crate 能检出` **exit 0**（自检里额外构造了「临时仓库建一个未登记的 `crates/other` → 必须检出」与「全部 crate 已登记 → 必须放行」两个场景） |
| 4 | `migrations/` 只有 README 就能过「非空」校验 | **规则暂不改**（本期无业务 DB 层）；提示写进 `modules/sandbox/migrations/README.md`（「规则何时升级」段）与 `crates/xtask/src/modules.rs` 的空目录提示（明确「DB 层落地后规则将升级为至少一个 `*.sql`」） | `cargo xtask check-modules` → 仍通过；新增单测 `xtask::tests::empty_migrations_dir_explains_the_future_rule` 断言提示语含 `*.sql` 与「DB 层落地后」 |

---

## 4.1 本轮修复轮：QA 退回的 5 项

上一轮 Dev 在**修复轮**中途被断电重启杀死（进程 `lost`），但改动留在工作树里未提交。
本轮**接管 → 复核 → 补漏 → 验证 → 提交**，没有从头重写，也**没有回退**那些改动。

| # | QA 登记的问题 | 本轮复核结论 | 落地位置 |
|---|--------------|--------------|---------|
| **中 1** | 健康检查 3 条错误路径有 2 条把中文塞进 `detail`，违反契约 C6；且上一轮报告写「符合 C6」属**不实结论** | **已修完并复验**：三条路径的 `detail` 现在全是英文，`detail` 统一过 `dev_detail()`（只保留可打印 ASCII —— 数据库服务端消息会随 `lc_messages` 本地化，不滤就有可能漏中文）；5 条单测守着 | `crates/platform-server/src/health.rs`（`dev_detail` / `detail_*` / `migration_error`） |
| **低 1** | RUNBOOK 称 Helm 默认渲染含 Ingress（实为 `ingress.enabled` 默认 false） | **已修完并复验**：RUNBOOK §4 改成「默认 5 类，不含 Ingress」并给出 `--set ingress.enabled=true`；本轮 `helm template` 实测确认默认 5 类 / 加开关 6 类。**上一轮报告与追溯矩阵里的「六类资源」表述本轮一并更正** | `deploy/RUNBOOK.md` §4 |
| **低 2** | readinessProbe 超时 5s 与连接池 acquire_timeout 5s 相撞，探针先超时拿不到 503 | **已修完并复验**：连接池 `acquire_timeout` 降到 **2s**，探针保持 5s；本轮实测 ready 在 **2.002s** 返回 503（修复前是 5.003s），探针有 3s 余量。1 条单测守着这个关系 | `crates/platform-server/src/db.rs`（`ACQUIRE_TIMEOUT`） |
| **低 3** | `core-change-detect.sh --self-test` 没覆盖「平台 crate 必须登记」 | **已修完并复验**：`check_core_coverage` 已并进 `self_test()`，自检里构造「未登记 crate 必须检出」与「全部已登记必须放行」两个场景；本轮实跑 exit 0 | `scripts/core-change-detect.sh` |
| **低 4** | prod compose 默认弱口令 `base` + 第三方镜像源 | **已修完并复验**：`${POSTGRES_PASSWORD:?...}`（缺失/空值 → compose 解析阶段报错退出 1，本轮实测）；基础镜像默认值改为**官方地址**。另补 `scripts/prod-preflight.sh` 拦弱口令与库名不一致 | `deploy/compose.prod.yaml`、`.env.example`、`scripts/prod-preflight.sh` |

**本轮补的漏**（上一轮改动里没做完或做错的）：

| # | 漏 | 处置 |
|---|----|------|
| 1 | RUNBOOK §3.1 与 compose 头部的 `./scripts/prod-preflight.sh` **路径不存在**（该段已经 `cd deploy`） | 改为 `../scripts/prod-preflight.sh`；compose 头部的用法示例统一成「在 `deploy/` 目录里执行」（原示例 `cp .env.example .env` 会把 `.env` 放错目录，compose 从 compose 文件所在目录读 `.env`） |
| 2 | 验证报告与追溯矩阵里的「`detail` 为英文 / 符合 C6」与「Helm 六类资源」两处**不实结论** | 本轮按实测更正（§3.2 / §3.5 / `traceability.md`） |
| 3 | compose 头部的 `env_file` 相对路径在 `podman-compose` 与 `docker compose` 下解析基准不同 | 不改语义，改为在 RUNBOOK 里统一「在 `deploy/` 目录里执行」——两种 compose 实现此时解析一致 |

---

## 5. 回归

| 套件 | 命令 | 结果 |
|------|------|------|
| Rust 单测 + 契约测试 | `cargo test --workspace -- --test-threads=2` | **138/138**（platform-core 41 / platform-contracts 8 / platform-server 32+5 / sandbox 2+4 / xtask 46；上一轮 132 个全绿，本轮 **+6** 只增不减） |
| Rust lint | `cargo clippy --workspace --all-targets -- -D warnings` | 0 告警 |
| Rust 格式 | `cargo fmt --all --check` | 通过 |
| 静态检查 | `cargo check --workspace --all-targets` | 通过 |
| 前端单测 | `pnpm -r test` | **23/23** |
| 前端构建 | `pnpm -r build` | 通过（产出 `apps/shell/dist`，56K） |
| E2E | `bun run test:e2e` | **1/1** |
| 前端 lint | `pnpm exec eslint .` / `pnpm exec prettier --check .` | 通过 |
| 能力索引一致性 | `cargo xtask capabilities --check` | 通过（13 个文件与源码一致） |
| 依赖方向 | `cargo xtask check-deps` | 通过 |
| 模块目录规范 | `cargo xtask check-modules` | 通过 |
| 契约快照 | `./scripts/sync-contracts.sh` | 通过（两份快照与 demand 分支正本一致） |

---

## 6. 未验证 / 偏离（如实登记）

### 6.1 未验证

| 项 | 原因 | 谁来补 |
|----|------|--------|
| **镜像实构建**（体积 <100MB、容器内启动、探针在容器里的行为） | 本机 4.2G 可用内存，`cargo build --release` + LTO 链接峰值远超余量（任务约束与设计摘要 §8 明确要求本机不构建） | CI `build-image` job（含体积硬校验 + 容器内迁移 + 健康检查冒烟）。**Actions 未实跑**（本仓库无 remote） |
| **GitHub Actions 实跑** | `git remote -v` 为空（local 模式） | 有 remote 后首次跑 CI |
| **真实 K8s 集群部署** | 本机无集群 | `helm install` 到测试集群（Chart 已经过 `helm template` 渲染校验） |
| **Trivy 扫描结果** | 需要联网拉扫描器与漏洞库 | CI `image-scan` job |
| **数据库 TLS** | `sqlx` 未开 TLS 特性（`tls-none`）：带 `sslmode=require` 的连接串会失败 | 随 REQ-006/007 的 DB 层一并处理；当前私有化部署走内网/本地 socket |

### 6.2 偏离

| # | 偏离 | 理由 |
|---|------|------|
| 1 | **新增 crate `crates/platform-server`**，而不是把服务代码放进 `platform-core` | 任务约束明确「不要修改 `crates/platform-core/` 与 `crates/platform-contracts/`」；且 `git diff 31ca87a..HEAD -- crates/platform-core crates/platform-contracts crates/platform-contracts-macros` **为空**。代价：`crates/platform-core/src/bin/platform-core.rs` 的示例服务与新服务并存（前者留作本地开发示例，容器里跑的是 `base`） |
| 2 | **新增 `BASE_ENV` 环境变量**（设计摘要 §3.2 清单之外） | AC-S-005 要求「同一镜像切分 dev/staging/prod」，必须有一个显式的环境标识；取值非法同样 fail-fast。已在 RUNBOOK §1.1 标注为「设计清单之外补的变量」 |
| 3 | **引入 `sqlx` 依赖**（AGENTS.md 原本记「sqlx 推迟到 REQ-006/007」） | 设计摘要 §4.2 明确要求「迁移工具用 sqlx migrate」，且 readiness 必须查数据库连通与迁移状态。本期只引入**部署期** DB 客户端（健康检查 + 迁移），**不引入业务数据访问层** —— AGENTS.md 的 `db_access_layer` 仍如实标为「尚未提供」，能力目录与 `AGENTS.md` 一字未改 |
| 4 | 迁移文件放在**仓库根 `migrations/`**（而不是 `crates/platform-server/migrations/`） | 平台级 schema 与业务模块 schema 分离（业务模块的迁移在 `modules/<name>/migrations/`）；镜像内路径 `/app/migrations` |
| 5 | `core-change-detect.sh` 把 `crates/platform-server` 与 `crates/xtask` **显式豁免**出核心路径清单 | 核心门禁的口径是「改动是否影响所有业务模块」：这两个 crate 业务模块不依赖。两者仍进 `.github/CODEOWNERS`（有人工评审），但不会触发 `core-change-review` 的强制批准检查 |
| 6 | `feature-meta/manifest.json` 补了 `contracts` / `adrs` / `delivered_by` | BA 建的 REQ-005 manifest 缺 `contracts` pin，导致 `sync-contracts.sh` 的 jq 读取失败、契约校验无法进行。pin 内容取自契约正本的 consumers 列表（REQ-005 同时消费 `api-conventions v1` 与 `extension-points v1`） |
| 7 | `restore.sh` 恢复到 `DATABASE_URL` 指向的库时**要求 `--yes`** | 该操作会删库重建，无确认容易误伤生产库；演练建议指定临时库名 |

---

## 7. 交付清单核对（设计摘要 §7）

| 项 | 状态 | 位置 |
|---|------|------|
| 多阶段 Dockerfile | ✅ | `Dockerfile`、`.dockerignore` |
| `compose.yaml`（开发）+ `deploy/compose.prod.yaml`（私有化） | ✅ | 两个文件都改（开发 compose 新增 `migrate` 服务） |
| `deploy/helm/base-platform/` | ✅ | Chart + values + 8 个模板 |
| 配置外置 + 启动时校验 + `.env.example` | ✅ | `crates/platform-server/src/config.rs`、`.env.example` |
| 迁移作为独立步骤 | ✅ | compose profile + Helm hook + `base migrate` 子命令 |
| `scripts/backup.sh` / `restore.sh` / `rollback.sh` | ✅ | 另加 `backup-verify.sh` 与共用库 `scripts/lib/pg.sh` |
| `deploy/RUNBOOK.md` + 恢复演练记录 | ✅ | 演练记录为真实执行结果 |
| 健康检查端点 | ✅ | `/health/live`、`/health/ready` |
| CI 构建镜像与扫描 job | ✅ | `build-image` / `image-scan`（另加 `migration-test`） |
| 金丝雀发布 | ❌ 不做 | 设计摘要明确「本期不做」，README/RUNBOOK 未提及接入方式（待 Argo Rollouts / Flagger 接入时再写） |
| 配置中心接入 | ❌ 不做 | 待 REQ-009 / REQ-010 |
| 蓝绿自动化 | ❌ 不做 | 本期只提供手工切换流程（RUNBOOK §3.2） |
