# F. 平台运维与治理 —— 测试用例（AC-022 ~ AC-028）

- **需求**: REQ-001 通用项目底座（Base Platform）
- **来源**: [acceptance.md](../acceptance.md) §F、[demand.md](../demand.md) §F（审计 / 通知 / 工作流 / 文件 / 任务 / API 治理 / 可观测性）
- **用例数**: 16（TC-F-001 ~ TC-F-016）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## F.0 本域约定

- 审计记录字段：`actor_id` / `actor_name` / `action` / `object_type` / `object_id` / `before` / `after` / `ip` / `user_agent` / `tenant_id` / `space_id` / `trace_id` / `at`；防篡改字段 `prev_hash` / `hash`。
- 审计查询：`GET /api/audit/logs?action=&object_type=&object_id=&tenant_id=`。
- 通知通道：`inapp` / `email` / `webhook`；通知投递记录：`GET /api/notify/deliveries?message_id=&channel=`；订阅偏好：`POST /api/notify/subscriptions`、`DELETE /api/notify/subscriptions/{id}`。
- 工作流实例：`POST /api/workflow/instances`、`POST /api/workflow/instances/{id}/approve`、`POST /api/workflow/instances/{id}/reject`。
- 对象存储（S3 兼容）：`POST /api/storage/uploads`（分片预签名）、`POST /api/storage/uploads/{upload_id}/complete`、`POST /api/storage/objects/{object_id}/download-url`。
- 异步任务：`POST /api/tasks/imports` → `{task_id}`；`GET /api/tasks/{task_id}` → `{status, total, processed, succeeded, failed}`；`GET /api/tasks/{task_id}/errors` → 行号 + 原因。
- 限流三级：**租户 / 用户 / 接口**；超限返回 HTTP 429 且带 `Retry-After` 响应头。
- 链路上下文：W3C `traceparent` 请求头（`00-<32 hex trace_id>-<16 hex span_id>-01`）。

---

### TC-F-001: 敏感操作审计字段完整
- **关联 AC**: AC-022
- **前置条件**:
  - 账号 `owner@example.com`（T1 管理员）已登录，来源 IP 固定（如 `10.0.0.9`）。
  - 准备一次「修改角色权限」操作与一次「导出数据」操作。
- **测试步骤**:
  1. 修改角色 `member` 的权限集合（增删 `order:invoice:write`）。
  2. 执行一次数据导出 `POST /api/tasks/exports`。
  3. 查询 `GET /api/audit/logs?action=permission.update` 与 `?action=data.export`。
- **预期结果**:
  1. 两条审计记录均存在，`at` 为 ISO-8601 时间戳，与操作发生时间相差 ≤ 1 秒。
  2. 每条含 `actor_id` / `actor_name`（= `owner@example.com`）、`object_type` / `object_id`、`before` 与 `after`（权限集合变更前后差异可读）、`ip`（= `10.0.0.9`）、`user_agent`、`tenant_id`（= T1）、`trace_id`（非空）。
  3. 权限变更的 `before` 与 `after` 能明确区分「新增了哪个权限点 / 删除了哪个权限点」，不得为 null 或空对象。
  4. 审计记录带 `action` 分类字段，可按类型筛选。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 字段完整性断言

### TC-F-002: 篡改审计记录后哈希链校验失败并告警
- **关联 AC**: AC-022
- **前置条件**:
  - 已产生至少 3 条审计记录（构成哈希链）。
  - 具备直连测试库修改审计表的能力（测试专用，不经业务接口）。
- **测试步骤**:
  1. 用 SQL 直接把第 2 条审计记录的 `after` 字段改写为伪造值（不改 `hash`）。
  2. 调用审计链校验命令/接口（`POST /api/audit/verify-chain` 或 `cargo xtask audit-verify`）。
  3. 查看告警输出与监控指标。
- **预期结果**:
  1. 校验返回失败（非 0 退出码 / `valid = false`），并明确指出**断链位置**为第 2 条记录。
  2. 告警被触发：出现告警事件（含断链记录 ID 与时间），可被监控系统采集。
  3. 业务接口层面**没有**任何写入或修改审计记录的路径（直接 UPDATE 审计表被数据库权限/触发器拒绝，或即便成功也被校验捕获）。
  4. 篡改不会导致审计查询接口返回被伪造的内容作为可信记录（校验失败状态需可见）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面枚举（篡改）+ 故障注入

