# 设计摘要：多租户与隔离（REQ-006）

- **需求编号**: REQ-006
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **架构决策**: [ADR-0001 D2（租户隔离级别）、D7（依赖方向）](../../decisions/ADR-0001-底座技术架构基线.md)
- **接口契约**: [api-conventions v1（尤其 C3 上下文请求头）](../../contracts/api-conventions/v1.md)
- **扩展点契约**: [extension-points v1（尤其 C-3 租户隔离）](../../contracts/extension-points/v1.md)
- **性质**: 需求细化附件 —— 数据访问层的架构决策在此定死
- **更新时间**: 2026-11-18 11:20

---

## 0. 一句话

**租户隔离要做双保险**：应用层每次查询都带租户条件，数据库层用 RLS 再兜一遍；
两处任一失效都不至于跨租户泄漏。

---

## 1. 数据访问层放在哪 —— 新建 `crates/platform-db`

**不动 `platform-core`**（它受 CODEOWNERS 保护，是「核心变更需人工评审」门禁的守门人）。

| crate | 内容 | 谁依赖它 |
|-------|------|---------|
| `crates/platform-core` | 扩展点 trait、`RequestContext`、错误类型 | 平台各 crate、业务模块 |
| **`crates/platform-db`** ← **本期新建** | 租户感知的数据访问、RLS 配合、迁移辅助 | 业务模块、平台服务 |
| `crates/platform-server` | 二进制（健康检查、静态资源、迁移 CLI） | 部署产物 |

依赖方向仍然是单向：

```text
platform-core  ←  platform-db  ←  业务模块
```

`check-deps` 的「平台 crate 未反向依赖业务模块」门禁继续生效 —— 新增 `platform-db`
后它也必须被纳入平台 crate 集合（REQ-005 已把集合改成「显式清单 ∪ crates/ 下工作区成员」，
新建 crate 后要确认**自动纳管**生效，否则等于开了个绕过门禁的口子）。

---

## 2. sqlx 的用法 —— 运行时检查查询，**不开 macros**

**决定**：`sqlx` 的 `macros` feature **关闭**，用 `query_as` + `FromRow`（运行时检查）。

**理由**

| | 开 macros | 不开 macros（推荐） |
|---|---|---|
| 依赖 | 拉 `syn` / `quote` / `proc-macro2`，**编译慢、占内存** | 依赖更小 |
| 编译期需活库 | ✅ 需要（每次编译都要 PG 在） | ❌ 不需要 |
| SQL 校验 | 编译期发现拼写错误 | 运行时才报错 → 用集成测试兜 |
| 多方言 | 每条 SQL 绑定单一方言 | 更容易跨库（L2/L3 或将来 MySQL） |

**本机约束**：7.7G 内存、`cargo check` 峰值已到 1087MB，再加 `syn`/`quote`/`proc-macro2`
会明显拖慢日常循环。**且 REQ-005 已经把 sqlx 引进来了**（依赖树 91 → 175 crate），
本期要复用它而不是再叠加一层宏工具链。

**代价与兜底**：失去编译期 SQL 校验 → 由**集成测试 + 契约测试 C-3** 兜住
（真实库跑多租户数据集，SQL 写错立刻红）。

---

## 3. 租户隔离的实现：双保险

### 3.1 应用层 —— 强制走租户作用域

平台只暴露**租户作用域**的数据访问入口，**不向业务模块暴露未加租户条件的连接**。

```rust
// 语义要求（具体 API 由 Dev 设计，但下列语义不可变）
// 1. 每次访问都在事务内执行 SET LOCAL app.tenant_id = $1
// 2. 业务模块拿不到"没带租户上下文"的查询入口
// 3. 没带 RequestContext 的查询入口不存在 —— 编译期就做不到
```

**关键细节：`SET LOCAL` 是事务局部的。**

