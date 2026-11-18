# 交付基线运维手册（RUNBOOK）

- **适用**: REQ-005 交付基线 —— 镜像、compose 私有化部署、Helm 部署、迁移、备份恢复
- **基线**: `develop` @ `37a93c5`（REQ-002 / 003 / 004 已合并）
- **契约**: `api-conventions v1`（探针与错误码）、`extension-points v1`
- **更新时间**: 2026-11-18

---

## 0. 三种落地方式一览

| 形态 | 编排 | 迁移怎么跑 | 入口文档 |
|---|---|---|---|
| 开发（本机） | `compose.yaml`（rootless podman） | `cargo run -p platform-server --bin base -- migrate up` | `docs/dev-environment.md` |
| 私有化部署（企业内网单机） | `deploy/compose.prod.yaml` | `docker compose --profile migrate run --rm migrate` | 本文件 §3 |
| 云上（K8s） | `deploy/helm/base-platform` | Helm hook Job（`pre-install,pre-upgrade`） | 本文件 §4 |

**同一套镜像、同一套配置**，靠环境变量切分 dev / staging / prod，不重新构建。

---

## 1. 配置外置（AC-S-005）

### 1.1 环境变量清单

| 变量 | 必填 | 默认值 | 说明 |
|---|---|---|---|
| `DATABASE_URL` | ✅ | — | 缺失则**启动即失败**（退出码 2，错误码 `CONFIG_MISSING_REQUIRED`） |
| `REDIS_URL` | | — | 不配则不使用缓存 |
| `HOST` | | `0.0.0.0` | 监听地址 |
| `SERVER_PORT` | | `8080` | 监听端口 |
| `BASE_ENV` | | `dev` | `dev` / `staging` / `prod` |
| `BASE_TENANT_MODE` | | `shared` | `shared` / `schema` / `database` |
| `BASE_LOG_LEVEL` | | `info` | `error` / `warn` / `info` / `debug` / `trace` |
| `BASE_AUTH_MODE` | | `local` | `local` / `sso` / `hybrid` |
| `BASE_SEED_ON_START` | | `false` | `BASE_ENV=prod` 时配 `true` 会**启动即失败**（`CONFIG_FORBIDDEN_COMBINATION`） |
| `BASE_MIGRATIONS_DIR` | | `./migrations` | 镜像内 `/app/migrations` |
| `BASE_STATIC_DIR` | | — | 前端静态目录；镜像内 `/app/static` |

> `BASE_ENV` 是设计摘要 §3.2 清单之外补的一个变量：AC-S-005 要求「同一镜像切分
> dev/staging/prod」，必须有一个显式的环境标识；取值非法同样 fail-fast。

### 1.2 失败路径样例

```console
$ unset DATABASE_URL && base serve
✗ 配置校验失败，进程退出（fail-fast）：
配置校验失败（1 项）
  - [CONFIG_MISSING_REQUIRED] DATABASE_URL：必填环境变量缺失或为空；数据库连接串必须显式提供，不使用默认值；示例：postgres://user:pass@host:5432/base
提示：必填项见 deploy/RUNBOOK.md 与 .env.example；生产用 K8s Secret / compose .env 注入。
$ echo $?
2
```

### 1.3 Secret 处理

- `.env.example` **入库**，`.env` **不入库**（`.gitignore` 已覆盖）。
- Secret 绝不进镜像（`.dockerignore` 已排除 `.env*`）。
- 生产：compose 用 `.env`（不入库）或 Docker secrets；K8s 用 Secret 引用（Chart **不生成** Secret）。

```bash
# K8s Secret 创建（Chart 只引用它）
kubectl create secret generic base-platform-secret \
  --from-literal=DATABASE_URL='postgres://base:***@postgres:5432/base' \
  --from-literal=REDIS_URL='redis://redis:6379'
```

---

## 2. 镜像

### 2.1 构建

```bash
# CI（境外，官方镜像地址）
docker build -t base-platform:0.1.0 .

# 国内构建（基础镜像换成可达的镜像源）
docker build -t base-platform:0.1.0 \
  --build-arg RUST_IMAGE=docker.m.daocloud.io/library/rust:lld \
  --build-arg NODE_IMAGE=docker.m.daocloud.io/library/node:22-bookworm-slim \
  --build-arg RUNTIME_IMAGE=docker.m.daocloud.io/library/debian:12-slim \
  --build-arg NPM_REGISTRY=https://registry.npmmirror.com/ .
```

