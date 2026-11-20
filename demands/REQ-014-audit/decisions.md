# 决策记录：REQ-014 审计日志

- **需求编号**: REQ-014
- **状态**: rev 1（2026-11-20）
- **更新时间**: 2026-11-20 09:20

> 本文件记录 BA 在写规格时的**自决**（BA 对验收标准、字段名、错误码、默认行为有拍板权）。
> 涉及不可逆事项或与用户原意相关的事，必须回到用户，见 D130~D140 里标「用户裁定」的条目。

## 0. 需求来源披露（dev-guide §9.1 / D122）

**REQ-014 的需求来源是「用户原话」，不是 BA 自补。**

原文（首个会话快照，用户消息）：

> 对，审计和国际化也是需要的。

- 这**一条**用户原话同时支撑 REQ-014（审计）与 REQ-021（国际化）两个子需求；
- 检索全部会话快照，用户消息中提到「审计」的**仅此 1 条**；
- 父需求 REQ-001 的 F1 条目「审计日志」承接本需求，父 AC 为 AC-022。

> 对比：REQ-017（工作流）是 BA 自补，用户从未提及 → 已在 D121 取消。
> 本条与 D122 的要求一致：**开工前披露来源**。

---

## D130: 哈希链**按租户分链**

- **决策**：`prev_hash` 取**同租户**上一条记录的 `hash`；每个租户的第一条记录
  其 `prev_hash` 为 **genesis**（64 个字符 `0`）。链与租户**一一对应**。
- **理由**:
  1. 审计表启用 RLS（D132），应用角色在查询时**只能看见本租户**的行；
     若用全局单链，每次写审计都要读「上一条」——在 RLS 下要么读不到（读不到就得
     提权读全局，破坏隔离），要么读到的是**本租户的最后一条**（那本来就是分链语义）。
  2. 全局单链意味着租户 B 的写入依赖租户 A 的写入历史 —— **跨租户耦合**，
     A 的高频写入会把 B 的链拉长，且 A 侧故障会影响 B 的写入。
  3. 分链后「篡改某租户某条记录」的影响**限于该租户**，爆炸半径可控。
- **代价**：无法用一条链证明「全平台审计未被整体替换」。这是**明确的取舍**，
  已记入 [tech-debt.md](../../plan/tech-debt.md)。
- **替代方案（已否）**：全局单链 + 提权读。否 —— 破坏租户隔离，违反 ADR-0001 D5。

## D131: 规范化事件体的**精确定义**（不留给实现自由发挥）

`api-conventions v1` C12 只写了 `hash = sha256(prev_hash + 规范化事件体)`，
**没有定义「规范化」**。不定义的后果：Dev 与 QA 各自实现一套规范化 →
链校验永远失败，或者两边都「算对了自己那套」→ 假绿。

- **决策**：规范化 = 对**固定顺序**的 10 个字段做**长度前缀**拼接，再与 `prev_hash` 拼接。

  ```
  canonical_fields = [
      tenant_id, actor_id, action, object_type, object_id,
      before, after, ip, user_agent, created_at_rfc3339_utc_micros
  ]
  canonical = concat(
      prev_hash_bytes,
      for f in canonical_fields: len(f).to_string() + ":" + f
  )
  hash = hex(sha256(canonical))
  ```

  - `len(f)` 是**字节长度**（不是字符数）；
  - `before` / `after` 是**已规范化的 JSON 文本**（键排序、无多余空白），
    空值为**空字符串**（不是 `"null"`、不是 `"{}"`）；
  - `created_at` 是 **UTC、RFC3339、微秒精度**（如 `2026-11-20T01:02:03.456789Z`）；
  - `prev_hash` 为 hex 文本（genesis 时是 64 个 `0`）。
- **长度前缀**而不是 `\x1f` 分隔：分隔符方案在字段本身含分隔符时有歧义
  （`before` 是任意 JSON，完全可能含 `\x1f`）→ 哈希不可复现。
- **配套要求**：`before` / `after` 的 JSON 规范化必须**由平台提供函数**完成
  （键排序 + 紧凑输出），**不得**依赖 `serde_json` 的 `Map` 迭代序
  （`preserve_order` feature 会打开时序不保证）。

## D132: 审计表**启用 RLS**（与 SSO 两张表相反）

- **决策**：`platform_audit_log` 与 `platform_audit_archive` **启用 RLS**，
  隔离维度为 `tenant_id`。
- **理由**：REQ-013 的两张 SSO 表**不开** RLS，是因为「写入/读取时都还没有权威的
  认证上下文」。审计表**没有**这个问题：
  - 写入时租户已知（登录场景由登录请求携带；登录**失败**时租户由请求的租户标识确定，
    此时 `actor_id` 为空但 `tenant_id` 已知）；
  - 读取时一定有认证上下文。
- **配套**：写入走 `SECURITY DEFINER` 函数（沿用 D56 的平台级写模式），
  使平台 sink 能以**单一代码路径**写入任意租户而不必给应用角色跨租户写权限。

## D133: append-only 用 **REVOKE** 实现；归档走受控 `SECURITY DEFINER` 函数

- **决策**：
  1. 审计表与归档表：`GRANT SELECT, INSERT`，
     **`REVOKE UPDATE, DELETE`**（对应用角色组）→ 表在**权限层面**只允许 INSERT；
  2. 归档不在应用角色手里做「DELETE + INSERT」，而是由一个
     `SECURITY DEFINER` 函数执行「把超期行搬到归档表」这一件事。
- **理由**：AC-123 / AC-S-131 要求**用应用角色验证**。若归档靠应用角色直删，
  就必须给它 DELETE 权限 → append-only 立刻破功。
  「受限的特权函数」是唯一同时满足两者的做法（D56 同款模式）。
- **注意**：REVOKE 之后，表 owner 仍可改 —— 这是 PostgreSQL 的固有行为。
  AC-123 明确要求**用应用角色**验证，不用 owner。

## D134: 敏感键在写入前**硬拒绝**（不是静默剥离）

- **决策**：`before` / `after` 的 JSON 若含敏感键（`password` / `secret` / `token` /
  `credential` / `private_key` / `api_key` 等，大小写与连字符变体一并匹配），
  **写入直接失败**并返回明确错误码；**不做**「悄悄把该字段删掉再写」。
- **理由**：
  1. 静默剥离会让调用方以为「已经记下来了」，实际丢了信息；
  2. 静默剥离也**不保证**没漏（键名变形、值里嵌 token）；
  3. 硬拒绝让错误用法**在类型/流程上暴露**，符合 D27 的思路。
- **配套**：敏感键扫描器必须带**阳性对照**（AC-S-133 步骤 4）。
  🔑 **基线键集合已由 D156 钉死为 14 个**（本条原写的「等」已作废）。
- **注意**：这**不是**万能防护 —— 它挡不住「把密码塞进一个叫 `value` 的键」。
  真正的兜底是「调用方不把凭据放进去」。本条只做**降低概率 + 早暴露**。

