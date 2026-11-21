# 验收标准：通知系统

- **需求编号**: REQ-015
- **父需求**: REQ-001
- **状态**: **rev 3**（2026-11-21 —— rev 2 补齐 10 条 AC；rev 3 按 QA 挑刺 11 条回填判据）
- **对应验收标准**: **10 条**（AC-S-023 **重写** + **AC-S-166 ~ AC-S-174**）+
  **1 个「明确不做」清单**（AC-S-166.b，**不是 AC**，是防止后人以为漏了的说明）
- **决策依据**: [decisions.md](decisions.md) **rev 3（D291~D318）**
- **基线代码**: `develop` @ `a2b7e52`（REQ-016 已合并）
- **更新时间**: 2026-11-21

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## 需求来源披露（D122）

**REQ-015 不是用户原话需求**。用户 2026-11-17 的 9 项清单里没有「通知」；
`roadmap.md` 把本需求标为「⚠️ **BA 补**」，用户此后裁定「出了工作流，其他的继续做」→ 继续。
**10 条 AC 的具体形态全部是 BA 的设计**，每条都有 `decisions.md` 里的决策依据。

---

## rev 2 变更摘要（**Dev 必读**）

rev 1 只有 **1 条空泛 AC**（AC-S-023，正文是「行为与本条标题一致」——
**没有任何可断言的事实**），而 `demand.md` 列了 **5 项功能要求** →
**1 条 AC 验收不了 5 项功能**。rev 2 补齐到 **10 条**。

**三个实测发现的缺口**（在 `develop` @ `a2b7e52` 上验的）：

1. **`NotificationSender` trait 已存在但全仓 0 个实现**
   （`crates/platform-core/src/extension/notification.rs:81`；
   唯一「实现」是 `platform-org/src/notify.rs:211` 里测试用的 `RecordingSender`）
   → 通道实现是**从零建**的。
2. **`platform-server` 没有任何后台任务**
   （`grep -c "JoinSet\|tokio::spawn\|interval("` = **0**）
   → 异步投递的 worker 是**从零建**的（D296 定成显式 CLI 命令）。
3. **`Cargo.lock` 里 0 个 SMTP 库**
   → 但这**不等于**邮件做不了：SMTP 是纯文本行协议，`tokio`（`net`）+ `base64` 已在树里，
   TLS 的 `rustls` 也已在（D123）→ **D305 决定自己写**（用户 2026-11-21 裁定）。

> ⚠️ **rev 1 犯过的错，记下来防复发**：rev 1 的 D292 用「lock 里没有 SMTP 库」
> 作为「邮件不做」的理由 —— **那是把「没有现成库」当成「做不出来」**，
> 并因此让父 AC-023 的明文要求「邮件送达」落空。**规格不许用错误的技术判断降低父 AC 的要求。**

---

## 全局约定

| 项 | 约定 | 依据 |
|---|---|---|
| 应用连接角色 | **`base_app`**（`base_platform_notify` 是 NOLOGIN 组角色，连不上） | D165 |
| 造数 / 篡改连接 | **`base`**（表 owner + 超管） | — |
| 库 | **`base_test`**（绝不写 `base_dev`） | dev-guide §9.8 |
| `app.tenant_id` | **事务局部**；凡涉及租户上下文的查询**必须在单个事务内** `set_config` → 查询 | D148 |
| 错误体 | **一律 Problem Details**（RFC 9457，契约 C6）—— 本需求**没有**协议端点 | 契约 C6 |
| 字段名 | **camelCase**（契约 C13） | D167 |
| 时间格式 | UTC、RFC3339、**微秒 6 位** | D142 |
| 静态扫描 | 必须带**阳性对照**（否则「0 命中」不可信） | dev-guide §9.2 |
| 判据落点 | 报告必须指到 `file:line` 的 `assert!`，且**同时给函数名**；AC 里的字段清单要逐个断言 | **dev-guide §9.15 / §9.15.1 / §9.15.2** |
| AC 里的数字 | 必须有 `file:line` 或实测命令支撑，**不许凭直觉** | **D275** |

### 本需求的平台错误码（**8 个**）

| 码 | HTTP | 何时 | 哪个 AC 触发它 |
|---|---|---|---|
| `NOTIFY_TEMPLATE_NOT_FOUND` | 404 | 模板不存在 | AC-S-167 步骤 10 |
| `NOTIFY_TEMPLATE_DUPLICATED` | 409 | 同 (code, channel, locale) 重复 | AC-S-167 步骤 9 |
| `NOTIFY_TEMPLATE_MISSING_VARIABLE` | 400 | 渲染时变量缺失 | AC-S-167 步骤 8 |
| `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER` | 400 | `{{...}}` 里是非法变量名 | AC-S-167 步骤 7 |
| `NOTIFY_CHANNEL_NOT_CONFIGURED` | 400 | 通道未配置（短信 / IM） | AC-S-166 步骤 6 与 7 |
| `NOTIFY_CATEGORY_MANDATORY` | 400 | 试图退订 `system` 类 | AC-S-169 步骤 5 |
| `NOTIFY_NOT_DEAD_LETTER` | 409 | 重投一个非死信的记录 | AC-S-172 步骤 4 |
| `NOTIFY_SENSITIVE_PAYLOAD` | 400 | `payload` 含敏感键 | AC-S-173 步骤 6 |

