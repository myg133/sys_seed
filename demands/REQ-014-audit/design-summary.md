# 设计摘要：审计日志

- **需求编号**: REQ-014
- **状态**: rev 1（2026-11-20）
- **对应验收标准**: [acceptance.md](acceptance.md)（13 条）
- **决策依据**: [decisions.md](decisions.md)（D130~D144）
- **更新时间**: 2026-11-20 09:25

> 本文只写**实现依据**：现状盘点、数据模型、端点、规范化、代码落点。
> 「要验什么」在 acceptance.md，不在这里重复。
>
> ⚠️ **本文件里每个 `file:line` 都是实测出来的**（dev-guide §9.11）。
> Dev **不需要重读**这些位置；但**引用到的 API 语义**若与本文件冲突，
> **以源码为准并立刻回报 BA** —— 本文件可能已经过期。

---

## 1. 现状盘点（写规格前实测，不靠推断）

### 1.1 已经有的（可直接复用，不要重造）

| 能力 | 位置（实测） | 复用方式 |
|---|---|---|
| 审计事件**声明面** | `crates/platform-core/src/extension/auditable.rs:8`（`AuditEventSpec`）、`:42`（`Auditable` trait） | **只有声明面**：`code/action/object_type/description`，**没有 payload**、没有 sink |
| 请求上下文 | `crates/platform-core/src/context.rs:20` | 字段只有 `tenant_id/space_id/user_id/trace_id/entered_at`；**没有 ip / user_agent** → 需按 D140 扩展 |
| 错误码底座 | `crates/platform-core/src/problem.rs:34`（`CODE_*`）、`:57`（`ALL_CODES`）、`:70`（`ProblemDetails`） | 新增错误码按 D125 的 **8 处**编辑点 |
| 错误码标题表 | `crates/platform-core/src/titles.rs` | 现 **79** 条（宽松正则 `\(\s*"([^"]*)"\s*,\s*"` 统计；**不要用全大写正则**，会漏 `org.*`） |
| 租户上下文注入 | `crates/platform-db/src/tenant.rs:28`（`set_config('app.tenant_id', $1, true)`，事务局部） | 复用；审计表靠它做 RLS |
| RLS policy 生成器 | `crates/platform-db/src/rls.rs:143` | 已有生成 `USING/WITH CHECK` 的 helper；**优先复用**而不是手写 policy |
| 平台级写通道模式 | `migrations/20261118001000_config_schema.up.sql:123`（`platform_config_upsert`，`SECURITY DEFINER` + `SET search_path = public`） | 审计写入 / 归档照此模式 |
| RLS 表样例 | `migrations/20261118000200_tenant_isolation.up.sql:29`、`:32`（policy） | 照抄 policy 写法 |
| 权限点注册流程 | `migrations/20261118000901_rbac_permission_seed.up.sql`（+ `crates/platform-rbac/src/registry.rs`） | **改 manifest → 加一条 INSERT → 跑迁移 → 启动对账通过**；忘了 INSERT → **启动失败**（D27） |
| SSO 审计事件 | `crates/platform-iam/src/sso/audit.rs:40/42/44`（三个事件码）、`:133`（`allowed_keys` 白名单） | **桥接**，不改（见 D135） |
| SSO recorder 注入点 | `crates/platform-iam/src/service.rs:369`（`with_sso_audit_recorder`） | 装配层注入平台 sink 的适配器 |
| org 审计事件 | `crates/platform-org/src/audit.rs:128`（`AUDIT_EVENTS`，9 条）、`:64`（`AuditRecorder` trait） | **桥接**，不改（见 D135） |
| 登录入口 | `crates/platform-iam/src/service.rs:568`（`login`）、`:1058`（`login_via_ldap`） | 埋点位置 |
| 权限变更入口 | `crates/platform-rbac/src/service.rs:245`（create_role）、`:299`（delete_role）、`:391`（grant_permission）、`:451`（revoke_permission） | 埋点位置 |
| 配置写入口 | `crates/platform-config/src/service.rs:257`（`write_entries`）、`:366`（`write_flag`） | 埋点位置 |
| 路由装配 | `crates/platform-server/src/main.rs:289`（`Router::new`）、`:317`（`nest("/api/iam", …)`） | 新增 `nest("/api/audit", …)` |
| 哈希依赖 | 根 `Cargo.toml:80`（`sha2 = "0.10"`），已被 `platform-config` / `platform-iam` / `platform-server` 使用 | **零新增第三方依赖** |

### 1.2 **没有的**（本需求要建）

- ❌ **没有任何审计表** —— `migrations/` 全量扫描无 audit 表
- ❌ **`platform-core` 没有 payload 类型的审计事件**，也没有 sink trait
  （REQ-013 的 P14 已踩过这个坑，**不能重犯**）
- ❌ **没有通用 sink** —— org 与 iam 各自建了绑死自己事件类型的 recorder
- ❌ **没有查询 API**、没有审计前端页面
- ❌ **没有链校验 / 保留策略 / 归档**

### 1.3 依赖方向（**实测的 Cargo.toml**）

```
platform-core                (叶子)
platform-db        → core
platform-iam       → core, db
platform-rbac      → core, db, iam
platform-org       → core, db, iam
platform-config    → core, db
platform-schema    → core
platform-server    → core, db, iam, org, rbac, config, schema
```

⚠️ **`platform-rbac → platform-iam` 已存在** → 审计 crate **不得**依赖 `platform-rbac`，
否则 `audit → rbac → iam → audit` **成环**。

