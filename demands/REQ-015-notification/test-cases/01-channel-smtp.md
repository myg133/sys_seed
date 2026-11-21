# 01 渠道抽象与 SMTP 协议（AC-S-166 / AC-S-166.b / AC-S-023）

> 测试落点：`crates/platform-notify/tests/channels.rs` +
> `crates/platform-notify/src/channels/smtp.rs` 内联单测（design-summary §9）。
> 全部用例的假服务器都用 `TcpListener::bind("127.0.0.1:0")`（D305 夹具），**不连外网**。

## 夹具前置：假 SMTP 服务器（后面多条用例共用）

`FakeSmtp` 是一个可编排应答的 `TcpListener`，在独立 task 里 accept 并按脚本回写。
它的关键能力（每个都是后面某条用例的判据来源）：

| 能力 | 用途 |
|---|---|
| `script: Vec<&str>` 逐行应答 | 主路径 250/334/354 全套 |
| `on(verb, code)` 覆盖某个动词的应答 | `AUTH`→`535`、第 2 个 `RCPT`→`550` |
| `silent()` accept 后**什么都不写** | 步骤 7「连上但不发 220」 |
| `auth_only_after_tls: bool` **只在 TLS 握手后才宣告 `AUTH`** | 证明客户端 TLS 后重发了 EHLO |
| `call_count()` 记录收到的 DNS/命令序列 | 断言协议顺序 |

---

### TC-015-001 主路径：3 个真 provider 逐个 `send()` 全部 delivered
- **AC**: AC-S-166 步骤 1 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（假 SMTP / 假 HTTP）｜ **方法**: 等价类（合法输入）
- **前置**: `FakeSmtp` 脚本全 250/334/354/250/221；Webhook 端点用假 HTTP 服务器回 `200`；`SmtpConfig` 由 5 个环境变量填充指向该假服务器；站内信（`in_app`）无需外部服务
- **步骤**: 1) 构造 `ChannelRegistry`，按 `channel()` 返回值注册 `in_app` / `smtp` / `webhook` 3 个 provider 2) 依次调 `send()` 3) 读每个 `DeliveryResult`
- **预期**: ① 站内信 `delivered == true`；② 邮件 `delivered == true` 且 `provider_message_id.is_some()`，其值**等于**假服务器 `DATA` 后回的那行 250 响应里的队列 ID（脚本里写死 `250 2.0.0 QUEUE-ID-7f3a`，断言 `provider_message_id == Some("QUEUE-ID-7f3a")` —— 证明取的是 250 响应而非自造 uuid）；③ Webhook `delivered == true`；④ 三次 `send()` 都返回 `Ok`，无 `Err`

---

### TC-015-002 注册表通道名集合恰好 7 个（含 4 个未配置）
- **AC**: AC-S-166 步骤 2 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测 ｜ **方法**: 等价类
- **前置**: 同 TC-015-001 的注册表
- **步骤**: 1) 读 `ChannelRegistry` 已注册的通道名集合 2) 对 `sms` / `dingtalk` / `wecom` / `feishu` 各调一次 `send()`
- **预期**: ① 集合**双向相等**于 `{"in_app","smtp","webhook","sms","dingtalk","wecom","feishu"}`（**7 个**，既不少也不多）；② 4 个未配置通道的 `send()` **全部返回 `Err`**，**没有**任何一个返回 `Ok(DeliveryResult{delivered:true})`，也**没有**任何一个返回 `Ok(DeliveryResult{delivered:false})`（D292：未配置不得静默成功）

---

### TC-015-003 路径 2：短信通道 → Err NOTIFY_CHANNEL_NOT_CONFIGURED
- **AC**: AC-S-166 步骤 3 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测 ｜ **方法**: 等价类
- **前置**: 注册表已含 `sms` 占位 provider；**未**设置任何短信凭据环境变量
- **步骤**: 调 `send()`，`recipient = "+8613800000000"`
- **预期**: 返回值是 `Err`；错误码**逐字** `NOTIFY_CHANNEL_NOT_CONFIGURED`；HTTP 映射为 **400**；`delivered` 字段**不存在**于返回值中（是 `Err` 不是 `Ok`）

---

