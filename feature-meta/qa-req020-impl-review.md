# REQ-020「底座可用性验证」实现结果独立复核

- 复核者：Verifier（独立 QA，不复用 BA 结论）
- 复核时间：2026-11-23
- 被测：`feature/REQ-020` @ `52b9314`（工作区 `/home/myg133/Projects/system_seed/feature-REQ-020`，`git status` 干净）
- 目标：`develop` @ `1be47f5`
- 手段：只读（`git` / `grep` / `find` / `wc` / 只读 `psql`）+ 两次非 cargo 的实跑（vitest、svelte-check）
- **未运行任何 `cargo` 命令**（遵守与终版全量测试的互斥要求）、**未修改任何代码**

## 1. 总结论

**有条件通过。**

8 条 AC 中 7 条判据成立、AC-S-187 按规格**如实判「不成立」**（不是隐瞒，规格本身要求「超标即判不成立并归因」）。AC-S-186 的「实际 diff 集合 == 登记清单」经机械比对**逐项相等（30 = 30，无漏项、无虚报）**，是本需求最硬的一条结论。

但存在 **1 条 BLOCKER**：`modules/leave/migrations/` 下的建表迁移**没有任何生产/部署应用路径**，而新 manifest 已声明 `tenantScoped.tables = ["leave_request"]` → 合并后任何**全新环境**执行 `base tenant-policies` 会对不存在的表执行 `ALTER TABLE`（无 `IF EXISTS`）而硬失败，服务起来后 `/api/leave/*` 也会 500。该缺口**既未登记在度量报告的 30 项分类表里，也未登记在「底座缺口清单」7 项里**——而这正是本需求存在的意义所在。

**合并条件**：F-01 必须处置（补模块迁移应用入口，或至少登记为 TD 并证明 `tenant-policies` 不因此硬失败），F-02 的归因表述需修正。

## 2. 8 条 AC 核验表