## D135: 通用审计类型放 `platform-core`；既有 recorder **不破坏**，用适配器桥接

- **决策**：
  1. `platform-core` 新增**通用**审计事件类型（带 payload）+ sink trait；
  2. `platform-org` 的 `AuditRecorder`（绑死 `OrgAuditEvent`）与
     `platform-iam` 的 `SsoAuditRecorder`（绑死 `SsoAuditEvent`）
     **保持原签名不变**；
  3. 用**适配器**把两者桥接到平台 sink。
- **理由**：
  - REQ-008 / REQ-013 已合并，改它们的 trait 签名会连带破坏既有测试
    （`platform-iam` 129 条用例、`platform-org` 的 real-db 用例）；
  - `platform-core` **没有** payload（`AuditEventSpec` 只有 `code/action/object_type/description`），
    这是 REQ-013 的 P14 已经踩过的坑 —— **不能重犯**；
  - `platform-org → platform-iam` 已是既成依赖方向，`platform-iam → platform-org` 会**成环**，
    所以通用类型必须落在**最底层的 `platform-core`**。
- **代价**：短期内平台里有 3 个 recorder 概念并存。记入 tech-debt，
  待 REQ-018 之后统一收敛。

## D136: 埋点范围 = 登录（本地 + SSO）+ RBAC 权限/角色 + 配置写入

- **决策**：本轮平台自己**实际埋点**三类：
  1. **登录审计**：本地登录成功/失败、SSO 登录成功/失败（桥接已有事件）；
  2. **权限变更**：授予权限、撤销权限、创建角色、删除角色；
  3. **配置写入**：平台配置的条目写入。
- **理由**：
  - 父 AC-022 的原文是「改权限、导出数据」→ **「改权限」是硬要求**；
  - demand 的「登录审计」是硬要求；
  - 配置写入是「平台级配置变更」的样板，且 REQ-010 已有配置表与写入路径；
  - 这三类**证明统一入口真的能用**，而不只是「有个 trait」。
- **不在本轮**：其它业务模块的写操作 —— 由业务方按统一入口自行接入。
  平台的承诺是「**提供入口**」，不是「替所有模块改一遍代码」
  （否则每次加业务模块都要改平台核心，违反 ADR-0001 D8 的依赖方向）。

## D137: 归档**显式触发**，不引入常驻调度器

- **决策**：归档由 (a) 管理接口 `POST /api/audit/archive` 与
  (b) 命令行 `cargo xtask audit archive` 触发；**不引入**后台定时任务。
- **理由**：
  1. 本仓**没有**调度器基础设施（无 cron、无常驻 worker），
     为归档单独引入一个会把「底座」变重；
  2. 审计归档的频率天然是**天级/周级**，显式触发足够；
  3. 低配机（8 线程 / 7.7G）不接受多一个常驻进程；
  4. 显式触发**可测** —— 定时任务在本仓是「测不到」的（与 D108 的 skip 纪律同类问题）。
- **代价**：没人调用就永远不归档。记入 tech-debt，并在归档接口的响应里
  **明确返回**「上次归档时间 / 下次建议时间」，让运维看得见。

## D138: 查询接口路径与权限点

- **决策**：
  - `GET  /api/audit/events` —— 分页 + 过滤
    （`actorId` / `action` / `objectType` / `objectId` / `from` / `to` / `page` / `size`）
  - `GET  /api/audit/events/{id}` —— 单条详情
  - `GET  /api/audit/verify` —— 链校验（返回 `ok` / `checked` / `broken`）
  - `POST /api/audit/archive` —— 触发归档
  - 权限点：`audit:events:read`、`audit:verify:run`、`audit:archive:run`
  - 前端路由：`/admin/audit`
- **理由**：
  - 路径沿用既有约定（`/api/rbac/roles`、`/api/org/...`），权限点沿用
    `模块:资源:动作` 三段式（既有 `sandbox:demo:read`、`sandbox:demo:write`）；
  - 分页参数用 `page` / `size`（1 起），`size` 上限 **200**；
  - `from` / `to` 为 RFC3339 UTC 时间戳。

## D139: 「数据导出审计」不在本轮

- **决策**：父 AC-022 的「导出数据」这一支**不在 REQ-014 交付**。
- **理由**：导出能力本身**尚未交付**（属 REQ-018 / 报表域）。
  审计一个不存在的操作没有意义。统一埋点入口（D135）本轮交付后，
  导出模块落地时自行接入即可 —— 这正是「底座」的交付方式。
- **记录**：这是**明确的范围收窄**，不是遗漏。

## D140: `RequestContext` 增加 `client_ip` 与 `user_agent`

- **决策**：`platform-core` 的 `RequestContext` 增加两个字段
  `client_ip: Option<String>` 与 `user_agent: Option<String>`，
  配套 `with_client_ip()` / `with_user_agent()` 构造器；默认 `None`。
- **理由**：契约 C12 要求审计记录含 `ip` / `user_agent`，
  而 `RequestContext` 当前**两者都没有**（实测：`crates/platform-core/src/context.rs:20`
  的字段只有 `tenant_id` / `space_id` / `user_id` / `trace_id` / `entered_at`）。
  审计 sink 需要一个稳定的取值来源。
- **兼容性**：字段私有 + 构造器新增 → **既有代码不受影响**，
  既有测试不需要改（`RequestContext::new()` 的默认值不变）。
- **注意**：这**不是** `tenant_row!` 那种「加 struct 字段却不改 SELECT」的运行期坑
  （那个坑在 DB 行映射上，这里不涉及）。但 `RequestContext` 若将来被序列化到别处，
  需要同步检查 —— 记入 tech-debt 的检查项。

---

## D141: `before` / `after` 用 **TEXT** 存规范化 JSON 文本，哈希覆盖**存储的那段文本**

- **决策**：`platform_audit_log.before` / `.after` 的类型是 **`TEXT`**（不是 `JSONB`），
  内容是**规范化后的 JSON 文本**；无变更时为 `NULL`。
  哈希计算覆盖的是**存进列里的那段文本本身**，**不是**重新序列化的结果。
- **理由**：
  1. 若列是 `JSONB`，校验方要复算哈希就必须**重新实现一遍 JSON 规范化**
     （键序、数字表示、字符串转义、Unicode 转义……）。
     规范一旦差一个字节（`1.0` vs `1`、`\u4e2d` vs `中`），哈希就永远对不上 →
     **「链校验失败」与「实现有 bug」无法区分**，这正是要避免的假绿。
  2. 存 TEXT 后，校验方**读回那段文本直接参与哈希** → **不需要重新实现任何序列化规则**
     → 测试与生产在「哈希公式 + 链连接」上是**真独立**的。
  3. 审计查询的过滤维度是 `actor` / `action` / `object_type` / `object_id` / 时间，
     **不按 `before`/`after` 的内容检索** → 失去 JSONB 的索引能力**没有实际损失**。
