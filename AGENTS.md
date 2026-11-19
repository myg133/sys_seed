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
能力索引、**租户作用域数据访问层**（`platform-db`）、**认证**（`platform-iam`）、
**组织架构**（`platform-org`）、**角色与权限**（`platform-rbac` + `require_permission`）。
通用 CRUD 脚手架、前端组件库、异步任务与文件存储都还没落地（见「尚未提供」）。

## 目录

```text
crates/platform-core/        平台核心（**禁止修改**，改动需人工评审）
crates/platform-config/       配置、品牌与 Feature Flag（/api/config/*、/api/feature-flags/*）
crates/platform-contracts/   扩展点契约测试套件（**禁止修改**）
crates/platform-contracts-macros/  契约测试宏（**禁止修改**）
crates/platform-db/           平台数据访问层（租户作用域入口 + RLS 兜底）
crates/platform-iam/          认证与账号体系（/api/iam/*）
crates/platform-org/          用户与组织架构（/api/org/*）
crates/platform-rbac/         角色与权限（/api/rbac/*、数据范围求值）
crates/platform-server/       交付基线服务（base serve / migrate / tenant-policies）
crates/xtask/                任务运行器（cargo xtask）
modules/<name>/              业务模块（在这里写业务代码）
packages/<name>/             前端占位包（@base/ui、@base/sdk、@base/auth …）
apps/shell/                  前端应用外壳
prompts/                     按任务的提示词模板
capabilities/                机器可读能力索引（自动生成，**勿手改**）
docs/contracts/              契约正文快照（**勿手改**，正本在 BA 分支）
```

## 硬规则

1. **不得修改 `crates/platform-core/` 与 `crates/platform-contracts/`**；
   需要新能力 → 提扩展点需求，不要就地扩平台。
   - **例外（仅限平台自身在已评审需求下的演进）**：平台 crate 可以在**已评审的需求**里演进
     （例如 REQ-009 在 `platform-core` 里新增了 `extension/permission.rs`：
     `PermissionChecker` / `DataScope` / `ScopeFilter` / `require_permission`）。
     规则的意图是「**业务方 / AI 不许就地扩平台**」，不是「平台自己不许演进」。
     走这条例外时：需求必须已评审，且要同步改本文件（引导文件必须与实际一致）。
2. **业务模块只依赖扩展点 trait**，不反向依赖平台内部模块；
   `platform-contracts` 只能出现在 `[dev-dependencies]`。
   - 鉴权能力走 `platform-core` 的 `require_permission` 与 `platform-db` 的
     `TenantQuery::with_scope` —— **不要**依赖 `platform-rbac`（那是平台内部模块）。
3. **数据访问必须走 `RequestContext`**，不要手写 `tenant_id` 过滤条件
   （租户只能从 `ctx` 取，漏传就编译不过）。
4. **数据访问必须走 `platform-db` 的租户作用域入口**（`TenantDb`），
   **不得直连连接池、不得自己建连接池、不得绕过 `{tenant}` 占位符** ——
   `platform-db` 的公开 API 拿不到裸连接，就是为了让这件事在编译期做不到。
   租户作用域表在 `manifest.json` 的 `tenantScoped` 段声明，
   **RLS 策略由平台统一生成**（开发期 `cargo xtask tenant-policies`，
   部署期 `base tenant-policies` —— 镜像里没有 cargo，但策略必须能落地），
   模块不手写 policy。
   **数据范围过滤也由平台叠加**（`TenantQuery::with_scope`）——
   业务模块不实现范围判定，平台**也没有**「跳过范围」的开关。
5. **权限点必须在 `manifest.json` 声明**，不要硬编码角色名；
   编码固定为 `module:resource:action`（`api-conventions v1` C4）。
   **每个已登记的权限点都必须有真实的鉴权调用点**
   （`require_permission("<字面量>")`）—— 契约检查 C-5 会核对；
   只定义常量、从不鉴权 = **假权限**。
6. **错误响应只返回错误码 + 参数**，不要拼面向用户的中文句子
   （文案由前端 `@base/i18n` 渲染，`api-conventions v1` C6）。
