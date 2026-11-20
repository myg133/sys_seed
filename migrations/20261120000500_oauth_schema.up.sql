-- 出站 IdP 的 5 张表（REQ-022，design §2.1~§2.5）。
--
-- # 5 张表
--
-- | 表 | 租户维度 | RLS | 作用 |
-- |----|---------|-----|------|
-- | `platform_oauth_client` | **平台级**（无 `tenant_id`） | **不启用** | 第三方应用（client）注册表（D219） |
-- | `platform_oauth_grant` | 有 | 启用 | 同意记录 / 撤销依据 / 登出定位（**一表三用**，D202） |
-- | `platform_oauth_code` | 有 | 启用 | 授权码（**交换后即删** → 天然一次性） |
-- | `platform_oauth_device_code` | 有 | 启用 | 设备码 |
-- | `platform_oauth_token` | 有 | 启用 | 令牌状态（支撑「撤销立即生效」，D210） |
--
-- # ⚠️ RLS policy **不在本迁移里手写**（D227）
--
-- 4 张租户表的 **RLS 策略由 `cargo xtask tenant-policies` 生成** ——
-- `crates/platform-oauth/manifest.json` 的 `tenantScoped` 段是**唯一来源**
-- （与 `platform-org` 同款做法）。
--
-- 生成物：
--
-- ```sql
-- ALTER TABLE <table> ENABLE ROW LEVEL SECURITY;
-- CREATE POLICY <table>_tenant_isolation ON <table>
--   USING (tenant_id = current_setting('app.tenant_id', true))
--   WITH CHECK (tenant_id = current_setting('app.tenant_id', true));
-- ```
--
-- ⚠️ **手写 policy 迟早会漏** —— 漏一张表 = 该表**完全无隔离**（不是「少一个条件」，
-- 是「谁都能查」）。→ 所以本迁移**只建表**；策略由平台统一生成
-- （`crates/platform-db/src/rls.rs`）。
--
-- # ⚠️ 只存**摘要**的列（D228）
--
-- `client_secret_hash` / `code_hash` / `device_code_hash` / `user_code_hash`
-- 一律存 **`digest_base64url`**（SHA-256 原始 32 字节 → base64url 无填充 → **43 字符**），
-- **不是** hex，也**不是**明文。
--
-- # ⚠️ `platform_oauth_code` **没有** `consumed_at` / `used_at`（D253）
--
-- 「删除即消费」—— 没有「已消费」标记列 → 一次性是**结构上**成立的。

-- ---------------------------------------------------------------------------
-- 1. client（**平台级**：无 tenant_id、**不启用 RLS**）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_oauth_client (
    client_id               TEXT        PRIMARY KEY,
    client_secret_hash      TEXT,
    client_type             TEXT        NOT NULL,
    name                    TEXT        NOT NULL,
    redirect_uris           TEXT[]      NOT NULL DEFAULT '{}',
    allowed_grant_types     TEXT[]      NOT NULL DEFAULT '{}',
    allowed_scopes          TEXT[]      NOT NULL DEFAULT '{}',
    backchannel_logout_uri  TEXT,
    frontchannel_logout_uri TEXT,
    status                  TEXT        NOT NULL DEFAULT 'active',
    expires_at              TIMESTAMPTZ,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by              TEXT
);

COMMENT ON TABLE platform_oauth_client IS 'OAuth 客户端注册表（**平台级**：所有租户共享同一批 client，D219）';
COMMENT ON COLUMN platform_oauth_client.client_secret_hash IS 'secret 的 **base64url 无填充** SHA-256 摘要（D228）；public client 为 NULL';
COMMENT ON COLUMN platform_oauth_client.redirect_uris IS '回调白名单（**精确匹配** → 防开放重定向）';
COMMENT ON COLUMN platform_oauth_client.backchannel_logout_uri IS 'back-channel 登出回调（注册时校验 scheme → **无 SSRF 入口**，D220）';
COMMENT ON COLUMN platform_oauth_client.frontchannel_logout_uri IS 'front-channel 登出回调（注册时校验 scheme）';
COMMENT ON COLUMN platform_oauth_client.expires_at IS '过期时刻；NULL = 不过期';

