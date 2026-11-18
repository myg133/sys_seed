# 当前迭代计划

- **迭代编号**: Sprint 1 —— 地基 ✅ **已完成**
- **时间范围**: 2026-11-17 23:15 → 2026-11-18 10:50（含一次断电重启）
- **父需求**: [REQ-001 通用项目底座](../demands/REQ-001-platform-foundation/demand.md)
- **路线图**: [roadmap.md](../plan/roadmap.md) ｜ **复盘**: [sprint-01-retro.md](sprint-01-retro.md)
- **下一个迭代**: Sprint 2 —— 身份（REQ-006 多租户 → REQ-007 认证 → REQ-008 用户与组织）

## 迭代目标

> 打通「二次开发友好」这条主线：**用 `create-module` 生成一个业务模块 → AI 按提示词补全代码 → 契约测试通过 → 平台核心零改动 → 能部署起来。**

这条链路跑通后，后续 19 个子需求都是在稳定的接缝上填内容。

## 需求列表

| 需求编号 | 名称 | 优先级 | 状态 | 负责人 | Worktree | 合并点 |
|---------|------|--------|------|--------|----------|--------|
| REQ-002 | 平台骨架与工程基线 | P0 | ✅ 已完成 | Dev (myg133-ba-1 认领) | 已回收 | `develop` @ `eacff21` |
| REQ-003 | 扩展点框架与契约测试 | P0 | ✅ 已完成 | Dev (myg133-ba-1 认领) | 已回收 | `develop` @ `c8236b0` |
| REQ-004 | AI 引导体系与能力索引 | P0 | ✅ 已完成 | Dev (myg133-ba-1 认领) | 已回收 | `develop` @ `37a93c5` |
| REQ-005 | 交付基线与发布 | P1 | ✅ 已完成 | Dev (myg133-ba-1 认领) | 已回收 | `develop` @ `806bb66` |

**承接父 AC**：AC-031~AC-039、AC-009 的基础设施部分（租户上下文机制的桩）。

## 进度

- 总需求: 4 | **已完成: 4** | 进行中: 0 | 待开始: 0
- 合计：241 文件 / +24548 行；Rust **142/142**、前端 23/23、E2E 1/1

## 迭代出口标准

| 出口标准 | 状态 | 证据 |
|---|---|---|
| `bun run dev` 一条命令起全栈 | ✅ | 后端 `/api/health` 200 + 前端 5173 200（Dev 与 QA 均实测） |
| `cargo build --workspace` 通过 | ✅ | 冷编译 21s / 峰值 1087MB |
| `create-module` 生成的模块立即可编译运行 | ✅ | **本迭代最关键的验收点** —— 直接过 `check-modules` 与契约 C-1~C-4（QA 用 `demo-qa-check` 与长名 `order-management` 各验一遍） |
| 故意漏掉租户上下文的模块实现被契约测试拦截 | ✅ | C-3 破坏性验证：报出 6 条跨租户细节 |
| `cargo xtask capabilities` 自动生成能力索引 | ✅ | 13 文件，逐字节可复现；手改即失败（`E_CAPABILITIES_OUTDATED`） |
| 仓库级 AGENTS.md + 模块级 AGENTS.md + `prompts/` 齐备 | ✅ | AGENTS.md 里 13 条命令逐条实测过；prompts 8 套 |
| `docker compose up` 可起完整环境 | ✅ | rootless podman，双容器 **81MB** |
| Helm 渲染 | ✅ | `helm lint` + `helm template` 过（默认 5 类，`ingress.enabled=true` 才 6 类）；**真实集群部署未验** |
| 数据库迁移与回滚演练通过 | ✅ | 无 `down.sql` 的版本被拒绝回滚（exit 1），表未动 |
| 备份恢复演练 | ✅ | 真实计时：备份 307ms / 恢复 704ms / 校验演练 1509ms（QA 复测同量级） |
| 健康检查异常路径 | ✅ | PG 停 → live 200 / ready **503**（**2.002s**） |
| 配置外置 + fail-fast | ✅ | 缺/空 `DATABASE_URL` → exit 2；prod compose 缺口令 → exit 1 |
| CI 六个 job 全绿 | ⚠️ | 已写但**未实跑**（仓库无 remote）；本地等价命令全绿 |

## 环境备注（低配机）

- 开发机 8 线程 i7-8550U / 7.7G 内存 / 4.6G 可用 —— **Rust 编译串行执行，不并行**
- 日常主力命令 `cargo check`（增量 < 1s）；依赖树仅 91 个 crate
- 容器用 rootless podman，**不用 docker**（省 ~375M 常驻内存）
- 镜像必须用 `docker.m.daocloud.io/` 显式前缀（docker.io 直连超时）
- 磁盘预算 20G，实际只用了约 2G
- **断电重启后必须手工恢复**：bus watcher + podman 容器（详见 dev-guide-low-spec.md §8.5）
- 详见 [dev-guide-low-spec.md](../plan/dev-guide-low-spec.md)

## 依赖顺序（已完成部分）

```text
REQ-002 平台骨架
   ├─→ REQ-003 扩展点框架 ──→ REQ-004 AI 引导体系
   └─→ REQ-005 交付基线
```

## 风险（实际发生的）

| # | 风险 | 实际结果 |
|---|------|---------|
| R1 | 扩展点签名定不下来导致后续返工 | ✅ 已冻结 5 个 trait，写入 [extension-points v1](../../contracts/extension-points/v1.md)；发现 RPITIT 形态**不可用于 trait object**，已登记为 REQ-015/017 的决策项 |
| R2 | 能力索引手工维护必然过期 | ✅ 由 `xtask capabilities` 自动生成 + `--check` 校验 + `source_fingerprint` 保证可复现 |
| R3 | 私有化部署与 K8s 环境不具备 | ⚠️ compose + Helm Chart 齐备，**真实集群部署与镜像实构建未验**（本机内存不够 / 仓库无 remote） |
| R4 | 机器断电导致进行中的 Agent 静默死亡 | ⚠️ 实际发生一次（REQ-005 修复轮），恢复成本约 20 分钟；已写恢复流程 |

## 下一步

1. 加 remote（GitHub / Gitea）→ CI 跑起来 → 验证 6 个 job 与镜像体积门禁
2. 在仓库设置里配置分支保护：`core-change-review` 设为 required check + 打开 Require review from Code Owners
3. 进入 Sprint 2（身份）：REQ-006 → REQ-007 → REQ-008，**顺序不可调换**（认证的会话与令牌必须从第一行就带租户维度）
4. 把复盘里的 9 项流程改进落到各子需求的模板里
