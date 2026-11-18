# 验收标准：多租户与隔离

- **需求编号**: REQ-006
- **父需求**: REQ-001
- **状态**: 已就绪
- **设计摘要**: [design-summary.md](design-summary.md)
- **更新时间**: 2026-11-18 12:10

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## 承接的父 AC

父 AC：AC-009（租户上下文全链路传播）、AC-010（跨租户数据绝对不串）

具体步骤见父需求 acceptance.md。

## 本需求自有 AC

### AC-S-007: RLS 兜底生效
- **前置条件**: `base_test` 库中有 tenant_a 与 tenant_b 的同名数据；
  目标表已 `ENABLE ROW LEVEL SECURITY` 且 policy 为 `tenant_id = current_setting('app.tenant_id', true)`
- **测试步骤**:
  1. 用平台内部连接池**直接查询**该表，**不设** `app.tenant_id`
  2. 用 tenant_a 的 `RequestContext` 查询
  3. 用 tenant_a 的 context `find()` tenant_b 的行 id
- **预期结果**:
  1. **0 行**（fail closed —— 不是报错，也不是全表）
  2. 只返回 tenant_a 的行
  3. 返回 `None`
- **类型**: 安全

### AC-S-008: 上下文跨消息边界
- **前置条件**: 平台提供异步消息投递与缓存访问能力
- **测试步骤**:
  1. 以 tenant_a 的上下文投递一条异步消息、写入一个缓存键
  2. 以 tenant_b 的上下文读取缓存、投递消息
  3. 检查消息体与缓存键的租户维度
- **预期结果**: 缓存 key **必带** `tenant_id` 维度；消息体携带租户上下文；
  tenant_b 读不到 tenant_a 的缓存与消息；跨租户污染用例通过
- **类型**: 安全

### AC-S-009: 连接池不串租户
- **前置条件**: 同一个连接池，tenant_a 与 tenant_b 各有数据
- **测试步骤**:
  1. 用 tenant_a 的 context 查询（拿到连接并归还池）
  2. **紧接着**用 tenant_b 的 context 查询（可能拿到同一条物理连接）
  3. 重复 50 次交替查询
- **预期结果**: **每次都只返回本租户的数据**，无任何一次返回上一个租户的行。
  这是 `SET LOCAL`（事务局部）而非 `SET`（会话级）的直接验证
- **类型**: 安全

### AC-S-010: 业务模块拿不到裸连接
- **前置条件**: `crates/platform-db` 已实现
- **测试步骤**:
  1. 审查 `platform-db` 的**公开 API 表面**（`pub fn` / `pub struct` 的公开方法）
  2. 写一段**业务模块视角**的代码，尝试获取一个不带租户上下文的查询入口
- **预期结果**: 公开 API 中**不存在**可绕过租户过滤的入口 —— 拿不到裸连接、
  拿不到不带 `RequestContext` 的查询方法。做不到「编译期就做不到」
- **类型**: 安全 / 可维护性

### AC-S-011: 租户作用域表的 policy 由 manifest 统一生成
- **前置条件**: 某模块的 manifest 声明了 `"tenantScoped": { "tables": [...], "column": "tenant_id" }`
- **测试步骤**:
  1. 跑迁移
  2. 查该表的 `pg_policies` 与 `relrowsecurity`
  3. 手动删掉其中一个表的 policy，再跑一次迁移
- **预期结果**: 声明的表**全部**有 policy 且 `relrowsecurity = true`；
  漏建的能被检出并**报错**（不是静默放过）
- **类型**: 功能 / 安全

### AC-S-012: 租户上下文全链路一致
- **前置条件**: 平台服务已起，tenant_a 与 tenant_b 各有数据
- **测试步骤**:
  1. 携带 `X-Tenant-Id: tenant_a` 请求业务接口
  2. 查看返回数据、DB 查询日志、消息体、缓存键
- **预期结果**: 数据仅来自 tenant_a；DB 查询含 tenant 条件；消息体与缓存键均带 tenant_a；
  **请求头与令牌绑定的租户不一致时返回 404**（不泄露资源存在性）
- **类型**: 功能 / 安全

### AC-S-013: 配额用量计数可靠
- **前置条件**: 租户配额已配置
- **测试步骤**:
  1. 产生若干次用量
  2. 查询用量计数与限额判定
  3. **重启服务**后再查
- **预期结果**: 用量计数**持久化在 DB**（重启不丢）；超限判定正确；
  计数不依赖进程内内存状态
- **类型**: 功能

### AC-S-014: 依赖方向门禁覆盖新 crate
- **前置条件**: 新增了 `crates/platform-db`
- **测试步骤**:
  1. 确认 `cargo xtask check-deps` **自动**把 `platform-db` 纳入平台 crate 集合
  2. 造一次 `platform-db → modules/sandbox` 的反向依赖
  3. 跑 `check-deps`
- **预期结果**: 第 2 步的违规**必须被检出**（非 0 退出）。
  若新 crate 没被自动纳管，等于开了个绕过门禁的口子 —— 这是必须拦住的
- **类型**: 可维护性 / 安全

### AC-S-015: 依赖树与编译资源在预算内
- **前置条件**: 本机 7.7G 内存 / 4.6G 可用
- **测试步骤**:
  1. `cargo tree --workspace` 数唯一 crate 数
  2. `cargo clean` 后实测冷编译时间与峰值内存
- **预期结果**:
  - **未引入** `sqlx` 的 `macros` feature（`sqlx-macros` 不参与编译），
    未引入 `syn` / `quote` / `proc-macro2`
  - 冷编译峰值内存 < 1.5G；若超，`.cargo/config.toml` 的 `jobs` 已从 3 调到 2
  - 日常增量 `cargo check` 仍 < 60 秒
- **类型**: 性能

## 明确不做的（不在本期验收范围）

- L2 独立 schema / L3 独立库的**实现**（只保证不把 L1 的假设写死）
- 跨租户运营后台（运营视角的租户列表、配额管理界面）
- 限流中间件（先只做配额的数据层；限流放 REQ-019）
- 数据库 TLS（随 REQ-006/007 的实际部署场景处理）
