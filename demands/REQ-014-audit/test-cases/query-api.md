# 统一埋点入口与审计查询接口（AC-S-129、AC-S-130）

- **覆盖 AC**: AC-S-129（7 条：TC-014-069 ~ 075）、AC-S-130（15 条：TC-014-076 ~ 090）
- **前置基线**:
  - 测试库 `base_test`；租户 `t-audit`（T-A）
  - T-A 下写入 **5 条**审计记录，它们在 `actor_id` / `action` / `object_type` / `created_at` 上**互不相同**
  - 一个具备 `audit:events:read` 的令牌（`u-auditor`）；一个**无**该权限的令牌（`u-noperm`）
- **实现事实（用例按此写）**:
  - 端点（design §5）：`GET /api/audit/events`、`GET /api/audit/events/{id}`、
    `GET /api/audit/verify`、`POST /api/audit/archive`
  - 权限点（design §9）：`audit:events:read` / `audit:verify:run` / `audit:archive:run`
  - 查询参数（design §5.1）：`page`（默认 1）/ `size`（默认 50，**上限 200**）/
    `actorId` / `action` / `objectType` / `objectId` / `from` / `to`
  - 响应形状（design §5.2）：`{ items, total, page, size }`
  - ⚠️ **契约 C7（`BA/contracts/api-conventions/v1.md:114-124`）与 design §5.1 口径不一致**：
    C7 要求**游标分页**（`cursor` + `limit`，响应含 `next_cursor`，默认 `limit=20`），
    兼容页码用 `?page=N&page_size=M`（**不是** `size`），
    过滤用 `?filter[field][op]=value`，排序用 `?sort=-created_at,name`。
    本文件按 **design §5.1** 写（AC 也是这么写的），**并在 §备注里标出与 C7 的 5 处冲突**。〔S-02〕
  - ⚠️ 仓里**没有任何**既有分页端点可参照（`grep 'page|size|limit|cursor' crates/**/*.rs` 无分页实现），
    现有查询参数全是 `Option<String>` 过滤器（如 `crates/platform-org/src/http.rs:308`），
    唯一的强类型查询参数是 `crates/platform-server/src/i18n.rs:905-914`（`minor: i64`、`as_of: String`）
  - ⚠️ axum 0.8 的 `Query` 反序列化失败返回 **400 + 纯文本 body**
    （`axum-0.8.9/src/extract/rejection.rs:84-90`：`#[body = "Failed to deserialize query string"]`），
    **不是** RFC 9457 ProblemDetails → 与契约 C6 冲突。〔S-09〕

---

## AC-S-129：统一埋点入口（业务模块不需自建审计表）

### TC-014-069: 平台的**通用审计事件类型 + sink trait** 能记录一条事件

- **关联 AC**: AC-S-129
- **前置条件**: T-A 下无 `action='demo.generic'` 的记录
- **测试步骤**:
  1. 用平台提供的**通用**审计事件类型构造一条事件
  2. 通过平台 sink trait 记录它
  3. 查审计表
- **预期结果**:
  1. 步骤 3 有且仅有 1 条 `action='demo.generic'`
  2. 该条 `tenant_id == 't-audit'`
  3. 该条 13 个字段齐全（与 TC-014-002 同一判据）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: design §2.3 把 trait 放在 `platform-core/extension/audit.rs`（**零依赖**），
  落库实现在 `platform-audit`。**AC 没写「谁注入 sink」** —— 业务模块拿到 `Arc<dyn AuditSink>`
  的装配点在哪？本用例按「由 `platform-server` 装配并注入」写。〔S-18〕

### TC-014-070: `platform-org` 已声明的事件（`org.node.create`）能送进平台 sink

- **关联 AC**: AC-S-129
- **前置条件**: T-A 下无 `action='org.node.create'` 的记录
- **测试步骤**:
  1. 通过 `platform-org` 侧的适配器，把 `org.node.create` 事件送进平台 sink
  2. 查审计表
