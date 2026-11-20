# REQ-014 验证报告（简版总表）

- **需求编号**：REQ-014（审计日志）
- **本轮范围**：**棒 1 —— 后端地基**（平台核心审计类型 + 新 crate `platform-audit` + 数据库 + 权限）
- **分支 / worktree**：`feature/REQ-014` @ `feature-REQ-014`（基线 `develop@0683115`）
- **规格版本**：`BA/demands/REQ-014-audit/acceptance.md` **rev 3**（13 条 AC）+ `decisions.md` rev 2（D130~D168）
- **详细自验**（逐 AC 断言、逐 crate 测试数字、变异验证）：见 [req-014-dev-backend-foundation.md](req-014-dev-backend-foundation.md)

> ⚠️ 本文件**覆盖**了上一轮（REQ-010）的同名报告 —— 历史版本在 git 里。
> **状态口径**：**已实现** = 本轮（棒 1）已落地且有可重跑的测试断言；
> **未实现（棒 2）** = 本轮**刻意未做**（下一棒的接入 / 装配 / 前端）；
> **未开始** = 既不在棒 1 也不在棒 2 的规划内。

**一句话结论**：棒 1 的 13 条 AC 中 **9 条已实现**（AC-S-022 / 122 / 123 / 124 / 125 / 126 / 129 / 131 / 133）
**/ 4 条留给棒 2**（AC-S-127 / 128 / 130 / 132）/ **0 条未开始**；
逐 crate 测试 **827 passed / 0 failed / 0 ignored**（10 个 crate，命令见自验报告 §2）；
**5 处变异验证全部变红并逐字节还原**（自验报告 §3）。

---

## 1. 13 条 AC 逐条状态

| AC | 内容 | 状态 | 落点 / 依据 |
|---|---|---|---|
| **AC-S-022** | 哈希链可校验：篡改任一条目即告警 | **已实现（链路与告警部分）** | `platform-audit/src/verify.rs`：两段式校验 + `tracing::error!`（含 `tenant_id` / `event_id` / `reason`，D164）；HTTP 入口（`GET /api/audit/verify`）属**棒 2** |
| **AC-S-122** | 一次敏感操作产生一条含全部固定字段的审计记录 | **已实现** | `platform-audit/src/sink.rs` + `migrations/20261120000200_audit.up.sql`（13 个固定字段，C12）；查询层的「`ip` / `userAgent` **不得缺键**」属**棒 2**（HTTP 响应层） |
| **AC-S-123** | 审计表 append-only（应用角色无法 UPDATE / DELETE） | **已实现** | 迁移的 `REVOKE UPDATE, DELETE` + **`deploy/seed/02-app-role.sql` 的审计表例外块**（D147）；测试含「**重跑种子脚本后仍拒**」 |
| **AC-S-124** | 哈希链按租户分链且可链接，并发写入不产生分叉 | **已实现** | 迁移的 `pg_advisory_xact_lock` + **新语句重读**（D161）+ 测试侧的**独立复算**；分链见 D130 |
| **AC-S-125** | 篡改可检测且沿链连锁 | **已实现** | `platform-audit/src/verify.rs`：两个 reason（`hash_mismatch` / `prev_hash_mismatch`，D146）都有真实触发路径 |
| **AC-S-126** | 审计记录跨租户不可见（数据库强制） | **已实现** | 三张表的 RLS（**手写**，D166）+ `WITH CHECK`；归档表同样受约束 |
| **AC-S-127** | 登录审计：成功与失败各一条，含 IP 与设备，不含凭据 | **未实现（棒 2）** | 业务模块埋点（iam）+ 平台中间件填 `client_ip` / `user_agent` 在**棒 2**；棒 1 已备好 `RequestContext.client_ip` / `.user_agent`（D140）与 sink |
| **AC-S-128** | 权限变更审计：授予 / 撤销 / 创建 / 删除各一条 | **未实现（棒 2）** | 业务模块埋点（rbac）在**棒 2**；事件码与 `before`/`after` 形状已按 D163 钉死并写进自验报告 |
| **AC-S-129** | 统一埋点入口：业务模块不需自建审计表 | **已实现（扫描口径那一半）** | `platform-core/src/extension/audit.rs` 的通用 `AuditEvent` + `AuditSink`；**Rust 侧 0 命中 + `migrations/**/*.sql` 恰好 1 处 + 阳性对照**已有测试；「各模块事件送进平台 sink」的**接入**属**棒 2** |
| **AC-S-130** | 审计查询接口：分页 / 过滤 / 排序 / 权限控制 | **未实现（棒 2）** | 查询**服务**（参数自解析 + C7 游标分页 + 过滤 + 排序）已在 `platform-audit/src/query.rs` 落地并有单测；**HTTP 路由**（`/api/audit/events`、403、ProblemDetails 响应体）在**棒 2** |
| **AC-S-131** | 保留策略与归档：超期记录搬入归档表，归档表同样 append-only | **已实现** | 迁移的 `platform_audit_archive_move`（`SECURITY DEFINER`）+ 链基准表（D159）；测试覆盖「搬走 / 幂等 / 归档表 append-only / 归档不破坏链」 |
| **AC-S-132** | 审计查询界面：列表、分页、过滤、字段展示 | **未实现（棒 2）** | 前端页面属**棒 2** |
| **AC-S-133** | 敏感值防护：`before` / `after` 含敏感键时写入被拒绝 | **已实现** | DB 侧 `platform_audit_sensitive_keys`（权威）+ Rust 侧 `scan_sensitive_keys`；**14 键基线 / 完整键名匹配 / 递归 / 两侧都做 / 一致性测试** |

