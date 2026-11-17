# Base Platform — AI 开发引导

> 这份文件遵循 [agents.md 开放规范](https://agents.md)：同一份内容可被 Claude Code /
> Codex / Cursor / mcode 等客户端消费，不为每个客户端各写一套。
>
> 配套的三层引导文件：
>
> | 层 | 位置 | 作用 |
> |---|---|---|
> | 仓库级 | 本文件 | 全局约定：架构、硬规则、可用命令、已有能力 |
> | 模块级 | `modules/<name>/AGENTS.md` | 单个业务模块的领域上下文 |
> | 任务级 | `prompts/*.md` | 按任务成套的提示词（直接粘贴使用） |

## 这是什么

通用管理系统底座。业务方在此之上开发自己的业务模块，**不修改底座本身**。

底座目前提供的是**骨架**：请求上下文、扩展点 trait、契约测试套件、目录规范校验、
能力索引。HTTP 层、DB 访问层、鉴权中间件、组件库都还没落地（见「尚未提供」）。

## 目录

```text
crates/platform-core/        平台核心（**禁止修改**，改动需人工评审）
crates/platform-contracts/   扩展点契约测试套件（**禁止修改**）
crates/platform-contracts-macros/  契约测试宏（**禁止修改**）
crates/xtask/                任务运行器（cargo xtask）
modules/<name>/              业务模块（在这里写业务代码）
packages/<name>/             前端占位包（@base/ui、@base/sdk、@base/auth …）
apps/shell/                  前端应用外壳
prompts/                     按任务的提示词模板
capabilities/                机器可读能力索引（自动生成，**勿手改**）
```

## 硬规则

1. **不得修改 `crates/platform-core/` 与 `crates/platform-contracts/`**；
   需要新能力 → 提扩展点需求，不要就地扩平台。
2. **业务模块只依赖扩展点 trait**，不反向依赖平台内部模块；
   `platform-contracts` 只能出现在 `[dev-dependencies]`。
3. **数据访问必须走 `RequestContext`**，不要手写 `tenant_id` 过滤条件
   （租户只能从 `ctx` 取，漏传就编译不过）。
4. **权限点必须在 `manifest.json` 声明**，不要硬编码角色名；
   编码固定为 `module:resource:action`（`api-conventions v1` C4）。
5. **错误响应只返回错误码 + 参数**，不要拼面向用户的中文句子
   （文案由前端 `@base/i18n` 渲染，`api-conventions v1` C6）。
6. **时间存 UTC，金额用最小单位整数**（`api-conventions v1` C13）。
7. **新增模块用 `cargo xtask create-module`**，不要手建目录。

> 规则 1 与 2 由 CI 机械检查：`cargo xtask check-deps` 扫 `cargo metadata` 的依赖图，
> 平台 crate 一旦反向依赖业务模块即失败。

## 命令

```bash
# 生成业务模块骨架（开箱即过 check-modules 与契约测试 C-1~C-4）
cargo xtask create-module <name>
cargo xtask create-module <name> --perm-prefix <prefix>

# 能力索引：生成 / 校验（CI 用 --check 比对，手改索引即失败）
cargo xtask capabilities
cargo xtask capabilities --check

# 校验模块目录规范 / 依赖方向
cargo xtask check-modules
cargo xtask check-deps

# 日常主力检查命令
cargo check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings

# 测试
cargo test --workspace -- --test-threads=2     # Rust 单测 + 契约测试
pnpm -r test                                    # 前端单测
bun run test:e2e                                # E2E（Playwright）

# 起全栈（数据库容器需已启动：bun run db:up）
bun run dev
```

## 已有能力

**先查 `capabilities/index.json`，别重写。** 下表与索引的 `already_provided` 段逐行一致
（由 `cargo test -p xtask` 的 `agents_doc_matches_catalog` 守住）。

| 能力 | 说明 |
|---|---|
| `request_context` | RequestContext 携带租户/空间/追踪 ID；Repository 每个方法强制传 ctx，租户隔离是编译期保证，勿自建租户上下文 |
| `repository` | Repository<T> 提供 find/list/insert/update/delete，业务模块实现它即可，勿另造 DAO 基类或数据访问框架 |
| `list_query` | ListQuery/SortSpec/FilterSpec 已定义游标分页（limit 默认 20、上限 200）与过滤算子白名单，勿自造分页参数 |
| `module_registry` | register_platform_module! + discover()/discover_enabled() 做编译期模块注册，勿自建插件或动态加载 |
| `manifest` | Manifest 解析与字段校验 + capabilities/schema/manifest.schema.json 已提供，勿手写 manifest 校验 |
| `contract_suite` | #[platform_module_contract] 自动生成 C-1~C-4 四项契约测试，勿自写重复的契约检查 |
| `audit_declaration` | Auditable + AuditEventSpec 提供审计事件声明面，平台据此自动埋点，勿自建审计表 |
| `notification_sender` | NotificationSender + NotificationMessage/DeliveryResult 提供通知投递接缝，勿自建消息中心 |
| `workflow_handler` | WorkflowHandler + TransitionEvent 提供工作流接入接缝，勿自建流程引擎 |
| `transaction_boundary` | TransactionalWrite + WriteBoundary 提供写边界声明，跨表写必须显式开平台事务（C-4 判定依据） |
| `config` | ConfigScope/ConfigResolver/AppConfig + manifest 的 config 段提供配置分层（platform/tenant/space），勿自建配置读取 |
| `error_code_i18n` | @base/i18n 的 translate()/hasMessage() 提供错误码到文案映射，勿在后端拼中文句子 |
| `theme` | @base/theme 的 defaultTheme()/mergeTheme() 提供主题 token，勿另造主题变量 |
| `ui_token` | @base/ui 的 toneClass()/TONES/defaultRadius() 提供色调与圆角 token，勿另造样式变量 |
| `schema_field_meta` | @base/schema 的 FieldMeta/describeField()/findField() 提供字段元数据形状，勿另造元数据结构 |
| `sdk_client` | @base/sdk 的 createApiClient() 提供 baseUrl 规范化与 URL 拼接，勿另造 HTTP 基类 |
| `auth_state` | @base/auth 的 isAuthenticated()/tenantId() 提供登录态形状，勿另造登录态模型 |
| `hooks_toggle` | @base/hooks 的 createToggle() 提供通用布尔开关状态容器，勿另造状态容器 |

## 尚未提供

**别当成已有。** 下表与索引的 `not_yet_provided` 段逐行一致。

| 能力 | 说明 |
|---|---|
| `ui_table_component` | @base/ui 尚无表格与分页组件，列表页要自己实现（REQ-012 补） |
| `permission_can` | 前端 can() 与服务端鉴权中间件尚未落地（REQ-009 补），权限判定别自己写一套 |
| `http_api_layer` | HTTP 接口层尚未就绪（REQ-011/012 补），capabilities/api.json 目前为空 |
| `db_access_layer` | 平台没有 DB 访问层（sqlx 推迟到 REQ-006/007），模块仓储当前是内存实现 |
| `audit_sink` | 审计只到事件声明面，落库与查询待 REQ-014 |
| `notification_dispatch` | 通知只到 trait，通道投递与模板待 REQ-015 |
| `workflow_engine` | 工作流只到 trait，流程定义与实例待 REQ-017 |

## 新增一个业务模块

1. `cargo xtask create-module <name>` —— 生成骨架并登记进工作区成员。
2. 实现 `backend/`：entity / repository / service，repository 必须走 `RequestContext`。
3. 在 `manifest.json` 声明权限点与菜单，不要硬编码角色名。
4. 实现 `frontend/`：优先用 `@base/ui` 与 `@base/schema`，只有特殊布局才写自定义组件。
5. 接入 `tests/contracts.rs` 的契约测试（骨架已生成，直接填业务实体）。
6. 更新 `modules/<name>/AGENTS.md`（领域模型、扩展点、权限点）。
7. `cargo xtask capabilities` —— 让能力索引包含本模块。

## 提示词

`prompts/` 下是 8 套成套的任务书，每套都含**目标 / 输入 / 步骤 / 约束 / 验收**：

| 文件 | 任务 |
|---|---|
| `prompts/01-new-crud-module.md` | 新建一个 CRUD 业务模块 |
| `prompts/02-add-permission.md` | 新增权限点并接入前端显隐 |
| `prompts/03-new-sso-protocol.md` | 接入一种新的 SSO 协议 |
| `prompts/04-define-workflow.md` | 定义一个审批流程 |
| `prompts/05-notification-template.md` | 新增通知模板与投递渠道 |
| `prompts/06-authorization-test.md` | 编写越权测试用例 |
| `prompts/07-write-test-cases.md` | 补测试用例 |
| `prompts/08-performance-optimization.md` | 做性能优化 |

用法：把整份提示词连同「输入」段填好的变量一起粘给 AI 即可 ——
**提示词解决「怎么写」，能力索引解决「不要重写」，两者要一起用**。

## 参考

| 文档 | 位置 |
|---|---|
| API 约定（路径、错误码、分页、命名） | `BA/contracts/api-conventions/v1.md` |
| 扩展点契约（5 个 trait 的冻结签名） | `BA/contracts/extension-points/v1.md` |
| 示例模块（扩展点实现的标准范例） | `modules/sandbox/` |
| 能力索引 | `capabilities/index.json` |
