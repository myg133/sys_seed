-- 请假单表（REQ-020 D348 的实体）。
--
-- ⚠️ **这里只建表，不建 RLS policy**：租户作用域表由**平台统一生成** policy
-- （manifest 的 `tenantScoped` 段 → `cargo xtask tenant-policies`）。
-- 模块手写 policy 迟早会漏 —— 漏一张表 = 该表完全无隔离。
--
-- 主键用 (tenant_id, id)：单列主键会让「别的租户已经用过这个 id」
-- 通过唯一约束冲突泄露出来（报错本身也是信息）。
--
-- `status` 是**闭集 4 个**（AC-S-189 的 N1）：`CHECK` 与 Rust 的 `Status` 枚举
-- 两层关死，不存在「第五个值」被写进去的路径。
--
-- 可逆：配套 20261123000100_leave_request.down.sql。

CREATE TABLE IF NOT EXISTS leave_request (
    tenant_id    TEXT        NOT NULL,
    id           TEXT        NOT NULL,
    applicant_id TEXT        NOT NULL,
    -- 起止日期：DATE（不是文本）——「日期」不该带时区语义。
    starts_on    DATE        NOT NULL,
    ends_on      DATE        NOT NULL,
    reason       TEXT        NOT NULL,
    status       TEXT        NOT NULL
        CHECK (status IN ('draft', 'pending', 'approved', 'rejected')),
    -- 审批人 / 审批时间：未审批时为 NULL（不是空串 —— 空串会伪装成「有人审批过」）。
    decided_by   TEXT,
    decided_at   TIMESTAMPTZ,
    -- 契约 C13：时间一律 UTC 存 timestamptz。
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id)
);

-- 列表按创建时间倒序（一条索引覆盖租户 + 排序）。
CREATE INDEX IF NOT EXISTS leave_request_created_idx
    ON leave_request (tenant_id, created_at DESC);

COMMENT ON TABLE leave_request IS '请假单（REQ-020 业务消费方；状态机在业务层，RLS 由平台生成）';
COMMENT ON COLUMN leave_request.status IS '闭集：draft / pending / approved / rejected';