- **预期结果**:
  1. 有且仅有 1 条 `action='org.node.create'`
  2. 该条 `object_type == 'org_node'`（**逐字**，值由实现约定）
  3. 该条 `object_id` 非空
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: D135 决定**不改** `platform-org` 的 `AuditRecorder` trait 签名
  （`crates/platform-org/src/audit.rs:64`），用适配器桥接。
  `AUDIT_EVENTS`（`platform-org/src/audit.rs:128`）里已有 9 条事件码。

### TC-014-071: `platform-iam` 的 SSO 事件能送进平台 sink

- **关联 AC**: AC-S-129
- **前置条件**: T-A 下无 `action='iam.sso.login.succeeded'` 的记录
- **测试步骤**:
  1. 通过 `platform-iam` 侧的适配器，把 SSO 事件送进平台 sink
  2. 查审计表
- **预期结果**:
  1. 有且仅有 1 条 `action='iam.sso.login.succeeded'`
  2. 该条 `tenant_id == 't-audit'`
  3. 该条 `before` / `after` **不含** `client_secret` / `code` / `access_token` / `id_token`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: SSO 载荷的键白名单已存在（`crates/platform-iam/src/sso/audit.rs:135`：
  `{provider, external_id, user_id, ip, user_agent}`）→ 适配器**必须**沿用白名单，
  不能把整个 `SsoAuditPayload` 序列化进去。第 3 条就是这条纪律的守卫。

### TC-014-072: 业务模块源码里**没有** `INSERT INTO platform_audit_log`

- **关联 AC**: AC-S-129
- **前置条件**: 代码已实现
- **测试步骤**:
  1. 扫描 `crates/**/*.rs`（**只扫 .rs**，不含 `migrations/`），找 `INSERT INTO platform_audit_log`
     （大小写不敏感，允许任意空白）
  2. 记录命中列表
- **预期结果**:
  1. 命中列表长度 `== 0`
  2. 扫描器确实扫到了文件（命中文件数 `> 0`，**证明不是扫了个空目录**）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 静态扫描
- **备注**: ⚠️ **AC 原文写「扫描结果只有平台 sink 所在模块命中」—— 与设计冲突。**
  design §3.5 规定写入走 `SECURITY DEFINER` 函数 `platform_audit_append`，
  Rust 侧只调 `SELECT platform_audit_append(...)` → **`INSERT` 语句在 plpgsql 函数体内
  （`migrations/**.sql`），Rust 源码里一处都没有** → 期望命中数是 **0**，
  不是「1」。AC 的措辞会让人以为应该命中 1 处。〔S-08〕

### TC-014-073: 扫描器**阳性对照** —— 注入一段确实含该 SQL 的样例，必然命中

- **关联 AC**: AC-S-129
- **前置条件**: 与 TC-014-072 同一个扫描器
- **测试步骤**:
  1. 构造样例文本：
     `fn demo() { sqlx::query("INSERT INTO platform_audit_log (id) VALUES ($1)").bind(1); }`
  2. 用**同一个**扫描器扫这段样例
  3. 记录命中数
- **预期结果**:
  1. 命中数 `>= 1`
  2. 命中位置落在样例里那段 SQL 上（不是别的东西）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 阳性对照（dev-guide §9.2）
- **备注**: **没有这条，TC-014-072 的「0 命中」不可信** —— 扫描器坏了与真的干净
  在输出上完全一样。dev-guide §9.2 的事故：REQ-021 的「零中文 title」三个人各踩一次。

### TC-014-074: `migrations/**.sql` 里 `INSERT INTO platform_audit_log` **恰好 1 处**

- **关联 AC**: AC-S-129
- **前置条件**: 代码已实现
- **测试步骤**:
  1. 扫描 `migrations/**/*.sql`，找 `INSERT INTO platform_audit_log`
  2. 记录命中位置
