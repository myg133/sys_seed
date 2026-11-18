# REQ-008 状态

- **需求**: REQ-008 用户与组织架构
- **分支**: `feature/REQ-008`
- **基线**: `develop` @ `a0a8d7a`（REQ-002 ~ REQ-007 已合并）
- **状态**: **待验证**（本状态由 Dev 按任务要求置位，**最终状态机由 BA 推进**）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18（**第一轮：实现 + 自验完成**；**第二轮：修复轮完成** —— QA `PASS-WITH-RISK` 的 2 中 + 3 低）
- **改动**: 第一轮 44 文件 / +10214 -775 行（新建 `crates/platform-org`）；**修复轮 9 文件 / +614 -58 行**（代码 6：2 份迁移 + 3 个源文件 + 1 个测试文件；文档 3 份）
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`
- **设计依据**: `BA/demands/REQ-008-user-org/design-summary.md`（D1–D12 照做）

## 修复轮（QA `PASS-WITH-RISK` → 2 中 + 3 低）

| # | 级别 | 问题 | 处理 | 证据 |
|---|------|------|------|------|
| 1 | **中** | 同步错误计数是**死代码**（失败路径不写 `sync_source_state`） | **已修**：`store.rs::record_sync_failure`（计数递增 / 错误摘要写入 / 上次成功时间不动）+ `service.rs::sync_ldap` 失败留痕 | 真实库层 `a_failed_sync_records_the_error_and_a_successful_one_clears_it` |
| 2 | **中** | `user_id` 侧**没有外键**（幽灵 / 跨租户 `user_id` 可写） | **已修**：新迁移 `20261118000800_org_user_fk`（3 张表加复合外键 + `platform_iam_user` 补 `UNIQUE (tenant_id, id)`） | 真实库层 `the_database_refuses_a_ghost_user_id` / `the_database_refuses_a_cross_tenant_user_id` |
| 3 | 低 | 依赖归属不准 | **已更正**：ldap3 = **8**、SCIM 出站 = **2**（`want` / `try-lock`） | `verification-report.md` §3.1.2 |
| 4 | 低 | 构建图绝对值口径不一致 | **已写清**：四个口径 + 逐字命令 | `verification-report.md` §3.1.1 |
| 5 | 低 | LDAP 缺**前置** URL 校验 | **已修**：`ldap.rs::validate_ldap_url`（连接前校验，三种 scheme 各有明确错误） | 4 个单测 + `verification-report.md` §9.3 |

## 交付形态（**与任务书预设不同，如实登记**）

| 项 | 结论 |
|----|------|
| **实现位置** | **`crates/platform-org`**（平台 crate），**不是** `modules/org`（业务模块） |
| 原因 1 | 离职禁用**必须**撤销 IAM 会话 → 业务模块会**依赖平台内部模块**（`extension-points v1` §1「D7 硬约束」明禁） |
| 原因 2 | `check-deps` 禁止 `platform-server`（平台 crate）依赖业务模块 → 业务模块形态下 **`/api/org/*` 挂不上** |
| 租户隔离 | 不降级：`crates/platform-org/manifest.json` 的 `tenantScoped` 段声明 6 张表 → 平台**统一生成** policy（`cargo xtask tenant-policies` ✅）；`platform-db::collect_scopes` 扩展为扫 `modules/` **与** `crates/` |
| 与 REQ-007 同构 | `platform-core ← platform-db ← platform-iam ← platform-org ← platform-server` |

## 本轮交付

| 交付项 | 位置 |
|--------|------|
| 组织树（**邻接表 + 递归 CTE**，不用 ltree）、`path_labels` 加速字段与重建 | `crates/platform-org/src/{store,service,tree}.rs` |
| 成员关系（多对多 + `is_primary`）、岗位、**汇报关系（`user.manager_id`）** | `crates/platform-org/src/{store,service}.rs`、`platform_iam_user.manager_id` |
| **SCIM 2.0 出站**（自己实现、**PUT 幂等**、**异步推送**、字段映射外置、指数退避重试） | `crates/platform-org/src/scim.rs`、`service.rs::full_scim_sync` |
| **LDAP / AD 入站**（`ldap3`、映射外置、**位运算离职判定**、增量 + 全量对账） | `crates/platform-org/src/ldap.rs`、`service.rs::sync_ldap` |
| **字段级冲突队列** + 人工裁决（默认 `manual`，**绝不静默覆盖**） | `crates/platform-org/src/{sync,service}.rs` |
| **离职自动禁用**（禁用 + **撤销全部会话** + 解绑 + 审计 + 通知 + 出站 `active:false`） | `crates/platform-org/src/service.rs::disable_departed_user` |
| 组织变更事件 → `NotificationSender`（**只转发**的可对象化适配器） | `crates/platform-org/src/notify.rs` |
| `/api/org/*`（**17 个路由**，每个接口都验令牌） | `crates/platform-org/src/http.rs` |
| 平台迁移：6 张组织表 + 账号表 3 列 | `migrations/20261118000700_org_schema.*.sql`、`20261118000600_iam_user_org_attrs.*.sql` |
| **修复轮**：用户侧复合外键（`(tenant_id, user_id)` → `platform_iam_user (tenant_id, id)`）+ `platform_iam_user` 的 `UNIQUE (tenant_id, id)` | `migrations/20261118000800_org_user_fk.*.sql` |
| 平台 crate 的 manifest 进镜像（否则部署期看不到组织表的租户声明） | `Dockerfile` |

## 自验结论（**实测**，2026-11-18；**修复轮复跑**）

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **299/299**（第一轮 295 → **+4**，只增不减）
- 带 `--include-ignored` ✅ **336/336**（第一轮 329 → **+7**；真实库层 **14/14**，SCIM **7/7**）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** / `cargo fmt --all --check` ✅
- `cargo xtask capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅
- `bash scripts/test-integration.sh` ✅ **exit 0**（容器 **2 → 2**）
- `pnpm -r test` ✅ **23/23** / `bun run test:e2e` ✅ **1/1**
- ★ **组织树**：移动 → 子树**立即**反映新结构；`path_labels` 与 `parent_id` 一致；写坏 → **报得出** → **可重建**；成员关系**不破坏**
- ★ **SCIM 出站**：**真的 HTTP** 打 mock 服务 → PUT 两次 **1 条**；两次全量同步 **条目数不变**；字段映射逐字段生效；目标延迟 800ms → **本地入队 < 100ms**
- ★ **LDAP 位运算**：`512`/`660`/`402653184` 在职，`514`/`546`/`4098`/`2` 离职（布尔比较会误判 `660`）；**scheme 前置校验**（`ldap://` 放行 / `ldaps://` 与其它 scheme 明确报错）
- ★ **冲突队列**：**字段级**；`manual` 下**本地不被覆盖**；裁决后**按裁决生效**
- ★ **离职禁用**：账号禁用 + **旧 refresh 立即作废**（直查库 `revoked_at` 非空）+ 会话全撤销 + 成员解绑 + 审计 + 通知
- ★ **租户隔离**：跨租户 id → `None`；**绕过应用层 → 0 行**；**直查 `pg_class`** → 6 张表**全部** `relrowsecurity=true` 且有 policy；**用户侧外键**（幽灵 / 跨租户 `user_id` 被 DB 层拒绝）
- ★ **同步状态可见性**：失败 → `error_count` 递增 + `last_error` 非空；成功 → 归零 / 清空
- **冷编译实测**（第一轮）：`cargo check` **52.7s / 1038 MB**；`cargo test --no-run`（含链接）**83.4s / 1638 MB**（`jobs` 保持 3）
- **依赖树**：口径 A（第三方 crate 去重名）**263 → 273（+10）** —— **ldap3 = 8、SCIM 出站 = 2**（`want` / `try-lock`）；口径 C（构建图去重 `name vX`）**239 → 253（+14）**；**ring / aws-lc-rs 0 命中**（四个口径的命令见 `verification-report.md` §3.1.1）

## 未完成 / 未验证（如实登记）

- **只支持明文**：SCIM `http://` / LDAP `ldap://` —— 树里无 TLS 栈（`ring` / `aws-lc-rs` 预算要求 0 命中）；HTTPS / LDAPS **明确报错**（LDAP 侧修复轮已加**连接前**校验），留后续需求
- **LDAP 真实服务端到端未验**（本期用可测抽象层 + 内置 mock；`ldap3` 的连接与检索代码**未验证**）
- **SCIM 出站的常驻后台 worker 未做**（只提供 `enqueue` / `drain_once`；推送由部署侧调度推进）
- **`/api/org/*` 未做真机 HTTP 冒烟**（路由与错误体有单测，鉴权提取器依赖 IAM 的 access token）
- **「解绑角色」无处可解**（角色表属 REQ-009，本期还不存在）；已解绑**成员关系与岗位**
- **审计落库未接**（sink 属 REQ-014）、**通知通道未接**（属 REQ-015）→ 事件**照常产生**并交给接缝
- **LDAP 入站不自动开号**（开号策略属账号生命周期决策）
- **设计摘要 §1.2 与 D3 自相矛盾**（`org_node.manager_id` vs `user.manager_id`）→ 本实现按 **D3**，**请 BA 确认**
- **多 1 张表 `sync_source_state`**（§9 要求「同步状态的可见性」，D4 的 5 张表里没有承载位置）
- **链接峰值内存 1638 MB**（比 REQ-007 的 1250 MB 高 388 MB；`ldap3` → `nom`），仍在余量内但越过 1.5G 参考线
- **修复轮新增**：`add_member` 传**不存在的 user_id** 现在被数据库拒绝 → 表现为 **500**（服务层没有 404 校验）→ 留后续需求
- **第一轮未做变异验证**（护栏沿用 REQ-006/007 的成熟模式；缺 `DATABASE_URL_TEST` 时确实会失败，已实测）—— **QA 已补做并通过**
