# REQ-008 追溯性矩阵

- **需求**: REQ-008 用户与组织架构
- **分支**: `feature/REQ-008`
- **基线**: `develop` @ `a0a8d7a`（REQ-002 ~ REQ-007 已合并）
- **设计依据**: `BA/demands/REQ-008-user-org/design-summary.md`（D1–D12 照做）
- **契约**: api-conventions v1（C1 路径前缀 / C3 租户上下文 / C5 越权 404 / C6 错误体 / C13 命名与时间）、extension-points v1（`NotificationSender` / `Auditable`）
- **开发者**: myg133-dev-1
- **更新时间**: 2026-11-18（第一轮：实现 + 自验；**修复轮**：QA `PASS-WITH-RISK` 的 2 中 + 3 低，见 §5）

图例：**PASS** = 已实现且有自动化验证证据；**未验证** = 没有证据（如实标注）；**部分** = 本期只做了一部分。

---

## 0. 交付形态：为什么是**平台 crate**而不是业务模块

| 候选 | 结论 |
|------|------|
| `modules/org`（业务模块） | ❌ 业务模块**不得**依赖平台内部模块（`extension-points v1` §1「D7 硬约束」），而离职禁用**必须**撤销 IAM 会话 → 越界；且 `check-deps` 禁止 `platform-server`（平台 crate）依赖业务模块 → **`/api/org/*` 无法挂载** |
| **`crates/platform-org`（平台 crate）** | ✅ 与 `platform-iam` 同级同构：`platform-core ← platform-db ← platform-iam ← platform-org ← platform-server`；租户作用域表的 policy 仍由平台按 manifest 的 `tenantScoped` 段**统一生成**（模块不手写） |

> 因此 `crates/platform-org/manifest.json` 是**平台 crate 的租户作用域声明**；
> `platform-db::collect_scopes` 扩展为扫 `modules/` **与** `crates/` 两处。

---

## 1. 自有 AC（AC-S-011 ~ AC-S-019）

