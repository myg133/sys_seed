# 沙箱模块（sandbox）

**示例业务模块 —— 扩展点框架的标准范例。**

它演示了业务模块与平台之间的全部接缝：实现 `PlatformModule` 与 `Repository` 两个扩展点，
并通过 `platform-contracts` 跑通 C-1 ~ C-4 四项契约检查。

## 能力

| 扩展点 | 实现位置 | 说明 |
|--------|----------|------|
| `PlatformModule` | `backend/src/mod.rs` | 模块注册：`id` / `version` / `dependencies` / `setup` / `enabled`，并做编译期注册 |
| `Repository<DemoItem>` | `backend/src/repository.rs` | 内存存储；**严格按 `ctx` 里的租户过滤** |

配套类型（`Auditable` / `NotificationSender` / `WorkflowHandler`）本期只在平台侧定义，
消费方分别落在 REQ-015 / REQ-017，沙箱模块不提前实现。

## 依赖

```toml
[dependencies]
platform-core = { path = "../../crates/platform-core" }   # 扩展点 trait

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