### TC-015-004 路径 3：IM 三通道各投一次 → 3 次全 Err NOTIFY_CHANNEL_NOT_CONFIGURED
- **AC**: AC-S-166 步骤 4 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测 ｜ **方法**: 等价类
- **前置**: 注册表含 `dingtalk` / `wecom` / `feishu` 三个占位 provider；三者凭据环境变量均未设置
- **步骤**: 依次对 3 个通道各调一次 `send()`
- **预期**: ① **恰好 3 次**调用，**3 次全部** `Err`；② 每次错误码都**逐字** `NOTIFY_CHANNEL_NOT_CONFIGURED`；③ 计数断言：`err_count == 3 && ok_count == 0`（若某个 provider 返回了 `Ok`，本条即红）

---

### TC-015-005 路径 4：AUTH 回 535 → Err 且错误文案含 535
- **AC**: AC-S-166 步骤 5 ｜ **类型**: 功能/安全 ｜ **优先级**: P0 ｜ **自动化**: 单测（假 SMTP）｜ **方法**: 错误推测
- **前置**: `FakeSmtp` 在前 4 步（220/EHLO/STARTTLS/第二 EHLO）正常应答，对 `AUTH LOGIN` 的**首次 334** 之后的 pass 应答回 `535 5.7.8 Authentication credentials invalid`
- **步骤**: 向邮件通道 `send()` 一次，读 `Err` 的文案
- **预期**: ① 返回 `Err`（**不是** `Ok(delivered:false)`）；② `format!("{}", err)` **包含子串 `"535"`**；③ 负向断言：错误文案**不只是** `"authentication failed"` 这类无码泛化文案 —— 断言 `err.to_string().contains("535")`，且**不含**完整的 base64 密码串（密码不得进错误文案）

---

### TC-015-006 路径 5：第 2 个 RCPT TO 回 550 → 整条 Err 且含 550
- **AC**: AC-S-166 步骤 6 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（假 SMTP）｜ **方法**: 边界值
- **前置**: `FakeSmtp` 对第 1 个 `RCPT TO` 回 `250`，对第 **2** 个回 `550 5.1.1 No such user`；收件人列表为 2 个地址 `[r1@example.com, r2@example.com]`
- **步骤**: 向邮件通道 `send()` 一次
- **预期**: ① 返回 `Err`（**不是**部分成功的 `Ok`）；② 错误文案**包含 `"550"`**；③ 关键负向断言：`FakeSmtp.call_count("DATA") == 0` —— 第 2 个收件人被拒后**根本没有发 `DATA`**，即不存在「只发给 r1」的部分成功；④ 该次 `send()` 的 `provider_message_id` 为 `None`（整条失败没有队列 ID）

---

### TC-015-007 路径 6：连上但不发 220 → 10 秒超时 Err
- **AC**: AC-S-166 步骤 7 ｜ **类型**: 功能/可靠性 ｜ **优先级**: P0 ｜ **自动化**: 单测（假 SMTP）｜ **方法**: 错误推测
- **前置**: `FakeSmtp::silent()` —— `TcpListener::accept()` 后**保持连接打开但一个字节都不写**（连接建立成功 → 客户端读不到 banner → 卡在等 `220`）
- **步骤**: 1) 记录 `Instant::now()` 2) 向邮件通道 `send()` 3) 读耗时与错误
- **预期**: ① 返回 `Err`；② 错误文案**包含超时语义**（`"timeout"` 或 `"timed out"`）；③ 耗时在 **10s ± 2s** 内返回（`10_000ms <= elapsed <= 12_000ms`）—— 断言「**不是**无限挂起、也**不是** 100ms 就放弃」；④ 该 `send()` 未进入 `AUTH`：`FakeSmtp.call_count("AUTH") == 0`

---

### TC-015-008 路径 6b（AC 补充）：banner 超时后连接被关闭，不泄漏 FD
- **AC**: AC-S-166 步骤 7（补强）｜ **类型**: 可靠性 ｜ **优先级**: P1 ｜ **自动化**: 单测（假 SMTP）｜ **方法**: 错误推测
- **前置**: 同 TC-015-007 的 `silent()` 服务器，连续投 5 次
- **步骤**: 1) 连续 5 次 `send()` 全部超时 2) 假服务器侧观察每条连接是否被对端 `FIN` 关闭
- **预期**: ① 5 次都 `Err`；② 假服务器读到 **5 次** EOF（对端主动关闭）—— 迟到连接的 socket 不会被丢弃不管；③ 第 6 次 `send()` 仍能在 10s 内返回超时 `Err`（未被前 5 次的残留连接拖死）

