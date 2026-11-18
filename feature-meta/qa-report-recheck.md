# 修复轮复核报告 —— REQ-009（轻量）

- **需求**: REQ-009「RBAC 权限与数据范围」
- **被验提交**: `cd65640`（修复轮）· 上一轮 HEAD `05bc6bd`
- **首轮报告**: [`qa-report.md`](./qa-report.md)（结论 PASS-WITH-RISK，HIGH 0 / MEDIUM 2 / LOW 2 / INFO 3）
- **验证者**: QA Agent（**独立复跑；Dev 自报数字一律不采信**）
- **日期**: 2026-11-18
- **结论**: **PASS** —— 首轮 2 项 MEDIUM 均已消解，2 项 LOW 均已修好，无回归。
  字面要求上只剩 1 条「异常路径用 RAII 装回」未按字面实现（风险已被 schema 隔离消除，见 LOW-1）。

> **本轮只做修复轮复核**，不是全量重验：16 条 AC 的逐条证据沿用首轮；本轮只查首轮 4 个问题 + 回归。

---

## 1. 逐条结论

| # | 首轮问题 | 本轮结论 | 一句话证据 |
|---|---|---|---|
| **M1** | trait 文档与 D40 相反（非法条件返回 `None`） | **PASS** | 文档改成两值语义 + 明写「非法不得返回 `None`」；实现**一字未改**（仍是 `Some(1 = 0)`）；真库用例断言 `is_some()` 且端到端 0 行 |
| **M2** | 全局摘唯一索引 + 跨租通配 DELETE | **PASS**（1 条字面要求由隔离替代，见 LOW-1） | 测试侧已无全局 DDL（唯一 DROP 带 schema 限定）；DELETE 带 `tenant_id`；用例改在探针 schema 跑；收尾断言**探针 + public** 两个索引都在 |
| **L3** | 违例位置是字符偏移，文档写「文件:行」 | **PASS** | 自造夹具实测报 `lib.rs:22:4` / `:23:27`，与真实行列一致（首轮报的是 `:96`） |
| **L4** | 同角色多 grant 的胜者靠排序副产物 | **PASS** | `contributions_for` 显式按具体度选；独立探针 9 例全过（含原随机的继承方向）；变异回旧实现→测试变红 |
| **回归** | —— | **PASS** | 完整集成套件 **3 轮 × 461/461**（EXIT=0） |

---

## 2. M1 —— trait 文档 vs D40

**修好了，且只动文档。**

1. **两值语义写清了**（`crates/platform-core/src/extension/permission.rs:180-193`）：
   一张表分别写 `None` = 「根本没有条件」→ **直接放行**；`Some(永不匹配)` = 「条件非法」→ **判不通过**。
2. **明确警示**（`:189`）：「⚠️ **非法条件不得返回 `None`**」—— 并把「误拒全部」与「静默放行」两种后果都写了。
   `crates/platform-rbac/src/checker.rs:361-369` 同步补了同义文档（指向调用方 `if let Some(c) = ...` 的陷阱）。
3. **实现未改**（这是关键，改文档不能顺手改实现）：
   - `checker.rs:42-46` `never_match()` 仍返回 `sql = "1 = 0"`、`values` 为空；
   - `checker.rs:53-55` `compile_condition` 仍是 `compile(...).unwrap_or_else(|_| never_match())`；
   - `git diff 05bc6bd..cd65640 -- crates/platform-rbac/src/checker.rs` **只有 +5 行文档**，无代码行变化。
4. **「引用白名单外属性 → `Some` 且永不匹配」有用例锁住**：
   `crates/platform-rbac/tests/rbac_real_db.rs:1902` `a_condition_outside_the_whitelist_never_grants_access`
   —— 用**去掉 `record.amount` 的白名单**重建判定器，让注册表里的条件变成非法，然后断言
   `condition(...)` **`is_some()`** → `sql == "1 = 0"` → `values.is_empty()` → 端到端可见行 **0**。
   两轮日志均 `... ok`。

**残留**：首轮那句错误措辞仍出现在 `feature-meta/verification-report.md` 与首轮 `qa-report.md` 里 —— 那是**历史报告**，不是代码契约，不影响判定。

---

## 3. M2 —— 测试夹具污染

**修好了，隔离做法比要求的更彻底。**

### 3.1 静态核对