---

## 2. 架构：新增 `platform-audit` crate

### 2.1 为什么单独建 crate

审计是**横切关注点**：被 iam / rbac / config / org 同时使用。
放进 `platform-core` 会让 core 背上 sqlx（现在 core 只依赖 axum）；
放进任一业务 crate 则其它 crate 都得依赖它。

### 2.2 依赖与装配

```
platform-core
  └── extension/audit.rs          通用 AuditEvent + AuditSink trait + 规范化 + 敏感键检查
                                    （**零依赖**，iam/org 只依赖 core → 不成环）

platform-audit  (新 crate)
  └── 依赖 platform-core, platform-db, sqlx
      ├── sink.rs      PgAuditSink：INSERT + 读同租户 prev_hash
      ├── query.rs     查询 service（过滤 + 分页）
      ├── verify.rs    链校验
      ├── archive.rs   归档
      └── lib.rs       事件类型（薄封装 core 的）、服务装配

platform-iam     → platform-audit   登录审计 + SSO 适配器（在 iam 侧实现，D135）
platform-rbac    → platform-audit   权限变更审计
platform-config  → platform-audit   配置写入审计
platform-org     → platform-audit   org 事件适配器（在 org 侧实现，D135）
platform-server  → platform-audit   装配 + `/api/audit` 路由（HTTP 放这里）
```

**为什么 HTTP 放 `platform-server` 而不是 `platform-audit`**：
权限检查要用 `platform-rbac`，而 `platform-audit → platform-rbac → platform-iam → platform-audit`
会**成环**。放 `platform-server` 则它已依赖全部 crate，无环。
先例：`crates/platform-server/src/i18n.rs` 就是在 server 侧自带 router 的。

**为什么适配器放在「拥有者」侧**（iam / org）：
`platform-audit` 若要构造 `OrgAuditEvent` / `SsoAuditEvent` 就得依赖 org / iam → 成环。
所以：**审计 crate 不知道 org/iam 的存在**，由它们各自实现转换。

### 2.3 `platform-core` 新增：`extension/audit.rs`

```rust
/// 通用审计事件（**带 payload**）。
pub struct AuditEvent {
    pub tenant_id: String,
    pub actor_id: Option<String>,
    pub action: String,
    pub object_type: String,
    pub object_id: Option<String>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

/// 审计落点（可对象化）。
pub trait AuditSink: Send + Sync {
    fn record<'a>(&'a self, ctx: &'a RequestContext, event: &'a AuditEvent)
        -> BoxFuture<'a, Result<()>>;
}
```

- `AuditSink` 用 `BoxFuture`（与 `platform-org/src/audit.rs:22` 的 `BoxFuture` 同款），
  保持「可对象化」，允许注入。
- 字段与契约 C12 **逐字对应**；`before`/`after` 用 `serde_json::Value`
  （不是字符串）→ 规范化在落库前统一做（§4）。
- **不引入 `Auditable` 的循环依赖**：`Auditable`（声明面）保留不变，
  平台启动时读各模块的 `audit_events()` 汇总成「已声明事件目录」，
  供 `GET /api/audit/events` 的元数据与文档使用。

---

## 3. 数据模型

### 3.1 新建：`platform_audit_log`（在线表）

字段**严格按契约 C12**（`api-conventions v1` §C12）：

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | `TEXT PRIMARY KEY` | UUID v4（既有惯例用 `uuid` crate） |
| `tenant_id` | `TEXT NOT NULL` | 租户；链的边界（D130） |
| `actor_id` | `TEXT` | 操作人；未认证（如登录失败）为 `NULL` |
| `action` | `TEXT NOT NULL` | 动作名 |
| `object_type` | `TEXT NOT NULL` | 对象类型 |
| `object_id` | `TEXT` | 对象 ID |
| `before` | `TEXT` | 变更前；**规范化 JSON 文本**；无则 `NULL`（D141） |
| `after` | `TEXT` | 变更后；**规范化 JSON 文本**；无则 `NULL`（D141） |
| `ip` | `TEXT NOT NULL DEFAULT ''` | 来源 IP；取不到落**空串**（AC-S-122 要求不得缺列） |
| `user_agent` | `TEXT NOT NULL DEFAULT ''` | 设备 |
| `created_at` | `TIMESTAMPTZ NOT NULL` | UTC；由 DB 的 `clock_timestamp()` 生成，同租户严格递增（D143） |
| `prev_hash` | `TEXT NOT NULL` | 上一条的 `hash`；本租户首条为 genesis |
| `hash` | `TEXT NOT NULL` | 本条哈希 |

> 🔑 **为什么 `before`/`after` 是 `TEXT` 而不是 `JSONB`**（D141）：
> 哈希覆盖的是**存进列里的那段文本**。校验方读回该文本直接参与哈希 →
> **不需要重新实现 JSON 序列化规则** → 测试与生产在「哈希公式 + 链连接」上真独立。
> 若用 `JSONB`，校验方必须重新序列化一次，而「键序 / 数字表示 / Unicode 转义」
> 差一个字节就会让**「链校验失败」与「实现有 bug」无法区分** → 假绿。
> 代价：按内容检索要写 `before::jsonb`（记入 tech-debt）。

- **索引**：`(tenant_id, created_at DESC)`、`(tenant_id, actor_id, created_at DESC)`、
  `(tenant_id, action, created_at DESC)`、`(tenant_id, object_type, object_id, created_at DESC)`
  —— 覆盖 AC-S-130 的四种过滤。
