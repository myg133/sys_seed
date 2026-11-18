-- 账号表的组织侧属性（REQ-008）。
--
-- REQ-008 交付「用户实体」的组织侧字段，挂在这张**账号表**上：
--
-- | 列 | 用途 |
-- |----|------|
-- | `display_name` | 显示名 —— SCIM / LDAP 同步的对象字段 |
-- | `manager_id` | **直属上级** —— 汇报关系是**人的属性**（design-summary D3），不是节点的属性 |
-- | `departed_at` | 离职时间 —— 区分「离职禁用」与「封禁禁用」 |
--
-- ⚠️ **这里不建 RLS policy**：`platform_iam_user` 的 policy 由 REQ-007 的迁移建好，
--    加列不影响它。列的增删也不改变策略定义。
--
-- 可逆：配套 20261118000600_iam_user_org_attrs.down.sql。

ALTER TABLE platform_iam_user ADD COLUMN IF NOT EXISTS display_name TEXT;
ALTER TABLE platform_iam_user ADD COLUMN IF NOT EXISTS manager_id TEXT;
ALTER TABLE platform_iam_user ADD COLUMN IF NOT EXISTS departed_at TIMESTAMPTZ;

COMMENT ON COLUMN platform_iam_user.display_name IS '显示名（REQ-008：SCIM / LDAP 同步的字段）';
COMMENT ON COLUMN platform_iam_user.manager_id IS '直属上级（汇报关系是人的属性，design-summary D3）';
COMMENT ON COLUMN platform_iam_user.departed_at IS '离职时间（非空 = 已离职禁用）';
