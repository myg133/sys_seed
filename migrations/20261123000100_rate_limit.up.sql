-- REQ-019 三级限流的**滑动窗口计数表**（D337）。
--
-- # 为什么是滑动窗口（每请求一行）
--
-- 固定窗口（`window_start` 对齐 + 计数列）在**窗口边界**会出现**双倍突发**：
-- 窗口 A 的最后 1 毫秒打满配额、窗口 B 的第 1 毫秒又打满
-- → 实际瞬时速率是配额的 **2 倍**。滑动窗口没有这个问题。
--
-- 代价：**每请求一行** → 必须配清理任务（`xtask observability prune-rate-limit`）。
-- ⚠️ 没有清理会在几周内变成性能瓶颈 —— 这不是可选项。
--
-- ✅ 该命令已实现：`crates/xtask/src/observability.rs`。
--    保留期 = 传入的最大窗口 × 2（默认 60 → 保留 120 秒）。
--    ⚠️ 部署侧必须挂 cron / systemd timer 定时调它，否则表仍会无限增长。
--    ⚠️ 列「哪些租户有行」要绕过 RLS → 用**管理员**连接串；
--       用应用角色跑只会看到 0 个租户（命令会打印提示）。
--
-- # ⚠️ RLS policy **不在本迁移里手写**（同 D227）
--
-- 策略由 `cargo xtask tenant-policies` 从
-- `crates/platform-observability/manifest.json` 的 `tenantScoped` 段生成 ——
-- 手写 policy 迟早漏一张，而漏一张 = 该表**完全无隔离**。
--
-- # ⚠️ 为什么 id 用 BIGSERIAL 而不是 UUID
--
-- 这张表的行数是**全库最多**的（每请求一行）。UUID 是 16 字节，
-- BIGSERIAL 是 8 字节且有自然的插入局部性 → 索引更小、写更快。
-- 主键在这里**没有跨系统传播的语义**（不像 object/app 的 id），所以不需要 UUID。

CREATE TABLE IF NOT EXISTS platform_rate_limit (
    -- 自增主键（见上方说明）。
    id          BIGSERIAL   PRIMARY KEY,
    -- 租户（RLS 维度）。⚠️ 无租户的请求写不进这里（见 TD-072）。
    tenant_id   TEXT        NOT NULL,
    -- 维度：`tenant` / `user` / `endpoint`（见 D337）。
    dimension   TEXT        NOT NULL,
    -- 桶键（形如 `rl:endpoint:GET:/api/rbac/roles`）。
    key         TEXT        NOT NULL,
    -- 请求时刻；滑动窗口按它算。
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ⚠️ **唯一索引，列顺序必须与查询完全一致**
--    查询是 `WHERE tenant_id = ? AND dimension = ? AND key = ? AND occurred_at > ?`
--    → 索引 `(tenant_id, dimension, key, occurred_at DESC)` 能一路走索引到范围扫。
--    顺序错了会退化成全表扫（这张表行数最多，退化的代价最大）。
CREATE INDEX IF NOT EXISTS platform_rate_limit_lookup_idx
    ON platform_rate_limit (tenant_id, dimension, key, occurred_at DESC);

-- 清理任务按 `occurred_at` 扫（不带 tenant/dimension/key 前缀）
-- → 需要一个**独立的** occurred_at 索引，否则清理会全表扫。
CREATE INDEX IF NOT EXISTS platform_rate_limit_occurred_at_idx
    ON platform_rate_limit (occurred_at);

-- ---------------------------------------------------------------------------
-- 应用角色授权（**新表必须显式 GRANT**，不会自动继承）
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_rate_limit TO base_platform_app';
        -- BIGSERIAL 的序列也要授权，否则 INSERT 拿不到 nextval。
        EXECUTE 'GRANT USAGE, SELECT ON SEQUENCE platform_rate_limit_id_seq TO base_platform_app';
    END IF;
END
$$;
