# REQ-014「审计日志」—— QA 挑刺报告（开工前）

- **审查对象**: `BA/demands/REQ-014-audit/` 的四份文档 —— `demand.md` / `acceptance.md` /
  `decisions.md` / `design-summary.md`（均 rev 1~2，2026-11-20）
- **代码基线**: `code/` @ `develop` @ `0683115` —— **只读核实；未改动任何代码文件**
- **环境**: Arch Linux / 8 线程 / podman `base-postgres` / **只读 + 临时表实测**（临时表已清理）
- **审查者**: QA（myg133）｜**审查时间**: 2026-11-20
- **方法**: 逐 AC、逐 design 章节核对；每条结论都给 `file:line` 或**可复跑的命令与实测输出**。
  实测过的 8 个必查项见 §5。**未跑 `cargo build` / `cargo test`**（需求尚未实现，跑不了；
  低配机 8 线程约束）。
- **配套用例集**: [test-cases/index.md](../demands/REQ-014-audit/test-cases/index.md)
  （**120 条**，13/13 AC 覆盖）
- **分级依据**: dev-guide §9.12 的**五档可判定性**（不是严重程度）；
  级别（阻塞/高/中/低）另标。

---

## 汇总

| 级别 | 数量 |
|---|---|
| **阻塞**（主体不存在 / 物理不可达 / 不可判定） | **12** |
| **高**（口径不一致 / 安全缺口 / 不可执行） | **10** |
| **中** | **10** |
| **低**（措辞 / 格式 / 冗余） | **4** |
| **合计** | **36** |

**按可判定性分档（dev-guide §9.12）**：

| 档位 | 数量 | 编号 |
|---|---|---|
| **主体不存在** | **4** | S-03、S-06、S-10、S-19 |
| **物理不可达** | **2** | S-04、S-05 |
| **不可判定** | **15** | S-07、S-11、S-12、S-13、S-15、S-16、S-18、S-20、S-24、S-25、S-27、S-29、S-30、S-33、S-34 |
| **口径不一致** | **10** | S-01、S-02、S-08、S-09、S-14、S-17、S-21、S-26、S-35、S-36 |
| **表述** | **5** | S-22、S-23、S-28、S-31、S-32 |

**13 条 AC 里，13 条都有问题**（无一条可原样实现并验收）。按 AC 分布：

| AC | 问题 | 其中阻塞 | 结论 |
|---|---|---|---|
| AC-S-022 | S-12、S-13 | 0 | 「定期」无执行者；告警字段名不一致 |
| AC-S-122 | S-26、S-27、S-28 | 0 | 事件命名法与 design 冲突；「存在该列」的层面未指明 |
| AC-S-123 | S-03、S-14 | **1** | **append-only 会被部署脚本静默破功（已实测）** |
| AC-S-124 | S-07、S-22、S-25 | **1** | 复算的时间戳微秒来源未钉死；并发无 AC 覆盖 |
| AC-S-125 | S-01 | **1** | **案例 B 的 `reason` 与 design §5.2 自相矛盾** |
| AC-S-126 | S-04、S-17 | **1** | **步骤 2/3 用裸连接 → 事务局部 → 步骤 3 不可达（已实测）** |
| AC-S-127 | S-05、S-15、S-24、S-29 | **1** | **「非空 ip」按字面不可达（已实测取值来源）** |
| AC-S-128 | S-30、S-31 | 0 | `before`/`after` 形状未定义；AC 漏了创建/删除角色 |
| AC-S-129 | S-06、S-08、S-36 | **2** | **sink 落库主体未定义；扫描口径与设计冲突** |
| AC-S-130 | S-02、S-09、S-11、S-18、S-23、S-32、S-33 | **3** | **与契约 C7 全面冲突；非法参数响应体违反 C6；排序未定义** |
| AC-S-131 | S-10、S-16、S-34 | **1** | **「足够旧的记录」无法通过 sink 构造** |
| AC-S-132 | S-35 | 0 | 需服务端分页，不能照抄既有页 |
| AC-S-133 | S-19、S-20、S-21 | **1** | **扫描器无宿主；敏感键集合留空** |

---

## 阻塞（12）

### S-01: AC-S-125 案例 B 的「第 3 条 `reason == hash_mismatch`」与 design §5.2 **自相矛盾**

- **档位**: 口径不一致 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-125 预期结果第 2 条
- **AC 原文**: 「案例 B：`broken` **恰好 2 条**（第 2 条与**第 3 条**），第 2 条 `reason` == `hash_mismatch`，
  第 3 条 `reason` == `hash_mismatch`」
- **问题**: design §5.2 定义了**两个** `reason`：
  - `hash_mismatch` = 「重算哈希与存储不符」
  - `prev_hash_mismatch` = 「本条的 `prev_hash` 与上一条的 `hash` 不接」
  而 D131 规定重算哈希的输入是**该条记录自己的 `prev_hash` 列**。
  案例 B 只改了第 2 条**存储的 `hash` 值**（其它列不动）→
  第 3 条的 `prev_hash` 列**没被改** → 用它自己的 `prev_hash` 重算 → **重算值 == 它存储的 `hash`**
  → 第 3 条**不构成** `hash_mismatch`；真正断掉的是「接续关系」→ 应为 `prev_hash_mismatch`。
  → **两个 reason 里必有一个是死值**：若按 AC 写成 `hash_mismatch`，`prev_hash_mismatch` 永远不可能出现；
  若按 design 写，第 3 条就不是 `hash_mismatch`。
- **证据**:
  - `BA/demands/REQ-014-audit/design-summary.md:399-402`（两个 reason 的定义）
  - `BA/demands/REQ-014-audit/decisions.md:48-60`（D131 的重算公式，`prev_hash` 是**本条**的输入）
  - `BA/demands/REQ-014-audit/design-summary.md:291`（`before`/`after` 取「存进列里的那段文本」，
    同理 `prev_hash` 也应取本条列里的值）
- **建议改法**（可直接落笔）:
  > - 案例 B：`ok = false`，`broken` **恰好 2 条**（第 2 条与**第 3 条**），
  >   第 2 条 `reason` == `hash_mismatch`（其存储的 `hash` 被直接改写），
  >   第 3 条 `reason` == **`prev_hash_mismatch`**（其 `prev_hash` 列未被改写，
  >   但与第 2 条**当前存储的** `hash` 接不上）。
  >   —— 第 3 条被牵连**证明链是连锁的**，不是逐条独立的校验。
  >
  > 配套口径（写入 design §5.2）：链校验采用「**逐条独立重算** + **接续校验**」两段式：
  > ① 用该条自己的 `prev_hash` 列与各字段重算哈希，与存储的 `hash` 比对 → 不符则 `hash_mismatch`；
  > ② 把该条存储的 `prev_hash` 与**上一条存储的** `hash` 比对 → 不接则 `prev_hash_mismatch`。

### S-02: AC-S-130 与**契约 C7** 在分页/过滤/排序上**五处全面冲突**

- **档位**: 口径不一致 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-130 步骤 1、2、4；design §5.1；decisions.md D138
- **问题**: 契约 C7（`api-conventions v1` §C7）是**平台级分页约定**，REQ-014 的设计与 AC 与它冲突：

| 项 | 契约 C7 | design §5.1 / AC-S-130 |
|---|---|---|
| 分页方式 | **游标分页**（`cursor` + `limit`），响应含 `next_cursor` | **页码分页**（`page` + `size`），响应**无** `next_cursor` |
| 默认条数 | `limit=20` | `size=50` |
| 兼容页码的参数名 | `?page=N&**page_size**=M` | `?page=N&**size**=M` |
| 排序 | `?sort=-created_at,name`，白名单外返回 400 | **无 `sort` 参数** |
| 过滤 | `?filter[field][op]=value`，`op ∈ {eq,ne,in,contains,range}` | 具名参数 `actorId`/`action`/`objectType`/`objectId`/`from`/`to` |

  → **照 AC 实现 = 破平台契约；照 C7 实现 = AC 自己写错。** Dev 只能二选一，
  而验的又是另一套 —— 这是 REQ-013「按错的规格实现 + 按错的规格验收」的自洽陷阱。
- **证据**:
  - `BA/contracts/api-conventions/v1.md:114-124`（C7 原文）
  - `BA/demands/REQ-014-audit/design-summary.md:363-377`（§5.1 查询参数表）
  - `BA/demands/REQ-014-audit/decisions.md:170`（D138「分页参数用 `page`/`size`（1 起），`size` 上限 200」）
  - `BA/demands/REQ-014-audit/acceptance.md:197`（AC-S-130 步骤 1 的 `?page=1&size=2`）
- **建议改法**（BA 二选一，然后同步改 AC + design + 契约）：
  > **选项 A（跟 C7，推荐）**：AC-S-130 改为
  > `GET /api/audit/events?cursor=…&limit=2`，默认 `limit=20`、上限 `200`，
  > 响应含 `items` / `total` / `next_cursor`；过滤改用 `?filter[action][eq]=…` 等；
  > 排序用 `?sort=-created_at`，白名单 `{created_at, action, actor_id, object_type}`，白名单外 400。
  > 兼容页码 `?page=N&page_size=M` 与游标**互斥**，同时给返回 400。
  > （**代价**：AC-S-132 的前端分页要改成游标式「加载下一页」。）
  >
  > **选项 B（跟 design）**：在契约 C7 里为审计接口**显式登记例外**
  > （C7 已有「兼容分页」一档，但没写「哪些模块可以用页码分页」），
  > 并把 C7 改成「页码分页用 `?page=N&page_size=M`」，把 design 的 `size` 改成 `page_size`。
  > （**代价**：契约要改版本，且要说明为什么审计查询不用游标。）

### S-03: append-only 会被**部署种子脚本**静默破功（已实测）

- **档位**: 主体不存在 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-123 前置与步骤 1、2；AC-S-131 步骤 4；design §3.4
- **问题**: design §3.4 只在**迁移**里写了
  `REVOKE UPDATE, DELETE ON platform_audit_log FROM base_platform_app;`。
  但 `deploy/seed/02-app-role.sql` 里有**两句全表授权**：
  - `:50` `GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO base_platform_app;`
  - `:69-70` `ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO base_platform_app;`
  种子脚本**自己**已经知道这个坑 —— `:53-66` 为 `platform_config` 专门做了一个**例外块**
  （注释原文：「它会在**本脚本重跑时**把迁移里的 REVOKE 又盖回去 —— 所以这里必须再收一次」）。
  **审计表没有对应的例外块。**
  而这个脚本是**幂等可重跑**的（`:14`「幂等：可重复执行」），
  `deploy/RUNBOOK.md` 还把「补建命令」列为标准运维动作。
  → **AC-S-123 / AC-S-131 会在「跑过一次补建脚本」的环境里假红，而 Dev 的第一次跑是绿的。**
- **实测证据**（`base_test`，临时表 `qa_probe_t`，已 `DROP`）:

  ```
  after_create        | {base=arwdDxtm/base,base_platform_app=arwd/base}   ← 默认权限自动授了 UPDATE/DELETE
  after_revoke        | {base=arwdDxtm/base,base_platform_app=ar/base}      ← 迁移 REVOKE 后：append-only 成立
  after_seed_rerun    | {base=arwdDxtm/base,base_platform_app=arwd/base}   ← 重跑种子 blanket GRANT：破功
  UPDATE（base_app）  | UPDATE 0                                            ← 无 permission denied → 真的能改
  ```
- **证据（源码）**:
  - `code/deploy/seed/02-app-role.sql:50`、`:69-70`（两句全表授权）
  - `code/deploy/seed/02-app-role.sql:53-66`（**只**为 `platform_config` 做了例外）
  - `code/deploy/seed/02-app-role.sql:14`（幂等可重跑）
  - `code/deploy/RUNBOOK.md:309-310`（「补建命令」是标准运维动作）
  - `BA/demands/REQ-014-audit/design-summary.md:196-209`（§3.4 只写迁移里的 REVOKE）