| AC | 内容 | 实现位置 | 验证方式与证据 | 结论 |
|----|------|----------|----------------|------|
| AC-S-011 | 组织树操作：移动后子树查询**立即**反映新结构；`path_labels` 与 `parent_id` **保持一致**（不一致以 `parent_id` 为准并可重建）；**成员关系不被移动破坏** | `crates/platform-org/src/store.rs`（`subtree_in`：**PostgreSQL 递归 CTE**，租户条件抽成 CTE `t` 以满足「`{tenant}` 只出现一次」）、`crates/platform-org/src/service.rs`（`move_node`：**一个事务**内改父节点 + 逐个重算子孙 `path_labels`；`verify_path_labels` / `rebuild_path_labels`）、`crates/platform-org/src/tree.rs`（`expected_path` / `inconsistent_nodes` / `rebuild_path`） | ① 真实库层 `moving_a_node_updates_the_subtree_immediately_and_keeps_memberships`：建 company→dept→team → 移动 dept → **子树查询立即**反映新结构（`parent_id` 已变）；`verify_path_labels` **返回空**；成员关系**仍在**（`org_node_id` 不变）；② 真实库层 `path_labels_are_detected_and_rebuilt_from_the_authoritative_parent_id`：人为写坏 `path_labels` → 校验**报出该节点** → `rebuild_path_labels` 修正 **1** 条 → 再校验**空**；③ 单测 `tree.rs::moving_a_node_changes_the_expected_path_of_its_descendants` / `inconsistent_nodes_reports_only_the_broken_ones` / `a_cycle_does_not_hang_the_path_computation` | **PASS** |
| AC-S-012 | 同步幂等：连续两次全量同步**不产生重复条目**（PUT 语义） | `crates/platform-org/src/scim.rs`（`ScimProviderConfig::user_payload` / `group_payload`；`HttpScimGateway::put_user` / `put_group` → **PUT**）、`crates/platform-org/src/service.rs`（`full_scim_sync`：**入队**即返回）、`crates/platform-org/src/entity.rs`（`ExternalIdentity`：幂等键 `(provider, external_id)`） | ① 真实库层 `two_consecutive_full_syncs_do_not_duplicate_entries`：目标端是**真的 SCIM mock HTTP 服务**（`tests/common/mod.rs`）→ 第一次同步后 User/Group **条目数**记下 → 第二次全量同步 + 推送 → **条目数不变**；② 单测 `scim.rs::pushing_the_same_user_twice_does_not_duplicate_it`（内存网关，PUT 两次 → 1 条）；③ 单测 `tests/scim_outbound.rs::put_semantics_do_not_duplicate_entries`（**真的 HTTP**） | **PASS** |
| AC-S-013 | SCIM 出站字段映射正确：按映射规则转换后的值送到目标端；**推送失败进重试且不阻塞本地请求** | `crates/platform-org/src/scim.rs`（`ScimUserFields` / `ScimGroupFields`：**字段名外置可配**；`user_payload` 按配置拼键名；`ScimOutbox`：入队即返回 + 指数退避重试 `MAX_PUSH_ATTEMPTS=3`）、`crates/platform-org/src/config.rs`（`BASE_ORG_SCIM_MAPPING`） | ① `tests/scim_outbound.rs::the_field_mapping_is_applied_to_the_payload`：映射成 `login` / `workEmail` / `cn` → mock 服务收到的请求体**逐字段断言**（且**不含**默认字段名）；② `tests/scim_outbound.rs::a_slow_target_does_not_block_the_local_request`：mock 延迟 **800ms** → **入队耗时 < 100ms**，推送本身 ≥ 700ms（**本地不等第三方**）；③ `tests/scim_outbound.rs::a_failed_push_is_retried`：失败 → 留在队列 → 退避到期 → 重试成功；④ 单测 `scim.rs::a_push_is_not_retried_forever`（上限 3 次后丢弃） | **PASS** |
| AC-S-014 | 冲突进人工队列且是**字段级**；裁决后按裁决生效；**不静默覆盖** | `crates/platform-org/src/sync.rs`（`ConflictStrategy`：**默认 `manual`**；`SyncField`：字段级粒度）、`crates/platform-org/src/service.rs`（`apply_remote_user`：`manual` → 记冲突**不改本地**；`local_wins` → 记冲突**状态 ignored**；`remote_wins` → 显式配置才覆盖；`resolve_conflict`：按裁决落值）、`crates/platform-org/src/store.rs`（`upsert_conflict`：同 `(source, external_id, field)` 只留一条） | ① 真实库层 `conflicts_are_recorded_per_field_and_applied_after_adjudication`：目录里邮箱不同 → 同步产出**恰好 1 个字段级冲突**（`field=email` / `local_value` / `remote_value` **逐字段断言**）→ **本地值未被覆盖**（manual 策略）→ 人工裁决 `take_remote` → **本地值生效** → 再同步**冲突数 = 0**；② 单测 `sync.rs::the_default_strategy_is_manual` / `an_unknown_strategy_is_rejected_instead_of_defaulting` | **PASS** |
| AC-S-015 | 离职自动禁用并撤销会话：账号禁用；**旧 refresh token 立即失效**；角色与成员关系解绑；产生审计事件 | `crates/platform-org/src/service.rs`（`disable_departed_user`：**6 步**，第 2 步是撤销全部会话）、`crates/platform-iam/src/store.rs`（`disable_user` / `revoke_all_refresh_tokens` / `revoke_all_sessions` —— **复用 REQ-007 的 family 作废**）、`crates/platform-org/src/audit.rs`（`AUDIT_EVENTS` 声明 + `AuditRecorder` 接缝）、`crates/platform-org/src/notify.rs` | ① 真实库层 `departure_disables_the_account_and_revokes_every_session`：`status=disabled` + `departed_at` 非空；`refresh_tokens_revoked=1` / `sessions_revoked=1`；**直查库**：旧 refresh 记录 `revoked_at` **非空**（**立即失效**，不是等 access TTL）；会话**全部** `revoked_at` 非空；成员关系**已解绑**；审计事件 `org.user.departed` **已产生**；通知**已发出**；② 真实库层 `the_ad_disabled_bit_triggers_the_departure_flow`（AD 位 → 离职 → 账号禁用）；③ ⚠️ **角色解绑属 REQ-009**（角色表本期还不存在）—— 见 §4 | **PASS**（角色解绑部分属 REQ-009，见 §4） |
| AC-S-016 | AD 离职位运算识别正确：按**位运算**判定，不是简单布尔比较 | `crates/platform-org/src/ldap.rs`（`AttrExpression::BitAnd` / `BitNot`；`LdapSourceConfig::map_entry` → `value_of`）、`crates/platform-org/src/entity.rs` | ① 单测 `ldap.rs::the_ad_disabled_flag_is_decided_by_bitwise_operations`：`512`→在职、`514`→离职、`546`→离职、`4098`→离职、`660`→**在职**（布尔比较会误判）、`402653184`→在职、`2`→离职；② 单测 `ldap.rs::bit_not_expresses_the_inverted_flag`（`bit_not` 语义 = 被掩码位**未**置上）；③ 真实库层 `the_ad_disabled_bit_triggers_the_departure_flow`（`userAccountControl=514` → 自动禁用） | **PASS** |
| AC-S-017 | 组织变更事件通知受影响人；**节点删除前成员已被处理**（不留悬空成员） | `crates/platform-org/src/notify.rs`（`PlatformNotifier`：**只转发**平台的 `NotificationSender`；`OrgEvent` / `OrgEventKind`）、`crates/platform-org/src/service.rs`（`add_member` / `remove_member` / `move_node` / `rename_node` / `delete_node` → `notify`；`delete_node`：**有成员且未指定迁移目标 → 拒绝删除**）、`crates/platform-org/src/entity.rs` | ① 真实库层 `members_receive_a_notification_when_their_node_moves`：节点移动 → 通知**已发出**；② 真实库层 `deleting_a_node_with_members_requires_handling_them_first`：有成员的节点 → **422 `ORG_NODE_HAS_MEMBERS`**（节点没被删）；指定 `move_members_to` → 成员**迁到目标节点** → 节点才删得掉；③ 单测 `notify.rs::the_platform_sender_is_forwarded_through_the_adapter` / `an_org_event_carries_its_recipients_and_payload` | **PASS** |
| AC-S-018 | 组织数据带租户维度：只返回本租户节点；**跨租户 id 访问被拒**；组织表按 `tenantScoped` 声明生成 policy | `crates/platform-org/src/store.rs`（全部语句带 `{tenant}`）、`crates/platform-org/manifest.json`（`tenantScoped.tables` = 6 张表）、`crates/platform-db/src/scopes.rs`（`collect_scopes`：**平台 crate 的 manifest 也参与收集**）、`crates/platform-db/src/rls.rs`（策略生成）、`migrations/20261118000700_org_schema.up.sql`（6 张表，跨表外键**带 tenant_id** → 结构上杜绝跨租户引用）、**`migrations/20261118000800_org_user_fk.up.sql`（修复轮：用户侧复合外键 `(tenant_id, user_id)` → `platform_iam_user (tenant_id, id)`）** | ① 真实库层 `organization_data_is_isolated_per_tenant`：两租户**同名**节点 → 各自只看到自己的；`find(tenant_b 的 id)` 在 tenant_a 下 → **`None`**；**绕过应用层**（不设 `app.tenant_id`）→ **0 行**（管理员连接能看到数据 → 对照组有效）；② 真实库层 `every_organization_table_has_a_tenant_policy`：**直查 `pg_class`** → 6 张表**全部** `relrowsecurity=true` 且**有 policy**；③ `cargo xtask tenant-policies --check` ✅「2 个模块声明的表都开了 RLS 且有策略」；④ **修复轮**：真实库层 `the_database_refuses_a_ghost_user_id` / `the_database_refuses_a_cross_tenant_user_id` —— 幽灵 / 跨租户 `user_id` 被**数据库**拒绝（`user_org_user_fk` 违约） | **PASS** |
| AC-S-019 | 结构可供 REQ-009 消费：`org_node` / `user_org` / `user_position` / 汇报关系**足以支撑**数据范围判定；且 REQ-008 **不实现任何权限判定逻辑** | `crates/platform-org/src/entity.rs`（`OrgNode` / `UserOrg`（`is_primary`）/ `UserPosition`）、`crates/platform-org/src/service.rs`（`subtree` / `members_of_subtree`：**本部门及下级**判定所需；`set_manager` / `manager_of`：**我的下属**判定所需）、`crates/platform-iam/src/store.rs`（`manager_id` 列） | ① 真实库层 `moving_a_node_updates_the_subtree_immediately_and_keeps_memberships`：`members_of_subtree` **按新结构**返回成员（「本部门及下级」的结构齐备）；② `platform_org::service` **无任何**「能不能访问 / 有没有权限」的判定分支（判定在 REQ-009）；③ 单测 `entity.rs::positions_are_not_an_organization_node_type`（岗位不是节点） | **PASS** |

