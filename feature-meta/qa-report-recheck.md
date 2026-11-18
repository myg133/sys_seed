# 验证报告 —— 修复复核（Recheck）

- 需求: REQ-007 认证与账号体系
- 分支: feature/REQ-007 @ `f2ad7b9`（修复轮 3 提交：`4f9d536` 修复枚举面与锁定续期 / `5b2b80a` 补防枚举与锁定回归用例 / `f2ad7b9` 补验证报告与偏离登记；基线 develop @ `91d8e09`）
- 首轮报告: `feature-meta/qa-report.md`（结论 **FAIL**：2 HIGH 阻塞 —— 账号枚举面 + 锁可无限续期）
- 复核范围: 6 个改动点 + 一次回归（**不重做全量验证**）
- 验证者: QA Agent（独立复现；**未采信 Dev 自验报告里的任何数字与结论**，全部自己重跑、自己计时、自己变异）
- 结论: **PASS**

> **核心结论：首轮 2 个 HIGH 阻塞项均已真实修复，且不是「碰巧修好」—— 我用双向变异证明护栏是承重的。**
>
> - **HIGH 1（账号枚举面）**：我把「锁定判定」挪回密码校验之前，用例**立刻变红**（5 条）；把 `ACCOUNT_LOCKED` 永久改成 `INVALID_CREDENTIALS`，另 2 条变红。
>   → 护栏守的正是那个洞，**双向都守**。
> - **HIGH 2（锁无限续期）**：锁定期内 `failed_attempts` 不增、`locked_until` **逐字节不动**；到期后第一次错密码回到 `failed_attempts=1` / `locked_until=NULL`；到期后**正确密码能登录** → **无永久锁死路径**。
> - **MEDIUM 3**：三条失败路径中位数 592.4 / 567.4 / 638.7 ms → **最差比值 1.126**（修复前 75.3×）。
> - **LOW 5 / LOW 6**：改密保留当前设备、IP 限流跨租户共享且只算失败 —— **均实测复现为真**。
>
> 回归全绿，依赖树**零变更**（第三方 232 crate，与上一轮持平），CODEOWNERS 三个保护目录**零改动**。
> 剩余 3 条均为 **LOW / 观察级**，无阻塞项。

---

## 逐项复核

