-- REQ-018 导入的**目标实体表** `platform_storage_probe_row`（第 5 张租户表，D333）。
--
-- # 为什么需要这张表
--
-- AC-S-180 步骤 5 要求「目标表里**恰好 49990 行**（合法行全入库）」，
-- AC-S-181 步骤 4 要求「只有 3 行入库（第 1、2、4 行）」——
-- 这两条判据的前提是**存在一个可以写入的目标表**。
--
-- 实现复核（F-01）发现：`worker.rs` 的合法行分支是空操作，
-- 且 `probe_rows` 这张表在迁移里**根本不存在** → 两条判据都没实现。
--
-- # 为什么是「通用探针表」而不是业务表
--
-- 本需求的导入/导出是**通道能力**，具体导到哪张业务表属于各业务域的事。
-- 本轮提供一个**列固定的探针实体** `probe_row`，用来：
--
-- 1. 证明「CSV → 解析 → 校验 → 落库」这条链路**真的通**；
-- 2. 给 AC-S-180 / AC-S-181 一个可断言的落点。
--
-- # ⚠️ 列名为什么是 email / amount / note
--
-- AC-S-181 明确要求错误回执里的列名是 **`email`** 与 **`amount`**
-- （「列名必须是真实的列名，不是 `column_2` 这种下标」）。
-- 规格里 AC-S-180 写的 `objectKey,sizeBytes` 与 AC-S-181 的 `email,amount`
-- 是**两套列**——这是规格自相矛盾，已由 D333 统一为**下面这一套**，
-- AC-S-180 的表头同步改为 `email,amount,note`。
--
-- # ⚠️ RLS policy **不在本迁移里手写**（同 D227）
--
-- 策略由 `cargo xtask tenant-policies` 从 `crates/platform-storage/manifest.json`
-- 的 `tenantScoped` 段生成。本表**必须**加进 `tenantScoped.tables`，
-- 否则 `tenant-policies --check` 会绿（没声明就不生成也不校验）→ 该表完全无隔离。

CREATE TABLE IF NOT EXISTS platform_storage_probe_row (
    -- 行 ID；导入时由服务端生成（CSV 里没有主键列）。
    id         UUID        PRIMARY KEY,
    tenant_id  TEXT        NOT NULL,
    -- 导入任务的 ID；同一份 CSV 重跑时按它区分批次（**不是外键**，同 job_error）。
    job_id     UUID        NOT NULL,
    -- ⚠️ `email` 在 AC-S-181 里是**必填列**：缺了要产生行级回执（列名 "email"）。
    email      TEXT        NOT NULL,
    -- ⚠️ `amount` 在 AC-S-181 里是**数字列**：写成非数字要产生行级回执（列名 "amount"）。
    amount     BIGINT      NOT NULL,
    -- 可选列：不参与必填校验，用来承载「含逗号 / 含换行」这类 round-trip 字段。
    note       TEXT        NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- ⚠️ 幂等键：同一任务里同一个 email 只占一行。
    -- 断点续跑被 `Partial` 退回后重跑时，`ON CONFLICT DO NOTHING` 靠它防重复翻倍；
    -- 没有它的话「部分成功 + 重试」会让行数变成 2 倍，而 AC-S-180 步骤 5 断言的是**恰好** 49990。
    CONSTRAINT platform_storage_probe_row_job_email_key UNIQUE (tenant_id, job_id, email)
);

-- AC-S-180 步骤 5 / AC-S-181 步骤 4 都要按「租户」数行；导入进度排查按 job 数。
CREATE INDEX IF NOT EXISTS platform_storage_probe_row_tenant_idx
    ON platform_storage_probe_row (tenant_id);

CREATE INDEX IF NOT EXISTS platform_storage_probe_row_tenant_job_idx
    ON platform_storage_probe_row (tenant_id, job_id);

-- ---------------------------------------------------------------------------
-- 应用角色授权（**新表必须显式 GRANT**，同 storage_schema 迁移）
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_storage_probe_row TO base_platform_app';
    END IF;
END
$$;
