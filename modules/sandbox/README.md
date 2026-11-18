# 沙箱模块（sandbox）

**示例业务模块 —— 扩展点框架的标准范例。**

它演示了业务模块与平台之间的全部接缝：实现 `PlatformModule` 与 `Repository` 两个扩展点，
并通过 `platform-contracts` 跑通 C-1 ~ C-4 四项契约检查。

## 能力

| 扩展点 | 实现位置 | 说明 |
|--------|----------|------|
| `PlatformModule` | `backend/src/mod.rs` | 模块注册：`id` / `version` / `dependencies` / `setup` / `enabled`，并做编译期注册 |
| `Repository<DemoItem>` | `backend/src/repository.rs` | 内存存储；**严格按 `ctx` 里的租户过滤**（契约层的语义范例） |
| `Repository<TenantItem>` | `backend/src/tenant_repository.rs` | **落库的范例**：走 `platform-db` 的租户作用域入口（`TenantDb`），语句里带 `{tenant}` |

配套类型（`Auditable` / `NotificationSender` / `WorkflowHandler`）本期只在平台侧定义，
消费方分别落在 REQ-015 / REQ-017，沙箱模块不提前实现。

## 依赖

```toml
[dependencies]
platform-core = { path = "../../crates/platform-core" }   # 扩展点 trait
platform-db = { path = "../../crates/platform-db" }       # 租户作用域数据访问入口

[dev-dependencies]
platform-contracts = { path = "../../crates/platform-contracts" }   # 契约测试套件
```

业务模块**不依赖**平台的任何内部模块 —— 平台代码也永不反向依赖业务模块。

## 租户隔离

租户只从 `ctx: &RequestContext` 取。这是**编译期保证**：实现方没有任何别的途径拿到租户，
漏传 `ctx` 就编译不过。契约检查 C-3 再用两个租户的同名数据做行为验证：

```text
tenant_a  user_a@example.com  demo-1  "同名记录"
tenant_b  user_b@example.com  demo-2  "同名记录"   ← 唯一区别是租户
```

- 用 tenant_a 的 ctx 调 `list()` → 只返回 tenant_a 的记录
- 用 tenant_a 的 ctx 调 `find(tenant_b 的 id)` → 返回 `None`

### 落库怎么做（照抄这段）

```rust
// ① 平台入口：每次访问都在事务内 SET LOCAL app.tenant_id
let db = TenantDb::connect(&url, DbOptions::default()).await?;

// ② 语句里必须带 {tenant}：平台强制，缺了直接报错
let query = TenantQuery::new(format!(
    "SELECT tenant_id, id, title, revision FROM {table} \
     WHERE tenant_id = {{tenant}} AND id = ?",
    table = db.table(TENANT_ITEM_TABLE),
))
.bind(id.clone());

// ③ 访问入口必须带 ctx —— 租户只能从 ctx 拿
let found = db.fetch_optional(&ctx, &query).await?;
```

三条规矩：

1. **租户只从 `ctx` 取**；
2. **每条语句都带 `{tenant}`**；
3. **拿不到裸连接** —— `platform-db` 的公开 API 里没有「拿连接」的方法。

### 数据库层还有一道兜底

模块在 `manifest.json` 的 `tenantScoped` 段声明自己的租户作用域表，
**RLS policy 由平台统一生成**（模块不手写 —— 漏建一张表 = 该表完全无隔离）：

```json
"tenantScoped": { "tables": ["sandbox_tenant_item"], "column": "tenant_id" }
```

即使应用层完全失守（漏写租户条件、漏设 `app.tenant_id`），
数据库层也**查不到任何行**（RLS fail closed）——
真实库层的第三条断言就在验这件事（`tests/c3_real_db.rs`）。

## 排序与过滤字段白名单

业务模块各自声明自己允许排序 / 过滤的字段，白名单写死在本模块里：

| 用途 | 白名单 | 位置 |
|------|--------|------|
| 排序 | `id` / `title` / `revision` | `SORTABLE_FIELDS` |
| 过滤 | `id` / `title` | `FILTERABLE_FIELDS` |

**排序字段不在白名单里 → 直接报错**，不静默忽略：

```text
sort field unknown is not sortable
```

对应契约 C7「排序 … 未声明字段返回 400」。静默忽略会让调用方以为排序生效了，
所以这里宁可报错 —— 业务模块照抄本范例时也要保持这个行为。

## 契约测试

```bash
cargo test -p sandbox -- --test-threads=2
```

`tests/contracts.rs` 用 `#[platform_module_contract]` 宏自动生成四个测试：

| 检查 | 判定 |
|------|------|
| C-1 | `manifest.json` 与实现一致（id / version / enabled / dependencies） |
| C-2 | 代码里用到的权限点都在 manifest 里声明（manifest 多余的只警告） |
| C-3 | 租户隔离生效 |
| C-4 | 单次写全有或全无；跨表写必须显式开平台事务 |

> C-2 的告警默认被测试框架捕获，加 `-- --nocapture` 可以看到。

## 前端

`frontend/routes.ts` 是最小路由表，`frontend/pages/DemoPage.svelte` 是最小页面。
前端只读 `manifest.json` 登记的权限点，不维护自己的映射（`api-conventions v1` C4）。