---

### TC-015-009 SMTP 协议顺序：TLS 后必须重发 EHLO（`AUTH` 只在 TLS 后可见）
- **AC**: AC-S-166 步骤 1（协议正确性）｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（假 SMTP）｜ **方法**: 状态迁移
- **前置**: `FakeSmtp{auth_only_after_tls: true}`：**TLS 握手完成之前**的 `EHLO` 应答里**不宣告 `AUTH`**（只回 `250 STARTTLS`）；TLS **之后**的 `EHLO` 才回 `250 AUTH LOGIN`。这是一条有区分力的夹具：客户端若不重发 EHLO，就永远发现不了 `AUTH` 能力
- **步骤**: 1) `send()` 一次 2) 读假服务器记录的命令序列 3) 读结果
- **预期**: ① 命令序列**恰好**为 `["EHLO", "STARTTLS", "EHLO", "AUTH", "MAIL", "RCPT", "RCPT", "DATA", "QUIT"]` —— **`EHLO` 出现 2 次**且第 2 次在 `STARTTLS` 之后；② 第 2 次 `EHLO` **之前**完成了 TLS 握手（`FakeSmtp.tls_handshakes == 1`）；③ `send()` 返回 `Ok(delivered:true)` —— 反过来说：若客户端不重发 EHLO，它看不到 `AUTH`，本用例会红（`AUTH` 不出现或 `delivered:false`）

---

### TC-015-010 AUTH LOGIN 是两次挑战应答（user → 334 → pass → 235）
- **AC**: AC-S-166 步骤 1（协议正确性）｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（假 SMTP）｜ **方法**: 状态迁移
- **前置**: `FakeSmtp` 对 `AUTH LOGIN` 回 `334 VXNlcm5hbWU6`，对第 1 段 base64 回 `334 UGFzc3dvcmQ6`，对第 2 段回 `235 2.7.0 Authentication successful`；环境变量 `BASE_SMTP_USER="u@example.com"`、`BASE_SMTP_PASSWORD="p@ss w0rd"`
- **步骤**: 1) `send()` 2) 解出假服务器收到的两段 base64 载荷
- **预期**: ① 假服务器**恰好**收到 **2 段** base64 载荷（不是 1 段 —— 一次性 `AUTH LOGIN <b64>` 的写法会只有 1 段）；② 第 1 段 `base64_decode == "u@example.com"`，第 2 段 `base64_decode == "p@ss w0rd"`（顺序不能反）；③ 客户端在收到第 2 个 `334` 后才发第 2 段；④ 最终返回 `Ok(delivered:true)` 且 `provider_message_id.is_some()`

---

### TC-015-011 不支持的 AUTH 机制显式报错，不静默降级
- **AC**: AC-S-166.b（`AUTH PLAIN`/`CRAM-MD5`/`XOAUTH2` 不做）｜ **类型**: 安全/功能 ｜ **优先级**: P1 ｜ **自动化**: 单测（假 SMTP）｜ **方法**: 错误推测
- **前置**: `FakeSmtp` 的 TLS 后 `EHLO` 应答里**只**宣告 `250 AUTH PLAIN CRAM-MD5`（**没有** `LOGIN`）
- **步骤**: 调 `send()`
- **预期**: ① 返回 `Err`；② 错误文案**点名**服务端宣告的机制（包含 `"PLAIN"` 或 `"CRAM-MD5"`）或明确「无受支持的认证机制」；③ 关键负向断言：假服务器 `call_count("AUTH") == 0` 或收到的 `AUTH` 参数**不是** `PLAIN`/`CRAM-MD5`/`XOAUTH2` —— 即**不得**静默降级成未实现的机制去试

---

