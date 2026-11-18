# 验证报告 —— 修复复核（Recheck）

- **需求**: REQ-008 用户与组织架构
- **分支**: `feature/REQ-008` @ `07bdd23`（首轮复核基点 `19cd1a7`；基线 `develop` @ `a0a8d7a`）
- **首轮报告**: `feature-meta/qa-report.md`（结论 **PASS-WITH-RISK**；2 中 / 3 低 / 3 信息）
- **复核范围**: **5 个改动点 + 一次回归**（不重做全量验证）
- **验证者**: QA Agent（独立复现；**未采信 Dev 报告里的任何数字与结论**，全部自己重跑、自己造违规数据、自己复算口径）
- **日期**: 2026-11-18
- **结论**: **PASS-WITH-RISK**

> 5 个改动点 **全部复现为真**：中 1、中 2 的功能目标已达成（同步错误计数真的会递增并在成功后清零；幽灵 / 跨租户 `user_id` 确实被**数据库层**拒绝），低 1/低 2/低 3 的说明与口径逐条对得上。
>
> ★ 唯一新登记的问题是 **低 1 级**：**全量含 ignored 回归不稳定** —— Dev 声称的 `336/336` 在我的 **4 次全量运行中只达成 2 次**，另 2 次因 `deadlock detected` 失败（详见 §仍存在的问题 #1）。该失败**与被测逻辑无关**（是**既有**测试夹具的锁序竞态），**不阻塞合并**。

---

## 逐项复核

| # | 问题 | Dev 声称 | 复核结果 | 证据 |
|---|------|---------|---------|------|
| **中 1** | 同步错误计数不再是死代码 | 新增 `record_sync_failure`：`ON CONFLICT` 时 `error_count = error_count + 1`、写 `last_error`、**`last_sync_at` 不动**；成功路径写 `error_count=0` / `last_error=NULL` / `last_sync_at=now` | ✅ **PASS** | 我用**独立探针**（`/tmp/qa008-probe`，临时工程，**不在仓库内**）驱动**真实** `OrgService::sync_ldap`：失败→`error_count=1`；**连续第二次失败→`error_count=2`（递增，不是覆盖）**；成功→`0` / `last_error=None` / `last_sync_at` 更新；成功后再失败→`1` 且 `last_sync_at` **保持成功时的时间**。§同步错误计数实测 |
| **中 2** | `user_org.user_id` 侧的引用完整性 | 新迁移 `20261118000800_org_user_fk.up.sql`：给 `platform_iam_user` 补 `UNIQUE (tenant_id, id)`；`user_org` / `user_position` / `external_identity` 各加 `(tenant_id, user_id) → platform_iam_user (tenant_id, id)` | ✅ **PASS** | 我用**应用角色 `base_app`**（非超管、`rolbypassrls=f`）**直插** 6 个违规场景 → **全部被数据库拒绝**，报错原文见 §外键拒绝实测；正向对照（同租户合法成员关系）**写入成功**；`pg_constraint` 直查确认 3 条新 FK 均已落库 |
| **低 1** | 依赖归属说明准确 | ldap3 = 8（`ldap3`/`lber`/`nom`/`minimal-lexical`/`futures`/`futures-macro`/`async-trait`/`tokio-util`）；SCIM 出站 = 2（`want`/`try-lock`） | ✅ **PASS** | `git show a0a8d7a:Cargo.lock \| grep -cE 'name = "(want\|try-lock)"'` → **0**（基线确实没有）；当前 lockfile → **2**；`cargo tree --workspace -e features -i want` → `want ← hyper(feature "client") ← hyper-util("client") ← hyper-util("client-legacy") ← platform-org`；`cargo tree -p ldap3` 闭包与 10 个新增名的交集**恰为 8 个** |
| **低 2** | 计数口径写清 | 4 种口径 + 逐字命令：去重名 263→273 / package 条目 272→284 / 构建图去重 `name vX` 239→253 / tree 原始行数 607→682 | ✅ **PASS** | 4 个数字**全部自己复算一致**（基线用 `git archive a0a8d7a \| tar -x -C /tmp/qa008-baseline` 取出，不注册 worktree）；**+14 的逐项构成也复算一致**（10 新名 + `platform-org` + 3 个新版本节点，无删除项）；「去掉工作区成员 = 244 / 231」也复算一致 |
| **低 3** | LDAP scheme 前置校验 | 新增 `ldap.rs::validate_ldap_url` + `Ldap3Source::validate`，**连接前**校验，三种 scheme 都有清晰错误；`LdapError::Config → INVALID_ARGUMENT`(422) | ✅ **PASS** | 三种 scheme 的**真实错误原文**见 §依赖与口径复核后的小节；**不含**底层 `UnknownScheme`；**校验在连接之前**（`ldaps://127.0.0.1:636` 报 scheme 错误，而 `ldap://127.0.0.1:1` 报「Connection refused」→ 证明 `ldap://` 走了真实连接、`ldaps://` 在建连前就被拒）；`sync_ldap` 层实测 `code=INVALID_ARGUMENT status=422` |

