-- 回滚：去掉账号表的组织侧属性。
--
-- ⚠️ 数据会丢（显示名 / 汇报关系 / 离职时间）。

ALTER TABLE platform_iam_user DROP COLUMN IF EXISTS departed_at;
ALTER TABLE platform_iam_user DROP COLUMN IF EXISTS manager_id;
ALTER TABLE platform_iam_user DROP COLUMN IF EXISTS display_name;
