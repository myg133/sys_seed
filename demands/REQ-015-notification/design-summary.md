# 设计摘要：通知系统（REQ-015）

- **需求编号**: REQ-015
- **状态**: rev 2（2026-11-21 —— 邮件改真实现，D292 rev 2 / D305~D307）
- **基线代码**: `develop` @ `a2b7e52`
- **决策依据**: [decisions.md](decisions.md)（D291~D307）
- **验收标准**: [acceptance.md](acceptance.md)（**10 条**：AC-S-023 重写 + AC-S-166~174）

---

## 1. 架构与依赖

```
                    ┌──────────────────────────────┐
   HTTP（平台端点）  │  platform-notify/http.rs     │  8 个端点
                    │  authz.rs（3 个权限点）      │
                    └──────────────┬───────────────┘
                                   │
                    ┌──────────────▼───────────────┐
                    │  service.rs（编排）           │
                    │  入队 / 渲染 / 偏好检查        │
                    └──────┬───────────────┬───────┘
                           │               │
              ┌────────────▼──┐   ┌────────▼─────────┐
              │ template.rs   │   │ store.rs         │
              │ 受限变量替换   │   │ TenantTx 数据访问 │
              └───────────────┘   └──────────────────┘
                                           │
                    ┌──────────────────────▼───────────────┐
                    │  worker.rs  tick()                   │
                    │  SELECT ... FOR UPDATE SKIP LOCKED   │
                    └──────────────────────┬───────────────┘
                                           │
                    ┌──────────────────────▼───────────────┐
                    │  channels/mod.rs  ChannelRegistry    │
                    └───┬──────────┬──────────────┬────────┘
                        │          │              │
                   ┌────▼───┐ ┌────▼────┐  ┌──────▼──────┐
                   │in_app  │ │smtp     │  │webhook      │  真实现 ×3
                   └────────┘ └─────────┘  └─────────────┘
                   ┌────────────────────────────────────┐
                   │sms / dingtalk / wecom / feishu     │  显式未配置 ×4
                   │→ Err(NOTIFY_CHANNEL_NOT_CONFIGURED)│
                   └────────────────────────────────────┘
```

### crate 依赖

```toml
[dependencies]
platform-core.workspace = true     # RequestContext / NotificationSender trait / ProblemDetails
platform-db.workspace = true       # TenantDb / TenantTx（RLS 兜底）
platform-config.workspace = true   # 读 4 个 notify.* 配置 + 默认 locale
axum / tokio / serde / serde_json / tracing / thiserror / uuid / time   # 与其余 crate 同款
# 出站 HTTP（Webhook）：与 platform-oauth 的 OutboundClient 同款栈，但**自己写薄的**
hyper / hyper-util / http / http-body-util / bytes / hyper-rustls / rustls
# 出站 SMTP：**裸 TCP + STARTTLS**（D305）
tokio-rustls.workspace = true      # ⚠️ 现为 platform-iam 的 dev 依赖 → 转正式（D307）
webpki-roots = "1"                 # ⚠️ 已在 lock（hyper-rustls 传递引入）→ 加 workspace 条目（D307）
base64.workspace = true            # MIME base64 + AUTH LOGIN

[dev-dependencies]
sqlx.workspace = true              # 真实库集成测试
platform-contracts.workspace = true # C-5 契约扫描（真 C-5 命令）
tower.workspace = true             # 驱动 axum Router（oneshot）
```

> ⚠️ **不依赖 `platform-rbac`**：权限用 `platform-core` 的 `require_permission` 注入（D302）。
> ⚠️ **不依赖 `platform-oauth`**：虽然那里有 `OutboundClient`（`outbound.rs:61`），
> 但「通知依赖 OAuth 服务器」语义不对（D302 / D220 同款理由）→ 自己写约 60 行出站。

---

## 2. 数据模型（3 张表）

### 2.1 `platform_notify_template` —— 模板（**租户级**，启用 RLS）

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | `uuid` | PK |
| `tenant_id` | `text` | |
| `code` | `text` | 模板编码（`^[a-z][a-z0-9_.]*$`） |
| `channel` | `text` | `in_app` / `smtp` / `webhook` / `sms` / `dingtalk` / `wecom` / `feishu` |
| `locale` | `text` | BCP-47（`zh-CN` / `en-US`） |
| `category` | `text` | `system` / `marketing` / `transactional` |
| `title_template` | `text` | |
| `body_template` | `text` | |
| `created_at` / `updated_at` | `timestamptz` | |

