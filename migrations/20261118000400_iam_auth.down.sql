-- 认证与账号体系（REQ-007）：回滚。
--
-- 可逆：配套 20261118000400_iam_auth.up.sql。
--
-- ⚠️ 回滚会**永久删除**全部账号、会话与令牌数据 —— 生产环境回滚前必须先备份
--    （scripts/backup.sh）。

DROP TABLE IF EXISTS platform_iam_idempotency;
DROP TABLE IF EXISTS platform_iam_login_attempt;
DROP TABLE IF EXISTS platform_iam_password_reset;
DROP TABLE IF EXISTS platform_iam_recovery_code;
DROP TABLE IF EXISTS platform_iam_mfa_secret;
DROP TABLE IF EXISTS platform_iam_refresh_token;
DROP TABLE IF EXISTS platform_iam_session;
DROP TABLE IF EXISTS platform_iam_password_history;
DROP TABLE IF EXISTS platform_iam_user;
