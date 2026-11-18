# REQ-008 状态

- **需求**: REQ-008 用户与组织架构
- **分支**: `feature/REQ-008`
- **基线**: `develop` @ `a0a8d7a`（REQ-002 ~ REQ-007 已合并）
- **状态**: **待验证**（本状态由 Dev 按任务要求置位，**最终状态机由 BA 推进**）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18（**第一轮：实现 + 自验完成**）
- **改动**: 44 文件 / +10214 -775 行（新建 `crates/platform-org`）
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`
- **设计依据**: `BA/demands/REQ-008-user-org/design-summary.md`（D1–D12 照做）

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
| 平台 crate 的 manifest 进镜像（否则部署期看不到组织表的租户声明） | `Dockerfile` |

## 自验结论（**实测**，2026-11-18）

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **295/295**（基线 225 → **+70**，只增不减）
- 带 `--include-ignored` ✅ **329/329**（真实库层 **11/11**，SCIM **7/7**）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** / `cargo fmt --all --check` ✅
- `cargo xtask capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅
- `bash scripts/test-integration.sh` ✅ **exit 0**（容器 **2 → 2**）
- `pnpm -r test` ✅ **23/23** / `bun run test:e2e` ✅ **1/1**
- ★ **组织树**：移动 → 子树**立即**反映新结构；`path_labels` 与 `parent_id` 一致；写坏 → **报得出** → **可重建**；成员关系**不破坏**
- ★ **SCIM 出站**：**真的 HTTP** 打 mock 服务 → PUT 两次 **1 条**；两次全量同步 **条目数不变**；字段映射逐字段生效；目标延迟 800ms → **本地入队 < 100ms**
- ★ **LDAP 位运算**：`512`/`660`/`402653184` 在职，`514`/`546`/`4098`/`2` 离职（布尔比较会误判 `660`）
- ★ **冲突队列**：**字段级**；`manual` 下**本地不被覆盖**；裁决后**按裁决生效**
- ★ **离职禁用**：账号禁用 + **旧 refresh 立即作废**（直查库 `revoked_at` 非空）+ 会话全撤销 + 成员解绑 + 审计 + 通知
- ★ **租户隔离**：跨租户 id → `None`；**绕过应用层 → 0 行**；**直查 `pg_class`** → 6 张表**全部** `relrowsecurity=true` 且有 policy
- **冷编译实测**：`cargo check` **52.7s / 1038 MB**；`cargo test --no-run`（含链接）**83.4s / 1638 MB**（`jobs` 保持 3）
- **依赖树**：第三方 **263 → 273**（+10，全是 `ldap3` 的传递依赖）；**ring / aws-lc-rs 0 命中**；SCIM 出站**增量 0**

## 未完成 / 未验证（如实登记）

- **只支持明文**：SCIM `http://` / LDAP `ldap://` —— 树里无 TLS 栈（`ring` / `aws-lc-rs` 预算要求 0 命中）；HTTPS / LDAPS **明确报错**，留后续需求
- **LDAP 真实服务端到端未验**（本期用可测抽象层 + 内置 mock；`ldap3` 的连接与检索代码**未验证**）
- **SCIM 出站的常驻后台 worker 未做**（只提供 `enqueue` / `drain_once`；推送由部署侧调度推进）
- **`/api/org/*` 未做真机 HTTP 冒烟**（路由与错误体有单测，鉴权提取器依赖 IAM 的 access token）
- **「解绑角色」无处可解**（角色表属 REQ-009，本期还不存在）；已解绑**成员关系与岗位**
- **审计落库未接**（sink 属 REQ-014）、**通知通道未接**（属 REQ-015）→ 事件**照常产生**并交给接缝
- **LDAP 入站不自动开号**（开号策略属账号生命周期决策）
- **设计摘要 §1.2 与 D3 自相矛盾**（`org_node.manager_id` vs `user.manager_id`）→ 本实现按 **D3**，**请 BA 确认**
- **多 1 张表 `sync_source_state`**（§9 要求「同步状态的可见性」，D4 的 5 张表里没有承载位置）
- **链接峰值内存 1638 MB**（比 REQ-007 的 1250 MB 高 388 MB；`ldap3` → `nom`），仍在余量内但越过 1.5G 参考线
- **本轮未做变异验证**（护栏沿用 REQ-006/007 的成熟模式；缺 `DATABASE_URL_TEST` 时确实会失败，已实测）—— 建议 QA 复核时补一次
