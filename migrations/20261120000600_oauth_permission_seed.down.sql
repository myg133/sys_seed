-- 权限点种子回滚：只删 REQ-022 的 6 个 oauth 权限点。
--
-- ⚠️ 只删 `source` 指向 `crates/platform-oauth/manifest.json` 的这 6 个码 ——
--    不碰其它模块的权限点。

DELETE FROM platform_permission
 WHERE code IN ('oauth:client:read', 'oauth:client:write', 'oauth:client:delete',
                'oauth:grant:read', 'oauth:grant:revoke', 'oauth:token:introspect')
   AND source = 'crates/platform-oauth/manifest.json';