---

## 同步错误计数实测

**方法**：独立临时工程 `/tmp/qa008-probe`（path 依赖仓库内的 `crates/platform-org`，**不修改仓库任何文件**），
以**普通角色 `base_app`** 连 `base_test`，调用**真实**的 `OrgService::sync_ldap`；
失败源 = `Ldap3Source::new("ldap://127.0.0.1:1")`（连接被拒），成功源 = `InMemoryLdapSource::default()`。

| 阶段 | error_count | last_error | last_sync_at |
|------|------------|------------|--------------|
| 初始（无行） | — | — | — |
| **失败 #1** | **1** | `Some("[INTERNAL_ERROR] an internal error occurred: ldap connection failed: I/O error: Connection refused (os error 111)")` | `None`（**未变**） |
| **失败 #2（连续）** | **2** ← 递增，非覆盖 | 同上 | `None`（**仍未变**） |
| **成功一次** | **0** ← 清零 | `None` ← 清空 | `Some(2026-11-18 10:15:29.679556 +00:00:00)` ← 更新 |
| **成功后再失败** | **1**（从 0 起算） | `Some("[INTERNAL_ERROR] …")` | `Some(2026-11-18 10:15:29.679556 +00:00:00)` ← **成功时间保持不动** |

- 第二次独立运行（起始行已存在 `error_count=1`）：**1 → 2 → 3 → 成功归 0 → 再失败 1**，递增规律一致。
- `ldaps://` 配置类失败同样留痕：`last_error = Some("[INVALID_ARGUMENT] invalid \`url\`: the ldap url uses ldaps:// but only plaintext ldap:// is supported in this release (ldap over tls is planned for a later requirement)")`。
- **独立复核**（不走服务层，直接 `podman exec … psql` 直查同一行）：
  ```
  tenant_id  |      source       | error_count |                  last_error                  |         last_sync_at
  qa-sync-t1 | qa-probe-ldap-src |           2 | [INVALID_ARGUMENT] invalid `url`: the ldap…  | 2026-11-18 10:15:42.224061+00
  ```
- **代码路径覆盖**：`record_sync_failure` 的**唯一**调用点是 `service.rs:819`（`sync_ldap` 外壳的失败分支）→
  **任何**经 `sync_ldap` 的失败都会被记录；成功路径 `service.rs:867-877` 写 `error_count: 0` / `last_error: None` / `last_sync_at: now`。
- ⚠️ **留痕失败不掩盖根因**：`service.rs:818` 用 `let _ = …` 吞掉留痕失败（库本身挂了时记不下来）——**设计取舍，可接受**。

---

## 外键拒绝实测

**方法**：`podman exec base-postgres psql -U base_app -d base_test`（**应用角色**，`rolsuper=f`、`rolbypassrls=f`），
先 `set app.tenant_id='qa-fk-t1'`，再**直插**（绕过服务层）。
种子：`platform_iam_user('qa-fk-user-a1','qa-fk-t1')`、`platform_iam_user('qa-fk-user-b1','qa-fk-t2')`、`org_node('qa-fk-node-1','qa-fk-t1')`。

