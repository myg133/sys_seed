# 设计摘要：AI 引导体系与能力索引（REQ-004）

- **需求编号**: REQ-004
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **架构决策**: [ADR-0001 D8](../../decisions/ADR-0001-底座技术架构基线.md)
- **关联契约**: [api-conventions v1](../../contracts/api-conventions/v1.md)
- **性质**: 需求细化附件 —— 把引导文件的形态与生成机制写死
- **更新时间**: 2026-11-18 00:40

---

## 0. 一句话

让 AI 辅助开发能**直接产出符合平台约定的代码**，而不是把底座已有的东西重写一遍。

关键认知：**提示词解决「怎么写」，能力索引解决「不要重写」。** 只给提示词是不够的。

---

## 1. 三层引导文件

| 层 | 位置 | 面向 | 谁维护 |
|---|---|---|---|
| 仓库级 | `AGENTS.md` | AI 助手进入仓库时读到的全局约定 | BA 拍板，Dev 落地 |
| 模块级 | `modules/<name>/AGENTS.md` | 该模块的领域上下文 | 模块开发者 |
| 任务级 | `prompts/*.md` | 按任务成套的提示词模板 | BA 维护 |

### 1.1 仓库级 AGENTS.md 骨架

遵循 **agents.md 开放规范**（同一份内容可被 Claude Code / Codex / Cursor / mcode 等消费，
不为每个客户端各写一套）。

```markdown
# Base Platform — AI 开发引导

## 这是什么
通用管理系统底座。业务方在此之上开发自己的业务模块。

## 目录
crates/platform-core/        平台核心（**禁止修改**，改动需人工评审）
crates/platform-contracts/   扩展点契约测试套件
crates/xtask/                任务运行器（cargo xtask）
modules/<name>/              业务模块（在这里写业务代码）
prompts/                     按任务的提示词模板
capabilities/                机器可读能力索引（自动生成，**勿手改**）

## 硬规则
1. 不得修改 crates/platform-core/ 与 crates/platform-contracts/；需要新能力 → 提扩展点需求
2. 业务模块只依赖扩展点 trait，不反向依赖平台内部模块
3. 数据访问必须走 RequestContext，不要手写 tenant_id 过滤
4. 权限点必须在 manifest.json 声明，不要硬编码角色名
5. 错误响应只返回错误码 + 参数，不要拼面向用户的中文句子
6. 时间存 UTC，金额用最小单位整数
7. 新增模块用 `cargo xtask create-module`，不要手建目录

## 命令
cargo xtask create-module <name>   # 新建业务模块
cargo xtask capabilities            # 重新生成能力索引
cargo xtask check-modules          # 校验模块目录规范
bun run dev                         # 起全栈
cargo check                         # 日常主力检查命令
bun run test                        # 单测 + 契约测试

## 已有能力（先查 capabilities/index.json，别重写）
分页、权限判定、表单渲染、审计埋点、租户过滤、通知发送、工作流发起、配置下发
```

### 1.2 模块级 AGENTS.md 模板

```markdown
# 模块：<名称>

## 领域模型
<实体与关键字段，指向 backend/entity.rs>

## 用到的平台扩展点
- <Trait 名>：<用途>

## 本模块权限点
<module:resource:action，逐条列出>

## 如何验证
cargo test -p <crate>
<e2e 命令>

## 注意
<易踩的坑，2-5 条>
```

由 `cargo xtask create-module` 生成骨架，开发者填内容。

---

## 2. 提示词库

**每份是成套任务书**，包含目标、上下文、约束、验收，可直接粘贴使用。
**不是零散片段** —— AI 需要的是完整上下文，不是单句指令。

```text
prompts/
├── 01-new-crud-module.md        新建一个 CRUD 业务模块
├── 02-add-permission.md         新增权限点并接入前端显隐
├── 03-new-sso-protocol.md       接入一种新的 SSO 协议
├── 04-define-workflow.md        定义一个审批流程
├── 05-notification-template.md  新增通知模板与投递渠道
├── 06-authorization-test.md     编写越权测试用例
├── 07-write-test-cases.md       补测试用例
└── 08-performance-optimization.md 做性能优化
```