---

## 2. 棒 1 ↔ 棒 2 的边界（**棒 2 不需要重做棒 1 的任何东西**）

棒 2 直接可用的落点：

| 棒 2 要做的 | 棒 1 已经备好的 |
|---|---|
| `/api/audit/events` 路由 | `platform_audit::AuditQueryService::query` + `AuditQueryParams::parse`（参数自解析；非法值 → `AUDIT_INVALID_FILTER`；超上限 → 夹到 200） |
| `/api/audit/verify` 路由 | `platform_audit::ChainVerifier::verify` → `VerifyReport { ok, checked, broken }` |
| `/api/audit/archive` 路由 | `platform_audit::ArchiveService::archive_before` → `ArchiveResult { moved, archivedAt, nextRecommendedAt }` |
| 路由鉴权 | `platform_audit::{require_events_read, require_verify_run, require_archive_run}`（C-5 字面量调用点） |
| sink 装配 | `platform_audit::PgAuditSink::new(TenantDb)` + `impl AuditSink` |
| 中间件填 IP / UA | `RequestContext::with_client_ip()` / `.with_user_agent()`（D140） |
| 业务模块埋点 | `platform_core::extension::audit::{AuditEvent, AuditSink}`（通用、带 payload） |
| 错误码 → ProblemDetails | `platform_core::problem::ProblemDetails::audit_*`（5 个构造器） |

---

## 3. 本轮**未开始**（既不在棒 1 也不在棒 2 的任务卡里）

| 项 | 说明 |
|---|---|
| `audit.retention.*` / `audit.page.*` 四个配置项登记 | design §8 要求登记进 `crates/platform-config/manifest.json` + 迁移 INSERT（配置项有严格流程，漏 INSERT → 启动失败）。棒 1 的 `crates/platform-audit/manifest.json` 的 `config` 段**留空**（**不引入未登记的配置项**）；归档节奏当前是常量 `RECOMMENDED_INTERVAL_DAYS = 1` |
| `platform_audit_log` 的**物理清理**（`archive_days`） | design §8：本轮只登记不执行 → 记入 tech-debt |
| 审计的**分库 / 分区** | 父 AC 之外的容量议题，本轮无判据 |

---

## 4. 棒 2 —— **前端（AC-S-132）** 更新（**追加**，不改上面棒 1 的任何内容）

