# REQ-016 应用接入与应用注册表 —— 棒 1（后端 Dev）交付报告

- **需求**: REQ-016（`acceptance.md` rev 4，11 条 AC + AC-S-163.b）
- **范围**: **棒 1 —— 后端**（与棒 2 前端并行，共用 `feature-REQ-016` worktree）
- **基线**: `develop@aa5b305` → worktree `feature-REQ-016 @ 5aba742`
- **决策依据**: `decisions.md` rev 2（D262~D289）
- **环境**: Arch Linux / podman `base-postgres` / 本机无 `psql`
- **实测口径**: 只跑增量（dev-guide §9.14 阶段一）；本阶段**不跑 E2E**（代码未冻结）

---

## 1. 交付清单（文件 → AC）

### 1.1 新 crate `crates/platform-app/`

| 文件 | 内容 | 对应 AC |
|---|---|---|
| `Cargo.toml` | `platform-core` / `platform-db` / **`platform-oauth`（只读 client 表）** + `axum` / `tokio` / `serde` / `serde_json` / `tracing` / `uuid` / `time`。**零新增第三方依赖** | — |
| `manifest.json` | 3 权限点 + 1 菜单（`route=/workbench`，**省略 `perm` 键**）+ `tenantScoped`（2 表）+ `config`（**D277 的 ConfigSpec 对象形状**，3 键全部 `scope: platform` —— D276） | AC-S-162、AC-S-165 |
| `src/error.rs` | **6** 个平台错误码（`APP_*`），D213 的 8 处登记 + 完备性/ASCII/无凭据回显/TITLES 登记单测 | AC-S-156、AC-S-158 |
| `src/events.rs` | **3** 个事件码（**三段式** `app.registry.*`）+ 单测；**不进 `TITLES`**（D223） | AC-S-164 |
| `src/model.rs` | 2 张表的行类型 + DTO（camelCase，C13）+ 两个来源的 `NavigationItem` 字段集单测 | AC-S-156、AC-S-162 |
| `src/store.rs` | 数据访问（`TenantDb` 事务内 `set_config`，D148）+ UPSERT **累加** + join 过滤孤儿行 | AC-S-157、AC-S-160、AC-S-161 |
| `src/service.rs` | 业务编排（CRUD / 工作台 / 常用最近 / 导航 / scope 投影 / URL scheme 校验）+ **钉死的校验顺序** | AC-S-156~162 |
| `src/http.rs` | **11** 个端点 + `router()`；`authenticate` **调 `ensure_fresh`**（TD-047） | 全部 |
| `src/authz.rs` | 3 个 `require_permission("<字面量>")` 薄封装（**在本 crate 内**，C-5 才扫得到） | AC-S-165 |
| `src/config.rs` | `AppConfigSource` trait + `AppSettings`（**每请求读**）；默认值钉死 | AC-S-160、AC-S-161 |
| `src/tokens.rs` | `TokenVerifier` 接缝（**不依赖 `platform-iam`**，装配层注入） | — |
| `src/lib.rs` | 模块声明 + 依赖方向 / 命名禁区文档 | — |
| `tests/contract_c5.rs` | **真 C-5 命令**（照 `platform-oauth/tests/contract_c5.rs`）+ manifest 合法性 + **配置 scope 判据** | AC-S-165 |
| `tests/common/mod.rs` | 真库 + 真 HTTP 夹具（`base_app` 业务连接 / `base` 造数） | — |
| `tests/app_integration.rs` | **27** 条真库 + 真 HTTP 集成用例 | 156~162、164 |

### 1.2 3 个迁移（编号接 REQ-022 的 `20261120000700`）

| 文件 | 内容 |
|---|---|
| `migrations/20261121000100_app_schema.{up,down}.sql` | **2 张表** + 唯一索引 + 工作台/最近/常用索引 + FK **`ON DELETE CASCADE`**（D264）。**RLS 由 xtask 生成，不手写** |
| `migrations/20261121000200_app_permission_seed.{up,down}.sql` | 3 个权限点，`source` 逐字 = `'crates/platform-app/manifest.json'` |
| `migrations/20261121000300_app_config_seed.{up,down}.sql` | 3 个配置键（lowerCamelCase），用 `platform_config_upsert` |

### 1.3 改既有文件

