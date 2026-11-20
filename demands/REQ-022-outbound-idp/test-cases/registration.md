# 登记、权限点、审计与租户隔离（AC-S-152、AC-S-153、AC-S-154、AC-S-155）

- **覆盖 AC**: AC-S-152（5 条：TC-022-109 ~ 113）、AC-S-153（5 条：TC-022-114 ~ 118）、
  AC-S-154（3 条：TC-022-119 ~ 121）、AC-S-155（3 条：TC-022-122 ~ 124）
- **前置基线**:
  - `crates/platform-oauth/` 已存在（`manifest.json` + 6 个权限点 + `src/error.rs` 的 10 个码）
  - `crates/platform-oauth/tests/contract_c5.rs` 已存在（C-5 的落点）
  - `base_test` 干净起点；`tenant-a` 与 `tenant-b` 各有用户与授权记录
- **实现事实（用例按此写）**:
  - `TITLES` 当前 **84** 行（实测；= 58 + 21 + 5），i18n 两个 locale 各 **84** 条 `error.*`
  - `platform_permission` 当前 **26** 行（实测），来源标签形如 `crates/<crate>/manifest.json`
  - `capabilities/` 下**恰好 14 个文件**（实测）；`permissions.json` 的 `total` 当前 **26**
  - `cargo xtask capabilities --check` 在**基线上就是红的**（实测 exit 1，`E_CAPABILITIES_OUTDATED`，
    根因是 `crates/xtask/src/catalog.rs:147-149` 仍写「audit_sink … 待 REQ-014」）
  - 平台现状：**31 张**表启用 RLS，policy 名统一为 `<table>_tenant_isolation`
    （`platform-db/src/rls.rs:36` 的 `POLICY_SUFFIX`）

---

## AC-S-152: 新增错误码按 8 处登记，且跨栈断言是「包含」不是「相等」

### TC-022-109: `TITLES` ⊇ `platform_oauth::error::ALL_CODES`（**包含**，不是相等）

- **关联 AC**: AC-S-152
- **前置条件**: `crates/platform-oauth/src/error.rs` 已有 `ALL_CODES`
- **测试步骤**:
  1. 读 `platform_oauth::error::ALL_CODES`
  2. 读 `platform_core::titles::TITLES` 的键集合
- **预期结果**:
  1. `ALL_CODES` **非空**，且长度 **== 10**（本需求的平台码数量，见备注）
  2. `ALL_CODES` 的**每一个**码都在 `TITLES` 的键集合里
  3. `TITLES` 的键集合**不必**等于 `ALL_CODES`（**允许**更大）—— 本用例**不**断言相等
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合包含关系
- **备注**: 〔S-28〕AC 步骤 1 的「包含」写法**是对的**（D194：两侧数字相等是巧合）。
  第 1 条的「== 10」取决于 BA 是否删掉 `OAUTH_CLIENT_SECRET_INVALID`（死码）→
  **BA 拍板后本条的数字要同步改**（9 或 10）。

### TC-022-110: 每个 locale 都有 `error.<OAUTH_*>`（**逐 locale** 检查）

- **关联 AC**: AC-S-152
- **前置条件**: 同 TC-022-109
- **测试步骤**:
  1. 对 `en-US` 与 `zh-CN` **逐个**读词库
  2. 对 `ALL_CODES` 的每个码查 `error.<CODE>`
- **预期结果**:
  1. `en-US` 的 `error.<CODE>` **全部存在**，且**值非空**（非空串）
  2. `zh-CN` 的 `error.<CODE>` **全部存在**，且**值非空**
  3. 两个 locale 的 `error.*` 键集合**各自**覆盖 `ALL_CODES`（**包含**关系，不要求相等）
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 逐 locale 包含关系
- **备注**: dev-guide §9.13 第 5 条：「跨栈断言写『**包含**』不写『相等』，并**逐 locale** 检查」。
  第 1/2 条的「值非空」抄自 `i18n_titles.rs:254-267` 的既有纪律（空串 = 登记了但没内容）。

### TC-022-111: `TITLES` 的**行数断言**与实际一致（**数字写死**）

