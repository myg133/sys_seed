# REQ-001 通用项目底座 —— 测试用例总索引

- **需求编号**: REQ-001「通用项目底座（Base Platform）」
- **来源**: [demand.md](../demand.md)、[acceptance.md](../acceptance.md)、[design-summary.md](../design-summary.md)、[ADR-0001](../../decisions/ADR-0001-底座技术架构基线.md)
- **用途**: 在实现开始前把验收标准转成可执行用例，作为 Dev 的实现输入与 QA 的验证基线
- **状态**: 待实现（用例本身尚未执行，不得据此判定需求通过）

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **84** |
| 覆盖 AC | **39 / 39**（AC-001 ~ AC-039，无遗漏） |
| 优先级分布 | P0 = 60、P1 = 24、P2 = 0 |
| 自动化分布 | 可自动化 = 64、半自动 = 20、手工 = 0 |
| 用例文件 | 8 个（另本索引 1 个） |
| 每条用例字段 | 标题 / 关联 AC / 前置条件 / 测试步骤 / 预期结果 / 类型 / 优先级 / 自动化 / 设计方法（9 项全部齐备） |

> 全部 84 条用例的「关联 AC」字段均非空，且每条 AC 在矩阵中至少映射 1 条 TC；安全重点 AC（AC-002、AC-007、AC-010、AC-022、AC-023、AC-033、AC-038）均配置了正常路径 + 边界 + 异常路径共 2~3 条。

## 2. 文件清单

| 文件 | 能力域 | AC 范围 | 用例数 |
|---|---|---|---|
| [A-identity-access.md](A-identity-access.md) | 身份与访问 | AC-001 ~ AC-008 | 19 |
| [B-org-tenant.md](B-org-tenant.md) | 组织与租户 | AC-009 ~ AC-012 | 8 |
| [C-config-branding.md](C-config-branding.md) | 配置与品牌 | AC-013 ~ AC-015 | 6 |
| [D-frontend-sdk.md](D-frontend-sdk.md) | 前端底座 | AC-016 ~ AC-019 | 8 |
| [E-app-extension.md](E-app-extension.md) | 应用接入与扩展 | AC-020 ~ AC-021 | 4 |
| [F-platform-ops.md](F-platform-ops.md) | 平台运维与治理 | AC-022 ~ AC-028 | 16 |
| [G-secondary-dev-ai.md](G-secondary-dev-ai.md) | 二次开发体验与 AI 辅助开发 | AC-031 ~ AC-039 | 19 |
| [H-platform-usability.md](H-platform-usability.md) | 底座可用性（需真实消费方） | AC-029 ~ AC-030 | 4 |

---

## 3. 测试夹具（造数方式）

所有用例共用下面这套夹具。夹具应由底座的种子数据命令（`bun run seed`，见 [G 文件](G-secondary-dev-ai.md) TC-G-019）一键生成，保证可重置、ID 稳定。

### 3.1 租户

| ID | 名称 | tenant_id | 用途 |
|---|---|---|---|
| `T1` | 示例科技 | `11111111-1111-1111-1111-111111111111` | 主测试租户 |
| `T2` | 另一公司 | `22222222-2222-2222-2222-222222222222` | 跨租隔离对照租户 |

### 3.2 组织树与空间（T1）

```text
org-hq        总部
├── org-sales      销售部
│   └── org-sales-1  销售一组
├── org-fin        财务部
└── org-legal      法务部
```

- 空间：`S1 = space-sales-1`「销售一组工作区」（挂 `org-sales-1`）、`S2 = space-legal`「法务工作区」（挂 `org-legal`）。
- T2 另有 `org-t2-hq`「另一公司总部」。

### 3.3 账号与角色（统一初始密码 `Passw0rd!`，符合密码策略）