> ⚠️ **规则（沿用 REQ-016 / REQ-022）**：**每个错误码必须有一个 AC 会真的触发它**。
> 上表每个码都指到了具体步骤；**任何一个找不到触发点 → 删掉它**，不许「先登记着」。
>
> ⚠️ **`TITLES` 行数：99 → 107**（实测基线 **99** 条、0 个重复 code，
> 见 `crates/platform-core/src/titles.rs:45`；+8 = 107）。
> ⚠️ 事件码（**4 个** `notify.*`：`notify.template.created` / `notify.template.updated` /
> `notify.template.deleted` / `notify.dead_letter.resent`）**不进 `TITLES`**（D223 同款口径）。

### 本需求的配置项（**5 个**，全在 manifest 的 `config` 段）

| 键 | 类型 | 默认 | scope |
|---|---|---|---|
| `notify.maxAttempts` | `number` | `5` | **platform** |
| `notify.retryBaseSeconds` | `number` | `30` | **platform** |
| `notify.retryMultiplier` | `number` | `2` | **platform** |
| `notify.retryMaxSeconds` | `number` | `3600` | **platform** |
| `notify.deliveringTimeoutSeconds` | `number` | `300` | **platform** |

> ⚠️ 第 5 个（`deliveringTimeoutSeconds`）是 **D313 / S-06** 加的：
> `delivering` 残留状态的回收阈值必须可配，否则「10 分钟」是魔数，
> 且没有它写不出「未超阈值**不**回收」的阴性对照。
>
> ⚠️ 每个键**必须**带 `"scope": "platform"`（REQ-016 的 S-01 教训：
> 漏写会落到 `Tenant` → 平台端点直接拒 → 配置永不生效）。
> ⚠️ `config` 段必须是 `ConfigSpec` **对象**，不是裸值（REQ-016 的 S-02 教训：裸值 → 服务起不来）。
> ⚠️ **SMTP 的连接信息与密码不进这 5 项**（`ConfigSpec` 无 `Secret` 类型，
> `crates/platform-core/src/extension/manifest.rs:189-207`）→ 走环境变量，见 AC-S-166 步骤 8。

---

## 本需求自有 AC

---

### AC-S-023: 死信可重投，且**不重复投递已成功的**

> ⚠️ **rev 2 重写**。rev 1 的内容是：
> 「**测试步骤**: 1. 按需求描述构造前置状态 2. 执行该能力对应的操作序列
> **预期结果**: 行为与本条标题一致，且失败路径有明确错误码」
> —— **这两行没有任何可断言的事实**（「按需求描述构造」= 循环引用；
> 「行为与标题一致」= 把判据交给标题）。无法据此写出一个会红的测试。

- **前置条件**:
  - `tenant-a` 下有管理员 `u-perm-notify@example.com`，持 `notify:delivery:read`；
  - 站内信通道已配置（**不需要任何外部服务**）；
  - 用**可控失败的假 SMTP 服务器**（D305 的夹具，`TcpListener::bind("127.0.0.1:0")`）
    让一条通知失败到次数用尽。
- **测试步骤**:
  1. 入队 **2 条**通知（站内信 + 邮件），都发同一收件人；
  2. 让站内信那条走 `tick()` 成功；让邮件那条连续 `tick()` 至 `maxAttempts` 用尽 → 进死信；
  3. 记下站内信那条的 `deliveryId`（记为 `D_ok`）与邮件那条的 `deliveryId`（记为 `D_dead`），
     以及 `D_ok` 的 `deliveredAt`；
  4. `POST /api/notify/deliveries/{D_dead}/resend`（重投死信）；
  5. 修好假 SMTP 服务器（改成全程成功），再 `tick()` 一次；
  6. 重新查 `D_ok` 与 `D_dead` 两行。
- **预期结果**:
  - 步骤 2：`D_dead.status == "dead_letter"` 且 `attempt_count == 5`（= `notify.maxAttempts`）；
  - 步骤 4：**200**，返回的行 `status == "pending"` 且 **`attempt_count == 0`**；
  - 步骤 5 后：`D_dead.status == "delivered"`，`attempt_count` **恰好 `1`**
    （不是 5、不是 6 —— 证明计数真的被重置了，而不是「接着第 6 次」）；
  - 步骤 6：**`D_ok` 的 `deliveredAt` 逐字节不变**，`attempt_count` **仍是 `1`**，
    `status` **仍是 `delivered`**
    —— ⚠️ **这一条是「不重复投递已成功的」的全部判据**；
    同时站内信 provider 的调用计数**仍是 1**（用计数型夹具 `RecordingSender` 数，不是看数据库状态：
    数据库状态没变**不代表**没重复发，可能发了但被覆盖）。
  - **两条判据互相独立**：只重置计数不改状态 → 步骤 5 红；重投时把已成功的也一起重置 →
    步骤 6 红。
- **类型**: 功能

---

### AC-S-166: 通道抽象 —— `NotificationSender` 注册表 + **3 个真实现** + 2 个显式「未配置」

> **不改 trait 签名**（D291）：`crates/platform-core/src/extension/notification.rs:81-91` 是
> REQ-003 冻结的扩展点契约（`crates/xtask/src/capabilities.rs:45-51` 的
> `EXTENSION_POINT_TRAITS` 含 `NotificationSender`）→ 改它会让所有 `manifest.json`
> 的 `dependencies` 跟着动。

- **前置条件**:
  - `tenant-a` 存在；SMTP 的 5 个环境变量按 D306 设置（指向假 SMTP 服务器）；
  - Webhook 指向一个 `page.route` / 假 HTTP 服务器。
