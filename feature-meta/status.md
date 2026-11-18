# REQ-007 状态

- **需求**: REQ-007 认证与账号体系
- **分支**: `feature/REQ-007`
- **基线**: `develop` @ `91d8e09`（REQ-002 + REQ-003 + REQ-004 + REQ-005 + REQ-006 已合并）
- **状态**: **待验证**（本状态由 Dev 按任务要求置位，**最终状态机由 BA 推进**）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18（**第二轮：QA Pre-merge FAIL → 修复轮完成**）
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`
- **设计依据**: `BA/demands/REQ-007-auth-account/design-summary.md`（D1–D11 照做）

## ★ 第二轮（修复轮）：QA Pre-merge 判定 FAIL 后的处理

QA 报告：`feature-meta/qa-report.md`（**FAIL**：2 HIGH + 2 MEDIUM + 3 LOW）。
**核心实现全部复验为真**；问题集中在**账号锁定逻辑的判定顺序**。
本轮修 **HIGH×2 + MEDIUM×2 + LOW×2**；**LOW 7（依赖预算）不改代码**（由 BA 重定基线到 235）。

| # | 级别 | 问题 | 修法 | 状态 |
|---|------|------|------|------|
| 1 | **HIGH** | `ACCOUNT_LOCKED` 是账号枚举预言机（**6 请求/候选**即可枚举） | 锁定判定挪到**密码校验之后**；失败分支统一 `INVALID_CREDENTIALS` + 401 | ✅ |
| 2 | **HIGH** | 账号锁可被**无限续期** → 永久登录 DoS | 锁定期内**不续期**、计数不增；到期后**从头开始** | ✅ |
| 3 | **MEDIUM** | 锁定路径跳过 Argon2 → **75 倍**时序差 | 随 #1 一并解决 | ✅ |
| 4 | **MEDIUM** | 防枚举用例**结构上看不见** #1 | 逐轮断言状态码 + **真中位数** + 拆成两条独立用例 | ✅ |
| 5 | **LOW** | 改密把**当前设备**的 refresh family 也作废了 | 改为「除当前 family 外」全作废 | ✅ |
| 6 | **LOW** | IP 限流按租户计 + 把成功登录也计数 | 限流**按 IP 全局（跨租户）** + **只算失败**（新增全局失败计数表） | ✅ |
| 7 | **LOW** | 依赖树超预算 22 个 crate | **不改代码**（QA：资源侧全达标 + 无可行更省替代）→ BA 重定基线到 235 | ⏭ **BA 记账** |

**修复轮的实测证据**（详见验证报告 §0.1）：枚举面**逐次**不可区分 · 三路径耗时比 **1.048** ·
锁不续期 · 改密保留当前设备 · 限流跨租户共享 · **两次变异验证**证明新用例承重。

## 本轮交付

账号体系**自持**（ADR-0001 A3）：底座是身份的唯一权威源。本期落地**本地账号 + 混合令牌 + Argon2id + TOTP**，
并顺带引入平台**第一个真实的 HTTP 业务路由**（`/api/iam/*`）与**租户上下文中间件**
（`X-Tenant-Id` → `RequestContext`）—— 闭环 REQ-006 留下的 AC-S-012。

| 交付项 | 位置 |
|--------|------|
| 新建 `crates/platform-iam`（平台 crate）：密码 / JWT / TOTP / 令牌 / 会话 / 幂等 | `crates/platform-iam/src/` |
| 混合令牌：access = **JWT**（15 分钟，带 `kid`）；refresh = **不透明串**（30 天，只存 hash） | `crates/platform-iam/src/{jwt,service}.rs` |
| **refresh 轮换 + 重放检测**：旧令牌立即作废；重放 → **整个 family 作废** | `crates/platform-iam/src/service.rs`（`refresh`，单事务 + `FOR UPDATE`） |
| 密码：Argon2id + **PHC 标准串** + 可配策略 + 本地常见口令词库 | `crates/platform-iam/src/{password,config}.rs` |
| TOTP MFA：绑定 / 校验 / **防重放** / 10 个一次性恢复码；**WebAuthn 只留接口** | `crates/platform-iam/src/totp.rs` |
| 设备会话：列表 / 踢下线 / 登出全端（**每个会话一个 refresh family**） | `crates/platform-iam/src/{service,store}.rs` |
| **全部会话与令牌绑定 `tenant_id`**；改密码撤销全部 family | `crates/platform-iam/src/store.rs`（9 张表全带 `tenant_id`） |
| **防账号枚举**：账号不存在与密码错误**响应体完全相同** + **耗时接近**（假哈希 + 同等数据库工作量）；**锁定判定排在密码校验之后**（`ACCOUNT_LOCKED` 不是枚举预言机） | `crates/platform-iam/src/service.rs`（`login`） |
| **防重置枚举**：已注册与未注册邮箱**响应完全相同** | `crates/platform-iam/src/http.rs`（`forgot_password`） |
| 登录限流（**按 IP 全局、跨租户、只算失败**）+ 账号锁定（DB 持久；**锁定期不续期**） | `crates/platform-iam/src/service.rs`、`crates/platform-iam/src/store.rs` |
| 统一错误体（契约 C6）：RFC 9457 + **英文 ASCII `detail`** + `params` 指出违反的规则 | `crates/platform-iam/src/{error,http}.rs` |
| 幂等键（契约 C8）：写操作支持 `Idempotency-Key`，按租户维度存储 | `crates/platform-iam/src/http.rs`（中间件） |
| **平台迁移新增 10 张 IAM 表**（9 张带 `tenant_id` + RLS + `WITH CHECK`；1 张**全局失败计数**表 `platform_iam_login_rate` —— **无租户维度、无租户数据**，只存 IP 的 SHA-256 摘要） | `migrations/20261118000400_iam_auth.{up,down}.sql`、`migrations/20261118000500_iam_login_rate.{up,down}.sql` |
| **HTTP 路由清单**：12 个 `/api/iam/*` + `/.well-known/jwks.json`；挂到服务上 | `crates/platform-iam/src/http.rs`、`crates/platform-server/src/main.rs` |
| 密钥只从环境注入 + 占位符密钥拒绝启动；`kid` 轮换宽限期；JWKS 只暴露公钥 | `crates/platform-iam/src/{config,jwt}.rs` |
| 能力索引 / 文档 / 依赖门禁收录 `platform-iam`（`iam_auth` 能力） | `crates/xtask/src/catalog.rs`、`AGENTS.md`、`capabilities/index.json`、`CHANGELOG.md`、`.env.example` |

## 自验结论（**修复轮实测，2026-11-18**）

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **225/225**（与第一轮**持平**，REQ-006 基线 **183 个全绿**，**只增不减**）
- 带 `--include-ignored` ✅ **240/240**（真实库层 **16/16**，第一轮 11 条 → 修复轮 **+5** 条）
- `bash scripts/test-integration.sh` ✅ **exit 0**（重置 `base_test`，容器数量 2 → 2）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** / `cargo fmt --all --check` ✅
- `cargo xtask capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅
- `pnpm -r test` ✅ **23/23** / `bun run test:e2e` ✅ **1/1**
- ★ **枚举面**（真机 HTTP 实测）✅ 存在账号 / 不存在账号**各 6 次错密码 → 逐次全部 401 `INVALID_CREDENTIALS`，两条序列完全相等**
- ★ **锁定期内用正确密码** ✅ **403 `ACCOUNT_LOCKED`**（只有知道密码的人拿得到 → 不是枚举信号）
- ★ **三路径耗时** ✅ 错密码(未锁) **563.1ms** / 错密码(已锁) **569.0ms** / 账号不存在 **543.2ms** → 最差比值 **1.048**
- ★ **锁不续期** ✅ 锁定期内错密码 → `locked_until` **不前移**、计数不增；到期后第一次错密码 → 计数**从头开始**（= 1）
- ★ **改密保留当前设备** ✅ 其它设备 refresh → **401 `TOKEN_REVOKED`**；当前设备 refresh → **200**
- ★ **IP 限流** ✅ 跨租户同 IP → **429 `LOGIN_RATE_LIMITED`**；20 次**成功**登录**不计入**失败窗口
- ★ **新用例能抓回归** ✅ 变异①（锁定判定挪回密码校验前）→ **3 条变红**；变异②（过度修正，不再返回 `ACCOUNT_LOCKED`）→ **1 条变红**；两次**均已还原**（sha256 一致）
- **防重置枚举** ✅ 两个响应体 **JSON 逐字段相等**
- **轮换与重放** ✅ 旧 refresh 立即失效；重放 → `TOKEN_REUSE_DETECTED` → **整个 family 作废**（refresh_2 也失效）
- **TOTP 防重放** ✅ 同一时间步第二次 → `MFA_INVALID`；恢复码第二次 → `MFA_INVALID`
- **设备会话** ✅ 从 A 下线 B → B 的 refresh **立即失效**；登出全端 → 全部 family 作废
- **租户隔离** ✅ A 的令牌 + B 的租户头 → **404**；会话记录**全部带 `tenant_id`**
- **JWKS** ✅ **只暴露公钥**（逐成员检查无私钥材料）；轮换后新旧 `kid` 都在
- **错误体合规** ✅ 逐个错误路径的 `detail` 都是**英文 ASCII**
- **防假绿护栏** ✅ 缺 `DATABASE_URL_TEST` → 子进程**非零退出**且输出含 `0 passed`（**护栏经变异验证承重**）
- **真机冒烟**：真的起 `base` 二进制跑 HTTP —— 注册 201 / 登录 200 / 会话列表 / 刷新轮换 /
  **重放 → 401 TOKEN_REUSE_DETECTED** / 跨租户 **404** / JWKS 只有公钥；占位符密钥 **EXIT=2 fail-fast**
- **冷编译实测**（第一轮）：`cargo check` **55.7s / 937 MB**；`cargo test --no-run`（含链接）**59.1s / 1250 MB** → 峰值 **< 1.5G**，`.cargo/config.toml` 的 `jobs` **保持 3**
- **依赖树**：第三方 **232**（总数口径 240）—— **修复轮未变**（`git diff -- '*Cargo.toml' '*Cargo.lock'` 为空）；
  **超出 AC-S-020 预算 22 个** → **不改代码**，BA 重定基线到 **235**（原因与影响见验证报告 §5）

## 未完成 / 未验证（如实登记）

- **「成功登录后失败计数清零」**、**幂等键的端到端重复请求** —— 实现都在，但**没有**端到端自动化用例
  （用例缺口，不是实现缺口；QA 已**独立实测**为真）
- **access token 的即时撤销** —— 本期**不做**（15 分钟短 TTL 的设计取舍，撤销由 refresh + 会话承担）
- **租户级注册策略**（开关 / 域名白名单 / 审批态）与**邀请链接** —— 属 REQ-010
- **WebAuthn / FIDO2 实现**、**短信验证码**、**图形验证码**、**完整 HIBP 泄露库接入** —— 设计摘要 §9「不做」
- **SSO 入站**、**企业目录同步** —— 属 REQ-013 / REQ-008
- **依赖树超出预算**（232 > 210）—— **不改代码**，BA 重定基线；原因见验证报告 §5
- **限流的多副本一致性** —— 单机单实例实测（限流是 DB 计数，多实例共享 DB 理论一致）
- **QA 附：观察 ① Redis 滑动窗口未实现** —— 实现是纯 DB 计数（功能等价，高并发下是瓶颈）→ **建议后续需求补**
- **QA 附：观察 ② 前端没有 IAM 接入面** —— 本期不含前端登录页，**不算缺陷**，但本期**没有 UI 验收面**
- **QA 附：观察 ③ `tenant-policies --check` 对库状态敏感** —— 已写进 `docs/dev-environment.md` §4.2：
  「**先跑 `scripts/test-integration.sh` 再跑 `tenant-policies --check`**」

## 改动了 `platform-core`（CODEOWNERS 门禁范围）

- **零改动** —— `crates/platform-core/` 与 `crates/platform-contracts/` **一行未改**
- 扩展的是 `crates/platform-db`（REQ-006 新建、**不在** CODEOWNERS 保护目录）：
  `BindValue` 新增 `Timestamp` / `NullableText` / `NullableTimestamp`（`sqlx` 打开 `time` feature），
  供令牌过期 / 重置令牌过期 / 账号锁定到期写入时间戳
