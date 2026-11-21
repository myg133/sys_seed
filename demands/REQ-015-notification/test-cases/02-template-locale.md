# 02 模板 CRUD、受限变量替换、多语言回退（AC-S-167 / AC-S-168）

> 测试落点：`crates/platform-notify/src/template.rs` 内联 `#[cfg(test)]`（**纯函数，无库无网**，
> AC-S-167）+ `crates/platform-notify/tests/notify_locale.rs`（**真实库**，AC-S-168）。
> 渲染接口 = 预览接口 `POST /api/notify/templates/{id}/preview`（design §3 表第 6 行，
> 「渲染接口即预览：给定 `payload` 返回渲染结果」）。

### TC-015-023 主路径：POST 建模板 → 201 且字段逐字回读
- **AC**: AC-S-167 步骤 1 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 等价类（合法输入）
- **前置**: `tenant-a` 管理员 `u-perm-notify@example.com` 持 `notify:template:read` + `notify:template:write`；`(tenant-a, "order.shipped", smtp, zh-CN)` 尚无行
- **步骤**: 1) `POST /api/notify/templates` body `{code:"order.shipped", channel:"smtp", locale:"zh-CN", category:"transactional", titleTemplate:"订单已发货", bodyTemplate:"你好 {{name}}，订单 {{order_id}} 已发货"}` 2) 读响应 3) `GET /api/notify/templates/{templateId}` 回读
- **预期**: ① HTTP **201**；② `templateId` 非空且是合法 UUID；③ `templateCode=="order.shipped"`、`channel=="smtp"`、`locale=="zh-CN"`、`category=="transactional"` 逐字等于请求；④ `titleTemplate` / `bodyTemplate` **逐字节**等于请求（占位符原样保留，建模板时**不**渲染）；⑤ `createdAt` 匹配 RFC3339 且小数位 **6 位**；⑥ 响应**不含** `tenantId` 之外的越权字段；⑦ 第 3 步回读与第 2 步响应**逐字**相同

### TC-015-024 列表与单个：数组语义 + 列表项与单个逐字相同
- **AC**: AC-S-167 步骤 2 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 等价类
- **前置**: TC-015-023 的模板已存在；同租户另有 2 条不同 `code` 的模板
- **步骤**: 1) `GET /api/notify/templates` 2) `GET /api/notify/templates/{templateId}`
- **预期**: ① 列表响应是**顶层数组**（不是 `{templates:[...]}`）；② 数组长度 == **3**（只含本租户 3 条）；③ 数组中 `templateCode=="order.shipped"` 那一项与第 2 步单个响应**逐字**相同（同 8 个字段全等）；④ 列表按 `createdAt` 倒序（最新在前）

### TC-015-025 步骤 3：PATCH 改 bodyTemplate → 200；DELETE → 204；再 GET → 404
- **AC**: AC-S-167 步骤 3 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 状态迁移
- **前置**: TC-015-023 的模板已存在，记下其 `titleTemplate` 原值 `"订单已发货"`
- **步骤**: 1) `PATCH /api/notify/templates/{templateId}` body `{bodyTemplate:"已发出，请注意查收"}` 2) `GET` 回读 3) `DELETE /api/notify/templates/{templateId}` 4) 再 `GET`
- **预期**: ① PATCH 返回 **200**；② 回读 `bodyTemplate=="已发出，请注意查收"`（逐字），且 `titleTemplate` **仍是** `"订单已发货"`（未提供字段不被清空 —— PATCH 语义不是 PUT）；③ `updatedAt` **大于** `createdAt`；④ DELETE 返回 **204** 且响应体为空；⑤ 第 4 步 `GET` 返回 **404** 且 `code=="NOTIFY_TEMPLATE_NOT_FOUND"`、`Content-Type: application/problem+json`

### TC-015-026 主路径渲染：`{{name}}`/`{{order_id}}` → 逐字等于期望串
- **AC**: AC-S-167 步骤 4 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数） + 集成（preview 端点）｜ **方法**: 等价类（合法输入）
- **前置**: 模板 `bodyTemplate = "你好 {{name}}，订单 {{order_id}} 已发货"`（TC-015-023 那条）
- **步骤**: 1) `POST /api/notify/templates/{templateId}/preview` body `{payload:{name:"张三", order_id:"A-1"}}` 2) 读 `renderedBody` 3) 同参数直接调纯函数 `render(template, payload)`
- **预期**: ① `renderedBody` **逐字节等于** `"你好 张三，订单 A-1 已发货"`（不是「包含 张三」这种弱断言）；② HTTP **200**；③ 响应里 `payload` 原样回显（`{name:"张三", order_id:"A-1"}`），便于前端对照；④ 纯函数 `render()` 的返回与 ① **逐字节相同**（同一实现两处一致）；⑤ 负向断言：`renderedBody` **不含** `"{{"` 与 `"}}"`

