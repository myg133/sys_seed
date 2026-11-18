# REQ-008 验证报告

- **需求**: REQ-008 用户与组织架构
- **分支**: `feature/REQ-008` @ `feature/REQ-008`（实现提交见 §0）
- **基线**: `develop` @ `a0a8d7a`（REQ-002 ~ REQ-007 已合并）
- **设计依据**: `BA/demands/REQ-008-user-org/design-summary.md`（D1–D12 照做）
- **契约**: api-conventions v1（C1 / C3 / C5 / C6 / C13）、extension-points v1
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: 第一轮（实现 + 自验）+ **第二轮（修复轮：QA `PASS-WITH-RISK` 的 2 中 + 3 低，见 §9）**

---

## 0. 一页速览

| 项 | 结果 |
|----|------|
| 分支 | `feature/REQ-008` |
| 改动 | **44 文件 / +10214 -775 行**（其中 `Cargo.lock` +181 -11；去掉锁文件是 **43 文件 / +10033 -764**）；**新建 crate `crates/platform-org`**（13 个源文件 + 3 个测试文件） |
| **★ 修复轮** | QA 结论 **PASS-WITH-RISK**（无阻塞合并项）→ 本轮修 **2 中 + 3 低**：<br>**中 1** 同步错误计数不再是死代码（失败留痕 / 成功清零）· **中 2** `user_id` 侧补复合外键（幽灵 / 跨租户 user_id 被 **DB 层**拒绝）· **低 1** 依赖归属更正（**ldap3 = 8、SCIM 出站 = 2**）· **低 2** 计数口径写死 · **低 3** LDAP scheme **连接前**校验。详见 **§9** |
| 新增第三方依赖 | **仅 `ldap3 0.11`（`default-features = false`）**。SCIM 出站**没有新增直接依赖**，但**间接引入了 2 个 crate**（`want` / `try-lock` —— `hyper-util` 的 `client-legacy` → `hyper` 的 http1 **客户端**）。⚠️ 第一轮报告写的「SCIM 出站零新增依赖」**不成立**，见 §3.1 |
| **ring / aws-lc-rs** | **0 命中** —— `cargo metadata` 里 `ring` / `aws-lc-rs` / `aws-lc-sys` 均**不存在**；唯一含 “ring” 字样的是 `stringprep`（Unicode 库，非加密库） |
| 依赖树 | **口径 A（第三方 crate 去重名）**：**263 → 273（+10）**；**口径 B（第三方 package 条目，含多版本）**：**272 → 284（+12）**；**口径 C（构建图去重 `name vX`）**：**239 → 253（+14）**；**口径 D（`cargo tree` 原始行数）**：**607 → 682（+75 行）**。**四个口径的命令都写在 §3.1**，可逐字复算 |
| TLS 方案 | **选 (a) 只支持明文**。理由：树里**无任何 TLS 栈**，引入会带 30~50 个 crate，且 `rustls` 默认后端带 `ring` / `aws-lc-rs`（依赖预算要求 0 命中）。SCIM `https://` 与 LDAP `ldaps://` **明确报错**（不静默降级），配置项已留口。**修复轮补上了 LDAP 侧的前置校验**（`ldaps://` 不再落到底层 `UnknownScheme`） |
| Rust 测试 | **299 passed / 0 failed**（非 ignored 口径；基线 225 → **+74**）；**带 `--include-ignored`：336 passed / 0 failed**（真实库层 **14/14** + SCIM **7/7**） |
| 前端测试 | **23/23**（`pnpm -r test`） |
| E2E | **1/1**（`bun run test:e2e`，chromium） |
| lint | `cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** |
| xtask 门禁 | `capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅「2 个模块声明的表都开了 RLS 且有策略」 |
| 冷编译实测 | `cargo check --workspace --all-targets` **52.7s / 1038 MB**；`cargo test --workspace --no-run`（含链接）**83.4s / 1638 MB**（`.cargo/config.toml` 的 `jobs` **保持 3**） |
| ★ 组织树 | ✅ 移动 dept → **子树查询立即**反映新结构；`path_labels` 与 `parent_id` **一致**；人为写坏 → **报得出** → **可重建**；**成员关系不破坏** |
| ★ SCIM 出站 | ✅ **真的 HTTP** 打 mock SCIM 服务：PUT 两次 → **1 条**；两次全量同步 → **条目数不变**；字段映射 `login` / `workEmail` / `cn` **逐字段生效**；目标延迟 800ms → **本地入队 < 100ms** |
| ★ LDAP 入站 | ✅ `userAccountControl` **位运算**：`512`/`660`/`402653184` 在职，`514`/`546`/`4098`/`2` 离职；映射**外置为配置**；**scheme 前置校验**（`ldap://` 放行 / `ldaps://` 与其它 scheme 明确报错） |
| ★ 冲突队列 | ✅ **字段级**（`field` / `local_value` / `remote_value`）；`manual` 策略下**本地值不被覆盖**；裁决 `take_remote` 后**按裁决生效** |
| ★ 离职禁用 | ✅ 账号禁用 + **旧 refresh 立即作废**（直查库 `revoked_at` 非空）+ 会话全撤销 + 成员解绑 + 审计 + 通知 |
| ★ 租户隔离 | ✅ 跨租户 id → `None`；**绕过应用层 → 0 行**；**直查 `pg_class`** → 6 张表**全部** `relrowsecurity=true` 且**有 policy**；**修复轮补齐用户侧外键**（幽灵 / 跨租户 `user_id` 被 DB 层拒绝） |
| **未完成 / 未验证** | 见 §6（如实登记） |

