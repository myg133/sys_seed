# 验证报告 —— Pre-merge

- **需求**: REQ-008 用户与组织架构
- **分支**: `feature/REQ-008` @ `19cd1a7`（基线 `develop` @ `a0a8d7a`）
- **改动规模**: 44 文件 / +10215 -775；新增工作区成员 `crates/platform-org`（共 9 个）
- **验证者**: QA Agent（独立复现；**未采信 Dev 自验报告里的任何数字与结论**，全部自己重跑、自己计时、自己造破坏）
- **环境**: rustc/cargo 1.98.1 · podman + postgres 17-alpine + redis 7-alpine · pnpm 12.19.0 · bun 1.4.4 · 8 核 / 7.84G 内存（实测可用 ~4.8G）· `.cargo/config.toml` jobs=3
- **日期**: 2026-11-18
- **结论**: **PASS-WITH-RISK**

> 核心结论：**10 项核心检查项全部复现为真**，无阻塞合并的缺陷。
> 两项 BA 偏离（`org_node` 无 `manager_id`、`sync_source_state` 第 6 张表）**代码实现与裁定完全一致**。
> ★ 本轮未做的**变异验证我补做了**（§变异验证），护栏确实会变红。
>
> **没有 HIGH / 阻塞级问题。** 但有 **2 项中等问题**必须如实登记（都不影响功能正确性，但影响交付完整性与依赖记账准确性）：
>
> **#1（中）`sync_source_state.error_count` / `last_error` 永远不会写入** —— §9 交付项「同步状态的可见性（上次同步时间、**同步错误计数**）」只实现了「上次同步时间」一半，**错误计数是死代码**。
>
> **#2（中）`user_org.user_id` / `user_position.user_id` / `external_identity.user_id` 没有外键** —— Dev 报告 §4「外键都带 tenant_id → 结构上杜绝跨租户挂父节点与悬空成员关系」**只对节点侧成立**；用户侧既无外键也无人校验，我实测能写进**不存在的用户**和**任意跨租户字符串**。
>
> 依赖树：实测 **263 → 273（+10）** 总数与 Dev 一致，但**归属不全对** —— 8 个来自 `ldap3`，**2 个（`want` / `try-lock`）来自 SCIM 出站打开的 hyper HTTP/1 client**，Dev 的「+10 全部是 ldap3」与「SCIM 出站增量 = 0」都不成立（§依赖与资源判定）。
>
> **冷编译：链接峰值我实测 1471–1763 MB（多次测量带方差），Dev 报 1638 MB。**
> **QA 明确结论：`jobs = 3` 保持不变，不需要下调**（依据见 §依赖与资源判定）。
> Dev 把「越过 1.5G 参考线」当成风险登记，**但那条 1.5G 参考线是给 `cargo check` 阶段的**（`docs/dev-environment.md:205`），
> 链接阶段项目自己写的预算是 **1.5–2.5G**（`docs/dev-environment.md:25`）—— 实测值**落在自己那条带子里**，不构成越线。

---

## 追溯

| AC | 实现 | 测试 | 结论 |
|----|------|------|------|
| **AC-S-011** 组织树操作（移动 / 批量移动 / 查子树 / `path_labels` 一致、可重建、成员不破） | `crates/platform-org/src/store.rs:289-311`（递归 CTE `subtree_in`）；`src/service.rs:341-395`（`move_node`，含环检测）；`src/tree.rs`（`expected_path` / `inconsistent_nodes` / `rebuild_path`） | `tests/org_real_db.rs::moving_a_node_updates_the_subtree_immediately_and_keeps_memberships`、`::path_labels_are_detected_and_rebuilt_from_the_authoritative_parent_id`；**我自己用真 SQL 复现**（三层树 / 改 `parent_id` / 写坏 `path_labels` / 重建 / 成员不动） | **PASS** |
| **AC-S-012** 同步幂等（两次全量同步不产生重复条目，PUT 语义） | `src/scim.rs:544-558`（`put_user` / `put_group` 走 `Method::PUT`）；`src/service.rs` 全量同步入队 | `tests/scim_outbound.rs::put_semantics_do_not_duplicate_entries`、`::groups_are_pushed_idempotently_with_their_members`；`tests/org_real_db.rs::two_consecutive_full_syncs_do_not_duplicate_entries`（11/11 真实库层内） | **PASS** |
| **AC-S-013** SCIM 出站字段映射正确 + 推送异步/失败重试不阻塞本地 | `src/scim.rs`（`ScimProviderConfig::user_payload` 按 `userFields` 映射；`ScimOutbox::enqueue` 只入队；`backoff_delay` 指数退避） | `tests/scim_outbound.rs::the_field_mapping_is_applied_to_the_payload`（逐字段断言 + 断言**不含**默认字段名）、`::a_slow_target_does_not_block_the_local_request`、`::a_failed_push_is_retried` | **PASS** |
| **AC-S-014** 冲突进人工队列且是字段级；裁决后按裁决生效；不静默覆盖 | `src/service.rs`（`apply_remote_user` / `resolve_conflict`）；`src/sync.rs`（`ConflictStrategy`，默认 `manual`）；`migrations/…org_schema.up.sql:93-111`（`sync_conflict` 字段级 + `UNIQUE(tenant_id,source,external_id,field)`） | `tests/org_real_db.rs::conflicts_are_recorded_per_field_and_applied_after_adjudication`；**我自己查真库**：`field=email` / `local_value` / `remote_value` / `status` / `resolution` 四行真实数据 | **PASS** |
| **AC-S-015** 离职自动禁用 + **旧 refresh 立即失效** + 解绑 + 审计 | `src/service.rs::disable_departed_user`；复用 `platform-iam` 撤销 refresh family 与全部 session | `tests/org_real_db.rs::departure_disables_the_account_and_revokes_every_session`；**旧 refresh 真失效我另找可执行证据**：`crates/platform-iam/src/service.rs:617-621` 判 `revoked_at.is_some()` → `TokenRevoked`，由预存用例 `iam_real_db.rs::refresh_rotation_revokes_the_whole_family_on_reuse` 实测断言 **401 + `TOKEN_REVOKED`** | **PASS** |
| **AC-S-016** AD `userAccountControl` **位运算**判定 | `src/ldap.rs:114-139`（`AttrExpression::BitAnd` / `BitNot`）、`ldap.rs:247-255`（`value & mask != 0`） | `src/ldap.rs::the_ad_disabled_flag_is_decided_by_bitwise_operations` —— **7 个取值逐个断言**（512/660/402653184 在职；514/546/4098/2 离职）；`org_real_db.rs::the_ad_disabled_bit_triggers_the_departure_flow` | **PASS** |
| **AC-S-017** 组织变更事件通知受影响人；删节点前先处理成员 | `src/service.rs`（移动/改名/加减成员 → `OrgEvent` → `Notifier`）；`src/error.rs:33,159-163`（`ORG_NODE_HAS_MEMBERS` → **422**） | `org_real_db.rs::members_receive_a_notification_when_their_node_moves`、`::deleting_a_node_with_members_requires_handling_them_first` | **PASS** |
| **AC-S-018** 组织数据带租户维度（跨租户 id 拒绝 + RLS） | `migrations/…org_schema.up.sql`（6 表 + `(tenant_id, parent_id)` 复合 FK）；`crates/platform-org/manifest.json` 的 `tenantScoped`；`crates/platform-db/src/scopes.rs`（`collect_scopes` 扩到 `crates/`） | `org_real_db.rs::organization_data_is_isolated_per_tenant`、`::every_organization_table_has_a_tenant_policy`；**我自己直查 `pg_class` / `pg_policies`（6/6）+ 破坏性删 policy + 跨租户 SQL 复现** | **PASS** |
| **AC-S-019** 结构可供 REQ-009 消费；**REQ-008 不实现权限判定** | `org_node` / `user_org` / `user_position` / `platform_iam_user.manager_id`；`src/service.rs::subtree` / `members_of_subtree` / `set_manager` / `manager_of` | **我全 crate 扫 `has_permission\|can_access\|authorize\|is_admin\|role_check` 等判定分支 → 0 命中**（唯一命中是 `error.rs:155` 的 `Unauthenticated` 鉴权**认证**映射） | **PASS** |

