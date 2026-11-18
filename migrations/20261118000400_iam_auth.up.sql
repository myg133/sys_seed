-- 认证与账号体系（REQ-007）：账号 / 会话 / 令牌 / MFA / 幂等键。
--
-- 与 REQ-006 的租户隔离保持同一套约定（L1：共享表 + tenant_id 列 + RLS）：
--
--   * 每张表都带 `tenant_id`；
--   * RLS policy 用 `current_setting('app.tenant_id', true)`（缺失 → NULL → 行不可见，fail closed）；
--   * `WITH CHECK` 一并建上：应用层也写不进别的租户的行。
--
-- 为什么这些表是**平台表**而不是业务模块表：
--   账号体系是底座自身的身份权威源（ADR-0001 A3），所有业务模块的权限模型都挂在它上面，
--   所以它由平台迁移统一建表 + 建 policy，不走模块 manifest 的 `tenantScoped` 声明。
--
-- ⚠️ 密码哈希列存的是 **Argon2id 的 PHC 标准串**（`$argon2id$v=19$m=...,t=...,p=...$salt$hash`），
--    **不是明文**。TOTP secret 存的是 **ChaCha20-Poly1305 密文**（密钥只从环境注入）。
--
-- 可逆：配套 20261118000400_iam_auth.down.sql。

-- ---------------------------------------------------------------------------
-- 账号
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_iam_user (
    id                   TEXT        PRIMARY KEY,
    tenant_id            TEXT        NOT NULL,
    username             TEXT        NOT NULL,
    email                TEXT,
    -- Argon2id 的 PHC 串（**绝不存明文**）
    password_hash        TEXT        NOT NULL,
    -- active | disabled
    status               TEXT        NOT NULL DEFAULT 'active',
    mfa_enabled          BOOLEAN     NOT NULL DEFAULT FALSE,
    -- 首次登录强制改密
    must_change_password BOOLEAN     NOT NULL DEFAULT FALSE,
    -- 登录失败计数与锁定到期时间（账号锁定）
    failed_attempts      INTEGER     NOT NULL DEFAULT 0,
    locked_until         TIMESTAMPTZ,
    password_changed_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT platform_iam_user_status_check
        CHECK (status IN ('active', 'disabled')),
    CONSTRAINT platform_iam_user_username_uq UNIQUE (tenant_id, username)
);

COMMENT ON TABLE platform_iam_user IS '本地账号（身份的唯一权威源）；password_hash 存 Argon2id PHC 串';

-- 邮箱在租户内唯一（未填邮箱的账号不参与唯一性）
CREATE UNIQUE INDEX IF NOT EXISTS platform_iam_user_email_uq
    ON platform_iam_user (tenant_id, email)
    WHERE email IS NOT NULL;

-- 历史密码（最近 N 条，用于「历史不重复」）
CREATE TABLE IF NOT EXISTS platform_iam_password_history (
    id            TEXT        PRIMARY KEY,
    tenant_id     TEXT        NOT NULL,
    user_id       TEXT        NOT NULL,
    password_hash TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE platform_iam_password_history IS '密码历史（最近 N 条；用于 PASSWORD_REUSED 判定）';

CREATE INDEX IF NOT EXISTS platform_iam_password_history_user_idx
    ON platform_iam_password_history (tenant_id, user_id, created_at DESC);

-- ---------------------------------------------------------------------------
-- 设备会话（每个会话一个 refresh family）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_iam_session (
    id           TEXT        PRIMARY KEY,
    tenant_id    TEXT        NOT NULL,
    user_id      TEXT        NOT NULL,
    device_label TEXT        NOT NULL DEFAULT '',
    ip           TEXT        NOT NULL DEFAULT '',
    user_agent   TEXT        NOT NULL DEFAULT '',
    -- 该会话的 refresh token 家族（踢下线 = 作废整个 family）
    family_id    TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at   TIMESTAMPTZ
);

COMMENT ON TABLE platform_iam_session IS '设备会话；踢下线 / 登出全端 = 作废对应的 refresh family';

CREATE INDEX IF NOT EXISTS platform_iam_session_user_idx
    ON platform_iam_session (tenant_id, user_id, created_at DESC);

-- ---------------------------------------------------------------------------
-- refresh token（不透明随机串；**只存 hash**）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_iam_refresh_token (
    id         TEXT        PRIMARY KEY,
    tenant_id  TEXT        NOT NULL,
    user_id    TEXT        NOT NULL,
    session_id TEXT        NOT NULL,
    -- 令牌家族：检测到重放 → 整个 family 作废
    family_id  TEXT        NOT NULL,
    -- SHA-256 摘要（base64url）；**不存明文**
    token_hash TEXT        NOT NULL,
    issued_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    -- 已轮换（used_at 非空 = 旧令牌已作废）
    used_at    TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    replaced_by TEXT
);

COMMENT ON TABLE platform_iam_refresh_token IS 'refresh token（不透明串，只存 hash；轮换与重放检测的依据）';

CREATE UNIQUE INDEX IF NOT EXISTS platform_iam_refresh_token_hash_uq
    ON platform_iam_refresh_token (token_hash);

CREATE INDEX IF NOT EXISTS platform_iam_refresh_token_family_idx
    ON platform_iam_refresh_token (tenant_id, family_id);

-- ---------------------------------------------------------------------------
-- MFA：TOTP secret（密文）+ 恢复码（hash）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_iam_mfa_secret (
    tenant_id      TEXT        NOT NULL,
    user_id        TEXT        NOT NULL,
    -- ChaCha20-Poly1305 密文（base64url）；密钥只从环境注入，不入库
    secret_cipher  TEXT        NOT NULL,
    -- 最近一次成功使用的**时间步**：同一时间步的验证码不可复用（防重放）
    last_used_step BIGINT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, user_id)
);

