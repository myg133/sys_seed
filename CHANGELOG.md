# 更新日志

本文件记录底座各需求的变更。

## Unreleased

### REQ-005 交付基线与发布

- **交付形态**：同一套镜像 + 同一套配置，三处复用 —— 开发（本机 compose）/
  私有化部署（企业内网 compose）/ 云上（K8s Helm）
- **多阶段镜像** `Dockerfile`：`builder`（`rust:lld`，cargo-chef 分层缓存依赖）/
  `frontend`（Node + pnpm）/ `runtime`（`debian:12-slim`，非 root，uid 10001），
  体积目标 < 100MB（CI `build-image` job 硬校验）；**本机不构建**，镜像构建交给 CI
- **私有化部署** `deploy/compose.prod.yaml`：镜像走 tag、不本地构建；
  **迁移是独立服务**（`profiles: ["migrate"]`），`up -d` 不会自动跑迁移
- **Helm Chart** `deploy/helm/base-platform/`：Deployment / Service / Ingress /
  ConfigMap / Secret **引用**（Chart 不生成 Secret）/ 迁移 Job
  （`helm.sh/hook: pre-upgrade,pre-install`）；探针 `/health/live` 与 `/health/ready`
- **配置外置**（AC-S-005）：`crates/platform-server` 的 `DeployConfig` 读环境变量，
  启动时校验、**缺必填项 fail-fast**（退出码 2，稳定错误码
  `CONFIG_MISSING_REQUIRED` / `CONFIG_INVALID_VALUE` / `CONFIG_FORBIDDEN_COMBINATION`）；
  `.env.example` 入库、`.env` 不入库，Secret 不进镜像
- **迁移独立化**（AC-S-006）：`base migrate up|down|status`，迁移文件用 sqlx 可逆格式
  `<版本>_<描述>.up.sql` + `.down.sql`；不可逆迁移拒绝回滚；`scripts/rollback.sh <版本>`
  （回滚前自动备份）
- **健康检查**：`/health/live`（进程活着）与 `/health/ready`（**数据库连通 + 迁移已应用**）；
  未就绪返回 503 + 稳定错误码（`DATABASE_UNAVAILABLE` / `MIGRATIONS_PENDING` /
  `MIGRATION_DIRTY`），错误体按 RFC 9457（契约 C6）
- **备份恢复**：`scripts/backup.sh`（pg_dump + gzip，只留最近 5 份）/
  `scripts/restore.sh` / `scripts/backup-verify.sh`（恢复到临时库 + 冒烟）；
  `deploy/RUNBOOK.md` 含**真实演练记录**与故障排查（迁移失败 / 容器起不来 /
  数据库连不上 / 磁盘满）
- **CI 增补**：`build-image`（构建 + 体积校验 + 健康检查冒烟）、`image-scan`
  （Trivy）、`migration-test`（up → 校验 → down → 再 up）；基础镜像一律官方地址
- **顺带修的 4 项遗留技术债**：`sync-contracts.sh` 拿不到 BA 正本时**明确失败**
  （不再静默跳过）；`core-change-review` 改用 **merge-base** 口径（不再被 base.sha 拖偏）；
  核心路径清单确认覆盖 `platform-core` / `platform-contracts` / `platform-contracts-macros`
  （并加自检，防新增平台 crate 漏进门禁）；`migrations/` 规则在 README 与
  `check-modules` 错误提示里写明「DB 层落地后升级为至少一个 `*.sql`」
- **新增第三方依赖**：`sqlx` 0.8（仅 `runtime-tokio` / `postgres` / `migrate`，
  关闭 `macros` 与 `json` 以压低编译开销）—— 只用于部署期健康检查与迁移，
  业务数据访问层仍待 REQ-006/007

### REQ-004 AI 引导体系与能力索引

- 仓库级 `AGENTS.md`（agents.md 开放规范）：目录 / 硬规则 / 真实可用的命令 /
  已有能力 / 尚未提供的能力；「已有能力」段与能力索引逐行一致（由 xtask 测试守住）
