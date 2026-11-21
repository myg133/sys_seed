-- 权限点种子：REQ-016 的 3 个 app 权限点（D271 / D27 的三件套之一）。
--
-- ⚠️ **只改 manifest 不写种子迁移 → 启动对账 fail-fast，进程起不来**。
-- 对账逻辑见 `crates/platform-rbac/src/registry.rs::require_consistent`
-- 与 `crates/platform-server/src/main.rs::reconcile_permissions`（双向对账，
-- 不一致直接退出，不是告警）。
--
-- `source` 必须是 `'crates/platform-app/manifest.json'` ——
-- 它由 `crates/platform-rbac/src/registry.rs::collect_manifest_permissions`
-- 用 `format!("{dir_name}/{name}/manifest.json")` 拼出来（`crates` + 目录名）。
--
-- # 为什么**恰好 3 个**（D271）
--
-- **工作台 / 导航不进权限点**：它们是**登录用户都能看**的（导航项已按权限点过滤）。
-- 若给它们加权限点，普通用户就打不开工作台了 —— 与「统一入口」的目标冲突。
--
-- 幂等：`ON CONFLICT DO UPDATE`。
-- 可逆：配套 20261121000200_app_permission_seed.down.sql（只删这 3 行）。

INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('app:registry:read',   '查看应用注册表', 'menu',   NULL, 'crates/platform-app/manifest.json'),
    ('app:registry:write',  '管理应用注册表', 'button', NULL, 'crates/platform-app/manifest.json'),
    ('app:registry:delete', '删除应用',       'button', NULL, 'crates/platform-app/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;
