# REQ-015 通知系统 —— 测试用例集

> 依据：`acceptance.md`（**10 条 AC** = AC-S-023 重写 + AC-S-166～174）+ `design-summary.md`。
> 状态：**已完成**（93 条用例，编号 `TC-015-001` ～ `TC-015-093`，连续无跳号）。

## 文件清单

| 文件 | 覆盖 AC | 用例数 | 编号区间 |
|---|---|---|---|
| [01-channel-smtp.md](01-channel-smtp.md) | AC-S-166（含 166.b）+ AC-S-023 | 22 | 001 ～ 022 |
| [02-template-locale.md](02-template-locale.md) | AC-S-167 + AC-S-168 | 19 | 023 ～ 041 |
| [03-preference-delivery.md](03-preference-delivery.md) | AC-S-169 + AC-S-173 | 16 | 042 ～ 057 |
| [04-worker-retry-deadletter.md](04-worker-retry-deadletter.md) | AC-S-170 + AC-S-171 + AC-S-172 | 20 | 058 ～ 077 |
| [05-permission-audit.md](05-permission-audit.md) | AC-S-174 | 16 | 078 ～ 093 |
| **合计** | **10/10 AC** | **93** | — |

## AC → 用例映射

### AC-S-023 死信可重投且不重复投递已成功的（7 条）

| 步骤 | 用例 |
|---|---|
| 步骤 1-3 造数（成功 1 / 死信 1） | TC-015-016 |
| 步骤 4 resend → 200 且计数重置为 0 | TC-015-017 |
| 步骤 5 tick → `attempt_count` 恰好 1 | TC-015-018 |
| **步骤 6 ⭐ `deliveredAt` 不变 + provider 计数仍 1** | **TC-015-019** |
| 判据独立性（只重置计数不改状态必红） | TC-015-020 |
| 判据独立性（重置已成功的必红） | TC-015-021 |
| 无权限调 resend → 403 且零变更 | TC-015-022 |

### AC-S-166 通道抽象 3 真实现 + 4 显式未配置（15 条）

| 步骤 | 用例 |
|---|---|
| 步骤 1 主路径 3 provider | TC-015-001 |
| 步骤 2 注册表恰好 7 个名字 | TC-015-002 |
| 步骤 3 路径 2 短信 → `NOTIFY_CHANNEL_NOT_CONFIGURED` | TC-015-003 |
| 步骤 4 路径 3 IM 三通道 → 3 次全 Err | TC-015-004 |
| 步骤 5 路径 4 AUTH 回 535 | TC-015-005 |
| 步骤 6 路径 5 第 2 个 RCPT 回 550 | TC-015-006 |
| 步骤 7 路径 6 连上不发 220 → 10s 超时 | TC-015-007 |
| 步骤 7 补强（连接被关闭、不泄漏 FD） | TC-015-008 |
| SMTP 协议：**TLS 后必须重发 EHLO** | TC-015-009 |
| SMTP 协议：**AUTH LOGIN 两次挑战应答** | TC-015-010 |
| AC-S-166.b 不支持的 AUTH 机制显式报错 | TC-015-011 |
| 头注入防御（`Subject` 含 `\r\nBcc:`） | TC-015-012 |
| base64 正文 76 字符换行 | TC-015-013 |
| 步骤 8 `Debug` 不含密码 + 含 `***` + 阳性对照 | TC-015-014 |
| AC-S-166.b 不做清单（465 / 多部分 MIME / 短信 IM 实现体） | TC-015-015 |

### AC-S-167 模板 CRUD + 受限变量替换（13 条）

