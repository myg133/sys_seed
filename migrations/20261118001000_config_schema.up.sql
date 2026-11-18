-- 配置三表 + 平台级写通道 + 缓存 epoch（REQ-010）。
--
-- # 三张表**分离**，不做「一张表 + 两个可空列」（D46）
--
-- 单表方案下 RLS 必须写成
--   WITH CHECK (tenant_id IS NULL OR tenant_id = current_setting('app.tenant_id', true))
-- 而 `tenant_id IS NULL` 这一支**同时放行了平台级行的插入** → 租户 A 可以 INSERT
-- 一行影响所有租户的配置（写侧跨租越权，比读侧更严重）。
-- 拆表后平台级表**没有 tenant_id 列**，这条语义写不出来。
--
-- # `platform_tenant_config.value`：TEXT → JSONB（D47）
--
-- 配置值有类型（string / number / boolean / array / object）。存 TEXT 要在应用层
-- 双重编码，类型校验也只能落在应用层。jsonb 让类型由数据库保证。
--
-- ⚠️ **down 迁移不得 DROP TABLE platform_tenant_config** —— 它是 REQ-006 建的历史表；
-- `20261118000200_tenant_isolation.down.sql` 的 `DROP TABLE` 是**那张表自己的**回滚，
-- 本迁移只改了它的一列类型，照抄会删掉整张表（AC-S-062 / R8）。
--
-- # 平台级表的写通道（D56 / R12）
--
-- `deploy/seed/02-app-role.sql` 对 public 的所有表 GRANT 了 SELECT/INSERT/UPDATE/DELETE，
-- 且 `ALTER DEFAULT PRIVILEGES` 让**之后新建的表自动继承** —— 所以本迁移必须
-- **显式** REVOKE 直接 DML，否则 AC-S-062 步骤 2 直接失败。
-- 唯一写通道是 `platform_config_upsert`（SECURITY DEFINER），它**同事务**自增
-- `scope_key='platform'` 的 epoch（D71）—— 写进去与失效信号绑在一起。
--
-- # 可逆
--
-- 配套 20261118001000_config_schema.down.sql（先插 3 种 JSON 类型值再 up/down/up，
-- 见 platform-config 的迁移可逆性测试）。

-- ---------------------------------------------------------------------------
-- 1. 平台级配置（无 tenant_id、无 RLS，同 platform_meta）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_config (
    key        TEXT        PRIMARY KEY,
    value      JSONB       NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE platform_config IS '平台级配置（无 tenant_id、无 RLS；直接 DML 已被 REVOKE，写入只走 SECURITY DEFINER 函数）';
COMMENT ON COLUMN platform_config.value IS 'JSONB：类型由数据库保证（string / number / boolean / array / object）';

-- ---------------------------------------------------------------------------
-- 2. 租户级配置：value 从 TEXT 迁到 JSONB（表本身是 REQ-006 建的）
-- ---------------------------------------------------------------------------
ALTER TABLE platform_tenant_config
    ALTER COLUMN value TYPE JSONB USING value::jsonb;

COMMENT ON COLUMN platform_tenant_config.value IS 'JSONB（REQ-010 从 TEXT 迁移；down 时转回 TEXT，但不删表）';

-- ---------------------------------------------------------------------------
-- 3. 空间级配置（tenant_id + space_id 都 NOT NULL；有 RLS）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_space_config (
    tenant_id  TEXT        NOT NULL,
    space_id   TEXT        NOT NULL,
    key        TEXT        NOT NULL,
    value      JSONB       NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, space_id, key)
);

COMMENT ON TABLE platform_space_config IS '空间级配置（L1：共享表 + tenant_id + space_id + RLS；space_id NOT NULL，空串 ≠ 没有空间）';

ALTER TABLE platform_space_config ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS platform_space_config_tenant_isolation ON platform_space_config;
CREATE POLICY platform_space_config_tenant_isolation ON platform_space_config
    USING (tenant_id = current_setting('app.tenant_id', true))
    WITH CHECK (tenant_id = current_setting('app.tenant_id', true));

-- ---------------------------------------------------------------------------
-- 4. 缓存 epoch（`scope_key` 主键：'platform' 或 'tenant:<id>'）
-- ---------------------------------------------------------------------------
--
-- ⚠️ 与 REQ-009 的 `platform_permission_epoch`（主键 tenant_id）**形状不同**，
-- 这是故意的：权限注册表不在运行期变，配置会。平台级配置一变，**所有租户**的缓存
-- 都要失效；`tenant_id` 主键装不下「平台级变更」（只能 O(N) 写或干脆失效不掉）。
CREATE TABLE IF NOT EXISTS platform_config_epoch (
    scope_key  TEXT        PRIMARY KEY,
    epoch      BIGINT      NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE platform_config_epoch IS '配置缓存版本（成对比较 (platform, tenant:<id>)；不要用 max(epoch) —— 会被更高的租户 epoch 掩蔽）';

-- ---------------------------------------------------------------------------
-- 5. 应用角色授权（组不存在时跳过 —— 生产角色由部署流程创建）
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT USAGE ON SCHEMA public TO base_platform_app';

        -- 平台级表：保留 SELECT（平台级行全体可见是设计），**收回**直接 DML。
        -- 这两句必须同时存在：GRANT 只是补建场景的兜底，REVOKE 才是安全边界。
        EXECUTE 'GRANT SELECT ON platform_config TO base_platform_app';
        EXECUTE 'REVOKE INSERT, UPDATE, DELETE ON platform_config FROM base_platform_app';

        -- 空间级表：读写都要（RLS 在数据库层兜底）。
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_space_config TO base_platform_app';

        -- epoch 表：读 + 自增。
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_config_epoch TO base_platform_app';
    END IF;
END
$$;

-- ---------------------------------------------------------------------------
-- 6. 平台级配置的**唯一写通道**：SECURITY DEFINER 函数
-- ---------------------------------------------------------------------------
--
-- 函数体**必须**在同一事务里自增 `platform` epoch（D71）：
-- 只 upsert 配置而不 bump → 写进去了但缓存失效不掉 → 与 AC-S-052 / 053 直接矛盾。
--
-- `SET search_path = public` 是 SECURITY DEFINER 的**必需防护**：
-- 不固定 search_path 时，调用方可以把一个同名表放进自己的 schema 劫持函数内的解析。
CREATE OR REPLACE FUNCTION platform_config_upsert(p_key TEXT, p_value JSONB)
RETURNS VOID
LANGUAGE sql
SECURITY DEFINER
SET search_path = public
AS $$
    INSERT INTO platform_config (key, value, updated_at)
    VALUES (p_key, p_value, now())
    ON CONFLICT (key) DO UPDATE
    SET value = EXCLUDED.value, updated_at = now();

    INSERT INTO platform_config_epoch (scope_key, epoch, updated_at)
    VALUES ('platform', 1, now())
    ON CONFLICT (scope_key) DO UPDATE
    SET epoch = platform_config_epoch.epoch + 1, updated_at = now();
$$;

COMMENT ON FUNCTION platform_config_upsert(TEXT, JSONB) IS '平台级配置的唯一写通道（SECURITY DEFINER；同事务自增 platform epoch）';

REVOKE ALL ON FUNCTION platform_config_upsert(TEXT, JSONB) FROM PUBLIC;
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT EXECUTE ON FUNCTION platform_config_upsert(TEXT, JSONB) TO base_platform_app';
    END IF;
END
$$;