```sql
UNIQUE (tenant_id, code, channel, locale)   -- D294：多语言按行存
```

⚠️ **唯一约束必须含 `locale`** —— 否则同一模板无法有第二个语言（AC-S-168 步骤 1 的判据）。

### 2.2 `platform_notify_preference` —— 订阅偏好（**租户级**，启用 RLS）

| 列 | 类型 | 说明 |
|---|---|---|
| `tenant_id` | `text` | |
| `user_id` | `text` | |
| `category` | `text` | |
| `opted_out` | `boolean` | `NOT NULL DEFAULT false` |
| `updated_at` | `timestamptz` | |

```sql
PRIMARY KEY (tenant_id, user_id, category)   -- D295
```

### 2.3 `platform_notify_delivery` —— 投递记录（**租户级**，启用 RLS）

**18 列**（D299，逐列见 [acceptance.md](acceptance.md) AC-S-173 步骤 7）：

```sql
id / tenant_id / template_code / channel / category / recipient / locale /
payload(jsonb) / rendered_title / rendered_body / status / attempt_count /
next_attempt_at / last_error / provider_message_id /
created_at / updated_at / delivered_at
```

```sql
CREATE INDEX ON platform_notify_delivery (tenant_id, status, next_attempt_at)
  WHERE status IN ('pending', 'delivering');   -- 出队索引
```

⚠️ **`rendered_title` / `rendered_body` 是快照**（D299）：
模板后续修改**不影响**已投递记录（可追溯；AC-S-173 步骤 2）。

### 2.4 RLS

三张表全部在 `crates/platform-notify/manifest.json` 的 `tenantScoped.tables` 里声明：

```json
"tenantScoped": { "tables": ["platform_notify_template", "platform_notify_preference",
                             "platform_notify_delivery"], "column": "tenant_id" }
```

→ `cargo xtask tenant-policies` 生成 policy；校验命令会检查**每张声明的表都真有 policy**
（漏建 = 完全无隔离，`crates/xtask/src/tenant_policies.rs:42`）。

---

## 3. 端点清单（**8 个**，一律 Problem Details + camelCase）

| # | 方法 | 路径 | 权限 | AC |
|---|---|---|---|---|
| 1 | `GET` | `/api/notify/templates` | `notify:template:read` | 167 |
| 2 | `GET` | `/api/notify/templates/{id}` | `notify:template:read` | 167 |
| 3 | `POST` | `/api/notify/templates` | `notify:template:write` | 167 |
| 4 | `PATCH` | `/api/notify/templates/{id}` | `notify:template:write` | 167 |
| 5 | `DELETE` | `/api/notify/templates/{id}` | `notify:template:write` | 167 |
| 6 | `POST` | `/api/notify/templates/{id}/preview` | `notify:template:read` | 167 步骤 4 |
| 7 | `GET` | `/api/notify/deliveries` | `notify:delivery:read` | 173 |
| 8 | `POST` | `/api/notify/deliveries/{id}/resend` | `notify:delivery:read` | 172 / 023 |
| — | `GET` | `/api/notify/preferences` | **登录即可** | 169 |
| — | `PUT` | `/api/notify/preferences` | **登录即可** | 169 |

> 表里 8 个带权限点；偏好 2 个**不进权限点**（D300：那是用户自己的设置）。
> 实际路由数 = 10。

### 3.1 跨栈 JSON 字段（D258 同款，钉死）

**平台端点一律 camelCase**（本需求**没有**协议端点，所以不存在两套）：

| JSON | 列 |
|---|---|
| `templateId` | `id` |
| `templateCode` | `code` |
| `titleTemplate` / `bodyTemplate` | `title_template` / `body_template` |
| `deliveryId` | `id`（delivery 表） |
| `attemptCount` | `attempt_count` |
| `nextAttemptAt` | `next_attempt_at` |
| `lastError` | `last_error` |
| `providerMessageId` | `provider_message_id` |
| `renderedTitle` / `renderedBody` | `rendered_title` / `rendered_body` |
| `deliveredAt` | `delivered_at` |

