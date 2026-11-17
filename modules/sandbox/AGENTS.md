# 沙箱模块 · Agent 指南

本目录是**业务模块的标准范例**：照着它写新模块即可，不用再问「模块该长什么样」。

## 目录结构

```text
modules/sandbox/
├── manifest.json      模块清单：id / version / 权限点 / 菜单 / 依赖 / 配置
├── backend/           Rust 实现（本模块用内存存储，不引入 sqlx）
│   └── src/
│       ├── mod.rs     实现 PlatformModule（模块注册）
│       ├── entity.rs  实体
│       ├── repository.rs  实现 Repository（数据访问）
│       ├── service.rs 业务服务层
│       └── permission.rs  权限点常量
├── frontend/          前端最小实现
├── migrations/        数据库迁移（本期无真实表）
├── tests/contracts.rs 契约测试（C-1 ~ C-4）
├── AGENTS.md          本文件
└── README.md          模块说明
```

## 硬规则

1. **依赖方向单向**：业务模块只依赖 `platform-core`（扩展点 trait）；
   `platform-contracts` 只能出现在 `[dev-dependencies]`。
   平台代码永不反向依赖业务模块。
2. **租户只能来自 `ctx`**：`Repository` 的每个方法都必须传 `ctx: &RequestContext`，
   实现里不允许出现自己拼的 `tenant_id` 过滤条件（`api-conventions v1` C3）。
3. **权限点必须登记**：代码里用到的权限点必须同时写在 `manifest.json` 的 `permissions` 里，
   编码为 `module:resource:action`（`api-conventions v1` C4）。
4. **manifest 与实现必须一致**：`manifest.json` 的 `id` / `version` / `enabled` / `dependencies`
   必须与 `PlatformModule` 的实现逐项一致。
5. **单次写操作全有或全无**：`insert` / `update` / `delete` 要么完整生效，要么完全不变；
   主键重复的 `insert` 必须报错，不得覆盖既有数据。

## 常见改动怎么做

| 场景 | 做法 |
|------|------|
| 新增一个扩展点实现 | 在 `backend/src/` 下加文件，实现 `platform_core::extension` 里的 trait |
| 新增权限点 | 同时改 `backend/src/permission.rs` 与 `manifest.json` 的 `permissions` |
| 改模块版本 | 改 `Cargo.toml` 的 `version` 与 `manifest.json` 的 `version`（C-1 会核对） |
| 加数据库表 | 在 `migrations/` 放迁移文件，并把 `repository.rs` 换成真实存储 |

## 自验命令

```bash
cargo test -p sandbox -- --test-threads=2   # 契约测试 C-1 ~ C-4
cargo xtask check-modules                   # 目录规范校验
```