- **建议改法**（两处都改，缺一不可）:
  > **① `code/deploy/seed/02-app-role.sql`** —— 在 `platform_config` 的例外块之后追加：
  > ```sql
  > -- ⚠️ REQ-014 审计表：append-only（D133）。
  > -- 上面那句 `GRANT … ON ALL TABLES` 会在**本脚本重跑时**把迁移里的 REVOKE 盖回去，
  > -- 所以这里必须再收一次（表还不存在时跳过：首次建库先跑本脚本、后跑迁移）。
  > DO $$
  > BEGIN
  >     IF to_regclass('public.platform_audit_log') IS NOT NULL THEN
  >         EXECUTE 'GRANT SELECT, INSERT ON platform_audit_log TO base_platform_app';
  >         EXECUTE 'REVOKE UPDATE, DELETE ON platform_audit_log FROM base_platform_app';
  >     END IF;
  >     IF to_regclass('public.platform_audit_archive') IS NOT NULL THEN
  >         EXECUTE 'GRANT SELECT, INSERT ON platform_audit_archive TO base_platform_app';
  >         EXECUTE 'REVOKE UPDATE, DELETE ON platform_audit_archive FROM base_platform_app';
  >     END IF;
  > END
  > $$;
  > ```
  >
  > **② `BA/demands/REQ-014-audit/design-summary.md` §3.4** —— 补一句：
  > 「⚠️ **种子脚本会盖回 REVOKE**：`deploy/seed/02-app-role.sql:50` 有 blanket GRANT，
  > `:69-70` 有 DEFAULT PRIVILEGES。审计表必须像 `platform_config` 那样在种子脚本里
  > 有**例外块**，否则重跑脚本后 append-only 失效（AC-S-123 / AC-S-131 会红）。」
  >
  > **③ AC-S-123 补一条验收步骤**：「步骤 3.5：重跑 `deploy/seed/02-app-role.sql` 后，
  > 步骤 1/2 的两条语句**仍然**失败。」

### S-04: AC-S-126 步骤 2/3 用裸连接设 `app.tenant_id` → **步骤 3 不可达**（已实测）

- **档位**: 物理不可达 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-126 步骤 2、步骤 3
- **AC 原文**: 「2. 以应用角色连接，把 `app.tenant_id` 设为 `t-other` 后直接查
  `SELECT * FROM platform_audit_log`；3. 把 `app.tenant_id` 设为 `t-audit` 再查一次。」
- **问题**: `app.tenant_id` 是**事务局部**的（`set_config(..., true)` ≡ `SET LOCAL`），
  事务结束（提交或回滚）即失效。裸连接（autocommit）上单独跑一条 `set_config`，
  下一条查询**看不到**这个设置 → `current_setting('app.tenant_id', true)` 返回 `NULL`
  → policy 求值为 `NULL` → **0 行**。
  → 步骤 2 的期望（「不含 t-audit 的那条」）会**假绿**（0 行当然不含）；
  → 步骤 3 的期望（「**含** t-audit 的那条」）**不可达**。
- **实测证据**（`base_test`，临时表 `qa_rls_t` + RLS policy，已 `DROP`）:

  ```
  A. 裸语句（autocommit，每条各自一个事务）：set_config 之后单独查 → 0 行
  B. 同一事务内：set_config('t-other') → 1 行；set_config('t-audit') → 1 行
  D. 完全不设 app.tenant_id → 0 行（fail closed，不报错）
  ```
- **证据（源码）**:
  - `code/crates/platform-db/src/tenant.rs:28`（`SELECT set_config('app.tenant_id', $1, true); -- true = 事务局部`）
  - `code/crates/platform-db/src/tenant.rs:15-25`（「`SET LOCAL` 只在**当前事务**内有效，事务结束即自动失效」）
  - `code/crates/platform-db/src/tenant.rs:187-190`（实际调用点）
  - `BA/demands/REQ-014-audit/design-summary.md:193-194`（`current_setting(..., true)` 的 `true` 不能省）
- **建议改法**（可直接落笔）:
  > - **步骤 2**：以应用角色连接，在**同一个事务**内执行
  >   `BEGIN; SELECT set_config('app.tenant_id','t-other',true); SELECT * FROM platform_audit_log; COMMIT;`
  > - **步骤 3**：同上，把租户换成 `t-audit`，**同样在单个事务内**。
  > - **补充步骤 4**：以应用角色连接、**不设** `app.tenant_id` 直接查 →
  >   期望**语句成功且返回 0 行**（fail closed，**不报错**）。
  > - 补充说明：`app.tenant_id` 是**事务局部**的（`set_config(..., true)` ≡ `SET LOCAL`），
  >   裸连接上单独设置后另起一条语句会**丢失**该设置 → 必须与查询同事务。

### S-05: AC-S-127 的「三条均含**非空** `ip`」按字面**不可达**

- **档位**: 物理不可达 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-127 预期结果第 4 条
- **AC 原文**: 「上述三条**均含**非空的 `ip` 与 `user_agent`」
- **问题**: 本仓登录侧的 `ip` **唯一来源**是请求头 `x-forwarded-for`
  （取第一跳），**取不到就是空串**（`unwrap_or_default()`）。
  一个普通本地登录请求**不会**带这个头 → `ip == ""` → 「非空」**不可达**。
  同时这与 AC-S-122 的「`ip` … **无值时为空串**，不得缺列」**口径相反**
  （同一条 AC 体系里，一个要求非空、一个允许空串，且没说清条件差异）。
  另外：`ip` 记录的是**客户端可控的请求头** —— 它证明「客户端自称来自哪里」，
  证不了「真的来自哪里」。这不阻塞交付，但规格应如实写明这个上限。
- **证据（源码）**:
  - `code/crates/platform-iam/src/http.rs:265-273`
    （`request_meta`：`ip` 只来自 `x-forwarded-for`，`user_agent` 只来自 `user-agent`，都 `unwrap_or_default()`）
  - `code/crates/platform-iam/src/service.rs:73-80`（`RequestMeta { ip, user_agent, device_label }`，**均为 `String` 非 `Option`**）
  - `code/crates/platform-iam/src/http.rs:592-600`（登录 handler 走 `request_meta(&headers)`）
  - `BA/demands/REQ-014-audit/acceptance.md:58`（AC-S-122：「无值时为空串，**不得缺列**」）
- **建议改法**（可直接落笔）:
  > - **步骤 1/2/3 的请求必须显式带** `X-Forwarded-For: 203.0.113.9` 与 `User-Agent: qa-agent/1.0`。
  > - 预期结果第 4 条改为：
  >   「上述三条的 `ip` 均 **`== "203.0.113.9"`**、`user_agent` 均 **`== "qa-agent/1.0"`**
  >   （**逐字相等**；取值为 `x-forwarded-for` 的**第一跳**）。
  >   ⚠️ 本仓 `ip` 的唯一来源是 `x-forwarded-for` 请求头（**客户端可控**）——
  >   记录的是「客户端自称的来源」，**不是**可信的物理来源。
  >   未携带该头时按 AC-S-122 落**空串**（不是缺列）。」
  >
  > 并在 demand §4 的「含 IP 与 UA」旁补一句同款说明（避免下游模块当成可信 IP 用）。

### S-06: AC-S-129 步骤 1 的「sink」**落库主体**在设计里不存在

- **档位**: 主体不存在 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-129 步骤 1~3；design §2.2 / §2.3
- **问题**: design §2.3 把 `AuditEvent` + `AuditSink` trait 放在 **`platform-core`**（**零依赖**，
  iam/org 只依赖 core → 不成环）。但 `AuditSink::record` 的签名是
  `fn record(&self, ctx: &RequestContext, event: &AuditEvent) -> BoxFuture<Result<()>>`
  —— **core 里没有任何落库实现**（core 只依赖 axum，不依赖 sqlx）。
  真正的落库实现在 `platform-audit` 的 `PgAuditSink`（design §2.2）。
  → AC 步骤 1 说「用平台提供的通用审计事件类型 + sink trait **记录一条事件**」，
  但**没说谁把 trait 的实现装配进去**（`platform-server` 的 `main.rs` 里
  `nest("/api/audit", …)` 那一层，design §10）。
  → 步骤 2/3 更进一步：「把 `platform-org` 已声明的事件送进平台 sink」——
  按 D135，适配器在**拥有者侧**（org/iam）实现，但 org/iam 要依赖 `platform-audit` 才能拿到
  `PgAuditSink`（design §2.2 的依赖图：`platform-org → platform-audit`）→ **成环风险**：
  `platform-audit` 若要构造 `OrgAuditEvent` 就会依赖 org。design §2.2 已说「适配器放在拥有者侧」，
  但**没给 org/iam 拿到 `PgAuditSink` 实例的路径**（它们不能自己建连接池）。
  → **AC 的步骤 1~3 目前没有可执行的执行路径**。
- **证据**:
  - `BA/demands/REQ-014-audit/design-summary.md:108-137`（§2.3：trait 在 core，`AuditEvent` 无 payload 之外的落库）
  - `BA/demands/REQ-014-audit/design-summary.md:79-107`（§2.2 依赖图 + 「为什么适配器放在拥有者侧」）
  - `BA/demands/REQ-014-audit/design-summary.md:510-564`（§10 代码落点）
  - `BA/demands/REQ-014-audit/decisions.md:112-127`（D135：通用类型放 core）
  - `code/crates/platform-core/Cargo.toml:19`（core 只依赖 `time`；**无 sqlx**）
- **建议改法**（可直接落笔）:
  > 在 design §2.2 的依赖图后追加一节「**sink 的装配路径**」：
  >
  > ```
  > platform-server（唯一持有 PgPool 与 PgAuditSink 实例的地方）
  >   └── 构造 Arc<PgAuditSink> → 注入 platform-iam / platform-rbac / platform-config / platform-org
  >         （这些 crate 只持有 Arc<dyn AuditSink>，**不依赖 platform-audit 的具体类型**）
  > ```
  >
  > 并在 AC-S-129 前置里加一句：
  > 「**前置条件**：平台 sink（`Arc<dyn AuditSink>`）已被装配进被测模块
  > （由 `platform-server` 在启动时注入；测试里用同一装配函数构造）」。
  >
  > 同时在 design §12「开工前必答项」补第 6 条：
  > 「`platform-iam` / `platform-org` 如何在**不依赖 `platform-audit`** 的前提下拿到
  > `Arc<dyn AuditSink>`？（建议：sink trait 定义在 core，实现类型由 server 构造后向下注入；
  > 若 org/iam 需要依赖 `platform-audit`，必须给出不成环的依赖图证明）」。

### S-07: AC-S-124 步骤 4「独立复算」的**时间戳微秒来源**未钉死（且仓里现成 helper 是秒精度）

- **档位**: 不可判定 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-124 步骤 4 与预期结果第 5 条；design §4.2.1
- **问题**: 步骤 4 要求「独立用 `sha256` 复算每条的 `hash`」，预期「独立复算结果与存储的 `hash` 逐条相同」。
  复算必须把 `created_at` 拼进哈希，而哈希里的 `created_at` 文本格式是
  `YYYY-MM-DDTHH:MM:SS.ffffffZ`（D142 §1，**6 位微秒**）。
  **但仓里现成的时间格式化 helper 全是秒精度**：
  - `platform_core::money::format_rfc3339_utc` → `"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z"`（**无小数部分**）
  - `platform-org/src/http.rs::format_timestamp` → 同款（**无小数部分**）
  若 Dev/QA「沿用既有惯例」复用它们 → Rust 侧得到 `2026-11-20T01:13:33Z`，
  DB 侧 `to_char(..., 'US')` 得到 `2026-11-20T01:13:33.339201Z` → **每一条都对不上**
  → 链校验永远失败 → 「实现有 bug」与「规范不一致」无法区分（D142 自己写了这个风险）。
  **第二个坑**：`format_description` 不含偏移分量，直接格式化 `OffsetDateTime`
  会打印**该对象本地时**的钟点再贴一个 `Z`。sqlx 解出的 `OffsetDateTime` 带的是
  **会话时区**的偏移 → 若库/连接的 `TimeZone` 不是 UTC，两侧仍会差一个时区。
  （本机实测 `SHOW timezone` = `UTC`，所以本地看不出来 —— **换环境就炸**。）