---

## 4. 关键流程

### 4.1 模板渲染（AC-S-167，D293）

```
输入：body_template = "你好 {{name}}，订单 {{order_id}} 已发货"
      payload = {name: "张三", order_id: "A-1"}

① 词法扫描 body_template，找所有 "{{" … "}}"
② 每个占位符里的内容必须匹配 ^[a-z][a-z0-9_]*$   ← 不匹配 → 400 INVALID_PLACEHOLDER
③ 每个变量名必须在 payload 里存在                ← 缺失   → 400 MISSING_VARIABLE（点名）
④ 逐个替换（**单趟**，替换结果不再二次扫描 → 防止 payload 值里的 {{x}} 被展开）
⑤ payload 里多余的键**忽略**                     ← 不报错（AC-S-167 步骤 8）
```

⚠️ **第 ④ 步是安全要点**：若对替换结果再扫一遍，攻击者可以把 `{{x}}` 放进 payload
的值里，让它在第二轮被展开 → **模板注入**。**必须单趟**。

⚠️ **不做**条件 / 循环 / 表达式 / 部分引用（`{{#if}}` / `{{x.y}}` / `{{> p}}`）
→ 全部落到 `INVALID_PLACEHOLDER`（AC-S-167 步骤 11）。

### 4.2 locale 三级回退（AC-S-168，D294）

```
请求 locale（用户偏好）
    ↓ 没有该 (code, channel, locale) 行
租户默认 locale（`platform-config` 的 tenant 作用域，缺省 `zh-CN`）
    ↓ 也没有
**失败**：NOTIFY_TEMPLATE_NOT_FOUND
```

⚠️ **不是**「回退到任意一个存在的 locale」—— 那会让用户收到看不懂的语言。

### 4.3 异步投递（AC-S-170，D296）

```rust
// worker.rs
pub async fn tick(db: &TenantDb, ctx: &RequestContext, batch: i64) -> Result<TickStats> {
    let mut tx = db.begin(ctx).await?;
    // ① 取一批（**必须是 FOR UPDATE SKIP LOCKED**）
    let rows: Vec<DeliveryRow> = tx.fetch_all(&TenantQuery::new(
        "SELECT ... FROM {tenant}.platform_notify_delivery
         WHERE status IN ('pending', 'delivering')
           AND next_attempt_at <= now()
         ORDER BY next_attempt_at
         LIMIT ?
         FOR UPDATE SKIP LOCKED")).await?;
    // ② 逐条置 delivering
    // ③ 逐条投递（投递本身**不在事务里**—— 见下）
    // ④ 更新状态
    tx.commit().await?;
}
```

⚠️ **一个必须处理的张力**：`FOR UPDATE` 持有行锁到事务结束，而投递要发网络请求
（可能 10 秒）→ 长时间持锁会拖死其他 worker。

**处理方式（Dev 必须二选一并写明理由）**：

| 方案 | 做法 | 代价 |
|---|---|---|
| **A（推荐）** | ① 短事务只做「**认领**」：`UPDATE ... SET status='delivering', updated_at=now() WHERE id IN (...) RETURNING *`，然后**提交**；② 在事务**外**投递；③ 另一个短事务写回结果 | 崩溃时留下 `delivering` 残留 → 由「`delivering` 超时回收」兜底（AC-S-170 步骤 5 就测这个） |
| B | 长事务持锁跨网络请求 | 锁持有时间 = 网络超时；一个慢上游会阻塞整批 |

→ **AC-S-170 步骤 5 的「`delivering` 超时回收」不是可选项，是方案 A 的必需配套**。

### 4.4 重试退避（AC-S-171，D297）

```rust
// 纯函数，**可单测**
fn backoff(attempt: i32, base: i64, multiplier: i64, max: i64) -> Duration {
    // base * multiplier^(attempt-1)，用整数运算，最后 clamp 到 max
    // ⚠️ 必须防溢出：multiplier.powi 在 attempt 大时会溢出 i64 → 用 checked_* 或先比较再乘
}
```

默认值下 `attempt = 1,2,3,4` → `[30, 60, 120, 240]` 秒。