> ⚠️ **本机（低配）不构建镜像**：release + LTO 的链接内存峰值远超本机余量，
> 镜像构建交给 CI（`.github/workflows/ci.yml` 的 `build-image` job）。

### 2.2 阶段与体积

| 阶段 | 基础镜像 | 产出 |
|---|---|---|
| `builder` | `rust:lld`（自带 mold/lld，不用官方 `rust` 镜像） | `/app/target/release/base` |
| `frontend` | `node:22-bookworm-slim` + pnpm | `apps/shell/dist` → `/app/static` |
| `runtime` | `debian:12-slim` | 二进制 + 静态文件 + 迁移脚本 |

体积目标 **< 100MB**（不含前端静态文件）；CI 里 `build-image` job 有硬校验（超过 100MB 直接失败）。
runtime 阶段非 root 运行（uid 10001），只装 `ca-certificates`，时区固定 UTC（契约 C13）。

### 2.3 依赖层缓存

`builder` 阶段用 `cargo-chef`：`cargo chef prepare` 生成 recipe → `cargo chef cook` 先把依赖编进缓存层
→ 最后才编业务代码。源码改动不会重编依赖。

---

## 3. 私有化部署（compose）

### 3.1 首次部署

> 以下命令**在 `deploy/` 目录里执行**（`.env` 与 `compose.prod.yaml` 同目录 ——
> `docker compose` 与 `podman-compose` 都从**compose 文件所在目录**读 `.env`）。
>
> §3.2 / §3.3 的示例从**仓库根目录**执行（用 `-f deploy/compose.prod.yaml`）。
> 两种 compose 实现解析服务级 `env_file` 的基准不同：`docker compose` 按
> **compose 文件所在目录**解析（哪里执行都行），`podman-compose` 按**当前目录**解析 ——
> 用 `podman-compose` 时请在 `deploy/` 目录里执行，或显式指定
> `ENV_FILE=deploy/.env`。

```bash
cd deploy
cp ../.env.example .env      # 编辑 .env：改 DATABASE_URL、BASE_ENV=prod

# ⚠️ 生产口令**没有默认值**：POSTGRES_PASSWORD 必须显式注入，
#    缺失时 compose 直接报错退出（弱口令另由 preflight 拒绝）。
#    生成：openssl rand -base64 24
#    .env 里至少要有：
#      POSTGRES_PASSWORD=<随机口令>
#      POSTGRES_DB=base
#      DATABASE_URL=postgres://base:<同一个口令>@postgres:5432/base   ← 库名必须与 POSTGRES_DB 一致

../scripts/prod-preflight.sh   # 口令 / 库名 / 连接串前置校验，不通过就不许 up

docker compose -f compose.prod.yaml up -d
docker compose -f compose.prod.yaml --profile migrate run --rm migrate
docker compose -f compose.prod.yaml ps
curl -fsS http://127.0.0.1:8080/health/ready
```

> **交付给客户**：`compose.prod.yaml` + `.env` + `scripts/prod-preflight.sh`
> 放在同一套目录里（脚本放同级 `scripts/` 子目录），命令改成 `./scripts/prod-preflight.sh`。

> 迁移是**独立步骤**：`up -d` **不会**自动跑迁移（`migrate` 服务带 `profiles: ["migrate"]`）。
> 必须先迁移、再滚 app —— 多副本并发迁移会冲突。

**私有化 compose 的三个要点**：

| 项 | 规则 |
|---|---|
| `POSTGRES_PASSWORD` | **无默认值**。`${POSTGRES_PASSWORD:?...}` —— 缺失或为空时 compose 解析阶段就报错退出，不会静默用弱口令建库；弱口令（`base` / `postgres` / `password` …）由 `scripts/prod-preflight.sh` 拒绝 |
| `POSTGRES_IMAGE` / `REDIS_IMAGE` | 默认**官方地址**（`docker.io/library/postgres:17-alpine` / `docker.io/library/redis:7-alpine`）。客户内网请指向自己的仓库：`POSTGRES_IMAGE=registry.internal/library/postgres:17-alpine`。国内加速是**开发环境**的事（仓库根 `compose.yaml` 才用 daocloud 前缀） |
| 库名 | `POSTGRES_DB`（默认 `base`）必须与 `DATABASE_URL` 里的库名一致，否则应用连到不存在的库 —— `prod-preflight.sh` 会比对 |

### 3.2 发布流程（备份 → 迁移 → 滚动 → 健康检查 → 回滚）

