# REQ-016 终版验收报告（BA 亲跑）

- **需求**: REQ-016 应用接入与应用注册表（父需求 REQ-001）
- **需求来源**: 用户原话**「9. 预留应用接入能力」**（2026-11-17）；与 REQ-022 同源（REQ-022 承 IdP 侧，本需求承应用侧）
- **worktree**: `feature-REQ-016`（分支 `feature/REQ-016`，基线 `develop@aa5b305`）
- **规格**: `acceptance.md` **rev 4**（**11 条 AC**）/ `decisions.md` **D262~D290** / `design-summary.md` **rev 2**
- **执行**: 2026-11-21 20:59 ~ 21:12
- **方法**: **每条判定都指到 `file:line` 的 `assert!` + 函数名**（`dev-guide §9.15 / §9.15.1`）
- **结论**: **11 / 11 通过**；Rust 全量 **1177 passed / 0 failed / 0 ignored**；前端 **426**；E2E **50 passed / 0 skipped**

---

## 0. 三条硬规矩（dev-guide §9.14）

| 规矩 | 遵守 |
|---|---|
| 全量只跑一次 | ✅ 代码冻结后跑（两棒交付 + E2E 只加测试文件） |
| 数字自己算 | ✅ 全部 `grep '^test result' \| awk` 聚合 |
| 全量绿 ≠ 需求通过 | ✅ 判据是 **11 条 AC 逐条判定**（§4） |

---

## 1. 冻结核对

**改动 33 项**（含 E2E）。文件集**与派单一致**，无越界：

| 类别 | 项 |
|---|---|
| 新 crate | `crates/platform-app/`（13 个 Rust 文件） |
| 新迁移 | 6 个（`20261121000100/200/300` 各 up+down） |
| 新测试 | `crates/platform-server/tests/app_real_stack.rs`、`e2e/apps.spec.ts` |
| 新前端 | `Workbench.svelte` / `AppRegistry.svelte` / `app-api.ts` / 3 个测试文件 + fixture |
| 改既有 | `Cargo.{toml,lock}` / `platform-core/src/titles.rs` / `platform-server/{src/main.rs,Cargo.toml,tests/i18n_titles.rs}` / `apps/shell/{App.svelte,lib/nav.ts,lib/routes.ts}` / `packages/i18n/**` / `capabilities/**` / `scripts/seed-e2e.sh` / **`crates/platform-audit/tests/audit_real_db.rs`（D290 修守卫）** |

⚠️ `apps/shell/src/lib/router.ts` **零改动**（`git diff --stat 5aba742..HEAD` → 空）✅

---

## 2. 终版全量（15 个 crate，现场数）

| crate | passed | 备注 |
|---|---|---|
| platform-core | 153 | |
| platform-audit | **54** | 修了 1 个**既有红**（D290） |
| platform-config | 89 | |
| platform-contracts | 32 | |
| platform-contracts-macros | 0 | proc-macro crate |
| **platform-app** | **76** | 46 lib + 27 集成 + 3 C-5 |
| platform-db | 46 | |
| platform-iam | 180 | |
| platform-oauth | 131 | REQ-022 |
| platform-org | 93 | |
| platform-rbac | 83 | |
| platform-schema | 20 | |
| platform-server | **136** | +5（`app_real_stack.rs`） |
| xtask | 51 | |
| sandbox | 33 | |
| **合计** | **1177** | **0 failed / 0 ignored** |

**聚合命令（我自己跑的）**：
```bash
grep -E '^test result' /tmp/req016-full.log | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
# 首轮：passed=1166 failed=1   ← platform-audit 的既有红
# 修后重跑 platform-audit：54 passed
# → 合计 1177 / 0 / 0
```

### 2.1 前端

| 包 | 结果 |
|---|---|
| `@base/shell` | **345 passed / 0 failed**（26 文件；基线 279 → +66） |
| `@base/i18n` | **45 passed** |
| `@base/sdk` | 36 passed / **2 skipped**（既有 `describe.skipIf(!built)`） |
| `svelte-check` | **0 errors / 0 warnings** |
| `i18n tsc --noEmit` | 通过 |
| `vite build` | 通过（`index-Cdh7eTR6.js` 171.67 kB） |

### 2.2 守卫命令

| 命令 | 结果 |
|---|---|
| `cargo test -p platform-app --test contract_c5`（真 C-5） | **3 passed** |
| `cargo xtask capabilities --check` | ✅ **14 个文件与源码一致** |
| `cargo xtask tenant-policies --check` | ✅ **5 个模块**（多了 app） |
| `capabilities/permissions.json` 的 `total` | **35**（32 + 3） |
| `TITLES` 行数 | **99**（93 + 6） |

---

## 3. E2E（P3）

### 3.1 干净起点

```bash
podman exec base-postgres psql -U base -d base_test -tAc "TRUNCATE <7 张表> CASCADE;"
DATABASE_URL_TEST=… ./scripts/seed-e2e.sh      # 6 个 app 夹具 + 账号 + 权限点
```

### 3.2 结果

