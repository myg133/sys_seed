-- 审计三表的回滚（REQ-014）。
--
-- 回滚顺序与建表顺序相反：先删函数（它们依赖表），再删表。
--
-- ⚠️ 本迁移**只**删自己建的表 —— 不碰任何其它需求的表。
--
-- 🔑 `tip_hash`（D171）是 `platform_audit_chain_state` 的**一列** ——
-- 回滚时随表一起消失（不必单独 `DROP COLUMN`）。

DROP FUNCTION IF EXISTS platform_audit_archive_move(TEXT, TIMESTAMPTZ);
DROP FUNCTION IF EXISTS platform_audit_tip_hash(TEXT);
DROP FUNCTION IF EXISTS platform_audit_head_hash(TEXT);
DROP FUNCTION IF EXISTS platform_audit_append(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT);
DROP FUNCTION IF EXISTS platform_audit_sensitive_keys(TEXT);
DROP FUNCTION IF EXISTS platform_audit_normalize_key(TEXT);
DROP FUNCTION IF EXISTS platform_audit_canonical_hash(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT);

DROP TABLE IF EXISTS platform_audit_chain_state;
DROP TABLE IF EXISTS platform_audit_archive;
DROP TABLE IF EXISTS platform_audit_log;