- **代价**：需要在检索 `before`/`after` 内容时写 `before::jsonb` 表达式（Postgres 侧转换，
  不影响存储与哈希）。记入 tech-debt。
- **配套**：生产侧**仍然**要做 JSON 规范化（键排序 + 紧凑），
  但那是**写入时的责任**，不参与校验链的正确性论证。

## D142: 规范文本的**精确格式**钉死（不留给实现自由发挥）

D131 定义了拼接规则，但两处细节没定死，会导致「实现A 与 实现B 算出不同哈希」：

1. **时间戳文本** —— 精确为 `YYYY-MM-DDTHH:MM:SS.ffffffZ`
   - 年 4 位、**月/日/时/分/秒各 2 位零填充**；
   - 小数部分**恰好 6 位**（微秒），不足补 0；
   - 结尾是**字面量 `Z`**（不是 `+00:00`、不是 `+0000`）。
2. **JSON 文本** —— 精确为：对象的键按 **UTF-8 字节序升序**排序（**递归**到每一层）、
   **无任何多余空白**、非 ASCII 字符**不转义**（按 UTF-8 原样输出）、
   控制字符用 JSON 标准短转义（`\n` `\t` `\r` `\b` `\f`）其余用 `\u00XX`。
3. **空值规则** —— `before` / `after` 为 `NULL` 时，**哈希里用空字符串**（零字节），
   不是字符串 `"null"`、不是 `"{}"`、不是 `"null"` 四个字符。

> 🔑 **为什么必须钉死到「字节」**：哈希是对字节算的。
> 「键排序」不够（还要说按什么序），「紧凑输出」不够（还要说数字怎么写、
> Unicode 转不转义）。差一个字节 → 链校验永远失败 → **「实现有 bug」与「规范不一致」
> 无法区分** → 假绿。

## D143: `created_at` 由数据库生成，同租户**严格递增**

- **决策**：`created_at TIMESTAMPTZ NOT NULL`，由写入函数内的
  `clock_timestamp()` 生成（**不用 `now()`** —— `now()` 是事务开始时间，
  同一事务里的多条审计会拿到**同一时刻**，链的时间序就废了）。
  写入函数**先取同租户最后一条并加锁，再取时间** → 同租户 `created_at` 严格递增。
- **理由**：
  1. 校验方按 `created_at ASC` 排序重算链；若插入顺序与时间序不一致，
     校验会**误报**「链断裂」。
  2. `now()` 在单事务多事件下会撞时间戳（批量导入场景真实存在）。
  3. 时间戳由**数据库**生成 = 权威、不可由应用伪造。
- **代价**：`clock_timestamp()` 依赖系统时钟；NTP 回拨理论上会破坏单调性。
  概率极低，且**真发生了也只是链校验告警**，不是数据丢失。记入 tech-debt。

---

## D144: 时间戳格式**必须手工拼装**，不得用 `time` 的 `Rfc3339`，也不得为此开 `time` 的新 feature

- **决策**：规范时间戳文本由**手工格式化**产生，形如
  `format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z", …)`；
  **禁止**使用 `time::format_description::well_known::Rfc3339`；
  **禁止**为此开启 `time` 的 `macros` 或 `parsing` feature。
- **理由**（实测）：
  1. `time` 的 `Rfc3339` 用 `Subsecond::OneOrMore` —— 微秒为 0 时**整个小数部分被省略**
     （输出 `2026-11-20T01:02:03Z`），**违反 D142 的「恰好 6 位」**。
     而哈希是对字节算的 → 同一时刻的记录会因「微秒是否恰好为 0」产出两种文本
     → 规范不稳定。
  2. 根 `Cargo.toml:49` 只给 `time` 开了 `formatting`：
     - `macros` feature **未开** → `format_description!` 宏**不可用**；
     - `parsing` feature **未开** → `format_description::parse()` **不可用**；
  3. 根 `Cargo.toml` 里 `time` 那行**紧邻的注释**写明
     「关掉默认特性里的 macros/json，避免拉进 proc-macro 编译链：低配机编译开销敏感」。
     为一个格式化需求去开 `macros`，等于**推翻本仓为低配机定下的编译成本决策**。
  4. 手工拼装**零 feature 变更、零新依赖**，且格式完全可控。
- **DB 侧对应写法**（已实测，Postgres 17.12）：
  ```sql
  to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
  ```
  实测输出：微秒 456789 → `2026-11-20T01:02:03.456789Z`；
  微秒为 0 → `2026-11-20T01:02:03.000000Z`（**恒 6 位** ✓）。
- **配套**：Dev 必须写一条测试，断言 Rust 侧手工格式化的输出与 DB 侧 `to_char` 的
  输出**逐字节相同**（含微秒为 0 的边界）。

---

## rev 2（2026-11-20 10:10）：QA 挑刺 36 条的**逐条裁决**

> QA 产出：[qa-req014-spec-review.md](../../feature-meta/qa-req014-spec-review.md)
> （36 条挑刺 / 12 条阻塞）+ 120 条用例（`test-cases/`）。
> **13 条 AC 全部有问题，无一条可原样实现并验收。**
> 以下逐条裁决 —— **BA 对「不可判定 / 物理不可达 / 口径不一致」的最终拍板**。

### 裁决总览

| 挑刺 | 档位 | 裁决 | 决策 |
|---|---|---|---|
| S-01 | 口径不一致 | **接受** | D146 |
| S-02 | 口径不一致 | **接受**（跟契约 C7） | D145 |
| S-03 | 主体不存在 | **接受** | D147 |
| S-04 | 物理不可达 | **接受** | D148 |
| S-05 | 物理不可达 | **接受** | D149 |
| S-06 | 主体不存在 | **接受** | D150 |
| S-07 | 不可判定 | **接受** | D151 |
| S-08 | 口径不一致 | **接受** | D152 |
| S-09 | 口径不一致 | **接受** | D153 |
| S-10 | 主体不存在 | **接受** | D154 |
| S-11 | 不可判定 | **接受** | D155 |
| S-12 | 不可判定 | **接受** | D164 |
| S-13 | 不可判定 | **接受** | D164 |
| S-14 | 口径不一致 | **接受** | D165 |
| S-15 | 不可判定 | **接受** | D163 |
| S-16 | 不可判定 | **接受** | D159 |
| S-17 | 口径不一致 | **接受**（手写 policy，路线 B） | D166 |
| S-18 | 不可判定 | **接受** | D167 |
| S-19 | 主体不存在 | **接受** | D156 |
| S-20 | 不可判定 | **接受** | D156 |
| S-21 | 口径不一致 | **接受** | D156 |
| S-22 | 表述 | **接受**（统一用 `tenant-a`/`tenant-b`） | D168 |
| S-23 | 表述 | **接受**（响应字段名写进 AC） | D167 |
| S-24 | 不可判定 | **接受**（检索面 = 全列） | D167 |
| S-25 | 不可判定 | **接受**（QA 的锁法分析正确） | D161 |
| S-26 | 口径不一致 | **接受**（事件码改用命名法） | D163 |
| S-27 | 不可判定 | **接受**（在查询层验「不缺列」） | D167 |
| S-28 | 表述 | **接受** | D168 |
| S-29 | 不可判定 | **接受** | D157 |
| S-30 | 不可判定 | **接受** | D163 |
| S-31 | 不可判定 | **接受**（补创建/删除角色） | D163 |
| S-32 | 表述 | **接受** | D168 |
| S-33 | 口径不一致 | **接受**（忽略 + warn） | D162 |
| S-34 | 口径不一致 | **接受** | D158 |
| S-35 | 口径不一致 | **接受**（服务端分页） | D145 |
| S-36 | 口径不一致 | **接受**（选项 A：薄封装） | D160 |