| 场景 | 报错原文 |
|------|---------|
| **正向对照**：同租户合法成员关系 | `INSERT 0 1`（**exit 0**）—— FK 没有误伤正常写入 |
| **幽灵 user_id**（用户不存在） | `ERROR:  insert or update on table "user_org" violates foreign key constraint "user_org_user_fk"` / `DETAIL:  Key is not present in table "platform_iam_user".` |
| **跨租户 user_id**（租户 B 的真实用户） | `ERROR:  insert or update on table "user_org" violates foreign key constraint "user_org_user_fk"` / `DETAIL:  Key is not present in table "platform_iam_user".` |
| `user_position` 幽灵 user_id | `ERROR:  insert or update on table "user_position" violates foreign key constraint "user_position_user_fk"` / `DETAIL:  Key is not present in table "platform_iam_user".` |
| `user_position` 跨租户 user_id | `ERROR:  insert or update on table "user_position" violates foreign key constraint "user_position_user_fk"` / `DETAIL:  Key is not present in table "platform_iam_user".` |
| `external_identity` 幽灵 user_id | `ERROR:  insert or update on table "external_identity" violates foreign key constraint "external_identity_user_fk"` / `DETAIL:  Key is not present in table "platform_iam_user".` |
| `external_identity` 跨租户 user_id | `ERROR:  insert or update on table "external_identity" violates foreign key constraint "external_identity_user_fk"` / `DETAIL:  Key is not present in table "platform_iam_user".` |

**补充证据**

- 跨租户那一例**确属"用户真实存在但不属于本租户"**：管理员直查 → `(qa-fk-t2, qa-fk-user-b1)` 存在；同一条语句以 `app.tenant_id='qa-fk-t1'` 的应用角色查 → **0 行**（RLS 隐藏）。
  → 拒绝**不是** RLS 顺带挡的，而是**复合键匹配不上**（结构上杜绝）。
- 落库确认（`pg_constraint` 直查）：
  ```
  user_org_node_fk        FOREIGN KEY (tenant_id, org_node_id) REFERENCES org_node(tenant_id, id)          ← 第一轮就有
  user_org_user_fk        FOREIGN KEY (tenant_id, user_id)      REFERENCES platform_iam_user(tenant_id, id)  ← 本轮新增
  user_position_user_fk   FOREIGN KEY (tenant_id, user_id)      REFERENCES platform_iam_user(tenant_id, id)  ← 本轮新增
  external_identity_user_fk FOREIGN KEY (tenant_id, user_id)    REFERENCES platform_iam_user(tenant_id, id)  ← 本轮新增
  platform_iam_user_tenant_id_uq UNIQUE CONSTRAINT (tenant_id, id)                                              ← 本轮新增（复合 FK 的必要前提）
  ```
- **REQ-007 既有行为未破坏**：`platform_iam_user` 只是**加**了一个唯一约束（不删不改既有列/约束），
  `iam_real_db` 全套（含 refresh 轮换/重放、TOTP、限流、会话）在本轮回归中 **全绿**。
- **迁移可重入 / 重置库后自动生效**：`scripts/test-integration.sh` 重置 `public` schema 后重跑，
  两条 FK 仍在（`user_org_node_fk` + `user_org_user_fk`）→ 迁移挂在 `migrations/` 目录、被 `sqlx::migrate` 自动应用。

---

## 依赖与口径复核

### 1. 归属（低 1）

```bash
$ git show a0a8d7a:Cargo.lock | grep -cE 'name = "(want|try-lock)"'   →  0    # 基线确实没有
$ grep -cE 'name = "(want|try-lock)"' Cargo.lock                       →  2    # 现在都在
$ cargo tree --workspace -e features -i want
want v0.3.2
└── want feature "default"
    └── hyper v1.12.0
        ├── hyper feature "client"
        │   └── hyper-util feature "client"
        │       └── hyper-util feature "client-legacy"
        │           └── platform-org v0.1.0 (crates/platform-org)
```
→ `want` / `try-lock` 由 **SCIM 出站**（`platform-org` 打开 `hyper-util` 的 `client-legacy` → `hyper` 的 http1 **客户端**）引入，**Dev 的归属更正正确**。

