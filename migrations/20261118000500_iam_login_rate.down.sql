-- 登录失败计数（IP 限流用）：回滚。
--
-- 可逆：配套 20261118000500_iam_login_rate.up.sql。

DROP TABLE IF EXISTS platform_iam_login_rate;
