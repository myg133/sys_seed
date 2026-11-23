# 用例：平台能力复用（不自造轮子）

> 覆盖 **AC-S-188**
> 用例编号：TC-020-012 ~ TC-020-018（7 条）
> **rev 2**：grep 模式修正（M2/M3）、阳性对照逐条指定（M4）

⚠️ 本文件全是 **grep 类静态断言**，因此**每条「期望 == 0」都必须带阳性对照**（§9.2）。
rev 1 笼统写「对 `modules/sandbox` 跑同样的 grep」—— **实测无效**：
sandbox 全走 `TenantQuery`，`sqlx::query` 本身就 0，拿它当对照永远「结论不可信」（M4）。

---

## TC-020-012 · AC-S-188 ① · 租户隔离走平台入口

- **层次**: 静态（grep）
- **步骤**:
  ```bash
  grep -rn "TenantDb\|TenantQuery" modules/leave/backend/src/ | wc -l   # 期望 > 0
  ```
- **期望**: **> 0**。
- **判据形态**: 计数。
- ⚠️ 正向断言（> 0），不需要阳性对照 —— 但它为 0 时说明业务根本没读写库。

---

## TC-020-013 · AC-S-188 ② · 不写裸 SQL

- **层次**: 静态（grep）
- **步骤**:
  ```bash
  grep -rn "sqlx::query\|sqlx::query_as" modules/leave/backend/src/ | wc -l
  ```
- **期望**: **== 0**。
- **阳性对照**: `grep -rn "sqlx::query\|sqlx::query_as" crates/platform-db/src/ | wc -l` → **10**（实测）。
- ⚠️ rev 1 的对照取 `modules/sandbox` → 实测 **0** → 对照失效（M4）。
- ⚠️ 注意：`TenantQuery::render` 内部会用 sqlx，但那是**平台**的代码，不在业务目录里。

---

## TC-020-014 · AC-S-188 ③ · 鉴权走平台唯一入口

- **层次**: 静态（grep）
- **步骤**:
  ```bash
  grep -rn "require_permission(" modules/leave/backend/src/ | wc -l
  ```
- **期望**: **> 0**（4 个路由组至少 3 个权限点）。
- **判据形态**: 计数。
- ⚠️ 与 TC-020-018（C-5 契约）互补：这条验「有没有调用」，那条验「每个权限点都被调用到」。

---

## TC-020-015 · AC-S-188 ④ · 不自己实现加密/令牌

- **层次**: 静态（grep）
- **步骤**:
  ```bash
  grep -rniE "jwt|jsonwebtoken|argon2|hmac|bcrypt" modules/leave/backend/src/ | wc -l
  ```
- **期望**: **== 0**。
- **阳性对照**: `grep -rniE "jwt|argon2|sha256" crates/platform-iam/src/ | wc -l` → **159**（实测）。
- ⚠️ 注意 grep 用 `-i`（大小写不敏感），rev 1 漏了，`JWT` 会漏网。

---

## TC-020-016 · AC-S-188 ⑤⑥ · 审计写平台 sink，不碰审计表

- **层次**: 静态（grep）
- **步骤**:
  ```bash
  # ⑤ 用了平台的审计类型
  grep -rn "AuditEvent\|AuditSink" modules/leave/backend/src/ | wc -l      # 期望 > 0
  # ⑥ 不得出现审计表名 / 审计函数名
  grep -rn "platform_audit_log\|platform_audit_append" modules/leave/backend/src/ | wc -l  # 期望 == 0
  ```
- **期望**: ⑤ **> 0**；⑥ **== 0**。
- **阳性对照**（⑥）: `grep -rn "platform_audit_append" crates/platform-audit/src/sink.rs | wc -l` → **> 0**。
- ⚠️ **rev 1 的 ⑥ 是 `INSERT INTO.*audit`，无区分力且可绕过**（M3 实测）：
  - 平台自己的审计写入走 `TenantQuery::new("SELECT platform_audit_append(...)")`，
    **不匹配**该模式 → 阳性对照也拿不到；
  - 业务可以用 `TenantQuery::new("INSERT INTO platform_audit_log\n …")` 绕过
    （不触发 `sqlx::query`，多行也不匹配点号）→ 哈希链被旁路而判据全绿。
  → 改为「不得出现表名/函数名」，这才是真正有区分力的模式。

---

## TC-020-017 · AC-S-188 ⑦⑧ · 通知走 trait，不依赖 notify crate

- **层次**: 静态（grep）
- **步骤**:
  ```bash
  # ⑦ 用了平台的 trait
  grep -rn "NotificationSender\|NotificationMessage" modules/leave/backend/src/ | wc -l
  # ⑧ 依赖清单里不得有 platform-notify
  grep -rn "platform-notify" modules/leave/Cargo.toml | wc -l
  ```
- **期望**: ⑦ **> 0**；⑧ **== 0**。
- ⚠️ **rev 1 的 ⑦ 是 `grep "NewDelivery\|notify"`，两处错**（M2 实测）：
  - `NewDelivery` 定义在 `platform-notify/src/store.rs:114` —— **正是 ⑧ 禁止依赖的 crate**；
  - `notify` 大小写敏感，**不匹配** `NotificationSender`
    （实测：`printf 'use ...NotificationSender;\n' | grep -c "notify"` → **0**）→ 假红。
  → 改为 `NotificationSender|NotificationMessage`。

---

## TC-020-018 · AC-S-188 · C-5 覆盖到新模块（含变异验证）

- **层次**: 契约测试
- **步骤**:
  1. 确认 `modules/leave/backend/` 挂了 `#[platform_module_contract]`；
  2. 跑该模块的契约测试；
  3. **变异**：删掉任意一层 `route_layer(require_permission(...))` → 重跑 C-5 → **还原**。
- **期望**:
  - 步骤 2：C-5 通过（declared 权限点都有调用点）；
  - 步骤 3：C-5 **必须红**。
- **判据形态**: 步骤 2 绿 + 步骤 3 红。
- ⚠️ 变异验证是必须的：C-5 曾**只覆盖 `modules/sandbox` 一处**，
  8 个平台 crate 从未被扫过（TD-003 的教训）。新模块必须确认真的被覆盖到。
- ⚠️ 若步骤 3 仍然绿 → 说明 C-5 没扫到这个模块（**契约宏没挂上**或路径不对）。

---

## 附：阳性对照总表（TC-020-019 汇总用）

| 断言 | 期望 | 阳性对照 | 实测值 |
|---|---|---|---|
| `sqlx::query` | == 0 | `crates/platform-db/src/` | 10 |
| `jwt\|argon2\|hmac` | == 0 | `crates/platform-iam/src/` | 159 |
| `platform_audit_append` | == 0 | `crates/platform-audit/src/sink.rs` | > 0 |
| `require_permission` | > 0 | —（正向断言） | — |
| `NotificationSender` | > 0 | —（正向断言） | — |