`cargo tree -p ldap3 --edges normal,build` 闭包 ∩ 10 个新增 crate 名 = **恰好 8 个**
（`async-trait` / `futures` / `futures-macro` / `lber` / `ldap3` / `minimal-lexical` / `nom` / `tokio-util`），
`want` / `try-lock` **不在** ldap3 闭包内 → **ldap3 = 8、SCIM 出站 = 2** ✅。

### 2. 四个口径（低 2）—— 全部自己复算

| 口径 | 基线 `a0a8d7a` | HEAD `07bdd23` | 增量 | Dev 声称 | 判定 |
|------|---------------|---------------|------|---------|------|
| A 第三方 crate **去重名** | **263** | **273** | +10 | 263→273 | ✅ |
| B 第三方 **package 条目** | **272** | **284** | +12 | 272→284 | ✅ |
| C 构建图去重 `name vX` | **239** | **253** | +14 | 239→253 | ✅ |
| D `cargo tree` **原始行数** | **607** | **682** | +75 行 | 607→682 | ✅ |
| C 去掉工作区成员 | **231** | **244** | +13 | 244/231 | ✅ |

- A 与 B 差 2 的根因（`thiserror` / `thiserror-impl` 的 **v1 线**：crate 名基线已有、版本新增）—— **成立**。
- **+14 的逐项构成也复算一致**（`comm -13` 无删除项）：
  `async-trait v0.1.92` / `futures v0.3.34` / `futures-macro v0.3.34` / `futures-executor v0.3.34` /
  `lber v0.4.2` / `ldap3 v0.11.5` / `minimal-lexical v0.2.1` / `nom v7.1.3` / `tokio-util v0.7.20` /
  `try-lock v0.2.5` / `want v0.3.2` / `thiserror v1.0.69` / `thiserror-impl v1.0.69` / `platform-org v0.1.0`
  → **10 新 crate 名 + 1 新工作区成员 + 3 个新版本节点**，与 Dev §3.1.3 一致。
- 「绝对值差 1」的根因（第一轮没写死命令 → 两次数法不同）—— **成立**：口径 C 的绝对值取决于是否含工作区成员、是否把 `name vX` 正则写死。

### 3. 本轮依赖增量 = **0**

```bash
$ git diff 19cd1a7..HEAD -- Cargo.toml Cargo.lock .cargo/ | wc -c   →  0     # 完全为空
```
`crates/platform-core/`、`crates/platform-contracts/`（CODEOWNERS 保护目录）：`git diff 19cd1a7..HEAD` **0 字节**。

### 4. LDAP scheme 三种错误原文（低 3）

```
ldap://127.0.0.1:389     => Ok(放行)
ldaps://127.0.0.1:636   => Err: ldap configuration is invalid: the ldap url uses ldaps:// but only plaintext
                            ldap:// is supported in this release (ldap over tls is planned for a later requirement)
https://dir.example.com => Err: ldap configuration is invalid: the ldap url must start with ldap:// (unsupported scheme: https)
ldap:/127.0.0.1:389     => Err: ldap configuration is invalid: the ldap url must start with ldap:// (unsupported scheme: ldap:/127.0.0.1:389)
127.0.0.1:389           => Err: ldap configuration is invalid: the ldap url must start with ldap:// (unsupported scheme: 127.0.0.1:389)
```

- **全部不含**底层 `UnknownScheme`（首轮实测的 `ERROR: unknown LDAP URL scheme: ldaps` 已消失）。
- **连接前**的证据：`Ldap3Source::search("ldaps://127.0.0.1:636")` → 报 **scheme 错误**；
  而 `Ldap3Source::search("ldap://127.0.0.1:1")` → `ldap connection failed: I/O error: Connection refused (os error 111)`
  → 说明 `ldap://` 真的去建连了（端口 1 无人监听），`ldaps://` 在建连前就被拦下（代码上 `ldap.rs:468` `connect()` 首行 `self.validate()?`）。
