-- 权限点种子：REQ-010 的 4 个 config 权限点（D67 的三件套之一）。
--
-- ⚠️ **只改 manifest 不写种子迁移 → 启动对账 fail-fast，进程起不来**。
-- 对账逻辑见 `crates/platform-rbac/src/registry.rs::require_consistent`
-- 与 `crates/platform-server/src/main.rs::reconcile_permissions`（双向对账，
-- 不一致直接退出，不是告警）。
--
-- 幂等：`ON CONFLICT DO UPDATE`。
-- 可逆：配套 20261118000902_config_permission_seed.down.sql（只删这 4 行）。

INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('config:entry:write',    '写租户级/空间级配置', 'button', NULL, 'crates/platform-config/manifest.json'),
    ('config:flag:write',     '改功能开关',         'button', NULL, 'crates/platform-config/manifest.json'),
    ('config:platform:write', '写平台级配置',       'button', NULL, 'crates/platform-config/manifest.json'),
    ('config:platform:read',  '读平台级配置',       'button', NULL, 'crates/platform-config/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;