| 文件 | 改动 | 对应 AC |
|---|---|---|
| `crates/platform-core/src/titles.rs` | **+6** 条 ASCII 标题（`APP_*`），**93 → 99** | AC-S-156/158 |
| `crates/platform-server/tests/i18n_titles.rs` | `all_codes()` 加 `platform_app` 一行 + `const REQ_016_NEW_CODES: usize = 6`（断言 **99**） | — |
| `crates/platform-server/src/main.rs` | 装配：`build_app_state` / `load_module_manifests` / `AppIamTokenVerifier` / `PlatformAppConfigSource`；IAM + RBAC 就绪后注入 `PermissionChecker` 与 `PermissionRefresher` | AC-S-162、AC-S-165 |
| `crates/platform-server/tests/app_real_stack.rs` | **新建**：**5** 条真装配用例（真 `ConfigService` + 真 `PgAuditSink`） | AC-S-160/161/162/164 |
| `crates/platform-server/Cargo.toml` | + `platform-app` 依赖 | — |
| `Cargo.toml` / `Cargo.lock` | workspace 成员 + `platform-app` | — |
| `packages/i18n/src/locales/{zh-CN,en-US}.json` | **只加 6 个 `error.APP_*`**（棒 2 的 25 条 `app.*` 界面文案未动） | AC-S-156/158 |
| `packages/i18n/test/i18n.test.ts` | 只**追加** 6 个码到 `codes` 数组；**保留**棒 2 加的 `'app'` domain（实测 `grep -c "'app'"` = 1） | — |
| `scripts/seed-e2e.sh` | 加 `u-perm-app` 用户 / 角色 / 3 权限点 / 绑定 + 6 个 `client-app-*` + 6 个应用（tenant-a 5 个、tenant-b 1 个）+ 强断言数字更新（用户 7、授权 `4 5 0 1 3 5 1 3`） | AC-S-156~160 |
| `capabilities/**` | `cargo xtask capabilities` 重新生成（**14 文件不变**，`total` 32 → **35**） | AC-S-165 |

### 1.4 边界（**未越界**）

- ⚠️ **`apps/shell/**` 一个文件都没碰**（`git status` 里那些是棒 2 的改动）。
- ✅ `git diff --stat aa5b305 -- apps/shell/src/lib/router.ts` → **空**（D270 / AC-S-163 步骤 4 的回归判据仍成立）。
- ✅ `crates/xtask/src/catalog.rs` **无需改动**（它是静态工具目录文案；`cargo xtask capabilities --check` 已绿，无过期项）。

---

## 2. 逐 crate 测试数字（**我自己跑的**）

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
```

| 命令 | passed | failed | ignored |
|---|---|---|---|
| `cargo test -p platform-app --all-targets -- --test-threads=1 --include-ignored` | **76** | 0 | 0 |
| ├─ `--lib`（单元） | 46 | 0 | 0 |
| ├─ `--test app_integration`（真库 + 真 HTTP） | 27 | 0 | 0 |
| └─ `--test contract_c5`（真 C-5） | 3 | 0 | 0 |
| `cargo test -p platform-core -p platform-oauth --all-targets -- --test-threads=1 --include-ignored` | **284** | 0 | 0 |
| `cargo test -p platform-server --all-targets -- --test-threads=1 --include-ignored` | **136** | 0 | 0 |
| ├─ `i18n_titles`（断言 TITLES = **99**） | 9 | 0 | 0 |
| └─ `app_real_stack`（真装配 5 条） | 5 | 0 | 0 |

**合计本轮的增量回归：76 + 284 + 136 = 496 passed / 0 failed / 0 ignored。**

其他校验：

| 命令 | 结果 |
|---|---|
| `cargo xtask capabilities --check` | ✅「14 个文件与源码一致」 |
| `cargo xtask tenant-policies --check` | ✅ 退出码 0（`app：platform_app, platform_app_access` 各 1 条 policy） |
| `cargo xtask check-deps` | ✅ 平台 crate 未反向依赖业务模块 |
| `cargo xtask check-modules` | ✅ |
| `cargo clippy -p platform-app -p platform-server --all-targets` | ✅ `platform-app` **0 警告** |
| `pnpm --filter @base/i18n test` | ✅ **45 passed**（含棒 2 的 `'app'` domain 用例） |
| `bash -n scripts/seed-e2e.sh` + 实跑 | ✅ 强断言全绿（用户 7 / 授权 `4 5 0 1 3 5 1 3`） |

**能力索引实测数字**：

```
find capabilities -type f | wc -l        → 14
permissions.json total                   → 35   (32 + 3)
modules 里 module=="app"                  → crates/platform-app/manifest.json
  permissions                            → app:registry:read / write / delete（逐字 3 条）
platform_app 列数（information_schema）    → 12
```

**RLS 实测**：

```
pg_policies → platform_app|platform_app_tenant_isolation
              platform_app_access|platform_app_access_tenant_isolation
