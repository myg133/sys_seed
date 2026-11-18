# REQ-010 验证报告（自验）

- **需求**: REQ-010「配置、品牌与 Feature Flag」
- **分支**: `feature/REQ-010`（基线 `develop` @ `12366e9`）
- **验收基线**: `BA/demands/REQ-010-config-branding/acceptance.md`（**rev 3**，AC-S-049 ~ AC-S-063，15 条）
- **契约**: `api-conventions v2`（C3 / C6 / C9 / C10）、`extension-points v2`（C-5）
- **开发者**: myg133-dev-1（Worker 会话）
- **更新时间**: 2026-11-18

> 本报告只写**实际跑过**的东西。没跑的一律进 §6「未验证项」，不写成「应该没问题」。

---

## 1. 结果摘要

| 项 | 结果 |
|---|---|
| Rust 全量（`cargo test --workspace -- --test-threads=2`） | **475 passed / 0 failed / 70 ignored**（ignored 的是 IAM / ORG / RBAC / sandbox 的真实库用例，默认 `#[ignore]`，由下一行的集成脚本用 `--include-ignored` 跑到） |
| Rust 集成（`scripts/test-integration.sh`，**重置 base_test 后 `--include-ignored`**） | **539 passed / 0 failed / 0 ignored**（含 IAM / ORG / RBAC / sandbox 的真实库用例；多轮连跑 536 → 537 → 539，差值都来自本轮新增的用例） |
| 本需求新增/改动的 Rust 用例 | **72**（`platform-config` lib 30 + `config_real_db` 25 + `http_authz` 10 + `contract_c5` 3 + `branding_static` 4） |
| 前端单测（`pnpm -r test`） | **19 passed**（shell 9：`nav` 4 + `branding` 5；packages 10：i18n 3 / sdk 2 / theme 2 / ui 3） |
| 前端类型检查（`pnpm -r check`） | **通过**（svelte-check 0 errors / 0 warnings） |
| E2E（Playwright，**同源** `platform-server` 静态目录） | **2 passed / 0 failed**，连跑两轮均绿（标题来自品牌接口；渲染色 ≠ `#1677ff`；菜单过滤） |
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo xtask check-deps` / `check-modules` / `capabilities --check` | 通过 |
| `cargo xtask tenant-policies --check` | 通过（4 个模块声明的表都开了 RLS 且有策略） |
| `./scripts/sync-contracts.sh` | 通过（`docs/contracts/api-conventions/v2.md`、`extension-points/v2.md` 与 BA 正本逐字节一致） |
| `Cargo.lock` | **第三方 registry 仍 284**；总包数 294 → **295**（新增的是本仓 workspace 成员 `platform-config`，不是第三方依赖） |

**真实跑过的关键数字**：
- 灰度 p=50、n=200 的命中数 **104**（接受区间 [70,130]）；
- 两进程命中集合**完全一致**（104 个用户，逐条比对）；
- p=60 相对 p=50 **严格超集**；改回 p=50 **完全复原**；
- 平台级 `platform_config_upsert` 写一行 → `platform` epoch **+1**；
- 掩蔽顺序：`tenant:tenant-a = 5`、`platform = 1`，平台级改值后 T1 仍读到新值。

---

## 2. AC 逐条结论（15 / 15）

| AC | 结论 | 直接证据（实测） |
|----|------|------------------|
| AC-S-049 | ✅ | `three_layer_resolution_reports_the_hit_layer`（platform/tenant/space/跨租同名 space）；`source_default_is_distinct_from_an_explicit_platform_value`（default vs platform 的判别性反例）；混合批量 404 + `params.unknownKeys` |
| AC-S-050 | ✅ | `scope_escalation_is_rejected_and_leaves_no_rows`（6 步 + 平台正对照 + 库侧 0 行）；`space_writes_require_the_branding_whitelist`（`loginBanner` 空间写 400，`reason=whitelist`） |
| AC-S-051 | ✅ | `unknown_keys_and_type_errors_follow_the_fixed_order`（未登记/类型/顺序 5 例）；`five_registered_types_are_checked_by_json_type`（5 种类型 × 正反 + `jsonb_typeof`） |
| AC-S-052 | ✅ | `a_platform_change_invalidates_every_tenant_and_survives_a_higher_tenant_epoch`（TTL=300s，两实例）；`a_platform_write_is_visible_on_the_next_request`（HTTP） |
| AC-S-053 | ✅ | 同上：先 5 次租户写（tenant epoch=5）再改平台（platform=1），T1 仍返回新值；平台写入不改租户 epoch |
| AC-S-054 | ✅ | `branding_returns_only_whitelisted_keys_and_resolves_the_tenant`（T1/T2/无租户/未知租户/空间忽略/键集合）；HTTP 同源实测（见 §4） |
| AC-S-055 | ✅ | `branding_static.rs` 4 条（`底座壳`/`#1677ff`/`示例平台`/`#0055aa` = 0；品牌接口被调用；`<title>` 留空；e2e 同步）；E2E 2 条实测通过（标题 = 接口值；计算色 ≠ `rgb(22,119,255)`） |
| AC-S-056 | ✅ | `flags_are_tenant_isolated_and_close_returns_200_semantics`；HTTP 同用例（T1 true/tenant、T2 false/default、无令牌 401） |
| AC-S-057 | ✅ | `rollout_is_deterministic_monotonic_and_bounded`（10 次一致；104 ∈ [70,130]；S50 ⊊ S60；S50'==S50；S10⊆S50；0/100；两独立实例 + 独立公式）；`rollout_across_two_processes_is_identical`（**真子进程**）；`rollout_out_of_range...`（写 400 / 读 fail closed） |
| AC-S-058 | ✅ | 同上第 6 段（无 user → false，p=100 不变）；HTTP 无令牌 → 401 |
| AC-S-059 | ✅（含 1 条与 AC 冲突的取舍，见 §5-Q1） | `unknown_and_invalid_flag_names_follow_the_contract`：拼错 → 200 false default；换行/1KB/kebab → 400 `FLAG_NAME_INVALID`；对照 `newInvoice` → true |
| AC-S-060 | ✅ | `write_permission_matrix_has_no_implicit_hierarchy`（M/A/P/O/R 五档 + 4 个权限点正交）；401（7 条路由）；跨租头 404 且不落库；`the_generic_write_route_rejects_the_flag_namespace`（400 `CONFIG_FLAG_USE_FLAG_ENDPOINT`） |
| AC-S-061 | ✅ | `contract_c5.rs` 3 条：4 个调用点逐个；删 1 → 1 条点名、删 2 → 2 条、不误报；基线 org 9/0、rbac 4/≥3、`rbac:permission:read` 仍缺；sandbox 无 `config:` |
| AC-S-062 | ✅ | `base_app_can_read_but_cannot_write_the_platform_table_directly`（SELECT 可；3 种 DML 均 **42501**；函数可写 + epoch +1；权限矩阵 5 项）；RLS 读写隔离（含跨租同名 space）；三列 `jsonb`；独立库 up/down/up 插 3 种 JSON 类型；保留租户 id 全入口拒绝 |
| AC-S-063 | ✅ | `read_authorization_splits_platform_scoped_keys`（M 读租户键 200 / 平台键 403 / 混合 403 无 `entries` / `favicon` 平台值不需要权限点 / 独立路由 M 403、P 200）；`the_platform_read_endpoint_only_serves_platform_scoped_keys` |