---

## 核心检查项实测

| # | 检查项 | Dev 声称 | 复核结果 | 证据 |
|---|--------|-----------|---------|------|
| **A1** | 三层树 + 递归 CTE 查子树 | ✅ | ✅ **PASS** | 我用真 SQL 复刻 `store.rs:289-308` 的 CTE：company→dept→team(+dept2) 返回 `c1(d1)/d1(d2)/d2(d2)/t1(d3)`，深度序正确 |
| **A2** | 移动节点后子树**立即**反映新结构 | ✅ | ✅ **PASS** | 改 `parent_id`（**故意不更新 `path_labels`**）→ `subtree(d2)` 立刻返回 `d2,t1`；`subtree(d1)` 只剩 `d1`。**证明权威是 `parent_id`，CTE 不读 `path_labels`** |
| **A3** | `path_labels` 与 `parent_id` 一致；写坏→报得出→能重建 | ✅ | ✅ **PASS** | 人为把 `t1` 写成 `["stale"]` → 校验器**报出该节点**；按 `parent_id` 重建 → 回到 `["c1","d1","t1"]`；再校验 → 一致 |
| **A4** | 成员关系不被移动破坏 | ✅ | ✅ **PASS** | 移动 `t1` 前后 `user_org.org_node_id` 不变（节点搬家，成员跟着走） |
| **A5** | 汇报关系挂 `user.manager_id`；`org_node` **无** `manager_id` | ✅ | ✅ **PASS** | `information_schema`：`org_node` 恰好 **7 列**（tenant_id/id/parent_id/node_type/code/name/path_labels），`manager_id` **0 命中**；`platform_iam_user` 有 `manager_id`/`display_name`/`departed_at` |
| **B1** | 6 张表全部有 policy 且 `relrowsecurity=true` | ✅ | ✅ **PASS** | 直查 `pg_class`+`pg_policies`：6/6 表 `relrowsecurity=t`、各 1 条 `*_tenant_isolation` policy |
| **B2** | **破坏性**：删一个 policy → `tenant-policies --check` 必须失败 → 还原 | 未做 | ✅ **PASS** | `DROP POLICY org_node_tenant_isolation` → check **exit 1**：`table \`org_node\` has no isolation policy`；按原定义**逐字节**重建 → check **exit 0**；还原后 policy 定义与原始**逐字符一致** |
| **B3** | 跨租户 id 访问被拒 | ✅ | ✅ **PASS** | 以 `base_app` 实测：同 `code` 节点分处两租户 → 各只见自己；ctx=qa-t-a 查 qa-b-root → **0 行**；**不设 `app.tenant_id` → 0 行**；以 ctx=qa-t-a 写 tenant_id=qa-t-b → **RLS WITH CHECK 拒绝**（exit 1） |
| **B4** | 外键都带 tenant_id | 「结构上杜绝跨租户引用与悬空成员」 | ⚠️ **PARTIAL** | 3 条 FK **全部**带 tenant_id（`org_node_parent_fk` / `user_org_node_fk` / `user_position_node_fk`）；但 **`user_id` 侧完全没有 FK** → 见 §问题 #2 |
| **C1** | 真 HTTP mock：PUT 两次 → 目标只有 1 条 | ✅ | ✅ **PASS** | `tests/common/mod.rs` 起**真 TCP** mock（`TcpListener::bind("127.0.0.1:0")` + `axum::serve`）；`put_user` ×2 → `user_count()==1` 且 `calls().len()==2`；Group 同样 |
| **C2** | 连续两次全量同步 → 条目数不变 | ✅ | ✅ **PASS** | `org_real_db.rs::two_consecutive_full_syncs_do_not_duplicate_entries`（真实库层，11/11 内）通过 |
| **C3** | 字段映射 `login`/`workEmail`/`cn` 逐字段生效 | ✅ | ✅ **PASS** | mock 收到的 body 逐字段断言 `login`/`workEmail`/`cn`，并断言**不含** `userName`/`emails` |
| **C4** | 推送异步：目标延迟 800ms → 本地入队 < 100ms | ✅ | ✅ **PASS** | `enqueue` 耗时 < 100ms 断言通过；同一次用例里真实推送耗时 **≥ 700ms**（证明第三方确实慢、本地确实不等） |
| **C5** | 失败退避重试 | ✅ | ✅ **PASS** | 失败→留队→退避到期→重试成功；`backoff_delay` = 100/200/400ms 指数增长；超 `MAX_PUSH_ATTEMPTS` 才丢弃 |
| **D1** | `userAccountControl` **位运算**判定（7 个取值） | ✅ | ✅ **PASS** | 单测逐个断言：512/660/402653184 → **在职**；514/546/4098/2 → **离职**。实现是 `value & mask != 0`（`ldap.rs:249`），**不是布尔比较** —— 660（512\|128\|16\|4）不会误判 |
| **D2** | 属性映射**外置为配置**且可改 | ✅ | ✅ **PASS** | `LdapSourceConfig::from_json`，JSON 含 `baseDn`/`filter`/`attributes`；`disabled` 用 `{"op":"bit_and","attr":"userAccountControl","mask":2}` 表达式 |
| **D3** | ★ **LDAP 真实服务端到端** | ❌ 未验 | ⚠️ **未验证（不阻塞，判定见 §未覆盖）** | 无 LDAP 服务器可连。`Ldap3Source::connect()` / `search()` 的真实连接与检索**未跑过** |
| **E1** | 同用户某字段两边不同 → 进 `sync_conflict`（字段级） | ✅ | ✅ **PASS** | 我查真库：`field=email`、`local_value`/`remote_value` 各自独立成列、`status`、`resolution` 齐全；**不是整条记录** |
| **E2** | `manual` 下本地不被覆盖 | ✅ | ✅ **PASS** | 裁决前直查账号表，邮箱仍是本地值 |
| **E3** | 裁决 `take_remote` → 生效 → 再同步冲突数 0 | ✅ | ✅ **PASS** | 裁决后邮箱 == remote 值；再同步 `summary.conflicts == 0` |
| **E4** | **绝不静默覆盖**（造冲突必须进队列） | ✅ | ✅ **PASS** | 默认策略 `manual`；真库里存在 `status=pending` 且 `local_value` 未被改写的行 |
| **F1** | 离职：禁用 + 旧 refresh 立即作废 + 会话全撤销 + 成员解绑 + 审计 + 通知 | ✅ | ✅ **PASS** | `status=disabled` + `departed_at` 非空；`refresh_tokens_revoked=1` / `sessions_revoked=1`；真库 `revoked_at` 非空；会话全 `revoked_at` 非空；成员解绑；审计 `org.user.departed`；通知已发 |
| **F2** | ★ 「旧 refresh 立即失效」**是真的**（拿旧 refresh 换 token 必须失败） | 只查库标志位 | ✅ **PASS（补足 Dev 的缺口）** | Dev 的用例只断言 `revoked_at` 非空。**我补了可执行链路**：`platform-iam/src/service.rs:617-621` 判 `revoked_at.is_some()` → `TokenRevoked` → HTTP **401**；预存用例 `iam_real_db.rs::refresh_rotation_revokes_the_whole_family_on_reuse` **实跑**断言 `401` + body `code=="TOKEN_REVOKED"` |
| **G1** | 移动/改名/加减成员 → 受影响人收到通知 | ✅ | ✅ **PASS** | `members_receive_a_notification_when_their_node_moves` 通过（录制的 `RecordingNotifier` 断言收件人与载荷） |
| **G2** | 删节点前成员必须先处理（`422 ORG_NODE_HAS_MEMBERS`） | ✅ | ✅ **PASS** | 有成员的节点删除 → 错误码 `ORG_NODE_HAS_MEMBERS`；`error.rs:159-163` 映射 **422 UNPROCESSABLE_ENTITY**；指定迁移目标 → 成员迁走 → 节点才删得掉（不留悬空） |
| **H1** | 依赖树 263 → 273（+10，全是 ldap3） | ✅ 数字 / ⚠️ 归属 | ✅ **PASS（总数）/ ❌ 归属** | 实测第三方**去重 crate 名** 263 → 273（+10）✅；但 **8 个来自 ldap3，2 个（`want`/`try-lock`）来自 SCIM 出站的 hyper 栈** → §问题 #3 |
| **H2** | `ring`/`aws-lc-rs`/`aws-lc-sys` **0 命中** | ✅ | ✅ **PASS** | `cargo tree --workspace --all-features \| grep -oE "\b(ring\|aws-lc-rs\|aws-lc-sys) v"` → **命中 0** |
| **H3** | `ldap3` 不拉 `native-tls`/OpenSSL | ✅ | ✅ **PASS** | 构建图 `--edges normal,build` 中 `native-tls\|openssl\|openssl-sys` → **0 命中**；根 `Cargo.toml:93` = `ldap3 = { version = "0.11", default-features = false }`（`ldap3` 的 `default` = `["sync","tls"]`，**必须**关掉） |
| **H4** | SCIM 出站**真的**零新增依赖 | ✅ | ❌ **FAIL** | `want`、`try-lock` **不在基线 `Cargo.lock`**（`git show a0a8d7a:Cargo.lock` 无这两个包）→ §问题 #3 |
| **H5** | TLS 方案 (a)：`https://` 与 `ldaps://` **明确报错**，不静默降级 | ✅ | ✅ **PASS** | `https://` → `ScimError::Config`（`scim.rs:196-211`，连接前即拒）；`ldaps://` → 我用**独立一次性工程**（`ldap3` 同版本 + `default-features=false`）实测：`ldaps://127.0.0.1:636 => ERROR: unknown LDAP URL scheme: ldaps`、`ldap://…` → 走明文 TCP 连接、**没有任何静默降级** |
| **H6** | 冷编译实测；**1638MB 是否越过 1.5G 参考线**、是否要调 jobs | 52.7s/1038MB；83.4s/1638MB | ✅ **PASS（数字可复现）** | 我自测：**check 46.0s / 1039MB**；**test --no-run（含链接）75.3s / 1471MB**；另一次（项目 target 目录内）链接峰值 **1763MB**。→ §依赖与资源判定 |
| **I1** | `cargo test --workspace` → 295/295 | ✅ | ✅ **PASS** | 实测 **295 passed / 0 failed / 34 ignored** |
| **I2** | `--include-ignored` → 329/329（真实库真跑） | ✅ | ✅ **PASS** | 实测 **329 passed / 0 failed**；真实库层 **11/11**、SCIM 层 **7/7** |
| **I3** | clippy 0 告警 / fmt 通过 | ✅ | ✅ **PASS** | `cargo clippy --workspace --all-targets -- -D warnings` → exit 0；`cargo fmt --all --check` → exit 0 |
| **I4** | 4 个 xtask 门禁全过 | ✅ | ✅ **PASS** | `capabilities --check`（13 文件一致）、`check-deps`、`check-modules`、`tenant-policies --check` 全 exit 0 |
| **I5** | `scripts/test-integration.sh` exit 0（容器 2→2） | ✅ | ✅ **PASS** | exit 0；**容器数量比对通过：2 → 2**；该脚本用 `--tests` 口径 = **324 passed / 0 failed**（比 329 少 5 个 doctest，属口径差异） |
| **I6** | 前端 23/23；E2E 1/1 | ✅ | ✅ **PASS** | `pnpm -r test` → **23 passed**（i18n3/auth3/schema3/hooks3/sdk2/theme2/ui3/shell4）；`bun run test:e2e` → **1 passed**（chromium） |
| **I7** | ★ **破坏性**：`platform-org → modules/sandbox` → `check-deps` 必须失败 → 还原 | 未做 | ✅ **PASS** | 注入反向依赖 → check **exit 1**：`平台 crate \`platform-org\` 反向依赖了业务模块 \`sandbox\`（依赖链：platform-org → sandbox）`，并连带报出 `platform-server → platform-org → sandbox`；还原后 **md5 逐字节一致**、check exit 0 |
| **I8** | `platform-core/` / `platform-contracts/`（CODEOWNERS 保护目录）改动最小 | ✅ | ✅ **PASS** | `git diff develop...HEAD -- crates/platform-core/ crates/platform-contracts/` → **空**（一行未动） |
| **J** | ★ **变异验证**（Dev 自陈未做） | ❌ 未做 | ✅ **PASS（我补做）** | §变异验证 |

