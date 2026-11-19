-- SSO 入站（OIDC + LDAP）与强制 SSO（REQ-013）。
--
-- 三件事：
--   1. `platform_iam_oidc_state` —— 授权请求的一次性凭证（state / nonce / PKCE verifier）
--   2. `platform_iam_sso_ticket`  —— 回调 → 前端的一次性交接票据（**只存 session_id，不存 token**）
--   3. `platform_iam_session.auth_provider` —— 会话来源（local / oidc / ldap）
--
-- # 为什么两张新表**不启用 RLS**
--
-- 两张表的共同特征：**写入/读取时都还没有权威的认证上下文**。
--
-- | 表 | 场景 | 拿不到租户的原因 |
-- |----|------|-----------------|
-- | `platform_iam_oidc_state` | IdP 回调（顶层浏览器导航） | 浏览器带不上 `x-tenant-id`，租户只能由 `state` 反查 |
-- | `platform_iam_sso_ticket`  | 前端换票（`POST /sso/exchange`） | 此刻用户还没令牌 |
--
-- RLS 要求 `app.tenant_id` 已知；在拿到行之前**不可能知道**，所以 RLS 在这里只能写成
-- 「恒不可见」或「恒可见」，等于没有。安全由**不可猜的高熵主键**保证
-- （`state` / `ticket` 都是 base64url(32B)），租户校验在应用层做：
-- `state.tenant_id` / `ticket.tenant_id` 必须与请求租户一致（有头时），否则拒绝。
--
-- ⚠️ 正因为**不开 RLS**，`GRANT` 就必须显式给 —— 否则应用角色 INSERT 直接
--    `permission denied`。写法照 20261118000500_iam_login_rate.up.sql（带角色存在性守卫）。
--
-- 可逆：配套 20261120000100_iam_sso.down.sql

-- ---------------------------------------------------------------------------
-- 1. OIDC 授权请求的一次性凭证
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_iam_oidc_state (
    -- base64url(32B)，不可猜 → 它就是「回调时的会话凭证」
    state          TEXT        PRIMARY KEY,
    tenant_id      TEXT        NOT NULL,
    -- 协议名（本轮固定 'oidc'；将来多 IdP 时区分）
    provider       TEXT        NOT NULL,
    nonce          TEXT        NOT NULL,
    -- PKCE 的 verifier **明文**存：换 token 时必须原样发出，无法只存摘要
    code_verifier  TEXT        NOT NULL,
    -- PKCE 的 challenge（S256）。用于在**换 token 之前**发现 verifier 被篡改
    -- （SSO_OIDC_PKCE_MISMATCH）；没有它，PKCE 篡改与「code 无效」在响应上不可区分。
    code_challenge TEXT        NOT NULL,
    -- 回调时必须与授权时**逐字一致**
    redirect_uri   TEXT        NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 代码常量 TTL（300 秒）—— **不**新增配置键（P9）
    expires_at     TIMESTAMPTZ NOT NULL,
    -- 非空 = 已用过（一次性）
    consumed_at    TIMESTAMPTZ
);

COMMENT ON TABLE platform_iam_oidc_state IS 'OIDC 授权请求的一次性凭证（state/nonce/PKCE；不启用 RLS，靠高熵主键 + 应用层租户校验）';

CREATE INDEX IF NOT EXISTS platform_iam_oidc_state_expiry_idx
    ON platform_iam_oidc_state (expires_at);

CREATE INDEX IF NOT EXISTS platform_iam_oidc_state_tenant_idx
    ON platform_iam_oidc_state (tenant_id, provider);

-- ---------------------------------------------------------------------------
-- 2. 回调 → 前端的一次性交接票据（P4 / D124 决策 ①）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_iam_sso_ticket (
    -- base64url(32B)
    ticket       TEXT        PRIMARY KEY,
    tenant_id    TEXT        NOT NULL,
    user_id      TEXT        NOT NULL,
    -- 已建好的会话；exchange 时**复用**同一套签发逻辑重新取令牌
    -- ⚠️ **不存 token**：token 绝不进 URL、绝不进数据库
    session_id   TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 代码常量 TTL（30 秒）
    expires_at   TIMESTAMPTZ NOT NULL,
    consumed_at  TIMESTAMPTZ
);

COMMENT ON TABLE platform_iam_sso_ticket IS 'SSO 登录票据（回调 302 携带；TTL 30 秒、一次性、只存 session_id）';

CREATE INDEX IF NOT EXISTS platform_iam_sso_ticket_expiry_idx
    ON platform_iam_sso_ticket (expires_at);

-- ---------------------------------------------------------------------------
-- 3. 会话来源
-- ---------------------------------------------------------------------------
ALTER TABLE platform_iam_session
    ADD COLUMN IF NOT EXISTS auth_provider TEXT NOT NULL DEFAULT 'local';

-- 取值：'local' | 'oidc' | 'ldap'。
-- 不建外键：provider 不是实体表，是一组配置。
-- 不建 CHECK：既有行（含将来的其他来源）不应因枚举收窄而写不进去；
-- 取值由 `platform_iam::sso::AuthProvider` 收口。

-- ---------------------------------------------------------------------------
-- 授权给应用角色组（若存在）
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_oidc_state TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_sso_ticket TO base_platform_app';
        -- 既有表新增列的权限随表走（表级 GRANT 已覆盖列）—— 这里只补一句话说明：
        -- platform_iam_session 早在 20261118000400_iam_auth.up.sql 已授权。
    END IF;
END
$$;