```

---

## 3. 变异验证证据（**8 条，全部还原干净**）

**规矩**（dev-guide §9.4）：每次还原后 `touch` 该文件，并 `sha256sum -c` 复核 + `grep -c MUTATION` = 0。

**基线指纹**（变异前记录在 `/tmp/req016-mut/baseline.sha256`）：

```
7a92decd...  crates/platform-app/src/service.rs
4a2f1790...  crates/platform-app/src/store.rs
1b0e306f...  crates/platform-app/src/http.rs
8e3f971e...  crates/platform-app/manifest.json
ef88696f...  migrations/20261121000100_app_schema.up.sql
d5c1f85c...  crates/platform-app/src/config.rs
```

**还原后复核**：6 个文件**全部「成功」**；`grep -rc MUTATION crates/platform-app/src/ migrations/… manifest.json` → **0 处残留**。

| # | 变异点（改**生产那一行**） | 变红的测试 | 观察到的数字 | 还原 |
|---|---|---|---|---|
| **M1** | `crates/platform-app/manifest.json`：删掉 3 个配置键的 `"scope": "platform"` | `contract_c5::every_app_config_key_is_platform_scoped` | `panicked: 配置键 app.recentCount 的 scope 必须是 Platform（D276…）` | ✅ `sha256sum` 成功；3 passed |
| **M2** | 迁移的 FK：`ON DELETE CASCADE` → `ON DELETE RESTRICT`（**改 DB 实测**，见下） | `app_integration::deleting_a_client_cascades_to_its_apps` | `SQLSTATE **23503**`：`violates foreign key constraint platform_app_client_id_fkey … still referenced` | ✅ 重建 CASCADE（`confdeltype=c`）；27 passed |
| **M3** | `service.rs::scopes_for`：改成「应用表自己存一份」（返回固定 `["rbac:role:read"]`，不读 client） | `app_integration::the_app_scopes_are_a_live_projection_of_the_client` | `left: ["rbac:role:read"]` / `right: ["rbac:role:read","rbac:role:write"]` | ✅ `sha256sum` 成功；1 passed |
| **M4** | `service.rs::workspace`：删掉 `.then(left.display_name.cmp(&right.display_name))` | `app_integration::the_workspace_groups_and_orders_apps_deterministically` | `left: ["D 应用","C 应用"]` / `right: ["C 应用","D 应用"]` | ✅ 还原；1 passed |
| **M5** | `store.rs::record_access`：`access_count = {table}.access_count + 1` → `EXCLUDED.access_count` | `app_integration::recents_and_frequents_track_access_order_and_count` | `{"accessCount":1,…}`（第 3 次仍是 1）/ 期望 **3** | ✅ `sha256sum` 成功；1 passed |
| **M6** | `service.rs::menu_is_visible`：去掉权限点过滤（恒 `true`） | `app_integration::navigation_merges_modules_and_apps_and_filters_by_permission` | 「不持 audit 权限时 audit 项必须消失」失败 —— 4 个 module 项全下发 | ✅ `sha256sum` 成功；1 passed |
| **M7** | `http.rs::authenticate`：删掉整个 `ensure_fresh` 调用块 | `app_integration::a_failing_permission_refresh_fails_the_protected_endpoint_closed` | `left: 200` / `right: 500`（fail closed 消失） | ✅ `sha256sum` 成功；1 passed |
| **M8** | `service.rs::validate_optional_url`：scheme 判据改成 `lowered.contains(':')`（放行 `javascript:`） | `app_integration::an_invalid_launch_url_scheme_is_rejected` | `javascript:alert(1)` → **201**（期望 400） | ✅ `sha256sum` 成功；1 passed |

### 3.1 M2 的做法说明（**故意的偏离**）

`ON DELETE CASCADE` 是**迁移里的 DDL**，而集成夹具会校验迁移文件 hash
（`VersionMismatch(20261121000100)`）→ 改迁移文件会让**准备阶段**就红，
而不是让**目标断言**红（那是假红，不是真敏感度）。

→ **做法**：迁移文件**保持原样**（校验和不变），在 `base_test` 里**手工把表重建成
`ON DELETE RESTRICT`** → 跑目标用例 → 断言在**删除 client** 那一步红
（`SQLSTATE 23503`，正是「被外键挡住」）→ 重建回 `CASCADE`（`confdeltype=c` 实测）+ 重跑全绿。

### 3.2 M4 发现的**真实覆盖缺口**（已修）

M4 第一次跑是**绿的**——说明原夹具**无区分力**：
C/D 的 `displayName` 顺序（C→D）恰好与它们的 `appId` 顺序（app-c→app-d）相同
→ 「按 id 兜底」的实现也能过。

→ **修法**：把夹具改成 **id 与 displayName 顺序刻意相反**
（`app-z`→「C 应用」、`app-a2`→「D 应用」）→ M4 重做后按 displayName 的判据才真的变红。
`app_real_stack.rs` 的工作台用例做了同样修正（对应 BA 的 D286 追加要求）。

### 3.3 静态扫描的**阳性对照**

命名禁区（D216）扫描**不是**只报「0 命中」—— 先证明扫描器会命中：

```
$ grep -rnE "check_permission|...|authorize|..." crates/platform-app/src/
→ 命中 8 处，**全部在文档注释里**（lib.rs:41-46、authz.rs:25-30）
→ 代码里 0 处（`AppService::authenticate` / `authz::require_registry_*`）
→ C-5 的 `custom_entries.is_empty()` 断言（contract_c5.rs）额外守住这一点
```

`contract_c5` 的三条断言分别是：C-5 无违例 + 3 个调用点**逐字**存在且**恰好 3 个** +
**无**非字面量调用 + **无**自建鉴权入口。

---

## 4. AC → 用例 → 测试函数对照表（**11/11**）

> 规则（§9.15.1）：每个 `file:line` **必须同时给函数名**；一条 AC 靠多个函数覆盖时逐个列出。

| AC | 判据（测试里**真的断什么**） | `file:line` + 函数名 |
|---|---|---|
| **AC-S-024**（回归） | 撤销后内省 `active=false`、验签仍有效、`exp-iat == access_token_expires_in` | **REQ-022 已交付**，本轮不重复定义（见 §5「与规格不一致」#7）。回归由 `crates/platform-oauth/tests/oauth_http.rs` 的既有用例覆盖 —— 本轮 `-p platform-oauth` **284 passed**，未破坏 |
| **AC-S-156** 步骤 1-4（CRUD 主路径 + 回读逐字） | 201；`appId`/`clientId`/`displayName`/`category`/`iconUrl`/`launchUrl`/`sortOrder`/`status`/`createdBy`/`scopes` 逐字；**列表是数组**；列表项 == 创建响应 == 单个响应 | `crates/platform-app/tests/app_integration.rs:44` `an_app_can_be_created_and_read_back_verbatim` |
| **AC-S-156** 步骤 5（PATCH 只改传入字段） | 200；`displayName`/`sortOrder` 变了；`launchUrl`/`category`/`clientId` **不变** | `app_integration.rs:132` `patching_an_app_changes_only_the_given_fields` |
| **AC-S-156** 步骤 6（DELETE → 404 + 二次 DELETE 不 500） | 204 → 404 `APP_NOT_FOUND` → 二次 DELETE 仍 404 `APP_NOT_FOUND` | `app_integration.rs:171` `deleting_an_app_then_reading_it_yields_app_not_found` |
| **AC-S-156** 步骤 7（列集合恰好 12 + 不含 6 个 client 列）+ **AC-S-159** 步骤 5（无 scope 列） | `information_schema.columns` **双向相等** 12 列；6 个 client 列**不在**；`column.contains("scope")` **0 个** | `app_integration.rs:213` `the_platform_app_table_has_exactly_twelve_columns` |
| **AC-S-156** 路径 2（409 ×2） | 同 clientId → 409 `field=clientId`；同 displayName → 409 `field=displayName`；**唯一组合 → 201**（阳性对照） | `app_integration.rs:277` `a_duplicate_client_or_display_name_is_rejected_with_conflict` |
| **AC-S-156** 路径 3（5 种非法 + 阳性对照） | 5 次全 400 `APP_LAUNCH_URL_INVALID`；**`https://` → 201** | `app_integration.rs:347` `an_invalid_launch_url_scheme_is_rejected` |
| **AC-S-156** 路径 4（同路径 3，`iconUrl`） | 5 次全 400 `APP_ICON_URL_INVALID`；**`https://` → 201** | `app_integration.rs:403` `an_invalid_icon_url_scheme_is_rejected` |
| **AC-S-156** 路径 5（缺必填/类型错） | 4 次全 400 `APP_INVALID_REQUEST` + 必有 `title`（Problem Details，非纯文本） | `app_integration.rs:460` `a_malformed_create_body_is_rejected_with_invalid_request` |
| **AC-S-156** 交叉（**校验顺序**） | 已占用 client + `javascript:` → `APP_LAUNCH_URL_INVALID`；不存在的 client + `javascript:` → `APP_CLIENT_INVALID` | `app_integration.rs:500` `the_validation_order_is_pinned` |
| **AC-S-157** 步骤 1/2/3/4（读侧 RLS） | `base` 看到 2 个租户；tenant-a 只见 `tenant-a`；tenant-b 只见 `tenant-b`；**不设上下文 → 0 行** | `app_integration.rs:562` `tenant_isolation_hides_other_tenants_rows`；`:646` `the_app_role_sees_nothing_without_a_tenant_context` |
| **AC-S-157** 步骤 5（写侧 `WITH CHECK`） | 跨租户 INSERT 被拒，理由**必须**含 `row-level security`/`42501`（**排除** 23503/23502）；本租户 INSERT 成功 | `app_integration.rs:677` `a_cross_tenant_insert_is_rejected_by_row_level_security` |
| **AC-S-157** 步骤 6（policy 数量） | `cargo xtask tenant-policies --check` 退出 **0**；`pg_policies` 里 2 张表**各 1 条** `*_tenant_isolation` | `scripts` 命令 + §2 的实测输出（本报告 §2） |
| **AC-S-158** 路径 1/2（client 不存在 / disabled / expired） | 3 次都是 400 `APP_CLIENT_INVALID` | `app_integration.rs:743` `an_app_must_bind_to_an_existing_and_usable_client` |
| **AC-S-158** 路径 3/4/5（级联 + 不误伤） | 删除 client **成功**（1 行）；应用行 `count == 0`；**别的** client 的应用**仍在** | `app_integration.rs:800` `deleting_a_client_cascades_to_its_apps` |
| **AC-S-158** + D264（跨租户级联） | 两租户引用同一 client → 删除后**两租户的行都为 0** | `app_integration.rs:866` `deleting_a_shared_client_removes_apps_in_every_tenant` |
| **AC-S-159** 步骤 1/3/4 + TC-016-029 | 初始 `scopes == ["rbac:role:read"]`；改 client 后**单个**读变两项；**列表**读也变两项 | `app_integration.rs:914` `the_app_scopes_are_a_live_projection_of_the_client` |
| **AC-S-159** 步骤 5（无 scope 列） | 同 AC-S-156 步骤 7 的那个函数（`contains("scope")` 0 个） | `app_integration.rs:213` |
| **AC-S-160** 步骤 1/2/3 | 200；`groups.length == 2`；`groups[0].category == "finance"`、`[1] == "general"`；finance 组 `["费用申请","差旅报销"]`；general 组 `["C 应用","D 应用"]`；hidden 的 E 不在；总数 4；**3 次逐字节相同** | `app_integration.rs:1114` `the_workspace_groups_and_orders_apps_deterministically` |
| **AC-S-160** 步骤 3（**原始 JSON**，D286 追加） | **不经前端函数**，直接对 `GET /api/apps/workspace` 的**原始响应体**逐字断言数组顺序（finance B→A、general C→D、hidden 被过滤） | `crates/platform-server/tests/app_real_stack.rs:558` `the_workspace_raw_json_order_is_pinned` |
| **AC-S-160** 步骤 4/5（配置真库生效） | 默认 `60`；`platform_config_upsert('app.workspace.maxApps','2')` → **立刻**（不 sleep）**恰好 2 条**且 == `[app-b, app-a]` | `app_real_stack.rs:638` `max_apps_from_the_database_takes_effect_immediately`；内存源那条：`app_integration.rs:1175` `the_workspace_respects_the_max_apps_configuration_immediately` |
| **AC-S-161** 步骤 1~4（UPSERT 累加） | 第 3 次 `accessCount == 3`；落库 `access_count == 3`；**行数仍 3**（绑定 `user_id`） | `app_integration.rs:1240` `recents_and_frequents_track_access_order_and_count` |
| **AC-S-161** 步骤 2/3（recent/frequent 首条） | `recent[0].appId == app-a`（最后访问）；`frequent[0].appId == app-a`（count 最大） | 同上 `app_integration.rs:1240` |
| **AC-S-161** 步骤 5（hidden 被过滤） | `recent` 里**不含** B；A 仍在 | `app_integration.rs:1322` `a_hidden_app_disappears_from_recents_and_frequents` |
| **AC-S-161** 步骤 6（`app.recentCount` 真库生效） | 默认 2 条 → `upsert('app.recentCount','1')` → **立刻** 1 条且是最后访问的 B | `app_real_stack.rs:684` `recent_count_from_the_database_takes_effect_immediately` |
| **AC-S-161** + D268（孤儿行不可见） | 孤儿访问记录（应用行不存在）**不出现**；只返回在册那个、长度 1 | `app_integration.rs:1364` `orphan_access_rows_are_not_returned` |
| **AC-S-162** 步骤 1/2/4/5 | 持 audit → module 的 `/admin/audit` + app 项都在；不持 → audit 消失但 app 项仍在；**module 项恰好 6 键**、**app 项恰好 7 键**；`kind`/`target` 逐字；`requiredPermission` 在 app 项**是 `null`**；module 在 app **之前** | `app_integration.rs:1420` `navigation_merges_modules_and_apps_and_filters_by_permission` |
| **AC-S-162** 步骤 3（`perm` 缺省总是下发） | 无任何权限点的账号 → `/workbench` 项**仍出现**；带 perm 的两条都**不**出现 | `app_integration.rs:1557` `a_menu_without_a_permission_is_always_visible` |
| **AC-S-162** 步骤 6（清空入口 → 不下发） | `DELETE …/launch-url` → 204；库里 `launch_url IS NULL`；导航里**没有**它；**工作台仍有**（D279） | `app_integration.rs:1593` `clearing_the_launch_url_removes_the_app_from_navigation` |
| **AC-S-162**（真装配 + 真 manifest） | 真 RBAC 权限：持 `audit:events:read` → audit 项在；只读用户 → 消失但 app 项在；`/workbench` 总在下发 | `app_real_stack.rs:830` `navigation_filters_module_items_by_real_permissions` |
| **AC-S-163** | **归棒 2**（前端）：`routesFromNavigation` 纯函数 + `external` 整页跳转 + `router.ts` 零改动。后端只保证 `kind` = `internal`/`external` 逐字（见 AC-S-162 的两个函数） | 棒 2 的 `apps/shell/test/app-navigation.test.ts` + `git diff --stat aa5b305 -- apps/shell/src/lib/router.ts`（**空**，本报告 §1.4 实测） |
| **AC-S-164**（埋点契约） | 注册/修改/删除各产生**恰好 1 条**对应事件码；`object_type == "platform_app"`；`object_id == appId`；`actor_id`/`tenant_id` 对 | `app_integration.rs:1687` `registry_mutations_emit_one_audit_event_each`（**假 sink** —— 只验埋点） |
| **AC-S-164**（**真落库** + 哈希链） | 从 `platform_audit_log` **读回**三条，`action` 恰好 `app.registry.registered/updated/deleted`；`ChainVerifier::verify` → `ok == true`、`broken.is_empty()`、`checked >= 3` | `app_real_stack.rs:741` `app_mutations_land_in_the_audit_log_with_an_intact_chain` |
| **AC-S-165** 步骤 1a（capabilities） | 14 个文件；`total == 35`；`module == "app"` 且 `manifest == "crates/platform-app/manifest.json"`；3 条 `app:*` 逐字 | `crates/xtask capabilities --check`（§2 实测）+ 迁移种子（§1.2） |
| **AC-S-165** 步骤 1b（**真 C-5**） | C-5 无违例；3 个调用点**逐字**且**恰好 3 个**；无非字面量；无自建鉴权入口 | `crates/platform-app/tests/contract_c5.rs:46` `every_app_permission_has_an_authorization_call_site` |
| **AC-S-165** 步骤 2（只改 manifest → 起不来） | 由 `platform_rbac::registry::require_consistent` + `main.rs::reconcile_permissions` 已交付的行为覆盖（TC-016-059 的证据行：`crates/platform-server/src/main.rs:1127-1155`）；本轮 manifest + 种子**都写了**，`cargo xtask capabilities --check` 与 `-p platform-server` 全绿即证明两侧一致 | 见 §2；**变异 M1** 证明该判据会红 |
| **AC-S-165** 步骤 3/4（C-5 阳性对照） | 见 §3.3：命名禁区的扫描**有命中**（文档注释 8 处），代码 0 处；`custom_entries` / `non_literal_calls` 断言守住 | `contract_c5.rs:46` + §3.3 |
| **AC-S-165**（D276 配置 scope） | 3 个键的 `ConfigSpec.scope == Platform` + `label` 非空 + 默认值 6/60/false | `contract_c5.rs:135` `every_app_config_key_is_platform_scoped`（**变异 M1 打红**） |
| **AC-S-165**（manifest 合法性 + tenantScoped） | `parse_checked` 通过；3 权限点；`tenantScoped` 恰好 2 张表 | `contract_c5.rs:90` `the_app_manifest_is_valid` |
| **TD-047**（`ensure_fresh`） | 刷新失败 → 受保护端点 **500**（fail closed）+ 断言刷新器**真的被调用** | `app_integration.rs:1035` `a_failing_permission_refresh_fails_the_protected_endpoint_closed`（**变异 M7 打红**） |