### 2.1 提示词模板的统一结构

```markdown
# 任务：<一句话>

## 输入
- <变量占位>

## 步骤
1. 读 AGENTS.md 与 capabilities/index.json —— **先确认哪些能力平台已有，不要重写**
2. <具体步骤>

## 约束
- 不修改 crates/platform-core/
- 权限点必须在 manifest.json 声明
- <本任务特有的约束>

## 验收
- [ ] <可判定的条件>
```

### 2.2 `01-new-crud-module.md` 的步骤（示例）

```markdown
1. 读 AGENTS.md 与 capabilities/index.json —— 先确认哪些能力平台已有，不要重写
2. 运行 `cargo xtask create-module <name>` 生成骨架
3. 实现 backend：entity / repository / service，repository 必须走 RequestContext
4. 在 manifest.json 声明权限点与菜单，不要硬编码角色名
5. 实现 frontend：优先用 @base/schema 渲染器；只有特殊布局才写自定义组件
6. 接入 tests/contracts.rs 契约测试
7. 更新 modules/<name>/AGENTS.md
8. 运行 `cargo test -p <name>` 与 `bun run lint`

## 验收
- [ ] 契约测试通过
- [ ] 未修改 crates/platform-core/
- [ ] 权限点全部在 manifest 声明
- [ ] cargo xtask capabilities 输出已包含本模块
```

---

## 3. 能力索引（防重造的核心）

### 3.1 目录结构

```text
capabilities/
├── index.json                  总索引，AI 的入口
├── permissions.json            全量权限点（所有模块 manifest 汇总）
├── extension-points.json       扩展点 trait 签名
├── schema/
│   ├── manifest.schema.json    模块 manifest 的 JSON Schema
│   └── metadata.schema.json    元数据驱动表单/列表的 JSON Schema
├── sdk/
│   ├── ui.json                 @base/ui 组件清单 + 用法示例
│   ├── sdk.json                @base/sdk API 用法示例
│   ├── auth.json               can() / 路由守卫用法
│   └── schema.json             @base/schema 渲染器用法
└── api.json                    OpenAPI 接口清单
```

### 3.2 index.json 的形态

```json
{
  "generated_at": "2026-11-18T00:40:00+08:00",
  "platform": { "version": "0.1.0", "modules": ["iam", "rbac", "tenant", "sandbox"] },
  "already_provided": {
    "pagination": "@base/ui Table 内置分页，勿自行实现",
    "permission_check": "@base/auth can()，勿自行判断角色",
    "crud_form": "@base/schema 渲染器，按后端元数据渲染，勿手写表单",
    "audit": "平台自动埋点，勿自行写审计表",
    "tenant_filter": "平台中间件自动注入 tenant_id，勿手工加过滤条件",
    "error_message": "后端只返回错误码 + 参数，文案由 @base/i18n 渲染",
    "idempotency": "POST/PUT/DELETE 支持 Idempotency-Key，勿自行实现去重"
  },
  "entry_points": {
    "repo": "crates/platform-core/src/extension/",
    "contracts": "crates/platform-contracts/",
    "xtask": "crates/xtask/src/cmd/"
  }
}
```

`already_provided` 是**防重造的核心** —— 直接告诉 AI 哪些东西底座已经有了。

### 3.3 生成机制（强制自动生成）

```bash
cargo xtask capabilities          # 生成到 capabilities/
cargo xtask capabilities --check  # 只校验，不写入（CI 用）
```

**数据来源**：

| 索引 | 来源 |
|---|---|
| `permissions.json` | 扫描所有 `modules/*/manifest.json` 的 `permissions[].code` |
| `extension-points.json` | 解析 `crates/platform-core/src/extension/` 的 trait 定义 |
| `schema/*.json` | 平台内置的 JSON Schema 常量 |
| `sdk/*.json` | 各 `packages/*/package.json` 的导出清单 + 用法示例 |
| `api.json` | 生成的 OpenAPI 文档 |