- 用 `SET LOCAL`（不是 `SET`）→ 设置在事务结束时自动清除 → **连接池安全**
- 若用普通 `SET`，连接归还池后仍带着上一个租户的值 → **下一个租户会串**
  这类 bug 极难复现，必须一开始就避开

**这是本设计里最容易做错、后果最严重的一处。**

### 3.2 数据库层 —— PostgreSQL RLS 兜底

```sql
-- 每个租户作用域表都要建
ALTER TABLE xxx ENABLE ROW LEVEL SECURITY;
CREATE POLICY xxx_tenant_isolation ON xxx
  USING (tenant_id = current_setting('app.tenant_id', true));
```

**为什么是 `current_setting('app.tenant_id', true)`**：

- `true` = 缺失时返回 NULL 而不是报错 → 应用层漏设时行不可见（**fail closed**，不泄漏）
- 若用 `current_setting('app.tenant_id')`（不带 true），漏设时会**直接报错**，
  让所有未设租户上下文的查询全挂 —— 开发和运维阶段很痛苦
- `fail closed` 更符合安全语义：宁可看不到数据，不要看到别人的数据

**RLS 的价值**：即使应用层漏写 `tenant_id` 过滤条件，数据库层也不会返回他租数据。
这是「防呆」而不是「性能优化」。

### 3.3 两层的关系

| 场景 | 应用层 | 数据库层 | 结果 |
|---|---|---|---|
| 正常查询 | ✅ 带租户 | ✅ RLS 匹配 | 只返回本租户 |
| 应用层漏写过滤条件 | ❌ | ✅ RLS 挡住 | **仍不泄漏** |
| 应用层传错租户 ID | ❌ | ✅ RLS 挡住 | 只返回传错的那个租户（不是数据泄漏，但仍是逻辑错误，需日志告警）|
| 未设租户上下文 | 不存在此入口 | ✅ fail closed | 查不到任何行 |

---

## 4. 租户作用域表的声明方式

平台表（`users` / `org_nodes` 等）在平台迁移里建 policy；
**业务模块的表**通过模块 manifest 声明，由平台统一生成 policy —— 否则每个模块都要手写 policy，迟早漏。

```json
{
  "id": "order",
  "tenantScoped": {
    "tables": ["order_item", "order_log"],
    "column": "tenant_id"
  }
}
```

平台在迁移阶段为这些表统一执行 `ENABLE ROW LEVEL SECURITY` + 建 policy。

> 名字统一用 `tenantScoped`（camelCase，与 manifest 其他字段一致）。

---

## 5. 契约检查 C-3 叠加真实库层

extension-points v1 的 C-3 目前用**内存夹具**验（不连库，快）。
REQ-006 落地后**叠加**一层真实库验证，**两层都要有**：

| 层 | 数据源 | 验什么 | 何时跑 |
|---|---|---|---|
| 内存层（现有） | `InMemoryRepository` | 契约语义：实现方拿不到别的租户数据 | 每次 `cargo test` |
| **真实库层（本期新增）** | `base_test` 库的 `tenant_a` / `tenant_b` 同名数据 | RLS 实际生效、应用层与数据库层双重隔离 | 集成测试（`base_test` 在时） |

真实库层要验三条：
1. 以 tenant_a 的上下文 `list()` → 只返回 tenant_a 的行
2. 以 tenant_a 的上下文 `find(tenant_b 的 id)` → 返回 `None`
3. **绕过应用层**（直接用平台内部连接池、不设 `app.tenant_id`）→ **查不到任何行**
   （验证 RLS 的 fail closed）

第 3 条最关键 —— 它证明的是「即使应用层完全失守，数据库层仍不泄漏」。

---

## 6. 隔离级别：L2 / L3 只留接口，不实现

ADR-0001 D2 定的三档：

| 档 | 本期 | 预留方式 |
|---|---|---|
| L1 共享表 + `tenant_id` + RLS | ✅ **实现** | — |
| L2 独立 schema | ❌ 不实现 | 数据访问层不硬编码 `public` schema；schema 名从配置来 |
| L3 独立库 | ❌ 不实现 | 连接串来自配置，换库即可 |