---

## 3. 破坏性验证（造违规 → 确认拦住 → **逐字节还原**）

每一项都用 `sha256sum` 记录修改前/后的文件指纹；还原后指纹**完全一致**，且测试重新变绿。

| # | 违规 | 期望被谁拦住 | 实测 |
|---|------|--------------|------|
| D1 | down 迁移追加 `DROP TABLE platform_tenant_config` | `the_schema_migration_is_reversible_without_dropping_the_historical_table` 的静态断言 | ✅ 测试 **FAILED**；还原后 sha256 一致 |
| D2 | 迁移/种子里的 `REVOKE` 被去掉（模拟忘了 REVOKE） | `base_app_can_read_but_cannot_write_the_platform_table_directly` 的 42501 断言 | ✅ 直接 DML 成功 → 测试 **FAILED**；恢复 REVOKE 后 **passed** |
| D3 | 通用写路由去掉 flag 命名空间隔离（D70） | `flags_cannot_be_written_through_the_generic_config_route` | ✅ **FAILED**；还原后 sha256 一致 |

> D2 的「造违规」按**运行时**做（`GRANT INSERT/UPDATE/DELETE ... TO base_platform_app`），
> 因为迁移已经应用过；同时把 `deploy/seed/02-app-role.sql` 里的守护 REVOKE 临时改成 GRANT，
> 否则夹具重跑会把违规又盖回去。恢复后脚本与 DB 权限都还原。