| 步骤 | 用例 |
|---|---|
| 步骤 1 POST → 201 | TC-015-023 |
| 步骤 2 列表 + 单个 | TC-015-024 |
| 步骤 3 PATCH / DELETE → 404 | TC-015-025 |
| 步骤 4 主路径渲染逐字相等 | TC-015-026 |
| 步骤 5 路径 2 合法变量名边界 4 种 | TC-015-027 |
| 步骤 6 路径 3 非法占位符 5 种 | TC-015-028 |
| 步骤 7 路径 4 未知变量点名 | TC-015-029 |
| 步骤 8 路径 5 多余变量被忽略 | TC-015-030 |
| 步骤 9 路径 6 重复 → 409 | TC-015-031 |
| 步骤 10 路径 7 不存在 → 404 | TC-015-032 |
| 步骤 11 路径 8 SSTI 4 种载荷 | TC-015-033 |
| 单趟替换（payload 值里的 `{{x}}` 不二次展开） | TC-015-034 |
| 渲染不改模板（预览只读） | TC-015-035 |

### AC-S-168 多语言三级回退（6 条）

| 步骤 | 用例 |
|---|---|
| 步骤 1 两 locale 行共存 + 唯一约束 4 列 | TC-015-036 |
| 步骤 2 locale=en-US 命中 | TC-015-037 |
| 路径 2 ja-JP → 回退租户默认 zh-CN | TC-015-038 |
| 路径 3 删 zh-CN → 失败 | TC-015-039 |
| 路径 4 只留 en-US → 失败（不回退 en-US） | TC-015-040 |
| 补强：跨租户不互相可见 | TC-015-041 |

### AC-S-169 订阅偏好（9 条）

| 步骤 | 用例 |
|---|---|
| 步骤 1 marketing 正常投递 | TC-015-042 |
| 步骤 2-3 退订后进死信且 `last_error` 点名 | TC-015-043 |
| 步骤 4 GET 偏好 | TC-015-044 |
| 步骤 5 退订 system → 400 | TC-015-045 |
| **步骤 5 ⭐ 偏好表 `system` 行数恰好 0（查库）** | **TC-015-046** |
| 步骤 6 system 不受影响 | TC-015-047 |
| 步骤 7 重新订阅恢复 | TC-015-048 |
| 偏好端点不需权限点（含 403 反向对照） | TC-015-049 |
| 补强：偏好只影响本人 | TC-015-050 |

### AC-S-170 异步投递不重复不遗漏（7 条）

| 步骤 | 用例 |
|---|---|
| **步骤 1-3 ⭐ 100 条 + 4 并发 → 全 delivered 且计数恰好 100** | **TC-015-058** |
| `SKIP LOCKED` 批次两两不相交 | TC-015-059 |
| 路径 2 未到期不取 | TC-015-060 |
| 路径 3 `delivering` 10 分钟 → 回收 | TC-015-061 |
| 路径 3 另一侧：5 秒未超阈值不回收 | TC-015-062 |
| 路径 4 已 delivered 幂等 | TC-015-063 |
| at-least-once 边界（已发未写回 → 重投） | TC-015-064 |

### AC-S-171 重试退避（7 条）

| 步骤 | 用例 |
|---|---|
| 步骤 1 第 5 次失败即 dead_letter | TC-015-065 |
| 步骤 2 `backoff` 返回 `[30,60,120,240]` | TC-015-066 |
| 路径 2 上限截断 → 恰好 3600 | TC-015-067 |
| **路径 3 ⭐ `maxAttempts=2` → 第 2 次即死信** | **TC-015-068** |
| 路径 3 补：`retryBaseSeconds` 联动 | TC-015-069 |
| 路径 4 无 `f64`（含阳性对照） | TC-015-070 |
| 溢出防护 | TC-015-071 |

### AC-S-172 死信重投（6 条）

| 步骤 | 用例 |
|---|---|
| 步骤 1 resend → 200 三字段重置 | TC-015-072 |
| 步骤 2 delivered → 409 字段不变 | TC-015-073 |
| 步骤 3 pending → 409 字段不变 | TC-015-074 |
| 步骤 4 不存在 → 404 `NOT_FOUND` | TC-015-075 |
| **步骤 5 ⭐ 并发 2 resend → 恰好 1 个 200 / 1 个 409** | **TC-015-076** |
| 并发后计数只 +1 | TC-015-077 |