CREATE INDEX IF NOT EXISTS platform_oauth_client_status_idx
    ON platform_oauth_client (status);

-- ---------------------------------------------------------------------------
-- 2. grant（同意记录 / 撤销依据 / 登出定位）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_oauth_grant (
    tenant_id  TEXT        NOT NULL,
    id         TEXT        PRIMARY KEY,
    client_id  TEXT        NOT NULL,
    user_id    TEXT        NOT NULL,
    scopes     TEXT[]      NOT NULL DEFAULT '{}',
    status     TEXT        NOT NULL DEFAULT 'active',
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    consent_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);

COMMENT ON TABLE platform_oauth_grant IS '同意记录（**一表三用**：撤销依据 + 登出定位 + 内省的 grant 判定）';
COMMENT ON COLUMN platform_oauth_grant.scopes IS '**实际授予**的 scope（不是请求的 scope）';
COMMENT ON COLUMN platform_oauth_grant.consent_at IS '用户点「允许」的时刻（**不是**发令牌的时刻，design §4.2）';

-- 同一租户 + client + user 只有一条 → 再次授权 = 更新（幂等）。
CREATE UNIQUE INDEX IF NOT EXISTS platform_oauth_grant_scope_idx
    ON platform_oauth_grant (tenant_id, client_id, user_id);

CREATE INDEX IF NOT EXISTS platform_oauth_grant_user_idx
    ON platform_oauth_grant (tenant_id, user_id, status);

-- ---------------------------------------------------------------------------
-- 3. code（授权码）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_oauth_code (
    tenant_id             TEXT        NOT NULL,
    id                    TEXT        PRIMARY KEY,
    client_id             TEXT        NOT NULL,
    user_id               TEXT        NOT NULL,
    code_hash             TEXT        NOT NULL,
    redirect_uri          TEXT        NOT NULL,
    scope                 TEXT        NOT NULL DEFAULT '',
    code_challenge        TEXT        NOT NULL DEFAULT '',
    code_challenge_method TEXT        NOT NULL DEFAULT 'S256',
    expires_at            TIMESTAMPTZ NOT NULL
);

COMMENT ON TABLE platform_oauth_code IS '授权码（**只存摘要**；交换后即删 → 天然一次性）';
COMMENT ON COLUMN platform_oauth_code.code_hash IS '授权码的 **base64url 无填充** SHA-256 摘要（D228）；**不是**明文、**不是** hex';
COMMENT ON COLUMN platform_oauth_code.code_challenge_method IS 'PKCE 挑战方法（**只有** `S256`，D206）';
COMMENT ON COLUMN platform_oauth_code.expires_at IS '过期时刻（签发 + `oauth.authorizationCodeTtlSeconds`，默认 60 秒）';

-- ⚠️ **没有** `consumed_at` / `used_at`（D253）：删除即消费，没有标记列。
CREATE UNIQUE INDEX IF NOT EXISTS platform_oauth_code_hash_idx
    ON platform_oauth_code (tenant_id, code_hash);

-- ---------------------------------------------------------------------------
-- 4. device_code（设备码）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_oauth_device_code (
    tenant_id              TEXT        NOT NULL,
    id                     TEXT        PRIMARY KEY,
    client_id              TEXT        NOT NULL,
    device_code_hash       TEXT        NOT NULL,
    user_code_hash         TEXT        NOT NULL,
    scope                  TEXT        NOT NULL DEFAULT '',
    status                 TEXT        NOT NULL DEFAULT 'pending',
    user_id                TEXT,
    device_code_expires_at TIMESTAMPTZ NOT NULL,
    user_code_expires_at   TIMESTAMPTZ NOT NULL,
    poll_interval_seconds  INTEGER     NOT NULL DEFAULT 5,
    last_polled_at         TIMESTAMPTZ
);

