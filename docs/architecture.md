# 架构总览

底座采用 **monorepo + 模块化单体**（ADR-0001 D1）：一个仓库同时承载 Rust 平台内核、业务模块与前端 SDK，
让新业务只写业务代码。

## 1. 目录布局

```text
.
├── Cargo.toml               # Rust workspace 根（profile 低配基线）
├── .cargo/config.toml       # 编译并发与任务别名（cargo xtask）
├── rust-toolchain.toml      # Rust 版本锁定
├── compose.yaml             # 本地 PostgreSQL + Redis
├── crates/
│   ├── platform-core/       # 平台核心：配置类型、错误类型、请求上下文
│   ├── platform-contracts/  # 扩展点契约测试套件骨架
│   └── xtask/               # 工程任务运行器（cargo xtask）
├── packages/                # 前端 SDK 各包（占位）
│   ├── ui/  sdk/  auth/  schema/  theme/  i18n/  hooks/
├── apps/
│   └── shell/               # 底座壳：Vite + Svelte 5 单体 SPA
├── modules/                 # 业务模块落位目录（当前为空）
├── deploy/seed/             # 种子数据 SQL
├── docs/                    # 开发环境与架构文档
├── scripts/                 # 统一命令的实现脚本
└── .github/workflows/ci.yml # CI 模板
```

## 2. 依赖方向：单向，不许回头

```text
        平台  ──→  扩展点(trait)  ←──  业务模块
       crates/                        modules/
```

- **平台代码永不依赖业务模块**。`crates/platform-core` 不引用 `modules/` 下任何内容。
- 业务模块实现平台定义的扩展点 trait，由平台分发运行时与配置。
- 扩展点 trait 目前只有**框架**（`crates/platform-contracts`），具体 trait 由 REQ-003 定义。
- CI 的目录规范校验（`cargo xtask check-modules`）守住 `modules/<name>/` 的目录约定，
  平台核心的改动在 PR 中单独标记、强制人工评审。

这条规则是「二次开发友好」的地基：框架能独立编译与升级，业务模块能单独测试、单独启停、单独打包。

## 3. 业务模块目录约定

一个目录 = 一个完整业务模块（ADR-0001 D7）：

```text
modules/<name>/
├── manifest.json    # 权限点、配置键、应用元数据（必填）
├── backend/         # 后端实现（必填）
├── frontend/        # 前端实现（必填）
├── migrations/      # 数据库迁移（必填）
├── AGENTS.md        # 模块级引导：领域模型、涉及哪些扩展点、如何验证
└── README.md        # 模块说明
```

缺任何一项，校验失败并列出缺什么（错误码 `E_MODULE_LAYOUT`）。

## 4. 后端分层

| 层 | 位置 | 职责 |
|---|---|---|
| 平台核心 | `crates/platform-core` | 配置类型、统一错误类型、请求上下文 |
| 契约套件 | `crates/platform-contracts` | 扩展点契约测试框架（业务模块实现同一套测试即合规） |
| 工程任务 | `crates/xtask` | 目录规范校验、后续脚手架与能力索引生成 |

错误类型遵循 `api-conventions v1` C6：稳定的机器可读错误码（大写 + 下划线），
面向用户的中文文案由前端 i18n 渲染，后端不拼中文句子。

## 5. 前端分层

| 层 | 位置 | 说明 |
|---|---|---|
| 底座壳 | `apps/shell` | 单体 SPA + 应用级路由；本期是最小可运行骨架 |
| SDK 包 | `packages/*` | ui / sdk / auth / schema / theme / i18n / hooks，本期为占位 |

依赖方向同样是单向：壳依赖 SDK 包，SDK 包之间不互相依赖（本期无依赖）。

## 6. 本地环境

```bash
bun run db:up        # PostgreSQL + Redis（rootless podman，合计 < 250M）
bun run seed         # 种子数据
bun run dev          # 后端 8080 + 前端 5173
```

本地环境细节与低配机参数见 [dev-environment.md](./dev-environment.md)。