```
npx playwright test e2e/ --reporter=list
→ 50 passed (1.2m)
```
- ✅ **0 failed** / ✅ **0 skipped**
- ✅ 既有 6 个 spec **44 条全过**（`audit` / `i18n` / `schema-driven` / `shell-auth` / `shell` / `sso`）
- ✅ `e2e/apps.spec.ts` **6 / 6**

### 3.3 ⚠️ 一个**环境坑**（记下来，避免下次重踩）

**首跑 5 failed**，页面显示「网络不可用，请检查连接」。根因不是测试：

```
LISTEN 0.0.0.0:8080  users:(("base",pid=247433,...))
247433 /home/myg133/Projects/system_seed/code/target/debug/base serve   ← code/ = develop 分支
```

→ **8080 上跑的是 `code/`（develop）的服务，它没有 `platform-app` 路由**
→ E2E 打 `/api/apps/*` 全 404 → SDK 报「网络不可用」
→ 而 Playwright 的 `reuseExistingServer: true` **复用了这个错的服务**。

**处置**：杀掉旧服务 → 清库 + 重 seed → 让 Playwright 起 `feature-REQ-016` 的服务 → **6/6 + 50/50 全绿**。

⚠️ 这与 `scripts/e2e-server.sh` 注释里记的 REQ-021 那个坑**同源**（`reuseExistingServer` 掩盖服务没起对）。

---

## 4. 11 条 AC 逐条判定（**每条指到 `file:line` + 函数名**）

| # | AC | 判据（测试里**真的断什么**） | `file:line` + 函数名 | 结论 |
|---|---|---|---|---|
| **024** | 撤销后内省立即失效（回归） | `active=false` 立即；`exp - iat == access_token_expires_in` | REQ-022 已交付（`platform-oauth/tests/oauth_http.rs`）；本轮 `-p platform-oauth` **131 passed** 未破坏 | ✅ |
| **156** | 注册表 CRUD + **引用不复制** | 201 + 10 字段逐字回读；PATCH 只改传入字段；DELETE→404；**列集合恰好 12 且不含 6 个 client 列**；409 双路径 + 唯一组合阳性对照；5 种非法 URL → 400 + `https` → 201（`launchUrl` 与 `iconUrl` 各一组）；4 种畸形体 → 400 | `app_integration.rs:44` `an_app_can_be_created_and_read_back_verbatim` · `:132` `patching_an_app_changes_only_the_given_fields` · `:171` `deleting_an_app_then_reading_it_yields_app_not_found` · `:213` `the_platform_app_table_has_exactly_twelve_columns` · `:277` `a_duplicate_client_or_display_name_is_rejected_with_conflict` · `:347` `an_invalid_launch_url_scheme_is_rejected` · `:403` `an_invalid_icon_url_scheme_is_rejected` · `:460` `a_malformed_create_body_is_rejected_with_invalid_request` · `:500` `the_validation_order_is_pinned` | ✅ |
| **157** | 租户隔离 | `base` 看到 2 租户；tenant-a/b 各自只见自己；**不设上下文 → 0 行**；**跨租户 INSERT 被 RLS 拒（断言 `42501` + `row-level security`，排除 23503/23502）**；本租户 INSERT 成功 | `app_integration.rs:562` `tenant_isolation_hides_other_tenants_rows` · `:646` `the_app_role_sees_nothing_without_a_tenant_context` · `:677` `a_cross_tenant_insert_is_rejected_by_row_level_security` | ✅ |
| **158** | 必须绑可用 client + **级联删除** | client 不存在/disabled/expired → 3 次 400；删 client **成功**、应用行 `count==0`、**别的 client 应用仍在**；**跨租户共享 client → 两租户的行都为 0** | `app_integration.rs:743` `an_app_must_bind_to_an_existing_and_usable_client` · `:800` `deleting_a_client_cascades_to_its_apps` · `:866` `deleting_a_shared_client_removes_apps_in_every_tenant` | ✅ |
| **159** | scope 是**只读投影** | 初始 `["rbac:role:read"]`；改 client 后**单个读**与**列表读**都变两项；**无任何 scope 列** | `app_integration.rs:914` `the_app_scopes_are_a_live_projection_of_the_client` + `:213`（列集合） | ✅ |
| **160** | 工作台分组/排序/过滤 | `groups.length==2`、finance 在前；组内 `["费用申请","差旅报销"]` / `["C 应用","D 应用"]`；hidden E 不在；总数 4；**3 次逐字节相同**；**原始 JSON 逐字断言**（id 与 displayName **刻意反序** → 按 id 兜底必错）；`maxApps=2` 立刻生效且**恰好 2 条** | `app_integration.rs:1114` `the_workspace_groups_and_orders_apps_deterministically` · **`platform-server/tests/app_real_stack.rs:558` `the_workspace_raw_json_order_is_pinned`** · `app_real_stack.rs:638` `max_apps_from_the_database_takes_effect_immediately` · + **E2E `apps.spec.ts:245`** | ✅ |
| **161** | 常用/最近 | 第 3 次 `accessCount==3`、库里 `access_count==3`、**行数仍 3（绑 user_id）**；`recent[0]`/`frequent[0]` 都是最后访问那个；hidden 被过滤 | `app_integration.rs:1240` `recents_and_frequents_track_access_order_and_count` · `:1322` `a_hidden_app_disappears_from_recents_and_frequents` · + **E2E `apps.spec.ts:290`（launch_url 为空仍显示）** | ✅ |
| **162** | 导航两项来源 + 权限过滤 + `kind` | module 项 6 字段 / app 项 7 字段**恰好**；`label` 是中文文案（**不是 `labelKey`**）；`kind` 由来源推出；**audit 项随 `audit:events:read` 出现/消失，app 项两边都在**；空 `launch_url` 不下发 | **E2E `apps.spec.ts:397`**（权限对照）+ 前端 `app-navigation.test.ts`（字段恰好 6/7）+ `app_integration.rs` 的 navigation 用例 | ✅ |
| **163** | 程序级路由 + `external` 整页跳 | `routesFromNavigation`：2 个 internal → 恰好 2 条（`path==target`、`title==label`）；**external → `[]`**；`router.ts` **零改动**；**E2E 点 external → `page.url()` 逐字等于 launch_url，`pushState` 0 次** | 前端 `app-navigation.test.ts:107-121`（函数级）· `:183-221`（真路由器 + pushState 0）· **E2E `apps.spec.ts:333`** · `git diff`（router.ts 空） | ✅ |
| **164** | 审计记录 | 3 个事件码 `app.registered/updated/deleted` 落 `platform_audit_log`；字段集齐；`object_type == "platform_app"`；哈希链 `ok==true` | `platform-app/src/events.rs`（3 码 + 单测）+ 审计经 `AuditSink`（**无裸写**，`no_rust_source_writes_the_audit_table_directly` 通过） | ✅ |
| **165** | 权限点三处齐 + fail-fast + C-5 | 3 个权限点恰好；`capabilities` **14 文件** + `total==35` + modules 7；真 C-5 **3 passed**；启动对账 fail-fast（`platform-rbac` 既有机制，已验证过 REQ-022 同款） | `platform-app/tests/contract_c5.rs`（3 passed）+ `capabilities --check`（14 文件一致）+ `platform-app/manifest.json` | ✅ |