- 任务级提示词库 `prompts/`：8 套成套任务书（新建模块 / 加权限点 / 接 SSO /
  定义流程 / 通知模板 / 越权测试 / 补测试用例 / 性能优化），
  每套含目标 / 输入 / 步骤 / 约束 / 验收
- 能力索引 `capabilities/`（**自动生成，勿手改**）：`index.json`（含 `already_provided`
  与 `not_yet_provided`）、`permissions.json`、`extension-points.json`、
  `schema/manifest.schema.json`、`schema/metadata.schema.json`、
  `sdk/<package>.json`、`api.json`（HTTP 层未就绪，占位并注明 REQ-011/012 补）
- 生成器 `cargo xtask capabilities`：`--check` 只校验不写入，CI 用它发现手改；
  产物逐字节可复现（不含墙钟时间戳），**新增第三方依赖：无**
  （trait 声明用正则 + 约定扫描，不引入 `syn` 等解析框架）
- 脚手架 `cargo xtask create-module <name> [--perm-prefix <prefix>]`：
  生成目录 + manifest + backend/frontend/migrations/tests + 模块级 AGENTS.md + README，
  自动登记进工作区成员，并对生成的 Rust 源文件跑 `rustfmt`；
  **产出开箱即过 `check-modules` 与契约检查 C-1 ~ C-4**
- 模块级 `AGENTS.md` 模板由 `create-module` 生成（领域模型 / 扩展点 / 权限点 /
  如何验证 / 易踩的坑）
- CI 护栏：新增 `guardrails` job（`capabilities --check` + `check-deps`）、
  `core-change-label`（平台核心变更自动打标）与 `core-change-review`
  （平台核心变更必须有非作者的批准评审）+ `.github/CODEOWNERS`
- `cargo xtask check-deps` 子命令：把 REQ-003 已有的 `cargo metadata` 依赖图
  遍历暴露给 CI（未重写实现）

### REQ-004 修复轮（首轮 QA PASS-WITH-RISK）

- **CI 补 `pull_request_review` 触发器**：`on:` 增加
  `pull_request_review: types: [submitted, edited, dismissed]`，`pull_request` 补
  `review_requested` —— 修复前评审者批准后 workflow 不会重跑，
  「核心变更需非作者批准」的门禁永远停在旧状态、无法变绿。
  分支保护（required check + Require review from Code Owners）仍是**仓库设置、需人工配置**
- **修正提示词里的假绿命令**：`prompts/02-add-permission.md` 与
  `prompts/07-write-test-cases.md` 的 `cargo test` 命令多了一个 `--`，
  会**跑 0 个用例却 exit 0**；已修正，并把这条坑写进 `AGENTS.md` 硬规则第 8 条
- **契约引用可操作**：`AGENTS.md` / `prompts/03` 不再指向仓库内不存在的 `BA/contracts/...`；
  正本用 `git show demand:contracts/<name>/<version>.md` 取，
  另在 `docs/contracts/` 放随版本发布的**快照**，由 `scripts/sync-contracts.sh`
  从正本生成并校验（CI 强制），不会出现两份正本漂移
- **`cargo xtask capabilities` 会清理多余产物**：此前只写不删，
  `--check` 报的「多余文件」按提示「重新生成」也修不掉；现在按同一口径剪除并清理空目录
- 口径更正：`already_provided` 为 **18** 条（非 17）；
  AGENTS.md 与 `index.json` 是**内容一致、行的顺序不同**（语义序 vs 字典序），非「逐行一致」

### REQ-003 扩展点框架与契约测试

- 扩展点 trait（`crates/platform-core/src/extension/`）：`PlatformModule` / `Repository` /
  `Auditable` / `NotificationSender` / `WorkflowHandler`，配套类型 `RequestContext`（复用 REQ-002）/
  `ListQuery` / `List` / `AuditEventSpec` / `NotificationMessage` / `DeliveryResult` / `TransitionEvent` /
  `ModuleContext`
- 编译期注册：`inventory` 收集 + `discover()` / `discover_enabled()`，业务模块用
  `platform_core::register_platform_module!` 登记
