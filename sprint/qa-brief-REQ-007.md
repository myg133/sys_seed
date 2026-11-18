# QA 任务书：REQ-007 认证与账号体系（Pre-merge）

> 用途：下个会话直接把这份作为 QA Agent 的任务书发出去。
> 背景：本轮 QA 跑在调查阶段因机器电量中断、报告未落盘，已取消。REQ-007 的 Dev 成果已提交（`e755e5c`）。

# 背景

- 项目：通用项目底座（Rust + Bun + Svelte 5），Git worktree 多 Agent 协作工作区
- 仓库根：`/home/myg133/Projects/system_seed`
- 被验证 worktree：`/home/myg133/Projects/system_seed/feature-REQ-007`（分支 `feature/REQ-007`，基线 `develop` @ `91d8e09`）
- Dev 自验产物：`feature-meta/verification-report.md`、`feature-meta/traceability.md`、`feature-meta/status.md`

# 角色

你是 QA Agent。规范：`/home/myg133/.agents/skills/agent-workspace/references/roles/qa.md`
（只读、只报告、不改实现代码、不 commit、不 merge、不 push）。

# 需求文档（绝对路径）

- 需求：`/home/myg133/Projects/system_seed/BA/demands/REQ-007-auth-account/demand.md`
- 验收：`/home/myg133/Projects/system_seed/BA/demands/REQ-007-auth-account/acceptance.md`
- 设计摘要（架构决策基线）：`/home/myg133/Projects/system_seed/BA/demands/REQ-007-auth-account/design-summary.md`
- 契约：`/home/myg133/Projects/system_seed/BA/contracts/api-conventions/v1.md`（**尤其 C6 错误体、C8 幂等键**）
- 扩展点契约：`/home/myg133/Projects/system_seed/BA/contracts/extension-points/v1.md`
- REQ-006 成果（租户隔离）：`/home/myg133/Projects/system_seed/BA/demands/REQ-006-tenant/design-summary.md`

# ⚠️ 本需求是第一个安全敏感模块 —— 每项自己复现，不采信 Dev 的数字

## A. 防账号枚举（最高优先级）

Dev 声称：两种情况错误码同为 `INVALID_CREDENTIALS`、响应体逐字段相同；
耗时 有账号+错密码 **248.7ms** vs 无账号 **219.2ms**（比值 1.13，阈值 1.5）。

- 自己跑 **≥20 轮**，取中位数与分位数，确认比值稳定
- 逐字段比对两种情况的响应体（状态码 / 全部 JSON 字段 / 响应头）
- 确认**响应头不泄露信息**（如 `X-User-Exists`、trace_id 长度模式差异）

## B. ★ 账号枚举面（Dev 自陈的未解决取舍）—— 请判定严重度

Dev 说 `ACCOUNT_LOCKED` / `ACCOUNT_DISABLED` 与「账号不存在 → INVALID_CREDENTIALS」
**在错误码上可区分** → 攻击者对候选账号发够 5 次错密码，若返回 `ACCOUNT_LOCKED`
就知道该账号存在 → **账号枚举面**。

请独立验证是否真实存在，然后判定：严重度 / 是否阻塞合并 / 建议修法
（要求：既不泄露账号存在性，又让被锁用户知道被锁 —— 两者兼得）

## C. 令牌轮换与重放

- `refresh_1` → `refresh_2` 成功
- **重放 `refresh_1`** → 必须检测到泄露 → **整个 family 全部作废**（含 `refresh_2`）
- Dev 自陈抓到过一个致命缺陷：「重放检测后 family 作废走了 **rollback** → 检测到泄露却什么都没作废」。
  **确认这个缺陷真的修好了**，且不是靠碰巧 —— 读代码确认作废走的是 **commit**
- access token 不可撤销是有意的设计取舍（15 分钟短 TTL，撤销由 refresh + 会话承担）

## D. TOTP

- 同一时间步的验证码**第二次必须被拒**
- 恢复码**只能用一次**
- `mfa/enroll` 返回的 secret 与 `mfa/verify` 能对上
- TOTP secret 在 DB 里**不是明文**（Dev 说用 chacha20poly1305 静态加密）

## E. 设备会话与撤销

- 会话列表含 deviceLabel / ip / lastUsedAt
- 从 A 端下线 B → **B 的 refresh 立即失效**
- 「登出全端」→ **全部 family 作废**（含 A 自己）
- 改密码后是否撤销其他设备

## F. 租户隔离

- A 租户 token + B 租户请求头 → 拒绝
- A 的 refresh 拿到 B 租户用 → 拒绝
- **直查库**：每条 session 记录都带 `tenant_id`
- 本期新增的 `X-Tenant-Id` 中间件：请求头与令牌绑定的租户不一致时**必须拒绝**

## G. JWKS

- 只暴露**公钥**（逐成员检查无私钥成员 `d`/`p`/`q`/`dp`/`dq`/`qi`/`k`）
- 轮换后新旧两个 `kid` 同时在 JWKS
- 私钥**绝不**出现在 JWKS 响应、配置文件、Git 里（grep 全仓）

## H. 错误体符合契约 C6

逐个错误路径实测，**`detail` 必须英文 ASCII**（`title` 中文允许，契约示例如此）。
Dev 声称单测覆盖 17 个错误路径 —— **自己抽查至少 6 条实际响应**。