| 判据 | 实测 |
|---|---|
| **① 全局索引还有没有被 DROP/CREATE** | **没有**。全仓 `DROP INDEX` 只有两处：`migrations/...down.sql:8`（迁移回滚，正常）与 `crates/platform-rbac/tests/rbac_real_db.rs:827` 的 **`DROP INDEX IF EXISTS {schema}.user_org_one_primary_idx`**（schema 限定，不是全局）。测试侧的 `CREATE UNIQUE INDEX`（`:845`）是 `ON {schema}.user_org` —— PostgreSQL 把索引建在**表所在 schema**。`public` 的索引不再被测试碰。 |
| **② DELETE 是否带租户条件** | **带了**。`:839` 与 `:907` 都是 `DELETE FROM user_org WHERE tenant_id = $1 AND user_id LIKE 'u-dual-primary-%'` + `.bind(tenant)`。全文件只有这两条 `DELETE FROM user_org`。 |
| **③ 有没有 RAII/guard** | **没有 guard**（见 LOW-1）；但**不再需要**：被摘的索引在**一次性 schema** 里。 |
| **④ 收尾有没有断言索引存在** | **有，且断言两条**：`:996`「用例结束时探针的部分唯一索引必须在位」+ `:1000`「**全局**的部分唯一索引必须仍在（这条用例**不许**动它）」。之后 `:1004` 才 drop 探针 schema。 |

### 3.2 独立 schema 的实现

`crates/platform-rbac/tests/common/mod.rs:359 create_probe_fixture`：
建 `rbac_probe_<8hex>`（`run_id()` 每进程唯一）→ `CREATE TABLE ... (LIKE public.x INCLUDING ALL)` + 拷数据
→ 把复制来的部分唯一索引改名为规范名 → 授权给 `base_platform_app` → 用 `DbOptions.schema` + `search_path` 建池。
删除侧 `drop_probe_schema:495` + 过期清理 `drop_stale_probe_schemas:507`（TTL 1 小时，只清别人的）。

### 3.3 实测（不是读代码）

| 检查 | 结果 |
|---|---|
| 完整套件 × 3 轮后 `pg_indexes` 查 `user_org_one_primary_idx` | **`public.user_org_one_primary_idx` 三轮都在** |
| 探针 schema 残留 | `SELECT count(*) FROM pg_namespace WHERE nspname LIKE 'rbac_probe%'` → **0** |
| 用例确实跑到了 | 每轮日志都有 `test a_missing_or_ambiguous_primary_department_falls_back_to_self ... ok` |

**一个反证**：如果这条用例其实还在 `public` 上跑，那么 `DROP {probe}.user_org_one_primary_idx` 根本解不开 `public` 的唯一约束，
第 3 步「造两个主部门」会直接被 DB 拒绝 → 用例必红。它连续 3 轮绿 ⇒ 确实走的是探针 schema。

---

## 4. L3 —— C-5 违例位置

**修好了：现在是真正的 `文件:行:列`。**

- `crates/platform-contracts/src/authz_scan.rs:145 line_and_column` / `:160 position`；三处报错点（`:116`、`:132`、以及 non-literal 分支）统一走 `position()`。
- **自造夹具**（`/tmp/qa-req009/fakerepo/modules/v7_line_probe`，与首轮同一套 `check_c5` 探针）：第 22 行 `fn check_perm(...)`、第 23 行 `pub fn other() -> usize { require_permission(PERM) }`。实测输出：
  ```
  VIOLATION[C-5] ... lib.rs:23:27 does not use a string literal ...
  VIOLATION[C-5] ... lib.rs:22:4 bypasses the platform entry point `require_permission`
  ```
  22 行 / 23 行**完全对得上**，列号 4 / 27 也正是标识符在行内的起始列。首轮同一条夹具报的是 `lib.rs:96`（文件只有 2 行）。
- 两个新单测（`a_violation_points_at_the_line_it_is_on`、`a_non_literal_call_points_at_the_line_it_is_on`）在套件里 **ok**。
- 格式变化的兼容性：唯二消费方 `checks.rs:530/540` 只是把字符串插进错误消息，**没有解析**，不会被打断。

**已知小瑕疵（已在代码注释里声明，非缺陷）**：同一行存在**块注释**时列号会偏小（`strip_comments` 会抹掉块注释内容但保留换行）；**行号始终精确**。

---

## 5. L4 —— 同角色多 grant 的胜者（D43）

**修好了：胜负由具体度显式决定，不再是遍历顺序的副产物。**

- `crates/platform-rbac/src/service.rs:884 contributions_for`：每条 grant 变成 `MatchedGrant { specificity, override_scope }`；
  `:964 pattern_specificity` 给 **精确=3 / `module:resource:*`=2 / `module:*`=1**；
  `:991 stricter` 在同具体度时按 **`ALL` < `OWN_ORG_AND_BELOW` < `OWN_ORG` < `SELF`** 取更严；`:975 containment_strictness` 对 `CUSTOM` 返回 `None`（**不参与**比较）。