---

## 1. 交付形态：为什么是**平台 crate**

| 候选 | 结论 |
|------|------|
| `modules/org`（业务模块） | ❌ 业务模块**不得**依赖平台内部模块（`extension-points v1` §1「D7 硬约束」），而离职禁用**必须**撤销 IAM 会话；且 `check-deps` 禁止 `platform-server` 依赖业务模块 → **`/api/org/*` 无法挂载** |
| **`crates/platform-org`（平台 crate）** | ✅ 与 `platform-iam` 同级同构；租户作用域表的 policy 仍由平台按 manifest 的 `tenantScoped` 段**统一生成** |

配套改动：`platform-db::collect_scopes` 扩展为扫 `modules/` **与** `crates/` 两处；
`Dockerfile` 把平台 crate 的 manifest 也拷进镜像（否则部署期 `base tenant-policies`
看不到组织表的声明 → **那批表完全没有隔离**）。

**未改 `platform-core` / `platform-contracts`**（CODEOWNERS 保护目录，一行未动）。

---

## 2. 实测证据

### 2.1 组织树（AC-S-011）

```
moving_a_node_updates_the_subtree_immediately_and_keeps_memberships ... ok
path_labels_are_detected_and_rebuilt_from_the_authoritative_parent_id ... ok
```

- 建 `company → dept → team`，把 `dept` 移到另一个部门下：
  - `subtree(company)` 立刻反映新 `parent_id`（**权威是 `parent_id`，不是 `path_labels`**）；
  - `verify_path_labels()` 返回**空**（移动后加速字段与权威一致）；
  - 成员的 `org_node_id` **不变**（节点搬家，成员跟着走）。
- 人为把某个节点的 `path_labels` 写成 `["stale"]`：
  `verify_path_labels()` **报出该节点** → `rebuild_path_labels()` 修正 **1** 条 → 再校验**空**。

### 2.2 SCIM 出站（AC-S-012 / AC-S-013）

```
put_semantics_do_not_duplicate_entries ... ok
the_field_mapping_is_applied_to_the_payload ... ok
a_slow_target_does_not_block_the_local_request ... ok
a_failed_push_is_retried ... ok
users_can_be_queried_with_a_filter ... ok
https_is_rejected_instead_of_silently_downgraded ... ok
two_consecutive_full_syncs_do_not_duplicate_entries ... ok
```

- **真的 HTTP**：测试起一个 SCIM mock 服务（axum），用自己实现的 `HttpScimGateway` 对话
  —— 「自己实现的协议」不能只靠内存替身验证。
- **幂等**：同一个 id PUT 两次 → 目标端 **1 条**；连续两次全量同步 → 目标端 **条目数不变**。
- **字段映射**：映射成 `login` / `workEmail` / `cn`（单值形态）→ mock 收到的请求体
  **逐字段断言**，且**不含**默认字段名 `userName` / `emails`。
- **不阻塞**：mock 延迟 **800ms** → 本地**入队耗时 < 100ms**；推送本身 ≥ 700ms。
- **重试**：失败 → 留在队列 → 退避到期 → 重试成功；超过 3 次才丢弃。

### 2.3 LDAP 入站与位运算（AC-S-016）

```
the_ad_disabled_flag_is_decided_by_bitwise_operations ... ok
bit_not_expresses_the_inverted_flag ... ok
the_ad_disabled_bit_triggers_the_departure_flow ... ok
```

| `userAccountControl` | 含义 | 判定 |
|---|---|---|
| 512 | NORMAL_ACCOUNT | **在职** |
| 514 | NORMAL_ACCOUNT \| ACCOUNTDISABLE | **离职** |
| 546 | 512 \| LOCKOUT \| ACCOUNTDISABLE | **离职** |
| 4098 | DONT_REQ_PREAUTH \| ACCOUNTDISABLE | **离职** |
| **660** | 512 \| 128 \| 16 \| 4 | **在职**（布尔比较 `== 514` 会**误判**为在职/离职） |
| 402653184 | NORMAL_ACCOUNT \| DONT_EXPIRE_PASSWORD | **在职** |
| 2 | 只有 ACCOUNTDISABLE 位 | **离职** |

