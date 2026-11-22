-- 回滚：REQ-018 的导入目标表 `platform_storage_probe_row`（D333）。
--
-- ⚠️ 顺序：先收授权再删表 —— 与 storage_schema.down.sql 同款口径，
-- 显式收回能在部分失败时看出是哪一步失败。
--
-- ⚠️ 本表**不依赖**其它 4 张 storage 表（`job_id` 不是外键），
-- 所以可以单独回滚；但按「后建先删」的习惯，它应在 storage_schema.down 之前执行。

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'REVOKE ALL ON platform_storage_probe_row FROM base_platform_app';
    END IF;
END
$$;

DROP TABLE IF EXISTS platform_storage_probe_row;