- **422 映射**：把 `ldaps://` 的源喂给 `sync_ldap` → `code=INVALID_ARGUMENT status=422`，
  `display=[INVALID_ARGUMENT] invalid \`url\`: the ldap url uses ldaps:// but only plaintext ldap:// is supported…`
  → 与 SCIM 的 `baseUrl` 校验**对称**（`error.rs:142/162` → `UNPROCESSABLE_ENTITY`）。

---

## 回归

| 项 | 结果 |
|----|------|
| **Rust（非 ignored）** `cargo test --workspace -- --test-threads=2` | ✅ **299 passed / 0 failed / 37 ignored**（首轮 295 → +4） |
| **Rust（含 ignored）** `cargo test --workspace -- --include-ignored --test-threads=2` | ⚠️ **4 次全量运行：2 次 336/336 绿、2 次 335+1 红**（`deadlock detected`，见 §仍存在的问题 #1）→ **Dev 的 336/336 只达成 2/4** |
| 同一套件**跳过 2 个新增 FK 用例** | ✅ **4/4 绿（334 passed / 0 failed）** → 定位到触发点 |
| `scripts/test-integration.sh` | ✅ **exit 0**；该脚本 `--tests` 口径 **331 passed / 0 failed**；**容器数量比对通过：2 → 2** |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ **0 告警**（exit 0） |
| `cargo fmt --all --check` | ✅ exit 0 |
| `cargo xtask capabilities --check` | ✅ exit 0（13 个文件与源码一致） |
| `cargo xtask check-deps` | ✅ exit 0（平台 crate 未反向依赖业务模块） |
| `cargo xtask check-modules` | ✅ exit 0（1 个模块） |
| `cargo xtask tenant-policies --check`（重置库**之后**跑） | ✅ exit 0（2 个模块声明的表都开了 RLS 且有策略） |
| **前端** `pnpm -r test` | ✅ **23/23**（i18n 3 / auth 3 / schema 3 / hooks 3 / sdk 2 / theme 2 / ui 3 / shell 4） |
| **E2E** `bun run test:e2e` | ✅ **1/1**（chromium，「底座壳渲染并可过滤菜单」） |
| **依赖增量** | ✅ **0**（`Cargo.toml` / `Cargo.lock` / `.cargo/` 的 `19cd1a7..HEAD` diff = **0 字节**） |
| **CODEOWNERS 保护目录** | ✅ `crates/platform-core/`、`crates/platform-contracts/` **0 行改动** |

> ⚠️ **命令形式**：全部真实库用例都用**只有一个 `--`** 的写法
> （`cargo test --workspace -- --include-ignored --test-threads=2`）。
> 我**实测确认**写错时（第二个 `--`）libtest 会把 `--test-threads=2` 当过滤串 → 假绿，Dev 的提示属实。
>
> ℹ️ 另有一次 `iam_real_db::the_three_failure_paths_have_similar_latency`（REQ-007 的**耗时比**用例，阈值 ratio < 1.5）
> 在**我并发跑探针时**失败（ratio 2.31）；**独占重跑即通过**。属**负载敏感**用例，与 REQ-008 无关（首轮同样通过）。

---