---

## 2. 承接的父 AC（REQ-001）

| 父 AC | 内容 | 实现位置 | 验证方式与证据 | 结论 |
|-------|------|----------|----------------|------|
| AC-011 | SCIM 出站：把底座组织推给第三方 | `crates/platform-org/src/scim.rs`、`crates/platform-org/src/service.rs`（`full_scim_sync`） | 见 AC-S-012 / AC-S-013（**真的 HTTP**打到 mock SCIM 服务） | **PASS** |
| AC-012 | 同步冲突进人工队列 | `crates/platform-org/src/sync.rs`、`crates/platform-org/src/service.rs`（`resolve_conflict`）、`crates/platform-org/src/http.rs`（`POST /api/org/sync-conflict/{id}/resolve`） | 见 AC-S-014 | **PASS** |

## 3. 设计摘要 D1–D12 逐条对照

| # | 决策 | 实现位置 | 结论 |
|---|------|----------|------|
| D1 | 组织树 = 邻接表 + **递归 CTE**，**不用 ltree** | `store.rs::subtree_in`（`WITH RECURSIVE`）；迁移里**没有** `CREATE EXTENSION` | **PASS** |
| D2 | 节点类型 `company`/`dept`/`team`；**岗位不是节点** | `entity.rs::NodeType`（`all()` 只有 3 个）；`user_position` 表；`entity.rs::positions_are_not_an_organization_node_type` | **PASS** |
| D3 | 汇报关系是**人的属性**（`user.manager_id`）；用户↔节点多对多 | `platform_iam_user.manager_id`（迁移 `20261118000600`）；`user_org` 表（含 `is_primary`） | **PASS**（⚠️ 见 §4 的设计摘要内部冲突） |
| D4 | 5 张表全部带 `tenant_id` + RLS | `migrations/20261118000700_org_schema.up.sql`（6 张：5 张 + `sync_source_state`）；**修复轮** `migrations/20261118000800_org_user_fk.up.sql`（补用户侧复合外键） | **PASS**（多 1 张，见 §4） |
| D5 | `path_labels` 是加速字段，权威是 `parent_id`，可重建 | `tree.rs` + `service.rs::verify_path_labels` / `rebuild_path_labels` | **PASS** |
| D6 | SCIM 出站**自己实现**、**PUT 幂等**、**推送异步** | `scim.rs`（无 SCIM 库依赖）；`ScimOutbox` | **PASS** |
| D7 | LDAP 入站用 `ldap3`；映射**外置**；离职用**位运算** | `ldap.rs` + `Cargo.toml`（`ldap3` `default-features = false`）；**修复轮** `ldap.rs::validate_ldap_url`（scheme **连接前**校验） | **PASS**（TLS 边界见 §4） |
| D8 | 冲突默认 `manual`、**字段级**、**绝不静默覆盖** | `sync.rs::ConflictStrategy`（`Default = Manual`）、`entity.rs::SyncConflict` | **PASS** |
| D9 | 离职：禁用 + **撤销全部 session** + 解绑 + 审计 + 通知 | `service.rs::disable_departed_user` | **PASS** |
| D10 | 组织变更事件 → `NotificationSender` | `notify.rs`（`PlatformNotifier` 适配） | **PASS** |
| D11 | **不实现任何权限判定逻辑** | 全 crate 无判定分支 | **PASS** |
| D12 | 不做：同步管理界面 / SCIM 入站 / 多版本历史 | 无相关代码 | **PASS**（HTTP API 已做，界面不做） |

