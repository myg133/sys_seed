# 决策记录：可观测性与 API 治理（REQ-019）

- **需求编号**: REQ-019
- **父需求**: REQ-001
- **状态**: rev 1（2026-11-22 —— 四件套：1 条空泛 AC → **9 条**）
- **基线代码**: `develop` @ `0b9697b`（REQ-018 已合并）
- **依赖**: REQ-002 ✅

---

## 0. 需求来源披露（D122 / dev-guide §9.1）

**REQ-019 的需求来源是 BA 补的，不是用户原话。**

用户 2026-11-17 的 9 项需求清单里**没有「可观测性与 API 治理」**。
`roadmap.md:79` 把它标为「⚠️ **BA 补**（用户已裁定继续）」——
用户此后裁定「出了工作流，其他的继续做」→ 本需求继续。

父需求 `REQ-001` 的 **AC-027 / AC-028** 是它的来源。
→ 本需求的具体形态（9 条 AC / 三级限流 / OTel / OpenAPI / 弃用头）
全部是 BA 的设计，每条都要有决策依据。

---

## D337: 三级限流 = **PG 滑动窗口**（用户裁定），零新依赖

### 问题

`demand.md` 的基线表第 5 项：三级限流**完全没有**。
树里只有一个 IAM 登录的**单点**防护（`platform-iam/src/service.rs:837`
`check_ip_rate_limit`，**无租户维度**），不是通用能力。

### 实测发现

| 需要的东西 | 树里有吗 |
|---|---|
| Redis / 任何外部计数器 | ❌ 没有（`podman ps` 只有 postgres/redis **容器**，但应用**不连**它） |
| 通用限流中间件 | ❌ 没有 |
| 429 响应 | ❌ 没有（`grep 429` 只命中 sha256 哈希里的数字巧合） |
| 租户维度计数 | ❌ 没有 |

### 决定（用户裁定：方案「PG 滑动窗口（存时间戳）」）

**滑动窗口**：每个请求写一行 `(tenant_id, dimension, key, occurred_at)`，
判定时数「最近 `windowSeconds` 秒内有多少行」。

```sql
-- 判定 + 记账（两步，不是一步）
SELECT count(*) FROM platform_rate_limit
 WHERE tenant_id = {tenant} AND dimension = $1 AND key = $2
   AND occurred_at > now() - ($3 || ' seconds')::interval;

INSERT INTO platform_rate_limit (tenant_id, dimension, key, occurred_at)
VALUES ({tenant}, $1, $2, now());
```

### ⚠️ 为什么不用固定窗口（虽然它更省行）

固定窗口（`window_start` 对齐 + 计数列）在**窗口边界**会有**双倍突发**：
窗口 A 的最后 1 毫秒打满配额、窗口 B 的第 1 毫秒又打满 → 实际瞬时速率是配额的 **2 倍**。
滑动窗口没有这个问题，代价是行数多（每条请求一行）。

**用户裁定选滑动窗口**，接受行数代价。

### ⚠️ 行数增长的处置（必须实现，不能留给以后）

每请求一行 → 一天 100 万请求就是 100 万行。**必须有清理**：
- 后台任务按 `occurred_at < now() - (最大窗口 × 2)` 删除（`xtask` 子命令或复用现有 worker 形态）；
- 清理**不是**可选项：没有它，表会在几周内变成性能瓶颈。
- 清理任务要记进 `design-summary.md` 的「定期任务」表；**不实现就等于埋雷**。

### ⚠️ 三个维度必须用独立的键前缀

否则会串桶（ABAC 上同一个 key 同时被租户维度和用户维度消费）。

| 维度 | 键形态 | 例 |
|---|---|---|
| `tenant` | `rl:tenant:<tenant_id>` | `rl:tenant:tenant-a` |
| `user` | `rl:user:<tenant_id>:<user_id>` | `rl:user:tenant-a:user-a` |
| `endpoint` | `rl:endpoint:<METHOD>:<路径模板>` | `rl:endpoint:GET:/api/rbac/roles` |

### ⚠️ 三个桶的实际作用域（**这是挑刺 B1 的根源，必须写清**）