- **测试步骤**（**主路径 + 9 条独立路径**）:
  1. （主）按 `channel()` 名字把 3 个 provider 装进 `ChannelRegistry`，逐个用 `send()` 投递；
  2. 查 `ChannelRegistry` 注册的通道名集合；
  3. **路径 2**：向短信通道投递 → 读错误；
  4. **路径 3**：向 IM 通道（`dingtalk` / `wecom` / `feishu` **各一次**）投递 → 读错误；
  5. **路径 4**：把假 SMTP 服务器设成 `AUTH` 回 `535`，向邮件通道投递；
  6. **路径 5**：把假 SMTP 服务器设成对**第 2 个** `RCPT TO` 回 `550`，投递 2 个收件人；
  7. **路径 6**：把假 SMTP 服务器设成「连上但不发 `220`」；
  8. 检查 `SmtpConfig` 的 `Debug` 输出；
  9. **路径 7（头注入）**：`payload = {userName: "x\r\nBcc: attacker@evil.example"}`，
     模板 `{{userName}}` 渲染进 `Subject`，向邮件通道投递；
  10. **路径 8（base64 折行）**：投递一条正文长度使 base64 超过 **76** 字符的通知；
  11. **路径 9（STARTTLS 后重发 EHLO）**：假服务器设成
      `auth_only_after_tls: true`（**只在 TLS 握手之后的 `EHLO`** 响应里带 `250-AUTH LOGIN`），
      向邮件通道投递；
  12. **路径 10（未配置形态）**：对 4 个未配置通道的返回值做**类型判别**
      （是 `Err` 还是 `Ok`）。
- **预期结果**:
  - 步骤 1：站内信 → `delivered == true`；邮件 → `delivered == true` 且
    `provider_message_id.is_some()`（**必须逐字等于**假服务器在 `DATA` 后回的那行 250 里
    写死的队列 ID，证明取的是服务端响应而不是自造 uuid）；Webhook → `delivered == true`；
  - 步骤 2：**恰好**是 `["in_app", "smtp", "webhook", "sms", "dingtalk", "wecom", "feishu"]`
    —— ⚠️ 后 4 个**也在注册表里**（它们必须存在以返回明确错误）；
  - 步骤 3/4：**4 次全部** `Err`，错误码 `NOTIFY_CHANNEL_NOT_CONFIGURED`
    （⚠️ **不是** `Ok(DeliveryResult { delivered: false })`、**不是**静默成功 —— D292 的铁律）；
  - 步骤 5：`Err`，`error` 含 **`535`**（证明服务端应答码被带出来了，不是「认证失败」这种泛化文案）；
    且 `error` **不含**密码明文（含 base64 后的密码串也不行）；
  - 步骤 6：整条 `Err`，`error` 含 **`550`**（**任一收件人被拒 → 整条失败**，不部分成功）；
  - 步骤 7：`Err`，且 **`10s <= elapsed <= 12s`**（⚠️ **必须断言实际耗时区间**：
    下界证明真的等了 10 秒而不是立刻失败，上界防止实现把 timeout 写成 60 秒而判据仍然绿），
    且 `error` 含超时语义；
  - 步骤 8：**不含**密码明文 —— 断言 `format!("{:?}", cfg)` 里
    **不包含**环境变量 `BASE_SMTP_PASSWORD` 的值，且包含 `"***"`；
    **阳性对照**：同时断言该 `Debug` 输出里**包含** `BASE_SMTP_HOST` 的值（证明 `Debug` 不是全空）；
  - 步骤 9（**安全判据**）：假服务器**实际收到的 `DATA` 段**里
    **`Bcc:` 出现 0 次**；
    **阳性对照**：同一份 `DATA` 里 **`To:` 出现 1 次**
    （证明扫的是真收到的内容而不是空串 —— 否则「什么都没收到」也会让上一条绿）；
    ⚠️ **不许**用「返回了错」代替本条 —— 那是另一件事；
  - 步骤 10：假服务器收到的 `DATA` 正文里，**除最后一行外每行 base64 长度恰好 76**，
    且总行数 = `ceil(len(base64) / 76)`
    （⚠️ **必须逐行算长度**；只断言「含换行」对「整段一行」也成立 → 零区分力）；
  - 步骤 11：① 投递成功（`delivered == true`）；② 假服务器记录的 **`EHLO` 次数恰好 2**
    （TLS 前 1 + TLS 后 1）。
    ⚠️ **两条缺一不可**：只断言「成功」不够 —— 客户端可能压根没做 STARTTLS
    就直接明文认证了（在某些服务器上也能成功，但**密码明文上网**）；
  - 步骤 12：4 个未配置通道的返回值 **全部是 `Err`**；
    **不得**出现 `Ok(_)` —— 包括 `Ok(DeliveryResult { delivered: false, .. })`
    （⚠️ 后者语义是「投递完成但失败」→ worker 会走**重试**分支
    → 对未配置的通道白重试 5 次才进死信，与 D292 冲突）。
- **类型**: 功能 / 安全

### AC-S-166.b: **明确不做**的事项（写进规格，防止「后面以为漏了」）

| 不做 | 为什么 |
|---|---|
| ❌ 短信真实现 | 无厂商 SDK + **无真实凭据**（短信 API 要实名模板报备）→ 写出来判据恒真 |
| ❌ IM（钉钉 / 企微 / 飞书）真实现 | 三者都要**先换 token 或算签名**，无凭据无法验签 → 同上 |
| ❌ SMTP 的隐式 TLS（465 端口） | 只做 STARTTLS（587）；465 需真实验证 → **TD-055** |
| ❌ SMTP 的 `AUTH PLAIN` / `CRAM-MD5` / `XOAUTH2` | 只做 `AUTH LOGIN`；不支持的机制**显式报错**，不静默降级 |
| ❌ 多部分 MIME（附件 / 内嵌图） | 通知正文只有 title + body；附件属 REQ-018 |