COMMENT ON TABLE platform_oauth_device_code IS '设备码（`device_code` 与 `user_code` **都只存摘要**，D228）';
COMMENT ON COLUMN platform_oauth_device_code.status IS '`pending` / `approved` / `denied`';
COMMENT ON COLUMN platform_oauth_device_code.poll_interval_seconds IS '当前轮询间隔（秒）；`slow_down` 时 **+5**（RFC 8628 §3.5）';

CREATE UNIQUE INDEX IF NOT EXISTS platform_oauth_device_code_hash_idx
    ON platform_oauth_device_code (tenant_id, device_code_hash);

CREATE UNIQUE INDEX IF NOT EXISTS platform_oauth_device_code_user_code_idx
    ON platform_oauth_device_code (tenant_id, user_code_hash);

-- ---------------------------------------------------------------------------
-- 5. token（令牌状态）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_oauth_token (
    tenant_id  TEXT        NOT NULL,
    id         TEXT        PRIMARY KEY,
    client_id  TEXT        NOT NULL,
    user_id    TEXT,
    token_id   TEXT        NOT NULL,
    scopes     TEXT[]      NOT NULL DEFAULT '{}',
    grant_id   TEXT,
    status     TEXT        NOT NULL DEFAULT 'active',
    issued_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ
);

COMMENT ON TABLE platform_oauth_token IS 'OAuth 令牌状态（内省 = 验签 + 查本表 + 查 grant 状态）';
COMMENT ON COLUMN platform_oauth_token.token_id IS '令牌的 `jti`（全局唯一）';
COMMENT ON COLUMN platform_oauth_token.user_id IS '`client_credentials` 签发的令牌**没有**用户（NULL）';
COMMENT ON COLUMN platform_oauth_token.grant_id IS '关联的 grant；grant 被撤销 → 该 grant 下**所有**令牌失效（D210）';

CREATE UNIQUE INDEX IF NOT EXISTS platform_oauth_token_token_id_idx
    ON platform_oauth_token (tenant_id, token_id);

CREATE INDEX IF NOT EXISTS platform_oauth_token_grant_idx
    ON platform_oauth_token (tenant_id, grant_id);

-- ---------------------------------------------------------------------------
-- 6. 两个 **SECURITY DEFINER** 查询函数（协议端点的租户来源）
-- ---------------------------------------------------------------------------
--
-- # 为什么需要
--
-- 协议端点（`/token` / `/introspect` / `/revoke`）**没有用户会话**：
-- 调用方是 client 的后端服务，它**不知道**授权码 / 设备码落在哪个租户。
-- 而 4 张租户表都**启用 RLS** → 用应用角色按摘要查行时，
-- 上下文里没有正确租户就**一行都看不见**（fail closed）。
--
-- → 这两个函数让协议端点能**按摘要**（不可猜的密钥）跨租户定位到那一行，
-- 并从返回值里拿到**它自己的租户**，随后的读写就都在那个租户里做。
--
-- # 为什么这**不是**越权
--
-- - 调用方必须知道 **43 字符的摘要**（SHA-256(base64url) 的原始密钥）→ 不可枚举；
-- - 函数**只返回那一行**（不给列表、不给统计）；
-- - 拿到的 `tenant_id` 只用于**后续在该租户内**读写 —— 后续每一步仍然过 RLS。
--
-- # ⚠️ `search_path` 钉死
--
-- SECURITY DEFINER 函数默认用**函数的属主**的 `search_path` →
-- 属主的搜索路径被污染时会把调用导向**别的表** → 必须 `SET search_path = public`。
--
-- # ⚠️ 回滚
--
-- 配套的 `.down.sql` **先删函数、再删表**（删表会连带删掉函数，但显式删更清楚）。