`platform_rate_limit` 是 **RLS 租户表**，`tenant_id` 是**列**。
查询恒带 `WHERE tenant_id = {tenant}` → **三个维度的桶都是「租户内」的**：

| 维度 | 作用域 | 谁共享这个桶 |
|---|---|---|
| `tenant` | 租户内**所有**请求 | 该租户的所有用户 |
| `user` | 租户内**某个用户** | 只有该用户 |
| `endpoint` | 租户内**某个端点** | **该租户的所有用户**（⚠️ 键里没有 user 段） |

⚠️ **因此「同一租户内 user-b 不受 user-a 的 endpoint 限流影响」是错的** ——
endpoint 桶本就按租户维度共享，`user-a` 打满后 `user-b` 打同一端点**必然** 429。

rev 1 的 AC-S-031 步骤 4 断言「`user-b` 打同一端点仍 200」，
**在实现正确时会红、在实现错误（键带 user_id）时才绿** —— 这是最坏的一种判据缺陷：
它主动奖励错误实现。rev 2 已改为**跨租户**验证（`tenant-b` 的桶与 `tenant-a` 完全独立）。

⚠️ **endpoint 用路径模板，不是完整 URL** —— 否则 `?x=1` 与 `?x=2` 会算两个桶，
攻击者加个随机 query 就能无限打（**AC-S-031 专门测这个**）。

### 代价（明码记账）

- **每请求一次 SELECT + 一次 INSERT** → 对 PG 有额外压力。
  高并发下 `count(*)` 会变慢；本合同轮**不加索引之外的优化**
  （索引：`(tenant_id, dimension, key, occurred_at)`），真到瓶颈再考虑分片或换 Redis。
- 需要**新表 + 清理任务**，比内存方案重。
- 但它**多实例准确** —— 这是 AC-S-033 专门验的判据。

---

## D338: OpenAPI = **引入 `utoipa`**（第三次依赖破例，用户裁定）

### 实测发现（`cargo generate-lockfile` 实测，不是查文档）

在 `develop` @ `0b9697b` 的 `Cargo.lock`（**329 包**）基础上加 `utoipa = "5"`：

| 新增包 | 版本 | 由谁引入 |
|---|---|---|
| `utoipa` | 5.5.0 | 直接依赖 |
| `utoipa-gen` | 5.5.0 | `utoipa` |

→ **净增 2 个包**，删除 0 个。**这是三次破例里最便宜的一次**
（D123 的 TLS 栈、D321 的 multipart 是 5 个）。

### 决定

**引入 `utoipa` v5**，用 derive 宏从路由与 DTO 生成 `openapi.json`。

### 为什么不用「手写生成器」或「手写 YAML」

| 方案 | 代价 | 问题 |
|---|---|---|
| **utoipa**（选它） | 净增 2 包 | 无 —— 2 个包换「不会漂移」很划算 |
| 手写生成器（xtask 从路由表生成） | 零依赖 | 路由元信息要**手工维护**（每个端点的参数/响应/状态码），
实际就是手写 YAML 换个位置，**照样会漂** |
| 手写静态 YAML | 零依赖 | **一定会漂** —— 没人会记得改完代码同步 YAML |

### ⚠️ 破例的边界（必须写清）

破例范围是 **`utoipa` 本体 + `utoipa-gen`**，**不**包含：
- `utoipa-swagger-ui`（在线 UI 不需要，产物是静态 JSON）
- `utoipa-redoc` / `utoipa-scalar`（同上）
- 任何 `openapi-*` 系列的其它包

若后续要加上述任何一个，需**重新走裁决**（不是自动放行）。

---

## D339: OpenTelemetry = **真接 OTLP（HTTP/protobuf）**（第四次依赖破例，用户裁定）

### 实测发现（`cargo generate-lockfile` 实测三种配置）

