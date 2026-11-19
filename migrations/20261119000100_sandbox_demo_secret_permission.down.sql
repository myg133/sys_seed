-- 回滚字段级权限点 `sandbox:demo:secret`（REQ-012）。
--
-- ⚠️ 只删**本迁移写入**的编码 —— 别人后来加的权限点不动。

DELETE FROM platform_permission WHERE code IN ('sandbox:demo:secret');