- **证据**:
  - `code/crates/platform-core/src/money.rs:484-495`（秒精度）
  - `code/crates/platform-org/src/http.rs:247-253`（秒精度）
  - `code/crates/platform-org/src/ldap.rs:284-288`（同款，且**先 `to_offset(UtcOffset::UTC)`**）
  - `code/Cargo.toml:49`（`time = { version = "0.3", features = ["formatting"] }`）
  - `code/Cargo.toml:59-61`（sqlx 的 `time` feature → `timestamptz ↔ time::OffsetDateTime`）
  - 实测（`base_test`）：`to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')`
    → `2026-11-20T01:13:33.339201Z`（**恰好 6 位、补零**，与 D142 一致 ✅）
    `to_char(TIMESTAMPTZ '2026-11-20 01:02:03+00', ...)` → `2026-11-20T01:02:03.000000Z`（**补零** ✅）
  - `~/.cargo/registry/src/*/time-0.3.55/src/formatting/mod.rs:786-795`
    （`Subsecond { digits: Six }` → **恰好 6 位**、不足补零 ✅）
  - `cargo tree -e features -i time` → `time/macros` 由 `sqlx-core` / `sqlx-postgres` 打开
    → `format_description!` **可用**，**不需新增依赖** ✅（这一条是好消息，见 §5.8）
- **建议改法**（可直接落笔）:
  > 在 design §4.2.1 追加：
  >
  > > **Rust 侧的确切算法（不得复用仓里的秒精度 helper）**：
  > > ```rust
  > > let utc = created_at.to_offset(time::UtcOffset::UTC); // 必须先转 UTC
  > > let text = format!(
  > >     "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
  > >     utc.year(), u8::from(utc.month()), utc.day(),
  > >     utc.hour(), utc.minute(), utc.second(),
  > >     utc.millisecond() * 1000 + u32::from(utc.microsecond()), // 6 位微秒，不足补零
  > > );
  > > ```
  > > ⚠️ **不得**使用 `platform_core::money::format_rfc3339_utc` 与
  > > `platform_org::http::format_timestamp` —— 它们只有**秒**精度。
  > > ⚠️ **必须先 `to_offset(UtcOffset::UTC)`** —— 否则按会话时区打印钟点却贴 `Z`。
  > >
  > > **DB 侧**：`to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')`
  > > （实测：恰好 6 位、不足补零，与上面逐字节相同）。
  >
  > 并在 AC-S-124 步骤 4 追加一句：
  > 「复算时的 `created_at` 文本格式**必须**为 `YYYY-MM-DDTHH:MM:SS.ffffffZ`
  > （6 位微秒、字面量 `Z`）；**直接使用列里 `created_at` 的值**按此格式渲染，
  > **不得**复用仓里只有秒精度的 `format_rfc3339_utc`。」

### S-08: AC-S-129 步骤 4 的扫描口径与 design 冲突（Rust 源码里**不会有** `INSERT INTO platform_audit_log`）

- **档位**: 口径不一致 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-129 步骤 4 与预期结果第 2 条；design §3.5
- **AC 原文**: 「4. 扫描全部 crate 的源码，查找 `INSERT INTO platform_audit_log`；
  预期：步骤 4 的扫描结果**只有平台 sink 所在模块**命中」
- **问题**: design §3.5 规定写入走 `SECURITY DEFINER` 函数 `platform_audit_append`，
  Rust 侧只调 `SELECT platform_audit_append(...)` → **`INSERT` 语句在 plpgsql 函数体内**
  （`migrations/**.sql`），**Rust 源码里一处都不会有**。
  → 期望命中数是 **0**，不是「1」；AC 的措辞（「只有…命中」）会让人以为应该命中 1 处，
  而 Dev 按 AC 写扫描断言时，很可能写成 `assert_eq!(hits, 1)` → **必红**。
  另一半问题：**扫描范围没定义** —— 只扫 `crates/**/*.rs`？还是连 `migrations/**.sql` 一起扫？
  （仓里既有 `INSERT INTO` 出现在 `platform-iam/src/store.rs` 等**业务** crate 的源码里，
  说明「Rust 源码里有裸 SQL」是本仓的既有风格 —— 所以「0 命中」是一个**有意义**的断言，
  而不是恒真。）
- **证据**:
  - `BA/demands/REQ-014-audit/design-summary.md:211-243`（§3.5：写入走 `SECURITY DEFINER` 函数）
  - `BA/demands/REQ-014-audit/design-summary.md:449`（「业务模块**不写审计表 SQL**。AC-S-129 步骤 4 用扫描断言这一点」）
  - 实测：`grep -c 'INSERT INTO' crates/**/*.rs` → `platform-iam/src/store.rs:17`、`platform-org/src/store.rs:7`、
    `platform-rbac/src/store.rs:6` 等 **23 个文件共 71 处** → 扫描器有真实的命中面
- **建议改法**（可直接落笔）:
  > - **步骤 4**：扫描 **`crates/**/*.rs`**（**只扫 Rust 源码，不含 `migrations/`**），
  >   查找 `INSERT INTO platform_audit_log`（大小写不敏感、允许任意空白）。
  > - **预期结果第 2 条**改为：
  >   「步骤 4 的命中数 **== 0**（写入 SQL 只存在于 `platform_audit_append` 的 plpgsql 函数体内，
  >   Rust 侧一律走 `SELECT platform_audit_append(...)`）；
  >   且扫描器**确实扫到了文件**（被扫描的 `.rs` 文件数 `> 0`，证明不是扫了个空目录）；
  >   扫描器**有阳性对照** —— 用同一扫描器扫一段确实含该 SQL 的样例，**必然命中**」。
  > - **补充步骤 4b**：扫描 **`migrations/**/*.sql`**，断言 `INSERT INTO platform_audit_log`
  >   **恰好 1 处**，且位于 `platform_audit_append` 函数体内、该函数带 `SET search_path = public`。

### S-09: AC-S-130 步骤 4 的「夹到上限」与既有惯例 / 契约 C6 **两处冲突**

- **档位**: 口径不一致 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-130 步骤 4、预期结果第 4 条；design §5.1、§7
- **问题**: 三件事混在一条 AC 里，彼此冲突：
  1. **「夹到上限」≠「非法值 400」**：design §7 把 `size = 0` 列为 `AUDIT_INVALID_FILTER`（400），
     而步骤 4 说 `size=1000` 被「夹到上限」。**超上限**与**非法值**是两类，
     AC 只覆盖了前者，且措辞「夹到上限」容易被实现成「所有越界都夹」。
  2. **仓里没有既有分页惯例可参照**：`grep 'page|size|limit|cursor' crates/**/*.rs` 无分页实现。
     既有查询参数全是 `Option<String>` 过滤器（`platform-org/src/http.rs:308`），
     唯一的强类型查询参数是 `platform-server/src/i18n.rs:905-914`（`minor: i64`、`as_of: String`）——
     强类型 + **缺参/错类型由 axum 直接 400**。所以「既有惯例」其实是
     **「反序列化失败 → 400」**，不是「夹到上限」。
  3. **axum 的 400 响应体不是 ProblemDetails** → 违反契约 C6：
     axum 0.8 的 `QueryRejection` 是 `#[status = BAD_REQUEST] #[body = "Failed to deserialize query string"]`
     —— **纯文本**。用 `Query<AuditQuery>` 时 `?size=abc` 会返回纯文本 400，
     而不是 C6 要求的 RFC 9457（含 `code` / `trace_id`）。
- **证据**:
  - `~/.cargo/registry/src/*/axum-0.8.9/src/extract/rejection.rs:84-90`（`FailedToDeserializeQueryString` 的 body 是纯文本）
  - `code/crates/platform-server/src/i18n.rs:905-914`（`ConvertQuery { from: String, to: String, minor: i64, as_of: String }` —— 强类型必填）
  - `code/crates/platform-org/src/http.rs:306-312`（`ListOrgNodesQuery { parent_id: Option<String> }` —— 过滤参数全 `Option`）
  - `BA/contracts/api-conventions/v1.md:92-112`（C6 统一错误响应体）
  - `BA/demands/REQ-014-audit/design-summary.md:368`（`size` 默认 50、上限 200）
  - `BA/demands/REQ-014-audit/design-summary.md:460`（`AUDIT_INVALID_FILTER`：如 `size = 0`、`from > to`）
- **建议改法**（可直接落笔）:
  > - **步骤 4 拆成三步**：
  >   4a. `GET /api/audit/events?page=1&size=1000` → `200`，响应 `size <= 200`（**超上限被夹**）。
  >   4b. `GET /api/audit/events?page=1&size=0` → `400`，`code == "AUDIT_INVALID_FILTER"`（**非法值被拒**）。
  >   4c. `GET /api/audit/events?page=1&size=abc` → `400`，
  >      `Content-Type: application/problem+json`，响应体含 `code` 与 `trace_id`
  >      （⚠️ **不得**是 axum `Query` 的默认纯文本 body）。
  > - 预期结果第 4 条改为：「步骤 4a 的 `size` 被**夹到上限**（`<= 200`），不返回 1000 条；
  >   **超上限被夹**与**非法值被拒（400）**是两类行为，不得混用。」
  >
  > 并在 design §5.1 补一句实现约束：
  > 「数值型查询参数用 `Option<String>` 自行解析（或给 `Query` 配自定义 rejection），
  > 以便把非法值映射成 `AUDIT_INVALID_FILTER` 的 **ProblemDetails**；
  > **不得**直接用 `Query<AuditQuery>` 的默认反序列化拒绝（那是纯文本 body，违反契约 C6）。」

### S-10: AC-S-131 的「**足够旧**的记录」**无法通过 sink 构造**（前置不成立）

- **档位**: 主体不存在 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-131 前置条件第 1 条
- **AC 原文**: 「审计表有 1 条**足够旧**的记录与 1 条**很新**的记录」
- **问题**: 写入函数内部固定用 `clock_timestamp()` 生成 `created_at`（D143），
  **没有**「指定 `created_at`」的入口 → **通过平台 sink 写不出旧记录**。
  唯一可行的构造方式是**管理员连接直接 `INSERT`**（绕开 sink），
  但那样这条记录的 `hash` / `prev_hash` 必须由测试自行计算（否则链一上来就断，
  归档后的链校验语义又立刻变成另一个未定义问题，见 S-16）。
  AC 的前置**没写**这一点，Dev 照字面做会卡住。
- **证据**:
  - `BA/demands/REQ-014-audit/decisions.md:236-248`（D143：`created_at` 由 DB 的 `clock_timestamp()` 生成）
  - `BA/demands/REQ-014-audit/design-summary.md:234-236`（§3.5 写入函数内部取 `clock_timestamp()`）
  - `BA/demands/REQ-014-audit/design-summary.md:481-482`（`audit.retention.online_days` 默认 90）
