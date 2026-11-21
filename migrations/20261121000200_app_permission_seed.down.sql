-- 权限点种子回滚（REQ-016，3 个 app 权限点）。
--
-- 只删**本迁移**登记的那 3 个码 —— 不碰任何其它权限点行。
-- 若某个角色**已授予**这些权限点，`platform_role_permission` 的外键会跟着级联
-- （与其它模块的种子回滚同款）。

DELETE FROM platform_permission
 WHERE code IN (
       'app:registry:read',
       'app:registry:write',
       'app:registry:delete'
 );
