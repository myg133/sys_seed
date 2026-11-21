# 决策记录：通知系统（REQ-015）

- **需求编号**: REQ-015
- **父需求**: REQ-001
- **状态**: rev 1（2026-11-21 —— 补齐四件套：1 条空泛 AC → **9 条**）
- **基线代码**: `develop` @ `a2b7e52`（REQ-016 已合并）
- **依赖**: REQ-002 ✅ / REQ-021 ✅ / **REQ-014 审计 ✅**（本需求要复用审计 sink）

---

## 0. 需求来源披露（D122 / dev-guide §9.1）

**REQ-015 的需求来源是 BA 补的，不是用户原话。** 这一点必须说清。

**用户原话**（2026-11-17 的 9 项需求清单）里**没有「通知」**这一项。
`roadmap.md` 把它标为「⚠️ **BA 补**（用户已裁定继续）」——
用户在 2026-11-17 之后**明确裁定「继续」**（见 `roadmap.md` 的 REQ-015 行）。

→ **本需求的具体形态（9 条 AC / 通道抽象 / 重试退避 / 死信）全部是 BA 的设计**。
每一条都要有决策依据，不能凭空发明。

---

## D291: 通道抽象 ="producer 侧统一，**provider 侧可插拔**"

### 实测发现（`develop` @ `a2b7e52`）

**好消息**：扩展点的 trait **已经存在**（REQ-003 定的契约）：

```rust
// crates/platform-core/src/extension/notification.rs:81-92
pub trait NotificationSender: Send + Sync {
    fn channel(&self) -> &str;
    fn send(&self, ctx: &RequestContext, msg: &NotificationMessage)
        -> impl Future<Output = Result<DeliveryResult>> + Send;
}
```

**坏消息**：**全仓没有任何实现**（只有 `platform-org/src/notify.rs:211` 测试里的 `RecordingSender`）。

### 决定

**不新建通道 trait** —— 直接用 `NotificationSender`，本需求只做**三件事**：

1. **provider 实现**：站内信 / Webhook 两个**真实现**（其余三个见 D292）；
2. **编排层**：`platform-notify` crate（模板渲染 / 订阅偏好 / 异步投递 / 重试 / 死信 / 记录）；
3. **注册表**：按 `channel()` 把多个 provider 装配成一个 `ChannelRegistry`。

### 为什么**不**改 `NotificationSender` 的签名

它是 **REQ-003 交付的扩展点契约**，`extension-points.json` 冻了它
（`crates/xtask/src/capabilities.rs:45-51` 的 `EXTENSION_POINT_TRAITS`）。

⚠️ 改它 = **破坏已冻结的扩展点契约** → 所有 `manifest.json` 的 `dependencies` 都要跟着动
→ 这才是真正该避免的「跨需求改动」。

→ **需要的东西（重试、退避、死信）都在编排层做**，不进 trait。

---

## D292: 五类通道 —— **3 个真实现 + 2 个显式「未配置」**（rev 2，用户裁定）

> 🔴 **rev 2 修订（2026-11-21，用户裁定）**。rev 1 的结论是「2 个真实现（站内信 / Webhook）+
> 3 个显式未配置（邮件 / 短信 / IM）」，其中**驳回邮件的理由是错的**，必须记下来：
>
> rev 1 写「邮件：❌ `Cargo.lock` 里 0 个 SMTP 库（`lettre` / `mail-send` 都没有）→ 未配置」。
> **这条推理不成立**：写 SMTP 客户端**不需要** SMTP 库 —— SMTP 是纯文本行协议，
> 用 `tokio`（`net` 特性）+ `base64` 就能实现；TLS 那部分 `rustls` 已在树里（D123）。
> 「lock 里没有现成库」**只能证明**要自己写，**不能证明**写不出来。
>
> 而父 AC-023 的预期结果**明文写「邮件送达」**
> （`BA/demands/REQ-001-platform-foundation/acceptance.md:196`）。
> → 用一条站不住的理由把父 AC 的明文要求排除掉 = **规格自我降级**。
> **用户 2026-11-21 裁定：做真 SMTP 客户端。**

### 实测发现（`develop` @ `a2b7e52`）

