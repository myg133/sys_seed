# 设计摘要：二次开发体验与 AI 辅助开发（G 域）

- **需求编号**: REQ-001
- **关联**: [demand.md](demand.md) §G、[acceptance.md](acceptance.md) AC-031 ~ AC-039、[ADR-0001](../../decisions/ADR-0001-底座技术架构基线.md) D7 / D8
- **性质**: 需求细化附件 —— 把 G 域的意图落成具体形态，避免实现时各自理解
- **更新时间**: 2026-11-17 23:40

---

## 1. 业务模块目录模板

`create-module` 按此模板生成，人和 AI 产出同一形态：

```text
modules/<module-name>/
├── manifest.json         # 模块注册信息（见 §2）
├── backend/
│   ├── mod.rs            # 实现平台定义的 trait，编译期自动注册
│   ├── entity.rs         # 领域模型
│   ├── repository.rs     # 数据访问（必须走平台租户上下文）
│   ├── service.rs        # 业务逻辑
│   └── permission.rs     # 权限点常量（与 manifest 对应）
├── frontend/
│   ├── routes.ts         # 路由注册
│   ├── pages/            # 页面
│   └── components/
├── migrations/           # 本模块的数据库迁移（只动本模块的表）
├── tests/
│   └── contracts.rs      # 接入平台契约测试套件
├── AGENTS.md             # 模块级 AI 引导（见 §4）
└── README.md             # 人读：模块做什么、怎么跑、依赖什么
```

### manifest.json

```json
{
  "id": "order",
  "name": "订单",
  "version": "0.1.0",
  "enabled": true,
  "permissions": [
    { "code": "order:invoice:read",  "name": "查看订单", "type": "menu" },
    { "code": "order:invoice:write", "name": "编辑订单", "type": "button" }
  ],
  "menus": [
    { "label": "订单", "icon": "receipt", "route": "/order/invoice", "perm": "order:invoice:read" }
  ],
  "dependencies": [],
  "config": { "itemsPerPage": { "type": "number", "default": 20, "scope": "tenant" } }
}
```

- `permissions` → 汇总进平台权限点注册表，前端 SDK 自动生成 `can()` 判定与 UI 显隐（AC-001）
- `menus` → 平台据此生成导航；`perm` 字段决定可见性
- `config` → 平台据此生成配置界面，业务方不用写配置页

---

## 2. 扩展点契约测试套件

平台分发 `platform-contracts` crate，业务模块在 `tests/contracts.rs` 接入：

```rust
use platform_contracts::prelude::*;

#[platform_module]
mod order {
    // 套件自动校验以下项，违规则测试失败：
    //   - 数据访问是否经过租户上下文（漏 tenant_id 会失败）
    //   - 声明的权限点是否在 manifest 中登记
    //   - 是否实现了契约要求的全部方法
    //   - 事务边界是否符合约定
}
```

**价值**：业务方不必读完扩展点文档才知道要做什么 —— 契约测试就是可执行的文档。
AC-033 专门验证「故意漏掉租户上下文」能被拦截。

---

## 3. `capabilities/` 机器可读能力索引

**由 `xtask capabilities` 自动生成，禁止手改**（CI 校验其与源码一致）。

```text
capabilities/
├── index.json            # 总索引：AI 的入口
├── permissions.json      # 全量权限点（来自所有模块 manifest 汇总）
├── extension-points.json # 扩展点 trait 签名
├── schema/
│   └── metadata.schema.json   # 元数据驱动的表单/列表 JSON Schema
├── sdk/
│   ├── ui.json           # @base/ui 组件清单 + 用法示例
│   ├── sdk.json          # @base/sdk API 用法示例
│   ├── auth.json         # can() / 路由守卫用法
│   └── schema.json       # @base/schema 渲染器用法
└── api.json              # OpenAPI 接口清单
```

### index.json 的样子

```json
{
  "generated_at": "2026-11-17T23:40:00+08:00",
  "platform": { "version": "0.1.0", "modules": ["iam", "rbac", "tenant", "order"] },
  "already_provided": {
    "pagination": "@base/ui Table 内置分页，勿自行实现",
    "permission_check": "@base/auth can()，勿自行判断角色",
    "crud_form": "@base/schema 渲染器，按后端元数据渲染，勿手写表单",
    "audit": "平台自动埋点，勿自行写审计表",
    "tenant_filter": "平台中间件自动注入 tenant_id，勿手工加过滤条件"
  }
}
```

`already_provided` 是**防重造的核心** —— 直接告诉 AI 哪些东西底座已经有了。
AC-038 验证 AI 做「订单列表页」时会不会重造分页、表格、权限判定。

---

## 4. 三层引导文件

### 4.1 仓库级 `AGENTS.md`（放在平台仓库根）