| AC | BA 声称 | 我的独立结论 | 证据 |
|---|---|---|---|
| **AC-S-186** 平台改动分类，A 类=0 | 成立；实际 30 个文件全归 B/C/D/生成物，A=0 | **成立**（且比 BA 表述更严格地被机械验证） | 见 §2.1：`git diff --name-only 9bcfbcb 52b9314 \| grep -v '^modules/leave/' \| grep -v '^feature-meta/'` = **30**；用脚本抽取 `metrics-report.md` 分类表第三列得 **30**；集合差 `漏项=[]`、`虚报=[]`；分布 `C=16 / B=5 / D=7 / 生成物=2` |
| **AC-S-187** 代码量 < 500 | **不成立**：1978 行（后端 1374 + 前端 604），超 1478 | **「不成立」结论成立，且如实**；但**归因表失准**（F-02） | `find modules/leave/backend/src -name '*.rs' \| xargs wc -l \| tail -1` → `1374`；前端 → `604`；合计 1978（复现逐字相同）。迁移 42 行单独列出（复现）。骨架 458 = 411+47 的**算术**可复现（8 个 `const` 块含声明行：405+6=411，45+2=47），但「模块里实际占 458 行」**不成立**：逐行重合仅 **128/450 行**（见 F-02） |
| **AC-S-188** 复用面 8 组 grep | 八条全满足，阳性对照均 >0 | **成立（8/8，阳性对照全部有效）** | ①`TenantDb\|TenantQuery`=20、②`sqlx::query\|query_as`=**0**（对照 `crates/platform-db/src/`=10）、③`require_permission(`=4、④`jwt\|jsonwebtoken\|argon2`=**0**（对照 `crates/platform-iam/src/`=163）、⑤`AuditEvent\|AuditSink`=9、⑥`platform_audit_log\|platform_audit_append`=**0**（对照 `crates/platform-audit/src/sink.rs`=6）、⑦`NotificationSender\|NotificationMessage`=15、⑧`platform-notify`(leave Cargo.toml)=**0**。另做扩面自查：`modules/leave` 内 `sqlx::` 仅出现在 `AGENTS.md:6` / `README.md:25` 的文档文字里，无宏形式 `query!` |
| **AC-S-189** 审批流 + 计数表 | 实测 建单1/0、提交1/0、审批3/1、非法0/0 | **断言层面成立（逐行断言，非只数条数）**；未能在本机重跑（见 §4） | `crates/platform-server/tests/leave_real_db.rs:930-1047`：第 1 行 `:939-949` 断言 audit 序列 `== ["leave.request.created"]` 且投递为空；第 2 行 `:953-963`（含 TD-088 说明）；第 3 行 `:967-984` 断言投递 `== [(申请人A, in_app, leave_decision)]`；第 4 行 `:990-1007`；第 5 行 `:1015-1046` 断言 `audit_before == audit_after` 且 `deliveries_before == deliveries_after`（即**零增量**）。非法流转 409 + `LEAVE_INVALID_TRANSITION` + `title == "Invalid Leave Transition"`（非兜底 `Error`）见 `:813-831` |
| **AC-S-190** 升级零改动 | 成立；v1 加弃用头、v2 加 `sources`，消费方未动 | **判据成立；但区分力弱**（F-15） | `git diff --name-only fd0d099 -- modules/leave/` → **0 行**（实测空）。`git show 52b9314`：v1 路由 `my_permissions` 函数体**一行未改**，仅新增 `/v2/me/permissions`（`crates/platform-rbac/src/http.rs` 路由处 +65 行）；弃用登记新增 `/api/rbac/me/permissions`（`crates/platform-observability/src/http.rs`）；`Sunset` 严格 IMF-fixdate 由 `crates/platform-observability/tests/http_contract.rs` 的 `PrimitiveDateTime::parse` 断言 |
| **AC-S-191** 度量可复算 | 成立（附 jq 瑕疵已修正） | **成立（抽查 12 个数字全部逐字复现）**；有一处跨交付物数字不一致（F-08） | 复现：1374 / 604 / 1978 / 42 / 30 / `service.rs`491 / `http.rs`307 / `repository.rs`253 / `entity.rs`200 / `mod.rs`99 / `permission.rs`24 / 前端 219+350+35；注释+空行 6 个数全部复现（144/88/68/55/56/16）；内联测试偏移复现（`repository.rs:171`→83 行、`entity.rs:156`→45 行）；`pnpm --filter @base/shell test --run` 实跑 → **`Test Files 29 passed (29)` / `Tests 422 passed (422)`**，与报告逐字相同 |
| **AC-S-192** E2E 1 passed / 0 skipped | 成立 | **测试本体有效，但「绿」依赖库中预先存在的表**（F-01）；`0 skipped` 无法实跑核验 | `e2e/leave.spec.ts:105-173`：真实登录（`:45-52`）→ 真实 API 夹具建单（`:112-122`）→ 点 `leave-new`/填三字段/点 `leave-create`（`:137-142`）→ 切账号登录审批人（`:152-155`）→ 点 `leave-approve`（`:160`）→ `page.reload()` 后断言状态仍「已通过」且审批按钮消失（`:166-172`）。文件内**无** `test.skip`/`fixme`。`playwright.config.ts` 单 project（chromium）、`workers:1`、无 grep 过滤 |
| **AC-S-193** 依赖面 | 成立 | **成立** | `grep -E '^platform-' modules/leave/Cargo.toml` → 仅 `platform-core` / `platform-db` / `platform-contracts`；后者的位置在 `[dev-dependencies]` 段（`modules/leave/Cargo.toml` 第 41-43 行区间）。`Cargo.lock` diff **只新增 `leave` 包**（依赖 anyhow/axum/serde/serde_json/time/tracing/uuid 全是既有 workspace 依赖）+ 给 `platform-server` 加 `leave` 一行，**无新增第三方 crate**。`check-deps` 的豁免名单 `crates/xtask/src/deps.rs:63` `ASSEMBLY_CRATES = ["platform-server"]` → `platform-server → leave` 属允许方向 |

### 2.1 AC-S-186 的机械比对（关键证据）

```
$ git diff --name-only 9bcfbcb 52b9314 | grep -v '^modules/leave/' | grep -v '^feature-meta/' | wc -l
30
# 报告分类表（正则抽第 3 列路径）条目数：30
# 漏项(实际有/报告无): []
# 虚报(报告有/实际无): []
# 分类分布: C=16, D=7, B=5, 生成物=2
```

补充说明（避免误判）：`crates/xtask/src/scaffold.rs` 在报告里以反引号出现，是 AC-S-187 归因的**引用**而非改动登记；`capabilities/index.json` / `capabilities/permissions.json` 是「生成物」行（不计入手改数）。两者都不是漏项。

## 3. FINDING 清单

### F-01（BLOCKER）模块建表迁移没有生产/部署应用路径，且未登记

