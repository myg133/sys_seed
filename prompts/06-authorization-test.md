# 任务：编写越权测试用例

## 目标

给一个业务模块补**越权（越租户 / 越权限）**测试用例，
证明「拿别人的 ctx 拿不到别人的数据」。

## 输入

- `<模块名>`：业务模块名
- `<被测接口 / 操作>`：要验证的操作（如「按 id 查详情」「改标题」）
- `<越权方式>`：`cross-tenant`（拿别的租户的 ctx）/ `cross-user`（同租户别人的数据）/
  `cross-role`（角色不足）

## 步骤

1. **先读** `modules/<模块名>/tests/contracts.rs` 与 `AGENTS.md`，
   确认夹具（`ContractFixture`）已经提供了什么。
2. **先看契约测试已经覆盖什么**：`#[platform_module_contract]` 宏**已经**自动
   生成了 C-3（租户隔离）的用例 —— 两个租户的同名数据，`list()` 只返本租户、
   `find()` 查他租户 id 返回 `None`。**不要重复造 C-3 已经覆盖的用例。**
   本任务补的是 C-3 没覆盖的**模块自有行为**。
3. **写用例**：在 `modules/<模块名>/tests/` 下加测试文件
   （如 `tests/authorization.rs`），用 `platform_core::context::RequestContext`
   造不同的 ctx：
   ```rust
   let ctx_a = RequestContext::new().with_tenant("tenant-a");
   let ctx_b = RequestContext::new().with_tenant("tenant-b");
   ```
   - `cross-tenant`：用 `ctx_a` 去查 / 改 `tenant_b` 的记录
     → 期望「查不到」（`find` 返回 `None`）、「改不动」（写操作报错）；
   - `cross-user`：同一租户下、另一个归属人的记录 → 按业务规则断言
     （本平台的租户维度是租户，用户维度由业务模块自己定）。
4. **断言口径**：
   - 资源类越权（访问不属于自己的资源）→ **404** 语义：不泄露资源存在性
     （`api-conventions v1` C5），测试里表现为「查不到」而不是「报无权限」；
   - 动作类越权（有资源但无操作权限）→ **403** 语义：资源本可见，是操作被拒。
   **两者的区分要在测试里写清楚注释**，不要混为一谈。
5. **写边界**：越权用例如果写数据，同样要遵守「一次写全有或全无」。
6. **自验**：
   ```bash
   cargo test -p <模块名> -- --test-threads=2
   cargo xtask check-modules
   ```

## 约束

- 不修改 `crates/platform-core/` 与 `crates/platform-contracts/`。
- 不改 `#[platform_module_contract]` 生成的四个用例 —— 那是平台契约，
  业务模块只补自己的用例。
- 越权用例**不依赖数据库**：本期平台没有 DB 访问层，用内存仓储 + 夹具数据。
- 不新增第三方依赖。

## 验收

- [ ] 新增用例与 C-3 已有的用例**不重复**（说明了区别）
- [ ] 每个越权用例都写明了是 404 语义还是 403 语义
- [ ] 用例用不同的 `RequestContext` 构造越权场景，没有绕过 `ctx`
- [ ] `cargo test -p <模块名> -- --test-threads=2` 全绿（原有 73 个 Rust 用例不被破坏）
- [ ] `cargo xtask capabilities --check` 通过