**全部 36 条接受**，无一条驳回。

---

## D145: 查询接口**遵循契约 C7**（游标分页 + `filter[...]` + `sort`）

- **决策**：AC-S-130 与 design §5.1 全部改为契约 C7 的口径：
  - **游标分页**：`?cursor=<token>&limit=<n>`，默认 `limit=20`，**上限 200**，
    响应含 `next_cursor`（无下一页时为 `null`）；
  - **兼容页码**：`?page=N&page_size=M`；与 `cursor` **互斥**，同时给 → **400**；
  - **排序**：`?sort=-created_at,actor_id`，白名单 = `created_at` / `actor_id` /
    `action` / `object_type` / `object_id`，未声明字段 → **400**；
  - **过滤**：`?filter[actorId][eq]=xxx`，`op` 白名单 = `eq` / `ne` / `in` /
    `contains` / `range`；
  - **数据范围过滤由平台自动叠加**（C7 原文），审计侧不得自行实现绕过开关。
- **理由**：
  1. **契约是平台级约束**。审计查询是平台 API，不能自己另立一套分页口径 ——
     否则前端要同时支持两套，且「什么算合法的分页请求」在平台内有两种答案。
  2. C7 给出的理由（深翻页在变动数据集上会漏数据或重复）对审计表**更成立**：
     审计表**持续追加**，按页码翻页必然出现重复/遗漏。
  3. 我原来写 `page` + `size`（默认 50）时**只读了契约 C12，没读 C7** ——
     这正是 dev-guide §9.11 的老毛病（**「我记得契约是这样」不是证据**）。
- **附带**：前端审计页**必须服务端分页**，不得照抄既有受保护页的客户端分页（S-35）。

## D146: 链校验 = **逐条独立重算 + 接续校验**（两段式，两个 reason 都有意义）

- **决策**：校验分两段，**两个 reason 都有真实用例**：
  1. **逐条重算**：用**本条自己的 `prev_hash` 列** + 本条内容重算 `hash`，
     与存储值比 → 不符 → `hash_mismatch`；
  2. **接续校验**：本条的 `prev_hash` 与**同租户上一条的存储 `hash`** 比 →
     不符 → `prev_hash_mismatch`。
- **理由**：我原来的 AC-S-125 案例 B 写「第 3 条 `reason == hash_mismatch`」是**错的**：
  第 3 条的 `prev_hash` 列没被改 → 它**自身**的重算值**等于**存储值 →
  它不是 `hash_mismatch`，断的是**接续关系** → 应为 `prev_hash_mismatch`。
  「这证明链是连锁的」这个**论证没错**，错的只是 reason 取值。
- **收益**：两段式下，**篡改 `hash` 列**会同时触发第 2 条的 `hash_mismatch`
  与第 3 条的 `prev_hash_mismatch` —— 两个 reason 都能被真实触发，没有死值。

## D147: 部署种子脚本**必须**为审计表加例外块

- **决策**：`deploy/seed/02-app-role.sql` 必须新增与 `platform_config` 同款的例外块：
  ```sql
  DO $$
  BEGIN
      IF to_regclass('public.platform_audit_log') IS NOT NULL THEN
          EXECUTE 'GRANT SELECT, INSERT ON platform_audit_log TO base_platform_app';
          EXECUTE 'REVOKE UPDATE, DELETE ON platform_audit_log FROM base_platform_app';
      END IF;
      IF to_regclass('public.platform_audit_archive') IS NOT NULL THEN
          EXECUTE 'GRANT SELECT, INSERT ON platform_audit_archive TO base_platform_app';
          EXECUTE 'REVOKE UPDATE, DELETE ON platform_audit_archive FROM base_platform_app';
      END IF;
  END
  $$;
  ```
- **理由**：QA **已实测**（临时表，已清理）：
  `base_platform_app=arwd` → 迁移 REVOKE → `=ar`（append-only 成立）
  → **重跑种子脚本的 blanket GRANT** → 回到 `=arwd` → `base_app` 的 `UPDATE`
  **成功执行**（无 permission denied）。
  种子脚本 `:50` 是 `GRANT … ON ALL TABLES IN SCHEMA public`，
  `:69-70` 是 `ALTER DEFAULT PRIVILEGES … GRANT … UPDATE, DELETE`。
  迁移里的 REVOKE **只解决当前状态**，解决不了**重跑种子**。
- **危险点**：**Dev 第一次跑是绿的，补建脚本之后才红** —— 极易被当成环境问题放过。
  → AC-S-123 必须加一步「**重跑种子脚本后仍拒**」。

## D148: RLS 验证必须在**单个事务内** `set_config` → 查询

- **决策**：AC-S-126 的步骤 2/3 改成「**单事务内** `set_config('app.tenant_id', …, true)`
  → 查询 → 提交」，**不得**用裸连接「设一次再查」。
- **理由**：`app.tenant_id` 是**事务局部**的（`set_config` 第三参数 `true`
  = 等价 `SET LOCAL`，见 `crates/platform-db/src/tenant.rs:28`）。
  裸连接单条执行后事务即结束 → 值被清 → 查到 0 行 → 「查不到别的租户」**恒真** → 假绿。
- **追加验收**：**未设**租户上下文时查询 → **0 行且不报错**（fail closed）。

## D149: `ip` / `user_agent` 的来源与**可信度声明**

- **决策**：
  - 取值来源：`x-forwarded-for` 的**第一跳**（与 `crates/platform-iam/src/http.rs:267-269`
    的既有惯例一致）；无该头 → **空串**；`user-agent` 同理。
  - **谁填**：平台中间件（在 `platform-server` 的 middleware 层）负责把
    `x-forwarded-for` / `user-agent` 填进 `RequestContext.client_ip` / `.user_agent`。
  - AC-S-127 的步骤**显式带上** `X-Forwarded-For: 203.0.113.9`，并断言**逐字相等**。
- **必须如实声明**：`ip` 是**客户端可控的头**，本仓**无法记录可信的物理来源**。
  规格与代码注释都**不得**把它写成「来源 IP 即真实客户端地址」。
  它的用途是**关联与排查**，不是**取证**。