### 2.4 冲突队列（AC-S-014）

```
conflicts_are_recorded_per_field_and_applied_after_adjudication ... ok
```

- 目录里邮箱不同 → 同步产出**恰好 1 个字段级冲突**
  （`field=email` / `local_value=alice-…@example.com` / `remote_value=remote-…@example.com`）；
- **`manual` 策略下本地值未被覆盖**（直查账号表）；
- 人工裁决 `take_remote` → 本地值**按裁决生效**；
- 再同步 → **冲突数 = 0**（两边已一致）。

### 2.5 离职自动禁用（AC-S-015）

```
departure_disables_the_account_and_revokes_every_session ... ok
```

- `status = disabled` + `departed_at` 非空；
- `refresh_tokens_revoked = 1` / `sessions_revoked = 1`；
- **直查库**：旧 refresh 记录 `revoked_at` **非空** → **立即失效**
  （不是等 access TTL 到期）；会话**全部** `revoked_at` 非空；
- 成员关系**已解绑**（`memberships_removed = 1`）；
- 审计事件 `org.user.departed` **已产生**；通知**已发出**。

### 2.6 租户隔离与 RLS（AC-S-018）

```
organization_data_is_isolated_per_tenant ... ok
every_organization_table_has_a_tenant_policy ... ok
```

- 两个租户各有**同名**节点 → 各自只看到自己的；
- `find(tenant_b 的 id)` 在 tenant_a 下 → **`None`**（不是 403，是「不存在」）；
- **绕过应用层**（不设 `app.tenant_id`）→ **0 行**；管理员连接能看到数据（对照组有效）；
- **直查 `pg_class`**：6 张组织表**全部** `relrowsecurity = true` 且**有 policy**；
- `cargo xtask tenant-policies --check` ✅。

### 2.7 组织变更事件（AC-S-017）

```
members_receive_a_notification_when_their_node_moves ... ok
deleting_a_node_with_members_requires_handling_them_first ... ok
```

- 节点移动 → 受影响成员**收到通知**；
- 删除仍有成员的节点 → **422 `ORG_NODE_HAS_MEMBERS`**（节点**没被删**）；
- 指定 `moveMembersTo` → 成员**迁到目标节点** → 节点才删得掉（**不留悬空成员**）。

---

## 3. 资源与依赖

### 3.1 依赖树

#### 3.1.1 计数口径（**先说清楚怎么数**，否则绝对值对不上）

⚠️ 第一轮报告只写了「240 → 254」而没写命令，QA 用别的口径数出 **239 → 253** ——
**增量 +14 一致，绝对值差 1**。根因是「数的是什么」没写死。本轮把四个口径与命令**全部写死**
（下面的命令**逐字可复现**）：

| 口径 | 数的是什么 | 命令 | 基线 `develop@a0a8d7a` | 本轮 | 增量 |
|------|-----------|------|------|------|------|
| **A** | 第三方 crate **去重名**（同一 crate 的多个版本只算 1 个） | `cargo metadata --format-version 1 --all-features \| jq '. as $m \| [$m.packages[] \| select(.id as $id \| ($m.workspace_members \| index($id)) \| not) \| .name] \| unique \| length'` | **263** | **273** | **+10** |
| **B** | 第三方 **package 条目**（含多版本，`thiserror` v1/v2 各算 1 条） | 同上，去掉 `\| unique` | **272** | **284** | **+12** |
| **C** | 构建图节点（`cargo tree` 去重 `name vX`，**含工作区成员**） | `cargo tree --workspace --edges normal,build \| grep -oE '\b[a-zA-Z0-9_-]+ v[0-9][0-9A-Za-z.+-]*' \| sort -u \| wc -l` | **239** | **253** | **+14** |
| **D** | `cargo tree` **原始行数**（同一棵树画了多遍） | `cargo tree --workspace --edges normal,build \| wc -l` | **607** | **682** | **+75 行** |

- 口径 A 与 B 的差（+10 vs +12）来自 `thiserror` / `thiserror-impl` 的 **v1 线**：
  基线里这两个 crate **名**已在（v2.0.21），`ldap3` 拉的 v1.0.69 是**同一 crate 名的第二个版本**
  → 口径 A 不加、口径 B 加 2。
- 口径 C **含工作区成员**（本轮 9 个、基线 8 个 —— 本轮多了 `platform-org`）；
  去掉工作区成员则是 **244 / 231**。