- **事实**：`modules/leave/migrations/20261123000100_leave_request.up.sql` 是 `leave_request` 表的**唯一**创建者，但仓内**没有任何部署或启动路径**会应用 `modules/*/migrations/`：
  - 服务端迁移只加载一个目录：`crates/platform-server/src/main.rs:312` `load_migrator(&config.migrations_dir)`，`crates/platform-server/src/config.rs:295` 该值来自 `BASE_MIGRATIONS_DIR`，默认 `./migrations`（`config.rs:421` 断言默认值）；
  - 生产 compose：`deploy/compose.prod.yaml:94` `command: ['migrate','up']`，`:115` `command: ['tenant-policies']`；helm 同理（`deploy/helm/base-platform/templates/policies-job.yaml:44`）；
  - E2E 链：`scripts/e2e-server.sh:96` 只跑平台 `migrate up`，`:132` 跑 `scripts/seed-e2e.sh`——后者只种用户/角色/权限点/存储夹具，`grep -rn "leave_request" scripts/ deploy/` **无命中**；
  - 唯一应用模块迁移的地方是**测试夹具**：`crates/platform-server/tests/leave_real_db.rs:300-329`（`apply_module_migrations`，手工遍历 `modules/*/migrations/*.up.sql`）。
- **观察到的后果（可推导，非猜测）**：
  1. 全新库上 `base tenant-policies` 会对 `leave_request` 执行 `ALTER TABLE public.leave_request ENABLE ROW LEVEL SECURITY`——`crates/platform-db/src/rls.rs:139` 生成的语句**不带 `IF EXISTS`**，缺表即报错，`apply_policies` 用 `?` 直接冒泡（`rls.rs:171-182`）→ **部署步骤硬失败**；
  2. 即便跳过策略步骤，服务起来后 `GET/POST /api/leave/requests` 也会因缺表 500；
  3. 当前 `base_test` 里 `leave_request` 有 46 行且 RLS 策略存在，是因为**先前的 Rust 真库测试**把表建出来了 → E2E 的「1 passed」在此环境下成立，但在干净环境不可自举。
- **为什么算 BLOCKER**：合并进 `develop` 后，`modules/leave/manifest.json` 会进入 `modules/` 被 `tenant-policies` 扫描，**平台级部署步骤对所有环境都受影响**，属于本分支引入的回归；而它恰好是本需求本该产出的「底座缺口」之一，却不在 30 项分类表、也不在缺口清单 7 项内。
- **建议**：二选一并写进度量报告：① 给 `migrate`/`tenant-policies` 增加模块迁移的应用入口（真正的缺口修复，建议独立 TD）；② 至少把「模块迁移需人工预应用」写进部署前提（像 `design-summary.md` §3b 对待通知模板那样），并让 `tenant-policies` 对缺失表给出可诊断错误而非裸 SQL 失败。
- **边界**：若贵方有仓外的部署脚本负责应用模块迁移（本仓未体现），本条降级为 MINOR。

### F-02（MAJOR）AC-S-187 归因表把「骨架模板大小」当成「模块内实际存在的必需样板」

- **事实**：`metrics-report.md:158` 记「脚手架骨架 458 行 → 必需样板 458 / 业务逻辑 0」。我按 `crates/xtask/src/scaffold.rs` 的 8 个 `const` 常量逐行与真实文件比对（去空白后精确匹配）：骨架 450 行中只有 **128 行**逐行出现在模块里（28%）：
  ```
  REPOSITORY_RS  骨架193 真实253 重合42 (22%)     # 骨架注释写「内存存储，不引入 sqlx」，真实实现走 TenantQuery → 基本重写
  SERVICE_RS     骨架 65 真实491 重合18 (28%)
  BACKEND_MOD_RS 骨架 56 真实 99 重合34 (61%)
  HTTP_RS        骨架 55 真实307 重合15 (27%)
  PERMISSION_RS  骨架 20 真实 24 重合 4 (20%)
  ENTITY_RS      骨架 16 真实200 重合 5 (31%)
  FRONTEND_ROUTES_TS   骨架26 真实 35 重合 6
  FRONTEND_PAGE_SVELTE 骨架19 真实350 重合 4
  ```
