# 交付基线运维手册（RUNBOOK）

- **适用**: REQ-005 交付基线 —— 镜像、compose 私有化部署、Helm 部署、迁移、备份恢复；REQ-006 多租户 —— 应用角色与 RLS、租户作用域策略
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

### 0.1 发布链路的四个步骤（顺序不能换）

| # | 步骤 | 做什么 | 用什么连接 | 漏了会怎样 |
|---|---|---|---|---|
| 1 | **应用角色** | 建普通角色（`base_app`）+ 组角色 `base_platform_app` | 超级用户 | 应用**连不上**（角色不存在） |
| 2 | **迁移** | 建表（`base migrate up`） | 超级用户（要 DDL） | 表不存在 → 策略步骤失败 |
| 3 | **策略** | 按 manifest 生成 RLS policy（`base tenant-policies`） | 超级用户（要表属主权限） | 租户作用域表**完全没有隔离** |
| 4 | **应用** | 起服务 | **普通角色**（`base_app`） | 用超级用户起 → **直接拒绝启动**（退出码 3） |

> 步骤 2 与 3 的顺序是硬约束：策略是对**已建好的表**做的，先建表再建策略。
> 详见 §3.4「应用角色与 RLS」。

---

## 1. 配置外置（AC-S-005）

### 1.1 环境变量清单

| 变量 | 必填 | 默认值 | 说明 |
|---|---|---|---|
| `DATABASE_URL` | ✅ | — | 缺失则**启动即失败**（退出码 2，错误码 `CONFIG_MISSING_REQUIRED`）。⚠️ **必须是普通角色的连接串** —— 超级用户会绕过 RLS，应用会**拒绝启动**（退出码 3，见 §3.4） |
| `BASE_ALLOW_SUPERUSER` | | `false` | 是否放行「绕过 RLS 的角色」启动。**仅本地开发**用；`BASE_ENV=prod` 时配 `true` 直接**启动即失败**（`CONFIG_FORBIDDEN_COMBINATION`） |
| `BASE_MANIFEST_ROOT` | | `.` | 租户作用域声明的根目录（其下是 `modules/<模块>/manifest.json`）；`base tenant-policies` 按它生成策略。镜像内 `/app` |
| `REDIS_URL` | | — | 不配则不使用缓存 |
| `HOST` | | `0.0.0.0` | 监听地址 |
| `SERVER_PORT` | | `8080` | 监听端口 |
| `BASE_ENV` | | `dev` | `dev` / `staging` / `prod` |
| `BASE_TENANT_MODE` | | `shared` | `shared` / `schema` / `database` |
| `BASE_LOG_LEVEL` | | `info` | `error` / `warn` / `info` / `debug` / `trace`；**真的生效**（过滤器同时覆盖 bin target `base` 与 lib target `platform_server`，见 §5.3） |
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
  --from-literal=DATABASE_URL='postgres://base_app:***@postgres:5432/base' \
  --from-literal=MIGRATION_DATABASE_URL='postgres://base:***@postgres:5432/base' \
  --from-literal=APP_ROLE_PASSWORD='***' \
  --from-literal=REDIS_URL='redis://redis:6379'