### TC-015-027 路径 2：变量名合法边界 4 种全部渲染成功
- **AC**: AC-S-167 步骤 5 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数）｜ **方法**: 边界值
- **前置**: 纯函数 `render` 可直接调用，无需建库
- **步骤**: 对变量名 `a` / `a1` / `a_1` / `abc_def_9` **各渲染一次**，模板为 `"值={{<var>}}"`，`payload` 为 `{<var>:"V"}`
- **预期**: ① **4 次全部成功**（4 个 `Ok`）；② 每次结果**逐字等于** `"值=V"`；③ 关键区分力：这 4 个名字覆盖白名单 `^[a-z][a-z0-9_]*$` 的**每一个字符类**（首字母字母、字母续接、数字续接、下划线、末位数字）—— 若白名单写窄成 `^[a-z]+$` 则 `a1`/`a_1`/`abc_def_9` 三条红

### TC-015-028 路径 3：5 种非法占位符 → 5 次全部 400 INVALID_PLACEHOLDER
- **AC**: AC-S-167 步骤 6 ｜ **类型**: 功能/安全 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数） + 集成（建模板即 400）｜ **方法**: 边界值
- **前置**: 每个非法写法用一条**独立**模板（`code` 各不相同，避免撞 409）
- **步骤**: 依次对 `{{Name}}`（大写）/ `{{1a}}`（数字开头）/ `{{a-b}}`（连字符）/ `{{}}`（空）/ `{{ a }}`（带空格）各建一个模板
- **预期**: ① **5 次全部 400**；② 5 次的 `code` 都**逐字**为 `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER`；③ `Content-Type: application/problem+json`；④ 逐条区分力：`{{Name}}` 只在大写字母上违规、`{{1a}}` 只在首字符上违规、`{{a-b}}` 只在非法符号上违规、`{{}}` 只长度为零、`{{ a }}` 只多空格 —— 5 条各打一个不同的规则，**不允许**用一条正则「整体不匹配就报错」把 4 条合法名（TC-015-027）也误杀；⑤ 库层回读：5 条**都没有**被写入 `platform_notify_template`

### TC-015-029 路径 4：未知变量 `{{missing}}` → 400 且错误体点名 `missing`
- **AC**: AC-S-167 步骤 7 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数） + 集成（preview）｜ **方法**: 错误推测
- **前置**: 模板 `bodyTemplate = "你好 {{name}} {{missing}}"`；`payload = {name:"张三"}`（**无** `missing`）
- **步骤**: 1) 调 `render` / preview 2) 读错误体全文
- **预期**: ① HTTP **400**；② `code=="NOTIFY_TEMPLATE_MISSING_VARIABLE"`；③ 错误体正文**包含子串 `"missing"`**（点名缺失的变量名，否则用户不知道补哪个）；④ 负向断言：错误体**不含** `"name"`（`name` 不缺，不该被点名 —— 防止实现把所有变量都列一遍）；⑤ 渲染**没有**产出半截结果：`renderedBody` 字段不存在于错误响应中

### TC-015-030 路径 5：多余变量 `extra` 不报错且不被代入
- **AC**: AC-S-167 步骤 8 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数）｜ **方法**: 等价类
- **前置**: 模板 `bodyTemplate = "你好 {{name}}"`；`payload = {name:"张三", extra:"SHOULD-NOT-APPEAR"}`
- **步骤**: 调 `render`
- **预期**: ① 返回 `Ok`（**不**报错 —— 多给变量不算错）；② 结果**逐字等于** `"你好 张三"`；③ **负向断言**：结果 **不含子串 `"SHOULD-NOT-APPEAR"`**，也**不含** `"extra"` —— 多余变量被忽略而不是被拼进正文

### TC-015-031 路径 6：同 (code, channel, locale) 再 POST → 409 DUPLICATED
- **AC**: AC-S-167 步骤 9 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 边界值
- **前置**: TC-015-023 的 `(order.shipped, smtp, zh-CN)` 已存在
- **步骤**: 用**完全相同**的三元组再 `POST` 一次（`bodyTemplate` 换文案，证明不是「内容相同才拒」）
- **预期**: ① HTTP **409**；② `code=="NOTIFY_TEMPLATE_DUPLICATED"`；③ 原行的 `bodyTemplate` **未被改写**（回读仍是旧文案）；④ 表里该三元组**只有 1 行**（`SELECT count(*)` == 1）