- `created_at` 用 `TIMESTAMPTZ`（契约 C13：一律 UTC 存储）。

### 3.2 新建：`platform_audit_archive`（归档表）

**字段与在线表完全相同**（AC-S-131 要求「逐字段相同，含 hash/prev_hash」），
外加 `archived_at TIMESTAMPTZ NOT NULL DEFAULT now()`。

### 3.3 RLS（D132）

两张表**都**启用 RLS，policy 照抄 `migrations/20261118000200_tenant_isolation.up.sql:32`：

```sql
ALTER TABLE platform_audit_log ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS platform_audit_log_tenant_isolation ON platform_audit_log;
CREATE POLICY platform_audit_log_tenant_isolation ON platform_audit_log
    USING (tenant_id = current_setting('app.tenant_id', true))
    WITH CHECK (tenant_id = current_setting('app.tenant_id', true));
```

⚠️ `current_setting('app.tenant_id', true)` 的第二个参数 `true` **不能省** ——
省了会导致所有未设租户上下文的查询直接报错（`crates/platform-db/src/rls.rs:15` 的注释）。

### 3.4 权限（D133）

```sql
GRANT SELECT, INSERT ON platform_audit_log TO base_platform_app;
REVOKE UPDATE, DELETE ON platform_audit_log FROM base_platform_app;   -- append-only
GRANT SELECT, INSERT ON platform_audit_archive TO base_platform_app;
REVOKE UPDATE, DELETE ON platform_audit_archive FROM base_platform_app;
```

- `GRANT`/`REVOKE` 写在 `DO $$ … IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') … $$`
  块里（与 `migrations/20261120000100_iam_sso.up.sql:95` 同款 —— 组不存在时跳过）。
- ⚠️ **RLS 只对不绕过它的角色生效**：超级用户与 `BYPASSRLS` 角色会绕过 →
  **应用连接必须用普通角色**（`migrations/20261118000200_tenant_isolation.up.sql:57` 的警告）。
  AC-S-126 步骤 2 因此**必须用应用角色**连接。

### 3.5 写入通道：`SECURITY DEFINER` 函数

照 `migrations/20261118001000_config_schema.up.sql:112` 的模式：

```sql
CREATE OR REPLACE FUNCTION platform_audit_append(
    p_tenant_id TEXT, p_actor_id TEXT, p_action TEXT,
    p_object_type TEXT, p_object_id TEXT,
    p_before TEXT, p_after TEXT,
    p_ip TEXT, p_user_agent TEXT
) RETURNS TEXT
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public AS $$ ... $$;
```

- `SET search_path = public` 是 **SECURITY DEFINER 的必需防护**（同文件 `:118` 的注释）。
- `p_before` / `p_after` 入参是 **TEXT**（已规范化的 JSON 文本或 `NULL`），
  **函数内部不做 JSON 处理** → 哈希覆盖的就是**存进列里的那段文本**（D141）。
- 函数内部**自己**算 `prev_hash`（读本租户最后一条）与 `hash` ——
  **哈希计算必须在数据库内完成**，否则并发写入会算出同一个 `prev_hash` → 链分叉。
- 函数内部做**敏感键拒绝**（D134）→ 拒绝时 `RAISE EXCEPTION`，应用层映射成错误码。
  🔑 敏感键检查作用在**入参文本**上：需要能识别 JSON 文本里的敏感键
  （可用 Postgres 的 `jsonb_path_exists` 或 `jsonb_each_text`；**不得**把值原样回显到错误信息里）。
- 函数内部做 **JSON 规范化**（D142 的 §4.2.2 规则）—— 规范化在**写入侧**发生，
  保证存进去的文本形态稳定。
- 函数内部取 `created_at`：**先锁同租户最后一条，再取 `clock_timestamp()`**（D143）。
- 返回新记录的 `id`。

> 🔑 **为什么 `prev_hash` 必须由数据库算**：
> 应用层「先查最后一条 → 再插入」在并发下有两个请求读到**同一条**最后一条，
> 两条新记录会拿到**相同**的 `prev_hash` → 链分叉 → 校验必然失败。
> 函数内用 `SELECT ... ORDER BY created_at DESC, id DESC LIMIT 1 FOR UPDATE`
> 锁住上一条，或用表级 `LOCK`。**具体手法 Dev 定，但必须能解释并发下的正确性**
> （见 design-summary §12 开工前必答项 1）。

### 3.6 归档通道：`SECURITY DEFINER` 函数

```sql
CREATE OR REPLACE FUNCTION platform_audit_archive_move(p_before TIMESTAMPTZ)
RETURNS BIGINT LANGUAGE plpgsql SECURITY DEFINER SET search_path = public AS $$ ... $$;
```

- 同一事务内 `INSERT ... SELECT` 到归档表 + `DELETE` 在线表；
- 搬过去的行**逐字段相同**（`hash` / `prev_hash` 原样带走）；
- **幂等**：第二次执行没有可搬的行 → 影响 0 行，不报错。

> 🔑 **为什么不让应用角色直接 DELETE**：
> 那样就必须给它 DELETE 权限 → append-only 立刻破功（AC-S-123 / AC-S-131 直接失败）。
> 受限的特权函数是唯一同时满足两者的做法（D56 同款）。

---

## 4. 规范化定义（D131 —— **必须逐字实现**）

