-- 权限点种子：REQ-022 的 6 个 oauth 权限点（D214 / D27 的三件套之一）。
--
-- ⚠️ **只改 manifest 不写种子迁移 → 启动对账 fail-fast，进程起不来**。
-- 对账逻辑见 `crates/platform-rbac/src/registry.rs::require_consistent`
-- 与 `crates/platform-server/src/main.rs::reconcile_permissions`（双向对账，
-- 不一致直接退出，不是告警）。
--
-- `source` 必须是 `'crates/platform-oauth/manifest.json'` ——
-- 它由 `crates/platform-rbac/src/registry.rs::collect_manifest_permissions`
-- 用 `format!("{dir_name}/{name}/manifest.json")` 拼出来（`crates` + 目录名）。
--
-- 幂等：`ON CONFLICT DO UPDATE`。
-- 可逆：配套 20261120000600_oauth_permission_seed.down.sql（只删这 6 行）。

INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('oauth:client:read',     '查看 OAuth 客户端', 'menu',   NULL, 'crates/platform-oauth/manifest.json'),
    ('oauth:client:write',    '管理 OAuth 客户端', 'button', NULL, 'crates/platform-oauth/manifest.json'),
    ('oauth:client:delete',   '删除 OAuth 客户端', 'button', NULL, 'crates/platform-oauth/manifest.json'),
    ('oauth:grant:read',      '查看授权记录',     'menu',   NULL, 'crates/platform-oauth/manifest.json'),
    ('oauth:grant:revoke',    '撤销授权',         'button', NULL, 'crates/platform-oauth/manifest.json'),
    ('oauth:token:introspect','内省令牌',         'button', NULL, 'crates/platform-oauth/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;
