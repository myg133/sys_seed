# 状态：REQ-008 用户与组织架构

- **状态**: 已完成
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-18 18:55
- **分支**: `develop` @ `400cefa`（已合并）

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-18 12:20 | 草稿 | 已就绪 | myg133-ba-1 | 规格写实（9 条 AC + 设计摘要） |
| 2026-11-18 16:30 | 已就绪 | 进行中 | dev | Dev 交回：44 文件 / +10215 行，`crates/platform-org` + SCIM + LDAP |
| 2026-11-18 17:40 | 进行中 | 待验证 | ba | 派 QA Pre-merge |
| 2026-11-18 18:00 | 待验证 | 已退回 | ba | QA 结论 **PASS-WITH-RISK**：中 2 / 低 3 / 信息 3 |
| 2026-11-18 18:20 | 已退回 | 进行中 | dev | 修复轮：5 项全修 |
| 2026-11-18 18:40 | 进行中 | 待验证 | ba | QA 修复复核 **PASS-WITH-RISK**，5 项复验为真，**新发现回归不稳定**（2/4 死锁） |
| 2026-11-18 18:50 | 待验证 | 进行中 | dev | 死锁修复轮：8 轮回归全绿 |
| 2026-11-18 18:55 | 进行中 | 已完成 | ba | 合并到 develop @ 400cefa，worktree 与分支已回收 |

## 完成记录

| 项 | 结果 |
|---|---|
| 合并 | `develop` @ `400cefa`，`--no-ff` |
| 规模 | 44 + 9 文件 / +10829 -693 行；新建 `crates/platform-org`（平台 crate） |
| 回收 | worktree 与分支已删除；两份 QA 报告随合并保留在 `develop` |
| 回归 | Rust **303/303**（含 ignored **341/341**，**连跑 8 轮全绿**）、前端 23/23、E2E 1/1、clippy 0 告警 |
| 依赖 | 第三方 **263 → 273（+10）**：ldap3 = 8、SCIM 出站 = 2（`want` / `try-lock`）；**ring / aws-lc-rs 0 命中** |

## 交付：组织架构（为 REQ-009 提供数据维度）

| 表 | 说明 |
|---|---|
| `org_node` | 组织节点（company / dept / team）。**无 `manager_id`** —— 汇报关系是人的属性 |
| `user_org` | 用户 ↔ 节点**多对多**（含 `is_primary`） |
| `user_position` | 岗位（挂在部门上的属性，**不是** org_node 的一种） |
| `external_identity` | SSO 身份映射 |
| `sync_conflict` | 同步冲突队列（**字段级**） |
| `sync_source_state` | 同步源状态可见性（`error_count` / `last_error` / `last_sync_at`） |

外键**全部带 `tenant_id`** → 结构上杜绝跨租户引用与悬空成员。

## 设计要点

| # | 决策 |
|---|---|
| D1 | **邻接表 + 递归 CTE** —— 不用 `ltree` 扩展（私有化部署常无扩展权限） |
| D3 | **汇报关系挂 `user.manager_id`**，不挂节点（部门经理 ≠ 我的上级，混在一起兼岗会判错） |
| D5 | `path_labels` 只是**加速字段**，权威是 `parent_id`；不一致时以 `parent_id` 为准并可重建 |
| D6 | **SCIM 2.0 出站自实现**（不用 SCIM 库），复用树里已有的 `hyper` / `hyper-util`；PUT 幂等；**异步推送不阻塞请求** |
| D7 | **LDAP 入站用 `ldap3`**（`default-features = false`，不带 `native-tls`）；属性映射**外置为配置**；离职用**位运算**判定（布尔比较会误判 `660`） |
| D8 | **冲突默认 `manual`** → 进字段级队列，**绝不静默覆盖** |
| D9 | **离职自动禁用**：禁用 + **撤销全部 session** + 解绑 + 审计 + 通知 |
| D11 | **REQ-008 不含任何权限判定逻辑** —— 只提供结构，判定在 REQ-009 |

**交付形态是平台 crate 而非业务模块**，理由：离职禁用必须撤销 IAM 会话（业务模块会依赖平台内部模块，
违反 extension-points v1 D7）；且 `check-deps` 禁止 `platform-server` 依赖业务模块 → `/api/org/*` 挂不上。
租户隔离**不降级** —— 6 张表走 `tenantScoped` 统一生成 policy。

