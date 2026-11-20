# REQ-014「审计日志」—— 测试用例总索引

- **需求编号**: REQ-014
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: AC-022（审计日志完整且不可篡改）
- **验收基线**: [acceptance.md](../acceptance.md) rev 2 —— **13 条 AC**
  （AC-S-022 + AC-S-122 ~ AC-S-133）
- **设计依据**: [design-summary.md](../design-summary.md) rev 1（D130~D143）
- **需求原文**: [demand.md](../demand.md)
- **代码基线**: `code/` @ `develop` @ `0683115` —— **本需求尚未实现**
- **状态**: 待实现（用例本身**尚未执行**，不得据此判定需求通过）
- **版本**: 第 1 版（rev 1）—— QA 逐条核对 acceptance rev 2 与 `code` 源码 + `base_test` 实测
- **配套挑刺报告**: [qa-req014-spec-review.md](../../../../feature-meta/qa-req014-spec-review.md)
  （**36 条**挑刺，其中 **12 条阻塞**）

> ⚠️ **本索引 §1.2 与配套的挑刺报告是同一轮产出**：
> 用例按**可机械判定**写（已对 AC 的不可执行处逐条标注 `〔S-xx〕` 与 `〔AC 修正〕`），
> 最危险的三条见 §1.2，全部 36 条的逐条证据见报告。

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **120**（TC-014-001 ~ TC-014-120，编号连续、无跳号、无重复 —— 已用脚本核对） |
| 覆盖 AC | **13 / 13**（无遗漏、无多余、无悬空 TC） |
| 用例文件 | 10 个（另本索引 1 个） |
| 类型分布 | 安全 **70** / 功能 **41** / 边界 **9** |
| 优先级分布 | P0 **95** / P1 **25** / P2 **0** |
| 自动化分布 | 可自动化 **115** / 半自动 **5** / 手工 **0** |
| 标注了挑刺编号的用例 | **50 条**（分布在 10 个文件里，见 §6） |
| 标注了 `〔AC 修正〕` 的用例 | **1 条**（TC-014-037 —— 该 AC 的期望与 design 自相矛盾） |
| 每条用例字段 | 关联 AC / 前置条件 / 测试步骤 / 预期结果 / 类型 / 优先级 / 自动化 / 设计方法 / 备注 |

### 1.1 为什么「手工 0 条」

testing.md 的测试预算原则是「**低风险**项才允许手工验证 / 暂不自动化」。
**REQ-014 里没有低风险 AC**：13 条 AC 全部带安全断言（append-only、不可篡改、
跨租户不可见、凭据不落库、越权 403、敏感键硬拒绝）。
把其中任何一条降级成手工，就等于把「忘了跑也没人知道」变成默认状态。

预算纪律在本轮是这样落实的：

- **5 条**判据本身不是断言式的（跑另两套测试、跑部署脚本、跑 xtask 子进程、跨浏览器/接口对账、
  审查 E2E 源码里有没有 `skip`）→ 标 **半自动**并写明理由，**不硬塞进常规套件**；
- **边界/并发/阳性对照**按风险加，不按覆盖率凑数（如 12 条里 4 条是同一个 `actorId` 过滤维度 ——
  因为「四个过滤维度只实现三个」是最常见的漏做，必须逐个钉死）。

### 1.2 本轮最危险的三个结论（先看这三条）

1. **append-only 会被部署脚本**静默**破功**（挑刺 S-03，已实测）。
   `deploy/seed/02-app-role.sql:50` 有一句
   `GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO base_platform_app;`
   `:69-70` 还有 `ALTER DEFAULT PRIVILEGES … GRANT … UPDATE, DELETE`。
   design §3.4 **只**在迁移里写了 `REVOKE`，**没有**在种子脚本里加例外块 ——
   而种子脚本自己已经为 `platform_config` 做了例外块（`:53-66`）。
   实测（`base_test`，临时表已清理）：新表建出来 `base_platform_app=arwd` →
   迁移 REVOKE 后 `=ar`（append-only 成立）→ **重跑种子的 blanket GRANT** → 回到 `=arwd` →
   `base_app` 的 `UPDATE` **成功执行**（无 permission denied）。
   → **AC-S-123 / AC-S-131 会在「跑过一次补建脚本」的环境里假红**，而 Dev 的第一次跑是绿的。