⚠️ **用 `time` crate，不用浮点**（D297）：低配机 + 确定性断言。
⚠️ **溢出防护**：`retryMaxSeconds` 存在的意义就是防爆炸，
但**计算过程本身**必须先防溢出（`i64` 上 `2^63` 会 panic 或回绕）。

### 4.5 死信重投（AC-S-172，D298）

```sql
UPDATE {tenant}.platform_notify_delivery
   SET status = 'pending', attempt_count = 0, next_attempt_at = now(), last_error = NULL
 WHERE id = ? AND status = 'dead_letter'
RETURNING *;
```

⚠️ **必须用「影响行数 = 0 → 409」守卫**，**不是**「先 SELECT 查状态再 UPDATE」：
后者在并发下两条请求都读到 `dead_letter` → **两条都成功**（AC-S-172 步骤 5 就测这个）。

### 4.6 SMTP 投递（AC-S-166，D305）

```
TCP 连接 → 220
EHLO <host>            → 250-…（**读到 "250 " 空格才停**）
STARTTLS               → 220 → rustls 握手（ServerName 来自 host，**不得是 IP**）
EHLO <host>            → 250-…（**TLS 之后必须重发**：能力集变了，TLS 前看不到 AUTH）
AUTH LOGIN             → 334 → b64(user) → 334 → b64(pass) → 235
MAIL FROM:<from>       → 250
RCPT TO:<r1>           → 250（**任一被拒 → 整条失败**）
RCPT TO:<r2>           → 250
DATA                   → 354 → 头 + 空行 + base64 正文（每 76 字符换行）+ "." 行 → 250
QUIT                   → 221
```

⚠️ **头注入防御**：`To` / `From` / `Subject` 里的 `\r` / `\n` **必须剥除或拒绝**。
否则 `Subject: x\r\nBcc: attacker@evil` 会注入额外收件人 —— **这是一条安全判据**。

⚠️ **base64 正文必须每 76 字符换行**（RFC 2045）；不换行会被严格 MTA 拒。

---

## 5. 错误码（**8 个**）

逐条见 [acceptance.md](acceptance.md) §全局约定 —— 每个码都指到了具体 AC 步骤。

⚠️ **`TITLES`：99 → 107**（实测基线 99，`crates/platform-core/src/titles.rs:45`）。
⚠️ **4 个事件码不进 `TITLES`**（D223）：
`notify.template.created` / `notify.template.updated` / `notify.template.deleted` /
`notify.dead_letter.resent`（三段式，与已交付的 `app.registry.*` / `oauth.client.*` 一致）。

### 5.1 ⚠️ 新增 8 个码必须同步改的**三处**（Dev 最容易漏）

这不是「加个常量」就完事的 —— 仓里有**三条既有机制**会因此变红或假绿：

| # | 位置 | 不改的后果 |
|---|---|---|
| 1 | `crates/platform-core/src/titles.rs:45` 的 `TITLES` **加 8 行** | 步骤 1c 的 `titles_cover_every_code` 红（取到 `FALLBACK_TITLE`） |
| 2 | `crates/platform-server/tests/i18n_titles.rs:174` 的 `the_title_table_has_the_pinned_number_of_rows` **加 `const REQ_015_NEW_CODES: usize = 8;`** | 该测试**硬编码**了 `58+21+5+9+6 = 99`；实际变 107 → **红** |
| 3 | 同文件 `:59` 的 `all_codes()` **加 `platform_notify::error::ALL_CODES`** | ⚠️ **会假绿**：新码不在清单里 → `titles_cover_every_code` 检查不到它们（表里缺了也不报） |
| 4 | `packages/i18n/src/locales/{zh-CN,en-US}.json` 各加 8 条 `error.<CODE>` | `every_error_code_is_registered_in_every_locale`（`:382`）红 |

> ⚠️ **第 3 条是最危险的**：它是唯一一个「不写不会红、但会让守卫失效」的改动
> —— 与 TD-047（`ensure_fresh` 只是散文不是机制）是同一类问题。

### 5.2 ⚠️ `org.*` 在 `TITLES` 里，`notify.*` 不进 —— 别照抄错的样板

`titles.rs:195-200` 里确实有 **6 个 `org.*` 事件码**，注释写明「platform-org 的通知标题
（事件码，不是 HTTP 错误码）」。**这不代表事件码都要进 `TITLES`**：

