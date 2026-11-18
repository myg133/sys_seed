# 任务：新增权限点并接入前端显隐

## 目标

给已有业务模块加一个权限点：后端声明、前端按权限点显隐、契约检查 C-2 仍然通过。

## 输入

- `<模块名>`：模块目录名（`modules/<模块名>/`）
- `<资源名>`：资源名（权限点编码的第二段）
- `<动作>`：动作（`read` / `write` / `approve` / 自定义）
- `<权限点名称>`：展示给人看的名字（中文）
- `<权限点类型>`：`menu` / `button` / `field` / `data`
- `<前端位置>`：哪个页面 / 组件要按这个权限点显隐

## 步骤

1. **先读** `modules/<模块名>/AGENTS.md` 与 `modules/<模块名>/manifest.json`，
   确认已有的权限点命名习惯（`module:resource:action`，三段、全小写连字符）。
2. **改代码常量**：在 `modules/<模块名>/backend/src/permission.rs` 里加常量，
   并把它加进该文件的 `PERMISSIONS` 数组。
   编码形如 `<模块名>:<资源名>:<动作>`。
3. **改 manifest**：在 `modules/<模块名>/manifest.json` 的 `permissions` 里加同一条
   （`code` / `name` / `type` 三个字段都要有）。**先改 manifest 再改代码也行，
   但两边必须同时到位** —— 契约检查 C-2 会核对：
   - 代码里用了、manifest 没声明 → **失败**；
   - manifest 声明了、代码里没用 → **告警**。
4. **接前端显隐**：在 `modules/<模块名>/frontend/routes.ts` 或页面组件里，
   按权限点决定是否渲染。权限点字符串**只从 manifest / permission.rs 来**，
   不要在前端另写一份映射（`api-conventions v1` C4：前端 `can()` 与后端鉴权
   读同一份注册表）。
5. **更新能力索引**：`cargo xtask capabilities`。
6. **自验**：
   ```bash
   cargo test -p <模块名> -- --test-threads=2 -- --nocapture
   cargo xtask capabilities --check
   ```

## 约束

- 权限点编码固定三段、全小写连字符分隔；不要用驼峰、不要用下划线。
- 不要硬编码角色名（如 `if role == "admin"`）—— 权限点 ≠ 角色。
- 不修改 `crates/platform-core/` 与 `crates/platform-contracts/`。
- 平台目前**没有**前端 `can()` 与服务端鉴权中间件
  （见 `capabilities/index.json` 的 `not_yet_provided.permission_can`）：
  在它们落地前，前端显隐只是「按权限点过滤」，**不等于**服务端已经鉴权，
  不要在注释或文档里宣称「已经受权限保护」。

## 验收

- [ ] `manifest.json` 与 `permission.rs` 里的权限点编码完全一致
- [ ] `cargo test -p <模块名> -- --test-threads=2` 全绿，且 C-2 **无告警**
      （加 `-- --nocapture` 确认）
- [ ] `cargo xtask capabilities --check` 通过，`capabilities/permissions.json`
      里出现新权限点
- [ ] 前端没有第二份权限点字面量（同一编码只出现在 manifest / permission.rs）
- [ ] `modules/<模块名>/AGENTS.md` 的「本模块权限点」小节已更新
