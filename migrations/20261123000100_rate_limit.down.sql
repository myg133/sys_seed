-- 回滚：REQ-019 的限流计数表（D337）。
--
-- ⚠️ 顺序：先收授权（含序列）再删表 —— 与 storage 的 down 同款口径，
-- 显式收回能在部分失败时看出是哪一步失败。
--
-- ⚠️ 序列（`platform_rate_limit_id_seq`）随表删除自动消失，
-- 但显式 REVOKE 能保证「角色没有残留的序列权限」。

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'REVOKE ALL ON platform_rate_limit FROM base_platform_app';
        -- 序列可能已随表不存在 → 用 IF EXISTS 保护
        IF EXISTS (SELECT 1 FROM pg_class WHERE relname = 'platform_rate_limit_id_seq') THEN
            EXECUTE 'REVOKE ALL ON SEQUENCE platform_rate_limit_id_seq FROM base_platform_app';
        END IF;
    END IF;
END
$$;

DROP TABLE IF EXISTS platform_rate_limit;