**覆盖结论**：**11/11**（AC-S-163 归棒 2，后端侧提供 `kind` 判据；AC-S-024 是 REQ-022 已交付的回归，本轮 `-p platform-oauth` 284 passed 未破坏）。

---

## 5. 与规格不一致的发现（**需 BA 拍板**）

### 🔴 #1 `platform_config` **没有 `scope` 列** → AC-S-160 步骤 0 的字面 SQL **物理不可执行**

- **AC 原文**（`acceptance.md:229`）：
  ```sql
  SELECT count(*) FROM platform_config WHERE key LIKE 'app.%' AND scope = 'platform'  -- == 3
  ```
- **实测**：
  ```
  $ SELECT column_name FROM information_schema.columns WHERE table_name='platform_config'
  → key, value, updated_at      （**没有 scope**）
  $ SELECT count(*) ... AND scope = 'platform'
  → ERROR: column "scope" does not exist
  ```
- **根因**：scope 是 **manifest 的 `ConfigSpec.scope` 元数据**，`platform_config` 表只存 `key/value`。
  `ConfigService::read_platform_entries` 在**运行时**按 manifest 的 spec 过滤（`service.rs:280`）。
- **我的处置**：实现了**可执行的等价判据** —— `contract_c5.rs:135` `every_app_config_key_is_platform_scoped`
  直接读 manifest 断言 3 个 `ConfigSpec.scope == ConfigScope::Platform`（**变异 M1 能打红**）。
  「配置真的能通过平台端点读写」由 `app_real_stack.rs` 的两条真 `ConfigService` 用例覆盖（`maxApps`/`recentCount` 真库变更**立刻**生效）。
