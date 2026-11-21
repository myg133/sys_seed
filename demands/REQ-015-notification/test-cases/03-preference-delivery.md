# 03 订阅偏好与投递记录（AC-S-169 / AC-S-173）

> 测试落点：`crates/platform-notify/tests/notify_preference.rs`（AC-S-169）+
> `crates/platform-notify/tests/notify_delivery.rs`（AC-S-173）。
> 偏好端点**不进权限点**（design §3 注：`GET`/`PUT /api/notify/preferences` **登录即可**，
> D300），这一条本身就是 TC-015-046 的判据。
> 站内信 provider 一律用 `CountingSender`（见 01 文件夹具说明）计数。

## AC-S-169：按 category 退订 + `system` 类不可退订

### TC-015-042 步骤 1：marketing 通知正常投递，计数 +1
- **AC**: AC-S-169 步骤 1 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 等价类
- **前置**: `tenant-a` 有普通用户 `u-notify@example.com`（**不持任何权限点**，仅登录）；存在 `category="marketing"` 的模板；`CountingSender` 挂在 `in_app` 位
- **步骤**: 1) 入队一条 `marketing` 通知给该用户 2) `tick()` 一次
- **预期**: ① `CountingSender.count()` **从 0 变成 1**（**恰好 +1**）；② 该行 `status == "delivered"`；③ `delivered_at IS NOT NULL`；④ `last_error IS NULL`

### TC-015-043 步骤 2-3：退订 marketing 后入队 → 进死信且 last_error 点名 category
- **AC**: AC-S-169 步骤 2-3 ｜ **类型**: 功能/安全 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: TC-015-042 已交付成功
- **步骤**: 1) `PUT /api/notify/preferences` body `{category:"marketing", optedOut:true}`（用 `u-notify` 自己的令牌）2) 再入队一条 `marketing` 通知 3) `tick()` 一次 4) 读该行
- **预期**: ① `PUT` 返回 **200**；② 新那条 `status == "dead_letter"`（**不是** `delivered`、**不是** `pending` 静默丢弃 —— 这是 design §11「退订留痕」机制的要求）；③ `last_error` **包含子串 `"marketing"`**（点名是哪个 category 导致退订）；④ **计数断言**：`CountingSender.count()` **仍是 1**（退订的通知**没有真发出去**，不是「发了但标成死信」）；⑤ 该行 `attempt_count == 0`（根本没尝试投递，在出队前就被偏好拦截）或 == 1（尝试后拒绝）—— 二者取其一且须由实现明确，测试要钉住实际值，**不允许**含糊

### TC-015-044 步骤 4：GET 偏好返回 marketing optedOut=true
- **AC**: AC-S-169 步骤 4 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 等价类
- **前置**: TC-015-043 的退订已生效
- **步骤**: `GET /api/notify/preferences`（用 `u-notify` 令牌）
- **预期**: ① HTTP **200**；② 响应是数组，其中**恰好一项**满足 `category=="marketing"` 且 `optedOut==true`；③ `optedOut` 是 JSON **布尔** `true`（不是 `"true"` 字符串）；④ 数组里**不含**其它用户的偏好（该用户只有 marketing 一条设置）

### TC-015-045 步骤 5：退订 system → 400 NOTIFY_CATEGORY_MANDATORY
- **AC**: AC-S-169 步骤 5 ｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 错误推测
- **前置**: `system` category 的模板存在
- **步骤**: `PUT /api/notify/preferences` body `{category:"system", optedOut:true}`
- **预期**: ① HTTP **400**；② `code=="NOTIFY_CATEGORY_MANDATORY"`；③ `Content-Type: application/problem+json`

