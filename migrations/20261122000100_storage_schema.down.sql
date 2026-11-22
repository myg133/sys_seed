-- 回滚：REQ-018 存储与导入导出的 4 张表（design §2）。
--
-- ⚠️ 顺序：先收授权再删表 —— 删表会连带收回，但显式更清晰，
-- 且部分失败时能看出是哪一步失败（同 REQ-015 的 down）。
--
-- ⚠️ `job_error` 先删：它按 `job_id` 逻辑归属 `job`（**不是外键**），
-- 顺序只是习惯，不删干净会在下次 up 时留下孤儿行。

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'REVOKE ALL ON platform_storage_job_error FROM base_platform_app';
        EXECUTE 'REVOKE ALL ON platform_storage_job FROM base_platform_app';
        EXECUTE 'REVOKE ALL ON platform_storage_upload FROM base_platform_app';
        EXECUTE 'REVOKE ALL ON platform_storage_object FROM base_platform_app';
    END IF;
END
$$;

DROP TABLE IF EXISTS platform_storage_job_error;
DROP TABLE IF EXISTS platform_storage_job;
DROP TABLE IF EXISTS platform_storage_upload;
DROP TABLE IF EXISTS platform_storage_object;