## 4. 变异验证（≥4 处；改坏 → 测试必须红 → 还原 → 绿）

| # | 变异 | 命中的测试 | 结果 | 还原指纹 |
|---|------|-----------|------|----------|
| M1 | 缓存新鲜度改回 `max(platform, tenant)` | `a_platform_change_invalidates_every_tenant_and_survives_a_higher_tenant_epoch` | **FAILED**（0 passed） | sha256 一致 |
| M2 | 灰度改用 `RandomState` 随机播种哈希 | `rollout_across_two_processes_is_identical` | **FAILED**（两进程集合不同；同进程每次运行也不同） | sha256 一致 |
| M3 | 品牌白名单加入 `conflictStrategy`（非白名单键） | `branding_returns_only_whitelisted_keys_and_resolves_the_tenant` | **FAILED**（公开接口泄露运维键） | sha256 一致 |
| M4 | 去掉 flag 命名空间检查（D70） | `flags_cannot_be_written_through_the_generic_config_route` | **FAILED** | sha256 一致 |
| M5 | down 迁移加 `DROP TABLE platform_tenant_config` | 迁移可逆性测试 | **FAILED** | sha256 一致 |
| M6 | 去掉 `REVOKE`（运行时） | 平台表写权限测试 | **FAILED**；恢复后 passed | 脚本 sha256 一致 |
| M7 | 去掉空间级写的**品牌白名单门**（D66 第二道门） | `space_writes_require_the_branding_whitelist` | **FAILED**（`loginBanner` 空间写变成功） | sha256 一致 |

> 每次变异都先 `cp` 备份、改坏、跑**指定**用例、再 `cp` 还原；还原后重跑为绿。
> M1 若用 `max(epoch)`，掩蔽用例在「tenant epoch=5 > platform epoch=2」时返回旧值。

## 5. 对规格的疑问 / 自行取舍（**按规格实现，未改设计**）