契约 C12 只写了 `hash = sha256(prev_hash + 规范化事件体)`，**没定义「规范化」**。
不统一定义 → Dev 与 QA 各算各的 → 链校验永远失败，或「各自算对了自己那套」→ 假绿。

### 4.1 字段顺序（**固定，不可换**）

```
tenant_id, actor_id, action, object_type, object_id,
before, after, ip, user_agent, created_at
```

### 4.2 拼接规则

```
canonical = prev_hash_bytes
          + for f in fields: (len(f).to_string() + ":" + f)
hash      = hex(sha256(canonical))
```

- `len(f)` 是**字节长度**（`str::len()`），不是字符数；
- `prev_hash` 按**文本**（hex 字符串）的 **64 个字节**参与拼接；
- 各字段的取值：
  | 字段 | 取值 |
  |---|---|
  | `tenant_id` | 原文本 |
  | `actor_id` | 无则**空字符串**（零字节） |
  | `action` / `object_type` | 原文本 |
  | `object_id` | 无则**空字符串**（零字节） |
  | `before` / `after` | **存进列里的那段规范化 JSON 文本**；`NULL` 时用**空字符串**（零字节，不是 `"null"`、不是 `"{}"`） |
  | `ip` / `user_agent` | 原文本（无则空串） |
  | `created_at` | **UTC、RFC3339、微秒精度**，精确格式见 §4.2.1 |

### 4.2.1 时间戳文本的**精确格式**（D142 —— 逐字节钉死）

```
YYYY-MM-DDTHH:MM:SS.ffffffZ
```

- 年 **4 位**；月 / 日 / 时 / 分 / 秒**各 2 位、零填充**（`03` 而非 `3`）；
- 小数部分**恰好 6 位**（微秒），不足**补 0**（`.100000` 而不是 `.1`）；
- 结尾是**字面量大写 `Z`** —— 不是 `+00:00`、不是 `+0000`、不是 `z`。

**DB 侧**用 `to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')`
（Postgres 17.12 实测：微秒 456789 → `2026-11-20T01:02:03.456789Z`；
微秒为 0 → `2026-11-20T01:02:03.000000Z`，**恒 6 位** ✓）。

**Rust 侧必须手工拼装**（D144）：

```rust
format!(
    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
    dt.year(), dt.month() as u8, dt.day(),
    dt.hour(), dt.minute(), dt.second(),
    dt.millisecond() * 1000 + dt.microsecond() % 1000,
)
```

- 🔑 **禁止** `time::format_description::well_known::Rfc3339`：
  它在微秒为 0 时**整个省略小数部分** → 违反「恰好 6 位」。
- 🔑 **禁止**为此开启 `time` 的 `macros` / `parsing` feature：
  根 `Cargo.toml:49` 只开了 `formatting`，且该行注释明确
  「避免拉进 proc-macro 编译链：低配机编译开销敏感」。
- Dev 必须写一条测试，断言 Rust 侧输出与 DB 侧 `to_char` 输出**逐字节相同**
  （含微秒为 0 的边界）。

> ⚠️ 两边不一致 → 链校验永远失败 → **「实现有 bug」与「规范不一致」无法区分** → 假绿。
> 测试会**独立实现**这个格式（用 `time` crate，不调用生产代码）→ 不一致时测试**红**，
> 这是**正确的**（真阳性）。

### 4.2.2 JSON 规范文本的**精确格式**（D142 —— 逐字节钉死）

写入时把 `before` / `after` 的 JSON 规范化成：

- 对象的键按 **UTF-8 字节序升序**排序，**递归**到每一层；
- **无任何多余空白**（紧凑）；
- 非 ASCII 字符**不转义**（按 UTF-8 原样输出）——
  🔑 **禁止** `\uXXXX` 转义非 ASCII；