## 仍存在的问题

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **低** | ★ **全量含 ignored 回归不稳定：Dev 声称的 `336/336` 只在我 4 次运行中达成 2 次**，另 2 次失败于新增用例 `the_database_refuses_a_cross_tenant_user_id`（`deadlock detected`）。**根因不在被测逻辑**，而在**既有**测试夹具的锁序反转：`setup()` 里的 `apply_policies`（`ALTER TABLE public.user_org ENABLE ROW LEVEL SECURITY`，要 `AccessExclusiveLock` on `user_org` 且锁住被引用表 `org_node`）与**并发**的 `INSERT INTO user_org`（FK 检查要 `RowShareLock` on `org_node`）互相等待 → 死锁。锁环里**只有 `user_org` ↔ `org_node`**（第一轮就有的 `user_org_node_fk`），**不含**本轮新增的用户侧 FK → **不是新外键引入的**；但**新增用例是第一个在用例体里早期直插 `user_org` 的**，把这条既有竞态变成了可见的随机失败。 | postgres 容器日志原文：`Process 8803 waits for RowShareLock on relation 24320 … blocked by process 8804` / `Process 8804 waits for AccessExclusiveLock on relation 24337 … blocked by process 8803` / `Process 8803: INSERT INTO public.user_org` / `Process 8804: ALTER TABLE public.user_org ENABLE ROW LEVEL SECURITY`；OID 对照：`24320=org_node`、`24337=user_org`；失败用例 panic：`应指出是哪条外键违约：数据库访问失败：error returned from database: deadlock detected`（`crates/platform-org/tests/org_real_db.rs:1195`）。**隔离实验**：含该用例 4 次全量 → 2 红 2 绿；`--skip the_database_refuses_a_cross_tenant_user_id --skip the_database_refuses_a_ghost_user_id` → **4/4 绿（334/334）** | ① 把 policy 应用移出并发窗口（如 `setup()` 里先统一应用策略再放开用例体，或真实库层用 `--test-threads=1`）；② 退一步：让新增用例在插入前**自持 advisory lock**，与 `setup()` 的 policy 应用互斥。**不影响功能正确性**（FK 拒绝已独立验证），但**「336/336 绿灯」当前不可复现**，合并前建议把这条登记进项目已知问题 |
| **2** | **信息** | `add_member` 传入**不存在的 user_id** 现在被数据库拒绝 → 服务层表现为 **`500 INTERNAL_ERROR` / `DATABASE_ERROR`**，而不是能指名道姓的 404/422（首轮 QA 曾建议「或退一步：在 `add_member` 里校验用户存在并返回 404」）。**Dev 已在 `status.md` #16 如实登记**，本轮只做结构保证。 | 探针实测：`code=INTERNAL_ERROR status=500 display=[INTERNAL_ERROR] an internal error occurred: DATABASE_ERROR`（`DbError::Database` → `OrgError::Internal`，`crates/platform-db/src/error.rs:12`） | 留 REQ-009 一并处理（服务层加用户存在性校验 → 404） |
| **3** | **信息** | 新迁移在**已有悬空成员行**的库上会失败（刻意的：把脏数据暴露出来）。**Dev 已在 `status.md` #18 登记**。`base_test` 实测无悬空行（3 张表各 0 条）→ 本轮无影响；但**部署到存量环境前需先清理**。 | `migrations/20261118000800_org_user_fk.up.sql:21-22`（注释原文）+ `pg_constraint`/`pg_class` 实测 | 部署清单里加一步「迁移前校验悬空行并清理」 |
| **4** | **信息** | `sync_ldap`（LDAP 入站同步）**没有 HTTP 触发路由**：全仓库 `sync_ldap` 的调用点只有 `service.rs` 自身与测试；HTTP 侧只有 `GET /api/org/sync-state`（**只读**，会返回 `errorCount` / `lastError` / `lastSyncAt`）与 `POST /api/org/scim/full-sync`（SCIM 出站入队）。→ 中 1 修好的「失败留痕」要被人看到，**前提是有人在程序里调 `sync_ldap`**（本期靠外部调度/嵌入调用）。**属既有范围**（首轮未列为缺陷），非本轮引入。 | `grep -rn "sync_ldap" crates modules --include=*.rs` → 只有 `service.rs:805/812/827` 与测试；`crates/platform-org/src/http.rs:312`（`/sync-state` 路由） | 后续需求补 LDAP 同步的触发入口或在文档里写明调用方 |
| **5** | **信息** | `sync_conflict` **无 `user_id` 列**（按 `(source, external_id, field)` 定位远端条目）→ 本轮**未**给它加用户侧 FK。核对 Dev 的说明：**成立**（表结构确实没有该列），不构成缺口。 | `migrations/20261118000700_org_schema.up.sql` 的 `sync_conflict` 列清单 | 无需处理，已核实 |

