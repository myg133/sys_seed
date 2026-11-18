-- 登录失败计数（IP 限流用，REQ-007 修复轮）。
--
-- # 为什么单独一张表，而不是复用 `platform_iam_login_attempt`
--
-- | 表 | 内容 | 租户维度 |
-- |----|------|---------|
-- | `platform_iam_login_attempt` | 审计流水（含 `username` —— **租户内**数据） | 有 `tenant_id` + RLS |
-- | `platform_iam_login_rate` | **失败**计数（只有 IP 摘要与时间戳） | **没有**租户维度 |
--
-- 限流预算必须**跨租户共享**：同一个 IP 换租户不应该拿到一份新的额度，
-- 否则攻击者只要把请求散到多个租户，限流预算就被放大成「租户数倍」
-- （QA 报告 LOW 6）。而「跨租户计数」在 RLS 下做不到 —— 租户作用域表
-- 只能看见自己租的行。
--
-- 所以这张表**故意不带 `tenant_id`、不开 RLS**：
--
-- - 它**不含任何租户数据**（没有用户名、没有账号、没有租户标识）；
-- - IP 存的是 **SHA-256 摘要**（不是明文 IP）—— 全局可读的表里不留明文地址；
-- - 只记**失败**尝试 —— 成功登录不进入失败窗口（QA 报告 LOW 6 第二条：
--   共用 NAT 出口的办公网不该被正常登录误伤）。
--
-- 可逆：配套 20261118000500_iam_login_rate.down.sql。

CREATE TABLE IF NOT EXISTS platform_iam_login_rate (
    id          TEXT        PRIMARY KEY,
    -- IP 的 SHA-256 摘要（base64url）—— 不是明文 IP
    ip_hash     TEXT        NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE platform_iam_login_rate IS '登录失败计数（IP 全局限流用；无租户维度，只存 IP 摘要）';

CREATE INDEX IF NOT EXISTS platform_iam_login_rate_window_idx
    ON platform_iam_login_rate (ip_hash, occurred_at DESC);

-- ---------------------------------------------------------------------------
-- 授权给应用角色组（若存在）
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_login_rate TO base_platform_app';
    END IF;
END
$$;