### TC-015-032 路径 7：GET 不存在的模板 → 404 NOTIFY_TEMPLATE_NOT_FOUND
- **AC**: AC-S-167 步骤 10 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 等价类（非法输入）
- **前置**: `tenant-a` 下不存在 `code == "does-not-exist"` 的模板
- **步骤**: `GET /api/notify/templates/does-not-exist`
- **预期**: ① HTTP **404**；② `code=="NOTIFY_TEMPLATE_NOT_FOUND"`（**不是**平台通用 `NOT_FOUND` —— 模板域有专用码，与 AC-S-172 步骤 4 的投递域口径不同）；③ `Content-Type: application/problem+json`；④ 响应体**不含**任何模板字段（不泄漏结构）

### TC-015-033 路径 8：SSTI 4 种载荷 → 4 次全部 INVALID_PLACEHOLDER
- **AC**: AC-S-167 步骤 11 ｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数） + 集成（建模板）｜ **方法**: 错误推测
- **前置**: 4 种载荷各建一条独立模板（`code` 各不相同）
- **步骤**: `bodyTemplate` 依次写 `{{#if x}}y{{/if}}` / `{{x.y}}` / `{{> partial}}` / `{{constructor.constructor('x')()}}`
- **预期**: ① **4 次全部**返回 `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER`；② 状态码 400；③ **安全判据**：`{{x.y}}` 与 `{{> partial}}` 若被当语法接受，等于把模板变成通用模板语言（design §4.1「不做条件/循环/表达式/部分引用」）；④ 库层回读：4 条都**未写入**表；⑤ 额外断言：`{{constructor.constructor('x')()}}` 这条**不得**产生任何函数调用副作用（若实现真的 eval 了，测试进程会出现 panic 或不可预期输出 —— 用 `catch_unwind` 包裹，断言 `is_err() == false` 且返回的是 `Err(INVALID_PLACEHOLDER)` 而非 `Ok`）

### TC-015-034 单趟替换：payload 值里的 `{{x}}` 不得被二次展开
- **AC**: AC-S-167 设计约束 §4.1 步骤 ④（安全要点）｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数）｜ **方法**: 错误推测
- **前置**: 模板 `bodyTemplate = "hi {{name}}"`；`payload = {name:"{{secret}}", secret:"LEAKED"}`
- **步骤**: 调 `render`
- **预期**: ① 结果**逐字等于** `"hi {{secret}}"`（替换进去的字面量**不再被扫描**）；② **负向断言**：结果 **不含 `"LEAKED"`** —— 若实现对结果再扫一遍，攻击者就能把 `{{secret}}` 放进 payload 值里二次展开，这条会红；③ 函数返回 `Ok`（不是 `Err`，因为 `{{secret}}` 是**字面量数据**不是模板语法）

### TC-015-035 渲染不修改原模板（预览是只读操作）
- **AC**: AC-S-167 步骤 4（预览语义）｜ **类型**: 功能 ｜ **优先级**: P1 ｜ **自动化**: 单测（纯函数）｜ **方法**: 因果
- **前置**: 模板 `bodyTemplate = "你好 {{name}}"`
- **步骤**: 1) 记下 `bodyTemplate` 原文 2) 调 `render` 两次（同 payload）3) 回读模板
- **预期**: ① 两次渲染结果**逐字节相同**（无隐藏状态、无缓存串味）；② 模板 `bodyTemplate` **仍是** `"你好 {{name}}"`（占位符没被就地替换掉）；③ 模板 `updatedAt` **未变**（预览不写库）

## AC-S-168：多语言按 locale 分行 + 三级回退

> 回退链**只有两级落点**：请求 locale → 租户默认 locale → **失败**（design §4.2）。
> ⚠️ **不是**「回退到任意一个存在的 locale」。

### TC-015-036 步骤 1：同 (code, channel) 的 zh-CN / en-US 两行能共存
- **AC**: AC-S-168 步骤 1 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 等价类
- **前置**: `tenant-a` 默认 locale = `zh-CN`（`platform-config` 的 `FALLBACK_LOCALES = &["zh-CN"]`）；`(order.shipped, smtp)` 下无行
- **步骤**: 1) `POST` 一条 `locale:"zh-CN"`、`bodyTemplate:"您的订单已发货"` 2) `POST` 一条 `locale:"en-US"`、`bodyTemplate:"Your order has shipped"`
- **预期**: ① 两次都 **201**（**不是 409** —— 这是判据核心：若 `UNIQUE` 漏了 `locale` 列，第 2 次会 409）；② 表里 `(tenant-a, order.shipped, smtp)` 下**恰好 2 行**，`locale` 分别是 `zh-CN` / `en-US`；③ 库层核对约束：`information_schema` 查 `platform_notify_template` 的唯一约束列集合**恰好** `{tenant_id, code, channel, locale}`（**4 列**）；④ 两行 `id` 不同