| 通道 | 实现所需的东西在树里吗 | 结论 |
|---|---|---|
| **站内信**（in-app） | 不需要外部依赖（落库即可） | ✅ **真实现** |
| **Webhook** | ✅ `hyper` + `hyper-rustls`（`platform-oauth/src/outbound.rs:61` 的 `OutboundClient` 已在用） | ✅ **真实现** |
| **邮件**（SMTP） | ✅ `tokio`（workspace `Cargo.toml:46` 含 `net`）+ `rustls`（`:125`，D123）+ `base64`（`:90`）；假服务器夹具先例见 `crates/platform-iam/tests/sso_support/mod.rs:474` 的 `TcpListener::bind` | ✅ **真实现**（见 D305） |
| **短信** | ❌ 无任何厂商 SDK；且**无真实凭据**（短信 API 全是签名 + 实名模板报备） | ⚠️ **显式「未配置」** |
| **IM**（钉钉/企微/飞书） | ⚠️ 技术上都是 HTTPS POST，但每个平台要**先换 token**（企微 `access_token` / 飞书 `tenant_access_token`）或**算签名**（钉钉 `timestamp+sign`）→ **没有真实凭据就无法验签，写出来是「看起来能跑」**（REQ-022 的教训：判据恒真的实现没价值） | ⚠️ **显式「未配置」** |

### 决定

**本轮实现 3 个真通道**（in-app / webhook / **smtp**），**其余 2 个显式「未配置」**：

1. 邮件按 **D305** 自己写 SMTP 客户端，**不新增任何依赖**；
2. 短信与 IM 三个只登记 `NOTIFY_CHANNEL_NOT_CONFIGURED`，**不写假实现**；
3. 未配置的通道**必须显式失败**，返回 `NOTIFY_CHANNEL_NOT_CONFIGURED`（**不是静默成功**）。

### 一条必须遵守的规则

> **未配置 ≠ 可用**。若某通道未配置而订阅偏好选了它，
> 投递**立即进死信**并标 `NOTIFY_CHANNEL_NOT_CONFIGURED` ——
> **不得**静默当作成功（那是最坏的「假绿」）。

### 代价（记 TD-054，**rev 2 已缩窄**）

「五类通道可投递」这个**完成定义**本轮达成 **3/5**（rev 1 是 2/5）。
→ **TD-054** 收窄为：**短信 + IM** 两类未做，需**真实凭据**（短信要运营商报备模板；
IM 要各平台的 corp_id / secret / 签名算法）才能写出可验证的实现。

---

## D293: 模板引擎 —— **受限变量替换**，不是通用模板语言

### 问题

`demand.md` 写「模板引擎：变量、多语言、预览」。

### 决定

**只做 `{{variable}}` 形式的变量替换**，且：

1. **变量名白名单**：只允许 `[a-z][a-z0-9_]*`（小写 + 下划线 + 数字）；
2. **未知变量** → 渲染时**显式失败**（`NOTIFY_TEMPLATE_MISSING_VARIABLE`），**不静默留空**；
3. **不做** 条件 / 循环 / 函数调用 / 表达式求值 —— 那是通用模板语言（Handlebars/Tera 级），
   要引入依赖 + 带来 **SSTI（服务端模板注入）** 风险；
4. `NotificationMessage.payload` 是 `BTreeMap<String, String>`（**已存在的类型**）
   → 变量替换就基于它。

### 为什么不做通用模板语言

- **零新增依赖**约束；
- **SSTI 是真实的攻击面**：租户管理员能编辑模板 → 若支持表达式，等于给了他一个
  在服务端执行的入口（与 REQ-016 的 `javascript:` 同类问题）；
- 「变量 + 多语言」已经能满足通知的真实需求（`{{name}}，你的订单 {{orderId}} 已发货`）。

---

## D294: 多语言 = **模板按 locale 分行存**，**不做运行时翻译**

### 决定

`platform_notify_template` 的**主键含 `locale`**：

```sql
UNIQUE (tenant_id, code, channel, locale)
```

- 渲染时按「用户 locale → 租户默认 locale → `zh-CN`」**逐级回退**；
- **不做**「模板里写 `{{t.someKey}}` 再查 i18n 词库」—— 那是两层间接，
  且把「模板」和「界面文案」两个域混在一起。

### 为什么不用 i18n 词库

