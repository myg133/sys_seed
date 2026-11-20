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

## 待用户裁定（**不阻塞开工**，但需在合并前回执）

| 编号 | 事项 | BA 默认 | 影响 |
|------|------|--------|------|
| — | 分链（D130）放弃「全平台单链」的审计强度 | 采纳分链 | 跨租户整体替换不可证 |
| — | 归档显式触发（D137） | 不引入调度器 | 需外部定时任务调用 xtask |

两项均有安全默认，且不影响验收判据 → **先按默认执行**。