### TC-015-012 头注入防御：Subject 含 `\r\nBcc:` 时不得发出
- **AC**: AC-S-166 步骤 1（design §4.6 头注入判据 / AC-S-174 步骤 3 的 TD-047 机制要求）｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 单测（假 SMTP）｜ **方法**: 错误推测
- **前置**: `FakeSmtp` 全程正常应答；`render_title = "hi\r\nBcc: attacker@evil.example"`
- **步骤**: 1) `send()` 2) 抓假服务器收到的 `DATA` 段原文
- **预期**: ① 二者取一且必须可断言：要么返回 `Err`（拒绝），要么 `Ok` 但 `DATA` 段里 **不含** `"Bcc:"`；② 若走「剥除」路线，`DATA` 里的 `Subject` 行**不含**任何 `\r` 或 `\n` 字符（断言 Subject 行 `split("\r\n")` 后长度 == 1）；③ 硬判据：假服务器最终收到的 `RCPT TO` 计数**等于**入参收件人数（若注入成功会多出 `attacker@evil.example`，此时计数会多 1 → 红）

---

### TC-015-013 base64 正文每 76 字符换行
- **AC**: AC-S-166 步骤 1（design §4.6 RFC 2045 判据）｜ **类型**: 功能 ｜ **优先级**: P1 ｜ **自动化**: 单测（假 SMTP）｜ **方法**: 边界值
- **前置**: `FakeSmtp` 全程正常；正文取一段能产生 >76 字符 base64 的文本（如 200 个 ASCII 字符）
- **步骤**: 1) `send()` 2) 取 `DATA` 段里 base64 正文，按 `\r\n` 切行
- **预期**: ① 除**最后一行**外，每一行长度**恰好 76**；② 最后一行长度 `1..=76`；③ 拼接所有行后 `base64_decode` == 原始正文（逐字节相等）；④ 负向断言：**不存在**长度 > 76 的行（否则红）

---

### TC-015-014 步骤 8：`SmtpConfig` 的 `Debug` 不含密码明文，含 `***`，且含 host
- **AC**: AC-S-166 步骤 8 ｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 单测 ｜ **方法**: 因果
- **前置**: `BASE_SMTP_HOST="smtp.fake.local"`、`BASE_SMTP_PASSWORD="S3cr3t-Value-xyz"`；由环境变量构造出 `SmtpConfig`
- **步骤**: 1) `let s = format!("{:?}", cfg);` 2) 检查 `s`
- **预期**: ① `s.contains("S3cr3t-Value-xyz") == false`（**密码值不出现在 Debug 输出里**）；② `s.contains("***") == true`（有脱敏占位）；③ **阳性对照**：`s.contains("smtp.fake.local") == true` —— 证明 `Debug` 不是全空串，否则①的「不含密码」是假绿；④ 另断言 `s.contains("S3cr3t")` 亦为 false（防只截断部分）

---

### TC-015-015 AC-S-166.b：短信 / IM 无真实实现，且不隐式 TLS、不做多部分 MIME
- **AC**: AC-S-166.b ｜ **类型**: 可维护性 ｜ **优先级**: P1 ｜ **自动化**: 静态扫描 ｜ **方法**: 边界值
- **前置**: 工作树干净（本轮交付的 `crates/platform-notify/src/` 可扫）
- **步骤**: 1) 扫 `crates/platform-notify/src/` 找隐式 TLS 端口常量 `465` 2) 扫 `AUTH PLAIN` / `XOAUTH2` / `CRAM-MD5` 的**实现**（不是注释）3) 扫 `multipart/` MIME 构造 4) 对 `sms`/`dingtalk`/`wecom`/`feishu` 4 个 provider 查阅实现体
- **预期**: ① `465` 作为连接端口**命中数 0**（**阳性对照**：同一次扫描对 `587` 命中 > 0，证明扫的是真代码）；② 三个未实现机制的实现体命中数 **0**；③ `"multipart/"` 的 MIME 构造命中数 **0**；④ 4 个 provider 的 `send()` 函数体**只有**返回 `NOTIFY_CHANNEL_NOT_CONFIGURED` 的分支（无任何真实 HTTP 调用），且**不含**任何 `token` / `sign` 计算的调用

---

## AC-S-023：死信可重投，且不重复投递已成功的