---

## 依赖与资源判定

### 1. 依赖树（我自己数了三遍，口径全部写明）

| 口径 | 基线 `develop@a0a8d7a` | 本轮 `feature/REQ-008@19cd1a7` | 增量 | Dev 声称 | 判定 |
|------|------|------|------|------|------|
| `cargo metadata` 第三方 crate **去重名** | **263** | **273** | **+10** | 263→273 (+10) | ✅ **一致** |
| `cargo metadata` 第三方 crate **包条目**（含多版本） | 272 | 284 | +12 | 未登记 | ℹ️ 差值因 `thiserror` v1/v2 并存 |
| `cargo tree --workspace --edges normal,build` 去重 `name vX` | **239** | **253** | **+14** | 240→254 (+14) | ⚠️ **增量一致，绝对值各差 1**（口径微差，不影响结论） |

**+10 的真实归属（我逐包算了依赖闭包）**：

| 来源 | crate | 说明 |
|------|-------|------|
| `ldap3` 传递依赖（**8**） | `ldap3` / `lber` / `nom` / `minimal-lexical` / `futures` / `futures-macro` / `async-trait` / `tokio-util` | 与 Dev 一致 |
| **SCIM 出站（2）** | `want` / `try-lock` | **由 `platform-org` 打开 `hyper-util` 的 `client-legacy` → `hyper`(http1 client) 引入**；两者**不在基线 `Cargo.lock`** 里（`git show a0a8d7a:Cargo.lock` 无这两个包），基线 `hyper` 只被当服务端用（不需要 http1 client） |