### AC-S-173 投递记录（7 条）

| 步骤 | 用例 |
|---|---|
| 步骤 1-2 快照语义 | TC-015-051 |
| 步骤 3 列表租户隔离 | TC-015-052 |
| 路径 2 camelCase 敏感键 4 种 | TC-015-053 |
| 路径 3 `secretary` 不误伤 | TC-015-054 |
| 路径 4 直连 RLS 隔离 | TC-015-055 |
| 路径 5 列集合恰好 18 列（显式枚举） | TC-015-056 |
| 出队部分索引 | TC-015-057 |

### AC-S-174 权限点 / C-5 / 对账 / 审计（16 条）

| 步骤 | 用例 |
|---|---|
| 1a `capabilities/` 恰好 14 文件 | TC-015-078 |
| 1a `permissions.json` total=38 + 3 码 | TC-015-079 |
| 1b `contract_c5` 通过 | TC-015-080 |
| 1c 行数钉死 + `REQ_015_NEW_CODES=8` | TC-015-081 |
| 1c `all_codes()` 加 `ALL_CODES`（防假绿） | TC-015-082 |
| 1c 两词库各 107 条 | TC-015-083 |
| 1c `notify.*` 不进 `TITLES` | TC-015-084 |
| 步骤 2 fail-fast | TC-015-085 |
| 步骤 2 阳性对照（补种子后能起） | TC-015-086 |
| 步骤 3 删调用点 → C-5 红 | TC-015-087 |
| 步骤 4 放 `authorize` → C-5 红 | TC-015-088 |
| 步骤 4 正向体检（8 禁区名零命中） | TC-015-089 |
| 步骤 5 恰好 4 个事件码 | TC-015-090 |
| 步骤 5 C12 九字段 + object_type | TC-015-091 |
| 步骤 5 哈希链 verify | TC-015-092 |
| 步骤 5 审计租户隔离 | TC-015-093 |

**覆盖率：10/10 AC，且每条 AC 的每一条步骤/路径都有对应用例。**

## 防恒真设计说明（本用例集的核心要求）

任务要求几处「最容易被写成恒真」的地方必须给有区分力的判据，逐条说明做法：

### 1. AC-S-023「没有重复投递已成功的」→ 只能数 provider 调用

数据库状态**不够**：若实现又发了一次、然后状态写回原值，`status` / `attempt_count` /
`deliveredAt` 三者全都不变 —— 三条断言全绿，但消息确实被重复投递了。

**怎么数**：用 `CountingSender` 夹具 —— 一个实现 `NotificationSender` 的 struct，
内部持 `Arc<AtomicUsize>`，每次 `send()` 被调用就 `fetch_add(1)`。它占据
`ChannelRegistry` 的 `in_app` 位。**计数器不会被 DB 覆盖**，这是它比状态断言强的地方。

判据（TC-015-019 ④）：`CountingSender.count() == 1`，**在 resend + 两次额外 tick 之后仍然 == 1**。
配套 TC-015-020 / TC-015-021 用变异补丁证明该判据真的会红（不是摆设）。

### 2. AC-S-170「100 条全部投递且不重复」→ 两个数字都要断言

- **不遗漏**：`count(status='delivered') == 100`
- **不重复**：`CountingSender.count() == 100`

只断言前者时，「同一行投两次、状态仍是 delivered、attempt_count 仍是 1」的坏实现会假绿。
所以 TC-015-058 把两个数字都钉成 `== 100`（不是 `>=`），并追加「每行 `attempt_count == 1`」
（100 行逐行检查）。TC-015-059 再用「4 个批次的 `claimed_ids` 两两交集为空」直接验
`SKIP LOCKED` 本身。

### 3. AC-S-171「配置生效」→ 必须能区分「写死 5 次」与「读配置的 2 次」