- 控制字符用 JSON 标准短转义（`\n` `\t` `\r` `\b` `\f`），其余控制字符用 `\u00XX`；
- 字符串里的 `"` 与 `\` 必须转义。

> 🔑 **校验方不需要重新实现这套规则**（D141）：它读回**列里存的那段文本**直接参与哈希。
> 这套规则只影响**写入侧**产出的文本形态 —— 它**必须稳定**（否则同一条记录的
> `hash` 与它的内容对不上），但**校验方不依赖它**。
>
> ⚠️ 因此：**测试不要**「把 `before` 解析回 JSON 再重新序列化再哈希」——
> 那是**测试验证生产代码的拷贝**（流程强制项 7），且会因序列化实现差异假红。
> 正确做法：**读回列里的 TEXT，直接哈希**。

### 4.3 JSON 规范化（写入侧）

- 🔑 **不得依赖 `serde_json` 的 `Map` 迭代序**：
  `serde_json` 默认 `Map` 是 `BTreeMap`（键有序 ✅），
  但若任一 crate 打开了 `preserve_order` feature，就变成 `IndexMap`（插入序 ❌）→
  **同一份 JSON 两次规范化产出不同文本** → 哈希不可复现。
- → 必须**自己实现**规范化（递归按键排序后紧凑输出），并**写测试锁住**：
  `{"b":1,"a":2}` 与 `{"a":2,"b":1}` 必须产出**同一段文本**。

### 4.4 genesis

本租户**第一条**记录的 `prev_hash` = **64 个字符 `0`**（不是空串，不是 `NULL`）。

### 4.5 `created_at` 的单调性（D143）

- 写入函数内**先取同租户最后一条并加锁**，**再取 `clock_timestamp()`**
  → 同租户 `created_at` **严格递增** → 校验方按 `created_at ASC` 排序即等于插入顺序。
- 🔑 **不用 `now()`** —— `now()` 是**事务开始**时间，同一事务里的多条审计会拿到
  **同一时刻**，链的时间序就废了（批量导入场景真实存在）。
- 校验方排序用 `ORDER BY created_at ASC, id ASC`（`id` 作 tiebreaker 兜底）。

---

## 5. 端点设计

| 方法 | 路径 | 权限点 | 说明 |
|---|---|---|---|
| GET | `/api/audit/events` | `audit:events:read` | 分页 + 过滤 |
| GET | `/api/audit/events/{id}` | `audit:events:read` | 单条详情 |
| GET | `/api/audit/verify` | `audit:verify:run` | 链校验 |
| POST | `/api/audit/archive` | `audit:archive:run` | 触发归档 |

### 5.1 查询参数（D138）

| 参数 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `page` | 正整数，1 起 | 1 | 页码 |
| `size` | 正整数 | 50 | **上限 200**（AC-S-130 步骤 4 要求夹住） |
| `actorId` | 文本 | — | 精确匹配 |
| `action` | 文本 | — | 精确匹配 |
| `objectType` | 文本 | — | 精确匹配 |
| `objectId` | 文本 | — | 精确匹配 |
| `from` / `to` | RFC3339 | — | `created_at` 区间（闭区间） |

- 查询参数名用 **camelCase**（契约 C13：JSON 字段 camelCase）。
- `tenant_id` **不接受**来自查询参数 —— 租户只来自认证上下文 + RLS（D132）。
  传了也**必须被忽略**（或直接 400），不得生效。

### 5.2 响应形状

```json
{
  "items": [ { "id": "...", "actorId": "...", "action": "...", "objectType": "...",
               "objectId": "...", "before": {...}, "after": {...},
               "ip": "...", "userAgent": "...", "createdAt": "...",
               "prevHash": "...", "hash": "..." } ],
  "total": 5,
  "page": 1,
  "size": 2
}
```

- `total` 是**该租户**满足过滤条件的总数（与 `items` 同过滤条件）。
- 校验响应：

```json
{ "ok": false, "checked": 3,
  "broken": [ { "id": "...", "reason": "hash_mismatch", "expected": "...", "actual": "..." } ] }
```

- `reason` 取值**恰好两个**：`hash_mismatch`（重算哈希与存储不符）、
  `prev_hash_mismatch`（本条的 `prev_hash` 与上一条的 `hash` 不接）。

### 5.3 归档响应

```json
{ "moved": 12, "archivedAt": "2026-11-20T01:02:03.456789Z",
  "nextRecommendedAt": "2026-11-21T01:02:03.456789Z" }