- **冲突消解**：AC-S-122 说「取不到落空串」，AC-S-127 说「均含非空」——
  不矛盾：**AC-S-122 是默认行为，AC-S-127 是带头的情形**。两条都保留，但 AC-S-127
  必须显式带头。

## D150: sink 的**装配路径**（AC-S-129 的主体）

- **决策**：装配路径固定为：
  `platform-server` 构造 `PgAuditSink`（持有 `PgPool`）→
  以 `Arc<dyn AuditSink>` **向下注入** `platform-iam` / `platform-rbac` /
  `platform-config` / `platform-org` 的 service；
  各模块**自己**实现「本模块事件 → 通用 `AuditEvent`」的转换（不引 `platform-audit` 的类型）。
- **理由**：`AuditSink` trait 在 `platform-core`（零依赖）→ 各业务模块只依赖 core，
  **不成环**；落库实现 `PgAuditSink` 在 `platform-audit`（依赖 db）→
  由 `platform-server` 装配。
- **AC-S-129 前置补**：「sink **已被注入**」。

## D151: 哈希复算的时间戳格式（**给出确切 Rust 代码**）

- **决策**：复算用的时间戳文本 =
  `format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z", …)`，
  且**必须先转 UTC**（`to_offset(time::UtcOffset::UTC)`）。
- **禁用**：仓里现成的两个 helper —— `crates/platform-core/src/money.rs:484`
  的 `format_rfc3339_utc` 与 `crates/platform-org/src/http.rs:248` 的
  `format_timestamp` —— **都只有秒精度**，复用即**每条都对不上**。
- **理由**：`format_description` 不含偏移分量，直接格式化 `OffsetDateTime`
  会打印**本地时**的钟点再贴个 `Z` —— 差几小时，链永远「校验失败」。
  仓里三处惯例都是先 `to_offset(UTC)`。

## D152: 「业务模块不写审计表」的**扫描口径**

- **决策**：AC-S-129 步骤 4 拆成两个断言：
  1. 扫 `crates/**/*.rs` → `INSERT INTO platform_audit_log` **0 命中**；
  2. 扫 `migrations/**/*.sql` → **恰好 1 处**（在平台自己的迁移里）。
- **理由**：写入走 `SECURITY DEFINER` 函数 → `INSERT` 在 **plpgsql 函数体内**，
  **Rust 源码里根本不会出现** → 原来「只有 sink 模块命中」的判据在 Rust 侧**恒为 0**
  → 又是假绿。扫描器**必须有阳性对照**（TC-014-073）。

## D153: 查询参数的**非法值 vs 超上限**

- **决策**：
  - **非数值**（`limit=abc`）/ **零值**（`limit=0` / `page=0`）→ **400**
    `AUDIT_INVALID_FILTER`；
  - **超上限**（`limit=1000`）→ **夹到 200**（C7 的上限）；
  - 非法值与超上限**是两条不同的路径**，不得混成一条。
- **响应体形态**：必须是 `application/problem+json` + `ProblemDetails`
  （契约 C6）。🔑 axum 0.8 的 `QueryRejection` 是**纯文本** 400 → 违反 C6。
  → **设计要求**：数值参数**自解析**（沿用既有惯例
  `Query<HashMap<String, String>>`，见 `crates/platform-iam/src/http.rs:456`），
  自行校验并返回 `ProblemDetails`。

## D154: 「足够旧的记录」只能**管理员直连 INSERT** 构造

- **决策**：AC-S-131 的前置改为「旧记录由**管理员直连 INSERT** 构造，
  **不参与链校验断言**」。写入函数固定用 `clock_timestamp()`（D143），
  **没有**指定 `created_at` 的入口 —— 这是有意的（时间戳由数据库生成 = 权威）。
- **代价**：旧记录的 `hash` / `prev_hash` 需测试侧自行计算，
  或用 admin 绕过链的自洽值。归档只关心「哪些行被搬走」，不重算链。

## D155: 查询排序 = `created_at DESC, id DESC`

- **决策**：默认排序 `ORDER BY created_at DESC, id DESC`；
  `?sort=` 的白名单 = `created_at` / `actor_id` / `action` / `object_type` /
  `object_id`（C7 格式，多字段逗号分隔，前缀 `-` 表示降序）。
- **理由**：`created_at` 并列时，页码/游标分页会**重复或遗漏** →
  AC-S-132 的「翻页后记录不重复」**判不出红绿**。`id` 作 tiebreaker 兜底。

## D156: 敏感键 —— **14 键基线 + 完整键名匹配 + 递归 + 两侧都做**

- **决策**：
  - **基线键（14 个，恰好）**：
    `password` / `passwd` / `pwd` / `secret` / `client_secret` / `token` /
    `access_token` / `refresh_token` / `id_token` / `authorization` /
    `credential` / `api_key` / `private_key` / `cookie`
  - **归一化**：键名转小写，并把 `-` 视为 `_`（`client-secret` ≡ `client_secret`）。
  - **匹配方式**：归一化后与基线键**精确相等**（**不是子串**）——
    否则 `secretary` / `tokenizer` 会被误伤，业务方会绕开防护。
  - **递归**：检查 JSON 的**每一层**（DB 侧 `jsonb_each_text` **不递归** → 必须自己递归）。
  - **放在哪**：**两侧都做** —— DB 函数内做（权威，真正的「写入前」）
    + Rust 侧提供 `scan_sensitive_keys(&str) -> BTreeSet<String>`
    （供测试 / 工具 / 提前拦截用）。
  - **一致性**：两侧的键集合**必须逐项相等** → 加一条**一致性测试**
    （从迁移 SQL 抽出 DB 侧键数组，与 Rust 常量比对），防止两边漂移。
- **理由**：D134 只写了「等」→ 集合留空 → AC-S-133 **不可判定**。

## D157: 审计写入失败时，**拒绝登录**（503）

- **决策**：审计 sink 写入失败（DB 不可用等）→ **拒绝登录**并返回 **503**
  （新错误码 `AUDIT_SINK_UNAVAILABLE`）。
- **理由**：放行 = 「有登录、无审计」→ 父 AC-022 的「完整且不可篡改」承诺被打破，
  而运维从日志里看不出「这次登录没被记录」。**宁可不登录，不可无审计地登录。**
- **代价**：审计表写不动 = 全平台登录不可用。**这是刻意的可用性取舍**，
  记入 tech-debt（缓解手段：审计写入失败率告警 + 降级开关，**本轮不做**）。

## D158: 归档**单租户**；xtask 用**应用角色**

- **决策**：
  - `POST /api/audit/archive` 的作用域 = **当前租户**（受 RLS 约束），
    不提供全平台归档；
  - `cargo xtask audit archive --tenant <id>` 用**应用角色**连接
    （`base_app`，受 RLS 约束）→ 逐租户调用。
- **理由**：全平台归档需要跨租户特权 + 独立权限点 → 本轮不做；
  审计数据是租户级资产，跨租户批量操作应由部署侧的循环逐租户发起。

