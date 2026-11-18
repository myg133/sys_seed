# 交接说明：Sprint 3 开工中（2026-11-18 19:40）

> 本文件是**当前时点**的交接快照。上一次交接见 [handoff-2026-11-18.md](handoff-2026-11-18.md)（Sprint 2 开工时），
> 那份已被本文件取代（REQ-007 已完成并合并）。

---

## 1. 当前状态一句话

**Sprint 2（身份）已完成并合并；Sprint 3（权限与界面）第一棒 REQ-009（RBAC）已完成并合并**
（`develop` @ `12366e9`）。**下一棒是 REQ-010（配置品牌）—— 规格仍是草稿，需要先写实。**

> 本文件写于 REQ-009 实现过程中，其中的「进行中」状态已被下面的「已完成」覆盖；
> 以 [sprint-current.md](sprint-current.md) 与 [sprint-03-retro.md](sprint-03-retro.md) 为准。

---

## 2. 恢复步骤（断电重启后按序执行）

### ① 恢复 bus watcher（不恢复会**静默失效**：消息无人投递、认领链路断掉）

```bash
AW=/home/myg133/.agents/skills/agent-workspace/scripts/aw.py
export XDG_RUNTIME_DIR=/run/user/$(id -u)
cd /home/myg133/Projects/system_seed
python $AW watch --ensure --interval 5 --wake-dir .aw/wake
```

> ⚠️ watcher **失败是静默的** —— 起来之后确认一次 `aw.py doctor` 里 watcher 那行显示 pid。

### ② 恢复数据库容器（podman 不随机器重启自启）

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
cd /home/myg133/Projects/system_seed/code
podman-compose -f compose.yaml up -d
```

> 容器起不来时：`podman rm -f base-postgres` 后从 `code/` 重起 —— **volume 保留数据**，不要删 volume。

### ③ 确认仓库拓扑

```bash
cd /home/myg133/Projects/system_seed
git worktree list        # 应为 4 个：根 / BA(demand) / code(develop) / Deploy(deploy)
                         # Sprint 3 开工后应为 5 个：+ feature-REQ-009
