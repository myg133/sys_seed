-- 回滚 20261118001000_config_schema.up.sql。
--
-- ⚠️ **不得删除 `platform_tenant_config` 表** —— 该表由 REQ-006 建立
-- （`20261118000200_tenant_isolation.up.sql`），本迁移只把它的 `value` 从 TEXT
-- 改成 JSONB。照抄那张表的 down（整表删除）会丢掉整个租户级配置表（R8 / AC-S-062）。
--
-- 顺序：先删函数（`LANGUAGE sql` 的函数体对表有依赖），再删本迁移新建的三张表，
-- 最后把 `platform_tenant_config.value` 转回 TEXT。
--
-- 解档语义：jsonb → text 得到 JSON 文本（字符串值带引号），再 up 时 `value::jsonb`
-- 能**原样**还原类型与值（AC-S-062 步骤 6 的判据）。

DROP FUNCTION IF EXISTS platform_config_upsert(TEXT, JSONB);

DROP TABLE IF EXISTS platform_config_epoch;
DROP TABLE IF EXISTS platform_space_config;
DROP TABLE IF EXISTS platform_config;

ALTER TABLE platform_tenant_config
    ALTER COLUMN value TYPE TEXT USING value::text;