- 契约测试套件（`crates/platform-contracts`）：C-1 manifest 一致 / C-2 权限点声明完整 /
  C-3 租户隔离 / C-4 事务边界，外加简化版 `#[platform_module_contract]` 宏
  （`crates/platform-contracts-macros`，过程宏，零第三方依赖）
- manifest JSON Schema + Rust 校验器：`crates/platform-core/src/extension/manifest.rs`，
  Schema 导出到 `capabilities/schema/manifest.schema.json`
- 目录规范校验补齐（`cargo xtask check-modules`）：manifest 符合 Schema、`manifest.id` 与目录名一致、
  `backend/`/`frontend/`/`migrations/` 非空；新增依赖方向检查（平台 crate 不得依赖业务模块）
- 示例模块 `modules/sandbox/`：扩展点实现 + 契约测试接入，**二次开发的标准范例**

### REQ-002 平台骨架与工程基线

- Rust workspace 骨架：`crates/platform-core`、`crates/platform-contracts`、`crates/xtask`
- 前端 pnpm workspace：`apps/shell`（Vite + Svelte 5）与 `packages/ui|sdk|auth|schema|theme|i18n|hooks` 占位包
- 工具链锁定：`rust-toolchain.toml`、`.node-version`、`.nvmrc`、`pnpm-lock.yaml`、`bun.lock`
- 低配开发基线：`.cargo/config.toml`（`jobs = 3`）、`Cargo.toml` 的 `profile.dev` / `profile.release`
- 本地环境：`compose.yaml`（PostgreSQL 256M + Redis 96M，显式 daocloud 镜像前缀）+ 种子数据
- 目录规范校验：`cargo xtask check-modules`
- CI 模板：`.github/workflows/ci.yml`（lint / test / build / 依赖漏洞扫描 / 契约检查 / E2E）
- 工程文档：`docs/dev-environment.md`、`docs/architecture.md`

### 退回修复轮（QA Pre-merge PASS-WITH-RISK）

- 链接器表述纠正：stable rustc 1.98 的 `x86_64-unknown-linux-gnu` target spec 默认 `linker-flavor = gnu-lld-cc`，
  即默认已用 LLD 22.1.8，无需显式配置；撤销上一轮「链接器偏离」的记账（`docs/dev-environment.md` §2.1、`.cargo/config.toml` 注释）
- 集成测试脚本：`scripts/test-integration.sh` 增加测试库重置（只重置 `base_test`，绝不碰 `base_dev`），
  并把容器数量比对从 `set -e` 的中断路径中解耦 —— 测试失败时比对照常执行，退出码反映测试结果
- 清理未使用依赖：`crates/platform-contracts` 去掉 `platform-core`；`apps/shell` 去掉
  `@base/auth` `@base/hooks` `@base/schema` `@base/theme`
- CI：新增 `e2e` job（Playwright chromium）；`dependency-audit` 去掉 `rustsec/audit-check@v2` 不支持的 `token` 入参

### 复核轮微修（QA Recheck PASS-WITH-RISK）

- 链接器内存对比表换成无采样缺口口径实测值：LLD **132.0 MB** / bfd **135.5 MB** / 差 **2.6%**
  （内核 `VmHWM` 高水位 + `wait4 ru_maxrss` 双仪器互证，真实仓库二进制）；作废上一版「LLD 16.1 MB /
  bfd 134.0 MB，约 8 倍」的说法（瞬时 `VmRSS` 采样 + 探针 crate，`16.1 MB` 低于 `ld.lld` 空载地板）。
  「默认即 LLD、显式配置反而坏事」的结论保留 —— 理由是 stable rustc 1.98 的默认行为、无需配置，
  而不是因为省内存（`docs/dev-environment.md` §2.1）
- 集成测试脚本：`scripts/test-integration.sh` 的测试库重置闸门从黑名单（`*base_dev*`）改为白名单 ——
  只有库名恰好是 `base_test` 才允许重置，其它库名一律明确报错并 `exit 2`（不静默跳过）