python $AW doctor        # 一页巡检：workspace / 拓扑 / 认领 / 队列 / 契约
```

---

## 3. 已合并到 `develop`（8 个需求）

| REQ | 名称 | 合并点 |
|---|---|---|
| REQ-002 | 平台骨架与工程基线 | `eacff21` |
| REQ-003 | 扩展点框架与契约测试 | `c8236b0` |
| REQ-004 | AI 引导体系与能力索引 | `37a93c5` |
| REQ-005 | 交付基线与发布 | `806bb66` |
| REQ-006 | 多租户与隔离 | `91d8e09` |
| REQ-007 | 认证与账号体系 | `a0a8d7a` |
| REQ-008 | 用户与组织架构 | `400cefa` |

**`develop` @ `400cefa` 基线**：248 文件，Rust **303/303**（含 ignored **341/341**，8 轮全绿）、前端 23/23、E2E 1/1、clippy 0 告警。

---

## 4. 已完成：REQ-009 RBAC 权限与数据范围（`develop` @ `12366e9`）

| 项 | 值 |
|---|---|
| 认领 | `myg133-ba-1`（BA 兼 Dev），worktree `feature-REQ-009`（**已回收**） |
| 分支 | `feature/REQ-009`（**已回收**），基线 `develop` @ `400cefa` |
| 规格 | `BA/demands/REQ-009-rbac/` —— `acceptance.md`（**16 条 AC：AC-S-033~048**）、`design-summary.md`、`status.md` |
| 用例 | `BA/demands/REQ-009-rbac/test-cases/` —— **61 条**（TC-009-001~061） |
| QA 任务书 | `BA/sprint/qa-brief-REQ-009.md` |
| 契约 | `BA/contracts/extension-points/v2.md`（**新增 C-5**） |

### 本需求最重要的三个决策

1. **兼岗默认按主部门**（`user_org.is_primary`），取并集必须**角色上显式开启**
   （`platform_role.use_all_memberships`）—— 默认取并集会撑大可见范围 = 数据泄漏
2. **主部门缺失或歧义 → fail closed 到 `SELF`**（不是 `ALL`）；
   DB 部分唯一索引是**预防层**，判定层是**防御层**，两层都要
3. **越权两码**：缺权限点 → **403 `INSUFFICIENT_PERMISSION`**；
   超数据范围 → **404 `NOT_FOUND`**（契约 C5）——
   「越权资源」与「不存在资源」必须无法区分

### Dev 已建出的骨架（交接时点）

```text
crates/platform-rbac/            新建平台 crate（9 个源文件 + tests/rbac_real_db.rs）
crates/platform-core/src/extension/permission.rs   PermissionChecker + require_permission 唯一入口
crates/platform-core/src/extension/expr.rs          受限表达式 DSL
crates/platform-core/src/problem.rs                 RFC 9457 错误体
crates/platform-contracts/src/authz_scan.rs         C-5 鉴权调用点扫描器
migrations/20261118000900_rbac_schema.*.sql        角色 / 权限 / epoch 表
migrations/20261118000901_rbac_permission_seed.*.sql
modules/sandbox/backend/src/http.rs                 参考实现：真实鉴权调用点
modules/sandbox/migrations/20261118000900_sandbox_scope_item.*.sql   范围用例夹具表
```

---

## 5. 下一步（按序）

1. **REQ-010（配置、品牌与 Feature Flag）** —— ⚠️ **规格仍是草稿**，必须先写实：
   - BA 写验收标准（**编号用全局唯一段**，REQ-010 既有 `AC-S-015`）
   - **QA 写用例 + 明确要求它挑验收标准的毛病** ← REQ-009 的最大经验（挑出 22 条，5 条硬伤）
   - BA 裁定 → Dev 实现 → QA 验证 → 合并 → 复盘
2. 之后 REQ-011（前端 SDK）→ REQ-012（元数据驱动渲染）

**流程模板**见 [sprint-03-retro.md](sprint-03-retro.md) §7。

---

## 6. 本轮新增的两个既有缺陷（REQ-009 内修）

| 缺陷 | 症状 | 修法 |
|---|---|---|
| **契约门禁在 CI 上常红** | `scripts/sync-contracts.sh` **exit 2** —— `feature-meta/manifest.json` 缺 `contracts` pin，脚本在无 pin 时**故意失败**（防门禁空转） | 加 `"contracts": { "api-conventions": "v1", "extension-points": "v2" }` |
| **roadmap 把 REQ-010 误标「已完成」** | 实际是草稿 | 已修正（提交 `df852e1`） |

---

## 7. 低配机提醒（每轮开工前看一眼）

- 开发机 8 线程 i7-8550U / 7.7G 内存 / **4.7G 可用** / 磁盘剩 114G（**别跑满**）
- **Rust 编译串行**（`.cargo/config.toml` 固定 `jobs = 3`，**不要改**）
- **复用 target 目录**：`export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target`
  —— 依赖已编译过，能省 10~20 分钟冷编译与 ~2.8G 磁盘
- **链接器什么都不要配** —— stable rustc 默认即 LLD，配了反而坏
- **禁止 `cargo install`**（每个编译 10~25 分钟）
- 容器用 rootless podman，**不用 docker**（数据库栈 81MB）
- 镜像必须用 `docker.m.daocloud.io/` 显式前缀（docker.io 直连超时）
- ⚠️ `cargo test` **只能有一个 `--`**（双 `--` → libtest 把 flag 当过滤串 → **0 passed 却 exit 0**，假绿）
- ⚠️ 应用连接**必须用普通角色 `base_app`**（超管会被 `require_isolation` 拒绝启动，退出码 3）
- ⚠️ 本机**没有 psql 客户端** → `podman exec base-postgres psql -U base -d base_test -tAc "..."`
- ⚠️ **测试只用 `base_test`**；**`base_dev` 绝不写**
- 依赖预算：`Cargo.lock` **293 包**（第三方 273）—— **不得增长**
  （不引策略引擎 / 不引 Redis 客户端 / 不用 nom·pest）