| 配置 | 净增包数 | 含什么 |
|---|---|---|
| 默认（`opentelemetry-otlp` 全 features） | **24** | 含 `tonic`（gRPC）+ `reqwest`（HTTP）+ `prost` + `regex` + `web-sys` … |
| `default-features = false, features = ["http-proto"]` | **18** | 仍含 `tonic`（`opentelemetry-proto` 默认拉 gRPC） |
| 再加 `with-serde` | **27** | 额外拉 `proptest`（测试框架，生产不需要） |

→ **最省 18 个**。用户裁定：**接受 18~27 的代价，真接**。

### 决定

引入 `opentelemetry` / `opentelemetry_sdk` / `opentelemetry-otlp` /
`tracing-opentelemetry`，走 **OTLP HTTP/protobuf**。

### ⚠️ 必须选对 features（否则会多拉 6~9 个包）

```toml
opentelemetry-otlp = { version = "0.31", default-features = false, features = ["http-proto"] }
```

- **不**要 gRPC（`grpc-tonic`）：我们只需要导出，HTTP 够用，gRPC 会多拉 `tonic` + `tonic-prost`；
- **不**要 `with-serde`：那会拉 `proptest`（测试专用）；
- 如果最终仍无法避免 `tonic`（`opentelemetry-proto` 的默认 feature），
  **在实现时实测并如实记录**，不要照抄规格里的「18」——
  **数字必须以实现时的实测为准**（D275）。

### ⚠️ 导出失败**不能**影响业务（AC-S-037 步骤 2 专门验这个）

OTel 接入最常见的坑：collector 不可达时 SDK 内部重试阻塞 →
请求变慢甚至超时。必须：
- 用**批量导出器**（`BatchSpanProcessor`）+ 有界队列，满了**丢弃**并打 warn；
- 明确配置超时（`OTEL_EXPORTER_OTLP_TIMEOUT`，默认给 3 秒，**不是**无限等）；
- 导出失败**只打日志**，不 `panic`、不返回错误给调用方。

### ⚠️ 明码不做

**真导出到 collector 无法验证**（本机无 collector）→ **TD-069**。
AC-S-037 只验「配置生效 / span 结构 / 失败不影响业务 / 可关闭」四件可验的事，
**不声称**验过真导出。

---

## D340: `trace_id` 的**来源与校验** = 入站沿用 + 严格格式校验

### 决定

1. **来源优先级**：入站头 `X-Request-Id` → 没有/非法则**生成**（32 位小写 hex，16 字节随机）；
2. **格式校验**（AC-S-030 的四条路径）：
   - 必须**恰好 32 个字符**；
   - 必须全是 hex（`0-9a-f`，**大写归一为小写**）；
   - **不能**含任何非 hex 字符（`/`、`.`、空格、控制字符一律拒绝）。
3. **回写**：响应头 `X-Request-Id` **必带**（成功与失败都带）；
4. **注入日志**：`tracing` 的 span 里带 `trace_id` 字段，每行日志自动继承。

### ⚠️ 校验的具体写法（先 ASCII 再长度，避免误读）

```rust
fn normalize_trace_id(raw: &str) -> Option<String> {
    // ① 先判 ASCII hex —— 这一步就排除了所有多字节 UTF-8
    if !raw.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    // ② 再判长度：此时字节数 == 字符数，所以 len() 是安全的
    if raw.len() != 32 { return None; }
    Some(raw.to_ascii_lowercase())
}
```

⚠️ **顺序很关键**：先判 ASCII 再判长度，`len()` 的语义就不再有歧义
（能过第 ① 步的串必然全是单字节 ASCII）。
rev 1 的写法（先 `len()` 再 `chars().all()`）**功能上也正确**，但注释里同时写着
「不能用 `len()` 判字节」，读起来自相矛盾（挑刺 m3）。

### ⚠️ 为什么必须校验（AC-S-030 步骤 4）

不校验就会形成**日志注入**：
`X-Request-Id: ../../etc/passwd` 或含换行的值会被原样写进日志/审计表 →
运维按 trace_id 搜会搜到奇怪的东西，严重的能**伪造日志行**（如果值里有 `\n`）。

这是**安全判据**，不是洁癖。

### ⚠️ 与 OTel 的关系