→ **Dev 报告 §3.1 的「新增的 10 个第三方 crate 全部是 ldap3 的传递依赖」与「SCIM 出站的依赖增量 = 0」两句都不成立**（总数 +10 正确）。见 §问题 #3。

### 2. `ring` / `aws-lc-rs` / `aws-lc-sys` / OpenSSL —— 全部 0 命中

```
$ cargo tree --workspace --all-features | grep -oE "\b(ring|aws-lc-rs|aws-lc-sys) v" | wc -l
0
$ cargo tree --workspace --edges normal,build | grep -cE '\b(native-tls|openssl|openssl-sys)( v|$)'
0
$ grep -n ldap3 Cargo.toml
93:ldap3 = { version = "0.11", default-features = false }
```
`ldap3` 的 `default` 特性是 `["sync","tls"]`，`tls` → `tls-native` → `native-tls` → `openssl-sys`。**`default-features = false` 是必需的**，且已正确设置 → ✅。

### 3. TLS 方案 (a)：`https://` 与 `ldaps://` 明确报错 —— **PASS**

- SCIM：`ScimProviderConfig::validate()`（`scim.rs:196-211`）在**连接前**拒绝 `https://` → `ScimError::Config`；用例 `https_is_rejected_instead_of_silently_downgraded` 通过。
- LDAP：`ldap3` 0.11.5 的 `conn.rs` 里 `ldaps` 分支被 `#[cfg(any(feature="tls-native", feature="tls-rustls"))]` **编译期剔除**（我们没开这两个 feature）→ 落到 `s => return Err(LdapError::UnknownScheme(...))`。我用**独立一次性工程**实测：
  ```
  ldaps://127.0.0.1:636 => ERROR: unknown LDAP URL scheme: ldaps
  ldap://127.0.0.1:3899 => ERROR: I/O error: Connection refused (os error 111)   # 走明文 TCP
  https://x.example     => ERROR: unknown LDAP URL scheme: https
  ```
  → **明确报错，没有静默降级到明文**。✅