- 角色级 scope 先算好（`role_scope`），「grant 无覆盖」时按角色级参与比较，最终仍 `override > 角色级`。

### 5.1 独立探针（自造夹具，直接调 `contributions_for`，不经 Dev 的测试）

`/tmp/qa-req009/l4probe`（独立 crate，path 依赖真实 `platform-rbac`）。9 例**全 PASS**：

| 场景 | 期望 | 实测 |
|---|---|---|
| 同角色 `sandbox:*`(ALL) + 精确 `sandbox:scope-item:read`(SELF) | SELF | **SELF** |
| 父角色上同时有这两条，子角色继承 | SELF | **SELF** |
| 父=精确 / 子=通配，**角色 ID 字典序两个方向**都跑 | SELF | **两个方向都 SELF** ← 首轮「等同随机」的场景 |
| **具体度 vs 严格度**：通配(SELF) + 精确(ALL) | **ALL**（精确赢，不是「总取更严」） | **ALL** ✓ 证明不是靠「取最严」蒙对 |
| `sandbox:scope-item:*`(ALL) vs `sandbox:*`(SELF) | 资源通配赢 → ALL | **ALL** |
| 同具体度重复 grant：`sandbox:*`(ALL) + `sandbox:*`(SELF) | 更严 → SELF | **SELF** |
| 同具体度含 `CUSTOM` + `OWN_ORG_AND_BELOW` | CUSTOM 不参与 → OWN_ORG_AND_BELOW | **OWN_ORG_AND_BELOW** |
| **跨角色仍并集**（D37 回归）：A=精确 SELF、B=通配 ALL | 两条贡献都在 | **{SELF, ALL}** |

### 5.2 变异验证（证明测试不是摆设）

把 `contributions_for` 的选取临时改回旧语义「**取最后一条匹配的**」（`matched = Some(candidate)`），
`crates/platform-rbac/src/service.rs` 净变 3 增 14 删：
- 单测 `an_exact_grant_on_a_parent_role_wins_over_a_wildcard_grant_on_the_child_role` **FAILED**（`left: All / right: OnlySelf`）
- 真库 `an_exact_grant_on_the_parent_role_wins_over_a_wildcard_grant_on_the_child_role` **FAILED**（看到 11 条，期望 1 条）
- 还原：`git checkout --` + **sha256 与变异前逐字节一致**（`1d330fce…3ec3fa`）；复跑 D43 子集 **4+3 全绿**。

---

## 6. 回归

| 跑法 | 结果 |
|---|---|
| `bash scripts/test-integration.sh` 第 1 轮（未变异树） | **461 passed / 0 failed / 0 ignored**，23 个二进制，EXIT=0 |
| 同上 第 2 轮（未变异树） | **461 passed / 0 failed / 0 ignored**，EXIT=0 |
| 同上 第 3 轮（**变异还原之后**） | **461 passed / 0 failed / 0 ignored**，EXIT=0 |

- 每轮都 `--include-ignored` + **只有一个 `--`**；只碰 `base_test`（脚本自带白名单重置）。
- **数字自洽**：首轮基线 452 + 本修复轮新增 **9** 个测试（`platform-rbac` lib 3 + `rbac_real_db` 4 + `authz_scan` 2）= **461** ⇒ 没有测试被删/被忽略。
- Dev 自报的 461/461 与「提交后 1 轮全绿」：**我只把它当参考**；上述 3 轮是我自己跑的。

---

## 7. 契约与基建（复跑）

| 检查 | 命令 | 结果 |
|---|---|---|
| 依赖预算 | `Cargo.lock` 计数（核 diff，不看 `Cargo.toml`） | **294 包**（第三方 registry **284**）；**修复轮 `Cargo.lock` 零改动** |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | **EXIT=0 / 0 告警**（首次命中了缓存，`touch` 6 个改动文件后**强制重建**，9 个 unit 真正重新检查） |
| 格式 | `cargo fmt --all --check` | EXIT=0 |
| 契约门禁 | `bash scripts/sync-contracts.sh` | **EXIT=0**（两份快照一致） |
| 能力索引 | `cargo xtask capabilities --check` | 通过（13 个文件） |
| 依赖方向 | `cargo xtask check-deps` | 通过 |
| 模块目录 | `cargo xtask check-modules` | 通过（1 个模块 sandbox） |
| 工作树 | `git status --short` | 仅两个 QA 报告：` M feature-meta/qa-report.md`、` M feature-meta/qa-report-recheck.md`（**QA 产物，未提交属正常**）；**无实现文件改动**。⚠️ 说明：`qa-report-recheck.md` 在本分支 HEAD 里**原本是 REQ-008 的归档版**（`qa-report.md` 同理），按本仓「每个需求覆盖同名报告、由 BA 归档」的惯例（见 `1110c00`/`5f426ee`）本次写入 REQ-009 版本；首轮 `qa-report.md` 未被改动。 |