COMMENT ON TABLE platform_iam_mfa_secret IS 'TOTP secret（静态加密）；last_used_step 用于防重放';

CREATE TABLE IF NOT EXISTS platform_iam_recovery_code (
    id         TEXT        PRIMARY KEY,
    tenant_id  TEXT        NOT NULL,
    user_id    TEXT        NOT NULL,
    -- 恢复码的 SHA-256 摘要；每个只能用一次
    code_hash  TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    used_at    TIMESTAMPTZ
);

COMMENT ON TABLE platform_iam_recovery_code IS 'MFA 恢复码（一次性；只存 hash）';

CREATE INDEX IF NOT EXISTS platform_iam_recovery_code_user_idx
    ON platform_iam_recovery_code (tenant_id, user_id);

-- ---------------------------------------------------------------------------
-- 密码重置
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_iam_password_reset (
    id         TEXT        PRIMARY KEY,
    tenant_id  TEXT        NOT NULL,
    user_id    TEXT        NOT NULL,
    token_hash TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at    TIMESTAMPTZ
);

COMMENT ON TABLE platform_iam_password_reset IS '密码重置令牌（只存 hash；用后即作废）';

CREATE UNIQUE INDEX IF NOT EXISTS platform_iam_password_reset_hash_uq
    ON platform_iam_password_reset (token_hash);

-- ---------------------------------------------------------------------------
-- 登录尝试（按 IP 的滑动窗口限流 + 审计）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_iam_login_attempt (
    id          TEXT        PRIMARY KEY,
    tenant_id   TEXT        NOT NULL,
    ip          TEXT        NOT NULL,
    username    TEXT        NOT NULL,
    succeeded   BOOLEAN     NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE platform_iam_login_attempt IS '登录尝试流水（按 IP 滑动窗口限流的依据）';

CREATE INDEX IF NOT EXISTS platform_iam_login_attempt_window_idx
    ON platform_iam_login_attempt (tenant_id, ip, occurred_at DESC);

-- ---------------------------------------------------------------------------
-- 幂等键（契约 C8：写操作支持 Idempotency-Key，24 小时内重复请求返回首次结果）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_iam_idempotency (
    tenant_id       TEXT        NOT NULL,
    idempotency_key TEXT        NOT NULL,
    method          TEXT        NOT NULL,
    path            TEXT        NOT NULL,
    -- 0 = 处理中；非 0 = 已完成，body 是首次响应体
    status          INTEGER     NOT NULL DEFAULT 0,
    body            TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, idempotency_key)
);

COMMENT ON TABLE platform_iam_idempotency IS '幂等键记录（按租户维度存储，跨租户不共享）';

-- ---------------------------------------------------------------------------
-- RLS：每张表一套（fail closed + WITH CHECK 兜住写侧）
-- ---------------------------------------------------------------------------
DO $$
DECLARE
    target TEXT;
    tables TEXT[] := ARRAY[
        'platform_iam_user',
        'platform_iam_password_history',
        'platform_iam_session',
        'platform_iam_refresh_token',
        'platform_iam_mfa_secret',
        'platform_iam_recovery_code',
        'platform_iam_password_reset',
        'platform_iam_login_attempt',
        'platform_iam_idempotency'
    ];
BEGIN
    FOREACH target IN ARRAY tables LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', target);
        EXECUTE format('DROP POLICY IF EXISTS %I_tenant_isolation ON %I', target, target);
        EXECUTE format(
            'CREATE POLICY %I_tenant_isolation ON %I '
            'USING (tenant_id = current_setting(''app.tenant_id'', true)) '
            'WITH CHECK (tenant_id = current_setting(''app.tenant_id'', true))',
            target,
            target
        );
    END LOOP;
END
$$;

-- ---------------------------------------------------------------------------
-- 授权给应用角色组（若存在）
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT USAGE ON SCHEMA public TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_user TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_password_history TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_session TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_refresh_token TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_mfa_secret TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_recovery_code TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_password_reset TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_login_attempt TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_iam_idempotency TO base_platform_app';
    END IF;
END
$$;
