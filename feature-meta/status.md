# REQ-007 状态

- **需求**: REQ-007 认证与账号体系
- **分支**: `feature/REQ-007`
- **基线**: `develop` @ `91d8e09`（REQ-002 + REQ-003 + REQ-004 + REQ-005 + REQ-006 已合并）
- **状态**: **待验证**（本状态由 Dev 按任务要求置位，**最终状态机由 BA 推进**）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`
- **设计依据**: `BA/demands/REQ-007-auth-account/design-summary.md`（D1–D11 照做）

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
| **防账号枚举**：账号不存在与密码错误**响应体完全相同** + **耗时接近**（假哈希 + 同等数据库工作量） | `crates/platform-iam/src/service.rs`（`login`） |
| **防重置枚举**：已注册与未注册邮箱**响应完全相同** | `crates/platform-iam/src/http.rs`（`forgot_password`） |
| 登录限流（按 IP 滑动窗口）+ 账号锁定（DB 持久） | `crates/platform-iam/src/service.rs` |
| 统一错误体（契约 C6）：RFC 9457 + **英文 ASCII `detail`** + `params` 指出违反的规则 | `crates/platform-iam/src/{error,http}.rs` |
| 幂等键（契约 C8）：写操作支持 `Idempotency-Key`，按租户维度存储 | `crates/platform-iam/src/http.rs`（中间件） |
| **平台迁移新增 9 张 IAM 表**（全带 `tenant_id` + RLS + `WITH CHECK`） | `migrations/20261118000400_iam_auth.{up,down}.sql` |
| **HTTP 路由清单**：12 个 `/api/iam/*` + `/.well-known/jwks.json`；挂到服务上 | `crates/platform-iam/src/http.rs`、`crates/platform-server/src/main.rs` |
| 密钥只从环境注入 + 占位符密钥拒绝启动；`kid` 轮换宽限期；JWKS 只暴露公钥 | `crates/platform-iam/src/{config,jwt}.rs` |
| 能力索引 / 文档 / 依赖门禁收录 `platform-iam`（`iam_auth` 能力） | `crates/xtask/src/catalog.rs`、`AGENTS.md`、`capabilities/index.json`、`CHANGELOG.md`、`.env.example` |

## 自验结论

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **225/225**（REQ-006 基线 **183 个全绿**，**+42 只增不减**）
- 真实库用例（默认 `#[ignore]`，由集成脚本 `--include-ignored` 跑到）✅ **11/11**
- `bash scripts/test-integration.sh` ✅ **exit 0**（重置 `base_test`，容器数量 2 → 2）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** / `cargo fmt --all --check` ✅
- `cargo xtask capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅
- `pnpm -r test` ✅ **23/23** / `bun run test:e2e` ✅ **1/1**
- **防账号枚举** ✅ 两种情况错误码与响应体**逐字段相同**；耗时 **248.7ms vs 219.2ms（比值 1.13）**
- **防重置枚举** ✅ 两个响应体 **JSON 逐字段相等**
- **轮换与重放** ✅ 旧 refresh 立即失效；重放 → `TOKEN_REUSE_DETECTED` → **整个 family 作废**（refresh_2 也失效）
- **TOTP 防重放** ✅ 同一时间步第二次 → `MFA_INVALID`；恢复码第二次 → `MFA_INVALID`
- **设备会话** ✅ 从 A 下线 B → B 的 refresh **立即失效**；登出全端 → 全部 family 作废
- **租户隔离** ✅ A 的令牌 + B 的租户头 → **404**；会话记录**全部带 `tenant_id`**
- **JWKS** ✅ **只暴露公钥**（逐成员检查无私钥材料）；轮换后新旧 `kid` 都在
- **错误体合规** ✅ 逐个错误路径的 `detail` 都是**英文 ASCII**
- **防假绿护栏** ✅ 缺 `DATABASE_URL_TEST` → 子进程**非零退出**且输出含 `0 passed`（**护栏经变异验证承重**）
- **冷编译实测**：见验证报告 §4.2 —— 峰值 **< 1.5G**，`.cargo/config.toml` 的 `jobs` **保持 3**
- **依赖树**：179 → **226**（**超出预估的 215**，原因与影响见验证报告 §5 偏离清单）

## 未完成 / 未验证（如实登记）

- **锁定期过后自动解锁**、**成功登录后失败计数清零**、**按 IP 限流的 429 分支**、
  **幂等键的端到端重复请求** —— 实现都在，但**没有**端到端自动化用例（用例缺口，不是实现缺口）
- **access token 的即时撤销** —— 本期**不做**（15 分钟短 TTL 的设计取舍，撤销由 refresh + 会话承担）
- **租户级注册策略**（开关 / 域名白名单 / 审批态）与**邀请链接** —— 属 REQ-010
- **WebAuthn / FIDO2 实现**、**短信验证码**、**图形验证码**、**完整 HIBP 泄露库接入** —— 设计摘要 §9「不做」
- **SSO 入站**、**企业目录同步** —— 属 REQ-013 / REQ-008
- **依赖树超出预算**（226 > 215）—— 原因见验证报告 §5

## 改动了 `platform-core`（CODEOWNERS 门禁范围）

- **零改动** —— `crates/platform-core/` 与 `crates/platform-contracts/` **一行未改**
- 扩展的是 `crates/platform-db`（REQ-006 新建、**不在** CODEOWNERS 保护目录）：
  `BindValue` 新增 `Timestamp` / `NullableText` / `NullableTimestamp`（`sqlx` 打开 `time` feature），
  供令牌过期 / 重置令牌过期 / 账号锁定到期写入时间戳