- **预期结果**:
  1. 命中数 `== 1`
  2. 该命中位于 `platform_audit_append` 函数体内（`SECURITY DEFINER`）
  3. 该函数带 `SET search_path = public`
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 静态扫描
- **备注**: 补充 TC-014-072 的另一半 —— 「写入 SQL 只存在于平台 sink 的落库通道」这个命题
  要**两处都扫**才说得完整：Rust 侧 0 处、迁移侧恰好 1 处。〔S-08〕

### TC-014-075: `platform-org` / `platform-iam` 的既有测试**全绿**（recorder 签名未破坏）

- **关联 AC**: AC-S-129
- **前置条件**: 代码已实现；`DATABASE_URL_TEST` 已设置
- **测试步骤**:
  1. `cargo test -p platform-org --all-targets -- --test-threads=1`
  2. `cargo test -p platform-iam --all-targets -- --test-threads=1`
- **预期结果**:
  1. 两条命令的退出码都为 `0`
  2. 两条命令的输出里 `FAILED` 出现次数 `== 0`
  3. 通过的用例数 **≥** REQ-013 合并时的基线（org ≥ 91、iam ≥ 165）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 半自动
- **自动化理由**: 这条本质是「跑另外两个 crate 的既有套件」——
  判据是**退出码**，不是某条断言。放进常规测试流程会重复跑两遍大套件，
  建议由**验收脚本/发布流水线**执行一次，而不是每轮 CI 都跑。QA 在合并前手工跑一遍并留输出。
- **设计方法**: 回归护栏
- **备注**: D135 的代价是「3 个 recorder 概念并存」，本条守住「不破坏既有签名」。
  ⚠️ 必须带 `--all-targets`（否则 lib 测试一个都不跑、退出码仍 0 = **假绿**，dev-guide §9.8）；
  必须逐 crate 跑（`cargo test --workspace` 在本仓必失败）。

---

## AC-S-130：审计查询接口（分页、多维过滤、权限控制）

### TC-014-076: `GET /api/audit/events?page=1&size=2` → **2 条** + 总数 **5**

- **关联 AC**: AC-S-130
- **前置条件**: T-A 下恰好 5 条记录
- **测试步骤**:
  1. 用具备 `audit:events:read` 的令牌调 `GET /api/audit/events?page=1&size=2`（带 `x-tenant-id: t-audit`）
  2. 读响应体
- **预期结果**:
  1. HTTP `200`
  2. `items` 长度 `== 2`
  3. `total == 5`
  4. `page == 1`、`size == 2`
  5. `items` 里的 2 条**互不相同**（`id` 不重复）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 响应字段名 `total` / `page` / `size` 取自 design §5.2 —— **AC 只说「总数 5」，没给字段名**。〔S-23〕

### TC-014-077: `total` 与过滤条件**同口径**（过滤后的总数，不是全量）

- **关联 AC**: AC-S-130
- **前置条件**: T-A 下恰好 5 条记录
- **测试步骤**:
  1. 按某个能命中 **2 条**的维度过滤（例如 `action=<只出现 2 次的动作>`）
  2. 读响应的 `items` 与 `total`
- **预期结果**:
  1. `items` 长度 `== 2`
  2. `total == 2`（**不是 5**）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径 + 对照
- **备注**: design §5.2 明写「`total` 是**该租户**满足过滤条件的总数（与 `items` 同过滤条件）」。
  第 2 条防「`total` 恒等于全量」的实现。

### TC-014-078: 按 `actorId` 过滤 → 结果**不多不少**

- **关联 AC**: AC-S-130
- **前置条件**: 5 条记录的 `actor_id` 互不相同
- **测试步骤**:
  1. 调 `GET /api/audit/events?actorId=<第 2 条的 actor_id>`
  2. 读 `items`
- **预期结果**:
  1. `items` 长度 `== 1`
  2. 该条 `actorId == <传入的 actor_id>`
  3. `total == 1`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: 参数名 `actorId`（camelCase）取自 design §5.1 与契约 C13。