| AC | 内容 | 状态 | 落点 / 依据 |
|---|---|---|---|
| **AC-S-132** | 审计查询界面：列表、分页、过滤、字段展示 | **已实现（前端侧；数据类 E2E 待后端路由落地）** | `apps/shell/src/lib/audit-api.ts` + `apps/shell/src/pages/Audit.svelte` + `apps/shell/src/lib/routes.ts`（`/admin/audit`，权限点 `audit:events:read`，路由 `load` 取首屏 → 守卫不放行时目标接口请求数 **0**）；**游标取数 + 游标栈页码指示**（D169）；5 个错误码 i18n（D170-1）；E2E 夹具（`scripts/seed-e2e.sh`：`u-perm-audit@example.com` + 11 条审计记录，D170-2） |

**测试数字（真跑）**：`@base/shell` **206 passed**（棒 1 后 195 → **+11**）／`@base/i18n` **43 passed**／
`@base/sdk` **38 passed**／`pnpm -r build` 全绿／`svelte-check` 0 errors 0 warnings／
**E2E 全量 30 passed / 5 skipped / 0 failed**（审计页 1 passed = TC-014-118/119 真跑通过；
5 条数据类按「**被测系统未就绪**」跳过 —— `crates/platform-server/src/audit.rs` 尚未落地，
探测口径见自验报告 §5.3）。

**详见**：[req-014-dev-frontend-round2.md](req-014-dev-frontend-round2.md)（含 2 条需 BA 确认的发现：
① 后端拒绝「游标 + 任何排序」；② 未注册的 `/api/...` 回落成 SPA → 就绪探测必须看内容类型）。

---

## 5. 棒 2 —— **后端（接入与接口）** 更新（**追加**，不改上面棒 1 的任何内容）

- **本棒范围**：D171 链尖 + 末尾截断检出、D172 驼峰拆词、业务模块埋点（iam / rbac / config / org）、
  sink 装配（D150）、`/api/audit` 路由、全局中间件（D149）、xtask、D178 配置登记、**D180**（静态回落的 API 前缀）。
- **详细自验**（逐 AC 断言、逐 crate 测试数字、5 处变异验证证据）：见
  [req-014-dev-backend-integration.md](req-014-dev-backend-integration.md)