### TC-F-003: 审计查询按租户 / 空间隔离
- **关联 AC**: AC-022
- **前置条件**:
  - T1、T2 各有若干条审计记录，且有同名对象 ID（如同一 `object_id` 在两个租户都出现过）。
  - 账号 `owner@example.com` 是 T1 管理员（非跨租超管）。
- **测试步骤**:
  1. 以 `owner@example.com` 登录，请求 `GET /api/audit/logs`（不显式传 tenant 过滤）。
  2. 请求 `GET /api/audit/logs?tenant_id=T2`。
  3. 对比结果中 `tenant_id` 分布。
- **预期结果**:
  1. 第 1 步返回的全部记录 `tenant_id = "T1"`，不含 T2 任何记录。
  2. 第 2 步被拒绝：返回 HTTP 403（`code` 如 `tenant_scope_denied`），不得返回 T2 数据。
  3. 越权请求同样写入审计日志（记录「越权查询审计」事件），便于发现异常探测。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 数据隔离 + 攻击面枚举

### TC-F-004: Webhook 首次 500 后按退避策略重试成功
- **关联 AC**: AC-023
- **前置条件**:
  - 配置 Webhook 通道 `wh_1`，目标地址为可编排的 mock server；mock server 首次响应 HTTP 500，之后响应 200。
  - 重试策略：指数退避（如 1s / 2s / 4s），最大重试次数 ≥ 3。
- **测试步骤**:
  1. 触发一条通知（`POST /api/notify/messages`，内容含变量 `{user}` / `{amount}`）。
  2. 观察 mock server 收到的请求次数与时间间隔。
  3. 查询 `GET /api/notify/deliveries?message_id=<id>&channel=webhook`。
- **预期结果**:
  1. mock server 收到 ≥ 2 次请求，且第 2 次成功（HTTP 200）。
  2. 重试间隔随次数递增（不小于第 1 次间隔；具体倍率按实现配置），总次数不超过最大重试次数。
  3. 投递记录最终 `status = "success"`，`attempts = 2`，含每次的尝试时间与响应码。
  4. 通知**不重复**产生业务副作用：同一消息只触发一次下游业务动作（由幂等键或消息 ID 保证）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径 + 状态迁移（重试 → 成功）

### TC-F-005: 邮件通道送达且投递记录可查
- **关联 AC**: AC-023
- **前置条件**:
  - 配置邮件通道 `email_1`（SMTP mock，如 MailHog / smtp4dev），收件人 `boss@example.com`。
  - 通知模板含多语言变量（中文模板 `notify-welcome-zh`、英文模板 `notify-welcome-en`）。
- **测试步骤**:
  1. 分别以中文、英文语言偏好触发同一条通知。
  2. 查询 SMTP mock 收件箱。
  3. 查询 `GET /api/notify/deliveries?message_id=<id>`。
- **预期结果**:
  1. SMTP mock 收到 2 封邮件，收件人地址正确。
  2. 中文邮件正文为中文模板内容、英文邮件为英文模板内容，且模板变量（`{user}` / `{amount}`）被实际替换，**不残留** `{user}` 这类占位符。
  3. 投递记录中两条 channel=`email` 的记录 `status = "success"`，含收件人、模板 ID、发送时间。
  4. 站内信（`inapp`）通道同样产生一条站内信，用户的「消息中心」可见。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 多通道等价类 + 模板变量替换

### TC-F-006: 用户退订某类型通知后不再投递
- **关联 AC**: AC-023
- **前置条件**:
  - 通知类型 `marketing`（营销类）与 `system`（系统类）已定义；用户 `staff@example.com` 默认订阅两者。
  - 投递通道 email + inapp 已配置。
- **测试步骤**:
  1. 用户在通知偏好页取消订阅 `marketing`，保存。
  2. 触发一条 `marketing` 通知与一条 `system` 通知。
  3. 查询投递记录与 SMTP mock 收件箱。
- **预期结果**:
  1. 订阅偏好保存返回 HTTP 200，`GET /api/notify/subscriptions` 中 `marketing` 状态为 `unsubscribed`。
  2. `marketing` 通知产生 0 条投递记录（所有通道均不投递），且记录一条 `suppressed` 原因的跳过记录（须二选一并全局统一；若跳过记录存在，其 `status` 明确表示「用户退订」而非失败）。
  3. `system` 通知正常投递（系统类强制通知是否豁免退订，须在实现中固定一致并在此按同一约定判定）。
  4. 退订操作写入审计记录（用户、通知类型、时间）。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 等价类（订阅 / 退订）+ 边界（不同通知类型互不影响）