`demand.md` 说「模板：…多语言」。最直接的做法是**一个模板 x 一个 locale 一行**。
若复用 `packages/i18n` 的 `error.*` 机制，会混淆两个域：
- **界面文案**随语言切换（前端 `t()`）；
- **通知正文**是**发给用户的内容**，应该由**管理员按 locale 维护完整文案**（可能完全不同，不是翻译）。

→ 独立表，独立编辑界面。

---

## D295: 订阅偏好 —— **按「通知类型」退订**，强制类不可退订

### 决定

```sql
-- platform_notify_preference
PRIMARY KEY (tenant_id, user_id, category)
opted_out boolean NOT NULL DEFAULT false
```

- `category` 是**通知分类**（由模板的 `category` 字段决定，如 `marketing` / `system`）；
- ⚠️ **`system` 类不可退订**（强制通知：密码过期、账号被锁、安全告警）
  → 尝试退订 `system` 返回 **400 `NOTIFY_CATEGORY_MANDATORY`**。

### 为什么按 category 而不是按模板

按模板退订太细（用户不想逐个通知去关）；按 category 退订是**用户能理解的粒度**
（「营销类不要发」「系统告警必须收」）。

### 父 AC 的对应

父 AC-023 写「**用户可退订该类型**」→ 「该类型」= `category` ✓

---

## D296: 异步投递 = **数据库驱动的轮询 worker**，**不用进程内队列**

### 实测发现

`crates/platform-server/src/main.rs` **没有任何后台任务**
（`grep -c "JoinSet\|tokio::spawn\|interval("` = **0**）。

### 决定

1. **出队靠数据库**：`platform_notify_delivery` 有 `status` + `next_attempt_at`；
   worker 用 **`SELECT ... FOR UPDATE SKIP LOCKED`** 取一批 → 天然支持多实例；
2. **worker 是一个显式的 `xtask notify worker` 命令**（**不是** web 进程内的后台线程）：
   - 理由 1：**web 进程重启会丢正在投递的消息**（进程内队列的本质缺陷）；
   - 理由 2：web 进程**多实例**时各自持有队列 → 投递重复或遗漏；
   - 理由 3：**测试可控**（E2E / 集成测试可以直接调一次 worker 的 `tick()`，不用 sleep 等后台）；
3. **`tick()` 是纯函数式的一批处理**：取 N 条 → 逐条投 → 更新状态 → 返回统计。
   → **可单测、可 E2E 驱动、可手工重跑**。

### 为什么不用「投递时就地发」

那样「重试退避」「死信」就没地方存了；而且 HTTP 请求里等第三方超时 = 用户等 5 秒。

---

## D297: 重试退避 —— **指数退避 + 上限**，参数进配置

| 配置键 | 默认 | 说明 |
|---|---|---|
| `notify.maxAttempts` | `5` | 最多尝试几次（含首次） |
| `notify.retryBaseSeconds` | `30` | 第 1 次重试的延迟 |
| `notify.retryMultiplier` | `2` | 每次翻倍 |
| `notify.retryMaxSeconds` | `3600` | 单次延迟上限（防指数爆炸） |
| `notify.deadLetterAfterSeconds` | `86400` | 超过这个时长仍未成功 → 死信 |

退避序列（默认）：**30s → 60s → 120s → 240s**（第 5 次失败 → 死信）。

⚠️ **用 `time` crate 算，不用浮点**（低配机 + 确定性断言）。

---

## D298: 死信 = **状态机的一个终态**，人工重投 = **状态回退 + 重置计数**

```
pending ──tick──▶ delivering ──成功──▶ delivered
                      │
                      ├──失败且还有次数──▶ pending（next_attempt_at = now + backoff）
                      └──失败且次数用尽──▶ dead_letter
dead_letter ──人工重投──▶ pending（attempt_count = 0）
```

### 「不重复投递已成功的」（AC-S-023 的原文要求）

**重投只针对死信的条目**，且：
- 重投**只重置 `dead_letter` 那一条**（按 `delivery_id` 精确定位）；
- `delivered` 的行**状态不可回退**（SQL 层用 `WHERE status = 'dead_letter'` 守卫，
  影响行数为 0 就返回 `409 NOTIFY_NOT_DEAD_LETTER`）；