| 账号 | 租户 | 组织 | 角色 | 数据范围 | 用途 |
|---|---|---|---|---|---|
| `owner@example.com` | T1 | org-hq | `tenant-owner` + `order-admin` | 本组织及下级 | 管理员：改权限、查审计、配品牌、导出数据；`order-admin` 含 `order:*` |
| `boss@example.com` | T1 | org-sales | `manager` | 本组织及下级 | 数据范围「本组织及下级」样本；审批主管节点 |
| `staff@example.com` | T1 | org-sales-1 | `member` | 仅本人 | 数据范围「仅本人」样本；**无** `order:invoice:approve` / `delete` |
| `finance@example.com` | T1 | org-fin | `finance` | 仅本人 | 审批流财务节点 |
| `dept_head@example.com` | T1 | org-hq | `dept_head` | 本组织及下级 | 审批超时升级目标 |
| `both@example.com` | T1 + T2 | org-hq / org-t2-hq | `auditor` | 全部（只读） | 同一账号跨两个租户，验证 `X-Tenant-Id` 切换 |
| `t2user@example.com` | T2 | org-t2-hq | `member` | 仅本人 | 跨租对照账号 |
| `newbie@example.com` | T1 | org-sales-1 | （待审批 / 待 SCIM 同步） | — | SCIM 出站同步用例的被同步用户 |

### 3.4 业务数据

| ID | 租户 | 归属人 | 组织 | 说明 |
|---|---|---|---|---|
| `order-1001` | T1 | `boss@example.com` | org-sales | 数据范围用例 |
| `order-1002` | T1 | `staff@example.com` | org-sales-1 | 数据范围用例 |
| `order-1003` | T1 | `owner@example.com` | org-fin | 平级/他部门数据，应对 A 角色不可见 |
| `order-2001` | T2 | `t2user@example.com` | org-t2-hq | 跨租越权目标 |
| `order-9999` | — | — | — | **不存在**的资源 ID，用于存在性泄露对照 |
| `INV-001` | T1 / T2 各一条 | 见上 | — | 同名业务数据，用于验证租户上下文传播 |
| `obj-001` | T1 | `staff@example.com` | — | 文件对象，用于预签名访问 |

### 3.5 业务模块与权限点

`modules/order/manifest.json` 声明（格式见 [design-summary.md](../design-summary.md) §1）：

```json
{
  "id": "order",
  "name": "订单",
  "version": "0.1.0",
  "enabled": true,
  "permissions": [
    { "code": "order:invoice:read",    "name": "查看订单",   "type": "menu" },
    { "code": "order:invoice:write",   "name": "编辑订单",   "type": "button" },
    { "code": "order:invoice:approve", "name": "审批订单",   "type": "button" },
    { "code": "order:invoice:delete",  "name": "删除订单",   "type": "button" }
  ],
  "menus": [
    { "label": "订单", "icon": "receipt", "route": "/order/invoice", "perm": "order:invoice:read" }
  ],
  "dependencies": [],
  "config": { "itemsPerPage": { "type": "number", "default": 20, "scope": "tenant" } }
}
```

另有 `modules/invoice/manifest.json`（权限点 `invoice:record:read`）、`modules/gift/`（权限点 `gift:card:read` / `gift:card:write`，用于扩展点用例）。

### 3.6 外部系统夹具

| 夹具 | 最小实现要求 | 服务于 |
|---|---|---|
| SAML IdP | 提供 `metadata.xml` 与测试用户 `saml-user` | TC-A-011 / TC-A-012 |
| OIDC IdP | 提供 `/.well-known/openid-configuration`、`token_endpoint`、测试用户 `oidc-user` | TC-A-011 / TC-A-012 |
| LDAP 服务 | 目录条目 `uid=ldap-user`（可配 `ldap3` 或 OpenLDAP 容器） | TC-A-011 / TC-A-012 |
| SCIM 2.0 服务端 | 实现 `Users` / `Groups` 端点（mock server 即可） | TC-B-005 ~ TC-B-008 |
| 第三方应用 | 模拟 OAuth 客户端：`app_demo` + `redirect_uri` + JWKS/introspect 校验 | TC-A-013 / TC-A-014 / TC-E-001 / TC-E-002 |
| SMTP mock | MailHog / smtp4dev（收件箱可查） | TC-F-005 / TC-F-006 |
| Webhook mock | 可编排返回 500 / 200，记录调用次数与时间戳 | TC-F-004 |
| 对象存储 | S3 兼容（MinIO），支持分片与预签名 | TC-F-009 / TC-F-010 |
| OTel Collector + tracing backend | 接收 trace/span，可按 `trace_id` 检索 | TC-F-015 / TC-F-016 |
| AI 编码助手 | 能读取仓库根 `AGENTS.md` 与 `capabilities/index.json` | TC-G-012 / TC-G-015 ~ TC-G-017 |
| 真实消费方项目 | 一个最小业务系统（实体 + 列表 + 表单 + 审批流） | TC-H-001 ~ TC-H-004 |