- **需要 BA 拍板**：把 AC-S-160 步骤 0 的字面 SQL 改成上面的 manifest 判据措辞（或补一条 `scope` 列 —— 但那是 REQ-010 的表结构变更，**跨需求**）。

### 🔴 #2 事件码：AC 正文**自相矛盾**（写「三段式」却列两段式）+ `object_type` 与已交付口径冲突

- **AC 原文**（`acceptance.md:384-388`）：
  - 「D215 同款命名规范：**三段式** `app.*`」，紧接着列 `app.registered` / `app.updated` / `app.deleted`（**两段**）；
  - 「`object_type` **恰好**是 `app`」。
- **已交付约定**（`crates/platform-oauth/src/events.rs`）：`oauth.client.registered`（三段），`object_type::CLIENT == "oauth_client"`；`crates/platform-audit` 的既有记录用**表名**。
- **用例集要求**（`test-cases/05-audit-permission.md` TC-016-053/054）：`app.registry.registered` + `object_type == "platform_app"`。
- **我的处置**：跟**用例集 + 已交付约定**走 —— `app.registry.registered/updated/deleted` + `object_type = "platform_app"`。
- **需要 BA 拍板**：把 `acceptance.md:73` 与 `:384-388` 改成三段式 + `platform_app`（两处都要改，否则 BA 核 AC 时会以为我实现错了）。
- **⚠️ 风险**：若 BA 坚持两段式 + `"app"`，只需改 `events.rs` 的 3 个常量与 `object_type::APP` 的 1 个值；集成用例与 `app_real_stack.rs` 里的字面量要同步（4 处）。