-- 按摘要查授权码（返回整行，含 `tenant_id`）。
CREATE OR REPLACE FUNCTION platform_oauth_code_by_hash(p_code_hash TEXT)
RETURNS TABLE (
    tenant_id             TEXT,
    id                    TEXT,
    client_id             TEXT,
    user_id               TEXT,
    code_hash             TEXT,
    redirect_uri          TEXT,
    scope                 TEXT,
    code_challenge        TEXT,
    code_challenge_method TEXT,
    expires_at            TIMESTAMPTZ
)
LANGUAGE sql
SECURITY DEFINER
SET search_path = public
AS $$
    SELECT c.tenant_id, c.id, c.client_id, c.user_id, c.code_hash, c.redirect_uri, c.scope,
           c.code_challenge, c.code_challenge_method, c.expires_at
      FROM platform_oauth_code c
     WHERE c.code_hash = p_code_hash
     LIMIT 1;
$$;

COMMENT ON FUNCTION platform_oauth_code_by_hash(TEXT) IS
    '按摘要查授权码（SECURITY DEFINER：协议端点没有租户上下文，摘要不可猜）';

-- 按摘要查设备码（返回整行，含 `tenant_id`）。
CREATE OR REPLACE FUNCTION platform_oauth_device_code_by_hash(p_device_code_hash TEXT)
RETURNS TABLE (
    tenant_id              TEXT,
    id                     TEXT,
    client_id              TEXT,
    device_code_hash       TEXT,
    user_code_hash         TEXT,
    scope                  TEXT,
    status                 TEXT,
    user_id                TEXT,
    device_code_expires_at TIMESTAMPTZ,
    user_code_expires_at   TIMESTAMPTZ,
    poll_interval_seconds  INTEGER,
    last_polled_at         TIMESTAMPTZ
)
LANGUAGE sql
SECURITY DEFINER
SET search_path = public
AS $$
    SELECT d.tenant_id, d.id, d.client_id, d.device_code_hash, d.user_code_hash, d.scope,
           d.status, d.user_id, d.device_code_expires_at, d.user_code_expires_at,
           d.poll_interval_seconds, d.last_polled_at
      FROM platform_oauth_device_code d
     WHERE d.device_code_hash = p_device_code_hash
     LIMIT 1;
$$;

COMMENT ON FUNCTION platform_oauth_device_code_by_hash(TEXT) IS
    '按摘要查设备码（SECURITY DEFINER：协议端点没有租户上下文，摘要不可猜）';

-- 按 **user_code** 摘要查设备码（平台端点「批准前展示」与「批准」用）。
--
-- ⚠️ 与 `platform_oauth_device_code_by_hash` 的区别：查的是**用户短码**的摘要
-- （用户是在设备码页**手输**短码的）→ 轮询端点与批准端点走的是两个摘要。
CREATE OR REPLACE FUNCTION platform_oauth_device_code_by_user_code_hash(p_user_code_hash TEXT)
RETURNS TABLE (
    tenant_id              TEXT,
    id                     TEXT,
    client_id              TEXT,
    device_code_hash       TEXT,
    user_code_hash         TEXT,
    scope                  TEXT,
    status                 TEXT,
    user_id                TEXT,
    device_code_expires_at TIMESTAMPTZ,
    user_code_expires_at   TIMESTAMPTZ,
    poll_interval_seconds  INTEGER,
    last_polled_at         TIMESTAMPTZ
)
LANGUAGE sql
SECURITY DEFINER
SET search_path = public
AS $$
    SELECT d.tenant_id, d.id, d.client_id, d.device_code_hash, d.user_code_hash, d.scope,
           d.status, d.user_id, d.device_code_expires_at, d.user_code_expires_at,
           d.poll_interval_seconds, d.last_polled_at
      FROM platform_oauth_device_code d
     WHERE d.user_code_hash = p_user_code_hash
     LIMIT 1;
$$;

COMMENT ON FUNCTION platform_oauth_device_code_by_user_code_hash(TEXT) IS
    '按用户短码摘要查设备码（SECURITY DEFINER：设备码行在批准前还没有租户）';