---

## 4. 命名与接口约定（与既有文档对齐）

| 约定项 | 取值 | 来源 |
|---|---|---|
| 权限点编码 | `module:resource:action`，如 `order:invoice:approve`；通配 `order:*` | demand.md §A1 |
| 业务模块目录 | `modules/<name>/`（`manifest.json` + `backend/` + `frontend/` + `migrations/` + `tests/contracts.rs` + `AGENTS.md` + `README.md`） | design-summary.md §1 / ADR-0001 D7 |
| 能力索引目录 | `capabilities/`（`index.json` / `permissions.json` / `extension-points.json` / `schema/metadata.schema.json` / `sdk/*.json` / `api.json`），**自动生成禁止手改** | design-summary.md §3 |
| 提示词库 | `prompts/*.md`（`01-new-crud-module.md` 等 8 份） | design-summary.md §4.3 |
| SDK 包名 | `@base/ui` `@base/sdk` `@base/auth` `@base/schema` `@base/theme` `@base/i18n` `@base/hooks` | demand.md §D1 |
| 业务 API 路径 | `/api/<module>/<resource>/<action>`，如 `GET /api/order/invoice/approve` | acceptance.md AC-001 |
| 前端路由 | `/<module>/<resource>`，与 `manifest.json` 的 `menus[].route` 一致，如 `/order/invoice` | design-summary.md §1 |
| 平台 API 分组 | `/api/iam` `/api/rbac` `/api/tenant` `/api/org` `/api/config` `/api/feature-flags` `/api/app` `/api/audit` `/api/notify` `/api/workflow` `/api/storage` `/api/tasks` `/api/meta` | 本用例集约定（见 §7） |
| 租户 / 空间上下文头 | `X-Tenant-Id`、`X-Space-Id` | acceptance.md AC-009（`X-Tenant-Id` 已定） |
| 错误格式 | RFC 9457 `application/problem+json`，字段 `status` / `title` / `code` / `detail` | demand.md §F10 |
| 限流 | 三级 = 租户 / 用户 / 接口；超限 HTTP 429 + `Retry-After` | acceptance.md AC-027 |
| 链路 | W3C `traceparent`，`trace_id` 为 32 位十六进制 | demand.md §F9 |
| 隔离级别 | L1 共享表 + `tenant_id` + PostgreSQL RLS（默认） | ADR-0001 D2 |
| 命令 | `cargo xtask create-module {name}` / `cargo xtask capabilities` / `bun run dev` / `cargo test --workspace` | design-summary.md §4.1 |

---

## 5. AC → TC 覆盖矩阵（39 / 39）