### 🟡 #3 AC-S-160 步骤 3 的「C→D」在**原播种数据下无区分力**

- **原播种**：C 的 `appId` 与 `displayName` 顺序恰好一致 → 「按 id 兜底」的实现也能过（**变异 M4 第一次没打红**）。
- **我的处置**：集成用例与 `app_real_stack.rs` 都把 id 与 displayName **刻意弄反**（`app-z`→C、`app-a2`/`app-d0`→D）。
  `scripts/seed-e2e.sh` 同理（`e2e-app-z`→「E2E C 应用」、`e2e-app-d0`→「E2E D 应用」）。
- **需要 BA 确认**：E2E 侧若按 displayName 文案断言，**须用带 `E2E ` 前缀的文案**（seed 的 displayName 是「E2E 差旅报销」等），
  否则与集成用例的夹具文案（「差旅报销」）混在一起会撞 `UNIQUE (tenant_id, display_name)`。

### 🟡 #4 seed 的每个应用**必须各用一个 client**

- **实测**：`client-A` 不能同时被 `e2e-app-a` 与 AC-S-156/159/162 的用例应用使用 →
  `UNIQUE (tenant_id, client_id)` 直接拒（`platform_app_tenant_client_idx`）。
- **我的处置**：seed 建 6 个 `client-app-*`（a/b/c/d/e/other），每个应用各配一个。
- **值得记的**：这不是缺陷，是 D265 的唯一索引在起作用；但**播种清单原来没写**这一条。
  建议在 `test-cases/06-夹具.md`（若存在）或 design §9 的夹具段补一句「每个应用一个 client」。