## D159: 归档后的链校验语义 + **新增链状态表**

- **决策**：新增第三张表 **`platform_audit_chain_state`**：
  | 列 | 说明 |
  |---|---|
  | `tenant_id` | PK |
  | `head_hash` | 该租户链的**当前基准**（genesis，或上次归档的尾 hash） |
  | `archived_at` | 最近一次归档时间；未归档为 `NULL` |
  | `updated_at` | 更新时间 |
  - 写入时更新 `head_hash` = 新记录的 `hash`；
  - 归档时更新 `head_hash` = **被搬走那段的尾 `hash`**、`archived_at` = 归档时间；
  - 校验时：在线表**第一条**的 `prev_hash` 必须等于 `head_hash`（接续校验的基准）。
- **理由**：不存基准的话，归档后在线表第一条的 `prev_hash` 指向**已被搬走**的那条 →
  接续校验**必然误报断裂** → 归档一次就「链坏了」。

## D160: C-5 契约检查 —— 在 `platform-audit/src/` 放**薄封装**（选项 A）

- **决策**：在 `crates/platform-audit/src/` 暴露薄封装函数，
  内部调用 **core** 里的鉴权入口：
  ```rust
  // crates/platform-audit/src/authz.rs
  pub fn require_events_read(ctx: &RequestContext) -> Result<()> {
      platform_core::extension::permission::require_permission(ctx, "audit:events:read")
  }
  ```
  `platform-server/src/audit.rs` 调这些封装。
- **理由**：C-5 契约检查 `check_c5_for(manifest_path, source_dir)` **只扫该 manifest
  所属 crate 自己的 `src/`**。HTTP 在 `platform-server`、manifest 在 `platform-audit`
  → 对 `platform-audit` 跑 C-5 会报「3 个已登记权限点**没有鉴权调用点**」→ **违例**。
  薄封装同时满足：C-5 命中字面量 ✓、**不**引入 `platform-audit → platform-rbac` ✓
  （鉴权入口在 **core**）。
- **不选选项 B**（在基线快照里登记成新债）—— 新 crate 不应该再开一个同 `platform-org`
  的缺口。

## D161: 并发防分叉 —— **禁用 `FOR UPDATE` 方案**，改用 advisory lock + **新语句重读**

- **决策**：写入函数内用
  `pg_advisory_xact_lock(hashtext('audit:' || p_tenant_id))`
  串行化同租户写入，**拿到锁后用新语句重读**最后一条。
- **明确排除**：`SELECT … ORDER BY created_at DESC LIMIT 1 FOR UPDATE` 锁上一条。
- **理由**（QA 的分析正确）：PostgreSQL 在 **READ COMMITTED**（sqlx 默认）下，
  `FOR UPDATE` **不会**扩展语句快照去看**新插入的其它行**：
  - 事务 A 锁住第 N 条，插入第 N+1 条（`prev_hash = hash(N)`），提交；
  - 事务 B 的 `SELECT … FOR UPDATE` **快照早于 A 的提交** → 它拿到第 N 条的锁，
    但**看不到** A 新插入的第 N+1 条 → 仍算出 `prev_hash = hash(N)`
    → **两条 `prev_hash` 相同 → 链分叉 → 校验必然失败**。
  - 正确手法是「**先拿锁，再在新语句里重读**」，让后到者**看到**先到者刚插入的那一行。
- **验收**：AC-S-124 追加一条**并发**验收项 —— 两个独立连接**同时**向同一租户写 2 条 →
  都成功；排序后第 2 条的 `prev_hash` == 第 1 条的 `hash`；两条 `prev_hash` **互不相同**；
  链校验 `ok == true`。

## D162: `tenant_id` 查询参数 = **忽略 + warn 日志**

- **决策**：查询接口**不接受**来自查询参数的租户；传了**忽略**并记一条 **warn** 日志
  （攻击尝试可观测），**不** 400。
- **理由**：400 会把「该参数不被支持」这个信息暴露给探测方；忽略 + warn 兼顾
  可观测与不泄露。租户只来自认证上下文 + RLS（D132）。

## D163: 事件码 / `object_type` / `before`·`after` 形状 —— 全部钉死

- **决策**：
  | 事件 | 事件码 | `object_type` | `before` / `after` 形状 |
  |---|---|---|---|
  | 本地登录成功 | `iam.login.succeeded` | `iam_session` | `NULL` / `NULL` |
  | 本地登录失败 | `iam.login.failed` | `iam_session` | `NULL` / `NULL` |
  | LDAP 登录 | `iam.login.succeeded` / `iam.login.failed` | `iam_session` | 同上 |
  | SSO 登录成功 | `iam.sso.login.succeeded` | `iam_sso_session` | 同上（沿用既有事件码） |
  | SSO 登录失败 | `iam.sso.login.failed` | `iam_sso_session` | 同上 |
  | 角色创建 | `rbac.role.create` | `role` | `NULL` / `{"permissions":[]}` |
  | 角色删除 | `rbac.role.delete` | `role` | `{"permissions":[...]}` / `NULL` |
  | 授予权限 | `rbac.role.grant` | `role` | `{"permissions":[...]}` / `{"permissions":[...+p-1]}` |
  | 撤销权限 | `rbac.role.revoke` | `role` | `{"permissions":[...+p-1]}` / `{"permissions":[...]}` |
  | 配置条目写入 | `config.entries.write` | `config_entry` | 变更前 / 变更后 |
  | 配置 flag 写入 | `config.flag.write` | `config_flag` | 变更前 / 变更后 |
  - `before` / `after` 的权限集合是**排序后**的字符串数组（确定性）。
  - **事件码命名法**统一为 `<域>.<对象>.<动作>`（既有 `org.node.create`、
    `iam.sso.login.succeeded`）。⚠️ AC-S-122 里我写的 `demo.update` **违反**该命名法
    → 改为 `demo.item.update`（S-26）。
  - AC-S-128 原本只点了「授予 / 撤销」→ **补上**「创建 / 删除角色」（S-31）。

## D164: 告警日志字段名 + 「定期校验」的**执行者**

- **决策**：
  - 告警日志的字段名用 **`event_id`**（= 表列 `id`），并**不要求**「恰好 3 个字段」
    —— 那是过度约束。必含：`tenant_id` / `event_id` / `reason`，**可含更多**。
  - 「定期校验链完整性」的**执行者**：本轮**没有**常驻调度器（D137 同款理由）→
    由 (a) `GET /api/audit/verify` 显式触发 + (b) `cargo xtask audit verify` 触发；
    运维用外部定时任务调用。**「定期」= 由外部调度器周期性调用这两个入口**。
  - 告警 = **error 级别结构化日志** + 校验结果里的 `ok = false`。
- **理由**：AC-S-022 原文写「**定期**校验」但**没指定执行者** → 不可判定。

## D165: 应用连接角色是 **`base_app`**（不是 `base_platform_app`）