| AC | 标题 | 覆盖 TC | 条数 | 外部依赖 | 最高优先级 |
|---|---|---|---|---|---|
| AC-001 | 权限点注册表驱动前后端权限判定 | TC-A-001, TC-A-002, TC-A-003 | 3 | 无 | P0 |
| AC-002 | 数据范围隔离生效 | TC-A-004, TC-A-005, TC-A-006 | 3 | 无 | P0 |
| AC-003 | 注册策略可配置且按租户隔离 | TC-A-007, TC-A-008 | 2 | 无 | P0 |
| AC-004 | 邀请链接一次性且可过期 | TC-A-009, TC-A-010 | 2 | 无 | P0 |
| AC-005 | SSO 入站三协议 | TC-A-011, TC-A-012 | 2 | **IdP（SAML/OIDC/LDAP）** | P0 |
| AC-006 | 底座作为 IdP 向第三方应用签发令牌 | TC-A-013, TC-A-014 | 2 | 第三方应用（可 mock） | P0 |
| AC-007 | 令牌轮换与撤销 | TC-A-015, TC-A-016, TC-A-017 | 3 | 无 | P0 |
| AC-008 | 设备会话可见且可远程下线 | TC-A-018, TC-A-019 | 2 | 无 | P0 |
| AC-009 | 租户上下文全链路传播 | TC-B-001, TC-B-002 | 2 | 无 | P0 |
| AC-010 | 跨租户数据绝对不串 | TC-B-003, TC-B-004 | 2 | 无 | P0 |
| AC-011 | 组织架构 SCIM 出站同步 | TC-B-005, TC-B-006 | 2 | **SCIM 2.0 服务端** | P1 |
| AC-012 | 组织同步冲突进人工队列 | TC-B-007, TC-B-008 | 2 | **SCIM / LDAP 外部系统** | P1 |
| AC-013 | 配置三层覆盖 | TC-C-001, TC-C-002 | 2 | 无 | P0 |
| AC-014 | 品牌配置前端实时生效 | TC-C-003, TC-C-004 | 2 | 无 | P0 |
| AC-015 | Feature Flag 按租户生效 | TC-C-005, TC-C-006 | 2 | 无 | P0 |
| AC-016 | SDK 按需引入且无全局副作用 | TC-D-001, TC-D-002 | 2 | 无 | P0 |
| AC-017 | 权限组件与路由守卫联动 | TC-D-003, TC-D-004 | 2 | 无 | P0 |
| AC-018 | schema 驱动零代码 CRUD | TC-D-005, TC-D-006 | 2 | 无 | P0 |
| AC-019 | API 类型自动同步 | TC-D-007, TC-D-008 | 2 | 无 | P0 |
| AC-020 | 应用注册与令牌校验 | TC-E-001, TC-E-002 | 2 | 第三方应用（可 mock） | P0 |
| AC-021 | 扩展点不修改平台核心 | TC-E-003, TC-E-004 | 2 | 无 | P0 |
| AC-022 | 审计日志完整且不可篡改 | TC-F-001, TC-F-002, TC-F-003 | 3 | 无 | P0 |
| AC-023 | 通知多渠道投递与重试 | TC-F-004, TC-F-005, TC-F-006 | 3 | **SMTP mock + Webhook mock** | P0 |
| ~~AC-024~~ | ~~审批流端到端~~ | ~~TC-F-007, TC-F-008~~ | ~~2~~ | 已取消（D121） | — |
| AC-025 | 文件上传与预签名访问 | TC-F-009, TC-F-010 | 2 | **S3 兼容对象存储（MinIO）** | P1 |
| AC-026 | 导入导出异步且可回溯 | TC-F-011, TC-F-012 | 2 | 无 | P0 |
| AC-027 | 限流三级生效 | TC-F-013, TC-F-014 | 2 | 无 | P0 |
| AC-028 | trace_id 全链路贯穿 | TC-F-015, TC-F-016 | 2 | **OTel Collector + tracing backend** | P0 |
| AC-029 | 新业务系统只在业务目录写代码 | TC-H-001, TC-H-002 | 2 | **真实消费方项目** | P0 |
| AC-030 | 底座升级不破坏业务 | TC-H-003, TC-H-004 | 2 | **真实消费方项目 + 平台版本发布** | P0 |
| AC-031 | 依赖方向单向，平台不依赖业务 | TC-G-001, TC-G-002 | 2 | 无 | P0 |
| AC-032 | 业务模块可独立启停 | TC-G-003, TC-G-004 | 2 | 无 | P0 |
| AC-033 | 扩展点契约测试套件可拦截违规实现 | TC-G-005, TC-G-006, TC-G-007 | 3 | 无 | P0 |
| AC-034 | 平台小版本升级不破坏扩展点 | TC-G-008, TC-G-009 | 2 | 平台可发布的小版本 | P0 |
| AC-035 | AI 引导文件体系完整且与实际一致 | TC-G-010, TC-G-011 | 2 | 无 | P0 |
| AC-036 | 提示词库可直接产出合规代码 | TC-G-012 | 1 | **AI 编码助手** | P1 |
| AC-037 | 脚手架生成的模块开箱即用 | TC-G-013, TC-G-014 | 2 | 无 | P0 |
| AC-038 | 能力索引有效防止 AI 重造轮子 | TC-G-015, TC-G-016, TC-G-017 | 3 | **AI 编码助手** | P0 |
| AC-039 | 一条命令起全栈 | TC-G-018, TC-G-019 | 2 | Docker 环境 | P0 |