### 4. 冷编译实测（自己跑、自己计时；Dev 的数字可复现）

口径与 Dev 完全一致：`scripts/measure-cold-build.sh`（采样 `cargo` + 全部 `rustc` 后代进程的 `VmHWM` 求和，偏保守的上界）。

| 操作 | Dev 声称 | **我实测** | 备注 |
|------|---------|-----------|------|
| 冷 `cargo check --workspace --all-targets` | 52.7 s / **1038 MB** | **46.0 s / 1039 MB** | 峰值**几乎逐 MB 吻合** |
| 冷 `cargo test --workspace --no-run`（含链接） | 83.4 s / **1638 MB** | **75.3 s / 1471 MB**（隔离 target 目录）<br>**69.5 s / 1763 MB**（项目 target 目录） | **同一口径下 3 次测量落在 1471–1763 MB** |
| 构建期间系统内存 | 未登记 | 总 7840 MB；**峰值已用 4799 MB；最低仍可用 3041 MB** | **无 OOM、无 swap 风暴** |

> 我第一次冷编译在项目 `target/` 内跑（链接峰值 **1763 MB**），第二次改用**独立 target 目录**（`CARGO_TARGET_DIR=/tmp/...`，其 `cargo clean` 只清理 /tmp，**未触碰项目 target/**）得 **1471 MB**。两者差异说明**这个采样口径的 run-to-run 方差约 ±300 MB**，Dev 的 1638 MB 落在区间内，不是异常值。

### 5. ★ **1638 MB 是否越过 1.5G 参考线？要不要调 `jobs`？—— QA 明确结论**

**结论：`jobs = 3` 保持不变，不需要下调。** 依据五条：

1. **那条 1.5G 参考线是给 `cargo check` 阶段的，不是链接阶段的闸。**
   `docs/dev-environment.md:205`（REQ-006 原始结论）：「峰值内存 925–940 MB **< 1.5 GB** → `jobs` 保持 3」—— 上下文是**冷 `cargo check`**。
   链接阶段项目自己写的预算是 **`docs/dev-environment.md:25`：`| cargo build 峰值 | 1.5-2.5G | **链接期是最高点** |`**。
   实测链接峰值 **1.47–1.76 GB**，**落在项目自己声明的 1.5–2.5G 带子内**（贴近下沿）。
   → Dev 把 check 阶段的 1.5G 套到链接阶段上判定「越线」，**属于口径误用**；按项目自己的口径，**没有越线**。
2. **实测 `cargo check` 峰值 1039 MB，仍 < 1.5 GB** → check 阶段本来就远在预算内。
3. **`jobs` 管的是并发 `rustc` 个数，不是链接器。** 链接期是**单个链接进程**的峰值，`jobs` 从 3 降到 2 对它几乎无影响；真正被 `jobs` 影响的是 check 阶段，而 check 阶段有 500 MB 余量。
4. **系统侧没有压力**：构建期间**最低仍有 3041 MB 可用**（总 7840 MB），峰值已用 4799 MB。`docs/dev-environment.md:32` 列的危险组合是「桌面 2.8G + Storybook 1.5G + Playwright 0.5G + `cargo build` 2.5G ≈ 7.3G」—— 我们**不在**该组合里（未开 Storybook / Playwright 与编译并行）。
5. **没有更省的可选项**：若为压内存去动 `profile.dev`（`debug`/`split-debuginfo`），代价是调试体验与增量编译时间，收益（几百 MB）在当前余量下不值得。

**给 BA 的一句话**：链接峰值确实比 REQ-007 高了一截，但**这是量级上的正常增长，不是资源事故**；建议把 `docs/dev-environment.md` 的预算表**按 check / build 两段分列**（现在 `cargo build` 那行写的是 1.5–2.5G，但 §2.3 的实测表里没有链接期实测值），避免下一轮再被当成「越线」。

---

## 两项 BA 偏离的核对

| # | BA 裁定 | 期望 | 代码实现 | 结论 |
|---|--------|------|---------|------|
| **1** | **按 D3**：`org_node` **无 `manager_id`**，汇报关系挂 **`user.manager_id`** | `org_node` 表无该列；`user` 表有 `manager_id` | `migrations/20261118000700_org_schema.up.sql:18-33`：`org_node` 列为 `tenant_id/id/parent_id/node_type/code/name/path_labels` —— **无 `manager_id`**；`migrations/20261118000600_iam_user_org_attrs.up.sql:17`：`ALTER TABLE platform_iam_user ADD COLUMN manager_id TEXT`。**运行期 `information_schema` 复核：`org_node` 恰好 7 列、`manager_id` 0 命中；`platform_iam_user.manager_id` 存在** | ✅ **完全一致** |
| **2** | 接受第 6 张表 `sync_source_state`（§9 同步状态可见性） | 表存在 + RLS + **有真实承载逻辑** | `migrations/…org_schema.up.sql:118-125` 建表；`manifest.json` 的 `tenantScoped` 列出它；`store.rs:855-890` 有 `upsert_sync_state_in` / `get_sync_state`；`service.rs:807-808` **读它算增量同步点**；`service.rs:844-855` **写它**；`http.rs:955-969` **暴露 HTTP 端点** | ✅ **表不是空壳 —— 真有读写链路**（但见 §问题 #1：**错误计数那一半是死的**） |

**补充**：`crates/platform-org/manifest.json` 的 `tenantScoped.tables` 列全了 6 张表，`platform-db/src/scopes.rs::collect_scopes` 已扩展到扫 `crates/` 与 `modules/` 两处，`Dockerfile` 也把平台 crate 的 manifest 拷进镜像 —— **部署期 `base tenant-policies` 不会漏掉组织表**（这正是「漏一张表 = 该表完全无隔离」的致命点，已堵住）。

---

## 变异验证（★ 本轮 Dev 未做，我补做）

**对象**：`crates/platform-org/tests/org_real_db.rs` 的**防假绿护栏** `missing_database_url_test_does_not_report_success`
（子进程去掉 `DATABASE_URL_TEST` 后必须**非零退出**且输出含 `0 passed`）。

**变异手法**：把真实库用例改成「**缺环境变量就早退**」——`required_test_database_url()` 改为返回 `Option<String>`、缺变量返回 `None`；`setup()` 改为返回 `Option<Fixture>`；**10 个真实库用例的调用点全部改为 `let Some(fixture) = setup().await else { return; };`**（即静默跳过、报「通过」）。

**变异前（原始代码）**：
```
test missing_database_url_test_does_not_report_success ... ok
test result: ok. 1 passed; 0 failed; ...        # 护栏绿
```

**变异后（护栏应变红）**：
```
thread 'missing_database_url_test_does_not_report_success' panicked at
  crates/platform-org/tests/org_real_db.rs:1062:5:
缺 DATABASE_URL_TEST 时子进程必须失败（它却成功退出了）：

running 10 tests
test conflicts_are_recorded_per_field_and_applied_after_adjudication ... ok
test deleting_a_node_with_members_requires_handing_them_first ... ok
... (10 个真实库用例全部报 ok)
test result: ok. 10 passed; 0 failed; 0 ignored; ...   # ← 假绿现场

test result: FAILED. 0 passed; 1 failed; ...
EXIT CODE = 101
```
→ **护栏确实抓住了「缺环境变量就早退」的假绿回归**，与 Dev 自陈的历史事故（“安静地返回 None → 测试框架报 ok”）完全对应。

**还原**：`cp` 原始字节回位 → `md5sum` **逐字节一致**（`7ab3e60d8dba1ce87a54dbebe91f0a8c`），`git status` 空，护栏恢复 **GREEN**（exit 0）。

**结论**：护栏**不是摆设**，本轮唯一的「Dev 未做」项由 QA 补齐并通过。

---

## 回归

- **Rust（非 ignored）**：`cargo test --workspace -- --test-threads=2` → **295 passed / 0 failed / 34 ignored** ✅
- **Rust（含 ignored，真实库真跑）**：`cargo test --workspace -- --include-ignored -- --test-threads=2` → **329 passed / 0 failed** ✅（真实库层 **11/11**、SCIM 层 **7/7**）
- **`scripts/test-integration.sh`** → **exit 0**，**容器数量比对通过：2 → 2** ✅（该脚本口径 `--tests` = 324 passed / 0 failed）
- **前端**：`pnpm -r test` → **23 passed / 23** ✅（i18n 3、auth 3、schema 3、hooks 3、sdk 2、theme 2、ui 3、shell 4）
- **E2E**：`bun run test:e2e` → **1 passed / 1** ✅（chromium，「底座壳渲染并可过滤菜单」）
- **Lint / 格式**：`cargo clippy --workspace --all-targets -- -D warnings` → **0 告警** ✅；`cargo fmt --all --check` → ✅
- **门禁**：`capabilities --check`（13 文件一致）✅ · `check-deps` ✅ · `check-modules` ✅ · `tenant-policies --check` ✅
- **依赖方向（破坏性）**：注入 `platform-org → modules/sandbox` → `check-deps` **exit 1** → 还原后 exit 0 ✅

---

## 问题（按严重度）

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **中** | **`sync_source_state` 的「同步错误计数」是死代码 —— §9 交付项只实现了一半。** LDAP 检索失败时 `sync_ldap` 在 `service.rs:815-817` 用 `?` **提前返回**，**绕过**了 `service.rs:844-855` 的 `upsert_sync_state`；而 `upsert_sync_state_in`（`store.rs:855-872`）是 `error_count`/`last_error` 的**唯一**写入点，唯一调用方传的是**字面量** `error_count: 0, last_error: None`。→ 运维侧**永远看不到同步失败**，§9「同步状态的可见性（上次同步时间、**同步错误计数**）」的错误计数半边不存在。 | `crates/platform-org/src/service.rs:812-817`（`?` 早退）、`service.rs:844-855`（唯一写入点，写死 0/None）、`store.rs:855-872`（唯一 writer）；**真库实测**：4 行 `sync_source_state` → `count(last_error)=0`、`max(error_count)=0` | 在 `sync_ldap` 的错误路径上也做一次 upsert（`error_count+1`、`last_error=Some(reason)`），并补一条「源不可达 → 错误计数递增」的失败注入用例 |
| **2** | **中** | **`user_id` 侧完全没有外键 → 「外键都带 tenant_id」只对节点侧成立，悬空/跨租户 user_id 可写入。** `pg_constraint` 里组织表只有 3 条 FK（`org_node_parent_fk` / `user_org_node_fk` / `user_position_node_fk`，**都带 tenant_id** ✓），但 `user_org.user_id` / `user_position.user_id` / `external_identity.user_id` **一条 FK 都没有**；且 `service.rs::add_member`（`service.rs:531-548`）**只校验 org 节点存在，不校验用户存在**。→ Dev 报告 §4「外键都带 tenant_id → **结构上**杜绝跨租户挂父节点与**悬空成员关系**」表述过强。 | `pg_constraint` 查询（仅 3 条 FK）；**我实测插入成功**：`user_org(user_id='ghost-user-does-not-exist')` → 写入成功（count=1）；`user_org(user_id='some-other-tenant-user')` → 也写入成功。`service.rs:531-548` 只查 `find_node_in` | 给 3 张表的 `user_id` 补复合外键 `(tenant_id, user_id) → platform_iam_user(tenant_id, id)`（或退一步：在 `add_member` 里校验用户存在并返回 404）。**影响评级**：不是租户隔离绕过 —— 行级 RLS 仍然成立，跨租户 user_id 在 REQ-009 join 时因 RLS 不可见而不会泄露，但会留下脏数据/悬空行 |
| **3** | **低** | **依赖归因不准：Dev 的「+10 全部是 ldap3 的传递依赖」与「SCIM 出站的依赖增量 = 0」两处都不成立。** `want` / `try-lock` **不在基线 `Cargo.lock`** 里，是 `platform-org` 打开 `hyper-util` 的 `client-legacy` → `hyper`(http1 client) 引入的，**属于 SCIM 出站的增量**。→ 依赖预算按「SCIM 零增量」记账会少记 2 个 crate。 | `git show a0a8d7a:Cargo.lock` 中**无** `want`/`try-lock`；依赖闭包分析：`ldap3` 可达 8 个，`want`+`try-lock` 归属 hyper 栈；`cargo tree -p platform-org` 可见 `hyper → http1 → want → try-lock` | 修正 `feature-meta/verification-report.md` §3.1 与 §0 表：ldap3 = 8，SCIM 出站 = 2；**预算总数 +10 不变** |
| **4** | **低** | **构建图口径绝对值对不上**：Dev 240 → 254，实测（去重 `name vX`）239 → 253。**增量 +14 一致**，绝对值各差 1 → 计数口径（是否含工作区成员 / 根节点）微差，不影响结论。 | `cargo tree --workspace --edges normal,build` 去重计数：基线 239 / 本轮 253 | 在报告里写死口径命令，让下一轮可逐字复现 |
| **5** | **低** | **LDAP 侧没有像 SCIM 那样的**前置** URL 校验。** SCIM 有 `ScimProviderConfig::validate()` 在连接前拒 `https://`；LDAP 侧 `Ldap3Source` 无 `validate()`，`ldaps://` 的拒绝发生在**连接期**且被包装成 `LdapError::Connection` → `OrgError::Internal`（不是配置类错误）。行为满足「不静默降级」，但**一致性弱于 SCIM**、错误码语义偏粗。 | `crates/platform-org/src/ldap.rs:407-456`（`Ldap3Source` 无 validate）；`ldap.rs:815-817`（错误映射）；对比 `scim.rs:196-211` | 给 `LdapSourceConfig` 加一条与 SCIM 对称的 `validate()`：非 `ldap://` 开头直接返回配置类错误（`ldap://` 之外一律拒，含 `ldaps://`/`starttls`） |
| **6** | **信息** | **全量 include-ignored 回归必须用**管理员**连接串，不能用 `base_app`。** 任务书示例给的 `DATABASE_URL_TEST=postgres://base_app:base@…` 会让**预存的** `platform-db/tests/quota_real_db.rs:126` 失败（`permission denied for schema public`）——该文件**不在 REQ-008 改动范围**，属**预存在问题**。REQ-008 自身用例在 `setup()` 内部通过 `app_role_url()`（`org_real_db.rs:104-109`）切到 `base_app`，并调用 `require_isolation()`（`:234`）自检「角色不绕过 RLS」。 | `crates/platform-db/tests/quota_real_db.rs:126`（`platform-iam`/`platform-db` 测试文件均未在 REQ-008 改动清单内）；`org_real_db.rs:104-109, 234` | 在 `scripts/test-integration.sh` 顶部注释里写明「`DATABASE_URL_TEST` 必须是管理员串；应用角色由各用例自己切换」，避免下一个人踩同一个坑 |
| **7** | **信息** | **Dev 收窄的预存 IAM 断言是必要的，不是掩盖 —— 我验证过了。** 我把 `crates/platform-iam/tests/iam_real_db.rs` 的收窄**还原成 develop 的原断言**后，在含 REQ-008 会话行的库上该用例**真的失败**：`每条会话记录都必须带 tenant_id：tenant-a`（原断言要求 `tenant_id` 以 `iam-test-` 开头，而 REQ-008 的会话租户是 `tenant-a`）。真库直查 `sessions_with_empty_tenant = 0` —— REQ-008 写入的会话**都带 tenant_id**，收窄没有掩盖任何缺陷。 | 我还原后跑该用例 → FAILED（exit 101）；`select count(*) from platform_iam_session where tenant_id is null or tenant_id=''` → **0** | 无需处理，登记为「已核实」 |
| **8** | **信息** | **预存在问题（不计入 Dev 账）**：`platform-db/tests/quota_real_db.rs` 需要管理员 DDL 权限，与 REQ-008 无关。 | 见 #6 | 登记到项目文档的「跑测试要管理员串」说明 |

---

## 未覆盖 / 无法验证

| 项 | 状态 | 判定与理由 |
|---|------|-----------|
| **LDAP 真实服务端到端**（`Ldap3Source::connect()` / `search()` 的真实连接与检索） | **未验证** —— 本机无 LDAP/AD 服务器可连 | **不阻塞合并。** 理由：① 需求 AC-S-016 验收的是**位运算判定**（7 个取值已逐个实测为真），AC-S-014 验收的是**字段级冲突**（已真实库跑通）；② 未覆盖的只是**传输层**（连接/检索），失败模式是「连不上就明确报错」而**不是**静默产生错误数据（`ldaps://` 已实测明确报错，无静默降级）；③ `SearchEntry → LdapEntry` 转换、增量过滤器拼装、十六进制解析、时间戳格式化均有单测且全绿。**建议**：REQ-013 或后续需求补一条真 LDAP 容器的 e2e（可复用 `base_test` 的容器模式，成本低）。 |
| `/api/org/*` 真机 HTTP 冒烟 | **未验证** | 路由、RFC 9457 错误体、英文 ASCII `detail`、422 错误码映射均有单测；但鉴权提取器依赖 IAM 的 access token，未起 `base serve` 打真机 HTTP。**不阻塞**（与 REQ-007 的同款未验项同级）。 |
| SCIM 出站**后台 worker 常驻循环** | **未验证** | 本期提供 `enqueue` / `drain_once`，常驻循环由部署侧调度。异步语义（入队不阻塞）已实测。 |
| 审计落库 sink / 通知通道 | **未验证**（生产用 `NoopAuditRecorder` / `LoggingNotifier`） | 事件**声明与产生**已实测（9 条 `AuditEventSpec` + 收件人/载荷断言）；落库与真实投递属 REQ-014 / REQ-015。 |
| 非 `base_test` 库上的行为 | **未验证（按规程不测）** | 测试库白名单闸门只允许 `base_test`；`base_dev` 从未被触碰。 |
| 链接峰值精确值 | **部分验证** | 实测 1471–1763 MB（同口径 3 次，方差 ±300 MB）。结论（jobs 不调）不依赖精确值。 |

---

## 结论

**PASS-WITH-RISK**

- 需求 AC-S-011 ~ AC-S-019 **全部 PASS**，每条都有实现 + 测试 + 我自己的独立复现证据。
- **无 HIGH / 阻塞级缺陷**；10 项核心检查项**逐条复现为真**。
- 两项 BA 偏离**实现与裁定完全一致**。
- ★ Dev 未做的**变异验证**（护栏）我补做并**通过**；另补做两项**破坏性验证**（删 RLS policy、造反向依赖）**均按预期变红并逐字节还原**。
- **遗留 2 项中等问题**（§问题 #1 同步错误计数是死代码 / #2 `user_id` 侧无外键）**建议在 REQ-009 开工前修掉**——因为 REQ-009 要消费 `user_org` 与 `external_identity`，这两张表的引用完整性会直接决定它能否安全地做 join 判定。两项都不影响本次合并的**功能正确性**，但 #1 直接削弱了 §9 承诺的运维可见性。
- **依赖树数字 +10 正确、归属需更正**（ldap3 = 8，SCIM 出站 = 2）。
- **冷编译：`jobs = 3` 保持不变**，链接峰值落在项目自己声明的 `cargo build` 1.5–2.5G 预算带内；Dev 引用的「1.5G 参考线」是 check 阶段口径。建议 BA 把 `docs/dev-environment.md` 的预算表按 check / build 分列，并补上链接期实测值。

---

### 附：本次验证的完整自测清单（命令级）

```bash
# 依赖 / 门禁
cargo xtask capabilities --check && cargo xtask check-deps && cargo xtask check-modules
DATABASE_URL=postgres://base_app:base@127.0.0.1:5432/base_test cargo xtask tenant-policies --check
cargo tree --workspace --all-features | grep -oE "\b(ring|aws-lc-rs|aws-lc-sys) v" | wc -l   # 0
cargo tree --workspace --edges normal,build | grep -cE '\b(native-tls|openssl|openssl-sys)( v|$)'  # 0

# 测试（注意：DATABASE_URL_TEST 必须是管理员串，见 §问题 #6）
cargo test --workspace -- --test-threads=2                          # 295/295
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
  cargo test --workspace -- --include-ignored --test-threads=2     # 329/329
bash scripts/test-integration.sh                                    # exit 0, 容器 2 → 2
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check

# 冷编译（隔离 target 目录，不触碰项目 target/）
CARGO_TARGET_DIR=/tmp/qa008-cold-target bash scripts/measure-cold-build.sh
#   cargo check --workspace --all-targets   46.0s / 1039 MB
#   cargo test --workspace --no-run         75.3s / 1471 MB

# 前端
pnpm -r test        # 23/23
bun run test:e2e    # 1/1
```

### 附：验证者留痕

- **工作树**：验证结束时 `git status --short` **为空**（除本报告文件）；`git diff` 为空；两处**故意破坏**（`platform-org/Cargo.toml`、IAM 测试断言）与两处**故意变异**（`org_real_db.rs`）均已**逐字节还原**（md5 核对一致）。
- **`base_test`**：我造的全部临时数据（`qa-t-a` / `qa-t-b` / `qa-tree` 三租户节点与成员）**已删除**（`org_node`/`user_org`/`user_position` 中 `tenant_id LIKE 'qa-%'` 计数均为 **0**）；我为验证临时 `GRANT` 给 `base_app` 的表权限**已 REVOKE 还原**（`information_schema.role_table_grants` 中 `base_app` 的直接授权数 = **0**，与初始一致）；`org_node` 的 RLS policy **已按原定义逐字节重建**，`tenant-policies --check` 复跑 **exit 0**；6 张表 `relrowsecurity=true` 且各 1 条 policy。
- **容器**：`base-postgres` / `base-redis` 均 healthy，**数量 2 → 2 未变**。
- **未触碰**：`feature-meta/` 下的 `status.md`（BA 的）、`verification-report.md`、`traceability.md`；`crates/platform-core/`、`crates/platform-contracts/`（CODEOWNERS 保护目录）。
- **本文件**：`feature-meta/qa-report.md` —— 按本仓库惯例（BA 每轮归档于 `5f426ee` 等提交）覆盖了上一轮 **REQ-007** 的 QA 报告；REQ-007 原报告在 git 历史中完好。
