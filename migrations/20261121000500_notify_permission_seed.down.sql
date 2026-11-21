-- 回滚：REQ-015 的 3 个 notify 权限点（D300）。
--
-- ⚠️ 必须先于 `20261121000400_notify_schema.down.sql` 或与之无依赖 ——
-- 权限点是 `platform_permission` 表的行，与通知三张表无关。
-- 但**先删权限点再删表**更安全：若表先没了而权限点还在，
-- 启动对账会看到「manifest 里有、库里也有、但代码里没人用」→ 只是警告；
-- 反过来「库里有、manifest 没了」→ 对账直接退出。

DELETE FROM platform_permission
 WHERE code IN (
    'notify:template:read',
    'notify:template:write',
    'notify:delivery:read'
 );