- **关联 AC**: AC-S-152
- **前置条件**: 同 TC-022-109
- **测试步骤**:
  1. 读 `TITLES.len()`
  2. 读 `crates/platform-server/tests/i18n_titles.rs` 里的行数常量
- **预期结果**:
  1. `TITLES.len()` == 断言里算出来的值（**逐字相等**）
  2. 断言的算式**逐项写死**为 `58 + 21 + 5 + <本需求平台码数> + <本需求事件码数>`，
     **不含**「本需求新增数」这种**占位说法**
  3. 若 BA 选了「事件码**不进** TITLES」→ 期望 **94**；
     若选了「进」→ 期望 **102**
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等值断言
- **备注**: 〔S-03〕〔AC 修正〕 —— AC 原文写「基线 58 + 21 + 5 + **本需求新增数**」→
  「新增数」**不是一个数** → 不可判定。实测基线 **84** 行 → 94 或 102。
  第 3 条把**两个候选值**都列出来，BA 拍板后删掉另一个。

### TC-022-112: `ALL_CODES` 与 enum 变体**一一对应**（漏一个变体 → 变红）

- **关联 AC**: AC-S-152
- **前置条件**: 同 TC-022-109
- **测试步骤**:
  1. 读 `platform_oauth::error` 的错误码 **enum 变体集合**
  2. 读 `ALL_CODES`
  3. 比对两个集合
- **预期结果**:
  1. 两个集合**完全相等**（`==`，**不是**包含关系 —— 这一条就是要抓漏项）
  2. 每个变体的 `code()` 返回值**逐字**等于它在 `ALL_CODES` 里的那条
  3. 每个变体的 `status()` 返回一个合法的 HTTP 状态码（100~599）
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合相等
- **备注**: AC 步骤 2 的原文（「漏一个变体 → 变红」）。
  这里是**唯一**该用「相等」的地方（同一个枚举内部的自洽）→ 与 TC-022-109 的「包含」**不冲突**。

### TC-022-113: `i18n_titles.rs` 的 `all_codes()` 里**有** OAuth 那一行（且**只**有一行）

- **关联 AC**: AC-S-152
- **前置条件**: 同 TC-022-109
- **测试步骤**:
  1. 读 `crates/platform-server/tests/i18n_titles.rs` 的 `all_codes()` 源码
  2. 搜 `platform_oauth`
- **预期结果**:
  1. 源码里**有** `for code in platform_oauth::error::ALL_CODES` 这一行
  2. 这一行**只出现 1 次**（不是 0 次、不是 2 次）
  3. 该行的来源标签**不是** `platform-org::notify`
     （否则会被 `error_codes()` 当成**组织事件码**排除 → 跨栈断言失效）
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合精确匹配（源码文本）
- **备注**: 〔S-04〕第 3 条是本轮**新加**的闸：若实现者图省事把 OAuth 的码挂到
  `ORG_EVENT_SOURCE` 那个标签上 → `every_error_code_is_registered_in_every_locale`
  会**静默跳过** OAuth 的码 → 跨栈断言**形同虚设**（假绿）。

---

## AC-S-153: 权限点三处齐、启动对账 fail-fast、C-5 在本 crate 上通过

### TC-022-114: 权限点**三处齐**（manifest / 种子迁移 / 启动对账）

- **关联 AC**: AC-S-153
- **前置条件**: `crates/platform-oauth/manifest.json` 与种子迁移已就位
- **测试步骤**:
  1. 读 manifest 的 `permissions[]` 的 `code` 集合
  2. 查 `platform_permission` 表里 `source = 'crates/platform-oauth/manifest.json'` 的 `code` 集合
  3. 比对两个集合
