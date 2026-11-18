# 状态：REQ-007 认证与账号体系

- **状态**: 已完成
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-18 16:55
- **分支**: `develop` @ `a0a8d7a`（已合并）

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-18 11:40 | 草稿 | 已就绪 | myg133-ba-1 | 规格写实（12 条 AC + 设计摘要） |
| 2026-11-18 13:00 | 已就绪 | 进行中 | dev | Dev 交回：30 文件 / +8114 行，15 个 HTTP 路由 |
| 2026-11-18 14:40 | 进行中 | 待验证 | ba | 派 QA Pre-merge（首轮因电量中断后重跑） |
| 2026-11-18 15:10 | 待验证 | 已退回 | ba | QA 结论 **FAIL**：**2 个 HIGH 阻塞**（账号枚举面 + 锁可无限续期）+ 2 MEDIUM + 3 LOW |
| 2026-11-18 16:20 | 已退回 | 进行中 | dev | 修复轮：10 文件 / +1102 行，6 项全修 |
| 2026-11-18 16:50 | 进行中 | 待验证 | ba | QA 修复复核 **PASS** —— 6 项逐条独立复现为真 |
| 2026-11-18 16:55 | 待验证 | 已完成 | ba | 合并到 develop @ a0a8d7a，worktree 与分支已回收 |

## 完成记录

| 项 | 结果 |
|---|---|
| 合并 | `develop` @ `a0a8d7a`，`--no-ff` |
| 规模 | 30 + 10 文件 / +9216 -929 行；新建 `crates/platform-iam`（9 源文件 + 真实库测试）+ 10 张 IAM 表迁移 |
| 回收 | worktree 与分支已删除；两份 QA 报告随合并保留在 `develop` |
| 回归 | Rust **225/225**（含 ignored **249/249**，真实库 **16/16**）、前端 23/23、E2E 1/1、clippy 0 告警 |
| 依赖 | 175 → **232** 第三方 crate（Cargo.lock 零变更，修复轮未新增依赖） |

## 交付：平台的第一个真实业务路由层

```
POST   /api/iam/register          POST   /api/iam/mfa/enroll
POST   /api/iam/login             POST   /api/iam/mfa/verify
POST   /api/iam/refresh           POST   /api/iam/password/forgot
POST   /api/iam/logout            POST   /api/iam/password/reset
POST   /api/iam/logout-all        POST   /api/iam/password/change
GET    /api/iam/sessions          DELETE /api/iam/sessions/{id}
GET    /.well-known/jwks.json
+ X-Tenant-Id → RequestContext 租户上下文中间件
```

**这顺带闭环了 REQ-006 留下的 AC-S-012**（HTTP 请求头 → 租户上下文）。

## 设计要点：混合令牌

| 令牌 | 形态 | TTL | 存储 | 撤销 |
|---|---|---|---|---|
| access | JWT（自包含，含 `kid`） | 15 分钟 | 不存 | 靠短 TTL 兜底 |
| refresh | 不透明随机串 | 30 天 | DB 存 hash | **可立即撤销** |

**为什么混合**：全 JWT 无法撤销（「设备会话管理」「登出全端失效」直接落空）；
全不透明则每次请求查库。混合式兼顾两者。

**轮换与重放检测**：refresh 每次刷新即作废旧的；同一 family 的 refresh 被重复使用
→ 判定泄露 → **整个 family 立即作废**。

## QA 两轮发现的问题（13 条，全部处置）

### 首轮（FAIL，2 个 HIGH 阻塞）

| 级别 | 问题 | 处置 |
|---|---|---|
| **HIGH** | **`ACCOUNT_LOCKED` 是账号枚举预言机** —— 存在账号第 6 次错密码返 `403 ACCOUNT_LOCKED`、不存在账号永远 `401 INVALID_CREDENTIALS` → **6 请求/候选即可枚举**。根因：锁定判定在密码校验**之前** | 已修：锁定判定挪到**密码校验之后**。攻击者只得 `INVALID_CREDENTIALS`，**知道密码**的被锁用户仍得 `ACCOUNT_LOCKED` —— 两约束兼得 |
| **HIGH** | **账号锁可被无限续期 → 永久登录 DoS** —— 锁定期内每次错密码都把 `locked_until` 重算成 `now+15min`；攻击者每 15 分钟一次错密码即可让受害者永远登不进去 | 已修：已在锁定期 → 不续期、计数不增；锁定已到期 → 失败计数**从头开始** |
| MEDIUM | 锁定路径跳过 Argon2 → **75 倍时序差**（8.1ms vs 606.9ms），比 6 请求枚举更便宜 | 随 HIGH 1 一并修复 → 三路径耗时比值 **1.126** |
| MEDIUM | Dev 的防枚举用例**结构上看不见**枚举面：状态码相等只断言第一次；`median()` 偶数长度取上中位数，8 个样本里 4 个锁定样本被丢掉 → 扩到 8 轮仍报「通过」 | 已修：逐轮断言 + 真中位数 + 拆成两条独立用例；**双向变异验证**（挪回 → 5 条红；永不返回 LOCKED → 2 条红） |
| LOW | 改密把**当前设备**的 refresh family 也作废了 | 已修：改为「除当前 family 外全作废」 |
| LOW | IP 限流按租户计、且把成功登录也计数（NAT 办公网会被误伤） | 已修：按 IP 全局计 + 只算失败尝试 |
| LOW | 依赖超预算 22 | **不改代码**，AC-S-020 预算重定基线到 **235**（不降级依赖，见 D22） |

### 复核轮（PASS，0 阻塞，6 项非阻塞观察）

Dev 回归数写 240 实测 244/249（自报偏低）；`iam_real_db.rs` 有全表断言导致用例偏脆；
403 分支也写限流计数；限流跨租户共享的连带影响未登记；
新增无租户维度的全局表 `platform_iam_login_rate`（只存 IP 的 SHA-256 摘要，不含租户数据、不开 RLS）；
并发计数 lost update 未验证。

## 遗留（不阻塞）

| # | 事项 | 归属 |
|---|------|------|
| 1 | 设计摘要 §6 的「Redis 滑动窗口 + Redis 锁定快计数」**未实现** —— 实现是纯 DB 计数（`platform-iam` 里 `grep -i redis` → 0 命中）。功能等价，但高并发下 DB 计数是瓶颈 | 后续需求补 |
| 2 | 前端 IAM 入口是**占位数据**（`apps/shell/src/lib/nav.ts`，点击落 404）—— 本期不含登录页，不算缺陷，但本期**无 UI 验收面** | REQ-011 前端 SDK |
| 3 | `cargo xtask tenant-policies --check` **对库状态敏感** —— 旧库上会失败（报 `sandbox_tenant_item` 未开 RLS） | 已写进 `docs/dev-environment.md` §4.2：**先跑 `test-integration.sh` 再跑 `--check`** |
| 4 | 「成功登录后失败计数清零」「幂等键端到端重复请求」仍无自动化用例（QA 已独立实测为真） | 补测 |
| 5 | access token 即时撤销本期不做（15 分钟短 TTL 的设计取舍） | 设计取舍 |
| 6 | 租户级注册策略（开关/域名白名单/审批态）与邀请链接 | REQ-010 |
| 7 | WebAuthn / 短信验证码 / 图形验证码 / HIBP 完整泄露库 | 设计摘要 §9 明确不做 |
| 8 | SSO 入站、企业目录同步 | REQ-013 / REQ-008 |

## 阻塞项

无 —— 依赖 REQ-006 已完成，REQ-007 已合并。**下一个：REQ-008 用户与组织架构**
（设计摘要与验收标准已写好，可直接认领开工）。
