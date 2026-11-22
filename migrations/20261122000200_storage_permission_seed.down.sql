-- 回滚：REQ-018 的 3 个 storage 权限点。
--
-- ⚠️ 权限点是 `platform_permission` 表的行，与 4 张 storage 表无关 ——
-- 但**先删权限点再删表**更安全：若表先没了而权限点还在，
-- 启动对账只会警告；反过来「库里有、manifest 没了」→ 对账直接退出。
--
-- 只删**本迁移**登记的这 3 行，不碰任何其它权限点。

DELETE FROM platform_permission
 WHERE code IN (
    'storage:file:read',
    'storage:file:write',
    'storage:job:read'
 );
