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
