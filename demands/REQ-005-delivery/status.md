# 状态：REQ-005 交付基线与发布

- **状态**: 已完成
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-18 10:50
- **分支**: `develop` @ `806bb66`（已合并）

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 已就绪 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-18 09:00 | 已就绪 | 进行中 | dev | Dev 交回：48 文件 / +5126 行，新增 `crates/platform-server` |
| 2026-11-18 09:20 | 进行中 | 待验证 | ba | 派 QA Pre-merge |
| 2026-11-18 09:40 | 待验证 | 已退回 | ba | QA 结论 PASS-WITH-RISK：12 项核心检查全真，**1 中 + 4 低** |
| 2026-11-18 10:05 | 已退回 | 进行中 | dev | **机器断电重启，Dev 进程被杀**；改动留在工作树未提交 |
| 2026-11-18 10:20 | 进行中 | 进行中 | dev-2 | 新 Dev 接管复核：5 项实际已完成，补修 3 个漏，Rust 138/138 |
| 2026-11-18 10:35 | 进行中 | 待验证 | ba | QA 修复复核 PASS-WITH-RISK，5 项复验通过，新发现 2 低 |
| 2026-11-18 10:45 | 待验证 | 进行中 | dev-3 | 最终微修：日志级别过滤 + RUNBOOK env_file 说明，Rust 142/142 |
| 2026-11-18 10:50 | 进行中 | 已完成 | ba | 合并到 develop @ 806bb66，worktree 与分支已回收 |

## 完成记录

| 项 | 结果 |
|---|---|
| 合并 | `develop` @ `806bb66`，`--no-ff` |
| 改动 | 9 个提交 / 49 文件 / +6080 -560 行；新增 `crates/platform-server`（`base` 二进制） |
| 回收 | worktree 与分支已删除；两份 QA 报告随合并保留在 `develop` |
| 回归 | Rust **142/142**、clippy 0 告警、前端 23/23、E2E 1/1 |

## QA 三轮发现的问题与处置

| 轮次 | 级别 | 问题 | 处置 |
|------|------|------|------|
| 首轮 | **中** | `health.rs` 把中文塞进 `detail`，违反契约 C6；且 Dev 报告写「符合 C6」属**不实结论** | 已修：`dev_detail()` 过滤为纯 ASCII（PG 服务端消息会随 `lc_messages` 本地化，一并挡掉）；5 条单测守门；报告的不实表述已更正 |
| 首轮 | 低 1 | RUNBOOK/报告称 Helm 默认渲染含 Ingress（实为 `ingress.enabled` 默认 false） | 已改：默认 5 类无 Ingress，`--set ingress.enabled=true` 才 6 类；三处文档同步 |
| 首轮 | 低 2 | readinessProbe 超时 5s 与连接池 acquire_timeout 5s 相撞，探针会先超时拿不到 503 | 已改：acquire_timeout → **2s**，探针 5s，有测试守住关系。实测 ready 503 耗时 **5.003s → 2.002s** |
| 首轮 | 低 3 | `core-change-detect.sh --self-test` 未覆盖「平台 crate 必须登记」 | 已补：自检同时覆盖「未登记必须检出」「已登记必须放行」两方向（QA 用变异测试证明两方向都真守住） |
| 首轮 | 低 4 | prod compose 默认弱口令 `base` + 第三方镜像源 | 已改：`${POSTGRES_PASSWORD:?...}` 缺失即报错退出；镜像默认官方地址且可 `POSTGRES_IMAGE` 覆盖 |
| 复核 | 低 1 | `BASE_LOG_LEVEL` 完全无效 —— 过滤器只匹配 lib target `platform_server`，事件却来自 bin target `base` → **默认零日志**，「中文运维说明在日志里」这条兜底通道是死的 | 已修：过滤器同时覆盖两个 target，+4 条单测；实测 `BASE_LOG_LEVEL=info` 出日志。RUNBOOK 新增**语言边界表**（detail 英文 / title 中文允许 / 日志中文允许） |
| 复核 | 低 2 | RUNBOOK §3.1 把 podman-compose 的 `env_file` 解析基准**说反了**，其备选 `ENV_FILE=deploy/.env` 实测报 `deploy/deploy/.env does not exist` | 已改：选定「方案 A —— 统一在 `deploy/` 目录里执行」，坏写法已删，推荐用法实跑通过；docker compose 与 podman-compose 的差异分别写明 |
| 恢复轮 | 中 | **CI 私有化 compose 校验会挂** —— `.env.example` 新增空的 `POSTGRES_PASSWORD=` → `cp` 到 `deploy/.env` 后 compose 解析必失败 | 已修：CI 显式注入非空占位口令 |
| 恢复轮 | 低 | `prod-preflight.sh` 路径不存在（RUNBOOK 已 `cd deploy` 却写 `./scripts/…`） | 已修 |
| 恢复轮 | 低 | compose 头部用法示例把 `.env` 放错目录 | 已修 |

## 交付要点（可验证的事实）

| 能力 | 实测 |
|---|---|
| 迁移独立于 `up -d` | 默认服务表**不含 migrate**；活证据：`serve` 对零表库未建表 |
| 健康检查异常路径 | PG 停 → live **200** / ready **503** `DATABASE_UNAVAILABLE`（**2.002s**）；PG 起回 → 200 |
| 迁移回滚 | down 只余 `_sqlx_migrations`；**无 `down.sql` 的版本被拒绝回滚**（exit 1）表未动 |
| 备份恢复闭环 | 备份 **307ms** / 恢复 **704ms** / 校验演练 **1509ms**（QA 复测 311/783/1769ms，同量级） |
| 配置 fail-fast | 缺/空 `DATABASE_URL` → exit 2；4 个非法值一次报全；prod+seed → FORBIDDEN |
| 契约 C6 | 4 条 detail 路径**全部纯 ASCII**，测试 `detail.is_ascii()` 守着 |
| sqlx 克制 | 仅 platform-server 依赖；`sqlx-macros` 计数 0；核心两 crate 零改动 |

## 未验证（不阻塞，已如实登记）

- **镜像实构建** —— 本机 4.2G 可用内存不够 release+LTO，交给 CI 的 `build-image` job（含 >100MB 体积硬校验）
- **GitHub Actions 实跑** —— 仓库无 remote
- **真实 K8s 集群部署** —— 本机 kubeconfig 指向的集群不可达；Chart 已过 `helm lint` + `helm template`
- **Trivy 扫描结果** —— 需联网拉漏洞库
- **docker compose v2 的 env_file 解析基准** —— 本机 docker CLI 无 compose 插件，口径来自官方文档
- 数据库 TLS 未开（`tls-none`），随 REQ-006/007 处理

## 遗留技术债

| # | 债务 | 建议归属 |
|---|------|----------|
| 1 | `title` 字段用中文短句（契约 C6 不约束 title，允许）—— 但要防止后续有人误以为 title 也能放中文长句 | 契约文档已注明边界 |
| 2 | `.env.example` 同时服务 dev 与 prod，`DATABASE_URL` 仍指 `base_dev`，prod 必须手改 | 由 `prod-preflight.sh` 兜底拦截 |
| 3 | compose 头部示例与 `env_file` 解析基准在 docker compose / podman-compose 下不同 | 已在 RUNBOOK 分别说明 |

## 阻塞项

无 —— 依赖 REQ-002 已完成，REQ-005 已合并。