- **预期结果**:
  1. manifest 的权限点集合**恰好 6 个**：
     `oauth:client:read` / `oauth:client:write` / `oauth:client:delete` /
     `oauth:grant:read` / `oauth:grant:revoke` / `oauth:token:introspect`
  2. 库里同 `source` 的行集合与 manifest **完全相等**（不多不少）
  3. 库里的 `source` 逐字等于 `crates/platform-oauth/manifest.json`
     （拼法见 `platform-rbac/src/registry.rs:140` 的 `format!("{dir_name}/{name}/manifest.json")`）
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合相等
- **备注**: D214 的「三处齐」+ §全局约定的「8 处登记」是**两件事**：前者是权限点，后者是错误码。
  第 3 条的 `source` 拼法是 REQ-014 的实测结论（`migrations/20261120000300_audit_permission_seed.up.sql` 的注释）。
  〔S-36〕**迁移文件名有两套**：design §8 写 `20261120000500_oauth_permission_seed.up.sql`，
  design §13 写 `20261120000600_oauth_permission_seed.up/down.sql`（而 `…0500_` 在 §13 里已是 schema 迁移）→
  **BA 必须一次定死**，否则 Dev 会建错文件名 / 建两个文件。本用例断言的是**内容**（集合相等），
  不受文件名影响 —— 但**种子迁移必须真的存在**（否则第 2 步的库里查不到行 → 变红）。

### TC-022-115: 启动对账 fail-fast —— 只改 manifest 不加种子 → 进程起不来

- **关联 AC**: AC-S-153
- **前置条件**: 临时改 `crates/platform-oauth/manifest.json`（加一个权限点），**不**改种子迁移
- **测试步骤**:
  1. 以管理员连接 `INSERT INTO platform_permission` **不加**那一行（模拟「只改 manifest」）
     —— 或直接改 manifest 后不跑种子迁移
  2. 起 `base serve`（管理员连接跑迁移、**不**跑种子）
  3. 读进程退出码与日志
- **预期结果**:
  1. 进程**退出码 != 0**（**不是** 0、**不是**「起了但带告警」）
  2. 日志里**逐条**列出缺失的权限点**编码**
  3. 日志里**含**来源 manifest 路径 `crates/platform-oauth/manifest.json`
  4. 日志里**含**「manifest 变更要同时改 manifest 与迁移」这类**可操作**提示
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 反向变异（制造不一致）
- **备注**: 对账逻辑见 `platform-rbac/src/registry.rs:197-206` 的 `require_consistent`
  + `platform-server/src/main.rs:951-963` 的 `reconcile_permissions`。
  ⚠️ 变异后**必须还原**，且还原要 `touch`（dev-guide §9.4：`cp -p` 保留旧 mtime → 复用陈旧二进制）。
  本用例要**手动起进程** → 标**半自动**。

### TC-022-116: `cargo xtask capabilities --check` 通过，且 `permissions.json` 的 `total` = 32

- **关联 AC**: AC-S-153
- **前置条件**: `crates/xtask/src/catalog.rs` 的过期文案**已修**（REQ-014 遗留债）；`capabilities/` 已重新生成
- **测试步骤**:
  1. `cargo xtask capabilities --check`
  2. 数 `capabilities/` 下的文件
  3. 读 `capabilities/permissions.json` 的 `total` 与 OAuth 的 6 个码
- **预期结果**:
  1. 步骤 1：退出码 **0**，无 `E_CAPABILITIES_OUTDATED`
  2. 步骤 2：文件数 **== 14**（**不增不减** —— 加一个 crate **不新增**索引文件）
  3. 步骤 3：`total == 32`（26 + 6）
  4. 步骤 3：`permissions.json` 里**含**那 **6 个** `oauth:*` 码（逐字）
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等值断言 + 集合包含
- **备注**: 〔S-13〕〔AC 修正〕 —— AC 原文「跑 `cargo xtask capabilities --check` … **通过**（14 个文件与源码一致，
  OAuth 的 manifest 计入）」有两处问题：
  ① **基线上这个命令就已经是红的**（实测 exit 1；根因是 `catalog.rs:147-149` 还写着
  「audit_sink … 待 REQ-014」，而 REQ-014 已合并）→ 本需求必须**先还这笔债**；
  ② 「OAuth 的 manifest 计入」如果被读成「多一个 `capabilities/oauth.json`」就错了 →
  实际是 `permissions.json` 的**内容**变（`total` 26 → 32），**文件数仍是 14**。

### TC-022-117: C-5 在 `platform-oauth` 上**通过**（真命令）

