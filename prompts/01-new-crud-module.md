# 任务：新建一个 CRUD 业务模块

## 目标

在 `modules/<name>/` 落一个**开箱即过 CI** 的 CRUD 业务模块：
`cargo xtask check-modules` 通过、契约检查 C-1 ~ C-4 通过、前后端 lint 通过。

## 输入

- `<name>`：模块名（小写 kebab-case，同时是目录名与 `manifest.json` 的 `id`）
- `<资源名>`：业务资源名（如 `invoice`，权限点编码的第二段）
- `<实体字段>`：实体字段清单（名称 / 类型 / 是否必填）
- `<菜单标题>`：菜单上显示的名字

## 步骤

1. **先读**仓库级 `AGENTS.md` 与 `capabilities/index.json` ——
   确认哪些能力平台已有（`already_provided`）、哪些还没有（`not_yet_provided`），
   **不要重写底座已有的东西**，也不要引用还不存在的 API。
2. **生成骨架**：`cargo xtask create-module <name>`
   （命令会一并把模块登记进根 `Cargo.toml` 的工作区成员）。
   骨架已包含 `manifest.json` / `backend` / `frontend` / `migrations` / `tests` /
   `AGENTS.md` / `README.md`，并**已经能编译、已经能过契约测试**。
3. **实现 backend**（`modules/<name>/backend/src/`）：
   - `entity.rs`：把 `<实体字段>` 落成实体。实体必须带 `tenant_id`（归属租户）
     与一个能判断「这次写是否真的生效」的字段（如 `revision`）。
   - `repository.rs`：实现 `platform_core::extension::repository::Repository`。
     **每个方法都必须用 `ctx: &RequestContext` 里的租户过滤**，
     不允许自己拼 `tenant_id` 条件；排序 / 过滤字段走模块自己的白名单，
     白名单外的排序字段**直接报错**，不静默忽略。
   - `service.rs`：业务编排。只有 service 层可以调 Repository。
   - `mod.rs`：实现 `PlatformModule`（`id` / `version` / `enabled` /
     `dependencies` 必须与 `manifest.json` 逐项一致），并用
     `platform_core::register_platform_module!` 做编译期注册。
4. **声明权限点与菜单**：在 `manifest.json` 的 `permissions` 里登记
   `<name>:<资源名>:read` / `<name>:<资源名>:write`（三段式、全小写连字符），
   菜单 `route` 用 `/<name>`。前端只读 manifest，不维护自己的权限映射。
5. **实现 frontend**（`modules/<name>/frontend/`）：
   - 路由表放 `frontend/routes.ts`，每个路由绑定一个权限点；
   - 页面放 `frontend/pages/`；
   - **优先用 `@base/ui` 与 `@base/schema`**：平台目前**没有**内置表格组件
     （见 `capabilities/index.json` 的 `not_yet_provided.ui_table_component`），
     所以列表页的分页要自己实现，但色调 / 圆角 / 字段元数据一律复用平台包。
6. **接入契约测试**：`tests/contracts.rs` 已生成，把夹具里的实体与仓储换成
   你的真实实现（`seed()` 要造**两个租户的同名数据**，`write_boundary()`
   要如实声明写边界）。
7. **更新模块文档**：填 `modules/<name>/AGENTS.md`（领域模型、用到的扩展点、
   本模块权限点、如何验证、易踩的坑）。
8. **刷新能力索引**：`cargo xtask capabilities` —— 让索引包含本模块的权限点。
9. **自验**（全部要过）：
   ```bash
   cargo xtask check-modules
   cargo test -p <name> -- --test-threads=2
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo xtask capabilities --check
   pnpm exec prettier --check .
   pnpm exec eslint .
   ```

## 约束

- 不修改 `crates/platform-core/` 与 `crates/platform-contracts/`。
- 业务模块只依赖 `platform-core`；`platform-contracts` 只能进 `[dev-dependencies]`。
- 权限点必须在 `manifest.json` 声明，代码里用到的权限点与 manifest 必须一一对上。
- 数据访问必须走 `RequestContext`，不手写租户过滤条件。
- 错误只返回错误码 + 参数，面向用户的中文文案交给 `@base/i18n`。
- 时间存 UTC，金额用最小单位整数。
- 不用 `sqlx`、不用 `async-trait`、不引任何新的第三方依赖。

## 验收

- [ ] `cargo xtask check-modules` 通过
- [ ] `cargo test -p <name> -- --test-threads=2` 全绿（C-1 ~ C-4）
- [ ] `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` 无告警
- [ ] `cargo xtask capabilities --check` 通过，且 `capabilities/permissions.json` 里能看到本模块的权限点
- [ ] 未修改 `crates/platform-core/` 与 `crates/platform-contracts/`
- [ ] `modules/<name>/AGENTS.md` 已填好（不是模板原样）
- [ ] 新增第三方依赖：无