### TC-015-037 步骤 2：用户 locale = en-US → 渲染出 en-US 那行
- **AC**: AC-S-168 步骤 2 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 等价类
- **前置**: TC-015-036 的两行都在；请求上下文的用户 locale = `en-US`
- **步骤**: 渲染 `(order.shipped, smtp)`，`locale = en-US`
- **预期**: ① `renderedBody` **逐字节等于** `"Your order has shipped"`；② **负向断言**：**不等于** `"您的订单已发货"`（不是取「第一条」或「最新一条」）；③ 返回 `Ok`

### TC-015-038 路径 2：用户 locale = ja-JP（无该行）→ 回退到租户默认 zh-CN
- **AC**: AC-S-168 步骤 3 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: 只有 `zh-CN` / `en-US` 两行，**无** `ja-JP` 行；租户默认 = `zh-CN`；用户 locale = `ja-JP`
- **步骤**: 渲染 `(order.shipped, smtp)`
- **预期**: ① `renderedBody` **逐字节等于** `"您的订单已发货"`（**zh-CN 那行**）；② **关键负向断言**：**不是** `"Your order has shipped"` —— design §4.2 明确「`ja-JP` 无 `ja-JP` → `en-US` 的回退不存在，回退只看租户默认」；③ 返回 `Ok`（**不是** `NOTIFY_TEMPLATE_NOT_FOUND`）；④ 若实现写了「回退到任意存在 locale」或「回退到 en-US」，本条红

### TC-015-039 路径 3：删掉 zh-CN 行，用户 locale = ja-JP → 失败 NOTIFY_TEMPLATE_NOT_FOUND
- **AC**: AC-S-168 步骤 4 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: 删掉 `zh-CN` 那行，只留 `en-US`；租户默认仍是 `zh-CN`；用户 locale = `ja-JP`
- **步骤**: 渲染 `(order.shipped, smtp)`
- **预期**: ① 返回**失败**，`code=="NOTIFY_TEMPLATE_NOT_FOUND"`；② **负向断言**：**没有**渲染出 `"Your order has shipped"` —— 不得静默退到「任意一个存在的 locale」（那会让用户收到他看不懂的语言）；③ 错误响应**不含**任何 `renderedBody` 字段

### TC-015-040 路径 4：只留 en-US，用户 locale = zh-CN → 失败（不回退到 en-US）
- **AC**: AC-S-168 步骤 5 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: 只留 `en-US` 行；租户默认 = `zh-CN`；用户 locale = `zh-CN`
- **步骤**: 渲染 `(order.shipped, smtp)`
- **预期**: ① 返回**失败**，`code=="NOTIFY_TEMPLATE_NOT_FOUND"`；② `platform_notify_template` 里 `locale='en-US'` 那行**仍然存在**（失败不是因为它被删）；③ 判据含义：用户要 `zh-CN` 而库里有 `en-US` —— **不得**跨语言兜底；④ 与 TC-015-038 对照：同样「请求 locale 无行」，结果一个成功一个失败，取决于**租户默认 locale 那行在不在** —— 这两条一起证明回退链是「请求 → 租户默认 → 失败」而不是「请求 → 任意**

### TC-015-041 跨租户同名模板不互相可见（locale 回退不跨租户）
- **AC**: AC-S-168（隔离补强）＋ AC-S-173 租户隔离 ｜ **类型**: 安全 ｜ **优先级**: P1 ｜ **自动化**: 集成（真实库）｜ **方法**: 边界值
- **前置**: `tenant-b` 有 `(order.shipped, smtp, zh-CN)` 行，`tenant-a` **只有** `en-US` 行；`tenant-a` 用户 locale = `zh-CN`
- **步骤**: 以 `tenant-a` 上下文渲染 `(order.shipped, smtp)`
- **预期**: ① 返回 `NOTIFY_TEMPLATE_NOT_FOUND`（**不是**取到 `tenant-b` 的 zh-CN 行）；② RLS 在库层生效：以 `base_app` + `SET app.tenant_id='tenant-a'` 查该表，`count(*)` **只**含 tenant-a 的行；③ 断言不允许出现「跨租户兜底命中」
