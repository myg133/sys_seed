# 05 审计 / 权限点 / C-5 / 启动对账（AC-S-164、AC-S-165）

### TC-016-053 注册/修改/删除各产生一条审计记录
- **AC**: AC-S-164 步骤 1-2 ｜ **类型**: 安全 ｜ P0 ｜ 集成（真库 sink）｜ 状态迁移
- **步骤**: `POST` → `PATCH` → `DELETE`，每步后**立即**查（在下一动作前）
- **预期**: `platform_audit_log` 中 `tenant_id='tenant-a'` 且 `object_id=<appId>` 的记录 `action` 依次为 `app.registry.registered` / `app.registry.updated` / `app.registry.deleted`
- **〔AC 修正〕**: 原文用两段式 `app.registered`，与已交付三段式（`oauth.client.registered`）冲突（S-12）

### TC-016-054 每条记录的字段集 / object_type / object_id
- **AC**: AC-S-164 步骤 3 ｜ P0 ｜ 集成 ｜ 边界值
- **预期**: 9 个字段**全部存在**：`actor_id`(== `u-perm-app@example.com`)、`action`、`object_type`、`object_id`(== `appId`)、`ip`、`user_agent`、`created_at`、`prev_hash`、`hash`；`object_type == "platform_app"`（**恰好**，对齐 `oauth_client`）
- **〔AC 修正〕**: 原文 `object_type == "app"` 与已交付 `oauth_client` 口径冲突（S-12）；`ip` 判据须**非空**（列默认 `''`，S-34）

### TC-016-055 哈希链自洽
- **AC**: AC-S-164 ｜ P0 ｜ 集成 ｜ 不变式
- **步骤**: 以 `tenant-a` 的 `RequestContext` 调 `ChainVerifier::new(TenantDb).verify(&ctx)`
- **预期**: `report.ok == true`；`report.broken.is_empty()`；`report.checked >= 3`

### TC-016-056 事件码登记在 `platform-app` 内部常量 + 单测完备性〔AC 补充〕
- **AC**: AC-S-164（D223 同款口径）｜ P1 ｜ 单测 ｜ 静态
- **预期**: `crates/platform-app/src/events.rs` 有 `pub mod code` 与 `pub const ALL_CODES: [&str; 3]`（照 `crates/platform-oauth/src/events.rs:20-53`）；3 个码**逐字**为三段式；单测断言 `ALL_CODES` 无重复、均以 `app.` 开头且含 2 个 `.`

### TC-016-057 capabilities：14 文件 + total 35 + modules 条目 + 3 条权限点
- **AC**: AC-S-165 步骤 1a ｜ 可维护性 ｜ P0 ｜ **命令** ｜ 静态
- **步骤**: `cargo xtask capabilities` → 检查 `capabilities/`
- **预期**: ① `find capabilities -type f | wc -l` == **14**；② `permissions.json` 的 `total == 35`；③ `modules[*]` 里**恰好一个** `module=="app"` 且 `manifest=="crates/platform-app/manifest.json"`；④ 该条目的 `permissions[*].code` **逐字** == `{app:registry:read, app:registry:write, app:registry:delete}`
- **⚠️ 落点要求**：这 4 条必须有 `file:line` 的 `assert!`（新增 `crates/xtask/tests/capabilities_numbers.rs`）—— `capabilities --check` **不断言数值**（S-30）
- **〔AC 修正〕**: 正文的「8 个 + 6 个 sdk」应改为「**7 个固定 + 7 个 sdk = 14**」（S-15）

### TC-016-058 真 C-5 在本 crate 通过
- **AC**: AC-S-165 步骤 1b ｜ P0 ｜ 命令 ｜ 契约
- **步骤**: `cargo test -p platform-app --test contract_c5`
- **预期**: 通过；且测试体内 `EXPECTED_CODES` 是 3 条**逐字**字面量，`scan_auth_calls(&src).codes` == 该集合（照 `crates/platform-oauth/tests/contract_c5.rs` 范式）

### TC-016-059 只改 manifest、不加种子 → 服务起不来 + 日志逐条列编码与源路径
- **AC**: AC-S-165 步骤 2 ｜ 可维护性 ｜ P0 ｜ 命令 ｜ 因果
- **步骤**: 1) 在 `crates/platform-app/manifest.json` 加一个假权限点 `app:probe:read` 2) **不写迁移** 3) 起 `platform-server`（捕获退出码与日志）4) **还原 + `touch` + 重跑确认回到绿**
- **预期**: ① 退出码 **≠ 0**；② 日志含 `app:probe:read` **和** `crates/platform-app/manifest.json`（`ManifestPermission.source` 逐条给出，`platform-rbac/src/registry.rs:100-140`）；③ 还原后 `cargo test` 回到绿
- **证据**: `crates/platform-server/src/main.rs:1127-1155` `reconcile_permissions` 已具备该行为 ✓

### TC-016-060 权限点改成非字面量 → C-5 红
- **AC**: AC-S-165 步骤 3 ｜ P0 ｜ 命令 ｜ 变异（阳性对照）
- **步骤**: 把某个 `require_permission("app:registry:read")` 改成 `require_permission(&code_var)` → `cargo test -p platform-app --test contract_c5` → **还原 + `touch`**
- **预期**: 测试**失败**，消息含「已登记但无鉴权调用点」或 `non_literal_calls`
- **证据**: `crates/platform-contracts/src/authz_scan.rs:36-52` 有 `non_literal_calls`（fail closed）✓

### TC-016-061 放一个叫 `authorize` 的函数 → C-5 红
- **AC**: AC-S-165 步骤 4 ｜ P0 ｜ 命令 ｜ 变异（阳性对照）
- **步骤**: 在 `crates/platform-app/src/` 新建 `mod probe { pub fn authorize() {} }` → 跑 C-5 → **删除 + `touch`**
- **预期**: 测试**失败**，`custom_entries` 非空且指向该处；还原后回绿
- **证据**: 黑名单 8 名含 `authorize`（`authz_scan.rs:40-49`）✓
- **⚠️ 提醒 Dev**: 「应用接入」语境极易写出 `authorize_app_access` / `authorize_external_launch` —— **都会直接判 C-5 失败**（dev-guide §9.4：还原后必须 `touch`）
