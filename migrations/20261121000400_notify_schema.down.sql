-- 回滚：通知系统的 3 张表（REQ-015）。
--
-- ⚠️ 顺序：**先收授权再删表**（虽然删表会连带收回，但显式更清晰、且可在
-- 部分失败时看出是哪一步）。

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'REVOKE ALL ON platform_notify_delivery FROM base_platform_app';
        EXECUTE 'REVOKE ALL ON platform_notify_preference FROM base_platform_app';
        EXECUTE 'REVOKE ALL ON platform_notify_template FROM base_platform_app';
    END IF;
END
$$;

-- 投递记录先删（它引用 template_code，但**不是**外键 —— 顺序只是习惯）。
DROP TABLE IF EXISTS platform_notify_delivery;
DROP TABLE IF EXISTS platform_notify_preference;
DROP TABLE IF EXISTS platform_notify_template;