```

- `nextRecommendedAt` 是 D137 要求的「让运维看得见下次该什么时候归档」。

---

## 6. 统一埋点入口与适配器（D135 / D136）

### 6.1 平台侧埋点（本轮实际接三类）

| 类别 | 位置（实测） | 事件码 |
|---|---|---|
| 本地登录成功 / 失败 | `crates/platform-iam/src/service.rs:568` | `iam.login.succeeded` / `iam.login.failed` |
| LDAP 登录 | `crates/platform-iam/src/service.rs:1058` | 同上（provider = `ldap`） |
| SSO 登录成功 / 失败 | 已有事件，`crates/platform-iam/src/sso/audit.rs:40/42` | `iam.sso.login.succeeded` / `iam.sso.login.failed` |
| 角色增删 | `crates/platform-rbac/src/service.rs:245` / `:299` | `rbac.role.create` / `rbac.role.delete` |
| 权限授予 / 撤销 | `crates/platform-rbac/src/service.rs:391` / `:451` | `rbac.role.grant` / `rbac.role.revoke` |
| 配置条目写入 | `crates/platform-config/src/service.rs:257` / `:366` | `config.entries.write` / `config.flag.write` |

- **失败登录也必须审计**（AC-S-127），且**不含**密码字面量。
- 事件码沿用既有命名法：`<域>.<对象>.<动作>`（既有 `org.node.create`、
  `iam.sso.login.succeeded`）。

### 6.2 适配器（在拥有者侧实现）

- `platform-iam` 实现 `SsoAuditEvent → AuditEvent` 的转换，
  实现 `platform_core::extension::audit::AuditSink`，
  在 `crates/platform-iam/src/service.rs:369`（`with_sso_audit_recorder`）处注入。
- `platform-org` 实现 `OrgAuditEvent → AuditEvent` 的转换 + `AuditSink` 实现。
- 🔑 **既有的 `SsoAuditRecorder` / `AuditRecorder` trait 签名一律不改**
  （AC-S-129 步骤 5 要求既有测试全绿）。

### 6.3 业务方怎么用（这是「底座」的交付方式）

```rust
// 业务模块：声明事件（已有机制）+ 用平台 sink 记录
let sink: Arc<dyn AuditSink> = app.audit_sink();
sink.record(&ctx, &AuditEvent { tenant_id, actor_id, action, object_type, object_id, before, after, ip, user_agent }).await?;
```

业务模块**不写审计表 SQL**。AC-S-129 步骤 4 用扫描断言这一点。

---

## 7. 错误码清单（**新增 4 个**）

| 常量 | 值 | 何时 |
|---|---|---|
| `AUDIT_SENSITIVE_PAYLOAD_REJECTED` | 同左 | `before`/`after` 含敏感键（D134） |
| `AUDIT_CHAIN_BROKEN` | 同左 | 校验发现链断裂 |
| `AUDIT_EVENT_NOT_FOUND` | 同左 | `GET /api/audit/events/{id}` 查不到 |
| `AUDIT_INVALID_FILTER` | 同左 | 过滤参数非法（如 `size = 0`、`from > to`） |

### 7.1 ⚠️ 新增错误码 = **8 处**编辑（D125，REQ-013 实测）

1. code 常量定义（`crates/platform-core/src/problem.rs` 或所属 crate 的 `error.rs`）
2. 对应的 `enum` 变体
3. `code()` 方法的 match 分支
4. `detail()` 方法的 match 分支
5. `ALL_CODES`（`crates/platform-core/src/problem.rs:57`）
6. `TITLES`（`crates/platform-core/src/titles.rs`）—— 现 **79** 条 → 变 **83**
7. 测试里的 `variants` 数组
8. 测试里的 `errors` 数组

**漏任何一处 → 启动失败 / 对账失败。** 计数 TITLES 时**必须**用宽松正则
`\(\s*"([^"]*)"\s*,\s*"`（全大写正则会漏掉 `org.*` 那 6 条）。

---

## 8. 配置项（登记进 `crates/platform-config/manifest.json`）

| 键 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `audit.retention.online_days` | number | 90 | 在线保留天数；超过即被归档搬走 |
| `audit.retention.archive_days` | number | 365 | 归档保留天数；**本轮不实现物理删除**（见下） |
| `audit.page.default_size` | number | 50 | 查询默认每页条数 |
| `audit.page.max_size` | number | 200 | 查询每页条数上限 |

- ⚠️ **配置项新增有严格流程**：改 `manifest.json` → 加迁移 INSERT → 跑迁移 → 启动对账通过。
  忘了 INSERT → **启动失败**（D27，见 `migrations/20261118000901_rbac_permission_seed.up.sql` 头注释）。
- `audit.retention.archive_days` 本轮**只登记不执行**（物理删除超出归档期的记录 = 删数据，
  风险与收益不匹配）→ 记入 tech-debt。

---

## 9. 权限点（新增 3 个）

| 编码 | 名称 | kind | source |
|---|---|---|---|
| `audit:events:read` | 查看审计日志 | `menu` | `crates/platform-audit/manifest.json` |
| `audit:verify:run` | 校验审计链 | `button` | 同上 |
| `audit:archive:run` | 触发审计归档 | `button` | 同上 |

- 沿用既有 `模块:资源:动作` 三段式（既有 `sandbox:demo:read`、`org:org-node:write`）。
- 🔑 `crates/platform-audit/manifest.json` **必须新建**，且**必须**在
  权限点种子迁移里加 **3 条 INSERT**（`source` 指向该 manifest 路径）——
  **两处都要有，否则启动对账失败**。

---

## 10. 代码落点（Dev 按此切文件，避免冲突）

### 后端（新建 crate）

```
crates/platform-audit/
├── Cargo.toml                （workspace 成员；依赖 core, db, sqlx）
├── manifest.json             （3 个权限点）
├── src/
│   ├── lib.rs                （事件类型、服务装配）
│   ├── sink.rs               （PgAuditSink：调用 SECURITY DEFINER 函数）
│   ├── query.rs              （查询 + 分页）
│   ├── verify.rs             （链校验）
│   ├── archive.rs            （归档）
│   └── error.rs              （4 个错误码 + 变体 + code/detail）
└── tests/
    ├── audit_real_db.rs      （真实库集成测试；用 base_test）
    └── audit_chain.rs        （链 + 篡改 + 规范化）