- **建议改法**（可直接落笔）:
  > - **前置条件**改为：「审计表有 1 条**足够旧**的记录与 1 条**很新**的记录。
  >   ⚠️ **旧记录只能由管理员连接直接 `INSERT` 构造**（平台的写入函数固定用 `clock_timestamp()`，
  >   **没有**指定 `created_at` 的入口）；该记录不参与链校验断言。
  >   「很新」的记录**必须**通过平台 sink 写入（保证链正确）。」
  > - 并补一条夹具约定到 design §10 或 §12：
  >   「测试夹具里的旧记录用 `created_at = now() - interval '120 days'`（默认保留期 90 天，
  >   留 30 天余量，**不依赖阈值的精确边界**）。」

### S-11: 查询接口的**排序**从未定义 → AC-S-132 步骤 2「翻页后记录不重复」**不可判定**

- **档位**: 不可判定 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-132 步骤 2；AC-S-130（间接）；design §5
- **AC 原文**: 「步骤 2 分页可用，翻页后记录**不重复**」
- **问题**: design §5 定义了查询参数与响应形状，**唯独没有定义列表的排序字段与 tiebreaker**。
  （只有链校验那边定义了 `ORDER BY created_at ASC, id ASC`，见 design §4.5。）
  审计列表按时间倒序是常识，但 `created_at` **会并列**（D143 说同租户严格递增 ——
  但那是**承诺**，并发下要靠 TC-014-027 才成立；而且归档 / 导入 / 手工插入都可能造成并列）。
  一旦 `created_at` 并列而排序**不带 tiebreaker**，页码分页就会**重复 / 遗漏** ——
  「翻页后记录不重复」这条 AC 就**判不出红绿**（数据不同，结论不同）。
  → 这是**双重不可判定**：既没定义排序，又要求一个依赖排序的结论。
- **证据**:
  - `BA/demands/REQ-014-audit/design-summary.md:354-391`（§5 端点与响应形状 —— 无排序字段）
  - `BA/demands/REQ-014-audit/design-summary.md:344-350`（§4.5 只有**校验**用的排序）
  - `BA/demands/REQ-014-audit/design-summary.md:170-172`（索引是 `(tenant_id, created_at DESC)` —— **单列**，
    没有 `id` tiebreaker → 并列时索引本身也不保证稳定序）
- **建议改法**（可直接落笔）:
  > 在 design §5.2 追加：
  >
  > > **列表排序**：固定 `ORDER BY created_at DESC, id DESC`
  > > （`id` 是**必需的 tiebreaker** —— `created_at` 可能并列）。
  > > 索引相应改为 `(tenant_id, created_at DESC, id DESC)` 与
  > > `(tenant_id, actor_id, created_at DESC, id DESC)` 等。
  > > 链校验的排序维持 `ORDER BY created_at ASC, id ASC`（design §4.5），
  > > **与列表排序方向相反是有意的**（校验按时间正序，列表按时间倒序）。
  >
  > 并在 AC-S-132 步骤 2 的预期里补一句：
  > 「翻页后记录**不重复**、**不遗漏**（三页的 `id` 并集恰好等于全部记录的 `id` 集合）」。

### S-19: AC-S-133 步骤 4 的「敏感键扫描器」**没有可实现的宿主**

- **档位**: 主体不存在 ｜**级别**: 阻塞
- **位置**: acceptance.md AC-S-133 步骤 4 与预期结果第 4 条；design §2.3 / §3.5
- **AC 原文**: 「4. 对一段**确实含**敏感键的样例字符串跑**同一个**敏感键扫描器。
  预期：步骤 4 的扫描器**命中**（阳性对照）」
- **问题**: AC 要求扫描器能接收**一段任意字符串**并返回命中结果。
  但 design §3.5 把敏感键检查放在**数据库的 `SECURITY DEFINER` 函数内**
  （作用在**入参文本**上，建议 `jsonb_path_exists` 或 `jsonb_each_text`）。
  **plpgsql 里的检查逻辑无法被喂一段任意字符串并返回「命中了哪些键」** ——
  它只能被 `platform_audit_append` 调用。
  → 步骤 4 要求的「同一个扫描器」在 DB 侧**不存在**。
  → 而 design §2.3 又说 `platform-core/extension/audit.rs` 里有「规范化 + 敏感键检查」——
  **两处都做**，则需要**两套实现**（Rust 一套、PL/pgSQL 一套），
  它们的**键集合必须完全一致**，否则会出现「core 拒了但 DB 没拒」或反之的绕过路径。
  **规格没有钉死这一层。**
- **证据**:
  - `BA/demands/REQ-014-audit/design-summary.md:79-82`（core 提供「规范化 + 敏感键检查」）
  - `BA/demands/REQ-014-audit/design-summary.md:230-233`（§3.5：函数内部做敏感键拒绝；作用在**入参文本**上）
  - `BA/demands/REQ-014-audit/design-summary.md:277-280`（PL/pgSQL 里没有 sha256，也没有「返回命中键集合」的现成构件）
- **建议改法**（可直接落笔）:
  > 在 design §3.5 追加：
  >
  > > **敏感键检查放在哪一层（必须两侧都做）**：
  > > | 层 | 职责 | 为什么必须在这层 |
  > > |---|---|---|
  > > | `platform-core`（Rust） | **规范化**（键排序 + 紧凑）+ **敏感键扫描器**（可单独调用，返回命中键集合） | 业务方构造 `AuditEvent` 时**早暴露**；AC-S-133 步骤 4 的阳性对照需要一个**能被喂字符串**的扫描器 |
  > > | `platform_audit_append`（PL/pgSQL） | **同一套键集合**的二次拒绝 | 绕过 core 直接调函数的路径也必须被拦 |
  > >
  > > **键集合的唯一来源**：`platform-core` 导出一份常量（如
  > > `pub const SENSITIVE_KEYS: &[&str]`），
  > > PL/pgSQL 侧由**迁移**按同一份清单生成（迁移里写死同一张列表 + 一条自检注释）。
  > > 两侧清单**必须逐字相同** → 验收时用一条用例比对「Rust 侧清单」与
  > > 「迁移里生成的清单」的长度与内容（TC-014-106）。
  > >
  > > **扫描器签名**（供 AC-S-133 步骤 4 直接调用）：
  > > `fn scan_sensitive_keys(json_text: &str) -> BTreeSet<String>`
  > > —— 返回**归一化后**（小写、`-`→`_`）的命中键集合；`json_text` 不是合法 JSON 时返回**空集**
  > > （不 panic、不报错）。
  >
  > 并在 AC-S-133 步骤 4 补一句：
  > 「用**生产路径上同一个**扫描器函数（`platform_core::extension::audit::scan_sensitive_keys`）
  > 扫这段样例 —— 断言返回的命中键集合**非空**且**包含** `password`。」

---

## 高（10）

### S-12: AC-S-022 步骤 5 的告警字段名 `event_id` 与表列名 `id` **不一致**，且「是否恰好 3 个字段」未定

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: acceptance.md AC-S-022 预期结果第 3 条
- **问题**: AC 要求日志含结构化字段 `tenant_id` / `event_id` / `reason`。
  但表里的主键列叫 **`id`**（契约 C12）→ 日志字段叫 `event_id` 是不是同一回事？
  另外「含结构化字段 `tenant_id`/`event_id`/`reason`」既可以读成「**至少**含这 3 个」
  也可以读成「**恰好**这 3 个」—— 前者允许日志里再塞 `before`/`after` 的明文（一次泄漏），
  后者会让日志实现很脆。判据不可判定。
- **证据**: `BA/contracts/api-conventions/v1.md:165-168`（C12：字段名是 `id`）；
  `BA/demands/REQ-014-audit/acceptance.md:39-40`（AC-S-022 步骤 5）
- **建议改法**:
  > 预期结果第 3 条改为：
  > 「步骤 4 期间产生**至少一条 `error` 级别**日志，其**结构化字段集合**包含
  > `tenant_id`、`event_id`、`reason`；其中 `tenant_id == "<被篡改记录的 tenant_id>`、
  > `event_id == "<被篡改记录的 id 列值>"`、`reason == "hash_mismatch"`。
  > 日志**不得**包含该记录的 `before` / `after` 明文（只允许 `id` 与原因码）。」
  >
  > 并在 design §5.2 补一句：「日志字段名用 `event_id`（**不是** `id`），
  > 其值等于记录的 `id` 列值 —— 避免与 `id`（自身）混淆。」

### S-13: AC-S-022 的「**定期**校验链完整性」**没有执行者**

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: acceptance.md AC-S-022 标题与步骤 2
- **问题**: 标题写「**定期**校验链完整性」，但 D137 明确「**不引入**后台定时任务」
  （归档也是显式触发）。→ 「定期」在本仓**没有任何执行者**。
  AC 的步骤 2 又只说「调用链校验能力」—— 没说**谁**在什么时机调用。
  → 「定期」这半句是**不可验收的承诺**（既没有 AC 覆盖，也没有实现主体）。
- **证据**: `BA/demands/REQ-014-audit/decisions.md:144-155`（D137）；
  `BA/demands/REQ-014-audit/acceptance.md:25`（AC 标题）
- **建议改法**:
  > 把 AC-S-022 的标题改为「**哈希链可校验**：提供链校验能力，篡改任一条目即被检出并告警」，
  > 并在需求描述（demand §2）里补一句：
  > 「链校验由**显式触发**（`GET /api/audit/verify` 或 `cargo xtask audit verify`）执行；
  > 本仓**不引入**后台调度器（D137），『定期』由外部定时任务调用该入口实现。」

### S-15: AC-S-127 / AC-S-128 的**事件码与 `object_type` 字面量**全部未给出

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: acceptance.md AC-S-127 预期结果第 1~3 条；AC-S-128 预期结果第 1 条
- **问题**: AC-S-127 直接写死了 `iam.login.succeeded` / `iam.login.failed` /
  `iam.sso.login.succeeded`（好），但 **AC-S-128 只说「`action` 为授予动作」** ——
  没有字面量。design §6.1 给了 `rbac.role.grant` / `rbac.role.revoke` /
  `rbac.role.create` / `rbac.role.delete`，但 AC 与 design 的关系**没写明**
  （AC 是验收基线，design 是实现依据；两者不一致时按 AC 走）。
  同理 `object_type`（「角色对象类型」）没有字面量。
  → 判据只能写成「逐字等于实现约定的值」，**判不出红绿**。
- **证据**: `BA/demands/REQ-014-audit/acceptance.md:162-164`；
  `BA/demands/REQ-014-audit/design-summary.md:424-425`（§6.1 埋点表）
- **建议改法**:
  > AC-S-128 预期结果第 1 条改为：
  > 「步骤 1 产生 **1 条**记录，`action == "rbac.role.grant"`，
  > `object_type == "role"`，`object_id == "r-1"`；
  > `before` / `after` 能解析出 `p-1` 在**变更前不在**、**变更后在**。」
  > 预期结果第 2 条改为：
  > 「步骤 2 再产生 **1 条**记录，`action == "rbac.role.revoke"`，`object_type == "role"`，
  > `object_id == "r-1"`，`before` / `after` 的方向**相反**。」
  >
  > AC-S-127 的 SSO 那条补一句：`object_type == "sso_identity"`（或钉死值），
  > `object_id` 为该 SSO 绑定的 `external_id`。

### S-16: 归档**之后**的链校验语义**未定义**；归档表的列集合也未在 AC 里钉死

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: acceptance.md AC-S-131 步骤 2 与预期结果第 4 条
- **问题**: 归档把中间的记录搬走后，剩余记录的 `prev_hash` 会**接不上**已搬走的那条 →
  链校验会怎样？AC 没写，design 也没写。三种都说得通：
  (a) `ok=false` + 报接续断裂；(b) `ok=true` + 只验在线表里**连续段**；
  (c) 归档时**重算**剩余记录的链（但那会破坏 AC 的「逐字段相同」）。
  → 归档与链校验这两个 AC **互相影响**，却没有交集判据。
  另外「归档表内容与被搬走的记录**逐字段相同**」没说归档表会不会**多**一列
  （design §3.2 说会多 `archived_at`）。