```bash
# 1. 备份（自动轮转，只留最近 5 份）
DATABASE_URL='postgres://base:***@127.0.0.1:5432/base' ./scripts/backup.sh

# 2. 拉新镜像
docker compose -f deploy/compose.prod.yaml pull

# 3. 迁移（失败就停在这里，不要滚 app）
docker compose -f deploy/compose.prod.yaml --profile migrate run --rm migrate

# 4. 滚动重启 app
docker compose -f deploy/compose.prod.yaml up -d --no-deps app

# 5. 健康检查（就绪探针：数据库连通 + 迁移已应用）
curl -fsS http://127.0.0.1:8080/health/ready

# 6. 失败 → 回滚（迁移回滚 + 应用镜像回退）
./scripts/rollback.sh 0                       # 迁移回滚到版本 0
TAG=0.1.0 docker compose -f deploy/compose.prod.yaml up -d --no-deps app
```

### 3.3 回滚迁移

```bash
# 看当前状态（不改动）
./scripts/rollback.sh 0 --dry-run

# 回滚到版本 0：先自动备份，再执行 down 迁移
./scripts/rollback.sh 0

# 私有化部署（在容器里跑迁移）
MIGRATE_CMD="docker compose -f deploy/compose.prod.yaml --profile migrate run --rm migrate" \
  ./scripts/rollback.sh 0
```

不可逆迁移（没有配套 `down.sql`）会被**拒绝回滚**，脚本明确报错，不会做半截数据破坏。

---

## 4. 云上部署（K8s / Helm）

```bash
# 1. Secret（Chart 只引用，不生成）
kubectl create secret generic base-platform-secret \
  --from-literal=DATABASE_URL='postgres://base:***@postgres:5432/base' \
  --from-literal=REDIS_URL='redis://redis:6379'

# 2. 安装（迁移 Job 作为 pre-install / pre-upgrade hook 自动跑）
helm upgrade --install base-platform deploy/helm/base-platform \
  --namespace base-platform --create-namespace \
  --set image.repository=registry.internal/base-platform \
  --set image.tag=0.1.0 \
  --set ingress.enabled=true \
  --set ingress.hosts[0].host=base.example.internal

# 3. 看结果
kubectl rollout status deployment/base-platform -n base-platform
kubectl get job base-platform-migrate -n base-platform

# 4. 回滚（Helm 自带）
helm rollback base-platform <revision> -n base-platform
```

模板渲染自检（本机无 helm 时用）：

```bash
helm template base-platform deploy/helm/base-platform | grep -E '^kind:'
# 默认渲染 5 类：ServiceAccount / ConfigMap / Service / Deployment / Job
# **不含 Ingress** —— values.yaml 里 ingress.enabled 默认 false

helm template base-platform deploy/helm/base-platform --set ingress.enabled=true | grep -E '^kind:'
# 加 --set ingress.enabled=true 后才有 6 类：上面 5 类 + Ingress
```

> `ingress.enabled` 默认 **false**（Chart 默认不接管入口，暴露方式由集群决定）。
> 需要 Ingress 时显式打开：helm install 时 `--set ingress.enabled=true --set ingress.hosts[0].host=...`（见本节第 2 步）。

---

## 5. 健康检查

| 探针 | 路径 | 语义 | 失败含义 |
|---|---|---|---|
| liveness | `/health/live` | 进程活着 | 进程挂了 → 重启 |
| readiness | `/health/ready` | **数据库连通 + 迁移已应用** | 不接流量（滚动更新时不会打到没迁移好的实例） |

```bash
curl -s http://127.0.0.1:8080/health/live
# {"status":"alive","traceId":"...","version":"0.1.0"}

curl -s http://127.0.0.1:8080/health/ready
# 正常：{"appliedMigrations":[20261118000100],"environment":"dev","status":"ready",...}
# 异常：HTTP 503 + {"code":"DATABASE_UNAVAILABLE"|"MIGRATIONS_PENDING"|"MIGRATION_DIRTY",...}
```

镜像自带健康检查：`docker exec <容器> base healthcheck`（就绪探针）。

### 5.1 错误码与 `detail` 的语言（契约 C6）

`detail` 是**面向开发者的英文描述**；**面向用户的中文文案由前端按 `code` 做 i18n 渲染**
（契约 `api-conventions v1` C6：后端不得在 `detail` 里拼中文句子）。
后端只保证 `code` 稳定、`detail` 是 ASCII 英文。