- **决策**：AC-S-123 / AC-S-126 的前置一律写 **`base_app`**。
- **理由**：`base_platform_app` 是 **NOLOGIN 的组角色**，**连不上**。
  `base_app` 是 LOGIN、且是 `base_platform_app` 的**成员** → 组权限继承
  （实测 `has_table_privilege('base_app', 'platform_tenant_config', 'UPDATE') = true`
  → 证实继承生效）→ `REVOKE ... FROM base_platform_app` **对 `base_app` 生效** ✓。
- **同时**：ACL 断言必须**用 `base_app`**，用表 owner（`base`）**永远能改** → 测不出任何东西。

## D166: RLS policy **手写**（路线 B）

- **决策**：审计表的 policy **手写**在迁移里（照
  `migrations/20261118000200_tenant_isolation.up.sql:32`），
  **不**走 `cargo xtask tenant-policies` 生成器。
- **理由**：design 内部自相矛盾（§14.5 给了两条路）。选手写是因为：
  1. 形态完全可控，与 D132 的要求逐字对齐；
  2. 生成器的 `tenantScoped.tables` 走的是**逻辑表名**（如 `org_node`），
     审计表的物理名（`platform_audit_log`）与逻辑名的关系尚未验证；
  3. 生成器路径是**运行时**行为，**测不到** → 违反 D108 的 skip 纪律精神。
- **代价**：与生成器机制不一致 → 记入 tech-debt。

## D167: 响应字段名 / 检索面 / 「不缺列」的验法

- **决策**：
  - 响应字段名**写进 AC**（camelCase，契约 C13）：
    `id` / `actorId` / `action` / `objectType` / `objectId` / `before` / `after` /
    `ip` / `userAgent` / `createdAt` / `prevHash` / `hash`；
  - 「检索该租户的全部审计记录**文本**」的检索面 = **全部列的文本拼接**
    （不只是 `before`/`after`）—— 凭据可能落在 `action` / `object_id` 等列；
  - AC-S-122 的「`ip` 与 `user_agent` **存在该列**（不得缺列）」在**查询层**验
    （读回的 JSON 对象里**有**这两个键），而不是在表结构层验。
  - 403 的**具体错误码**钉死为 `INSUFFICIENT_PERMISSION`（既有码，不新增）。

## D168: 夹具命名与契约关系

- **决策**：
  - 夹具租户名统一用 **`tenant-a` / `tenant-b`**（与 REQ-013 一致），
    不用 `t-audit` / `t-other`；
  - AC-S-122 与契约 C12 的关系写明：本需求的 13 个字段**就是** C12 的固定字段，
    **不增不减**；`before`/`after` 的**类型**（`TEXT`）是 C12 未规定处的实现选择。

---

## D169: 审计页**用游标取数**，页码指示用**游标栈**（裁定 Dev-C 的提问）

- **背景**：前端 Dev 按 TC-014-113（请求含 `page=2`、页码指示 1→2）实现了**页码模式**，
  并提出：契约 C7 说游标是主口径，但「游标算不出页数」→ 请 BA 定。
- **决策**：**数据获取用游标**（C7 主口径）；**页码指示用「游标栈」实现**：
  前端在内存里维护一个已访问的 `cursor` 数组（`cursorStack[i]` = 第 i+1 页的入参游标），
  页码指示 = `cursorStack.length`，「上一页」= 弹栈、「下一页」= 取 `nextCursor` 压栈。
- **理由**：
  1. **AC-S-132 步骤 2「翻页后记录不重复」是硬判据**。
     审计表**持续追加**，页码分页在变动数据集上必然出现重复/遗漏 ——
     这正是契约 C7 把游标定为主口径的原话理由。
     → **页码模式无法满足 AC-S-132**，不能因为 UI 好看就换。
  2. C7 的页码路径原文是「**需要页码跳转时**」用 `?page=N&page_size=M` ——
     审计页**不需要跳到任意页**（审计场景通常是「往前翻看」），不在这个前提里。
  3. 游标栈**保住了页码观感**（1→2→3），又**不违反** C7。
- **保留**：前端 `buildAuditQuery` 的**页码模式实现保留**（C7 允许的兼容路径），
  只是**默认不走**。不做删除，避免为「看起来更干净」多写代码、多一个测试面。
- **已知代价**：游标分页**不能跳到任意页**（只能顺序翻）。
  页面上的页码指示应理解为「**本次浏览翻到第几页**」，不是「全量第几页」。
  UI 文案要如实表达（不写「第 3 页 / 共 20 页」这种会被误解的说法）。

## D170: 前端**遗留两项**归入棒 2（接入与接口）

- **决策**：前端 Dev 明确留下两项，**由棒 2 一并做**：
  1. **5 个新错误码的 i18n 本地化** —— 前端 Dev 故意没做
     （会与后端 Dev 撞同一处编辑，且 i18n 测试有「错误域条目数 == 抽样列表长度」的
     精确计数断言，必须两侧同改）；
  2. **E2E 夹具** —— `scripts/seed-e2e.sh` 需加 `u-perm-audit@example.com`
     （授 `audit:events:read`）；翻页用例还需 **≥11 条**审计记录。
     ⚠️ 夹具缺失**必须 fail**，**不得**用 skip 掩盖（D108）。
- **理由**：这两项都不是前端页面本身的事，且都与后端耦合 → 放棒 2 一次做完。

---

## D171: D159 的「2 与 3 互斥」——**采纳 Dev 的实现**，并补 `tip_hash` 列

- **背景**：Dev-A 报 🔴 偏离 1 —— D159 同时说了
  ① `head_hash` = 当前基准（genesis 或上次归档的尾 hash）；
  ② **写入时**更新 `head_hash` = 新记录的 `hash`；
  ③ 校验时在线表**第一条**的 `prev_hash` 必须等于 `head_hash`。
  **② 与 ③ 互斥**：若 ② 成立，接续校验会拿「在线段第一条的 `prev_hash`」（= genesis）
  与「链尖」（= 最新一条）比 → **每写一条都误报断裂**。
  Dev 实测：按 ② 实现，4 条链/并发/篡改用例**全红**（`PrevHashMismatch`）。
- **裁决**：**采纳 ① + ③**（Dev 的实现正确）——
  `head_hash` = 当前**在线段**的基准；**写入只维护 `updated_at`**；**归档才更新 `head_hash`**。
- **追加**（Dev 提出的缺口，本轮补上）：①+③ 的代价是「**链的尖端不落库**
  → **末尾被截断检不出来**」（链内相邻断裂仍能检出）。
  → **`platform_audit_chain_state` 增加 `tip_hash` 列**：
  | 列 | 语义 | 谁更新 |
  |---|---|---|
  | `head_hash` | 在线段的**基准**（genesis 或上次归档的尾 hash） | **只在归档时**更新 |
  | `tip_hash` | 链**尖端**（最新一条的 `hash`） | **写入时**更新；归档时 → `head_hash` |
  - 校验时：接续校验用 `head_hash`；**截断检测**用 `tip_hash`
    （在线表最后一条的 `hash` 必须 == `tip_hash`，否则「尾巴被砍了」）。
