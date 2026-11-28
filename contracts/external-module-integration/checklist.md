# 准入检查表（checklist）

> 提交准入包时逐项填写：状态（✅ / ❌ / N/A）、证据（命令输出、截图、链接）、负责人。
> 任何一项 ❌ 不进入灰度。N/A 必须写理由。

## 0. 能力声明

- [ ] 0.1 manifest 的 `capabilities` 已声明；必选 `manifest` + `config` 均在
- [ ] 0.2 能力依赖已满足（`admin-ui`→`config`，`events`→`config`，`authorization`/`proxy.http`→`identity`）
- [ ] 0.3 未声明能力对应的检查项已填 N/A，并写明理由

## A. 契约与 manifest（通用）

- [ ] A1 仓库记录 pin：`external-module-integration: <version>`，与 `contracts.lock` 一致
- [ ] A2 `contract-verify --manifest` 退出码 0
- [ ] A3 manifest 的 `id` / `version` / `enabled` 与实现一致
- [ ] A4 `runtime.kind = remote`，`baseUrl` / `healthPath` / `readyPath` 正确
- [ ] A5 `runtime.routes` 覆盖全部对外路径；未声明路径默认拒绝已验证
- [ ] A6 `errorCodes` 已注册；未注册码不会出现在响应里
- [ ] A7 契约进入方式已声明（Pull / Push / 导入），并按该方式验证成功
- [ ] A8 manifest 带 `version` + `contentHash`；重复提交幂等
- [ ] A9 校验失败返回 422，`active` 版本不受影响；回滚可用
- [ ] A10 `runtime.reachability` 与实测一致（`bidirectional` / `outbound-only`）
- [ ] A11 仅出站模式：长轮询 `since` 续传不丢事件；快照/命令队列降级行为已验证（如适用）

## B. 认证与上下文（通用）

- [ ] B1 所有业务路由经过验签中间件；健康检查显式豁免
- [ ] B2 缺失/伪造/过期/重放四类请求均返回 401 `MODULE_CONTEXT_INVALID`
- [ ] B3 模块不解析用户 JWT，不调用 IAM 校验用户令牌
- [ ] B4 `X-Base-Trace` 写入日志，并向下游透传
- [ ] B5 容器时间与 NTP 同步；偏移 > 300s 的请求被拒绝

## C. 权限（`authorization` 或 `config` 租户隔离）

- [ ] C1 权限点编码符合 `module:resource:action`
- [ ] C2 每个已登记权限点都有判定点（网关 route 或模块显式判定）
- [ ] C3 越权动作返回 403，越权资源返回 404
- [ ] C4 404 响应不泄露资源是否存在
- [ ] C5 字段级权限：无权字段不出现在列表/详情
- [ ] C6 `X-Base-Scope` 的五种语义都有实现或明确不支持

## D. 数据与租户（`config`）

- [ ] D1 每张业务表 `tenant_id NOT NULL`
- [ ] D2 RLS 覆盖 SELECT/INSERT/UPDATE/DELETE，或应用层强制有测试证明
- [ ] D3 负向测试：租户 A 上下文查租户 B 数据 → 404/空
- [ ] D4 日志/审计/错误体不含其它租户标识
- [ ] D5 软删除与后台任务同样受租户约束
- [ ] D6 迁移可重复执行，失败可回滚

## E. 审计（`audit.management` / `audit.business`）

- [ ] E1 所有写操作在**业务事务内**写 outbox
- [ ] E2 `eventId` 全局唯一；重复投递不重复入链
- [ ] E3 `before/after` 已脱敏，无 token/密码/PII
- [ ] E4 投递失败有退避与死信；死信可重放
- [ ] E5 平台审计页能看到事件，链校验通过
- [ ] E6 事件里的 `tenantId` 与请求上下文一致
- [ ] E7 配置保存一次只产生一个事件；`changedKeys` 聚合且 ≤200
- [ ] E8 系统级与用户级配置事件分别用 `config.update` / `user.config.update`
- [ ] E9 `secretRef` 与敏感键只出现在 `changedKeys`，值不落审计

## F. 配置托管与分层（`config` / `events`）