```

| 键 | 用途 | 必须是 | 谁引用它 |
|---|---|---|---|
| `DATABASE_URL` | **应用**连接 | **普通角色**（`base_app`）—— 超级用户会绕过 RLS，应用拒绝启动 | app Deployment |
| `MIGRATION_DATABASE_URL` | 迁移 / 建策略连接 | **超级用户**（要 DDL 权限） | migrate / policies / app-role 三个 hook Job |
| `APP_ROLE_PASSWORD` | 应用角色口令 | 与 `DATABASE_URL` 里的口令一致 | app-role hook Job |
| `REDIS_URL` | 缓存 | 可选 | app Deployment（`optional: true`：没这个键也不影响启动） |

> `APP_ROLE_PASSWORD` 与 `DATABASE_URL` 里的口令**必须一致**：`app-role` hook Job 用
> `APP_ROLE_PASSWORD` 建角色，应用用 `DATABASE_URL` 连 —— 不一致就认证失败。
> 两处都由运维在集群外填入，Chart 不生成 Secret。

⚠️ **应用 Pod 只引用它需要的键，不整包引用 Secret**：

| 工作负载 | 引用的 Secret 键 |
|---|---|
| app Deployment | `DATABASE_URL`（+ 可选的 `REDIS_URL`） |
| migrate / policies / app-role 三个 hook Job | `MIGRATION_DATABASE_URL`（+ app-role 另有 `APP_ROLE_PASSWORD`） |

**为什么不能整包 `envFrom`**：Secret 里躺着超管连接串 —— 整包注入 = 应用 Pod 的
环境变量里就有超管凭据。应用一旦被拿下（环境变量泄露 / 崩溃转储 / 某个依赖读环境变量），
攻击者就能拿超管连库，**RLS 被绕过 → 跨租户全量可读** —— 等于把「应用用普通角色」
这层白做。compose 的 `app` 服务同理：**不用 `env_file: .env`**，只逐个列变量
（见 §3.1）。


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

> **在 `deploy/` 目录里执行**（推荐，也是本手册唯一推荐的写法）：`.env` 与
> `compose.prod.yaml` 同目录，于是 compose 的项目级 `.env`、服务级 `env_file`、
> `scripts/prod-preflight.sh` 的 `ENV_FILE` **三者解析到同一个文件**，不会串。
>
> §3.2 / §3.3 的示例从**仓库根目录**执行（用 `-f deploy/compose.prod.yaml`）——
> 这也没问题：`.env` 仍然按**compose 文件所在目录**找（见下方「`env_file` 解析基准」）。
>
> **服务级 `env_file` 的解析基准**（2026-11-18 实测更正 —— 本节此前把
> `podman-compose` 的基准写反了）：
> 相对路径**按 compose 文件所在目录**解析，两种 compose 实现**一致**：
>
> | 实现 | 服务级 `env_file` 相对路径基准 | 项目级 `.env`（供 `${VAR}` 插值） |
> |---|---|---|
> | `docker compose` v2 | **compose 文件所在目录** | **compose 文件所在目录**（`--project-directory` 可改） |
> | `podman-compose` 1.6.0 | **compose 文件所在目录** | **compose 文件所在目录** |
>
> - `docker compose` v2 的依据是官方文档：*"Relative paths are resolved from the
>   Compose file's parent folder."*（<https://docs.docker.com/reference/compose-file/services/#env_file>）
> - `podman-compose` 1.6.0 的依据是**实测**（见 `feature-meta/verification-report.md` §3.8）
>   与源码：服务级 `env_file` 用 `os.path.realpath(os.path.join(dirname, path))` 拼接，
>   `dirname` 就是 compose 文件所在目录。
>
> ⚠️ **不要再用 `ENV_FILE=deploy/.env`**（本节此前的备选写法是**错的**）：
> 那是按**当前目录**写的相对路径，compose 会把它拼到 compose 文件所在目录 →
> 变成 `deploy/deploy/.env` → 直接报错退出：
>
> ```console
> $ cd <仓库根> && ENV_FILE=deploy/.env podman-compose -f deploy/compose.prod.yaml up app
> ValueError: Env file at <仓库根>/deploy/deploy/.env does not exist
> ```
>
> 要换 env 文件，值必须是**相对 compose 文件所在目录**的路径（如 `./prod.env`）
> 或**绝对路径**。
>
> ⚠️ **`ENV_FILE` 在两个工具里基准不同**（易踩坑）：
> `scripts/prod-preflight.sh` 是普通 shell 脚本，它的 `ENV_FILE` 按**当前工作目录**解析；
> compose 的服务级 `env_file` 按**compose 文件所在目录**解析。
> 在 `deploy/` 里执行时两者基准相同（都是 `deploy/`）→ 没差异；
> 从仓库根执行时，preflight 要 `ENV_FILE=deploy/.env`、compose 那边要 `ENV_FILE=./.env`
> —— **用绝对路径最省事**（两个工具都认同一份）。
>
> 报错风格差异：`podman-compose` 缺必填变量时抛 **Python traceback**
> （看最后一行 `ValueError: required variable POSTGRES_PASSWORD is missing a value: …` 即可），
> `docker compose` 是一行 `error: …`。

```bash
cd deploy
cp ../.env.example .env      # 编辑 .env：改 APP_DATABASE_URL / MIGRATION_DATABASE_URL、BASE_ENV=prod