### TC-015-046 ⭐ 步骤 5 关键：偏好表里 `system` 行数**恰好 0**（查库，不信响应码）
- **AC**: AC-S-169 步骤 5 ｜ **类型**: 安全/数据完整性 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库，`base` 超管连接查库）｜ **方法**: 因果
- **前置**: TC-015-045 刚返回 400；用 `base` 连接（表 owner，绕过 RLS）
- **步骤**: 1) `SELECT count(*) FROM platform_notify_preference WHERE tenant_id='tenant-a' AND user_id='u-notify@example.com' AND category='system'` 2) 再查该用户全部偏好行
- **预期**: ① 第 1 步结果 **`count == 0`**（**逐字断言等于 0**，不是「不大于 1」）—— 这是判据核心：若实现「先 INSERT 再校验报错」，用户的 `system` 就**真的被关掉过**（哪怕后续回滚失败或走了非事务路径）；② 第 2 步结果**只含 `marketing` 一行**，**不含** `system`、`transactional`；③ 对照组：TC-015-043 写入的 `marketing` 行**仍在**且 `opted_out == true`（400 没有回滚掉之前的合法写入）；④ 只信响应码会漏掉「先写后报错」这条 —— 因此本条**必须**走 `SELECT`，不接受「响应是 400 就算过」

### TC-015-047 步骤 6：system 通知不受 marketing 退订影响，投递成功
- **AC**: AC-S-169 步骤 6 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 因果
- **前置**: `u-notify` 已退订 `marketing`（TC-015-043）；`system` 类模板存在
- **步骤**: 1) 入队一条 `category="system"` 通知 2) `tick()` 3) 读该行与计数
- **预期**: ① `status == "delivered"`；② `CountingSender.count()` 从 1 **变 2**（**恰好 +1**）；③ `last_error IS NULL`；④ 判据含义：`system` 既不受 `marketing` 退订牵连，本身也不可退订 → 必然可投

### TC-015-048 步骤 7：重新订阅 marketing → 恢复投递
- **AC**: AC-S-169 步骤 7 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: `u-notify` 已退订 `marketing`
- **步骤**: 1) `PUT /api/notify/preferences` body `{category:"marketing", optedOut:false}` 2) 入队一条 `marketing` 通知 3) `tick()` 4) 读该行与计数
- **预期**: ① `PUT` 返回 **200**；② 新那条 `status == "delivered"`；③ `CountingSender.count()` **+1**（相对步骤 2 之前的值）；④ 库层：该用户 `marketing` 行的 `opted_out == false`（**同一行被更新**，不是新增第二行 —— 主键是 `(tenant_id, user_id, category)`，查 `count(*)` 对 marketing 仍为 **1**）

### TC-015-049 偏好端点不需要权限点（普通用户即可改自己的设置）
- **AC**: AC-S-169 前置条件（D300）｜ **类型**: 功能/安全 ｜ **优先级**: P1 ｜ **自动化**: 集成 ｜ **方法**: 等价类（无权限）
- **前置**: `u-notify` **不持** `notify:template:*` / `notify:delivery:*` 任何权限点
- **步骤**: 1) `GET /api/notify/preferences` 2) `PUT /api/notify/preferences` `{category:"marketing", optedOut:true}`
- **预期**: ① 两步都**不是 403**（`GET` 200、`PUT` 200）；② design §3 注与 D300 的判据：偏好是「用户自己的设置」，**不进权限点表**；③ 反向对照：`GET /api/notify/templates` 用同一令牌返回 **403**（证明该用户**确实**没有权限点，不是因为他有超管身份）—— 这条对照是本用例有区分力的关键

### TC-015-050 偏好只影响本人：同租户另一用户不受牵连
- **AC**: AC-S-169（隔离补强）｜ **类型**: 安全 ｜ **优先级**: P1 ｜ **自动化**: 集成（真实库）｜ **方法**: 边界值
- **前置**: `u-notify` 已退订 `marketing`；同租户另有 `u-other@example.com` **未**做任何设置
- **步骤**: 1) 给 `u-other` 入队一条 `marketing` 通知 2) `tick()` 3) 读该行
- **预期**: ① `u-other` 那条 `status == "delivered"`（不受 `u-notify` 退订影响）；② `CountingSender.count()` **+1**；③ `platform_notify_preference` 里 `u-other` 的**行数为 0**（没人给他写过偏好行）；④ 库层：两条偏好查询以 `(tenant_id, user_id)` 双键隔离

