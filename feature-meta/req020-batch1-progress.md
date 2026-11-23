# REQ-020 批次一进度：modules/leave 后端核心

状态：进行中

## 文件清单

<!-- 每完成一个文件追加一行 -->
| 文件 | 状态 |
|---|---|
| `modules/leave/Cargo.toml` | 完成（含依赖面注释） |
| 根 `Cargo.toml` members += `modules/leave` | 完成 |
| `modules/leave/backend/src/entity.rs` | 完成（`LeaveRequest` + `Status` 闭集 4 值 + `FromRow` + `parse_date`） |
| `modules/leave/backend/src/permission.rs` | 完成（3 个权限点常量） |
| `modules/leave/backend/src/repository.rs` | 完成（`TenantQuery` 纯函数构造器 + `PgLeaveRepository`，CAS 状态更新） |
| `modules/leave/backend/src/service.rs` | 完成（状态机 + 编排 + 审计/通知注入口） |
| `modules/leave/backend/src/http.rs` | 完成（4 端点 × 3 权限组 + 409 映射） |
| `modules/leave/backend/src/mod.rs` | 完成（`PlatformModule` + 注册宏 + `mount`/`connect`） |
| `modules/leave/migrations/20261123000100_leave_request.{up,down}.sql` | 完成 |
| `modules/leave/manifest.json` | 完成（3 权限点 + `tenantScoped`） |
| `modules/leave/README.md` / `AGENTS.md` / `frontend/README.md` | 完成（check-modules 需要） |
| `modules/leave/tests/contracts.rs` | 完成（内存 `Repository` 夹具 + `#[platform_module_contract]`，C-1~C-6 全绿） |
| `modules/leave/tests/state_machine.rs` | 完成（4 条合法 + 5 条非法，非法逐条断言 `LEAVE_INVALID_TRANSITION`） |
| `capabilities/index.json` + `capabilities/permissions.json` | **生成物**已重新生成（`cargo xtask capabilities`），非手改 |

## 验证结果（实跑）

```console
$ cargo build -p leave
    Finished `dev` profile [unoptimized + debuginfo]

$ cargo test -p leave --all-targets -- --test-threads=1
unittests backend/src/mod.rs : ok. 8 passed; 0 failed
tests/contracts.rs           : ok. 6 passed; 0 failed   (C-1~C-6)
tests/state_machine.rs       : ok. 11 passed; 0 failed
总计 25 passed / 0 failed

$ cargo fmt -p leave -- --check      → clean
$ cargo clippy -p leave --all-targets → 无 warning / error
$ cargo xtask check-modules          → 通过（leave / sandbox 2 个模块）
$ cargo xtask check-deps             → exit 0
$ cargo xtask capabilities --check   → 通过（14 个文件与源码一致）
```

C-5 **变异验证**（临时删掉 `approve` 组的一层 `route_layer`）：

```console
[C-5] contract violation:
  - the permission point `leave:request:approve` is declared in manifest.json but has no authorization call site
test result: FAILED. 5 passed; 1 failed
```

恢复后重新全绿 —— 说明 C-5 真的在扫调用点，不是恒绿。

## 红线 6 条（逐条实测）

| # | 断言 | 结果 |
|---|---|---|
| ① 无裸 `sqlx::query` | `grep -rn "sqlx::query" modules/leave/backend/src/ \| wc -l` | **0** |
| ② 不依赖 `platform-notify` | `grep -rn "platform-notify" modules/leave/Cargo.toml \| wc -l` | **0** |
| ③ 无审计表名/函数名 | `grep -rn "platform_audit_log\|platform_audit_append" modules/leave/backend/src/ \| wc -l` | **0** |
| ④ 不自己解析 JWT | `grep -rniE "jwt\|argon2\|hmac" modules/leave/backend/src/ \| wc -l` | **0** |
| ⑤ 每个路由组挂鉴权 | `grep -rn "route_layer(require_permission" ... \| wc -l` | **3**（读/写/审批），`require_permission(` 共 4 处（含 import） |
| ⑥ 注释与代码一致 | 人工核对 + 上面 C-5 变异验证 | 一致 |