### 4.1 汇总

| 结论 | 条数 |
|---|---|
| ✅ 通过 | **11** |
| ❌ 不通过 | 0 |
| ⚠️ 判不了 | 0 |

---

## 5. 本轮修的一个**既有红**（D290 / TD-053）

`crates/platform-audit/tests/audit_real_db.rs` 的 `no_rust_source_writes_the_audit_table_directly`
在 **`develop@aa5b305`**（**已合并 REQ-022**）上**一直是红的**：

```
crates/**/*.rs 里不得出现裸 INSERT：
  [".../crates/platform-server/tests/oauth_real_stack.rs:761"]
```

**根因**：REQ-014 的 `scan_dir` 无条件扫 `crates/` 全部 `.rs`（含 `tests/`），
而 REQ-022 的**阳性对照必须直插**审计表（为验 AC-S-154 步骤 4 的敏感值扫描）。

**修法**：`scan_dir` 跳过 `tests/` / `benches/` / `examples/` / `target`。

**敏感度验证（BA 亲跑）**：
- 变异：往**生产代码** `platform-audit/src/sink.rs` 塞裸 INSERT → **仍然红**（报 `sink.rs:154`）✅
- 还原：`sha256sum -c` 成功 / `grep -c MUTATION` = 0

⚠️ **这是 BA 自己的流程失误**：REQ-022 合并时只抽验了 3 个 crate（`platform-oauth`/`server`/`xtask`），
**漏了 `platform-audit`**（它依赖 `platform-core`，在影响面内）→ 红灯带着合并进了 `develop`。

---

## 6. 遗留技术债（本轮 +6 条：TD-048~053）

| # | 内容 |
|---|---|
| **TD-048** | 底座内的页面**不能由后端下发**（`RouteDefinition` 无组件槽 + `App.svelte` 10 处硬编码 + `Router` 无 `addRoute`）→ AC-S-163 降级的根因 |
| **TD-049** | `menus[].label` 是**写死的中文文案** → 模块菜单**不随语言切换** |
| **TD-050** | 应用注册**无数量配额** |
| **TD-051** | `PATCH /api/apps/{id}` **无并发控制**（last-write-wins） |
| **TD-052** | `Home.svelte` 占位菜单与工作台真实导航**并存** |
| **TD-053** | REQ-014 的审计守卫**误报**（`scan_dir` 扫了 `tests/`）→ **本轮已修** |

技术债累计 **53 条**（TD-001~053）；**TD-044 / TD-053 本轮已还**。

---

## 7. 待办

- [ ] **独立复核**（verifier 子 Agent，**不是 BA 自查**）
- [ ] BA 终验签字
- [ ] 合并 `develop` + 回收 worktree + 推送 GitHub
- [ ] roadmap / sprint / status 同步 + 复盘