7. **时间存 UTC，金额用最小单位整数**（`api-conventions v1` C13）。
8. **新增模块用 `cargo xtask create-module`**，不要手建目录。
9. **验收命令必须真的跑到用例**：`cargo test` 的参数分隔符 `--` **只写一次**。
   多写一个，libtest 会把后面的 flag 当成「用例名过滤串」的一部分，
   结果是**一个用例都没跑、但退出码仍是 0** —— 假绿，验收形同虚设。
   - ✅ `cargo test -p <模块名> -- --test-threads=2 --nocapture`
   - ❌ `cargo test -p <模块名> -- --test-threads=2 -- --nocapture`
   验收前先看输出里的「N passed」，**N 必须 > 0**。

> 规则 1 与 2 由 CI 机械检查：`cargo xtask check-deps` 扫 `cargo metadata` 的依赖图，
> 平台 crate 一旦反向依赖业务模块即失败。

## 配置怎么读（REQ-010 起）

- 业务模块用 `platform_core::extension::config::ConfigResolver`（`kind = "platform-capability"`，
  **不是扩展点**；`extension_point_count` 仍是 5）。实现是 `platform-config` 的 `DbConfigResolver`。
- 同步方法只读**已刷新的快照**；平台在调用前 `refresh`（`(platform_epoch, tenant_epoch)` **成对比较**，
  不要用 `max(epoch)` —— 会被更高的租户 epoch 掩蔽）。
- 写配置只能走 `/api/config/entries`（租户/空间）与 `/api/config/platform-entries`（平台级，独立权限点）；
  **Feature Flag 只能经 `/api/feature-flags/{flag}` 改**，通用写路由收到 flag 名 / `flags.` 前缀 → 400（D70）。
- 4 个权限点：`config:entry:write` / `config:flag:write` / `config:platform:write` / `config:platform:read`
  （manifest + 权限点种子迁移 + 重生成 `capabilities/` 三件套，缺一启动对账 fail-fast）。

## 鉴权怎么写（REQ-009 起）

鉴权入口**只有一个**：`platform_core::extension::permission::require_permission`。

```rust
use platform_core::extension::permission::require_permission;

// 一组路由一个权限点（`route_layer` 绑在子路由上）。
let read = Router::new().route("/item", get(list_items));
let write = Router::new().route("/item", post(create_item));

Router::new()
    .merge(read.route_layer(require_permission("order:item:read")))
    .merge(write.route_layer(require_permission("order:item:write")))
```

- ⚠️ **参数必须是字面量**（不是常量）：契约检查 C-5 要能**静态确认**
  「这个权限点真的有鉴权调用点」；参数是常量就没法确认 → 判为**失败**
  （无法确认 = 不放行，fail closed）。
- 判定实现由**平台**通过请求扩展注入（`Arc<dyn PermissionChecker>`），
  业务模块**不实现**它、也不依赖 `platform-rbac`。
- 数据范围：向平台要 `ScopeFilter`，交给 `TenantQuery::with_scope`
  （条件在 **SQL 层**生效，值走参数绑定）。
- 三种拒绝语义（契约 C5）：未认证 **401 `UNAUTHENTICATED`**；
  缺权限点 **403 `INSUFFICIENT_PERMISSION`**；
  记录不在数据范围内 **404 `NOT_FOUND`**（不泄露资源存在性）。

### 「我有哪些权限点」（REQ-011 起）

`platform_core::extension::permission::PermissionProvider` 是**平台能力**
（`kind = "platform-capability"`，**不是扩展点**；`extension_point_count` 仍是 5）。
实现方是 `platform-rbac`（`RbacService`），装配由 `platform-server` 注入。

- 它给的是**权限点全集**（已展开通配、升序去重），
  与 `PermissionChecker` 的「判一个权限点」是**两件事** —— 后者方法全同步，
  逐个 `has()` 是 N 次判定，且拿不到「全集」语义。
- `platform-iam` **不能**依赖 `platform-rbac`（依赖方向是 rbac → iam，反过来成环），
  所以登录响应里的 `permissions` 靠**这个 trait** 注入，**未注入时是空数组**、
  登录**仍然成功**（fail closed 的是「判定」，不是「登录」）。
- 业务模块**不需要**它 —— 「当前用户有哪些权限点」是前端 SDK 的事
  （`GET /api/rbac/me/permissions`）。


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

# 租户作用域的 RLS 策略：按 manifest 的 tenantScoped 段统一生成 / 校验
DATABASE_URL=postgres://... cargo xtask tenant-policies
DATABASE_URL=postgres://... cargo xtask tenant-policies --check

# 契约快照（正本在 BA 分支 demand）：校验 / 同步
./scripts/sync-contracts.sh            # 校验：快照与 BA 正本不一致则失败
./scripts/sync-contracts.sh --sync     # 同步：用 BA 正本更新 docs/contracts/

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