# ⚠️ 生产口令**没有默认值**：POSTGRES_PASSWORD 必须显式注入，
#    缺失时 compose 直接报错退出（弱口令另由 preflight 拒绝）。
#    生成：openssl rand -base64 24
#    .env 里至少要有：
#      POSTGRES_PASSWORD=<随机口令>
#      APP_ROLE_PASSWORD=<随机口令（与 APP_DATABASE_URL 里的一致）>
#      POSTGRES_DB=base
#      APP_DATABASE_URL=postgres://base_app:<APP_ROLE_PASSWORD>@postgres:5432/base   ← **普通角色**
#      MIGRATION_DATABASE_URL=postgres://base:<POSTGRES_PASSWORD>@postgres:5432/base ← 超级用户
#    ⚠️ 口令含 / @ : # 等 URL 保留字符时必须百分号编码（%2F、%40 …）

../scripts/prod-preflight.sh   # 口令 / 库名 / 连接串前置校验，不通过就不许 up

# ⚠️ 应用容器**只拿它需要的变量**（compose.prod.yaml 的 app 服务逐个列 environment，
#    不用 env_file 整包注入 .env）：.env 里的 MIGRATION_DATABASE_URL（超管连接串）
#    与 APP_ROLE_PASSWORD **不进应用容器**。应用被拿下 ≠ 超管被拿下。
#    .env 里其余变量仍会经 compose 插值进 app 的 environment（BASE_ENV / BASE_LOG_LEVEL …）。

docker compose -f compose.prod.yaml up -d postgres redis
docker compose -f compose.prod.yaml --profile app-role run --rm app-role   # ① 建应用角色
docker compose -f compose.prod.yaml --profile migrate run --rm migrate     # ② 迁移（建表）
docker compose -f compose.prod.yaml --profile policies run --rm policies   # ③ 策略（建 policy）
docker compose -f compose.prod.yaml up -d --no-deps app                     # ④ 起应用
docker compose -f compose.prod.yaml ps
curl -fsS http://127.0.0.1:8080/health/ready
```

> **交付给客户**：`compose.prod.yaml` + `.env` + `scripts/prod-preflight.sh`
> 放在同一套目录里（脚本放同级 `scripts/` 子目录），命令改成 `./scripts/prod-preflight.sh`。

> 迁移 / 策略是**独立步骤**：`up -d` **不会**自动跑它们（`migrate` / `policies` 服务带
> `profiles: ["migrate"]` / `["policies"]`）。必须按 ①②③④ 的顺序手动跑 ——
> 多副本并发迁移会冲突，而策略必须建在**已存在的表**上。

**私有化 compose 的四个要点**：

| 项 | 规则 |
|---|---|
| `POSTGRES_PASSWORD` | **无默认值**。`${POSTGRES_PASSWORD:?...}` —— 缺失或为空时 compose 解析阶段就报错退出，不会静默用弱口令建库；弱口令（`base` / `postgres` / `password` …）由 `scripts/prod-preflight.sh` 拒绝 |
| `POSTGRES_IMAGE` / `REDIS_IMAGE` | 默认**官方地址**（`docker.io/library/postgres:17-alpine` / `docker.io/library/redis:7-alpine`）。客户内网请指向自己的仓库：`POSTGRES_IMAGE=registry.internal/library/postgres:17-alpine`。国内加速是**开发环境**的事（仓库根 `compose.yaml` 才用 daocloud 前缀） |
| 库名 | `POSTGRES_DB`（默认 `base`）必须与 `APP_DATABASE_URL` 里的库名一致，否则应用连到不存在的库 —— `prod-preflight.sh` 会比对 |
| **角色** | `APP_DATABASE_URL` **必须用普通角色**。`prod-preflight.sh` 会比对连接串里的用户名与 `POSTGRES_USER`（默认 `base`）：**是超级用户就直接拒绝部署** |

### 3.2 发布流程（备份 → 迁移 → 滚动 → 健康检查 → 回滚）

```bash
# 1. 备份（自动轮转，只留最近 5 份）
#    备份 / 恢复是**管理操作**，要用**超级用户**连接
DATABASE_URL='postgres://base:***@127.0.0.1:5432/base' ./scripts/backup.sh

