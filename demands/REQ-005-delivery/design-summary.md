# 设计摘要：交付基线与发布（REQ-005）

- **需求编号**: REQ-005
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **关联契约**: [api-conventions v1](../../contracts/api-conventions/v1.md)
- **环境依据**: [dev-guide-low-spec.md](../../plan/dev-guide-low-spec.md)
- **性质**: 需求细化附件
- **更新时间**: 2026-11-18 00:30

---

## 1. 交付形态

底座要能同时满足三种落地方式，**同一套镜像与配置，三处复用**：

| 形态 | 面向 | 编排 | 产出 |
|---|---|---|---|
| 开发 | 本机/单机 | `compose.yaml`（rootless podman） | 一条命令起全栈 |
| 私有化部署 | 企业内网 | `deploy/compose.prod.yaml` | 单机 docker compose 交付 |
| 云上 | K8s 集群 | `deploy/helm/base-platform` | Helm release |

> 「私有化部署」是这个底座的重要场景 —— 政企客户不会上公有云。

---

## 2. 镜像构建

### 2.1 多阶段 Dockerfile

```text
stage: builder        Rust 工具链 + 源码 → 编译出静态二进制
stage: frontend       Node/Bun → 构建前端产物
stage: runtime        最小运行镜像，只带二进制与静态文件
```

**关键点**：

- builder 阶段用 `rust:lld` 镜像（自带 mold/lld），**不要用官方 `rust` 镜像** —— 后者的 GNU ld 链接内存是本机问题的放大版
- runtime 阶段用 `debian:12-slim` 或 `gcr.io/distroless/cc`，**不要用完整发行版**
- 最终镜像目标 **< 100MB**（不含前端静态文件）
- 前端静态文件由 nginx 或 Rust 侧静态托管二选一，本期统一用 Rust 侧托管（少一个进程、少一份内存）

### 2.2 构建缓存

Rust 依赖层单独缓存：

```dockerfile
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release --locked
COPY . .
RUN cargo build --release --locked    # 二次构建只增量
```

或用 `cargo-chef`（需从源码编译，**本机不装**，CI 里用）。

> 本机不构建镜像 —— 构建 release 镜像会吃光内存。镜像构建交给 CI 或云上。

---

## 3. 配置外置（AC-S-005）

**同一个镜像，通过环境变量切分 dev / staging / prod，不重新构建。**

### 3.1 配置优先级

```text
默认值（代码内）  <  环境变量  <  配置文件  <  配置中心（后续 REQ）
```

与契约 C9 的三层覆盖（平台/租户/空间）是**两个维度**：这里说的是部署环境，契约 C9 说的是租户层级。

### 3.2 环境变量清单（启动时校验，缺必填则快速失败）

| 变量 | 必填 | 示例 |
|---|---|---|
| `DATABASE_URL` | ✅ | `postgres://base:base@pg:5432/base_dev` |
| `REDIS_URL` | | `redis://redis:6379` |
| `SERVER_PORT` | | `8080` |
| `BASE_TENANT_MODE` | | `shared` \| `schema` \| `database` |
| `BASE_LOG_LEVEL` | | `info` |
| `BASE_AUTH_MODE` | | `local` \| `sso` \| `hybrid` |
| `BASE_SEED_ON_START` | | `false`（生产必须 false） |

**配置校验失败要 fail-fast**，不要启动到一半才报错。

### 3.3 Secret 处理

- Secret **绝不进镜像**、**绝不进 Git**
- 生产用 Docker secrets / K8s Secret，由环境注入
- `.env.example` 入库，`.env` 不入库

---

## 4. 数据库迁移

### 4.1 迁移是独立步骤，不是启动流程

```text
❌ app 容器启动时自动跑迁移    → 多副本并发迁移会冲突
✅ 部署流程里先跑 migrate 容器，再滚 app
```

compose：

```yaml
migrate:
  image: base-platform:${TAG}
  command: ["base", "migrate"]
  depends_on: [postgres]
  profiles: ["migrate"]      # 手动触发，不随 up 自动跑
```

K8s：

```yaml
# Job，每次发布前手动 apply；或 Helm pre-upgrade hook
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ include "base-platform.fullname" . }}-migrate
  annotations:
    "helm.sh/hook": pre-upgrade,pre-install
    "helm.sh/hook-weight": "-5"
    "helm.sh/hook-delete-policy": before-hook-creation
```