- → **天然不会重复投递已成功的**。

⚠️ **这条必须用「影响行数」断言**，不是「查一下状态对不对」。

---

## D299: 投递记录 = `platform_notify_delivery`（租户级 + RLS）

| 列 | 说明 |
|---|---|
| `id` / `tenant_id` | |
| `template_code` / `channel` / `category` | 来源 |
| `recipient` | 接收人（站内信是 user_id；webhook 是 URL） |
| `locale` | 渲染用的语言 |
| `payload` | `jsonb`（渲染变量） |
| `rendered_title` / `rendered_body` | 渲染结果（**快照**，模板改了不追溯） |
| `status` | `pending` / `delivering` / `delivered` / `dead_letter` |
| `attempt_count` | |
| `next_attempt_at` | |
| `last_error` | |
| `provider_message_id` | |
| `created_at` / `updated_at` / `delivered_at` | |

- **`rendered_*` 是快照** → 模板后续修改**不影响已投递的记录**（可追溯）；
- ⚠️ **`payload` 可能含敏感值** → 落库前过 `scan_sensitive_keys`（**复用 REQ-014 的**）
  → 命中则拒（`NOTIFY_SENSITIVE_PAYLOAD`）。

---

## D300: 权限点 **3 个**

| 权限点 | 用途 |
|---|---|
| `notify:template:read` | 查看模板 |
| `notify:template:write` | 维护模板 |
| `notify:delivery:read` | 查看投递记录 + 重投死信 |

- **订阅偏好不进权限点**：那是**用户自己的**设置（登录即可改），与 REQ-016 的 D271 同款口径。
- **C-5**：3 个权限点各要有字面量 `require_permission("<字面量>")` 调用点，在 `crates/platform-notify/src/` 内。

---

## D301: AC 编号 **AC-S-166 ~ AC-S-174**（+ 既有 AC-S-023 重写）

实测：全仓最大 AC-S 编号 = **165**（REQ-016）；`166`~`174` 未被占用。

| AC | 主题 |
|---|---|
| **AC-S-023** | （既有，**重写**）死信可重投且**不重复投递已成功的** |
| **AC-S-166** | 通道抽象：`NotificationSender` 注册表 + **2 个真实现** + 3 个显式「未配置」 |
| **AC-S-167** | 模板 CRUD + **受限变量替换** + 未知变量显式失败 |
| **AC-S-168** | 模板**多语言**按 locale 分行 + 三级回退 |
| **AC-S-169** | 订阅偏好：按 category 退订 + **`system` 类不可退订** |
| **AC-S-170** | 异步投递：`FOR UPDATE SKIP LOCKED` + **多 worker 不重复不遗漏** |
| **AC-S-171** | 重试退避：指数 + 上限 + 次数用尽进死信 |
| **AC-S-172** | 死信人工重投：**只重置死信条目** + 影响行数守卫 |
| **AC-S-173** | 投递记录：快照 + 租户隔离 + **敏感键拒收** |
| **AC-S-174** | 权限点三处齐 + 启动对账 + C-5 + 审计埋点 |

**共 10 条**（9 新 + 1 既有重写）。

---

## D302: 新 crate `platform-notify` + 2 个 provider 子模块

```
crates/platform-notify/
  src/
    lib.rs
    error.rs      // 8 个错误码
    events.rs     // 审计事件码
    model.rs      // 表行 + DTO
    template.rs   // 受限变量替换（**核心，可单测**）
    store.rs      // 数据访问
    service.rs    // 编排（入队 / 渲染 / 偏好检查）
    worker.rs     // tick() 批处理
    channels/
      mod.rs      // ChannelRegistry
      in_app.rs   // 站内信（真实现）
      webhook.rs  // Webhook（真实现，复用 hyper）
    http.rs       // 平台端点
    authz.rs      // 3 个 require_permission
    config.rs     // 配置源
```

依赖：`platform-core` + `platform-db` + `platform-config`（读 locale 默认值）。
**不依赖** `platform-rbac`（用 trait 注入）。