### TC-014-079: 按 `action` 过滤 → 结果**不多不少**

- **关联 AC**: AC-S-130
- **前置条件**: 5 条记录的 `action` 互不相同
- **测试步骤**:
  1. 调 `GET /api/audit/events?action=<第 3 条的 action>`
  2. 读 `items`
- **预期结果**:
  1. `items` 长度 `== 1`
  2. 该条 `action == <传入值>`
  3. `total == 1`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: `action` 是**精确匹配**（不是前缀、不是模糊）→ 第 1 条「恰好 1 条」就锁住了这一点。

### TC-014-080: 按 `objectType` 过滤 → 结果**不多不少**

- **关联 AC**: AC-S-130
- **前置条件**: 5 条记录的 `object_type` 互不相同
- **测试步骤**:
  1. 调 `GET /api/audit/events?objectType=<第 4 条的 object_type>`
  2. 读 `items`
- **预期结果**:
  1. `items` 长度 `== 1`
  2. 该条 `objectType == <传入值>`
  3. `total == 1`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: 与 TC-014-078/079 同族，三条必须都在 —— 只实现其中两个过滤维度是最常见的漏做。

### TC-014-081: 按 `objectId` 过滤 → 结果**不多不少**

- **关联 AC**: AC-S-130
- **前置条件**: 5 条记录的 `object_id` 互不相同
- **测试步骤**:
  1. 调 `GET /api/audit/events?objectId=<第 5 条的 object_id>`
  2. 读 `items`
- **预期结果**:
  1. `items` 长度 `== 1`
  2. 该条 `objectId == <传入值>`
  3. `total == 1`
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 等价类
- **备注**: AC-S-130 步骤 2 只列了 `actorId` / `action` / `objectType` / `from`/`to`，
  **漏了 `objectId`** —— 但 design §5.1 与 design §3.1 的索引都含 `object_id`
  （`(tenant_id, object_type, object_id, created_at DESC)`）→ 补上。〔S-32〕

### TC-014-082: 按 `from` / `to` 过滤 → **闭区间**

- **关联 AC**: AC-S-130
- **前置条件**: 5 条记录的 `created_at` 互不相同且已知
- **测试步骤**:
  1. 取第 2 条与第 4 条的 `created_at`，构造 `from=<第2条> & to=<第4条>`
  2. 调 `GET /api/audit/events?from=…&to=…`
  3. 读 `items`
- **预期结果**:
  1. `items` 长度 `== 3`（第 2、3、4 条）
  2. 第 2 条与第 4 条**都在**结果里（**闭区间**：端点含）
  3. 第 1 条与第 5 条**都不在**
  4. `total == 3`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（区间端点）
- **备注**: design §5.1 明写「`created_at` 区间（**闭区间**）」。
  第 2 条是**闭区间**的机械判据 —— 开区间实现会在这里红。
  ⚠️ `from` / `to` 的格式是 RFC3339（design §5.1），但**微秒精度**的 RFC3339
  能不能解析要 BA 确认（`time` crate 的 `parsing` feature **没在** workspace 里开，
  只有 `formatting`）→ 见挑刺 S-09。〔S-09〕

### TC-014-083: 无 `audit:events:read` 权限的令牌 → **403** + 明确错误码

- **关联 AC**: AC-S-130
- **前置条件**: `u-noperm` 已登录但**没有** `audit:events:read`
- **测试步骤**:
  1. 用 `u-noperm` 的令牌调 `GET /api/audit/events`
  2. 读状态码与响应体
- **预期结果**:
  1. HTTP `403`（**不是** 500、**不是** 404）
  2. 响应体是 RFC 9457 ProblemDetails
  3. `code == "INSUFFICIENT_PERMISSION"`（既有码，`crates/platform-core/src/problem.rs:36`）
  4. `params.required == "audit:events:read"`
  5. 响应体**不含**任何审计记录内容
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径
- **备注**: AC 只说「错误码**明确**（不是 500）」—— 本用例把「明确」落成
  `INSUFFICIENT_PERMISSION` + `params.required`（契约 C6 的 `params` 扩展位）。
  契约 C5 规定「动作类越权（有资源但无操作权限）→ **403**」→ 与 AC 一致。〔S-18〕