- 「cargo tree 行数」（口径 D）**不是 crate 数** —— 同一节点在树里出现多次就会多行；
  拿它当「依赖个数」是错的。
- 基线数字的取法：`git archive a0a8d7a \| tar -x -C /tmp/base008` 后在 `/tmp/base008`
  里跑同一批命令（**不注册 git worktree、不动本仓库的 `target/`**）。

#### 3.1.2 +10 的**逐 crate 归属**（⚠️ 更正第一轮的说法）

第一轮写的是「新增的 10 个第三方 crate **全部**是 `ldap3` 的传递依赖」「**SCIM 出站的依赖增量 = 0**」。
**这两句都不成立**（QA 实测纠正，本轮复核确认）：

| 来源 | crate（新增的 **crate 名**） | 说明 |
|------|---------------------------|------|
| **`ldap3` 的传递依赖（8）** | `ldap3` / `lber` / `nom` / `minimal-lexical` / `futures` / `futures-macro` / `async-trait` / `tokio-util` | 与第一轮一致（第一轮把 `thiserror` v1 / `thiserror-impl` v1 也列了进去 —— 它们的 **crate 名**基线已有，只是**版本**新增，见 §3.1.1） |
| **SCIM 出站（2）** | **`want`** / **`try-lock`** | 由 `platform-org` 打开 `hyper-util` 的 **`client-legacy` + `http1`** 特性 → `hyper` 的 **http1 客户端** → `want` → `try-lock` |

**证据（可复算）**：

```bash
# 基线锁文件里根本没有这两个包：
git show a0a8d7a:Cargo.lock | grep -cE 'name = "(want|try-lock)"'   # → 0

# 基线的 hyper 只被当「服务端」用（依赖里没有 want）：
#   基线 hyper 1.12.0 deps: atomic-waker, bytes, futures-core, http, http-body,
#                          httparse, httpdate, itoa, pin-project-lite, smallvec, tokio
# 本轮 hyper 1.12.0 deps: ……多出 want → want → try-lock
cargo tree --workspace --edges normal,build -i want        # → want v0.3.2 └── hyper v1.12.0
cargo tree --workspace --edges normal,build -i try-lock    # → try-lock v0.2.5 └── want v0.3.2 └── hyper v1.12.0
```

→ **「SCIM 出站零新增依赖」是错的**（它零新增**直接依赖**，但间接引入了 **2 个 crate**）。
复用 `hyper` / `hyper-util` 的做法**本身是对的**（不引第三方 SCIM 库、TLS 栈仍然是 0 命中），
错的只是**记账归属**。依赖预算的总数 **+10 不变**。

#### 3.1.3 构建图口径的 **+14** 是怎么来的（更正第一轮的说法）

第一轮写的是「多出的 4 个是 `hyper-util` 的 `client-legacy` 特性所需
（`tower-service` / `pin-project-lite` / `futures-util` / `http-body-util`）」——
**这 4 个 crate 基线的构建图里本来就有**（实测：`tower-service` / `pin-project-lite` /
`futures-util` / `http-body-util` 在基线构建图里各命中 1 次），不是本轮新增。

`cargo tree --workspace --edges normal,build` 去重后逐个节点做差集，**+14 的真实构成**：

| 类别 | 数量 | 节点 |
|------|------|------|
| **新 crate 名** | **10** | `ldap3` / `lber` / `nom` / `minimal-lexical` / `futures` / `futures-macro` / `async-trait` / `tokio-util`（ldap3 8）+ `want` / `try-lock`（SCIM 出站 2） |
| **新工作区成员** | 1 | `platform-org v0.1.0` |
| **新版本节点**（crate 名基线已有，只是版本是新的） | 3 | `thiserror v1.0.69` / `thiserror-impl v1.0.69`（ldap3 拉的 v1 线）/ `futures-executor v0.3.34` |

差集可复算（**无删除项**）：

```bash
cargo tree --workspace --edges normal,build \
  | grep -oE '\b[a-zA-Z0-9_-]+ v[0-9][0-9A-Za-z.+-]*' | sort -u > /tmp/tree-head.txt
# 基线同法得到 /tmp/tree-base.txt 后：
comm -13 /tmp/tree-base.txt /tmp/tree-head.txt     # → 上表 14 个节点
comm -23 /tmp/tree-base.txt /tmp/tree-head.txt     # → 空（没有节点消失）
```

### 3.2 冷编译实测（`scripts/measure-cold-build.sh`，本机 8 线程 i7-8550U / 4.6G 可用）

| 操作 | 耗时 | 峰值内存 |
|------|------|----------|
| `cargo check --workspace --all-targets` | **52.7 s** | **1038 MB** |
| `cargo test --workspace --no-run`（含链接） | **83.4 s** | **1638 MB** |