> ❌ **本用例已作废（D121，2026-11-19 用户裁定取消 REQ-017 工作流引擎）**
>
> AC-024 已标注「不再验收」。以下内容仅作历史留档，**不进入任何测试计划**。
> 业务方需要审批流时，实现 `WorkflowHandler` 扩展点自行挂接，底座不提供流程引擎。

### TC-F-007: ~~两级审批流端到端（提交 → 主管通过 → 财务驳回）~~
- **关联 AC**: AC-024（**已取消**）
- **前置条件**:
  - 已定义流程 `expense`（两级：主管 → 财务），实例变量含金额与事由。
  - 角色 `manager`（主管节点审批人，`boss@example.com`）、`finance`（财务节点审批人，`finance@example.com`）、申请人 `staff@example.com`。
  - 流程定义含「金额 > 5000 走两级，否则一级」的分支（本用例取 8000）。
- **测试步骤**:
  1. 以 `staff@example.com` 提交申请 `POST /api/workflow/instances`（金额 8000）。
  2. 以 `boss@example.com` 执行 `POST /api/workflow/instances/{id}/approve`。
  3. 以 `finance@example.com` 执行 `POST /api/workflow/instances/{id}/reject`，`comment = "发票金额与实际不符"`。
  4. 申请人查询 `GET /api/workflow/instances/{id}` 与自己的待办列表。
- **预期结果**:
  1. 提交成功返回 HTTP 201，实例 `status = "running"`，当前节点为「主管审批」，生成申请人待办。
  2. 主管通过后实例 `status = "running"`，当前节点变为「财务审批」，主管待办消失，财务收到待办。
  3. 财务驳回后实例 `status = "rejected"`，`comment` 保留为「发票金额与财务驳回意见原文」，流程回到发起人（申请人待办重新出现），财务待办消失。
  4. 每一步（提交 / 通过 / 驳回）均产生审计记录（含操作人、节点、结果、意见）。
  5. 金额改为 1000 时只生成一级审批（主管），不进入财务节点（分支条件生效）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（正常路径 + 驳回回退）+ 判定表（条件分支）

> ❌ **本用例已作废（D121，2026-11-19 用户裁定取消 REQ-017 工作流引擎）**
>
> AC-024 已标注「不再验收」。以下内容仅作历史留档，**不进入任何测试计划**。
> 业务方需要审批流时，实现 `WorkflowHandler` 扩展点自行挂接，底座不提供流程引擎。

### TC-F-008: ~~审批每步产生通知，超时自动升级~~
- **关联 AC**: AC-024（**已取消**）
- **前置条件**:
  - 流程 `expense` 的主管节点超时时间配置为 60 秒（测试环境可调），超时动作为「升级到上级审批人 `dept_head`」。
  - 存在账号 `dept_head@example.com`（`dept_head` 角色）。
  - 通知通道 email 已配置。
- **测试步骤**:
  1. 提交申请后不执行审批，等待节点超时时间 + 宽限时间。
  2. 查看实例当前审批人、待办列表与通知投递记录。
  3. 以 `dept_head@example.com` 登录查看待办并审批。
- **预期结果**:
  1. 每次节点流转（提交、待审批、审批结果）均产生通知：审批人收到待办通知，申请人收到结果通知；投递记录中状态为 `success`。
  2. 超时后实例 `status` 仍为 `running`（未静默结束），当前节点变为升级后的审批人 `dept_head`，原主管待办转为「已超时升级」状态，原主管收到超时提醒。
  3. `dept_head` 登录后可见待办并能正常审批；审批后流程按定义流转。
  4. 超时升级动作本身写入审计记录（含原审批人、升级目标、超时时间）。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 时间边界（超时）+ 状态迁移

### TC-F-009: 大文件分片上传与断点续传
- **关联 AC**: AC-025
- **前置条件**:
  - S3 兼容对象存储（MinIO）可用；账号 `staff@example.com` 有文件上传权限点 `file:object:write`。
  - 准备 1 GB 大文件 `big.bin`。
- **测试步骤**:
  1. 调用 `POST /api/storage/uploads`（`filename=big.bin`、`size=1GB`、`part_size=16MB`）取得分片上传地址与 `upload_id`。
  2. 上传前 30 个分片后**中断**（模拟网络断），记录已成功分片与 ETag。
  3. 重新调用续传接口查询已上传分片，从断点继续上传剩余分片，最后调用 `POST /api/storage/uploads/{upload_id}/complete`。
  4. 通过下载接口取回对象并计算校验和。