2. **AC-S-130 的分页/过滤/排序与契约 C7 全面冲突**（挑刺 S-02）。
   契约 C7（`BA/contracts/api-conventions/v1.md:114-124`）要求**游标分页**
   （`cursor` + `limit`、默认 `limit=20`、响应含 `next_cursor`）、
   兼容页码用 `?page=N&page_size=M`、过滤用 `?filter[field][op]=value`、排序用 `?sort=`。
   design §5.1 / D138 / AC-S-130 用的是 `page` + **`size`**（默认 **50**）、
   具名过滤参数、**没有 `sort`**、响应**没有 `next_cursor`**。
   **五条分歧**。AC 若按 design 实现 = 平台契约被破；AC 若按 C7 实现 = AC 自己写错。
   **BA 必须在两者之间拍板**，否则 Dev 只能二选一，而验的又是另一套。

3. **AC-S-125 案例 B 的 `reason` 与 design §5.2 自相矛盾**（挑刺 S-01）。
   AC 写「第 3 条 `reason == hash_mismatch`」；但 design §5.2 定义了两个 reason，
   且 D131 规定重算哈希用的是**该条自己的 `prev_hash` 列** ——
   第 3 条的 `prev_hash` 没被改 → 它**自身**的重算哈希 **等于**存储值 →
   它**不是** `hash_mismatch`，断掉的是**接续关系** → 应为 `prev_hash_mismatch`。
   若改成「链式重算」，第 3 条才是 `hash_mismatch`，但 `prev_hash_mismatch` 就**永远不可能出现**。
   → **「这证明链是连锁的」这个论证本身没错**（第 3 条确实进 broken），
   **错的是 reason 的取值**。本轮把这条拆成 TC-014-036/037/038 三条独立判据。

---

## 2. 文件清单与 AC 映射

| 文件 | 能力域 | AC 范围 | 用例数 | 编号段 |
|---|---|---|---|---|
| [write-append.md](write-append.md) | 审计写入完整（13 固定字段）+ append-only 权限 | AC-S-122、AC-S-123 | **16** | 001 ~ 016 |
| [chain.md](chain.md) | 哈希链分链、规范化、独立复算、并发不分叉 | AC-S-124 | **12** | 017 ~ 028 |
| [tamper.md](tamper.md) | 篡改检出、沿链连锁、告警日志 | AC-S-022、AC-S-125 | **14** | 029 ~ 042 |
| [tenant-isolation.md](tenant-isolation.md) | RLS 跨租户不可见（数据库强制） | AC-S-126 | **8** | 043 ~ 050 |
| [login.md](login.md) | 登录审计（本地 + SSO）与凭据不落库 | AC-S-127 | **10** | 051 ~ 060 |
| [rbac-change.md](rbac-change.md) | 权限/角色变更审计与前后值方向 | AC-S-128 | **8** | 061 ~ 068 |
| [query-api.md](query-api.md) | 统一埋点入口 + 审计查询接口 | AC-S-129、AC-S-130 | **22** | 069 ~ 090 |
| [archive.md](archive.md) | 保留策略、归档、归档表 append-only | AC-S-131 | **10** | 091 ~ 100 |
| [sensitive.md](sensitive.md) | 敏感值硬拒绝 + 扫描器阳性对照 | AC-S-133 | **10** | 101 ~ 110 |
| [frontend.md](frontend.md) | 审计查询界面（列表/分页/过滤/403） | AC-S-132 | **10** | 111 ~ 120 |
| **合计** | — | **13 条 AC** | **120** | **001 ~ 120** |

---

## 3. 追溯矩阵（AC → TC）

