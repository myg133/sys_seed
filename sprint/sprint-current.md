# 当前迭代计划

- **迭代编号**: Sprint 5 —— 身份闭环与出站 IdP
  （✅ REQ-013 ｜ ✅ REQ-014 ｜ ✅ REQ-022 @ `aa5b305` ｜ ✅ **REQ-016 @ `a2b7e52`**）
- **时间范围**: 2026-11-19 09:00 → （进行中；REQ-013 @ `0683115`、
  REQ-014 @ `310e4d3`）
- **父需求**: [REQ-001 通用项目底座](../demands/REQ-001-platform-foundation/demand.md)
- **路线图**: [roadmap.md](../plan/roadmap.md) ｜ 前一轮复盘:
  [req-014-retro.md](req-014-retro.md)、[req-013-retro.md](req-013-retro.md)、
  [sprint-04-retro.md](sprint-04-retro.md)
- **本迭代需求**: REQ-013（SSO 入站）→ REQ-014（审计日志）→ **REQ-022（出站 IdP 与单点登出）**

## 迭代目标

把「**身份**」这条线闭环：REQ-013 让底座能**接住**外部 IdP，
REQ-014 让所有敏感操作**可追溯**，
REQ-022 让底座**自己**当 IdP 签发令牌 —— 三棒串起来，
「应用接入」（REQ-016）才有地基。

本迭代的流程强制项（来自 Sprint 2~4 复盘）：

1. **让 QA 在 Dev 动手前写用例并挑验收标准的毛病** —— 本迭代的流程强制项
2. **修漏洞会制造新漏洞** —— 修复轮后必须有**独立复核**（不由修复者自验）
3. **注释与代码不一致比没有注释更坏** —— 注释里写「也覆盖 X」必须有变异验证支撑
4. **引用契约 / 既有 API 必须打开源码确认**（§9.11）
5. **判据必须与实现同批交付，并附敏感度证据**（§9.13）

## 需求列表

| 需求编号 | 名称 | 优先级 | 状态 | 负责人 | Worktree | 合并点 |
|---------|------|--------|------|--------|----------|--------|
| REQ-013 | SSO 入站（OIDC + LDAP） | P1 | ✅ **已完成** | BA (myg133-ba-1) | 已回收 | `develop` @ `0683115` |
| REQ-014 | 审计日志 | P1 | ✅ **已完成** | BA (myg133-ba-1) | 已回收 | `develop` @ `310e4d3` |
| **REQ-022** | **出站 IdP 与单点登出** | P1 | ✅ **已完成** | BA (myg133-ba-1) | 已回收 | `develop` @ `aa5b305` |
| **REQ-016** | **应用接入与应用注册表** | P1 | ✅ **已完成** | BA (myg133-ba-1) | 已回收 | `develop` @ `a2b7e52` |

**承接父 AC**：AC-005（SSO 多协议）、AC-006（底座作为 IdP）、AC-022（审计完整不可篡改）

**REQ-022 自有 AC**：**AC-S-134 ~ AC-S-155**（**22 条**，全局唯一编号段；
原预留的 AC-S-110~121 不够用且与既有编号段冲突）

## REQ-022 的范围（经用户拍板）

| 项 | 决定 |
|---|---|
| **范围** | 用户 2026-11-20 22:18 裁定：「**全做，但是拆成小任务，如果可以并行，可以让多个子 agent 一块来做。**」→ 草稿 A/B/C/D 四组全做（4 种 grant + introspect + 免登 + 单点登出 + client 模型） |
| **实现策略** | 棒 1（后端 `platform-oauth`）∥ 棒 2（前端同意页 / 设备码页 / 我的授权）**并行子 Agent**，文件集不相交 |
| **规模** | 22 条 AC（本项目最大一轮；REQ-013 是 12 条 / REQ-014 是 13 条） |
| **依赖预算** | **零新增第三方依赖**（`Cargo.lock` 里 `sha2`/`base64`/`jsonwebtoken`/`hyper`/`hyper-util`/`hyper-rustls`/`rustls` **全在**，已逐个核实） |
| **关键取舍** | 底座业务端点**默认拒绝**第三方 OAuth 令牌（D209 / TD-041）—— 数据范围无处继承，宁可不开放 |

## 迭代出口标准

| 出口标准 | 状态 | 证据 |
|---|---|---|
| **REQ-013 全部 12 条 AC** | ✅ | 6 棒验证链（QA 挑刺 51 → Pre-merge FAIL → 修复 → 独立复核 → 修复轮2 → BA 终验） |
| **REQ-014 全部 13 条 AC** | ✅ | 6 棒验证链；12 crate **963 passed**；前端 206/43/38；E2E 干净库 **35 passed / 0 skipped** |
| **REQ-022 全部 22 条 AC** | ✅ | **22/22 通过**，每条指到 `file:line` 的 `assert!`；两轮独立复核（FAIL→修复→PASS-WITH-RISK） |
| **REQ-016 全部 11 条 AC** | ✅ | **11/11 通过**；独立复核 **PASS-WITH-RISK**（11 行判据 **0 行虚构**）+ 4 条 FINDING 全部处置 |
| **Sprint 5 剩余** | ⏳ | **REQ-015 通知系统**（次长关键链最后一环）+ **REQ-018 文件与导入导出** |
| 零新增第三方依赖 | ✅ | `Cargo.lock` 逐包核实（21 个必需包全在） |
| 跨需求改动**全部显式声明** | ✅ | REQ-022 的 4 处跨需求改动已在 demand 依赖段声明（D109） |
| 回归口径**逐 crate 跑** | ✅ | 12 个有测试的 crate，串行 `-- --test-threads=1`（dev-guide §9.8） |

### 本迭代新增的出口标准（写进下一轮的强制项）

| 新标准 | 为什么加 |
|---|---|
| **回归清单要数 workspace 成员，别手抄** | REQ-014 全周期写「11 crate」，漏了 `platform-contracts` 的 32 条 |
| **E2E 必须在干净起点跑 + 修复要附负向对照** | 脏库（42 条）全绿、干净库（11 条）必败 |
| **skip ≠ 已验证** | Dev 报「30 passed / 5 skipped」，那 5 条 skip 正好含审计页关键用例 |

## 环境备注（低配机）

- 开发机 8 线程 i7-8550U / 7.7G 内存 / 磁盘剩 109G
- **Rust 编译串行执行**（`.cargo/config.toml` 固定 `jobs = 3`）
- 复用 target 目录：`export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target`
- 容器用 rootless podman，**不用 docker**；`export XDG_RUNTIME_DIR=/run/user/$(id -u)`
- ⚠️ **必须设 `DATABASE_URL_TEST`**（应用连接用 `base_app`，测试只用 `base_test`）
- ⚠️ **本机无 psql 客户端** → `podman exec base-postgres psql -U base -d base_test -tAc "…"`
- ⚠️ **`cp -p` 保留旧 mtime** → cargo 复用陈旧二进制 → **还原后必须 `touch`**
- ⚠️ `cargo test` **只能有一个 `--`**（双 `--` → 0 passed 假绿）
- ⚠️ 应用连接**必须用普通角色 `base_app`**（超管会被 `require_isolation` 拒绝启动，退出码 3）
- ⚠️ 前端断言**必须断「等于期望值」**，不能只断「不等于默认值」（D83）
- ⚠️ **静态扫描「0 命中」必须带阳性对照**（§9.2）
- **断电后必须手工恢复**：bus watcher + podman 容器（都不会自启）
  → 见 [dev-guide-low-spec.md](../plan/dev-guide-low-spec.md) §8.5
