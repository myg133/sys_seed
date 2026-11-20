-- 权限点种子回滚：只删 REQ-014 的 3 个 audit 权限点。
--
-- ⚠️ 只删 `source` 指向 `crates/platform-audit/manifest.json` 的这 3 个码 ——
--    不碰其它模块的权限点。

DELETE FROM platform_permission
 WHERE code IN ('audit:events:read', 'audit:verify:run', 'audit:archive:run')
   AND source = 'crates/platform-audit/manifest.json';
