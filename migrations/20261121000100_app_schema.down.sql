-- 应用接入的 2 张表回滚（REQ-016，design §2.1 / §2.2）。
--
-- # 顺序：先删访问记录，再删应用注册表
--
-- 两张表都有指向 `platform_oauth_client` 的外键，但**彼此之间没有** FK →
-- 删除顺序只影响可读性，不影响能否删掉。
--
-- # ⚠️ RLS policy 由 `cargo xtask tenant-policies` 生成（D227）
--
-- 本迁移**不**创建 policy → 回滚时也不需要显式 `DROP POLICY`
-- （`DROP TABLE` 会连带删掉该表上的 policy）。
--
-- ⚠️ 但**已有的 policy**（由 xtask 生成）会随 `DROP TABLE` 一起消失 ——
-- 回滚后若要重新启用，必须**重新跑** `cargo xtask tenant-policies`。
--
-- # ⚠️ 索引随表消失
--
-- 表删掉时索引会**自动**跟着消失 → 不需要单独 DROP INDEX。

DROP TABLE IF EXISTS platform_app_access;
DROP TABLE IF EXISTS platform_app;