- **为什么不违反契约**：`platform_audit_chain_state` 是**本需求自建**的表；
  契约 C12 只钉死 `platform_audit_log` 的 **13 个字段**，与本表无关。
- **本条是 BA 的规格缺陷**（不是 Dev 的）→ 记入本轮复盘。

## D172: 敏感键归一化**追加「驼峰拆词」**规则（修正 D156 的漏网）

- **背景**：Dev-A 报 🟡 偏离 5 —— QA 的 TC-014-107 期望 `{"apiKey":"x"}` **命中**，
  而 D156 的归一化（转小写 + `-`→`_`）把 `apiKey` 变成 `apikey` ≠ `api_key` → **不命中**。
- **裁决**：**采纳 QA 的期望，归一化规则追加「驼峰拆词」**。完整规则（按序）：
  1. 在**小写/数字与随后的大写字母**之间插入 `_`；
  2. 全部转**小写**；
  3. `-` 视为 `_`。
  | 输入 | 归一化 | 是否命中 |
  |---|---|---|
  | `apiKey` | `api_key` | ✅ |
  | `APIKey` | `api_key` | ✅ |
  | `clientSecret` | `client_secret` | ✅ |
  | `api_key` / `api-key` | `api_key` | ✅ |
  | `secretary` | `secretary` | ❌ 不命中 |
  | `tokenizer` | `tokenizer` | ❌ 不命中 |
- **理由**：D156 的目的是「挡不住就等于没挡」。`apiKey` / `clientSecret` 是**极常见**的写法，
  只挡 `api_key` 等于给了一个绕过口。而 `secretary` / `tokenizer` 仍**不**命中 →
  「非子串匹配」这条纪律**不受影响**。
- **同步**：AC-S-133 追加一条断言（`apiKey` 必须命中）。

## D173: 鉴权薄封装的**正确签名**（修正 design §15.2 的写不成的示例）

- **背景**：Dev-A 报 🟡 偏离 2 —— design §15.2 写的
  `require_events_read(ctx: &RequestContext) -> Result<()>` **编译不过**：
  core 的 `require_permission` 是**路由层构造器**（返回 `PermissionLayer`），
  **没有**「传 ctx 直接判定」的形态。
- **裁决**：**采纳 Dev 的实现** —— `pub fn require_events_read() -> PermissionLayer { require_permission("audit:events:read") }`，
  与既有惯例一致（`crates/platform-rbac/src/http.rs:451`）。
  C-5 照样扫得到字面量 ✓，**不影响任何 AC**。

## D174: 归档函数**必须带租户参数 + 显式守卫**（SECURITY DEFINER 绕过 RLS）

- **背景**：Dev-A 报 🟡 偏离 3 —— design §3.6 的
  `platform_audit_archive_move(p_before TIMESTAMPTZ)` **没有**租户参数，
  但 D158 要求归档**单租户**。
- **裁决**：**采纳 Dev 的实现** ——
  `platform_audit_archive_move(p_tenant_id TEXT, p_before TIMESTAMPTZ)`，
  函数内**显式守卫**：`p_tenant_id` 必须与 `current_setting('app.tenant_id', true)` 一致，
  否则 `RAISE EXCEPTION … ERRCODE = 'insufficient_privilege'`。
  `platform_audit_head_hash(p_tenant_id)` 同样带守卫。
- **理由（重要）**：`SECURITY DEFINER` 函数**以 owner 身份执行 → 绕过 RLS**。
  若按 design 的签名实现，就等于给应用一把**跨租户搬数据**的钥匙 ——
  与 D158「单租户」直接矛盾。**守卫是必须的**，不能靠「上层已经过滤了」。

## D175: 排序字段名 —— **camelCase 为规范**，snake_case 为**兼容别名**

- **裁决**：采纳 Dev 的实现：**两种写法都接受**（指向同一列）；
  **未声明**的字段一律 **400 `AUDIT_INVALID_FILTER`**。
- **规范**：以 **camelCase** 为准（契约 C13 的 JSON 字段惯例，acceptance rev 3 用它）；
  snake_case 仅作兼容别名。

## D176: JSON 规范化**只在 Rust 侧做**，DB 侧只做敏感键检查

- **裁决**：**采纳 Dev 的实现** —— Rust 侧 `canonical_json` 产出规范化文本 →
  传给 DB 函数的就是**规范化文本** → DB **原样存储**（哈希覆盖存进去的那段文本，D141）。
- **理由**：D142 要求键序是「UTF-8 **字节序**升序」，而 Postgres 的 `jsonb`
  是按「**长度 + 字节**」排的 → **DB 侧用 jsonb 做不出这个序**；
  用 plpgsql 递归实现 JSON 规范化复杂且易错。
- **代价**（记入 tech-debt）：绕过 Rust 直接调 `platform_audit_append` 时，
  存进去的 JSON **可能不是规范形态** —— 但**哈希仍自洽**（哈希覆盖的就是存进去的那段文本）。

## D177: 批准修改 REQ-013 的那条既有断言（保留原意 + 加阳性对照）

- **背景**：Dev-A 报 🟡 偏离 7 —— `crates/platform-iam/tests/sso_audit.rs` 的
  `ac_s_108_no_sso_audit_table_exists_yet` 断言「库里**没有任何** `*audit*` 表」，
  REQ-014 落地后**必然红**。
- **裁决**：**批准**。改为「没有任何 **SSO 专用**审计表」（排除 `platform_audit_*`），
  并**加一条阳性对照**：平台级通用审计表**应当存在**。
- **理由**：那条断言的**意图**是「不要偷偷建 SSO 专用表」—— 意图**完整保留**，
  只是把「平台级通用审计表」从「不存在」改成了「存在」。
  这是**需求演进导致的必然修改**，不是放宽判据。

## D178: 4 个配置项的登记归**棒 2**

- **裁决**：`audit.retention.online_days` / `audit.retention.archive_days` /
  `audit.page.default_size` / `audit.page.max_size` 的登记（改
  `crates/platform-config/manifest.json` + 加迁移 INSERT）**归棒 2**。
- **理由**：登记配置项要**同时**改 manifest 与迁移，漏 INSERT → **启动失败**（D27）。
  棒 1 的文件清单不含 `platform-config/manifest.json` →
  Dev-A 选择**不引入未登记的配置项**（`platform-audit/manifest.json` 的 `config` 段留空）
  是**正确**的保守做法。

---

## 待用户裁定（**不阻塞开工**，但需在合并前回执）

| 编号 | 事项 | BA 默认 | 影响 |
|------|------|--------|------|
| — | 分链（D130）放弃「全平台单链」的审计强度 | 采纳分链 | 跨租户整体替换不可证 |
| — | 归档显式触发（D137） | 不引入调度器 | 需外部定时任务调用 xtask |

两项均有安全默认，且不影响验收判据 → **先按默认执行**。