| AC | 主题 | 用例编号 | 用例数 | 其中 P0 | 悬空 TC |
|---|---|---|---|---|---|
| AC-S-022 | 哈希链可校验 + 篡改告警 | TC-014-029 ~ 032 | **4** | 4 | 0 |
| AC-S-122 | 审计写入完整（13 固定字段） | TC-014-001 ~ 011 | **11** | 9 | 0 |
| AC-S-123 | 审计表 append-only（权限层面） | TC-014-012 ~ 016 | **5** | 4 | 0 |
| AC-S-124 | 哈希链按租户分链且可链接 | TC-014-017 ~ 028 | **12** | 10 | 0 |
| AC-S-125 | 篡改可检测且沿链连锁 | TC-014-033 ~ 042 | **10** | 8 | 0 |
| AC-S-126 | 跨租户不可见（数据库强制） | TC-014-043 ~ 050 | **8** | 6 | 0 |
| AC-S-127 | 登录审计（成功/失败/SSO，含 IP+UA） | TC-014-051 ~ 060 | **10** | 7 | 0 |
| AC-S-128 | 权限变更审计（授予/撤销/前后值） | TC-014-061 ~ 068 | **8** | 6 | 0 |
| AC-S-129 | 统一埋点入口（业务模块不建表） | TC-014-069 ~ 075 | **7** | 6 | 0 |
| AC-S-130 | 查询接口（分页/过滤/权限控制） | TC-014-076 ~ 090 | **15** | 11 | 0 |
| AC-S-131 | 保留策略与归档（归档表 append-only） | TC-014-091 ~ 100 | **10** | 8 | 0 |
| AC-S-132 | 审计查询界面 | TC-014-111 ~ 120 | **10** | 9 | 0 |
| AC-S-133 | 敏感值防护（写入被拒绝） | TC-014-101 ~ 110 | **10** | 8 | 0 |
| **合计** | — | **TC-014-001 ~ 120** | **120** | **95** | **0** |

**核对方式（可复跑）**：

```bash
cd BA/demands/REQ-014-audit/test-cases
grep -h '^### TC-014-' *.md | sed -E 's/^### (TC-014-[0-9]+).*/\1/' | sort > /tmp/got.txt
seq -f "TC-014-%03g" 1 120 > /tmp/want.txt
diff /tmp/want.txt /tmp/got.txt        # 必须无输出
grep -h '^- \*\*关联 AC\*\*:' *.md | sed -E 's/.*: (AC-S-[0-9]+).*/\1/' | sort -u
# 必须恰好 13 行，且是 AC-S-022 + AC-S-122~133
```

---

## 4. 夹具约定

> **原则 1**：造数走管理员连接（`base`，表 owner），业务访问走应用连接（`base_app`）；
> 绝不写 `base_dev`（acceptance 全局断言 5）。
> **原则 2**：夹具走独立幂等播种脚本，不依赖其它测试的副作用。
> **原则 3**：E2E 的 `skip` 只允许「被测系统未就绪」；403/500 必须 `fail`（D108 / dev-guide §9.9）。
> **原则 4**：本轮**未执行任何用例**（需求尚未实现）—— 用例是**给 Dev 的输入**，不是验收结论。

### 4.1 租户与账号

| 标识 | 角色 | 用途 |
|---|---|---|
| `t-audit`（T-A） | 主租户 | 审计写入、链、篡改、归档 |
| `t-other`（T-B） | 对照租户 | 分链、跨租户不可见 |

| 账号 | 租户 | 权限 | 用途 |
|---|---|---|---|
| `u-1` | t-audit | 基础 | AC-S-122 的 actor |
| `u-op` | t-audit | 角色管理 | AC-S-128 的操作者 |
| `u-auditor` | t-audit | `audit:events:read` | AC-S-130 / AC-S-132 |
| `u-noperm` | t-audit | **无** `audit:events:read` | AC-S-130 步骤 3 / AC-S-132 步骤 4 |
| `u-ghost` | t-audit | 不存在 | 登录失败审计的**不可枚举**对照（TC-014-059） |