TC-015-068 把 `maxAttempts` 改成 **2**，断言**第 2 次**失败后即 `dead_letter`，
且 `FakeSmtp.call_count("MAIL") == 2`（**恰好 2**）。
写死 5 次的实现会投 5 次 → 计数 5 → 红。
它与 TC-015-065（配置 5 → 死信在第 5 次）配对：**只改一个配置项，死信点随之移动**，
这才证明配置通路真的通。

### 4. AC-S-172「并发两个 resend 恰好 1 成功 1 失败」→ 并发怎么构造

**不能**「连着发两次」—— 顺序执行必然一成一败，那样的测试对「先 SELECT 后 UPDATE」的
坏实现**也是绿的**（假绿）。

TC-015-076 的做法：两个独立连接 + `tokio::sync::Barrier::new(2)`，在两个 task 的
「进入事务后、执行 UPDATE 前」同步，然后各自发
`UPDATE ... WHERE id=$1 AND status='dead_letter' RETURNING *`，读 `rows_affected`。
断言两个 `rows_affected` 的**集合恰好 `{1, 0}`**。
判据直接落在 AC 要求的「SQL 影响行数守卫」上 ——「先 SELECT」的实现在这里会给出 `{1,1}` → 红。

### 5. AC-S-166 步骤 7「连上但不发 220」→ 假服务器怎么写

`FakeSmtp::silent()`：`TcpListener::accept()` 拿到连接后，**保持 socket 打开、一个字节都不写**。
客户端在等 banner `220`，读不到 → 卡到超时。断言三件事：返回 `Err`、
文案含 timeout 语义、耗时落在 `10s ~ 12s`（既不是无限挂起，也不是 100ms 就放弃）。
再加一条 `call_count("AUTH") == 0`（超时发生在 banner 阶段，没走到认证）。

### 6. AC-S-166 SMTP 细节

**AUTH LOGIN 两次挑战应答**（TC-015-010）：假服务器回 `334` → 收 base64(user) → 回 `334` →
收 base64(pass) → 回 `235`。断言**恰好收到 2 段** base64 载荷，且解码后分别是 user 与 pass
（顺序不能反）。一次性 `AUTH LOGIN <b64>` 的写法只会发 1 段 → 红。

**TLS 后必须重发 EHLO**（TC-015-009）：假服务器设 `auth_only_after_tls = true` ——
**TLS 握手完成之前**的 `EHLO` 应答里**不宣告 `AUTH`**，只有 TLS **之后**的 `EHLO` 才回
`250 AUTH LOGIN`。客户端若不重发 EHLO，就发现不了 `AUTH` 能力 → 投递失败。
断言命令序列**恰好**是 `["EHLO","STARTTLS","EHLO","AUTH","MAIL","RCPT","RCPT","DATA","QUIT"]`
（`EHLO` 出现 2 次）。

### 7. AC-S-169「偏好表里不得出现 `system` 行」→ 直接查库断言行数

TC-015-046 用 `base` 连接（绕过 RLS）跑
`SELECT count(*) FROM platform_notify_preference WHERE tenant_id='tenant-a' AND user_id='u-notify@example.com' AND category='system'`，
断言**逐字等于 0**（不是「不大于 1」）。
只信响应码会漏掉「先写后报错」—— 若实现先 `INSERT` 再校验并报 400，用户的 `system`
就**真的被关掉过**（哪怕事务回滚，非事务路径或部分失败仍会留痕）。

## 自动化可行性说明