---

## 8. 残留问题

### LOW-1（低）M2 的「异常路径也保证装回」是**用隔离实现**的，不是 RAII guard

- **事实**：`drop_probe_primary_index` 与 `restore_probe_primary_index` 之间**没有 Drop guard**。若断言在此之间 panic，**探针 schema 里的索引**会保持被摘状态，该 schema 残留到 `drop_stale_probe_schemas` 的 1 小时 TTL 才被清。
- **影响**：**不影响任何其它用例** —— 被摘的是 `rbac_probe_*` 里的一次性索引，`public` 的预防层从未被触碰（3 轮实测索引均在位）。首轮 MEDIUM 的实质危害（全局预防层留在关闭状态）已**结构性消除**。
- **为什么仍要记**：交付说明里写的是「异常路径也保证装回」，字面上**没有实现**。若要按字面达标，加一个 `Drop` guard 即可；不加也可接受。

### INFO-1 D43 的回归灵敏度集中在**一条**用例

- 变异回「取最后一条匹配」后：3 条单测里**只有 1 条红**，4 条真库里**只有 1 条红**。
- 原因：同角色内 grant 的**插入/排序顺序**恰好是「通配在前、精确在后」（`'*' < 小写字母`），旧实现**碰巧**也选到精确；只有「父角色精确 + 子角色通配且子角色 ID 排序在后」才暴露。
- **不是缺陷**（至少有一条能红），但要知道护栏只有一道：**别删** `an_exact_grant_on_the_parent_role_wins_over_a_wildcard_grant_on_the_child_role`（单测 + 真库各一条）。

### INFO-2 L3 的列号在「同一行有块注释」时偏小

- `line_and_column` 在**剥注释后**的源码上算列；块注释内容被抹掉但换行保留 ⇒ **行号精确**，列号可能偏小。代码注释（`authz_scan.rs:141-143`）已声明。

### INFO-3 探针 schema 每次复制 `public` 全表

- `CREATE TABLE ... (LIKE ... INCLUDING ALL)` + `INSERT SELECT *` 覆盖 public 所有表。当前测试库很小（该二进制 ~10s 内跑完），未成瓶颈。数据量变大后这里会变慢（不影响正确性）。

---

## 9. 未覆盖（如实列）

| 项 | 状态 | 原因 |
|---|---|---|
| 真实多实例部署的 epoch 收敛 / Redis 形态 / CI 真跑 / 前端 E2E | **未覆盖** | 同首轮，属部署侧或本需求范围外（修复轮也没改前端，`git diff` 无前端文件） |
| **panic 路径**下探针 schema 的残留与 TTL 清理 | **未实测** | 需要故意让用例 panic；只做了代码级核对（`drop_stale_probe_schemas` 的 `created_at` 注释解析分支未用**真正过期**的 schema 跑过） |
| `pnpm -r test` | **未重跑** | 修复轮无前端改动（已核 `git diff --name-only`），首轮 23/23 仍有效 |
| 首轮其余 AC 的端到端复跑 | **未重做** | 本轮定位是轻量复核；完整套件 461 条已覆盖对应断言 |

---

## 10. 结论

- **VERDICT: PASS**
- **问题计数**：HIGH **0** / MEDIUM **0**（首轮 2 项已消解）/ LOW **1**（新增，见 LOW-1，不阻塞）/ INFO **3**
- **实际 passed**：完整集成套件 **461/461 × 3 轮**（含 ignored，23 个二进制，EXIT=0）；`rbac_real_db` 单二进制 22 条；`platform-rbac` lib 42 条
- **建议**：可直接进入合并；LOW-1 与 INFO-1 交给 BA/Dev 决定是否顺手处理。
- **纪律**：本轮**未改实现、未改规格、未 commit**；报告只写了 `feature-meta/qa-report-recheck.md`（该路径在本分支原有 REQ-008 归档内容，按既有惯例替换为 REQ-009 版本；首轮 `qa-report.md` 未动）；只做过 1 处临时变异（`service.rs`），已 `git checkout` 还原并**逐字节校验**；`touch` 6 个文件仅改 mtime（内容 sha256 校验未变）；夹具与探针全部落在 `/tmp/qa-req009/`。
