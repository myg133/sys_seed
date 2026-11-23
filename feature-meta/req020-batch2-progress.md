# REQ-020 第二批进度

状态：**完成**（5 项全部落地；3 处规格与实际不符已如实记录）

## 已完成

### ① 错误码注册（D 类）
- `crates/platform-core/src/titles.rs`：`TITLES` 加 `("LEAVE_INVALID_TRANSITION", "Invalid Leave Transition")`。
- `crates/platform-core/src/problem.rs`：`ALL_CODES` 加 `"LEAVE_INVALID_TRANSITION"`。
- 连带（同一判据的另外两半，不补就红）：
  - `packages/i18n/src/locales/{en-US,zh-CN}.json`：`error.LEAVE_INVALID_TRANSITION`
    （D125 跨栈：会进响应体的码必须两边登记）；
  - `crates/platform-server/tests/i18n_titles.rs`：钉死行数 117 → 118（`REQ_020_NEW_CODES = 1`）。

### ② 模块能被服务加载（C 类）
- 根 `Cargo.toml`：`[workspace.dependencies]` 加 `leave = { path = "modules/leave" }`。
- `crates/platform-server/Cargo.toml`：`leave.workspace = true`。
- `crates/platform-server/src/main.rs`：`"leave" => { ... }` 装配臂（`leave::mount(&config.database_url)`
  + `authenticate_module` + `nest("/api/leave")`）。

### ②-b 发现并补齐：权限点种子迁移（C 类，**设计清单未列**）
- `migrations/20261123000200_leave_permission_seed.{up,down}.sql`。
- 依据：`main.rs::reconcile_permissions` → `platform_rbac::registry::require_consistent`
  在启动时对账；manifest 有、`platform_permission` 没有 → **进程直接退出**。
  实测：补迁移前 `leave_real_db` 在 `grant_permission` 处报
  `the permission leave:request:read is not in the registry`。

### ③ 通知落库适配器（D 类，D354）
- `crates/platform-notify/src/db_sender.rs`（新）：`DbNotificationSender`
  —— `channel() = "in_app"`，`send()` 逐收件人 `NotifyService::enqueue`。
- `crates/platform-notify/src/lib.rs`：`pub mod db_sender;`。

### ④⑤ 真库测试
- `crates/platform-server/tests/leave_real_db.rs`（新，5 条）：201+落库、列表、
  401、403、租户隔离、409 非法流转 + 注册后的 `title`。
- `crates/platform-notify/tests/db_sender_real_db.rs`（新，5 条）：真表落库、多收件人、
  空收件人、缺模板、`channel()`。
- `crates/platform-notify/tests/common/mod.rs`：`Fixture.service` 改 `Arc<NotifyService>`
  （适配器按装配层真实形态持有 Arc）。

## 与规格不符（如实记录）

1. `leave::mount` 签名是 `mount<S2>(database_url)`（**一个参数**），不是简报里的
   `mount(db, manifest_root)` → 按实际签名接线。
2. `leave::mount` **不注入** `AuditSink` / `NotificationSender`
   （`LeaveState::with_audit/with_notifier` 存在，但不是 `mount` 的入口）
   → 走真实装配的 `/api/leave/*` **不写审计、不发通知**；AC-S-189 的审计/通知计数表
   在本批次**不可达**（需改 `mount` 或改用 `connect` + `http::router` 装配）。
3. `enqueue` 要求**已存在的模板**（code/channel/locale 三级回退）→ 适配器只能收模板编码，
   `NotificationMessage.title/body` 不参与渲染。
4. 业务通知审批人时只给 `payload["audience"]`、`recipients` 为空；平台没有
   「按权限点解析收件人」的能力 → 提交通知（AC-S-189 第 2 行）不会落库（适配器不猜）。
5. `authenticate_module` 是 bin 私有函数，集成测试调不到 → 测试里逐行复刻
   （同一组平台 API），文件头已登记局限；根本解法是 TD-076（把 build_router 抽到 lib）。

## 验证结果

- `cargo build -p platform-server`：通过
- `cargo test -p platform-core`：146 + 7 通过
- `cargo test -p leave --all-targets -- --test-threads=1`：8 + 6 + 11 通过
- `cargo test -p platform-server --test leave_real_db --include-ignored`：5 通过
- `cargo test -p platform-notify --test db_sender_real_db --include-ignored`：5 通过
- `cargo test -p platform-notify --test notify_real_db`（真库）：13 通过（夹具改 Arc 未回归）
- `cargo test -p platform-server --test i18n_titles`：9 通过
- `cargo xtask check-deps` / `capabilities --check` / `check-modules`：退出码 0

## 遗留风险

- `modules/sandbox/tests/http_authz.rs` 的 setup 会对**所有** manifest 声明的表
  `apply_policies`，但只应用 sandbox 自己的迁移 → **全新 base_test** 上会因
  `leave_request` 不存在而失败（当前库已有该表，实测通过）。
  这是跨模块测试卫生问题，未在本批修改（不属于本批范围）。