### 🟢 #5 `POST /api/apps/{id}/access` 不要求 `launch_url` 非空（D289）—— 已按此实现

- `record_access` 只校验「应用存在 + 当前租户 + `status == active`」，**不看 `launch_url`**。
- 与 D279 / D289 一致；`app_integration.rs:1240` 的用例里 A/B/C **都没有** `launch_url` 仍能记访问（实测通过）。

### 🟢 #6 工作台**不按 `launch_url` 过滤**（D279）—— 已按此实现并有反向判据

- `clearing_the_launch_url_removes_the_app_from_navigation`（`app_integration.rs:1593`）同时断言：
  清空入口后**导航里没有**、**工作台里仍有** → 两条规则不会混。

### 🟢 #7 AC-S-024 在本轮**没有新写用例**

- AC 正文（`acceptance.md:100-103`）自己写明「REQ-022 的 AC-S-143/144/147 已验过同一件事，本条是**跨需求回归判据**」。
- 我跑了 `cargo test -p platform-oauth --all-targets --include-ignored` → **284 passed / 0 failed**，
  即「应用注册表存在之后撤销路径未被破坏」。
- 若 BA 要求在 `platform-app` 侧**再写一遍**（真发令牌 + 真撤销），需要 IAM 的登录流程（Argon2，低配机上最慢的一步）——
  可以做，但属重复覆盖。**请 BA 决定**是否需要。