⚠️ **Webhook 的出站**：`platform-oauth/src/outbound.rs` 有 `OutboundClient`，
但它是 `pub struct` 且 `pub mod outbound` —— **可以复用**（`platform-notify` 依赖 `platform-oauth` 也可，
但那个方向会让「通知」依赖「OAuth 服务器」，**语义不对**）。
→ **本轮自己写一个薄的出站**（与 D220 同款理由：出站 client 独立），
代码量约 60 行（`hyper` + 超时）。

---

## D303: 8 个错误码（每个都必须有 AC 会触发）

| 码 | HTTP | 何时 | 触发它的 AC |
|---|---|---|---|
| `NOTIFY_TEMPLATE_NOT_FOUND` | 404 | 模板不存在 | 167 |
| `NOTIFY_TEMPLATE_DUPLICATED` | 409 | 同 (code, channel, locale) 重复 | 167 |
| `NOTIFY_TEMPLATE_MISSING_VARIABLE` | 400 | 渲染时变量缺失 | 167 |
| `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER` | 400 | `{{...}}` 里是非法变量名 | 167 |
| `NOTIFY_CHANNEL_NOT_CONFIGURED` | 400 | 通道未配置 | 166 |
| `NOTIFY_CATEGORY_MANDATORY` | 400 | 试图退订 `system` 类 | 169 |
| `NOTIFY_NOT_DEAD_LETTER` | 409 | 重投一个非死信的记录 | 172 |
| `NOTIFY_SENSITIVE_PAYLOAD` | 400 | `payload` 含敏感键 | 173 |

⚠️ **`TITLES` 行数**：**99 → 107**（58+21+5+6+9 的当前 99 + **8**）。
⚠️ 事件码（**4 个** `notify.*`）**不进 `TITLES`**（D223 同款）。

> 实测复核（BA，2026-11-21）：`crates/platform-core/src/titles.rs:45` 的 `TITLES`
> 解析出 **99** 条、**0** 个重复 code（python 正则解析，非肉眼数）。


---

## D304: 4 个配置项

| 键 | 类型 | 默认 | scope |
|---|---|---|---|
| `notify.maxAttempts` | `number` | `5` | platform |
| `notify.retryBaseSeconds` | `number` | `30` | platform |
| `notify.retryMultiplier` | `number` | `2` | platform |
| `notify.retryMaxSeconds` | `number` | `3600` | platform |

⚠️ **每个键必须带 `"scope": "platform"`**（REQ-016 的 S-01 教训：漏写会落到 `Tenant` → 平台端点直接拒 → 配置永不生效）。
⚠️ `config` 段必须是 `ConfigSpec` 对象，**不是裸值**（REQ-016 的 S-02 教训：裸值 → 服务起不来）。

---

## D305: SMTP 邮件通道 —— **自己写，不引 SMTP 库**（用户裁定）

### 要实现的协议范围（**只做最小可用集**）

不做「完整 SMTP 客户端」，只做**发一封纯文本/HTML 邮件的必要子集**：

```
① TCP 连接          →  220
② EHLO <helo>       →  250-（读多行，遇 "250 " 空格结尾才停）
③ STARTTLS          →  220   →  TLS 握手（rustls）
④ EHLO <helo>       →  250-（**TLS 之后再发一次**，因为能力集变了）
⑤ AUTH LOGIN        →  334 → base64(user) → 334 → base64(pass) → 235
⑥ MAIL FROM:<...>   →  250
⑦ RCPT TO:<...>     →  250（可能多个；**任何一个被拒 → 整条失败**）
⑧ DATA              →  354 → 正文 + "." 行 → 250
⑨ QUIT              →  221
```

**明确不做**（写进规格，防止后续留悬念）：

| 不做 | 为什么 |
|---|---|
| ❌ `AUTH PLAIN` / `CRAM-MD5` / `XOAUTH2` | 三种机制**同一位置**，先只做 `LOGIN`；覆盖不到时显式报错而非静默降级 |
| ❌ 隐式 TLS（端口 465，连上即握手） | 只做 **STARTTLS**（587）；465 记 TD-055 |
| ❌ 多部分 MIME（附件 / 内嵌图片） | 本轮通知正文只有 title + body；附件见 REQ-018 |
| ❌ DKIM 签名 / SPF 配置 | 属于**域名运维**，不是应用代码 |
| ❌ 连接池 / 长连接复用 | 通知量级下每封新建连接足够；连接池引入状态管理复杂度 |