| # | 问题 | Dev 声称 | 复核结果 | 证据 |
|---|------|---------|---------|------|
| **HIGH 1** | 账号枚举面（`ACCOUNT_LOCKED` 是枚举预言机） | 存在/不存在各 6 次全部 401 `INVALID_CREDENTIALS`，**不可区分 = True**；锁定期内正确密码 → `ACCOUNT_LOCKED` | ✅ **已修复** | 我自己跑真机（`base serve` + 应用角色 `base_app` + 生产 Argon2 m=19456/t=2/p=1）：**6 轮逐次**状态码 / 错误码 / 响应体（去 `trace_id`）/ 响应头 **全部相同**；第 6 次越阈值仍 401 `INVALID_CREDENTIALS`（DB 证明此时账号**已锁**：`failed_attempts=5`、`locked_until=2026-11-18 08:38:21.920416`）；锁定期内正确密码 → **403 `ACCOUNT_LOCKED`**；泄号头扫描 = 空。**双向变异验证护栏承重**（见 MED 4） |
| **HIGH 2** | 锁可被无限续期 → 永久登录拒绝服务 | 锁定期内不前移/不增；到期后 `failed_attempts=1`、`locked_until=NULL` | ✅ **已修复** | 锁定期内再错一次 → `failed_attempts 5→5`、`locked_until 08:38:39.068376 → 08:38:39.068376`（**逐字节不变**）；**锁定期内用正确密码**（走 `ACCOUNT_LOCKED` 分支）→ 同样不变；连续多次失败仍不变；人为把 `locked_until` 置到过去 → 再错一次 → `failed_attempts=1`、`locked_until=NULL`；到期后**正确密码登录 200**（无永久锁死）；成功后计数清零（`failed_attempts=0`） |
| **MEDIUM 3** | 三条失败路径耗时对齐（锁定路径跳过 Argon2 → 75× 时序差） | 563.1 / 569.0 / 543.2 ms，最差比值 **1.048** | ✅ **已修复** | 每条路径 12 个样本，取**真中位数**：错密码(未锁) **592.4ms** / 错密码(已锁) **567.4ms** / 账号不存在 **638.7ms** → **最差比值 1.126**（<1.5）。锁定路径耗时与普通失败同量级 → **确实执行了 Argon2**；另证：锁定期内**正确密码**的 403 分支 **603.9ms**（同样经 Argon2）。枚举面两侧耗时比值 **1.026**（存在 503.9 vs 不存在 491.3） |
| **MEDIUM 4** | 防枚举用例必须守**两个方向** | 双向变异：①「锁定判定挪回密码校验前」→ 3 条变红；②「不再返回 `ACCOUNT_LOCKED`」→ 1 条变红 | ✅ **已修复**（我独立做了**两个**方向的变异，Dev 的「3 条 / 1 条」偏低） | **变异①**（我把 `service.rs` 的两个判定块对调：密码校验 → 锁定判定，改成 锁定判定 → 密码校验）：`cargo test -p platform-iam --test iam_real_db -- --include-ignored` → **10 passed / 5 failed**，变红的是 `six_wrong_passwords_never_reveal_that_the_account_exists`、`the_three_failure_paths_have_similar_latency`、`a_lock_is_never_extended_and_the_failure_count_restarts_after_it_expires`、`repeated_failures_lock_the_account`、`the_ip_rate_limit_is_shared_across_tenants_and_only_counts_failures`（失败断言原文：`第 6 次（账号存在）必须是 401 … left: 403 right: 401`）。**变异②**（`AccountLocked` → `InvalidCredentials`）：**14 passed / 2 failed**，变红的是 `a_locked_account_with_the_right_password_is_told_it_is_locked`、`repeated_failures_lock_the_account`。**两次变异后都逐字节还原**：sha256 == HEAD blob `b60702eeab16406851cfe7d914ed48586ae0a37e3fb51589f10ce122e40b7109`，`git diff crates/` 为空；还原后重新编译并重跑枚举探针，结论不变 |
| **LOW 5** | 改密把「当前设备」的 refresh 也作废了 | 改密 204 → 其它设备 refresh 401 `TOKEN_REVOKED` → 当前设备 refresh 200 | ✅ **已修复** | 设备 A / B 各登录一次（`x-device-label` 区分）→ 用 A 的 access token 改密 → **204** → B 的 refresh **401 `TOKEN_REVOKED`** → **A 的 refresh 200**；改密后用新密码登录 200；`GET /sessions` 返回 2 条 |
| **LOW 6** | IP 限流按租户计 + 把成功登录也计数 | 第 21 次 → 429 `LOGIN_RATE_LIMITED`；换租户 B 同 IP 仍 429；另一 IP 20 次成功登录后错一次仍 401 | ✅ **已修复** | 同 IP 同租户连打：**前 20 次全部 401**（状态码集合 `[401]`），**第 21 次 → 429 `LOGIN_RATE_LIMITED`**（3.4ms）；**同 IP 换租户 B → 仍 429 `LOGIN_RATE_LIMITED`**；**另一 IP：20 次成功登录（20/20）后错一次 → 401 `INVALID_CREDENTIALS`**（非 429） |

---

## 枚举面实测

真机（`./target/debug/base serve`，应用角色 `base_app` 自检通过：`租户隔离前提自检通过：连接角色不绕过 RLS role=base_app`），
生产默认 Argon2 参数（m=19456 / t=2 / p=1），同一租户、同一 IP（`x-forwarded-for`）。

| 轮次 | 存在账号（alice） | 不存在账号（ghost-never-registered） | 可区分? |
|------|--------------------|-------------------------------------|---------|
| 1 | **401** `INVALID_CREDENTIALS` 585.0ms | **401** `INVALID_CREDENTIALS` 510.5ms | **否** |
| 2 | **401** `INVALID_CREDENTIALS` 478.3ms | **401** `INVALID_CREDENTIALS` 497.6ms | **否** |
| 3 | **401** `INVALID_CREDENTIALS` 547.7ms | **401** `INVALID_CREDENTIALS` 484.0ms | **否** |
| 4 | **401** `INVALID_CREDENTIALS` 494.4ms | **401** `INVALID_CREDENTIALS` 494.8ms | **否** |
| 5 | **401** `INVALID_CREDENTIALS` 494.4ms | **401** `INVALID_CREDENTIALS` 485.3ms | **否** |
| **6（越过锁定阈值）** | **401** `INVALID_CREDENTIALS` 513.5ms | **401** `INVALID_CREDENTIALS` 487.8ms | **否** |

- 逐项比对：`状态码逐次相同 [True×6]` · `错误码逐次相同 [True×6]` · `响应体逐次相同 [True×6]` · `响应头逐次相同 [True×6]`
- 状态码集合：存在 `[401]` / 不存在 `[401]`；错误码集合：存在 `['INVALID_CREDENTIALS']` / 不存在 `['INVALID_CREDENTIALS']`
- 第 6 次响应体（两侧**逐字节相同**，仅 `trace_id` 不同 —— UUIDv4）：
  `{"code":"INVALID_CREDENTIALS","detail":"invalid username or password","instance":"/login","status":401,"title":"凭证无效","type":"https://errors.base.example/invalid-credentials"}`