| AC | 测试类型（design §9） | 落点 | 可行性 |
|---|---|---|---|
| 023 | 集成（真实库） | `tests/notify_resend.rs` | ✅ 需 `base_test` 库 + `base` 造数 |
| 166 | 单测（假 SMTP / 假 HTTP） | `tests/channels.rs` + `src/channels/smtp.rs` 内联 | ✅ **不连外网**，`TcpListener::bind("127.0.0.1:0")` |
| 167 | 单测（**纯函数**，无库无网） | `src/template.rs` 内联 `#[cfg(test)]` | ✅ 最快，无依赖 |
| 168 | 集成（真实库） | `tests/notify_locale.rs` | ✅ 需库 |
| 169 | 集成（真实库） | `tests/notify_preference.rs` | ✅ 需库 |
| 170 | 集成（真实库，**并发**） | `tests/notify_worker.rs` | ✅ 需 `tokio::join!` + 多连接 |
| 171 | 单测（纯函数）+ 集成 | `src/worker.rs` 内联 + `tests/notify_retry.rs` | ✅ 纯函数部分无依赖 |
| 172 | 集成（真实库，**并发**） | `tests/notify_resend.rs` | ✅ 需 `Barrier` 保证真并发 |
| 173 | 集成（真实库）+ 静态扫描 | `tests/notify_delivery.rs` | ✅ 需库 + `information_schema` |
| 174 | 契约 + 集成 | `tests/contract_c5.rs` + `tests/notify_audit.rs` | ✅ 需 `cargo xtask` 可跑 |

**E2E**：`e2e/notify.spec.ts` —— 建模板 → 预览 → 入队 → 重投（**1 条主链路**），
不在 E2E 里重跑 170/172 的并发语义（design §9）。

**本轮未执行的验证**（按要求）：不跑 `cargo test` / `pnpm test` / `cargo build`，
不查库（本机无 `psql`）。本集是**用例设计产物**，其执行属于 Dev/QA 下一轮。

**夹具清单**（多条用例共用，须在测试代码里实现一次）：

1. `FakeSmtp` —— 可编排应答的 `TcpListener`（`script` / `on(verb,code)` / `silent()` /
   `auth_only_after_tls` / `call_count(verb)` / `tls_handshakes`）
2. `CountingSender` —— 实现 `NotificationSender`，`Arc<AtomicUsize>` 计数
3. `FakeHttp` —— Webhook 用的假 HTTP 服务器
4. `db` 双连接 + `Barrier` —— 并发用例 076 / 058 用

## 〔AC 修正〕清单

> ✅ **2026-11-21 更新：这 7 处已全部被 BA 采纳，回填进 `acceptance.md` rev 3**
> （裁决见 `decisions.md` 的 **D308~D318**，挑刺报告见
> `BA/feature-meta/qa-req015-spec-review.md`）。
> 从此表起，用例里的〔AC 修正〕标记 **= 已生效的 AC 正文**，不再是"建议"。
>
> ⚠️ 其中 3 处升级为更高要求：
> - AC-S-169 步骤 3 → 钉死 **`attempt_count == 0`**（D312）
> - AC-S-170 步骤 5 → 补**阴性对照**（未超阈值不回收）+ 新增第 5 个配置项（D313）
> - AC-S-174 步骤 1c → 判据改成**读源码断言**（D318）

以下 7 处原本在用例里用〔AC 修正〕给出了修正后判据，
原因都是「AC 原文留了判定空间，按原文字面写会得到一个恒真或无法执行的断言」：