| # | 位置 | 问题 | 本实现的取舍 |
|---|------|------|--------------|
| Q1 | AC-S-059 步骤 4 vs 同节「flag 名规范 `^[a-z][a-zA-Z0-9]{0,63}$`（**无连字符**）」 | 步骤 4 要求 `new-invoice`（kebab）→ 200 + false；字符集规则要求它 **400**。**同一条 AC 内部矛盾**。 | 按**更严格**的字符集规则实现：`new-invoice` → **400 `FLAG_NAME_INVALID`**。理由：D65 的安全动机（非法名不进日志，防日志注入 / 刷爆），且「不归一化」已由 `newinvoce` vs `newInvoice` 两个合法名覆盖。**QA 若按 TC-010-030 步骤 4 断言 200，会红** —— 需 BA 裁定。 |
| Q2 | design §1 的 `ConfigResolver` 方法是**同步**的；§7.1 又要求**每个请求**读 `(platform_epoch, tenant_epoch)` 判新鲜 | 同步 trait 无法做 DB I/O；若在异步 handler 里 `block_on` 会阻塞运行时。 | trait 保持同步（dyn 兼容，业务模块 `Arc<dyn ConfigResolver>`）；平台实现 `DbConfigResolver` 另提供 **异步 `refresh(&ctx)`**，HTTP 中间件在每个请求进入时调用，trait 方法只读已刷新快照。**未改 trait 签名**。 |
| Q3 | 规格没有说明「哪些 `config` 键是 flag」 | `ConfigSpec` 没有 `isFlag` 字段；D57 只说不改 manifest schema。 | 把**登记为 `boolean` 的配置键**视为 flag（当前仓库只有 `newInvoice` / `beta`）。派生键因此可自动生成。若将来出现「布尔但非开关」的配置，需要新字段。 |
| Q4 | D71 要求 `platform_config_upsert` 同事务自增 epoch；AC-S-062 步骤 7 只验「写一行 +1」 | 一次请求写 N 条平台级配置会 +N（函数按行调用）。 | 保持函数按调用 +1（与 design 的 DDL 一致）；多条目平台写会多次 bump。**不影响正确性**（任何 +1 都让缓存失效），但「一次写请求只 +1」需要函数改成批量签名（规格未要求）。 |
| Q5 | D72 要求真实租户 id 为 `"platform"` 时读写报错，但**没有给错误码** | 新增稳定码会扩大 D62 的清单。 | 新增 `CONFIG_TENANT_RESERVED`（400），四个对外入口（读/写/品牌/refresh）都拒绝；写进本报告。 |
| Q6 | AC-S-050 / TC-010-005 步骤 8 要求「另外两张表 `key='conflictStrategy'` 行数均为 0」，但 AC-S-054 的夹具**故意**在 `platform_space_config` 放了一行 `conflictStrategy=remote_wins`（管理员 SQL 合成） | 两条用例的夹具互斥。 | 测试改为断言「**本次越级写没有新增行**」：租户表 0 行；空间表除夹具合成行外 0 行。规格文本未改。 |
| Q7 | 通用读路由混入平台级键的行为在 acceptance 里是「整请求 403」，但**没有说**持 `config:platform:read` 的用户能否走通用路由 | 契约 9.7 只说「通用端点收到平台级键 → 整请求 403」。 | 通用路由**无条件 403**（平台级读一律走独立路由），避免「同一端点两种授权语义」。 |
| Q8 | 品牌公开接口遇到 `X-Tenant-Id: platform` | D72 说保留值读写都报错；branding 是公开接口，规格没说。 | 返回 **400 `CONFIG_TENANT_RESERVED`**（更严格；不会把平台伪租户当成真实租户去解析）。 |
| Q9 | `GET /api/feature-flags`（批量）的响应形状 | design §6 只给了单条 `{"flag","enabled","source"}`；TC 写 `{"flags":[...]}`。 | 实现为 `{"flags":[FlagDecision...]}`（按 registry 的 flag 名排序）。 |
| Q10 | TC-010-013（rev 2）步骤 4 用 **通用读路由** 读 `conflictStrategy,scimBaseUrl,cacheTtlSeconds` 并期望 200 | 与 rev 3 的 AC-S-063 / 契约 C9 §9.7 冲突：通用端点收到平台级键必须**整请求 403**，平台级读走独立端点。 | 按 **AC（rev 3）** 实现：通用读路由 403；平台级键请改用 `GET /api/config/platform-entries`（P/R 角色 200）。TC-010-013 需按 rev 3 修订。 |

## 6. 未验证项（如实列出）

| 项 | 原因 | 影响 |
|----|------|------|
| **CI 真跑**（6 个 job / 镜像体积门禁 / Trivy） | 本仓**无 remote**，GitHub Actions 跑不起来（与 REQ-009 相同） | 本地跑通了 lint / test / build / E2E；CI 配置未改 |
| **分支保护**（required check + Code Owners） | 代码里做不到，需人工在远端配置 | — |
| **真实多实例部署的缓存收敛** | 本机单实例；AC-S-052/053 用**两个独立解析器实例**、AC-S-057 用**真子进程**模拟 | 未在 K8s / 多副本下实测 |
| **主题令牌体系**（深色模式 / 组件密度 / 字体栈的组件级应用） | 明确属 REQ-011（D52） | 本期只做「shell 从接口读并应用」 |
| **配置管理界面** | 属 REQ-012 | — |
| **覆盖率数字** | 本期不设覆盖率门槛 | — |
| **`platform-org` / `platform-rbac` 的 C-5 缺口** | 本需求**不扩围**（D58） | 基线：org 9/0、rbac 4/3（`rbac:permission:read` 仍缺）；已在本报告登记 |
| **未知租户 404 vs 回落平台默认** | 本仓没有租户注册表；AC-S-054/TC-010-014 采用「回落平台默认」 | 若后续裁定未知租户必须 404，需要租户注册表 |
| **`X-Tenant-Id` 在既有 `/api/rbac` 路由上的跨租 404** | 既有中间件不解析该头（它假定上游注入）；本需求只在 `/api/config` / `/api/feature-flags` 自带中间件里实现 | 未改 rbac（超出本需求范围）；已在 §7 风险里点名 |

## 7. 风险与遗留