## AC-S-173：投递记录 —— 快照 + 租户隔离 + 敏感键拒收

### TC-015-051 步骤 1-2：改模板后 renderedBody 仍是旧文案（快照语义）
- **AC**: AC-S-173 步骤 1-2 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 因果
- **前置**: `tenant-a` 有模板 `(order.shipped, smtp, zh-CN)`，`bodyTemplate = "您的订单 {{order_id}} 已发货"`；`tenant-a` 管理员持 `notify:template:write`
- **步骤**: 1) 入队一条并 `tick()` 成功，记下 `renderedTitle` / `renderedBody` 2) `PATCH` 该模板 `bodyTemplate` 改成完全不同的文案 `"订单已取消"` 3) 重新查那条**已存在的**投递记录
- **预期**: ① 步骤 1 的 `rendered_body` 是渲染后的具体文案（如 `"您的订单 A-1 已发货"`，**不是**存 `"{{order_id}}"` 原模板）；② 步骤 3 回读 `renderedBody` **逐字节等于**步骤 1 记下的值（**仍是旧文案**）；③ `renderedTitle` 同理不变；④ **负向断言**：回读 `renderedBody` **不含** `"订单已取消"` —— 若实现是运行时 join 模板表，这里就会变 → 红；⑤ 模板表本身**已经**是新文案（证明 PATCH 生效了，不是 PATCH 失败导致的假绿）

### TC-015-052 步骤 3：tenant-a 列表不含 tenant-b 的任何记录
- **AC**: AC-S-173 步骤 3 ｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 等价类
- **前置**: `tenant-a` 与 `tenant-b` 各有 ≥1 条投递记录（`scripts/seed-e2e.sh` 播种）；`tenant-a` 管理员持 `notify:delivery:read`
- **步骤**: 1) 以 `tenant-a` 令牌 `GET /api/notify/deliveries` 2) 以 `tenant-b` 令牌 `GET /api/notify/deliveries`
- **预期**: ① 两次都 **200**；② `tenant-a` 响应里**每一项**的 `tenantId` **逐字等于** `"tenant-a"`（逐项断言，不是只查第一条）；③ `tenant-a` 响应里**不存在** `tenant-b` 的记录 `deliveryId`（用 `tenant-b` 的 id 集合做交集，断言交集为空）；④ 两次响应的 `deliveryId` 集合**无交集**；⑤ `tenant-b` 侧同理 `tenantId` 全为 `"tenant-b"`

### TC-015-053 路径 2：4 种 camelCase 敏感键 → 4 次全部 400 NOTIFY_SENSITIVE_PAYLOAD
- **AC**: AC-S-173 步骤 4 ｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 等价类（非法输入）
- **前置**: 每次入队用**独立的**一条（`code` 唯一）；payload 必须用 **camelCase** 写法（平台端点口径，D167 / D172）
- **步骤**: `payload` 依次为 `{"password":"x"}` / `{"clientSecret":"x"}` / `{"apiKey":"x"}` / `{"authorization":"x"}` 各入队一次
- **预期**: ① **4 次全部 400**；② 4 次的 `code` 都**逐字**为 `NOTIFY_SENSITIVE_PAYLOAD`；③ **关键判据来源**：`normalize_sensitive_key` 会把 `clientSecret` 归一化成 `client_secret`、`apiKey` → `api_key`（`crates/platform-core/src/extension/audit.rs:452-453` 的表格）；若写成 snake_case 就**测不到**「驼峰能否命中」（D172 修的就是这个洞）；④ 库层回读：4 条都**未写入** `platform_notify_delivery`（拒收发生在写库之前）；⑤ 逐条分开断言 —— 不允许「4 个里命中 3 个就算过」，尤其是 `clientSecret` 与 `apiKey` 两个**驼峰**必须单独有断言