| # | AC 位置 | 原文的缺口 | 本集给的修正后判据 | 用例 |
|---|---|---|---|---|
| 1 | AC-S-169 步骤 3 | 只说进死信，**没说** `attempt_count` 是 0 还是 1 | 实现须钉死一个具体值，测试断言该值（拒绝「二者皆可」） | TC-015-043 ⑤ |
| 2 | AC-S-173 步骤 4 | 只说「必须用 camelCase」，未指出命中靠**归一化** | 补 `clientSecret`→`client_secret`、`apiKey`→`api_key` 两条**逐个**断言（`audit.rs:452-453`） | TC-015-053 ③⑤ |
| 3 | AC-S-172 步骤 5 | 说「恰好 1 个 200、1 个 409」，但**没给并发构造法** | `Barrier` + 双 SQL，断言 `rows_affected` 集合 `== {1,0}` | TC-015-076 |
| 4 | AC-S-170 步骤 5 | 「10 分钟前」的阈值只有**超阈值一侧**，缺未超阈值的对照 | 补 5 秒未超阈值 → 不回收（否则「见 delivering 就抢」也会绿） | TC-015-062 |
| 5 | AC-S-166 步骤 7 | 「含超时（10 秒）」未给**可断言区间** | `10s ≤ elapsed ≤ 12s` + `call_count("AUTH") == 0` | TC-015-007 |
| 6 | AC-S-166 步骤 2 | 只说 4 个通道 `send()` 返回 `Err`，未封死 `delivered:false` 形态 | 明确禁止 `Ok(DeliveryResult{delivered:false})`（D292 铁律） | TC-015-002 ② |
| 7 | AC-S-167 步骤 11 | SSTI 载荷未说「若真被 eval 了怎么判」 | `catch_unwind` 断言不 panic 且返回 `Err(INVALID_PLACEHOLDER)` | TC-015-033 ⑤ |

另有 2 处**补强**（不是修正，AC 没要求但不补就会留下假绿口子）：

- **TC-015-082**：AC-S-174 步骤 1c 说「`all_codes()` 必须加 `platform_notify::error::ALL_CODES`，
  否则**会假绿**」。用例因此把判据做成「读源码断言该行存在 + 集合含 8 个码」，
  **不能**只看 `titles_cover_every_code` 是绿的 —— 因为漏加时它恰好就是绿的。
- **TC-015-077**：并发 resend 之后计数只 +1，但**单靠计数抓不到「两个都成功」**
  （重置叠加后仍只有一行 pending）。用例里明确写出「本条必须与 TC-015-076 的
  `rows_affected` 判据配对」，防止后人误以为计数可替代行数守卫。

## 与设计的一致性核对

用例引用的表名 / 字段名 / 端点 / 环境变量全部取自 `design-summary.md`，未自造：

- 3 张表：`platform_notify_template` / `platform_notify_preference` / `platform_notify_delivery`（§2）
- 投递表 **18 列**（§2.3 / D299），逐列写进 TC-015-056 的显式集合
- 唯一约束 `UNIQUE (tenant_id, code, channel, locale)`（§2.1 / D294）
- 偏好主键 `PRIMARY KEY (tenant_id, user_id, category)`（§2.2 / D295）
- 出队部分索引 `(tenant_id, status, next_attempt_at) WHERE status IN ('pending','delivering')`（§2.3）
- 8 个端点 + 2 个偏好端点（§3，实际路由 10 条）
- camelCase 映射：`templateId` / `templateCode` / `titleTemplate` / `bodyTemplate` /
  `deliveryId` / `attemptCount` / `nextAttemptAt` / `lastError` / `providerMessageId` /
  `renderedTitle` / `renderedBody` / `deliveredAt`（§3.1）
- SMTP 协议 11 步（§4.6）：`220` → `EHLO` → `STARTTLS` → rustls → `EHLO` → `AUTH LOGIN` →
  `MAIL FROM` → `RCPT TO`×2 → `DATA` → `QUIT`
- 8 个错误码与 HTTP 状态（§5 / acceptance 全局约定表）
- 4 个配置项与默认值 5 / 30 / 2 / 3600（§6）
- SMTP 5 个环境变量走 env 不走 manifest（D306，`manifest.rs:189-207` 无 `Secret` 类型）
- C-5 命名禁区 8 个别名（§10，`authz_scan.rs:48`）
- `capabilities/` 恰好 14 文件、`permissions.json` total 35→38（§6 / §7）

**本需求不做的部分**（AC-S-166.b）在用例里只验「返回 `NOTIFY_CHANNEL_NOT_CONFIGURED`」
（TC-015-003 / TC-015-004）与「无真实实现、无隐式 TLS、无多部分 MIME」（TC-015-015），
**不**设计任何短信 / IM 真实投递的成功路径。