| # | 风险 | 处置 |
|---|------|------|
| R1 | **`deploy/seed/02-app-role.sql` 被修改**：重跑时会对 `platform_config` 再收一次直接 DML | 这是保持 AC-S-062 在「补建角色」场景下仍成立的**必要**改动（否则 `GRANT ALL TABLES` 会把迁移里的 REVOKE 盖回去）。已加条件 `to_regclass`，表不存在时跳过 |
| R2 | 平台级写一次请求写 N 条 → epoch +N | 正确性不受影响；若要「一次请求 +1」，需改函数签名为批量（规格未要求） |
| R3 | `ConfigResolver` 的「先 refresh 再 resolve」是**调用约定**，不是类型强制 | 平台 HTTP 中间件已统一 refresh；业务模块直接调用同步 trait 时若忘了 refresh，会读到 `None`（fail closed，不会读旧值） |
| R4 | E2E 依赖 `DATABASE_URL_TEST` 与本机容器 | `scripts/e2e-server.sh` 已说明前置；`playwright.config.ts` 的 webServer 会自动迁移 + 起服务 |
| R5 | C-5 扫描器修了「生命周期被当字符字面量」的缺陷 | 新增 2 条单元测试守住；影响面是**修复**（此前 permission.rs 无生命周期所以未暴露） |
| R6 | `platform_space_config` 同时由迁移与 `cargo xtask tenant-policies` 建策略 | 两者条件/名字一致（`platform_space_config_tenant_isolation`），幂等 |
| R7 | `feature-meta/verification-report.md` 与 `traceability.md` 覆盖了分支上原有的 REQ-009 副本 | 这两个文件在 `feature/REQ-010` 分支上本是模板副本；本报告替换为 REQ-010 内容 |

## 8. 实际命令与输出（可复跑）

```bash
# 1) 全量 Rust（真实库用例默认 ignored）
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST='postgres://base:base@127.0.0.1:5432/base_test'
cargo test --workspace -- --test-threads=2
# → 475 passed / 0 failed / 70 ignored

# 2) 集成（重置 base_test + include-ignored）
./scripts/test-integration.sh
# → 539 passed / 0 failed / 0 ignored；容器数量 2 → 2

# 3) lint / 结构
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask check-deps
cargo xtask check-modules
cargo xtask capabilities --check
DATABASE_URL='postgres://base:base@127.0.0.1:5432/base_test' cargo xtask tenant-policies --check
./scripts/sync-contracts.sh

# 4) 前端
pnpm install --offline --frozen-lockfile
pnpm -r test        # → 19 passed
pnpm -r check       # → 0 errors / 0 warnings
pnpm --filter @base/shell build

# 5) E2E（同源：platform-server 托管 apps/shell/dist）
pnpm run test:e2e   # → 2 passed

# 6) 同源手工冒烟（无 IAM 时公开品牌仍可用）
DATABASE_URL='postgres://base_app:base@127.0.0.1:5432/base_test' \
BASE_STATIC_DIR=/tmp/req010-static BASE_MANIFEST_ROOT=. SERVER_PORT=18080 \
  ./target/debug/base serve
curl -s -H 'X-Tenant-Id: tenant-a' http://127.0.0.1:18080/api/config/branding
# → logo=/branding/tenant-a.svg（T1 租户值）
curl -s http://127.0.0.1:18080/api/config/branding
# → logo=/branding/platform-a.svg（平台默认）
curl -s -o /dev/null -w '%{http_code}' 'http://127.0.0.1:18080/api/config/entries?keys=logo'
# → 401
```

## 9. 权限点三件套与注册表对账

| 项 | 值 |
|---|---|
| manifest | `crates/platform-config/manifest.json`（4 个权限点） |
| 种子迁移 | `migrations/20261118000902_config_permission_seed.up.sql`（4 行；down 只删这 4 行） |
| `capabilities/permissions.json` | 4 个 config 点；总数 22 |
| `platform_permission` 实测 | 4 行（`config:entry:write` / `config:flag:write` / `config:platform:read` / `config:platform:write`） |
| 启动对账 | `reconcile_permissions` 扫描 `crates/*/manifest.json` 时自然包含本 crate；种子迁移已应用 → 一致（未在「配 IAM」的完整启动里重复跑，见 §6） |