## QA 三轮发现的问题（21 条，全部处置）

### 首轮（PASS-WITH-RISK，中 2 / 低 3 / 信息 3）

| 级别 | 问题 | 处置 |
|---|---|---|
| **中** | **同步错误计数是死代码** —— `upsert_sync_state` 只有成功路径调用点，失败完全不写 | 已修：新增 `record_sync_failure`，`error_count` 递增、写 `last_error`、**`last_sync_at` 不动**（它是增量同步点） |
| **中** | **`user_org.user_id` 无引用完整性** —— 可写幽灵与跨租户 user_id | 已修：补 `UNIQUE (tenant_id, id)` + 复合外键 → **DB 层拒绝** |
| 低 | 依赖归属说明不准（SCIM 出站并非零新增） | 已更正：`want`/`try-lock` 是 hyper 的 http1 客户端引入的 |
| 低 | 计数口径没写清 | 已写 4 种口径 + 逐字命令 |
| 低 | LDAP 源缺前置 URL 校验（错误来自底层 `UnknownScheme`） | 已修：`validate_ldap_url` 连接前校验，三种 scheme 各自明确报错 |
| 信息 | `add_member` 传不存在 user → 500 | 已修 → **404 `NOT_FOUND`**（从真实 FK 违约映射，不是先查一遍） |
| 信息 | 迁移遇存量悬空行会失败 | 刻意暴露脏数据，已登记 |
| 信息 | `/api/org/*` 未做真机 HTTP 冒烟 | 未验，已登记 |
| 信息 | `sync_conflict` 无 `user_id` 列 | 设计如此，已说明 |

### 复核轮（PASS-WITH-RISK）

| 级别 | 问题 | 处置 |
|---|---|---|
| **低** | **回归不稳定** —— 含 ignored 的全套跑 4 次有 2 次 `deadlock detected` | 已修：根因是 **DDL 与 DML 并发**（用例体 `INSERT user_org` 持锁等 `org_node` ↔ 夹具 `ALTER TABLE ... ENABLE RLS` 反向等）→ 库结构准备抽成 `prepare_schema()` + `OnceCell` 只跑一次。**8 轮全绿**。顺带提速：真实库用例 3.10s → **1.47s** |

### 最终微修轮

QA 在复核里补做了 Dev 漏掉的**变异验证**（护栏转红 → 还原 → 复绿）。

## 遗留（不阻塞）

| # | 事项 | 归属 |
|---|------|------|
| 1 | **LDAP 真实服务端到端未验** —— 本机无 LDAP 服务器，只验到「连接前校验」这一层 | 后续需求 / 集成环境 |
| 2 | **SCIM 常驻后台 worker 未做**（只给 `enqueue` / `drain_once`） | 后续需求 |
| 3 | **`/api/org/*` 真机 HTTP 冒烟未做** | REQ-011 前端 SDK |
| 4 | 「解绑角色」无处可解（角色表属 REQ-009） | REQ-009 |
| 5 | 审计落库属 REQ-014、通知通道属 REQ-015（事件照常产生并交接缝） | REQ-014 / REQ-015 |
| 6 | LDAP 入站**不自动开号** | 设计取舍 |
| 7 | 唯一约束违约仍落 500 | 后续需求 |
| 8 | `DbError::MissingTenant.code()` 返回 `TENANT_CONTEXT_MISSING`，但 `platform-org` 匹配的是 `MISSING_TENANT` → 缺租户时落成 500（应为 401） | 后续需求（先于 REQ-008 存在） |
| 9 | **TLS 只支持明文** —— `https://` / `ldaps://` 明确报错（不静默降级） | 后续需求 |
| 10 | 链接峰值 **1763MB**（另一次 1471MB，方差 ±300MB）—— 越过 `cargo check` 的 1.5G 参考线，但链接阶段项目自定预算 1.5–2.5G，`jobs` 管并发 rustc 不压单链接器峰值 → **保持 3** | 已记录 |

## 阻塞项

无 —— 依赖 REQ-002 / REQ-006 / REQ-007 已完成，REQ-008 已合并。
**Sprint 2 三个需求全部完成。**