- **关联 AC**: AC-S-153
- **前置条件**: `crates/platform-oauth/tests/contract_c5.rs` 已存在
- **测试步骤**:
  1. `cargo test -p platform-oauth --test contract_c5 -- --test-threads=1`
  2. 读退出码
- **预期结果**:
  1. 退出码 **0**
  2. 测试**实际跑了 >= 1 条**（`test result: ok. N passed` 里 `N >= 1`，
     **不是** `0 passed` —— 双 `--` 会让 libtest 把 flag 当过滤串 → **假绿**）
  3. 该测试断言：6 个 `oauth:*` 权限点**各有**一个
     `require_permission("oauth:…")` **字面量**调用点
  4. 该测试断言：`scan.codes.len() == 6`（**恰好** 6 个）
  5. `scan.non_literal_calls` **为空**（参数必须是字面量）
  6. `scan.custom_entries` **为空**（没有自建鉴权入口）
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: 〔S-13〕〔AC 修正〕 —— AC 的「再跑 C-5」**没有指名命令**；
  平台 crate 的 C-5 落点是**每个 crate 一个 `tests/contract_c5.rs`**
  （范式见 `crates/platform-audit/tests/contract_c5.rs:24-64`）→
  **该文件必须由本需求新增**（补进 design §13 的文件清单）。
  第 2 条是 dev-guide §9.8 的「假绿」防线。**逐 crate 跑**（`--workspace` 必失败）。

### TC-022-118: C-5 的**阳性对照** —— 删掉一个调用点 / 放一个 `authorize` 函数 → C-5 **失败**

- **关联 AC**: AC-S-153
- **前置条件**: 同 TC-022-117；已备份被改的文件并记下 `sha256` 基线
- **测试步骤**:
  1. **变体 A**：把某个 `require_permission("oauth:client:read")` 调用点**删掉** → 跑 C-5
  2. **变体 B**：在 `crates/platform-oauth/src/` 里放一个叫 `authorize` 的函数 → 跑 C-5
  3. **还原**两个变体 → 跑 C-5
- **预期结果**:
  1. 变体 A：C-5 **失败**，失败信息里**含**那个被删掉的权限点编码
     （「已登记但无鉴权调用点」）
  2. 变体 B：C-5 **失败**，失败信息里**含** `crates/platform-oauth/src/` 下的某个 `文件:行:列`
     （D216 的命名禁区：`authorize` 在 8 个别名里）
  3. 还原后：C-5 **通过**，且被还原文件的 `sha256` 与**基线逐字节相同**
- **类型**: 可维护性
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 阳性对照（反向变异）
- **备注**: 〔S-13〕AC 步骤 3、4 的原文 —— 这两条**写得对**（带阳性对照）。
  ⚠️ dev-guide §9.4 的纪律：**还原文件后必须 `touch`**（`cp -p` 保留旧 mtime →
  cargo 判定「没变」→ 复用陈旧二进制 → 变异验证结果无效）；
  untracked 文件 `git checkout` 还原不了 → **必须备份 + 记 `sha256` 基线**。

---

## AC-S-154: 关键动作产生审计记录

### TC-022-119: 五个事件码**全部**出现，且字段集齐全

- **关联 AC**: AC-S-154
- **前置条件**: `tenant-a` 下有用户与 client；审计 sink 已接真实库
- **测试步骤**:
  1. 走一遍：注册 client → 同意授权 → 换令牌 → 撤销 → 登出
  2. 查 `platform_audit_log`（`tenant_id='tenant-a'`）的 `action` 集合
- **预期结果**:
  1. 集合**包含**这 5 个：`oauth.client.registered` / `oauth.authorization.approved` /
     `oauth.token.issued` / `oauth.token.revoked` / `oauth.logout.executed`
  2. **每个**事件码**至少 1 条**
  3. 每条含契约 C12 的固定字段集：`actor_id` / `action` / `object_type` / `object_id` /
     `ip` / `user_agent` / `created_at` / `prev_hash` / `hash`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合包含关系