- **影响**：「骨架已占 458 行，预算只剩 42 行给业务逻辑」这一归因**不成立于代码层面**；真实保留的样板约 128 行。「500 行不成立」的**结论不变**（1978 远超），但 AC-S-187 要求的是「超了多少行 + **逐项归因**」，此处归因口径把模板体积误当成实现体积。若据此做「骨架瘦身」需求（缺口清单第 5 项的建议），会以错误的基线立项。
- **建议**：改写为「脚手架起点 458 行；本模块实际保留 128 行，其余为重写/扩写」，并说明骨架的「内存存储」范例与真实 DB 访问的落差本身才是样板偏重的真正原因。

### F-03（MAJOR）平台的 `NotificationSender` 非 dyn 兼容，迫使每个消费方自写装箱 wrapper（BA 未登记为缺口）

- **事实**：`crates/platform-core/src/extension/notification.rs:81-91` 的 `send()` 返回 `impl Future` → trait object 不安全，`Arc<dyn NotificationSender>` 写不出来（`modules/leave/backend/src/service.rs:112-125` 的注释称实测 E0038）。于是业务侧必须自造一层 `Notifier` + blanket impl（`service.rs:126-143`，约 18 行）才能把 sender 存进状态。
- **影响**：这是「加一个业务模块要写多少样板」的直接构成（每个消费方都要抄一遍），但缺口清单 7 项里只提了「没有落库适配器」，没提「trait 不可装箱」。对比同目录 `AuditSink` 是可 `Arc<dyn>` 的（`leave` 直接持有），两者不一致。
- **建议**：新增缺口项，或把 trait 改成返回 `BoxFuture`（平台既有 `BoxFuture` 类型已存在，`leave` 已在用）；否则 AC-S-187 的行数超标会持续被这 18 行/模块摊薄。

### F-04（MINOR）落库与审计非原子：审计失败会「数据已改但返回 500」

- **事实**：`modules/leave/backend/src/service.rs:296-301`（先 `repository.insert` 再 `record`）、`:413-423`（先 `repository.transition` 再 `record`）、`record` 失败映射为 `LeaveError::Audit` → 500（`service.rs:172-174`）。两者不在同一事务里。
- **影响**：审计 sink 故障时，客户端收到 500 但状态已推进/单据已建；重试会得到 409（重复提交）或重复建单（建单无幂等键）。AC-S-189 的计数表不覆盖此路径。
- **建议**：要么把「写库 + 写审计」放进同一事务，要么在 500 的 detail 里区分「已生效但审计失败」，并给建单加幂等键。

### F-05（MINOR）列表未按设计实现 cursor 分页，且偏差未登记

- **事实**：`design-summary.md` §1.3 写「列表（cursor 分页）」、§1.4 把「分页 = 平台 cursor 契约」列为复用项；实现返回**裸 JSON 数组** + `limit`（`modules/leave/backend/src/http.rs:218-232`，`repository.rs:42-52` 注释自认「cursor 的编码形态平台侧还没有统一编解码器」）；AC-S-189 的测试直接 `listed.as_array()`（`leave_real_db.rs:668`）。
- **影响**：响应信封与平台其它列表接口（`List`）不一致，且这条「平台缺 cursor 编解码器」的缺口只在代码注释里，没进缺口清单。
- **建议**：登记为缺口或改用平台 `List` 信封。

### F-06（MINOR）`service.rs:475` 的注释与实际适配器行为矛盾

- **事实**：`service.rs:475-477` 写「审批人身份由平台适配器按权限点解析（业务侧不查权限表）」；但 `crates/platform-notify/src/db_sender.rs:107-112` 在 `recipients` 为空时**直接**返回 `DeliveryResult::failed`，**从不读取** `payload["audience"]`。
- **影响**：注释承诺的能力不存在（同文件 `:306-315` 的文档反而写对了）。后续有人依赖该注释排查「审批人没收到通知」会走错方向。
- **建议**：把 `:475` 改成「适配器当前不解析 audience（TD-088），此处仅保留意图」。

### F-07（MINOR）AC-S-189 的生产接线未被测试覆盖（测试复刻了一份认证中间件）

- **事实**：`leave_real_db.rs:29-38` 明确登记「`authenticate_module` 是 `main.rs`（bin）私有函数，测试只能逐行复刻」。我逐行比对 `leave_real_db.rs:138-198` 与 `crates/platform-server/src/main.rs:1913-1977`：**当前完全一致**（含 x-tenant-id 与令牌租户不一致 → 404、`ensure_fresh` → 500）。
- **影响**：今天一致 ≠ 明天一致；生产中间件改动后测试仍会绿。属已披露的结构性风险（根因 TD-076）。
- **建议**：加一条「复刻块与生产块文本指纹一致」的守护测试，或把 `build_router`/中间件抽到 lib。