### 4.2 可回滚（AC-S-006）

- 迁移工具用 **sqlx migrate**，每个迁移**必须**有配套的 `down` 或明确标注不可逆
- 不可逆迁移（删列、改类型）必须：
  1. 拆成「加新列 → 双写 → 切读 → 删旧列」四步跨版本发布
  2. 在 PR 里标注 `⚠️ 不可逆`
- 回滚脚本：`scripts/rollback.sh <version>`，先查 backup 再回滚

### 4.3 迁移测试

- 每个迁移必须有**升级测试**（从上一版本 schema 升上来）
- **降级测试**（可逆迁移才需要）
- 测试库：`base_test`，跑之前重置

---

## 5. 发布与回滚

### 5.1 私有化部署（compose）

```bash
deploy/compose.prod.yaml 的发布脚本：
1. 备份：scripts/backup.sh → backup/base-<timestamp>.sql.gz
2. 拉取新镜像
3. 跑 migrate 容器
4. 滚动重启 app（docker compose up -d --no-deps app）
5. 健康检查通过才算成功
6. 失败 → scripts/rollback.sh <上一版本 tag>
```

### 5.2 K8s

- 基础：`deploy/helm/base-platform`（Deployment + Service + Ingress + ConfigMap + Secret 引用）
- 蓝绿：两套 Deployment（blue/green）+ Service 切流
- 金丝雀：**本期不做**，用 Argo Rollouts 或 Flagger 的话在 README 里注明为可选接入
- 回滚：`helm rollback <release> <revision>`

### 5.3 健康检查

| 探针 | 路径 | 语义 |
|---|---|---|
| liveness | `/health/live` | 进程活着 |
| readiness | `/health/ready` | 数据库连得上、迁移已应用 |

readiness **必须**检查数据库连通性 —— 否则滚动更新时流量会打到还没迁移好的实例。

---

## 6. 备份与恢复

```bash
scripts/backup.sh          # pg_dump + gzip → backup/base-<ts>.sql.gz
scripts/restore.sh <file>  # 恢复到目标库
scripts/backup-verify.sh   # 恢复演练：恢复到临时库并跑一次冒烟
```

**演练是必交付项**（AC-S-006）：写一份 `deploy/RUNBOOK.md`，包含

- 备份/恢复的完整命令
- **演练记录**：日期、执行人、耗时、遇到的问题
- 故障排查：迁移失败、容器起不来、数据库连不上、磁盘满

---

## 7. 本期交付边界

**做**：

- [ ] 多阶段 Dockerfile（builder / frontend / runtime），目标镜像 < 100MB
- [ ] `compose.yaml`（开发）+ `deploy/compose.prod.yaml`（私有化）
- [ ] `deploy/helm/base-platform/`：Chart + values.yaml
- [ ] 配置外置：环境变量清单 + 启动时校验 + `.env.example`
- [ ] 迁移作为独立步骤（compose profile + Helm hook）
- [ ] `scripts/backup.sh` / `restore.sh` / `rollback.sh`
- [ ] `deploy/RUNBOOK.md` + **恢复演练记录**
- [ ] 健康检查端点（liveness / readiness）
- [ ] CI 增加构建镜像与扫描 job

**不做**：

- 金丝雀发布（README 注明可选接入 Argo Rollouts / Flagger）
- 配置中心接入（等 REQ-009 / REQ-010）
- 蓝绿自动化（只提供手工切换脚本）

---

## 8. 资源约束提醒

**本机不构建 release 镜像。** `cargo build --release` + LTO 的内存峰值远超本机余量（4.4G 可用）。
镜像构建放 CI；本机只验证 Dockerfile 语法与 compose 配置：

```bash
podman-compose -f compose.yaml config      # 校验 compose 语法
podman build --help                        # 不实际构建
```

CI 里构建时注意：GitHub Actions runner 默认 7G 内存，Rust release 构建用 `jobs=2` 并开 `--locked`。

---

## 9. 磁盘控制

| 项 | 控制 |
|---|---|
| 本地镜像 | 只保留 `base-platform:dev` 一个 tag，`podman image prune` 清理旧 tag |
| 备份文件 | `backup/` 目录保留最近 5 份，脚本自动轮转 |
| 构建缓存 | CI 用 GHA cache；本机不缓存镜像层 |
| 定期 | `podman system prune -af` + `df -h /home` 检查 |
