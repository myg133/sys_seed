# 沙箱模块 · Agent 指南

本目录是**业务模块的标准范例**：照着它写新模块即可，不用再问「模块该长什么样」。

## 目录结构

```text
modules/sandbox/
├── manifest.json      模块清单：id / version / 权限点 / 菜单 / 依赖 / 配置 / tenantScoped
├── backend/           Rust 实现（内存实现 + 走 platform-db 的租户作用域实现）
│   └── src/
│       ├── mod.rs     实现 PlatformModule（模块注册）
│       ├── entity.rs  实体
│       ├── repository.rs  实现 Repository（内存存储，演示契约语义）
│       ├── tenant_repository.rs  实现 Repository（走 platform-db 的租户作用域入口）
│       ├── service.rs 业务服务层
│       └── permission.rs  权限点常量
├── frontend/          前端最小实现
├── migrations/        数据库迁移（租户作用域表的建表脚本；policy 由平台生成）
├── tests/contracts.rs 契约测试（C-1 ~ C-4，内存层）
├── tests/c3_real_db.rs  C-3 真实库层（跑在 base_test 库）
├── AGENTS.md          本文件
└── README.md          模块说明
```

## 硬规则

1. **依赖方向单向**：业务模块只依赖 `platform-core`（扩展点 trait）与 `platform-db`
   （租户作用域数据访问入口）；`platform-contracts` 只能出现在 `[dev-dependencies]`。
   平台代码永不反向依赖业务模块。
2. **租户只能来自 `ctx`**：`Repository` 的每个方法都必须传 `ctx: &RequestContext`，
   实现里不允许出现自己拼的 `tenant_id` 过滤条件（`api-conventions v1` C3）。
3. **落库必须走 `platform-db` 的租户作用域入口**：`TenantDb` 的每条语句都要带
   `{tenant}` 占位符（平台强制，缺了就报错）；**不要自己建连接池、不要直连数据库** ——
   `platform-db` 的公开 API 拿不到裸连接，就是为了让这件事做不到。
4. **租户作用域表在 manifest 的 `tenantScoped` 段声明**，**RLS policy 由平台统一生成**
   （`cargo xtask tenant-policies`）—— **模块不手写 policy**（漏建一张表 = 该表完全无隔离）。
5. **权限点必须登记**：代码里用到的权限点必须同时写在 `manifest.json` 的 `permissions` 里，
   编码为 `module:resource:action`（`api-conventions v1` C4）。
6. **manifest 与实现必须一致**：`manifest.json` 的 `id` / `version` / `enabled` / `dependencies`
   必须与 `PlatformModule` 的实现逐项一致。
7. **单次写操作全有或全无**：`insert` / `update` / `delete` 要么完整生效，要么完全不变；
   主键重复的 `insert` 必须报错，不得覆盖既有数据。
8. **排序字段必须先声明**：不在 `SORTABLE_FIELDS` 白名单里的排序字段直接报错，
   不静默忽略（`api-conventions v1` C7）。

## 常见改动怎么做

| 场景 | 做法 |
|------|------|
| 新增一个扩展点实现 | 在 `backend/src/` 下加文件，实现 `platform_core::extension` 里的 trait |
| 新增权限点 | 同时改 `backend/src/permission.rs` 与 `manifest.json` 的 `permissions` |
| 改模块版本 | 改 `Cargo.toml` 的 `version` 与 `manifest.json` 的 `version`（C-1 会核对） |
| 加数据库表 | 在 `migrations/` 放迁移文件（只建表），并在 `manifest.json` 的 `tenantScoped` 段声明；policy 由平台生成 |
| 仓储换成真实存储 | 照抄 `backend/src/tenant_repository.rs`：用 `platform_db::TenantDb`，语句里带 `{tenant}` |

## 自验命令

```bash
cargo test -p sandbox -- --test-threads=2   # 契约测试 C-1 ~ C-4（内存层）
# 真实库层（需要 base_test 库；默认 #[ignore]，由 scripts/test-integration.sh 跑到）
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
  cargo test -p sandbox --test c3_real_db -- --include-ignored --test-threads=2
cargo xtask check-modules                   # 目录规范校验
```