OTel 的 `trace_id` 是 16 字节（32 hex），与我们的格式**天然一致** ——
所以可以直接用入站的 `X-Request-Id` 作为 OTel trace_id
（AC-S-037 步骤 1 断言两者**逐字相等**）。

---

## D341: 分页契约 = **cursor 为准，offset 不提供**

### 决定

统一响应信封：

```json
{ "items": [...], "nextCursor": "..." | null, "total": 25 }
```

请求参数：`limit`（1~100，默认 20）、`cursor`（不透明字符串）。

### ⚠️ 与契约 C7 的关系（挑刺 M12）

`docs/contracts/api-conventions/v1.md:118-119` 的 C7 明写：
「响应含 `next_cursor`、上限 200，**兼容分页 `?page=N&page_size=M`，二者互斥**」。

本需求：
- **保留** `?page=&page_size=`（不破坏已验收行为）；
- 把「超上限」从**夹取**改为 **400**（破坏性，见上表）；
- **新增**端点只用 cursor，不提供 `page`（C7 说的是「兼容」，不是「必须」）。

→ 需要在实现时**同步更新 C7 文档**，把「夹到上限」改成「超上限 400」。

### ⚠️ 为什么不提供 `offset`

`offset` 在大表上**越翻越慢**（`OFFSET 100000` 要扫 10 万行），
且**并发写入时会漏行/重行**（翻页期间插入一条，第 2 页会重复第一条）。
cursor（keyset）没有这两个问题。

### ⚠️ `limit` 越界必须 400，不能静默截断 —— **这是对 D153 / C7 的破坏性修正**

`limit=1000` 静默截到上限 → 调用方以为自己拿到了 1000 条 → **数据静默丢失**。
这是 AC-S-034 步骤 4 的判据。

⚠️ **必须明码声明破坏面**（挑刺 B4 / M12）：

| 项 | 现状（D153 + 契约 C7） | 本需求改为 | 破坏性 |
|---|---|---|---|
| `limit` 上限 | 200（`query.rs:53`） | **200（不变）** | 否 |
| 超上限行为 | **夹到 200 返回 200**（`query.rs:178`） | **400 + `INVALID_REQUEST`** | **是** |
| `limit <= 0` | `AUDIT_INVALID_FILTER` | **400 + `INVALID_REQUEST`** | **是** |
| `?page=&page_size=` | 支持 | **保留** | 否 |

**要改的既有测试**（实现时必须在提交信息里点名）：
`crates/platform-audit/src/query.rs:646-652`
`an_oversized_limit_is_clamped_instead_of_rejected` → 改为 `an_oversized_limit_is_rejected`。

⚠️ 上限**保持 200 不改 100** 是为了减少破坏面 —— C7 的 200 继续有效。

### ⚠️ 本轮只做**样板**，不改造全部 crate

改所有列表端点属**跨需求改动**（D109），会影响已完成的 19 个子需求的响应契约。
本轮只把 `platform-audit` 的现有 cursor 实现对齐契约 + 新增端点用新契约，
其余按需迁移 → **TD-070**。

---

## D342: 弃用头 = **`Deprecation` 用 `true`，`Sunset` 用 IMF-fixdate**

### 决定

| 头 | 取值 | 规范 |
|---|---|---|
| `Deprecation` | `true` | RFC 9745（2025）。⚠️ 该 RFC 允许 `true` 或日期；**本实现统一用 `true`**，理由是「弃用是布尔状态，什么时候宣布弃用对客户端无操作价值」 |
| `Sunset` | IMF-fixdate | RFC 8594。形如 `Sun, 31 Dec 2026 23:59:59 GMT` |

### ⚠️ 为什么 `Sunset` 必须严格 IMF-fixdate

AC-S-036 步骤 3 专门验格式。用 ISO8601（`2026-12-31T23:59:59Z`）或 Unix 时间戳
都是**不符合 RFC** 的 —— 客户端按 RFC 解析会失败，等于没发。

### ⚠️ 弃用 ≠ 下线

被弃用端点**仍然返回 200**（AC-S-036 明确判据）。
立刻返回 410 是**行为破坏**，客户端来不及迁移。

