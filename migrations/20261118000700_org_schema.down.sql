-- 回滚：删掉组织架构的表。
--
-- ⚠️ 数据会丢（组织树、成员关系、岗位、外部身份、冲突队列、同步状态）。

DROP TABLE IF EXISTS sync_source_state;
DROP TABLE IF EXISTS sync_conflict;
DROP TABLE IF EXISTS external_identity;
DROP TABLE IF EXISTS user_position;
DROP TABLE IF EXISTS user_org;
DROP TABLE IF EXISTS org_node;