## I. ★ 防假绿护栏（承重性验证）

- 不设 `DATABASE_URL_TEST` 跑真实库用例 → 必须非 0 退出、输出**不含**「N passed」
- **自己做一次变异验证**：把用例改成「缺环境变量就早退」→ 护栏应红 → 还原

## J. 依赖预算（Dev 超了预估，请判定是否可接受）

Dev 声称依赖树 179 → **240**（第三方 172 → 232，**+60**），超出 AC-S-020 写的 200~210。
原因：`jsonwebtoken` **10** 的 `rust_crypto` 是**全量 feature**
（ed25519-dalek + p256 + p384 + rsa + hmac + sha2）；9.x 默认后端是 ring（任务禁止）→ 只能整包接受；
另加 `chacha20poly1305`(+14)、sqlx 的 `time` feature(+1)。

- 独立验证：`cargo tree --workspace --all-features | grep -oE "\b(ring|aws-lc-rs|aws-lc-sys) v[0-9][^ ]*"` 是否 0 命中
  （Dev 说唯一含 "ring" 字样的是 stringprep，Unicode 库非加密库 —— 自己确认）
- **自己复测一次冷编译**（`cargo clean` 后）：Dev 声称 check 55.7s/937MB、test+link 59.1s/1250MB
- 判定：240 在这台机器上可接受吗？增量编译是否明显变慢（Dev 说增量 check 1s）？
  有没有更省的替代（argon2 参数可调？chacha20poly1305 是否必要？）

## K. Dev 自陈的 4 个**用例缺口**（实现有但无端到端用例）

请判定严重度，并尽量自己补验（不改代码，只跑）：
1. 锁定期过后自动解锁
2. 成功登录后失败计数清零
3. IP 限流的 429 分支
4. 幂等键端到端重复请求

⚠️ 「实现有但无用例」与「实现缺失」不同 —— 请自己跑一下确认实现是否真的正确。

## L. 回归

- `cargo test --workspace -- --test-threads=2` → 应 225/225（基线 183 全绿）
- `cargo clippy --workspace --all-targets -- -D warnings` → 0 告警
- `cargo fmt --all --check` → 通过
- `cargo xtask capabilities --check` / `check-deps` / `check-modules` / `tenant-policies --check` 全过
- `scripts/test-integration.sh` exit 0（真实库用例真跑，容器 2→2）
- `pnpm -r test` 23/23；`bun run test:e2e` 1/1
- 确认 `crates/platform-core/` 与 `crates/platform-contracts/`（CODEOWNERS 保护目录）改动最小
  （Dev 说只扩展了 platform-db 的 BindValue 与 sqlx 的 `time` feature）

# 环境注意

- `export XDG_RUNTIME_DIR=/run/user/$(id -u)` 是 rootless podman 必需
- 容器已运行且 healthy；本机**没有 psql 客户端**（用 `podman exec base-postgres psql -U base -d <db>`）
- **测试只用 `base_test`**（`postgres://base_app:base@127.0.0.1:5432/base_test`）；**`base_dev` 不要写**
- ⚠️ **应用连接必须用普通角色 `base_app`** —— 用超管 `base` 会被 `require_isolation` 拒绝启动（退出码 3）
- 不要 `cargo install`；不要升级 `@playwright/test`；**不要 `cargo clean`**（除非做冷编译实测，且只做一次）
- 内存紧张：测试用 `--test-threads=2`
- **临时数据必须清理**（base_test 会被集成脚本整体重置）；结束前 `git status` 为空（除报告文件外）

# 产出（唯一交付物）

写 `/home/myg133/Projects/system_seed/feature-REQ-007/feature-meta/qa-report.md`：

```markdown
# 验证报告 —— Pre-merge
- 需求: REQ-007 认证与账号体系
- 分支: feature/REQ-007 @ <sha>
- 结论: PASS / FAIL / PASS-WITH-RISK

## 追溯
| AC | 实现 | 测试 | 结论 |

## 安全断言实测
| 项 | 期望 | 实测 |

## 账号枚举面判定
<严重度 / 是否阻塞 / 建议修法>

## 依赖预算判定
<240 是否可接受 / 冷编译实测 / 更省的替代>

## 用例缺口补验
| 缺口 | 实现是否正确 | 证据 |

## 回归
- Rust X/X，前端 X/X，E2E X/X

## 问题（按严重度）
| # | 级别 | 描述 | 证据 | 建议 |

## 未覆盖 / 无法验证
```

⚠️ **`feature-meta/qa-report.md` 是本次唯一交付物，写完绝对不要删它。**
清理只清自己造的临时物，**不要碰 feature-meta/ 目录**。
写完用 `ls -la feature-meta/qa-report.md` 确认在位再回报。

# 纪律

- 只报告能复现的问题，附命令输出原文
- 无法验证的写「未验证」，不要算通过
- 不采信 Dev 报告里的数字，自己跑、自己计时

# 回报格式

不超过 300 字：整体结论、**账号枚举面的独立验证与严重度判定**、防枚举多轮耗时统计、
轮换重放与 family 作废（含 rollback 缺陷是否真修好）、TOTP 防重放、会话撤销、租户隔离、JWKS、
错误体抽样、依赖 240 的判定与冷编译实测、4 个用例缺口补验结论、问题数、
工作树与 base_test 是否干净、报告路径与字节数。
