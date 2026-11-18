-- 数据范围示例表（REQ-009）。
--
-- **为什么要有这张表**：`sandbox_tenant_item` 只有 `title` / `revision`，
-- **没法表达数据范围** —— 数据范围要回答「这条记录归属哪个组织节点、是谁创建的」。
--
-- 三列是数据范围语义的全部输入：
--
-- | 列 | 用途 |
-- |----|------|
-- | `org_node_id` | `OWN_ORG` / `OWN_ORG_AND_BELOW` 的归属节点 |
-- | `created_by` | `SELF` 的归属人 |
-- | `amount` | 属性级校验（`$record.amount <= 10000`）的取值 |
--
-- ⚠️ **这里只建表，不建 RLS policy**：租户作用域表由**平台统一生成** policy
-- （`modules/sandbox/manifest.json` 的 `tenantScoped` 段 → `cargo xtask tenant-policies`）。
--
-- 可逆：配套 20261118000900_sandbox_scope_item.down.sql。

CREATE TABLE IF NOT EXISTS sandbox_scope_item (
    tenant_id   TEXT NOT NULL,
    id          TEXT NOT NULL,
    title       TEXT NOT NULL,
    -- 金额（最小单位整数，契约 C13）。
    amount      BIGINT NOT NULL DEFAULT 0,
    -- 归属组织节点（数据范围的判定依据）。
    org_node_id TEXT NOT NULL,
    -- 创建人（`SELF` 范围的判定依据）。
    created_by  TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id)
);

CREATE INDEX IF NOT EXISTS sandbox_scope_item_org_idx
    ON sandbox_scope_item (tenant_id, org_node_id);

CREATE INDEX IF NOT EXISTS sandbox_scope_item_creator_idx
    ON sandbox_scope_item (tenant_id, created_by);

COMMENT ON TABLE sandbox_scope_item IS '数据范围示例（组织归属 + 创建人 + 金额）';