| | `org.*`（**在**表里） | `notify.*`（**不进**表） |
|---|---|---|
| 用途 | 通知标题的**文案来源** | 审计记录的 `action` 字段 |
| 谁查 | `OrgEventKind::title()` 显式调 `title_of_code()`（`crates/platform-org/src/notify.rs:144`） | 没人查 |
| 前端消费 | 是 | 否 |

→ 4 个 `notify.*` 若被塞进 `TITLES`，`TITLES.len()` 就是 **111 ≠ 107** → 步骤 1c 红。

---

## 6. 权限点与 manifest

```json
{
  "id": "notify",
  "name": "通知",
  "version": "0.1.0",
  "enabled": true,
  "permissions": [
    { "code": "notify:template:read",  "name": "查看通知模板", "type": "menu" },
    { "code": "notify:template:write", "name": "维护通知模板", "type": "button" },
    { "code": "notify:delivery:read",  "name": "查看投递记录", "type": "menu" }
  ],
  "menus": [
    { "label": "通知", "icon": "bell", "route": "/notify" }
  ],
  "tenantScoped": {
    "tables": ["platform_notify_template", "platform_notify_preference",
               "platform_notify_delivery"],
    "column": "tenant_id"
  },
  "dependencies": [],
  "config": {
    "notify.maxAttempts":       { "type": "number", "default": 5,    "scope": "platform", "label": "最多尝试次数（含首次）" },
    "notify.retryBaseSeconds":  { "type": "number", "default": 30,   "scope": "platform", "label": "首次重试延迟（秒）" },
    "notify.retryMultiplier":   { "type": "number", "default": 2,    "scope": "platform", "label": "退避倍数" },
    "notify.retryMaxSeconds":   { "type": "number", "default": 3600, "scope": "platform", "label": "单次重试延迟上限（秒）" },
    "notify.deliveringTimeoutSeconds": { "type": "number", "default": 300, "scope": "platform", "label": "delivering 残留状态的回收阈值（秒）" }
  }
}
```

⚠️ **5 个**配置项**全部** `"scope": "platform"` 且是 `ConfigSpec` **对象**
（REQ-016 的 S-01 / S-02 两条教训）。
⚠️ **SMTP 的 5 个环境变量不进 manifest**（`ConfigSpec` 无 `Secret` 类型，D306）。
⚠️ **键名必须是 lowerCamelCase**（契约 C9）：`ConfigKey::parse` 要求**每一段**
「小写字母开头 + 只含 ASCII 字母数字」（`crates/platform-core/src/config.rs:213`
的 `is_lower_camel_case`）→ **段内带 `_` 会让服务起不来**。
本需求 5 个键已逐段核对合规。

### 权限点总数

`capabilities/permissions.json` 的 `total`：**35 → 38**（实测基线 35 = 6+3+3+4+6+9+4）。
⚠️ `capabilities/` 文件数**不增不减**：**14**（7 固定 + 7 个 sdk 包）。

---

## 7. 迁移（3 个 + down）

| 文件 | 内容 |
|---|---|
| `20261121000400_notify_schema.{up,down}.sql` | 3 张表 + 索引 + `base_platform_notify` 组角色授权 |
| `20261121000500_notify_permission_seed.{up,down}.sql` | 3 个权限点 + 1 个菜单 |
| `20261121000600_notify_config_seed.{up,down}.sql` | 5 个配置项（`scope = platform`） |

⚠️ 命名沿用 `2026112100xxxx` 序列（现有最后一个是 `20261121000300_app_config_seed`）。

---

## 8. 前端交付

| 文件 | 内容 |
|---|---|
| `apps/shell/src/lib/notify-api.ts` | 4 个函数的薄封装（列表 / 建 / 改 / 重投） |
| `apps/shell/src/lib/notify-api.test.ts` | 单测（错误码 → 文案映射） |
| `apps/shell/src/routes/NotifyTemplates.svelte` | 模板列表 + 编辑（**含「预览」面板**） |
| `apps/shell/src/routes/NotifyDeliveries.svelte` | 投递记录 + 重投按钮 |
| `packages/i18n/src/locales/{zh-CN,en-US}.json` | 8 个错误码的文案 + 页面文案 |

