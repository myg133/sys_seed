# REQ-009 状态

- **需求**: REQ-009 RBAC 权限与数据范围
- **分支**: `feature/REQ-009`
- **基线**: `develop` @ `400cefa`（REQ-002 ~ REQ-008 已合并）
- **状态**: **待验证**（本状态由 Dev 按任务要求置位，**最终状态机由 BA 推进**）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18（**第一轮：实现 + 自验完成** —— 含破坏性验证 4 类 + 变异验证 4 项）
- **改动**: 60 文件 / +11653 −675 行（新建 `crates/platform-rbac`；新建 `platform-core` 的 `extension/permission.rs`、`extension/expr.rs`、`problem.rs`；契约 C-5；平台迁移 2 份 + 模块迁移 1 份）
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`
- **设计依据**: `BA/demands/REQ-009-rbac/design-summary.md`（照做）；验收基线 `acceptance.md`（2026-11-18 21:40 修订版，含 BA 第二轮 10 条裁定）

## 交付形态

| 项 | 结论 |
|----|------|
| **鉴权入口位置** | **`crates/platform-core/src/extension/permission.rs`**（**不是** `platform-rbac`）—— 业务模块的运行期依赖仍然只有 `platform-core` + `platform-db`（AGENTS.md 硬规则 2）。判定实现由平台通过**请求扩展**注入（`Arc<dyn PermissionChecker>`，方法全同步 → dyn 兼容） |
| **判定实现位置** | **`crates/platform-rbac`**（平台 crate），与 `platform-org` 同级；`platform-server` 挂载 `/api/rbac/*` 并在**启动时**做权限点注册表对账 |
| **依赖方向** | `platform-core ← platform-db ← platform-iam ← platform-rbac ← platform-server`；业务模块**不依赖** `platform-rbac` |
| **契约 C-5** | 新增于 `extension-points v2`；契约测试宏从 **4 项变 5 项**；C-5 扫的是**调用**（不是字面量）—— 沿用 C-2 的逻辑会**假绿** |

## 本轮交付

| 交付项 | 位置 |
|--------|------|
| 角色模型（角色 / 授权 / 继承 / 绑定 / 内置角色）+ 通配语义 | `crates/platform-rbac/src/{entity,service,store}.rs` |
| 数据范围 5 种语义 + 兼岗默认主部门 + 主部门缺失 / 歧义 fail closed 到 `SELF` | `crates/platform-rbac/src/scope.rs` |
| 受限表达式 DSL（白名单 + 参数化 + 禁函数调用 + 禁多语句） | `crates/platform-core/src/extension/expr.rs` |
| 鉴权入口（**唯一**）+ 三种拒绝语义（401 / 403 / 404） | `crates/platform-core/src/extension/permission.rs` |
| RFC 9457 错误体（**唯一一份**） | `crates/platform-core/src/problem.rs` |
| 缓存 + **数据库 epoch 主动失效**（撤销在下一个请求即生效） | `crates/platform-rbac/src/{checker,store}.rs` |
| 权限点注册表**双向对账**（不一致 → 启动失败） | `crates/platform-rbac/src/registry.rs`、`crates/platform-server/src/main.rs` |
| `/api/rbac/*`（**每个路由绑定管理权限点**；注册表查询豁免） | `crates/platform-rbac/src/http.rs`、`crates/platform-rbac/manifest.json` |
| 契约检查 **C-5**（已登记权限点必须有鉴权调用点 + 入口全仓库唯一） | `crates/platform-contracts/src/authz_scan.rs`、`checks.rs`、宏 |
| 平台迁移（5 张 RBAC 表 + 权限点种子 + `user_org_one_primary_idx`） | `migrations/20261118000900_*`、`20261118000901_*` |
| 参考实现（模块侧） | `modules/sandbox/backend/src/http.rs`、`scope_item.rs`、`sandbox_scope_item` 迁移 |
| 基建：契约 pin + v2 快照 + `AGENTS.md` + 脚手架模板 + 能力索引 | `feature-meta/manifest.json`、`docs/contracts/`、`AGENTS.md`、`crates/xtask/src/scaffold.rs`、`capabilities/` |

## 自验结论（**实测**，2026-11-18）

- `bash scripts/test-integration.sh` ✅ **exit 0** → **439 passed / 0 failed**（含真实库层 25 条）；**连跑 2 次全绿**
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** / `cargo fmt --all --check` ✅
- `cargo xtask capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `scripts/sync-contracts.sh` ✅ **exit 0**
- ★ **破坏性验证**：C-5 的 **4 类违规**（登记无调用点 / 自建入口 / 非字面量参数 / 第二个入口定义）逐个造出来 → **全部被拦住** → **逐字节还原**（sha256 校验）
- ★ **变异验证**：**4 个关键判定**故意改错（fail closed→fail open、默认主部门→并集、鉴权静默放行、对账只告警）→ 对应测试**变红** → 还原 → **变绿**
  - ⚠️ 其中「对账只告警」**第一次没变红** → 补断言（`require_consistent` 必须 Err）→ 再验变红 —— **发现的是测试的漏洞**，已补
- ★ **依赖预算**：锁文件里 **registry 包 284 → 284 不变**（总数 293 → 294，多出的 1 个是本地工作区成员 `platform-rbac`）

## 未完成 / 未验证（如实登记）

- **真机 `base serve` 的启动对账未跑**（已验证对账函数与启动路径接线；**没有**真的起进程注入差异看退出码）
- **多实例真机部署下的 epoch 收敛未跑**（用**两个独立 checker 实例**模拟，共享同一个数据库）
- **`platform-org` 的 9 个权限点「登记了但从不鉴权」** —— 按 BA 指示**不顺手改**（REQ-008 范围），已如实登记在 `traceability.md` §4
- **C-5 未扩到平台 crate**（BA 说本期不要求）→ 平台 crate 的权限点目前**没有机械门禁**
- **CI 真跑不了**（本仓**无 remote**）；前端 `can()` 属 REQ-011
- **设计摘要 §4.4 的 `CUSTOM` 组合口径有歧义**（照规格实现，疑问见 `verification-report.md` §7 第 1 条）