> ⚠️ 本节是「防恒真」的重点。**核心判据是 provider 调用计数，不是数据库状态。**
> 计数怎么数：用 **`CountingSender` 夹具**替换真实的 `in_app` provider ——
> 一个实现了 `NotificationSender` 的 struct，内部持 `Arc<AtomicUsize>`，
> 每次 `send()` 被调用就 `fetch_add(1)` 并返回 `Ok(DeliveryResult{delivered:true, provider_message_id:Some("inapp-{n}")})`。
> 它出现在 `ChannelRegistry` 的 `in_app` 位；`smtp` 位用 `FakeSmtp` 的可控失败/成功脚本。
> **数据库状态可能被覆盖，但 `AtomicUsize` 的计数器不会** —— 这就是「发了但状态被写成已发」也能抓到的原因。

### TC-015-016 前置造数：1 成功 1 失败到 `maxAttempts` 用尽进死信
- **AC**: AC-S-023 步骤 1-3 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: `tenant-a` 有管理员 `u-perm-notify@example.com` 持 `notify:delivery:read`；`in_app` 位挂 `CountingSender`；`smtp` 位挂 `FakeSmtp` **恒定失败**（对 `MAIL FROM` 回 `451 4.3.0 try again`）；`notify.maxAttempts = 5`（默认）
- **步骤**: 1) 入队 2 条通知（站内信 + 邮件），同收件人 2) `tick()` 让站内信那条成功 3) 连续 `tick()` 5 次让邮件那条耗尽次数 4) 读出两行的 `deliveryId` / `status` / `attempt_count` / `deliveredAt`
- **预期**: ① 站内信那行 `status == "delivered"`、`deliveredAt` 非空，且 `CountingSender.count() == 1`；② 邮件那行 `status == "dead_letter"` 且 **`attempt_count == 5`**（**恰好 5**，不是 4 也不是 6 —— 证明 `maxAttempts` 含首次）；③ `FakeSmtp.call_count("MAIL") == 5`（5 次真投，与 `attempt_count` 一致）；④ 记 `D_ok = 站内信 id`、`D_dead = 邮件 id`、`D_ok.deliveredAt = T0`

---

### TC-015-017 resend 死信 → 200 且计数重置为 0（不是接着第 6 次）
- **AC**: AC-S-023 步骤 4 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: TC-015-016 已跑完；`D_dead.status == "dead_letter"`、`attempt_count == 5`
- **步骤**: 1) `POST /api/notify/deliveries/{D_dead}/resend`（带 `u-perm-notify` 令牌）2) 读响应体 3) 查库回读 `D_dead` 那行
- **预期**: ① HTTP **200**；② 响应 `status == "pending"`；③ 响应 `attemptCount == 0`（**恰好 0**）；④ 库回读 `next_attempt_at <= now()`；⑤ 库回读 `last_error IS NULL`；⑥ 关键区分力：`attemptCount` 是 **0** 而不是 5 或 6 —— 「重置」与「单调递增计数」在这里分叉

---

### TC-015-018 修好 SMTP 后 tick → D_dead 投递成功且 `attempt_count` 恰好 1
- **AC**: AC-S-023 步骤 5 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 状态迁移
- **前置**: TC-015-017 已执行（`D_dead` 已重置为 `pending` / `attempt_count=0`）；把 `FakeSmtp` 改成**全程成功**脚本
- **步骤**: 1) `tick()` 一次 2) 查 `D_dead` 那行 3) 读 `FakeSmtp.call_count("MAIL")` 的新增次数
- **预期**: ① `D_dead.status == "delivered"`；② `D_dead.attempt_count == 1`（**恰好 1**，不是 5、不是 6 —— 若实现是「接着第 6 次」这里会是 6 → 红）；③ `provider_message_id IS NOT NULL`；④ `delivered_at IS NOT NULL`；⑤ `FakeSmtp` 的 `MAIL` 调用**新增恰好 1 次**（5 → 6）

---