---

## D343: 新增 crate `platform-observability`（限流 + trace + OTel 装配）

### 决定

新建 `crates/platform-observability`：
- `rate_limit.rs`：PG 滑动窗口 + 三级维度 + 清理任务；
- `trace.rs`：trace_id 生成/校验 + `tracing` 层；
- `otel.rs`：OTel SDK 装配（可关闭）；
- `manifest.json`：权限点（若有）+ 配置项。

### ⚠️ 为什么单独一个 crate 而不是塞进 `platform-core`

`platform-core` 是**所有 crate 的依赖**（轻量、无 IO）。
限流要 PG、OTel 要 exporter —— 塞进 core 会让**每个 crate 都编译 OTel**，
编译时间与二进制体积都会涨。单独 crate 只有 `platform-server` 依赖它。

**遵循 D150**（业务模块只认 core 的 trait，具体实现在独立 crate）。

---

## D344: 限流配置 = **3 个维度 × 2 项（requests + windowSeconds）+ 1 个全局开关**

### 配置键（全部 `scope: platform`）

| 键 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `rateLimit.enabled` | `boolean` | `false` | **默认关闭** |
| `rateLimit.tenant.requests` | `number` | `0` | 0 = 该维度禁用 |
| `rateLimit.tenant.windowSeconds` | `number` | `60` | |
| `rateLimit.user.requests` | `number` | `0` | |
| `rateLimit.user.windowSeconds` | `number` | `60` | |
| `rateLimit.endpoint.requests` | `number` | `0` | |
| `rateLimit.endpoint.windowSeconds` | `number` | `60` | |

### ⚠️ 为什么默认**关闭**

打开限流会改变**所有**现有端点的行为（可能让已有的集成测试变成 429）。
**默认关闭 + 显式开启**让「升级底座」不会突然打断业务 ——
这是 `REQ-020`（底座可用性验证）要求的「平台升级业务零改动」的前提。

### ⚠️ `requests = 0` 表示**禁用该维度**，不是「允许 0 次」

两者语义完全不同：前者不限流，后者**拒绝一切请求**。
判据（AC-S-031）专门测：`user.requests = 0` 时用户维度**不再**限流。

### ✅ 为什么默认关闭能保证不打断现有测试（**已实测确认**）

现有 1496 个测试的数据库里**没有** `rateLimit.*` 这些配置键（它们是新迁移才加的）。
那读不到配置会怎样？**实测** `platform-storage` 的同类读法（`config.rs:126-146`）：

```rust
max_upload_bytes: self.read(ctx, CONFIG_KEY_MAX_UPLOAD_BYTES).await
    .unwrap_or(d.max_upload_bytes),   // ← 读不到就降级到代码默认值，不报错
```

即 **`read()` 失败 → 用 `StorageSettings::default()`**，**不会** panic、不会拒绝请求。

→ 本需求的限流配置也必须**照这个模式**实现：
`enabled` 读不到 → `false` → 中间件直接放行（零查库开销）。
这样现有 19 个子需求的 1496 个测试**一个都不会因为限流变红**。

⚠️ **如果实现时改成「读不到配置就报错」**，那 1496 个测试会集体变红 ——
这属于**实现阶段最容易踩的坑**，写在这里防止忘记。

---

## 未决 / 待用户复核

| # | 事项 | 结论 |
|---|---|---|
| 1 | 限流存储 = **PG 滑动窗口**（存时间戳） | ✅ 用户 2026-11-22 裁定 → D337 |
| 2 | OpenAPI = **引入 `utoipa`**（第三次破例，净增 2 包） | ✅ 用户 2026-11-22 裁定 → D338 |
| 3 | OTel = **真接 OTLP**（第四次破例，净增 18~27） | ✅ 用户 2026-11-22 裁定 → D339 |
| 4 | 真导出到 collector **无法验证** | BA 记账 → TD-069 |
| 5 | 分页只做样板，不改造全部 crate | BA 提议（D341）→ TD-070 |
| 6 | 限流默认**关闭** | BA 提议（D344） |