## 4. 偏离与「未做」登记（如实）

| # | 项 | 说明 |
|---|----|------|
| 1 | **设计摘要 §1.2 与 D3 自相矛盾** | §1.2 写「`org_node.manager_id`（直属上级）」，D3 写「汇报关系是**人的属性**（`user.manager_id`），**不是**节点的属性」。本实现按 **D3**（任务书里的冻结决策）做：`manager_id` 挂在 `platform_iam_user` 上，`org_node` **没有** `manager_id` 列 —— 否则「部门经理」与「我的上级」会混为一谈。**请 BA 确认** |
| 2 | **多 1 张表 `sync_source_state`** | 设计摘要 §9 要求交付「同步状态的可见性（上次同步时间、同步错误计数）」，D4 的 5 张表里没有承载它的地方 → 加第 6 张表 |
| 3 | **只支持明文** | SCIM 只支持 `http://`，LDAP 只支持 `ldap://`。理由：树里**无任何 TLS 栈**，引入会带 30~50 个 crate，且 `rustls` 默认后端会拉进 `ring` / `aws-lc-rs`（依赖预算要求两者 **0 命中**）。配置项已留口（`ScimProviderConfig::tls_mode` / LDAP 连接串），HTTPS/LDAPS 留后续需求 |
| 4 | **LDAP 入站不自动开号** | 目录里有、本地没有的账号**不自动创建**（计入 `skipped`）。开号涉及密码策略 / SSO 绑定 / 初始状态，属账号生命周期的决策，不该由同步顺带决定。SSO 绑定属 REQ-013 |
| 5 | **角色解绑属 REQ-009** | 离职流程解绑了**组织成员关系与岗位**；**角色**绑定表本期还不存在（REQ-009 才建），因此「解绑角色」本期**无处可解** |
| 6 | **审计落库属 REQ-014** | 本模块**声明**审计事件（`Auditable` + `AUDIT_EVENTS`）并把事件交给 `AuditRecorder` 接缝；平台的审计 sink（落库与查询）本期不存在 → 生产用 `NoopAuditRecorder`，测试用 `RecordingAuditRecorder` |
| 7 | **通知通道属 REQ-015** | 事件**照常产生**并交给平台的 `NotificationSender`；生产用 `LoggingNotifier`（写日志），测试用 `RecordingNotifier`。平台没有内置通道实现 |
| 8 | **同步的管理界面** | 按 D12 不做（数据层 + API 已做） |
| 9 | **`platform-iam` 的测试夹具被本需求改动** | `crates/platform-iam/tests/iam_real_db.rs` 的「所有会话记录都带 tenant_id」断言原本检查**整张表** —— 测试库 `base_test` 是共享的，本需求的离职用例也会写会话 → 断言误判。已改成**只看本用例的租户前缀**（`iam-test-%`） |
| 10 | **`platform-db` / `platform-iam` / `platform-server` 的改动** | `platform-db`：`TenantTx` 增 `schema()`、`collect_scopes` 增扫 `crates/`；`platform-iam`：`UserRecord` 增 3 列 + 增 `disable_user` / `set_user_manager` / `update_user_profile_field` / `list_users`、`IamState.service` 改 `Arc`；`platform-server`：挂 `/api/org/*`。**均未改 `platform-core` / `platform-contracts`**（CODEOWNERS 保护目录） |

