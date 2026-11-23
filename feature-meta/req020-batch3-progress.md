# REQ-020 第三批 进度

进行中

## ① 注入式装配（main.rs）— 完成

- `main.rs` 的 `"leave"` 臂改成 `leave::connect` → `.with_audit(audit_sink)` →
  `.with_notifier(DbNotificationSender::new(service, NOTIFICATION_TEMPLATE))` → `leave::http::router`。
- **与预想不符**：`notify_service` 在 `main.rs` **原本不存在**（通知服务在模块装配循环
  **之后**才构造，而循环前又没有 `config_service`）。→ 把 `build_config_service` 与
  「通知服务」提到模块装配之前；`build_notify_state` 拆成 `build_notify_service`（服务）
  + `build_notify_state`（接 HTTP，不再建第二个实例）。
- 模板编码常量放在业务侧：`modules/leave/.../service.rs` 的 `NOTIFICATION_TEMPLATE = "leave_decision"`。

## ② 计数表测试 — 完成

`crates/platform-server/tests/leave_real_db.rs::the_audit_and_notification_counts_match_the_table`：
查 `platform_audit_log`（action 逐条相等）+ `platform_notify_delivery`（收件人 = 申请人 A）。
6/6 绿（含新增）。

## ③ 通知模板种子 — **未按「迁移」实现（规格不可实现）**

实测：`platform_notify_template` 是**租户级**表（`tenant_id NOT NULL`，
唯一键 `(tenant_id, code, channel, locale)`），`resolve_template` 的 SQL 按
`tenant_id = {ctx.tenant}` 过滤 → **迁移里没有租户可写**，写 `tenant_id='*'` 永远匹配不上。
REQ-015 也没有任何模板种子迁移（平台约定 = 租户通过 `POST /api/notify/templates` 自己种）。
→ 改为**测试夹具**用真服务方法 `NotifyService::create_template` 在真表里种
zh-CN + en-US 两条（见 `seed_decision_template`）。不降低判据强度（计数查的仍是真表）。

## ④ v2 升级 + 弃用登记 — 完成

### 4a v2 端点
- `GET /api/rbac/v2/me/permissions` → `{permissions, sources}`；v1 一行未改。
- **`sources` 是真实计算**：`RbacService::permission_sources`（新增）用与
  `effective_permissions` **同一份**展开规则（`user_role_ids` + `grants_of` +
  `pattern.expand(registry)` + 内置管理员 = 注册表全集），把「权限点 → 提供它的角色 code」聚起来。
  角色用 **code**（不是 id）—— 消费方拿到就能比对。
- ⚠️ **与预想不符（需 BA 补登记）**：`sources` **无法只改 `platform-rbac/src/http.rs`** ——
  `load_graph` / `user_role_ids` 都是 `service` 模块私有，http 层拿不到角色图。
  要么新增 `crates/platform-rbac/src/service.rs` 的方法（**本实现选的**），
  要么按规格允许的退路返回占位 `{}`（会让「升级」只剩「多一个键」）。
  → **`design-summary.md` §3 需补一行 B 类**：`crates/platform-rbac/src/service.rs`。

### 4b 弃用登记
- `platform-observability/src/http.rs::default_deprecations()` 加
  `"/api/rbac/me/permissions"` → `2027-06-30`（与既有那条同一写法/窗口）。
- v2 路径**不登记**（按路径精确匹配，v2 不会继承头）。

### 4c 测试
- `crates/platform-rbac/tests/me_permissions.rs::tc_020_v2_lists_permission_sources_and_keeps_v1_intact`
  （v1 仍「恰好一个键」；v2 两个键且 `permissions` 与 v1 逐项相同；`sources` 键集合恰好等于
  `permissions`，值恰好是角色 code —— **不是占位空对象**）。
- `crates/platform-observability/tests/http_contract.rs::the_rbac_v1_permissions_endpoint_is_deprecated_and_v2_is_not`
  （v1 两个头 + Sunset 严格 IMF-fixdate；v2 与 `/api/leave/requests` 都**不带头**）。

## ⑤ router_assembly — 完成

- `REQUIRED_MOUNTS` 加 `("请假模块", r#".nest("/api/leave""#)`。
- **变异验证**：把 `main.rs` 的 `"leave"` 臂改成 `"leave" => {}` →
  `every_subsystem_is_mounted_in_main` **红**（`缺少 1 个挂载点：请假模块`）→ 还原 → 绿。

## 变异验证（计数表是否真有区分力）

1. 去掉 `.with_audit(...)` → `第 1 行：建单恰好 1 条审计` 红（`left: []`）；
2. 再去掉 `.with_notifier(...)` → `第 3 行：恰好 1 条投递…` 红（`left: []`，右值是真实
   `("dbcf5daf-…", "in_app", "leave_decision")`）；
3. 两次均为手工改动 → 已还原并重跑全绿。

## 完成状态

- `cargo build -p platform-server`：通过（2 条**既有**未用 import 警告：`log_filter` /
  `SubscriberExt`，非本批引入，未顺手清理）。
- `leave_real_db` 6/6 绿；`platform-rbac --all-targets` 86 绿；`platform-observability --all-targets` 110 绿；
  `leave --all-targets` 25 绿；`platform-notify db_sender_real_db` 5 绿；`router_assembly` 2 绿。
- `xtask check-deps` / `capabilities --check` / `check-modules` 均退出码 0。

## 计数表的实际观测值（临时打印后已移除打印）

```text
[ROW1] audit=["leave.request.created"] notify=[]
[ROW2] audit=["leave.request.created", "leave.request.submitted"] notify=[]
[ROW3] audit=["leave.request.created", "leave.request.submitted", "leave.request.approved"]
       notify=[("<applicant user id>", "in_app", "leave_decision")]
[ROW4] audit=["leave.request.created", "leave.request.submitted", "leave.request.rejected"]
       notify=[("<applicant user id>", "in_app", "leave_decision")]
[ROW5] audit=[同上 ROW3] notify=[同上 ROW3]      ← 非法流转前后完全一致
```
