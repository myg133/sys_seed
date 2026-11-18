# 更新日志

本文件记录底座各需求的变更。

## Unreleased

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