```

### 平台核心

```
crates/platform-core/src/extension/audit.rs    （新增：AuditEvent + AuditSink + 规范化 + 敏感键）
crates/platform-core/src/extension/mod.rs      （挂载新模块）
crates/platform-core/src/context.rs             （+ client_ip / user_agent，D140）
crates/platform-core/src/problem.rs             （+ 4 个错误码的 8 处编辑点）
crates/platform-core/src/titles.rs              （+ 4 条标题）
```

### 数据库

```
migrations/20261120000200_audit.up.sql          （两张表 + RLS + GRANT/REVOKE + 2 个 SECURITY DEFINER 函数）
migrations/20261120000200_audit.down.sql
migrations/20261120000300_audit_permission_seed.up.sql   （3 条权限点 INSERT）
migrations/20261120000300_audit_permission_seed.down.sql
```

### 业务模块（埋点 + 适配器）

```
crates/platform-iam/src/service.rs       （登录埋点）
crates/platform-iam/src/sso/            （SSO 适配器；**不改 trait 签名**）
crates/platform-rbac/src/service.rs      （权限变更埋点）
crates/platform-config/src/service.rs    （配置写入埋点）
crates/platform-org/src/                 （org 适配器；**不改 trait 签名**）
```

### 平台侧

```
crates/platform-server/src/audit.rs      （/api/audit 路由；**放这里**，避免 audit→rbac→iam→audit 成环）
crates/platform-server/src/main.rs        （+ nest("/api/audit", …)；+ 注入 sink 到各 service）
crates/xtask/src/                        （+ `audit archive` 命令）
```

### 前端

```
apps/shell/src/pages/Audit.svelte        （新增页面）
apps/shell/src/lib/routes.ts             （+ /admin/audit 路由）
apps/shell/src/lib/audit-api.ts          （新增：SDK 调用）
apps/shell/src/lib/i18n.ts（或对应文案表）（+ 审计页文案）
```

---

## 11. 依赖变更

- **零新增第三方依赖**（dev-guide §9.10）。
  `sha2`（根 `Cargo.toml:80`）已在树里；`sqlx` / `axum` / `serde` / `uuid` / `time` 均已在树里。
- 新增 **1 个 workspace 成员**：`crates/platform-audit`。
- `Cargo.lock` 的编译单元预计 **+1 ~ +3**（`platform-audit` 本体 + `platform-core` 不变）。
  实际数字以实现后的 `cargo tree` 为准，**不要预估后当结论报**。

---

## 12. 开工前必答项（Dev 不得自行决定，遇阻即报 BA）

1. **`platform_audit_append` 里 `prev_hash` 的并发安全方案**：打算怎么保证并发写入不产生
   相同 `prev_hash`？（本文件建议：同事务 + 锁；具体手法 Dev 定，但**必须**给出理由）
2. **JSON 规范化的实现位置**：放 `platform-core`（可被业务方复用）还是 `platform-audit`？
   本文件建议 **core**（业务方自己落审计时也需要）。Dev 若不同意，给理由。
3. **`RequestContext` 加字段后，有没有任何地方把它序列化/跨进程传递**？
   （D140 的连带检查项；有的话必须同步改）
4. **查询接口放在 `platform-server` 还是 `platform-audit`**？本文件建议 server（防成环）。
   Dev 若认为有更优解，给出**依赖图证明**。
5. **`audit.retention.archive_days` 物理删除**：本轮只登记不执行 —— 确认同意。

---

## 13. 与 AC 的对应

| AC | 实现落点 |
|---|---|
| AC-S-022 | `platform-audit/src/verify.rs` + error 日志 |
| AC-S-122 | `platform-audit/src/sink.rs` + `platform-core/src/extension/audit.rs` |
| AC-S-123 | 迁移 §3.4 的 `REVOKE UPDATE, DELETE` |
| AC-S-124 | 迁移 §3.5（数据库内算链）+ §4 规范化 |
| AC-S-125 | `platform-audit/src/verify.rs`（两种 reason） |
| AC-S-126 | 迁移 §3.3 的 RLS policy |
| AC-S-127 | `platform-iam/src/service.rs:568` / `:1058` + SSO 适配器 |
| AC-S-128 | `platform-rbac/src/service.rs:245/299/391/451` |
| AC-S-129 | `platform-core` 的 `AuditSink` + 两侧适配器 + 扫描断言 |
| AC-S-130 | `platform-server/src/audit.rs` + `platform-audit/src/query.rs` |
| AC-S-131 | 迁移 §3.6 + `platform-audit/src/archive.rs` |
| AC-S-132 | `apps/shell/src/pages/Audit.svelte` |
| AC-S-133 | `platform-core/src/extension/audit.rs` 的敏感键检查 |

---

## 14. 与既有机制的接缝（**Dev 必读 —— 漏一处就起不来**）

> 本节全部是**实测**的既有约束，不是我「记得」的。dev-guide §9.11。

### 15.1 权限点**三处必须齐**（D27：不齐 → 进程起不来）

新增的 3 个权限点必须同时出现在：

| # | 位置 | 要求 | 实测依据 |
|---|---|---|---|
| 1 | `crates/platform-audit/manifest.json` 的 `permissions[]` | 字段是 `code` / `name` / **`type`**（⚠️ **不是 `kind`**） | `crates/platform-org/manifest.json` 的实际字段 |
| 2 | 权限点种子迁移的 **3 条 INSERT** | `source` 列**必须**是 `'crates/platform-audit/manifest.json'` | `crates/platform-rbac/src/registry.rs:140` 用 `format!("{dir_name}/{name}/manifest.json")` 拼出来 → `crates/platform-audit/manifest.json` |
| 3 | 代码里的 `require_permission("…")` **字面量**调用点 | 参数**必须是字符串字面量** | `crates/platform-contracts/src/authz_scan.rs`：调用点参数不是字面量 → **fail closed** |

- 漏 1 → 库里有权限点、manifest 里没有 → **启动对账失败**；
- 漏 2 → manifest 里有、库里没有 → **启动对账失败**；
- 漏 3 → 「登记了却不执行 = 假权限」→ **C-5 契约测试失败**。

### 15.2 路由鉴权的既有写法

照 `crates/platform-rbac/src/http.rs:451`：

```rust
.route_layer(require_permission("audit:events:read"));   // ← 必须是字面量
```

- 权限点 `audit:events:read` 被两个路由用（`/events` 与 `/events/{id}`）→
  可以用**一个** `route_layer` 覆盖一组路由（既有 `rbac:role:read` 就是这么用的）。
- 🔑 **鉴权入口必须唯一**：`crates/platform-core/src/extension/permission.rs`。
  **自己写一个 `check_permission()` 会被 C-5 扫描判失败**（`authz_scan.rs` 的
  `AUTH_ENTRY_ALIASES`）。

### 15.3 manifest 的字段集

`crates/platform-audit/manifest.json` 会被 `Manifest::parse_checked` 校验
（`crates/xtask/src/capabilities.rs:241`、`crates/platform-rbac/src/registry.rs:130`）。
照 `crates/platform-org/manifest.json` 的字段集写：

```json
{
  "id": "audit",
  "name": "审计日志",
  "version": "0.1.0",
  "enabled": true,
  "permissions": [
    { "code": "audit:events:read",  "name": "查看审计日志", "type": "menu" },
    { "code": "audit:verify:run",   "name": "校验审计链",   "type": "button" },
    { "code": "audit:archive:run",  "name": "触发审计归档", "type": "button" }
  ],
  "menus": [
    { "label": "审计日志", "icon": "file-search", "route": "/admin/audit", "perm": "audit:events:read" }
  ],
  "dependencies": [],
  "tenantScoped": { "tables": [], "column": "tenant_id" },
  "config": {}
}
```

- `menus` 段是**前端审计页的入口声明**（前端按 manifest 渲染菜单）。
- ⚠️ `tenantScoped.tables` **留空**（见 15.5）。

### 15.4 `capabilities/permissions.json` 是**生成物**

- `cargo xtask capabilities` 会重扫 `modules/` + `crates/` 的 manifest
  （`crates/xtask/src/capabilities.rs:230`）→ 新 crate 的 3 个权限点**会自动进去**。
- 🔑 **新增 manifest 后必须重跑 `cargo xtask capabilities`**，
  否则 `cargo xtask capabilities --check` **会红**（生成物与源码不一致）。
- 文件**数量仍是 14**（文件名没变），变的是 `permissions.json` 的**内容**。

### 15.5 RLS policy 的两条路（Dev 必须选一条并说明）

| 路线 | 做法 | 取舍 |
|---|---|---|
| **A. 声明式** | 把审计表写进 `crates/platform-audit/manifest.json` 的 `tenantScoped.tables`，让 `cargo xtask tenant-policies` 生成 policy（`crates/platform-db/src/rls.rs:143`） | 与既有机制一致、**不手写 policy**；但要确认生成器产出的 policy 形态与 D132 要求一致 |
| **B. 手写** | 在迁移里手写 `CREATE POLICY`（照 `migrations/20261118000200_tenant_isolation.up.sql:32`） | 形态完全可控；但**绕过了** manifest 声明，与既有机制不一致 |

- ⚠️ **两条路不能都做** → 会重复建 policy。
- 本文件的 §3.3 给的是路线 B 的写法；**Dev 若选路线 A，必须确认**
  `tenantScoped` 生成的 policy 与 §3.3 逐字等价（`USING` + `WITH CHECK` + `current_setting('app.tenant_id', true)`）。
- ⚠️ 注意 `platform-org` 的 `tenantScoped.tables` 里写的是**逻辑表名**（`org_node`），
  不是物理表名（`platform_org_node`）—— Dev 必须确认生成器认哪种。

---

## 15. 本轮已替 Dev 实测掉的事实（不必重跑）

| 事实 | 结论 | 怎么验的 |
|---|---|---|
| 应用连接角色 `base_app` 是 `base_platform_app` 的**组成员** | `REVOKE UPDATE, DELETE ... FROM base_platform_app` **对 `base_app` 生效** → AC-S-123 **可达** | `pg_auth_members` 查询 + `has_table_privilege('base_app', ...)` = t/t/t |
| `base_app` 不是表 owner（owner 是 `base`），且 `rolbypassrls = false` | REVOKE 有效；**RLS 对它生效** → AC-S-126 **可达** | `pg_class.relowner` + `pg_roles.rolbypassrls` |
| `base` 是超管 + `BYPASSRLS` | 应用连接**必须**用 `base_app`（超管 `base` 会被 `require_isolation` 拒绝，且绕过 RLS） | `pg_roles` 查询 |
| Postgres `to_char(…, 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')` | 微秒 456789 → `…03.456789Z`；微秒 0 → `…03.000000Z`（**恒 6 位**） | Postgres 17.12 实测 |
| `time` crate 只开了 `formatting` | `format_description!` 宏与 `format_description::parse()` **都不可用** → 必须手工拼装（D144） | 根 `Cargo.toml:49` |
| `sha2 = "0.10"` 已在树里（`platform-config` / `platform-iam` / `platform-server` 在用） | 本轮**零新增第三方依赖** ✓ | 根 `Cargo.toml:80` |

---

## 16. 低配机注意事项（8 线程 / 7.7G）

- `.cargo/config.toml` 已固定 `jobs = 3` —— **不要**覆盖。
- 新增 crate 会增加一次编译。`platform-audit` 只依赖 core/db，**编译成本低**。
- 🔑 **`cargo test -p <crate>` 必须加 `--all-targets`**，否则 lib 里的测试一个都不跑、
  退出码仍是 0（**假绿**）。
- 🔑 **`cargo test --workspace` 在本仓必失败** → 必须逐 crate 跑。
- 🔑 **`cargo test` 只能有一个 `--`**（双 `--` → 0 passed 却 exit 0）。
- 🔑 **`DATABASE_URL_TEST` 必须设置**（`postgres://base:base@127.0.0.1:5432/base_test`），
  否则真实库用例会 panic。**测试只用 `base_test`，绝不写 `base_dev`。**
- 真实库集成测试**串行**跑：`-- --test-threads=1`。
- 应用连接用 **`base_app`**（超管 `base` 会被 `require_isolation` 拒绝）；
  本机**无 psql 客户端** → `podman exec base-postgres psql -U base -d base_test -tAc "…"`。
- rootless podman：`export XDG_RUNTIME_DIR=/run/user/$(id -u)`。
- 复用 target：`export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target`。