- [ ] F1 每个 key 有 descriptor：类型、`scopes`、`userEditable`、默认值、i18n key、group/order
- [ ] F2 对象 schema 合法：只用于受支持子集、深度 ≤4、属性 ≤100、无 oneOf/$ref
- [ ] F3 四层解析（default → platform → tenant → user）与合并规则已实现：对象深合并、数组整替换、`null` 清除
- [ ] F4 模块有本地快照（revision/etag/合并值/校验和），加载时原子替换
- [ ] F5 无快照且平台不可用 → 启动失败；运行期失败 → 冻结在用快照
- [ ] F6 应用侧**没有**配置持久化（配置文件、环境变量配置源、自建配置表均已删除）
- [ ] F7 模块不提供配置写接口；写只发生在平台
- [ ] F8 `secretRef` 不回显、不落明文、不写日志；用户层不出现 secret
- [ ] F9 `profile` 页只显示 `userEditable` 项；用户只能写自己的 `userId`
- [ ] F10 平台侧有变更历史、diff、一键回滚；模块按 revision 单调应用
- [ ] F11 导出/导入可用；导出不含明文密钥；导入进审计
- [ ] F12 平台改配置后，模块在承诺周期内生效（轮询 ≤30s，webhook 仅加速）
- [ ] F13 `POST /internal/events` 已实现：验签、去重、未知类型返回 2xx
- [ ] F14 同一事件重复投递只应用一次；4xx 不重试、5xx 重试
- [ ] F15 多副本场景：每个副本独立轮询；停掉 webhook 后 30s 内仍收敛
- [ ] F16 平台管理页能看到通知状态（成功/重试中/死信）
- [ ] F17 按用户读取：`GET .../users/{userId}` 返回合并值与 revision，支持 304
- [ ] F18 平台侧缓存默认 120 秒（可配）：重复查询不回源；变更后主动失效
- [ ] F19 应用完全忽略事件时，TTL 内也能拿到新值（默认 120 秒）
- [ ] F20 事件通道可用时，收到通知后 ≤1 秒生效（可选加速）
- [ ] F21 租户级变更只失效该租户缓存；用户级变更只失效该用户
- [ ] F22 缓存不可用时查询直接回源，不返回 5xx
- [ ] F23 容量验证：10 万用户规模下，缓存命中率与回源 QPS 在预期内
- [ ] F24 TTL 是平台级配置（默认 120s、上限 300s、下限待定），租户/用户不可见
- [ ] F25 修改 TTL 后清空该应用缓存，其它应用不受影响；响应头带当前 TTL

## G. 管理面（`admin-ui`）

- [ ] G1 `admin.version` 与平台支持版本一致
- [ ] G2 只用白名单组件；无 HTML/脚本/表达式
- [ ] G3 `form.fields[].key` 都能在 config 声明中找到
- [ ] G4 `table` / `stats` 的 `proxy` 路径已在 `runtime.routes` 声明
- [ ] G5 每个页面与动作的 `permission` 已注册
- [ ] G6 未知组件/字段类型的降级行为已验证（不白屏）
- [ ] G7 描述大小与页面数未超限（256 KB / 50 页）
- [ ] G8 写操作确实走网关判定，不依赖描述里的 permission 做安全

## H. 运维与可观测（通用）

- [ ] H1 `/healthz` 不查依赖；`/readyz` 在 DB 断开时 503
- [ ] H2 关键指标可获取（请求数、错误、延迟、审计积压）
- [ ] H3 日志结构化，含 trace/tenant/user（脱敏）
- [ ] H4 资源限制与超时已配置；长任务走 202 + job
- [ ] H5 部署清单：端口、环境变量、探针、副本数、资源
- [ ] H6 回滚方案：平台侧关闭路由即可，无需回滚数据库

## I. 文档与交付（通用）

- [ ] I1 端点清单（路径、方法、权限点、请求/响应样例）
- [ ] I2 权限点 ↔ 判定点对照表
- [ ] I3 审计事件清单与敏感字段声明
- [ ] I4 自定义错误码清单
- [ ] I5 租户隔离说明与负向测试报告
- [ ] I6 联调记录：trace、问题、修复
- [ ] I7 准入包归档（版本、校验和、日期、负责人）

## 签署

| 项 | 姓名 | 日期 | 结论 |
|---|---|---|---|
| 接入方自检 | | | |
| 平台方评审 | | | |
| 灰度批准 | | | |