| 角色 / 权限 | 用途 |
|---|---|
| `r-1` | AC-S-128 的被操作角色 |
| `p-1` | AC-S-128 的被授予权限 |

### 4.2 连接角色（**关键**）

| 连接 | 身份 | 用途 | 证据 |
|---|---|---|---|
| `base` | 超级用户 + **表 owner** | 造数、篡改、查 ACL、跑迁移 | `deploy/seed/02-app-role.sql:31-37` |
| `base_platform_app` | **NOLOGIN** 组角色 | **不能连接**；持有权限 | `:19-23` |
| `base_app` | LOGIN，`base_platform_app` 成员 | **应用连接**（权限、RLS 生效） | `:33-40` |

> ⚠️ **RLS 对超级用户与 owner 不生效**（`deploy/RUNBOOK.md:299-305`）。
> 凡是要验「跨租户不可见」的用例，**必须**用 `base_app`；
> 用 `base` 会**假绿**（看到全表）。AC-S-126 的前置里写的是 `base_platform_app`，
> 那个角色**连不上** —— 见挑刺 S-14。

### 4.3 关键命令与环境

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"

# 本机无 psql 客户端
podman exec base-postgres psql -U base       -d base_test -tAc "<SQL>"   # 管理员
podman exec base-postgres psql -U base_app   -d base_test -tAc "<SQL>"   # 应用角色
```

### 4.4 写入路径与「构造旧记录」

- **正常写入**一律走**平台 sink**（`AuditSink` trait）—— 不得直接 `INSERT`，
  否则 `hash` / `prev_hash` 算不对（TC-014-021 会红）。
- **构造「足够旧」的记录**（TC-014-091 需要）只能**管理员直连 `INSERT`**：
  写入函数内部固定用 `clock_timestamp()`（D143），**没有**指定 `created_at` 的入口。
  → 旧记录的 `hash` / `prev_hash` 需测试侧自行计算（或用 admin 绕过链的自洽值）。
- **篡改**一律管理员直连 `UPDATE`（绕过应用），这是 AC 原文的要求。

### 4.5 哈希复算约定（TC-014-021 ~ 028）

- 测试侧**独立实现** sha256 + 长度前缀拼接，**不调用**生产代码的规范化/拼接函数。
- 复算时**直接使用列里存的 `before` / `after` 文本**，**不得**解析回 JSON 再序列化
  （design §4.2.2 的明确警告；TC-014-022 是这条纪律的守卫）。
- 时间戳文本格式：`YYYY-MM-DDTHH:MM:SS.ffffffZ`（D142 §1）。
  ⚠️ 仓里现成的 `format_rfc3339_utc`（`crates/platform-core/src/money.rs:484-495`）
  与 `platform-org/src/http.rs:248-253` 都**只有秒精度** —— **不能复用**，必须自己补 6 位微秒。
  ⚠️ 还要**先转 UTC**（`to_offset(time::UtcOffset::UTC)`，仓里三处惯例都是这么写的）：
  `format_description` 不含偏移分量，直接格式化 `OffsetDateTime` 会打印**本地时**的钟点再贴个 `Z`。
- `len(f)` 是**字节长度**；`prev_hash` 以 **64 字节文本**参与拼接；genesis = **64 个 `0`**。

---

## 5. 执行层次与命令（BA/Dev 待确认）

| 层 | 内容 | 命令 |
|---|---|---|
| `rust-unit` | 规范化 JSON、长度前缀、时间戳格式、敏感键集合 | `cargo test -p platform-audit --all-targets -- --test-threads=1` |
| `rust-integration`（真库） | 写入 / 链 / 篡改 / RLS / 归档 / append-only | 同上（需 `DATABASE_URL_TEST`） |
| `rust-integration`（权限） | 应用角色 UPDATE/DELETE 被拒 | 同上；**必须逐 crate 跑** |
| `rust-regression` | `platform-org` / `platform-iam` 既有套件全绿 | `cargo test -p platform-org --all-targets -- --test-threads=1`（再跑 `-p platform-iam`） |
| `frontend-unit` | 审计页渲染 / 过滤 / 403 页 | `pnpm --filter @base/shell test` |
| `e2e` | 审计页端到端 + 403 落点 | `pnpm playwright test e2e/audit.spec.ts` |
| `ops-script` | 种子脚本重跑后 append-only 仍成立 | 手工跑 `deploy/seed/02-app-role.sql` 后执行 TC-014-016 |

> **低配机约束**（design §14）：`cargo test` **逐 crate** 跑，`--workspace` 必失败；
> 必须带 `--all-targets`（否则 lib 测试一个都不跑、退出码仍 0 = **假绿**，dev-guide §9.8）；
> 只能有**一个** `--`；真实库用例 `-- --test-threads=1`。
> **本轮未跑任何 cargo 命令**（QA 只做只读核实 + `base_test` 上的只读/临时表实测）。

---

## 6. 本需求的假绿清单（每条都有对应用例）

| 陷阱 | 为什么在本需求特别危险 | 守它的用例 |
|---|---|---|
| 用 `base`（超管）验 RLS | RLS 对超管**不生效** → 看到全表 → 「隔离生效」假绿 | TC-014-049、TC-014-044/045 |
| 用 `base_app` 之外的角色验 append-only | 表 owner 永远能改 → 测不出任何东西 | TC-014-015、TC-014-012/013 |
| 种子脚本重跑 | blanket GRANT 把 REVOKE 盖回去 → append-only 静默破功 | **TC-014-016** |
| `set_config` 在裸连接上单条执行 | 事务局部 → 设置丢失 → 查到 0 行 → 「查不到别的租户」恒真 | TC-014-044/045/046 |
| 只断言「不含 X」不配对照组 | 「什么也没查到」也能通过 | TC-014-043、TC-014-104、TC-014-059 |
| 静态扫描只断言「0 命中」 | 扫描器坏了与真的干净，输出完全一样 | **TC-014-073、TC-014-105** |
| 复算哈希时把 JSON 解析再序列化 | 序列化差异 → 假红；或反过来掩盖真 bug | **TC-014-022** |
| 复用 `format_rfc3339_utc` 复算 | 只有秒精度 → 与 DB 的微秒串不等 → 链永远「校验失败」 | TC-014-021（备注） |
| 复用 `ProtectedRoles.svelte` 的客户端分页 | 审计量大 → 必须服务端分页 | TC-014-113 |
| 用 `skip` 掩盖 403 / 空态 | dev-guide §9.9 只允许「被测系统未就绪」 | **TC-014-120** |
| 把 `reason` 断言成「hash_mismatch」就算过 | 漏掉「接续关系断裂」这一类检出 | TC-014-037、TC-014-038 |
| 敏感键扫描用**子串**匹配 | `secretary` / `tokenizer` 误伤 → 业务方绕开防护 | TC-014-109 |
| 嵌套层敏感键不检 | `jsonb_each_text` 不递归 → 漏 | TC-014-108 |
| 拒绝时把值回显进错误信息/日志 | 拒绝本身又泄漏一次 | TC-014-110 |
| 只查 `before`/`after` 两列 | 凭据可能落在 `action` / `object_id` 等列 | TC-014-057、TC-014-058 |

---

## 7. 用例对 AC 的「可执行化修正」清单

| 用例 | AC 原文不可执行处 | 修正为 | 挑刺编号 |
|---|---|---|---|
| TC-014-015 | 前置写「应用连接角色（`base_platform_app`）」—— 该角色 NOLOGIN，**连不上** | 用 `base_app`（组角色成员）连接，并断言组角色不可登录 | S-14 |
| TC-014-021 | 步骤 4「独立复算」未说时间戳的**微秒**从哪来 | 明确 `YYYY-MM-DDTHH:MM:SS.ffffffZ` + 先转 UTC + **不用**仓里的秒精度 helper | S-07 |
| TC-014-022 | AC 只说「不得解析再序列化」，没说**怎么证明** | 加一条反例：解析再序列化得到**不同**文本 → 哈希不同 | S-07 |
| TC-014-027 | AC 完全没有并发条目，但 D143 承诺「严格递增」 | 补一条并发写入不产生相同 `prev_hash` | S-25 |
| TC-014-037 | 案例 B 第 3 条的 `reason` 与 design §5.2 冲突 | 改为 `prev_hash_mismatch`，并拆出 TC-014-038 作依据 | **S-01**〔AC 修正〕 |
| TC-014-041/042 | 「恰好 1 条 broken」在改 `prev_hash` / `created_at` 时不成立 | 只断言「`>= 1` 且含被篡改那条」 | S-11 |
| TC-014-044/045 | 步骤 2/3 用裸连接设 `app.tenant_id` → 事务结束即失效 | 改成**单事务内** `set_config` → 查询 | **S-04** |
| TC-014-046 | AC 未定义「没设租户上下文」的行为 | 补 0 行 / 不报错（fail closed） | S-04 |
| TC-014-047 | AC 只说「审计记录」，没说归档表 | 补归档表同样受 RLS | S-04 |
| TC-014-048 | AC 只验读侧 | 补写侧（`WITH CHECK`） | S-04 |
| TC-014-054 | 「均含**非空** ip」按字面（不发 `X-Forwarded-For`）不可达 | 显式带 `X-Forwarded-For: 203.0.113.9` 并断言逐字相等 | **S-05** |
| TC-014-056 | AC-S-127「非空」与 AC-S-122「空串」口径冲突 | 补「不发头时落空串」并把冲突挑明 | S-05 |
| TC-014-059 | AC 未禁「失败记录含用户名」 | 补不可枚举判据 | S-05 |
| TC-014-066 | 「`object_type` == 角色对象类型」没有字面量 | 断言「非空 + 逐字等于钉死值 + 不是 `permission`」 | S-15 |
| TC-014-072 | 步骤 4 期望「只有 sink 模块命中」，但设计里 INSERT 在 SQL 函数体内 → Rust 侧 **0 命中** | Rust 侧断言 **0 命中**；另补 TC-014-074 扫 `migrations/**.sql` 断言**恰好 1 处** | **S-08** |
| TC-014-086 | AC 未说非法参数的**响应体形状** | 断言 400 + RFC 9457 Problem Details 形状（axum 默认是纯文本）；Content-Type 跟随平台现状 `application/json`（D183） | S-09 |
| TC-014-088 | design 说「忽略**或**直接 400」→ 判据不可判定 | 二选一断言 + 标为待拍板 | S-33 |
| TC-014-090 | AC-S-132 要求「翻页不重复」，但**排序未定义** | 断言三页 id 的并集恰好 5 条、两两无交集 | S-11 |
| TC-014-093 | AC 说「逐字段相同」，没说归档表会不会多列 | 断言归档表列集合 = 在线表 **∪** `{archived_at}` | S-16 |
| TC-014-098 | 归档后的链校验语义**未定义** | 断言 `ok==true` + `checked` 减少 + 标为待拍板 | S-16 |
| TC-014-105 | AC 说「同一个敏感键扫描器」但没说扫描器宿主 | 要求扫描器**可单独调用**（→ 必须在 Rust 侧） | S-19 |
| TC-014-106 | 「敏感键」集合留空（D134 只写「等」） | 钉成 D134 明列的 **6 个**基线键 + 标为待补全 | S-20 |
| TC-014-118 | AC 只说「落 403 页」 | 补「目标接口请求计数 == 0」（既有守卫机制的硬要求） | S-35 |

---

## 8. 依赖 BA 先钉死的前提（否则对应用例只能写成「守卫」）

**这 12 项不钉死，对应断言无法机械判定。**

| # | 待钉死项 | 影响用例 | 建议取值 |
|---|---|---|---|
| P1 | **分页/过滤/排序**跟 design §5.1 还是契约 C7？ | 076 ~ 090、113、114 | **二选一并写进契约**；若跟 C7，则 `size`→`page_size`、默认 50→20、补 `sort` 与 `next_cursor` |
| P2 | **链校验的 reason 口径**：逐条重算+接续校验，还是链式重算？ | 034、036 ~ 038 | 逐条重算 + 接续校验（`hash_mismatch` / `prev_hash_mismatch` 两值都有意义） |
| P3 | **敏感键的完整清单** | 106、107 | D134 的 6 个 + `passwd`/`pwd`/`client_secret`/`refresh_token`/`authorization`/`cookie`；**匹配按完整键名，非子串** |
| P4 | **敏感键检查放在哪一层**（core / DB 函数 / 两边都做） | 101 ~ 110 | **两边都做，键集合共用一份常量**；扫描器在 Rust 侧 |
| P5 | **登录审计的 `ip` 取值来源**（`RequestMeta.ip` 还是 `RequestContext.client_ip`） | 054 ~ 056 | 统一到 `RequestContext.client_ip`；取值规则 = `x-forwarded-for` 第一跳（与 `crates/platform-iam/src/http.rs:267-269` 一致）；并规定**谁**把它填进 `RequestContext` |
| P6 | **列表排序字段与 tiebreaker** | 090、114 | `ORDER BY created_at DESC, id DESC` |
| P7 | **归档后的链校验语义** | 098 | 归档后只验**在线表剩余记录构成的连续段**；被搬走的段不参与 `checked` |
| P8 | **`tenant_id` 查询参数**是忽略还是 400 | 088 | 忽略 + 记一条 warn 日志（攻击尝试可观测） |
| P9 | **`page = 0` / `size = 0`** 是 400 还是夹到 1 | 085、087 | 都 400（`AUDIT_INVALID_FILTER`）；非法值与超上限**分开** |
| P10 | **归档的作用域**（全平台 / 单租户）与 xtask 的连接角色 | 100 | 单租户 + `base_app`（受 RLS 约束）；全平台归档需独立权限点 |
| P11 | **审计写入失败时登录怎么办** | 060 | 拒绝登录并返回 `503`（放行 = 有登录无审计，违反「不可篡改」的承诺） |
| P12 | **RBAC 变更审计的事件码与 `before`/`after` 形状** | 061 ~ 068 | 事件码用 design §6.1 的 `rbac.role.grant`/`revoke`/`create`/`delete`；形状 `{"permissions":["p-1"]}`；`object_type == "role"` |

---

## 9. 未覆盖 / 不覆盖的说明（避免「看起来全绿」）

| 项 | 为什么不覆盖 |
|---|---|
| **物理删除**超归档期（`audit.retention.archive_days`） | 本轮**只登记不执行**（design §8）→ 无可验对象 |
| **全平台单链**（跨租户整体替换不可证） | D130 明确分链，是**已声明的取舍**（记入 tech-debt）→ 不存在可验的期望 |
| **跨租户整体替换的检出** | 同上（分链的固有上限） |
| **实时告警通道**（短信/邮件/webhook） | demand 明确**不在本轮**（属 REQ-015） |
| **业务模块的写操作审计**（除 org/iam/rbac/config 外） | D136 明确本轮只埋三类；其余由业务方按统一入口自行接入 |
| **SAML / CAS 审计** | D124 范围收窄，本项目不交付 |
| **多 IdP 审计** | 属 REQ-013/REQ-022 范畴；本轮只验「SSO 事件能落库」 |
| **`ip` 的真实性** | 本仓 `ip` 唯一来源是**客户端可控的请求头**（`x-forwarded-for`）→ 只能验「取到了什么」，验不了「是不是真的」；这是**既有的架构限制**，不是本需求的缺陷 |
| **审计表自身的性能** | 无 AC 规定性能指标 → 不写无判据的性能用例 |
| **归档的定时触发** | D137 明确不引入调度器 → 只有显式触发的路径可验 |
| **前端视觉/品牌** | 属 REQ-010 的范围；本轮只验「字段齐全 / 分页 / 过滤 / 403」 |