- **预期结果**:
  1. 初始化返回 HTTP 200，含 `upload_id` 与分片 URL 数组；分片数 = 64（1 GB / 16 MB）。
  2. 断点续传接口返回已完成分片列表，与步骤 2 记录一致（平台持久化了分片状态）。
  3. 续传完成后 `complete` 返回 HTTP 200，对象 `size = 1 GB`，分片数完整。
  4. 下载得到的对象 SHA-256 与源文件一致；上传中断期间不产生「半个对象」被当作完整对象可下载的情况。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 异常路径（中断）+ 幂等/断点续传

### TC-F-010: 预签名下载链接限时有效，过期后失效
- **关联 AC**: AC-025
- **前置条件**:
  - 对象 `obj-001` 已存在；预签名链接有效期配置为 60 秒（测试环境可调）。
  - 账号 `staff@example.com` 有 `file:object:read` 权限。
- **测试步骤**:
  1. 请求 `POST /api/storage/objects/obj-001/download-url` 取得预签名链接与 `expires_at`。
  2. 链接有效期内直接下载。
  3. 等待链接过期后再次请求同一路径下载（复用步骤 1 的 URL）。
  4. 无权限账号请求同一对象的下载链接。
- **预期结果**:
  1. 返回 HTTP 200，含 `url`（带签名参数）与 `expires_at`，有效期不超过配置值（60 秒）。
  2. 有效期内下载 HTTP 200，内容字节与对象一致。
  3. 过期后用旧 URL 下载返回 HTTP 403（签名过期），不得返回对象内容。
  4. 无权限账号请求下载链接返回 HTTP 403，不泄露对象存在性细节。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（有效期临界）+ 攻击面枚举

### TC-F-011: 5 万行导入异步执行且进度可查
- **关联 AC**: AC-026
- **前置条件**:
  - 导入模板 `invoice_import.csv`，含 50000 行，其中 10 行为非法数据（缺必填 `amount` 或金额非数字）。
  - 账号 `staff@example.com` 有导入权限。
- **测试步骤**:
  1. 上传模板并触发导入 `POST /api/tasks/imports`。
  2. 记录返回的 `task_id`，立即再查 `GET /api/tasks/{task_id}`。
  3. 轮询该任务直到 `status` 变为终态。
- **预期结果**:
  1. 触发导入返回 HTTP 202/200，含 `task_id`；**接口不阻塞**至导入完成（响应耗时 < 2 秒，导入在后台执行）。
  2. 立即查询返回 `status` 为 `running`（或 `pending`），含 `total = 50000`、`processed` 递增的进度字段。
  3. 轮询期间 `processed` 单调不减且不超过 `total`；最终 `status = "completed"`（或 `completed_with_errors`），`succeeded = 49990`、`failed = 10`。
  4. `total` 与实际模板行数一致（50000）。
- **类型**: 功能 / 性能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常路径 + 进度不变量（单调性）

### TC-F-012: 非法行返回带行号与原因的回执
- **关联 AC**: AC-026
- **前置条件**:
  - 同 TC-F-011，导入已完成且 `failed = 10`。
- **测试步骤**:
  1. 查询 `GET /api/tasks/{task_id}/errors`。
  2. 逐条核对错误行号与原始数据行内容。
  3. 在数据库核对合法行是否入库、非法行是否未入库。
- **预期结果**:
  1. 返回 HTTP 200，错误条数 = 10，每条含 `row_number`（CSV 实际行号）、`reason`（明确原因，如「amount 缺失」「amount 非数字：abc」）、原始行内容或定位信息。
  2. 错误行号与模板中非法行位置逐一对应（不多不少，不重复）。
  3. 合法 49990 行已入库且数据正确；10 条非法行**未**入库（不产生半条脏数据）。
  4. 回执可导出（如 `GET /api/tasks/{task_id}/errors?format=csv`），下载内容与查询一致。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径 + 数据质量断言

### TC-F-013: 租户 / 用户 / 接口三级限流分别触发
- **关联 AC**: AC-027
- **前置条件**:
  - 限流配置：接口级 `GET /api/order/invoice` = 10 次/分钟；用户级 `staff@example.com` = 20 次/分钟；租户级 T1 = 50 次/分钟（三者独立生效）。
  - 测试环境可重置限流计数。