# 2. 拉新镜像
docker compose -f deploy/compose.prod.yaml pull

# 3. 迁移（失败就停在这里，不要往下走）
docker compose -f deploy/compose.prod.yaml --profile migrate run --rm migrate

# 3.5 租户作用域策略（**必须**在迁移之后；失败就停在这里）
#     漏跑 = 租户作用域表完全没有隔离
docker compose -f deploy/compose.prod.yaml --profile policies run --rm policies

# 4. 滚动重启 app
docker compose -f deploy/compose.prod.yaml up -d --no-deps app

# 5. 健康检查（就绪探针：数据库连通 + 迁移已应用）
curl -fsS http://127.0.0.1:8080/health/ready

# 6. 失败 → 回滚（迁移回滚 + 应用镜像回退）
./scripts/rollback.sh 0                       # 迁移回滚到版本 0
TAG=0.1.0 docker compose -f deploy/compose.prod.yaml up -d --no-deps app
```

> **策略步骤为什么单独一步**：业务模块的 RLS policy 由**平台**按 manifest 统一生成
> （模块不手写 —— 手写迟早会漏，漏一张表 = 该表完全无隔离）。
> 迁移只建表、不建 policy，所以策略必须**单独**跑，且必须**在迁移之后**。
> 生产重放策略时**建议放在低峰期**：它对表加 `ACCESS EXCLUSIVE` 锁，
> 并发读会短暂阻塞（不会读到「空表」，策略应用在一个事务里完成）。
> 校验（不改库、失败即让部署失败）：`base tenant-policies --check`。

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

### 3.4 应用角色与 RLS（租户隔离的前提）

#### 3.4.1 为什么应用必须用普通角色

PostgreSQL 的 RLS（行级安全）对**超级用户**与带 `BYPASSRLS` 属性的角色**不生效**：

| 连接角色 | RLS 是否生效 | 后果 |
|---|---|---|
| 超级用户（`base`，`rolsuper=t`） | ❌ 不生效 | 租户隔离 **fail open** —— 数据库层的兜底**完全失效** |
| `BYPASSRLS` 角色 | ❌ 不生效 | 同上 |
| **普通角色**（`base_app`，`rolsuper=f` / `rolbypassrls=f`） | ✅ 生效 | 不设 `app.tenant_id` → **0 行**（fail closed） |

所以：**超级用户只用来建库、迁移、建策略；应用连接必须用普通角色。**

> 这不是「建议」而是**强制**：应用启动时会自检连接角色，绕过 RLS 就**拒绝启动**
> （退出码 3，见 §7.2）。`BASE_ALLOW_SUPERUSER=true` 只在本地开发用，
> `BASE_ENV=prod` 时配它会直接**启动即失败**（退出码 2）。

角色的分工：

| 角色 | 属性 | 用途 |
|---|---|---|
| `base`（`POSTGRES_USER`） | 超级用户 | 建库、迁移（DDL）、建 RLS 策略 |
| `base_platform_app` | 组角色，NOLOGIN | 持有各租户作用域表的权限 |
| `base_app`（`APP_ROLE_NAME`） | 登录角色，**普通角色** | **应用连接**（组角色的成员） |

#### 3.4.2 怎么建角色

| 形态 | 命令 |
|---|---|
| 私有化 compose | `docker compose -f compose.prod.yaml --profile app-role run --rm app-role`（幂等，可重复跑） |
| K8s / Helm | `app-role` hook Job 自动跑（`pre-install,pre-upgrade`，hook-weight `-10`，**先于**迁移） |
| 开发环境（首次建卷时） | `deploy/seed/02-app-role.sql` 由 compose 挂到 `docker-entrypoint-initdb.d` 自动执行 |
| **已有卷 / 角色缺失** | 见 §3.4.3 |

**应用角色的口令要求**：

| 环境 | 口令 | 从哪来 |
|---|---|---|
| 开发（本机 `base_dev` / `base_test`） | `base`（**弱口令，仅开发用**） | `deploy/seed/02-app-role.sql` 里写死 |
| 私有化生产 | **不得**用弱口令 | `APP_ROLE_PASSWORD`，`openssl rand -base64 24` 生成 |
| K8s 生产 | **不得**用弱口令 | Secret 的 `APP_ROLE_PASSWORD` 键 |

> `deploy/seed/02-app-role.sql` 里的 `base` 是**开发环境专用**的弱口令 ——
> 它只用于本机开发库与测试库。**生产库不要靠这个脚本建角色**：生产角色由
> `app-role` 步骤（compose）或 `app-role` hook Job（Helm）用 `APP_ROLE_PASSWORD` 建，
> 口令要求见上表。弱口令（`base` / `postgres` / `password` …）会被
> `scripts/prod-preflight.sh` 直接拒绝。

#### 3.4.3 角色缺失时怎么补建（数据卷已存在的情况）

⚠️ `deploy/seed/*.sql` **只在数据卷首次创建时执行**（PostgreSQL 官方镜像的
`docker-entrypoint-initdb.d` 约定）。**已有卷的环境升级后不会重跑** —— 角色可能缺失，
或者表权限没落到目标库上。

⚠️ **两套授权机制都要有，缺一不可**：

| 语句 | 管哪一段对象 | 少了会怎样 |
|---|---|---|
| `GRANT … ON ALL TABLES IN SCHEMA public` | **存量**表（早就建好的那些） | 存量表权限仍是 `f` → 应用报 `permission denied for table …` |
| `ALTER DEFAULT PRIVILEGES IN SCHEMA public` | **之后**新建的表 | 新表建出来就没权限 → 应用报 `permission denied for table …` |

> **`ALTER DEFAULT PRIVILEGES` 只管「未来表」** —— 它只影响**执行之后创建**的对象，
> 对**已经存在**的表**完全无效**。而补建场景恰恰是「表早就建好了」：
> 只重跑默认权限，存量表的权限仍然是 `f`（实测：`has_table_privilege` 仍是 `f`，
> 应用照样 `permission denied for table`）。
>
> 所以存量表必须靠 `GRANT … ON ALL TABLES IN SCHEMA public` 补 ——
> 下面三个建角色的地方（`deploy/seed/02-app-role.sql`、compose 的 `app-role` 服务、
> Helm 的 `app-role` hook Job）**都已经带上这句**。

补建命令（**幂等**，可重复执行）：

```bash
# 开发库（base_dev）：重跑角色脚本（存量授权 + 默认权限两套都在里面）
podman exec -i base-postgres psql -v ON_ERROR_STOP=1 -U base -d base_dev < deploy/seed/02-app-role.sql

# 私有化生产库：重跑 app-role 步骤（用生产口令；同样含存量 + 增量两套授权）
docker compose -f deploy/compose.prod.yaml --profile app-role run --rm app-role

# K8s：重新触发 hook（删掉旧 Job 再 upgrade）
kubectl delete job base-platform-app-role -n base-platform
helm upgrade base-platform deploy/helm/base-platform -n base-platform
```

**手工补存量授权**（上面任一条跑完权限仍不对时，用**超级用户**连接直接执行）：

```sql
GRANT USAGE ON SCHEMA public TO base_platform_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO base_platform_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO base_platform_app;
```

> ⚠️ 这三句**只影响执行当时已存在的表**；之后再建的表靠
> `ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO base_platform_app;`
> （上面三个建角色的地方都有这句）。
>
> ⚠️ `GRANT … ON ALL TABLES IN SCHEMA public` 会把 **public schema 下所有表**
> 都授权给应用角色组 —— 私有化单机的 public schema 里只有平台表与模块表，
> 这正是要授权的范围。若库里有**不该给应用**的表，把它挪出 public schema
> 或单独 `REVOKE`。

**症状对照**：

| 现象 | 原因 | 处置 |
|---|---|---|
| 应用启动报 `role "base_app" does not exist` | 角色没建 | 跑 §3.4.3 的补建命令 |
| 应用连得上，但 `permission denied for table <表>` | 角色存在但**没表权限**（存量表没被授权，或默认权限没落到这个库） | 同上：重跑角色脚本 / app-role 步骤（**存量授权 + 默认权限两套都跑**；`ALTER DEFAULT PRIVILEGES` 是**按库**生效的，且只管未来表） |
| readiness 503 + `MIGRATIONS_PENDING`（detail 是 `permission denied for table _sqlx_migrations`） | 同上 | 同上 |

> 确认补建是否真的生效（两条都要看）：
>
> ```bash
> # ① 权限位：期望 t
> podman exec -i base-postgres psql -U base -d base_test -c \
>   "select has_table_privilege('base_app','public.platform_tenant_quota','SELECT') as quota_select;"
> # ② 实际能读：期望能出结果行（不报 permission denied）
> podman exec -i base-postgres psql -U base_app -d base_test -c \
>   "select count(*) from platform_tenant_quota;"
> ```


#### 3.4.4 怎么确认 RLS 真的生效

```bash
# 1. 角色属性：应用角色必须不是超管、没有 BYPASSRLS
podman exec base-postgres psql -U base -d base -c \
  "select rolname, rolsuper, rolbypassrls from pg_roles where rolname in ('base','base_app','base_platform_app');"
# 期望：base|t|t ；base_app|f|f ；base_platform_app|f|f

# 2. 表确实开了 RLS 且有 policy
podman exec base-postgres psql -U base -d base -c \
  "select c.relname, c.relrowsecurity, (select count(*) from pg_policy p where p.polrelid=c.oid) as policies
     from pg_class c join pg_namespace n on n.oid=c.relnamespace
    where n.nspname='public' and c.relkind='r' and c.relrowsecurity;"

# 3. 策略齐全（平台统一校验；失败即非 0）
base tenant-policies --check        # 交付二进制
cargo xtask tenant-policies --check # 开发期

# 4. 行为验证：不设 app.tenant_id，普通角色 0 行；超级用户能看到行（对照组）
podman exec base-postgres psql -U base_app -d base -c "select count(*) from <租户作用域表>;"  # 期望 0
podman exec base-postgres psql -U base     -d base -c "select count(*) from <租户作用域表>;"  # 对照组：> 0
```

> 第 4 步的对照组很关键：普通角色看到 0 行，必须**同时**确认超级用户能看到行 ——
> 否则「0 行」可能只是因为表里没数据。

---

## 4. 云上部署（K8s / Helm）

```bash
# 1. Secret（Chart 只引用，不生成）
kubectl create secret generic base-platform-secret \
  --from-literal=DATABASE_URL='postgres://base_app:***@postgres:5432/base' \
  --from-literal=MIGRATION_DATABASE_URL='postgres://base:***@postgres:5432/base' \
  --from-literal=APP_ROLE_PASSWORD='***' \
  --from-literal=REDIS_URL='redis://redis:6379'

# 2. 安装（hook Job 自动跑：应用角色 → 迁移 → 租户作用域策略）
helm upgrade --install base-platform deploy/helm/base-platform \
  --namespace base-platform --create-namespace \
  --set image.repository=registry.internal/base-platform \
  --set image.tag=0.1.0 \
  --set ingress.enabled=true \
  --set ingress.hosts[0].host=base.example.internal

# 3. 看结果
kubectl rollout status deployment/base-platform -n base-platform
kubectl get job base-platform-app-role -n base-platform
kubectl get job base-platform-migrate -n base-platform
kubectl get job base-platform-policies -n base-platform

# 4. 回滚（Helm 自带）
helm rollback base-platform <revision> -n base-platform
```

**三个 hook Job 的顺序**（`helm.sh/hook-weight` 决定，数字小的先跑）：

| Job | hook-weight | 做什么 | 用什么连接 |
|---|---|---|---|
| `base-platform-app-role` | `-10` | 建应用角色（普通角色）+ 组角色 | 超级用户（`MIGRATION_DATABASE_URL`） |
| `base-platform-migrate` | `-5` | 建表 | 超级用户 |
| `base-platform-policies` | `-4` | 按 manifest 生成 RLS policy | 超级用户 |

> **策略 Job 必须在迁移之后**（`-4` > `-5`）：策略是对已建好的表做的。
> 应用（Deployment）用 `DATABASE_URL` = **普通角色**连接；超级用户连接串只出现在
> 这三个 Job 里。
>
> ⚠️ **不整包引用 Secret**：Deployment 逐个 `valueFrom.secretKeyRef` 点名
> `DATABASE_URL`（+ 可选的 `REDIS_URL`），三个 hook Job 各自只点名自己要用的键
> （`MIGRATION_DATABASE_URL`；app-role 另有 `APP_ROLE_PASSWORD`）。
> 整包 `envFrom: secretRef` = 应用 Pod 的环境变量里躺着超管连接串 ——
> Pod 一旦被拿下就等于超管被拿下（RLS 被绕过 → 跨租户全量可读）。
> 自检：`helm template base-platform deploy/helm/base-platform` 渲染出的
> **Deployment 块里不应出现 `envFrom`，也不应出现 `MIGRATION_DATABASE_URL`**。

模板渲染自检（本机无 helm 时用）：

```bash
helm template base-platform deploy/helm/base-platform | grep -E '^kind:'
# 默认渲染 7 类：ServiceAccount / ConfigMap / Service / Deployment / Job(app-role) / Job(migrate) / Job(policies)
# **不含 Ingress** —— values.yaml 里 ingress.enabled 默认 false

helm template base-platform deploy/helm/base-platform --set ingress.enabled=true | grep -E '^kind:'
# 加 --set ingress.enabled=true 后才有 8 类：上面 7 类 + Ingress
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

**语言边界（两条通道各管各的，别越界）**：

| 通道 | 语言 | 给谁看 | 规则 |
|---|---|---|---|
| 响应体 `detail` | **英文（ASCII）** | 开发者 / 前端 / 监控系统 | 契约 C6：**不得**出现中文句子；后端只保证 `code` 稳定 + `detail` 是英文 |
| 响应体 `title` | 中文短句 | 人 | 允许（如 `服务未就绪`）—— C6 只约束 `detail`，不约束 `title` |
| 日志（`tracing::warn!` / `info!`） | **中文运维说明** | 运维 | 运维视角友好，**允许**中文；日志里的中文**不进任何响应体** |

> **一句话**：**响应体 `detail` 永远是英文；给运维看的中文说明永远在日志里。**
> 别把中文写进 `detail`（违反 C6），也别把日志里的中文搬进响应体。

运维视角的中文说明在**日志**里（`tracing::warn!` 记 `code` + `detail` + `pending` + `dirty`），
不进响应体。实测（`BASE_LOG_LEVEL=info`，数据库不可达）：

```console
2026-11-18T02:31:12Z  WARN base: readiness 未通过：数据库不可达或迁移未应用（面向用户的中文文案由前端按 code 渲染） code="DATABASE_UNAVAILABLE" detail=database is unreachable: pool timed out while waiting for an open connection pending=[20261118000100] dirty=None
```

同一次请求的**响应体**（`detail` 是英文，`title` 是中文短句，两条通道互不串味）：

```json
{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,"detail":"database is unreachable: pool timed out while waiting for an open connection","instance":"/health/ready","code":"DATABASE_UNAVAILABLE","trace_id":"b8dbc46d-…"}
```

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

### 5.3 日志级别（`BASE_LOG_LEVEL`）

| 变量 | 默认 | 说明 |
|---|---|---|
| `BASE_LOG_LEVEL` | `info` | `error` / `warn` / `info` / `debug` / `trace`；**真的生效**（见下） |
| `RUST_LOG` | — | 设了就用它（标准 tracing 过滤语法），优先级高于 `BASE_LOG_LEVEL` |

**过滤器必须覆盖事件所在的 target，否则日志被静默丢弃**（一行都不打，还不报错）。
本服务的日志事件写在 `crates/platform-server/src/main.rs` 里 → tracing 的 target 是
**bin crate 名 `base`**（不是 lib crate 名 `platform_server`）。
过滤器同时覆盖 **bin target `base`** 与 **lib target `platform_server`**
（`main.rs` 的 `log_filter()`，4 条单测守着，见 `feature-meta/verification-report.md` §3.7）。

```console
$ cd deploy
$ docker compose -f compose.prod.yaml logs app | tail -20     # 看日志
# 改级别：编辑 deploy/.env 里的 BASE_LOG_LEVEL=warn（默认 info），改完 up -d 生效
# K8s：改 deploy/helm/base-platform/values.yaml 里的 BASE_LOG_LEVEL
```

实测三档（`base serve`，数据库不可达）：

| `BASE_LOG_LEVEL` | 日志行数 | 内容 |
|---|---|---|
| `info` | 2 | `INFO base: 底座服务已启动 …` + `WARN base: readiness 未通过：…` |
| `warn` | 1 | 只有 `WARN base: readiness 未通过：…` |
| `error` | 0 | 无（没有 error 级事件） |

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

### 7.1 迁移 / 策略步骤失败

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
| 策略步骤报 `must be owner of table` | 用了**应用角色**跑策略 | 策略要用**超级用户**连接（`MIGRATION_DATABASE_URL`） |
| 策略步骤报 `relation "xxx" does not exist` | **没先跑迁移**（策略是对已建好的表做的） | 先跑迁移，再跑策略 |
| 策略步骤报 `租户隔离前提不成立：table ... has no isolation policy` | 校验模式发现漏建的表 | 跑 `base tenant-policies`（不带 `--check`）补建 |

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
| **退出码 3，日志有「租户隔离前提不成立」** | **连接角色绕过 RLS**（用了超级用户 / BYPASSRLS 角色） | **把 `DATABASE_URL` 换成普通角色**（`base_app`）—— 见 §3.4.1；本地开发临时可用 `BASE_ALLOW_SUPERUSER=true` |
| 退出码 2，`CONFIG_FORBIDDEN_COMBINATION` + `BASE_ALLOW_SUPERUSER` | 生产环境开了逃生开关 | 去掉它，改用普通角色连接 |
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