### F-08（MINOR）同一指标的通过数在两处交付物里不一致

- **事实**：`metrics-report.md:107` 记 `platform-observability: 127/0/0`；提交 `52b9314` 的提交信息写「platform-observability **110** passed（含 v1 带头 / v2 不带头 / leave 不带头 三条）」。该 crate 内 `#[ignore]` 数量为 **0**（`grep -rn "#\[ignore" crates/platform-observability/` → 0），所以「110 vs 127 是 --include-ignored 差异」解释不通。
- **影响**：AC-S-191 要求「同一 commit 换人重跑逐字相同」；两个数字必有一个是错口径或错时点。
- **建议**：统一到 `--include-ignored` 口径并重跑贴原文。

### F-09（MINOR）分类一致性：同一类「新错误码的测试载体」被分到 C 与 D 两个类

- **事实**：`#26 crates/platform-server/tests/i18n_titles.rs` 记为 **D（缺口修复）**；而性质相同的判据载体 `#13 router_assembly.rs` / `#14 leave_real_db.rs` / `#15 db_sender_real_db.rs` / `#16 common/mod.rs` 记为 **C**。按 D353 的定义（B=升级示范、C=装配必需、D=补平台能力），一个断言标题行数的测试既不是「升级」也不是「补能力」。
- **影响**：分类口径可解释性受损，A 类=0 的结论虽不受影响，但「逐项归因」的说服力下降。

### F-10（MINOR）缺口清单 #1「每个新模块改平台 3 个文件」对 C 类实测 16 项不完整

- **事实**：`metrics-report.md:241` 与 `status.md:77` 都写「每个新模块要改平台 3 个文件（根 Cargo.toml / server Cargo.toml / main.rs）」。实测 C 类共 **16 个文件**，其中换模块必然重演的还有：权限种子迁移 2 个（`migrations/20261123000200_*`，因为平台只应用 `migrations/` 单个目录）、`capabilities/*` 2 个（生成物）、自定义页场景下 `apps/shell` 3 个（App.svelte / routes.ts / tsconfig.json）、以及 5 个判据载体。
- **影响**：「3 个文件」是**装配点**子集，不是「新模块的平台改动面」。本需求的核心产出之一就是量这个数，取子集当总数会低估后续排期。
- **建议**：写成「装配 3 个 + 权限种子 2 个 + 生成物 2 个 + 自定义 UI 时 3 个 + 判据载体 ~5 个」。

### F-11（MINOR）真库证据在共享库上不可事后追溯

- **事实**：`crates/platform-audit/tests/common/mod.rs:169-182` 的 `reset_audit()` 执行**无租户条件的** `DELETE FROM platform_audit_log` / `platform_audit_archive` / `platform_audit_chain_state`。实测后果：`base_test` 里 `leave_request` 有 46 行（4 种状态齐全）、`platform_audit_chain_state` 只剩 `tenant-a` / `tenant-req018-audit` 两个租户，`platform_audit_log` 中 `object_type like 'leave%'` 的行数为 **0**——正是被该夹具抹掉的。
- **影响**：AC-S-189 声称「查真表」，但任何人事后去查都查不到（跨 crate 互相清表），削弱证据链可复核性。**注意：这不是 REQ-020 的实现缺陷**（我已排除「装配没写审计」这一假设），是平台测试卫生问题。
- **建议**：改成按租户/按 id 清理。

### F-12（NIT）AC-S-188 ④ 的阳性对照数值已过期

- `acceptance.md` 写 `crates/platform-iam/src/` → 159；实测 **163**。仍 >0，不影响判定，但 AC 里的「实测值」应刷新。

### F-13（NIT）`decisions.md` D353 的登记要求仍写旧口径

- `decisions.md:201` 写「与 `git diff BASE -- crates/` 的实际输出逐项对应」，与 AC-S-186 已改为「全仓 diff 减 `modules/leave/`」不一致（实现按新口径做，30 项里含 `apps/`、`packages/`、`migrations/`、`scripts/`，按旧口径会全部漏判）。属规格残留，不影响实现。

### F-14（NIT）允许自批：`decide` 不禁止 applicant == approver

