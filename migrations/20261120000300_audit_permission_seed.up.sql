-- 权限点种子：REQ-014 的 3 个 audit 权限点（D27 的三件套之一）。
--
-- ⚠️ **只改 manifest 不写种子迁移 → 启动对账 fail-fast，进程起不来**。
-- 对账逻辑见 `crates/platform-rbac/src/registry.rs::require_consistent`
-- 与 `crates/platform-server/src/main.rs::reconcile_permissions`（双向对账，
-- 不一致直接退出，不是告警）。
--
-- `source` 必须是 `'crates/platform-audit/manifest.json'` ——
-- 它由 `crates/platform-rbac/src/registry.rs::collect_manifest_permissions`
-- 用 `format!("{dir_name}/{name}/manifest.json")` 拼出来（`crates` + 目录名）。
--
-- 幂等：`ON CONFLICT DO UPDATE`。
-- 可逆：配套 20261120000300_audit_permission_seed.down.sql（只删这 3 行）。

INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('audit:events:read',  '查看审计日志', 'menu',   NULL, 'crates/platform-audit/manifest.json'),
    ('audit:verify:run',   '校验审计链',   'button', NULL, 'crates/platform-audit/manifest.json'),
    ('audit:archive:run',  '触发审计归档', 'button', NULL, 'crates/platform-audit/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;