- `.cargo/config.toml` 的 `jobs` **保持 3**（未动 profile、未加 linker 配置）；
- ⚠️ **链接峰值 1638 MB** 比 REQ-007 的 1250 MB 高 **388 MB**（`ldap3` → `nom` 这条链
  编译时吃内存）。仍在机器余量内（4.6G 可用），但**已越过** REQ-007 报告里的 1.5G 参考线
  —— 如实登记（见 §6）。

### 3.3 前端与 E2E

- `pnpm -r test` → **23/23**（`@base/ui` 3 + `@base/sdk` 2 + `@base/shell` 4 + 其余包）；
- `bun run test:e2e` → **1/1**（chromium，「底座壳渲染并可过滤菜单」）；
- 本需求**不含前端改动**（D12：同步的管理界面不做）。

---

## 4. 数据表（全部带 `tenant_id` + RLS）

| 表 | 关键列 | 说明 |
|----|--------|------|
| `org_node` | `id` / `parent_id` / `node_type` / `code` / `name` / `path_labels` | 组织节点（邻接表）；`CHECK (node_type IN ('company','dept','team'))`；`UNIQUE (tenant_id, code)`；自引用外键 `(tenant_id, parent_id)` |
| `user_org` | `user_id` / `org_node_id` / `role` / `is_primary` | 用户 ↔ 节点多对多；`UNIQUE (tenant_id, user_id, org_node_id)`；外键 `(tenant_id, org_node_id)` → `org_node`、**`(tenant_id, user_id)` → `platform_iam_user`**（修复轮补） |
| `user_position` | `user_id` / `position_code` / `org_node_id` | 岗位（挂在部门上的属性）；外键 `(tenant_id, org_node_id)` → `org_node`、**`(tenant_id, user_id)` → `platform_iam_user`**（修复轮补） |
| `external_identity` | `provider` / `external_id` / `user_id` / `last_synced_at` | 外部身份映射；幂等键 `UNIQUE (tenant_id, provider, external_id)`；外键 **`(tenant_id, user_id)` → `platform_iam_user`**（修复轮补） |
| `sync_conflict` | `source` / `external_id` / `field` / `local_value` / `remote_value` / `status` | 字段级冲突队列；`UNIQUE (tenant_id, source, external_id, field)`；**没有 `user_id` 列**（它按 `(source, external_id, field)` 定位**远端**条目）→ **不需要**用户侧外键 |
| `sync_source_state` | `source` / `last_sync_at` / `last_error` / `error_count` | 同步状态可见性（**D4 的 5 张表之外多这一张**，见 §6 偏离 2）；**修复轮补齐了错误路径的写入**（见 §9 中 1） |

**外键都带 `tenant_id` → 结构上杜绝跨租户引用与悬空成员关系。**

- 节点侧：`org_node_parent_fk` / `user_org_node_fk` / `user_position_node_fk`（第一轮就有）；
- 用户侧（**修复轮补齐**）：`user_org_user_fk` / `user_position_user_fk` / `external_identity_user_fk`
  → `(tenant_id, user_id)` 指向 `platform_iam_user (tenant_id, id)`；
  为此给 `platform_iam_user` 补了 `UNIQUE (tenant_id, id)`（复合外键的目标必须唯一，
  原来只有单列主键 `id` —— **REQ-007 遗留的结构缺口**）。

账号表（REQ-007 的 `platform_iam_user`）加 3 列：`display_name` / `manager_id` / `departed_at`；
修复轮再加 1 个约束：`platform_iam_user_tenant_id_uq UNIQUE (tenant_id, id)`。

---

## 5. 为 REQ-009 提供的结构

| REQ-009 需要 | 本期提供的结构 | 位置 |
|--------------|----------------|------|
| 「本部门及下级」判定 | `org_node`（邻接表）+ **递归 CTE** 查子树；`path_labels` 加速字段 | `store.rs::subtree_in`、`service.rs::subtree` / `members_of_subtree` |
| 「我的下属」判定 | `user.manager_id`（**汇报关系是人的属性**，D3） | `platform_iam_user.manager_id`、`service.rs::set_manager` / `manager_of` |
| 主部门判定 | `user_org.is_primary` | `service.rs::add_member`（一个用户只有一个主部门） |
| 岗位型权限 | `user_position`（`position_code` + 挂靠节点） | `service.rs::add_position` / `list_positions` |
| SSO 用户的权限挂载点 | `external_identity`（`provider` + `external_id` → `user_id`） | `service.rs::apply_remote_user` |

**REQ-008 不含任何权限判定逻辑**（D11）—— 全 crate 没有「能不能访问 / 有没有权限」的分支。