```markdown
# Base Platform — AI 开发引导

## 这是什么
通用管理系统底座。业务方在此之上开发自己的业务模块。

## 目录
crates/platform-core/   平台核心（**禁止修改**，改动需人工评审）
crates/platform-contracts/ 扩展点契约测试套件
modules/<name>/        业务模块（在这里写业务代码）
prompts/               按任务的提示词模板
capabilities/          机器可读能力索引（自动生成，勿手改）

## 硬规则
1. 不得修改 crates/platform-core/；需要新能力 → 提扩展点需求
2. 业务模块只依赖扩展点 trait，不反向依赖平台内部模块
3. 数据访问必须走平台租户上下文，不要手写 tenant_id 过滤
4. 权限点必须在 manifest.json 声明，不要硬编码角色名
5. 新增模块用 `cargo xtask create-module`，不要手建目录

## 命令
cargo xtask create-module      # 新建业务模块
cargo xtask capabilities       # 重新生成能力索引
bun run dev                     # 起全栈（前后端 + PG + Redis + 种子数据）
cargo test --workspace          # 全量测试（含契约测试）

## 已有能力（先查 capabilities/，别重写）
分页、权限判定、表单渲染、审计、租户过滤、通知发送、工作流发起
```

### 4.2 模块级 `modules/<name>/AGENTS.md`

```markdown
# 模块：订单

## 领域模型
Order / OrderItem / Invoice（字段与状态机见 backend/entity.rs）

## 用到的平台扩展点
- `OrderRepository`：数据访问（已实现租户上下文）
- `Auditable`：自动审计埋点

## 本模块权限点
order:invoice:read / order:invoice:write / order:invoice:approve

## 如何验证
cargo test -p order
bun run test:e2e -- order

## 注意
状态从 `draft` 变 `paid` 必须走 `WorkflowService::transition`，不要直接改字段
```

### 4.3 `prompts/` 提示词库

每份提示词是**成套任务书**，包含目标、上下文、约束、验收，可直接粘贴使用。

```text
prompts/
├── 01-new-crud-module.md
├── 02-add-permission.md
├── 03-new-sso-protocol.md
├── 04-define-workflow.md
├── 05-notification-template.md
├── 06-authorization-test.md
├── 07-write-test-cases.md
└── 08-performance-optimization.md
```

示例（`01-new-crud-module.md` 的结构）：

```markdown
# 任务：新建一个业务模块

## 输入
- 模块名：{name}
- 领域描述：{description}
- 核心实体与字段：{fields}

## 步骤
1. 读 AGENTS.md 与 capabilities/index.json —— 先确认哪些能力平台已有，不要重写
2. 运行 `cargo xtask create-module {name}` 生成骨架
3. 实现 backend：entity / repository / service，repository 必须走平台租户上下文
4. 在 manifest.json 声明权限点与菜单，不要硬编码角色名
5. 实现 frontend：优先用 @base/schema 渲染器；只有特殊布局才写自定义组件
6. 接入 tests/contracts.rs 契约测试
7. 更新 modules/{name}/AGENTS.md
8. 运行 `cargo test -p {name}` 与 `bun run lint`

## 验收
- [ ] 契约测试通过
- [ ] 未修改 crates/platform-core/
- [ ] 权限点全部在 manifest 声明
- [ ] cargo xtask capabilities 输出已包含本模块
```

---

## 5. AI 生成代码的护栏

| 护栏 | 实现 |
|---|---|
| 同一套 CI | AI 与人工代码走完全相同的 lint / test / 契约检查 |
| 核心修改标记 | `crates/platform-core/` 变更在 PR 中自动打标，强制人工评审 |
| 索引一致性 | CI 校验 `capabilities/` 与 manifest / trait / OpenAPI 一致，索引过期即失败 |
| 越权测试 | 每个新权限点自动生成一条越权用例进 CI |

**为什么索引必须自动生成**：手维护的能力索引一定会过期，而**过期的索引比没有索引更危险** ——
AI 会严格按一份过时的清单去做事，错得比没有依据还理直气壮。

---

## 6. 开发效率度量（用于验收与持续改进）

| 指标 | 目标 | 测量方式 |
|---|---|---|
| 标准 CRUD 模块业务代码量 | < 500 行 | 首个消费方项目统计 |
| 对平台核心的修改次数 | 0 | PR 审查统计 |
| 平台升级导致业务改动次数 | 0（同大版本内） | 升级记录 |
| 新工程师上手时间 | 1 天跑起环境 / 1 周交付首个模块 | 团队实测 |
| AI 产出一次通过 CI 比例 | > 80% | CI 记录统计 |

这些指标进 AC-029 / AC-030，由「第一个真实消费方」项目验收。
