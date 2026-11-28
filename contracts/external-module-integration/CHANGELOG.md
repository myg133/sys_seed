# CHANGELOG — external-module-integration

## v1.0.0（draft，2026-11-28）

首版。定义平台与外部模块的进程边界。

### 边界

- 平台提供能力，应用选择接入；未声明能力不校验、不展示。
- 必选能力：`manifest`、`config`；其余可选（`admin-ui`、`events`、`audit.management`、`audit.business`、`identity`、`authorization`、`proxy.http`、`observability`）。
- 不做：WebSocket 代理、业务指标存储、强制业务事件入链。

### 配置

- 平台是配置的唯一持久化；模块只读，且必须实现本地快照（无快照且平台不可用 → 启动失败）。
- 四层解析：default → platform → tenant → user；对象深合并、数组整替换、`null` 清除。
- 查询接口：租户级 `/apps/{id}?scope=effective`、用户级 `/apps/{id}/users/{uid}`。
- 平台侧缓存 TTL 按应用可配，默认 120 秒，上限 300 秒，下限待压测确定；变更主动失效。
- 用户级配置禁止导出；JSON 导入仅管理员入口。

### 管理面

- `admin.pages[].placement`：`app`（管理员，platform/tenant）与 `profile`（用户，user）。
- 组件 v1：`section` / `tabs` / `form` / `table` / `stats` / `actions`；对象配置平铺，不做原始 JSON 编辑。
- 描述文件只允许白名单组件与受限 props，禁止 HTML/脚本/表达式。

### 事件

- 平台 → 模块：`config.changed` / `config.user.changed` / `app.disabled` / `key.rotated`。
- webhook 为主，长轮询为备；事件是加速，不是正确性保证（TTL 兜底）。
- 多副本：webhook 不广播，每副本独立轮询。

### 契约注册

- Pull（默认）/ Push（仅出站）/ 导入（迁移）。
- `version` + `contentHash` 幂等；`draft → active → deprecated`；校验失败不影响 `active`。

### 审计

- 管理事件进链；业务事件可选（建议采样）。
- 配置保存一次只产生一个事件，`changedKeys` 聚合 ≤200；敏感值不落。

### 分发

- 契约包：正文 + schema + OpenAPI + 契约测试 + Go 示例 + prompts + 校验和。
- Git tag 主渠道，制品库镜像，在线元数据 `GET /api/contracts`。
- `contract-verify` 校验 manifest/schema/测试/版本支持窗口。

### 测试

- 35 条，按能力分组；通用组必跑，其余仅在声明对应能力时跑。

### 平台侧前置

P0-1 网关 / P0-2 manifest 扩展 / P0-3 审计 ingest / P0-4 配置托管与缓存 / P0-5 密钥 / P0-6 PageRenderer / P0-7 契约测试 / P0-8 契约包发布。