| `code` | 触发条件 | `detail`（英文） |
|---|---|---|
| `DATABASE_UNAVAILABLE` | 数据库连不上 | `database is unreachable: <原因>` |
| `MIGRATIONS_PENDING` | 连上了但迁移没跑 / 迁移表读不出来 | `pending migrations: [...]` / `migration state is unreadable, ...` |
| `MIGRATION_DIRTY` | 迁移执行到一半失败，需人工处理 | `migration <版本> is in a dirty state and needs manual recovery` |

运维视角的中文说明在**日志**里（`tracing::warn!` 记 `code` + `detail` + `pending` + `dirty`），
不进响应体。

### 5.2 探针超时与连接池超时（改一个必须同时改另一个）

| 谁 | 值 | 位置 |
|---|---|---|
| 连接池 `acquire_timeout` | **2s** | `crates/platform-server/src/db.rs`（`ACQUIRE_TIMEOUT`） |
| K8s `readinessProbe.timeoutSeconds` | **5s** | `deploy/helm/base-platform/templates/deployment.yaml` |
| 镜像 `HEALTHCHECK --timeout` | **5s** | `Dockerfile` |

数据库不可达时，`/health/ready` 要等满 `acquire_timeout` 才返回 **503**（实测约 2s）。
**探针超时必须大于 `acquire_timeout`**，否则 kubelet / docker 会先掐断请求，
那个语义清晰的 `503 / DATABASE_UNAVAILABLE` 就拿不到（只会看到探针超时）。
`crates/platform-server/src/db.rs` 有一条单测守着这个关系。

---

## 6. 备份与恢复

### 6.1 完整命令

```bash
# 备份（pg_dump + gzip → backup/base-<时间戳>.sql.gz，只留最近 5 份）
DATABASE_URL='postgres://base:***@127.0.0.1:5432/base' ./scripts/backup.sh

# 恢复到指定库（演练用；目标库会被删掉重建）
./scripts/restore.sh backup/base-20261118-093935.sql.gz base_restore_drill

# 恢复到 DATABASE_URL 指向的库（**危险**：必须显式 --yes）
./scripts/restore.sh backup/base-20261118-093935.sql.gz --yes

# 恢复演练：恢复到临时库 + 冒烟 + 自动清理
./scripts/backup-verify.sh
```

环境变量：`BACKUP_DIR`（默认 `backup`）、`BACKUP_KEEP`（默认 5）、`PG_CONTAINER`（默认 `base-postgres`）。

> 本机没装 psql 客户端，脚本会自动改用**容器内执行**（`podman exec` / `docker exec`），
> 走容器内的本地 socket 连接 —— **口令不落到命令行、也不落到进程列表**。

### 6.2 演练记录（真实执行结果）

| 项 | 值 |
|---|---|
| 日期 | 2026-11-18 |
| 执行人 | myg133-dev-1（Dev Agent，REQ-005） |
| 环境 | 本机低配机：i7-8550U / 7.7G 内存，PostgreSQL 17.12 容器（`base-postgres`） |
| 备份耗时 | **307 ms**（`base_dev`，4 张表，产物 1.4K gzip） |
| 恢复耗时 | **704 ms**（恢复到临时库 `base_runbook_drill`） |
| 恢复校验 | 通过 —— `runbook_probe` 表与数据 `1|runbook-drill` 均在临时库中，临时库共 4 张表 |
| 备份校验演练 | **1509 ms**（`backup-verify.sh`：导入 + 3 项冒烟全过，自动清理临时库，退出码 0） |
| 迁移回滚 | 通过 —— `base_migrate_drill` 上 `migrate up` → `migrate down --target 0` → 业务表清空（只剩 sqlx 自身的 `_sqlx_migrations` 记账表），再 `up` 可重复执行 |
| 清理 | 演练产生的临时库（`base_runbook_drill` / `base_verify_*` / `base_migrate_drill`）与测试表（`runbook_probe` / `restore_drill_probe`）**已全部删除**；`base_dev` / `base_test` 完好 |

#### 演练中发现并修掉的问题

| # | 问题 | 处置 |
|---|---|---|
| 1 | `backup-verify.sh` 第一版**没有把备份导入临时库**就做冒烟检查 → 必然误报「备份里没有迁移表」 | 补上导入步骤，冒烟才有意义（已修，见脚本第 55 行附近） |
| 2 | 数据库容器因绑定挂载指向已回收的 REQ-002 worktree，`podman start` 起不来 | 用本 worktree 的 `compose.yaml` 重建容器（数据在 volume `base-platform_pgdata` 中，未丢失） |
| 3 | `pg_database_exists` 连的是目标库，目标库不存在时报连接错误（噪音） | 改为主在维护库 `postgres` 上查（已修） |

### 6.3 磁盘控制