- **证据**: `BA/demands/REQ-014-audit/design-summary.md:175-178`（§3.2 归档表多一列）；
  `BA/demands/REQ-014-audit/design-summary.md:245-258`（§3.6 归档函数）
- **建议改法**:
  > 在 design §3.6 追加：
  >
  > > **归档后的链校验语义**：链校验只针对**在线表**里的记录。
  > > 归档搬走记录后，剩余记录构成的**连续段**（从「`prev_hash` 等于 genesis 的那条」
  > > 开始，到在线表最后一条）参与校验；**被搬走的段不参与**，`checked` 等于该连续段的条数。
  > > 搬走**首条**（genesis 那条）之后的场景：在线表剩余记录的 `prev_hash` 指向已搬走的记录 →
  > > 此时该连续段**为空** → `checked == 0`、`ok == true`、`broken == []`。
  > > （**不重算**任何哈希 —— 归档必须保持「逐字段相同」。）
  >
  > 并在 AC-S-131 预期结果里补一条：
  > 「归档表的列集合 `==` 在线表的列集合 **∪** `{archived_at}`（14 列）；」
  > 「归档后链校验 `ok == true`、`broken` 为空，`checked` 等于在线表剩余记录数。」

### S-17: design 内部对 RLS policy 的做法**自相矛盾**（生成器 vs 手写）

- **档位**: 口径不一致 ｜**级别**: 高
- **位置**: design §1.1（现状盘点表）与 §3.3
- **问题**: §1.1 写「RLS policy 生成器 `crates/platform-db/src/rls.rs:143` |
  已有生成 `USING/WITH CHECK` 的 helper；**优先复用**而不是手写 policy」；
  §3.3 却给出手写的 policy SQL（照抄 `migrations/20261118000200_tenant_isolation.up.sql:32`）。
  两条指令给 Dev 两个不同的工作量与两个不同的产物。
  更要紧的是**「优先复用」这条本身有坑**：生成器吃的是 `TenantScopedSpec`
  （来自 `platform_core::extension::manifest`），而**审计表不是模块表** ——
  `base tenant-policies` 扫的是 `modules/*/manifest.json` 与 `crates/*/manifest.json`
  里的 `tenantScoped` 声明，审计表的 manifest 是**权限点** manifest，
  不一定会声明 `tenantScoped` → **生成器这条路对审计表未必走得通**。
- **证据**:
  - `BA/demands/REQ-014-audit/design-summary.md:29`（§1.1「优先复用」）
  - `BA/demands/REQ-014-audit/design-summary.md:180-194`（§3.3 手写 policy）
  - `code/crates/platform-db/src/rls.rs:112-166`（`policy_statements(spec: &TenantScopedSpec, …)`）
  - `code/crates/platform-db/src/rls.rs:29`（`use platform_core::extension::manifest::TenantScopedSpec`）
- **建议改法**:
  > 把 §1.1 那一行的「优先复用而不是手写 policy」改为：
  > 「RLS policy 生成器 `crates/platform-db/src/rls.rs:143` 的**输入**是
  > `TenantScopedSpec`（来自 manifest 的 `tenantScoped` 声明）。
  > **审计表不是模块作用域表**（它的 manifest 声明的是权限点），
  > 因此**不走生成器**，policy 在迁移里手写（照 `20261118000200_tenant_isolation.up.sql:32`）。
  > ⚠️ 后果：审计表**不在** `base tenant-policies --check` 的自检范围内 ——
  > 平台不会自动发现审计表的 policy 被误删。**因此审计表的 RLS 必须由本需求的
  > 自动化用例覆盖**（TC-014-046 / TC-014-047）。」

### S-18: AC-S-130 步骤 3 的「错误码**明确**」没有给具体码

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: acceptance.md AC-S-130 预期结果第 3 条
- **问题**: 「返回 **403** 且错误码**明确**（不是 500）」—— 「明确」不可判定。
  仓里已有现成码 `INSUFFICIENT_PERMISSION`（契约 C6 的示例也用它），但 AC 没点名。
- **证据**: `code/crates/platform-core/src/problem.rs:35-36`；
  `BA/contracts/api-conventions/v1.md:103`（C6 示例的 `code`）
- **建议改法**:
  > 预期结果第 3 条改为：
  > 「步骤 3 返回 **403**，响应体是 RFC 9457 ProblemDetails，
  > `code == "INSUFFICIENT_PERMISSION"`，`params.required == "audit:events:read"`，
  > 且**不返回**任何审计记录内容。」

### S-20: AC-S-133 的「哪些键算敏感」**留空**（D134 只写了「等」）

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: acceptance.md AC-S-133 步骤 1；decisions.md D134
- **问题**: D134 列了 6 个键（`password` / `secret` / `token` / `credential` /
  `private_key` / `api_key`）**加一个「等」**。AC 完全没定义键集合 →
  扫描器实现者自选 → 「命中」的判据不可判定（连阳性对照都写不出来）。
  同时「大小写与连字符变体一并匹配」也**没定义归一化规则**
  （`apiKey` 算不算 `api_key`？`Password` 归一化成什么？）
- **证据**: `BA/demands/REQ-014-audit/decisions.md:98-109`（D134）；
  `BA/demands/REQ-014-audit/acceptance.md:250-262`（AC-S-133）
- **建议改法**（可直接落笔）:
  > 在 AC-S-133 前置条件后追加「**敏感键集合（唯一来源）**」：
  >
  > > ```
  > > password, passwd, pwd, secret, client_secret, token, access_token,
  > > refresh_token, id_token, credential, private_key, api_key, authorization, cookie
  > > ```
  > >
  > > **匹配规则**：
  > > 1. **按完整键名匹配**（**不是子串**）—— `secretary` / `tokenizer` / `passwordPolicy` **不命中**；
  > > 2. 键名先**归一化**：转小写 → `-` 换成 `_` → 去掉其余非 `[a-z0-9_]` 字符；
  > >    归一化后与集合比对（`api-key` / `apiKey` / `API_KEY` 归一化后都是 `api_key`）；
  > > 3. **递归**检查所有层级（嵌套对象、数组里的对象都算）。
  >
  > 并把 D134 的「等」改成完整清单（同一份，两处必须逐字相同）。

### S-24: AC-S-127 步骤 4 / AC-S-133 步骤 3 的「检索该租户的全部审计记录**文本**」未定义检索面

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: acceptance.md AC-S-127 步骤 4；AC-S-133 步骤 3
- **问题**: 「检索该租户的全部审计记录文本」—— 检索**哪些字段**？用**哪个接口**？
  若只检索 `before`/`after`，凭据落在 `action` / `object_id` / `ip` 里就漏；
  若用 SQL 全表扫，则**绕过了应用**（这条 AC 验的是「系统没把凭据写进去」，
  用 SQL 扫反而是最直接的）。两条 AC 的措辞相同但都没钉死。
- **证据**: `BA/demands/REQ-014-audit/acceptance.md:150`、`:256`
- **建议改法**:
  > 把两句统一改为：
  > 「检索该租户**全部**审计记录的**全部列拼接文本**：
  > `SELECT string_agg(t::text, E'\n') FROM platform_audit_log t WHERE tenant_id = '<租户>'`
  > （`t::text` 展开**所有列**）→ 在该文本里搜 canary 字面量。命中数必须 `== 0`，
  > 且**该文本的长度 `> 0`**（防空绿：必须真的检索到了东西）。」

### S-25: D143 承诺的「同租户 `created_at` **严格递增**」**没有任何 AC 覆盖**，且 design 建议的锁法在 READ COMMITTED 下**防不住链分叉**

- **档位**: 不可判定 ｜**级别**: 高 ｜**决策层质疑**
- **位置**: decisions.md D143；design §3.5 的注、§4.5、§12 开工前必答项 1
- **问题**: 两件事：
  1. **承诺无验收**：D143 承诺「同租户 `created_at` 严格递增」，但 13 条 AC 里**没有一条**覆盖它
     （AC-S-124 只验**串行**写 3 条的链接关系）。承诺不可验收 = 承诺不存在。
  2. **锁法不够**（我不同意 design 的建议方案，理由见下）：
     design §3.5 建议「函数内用 `SELECT … ORDER BY created_at DESC, id DESC LIMIT 1 FOR UPDATE`
     锁住上一条，或用表级 `LOCK`」，并把方案留给 Dev。
     **在 PostgreSQL 的 READ COMMITTED（sqlx 的默认隔离级别）下，`FOR UPDATE` 锁最后一行防不住分叉**：
     - 事务 A 锁住第 N 条，插入第 N+1 条（`prev_hash = hash(N)`），提交；
     - 事务 B 的 `SELECT … FOR UPDATE` 语句**快照早于 A 的提交** → 它在 A 提交后拿到第 N 条的锁，
       但该语句的快照里**看不到** A 新插入的第 N+1 条 → B 仍算出 `prev_hash = hash(N)`
       → **两条记录的 `prev_hash` 相同 → 链分叉 → 链校验必然失败**。
     - 正确手法是「**先拿锁，再在新语句里重读**」（新语句拿新快照）或
       `pg_advisory_xact_lock(租户哈希)` / 表级 `LOCK`，让 B 在拿到锁后**重新**看到第 N+1 条。
- **证据**:
  - `BA/demands/REQ-014-audit/decisions.md:236-248`（D143）
  - `BA/demands/REQ-014-audit/design-summary.md:238-243`（§3.5 的注）
  - `BA/demands/REQ-014-audit/design-summary.md:344-350`（§4.5 声称「严格递增」）
  - `BA/demands/REQ-014-audit/design-summary.md:589-591`（§12 开工前必答项 1）
  - PostgreSQL 文档行为：`SELECT … FOR UPDATE` 在 READ COMMITTED 下会在锁释放后重新检查**被更新的行**，
    但**不会**扩展语句快照去看**新插入的其它行**
- **建议改法**（可直接落笔）:
  > **① 在 AC-S-124 追加一条验收项**：
  > 「**并发**：两个独立连接**同时**向**同一租户**写入 2 条记录 →
  > 两条都写入成功；按 `created_at ASC, id ASC` 排序后，第 2 条的 `prev_hash`
  > **==** 第 1 条的 `hash`；两条的 `prev_hash` **互不相同**；链校验 `ok == true`。」
  >
  > **② 把 design §3.5 的注改为**（明确排除 `FOR UPDATE` 方案）**：
  > > ⚠️ **不要**用「`SELECT … ORDER BY created_at DESC LIMIT 1 FOR UPDATE` 锁上一条」——
  > > 在 READ COMMITTED（sqlx 默认）下**防不住链分叉**：后到的事务在拿到锁后
  > > **仍然看不到**先到事务新插入的那一行 → 两者的 `prev_hash` 相同。
  > > 必须用**真正串行化**的手法，且**锁之后要在新语句里重读**：
  > > (a) `pg_advisory_xact_lock(hashtext('audit:' || p_tenant_id))` → 然后**新语句**重读最后一条；
  > > 或 (b) `LOCK TABLE platform_audit_log IN EXCLUSIVE MODE` → 然后**新语句**重读。
  > > （两者都保证「后到者看到先到者刚插入的那一行」。）
  >
  > **③ D143 的表述补一句**：「『严格递增』指**写入函数返回后**观察到的顺序；
  > `clock_timestamp()` 在极端情况（NTP 回拨）下仍可能倒退 —— 该风险记入 tech-debt，
  > 表现为链校验**告警**，不是数据丢失。」

### S-29: 「审计写入失败时登录怎么办」**没有规定**

