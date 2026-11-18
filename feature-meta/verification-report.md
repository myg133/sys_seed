# REQ-008 验证报告

- **需求**: REQ-008 用户与组织架构
- **分支**: `feature/REQ-008` @ `feature/REQ-008`（实现提交见 §0）
- **基线**: `develop` @ `a0a8d7a`（REQ-002 ~ REQ-007 已合并）
- **设计依据**: `BA/demands/REQ-008-user-org/design-summary.md`（D1–D12 照做）
- **契约**: api-conventions v1（C1 / C3 / C5 / C6 / C13）、extension-points v1
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: 第一轮（实现 + 自验）

---

## 0. 一页速览

| 项 | 结果 |
|----|------|
| 分支 | `feature/REQ-008` |
| 改动 | **44 文件 / +10214 -775 行**（其中 `Cargo.lock` +181 -11；去掉锁文件是 **43 文件 / +10033 -764**）；**新建 crate `crates/platform-org`**（13 个源文件 + 3 个测试文件） |
| 新增第三方依赖 | **仅 `ldap3 0.11`（`default-features = false`）**。SCIM 出站**不新增任何 crate**（复用树里已有的 `hyper` / `hyper-util` / `http`） |
| **ring / aws-lc-rs** | **0 命中** —— `cargo metadata` 里 `ring` / `aws-lc-rs` / `aws-lc-sys` 均**不存在**；唯一含 “ring” 字样的是 `stringprep`（Unicode 库，非加密库） |
| 依赖树 | 第三方 **263 → 273**（`cargo metadata` 去工作区成员口径，**+10**，全部是 `ldap3` 的传递依赖）；构建图口径 `cargo tree --edges normal,build` **240 → 254**（+14：多出的 4 个是 `hyper-util` 的 `client-legacy` 特性所需，此前只在 dev 依赖里出现） |
| TLS 方案 | **选 (a) 只支持明文**。理由：树里**无任何 TLS 栈**，引入会带 30~50 个 crate，且 `rustls` 默认后端带 `ring` / `aws-lc-rs`（依赖预算要求 0 命中）。SCIM `https://` 与 LDAP `ldaps://` **明确报错**（不静默降级），配置项已留口 |
| Rust 测试 | **295 passed / 0 failed**（非 ignored 口径；基线 225 → **+70**）；**带 `--include-ignored`：329 passed / 0 failed**（真实库层 **11/11** + SCIM **7/7**） |
| 前端测试 | **23/23**（`pnpm -r test`） |
| E2E | **1/1**（`bun run test:e2e`，chromium） |
| lint | `cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** |
| xtask 门禁 | `capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅「2 个模块声明的表都开了 RLS 且有策略」 |
| 冷编译实测 | `cargo check --workspace --all-targets` **52.7s / 1038 MB**；`cargo test --workspace --no-run`（含链接）**83.4s / 1638 MB**（`.cargo/config.toml` 的 `jobs` **保持 3**） |
| ★ 组织树 | ✅ 移动 dept → **子树查询立即**反映新结构；`path_labels` 与 `parent_id` **一致**；人为写坏 → **报得出** → **可重建**；**成员关系不破坏** |
| ★ SCIM 出站 | ✅ **真的 HTTP** 打 mock SCIM 服务：PUT 两次 → **1 条**；两次全量同步 → **条目数不变**；字段映射 `login` / `workEmail` / `cn` **逐字段生效**；目标延迟 800ms → **本地入队 < 100ms** |
| ★ LDAP 入站 | ✅ `userAccountControl` **位运算**：`512`/`660`/`402653184` 在职，`514`/`546`/`4098`/`2` 离职；映射**外置为配置** |
| ★ 冲突队列 | ✅ **字段级**（`field` / `local_value` / `remote_value`）；`manual` 策略下**本地值不被覆盖**；裁决 `take_remote` 后**按裁决生效** |
| ★ 离职禁用 | ✅ 账号禁用 + **旧 refresh 立即作废**（直查库 `revoked_at` 非空）+ 会话全撤销 + 成员解绑 + 审计 + 通知 |
| ★ 租户隔离 | ✅ 跨租户 id → `None`；**绕过应用层 → 0 行**；**直查 `pg_class`** → 6 张表**全部** `relrowsecurity=true` 且**有 policy** |
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

| 口径 | 基线（`develop` @ `a0a8d7a`） | 本轮 | 增量 |
|------|------|------|------|
| `cargo metadata` 去工作区成员（第三方 crate） | **263** | **273** | **+10** |
| `cargo tree --workspace --edges normal,build`（含工作区成员） | **240** | **254** | **+14** |
| `ring` / `aws-lc-rs` / `aws-lc-sys` | **0** | **0** | **0** |

新增的 10 个第三方 crate **全部**是 `ldap3` 的传递依赖：
`ldap3` / `lber` / `nom` / `minimal-lexical` / `futures` / `futures-macro` /
`async-trait` / `thiserror`(v1) / `thiserror-impl` / `tokio-util`。