**先查 `capabilities/index.json`，别重写。** 下表与索引的 `already_provided` 段
**内容完全一致**（27 条，key 与说明逐条相同）；**行的顺序不同** ——
下表按「后端接缝 → 前端包」的语义序，索引按字典序输出。
机器守卫 `cargo test -p xtask` 的 `agents_doc_matches_catalog` 比对的是下表与能力目录
（同序），不覆盖索引的排序。

| 能力 | 说明 |
|---|---|
| `request_context` | RequestContext 携带租户/空间/追踪 ID；Repository 每个方法强制传 ctx，租户隔离是编译期保证，勿自建租户上下文 |
| `repository` | Repository<T> 提供 find/list/insert/update/delete，业务模块实现它即可，勿另造 DAO 基类或数据访问框架 |
| `tenant_scoped_db` | platform-db 的 TenantDb 提供带 RequestContext 的租户作用域数据访问（事务内 SET LOCAL app.tenant_id + RLS 兜底），公开 API 拿不到裸连接，勿自建连接池或直连数据库 |
| `list_query` | ListQuery/SortSpec/FilterSpec 已定义游标分页（limit 默认 20、上限 200）与过滤算子白名单，勿自造分页参数 |
| `module_registry` | register_platform_module! + discover()/discover_enabled() 做编译期模块注册，勿自建插件或动态加载 |
| `manifest` | Manifest 解析与字段校验（含 schema 实体元数据段：两阶段校验、字段类型只有 text/number/boolean/date/reference）+ capabilities/schema/manifest.schema.json 与 schema/entity.schema.json 已提供，勿手写 manifest 校验 |
| `contract_suite` | #[platform_module_contract] 自动生成 C-1~C-6 六项契约测试（C-5：已登记权限点必须有鉴权调用点；C-6：声明了字段级权限的模块必须有 SchemaProjector::project 调用点），勿自写重复的契约检查 |
| `schema_metadata` | platform-schema 提供 GET /api/schema/<module>/<entity> 元数据接口（已认证即可读；无权字段在元数据里就被剔除，不是 hidden 标记），前端据此渲染，勿自造元数据接口或另立形状 |
| `schema_projector` | SchemaProjector::project 是字段级权限的唯一序列化入口（白名单语义：元数据未声明的字段默认不输出，列表要逐条投影、信封不动），返回实体的 handler 必须经它，勿直接 Json(entity) |
| `audit_declaration` | Auditable + AuditEventSpec 提供审计事件声明面，平台据此自动埋点，勿自建审计表 |
| `notification_sender` | NotificationSender + NotificationMessage/DeliveryResult 提供通知投递接缝，勿自建消息中心 |
| `workflow_handler` | WorkflowHandler + TransitionEvent 提供工作流接入接缝，勿自建流程引擎 |
| `transaction_boundary` | TransactionalWrite + WriteBoundary 提供写边界声明，跨表写必须显式开平台事务（C-4 判定依据） |
| `config` | ConfigScope/ConfigResolver（platform-capability，不是扩展点）/AppConfig + manifest 的 config 段提供三层配置与 Feature Flag（4 个权限点 config:entry:write / config:flag:write / config:platform:write / config:platform:read），勿自建配置读取 |
| `error_code_i18n` | @base/i18n 的 translate()/hasMessage() 提供错误码到文案映射，勿在后端拼中文句子 |
| `theme` | @base/theme 的 applyTheme()/toggleTheme() 提供 CSS 变量令牌与暗色模式，勿另造主题变量或硬编码颜色 |
| `ui_token` | @base/ui 的 toneClass()/TONES/defaultRadius() 提供色调与圆角 token，勿另造样式变量 |
| `ui_table_component` | @base/ui 的 Table 契约与 apps/shell 的 Table.svelte 提供只读表格（只接受 props，分页状态由 @base/hooks 组合传入），勿自造表格渲染 |
| `schema_field_meta` | @base/schema 的 FieldMeta/describeField()/findField() 提供字段元数据形状，勿另造元数据结构 |
| `sdk_client` | @base/sdk 的 createApiClient() 提供类型安全请求、RFC 9457 错误码、token 自动刷新与幂等键注入，勿另造 HTTP 基类 |
| `auth_state` | @base/auth 的 isAuthenticated()/tenantId() 提供登录态形状，勿另造登录态模型 |
| `permission_can` | @base/auth 的 can()/guard() 提供权限点判定与路由守卫（权限点来自 GET /api/rbac/me/permissions），前端别自己写一套；服务端鉴权入口仍是 require_permission |
| `hooks_toggle` | @base/hooks 的 createToggle()/createPagination()/createTableState() 提供通用状态容器，勿另造状态容器 |
| `iam_auth` | platform-iam 提供 /api/iam/* 认证接口（本地账号 + JWT access / 不透明 refresh + Argon2id + TOTP）与 /.well-known/jwks.json；业务模块要鉴权请用这些接口，勿自造认证 |
| `org_structure` | platform-org 提供 /api/org/* 组织架构接口（组织树用邻接表 + 递归 CTE、用户↔节点成员关系、岗位与汇报关系）、SCIM 2.0 出站与 LDAP/AD 入站、字段级同步冲突队列与人工裁决；数据范围的输入是它，勿自造组织树 |
| `rbac_authorization` | platform-core 的 require_permission 是平台唯一鉴权入口，platform-rbac 提供 /api/rbac/* 角色授权接口、5 种数据范围与 DB epoch 主动失效；业务模块用 require_permission + TenantQuery::with_scope，勿自写鉴权或数据范围 |

## 尚未提供

**别当成已有。** 下表与索引的 `not_yet_provided` 段**内容完全一致**（5 条，
key 与说明逐条相同）；行的顺序同样与索引不同（语义序 vs 字典序）。

| 能力 | 说明 |
|---|---|
| `http_api_layer` | HTTP 接口层尚未就绪（REQ-011/012 补），capabilities/api.json 目前为空 |
| `crud_scaffolding` | 平台没有通用 CRUD/分页脚手架（模块的仓储要自己实现），但数据访问必须走 platform-db 的租户作用域入口 |
| `audit_sink` | 审计只到事件声明面，落库与查询待 REQ-014 |
| `notification_dispatch` | 通知只到 trait，通道投递与模板待 REQ-015 |
| `workflow_engine` | 工作流只到 trait，流程定义与实例待 REQ-017 |

## 新增一个业务模块

1. `cargo xtask create-module <name>` —— 生成骨架并登记进工作区成员。
2. 实现 `backend/`：entity / repository / service，repository 必须走 `RequestContext`；
   落库的实现走 `platform-db` 的 `TenantDb`（租户作用域入口），语句里必须带 `{tenant}`。
3. 在 `manifest.json` 声明权限点与菜单，不要硬编码角色名；
   有租户作用域表就在 `tenantScoped` 段声明（平台据此生成 RLS 策略，模块不手写 policy）。
4. 实现 `frontend/`：优先用 `@base/ui` 与 `@base/schema`，只有特殊布局才写自定义组件。
5. 接入 `tests/contracts.rs` 的契约测试（骨架已生成，直接填业务实体）。
6. 更新 `modules/<name>/AGENTS.md`（领域模型、扩展点、权限点）。
7. `cargo xtask capabilities` —— 让能力索引包含本模块。
8. `cargo xtask tenant-policies` —— 让平台按 manifest 生成该模块的 RLS 策略。

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
| API 约定（路径、错误码、分页、命名） | 快照 `docs/contracts/api-conventions/v1.md`；正本 `git show demand:contracts/api-conventions/v1.md` |
| 扩展点契约（5 个 trait 的冻结签名 + C-5 鉴权调用点检查） | 快照 `docs/contracts/extension-points/v2.md`；正本 `git show demand:contracts/extension-points/v2.md`（v1 保留一个 Sprint） |
| 示例模块（扩展点实现 + 鉴权接入的标准范例） | `modules/sandbox/`（`backend/src/http.rs` 是鉴权与数据范围的参考实现） |
| 能力索引 | `capabilities/index.json` |

> **契约正本在 BA 分支，不在代码仓库里** —— `BA/` 是 workspace 里 `demand` 分支的 worktree，
> 契约正文在它的 `contracts/` 下。上面「正本」列的 `git show demand:...` 就是同一个文件，
> 本地没有 `demand` 分支时先 `git fetch origin demand`。
>
> `docs/contracts/` 里的是**随版本发布的快照**（与正本逐字节相同），给只拿到代码仓库的人用。
> 它由 `scripts/sync-contracts.sh --sync` 从正本生成、由 `scripts/sync-contracts.sh` 校验，
> CI 强制执行 —— **不要手改**，改了契约请改 BA 分支上的正本再同步。