- **档位**: 不可判定 ｜**级别**: 高
- **位置**: acceptance.md AC-S-127（未覆盖）；demand §4
- **问题**: 登录成功/失败各产生一条审计记录 —— 但如果**审计写不进去**（DB 抖动、审计表不可写、
  敏感键误触发）而登录照常成功 → 就有一次**没有审计的登录**，这与需求「所有敏感操作可追溯」冲突；
  反之若审计失败就让登录失败 → 可用性受损。AC 没规定这个取舍，
  Dev 只能自己选，而两种选择的**安全含义相反**。
- **证据**: `BA/demands/REQ-014-audit/demand.md:13`（「所有敏感操作可追溯」）；
  `BA/demands/REQ-014-audit/acceptance.md:137-151`（AC-S-127 无此判据）
- **建议改法**:
  > 在 AC-S-127 追加一步 5 与一条预期：
  > 「5. 制造一次审计写入失败（如让 `platform_audit_append` 抛错），然后用**正确密码**登录。
  > 预期：登录**不返回 200**（返回 `503` + ProblemDetails，`code` 为已登记码）；
  > 响应体与日志均**不含**令牌。」
  >
  > 并在 demand §4 补一句取舍说明：
  > 「**审计写入失败时，登录一律失败**（fail-closed）——
  > 宁可少一次登录成功，不留一次无审计的登录。」

---

## 中（10）

### S-14: AC-S-123 前置写「应用连接角色（`base_platform_app`）」—— 该角色 **NOLOGIN，连不上**

- **档位**: 口径不一致 ｜**级别**: 中
- **位置**: acceptance.md AC-S-123 前置第 3 条
- **证据**（实测 `base_test`）:
  ```
  rolname           | rolsuper | rolbypassrls | rolcanlogin
  base              | t       | t            | t
  base_platform_app | f       | f            | f      ← NOLOGIN 组角色
  base_app          | f       | f            | t      ← 能登录，且是 base_platform_app 的成员
  ```
  `code/deploy/seed/02-app-role.sql:9-10`（角色说明）、`:19-23`（NOLOGIN）、`:33-40`（成员关系）
- **建议改法**:
  > 前置条件第 3 条改为：
  > 「持有**应用连接**（`base_app`，它是组角色 `base_platform_app` 的成员）的连接。
  > ⚠️ **不要**用 `base_platform_app`（NOLOGIN 组角色，**连不上**），
  > 也**不要**用 `base`（超级用户 + 表 owner，会绕过 RLS 且能改表）。」
  >
  > 同时把该条 AC 的 ⚠️ 注记改为：
  > 「该判据**必须**用应用角色验证。用表 owner 连接会绕过权限，测不出任何东西。」

### S-21: 敏感键 / JSON 规范化的**归属层**在 design 内部**前后不一**

- **档位**: 口径不一致 ｜**级别**: 中
- **位置**: design §2.3、§3.5、§4.3
- **问题**: §2.3 说 core 的 `extension/audit.rs` 提供「规范化 + 敏感键检查」；
  §3.5 说 **DB 函数内部**做敏感键拒绝**和** JSON 规范化；§4.3 又说「必须自己实现规范化」。
  → **规范化做两遍**（Rust 一遍、PL/pgSQL 一遍）还是只做一遍？
  两遍就有「两套实现产出不同文本」的风险（键排序规则、Unicode 转义规则 —— D142 明确禁止
  `\uXXXX` 转义非 ASCII，而 PL/pgSQL 的 `jsonb` 序列化**默认就转义非 ASCII**
  且**重排键顺序**、把 `1.0` 写成 `1.0` 还是 `1` 另说）→ **极可能产出不同文本**，
  而 D141 又要求「哈希覆盖**存进列里的那段文本**」→ 落库文本由谁定？
- **证据**:
  - `BA/demands/REQ-014-audit/design-summary.md:79-82`（core 侧）
  - `BA/demands/REQ-014-audit/design-summary.md:230-234`（DB 函数侧）
  - `BA/demands/REQ-014-audit/design-summary.md:312-330`（§4.2.2 的字节级格式）
  - `BA/demands/REQ-014-audit/design-summary.md:331-338`（§4.3）
  - PostgreSQL 行为：`jsonb` 类型**会**重排对象键、**会**把非 ASCII 转成 `\uXXXX`、
    数值 `1.0` 与 `1` **不等价** —— 所以 PL/pgSQL 里用 `jsonb` 做规范化**与 D142 冲突**
- **建议改法**:
  > 在 design §4.3 追加：
  >
  > > **规范化的唯一实现位置 = Rust 侧（`platform-core`）**。
  > > 规范化后的**文本**作为**入参**传给 `platform_audit_append`，
  > > **函数内部不做任何 JSON 处理**（design §3.5 已经这么写，本节把它钉死）。
  > > ⚠️ **禁止**在 PL/pgSQL 里用 `jsonb` / `jsonb_build_object` 做规范化 ——
  > > `jsonb` 会**重排键**、**把非 ASCII 转成 `\uXXXX`**、并且 `1.0` 与 `1` 不等价，
  > > 与 D142 的字节级格式**冲突**。
  > > 函数内的敏感键检查作用在**已经规范化过的入参文本**上（键名已归一化 →
  > > 检查只需按完整键名比对）。
  >
  > （**代价**：绕过 core 直接调函数的调用方能写入未规范化的文本 →
  > 那条路径的 `hash` 仍自洽（D141：哈希覆盖存进去的文本），
  > 但**存储形态**不再满足 D142。本轮靠「函数只被 core 调用」保证；
  > 建议在 design §12 记一条待办。）

### S-23: AC-S-130 的响应**字段名**没在 AC 里给出

- **档位**: 表述 ｜**级别**: 中
- **问题**: AC 只说「返回该租户的 **2 条**记录 + **总数 5**」，没说字段名。
  design §5.2 用 `items` / `total` / `page` / `size`。前端与测试都要按字段名写。
- **证据**: `BA/demands/REQ-014-audit/design-summary.md:381-392`
- **建议改法**:
  > 预期结果第 1 条改为：
  > 「步骤 1 返回 `200`，响应体 `items` 长度 `== 2`、**`total == 5`**、`page == 1`、`size == 2`；
  > `items` 里的 2 条 `id` **互不相同**。」

### S-26: AC-S-122 的 `action = "demo.update"` 与 design §6.1 的命名法 `<域>.<对象>.<动作>` **冲突**

- **档位**: 口径不一致 ｜**级别**: 中
- **问题**: design §6.1 写「事件码沿用既有命名法：`<域>.<对象>.<动作>`
  （既有 `org.node.create`、`iam.sso.login.succeeded`）」。
  而 AC-S-122 用 `demo.update`（**两段**）+ `object_type = "demo_item"`。
  若 Dev 按 design 规范化 → `demo.demo_item.update` → AC 的「逐字相同」判据**失败**。
  （注意 `iam.sso.login.succeeded` 是**四段**，说明既有命名法本身就不统一 → 更该钉死。）
- **证据**: `BA/demands/REQ-014-audit/design-summary.md:428-430`（§6.1）；
  `BA/demands/REQ-014-audit/acceptance.md:49-51`（AC-S-122 步骤 1）
- **建议改法**:
  > 把 design §6.1 的那句改为：
  > 「事件码沿用既有命名法：`<域>.<对象>.<动作>`。**AC 里的示例值按其原样写**，
  > 实现**不得**对调用方传入的 `action` 做任何规范化 / 补段 / 改大小写 ——
  > 「逐字相同」是硬判据（AC-S-122）。」
  >
  > 并在 AC-S-122 步骤 1 的 `action` 值旁加一句
  > 「（**原样**；平台不改写、不补段、不改大小写）」。

### S-27: AC-S-122 的「`ip` 与 `user_agent` **存在该列**（**不得缺列**）」—— 在哪一层验？

- **档位**: 不可判定 ｜**级别**: 中
- **问题**: 「存在该列」在 **DB 列**层面可验；「不得缺列」在 **API 响应**层面也说得通
  （响应体里必须有 `ip` / `userAgent` 键）。AC 两处混在一起，判据不可判定。
- **证据**: `BA/demands/REQ-014-audit/acceptance.md:58`；
  `BA/demands/REQ-014-audit/design-summary.md:157-158`（列定义 `TEXT NOT NULL DEFAULT ''`）、
  `:383-386`（响应形状含 `ip` / `userAgent`）
- **建议改法**:
  > 预期结果第 5 条拆成两条：
  > 「- `platform_audit_log` 表**有** `ip` 与 `user_agent` 两列，均 `NOT NULL`，
  >   默认值 `''`；本次写入取不到值时两列的值为**空串**（不是 `NULL`、不是缺列）。
  > - 查询接口的响应项里**有** `ip` 与 `userAgent` 两个键（值可以是空串，但**键不得缺**）。」

### S-30: AC-S-128 的 `before` / `after` **内容形状**未定义

- **档位**: 不可判定 ｜**级别**: 中
- **问题**: AC 说「`before` / `after` 能解析出 `p-1` 在**变更前不在**、**变更后在**」。
  「能解析出」的**形状**没定：顶层数组 `["p-1"]`？顶层对象 `{"permissions":["p-1"]}`？
  还是整个角色对象 `{"id":"r-1","permissions":[...]}`？
  → 实现可以自选，判据只能写成「在某一层能取到 `p-1`」这种弱断言。
- **证据**: `BA/demands/REQ-014-audit/acceptance.md:163-166`
- **建议改法**:
  > 预期结果第 1~2 条改为：
  > 「`before` / `after` 解析后**恰好**是权限码数组（JSON 数组，元素为字符串）：
  > `before == []`、`after == ["p-1"]`（授予）；`before == ["p-1"]`、`after == []`（撤销）。
  > ⚠️ 形状由本 AC 钉死 —— 实现不得改成对象或数组套对象。」
  >
  > （若要携带角色 id，放 `object_id` 列，**不放** `before`/`after` 里。）

### S-33: design §5.1 的「传了 `tenant_id` **也必须被忽略**（**或**直接 400）」—— 二选一

- **档位**: 不可判定 ｜**级别**: 中
- **问题**: 「忽略」与「400」是两种**不同**的行为，AC 的判据写不出红绿。
- **证据**: `BA/demands/REQ-014-audit/design-summary.md:376-377`
- **建议改法**:
  > 把 design §5.1 的那句改为：
  > 「`tenant_id` **不接受**来自查询参数 —— 租户只来自认证上下文 + RLS（D132）。
  > 传了**直接 400**（`AUDIT_INVALID_FILTER`），**不得**静默忽略 ——
  > 静默忽略会让调用方误以为换租户生效了。」
  >
  > 并在 AC-S-130 追加一步：
  > 「5. `GET /api/audit/events?tenant_id=t-other` → `400`，`code == "AUDIT_INVALID_FILTER"`；
  >    且**任何**情况下都**不返回** `t-other` 的记录。」

### S-34: 归档的**作用域**（全平台 / 单租户）与 xtask 的**连接角色**未定义

- **档位**: 不可判定 ｜**级别**: 中
- **问题**: D137 说归档由「管理接口 + 命令行」触发，但没说：
  - 接口 `POST /api/audit/archive` 是**当前租户**归档还是**全平台**归档？
  - xtask 用**哪个角色**连？用超级用户 `base` → 绕过 RLS → 能搬所有租户的记录；
    用 `base_app` → 受 RLS 限制 → 只搬得到本租户。
  这直接决定归档后各租户的 `checked` 变化（见 S-16），也决定 AC-S-131 步骤 2 的断言面。
- **证据**: `BA/demands/REQ-014-audit/decisions.md:144-155`（D137）；
  `BA/demands/REQ-014-audit/design-summary.md:245-249`（§3.6 归档函数签名只有 `p_before`，**没有租户参数**）