---

## 5. 修复轮（QA `PASS-WITH-RISK` → 2 中 + 3 低）

> 输入：`feature-meta/qa-report.md`（QA 独立复现，无阻塞合并项）。范围：**只修 QA 登记的 2 中 + 3 低**。

| # | 级别 | 问题 | 实现位置 | 验证方式与证据 | 结论 |
|---|------|------|----------|----------------|------|
| **中 1** | 中 | 同步错误计数是**死代码**（失败路径不写 `sync_source_state`） | `crates/platform-org/src/store.rs::record_sync_failure` / `record_sync_failure_in`（`error_count = error_count + 1`、`last_error` 写入、`last_sync_at` **不动**）；`crates/platform-org/src/service.rs::sync_ldap`（外壳留痕）+ `sync_ldap_inner`（本体）；成功路径仍走 `upsert_sync_state`（`error_count = 0` / `last_error = NULL` / `last_sync_at = now`） | 真实库层 `a_failed_sync_records_the_error_and_a_successful_one_clears_it`：失败后 `error_count=1`、`last_error=Some("[INTERNAL_ERROR] … ldap connection failed: the directory server is unreachable")`、`last_sync_at=None`；成功一次后 `error_count=0`、`last_error=None`、`last_sync_at=Some(…)` | **已修** |
| **中 2** | 中 | `user_id` 侧**没有外键**（幽灵 / 跨租户 `user_id` 可写入） | `migrations/20261118000800_org_user_fk.up.sql`：`platform_iam_user` 加 `UNIQUE (tenant_id, id)`（复合外键的必要前提，**REQ-007 遗留缺口**）；`user_org` / `user_position` / `external_identity` 各加 `(tenant_id, user_id)` → `platform_iam_user (tenant_id, id)`。`sync_conflict` **无 `user_id` 列** → 不需要 | 真实库层 `the_database_refuses_a_ghost_user_id` / `the_database_refuses_a_cross_tenant_user_id`：**应用角色 `base_app`** 直插 → `ERROR: insert or update on table "user_org" violates foreign key constraint "user_org_user_fk"`（拒绝发生在**数据库**） | **已修** |
| **低 1** | 低 | 依赖归属不准（「SCIM 出站零新增依赖」不成立） | 无代码改动 | `verification-report.md` §3.1.2：**ldap3 = 8、SCIM 出站 = 2**（`want` / `try-lock`）；`git show a0a8d7a:Cargo.lock \| grep -cE 'name = "(want\|try-lock)"'` → **0**（基线没有） | **已更正** |
| **低 2** | 低 | 构建图绝对值口径不一致 | 无代码改动 | `verification-report.md` §3.1.1：**四个口径 + 逐字命令**（第三方去重名 263→273 / package 条目 272→284 / 构建图去重 `name vX` 239→253 / tree 行数 607→682） | **已写清** |
| **低 3** | 低 | LDAP 源缺**前置** URL 校验（`ldaps://` 落到底层 `UnknownScheme`） | `crates/platform-org/src/ldap.rs::validate_ldap_url` + `Ldap3Source::validate`；`connect()` **连接前**校验；`service.rs::map_ldap_error`（`LdapError::Config` → `INVALID_ARGUMENT` 422，与 SCIM 的 `baseUrl` 校验对称） | 单测 `the_plaintext_scheme_is_accepted` / `the_ldaps_scheme_is_rejected_with_a_clear_message` / `any_other_scheme_is_rejected_with_a_clear_message` / `the_ldap_source_rejects_an_unsupported_scheme_before_connecting`；实测三条错误信息见 `verification-report.md` §9.3（**均不含** `UnknownScheme`） | **已修** |

