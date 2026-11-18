-- 回滚 20261118000902_config_permission_seed.up.sql。
--
-- ⚠️ 只删**本迁移写入**的 4 个编码 —— 别人后来加的权限点不动。

DELETE FROM platform_permission WHERE code IN (
    'config:entry:write',
    'config:flag:write',
    'config:platform:write',
    'config:platform:read'
);
