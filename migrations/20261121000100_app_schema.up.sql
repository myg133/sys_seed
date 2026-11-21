-- 应用接入的 2 张表（REQ-016，design §2.1 / §2.2）。
--
-- # 2 张表
--
-- | 表 | 租户维度 | RLS | 作用 |
-- |----|---------|-----|------|
-- | `platform_app` | **租户级**（有 `tenant_id`） | 启用 | 应用注册表（D262） |
-- | `platform_app_access` | **租户级** | 启用 | 访问记录（最近 / 常用，D268） |
--
-- # ⚠️ `platform_app` **恰好 12 列**（D265 是唯一权威）
--
-- `id` / `tenant_id` / `client_id` / `display_name` / `description` / `category` /
-- `icon_url` / `launch_url` / `sort_order` / `status` / `created_at` / `created_by`
--
-- # ⚠️ **不含**任何 client 的敏感字段（D262 / AC-S-156 步骤 7）
--
-- 明令**不得**出现：`client_secret_hash` / `redirect_uris` / `allowed_grant_types` /
-- `allowed_scopes` / `backchannel_logout_uri` / `frontchannel_logout_uri`。
-- → 「**引用**而非复制」：那些一律读 `platform_oauth_client`。
--
-- # ⚠️ 也**没有** scope 列（D266 / AC-S-159 步骤 5）
--
-- 应用级权限声明是 client 的 `allowed_scopes` 的**只读投影** —— 应用表存一份
-- 就是**第三份真值**（必然漂移）。
--
-- # ⚠️ `client_id` 是外键 + `ON DELETE CASCADE`（D264）
--
-- client 删除 → 应用自动下线（行被删）。用 `RESTRICT` 会让**有应用引用的 client
-- 删不掉** → 改动 REQ-022 已交付的 AC-S-135 步骤 7（删除后 GET 404）。
--
-- ⚠️ RLS 用 `ENABLE` 而不是 `FORCE`（`platform-db/src/rls.rs` 的生成物同款）→
-- 表属主（迁移 / 管理员连接）**绕过** RLS → CASCADE **不会被 RLS 挡住**。
--
-- # ⚠️ RLS policy **不在本迁移里手写**（D227）
--
-- 两张表的 RLS 策略由 `cargo xtask tenant-policies` **生成** ——
-- `crates/platform-app/manifest.json` 的 `tenantScoped` 段是**唯一来源**。
-- 手写 policy 迟早会漏 —— 漏一张表 = 该表**完全无隔离**（不是「少一个条件」）。
--
-- 生成物：
-- ```sql
-- ALTER TABLE <table> ENABLE ROW LEVEL SECURITY;
-- CREATE POLICY <table>_tenant_isolation ON <table>
--   USING (tenant_id = current_setting('app.tenant_id', true))
--   WITH CHECK (tenant_id = current_setting('app.tenant_id', true));
-- ```

-- ---------------------------------------------------------------------------
-- 1. 应用注册表（**租户级**）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_app (
    id           TEXT        PRIMARY KEY,
    tenant_id    TEXT        NOT NULL,
    client_id    TEXT        NOT NULL
        REFERENCES platform_oauth_client (client_id) ON DELETE CASCADE,
    display_name TEXT        NOT NULL,
    description  TEXT,
    category     TEXT        NOT NULL DEFAULT 'general',
    icon_url     TEXT,
    launch_url   TEXT,
    sort_order   INTEGER     NOT NULL DEFAULT 0,
    status       TEXT        NOT NULL DEFAULT 'active',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by   TEXT        NOT NULL
);

COMMENT ON TABLE platform_app IS
    '应用注册表（**租户级**：一个租户接入哪些既有 client，D262）；**引用** client，不复制任何敏感字段';
COMMENT ON COLUMN platform_app.client_id IS
    '引用 REQ-022 的 `platform_oauth_client.client_id`（**ON DELETE CASCADE**：client 删除 → 应用下线，D264）';
COMMENT ON COLUMN platform_app.display_name IS
    '该租户工作台的展示名（**可与 client.name 不同**：同意页永远用 client.name，D262）';
COMMENT ON COLUMN platform_app.icon_url IS
    '图标 URL（**不做文件上传** —— 那是 REQ-018 的范围，D263）；注册时校验 scheme';
COMMENT ON COLUMN platform_app.launch_url IS
    '应用入口 URL（D269）；为空 → **不出现在导航**（工作台仍显示，D279）；**没有** scope 列（D266）';
COMMENT ON COLUMN platform_app.sort_order IS
    '升序排列（`int4`：**数值序**，不是字典序）';
COMMENT ON COLUMN platform_app.status IS
    '`active` / `hidden`；`hidden` 不出现在工作台 / 导航 / 最近 / 常用';

-- 同一租户不能重复接入同一个 client（幂等 upsert）。
CREATE UNIQUE INDEX IF NOT EXISTS platform_app_tenant_client_idx
    ON platform_app (tenant_id, client_id);

-- 同一租户内展示名唯一（工作台上两个同名应用没法区分）。
CREATE UNIQUE INDEX IF NOT EXISTS platform_app_tenant_display_name_idx
    ON platform_app (tenant_id, display_name);

-- 工作台查询走这条（`status` + `category` + `sort_order`）。
CREATE INDEX IF NOT EXISTS platform_app_workspace_idx
    ON platform_app (tenant_id, status, category, sort_order);

-- ---------------------------------------------------------------------------
-- 2. 访问记录（**租户级**）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_app_access (
    tenant_id        TEXT        NOT NULL,
    user_id          TEXT        NOT NULL,
    client_id        TEXT        NOT NULL
        REFERENCES platform_oauth_client (client_id) ON DELETE CASCADE,
    last_accessed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    access_count     BIGINT      NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, user_id, client_id)
);

COMMENT ON TABLE platform_app_access IS
    '应用访问记录（最近 / 常用，D268）；UPSERT 到 `(tenant, user, client)` 唯一行';
COMMENT ON COLUMN platform_app_access.client_id IS
    '用 **client_id 而不是 app_id**（D268：应用行会被级联删除，访问记录属于「用户用过哪个应用」）；同样 `ON DELETE CASCADE` → client 删除后不留孤儿行（S-13）';
COMMENT ON COLUMN platform_app_access.access_count IS
    '累计访问次数（UPSERT 时**累加**，**不**用 `EXCLUDED.access_count` —— 那会把计数重置成 1）';

-- 最近访问：`last_accessed_at DESC`。
CREATE INDEX IF NOT EXISTS platform_app_access_recent_idx
    ON platform_app_access (tenant_id, user_id, last_accessed_at DESC);

-- 常用应用：`access_count DESC`。
CREATE INDEX IF NOT EXISTS platform_app_access_frequent_idx
    ON platform_app_access (tenant_id, user_id, access_count DESC);
