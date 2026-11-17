# 需求：平台骨架与工程基线

- **需求编号**: REQ-002
- **父需求**: [REQ-001 通用项目底座](../REQ-001-platform-foundation/demand.md)
- **优先级**: P0
- **状态**: 已就绪
- **所属迭代**: Sprint 1
- **更新时间**: 2026-11-17 23:45

## 需求描述

搭出可编译、可运行、可扩展的 monorepo 骨架，让后续所有模块有统一落位。

本需求是 REQ-001「通用项目底座」的拆分子项，负责其中的一个能力域。

## 依赖

无 —— 可直接开工

## 功能要求

- Rust workspace：crates/platform-core（平台核心）、crates/platform-contracts（契约套件）、crates/*-cli（xtask）
- 前端 pnpm workspace：apps/shell（底座壳）、packages/ui|sdk|auth|schema|theme|i18n|hooks（SDK 各包占位）
- 目录规范：modules/<name>/ 业务模块目录约定 + manifest.json schema
- 工具链固定：Rust 版本、Node/Bun 版本、PostgreSQL 版本，全部锁文件
- CI 模板：lint / test / build / 依赖扫描 / 契约检查 五个 job
- 本地环境：docker compose 一键起 PostgreSQL + Redis + 种子数据

### 低配开发基线（硬性要求）

开发机实测：8 线程 i7-8550U / 7.7G 内存 / 4.9G 可用 / 8G swap。
底座**必须能在 4G 可用内存下完成日常开发**，不得假设开发者有 16G+ 的机器。

- `.cargo/config.toml`：`build.jobs = 3`；链接器用 `rust-lld`（或 mold），链接期内存降 30-50%
- `Cargo.toml` `[profile.dev]`：`debug = "line-tables-only"` + `split-debuginfo = "unpacked"`
- `[profile.dev.package."*"]`：`opt-level = 0` + `debug = 0`（第三方依赖不优化，省编译时间）
- `[profile.release]`：`lto = "thin"` + `strip = "debuginfo"`
- 日常主力命令是 `cargo check`（比 build 快 40-50%、内存低 60%），不做 watch 式全量检查
- Sprint 1 的依赖集刻意保持最小（≤250 crates），重量级依赖（sqlx / reqwest / aws-sdk / opendal）推到后续 Sprint
- 前端开发用 MSW mock，不依赖 Rust 后端常驻
- `svelte-check --watch` 与 Storybook 不设为常驻服务
- 集成测试用本机 `base_test` 库，**不用 testcontainers**（避免每个测试环境起新容器）
- 完整 E2E 只在验收时串行跑，不进本地日常循环

详见 [低配开发指南](../../plan/dev-guide-low-spec.md)。

## 不在范围内

- 业务功能实现（各子需求负责）
- 具体 SDK 包实现（见 REQ-011）

## 关联验收标准

父需求 AC：AC-039

本需求自有 AC：

### AC-S-001: 目录规范可校验: 新增一个模块目录不符合约定时，CI 校验失败并指出缺失项
- **前置条件**: 见父需求 REQ-001 对应域的测试夹具约定
- **测试步骤**: 1. 按需求描述构造前置状态
2. 执行该能力对应的操作序列
- **预期结果**: 行为与本条标题一致，且失败路径有明确错误码
- **类型**: 功能

### AC-S-002: 工具链版本锁定: 新克隆仓库无需手工安装 Rust/Bun/PostgreSQL 即可构建
- **前置条件**: 见父需求 REQ-001 对应域的测试夹具约定
- **测试步骤**: 1. 按需求描述构造前置状态
2. 执行该能力对应的操作序列
- **预期结果**: 行为与本条标题一致，且失败路径有明确错误码
- **类型**: 功能


## 完成定义

- `bun run dev` 起全栈
- `cargo build --workspace` 通过
- CI 五个 job 全绿
- 目录规范校验生效