---

## 6. 未完成 / 未验证 / 偏离（如实登记）

| # | 项 | 说明 |
|---|----|------|
| 1 | **设计摘要 §1.2 与 D3 自相矛盾** | §1.2 写 `org_node.manager_id`，D3 写「汇报关系是**人的属性**（`user.manager_id`），**不是**节点的属性」。本实现按 **D3**：`org_node` **没有** `manager_id` 列。**请 BA 确认** |
| 2 | **多 1 张表 `sync_source_state`** | §9 要求交付「同步状态的可见性（上次同步时间、同步错误计数）」，D4 的 5 张表里没有承载位置 → 加第 6 张 |
| 3 | **只支持明文**（SCIM `http://` / LDAP `ldap://`） | 树里**无 TLS 栈**；引入会带 30~50 crate 且拉进 `ring` / `aws-lc-rs`。HTTPS / LDAPS 明确报错（不静默降级），配置项已留口 → 留后续需求 |
| 4 | **LDAP 入站不自动开号** | 目录有、本地无的账号**不自动创建**（计入 `skipped`）。开号涉及密码策略 / SSO 绑定 / 初始状态，属账号生命周期的决策 → 属 REQ-013 / 管理员动作 |
| 5 | **「解绑角色」无处可解** | 角色绑定表属 REQ-009，本期**还不存在**。离职流程已解绑**组织成员关系与岗位** |
| 6 | **审计落库未接** | 平台审计 sink（落库 + 查询）属 REQ-014 → 本期生产用 `NoopAuditRecorder`（事件**声明**齐备：`Auditable` + 9 条 `AuditEventSpec`），测试用 `RecordingAuditRecorder` **断言事件已产生** |
| 7 | **通知通道未接** | 平台通知通道属 REQ-015 → 本期生产用 `LoggingNotifier`（事件**照常产生**并写日志），测试用 `RecordingNotifier` **断言收件人与载荷** |
| 8 | **LDAP 真实服务端到端未验** | 本期用**可测的抽象层**（`LdapSource` trait）+ 内置 mock（`InMemoryLdapSource`）验证映射与位运算；
    `ldap3` 的**连接与检索**代码（`Ldap3Source`）**没有真 LDAP 服务器可验** → **未验证**。
    纯逻辑部分（`SearchEntry` → `LdapEntry` 的转换、时间戳格式化、过滤器拼装）**有单测** |
| 9 | **SCIM 出站 worker 未在服务端常驻** | 本期 `ScimOutbox` 提供 `enqueue` / `drain_once`；**后台 worker 的常驻循环**由部署侧调度（本期 `base serve` 只提供 API，
    触发全量同步后由调用方 `drain_outbox` 或后续调度器推进）→ **未验证**（无常驻推送进程） |
| 10 | **`/api/org/*` 未做真机 HTTP 冒烟** | 路由与错误体有**单测**（RFC 9457 形状 + 英文 ASCII `detail`），但**没有**起 `base serve` 打真机 HTTP
    （鉴权提取器依赖 IAM 的 access token）→ **未验证** |
| 11 | **链接峰值内存 1638 MB** | 比 REQ-007 的 1250 MB 高 388 MB（`ldap3` → `nom`）。仍在余量内，但越过 1.5G 参考线 → 如实登记 |
| 12 | **依赖树增量 +10（归属已更正）** | **+10 属实**，但归属**不是**「全部是 ldap3」：**ldap3 = 8、SCIM 出站 = 2**（`want` / `try-lock`）。四个计数口径与命令写在 §3.1，**请 BA 按同一口径记账** |
| 13 | **`platform-iam` 的测试夹具被改动** | `iam_real_db.rs` 的「所有会话记录都带 tenant_id」断言原本查**整张表** —— `base_test` 是共享库，
    本需求的离职用例也会写会话 → 断言误判。已改成只看本用例的租户前缀（`iam-test-%`） |
| 14 | **同步的管理界面** | 按 D12 不做（数据层 + API 已做） |
| 15 | **组织架构多版本历史 / SCIM 入站 / 跨租户组织共享** | 按 D12 不做 |
| 16 | **修复轮新增**：`add_member` 传入**不存在的 user_id** 现在被**数据库**拒绝 → 表现为 `500 INTERNAL_ERROR` | 用户侧外键补齐后（§9 中 2），**悬空成员关系**写不进去了；但 `add_member` **没有**服务层的「用户存在吗」校验 → 错误码是 500 而不是 404。
    本轮**按任务范围只做结构保证**（DB 层拒绝），服务层的 404 校验**留后续需求**（REQ-009 要消费 `user_org`，届时一并处理） |