**禁止手改**：`capabilities/` 在 `.gitignore` 里只忽略**生成物**，
生成物入库（这样 CI 与其他人拿到的是同一份），但 CI 用 `--check` 比对，
手改就会被发现。

> 关键：**过期的索引比没有索引更危险** —— AI 会严格按一份过时清单办事，
> 错得比没有依据还理直气壮。所以索引必须自动生成 + CI 强制校验。

---

## 4. `cargo xtask create-module`

生成内容（与 REQ-003 的目录规范、REQ-002 的 xtask 骨架对齐）：

```text
modules/<name>/
├── manifest.json         # 从模板填入模块名，permissions 留空数组
├── backend/
│   ├── mod.rs            # impl PlatformModule（骨架）
│   ├── entity.rs
│   ├── repository.rs     # impl Repository（骨架，注释说明必须走 ctx）
│   ├── service.rs
│   └── permission.rs     # 权限点常量
├── frontend/
│   ├── routes.ts
│   └── pages/
├── migrations/
├── tests/
│   └── contracts.rs      # 接入 platform-contracts
├── AGENTS.md             # 模板填好模块名
└── README.md             # 模板
```

参数：`cargo xtask create-module <name> [--perm-prefix <prefix>]`

---

## 5. 护栏

| 护栏 | 实现 |
|---|---|
| 同一套 CI | AI 与人工代码走完全相同的 lint / test / 契约检查 |
| 核心修改标记 | `crates/platform-core/`、`crates/platform-contracts/` 变更在 PR 中自动打标，强制人工评审 |
| 索引一致性 | CI 跑 `cargo xtask capabilities --check`，不一致即失败 |
| 依赖方向单向 | CI 检查平台 crate 的依赖图不含业务模块路径 |
| 越权测试 | 每个新权限点自动生成一条越权用例进 CI |

---

## 6. 本期交付边界

**做**：

- [ ] 仓库级 `AGENTS.md`
- [ ] `prompts/` 8 套提示词
- [ ] `capabilities/` 目录结构 + `index.json`（含 `already_provided`）
- [ ] `cargo xtask capabilities` 生成器 + `--check` 模式
- [ ] `cargo xtask create-module` 完整实现
- [ ] 模块级 AGENTS.md 模板
- [ ] CI：索引一致性校验 + 核心改动打标 + 依赖方向检查

**不做**：

- 多端规则文件转换脚本（先只出 AGENTS.md，客户端差异后续处理）
- AI 行为的自动化评测（AC-038 用静态扫描 + 人工确认判定）
- `prompts/` 的第 8 套以后扩展

---

## 7. 验收对应

| AC | 判定方式 |
|---|---|
| AC-035 引导文件完整且与实际一致 | `cargo xtask capabilities --check` 通过；人为篡改索引后 CI 失败 |
| AC-036 提示词可直接产出合规代码 | 拿 `01-new-crud-module.md` 给 AI，产出代码**一次通过 CI**，无需人工重排结构 |
| AC-037 脚手架开箱即用 | `cargo xtask create-module demo` 产出立即可编译运行 |
| AC-038 能力索引防重造 | 让 AI 实现「订单列表页」，静态扫描确认复用了 `@base/ui` 与 `@base/schema`，未重复实现分页/表格/权限判定 |
| AC-S-004 索引不可手改 | 手改 `capabilities/index.json` 后 CI 失败 |

---

## 8. 资源约束提醒

`cargo xtask capabilities` 需要扫描全部模块 manifest 与解析 trait ——
在 20+ 模块规模下是秒级操作，不构成负担。**不要**为了生成索引引入代码解析框架
（如 `syn` 全量解析），用正则 + 简单的 trait 声明约定即可，保持依赖轻。