→ 若要做短信/IM，需要：**真实凭据** + **各平台的签名算法** + **一个能回放的录制夹具**
（`TD-054`）。

---

### AC-S-167: 模板 CRUD + **受限变量替换** + 未知变量显式失败

> **只做 `{{variable}}` 替换**（D293）：**不做**条件 / 循环 / 表达式 / 函数调用。
> 理由有二：**零新增依赖**；且租户管理员能编辑模板 → 支持表达式等于给他
> **服务端模板注入（SSTI）** 的入口。

- **前置条件**: `tenant-a` 下有管理员持 `notify:template:read` / `notify:template:write`。
- **测试步骤**（**主路径 + 8 条独立路径**）:
  1. （主）`POST /api/notify/templates`，body 含 `code` / `channel` / `locale` / `category` /
     `titleTemplate` / `bodyTemplate`；
  2. `GET /api/notify/templates` 列出；`GET /api/notify/templates/{id}` 读单个；
  3. （主）`PATCH` 改 `bodyTemplate`；`DELETE` 后重查；
  4. **渲染**：`bodyTemplate = "你好 {{name}}，订单 {{order_id}} 已发货"`，
     `payload = {name: "张三", order_id: "A-1"}`；
  5. **路径 2（变量名合法边界）**：变量名依次用 `a` / `a1` / `a_1` / `abc_def_9` 各渲染一次；
  6. **路径 3（非法变量名）**：`{{Name}}`（大写）/ `{{1a}}`（数字开头）/
     `{{a-b}}`（连字符）/ `{{}}`（空）/ `{{ a }}`（带空格）各建一个模板；
  7. **路径 4（未知变量）**：模板用 `{{missing}}`，`payload` 里**没有** `missing`；
  8. **路径 5（多余变量）**：模板只用 `{{name}}`，`payload` 里**多给** `extra`；
  9. **路径 6（重复）**：同 `(code, channel, locale)` 再 `POST` 一次；
  10. **路径 7（不存在）**：`GET /api/notify/templates/does-not-exist`；
  11. **路径 8（SSTI 尝试）**：`bodyTemplate` 依次写
      `{{#if x}}y{{/if}}` / `{{x.y}}` / `{{> partial}}` / `{{constructor.constructor('x')()}}`。
- **预期结果**:
  - 步骤 2/3：字段**逐字**回读一致；`DELETE` 后 `GET` → **404** + `NOTIFY_TEMPLATE_NOT_FOUND`；
  - 步骤 4：渲染结果 **逐字等于** `"你好 张三，订单 A-1 已发货"`；
  - 步骤 5：**4 次全部成功**（证明白名单 `[a-z][a-z0-9_]*` 的正向边界没写窄）；
  - 步骤 6：**5 次全部 400** + `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER`；
  - 步骤 7：渲染 **400** + `NOTIFY_TEMPLATE_MISSING_VARIABLE`，
    错误体里**点名** `missing`（否则用户不知道该补哪个变量）；
  - 步骤 8：**成功**，且 `extra` **不出现**在渲染结果里
    （多给变量不算错，但不得被代入）；
  - 步骤 9：**409** + `NOTIFY_TEMPLATE_DUPLICATED`；
  - 步骤 10：**404** + `NOTIFY_TEMPLATE_NOT_FOUND`；
  - 步骤 11：**4 次全部** `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER`
    —— ⚠️ **这是安全判据**：`{{x.y}}` 与 `{{> partial}}` 若被当语法接受，
    等于把模板变成了通用模板语言；`{{constructor.constructor(...)}}` 是 SSTI 的经典载荷。
    ⚠️ **必须用 `std::panic::catch_unwind` 包住渲染调用**，同时断言：
    ① **不 panic**（若载荷导致索引越界等 panic，测试会以 panic 结束 →
    看起来是「测试崩了」而不是「发现漏洞」，会掩盖问题）；
    ② 返回 `Err` 且错误码为 `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER`。
    **两条缺一不可。**
- **类型**: 功能 / 安全

---

### AC-S-168: 模板**多语言**按 locale 分行 + **三级回退**

- **前置条件**: `tenant-a` 的默认 locale = `zh-CN`（`crates/platform-config/src/registry.rs:44`
  的 `FALLBACK_LOCALES = &["zh-CN"]`）；同 `code` + `channel` 下备 3 个 locale 行。
- **测试步骤**（**主路径 + 4 条独立路径**）:
  1. 为同一 `(code, channel)` 写入 `zh-CN` / `en-US` **两行**（不同 `bodyTemplate` 文案）；
  2. 用户 locale = `en-US` → 渲染；
  3. **路径 2**：用户 locale = `ja-JP`（**没有这行**）→ 渲染；
  4. **路径 3**：删掉 `zh-CN` 那行，用户 locale = `ja-JP` → 渲染；
  5. **路径 4**：只留 `en-US` 行，用户 locale = `zh-CN` → 渲染。
- **预期结果**:
  - 步骤 2：渲染出 **`en-US` 那行的文案**（逐字）；
  - 步骤 3：回退到租户默认 `zh-CN` → 渲染出 **`zh-CN` 那行**（⚠️ 不是 `en-US`，
    也不是报错；`ja-JP` 无 `ja-JP` → `en-US` 的回退，**回退只看租户默认**）；
  - 步骤 4：**渲染失败**，`NOTIFY_TEMPLATE_NOT_FOUND`
    （⚠️ **不得**静默退到「任意一个存在的 locale」—— 那会让用户收到他看不懂的语言）；
  - 步骤 5：**渲染失败**，`NOTIFY_TEMPLATE_NOT_FOUND`（同理，不回退到 `en-US`）；
  - 步骤 1 的库层判据：`platform_notify_template` 上有
    **`UNIQUE (tenant_id, code, channel, locale)`** —— 同一 `(code, channel)` 的
    **两个 locale 能共存**（若唯一约束漏了 `locale`，步骤 1 就会 409）。
