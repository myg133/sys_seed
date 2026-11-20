-- 出站 IdP 的 5 张表回滚（REQ-022，design §2.1~§2.5）。
--
-- # 顺序：**先删索引随表，再删表**
--
-- 表删掉时索引会**自动**跟着消失 → 不需要单独 DROP INDEX。
-- 但**策略**不会跟着表走干净 → 见下。
--
-- # ⚠️ RLS policy 由 `cargo xtask tenant-policies` 生成（D227）
--
-- 本迁移**不**创建 policy → 回滚时也不需要显式 DROP POLICY
-- （`DROP TABLE` 会连带删掉该表上的 policy）。
--
-- ⚠️ 但**表上已有的 policy**（由 xtask 生成）会随 `DROP TABLE` 一起消失 →
-- 回滚后若要重新启用，必须**重新跑** `cargo xtask tenant-policies`。
--
-- ⚠️ **函数先删、表后删**（契约：先删函数 / 行，再删表）。
-- `DROP TABLE` 也会连带删掉函数，但显式先删更清楚、也不依赖删除顺序。

DROP FUNCTION IF EXISTS platform_oauth_device_code_decide(TEXT, TEXT, TEXT, TEXT);
DROP FUNCTION IF EXISTS platform_oauth_device_code_by_user_code_hash(TEXT);
DROP FUNCTION IF EXISTS platform_oauth_device_code_by_hash(TEXT);
DROP FUNCTION IF EXISTS platform_oauth_code_by_hash(TEXT);

DROP TABLE IF EXISTS platform_oauth_token;
DROP TABLE IF EXISTS platform_oauth_device_code;
DROP TABLE IF EXISTS platform_oauth_code;
DROP TABLE IF EXISTS platform_oauth_grant;
DROP TABLE IF EXISTS platform_oauth_client;