### 关键技术选择

1. **明文行协议 + `tokio::net::TcpStream`**：`tokio` 的 `net` 特性已在 workspace 开启
   （`Cargo.toml:46`）；`AsyncReadExt` / `AsyncWriteExt` 在同仓 `crates/platform-server/src/probe.rs:6`
   已在用（**这是「零新增依赖」的实证，不是推测**）。
2. **TLS 用 `tokio-rustls`**（见 D307 的依赖提升理由）；
   根证书用 `webpki_roots::TLS_SERVER_ROOTS`。
3. ⚠️ **`rustls` 的 `ServerName` 必须来自配置的 host**，且**不得**为 `IP` 型
   （rustls 拒绝 IP 型 ServerName 做 SNI，除非显式构造 `IpAddress` 变体）→ 配置校验阶段就拦。
4. **凭据走环境变量，不进配置表**（见 D306）。
5. **超时**：整体 `tokio::time::timeout` 包住，**10 秒**（与 `OutboundClient` 的 5 秒不同：
   SMTP 要 9 轮往返 + TLS 握手，5 秒偏紧）。
6. **正文编码**：`Content-Type: text/plain; charset=UTF-8` +
   `Content-Transfer-Encoding: base64` → **base64 已在 workspace**（`Cargo.toml:90`）。
   ⚠️ base64 后必须**每 76 字符换行**（RFC 2045）；不换行会被严格 MTA 拒。
7. **头注入防御**：`To` / `From` / `Subject` 里的 `\r` / `\n` **必须剥除或拒绝**
   —— 否则 `Subject: x\r\nBcc: attacker@evil` 可以注入额外收件人。**这是一条安全判据**。

### 怎么测（**这决定实现是否有区分度**）

用 `tokio::net::TcpListener::bind("127.0.0.1:0")` 起**假 SMTP 服务器**（同仓先例：
`crates/platform-iam/tests/sso_support/mod.rs:474`），逐行脚本化应答：

| 用例 | 假服务器行为 | 断言 |
|---|---|---|
| 成功 | 全程 2xx/3xx | 收到 `DATA` 正文；`delivered == true`；`provider_message_id` 非空（取 250 响应里的队列 ID） |
| 认证失败 | `AUTH LOGIN` 回 535 | `delivered == false`；`error` **含 `535`** |
| `RCPT` 被拒 | 第 2 个 `RCPT TO` 回 550 | 整条失败；`error` 含 `550` |
| STARTTLS 后能力变化 | **仅 TLS 之后**才宣告 `AUTH` | 证明**真的重发了 `EHLO`**（TLS 前发 `AUTH` → 该用例失败） |
| 超时 | 连上但不发 `220` | 10 秒后失败；`error` 含超时 |
| 多行 250 | `250-...` 多行后 `250 ` 收尾 | 能正确读完（**只读一行就发下一句 → 该用例失败**） |

> ⚠️ **阳性对照要求**：上表每一条都必须能**独立变红**。
> 「STARTTLS 后重发 EHLO」那条尤其关键 —— 它是 SMTP 实现里最常被漏掉的一步
> （TLS 前 `EHLO` 看不到 `AUTH`，不重发就永远认证不了）。

---

## D306: SMTP 凭据 = **环境变量**，**不进配置表**

### 问题

`ConfigSpec`（`crates/platform-core/src/extension/manifest.rs:189-207`）的 `ConfigType`
只有 `String` / `Number` / `Boolean` / `Array` / `Object`，**没有 `Secret`**；
`crates/platform-config/src/` 里也**没有任何加密/脱敏机制**（`grep -c "secret\|encrypt"` = 0）。

→ **SMTP 密码若进配置表，就是明文落库 + 通过配置读接口回传给任何持
`config:read` 的人**。这是**不可接受**的。

### 决定

`SmtpConfig` 走**环境变量**，与既有 SSO / SCIM 同款（`crates/platform-org/src/config.rs:37`
的 `SCIM_BASE_URL_ENV`；`crates/platform-iam/src/sso/config.rs` 的
`LDAP_SERVICE_BIND_PASSWORD_ENV`，装配点见 `crates/platform-server/src/main.rs:621-627`）：