- **类型**: 功能

---

### AC-S-169: 订阅偏好 —— 按 `category` 退订 + **`system` 类不可退订**

- **前置条件**: `tenant-a` 下有普通用户 `u-notify@example.com`（**不需要任何权限点**，
  登录即可改自己的偏好 —— D300）；存在 `marketing` 与 `system` 两个 category 的模板。
- **测试步骤**:
  1. 入队一条 `marketing` 通知给该用户，`tick()`；
  2. `PUT /api/notify/preferences`，body `{category: "marketing", optedOut: true}`；
  3. 再入队一条 `marketing` 通知，`tick()`；
  4. `GET /api/notify/preferences`；
  5. 对 `system` 执行同样的 `PUT`；
  6. 入队一条 `system` 通知（用户已退订 `marketing` 不影响它），`tick()`；
  7. 再 `PUT` 一次 `marketing`，`optedOut: false`（**重新订阅**），入队并 `tick()`。
- **预期结果**:
  - 步骤 1：投递成功（`delivered`），站内信 provider 计数 +1；
  - 步骤 3：**该条进死信**，`status == "dead_letter"`，
    `last_error` 指明「用户已退订 `marketing`」，
    且 **`attempt_count == 0`**
    （⚠️ **钉死为 0**：退订检查必须在**投递尝试之前**完成 ——
    「用户已退订」是**确定性终态**，重试 5 次不会改变结论 → 重试纯属浪费且拖慢 worker；
    若留空间给 `1`，「退订后还重试 5 次」这种明显错误的实现也能通过，
    因为它的 `last_error` 同样与退订相关）；
    ⚠️ **不是 `delivered`、不是 `pending` 静默丢弃**：退订必须**留下可查的记录**
    （用户投诉「为什么没收到」时能回答）；
  - 步骤 4：返回该用户的偏好列表，含 `{category: "marketing", optedOut: true}`；
  - 步骤 5：**400** + `NOTIFY_CATEGORY_MANDATORY`；
    ⚠️ 且**偏好表里不得出现 `system` 行**（用 `SELECT` 查库断言行数 = 0：
    若先写后报错，用户的 `system` 就真的被关掉了）；
  - 步骤 6：投递**成功**（`system` 类不受 `marketing` 退订影响，且它本身不可退订）；
  - 步骤 7：投递**成功**（退订可逆）。
- **类型**: 功能 / 安全

---

### AC-S-170: 异步投递 —— `FOR UPDATE SKIP LOCKED` + **多 worker 不重复不遗漏**

> **数据库驱动的显式 worker**（D296）：`xtask notify worker` / `tick()`。
> **不是**进程内队列 —— 进程重启会丢消息、多实例会重复投递。

- **前置条件**: 库已建；`base` 连接可直连（造数）。
- **测试步骤**（**主路径 + 3 条独立路径**）:
  1. 入队 **100 条**可立即投递的通知（`next_attempt_at <= now`）；
  2. **并发**跑 **4 个** `tick()`（`tokio::join!` 4 个 future，各自独立事务），每批取 30；
  3. 汇总：查 provider 的总调用次数与 100 条的最终状态；
  4. **路径 2（未到期不取）**：入队 1 条 `next_attempt_at = now + 1 小时`，`tick()` 一次；
  5. **路径 3（崩溃恢复 + 阴性对照）**：
     - **3a（超阈值）**：把一条置成 `status = "delivering"` 且 `updated_at` 是
       `now - notify.deliveringTimeoutSeconds - 60s`（默认阈值 300 → 即 6 分钟前），`tick()` 一次；
     - **3b（未超阈值）**：把另一条置成 `status = "delivering"` 且 `updated_at = now - 5s`，
       `tick()` 一次；
  6. **路径 4（幂等）**：对已 `delivered` 的行再 `tick()` 三次。
- **预期结果**:
  - 步骤 2/3：100 条**全部 `delivered`**（不遗漏），且 provider 总调用次数
    **恰好 100**（不重复）——
    ⚠️ **两个数字都要断言**：只断言「全部 delivered」的话，
    「投两次但状态只记一次」这种重复投递会漏过去；
  - 步骤 4：那条**仍是 `pending`**（`next_attempt_at` 未到时不许取）；
  - 步骤 5-3a：那条被**重新取到并投递**（`delivering` 是崩溃残留状态，
    超过阈值必须回收 —— 否则一条消息永远卡住）→ 最终 `delivered`；
  - 步骤 5-3b：那条 **`status` 仍是 `delivering`**，且 provider 调用次数**不变**
    （⚠️ **这条是阴性对照，缺了它就有零区分力**：
    一个「只要看到 `delivering` 就抢」的实现也能过 3a，
    而它会在**正常投递进行中**把行抢走 → 同一通知被两个 worker 同时投递
    → **重复投递**，正是 D296 要避免的事）；
  - 步骤 6：3 次 `tick()` 后 provider 调用次数**不再增加**
    （`delivered` 是终态，不参与出队）。
- **类型**: 功能 / 可靠性