**合计 39 条 AC、84 条 TC** —— 其中 **AC-024 及其 TC-F-007/008 已于 2026-11-19 作废（D121）**，
现存 **38 条 AC、82 条 TC**，无遗漏 AC、无悬空 TC。

---

## 6. 当前无法立即执行的 AC（外部依赖清单）

> 前提说明：**所有**用例都要等平台实现代码落地后才可能执行。下表标的是「除平台实现之外，还需要哪些平台之外的前置依赖」，这些依赖不齐时，对应用例只能先写脚本、不能跑出判定结果。

### 6.1 需要真实外部服务的 AC

| AC | 外部依赖 | 缺失时的替代方案 | 受影响 TC |
|---|---|---|---|
| AC-005 | SAML IdP / OIDC IdP / LDAP 服务各一套 | 用 mock IdP 顶替可跑通 happy path，但「协议失败提示可区分」需真实错误注入 | TC-A-011, TC-A-012 |
| AC-011 | SCIM 2.0 `Users`/`Groups` 服务端 | mock SCIM server 可替代（推荐先做 mock） | TC-B-005, TC-B-006 |
| AC-012 | 外部系统中的冲突数据（部门不一致） | 可由 mock SCIM 预置冲突数据替代 | TC-B-007, TC-B-008 |
| AC-023 | SMTP 服务、Webhook 接收端 | MailHog / smtp4dev + 可编排 webhook mock 可替代（**强烈建议先备 mock**） | TC-F-004 ~ TC-F-006 |
| AC-025 | S3 兼容对象存储（MinIO） | 无本地替代方案；MinIO 用 docker-compose 起即可 | TC-F-009, TC-F-010 |
| AC-028 | OTel Collector + tracing 后端 | 可先用内存 exporter 验证日志/消息中的 `trace_id` 一致性，但「链路树完整」需 Collector | TC-F-015, TC-F-016 |
| AC-039 | Docker / Compose 环境 | 裸机跑不通全栈（需 PG + Redis） | TC-G-018, TC-G-019 |

### 6.2 需要 AI 环境或跨项目协作的 AC

| AC | 外部依赖 | 说明 | 受影响 TC |
|---|---|---|---|
| AC-036 | 可用的 AI 编码助手，且能读取仓库根 `AGENTS.md` 与 `capabilities/index.json` | 需固定 AI 客户端与模型版本，否则结果不可复现；建议记录所用客户端与 prompt 原文 | TC-G-012 |
| AC-038 | 同上 | 判定依据是「产出是否复用底座能力」，属人工 + 静态扫描混合判定 | TC-G-015 ~ TC-G-017 |
| AC-029 | **第一个真实消费方项目** | acceptance.md 已明确该层验证不属于 REQ-001 实现范围，需另开 REQ | TC-H-001, TC-H-002 |
| AC-030 | 真实消费方项目 + 平台可发布的小版本 | 需平台有一次真实的版本升级动作 | TC-H-003, TC-H-004 |

### 6.3 需要平台版本动作的 AC

