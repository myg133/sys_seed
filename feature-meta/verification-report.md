# REQ-005 验证报告

- **需求**: REQ-005 交付基线与发布
- **分支**: `feature/REQ-005`
- **基线**: `develop` @ `37a93c5`（REQ-002 工程基线 + REQ-003 扩展点框架 + REQ-004 AI 引导体系已合并）
- **设计依据**: `BA/demands/REQ-005-delivery/design-summary.md`
- **契约**: api-conventions v1、extension-points v1（已 ratified，已在 `feature-meta/manifest.json` pin）
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: 第一轮（开发完成，待 QA 验证）

---

## 0. 摘要

| 项 | 结果 |
|----|------|
| 改动 | **48 文件 / +5111 -574 行**（基线 `31ca87a` → 本轮；删除行主要来自 REQ-004 的 `feature-meta/*.md` 被本轮文档替换） |
| 新增第三方依赖 | **`sqlx` 0.8**（`runtime-tokio` + `postgres` + `migrate`，**关闭** `macros` / `json` / TLS 特性）；锁文件 107 → 234 个 crate，其中 `sqlx-macros` / `sqlx-mysql` / `sqlx-sqlite` 只进锁文件**不参与编译**（`cargo tree` 实测 0 处引用） |
| 新增 crate | `crates/platform-server`（交付基线服务：`base` 二进制）——**未改动** `platform-core` / `platform-contracts` |
| Rust 测试 | **132/132 通过**（原有 99 个全绿，只增不减；新增 33 个：`platform-server` 31 + `xtask` 2） |
| 前端测试 | **23/23**（`pnpm -r test`） |
| E2E | **1/1**（Playwright chromium） |
| lint | `cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` ✅ 0 告警 / `pnpm exec eslint .` ✅ / `pnpm exec prettier --check .` ✅ |
| compose 校验 | `podman-compose -f compose.yaml config` ✅ / `podman-compose -f deploy/compose.prod.yaml config` ✅；**`up -d` 不会自动跑迁移**（profile 验证通过） |
| Helm | `helm lint` ✅ / `helm template` ✅ 渲染出 Deployment / Service / Ingress / ConfigMap / Secret 引用 / 迁移 Job |
| 健康检查 | live **200** / ready **200**（正常）；停库后 ready **503**（`DATABASE_UNAVAILABLE`）/ live 仍 **200** |
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

### 3.2 健康检查

```console
# 正常
$ curl -s -w ' HTTP %{http_code}\n' http://127.0.0.1:18080/health/live
{"status":"alive","traceId":"d10b7ef9-...","version":"0.1.0"} HTTP 200
$ curl -s -w ' HTTP %{http_code}\n' http://127.0.0.1:18080/health/ready
{"appliedMigrations":[20261118000100],"environment":"dev","status":"ready",...} HTTP 200

# 停掉数据库容器后
$ podman stop base-postgres
$ curl -s -w ' HTTP %{http_code}\n' http://127.0.0.1:18080/health/live
{"status":"alive",...} HTTP 200                                  ← 进程还活着
$ curl -s -w ' HTTP %{http_code}\n' http://127.0.0.1:18080/health/ready
{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,
 "detail":"database is unreachable: pool timed out while waiting for an open connection",
 "instance":"/health/ready","code":"DATABASE_UNAVAILABLE","trace_id":"4ec5f830-..."} HTTP 503

$ podman start base-postgres   # 恢复后 ready 回到 200
```

错误体符合契约 C6（RFC 9457 + 稳定 `code` + `trace_id`，`detail` 为英文）。

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

### 3.5 compose 与 Helm

```console
$ podman-compose -f compose.yaml config                       # exit 0
$ podman-compose -f deploy/compose.prod.yaml config           # exit 0
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
kind: ServiceAccount / kind: ConfigMap / kind: Service / kind: Deployment / kind: Ingress / kind: Job
```

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
| 2 | `core-change-review` 用 `pull_request.base.sha`（不随 base 前进，偏严） | **已修正**为 merge-base 口径；判定抽到共享脚本 `scripts/core-change-detect.sh`，打标与强制评审两个 job 同源 | `./scripts/core-change-detect.sh --self-test` → `✓ 自检通过：merge-base 口径不误算 base 侧改动，PR 侧核心改动能检出`（自检里构造了「base 侧改核心 + PR 侧只改非核心」与「PR 侧改核心」两个场景） |
| 3 | 核心 diff 判定只看路径 | 清单确认覆盖 `platform-core` / `platform-contracts` / `platform-contracts-macros`；**加自检**：`crates/` 下每个平台 crate 都必须在清单里，忘了登记直接失败（`platform-server` / `xtask` 显式豁免并写明理由） | `BASE_REF=develop HEAD_SHA=HEAD ./scripts/core-change-detect.sh` → exit 0（覆盖检查通过） |
| 4 | `migrations/` 只有 README 就能过「非空」校验 | **规则暂不改**（本期无业务 DB 层）；提示写进 `modules/sandbox/migrations/README.md`（「规则何时升级」段）与 `crates/xtask/src/modules.rs` 的空目录提示（明确「DB 层落地后规则将升级为至少一个 `*.sql`」） | `cargo xtask check-modules` → 仍通过；新增单测 `xtask::tests::empty_migrations_dir_explains_the_future_rule` 断言提示语含 `*.sql` 与「DB 层落地后」 |

---

## 5. 回归

| 套件 | 命令 | 结果 |
|------|------|------|
| Rust 单测 + 契约测试 | `cargo test --workspace -- --test-threads=2` | **132/132**（platform-core 41 / platform-contracts 8 / platform-server 26+5 / sandbox 2+4 / xtask 46；原有 99 全绿） |
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