> ⚠️ **阈值必须来自配置 `notify.deliveringTimeoutSeconds`（默认 300）**，不是代码里的魔数。
> 依据 **D313 / S-06**。

---

### AC-S-171: 重试退避 —— 指数 + 上限 + 次数用尽进死信

- **前置条件**: 配置为默认值（`maxAttempts=5` / `retryBaseSeconds=30` / `retryMultiplier=2` /
  `retryMaxSeconds=3600`）；一个**恒定失败**的假 SMTP 服务器。
- **测试步骤**（**主路径 + 3 条独立路径**）:
  1. 入队 1 条邮件通知，连续 `tick()` 5 次（每次把 `next_attempt_at` 视为已到期 —— 见步骤 2）；
  2. **只单测退避函数**（不跑 worker）：`backoff(attempt)` 对 `attempt = 1,2,3,4` 的返回值；
  3. **路径 2（上限）**：把 `retryBaseSeconds` 设为 `2000`、`retryMultiplier` 设为 `2`、
     `retryMaxSeconds` 设为 `3600`，算 `attempt = 2` 的延迟；
  4. **路径 3（配置生效）**：把 `maxAttempts` 改成 `2`，同样入队后 tick 到死信；
  5. **路径 4（用 `time` 不用浮点）**：检查退避计算里没有 `f64` / `as_secs_f64`。
- **预期结果**:
  - 步骤 1：第 1~4 次失败后 `status == "pending"`，`attempt_count` 依次 `1,2,3,4`；
    **第 5 次**失败后 `status == "dead_letter"`（**不是**第 6 次 —— `maxAttempts` 含首次）；
  - 步骤 2：**恰好** `[30, 60, 120, 240]` 秒
    （⚠️ **不是 5 个值**：第 5 次失败直接进死信，不再算延迟）；
  - 步骤 3：**恰好 3600**（`2000 * 2 = 4000 > 3600` → 被上限截断；
    若没有上限就是 4000，**这条区分力全部来自上限**）；
  - 步骤 4：**第 2 次**失败后就 `dead_letter`（证明配置真的被读了，不是写死 5 次）；
  - 步骤 5：`grep -c "f64\|as_secs_f64"` = **0**，且阳性对照
    `grep -c "Duration::seconds"` > 0（证明扫的是真代码文件，不是扫空）。
- **类型**: 功能

---

### AC-S-172: 死信人工重投 —— **只重置死信条目** + 影响行数守卫

- **前置条件**: 已有 1 条 `dead_letter` 记录 `D_dead`、1 条 `delivered` 记录 `D_ok`、
  1 条 `pending` 记录 `D_pending`；管理员持 `notify:delivery:read`。
- **测试步骤**（**主路径 + 3 条独立路径**）:
  1. `POST /api/notify/deliveries/{D_dead}/resend`；
  2. `POST /api/notify/deliveries/{D_ok}/resend`；
  3. `POST /api/notify/deliveries/{D_pending}/resend`；
  4. `POST /api/notify/deliveries/does-not-exist/resend`；
  5. **并发**：对 `D_dead` 同时发 2 个 `resend`。
- **预期结果**:
  - 步骤 1：**200**；`status == "pending"`、`attempt_count == 0`、
    `next_attempt_at <= now`、`last_error` 被清空（`None`）；
  - 步骤 2/3：**409** + `NOTIFY_NOT_DEAD_LETTER`；且这两行的
    `status` / `attempt_count` / `next_attempt_at` **逐字段不变**；
  - 步骤 4：**404** + **`NOT_FOUND`**（平台通用码；本需求**不新增**专用码，
    理由见本条末尾的说明）；
  - 步骤 5：**两次的 `rows_affected` 集合恰好是 `{1, 0}`**
    （⚠️ **不是** `{1}`，也**不是**「两个都 1」）。
    **必须用 `WHERE status = 'dead_letter'` 的影响行数做判据**，
    **不是**「先 SELECT 查状态、再 UPDATE」：后者在并发下两条都读到 `dead_letter` → 两条都成功。
    ⚠️ **并发必须真的并发**（**D314 / S-07**，构造法二选一，Dev 在交付说明里写明选了哪个）：
    - (a) `tokio::sync::Barrier::new(2)` 让两个 task 同一时刻发 HTTP 请求；
    - (b) **更可靠**：两个独立 `TenantTx`，各自 `execute` 同一条 UPDATE 语句。
    **若写成串行**（先发一次、再发一次），第一个已改成 `pending`、第二个自然 409
    → **判据是绿的，但根本没测并发**。
- **类型**: 功能 / 可靠性

> ⚠️ **步骤 4 的码口径**：本需求**不新增**「投递记录不存在」专用码。
> 理由：一个**只有一条 AC 的 404 分支**不值得占 `TITLES` 一行 + 两条 i18n 词条；
> 用平台通用的 `NOT_FOUND` 语义一致（沿用 REQ-016 删死码的同一逻辑 ——
> 「每个错误码必须有 AC 真的触发它」，反过来「一个 AC 分支不值得一个新码」）。
> **若 Dev 认为需要专用码，必须先回报 BA**（不许自行加）。

---

### AC-S-173: 投递记录 —— **快照** + 租户隔离 + **敏感键拒收**

- **前置条件**: `tenant-a` 与 `tenant-b` 各有 1 条投递记录（`scripts/seed-e2e.sh` 播种）；
  `crates/platform-notify/manifest.json` 含 `tenantScoped` 段。
