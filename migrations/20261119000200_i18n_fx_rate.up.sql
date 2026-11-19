-- 汇率快照表 `platform_fx_rate`（REQ-021 §5.3 / AC-S-097）。
--
-- # 为什么快照是「一行一个时刻」而不是「一行一个币种对」
--
-- 换算必须**可追溯**：同一笔 100 CNY 在 2026-01-01 与 2026-06-01 的汇率下结果不同，
-- 调用方要能反查「用的是哪一条汇率」。只存「最新汇率」的话，历史单据无法重算，
-- 且并发下同一个查询会因为「刚好有人写了新汇率」而给出两个答案（design R4）。
--
-- 所以主键语义是 `(from_currency, to_currency, as_of)`：
-- **as_of 是快照标识**，换算时由调用方显式传入，**不隐式取最新**。
--
-- # 为什么汇率是整数
--
-- `rate_micros = 汇率 × 10^6`（如 1 CNY = 0.1395 USD → 139500）。
-- 浮点存不下精确的十进制汇率；换算走 `platform_core::money::convert_minor_units`
-- 的 `i128` 中间量 + 银行家舍入（裁定 #12）。
--
-- # 没有 RLS / 没有 tenant_id（**故意的**）
--
-- 汇率是**平台级参考数据**，不是租户数据：租户 A 与租户 B 必须看到同一份汇率
-- （否则跨租户报表口径不一致）。所以本表与 `platform_config` 同类 ——
-- 无 `tenant_id`、不开 RLS；应用层用伪租户 `"platform"` 访问
-- （`{tenant} IS NOT NULL` 恒真），与 `ConfigStore` 同一模式。
--
-- ⚠️ 不把本表登记进任何 manifest 的 `tenantScoped.tables` —— 那会让平台给它
-- 生成 RLS 策略，`current_setting('app.tenant_id')` 与无 tenant_id 的列对不上。
--
-- # 写入通道
--
-- **本轮不提供 HTTP 写接口**（D115）：管理端需求尚未开工，硬做就要连带做
-- 权限点三件套（manifest `permissions[]` + 种子迁移 + 启动对账，D27）。
-- 写入只走 store API（`platform_db::fx::FxRateStore::insert_rate`），
-- 测试自己播种（D101 独立幂等），生产靠 SQL / 后续迁移。
--
-- 幂等：可重复执行（`CREATE TABLE IF NOT EXISTS` + 唯一约束 + `ON CONFLICT DO UPDATE`）。
-- 可逆：配套 20261119000200_i18n_fx_rate.down.sql（DROP TABLE）。

CREATE TABLE IF NOT EXISTS platform_fx_rate (
    id            BIGSERIAL   PRIMARY KEY,
    from_currency CHAR(3)     NOT NULL,
    to_currency   CHAR(3)     NOT NULL,
    -- 汇率 × 10^6。必须为正：0 / 负数不是汇率，是数据错误。
    rate_micros   BIGINT      NOT NULL CHECK (rate_micros > 0),
    -- 快照生效时刻（UTC，契约 C13）。与币种对一起构成快照标识。
    as_of         TIMESTAMPTZ NOT NULL,
    -- 来源（`manual` / `ecb` …），用于可追溯（AC-S-097 步骤 4）。
    source        TEXT        NOT NULL DEFAULT 'manual',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT platform_fx_rate_pair_as_of_key UNIQUE (from_currency, to_currency, as_of)
);

COMMENT ON TABLE platform_fx_rate IS '汇率快照（平台级参考数据，无 tenant_id / 无 RLS；as_of 是快照标识，换算时显式传入）';
COMMENT ON COLUMN platform_fx_rate.rate_micros IS '汇率 × 10^6 的整数（如 0.1395 → 139500），避免浮点';
COMMENT ON COLUMN platform_fx_rate.as_of IS '快照生效时刻（UTC）；与 (from,to) 一起唯一确定一条快照';
COMMENT ON COLUMN platform_fx_rate.source IS '汇率来源（manual / ecb / …），换算结果会带出去以支持反查';

-- 换算查询的形状就是 `(from, to, as_of)`，索引按同一顺序建。
CREATE INDEX IF NOT EXISTS platform_fx_rate_pair_as_of_idx
    ON platform_fx_rate (from_currency, to_currency, as_of DESC);

-- 应用角色授权（组不存在时跳过 —— 生产角色由部署流程创建）。
--
-- 本表需要读 + 写（store API 由业务侧调用），**不需要** DELETE
-- （快照是审计凭证，删掉就不可追溯）。
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT SELECT, INSERT, UPDATE ON platform_fx_rate TO base_platform_app';
    END IF;
END
$$;