- **备注**: AC 步骤 1~2 的原文。⚠️ D215 列了 **8 个**事件码，AC 只列了 **5 个** →
  本用例钉 AC 的 5 个；另 3 个（`oauth.client.updated` / `oauth.client.deleted` /
  `oauth.authorization.denied`）**建议 BA 补进 AC**（本轮先不断言）。
  契约 C12 的 13 个固定字段见 `api-conventions/v1.md`。

### TC-022-120: 审计表**不含**任何 secret / token / code 的**明文**（带阳性对照）

- **关联 AC**: AC-S-154
- **前置条件**: 同 TC-022-119；敏感值扫描器已实现且**可单独调用**
- **测试步骤**:
  1. 对 `tenant-a` 的**全部**审计行跑敏感值扫描（**所有文本列**，不只 `before`/`after`）
  2. 阳性对照：往审计表插一条**含假 secret** 的行 → 重跑扫描
  3. 删掉那条对照行 → 重跑扫描
- **预期结果**:
  1. 步骤 1：命中数 **== 0**
  2. 步骤 2：命中数 **>= 1**，且命中位置包含那一条
  3. 步骤 3：命中数 **== 0**
  4. 扫描的**敏感键集合**至少含：`client_secret` / `code` / `code_verifier` /
     `device_code` / `user_code` / `access_token` / `refresh_token` / `logout_token` /
     `authorization` / `password`（**逐字比对完整键名，非子串**）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 阳性对照
- **备注**: 〔S-35〕〔AC 修正〕 —— AC 原文「对全表做敏感值扫描，**带阳性对照**」**没定**：
  扫描器宿主、扫描面（哪些列）、阳性对照的注入点、敏感键的完整清单。
  第 4 条把键集合**列全**（REQ-014 的教训：只写「等」= 漏项）。

### TC-022-121: 审计链自洽 —— `GET /api/audit/verify` 返回 `ok = true`

- **关联 AC**: AC-S-154
- **前置条件**: 同 TC-022-119（已写入多条审计记录）
- **测试步骤**:
  1. `GET /api/audit/verify?tenantId=tenant-a`（带 `audit:verify:run` 权限的会话）
     —— 或 `cargo xtask audit verify`
  2. 读响应体
- **预期结果**:
  1. HTTP **200**
  2. `ok` 逐字等于 `true`（**布尔**）
  3. `checked` **> 0**（**真的查了**，不是「没东西可查所以通过」）
  4. `broken` 数组**为空**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 的最后一个预期（「哈希链自洽」）。第 3 条是**阳性对照**
  —— `checked == 0` 的 `ok=true` 是**假绿**（REQ-014 的 B-01 就是这个形态）。

---

## AC-S-155: OAuth 数据跨租户不可见

### TC-022-122: `tenant-a` 与 `tenant-b` **各只看得到自己**的行

- **关联 AC**: AC-S-155
- **前置条件**: `tenant-a` 与 `tenant-b` 各有用户、client 与授权记录
- **测试步骤**:
  1. 以 `base_app` 在**单个事务内** `set_config('app.tenant_id','tenant-a')` →
     查 `platform_oauth_grant` / `platform_oauth_token` / `platform_oauth_code` /
     `platform_oauth_device_code`
  2. 同样以 `tenant-b` 的上下文查这 4 张表
  3. 以 `base`（超管）查全部行
- **预期结果**:
  1. 步骤 1 的每一张表里**只**有 `tenant-a` 的行（逐行断言 `tenant_id == 'tenant-a'`）
  2. 步骤 2 的每一张表里**只**有 `tenant-b` 的行
  3. 步骤 3 能看到**两租户**的全部行（证明步骤 1、2 不是「表本来就空」）
  4. 步骤 1、2 的**每一张表**的行数 **> 0**（夹具确实在）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 阳性对照
- **备注**: AC 步骤 1~3 的原文。⚠️ **必须**在**单个事务内** `set_config`
  （租户上下文是**事务局部**的）—— 裸连接单条执行会设置失败 → 查到 0 行 → **恒真**（REQ-014 的教训）。
  第 4 条是**阳性对照**。

