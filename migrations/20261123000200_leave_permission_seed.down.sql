-- 回滚：REQ-020 的 3 个 leave 权限点。
--
-- 只删**本迁移**登记的这 3 行，不碰任何其它权限点。
-- ⚠️ 顺序：先撤 leave 的 manifest（或整个模块）再回滚本迁移 ——
-- 否则「库里有、manifest 没了」会让启动对账直接退出（`require_consistent`）。

DELETE FROM platform_permission
 WHERE code IN (
    'leave:request:read',
    'leave:request:write',
    'leave:request:approve'
 );