⚠️ ① 第一次实测为 **1**：命中 `repository.rs:3` 的**注释**（`//! # 为什么没有裸 \`sqlx::query\``）。
判据是纯 grep，注释也算 → 已改写成「裸 SQL 调用（`sqlx` 的 query 系列）」，复测 0。

## 规格与现实的差异（如实登记）

1. **`Cargo.toml` 的位置与任务/AC 不一致（需要裁定）**
   - 任务写 `modules/leave/backend/Cargo.toml`，AC-S-188 ⑧ 与 AC-S-193 也 grep `modules/leave/backend/Cargo.toml`；
   - 但仓库范式（`cargo xtask create-module` 的 `CARGO_TOML` 模板、`modules/sandbox/Cargo.toml`）是
     **`modules/<name>/Cargo.toml` + `[lib] path = "backend/src/mod.rs"`**，根 members 只能是 `"modules/leave"`；
   - 两者不可兼得（一个 crate 只能有一个 manifest）。本批次按**仓库范式**落
     `modules/leave/Cargo.toml`，因为任务同时要求 members 加 `"modules/leave"`，且 scaffold/check-modules 都按这个布局；
   - **影响**：AC-S-188 ⑧、AC-S-193 里的 `grep ... modules/leave/backend/Cargo.toml` 会**空输出/报错**，
     应改为 `modules/leave/Cargo.toml`（或平台改范式）。**我没有为了过判据另造一份 manifest。**
2. **`NotificationSender` 不是 dyn-compatible**（`send` 返回 `impl Future`）→ `Arc<dyn NotificationSender>`
   编译不过（E0038，13 处报错）。业务侧加了一个装箱 shim `service::Notifier`（唯一实现就是调平台 `send()`），
   `with_notifier(impl NotificationSender)` 仍然是「注入平台的 sender」。
3. **`time` 只开 `formatting`**（`parsing` / `macros` 只在 sqlx 的传递 feature 里）→
   日期解析手写 `entity::parse_date`（不依赖传递 feature）；时间输出用 `Rfc3339`（formatting 够用）。
4. **`BindValue` 没有 UUID / Date 变体** → `id` 用 `TEXT` 存 uuid v4 文本；`starts_on` / `ends_on`
   是 `DATE` 列但绑定走文本 + `?::date`（不为此扩平台 API）。
5. **cursor 分页没有平台统一编解码器**（`List::next_cursor` 只是一个字段）→ 只实现 `limit`
   （缺省 20 / 上限 200），没有自己发明游标格式。
6. **业务错误码不在平台 titles 表** → `LEAVE_INVALID_TRANSITION` 的 `title` 落到 `FALLBACK_TITLE`（"Error"）。
   要正式标题得在平台登记（B/C 类改动），本批次不做。
7. **「审批人」收件人不可知**：`submit` 的通知用 `payload.audience = "leave:request:approve"`，
   收件人解析留给平台适配器（第二批的 `DbNotificationSender`）。本批次**不声称**通知已落库。
8. **审计与状态变更不在同一事务**：审计 sink 是独立连接（平台侧），状态落库后审计失败只能返回 500。
   如实记为限制，未用「假事务」掩盖。
9. **真库租户隔离测试未做**（任务里标「可选」）：需要 `sqlx` dev-dependency + 迁移 + 应用角色 RLS，
   参照 `modules/sandbox/tests/c3_real_db.rs`（429 行基建），本批次时间不足，留给后续。
10. **`capabilities/**` 是生成物**：`capabilities --check` 原本红（新模块不在索引里），
    已跑 `cargo xtask capabilities` 重新生成；diff 只增加 `leave` 的 3 个权限点与模块条目。

## 代码量（AC-S-187 口径预览，最终报告在第三批）

```console
$ find modules/leave/backend/src -name '*.rs' | xargs wc -l | tail -1
 1341 总计
```
构成：注释 278 行、空行 117 行、`#[cfg(test)]` 内 128 行 → 代码约 818 行。
→ **远超 500 行预算**（D357 已预告骨架占 458 行；本实现连文档与内联单测一起也被口径计入）。
