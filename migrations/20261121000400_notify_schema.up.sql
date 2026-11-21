-- 通知系统的 3 张表（REQ-015，design §2 / D294 / D295 / D299）。
--
-- # 3 张表
--
-- | 表 | 租户维度 | RLS | 作用 |
-- |----|---------|-----|------|
-- | `platform_notify_template`   | **租户级**（有 `tenant_id`） | 启用 | 模板（按 locale 分行存） |
-- | `platform_notify_preference` | **租户级** | 启用 | 订阅偏好（按 category 退订） |
-- | `platform_notify_delivery`   | **租户级** | 启用 | 投递记录（**恰好 18 列**） |
--
-- # ⚠️ RLS policy **不在本迁移里手写**（D227）
--
-- 三张表的 RLS 策略由 `cargo xtask tenant-policies` **生成** ——
-- `crates/platform-notify/manifest.json` 的 `tenantScoped` 段是**唯一来源**。
-- 手写 policy 迟早会漏 —— 漏一张表 = 该表**完全无隔离**（不是「少一个条件」）。
--
-- → AC-S-173 步骤 8 会断言 `tenantScoped.tables` **恰好**是这三张表，
-- 并对 `template` 与 `preference` **各做一次**直连 RLS 验证（D311 / S-04）。
--
-- ⚠️ RLS 用 `ENABLE` 而不是 `FORCE`（`platform-db/src/rls.rs` 的生成物同款）→
-- 表属主（迁移 / 管理员连接）**绕过** RLS（测试的阳性对照依赖这一点）。

-- ---------------------------------------------------------------------------
-- 1. 模板（**租户级**）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_notify_template (
    id              UUID        PRIMARY KEY,
    tenant_id       TEXT        NOT NULL,
    code            TEXT        NOT NULL,
    channel         TEXT        NOT NULL,
    locale          TEXT        NOT NULL,
    category        TEXT        NOT NULL,
    title_template  TEXT        NOT NULL,
    body_template   TEXT        NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE platform_notify_template IS
    '通知模板（REQ-015）；**按 locale 分行存**（D294）—— 同一 code+channel 可有多个语言';

COMMENT ON COLUMN platform_notify_template.locale IS
    'BCP-47 语言标签（zh-CN / en-US）；渲染时按「用户 locale → 租户默认 → 失败」三级回退（D294）';

COMMENT ON COLUMN platform_notify_template.category IS
    '通知分类（system / marketing / transactional）；system **不可退订**（D295）';

COMMENT ON COLUMN platform_notify_template.title_template IS
    '标题模板；只支持 {{variable}} 受限变量替换（D293，**不做**条件/循环/表达式）';

COMMENT ON COLUMN platform_notify_template.body_template IS
    '正文模板；同上';

-- ⚠️ 唯一约束**必须含 locale** —— 否则同一模板无法有第二个语言
-- （AC-S-168 步骤 1 的判据：同一 (code, channel) 的两个 locale 能共存）。
CREATE UNIQUE INDEX IF NOT EXISTS platform_notify_template_unique_idx
    ON platform_notify_template (tenant_id, code, channel, locale);

-- 渲染入口按 (code, channel, locale) 查。
CREATE INDEX IF NOT EXISTS platform_notify_template_lookup_idx
    ON platform_notify_template (tenant_id, code, channel, locale);

-- ---------------------------------------------------------------------------
-- 2. 订阅偏好（**租户级**）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_notify_preference (
    tenant_id   TEXT        NOT NULL,
    user_id     TEXT        NOT NULL,
    category    TEXT        NOT NULL,
    opted_out   BOOLEAN     NOT NULL DEFAULT false,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, user_id, category)
);

COMMENT ON TABLE platform_notify_preference IS
    '订阅偏好（REQ-015 / D295）；按 **category** 退订，不按模板';

COMMENT ON COLUMN platform_notify_preference.category IS
    '通知分类；**不得出现 system 行** —— 强制类不可退订（AC-S-169 步骤 5 会断言行数为 0）';

COMMENT ON COLUMN platform_notify_preference.opted_out IS
    '是否已退订；可逆（再次 PUT false 即恢复，AC-S-169 步骤 7）';

-- ---------------------------------------------------------------------------
-- 3. 投递记录（**租户级**，**恰好 18 列**，D299）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_notify_delivery (
    id                    UUID        PRIMARY KEY,
    tenant_id             TEXT        NOT NULL,
    template_code         TEXT        NOT NULL,
    channel               TEXT        NOT NULL,
    category              TEXT        NOT NULL,
    recipient             TEXT        NOT NULL,
    locale                TEXT        NOT NULL,
    payload               JSONB       NOT NULL DEFAULT '{}'::jsonb,
    rendered_title        TEXT        NOT NULL,
    rendered_body         TEXT        NOT NULL,
    status                TEXT        NOT NULL DEFAULT 'pending',
    attempt_count         INTEGER     NOT NULL DEFAULT 0,
    next_attempt_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error            TEXT,
    provider_message_id   TEXT,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at          TIMESTAMPTZ,
    CONSTRAINT platform_notify_delivery_status_check
        CHECK (status IN ('pending', 'delivering', 'delivered', 'dead_letter'))
);

COMMENT ON TABLE platform_notify_delivery IS
    '投递记录（REQ-015 / D299）；**恰好 18 列**（AC-S-173 步骤 7 逐列钉死）';

COMMENT ON COLUMN platform_notify_delivery.template_code IS
    '来源模板的 code，**不是外键** —— 模板删除后记录仍可追溯';

COMMENT ON COLUMN platform_notify_delivery.rendered_title IS
    '渲染结果**快照**（D299）—— 模板后续修改**不影响**已投递记录（AC-S-173 步骤 2）';

COMMENT ON COLUMN platform_notify_delivery.rendered_body IS
    '渲染结果**快照**；同上';

COMMENT ON COLUMN platform_notify_delivery.payload IS
    '渲染变量（jsonb）；**落库前已过** scan_sensitive_keys（D299 / REQ-014 复用）';

COMMENT ON COLUMN platform_notify_delivery.attempt_count IS
    '已尝试次数（**含首次**）；maxAttempts=5 时第 5 次失败即进死信（D297）';

COMMENT ON COLUMN platform_notify_delivery.next_attempt_at IS
    '下次尝试时间；worker 只取 <= now() 的（AC-S-170 步骤 4）';

-- 出队索引：worker 的 SELECT 走这条（部分索引，只覆盖活跃状态）。
CREATE INDEX IF NOT EXISTS platform_notify_delivery_queue_idx
    ON platform_notify_delivery (tenant_id, status, next_attempt_at)
    WHERE status IN ('pending', 'delivering');

-- 列表查询走这条（新的在前）。
CREATE INDEX IF NOT EXISTS platform_notify_delivery_list_idx
    ON platform_notify_delivery (tenant_id, created_at DESC);

-- ---------------------------------------------------------------------------
-- 4. 应用角色授权（**新表必须显式 GRANT**，不会自动继承）
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_notify_template TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_notify_preference TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_notify_delivery TO base_platform_app';
    END IF;
END
$$;