- **测试步骤**（**主路径 + 5 条独立路径**）:
  1. 入队一条，记下 `renderedTitle` / `renderedBody`；
  2. **改模板的 `bodyTemplate`**（换成完全不同的文案），再查那条记录的 `renderedBody`；
  3. `GET /api/notify/deliveries`（`tenant-a` 管理员）；再用 `tenant-b` 的令牌查；
  4. **路径 2（敏感键）**：`payload` 依次含
     `{"password": "x"}` / `{"clientSecret": "x"}` / `{"apiKey": "x"}` /
     `{"authorization": "x"}` 各入队一次
     ⚠️ **必须用 camelCase 写法**（平台端点口径，D167）——
     实测依据：`normalize_sensitive_key` 会把 `clientSecret` 归一化成 `client_secret`、
     `apiKey` → `api_key`（`crates/platform-core/src/extension/audit.rs:452-453` 的表格）；
     若写成 snake_case 就**测不到**「驼峰能否命中」这条（D172 修的就是这个洞）；
  5. **路径 3（阳性对照）**：`payload = {"secretary": "x"}`（**不应**命中的近似词）；
  6. **路径 4（RLS 直连绕过）**：用 `base_app` 直连，`SET app.tenant_id = 'tenant-a'`
     后 `SELECT count(*)` 全表；
  7. **路径 5（列集合）**：查 `information_schema.columns`；
  8. **路径 6（3 张表全验 RLS，D311 / S-04）**：
     ① 断言 `crates/platform-notify/manifest.json` 的 `tenantScoped.tables`
     **双向相等**于 `{platform_notify_template, platform_notify_preference,
     platform_notify_delivery}`；
     ② `DATABASE_URL="$DATABASE_URL_TEST" cargo xtask tenant-policies --check` **退出码 0**；
     ③ 对 **`platform_notify_template`** 与 **`platform_notify_preference`** 各做一次
     与步骤 6 同手法的直连 RLS 验证（换表，断言只看到当前租户的行）。
- **预期结果**:
  - 步骤 2：`renderedBody` **仍是旧文案**（逐字）—— **快照**语义，模板改了不追溯；
  - 步骤 3：`tenant-a` 的列表**不含** `tenant-b` 的任何记录（`tenantId` 全部逐字等于 `tenant-a`）；
  - 步骤 4：**4 次全部 400** + `NOTIFY_SENSITIVE_PAYLOAD`；
  - 步骤 5：**成功**（`secretary` 不含敏感键 → 不得误伤，
    这正是 `scan_sensitive_keys` 的**完整键名匹配**语义，
    `crates/platform-core/src/extension/audit.rs:430`）；
  - 步骤 6：**只返回 `tenant-a` 的 1 条**（RLS 在库层生效，不靠应用层过滤）；
  - 步骤 7：列集合**恰好**是 D299 那 **18** 列
    （`id` / `tenant_id` / `template_code` / `channel` / `category` / `recipient` /
    `locale` / `payload` / `rendered_title` / `rendered_body` / `status` /
    `attempt_count` / `next_attempt_at` / `last_error` / `provider_message_id` /
    `created_at` / `updated_at` / `delivered_at`），
    且 `payload` 的类型是 `jsonb`。
    ⚠️ **判据写法要求**：断言要写成「查询结果与一个**显式列出的** 18 元素集合相等」，
    **不许**写成「列数 = 18」（列数对但列错，那种断言会放过改名列）；
  - 步骤 8：① 集合**双向相等**（既不缺也不多）；
    ② 退出码 **0**；
    ③ **两张表各自**都只返回当前租户的行。
    ⚠️ **为什么必须验另外两张表**：`design-summary` §2.4 声明了 3 张表进 `tenantScoped`，
    而原先只有 delivery 一张有判据 → 若 manifest 漏写某张表的声明，
    `tenant-policies --check` **会绿**（**没声明就不生成也不校验**）
    → 那张表**完全无隔离** → **跨租户读别人的通知模板 / 订阅偏好**。
- **类型**: 安全

---

### AC-S-174: 权限点三处齐、启动对账 **fail-fast**、C-5 在本 crate 上通过、审计埋点

- **前置条件**: 本需求新增 `crates/platform-notify/manifest.json` 与权限点种子迁移。
- **测试步骤**（**5 条独立路径**）:
  1a. `cargo xtask capabilities` **重新生成**；
  1b. `cargo test -p platform-notify --test contract_c5`（**该测试文件由本需求新增**）；
  1c. `cargo test -p platform-server --test i18n_titles`（**既有测试，本需求必须让它继续绿**）；
  2. **只改 manifest、不加种子 INSERT**，起服务；
  3. 把 manifest 里某个权限点从代码的 `require_permission` 调用点**删掉**（改成非字面量），
     再跑 1b；
  4. 在 `crates/platform-notify/src/` 里放一个叫 `authorize` 的函数，再跑 1b；
  5. 走一遍：建模板 → 改模板 → 删模板 → 重投死信，查 `platform_audit_log`。
