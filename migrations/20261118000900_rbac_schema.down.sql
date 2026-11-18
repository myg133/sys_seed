-- 回滚 REQ-009 的平台表（20261118000900_rbac_schema 的逆操作）。
--
-- ⚠️ 顺序：先删依赖方（外键），再删被依赖方。
--
-- `user_org_one_primary_idx` 是**加在 REQ-008 的表上的索引** ——
-- 回滚时一并去掉（它属于本需求的迁移）。

DROP INDEX IF EXISTS user_org_one_primary_idx;

DROP TABLE IF EXISTS platform_permission_epoch;
DROP TABLE IF EXISTS platform_user_role;
DROP TABLE IF EXISTS platform_role_inherit;
DROP TABLE IF EXISTS platform_role_permission;
DROP TABLE IF EXISTS platform_role;
DROP TABLE IF EXISTS platform_permission;