- 第 6 次响应头（两侧相同）：`{'content-type': 'application/json', 'content-length': '233'}`（除 `date` 外无差异）
- **DB 证据（第 6 次请求时账号确实已被锁）**：`failed_attempts=5`、`locked_until=2026-11-18 08:38:21.920416`
- **锁定期内用正确密码** → `403 ACCOUNT_LOCKED`（`"detail":"the account is temporarily locked"`）→ 「被锁用户知道自己被锁」**未被改坏**
- 泄号头扫描（`user` / `exist` / `account`）= **空**
- 枚举面两侧耗时比值 **1.026**（存在 503.9ms vs 不存在 491.3ms）

---

## 回归

| 项 | 结果 |
|----|------|
| **Rust（非 ignored）** `cargo test --workspace -- --test-threads=2` | ✅ **225 passed / 0 failed / 24 ignored**（与 Dev 声称一致） |
| **Rust（含 ignored，真实库真跑）** `cargo test --workspace -- --include-ignored --test-threads=2`（`DATABASE_URL_TEST` 已设） | ✅ **249 passed / 0 failed**（真实库层 `iam_real_db` **16/16**、`quota_real_db` 2/2、`c3_real_db` 4/4）—— ⚠️ **Dev 自验报告写的是 240，实际 244（`--workspace --tests` 口径）/ 249（`--workspace` 含 doctest）→ Dev 的数字偏低**，见「仍存在的问题」#1 |
| **Lint** `cargo clippy --workspace --all-targets -- -D warnings` | ✅ **exit 0，0 告警** |
| **格式** `cargo fmt --all --check` | ✅ **exit 0** |
| **工程门禁** `cargo xtask capabilities --check` | ✅ exit 0（13 个文件与源码一致） |
| **工程门禁** `cargo xtask check-deps` | ✅ exit 0（平台 crate 未反向依赖业务模块） |
| **工程门禁** `cargo xtask check-modules` | ✅ exit 0（1 个模块 sandbox） |
| **工程门禁** `cargo xtask tenant-policies --check` | ✅ exit 0（**已先跑 `scripts/test-integration.sh` 重置库**，符合「库状态敏感」的顺序要求） |
| **集成脚本** `scripts/test-integration.sh` | ✅ **exit 0**，**容器数量比对通过：2 → 2**，真实库层真跑（我**跑了两次**：开跑前 + 收尾重置，均 exit 0） |
| **前端单测** `pnpm -r test` | ✅ **23 passed / 23**（8 个包：auth 3 · schema 3 · i18n 3 · hooks 3 · theme 2 · sdk 2 · ui 3 · shell 4） |
| **前端 E2E** `bun run test:e2e` | ✅ **1 passed / 1**（chromium，5.3s） |
| **依赖树** `cargo tree --workspace --prefix none --all-features` | ✅ **240（总数）− 8（工作区成员）= 232 个第三方 crate**，与上一轮**持平**；`git diff e755e5c..f2ad7b9 -- Cargo.lock '*Cargo.toml'` **为空** → **本轮零依赖变更** |
| **CODEOWNERS 保护目录** | ✅ `crates/platform-core/` **0 改动** · `crates/platform-contracts/` **0 改动** · `crates/platform-contracts-macros/` **0 改动** · `modules/` **0 改动**；`crates/platform-db/` 3 文件、`crates/platform-server/` 2 文件 —— 均为**首轮已有**变更，**本轮 3 个提交未触碰这两个目录**（首轮已判 PASS：纯增量、无既有行为改动） |

### 工作树与库状态

| 项 | 状态 |
|----|------|
| `git status --porcelain`（feature-REQ-007） | 仅 `M feature-meta/qa-report.md`（**首轮 QA 报告，未提交 —— 我进场时就是这个样子，我没动它**）+ 本报告 `feature-meta/qa-report-recheck.md` |
| `crates/` 与 `migrations/` | **零改动**（两次变异均逐字节还原，sha256 == HEAD blob） |
| `base_test` | ✅ 已重置；`platform_iam_user` 里 `tenant_id like 'qa-%'` 的行数 = **0**（我造的临时账号全部清掉） |
| `base_dev` | ✅ **零 IAM 表、零写入**（`information_schema.tables` 里 `platform_iam%` 计数 = 0） |
| 容器 | ✅ **2 个**（`base-postgres` / `base-redis`，均 healthy），全程未新建容器 |
| 本机 psql | 无（走 `podman exec base-postgres psql -U base -d base_test`） |
| 应用连接角色 | `base_app`（普通角色，非超管）—— 服务自检日志：`租户隔离前提自检通过：连接角色不绕过 RLS role=base_app` |