- **建议改法**:
  > 在 design §3.6 追加：
  >
  > > **归档作用域 = 单租户**：归档函数按**当前连接的租户上下文**
  > > （`app.tenant_id`）只搬本租户的行 → RLS 对归档函数同样生效（函数是
  > > `SECURITY DEFINER`，但**显式**加 `WHERE tenant_id = current_setting('app.tenant_id', true)`）。
  > > `cargo xtask audit archive` **必须**用**应用角色**（`base_app`）连接并设置租户上下文，
  > > **不得**用超级用户连接（超管会绕过 RLS，把所有租户的记录一次性搬走）。
  > > `POST /api/audit/archive` 同样只归档**当前令牌所属租户**的记录。

### S-35: 审计页需要**服务端分页**，不能照抄既有受保护页（客户端分页）

- **档位**: 口径不一致 ｜**级别**: 中
- **位置**: design §10 前端落点；acceptance.md AC-S-132 步骤 2
- **问题**: 既有的受保护页 `/admin/roles`（`apps/shell/src/pages/ProtectedRoles.svelte`）
  用的是 `@base/hooks` 的 `createPagination({ pageSize: 10 })` + `components/Table.svelte`
  —— **客户端分页**（`total={props.roles.length}`，数据一次性全量由路由 `load` 取回）。
  审计列表**必须**服务端分页（后端有 `page`/`size`/`total`，记录量大）。
  两套分页模型的 props 形状不同（服务端版需要 `total` 来自后端、`onPageChange` 要重新请求）。
  design §10 只写「新增 `Audit.svelte`」，没提示这个差异 → Dev 极可能照抄。
- **证据**:
  - `code/apps/shell/src/pages/ProtectedRoles.svelte:42-66`（客户端分页）
  - `code/apps/shell/src/lib/routes.ts:75-83`（`/admin/roles` 的 `load` 一次性取回全部）
  - `code/apps/shell/src/lib/router.ts:244-250`（`load` 在守卫放行后才调用）
  - `BA/demands/REQ-014-audit/design-summary.md:566-573`（§10 前端落点）
- **建议改法**:
  > 在 design §10 的前端段追加：
  >
  > > ⚠️ 审计页用**服务端分页**（后端 `page`/`size`/`total`），
  > > **不能照抄** `ProtectedRoles.svelte` 的**客户端分页**
  > > （`createPagination` + `total={props.roles.length}`）——
  > > 审计记录量大，必须让后端分页。
  > > `onPageChange` 回调要**重新请求**后端并更新 `items` 与 `total`，
  > > 而不是切片本地数组。
  > > 路由的 `load` 只负责**第一页**；`permission: 'audit:events:read'`，
  > > 守卫不放行时 `load` 不被调用 → 目标接口请求计数为 **0**（与 `/admin/roles` 同款纪律）。

### S-36: 权限点 manifest 在 `platform-audit`、HTTP 在 `platform-server` → **C-5 契约检查**会开新缺口

- **档位**: 口径不一致 ｜**级别**: 中
- **位置**: design §2.2（HTTP 放 server）、§9（manifest 放 `crates/platform-audit/manifest.json`）
- **问题**: 平台有 **C-5 契约检查**：「已登记权限点必须有**鉴权调用点**」，
  而它的实现是 `check_c5_for(manifest_path, source_dir)` ——
  **扫描的是「该 manifest 所属 crate 自己的 `src/`」**里的
  `require_permission("字面量")` 调用点。
  design §2.2 规定 HTTP 层放 `platform-server`（防成环），§9 规定 manifest 放
  `crates/platform-audit/manifest.json` → **`require_permission("audit:events:read")`
  会落在 `crates/platform-server/src/audit.rs`，不在 `crates/platform-audit/src/`**
  → 对 `platform-audit` 跑 C-5 会报「3 个已登记权限点**没有鉴权调用点**」→ **违例**。
  既有先例：`platform-org` 已经是「9 个登记 / 0 个调用点」的**已知技术债**
  （`crates/platform-config/tests/contract_c5.rs:147-155` 的基线快照），
  `platform-rbac` 是「4 登记 / 3 调用点」（`rbac:permission:read` 缺）。
  → 新 crate 不应该再开一个同类的缺口。
  （另：权限点**注册表对账**这条路是通的 —— `collect_manifest_permissions` 扫
  `crates/*/manifest.json`，所以 3 个权限点必须在 `platform_permission` 表里 INSERT，
  否则**启动失败**。这部分 design §9 是对的。）
- **证据**:
  - `code/crates/platform-contracts/src/checks.rs:479-504`（`check_c5_for(manifest_path, source_dir)` 的说明与参数）
  - `code/crates/platform-contracts/src/authz_scan.rs:66-81`（`scan_auth_calls(backend_dir)` 只扫一个目录）
  - `code/crates/platform-config/tests/contract_c5.rs:143-158`（platform-org / platform-rbac 的基线快照）
  - `code/crates/platform-rbac/src/registry.rs:92-153`（`collect_manifest_permissions` 扫 `crates/*/manifest.json`）
  - `BA/demands/REQ-014-audit/design-summary.md:99-102`（§2.2「为什么 HTTP 放 platform-server」）、
    `:494-506`（§9 权限点与 manifest 路径）
- **建议改法**（二选一）:
  > **选项 A（推荐，不动依赖方向）**：把 `crates/platform-audit/manifest.json` 里的
  > **权限点**仍然登记在那里（注册表对账要它），但**鉴权调用点**放在
  > `crates/platform-audit/src/` 里 —— 具体做法是：在 `platform-audit` 里暴露
  > 一组**薄封装**函数（如 `pub fn require_events_read(ctx) -> Result<()>`），
  > 内部调用 `platform_core::extension::permission::require_permission(ctx, "audit:events:read")`，
  > `platform-server/src/audit.rs` 调这个封装。
  > → C-5 扫 `crates/platform-audit/src` 时能命中字面量，且**不**引入
  > `platform-audit → platform-rbac` 的依赖（`require_permission` 在 **core** 里）。
  >
  > **选项 B**：在 `crates/platform-config/tests/contract_c5.rs` 的基线快照里
  > 登记 `platform-audit`（3 登记 / 0 调用点）—— 即**承认**这是新债。
  > （与 `platform-org` 同款处理，但**要写进 tech-debt**。）
  >
  > 并在 design §9 追加：
  > 「⚠️ C-5 契约检查要求「manifest 登记的权限点」在**同一 crate 的 `src/`** 里有
  > `require_permission("字面量")` 调用点。本需求的 HTTP 层在 `platform-server`（防成环），
  > manifest 在 `platform-audit` → **必须在 `platform-audit/src/` 里放薄封装**，
  > 否则 C-5 会报违例。」

---

## 低（4）

### S-22: 夹具的**租户命名**在本需求内不统一（`t-audit`/`t-other` vs REQ-013 的 `tenant-a`/`tenant-b`）

- **档位**: 表述 ｜**级别**: 低
- **问题**: REQ-014 的 AC 用 `t-audit` / `t-other`；REQ-013 的用例集用 `tenant-a` / `tenant-b`。
  两个需求共用同一个测试库 → 两套命名会**互相踩**（同一个库里的租户 id 不同，
  但夹具脚本各写各的，清理逻辑会互相误删）。
- **证据**: `BA/demands/REQ-014-audit/acceptance.md:83`、`:123`；
  `BA/demands/REQ-013-sso/test-cases/index.md:109-111`
- **建议改法**:
  > 在 test-cases/index.md §4.1 补一句：
  > 「本需求沿用 REQ-013 的租户命名：**`tenant-a`（T-A）/ `tenant-b`（T-B）**
  > —— AC 里的 `t-audit`/`t-other` 是**占位名**，夹具统一用 `tenant-a`/`tenant-b`，
  > 避免两个需求的夹具脚本互相踩。」

### S-28: AC-S-122 与**契约 C12** 的关系没有说明

- **档位**: 表述 ｜**级别**: 低
- **问题**: design §3.1 标题写「字段**严格按契约 C12**」，但 C12 只固定**字段名**
  （13 个），**不固定类型、可空性、默认值**。
  `before`/`after` 用 `TEXT`（D141）、`actor_id` 可空、`ip`/`user_agent` `NOT NULL DEFAULT ''`
  —— 这些**都不违反** C12（C12 没说），但「严格按」这个措辞会让人以为 C12 管这些。
  → **明确结论：design §3.1 的字段设计（TEXT / `actor_id` 可空 / 无额外字段）**不违反**契约 C12。**
- **证据**: `BA/contracts/api-conventions/v1.md:165-171`（C12 原文 —— 只有字段名 + 哈希公式 + append-only）
- **建议改法**:
  > 把 design §3.1 的标题「字段**严格按契约 C12**」改为
  > 「字段**名**严格按契约 C12（13 个）；**类型与可空性由本需求自定**（C12 未约束）：
  > `before`/`after` 用 `TEXT`（D141）、`actor_id` 可空、`ip`/`user_agent` `NOT NULL DEFAULT ''`。」
  >
  > 另：契约 C12 只写了 `hash = sha256(prev_hash + 规范化事件体)`，**没有定义「规范化」** ——
  > D131 / D142 补的是这个空缺。**建议把 D131 / D142 的字节级定义回写进契约 C12**
  > （否则契约读者仍拿不到可复现的算法）。若本轮不改契约，则在 design 里注明
  > 「D131/D142 是 C12 的补充定义，尚未回写进契约」。

### S-31: AC-S-128 只点了「授予 / 撤销」，漏了 design §6.1 与 demand §5 的**创建 / 删除角色**

- **档位**: 表述 ｜**级别**: 低
- **问题**: demand §5 写「授予权限、撤销权限、**创建角色、删除角色**各产生一条审计记录」，
  design §6.1 也埋了这四个点；但 AC-S-128 的步骤只写了授予与撤销。
  → AC 覆盖不全 → 那两个埋点**没有验收判据**（做了算加分，不做也不算红）。
- **证据**: `BA/demands/REQ-014-audit/demand.md:64`；
  `BA/demands/REQ-014-audit/design-summary.md:424`（§6.1 的四个 RBAC 事件码）；
  `BA/demands/REQ-014-audit/acceptance.md:158-161`（AC-S-128 只两步）
- **建议改法**:
  > AC-S-128 追加步骤 3、4 与两条预期：
  > 「3. 创建一个角色；4. 删除该角色。
  > 预期：步骤 3 产生 **1 条** `action == "rbac.role.create"`（`before IS NULL`、`after` 非空）；
  > 步骤 4 产生 **1 条** `action == "rbac.role.delete"`（`before` 非空、`after IS NULL`），
  > `object_id` == 被删角色的 id。」

### S-32: AC-S-130 步骤 2 漏了 `objectId` 过滤维度

- **档位**: 表述 ｜**级别**: 低
- **问题**: design §5.1 的参数表里有 `objectId`，design §3.1 的索引也有
  `(tenant_id, object_type, object_id, created_at DESC)`；AC-S-130 步骤 2 只列了
  `actorId` / `action` / `objectType` / `from`/`to`。
- **证据**: `BA/demands/REQ-014-audit/design-summary.md:372`、`:171`；
  `BA/demands/REQ-014-audit/acceptance.md:198`
- **建议改法**:
  > 步骤 2 改为：「分别按 `actorId`、`action`、`objectType`、**`objectId`**、`from`/`to` 过滤」。

---

## 5. 我**实际验证过**的检查点（8 项必查 + 若干附加）

> 这一节是「不采信 BA 自查」的证据。**所有结论都可复跑。**

### 5.1 哈希复算的可执行性（必查 #1）—— **部分可行，但有一处必须钉死**

