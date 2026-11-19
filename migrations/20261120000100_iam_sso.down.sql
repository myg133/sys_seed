-- SSO 入站（REQ-013）：回滚。
--
-- 可逆：配套 20261120000100_iam_sso.up.sql。
--
-- 策略：
--   - 本迁移**新建**的两张表 → `DROP TABLE`（它们是本轮才出现的，回滚即消失）；
--   - **既有**表 `platform_iam_session` 只 `DROP COLUMN`，**不删表**
--     （与 REQ-010 对 `platform_tenant_config.value` 的 down 策略一致：
--      降级不应丢既有数据）。

DROP TABLE IF EXISTS platform_iam_sso_ticket;
DROP TABLE IF EXISTS platform_iam_oidc_state;

ALTER TABLE platform_iam_session
    DROP COLUMN IF EXISTS auth_provider;