### TC-015-054 路径 3：近似词 `secretary` 不误伤，入队成功
- **AC**: AC-S-173 步骤 5 ｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 边界值（阳性对照的反面）
- **前置**: 同 TC-015-053 的入队通道
- **步骤**: `payload = {"secretary":"x"}`，入队
- **预期**: ① **入队成功**（201 或 202，非 400）；② `code` **不是** `NOTIFY_SENSITIVE_PAYLOAD`；③ 库层：该条**已写入**，`payload->>'secretary' == "x"`；④ 判据来源：`scan_sensitive_keys` 用**完整键名匹配**（`crates/platform-core/src/extension/audit.rs:430`），`secretary` 含 `secret` 子串但不等于 → 不得命中；⑤ 这条与 TC-015-053 合起来排除「子串匹配」这种过度拦截

### TC-015-055 路径 4：base_app 直连 + SET app.tenant_id → 只返回 tenant-a 的 1 条
- **AC**: AC-S-173 步骤 6 ｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库，`base_app` 角色）｜ **方法**: 因果
- **前置**: `base_app` 是应用连接角色（`base_platform_notify` 是 NOLOGIN 组角色，连不上，D165）；`tenant-a` 与 `tenant-b` 各有记录
- **步骤**: 1) 用 `base_app` 连库，**在单个事务内** `SET LOCAL app.tenant_id = 'tenant-a'` 2) `SELECT count(*) FROM platform_notify_delivery`（不带 WHERE）3) 同事务内 `SET LOCAL app.tenant_id = 'tenant-b'` 后重查
- **预期**: ① `tenant-a` 上下文下 `count == 1`（**只** tenant-a 那条，逐字等于 1）；② `tenant-b` 上下文下 `count == 1`；③ **关键**：SQL **没有** `WHERE tenant_id = ...`，隔离完全由 RLS policy 提供 —— 若 RLS 没建，两次都会返回 2 → 红；④ 上下文必须在**同一事务内** `set_config` → 查询（D148：`app.tenant_id` 是事务局部）；⑤ 未设置 `app.tenant_id` 时 `count == 0`（fail-closed，不是返回全表）

### TC-015-056 路径 5：列集合恰好 18 列（显式枚举，不比列数）
- **AC**: AC-S-173 步骤 7 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（`information_schema`）｜ **方法**: 边界值
- **前置**: 迁移已执行
- **步骤**: `SELECT column_name, data_type FROM information_schema.columns WHERE table_name='platform_notify_delivery' ORDER BY column_name`
- **预期**: ① 列名集合**双向相等**于以下 18 元素显式集合（**逐字枚举，不许写成「列数 = 18」** —— 列数对但列错会放过改名列）：`{attempt_count, category, channel, created_at, delivered_at, id, last_error, locale, next_attempt_at, payload, provider_message_id, recipient, rendered_body, rendered_title, status, template_code, tenant_id, updated_at}`；② `payload` 的 `data_type == "jsonb"`（**不是** `text` / `json`）；③ `delivered_at` 与 `last_error` 可空（`is_nullable == "YES"`）；④ 负向断言：**不含** `body_template` / `title_template` / `rendered_html` 这类不该在投递表里的列

### TC-015-057 投递记录出队索引存在且是部分索引
- **AC**: AC-S-173（表结构）＋ AC-S-170（出队依赖）｜ **类型**: 功能/可靠性 ｜ **优先级**: P1 ｜ **自动化**: 集成（`pg_indexes`）｜ **方法**: 边界值
- **前置**: 迁移已执行
- **步骤**: 查 `pg_indexes` 中 `tablename='platform_notify_delivery'` 的索引定义
- **预期**: ① 存在一个索引，列顺序**恰好** `(tenant_id, status, next_attempt_at)`；② 它是**部分索引**，`indexdef` **包含** `WHERE` 且谓词含 `pending` 与 `delivering`（design §2.3）；③ 负向断言：`indexdef` **不含** `dead_letter` / `delivered`（终态不进索引）