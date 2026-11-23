# modules/leave —— 给接手这个模块的 AI / 人的规则

## 硬规则（违反则有契约测试兜底）

1. **数据访问只走 `platform_db::TenantDb` + `TenantQuery`**。
   裸 `sqlx::query` / `sqlx::query_as` 一律不允许（AC-S-188 ②）。
2. **不依赖平台内部 crate**：运行期只有 `platform-core` / `platform-db`，
   `platform-contracts` 只在 `[dev-dependencies]`（AC-S-193）。
   尤其**不得**依赖 `platform-notify`（通知走 `NotificationSender` trait）。
3. **鉴权只能调 `require_permission("...")`**，参数必须是**字面量**，
   且每个权限点都要有调用点（C-5）。
4. **状态机在业务层**：`draft → pending → approved/rejected`，
   非法流转必须 409 + `LEAVE_INVALID_TRANSITION`。
5. **租户列 `tenant_id` 不许由请求体决定**，只从 `RequestContext` 取。

## 修改顺序建议

- 加字段：`migrations/` 新迁移 → `entity.rs` → `repository.rs` 的列清单 → 响应体；
- 加状态：先改状态机与错误码测试，再改 `CHECK` 约束（两者必须同步）。