- `modules/leave/backend/src/service.rs:352-386` 只校验状态机与权限点；若同一用户同时持有 `leave:request:approve`，可批准自己的单。规格未要求职责分离，属观察项。

### F-15（NIT）AC-S-190 的「零改动」区分力弱

- 消费方前端经 `@base/auth` 的 `can()` 读**默认路径** `/api/rbac/me/permissions`（`packages/auth/src/index.ts:244,279`），代码里**从未出现** `v2`（`grep -rn "v2" modules/leave/ apps/shell/src/ packages/auth/src/` 无命中）。v1 又一行未改 → 「消费方零改动」近乎结构性必然，并未真正验证「消费方能在破坏性变更下存活」。这是 D352/D349 的口径选择，不是实现问题；但结论的说服力应如实标注（design-summary §2.2 已部分承认）。

## 4. BA 声称但本次无法核验的项

| 项 | 原因 |
|---|---|
| 全量测试 **684 passed / 0 failed / 0 ignored**（逐 crate 6 个） | 需 `cargo test`，与并行的终版全量测试互斥，**按指令禁跑**。仅核对算术：153+149+86+127+25+144 = **684**（自洽） |
| `check-modules` / `check-deps` / `capabilities --check` 退出码 **0/0/0** | 需 `cargo xtask`，禁跑。间接证据：`capabilities/permissions.json` 的 `leave` 段与 `modules/leave/manifest.json` 三个权限点逐字一致，`index.json` 的 `platform.modules` 已含 `leave`、`permissions.total` 41→44 |
| `leave: 25/0/0`（含计数表用例） | 同上。DB 快照无 leave 审计行，但已证明由 `reset_audit()` 造成，**不能**据此判失败（见 F-11） |
| E2E `1 passed / 0 skipped` | 需起 `platform-server` + 浏览器 + 库，且会与并行全量冲突。只做了代码级有效性判断（见 AC 表） |
| AC-S-190 步骤 2「升级后消费方全部测试 + e2e 通过」 | `metrics-report.md` 写于两个提交**之前**（其 `BASE_UPGRADE = 不存在`），所以报告不可能覆盖 `52b9314` 之后的消费方回归；`52b9314` 提交信息只列了平台侧两个 crate |
| `pnpm --filter @base/shell build`（162 modules / 181.23 kB） | 会写 `apps/shell/dist`（E2E 的 webServer 也用它），为避免干扰未跑 |

## 5. 我发现但 BA 完全没提的问题

1. **F-01** 模块迁移无应用路径（BLOCKER）——最重要的一条。
2. **F-03** `NotificationSender` 非 dyn 兼容导致的「每模块自写装箱 wrapper」样板，未进缺口清单。
3. **F-05** 列表缺 cursor 分页、响应信封与平台不一致，只在代码注释里承认。
4. **F-11** 平台审计测试夹具全表 `DELETE`，使「查真表」类判据无法事后复核。
5. 顺带核实（**不是**问题，避免误报）：`modules/leave` **没有**自造审计/租户隔离/鉴权的等价物——`grep` 结果与代码结构一致（审计走注入的 `AuditSink`、租户走 `TenantQuery` 的 `{tenant}` 占位符且 RLS 策略实测存在于 `pg_policy`、鉴权走 `require_permission`）。两处「看起来像自造」的代码（`Notifier` 装箱、`parse_date` 手写解析）各有真实约束（trait 非 dyn 兼容；`time` 未开 `parsing` feature）。

## 6. 复核动作清单（可重放）

```
git diff --name-only fd0d099~1 fd0d099 | wc -l                  # 47
git diff --name-only fd0d099 52b9314 | wc -l                    # 5
git diff --name-only 9bcfbcb 52b9314 | grep -v '^modules/leave/' | grep -v '^feature-meta/' | wc -l   # 30
git diff --name-only fd0d099 -- modules/leave/ | wc -l          # 0
find modules/leave/backend/src -name '*.rs' | xargs wc -l | tail -1        # 1374
find modules/leave/frontend -type f \( -name '*.svelte' -o -name '*.ts' \) | xargs wc -l | tail -1  # 604
grep -rn "sqlx::query\|sqlx::query_as" modules/leave/backend/src/ | wc -l  # 0
pnpm --filter @base/shell test --run                            # 29 files / 422 tests passed
pnpm --filter @base/shell check                                 # 0 errors / 0 warnings
```

---

## BA 回执（2026-11-23，15 条全处置）