---

## 仍存在的问题

**无阻塞项。** 以下全部为 LOW / 观察级。

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| 1 | LOW | **Dev 自验报告的回归数字偏低**：其表格声明 `cargo test --workspace --tests -- --test-threads=2 --include-ignored` → **240 passed**，实测**同一命令 244 passed**（`--workspace` 全量含 doctest = **249**）。偏差方向安全（少报不是多报），但**自验报告的数字不能直接引用** | `scripts/test-integration.sh` 输出：`test result: ok. … 244 passed; 0 failed`；`cargo test --workspace -- --include-ignored` → 249 passed（`iam_real_db` 16 + `quota_real_db` 2 + `c3_real_db` 4 + 单测 222 + doctest 5） | 由 Dev 更正 `feature-meta/verification-report.md` 的数字；QA 报告中一律以**实测**为准 |
| 2 | LOW | **`iam_real_db.rs:1288-1295` 的「会话都带 tenant_id」断言是全表扫描**，不是按本用例自己的租户过滤：`SELECT tenant_id, user_id FROM platform_iam_session` 后逐行断言 `tenant_id.starts_with("iam-test-")`。库里只要存在**任何**非 `iam-test-` 前缀的会话（同库其它租户、手工造的探针数据、并行跑的其它测试），这条用例就**误报变红** | 我跑变异①时第一次结果有 6 条变红，其中 `tokens_cannot_cross_tenants_and_sessions_record_the_tenant` 变红的原因**与变异无关**：我的探针租户 `qa-recheck-lock` 有会话 → `panicked at iam_real_db.rs:1293: 每条会话记录都必须带 tenant_id：qa-recheck-lock`。清掉探针数据后重跑 → 变红数从 6 降到 5（全是真正的变异后果） | 断言改成 `WHERE tenant_id = <本用例租户>`（只查自己造的会话），或把断言放宽为「非空」；否则 CI 上「谁先跑谁污染」，脆 |
| 3 | LOW | **`ACCOUNT_LOCKED` 分支也会写一条限流失败计数**：`service.rs` 的 403 分支调用 `record_failed_login` → 内部 `record_login_rate_in` 写入 `platform_iam_login_rate`。后果：**知道密码的被锁用户**每次用**正确密码**登录都会消耗同 IP 的 20 次/5 分钟失败预算；用正确密码反复试可把该 IP 打成 429（限流窗口内同 IP 的其他人一起被 429） | 实测：5 次错密码后 rate 表 81 行 → **锁定期内用正确密码（403 `ACCOUNT_LOCKED`）后 82 行**（+1）；再错一次 → 83 行 | 若认为「密码正确但被锁」不应计入失败预算，可在 403 分支改用只记流水的路径；否则在 `traceability.md` 登记为已知取舍（属 LOW） |
| 4 | LOW | **限流预算现在是「IP 全局、跨租户共享」**（Dev 按 QA 首轮建议改的），代价是**一个租户的失败洪峰会连带影响共享同一出口 IP 的其它租户**（同一 NAT 出口的多租户办公网 → 一家被攻击 → 全部 429）。这正是首轮建议的方向（首轮问题是「按租户计 → 预算被放大成租户数倍」），属于**有意的取舍**，但**共享 NAT 场景下的连带影响此前未被登记** | `service.rs:445-470` `check_ip_rate_limit` + `platform_iam_login_rate`（**无 `tenant_id`、无 RLS**，`relrowsecurity=f`，只存 IP 的 SHA-256 摘要）；实测同 IP 换租户 B → 仍 429 | 在 `traceability.md` §5 偏离清单里补一条「限流预算跨租户共享 → 同出口 IP 的租户互相影响」；表里只存 IP 摘要（非明文）、不存任何租户数据，这个设计本身是好的，建议保留 |
| 5 | 观察 | **`platform_iam_login_rate` 是 `public` schema 下**无 RLS** 的全局表**，`base_platform_app` 组角色对它有 SELECT/INSERT（实测 `has_table_privilege('base_app', …, 'SELECT')=t`、`INSERT=t`）→ 任何持有该组的角色都能读**全表**的 IP 摘要（无明文 IP、无租户数据、无用户名）。风险可控，但这是本期**新增的一张跨租户共享表** | `migrations/20261118000500_iam_login_rate.up.sql`；`pg_class.relrowsecurity=f`；`deploy/seed/02-app-role.sql` 的 `ALTER DEFAULT PRIVILEGES` + `ON ALL TABLES` 两种顺序都能覆盖到这张表（迁移里的条件 GRANT 是第三重保险） | 保持现状可接受；若要更严，可考虑把该表放进独立 schema 并只授 INSERT/SELECT 必要列。建议在 `traceability.md` 登记 |
| 6 | 观察 | **失败计数是「读-改-写」，非原子**：锁定判定与计数更新用的是同一次读取的 `user` 快照（`service.rs` step 2 的 `find_user_by_username`）→ 并发失败请求下 `failed_attempts` 存在 lost update 风险（计数可能少记）。首轮与本轮都**未做并发实测** | 代码结构（`next_lock_state(user, now, …)` 吃的是快照）；本轮所有探针均为串行 | 并发压力下验证：同一账号 10 个并发错密码，`failed_attempts` 应恰好到 10。若确实少记，用 `UPDATE … SET failed_attempts = failed_attempts + 1` 改成原子自增 |