⚠️ **不做**可视化模板编辑器（只用 textarea + 变量提示）；理由：`{{var}}` 语法极简，
富文本编辑器会引入依赖且与「受限变量替换」的定位不符。

---

## 9. 测试落点

| AC | 测试类型 | 落点 |
|---|---|---|
| 023 | 集成（真实库） | `tests/notify_resend.rs` |
| 166 | 单测（假 SMTP / 假 HTTP）+ 集成 | `tests/channels.rs` + `src/channels/smtp.rs` 内联单测 |
| 167 | 单测（**纯函数**，无库无网） | `src/template.rs` 内联 `#[cfg(test)]` |
| 168 | 集成（真实库） | `tests/notify_locale.rs` |
| 169 | 集成（真实库） | `tests/notify_preference.rs` |
| 170 | 集成（真实库，**并发**） | `tests/notify_worker.rs` |
| 171 | 单测（**纯函数**）+ 集成 | `src/worker.rs` 内联 + `tests/notify_retry.rs` |
| 172 | 集成（真实库，**并发**） | `tests/notify_resend.rs` |
| 173 | 集成（真实库）+ 静态扫描 | `tests/notify_delivery.rs` |
| 174 | 契约（`contract_c5`）+ 集成 | `tests/contract_c5.rs` + `tests/notify_audit.rs` |

**E2E**：`e2e/notify.spec.ts` —— 建模板 → 预览 → 入队 → 重投（**1 条主链路**即可，
不在 E2E 里重跑 170/172 的并发语义）。

---

## 10. ⚠️ 命名禁区（C-5，D216）

`crates/platform-contracts/src/authz_scan.rs:48` 的禁区表含 **`authorize`** 等 8 个别名。
**在 `crates/platform-notify/src/` 里不得出现**这些名字作为函数名 —— 会被 C-5 判为「第二个鉴权入口」。

⚠️ 本 crate 里极易踩的同义命名：`authorize` / `check_permission` / `can` / `allow`
（**通知域里「是否允许给该用户发」听起来很像鉴权** —— 但它不是，要用别的名字，
如 `is_recipient_eligible`）。

---

## 11. ⚠️ TD-047 的教训（本需求必须遵守）

`ensure_fresh` 是「写在文档里的散文」，不是「机制」→ 结果 `platform-oauth` 的
`authenticate` 漏调它，第一次必 403（REQ-022 修复轮抓到的真 bug）。

**对应到本需求**：以下几种「散文要求」必须落成**可执行断言**，不许只写在规格里：

| 散文要求 | 必须落成的机制 |
|---|---|
| 「未配置 ≠ 可用」 | 单测：4 个未配置通道的 `send()` **必须返回 `Err`**（AC-S-166 步骤 3/4） |
| 「不重复投递已成功的」 | 断言 provider **调用计数**（不是只看库状态）（AC-S-023 步骤 6） |
| 「影响行数守卫」 | 断言**并发两个 resend 恰好 1 赢 1 输**（AC-S-172 步骤 5） |
| 「头注入防御」 | 单测：`Subject` 含 `\r\n` 时**不发出去**（AC-S-166 的安全判据） |
| 「退订留痕」 | 断言退订后是 `dead_letter` 而**不是**静默丢弃（AC-S-169 步骤 3） |

---

## 12. 已识别的风险

| # | 风险 | 缓解 |
|---|---|---|
| 1 | **SMTP 是自研协议实现**，易漏 `EHLO` 重发 / 多行 250 / 76 字符换行 | D305 的 6 条假服务器用例逐条覆盖；**每条都能独立变红** |
| 2 | **`FOR UPDATE` 跨网络请求持锁** | 方案 A（短事务认领 + 事务外投递）+ `delivering` 超时回收（§4.3） |
| 3 | **退避计算的整数溢出** | 用 `checked_*` 或先比较再乘；`retryMaxSeconds` 不是防溢出机制 |
| 4 | **多语言回退"看起来对但取错行"** | AC-S-168 的四条路径分别断言**具体文案**（不是只断言「有结果」） |
| 5 | 短信 / IM 未实现 | **TD-054** 明码记账；4 个通道返回明确错误而非静默成功 |
| 6 | SMTP 隐式 TLS（465）未做 | **TD-055** |