| 检查 | 结果 |
|---|---|
| Postgres `to_char(..., 'US')` 是否恰好 6 位微秒、补零？ | ✅ **是**。实测：`2026-11-20T01:13:33.339201Z`；`TIMESTAMPTZ '…03+00'` → `2026-11-20T01:02:03.000000Z` |
| `clock_timestamp()` 的微秒精度经 sqlx 往返是否丢？ | ✅ **不丢**。Postgres `timestamptz` 本身就是微秒分辨率；sqlx 的 `time` feature 映射到 `time::OffsetDateTime`（纳秒位恒为 1000 的倍数） |
| `time` crate 的 `Subsecond{6}` 是否恰好 6 位、补零？ | ✅ **是**（`time-0.3.55/src/formatting/mod.rs:786-795`） |
| `format_description!` 宏可用吗（要不要新增依赖）？ | ✅ **可用**。`cargo tree -e features -i time` → `time/macros` 由 `sqlx-core`/`sqlx-postgres` 打开 → `time-macros` 已在 `Cargo.lock:2554`。**不需要新增依赖** |
| Rust 侧会不会产出与 DB **不同**的串？ | ⚠️ **会**，两个坑：① 仓里的 `format_rfc3339_utc` / `format_timestamp` 都**只有秒精度**；② `format_description` 不含偏移分量，不先 `to_offset(Utc)` 会打印会话时区的钟点却贴 `Z` → 见 **S-07** |

### 5.2 应用角色与权限（必查 #2）—— **AC 可达，但有一处破功路径**（已实测）

| 检查 | 结果 |
|---|---|
| `base_app` 是不是表 owner？ | ❌ **不是**。owner 是 `base`（迁移执行者）→ **REVOKE 对 `base_app` 生效**，AC-S-123 **可达** ✅ |
| `base_app` 有没有该表的权限？ | ✅ 有（`base_platform_app` 是 `base_app` 的成员） |
| `base_platform_app` 能登录吗？ | ❌ **不能**（NOLOGIN）→ AC-S-123 前置的写法**连不上** → 见 **S-14** |
| 有没有别的东西把 REVOKE 盖回去？ | ⚠️ **有**：`deploy/seed/02-app-role.sql:50` + `:69-70` → **实测破功** → 见 **S-03**（**阻塞**） |

### 5.3 `app.tenant_id` 的事务局部性（必查 #3）—— **已实测，AC 步骤 3 不可达**

见 **S-04** 的实测输出（裸连接 0 行 / 单事务 1 行 / 不设 0 行）。

### 5.4 敏感键扫描器的可实现性（必查 #4）—— **宿主没定**

见 **S-19**（扫描器必须在 Rust 侧；DB 侧无法喂字符串）+ **S-20**（键集合留空）。
本轮已给出**可直接实现的**样例串与键集合建议（见 S-19 / S-20 的「建议改法」）。

### 5.5 查询参数的既有惯例（必查 #5）—— **无分页惯例；非法值走 400，且响应体不符合 C6**

见 **S-09**（axum 的 `QueryRejection` 是纯文本 body）。

### 5.6 前端的既有做法（必查 #6）—— **403 页已存在且非空白；分页模型不同**

| 检查 | 结果 |
|---|---|
| 有没有 403 页？ | ✅ 有：`apps/shell/src/pages/Forbidden.svelte`，渲染 `<h2>403</h2>` + `common.forbiddenMessage`，`data-testid="forbidden"` → **AC-S-132「403 页不是空白页」既有做法已满足** ✅ |
| 权限门禁怎么做？ | `routes.ts` 的 `permission` + `router.ts:204-227` 的 `guard()` → 不放行则改写成 `/403` 且 **`load` 不被调用**（目标接口请求计数 0） |
| 空态 / 分页怎么做？ | `ProtectedRoles.svelte` 是**客户端分页**（`createPagination` + `total={props.roles.length}`）→ 审计页要**服务端分页** → 见 **S-35** |
| E2E 的 `skip` 纪律 | dev-guide §9.9 / D108：只允许「被测系统未就绪」→ 用例 TC-014-120 守住 |

### 5.7 契约一致性（必查 #7）—— **不违反 C12；但与 C7 冲突**（见 S-02）

- ✅ design §3.1 的字段设计（`before`/`after` 用 `TEXT`、`actor_id` 可空、无额外字段）
  **不违反**契约 C12 —— C12 **只固定字段名**（13 个），不约束类型 / 可空性 / 默认值。
- ❌ 但 **AC-S-130 / design §5.1 与契约 C7 的分页/过滤/排序口径全面冲突**（5 处）→ **S-02（阻塞）**。
- ⚠️ C12 的「规范化事件体」仍**未定义**（D131/D142 是补充定义，尚未回写契约）→ 见 S-28。

### 5.8 哈希依赖（必查 #8）—— **零新增第三方依赖** ✅

- `sha2 = "0.10"` 在根 `Cargo.toml:80` → 新建 `platform-audit` **不需要新增任何第三方依赖** ✅
- `sqlx` / `axum` / `serde` / `serde_json` / `uuid` / `time` 均已在树里 ✅
- `time` 的 `macros` feature 由 `sqlx` 传递打开 → `format_description!` **可用**，**不需改** `Cargo.toml` ✅
- `Cargo.lock` 的编译单元增量：`platform-audit` 本体（+1），传递依赖 **+0**。

### 5.9 附加核查（本轮顺手确认的）

| 检查 | 结果 |
|---|---|
| 审计表已存在？ | ❌ 不存在（`to_regclass('public.platform_audit_log')` → 空）→ 与 design §1.2 一致 ✅ |
| `base_test` 库在？ | ✅ 在；`base_dev` 也存在但**本轮未触碰** |
| 既有 `INSERT INTO` 在 Rust 源码里多不多（决定 AC-S-129 扫描是否有意义）？ | 多：`platform-iam/src/store.rs` 17 处、`platform-org/src/store.rs` 7 处、`platform-rbac/src/store.rs` 6 处…… **23 个文件 71 处** → 「0 命中」是有意义的断言 ✅ |
| 仓里有没有分页端点可参照？ | ❌ **没有**（`grep 'page\|size\|limit\|cursor'` 无分页实现）→ AC-S-130 的分页**无既有惯例可依** |
| 权限点注册表会扫到 `crates/platform-audit/manifest.json` 吗？ | ✅ 会（`collect_manifest_permissions` 扫 `crates/*/manifest.json`）→ design §9 的「两处都要有」是对的 ✅ |
| 前端有 vitest 吗？ | ✅ 有：`apps/shell/test/*.test.ts` **15 个文件**，`pnpm --filter @base/shell test` |
| SSO 事件的载荷白名单？ | ✅ `sso/audit.rs:135`：`{provider, external_id, user_id, ip, user_agent}` → 适配器必须沿用 |

---

## 6. 需要 BA 拍板、**用例解决不了**的问题（12 项）

**这 12 项不钉死，对应用例只能写成「守卫」（断言写死一个 BA 尚未认可的值）。**

| # | 待拍板项 | 阻塞在 | 影响用例 | 我的建议（可直接落笔） |
|---|---|---|---|---|
| 1 | **分页/过滤/排序**跟 design §5.1 还是契约 C7？ | S-02 | 076~090、113、114 | 跟 C7（游标 + `limit=20` + `filter[...]` + `sort`）；页码分页在 C7 里登记为例外 |
| 2 | **链校验的 reason 口径**：逐条重算 + 接续校验，还是链式重算？ | S-01 | 034、036~038 | 逐条重算 + 接续校验（两个 reason 都有意义） |
| 3 | **敏感键的完整清单 + 归一化规则** | S-20 | 106、107 | 14 键清单；按完整键名匹配（**非子串**）；归一化 = 小写 + `-`→`_` |
| 4 | **敏感键检查放哪一层**（core / DB / 两边） | S-19 | 101~110 | 两边都做，键集合共用一份常量；扫描器在 Rust 侧 |
| 5 | **登录审计的 `ip` 取自 `RequestMeta.ip` 还是 `RequestContext.client_ip`？谁把 `RequestContext` 的 ip/ua 填上？** | S-05 | 054~056 | 统一到 `RequestContext.client_ip`；取值 = `x-forwarded-for` 第一跳；由审计中间件填充 |
| 6 | **列表排序字段与 tiebreaker** | S-11 | 090、114 | `ORDER BY created_at DESC, id DESC`；索引同步加 `id` |
| 7 | **归档后的链校验语义** | S-16 | 098 | 只验在线表的**连续段**；被搬走的段不参与；`checked` = 连续段条数 |
| 8 | **`tenant_id` 查询参数**是忽略还是 400 | S-33 | 088 | 400（`AUDIT_INVALID_FILTER`）；静默忽略会让调用方误以为换租户生效 |
| 9 | **`page = 0` / `size = 0`** 是 400 还是夹到 1 | S-09 | 085、087 | 都 400；「超上限被夹」与「非法值被拒」分开 |
| 10 | **归档的作用域**（全平台 / 单租户）+ xtask 的连接角色 | S-34 | 100 | 单租户；xtask **必须**用 `base_app`（超管会一次搬走所有租户的记录） |
| 11 | **审计写入失败时登录怎么办** | S-29 | 060 | fail-closed：登录失败（`503`）—— 宁可少一次登录成功，不留无审计的登录 |
| 12 | **RBAC 变更审计的事件码 + `before`/`after` 形状 + `object_type`** | S-15、S-30 | 061~068 | `rbac.role.grant`/`revoke`/`create`/`delete`；形状 = 权限码数组；`object_type == "role"` |

**另有一项需要用户裁定（BA 已登记在 decisions.md「待用户裁定」表，本轮不重复提）：**
D130 的分链取舍、D137 的显式触发 —— 两项都有安全默认，不影响验收判据。

---

## 7. 本轮**没有**做的事（避免「看起来查过了」）

| 项 | 说明 |
|---|---|
| **没有编译 / 跑任何测试** | 需求尚未实现，无从编译。本轮只做了**只读核实** + `base_test` 上的**临时表实测**（临时表已 `DROP`） |
| **没有改 `code/` 下任何文件** | 只读；`git status` 全程为空 |
| **没有执行任何用例** | 用例是**给 Dev 的输入**，不是验收结论 |
| **没有改 `status.md`** | 那是 BA 的 |
| **没有 commit** | 按派单要求，BA 统一提交 |
| **没有审 D130~D143 的「值不值得」** | 只审**可判定性**；决策层的取舍（分链强度、显式触发）只在与验收判据冲突时才提（S-25 除外，已标「决策层质疑」） |
| **没有覆盖性能 / 压测** | 13 条 AC 里没有性能判据 → 不写无判据的用例 |

---

## 8. 复核建议（给 BA 的下一步）

1. **先拍板第 6 节表里的 12 项**（其中 1、2、5 三项不拍板，Dev 一动手就会写出自相矛盾的实现）。
2. **S-03 必须先修** —— 它是**唯一**一个「Dev 第一次跑是绿的、补建脚本之后才红」的问题，
   很容易被当成环境问题而放过。修法已给（种子脚本加例外块 + AC 加一步）。
3. **S-02 需要 BA 与契约维护者一起定** —— 它不是 QA 单方面能判的：
   要么 AC 改，要么契约 C7 改。
4. 用例集已按「可机械判定」写好，**Dev 可以直接开工**；
   但标注了 `〔S-xx〕` 的 **50 条**用例里，**25 条**在 BA 拍板前**判不出红绿** ——
   P1（分页口径）单独就波及 `query-api` 的 **15 条** + `frontend` 的 **2 条**，
   P2（reason 口径）波及 `tamper` 的 **4 条**，P5（ip 取值来源）波及 `login` 的 **4 条**。
   其余 **25 条**不受拍板影响（它们验的是字段完整性、权限拒绝、RLS 隔离、敏感值拒绝等
   **不依赖任何待定口径**的判据），Dev 可以先写。
   受影响的那 25 条已在用例的「备注」里逐条点名了对应的 `S-xx`。