-- 设备码的**批准 / 拒绝**（跨租户写 + **绑定租户**）。
--
-- # 为什么必须跨租户
--
-- 设备码行在**申请**时还不知道用户属于哪个租户 → 它落在 OAuth 自己的保留伪租户里。
-- 而**批准**发生在用户的租户里 → 正常 UPDATE 会被 RLS 挡住
-- （`USING` 看的是**旧行**的 `tenant_id`，它不在用户的租户里 → 一行都看不见）。
--
-- → 用 SECURITY DEFINER 做这次写入，并在**批准**时把行**迁到用户的租户**：
-- 迁完之后，轮询换令牌、grant 级撤销都落在**同一个租户**里。
--
-- # 返回值
--
-- `TRUE` = 有一行被改了；`FALSE` = 没有 `pending` 且未过期的行
-- （已被别人用过 / 已过期 / 不存在）。
--
-- # ⚠️ 参数
--
-- - `p_action`：只认 `'approve'` / `'deny'`（其它值按 `deny` 处理）；
-- - `p_user_id` / `p_tenant_id`：只在 `approve` 时写入。
CREATE OR REPLACE FUNCTION platform_oauth_device_code_decide(
    p_user_code_hash TEXT,
    p_action         TEXT,
    p_user_id        TEXT,
    p_tenant_id      TEXT
)
RETURNS BOOLEAN
LANGUAGE sql
SECURITY DEFINER
SET search_path = public
AS $$
    WITH updated AS (
        UPDATE platform_oauth_device_code d
           SET status = CASE WHEN p_action = 'approve' THEN 'approved' ELSE 'denied' END,
               user_id = CASE WHEN p_action = 'approve' THEN p_user_id ELSE NULL END,
               tenant_id = CASE WHEN p_action = 'approve' THEN p_tenant_id ELSE d.tenant_id END
         WHERE d.user_code_hash = p_user_code_hash
           AND d.status = 'pending'
           AND d.user_code_expires_at > now()
        RETURNING 1
    )
    SELECT EXISTS (SELECT 1 FROM updated);
$$;

COMMENT ON FUNCTION platform_oauth_device_code_decide(TEXT, TEXT, TEXT, TEXT) IS
    '设备码的批准 / 拒绝（SECURITY DEFINER：跨租户写，并在批准时把行迁到用户的租户）';

-- ⚠️ **先收回 PUBLIC 的默认 EXECUTE**（新函数默认对 PUBLIC 可执行）。
--
-- 与 `platform_config_upsert` / `platform_audit_append` 同一口径（契约 C12 的
-- 「最小权限」）：只有**应用角色组**能调用这两个跨租户查询函数。
REVOKE ALL ON FUNCTION platform_oauth_code_by_hash(TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_oauth_device_code_by_hash(TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_oauth_device_code_by_user_code_hash(TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_oauth_device_code_decide(TEXT, TEXT, TEXT, TEXT) FROM PUBLIC;

-- 应用角色需要能调用这两个函数。
--
-- ⚠️ **角色可能还不存在**（首次建库时迁移先于角色种子）→ 用 `pg_roles` 守卫，
-- 与 `02-app-role.sql` / `platform_config_upsert` 的写法一致。
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT EXECUTE ON FUNCTION platform_oauth_code_by_hash(TEXT) TO base_platform_app';
        EXECUTE 'GRANT EXECUTE ON FUNCTION platform_oauth_device_code_by_hash(TEXT) TO base_platform_app';
        EXECUTE 'GRANT EXECUTE ON FUNCTION platform_oauth_device_code_by_user_code_hash(TEXT) TO base_platform_app';
        EXECUTE 'GRANT EXECUTE ON FUNCTION platform_oauth_device_code_decide(TEXT, TEXT, TEXT, TEXT) TO base_platform_app';
    END IF;
END
$$;