**不为 L2/L3 提前写代码** —— 只保证「不把 L1 的假设写死」，将来要切时改动面可控。

---

## 7. 配额与限流的位置

| 能力 | 放哪 | 备注 |
|---|---|---|
| 租户配额（存储空间、API 次数、并发） | `platform-db` 提供**用量计数与限额查询**；**执行点**在 `platform-server`/中间件 | 计数写 DB（可靠），不靠内存计数（重启丢失） |
| 限流（租户/用户/接口三级） | 请求中间件 | 用 Redis（已在 Sprint 1 具备） |

> **限流与配额是两回事**：限流是「防止瞬时打爆」（滑动窗口，Redis），配额是「防止长期超量」
> （累计计数，DB）。不要混用。

---

## 8. 本期交付边界

**做**

- [ ] `crates/platform-db`：租户感知的数据访问、`SET LOCAL` + RLS 配合、迁移辅助
- [ ] **不向业务模块暴露未加租户条件的连接**（编译期防呆）
- [ ] 平台迁移：建 RLS 策略；模块表的 policy 由 manifest 声明统一生成
- [ ] `manifest.json` 新增 `tenantScoped` 段 + schema 同步
- [ ] C-3 契约检查叠加真实库层（3 条断言）
- [ ] 配额用量计数与限额查询
- [ ] 集成测试全部跑在 `base_test` 库（`base_dev` 绝不被动）

**不做**（留给后续）

- L2 / L3 隔离级别实现
- 跨租户运营后台（运营视角的租户列表、配额管理）→ 需求 F 项，可放 REQ-020 或独立需求
- 限流中间件的具体实现（先只做配额的数据层，限流放 REQ-019 API 治理）

---

## 9. 资源约束（必须遵守）

- 本机 7.7G 内存 / 4.6G 可用 / 磁盘 121G
- **依赖树已经从 91 涨到 175 crate**（REQ-005 引入 sqlx）。本期**不得再引入新的重量级依赖**
  —— 特别是 **不要再开 sqlx 的 macros feature**（会拉 `syn`/`quote`/`proc-macro2`）
- 本期**必须重新实测**冷编译时间与峰值内存（依赖树变了，之前的 21s / 1087MB 不再作数）
- 日常命令 `cargo check`；测试 `cargo test -- --test-threads=2`
- **不要 `cargo clean`**、不要 `cargo install`、不要升级 `@playwright/test`
- 集成测试用 `base_test`；**`base_dev` 是开发库，测试绝不能碰**
  （REQ-002 的 `scripts/test-integration.sh` 已有白名单闸门，重置只允许 `base_test`）
- 本机**没有 psql 客户端**（用 `podman exec base-postgres psql -U base -d <db>`）
- `export XDG_RUNTIME_DIR=/run/user/$(id -u)` 是 rootless podman 必需

---

## 10. 风险

| # | 风险 | 应对 |
|---|------|------|
| R1 | **连接池 + 租户上下文串号** —— 用普通 `SET` 而非 `SET LOCAL`，归还池后仍带旧租户 | 强制 `SET LOCAL`；集成测试专门覆盖「A 租户查询后立刻用同一池做 B 租户查询」 |
| R2 | 应用层提供了「拿裸连接」的入口 → 业务模块可以绕过租户过滤 | **编译期就不提供**这个入口；契约测试检查公开 API 表面 |
| R3 | RLS policy 漏建某张表 → 该表完全无隔离 | 迁移里统一生成；加一条检查：所有 `tenantScoped` 声明的表都必须有 policy |
| R4 | 依赖树继续变大 → 编译越来越慢 | 本期不引入新重量依赖；实测后若峰值超 1.5G，把 `jobs` 从 3 调到 2 |
| R5 | `SET LOCAL` 忘记在事务里执行 → 报错或静默失效 | 平台封装，业务模块拿不到裸连接；契约测试覆盖 |