| 17 | **修复轮新增**：`platform_iam_user` 加了 `UNIQUE (tenant_id, id)` | 这是复合外键的**必要前提**（PostgreSQL 要求被引用列组唯一），属于**补齐 REQ-007 遗留的结构缺口**。约束名 `platform_iam_user_tenant_id_uq` |
| 18 | **修复轮新增**：新迁移 `20261118000800_org_user_fk.*` 遇到**已有悬空成员行**的库会失败 | 这是刻意的：把脏数据暴露出来，而不是悄悄留着。`base_test` 实测**无悬空行**（`user_org` / `user_position` / `external_identity` 各 0 条） |

---

## 7. 回归与门禁（**修复轮实测**，2026-11-18）

| 门禁 | 结果 |
|------|------|
| `cargo check --workspace --all-targets` | ✅ |
| `cargo test --workspace -- --test-threads=2` | ✅ **299 passed / 0 failed / 37 ignored**（第一轮 295 → **+4**，只增不减） |
| `cargo test --workspace -- --include-ignored --test-threads=2` | ✅ **336 passed / 0 failed**（第一轮 329 → **+7**；真实库层 **14/14**，SCIM **7/7**） |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ **0 告警** |
| `cargo fmt --all --check` | ✅ |
| `cargo xtask capabilities --check` | ✅ 13 文件一致 |
| `cargo xtask check-deps` | ✅ 平台 crate 未反向依赖业务模块 |
| `cargo xtask check-modules` | ✅ |
| `cargo xtask tenant-policies --check` | ✅ 2 个模块声明的表都开了 RLS 且有策略 |
| `scripts/test-integration.sh` | ✅ **exit 0**（容器 **2 → 2**；该脚本 `--tests` 口径 = **331 passed / 0 failed**） |
| `pnpm -r test` | ✅ **23/23** |
| `bun run test:e2e` | ✅ **1/1** |

> ⚠️ **命令写法有坑**：`cargo test --workspace -- --include-ignored -- --test-threads=2`
> 里的**第二个 `--`** 会让 libtest 把 `--test-threads=2` 当成**过滤器**，
> 结果是 **0 passed / 0 failed**（**假绿**）。正确写法是
> `cargo test --workspace -- --include-ignored --test-threads=2`（**一个** `--`）。
> 本轮实测：写错时全量输出为「0 passed」；写对时 **336 passed**。

### 防假绿护栏（沿用 REQ-006/007 模式）

`crates/platform-org/tests/org_real_db.rs::missing_database_url_test_does_not_report_success`：

- 缺 `DATABASE_URL_TEST` → 子进程**非零退出** 且输出含 **`0 passed`**（不得出现「N passed」）；
- 真实库用例默认 `#[ignore]`，由 `scripts/test-integration.sh` 的 `--include-ignored` 真正跑到；
- 库名不是 `base_test` → **直接失败**（`base_dev` 是开发库，测试绝不能碰）。

> ⚠️ 第一轮**未做变异验证**（故意破坏 → 护栏变红 → 还原）—— **QA 已补做并通过**
> （见 `feature-meta/qa-report.md` §变异验证）。
> 本轮（修复轮）沿用同一个护栏，**未再重复做变异验证**。

---

## 8. 复现命令

```bash
# 依赖 / 门禁
cargo xtask capabilities --check
cargo xtask check-deps
cargo xtask check-modules
DATABASE_URL=postgres://base:base@127.0.0.1:5432/base_test cargo xtask tenant-policies --check

# 依赖（口径见 §3.1.1，归属见 §3.1.2）
cargo tree --workspace --edges normal,build \
  | grep -oE '\b[a-zA-Z0-9_-]+ v[0-9][0-9A-Za-z.+-]*' | sort -u | wc -l
git show a0a8d7a:Cargo.lock | grep -cE 'name = "(want|try-lock)"'   # → 0（基线没有这两个包）

# 测试
cargo test --workspace -- --test-threads=2
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
  cargo test --workspace -- --include-ignored --test-threads=2
bash scripts/test-integration.sh

# 前端
pnpm -r test
bun run test:e2e
```

---

## 9. 修复轮（QA `PASS-WITH-RISK` → **2 中 + 3 低**）

- **输入**：`feature-meta/qa-report.md`（QA 独立复现，10 项核心检查全部复验为真，无阻塞合并项）
- **范围**：**只修 QA 登记的 2 中 + 3 低**，不扩大 scope
- **约束遵守**：未新增任何依赖；未动 `crates/platform-core/` 与 `crates/platform-contracts/`；
  未动 `.cargo/config.toml` 与 profile；未 `cargo install` / `cargo clean`；
  测试只碰 `base_test`；应用连接用**普通角色 `base_app`**（`require_isolation` 自检通过）