逐条处置表见 `demands/REQ-020-platform-consumption/metrics-report.md`
末尾「独立复核（实现阶段）的 15 条处置」。要点：

| # | 处置 |
|---|---|
| **F-01** BLOCKER | **成立，严重度降为 MAJOR**。我在干净库上复现了（`migrate up` → 表不存在 → `tenant-policies` exit=1）。但用 `develop`（`1be47f5`）的 xtask 在同一干净库上跑**同样 exit=1**（报 `sandbox_tenant_item`）→ **既存缺口，非本分支引入**，原判「本分支引入回归」的前提不成立。已登记 **TD-089（P1）** + 缺口清单 #8 + 写进 `design-summary.md` §3b 部署前提。**你连带指出的「`tenant-policies --check` 在 `base_test` 上是假绿」我确认成立，已写进 metrics-report。** |
| **F-02** MAJOR | 成立，已修正。我按带重数的逐行精确匹配重算：模板非空 396 行 / 真实 1613 行 / **重合 107（27%）**；`REPOSITORY_RS` 只留 29/172。归因表与结论已改写，复算命令附在报告里。 |
| **F-03** MAJOR | 成立，已登记 **TD-090** + 缺口清单 #9。 |
| **F-04** MINOR | 成立，登记 **TD-092**。 |
| **F-05** MINOR | 成立，登记 **TD-091** + 缺口清单 #10。 |
| **F-06** MINOR | 成立，**已改代码**：先确认全仓无任何代码消费 `payload["audience"]`，且 `db_sender.rs:30` 文档也明说「不猜收件人」→ 注释确属错误。已改为如实说明（TD-088 + 会落为合法失败）。改后重跑 `-p leave`：**25 passed / 0 failed / 0 ignored**。 |
| **F-07** MINOR | 成立，已知结构性风险（根因 TD-076），本轮不修。 |
| **F-08** MINOR | 成立。权威数取 **127**（终版全量实测）；提交信息里的 110 是旧口径，提交信息不可改写，以度量报告为准。 |
| **F-09** MINOR | 成立，已统一：新增「分类规则」——**测试文件（判据载体）一律归 C**，`i18n_titles.rs` 由 D 改 C。分布 C17/B5/D6/生成物2，**A 仍为 0**。 |
| **F-10** MINOR | 成立，已改为「装配 3 + 权限种子 2 + 生成物 2 + 自定义 UI 3 + 判据载体约 5 = 实测 C 类 17」。 |
| **F-11** MINOR | 成立（平台测试卫生，非本需求缺陷），登记 **TD-093**。**感谢你据此排除「装配没写审计」的误判。** |
| **F-12** NIT | 已修：`acceptance.md` 阳性对照 `iam` 159 → **163**。 |
| **F-13** NIT | 已修：`decisions.md` D353 登记口径由 `git diff BASE -- crates/` 改为「全仓 diff 减 `modules/leave/`」。 |
| **F-14** NIT | 成立但不在射程（规格未要求职责分离），记 **TD-094（P3）**，不改实现。 |
| **F-15** NIT | 如实标注：AC-S-190 结论成立，但**不宣称「消费方能扛住破坏性变更」**。 |

### 你提的「无法核验项」，现已补齐实测（在同一 `52b9314` 上）

| 项 | 实测 |
|---|---|
| 全量 19 crate（不止 6 个） | **1673 passed / 0 failed / 0 ignored**（逐 crate，`--all-targets -- --test-threads=1 --include-ignored`） |
| `check-modules` / `check-deps` / `capabilities --check` | 退出码 **0 / 0 / 0** |
| `tenant-policies --check` | **base_app** 角色 exit 0、无警告；**base** 角色 exit 0 但带 fail-open 警告；**干净库** exit 1（见 F-01） |
| E2E | `{"expected":1,"skipped":0,"unexpected":0,"flaky":0}`（需带 `DATABASE_URL_TEST`，否则 webServer exit 2） |
| `pnpm --filter @base/shell build` | 162 modules / 181.30 kB / built in 1.76s |
| AC-S-190 步骤 2「升级后消费方全部测试通过」 | 升级后 `git diff fd0d099 -- modules/leave/` = 0 → 消费方代码与测试**逐字未变**，其 25 条测试在终版全量中通过 |

**合并条件已满足**（F-01 已解释为既存缺口并登记 TD-089 + 部署前提；
F-02 归因已修正）。