| 环境变量 | 必填 | 说明 |
|---|---|---|
| `BASE_SMTP_HOST` | 是 | 主机名（**不接受 IP**，见 D305 第 3 条） |
| `BASE_SMTP_PORT` | 是 | 通常 `587` |
| `BASE_SMTP_USERNAME` | 是 | |
| `BASE_SMTP_PASSWORD` | 是 | **敏感值，永不进日志**（复用 AC-S-106 的敏感值扫描判据） |
| `BASE_SMTP_FROM` | 是 | 发件人地址 |

- **`is_configured()` 判定**：5 个变量**全有**才算配置好；缺任一 →
  该通道注册为「未配置」→ 投递返回 `NOTIFY_CHANNEL_NOT_CONFIGURED`（**不是静默成功**，D292）；
- ⚠️ **密码不得出现在任何 `Debug` / `tracing` 输出里** —— `SmtpConfig` 手写 `Debug`，
  密码字段固定输出 `"***"`（同 `OutboundClient` 的手写 `Debug` 手法，`outbound.rs:69`）。

### 配置项（manifest）里放什么

manifest 的 `config` 段**只放非敏感项**（D304 那 4 个退避参数）；
SMTP 的连接信息**一个都不进** manifest。

---

## D307: 依赖提升 —— `tokio-rustls` 与 `webpki-roots` **转正式依赖**（**不新增 lock 条目**）

### 实测事实（`develop` @ `a2b7e52`）

| crate | 现在在哪 | 版本 |
|---|---|---|
| `tokio-rustls` | `crates/platform-iam/Cargo.toml:70`，**`[dev-dependencies]`** 里（注释写「仅测试用」） | `0.26.6` |
| `webpki-roots` | **只在 `Cargo.lock`**，由 `hyper-rustls` 的 `webpki-tokio` 特性传递引入 | `1.0.12` |

### 决定

1. `tokio-rustls` 在 workspace `Cargo.toml:139` **已有条目**，`platform-notify`
   把它写进 **`[dependencies]`** —— 这是**从 dev 升为正式依赖**，不是新增依赖；
2. `webpki-roots` **需要在 workspace `Cargo.toml` 新增一行**
   （现在只在 lock 里被传递引入，没有 workspace 条目）。
   ⚠️ **但这不新增 `Cargo.lock` 条目** —— 包已在 lock 中，只是「从传递依赖变直接依赖」。
3. ⚠️ **`platform-iam/Cargo.toml:70` 的注释「SSO 测试：mock IdP 的**服务端** TLS」仍然准确**
   —— 那里它确实只用于测试服务端。**不改那条注释**（它在那个 crate 里是对的）。
   新增的是 `platform-notify` 里的**客户端**用法。
4. 若 `cargo` 报「同一个 crate 被同一 package 同时以 dev 与正常依赖引入，需 `features` 一致」
   → 把 `platform-iam` 的那条也挪到 `[dependencies]` 或统一特性。
   **预计不会**（`platform-notify` 与 `platform-iam` 是两个不同 package）。

### 这条要不要算「破例」

**不算**。D123 的破例是**为 TLS 引入新 crate**；本轮**没有新 crate 进 lock**，
只是把已有 crate 的引用方式从「传递/dev」改成「直接」。已在 D292 rev 2 里说明。

---

## 未决 / 待用户复核

| # | 事项 | 结论 |
|---|---|---|
| 1 | **五类通道做 3 个**（站内信 / Webhook / SMTP），其余 2 类（短信 / IM）显式未配置 | ✅ **用户 2026-11-21 已裁定**「做真 SMTP 客户端」→ TD-054 收窄为 2 类 |
| 2 | **模板只做变量替换**，不做条件/循环 | BA 提议（避免 SSTI + 零新增依赖） |
| 3 | **worker 是显式 CLI 命令**，不是 web 进程内后台 | BA 提议（进程重启不丢消息 + 多实例不重复 + 测试可控） |
| 4 | **多语言按 locale 分行存**，不复用 i18n 词库 | BA 提议（通知正文是「管理员维护的内容」，不是「界面翻译」） |
| 5 | **SMTP 只做 STARTTLS（587），不做隐式 TLS（465）** | 需真实凭据才能验 465；记 TD-055 |