### TC-014-084: `size=1000` → 被**夹到上限**（≤ 200），不返回 1000 条

- **关联 AC**: AC-S-130
- **前置条件**: T-A 下 5 条记录（条数不足以暴露「返回 1000 条」，所以要**同时**看响应里的 `size`）
- **测试步骤**:
  1. 调 `GET /api/audit/events?page=1&size=1000`
  2. 读响应的 `size` 与 `items` 长度
- **预期结果**:
  1. HTTP `200`（**不是 400**）
  2. 响应 `size <= 200`（**恰好** 200，或更小 —— 只要 `<= 200` 且 `> 0`）
  3. `items` 长度 `<= 200`
  4. `total == 5`（**不受 `size` 影响** —— `total` 是过滤后的总数，不是当页条数）
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（超上限）
- **备注**: 第 4 条防「`total` 被当页大小」的实现错误。
  ⚠️ **口径冲突**：design §7 把 `size = 0` 列为 `AUDIT_INVALID_FILTER`（400），
  但 AC-S-130 步骤 4 说「夹到上限」—— **「非法值」与「超上限」是两类**：
  `size=0` 非法（400），`size=1000` 超上限（夹到 200）。本用例按此写。〔S-09〕

### TC-014-085: `size=0` → **400** + `AUDIT_INVALID_FILTER`

- **关联 AC**: AC-S-130
- **前置条件**: 同 TC-014-084
- **测试步骤**:
  1. 调 `GET /api/audit/events?page=1&size=0`
  2. 读状态码与响应体
- **预期结果**:
  1. HTTP `400`
  2. 响应体是 RFC 9457 ProblemDetails
  3. `code == "AUDIT_INVALID_FILTER"`
  4. `params` 指明是 `size`（**点名参数名**）
- **类型**: 边界
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（零）
- **备注**: 错误码取自 design §7（`AUDIT_INVALID_FILTER`）。
  ⚠️ 若实现用 `Option<u32>` + axum `Query`，`size=0` 能**解析成功** → 由应用层判 0 → 400 ✓；
  但 `size=abc` 会走 axum 的反序列化拒绝 → **400 + 纯文本**（见 TC-014-086）。〔S-09〕

### TC-014-086: `size=abc` → **400** 且响应体是 **ProblemDetails**（不是纯文本）

- **关联 AC**: AC-S-130
- **前置条件**: 同 TC-014-084
- **测试步骤**:
  1. 调 `GET /api/audit/events?page=1&size=abc`
  2. 读状态码与**响应头 `Content-Type`** 与响应体
- **预期结果**:
  1. HTTP `400`
  2. `Content-Type` 是 `application/problem+json`（**不是** `text/plain`）
  3. 响应体可解析成 JSON，且含 `code` 与 `trace_id`
  4. `code` 是 `AUDIT_INVALID_FILTER` 或另一枚**已登记**的错误码（**不是**裸文本）
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: ⚠️ axum 0.8 的 `QueryRejection` 返回 **400 + 纯文本 body**
  （`axum-0.8.9/src/extract/rejection.rs:84-90`：`#[body = "Failed to deserialize query string"]`）
  → 直接用 `Query<AuditQuery>` 会**违反契约 C6**（统一错误响应体）。
  实现要么把数值参数收成 `Option<String>` 自己解析，要么给 `Query` 加自定义 rejection。〔S-09〕

### TC-014-087: `page=0` → **400**（`page` 从 1 起）

- **关联 AC**: AC-S-130
- **前置条件**: 同 TC-014-084
- **测试步骤**:
  1. 调 `GET /api/audit/events?page=0&size=2`
  2. 读状态码与响应体