**修复轮回归**：Rust **299 / 299**（非 ignored）、**336 / 336**（含 ignored，真实库层 14/14）· 前端 **23/23** · E2E **1/1** ·
`scripts/test-integration.sh` **exit 0**（容器 2 → 2）· clippy **0 告警** · fmt ✅ · 4 个 xtask 门禁 ✅。

---

## 6. 复核轮（QA 复核 → **低 1 回归不稳定** + **信息 1 `add_member` 500**）

> 输入：`feature-meta/qa-report-recheck.md`（QA 复核结论 **PASS-WITH-RISK**；5 个改动点**全部复现为真**，
> 登记了 **1 个新的低危问题**：全量含 ignored 回归**间歇性死锁**（2/4 次失败）+ 2 条信息项）。
> 本轮范围：**低 1 必修** + **信息 1 一并做**。

| # | 级别 | 问题 | 实现位置 | 验证方式与证据 | 结论 |
|---|------|------|----------|----------------|------|
| **低 1** | 低 | ★ 全量含 ignored 回归**间歇性 `deadlock detected`**（QA 实测 4 次里 2 次红）。**根因不在被测逻辑**，而是**测试之间的并发竞态**：用例 A 在 `INSERT INTO user_org`（外键检查要 `org_node` 的 `RowShareLock`），用例 B 的 `setup()` 在跑 `ALTER TABLE … ENABLE ROW LEVEL SECURITY`（要 `AccessExclusiveLock`，且**先**锁 `org_node`）→ **两条路径加锁顺序相反 → 锁环** | `crates/platform-org/tests/org_real_db.rs`：新增 `static SCHEMA_READY: tokio::sync::OnceCell<()>`；`setup()` 把「应用角色 → 迁移 → RLS 策略」整段挪进 `prepare_schema()`，由 `SCHEMA_READY.get_or_init(...)` 保证**整个测试二进制只做一次**（跨进程仍受原有 `pg_advisory_lock(SETUP_LOCK)` 保护）。**DDL 因此一定发生在任何用例体开始之前 → DDL 与数据写入不再并发** | 修复前：第 1 次跑就红（`13 passed; 1 failed`，PG 日志锁环 `24320=org_node` ↔ `24337=user_org`）。修复后：**连跑 8 次，8/8 全绿**（每轮 341 passed / 0 failed，见 `verification-report.md` §10.1）。**用例体之间仍真并发**（`--test-threads=2` 不变）—— 只让**改 schema 的那一步**串行，没有把全套测试改成单线程，也没有用超时/重试盖问题 | **已修** |
| **信息 1** | 信息 | `add_member` 传**不存在的 `user_id`** → 数据库层拒绝（对的），但服务层把 FK 违反映射成 **500 `INTERNAL_ERROR`** —— 用户视角应是「这个用户不存在」 | `crates/platform-db/src/error.rs`：新增 `DbError::foreign_key_constraint()`（认出 SQLSTATE `23503` 并给出**约束名**）；`crates/platform-org/src/error.rs`：`From<DbError> for OrgError` 把外键违约映射成 **`OrgError::NotFound` → 404 `NOT_FOUND`**（`params.resource = "user"` / `"org_node"`），其它数据库错误仍是 500 | 真实库层 `adding_a_member_for_an_unknown_user_is_a_client_error`：`code=NOT_FOUND`、`status=404`、`params={"resource":"user"}`；单测 `a_foreign_key_violation_is_a_client_error_not_an_internal_one` / `other_database_errors_stay_internal`（platform-org）+ `a_foreign_key_violation_is_recognized_by_its_constraint` / `other_database_errors_are_not_foreign_key_violations`（platform-db） | **已修** |

**复核轮回归**：Rust **303 / 303**（非 ignored）、**341 / 341**（含 ignored，**连跑 8 次 8/8 全绿**）· 前端 **23/23** · E2E **1/1** ·
`scripts/test-integration.sh` **exit 0**（容器 2 → 2）· clippy **0 告警** · fmt ✅ · 4 个 xtask 门禁 ✅。
