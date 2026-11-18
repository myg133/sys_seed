-- 回滚权限点注册表的初始内容（REQ-009）。
--
-- ⚠️ 只删**本迁移写入**的那些编码 —— 别人后来加的权限点不动。
DELETE FROM platform_permission WHERE code IN (
    'sandbox:demo:read',
    'sandbox:demo:write',
    'sandbox:scope-item:read',
    'sandbox:scope-item:approve',
    'sandbox:scope-item:export',
    'org:org-node:read',
    'org:org-node:write',
    'org:user-org:read',
    'org:user-org:write',
    'org:user-position:read',
    'org:user-position:write',
    'org:sync-conflict:read',
    'org:sync-conflict:resolve',
    'org:scim-sync:run',
    'rbac:role:read',
    'rbac:role:write',
    'rbac:user-role:write',
    'rbac:permission:read'
);