- **预期结果**:
  - 步骤 1a：`capabilities/` 下**恰好 14 个文件**（**不增不减**）。
    ⚠️ **实测依据**（`crates/xtask/src/capabilities.rs:54-81` 的 `generate`）：
    固定产出 **7 个**（`permissions` / `extension-points` / `schema/manifest` /
    `schema/metadata` / `schema/entity` / `api` / `index`）+
    `capabilities/sdk/` 下**每包一个**（实测 **7 个**：`auth` / `hooks` / `i18n` /
    `schema` / `sdk` / `theme` / `ui`）= **14**。
    **新增 Rust crate 只让 `permissions.json` 多一个 module 条目，不新增文件**；
  - 步骤 1a 续：`capabilities/permissions.json` 的 `total == 38`
    （实测基线 **35** = 6+3+3+4+6+9+4，各 manifest 的权限点数见
    `capabilities/permissions.json`；**+3** 个 `notify:*` = 38），
    且 `modules` 里**新增**一个 `crates/platform-notify/manifest.json` 条目，
    且含 `notify:template:read` / `notify:template:write` / `notify:delivery:read`
    **逐字 3 条**；
  - 步骤 1b：**通过**（真 C-5 命令）；
  - 步骤 1c：**通过**，且其中**两条既有断言必须被同步更新**（⚠️ **Dev 最容易漏的一步**）：
    - **`the_title_table_has_the_pinned_number_of_rows`**
      （`crates/platform-server/tests/i18n_titles.rs:174`）**硬编码了**当前基线：
      `58 + REQ_013(21) + REQ_014(5) + REQ_022(9) + REQ_016(6) = 99`。
      → 必须加一行 **`const REQ_015_NEW_CODES: usize = 8;`** 并计入 assert，
      否则 `TITLES.len() == 99` 而实际是 107 → **这条测试会红**；
    - **`all_codes()`**（同文件 `:59`）是**逐 crate 手工列举**的
      （`platform_core::problem::ALL_CODES`、`platform_app::error::ALL_CODES` …）。
      → 必须加 `for code in platform_notify::error::ALL_CODES { … }`，
      否则新加的 8 个码**不在清单里** → `titles_cover_every_code` 检查不到它们
      （**会假绿**：表里缺了也不报）。
      ⚠️ **判据必须读源码，不能只看测试绿**（**D318 / S-11**）——
      因为「漏加时它恰好就是绿的」，看绿等于没验：
      `grep -c "platform_notify::error::ALL_CODES" crates/platform-server/tests/i18n_titles.rs`
      **≥ 1**；
      **阳性对照**：`grep -c "platform_app::error::ALL_CODES"`（同文件）**= 1**
      （证明扫的是真文件、模式有效）。
    ⚠️ 同时须满足 `every_error_code_is_registered_in_every_locale`（`:382`）：
    **每个新码在 `zh-CN` 与 `en-US` 两个词库里都要有 `error.<CODE>` 条目**
    （实测基线：两库各 **99** 条 `error.*`，加 8 条后各 **107**）。
  - 步骤 2：服务**起不来**（退出码非 0），日志**逐条**列出缺失的权限点编码与
    来源 manifest 路径 `crates/platform-notify/manifest.json`；
  - 步骤 3：C-5 **失败**（已登记但无鉴权调用点）→ **阳性对照**；
  - 步骤 4：C-5 **失败**（出现第二个鉴权入口，`authorize` 在 C-5 命名禁区里，
    D216 的 8 个别名）→ 证明命名禁区有效；
  - 步骤 5：**恰好 4 个**事件码进 `platform_audit_log`（`notify.template.created` /
    `notify.template.updated` / `notify.template.deleted` / `notify.dead_letter.resent`），
    每条含契约 C12 的固定字段集
    （`actor_id` / `action` / `object_type` / `object_id` / `ip` / `user_agent` /
    `created_at` / `prev_hash` / `hash`），`object_type` **恰好**是 `platform_notify_template`
    或 `platform_notify_delivery`（表名语义），且 `ChainVerifier::verify(tenant-a)` → `ok == true`。
- **类型**: 可维护性 / 安全

> ⚠️ **为什么 4 个 `notify.*` 事件码不进 `TITLES`，而 6 个 `org.*` 事件码在 `TITLES` 里**
> —— 这两件事看起来矛盾，必须说清（否则 Dev 会照着 `org` 的样子把 `notify.*` 也塞进去）：
>
> | | `org.*`（**在** `TITLES` 里） | `notify.*`（**不进** `TITLES`） |
> |---|---|---|
> | 用途 | **通知标题的文案来源** | **审计记录的 `action` 字段** |
> | 谁查 | `OrgEventKind::title()` 显式调 `title_of_code()`（`crates/platform-org/src/notify.rs:144`） | 没人查 `title_of_code` |
> | 前端消费 | 是（按事件码渲染中文文案） | 否 |
> | 依据 | 已交付事实（`titles.rs:195-200` 的注释「platform-org 的通知标题（事件码，不是 HTTP 错误码）」） | D223（审计事件码不是 HTTP 错误码，不进 `TITLES`） |
>
> → **判据**：`notify.*` 这 4 个码**不得**出现在 `TITLES` 里
> （步骤 1c 的钉死行数会自动抓住 —— 多写 4 行就是 111 ≠ 107）。

---

## 完成定义对照

| `demand.md` 的功能要求 | AC | 本轮是否真达成 |
|---|---|---|
| 多渠道通道抽象（5 类） | 166 | **3/5**（站内信 / 邮件 / Webhook）→ **TD-054** |
| 模板引擎：变量 | 167 | ✅（**只做变量替换**，D293） |
| 模板引擎：多语言 | 168 | ✅（按 locale 分行 + 三级回退） |
| 模板引擎：预览 | 167 步骤 4 | ✅（渲染接口即预览：给定 `payload` 返回渲染结果） |
| 用户订阅偏好（可退订） | 169 | ✅（按 `category`，`system` 不可退订） |
| 异步投递 + 重试退避 + 死信 | 170 / 171 / 023 | ✅ |
| 投递记录可查 | 173 | ✅ |

> ⚠️ **「五类通道可投递」只达成 3/5** —— 这是**明码的代价**，不是遗漏。
> 缺的两类（短信 / IM）需要**真实凭据**才能写出可验证的实现 → **TD-054**。