### 🟢 #8 `AppView` 的字段集与棒 2 逐字一致（已核对）

棒 2 要求的 12 字段（`appId/clientId/displayName/description/category/iconUrl/launchUrl/sortOrder/status/scopes/createdAt/createdBy`）
与 `model.rs::AppView` **逐字相同**（`model.rs` 的单测 `the_app_view_uses_camel_case_keys` 断言这 12 个键存在且 snake_case 不存在）。
`NavigationItem` 的 6/7 键也由单测 `a_module_navigation_item_has_exactly_six_keys` /
`an_app_navigation_item_has_exactly_seven_keys_with_a_null_permission` 钉死。

---

## 6. 未完成项 + 原因

| 项 | 状态 | 原因 |
|---|---|---|
| **E2E**（`e2e/apps.spec.ts`） | ⬜ 未写 / 未跑 | dev-guide §9.14 阶段一明确「**增量阶段不跑 E2E**」；且 E2E 由 BA 终版统一跑。棒 2 的前端已交付，E2E 属于终版范围 |
| **AC-S-163**（前端运行时路由） | ✅ **归棒 2** | 棒 2 已交付（344 测试通过）；后端只提供 `kind="internal"/"external"` 判据 |
| **AC-S-024** 在 `platform-app` 侧重写 | 🟡 未写（**等 BA 决定**） | 见 §5 #7：REQ-022 已覆盖，重写需要完整登录流程 |
| **全量 14 crate 回归** | ⬜ 未跑 | 任务书明确「不要跑 14 个 crate 的全量（终版 BA 会跑一次）」；本阶段只跑增量（76 + 284 + 136 = 496 passed） |
| `crates/xtask/src/catalog.rs` | ✅ 无需改 | `cargo xtask capabilities --check` 已绿，无过期文案 |

---

## 7. 对棒 2 文件集之外的改动

**应为空 —— 实际为空。**

`git status --short` 里属于棒 2 的文件（**全部未被我触碰**）：

```
 M apps/shell/src/App.svelte
 M apps/shell/src/lib/nav.ts
 M apps/shell/src/lib/routes.ts
?? apps/shell/src/lib/app-api.ts
?? apps/shell/src/pages/AppRegistry.svelte
?? apps/shell/src/pages/Workbench.svelte
?? apps/shell/test/{app-api,app-navigation,app-registry}.test.ts
?? apps/shell/test/fixtures/
?? feature-meta/req-016-dev-frontend.md
```

**共享文件的处置（只有两个，且都只「追加」不「覆盖」）**：

| 文件 | 棒 2 的改动 | 我的改动 | 冲突？ |
|---|---|---|---|
| `packages/i18n/src/locales/{zh-CN,en-US}.json` | +25 条 `app.*`（各 223 键） | **只追加** 6 条 `error.APP_*` | ❌ 实测：`error.APP_*` = 6，`app.*` = 25，两个 locale 都是 |
| `packages/i18n/test/i18n.test.ts` | domain 白名单加 `'app'` | **只追加** 6 个码到 `codes` 数组 | ❌ 实测：`grep -c "'app'"` = **1**（保留）；`pnpm --filter @base/i18n test` → **45 passed** |

---

## 8. 一句话结论

**后端 11 个端点 / 2 张表 / 3 个权限点 / 6 个错误码 / 3 个事件码 / 三组迁移全部落地；
496 条增量测试全绿（0 failed / 0 ignored）；8 条变异全部打红并逐字节还原（M4 顺带挖出一个无区分力夹具，已修）；
11/11 AC 有可核对的 `file:line` + 函数名；未越界到 `apps/shell/**`。**

**两件需要 BA 拍板**：
1. **AC-S-160 步骤 0 的 SQL 物理不可执行**（`platform_config` 无 `scope` 列）→ 我用了 manifest 判据，请确认；
2. **事件码三段式 vs 两段式 / `object_type` 用 `platform_app` vs `app`** → 我跟用例集与已交付约定走，请确认并改 AC 正文两处。