### TC-022-123: RLS 真在生效 —— 不 `set_config` → **0 行**；`set_config` → 有行

- **关联 AC**: AC-S-155
- **前置条件**: 同 TC-022-122；连接角色是 **`base_app`**（**不是** `base_platform_app`，它 NOLOGIN 连不上；
  **不是** `base`，超管会绕过 RLS）
- **测试步骤**:
  1. 以 `base_app` **单条**（**不**在事务里 `set_config`）查 `platform_oauth_grant` 的行数
  2. 以 `base_app` 在**单个事务内** `set_config('app.tenant_id','tenant-a')` 后查同一张表
  3. 查 `pg_roles` 里 `base_app` 的 `rolsuper` 与 `rolbypassrls`
  4. 查 `pg_policies` 里这 4 张表的 policy
- **预期结果**:
  1. 步骤 1：行数 **== 0**（RLS 默认隔离 → fail closed）
  2. 步骤 2：行数 **> 0**
  3. 步骤 3：`rolsuper` **== false**、`rolbypassrls` **== false`（不是超管、不绕过 RLS）
  4. 步骤 4：4 张表**各**有 **1 条** policy，policy 名逐字等于 `<table>_tenant_isolation`
  5. 步骤 4：这 4 张表的 `pg_tables.rowsecurity` **== true**
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 阳性对照
- **备注**: AC 的「RLS 真在生效的证据」段 —— **判据形状是对的**（QA 已实测复现：
  `base_app` 不 `set_config` → 0 行；事务内 `set_config('tenant-a')` → 有行；
  `set_config('tenant-zzz')` → 0 行）。第 3/4/5 条是 QA 补的「连接角色没有绕过 RLS」的证据。
  〔S-22〕**policy 的来源还没钉**：平台规定「RLS 策略的**唯一来源**是 manifest 的 `tenantScoped` 段」
  （`platform-db/src/scopes.rs:5`），而 design §8 的 `crates/platform-oauth/manifest.json` 模板里**没有** `tenantScoped`
  → 这 4 张表会**落在** `cargo xtask tenant-policies --check` 的覆盖范围**之外**（平台自检查不到它们）。
  → 本用例第 4/5 条把「policy 真的存在」**直接查 `pg_policies`**（不依赖平台自检）→
  即使 BA 选了「手写策略」这条路，判据仍然成立；但**建议**同时在 manifest 里补 `tenantScoped` 段。

### TC-022-124: RLS 的**写侧**（`WITH CHECK`）生效 + 平台级表**不**参与

- **关联 AC**: AC-S-155
- **前置条件**: 同 TC-022-123
- **测试步骤**:
  1. 以 `base_app` 在**单个事务内** `set_config('app.tenant_id','tenant-a')` →
     往 `platform_oauth_grant` **插入**一行 `tenant_id = 'tenant-b'` → 读结果
  2. 同一事务内插入一行 `tenant_id = 'tenant-a'` → 读结果
  3. 以 `base_app` 查 `platform_oauth_client` 的行数（**不** `set_config`）
  4. 查 `pg_tables` 里 `platform_oauth_client` 的 `rowsecurity`
- **预期结果**:
  1. 步骤 1：**插入失败**（`WITH CHECK` 拒绝；错误信息含 `row-level security`）
  2. 步骤 2：**插入成功**
  3. 步骤 3：`platform_oauth_client` 的行数 **> 0**（**平台级表，不受 RLS 约束** → 能看到全部）
  4. 步骤 4：`platform_oauth_client.rowsecurity` **== false**（**不启用** RLS，D219）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 阳性对照
- **备注**: 〔S-23〕〔AC 修正〕 —— AC 的 RLS 判据**只覆盖读侧**；
  仓里所有既有 policy 都是 `USING … WITH CHECK …` 成对（`platform-db/src/rls.rs:138-146`），
  **没有 `WITH CHECK` → 应用层能往表里写别的租户的行**（写得进去、读不出来 = 静默写侧泄漏）。
  第 3/4 条把「`platform_oauth_client` 是平台级表、**不**参与 RLS」**写进判据**
  （AC 的「补充」段提了，但主步骤容易误读）。
