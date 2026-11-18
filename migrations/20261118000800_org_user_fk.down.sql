-- 回滚：去掉用户侧的外键与 `(tenant_id, id)` 唯一约束。
--
-- ⚠️ 回滚后「幽灵 user_id / 跨租户 user_id」又可以写进来（回到修复前的结构）。

ALTER TABLE external_identity DROP CONSTRAINT IF EXISTS external_identity_user_fk;
ALTER TABLE user_position DROP CONSTRAINT IF EXISTS user_position_user_fk;
ALTER TABLE user_org DROP CONSTRAINT IF EXISTS user_org_user_fk;
ALTER TABLE platform_iam_user DROP CONSTRAINT IF EXISTS platform_iam_user_tenant_id_uq;