---

## 未覆盖 / 无法验证

| 项 | 状态 | 判定与理由 |
|----|------|-----------|
| **LDAP 真实服务端到端**（`Ldap3Source::connect()` / `search()` 的真连接与检索） | **未验证**（本机无 LDAP/AD 服务器） | 与首轮同项，**不阻塞**。本轮新增的只是**连接前**的 scheme 校验，其行为已由探针实测（`ldap://` 走到真实建连、`ldaps://` 在建连前被拒）。`ldap://` 之后的绑定/检索仍**未跑过真服务器** |
| **`/api/org/*` 真机 HTTP 冒烟** | **未验证** | 与首轮同项；`/api/org/sync-state` 的响应形状由代码读出（`errorCount` / `lastError` / `lastSyncAt`），**未起服务实打** |
| **迁移在「有悬空行」的存量库上的行为** | **未验证**（构造会污染 `base_test`，未做） | 由迁移注释与 Dev 的登记推定；`base_test` 当前无悬空行 |
| **`add_member` 的 404 语义** | **未实现**（Dev 明确留后续需求） | 已实测当前实际行为 = 500（见 §仍存在的问题 #2） |
| **并发压力下的 FK 拒绝稳定性** | **部分验证** | 6 个违规场景各验证 1 次（DB 层）；并发下的表现由全量回归间接覆盖（无 FK 相关失败） |
| 冷编译峰值内存、链接耗时 | **未复测**（本轮未改依赖，增量为 0） | 首轮实测 1471–1763 MB；本轮 `Cargo.lock` **0 字节**变化 → 结论不变 |

---

## 验证者留痕

- **工作树**：本轮结束时 `git status --short` 仅有 `feature-meta/qa-report.md`（**首轮报告，未提交 —— 我进入时就是这个状态，我没有改它**）与本文件 `feature-meta/qa-report-recheck.md`（本轮唯一交付物）；**仓库源码、测试、迁移、配置零改动**。
- **临时物**：探针工程 `/tmp/qa008-probe`（+ `/tmp/qa008-probe-target`）与基线副本 `/tmp/qa008-baseline` 均在 **/tmp**（仓库外），复核结束后已删除；**未在仓库内新增/修改任何文件**。
- **`base_test`**：我造的全部临时数据（租户 `qa-fk-t1` / `qa-fk-t2` / `qa-sync-t1` 的用户、节点、成员、岗位、外部身份、同步状态）**已删除**
  —— 复核后直查 `org_node` / `user_org` / `user_position` / `external_identity` / `sync_source_state` / `platform_iam_user` 中 `tenant_id LIKE 'qa-%'` 的计数**均为 0**；
  `base_app` 的**直接表授权数 = 0**（与初始一致，我没有额外 GRANT）；RLS policy 未改动。
- **容器**：`base-postgres` / `base-redis` 均 healthy，**数量 2 → 2 未变**。
- **未触碰**：`base_dev`（开发库）全程未连接；`feature-meta/` 下的 `status.md` / `verification-report.md` / `traceability.md` 未改动。
- **本文件覆盖说明**：`feature-meta/qa-report-recheck.md` 在本轮之前存的是 **REQ-007 的修复复核报告**（19768 字节，已归档于提交 `5f426ee`）。按本仓库逐需求归档惯例（每轮覆盖同名报告、旧内容留在 git 历史），本文件被本轮 REQ-008 的复核报告覆盖；**REQ-007 原件在 git 历史中完好可取**（`git show 5f426ee:feature-meta/qa-report-recheck.md`）。首轮报告 `feature-meta/qa-report.md` 同理（存的是 REQ-007 的首轮报告，本轮之前已被 REQ-008 首轮报告覆盖并保留在历史里）。