构建图口径多出的 4 个是 `hyper-util` 的 `client-legacy` 特性所需
（`tower-service` / `pin-project-lite` / `futures-util` / `http-body-util`）——
**这些 crate 早就在锁文件里**（此前只被 dev 依赖用到），打开特性**不新增任何 crate**。

**SCIM 出站的依赖增量 = 0**（复用 `hyper` / `hyper-util` / `http` / `bytes` / `http-body-util`，
全部已在树里）。

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
| `user_org` | `user_id` / `org_node_id` / `role` / `is_primary` | 用户 ↔ 节点多对多；`UNIQUE (tenant_id, user_id, org_node_id)`；外键 `(tenant_id, org_node_id)` |
| `user_position` | `user_id` / `position_code` / `org_node_id` | 岗位（挂在部门上的属性） |
| `external_identity` | `provider` / `external_id` / `user_id` / `last_synced_at` | 外部身份映射；幂等键 `UNIQUE (tenant_id, provider, external_id)` |
| `sync_conflict` | `source` / `external_id` / `field` / `local_value` / `remote_value` / `status` | 字段级冲突队列；`UNIQUE (tenant_id, source, external_id, field)` |
| `sync_source_state` | `source` / `last_sync_at` / `last_error` / `error_count` | 同步状态可见性（**D4 的 5 张表之外多这一张**，见 §6 偏离 2） |

外键都带 `tenant_id` → **结构上**杜绝跨租户挂父节点与悬空成员关系。

账号表（REQ-007 的 `platform_iam_user`）加 3 列：`display_name` / `manager_id` / `departed_at`。

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
| 12 | **依赖树增量 +10** | 全部是 `ldap3` 的传递依赖（§3.1）。预算口径「当前 233 / 基线 235」与本机实测口径不同（第三方 263 / 构建图 240），
    已在 §3.1 用**命令 + 口径**写明，请 BA 按同一口径记账 |
| 13 | **`platform-iam` 的测试夹具被改动** | `iam_real_db.rs` 的「所有会话记录都带 tenant_id」断言原本查**整张表** —— `base_test` 是共享库，
    本需求的离职用例也会写会话 → 断言误判。已改成只看本用例的租户前缀（`iam-test-%`） |
| 14 | **同步的管理界面** | 按 D12 不做（数据层 + API 已做） |
| 15 | **组织架构多版本历史 / SCIM 入站 / 跨租户组织共享** | 按 D12 不做 |

---

## 7. 回归与门禁

| 门禁 | 结果 |
|------|------|
| `cargo check --workspace --all-targets` | ✅ |
| `cargo test --workspace -- --test-threads=2` | ✅ **295 passed / 0 failed**（基线 225 → **+70**，只增不减） |
| `cargo test --workspace -- --include-ignored -- --test-threads=2` | ✅ **329 passed / 0 failed**（真实库层 **11/11**，SCIM **7/7**） |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ **0 告警** |
| `cargo fmt --all --check` | ✅ |
| `cargo xtask capabilities --check` | ✅ 13 文件一致 |
| `cargo xtask check-deps` | ✅ 平台 crate 未反向依赖业务模块 |
| `cargo xtask check-modules` | ✅ |
| `cargo xtask tenant-policies --check` | ✅ 2 个模块声明的表都开了 RLS 且有策略 |
| `scripts/test-integration.sh` | ✅ **exit 0**（容器 **2 → 2**） |
| `pnpm -r test` | ✅ **23/23** |
| `bun run test:e2e` | ✅ **1/1** |

### 防假绿护栏（沿用 REQ-006/007 模式）

`crates/platform-org/tests/org_real_db.rs::missing_database_url_test_does_not_report_success`：

- 缺 `DATABASE_URL_TEST` → 子进程**非零退出** 且输出含 **`0 passed`**（不得出现「N passed」）；
- 真实库用例默认 `#[ignore]`，由 `scripts/test-integration.sh` 的 `--include-ignored` 真正跑到；
- 库名不是 `base_test` → **直接失败**（`base_dev` 是开发库，测试绝不能碰）。

> ⚠️ 本轮**未做变异验证**（故意破坏 → 护栏变红 → 还原）。
> 护栏本身是 REQ-006 的成熟模式（同一段代码），且本轮**实测过**它会拦住：
> 开发过程中缺 `DATABASE_URL_TEST` 时真实库用例确实**失败**（而不是静默跳过）。
> 建议 QA 复核时补一次变异验证。

---

## 8. 复现命令

```bash
# 依赖 / 门禁
cargo xtask capabilities --check
cargo xtask check-deps
cargo xtask check-modules
DATABASE_URL=postgres://base:base@127.0.0.1:5432/base_test cargo xtask tenant-policies --check

# 测试
cargo test --workspace -- --test-threads=2
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
  cargo test --workspace -- --include-ignored --test-threads=2
bash scripts/test-integration.sh

# 前端
pnpm -r test
bun run test:e2e
```