| # | 级别 | 问题 | 本轮怎么修 | 证据 |
|---|------|------|-----------|------|
| **1** | **中** | 同步错误计数是**死代码**：`upsert_sync_state` 只有成功路径一个调用点，且写死 `error_count: 0` → `sync_source_state.error_count` 永远是 0、`last_error` 永远 NULL | `store.rs::record_sync_failure`（新增：`ON CONFLICT` 时 `error_count = error_count + 1`、`last_error` 写入、`last_sync_at` **不动**）；`service.rs::sync_ldap` 拆成「外壳 + 本体」：**任何失败路径**都留痕，成功路径仍写 `error_count = 0` / `last_error = NULL` / `last_sync_at = now` | 真实库层 `a_failed_sync_records_the_error_and_a_successful_one_clears_it`（见 §9.1） |
| **2** | **中** | `user_org.user_id` 侧**没有外键** → 能写**幽灵 user_id** 与**跨租户 user_id** | 新迁移 `20261118000800_org_user_fk`：`platform_iam_user` 加 `UNIQUE (tenant_id, id)`（复合外键的**必要前提**）；`user_org` / `user_position` / `external_identity` 各加 `(tenant_id, user_id)` → `platform_iam_user (tenant_id, id)` 的复合外键。`sync_conflict` **没有 `user_id` 列** → 不需要（§4 有说明） | 真实库层 `the_database_refuses_a_ghost_user_id` / `the_database_refuses_a_cross_tenant_user_id`（见 §9.2） |
| **3** | **低** | 依赖归属不准：「SCIM 出站零新增依赖」**不成立** | 报告更正为 **ldap3 = 8、SCIM 出站 = 2**（`want` / `try-lock`），并附依赖闭包证据。**代码不改**（复用 `hyper`/`hyper-util` 仍然对） | §3.1.2 |
| **4** | **低** | 构建图绝对值口径不一致（240 → 254 vs 239 → 253） | 报告写死**四个口径 + 逐字命令**，并解释「增量一致、绝对值差 1」的来源 | §3.1.1 / §3.1.3 |
| **5** | **低** | LDAP 源缺**前置** URL 校验（`ldaps://` 的错误来自底层 `UnknownScheme`） | `ldap.rs::validate_ldap_url`（新增）+ `Ldap3Source::validate`；`Ldap3Source::connect()` **连接前**先校验；`LdapError::Config` → `OrgError::InvalidArgument`（**422**，与 SCIM 的 `baseUrl` 校验对称） | 4 个单测 + 实测输出（见 §9.3） |

### 9.1 中 1 的实测输出（真实库层）

```
【失败后】error_count=1 last_error=Some("[INTERNAL_ERROR] an internal error occurred: ldap connection failed: the directory server is unreachable") last_sync_at=None
【成功后】error_count=0 last_error=None last_sync_at=Some(2026-11-18 10:02:25.238249 +00:00:00)
```

- 失败 → `error_count` **递增到 1**、`last_error` **非空**、上次**成功**同步时间**不动**（`None`）；
- 随后成功一次 → `error_count` **归 0**、`last_error` **清空**、`last_sync_at` **更新**。

### 9.2 中 2 的实测输出（**数据库层**的报错原文）

用**应用角色 `base_app`**（不绕过 RLS，与生产路径同权）直插 `user_org`：

```
--- 幽灵 user_id（应用角色）---
ERROR:  insert or update on table "user_org" violates foreign key constraint "user_org_user_fk"
DETAIL:  Key is not present in table "platform_iam_user".
--- 跨租户 user_id（应用角色）---
ERROR:  insert or update on table "user_org" violates foreign key constraint "user_org_user_fk"
DETAIL:  Key is not present in table "platform_iam_user".
```

（跨租户那条用**管理员**连接复核时，DETAIL 更具体：
`Key (tenant_id, user_id)=(tenant-a, u-other-tenant-86a95e03-…) is not present in table "platform_iam_user"`。）
→ 拒绝发生在**数据库**，**不是**服务层报错。

### 9.3 低 3 的实测输出（三种 scheme）

```
ldap://127.0.0.1:389   => OK（放行）
ldaps://127.0.0.1:636 => ldap configuration is invalid: the ldap url uses ldaps:// but only plaintext ldap:// is supported in this release (ldap over tls is planned for a later requirement)
https://dir.example.com => ldap configuration is invalid: the ldap url must start with ldap:// (unsupported scheme: https)
```

- `ldap://` → 放行；
- `ldaps://` → **明确报错**（写清「本期只支持明文、TLS 留后续需求」）；
- 其它 scheme → **明确报错**（写清实际用的 scheme）；
- 三条都**不含**底层的 `UnknownScheme`。
