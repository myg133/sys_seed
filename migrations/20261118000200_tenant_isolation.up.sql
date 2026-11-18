-- 租户作用域的平台表 + RLS 策略（REQ-006：多租户与隔离）。
--
-- L1 隔离形态：**共享表 + tenant_id 列 + PostgreSQL RLS**。
-- RLS 是数据库层的兜底：即使应用层漏写租户条件，数据库层也不会返回他租数据。
--
-- 为什么 policy 用 `current_setting('app.tenant_id', true)`：
--   第二个参数 true = 缺失时返回 NULL 而不是报错 → 行不可见（**fail closed**）。
--   若写成 `current_setting('app.tenant_id')`，所有未设租户上下文的查询会直接报错。
--
-- 为什么还要 `WITH CHECK`：
--   没有它，应用层可以往表里写入别的租户的行（写得进去、读不出来 —— 静默的写侧泄漏）。
--
-- 业务模块的表**不在这里建 policy** —— 由 manifest 的 `tenantScoped` 段统一生成
-- （`cargo xtask tenant-policies`），避免手写漏建。
--
-- 可逆：配套 20261118000200_tenant_isolation.down.sql。

-- 租户级配置
CREATE TABLE IF NOT EXISTS platform_tenant_config (
    tenant_id TEXT        NOT NULL,
    key        TEXT        NOT NULL,
    value      TEXT        NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, key)
);

COMMENT ON TABLE platform_tenant_config IS '租户级配置（L1：共享表 + tenant_id + RLS）';

ALTER TABLE platform_tenant_config ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS platform_tenant_config_tenant_isolation ON platform_tenant_config;
CREATE POLICY platform_tenant_config_tenant_isolation ON platform_tenant_config
    USING (tenant_id = current_setting('app.tenant_id', true))
    WITH CHECK (tenant_id = current_setting('app.tenant_id', true));

-- 租户配额用量（计数落库：重启、扩容都不丢）
CREATE TABLE IF NOT EXISTS platform_tenant_quota (
    tenant_id   TEXT        NOT NULL,
    metric      TEXT        NOT NULL,
    used        BIGINT      NOT NULL DEFAULT 0,
    quota_limit BIGINT,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, metric)
);

COMMENT ON TABLE platform_tenant_quota IS '租户配额用量（计数落库；配额 ≠ 限流）';

ALTER TABLE platform_tenant_quota ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS platform_tenant_quota_tenant_isolation ON platform_tenant_quota;
CREATE POLICY platform_tenant_quota_tenant_isolation ON platform_tenant_quota
    USING (tenant_id = current_setting('app.tenant_id', true))
    WITH CHECK (tenant_id = current_setting('app.tenant_id', true));

-- 授权给应用角色组（若存在）。
--
-- ⚠️ **RLS 只对不绕过它的角色生效**：超级用户与带 BYPASSRLS 属性的角色会绕过 RLS，
-- 租户隔离会 fail open。所以应用连接必须用**普通角色**（组 `base_platform_app` 的成员）。
-- 组角色不存在时跳过（生产环境的角色由部署流程创建，见 deploy/seed/02-app-role.sql）。
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT USAGE ON SCHEMA public TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_tenant_config TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_tenant_quota TO base_platform_app';
    END IF;
END
$$;