- **测试步骤**:
  1. 单用户连续请求 `GET /api/order/invoice` 12 次（接口级阈值 10）。
  2. 用 3 个不同账号各请求 9 次（共 27 次，触发用户/租户级但接口级各自未超）。
  3. 记录每次响应的状态码与 `Retry-After`。
- **预期结果**:
  1. 前 10 次 HTTP 200；第 11、12 次 HTTP 429，响应头含 `Retry-After`（秒数 > 0），响应体为 `application/problem+json` 且含限流维度标识（如 `scope=endpoint`）。
  2. 各账号自身未触发接口级 429；用户级与租户级阈值被触发时，同样返回 429 且 `Retry-After` 存在，响应体中的维度标识与实际触发的维度一致（用户级 `scope=user`、租户级 `scope=tenant`）。
  3. 被限流的请求不执行业务逻辑（响应中无数据体），且限流计数在 `Retry-After` 到期后恢复（限流窗口内继续 429）。
- **类型**: 性能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（三级维度）+ 边界（阈值第 N 次与 N+1 次）

### TC-F-014: 未触发限流的维度不受影响
- **关联 AC**: AC-027
- **前置条件**:
  - 同 TC-F-013；当前仅接口级计数达到 10（用户级 20、租户级 50 均未达到）。
- **测试步骤**:
  1. 继续请求受限接口 `GET /api/order/invoice` 3 次（应持续 429）。
  2. 立即请求另一接口 `GET /api/order/invoice/order-1001` 5 次。
  3. 用另一个账号 `boss@example.com` 请求受限接口 3 次。
- **预期结果**:
  1. 步骤 1 全部 HTTP 429（接口级窗口内持续生效）。
  2. 步骤 2 全部 HTTP 200（接口级限流只作用于该接口，其他接口不被误伤）。
  3. 步骤 3 全部 HTTP 200（接口级计数按接口维度而非按用户累计；同接口不同账号共享租户/接口配额的行为须与实现定义一致——若实现为「接口维度全局计数」，则在步骤 3 也应为 429，本用例判定依据以实现文档为准并须全局统一）。
- **类型**: 性能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界（不误伤）+ 隔离性断言

### TC-F-015: trace_id 在日志与链路中一致
- **关联 AC**: AC-028
- **前置条件**:
  - OTel Collector 已部署并接收应用上报；日志为结构化输出；应用暴露 `GET /health` 与 `GET /ready`。
- **测试步骤**:
  1. 发起一次跨服务调用（如前端 → 网关 → `GET /api/order/invoice`），捕获请求 `traceparent` 头或响应头中的 `trace_id`。
  2. 在日志平台按该 `trace_id` 检索全部日志行。
  3. 在链路平台（OTel Collector → tracing backend）按该 `trace_id` 检索 span 树。
- **预期结果**:
  1. 请求携带/返回的 `trace_id` 为 32 位十六进制字符串，且在所有相关组件中保持同一值。
  2. 日志中每条相关日志行均含同值 `trace_id` 字段（结构化字段，不是拼进 message 文本）。
  3. 链路中 span 树完整，父子 span 的 `trace_id` 相同、`span_id` 不同，链路层级与调用关系一致（前端 → 网关 → 后端 → DB）。
  4. 未注入 `traceparent` 的请求由平台自行生成 `trace_id`，同样可串联。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 全链路一致性断言

### TC-F-016: 异步消息携带同一 trace_id 并可串联
- **关联 AC**: AC-028
- **前置条件**:
  - 触发一次会产生异步任务的请求（如 `POST /api/tasks/exports`，或一条触发通知与工作流的操作）。
  - 任务执行器与消息体落库/日志可见。
- **测试步骤**:
  1. 记录触发请求返回的 `trace_id`。
  2. 在任务执行器日志、消息记录体、投递记录中检索该 `trace_id`。
  3. 检索该任务最终产物（导出文件/通知发送记录）的关联标识。
- **预期结果**:
  1. 消息记录体含 `trace_id` 字段且值与触发请求一致；任务执行器日志含同一 `trace_id`。
  2. 由该消息触发的下游动作（通知投递、工作流流转）日志中可检索到同一 `trace_id` 或可回溯的父子 `span_id` 关系。
  3. 消息体中除 `trace_id` 外还须携带租户上下文（`tenant_id`），二者不互相覆盖。
  4. 从触发请求到任务完成，可通过 `trace_id` 完整串联出全部环节，无断点。
- **类型**: 功能 / 可维护性
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 全链路传播断言（含异步边界）