| AC | 外部依赖 | 受影响 TC |
|---|---|---|
| AC-034 | 平台存在可升级的**次要版本**（如 `0.1.0` → `0.1.1`）且保持扩展点签名不变 | TC-G-008, TC-G-009 |

### 6.4 可在无外部依赖下执行的部分

以下 AC 在平台代码落地后即可全部执行（AC-001 ~ AC-004、AC-007 ~ AC-010、AC-013 ~ AC-022、AC-026、AC-027、AC-031 ~ AC-033、AC-035、AC-037，共 **27 条 AC / 63 条 TC**；已扣除作废的 AC-024 与 TC-F-007/008），建议作为 Dev 的第一批自验与 CI 门禁。

---

## 7. 假设与待 BA/Dev 确认的约定

以下条目在既有文档中**没有**明确写死，本用例集按最合理方式取值并在此登记；如与实现冲突，以 BA 裁定为准，用例相应调整：

1. **平台 API 分组前缀**：`/api/iam`、`/api/rbac`、`/api/tenant`、`/api/org`、`/api/config`、`/api/feature-flags`、`/api/app`、`/api/audit`、`/api/notify`、`/api/workflow`、`/api/storage`、`/api/tasks`、`/api/meta` —— 依据 ADR-0001 D1 的 crate 划分（iam / rbac / tenant / org / notify / audit / workflow / storage / app）推导。
2. **业务 API 路径形式**：`/api/<module>/<resource>/<action>`，沿用 acceptance.md AC-001 已有的 `GET /api/order/invoice/approve`。
3. **空间上下文请求头** `X-Space-Id`（`X-Tenant-Id` 已由 AC-009 写死，空间头为本用例集推导）。
4. **配置键名**为 camelCase（`logo` / `primaryColor` / `theme.radius`），与 design-summary.md `manifest.json` 的 `config.itemsPerPage` 风格一致。
5. **配置读取接口**返回 `source` 字段以暴露命中的层级；**Feature Flag 在功能关闭时**返回 200 + `enabled=false`（AC-015 未规定状态码，TC-C-005 已写明「二选一并全局统一」）。
6. **通用渲染器组件名** `@base/schema` 的 `SchemaTable` / `SchemaForm`（命名待 Dev 对齐，功能语义以 AC-018 为准）。
7. **权限缺失时的响应码**：AC-010 允许「404 或 403」二选一，本用例集统一按 **「资源类越权返回 404、动作类越权返回 403」** 编写，执行前需实现方确认该口径（TC-A-006 / TC-B-003 的预期结果已写成「二者之一 + 不得泄露存在性」，因此口径统一前用例仍可执行）。
8. **审计防篡改**采用 append-only + 哈希链（`prev_hash` / `hash`），与 demand.md §F1「不可篡改（append-only + 哈希链）」一致。
9. ~~**AC-024 的金额分支**（> 5000 走两级）~~ —— 已随 AC-024 取消（D121），TC-F-007/008 一并作废。
10. **AC-038 的判定方式**：以「产出代码的静态扫描 + 人工确认」判定是否复用 `@base/ui` / `@base/schema` / `@base/auth`，不依赖 AI 行为稳定性的字面比对。

## 8. 建议执行顺序

| 阶段 | 范围 | 说明 |
|---|---|---|
| 第一批（Dev 自验 + CI 门禁） | AC-001 ~ AC-004、AC-007、AC-009、AC-010、AC-013 ~ AC-022、AC-026、AC-027、AC-031 ~ AC-033、AC-035、AC-037 | 无外部依赖，CI 可全自动化 |
| 第二批（需本地 mock） | AC-006、AC-011、AC-012、AC-020、AC-023 | 先备 mock SCIM / OAuth 客户端 / SMTP / Webhook |
| 第三批（需真实中间件） | AC-005、AC-025、AC-028、AC-039 | 需 IdP、MinIO、OTel、Docker 全栈 |
| 第四批（AI 与跨项目） | AC-036、AC-038、AC-029、AC-030、AC-034 | 需 AI 环境、消费方项目与平台版本发布动作 |