---

## 未覆盖 / 无法验证

| 项 | 原因 | 说明 |
|----|------|------|
| **并发下的锁定计数正确性** | 本轮未做并发压测 | 失败计数是快照读-改-写（见「仍存在的问题」#6），并发下可能少记；**未验证** |
| **多副本/多实例下的限流与锁定一致性** | 单机单实例 | 限流是 DB 计数（本轮新表 `platform_iam_login_rate` **无租户维度**，多实例共享 DB 时天然跨实例，但**未实测**）；首轮已登记同项 |
| **密钥轮换的跨进程行为 / `BASE_IAM_SECRET_KEY` 轮换** | 超出本轮 6 项范围 | 首轮已登记，本轮未重做 |
| **生产部署链路（Post-merge）** | 本阶段是 Pre-merge | 镜像构建、K8s Secret 注入、`base tenant-policies` 在生产库上的表现未验证；`platform_iam_login_rate` 的生产授权依赖 `deploy/seed/02-app-role.sql` 的执行顺序（两种顺序我都核过逻辑，覆盖完整，但**未在真实生产库验证**） |
| **前端登录 / MFA 的 UI 验收面** | 本期无前端接入面 | 前端只有占位导航项，**没有登录页** → 前端无法做 IAM 端到端验收（首轮同项） |
| **AC-S-020 依赖预算重定基线** | 属 BA 记账，非代码 | 实测第三方 232 crate，仍超 AC-S-020 的 200~210；Dev 已在 `traceability.md` §5.1 与 `status.md` 登记「由 BA 把预算重定基线到 235」→ **等 BA 动作** |

---

## 附：本次验证的自证材料

| 材料 | 位置 | 说明 |
|------|------|------|
| 探针脚本 | `/tmp/qa-req007-recheck/p1_enum.py` / `p2_lock.py` / `p3_latency.py` / `p5_chpwd.py` / `p6_ratelimit.py`（共用 `qa.py`） | 全部基于**真实 HTTP 服务器**（`./target/debug/base serve`，连 `base_test` + **应用角色 `base_app`**），**不是**单元测试替身；用 `x-forwarded-for` 精确控制来源 IP 以便分场景避开限流 |
| 探针原始输出 | `/tmp/qa-req007-recheck/p{1,2,3,5,6}.out` | 本报告所有数字的出处 |
| 变异验证 | `/tmp/qa-req007-recheck/mutation-test2.log`（变异① 5 条红）、`mutation-test3.log`（变异② 2 条红） | 均在 `crates/platform-iam/src/service.rs` 上原地变异，**两次都逐字节还原**（sha256 == HEAD blob `b60702ee…b7109`），`git diff crates/` 为空；还原后重新编译并重跑枚举探针，结论不变 |
| 回归日志 | `/tmp/qa-req007-recheck/{integration,integration2,integration3,test-plain,test-all,clippy,fmt}.log` | Rust 单测 / 含 ignored 的真实库层 / 集成脚本 / clippy / fmt 的原始输出 |
| 依赖树 | `cargo tree --workspace --prefix none --all-features` | 总数 240 − 工作区成员 8 = **第三方 232** |

**声明**：本报告中的所有数字（逐次状态码与错误码、耗时与比值、锁状态快照、测试计数、crate 数、容器数）**全部由 QA Agent 自己重跑、自己计时、自己读库取样**，
**未采信 Dev 自验报告（`feature-meta/verification-report.md`）里的任何数字**。
与 Dev 说法不一致的一处已在正文标注：回归计数（Dev 写 240，实测 244 / 249，方向安全）。
