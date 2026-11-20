-- 审计配置项默认值的回滚（REQ-014 D178）。
--
-- 只删**本迁移**登记的那 4 个键 —— 不碰任何其它配置行。
--
-- ⚠️ `platform_config` 的直接 DML 已被 REVOKE（REQ-010 D56），
-- 但**迁移跑在表 owner 上** → 可以直接 DELETE（回滚是 owner 权限的活）。

DELETE FROM platform_config
 WHERE key IN (
       'audit.retention.onlineDays',
       'audit.retention.archiveDays',
       'audit.page.defaultSize',
       'audit.page.maxSize'
 );
