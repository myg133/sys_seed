# 更新日志

本文件记录底座各需求的变更。

## Unreleased

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