| AC | 内容 | 状态 | 落点 / 依据 |
|---|---|---|---|
| **AC-S-022** | 哈希链可校验：篡改任一条目即告警 | **已实现** | 棒 1 的校验器 + **本棒**的 D171 链尖（`platform_audit_chain_state.tip_hash` + `platform_audit_tip_hash`）→ **末尾截断**也能检出（`a_truncated_online_tail_is_detected_after_the_archive`）；**HTTP 入口** `GET /api/audit/verify`（`crates/platform-server/src/audit.rs`）+ **`cargo xtask audit verify`**（实测：篡改一条 → `失败（断裂 1 条）` + **exit=1**） |
| **AC-S-126** | 审计记录跨租户不可见（数据库强制） | **已实现（本棒补齐接入面）** | 三张表的 RLS + `WITH CHECK`（棒 1）；**本棒**新增的三个业务模块埋点全部走**租户上下文**（`RequestContext.tenant_id`）→ 事件**无法**写到别的租户；`platform_audit_tip_hash` / `platform_audit_archive_move` 有**显式租户守卫** |
| **AC-S-127** | 登录审计：成功与失败各一条，含 IP 与设备，不含凭据 | **已实现** | `crates/platform-iam/src/audit.rs` 的 `login_event()`（**结构上无法**产出含账号名的失败事件）+ `crates/platform-iam/src/service.rs` 的三条失败路径各写 1 条；**全局中间件** `crates/platform-server/src/client_info.rs` 填 `client_ip`（`x-forwarded-for` 第一跳）/ `user_agent`；`tests/audit_events.rs` 4 条真库用例断言「成功 1 条 / 失败 1 条 / 两条失败路径**条数相同**（不可枚举）/ 审计写失败 → **503 `AUDIT_SINK_UNAVAILABLE`**」 |
| **AC-S-128** | 权限变更审计：授予 / 撤销 / 创建 / 删除各一条 | **已实现** | `crates/platform-rbac/src/service.rs` 四处埋点（`rbac.role.create/delete/grant/revoke`）；`before` / `after` 是**排序后**的字符串数组；**新增** `crates/platform-rbac/tests/audit_events.rs` 5 条真库用例逐条断言事件码 / `objectType` / `objectId` / 两侧数组 / 排序 |
| **AC-S-129** | 统一埋点入口：业务模块不需自建审计表 | **已实现（本棒补齐「注入」那一半）** | **装配路径落地**：`crates/platform-server/src/main.rs` 构造 `PgAuditSink` → `Arc<dyn AuditSink>` → 注入 iam / rbac / config / org / sso-recorder；既有 recorder trait 的**签名一律未改**（`SsoAuditRecorder` / org 的 `AuditRecorder` 都实现**同一个 trait**）；棒 1 的扫描用例仍绿（Rust 侧 **0** 命中 + `migrations/**` 恰好 **1** 处 + 阳性对照） |
| **AC-S-130** | 审计查询接口：分页 / 过滤 / 排序 / 权限控制 | **已实现（HTTP 层）** | `crates/platform-server/src/audit.rs`：`GET /api/audit/events`（**参数自解析** → 非法值 **400 `AUDIT_INVALID_FILTER`**、超上限夹到 200）、`GET /api/audit/events/{id}`（404 `AUDIT_EVENT_NOT_FOUND`）；权限点走 `platform-audit` 的**鉴权薄封装**（D160，**不**在本文件写权限点字面量）；`tenant_id` 查询参数**忽略 + warn**（D162）；响应字段 **camelCase**（D167）；⚠️ **游标 + 排序 → 400** 是本仓的**收紧**（D179，契约 C7 未写 → 见自验报告 §5 偏离 4） |
| **AC-S-131** | 保留策略与归档：超期记录搬入归档表，归档表同样 append-only | **已实现（本棒补齐「执行入口」）** | 棒 1 的 `platform_audit_archive_move` + `ArchiveService`；**本棒**：`ArchiveService::DEFAULT_ONLINE_DAYS = 90`（与登记的 `audit.retention.onlineDays` 一致）、`archive_older_than()`、**HTTP 入口** `POST /api/audit/archive`（权限点 `audit:archive:run`）与 **`cargo xtask audit archive --tenant <id>`**（**单租户**，D158；**应用角色**连接，D174）；实测：`已归档租户 tenant-b 的超期记录：1 条`，旧记录进归档表、新记录留在线表 |

**测试数字（真跑）**：**912 passed / 0 failed / 0 ignored**（11 个 crate，命令见自验报告 §2）；
逐 crate：`platform-core` 153 / `platform-audit` 46 / `platform-iam` 178 / `platform-rbac` 83 /
`platform-config` 88 / `platform-org` 93 / `platform-server` 121 / `platform-db` 46 /
`platform-schema` 20 / `xtask` 51 / `sandbox` 33。
**6 处变异验证全部变红并逐字节还原**（自验报告 §3：M1 末尾截断 / M2 驼峰拆词 / M3 API 前缀 /
M4 `after` 排序 / M5 `before` 排序 / M6 路由提取器接线）。
**另外如实记录两个本棒自己写出来、又在交付前抓到的接线 bug**（`Extension<ReqCtx>` / `Extension<Arc<AuditState>>`
→ `/api/audit/*` 会全返回 **500**）与对应的兜底用例，见自验报告 §3.6。
**真实库**（`base_test`）：3 张表 + 7 个函数 + 迁移 `20261120000400`（4 个 `audit.*` 配置项）全部就位。

**需 BA 裁定（3 条，详见自验报告 §5）**：
① **D178 的键名**（snake_case vs 契约 C9 的 camelCase）—— 本棒按 camelCase 落地并留了映射表；
② `audit.page.defaultSize` = 50 vs 契约 C7 的默认 limit = 20 —— 本棒**只登记不执行**，行为按 C7；
③ 契约 C6 是否强制 `application/problem+json` —— 全平台 `problem_response` 目前统一 `application/json`，
本棒**只改错误体形状**、不另立一套 Content-Type。