### TC-015-019 ⭐ 核心：重投后 `D_ok` 的 deliveredAt 逐字节不变 **且** 站内信计数仍为 1
- **AC**: AC-S-023 步骤 6 ｜ **类型**: 功能/可靠性 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 因果
- **前置**: TC-015-018 已执行完成（`D_dead` 已 `delivered`）；TC-015-016 记下的 `T0` 与 `CountingSender.count() == 1`
- **步骤**: 1) 重新查 `D_ok` 整行 2) 读 `CountingSender.count()` 3) 再 `tick()` 两次 4) 再读 `D_ok` 与计数
- **预期**: ① `D_ok.deliveredAt` **逐字节等于 `T0`**（字符串相等，不是「不为空」）；② `D_ok.attempt_count == 1`（**仍是 1**）；③ `D_ok.status == "delivered"`（**仍是 delivered**）；④ **`CountingSender.count() == 1`**（**仍是 1**，不是 2）—— 这是「不重复投递已成功的」的唯一硬判据：只看①②③的话，若实现「又发了一次、状态写回原值」，三者全过 → 本条必须靠④才红；⑤ 追加两次 `tick()` 后计数**仍是 1**（`delivered` 是终态，不参与出队）；⑥ `FakeSmtp` 的 `MAIL` 调用次数**不再增长**（仍是 6）

---

### TC-015-020 两条判据互相独立：只重置计数不改状态 → 步骤 5 必红
- **AC**: AC-S-023（判据独立性）｜ **类型**: 功能/可靠性 ｜ **优先级**: P1 ｜ **自动化**: 集成（真实库）｜ **方法**: 变异检测
- **前置**: 通过测试专用开关或本地变异补丁，让 resend 的 SQL **只**重置 `attempt_count`/`next_attempt_at` 但**不改** `status`（仍留 `dead_letter`）
- **步骤**: 1) 对 `D_dead` 走 resend 2) `tick()` 一次
- **预期**: ① resend 返回的行 `status` **仍是 `"dead_letter"`**（模拟坏实现）；② 随后 `tick()` **不会**把它取出投递 → `FakeSmtp.call_count("MAIL")` **不增长**，`D_dead.status` 恒为 `dead_letter` ③ 断言点落在 TC-015-018 的「`status == "delivered"`」上 → **该用例必须变红**。本条的作用是证明 TC-015-018 的判据有区分力，不是恒真

---

### TC-015-021 另一侧独立性：重投时把已成功的也一起重置 → 步骤 6 必红
- **AC**: AC-S-023（判据独立性）｜ **类型**: 功能/可靠性 ｜ **优先级**: P1 ｜ **自动化**: 集成（真实库）｜ **方法**: 变异检测
- **前置**: 本地变异补丁：resend 的 `UPDATE` **去掉** `AND status = 'dead_letter'` 条件（于是同租户所有行都被重置）
- **步骤**: 1) 对 `D_dead` 走 resend 2) 查 `D_ok` 3) `tick()` 两次 4) 读 `CountingSender.count()`
- **预期**: ① `D_ok.status` 被改成 `"pending"`（坏实现暴露）；② `tick()` 后 `CountingSender.count() == 2`（**把已成功的又发了一次**）；③ 断言点落在 TC-015-019 的「计数仍是 1」与「`deliveredAt` 逐字节不变」上 → **该用例必须变红**。本条证明 TC-015-019 的计数判据能抓到「状态被覆盖」这一类坏实现

---

### TC-015-022 无权限令牌调 resend → 403，且不产生任何状态变更
- **AC**: AC-S-023 步骤 4（权限）＋ AC-S-174 权限点落点 ｜ **类型**: 安全 ｜ **优先级**: P1 ｜ **自动化**: 集成 ｜ **方法**: 等价类（无权限）
- **前置**: TC-015-016 造出的 `D_dead` 仍在 `dead_letter`；另备一个**不持** `notify:delivery:read` 的用户 `u-noperm@example.com`
- **步骤**: 1) 用 `u-noperm` 的令牌 `POST /api/notify/deliveries/{D_dead}/resend` 2) 查 `D_dead`
- **预期**: ① HTTP **403**；② `Content-Type: application/problem+json`；③ `D_dead.status` **仍是 `"dead_letter"`**、`attempt_count` **仍是 `5`**（鉴权失败发生在状态变更之前）；④ `FakeSmtp` 的 `MAIL` 计数**不增长**