- **预期结果**:
  1. HTTP `400`
  2. 响应体是 ProblemDetails
  3. `code == "AUDIT_INVALID_FILTER"`
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（零）
- **备注**: design §5.1 说 `page` 是「正整数，1 起」，但 design §7 的
  `AUDIT_INVALID_FILTER` 例子**只列了 `size = 0`** → `page = 0` 的口径**没定**。
  本用例按「与 `size=0` 同处理」写；**BA 需明确 `page=0` 是 400 还是夹到 1**。〔S-09〕

### TC-014-088: 查询参数里的 `tenant_id` **不生效**

- **关联 AC**: AC-S-130
- **前置条件**: T-A、T-B 各有记录；用 T-A 的令牌
- **测试步骤**:
  1. 调 `GET /api/audit/events?tenant_id=t-other`
  2. 读 `items`
- **预期结果**:
  1. HTTP `200`（或 `400`，**二者择一，由 BA 钉死**）
  2. `items` 里**没有**任何 `tenant_id == 't-other'` 的记录
  3. `items` 里的记录**全部** `tenant_id == 't-audit'`
  4. `total` 不因该参数改变
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击路径
- **备注**: design §5.1 写「传了也**必须被忽略**（或直接 400），不得生效」——
  **两者都可接受** → 判据不可判定。本用例把第 1 条写成二选一，
  **BA 必须钉死一个**（建议：忽略并记一条 warn 日志，这样攻击尝试可观测）。〔S-33〕

### TC-014-089: `from > to` → **400** `AUDIT_INVALID_FILTER`

- **关联 AC**: AC-S-130
- **前置条件**: 同 TC-014-084
- **测试步骤**:
  1. 调 `GET /api/audit/events?from=<晚于 to 的时间>&to=<较早的时间>`
  2. 读状态码与响应体
- **预期结果**:
  1. HTTP `400`
  2. `code == "AUDIT_INVALID_FILTER"`
  3. 响应体**不包含**任何审计记录内容
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（区间反转）
- **备注**: design §7 明确列了「`from > to`」。**边界值**：`from == to` 必须**合法**
  （返回该时刻的记录，闭区间）—— 这条由 TC-014-082 覆盖。

### TC-014-090: 列表**排序稳定** —— 翻页不重复、不遗漏

- **关联 AC**: AC-S-130（同时是 AC-S-132 步骤 2 的前提）
- **前置条件**: T-A 下 5 条记录；`created_at` 互不相同
- **测试步骤**:
  1. 调 `?page=1&size=2`，记下 2 个 `id`
  2. 调 `?page=2&size=2`，记下 2 个 `id`
  3. 调 `?page=3&size=2`，记下 1 个 `id`
  4. 比对三页的 `id` 集合
- **预期结果**:
  1. 三页的 `id` **并集**恰好是 5 条记录的 `id` 集合（**不重不漏**）
  2. 三页的 `id` **两两无交集**
  3. 第 3 页 `items` 长度 `== 1`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（分页游标）
- **备注**: ⚠️ **AC-S-132 步骤 2 要求「翻页后记录不重复」，但 design §5 从未定义列表的排序字段**
  （只定义了校验用的 `ORDER BY created_at ASC, id ASC`）。
  若列表按 `created_at DESC` 排序而 `created_at` **有并列**（同一微秒写入两条），
  页码分页就会重复/遗漏 → 需求未定义排序口径就**无法判定**。
  **BA 需钉死**：列表排序 = `ORDER BY created_at DESC, id DESC`（`id` 作 tiebreaker）。
  本用例在 5 条 `created_at` 互不相同的前提下通过 —— **它锁的是「实现必须带 tiebreaker」**：
  若实现只按 `created_at` 排序，本用例在**并列数据**下才红，所以另需一条并列数据用例（BA 拍板后补）。〔S-11〕

---