| 项 | 控制 |
|---|---|
| 备份文件 | `backup/` 保留最近 5 份，`scripts/backup.sh` 自动轮转 |
| 镜像 | 本机只保留 `base-platform:dev` 一个 tag，`podman image prune` 清理 |
| 定期 | `podman system prune -af` + `df -h /home` |

---

## 7. 故障排查

### 7.1 迁移失败

```bash
# 看当前状态
DATABASE_URL='postgres://base:***@127.0.0.1:5432/base' cargo run -p platform-server --bin base -- migrate status
```

| 现象 | 原因 | 处置 |
|---|---|---|
| 退出码 1，报 `迁移状态不可读` | 迁移表不存在 / 连不上库 | 检查 `DATABASE_URL`、容器健康度 |
| 报 `目标版本 N 之上存在不可逆迁移` | 该迁移没有 `down.sql` | 不可逆迁移按「加列 → 双写 → 切读 → 删列」四步拆版本发布，别硬回滚 |
| 报 `数据库访问失败：...` | 连接失败 | `podman exec base-postgres psql -U base -d base -c 'select 1'` 验证 |
| `dirty` 状态（`MIGRATION_DIRTY`） | 迁移执行到一半失败 | 手工修完后 `UPDATE _sqlx_migrations SET success = true WHERE version = <版本>`，再重跑 |

### 7.2 容器起不来

```bash
podman ps -a                      # 看退出码与状态
podman logs base-app              # 看应用日志
docker compose -f deploy/compose.prod.yaml logs app
```

| 现象 | 原因 | 处置 |
|---|---|---|
| 退出码 2，日志有 `CONFIG_MISSING_REQUIRED` | 缺 `DATABASE_URL` | 补配置（compose 检查 `env_file` 路径、K8s 检查 Secret 里的键名） |
| 退出码 2，`CONFIG_INVALID_VALUE` | 环境变量取值非法 | 按 §1.1 的取值范围改 |
| `监听 0.0.0.0:8080 失败` | 端口被占 | `ss -ltnp | grep 8080` 找占用进程 |
| 镜像拉不到 | 国内直连 docker.io 超时 | 用 `--build-arg`/环境变量换镜像源（见 §2.1） |

### 7.3 数据库连不上

| 现象 | 原因 | 处置 |
|---|---|---|
| readiness 503 + `DATABASE_UNAVAILABLE` | 数据库不可达 | `podman exec base-postgres pg_isready -U base -d base`；容器起没起、端口对不对 |
| readiness 503 + `MIGRATIONS_PENDING` | 连上了，但迁移没跑 | 跑迁移：`base migrate up` |
| 连得上但认证失败 | 口令错 / `pg_hba` 拒绝 | 核对 `DATABASE_URL` 里的用户口令；容器内本地 socket 连接不需要口令 |
| `pool timed out while waiting for an open connection` | 连接池打满 / 数据库卡住 | 看数据库负载；`max_connections` 是否太小 |
| ready 要 **2 秒**左右才返回 503 | 数据库不可达时要等满 `acquire_timeout`（2s） | 正常现象；探针超时（当前 5s）必须大于它，见 §5.2 |

### 7.4 磁盘满

```bash
df -h /home
podman system df
du -sh backup/ ~/.cargo/registry target/ 2>/dev/null
```

| 现象 | 原因 | 处置 |
|---|---|---|
| `backup/` 堆积 | 备份脚本没跑或保留数被调大 | 跑 `scripts/backup.sh`（自动轮转到 5 份）；`BACKUP_KEEP=3` 收紧 |
| podman 存储膨胀 | 旧镜像 / 悬空容器 / 构建缓存 | `podman system prune -af`（**会删掉未使用的镜像与缓存**） |
| `target/` 过大 | Rust 构建产物 | `cargo clean`（保留 `~/.cargo` 缓存，重编译慢一次但省磁盘） |
| PG 报 `no space left on device` | 数据目录所在盘满 | 先清备份与镜像；必要时 `VACUUM FULL` |

---

## 8. 迁移文件约定

- 目录：仓库根 `migrations/`
- 命名：`<版本>_<描述>.up.sql` + `<版本>_<描述>.down.sql`（sqlx 可逆迁移格式）
- **每个迁移必须有配套 `down`**；确实不可逆的，在 PR 里标注 `⚠️ 不可逆`，并按
  「加新列 → 双写 → 切读 → 删旧列」四步拆成多个版本发布
- 迁移在**部署流程**里执行，不在应用启动流程里（`base serve` 不跑迁移）
